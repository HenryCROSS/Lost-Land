//! 历史事件：击杀/死亡记录落地，作为 `HistoricalEvent` 信封的第一个
//! `kind` 变体（`knowledge/design/kill-and-death-events.md` 二节）。
//!
//! # 为什么落在 `ll-world`，不落在 `ll-sim`
//!
//! [`HistoricalEvent`] 是 [`crate::state::WorldState`] 的持久字段
//! （`history: Vec<HistoricalEvent>`）——它是存档数据的一部分，与
//! `Agent`/`Goal`/`Affiliation` 同一个依赖层级。`ll-sim` 依赖 `ll-world`
//! （规格 §5 依赖顺序），若把这些类型定义在 `ll-sim`，`ll-world` 的
//! `WorldState` 就没有地方引用它们——依赖方向不允许反过来。
//!
//! # 变体是怎么长出来的
//!
//! 本模块最初只有 `Kill` 一个变体，当时的取舍写在这里：设计文档的
//! 信封草图还列了 `SettlementFounded`/`War`/`DynastyChange`/`Rename`
//! 四个，但那四个都还没有任何字段定案、也没有任何生产代码会构造它们，
//! 提前占位是投机性设计（YAGNI），并且会在 `ll-content::remap`/
//! `WorldState::hash` 的穷尽匹配里凭空多出永远走不到的分支。**留下的
//! 机制是「等系统真正落地时再扩展这个枚举」**——`enum` 新增变体会让
//! 全部既有穷尽 `match` 在编译期报错，逼着那时的实现者显式处理。
//!
//! **世界历史生成批次正是那一次落地**：[`crate::chronicle`] 交付了一个
//! 真的会跑的历史推演器，`SettlementFounded`/`SettlementAbandoned` 两个
//! 变体因此有了确定的字段与真实的构造点（前者还真的改变了世界当前的
//! 地形，见 [`crate::settlement::stamp_settlement`]）。`War`/
//! `DynastyChange`/`Rename` 仍然**不是事件变体**——同一条 YAGNI 判据
//! 继续对它们成立。
//!
//! **战争的落点在别处**：据点覆灭原因批次给
//! [`SettlementAbandonedRecord`] 加了一个 [`SettlementDemise`] 字段，
//! 「被谁打没的」是 [`SettlementDemise::War`] 携带的攻方据点号，不是
//! 一个独立的 `War` 事件。这不是把战争塞进错误的位置：那一批次真正发生
//! 的事就是「某座据点在某个纪元没了」，攻方是它的**原因**。一场需要
//! 独立记载的战争（宣战、战役、和约）是另一套系统，那时候再加事件
//! 变体，与本模块「等系统真正落地时再扩展这个枚举」的既有纪律一致。
//!
//! **占领批次是那条纪律第三次生效**：项目所有者裁定「同种族的话更倾向
//! 于占领而不是毁灭」，于是战争第一次有了「据点不死」的结局。那个结局
//! 无论如何塞不进 [`SettlementDemise`]（那个枚举回答的是「怎么没的」），
//! 因此新增了 [`HistoricalEventKind::SettlementConquered`]。仍然**没有**
//! 加 `War`/`DynastyChange`/`Rename` 三个占位变体——同一条 YAGNI 判据
//! 对它们继续成立。
//!
//! [世界历史生成]: ../../../knowledge/design/world-history.md
//! [命名、改名与本地化]: ../../../knowledge/design/naming-and-localization.md

use ll_core::ident::{ContentIndex, WorldId};
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use serde::{Deserialize, Serialize};

use crate::entity::EntityId;
use crate::terrain::TerrainKind;

/// 记录一次击杀所需的全部原始信息——[`crate::state::WorldState::record_kill`]
/// 的入参，把七个独立参数收进一个结构体（避免
/// `clippy::too_many_arguments`），字段与
/// `ll_sim::effect::Effect::RecordHistoricalEvent` 携带的数据一一对应，
/// 是"resolve 已经读出的朴素数据"到"落盘调用"之间的直接映射，不含
/// 任何需要在这里做出的判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KillReport {
    /// 事件发生的世界时刻。
    pub at: Tick,
    /// 事件发生地点。
    pub location: TorusPos,
    /// 被击杀的实体。
    pub victim: EntityId,
    /// 击杀者，若有。
    pub killer: Option<EntityId>,
    /// 怎么杀的。
    pub cause: KillCause,
    /// 致命一击造成的伤害量。
    pub damage: i32,
    /// 致命一击结算后的剩余生命值。
    pub remaining_health: i32,
}

