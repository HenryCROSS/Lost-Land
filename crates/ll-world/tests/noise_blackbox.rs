//! 可平铺噪声的黑箱属性测试。
//!
//! 无缝性是这个模块存在的全部理由，而它只能靠属性测试来验——手写用例
//! 不可能覆盖所有接缝位置与周期组合。

use ll_world::noise::{CELL_SIZE, TileableNoise};
use proptest::prelude::*;

/// 生成噪声周期，偏向 2 的幂与其他小公因数值，而不是纯均匀随机。
///
/// [`TileableNoise::octaves`] 的粗层分支只在 `gcd(period_x, period_y)`
/// 含 2 的因子时才会被走到——纯均匀随机的 `period` 里，两个独立随机数
/// 同为偶数的概率只有约四分之一，且就算都是偶数，`gcd` 未必是 2 的幂
/// 的倍数。这里显式把常见的 2 的幂样本混进候选池，让接缝测试的绝大多数
/// 样本真正落在粗层分支里，而不是主要测到 `coarse_scale == 1` 的退化
/// 路径——退化路径已经等价于改动前的行为，早被其余测试覆盖过。
fn period_strategy() -> impl Strategy<Value = u32> {
    prop_oneof![
        3 => prop_oneof![Just(2u32), Just(4u32), Just(8u32), Just(16u32), Just(32u32)],
        1 => 2u32..64,
    ]
}

proptest! {
    #[test]
    fn 东西接缝处采样值连续(period in 2u32..16, y in -500i32..500) {
        // 接缝不连续时，玩家跨越世界东西边界会看到地形突变。
        // Arrange
        let noise = TileableNoise::new(0xABCD, period, period).expect("周期非零");
        let world_width = period as i32 * CELL_SIZE;

        // Act
        let west = noise.sample(0, y);
        let east = noise.sample(world_width, y);

        // Assert
        prop_assert_eq!(west, east);
    }

    #[test]
    fn 南北接缝处采样值连续(period in 2u32..16, x in -500i32..500) {
        // Arrange
        let noise = TileableNoise::new(0xABCD, period, period).expect("周期非零");
        let world_height = period as i32 * CELL_SIZE;

        // Act
        let north = noise.sample(x, 0);
        let south = noise.sample(x, world_height);

        // Assert
        prop_assert_eq!(north, south);
    }

    #[test]
    fn 任意坐标的采样值都在有效区间内(
        seed in any::<u64>(),
        period in 1u32..32,
        x in i32::MIN / 4..i32::MAX / 4,
        y in i32::MIN / 4..i32::MAX / 4,
    ) {
        // 极端坐标最容易触发溢出，而溢出后的值会静默越界。
        // Arrange
        let noise = TileableNoise::new(seed, period, period).expect("周期非零");

        // Act
        let value = noise.sample(x, y);

        // Assert
        prop_assert!((0..=1000).contains(&value));
    }

    #[test]
    fn 多倍频在任意层数下都不溢出(
        octaves in 0u32..24,
        x in -100_000i32..100_000,
        y in -100_000i32..100_000,
    ) {
        // 层数过多时频率翻倍会让坐标溢出。
        // Arrange
        let noise = TileableNoise::new(7, 8, 8).expect("周期非零");

        // Act
        let value = noise.octaves(x, y, octaves);

        // Assert
        prop_assert!((0..=1000).contains(&value));
    }

    #[test]
    fn 多倍频在东西接缝处采样值连续(
        period_x in period_strategy(),
        period_y in period_strategy(),
        octaves in 1u32..8,
        y in -500i32..500,
    ) {
        // `sample()` 的接缝测试只覆盖最细一层，生产代码实际走的是
        // `octaves()`——它新增的粗层分支（大陆尺度那一层）必须单独证明
        // 无缝，不能只靠 generate.rs 里固定参数的单元测试兜底。
        // period_x/period_y 独立随机且偏向 2 的幂（见 period_strategy
        // 文档），覆盖 coarse_scale 由二者最大公约数推导这条新逻辑，
        // 而不只是退化到 coarse_scale == 1 的旧行为。
        // Arrange
        let noise = TileableNoise::new(0xABCD, period_x, period_y).expect("周期非零");
        let world_width = period_x as i32 * CELL_SIZE;

        // Act
        let west = noise.octaves(0, y, octaves);
        let east = noise.octaves(world_width, y, octaves);

        // Assert
        prop_assert_eq!(west, east);
    }

    #[test]
    fn 多倍频在南北接缝处采样值连续(
        period_x in period_strategy(),
        period_y in period_strategy(),
        octaves in 1u32..8,
        x in -500i32..500,
    ) {
        // 理由同上一条。
        // Arrange
        let noise = TileableNoise::new(0xABCD, period_x, period_y).expect("周期非零");
        let world_height = period_y as i32 * CELL_SIZE;

        // Act
        let north = noise.octaves(x, 0, octaves);
        let south = noise.octaves(x, world_height, octaves);

        // Assert
        prop_assert_eq!(north, south);
    }
}
