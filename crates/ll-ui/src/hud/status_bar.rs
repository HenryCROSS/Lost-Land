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
//! # 现在显示季节——核实结论：季节会真的影响玩法，不是纯装饰
//!
//! 此前的判断是「保持最小范围，不显示季节」；本批次所有者要求补上,
//! 补之前先核实了一件事：**如果季节只是显示出来却不影响任何东西,那
//! 这个显示就是误导玩家的**。核实结论——影响链路是真的接上的：
//! [`ll_world::light::season_light_scale`]（冬 750、春秋 900、夏 1000，
//! 千分比）被 [`ll_world::light::ambient_light`] 调用，`ambient_light`
//! 又经 [`ll_world::space_profile::effective_ambient_light`] 被
//! `ll_game::layout::effective_sight_radius` 调用——后者算出的半径就是
//! 玩家实机看到的视野半径（`ll_game::app` 每帧拿它决定画多远）。也就是
//! 说冬天视野确实比其余三季更小,不是纯换色板。这条链路上一批就已经
//! 存在,本批次只是核实并把结论写在这里,没有新接线。
//!
//! 季节名走 [`season_key`]，Fluent 键在 `assets/locales/*.ftl` 的
//! `season-*-display_name` 分组，与 [`crate::hud::character_panel`] 里
//! `attribute_key` 同一套「按枚举变体查键」的写法。
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
//!
//! # 昼夜滑条指针位置：纯函数,不做动画,回绕靠取模天然成立
//!
//! [`day_night_pointer_fraction`] 只回答「给定一个世界时刻,指针该停在
//! 滑条的百分之几」这一个问题——`0.0` 是当日 `00:00`（滑条最左端）,
//! 沿一整天线性推进,趋近 `1.0` 但恒小于它（次日 `00:00` 那一刻会重新
//! 从 `0.0` 算起,不是恰好等于 `1.0`）。**具体算法是拿 `clock.0` 对
//! [`TICKS_PER_DAY`] 取模再除以 [`TICKS_PER_DAY`]**——这一步刻意不是
//! 「累计经过的绝对刻度数除以某个常数」：那样算出来的分数会随游戏进行
//! 的天数无限增长,滑条第二天就会冲出条外而不是回到左端,是这类「一天
//! 一循环」显示最容易踩的坑（见本模块「跨天回绕」测试）。真正让指针
//! **平滑滑动**（而不是每次世界时钟推进就瞬间跳到新位置）的动画发生
//! 在调用点（`crate::hud::render::build_hud_frame` 经
//! `WidgetStateTable::animate`）,本函数只产出这一帧的真实目标位置,
//! 不知道、也不需要知道动画的存在——与 [`health_bar_fraction`] 「只算
//! 真实比例，动画是调用点的事」同一条分工。

use ll_core::time::{Season, TICKS_PER_DAY, TICKS_PER_HOUR, TICKS_PER_MINUTE, Tick};
use ll_i18n::Catalog;
use ll_text::MeasureText;
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

/// 昼夜滑条指针的归一化位置——见模块文档「昼夜滑条指针位置」一节。
pub fn day_night_pointer_fraction(clock: Tick) -> f32 {
    let ticks_into_day = clock.0.rem_euclid(TICKS_PER_DAY);
    ticks_into_day as f32 / TICKS_PER_DAY as f32
}

/// 把 [`Season`] 变体映射到 Fluent 键，写法同
/// `crate::hud::character_panel::attribute_key`。
fn season_key(season: Season) -> &'static str {
    match season {
        Season::Spring => "lostland:season.spring.display_name",
        Season::Summer => "lostland:season.summer.display_name",
        Season::Autumn => "lostland:season.autumn.display_name",
        Season::Winter => "lostland:season.winter.display_name",
    }
}

