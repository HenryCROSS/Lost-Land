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
use ll_mod::native_behavior::{BehaviorRuleCatalogs, NativeBehaviorSource, NativeBehaviorTree};
use ll_mod::roster::SettlementRoles;
use ll_platform::config::{DisplayConfig, GameConfig, ScaleFilter};
use ll_platform::fps::FpsCounter;
use ll_platform::input::{GameKey, InputState};
use ll_platform::keybind::InputContext;
use ll_platform::keybind::KeyBindings;
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
use ll_sim::rule_modifier::{SubjectRegistry, agent_rule_modifiers, rule_modifier_displays};
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_text::TextRenderer;
use ll_ui::hud::character_panel::CharacterPanelData;
use ll_ui::hud::render::render_hud;
use ll_ui::hud::status_bar::StatusBarData;
use ll_ui::hud::world_map::{WorldMapPanelData, WorldMapSite};
use ll_ui::screen::render::render_screen;
use ll_ui::widget::quad::QuadRenderer;
use ll_ui::widget::skin::NineSliceSkin;
use ll_ui::widget::state::WidgetStateTable;
use ll_ui::widget::textured_quad::TexturedQuadRenderer;
use ll_ui::widget::ui_mode::{UiMode, UiModeStack};
use ll_world::fov::compute_fov;
use ll_world::overview::{ContinentField, generate_continent_field};
use ll_world::settlement::SettlementStatus;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::surface_store::SurfaceWindow;
use ll_world::weather::Weather;
use ll_world::world_map::{WorldMapView, world_map_slice};

use crate::animation::{self, FALLBACK_SPRITE};
use crate::content::{LoadedContent, RuntimeCatalogs};
use crate::layout::{
    effective_sight_radius, effective_sight_radius_for_race, effective_tint, terrain_atlas_key,
    tile_tint,
};
use crate::menu_screen::{
    ScreenNotice, ScreenOutcome, ScreenState, SettingsContext, menu_focus_index, screen_data,
    settings_rows, update_menu, update_settings,
};
use crate::player_action::{Feedback, PlayerCommand, PlayerMenu, player_command};
use crate::save::save_game;
use crate::settings_view::{menu_row_texts, settings_row_texts};
use crate::surface_draw::{PLAYER_ENTITY, SurfaceDraw, TERRAIN_ENTITY_BASE, surface_draws};
use crate::world::{GameWorld, MAX_SAFE_ZOOM, MIN_SAFE_ZOOM, STREAM_RADIUS_ZONES};

/// 本体二进制的 NPC 决策来源：**按职业选行为树**。
///
/// # 这里此前是什么，为什么必须改
///
/// NPC 生成批次落地时，这里硬选了**卫兵那棵树**发给全部物化出来的
/// NPC，当时的文档如实记着代价：「一整座村子的居民都会朝玩家走过来
/// ——这是分支二的直接后果，不是缺陷修不了，而是『按职业选一棵树』这
/// 条内容绑定还不存在。」
///
/// 那条内容绑定现在存在了：[`ll_mod::behavior_binding::ClassBehaviorBindings`]
/// ——`mods/lostland/classes.json5` 每条职业上的一个 `behavior` 字段，
/// 落进一张**不产生新 `ContentIndex`** 的旁表（形状照抄
/// `XpCurveBindings`）。本函数把它交给决策来源，于是：
///
/// - 卫兵 / 民兵 → 守卫型（走向视野内的人；盘查仍只有卫兵做）
/// - 据点管理者 / 农夫 / 猎户 / 屠夫 / 铁匠 / 渔夫 / 牧羊人 / 石匠
///   → 平民型（**连一次目标查询都不做**，因此不可能朝玩家走过来）
///
/// # 兜底为什么是平民，不是卫兵
///
/// 没有绑定的职业（第三方 mod 的新职业没写 `behavior`、或者实体压根
/// 没有职业）落在
/// [`NativeBehaviorSource::fallback`](ll_mod::native_behavior::NativeBehaviorSource)
/// 上。选平民那棵：**兜底应当是伤害最小的那一个**。选错成平民的代价
/// 是「这个 NPC 站着不动」，选错成卫兵的代价正是本批次要修的那个缺陷
/// ——一个没写绑定的 mod 职业会让整座村子重新朝玩家走过来。
///
/// # 哥布林那棵树在生产路径上仍然没有调用点
///
/// 野兽型原型已经接好（`behavior: "beast"` 就能绑上去），但本体内容
/// 里**没有任何一条职业绑它**——本体至今不生成怪物，`examplemod:frostbolt`
/// 那条技能也只在示例 mod 里。如实标注：这一支在生产装载下恒不成立，
/// 它的证据在 `crates/ll-mod/tests/` 那批用示例 mod 的集成测试里。
fn npc_behavior_source(content: &LoadedContent, world_seed: u64) -> NativeBehaviorSource {
    NativeBehaviorSource::new(
        NativeBehaviorTree::townsfolk(),
        BehaviorRuleCatalogs::snapshot(
            &content.race_table,
            &content.class_table,
            &content.subclass_table,
            &content.trait_table,
            &content.item_table,
        ),
        world_seed,
    )
    .with_class_bindings(content.class_behavior_bindings.clone(), &content.registry)
}

/// 每次「放大/缩小」动作激活时，缩放倍率的调整步长。
///
/// 取一个小到不会让画面一步跳变太多、又大到几次按键/滚动就能感受到
/// 明显差异的值——纯粹的手感取舍，不影响正确性，任意正数都不会破坏
/// `Zoom::new`/`MIN_SAFE_ZOOM`/`MAX_SAFE_ZOOM` 的钳制。
const ZOOM_STEP: f32 = 0.1;

