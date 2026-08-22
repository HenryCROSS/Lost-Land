//! 把 `register-recipe-category`/`recipe-category-requires-subclass!`
//! 注册进脚本引擎：落地 `knowledge/design/crafting-system.md` 十节①②。
//!
//! 与 [`crate::script_weapon_category_api`] 结构完全同构（同一套
//! `thread_local!` + `ACTIVE_TABLE` 手法），模块文档不重复其论证。
//!
//! # ADR 0020 核对
//!
//! 两个函数的参数只有字符串（命名空间标识符），没有任何浮点，也没有
//! 任何流进世界状态的数值量——不需要 `Milli` 量化。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::recipe_category::{RecipeCategoryError, RecipeCategoryTable};
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-recipe-category` 应该写入的类别表。
    static ACTIVE_TABLE: RefCell<Option<RecipeCategoryTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内可写入的目标。
pub fn set_active_target(table: RecipeCategoryTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 [`RecipeCategoryTable`]。
pub fn take_active_target() -> RecipeCategoryTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把两个配方类别注册函数注册进 `engine`。
pub fn register_recipe_category_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-recipe-category", register_recipe_category);
    engine.register_fn(
        "recipe-category-requires-subclass!",
        recipe_category_requires_subclass,
    );
}

/// `(register-recipe-category id display-name-key)`——登记一个配方类别。
///
/// - `id`：完整命名空间标识符字符串，例如 `"lostland:forging"`。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
///
/// 新登记的类别**不设任何副职闸门**（人人可做）——要设闸门走
/// [`recipe_category_requires_subclass`]，理由见
/// [`crate::recipe_category::RecipeCategoryTable::add_required_subclass`]
/// 文档「为什么是独立函数」一节。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_recipe_category(id: String, display_name_key: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err(
                    "register-recipe-category 在没有活跃配方类别表的窗口内被调用".to_string(),
                );
            };
            do_register_recipe_category(registry, table, &id, &display_name_key)
        })
    })
}

/// [`register_recipe_category`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_recipe_category(
    registry: &mut Registry,
    table: &mut RecipeCategoryTable,
    id: &str,
    display_name_key: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let parsed_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    table
        .define(index, parsed_key)
        .map(|()| true)
        .map_err(|err: RecipeCategoryError| err.to_string())
}

/// `(recipe-category-requires-subclass! category-id subclass-id)`——
/// 给一个已注册的配方类别追加一道副职闸门（any-of，可多次调用）。
///
/// - `category-id`：已经通过 `register-recipe-category` 注册过的完整
///   标识符字符串——目标必须已存在（ADR 0017「注册期完整校验」）。
/// - `subclass-id`：已经通过 `register-subclass` 注册过的完整标识符
///   字符串——**要求已存在**（跨表存在性校验，本函数不 `intern`，
///   只 `get`），不允许静默创建一个指向不存在副职的悬空闸门：那样的
///   闸门谁都过不去，且完全不会报错，是最难查的一类内容 bug。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn recipe_category_requires_subclass(
    category_id: String,
    subclass_id: String,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err(
                    "recipe-category-requires-subclass! 在没有活跃配方类别表的窗口内被调用"
                        .to_string(),
                );
            };
            do_recipe_category_requires_subclass(registry, table, &category_id, &subclass_id)
        })
    })
}

/// [`recipe_category_requires_subclass`] 的纯函数核心。
fn do_recipe_category_requires_subclass(
    registry: &Registry,
    table: &mut RecipeCategoryTable,
    category_id: &str,
    subclass_id: &str,
) -> Result<bool, String> {
    let parsed_category = NamespacedId::parse(category_id)
        .map_err(|err| format!("非法内容标识符 {category_id:?}：{err}"))?;
    let Some(category_index) = registry.get(&parsed_category) else {
        return Err(format!(
            "配方类别 {category_id:?} 尚未通过 register-recipe-category 注册"
        ));
    };
    let parsed_subclass = NamespacedId::parse(subclass_id)
        .map_err(|err| format!("非法副职标识符 {subclass_id:?}：{err}"))?;
    let Some(subclass_index) = registry.get(&parsed_subclass) else {
        return Err(format!(
            "副职 {subclass_id:?} 尚未通过 register-subclass 注册"
        ));
    };

    table
        .add_required_subclass(category_index, subclass_index)
        .map(|()| true)
        .map_err(|err: RecipeCategoryError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_subclass_api::{
        register_subclass_api, set_active_target as set_active_subclass_target,
        take_active_target as take_active_subclass_target,
    };
    use crate::subclass::SubclassTable;

    /// 把线程局部目标恢复成「没有活跃表」，供失败路径的测试收尾。
    fn cleanup() {
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn 合法类别声明注册成功且默认不设闸门() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_recipe_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RecipeCategoryTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-recipe-category "yourmod:cooking" "yourmod:cooking_display_name")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:cooking").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(
            table
                .get(index)
                .expect("已注册")
                .required_subclasses
                .is_empty()
        );
    }

    #[test]
    fn 副职闸门经脚本追加后写进类别表() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_subclass_api(&mut engine);
        register_recipe_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_subclass_target(SubclassTable::new());
        set_active_target(RecipeCategoryTable::new());
        engine
            .load_source(
                r#"(register-subclass "yourmod:artisan" "yourmod:artisan_display_name")"#
                    .to_string(),
            )
            .expect("副职注册应当成功");
        engine
            .load_source(
                r#"(register-recipe-category "yourmod:forging" "yourmod:forging_display_name")"#
                    .to_string(),
            )
            .expect("类别注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(recipe-category-requires-subclass! "yourmod:forging" "yourmod:artisan")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let _subclasses = take_active_subclass_target();
        let forging = registry
            .get(&NamespacedId::parse("yourmod:forging").unwrap())
            .expect("类别已注册");
        let artisan = registry
            .get(&NamespacedId::parse("yourmod:artisan").unwrap())
            .expect("副职已注册");
        assert_eq!(
            table.get(forging).expect("已注册").required_subclasses,
            vec![artisan]
        );
    }

    #[test]
    fn 给未注册的类别追加闸门失败而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_recipe_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RecipeCategoryTable::new());

        // Act
        let result = engine.load_source(
            r#"(recipe-category-requires-subclass! "yourmod:never_defined" "yourmod:artisan")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        cleanup();
    }

    #[test]
    fn 闸门指向未注册的副职失败而不静默创建悬空引用() {
        // 反例：若这里放行，产出的闸门谁都过不去且完全不报错。
        // Arrange
        let mut engine = ScriptEngine::new();
        register_recipe_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RecipeCategoryTable::new());
        engine
            .load_source(
                r#"(register-recipe-category "yourmod:forging" "yourmod:forging_display_name")"#
                    .to_string(),
            )
            .expect("类别注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(recipe-category-requires-subclass! "yourmod:forging" "yourmod:never_defined")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        cleanup();
    }

    #[test]
    fn 重复定义同一个类别索引返回错误而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_recipe_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RecipeCategoryTable::new());
        engine
            .load_source(
                r#"(register-recipe-category "yourmod:cooking" "yourmod:cooking_display_name")"#
                    .to_string(),
            )
            .expect("首次注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(register-recipe-category "yourmod:cooking" "yourmod:cooking_display_name")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        cleanup();
    }
}
