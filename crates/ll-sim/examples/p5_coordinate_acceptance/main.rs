//! P5 验收 demo：两级坐标系重写（`.superpowers/sdd/2026-08-18-coordinate-system-rewrite`）
//! 的验收 demo，证明四件事：
//!
//! 1. 跨区块边界时地表无缝——世界有 24 个区块，出生点邻域预热只覆盖
//!    其中一部分（见 [`layout::ZONE_COUNT_X`] 文档「为什么不能更小」），
//!    向东走足够远必然跨过一个此前未常驻的区块边界。
//! 2. 世界地图（`continent_map`）上的标记跟着玩家位置更新。
//! 3. 能进出一个 `Interior`（站在入口格按确认键），且只渲染当前层。
//! 4. 层属性生效——`Interior` 的 `exposed_to_sky = false`，有效环境光
//!    与由此推出的视野半径明显小于地表。
//!
//! # 完整调用链
//!
//! 按键 → [`ll_platform::input::InputState`] → [`ll_sim::intent::intent_from_input`]
//! （移动/等待）或本文件 [`Demo::try_interact`]（进出 `Interior`，
//! `intent_from_input` 按设计不产出这个变体，见其文档）→
//! [`ll_sim::resolve::resolve`] → [`ll_sim::effect::Effect`] →
//! [`ll_sim::apply::apply`]（唯一写入口）→ 若跨越区块边界，
//! [`ll_world::surface_store::SurfaceStore::stream_neighborhood`]（任务
//! 14）在移动**之前**已经把下一步可能用到的区块流式加载好 →
//! [`Camera`]/[`BoundedCamera`] 按 `Agent::current_space` 二选一 →
//! [`ll_world::fov::compute_fov`] 重算可见集合 → 世界地图用
//! [`ll_world::overview::ContinentField`]（任务 13）现算，不触发任何
//! 区块生成。
//!
//! 运行：`cargo run -p ll-sim --example p5_coordinate_acceptance`
//! 操作：WASD 移动（方向键在这台机器上的 `SendKeys` 自动化测试里不可靠，
//! 见 P4 交接纪律，因此本 demo 的验收全程用 WASD），Enter/空格
//! （`GameKey::Confirm`）在入口格进入/退出 `Interior`，F2 存基准 PNG，
//! Esc 退出。

mod layout;
mod png;
#[cfg(test)]
mod walkthrough_test;
mod world;

use std::sync::Arc;

use ll_platform::input::{GameKey, InputState};
use ll_platform::logging::init_logging;
use ll_platform::window::{
    AppHandler, FrameId, FrameOutcome, PhysicalSize, Window, WindowConfig, run,
};
use ll_render::anim::{AnimStateMachine, Clip, Playback};
use ll_render::atlas::{Atlas, AtlasEntry, AtlasMetadata};
use ll_render::batch::{SpriteBatch, SpriteInstance};
use ll_render::camera::{BoundedCamera, Camera};
use ll_render::gpu::GpuContext;
use ll_render::sprite::{DrawOrder, Layer, footprint_bottom_screen_y, sprite_draw_position};
use ll_render::target::{RenderTarget, fit_viewport};
use ll_render::wgpu;
use ll_sim::apply::apply;
use ll_sim::intent::{Intent, intent_from_input};
use ll_sim::resolve::resolve;
use ll_world::fov::compute_fov;
use ll_world::overview::{ContinentField, continent_map, generate_continent_field};
use ll_world::space::Space;
use ll_world::surface_store::SurfaceWindow;

use layout::{
    IDLE_BREATHE_FRAMES_PER_STEP, INTERIOR_VIEW_CENTER, MINIMAP_CELL_PX, MINIMAP_DOWNSAMPLE,
    STREAM_RADIUS_ZONES, WALK_EXIT_GRACE_FRAMES, WALK_FRAMES_PER_STEP, effective_sight_radius,
    effective_tint, minimap_cell_screen_pos, terrain_entry_name,
};
use png::save_baseline_png;
use world::{DemoWorld, build_demo_world};

/// 绘制顺序号：玩家标记的固定实体号。
const PLAYER_ENTITY: u64 = 0;
/// 绘制顺序号：地形瓦片的起始偏移。
const TERRAIN_ENTITY_BASE: u64 = 1;
/// 绘制顺序号：小地图格子的起始偏移，远离地形瓦片可能用到的范围。
const MINIMAP_ENTITY_BASE: u64 = 1_000_000;
/// 绘制顺序号：小地图「你在这里」标记。
const MINIMAP_MARKER_ENTITY: u64 = 2_000_000;

