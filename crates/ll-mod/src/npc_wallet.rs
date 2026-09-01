//! NPC 出生时口袋里有多少钱。
//!
//! 落地**项目所有者裁定第 4 条**
//! （`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节）：
//!
//! > **NPC 初始钱包不能是 0**，按职业量级给、从据点人口派生。
//! > 现状 `ll_mod::roster::build_npc_agent` 写死 `wallet: 0` ⇒
//! > **玩家只能买不能卖，交易落地即残废**。触及货币守恒，落地时要在
//! > 文档里写明取值理由。
//!
//! 计划文档 `docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md`
//! 四节。
//!
//! # 为什么单独一个模块
//!
//! [`crate::roster`] 已经越过规格 §13 的 800 行上限（在行数棘轮快照
//! 里），而门禁的原话是「要往这个文件加东西，先把要加的那部分**按职责
//! 拆进一个新模块**」。本模块与 `roster` 的接口只有一个纯函数，拆开
//! 零代价——与 `ll_sim::ownership` 从 `resolve.rs` 拆出去是同一条纪律。
//!
//! # 通胀这笔账，如实记在这里
//!
//! `knowledge/design/agent-goals-and-economy.md` 四节有「货币守恒与回收
//! 汇」一节，而**世界生成期这一次性发放是凭空造钱**。
//!
//! 要分清的是两件事：
//!
//! - **交易本身是守恒的**——`ll_sim::resolve` 的 `Intent::Trade` 一支
//!   产出的两条 `AdjustWallet` 一正一负、绝对值相同，没有第三方抽成。
//! - **不守恒的只有本模块**：每物化一个 NPC 就凭空多出一笔钱。它是一个
//!   **已知的**通胀源，记在 P9 的账上；真正的回收汇（税、损耗、商队
//!   成本）属于那一批。
//!
//! 在此之前，`wallet: 0` 那条「更守恒」的旧行为的代价是交易整条是死的
//! （玩家只能买不能卖，而玩家出生时也是 0）——所有者裁定的正是这一点。

use ll_core::ident::ContentIndex;

use crate::roster::SettlementRoles;

/// 每位居民给这座据点里的一个 NPC 贡献多少钱（最小货币单位）。
///
/// # 量纲从哪来
///
/// `ll_world::entity::Agent::wallet` 的单位是「最小货币单位」，而
/// `ll_mod::item::ItemDef::base_price` 那个 `Milli` 的最小单位就是它
/// （见 `ll_sim::item::ItemRule::base_price` 文档）。因此可以直接对着
/// 本体内容读数量级：`mods/lostland/items.json5` 里一份烤肉 900、
/// 一把像样的武器 40000~55000。
///
/// 取 200 之后，一座 40 人的据点：普通居民 8000（约九份口粮，买不起
/// 武器）、有产出可卖的职业 24000、管理者 40000（**刚好一把武器，
/// 买不起两把**）。这一档同时满足两条：**玩家连一把武器都卖得掉**
/// （裁定的目的——`wallet: 0` 的旧行为让交易整条是死的），而且
/// **不足以让玩家靠倒卖致富**（NPC 的钱是有限的，卖光就没了，而且要卖
/// 得掉一把武器得先找到一座够大的据点的管理者）。
///
/// 更小的据点按比例更穷，这正是「从据点人口派生」那半句裁定的意思：
/// 一个二十户的农庄拿不出四万块，那是应该的。
///
/// **这是一个占位量级**，与 `ll_sim::trade::trade_price` 那条占位公式
/// 同批、同理由：它将来会被 `agent-goals-and-economy.md` 的经济系统
/// 替换，届时「一个 NPC 有多少钱」应当由他的生产与交易历史推出来，
/// 而不是由出生那一刻的一次乘法定死。
pub const NPC_WALLET_PER_RESIDENT: i64 = 200;

/// 据点管理者的档位——他掌着整座据点的账。
pub const NPC_WALLET_TIER_STEWARD: i64 = 5;

