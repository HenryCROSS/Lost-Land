//! 昼夜四季驱动的环境光照与视野半径。
//!
//! # 光照是纯函数派生，绝不进世界状态
//!
//! 本模块所有函数都以 [`Tick`] 为输入现算现出，没有任何字段把结果存进
//! [`crate::state::WorldState`]。存进去必然与世界时钟失同步——时钟推进
//! 了而缓存的光照没跟着变，表现为「白天却一片漆黑」这种极难复现的
//! 缺陷，因为查代码时时钟和光照看起来各自都对，只有两者一起看才会
//! 发现矛盾。派生成本本身也低到不值得缓存：一次求值只是几个整数比较
//! 与一次除法。
//!
//! # 昼夜基准曲线在 `ll-core`，本模块只做季节缩放
//!
//! 未经季节缩放的昼夜渐变曲线（日出日落时刻、午夜/正午基准光照）定义
//! 在 [`ll_core::light`]，不在这里——那条曲线同时也是
//! [`ll_core::time::Tick::is_daylight`] 的判定依据，两处消费者共用同
//! 一份定义，不再各自维护一套边界。详见 `ll_core::light` 的模块文档
//! 「为什么这条曲线要下沉到 `ll-core`」一节：P2 阶段曾经因为两处各写
//! 一份而互相矛盾，收敛到 `ll-core` 是唯一同时满足单一真相源与
//! 「`ll-core` 不能反向依赖 `ll-world`」这条依赖方向约束的修法。
//! 本模块只在那条基准曲线之上叠加 [`season_light_scale`]，理由是季节
//! 缩放依赖 [`Season`]，`ll-core` 已有这个类型，不构成新的依赖问题，
//! 而「随季节调暗调亮」终归是世界层的玩法参数，不属于 `ll-core` 该管
//! 的纯数据基础设施。
//!
//! # 全程整数运算
//!
//! 昼夜渐变用「已经过的刻度数 / 渐变总刻度数」这个整数比例插值，不用
//! 浮点：世界状态禁止浮点数（详见 `ll-core` 的说明），而光照虽然本身
//! 不进状态，但会被视野半径这类确定性系统消费，浮点误差一旦混进来，
//! 两台机器上算出的视野半径可能不同，破坏跨平台确定性重放。

use ll_core::light::day_curve;
use ll_core::time::{Season, Tick};

/// 千分比表示的环境光照，`0..=1000`，1000 为最亮。
///
/// 用千分比整数而非百分比或浮点：千分比在日出日落的两小时渐变窗口内
/// 提供足够的插值精度（每刻度对应的光照变化不会被整数除法舍成 0），
/// 又全程是整数运算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LightLevel(pub i32);

/// 求某一世界时刻的环境光照。
///
/// 昼夜曲线（[`ll_core::light::day_curve`]）先给出未经季节缩放的基准
/// 值，再乘以 [`season_light_scale`] 得到最终光照。两步分开是为了让
/// 季节缩放能被单独测试与复用，而不必每次都重新构造一整天的 `Tick`。
pub fn ambient_light(tick: Tick) -> LightLevel {
    let base = day_curve(tick);
    let scale = season_light_scale(tick.season());
    // i64 中间结果避免 1000 * 1000 这种量级在极端输入下溢出 i32。
    let scaled = (i64::from(base) * i64::from(scale)) / 1000;
    LightLevel(scaled.clamp(0, 1000) as i32)
}

/// 季节对光照的缩放系数，千分比：夏 1000、春秋 900、冬 750。
///
/// 冬季明显低于其余三季，是为了让冬季在玩法上真正有压迫感——四季若
/// 只是换个色板，就没有存在的必要。
pub fn season_light_scale(season: Season) -> i32 {
    match season {
        Season::Summer => 1000,
        Season::Spring | Season::Autumn => 900,
        Season::Winter => 750,
    }
}

/// 按光照缩放基准视野半径，下限为 1。
///
/// 下限存在的理由与午夜光照取 100 而非 0 相同：视野缩到零会让玩家连
/// 自己脚下都看不见，那是卡住而不是难度。`light` 的分量在调用前会被
/// 夹到 `0..=1000`，即便调用方传入了越界值（例如某个未来的负面效果
/// 直接构造了 `LightLevel(-1)`），也不会产出负的或超比例的半径。
pub fn sight_radius_at(base_radius: u32, light: LightLevel) -> u32 {
    let clamped_light = light.0.clamp(0, 1000) as u64;
    let scaled = (u64::from(base_radius) * clamped_light) / 1000;
    (scaled as u32).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR};

    #[test]
    fn 正午光照最强() {
        // 夏季（缩放 1000，不折损）第 30 天正午——理论上能达到的最大值。
        // Arrange
        let summer_noon = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);

        // Act
        let light = ambient_light(summer_noon);

        // Assert
        assert_eq!(light.0, 1000);
    }

    #[test]
    fn 午夜光照不为零() {
        // Arrange
        let midnight = Tick(0);

        // Act
        let light = ambient_light(midnight);

        // Assert
        assert!(light.0 > 0);
    }

    #[test]
    fn 日出时段光照递增() {
        // 三个采样点落在同一天（春季），只有小时不同，季节缩放恒定，
        // 不会干扰基准曲线的单调性。日出窗口本身（5-7 点）的定义在
        // `ll_core::light`，这里直接写字面量小时数，不重复定义常量。
        // Arrange
        let at_dawn_start = Tick(5 * TICKS_PER_HOUR);
        let at_dawn_mid = Tick(6 * TICKS_PER_HOUR);
        let at_dawn_end = Tick(7 * TICKS_PER_HOUR - 1);

        // Act
        let start_light = ambient_light(at_dawn_start).0;
        let mid_light = ambient_light(at_dawn_mid).0;
        let end_light = ambient_light(at_dawn_end).0;

        // Assert
        assert!(start_light < mid_light && mid_light < end_light);
    }

    #[test]
    fn 冬季光照弱于夏季() {
        // 同为正午，仅季节不同：日 30 落在夏季，日 90 落在冬季。
        // Arrange
        let summer_noon = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);
        let winter_noon = Tick(90 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);

        // Act
        let summer_light = ambient_light(summer_noon).0;
        let winter_light = ambient_light(winter_noon).0;

        // Assert
        assert!(winter_light < summer_light);
    }

    #[test]
    fn 视野半径下限为一() {
        // 基准半径为零时，若不设下限，缩放结果恒为零。
        // Arrange
        let base_radius = 0;
        let full_light = LightLevel(1000);

        // Act
        let radius = sight_radius_at(base_radius, full_light);

        // Assert
        assert_eq!(radius, 1);
    }

    #[test]
    fn 光照为零时视野仍为一() {
        // Arrange
        let base_radius = 10;
        let no_light = LightLevel(0);

        // Act
        let radius = sight_radius_at(base_radius, no_light);

        // Assert
        assert_eq!(radius, 1);
    }
}