/// 行走动画剪辑在 [`Demo::clips`] 里的下标。
const WALK_CLIP: usize = 0;
/// 待机呼吸动画剪辑在 [`Demo::clips`] 里的下标。
const IDLE_CLIP: usize = 1;

/// 玩家精灵缺帧时兜底显示的图集条目。
///
/// 动画剪辑引用的帧（`hero_walk_0`/`hero_walk_1`/`hero_idle_1`）是
/// 「锦上添花」的可选资产——本 demo 内嵌的图集恒定包含它们，但动画帧
/// 名最终来自可被 mod 覆盖的剪辑数据，属于外部不可信输入（见
/// `ll_render::anim` 模块文档「降级而非崩溃」一节）。`hero_idle_0` 是
/// 玩家精灵唯一「必须存在」的一帧，缺了它玩家标记本就画不出来，因此
/// 拿它当兜底：mod 只提供这一帧是完全正常的情况，不该因此报错或让
/// 玩家标记消失。
const FALLBACK_SPRITE: &str = "hero_idle_0";

const ATLAS_JSON: &str = include_str!("../../../../assets/atlas/placeholder.json");
const ATLAS_PNG: &[u8] = include_bytes!("../../../../assets/atlas/placeholder.png");

const BASELINE_PNG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/visual/baseline/p5_coordinate_acceptance.png"
);

/// 存活于 `on_resume` 之后的 GPU 相关资源——与 `p2_acceptance::GpuResources`
/// 同一个拆分理由，见其文档。
struct GpuResources {
    gpu: GpuContext,
    render_target: RenderTarget,
    atlas: Atlas,
    batch: SpriteBatch,
    window_size: PhysicalSize<u32>,
}