/// **有产出可卖**的职业的档位：农夫、猎户、屠夫、铁匠、渔夫、牧羊人、
/// 石匠。他们手上过货，因此手上有钱。
pub const NPC_WALLET_TIER_TRADER: i64 = 3;

/// 其余所有人的档位——守卫、民兵，以及职业索引压根没匹配上任何一格的
/// 那些（内容里一条职业都没装载时 `NpcProfile::profession` 是
/// `ContentIndex::default()`，见 [`crate::roster::NpcProfile`] 文档）。
///
/// **不是 0**：所有者裁定的第一句就是「不能是 0」。一个守卫也该买得起
/// 一顿饭。
pub const NPC_WALLET_TIER_DEFAULT: i64 = 1;

/// 这个 NPC 出生时口袋里有多少钱。
///
/// ```text
/// wallet = 据点人口 × NPC_WALLET_PER_RESIDENT × 职业档位
/// ```
///
/// # 「职业量级」的判据是 [`SettlementRoles`]，**不是一个新的内容字段**
///
/// 「哪个 `ContentIndex` 是管理者、哪个是铁匠」这件事今天已经有唯一的
/// 真相源了，就是 [`SettlementRoles`]——它是名册派生本来就要用的那张
/// 表，`ll_mod::roster::build_npc_agent` 手上现成有一份。
///
/// 另一条路是给 `ClassAttrs` 加一个 `wealth_tier` 内容字段。**否决**：
/// 那要动内容 schema、动内容哈希、动本体十三条职业的每一条，换来的能力
/// 是「mod 作者能给自己的职业定财富档」——而今天没有任何一条内容需要
/// 它（YAGNI）。反转成本：加那个字段时，本函数改成读它，档位常量作废。
///
/// # 人口为零时是零
///
/// 废墟与空据点产出空名册（[`crate::roster::settlement_roster`]），
/// 根本走不到物化这一步。这里不为那种情形另设下界——一个「人口 0 的
/// 据点里的居民」本来就不该存在，凭空给他一笔钱只会掩盖上游的错。
///
/// # 顺序确定（约束 C5）
///
/// 全程只做 `Option<ContentIndex>` 的相等比较，不碰任何哈希容器。
pub fn npc_initial_wallet(
    profession: ContentIndex,
    roles: &SettlementRoles,
    population: u32,
) -> i64 {
    i64::from(population) * NPC_WALLET_PER_RESIDENT * wallet_tier(profession, roles)
}

