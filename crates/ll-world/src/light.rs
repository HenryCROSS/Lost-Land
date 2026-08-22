//! 昼夜、四季与天气驱动的环境光照与视野半径。
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

use crate::weather::{WEATHER_SCALE_ONE, Weather};

/// 千分比表示的环境光照，`0..=1000`，1000 为最亮。
///
/// 用千分比整数而非百分比或浮点：千分比在日出日落的两小时渐变窗口内
/// 提供足够的插值精度（每刻度对应的光照变化不会被整数除法舍成 0），
/// 又全程是整数运算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LightLevel(pub i32);

/// 求某一世界时刻在**晴空基准**下的环境光照——不考虑天气。
///
/// 昼夜曲线（[`ll_core::light::day_curve`]）先给出未经季节缩放的基准
/// 值，再乘以 [`season_light_scale`] 得到最终光照。两步分开是为了让
/// 季节缩放能被单独测试与复用，而不必每次都重新构造一整天的 `Tick`。
///
/// 想要把天气也算进去，用 [`ambient_light_under`]——本函数等价于
/// `ambient_light_under(tick, Weather::CLEAR)`（有单元测试钉住这条
/// 等价关系，两者不会各自漂移）。**露天空间的生产路径不该直接调用本
/// 函数**：它不知道调用它的是地表还是地下城，也不知道外面在不在下雨，
/// 唯一正确的入口是 [`crate::space_profile::effective_ambient_light`]。
pub fn ambient_light(tick: Tick) -> LightLevel {
    ambient_light_under(tick, Weather::CLEAR)
}

/// 求某一世界时刻、某种天气下的环境光照——环境光管线的**唯一**真相源。
///
/// # 天气是第三个因子
///
/// ```text
/// ambient_light_under(tick, weather)
///     = day_curve(tick)                      // 昼夜，定义在 ll_core::light
///     × season_light_scale(tick.season())    // 四季，本模块
///     × weather.light_scale                  // 天气，crate::weather
/// ```
///
/// 三个因子逐个相乘、每一步都夹回 `0..=1000`，全程整数：中间结果走
/// `i64` 避免 `1000 × 1000` 这种量级溢出 `i32`。这里**不**为天气单独
/// 再写一条昼夜/季节判定——ADR 0010「白昼判定收敛为同一份真相源」的
/// 教训在这里同样成立，天气只贡献一个乘数，它不知道、也不需要知道现在
/// 是几点、哪一季。
///
/// # 天气只作用于露天空间
///
/// 本函数本身不判断露天与否——判断在
/// [`crate::space_profile::effective_weather`]，非露天空间在那里就已经
/// 被换成了 [`Weather::CLEAR`]，因此洞窟里下不下雨对环境光没有任何影响
/// （洞窟的环境光根本就不走本函数，恒等于 `ambient_light_floor`）。
pub fn ambient_light_under(tick: Tick, weather: Weather) -> LightLevel {
    let base = day_curve(tick);
    let season = season_light_scale(tick.season());
    // i64 中间结果避免 1000 * 1000 这种量级在极端输入下溢出 i32。
    let after_season = (i64::from(base) * i64::from(season)) / 1000;
    // 调用方即便构造了一个越界的 Weather（本 crate 的注册期校验拦得住
    // 表里的数据，拦不住手工构造的值），也不会产出负的或超比例的光照。
    let weather_scale = i64::from(weather.light_scale.clamp(0, WEATHER_SCALE_ONE));
    let scaled = (after_season * weather_scale) / 1000;
    LightLevel(scaled.clamp(0, 1000) as i32)
}