/// 一条历史事件的通用信封——本批次只有 [`HistoricalEventKind::Kill`]
/// 这一个变体，见模块文档「为什么只有 Kill 一个 kind 变体」一节。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoricalEvent {
    /// 永久标识，供跨引用与传说浏览查询——与势力/家族/聚落共用
    /// `WorldId` 空间（`identity-and-ids.md`「类型/实例分离」定案表已
    /// 把「历史事件」列入 `WorldId` 一侧）。由
    /// [`crate::state::WorldState::allocate_world_id`] 分配，永不复用。
    pub id: WorldId,
    /// 发生时刻。
    pub at: Tick,
    /// 发生地点。
    pub location: TorusPos,
    /// 具体事件种类与载荷。
    pub kind: HistoricalEventKind,
}

/// 历史事件的具体种类。
///
/// # 四个变体各自的来源
///
/// - [`Self::Kill`]：游戏内的战斗结算（`ll_sim::resolve`），经
///   [`crate::state::WorldState::record_kill`] 落进
///   `WorldState::history`，随存档走。
/// - [`Self::SettlementFounded`] / [`Self::SettlementAbandoned`] /
///   [`Self::SettlementConquered`]：**世界历史生成**
///   （[`crate::chronicle`]），产生于玩家进入之前，
///   **不进存档**——整份编年史是种子的纯函数，读档时重新派生（ADR
///   0009「默认派生，只存偏差」）。
///
/// # 「没了」与「易主」是两个变体，不是一个变体的两种载荷
///
/// 一场战争现在有两种结局。铲平走 [`Self::SettlementAbandoned`]
/// （载荷里的 [`SettlementDemise::War`] 说明是谁打的），占领走
/// [`Self::SettlementConquered`]。分成两个变体的理由见
/// [`SettlementConqueredRecord`] 文档：被占领的据点**没有没**，它还
/// 有人、还有门、还在长，把它塞进「遗弃」那条记录会让每一个既有消费
/// 方都读错。
///
/// 两类事件共用同一个信封，是因为它们是同一件事的两端：「这个世界上
/// 发生过什么」。传说浏览之类的消费方将来只需要遍历一份合并视图，不
/// 需要认识两套类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HistoricalEventKind {
    /// 一次击杀/死亡。
    Kill(KillRecord),
    /// 某地建立了一座据点。
    SettlementFounded(SettlementFoundedRecord),
    /// 某座据点被遗弃，留下废墟。
    SettlementAbandoned(SettlementAbandonedRecord),
    /// 某座据点**易主**——被打下来了，但没被铲平。
    SettlementConquered(SettlementConqueredRecord),
}

/// 一次据点建立——[`crate::chronicle`] 的推演在某个纪元把一处空地变成
/// 了定居点。
///
/// 事件信封上的 `location` 就是据点锚点，`at` 是该纪元的（负数）时刻，
/// 因此这里不重复记录位置与时间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementFoundedRecord {
    /// 这座据点的永久标识——与
    /// [`crate::settlement::SettlementSite::id`] 是同一个号。
    pub site: WorldId,
    /// 第几个纪元。
    pub epoch: u32,
    /// 建立时有多少人。
    pub initial_population: u32,
    /// 选址时该区块的最大连通可行走陆地面积（格）——「为什么这里能住
    /// 人」的那条判据的取值。
    pub land_area: u32,
}

/// 一次据点遗弃——此处此后只剩废墟。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementAbandonedRecord {
    /// 被遗弃的那座据点，与建立事件里的
    /// [`SettlementFoundedRecord::site`] 是同一个号。
    pub site: WorldId,
    /// 第几个纪元。
    pub epoch: u32,
    /// 它存在期间达到过的最高人口——废墟规模由它决定（见
    /// [`crate::settlement::SettlementSite::peak_population`]）。
    pub peak_population: u32,
    /// 从建立到遗弃经历了多少个纪元。
    pub epochs_inhabited: u32,
    /// **为什么**没了。见 [`SettlementDemise`]。
    pub cause: SettlementDemise,
}

