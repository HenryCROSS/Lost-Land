//! P1 验收 Demo：把渲染层的每一条硬性约束摆到画面上。
//!
//! 六件事同时发生，任何一件坏了都该一眼看出来，而不必去读代码：
//!
//! 1. 一层瓦片地形（草/土棋盘格）铺满视口。
//! 2. 一个 16×24 的普通单位，沿固定路径自动巡逻，循环播放三帧行走动画。
//! 3. 一个 32×48 的重点目标，占 2×2 格却画得比格子高；普通单位巡逻
//!    经过它脚下时，二者的遮挡关系随 Y 排序正确切换。
//! 4. 方向键平移相机；世界是环面，移到边缘会无缝绕回而不是跳变。
//! 5. 窗口尺寸变化时，离屏画面始终整数倍居中、四周黑边。
//! 6. 按 M 键（见下方「按键替代」）把当前离屏纹理存成 PNG——这是冻结
//!    视觉回归基准的入口，不是调试功能。
//!
//! 运行：`cargo run -p ll-render --example p1_acceptance`
//! 操作：方向键/WASD 平移相机，M 存基准 PNG，Esc 退出。
//!
//! # 文件拆分
//!
//! [`layout`] 放不依赖 GPU 的纯计算（UV 归一化、地形花色、巡逻路径、
//! 精灵摆放），[`png`] 放基准 PNG 落盘，本文件只留 GPU 资源装配、
//! `Demo` 状态与 [`ll_platform::window::AppHandler`] 接线——三个文件
//! 各自控制在几百行内，理由见 `coding-style.md` 的文件规模约束。
//!
//! # 按键替代：M 而非 F2
//!
//! 简报要求用 F2 触发存图，但 `ll_platform::input::GameKey` 与
//! `map_physical_key` 都不认识 F2——两者都属于本任务不可修改的
//! 已完成模块（改动它们超出本任务范围）。这里改用已经映射到物理键
//! `M` 的 [`GameKey::Map`]，语义上与「产出一张当前视图的快照」也算
//! 贴切。真正接上 F2 需要先在 `ll-platform` 补一个动作键与物理键
//! 映射，留给后续任务。
//!
//! # 呈现流程
//!
//! 每帧先把完整场景画进离屏 [`RenderTarget`]（地形、动画、遮挡、相机
//! 换算），再用 [`GpuContext::acquire_frame`] 取窗口 surface 的当前帧、
//! 从它的纹理建一个视图、[`RenderTarget::blit_to`] 按整数倍缩放
//! blit 进去、最后 `present()` 提交给合成器。`acquire_frame` 对
//! `Outdated`/`Lost` 已经内部重配重试过一次，仍失败或遇到
//! `Timeout`/`Occluded`/`Validation` 时本帧直接跳过（记一条警告日志，
//! 不影响下一帧继续尝试）。

mod layout;
mod png;

use image::GenericImageView;
use layout::{
    footprint_bottom_world_y, hero_patrol_y, normalized_uv_rect, sprite_draw_position,
    terrain_entry_name,
};
use ll_core::torus::TorusSize;
use ll_platform::input::{GameKey, InputState};
use ll_platform::logging::init_logging;
use ll_platform::window::{
    AppHandler, FrameId, FrameOutcome, PhysicalSize, Window, WindowConfig, run,
};
use ll_render::anim::{Clip, Playback};
use ll_render::atlas::{Atlas, AtlasEntry, AtlasMetadata};
use ll_render::batch::{SpriteBatch, SpriteInstance};
use ll_render::camera::Camera;
use ll_render::gpu::GpuContext;
use ll_render::sprite::{DrawOrder, Footprint, Layer, SpriteSize, TILE_SIZE};
use ll_render::target::{RenderTarget, fit_viewport};
use png::save_baseline_png;
use std::collections::HashMap;
use std::sync::Arc;

/// 演示世界的宽度（格）。刻意大于相机单帧可见的瓦片跨度（约 43 格），
/// 平时看不到重复瓦片；又刻意不太大，短暂按住方向键就能移动到接缝。
const WORLD_WIDTH: u32 = 48;

/// 演示世界的高度（格），理由同 [`WORLD_WIDTH`]。
const WORLD_HEIGHT: u32 = 32;

/// 棋盘格地形要求宽高都是偶数：奇数会让世界接缝处两块同色地形相邻，
/// 看起来像是棋盘格本身断了一条缝，即便绕回逻辑其实完全正确。
const _: () = assert!(WORLD_WIDTH.is_multiple_of(2) && WORLD_HEIGHT.is_multiple_of(2));