/// 季节对光照的缩放系数，千分比：夏 1000、春秋 900、冬 750。
///
/// 冬季明显低于其余三季，是为了让冬季在玩法上真正有压迫感——四季若
/// 只是换个色板，就没有存在的必要。
///
/// # 裁定：季节是纯函数派生，不是时间轴事件（W-03，P3 收尾裁定）
///
/// 规格 §7.2 原文把季节更替描述成「时间轴上的一个定时事件，其 `Effect`
/// 修改各城镇生产速率、地形通行性与野怪分布表」，`knowledge/audit/worklist.md`
/// 的 W-03 要求 P3 在建完时间轴调度器（`ll_sim::timeline`，定义在
/// 依赖本 crate 的下游 crate，本文件无法以文档链接形式指向它）后就此
/// 二选一裁定。这里选**纯函数派生**（维持本模块当前实现），不接入
/// `ll_sim::effect::Effect`：
///
/// 1. 规格原文要求的「城镇生产速率、地形通行性、野怪分布表」三个受季节
///    驱动的数据结构本身都还不存在——生产/城镇经济属 P8，野怪分布尚无
///    归属阶段。把季节做成 `Effect` 却没有任何真实状态可供它修改，只是
///    多一层没有内容的抽象。
/// 2. `ambient_light`/`sight_radius_at` 这类每帧都要现算的查询，若改走
///    `Effect` 驱动的缓存字段，就违反了本模块开篇「光照是纯函数派生，
///    绝不进世界状态」的纪律——缓存与时钟失同步的风险（「白天却一片
///    漆黑」）比多一次整数除法的成本大得多。
///
/// 这不是回避决策，是把「等到真的有城镇生产速率/地形通行性/野怪分布表
/// 这些状态时，再让驱动它们变化的那个批次决定要不要为它们各自接一个
/// `Effect`」这件事，留给真正引入这些系统的阶段——而不是现在为三个还
/// 不存在的数据结构预先决定一套接线方式。**规格 §7.2 原文与
/// `knowledge/handoff/p2-to-p3.md` 第四节仍需要相应更正**（把「尚无人
/// 认领」改为「已裁定为纯函数派生」），这两处都不在 `crates/**` 范围内，
/// 需要负责 `docs/**`/`knowledge/**` 的一方另行处理，本次只在代码侧
/// 把裁定落实并留下依据。
pub fn season_light_scale(season: Season) -> i32 {
    match season {
        Season::Summer => 1000,
        Season::Spring | Season::Autumn => 900,
        Season::Winter => 750,
    }
}

/// **未声明暗视**的生物在夜里保留的视野半径（格）——不是所有生物的
/// 绝对下限。
///
/// 原先的下限是 1，那只保证「不至于连脚下都看不见」，实测下来午夜开局
/// 是一片黑加正中央五个格子——项目所有者的要求是「让黑夜有个最低视野
/// 范围」。取 4 的理由：它足够看清相邻几格、能走能躲，又明显小于白天的
/// 基准半径（12），夜晚仍然是需要谨慎的时段而不只是换了个色调。
///
/// # 为什么不再叫 `MIN_SIGHT_RADIUS`
///
/// 暗视从「光照千分比下限」改成「夜间视野格数下限」之后，种族可以
/// 声明一个**低于**本常量的值（例如 `2`，表示这个种族夜里几乎全瞎），
/// 见 [`sight_radius_at`] 文档「为什么不是 `max(默认值, 声明值)`」
/// 一节。名字若仍叫「最小视野半径」，下一个读代码的人会理所当然地
/// 以为 4 是保底，然后被一个 2 格的种族打脸。它现在只是**未声明时的
/// 默认值**，名字如实反映这一点。
///
/// 这条是**玩法规则**，不是表现层调节：视野半径决定 FOV，FOV 决定探索
/// 记忆写入哪些格子，而探索记忆进 `WorldState::hash()`。画面亮度的下限
/// 是另一回事，见 `ll_game::layout` 的 `MIN_VISIBLE_TINT`——而且暗视
/// **只**影响本模块这一路（看多远），不影响那一路（看多清），见
/// [`sight_radius_at`] 文档「暗视只买视野格数，不买画面亮度」一节。
pub const DEFAULT_NIGHT_SIGHT_RADIUS: u32 = 4;

