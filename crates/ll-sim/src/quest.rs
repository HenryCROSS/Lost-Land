//! `resolve` 侧需要的任务完成判定接口 + 任务进度基础操作（P5-B 接线
//! 批次）。
//!
//! # 为什么任务进度的基础操作（曾经）在 `ll-mod`，现在搬到这里
//!
//! `quest_progress_key`/`mark_quest_completed`/`is_quest_completed` 三个
//! 函数最初由 P5-B 任务 7 写在 `crates/ll-mod/src/quest.rs`——当时只需要
//! 「产出一条脚本状态写入」与「查询是否已完成」这两个纯读写操作，不需要
//! 知道任何 `QuestTable`/`QuestDef` 的内容，放在 `ll-mod` 只是因为
//! `QuestTable` 恰好也在那个文件。
//!
//! 接线批次要把「击杀是否推进了任务进度」接进真实的 `resolve` 结算
//! （`Intent::Attack` 产出 `Effect::Kill` 时，若被击杀者的种类匹配某个
//! `QuestCondition::KillCount`，应当顺带产出击杀计数与可能的任务完成
//! 写入）——这段逻辑天然属于 `resolve`，但依赖方向不允许 `ll-sim`
//! 反过来依赖 `ll-mod`（`QuestTable`/`QuestCondition` 定义在那里，见
//! `crates/ll-mod/src/quest.rs` 模块文档「本体即 Mod」一节的裁定）。
//! 与技能系统同一个架构缺口（见 [`crate::skill`] 模块文档「本任务选择
//! 的解法」一节），同一个解法：**依赖倒置**——[`QuestCatalog`] trait
//! 定义在本模块，`ll-mod::quest::RegisteredQuests` 实现它。
//!
//! 三个基础操作本身不依赖任何 `ll-mod` 类型（只用到
//! `ll_world::entity::{Agent, EntityId}` 与
//! `ll_world::mod_state::{ModStateValue, ModStateWrite}`，两者都在 `ll-sim` 的下游依赖 `ll-world` 里），因此
//! 随判定逻辑一起下沉到这里是干净的——不是"为了让代码能编译才硬凑"，
//! 是它们原本就没有理由必须待在 `ll-mod`。`ll-mod::quest` 现在
//! `pub use` 重新导出这三个函数，保持既有调用点（包括它自己的测试）
//! 不需要改名。
//!
//! # 击杀计数：为什么用 `Agent::race` 作为 `target_kind`，如实记录简化
//!
//! `QuestCondition::KillCount.target_kind` 从任务 6 起就是一个裸
//! `ContentIndex`，`crates/ll-mod/src/quest.rs` 模块文档「跨表引用」
//! 一节已经记录：当前代码库没有任何"敌人/生物类型"专用注册表。
//! [`kill_progress_effects`] 选择直接复用 `Agent::race`（同样是一个
//! 指向注册表的 `ContentIndex`，语义上标注"这是什么种类的生物"）作为
//! 击杀匹配的依据——不新增字段、不新增注册表，是当前代码库能做到的
//! 最小接线。**这是一处已知的语义借用，不是精确解**：`race` 原本设计
//! 给玩家角色种族，用它兼职"怪物类型"会让"击杀 3 个哥布林"与"击杀
//! 3 个哥布林种族的玩家角色"共用同一个索引——一张真正的"敌人类型"
//! 注册表迟早需要落地来消除这个借用，本批次的验收 demo 只在"怪物"
//! 用途上使用 `race`，不会撞见这个混淆，但记入验收报告，不假装这是
//! 完整解。
//!
//! # `Intent::Attack` 与 `Intent::UseSkill` 都会触发这条接线（缺口
//! 修补批次已解除的范围边界）
//!
//! 本节曾经记录一条范围边界：[`crate::resolve::resolve_with_skills_and_quests`]
//! 只在 `Intent::Attack` 产出 `Effect::Kill` 时才调用
//! [`kill_progress_effects`]，因为 `resolve_use_skill` 的
//! `SkillEffect::DealDamage` 分支当时不判断"这一下是否致死"，永远不
//! 产出 `Effect::Kill`——把 `Intent::UseSkill` 也接进这条判定在当时没
//! 有意义（接了也触发不到）。缺口修补批次（P5-C）先补上了
//! `resolve_use_skill` 缺失的致死判定（`crates/ll-sim/src/resolve.rs`
//! 的 `resolve_use_skill` 文档「与 `resolve_attack` 共享同一条致死判定
//! 纪律」一节），这条边界随之解除的前提就成立了——
//! `resolve_with_skills_and_quests` 现在对 `Intent::Attack` 与
//! `Intent::UseSkill` 都会调用 [`kill_progress_effects`]（准确地说是
//! 调用 `crate::resolve::append_quest_kill_progress`，它再转调本函数），
//! 见该函数当前文档。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_world::entity::{Agent, EntityId};
use ll_world::mod_state::{ModStateValue, ModStateWrite};
use ll_world::state::WorldState;