/// 重点目标（boss）左上角所在格，占 2×2。
const BOSS_TILE: (i32, i32) = (23, 14);

/// 普通单位（hero）巡逻路径固定的横坐标，落在 boss 的占地列内，
/// 这样巡逻路径必然穿过它的脚下，才能演示遮挡关系的切换。
const HERO_PATROL_X: i32 = BOSS_TILE.0;

/// 巡逻路径纵坐标的上下界，取得比 boss 的 2 格占地更宽，这样巡逻既有
/// 「完全在 boss 之后」也有「完全在 boss 之前」的区间，不只是临界点。
const HERO_PATROL_MIN_Y: i32 = 8;
const HERO_PATROL_MAX_Y: i32 = 22;

/// 巡逻路径每挪一格所停留的帧数。60fps 下约 100ms 一格，肉眼能跟上。
const HERO_PATROL_FRAMES_PER_STEP: u64 = 6;

/// 行走动画每帧停留的游戏帧数。
const WALK_FRAMES_PER_STEP: u32 = 8;

/// 相机初始注视点，取世界近似中心，一开局就能看见 boss 与 hero。
const INITIAL_CAMERA: (i32, i32) = (24, 16);

/// 绘制顺序里给 hero 用的稳定实体号。
const HERO_ENTITY: u64 = 1;
/// 绘制顺序里给 boss 用的稳定实体号。
///
/// **必须小于 [`HERO_ENTITY`]**：当两者的 `foot_y` 恰好相等（hero 走到
/// boss 占地的最下一行）时，[`DrawOrder`] 按实体号打破平局，数值小的
/// 先绘制——boss 先画、hero 后画，hero 站在 boss 的落脚线上时应显示在
/// 前面，这正是较大的实体号后画、盖住先画者的直觉。
const BOSS_ENTITY: u64 = 0;

/// 瓦片绘制顺序号的起始偏移，避开 [`HERO_ENTITY`]/[`BOSS_ENTITY`] 这两个
/// 保留号，避免撞车导致排序键意外相等。
const TILE_ENTITY_BASE: u64 = 1000;

/// 图集元数据 JSON，编译期内嵌，不依赖运行时工作目录。
const ATLAS_JSON: &str = include_str!("../../../../assets/atlas/placeholder.json");
/// 图集图片字节，编译期内嵌，理由同上。
const ATLAS_PNG: &[u8] = include_bytes!("../../../../assets/atlas/placeholder.png");

/// 视觉回归基准 PNG 的落盘路径。
const BASELINE_PNG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/visual/baseline/p1_acceptance.png"
);

/// 存活于 `on_resume` 之后的 GPU 相关资源，与不依赖 GPU 的世界/相机
/// 状态分开存放，让 [`Demo`] 在窗口就绪前也能被构造与单测。
pub(crate) struct GpuResources {
    gpu: GpuContext,
    render_target: RenderTarget,
    atlas: Atlas,
    batch: SpriteBatch,
    /// 图集条目名 -> 归一化 UV 矩形，`on_resume` 时算好，避免每帧重算。
    uv_table: HashMap<String, [f32; 4]>,
    /// 最近一次已知的窗口物理尺寸，`on_frame` 据此算 [`Viewport`]——
    /// `on_frame` 本身收不到窗口尺寸，只能由 `on_resume`/`on_resize`
    /// 更新后存在这里。
    window_size: PhysicalSize<u32>,
}

impl GpuResources {
    fn new(window: Arc<Window>, size: PhysicalSize<u32>) -> GpuResources {
        let gpu = GpuContext::new(window, size).expect("demo 环境应能取得可用的图形适配器");
        let render_target = RenderTarget::new(&gpu);

        let metadata = AtlasMetadata::parse(ATLAS_JSON).expect("内嵌图集元数据应为合法 JSON");
        let (image_width, image_height) = image::load_from_memory(ATLAS_PNG)
            .expect("内嵌图集 PNG 应能解码")
            .dimensions();

        let mut uv_table = HashMap::with_capacity(metadata.entries.len());
        for entry in &metadata.entries {
            uv_table.insert(
                entry.name.clone(),
                normalized_uv_rect(entry.rect, image_width, image_height),
            );
        }

        let atlas =
            Atlas::load(&gpu, metadata, ATLAS_PNG).expect("内嵌图集资源应能上传为 GPU 纹理");
        let format = gpu.surface_format();
        let batch = SpriteBatch::new(&gpu, &atlas, format);

        GpuResources {
            gpu,
            render_target,
            atlas,
            batch,
            uv_table,
            window_size: size,
        }
    }

