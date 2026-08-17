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

/// 白昼开始的小时（含）。
const DAYLIGHT_START_HOUR: i64 = 6;

/// 白昼结束的小时（不含）。
const DAYLIGHT_END_HOUR: i64 = 18;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
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
    /// 现阶段昼夜边界固定。后续若要让日照长度随季节变化，应在此处按
    /// [`Self::season`] 调整边界，而非另设时钟。
    pub const fn is_daylight(&self) -> bool {
        let hour = self.hour_of_day();
        hour >= DAYLIGHT_START_HOUR && hour < DAYLIGHT_END_HOUR
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