use crate::effect::Effect;

/// 一条"击杀某种类达到某个数量即完成"的任务规则——[`QuestCatalog`] 的
/// 查询结果，供 [`kill_progress_effects`] 使用。
///
/// 携带 `quest` 这个 [`NamespacedId`]（而不是只有 `ContentIndex`）：
/// [`mark_quest_completed`] 按命名空间隔离写入任务进度（见其文档），
/// 需要完整的字符串标识符，`QuestTable` 自身不存这个字符串（见
/// `crates/ll-mod/src/quest.rs` 的 `QuestView` 文档），因此查询结果
/// 必须把反查得到的 `NamespacedId` 一并带出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestKillRule {
    /// 这条规则对应的任务节点。
    pub quest: NamespacedId,
    /// 需要击杀的目标"种类"——本批次与 [`ll_world::entity::Agent::race`]
    /// 匹配，见本模块文档「击杀计数」一节的简化说明。
    pub target_kind: ContentIndex,
    /// 需要达到的击杀数量。
    pub required_count: u32,
    /// 这条任务节点的前置任务，同样已反查成 [`NamespacedId`]——
    /// [`kill_progress_effects`] 用它判断"这个节点是否已经解锁"（全部
    /// 前置都已完成），不满足就不产出完成写入，即使 `required_count`
    /// 本身已经达标。**这不是可选的装饰字段**：这是验收 demo（P5-B
    /// 任务 9）实测抓出的一处真实缺陷的修复——`finale` 节点自己的
    /// `KillCount` 条件只要求击杀 1 个哥布林，若不检查前置，玩家杀满
    /// 3 个哥布林（够 `main_quest_1` 的阈值）时 `finale` 会跟着一起
    /// "完成"，即便 `branch_a`/`branch_b` 两个前置任务都还没做——一个
    /// 尚未解锁、玩家从未在任务日志里见过的任务节点凭空显示为已完成。
    pub prerequisites: Vec<NamespacedId>,
}

/// `resolve` 结算击杀时需要的最小"任务目录"接口——把"哪些任务节点有
/// `KillCount` 完成条件"这件事与"任务定义具体存在哪个 crate、用什么
/// 容器存"解耦，见本模块文档「为什么搬到这里」一节完整论证。
///
/// 只交付 `KillCount`（一档）：`Script`（三档）需要真正调用脚本引擎，
/// 那是运行期判定管线的职责（`crates/ll-mod/src/quest.rs` 模块文档
/// 「完成条件分档」一节已经记录同一条边界），本 trait 不越界代为决定
/// 脚本条件是否满足。
pub trait QuestCatalog {
    /// 返回全部登记了 `KillCount` 完成条件的任务规则，任意确定顺序
    /// （[`kill_progress_effects`] 只按 `target_kind` 过滤，不依赖调用方
    /// 给出的顺序）。
    fn kill_count_quests(&self) -> Vec<QuestKillRule>;
}

