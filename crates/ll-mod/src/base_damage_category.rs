//! 本体默认伤害类别注册——`knowledge/design/damage-formula-mod-api.md`
//! 十九节「没有声明任何分项时……三个本体默认伤害类别」的落地，退化到
//! 本批次没有分项列表的模型时，就是「资源全局唯一的一个默认类别」这
//! 一层：本体武器不显式声明 `damage_category` 时，`resolve_attack` 退回
//! 这一个索引，见 [`ll_sim::damage_category::DamageCategoryCatalog`]
//! 文档。
//!
//! # 为什么这里只注册一个（`lostland:physical`），不是三个
//!
//! 十九节原文的三个本体默认类别（`lostland:physical`/`lostland:magic`/
//! `lostland:spirit`）分别对应 `DamageSchool` 的三个变体——但
//! `DamageSchool` 本身在当前代码库里还不存在（`crates/ll-world/src/item.rs`
//! 「为什么不是只有 `AttributeKind` 一种取值」一节已核实：`resolve_attack`
//! 本批次仍是纯物理近战占位实现，三轴战斗结算本身是后续批次的工作）。
//! 本模块因此只注册与当前唯一存在的战斗形态（纯物理近战）对应的一个
//! 默认类别，不预先注册另外两个没有任何路径能产出"魔法伤害"/"精神
//! 伤害"的死类别——`DamageSchool` 真正落地时，照本模块的先例各加一个
//! 即可，与 `ll_world::item::StatTarget` 文档「为什么不现在就加魔抗/
//! 意志抗性两个变体」同一条 YAGNI 判断。
//!
//! 上面这段论证**至今一字未改地成立**，但它论证的是「魔法/精神这两个
//! 特定类别不注册」，不是「本体只能有一个伤害类别」。本体的第二个
//! 伤害类别 `lostland:fire` 已经落地——它走的是**内容数据文件**那条
//! 通道（`mods/lostland/damage_categories.json5`），不在本模块里，
//! 因为它不需要引擎在 `load_all` 之前就知道它：
//!
//! * 本模块注册的这一个之所以必须走 Rust 侧，是因为它同时是
//!   [`ll_sim::damage_category::DamageCategoryCatalog::default_category`]
//!   ——「没有任何声明时退回哪一类」这个问题必须在任何 mod 装载之前
//!   就有答案（同 [`crate::base_damage_formula`]）。
//! * `lostland:fire` 没有这条约束：它只是一个普通类别，与
//!   `mods/example_mod/damage_categories.json5` 的 `examplemod:acid`
//!   走同一条路径、同一套校验，没有任何本体专属通道。这正是「本体即
//!   Mod」想要的形状——本模块的存在是**默认值**这个语义的代价，不是
//!   本体内容的入口。
//!
//! 换句话说：本模块里永远只会有「全局默认」那一个；本体想加第几个
//! 伤害类别，都加在内容数据文件里。
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

/// 本体默认伤害类别的显示名文案键——与
/// [`crate::base_weather::register_base_weathers`] 里那几条一样写成字面
/// 量：引擎侧注册的内容同样要填
/// [`DamageCategoryDef::display_name_key`]（必填字段），没有本体专属的
/// 豁免通道。对应的文案在 `assets/locales/*.ftl` 的
/// `damage_category-physical-display_name`（点号在查表时换成连字符，见
/// `ll_i18n::to_fluent_id`）。
pub const DEFAULT_DAMAGE_CATEGORY_DISPLAY_NAME_KEY: &str =
    "lostland:damage_category.physical.display_name";

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
            display_name_key: NamespacedId::parse(DEFAULT_DAMAGE_CATEGORY_DISPLAY_NAME_KEY)
                .expect("本体默认类别显示名键字面量恒合法"),
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
        // 显示名键与其余伤害类别走同一个必填字段，引擎侧注册没有豁免。
        assert_eq!(
            table
                .get(index)
                .map(|def| def.display_name_key.to_string())
                .as_deref(),
            Some(DEFAULT_DAMAGE_CATEGORY_DISPLAY_NAME_KEY)
        );
    }
}
