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
use ll_render::atlas::{Atlas, AtlasEntry, AtlasMetadata};
use ll_render::batch::{SpriteBatch, SpriteInstance};
use ll_render::camera::{BoundedCamera, Camera};
use ll_render::gpu::GpuContext;
use ll_render::sprite::{DrawOrder, Layer};
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
    INTERIOR_VIEW_CENTER, MINIMAP_CELL_PX, MINIMAP_DOWNSAMPLE, STREAM_RADIUS_ZONES,
    effective_sight_radius, effective_tint, minimap_cell_screen_pos, terrain_entry_name,
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

        Demo {
            demo_world,
            continent_field,
            camera,
            resources: None,
        }
    }

    /// 每帧输入处理：先维护流式邻域，再处理交互键，最后处理移动。
    ///
    /// 流式邻域维护必须排在最前——保证这一帧玩家可能移动到的下一格
    /// 所在区块，在 `resolve`/FOV 真正查询它之前就已经常驻，见
    /// `ll_world::surface_store::SurfaceStore::stream_neighborhood`
    /// 文档「为什么调用方必须在查询视野之前调用这个方法」。
    fn advance(&mut self, input: &InputState) {
        self.maintain_streaming();

        if input.was_just_pressed(GameKey::Confirm) {
            self.try_interact();
        }

        let player = self.demo_world.player;
        if let Some(intent) = intent_from_input(player, input) {
            self.apply_intent(&intent);
        }

        if let Some(agent) = self.demo_world.world.actors.get(player)
            && matches!(agent.current_space, Space::Surface { .. })
        {
            self.camera.center = agent.pos;
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
    push_player_marker(px, py, tint, resources);
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
    push_player_marker(px, py, tint, resources);
    push_minimap(demo_world, continent_field, resources);
}

/// 画出玩家标记（复用图集里的 `hero_idle_0`），地表/`Interior` 共用。
fn push_player_marker(sx: i32, sy: i32, tint: [f32; 4], resources: &mut GpuResources) {
    let Some((entry, uv)) = resources.lookup("hero_idle_0") else {
        return;
    };
    let order = DrawOrder::new(
        Layer::ENTITY,
        sy + entry.sprite_size().height as i32,
        PLAYER_ENTITY,
    );
    resources.batch.push(
        order,
        sprite_instance(sx as f32, sy as f32, entry.sprite_size(), uv, tint),
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
    let cells = continent_map(continent_field, layout, MINIMAP_DOWNSAMPLE);

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

    fn on_frame(&mut self, _frame: FrameId, input: &InputState) -> FrameOutcome {
        if input.was_just_pressed(GameKey::Cancel) {
            return FrameOutcome::Exit;
        }

        self.advance(input);

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        if input.was_just_pressed(GameKey::Screenshot) {
            save_baseline_png(resources, BASELINE_PNG_PATH);
        }

        let current_space = self
            .demo_world
            .world
            .actors
            .get(self.demo_world.player)
            .map(|agent| agent.current_space);

        // render_* 只取 &self.demo_world/&self.continent_field 两个
        // 字段引用，与借出的 resources（&mut self.resources）是三个
        // 互不重叠的字段借用——借用检查器能看出这一点，不需要像
        // p1/p2/p3_acceptance 那样为了绕开「借了整个 self」的假阳性
        // 而拆成自由函数（这里本来就是自由函数，只是连 &self 都不传）。
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
                    resources,
                );
            }
            _ => render_surface(
                &self.demo_world,
                &self.continent_field,
                &self.camera,
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
