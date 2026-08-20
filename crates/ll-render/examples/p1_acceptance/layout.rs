//! 与 GPU 无关的纯计算：地形花色、巡逻路径、精灵摆放；以及描述演示
//! 场景布局的常量。
//!
//! 单独成模块是为了让这些逻辑可以脱离 [`super::GpuResources`] 被单测
//! 覆盖——它们都是纯函数，不需要真实窗口或图形适配器。
//!
//! # 常量必须只有一处定义
//!
//! [`BOSS_TILE`]/[`BOSS_ENTITY`]/[`HERO_ENTITY`]/巡逻区间这些常量曾经
//! 在 `main.rs` 定义一份、又在这个文件的测试模块里重抄一份用来断言。
//! 两份数字长得一样不代表它们是同一个东西——把 `main.rs` 里的
//! `BOSS_ENTITY` 改掉，测试模块里那份重抄的副本纹丝不动，遮挡关系
//! 反转了测试照样全绿，测试形同虚设。这里把它们收作本模块唯一的
//! 权威定义，`main.rs` 与测试模块都从这里引用，改一处、两边同时生效。

use ll_core::torus::TorusPos;
use ll_platform::window::FrameId;
// 「占地锚点 − pivot = 图像左上角」这条换算曾经是本文件的私有函数，
// 且被后续几个 demo 各抄一份、又被 P5 demo 完全遗漏——遗漏没有在编译期
// 暴露，因为大多数 demo 画的是 1×1 小图，凑巧看不出偏移方向反了。现已
// 提升为 `ll_render::sprite` 的公开函数，这里只重新导出 `main.rs` 真正
// 用到的两个（`footprint_anchor_pixel` 只在本文件测试模块里直接用到，
// 在那里单独导入，不在此处重新导出，避免非测试构建下的未使用警告），
// 不再保留第二份实现，理由见其文档「调用方不得自行重实现这条换算」
// 一节。
pub(crate) use ll_render::sprite::{footprint_bottom_screen_y, sprite_draw_position};

/// 演示世界的宽度（格）。刻意大于相机单帧可见的瓦片跨度（约 43 格），
/// 平时看不到重复瓦片；又刻意不太大，短暂按住方向键就能移动到接缝。
pub(crate) const WORLD_WIDTH: u32 = 48;

/// 演示世界的高度（格），理由同 [`WORLD_WIDTH`]。
pub(crate) const WORLD_HEIGHT: u32 = 32;

/// 棋盘格地形要求宽高都是偶数：奇数会让世界接缝处两块同色地形相邻，
/// 看起来像是棋盘格本身断了一条缝，即便绕回逻辑其实完全正确。
const _: () = assert!(WORLD_WIDTH.is_multiple_of(2) && WORLD_HEIGHT.is_multiple_of(2));

/// 重点目标（boss）左上角所在格，占 2×2。
pub(crate) const BOSS_TILE: (i32, i32) = (23, 14);

/// 巡逻路径纵坐标的上下界，取得比 boss 的 2 格占地更宽，这样巡逻既有
/// 「完全在 boss 之后」也有「完全在 boss 之前」的区间，不只是临界点。
pub(crate) const HERO_PATROL_MIN_Y: i32 = 8;
pub(crate) const HERO_PATROL_MAX_Y: i32 = 22;

/// 巡逻路径每挪一格所停留的帧数。60fps 下约 100ms 一格，肉眼能跟上。
pub(crate) const HERO_PATROL_FRAMES_PER_STEP: u64 = 6;

/// 绘制顺序里给 hero 用的稳定实体号。
pub(crate) const HERO_ENTITY: u64 = 1;

/// 绘制顺序里给 boss 用的稳定实体号。
///
/// **必须小于 [`HERO_ENTITY`]**：当两者的 `foot_y` 恰好相等（hero 走到
/// boss 占地的最下一行）时，`DrawOrder` 按实体号打破平局，数值小的
/// 先绘制——boss 先画、hero 后画，hero 站在 boss 的落脚线上时应显示在
/// 前面，这正是较大的实体号后画、盖住先画者的直觉。
pub(crate) const BOSS_ENTITY: u64 = 0;

/// 世界格坐标决定地形贴图种类：奇偶棋盘格。
///
/// 用棋盘格而非纯色铺满，是因为纯色地形无法用肉眼验证「相机移动是否
/// 连续、绕回处是否无缝」——单色画面里，画面撕裂或错位的瓦片和正常
/// 瓦片长得一模一样。棋盘格的格线本身就是最敏感的错位探测器。
pub(crate) fn terrain_entry_name(pos: TorusPos) -> &'static str {
    if (pos.x() + pos.y()) % 2 == 0 {
        "terrain_grass"
    } else {
        "terrain_dirt"
    }
}

