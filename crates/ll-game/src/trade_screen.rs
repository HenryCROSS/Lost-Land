//! 交易屏：跟一个 NPC 讨价还价的那一块模态屏。
//!
//! 设计冻结在 `knowledge/design/dialogue-system.md` **五节 5.3**，落地
//! 计划见 `docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md`
//! 五节。
//!
//! # 它就是既有的模态屏，不是新造的一套
//!
//! 与 [`crate::dialogue_screen`] 逐条同形，`ll_ui::screen::ScreenData`
//! 的五个槽刚好装得下：
//!
//! | 槽 | 装什么 |
//! |---|---|
//! | `title_key` | `screen-trade-title`（字面量——只有会话屏的标题是现算的） |
//! | `rows` | 对方背包（买）在前、自己背包（卖）在后，各行带价 |
//! | `cursor` | 预选第一项 |
//! | `empty_key` | 两边都没有可交易的东西时的占位行 |
//! | `hint_key` | 底部按键提示 |
//!
//! 白送的三样东西（行矩形 = 鼠标点在第几行、聚焦/悬停高亮、面板按内容
//! 宽度伸缩）与其余模态屏**同一次**产出，`crate::pointer` 那四条约定
//! 一行代码都不用重写。
//!
//! # 顺序确定（约束 C5）
//!
//! 两侧的 `inventory` 都是 `Vec`，全程线性扫描，不碰任何哈希容器。
//! 这条在这里是**真陷阱**而不是形式要求：玩家按的是「第几行」，而按下
//! 去的后果是一笔钱和一件东西换手。
//!
//! # 判据只写一份
//!
//! 行上显示的价钱与 `resolve` 真正结算的价钱**调的是同一个函数**
//! （`ll_sim::trade::trade_price` + `ll_sim::trade::partner_standing`）。
//! 这与会话屏的选项过滤共用 `all_conditions_hold` 是同一条纪律（规格
//! 7.2）：UI 看的是某一帧的世界，`resolve` 结算时世界可能已经变了，
//! 但**判据本身只有一份**，分叉时没有任何东西会报错。
//!
//! # 交易不消耗回合
//!
//! **本批自裁，规格没写**（计划文档三节 3.6）。落点在 `ll-sim` 那一侧
//! （`resolve_trade` 不产 `Effect::ScheduleNext`），本模块只是不做任何
//! 与时间有关的事。

use ll_core::ident::ContentIndex;
use ll_i18n::{Catalog, FluentArgs};
use ll_mod::item::ItemTable;
use ll_platform::input::{GameKey, InputState};
use ll_sim::intent::Intent;
use ll_sim::item::ItemCatalog;
use ll_sim::trade::{TradeDirection, partner_standing, trade_price};
use ll_ui::hud::item_display_name;
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

use crate::menu_screen::{ScreenOutcome, ScreenState};
use crate::pointer::RowPointer;

/// 交易屏这一帧的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeRow {
    /// 这一行是买还是卖。
    pub direction: TradeDirection,
    /// 成交的是哪一种东西。
    pub item: ContentIndex,
    /// 这一件多少钱——**显示的就是结算会用的那个数**，见模块文档
    /// 「判据只写一份」。
    pub price: i64,
    /// 这一行显示什么（已经过 `Catalog` 解析）。
    pub text: String,
}

/// 这一帧交易屏要显示的行。
///
/// **对方的货在前、自己的货在后**：玩家进这块屏的第一意图是「他卖什么」
/// （`open-trade` 那条选项的措辞就是在问这个）。同一件东西两边都有时
/// 会各占一行，那是对的——买价与卖价是两笔不同的交易。
///
/// 查不到实体（对方刚死、玩家不在世界里）时返回空列表：玩家会看到占位
/// 行，按取消退出。**不 panic**——与本 crate 其余降级路径同一条纪律。
///
/// # 顺序确定（约束 C5）
///
/// 见模块文档。
pub fn trade_rows(
    world: &WorldState,
    player: EntityId,
    partner: EntityId,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
) -> Vec<TradeRow> {
    let (Some(player_agent), Some(partner_agent)) =
        (world.actors.get(player), world.actors.get(partner))
    else {
        return Vec::new();
    };
    // 声望只查一次：它与「这一行是哪一件东西」无关，逐行重查只是重复
    // 同一次遍历。
    let standing = partner_standing(world, player, partner);
    let mut rows = Vec::new();
    for (direction, owner_agent, key) in [
        (TradeDirection::Buy, partner_agent, "screen-trade-buy"),
        (TradeDirection::Sell, player_agent, "screen-trade-sell"),
    ] {
        for stack in &owner_agent.inventory {
            let Some(rule) = ItemCatalog::item(items, stack.def) else {
                // 内容里查不到这条规则 ⇒ 结算侧那道闸门也会拒
                // （`resolve_trade` 第四道）。**这里就不显示它**，
                // 而不是显示一行按下去什么都不会发生的死行。
                continue;
            };
            let price = trade_price(rule.base_price, standing);
            let mut args = FluentArgs::new();
            args.set(
                "item",
                item_display_name(
                    stack.def,
                    items,
                    catalog,
                    language,
                    &player_agent.identified_items,
                ),
            );
            args.set("count", stack.count);
            args.set("price", price);
            rows.push(TradeRow {
                direction,
                item: stack.def,
                price,
                text: catalog.resolve_with_args(language, key, Some(&args)),
            });
        }
    }
    rows
}

