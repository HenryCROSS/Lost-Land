//! 极端环境暴露——温度在结算侧的**唯一**消费者。
//!
//! # 这个模块存在的理由：温度必须有真实消费者
//!
//! `ll_world::temperature` 能算出「此刻这里多冷」，但一个算得出、没人
//! 读的标量与一个不存在的标量没有任何区别。本仓库已经因为「声明了却
//! 从没接线」的字段付出过反复的代价（`scripts/ci/check_field_consumers.py`
//! 这道阻断式门禁就是为此存在的）。本模块是温度那一路的落点：
//! [`exposure_strength_penalty`] 把「环境温度」与「身上的绝缘值」两个
//! 数合成一个力量惩罚，[`crate::resolve::derive_stats`] 把它作为第三条
//! 来源并进最终属性，`resolve_attack` 随后读到的攻击力就已经含了它。
//!
//! # 只在极端条件下产生后果——这是本模块最硬的一条纪律
//!
//! 项目所有者对保暖系统只提过一条红线：**不要做成一根一直在掉的温度
//! 条，逼着玩家每隔几步就烤火**。那种设计把「保暖」变成重复劳动，而
//! 不是决策。
//!
//! 因此本模块**不引入任何资源池、不引入任何随时间衰减的状态**——它甚至
//! 不往世界状态里写任何字段。它是一个纯函数：
//!
//! ```text
//! 体感温度 = 环境温度 + 绝缘值
//! 惩罚     = 体感温度 < 冰点 ? 台阶换算(体感温度) : 0
//! ```
//!
//! 体感温度在冰点以上时，本模块产出 0，`derive_stats` 的结果与温度这
//! 一路完全没接线时**逐位相同**。春夏、任何季节的白天、任何非露天空
//! 间，都落在这一侧——有测试逐条钉住（见本模块
//! `春夏两季与任何季节的白天都不产生惩罚`、以及
//! `ll_world::temperature` 里同名的一组）。
//!
//! 这与本项目对饱食度的既有裁定同源：饱食度当初被判定**不该**用资源
//! 池，因为资源池是「显式授予」而饱食度是「人人都有」。温度同理——
//! 它是环境属性，不是角色身上的一格容量。
//!
//! # 玩家怎么规避
//!
//! 三条路，全部是玩家能主动做出的决策，且互相独立：
//!
//! 1. **穿够衣服**——绝缘值来自装备，逐件求和（见
//!    [`ll_world::item::StatTarget::Insulation`]），两层比一层暖。
//! 2. **进洞穴 / 进屋**——非露天空间的温度恒等于自己的基准值，本体三
//!    种非露天空间的基准全部明确高于冰点（有测试钉住，见
//!    `ll_world::space_profile`）。
//! 3. **等天亮**——昼夜温差 12℃（`ll_world::temperature::DIURNAL_SWING`
//!    的两倍），冬季正午 8℃、午夜 -4℃，等待本身就是有效的解法。
//!
//! 不可规避的惩罚只会增加重复劳动，这三条是本模块设计时唯一的验收
//! 标准。
//!
//! # 为什么惩罚落在力量上
//!
//! [`crate::resolve::DerivedStats`] 当前唯一的生产消费者是
//! `resolve_attack`，而它**无条件**读取的派生量只有力量
//! （`attack_power_input`）与护甲。惩罚必须落在一个保证被读到的量上，
//! 否则就是第二处「声明了没人读」——把惩罚挂在敏捷上曾经是更贴切的
//! 叙述（冻僵的手指），但敏捷只经 `FormulaInputs` 的 `dex-mod` 操作数
//! 间接生效，而本体默认伤害公式并不引用那个操作数，惩罚会在本体内容
//! 下静默失效。
//!
//! 「冻得使不上劲」在叙述上同样成立，而且它落在一条已经存在、已经被
//! 读、已经有回归测试的路径上——这比一个更贴切但不生效的选择好。
//!
//! # ADR 0020：乙区，全程整数
//!
//! 温度与绝缘值都是十分之一摄氏度的整数，惩罚是整数点数，换算用整数
//! 除法。这条链路最终改变伤害数值（世界状态），是 ADR 0020 的乙区，
//! 一个浮点都不能有。