/// 把资产 VFS 已经解析完覆盖规则的精灵声明，逐个从磁盘读出图片字节，
/// 转换成 `ll_render::atlas_pack::pack_atlas` 需要的输入。
///
/// 单个精灵文件读不到（mod 作者声明的路径写错、文件被误删）只跳过
/// 那一条并记警告日志，不让整个图集打包失败——`pack_atlas` 自己对
/// 「读到了字节但解码失败」这一层已经做了同样的降级（见其模块文档
/// 「打包失败必须优雅」一节），这里补的是更前一步「连字节都读不到」
/// 的同一条降级路径。
///
/// # 为什么是 `pub`
///
/// `crates/ll-game/tests/surface_render.rs`（地表内容渲染的端到端验收）
/// 要在没有 GPU 的进程里，用**与真实游戏完全同一段代码**把真实
/// `assets/` + `mods/` 打成图集，再去问「这个图集键真的对应一张画了
/// 东西的图吗」。若那条验收自己另写一份读盘逻辑，它验的就不再是生产
/// 路径——ADR 0018 要的正是「经真实内容、走真实路径」的证据。本函数
/// 本身与 GPU 无关（只读文件、拼结构体），公开它不会把任何 GPU 状态
/// 泄漏出去。
pub fn load_sprite_sources(vfs: &AssetVfs) -> Vec<SpriteSource> {
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

    /// 按 `names` 给出的优先级取**第一个真的在图集里**的条目。
    ///
    /// # 为什么不能直接对每个候选调用 [`Self::lookup`]
    ///
    /// [`Self::lookup`] 查不到时会打一条 `error!` 日志——那对「就这一个
    /// 名字，查不到就是缺图」的既有调用方是对的，但对本方法是错的：
    /// 「内容没有自带贴图，退回通用记号」是**预期内的正常路径**（绝大
    /// 多数家具与种族都不会自带图），不是缺陷。用 `lookup` 逐个试会让
    /// 每一帧、每一个 NPC 都刷一条 error 日志，日志本身随即失去信噪比。
    /// 因此前面的候选走不打日志的探测，只有最后一个候选（兜底记号）
    /// 走 [`Self::lookup`]——兜底记号缺席才真的是缺陷，那条 error 该打。
    fn lookup_first<'a, 'n>(
        &'a self,
        names: impl Iterator<Item = &'n str>,
    ) -> Option<(&'a AtlasEntry, [f32; 4])> {
        let mut names = names.peekable();
        while let Some(name) = names.next() {
            if names.peek().is_none() {
                return self.lookup(name);
            }
            if self.atlas.metadata().lookup(name).is_some()
                && let Some(uv) = self.atlas.uv_rect(name)
            {
                return self.atlas.metadata().lookup(name).map(|entry| (entry, uv));
            }
        }
        None
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
    /// 玩家配置的**唯一真相源**：键位 + 显示 + 语言 + 刻意解绑清单。
    ///
    /// 此前本结构体只留了 `display`/`language` 两份拷贝，改不动、也存
    /// 不回。设置界面要能改这三样并显式写盘，就必须整份持有——两份
    /// 拷贝在设置界面落地那一刻会立刻变成两个会漂移的真相源。
    ///
    /// **仍然不是世界状态**：`ll_platform::config` 模块文档「配置不是
    /// 世界状态」一节那条约束原样成立，本字段绝不进 `GameWorld`/
    /// `WorldState`、不参与 `hash()`、不影响确定性重放；`ll-platform`
    /// 依赖不到 `ll-world` 这条依赖方向就是结构性保证。
    config: GameConfig,
    /// 配置文件路径——设置界面按下「保存」时写到这里。
    config_path: PathBuf,
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
    resources: Option<GpuResources>,
    /// 本地化目录（P7 第一批：只读观测 HUD）——状态栏/角色面板/背包/
    /// 装备栏的全部标签、属性名、槽位名、物品名都经它解析，见
    /// `ll_ui::hud` 模块文档「三、所有文本必须走 i18n」一节对应的
    /// 任务书要求。由 [`crate::run_game`] 装载后移交给本类型持有——
    /// `run_game` 已经装载过一次用于解析窗口标题，本字段是同一份
    /// `Catalog`，不重复装载第二份。
    catalog: Catalog,
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
    /// 玩家菜单（背包 / 制作）当前的状态与光标位置——I 键与 C 键切换，
    /// 见 [`crate::player_action`] 模块文档。与 `world_map_open` 同一条
    /// 纪律：纯表现层状态，不进 `GameWorld`/`WorldState`、不进存档、
    /// 不参与回放，该模块文档「菜单状态算不算跨帧隐式状态（约束 C1）」
    /// 一节给了完整的三条判据。
    menu: PlayerMenu,
    /// 上一次玩家操作留下的反馈（`None` 表示没有话要说）——它是
    /// 「静默作废对玩家不成立」这条的落点，见
    /// `ll_sim::turn::PlayerTurnOutcome` 文档。
    ///
    /// # 为什么留到下一次操作，不做成定时淡出
    ///
    /// 定时淡出要一个墙钟或帧计数器，也就要回答「暂停时算不算」「掉帧
    /// 时补不补」这类与本批次无关的问题。留到下一次操作是更简单也更
    /// 诚实的语义：屏幕上那句话恒等于「你最近这一下按出了什么结果」，
    /// 玩家再按一次它就被换掉。
    feedback: Option<Feedback>,
    /// 世界地图当前是否处于打开状态——M 键（`GameKey::Map`）切换,见
    /// [`Demo::advance`] 里的开关逻辑与 `ll_ui::hud::world_map` 模块
    /// 文档。纯粹的表现层 UI 状态,同样不进 `GameWorld`/`WorldState`。
    world_map_open: bool,
    /// 世界地图当前的缩放档位与视野中心——所有者要的「直接对地图做一定
    /// 的缩放」落在这里，见 `ll_world::world_map::WorldMapView`。
    ///
    /// 与 `world_map_open` 同一条纪律：**纯表现层状态**，不进
    /// `GameWorld`/`WorldState`、不进存档、不参与回放。世界不因为玩家把
    /// 地图拖到哪里、放大到第几档而有任何不同。
    ///
    /// 打开地图那一刻重新对准玩家（见 [`Demo::advance`]），而不是记住上
    /// 次关掉时停在哪：玩家按 M 最常见的意图是「我现在在哪」，每次都从
    /// 自己身上开始看比恢复一个可能已经与当前位置无关的旧视野更有用。
    world_map_view: WorldMapView,
    /// NPC 决策来源——引擎自带的行为树，见 [`npc_behavior_source`] 文档。
    ///
    /// 做成字段而不是每帧现造：[`NativeBehaviorSource`] 持有一份**内容表
    /// 快照**（`BehaviorRuleCatalogs`，五张表的克隆，见其类型文档「为什么
    /// 是快照」），每帧克隆五张表是一笔白付的开销；而内容表在装载之后不再
    /// 变化，快照一次就够。
    npc_ai: NativeBehaviorSource,
    /// 据点职业名册解析结果——[`crate::world::materialize_nearby_settlements`]
    /// 每次物化都要用，同样只在建局/读档后解析一次（`SettlementRoles::resolve`
    /// 只是几次注册表查询，但它的输入——注册表——装载后不再变化）。
    settlement_roles: SettlementRoles,
    /// 状态栏帧率读数的墙钟计数器——见 `ll_platform::fps` 模块文档「为
    /// 什么用墙钟，不用帧计数」一节：只活在表现层，每帧调用一次
    /// [`FpsCounter::record_frame`]，产出的浮点数只用来拼状态栏文本。
    fps_counter: FpsCounter,
    /// 模态 UI 栈——**驱动 `InputContext` 在 `Gameplay`/`Menu` 之间切换
    /// 的那个真相源**，见 `ll_ui::widget::ui_mode` 模块文档与
    /// [`AppHandler::input_context`]。
    ///
    /// 栈非空 ⇔ 有一块模态屏盖在世界上 ⇔ 平台层按菜单表解析物理键 ⇔
    /// [`Demo::advance`] 整段早退（世界一个字节都不动）。这四件事必须
    /// 同时成立，因此它们由同一个字段决定，而不是各自留一个布尔量。
    ui_modes: UiModeStack,
    /// 模态屏当前开着哪一块（`None` = 没开）——与 `ui_modes` 是**同一
    /// 件事的两面**：栈管「输入上下文该切到哪」，本字段管「这块屏里
    /// 具体在显示什么、光标在哪」。不合并成一个：前者住在 `ll-ui`
    /// 且刻意只有 `Menu` 一个变体（见 `UiMode` 文档），后者是
    /// `ll-game` 自己的导航状态。
    screen: Option<ScreenState>,
    /// 菜单屏三条选项的焦点表——[`ll_ui::widget::focus`] 读写它。
    ///
    /// 与 `hud_anim` 同一条纪律（`ll_ui::widget::state` 模块文档「为
    /// 什么是旁表」）：结构上不可能污染 `WorldState`。
    screen_focus: WidgetStateTable,
    /// 设置界面这一帧要说的一句话（键位冲突、已保存等），`None` 表示
    /// 没有话要说。与 `feedback` 同一条「留到下一次操作」的语义。
    screen_notice: Option<ScreenNotice>,
    /// 本帧被设置界面改过、尚未交给平台层的键位表，见
    /// [`AppHandler::take_rebound_keys`]。
    pending_bindings: Option<KeyBindings>,
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
        config: GameConfig,
        config_path: PathBuf,
        catalog: Catalog,
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
        // 视野必须在 `continent_field` 被移进下方的结构体字面量之前建好
        // ——它借 `&continent_field`，与上面那句借 `layout()` 是同一条
        // 顺序约束。
        let world_map_view = WorldMapView::centered_on_tile(&continent_field, player_pos);
        let npc_ai = npc_behavior_source(&content, game_world.world.seed);
        let settlement_roles = SettlementRoles::resolve(
            &content.registry,
            &content.class_table,
            &content.resource_table,
            &content.culture_table,
        );
        Demo {
            content,
            game_world,
            camera,
            zoom: Zoom::default(),
            save_path,
            character_name,
            config,
            config_path,
            walk_clip,
            idle_clip,
            engine,
            anim: AnimStateMachine::new(idle_clip, FrameId(0)),
            resources: None,
            catalog,
            hud_anim: WidgetStateTable::new(),
            continent_field,
            menu: PlayerMenu::default(),
            feedback: None,
            world_map_open: false,
            // 视野先对准出生点；每次打开地图时还会重新对准玩家当前位置，
            // 见 `Demo::world_map_view` 字段文档。
            world_map_view,
            npc_ai,
            settlement_roles,
            fps_counter: FpsCounter::new(),
            ui_modes: UiModeStack::new(),
            screen: None,
            screen_focus: WidgetStateTable::new(),
            screen_notice: None,
            pending_bindings: None,
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
    /// NPC——NPC 生成批次之后**不再恒是空操作**，见
    /// [`npc_behavior_source`] 文档),再尝试用本帧输入
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
            if self.world_map_open {
                // 每次打开都重新对准玩家，见 `Demo::world_map_view`
                // 字段文档。
                if let Some(agent) = self.game_world.world.actors.get(self.game_world.player) {
                    self.world_map_view =
                        WorldMapView::centered_on_tile(&self.continent_field, agent.pos);
                }
            }
        }
        // 地图打开时，方向键与缩放键**只作用于地图**，本帧不再驱动玩家
        // 移动与画面缩放，直接返回。
        //
        // # 规格没裁定，本批临时选定
        //
        // 地图是一层全屏浮层。玩家盯着地图按方向键，期待的是移动地图，
        // 而不是让角色在看不见的地方走两步——后者还会**推进世界时钟**
        // （见本方法文档「玩家不行动，时间就不走」），是不可撤销的。
        // 选这条是因为它最保守：反转它只需要删掉这个 `if` 分支。
        //
        // 早退也顺带保证了地图开着的时候世界完全静止：NPC 不动、流式
        // 加载不跑、时钟不走。玩家看地图看多久都不会被咬。
        if self.world_map_open {
            self.pan_and_zoom_world_map(input);
            return;
        }
        // 模态屏盖着的时候，世界一个字节都不动——不跑流式维护、不跑
        // AI、不跑玩家指令，见 `crate::menu_screen` 模块文档「世界在
        // 这块屏底下不动」一节。
        //
        // 这条比「回合制本来就是玩家不动世界就不走」更强，也必须更强：
        // 后者只保证**时钟**不前进，但方向键仍然会被 `player_command`
        // 读成移动意图。整段早退是最保守、也最容易向玩家解释的语义。
        if self.screen.is_some() {
            return;
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
        // 本体二进制不渲染伤害飘字（`p3_acceptance` 才有，那是纯呈现层
        // 的验收效果，见 `ll_sim::turn` 模块文档），因此这条回调在这里
        // 是空操作。
        //
        // 它曾经是 mod 事件监听的落点；脚本系统拆除之后那条通道没有了
        // （判据与论证见本批次提交信息）。回调本身**保留**：它是
        // 「一条效果在呈现层意味着什么」这个问题唯一的接缝，`ll-sim`
        // 不知道调用方在不在渲染。
        let mut on_effect = |_world: &WorldState, _effect: &Effect| {};
        // 行为树真的驱动回合推进这条链路的唯一标准接法，见
        // `ll_sim::behavior::behavior_ai_intent` 文档。`self.npc_ai` 与
        // `self.game_world.world` 是同一个 `self` 上的两个不同字段，
        // 借用检查器分得开，不需要把决策来源搬出去。
        let mut ai_intent = ll_sim::behavior::behavior_ai_intent(&mut self.npc_ai);
        self.engine.advance_ai(
            &mut self.game_world.world,
            player,
            &mut ai_intent,
            &catalogs,
            &mut on_effect,
        );
        drop(ai_intent);
        // 玩家这一回合提交什么，由 `crate::player_action` 决定——它是
        // 物品链那六个意图（`PickUp`/`Drop`/`Equip`/`Unequip`/`Use`/
        // `Craft`）唯一的键位产出者，见该模块文档「这个模块补的是哪条
        // 断线」一节。此前这里调的是 `TurnEngine::try_player_turn`，
        // 它内部只认 `intent_from_input` 的 `Move`/`Wait` 两种，于是
        // 那六个意图在真实游戏里一个都提交不出来。
        //
        // 查不到玩家实体时跳过（与 `draw_hud` 同一条降级纪律）：菜单
        // 要读它的背包与装备。
        let command = player_command(
            &mut self.menu,
            input,
            &self.game_world.world,
            player,
            &self.content.recipe_table,
        );
        match command {
            PlayerCommand::Idle => {}
            PlayerCommand::Rejected(feedback) => self.feedback = Some(feedback),
            PlayerCommand::Submit(intent) => {
                let outcome = self.engine.try_player_intent(
                    &mut self.game_world.world,
                    player,
                    intent,
                    &catalogs,
                    &mut on_effect,
                );
                // 「按了键但屏幕纹丝不动」这一刻必须说话，见
                // `Demo::feedback` 字段文档与
                // `ll_sim::turn::PlayerTurnOutcome` 文档。还没轮到玩家
                // （`NotYet`）不算按空——这次输入压根没被消费，下一帧
                // 原样重试，说话反而是噪音。
                self.feedback = match outcome {
                    PlayerTurnOutcome::Nothing => Some(Feedback::NothingHappened),
                    PlayerTurnOutcome::Acted => None,
                    PlayerTurnOutcome::NotYet => self.feedback,
                };
            }
        }

        if let Some(agent) = self.game_world.world.actors.get(player)
            && matches!(agent.current_space, Space::Surface { .. })
        {
            self.camera.center = agent.pos;
        }
    }

    /// 打开游戏内菜单：压一层模态 UI 栈（这一步同时把输入上下文切到
    /// `InputContext::Menu`、把这一刻按住的键视为全部松开），并把屏
    /// 状态置成菜单。
    ///
    /// 焦点**刻意不预置**在任何一项上（`screen_focus` 保持全空）：玩家
    /// 第一次按方向键才出现焦点，与
    /// `ll_ui::widget::focus::move_focus` 文档「起点」一节的既有约定
    /// 一致。
    fn open_menu(&mut self, input: &mut InputState) {
        self.ui_modes.push(UiMode::Menu, input);
        self.screen = Some(ScreenState::Menu);
        self.screen_focus = WidgetStateTable::new();
        self.screen_notice = None;
    }

    /// 关掉整块模态屏，回到游戏——把栈弹空（同样清空按键状态：玩家在
    /// 菜单里按着方向键就关掉菜单时，角色不该立刻窜出去）。
    fn close_screen(&mut self, input: &mut InputState) {
        while self.ui_modes.pop(input).is_some() {}
        self.screen = None;
        self.screen_notice = None;
    }

    /// 处理模态屏这一帧的输入。返回 `true` 表示玩家要退出整局。
    ///
    /// 拆成独立方法而不是塞进 [`Demo::on_frame`]：`on_frame` 已经同时
    /// 承担着「退出判定 + 世界推进 + 三条渲染通道」，再往里塞一段
    /// 二十行的菜单路由会让它越过 50 行的函数上限。
    fn update_screen(&mut self, input: &mut InputState) -> bool {
        let Some(state) = self.screen else {
            return false;
        };
        let (outcome, next_state) = match state {
            ScreenState::Menu => update_menu(&mut self.screen_focus, input),
            ScreenState::Settings { .. } => {
                let mut state = state;
                let mut ctx = SettingsContext {
                    config: &mut self.config,
                    config_path: &self.config_path,
                    catalog: &self.catalog,
                };
                let (outcome, notice) = update_settings(&mut state, input, &mut ctx);
                if notice.is_some() {
                    self.screen_notice = notice;
                }
                // 设置界面每一帧都可能改过键位表；把它送回平台层的通道
                // 是 `take_rebound_keys`，见其文档。整表克隆只发生在
                // 玩家真的在这块屏里操作的那些帧，不是每帧。
                self.pending_bindings = Some(self.config.bindings.clone());
                // 滤波方式当场生效（`blit_filter` 是一个普通字段）；
                // 垂直同步做不到，它只在 `GpuContext::new` 时决定呈现
                // 模式，屏上那一行因此带着「重启后生效」的提示。
                if let Some(resources) = self.resources.as_mut() {
                    resources.blit_filter = match self.config.display.scale_filter {
                        ScaleFilter::Nearest => BlitFilter::Nearest,
                        ScaleFilter::SharpBilinear => BlitFilter::SharpBilinear,
                    };
                }
                (outcome, Some(state))
            }
        };
        if let Some(next) = next_state {
            self.screen = Some(next);
        }
        match outcome {
            ScreenOutcome::Idle => false,
            ScreenOutcome::Close => {
                self.close_screen(input);
                false
            }
            ScreenOutcome::Quit => true,
        }
    }

    /// 把当前世界层画面存成一张 PNG（`GameKey::Screenshot`，默认 F2）。
    ///
    /// # 这是交接文档第四节第 16 条的接线点
    ///
    /// `GameKey::Screenshot` 此前在 `ll-game` 里**零消费点**——真实
    /// 消费点只在五个验收 demo 里，本体按 F2 没反应。
    ///
    /// **与验收 demo 那一侧的语义区别，必须写清楚**：demo 里那个键存的
    /// 是 `crates/ll-render/tests/visual/baseline/` 下的**视觉回归基准**
    /// （`GameKey::Screenshot` 的枚举文档描述的是那一侧）。本体这一侧
    /// 存的是**玩家截图**，落在数据目录的 `screenshots/` 下、按帧号
    /// 编名、绝不覆盖既有文件。`crates/ll-game/tests/visual/` 下那三张
    /// 基准由 `examples/*_preview.rs` 产出，与本方法无关。
    fn take_screenshot(&self, frame: FrameId) {
        let Some(resources) = self.resources.as_ref() else {
            return;
        };
        let path = self
            .screenshot_dir()
            .join(ll_render::screenshot::screenshot_file_name(frame.0));
        // 存图失败只记日志：按一次截图键失败的代价应当是一条日志，
        // 不是一局游戏，与本模块其余降级路径同一条纪律。
        if let Err(error) =
            ll_render::screenshot::save_png(&resources.gpu, &resources.render_target, &path)
        {
            tracing::warn!(%error, path = %path.display(), "截图失败");
        }
    }

    /// 截图目录：与配置文件同一个数据目录下的 `screenshots/`。
    ///
    /// 从 `config_path` 的父目录推，而不是再传一份路径进来——两者本来
    /// 就同属一个数据目录（见 `crate::GamePaths`），多传一个参数只会
    /// 多一处可能对不上的地方。
    fn screenshot_dir(&self) -> PathBuf {
        self.config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("screenshots")
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
    /// 地图打开时把方向键与缩放键接到世界地图视野上。
    ///
    /// # 为什么复用既有按键，不新增 `GameKey`
    ///
    /// `ZoomIn`/`ZoomOut` 与四个方向键**已经存在**，且已经接好了自动
    /// 重复与滚轮（见 `ll_platform::input::GameKey::ZoomIn` 文档）。
    /// 新增两个「地图专用缩放键」意味着玩家要记两套键、要在设置里绑两
    /// 次，而两套键在任何时刻都恰好只有一套可用——纯粹的重复。复用还有
    /// 一个直接好处：滚轮缩放在地图上白拿，不用再接一遍。
    ///
    /// 方向键走 `was_activated`（参与自动重复）而不是 `was_just_pressed`：
    /// 按住方向键连续平移是地图的通行手感，与它们在游戏内驱动连续移动
    /// 是同一条既有约定。
    fn pan_and_zoom_world_map(&mut self, input: &InputState) {
        if input.was_activated(GameKey::ZoomIn) {
            self.world_map_view.zoom_in();
        }
        if input.was_activated(GameKey::ZoomOut) {
            self.world_map_view.zoom_out();
        }
        // 四个方向各算一次而不是 `else if` 串联：同时按下左和上应当斜着
        // 平移，与游戏内八向移动的既有预期一致。
        let dx = i32::from(input.was_activated(GameKey::Right))
            - i32::from(input.was_activated(GameKey::Left));
        let dy = i32::from(input.was_activated(GameKey::Down))
            - i32::from(input.was_activated(GameKey::Up));
        if dx != 0 || dy != 0 {
            self.world_map_view.pan(&self.continent_field, dx, dy);
        }
    }

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
        // NPC 物化——**必须排在流式加载之后**：物化要读地形判断「这一格
        // 能不能站人」，读的正是上一行刚刚装进来的那些区块（见
        // `crate::world::materialize_nearby_settlements` 文档「时机」一节）。
        let spawned = crate::world::materialize_nearby_settlements(
            &mut self.game_world.world,
            &self.content,
            &self.settlement_roles,
        );
        // 新出现的实体要自己排进时间轴——`rebuild_timeline` 那条整条重建
        // 的路径在这里用不了（会丢掉 `TurnEngine::pending`），见
        // `ll_sim::turn::TurnEngine::schedule` 文档。
        for actor in spawned {
            self.engine.schedule(actor, clock);
        }
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
    world_map_view: &WorldMapView,
    // 玩家菜单与反馈行，见 `Demo::menu`/`Demo::feedback` 字段文档。
    menu: PlayerMenu,
    feedback: Option<Feedback>,
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
    // 规则修正（抗性/易伤/偷袭/盘查减免/藏匿/制作产出加成/优势/劣势/
    // 重掷）——**这里是唯一的装配点**：`agent_rule_modifiers` 要同时
    // 拿到种族/职业/副职三张授予表、天赋表与物品表，`rule_modifier_displays`
    // 还要从伤害类别表/配方类别表里读出主语的 `display_name_key`，这七张
    // 表只在本函数里同时够得着（`content: &LoadedContent`）。面板层拿到的
    // 是已经按加值类型规则合并好的成品行，见
    // `ll_ui::hud::character_panel::CharacterPanelData::rule_modifiers`。
    //
    // 每帧现算一次，与上面天气「派生而不缓存」同一条纪律：修正来自
    // 天赋与**当前装备**，缓存就得有人负责在换装/损坏时让它失效。
    let rule_modifiers = rule_modifier_displays(
        &agent_rule_modifiers(
            agent,
            &content.race_table,
            &content.class_table,
            &content.subclass_table,
            &content.trait_table,
            &content.item_table,
        ),
        // 主语的显示名文案键：**读内容表声明的字段**，不按约定拼键
        // （旧做法与它的代价见 `ll_mod::damage_category` 模块文档
        // 「显示名字段」一节）。两张表在这里都够得着，这也正是本函数
        // 是唯一装配点的原因之一。
        &|registry, index| match registry {
            SubjectRegistry::DamageCategory => content
                .damage_category_table
                .get(index)
                .map(|def| def.display_name_key.clone()),
            SubjectRegistry::RecipeCategory => content
                .recipe_category_table
                .get(index)
                .map(|def| def.display_name_key.clone()),
        },
    );
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
        rule_modifiers: &rule_modifiers,
    };

    // 见本函数文档「世界地图」一节：`world_map_slice_data`/`world_map_sites`
    // 声明在 `if` 之外，让 `world_map_data` 借用的数据在传给 `render_hud`
    // 那一刻仍然存活。
    let world_map_slice_data;
    let world_map_sites;
    let world_map_data = if world_map_open {
        let layout = *game_world.world.terrain.layout();
        world_map_slice_data = world_map_slice(
            continent_field,
            &layout,
            &game_world.world.exploration,
            world_map_view,
        );
        // 玩家位置标记——纯呈现，由玩家坐标现算，不进 `WorldState`、
        // 不进 `OverviewCell`，见 `ll_ui::hud::world_map::WorldMapPanelData::player`
        // 字段文档。环面换算由 `WorldMapSlice::cell_of_tile` 负责（内部
        // 走 `TorusSize`），这里不手写任何取模；它与画格子用的是同一个
        // 视野原点，因此任何缩放档位、任何平移下标记都对得上。
        //
        // 不区分玩家当前在哪个 `Space`：世界地图画的是大陆平面，玩家
        // 下到地下时他在大陆上的**横向**位置没变，标记仍然应该指在那
        // 里——藏起来只会让玩家在地下彻底失去方位感。
        let player = world_map_slice_data.cell_of_tile(agent.pos);

        // 据点标记——所有者要「显示多点细节，好让玩家决定选哪里」，而
        // 「哪里有村子、哪里只剩废墟」大概率是那个决定里最重的一条。
        //
        // 数据来自编年史的 `sites()`：一个**已按区块光栅序排好的切片**
        // （见 `ll_world::chronicle::WorldChronicle::sites` 文档），顺序
        // 因此是世界数据自身的确定性顺序，不是任何哈希容器的桶序
        // （约束 C5）。默认世界二百多座，每帧一次线性遍历，与同一帧已经
        // 在跑的整屏归并相比可以忽略。
        //
        // 编年史拿不到时（`chronicle_handle` 为 `None`）就不画据点——
        // 与 `GpuResources::lookup`「图集条目缺失，跳过本次绘制」同一条
        // 显示层降级纪律，不 panic。
        //
        // **资源点没有画**：`SettlementSite` 只存了这座据点靠什么吃饭的
        // 两种资源（那是据点的属性），世界里真正的资源点分布没有一份
        // 可供概览查询的索引，要现算就得逐区块跑资源采样——那正是
        // `ContinentField` 存在的理由所要避免的开销。如实不做，不硬接。
        let chronicle = game_world.world.terrain.chronicle_handle();
        world_map_sites = chronicle
            .as_ref()
            .map(|chronicle| {
                chronicle
                    .sites()
                    .iter()
                    .filter_map(|site| {
                        world_map_slice_data
                            .cell_of_tile(site.anchor)
                            .map(|cell| WorldMapSite {
                                cell,
                                inhabited: matches!(site.status, SettlementStatus::Inhabited),
                            })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Some(WorldMapPanelData {
            cells: &world_map_slice_data.cells,
            cols: world_map_slice_data.cols,
            rows: world_map_slice_data.rows,
            terrain_ids: &content.terrain_ids,
            player,
            sites: &world_map_sites,
            tiles_per_cell: world_map_view.tiles_per_cell(continent_field),
        })
    } else {
        None
    };

    // 菜单这一帧的行：与 `player_command` 各自独立重建一次，理由见
    // `crate::player_action::menu_rows` 文档。`menu_rows` 必须在
    // `menu_data` 之前声明——后者借用前者产出的字符串。
    let menu_rows = crate::player_action::menu_rows(
        menu,
        &game_world.world,
        game_world.player,
        &content.recipe_table,
        &content.item_table,
        catalog,
        language,
    );
    let menu_data = crate::player_action::menu_data(menu, &menu_rows);
    let feedback_text = feedback.map(|feedback| catalog.resolve(language, feedback.i18n_key()));

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
        // 观察者是玩家自己——未鉴定的东西在背包/装备两块面板上显示成
        // 「未鉴定的物品」，见 `ll_ui::hud::item_display_name`。
        &agent.identified_items,
        &content.item_table,
        &content.item_table,
        catalog,
        language,
        &resources.skin,
        hud_anim,
        frame.0,
        world_map_data.as_ref(),
        menu_data.as_ref(),
        feedback_text.as_deref(),
    );
}

/// 把模态屏（菜单/设置）画到 `view` 上——**排在 [`draw_hud`] 之后**，
/// 因此那层压暗背板会把世界层与 HUD 一起压暗，见 `ll_ui::screen::render`
/// 模块文档。
///
/// `screen` 为 `None` 时整块不参与本次产出——不是「画出来但透明」，是
/// 压根不调用渲染函数，与 `draw_hud` 对世界地图/动作菜单的同一条纪律。
fn draw_screen(
    screen: Option<ScreenState>,
    config: &GameConfig,
    catalog: &Catalog,
    focus: &WidgetStateTable,
    notice: Option<ScreenNotice>,
    resources: &mut GpuResources,
    view: &wgpu::TextureView,
) {
    let Some(state) = screen else {
        return;
    };
    let language = config.language.as_str();
    // 行文字与光标位置由 `crate::menu_screen` 排版，本函数只负责把
    // 结果交给 GPU——见该模块文档「为什么排版在这一层」一节。
    let (rows, cursor) = match state {
        ScreenState::Menu => (menu_row_texts(catalog, language), menu_focus_index(focus)),
        ScreenState::Settings { cursor, capturing } => {
            let rows = settings_rows();
            (
                settings_row_texts(&rows, config, catalog, capturing, cursor),
                cursor,
            )
        }
    };
    let notice_text = notice.map(|notice| notice.resolve(catalog, language));
    let data = screen_data(state, &rows, cursor, notice_text.as_deref());
    let size = resources.window_size;
    render_screen(
        &mut resources.quad_renderer,
        &mut resources.textured_quad_renderer,
        &mut resources.text_renderer,
        resources.gpu.device(),
        resources.gpu.queue(),
        view,
        size.width,
        size.height,
        &data,
        catalog,
        language,
        &resources.skin,
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

    // 地面物品堆 / 放置家具 / NPC——地形与玩家之外的三类世界内容，见
    // `crate::surface_draw` 模块文档「这个模块补的是哪个洞」。三类共用
    // 同一个 push 帮手，绘制层序由各自指令里的 `layer` 决定（地形
    // → 地面物品/家具 → NPC → 玩家）。
    for draw in surface_draws(world, &content.registry, game_world.player) {
        // **只画当前视野内的**，不画「记得那里曾经有东西」：迷雾记忆
        // （`ll_world::exploration`）记的是地形，不是物品与人——地形不会
        // 自己跑掉，物品与 NPC 会。把它们也按记忆画出来，等于让玩家隔着
        // 迷雾看见一堆早就被人捡走的东西。这条是本批次的判断，不是所有者
        // 的裁定；真要做「上次见到时那里有东西」的记忆层，需要先有一份
        // 存进 `WorldState` 的观察记录，不是渲染层能自己变出来的。
        if !visible.contains(draw.pos) {
            continue;
        }
        push_surface_draw(&draw, camera, tint, zoom, resources);
    }

    let (px, py) = camera.world_to_screen(player_pos);
    push_player_marker(px, py, sprite_name, tint, zoom, resources);
}

/// 把一条 [`SurfaceDraw`] 变成一个精灵实例推进批次。
///
/// 三类世界内容（地面物品堆、放置家具、NPC）共用这一个消费点——这正是
/// `crate::surface_draw` 模块文档里「不许把同一段查图逻辑抄三遍」那条
/// 抽象理由的落点：查图次序（内容自带键 → 通用记号）在
/// [`SurfaceDraw::keys`]，锚点/缩放/绘制顺序换算在这里，两处各只有一份。
///
/// 查不到任何图集条目时**跳过**而不是 panic：兜底记号缺席已经由
/// [`GpuResources::lookup_first`] 打了 error 日志，画面上少一个记号远好于
/// 让整局游戏崩掉（与 [`push_player_marker`] 同一条降级纪律）。
fn push_surface_draw(
    draw: &SurfaceDraw,
    camera: &Camera,
    tint: [f32; 4],
    zoom: Zoom,
    resources: &mut GpuResources,
) {
    let Some((entry, uv)) = resources.lookup_first(draw.keys()) else {
        return;
    };
    let (sx, sy) = camera.world_to_screen(draw.pos);
    let footprint = entry.footprint;
    // 锚点换算与 zoom 后处理的次序与 `push_player_marker` 逐字一致，
    // 理由见那边的注释。
    let [ax, ay] = sprite_draw_position((sx, sy), footprint, entry.pivot);
    let order = DrawOrder::new(
        draw.layer,
        footprint_bottom_screen_y(sy, footprint.height),
        draw.entity,
    );
    let [zx, zy] = apply_zoom([ax, ay], zoom);
    resources.batch.push(
        order,
        sprite_instance(zx, zy, entry.sprite_size(), uv, tint, zoom),
    );
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
            self.config.display,
            &self.content.asset_vfs,
        ));
    }

    fn on_resize(&mut self, size: PhysicalSize<u32>) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.resize(size);
    }

    fn on_frame(&mut self, frame: FrameId, input: &mut InputState) -> FrameOutcome {
        // 墙钟采样,见 `ll_platform::fps` 模块文档「为什么用墙钟,不用
        // 帧计数」一节——`Instant::now()` 只在这一处调用,产出的浮点数
        // 只流向状态栏文本,不进 `self.game_world`/`WorldState`。
        let fps = self.fps_counter.record_frame(std::time::Instant::now());

        // 截图键（默认 F2）——一次性动作，与地图键同一类。排在最前面：
        // 存的是**上一帧已经画好**的离屏目标，与本帧要不要推进世界无关。
        if input.was_just_pressed(GameKey::Screenshot) {
            self.take_screenshot(frame);
        }

        // 模态屏开着时，这一帧的输入全部归它——**必须排在下面那条
        // 「取消键退出游戏」之前**：否则玩家想关个菜单会直接退出整局
        // （与 `crate::player_action` 里 `player_command` 第 ② 步防的
        // 是同一个陷阱）。
        if self.screen.is_some() {
            if self.update_screen(input) {
                return FrameOutcome::Exit;
            }
        } else if input.was_just_pressed(GameKey::Menu) && !self.menu.is_open() {
            // 菜单键（默认 Tab）——交接文档第四节第 17 条那条死路径的
            // 消费点。`was_just_pressed` 而非 `was_activated`：一次性
            // 动作键，长按不该反复开关。
            //
            // 背包/制作/交互列表开着时不叠第二块模态 UI：两块屏叠在
            // 一起会立刻引出「Esc 关哪一层」的新裁定，而没有任何人
            // 要求过这件事。
            self.open_menu(input);
        }

        // 菜单开着时取消键归菜单用（关掉它），见 `crate::player_action`
        // 里 `player_command` 第 ② 步的同一段说明。
        //
        // # 什么都没开时按取消键：**开主菜单，不退出游戏**
        //
        // 这一段推翻了本函数此前的行为。游戏内菜单落地之前，顶层按取消
        // 键是唯一的退出通道，于是它直接返回 [`FrameOutcome::Exit`]
        // ——按一下 Esc 整局就没了，没有任何确认。项目所有者实机撞到这
        // 件事并要求改掉。
        //
        // 现在退出有了正经去处：菜单里那一项，经 `update_screen` 的
        // `ScreenOutcome::Quit` 走同一个 `FrameOutcome::Exit`。因此 Esc
        // 回归它在绝大多数游戏里的含义——**逐层往回退**：开着子菜单就
        // 关子菜单，什么都没开就开主菜单，再从菜单里选退出。
        //
        // **刻意不改键位表**：Esc 绑给 `GameKey::Cancel`（`ll_platform`
        // 的默认表，玩家磁盘上那份也是这么写的）本来就是对的。错的是
        // 「顶层 Cancel 等于退出」这条行为，不是那条绑定——改键位只会
        // 把同一个问题挪到另一个键上。
        if self.screen.is_none()
            && !self.menu.is_open()
            && input.was_just_pressed(ll_platform::input::GameKey::Cancel)
        {
            self.open_menu(input);
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
                &self.config.language,
                resources,
                &view,
                &mut self.hud_anim,
                frame,
                fps,
                self.world_map_open,
                &self.continent_field,
                &self.world_map_view,
                self.menu,
                self.feedback,
            );
            draw_screen(
                self.screen,
                &self.config,
                &self.catalog,
                &self.screen_focus,
                self.screen_notice,
                resources,
                &view,
            );
            resources.present_frame(surface_frame);
        }

        FrameOutcome::Continue
    }

    /// 平台层每次按键/滚轮事件都问一句：这一帧该按哪张表解析物理键。
    ///
    /// 答案完全由 [`Demo::ui_modes`] 决定——它是本项目里「现在有没有
    /// 一块模态屏盖着」的唯一真相源，见该字段文档与
    /// [`AppHandler::input_context`] 的完整论证。
    fn input_context(&self) -> InputContext {
        self.ui_modes.current_context()
    }

    /// 把设置界面这一帧改好的键位表交给平台层，见
    /// [`AppHandler::take_rebound_keys`]。
    fn take_rebound_keys(&mut self) -> Option<KeyBindings> {
        self.pending_bindings.take()
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
    use crate::player_action::{InventoryEntry, inventory_entries};
    use ll_core::ident::{ContentIndex, NamespacedId};
    use ll_core::time::Tick;
    use ll_core::torus::TorusPos;
    use ll_platform::input::GameKey;
    use ll_sim::item::NoItems;
    use ll_sim::resolve::derive_stats;
    use ll_world::entity::{ActiveStatModifier, AttributeKind};
    use ll_world::item::ItemStack;

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
        let game_world = crate::world::build_new_world(
            &content,
            ll_world::generate::GenParams {
                seed: 1,
                ..ll_world::generate::GenParams::default()
            },
        )
        .expect("测试用布局满足全部构造前置条件");
        let save_path =
            crate::test_support::unique_temp_path("ll-game-app-test-save").with_extension("llsave");
        Demo::new(
            content,
            game_world,
            save_path,
            "测试旅人".to_string(),
            GameConfig::default(),
            crate::test_support::unique_temp_path("ll-game-app-test-config").join("config.json5"),
            Catalog::load_dir(&std::env::temp_dir().join("ll-game-app-test-empty-locales")),
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
    // ───────────────────── 输入接线批次：物品链六个意图 ─────────────────────
    //
    // 下面这一组的共同点：**Act 一律只有按键**。它们要证明的不是
    // 「`resolve_craft` 会扣食材」（那条早就有测试，走 AI 策略直接返回
    // 意图这条最小提交路径），而是「玩家在真实游戏里按得出来」——本
    // 批次全部的价值就在这一句上，证据因此必须从输入侧出发。
    //
    // 全程跑在 `test_demo` 建出的真实 `Demo` 上：真实 `mods/` 内容
    // （与 `crates/ll-mod/tests/furniture_placement.rs` 同一份 `mods/`）、
    // 真实 `build_new_world`、真实 `TurnEngine`。Arrange 里只摆「玩家
    // 身上有什么」（背包、已知配方、脚下地形），一次都不直接构造
    // `Intent`、不直接调 `resolve`/`apply`。

    /// 按内容 id 取索引——真实注册表，查不到直接 panic（说明 `mods/`
    /// 里那条内容被改名或删了，应当立刻显形）。
    fn content_index(demo: &Demo, id: &str) -> ContentIndex {
        demo.content
            .registry
            .get(&NamespacedId::parse(id).expect("测试里的字面量恒合法"))
            .unwrap_or_else(|| panic!("{id} 应当已被 mods/lostland/ 注册"))
    }

    /// 跑一帧，本帧按住 `keys` 里的每个键。
    ///
    /// 每次都新建 `InputState`：`was_just_pressed` 因此在这一帧恰好置位
    /// 一次，与真实事件循环里「按下 → 下一帧清标志」的时序等价，且不
    /// 依赖 `begin_frame`/`end_frame`——与本模块既有的地图开关测试同一
    /// 个手法。
    fn frame(demo: &mut Demo, at: u64, keys: &[GameKey]) {
        let mut input = InputState::new();
        for key in keys {
            input.press(*key);
        }
        demo.advance(&input, FrameId(at));
    }

    /// 把光标从第 0 行按到第 `row` 行——每帧一次「下」。
    fn move_cursor_to(demo: &mut Demo, at: &mut u64, row: usize) {
        for _ in 0..row {
            frame(demo, *at, &[GameKey::Down]);
            *at += 1;
        }
    }

    /// 背包里这一种东西一共有几个（一件都没有时是 0）。
    ///
    /// **把全部同种堆加起来，不是只看第一条**：同一种东西在背包里完全
    /// 可能占着不止一堆（出生装备里已经有一份、测试又塞了一份，两者不
    /// 会自动合并——合并只发生在 `merge_into_inventory_effect` 那条路径
    /// 上）。只看第一条会在第一堆被吃空、`apply` 把它移除之后突然"变
    /// 多"，那是一次真实踩到的假失败。
    fn carried(demo: &Demo, def: ContentIndex) -> u32 {
        demo.game_world
            .world
            .actors
            .get(demo.game_world.player)
            .expect("玩家仍存在")
            .inventory
            .iter()
            .filter(|stack| stack.def == def)
            .map(|stack| stack.count)
            .sum()
    }

    /// 玩家脚下那一格摆着的全部地面物品定义。
    fn ground_defs_underfoot(demo: &Demo) -> Vec<ContentIndex> {
        let pos = demo
            .game_world
            .world
            .actors
            .get(demo.game_world.player)
            .expect("玩家仍存在")
            .pos;
        demo.game_world
            .world
            .ground_items
            .iter()
            .filter(|ground| ground.pos == pos)
            .map(|ground| ground.stack.def)
            .collect()
    }

    /// 把玩家脚下那一格改成草地。
    ///
    /// **不是可有可无的布景**：`build_new_world` 生成出来的出生格是什么
    /// 地形取决于噪声，完全可能是深水（`blocks_move` 为真），那会让
    /// `can_place_furniture` 的第 ② 道前置不成立，于是「放得下」与
    /// 「放不下」两侧同时因为一个与被测逻辑无关的原因落到同一侧——
    /// `crates/ll-mod/tests/furniture_placement.rs` 的
    /// `Scene::terrain_underfoot` 字段文档记的是同一个坑。
    fn clear_terrain_underfoot(demo: &mut Demo) {
        let pos = demo
            .game_world
            .world
            .actors
            .get(demo.game_world.player)
            .expect("玩家仍存在")
            .pos;
        let grass = demo.content.terrain_ids.grass;
        demo.game_world.world.terrain.set_terrain(pos, grass);
    }

    /// 玩家背包/装备菜单里，第一条指着 `def` 的行是第几行。
    fn inventory_row_of(demo: &Demo, def: ContentIndex) -> usize {
        let agent = demo
            .game_world
            .world
            .actors
            .get(demo.game_world.player)
            .expect("玩家仍存在");
        inventory_entries(agent)
            .iter()
            .position(|entry| match entry {
                InventoryEntry::Carried { def: candidate } => *candidate == def,
                InventoryEntry::Equipped { def: candidate, .. } => *candidate == def,
            })
            .expect("这一行应当在菜单里")
    }

    /// 制作菜单里这条配方是第几行。
    fn craft_row_of(demo: &Demo, recipe: ContentIndex) -> usize {
        crate::player_action::craft_entries(&demo.content.recipe_table)
            .iter()
            .position(|candidate| *candidate == recipe)
            .expect("这条配方应当在菜单里")
    }

    /// [`arrange_smith`] 摆出来的那一组内容索引。
    struct SmithFixture {
        iron_ingot: ContentIndex,
        leather_strip: ContentIndex,
        smith_hammer: ContentIndex,
        forge: ContentIndex,
        iron_shortsword: ContentIndex,
        forge_recipe: ContentIndex,
        iron_shortsword_recipe: ContentIndex,
    }

    /// 玩家此刻站在哪一格。
    fn player_pos(demo: &Demo) -> TorusPos {
        demo.game_world
            .world
            .actors
            .get(demo.game_world.player)
            .expect("玩家仍存在")
            .pos
    }

    /// 直接往玩家脚下这一格摆一堆东西（Arrange 用，不经按键）。
    fn put_on_ground(demo: &mut Demo, def: ContentIndex, placed: bool) {
        let pos = player_pos(demo);
        let clock = demo.game_world.world.clock;
        demo.game_world
            .world
            .ground_items
            .push(ll_world::item::GroundItemStack {
                pos,
                stack: ItemStack::new(def, 1),
                dropped_at: clock,
                contents: Vec::new(),
                placed,
            });
    }

    /// 按键把铁匠锤装上——多条用例共用的一段 Arrange，**仍然全程按键**
    /// （不是直接写 `agent.equipment`）：它在别的用例里是 Arrange，在
    /// 「按装备键…」那条里是被验证的 Act，两处走同一条路才谈得上一致。
    fn equip_hammer_by_keys(demo: &mut Demo, at: &mut u64, fixture: &SmithFixture) {
        frame(demo, *at, &[GameKey::Inventory]);
        *at += 1;
        let row = inventory_row_of(demo, fixture.smith_hammer);
        move_cursor_to(demo, at, row);
        frame(demo, *at, &[GameKey::Equip]);
        *at += 1;
        frame(demo, *at, &[GameKey::Inventory]);
        *at += 1;
    }

    /// 按键砌出一座锻炉（进背包，还没立起来）。
    fn craft_forge_by_keys(demo: &mut Demo, at: &mut u64, fixture: &SmithFixture) {
        frame(demo, *at, &[GameKey::Craft]);
        *at += 1;
        let row = craft_row_of(demo, fixture.forge_recipe);
        move_cursor_to(demo, at, row);
        frame(demo, *at, &[GameKey::Confirm]);
        *at += 1;
        frame(demo, *at, &[GameKey::Craft]);
        *at += 1;
    }

    /// 按键把背包里的锻炉立在脚下。
    fn place_forge_by_keys(demo: &mut Demo, at: &mut u64, fixture: &SmithFixture) {
        frame(demo, *at, &[GameKey::Inventory]);
        *at += 1;
        let row = inventory_row_of(demo, fixture.forge);
        move_cursor_to(demo, at, row);
        frame(demo, *at, &[GameKey::Place]);
        *at += 1;
        frame(demo, *at, &[GameKey::Inventory]);
        *at += 1;
    }

    /// 摆好「能打铁的玩家」：背包里 8 块铁锭 + 1 条皮革 + 1 把铁匠锤，
    /// 已知打铁短剑那条配方（它 `requires_discovery: true`），脚下是草地。
    ///
    /// 8 块铁锭 = 砌锻炉 6 块 + 打短剑 2 块，正好是整条链的用量。
    fn arrange_smith(demo: &mut Demo) -> SmithFixture {
        let fixture = SmithFixture {
            iron_ingot: content_index(demo, "lostland:iron_ingot"),
            leather_strip: content_index(demo, "lostland:leather_strip"),
            smith_hammer: content_index(demo, "lostland:smith_hammer"),
            forge: content_index(demo, "lostland:forge"),
            iron_shortsword: content_index(demo, "lostland:iron_shortsword"),
            forge_recipe: content_index(demo, "lostland:forge_recipe"),
            iron_shortsword_recipe: content_index(demo, "lostland:iron_shortsword_recipe"),
        };
        let hammer_durability = demo
            .content
            .item_table
            .get(fixture.smith_hammer)
            .and_then(|view| view.max_durability);
        let player = demo.game_world.player;
        let agent = demo
            .game_world
            .world
            .actors
            .get_mut(player)
            .expect("玩家刚建局，必然存在");
        agent.inventory.push(ItemStack::new(fixture.iron_ingot, 8));
        agent
            .inventory
            .push(ItemStack::new(fixture.leather_strip, 1));
        agent.inventory.push(ItemStack::freshly_made(
            fixture.smith_hammer,
            1,
            hammer_durability,
        ));
        agent.known_recipes.push(fixture.iron_shortsword_recipe);
        clear_terrain_underfoot(demo);
        fixture
    }

    #[test]
    fn 全程只按键就能砌炉子立起来再打出一把铁短剑且铁锭真的被扣掉() {
        // 本批次的验收线本身。Act 里没有任何一次手工构造的 `Intent`：
        // 从开背包、选锤子、按装备键，到砌炉子、按放置键立起来、走到
        // 它上面按空格交互、在制作菜单里选配方按确认——全部经由
        // `crate::player_action::player_command` →
        // `TurnEngine::try_player_intent`，与玩家真的坐在键盘前一模一样。
        // Arrange
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        assert_eq!(
            carried(&demo, fixture.iron_ingot),
            8,
            "Arrange 应当摆了 8 块铁锭"
        );
        let mut at = 0u64;

        // Act ①：开背包 → 光标移到铁匠锤 → 按装备键。
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        let hammer_row = inventory_row_of(&demo, fixture.smith_hammer);
        move_cursor_to(&mut demo, &mut at, hammer_row);
        frame(&mut demo, at, &[GameKey::Equip]);
        at += 1;
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        assert!(
            demo.game_world
                .world
                .actors
                .get(demo.game_world.player)
                .expect("玩家仍存在")
                .equipment
                .values()
                .any(|stack| stack.def == fixture.smith_hammer),
            "按装备键之后铁匠锤应当真的穿在身上——打铁短剑的 required_tool"
        );

        // Act ②：开制作菜单 → 光标移到「砌锻炉」→ 确认。
        frame(&mut demo, at, &[GameKey::Craft]);
        at += 1;
        let forge_recipe_row = craft_row_of(&demo, fixture.forge_recipe);
        move_cursor_to(&mut demo, &mut at, forge_recipe_row);
        frame(&mut demo, at, &[GameKey::Confirm]);
        at += 1;
        assert_eq!(
            carried(&demo, fixture.forge),
            1,
            "按确认之后背包里应当多出一座锻炉"
        );
        assert_eq!(
            carried(&demo, fixture.iron_ingot),
            2,
            "砌锻炉吃掉 6 块铁锭，应当只剩 2 块"
        );

        // Act ③：关制作菜单 → 开背包 → 光标移到锻炉 → 按**放置**键。
        // 按丢弃键在这里是不够的——丢下去的炉子是躺着的，当不了场地，
        // 见 `ll_sim::intent::Intent::Place` 文档。
        frame(&mut demo, at, &[GameKey::Craft]);
        at += 1;
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        let forge_row = inventory_row_of(&demo, fixture.forge);
        move_cursor_to(&mut demo, &mut at, forge_row);
        frame(&mut demo, at, &[GameKey::Place]);
        at += 1;
        let player_pos = player_pos(&demo);
        assert_eq!(
            demo.game_world
                .world
                .placed_at(player_pos)
                .map(|ground| ground.stack.def),
            Some(fixture.forge),
            "按放置键之后锻炉应当**立**在脚下这一格上"
        );
        assert_eq!(carried(&demo, fixture.forge), 0, "锻炉应当已经离开背包");

        // Act ④：关背包 → 按**空格**与脚下的炉子交互 → 在交互列表里
        // 按确认（那一行的主交互是「在此开工」，会换开制作菜单）→
        // 光标移到「打铁短剑」→ 确认。
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        frame(&mut demo, at, &[GameKey::Interact]);
        at += 1;
        assert!(
            matches!(demo.menu, PlayerMenu::Interact { .. }),
            "脚下立着炉子，按空格应当开出交互列表，实际是 {:?}",
            demo.menu
        );
        frame(&mut demo, at, &[GameKey::Confirm]);
        at += 1;
        assert!(
            matches!(demo.menu, PlayerMenu::Craft { .. }),
            "对着立着的炉子按确认应当换开制作菜单，实际是 {:?}",
            demo.menu
        );
        let sword_row = craft_row_of(&demo, fixture.iron_shortsword_recipe);
        move_cursor_to(&mut demo, &mut at, sword_row);
        frame(&mut demo, at, &[GameKey::Confirm]);

        // Assert：剑真的造出来了，两味食材真的被扣掉。
        assert_eq!(
            carried(&demo, fixture.iron_shortsword),
            1,
            "按确认之后背包里应当多出一把铁短剑"
        );
        assert_eq!(
            carried(&demo, fixture.iron_ingot),
            0,
            "打短剑再吃掉 2 块铁锭，8 块应当一块不剩"
        );
        assert_eq!(
            carried(&demo, fixture.leather_strip),
            0,
            "打短剑吃掉那一条皮革"
        );
    }

    #[test]
    fn 按丢弃键放下的炉子是躺着的当不了场地() {
        // 上一条的反例，也是「丢弃与放置是两个动作」这条裁定在输入侧的
        // 直接后果：同样的按键序列，只把放置键换成丢弃键，制作就做不出
        // 来。没有这一条，无法排除「其实丢弃也照样能立起来」。
        // Arrange：按键砌一座炉子出来，然后**丢**在脚下。
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        let mut at = 0u64;
        equip_hammer_by_keys(&mut demo, &mut at, &fixture);
        craft_forge_by_keys(&mut demo, &mut at, &fixture);
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        let forge_row = inventory_row_of(&demo, fixture.forge);
        move_cursor_to(&mut demo, &mut at, forge_row);
        frame(&mut demo, at, &[GameKey::Drop]);
        at += 1;
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        let player_pos = player_pos(&demo);
        assert!(
            ground_defs_underfoot(&demo).contains(&fixture.forge),
            "Arrange：炉子确实落在了脚下"
        );
        assert!(
            demo.game_world.world.placed_at(player_pos).is_none(),
            "Arrange：但它是躺着的，不是立着的"
        );

        // Act：开制作菜单选打铁短剑，按确认。
        frame(&mut demo, at, &[GameKey::Craft]);
        at += 1;
        let sword_row = craft_row_of(&demo, fixture.iron_shortsword_recipe);
        move_cursor_to(&mut demo, &mut at, sword_row);
        frame(&mut demo, at, &[GameKey::Confirm]);

        // Assert：一把剑都没有，食材一块没少。
        assert_eq!(carried(&demo, fixture.iron_shortsword), 0);
        assert_eq!(
            carried(&demo, fixture.iron_ingot),
            2,
            "静默失败不消耗任何东西"
        );
    }

    #[test]
    fn 按空格开出的交互列表里按捡起键能把立着的炉子收回背包() {
        // 「摆下去还能收回来」这条闭环的出口。
        // Arrange：按键把炉子立起来。
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        let mut at = 0u64;
        craft_forge_by_keys(&mut demo, &mut at, &fixture);
        place_forge_by_keys(&mut demo, &mut at, &fixture);
        let player_pos = player_pos(&demo);
        assert!(
            demo.game_world.world.placed_at(player_pos).is_some(),
            "Arrange 应当已经把锻炉立在脚下"
        );

        // Act：空格开列表 → 按捡起键。
        frame(&mut demo, at, &[GameKey::Interact]);
        at += 1;
        frame(&mut demo, at, &[GameKey::PickUp]);

        // Assert
        assert_eq!(
            carried(&demo, fixture.forge),
            1,
            "按捡起键之后锻炉应当回到背包"
        );
        assert!(
            !ground_defs_underfoot(&demo).contains(&fixture.forge),
            "锻炉应当已经离开地面"
        );
    }

    #[test]
    fn 脚下只有一样东西时按空格也照样弹列表() {
        // 所有者原话「无论是一个还是 N 个，交互的时候都统一以列表显示」
        // ——这一条守的正是那句话：不许有「只有一件就直接捡走」的捷径。
        // 没有它，实现随时可能为了「体贴」加回那条捷径，从而造出两条
        // 拾取路径。
        // Arrange：脚下恰好一堆铁锭。
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        put_on_ground(&mut demo, fixture.iron_ingot, false);
        let carried_before = carried(&demo, fixture.iron_ingot);

        // Act：按一次空格。
        frame(&mut demo, 0, &[GameKey::Interact]);

        // Assert：列表开着，东西**还在地上**——没有被直接捡走。
        assert!(
            matches!(demo.menu, PlayerMenu::Interact { .. }),
            "只有一样东西时也必须弹列表，实际是 {:?}",
            demo.menu
        );
        assert_eq!(
            carried(&demo, fixture.iron_ingot),
            carried_before,
            "开列表这一步不该捡走任何东西"
        );
        assert!(ground_defs_underfoot(&demo).contains(&fixture.iron_ingot));
    }

    #[test]
    fn 附近什么都没有时按空格给出提示而不是静默作废() {
        // 所有者那张表的第 0 行：范围内一格有东西的都没有 → 一句话。
        // Arrange：确认脚下与相邻八格全空。
        let mut demo = test_demo();
        assert!(
            crate::player_action::interact_tiles(&demo.game_world.world, player_pos(&demo))
                .is_empty(),
            "Arrange 假设出生点周围没有任何地面物品"
        );

        // Act
        frame(&mut demo, 0, &[GameKey::Interact]);

        // Assert
        assert_eq!(demo.menu, PlayerMenu::Closed, "什么都没有时不该开出菜单");
        assert_eq!(demo.feedback, Some(Feedback::NothingNearby));
    }

    #[test]
    fn 范围内两格有东西时按空格先弹方向列表() {
        // 所有者那张表的第「2 以上」行：先选和哪一格交互。
        // Arrange：脚下一堆铁锭，正东相邻格再一堆。
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        put_on_ground(&mut demo, fixture.iron_ingot, false);
        let here = player_pos(&demo);
        let east = demo.game_world.world.size.wrap(here.x() + 1, here.y());
        let clock = demo.game_world.world.clock;
        demo.game_world
            .world
            .ground_items
            .push(ll_world::item::GroundItemStack {
                pos: east,
                stack: ItemStack::new(fixture.leather_strip, 1),
                dropped_at: clock,
                contents: Vec::new(),
                placed: false,
            });

        // Act
        frame(&mut demo, 0, &[GameKey::Interact]);

        // Assert：开的是方向列表，不是物品列表。
        assert!(
            matches!(demo.menu, PlayerMenu::InteractDirection { .. }),
            "两格有东西时应当先弹方向列表，实际是 {:?}",
            demo.menu
        );

        // 再按确认：选中第一行（脚下），进那一格的物品列表——**不**捡走
        // 任何东西（所有者原话「不是捡走，只是打开交互列表」）。
        let carried_before = carried(&demo, fixture.iron_ingot);
        frame(&mut demo, 1, &[GameKey::Confirm]);
        assert_eq!(
            demo.menu,
            PlayerMenu::Interact {
                pos: here,
                cursor: 0
            },
            "选完方向应当进那一格的物品列表"
        );
        assert_eq!(
            carried(&demo, fixture.iron_ingot),
            carried_before,
            "选方向这一步不该捡走任何东西"
        );
    }

    #[test]
    fn 隔一格也能从方向列表里把东西捡过来() {
        // 「够得着的范围」那条规则的正向证据：范围是脚下加相邻八格
        // （切比雪夫 1），因为移动本身是八向的。这里取**斜后方**那一格
        // ——若范围只认正交四邻，本条会红。
        // Arrange：脚下空着，西北相邻格上一堆皮革。
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        let here = player_pos(&demo);
        let north_west = demo.game_world.world.size.wrap(here.x() - 1, here.y() - 1);
        let clock = demo.game_world.world.clock;
        demo.game_world
            .world
            .ground_items
            .push(ll_world::item::GroundItemStack {
                pos: north_west,
                stack: ItemStack::new(fixture.leather_strip, 2),
                dropped_at: clock,
                contents: Vec::new(),
                placed: false,
            });
        let carried_before = carried(&demo, fixture.leather_strip);

        // Act：只有一格有东西 → 直接进那一格的物品列表 → 按确认捡起。
        frame(&mut demo, 0, &[GameKey::Interact]);
        assert_eq!(
            demo.menu,
            PlayerMenu::Interact {
                pos: north_west,
                cursor: 0
            },
            "只有一格有东西时应当跳过方向列表，直接开那一格的物品列表"
        );
        frame(&mut demo, 1, &[GameKey::Confirm]);

        // Assert：斜后方那一格上的东西真的到了背包里。
        assert_eq!(carried(&demo, fixture.leather_strip), carried_before + 2);
        assert!(
            demo.game_world
                .world
                .ground_items
                .iter()
                .all(|ground| ground.pos != north_west)
        );
    }

    #[test]
    fn 够不着的两格之外按拾取意图静默无效() {
        // 上一条的反例：把同一堆东西挪到切比雪夫距离 2，一样的意图就
        // 什么都不发生。没有这一条，无法排除「够得着判定其实没生效，
        // 隔多远都能捡」。
        // Arrange
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        let here = player_pos(&demo);
        let far = demo.game_world.world.size.wrap(here.x() + 2, here.y());
        let clock = demo.game_world.world.clock;
        demo.game_world
            .world
            .ground_items
            .push(ll_world::item::GroundItemStack {
                pos: far,
                stack: ItemStack::new(fixture.leather_strip, 2),
                dropped_at: clock,
                contents: Vec::new(),
                placed: false,
            });
        let carried_before = carried(&demo, fixture.leather_strip);

        // Act：按空格——那一格在范围外，因此扫不到任何候选格。
        frame(&mut demo, 0, &[GameKey::Interact]);

        // Assert
        assert_eq!(demo.menu, PlayerMenu::Closed);
        assert_eq!(demo.feedback, Some(Feedback::NothingNearby));
        assert_eq!(carried(&demo, fixture.leather_strip), carried_before);
    }

    #[test]
    fn 这一格立着东西时按丢弃键丢不下去并给出反馈() {
        // 「静默作废对 AI 可以，对玩家不行」这条的落点，见
        // `ll_sim::turn::PlayerTurnOutcome` 文档；同时守所有者那条
        // 「家具如果是放置在那个地方，那物品就无法被丢在那」。
        // Arrange：脚下立着一座炉子，背包里有铁锭。
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        put_on_ground(&mut demo, fixture.forge, true);
        let carried_before = carried(&demo, fixture.iron_ingot);
        let mut at = 0u64;

        // Act：开背包 → 移到铁锭 → 按丢弃键。
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        let ingot_row = inventory_row_of(&demo, fixture.iron_ingot);
        move_cursor_to(&mut demo, &mut at, ingot_row);
        frame(&mut demo, at, &[GameKey::Drop]);

        // Assert：一块都没丢出去，而且玩家被告知了。
        assert_eq!(carried(&demo, fixture.iron_ingot), carried_before);
        assert_eq!(
            demo.feedback,
            Some(Feedback::NothingHappened),
            "按了键却什么都没发生时，必须有一句话告诉玩家"
        );
    }

    #[test]
    fn 菜单开着时方向键只移动光标不移动角色() {
        // 守 `player_command` 里「菜单开着时不走回落分支」这条顺序——
        // 顺序反过来的话，玩家在菜单里挑东西的同时角色会在地图上一路
        // 走动。
        // Arrange
        let mut demo = test_demo();
        arrange_smith(&mut demo);
        frame(&mut demo, 0, &[GameKey::Inventory]);
        let pos_before = player_pos(&demo);
        let clock_before = demo.game_world.world.clock;

        // Act：菜单开着，连按三次「下」。
        for at in 1..4u64 {
            frame(&mut demo, at, &[GameKey::Down]);
        }

        // Assert：角色没动，时钟也没走（移动会消耗一次回合）。
        assert_eq!(player_pos(&demo), pos_before);
        assert_eq!(demo.game_world.world.clock, clock_before);
        assert!(demo.menu.is_open(), "菜单应当仍然开着");
    }

    /// 走**真实生产入口** `on_frame` 跑一帧——不是直接调 `advance`：
    /// 菜单键的消费点、模态屏路由、退出判定全部住在 `on_frame` 里，
    /// `advance` 根本看不见它们。`resources` 为 `None` 时 `on_frame`
    /// 会在渲染那一步提前返回 `Continue`，因此脱离 GPU 也能跑。
    fn 走一帧(demo: &mut Demo, at: u64, keys: &[GameKey]) -> FrameOutcome {
        let mut input = InputState::new();
        for key in keys {
            input.press(*key);
        }
        demo.on_frame(FrameId(at), &mut input)
    }

    #[test]
    fn 按下菜单键打开模态屏且输入上下文切到菜单() {
        // 交接文档第四节第 17 条那条死路径的端到端验收：按 Tab 之前
        // 上下文是 Gameplay，按下之后是 Menu，平台层从此按菜单那张表
        // 解析物理键。
        // Arrange
        let mut demo = test_demo();
        assert_eq!(demo.input_context(), InputContext::Gameplay);

        // Act
        走一帧(&mut demo, 0, &[GameKey::Menu]);

        // Assert
        assert_eq!(demo.screen, Some(ScreenState::Menu));
        assert_eq!(demo.input_context(), InputContext::Menu);
        assert_eq!(demo.ui_modes.depth(), 1);
    }

    #[test]
    fn 模态屏开着时世界时钟与玩家坐标都不动() {
        // 「打开菜单时世界不应继续推进」的直接验收。回合制本来就是
        // 「玩家不提交意图时钟就不走」，但方向键仍然会被
        // `player_command` 读成移动——`advance` 的整段早退才是真正
        // 挡住它的那一步，见该方法里的注释。
        // Arrange
        let mut demo = test_demo();
        走一帧(&mut demo, 0, &[GameKey::Menu]);
        let 开屏后坐标 = player_pos(&demo);
        let 开屏后时钟 = demo.game_world.world.clock;

        // Act：菜单开着，连按十帧方向键与等待键。
        for at in 1..11u64 {
            走一帧(&mut demo, at, &[GameKey::Right]);
            走一帧(&mut demo, at + 100, &[GameKey::Wait]);
        }

        // Assert
        assert_eq!(player_pos(&demo), 开屏后坐标, "角色不该动");
        assert_eq!(demo.game_world.world.clock, 开屏后时钟, "时钟不该走");
        assert!(demo.screen.is_some(), "菜单应当仍然开着");
    }

    #[test]
    fn 模态屏关掉之后世界重新可以推进() {
        // 上一条的另一半：早退不能把游戏永久关死。
        // Arrange
        let mut demo = test_demo();
        走一帧(&mut demo, 0, &[GameKey::Menu]);
        走一帧(&mut demo, 1, &[GameKey::Cancel]);
        let 关屏后时钟 = demo.game_world.world.clock;

        // Act：连按几帧「等待」——第一帧多半还轮不到玩家
        // （`PlayerTurnOutcome::NotYet`，非受控实体先结算），与既有的
        // `连续多次玩家等待后世界时钟真的前进` 那条测试同一个理由。
        for at in 2..8u64 {
            走一帧(&mut demo, at, &[GameKey::Wait]);
        }

        // Assert
        assert!(demo.screen.is_none());
        assert!(demo.game_world.world.clock > 关屏后时钟, "等待应当推进时钟");
    }

    #[test]
    fn 模态屏开着时取消键只关屏不退出游戏() {
        // 与背包菜单那条同型的陷阱：玩家想关个菜单不该直接退出整局。
        // Arrange
        let mut demo = test_demo();
        走一帧(&mut demo, 0, &[GameKey::Menu]);

        // Act
        let outcome = 走一帧(&mut demo, 1, &[GameKey::Cancel]);

        // Assert
        assert_eq!(outcome, FrameOutcome::Continue, "不该退出整局");
        assert!(demo.screen.is_none(), "取消键应当把模态屏关掉");
        assert_eq!(demo.input_context(), InputContext::Gameplay);
    }

    #[test]
    fn 模态屏里选中退出项才真的退出() {
        // Arrange：开菜单，焦点连按三次「下」落到第三项（退出游戏）。
        let mut demo = test_demo();
        走一帧(&mut demo, 0, &[GameKey::Menu]);
        for at in 1..4u64 {
            走一帧(&mut demo, at, &[GameKey::Down]);
        }

        // Act
        let outcome = 走一帧(&mut demo, 4, &[GameKey::Confirm]);

        // Assert
        assert_eq!(outcome, FrameOutcome::Exit);
    }

    #[test]
    fn 模态屏里选中设置项进入设置界面() {
        // Arrange：焦点落到第二项（设置）。
        let mut demo = test_demo();
        走一帧(&mut demo, 0, &[GameKey::Menu]);
        走一帧(&mut demo, 1, &[GameKey::Down]);
        走一帧(&mut demo, 2, &[GameKey::Down]);

        // Act
        走一帧(&mut demo, 3, &[GameKey::Confirm]);

        // Assert
        assert!(matches!(demo.screen, Some(ScreenState::Settings { .. })));
    }

    #[test]
    fn 设置界面里按取消退回菜单屏而不是退出整局() {
        // 这一条比「菜单屏按取消不退出」咬得更紧：菜单屏那一条会关屏，
        // 而关屏顺带 `InputState::clear()`（`UiModeStack::pop` 的语义），
        // 取消键因此被吃掉，即使少一道闸门也看不出问题。设置界面按
        // 取消**不关屏**（只退回菜单屏），取消键的「刚按下」标志原封
        // 不动地留到下面那条退出判定——`self.screen.is_none()` 那道
        // 闸门在这条路径上是真的在挡事。
        // Arrange：进设置界面。
        let mut demo = test_demo();
        走一帧(&mut demo, 0, &[GameKey::Menu]);
        走一帧(&mut demo, 1, &[GameKey::Down]);
        走一帧(&mut demo, 2, &[GameKey::Down]);
        走一帧(&mut demo, 3, &[GameKey::Confirm]);
        assert!(matches!(demo.screen, Some(ScreenState::Settings { .. })));

        // Act
        let outcome = 走一帧(&mut demo, 4, &[GameKey::Cancel]);

        // Assert
        assert_eq!(outcome, FrameOutcome::Continue, "不该退出整局");
        assert_eq!(demo.screen, Some(ScreenState::Menu));
    }

    #[test]
    fn 背包开着时按菜单键不叠第二块模态屏() {
        // 两块模态 UI 叠在一起会立刻引出「Esc 关哪一层」的新裁定，
        // 而没有任何人要求过这件事。
        // Arrange
        let mut demo = test_demo();
        走一帧(&mut demo, 0, &[GameKey::Inventory]);
        assert!(demo.menu.is_open(), "Arrange 应当把背包打开");

        // Act
        走一帧(&mut demo, 1, &[GameKey::Menu]);

        // Assert
        assert!(demo.screen.is_none());
        assert_eq!(demo.input_context(), InputContext::Gameplay);
    }

    #[test]
    fn 打开模态屏时按住的键被视为全部松开() {
        // 设计文档 2.3 节的硬结论：上下文切换是第三种「隐式全键松开」
        // 边界。不清空的话，打开菜单那一刻按着的 W 会带着「已按住」
        // 进菜单，用移动场景的重复计时基准去滚菜单光标。
        // Arrange
        let mut demo = test_demo();
        let mut input = InputState::new();
        input.press(GameKey::Up);
        input.press(GameKey::Menu);

        // Act
        demo.on_frame(FrameId(0), &mut input);

        // Assert
        assert!(!input.is_held(GameKey::Up));
    }

    #[test]
    fn 设置界面改了键位之后新表会被平台层取走() {
        // 真正被解析路径查的表住在平台层；不送回去，玩家会看到「设置
        // 界面里改好了，按下去还是旧的」。
        // Arrange：进设置界面，把光标移到某个键位行并触发一次改动。
        let mut demo = test_demo();
        走一帧(&mut demo, 0, &[GameKey::Menu]);
        走一帧(&mut demo, 1, &[GameKey::Down]);
        走一帧(&mut demo, 2, &[GameKey::Down]);
        走一帧(&mut demo, 3, &[GameKey::Confirm]);
        demo.take_rebound_keys();

        // Act：在设置界面里动一下（哪怕只是移动光标）。
        走一帧(&mut demo, 4, &[GameKey::Down]);

        // Assert
        assert!(demo.take_rebound_keys().is_some());
    }

    #[test]
    fn 菜单开着时取消键只关菜单不退出游戏() {
        // 守 `on_frame` 里那道 `!self.menu.is_open()` 闸门：没有它，玩家
        // 想关个背包会直接退出整局。
        //
        // 本条**必须**走 `AppHandler::on_frame`（不是像其余几条那样直接
        // 调 `advance`）：退不退出这个决定就做在 `on_frame` 里，`advance`
        // 根本看不见它。`on_frame` 在 `resources` 为 `None` 时会在渲染
        // 那一步提前返回 `Continue`（见其实现），因此脱离 GPU 也能跑。
        // Arrange
        let mut demo = test_demo();
        let mut open_inventory = InputState::new();
        open_inventory.press(GameKey::Inventory);
        assert_eq!(
            demo.on_frame(FrameId(0), &mut open_inventory),
            FrameOutcome::Continue
        );
        assert!(demo.menu.is_open(), "Arrange 应当把背包菜单打开");

        // Act
        let mut cancel = InputState::new();
        cancel.press(GameKey::Cancel);
        let outcome = demo.on_frame(FrameId(1), &mut cancel);

        // Assert：没退出，菜单关了。
        assert_eq!(outcome, FrameOutcome::Continue, "不该退出整局");
        assert!(!demo.menu.is_open(), "取消键应当把菜单关掉");

        // 再按一次（子菜单已经关着、模态屏也没开）——**开主菜单，不退出**。
        //
        // 这一半在「顶层取消键改成开主菜单」那次改动里反转过：此前它断言
        // `FrameOutcome::Exit`（按一下 Esc 整局就没了，没有任何确认，
        // 项目所有者实机撞到并要求改掉）。现在 Esc 是逐层往回退，退出的
        // 唯一入口是主菜单里那一项。
        let mut cancel_again = InputState::new();
        cancel_again.press(GameKey::Cancel);
        assert_eq!(
            demo.on_frame(FrameId(2), &mut cancel_again),
            FrameOutcome::Continue,
            "什么都没开时按取消键不该退出整局"
        );
        assert!(
            demo.screen.is_some(),
            "什么都没开时按取消键应当开出主菜单——否则这个键就成了死键，\
             玩家既退不出也进不去菜单"
        );
    }

    #[test]
    fn 不开制作菜单时按确认键不会制作任何东西() {
        // 反例守卫：证明上面那条锻造测试真正依赖的是「开菜单 + 选中
        // 那一行」，不是「按了确认键就会造东西」。
        // Arrange
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);

        // Act：菜单全关着，连按三次确认。
        for at in 0..3u64 {
            frame(&mut demo, at, &[GameKey::Confirm]);
        }

        // Assert
        assert_eq!(carried(&demo, fixture.forge), 0);
        assert_eq!(
            carried(&demo, fixture.iron_ingot),
            8,
            "一块铁锭都不该被吃掉"
        );
    }

    #[test]
    fn 按装备键再按一次就把它卸回背包() {
        // `Intent::Unequip` 的键位产出者——背包菜单里落在「已装备」那
        // 一段的行，见 `crate::player_action` 模块文档「为什么装备与
        // 卸下共用一个键」一节。
        // Arrange：先按键把锤子装上。
        let mut demo = test_demo();
        let fixture = arrange_smith(&mut demo);
        let mut at = 0u64;
        equip_hammer_by_keys(&mut demo, &mut at, &fixture);
        assert_eq!(
            carried(&demo, fixture.smith_hammer),
            0,
            "Arrange 之后锤子应当已经不在背包里"
        );

        // Act：开背包、光标归零后移到「已装备」那一段的锤子上，再按一
        // 次装备键。
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        let equipped_row = inventory_row_of(&demo, fixture.smith_hammer);
        move_cursor_to(&mut demo, &mut at, equipped_row);
        frame(&mut demo, at, &[GameKey::Equip]);

        // Assert
        assert_eq!(
            carried(&demo, fixture.smith_hammer),
            1,
            "再按一次装备键应当把它卸回背包"
        );
        assert!(
            demo.game_world
                .world
                .actors
                .get(demo.game_world.player)
                .expect("玩家仍存在")
                .equipment
                .values()
                .all(|stack| stack.def != fixture.smith_hammer),
            "装备栏里不该还留着这把锤子"
        );
    }

    #[test]
    fn 按使用键能吃掉背包里的一份烤肉() {
        // `Intent::Use` 的键位产出者。
        // Arrange
        let mut demo = test_demo();
        let roast = content_index(&demo, "lostland:roast_meat");
        let player = demo.game_world.player;
        demo.game_world
            .world
            .actors
            .get_mut(player)
            .expect("玩家刚建局")
            .inventory
            .push(ItemStack::new(roast, 2));
        let before = carried(&demo, roast);
        let mut at = 0u64;

        // Act：开背包 → 移到烤肉 → 按使用键。
        frame(&mut demo, at, &[GameKey::Inventory]);
        at += 1;
        let row = inventory_row_of(&demo, roast);
        move_cursor_to(&mut demo, &mut at, row);
        frame(&mut demo, at, &[GameKey::Use]);

        // Assert：恰好少一份——不是「变少了」，是「少了一份」。
        assert_eq!(carried(&demo, roast), before - 1);
    }
}
