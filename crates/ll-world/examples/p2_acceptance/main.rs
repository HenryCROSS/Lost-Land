//! P2 验收 Demo：把真实生成的环面大陆、FOV 视野、昼夜四季光照全部画到
//! 屏幕上。这是 P2 阶段的最后一个任务——P0/P1 打好的环面坐标、噪声、
//! 分块地形、阴影投射视野、昼夜四季、渲染管线，第一次在同一个画面里
//! 全部接上。
//!
//! 六件事同时发生，任何一件坏了都该一眼看出来，而不必去读代码：
//!
//! 1. 一张真实生成的环面地形（512×320 格，`ll_world::generate::generate_terrain`
//!    的默认参数），能看出水/沙/草/林/丘/山/雪的分布。
//! 2. 走到世界边缘时地形无缝绕回——玩家移动与相机换算全程只走
//!    `TorusSize::wrap`/`TorusSize::delta`，不手写任何取模或钳制，
//!    绕回与否与噪声/生成入口自身的接缝测试（`ll-world` 的
//!    `noise_blackbox.rs`/`generate.rs` 测试）验证的是同一份实现。
//! 3. 视野随移动更新，墙后（本 demo 用天然阻挡视线的山地充当，见
//!    [`spawn`] 模块文档）的格子不可见——每帧用玩家当前位置重新调用
//!    `ll_world::fov::compute_fov`，不缓存。
//! 4. 按等待键（`.`）推进一小时、按确认键（Enter/空格）推进一个季节：
//!    画面随昼夜变暗变亮（[`layout::ambient_tint`] 的亮度分量）、随
//!    季节变色（同一函数的色相分量）。
//! 5. 小地图（`ll_world::overview::continent_map` 的下采样概览）显示
//!    在左上角。
//! 6. 按 F2 存图，作为 P2 的视觉回归基准。
//!
//! 运行：`cargo run -p ll-world --example p2_acceptance`
//! 操作：方向键/WASD 移动玩家（相机跟随），`.` 推进一小时，
//! Enter/空格推进一个季节，F2 存基准 PNG，Esc 退出。
//!
//! # 完整调用链
//!
//! 从按键到出图的每一步都能从上一步取到参数，详见
//! `.superpowers/sdd/2026-08-17-p2-world-terrain/task-8-brief.md` 的
//! 自查表；本文件的实现与那张表逐项对应。
//!
//! # 文件拆分
//!
//! [`layout`] 放不依赖 GPU、也不改动世界数据的纯呈现计算（地形→图集
//! 条目名、光照色调、小地图版式、精灵锚点换算）；[`spawn`] 放会改动
//! `ChunkGrid` 内容的出生点搜索与山脊雕刻；[`png`] 放基准 PNG 落盘；
//! 本文件只留 GPU 资源装配、`Demo` 状态与
//! [`ll_platform::window::AppHandler`] 接线——与 `ll-render` 的
//! `p1_acceptance` 同样的拆分理由，见其模块文档。

mod layout;
mod png;
mod spawn;

use layout::{
    BASE_SIGHT_RADIUS, CLOCK_STEP_HOUR, CLOCK_STEP_SEASON, INITIAL_CLOCK_TICKS, MINIMAP_CELL_PX,
    MINIMAP_DOWNSAMPLE, WORLD_HEIGHT, WORLD_WIDTH, ambient_tint, footprint_bottom_screen_y,
    minimap_cell_screen_pos, sprite_draw_position, terrain_entry_name,
};
use ll_core::torus::{TorusPos, TorusSize};
use ll_platform::input::{GameKey, InputState};
use ll_platform::logging::init_logging;
use ll_platform::window::{
    AppHandler, FrameId, FrameOutcome, PhysicalSize, Window, WindowConfig, run,
};
use ll_render::atlas::{Atlas, AtlasEntry, AtlasMetadata};
use ll_render::batch::{SpriteBatch, SpriteInstance};
use ll_render::camera::Camera;
use ll_render::gpu::GpuContext;
use ll_render::sprite::{DrawOrder, Footprint, Layer, SpriteSize};
use ll_render::target::{RenderTarget, fit_viewport};
// 走 ll_render 重新导出的 wgpu，不直接依赖 wgpu crate 本身，理由与
// p1_acceptance 完全一致：独立 crate 的下游只有这一条路径能用。
use ll_render::wgpu;
use ll_world::fov::{VisibleSet, compute_fov};
use ll_world::generate::GenParams;
use ll_world::light::{ambient_light, sight_radius_at};
use ll_world::overview::continent_map;
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use png::save_baseline_png;
use std::sync::Arc;

