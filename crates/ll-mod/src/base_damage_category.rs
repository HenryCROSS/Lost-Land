//! 本体默认伤害类别注册——`knowledge/design/damage-formula-mod-api.md`
//! 十九节「没有声明任何分项时……三个本体默认伤害类别」的落地，退化到
//! 本批次没有分项列表的模型时，就是「资源全局唯一的一个默认类别」这
//! 一层：本体武器不显式声明 `damage_category` 时，`resolve_attack` 退回
//! 这一个索引，见 [`ll_sim::damage_category::DamageCategoryCatalog`]
//! 文档。
//!
//! # 为什么只注册一个（`lostland:physical`），不是三个
//!
//! 十九节原文的三个本体默认类别（`lostland:physical`/`lostland:magic`/
//! `lostland:spirit`）分别对应 `DamageSchool` 的三个变体——但
//! `DamageSchool` 本身在当前代码库里还不存在（`crates/ll-world/src/item.rs`
//! 「为什么不是只有 `AttributeKind` 一种取值」一节已核实：`resolve_attack`
//! 本批次仍是纯物理近战占位实现，三轴战斗结算本身是后续批次的工作）。
//! 本批次因此只注册与当前唯一存在的战斗形态（纯物理近战）对应的一个
//! 默认类别，不预先注册另外两个没有任何路径能产出"魔法伤害"/"精神
//! 伤害"的死类别——`DamageSchool` 真正落地时，照本模块的先例各加一个
//! 即可，与 `ll_world::item::StatTarget` 文档「为什么不现在就加魔抗/
//! 意志抗性两个变体」同一条 YAGNI 判断。
//!
//! # 为什么走与 [`crate::base_damage_formula`] 完全相同的模式
//!
//! 与「本体即 Mod」既有先例同一条纪律：本体默认类别与 mod 通过
//! `register-damage-category` 声明的类别共用**完全相同**的
//! [`crate::damage_category::DamageCategoryTable::define`] 调用，没有
//! 任何本体专属的特权通道——本模块只是把这次调用挪到 Rust 侧直接执行
//! （本体注册向来不经过脚本管线，见 `crate::pipeline` 模块文档「本体
//! 内容不经过这条管线」一节），不是发明一条新的注册路径。

use ll_core::ident::{ContentIndex, NamespacedId};

use crate::damage_category::{DamageCategoryDef, DamageCategoryError, DamageCategoryTable};

/// 本体默认伤害类别的完整命名空间标识符。
pub const DEFAULT_DAMAGE_CATEGORY_ID: &str = "lostland:physical";

/// 本体默认伤害类别注册的唯一入口：`intern` 是外部传入的解析回调，
/// 理由同 [`crate::base_xp_curve::register_base_xp_curve`] 文档。
pub fn register_base_damage_category(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(ContentIndex, DamageCategoryTable), DamageCategoryError> {
    let mut table = DamageCategoryTable::new();
    let index = intern(
        NamespacedId::parse(DEFAULT_DAMAGE_CATEGORY_ID).expect("本体默认类别 id 字面量恒合法"),
    );
    table.define(
        index,
        DamageCategoryDef {
            default_formula: None,
        },
    )?;
    Ok((index, table))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    #[test]
    fn 本体默认伤害类别注册成功且可查到定义() {
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (index, table) = register_base_damage_category(&mut |id| registry.intern(id))
            .expect("本体默认伤害类别注册恒不失败");

        // Assert
        assert!(table.is_defined(index));
        assert_eq!(
            registry.resolve(index).map(ToString::to_string),
            Some(DEFAULT_DAMAGE_CATEGORY_ID.to_string())
        );
    }
}
