//! 游戏本体的 [`AppHandler`] 实现：把 [`crate::content`]/[`crate::world`]/
//! [`crate::save`] 接到窗口事件循环上——启动 → 装载内容 → 建世界/读档
//! 已经在 [`crate::run_game`] 完成，本模块只负责「每帧输入 → 世界推进 →
//! 渲染」与「退出前存档」。
//!
//! 渲染管线（图集加载、精灵批、相机、FOV 裁剪可见格）直接复用
//! `ll-render` 已经交付的部件，取舍与 `ll-sim` 的
//! `p5_coordinate_acceptance` 完全一致（同一批零件，包括玩家精灵的
//! 行走/待机动画状态机——见 [`crate::animation`] 模块文档「这是『声明
//! 了但从没接线』的第十处修复」），差异只在本模块更薄——不做 Interior
//! 出入、不画小地图（规格 §15 把这类打磨排在 P7，见任务顶层说明「不是
//! 做 UI 项目」），聚焦「能玩、能存」这条最小闭环本身。

use std::path::PathBuf;
use std::sync::Arc;

use ll_i18n::Catalog;
use ll_mod::asset_vfs::AssetVfs;
use ll_mod::script_event_source::ScriptEventSource;
use ll_platform::config::DisplayConfig;
use ll_platform::config::ScaleFilter;
use ll_platform::fps::FpsCounter;
use ll_platform::input::{GameKey, InputState};
use ll_platform::window::{AppHandler, FrameId, FrameOutcome, PhysicalSize, Window};
use ll_render::anim::{AnimStateMachine, current_sprite_name};
use ll_render::atlas::{Atlas, AtlasEntry};
use ll_render::atlas_pack::{SpriteSource, pack_atlas};
use ll_render::batch::{SpriteBatch, SpriteInstance};
use ll_render::camera::{Camera, Zoom, apply_zoom};
use ll_render::gpu::GpuContext;
use ll_render::sprite::{DrawOrder, Layer, footprint_bottom_screen_y, sprite_draw_position};
use ll_render::target::{BlitFilter, RenderTarget, fit_viewport};
use ll_render::wgpu;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::turn::TurnEngine;
use ll_text::TextRenderer;
use ll_ui::hud::character_panel::CharacterPanelData;
use ll_ui::hud::render::render_hud;
use ll_ui::hud::status_bar::StatusBarData;
use ll_ui::hud::world_map::WorldMapPanelData;
use ll_ui::widget::quad::QuadRenderer;
use ll_ui::widget::skin::NineSliceSkin;
use ll_ui::widget::state::WidgetStateTable;
use ll_ui::widget::textured_quad::TexturedQuadRenderer;
use ll_world::entity::EntityId;
use ll_world::fov::compute_fov;
use ll_world::overview::{ContinentField, continent_map, generate_continent_field};
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::surface_store::SurfaceWindow;
use ll_world::weather::Weather;

use crate::animation::{self, FALLBACK_SPRITE};
use crate::content::{LoadedContent, RuntimeCatalogs};
use crate::layout::{
    effective_sight_radius, effective_sight_radius_for_race, effective_tint, terrain_atlas_key,
    tile_tint,
};
use crate::save::save_game;
use crate::world::{GameWorld, MAX_SAFE_ZOOM, MIN_SAFE_ZOOM, STREAM_RADIUS_ZONES};

/// 本体二进制目前唯一的实体是玩家——`crate::world::spawn_player` 是整个
/// `ll-game` 里唯一一处 `world.actors.spawn` 调用（已 grep 核实），
/// 传给 [`ll_sim::turn::TurnEngine::advance_ai`] 的 `ai_intent` 参数
/// 因此恒不会被调用（时间轴里除了玩家没有别的实体会被弹出）。
/// 恒返回 `Intent::Wait`——即使真被调用到，也不会产出任何空效果导致
/// `ll_sim::turn` 模块文档「必须保证进展」一节描述的死循环
/// （`Intent::Wait` 恒产出 `Effect::ScheduleNext`，见 `ll_sim::resolve`
/// 文档）。
///
/// # 为什么这里**还不是** `ScriptBehaviorSource`
///
/// 如实记录，不是遗漏。`advance_ai` 的 `ai_intent` 原本是 `fn` 指针，
/// 捕获不进任何需要 `&mut self` 的决策来源——这条**类型层面的**阻塞
/// 已经在本批次解除（签名放宽成 `&mut dyn FnMut`，标准接法见
/// `ll_sim::behavior::behavior_ai_intent`）。剩下的两条阻塞是内容层面
/// 的，不是接线层面的，各自都需要独立批次：
///
/// 1. **本体二进制没有任何 NPC 生成路径**——没有生物注册表、没有刷怪
///    表，`build_new_world` 只生成玩家一个实体。哪怕这里换成真正的
///    行为树决策来源，`advance_ai` 也永远弹不出一个非受控实体来调用
///    它，「接上了但恒不执行」与现状没有任何可观察差别。
/// 2. **没有「哪个生物用哪棵行为树」的内容绑定**——
///    `ScriptBehaviorSource::new` 要一份脚本源码与一个入口函数名，而
///    `mods/example_mod/behavior.scm` 刻意不在 `entry_points` 里（见
///    该文件头注释），`LoadedContent::script_sources` 因此根本读不到
///    它。在没有绑定内容类型的前提下由 Rust 侧硬选一棵树，就是把
///    「本体 = 框架，脚本 = 内容」这条裁定反过来写。
///
/// 在这两条补上之前，这里挂一个恒 `Wait` 的占位比挂一个恒不执行的
/// `NoBehavior` 更诚实：后者会让读者以为行为树已经接通了。
/// 行为树经由 `TurnEngine` 真实驱动结算这条链路本身已经有可执行证据，
/// 见 `crates/ll-mod/tests/example_mod_stealth.rs`。
fn no_npc_ai(_world: &WorldState, actor: EntityId, _player: EntityId) -> Intent {
    Intent::Wait { actor }
}

/// 每次「放大/缩小」动作激活时，缩放倍率的调整步长。
///
/// 取一个小到不会让画面一步跳变太多、又大到几次按键/滚动就能感受到
/// 明显差异的值——纯粹的手感取舍，不影响正确性，任意正数都不会破坏
/// `Zoom::new`/`MIN_SAFE_ZOOM`/`MAX_SAFE_ZOOM` 的钳制。
const ZOOM_STEP: f32 = 0.1;

/// 玩家标记在绘制顺序里固定的实体号。
const PLAYER_ENTITY: u64 = 0;
/// 地形瓦片绘制顺序号的起始偏移。
const TERRAIN_ENTITY_BASE: u64 = 1;

/// 世界地图（M 键切换）按区块下采样的倍率——喂给
/// `ll_world::overview::continent_map` 的 `downsample` 参数。世界默认
/// 64×48 个区块（见 `crate::world` 的 `ZONE_COUNT`），downsample=2 时
/// 世界地图是 32×24=768 格，比 1:1 的 3072 格更适合铺进一块屏幕大小的
/// 面板（单格不至于小到看不清是什么地形），又远没有稀疏到丢失大陆
/// 轮廓——纯粹的表现层取舍，不影响 `continent_map` 本身「不触发按需
/// 生成」这条只读保证（见其文档）。
const WORLD_MAP_DOWNSAMPLE: u32 = 2;

