//! 伤害公式定义表：`register-damage-formula` 的存储落点
//! （`knowledge/design/damage-formula-mod-api.md` 三、十九节）。
//!
//! # 为什么不是列式存储（不照抄 `RaceTable`/`ClassTable`）
//!
//! 与 [`crate::xp_curve::XpCurveTable`] 同一条理由（其模块文档「为
//! 什么不是列式存储」一节）：公式的查询频率与一次攻击同数量级——
//! `crate::resolve::resolve_attack`（`ll-sim`）每次攻击查一次
//! `ItemCatalog::item`，本表的查询与它同一个调用点、同一个频率，不是
//! 逐 tick 高频路径（那类路径是 `RaceTable`/`ClassTable` 走列式存储的
//! 判据，ADR 0017）。`BTreeMap<ContentIndex, FormulaDef>` 的 `O(log n)`
//! 查询对这个频率完全足够，不需要为一张公式表引入列式存储的额外复杂
//! 度（YAGNI）。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::ContentIndex;
use ll_sim::formula::{DamageFormulaCatalog, FormulaDef, default_attack_power_instructions};

/// 伤害公式注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaError {
    /// 同一个内容索引被定义了两次。
    DuplicateDefinition(ContentIndex),
}

impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormulaError::DuplicateDefinition(index) => {
                write!(f, "伤害公式索引 {} 被重复定义", index.get())
            }
        }
    }
}

impl std::error::Error for FormulaError {}

/// 伤害公式定义表：`ContentIndex`（公式自身的命名空间标识符）→
/// [`FormulaDef`]。
#[derive(Debug, Default, Clone)]
pub struct FormulaTable {
    formulas: BTreeMap<ContentIndex, FormulaDef>,
}

impl FormulaTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：登记一条公式定义。
    pub fn define(&mut self, index: ContentIndex, def: FormulaDef) -> Result<(), FormulaError> {
        if self.formulas.contains_key(&index) {
            return Err(FormulaError::DuplicateDefinition(index));
        }
        self.formulas.insert(index, def);
        Ok(())
    }

    /// 查询一条公式定义，未注册返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, index: ContentIndex) -> Option<&FormulaDef> {
        self.formulas.get(&index)
    }

    /// 给定的公式索引当前是否已经登记过定义——供
    /// [`crate::content_hash::classify_index`] 判定表归属，也供
    /// [`RegistryFormulas::formula_for`] 判断「武器显式引用的公式是否
    /// 真的存在」。
    pub fn is_defined(&self, index: ContentIndex) -> bool {
        self.formulas.contains_key(&index)
    }
}

/// `ll_sim::resolve::resolve_attack` 消费的真实公式目录：组合
/// [`FormulaTable`]（公式定义）与一个保底的全局默认公式索引——本批次
/// 没有武器类别/伤害类别（任务简报「本批次范围」），四层下探
/// （`damage-formula-mod-api.md` 十九节）因此退化成两层：
///
/// 1. 内容自身显式声明的公式（`explicit`，且必须真的在 `formulas` 里
///    存在——武器引用了一个从未注册过的公式 id，与「未显式声明」一样
///    退回默认，而不是让 `resolve_attack` 卡死或 panic）。
/// 2. 全局默认公式（`default_formula`）。
pub struct RegistryFormulas<'a> {
    /// 公式定义表。
    pub formulas: &'a FormulaTable,
    /// 未显式声明（或显式引用未注册）时的保底公式索引——必须已经在
    /// `formulas` 里定义过，找不到时 [`DamageFormulaCatalog::formula_for`]
    /// 退化到 [`default_attack_power_instructions`]（防御性兜底：装载
    /// 期若真的漏掉默认公式的注册，运行期也不应该 panic，与
    /// `crate::xp_curve::RegistryXpCurves::curve_for` 同一条既有纪律）。
    pub default_formula: ContentIndex,
}