use ll_core::ident::ContentIndex;
use ll_world::space::Space;
use ll_world::space_profile::SpaceProfileTable;
use ll_world::state::WorldState;
use ll_world::temperature::Temperature;
use ll_world::weather::{Weather, WeatherTable};

/// 体感温度每低于冰点这么多（十分之一摄氏度），惩罚多一点。
///
/// 取 50（5℃）：本体冬夜地表是 -4℃（`-40`），冬夜下雪是 -12℃
/// （`-120`），两者分别落在第一档与第三档，惩罚是 1 与 3 点力量——在
/// 一个基准力量为 10 的角色身上分别是「察觉得到」与「明显打不动」，
/// 正是这套系统想表达的强度梯度。台阶更细（例如 10）会让惩罚在冬夜
/// 里随昼夜曲线连续抖动，玩家读不出规律；更粗（例如 200）则本体内容
/// 永远只能触发第一档，梯度形同虚设。
pub const EXPOSURE_PENALTY_STEP: i32 = 50;

/// 暴露惩罚的上限，点数。
///
/// 取 6：本体基准力量是 10（`ll_world::entity::BaseStats::BASELINE`），
/// 上限必须**明确小于**它，否则一个足够极端的 mod 天气能把力量压到
/// 零甚至负数，让攻击彻底失效——那不再是「冷得使不上劲」，而是一个
/// 不可规避的死亡判定，与本模块「只在极端条件下、且可规避」的纪律
/// 相悖。封顶还顺带挡住了 mod 用一个荒谬的 `base_temperature` 制造
/// 整数溢出的可能。
pub const EXPOSURE_MAX_PENALTY: i32 = 6;

/// 体感温度：环境温度加上身上装备提供的绝缘值。
///
/// `insulation` 是 [`crate::resolve::DerivedStats::insulation`]——逐件
/// 已装备物品的 [`ll_world::item::StatTarget::Insulation`] 求和的结果，
/// 与温度同一量纲（十分之一摄氏度）。「绝缘值 90 的斗篷」的含义因此
/// 就是字面意思：让你感觉比外面暖 9℃。
///
/// 用加法而不是「按比例向常温回归」这类更真实的传热模型：加法是玩家
/// 一眼能推算的（「-12℃，斗篷 +9℃，还差 3℃，再套一件」），而比例模型
/// 需要玩家心算一个他看不见的公式。这与 `ll_world::temperature` 三个
/// 偏移量选加法是同一条取舍。
///
/// 饱和加法，理由同 [`Temperature::offset_by`]：绝缘值来自 mod 可以
/// 任意填的 `StatBonus::amount`。
pub fn felt_temperature(ambient: Temperature, insulation: i32) -> Temperature {
    ambient.offset_by(insulation)
}

/// 某个体感温度下的力量惩罚点数，恒非负。
///
/// 体感温度在冰点及以上时恒为 **0**——这是模块文档「只在极端条件下」
/// 那条纪律的唯一落点，其余任何地方都不再重复这个判断。
///
/// 冰点以下按 [`EXPOSURE_PENALTY_STEP`] 分档，**第一档从刚跌破冰点就
/// 开始**（`1 + 超出量 / 台阶`，不是 `超出量 / 台阶`）：后者会让
/// `-0.1℃` 到 `-5.0℃` 这一整段算出 0 点惩罚，等于把真正的阈值悄悄挪
/// 到 -5℃，而 [`Temperature::FREEZING`] 的文档说的是冰点。阈值只能有
/// 一个，且必须是文档里写的那一个。
///
/// 结果按 [`EXPOSURE_MAX_PENALTY`] 封顶。
pub fn exposure_strength_penalty(felt: Temperature) -> i32 {
    if !felt.is_freezing() {
        return 0;
    }
    // 已知 felt.0 < 0，取相反数用饱和：felt.0 可能是 i32::MIN（一个把
    // base_temperature 填成 i32::MIN 的 mod 层属性），`-i32::MIN` 会溢出。
    let below_freezing = felt.0.saturating_neg();
    (1 + below_freezing / EXPOSURE_PENALTY_STEP).min(EXPOSURE_MAX_PENALTY)
}

