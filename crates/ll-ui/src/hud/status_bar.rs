//! 状态栏：时间 / 生命 / 法力——本批次（P7 第一批，只读观测界面）**唯一
//! 常驻、不需要按任何键就能看见**的一块，见任务书「关键约束一」：ADR
//! 0025 禁止合成按键验证实机截图，任何需要按键才能看到的内容都无法被
//! 所有者或本次实现验证，只有常驻的部分才能被截图直接证明。
//!
//! # 为什么生命/法力只显示当前值，不显示「当前/上限」
//!
//! `attribute-system.md` 与 `Agent::health`/`Agent::mana` 字段文档都
//! 明确记录：生命/法力的上限公式（分别由体质/智力衍生）**尚未落地**——
//! `Agent::STARTING_HEALTH`/`STARTING_MANA` 只是出生时刻的占位常量，
//! 不是可查询的「当前上限」。全仓库搜索确认当前没有任何
//! `max_health`/`max_mana` 一类的衍生值存在。若这里编出一个「当前/上限」
//! 格式却拿不出真实上限，只能编造一个假上限——这与项目一贯的「不编造
//! 尚未落地的数值」纪律冲突，因此本模块只显示当前值。等衍生公式真的
//! 落地，扩展这里显示「当前/上限」是一处局部改动，不影响本模块其余
//! 结构。
//!
//! # 为什么不显示季节
//!
//! 任务书明确列出状态栏必须显示的三项是「时间 / 生命 / 法力」，没有
//! 季节——保持最小范围（YAGNI），需要时再加。
//!
//! # 生命/法力条形：显示比例参照值，不是编造的上限
//!
//! 项目所有者追加要求血条/法力条要有动画（见
//! `crate::widget::anim`/`crate::widget::state` 模块文档）——条形需要
//! 一个分母才能算出填充比例，但上一节已经论证过真实上限不存在。
//! [`health_bar_fraction`]/[`mana_bar_fraction`] 因此拿
//! [`ll_world::entity::Agent::STARTING_HEALTH`]/
//! [`ll_world::entity::Agent::STARTING_MANA`]——本就存在、有文档记录
//! 「出生时刻占位常量」的既有常量——当**纯粹的显示参照值**，不是声称
//! 「这就是上限」：比例超过 1.0（例如被 buff 顶到超过起始值）时钳制
//! 显示满条，不会显示「超过 100%」这种没有意义的视觉。这是一处明确
//! 标注、如实记录的折中，不是编造：等衍生生命/法力上限公式真的落地，
//! 把分母换成真实上限是这两个函数内部的局部改动，`status_bar_panel`/
//! 调用点的接口不需要变。

use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR, TICKS_PER_MINUTE, Tick};
use ll_i18n::Catalog;
use ll_world::entity::Agent;

use super::{PanelContent, build_panel};

/// 生命条的填充比例——见模块文档「生命/法力条形」一节。
pub fn health_bar_fraction(health: i32) -> f32 {
    (health as f32 / Agent::STARTING_HEALTH as f32).clamp(0.0, 1.0)
}

/// 法力条的填充比例，理由同 [`health_bar_fraction`]。
pub fn mana_bar_fraction(mana: i32) -> f32 {
    (mana as f32 / Agent::STARTING_MANA as f32).clamp(0.0, 1.0)
}

/// 状态栏需要的全部输入：一次读三个来源（`world.clock`/`agent.health`/
/// `agent.mana`），不做任何衍生计算——衍生（日/时/分换算）留给
/// [`status_bar_text`] 内部完成，本类型只是把调用方已经有的三个值
/// 打包传递，避免函数签名变成三个裸参数（未来若要加第四项状态量，
/// 改这里一处即可，调用点不用跟着改参数列表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusBarData {
    /// 世界时钟——`WorldState::clock`。
    pub clock: Tick,
    /// 当前生命值——`Agent::health`。
    pub health: i32,
    /// 当前法力值——`Agent::mana`。
    pub mana: i32,
}

/// 把 `clock` 换算成「第 N 天 HH:MM」——绝对天数（从世界创建那一刻
/// 起累计，不是 `Tick::day_of_year` 那种按年份取模后的「今年第几天」），
/// 理由：状态栏要回答「游戏进行到第几天了」，不是「现在是这一年的第
/// 几天」，两个问题的答案在跨年后会不同。
///
/// 复用 `ll_core::time` 已有的 `TICKS_PER_DAY`/`TICKS_PER_HOUR`/
/// `TICKS_PER_MINUTE` 常量与 `Tick::hour_of_day`，不重新推导换算——
/// 任务书「数据从哪来」一节明确要求「别自己再算一遍」。
fn format_clock(clock: Tick) -> String {
    let day = clock.0.div_euclid(TICKS_PER_DAY);
    let hour = clock.hour_of_day();
    // `rem_euclid` 而非取余：与 `Tick::hour_of_day` 同一条既有纪律
    // （见其文档），世界时钟理论上不会为负，但防御性地保持一致写法。
    let minute = clock.0.rem_euclid(TICKS_PER_HOUR) / TICKS_PER_MINUTE;
    format!("{day} {hour:02}:{minute:02}")
}