/// 绘制顺序号：玩家标记的固定实体号。
const PLAYER_ENTITY: u64 = 0;

/// 绘制顺序号：地形瓦片的起始偏移，避开 [`PLAYER_ENTITY`] 这个保留号。
const TERRAIN_ENTITY_BASE: u64 = 1;

/// 绘制顺序号：小地图格子的起始偏移，远大于地形瓦片可能用到的最大值
/// （`WORLD_WIDTH * WORLD_HEIGHT + TERRAIN_ENTITY_BASE` ≈ 164000），
/// 避免与地形瓦片的实体号撞车——虽然不同图层之间撞车本身无害（见
/// `ll_render::sprite::DrawOrder` 文档：图层是第一排序键），分开取值
/// 范围仍然方便调试时按数值段判断一个绘制顺序号来自哪一类精灵。
const MINIMAP_ENTITY_BASE: u64 = 1_000_000;

/// 图集元数据 JSON，编译期内嵌，不依赖运行时工作目录。
const ATLAS_JSON: &str = include_str!("../../../../assets/atlas/placeholder.json");
/// 图集图片字节，编译期内嵌，理由同上。
const ATLAS_PNG: &[u8] = include_bytes!("../../../../assets/atlas/placeholder.png");

/// 视觉回归基准 PNG 的落盘路径。
const BASELINE_PNG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/visual/baseline/p2_acceptance.png"
);

/// 存活于 `on_resume` 之后的 GPU 相关资源，与不依赖 GPU 的世界/相机
/// 状态分开存放，让 [`Demo`] 在窗口就绪前也能被构造与单测——与
/// `p1_acceptance::GpuResources` 同样的拆分理由。
struct GpuResources {
    gpu: GpuContext,
    render_target: RenderTarget,
    atlas: Atlas,
    batch: SpriteBatch,
    /// 最近一次已知的窗口物理尺寸，`on_frame` 据此算 [`ll_render::target::Viewport`]
    /// ——`on_frame` 本身收不到窗口尺寸，只能由 `on_resume`/`on_resize`
    /// 更新后存在这里。
    window_size: PhysicalSize<u32>,
}

impl GpuResources {
    fn new(window: Arc<Window>, size: PhysicalSize<u32>) -> GpuResources {
        let gpu = GpuContext::new(window, size).expect("demo 环境应能取得可用的图形适配器");
        let render_target = RenderTarget::new(&gpu);

        let metadata = AtlasMetadata::parse(ATLAS_JSON).expect("内嵌图集元数据应为合法 JSON");
        let atlas =
            Atlas::load(&gpu, metadata, ATLAS_PNG).expect("内嵌图集资源应能上传为 GPU 纹理");
        // 批渲染画的是离屏 render_target，管线的 color target 格式必须
        // 跟着它走，不是窗口 surface 的格式——理由见 p1_acceptance 与
        // target.rs 模块文档。
        let batch = SpriteBatch::new(&gpu, &atlas, render_target.format());

        GpuResources {
            gpu,
            render_target,
            atlas,
            batch,
            window_size: size,
        }
    }

    /// 窗口尺寸变化时重配 surface 并记下新尺寸，供下一帧算 Viewport。
    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.window_size = size;
    }

    /// 把离屏 `render_target` 按整数倍缩放呈现到窗口，理由与
    /// `p1_acceptance::GpuResources::present` 完全一致。
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

    /// 查条目名对应的 [`AtlasEntry`] 与它的归一化 UV 矩形，找不到时记录
    /// 一条错误日志，理由与 `p1_acceptance::GpuResources::lookup` 一致。
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

