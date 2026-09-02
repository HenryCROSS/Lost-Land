//! `app::session_screens`：**底下有一局世界、而且会提交意图**的那两块屏
//! ——会话屏与交易屏。
//!
//! 本模块由 [`super::screen_flow`] 按职责拆出（批次 31，纯搬移，没有改动
//! 任何逻辑）。拆分依据不是行数而是一条能用一句话说清的分界：
//!
//! - **`screen_flow`** 回答「这一帧这块屏画哪些行、按了键之后切到哪块屏」；
//! - **本模块**回答「这一帧的输入怎么变成一条 `ll_sim::intent::Intent`，
//!   并经 `TurnEngine` 落到世界上」。
//!
//! 除这两块屏之外的六块屏都不提交意图（它们改的是配置、存档槽、草稿这类
//! **不属于世界状态**的东西），因此这条分界正好把这两块屏与其余六块分开，
//! 而不是把同一件事切成两半。与 `crate::interact_list` 从
//! `crate::player_action` 拆出去（对话批次 2）是同一条纪律：
//! 「本模块回答『有什么』，那边回答『按了什么键、于是提交什么』」。
//!
//! **约束 C1**：提交走的是与玩家按键完全相同的
//! [`ll_sim::turn::TurnEngine::try_player_intent`]，**不是**另开一条
//! 「屏里也能改世界」的小路。

use ll_platform::input::InputState;

use crate::content::RuntimeCatalogs;
use crate::menu_screen::{ScreenOutcome, ScreenState};
use ll_sim::effect::Effect;

use super::Demo;

impl Demo {
    /// 会话屏这一帧：算行 → 交给 [`crate::dialogue_screen::update_dialogue`]
    /// → 把它要提交的意图送进回合引擎。
    ///
    /// # 行在这里**又算了一遍**，这不是那个「两份同一个算法」的形状
    ///
    /// [`Demo::resolve_screen_pointer`] 刚刚算过一遍同样的行（为了拿行
    /// 矩形）。两遍调的是**同一个函数**
    /// （[`crate::dialogue_screen::dialogue_rows`]）、跑在同一帧的同一份
    /// 世界上，因此逐条相同——与 `crate::player_action::menu_rows` 和
    /// `player_command` 各自重建一次列表是同一条既有取舍，理由见那里
    /// 的文档：攒成一个跨帧字段才是真正的风险（要有人负责让它失效）。
    ///
    /// # 提交意图这条路径
    ///
    /// 走的是与玩家按键完全相同的
    /// [`ll_sim::turn::TurnEngine::try_player_intent`]——**不是**另开
    /// 一条「屏里也能改世界」的小路（约束 C1）。因此
    /// `Intent::DialogueChoose` 的结算、条件重新校验、效果落地全部与
    /// `crate::player_action` 提交的那六个意图逐条同办。
    pub(super) fn update_dialogue_screen(
        &mut self,
        speaker: ll_world::entity::EntityId,
        node: ll_core::ident::ContentIndex,
        cursor: &mut usize,
        input: &InputState,
        pointer: crate::pointer::RowPointer,
    ) -> (ScreenOutcome, Option<ScreenState>) {
        let Some(session) = self.session.as_mut() else {
            // 底下没有世界却停在会话屏上是不该发生的状态；关掉这块屏
            // 比 panic 好，与本模块其余降级路径一致。
            return (ScreenOutcome::Close, None);
        };
        let player = session.game_world.player;
        let Some(agent) = session.game_world.world.actors.get(player) else {
            return (ScreenOutcome::Close, None);
        };
        let rows = crate::dialogue_screen::dialogue_rows(
            node,
            &self.content.dialogue_node_table,
            agent,
            &self.content.registry,
            &self.catalog,
            &self.config.language,
        );
        let update = crate::dialogue_screen::update_dialogue(
            node,
            cursor,
            &rows,
            &self.content.dialogue_node_table,
            crate::dialogue_screen::DialogueParticipants {
                actor: player,
                speaker,
            },
            input,
            pointer,
        );
        if let Some(intent) = update.submit {
            let runtime_catalogs = RuntimeCatalogs::new(&self.content);
            let catalogs = runtime_catalogs.as_resolve_catalogs();
            let mut on_effect = |_world: &ll_world::state::WorldState, _effect: &Effect| {};
            session.engine.try_player_intent(
                &mut session.game_world.world,
                player,
                intent,
                &catalogs,
                &mut on_effect,
            );
        }
        (update.outcome, update.next)
    }

    /// 交易屏这一帧：算行、走状态机、把成交那条意图交给回合引擎。
    ///
    /// 与 [`Self::update_dialogue_screen`] 逐条同形，两处差别各有理由：
    ///
    /// - **光标先夹后用**：成交之后行会变少（整堆卖光了），而光标是跨帧
    ///   留下的。夹住而不是复位到 0——复位会把玩家刚翻到的位置扔掉，
    ///   见 `crate::trade_screen::clamp_cursor`。
    /// - **不换屏**：成交之后留在这块屏上（玩家通常要连买几件）。
    pub(super) fn update_trade_screen(
        &mut self,
        partner: ll_world::entity::EntityId,
        cursor: &mut usize,
        input: &InputState,
        pointer: crate::pointer::RowPointer,
    ) -> (ScreenOutcome, Option<ScreenState>) {
        let Some(session) = self.session.as_mut() else {
            // 底下没有世界却停在交易屏上是不该发生的状态；关掉这块屏
            // 比 panic 好，与本模块其余降级路径一致。
            return (ScreenOutcome::Close, None);
        };
        let player = session.game_world.player;
        let rows = crate::trade_screen::trade_rows(
            &session.game_world.world,
            player,
            partner,
            &self.content.item_table,
            &self.catalog,
            &self.config.language,
        );
        *cursor = crate::trade_screen::clamp_cursor(*cursor, rows.len());
        let update =
            crate::trade_screen::update_trade(cursor, &rows, player, partner, input, pointer);
        if let Some(intent) = update.submit {
            let runtime_catalogs = RuntimeCatalogs::new(&self.content);
            let catalogs = runtime_catalogs.as_resolve_catalogs();
            let mut on_effect = |_world: &ll_world::state::WorldState, _effect: &Effect| {};
            session.engine.try_player_intent(
                &mut session.game_world.world,
                player,
                intent,
                &catalogs,
                &mut on_effect,
            );
        }
        (update.outcome, update.next)
    }
}