/// 产出状态栏这一整行的最终显示文本——纯函数，不接触 GPU，可脱离窗口
/// 单元测试（本模块的测试就是这么做的）。
///
/// 三段标签（时间/生命/法力）经 `catalog` 按 `language` 解析，数值本身
/// 用 Rust 格式化拼接，不经 Fluent 变量插值——理由见 crate 顶层任务书
/// 「i18n」一节的既有边界：数字格式化本身不是「属性名/槽位名/物品名」
/// 那一类需要翻译的用户可见名词。
pub fn status_bar_text(data: &StatusBarData, catalog: &Catalog, language: &str) -> String {
    let time_label = catalog.resolve(language, "hud-status-time-label");
    let health_label = catalog.resolve(language, "hud-status-health-label");
    let mana_label = catalog.resolve(language, "hud-status-mana-label");
    format!(
        "{time_label} {}   {health_label} {}   {mana_label} {}",
        format_clock(data.clock),
        data.health,
        data.mana,
    )
}

/// 建出状态栏这一块面板：背景矩形 + 唯一一行文字
/// （[`status_bar_text`]）。这是状态栏在 [`super::render::render_hud`]
/// 里真正被调用的入口——常驻，不需要按任何键就能看见,见模块文档
/// 开篇。
pub fn status_bar_panel(
    data: &StatusBarData,
    catalog: &Catalog,
    language: &str,
    origin: (f32, f32),
    width: f32,
) -> PanelContent {
    build_panel(origin, width, |cursor, labels| {
        cursor.push(labels, status_bar_text(data, catalog, language));
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_fixture_catalog(dir: &Path) {
        // 中文字面量与 `.expect(` 同一行——`check_i18n_strings.py` 按
        // 「本行是否含诊断宏」逐行判定豁免（见其模块文档），拆成多行
        // 会让字面量单独落在一行、不再触发豁免；与 `ll_i18n::Catalog`
        // 自身测试的既有写法（`crates/ll-i18n/src/lib.rs`）保持一致。
        std::fs::write(dir.join("zh-CN.ftl"), "hud-status-time-label = 时间\nhud-status-health-label = 生命\nhud-status-mana-label = 法力\n").expect("测试用写入应当成功");
        std::fs::write(dir.join("en.ftl"), "hud-status-time-label = Time\nhud-status-health-label = HP\nhud-status-mana-label = MP\n").expect("测试用写入应当成功");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ll-ui-hud-status-bar-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        dir
    }

    #[test]
    fn 状态栏文本包含建局时刻的天数与时分() {
        // Arrange：建局那一刻（Tick(0)）——第 0 天 00:00。
        let dir = temp_dir("day-zero");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
        };

        // Act
        let text = status_bar_text(&data, &catalog, "zh-CN");

        // Assert
        assert!(text.contains("0 00:00"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏文本正确换算跨天的时分() {
        // Arrange：第 2 天 08:05——2 * TICKS_PER_DAY + 8 小时 + 5 分钟。
        let dir = temp_dir("day-two-eight-oh-five");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let clock = Tick(2 * TICKS_PER_DAY + 8 * TICKS_PER_HOUR + 5 * TICKS_PER_MINUTE);
        let data = StatusBarData {
            clock,
            health: 100,
            mana: 50,
        };

        // Act
        let text = status_bar_text(&data, &catalog, "zh-CN");

        // Assert
        assert!(text.contains("2 08:05"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏文本包含当前生命值() {
        // Arrange
        let dir = temp_dir("health-value");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 42,
            mana: 50,
        };

        // Act
        let text = status_bar_text(&data, &catalog, "zh-CN");

        // Assert
        assert!(text.contains("42"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏文本包含当前法力值() {
        // Arrange
        let dir = temp_dir("mana-value");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 12,
        };

        // Act
        let text = status_bar_text(&data, &catalog, "zh-CN");

        // Assert
        assert!(text.contains("12"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏文本随语言切换标签文字() {
        // Arrange
        let dir = temp_dir("language-switch");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
        };

        // Act
        let zh_text = status_bar_text(&data, &catalog, "zh-CN");
        let en_text = status_bar_text(&data, &catalog, "en");

        // Assert
        assert_ne!(zh_text, en_text);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏面板恒产出一行文字() {
        // Arrange
        let dir = temp_dir("panel-one-line");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
        };

        // Act
        let panel = status_bar_panel(&data, &catalog, "zh-CN", (0.0, 0.0), 400.0);

        // Assert
        assert_eq!(panel.labels.len(), 1);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏面板矩形宽度等于传入的宽度() {
        // Arrange
        let dir = temp_dir("panel-width");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
        };

        // Act
        let panel = status_bar_panel(&data, &catalog, "zh-CN", (0.0, 0.0), 400.0);

        // Assert
        assert_eq!(panel.rect.width, 400.0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 满生命值时生命条比例为满() {
        // Arrange & Act
        let fraction = health_bar_fraction(Agent::STARTING_HEALTH);

        // Assert
        assert_eq!(fraction, 1.0);
    }

    #[test]
    fn 半生命值时生命条比例为一半() {
        // Arrange & Act
        let fraction = health_bar_fraction(Agent::STARTING_HEALTH / 2);

        // Assert
        assert_eq!(fraction, 0.5);
    }

    #[test]
    fn 生命值超过参照值时比例钳制到满而不是超过一() {
        // Arrange & Act：buff 顶到超过起始值。
        let fraction = health_bar_fraction(Agent::STARTING_HEALTH * 2);

        // Assert
        assert_eq!(fraction, 1.0);
    }

    #[test]
    fn 满法力值时法力条比例为满() {
        // Arrange & Act
        let fraction = mana_bar_fraction(Agent::STARTING_MANA);

        // Assert
        assert_eq!(fraction, 1.0);
    }
}