/// 把资产 VFS 已经解析完覆盖规则的精灵声明，逐个从磁盘读出图片字节，
/// 转换成 `ll_render::atlas_pack::pack_atlas` 需要的输入。
///
/// 单个精灵文件读不到（mod 作者声明的路径写错、文件被误删）只跳过
/// 那一条并记警告日志，不让整个图集打包失败——`pack_atlas` 自己对
/// 「读到了字节但解码失败」这一层已经做了同样的降级（见其模块文档
/// 「打包失败必须优雅」一节），这里补的是更前一步「连字节都读不到」
/// 的同一条降级路径。
fn load_sprite_sources(vfs: &AssetVfs) -> Vec<SpriteSource> {
    vfs.sprites
        .iter()
        .filter_map(|sprite| match std::fs::read(&sprite.source_file) {
            Ok(image_bytes) => Some(SpriteSource {
                name: sprite.atlas_name.clone(),
                image_bytes,
                pivot: ll_render::sprite::Pivot {
                    x: sprite.pivot.x,
                    y: sprite.pivot.y,
                },
                footprint: ll_render::sprite::Footprint {
                    width: sprite.footprint.width,
                    height: sprite.footprint.height,
                },
            }),
            Err(error) => {
                tracing::warn!(
                    name = %sprite.atlas_name,
                    path = %sprite.source_file.display(),
                    %error,
                    "精灵源文件读取失败，已跳过并降级"
                );
                None
            }
        })
        .collect()
}

/// 存活于 `on_resume` 之后的 GPU 相关资源——不能在 `Demo::new` 阶段
/// 就创建：窗口句柄要等 `on_resume` 才可用。
struct GpuResources {
    gpu: GpuContext,
    render_target: RenderTarget,
    atlas: Atlas,
    batch: SpriteBatch,
    window_size: PhysicalSize<u32>,
    /// 离屏画面放大到窗口时的采样滤波方式，来自
    /// [`crate::run_game`] 装载的 [`DisplayConfig::scale_filter`]，
    /// 只读一份贯穿整个运行期——切换滤波方式需要重启（P7 之前没有
    /// 设置界面，见规格 §15），不是本体二进制现在要支持的场景。
    blit_filter: BlitFilter,
    /// HUD 文本渲染器（P7 第一批：只读观测界面）——与世界层
    /// `render_target`/`batch` 是完全独立的第二条渲染通道，直接画在
    /// 窗口 surface 的原生分辨率上，见 `ll_text` crate 顶层文档「两条
    /// 渲染通道」一节与 `ll_ui::widget::quad` 模块文档。
    text_renderer: TextRenderer,
    /// HUD 面板背景/经验条渲染器——与 `text_renderer` 同一条通道，见
    /// `ll_ui::widget::quad::QuadRenderer` 文档。
    quad_renderer: QuadRenderer,
    /// HUD 真实贴图（九宫格边框/条形）渲染器——采样与 `batch` 同一份
    /// `atlas`，见 `ll_ui::widget::textured_quad::TexturedQuadRenderer`
    /// 文档「与 SpriteBatch 的关系」一节。
    textured_quad_renderer: TexturedQuadRenderer,
    /// HUD 皮肤——引用 `atlas` 里 `ll-artgen` 生成的占位 UI 贴图,见
    /// `ll_ui::widget::skin::NineSliceSkin` 文档。构造好之后不依赖任何
    /// 运行期状态,贯穿整个会话复用同一份。
    skin: NineSliceSkin,
}

impl GpuResources {
    /// `asset_vfs` 是本次装载会话已经解析完覆盖规则的资产清单
    /// （[`LoadedContent::asset_vfs`]）——图集不再是编译期
    /// `include_bytes!` 烧死的单一 PNG，而是运行期从本体与全部 mod
    /// 的松散贴图现场打包，见 `ll_render::atlas_pack` 模块文档「为什么
    /// 本体资产也要走这条路径」一节。
    fn new(
        window: Arc<Window>,
        size: PhysicalSize<u32>,
        display: DisplayConfig,
        asset_vfs: &AssetVfs,
    ) -> GpuResources {
        let gpu =
            GpuContext::new(window, size, display.vsync).expect("运行环境应能取得可用的图形适配器");
        let render_target = RenderTarget::new(&gpu);
        let sources = load_sprite_sources(asset_vfs);
        tracing::info!(sprite_count = sources.len(), "开始运行期图集打包");
        let packed = pack_atlas(&sources);
        let atlas = Atlas::from_rgba(&gpu, packed.metadata, packed.canvas)
            .expect("运行期打包的图集画布应能上传为 GPU 纹理");
        let batch = SpriteBatch::new(&gpu, &atlas, render_target.format());
        // HUD 两条子通道都画在窗口 surface 的原生分辨率上（不是
        // `render_target` 的 640×360 逻辑分辨率），格式必须是
        // `gpu.surface_format()`，不是 `render_target.format()`——
        // 两者当前多数环境下相同，但语义上前者才是「最终真正呈现的
        // 那张纹理」的格式，见 `ll_text::TextRenderer::new` 文档。
        let text_renderer = TextRenderer::new(gpu.device(), gpu.queue(), gpu.surface_format())
            .expect("内置字体资产应能正常装载");
        let quad_renderer = QuadRenderer::new(gpu.device(), gpu.queue(), gpu.surface_format());
        // 与 `text_renderer`/`quad_renderer` 同一条「原生分辨率、blit
        // 之后」通道,但采样的是 `atlas`——`NineSliceSkin::new` 在这里
        // 一次性查出全部需要的贴图 UV,之后每帧只是克隆已经查好的数据,
        // 不重复查图集。
        let textured_quad_renderer =
            TexturedQuadRenderer::new(gpu.device(), gpu.queue(), gpu.surface_format(), &atlas);
        let skin = NineSliceSkin::new(&atlas);
        GpuResources {
            gpu,
            render_target,
            atlas,
            batch,
            window_size: size,
            blit_filter: match display.scale_filter {
                ScaleFilter::Nearest => BlitFilter::Nearest,
                ScaleFilter::SharpBilinear => BlitFilter::SharpBilinear,
            },
            text_renderer,
            quad_renderer,
            textured_quad_renderer,
            skin,
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.window_size = size;
    }

    /// 取得本帧窗口 surface 纹理并把世界层（离屏 `render_target`）
    /// blit 上去——不在这一步就 `present`，留出空档让调用方在
    /// [`Demo::on_frame`] 里追加 HUD 这第二条渲染通道（[`draw_hud`]），
    /// 再调用 [`Self::present_frame`] 真正提交。取不到可用 surface 帧时
    /// 返回 `None`，本帧直接跳过呈现（既有降级行为，只是从「一步做完」
    /// 拆成了两步）。
    fn acquire_and_blit(&self) -> Option<(wgpu::SurfaceTexture, wgpu::TextureView)> {
        let frame = match self.gpu.acquire_frame() {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, "跳过本帧的窗口呈现");
                return None;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let viewport = fit_viewport(self.window_size.width, self.window_size.height);
        self.render_target
            .blit_to(&self.gpu, &view, viewport, self.blit_filter);
        Some((frame, view))
    }

    /// 真正把已经画好（世界层 + HUD）的 `frame` 提交呈现。
    fn present_frame(&self, frame: wgpu::SurfaceTexture) {
        self.gpu.queue().present(frame);
    }

    fn lookup<'a>(&'a self, name: &str) -> Option<(&'a AtlasEntry, [f32; 4])> {
        let entry = self.atlas.metadata().lookup(name);
        let uv = self.atlas.uv_rect(name);
        match (entry, uv) {
            (Some(entry), Some(uv)) => Some((entry, uv)),
            _ => {
                tracing::error!(name, "图集条目缺失，跳过本次绘制");
                None
            }
        }
    }
}