impl GpuResources {
    fn new(window: Arc<Window>, size: PhysicalSize<u32>) -> GpuResources {
        let gpu = GpuContext::new(window, size).expect("demo 环境应能取得可用的图形适配器");
        let render_target = RenderTarget::new(&gpu);
        let metadata = AtlasMetadata::parse(ATLAS_JSON).expect("内嵌图集元数据应为合法 JSON");
        let atlas =
            Atlas::load(&gpu, metadata, ATLAS_PNG).expect("内嵌图集资源应能上传为 GPU 纹理");
        let batch = SpriteBatch::new(&gpu, &atlas, render_target.format());
        GpuResources {
            gpu,
            render_target,
            atlas,
            batch,
            window_size: size,
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
        self.render_target.blit_to(&self.gpu, &view, viewport);
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

/// P5 验收 demo 的完整状态。
struct Demo {
    demo_world: DemoWorld,
    /// 世界创建时一次性生成的粗粒度地形场（任务 13），专供小地图使用。
    continent_field: ContinentField,
    /// 地表相机——`Agent.current_space` 是 `Interior` 时不使用它（改用
    /// 每帧现算的固定 `BoundedCamera`，见 `layout::INTERIOR_VIEW_CENTER`
    /// 文档），但字段本身继续保留玩家离开地表前最后的位置，退出后相机
    /// 立刻重新跟随玩家（玩家的 `pos` 在 Interior 内不变，见
    /// `ll_world::entity::Agent::current_space` 文档）。
    camera: Camera,
    /// 玩家精灵的两段（未来可扩展更多）动画剪辑：下标 [`WALK_CLIP`] 是
    /// 行走、[`IDLE_CLIP`] 是待机呼吸。具体维护哪些剪辑、每种剪辑对应
    /// 哪个触发式状态由这里（调用方）决定，[`AnimStateMachine`] 本身
    /// 不关心——将来新增攻击/施法/受击/死亡时，只需要在这个表里再加
    /// 一段 `Clip`。
    clips: Vec<Clip>,
    /// 玩家精灵触发式动画状态的生命周期管理：收到移动事件后维持
    /// [`Clip::exit_grace_frames`]（见 [`layout::WALK_EXIT_GRACE_FRAMES`]）
    /// 帧，期间即使没有新的移动事件也不回落到待机，见
    /// [`AnimStateMachine`] 模块文档「要解决的问题」。这正是修复「走
    /// 一格闪一下」缺陷的落点——此前直接用 `Playback` 绑定「本帧是否
    /// 有移动意图」，瞬时信号驱动导致状态没有自己的生命周期，见
    /// [`Demo::update_walk_animation`] 文档。
    anim: AnimStateMachine,
    resources: Option<GpuResources>,
}

impl Demo {
    fn new() -> Demo {
        let demo_world = build_demo_world();
        let player_pos = demo_world
            .world
            .actors
            .get(demo_world.player)
            .expect("玩家刚生成，必然存在")
            .pos;
        let camera = Camera {
            center: player_pos,
            world: demo_world.world.size,
        };
        let continent_field = generate_continent_field(
            demo_world.world.terrain.layout(),
            &demo_world.noise,
            &demo_world.params,
            &demo_world.terrain_ids,
        );
        tracing::info!(
            spawn = ?player_pos,
            interior_id = ?demo_world.interior_id,
            interior_anchor = ?demo_world.interior_anchor,
            "demo world ready"
        );

        // 行走剪辑：走姿 0 -> 立姿 -> 走姿 1 -> 立姿，与
        // `p1_acceptance::Demo::new` 的 `walk_clip` 逐字同构（同一套
        // 图集帧，没有理由播放节奏不一样）；用立姿做两个走姿之间的
        // 过渡帧，避免两张走姿贴图直接互跳显得生硬。
        let walk_clip = Clip {
            frames: vec![
                "hero_walk_0".to_string(),
                FALLBACK_SPRITE.to_string(),
                "hero_walk_1".to_string(),
                FALLBACK_SPRITE.to_string(),
            ],
            frames_per_step: WALK_FRAMES_PER_STEP,
            looping: true,
            // 见 `layout::WALK_EXIT_GRACE_FRAMES` 文档「为什么是 12」
            // ——这是项目所有者要的可自定义延迟，覆盖按键自动重复脉冲
            // 之间的空档，避免连续移动时闪回待机。
            exit_grace_frames: WALK_EXIT_GRACE_FRAMES,
        };
        // 待机呼吸剪辑：只在待机图与「吸气」图之间缓慢往返，幅度克制
        // （见 `assets/atlas/placeholder.json` 里 `hero_idle_1` 与
        // `hero_idle_0` 的差异，只挪了 1 像素）。
        let idle_clip = Clip {
            frames: vec![FALLBACK_SPRITE.to_string(), "hero_idle_1".to_string()],
            frames_per_step: IDLE_BREATHE_FRAMES_PER_STEP,
            looping: true,
            // 待机是 `AnimStateMachine` 的默认状态，从不「过期」，这个
            // 字段在这里从不被读取（见 `Clip::exit_grace_frames` 文档）。
            exit_grace_frames: 0,
        };

        Demo {
            demo_world,
            continent_field,
            camera,
            clips: vec![walk_clip, idle_clip],
            anim: AnimStateMachine::new(IDLE_CLIP, FrameId(0)),
            resources: None,
        }
    }

    /// 每帧输入处理：先维护流式邻域，再处理交互键，最后处理移动。
    ///
    /// 流式邻域维护必须排在最前——保证这一帧玩家可能移动到的下一格
    /// 所在区块，在 `resolve`/FOV 真正查询它之前就已经常驻，见
    /// `ll_world::surface_store::SurfaceStore::stream_neighborhood`
    /// 文档「为什么调用方必须在查询视野之前调用这个方法」。
    fn advance(&mut self, input: &InputState, frame: FrameId) {
        self.maintain_streaming();

        if input.was_just_pressed(GameKey::Confirm) {
            self.try_interact();
        }

        let player = self.demo_world.player;
        let intent = intent_from_input(player, input);
        // 「有没有移动意图」而非「这一步是否真的挪动了位置」——按住方向
        // 键顶着墙走不动时仍应播放行走动画（这也是绝大多数游戏的直觉：
        // 角色原地踏步，而不是因为撞墙就悄悄切回待机姿势），且不需要
        // 读 `resolve`/`apply` 的结果来判断，输入层这一步信息已经够用。
        self.update_walk_animation(intent.as_ref(), frame);
        if let Some(intent) = intent {
            self.apply_intent(&intent);
        }

        if let Some(agent) = self.demo_world.world.actors.get(player)
            && matches!(agent.current_space, Space::Surface { .. })
        {
            self.camera.center = agent.pos;
        }
    }

    /// 推进玩家行走/待机的触发式动画状态：先老化当前状态（可能因为
    /// 超过余韵而回落到待机），再看本帧有没有新的移动意图，有就触发
    /// （或续期）行走状态。
    ///
    /// # 为什么不能再用「本帧是否有移动意图」直接驱动
    ///
    /// 此前这里直接比较「本帧是否有移动意图」与上一帧的记录，一旦状态
    /// 变化就重建 `Playback`——但回合制的移动意图本身只在按键刚按下、
    /// 或自动重复脉冲触发的那一帧才存在（见
    /// `ll_platform::input::InputState` 模块文档「为什么要区分『按住』
    /// 与『刚按下』」），脉冲之间的空档没有意图，直接绑定意味着状态会
    /// 在每个空档都先弹回待机、下一个脉冲再弹回行走——这正是项目所有者
    /// 报告的「走一格闪一下」。现在改用 [`AnimStateMachine`]：意图存在
    /// 的那一帧触发/续期行走状态，`update` 每帧无条件调用负责老化，
    /// 只有真的超过 [`Clip::exit_grace_frames`] 声明的余韵仍没有新意图
    /// 才回落待机——这段余韵就是项目所有者要的可自定义延迟。
    ///
    /// `update` 必须排在 `trigger` 之前：若顺序反过来，刚触发本状态就
    /// 立刻用当前帧检查过期，`exit_grace_frames = 0`（例如未来某个一
    /// 次性状态刻意不要余韵）时会导致触发的这一帧都观察不到目标状态；
    /// 先老化上一帧遗留的状态、再处理本帧的新触发，触发本身产生的新
    /// 状态才总能在触发的这一帧被观察到。
    fn update_walk_animation(&mut self, intent: Option<&Intent>, frame: FrameId) {
        self.anim.update(frame);
        if matches!(intent, Some(Intent::Move { .. })) {
            self.anim.trigger(&self.clips, WALK_CLIP, frame);
        }
    }

    fn maintain_streaming(&mut self) {
        let player = self.demo_world.player;
        let Some(agent) = self.demo_world.world.actors.get(player) else {
            return;
        };
        if !matches!(agent.current_space, Space::Surface { .. }) {
            return;
        }
        let pos = agent.pos;
        let clock = self.demo_world.world.clock;
        self.demo_world.world.terrain.stream_neighborhood(
            &self.demo_world.noise,
            &self.demo_world.params,
            &self.demo_world.terrain_ids,
            pos,
            STREAM_RADIUS_ZONES,
            clock,
        );
    }

    fn apply_intent(&mut self, intent: &Intent) {
        let effects = resolve(&self.demo_world.world, intent);
        for effect in &effects {
            apply(&mut self.demo_world.world, effect);
        }
    }

    /// 站在入口格按确认键进入 `Interior`；已在 `Interior` 内按确认键
    /// 退出——`intent_from_input` 按设计不产出 `EnterSpace`/`ExitSpace`
    /// （见 `ll_sim::intent` 模块文档），这两个意图具体喂哪个目标由
    /// 已经知道场景细节的调用方（这里）决定。
    fn try_interact(&mut self) {
        let player = self.demo_world.player;
        let Some(agent) = self.demo_world.world.actors.get(player) else {
            return;
        };
        let intent = match agent.current_space {
            Space::Surface { .. } => self
                .demo_world
                .world
                .interiors
                .entries_at(agent.pos)
                .first()
                .map(|&target| Intent::EnterSpace {
                    actor: player,
                    target,
                }),
            Space::Interior { .. } => Some(Intent::ExitSpace { actor: player }),
        };
        let Some(intent) = intent else {
            tracing::info!("interact：当前位置没有可进入的 Interior 入口");
            return;
        };
        self.apply_intent(&intent);

        if let Some(agent) = self.demo_world.world.actors.get(player) {
            let space = agent.current_space;
            let profile = self.demo_world.profile_of(space);
            let clock = self.demo_world.world.clock;
            let light = ll_world::space_profile::effective_ambient_light(&profile, clock);
            let radius = effective_sight_radius(&profile, clock);
            tracing::info!(?space, light = light.0, radius, "空间切换完成");
        }
    }
}

/// 把地表世界画到离屏目标：地形（仅可见格）、玩家标记、小地图。
///
/// 只取 `&DemoWorld`/`&ContinentField`/`&Camera` 三个字段引用而不是
/// `&Demo` 整体——`on_frame` 需要同时持有对 `self.resources` 的
/// `&mut` 借用与对其余字段的 `&` 借用，取自由函数 + 拆开的字段引用是
/// 让借用检查器能看出这些借用互不重叠的唯一办法（与
/// `p1_acceptance::collect_sprites` 同样的理由，见其文档）。
fn render_surface(
    demo_world: &DemoWorld,
    continent_field: &ContinentField,
    camera: &Camera,
    sprite_name: &str,
    resources: &mut GpuResources,
) {
    let world = &demo_world.world;
    let terrain_ids = &demo_world.terrain_ids;
    let player_pos = world
        .actors
        .get(demo_world.player)
        .map(|agent| agent.pos)
        .unwrap_or(camera.center);
    let profile = demo_world.profile_of(Space::surface(
        world.terrain.layout().tile_to_zone(player_pos).0,
        demo_world.space_ids.surface,
    ));
    let clock = world.clock;
    let radius = effective_sight_radius(&profile, clock);
    let tint = effective_tint(&profile, clock);

    let visible = compute_fov(
        &SurfaceWindow::new(&world.terrain),
        &world.terrain_table,
        player_pos,
        radius,
    );

    let world_width = world.size.width() as u64;
    for pos in camera.visible_tiles() {
        if !visible.contains(pos) {
            continue;
        }
        // 靠近流式邻域边缘时，极端情况下相机视口可能探出常驻半径——
        // 优雅跳过而非 panic，保持画面「未生成的地方留黑」而不是崩溃。
        let Some(kind) = world.terrain_at(pos) else {
            continue;
        };
        let Some(name) = terrain_entry_name(kind, terrain_ids) else {
            continue;
        };
        let Some((entry, uv)) = resources.lookup(name) else {
            continue;
        };
        let (sx, sy) = camera.world_to_screen(pos);
        let order = DrawOrder::new(
            Layer::TERRAIN,
            sy,
            TERRAIN_ENTITY_BASE + pos.y() as u64 * world_width + pos.x() as u64,
        );
        resources.batch.push(
            order,
            sprite_instance(sx as f32, sy as f32, entry.sprite_size(), uv, tint),
        );
    }

    let (px, py) = camera.world_to_screen(player_pos);
    push_player_marker(px, py, sprite_name, tint, resources);
    push_minimap(demo_world, continent_field, resources);
}

/// 把当前所在 `Interior` 楼层画到离屏目标——只画这一层，地表不可见
/// （验收点③）。参数拆分理由同 [`render_surface`]。
fn render_interior(
    demo_world: &DemoWorld,
    continent_field: &ContinentField,
    id: ll_world::space::SpaceId,
    floor: i16,
    profile: ll_world::space_profile::SpaceProfile,
    sprite_name: &str,
    resources: &mut GpuResources,
) {
    let world = &demo_world.world;
    let terrain_ids = &demo_world.terrain_ids;
    let Some(interior) = world.interiors.get(id) else {
        tracing::error!(?id, "当前 current_space 指向的 Interior 实例不存在");
        return;
    };
    let Some(grid) = interior.floor(floor) else {
        tracing::error!(?id, floor, "当前 current_space 指向的楼层不存在");
        return;
    };
    let clock = world.clock;
    let radius = effective_sight_radius(&profile, clock);
    let tint = effective_tint(&profile, clock);

    let center = grid
        .size()
        .try_pos(INTERIOR_VIEW_CENTER.0, INTERIOR_VIEW_CENTER.1)
        .expect("INTERIOR_VIEW_CENTER 落在楼层范围内");
    let camera = BoundedCamera {
        center,
        world: grid.size(),
    };
    let visible = compute_fov(grid, &world.terrain_table, center, radius);

    let floor_width = grid.size().width() as u64;
    for pos in camera.visible_tiles() {
        if !visible.contains(pos) {
            continue;
        }
        let kind = grid.terrain_at(pos);
        let Some(name) = terrain_entry_name(kind, terrain_ids) else {
            continue;
        };
        let Some((entry, uv)) = resources.lookup(name) else {
            continue;
        };
        let (sx, sy) = camera.world_to_screen(pos);
        let order = DrawOrder::new(
            Layer::TERRAIN,
            sy,
            TERRAIN_ENTITY_BASE + pos.y() as u64 * floor_width + pos.x() as u64,
        );
        resources.batch.push(
            order,
            sprite_instance(sx as f32, sy as f32, entry.sprite_size(), uv, tint),
        );
    }

    let (px, py) = camera.world_to_screen(center);
    push_player_marker(px, py, sprite_name, tint, resources);
    push_minimap(demo_world, continent_field, resources);
}

/// 画出玩家标记，地表/`Interior` 共用。
///
/// `sprite_name` 是当前动画帧应显示的图集条目名（由 `on_frame` 通过
/// [`Demo::playback`] 现算，缺帧时已经退回 [`FALLBACK_SPRITE`]，见
/// [`resolve_player_sprite_name`]），不再硬编码 `"hero_idle_0"`——这正是
/// 「接上行走/待机动画」这条修复的落点：此前这里恒定画同一帧，
/// `hero_walk_0`/`hero_walk_1` 从未被显示过。
///
/// `sx`/`sy` 是**占地格左上角**的屏幕坐标（`Camera::world_to_screen`/
/// `BoundedCamera::world_to_screen` 的返回值），不是精灵图像左上角
/// ——这两者的换算必须走 [`sprite_draw_position`]，不能像本函数曾经
/// 那样直接把 `(sx, sy)` 当绘制原点：`hero_idle_0` 是 16×24、
/// [`ll_render::sprite::Pivot`] 是 `(8, 24)`，直接拿 `(sx, sy)` 当绘制
/// 原点会让图像从占地格顶部往下多画出 8 像素、脚底凸出格子下方，而
/// 不是头顶探出格子上方——这正是项目所有者实机操作时发现的缺陷，
/// 详见 [`sprite_draw_position`] 文档「为什么高出格子的部分向上溢出，
/// 而不是向下」一节。
fn push_player_marker(
    sx: i32,
    sy: i32,
    sprite_name: &str,
    tint: [f32; 4],
    resources: &mut GpuResources,
) {
    let Some((entry, uv)) = resources.lookup(sprite_name) else {
        return;
    };
    let footprint = entry.footprint;
    let [px, py] = sprite_draw_position((sx, sy), footprint, entry.pivot);
    let order = DrawOrder::new(
        Layer::ENTITY,
        footprint_bottom_screen_y(sy, footprint.height),
        PLAYER_ENTITY,
    );
    resources.batch.push(
        order,
        sprite_instance(px, py, entry.sprite_size(), uv, tint),
    );
}

/// 画出左上角的世界地图（[`continent_map`] 的区块级概览，任务 13）与
/// 玩家当前位置标记（验收点②：标记跟着位置更新）。
///
/// 小地图不套用光照色调——理由同 `p2_acceptance::push_minimap`。
fn push_minimap(
    demo_world: &DemoWorld,
    continent_field: &ContinentField,
    resources: &mut GpuResources,
) {
    let world = &demo_world.world;
    let terrain_ids = &demo_world.terrain_ids;
    let layout = world.terrain.layout();
    let cols = layout.zone_count().width().div_ceil(MINIMAP_DOWNSAMPLE);
    let cells = continent_map(
        continent_field,
        layout,
        &world.exploration,
        MINIMAP_DOWNSAMPLE,
    );

    for (index, cell) in cells.iter().enumerate() {
        let col = index as u32 % cols;
        let row = index as u32 / cols;
        let Some(name) = terrain_entry_name(cell.terrain, terrain_ids) else {
            continue;
        };
        let Some((_, uv)) = resources.lookup(name) else {
            continue;
        };
        let (x, y) = minimap_cell_screen_pos(col, row);
        let order = DrawOrder::new(Layer::UI, 0, MINIMAP_ENTITY_BASE + index as u64);
        resources.batch.push(
            order,
            SpriteInstance {
                position: [x as f32, y as f32],
                size: [MINIMAP_CELL_PX as f32, MINIMAP_CELL_PX as f32],
                uv_rect: uv,
                color: [1.0, 1.0, 1.0, 1.0],
            },
        );
    }

    // 「你在这里」标记：玩家当前所在的世界坐标（Interior 内也恒是
    // 入口锚点，见 Agent::current_space 文档）换算出的区块坐标，用
    // hero_idle_0 的轮廓叠一层红色 tint——不管底下那格地形是什么，
    // 都能一眼认出这是玩家标记而非地形本身。
    if let Some(agent) = world.actors.get(demo_world.player)
        && let Some((_, uv)) = resources.lookup("hero_idle_0")
    {
        let (zone, _) = layout.tile_to_zone(agent.pos);
        let col = zone.x() as u32 / MINIMAP_DOWNSAMPLE;
        let row = zone.y() as u32 / MINIMAP_DOWNSAMPLE;
        let (x, y) = minimap_cell_screen_pos(col, row);
        let order = DrawOrder::new(Layer::UI, 1, MINIMAP_MARKER_ENTITY);
        resources.batch.push(
            order,
            SpriteInstance {
                position: [x as f32, y as f32],
                size: [MINIMAP_CELL_PX as f32, MINIMAP_CELL_PX as f32],
                uv_rect: uv,
                color: [1.0, 0.15, 0.15, 1.0],
            },
        );
    }
}

fn sprite_instance(
    x: f32,
    y: f32,
    size: ll_render::sprite::SpriteSize,
    uv_rect: [f32; 4],
    color: [f32; 4],
) -> SpriteInstance {
    SpriteInstance {
        position: [x, y],
        size: [size.width as f32, size.height as f32],
        uv_rect,
        color,
    }
}

/// 若 `frame_name` 在图集里查得到就原样用它，否则退回 `fallback`。
///
/// 这是「动画帧缺失时优雅退回」这条要求真正落地的地方，且刻意只依赖
/// [`AtlasMetadata`]（纯数据，`AtlasMetadata::parse` 不需要 GPU）而不是
/// 整个 [`GpuResources`]——这样它可以脱离窗口/图形适配器被单测直接
/// 覆盖，不需要真的起一个 GPU 上下文才能验证「缺帧退回静态图」这条
/// 行为。
///
/// 不直接调用 `GpuResources::lookup(frame_name)` 再在失败时回退：那样
/// 每次「mod 只提供了部分帧」这种完全正常的情况都会先触发一条
/// `tracing::error!`（见 `GpuResources::lookup` 文档），日志会被刷屏；
/// 这里先用 `AtlasMetadata::lookup` 静默探测存在性，只有连 `fallback`
/// 本身都查不到时，才会在调用方最终的 `resources.lookup` 里触发那条
/// 错误日志——那已经是资产整体损坏，值得被记下来。
fn resolve_player_sprite_name<'a>(
    metadata: &AtlasMetadata,
    frame_name: &'a str,
    fallback: &'a str,
) -> &'a str {
    if metadata.lookup(frame_name).is_some() {
        frame_name
    } else {
        fallback
    }
}

/// 算出 `frame` 这一帧玩家精灵应显示的图集条目名，两层兜底叠加：
///
/// 1. [`Playback::current_frame`] 对损坏的剪辑数据（空剪辑、剪辑下标
///    越界，见 `ll_render::anim` 模块文档「降级而非崩溃」）返回
///    [`None`]，这里退回 [`FALLBACK_SPRITE`]。
/// 2. 就算剪辑给出了一个帧名，那一帧也可能不在图集里（mod 只提供
///    部分帧是正常情况），再用 [`resolve_player_sprite_name`] 确认。
///
/// 两层兜底都用同一个 `FALLBACK_SPRITE`，因为它是玩家精灵唯一「必须
/// 存在」的一帧（见其文档）。
fn current_player_sprite_name<'a>(
    playback: &Playback,
    clips: &'a [Clip],
    frame: FrameId,
    metadata: &AtlasMetadata,
) -> &'a str {
    let raw = playback
        .current_frame(clips, frame)
        .unwrap_or(FALLBACK_SPRITE);
    resolve_player_sprite_name(metadata, raw, FALLBACK_SPRITE)
}