/// 结算期查询「此刻这个空间多冷」所需的两张只读内容表——温度这一路
/// 的**依赖入口**。
///
/// # 为什么不是一个 trait（ADR 0021）
///
/// `resolve` 对技能/任务/物品/天赋等等一律走「`ll-sim` 定 trait、
/// `ll-mod` 实现」的依赖倒置，理由是那些表都定义在**下游**的 `ll-mod`
/// （`SkillTable`/`ItemTable`/…），`ll-sim` 不能反过来依赖它们。
///
/// 温度这一路不同：[`SpaceProfileTable`] 与 [`WeatherTable`] 都定义在
/// **`ll-world`**（见 `ll_world::weather` 模块文档「为什么天气表定义在
/// `ll-world`」一节），而 `ll-world` 就在 `ll-sim` 上游。这里直接借这
/// 两个具体类型不违反任何依赖方向。
///
/// ADR 0021 说抽象的理由是「有算法可共享」，不是对称：为这两张表另定
/// 一对 trait，只会得到一份没有第二个实现、也没有任何算法被共享的样板
/// ——它唯一的作用是让代码「看起来和别的目录一样」。不做。
///
/// # 空对象：[`AmbientSource::NONE`]
///
/// 不装载任何内容表的调用方（`resolve` 这个薄入口、自己合成世界的验收
/// demo、只测移动/开门的单元测试）用它。它让
/// [`Self::temperature_in`] 恒返回 [`Temperature::TEMPERATE_BASELINE`]，
/// 于是暴露惩罚恒为 0，整条温度链路与「压根没接」逐位等价。
#[derive(Debug, Clone, Copy)]
pub struct AmbientSource<'a> {
    profiles: Option<&'a SpaceProfileTable>,
    weathers: Option<&'a WeatherTable>,
}

impl AmbientSource<'static> {
    /// 「温度这一路没接」的空对象，见类型文档。
    pub const NONE: AmbientSource<'static> = AmbientSource {
        profiles: None,
        weathers: None,
    };
}