/// 游戏本体的完整运行期状态。
pub struct Demo {
    content: LoadedContent,
    game_world: GameWorld,
    camera: Camera,
    /// 当前画面缩放倍率——ADR 0020 甲区（渲染层浮点，结果只变成
    /// 像素，见 `ll_render::camera::Zoom` 文档），钳制在
    /// `[MIN_SAFE_ZOOM, MAX_SAFE_ZOOM]`（不是 `Zoom` 的通用上下限，
    /// 那两个常量的推导见 `crate::world` 模块文档「常驻区块集合完全
    /// 解耦」——本字段绝不进 `GameWorld`/`WorldState`，只是 `Demo`
    /// 自己的运行期渲染状态。
    zoom: Zoom,
    save_path: PathBuf,
    character_name: String,
    /// 垂直同步与缩放滤波偏好，`on_resume` 建 [`GpuResources`] 时需要，
    /// 但 `resources` 要等窗口就绪才能创建（见该字段文档），因此本
    /// 结构体自己先存一份。
    display: DisplayConfig,
    /// 玩家行走剪辑在 `content.clip_table` 里的下标——装载期由
    /// [`ll_mod::base_clip::register_base_clips`] 分配，见
    /// `LoadedContent::clip_ids`。
    walk_clip: usize,
    /// 玩家待机剪辑在 `content.clip_table` 里的下标，理由同
    /// `walk_clip` 字段文档。
    idle_clip: usize,
    /// 玩家精灵行走/待机动画状态的生命周期管理：电平驱动
    /// （[`AnimStateMachine::set_level`]），每帧由
    /// [`animation::update_player_animation`] 算出「现在该播放哪个
    /// 状态」——与 `ll-sim` 的 `p5_coordinate_acceptance::Demo::anim`
    /// 同一套接线方式，只是本体二进制这一份是独立的运行期实例。
    anim: AnimStateMachine,
    /// 回合引擎——世界时钟推进的唯一驱动者，见 [`Demo::advance`] 文档
    /// 「世界时钟为什么会走」一节。由 [`Demo::new`] 从
    /// `game_world.timeline` 接管（[`std::mem::take`]，见其字段文档），
    /// 此后 `game_world.timeline` 恒为空，时间轴的权威副本只在本字段
    /// 里——`GameWorld` 只是「建世界/读档」这一步的搬运容器，不是本
    /// 引擎持续读写的地方。
    engine: TurnEngine,
    /// 运行期事件分发器（事件监听 API 批次）——每条效果落地之前回调
    /// 已订阅的 mod 处理函数，把它们产出的反应效果交回 `TurnEngine`
    /// 由同一个 `apply` 执行，见 `ll_mod::script_event_source` 模块
    /// 文档。
    ///
    /// `None` 表示**一条订阅都没有**：这时连引擎都不建（见
    /// `crate::run_game` 里的接线），事件分发在结算路径上退化成一次
    /// `Option::is_none` 判断，与本批次之前逐字等价。这是那条「没人
    /// 订阅就一分钱都不花」承诺的最外层落点。
    event_source: Option<ScriptEventSource>,
    resources: Option<GpuResources>,
    /// 本地化目录（P7 第一批：只读观测 HUD）——状态栏/角色面板/背包/
    /// 装备栏的全部标签、属性名、槽位名、物品名都经它解析，见
    /// `ll_ui::hud` 模块文档「三、所有文本必须走 i18n」一节对应的
    /// 任务书要求。由 [`crate::run_game`] 装载后移交给本类型持有——
    /// `run_game` 已经装载过一次用于解析窗口标题，本字段是同一份
    /// `Catalog`，不重复装载第二份。
    catalog: Catalog,
    /// 当前显示语言标签（如 `"zh-CN"`），来自
    /// [`ll_platform::config::GameConfig::language`]。
    language: String,
    /// HUD 条形动画的持久状态（P7 追加：血条/经验条动画）——按控件 id
    /// 索引的旁表,见 `ll_ui::widget::state` 模块文档「为什么是旁表」
    /// 一节：结构上不可能污染 `WorldState`,只影响画面。
    hud_anim: WidgetStateTable,
    /// 世界地图（M 键切换）用的粗粒度地形场——[`Demo::new`] 建局时算
    /// 一次并长期持有，理由见
    /// `ll_world::overview::generate_continent_field` 文档「调用方应在
    /// 世界创建时调用一次并长期持有结果」一节：这份数据只依赖噪声种子
    /// 与地形表（两者建局后不再变化），每帧重新生成毫无必要。**只是
    /// `Demo` 自己的表现层缓存**，不进 `GameWorld`/`WorldState`、不参与
    /// 存档序列化——读档后的会话会在 [`Demo::new`] 里用读到的
    /// `game_world.noise`/`game_world.params` 重新生成同一份数据（种子
    /// 相同则地形场逐位相同），不需要随存档往返。
    continent_field: ContinentField,
    /// 世界地图当前是否处于打开状态——M 键（`GameKey::Map`）切换,见
    /// [`Demo::advance`] 里的开关逻辑与 `ll_ui::hud::world_map` 模块
    /// 文档。纯粹的表现层 UI 状态,同样不进 `GameWorld`/`WorldState`。
    world_map_open: bool,
    /// 状态栏帧率读数的墙钟计数器——见 `ll_platform::fps` 模块文档「为
    /// 什么用墙钟，不用帧计数」一节：只活在表现层，每帧调用一次
    /// [`FpsCounter::record_frame`]，产出的浮点数只用来拼状态栏文本。
    fps_counter: FpsCounter,
}

