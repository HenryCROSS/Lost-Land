//! 配方类别注册表：`register-recipe-category` 的存储落点（制作系统
//! 批次，`knowledge/design/crafting-system.md` 四、七节）。
//!
//! # 为什么类别值得一张独立的表
//!
//! 照抄 [`crate::weapon_category::WeaponCategoryTable`] 与
//! [`crate::damage_category::DamageCategoryTable`] 两张**已落地**的开放
//! 类别表确立的模式：类别是一个独立的内容表（`BTreeMap` + `define`
//! 注册期查重 + ADR 0015「未注册返回 `None`」），引用方持有一个指向它
//! 的 `ContentIndex`（这里是 `RecipeDef.category`）。
//!
//! 表在注册期提供一条真实的校验：`register-recipe` 传进来的
//! `category-id` 若从未注册过，当场拒绝。这能拦住 `"lostlan:cooking"`
//! 这类拼写错误——拼写错误若不拦，症状是「这条配方永远不出现在任何
//! 分类里」，是最难查的一类内容 bug。**这条注册期校验是今天就成立的
//! 消费者**，不是等制作界面才有用的承诺。
//!
//! 反过来，给 `RecipeDef.category` 用一个封闭 Rust 枚举
//! （`enum RecipeCategory { Cook, Forge, Tailor, Alchemy }`）会直接违反
//! ADR 0018——mod 作者想加「木工」「制符」「炼器」时必须改本体 Rust
//! 代码。上面两张已落地的类别表当初正是为了避开这一点才做成开放注册表
//! （`weapon_category.rs` 模块文档原文：「可扩展项没有自然上限」）。
//!
//! # 与那两张表的两处不同
//!
//! 1. **多了 `display_name_key`。** 武器/伤害类别至今没有任何 UI 落点，
//!    所以它们没有这个字段；制作类别从设计上就是玩家会看见的分组维度
//!    （制作界面按类别分栏）。在制作界面真的落地之前它仍然是一个待
//!    接线字段，与 `ItemDef.display_name_key` 同款，走
//!    `scripts/ci/check_field_consumers.py` 的同一条豁免。
//! 2. **多了 `required_subclasses`，且由独立函数写入。**
//!    见 [`RecipeCategoryDef::required_subclasses`] 与
//!    [`RecipeCategoryTable::add_required_subclass`] 的文档。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};

/// 配方类别的注册表条目——「烹饪」「锻造」「裁缝」「炼金」这一类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeCategoryDef {
    /// 指向 Fluent 本地化键，不存字面字符串——与
    /// [`crate::subclass::SubclassDef::display_name_key`] 同一条纪律。
    /// 制作界面按类别分栏展示时的标题。
    pub display_name_key: NamespacedId,

    /// 拥有其中**任意一个**副职即可使用本类别的配方（any-of 语义，
    /// 与 `ll_sim::craft::RecipeCatalog::category_required_subclasses`
    /// 的返回值一一对应）。
    ///
    /// **空列表 = 不设闸门，人人可做**——这条默认值正是
    /// `knowledge/design/food-and-cooking-system.md` 五节「菜谱全部
    /// 已知不设解锁门槛」裁定的直接延续：`lostland:cooking` 不调用
    /// [`RecipeCategoryTable::add_required_subclass`]，于是人人都能
    /// 做饭；锻造/裁缝各调用一次，于是需要对应副职。**「有没有闸门」
    /// 因此是一个纯内容决定，系统不预设立场。**
    ///
    /// # 为什么闸在类别而不是每条配方
    ///
    /// 若每条配方各自声明需要哪个副职，「工匠能做的全部东西」就散落
    /// 在几十条配方里，加一个新副职要逐条改；闸在类别上，「工匠 =
    /// 锻造类别的访问权」是一句话，新增一条锻造配方自动继承闸门。
    ///
    /// # 这道闸不是「学会配方」
    ///
    /// 两件事必须分清：**类别访问权**（本字段）问「你是不是工匠」，
    /// 读的是已落地的 `ll_world::entity::Agent::subclasses`，零新增
    /// 实体字段；**配方解锁**（`known_recipes`，食物系统五节评估后
    /// 否决）问「你知不知道这张图纸」，需要一个新的 `Agent` 字段，
    /// 本批次同样不做。
    pub required_subclasses: Vec<ContentIndex>,
}

/// 配方类别注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些
/// 错误在加载时就报出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeCategoryError {
    /// 同一个内容索引被定义了两次。
    DuplicateDefinition(ContentIndex),
    /// 想给一个从未 [`RecipeCategoryTable::define`] 过的类别追加副职
    /// 闸门——与 `ItemTable::set_damage_category` 要求目标已存在同一条
    /// 「新增能力用新函数，但目标必须已注册」的纪律。
    UnknownCategory(ContentIndex),
}

impl fmt::Display for RecipeCategoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecipeCategoryError::DuplicateDefinition(index) => {
                write!(f, "配方类别索引 {} 被重复定义", index.get())
            }
            RecipeCategoryError::UnknownCategory(index) => {
                write!(
                    f,
                    "配方类别索引 {} 尚未通过 register-recipe-category 注册",
                    index.get()
                )
            }
        }
    }
}

impl std::error::Error for RecipeCategoryError {}

/// 配方类别定义表：`ContentIndex`（类别自身的命名空间标识符）→
/// [`RecipeCategoryDef`]。
///
/// `BTreeMap` 存储而非列式存储：类别表条目数量少（本体四类），没有
/// 列式访问的性能诉求——与 [`crate::weapon_category::WeaponCategoryTable`]
/// 同一条取舍。配方本体（[`crate::recipe::RecipeTable`]）条目多得多，
/// 因此那一张走列式存储。
#[derive(Debug, Default, Clone)]
pub struct RecipeCategoryTable {
    entries: BTreeMap<ContentIndex, RecipeCategoryDef>,
}

