//! 树木结算需要的那三个索引，以及一条窄接口。
//!
//! # 这个模块为什么存在
//!
//! `crate::resolve::tree::resolve_tend_tree` 要回答三个问题：
//!
//! 1. **哪一种 `TerrainKind` 是 `forest`**（树只长在森林上）；
//! 2. **砍下来的木料是哪件物品**；
//! 3. **采下来的种子是哪件物品**。
//!
//! 三个答案都是 `ll_core::ident::ContentIndex`，而 `ContentIndex` 的真相源
//! 是 `ll_mod::registry::Registry`——`ll-sim` **不能依赖 `ll-mod`**
//! （依赖方向，规格 §5）。因此走与
//! [`crate::skill::SkillCatalog`]/[`crate::quest::QuestCatalog`]/
//! [`crate::dialogue::ContentIdLookup`] **完全相同**的依赖倒置手法：本 crate
//! 只声明接口，实现住在上游。这不是一个新发明的模式。
//!
//! # 为什么不给 `resolve_*` 那一串入口加参数
//!
//! `crate::resolve` 已经有一串 `resolve_with_skills_traits_pools_items_and_formulas`
//! 那样的入口，每加一路目录就要在每个入口上多传一个。
//! [`crate::catalogs::ResolveCatalogs`] 就是为此存在的那一束——本路挂进去
//! 即可，既有入口一个都不用改签名。
//!
//! # 不接这一路会怎样
//!
//! [`NoTrees`] 三条查询全返回 `None` ⇒ [`Intent::TendTree`](crate::intent::Intent::TendTree)
//! **零效果**。与仓库既有的空目录纪律一致（「不接等于这条玩法不存在」），
//! 而**不是** panic、也不是拿一个占位索引冒充。

use ll_core::ident::ContentIndex;

/// 树木结算需要的三个内容索引来源。
///
/// 三条查询都返回 `Option`（ADR 0015：「结构合法」与「已注册」是两件事）
/// ——本体内容缺了任何一条，树的那条玩法就整个不成立，`resolve` 侧因此
/// 直接返回空效果，不做任何降级猜测。
pub trait TreeCatalog {
    /// 哪一种地形是森林——树的派生层只在它上面长树。
    ///
    /// 返回裸 [`ContentIndex`] 而不是 `ll_world::terrain::TerrainKind`：
    /// 本 trait 的实现方（`ll-game`）手上是 `BaseTerrainIds`，调用方
    /// （`resolve`）要的是 `TerrainKind`，两者之间只差一次
    /// `TerrainKind::from_index`。**用最窄的那个类型当接口**，与
    /// [`crate::dialogue::ContentIdLookup`] 同一条取舍。
    fn forest_terrain(&self) -> Option<ContentIndex>;

    /// 砍倒一棵树产出的木料是哪件物品。
    fn timber(&self) -> Option<ContentIndex>;

    /// 采一次果产出的树种是哪件物品，也是培植消耗的那件。
    ///
    /// **一件而不是两件**：采 → 得种 → 种 → 长树 → 砍 → 得木料，闭环。
    /// 「果子」与「种子」在本批里是同一件东西——给它们各开一件物品会
    /// 造出一件没有任何消费者的果子（吃它需要食物系统那一路，不在本批
    /// 范围），那正是「声明了没人读」。
    fn tree_seed(&self) -> Option<ContentIndex>;
}

/// 不接树木这一路时的空实现——三条查询全 `None`。
///
/// 效果是 [`Intent::TendTree`](crate::intent::Intent::TendTree) 恒产出空
/// 效果，即「这个世界里没有树可以砍」。这与
/// [`crate::skill::NoSkills`]/[`crate::quest::NoQuests`] 是同一档的空对象。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTrees;

impl TreeCatalog for NoTrees {
    fn forest_terrain(&self) -> Option<ContentIndex> {
        None
    }

    fn timber(&self) -> Option<ContentIndex> {
        None
    }

    fn tree_seed(&self) -> Option<ContentIndex> {
        None
    }
}

/// 对一棵树做什么——[`Intent::TendTree`](crate::intent::Intent::TendTree)
/// 的载荷。
///
/// # 为什么是一个意图带一个动作枚举，而不是三个意图
///
/// 三条路共用**同样的前置**（发起者活着、够得着、目标格是森林），也共用
/// 同样的「一次操作消耗一个回合」计费。拆成三个 `Intent` 变体等于把那三
/// 条闸门抄三份，而抄出来的三份**会各自漂移**——ADR 0021 点名要拦的正是
/// 这个形状。[`Intent::Trade`](crate::intent::Intent::Trade) 用
/// `direction: TradeDirection` 表达买/卖是同一条先例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TreeAction {
    /// 砍倒：树消失，产出按树种数量不同的木料。
    Fell,
    /// 采果：树留着，产出一颗树种，果子要过
    /// [`ll_world::tree::FRUIT_REGROW_TICKS`] 才重新长好。
    Harvest,
    /// 培植：消耗一颗树种，在一格没有树的森林上种出一棵。
    ///
    /// **长出什么树由那块地的气候决定，不由种子决定**
    /// （`ll_world::tree::derived_species_at`）——这与「分布由气候决定」
    /// 是同一条规则的两次应用，不是两套逻辑。
    Plant,
}
