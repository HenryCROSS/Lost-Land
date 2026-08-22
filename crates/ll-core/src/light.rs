//! 昼夜基准光照曲线：白昼判定与环境光照共用的唯一真相源。
//!
//! # 为什么这条曲线要下沉到 `ll-core`
//!
//! P2 阶段一度存在两套互相矛盾的「白昼」定义：[`crate::time::Tick::is_daylight`]
//! 用固定的 6 点/18 点边界，`ll-world::light` 的环境光照渐变曲线用
//! 5–7 点日出、17–19 点日落。两者在早晚各有约两小时的窗口结论相反
//! ——`is_daylight()` 说是白天，光照曲线却只给到四五成亮度。更糟的是
//! `is_daylight` 的文档当时写着「后续若要让日照时长随季节变化，应在
//! 此处调整边界，而非另设时钟」——结果 `ll-world::light` 恰恰另设了
//! 一套边界，正是那条警告想要拦住的事。
//!
//! 两套定义收敛成一套是唯一的修法，但收敛到哪一层受限于依赖方向：
//! `ll-core` 不能依赖 `ll-world`（`ll-core` 是全项目最底层的纯数据
//! crate），所以不能让 `Tick::is_daylight()` 反过来调用
//! `ll-world::light` 的函数。于是把渐变曲线本身（连同日出日落时刻、
//! 午夜/正午基准光照这些常量）整体下沉到这里：`ll-core::time` 与
//! `ll-world::light` 都从这一份定义读数，不再各自维护一份。
//! `ll-world::light` 现在只负责在这条基准曲线之上叠加季节缩放
//! （`ll-world` 的 `light` 模块里的 `season_light_scale`，那部分依赖
//! [`crate::time::Season`]，是 `ll-core` 已有的类型，不构成新的依赖
//! 方向问题）。这里特意不用 intra-doc 链接语法指向 `ll_world` 的条目：
//! `ll-core` 不依赖 `ll-world`，rustdoc 无法解析这样一个链接，用了会
//! 让 `cargo doc` 报 broken link。
//!
//! # 全程整数运算
//!
//! 理由与 `ll-core` 其余部分一致：世界状态禁止浮点，浮点误差会破坏
//! 跨平台确定性重放。

use crate::time::{TICKS_PER_DAY, TICKS_PER_HOUR, Tick};

/// 千分比表示的昼夜基准光照，`100..=1000`，1000 为最亮。还没有经过
/// 季节缩放——那一步由 `ll-world::light::ambient_light` 负责。
///
/// 取十分之一亮度（100）而非零作为午夜下限：全黑不是难度，是卡住——
/// 依赖这条曲线的视野半径会被缩到只剩原点，玩家在那种状态下什么都
/// 做不了。取值下限存在的意义与 [`crate::time::Tick::is_daylight`]
/// 的判定阈值取曲线中点是同一类考量：曲线本身的形状，与在曲线上如何
/// 划出「白天/黑夜」这条线，要保持一致。
pub(crate) const MIDNIGHT_LIGHT: i32 = 100;

/// 正午（未经季节缩放的）基准光照。
pub(crate) const NOON_LIGHT: i32 = 1000;

/// 日出开始的小时（含）。
const SUNRISE_START_HOUR: i64 = 5;

/// 日出结束的小时（不含），此后进入全天光照。
const SUNRISE_END_HOUR: i64 = 7;

/// 日落开始的小时（含）。
const SUNSET_START_HOUR: i64 = 17;

/// 日落结束的小时（不含），此后进入夜间光照。
const SUNSET_END_HOUR: i64 = 19;

/// [`crate::time::Tick::is_daylight`] 的判定阈值：曲线的中点
/// `(MIDNIGHT_LIGHT + NOON_LIGHT) / 2`。
///
/// 取中点而不是另挑一个数字，是让「白昼」与曲线本身的渐变过程对齐——
/// 曲线在日出窗口正中（6 点）与日落窗口正中（18 点）都恰好经过这个
/// 值（见 [`day_curve`] 与 `ll-core/tests/determinism.rs` 的验证），
/// 白昼判定因此不是又一条独立选定的边界，而是直接从曲线本身读出来的
/// 结论——这正是本模块要解决的「两套定义」问题的核心：以后曲线的形状
/// 变了（例如按季节调整日照时长），白昼判定会自动跟着变，不需要有人
/// 记得去同步一条平行维护的常量。
pub(crate) const DAYLIGHT_THRESHOLD: i32 = (MIDNIGHT_LIGHT + NOON_LIGHT) / 2;

