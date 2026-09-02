//! 交易的**判定**那一半：方向、占位价格公式、以及价格要读的那条声望。
//!
//! 设计冻结在 `knowledge/design/dialogue-system.md` **五节 5.3**，落地
//! 计划见 `docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md`
//! 三节。结算（产出 [`crate::effect::Effect`]）那一半在
//! `crate::resolve` 的 `Intent::Trade` 一支。
//!
//! # 这不是经济系统，是一个显式的占位
//!
//! 规格 5.3 原话：
//!
//! > `agent-goals-and-economy.md` 三节的完整设计（行会中介、库存/需求/
//! > 政策/商路四因子的本地价、再乘一项买家归属系数）依赖行会、库存、
//! > 商路——**全部属于 P9**。不要在这里实现它的任何一部分。
//! >
//! > ……**价格 = 物品基础价 × 买家归属系数**，两个因子今天都有……
//! > 四因子的本地价那一层**留空**，等 P9。这不是简化版的经济系统，
//! > 是**一个显式的占位公式**，要在代码里写明它将来会被
//! > `agent-goals-and-economy.md` 三节的公式替换。
//!
//! 这一段就是那句「在代码里写明」。[`trade_price`] 将来会被
//! `agent-goals-and-economy.md` 三节那条四因子公式**整体替换**，而不是
//! 在它上面叠加——本模块里的两个常量届时一并作废。
//!
//! # 货币守恒
//!
//! 一次成交产出**两条** [`crate::effect::Effect::AdjustWallet`]：买方
//! 减多少，卖方就加多少，和恒为零。本批唯一不守恒的地方是**世界生成期
//! 那一次性发放**（`ll_mod::roster::npc_initial_wallet`），它是一个已知
//! 的通胀源，记在 P9 的账上，见那个函数的文档。

use serde::{Deserialize, Serialize};

use ll_core::scaled::Milli;
use ll_world::entity::{Affiliation, AffiliationKind, Agent, EntityId, OrgRef};
use ll_world::state::WorldState;

/// 一次成交的方向，**站在发起者（玩家）的角度**。
///
/// 收成一个枚举而不是一个 `bool`：`Intent::Trade { .., direction: true }`
/// 在调用点看不出 `true` 是买还是卖，而这两者把钱和货的流向整个对调。
///
/// # 为什么它也要 `Serialize`/`Deserialize`
///
/// [`crate::intent::Intent`] 整个枚举是可序列化的（意图日志与回放），
/// 一个字段类型不可序列化会让整条链断掉。**新变体一律往后接**，与
/// `Intent` 自己那条纪律相同。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TradeDirection {
    /// 发起者**买进**：货从对方到发起者，钱从发起者到对方。
    Buy,
    /// 发起者**卖出**：货从发起者到对方，钱从对方到发起者。
    Sell,
}

impl TradeDirection {
    /// `(卖方, 买方)`——这一条把「方向」翻译成两位当事人的唯一一处。
    ///
    /// 结算侧全程只用这个二元组，不再各自 `match` 一遍方向：交易里
    /// 「谁交货、谁付钱」在两个方向上是同一段算法，只是两个参数对调
    /// （ADR 0021 说的共享算法）。
    pub fn seller_and_buyer(self, actor: EntityId, partner: EntityId) -> (EntityId, EntityId) {
        match self {
            TradeDirection::Buy => (partner, actor),
            TradeDirection::Sell => (actor, partner),
        }
    }
}

/// 声望为零（中立）时，价格就是基础价——系数的千分比基准。
pub const TRADE_PRICE_NEUTRAL_PERMILLE: i64 = 1000;

/// 声望从中立走到满值时，价格最多摆动多少（千分比）。
///
/// 取 200 = **两成**：满声望时打八折，敌对到底时加价两成。
///
/// # 为什么是 200，而不是别的数
///
/// 它必须同时满足两条，而这两条把它挤进一个很窄的区间：
///
/// 1. **必须大到玩家看得出来**——否则「加入据点」那条后果对交易的影响
///    观察不到，这条系数就等于不存在；
/// 2. **必须小到不足以改变「值不值得」**——占位公式的定价能力不该强到
///    让玩家围着它做决策，因为它整条都会在 P9 被替换掉。两成刚好落在
///    「看得见、但不足以主导」这一档。
///
/// 与 [`crate::dialogue::JOIN_SETTLEMENT_STANDING`] 一样，这是一次**数值
/// 决定**而不是一条推导出来的常量；写明理由是为了让后来人知道改它要付
/// 什么代价，不是为了假装它是算出来的。
pub const TRADE_STANDING_SWING_PERMILLE: i64 = 200;

