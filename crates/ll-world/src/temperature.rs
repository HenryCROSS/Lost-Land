//! 温度——区域基准 + 季节偏移 + 天气偏移 + 昼夜偏移。
//!
//! # 温度是纯派生的，零存档状态
//!
//! 本模块**没有任何字段进 [`crate::state::WorldState`]**。「此刻这里
//! 多冷」是 `(空间层属性, 世界时钟, 世界种子)` 的纯函数，与
//! [`crate::light`]（光照）、[`crate::weather`]（天气）是同一条纪律的
//! 第三次复用，理由逐字相同：
//!
//! 1. **零存档字段、零同步问题**。温度若存进 `WorldState`，就必须有人
//!    在时钟推进、玩家换空间、天气切换时把它改掉；漏改一次就表现成
//!    「进了洞窟还在挨冻」，而查代码时空间、时钟、天气各自都对，只有
//!    四者一起看才发现矛盾。
//! 2. **约束 C3/C4 天然满足**。温度自己**一次随机数都不掷**——它唯一
//!    的随机来源是天气，而天气的随机性已经全部收敛到
//!    [`ll_core::rng::DetRng::for_entity`]（C3），且只由 `(种子, 刻度)`
//!    决定（C4）。温度建在天气之上，因此天然继承了这两条：后台推进到
//!    确定 tick 之后再问温度，答案与从头逐 tick 走过来完全一致。
//! 3. **约束 C5**。本模块不遍历任何容器，四个加数全部是常量级查表或
//!    算术，不存在迭代顺序问题。
//!
//! # 单位：十分之一摄氏度（ADR 0020 乙区）
//!
//! [`Temperature`] 是一个整数，单位是十分之一摄氏度——`200` 即 20℃，
//! `-120` 即 -12℃。温度会流进结算（见 `ll_sim::exposure`：低温削弱
//! 力量，进而改变伤害数值），属于 ADR 0020 的**乙区**，必须量化为整数，
//! 一个浮点都不能有。
//!
//! 选十分之一度而不是整度：季节/昼夜/天气三个偏移量叠加之后，整度的
//! 分辨率会让「阴天比晴天冷一点」这种一两度以内的差别在取整后整个塌
//! 缩掉；十分之一度既有足够分辨率，又离 `i32` 的上下界远得不可能溢出
//! （±2.1×10^8 度）。这与 [`crate::light::LightLevel`] 选千分比而不是
//! 百分比是同一条取舍。
//!
//! # 四个加数，只有露天空间吃后三个
//!
//! ```text
//! 温度 = SpaceProfile.base_temperature   ← 区域基准
//!      + 季节偏移                        ┐
//!      + 天气偏移                        │ 仅当 exposed_to_sky
//!      + 昼夜偏移                        ┘
//! ```
//!
//! 「这个空间受不受外界影响」这个判断**只有一处**——
//! [`crate::space_profile::effective_weather`]，光照那一路
//! （[`crate::space_profile::effective_ambient_light`]）已经在用它。
//! 温度这一路的入口 [`crate::space_profile::effective_temperature`] 与
//! 它并列，走同一个字段、同一个函数，不新增第二条判据。非露天空间的
//! 温度恒等于自己的 `base_temperature`，与世界时钟、与外面在不在下雪
//! 都无关——洞窟冬暖夏凉这件事因此是免费得到的，不需要额外的建模。
//!
//! # 三个偏移量为什么是加法，不是乘法
//!
//! 见 [`crate::weather::WeatherDef::temperature_offset`] 文档「为什么
//! 是增量而不是乘数」一节：温度没有「零点」可言，乘法会得出「-20℃ 打
//! 八折等于变暖」这种荒谬结论。三个偏移量因此形状一致，都是加法项，
//! 相加的顺序不影响结果。
//!
//! # 这些具体数值是内容取舍，不是结构性常量
//!
//! [`SEASON_TEMPERATURE_OFFSETS`] 与 [`DIURNAL_SWING`] 的取值都可以在
//! 后续批次调整，本模块给出的是一组内部自洽的默认值，硬约束只有一条：
//! **本体地表（`base_temperature = 200`）在春夏两季、以及任何季节的
//! 白天，都不得跌破冰点**——那是 `ll_sim::exposure` 那条「平时完全不
//! 触发」的红线在本表数值上的投影，有测试钉住（见本模块测试
//! `春夏两季无论昼夜天气地表都不结冰`）。