impl Demo {
    /// 用已经装载好的内容与已经建好（新游戏或读档得来）的世界构造
    /// 运行期状态——两者都由 [`crate::run_game`] 在事件循环启动前准备好，
    /// 本类型不负责「建世界还是读档」这个决定本身。
    // 八个参数：全部是不同类型的具名值（内容、世界、路径、名字、
    // 显示配置、本地化目录、语言标签、事件分发器），调用点只有两处
    // （`crate::run_game` 与本模块的测试帮手），编译器对每一个都做
    // 类型检查——这里没有 `register-race` 那种「13 个裸整数靠数位置」
    // 的风险。真要收拢，正确形状是把「显示配置 + 本地化目录 + 语言」
    // 三个表现层参数打包成一个类型，那是一次独立的重构，不夹带在
    // 事件监听接线里。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        content: LoadedContent,
        mut game_world: GameWorld,
        save_path: PathBuf,
        character_name: String,
        display: DisplayConfig,
        catalog: Catalog,
        language: String,
        event_source: Option<ScriptEventSource>,
    ) -> Demo {
        let player_pos = game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家刚生成或刚读档，必然存在")
            .pos;
        let camera = Camera {
            center: player_pos,
            world: game_world.world.size,
        };
        let walk_clip = content.clip_ids.hero_walk.get() as usize;
        let idle_clip = content.clip_ids.hero_idle.get() as usize;
        tracing::info!(
            clip_count = content.clip_table.as_clips().len(),
            walk_clip,
            idle_clip,
            "玩家动画状态机已装载"
        );
        // 接管时间轴——见 `Demo::engine` 字段文档：本引擎此后是时间轴
        // 唯一的权威持有者,`game_world.timeline` 留下的空值不再被读取。
        let engine = TurnEngine::new(std::mem::take(&mut game_world.timeline));

        // 世界地图的粗粒度地形场——建局/读档后只算这一次，见
        // `Demo::continent_field` 字段文档。必须在 `game_world` 被移进
        // 下方的结构体字面量之前借出 `&game_world.world.terrain.layout()`。
        let continent_field = generate_continent_field(
            game_world.world.terrain.layout(),
            &game_world.noise,
            &game_world.params,
            &content.terrain_ids,
        );
        Demo {
            content,
            game_world,
            camera,
            zoom: Zoom::default(),
            save_path,
            character_name,
            display,
            walk_clip,
            idle_clip,
            engine,
            event_source,
            anim: AnimStateMachine::new(idle_clip, FrameId(0)),
            resources: None,
            catalog,
            language,
            hud_anim: WidgetStateTable::new(),
            continent_field,
            world_map_open: false,
            fps_counter: FpsCounter::new(),
        }
    }

    /// 每帧输入处理：先维护流式邻域（必须排在移动之前，见
    /// `ll_world::surface_store::SurfaceStore::stream_neighborhood`
    /// 文档），再处理缩放、动画与移动——动画判定只读 `input`（按住
    /// 状态），不依赖本帧是否真的产生了移动意图或移动是否成功（见
    /// [`animation::update_player_animation`] 文档），因此与缩放、移动
    /// 结算互不依赖，顺序先后不影响正确性，这里的排列只是让「本帧
    /// 输入」的处理顺序读起来更顺。
    ///
    /// # 世界时钟为什么会走
    ///
    /// 本方法此前直接 `intent_from_input` → `resolve` → `apply`,完全
    /// 绕开时间轴——`world.clock` 只在 `crate::world::build_new_world`
    /// 建局那一刻被赋值一次,此后再没有任何生产代码推进它。真实游玩
    /// 时,昼夜循环、buff 到期、技能冷却、地面物品老化全部靠这个会走
    /// 的时钟,而它从未走过,是本项目当时最严重的缺陷。
    ///
    /// 现在改由 [`TurnEngine::advance_ai`]/[`TurnEngine::try_player_turn`]
    /// 驱动：先结算排在玩家之前的非受控实体回合（本体二进制目前没有
    /// NPC,这一步恒是空操作,见 [`no_npc_ai`] 文档),再尝试用本帧输入
    /// 结算玩家一次行动——`try_player_turn` 内部才会真正
    /// `world.clock = entry.at`。**这是本仓库回合制的核心手感：玩家不
    /// 行动,时间就不走**（详见 `ll_sim::timeline` 模块文档「为什么不是
    /// 『每个实体一轮』的传统回合制」与 `Intent::Wait` 的存在本身——
    /// 「等待一回合」在纯实时游戏里没有意义,只有回合制才需要一个显式
    /// 「什么都不做但仍然让时间前进」的意图）。没有按任何方向/等待键
    /// 的这一帧,`try_player_turn` 直接返回假,时钟原地不动。
    fn advance(&mut self, input: &InputState, frame: FrameId) {
        // 世界地图开关——一次性动作，`was_just_pressed` 而非
        // `was_activated`：与 `GameKey::Screenshot`/`GameKey::Menu` 同一类
        // 键（`GameKey::is_repeatable` 没有把 `Map` 收进去），长按不该
        // 反复切换。不依赖世界时钟是否前进，因此排在 `maintain_streaming`
        // 之前——地图是否打开与本帧是否真的推进了一次回合无关。
        if input.was_just_pressed(GameKey::Map) {
            self.world_map_open = !self.world_map_open;
        }
        self.maintain_streaming();
        // 地面物品老化清理（NPC 生命周期批次）——见
        // `crate::world::cleanup_aged_ground_items` 文档「为什么挂在
        // 这里」一节：与 `maintain_streaming` 并列，是当前代码库里
        // 已经存在、每帧真正跑一遍的位置。
        crate::world::cleanup_aged_ground_items(&mut self.game_world.world);
        self.update_zoom(input);
        animation::update_player_animation(
            &mut self.anim,
            input,
            frame,
            self.walk_clip,
            self.idle_clip,
        );

        let player = self.game_world.player;
        // 本帧的结算目录束——每帧现借一次，不长期持有：`RuntimeCatalogs`
        // 只借用 `self.content`（装载期产物，建局后不再变化），构造成本
        // 是几个引用的复制，不是查表，与 ADR 0016/0017 的性能分级无关
        // （它不跨脚本边界，也不进结算热路径的内层循环）。之所以是局部
        // 变量而不是 `Demo` 的字段：`RuntimeCatalogs<'a>` 借着
        // `self.content`，做成字段就是自引用结构体。
        //
        // 这一束是「天赋在真实游戏里生效」的唯一通道：本方法此前把
        // `TurnEngine` 接上了时间轴（见上文「世界时钟为什么会走」），
        // 但 `TurnEngine::perform` 当时调的是不带任何目录的 `resolve`,
        // 于是种族/职业天赋、抗性、偷袭规则、资源池容量在真正能跑的
        // 游戏里全都是死的——同一处接线缺口的第二层。
        let runtime_catalogs = RuntimeCatalogs::new(&self.content);
        let catalogs = runtime_catalogs.as_resolve_catalogs();
        // 本体二进制不渲染伤害飘字（`p3_acceptance` 才有,那是纯呈现层
        // 的验收效果,见 `ll_sim::turn` 模块文档），但 `on_effect` 这条
        // 回调**不再**是空操作：它现在是 mod 事件监听的落点。
        //
        // 没有任何订阅时 `event_source` 是 `None`，闭包退化成一次
        // `Option::is_none` 判断加一个空 `Vec`——与接线之前逐字等价，
        // 没装 mod 的玩家不为这套机制付任何代价。
        let event_source = &mut self.event_source;
        let mut on_effect = |world: &WorldState, effect: &Effect| match event_source {
            Some(source) => source.dispatch(world, effect),
            None => Vec::new(),
        };
        self.engine.advance_ai(
            &mut self.game_world.world,
            player,
            &mut no_npc_ai,
            &catalogs,
            &mut on_effect,
        );
        self.engine.try_player_turn(
            &mut self.game_world.world,
            player,
            input,
            &catalogs,
            &mut on_effect,
        );

        if let Some(agent) = self.game_world.world.actors.get(player)
            && matches!(agent.current_space, Space::Surface { .. })
        {
            self.camera.center = agent.pos;
        }
    }

    /// 按本帧激活的缩放动作调整 `self.zoom`。`was_activated` 而非
    /// `was_just_pressed`：缩放键参与自动重复（`GameKey::is_repeatable`
    /// 已把 `ZoomIn`/`ZoomOut` 收进去），长按应当连续变化；滚轮每次
    /// 滚动只调用一次 `InputState::pulse`，`was_activated` 对它同样
    /// 恰好触发一帧，两种输入源殊途同归，见 `ll-platform` 的
    /// `crate::keybind::WheelDirection` 模块文档。
    ///
    /// 钳制到 `[MIN_SAFE_ZOOM, MAX_SAFE_ZOOM]`，不是 `Zoom` 的通用
    /// 上下限——这是拉远不会让渲染剔除范围超出常驻区块集合覆盖范围的
    /// 唯一强制点，见 `crate::world::MIN_SAFE_ZOOM` 文档。
    fn update_zoom(&mut self, input: &InputState) {
        let mut value = self.zoom.get();
        if input.was_activated(GameKey::ZoomIn) {
            value += ZOOM_STEP;
        }
        if input.was_activated(GameKey::ZoomOut) {
            value -= ZOOM_STEP;
        }
        self.zoom = Zoom::new(value.clamp(MIN_SAFE_ZOOM, MAX_SAFE_ZOOM));
    }

    fn maintain_streaming(&mut self) {
        let player = self.game_world.player;
        let Some(agent) = self.game_world.world.actors.get(player) else {
            return;
        };
        if !matches!(agent.current_space, Space::Surface { .. }) {
            return;
        }
        let pos = agent.pos;
        let clock = self.game_world.world.clock;
        self.game_world.world.terrain.stream_neighborhood(
            &self.game_world.noise,
            &self.game_world.params,
            &self.content.terrain_ids,
            pos,
            STREAM_RADIUS_ZONES,
            clock,
        );
    }

    /// 退出前存档——`on_exit` 恰好调用一次（`ll_platform::window`
    /// 文档保证），是「游玩 → 存档 → 退出」这条闭环里存档动作唯一的
    /// 触发点。
    fn save_on_exit(&self) {
        match save_game(
            &self.save_path,
            &self.content,
            &self.game_world,
            &self.character_name,
            "旷野",
            ll_content::mode::SaveMode::Permadeath,
        ) {
            Ok(()) => tracing::info!(path = %self.save_path.display(), "退出前存档完成"),
            Err(error) => tracing::error!(%error, "退出前存档失败"),
        }
    }
}