/// 状态栏需要的全部输入：一次读三个世界状态来源（`world.clock`/
/// `agent.health`/`agent.mana`）加一个表现层专属的 `fps`，不做任何衍生
/// 计算——衍生（日/时/分换算）留给 [`status_bar_fields`] 内部完成，本
/// 类型只是把调用方已经有的几个值打包传递，避免函数签名变成一长串裸
/// 参数（未来若要加下一项状态量，改这里一处即可，调用点不用跟着改
/// 参数列表）。
///
/// `fps` 是本结构体里唯一不来自 `WorldState` 的字段——见其文档：来自
/// `ll_platform::fps::FpsCounter`，只活在表现层，`status_bar_fields` 只是
/// 把这个已经算好的浮点数格式化进文本，不知道、也不需要知道它是怎么
/// 算出来的。因为混进了这个浮点字段，本结构体不再能派生 `Eq`（`f32`
/// 不实现 `Eq`），只保留 `PartialEq`——这与
/// `crate::widget::anim::AnimatedValue`（同样含 `f32` 字段、同样只派生
/// `PartialEq`）是同一个理由。
///
/// # 为什么带一个生命周期参数
///
/// [`Self::weather_display_name_key`] 是一个借来的字符串切片——天气是
/// **mod 可注册**的内容（`ll_world::weather`），它的本地化键不是一个
/// 有限枚举，没有 `&'static str` 可用（季节名那种写死的 [`season_key`]
/// 在这里行不通）。借用而不是 `String`，是为了保住本结构体的 `Copy`：
/// 它每帧构造一次，`Copy` 让调用点不必关心所有权。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusBarData<'a> {
    /// 世界时钟——`WorldState::clock`。
    pub clock: Tick,
    /// 当前生命值——`Agent::health`。
    pub health: i32,
    /// 当前法力值——`Agent::mana`。
    pub mana: i32,
    /// 当前显示用的平滑帧率（见 `ll_platform::fps` 模块文档「平滑算法」
    /// 一节）——不是世界状态，纯粹的表现层读数，数字仍然瞬时显示（本
    /// 字段本身已经是平滑过的值，`status_bar_fields` 不会再对它做二次
    /// 动画，符合「数字瞬时，条形动画」硬规则：这里既不是条形，平滑也
    /// 发生在调用点而不是 `crate::widget::anim::AnimatedValue`）。
    pub fps: f32,
    /// 当前天气展示名的 Fluent 本地化键（天气系统批次新增），`None`
    /// 表示这一刻没有天气（晴空基准，或世界里压根没注册任何天气）。
    ///
    /// # 为什么状态栏要显示天气——与季节同一条判据
    ///
    /// 本模块「现在显示季节」一节立过一条规矩：**如果一样东西只是显示
    /// 出来却不影响任何东西，那这个显示就是在误导玩家**。天气过得了这
    /// 条判据，而且链路比季节还短一截：
    /// `ll_world::weather::Weather::derive` → `WeatherDef::light_scale`
    /// → `ll_world::light::ambient_light_under` →
    /// `ll_world::space_profile::effective_ambient_light` →
    /// `ll_game::layout::effective_sight_radius`/`effective_tint`——
    /// 前者就是玩家实机看到的视野半径，后者就是画面亮度；
    /// `WeatherDef::sight_scale` 还在视野那一路上再乘一次。也就是说
    /// 下雨天确实看得更近、画面更暗，不是纯换色板。
    ///
    /// 反过来说，**不显示**才是有害的：玩家会看到画面突然变暗、视野
    /// 突然缩短，却没有任何线索说明为什么。
    pub weather_display_name_key: Option<&'a str>,
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