impl AppHandler for Demo {
    fn on_resume(&mut self, window: Arc<Window>, size: PhysicalSize<u32>) {
        tracing::info!(width = size.width, height = size.height, "window resumed");
        self.resources = Some(GpuResources::new(window, size));
    }

    fn on_resize(&mut self, size: PhysicalSize<u32>) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.resize(size);
    }

    fn on_frame(&mut self, frame: FrameId, input: &InputState) -> FrameOutcome {
        if input.was_just_pressed(GameKey::Cancel) {
            return FrameOutcome::Exit;
        }

        self.advance(input, frame);

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        if input.was_just_pressed(GameKey::Screenshot) {
            save_baseline_png(resources, BASELINE_PNG_PATH);
        }

        // 当前动画帧应显示的图集条目名，两层兜底见
        // `current_player_sprite_name` 文档；两层都失败时（连
        // `FALLBACK_SPRITE` 本身都缺失）才会在 `GpuResources::lookup`
        // 里记一条错误日志——那已经是资产整体损坏，不再是「可选帧
        // 缺失」。
        let sprite_name = current_player_sprite_name(
            self.anim.playback(),
            &self.clips,
            frame,
            resources.atlas.metadata(),
        );

        let current_space = self
            .demo_world
            .world
            .actors
            .get(self.demo_world.player)
            .map(|agent| agent.current_space);

        // render_* 只取 &self.demo_world/&self.continent_field/sprite_name
        // 三个只读引用，与借出的 resources（&mut self.resources）互不
        // 重叠——借用检查器能看出这一点，不需要像 p1/p2/p3_acceptance
        // 那样为了绕开「借了整个 self」的假阳性而拆成自由函数（这里本来
        // 就是自由函数，只是连 &self 都不传）。
        match current_space {
            Some(Space::Interior {
                id,
                floor,
                anchor,
                profile,
            }) => {
                let profile_full = self.demo_world.profile_of(Space::Interior {
                    id,
                    floor,
                    anchor,
                    profile,
                });
                render_interior(
                    &self.demo_world,
                    &self.continent_field,
                    id,
                    floor,
                    profile_full,
                    sprite_name,
                    resources,
                );
            }
            _ => render_surface(
                &self.demo_world,
                &self.continent_field,
                &self.camera,
                sprite_name,
                resources,
            ),
        }

        resources
            .batch
            .flush(&resources.gpu, resources.render_target.view());
        resources.present();

        FrameOutcome::Continue
    }

    fn on_exit(&mut self) {
        tracing::info!("demo exiting");
    }
}

