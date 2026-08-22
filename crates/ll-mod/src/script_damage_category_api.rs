//! 把 `register-damage-category` 注册进脚本引擎：落地
//! `knowledge/design/damage-formula-mod-api.md` 二十一节。
//!
//! 与 [`crate::script_terrain_api`]/[`crate::script_damage_formula_api`]
//! 同一套 `thread_local!` + `ACTIVE_TABLE` 手法，模块文档不重复其论证。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::damage_category::{DamageCategoryDef, DamageCategoryError, DamageCategoryTable};
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-damage-category` 应该写入的伤害类别表。
    static ACTIVE_TABLE: RefCell<Option<DamageCategoryTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内可写入的目标。
pub fn set_active_target(table: DamageCategoryTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 [`DamageCategoryTable`]。
pub fn take_active_target() -> DamageCategoryTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-damage-category` 注册进 `engine`。
pub fn register_damage_category_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-damage-category", register_damage_category);
}

/// `(register-damage-category id default-formula-id)`——设计文档二十一
/// 节：`default-formula-id` 传空字符串 `""` 表示不声明类别默认（十九节
/// 第 2 层落空，本批次这个字段本身不接线，见 `crate::damage_category`
/// 模块文档「本批次范围」一节，但注册期仍按设计文档原文完整校验）——
/// 与 `register-terrain` 的 `opens-into` 空串哨兵是同一个既有约定。
///
/// `default-formula-id` 非空时必须已经通过 `register-damage-formula`
/// 注册过——查不到即拒绝整个 mod 加载（设计文档二十一节原文），与
/// [`crate::script_item_api`] 的 `register-item-damage-formula` 对
/// `formula-id` 的校验同一条纪律：本函数不 `intern`，只 `get`。
fn register_damage_category(id: String, default_formula_id: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err(
                    "register-damage-category 在没有活跃伤害类别表的窗口内被调用".to_string(),
                );
            };
            do_register_damage_category(registry, table, &id, &default_formula_id)
        })
    })
}

/// [`register_damage_category`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_damage_category(
    registry: &mut Registry,
    table: &mut DamageCategoryTable,
    id: &str,
    default_formula_id: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let default_formula = if default_formula_id.is_empty() {
        None
    } else {
        let parsed_formula_id = NamespacedId::parse(default_formula_id).map_err(|err| {
            format!("非法 default-formula-id 标识符 {default_formula_id:?}：{err}")
        })?;
        let Some(formula_index) = registry.get(&parsed_formula_id) else {
            return Err(format!(
                "伤害公式 {default_formula_id:?} 尚未通过 register-damage-formula 注册"
            ));
        };
        Some(formula_index)
    };

    table
        .define(index, DamageCategoryDef { default_formula })
        .map(|()| true)
        .map_err(|err: DamageCategoryError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::FormulaTable;
    use crate::script_damage_formula_api::{
        register_damage_formula_api, set_active_target as set_active_formula_target,
        take_active_target as take_active_formula_target,
    };

    #[test]
    fn 合法伤害类别声明注册成功并写入伤害类别表() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_damage_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(DamageCategoryTable::new());

        // Act
        let result =
            engine.load_source(r#"(register-damage-category "yourmod:fire" "")"#.to_string());

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:fire").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.is_defined(index));
    }

    #[test]
    fn 默认公式已注册时伤害类别声明携带对应的默认公式索引() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_damage_formula_api(&mut engine);
        register_damage_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_formula_target(FormulaTable::new());
        set_active_target(DamageCategoryTable::new());
        engine
            .load_source(
                r#"(register-damage-formula "yourmod:fire_default_formula" (quote (d 1 4)))"#
                    .to_string(),
            )
            .expect("公式注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(register-damage-category "yourmod:fire" "yourmod:fire_default_formula")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let formula_table = take_active_formula_target();
        let table = take_active_target();
        let category_index = registry
            .get(&NamespacedId::parse("yourmod:fire").unwrap())
            .expect("刚注册的类别应能查到索引");
        let formula_index = registry
            .get(&NamespacedId::parse("yourmod:fire_default_formula").unwrap())
            .expect("刚注册的公式应能查到索引");
        assert_eq!(
            table.get(category_index).unwrap().default_formula,
            Some(formula_index)
        );
        assert!(formula_table.is_defined(formula_index));
    }

    #[test]
    fn 默认公式未注册时伤害类别声明失败而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_damage_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(DamageCategoryTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-damage-category "yourmod:fire" "yourmod:never_registered_formula")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn 重复定义同一个伤害类别索引返回错误而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_damage_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(DamageCategoryTable::new());
        engine
            .load_source(r#"(register-damage-category "yourmod:fire" "")"#.to_string())
            .expect("首次注册应当成功");

        // Act
        let result =
            engine.load_source(r#"(register-damage-category "yourmod:fire" "")"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
