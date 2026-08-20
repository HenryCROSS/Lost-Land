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
//! # 为什么只有 `Kill` 一个 `kind` 变体
//!
//! 设计文档给出的信封草图还列了 `SettlementFounded`/`War`/
//! `DynastyChange`/`Rename` 四个变体，但那四个都属于[世界历史生成]
//! （P7，仍是纯设计）与[命名、改名与本地化]的范围，此刻没有任何字段
//! 定案、也没有任何生产代码会构造它们——本批次只落地"击杀与死亡记录"
//! 这一件事（P3 已经落地的战斗结算需要它），提前给四个连字段都不存在
//! 的变体占位是纯粹的投机性设计（YAGNI），也会在 `ll-content::remap`/
//! `WorldState::hash` 的穷尽匹配里凭空多出四条永远走不到的分支。等
//! 那四个系统真正定案字段时，再各自扩展这个枚举——`HistoricalEventKind`
//! 是一个 `enum`，新增变体本就会让所有既有的穷尽 `match`（`remap`/
//! `hash`）在编译期报错，逼着那时的实现者显式处理，这正是本仓库一贯
//! 依赖的机制，不需要提前预留。
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

/// 历史事件的具体种类——本批次只交付 [`Self::Kill`]，见模块文档。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HistoricalEventKind {
    /// 一次击杀/死亡。
    Kill(KillRecord),
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
        /// 使用的武器；`None` 表示徒手，或当前会话还没有可归因的具体
        /// 武器（装备系统未落地前，`ll-sim::resolve` 恒传 `None`，见
        /// `crate::terrain` 同一类"系统未落地前先留字段"的既有模式）。
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