impl RecipeCategoryTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：登记一条配方类别定义。
    ///
    /// `required_subclasses` 恒以空列表开始——副职闸门走
    /// [`Self::add_required_subclass`] 单独追加，不是本函数的参数，
    /// 理由见该方法文档。
    pub fn define(
        &mut self,
        index: ContentIndex,
        display_name_key: NamespacedId,
    ) -> Result<(), RecipeCategoryError> {
        if self.entries.contains_key(&index) {
            return Err(RecipeCategoryError::DuplicateDefinition(index));
        }
        self.entries.insert(
            index,
            RecipeCategoryDef {
                display_name_key,
                required_subclasses: Vec::new(),
            },
        );
        Ok(())
    }

    /// 给一个已注册的类别追加一个副职闸门（any-of，可多次调用）。
    ///
    /// # 为什么是独立函数，不是 [`Self::define`] 的参数
    ///
    /// 照抄 `skill-requires!`（`knowledge/design/skill-learn-requirements.md`
    /// 六节）与**已落地**的 `register-item-damage-category`
    /// （[`crate::script_item_api`]）两条先例：「分类展示」与「强制
    /// 闸门」是两件独立的事，混在一起会在将来某天有人想「只分类展示、
    /// 不强制」或反过来时变成一处隐藏耦合。给一张已注册的内容表条目
    /// 追加一个可选属性，走独立函数写入同一张表，不加宽原注册函数的
    /// 位置参数列表。
    ///
    /// **不做去重校验**——重复追加同一个副职在 any-of 判定里是幂等的
    /// （理由同 `RaceTable::add_trait_grant`）。**追加，不是覆盖**：
    /// 与 `ItemTable::set_damage_category` 的「单值覆盖」语义相反，
    /// 因为一个类别可以同时开放给多个副职。
    pub fn add_required_subclass(
        &mut self,
        index: ContentIndex,
        subclass: ContentIndex,
    ) -> Result<(), RecipeCategoryError> {
        let Some(def) = self.entries.get_mut(&index) else {
            return Err(RecipeCategoryError::UnknownCategory(index));
        };
        def.required_subclasses.push(subclass);
        Ok(())
    }

    /// 查询一条配方类别定义，未注册返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, index: ContentIndex) -> Option<&RecipeCategoryDef> {
        self.entries.get(&index)
    }

    /// 给定的配方类别索引当前是否已经登记过定义——供
    /// [`crate::content_hash::classify_index`] 判定表归属。
    pub fn is_defined(&self, index: ContentIndex) -> bool {
        self.entries.contains_key(&index)
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

    #[test]
    fn 定义后可以查到同一条配方类别且默认不设闸门() {
        // Arrange
        let mut interner = Interner::new();
        let cooking = index(&mut interner, "lostland:cooking");
        let mut table = RecipeCategoryTable::new();

        // Act
        table
            .define(
                cooking,
                key("lostland:recipe_category_cooking_display_name"),
            )
            .expect("首次定义应当成功");

        // Assert：空闸门是默认值，正是「人人可做」这条既有裁定的落点。
        let def = table.get(cooking).expect("刚定义的类别应能查到");
        assert!(def.required_subclasses.is_empty());
        assert!(table.is_defined(cooking));
    }

    #[test]
    fn 追加副职闸门可以多次调用且按追加顺序保留() {
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let artisan = index(&mut interner, "lostland:artisan");
        let smith = index(&mut interner, "yourmod:master_smith");
        let mut table = RecipeCategoryTable::new();
        table
            .define(
                forging,
                key("lostland:recipe_category_forging_display_name"),
            )
            .expect("首次定义应当成功");

        // Act
        table
            .add_required_subclass(forging, artisan)
            .expect("追加应当成功");
        table
            .add_required_subclass(forging, smith)
            .expect("第二次追加同样应当成功——any-of 允许多个副职");

        // Assert
        assert_eq!(
            table.get(forging).expect("已定义").required_subclasses,
            vec![artisan, smith]
        );
    }

    #[test]
    fn 给未注册的类别追加副职闸门返回错误而非静默创建() {
        // 反例：若这里静默创建一条类别，`register-recipe` 的
        // 「category-id 必须已注册」那条拼写错误防线就会被绕过。
        // Arrange
        let mut interner = Interner::new();
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let artisan = index(&mut interner, "lostland:artisan");
        let mut table = RecipeCategoryTable::new();

        // Act
        let result = table.add_required_subclass(never_defined, artisan);

        // Assert
        assert_eq!(
            result,
            Err(RecipeCategoryError::UnknownCategory(never_defined))
        );
        assert!(!table.is_defined(never_defined));
    }

    #[test]
    fn 重复定义同一个配方类别索引返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let cooking = index(&mut interner, "lostland:cooking");
        let mut table = RecipeCategoryTable::new();
        table
            .define(
                cooking,
                key("lostland:recipe_category_cooking_display_name"),
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(cooking, key("yourmod:other_display_name"));

        // Assert
        assert_eq!(
            result,
            Err(RecipeCategoryError::DuplicateDefinition(cooking))
        );
    }

    #[test]
    fn 未注册的配方类别索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let table = RecipeCategoryTable::new();

        // Act & Assert
        assert_eq!(table.get(never_defined), None);
        assert!(!table.is_defined(never_defined));
    }
}