/// 求某一世界时刻的昼夜基准光照，`100..=1000`。
///
/// 正午 1000、午夜 100，日出（5–7 点）与日落（17–19 点）之间线性渐变。
/// 用 `tick.0`（刻度总数）而非只用 `hour_of_day` 计算：只精确到小时会
/// 让渐变在两小时窗口内只有三级台阶，用刻度数能算出连续的线性插值。
///
/// 这是 [`crate::time::Tick::is_daylight`] 与
/// `ll_world::light::ambient_light` 共用的唯一定义，理由见模块文档。
pub fn day_curve(tick: Tick) -> i32 {
    let ticks_of_day = tick.0.rem_euclid(TICKS_PER_DAY);
    let sunrise_start = SUNRISE_START_HOUR * TICKS_PER_HOUR;
    let sunrise_end = SUNRISE_END_HOUR * TICKS_PER_HOUR;
    let sunset_start = SUNSET_START_HOUR * TICKS_PER_HOUR;
    let sunset_end = SUNSET_END_HOUR * TICKS_PER_HOUR;

    if ticks_of_day < sunrise_start {
        MIDNIGHT_LIGHT
    } else if ticks_of_day < sunrise_end {
        interpolate(
            MIDNIGHT_LIGHT,
            NOON_LIGHT,
            ticks_of_day - sunrise_start,
            sunrise_end - sunrise_start,
        )
    } else if ticks_of_day < sunset_start {
        NOON_LIGHT
    } else if ticks_of_day < sunset_end {
        interpolate(
            NOON_LIGHT,
            MIDNIGHT_LIGHT,
            ticks_of_day - sunset_start,
            sunset_end - sunset_start,
        )
    } else {
        MIDNIGHT_LIGHT
    }
}

/// 昼夜曲线相对**曲线中点**的归一化偏离，千分比，取值 `-1000..=1000`。
///
/// 午夜恰好 `-1000`，正午恰好 `+1000`，日出正中（6 点）与日落正中
/// （18 点）恰好 `0`——因为中点用的正是 [`DAYLIGHT_THRESHOLD`]，与
/// [`crate::time::Tick::is_daylight`] 判定白昼用的是同一个值。本函数
/// 因此不是又一条独立选定的昼夜刻度，而是把同一条曲线换算成一个**与
/// 光照量纲无关**的比例，供「随昼夜起落、但本身不是光照」的派生量复用。
///
/// # 为什么需要这个换算，为什么它落在 `ll-core`
///
/// 第一个消费者是 `ll_world::temperature`（温度的昼夜偏移：正午最热、
/// 午夜最冷）。温度不是光照，不能直接乘 [`day_curve`] 的绝对值——那条
/// 曲线的量纲是「千分比亮度，午夜 100 正午 1000」，它的零点在「全黑」
/// 而不在「不冷不热」，直接拿来当温度系数会得出「午夜仍比基准温度高
/// 十分之一」这种没有意义的结论。温度需要的是**有符号的、以昼夜中点
/// 为零的偏离比例**，也就是本函数。
///
/// 换算本身只依赖 [`day_curve`] 与它的三个私有常量，放在这里可以让
/// 那三个常量继续保持私有——若让 `ll-world` 自己算，就必须把
/// `MIDNIGHT_LIGHT`/`NOON_LIGHT`/[`DAYLIGHT_THRESHOLD`] 三个常量全部
/// 公开出去，等于把「曲线的形状」这件本模块的内部知识散到下游，而本
/// 模块文档开篇「为什么这条曲线要下沉到 `ll-core`」要防的正是这件事。
///
/// # 整数运算，两端恰好取到 ±1000
///
/// `NOON_LIGHT - DAYLIGHT_THRESHOLD` 与 `DAYLIGHT_THRESHOLD -
/// MIDNIGHT_LIGHT` 相等（中点的定义），因此同一个半幅分母对上下两侧
/// 都成立，两端各自整除得到恰好的 ±1000，不靠舍入凑近似值。
pub fn day_curve_deviation_permille(tick: Tick) -> i32 {
    /// 曲线半幅：中点到任一端的距离。中点取自两端的算术平均，因此
    /// 上下两半等长，同一个分母对两侧都成立。
    const HALF_SPAN: i32 = NOON_LIGHT - DAYLIGHT_THRESHOLD;
    (day_curve(tick) - DAYLIGHT_THRESHOLD) * 1000 / HALF_SPAN
}