/// 把地表世界画到离屏目标：地形 + 玩家标记。
///
/// # 三层可见性（战争迷雾）
///
/// 项目所有者原话：「没有视野的地方就暗下来一些，有视野的地方就没
/// 问题。而没去过的地方就黑着」。三层对应到这里的三种处理：
///
/// 1. **从未探索**——完全跳过绘制，留下 `ll_render::batch` 既有的黑色
///    清屏背景（见 `crates/ll-render/src/batch.rs` 的
///    `wgpu::LoadOp::Clear(wgpu::Color::BLACK)`），不需要本函数另画
///    一层黑色。
/// 2. **探索过、当前无视野**——照常画出该格当前的地形（地形是确定性
///    噪声，参见 `ll_world::exploration` 模块文档「只存位图，不存
///    地形副本」：记忆里的样子等价于现在重新算出的样子，没有另存
///    快照的必要），但用比当前光照更暗的记忆色调。
/// 3. **当前有视野**——按 [`effective_tint`] 正常绘制。
///
/// 三层的判定表本身是 [`crate::layout::tile_tint`]（与 GPU 无关的纯
/// 函数，见其文档），本函数只负责喂参数、按结果决定画不画。
///
/// `exploration` 读自 `world.exploration`——`ExplorationMemory` 是随
/// `WorldState` 一起持久化、参与 `hash()` 的世界状态（见
/// `ll_world::state::WorldState::exploration` 字段文档），写入路径是
/// `ll_sim::resolve::resolve_move` 在玩家移动后追加的
/// `ll_sim::effect::Effect::MarkExplored`，经 `apply` 落地——本函数只
/// 读，不写。
///
/// # 缩放与可见性是两件独立的事
///
/// `zoom` 只影响**画在哪里、画多大**（`apply_zoom` 与逐精灵尺寸乘法）
/// 与**枚举多大范围**（`visible_tiles_zoomed`），完全不影响 FOV 半径
/// `radius`——视野看得多远是玩法规则（`effective_sight_radius`
/// 读的是空间属性表与时钟，两者都不知道 `zoom` 存在），缩放只是把
/// 「已经算好可见的这批格子」画得更大或更小、连带能塞进画布的格子
/// 更少或更多，从未反过来影响「哪些格子算可见」这个判定本身。
///
/// 同理，缩放也不改变上面三层的**归属**：拉远只是让更多格子进入枚举
/// 范围，每一格属于哪一层仍由 FOV 与探索记忆决定。
/// 画出只读观测 HUD（P7 第一批）：状态栏（常驻）、角色面板、背包、
/// 装备栏——四块面板全部读玩家 `Agent` 与 `world.clock` 现算,不修改
/// 任何世界状态,见 `ll_ui::hud` 模块文档「只读，不做任何交互」一节。
///
/// 拆成自由函数而非 `Demo` 的方法，理由与 [`render_surface`] 一致：
/// 调用点需要同时持有 `&self.game_world`/`&self.content`/`&self.catalog`
/// 与 `&mut resources`，写成 `&self` 方法会让借用检查器把两者混为一谈。
///
/// 玩家实体查不到时（不应该发生——`GameWorld::player` 恒指向一个刚
/// 生成或刚读档必然存在的实体）跳过本帧 HUD 绘制并记一条警告,不
/// panic：显示层的降级纪律与 `GpuResources::lookup`「图集条目缺失，
/// 跳过本次绘制」一致，不能因为一次意外的查询落空就让整个游戏崩溃。
///
/// # 世界地图（`world_map_open`/`continent_field`）
///
/// `world_map_open` 为假时 [`ll_ui::hud::render::build_hud_frame`] 收到
/// 的是 `None`，世界地图整块不参与本帧渲染——见 `ll_ui::hud::world_map`
/// 模块文档「战争迷雾」一节与 `ll_platform::input::GameKey::Map` 文档。
/// 为真时才现算一份 [`ll_world::overview::continent_map`] 输出：这一步
/// 只读 `continent_field`（建局时算过一次，见 [`Demo::continent_field`]
/// 字段文档）与 `game_world.world.exploration`（真实探索记忆），不触发
/// 任何区块的按需生成、不修改任何世界状态——按需才算，避免地图关着的
/// 绝大多数帧白白花这份 O(区块数) 的开销。
#[allow(clippy::too_many_arguments)]
fn draw_hud(
    game_world: &GameWorld,
    content: &LoadedContent,
    catalog: &Catalog,
    language: &str,
    resources: &mut GpuResources,
    view: &wgpu::TextureView,
    hud_anim: &mut WidgetStateTable,
    frame: FrameId,
    fps: f32,
    world_map_open: bool,
    continent_field: &ContinentField,
) {
    let Some(agent) = game_world.world.actors.get(game_world.player) else {
        tracing::warn!("玩家实体查不到，本帧跳过 HUD 绘制");
        return;
    };

    // 状态栏里的天气：与 `render_surface` 各自派生一次，而不是从那边
    // 传过来。两处算出来的必然是同一个值（`Weather::derive` 是纯函数，
    // 输入只有世界种子与世界时钟，两处读的是同一个 `WorldState`），
    // 把它拎成一个跨函数参数只会在 `draw_hud` 的参数表上再加一项，换
    // 不来任何正确性——这正是「派生而不缓存」这条纪律的好处：不需要有
    // 人负责保证两处看到的天气一致。
    //
    // `weather_name_key` 必须在 `status` 之外声明：`StatusBarData` 借用
    // 它的字符串切片，与下面 `world_map_cells` 同一条既有写法。
    let weather = Weather::derive(
        game_world.world.seed,
        game_world.world.clock,
        &content.weather_table,
    );
    let weather_name_key = weather
        .kind
        .and_then(|kind| content.weather_table.display_name_key(kind))
        .map(|key| key.to_string());
    let status = StatusBarData {
        clock: game_world.world.clock,
        health: agent.health,
        mana: agent.mana,
        fps,
        weather_display_name_key: weather_name_key.as_deref(),
    };
    let character = CharacterPanelData {
        base_stats: agent.stats,
        active_stat_modifiers: &agent.active_stat_modifiers,
        equipment: &agent.equipment,
        level: agent.level,
        experience: agent.experience,
        xp_to_next_level: agent.xp_to_next_level,
        unspent_attribute_points: agent.unspent_attribute_points,
        unspent_skill_points: agent.unspent_skill_points,
        // 职业主属性倾向——查不到职业定义时是 None，面板整行不出现，
        // 见 `CharacterPanelData::primary_attribute` 文档。这是本仓库
        // 里 `ClassDef::primary_attribute` 的第一个真实消费者。
        primary_attribute: content
            .class_table
            .get(agent.profession)
            .map(|view| view.primary_attribute),
        now: game_world.world.clock,
    };

    // 见本函数文档「世界地图」一节：`world_map_cells` 声明在 `if` 之外，
    // 让 `world_map_data` 借用的数据在传给 `render_hud` 那一刻仍然存活。
    let world_map_cells;
    let world_map_data = if world_map_open {
        let layout = *game_world.world.terrain.layout();
        let zone_count = layout.zone_count();
        let cols = zone_count.width().div_ceil(WORLD_MAP_DOWNSAMPLE);
        let rows = zone_count.height().div_ceil(WORLD_MAP_DOWNSAMPLE);
        world_map_cells = continent_map(
            continent_field,
            &layout,
            &game_world.world.exploration,
            WORLD_MAP_DOWNSAMPLE,
        );
        Some(WorldMapPanelData {
            cells: &world_map_cells,
            cols,
            rows,
            terrain_ids: &content.terrain_ids,
        })
    } else {
        None
    };

    render_hud(
        &mut resources.quad_renderer,
        &mut resources.textured_quad_renderer,
        &mut resources.text_renderer,
        resources.gpu.device(),
        resources.gpu.queue(),
        view,
        resources.window_size.width,
        resources.window_size.height,
        &status,
        &character,
        &agent.inventory,
        &agent.equipment,
        &content.item_table,
        &content.item_table,
        catalog,
        language,
        &resources.skin,
        hud_anim,
        frame.0,
        world_map_data.as_ref(),
    );
}

