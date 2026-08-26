//! `resolve` 侧需要的最小「伤害类别目录」接口——落地
//! `knowledge/design/damage-formula-mod-api.md` 十七节武器类别/伤害
//! 类别设计里,伤害类别与 `resolve` 唯一真实的交汇点：查询"武器没有
//! 显式声明伤害类别时,这一下攻击该算哪个类别"。
//!
//! # 为什么只有"默认类别"这一个方法,不是完整的四层默认解析
//!
//! 十九节设计的完整四层挂载链条（分项自身 → 伤害类别默认公式 → 武器
//! 类别默认公式 → 全局默认公式）服务的是"这一分项该用哪条公式"这个
//! 问题——本批次任务书明确排除"分项相加变成伤害列表"（`DamageComponent`
//! 尚未落地,见 `crates/ll-mod/src/weapon_category.rs`/
//! `crates/ll-mod/src/damage_category.rs` 模块文档「本批次范围」
//! 一节),因此四层解析链条本身不在本批次的接线范围内——`resolve_attack`
//! 仍然只用 `crate::formula::DamageFormulaCatalog` 现有的两层
//! （显式引用 → 全局默认公式）挑公式,伤害类别与"挑哪条公式"这件事
//! 完全无关。
//!
//! `resolve_attack` 唯一需要伤害类别的地方是抗性查询
//! （`crate::traits::resistance_multiplier_permille`）——武器没有显式
//! 声明伤害类别时,查询默认伤害类别不是为了挑公式,而是为了知道"这一
//! 下该查哪个类别的抗性",本 trait 因此只暴露这一个方法,不是完整搬运
//! `ll_mod::damage_category::DamageCategoryTable` 的全部查询能力
//! ——与 [`crate::item::ItemCatalog`]/[`crate::formula::DamageFormulaCatalog`]
//! 同一套"只收敛 resolve 真正要读的字段/方法"依赖倒置手法。

use ll_core::ident::ContentIndex;

/// `resolve` 依赖的最小「伤害类别目录」接口——真正的
/// `DamageCategoryTable` 定义在下游的 `ll-mod`（依赖方向，规格 §5）。
pub trait DamageCategoryCatalog {
    /// 武器/技能没有显式声明伤害类别时使用的默认伤害类别——与
    /// [`crate::formula::DamageFormulaCatalog::formula_for`] 的
    /// `explicit` 参数是同一层"两层下探"的另一半：这里没有 `explicit`
    /// 参数是因为调用方（`resolve_attack`）自己先做
    /// `explicit.unwrap_or_else(|| categories.default_category())`
    /// 这一步判断,本 trait 只负责"没有显式声明时该用哪个"，不重复
    /// `Option` 的解包逻辑。
    fn default_category(&self) -> ContentIndex;
}

/// 空伤害类别目录：默认类别恒为 [`ContentIndex::default()`]——理由同
/// [`crate::formula::NoFormulas`]：调用方没有接好真正的
/// `ll_mod::damage_category::RegistryDamageCategories`（多数只测试移动/
/// 开门这类不涉及内容注册表的既有测试场景）时的保底实现。
///
/// # 这个值**不是**一个保留哨兵——此前这里写错了
///
/// 本段原文是「`ContentIndex::default()` 与任何真实注册的伤害类别都不会
/// 撞上（`Registry::intern` 从 1 开始分配）」。**那句话是错的**：
/// [`ll_core::ident::Interner::intern`] 分配的索引就是插入顺序下标，
/// 从 **0** 开始，`ContentIndex::default()` 因此正是**第一个被 intern
/// 的那条内容**（见 `ll_core::ident::ContentIndex::default` 文档原文
/// 「索引 `0` 在不同会话、不同 mod 组合下可能对应完全不同的内容」）。
///
/// 它在实践中仍然表现成"查不到任何匹配的抗性"，但靠的是另一件事：
/// 生产装载路径（`ll_game::content::load_content`）最先 intern 的是
/// **本体地形**（`register_base_terrain`），伤害类别排在它之后，因此
/// 没有任何伤害类别拿得到索引 0。这是一条**依赖装载顺序**的保证，
/// 不是类型层面的保证——真正的修复不是加固这个空实现，而是让生产路径
/// 根本不用它，那正是 `RegistryDamageCategories` 落地要做的事。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDamageCategories;

impl DamageCategoryCatalog for NoDamageCategories {
    fn default_category(&self) -> ContentIndex {
        ContentIndex::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空伤害类别目录的默认类别是content_index的默认值() {
        // Arrange
        let catalog = NoDamageCategories;

        // Act & Assert
        assert_eq!(catalog.default_category(), ContentIndex::default());
    }
}
