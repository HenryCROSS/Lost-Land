//! 武器类别定义表：`register-weapon-category` 的存储落点
//! （`knowledge/design/damage-formula-mod-api.md` 十七、二十一节）。
//!
//! 与 [`crate::damage_category::DamageCategoryTable`] 结构完全同构——
//! 两者都是「可扩展项没有自然上限」的开放集合（十七节「表达方式」
//! 一节），两份独立的存储只是因为它们回答的是两个独立的问题（十七节
//! 「是不是同一种东西：不是」），不是同一张表的两种视图。本模块的
//! 全部设计取舍（`BTreeMap`、不接四层默认公式解析链条）与
//! [`crate::damage_category`] 模块文档完全相同，不在此重复。
//!
//! # 本批次没有给 `ItemDef` 加对应字段
//!
//! 与 [`crate::damage_category::DamageCategoryTable`] 不同——
//! `ItemDef.damage_category`（伤害类别/抗性接线批次新增）有真实的
//! `resolve` 侧消费路径（抗性查询需要知道"这一下是哪个伤害类别"），
//! 而"这件武器算哪一类武器"（剑/斧/弓……）在本批次没有任何消费者：
//! 十九节的默认公式挂载链条第 3 层（武器类别默认公式）不在本批次范围
//! （见 [`crate::damage_category`] 模块文档「本批次范围」一节），除此
//! 之外武器类别设计文档没有给出任何其它用途。给 `ItemDef` 加一个没有
//! 任何读者的 `weapon_category` 字段会立刻撞上
//! `scripts/ci/check_field_consumers.py`（若将来把它加进
//! `TARGET_TYPES`）同一类"声明了但没人读"的死字段——本批次因此只落地
//! `register-weapon-category` 这一半（任务书「本批次范围」第 1/2 条明确
//! 要求的注册表 + 脚本绑定），不额外发明一个当前无处可用的 `ItemDef`
//! 字段（YAGNI）。真正需要"这把武器是剑还是斧"这个问题被回答时（多轮
//! 判定/`DamageComponent` 落地批次），照 `ItemDef.damage_category` 的
//! 先例加一个字段即可。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::ContentIndex;

/// 武器类别的注册表条目——「剑」「斧」「弓」「弩」这一类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponCategoryDef {
    /// 这个武器类别没有被具体武器覆盖时使用的默认公式——本批次不接线，
    /// 见 [`crate::damage_category`] 模块文档「本批次范围」一节。
    pub default_formula: Option<ContentIndex>,
}

/// 武器类别注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponCategoryError {
    /// 同一个内容索引被定义了两次。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for WeaponCategoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WeaponCategoryError::DuplicateDefinition(index) => {
                write!(f, "武器类别索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for WeaponCategoryError {}

/// 武器类别定义表：`ContentIndex`（类别自身的命名空间标识符）→
/// [`WeaponCategoryDef`]。
#[derive(Debug, Default, Clone)]
pub struct WeaponCategoryTable {
    entries: BTreeMap<ContentIndex, WeaponCategoryDef>,
}

impl WeaponCategoryTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：登记一条武器类别定义。
    pub fn define(
        &mut self,
        index: ContentIndex,
        def: WeaponCategoryDef,
    ) -> Result<(), WeaponCategoryError> {
        if self.entries.contains_key(&index) {
            return Err(WeaponCategoryError::DuplicateDefinition(index));
        }
        self.entries.insert(index, def);
        Ok(())
    }

    /// 查询一条武器类别定义，未注册返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, index: ContentIndex) -> Option<&WeaponCategoryDef> {
        self.entries.get(&index)
    }

    /// 给定的武器类别索引当前是否已经登记过定义——供
    /// [`crate::content_hash::classify_index`] 判定表归属。
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
    fn 定义后可以查到同一条武器类别() {
        // Arrange
        let mut interner = Interner::new();
        let index = index(&mut interner, "lostland:sword");
        let mut table = WeaponCategoryTable::new();

        // Act
        table
            .define(
                index,
                WeaponCategoryDef {
                    default_formula: None,
                },
            )
            .expect("首次定义应当成功");

        // Assert
        assert!(table.get(index).is_some());
        assert!(table.is_defined(index));
    }

    #[test]
    fn 重复定义同一个武器类别索引返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let index = index(&mut interner, "lostland:axe");
        let mut table = WeaponCategoryTable::new();
        table
            .define(
                index,
                WeaponCategoryDef {
                    default_formula: None,
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            WeaponCategoryDef {
                default_formula: None,
            },
        );

        // Assert
        assert_eq!(result, Err(WeaponCategoryError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的武器类别索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let table = WeaponCategoryTable::new();

        // Act & Assert
        assert_eq!(table.get(never_defined), None);
        assert!(!table.is_defined(never_defined));
    }
}
