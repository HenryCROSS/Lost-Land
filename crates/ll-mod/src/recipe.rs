//! 配方注册表：`register-recipe` 的存储落点（制作系统批次，
//! `knowledge/design/crafting-system.md` 三、十节）。
//!
//! # 一套机制、四类配方
//!
//! 烹饪/锻造/裁缝/炼金**共用这一张表**，四类的差别全部落在数据上：
//! [`RecipeDef::category`]（谁能做）、[`RecipeDef::ingredients`]
//! （用什么）、[`RecipeDef::required_station`]（在哪做）、
//! [`RecipeDef::required_tool`]（拿什么做）。
//!
//! 设计文档二节用 ADR 0021 的双向判据独立复核过这条统一：
//! `ll_sim::resolve::resolve_craft` 要走的「查配方 → 校验前置 → 逐条
//! 校验食材 → 逐条产出扣减 → 合并成品」这串步骤里，**没有任何一步会
//! 因为「这是锻造不是烹饪」而不同**。拆成四套会把「校验食材是否齐全、
//! 按数量扣减、合并产出」复制四份——食材不足时的静默返回、堆叠上限
//! 溢出、耐久参与合并判据，每一条都是容易写错且写错了测试未必发现的
//! 细节。
//!
//! 将来若某一类制作真的需要一段**结构不同**的结算（例如锻造做成
//! 「加热→锻打→淬火」的跨回合半成品流程），那时候拆分才有理由——理由
//! 会是「发现了不能共用的算法」，不是「本来就该分开」。
//!
//! # 列式存储
//!
//! 照抄 [`crate::item::ItemTable`]/[`crate::subclass::SubclassTable`]
//! 已验证的手法（ADR 0016/0017：声明式、注册期物化、运行期查表）：
//! 每字段一列 + 一份 `defined` 位图，下标是全局 `ContentIndex` 号段的
//! 一部分。与 [`crate::recipe_category::RecipeCategoryTable`] 走
//! `BTreeMap` 的取舍不同，理由见那张表的模块文档。
//!
//! # 同一成品刻意允许多条配方
//!
//! [`RecipeTable::define`] **不**在 [`RecipeDef::product`] 上加唯一性
//! 约束：铁剑可以有「铁锭×2」和「废铁×3 + 木炭」两条路，粗铁匠与精
//! 铁匠的配方可以产出同一件东西。这是零成本的变化度——代价真的是零，
//! 只需要在这里**不写**那条约束。这是刻意的，不是遗漏。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::craft::{RecipeCatalog, RecipeRule};

pub use ll_sim::craft::RecipeIngredient;

use crate::recipe_category::RecipeCategoryTable;

