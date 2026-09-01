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
//!   逐条同办。
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
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

use crate::dialogue::{
    ContentIdLookup, DialogueCatalog, DialogueOutcome, all_conditions_hold, set_dialogue_flag,
};
use crate::effect::Effect;

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
    let writes: Vec<_> = view
        .outcomes
        .iter()
        .map(|outcome| match outcome {
            DialogueOutcome::SetFlag(flag) => set_dialogue_flag(actor, flag),
        })
        .collect();
    if writes.is_empty() {
        return Vec::new();
    }
    // 一次决策里的全部写入攒成**一条** `Effect`，见
    // [`Effect::SetModState`] 文档「为什么是一条 `Effect` 携带一批」。
    //
    // **这里就是「不产 `ScheduleNext`」的落点**：这个 `vec!` 只有一项。
    vec![Effect::SetModState { writes }]
}