/// 这个职业属于哪一档，见三个 `NPC_WALLET_TIER_*` 常量。
///
/// 一个职业索引理论上可以同时等于两格（内容里两个角色指向同一条职业
/// 定义）。判定顺序因此是**从高到低**且**第一条命中就返回**——这不是
/// 形式要求：管理者若恰好也被登记成铁匠，他该拿管理者那一档。
fn wallet_tier(profession: ContentIndex, roles: &SettlementRoles) -> i64 {
    if roles.steward == Some(profession) {
        return NPC_WALLET_TIER_STEWARD;
    }
    let 有产出可卖 = [
        roles.blacksmith,
        roles.butcher,
        roles.farmer,
        roles.fisher,
        roles.hunter,
        roles.mason,
        roles.shepherd,
    ];
    if 有产出可卖.contains(&Some(profession)) {
        return NPC_WALLET_TIER_TRADER;
    }
    NPC_WALLET_TIER_DEFAULT
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    fn 索引(raw: &str, interner: &mut Interner) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("固定字面量恒合法"))
    }

    /// **全部索引必须出自同一个 `Interner`**——两个各自新建的注册表都
    /// 从 0 号开始发号，「陌生职业」会与「管理者」撞成同一个索引，那时
    /// `不认识的职业落默认档` 测的是一个假前提。
    fn 一份注册表() -> Interner {
        Interner::new()
    }

    /// 三格各不相同的一份角色表。
    fn 角色表() -> (SettlementRoles, ContentIndex, ContentIndex, ContentIndex) {
        let mut interner = 一份注册表();
        let steward = 索引("lostland:steward", &mut interner);
        let blacksmith = 索引("lostland:blacksmith", &mut interner);
        let guard = 索引("lostland:guard", &mut interner);
        // `SettlementRoles` 有几个模块私有字段（资源亲和那几张表），
        // 本模块构造不出完整字面量——从 `default()` 起手再填公开的那三格。
        let mut roles = SettlementRoles::default();
        roles.steward = Some(steward);
        roles.blacksmith = Some(blacksmith);
        roles.guard = Some(guard);
        (roles, steward, blacksmith, guard)
    }

    /// 三档真的互不相同，且**都不是 0**——裁定的第一句就是「不能是 0」。
    ///
    /// 故意改坏的反例（本批实测）：把 `NPC_WALLET_TIER_DEFAULT` 改回 0，
    /// 本条当场红。
    #[test]
    fn 三档钱包互不相同且都不是零() {
        // Arrange
        let (roles, steward, blacksmith, guard) = 角色表();

        // Act
        let 管理者 = npc_initial_wallet(steward, &roles, 40);
        let 铁匠 = npc_initial_wallet(blacksmith, &roles, 40);
        let 卫兵 = npc_initial_wallet(guard, &roles, 40);

        // Assert
        assert!(卫兵 > 0, "所有者裁定：初始钱包不能是 0");
        assert!(
            管理者 > 铁匠 && 铁匠 > 卫兵,
            "档位单调：{管理者} > {铁匠} > {卫兵}"
        );
        assert_eq!(卫兵, 40 * NPC_WALLET_PER_RESIDENT * NPC_WALLET_TIER_DEFAULT);
    }

    /// **从据点人口派生**：同一个职业，人多的据点更有钱。
    ///
    /// 故意改坏的反例（本批实测）：把公式里的 `population` 那一项去掉，
    /// 本条当场红。
    #[test]
    fn 人口越多的据点里同一个职业越有钱() {
        // Arrange
        let (roles, _steward, blacksmith, _guard) = 角色表();

        // Act & Assert
        assert!(
            npc_initial_wallet(blacksmith, &roles, 80) > npc_initial_wallet(blacksmith, &roles, 20)
        );
    }

    /// 没匹配上任何一格的职业落默认档，**不 panic、不返回 0**。
    #[test]
    fn 不认识的职业落默认档() {
        // Arrange
        let (roles, steward, blacksmith, guard) = 角色表();
        // 与三格同出一个注册表，否则会与它们撞号（见 `一份注册表` 文档）。
        let mut interner = 一份注册表();
        for raw in ["lostland:steward", "lostland:blacksmith", "lostland:guard"] {
            索引(raw, &mut interner);
        }
        let 陌生职业 = 索引("mymod:tinker", &mut interner);
        // 先断言前提成立：它真的不是那三格里的任何一个。
        assert!(![steward, blacksmith, guard].contains(&陌生职业));

        // Act & Assert
        assert_eq!(
            npc_initial_wallet(陌生职业, &roles, 10),
            10 * NPC_WALLET_PER_RESIDENT * NPC_WALLET_TIER_DEFAULT
        );
    }

    /// 一个 40 人据点的管理者，买得起一把武器（本体武器 40000~55000）
    /// 但买不起两把——[`NPC_WALLET_PER_RESIDENT`] 文档里那句量纲论证的
    /// 可执行版本。
    ///
    /// **它守的是量纲，不是具体数字**：改数值时这一条会红，那正是它的
    /// 用途——逼改数值的人重新读一遍那段量纲论证。
    #[test]
    fn 一座中等据点的管理者买得起一把武器买不起两把() {
        // Arrange
        let (roles, steward, _blacksmith, _guard) = 角色表();
        // 本体最贵的那几件武器落在 40000~55000（`mods/lostland/items.json5`）。
        let 一把武器 = 40_000;

        // Act
        let 钱包 = npc_initial_wallet(steward, &roles, 40);

        // Assert
        assert!(钱包 >= 一把武器, "买得起一把：{钱包}");
        assert!(钱包 < 一把武器 * 2, "买不起两把：{钱包}");
    }
}