/// 单条配方声明：本体与 mod 注册配方时共用的同一个输入形状。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeDef {
    /// 命名空间标识符，例如 `lostland:iron_sword_recipe`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串。
    pub display_name_key: NamespacedId,

    /// 这条配方属于哪一类——指向
    /// [`crate::recipe_category::RecipeCategoryTable`]。
    ///
    /// **恒必填，不是 `Option`**：类别是副职闸门唯一的挂载点，一条没有
    /// 类别的配方在那里无处安放；且「不属于任何类别」这个语义已经由
    /// 「类别自身不设副职闸门」（[`crate::recipe_category::RecipeCategoryDef::required_subclasses`]
    /// 为空）完整表达，不需要第二种表达方式。
    ///
    /// # 为什么是显式字段，不从 `ingredients` 的材质反推
    ///
    /// 三条理由（设计文档四节完整论证）：①`ItemDef` 上**没有任何材质
    /// 字段**，推导要先发明一个 `material` 字段再写一段分类算法——为了
    /// 避免一个数据字段而发明一段算法，方向反了；②混合材料的配方没有
    /// 定义良好的答案（皮甲用皮革配麻线）；③mod 作者无法预测——本体
    /// 任何一次分类规则调整都会静默改变所有第三方 mod 的配方归属。
    pub category: ContentIndex,

    /// 需要的食材与各自数量——**恒非空**，注册期拒绝空列表
    /// （[`RecipeError::EmptyIngredients`]）。
    pub ingredients: Vec<RecipeIngredient>,

    /// 产出物品，指向 [`crate::item::ItemTable`]。
    ///
    /// **不校验它是不是一件已定义的物品**——只校验索引本身已 intern，
    /// 理由同 `RaceTable::add_trait_grant`：跨表强校验会让注册顺序产生
    /// 不必要的耦合。装载全部完成后的跨表引用完整性由
    /// [`crate::content_audit`] 统一兜住，那才是这类校验的正确位置。
    pub product: ContentIndex,

    /// 产出数量，恒 ≥ 1（[`RecipeError::ZeroProductCount`]）。
    ///
    /// 单个而不是 `Vec<RecipeProduct>`：副产物/多产出没有真实驱动，
    /// 且因为配方数据**不进存档**（内容表是装载期产物），将来若要放宽
    /// 成多产出是一次纯粹的字段加宽，零迁移代价。不为「以后可能要」
    /// 预付复杂度。
    pub product_count: u32,

    /// 必须站在哪件**家具**上才能制作，指向物品表（一件
    /// [`crate::item::ItemDef::furniture`] 为真的物品）；`None` = 随地
    /// 可做。
    ///
    /// # 家具层批次：从地形改指家具
    ///
    /// 本字段此前指向**地形表**（「必须站在哪种地形上」）。项目所有者
    /// 裁定「家具也应该算是一种可以放在地形上的可交互物品」之后，
    /// 场地改成「脚下那一格摆着的那件家具」——判定见
    /// `ll_sim::resolve::resolve_craft` 第 5 步（`furniture_at`，此前是
    /// `terrain_at`）。
    ///
    /// 推着这次改动的是本体内容里一处**明知的将就**：三条锻造配方当时
    /// 只能拿 `lostland:floor_stone`（石地面）冒充铁匠铺，
    /// `mods/lostland/crafting.json5` 的注释逐字写着「真正该当场地的是
    /// 炉子或铁砧那样的家具」。设计文档的更正记录见
    /// `knowledge/design/crafting-system.md` 六节末尾。
    ///
    /// **必须真的是家具**：`furniture_at` 只认带标志的那一条，指向一件
    /// 普通物品的配方会变成永远做不出来。这一条由结算侧兜住而不是
    /// `crate::content_audit`——见 `inspect_recipe` 里的注释（引用违规
    /// 那个类型表达得了「落错表」，表达不了「表对了但标志不对」）。
    ///
    /// # 为什么现在就有这个字段
    ///
    /// `food-and-cooking-system.md` 四节曾把场地需求判为 YAGNI，
    /// 设计文档六节**明确推翻了那条裁定**，理由是四类制作合并成一套
    /// 机制这个决定本身产生了当时不存在的需求：去掉场地这一维，锻造
    /// 与烹饪在系统里的唯一差别就只剩「食材不同 + 归不同副职管」——
    /// 那不是四类制作，那是一类制作贴了四个标签。铁砧/织机/炼金台
    /// 恰恰是让四类在玩法上真的不同的东西。
    ///
    /// # 「工作台必须站得上去」现在是引擎强制的
    ///
    /// 判定仍然是「站在这格上」。此前这条只能是一句写给内容设计的
    /// 纪律（「工作台地形必须可通行，别把它做成一个挡路的铁砧方块」）；
    /// 家具层落地后它成了**结构上不可能违反**的事：家具放不到
    /// `blocks_move` 的格子上（`ll_sim::resolve::resolve_drop` 的放置
    /// 前置），因此「玩家站不上去的工作台」摆都摆不出来。
    pub required_station: Option<ContentIndex>,

    /// 必须装备着哪件物品才能制作，指向 [`crate::item::ItemTable`]；
    /// `None` = 徒手可做。
    ///
    /// # 为什么是「装备着」而不是「背包里有」
    ///
    /// ①**有代价**——装备着意味着占一个槽位，拿着锤子就腾不出那只手
    /// 拿盾；「背包里有」毫无代价，等于只是一道「你买过这个道具吗」的
    /// 检查。②和「工具」这个词的物理直觉一致。③**它是采矿/种植将来
    /// 唯一的正确接入点**：将来的 `resolve_mine` 必然要问「他手里拿着
    /// 镐子吗」，这里定下的判定形状会被那个批次原样复用，现在选错将来
    /// 要么跟着错、要么两套判定并存。
    ///
    /// 「坏掉的工具算不算装着」由 `ll_sim::resolve::resolve_craft` 回答
    /// （不算，见其文档），不是本字段的语义。
    pub required_tool: Option<ContentIndex>,

    /// 这条配方是否必须先「发现」才做得出来（配方发现批次）——项目
    /// 所有者裁定「菜谱就是通过随机丢入东西煮获取或者阅读书籍的时候
    /// 获取」的落点，完整语义见 [`ll_sim::craft::RecipeRule::requires_discovery`]。
    ///
    /// `false`（默认值，也是既有全部内容的取值）= 人人天生会做，
    /// `resolve_craft` 完全不看 `Agent::known_recipes`。
    ///
    /// # 为什么不是 `register-recipe` 的参数，走 `set_requires_discovery` 追加
    ///
    /// 与 [`Self::required_station`]/[`Self::required_tool`] 同一条既有
    /// 先例（`register-recipe` 的七参数签名不能改参数个数，会破坏仓库
    /// 里已有的真实 mod 脚本）——脚本层对应函数是
    /// `recipe-requires-discovery!`（`crafting.json5`），
    /// Rust 层对应方法是 [`RecipeTable::set_requires_discovery`]。
    /// **覆盖，不是追加**：一条配方只有一个「要不要先学会」的答案。
    pub requires_discovery: bool,
}

