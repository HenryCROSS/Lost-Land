//! 对话族意图的结算：目前只有 [`Intent::DialogueChoose`](crate::intent::Intent::DialogueChoose)
//! 一条。
//!
//! 设计冻结在 `knowledge/design/dialogue-system.md` **七节 7.2**，落地
//! 计划见 `docs/superpowers/plans/2026-08-31-batch21-dialogue-ui.md` 五节。
//!
//! # 三条硬约束在这里各自的落点
//!
//! - **C1（`apply` 是唯一写入口）**：本模块一个字节的世界状态都不改，
//!   只产出 [`Effect`]。`set-flag` 走
//!   [`Effect::SetModState`]，与 [`crate::quest::mark_quest_completed`]
//!   逐条同办；`join-settlement`（批次 3）走
//!   [`Effect::AddAffiliation`]，同一条纪律。
//! - **对话不消耗回合**（所有者裁定，交接文档第〇之二节第 2 条）：
//!   **不产出 [`Effect::ScheduleNext`]**。`TurnEngine::perform` 末尾
//!   无条件 `timeline.schedule(actor, agent.next_action_at)`，因此
//!   `next_action_at` 不变 = 玩家在同一刻重新入列，`world.clock` 也
//!   不动。
//! - **C5（哈希容器的迭代顺序不参与判断）**：`outcomes` 是 `Vec`
//!   （JSON5 数组按书写顺序），逐条线性翻译，不碰任何哈希容器。
//!
//! # 为什么要重新校验条件
//!
//! `option` 是 UI 按**某一帧**的世界快照算出来的下标。UI 与结算之间
//! 世界可能已经变了（另一条选项的后果刚刚设过一条旗标、背包里那件
//! 东西刚被用掉），那一行此刻可能已经不该显示。
//!
//! **判据只写一份**：这里调的 [`all_conditions_hold`] 与
//! `ll_game::dialogue_screen::dialogue_rows` 过滤显示行时调的是**同一个
//! 函数**（规格 7.2）。两边各写一遍的代价不是多几行，是两份判据会各自
//! 漂移，而漂移时没有任何东西会报错——正是 ADR 0021 点名要拦的形状。

use ll_core::ident::ContentIndex;
use ll_world::entity::{Affiliation, AffiliationKind, EntityId, OrgRef};
use ll_world::mod_state::ModStateWrite;
use ll_world::state::WorldState;

use crate::dialogue::{
    ContentIdLookup, DialogueCatalog, DialogueOutcome, JOIN_SETTLEMENT_STANDING,
    all_conditions_hold, set_dialogue_flag,
};
use crate::effect::Effect;
use crate::quest::mark_quest_completed;