fn render_surface(
    game_world: &GameWorld,
    content: &LoadedContent,
    camera: &Camera,
    zoom: Zoom,
    sprite_name: &str,
    resources: &mut GpuResources,
) {
    let world = &game_world.world;
    let player_agent = world.actors.get(game_world.player);
    let player_pos = player_agent.map(|agent| agent.pos).unwrap_or(camera.center);
    let profile = space_profile_of(content, world.surface_profile);
    let clock = world.clock;
    // 天气：**每帧派生一次**，随后这一帧的每一格都复用同一个值。绝不
    // 在逐格循环里再算——`Weather::derive` 要走一次加权遍历，虽然只有
    // 六条内容，放进每帧上万次的逐格循环仍然是白白浪费（ADR 0016/0017
    // 的热路径纪律）。天气不进 `WorldState`，这里是它在生产渲染路径上
    // 的唯一派生点，输入只有世界种子与世界时钟，见 `ll_world::weather`
    // 模块文档。
    let weather = Weather::derive(world.seed, clock, &content.weather_table);
    // 视野半径叠加玩家种族声明的夜间视野格数（见 `effective_sight_radius_for_race`
    // 模块文档「为什么接在这一步」一节）——玩家实体查不到（理论上不该
    // 发生，见上方 `unwrap_or(camera.center)` 同一条降级纪律）时退化到
    // 不叠加暗视的 `effective_sight_radius`，不是 panic。
    let radius = match player_agent {
        Some(agent) => effective_sight_radius_for_race(
            &profile,
            clock,
            weather,
            agent.race,
            &content.race_table,
        ),
        None => effective_sight_radius(&profile, clock, weather),
    };
    let tint = effective_tint(&profile, clock, weather);
    let layout = *world.terrain.layout();

    let visible = compute_fov(
        &SurfaceWindow::new(&world.terrain),
        &world.terrain_table,
        player_pos,
        radius,
    );

    let world_width = world.size.width() as u64;
    for pos in camera.visible_tiles_zoomed(zoom) {
        let explored = world.exploration.is_explored(&layout, pos);
        let Some(pos_tint) = tile_tint(visible.contains(pos), explored, tint) else {
            // 从未探索：不画，交给清屏背景表现「黑」，见本函数文档。
            continue;
        };
        let Some(kind) = world.terrain_at(pos) else {
            continue;
        };
        let Some(name) = terrain_atlas_key(kind, &content.terrain_ids, &content.registry) else {
            continue;
        };
        let Some((entry, uv)) = resources.lookup(&name) else {
            continue;
        };
        // 先按未缩放的相机换算拿到屏幕坐标（DrawOrder 排序用这份原始
        // 坐标即可——zoom 是围绕视口中心的单调变换，纵坐标的大小关系
        // 恒定不变，见 apply_zoom 文档），再对绘制位置单独套一次
        // apply_zoom。
        let (sx, sy) = camera.world_to_screen(pos);
        let order = DrawOrder::new(
            Layer::TERRAIN,
            sy,
            TERRAIN_ENTITY_BASE + pos.y() as u64 * world_width + pos.x() as u64,
        );
        let [zx, zy] = apply_zoom([sx as f32, sy as f32], zoom);
        resources.batch.push(
            order,
            sprite_instance(zx, zy, entry.sprite_size(), uv, pos_tint, zoom),
        );
    }

    let (px, py) = camera.world_to_screen(player_pos);
    push_player_marker(px, py, sprite_name, tint, zoom, resources);
}

/// 空间的完整 [`ll_world::space_profile::SpaceProfile`]——`Space` 本身
/// 只携带一个索引，渲染层需要的是完整字段，从装载好的空间层属性表
/// 现查现拼，不缓存（理由与 `p5_coordinate_acceptance::DemoWorld::profile_of`
/// 一致）。
fn space_profile_of(
    content: &LoadedContent,
    index: ll_core::ident::ContentIndex,
) -> ll_world::space_profile::SpaceProfile {
    ll_world::space_profile::SpaceProfile {
        id: ll_core::ident::NamespacedId::parse("lostland:runtime_profile").expect("字面量恒合法"),
        ambient_light_floor: content.space_table.ambient_light_floor(index),
        exposed_to_sky: content.space_table.exposed_to_sky(index),
        base_temperature: content.space_table.base_temperature(index),
        diggable: content.space_table.diggable(index),
        buildable: content.space_table.buildable(index),
        reverb_tag: content.space_table.reverb_tag(index),
    }
}