use ll_core::light::day_curve_deviation_permille;
use ll_core::time::{Season, Tick};

use crate::weather::Weather;

/// 温度，单位是十分之一摄氏度（`200` = 20℃）。见模块文档「单位」一节。
///
/// 新类型而不是裸 `i32`：温度与 [`crate::light::LightLevel`]（千分比）、
/// 与天气的两个乘数（也是千分比）在类型上都是 `i32`，四者混用一个裸
/// 整数时，把光照当温度传进去编译器不会有任何意见。包一层之后这类错
/// 位在编译期就被挡住——与 `LightLevel` 当初包一层是同一条理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Temperature(pub i32);

impl Temperature {
    /// 冰点，0℃。
    ///
    /// 这是**唯一**一条与温度有关的玩法阈值：`ll_sim::exposure` 判定
    /// 「要不要产生后果」时比的就是它（体感温度低于冰点才有后果）。
    /// 取摄氏零度而不是另挑一个数字，是因为它是玩家不需要学习就能理解
    /// 的那一条线——状态栏显示「-3℃」时，玩家不必查任何数值表就知道
    /// 自己该穿件衣服了。
    pub const FREEZING: Temperature = Temperature(0);

    /// 「没有环境信息可用」时的中性温度，20℃。
    ///
    /// # 这不是一个内容数值，是一个空对象
    ///
    /// 结算侧（`ll_sim`）总会遇到「这次调用没有空间层属性表可查」的
    /// 情形——不装载任何内容表的验收 demo、只测移动/开门的单元测试、
    /// 以及 `ll_sim::resolve::resolve` 这个不带任何目录的薄入口。这些
    /// 调用点需要一个「温度这一路等于没接」的取值，本常量就是它，与
    /// `ll_sim::skill::NoSkills` 一类空实现是同一个角色。
    ///
    /// 取 20℃（而不是 0℃ 或 `i32::MAX`）：它必须**明确高于冰点**，
    /// 否则「没接温度」会退化成「恒定处于极寒」，把一个本该无声的空
    /// 对象变成全局惩罚；同时它应当是一个说得通的常温值，让「温度这
    /// 一路没接」在调试输出里读起来不像一个哨兵魔数。
    pub const TEMPERATE_BASELINE: Temperature = Temperature(200);

    /// 是否低于冰点。`ll_sim::exposure` 的判据，写成方法是为了让「低于
    /// 冰点」这句话只有一处定义（比较方向若在两处各写一遍，迟早有一处
    /// 写成 `<=`）。
    pub const fn is_freezing(self) -> bool {
        self.0 < Self::FREEZING.0
    }

    /// 加上一个增量，得到新的温度值。
    ///
    /// 饱和加法而不是 `+`：三个偏移量叠加在正常取值下离 `i32` 边界远
    /// 得离谱，但 `base_temperature` 是 mod 可以任意填的 `i32`（注册期
    /// 只校验它能放进 `i32`，见 `ll_mod::script_space_profile_api`），
    /// 一个填了 `i32::MAX` 的层属性不该让整局游戏 panic——与
    /// `ll_sim::formula::eval_formula` 全程饱和运算是同一条既有纪律。
    pub const fn offset_by(self, delta: i32) -> Temperature {
        Temperature(self.0.saturating_add(delta))
    }
}