/// hero 巡逻路径在给定帧号时所在的纵坐标：在 `min_y`/`max_y` 之间往返，
/// 纯整数三角波。
///
/// 全程整数运算：世界状态不允许浮点（见 crate 顶层文档「浮点边界」），
/// 巡逻路径虽只存在于这个 demo 里，仍遵循同一纪律。
pub(crate) fn hero_patrol_y(frame: FrameId, min_y: i32, max_y: i32, frames_per_step: u64) -> i32 {
    let span = max_y - min_y;
    let period = (span as u64) * 2;
    let step = (frame.0 / frames_per_step) % period.max(1);
    let offset = if step <= span as u64 {
        step as i32
    } else {
        2 * span - step as i32
    };
    min_y + offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::torus::TorusSize;
    use ll_render::sprite::{
        DrawOrder, Footprint, Layer, Pivot, TILE_SIZE, footprint_anchor_pixel,
    };

    #[test]
    fn 棋盘格相邻格纹理种类交替() {
        // 棋盘格的意义在于让相机绕回处的错位一眼可见；前提是相邻格
        // 确实交替，而不是意外全同色。
        // Arrange
        let world = TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("常量非零");
        let a = world.wrap(4, 4);
        let b = world.wrap(5, 4);

        // Act & Assert
        assert_ne!(terrain_entry_name(a), terrain_entry_name(b));
    }

    #[test]
    fn 世界接缝两侧的棋盘格仍然交替() {
        // 宽高取偶数正是为了保证这一点：接缝处不应看起来像棋盘格断开。
        // Arrange
        let world = TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("常量非零");
        let last_column = world.wrap(WORLD_WIDTH as i32 - 1, 4);
        let wrapped_first_column = world.wrap(WORLD_WIDTH as i32, 4);

        // Act & Assert
        assert_ne!(
            terrain_entry_name(last_column),
            terrain_entry_name(wrapped_first_column)
        );
    }

    #[test]
    fn 巡逻路径在起点停留于下界() {
        // Arrange & Act
        let y = hero_patrol_y(
            FrameId(0),
            HERO_PATROL_MIN_Y,
            HERO_PATROL_MAX_Y,
            HERO_PATROL_FRAMES_PER_STEP,
        );

        // Assert
        assert_eq!(y, HERO_PATROL_MIN_Y);
    }

    #[test]
    fn 巡逻路径会到达上界() {
        // Arrange
        let span = (HERO_PATROL_MAX_Y - HERO_PATROL_MIN_Y) as u64;
        let frame = FrameId(span * HERO_PATROL_FRAMES_PER_STEP);

        // Act
        let y = hero_patrol_y(
            frame,
            HERO_PATROL_MIN_Y,
            HERO_PATROL_MAX_Y,
            HERO_PATROL_FRAMES_PER_STEP,
        );

        // Assert
        assert_eq!(y, HERO_PATROL_MAX_Y);
    }

    #[test]
    fn 巡逻路径到达上界后折返下降() {
        // 三角波过了半程应该往回走，而不是继续单调递增或突然跳变。
        // Arrange
        let span = (HERO_PATROL_MAX_Y - HERO_PATROL_MIN_Y) as u64;
        let at_peak = FrameId(span * HERO_PATROL_FRAMES_PER_STEP);
        let one_step_later = FrameId(at_peak.0 + HERO_PATROL_FRAMES_PER_STEP);

        // Act
        let y = hero_patrol_y(
            one_step_later,
            HERO_PATROL_MIN_Y,
            HERO_PATROL_MAX_Y,
            HERO_PATROL_FRAMES_PER_STEP,
        );

        // Assert
        assert_eq!(y, HERO_PATROL_MAX_Y - 1);
    }

    #[test]
    fn 普通单位锚点在占地格底边中点() {
        // 1x1 占地的普通单位，锚点应落在这一格的水平中点、底边。
        // Arrange
        let footprint = Footprint {
            width: 1,
            height: 1,
        };

        // Act
        let (ax, ay) = footprint_anchor_pixel((100, 200), footprint);

        // Assert
        assert_eq!(
            (ax, ay),
            (100 + TILE_SIZE as i32 / 2, 200 + TILE_SIZE as i32)
        );
    }

    #[test]
    fn 重点目标锚点在两格宽占地的底边中点() {
        // 2x2 占地的重点目标，锚点横坐标应是两格宽度的中点，而不是
        // 单格宽度的中点——这正是 footprint 与 pivot 解耦要覆盖的场景。
        // Arrange
        let footprint = Footprint {
            width: 2,
            height: 2,
        };

        // Act
        let (ax, ay) = footprint_anchor_pixel((100, 200), footprint);

        // Assert
        assert_eq!(
            (ax, ay),
            (100 + TILE_SIZE as i32, 200 + 2 * TILE_SIZE as i32)
        );
    }

    #[test]
    fn 重点目标的绘制位置比自己的占地更靠上() {
        // 32x48 的贴图占 2x2 格却要画得比格子高——绘制原点的 y 必须比
        // 单纯的格子顶部再往上偏移，否则贴图会从占地区域内部才开始画。
        // Arrange
        let footprint = Footprint {
            width: 2,
            height: 2,
        };
        let pivot = Pivot { x: 16, y: 48 };

        // Act
        let [_, draw_y] = sprite_draw_position((100, 200), footprint, pivot);

        // Assert：占地顶部是 200，绘制原点应比它更靠上（更小）。
        assert!(draw_y < 200.0);
    }

    #[test]
    fn 站在重点目标占地最下一行时普通单位排在前面() {
        // hero 的 foot_y 与 boss 的 foot_y 恰好相等时（hero 站在 boss
        // 占地的最后一行），必须由实体号打破平局，且 hero 应该排在后面
        // （画在上层、盖住 boss），因为它此刻站在最靠近镜头的落脚线上。
        //
        // BOSS_ENTITY/HERO_ENTITY 引用的是本模块顶部唯一的权威定义
        // ——谁在 main.rs 改了绘制时用的实体号，这里立刻反映出来，
        // 而不是像此前那样各测各的、改了权威定义测试也发现不了。
        // Arrange：本测试不涉及相机换算，直接用「格坐标 × 瓦片边长」
        // 充当屏幕纵坐标——这里只验证占地底边的相对排序算术，真实的
        // 世界到屏幕换算由 `Camera::world_to_screen` 承担、在其自己的
        // 测试里覆盖。
        let boss_foot_y = footprint_bottom_screen_y(BOSS_TILE.1 * TILE_SIZE as i32, 2);
        let hero_tile_y = BOSS_TILE.1 + 1; // boss 占地的最下一行
        let hero_foot_y = footprint_bottom_screen_y(hero_tile_y * TILE_SIZE as i32, 1);
        assert_eq!(
            boss_foot_y, hero_foot_y,
            "本测试的前提是两者 foot_y 恰好相等"
        );

        // Act
        let boss_order = DrawOrder::new(Layer::ENTITY, boss_foot_y, BOSS_ENTITY);
        let hero_order = DrawOrder::new(Layer::ENTITY, hero_foot_y, HERO_ENTITY);

        // Assert
        assert!(boss_order < hero_order);
    }

    #[test]
    fn 巡逻路径完全在重点目标上方时重点目标排在前面() {
        // Arrange：格坐标 × 瓦片边长充当屏幕纵坐标，理由同上一测试。
        let boss_foot_y = footprint_bottom_screen_y(BOSS_TILE.1 * TILE_SIZE as i32, 2);
        let hero_foot_y = footprint_bottom_screen_y(HERO_PATROL_MIN_Y * TILE_SIZE as i32, 1);

        // Act
        let boss_order = DrawOrder::new(Layer::ENTITY, boss_foot_y, BOSS_ENTITY);
        let hero_order = DrawOrder::new(Layer::ENTITY, hero_foot_y, HERO_ENTITY);

        // Assert：boss 排在 hero 之后，也就是画在上层、遮住 hero。
        assert!(hero_order < boss_order);
    }

    #[test]
    fn 巡逻路径完全在重点目标下方时普通单位排在前面() {
        // Arrange：格坐标 × 瓦片边长充当屏幕纵坐标，理由同上。
        let boss_foot_y = footprint_bottom_screen_y(BOSS_TILE.1 * TILE_SIZE as i32, 2);
        let hero_foot_y = footprint_bottom_screen_y(HERO_PATROL_MAX_Y * TILE_SIZE as i32, 1);

        // Act
        let boss_order = DrawOrder::new(Layer::ENTITY, boss_foot_y, BOSS_ENTITY);
        let hero_order = DrawOrder::new(Layer::ENTITY, hero_foot_y, HERO_ENTITY);

        // Assert
        assert!(boss_order < hero_order);
    }
}
