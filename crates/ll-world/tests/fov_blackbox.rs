//! 对称阴影投射视野的黑箱属性测试。
//!
//! 对称性是这个模块存在的全部理由，而它只能靠属性测试来验：能真正
//! 暴露「A 能看见 B、B 却看不见 A」这类缺陷的，往往是特定的墙角几何
//! 形状——手写用例几乎不可能覆盖到，必须在随机地形上随机取点反复验证。

use ll_core::rng::DetRng;
use ll_core::torus::TorusSize;
use ll_world::chunk::ChunkGrid;
use ll_world::fov::compute_fov;
use ll_world::terrain::TerrainKind;
use proptest::prelude::*;

/// 属性测试用的世界宽度（格）。取值远大于测试用到的最大半径，
/// 避免视野的环面绕回上限（半宽/半高）反过来限制了测试想覆盖的半径。
const WORLD_WIDTH: u32 = 64;
/// 属性测试用的世界高度（格）。
const WORLD_HEIGHT: u32 = 64;

/// 按 `wall_seed` 确定性地铺出一张带随机墙体的网格。
///
/// 用 [`DetRng`] 而非 `proptest` 自带的随机源直接生成地形：这样同一个
/// `wall_seed` 恒对应同一张网格，`proptest` 收缩失败用例时能复现同一
/// 张地图，而不是每次收缩都换一张全新的地图。
fn random_grid(wall_seed: u64) -> ChunkGrid {
    let world = TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("常量尺寸合法");
    let mut grid = ChunkGrid::new(world).expect("64x64 满足 ChunkGrid 的最小视口跨度");
    let mut rng = DetRng::for_entity(wall_seed, 0, 0);

    for y in 0..WORLD_HEIGHT as i32 {
        for x in 0..WORLD_WIDTH as i32 {
            // 约两成概率放一堵墙：密度太低难以形成有意义的遮挡组合，
            // 太高又会让大部分格子相互不可见，两种极端都削弱这条属性
            // 测试的覆盖力。
            if rng.chance(1, 5) {
                grid.set_terrain(world.wrap(x, y), TerrainKind::WALL_STONE);
            }
        }
    }
    grid
}

proptest! {
    #[test]
    fn 视野是对称的(
        wall_seed in any::<u64>(),
        ax in 0i32..WORLD_WIDTH as i32,
        ay in 0i32..WORLD_HEIGHT as i32,
        bx in 0i32..WORLD_WIDTH as i32,
        by in 0i32..WORLD_HEIGHT as i32,
        radius in 1u32..16,
    ) {
        // 非对称算法会让玩家被自己看不见的敌人攻击——这条属性测试就是
        // 专门守护这一点：随机地形上随机取两点，互相可见性必须一致。
        // Arrange
        let grid = random_grid(wall_seed);
        let world = grid.world();
        let a = world.wrap(ax, ay);
        let b = world.wrap(bx, by);

        // Act
        let a_sees_b = compute_fov(&grid, a, radius).contains(b);
        let b_sees_a = compute_fov(&grid, b, radius).contains(a);

        // Assert
        prop_assert_eq!(a_sees_b, b_sees_a);
    }

    #[test]
    fn 可见格恒在半径之内(
        wall_seed in any::<u64>(),
        ox in 0i32..WORLD_WIDTH as i32,
        oy in 0i32..WORLD_HEIGHT as i32,
        radius in 0u32..16,
    ) {
        // Arrange
        let grid = random_grid(wall_seed);
        let world = grid.world();
        let origin = world.wrap(ox, oy);

        // Act
        let visible = compute_fov(&grid, origin, radius);

        // Assert
        prop_assert!(visible.iter().all(|pos| world.chebyshev(origin, pos) <= radius));
    }

    #[test]
    fn 任意输入都不崩溃(
        wall_seed in any::<u64>(),
        ox in 0i32..WORLD_WIDTH as i32,
        oy in 0i32..WORLD_HEIGHT as i32,
        radius in prop_oneof![0u32..16, Just(u32::MAX), Just(u32::MAX - 1)],
    ) {
        // 半径极大（含 u32::MAX）与原点贴着世界边缘（坐标取值范围含
        // 0 与宽/高减一）都在覆盖范围内。
        // Arrange
        let grid = random_grid(wall_seed);
        let world = grid.world();
        let origin = world.wrap(ox, oy);

        // Act
        let visible = compute_fov(&grid, origin, radius);

        // Assert
        prop_assert!(visible.contains(origin));
    }
}