/// 一次据点易主——打赢的一方没有铲平这座城，只是换了主子。
///
/// # 为什么这是一个**事件变体**，而不是 [`SettlementDemise`] 的第五种
///
/// [`SettlementDemise`] 的四个变体回答的是同一个问题：「这座据点是
/// 怎么**没**的」。占领的据点没有没——它还有人、还有门、还在长。把它
/// 塞进那个枚举，等于让 `SettlementAbandonedRecord` 携带一种「其实没
/// 被遗弃」的遗弃原因，那条穷尽 `match` 的每一个既有消费方都会立刻
/// 读错（`crate::state::write_historical_event` 会把一座活着的城混进
/// 「废墟」那一档，[`crate::chronicle`] 的 `final_sites` 会把它铺成
/// 废墟）。
///
/// 这正是 [`HistoricalEventKind`] 模块文档留的那句话第二次生效：
/// 「等系统真正落地时再扩展这个枚举」。第一次是世界历史生成把
/// `SettlementFounded`/`SettlementAbandoned` 加进来；本次是占领。
///
/// # 事件信封上的 `location` 是**被占领那座**据点的锚点
///
/// 与 [`SettlementAbandonedRecord`] 一致：事件说的是这座城身上发生了
/// 什么，攻方是它的原因、不是它的地点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementConqueredRecord {
    /// 被占领的那座据点。**它的 `WorldId` 不变**——同一座城换了主子，
    /// 不是旧城没了、新城建起来了。因此它与更早那条
    /// [`SettlementFoundedRecord::site`] 是同一个号，顺着号码能读出
    /// 「这座城建于第 2 纪元、第 6 纪元易主、至今仍有人住」。
    pub site: WorldId,
    /// 第几个纪元。
    pub epoch: u32,
    /// 打下它的那座据点的永久标识——「被谁占的」这条因果必须能顺着
    /// 号码查回去，判据与 [`SettlementDemise::War`] 携带的那个
    /// `aggressor` 逐字相同。
    pub conqueror: WorldId,
    /// 易主**之前**这座据点信的文化。
    ///
    /// # 为什么记文化，而不是记一个「势力号」
    ///
    /// 「归属」这件事在当前的世界模型里就是文化：
    /// [`crate::settlement::SettlementSite::culture`] 是**唯一**一个
    /// 同时决定「用什么建材盖房」（[`crate::settlement`] 的
    /// `wall_terrain`）、「住的是哪一族」
    /// （[`crate::culture::founder_race`]）、「跟谁不对付」
    /// （[`crate::chronicle`] 的 `hostility_between`）的属性。占领改
    /// 掉它，三处消费者立刻跟着变——这是「归属真的换了」能在地上被
    /// 看见的那条通路。
    ///
    /// 另造一个 `faction: WorldId` 字段会是本仓库已经数出三十一处的
    /// 那种「声明了但没接线」：今天没有任何一行游戏逻辑会读它。
    ///
    /// 不是 `Option`：占领**要求**攻守双方都有文化，没有文化这一层的
    /// 世界里没有东西可以易主，那样的战争恒以毁灭收场。这条不变量由
    /// [`crate::chronicle`] 的 `occupation_numerator` 守着。
    pub former_culture: ContentIndex,
    /// 易主**之后**这座据点信的文化——也就是攻方的那一份。
    ///
    /// 与 `former_culture` **可以相同**：两座同文化的城互相吞并时，
    /// 换的是主子而不是信仰。那种情形下这条记录仍然成立（这座城此后
    /// 属于 `conqueror`），只是在地上看不出区别。
    pub new_culture: ContentIndex,
    /// 易主之后这座据点还剩多少人——恒 `> 0`（一座人被打光的城是
    /// 毁灭，不是占领）。
    ///
    /// **谁读它**：端到端验收要拿它对上「那座城在地上仍然是活的」，
    /// 见 `crates/ll-game/tests/culture_and_war.rs`。
    pub survivors: u32,
}

