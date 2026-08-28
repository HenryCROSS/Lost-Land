//! 气候条带的黑盒属性测试：**南北接缝处必须连续**。
//!
//! # 为什么气候需要自己的接缝测试
//!
//! `tests/noise_blackbox.rs` 已经证明了噪声的无缝性，`generate.rs` 末尾
//! 的两条测试又单独证明了「阈值判断本身没有引入不连续」。气候是**第三
//! 条**独立的输入：它不看噪声、只看 `y`，因此上面两条一条也覆盖不到它。
//! 规格 §14.2 的属性测试表里「可平铺整数噪声接缝处连续」那一条讲的是
//! 噪声；这个文件是**同型**的那一条，讲气候。
//!
//! 不连续会怎样：玩家沿南北方向跨过世界接缝时，脚下的地形会从沙漠突变
//! 成冻原（或反过来）——而规格 §7.1 恰恰承诺「迷途大陆即一片没有边缘、
//! 首尾相接的土地」。这类缺陷在小世界里未必看得出来，正是属性测试比
//! 具体用例更合适的场景。
//!
//! # 用属性测试而不是几个具体用例
//!
//! 连续性是「对**任意**合法世界高度、任意 `x` 都成立」这种形状的断言，
//! 具体用例只能证明「这一个高度是对的」。世界高度的合法取值是
//! `ll_world::noise::CELL_SIZE` 的任意整数倍，用例列举不完。

use ll_world::climate::{ClimateBand, band_at, warmth_at};
use ll_world::generate::TerrainShape;
use ll_world::noise::CELL_SIZE;

/// 把 proptest 抽到的「多少个格点周期」换算成合法的世界瓦片高度。
///
/// 世界宽高必须是 [`CELL_SIZE`] 的整数倍，否则
/// `ll_world::generate::generate_terrain` 在入口就拒绝生成——测试只在
/// 这个合法域里取值，不去验一个生成器根本不接受的世界。
fn world_height(periods: u32) -> u32 {
    periods * CELL_SIZE as u32
}

proptest::proptest! {
    #[test]
    fn 纬度暖度在南北接缝处连续(periods in 2u32..64) {
        // 反例（本次开发实跑）：把 `warmth_at` 的周期从 `world_height / 2`
        // 改成常数 `100`（不整除大多数世界高度），本条立刻在
        // periods = 3 上报 `left: 400, right: 1000`。
        // Arrange
        let height = world_height(periods);

        // Act
        let north = warmth_at(0, height);
        let south = warmth_at(height as i32, height);

        // Assert
        proptest::prop_assert_eq!(north, south);
    }

    #[test]
    fn 气候带在南北接缝处连续(periods in 2u32..64, width in 0i32..=TerrainShape::MAX_CLIMATE_BAND_WIDTH) {
        // 暖度连续不等于气候带连续：中间还隔着一次阈值切分，与
        // 「噪声无缝不等于地形无缝」是同一条道理（见
        // `crates/ll-world/src/generate.rs` 模块文档）。
        // Arrange
        let height = world_height(periods);

        // Act
        let north = band_at(0, height, width);
        let south = band_at(height as i32, height, width);

        // Assert
        proptest::prop_assert_eq!(north, south);
    }

    #[test]
    fn 绕世界一圈回到同一条气候带(periods in 2u32..64, y in -5000i32..5000, laps in 1i32..4) {
        // 接缝连续只验了 y=0 这一个点。真环面上「向北走整整一圈回到原
        // 处」对**每一个** y 都必须成立，否则接缝那一格对上了、别处仍
        // 是错位的。
        // Arrange
        let height = world_height(periods);
        let width = TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH;

        // Act
        let here = band_at(y, height, width);
        let after_laps = band_at(y + laps * height as i32, height, width);

        // Assert
        proptest::prop_assert_eq!(here, after_laps);
    }

    #[test]
    fn 一个世界高度里恰有两条赤道与两条极圈(periods in 2u32..64) {
        // 规格 §7.1「两条赤道 + 两条极圈」这句话本身，对任意合法世界
        // 高度成立——不是只在某一个测试尺寸上凑巧。
        //
        // 反例（本次开发实跑）：把周期从 `world_height / 2` 改成
        // `world_height`，本条报「赤道条数 1 != 2」。
        // Arrange
        let height = world_height(periods);

        // Act
        let equators = (0..height as i32)
            .filter(|&y| warmth_at(y, height) == ll_world::climate::WARMTH_MAX)
            .count();
        let poles = (0..height as i32)
            .filter(|&y| warmth_at(y, height) == ll_world::climate::WARMTH_MIN)
            .count();

        // Assert
        proptest::prop_assert_eq!(equators, 2);
        proptest::prop_assert_eq!(poles, 2);
    }

    #[test]
    fn 带宽为零时任何纬度都是温带(periods in 2u32..64, y in -5000i32..5000) {
        // 「关掉气候即恒等」这条性质的属性版本——`generate.rs` 里那条
        // 逐格比对只覆盖一个 64×64 的世界，这条覆盖任意世界高度与任意
        // 纬度。两条一起才等于「恒等」这个词的完整含义。
        // Arrange
        let height = world_height(periods);

        // Act
        let band = band_at(y, height, 0);

        // Assert
        proptest::prop_assert_eq!(band, ClimateBand::Temperate);
    }
}
