//! 制作结算侧接口——[`RecipeRule`]/[`RecipeIngredient`]/[`RecipeCatalog`]，
//! 落地 `knowledge/design/crafting-system.md` 三节的 `resolve` 侧最小
//! 视图。
//!
//! # 依赖倒置：trait 在这里，实现在 `ll-mod`
//!
//! 真正的 `RecipeDef`/`RecipeTable`/`RecipeCategoryDef`/
//! `RecipeCategoryTable` 定义在下游的 `ll-mod`，`ll-sim` 不能反过来
//! 依赖它（依赖方向：`ll-core ← ll-world ← ll-sim ← ll-script ←
//! ll-mod`）。与 [`crate::skill::SkillCatalog`]/[`crate::item::ItemCatalog`]/
//! [`crate::quest::QuestCatalog`] 同一套既有手法：本模块只声明
//! `crate::resolve::resolve_craft` 真正要问的两个问题——「这条配方长
//! 什么样」与「这个类别要求哪些副职」——真正的实现在 `ll-mod` 侧补上。
//!
//! # 为什么 [`RecipeIngredient`] 定义在这里而不是 `ll-mod`
//!
//! 它同时出现在两侧：`ll-mod` 的 `RecipeDef.ingredients`（注册期存储）
//! 与本模块的 `RecipeRule.ingredients`（结算期读取）。定义在上游、由
//! 下游 `pub use` 回去，是 `ll_mod::trait_def` `pub use`
//! [`crate::resource_pool::ResourcePoolGrant`] 已经确立的既有先例
//! （见其模块文档）——反过来（定义在 `ll-mod`）会让本模块无法表达
//! `RecipeRule.ingredients` 的类型，只能复制一份必然漂移的副本。
//!
//! # 为什么类别闸门是「一个方法」而不是把类别定义整条搬过来
//!
//! `RecipeCategoryDef` 还有一个 `display_name_key` 字段（制作界面按
//! 类别分栏展示时的标题），而 `resolve_craft` 从不读它——与
//! [`crate::item::ItemRule`]「只收敛 resolve 真正要读的字段」是同一条
//! 既有纪律，不整条转发定义。

use ll_core::ident::ContentIndex;

/// 一条配方需要的一味食材与它的数量。
///
/// `count` 恒 ≥ 1（注册期校验，见 `ll_mod::recipe::RecipeTable::define`）
/// ——数量为零的食材条目没有意义，那是「不需要这味食材」，正确的表达
/// 是不写这一条。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipeIngredient {
    /// 指向 `ItemDef` 的内容索引——要消耗哪种物品。
    pub item: ContentIndex,
    /// 需要的数量，恒 ≥ 1。
    pub count: u32,
}

/// `resolve` 侧需要的一条配方定义的最小只读视图。
///
/// 与 `ll_mod::recipe::RecipeDef` 的差别只有一个：不含 `id` 与
/// `display_name_key`（结算不读展示名，理由同 [`crate::item::ItemRule`]
/// 不含 `ItemDef.display_name_key`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeRule {
    /// 这条配方属于哪一类——指向配方类别表，恒必填。
    /// `crate::resolve::resolve_craft` 拿它去问
    /// [`RecipeCatalog::category_required_subclasses`]。
    pub category: ContentIndex,
    /// 需要的食材与各自数量，恒非空（注册期校验）。
    ///
    /// 是 `Vec` 而不是 `BTreeMap<ContentIndex, u32>`：食材校验要逐条
    /// 遍历，`Vec` 本身保序，不依赖任何哈希表遍历顺序（约束 C5）；
    /// 且同一味食材在一条配方里出现两次是内容作者的笔误，不是需要
    /// 用 map 键去重来兜住的正常情形。
    pub ingredients: Vec<RecipeIngredient>,
    /// 产出物品，指向 `ItemDef`。
    pub product: ContentIndex,
    /// 产出数量，恒 ≥ 1。
    pub product_count: u32,
    /// 必须站在哪件**家具**上才能制作，指向物品表（一件
    /// [`crate::item::ItemRule::furniture`] 为真的物品）；`None` = 随地
    /// 可做。
    ///
    /// 判定是「站在这格上」（脚下那一格的
    /// [`ll_world::state::WorldState::ground_items`] 里摆着的那件家具是
    /// 不是它），不是「站在旁边」——见设计文档六节：相邻判定会引入
    /// 「多个相邻工作台算哪个」这类不必要的问题。
    ///
    /// 家具层批次之前本字段指向**地形表**、判定走
    /// [`ll_world::state::WorldState::terrain_at`]，完整的更正理由见
    /// `ll_mod::recipe::RecipeDef::required_station` 文档。
    pub required_station: Option<ContentIndex>,
    /// 必须装备着哪件物品才能制作，指向 `ItemDef`；`None` = 徒手可做。
    ///
    /// 判定是「装备着**且耐久未归零**」——见
    /// `crate::resolve::resolve_craft` 文档「坏掉的工具不算装着」一节。
    pub required_tool: Option<ContentIndex>,
    /// 这条配方是否必须先「学会」才做得出来（配方发现批次）——项目
    /// 所有者裁定「菜谱就是通过随机丢入东西煮获取或者阅读书籍的时候
    /// 获取」的落点，**推翻了** `food-and-cooking-system.md` 五节
    /// 「菜谱全部已知、不设解锁门槛」那条裁定（更正记录见该文档五节
    /// 末尾）。
    ///
    /// `false`（既有内容的默认值）= 人人天生会做，`crate::resolve::resolve_craft`
    /// 完全不看 `Agent::known_recipes`，与本字段落地之前逐字节等价；
    /// `true` = 多一道闸门，行动者的 `Agent::known_recipes` 必须含有
    /// 这条配方。
    ///
    /// # 为什么是**逐配方**的开关，不是全局一刀切
    ///
    /// 「烤一块肉」和「配一剂返魂药」不该共用同一条获取难度。所有者的
    /// 裁定说的是「菜谱要靠发现」这件事必须**存在**，不是「每一条配方
    /// 都必须先发现」——后者会让新角色连生火烤肉都做不了，而这不是任何
    /// 一方要过的玩法。把开关做成内容数据，两种都表达得出来，且既有
    /// 内容一个字不改就保持原状（默认 `false`）。
    ///
    /// # 与副职类别闸门（[`RecipeCatalog::category_required_subclasses`]）
    /// 的分工
    ///
    /// 类别闸门问「你**有没有资格**做这一类」（你是不是工匠），本字段
    /// 问「你**知不知道**这一张图纸」。两者正交，`resolve_craft` 分两步
    /// 各判各的——`crafting-system.md` 十四节①那张表把这两件事拆开时
    /// 用的就是这个分界，本字段是那一行「配方解锁」的落地。
    pub requires_discovery: bool,
}

