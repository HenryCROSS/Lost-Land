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
/// # 为什么是 600 而不是 60
///
/// 曾经取 60（一刻度恰好等于一游戏秒，方便调试时肉眼换算），但那个
/// 取值让世界时钟走得太快：一次行动的基础代价——`ll_sim::timeline::
/// action_cost` 的入参 `BASE_ACTION_COST`（本 crate 不依赖 `ll-sim`，
/// 此处只是文字引用，不是可解析的文档内链，与 `ll_ui::hud::world_map`
/// 模块文档「本 crate 不依赖 `ll-platform`」一节同一条既有写法）
/// （`crates/ll-sim/src/resolve.rs`）恒为 100 刻度，60 刻度一分钟时
/// 一次行动就是 100/60 ≈ 1.67 游戏内分钟——玩家实测反馈「走过屏幕
/// 约 20 格就等于半小时游戏内时间」，太快。
///
/// 换成拉长一天（本常量 ×10）而不是改 `BASE_ACTION_COST`（缩小它），
/// 是权衡过的取舍：`action_cost(base_cost, effective_speed) =
/// base_cost * 1000 / effective_speed` 是整数除法，`base_cost` 越小，
/// 相邻敏捷值算出的耗时就越容易在取整后重合（分辨率变粗）——实测
/// `base_cost=100` 时敏捷 5..30 这段区间 26 个取值算出 26 个互不相同
/// 的耗时（满分辨率），`base_cost=10` 时同一段区间只剩 13 个互不相同
/// 的取值（一半塌缩到同一个耗时），高敏捷区间塌缩得更严重。本常量
/// 完全不出现在 `action_cost`/`effective_speed_from_dexterity`
/// 的公式里（两者只从 `ll_core::time::Tick` 取用原始刻度，不引用
/// `TICKS_PER_MINUTE`/`TICKS_PER_HOUR`/`TICKS_PER_DAY`），调大它不会
/// 触碰这条分辨率，`resolve`/`apply`/时间轴调度器的既有行为（含黄金
/// 基准回归）因此逐位不变，只有「一刻度对应多少分钟/小时/天」这一层
/// 日历换算变了——一次行动仍是 100 刻度，但 100 刻度现在只占
/// 100/600 ≈ 0.167 分钟（10 游戏内秒），一天需要走满 8640 步而不是
/// 864 步，走过屏幕不再等于半小时。
///
/// 代价：`ll-mod` 里少数内容表用裸刻度字面量描述冷却/持续时间（例如
/// `crates/ll-mod/src/skill.rs` 的 `materialize_base_skills`），这些
/// 数值相对于「一次行动」的意义不变（还是同一个绝对刻度数，行动代价
/// 本身没变，冷却在“需要几次行动”这个尺度上不变），只有拿这些裸刻度
/// 换算成分钟/小时展示给玩家时换算结果会变短——已核实这些字面量全部
/// 落在冷却/持续时间量级（个位数到三十出头），不到一分钟（600 刻度），
/// 换算展示层面几乎不可感知，判定不需要连带调整。
pub const TICKS_PER_MINUTE: i64 = 600;

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