/// 状态栏这一帧的**每一格**——纯函数，不接触 GPU，可脱离窗口单元
/// 测试（本模块的测试就是这么做的）。
///
/// # 为什么是一列格子，不是一整行字符串（规格 W6）
///
/// 此前这里把六段翻译 `format!` 成**一个** `String`、交给一个 `Label`
/// 画出去。`knowledge/design/ui-and-navigation.md` §8.5 W6 记着这条的
/// 后果：那一整行只要有一段变长（英文的 `Overcast`、某个译者把「帧率」
/// 写成「每秒帧数」），**整行**就一起超出面板内容宽，于是尾巴上的帧率
/// 被挤到第二行去（溢出清单 O-1 的根子）。
///
/// 拆成独立的格子之后，某一段变长只影响它自己那一格与它右边那几格的
/// 起点，不再把整行拖成两行。横排由
/// [`crate::widget::list::RowCursor::push_fields`] 做，格间距取
/// [`crate::widget::metrics::PANEL_GAP`]（批次 30 收敛出来的间距刻度，
/// 不新造一个常量）。
///
/// # 括号为什么没有了
///
/// 季节与天气此前共用一对括号，理由原文是「分开成两组括号只会让这行
/// 更拥挤」——那条理由的前提是这些段挤在**同一条连续文本**里。拆成
/// 各自定位的格子之后，格与格之间靠间隔区分，括号不再承担分隔职责。
///
/// # 没有天气时是五格，不是一个空格子
///
/// [`StatusBarData::weather_display_name_key`] 本来就是 `Option`。编一个
/// 空格子出来只会让那一格的间隔无缘无故存在（规格 W6 的判据写「≥6 个
/// 标签」，说的是有天气那一档）。
///
/// 各段标签经 `catalog` 按 `language` 解析，数值本身用 Rust 格式化拼接，
/// 不经 Fluent 变量插值——理由见 crate 顶层任务书「i18n」一节的既有
/// 边界：数字格式化本身不是「属性名/槽位名/物品名」那一类需要翻译的
/// 用户可见名词。季节名（[`season_key`]）与属性名/槽位名同一类——是一个
/// 有限枚举的展示名，因此和它们一样走 `catalog.resolve`。
///
/// `fps` 按四舍五入到整数显示（`{:.0}`）——玩家关心的是「大概多少帧」，
/// 平滑算法本身已经抹平了逐帧抖动（见 `ll_platform::fps` 模块文档），
/// 小数位不会带来任何额外信息，只会让这一格更宽。
pub fn status_bar_fields(
    data: &StatusBarData<'_>,
    catalog: &Catalog,
    language: &str,
) -> Vec<String> {
    let 标签 = |key: &str| catalog.resolve(language, key);
    let mut fields = vec![
        format!(
            "{} {}",
            标签("hud-status-time-label"),
            format_clock(data.clock)
        ),
        catalog.resolve(language, season_key(data.clock.season())),
    ];
    if let Some(key) = data.weather_display_name_key {
        fields.push(catalog.resolve(language, key));
    }
    fields.push(format!(
        "{} {}",
        标签("hud-status-health-label"),
        data.health
    ));
    fields.push(format!("{} {}", 标签("hud-status-mana-label"), data.mana));
    fields.push(format!("{} {:.0}", 标签("hud-status-fps-label"), data.fps));
    fields
}