impl<'a> AmbientSource<'a> {
    /// 用两张真实内容表建立一个能真正算出温度的来源。
    ///
    /// 生产路径唯一的构造点是 `ll_game::content`（把
    /// `LoadedContent::space_table`/`weather_table` 借进
    /// [`crate::catalogs::ResolveCatalogs`]）。
    pub fn new(profiles: &'a SpaceProfileTable, weathers: &'a WeatherTable) -> AmbientSource<'a> {
        AmbientSource {
            profiles: Some(profiles),
            weathers: Some(weathers),
        }
    }

    /// 某个实体所在空间此刻的环境温度。
    ///
    /// # 天气在这里现派生，不由调用方传进来
    ///
    /// [`Weather::derive`] 的输入是 `(world.seed, world.clock)`，而
    /// `TurnEngine::perform` 会在每次结算**之前**把 `world.clock` 拨到
    /// 该实体计划行动的时刻。若让调用方在建目录束时先算好一个
    /// [`Weather`] 传进来，那个值会在 `advance_ai` 连续结算多个实体时
    /// 逐步过期——同一批推进里越靠后的实体看到的天气越旧。在这里现算
    /// 因此不是效率上的疏忽，是正确性上的唯一选择：本函数读的永远是
    /// 「这一刻」的世界时钟。
    ///
    /// 代价是一次加权遍历（本体只有六条），且只在真的持有内容表时才
    /// 发生——`AmbientSource::NONE` 走的是第一个 `else` 分支，一次
    /// 随机数都不掷。
    ///
    /// # 未注册的层属性索引退回中性温度
    ///
    /// 空间的 `profile` 索引来自 `Agent::current_space`，而
    /// `ContentIndex::default()` 是仓库里大量测试夹具的占位取值（见
    /// `ll_world::state::WorldState::surface_profile` 文档），它多半
    /// **不在**层属性表里。这里用 [`SpaceProfileTable::is_defined`] 把
    /// 关，查不到就退回 [`Temperature::TEMPERATE_BASELINE`]——与
    /// 「没有内容表」同一条降级，而不是让一个占位索引在调试构建下触发
    /// 表内部的 `debug_assert`。
    pub fn temperature_in(&self, world: &WorldState, space: &Space) -> Temperature {
        let (Some(profiles), Some(weathers)) = (self.profiles, self.weathers) else {
            return Temperature::TEMPERATE_BASELINE;
        };
        let index = space_profile_index(space);
        if !profiles.is_defined(index) {
            return Temperature::TEMPERATE_BASELINE;
        }
        let weather = Weather::derive(world.seed, world.clock, weathers);
        profiles.effective_temperature(index, world.clock, weather)
    }
}

