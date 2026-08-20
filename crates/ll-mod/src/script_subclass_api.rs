//! 把 `register-subclass` 注册进脚本引擎：mod 脚本借此定义自定义副职。
//!
//! 模式与 [`crate::script_class_api`] 完全相同（`SubclassDef` 本身比
//! `ClassDef` 更简单，只有 `id`/`display_name_key` 两个字段，见
//! `crate::subclass` 模块文档「裁定 P5-4」一节），因此本模块只是把同一
//! 套接线换一张表。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::registry::Registry;
use crate::subclass::{SubclassAttrs, SubclassError, SubclassTable};

thread_local! {
    /// 当前调用窗口内，`register-subclass` 应该写入的副职表。
    static ACTIVE_TABLE: RefCell<Option<SubclassTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-subclass` 可写入的目标。
pub fn set_active_target(table: SubclassTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `SubclassTable`。
pub fn take_active_target() -> SubclassTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-subclass` 注册进 `engine`。
pub fn register_subclass_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-subclass", register_subclass);
}

/// `(register-subclass id display-name-key)`。
///
/// - `id`：完整命名空间标识符字符串，如 `"yourmod:shadowdancer"`。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_subclass(id: String, display_name_key: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-subclass 在没有活跃副职表的窗口内被调用".to_string());
            };
            do_register_subclass(registry, table, &id, &display_name_key)
        })
    })
}

/// [`register_subclass`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_subclass(
    registry: &mut Registry,
    table: &mut SubclassTable,
    id: &str,
    display_name_key: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;

    table
        .define(index, SubclassAttrs { display_name_key })
        .map(|()| true)
        .map_err(|err: SubclassError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法副职声明注册成功并写入副职表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();

        // Act
        let result = do_register_subclass(
            &mut registry,
            &mut table,
            "yourmod:shadowdancer",
            "yourmod:shadowdancer_display_name",
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:shadowdancer").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.get(index).is_some());
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();

        // Act
        let result = do_register_subclass(&mut registry, &mut table, "Not Valid", "yourmod:x");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_subclass() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_subclass_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(SubclassTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-subclass "yourmod:shadowdancer" "yourmod:shadowdancer_display_name")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:shadowdancer").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.get(index).is_some());
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：非法命名空间——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_subclass_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(SubclassTable::new());

        // Act
        let result =
            engine.load_source(r#"(register-subclass "Not Valid" "yourmod:x")"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_terrain_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