    /// 窗口尺寸变化时重配 surface 并记下新尺寸，供下一帧算 [`Viewport`]。
    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.window_size = size;
    }

    /// 把离屏 `render_target` 按整数倍缩放呈现到窗口。
    ///
    /// `acquire_frame` 失败时只记一条警告并跳过本帧——单帧呈现失败
    /// （`Timeout`/`Occluded` 等，见 [`GpuContext::acquire_frame`] 文档）
    /// 不该让整个 demo 崩溃或卡住，下一帧还会正常重试。
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
        // wgpu 30 把 `present` 挪到了 `Queue` 上（接收 `SurfaceTexture`
        // 按值消费），不再是 `SurfaceTexture` 自己的方法。
        self.gpu.queue().present(frame);
    }

    /// 查条目名对应的 [`AtlasEntry`]，找不到时记录一条错误日志。
    ///
    /// 内嵌资产在编译期就已固定，正常运行下这里恒能查到；仍返回
    /// `Option` 而不是 `unwrap`，是不假设调用方传入的名字一定存在于
    /// 图集里——这条防线比「反正不会错」更值得信任。
    fn lookup<'a>(&'a self, name: &str) -> Option<(&'a AtlasEntry, [f32; 4])> {
        let entry = self.atlas.metadata().lookup(name);
        let uv = self.uv_table.get(name).copied();
        match (entry, uv) {
            (Some(entry), Some(uv)) => Some((entry, uv)),
            _ => {
                tracing::error!(name, "图集条目缺失，跳过本次绘制");
                None
            }
        }
    }
}

/// P1 验收 demo 的完整状态。
struct Demo {
    world: TorusSize,
    camera: Camera,
    walk_clip: Vec<Clip>,
    walk_playback: Playback,
    resources: Option<GpuResources>,
}

impl Demo {
    fn new() -> Demo {
        let world = TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("演示世界尺寸为非零常量");
        let camera = Camera {
            center: world.wrap(INITIAL_CAMERA.0, INITIAL_CAMERA.1),
            world,
        };

        // 三帧交替（走姿 0 -> 立姿 -> 走姿 1 -> 立姿）拼出一个简单但连续
        // 循环的行走动画；用立姿作为两个走姿之间的过渡帧，避免两张走姿
        // 贴图直接互跳显得生硬。
        let walk_clip = vec![Clip {
            frames: vec![
                "hero_walk_0".to_string(),
                "hero_idle_0".to_string(),
                "hero_walk_1".to_string(),
                "hero_idle_0".to_string(),
            ],
            frames_per_step: WALK_FRAMES_PER_STEP,
            looping: true,
        }];
        let walk_playback = Playback::new(0, FrameId(0));

        Demo {
            world,
            camera,
            walk_clip,
            walk_playback,
            resources: None,
        }
    }

    /// 方向键按住/自动重复时把相机中心移动一格；用 [`TorusSize::wrap`]
    /// 归一化，越界坐标由它负责绕回，这里不手写任何取模或钳制。
    fn pan_camera(&mut self, dx: i32, dy: i32) {
        self.camera.center = self
            .world
            .wrap(self.camera.center.x() + dx, self.camera.center.y() + dy);
    }
}

/// 把本帧应绘制的全部精灵推入 `resources.batch`。
///
/// 取自由函数而非 `Demo` 的方法：`on_frame` 需要同时持有
/// `&mut self.resources` 与 `self` 的其余字段（世界、相机、动画剪辑），
/// 若写成 `&self` 方法，编译器只看到「借了整个 `self`」，无法识别
/// `resources` 借的是另一个字段，会报借用冲突。拆成显式参数后，各字段
/// 各借各的，冲突自然消失。
fn collect_sprites(
    world: TorusSize,
    camera: &Camera,
    walk_clip: &[Clip],
    walk_playback: &Playback,
    frame: FrameId,
    resources: &mut GpuResources,
) {
    for pos in camera.visible_tiles() {
        let name = terrain_entry_name(pos);
        let Some((entry, uv)) = resources.lookup(name) else {
            continue;
        };
        let (sx, sy) = camera.world_to_screen(pos);
        let order = DrawOrder::new(
            Layer::TERRAIN,
            pos.y() * TILE_SIZE as i32,
            TILE_ENTITY_BASE + pos.y() as u64 * WORLD_WIDTH as u64 + pos.x() as u64,
        );
        resources.batch.push(
            order,
            sprite_instance(
                sx as f32,
                sy as f32,
                entry.rect.width,
                entry.rect.height,
                uv,
            ),
        );
    }

    push_boss(world, camera, resources);
    push_hero(world, camera, walk_clip, walk_playback, frame, resources);
}