/// 空任务目录：不知道任何 `KillCount` 规则。
///
/// 是 [`crate::resolve::resolve`]/[`crate::resolve::resolve_with_skills`]
/// （不接收任务目录参数的两个既有入口）内部用来结算击杀时的默认实现
/// ——与 [`crate::skill::NoSkills`] 同一个理由：调用方没有任务内容时
/// 的保底行为，不是特殊路径。真正想让击杀推进任务进度的调用方应改用
/// [`crate::resolve::resolve_with_skills_and_quests`]，传入一个实现了
/// [`QuestCatalog`] 的目录（`ll_mod::quest::RegisteredQuests` 就是这样
/// 一个实现）。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoQuests;

impl QuestCatalog for NoQuests {
    fn kill_count_quests(&self) -> Vec<QuestKillRule> {
        Vec::new()
    }
}

/// 任务进度键的前缀——脚本状态存储按 `(mod_namespace, key)` 隔离
/// （`ll_world::mod_state` 模块文档），前缀避免任务进度与该 mod 存
/// 的其他状态（声望、计数器等）撞键。
const QUEST_PROGRESS_KEY_PREFIX: &str = "quest_progress:";

/// 给定任务节点 id，返回它在脚本状态存储里对应的键。
///
/// # 为什么存储命名空间用任务自身的定义命名空间
///
/// "任务是 mod 定义的内容，进度应该按 mod 命名空间隔离"指的是**任务
/// 本身归属的 mod**，不是"判定这次完成的逻辑恰好跑在哪个 mod 的脚本
/// 上下文里"——即便未来某个通用"任务判定引擎"由另一个 mod 提供，某个
/// 任务节点的完成记录也应该始终落在定义它的那个 mod 的命名空间下，这样
/// 卸载/替换判定引擎 mod 不会让已有的任务进度数据变得孤儿或错位。
/// [`mark_quest_completed`]/[`is_quest_completed`] 因此都用
/// `quest.namespace()` 作为存储命名空间，键本身再把完整 `NamespacedId`
/// （含命名空间）拼进字符串——这一层看起来冗余（命名空间在存储位置与
/// 键字符串里各出现一次），但保留完整 id 让读者从键本身就能确认"这是
/// 哪个任务"，不需要再回头看外层是哪个 mod 命名空间才能拼出完整语义。
pub fn quest_progress_key(quest: &NamespacedId) -> String {
    format!("{QUEST_PROGRESS_KEY_PREFIX}{quest}")
}

/// 产出一条标记 `actor` 已完成 `quest` 的脚本状态写入记录。
///
/// **不直接改任何 `WorldState`**——本函数只产出数据：调用方（
/// [`kill_progress_effects`]，或未来串起任务判定管线的 `resolve`）负责
/// 把返回值包进 [`Effect::SetModState`] 交给
/// [`crate::apply::apply`]（约束 C1，唯一写入口）。写入的值固定是
/// `ModStateValue::Int(1)`——"完成"是一个存在性判断（写过就是完成），
/// 不需要一个可以取多种值的状态机。
pub fn mark_quest_completed(actor: EntityId, quest: &NamespacedId) -> ModStateWrite {
    ModStateWrite {
        entity: actor,
        mod_namespace: quest.namespace().to_string(),
        key: quest_progress_key(quest),
        value: ModStateValue::Int(1),
    }
}

/// 查询 `agent` 是否已完成 `quest`——直接读取已提交的
/// [`Agent::mod_state`]，不经脚本调用。
///
/// # 为什么是 Rust 直接读取路径，不强制经脚本
///
/// `ll_script::api::state` 的 `entity-state-get!` 是脚本调用路径；C1
/// 「`apply` 是唯一写入口」只约束**写**，不约束读——直接读取已提交的
/// `WorldState` 字段是全代码库到处都在做的事。本函数因此是给 Rust 侧
/// 判定逻辑（[`kill_progress_effects`]、未来的 `QuestLogView`）准备的
/// 直接路径，不需要每次判定都起一次 Steel VM 调用。
pub fn is_quest_completed(agent: &Agent, quest: &NamespacedId) -> bool {
    matches!(
        agent
            .mod_state
            .get(&(quest.namespace().to_string(), quest_progress_key(quest))),
        Some(ModStateValue::Int(1))
    )
}