impl DamageFormulaCatalog for RegistryFormulas<'_> {
    fn formula_for(&self, explicit: Option<ContentIndex>) -> FormulaDef {
        let resolved = explicit
            .filter(|id| self.formulas.is_defined(*id))
            .unwrap_or(self.default_formula);
        self.formulas
            .get(resolved)
            .cloned()
            .unwrap_or_else(|| FormulaDef {
                id: ContentIndex::default(),
                instructions: default_attack_power_instructions(),
                needs_rng: false,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_sim::formula::{FormulaOp, FormulaOperand, eval_formula};

    fn distinct_indices(count: usize) -> Vec<ContentIndex> {
        let mut interner = Interner::new();
        (0..count)
            .map(|i| interner.intern(NamespacedId::parse(&format!("test:slot_{i}")).unwrap()))
            .collect()
    }

    fn sample_formula(const_value: i64) -> FormulaDef {
        FormulaDef {
            id: ContentIndex::default(),
            instructions: vec![FormulaOp::Ref(FormulaOperand::Const(const_value))],
            needs_rng: false,
        }
    }

    #[test]
    fn 定义后可以查到同一条公式() {
        // Arrange
        let mut table = FormulaTable::new();
        let [index] = distinct_indices(1)[..] else {
            unreachable!()
        };

        // Act
        table
            .define(index, sample_formula(7))
            .expect("首次定义应当成功");

        // Assert
        assert_eq!(table.get(index).unwrap().instructions.len(), 1);
    }

    #[test]
    fn 重复定义同一个公式索引返回错误() {
        // Arrange
        let mut table = FormulaTable::new();
        let [index] = distinct_indices(1)[..] else {
            unreachable!()
        };
        table
            .define(index, sample_formula(7))
            .expect("首次定义应当成功");

        // Act
        let result = table.define(index, sample_formula(9));

        // Assert
        assert_eq!(result, Err(FormulaError::DuplicateDefinition(index)));
    }

    #[test]
    fn 显式引用已注册的公式时优先于默认公式() {
        // Arrange
        let mut table = FormulaTable::new();
        let indices = distinct_indices(2);
        let [explicit, default] = indices[..] else {
            unreachable!()
        };
        table
            .define(explicit, sample_formula(42))
            .expect("定义应当成功");
        table
            .define(default, sample_formula(1))
            .expect("定义应当成功");
        let resolver = RegistryFormulas {
            formulas: &table,
            default_formula: default,
        };

        // Act
        let def = resolver.formula_for(Some(explicit));

        // Assert：显式公式（42）胜出，不是默认公式（1）。
        let mut rng = ll_core::rng::DetRng::for_entity(1, 2, 3);
        assert_eq!(
            eval_formula(
                &def,
                &ll_sim::formula::FormulaInputs::new(0, 0, 0, 0, [0; 7], false),
                &mut rng
            ),
            42
        );
    }

    #[test]
    fn 显式引用指向不存在的公式时退回默认公式() {
        // Arrange：武器声明了一个从未真正注册过的公式 id——与「未显式
        // 声明」同一种退化，不让 resolve_attack 卡死。
        let mut table = FormulaTable::new();
        let indices = distinct_indices(2);
        let [never_registered, default] = indices[..] else {
            unreachable!()
        };
        table
            .define(default, sample_formula(1))
            .expect("定义应当成功");
        let resolver = RegistryFormulas {
            formulas: &table,
            default_formula: default,
        };

        // Act
        let def = resolver.formula_for(Some(never_registered));

        // Assert
        let mut rng = ll_core::rng::DetRng::for_entity(1, 2, 3);
        assert_eq!(
            eval_formula(
                &def,
                &ll_sim::formula::FormulaInputs::new(0, 0, 0, 0, [0; 7], false),
                &mut rng
            ),
            1
        );
    }

    #[test]
    fn 未显式声明时使用默认公式() {
        // Arrange
        let mut table = FormulaTable::new();
        let [default] = distinct_indices(1)[..] else {
            unreachable!()
        };
        table
            .define(default, sample_formula(1))
            .expect("定义应当成功");
        let resolver = RegistryFormulas {
            formulas: &table,
            default_formula: default,
        };

        // Act
        let def = resolver.formula_for(None);

        // Assert
        let mut rng = ll_core::rng::DetRng::for_entity(1, 2, 3);
        assert_eq!(
            eval_formula(
                &def,
                &ll_sim::formula::FormulaInputs::new(0, 0, 0, 0, [0; 7], false),
                &mut rng
            ),
            1
        );
    }
}