/// 在 `[from, to]` 之间按 `elapsed / span` 的比例线性插值。
///
/// 要求 `0 <= elapsed < span`，由调用方（[`day_curve`] 的分支条件）保证。
fn interpolate(from: i32, to: i32, elapsed: i64, span: i64) -> i32 {
    from + ((i64::from(to - from) * elapsed) / span) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 正午光照达到上界() {
        // Arrange
        let noon = Tick(12 * TICKS_PER_HOUR);

        // Act
        let light = day_curve(noon);

        // Assert
        assert_eq!(light, NOON_LIGHT);
    }

    #[test]
    fn 午夜光照为下界() {
        // Arrange
        let midnight = Tick(0);

        // Act
        let light = day_curve(midnight);

        // Assert
        assert_eq!(light, MIDNIGHT_LIGHT);
    }

    #[test]
    fn 日出窗口正中恰好等于判定阈值() {
        // 这条测试锁住 DAYLIGHT_THRESHOLD 取曲线中点的前提：日出窗口
        // （5-7 点）正中的 6 点必须恰好落在阈值上，白昼判定才能与曲线
        // 本身对齐，而不是又一条独立选定的边界。
        // Arrange
        let mid_sunrise = Tick(6 * TICKS_PER_HOUR);

        // Act
        let light = day_curve(mid_sunrise);

        // Assert
        assert_eq!(light, DAYLIGHT_THRESHOLD);
    }

    #[test]
    fn 日落窗口正中恰好等于判定阈值() {
        // 与上一条对称：日落窗口（17-19 点）正中的 18 点同样恰好落在
        // 阈值上。
        // Arrange
        let mid_sunset = Tick(18 * TICKS_PER_HOUR);

        // Act
        let light = day_curve(mid_sunset);

        // Assert
        assert_eq!(light, DAYLIGHT_THRESHOLD);
    }

    #[test]
    fn 日出时段光照递增() {
        // Arrange
        let at_dawn_start = Tick(SUNRISE_START_HOUR * TICKS_PER_HOUR);
        let at_dawn_mid = Tick(6 * TICKS_PER_HOUR);
        let at_dawn_end = Tick(SUNRISE_END_HOUR * TICKS_PER_HOUR - 1);

        // Act
        let start_light = day_curve(at_dawn_start);
        let mid_light = day_curve(at_dawn_mid);
        let end_light = day_curve(at_dawn_end);

        // Assert
        assert!(start_light < mid_light && mid_light < end_light);
    }

    #[test]
    fn 昼夜偏离在午夜与正午恰好取到正负一千() {
        // 两端恰好整除（不是近似），是 day_curve_deviation_permille
        // 文档「中点等分上下两半」那条论证的直接验证。
        // Arrange
        let midnight = Tick(0);
        let noon = Tick(12 * TICKS_PER_HOUR);

        // Act
        let at_midnight = day_curve_deviation_permille(midnight);
        let at_noon = day_curve_deviation_permille(noon);

        // Assert
        assert_eq!((at_midnight, at_noon), (-1000, 1000));
    }

    #[test]
    fn 昼夜偏离的零点与白昼判定阈值是同一条边界() {
        // 这条测试钉住「温度的昼夜偏移与 is_daylight 共用同一条曲线」：
        // 偏离非负 ⟺ 判定为白昼。两者若各自漂移，这里立刻变红。
        // Arrange：一整天每半小时采样一次，覆盖两段渐变窗口的内部。
        let samples: Vec<Tick> = (0..48).map(|i| Tick(i * TICKS_PER_HOUR / 2)).collect();

        // Act & Assert
        for tick in samples {
            assert_eq!(
                day_curve_deviation_permille(tick) >= 0,
                tick.is_daylight(),
                "刻度 {} 上两条判定不一致",
                tick.0
            );
        }
    }

    #[test]
    fn 昼夜偏离恒落在正负一千的闭区间内() {
        // 下游（温度）按这个区间换算偏移量，越界会让偏移量放大到设计
        // 之外的幅度。
        // Arrange
        let samples: Vec<Tick> = (0..(24 * 4))
            .map(|i| Tick(i * TICKS_PER_HOUR / 4))
            .collect();

        // Act & Assert
        for tick in samples {
            let deviation = day_curve_deviation_permille(tick);
            assert!(
                (-1000..=1000).contains(&deviation),
                "刻度 {} 的昼夜偏离 {deviation} 越界",
                tick.0
            );
        }
    }
}