/// 一座据点是怎么没的——项目所有者点名的那三种原因，加上原本唯一的
/// 那一种。
///
/// > 「历史计算的时候可能有的据点已经**覆灭**了，又有**新的据点出现**，
/// > 这些可能是因为**资源**，可能是**打仗**，也可能是**疾病**」
///
/// # 为什么是一个枚举，而不是几个布尔标记
///
/// 一座据点只会以一种方式覆灭——这是互斥的，不是可叠加的。枚举把这条
/// 互斥性写进类型，也让消费方（将来的传说浏览）的 `match` 由编译器
/// 保证穷尽：再加一种死法时，忘了处理的地方会直接编译不过。
///
/// # 为什么它进了序列化与哈希，尽管编年史不进存档
///
/// 因为它住在 [`HistoricalEventKind`] 这个**共用信封**里，而信封的另
/// 一个变体（[`HistoricalEventKind::Kill`]）确实随 `WorldState::history`
/// 进存档。类型层面的可序列化是信封的性质，不是本枚举自己要求的——
/// 与 `SettlementFoundedRecord` 从落地起就是这个情形，本枚举只是跟着
/// 同一条既有安排走。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementDemise {
    /// 人口自然凋零到零——迁出、歉收、老去，没有一件特定的事故。
    /// 这是本枚举出现之前**唯一**的一种遗弃原因。
    Depopulation,
    /// 赖以立足的可枯竭资源被采光了（[`crate::resource`] 的
    /// `exhaustible`）。矿脉一空，靠它吃饭的人就散了。
    ResourceExhausted {
        /// 采光的是哪一种资源——展示层由
        /// [`crate::resource::ResourceTable::display_name_key`] 取名字。
        resource: ContentIndex,
    },
    /// 被另一座据点攻灭。
    War {
        /// 攻方那座据点的永久标识——「谁灭的」这条因果必须能顺着号码
        /// 查回去，否则编年史里只剩一句「它被打没了」。
        aggressor: WorldId,
    },
    /// 瘟疫。
    Plague {
        /// 这一场疫病夺走了多少人。
        dead: u32,
    },
}

/// 一条击杀记录——"怎么杀的"必须能表达到武器/技能/环境这一级，是本
/// 批次的核心要求（见 `kill-and-death-events.md` 二节）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KillRecord {
    /// 谁杀的。环境/坠落/饥饿致死，或击杀者本身尚未"具名"（见
    /// `Agent::remembered_id` 文档）时为 `None`。
    pub killer: Option<WorldId>,
    /// 被杀的是谁——只有已具名（`remembered_id` 已赋值）的死亡才会
    /// 产出一条本记录，见 `crate::state::WorldState::remembered_id_of_or_assign`
    /// 文档「落地范围」一节。
    pub victim: WorldId,
    /// 用什么杀的。
    pub cause: KillCause,
    /// 致命一击的数值。
    pub killing_blow: KillingBlow,
    /// 死亡那一刻的状态标记。
    pub victim_state: VictimState,
}

/// "怎么杀的"——武器/技能/地形/坠落/饥饿，项目所有者点名要求的字段。
///
/// # 本批次实际会被构造的只有两个变体
///
/// `Melee`/`Skill` 是当前 `ll-sim::resolve` 真正会产出的两条致死路径
/// （近战攻击、技能伤害）。`Terrain`/`Fall`/`Starvation`/`Poison`/
/// `Environmental` 五个变体照设计文档定型，但环境伤害/坠落/饥饿/持续
/// 伤害致死这些系统本身尚未落地（分别属于地形交互、生存机制、
/// `buffs-and-triggers.md` 的持续伤害触发器，均不在本批次范围内）——
/// 这不是遗漏：枚举形状照设计文档冻结，供这些系统落地时直接产出对应
/// 变体，不需要再回来改 `KillCause` 本身；`remap`/`hash` 的穷尽匹配
/// 已经覆盖全部变体，不会因为暂时没有生产代码构造它们而漏测。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KillCause {
    /// 近战。
    Melee {
        /// 使用的武器——武器引用与穿透接线批次（P6 第六批）起，
        /// `ll-sim::resolve::resolve_attack` 真正查询攻击者主手
        /// （`EquipSlot::MAIN_HAND`）已装备的物品并填入这里；`None`
        /// 表示徒手（主手为空）。此前（装备系统落地之前）本字段恒传
        /// `None`，与「徒手」在类型上无法区分——这条区分现在是实的，
        /// 见 `ll_sim::resolve::resolve_attack` 文档「武器引用」一节。
        weapon: Option<ContentIndex>,
    },
    /// 技能击杀。
    Skill {
        /// 使用的技能，指向技能注册表。
        skill: ContentIndex,
    },
    /// 地形致死（熔岩、深渊……）。
    Terrain {
        /// 致死的具体地形种类。
        kind: TerrainKind,
    },
    /// 坠落致死。
    Fall,
    /// 饥饿致死。
    Starvation,
    /// 持续伤害类致死（`buffs-and-triggers.md` 的 `on_turn_start` 持续
    /// 伤害触发到生命值归零）。
    Poison,
    /// mod 扩展死因，走注册表而不是给 Rust 枚举反复加变体。
    Environmental(ContentIndex),
}