/// 四季各自的温度偏移，十分之一摄氏度，下标由
/// [`crate::weather::season_slot`] 给出（春/夏/秋/冬）。
///
/// # 取值论证
///
/// 以本体地表 `base_temperature = 200`（20℃，全年均温）为参照：
///
/// | 季节 | 偏移 | 地表均温 | 正午 | 午夜 |
/// |------|------|----------|------|------|
/// | 春   | 0    | 20℃      | 26℃  | 14℃  |
/// | 夏   | +100 | 30℃      | 36℃  | 24℃  |
/// | 秋   | -20  | 18℃      | 24℃  | 12℃  |
/// | 冬   | -180 | 2℃       | 8℃   | -4℃  |
///
/// 只有**冬季的夜晚**跌破冰点，这是刻意的：`ll_sim::exposure` 那条
/// 「平时完全不触发」的红线要求春夏、以及任何季节的白天都不能有后果，
/// 而「冬夜出门要穿衣服」正是这套系统想表达的唯一一件事。
///
/// 冬季的幅度（-18℃）明显大于其余三季，是因为四季偏移必须与
/// [`DIURNAL_SWING`]（±6℃）和天气偏移（本体最多 -8℃）**一起**才够把
/// 一个 20℃ 的地表推到冰点以下：`200 - 180 - 60 = -40`（-4℃），叠上
/// 雪再降到 -12℃。三者若都只有几度，任何组合都跌不破冰点，整套惩罚
/// 就成了永远不触发的死代码。
pub const SEASON_TEMPERATURE_OFFSETS: [i32; 4] = [0, 100, -20, -180];

/// 昼夜温差的**半幅**，十分之一摄氏度：正午 `+DIURNAL_SWING`，午夜
/// `-DIURNAL_SWING`，全天温差因此是它的两倍（12℃）。
///
/// 12℃ 的昼夜温差对应现实里的内陆温带气候，是一个玩家不会觉得奇怪的
/// 量级；更重要的是它**足够大到让「等天亮」成为一个真实的选择**——
/// 冬夜地表 -4℃、冬日正午 8℃，中间隔着 12℃，正好是一件衣服的绝缘量级
/// （见 `ll_sim::exposure` 的惩罚台阶），玩家因此可以在「穿够衣服」与
/// 「等到天亮再走」之间真的做取舍，而不是只有一条路可走。
pub const DIURNAL_SWING: i32 = 60;

/// 某个季节的温度偏移，十分之一摄氏度。
///
/// 单独成函数而不是让调用方自己去索引 [`SEASON_TEMPERATURE_OFFSETS`]：
/// 与 [`crate::weather::season_slot`] 单独成函数是同一条理由——四季与
/// 下标的对应关系一旦有两处各写一份，就会出现「春天用了冬天的偏移」
/// 这种查起来极痛苦的错位。本函数内部复用的正是 `season_slot`，不另写
/// 一个 `match`。
pub fn season_temperature_offset(season: Season) -> i32 {
    SEASON_TEMPERATURE_OFFSETS[crate::weather::season_slot(season)]
}

/// 某一世界时刻的昼夜温度偏移，十分之一摄氏度，取值
/// `-DIURNAL_SWING..=DIURNAL_SWING`。
///
/// # 与白昼判定共用同一条曲线，不是第二条边界
///
/// 偏移量直接由 [`ll_core::light::day_curve_deviation_permille`] 缩放
/// 得来，而那个函数的零点就是 [`ll_core::time::Tick::is_daylight`] 的
/// 判定阈值（有测试钉住这条等价，见 `ll_core::light` 的
/// `昼夜偏离的零点与白昼判定阈值是同一条边界`）。因此「天亮了」与
/// 「开始回暖」在时间轴上是同一刻，不需要有人记得去同步两条平行维护
/// 的边界——这正是 `ll_core::light` 模块文档「为什么这条曲线要下沉到
/// `ll-core`」当初要解决的那个问题，本函数是它的第三个受益者
/// （前两个是环境光与 `is_daylight`）。
///
/// # 全程整数
///
/// 千分比偏离先乘半幅再除以 1000，不是先除后乘：先除会让绝大多数刻度
/// 的偏离被整数除法舍成 0，昼夜温差整个消失。
pub fn day_night_temperature_offset(tick: Tick) -> i32 {
    day_curve_deviation_permille(tick) * DIURNAL_SWING / 1000
}

