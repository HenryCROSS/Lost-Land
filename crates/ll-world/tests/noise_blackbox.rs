//! 可平铺噪声的黑箱属性测试。
//!
//! 无缝性是这个模块存在的全部理由，而它只能靠属性测试来验——手写用例
//! 不可能覆盖所有接缝位置与周期组合。

use ll_world::noise::{CELL_SIZE, TileableNoise};
use proptest::prelude::*;

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
}