/// 交易屏这一帧的产出，形状同
/// [`crate::dialogue_screen::DialogueUpdate`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeUpdate {
    /// 处理完这一帧输入之后调用方该做什么。
    pub outcome: ScreenOutcome,
    /// 要切到哪一块屏，`None` 表示留在这块屏上。
    pub next: Option<ScreenState>,
    /// 这一帧要提交的意图，`None` 表示什么都不提交。
    ///
    /// 与会话屏同一条理由：「提交一条意图」与「这块屏接下来怎么办」是
    /// 两件正交的事。交易屏成交之后**留在屏上**（玩家通常要连买几件），
    /// 因此这里恒是 `Idle` + `Some(意图)`。
    pub submit: Option<Intent>,
}

impl TradeUpdate {
    fn idle() -> Self {
        Self {
            outcome: ScreenOutcome::Idle,
            next: None,
            submit: None,
        }
    }
}

/// 处理交易屏这一帧的输入。
///
/// 键盘与鼠标走**同一条动作分派**（`input.was_just_pressed(Confirm)
/// || pointer.activated()`），不为鼠标另写一套——与
/// [`crate::dialogue_screen::update_dialogue`] 逐条同形。
///
/// # 成交之后留在屏上，光标不动
///
/// 与会话屏「换节点就把光标复位到第一项」相反：交易屏的行列表在成交后
/// 只是**某一行的数量少了一件**（或者整行消失），把光标弹回第一项会让
/// 连买三件变成每次都要重新翻。行数变少时由调用方按新的行数夹住光标
/// （[`clamp_cursor`]）。
pub fn update_trade(
    cursor: &mut usize,
    rows: &[TradeRow],
    player: EntityId,
    partner: EntityId,
    input: &InputState,
    pointer: RowPointer,
) -> TradeUpdate {
    // 取消键**关掉整块屏**：交易屏的上一层是世界，不是刚才那段对话
    // （`ll_game::modal::Modal` 只有一块屏，不是栈，见计划文档 2.3）。
    if input.was_just_pressed(GameKey::Cancel) {
        return TradeUpdate {
            outcome: ScreenOutcome::Close,
            next: None,
            submit: None,
        };
    }
    if rows.is_empty() {
        // 两边都拿不出可交易的东西：上下键无处可动，确认键无行可选。
        return TradeUpdate::idle();
    }
    // 规格 N11：循环 + 长按连发，走九块屏共用的
    // `crate::nav_row::moved_cursor`。
    if let Some(next) = crate::nav_row::moved_cursor(input, *cursor, rows.len()) {
        *cursor = next;
    }
    // 指针按下把光标挪过去（约定一：**悬停**不改焦点，按下才改）。
    if let Some(row) = pointer.focus_row() {
        *cursor = row.min(rows.len() - 1);
    }
    if !(input.was_just_pressed(GameKey::Confirm) || pointer.activated()) {
        return TradeUpdate::idle();
    }
    let Some(row) = rows.get(*cursor) else {
        return TradeUpdate::idle();
    };
    TradeUpdate {
        outcome: ScreenOutcome::Idle,
        next: None,
        submit: Some(Intent::Trade {
            actor: player,
            partner,
            item: row.item,
            direction: row.direction,
        }),
    }
}

/// 把光标夹进当前行数——成交之后行可能变少（整堆卖光了）。
///
/// 与 `crate::save_list::clamp_cursor` 同一条既有手法：行数是每一帧现算
/// 的，光标是跨帧留下的，两者对不齐时**夹住**而不是 panic、也不是复位
/// 到 0（复位会把玩家刚才翻到的位置扔掉）。
pub fn clamp_cursor(cursor: usize, rows: usize) -> usize {
    cursor.min(rows.saturating_sub(1))
}