/// **露天空间**在某一世界时刻、某种天气下的温度。
///
/// `base` 是 [`crate::space_profile::SpaceProfile::base_temperature`]。
/// 三个偏移量按模块文档的公式相加。
///
/// # 生产路径不该直接调用本函数
///
/// 与 [`crate::light::ambient_light_under`] 同一条纪律：本函数不知道
/// 调用它的是地表还是地下城——它**无条件**叠加季节与昼夜，对一个不
/// 露天的空间来说那是错的。唯一正确的入口是
/// [`crate::space_profile::effective_temperature`]，它先按
/// `exposed_to_sky` 分流，再决定要不要走到这里。本函数保持公开只是为
/// 了让「露天温度怎么算」这一段能被单独测试与复用，与
/// `ambient_light_under` 的处境完全一致。
pub fn temperature_under(base: i32, tick: Tick, weather: Weather) -> Temperature {
    Temperature(base)
        .offset_by(season_temperature_offset(tick.season()))
        .offset_by(weather.temperature_offset)
        .offset_by(day_night_temperature_offset(tick))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::base_weather_fixture;
    use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR};

    /// 本体地表的温度基准（见
    /// `crate::space_profile::materialize_base_space_profiles`）。
    const SURFACE_BASE: i32 = 200;

    /// 某一季某一天的午夜刻度。`season_index` 取 0..4（春夏秋冬）。
    fn midnight_of(season_index: i64) -> Tick {
        Tick(season_index * 30 * TICKS_PER_DAY)
    }

    /// 某一季某一天的正午刻度。
    fn noon_of(season_index: i64) -> Tick {
        Tick(season_index * 30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR)
    }

    #[test]
    fn 昼夜偏移在正午与午夜恰好取到正负半幅() {
        // Arrange
        let midnight = Tick(0);
        let noon = Tick(12 * TICKS_PER_HOUR);

        // Act
        let at_midnight = day_night_temperature_offset(midnight);
        let at_noon = day_night_temperature_offset(noon);

        // Assert
        assert_eq!((at_midnight, at_noon), (-DIURNAL_SWING, DIURNAL_SWING));
    }

    #[test]
    fn 四季偏移按春夏秋冬的顺序取值() {
        // 错位（例如春天用了冬天的偏移）是这套下标映射最容易出的缺陷，
        // 逐个季节钉一遍。
        // Act & Assert
        assert_eq!(season_temperature_offset(Season::Spring), 0);
        assert_eq!(season_temperature_offset(Season::Summer), 100);
        assert_eq!(season_temperature_offset(Season::Autumn), -20);
        assert_eq!(season_temperature_offset(Season::Winter), -180);
    }

    #[test]
    fn 露天温度等于基准加三个偏移之和() {
        // 公式本身的直接验证：四个加数拆开各自算一遍，与函数结果比对。
        // Arrange：冬季午夜下雪——三个偏移全部取负，是最容易暴露符号
        // 写反的一组输入。
        let (ids, table) = base_weather_fixture();
        let tick = midnight_of(3);
        let snow = Weather {
            kind: Some(ids.snow),
            light_scale: table.light_scale(ids.snow),
            sight_scale: table.sight_scale(ids.snow),
            temperature_offset: table.temperature_offset(ids.snow),
        };

        // Act
        let actual = temperature_under(SURFACE_BASE, tick, snow);

        // Assert
        let expected = SURFACE_BASE
            + season_temperature_offset(tick.season())
            + snow.temperature_offset
            + day_night_temperature_offset(tick);
        assert_eq!(actual, Temperature(expected));
        // 并且这组输入确实跌破冰点——否则这条测试无法区分"公式对"与
        // "四个加数恰好都是零"。
        assert!(actual.is_freezing(), "冬季雪夜的地表应当低于冰点");
    }

    #[test]
    fn 春夏两季无论昼夜天气地表都不结冰() {
        // 模块文档那条唯一的硬约束：`ll_sim::exposure` 的「平时完全不
        // 触发」红线在本表数值上的投影。春夏两季 × 一整天每小时 × 本体
        // 全部六种天气，穷举验证。
        // Arrange
        let (_ids, table) = base_weather_fixture();
        let coldest_offset = table
            .registered()
            .iter()
            .map(|index| table.temperature_offset(*index))
            .min()
            .expect("本体注册了六种天气");

        // Act & Assert
        for season_index in 0..2 {
            for hour in 0..24 {
                let tick = Tick(season_index * 30 * TICKS_PER_DAY + hour * TICKS_PER_HOUR);
                let worst = Weather {
                    kind: None,
                    light_scale: 1000,
                    sight_scale: 1000,
                    temperature_offset: coldest_offset,
                };
                let temperature = temperature_under(SURFACE_BASE, tick, worst);
                assert!(
                    !temperature.is_freezing(),
                    "第 {season_index} 季 {hour} 点最差天气下地表温度 {} 不该跌破冰点",
                    temperature.0
                );
            }
        }
    }

    #[test]
    fn 任何季节的正午地表都不结冰() {
        // 红线的第二半：「白天」不触发。取每季正午 × 最冷天气。
        // Arrange
        let (_ids, table) = base_weather_fixture();
        let coldest_offset = table
            .registered()
            .iter()
            .map(|index| table.temperature_offset(*index))
            .min()
            .expect("本体注册了六种天气");

        // Act & Assert
        for season_index in 0..4 {
            let tick = noon_of(season_index);
            let worst = Weather {
                kind: None,
                light_scale: 1000,
                sight_scale: 1000,
                temperature_offset: coldest_offset,
            };
            let temperature = temperature_under(SURFACE_BASE, tick, worst);
            assert!(
                !temperature.is_freezing(),
                "第 {season_index} 季正午最差天气下地表温度 {} 不该跌破冰点",
                temperature.0
            );
        }
    }

    #[test]
    fn 冬季午夜的地表跌破冰点() {
        // 反面：整套系统若一次都触发不了，就是又一处死代码。这条与上面
        // 两条一起，把「只在极端条件下」这句话的两侧都钉住。
        // Arrange
        let tick = midnight_of(3);

        // Act
        let temperature = temperature_under(SURFACE_BASE, tick, Weather::CLEAR);

        // Assert
        assert!(
            temperature.is_freezing(),
            "冬季午夜晴天的地表温度 {} 应当已经跌破冰点",
            temperature.0
        );
    }

    #[test]
    fn 同一组输入恒派生出同一个温度() {
        // 温度「零存档状态」得以成立的基石，与
        // `crate::weather` 的同名测试同一条道理。
        // Arrange
        let (_ids, table) = base_weather_fixture();
        let tick = Tick(97 * TICKS_PER_DAY + 3 * TICKS_PER_HOUR);
        let weather = Weather::derive(0xC0FF_EE12, tick, &table);

        // Act
        let first = temperature_under(SURFACE_BASE, tick, weather);
        let second = temperature_under(SURFACE_BASE, tick, weather);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 极端基准值不溢出而是饱和() {
        // base_temperature 是 mod 可以任意填的 i32；一个填了 i32::MAX
        // 的层属性不该让整局游戏 panic（见 Temperature::offset_by）。
        // Arrange：分别取偏移量全正（夏季正午）与全负（冬季午夜）的两
        // 个时刻，才能真的把两侧都推到饱和——同一个时刻只能推一侧。
        let hottest = noon_of(1);
        let coldest = midnight_of(3);

        // Act
        let hot = temperature_under(i32::MAX, hottest, Weather::CLEAR);
        let cold = temperature_under(i32::MIN, coldest, Weather::CLEAR);

        // Assert
        assert_eq!(hot, Temperature(i32::MAX));
        assert_eq!(cold, Temperature(i32::MIN));
    }

    #[test]
    fn 温度低于冰点时才判定为结冰() {
        // 比较方向：恰好 0℃ 不算结冰（`<` 而不是 `<=`）。
        // Act & Assert
        assert!(!Temperature(0).is_freezing());
        assert!(!Temperature(1).is_freezing());
        assert!(Temperature(-1).is_freezing());
        assert!(!Temperature::TEMPERATE_BASELINE.is_freezing());
    }
}