/// 建出状态栏这一块面板：背景矩形 + **横排的那一列格子**
/// （[`status_bar_fields`]）。这是状态栏在 [`super::render::render_hud`]
/// 里真正被调用的入口——常驻，不需要按任何键就能看见,见模块文档
/// 开篇。
pub fn status_bar_panel(
    data: &StatusBarData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn MeasureText,
    origin: (f32, f32),
    width: f32,
) -> PanelContent {
    build_panel(measure, origin, width, |cursor, labels| {
        cursor.push_fields(
            labels,
            &status_bar_fields(data, catalog, language),
            crate::widget::metrics::PANEL_GAP,
        );
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 把这一帧的全部格子拼成一条字符串——**只给「这一行里有没有出现
    /// 某段内容」那一类断言用**，不是生产代码里的第二份排版：真正画到
    /// 屏幕上的是 [`status_bar_fields`] 的每一格各自一个 `Label`
    /// （规格 W6）。格子的**位置**由 `状态栏面板*` 那几条直接断言。
    fn 拼起来(data: &StatusBarData<'_>, catalog: &Catalog, language: &str) -> String {
        status_bar_fields(data, catalog, language).join("  ")
    }

    fn write_fixture_catalog(dir: &Path) {
        // 中文字面量与 `.expect(` 同一行——`check_i18n_strings.py` 按
        // 「本行是否含诊断宏」逐行判定豁免（见其模块文档），拆成多行
        // 会让字面量单独落在一行、不再触发豁免；与 `ll_i18n::Catalog`
        // 自身测试的既有写法（`crates/ll-i18n/src/lib.rs`）保持一致。
        std::fs::write(dir.join("zh-CN.ftl"), "hud-status-time-label = 时间\nhud-status-health-label = 生命\nhud-status-mana-label = 法力\nhud-status-fps-label = 帧率\nseason-spring-display_name = 春\nseason-summer-display_name = 夏\nseason-autumn-display_name = 秋\nseason-winter-display_name = 冬\nweather-rain-display_name = 雨\n").expect("测试用写入应当成功");
        std::fs::write(dir.join("en.ftl"), "hud-status-time-label = Time\nhud-status-health-label = HP\nhud-status-mana-label = MP\nhud-status-fps-label = FPS\nseason-spring-display_name = Spring\nseason-summer-display_name = Summer\nseason-autumn-display_name = Autumn\nseason-winter-display_name = Winter\nweather-rain-display_name = Rain\n").expect("测试用写入应当成功");
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
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let text = 拼起来(&data, &catalog, "zh-CN");

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
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let clock = Tick(2 * TICKS_PER_DAY + 8 * TICKS_PER_HOUR + 5 * TICKS_PER_MINUTE);
        let data = StatusBarData {
            clock,
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let text = 拼起来(&data, &catalog, "zh-CN");

        // Assert
        assert!(text.contains("2 08:05"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏文本在有天气时把天气名显示在季节旁边() {
        // 天气真的影响视野与画面亮度（见 StatusBarData::weather_display_name_key
        // 文档），因此必须让玩家看得见——否则画面突然变暗、视野突然缩短
        // 却没有任何线索说明为什么。
        // Arrange
        let dir = temp_dir("weather-shown");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: Some("lostland:weather.rain.display_name"),
        };

        // Act
        let text = 拼起来(&data, &catalog, "zh-CN");

        // Assert：天气名经 catalog 解析而不是把键名直接印出来，且它
        // 自己就是一格（规格 W6 拆字段之后括号不再承担分隔职责，见
        // `status_bar_fields` 文档「括号为什么没有了」一节）。
        assert!(text.contains("雨"), "实际文本：{text}");
        let fields = status_bar_fields(&data, &catalog, "zh-CN");
        assert_eq!(fields[1], "春", "第 2 格是季节");
        assert_eq!(fields[2], "雨", "第 3 格是天气");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏在没有天气时少一格而不是留一个空格子() {
        // 没有天气（晴空基准／世界里没注册任何天气）时**不该留下一个
        // 空格子**——那一格的间隔会无缘无故地存在。规格 W6 的判据写
        // 「≥6 个标签」说的是有天气那一档，见 `status_bar_fields` 文档
        // 「没有天气时是五格」一节。
        // Arrange
        let dir = temp_dir("weather-absent");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let text = 拼起来(&data, &catalog, "zh-CN");

        // Assert
        assert!(text.contains("春"), "实际文本：{text}");
        let fields = status_bar_fields(&data, &catalog, "zh-CN");
        assert_eq!(fields.len(), 5, "无天气时五格：{fields:?}");
        assert_eq!(fields[1], "春");
        assert_eq!(fields[2], "生命 100", "季节之后直接接生命，中间没有空格子");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏文本包含当前生命值() {
        // Arrange
        let dir = temp_dir("health-value");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 42,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let text = 拼起来(&data, &catalog, "zh-CN");

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
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 12,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let text = 拼起来(&data, &catalog, "zh-CN");

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
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let zh_text = 拼起来(&data, &catalog, "zh-CN");
        let en_text = 拼起来(&data, &catalog, "en");

        // Assert
        assert_ne!(zh_text, en_text);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏面板把每一格各画成一个标签且全部在同一行上() {
        // Arrange
        let dir = temp_dir("panel-one-line");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let panel = status_bar_panel(
            &data,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            (0.0, 0.0),
            400.0,
        );

        // Assert：规格 W6 的主判据——一格一个 `Label`，不是拼成一整行。
        //
        // 反例验证（已实跑）：把 `status_bar_panel` 改回
        // `cursor.push(labels, fields.join(" "))`，本条红在「5 ≠ 1」。
        let fields = status_bar_fields(&data, &catalog, "zh-CN");
        assert_eq!(
            panel.labels.len(),
            fields.len(),
            "每一格恰好一个标签：{fields:?}"
        );
        for (label, field) in panel.labels.iter().zip(fields.iter()) {
            assert_eq!(&label.text, field);
        }
        // 真的是**横排**：全部格子同一个 y，且 x 严格递增。
        //
        // 反例验证（已实跑）：把 `RowCursor::push_fields` 改成逐格调
        // `push`（即每格换一行），本条红在「第 1 格的 y」。
        let 首行y = panel.labels[0].y;
        for (i, label) in panel.labels.iter().enumerate() {
            assert_eq!(label.y, 首行y, "第 {i} 格的 y 应当与第一格相同");
        }
        for pair in panel.labels.windows(2) {
            assert!(
                pair[1].x > pair[0].x,
                "格子应当从左往右摆：{} 之后是 {}",
                pair[0].x,
                pair[1].x
            );
        }
        // 面板只有一行高——横排不该把高度撑成五行。
        assert_eq!(
            panel.rect.height,
            super::super::DEFAULT_LINE_HEIGHT + super::super::DEFAULT_PADDING * 2.0
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏面板矩形宽度等于传入的宽度() {
        // Arrange
        let dir = temp_dir("panel-width");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let panel = status_bar_panel(
            &data,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            (0.0, 0.0),
            400.0,
        );

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

    #[test]
    fn 状态栏文本包含当前季节名称() {
        // Arrange：日 40 落在夏季（0..30 春、30..60 夏）。
        let dir = temp_dir("season-name");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(40 * TICKS_PER_DAY),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };

        // Act
        let text = 拼起来(&data, &catalog, "zh-CN");

        // Assert
        assert!(text.contains('夏'));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 状态栏文本包含四舍五入到整数的帧率() {
        // Arrange
        let dir = temp_dir("fps-value");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let data = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 59.6,
            weather_display_name_key: None,
        };

        // Act
        let text = 拼起来(&data, &catalog, "zh-CN");

        // Assert：59.6 四舍五入显示为 60，不带小数位。
        assert!(text.contains("60"));
        assert!(!text.contains("59.6"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 帧率不同时状态栏文本随之变化() {
        // Arrange
        let dir = temp_dir("fps-changes-text");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let low_fps = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 30.0,
            weather_display_name_key: None,
        };
        let high_fps = StatusBarData {
            fps: 144.0,
            ..low_fps
        };

        // Act
        let low_text = 拼起来(&low_fps, &catalog, "zh-CN");
        let high_text = 拼起来(&high_fps, &catalog, "zh-CN");

        // Assert
        assert_ne!(low_text, high_text);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 昼夜滑条指针在午夜时位于滑条最左端() {
        // Arrange & Act
        let fraction = day_night_pointer_fraction(Tick(0));

        // Assert
        assert_eq!(fraction, 0.0);
    }

    #[test]
    fn 昼夜滑条指针在正午时位于滑条正中间() {
        // Arrange & Act
        let fraction = day_night_pointer_fraction(Tick(12 * TICKS_PER_HOUR));

        // Assert
        assert_eq!(fraction, 0.5);
    }

    #[test]
    fn 昼夜滑条指针在黄昏时位于当日时刻对应的比例() {
        // Arrange：18:00，一天的 18/24 处。
        // Act
        let fraction = day_night_pointer_fraction(Tick(18 * TICKS_PER_HOUR));

        // Assert
        assert_eq!(fraction, 0.75);
    }

    #[test]
    fn 跨天回绕时指针回到滑条左端而不是继续向右冲出去() {
        // 这条测试专门防「用绝对 tick 除以某个数」的错误实现——那样
        // 第二天指针的分数会比前一天 23:59 那一刻更大（继续右冲），
        // 而不是回绕到接近零。见模块文档「昼夜滑条指针位置」一节。
        // Arrange：第 0 天 23:59，与紧接着的第 1 天 00:01。
        let near_midnight = Tick(23 * TICKS_PER_HOUR + 59 * TICKS_PER_MINUTE);
        let next_day_just_after_midnight = Tick(TICKS_PER_DAY + TICKS_PER_MINUTE);

        // Act
        let near_midnight_fraction = day_night_pointer_fraction(near_midnight);
        let next_day_fraction = day_night_pointer_fraction(next_day_just_after_midnight);

        // Assert：次日刚过午夜的分数应远小于前一天临近午夜的分数，
        // 即指针回到了左端，而不是继续朝右端累加。
        assert!(next_day_fraction < near_midnight_fraction);
    }
}