/// 一个 [`Space`] 的层属性索引——两个变体的 `profile` 字段是同一个
/// 意思，抽成函数避免调用点各写一个 `match`。
fn space_profile_index(space: &Space) -> ContentIndex {
    match space {
        Space::Surface { profile, .. } | Space::Interior { profile, .. } => *profile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR, Tick};
    use ll_world::space_profile::{base_space_profile_fixture, effective_temperature};
    use ll_world::temperature::temperature_under;
    use ll_world::weather::{Weather, base_weather_fixture};

    /// 本体地表的温度基准。
    const SURFACE_BASE: i32 = 200;

    /// 本体六种天气里最冷的那一条的温度偏移。
    fn coldest_weather() -> Weather {
        let (_ids, table) = base_weather_fixture();
        let offset = table
            .registered()
            .iter()
            .map(|index| table.temperature_offset(*index))
            .min()
            .expect("本体注册了六种天气");
        Weather {
            kind: None,
            light_scale: 1000,
            sight_scale: 1000,
            temperature_offset: offset,
        }
    }

    #[test]
    fn 冰点及以上完全没有惩罚() {
        // 「平时完全不触发」在函数这一层的直接验证：阈值两侧各钉一点。
        // Act & Assert
        assert_eq!(exposure_strength_penalty(Temperature(0)), 0);
        assert_eq!(exposure_strength_penalty(Temperature(1)), 0);
        assert_eq!(
            exposure_strength_penalty(Temperature::TEMPERATE_BASELINE),
            0
        );
        assert_eq!(exposure_strength_penalty(Temperature(3000)), 0);
    }

    #[test]
    fn 刚跌破冰点就有第一档惩罚() {
        // 阈值只能有一个：文档说的是冰点，就不能因为整数除法把它悄悄
        // 挪到 -5℃。
        // Act & Assert
        assert_eq!(exposure_strength_penalty(Temperature(-1)), 1);
        assert_eq!(exposure_strength_penalty(Temperature(-49)), 1);
        assert_eq!(exposure_strength_penalty(Temperature(-50)), 2);
    }

    #[test]
    fn 惩罚随体感温度单调不减且封顶() {
        // Arrange：从冰点一路降到远低于封顶所需的温度。
        let samples: Vec<Temperature> = (0..80).map(|i| Temperature(-i * 10)).collect();

        // Act
        let penalties: Vec<i32> = samples
            .iter()
            .map(|felt| exposure_strength_penalty(*felt))
            .collect();

        // Assert
        for pair in penalties.windows(2) {
            assert!(pair[0] <= pair[1], "惩罚必须随温度下降单调不减");
        }
        assert_eq!(*penalties.last().expect("样本非空"), EXPOSURE_MAX_PENALTY);
        for penalty in penalties {
            assert!(penalty <= EXPOSURE_MAX_PENALTY);
        }
    }

    #[test]
    fn 极端温度不溢出且仍然封顶() {
        // base_temperature 是 mod 可以填 i32::MIN 的字段；`-i32::MIN`
        // 会溢出，这里验证饱和取反真的挡住了。
        // Act
        let penalty = exposure_strength_penalty(Temperature(i32::MIN));

        // Assert
        assert_eq!(penalty, EXPOSURE_MAX_PENALTY);
    }

    #[test]
    fn 上限明确小于基准力量() {
        // EXPOSURE_MAX_PENALTY 文档那条约束：封顶必须小于基准力量，
        // 否则攻击会被压到零，成为不可规避的死亡判定。
        //
        // 两个操作数都是常量，因此写成 `const` 块——编译期就会失败，
        // 比等到跑测试才发现更早，也是 clippy 的
        // `assertions_on_constants` 明确建议的写法。
        // Assert
        const {
            assert!(EXPOSURE_MAX_PENALTY < ll_world::entity::BaseStats::BASELINE.strength);
        }
    }

    #[test]
    fn 绝缘值把体感温度抬回冰点以上() {
        // 「穿够衣服」这条规避路径的直接验证。
        // Arrange：冬季午夜的本体地表。
        let ambient = temperature_under(SURFACE_BASE, Tick(90 * TICKS_PER_DAY), Weather::CLEAR);
        assert!(ambient.is_freezing(), "前置条件：这一刻本该冷到结冰");

        // Act
        let bare = exposure_strength_penalty(felt_temperature(ambient, 0));
        let one_layer = exposure_strength_penalty(felt_temperature(ambient, 50));
        let two_layers = exposure_strength_penalty(felt_temperature(ambient, 100));

        // Assert
        assert!(bare > 0, "不穿衣服的冬夜应当有惩罚");
        assert!(one_layer < bare, "一层衣服应当减轻惩罚");
        assert_eq!(two_layers, 0, "两层衣服应当把冬夜的惩罚完全抵消");
    }

    #[test]
    fn 春夏两季与任何季节的白天都不产生惩罚() {
        // 红线：平时（春夏、白天、露天）完全不触发。这里走的是完整的
        // 生产路径（真实的层属性表 + effective_temperature），不是只测
        // 本模块自己的算术。
        // Arrange
        let (ids, table) = base_space_profile_fixture();
        let surface = ll_world::space_profile::SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:surface").expect("合法"),
            ambient_light_floor: table.ambient_light_floor(ids.surface),
            exposed_to_sky: table.exposed_to_sky(ids.surface),
            base_temperature: table.base_temperature(ids.surface),
            diggable: table.diggable(ids.surface),
            buildable: table.buildable(ids.surface),
            reverb_tag: table.reverb_tag(ids.surface),
        };
        let worst = coldest_weather();

        // Act & Assert：春夏两季全天候。
        for season_index in 0..2 {
            for hour in 0..24 {
                let tick = Tick(season_index * 30 * TICKS_PER_DAY + hour * TICKS_PER_HOUR);
                let ambient = effective_temperature(&surface, tick, worst);
                assert_eq!(
                    exposure_strength_penalty(felt_temperature(ambient, 0)),
                    0,
                    "第 {season_index} 季 {hour} 点不该有任何暴露惩罚（体感 {}）",
                    ambient.0
                );
            }
        }

        // Act & Assert：四季的正午。
        for season_index in 0..4 {
            let tick = Tick(season_index * 30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);
            let ambient = effective_temperature(&surface, tick, worst);
            assert_eq!(
                exposure_strength_penalty(felt_temperature(ambient, 0)),
                0,
                "第 {season_index} 季正午不该有任何暴露惩罚（体感 {}）",
                ambient.0
            );
        }
    }

    #[test]
    fn 任何非露天空间在任何时刻都不产生惩罚() {
        // 「进洞穴 / 进屋」这条规避路径：本体三种非露天空间在一年里的
        // 任何一刻、任何天气下都不该有惩罚。
        // Arrange
        let (ids, table) = base_space_profile_fixture();
        let worst = coldest_weather();

        // Act & Assert
        for index in [ids.cave, ids.dungeon, ids.building_interior] {
            let profile = ll_world::space_profile::SpaceProfile {
                id: ll_core::ident::NamespacedId::parse("lostland:indoor").expect("合法"),
                ambient_light_floor: table.ambient_light_floor(index),
                exposed_to_sky: table.exposed_to_sky(index),
                base_temperature: table.base_temperature(index),
                diggable: table.diggable(index),
                buildable: table.buildable(index),
                reverb_tag: table.reverb_tag(index),
            };
            for season_index in 0..4 {
                for hour in [0, 6, 12, 18] {
                    let tick = Tick(season_index * 30 * TICKS_PER_DAY + hour * TICKS_PER_HOUR);
                    let ambient = effective_temperature(&profile, tick, worst);
                    assert_eq!(
                        exposure_strength_penalty(felt_temperature(ambient, 0)),
                        0,
                        "非露天空间在第 {season_index} 季 {hour} 点不该有暴露惩罚"
                    );
                }
            }
        }
    }

    #[test]
    fn 冬季雪夜的露天空间确实产生惩罚() {
        // 反面：整套系统若一次都触发不了就是死代码。这一条与上面两条
        // 一起，把「只在极端条件下」这句话的两侧都钉住。
        // Arrange
        let (ids, space_table) = base_space_profile_fixture();
        let (weather_ids, weather_table) = base_weather_fixture();
        let surface = ll_world::space_profile::SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:surface").expect("合法"),
            ambient_light_floor: space_table.ambient_light_floor(ids.surface),
            exposed_to_sky: space_table.exposed_to_sky(ids.surface),
            base_temperature: space_table.base_temperature(ids.surface),
            diggable: space_table.diggable(ids.surface),
            buildable: space_table.buildable(ids.surface),
            reverb_tag: space_table.reverb_tag(ids.surface),
        };
        let snow = Weather {
            kind: Some(weather_ids.snow),
            light_scale: weather_table.light_scale(weather_ids.snow),
            sight_scale: weather_table.sight_scale(weather_ids.snow),
            temperature_offset: weather_table.temperature_offset(weather_ids.snow),
        };
        let winter_midnight = Tick(90 * TICKS_PER_DAY);

        // Act
        let ambient = effective_temperature(&surface, winter_midnight, snow);
        let penalty = exposure_strength_penalty(felt_temperature(ambient, 0));

        // Assert
        assert!(
            penalty > 0,
            "冬季雪夜的露天空间（体感 {}）应当产生惩罚",
            ambient.0
        );
    }

    #[test]
    fn 中性温度这个空对象不产生任何惩罚() {
        // Temperature::TEMPERATE_BASELINE 是「温度这一路没接」的空对象，
        // 它绝不能退化成一个全局惩罚（见其文档）。
        // Act & Assert
        assert_eq!(
            exposure_strength_penalty(felt_temperature(Temperature::TEMPERATE_BASELINE, 0)),
            0
        );
        // 负绝缘值（诅咒装备："这件铠甲冰冷刺骨"）也不该把空对象推过
        // 冰点——中性温度与最大可能负绝缘之间必须留有余量吗？不必：
        // 负绝缘是内容设计的合法选择，这里只验证空对象**自己**是中性的。
        assert!(!Temperature::TEMPERATE_BASELINE.is_freezing());
    }
}
