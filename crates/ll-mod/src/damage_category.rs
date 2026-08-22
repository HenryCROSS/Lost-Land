//! 伤害类别定义表：`register-damage-category` 的存储落点
//! （`knowledge/design/damage-formula-mod-api.md` 十七、二十一节）。
//!
//! # 与武器类别、`DamageSchool` 都是独立的轴
//!
//! 十七节「是不是同一种东西：不是」——伤害类别（物理/火/冰……）回答
//! "这一下造成哪种伤害"，服务两个目的：挂公式（十八/十九节，本批次不
//! 落地，见「本批次范围」一节）与挂抗性（二十节，本批次落地）。
//! [`crate::weapon_category::WeaponCategoryTable`] 回答的是完全独立的
//! 另一个问题（"用什么打"），`DamageSchool`（`ll_world::item` 模块文档
//! 已核实：本仓库尚未落地这个类型，`resolve_attack` 目前仍是纯物理
//! 近战占位实现,见 `crates/ll-world/src/item.rs`「为什么不现在就加
//! 魔抗/意志抗性两个变体」一节）描述的又是第三个问题（"读哪组防御
//! 字段"）——三者不合并，见十七节「与既有 `DamageSchool` 的关系：
//! 正交，不合并」一节完整论证。
//!
//! # 为什么是 `BTreeMap`，不是列式存储
//!
//! 与 [`crate::formula::FormulaTable`] 同一条理由（其模块文档「为什么
//! 不是列式存储」一节）：伤害类别的查询发生在装载期（`register-item-damage-category`
//! 校验类别是否已注册）与一次攻击同数量级（`resistance_multiplier_permille`
//! 每次攻击查一次），不是逐 tick 高频路径。设计文档十七节「表达方式」
//! 一节进一步指出：与 `SurfaceKindTable` 刻意的一处不同是本表**不**
//! 分配稠密位下标——地表分类是高频运行期位测试，伤害类别的消费场景
//! 是一次性查表，`BTreeMap<ContentIndex, _>` 已经足够。
//!
//! # 本批次范围：注册表 + 校验，不接四层默认公式解析链条
//!
//! `default_formula` 字段按设计文档十七节的形状声明（`register-damage-category`
//! 的第二个参数），注册期校验它若非空则必须已经通过
//! `register-damage-formula` 注册过（见 `crate::script_damage_category_api`
//! 文档）——但十九节「默认公式的挂载层级与优先级」这条完整的四层
//! 解析链条（分项自身 → 伤害类别默认 → 武器类别默认 → 全局默认）本批次
//! 不接线：`resolve_attack` 仍然只用
//! `ll_sim::formula::DamageFormulaCatalog` 现有的两层（显式引用 → 全局
//! 默认）挑公式，见 `crate::script_damage_formula_api` 模块文档「本批次
//! 排除」一节同一条 YAGNI 判断——四层解析链条服务的是"分项相加"
//! （十八节），而分项列表本身（`DamageComponent`）依赖 `WeaponDef`/
//! `SkillDef`（P6 范畴，见该文档二十三节前置依赖清单第 4 项），两者都
//! 不在本批次范围内。`default_formula` 字段因此现在只是"声明先行"，
//! 与 `TraitDef.rule_modifiers` 里其余三个 `RuleModifier` 变体、
//! `RaceDef.xp_reward` 早期状态是同一条既有纪律——先把形状定下来，接
//! 消费者留给挂载链条真正落地的批次，不假装它已经在装载期之外的任何
//! 地方生效。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::ContentIndex;

/// 伤害类别的注册表条目——「物理」「火」「冰」这一类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageCategoryDef {
    /// 这个伤害类别没有被具体分项覆盖时使用的默认公式（十九节，本批次
    /// 不接线，见模块文档「本批次范围」一节）——`None` 表示不声明类别
    /// 默认，继续下探到全局默认。
    pub default_formula: Option<ContentIndex>,
}

/// 伤害类别注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageCategoryError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::race::RaceError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for DamageCategoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DamageCategoryError::DuplicateDefinition(index) => {
                write!(f, "伤害类别索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for DamageCategoryError {}

/// 伤害类别定义表：`ContentIndex`（类别自身的命名空间标识符）→
/// [`DamageCategoryDef`]，理由见模块文档「为什么是 `BTreeMap`」一节。
#[derive(Debug, Default, Clone)]
pub struct DamageCategoryTable {
    entries: BTreeMap<ContentIndex, DamageCategoryDef>,
}

impl DamageCategoryTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：登记一条伤害类别定义。
    pub fn define(
        &mut self,
        index: ContentIndex,
        def: DamageCategoryDef,
    ) -> Result<(), DamageCategoryError> {
        if self.entries.contains_key(&index) {
            return Err(DamageCategoryError::DuplicateDefinition(index));
        }
        self.entries.insert(index, def);
        Ok(())
    }

    /// 查询一条伤害类别定义，未注册返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, index: ContentIndex) -> Option<&DamageCategoryDef> {
        self.entries.get(&index)
    }

    /// 给定的伤害类别索引当前是否已经登记过定义——供
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
    fn 定义后可以查到同一条伤害类别() {
        // Arrange
        let mut interner = Interner::new();
        let index = index(&mut interner, "lostland:fire");
        let mut table = DamageCategoryTable::new();

        // Act
        table
            .define(
                index,
                DamageCategoryDef {
                    default_formula: None,
                },
            )
            .expect("首次定义应当成功");

        // Assert
        assert!(table.get(index).is_some());
        assert!(table.is_defined(index));
    }

    #[test]
    fn 重复定义同一个伤害类别索引返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let index = index(&mut interner, "lostland:physical");
        let mut table = DamageCategoryTable::new();
        table
            .define(
                index,
                DamageCategoryDef {
                    default_formula: None,
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            DamageCategoryDef {
                default_formula: None,
            },
        );

        // Assert
        assert_eq!(result, Err(DamageCategoryError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的伤害类别索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let table = DamageCategoryTable::new();

        // Act & Assert
        assert_eq!(table.get(never_defined), None);
        assert!(!table.is_defined(never_defined));
    }
}