/// 击杀计数的存储命名空间——本体保留命名空间，与真实 mod 使用的
/// 命名空间不冲突（`lostland` 本就是本体的既有命名空间，`materialize_
/// base_quests` 的本体任务/`materialize_base_skills` 的本体技能都用
/// 它），击杀计数因此可以看成"本体额外提供的一项引擎级统计"。
const KILL_COUNT_NAMESPACE: &str = "lostland";

/// 击杀计数键前缀。键本身用 `ContentIndex::get()` 的数值而不是字符串
/// 标识符——与 `QuestCondition::KillCount.target_kind` 本身就是裸
/// `ContentIndex`（见 [`QuestKillRule::target_kind`] 文档）同一个抽象
/// 层级，不为此额外做一次"数值转字符串"的反查（那需要一份 `Registry`
/// 引用，`kill_progress_effects` 不需要为击杀计数这一件事额外要求
/// 调用方传入它——`QuestKillRule` 已经把需要反查的部分：任务自身的
/// `NamespacedId`，在 `QuestCatalog::kill_count_quests` 里一次性做完了）。
const KILL_COUNT_KEY_PREFIX: &str = "kill_count:";

fn kill_count_key(kind: ContentIndex) -> String {
    format!("{KILL_COUNT_KEY_PREFIX}{}", kind.get())
}

/// 读取 `agent` 当前对 `kind` 这个种类的累计击杀数，未写入过时为 0。
fn kill_count(agent: &Agent, kind: ContentIndex) -> i64 {
    match agent
        .mod_state
        .get(&(KILL_COUNT_NAMESPACE.to_string(), kill_count_key(kind)))
    {
        Some(ModStateValue::Int(n)) => *n,
        _ => 0,
    }
}

/// 击杀结算的核心：`actor` 击杀了一个种类为 `killed_kind` 的目标之后，
/// 应该产出的效果——累计击杀数 +1，以及任何因此达标、且尚未完成的
/// `KillCount` 任务的完成写入，全部打包进**一条** `Effect::SetModState`
/// （与批量写入的既有纪律一致，见 [`Effect::SetModState`] 文档）。
///
/// `actor` 不存在（已被同一批结算里更早的效果销毁）时返回空
/// `Vec`——与本 crate `resolve`/`apply` 全部既有分支「目标不存在时
/// 静默忽略」的纪律一致。
pub fn kill_progress_effects(
    world: &WorldState,
    actor: EntityId,
    killed_kind: ContentIndex,
    quests: &dyn QuestCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let new_count = kill_count(agent, killed_kind) + 1;
    let mut writes = vec![ModStateWrite {
        entity: actor,
        mod_namespace: KILL_COUNT_NAMESPACE.to_string(),
        key: kill_count_key(killed_kind),
        value: ModStateValue::Int(new_count),
    }];
    for rule in quests.kill_count_quests() {
        let prerequisites_met = rule
            .prerequisites
            .iter()
            .all(|prerequisite| is_quest_completed(agent, prerequisite));
        if rule.target_kind == killed_kind
            && new_count >= i64::from(rule.required_count)
            && prerequisites_met
            && !is_quest_completed(agent, &rule.quest)
        {
            writes.push(mark_quest_completed(actor, &rule.quest));
        }
    }
    vec![Effect::SetModState { writes }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    #[test]
    fn quest_progress_key对不同任务产出不同的键() {
        // Arrange
        let quest_a = id("lostland:main_quest_1");
        let quest_b = id("lostland:branch_a");

        // Act & Assert
        assert_ne!(quest_progress_key(&quest_a), quest_progress_key(&quest_b));
    }

    #[test]
    fn noquests的kill_count_quests恒返回空列表() {
        // Arrange
        let catalog = NoQuests;

        // Act & Assert
        assert!(catalog.kill_count_quests().is_empty());
    }
}