fn main() {
    init_logging(true).expect("首次初始化日志不应失败");
    tracing::info!(
        "P5 acceptance demo: WASD moves (arrow keys unreliable under SendKeys automation, \
         see P4 handoff), Enter/Space enters or exits an Interior while standing on its \
         entrance tile, F2 saves baseline PNG, Esc quits."
    );

    let demo = Demo::new();
    if let Err(error) = run(WindowConfig::default(), demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}

#[cfg(test)]
mod animation_fallback_tests {
    use super::*;

    fn embedded_metadata() -> AtlasMetadata {
        AtlasMetadata::parse(ATLAS_JSON).expect("内嵌图集元数据应为合法 JSON")
    }

    #[test]
    fn 动画帧在图集里存在时按原样使用() {
        // Arrange
        let metadata = embedded_metadata();

        // Act
        let resolved = resolve_player_sprite_name(&metadata, "hero_walk_0", FALLBACK_SPRITE);

        // Assert
        assert_eq!(resolved, "hero_walk_0");
    }

    #[test]
    fn 动画帧在图集里缺失时退回兜底帧() {
        // 模拟 mod 覆盖图集后没有提供某一帧行走动画——这是完全正常的
        // 情况（见 FALLBACK_SPRITE 文档），必须退回兜底帧，而不是画出
        // 空白或让调用方 panic。
        // Arrange
        let metadata = embedded_metadata();

        // Act
        let resolved =
            resolve_player_sprite_name(&metadata, "hero_walk_missing_from_mod", FALLBACK_SPRITE);

        // Assert
        assert_eq!(resolved, FALLBACK_SPRITE);
    }

    #[test]
    fn 剪辑数据损坏时退回兜底帧而不是崩溃() {
        // 模拟一段损坏的动画数据：`Playback` 引用的剪辑下标越界（例如
        // mod 打包时漏掉了某段剪辑定义）——`Playback::current_frame`
        // 对此返回 None（`ll_render::anim` 自身的测试已覆盖这一保证），
        // 这里锁住「调用方在此基础上还能优雅退回静态图」这一步。
        // Arrange
        let metadata = embedded_metadata();
        let clips = vec![Clip {
            frames: vec!["hero_walk_0".to_string()],
            frames_per_step: WALK_FRAMES_PER_STEP,
            looping: true,
            exit_grace_frames: 0, // 本测试只验证 `Playback` 本身的降级，不涉及状态机
        }];
        let corrupted_playback = Playback::new(99, FrameId(0));

        // Act
        let resolved =
            current_player_sprite_name(&corrupted_playback, &clips, FrameId(0), &metadata);

        // Assert
        assert_eq!(resolved, FALLBACK_SPRITE);
    }

    #[test]
    fn 剪辑数据完好时按剪辑当前帧显示() {
        // Arrange
        let metadata = embedded_metadata();
        let clips = vec![Clip {
            frames: vec!["hero_walk_0".to_string()],
            frames_per_step: WALK_FRAMES_PER_STEP,
            looping: true,
            exit_grace_frames: 0, // 本测试只验证 `Playback` 本身的降级，不涉及状态机
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act
        let resolved = current_player_sprite_name(&playback, &clips, FrameId(0), &metadata);

        // Assert
        assert_eq!(resolved, "hero_walk_0");
    }
}