/// 致命一击的数值——只记"这一下怎么打死的"，不是整场战斗的流水账
/// （见设计文档「克制」一节：需要完整战斗过程时查询同一时间窗口内的
/// 其余历史事件即可重建，不在单条记录里预先塞进整场战斗）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillingBlow {
    /// 这一下造成的伤害量。
    pub damage: i32,
    /// 致命一击结算后的剩余生命值（通常 ≤ 0，允许记录过量伤害）。
    pub remaining_health: i32,
}

/// 死亡时的状态标记，定宽位标记，成本可以忽略不计。
///
/// # 为什么当前恒为 `false`
///
/// `poisoned` 依赖 `buffs-and-triggers.md` 的持续伤害触发器判断"死亡
/// 时是否处于中毒状态"，`surrounded` 依赖判断"结算时同一 tick 内是否
/// 有 2 个以上攻击者对其造成过伤害"——两套判断都需要本批次范围之外的
/// 数据（前者需要一个尚未落地的"中毒"标记类型，后者需要跨多个
/// `resolve` 调用聚合同一 tick 内的伤害来源，当前结算管线按单个
/// `Intent` 逐次调用 `resolve`，不做这种聚合）。字段本身照设计文档
/// 定型、真实参与序列化与哈希，只是取值恒为 `false`，直到对应的输入
/// 数据存在——与 `crate::state::WorldState::surface_profile` 等"字段
/// 已就位、真实取值等系统落地"的既有模式一致，不是死代码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VictimState {
    /// 死亡时是否处于中毒状态。
    pub poisoned: bool,
    /// 结算时同一 tick 内是否有 2 个以上攻击者对其造成过伤害——"被
    /// 围攻"的一个可判定近似,不追求叙事上的精确围攻定义。
    pub surrounded: bool,
}

impl VictimState {
    /// 本批次唯一会被构造的取值——见本类型文档「为什么当前恒为
    /// `false`」一节。
    pub const UNKNOWN: VictimState = VictimState {
        poisoned: false,
        surrounded: false,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record() -> KillRecord {
        let mut counter = 0u32;
        KillRecord {
            killer: Some(WorldId::next(&mut counter)),
            victim: WorldId::next(&mut counter),
            cause: KillCause::Melee {
                weapon: Some(ContentIndex::default()),
            },
            killing_blow: KillingBlow {
                damage: 42,
                remaining_health: -2,
            },
            victim_state: VictimState::UNKNOWN,
        }
    }

    #[test]
    fn historicalevent序列化往返后全部字段逐一相等() {
        // Arrange
        let mut counter = 5u32;
        let event = HistoricalEvent {
            id: WorldId::next(&mut counter),
            at: Tick(100),
            location: ll_core::torus::TorusSize::new(64, 64)
                .expect("64x64 是合法尺寸")
                .wrap(3, 4),
            kind: HistoricalEventKind::Kill(sample_record()),
        };

        // Act
        let encoded = serde_json::to_string(&event).expect("全部字段均已可派生序列化");
        let decoded: HistoricalEvent =
            serde_json::from_str(&encoded).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, event);
    }

    #[test]
    fn killcause不同变体互不相等() {
        // Arrange
        let melee = KillCause::Melee { weapon: None };
        let fall = KillCause::Fall;

        // Act & Assert
        assert_ne!(melee, fall);
    }

    #[test]
    fn victimstate的unknown常量两个标记均为假() {
        // Arrange & Act
        let state = VictimState::UNKNOWN;

        // Assert
        assert!(!state.poisoned && !state.surrounded);
    }
}
