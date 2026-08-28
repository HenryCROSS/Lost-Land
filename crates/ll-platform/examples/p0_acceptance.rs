//! P0 验收 Demo。
//!
//! 证明平台地基端到端可用：开窗、收输入、日志、并行任务池、环面坐标、
//! 确定性随机、世界时间、状态摘要全部串起来。
//!
//! 本 demo **不渲染任何画面**——渲染是 P1 的职责。窗口是黑的，一切反馈
//! 通过日志输出。这是刻意的：地基层的验收不该依赖尚不存在的上层。
//!
//! 运行：`cargo run -p ll-platform --example p0_acceptance`
//! 操作：方向键或 WASD 移动光标，M 打印世界快照，Esc 或关闭窗口退出。

use ll_core::hashing::StateHasher;
use ll_core::rng::DetRng;
use ll_core::time::{TICKS_PER_HOUR, Tick};
use ll_core::torus::{TorusPos, TorusSize};
use ll_platform::input::{GameKey, InputState};
use ll_platform::jobs::JobPool;
use ll_platform::logging::init_logging;
use ll_platform::window::{AppHandler, FrameId, FrameOutcome, WindowConfig, run};
use std::sync::Arc;
use winit::dpi::PhysicalSize;
use winit::window::Window;

/// 演示用的极小世界，尺寸取小以便肉眼观察绕回行为。
const WORLD_WIDTH: u32 = 32;

/// 演示用世界的高度。
const WORLD_HEIGHT: u32 = 32;

/// 每次移动推进的世界时间。
const TICKS_PER_MOVE: i64 = TICKS_PER_HOUR;

/// 演示用的固定世界种子。
const DEMO_SEED: u64 = 0x1057_1A4D;

/// 演示用的主角实体号。
const PLAYER_ENTITY: u64 = 1;

struct Demo {
    world: TorusSize,
    cursor: TorusPos,
    clock: Tick,
    pool: JobPool,
    move_count: u64,
}

impl Demo {
    fn new() -> Self {
        let world = TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("演示世界尺寸为常量且非零");
        Demo {
            world,
            cursor: world.wrap(0, 0),
            clock: Tick(0),
            pool: JobPool::new(4).expect("演示用固定线程数，构建失败属环境异常"),
            move_count: 0,
        }
    }

    /// 按位移推进光标与世界时钟。
    fn step(&mut self, dx: i32, dy: i32) {
        self.cursor = self.world.wrap(self.cursor.x() + dx, self.cursor.y() + dy);
        self.clock = Tick(self.clock.0 + TICKS_PER_MOVE);
        self.move_count += 1;

        // 每次移动都为该次事件派生一条独立随机流，演示「随机数由三元组
        // 算出而非从共享流取出」。
        let mut rng = DetRng::for_entity(DEMO_SEED, PLAYER_ENTITY, self.move_count);
        let flavour = rng.gen_range(100);

        tracing::info!(
            x = self.cursor.x(),
            y = self.cursor.y(),
            hour = self.clock.hour_of_day(),
            season = ?self.clock.season(),
            daylight = self.clock.is_daylight(),
            flavour,
            "cursor moved"
        );
    }

    /// 用任务池并行计算全世界每格到光标的距离，并摘要结果。
    ///
    /// 这同时验证三件事：任务池顺序保持、环面距离正确、状态摘要可用。
    fn snapshot(&self) {
        let cells: Vec<TorusPos> = (0..WORLD_HEIGHT as i32)
            .flat_map(|y| (0..WORLD_WIDTH as i32).map(move |x| (x, y)))
            .map(|(x, y)| self.world.wrap(x, y))
            .collect();

        let cursor = self.cursor;
        let world = self.world;
        let distances = self
            .pool
            .map_collect(&cells, |cell| world.chebyshev(cursor, *cell));

        let mut hasher = StateHasher::new();
        hasher.write_i64(cursor.x() as i64);
        hasher.write_i64(cursor.y() as i64);
        hasher.write_i64(self.clock.0);
        for distance in &distances {
            hasher.write_u64(*distance as u64);
        }

        tracing::info!(
            cells = distances.len(),
            threads = self.pool.thread_count(),
            digest = format_args!("{:#018x}", hasher.finish()),
            "world snapshot"
        );
    }
}

impl AppHandler for Demo {
    fn on_resume(&mut self, _window: Arc<Window>, size: PhysicalSize<u32>) {
        tracing::info!(width = size.width, height = size.height, "window resumed");
    }

    fn on_resize(&mut self, size: PhysicalSize<u32>) {
        tracing::info!(width = size.width, height = size.height, "window resized");
    }

    fn on_frame(&mut self, _frame: FrameId, input: &mut InputState) -> FrameOutcome {
        if input.was_just_pressed(GameKey::Cancel) {
            return FrameOutcome::Exit;
        }

        // 方向键用 was_activated：首次按下立即走一格，长按则由输入层的
        // 自动重复驱动连续移动。Map 仍用 was_just_pressed——它是一次性
        // 动作，不该跟着长按连续触发。
        if input.was_activated(GameKey::Up) {
            self.step(0, -1);
        }
        if input.was_activated(GameKey::Down) {
            self.step(0, 1);
        }
        if input.was_activated(GameKey::Left) {
            self.step(-1, 0);
        }
        if input.was_activated(GameKey::Right) {
            self.step(1, 0);
        }
        if input.was_just_pressed(GameKey::Map) {
            self.snapshot();
        }

        FrameOutcome::Continue
    }

    fn on_exit(&mut self) {
        tracing::info!(moves = self.move_count, "demo exiting");
    }
}

fn main() {
    init_logging(true).expect("首次初始化日志不应失败");
    tracing::info!("P0 acceptance demo: arrows/WASD to move, M for snapshot, Esc to quit");

    let demo = Demo::new();
    if let Err(error) = run(WindowConfig::default(), demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}