/// P2 验收 demo 的完整状态。
///
/// `player` 独立于 [`WorldState`]：P2 阶段的世界状态只含种子、时钟、
/// 尺寸、地形（见 `ll_world::state::WorldState` 文档），实体存储要到
/// P3 才随 Intent/Effect 管线加入（见简报「有意留给后续阶段的缺口」），
/// 这里的玩家标记只是本 demo 自己的镜头/视野锚点，不是真正的游戏实体。
struct Demo {
    world: WorldState,
    /// 本体地形的固定索引缓存——demo 现造一个空 `Interner` 注册出来
    /// （见 `ll_world::terrain::base_terrain_fixture` 文档），不牵扯
    /// 真实的 mod 加载流程。
    terrain_ids: BaseTerrainIds,
    player: TorusPos,
    camera: Camera,
    resources: Option<GpuResources>,
}

impl Demo {
    fn new() -> Demo {
        let size = TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("演示世界尺寸为非零常量");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let mut world = WorldState::new(size, &GenParams::default(), &terrain_ids, terrain_table)
            .expect("演示世界尺寸满足生成入口的全部约束");
        // 开局定在正午而非引擎默认的午夜，理由见 INITIAL_CLOCK_TICKS 文档。
        world.advance(INITIAL_CLOCK_TICKS);

        let player = spawn::find_spawn(&world.terrain, &world.terrain_table);
        spawn::carve_wall_ridge(&mut world.terrain, player, &terrain_ids);

        let camera = Camera {
            center: player,
            world: size,
        };

        Demo {
            world,
            terrain_ids,
            player,
            camera,
            resources: None,
        }
    }

    /// 按方向键移动玩家一格，相机随之跟随。用 [`TorusSize::wrap`]
    /// 归一化——越界坐标由它负责绕回，这里不手写任何取模或钳制，这正是
    /// 「走到世界边缘无缝绕回」这条验收点的实现依据。
    fn move_player(&mut self, dx: i32, dy: i32) {
        self.player = self
            .world
            .size
            .wrap(self.player.x() + dx, self.player.y() + dy);
        self.camera.center = self.player;
    }
}

/// 把本帧应绘制的全部精灵推入 `resources.batch`。
///
/// 取自由函数而非 `Demo` 的方法，理由与 `p1_acceptance::collect_sprites`
/// 一致：`on_frame` 需要同时持有 `&mut self.resources` 与 `self` 的
/// 其余字段，写成 `&self` 方法会让编译器把「借了整个 `self`」和「借了
/// 其中一个字段」混为一谈，报出并不存在的借用冲突。
fn collect_sprites(
    world: &WorldState,
    terrain_ids: &BaseTerrainIds,
    camera: &Camera,
    player: TorusPos,
    visible: &VisibleSet,
    tint: [f32; 4],
    resources: &mut GpuResources,
) {
    push_terrain(world, terrain_ids, camera, visible, tint, resources);
    push_player(camera, player, tint, resources);
    push_minimap(world, terrain_ids, resources);
}

/// 画出相机视口内、且落在本帧视野（[`VisibleSet`]）内的地形瓦片。
///
/// **只画视野内的瓦片**：这正是「视野随移动更新，墙后的格子不可见」
/// 这条验收点的落地方式——[`SpriteBatch::flush`] 每帧先把离屏目标清成
/// 黑色（见 `batch.rs` 模块文档），没被本函数画到的格子就保持黑色，
/// 不需要额外画一层「未探索」贴图。
fn push_terrain(
    world: &WorldState,
    terrain_ids: &BaseTerrainIds,
    camera: &Camera,
    visible: &VisibleSet,
    tint: [f32; 4],
    resources: &mut GpuResources,
) {
    for pos in camera.visible_tiles() {
        if !visible.contains(pos) {
            continue;
        }
        let kind = world.terrain.terrain_at(pos);
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
            TERRAIN_ENTITY_BASE + pos.y() as u64 * WORLD_WIDTH as u64 + pos.x() as u64,
        );
        resources.batch.push(
            order,
            sprite_instance(sx as f32, sy as f32, entry.sprite_size(), uv, tint),
        );
    }
}

