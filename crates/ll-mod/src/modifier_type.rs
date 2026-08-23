//! 加值类型注册表：`modifier_types.json5` 的存储落点——规则修正
//! （[`ll_sim::rule_modifier::RuleModifier`]）多来源合并时「同一类型取
//! 最强、不同类型相加」这条规则里的**类型**是什么，答案在这张表。
//!
//! # 模型出处：D&D 3.5e / 开拓者的「加值类型」
//!
//! 项目所有者的裁定：
//!
//! ```text
//! 同一类型 → 取最强，不叠加
//! 不同类型 → 相加
//! ```
//!
//! 合并算法本身住在 `ll_sim::rule_modifier::merged_across_types`（决策
//! 层），本表只负责回答「有哪些类型」以及「你引用的这个类型注册过
//! 吗」。两者分居两个 crate 的理由与
//! [`crate::damage_category::DamageCategoryTable`] 完全相同：注册表是
//! 装载期的存储层，消费它的是结算层。
//!
//! # 为什么是开放注册表，不是封闭 Rust 枚举
//!
//! 照抄 [`crate::damage_category::DamageCategoryTable`] /
//! [`crate::weapon_category::WeaponCategoryTable`] /
//! [`crate::recipe_category::RecipeCategoryTable`] 三张**已落地**的开放
//! 类别表确立的模式，理由也逐字相同（`weapon_category.rs` 模块文档
//! 原文：「可扩展项没有自然上限」）：3.5e 自己就有增强/闪避/威慑/
//! 士气/洞察/天生……一长串，而一个 mod 想加「符文加值」「潮汐加值」
//! 时不该需要改本体 Rust 代码（[ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)）。
//!
//! 表在注册期提供一条**今天就成立**的真实校验：任何一条规则修正声明
//! 的 `modifier_type` 若从未注册过，装载当场失败，与
//! `register-recipe` 拒绝未注册配方类别是同一条先例。这拦的是
//! `"lostlan:enhancement"` 这类拼写错误——它若不拦，症状是「这条修正
//! 悄悄自成一个谁也够不着的桶」，而**分桶恰恰会让它比正确写法更强**
//! （不与同类竞争、还能与别人相加），是最难查的一类内容 bug。
//!
//! # 为什么 [`ModifierTypeDef`] 一个字段都没有
//!
//! 加值类型是**纯身份**：合并规则只问「这两条修正的类型是不是同一
//! 个」，不问这个类型本身有任何属性。上面三张表各自多出来的字段都有
//! 具体用途（伤害/武器类别的 `default_formula`、配方类别的
//! `display_name_key` 与副职闸门），这里一个都套不上：
//!
//! - **不加 `display_name_key`**：加值类型至今没有任何 UI 落点。角色
//!   面板显示的是「减伤 5」这个结果，不是「附魔 3 + 炼金 2」这个分解
//!   （那需要一整套「加值来源明细」的呈现层，本批次没有）。凭空加一个
//!   本地化键字段，等于凭空要求本体为每个类型写一条永远没人读的翻译。
//! - **不加「这个类型能不能叠加自身」这类开关**：所有者的裁定是一条
//!   全局规则（同类型取最强），不是一个逐类型可调的参数。做成字段就是
//!   替所有者发明了一个他没有裁定过的自由度。
//!
//! 空结构体因此不是「占位」，是这张表**当前真实的形状**；哪天加值类型
//! 真的需要携带属性，照 [`crate::recipe_category::RecipeCategoryDef`]
//! 的先例加字段即可。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::ContentIndex;

/// 加值类型的注册表条目——「增强」「炼金」「天生」这一类。
///
/// 没有字段，理由见模块文档「为什么 [`ModifierTypeDef`] 一个字段都
/// 没有」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModifierTypeDef {}

/// 加值类型注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierTypeError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::damage_category::DamageCategoryError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for ModifierTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModifierTypeError::DuplicateDefinition(index) => {
                write!(f, "加值类型索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for ModifierTypeError {}

/// 加值类型定义表：`ContentIndex`（类型自身的命名空间标识符）→
/// [`ModifierTypeDef`]，`BTreeMap` 的理由见
/// [`crate::damage_category`] 模块文档「为什么是 `BTreeMap`」一节
/// （本表的查询同样只发生在装载期）。
#[derive(Debug, Default, Clone)]
pub struct ModifierTypeTable {
    entries: BTreeMap<ContentIndex, ModifierTypeDef>,
}

impl ModifierTypeTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：登记一条加值类型定义。
    pub fn define(
        &mut self,
        index: ContentIndex,
        def: ModifierTypeDef,
    ) -> Result<(), ModifierTypeError> {
        if self.entries.contains_key(&index) {
            return Err(ModifierTypeError::DuplicateDefinition(index));
        }
        self.entries.insert(index, def);
        Ok(())
    }

    /// 查询一条加值类型定义，未注册返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, index: ContentIndex) -> Option<&ModifierTypeDef> {
        self.entries.get(&index)
    }

    /// 给定的索引当前是否已经登记过定义——供
    /// [`crate::content_hash::classify_index`] 判定表归属，也供
    /// [`crate::content_schema_gear`] 在注册期拒绝未注册的类型引用。
    pub fn is_defined(&self, index: ContentIndex) -> bool {
        self.entries.contains_key(&index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    #[test]
    fn 定义后可以查到同一条加值类型() {
        // Arrange
        let mut interner = Interner::new();
        let index = index(&mut interner, "lostland:enhancement");
        let mut table = ModifierTypeTable::new();

        // Act
        table
            .define(index, ModifierTypeDef {})
            .expect("首次定义应当成功");

        // Assert
        assert!(table.get(index).is_some());
        assert!(table.is_defined(index));
    }

    #[test]
    fn 重复定义同一个加值类型索引返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let index = index(&mut interner, "lostland:alchemical");
        let mut table = ModifierTypeTable::new();
        table
            .define(index, ModifierTypeDef {})
            .expect("首次定义应当成功");

        // Act
        let result = table.define(index, ModifierTypeDef {});

        // Assert
        assert_eq!(result, Err(ModifierTypeError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的加值类型索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let table = ModifierTypeTable::new();

        // Act & Assert
        assert_eq!(table.get(never_defined), None);
        assert!(!table.is_defined(never_defined));
    }
}