/// `resolve` 依赖的最小「配方定义来源」接口，见模块文档「依赖倒置」
/// 一节。
pub trait RecipeCatalog {
    /// 查询一条配方定义；未注册的索引返回 `None`（ADR 0015）。
    fn recipe(&self, recipe: ContentIndex) -> Option<RecipeRule>;

    /// 这个配方类别要求持有哪些副职才能使用——**any-of 语义**：
    /// 返回列表非空时，行动者的 `Agent::subclasses` 只要与它有任意
    /// 一个交集就放行。
    ///
    /// **空列表 = 不设闸门，人人可做**，这也是「查不到这个类别」时的
    /// 返回值：与 [`crate::item::NoItems`]「查不到目录不该表现成内容
    /// 异常」同一条降级方向——一个查不到的类别不应该让全部配方变得
    /// 谁都做不了。真正拦住「配方指向一个不存在的类别」这类内容错误
    /// 的是注册期校验（`register-recipe` 要求 `category-id` 已注册）
    /// 与装载后的跨表引用校验（`ll_mod::content_audit`），不是结算期。
    fn category_required_subclasses(&self, category: ContentIndex) -> Vec<ContentIndex>;

    /// 某个配方类别下已注册的全部配方，**按索引升序**（约束 C5：这个
    /// 顺序会参与 `crate::resolve::resolve_experiment` 的候选筛选与
    /// 随机抽取，必须确定）。
    ///
    /// # 为什么是「返回全部、由 `resolve` 自己筛」
    ///
    /// 与 [`crate::quest::QuestCatalog::kill_count_quests`]/
    /// [`crate::subclass::SubclassUnlockCatalog::craft_unlocks`] 同一个
    /// 既有手法：一个类别下的配方数是「内容作者写了几条」这个小量级，
    /// 一次线性过滤远比给注册表维护一份「按类别 + 按食材」的反向索引
    /// 便宜——后者要为一个每局只会触发几十次的动作，付出装载期建索引
    /// 与运行期维护一致性的代价。
    ///
    /// 查不到这个类别时返回空列表（不是错误）——理由同
    /// [`Self::category_required_subclasses`]：ADR 0015，未注册的索引
    /// 当作「没有」，内容错误由注册期校验与 `ll_mod::content_audit`
    /// 拦，不由结算期拦。
    fn recipes_in_category(&self, category: ContentIndex) -> Vec<ContentIndex>;
}

/// 空配方目录：查询任何配方恒返回 `None`，任何类别恒返回空闸门——
/// 理由同 [`crate::skill::NoSkills`]。
///
/// 效果是 `Intent::Craft` 一律静默无效（`resolve_craft` 第 2 步查不到
/// 配方就返回空效果）：没接目录的调用方本来就没有任何配方内容可做。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRecipes;

impl RecipeCatalog for NoRecipes {
    fn recipe(&self, _recipe: ContentIndex) -> Option<RecipeRule> {
        None
    }

    fn category_required_subclasses(&self, _category: ContentIndex) -> Vec<ContentIndex> {
        Vec::new()
    }

    fn recipes_in_category(&self, _category: ContentIndex) -> Vec<ContentIndex> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    #[test]
    fn 空配方目录查询任意配方恒返回none() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:roast_meat_recipe").unwrap());

        // Act & Assert
        assert_eq!(NoRecipes.recipe(index), None);
    }

    #[test]
    fn 空配方目录里任何类别都查不出配方() {
        // 与上面两条同一条降级方向：空目录不该表现成「某个类别下有
        // 一堆配方，只是查不到定义」——`resolve_experiment` 因此在没接
        // 目录时恒产出空效果，而不是对着一串幽灵索引掷骰。
        // Arrange
        let mut interner = Interner::new();
        let category = interner.intern(NamespacedId::parse("lostland:cooking").unwrap());

        // Act & Assert
        assert!(NoRecipes.recipes_in_category(category).is_empty());
    }

    #[test]
    fn 空配方目录的类别闸门恒为空即人人可做() {
        // 这一条守的是降级方向：空目录不该表现成「全部类别都锁死」。
        // Arrange
        let mut interner = Interner::new();
        let category = interner.intern(NamespacedId::parse("lostland:forging").unwrap());

        // Act & Assert
        assert!(NoRecipes.category_required_subclasses(category).is_empty());
    }
}