/// 一件东西在这次交易里的价格，单位是
/// [`ll_world::entity::Agent::wallet`] 的「最小货币单位」。
///
/// ```text
/// 系数(千分比) = 1000 - standing × 200 / Affiliation::STANDING_FULL
/// 价格         = base.0 × 系数 / 1000        （非零基础价至少收 1）
/// ```
///
/// # 为什么读 `Milli` 的原始值而不是 `whole()`
///
/// 钱包的文档写的是「最小货币单位」，而 `Milli` 的最小单位就是它。
/// 本体 `mods/lostland/items.json5` 里一份烤肉是 `base_price: 900`
/// ——按 `whole()` 取整就是 **0**，那件东西会变成白拿。整套本体内容
/// 里 900 这一档并不罕见（草药、绳索、火把都在千位以下）。
///
/// # 为什么非零基础价至少收 1
///
/// 否则「满声望 + 低价物」这条组合会真的产出零价，而零价与
/// [`crate::effect::Effect::AdjustWallet`] `delta: 0` 是同一个东西——
/// 一次可以无限重复的免费搬运。基础价**本身**为 0 的东西价格仍然是 0，
/// 那是内容作者说它不值钱，不是公式算出来的边界。
///
/// # 溢出
///
/// 中间乘积走 `i128`（`base.0` 是 `i64`，系数至多 1200），结果再夹回
/// `i64`。这与 `ll_core::scaled::Milli::checked_mul_ratio` 用 `i128`
/// 承接是同一条既有纪律。
pub fn trade_price(base: Milli, standing: i32) -> i64 {
    if base.0 <= 0 {
        // 基础价非正 = 内容作者说它不值钱。**不夹到 1**：那会凭空给
        // 每一件无价物定出一个价，是公式在替内容作者做决定。
        return 0;
    }
    let standing = Affiliation::clamp_standing(standing) as i64;
    let permille = TRADE_PRICE_NEUTRAL_PERMILLE
        - standing * TRADE_STANDING_SWING_PERMILLE / Affiliation::STANDING_FULL as i64;
    let scaled =
        i128::from(base.0) * i128::from(permille) / i128::from(TRADE_PRICE_NEUTRAL_PERMILLE);
    i64::try_from(scaled).unwrap_or(i64::MAX).max(1)
}

/// `actor` 与 `partner` 所属势力之间的声望，查不到就是 `0`（中立原价）。
///
/// 查询链与 `crate::resolve` 的 `join-settlement` 一支**逐条相同**：
/// 说话人的 [`Agent::home`] → [`ll_world::faction::FactionTable::faction_of`]
/// → 发起者身上那条 `(Faction, OrgRef::Instance(势力))` 归属。
/// 批次 3（加入据点）让玩家第一次真的有了一条
/// [`Affiliation::standing`]，**本函数是它的第一个读者**。
///
/// # 两个方向都读**发起者**的声望
///
/// 规格 5.3 写的是「买家归属系数」。玩家**卖**东西时买家是 NPC，而
/// NPC 对玩家没有 `standing` 这个量（`Agent::affiliations` 里只有文化
/// 与势力两类归属，没有「对某个个体的态度」）。因此本批两个方向都用
/// 「玩家与对方势力」的那一条，如实登记在计划文档十一节。
///
/// # 为什么不复用 `DialogueCondition` 那条 `best_standing`
///
/// 那一条取的是「该类归属里的**最大值**」（「你在任何一个势力里够格
/// 就行」）；定价要的是「**跟眼前这个人所属的那个势力**关系如何」。
/// 判据不同，共用会让「在别处混得好」白白压低这里的价钱。共享的只有
/// 「归属列表是个 `Vec`，线性找」这点写法，那不是算法（ADR 0021）。
pub fn partner_standing(world: &WorldState, actor: EntityId, partner: EntityId) -> i32 {
    let Some(faction) = world
        .actors
        .get(partner)
        .and_then(|agent| agent.home)
        .and_then(|home| world.factions.faction_of(home))
    else {
        return 0;
    };
    let Some(agent) = world.actors.get(actor) else {
        return 0;
    };
    standing_towards(agent, faction)
}

/// `agent` 对 `faction` 这个势力的声望；没有这条归属就是 `0`。
///
/// 顺序确定（约束 C5）：`affiliations` 是 `Vec`，线性扫描，不碰任何
/// 哈希容器。同一个 `(kind, org)` 至多一条——`Effect::AddAffiliation`
/// 的 `apply` 一支保证了这点，见它的文档。
fn standing_towards(agent: &Agent, faction: ll_core::ident::WorldId) -> i32 {
    agent
        .affiliations
        .iter()
        .find(|affiliation| {
            affiliation.kind == AffiliationKind::Faction
                && affiliation.org == OrgRef::Instance(faction)
        })
        .map_or(0, |affiliation| affiliation.standing)
}
