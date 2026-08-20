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

use ll_mod::asset_vfs::AssetVfs;
use ll_platform::config::DisplayConfig;
use ll_platform::config::ScaleFilter;
use ll_platform::input::{GameKey, InputState};
use ll_platform::window::{AppHandler, FrameId, FrameOutcome, PhysicalSize, Window};
use ll_render::anim::{AnimStateMachine, Clip, current_sprite_name};
use ll_render::atlas::{Atlas, AtlasEntry};
use ll_render::atlas_pack::{SpriteSource, pack_atlas};
use ll_render::batch::{SpriteBatch, SpriteInstance};
use ll_render::camera::{Camera, Zoom, apply_zoom};
use ll_render::gpu::GpuContext;
use ll_render::sprite::{DrawOrder, Layer, footprint_bottom_screen_y, sprite_draw_position};
use ll_render::target::{BlitFilter, RenderTarget, fit_viewport};
use ll_render::wgpu;
use ll_sim::apply::apply;
use ll_sim::intent::intent_from_input;
use ll_sim::resolve::resolve;
use ll_world::fov::compute_fov;
use ll_world::space::Space;
use ll_world::surface_store::SurfaceWindow;

use crate::animation::{self, FALLBACK_SPRITE};
use crate::content::LoadedContent;
use crate::layout::{effective_sight_radius, effective_tint, terrain_atlas_key, tile_tint};
use crate::save::save_game;
use crate::world::{GameWorld, MAX_SAFE_ZOOM, MIN_SAFE_ZOOM, STREAM_RADIUS_ZONES};

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
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.window_size = size;
    }

    fn present(&self) {
        let frame = match self.gpu.acquire_frame() {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, "跳过本帧的窗口呈现");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let viewport = fit_viewport(self.window_size.width, self.window_size.height);
        self.render_target
            .blit_to(&self.gpu, &view, viewport, self.blit_filter);
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
    /// 玩家精灵的行走/待机两段动画剪辑——下标含义见
    /// [`animation::WALK_CLIP`]/[`animation::IDLE_CLIP`]，构造见
    /// [`animation::player_clips`]。
    clips: Vec<Clip>,
    /// 玩家精灵行走/待机动画状态的生命周期管理：电平驱动
    /// （[`AnimStateMachine::set_level`]），每帧由
    /// [`animation::update_player_animation`] 算出「现在该播放哪个
    /// 状态」——与 `ll-sim` 的 `p5_coordinate_acceptance::Demo::anim`
    /// 同一套接线方式，只是本体二进制这一份是独立的运行期实例。
    anim: AnimStateMachine,
    resources: Option<GpuResources>,
}

impl Demo {
    /// 用已经装载好的内容与已经建好（新游戏或读档得来）的世界构造
    /// 运行期状态——两者都由 [`crate::run_game`] 在事件循环启动前准备好，
    /// 本类型不负责「建世界还是读档」这个决定本身。
    pub fn new(
        content: LoadedContent,
        game_world: GameWorld,
        save_path: PathBuf,
        character_name: String,
        display: DisplayConfig,
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
        let clips = animation::player_clips();
        tracing::info!(
            clip_count = clips.len(),
            walk_clip = animation::WALK_CLIP,
            idle_clip = animation::IDLE_CLIP,
            "玩家动画状态机已装载"
        );
        Demo {
            content,
            game_world,
            camera,
            zoom: Zoom::default(),
            save_path,
            character_name,
            display,
            clips,
            anim: AnimStateMachine::new(animation::IDLE_CLIP, FrameId(0)),
            resources: None,
        }
    }

    /// 每帧输入处理：先维护流式邻域（必须排在移动之前，见
    /// `ll_world::surface_store::SurfaceStore::stream_neighborhood`
    /// 文档），再处理缩放、动画与移动——动画判定只读 `input`（按住
    /// 状态），不依赖本帧是否真的产生了移动意图或移动是否成功（见
    /// [`animation::update_player_animation`] 文档），因此与缩放、移动
    /// 结算互不依赖，顺序先后不影响正确性，这里的排列只是让「本帧
    /// 输入」的处理顺序读起来更顺。
    fn advance(&mut self, input: &InputState, frame: FrameId) {
        self.maintain_streaming();
        self.update_zoom(input);
        animation::update_player_animation(&mut self.anim, input, frame);

        let player = self.game_world.player;
        let intent = intent_from_input(player, input);
        if let Some(intent) = intent {
            let effects = resolve(&self.game_world.world, &intent);
            for effect in &effects {
                apply(&mut self.game_world.world, effect);
            }
        }

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
fn render_surface(
    game_world: &GameWorld,
    content: &LoadedContent,
    camera: &Camera,
    zoom: Zoom,
    sprite_name: &str,
    resources: &mut GpuResources,
) {
    let world = &game_world.world;
    let player_pos = world
        .actors
        .get(game_world.player)
        .map(|agent| agent.pos)
        .unwrap_or(camera.center);
    let profile = space_profile_of(content, world.surface_profile);
    let clock = world.clock;
    let radius = effective_sight_radius(&profile, clock);
    let tint = effective_tint(&profile, clock);
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
            &self.clips,
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
        resources.present();

        FrameOutcome::Continue
    }

    fn on_exit(&mut self) {
        tracing::info!("demo exiting");
        self.save_on_exit();
    }
}
