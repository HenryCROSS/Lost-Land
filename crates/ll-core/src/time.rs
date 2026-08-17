//! 世界时间与四季。
//!
//! 全世界只有**一个时钟**，昼夜、季节、时间轴调度、经济推进全部由它
//! 派生。设立多个时钟必然导致它们逐渐失同步，进而出现「城镇已入冬但
//! 野外还是盛夏」这类缺陷。
//!
//! 时间以整数刻度表示，不使用浮点——理由同世界状态的其余部分：浮点会
//! 破坏跨平台确定性。

/// 一分钟对应的刻度数。
///
/// 取 60 使一刻度恰好等于一游戏秒，方便调试时肉眼换算。
pub const TICKS_PER_MINUTE: i64 = 60;

/// 一小时对应的刻度数。
pub const TICKS_PER_HOUR: i64 = TICKS_PER_MINUTE * 60;

/// 一天对应的刻度数。
pub const TICKS_PER_DAY: i64 = TICKS_PER_HOUR * 24;

/// 每个季节的天数。
pub const DAYS_PER_SEASON: i64 = 30;

/// 一年的季节数。
pub const SEASONS_PER_YEAR: i64 = 4;

/// 四季。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Season {
    /// 春。
    Spring,
    /// 夏。
    Summer,
    /// 秋。
    Autumn,
    /// 冬。
    Winter,
}

/// 世界时刻，以刻度计，从世界创建那一刻开始计数。
///
/// `serde` 派生由同名 feature 开关（默认关闭）：见 `ll-core` 的
/// `Cargo.toml` 顶部说明。`WorldState` 需要它才能完整序列化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tick(pub i64);

impl Tick {
    /// 当日的小时数，取值 `0..24`。
    pub const fn hour_of_day(&self) -> i64 {
        // rem_euclid 而非取余：世界时钟理论上不会为负，但读档迁移或
        // 时间倒流类效果可能产生负值，取余会得到负小时数。
        (self.0.rem_euclid(TICKS_PER_DAY)) / TICKS_PER_HOUR
    }

    /// 当年的第几天，取值 `0..(DAYS_PER_SEASON * SEASONS_PER_YEAR)`。
    pub const fn day_of_year(&self) -> i64 {
        let days_per_year = DAYS_PER_SEASON * SEASONS_PER_YEAR;
        (self.0.div_euclid(TICKS_PER_DAY)).rem_euclid(days_per_year)
    }

    /// 当前季节。
    pub const fn season(&self) -> Season {
        match self.day_of_year() / DAYS_PER_SEASON {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Autumn,
            // day_of_year 已对一年取模，故此分支只可能是第四季。
            _ => Season::Winter,
        }
    }

    /// 当前是否为白昼。
    ///
    /// # 与环境光照曲线是同一份定义，不是另一条边界
    ///
    /// 这里曾经是一条独立选定的 `6..18` 小时边界，与 `ll-world::light`
    /// 的昼夜渐变曲线（日出 5–7 点、日落 17–19 点）各自维护，结果两者
    /// 在早晚各约两小时的窗口里结论相反——`is_daylight()` 说是白天，
    /// 光照却只有四五成亮度。旧版本的文档在这里写着「后续若要让日照
    /// 长度随季节变化，应在此处调整边界，而非另设时钟」，而
    /// `ll-world::light` 恰恰另设了一套边界，正是这条警告想拦住的事。
    ///
    /// 现在改为直接从 [`crate::light::day_curve`] 这条唯一的基准曲线
    /// 派生：曲线值达到或超过 [`crate::light::DAYLIGHT_THRESHOLD`]
    /// （曲线中点）即为白昼。曲线以后若要按季节调整日照时长，这里会
    /// 自动跟着变，不需要再有一条平行维护的边界。
    pub fn is_daylight(&self) -> bool {
        crate::light::day_curve(*self) >= crate::light::DAYLIGHT_THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 一天结束时小时数归零() {
        // Arrange
        let midnight = Tick(TICKS_PER_DAY);

        // Act
        let hour = midnight.hour_of_day();

        // Assert
        assert_eq!(hour, 0);
    }

    #[test]
    fn 一年第一天属于春季() {
        // Arrange
        let start = Tick(0);

        // Act
        let season = start.season();

        // Assert
        assert_eq!(season, Season::Spring);
    }

    #[test]
    fn 跨过三个季节长度后进入冬季() {
        // Arrange
        let winter_start = Tick(TICKS_PER_DAY * DAYS_PER_SEASON * 3);

        // Act
        let season = winter_start.season();

        // Assert
        assert_eq!(season, Season::Winter);
    }

    #[test]
    fn 满一年后季节回到春季() {
        // 世界时钟会长期累加，季节必须正确循环而非越界。
        // Arrange
        let next_year = Tick(TICKS_PER_DAY * DAYS_PER_SEASON * SEASONS_PER_YEAR);

        // Act
        let season = next_year.season();

        // Assert
        assert_eq!(season, Season::Spring);
    }

    #[test]
    fn 午夜不是白昼() {
        // Arrange
        let midnight = Tick(0);

        // Act & Assert
        assert!(!midnight.is_daylight());
    }

    #[test]
    fn 正午是白昼() {
        // Arrange
        let noon = Tick(TICKS_PER_HOUR * 12);

        // Act & Assert
        assert!(noon.is_daylight());
    }

    #[test]
    fn 白昼判定与光照曲线的阈值恒一致() {
        // 这是本次收敛的核心不变式：is_daylight() 不能再悄悄漂移成
        // 一条独立维护的边界，必须始终等价于「光照曲线是否达到阈值」
        // ——覆盖日出日落窗口内外的若干采样点，包含旧版本固定边界
        // （6/18 点）与新阈值分歧的那个小时（18 点，见 light.rs
        // 「日落窗口正中恰好等于判定阈值」测试）。
        // Arrange
        let hours = [0, 5, 6, 7, 12, 17, 18, 19, 23];

        // Act & Assert
        for hour in hours {
            let tick = Tick(hour * TICKS_PER_HOUR);
            let expected = crate::light::day_curve(tick) >= crate::light::DAYLIGHT_THRESHOLD;
            assert_eq!(tick.is_daylight(), expected);
        }
    }

    #[test]
    fn 负时刻不会得到负小时数() {
        // 读档迁移或时间倒流类效果可能产生负值，用取余会得到负小时。
        // Arrange
        let before_start = Tick(-TICKS_PER_HOUR);

        // Act
        let hour = before_start.hour_of_day();

        // Assert
        assert_eq!(hour, 23);
    }
}
