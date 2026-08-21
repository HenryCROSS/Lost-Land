//! 把 `register-item` 注册进脚本引擎：mod 脚本借此定义自定义物品
//! （箭矢、铁剑……），落地 `knowledge/design/item-system.md`。
//!
//! 模式同 [`crate::script_resource_pool_api`]：扁平参数,没有为
//! `Option<i32>`（`max-durability`）或 `Milli`（`base-weight`/
//! `base-price`）发明任何新的 FFI 编码方式,理由见下面两节。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_core::scaled::Milli;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::item::{ItemAttrs, ItemError, ItemTable};
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-item` 应该写入的物品表。
    static ACTIVE_TABLE: RefCell<Option<ItemTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-item` 可写入的目标。
pub fn set_active_target(table: ItemTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `ItemTable`。
pub fn take_active_target() -> ItemTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-item` 注册进 `engine`。
pub fn register_item_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-item", register_item);
}

/// `(register-item id display-name-key stack-limit base-weight base-price max-durability)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
/// - `stack-limit`：堆叠上限，必须 ≥ 1（`0` 没有意义——一堆连一个都
///   装不下的物品不该存在，直接拒绝而不是静默钳位成 1，理由同
///   `register-resource-pool` 拒绝 `tier-count == 0` 的文档）。`1`
///   表示不可堆叠。
/// - `base-weight`/`base-price`：以 `Milli` 千分之一为单位的**原始**
///   整数——`Milli(1_500)` 表示 1.5，这里的参数就是 `1500`,不是
///   "整数会被自动乘 1000"那种写法，与 `Milli` 自身文档「`Milli(1_500)`
///   表示 1.5」同一个换算关系，没有为它另外发明一层"填整数、内部
///   放大"的转换（那会让内容作者搞不清一个数字究竟是"1.5"还是
///   "填 1 会自动变 1000"，读脚本时也看不出来）。
/// - `max-durability`：耐久上限，`-1` 表示这件物品没有耐久概念
///   （`None`），`>= 0` 表示有（`Some`）——与 `register-terrain` 的
///   `opens-into` 用空串表示 `None` 是同一条"用一个该字段合法值域之外
///   的哨兵表示空"的既有约定，只是这里的字段是数值,空串哨兵不适用，
///   改用负数（耐久上限本身不该是负的，`-1` 因此是安全的哨兵）。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item(
    id: String,
    display_name_key: String,
    stack_limit: i64,
    base_weight: i64,
    base_price: i64,
    max_durability: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item(
                registry,
                table,
                &id,
                &display_name_key,
                stack_limit,
                base_weight,
                base_price,
                max_durability,
            )
        })
    })
}

/// [`register_item`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_item(
    registry: &mut Registry,
    table: &mut ItemTable,
    id: &str,
    display_name_key: &str,
    stack_limit: i64,
    base_weight: i64,
    base_price: i64,
    max_durability: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;

    if stack_limit < 1 {
        return Err(format!("堆叠上限 {stack_limit} 非法（必须 >= 1）"));
    }
    let max_durability = match max_durability {
        -1 => None,
        value if value >= 0 => Some(value as i32),
        other => {
            return Err(format!(
                "耐久上限 {other} 非法（必须 >= 0，或用 -1 表示无耐久）"
            ));
        }
    };

    table
        .define(
            index,
            ItemAttrs {
                display_name_key,
                stack_limit: stack_limit as u32,
                base_weight: Milli(base_weight),
                base_price: Milli(base_price),
                max_durability,
            },
        )
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法可堆叠物品声明注册成功并写入物品表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:arrow").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的物品应能查到属性");
        assert_eq!(view.stack_limit, 99);
        assert_eq!(view.base_price, Milli(2000));
        assert_eq!(view.max_durability, None);
    }

    #[test]
    fn 合法不可堆叠物品声明携带耐久上限() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:iron_sword",
            "yourmod:item.iron_sword",
            1,
            3000,
            50000,
            100,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:iron_sword").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的物品应能查到属性");
        assert_eq!(view.stack_limit, 1);
        assert_eq!(view.max_durability, Some(100));
    }

    #[test]
    fn 堆叠上限为零时返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:item.x",
            0,
            0,
            0,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 耐久上限小于负一时返回错误而不panic() {
        // Arrange：-2 不是合法的"无耐久"哨兵（只有 -1 是）。
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:item.x",
            1,
            0,
            0,
            -2,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "InvalidNamespace:foo",
            "yourmod:item.foo",
            1,
            0,
            0,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 重复定义同一个物品索引返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        )
        .expect("首次注册应当成功");

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-item "yourmod:arrow" "yourmod:item.arrow" 99 50 2000 -1)"#.to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:arrow").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).unwrap().stack_limit, 99);
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：堆叠上限为零——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());

        // Act
        let result = engine
            .load_source(r#"(register-item "yourmod:x" "yourmod:item.x" 0 0 0 -1)"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_trait_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