/// [`RecipeTable::define`] 实际存进列式存储的属性子集——不含 `id`，
/// 理由同 [`crate::subclass::SubclassAttrs`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeAttrs {
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
    /// 配方类别，恒必填。
    pub category: ContentIndex,
    /// 食材表，恒非空。
    pub ingredients: Vec<RecipeIngredient>,
    /// 产出物品。
    pub product: ContentIndex,
    /// 产出数量，恒 ≥ 1。
    pub product_count: u32,
}

/// 一次配方查询命中的完整结果，理由同 [`crate::subclass::SubclassView`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipeView<'a> {
    /// 指向 Fluent 本地化键。
    pub display_name_key: &'a NamespacedId,
    /// 配方类别。
    pub category: ContentIndex,
    /// 食材表。
    pub ingredients: &'a [RecipeIngredient],
    /// 产出物品。
    pub product: ContentIndex,
    /// 产出数量。
    pub product_count: u32,
    /// 场地前置。
    pub required_station: Option<ContentIndex>,
    /// 工具前置。
    pub required_tool: Option<ContentIndex>,
    /// 是否必须先发现，见 [`RecipeDef::requires_discovery`]。
    pub requires_discovery: bool,
}

/// 配方注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeError {
    /// 同一个内容索引被定义了两次。
    DuplicateDefinition(ContentIndex),
    /// 食材列表为空——「不需要任何材料就能凭空变出东西」不是配方。
    EmptyIngredients(ContentIndex),
    /// 某一味食材的数量为零。
    ZeroIngredientCount(ContentIndex),
    /// 产出数量为零。
    ZeroProductCount(ContentIndex),
    /// 想给一个从未 [`RecipeTable::define`] 过的配方追加场地/工具前置。
    UnknownRecipe(ContentIndex),
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecipeError::DuplicateDefinition(index) => {
                write!(f, "配方索引 {} 被重复定义", index.get())
            }
            RecipeError::EmptyIngredients(index) => {
                write!(f, "配方索引 {} 的食材列表为空", index.get())
            }
            RecipeError::ZeroIngredientCount(index) => {
                write!(f, "配方索引 {} 有一味食材的数量为零", index.get())
            }
            RecipeError::ZeroProductCount(index) => {
                write!(f, "配方索引 {} 的产出数量为零", index.get())
            }
            RecipeError::UnknownRecipe(index) => {
                write!(f, "配方索引 {} 尚未通过 register-recipe 注册", index.get())
            }
        }
    }
}

impl std::error::Error for RecipeError {}

/// 配方属性的列式存储，见模块文档「列式存储」一节。
#[derive(Debug, Default, Clone)]
pub struct RecipeTable {
    display_name_key: Vec<Option<NamespacedId>>,
    category: Vec<Option<ContentIndex>>,
    ingredients: Vec<Vec<RecipeIngredient>>,
    product: Vec<Option<ContentIndex>>,
    product_count: Vec<u32>,
    required_station: Vec<Option<ContentIndex>>,
    required_tool: Vec<Option<ContentIndex>>,
    /// 配方发现批次新增，见 [`RecipeDef::requires_discovery`]。
    requires_discovery: Vec<bool>,
    defined: Vec<bool>,
    /// 已注册配方的索引清单（配方发现批次新增）——[`Self::in_category`]
    /// 要枚举「一共登记了哪些配方」，而列式存储只有「按下标查属性」的
    /// 能力，没有把下标还原成 [`ContentIndex`] 的合法途径
    /// （`ContentIndex` 刻意不提供 `from_raw`：索引只能来自
    /// `ll_core::ident::Interner::intern`，见其文档）。与
    /// [`crate::quest::QuestTable`] 的同名字段是同一个手法、同一个理由。
    defined_ids: Vec<ContentIndex>,
}