/// 画出玩家标记（复用图集里的 `hero_idle_0`）。
fn push_player(camera: &Camera, player: TorusPos, tint: [f32; 4], resources: &mut GpuResources) {
    let Some((entry, uv)) = resources.lookup("hero_idle_0") else {
        return;
    };
    let (tile_x, tile_y) = camera.world_to_screen(player);
    let footprint = Footprint {
        width: 1,
        height: 1,
    };
    let [px, py] = sprite_draw_position((tile_x, tile_y), footprint, entry.pivot);
    let order = DrawOrder::new(
        Layer::ENTITY,
        footprint_bottom_screen_y(tile_y, 1),
        PLAYER_ENTITY,
    );
    resources.batch.push(
        order,
        sprite_instance(px, py, entry.sprite_size(), uv, tint),
    );
}

/// 画出左上角的小地图（[`continent_map`] 的下采样概览）。
///
/// 小地图**不套用光照色调**：它是常驻的导航 UI，不是玩家视野本身的
/// 一部分，传统 roguelike 里小地图/大地图也通常不受局部光照影响
/// ——若也被夜晚调暗，会让玩家在最需要靠小地图辨认方向的夜间场景里
/// 反而看不清它，这与它的作用背道而驰。
fn push_minimap(world: &WorldState, terrain_ids: &BaseTerrainIds, resources: &mut GpuResources) {
    let cols = world.size.width().div_ceil(MINIMAP_DOWNSAMPLE);
    let cells = continent_map(world, MINIMAP_DOWNSAMPLE);

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
}

/// 拼一个 [`SpriteInstance`]：位置+像素尺寸+UV+颜色调制。
fn sprite_instance(
    x: f32,
    y: f32,
    size: SpriteSize,
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
        tracing::info!(width = size.width, height = size.height, "window resized");
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.resize(size);
        let viewport = fit_viewport(size.width, size.height);
        tracing::info!(
            scale = viewport.scale,
            offset_x = viewport.offset_x,
            offset_y = viewport.offset_y,
            "recomputed integer-scaled viewport"
        );
    }

    fn on_frame(&mut self, _frame: FrameId, input: &InputState) -> FrameOutcome {
        if input.was_just_pressed(GameKey::Cancel) {
            return FrameOutcome::Exit;
        }

        if input.was_activated(GameKey::Up) {
            self.move_player(0, -1);
        }
        if input.was_activated(GameKey::Down) {
            self.move_player(0, 1);
        }
        if input.was_activated(GameKey::Left) {
            self.move_player(-1, 0);
        }
        if input.was_activated(GameKey::Right) {
            self.move_player(1, 0);
        }
        if input.was_activated(GameKey::Wait) {
            self.world.advance(CLOCK_STEP_HOUR);
        }
        if input.was_just_pressed(GameKey::Confirm) {
            self.world.advance(CLOCK_STEP_SEASON);
        }

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        if input.was_just_pressed(GameKey::Screenshot) {
            save_baseline_png(resources, BASELINE_PNG_PATH);
        }

        // 光照与视野半径是纯函数派生，每帧现算，绝不缓存——理由见
        // `ll_world::light` 模块文档「光照是纯函数派生，绝不进世界
        // 状态」：缓存会与世界时钟失同步，表现为「白天却一片漆黑」
        // 这种极难复现的缺陷。
        let light = ambient_light(self.world.clock);
        let radius = sight_radius_at(BASE_SIGHT_RADIUS, light);
        let visible = compute_fov(
            &self.world.terrain,
            &self.world.terrain_table,
            self.player,
            radius,
        );
        let tint = ambient_tint(self.world.clock);

        collect_sprites(
            &self.world,
            &self.terrain_ids,
            &self.camera,
            self.player,
            &visible,
            tint,
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
    }
}

fn main() {
    init_logging(true).expect("首次初始化日志不应失败");
    tracing::info!(
        "P2 acceptance demo: arrows/WASD move player, '.' advances an hour, \
         Enter/Space advances a season, F2 saves baseline PNG, Esc to quit"
    );

    let demo = Demo::new();
    if let Err(error) = run(WindowConfig::default(), demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}
