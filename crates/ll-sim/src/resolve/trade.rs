//! [`Intent::Trade`](crate::intent::Intent::Trade) 的结算：一件物品换
//! 一笔钱。
//!
//! 设计冻结在 `knowledge/design/dialogue-system.md` **五节 5.3**，落地
//! 计划见 `docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md`
//! 三节。判定那一半（方向、价格公式、声望查询）在 [`crate::trade`]。
//!
//! # 三条硬约束在这里各自的落点
//!
//! - **C1（`apply` 是唯一写入口）**：本模块一个字节的世界状态都不改，
//!   只产出 [`Effect`]。**五道闸门全部在这里判完**——`apply` 只机械
//!   执行，一句判断都不做。
//!   `open-trade` 那条对话后果「不产 `Effect` 只推 UI」与这一点不矛盾：
//!   那是 UI 状态，这是世界状态，分界见规格七节 7.1。
//! - **交易不消耗回合**（**本批自裁，规格没写**）：**不产出
//!   [`Effect::ScheduleNext`]**。理由与反转成本写在计划文档三节 3.6，
//!   一句话是「加一条是一行，撤一条要动回放摘要」。守卫是
//!   `crates/ll-sim/tests/trade.rs` 的 `交易不消耗回合`。
//! - **C5（哈希容器的迭代顺序不参与判断）**：背包是 `Vec`，取第一条
//!   匹配（与 [`Effect::RemoveFromInventory`] /
//!   [`Effect::ConsumeInventoryItem`] 的既有定位纪律相同）；归属列表也
//!   是 `Vec`。全程不碰任何哈希容器。
//!
//! # 货币守恒
//!
//! 产出的两条 [`Effect::AdjustWallet`] 一正一负、绝对值相同：买方付
//! 多少，卖方就收多少。**不存在第三方**（没有税、没有中介抽成——行会
//! 中介整体属 P9，规格 5.3 点名不要在这里实现它的任何一部分）。

use ll_core::ident::ContentIndex;
use ll_world::entity::EntityId;
use ll_world::item::ItemStack;
use ll_world::state::WorldState;

use crate::effect::Effect;
use crate::item::ItemCatalog;
use crate::ownership::{holder_owner, may_give_away};
use crate::trade::{TradeDirection, partner_standing, trade_price};

use super::inventory::merge_into_inventory_effect;

/// 结算一次成交。
///
/// # 五道闸门，任何一道不过就返回**空效果**
///
/// 1. **两位当事人都还在世界里**；
/// 2. **卖方背包里真的有一堆 `item`**（第一条匹配）；
/// 3. **owner 校验硬前置**——[`may_give_away`]，与对话赠送**调的是同
///    一个函数**（ADR 0021，见那个函数文档「两个调用方，一份判据」）。
///    卖不掉的东西也送不掉，反过来也一样：这条判据没有「交易版」；
/// 4. **这件东西查得到定价**（`items` 里没有这条规则 ⇒ 不知道多少钱
///    ⇒ 不成交，而不是按 0 白送）；
/// 5. **买方的钱包付得起**。
///
/// 空效果经 `TurnEngine::try_player_intent` 变成
/// [`crate::turn::PlayerTurnOutcome::Nothing`]——「按了但什么都没发生」，
/// 玩家收到一句通用反馈。与批次 4 的 `give-item` 逐条同一条纪律：不
/// panic、不产出一条什么都不做的效果、不新增一条反馈键。
///
/// # 为什么产出的**不是** [`Effect::TransferOwnership`]
///
/// 批次 4 已经把这条论证做完了（那个效果只改 `owner` 不搬运，两种排法
/// 一种会波及卖方剩下的几件、另一种定位不到刚收下的那一堆），完整两张
/// 表见 `crate::resolve::dialogue` 的 `give_item` 文档与
/// `docs/superpowers/plans/2026-08-31-batch29-dialogue-quest.md` 三节
/// 3.5。**本批一个字不改地继承它**：归属由 `resolve` 算好写进搬运效果
/// （[`holder_owner`]），`Effect::TransferOwnership` 至今仍无调用方。
pub(super) fn resolve_trade(
    world: &WorldState,
    actor: EntityId,
    partner: EntityId,
    item: ContentIndex,
    direction: TradeDirection,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let (seller, buyer) = direction.seller_and_buyer(actor, partner);
    let (Some(seller_agent), Some(buyer_agent)) =
        (world.actors.get(seller), world.actors.get(buyer))
    else {
        return Vec::new();
    };
    let Some(held) = seller_agent
        .inventory
        .iter()
        .find(|stack| stack.def == item)
    else {
        return Vec::new();
    };
    // **与对话赠送共用的那一条**：不属于卖方的东西卖不出去。
    if !may_give_away(holder_owner(world, seller_agent, seller), held.owner) {
        return Vec::new();
    }
    let Some(rule) = items.item(item) else {
        // 查不到定价规则 ⇒ 不知道它值多少钱。**不按 0 成交**——那等于
        // 让一件内容没登记价格的东西变成免费搬运通道。
        return Vec::new();
    };
    // 价格系数读的是**发起者**与对方所属势力之间的声望，两个方向同价
    // （不设买卖差价，理由见计划文档十一节）。
    let price = trade_price(rule.base_price, partner_standing(world, actor, partner));
    if buyer_agent.wallet < price {
        return Vec::new();
    }
    // 一次一件（见 `Intent::Trade` 文档「为什么不带 count」），归属在
    // 这里算好——`apply` 只照单执行。
    let sold = ItemStack {
        count: 1,
        owner: holder_owner(world, buyer_agent, buyer),
        ..*held
    };
    vec![
        Effect::ConsumeInventoryItem {
            actor: seller,
            def: item,
            durability: held.durability,
        },
        merge_into_inventory_effect(buyer_agent, buyer, sold, items),
        // 货币守恒：这两条的和恒为零。
        Effect::AdjustWallet {
            actor: buyer,
            delta: -price,
        },
        Effect::AdjustWallet {
            actor: seller,
            delta: price,
        },
    ]
}