/// 画出玩家标记。`sprite_name` 是当前动画帧应显示的图集条目名（由
/// [`AppHandler::on_frame`] 通过 [`current_sprite_name`] 现算，缺帧时
/// 已经退回 [`FALLBACK_SPRITE`]），不再恒定画同一帧静态图——这正是
/// 「接上行走/待机动画」这条修复的落点，见 [`crate::animation`]
/// 模块文档。
fn push_player_marker(
    sx: i32,
    sy: i32,
    sprite_name: &str,
    tint: [f32; 4],
    zoom: Zoom,
    resources: &mut GpuResources,
) {
    let Some((entry, uv)) = resources.lookup(sprite_name) else {
        return;
    };
    let footprint = entry.footprint;
    // 锚点/pivot 换算全部走未缩放坐标——sprite.rs 的这套算术不知道、
    // 也不需要知道 zoom 存在（见 apply_zoom 文档「为什么是围绕视口
    // 自由函数」），缩放作为最后一步的后处理套在算出来的最终绘制
    // 位置上：这等价于把整张离屏画布围绕中心缩放，pivot 偏移量本身
    // 也会随之成比例缩放，marker 不会因为缩放而错位。
    let [px, py] = sprite_draw_position((sx, sy), footprint, entry.pivot);
    let order = DrawOrder::new(
        Layer::ENTITY,
        footprint_bottom_screen_y(sy, footprint.height),
        PLAYER_ENTITY,
    );
    let [zx, zy] = apply_zoom([px, py], zoom);
    resources.batch.push(
        order,
        sprite_instance(zx, zy, entry.sprite_size(), uv, tint, zoom),
    );
}

/// 构造一个精灵实例：`x`/`y` 应为已经套过 [`apply_zoom`] 的最终屏幕
/// 位置，`size` 是精灵未缩放的原生像素尺寸，本函数负责按 `zoom` 把
/// 它换算成绘制尺寸——位置需要「围绕中心」的特殊变换（`apply_zoom`），
/// 尺寸只是单纯的比例缩放，两者不能共用同一个变换，因此调用方必须
/// 先各自处理好再传进来，本函数只做最后的结构体组装。
fn sprite_instance(
    x: f32,
    y: f32,
    size: ll_render::sprite::SpriteSize,
    uv_rect: [f32; 4],
    color: [f32; 4],
    zoom: Zoom,
) -> SpriteInstance {
    SpriteInstance {
        position: [x, y],
        size: [
            size.width as f32 * zoom.get(),
            size.height as f32 * zoom.get(),
        ],
        uv_rect,
        color,
    }
}

impl AppHandler for Demo {
    fn on_resume(&mut self, window: Arc<Window>, size: PhysicalSize<u32>) {
        tracing::info!(width = size.width, height = size.height, "window resumed");
        self.resources = Some(GpuResources::new(
            window,
            size,
            self.display,
            &self.content.asset_vfs,
        ));
    }

    fn on_resize(&mut self, size: PhysicalSize<u32>) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.resize(size);
    }

    fn on_frame(&mut self, frame: FrameId, input: &InputState) -> FrameOutcome {
        // 墙钟采样,见 `ll_platform::fps` 模块文档「为什么用墙钟,不用
        // 帧计数」一节——`Instant::now()` 只在这一处调用,产出的浮点数
        // 只流向状态栏文本,不进 `self.game_world`/`WorldState`。
        let fps = self.fps_counter.record_frame(std::time::Instant::now());

        if input.was_just_pressed(ll_platform::input::GameKey::Cancel) {
            return FrameOutcome::Exit;
        }

        self.advance(input, frame);

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        // 当前动画帧应显示的图集条目名，两层兜底见
        // `current_sprite_name` 文档；两层都失败时（连 `FALLBACK_SPRITE`
        // 本身都缺失）才会在 `GpuResources::lookup` 里记一条错误日志，
        // 那已经是资产整体损坏，不再是「可选帧缺失」。
        let sprite_name = current_sprite_name(
            self.anim.playback(),
            self.content.clip_table.as_clips(),
            frame,
            resources.atlas.metadata(),
            FALLBACK_SPRITE,
        );

        render_surface(
            &self.game_world,
            &self.content,
            &self.camera,
            self.zoom,
            sprite_name,
            resources,
        );

        resources
            .batch
            .flush(&resources.gpu, resources.render_target.view());

        // 世界层已经 blit 到窗口 surface——HUD（状态栏/角色面板/背包/
        // 装备栏，P7 第一批）是紧接着追加的第二/三条渲染通道，画在
        // 同一张 surface 视图上，见 `GpuResources::acquire_and_blit`
        // 文档。取不到可用帧时（`acquire_and_blit` 返回 `None`）本帧
        // 直接跳过，与既有降级行为一致。
        if let Some((surface_frame, view)) = resources.acquire_and_blit() {
            draw_hud(
                &self.game_world,
                &self.content,
                &self.catalog,
                &self.language,
                resources,
                &view,
                &mut self.hud_anim,
                frame,
                fps,
                self.world_map_open,
                &self.continent_field,
            );
            resources.present_frame(surface_frame);
        }

        FrameOutcome::Continue
    }

    fn on_exit(&mut self) {
        tracing::info!("demo exiting");
        self.save_on_exit();
    }
}

#[cfg(test)]
mod tests {
    //! 世界时钟推进批次的组合断言——这是本任务真正要修的缺陷：
    //! `Demo::advance`（真实生产入口，不是手搭的测试世界）此前完全不碰
    //! `world.clock`，昼夜循环、buff 到期、技能冷却、地面物品老化全部
    //! 因此失效。下面两条测试都跑在 [`test_demo`] 建出的真实
    //! `Demo`（真实内容装载、真实 `build_new_world`、真实
    //! `TurnEngine`）上，不是直接摆弄 `WorldState`/`resolve`/`apply`——
    //! 那种写法只能证明「结算管线本身正确」，证明不了「真的接到了
    //! 玩家输入这条生产路径上」。
    //!
    //! 手工验证过这两条测试确实会红：临时把 `Demo::advance` 里
    //! `self.engine.advance_ai(...)`/`try_player_turn(...)` 两行注释掉、
    //! 换回改动前那种直接 `intent_from_input` → `resolve` → `apply`
    //! （不途经 `TurnEngine`，因此不写 `world.clock`）的写法，两条测试
    //! 都会失败：第一条因为 `clock` 全程不变，第二条因为 buff 从不到期
    //! （`derive_stats` 用的 `now` 全程等于建局时刻，恒早于 `expires_at`）。
    //! 恢复后两条都转绿。

    use super::*;
    use ll_core::ident::ContentIndex;
    use ll_core::time::Tick;
    use ll_platform::input::GameKey;
    use ll_sim::item::NoItems;
    use ll_sim::resolve::derive_stats;
    use ll_world::entity::{ActiveStatModifier, AttributeKind};

    fn test_content() -> LoadedContent {
        let dir = crate::test_support::unique_temp_path("ll-game-app-test-content");
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
        // mods_root 指向仓库真实的 mods/ 目录（本体内容住在
        // mods/lostland/，临时空目录下契约解析必然失败）；assets_root
        // 仍指向临时目录，本文件的测试不需要真实贴图。
        let content = crate::content::load_content(
            &crate::test_support::repo_mods_dir(),
            &dir.join("assets"),
        )
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
        let _ = std::fs::remove_dir_all(&dir);
        content
    }

    /// 建一个真实可用的 `Demo`——`Demo::new` 本身不触碰 GPU/窗口（那些
    /// 在 `on_resume` 才建，见 [`GpuResources`] 字段文档），因此可以
    /// 脱离真实窗口直接在单元测试里构造并调用私有的 `advance`。
    fn test_demo() -> Demo {
        let content = test_content();
        let game_world =
            crate::world::build_new_world(&content, 1).expect("测试用布局满足全部构造前置条件");
        let save_path =
            crate::test_support::unique_temp_path("ll-game-app-test-save").with_extension("llsave");
        Demo::new(
            content,
            game_world,
            save_path,
            "测试旅人".to_string(),
            DisplayConfig::default(),
            Catalog::load_dir(&std::env::temp_dir().join("ll-game-app-test-empty-locales")),
            "zh-CN".to_string(),
            // 事件分发不在本测试帮手的范围内：建它要求「全部引擎构造
            // 先于全部脚本编译」（C6），而 `test_content()` 已经在本
            // 线程上装载过 mod。真实接线在 `crate::run_game`，端到端
            // 证据在 `crates/ll-mod/tests/example_mod_events.rs`。
            None,
        )
    }