/// 把一个种族声明的暗视格数换算成它在这一格基准半径下真正的夜间下限。
///
/// 抽成私有函数而不是在两个调用点各写一遍：`sight_radius_at` 与
/// [`sight_radius_under_weather`] 必须给出**逐字节相同**的下限，否则
/// 恶劣天气会把暗视削掉一部分（那正是这次改动要修的缺陷之一）。两处
/// 各写一遍同一个三步表达式，是让它们将来分叉的最短路径。
fn night_sight_floor(base_radius: u32, darkvision_cells: u32) -> u32 {
    // 0 是「这个种族没有声明暗视」，不是「声明了 0 格」——列式存储对
    // 未定义槽位、以及 `RaceDarkvisionSource` 对查不到的索引，都返回 0
    // （ADR 0015「查不到就是查不到」），因此 0 必须落回默认值而不是
    // 被当成一个真实的极端声明。
    let declared = if darkvision_cells == 0 {
        DEFAULT_NIGHT_SIGHT_RADIUS
    } else {
        darkvision_cells
    };
    // 夜间下限不得反过来把「基准视野本就很小」的角色**放大**，故先与
    // `base_radius` 取小；但「永不为零」这条绝对底线始终成立——基准为
    // 零时仍返回 1，与本模块原有契约一致。
    declared.min(base_radius).max(1)
}

/// 按光照缩放基准视野半径，夜间下限由 `darkvision_cells` 决定。
///
/// 下限存在的理由与午夜光照取 100 而非 0 相同：视野缩到零会让玩家连
/// 自己脚下都看不见，那是卡住而不是难度。`light` 的分量在调用前会被
/// 夹到 `0..=1000`，即便调用方传入了越界值（例如某个未来的负面效果
/// 直接构造了 `LightLevel(-1)`），也不会产出负的或超比例的半径。
///
/// # `darkvision_cells` 是**视野格数**，不是光照下限
///
/// 这个参数此前的形态是「光照千分比下限」（`RaceDef::darkvision_floor`，
/// 经一个 `max(实际光照, 下限)` 折进 `light`）。那个形态在本作的量纲
/// 下**永远不可能生效**：本体矮人声明的是 4，而午夜环境光是 100
/// （`ll_core::light::MIDNIGHT_LIGHT`），最暗的冬夜下雨也还有约 52
/// ——`max(52, 4)` 恒等于 52，矮人的暗视等于不存在。就算把那个数字
/// 调大，下游还有第二个下限（本模块自己的 4 格）会把它吃掉：基准 12
/// 格时光照 300 只算出 3 格，仍旧被 4 格的下限抬回 4，暗视值从 100
/// 涨到 300 最终一格没变。两个下限串在一起，后面那个把前面那个吃掉。
/// 改成直接声明格数，是让这个数字与它想表达的东西（「夜里能看多远」）
/// 之间不再隔着一层会把它整个吸收掉的换算。
///
/// # 为什么不是 `max(默认值, 声明值)`
///
/// 用「为 0 取默认、非 0 直接用」而不是 `max`：`max` 会**禁止**「夜视
/// 比常人差」这一整类设定——任何低于 4 的声明都会被默默抬回 4，写它的
/// 人得不到任何提示。按当前写法，一个种族声明 `2` 就真的是夜里只看得见
/// 两格，表达力严格更强，而「没声明」这个真实存在的状态由 0 承担。
///
/// # 暗视只买视野格数，不买画面亮度
///
/// 本函数是暗视唯一的落点：它改变的只有「看多远」。「看多清」那一路
/// （`ll_game::layout::effective_tint`）读的是环境光本身，与暗视无关
/// ——夜视好的种族在黑暗里看得**更远**，不是让整个世界对它变亮。
pub fn sight_radius_at(base_radius: u32, light: LightLevel, darkvision_cells: u32) -> u32 {
    let clamped_light = light.0.clamp(0, 1000) as u64;
    let scaled = (u64::from(base_radius) * clamped_light) / 1000;
    let night_floor = night_sight_floor(base_radius, darkvision_cells);
    (scaled as u32).max(night_floor)
}