impl RecipeTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上配方属性。
    ///
    /// 校验四条：不得重复定义、食材列表非空、每味食材数量 ≥ 1、
    /// 产出数量 ≥ 1。**刻意不校验的两条**：`product` 与各食材的
    /// `item` 是不是已定义的物品（跨表校验会让注册顺序产生耦合，见
    /// [`RecipeDef::product`] 文档），以及 `product` 的唯一性（同一成品
    /// 允许多条配方，见模块文档）。
    ///
    /// `category` 必须已注册这条校验**不在这里**——它需要
    /// [`RecipeCategoryTable`]，属于跨表校验，落在脚本注册函数
    /// （`crafting.json5`）那一层，与
    /// `register-trait-resource-pool` 的 `pool-id` 存在性校验同一条
    /// 既有先例。
    pub fn define(&mut self, index: ContentIndex, attrs: RecipeAttrs) -> Result<(), RecipeError> {
        if attrs.ingredients.is_empty() {
            return Err(RecipeError::EmptyIngredients(index));
        }
        if attrs.ingredients.iter().any(|item| item.count == 0) {
            return Err(RecipeError::ZeroIngredientCount(index));
        }
        if attrs.product_count == 0 {
            return Err(RecipeError::ZeroProductCount(index));
        }

        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.category.resize(new_len, None);
            self.ingredients.resize(new_len, Vec::new());
            self.product.resize(new_len, None);
            self.product_count.resize(new_len, 0);
            self.required_station.resize(new_len, None);
            self.required_tool.resize(new_len, None);
            self.requires_discovery.resize(new_len, false);
        }

        if self.defined[idx] {
            return Err(RecipeError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.defined_ids.push(index);
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.category[idx] = Some(attrs.category);
        self.ingredients[idx] = attrs.ingredients;
        self.product[idx] = Some(attrs.product);
        self.product_count[idx] = attrs.product_count;
        Ok(())
    }

    /// 给一条已注册的配方设置场地前置——**覆盖，不是追加**（一条配方
    /// 只有一个场地），与 `ItemTable::set_damage_category` 同一种单值
    /// 覆盖语义。`station` 指向的是一件家具，见
    /// [`RecipeDef::required_station`]。
    pub fn set_required_station(
        &mut self,
        index: ContentIndex,
        station: ContentIndex,
    ) -> Result<(), RecipeError> {
        if !self.is_defined(index) {
            return Err(RecipeError::UnknownRecipe(index));
        }
        self.required_station[index.get() as usize] = Some(station);
        Ok(())
    }

    /// 给一条已注册的配方设置工具前置——覆盖语义，理由同
    /// [`Self::set_required_station`]。
    pub fn set_required_tool(
        &mut self,
        index: ContentIndex,
        tool: ContentIndex,
    ) -> Result<(), RecipeError> {
        if !self.is_defined(index) {
            return Err(RecipeError::UnknownRecipe(index));
        }
        self.required_tool[index.get() as usize] = Some(tool);
        Ok(())
    }

    /// 声明这条配方必须先被发现才做得出来（配方发现批次）——脚本层
    /// 对应函数是 `recipe-requires-discovery!`，见
    /// [`RecipeDef::requires_discovery`] 文档。**覆盖语义**，理由同
    /// [`Self::set_required_station`]。
    ///
    /// 只有「设为真」这一个方向：没有 `clear_requires_discovery`，因为
    /// 「不需要发现」就是不调用本函数（默认值），与
    /// `recipe-requires-station!` 没有配套的「取消场地要求」是同一条
    /// 既有形状——注册期声明是**加法**，不是一台可以来回拨的开关。
    pub fn set_requires_discovery(&mut self, index: ContentIndex) -> Result<(), RecipeError> {
        if !self.is_defined(index) {
            return Err(RecipeError::UnknownRecipe(index));
        }
        self.requires_discovery[index.get() as usize] = true;
        Ok(())
    }

    /// 某个类别下已注册的全部配方，按索引升序——
    /// [`ll_sim::craft::RecipeCatalog::recipes_in_category`] 的真实
    /// 实现，见该方法文档「为什么是『返回全部、由 `resolve` 自己筛』」
    /// 一节。
    ///
    /// 一次线性扫描列式存储的 `category` 列。升序由「按下标从小到大
    /// 遍历 `Vec`」直接保证，不依赖任何哈希容器（约束 C5）。
    pub fn in_category(&self, category: ContentIndex) -> Vec<ContentIndex> {
        let mut found: Vec<ContentIndex> = self
            .defined_ids
            .iter()
            .copied()
            .filter(|index| self.category[index.get() as usize] == Some(category))
            .collect();
        // 排序不依赖 `defined_ids` 的原始注册顺序——那是装载顺序，会随
        // mod 集合变化（约束 C5）。手法同
        // `crate::quest::QuestTable::defined_indices`。
        found.sort_by_key(ContentIndex::get);
        found
    }

    /// 已注册的**全部**配方，按索引升序（约束 C5）。
    ///
    /// 与 [`Self::in_category`] 同一个手法、同一条排序理由，只是不带
    /// 类别过滤。存在的理由是制作菜单要列出「一共有哪些配方可选」——
    /// 那块菜单不按类别分页（本体一共十来条配方，分页只会多一层要按
    /// 的键），因此需要一条不带类别参数的枚举入口。
    ///
    /// **排序键是 `ContentIndex` 而不是注册顺序**：后者是装载顺序,会
    /// 随 mod 集合与装载次序变化,把它端到玩家眼前意味着"换一个 mod
    /// 顺序,菜单第三条就变成别的东西",正是约束 C5 要防的那类不确定
    /// 顺序（这里落在**显示顺序**上,不是逻辑判断,但玩家按的是"第几
    /// 条",显示顺序在这一刻就是逻辑输入）。
    pub fn defined_indices(&self) -> Vec<ContentIndex> {
        let mut found = self.defined_ids.clone();
        found.sort_by_key(ContentIndex::get);
        found
    }

    /// 给定的配方索引当前是否已经登记过属性。
    pub fn is_defined(&self, recipe: ContentIndex) -> bool {
        self.defined
            .get(recipe.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一条配方的完整属性，未注册的索引返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, recipe: ContentIndex) -> Option<RecipeView<'_>> {
        if !self.is_defined(recipe) {
            return None;
        }
        let idx = recipe.get() as usize;
        Some(RecipeView {
            display_name_key: self.display_name_key[idx]
                .as_ref()
                .expect("defined 为真时 display_name_key 必已写入"),
            category: self.category[idx].expect("defined 为真时 category 必已写入"),
            ingredients: &self.ingredients[idx],
            product: self.product[idx].expect("defined 为真时 product 必已写入"),
            product_count: self.product_count[idx],
            required_station: self.required_station[idx],
            required_tool: self.required_tool[idx],
            requires_discovery: self.requires_discovery[idx],
        })
    }
}