fn push_boss(world: TorusSize, camera: &Camera, resources: &mut GpuResources) {
    let Some((entry, uv)) = resources.lookup("boss_idle_0") else {
        return;
    };
    let boss_pos = world.wrap(BOSS_TILE.0, BOSS_TILE.1);
    let (tile_x, tile_y) = camera.world_to_screen(boss_pos);
    let [px, py] = sprite_draw_position(
        (tile_x, tile_y),
        Footprint {
            width: 2,
            height: 2,
        },
        entry.pivot,
    );
    let order = DrawOrder::new(
        Layer::ENTITY,
        footprint_bottom_world_y(BOSS_TILE.1, 2),
        BOSS_ENTITY,
    );
    resources.batch.push(
        order,
        sprite_instance(px, py, entry.rect.width, entry.rect.height, uv),
    );
}

fn push_hero(
    world: TorusSize,
    camera: &Camera,
    walk_clip: &[Clip],
    walk_playback: &Playback,
    frame: FrameId,
    resources: &mut GpuResources,
) {
    let hero_y = hero_patrol_y(
        frame,
        HERO_PATROL_MIN_Y,
        HERO_PATROL_MAX_Y,
        HERO_PATROL_FRAMES_PER_STEP,
    );
    let frame_name = walk_playback
        .current_frame(walk_clip, frame)
        .unwrap_or_else(|| {
            tracing::warn!("行走动画剪辑异常，退化为静止帧");
            "hero_idle_0"
        });
    let Some((entry, uv)) = resources.lookup(frame_name) else {
        return;
    };

    let hero_pos = world.wrap(HERO_PATROL_X, hero_y);
    let (tile_x, tile_y) = camera.world_to_screen(hero_pos);
    let [px, py] = sprite_draw_position(
        (tile_x, tile_y),
        Footprint {
            width: 1,
            height: 1,
        },
        entry.pivot,
    );
    let order = DrawOrder::new(
        Layer::ENTITY,
        footprint_bottom_world_y(hero_y, 1),
        HERO_ENTITY,
    );
    resources.batch.push(
        order,
        sprite_instance(px, py, entry.rect.width, entry.rect.height, uv),
    );
}

/// 拼一个 [`SpriteInstance`]：位置+像素尺寸+UV+不调制的颜色。
///
/// 抽成自由函数是因为地形、boss、hero 三处调用点共享同一套字段拼装
/// 逻辑，唯一的差别只是位置、尺寸与 UV 从哪里来。
fn sprite_instance(x: f32, y: f32, width: u16, height: u16, uv_rect: [f32; 4]) -> SpriteInstance {
    let size = SpriteSize { width, height };
    SpriteInstance {
        position: [x, y],
        size: [size.width as f32, size.height as f32],
        uv_rect,
        color: [1.0, 1.0, 1.0, 1.0],
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

    fn on_frame(&mut self, frame: FrameId, input: &InputState) -> FrameOutcome {
        if input.was_just_pressed(GameKey::Cancel) {
            return FrameOutcome::Exit;
        }

        if input.was_activated(GameKey::Up) {
            self.pan_camera(0, -1);
        }
        if input.was_activated(GameKey::Down) {
            self.pan_camera(0, 1);
        }
        if input.was_activated(GameKey::Left) {
            self.pan_camera(-1, 0);
        }
        if input.was_activated(GameKey::Right) {
            self.pan_camera(1, 0);
        }

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        if input.was_just_pressed(GameKey::Map) {
            save_baseline_png(resources, BASELINE_PNG_PATH);
        }

        collect_sprites(
            self.world,
            &self.camera,
            &self.walk_clip,
            &self.walk_playback,
            frame,
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
    tracing::info!("P1 acceptance demo: arrows/WASD pan camera, M saves baseline PNG, Esc to quit");

    let demo = Demo::new();
    if let Err(error) = run(WindowConfig::default(), demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}