/// 在 [`sight_radius_at`] 之上再叠加天气的视野缩减。
///
/// # 为什么是第二个乘数，不是折进光照
///
/// 天气对「看多远」的影响不只来自「有多暗」：雾几乎不遮光，却让人只
/// 看得见几步之内；阴天明显更暗，能看多远却几乎不变。若只有
/// [`crate::weather::WeatherDef::light_scale`] 一个旋钮，这两种天气在
/// 玩法上必然坍缩成同一种东西的强弱版本。因此
/// [`crate::weather::WeatherDef::sight_scale`] 是**独立的第二个乘数**，
/// 接在光照换算完成**之后**——顺序不能反过来：折进光照会让雾同时把
/// 画面也压黑（`effective_tint` 读的是光照），那不是雾。
///
/// # 夜间下限在这里**第二次**应用，两次都必须认识暗视
///
/// 下限在本函数里被应用了两次：一次在 [`sight_radius_at`] 内部，天气
/// 乘数算完之后又来一次。这是刻意的——雾雪吃不掉「人在暗处也能摸到
/// 周围」这条底线。正因为是两次，两次都必须用同一个
/// `darkvision_cells`：只在 [`sight_radius_at`] 那一处认暗视，恶劣天气
/// 就会把矮人从 7 格削回默认的 4 格，暗视在最需要它的场合反而失效。
/// 两处共用 `night_sight_floor` 这一个私有函数，不是各写一遍。
pub fn sight_radius_under_weather(
    base_radius: u32,
    light: LightLevel,
    weather: Weather,
    darkvision_cells: u32,
) -> u32 {
    let radius = sight_radius_at(base_radius, light, darkvision_cells);
    let scale = u64::from(weather.sight_scale.clamp(0, WEATHER_SCALE_ONE) as u32);
    let scaled = (u64::from(radius) * scale) / 1000;
    let night_floor = night_sight_floor(base_radius, darkvision_cells);
    (scaled as u32).max(night_floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::base_weather_fixture;
    use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR};

    /// 与 `ll_game::layout::BASE_SIGHT_RADIUS` 同值——本 crate 不依赖
    /// `ll-game`（依赖方向不允许），组合断言需要一个基准半径才能问出
    /// 「天气会不会把视野压到不可玩」，这里复制那个取值并在此说明。
    /// 两者若哪天分叉，本节断言的结论仍然对本 crate 成立，只是不再直接
    /// 代表实机画面。
    const PLAYER_BASE_SIGHT_RADIUS: u32 = 12;

    /// 与 `ll_game::layout::MIN_VISIBLE_TINT` 同值，理由同上——那是纯
    /// 表现层常量（ADR 0020 甲区），定义在 `ll-game`。
    const PLAYER_MIN_VISIBLE_TINT: f32 = 0.4;

    /// 「这个调用方不知道谁在看，也不关心暗视」的显式取值——0 表示
    /// **未声明**，`night_sight_floor` 会把它落回
    /// [`DEFAULT_NIGHT_SIGHT_RADIUS`]，与本函数长出暗视参数之前的行为
    /// 逐格相同。写成具名常量而不是散落的字面量 `0`，是为了让「这里
    /// 传 0 是因为没有观察者」与「某个种族真的声明了 0」在读代码时不
    /// 会混淆（后者不可能出现——0 恒被解读成未声明）。
    const NO_DARKVISION: u32 = 0;

    fn all_seasons() -> [Season; 4] {
        [
            Season::Spring,
            Season::Summer,
            Season::Autumn,
            Season::Winter,
        ]
    }

    /// 某一季正午的世界时刻。四季分别落在每年第 0/30/60/90 天。
    fn noon_of(season: Season) -> Tick {
        let day = match season {
            Season::Spring => 0,
            Season::Summer => 30,
            Season::Autumn => 60,
            Season::Winter => 90,
        };
        Tick(day * TICKS_PER_DAY + 12 * TICKS_PER_HOUR)
    }

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
    fn 基准半径为零时视野仍不为零() {
        // 基准半径为零时，若不设下限，缩放结果恒为零。
        // Arrange
        let base_radius = 0;
        let full_light = LightLevel(1000);

        // Act
        let radius = sight_radius_at(base_radius, full_light, NO_DARKVISION);

        // Assert
        assert_eq!(radius, 1);
    }

    #[test]
    fn 光照为零时视野仍保有夜间下限() {
        // Arrange
        let base_radius = 10;
        let no_light = LightLevel(0);

        // Act
        let radius = sight_radius_at(base_radius, no_light, NO_DARKVISION);

        // Assert
        assert_eq!(radius, DEFAULT_NIGHT_SIGHT_RADIUS);
    }

    #[test]
    fn 晴空基准下的天气版光照与不带天气的版本逐位相同() {
        // 两个函数不能各自漂移：ambient_light 现在只是 CLEAR 的特例，
        // 这条断言把「特例」这件事钉住，而不是靠两处各写一遍公式。
        // Arrange
        let samples = [
            Tick(0),
            Tick(6 * TICKS_PER_HOUR),
            Tick(12 * TICKS_PER_HOUR),
            Tick(90 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR),
        ];

        // Act & Assert
        for tick in samples {
            assert_eq!(
                ambient_light(tick),
                ambient_light_under(tick, Weather::CLEAR)
            );
        }
    }

    #[test]
    fn 天气是环境光的第三个乘数() {
        // 半亮的天气应当把同一时刻的光照压到大约一半——「第三个因子」
        // 这句话的可验证形式。
        // Arrange
        let noon = noon_of(Season::Summer);
        let half = Weather {
            kind: None,
            light_scale: 500,
            sight_scale: 1000,
            temperature_offset: 0,
        };

        // Act
        let clear = ambient_light_under(noon, Weather::CLEAR).0;
        let dimmed = ambient_light_under(noon, half).0;

        // Assert：整数除法可能差 1，用相等而不是范围判断反而更脆。
        assert_eq!(dimmed, clear / 2);
    }

    #[test]
    fn 天气乘数越界时被夹住而不是算出越界光照() {
        // 表里的数据有注册期校验兜着，手工构造的 Weather 没有——这条
        // 保证越界输入不会产出负的或超比例的光照。
        // Arrange
        let noon = noon_of(Season::Summer);
        let absurd_low = Weather {
            kind: None,
            light_scale: -5000,
            sight_scale: 1000,
            temperature_offset: 0,
        };
        let absurd_high = Weather {
            kind: None,
            light_scale: 9999,
            sight_scale: 1000,
            temperature_offset: 0,
        };

        // Act & Assert
        assert_eq!(ambient_light_under(noon, absurd_low).0, 0);
        assert_eq!(
            ambient_light_under(noon, absurd_high),
            ambient_light(noon),
            "超过 1000 的乘数被夹到 1000，等价于晴空"
        );
    }

    #[test]
    fn 天气的视野乘数独立于光照生效() {
        // 雾：光照几乎不变，视野明显缩短——两个旋钮必须能分别拨动，
        // 否则 sight_scale 就是多余的。
        // Arrange
        let noon = noon_of(Season::Summer);
        let foggy = Weather {
            kind: None,
            light_scale: 1000,
            sight_scale: 500,
            temperature_offset: 0,
        };
        let light = ambient_light_under(noon, foggy);

        // Act
        let clear_radius = sight_radius_under_weather(
            PLAYER_BASE_SIGHT_RADIUS,
            light,
            Weather::CLEAR,
            NO_DARKVISION,
        );
        let foggy_radius =
            sight_radius_under_weather(PLAYER_BASE_SIGHT_RADIUS, light, foggy, NO_DARKVISION);

        // Assert
        assert_eq!(light, ambient_light(noon), "雾不改变光照");
        assert!(foggy_radius < clear_radius, "雾必须真的缩短视野");
    }

    #[test]
    fn 任何季节任何本体天气下视野都不低于夜间下限() {
        // 组合断言：天气叠加四季叠加昼夜之后，视野半径不会被压到不可玩。
        // 这条是新增天气之后最容易出问题的地方——两个乘数相乘，很容易
        // 在某个组合上把玩家压成瞎子。
        // Arrange
        let (_ids, table) = base_weather_fixture();

        // Act & Assert
        for index in table.registered() {
            let weather = Weather {
                kind: Some(*index),
                light_scale: table.light_scale(*index),
                sight_scale: table.sight_scale(*index),
                temperature_offset: 0,
            };
            for season in all_seasons() {
                for hour in 0..24i64 {
                    let tick =
                        Tick(noon_of(season).0 - 12 * TICKS_PER_HOUR + hour * TICKS_PER_HOUR);
                    let light = ambient_light_under(tick, weather);
                    let radius = sight_radius_under_weather(
                        PLAYER_BASE_SIGHT_RADIUS,
                        light,
                        weather,
                        NO_DARKVISION,
                    );
                    assert!(
                        radius >= DEFAULT_NIGHT_SIGHT_RADIUS,
                        "季节 {season:?} 第 {hour} 小时的视野半径 {radius} 跌破夜间下限"
                    );
                }
            }
        }
    }

    #[test]
    fn 夏季正午在任何本体天气下都明显好于夜间下限() {
        // 上一条只保证「不至于瞎」；这一条保证「一天里最好的时段不会
        // 被天气压成和午夜一样」——否则天气就成了单纯的惩罚开关。
        // Arrange
        let (_ids, table) = base_weather_fixture();
        let noon = noon_of(Season::Summer);

        // Act & Assert
        for index in table.registered() {
            let weather = Weather {
                kind: Some(*index),
                light_scale: table.light_scale(*index),
                sight_scale: table.sight_scale(*index),
                temperature_offset: 0,
            };
            let light = ambient_light_under(noon, weather);
            let radius =
                sight_radius_under_weather(PLAYER_BASE_SIGHT_RADIUS, light, weather, NO_DARKVISION);
            assert!(
                radius > DEFAULT_NIGHT_SIGHT_RADIUS,
                "夏季正午的视野半径 {radius} 与午夜一样只剩下限"
            );
        }
    }

    #[test]
    fn 任何季节正午在任何本体天气下画面亮度都高于表现层下限() {
        // 甲区（纯表现）那一侧的同一条组合检查：白天不该靠
        // MIN_VISIBLE_TINT 兜底才看得见——那说明天气把画面压过头了。
        // 这里直接复算 ll_game::layout::effective_tint 的换算式（本 crate
        // 不能依赖 ll-game），只做「未经下限钳制的原始亮度」这一步。
        // Arrange
        let (_ids, table) = base_weather_fixture();

        // Act & Assert
        for index in table.registered() {
            let weather = Weather {
                kind: Some(*index),
                light_scale: table.light_scale(*index),
                sight_scale: table.sight_scale(*index),
                temperature_offset: 0,
            };
            for season in all_seasons() {
                let light = ambient_light_under(noon_of(season), weather);
                let raw_tint = light.0.clamp(0, 1000) as f32 / 1000.0;
                assert!(
                    raw_tint > PLAYER_MIN_VISIBLE_TINT,
                    "季节 {season:?} 正午的原始亮度 {raw_tint} 未高于表现层下限"
                );
            }
        }
    }

    #[test]
    fn 声明的暗视格数在夜里真的换来更远的视野() {
        // 这条是本次改动的**意义本身**：旧公式（暗视是光照千分比下限）
        // 下矮人与人类的夜间视野完全相同——两者都撞在 4 格这个下游
        // 下限上，`max(100, 4)` 连光照都没抬起来。
        // Arrange：午夜光照（千分之一百），基准半径 12。
        let midnight_light = LightLevel(100);

        // Act
        let dwarf = sight_radius_at(PLAYER_BASE_SIGHT_RADIUS, midnight_light, 7);
        let human = sight_radius_at(PLAYER_BASE_SIGHT_RADIUS, midnight_light, NO_DARKVISION);

        // Assert
        assert_eq!(dwarf, 7);
        assert_eq!(human, DEFAULT_NIGHT_SIGHT_RADIUS);
        assert!(dwarf > human);
    }

    #[test]
    fn 声明低于默认值的暗视格数不被抬回默认值() {
        // 「不能写成 max(默认值, 声明值)」这条语义的可执行形式：一个
        // 夜里几乎全瞎的种族（声明 2 格）必须真的只剩 2 格，而不是被
        // 默默抬到 4 格——否则「夜视比常人差」这一整类设定根本无法表达。
        // Arrange
        let midnight_light = LightLevel(100);

        // Act
        let nearly_blind = sight_radius_at(PLAYER_BASE_SIGHT_RADIUS, midnight_light, 2);

        // Assert
        assert_eq!(nearly_blind, 2);
        assert!(nearly_blind < DEFAULT_NIGHT_SIGHT_RADIUS);
    }

    #[test]
    fn 未声明暗视与显式传零的行为完全一致() {
        // 0 是「没声明」，不是「声明了 0 格」——列式存储的未定义槽位与
        // `RaceDarkvisionSource` 查不到的索引都返回 0，两者必须落回默认。
        // Arrange & Act
        let radius = sight_radius_at(PLAYER_BASE_SIGHT_RADIUS, LightLevel(0), 0);

        // Assert
        assert_eq!(radius, DEFAULT_NIGHT_SIGHT_RADIUS);
    }

    #[test]
    fn 白天暗视不起作用() {
        // 正午满光照下 12 格远高于任何一个种族声明的暗视格数——暗视是
        // 暗处的能力，不是无条件加成。
        // Arrange
        let noon_light = ambient_light(noon_of(Season::Summer));

        // Act
        let dwarf = sight_radius_at(PLAYER_BASE_SIGHT_RADIUS, noon_light, 7);
        let human = sight_radius_at(PLAYER_BASE_SIGHT_RADIUS, noon_light, NO_DARKVISION);

        // Assert
        assert_eq!(dwarf, human);
        assert_eq!(dwarf, PLAYER_BASE_SIGHT_RADIUS);
    }

    #[test]
    fn 恶劣天气不把暗视削回默认值() {
        // 守住「两处调用点都换成了暗视版本」：夜间下限在
        // `sight_radius_under_weather` 里被应用两次，只改前一处的话，
        // 雾/雪的 sight_scale 会把矮人从 7 格削回 4 格——暗视在最需要
        // 它的场合失效。
        // Arrange：本体全部天气，四季全时段。
        let (_ids, table) = base_weather_fixture();
        const DWARF_CELLS: u32 = 7;

        // Act & Assert
        for index in table.registered() {
            let weather = Weather {
                kind: Some(*index),
                light_scale: table.light_scale(*index),
                sight_scale: table.sight_scale(*index),
                temperature_offset: 0,
            };
            for season in all_seasons() {
                for hour in 0..24i64 {
                    let tick =
                        Tick(noon_of(season).0 - 12 * TICKS_PER_HOUR + hour * TICKS_PER_HOUR);
                    let light = ambient_light_under(tick, weather);
                    let dwarf = sight_radius_under_weather(
                        PLAYER_BASE_SIGHT_RADIUS,
                        light,
                        weather,
                        DWARF_CELLS,
                    );
                    assert!(
                        dwarf >= DWARF_CELLS,
                        "季节 {season:?} 第 {hour} 小时、天气乘数 {} 下的暗视视野 {dwarf} 跌破声明的 {DWARF_CELLS} 格",
                        weather.sight_scale
                    );
                }
            }
        }
    }

    #[test]
    fn 声明低于默认值的种族在恶劣天气下也不被抬回默认值() {
        // 与上一条对称的另一半：天气那一处的下限同样必须认「声明值」，
        // 否则一个 2 格的种族会在雾里反而**看得更远**（被抬到 4 格）。
        // Arrange：本体雾天。
        let (ids, table) = base_weather_fixture();
        let fog = Weather {
            kind: Some(ids.fog),
            light_scale: table.light_scale(ids.fog),
            sight_scale: table.sight_scale(ids.fog),
            temperature_offset: 0,
        };
        let midnight = Tick(30 * TICKS_PER_DAY);
        let light = ambient_light_under(midnight, fog);

        // Act
        let nearly_blind = sight_radius_under_weather(PLAYER_BASE_SIGHT_RADIUS, light, fog, 2);

        // Assert
        assert_eq!(nearly_blind, 2);
    }

    #[test]
    fn 暗视不会把基准视野极差的生物放大() {
        // `.min(base_radius)` 那一句在新语义下的理由不变：一个基准半径
        // 只有 1 格的生物，即使声明了 7 格暗视，也不该在夜里比白天
        // 看得更远。
        // Arrange & Act
        let radius = sight_radius_at(1, LightLevel(0), 7);

        // Assert
        assert_eq!(radius, 1);
    }
}