/// [`RecipeTable`] 与 [`RecipeCategoryTable`] 绑在一起的结算目录——
/// `ll_sim::craft::RecipeCatalog` 的真实实现。
///
/// # 为什么需要这个中间类型（而不是 `impl RecipeCatalog for RecipeTable`）
///
/// 与 `impl ItemCatalog for ItemTable` 那类「某张表自己就实现了 trait」
/// 不同：`RecipeCatalog` 要回答两个问题，其中「这个类别要求哪些副职」
/// 的答案在**另一张表**里。这与 [`crate::quest::RegisteredQuests`] 要把
/// `QuestTable` 与 `Registry` 绑在一起是同一种情形，走同一条先例：
/// 一个只借用、不持有所有权的轻量绑定类型。
#[derive(Debug, Clone, Copy)]
pub struct RegisteredRecipes<'a> {
    /// 配方本体表。
    pub recipes: &'a RecipeTable,
    /// 配方类别表——副职闸门的出处。
    pub categories: &'a RecipeCategoryTable,
}

impl RecipeCatalog for RegisteredRecipes<'_> {
    fn recipe(&self, recipe: ContentIndex) -> Option<RecipeRule> {
        let view = self.recipes.get(recipe)?;
        Some(RecipeRule {
            category: view.category,
            ingredients: view.ingredients.to_vec(),
            product: view.product,
            product_count: view.product_count,
            required_station: view.required_station,
            required_tool: view.required_tool,
            requires_discovery: view.requires_discovery,
        })
    }

    fn category_required_subclasses(&self, category: ContentIndex) -> Vec<ContentIndex> {
        self.categories
            .get(category)
            .map(|def| def.required_subclasses.clone())
            .unwrap_or_default()
    }

    fn recipes_in_category(&self, category: ContentIndex) -> Vec<ContentIndex> {
        self.recipes.in_category(category)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::Interner;

    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    fn key(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    fn attrs(category: ContentIndex, item: ContentIndex, product: ContentIndex) -> RecipeAttrs {
        RecipeAttrs {
            display_name_key: key("lostland:roast_meat_recipe_display_name"),
            category,
            ingredients: vec![RecipeIngredient { item, count: 1 }],
            product,
            product_count: 1,
        }
    }

    #[test]
    fn 定义后可以查到全部字段且两条前置默认为none() {
        // Arrange
        let mut interner = Interner::new();
        let cooking = index(&mut interner, "lostland:cooking");
        let raw_meat = index(&mut interner, "lostland:raw_meat");
        let roast = index(&mut interner, "lostland:roast_meat");
        let recipe = index(&mut interner, "lostland:roast_meat_recipe");
        let mut table = RecipeTable::new();

        // Act
        table
            .define(recipe, attrs(cooking, raw_meat, roast))
            .expect("首次定义应当成功");

        // Assert
        let view = table.get(recipe).expect("刚定义的配方应能查到");
        assert_eq!(view.category, cooking);
        assert_eq!(
            view.ingredients,
            [RecipeIngredient {
                item: raw_meat,
                count: 1
            }]
        );
        assert_eq!(view.product, roast);
        assert_eq!(view.product_count, 1);
        assert_eq!(view.required_station, None);
        assert_eq!(view.required_tool, None);
    }

    #[test]
    fn 空食材列表被注册期拒绝() {
        // Arrange
        let mut interner = Interner::new();
        let cooking = index(&mut interner, "lostland:cooking");
        let roast = index(&mut interner, "lostland:roast_meat");
        let recipe = index(&mut interner, "lostland:free_lunch");
        let mut table = RecipeTable::new();

        // Act
        let result = table.define(
            recipe,
            RecipeAttrs {
                display_name_key: key("lostland:free_lunch_display_name"),
                category: cooking,
                ingredients: Vec::new(),
                product: roast,
                product_count: 1,
            },
        );

        // Assert
        assert_eq!(result, Err(RecipeError::EmptyIngredients(recipe)));
        assert!(!table.is_defined(recipe));
    }

    #[test]
    fn 食材数量为零或产出数量为零都被注册期拒绝() {
        // Arrange
        let mut interner = Interner::new();
        let cooking = index(&mut interner, "lostland:cooking");
        let raw_meat = index(&mut interner, "lostland:raw_meat");
        let roast = index(&mut interner, "lostland:roast_meat");
        let zero_ingredient = index(&mut interner, "lostland:zero_ingredient_recipe");
        let zero_product = index(&mut interner, "lostland:zero_product_recipe");
        let mut table = RecipeTable::new();

        // Act
        let ingredient_result = table.define(
            zero_ingredient,
            RecipeAttrs {
                ingredients: vec![RecipeIngredient {
                    item: raw_meat,
                    count: 0,
                }],
                ..attrs(cooking, raw_meat, roast)
            },
        );
        let product_result = table.define(
            zero_product,
            RecipeAttrs {
                product_count: 0,
                ..attrs(cooking, raw_meat, roast)
            },
        );

        // Assert
        assert_eq!(
            ingredient_result,
            Err(RecipeError::ZeroIngredientCount(zero_ingredient))
        );
        assert_eq!(
            product_result,
            Err(RecipeError::ZeroProductCount(zero_product))
        );
    }

    #[test]
    fn 同一件成品允许两条不同的配方() {
        // 设计文档九节④：这是刻意不加唯一性约束换来的零成本变化度，
        // 一条测试把「刻意」钉住——将来若有人顺手加上唯一性校验，
        // 这条会立刻变红。
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let ingot = index(&mut interner, "lostland:iron_ingot");
        let scrap = index(&mut interner, "lostland:scrap_iron");
        let sword = index(&mut interner, "lostland:iron_sword");
        let from_ingot = index(&mut interner, "lostland:iron_sword_recipe");
        let from_scrap = index(&mut interner, "lostland:iron_sword_from_scrap");
        let mut table = RecipeTable::new();

        // Act
        table
            .define(from_ingot, attrs(forging, ingot, sword))
            .expect("第一条配方应当成功");
        let second = table.define(from_scrap, attrs(forging, scrap, sword));

        // Assert
        assert!(second.is_ok());
        assert_eq!(table.get(from_ingot).expect("已定义").product, sword);
        assert_eq!(table.get(from_scrap).expect("已定义").product, sword);
    }

    #[test]
    fn 两条前置可以后续设置且给未注册配方设置会返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let ingot = index(&mut interner, "lostland:iron_ingot");
        let sword = index(&mut interner, "lostland:iron_sword");
        let forge_station = index(&mut interner, "lostland:forge");
        let hammer = index(&mut interner, "lostland:smithing_hammer");
        let recipe = index(&mut interner, "lostland:iron_sword_recipe");
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let mut table = RecipeTable::new();
        table
            .define(recipe, attrs(forging, ingot, sword))
            .expect("首次定义应当成功");

        // Act
        table
            .set_required_station(recipe, forge_station)
            .expect("设置场地应当成功");
        table
            .set_required_tool(recipe, hammer)
            .expect("设置工具应当成功");
        let orphan = table.set_required_station(never_defined, forge_station);

        // Assert
        let view = table.get(recipe).expect("已定义");
        assert_eq!(view.required_station, Some(forge_station));
        assert_eq!(view.required_tool, Some(hammer));
        assert_eq!(orphan, Err(RecipeError::UnknownRecipe(never_defined)));
    }

    #[test]
    fn 重复定义同一个配方索引返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let cooking = index(&mut interner, "lostland:cooking");
        let raw_meat = index(&mut interner, "lostland:raw_meat");
        let roast = index(&mut interner, "lostland:roast_meat");
        let recipe = index(&mut interner, "lostland:roast_meat_recipe");
        let mut table = RecipeTable::new();
        table
            .define(recipe, attrs(cooking, raw_meat, roast))
            .expect("首次定义应当成功");

        // Act
        let result = table.define(recipe, attrs(cooking, raw_meat, roast));

        // Assert
        assert_eq!(result, Err(RecipeError::DuplicateDefinition(recipe)));
    }

    #[test]
    fn 结算目录把两张表绑在一起并如实转发类别闸门() {
        // 直接验收 `impl RecipeCatalog for RegisteredRecipes`——
        // resolve_craft 只经这条接口读配方与闸门。
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let artisan = index(&mut interner, "lostland:artisan");
        let ingot = index(&mut interner, "lostland:iron_ingot");
        let sword = index(&mut interner, "lostland:iron_sword");
        let recipe = index(&mut interner, "lostland:iron_sword_recipe");
        let mut recipes = RecipeTable::new();
        recipes
            .define(recipe, attrs(forging, ingot, sword))
            .expect("定义应当成功");
        let mut categories = RecipeCategoryTable::new();
        categories
            .define(
                forging,
                key("lostland:recipe_category_forging_display_name"),
            )
            .expect("定义应当成功");
        categories
            .add_required_subclass(forging, artisan)
            .expect("追加闸门应当成功");
        let catalog = RegisteredRecipes {
            recipes: &recipes,
            categories: &categories,
        };

        // Act
        let rule = catalog.recipe(recipe).expect("已注册的配方应能查到");

        // Assert
        assert_eq!(rule.category, forging);
        assert_eq!(rule.product, sword);
        assert_eq!(catalog.category_required_subclasses(forging), vec![artisan]);
    }

    #[test]
    fn 未注册的类别在结算目录里退化成不设闸门() {
        // 降级方向：查不到类别不该表现成「这条配方谁都做不了」，
        // 见 ll_sim::craft::RecipeCatalog::category_required_subclasses 文档。
        // Arrange
        let mut interner = Interner::new();
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let recipes = RecipeTable::new();
        let categories = RecipeCategoryTable::new();
        let catalog = RegisteredRecipes {
            recipes: &recipes,
            categories: &categories,
        };

        // Act & Assert
        assert!(
            catalog
                .category_required_subclasses(never_defined)
                .is_empty()
        );
        assert_eq!(catalog.recipe(never_defined), None);
    }
}