    /// 读出玩家当前结算出的力量值——途经与真实战斗结算
    /// （`ll_sim::resolve::resolve_attack`）完全相同的
    /// `ll_sim::resolve::derive_stats` 聚合入口,不是另写一套判断逻辑。
    fn player_derived_strength(demo: &Demo) -> i32 {
        let player = demo.game_world.player;
        let agent = demo
            .game_world
            .world
            .actors
            .get(player)
            .expect("玩家仍应存在");
        let now = demo.game_world.world.clock;
        derive_stats(
            agent.stats,
            &agent.active_stat_modifiers,
            &agent.equipment,
            &NoItems,
            now,
        )
        .attribute(AttributeKind::Strength)
    }

    #[test]
    fn 连续多次玩家等待后世界时钟真的前进() {
        // Arrange
        let mut demo = test_demo();
        let clock_before = demo.game_world.world.clock;
        let mut input = InputState::new();
        input.press(GameKey::Wait);

        // Act：推进三帧，每帧都带着等待键——`was_activated` 只依赖
        // `just_pressed`/`repeated` 标志位（见 `ll_platform::input`
        // 文档），本测试不调用 `begin_frame`/`end_frame`，`just_pressed`
        // 因此在整个循环里保持置位，每一帧都会被 `try_player_turn`
        // 判定为「等待键激活」并真正消费一次回合——与
        // `ll_sim::turn::tests` 里驱动 `TurnEngine` 的现成测试同一个
        // 手法（不模拟按键事件，只构造 `InputState` 的值）。
        for frame in 0..3u64 {
            demo.advance(&input, FrameId(frame));
        }

        // Assert：不是「变了」，是「前进了」——严格大于，不允许倒退。
        assert!(demo.game_world.world.clock > clock_before);
    }

    #[test]
    fn 临时属性修正过期后其加成不再计入结算() {
        // 比「时钟前进了」更强的一条：验证时钟推进与既有的惰性到期
        // 判定（`ll_sim::resolve::derive_stats`）真的咬合，不只是
        // `world.clock` 这个数字在动——单看时钟前进可能被一个「每帧
        // 直接 +1」之类的假实现骗过,这条测试还要求它与既有到期判定
        // 生效的那一刻精确对齐。
        // Arrange：跑两次等待，量出「一次行动」真实推进的 tick 数——
        // 不写死 `ll-sim` 内部私有的 `BASE_ACTION_COST`,只依赖公开可
        // 观察的时钟差值,避免测试与结算层的内部常量耦合。
        //
        // 第一次等待只是「热身」：玩家的初次可行动时刻就等于建局时的
        // `world.clock`（`crate::world::spawn_player` 把 `next_action_at`
        // 设成建局时的 `world.clock`,见其文档),`TurnEngine::perform`
        // 结算这次弹出的条目时 `world.clock = entry.at` 恰好是「设成
        // 它已经是的那个值」,不产生可观察的变化——真正能测出「一次
        // 行动的 tick 代价」要看第二次、第三次行动之间的差值。
        let mut demo = test_demo();
        let player = demo.game_world.player;
        let mut input = InputState::new();
        input.press(GameKey::Wait);
        demo.advance(&input, FrameId(0));
        let clock_after_warm_up = demo.game_world.world.clock;
        demo.advance(&input, FrameId(1));
        let clock_after_second_wait = demo.game_world.world.clock;
        let ticks_per_wait = clock_after_second_wait.0 - clock_after_warm_up.0;
        assert!(ticks_per_wait > 0, "第二次等待起，世界时钟应当真实推进");

        // 给玩家叠一条力量 +50 的临时修正，到期时刻卡在「刚才那次行动
        // 结束的时刻」与「下一次行动结束的时刻」正中间——按同一份
        // dexterity 结算出的行动代价恒定（本用例全程不改属性），下一次
        // 等待结算后世界时钟必然已经越过这个到期时刻。
        let source = ContentIndex::default();
        let expires_at = Tick(clock_after_second_wait.0 + ticks_per_wait / 2);
        {
            let agent = demo
                .game_world
                .world
                .actors
                .get_mut(player)
                .expect("玩家刚建局，必然存在");
            agent
                .active_stat_modifiers
                .entry(AttributeKind::Strength)
                .or_default()
                .insert(
                    source,
                    ActiveStatModifier {
                        delta: 50,
                        expires_at,
                    },
                );
        }
        let base_strength = demo
            .game_world
            .world
            .actors
            .get(player)
            .expect("玩家仍存在")
            .stats
            .strength;
        assert_eq!(
            player_derived_strength(&demo),
            base_strength + 50,
            "buff 生效期间应体现在结算出的力量值上——这一步只是确认
             Arrange 本身摆对了，不是本测试真正要验证的断言"
        );

        // Act：再跑一次等待（第三次）——时钟应当越过 expires_at。
        demo.advance(&input, FrameId(2));

        // Assert：buff 已过期，结算值应回落到裸属性值。
        assert_eq!(
            player_derived_strength(&demo),
            base_strength,
            "buff 过期后不应再计入结算——时钟真的推进到了 expires_at \
             之后，且推进结果真的被既有到期判定读到"
        );
    }

    #[test]
    fn 世界地图新建时默认关闭() {
        // ADR 0025 相关的验收难题第一层——程序化断言开关的初始状态,
        // 不依赖任何合成按键。
        // Arrange & Act
        let demo = test_demo();

        // Assert
        assert!(!demo.world_map_open);
    }

    #[test]
    fn 按下地图键后开关状态翻转为打开() {
        // 程序化验证「M 键事件 → 开关状态真的翻转」——见任务书「验收
        // 难题」一节要求的第一层。
        // Arrange
        let mut demo = test_demo();
        let mut input = InputState::new();
        input.press(GameKey::Map);

        // Act
        demo.advance(&input, FrameId(0));

        // Assert
        assert!(demo.world_map_open);
    }

    #[test]
    fn 再次按下地图键后开关状态翻回关闭() {
        // 与 `ll_sim::turn` 等既有测试同一个手法（见 `test_demo` 上方
        // 「连续多次玩家等待后世界时钟真的前进」测试文档）：本测试全程
        // 不调用 `begin_frame`/`end_frame`，`just_pressed` 因此在两次
        // `advance` 调用之间保持置位，每次调用都会被判定为「地图键
        // 激活」并各自触发一次翻转——恰好用来验证「开 → 关」这条翻转
        // 本身，而不是「按住不会重复翻转」（那是 `was_just_pressed` 与
        // 真实按键事件循环之间的既有职责划分，见
        // `ll_platform::input::InputState` 模块文档，不是本测试要
        // 覆盖的范围）。
        // Arrange
        let mut demo = test_demo();
        let mut input = InputState::new();
        input.press(GameKey::Map);
        demo.advance(&input, FrameId(0));
        assert!(demo.world_map_open, "第一次按下后应先翻转为打开");

        // Act
        demo.advance(&input, FrameId(1));

        // Assert
        assert!(!demo.world_map_open);
    }
}