/// 结算一次「选中了一条带后果的对话选项」。
///
/// 四道闸门，任何一道不过就返回**空效果**（不是 panic、也不是一个
/// 「什么都不做的效果」）：
///
/// 1. 发起者还在世界里；
/// 2. 这个节点的第 `option` 条选项查得到（节点被换掉、下标越界——两样
///    都可能来自一个已经过时的 UI 帧）；
/// 3. 这条选项的条件此刻**仍然**全部满足（见模块文档）；
/// 4. 它真的带后果（空 `outcomes` 按规格 7.2 压根不该产出这条意图，
///    但结算侧不假设调用方守规矩）。
///
/// 空效果经 `TurnEngine::try_player_intent` 会变成
/// [`crate::turn::PlayerTurnOutcome::Nothing`]——「按了但什么都没发生」，
/// 玩家会收到一句反馈，而不是静默作废。
pub(super) fn resolve_dialogue_choose(
    world: &WorldState,
    actor: EntityId,
    speaker: EntityId,
    node: ContentIndex,
    option: usize,
    dialogues: &dyn DialogueCatalog,
    content_ids: &dyn ContentIdLookup,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(view) = dialogues.option(node, option) else {
        return Vec::new();
    };
    if !all_conditions_hold(view.conditions, agent, &content_ids) {
        return Vec::new();
    }
    // 逐条把**声明**翻译成 `Effect`（规格五节 5.0 的分工）。顺序就是
    // `outcomes` 的书写顺序（`Vec`，不碰任何哈希容器——约束 C5）。
    let mut effects: Vec<Effect> = Vec::new();
    let mut writes: Vec<ModStateWrite> = Vec::new();
    for outcome in view.outcomes {
        match outcome {
            DialogueOutcome::SetFlag(flag) => writes.push(set_dialogue_flag(actor, flag)),
            DialogueOutcome::JoinSettlement => {
                effects.extend(join_settlement(world, actor, speaker));
            }
            // 「把这条任务标记成已完成」——**调既有函数**，不重写一份
            // 完成逻辑（ADR 0021，见 `DialogueOutcome::CompleteQuest`
            // 文档）。`mark_quest_completed` 返回的就是一条
            // `ModStateWrite`，形状与 `set-flag` 那一支天生对得上，因此
            // 攒进同一条 `Effect::SetModState`。
            //
            // 反查不到（`content_ids` 是 `NoContentIds`，或者这个索引
            // 压根没注册过）⇒ 这一条后果零效果，与本模块其余闸门同一条
            // 纪律：不 panic，也不产出一条什么都不做的效果。
            DialogueOutcome::CompleteQuest(quest) => {
                if let Some(id) = content_ids.id_of(*quest) {
                    writes.push(mark_quest_completed(actor, id));
                }
            }
        }
    }
    if !writes.is_empty() {
        // 一次决策里的全部脚本状态写入攒成**一条** `Effect`，见
        // [`Effect::SetModState`] 文档「为什么是一条 `Effect` 携带一批」。
        effects.push(Effect::SetModState { writes });
    }
    // **这里就是「不产 `ScheduleNext`」的落点**：上面每一条分支产出的
    // 都只是世界状态写入，没有任何一条往时间轴里放东西。
    effects
}

/// 「加入说话人所属据点的势力」——[`DialogueOutcome::JoinSettlement`]
/// 那一支的全部内容。
///
/// # 五道闸门（前三道在调用方，这里是后两道）
///
/// 4. **说话人还在世界里，且他有 `home`**。玩家、以及任何不隶属于某座
///    据点的实体是 `None`——跟他说话没有「加入哪里」这回事。
/// 5. **那座据点查得到一个势力**
///    （[`ll_world::faction::FactionTable::faction_of`]，废墟与从不存在
///    的号返回 `None`）。
///
/// 任何一道不过就返回**空效果**，与本模块其余闸门同一条纪律：不 panic，
/// 也不产出一条什么都不做的效果。
///
/// # 加入的是**势力**，不是据点
///
/// 规格 5.1 原文那条「拿据点 `WorldId` 冒充 `Faction` 归属」的变通在势力
/// 播种批次之后已经作废；这里指向的是真正的
/// [`ll_world::faction::Faction`]，`org` 因此是
/// [`OrgRef::Instance`]（`AffiliationKind::Faction` 恒配它，见
/// `ll_world::entity::OrgRef` 文档）。
///
/// # 重复加入由 `apply` 兜住，不在这里再判一遍
///
/// 内容那一侧的 `not-affiliated` 条件已经挡了一层，`apply` 那一侧的
/// 「同一条 `(kind, org)` 已经在了就整条不做」是最终防线（见
/// [`Effect::AddAffiliation`] 文档）。在这里再抄一份判据，就是让同一条
/// 规则有两处真相源——ADR 0021 点名要拦的形状。
fn join_settlement(world: &WorldState, actor: EntityId, speaker: EntityId) -> Option<Effect> {
    let home = world.actors.get(speaker)?.home?;
    let faction = world.factions.faction_of(home)?;
    Some(Effect::AddAffiliation {
        entity: actor,
        affiliation: Affiliation {
            kind: AffiliationKind::Faction,
            org: OrgRef::Instance(faction),
            standing: JOIN_SETTLEMENT_STANDING,
        },
    })
}
