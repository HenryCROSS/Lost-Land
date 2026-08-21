//! 把 `register-resource-pool` 注册进脚本引擎：mod 脚本借此定义自定义
//! 资源池（法力、气……），落地
//! `knowledge/design/resource-pools-and-rest.md` 二节「注册身份层
//! 统一」。
//!
//! 模式同 [`crate::script_trait_api`]：`shape`/`regen-kind` 没有直接的
//! Steel 表示,用字符串「标签」参数 + 按标签解释的数值参数,理由见
//! [`crate::script_skill_api`] 模块文档「为什么这里多出两处 FFI 转换
//! 上的麻烦」一节同一条既有约定。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use ll_sim::resource_pool::{RegenRule, ResourcePoolShape};

use crate::active_registry::with_active_registry;
use crate::registry::Registry;
use crate::resource_pool::{ResourcePoolAttrs, ResourcePoolError, ResourcePoolTable};

thread_local! {
    /// 当前调用窗口内，`register-resource-pool` 应该写入的资源池表。
    static ACTIVE_TABLE: RefCell<Option<ResourcePoolTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-resource-pool` 可写入的目标。
pub fn set_active_target(table: ResourcePoolTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `ResourcePoolTable`。
pub fn take_active_target() -> ResourcePoolTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-resource-pool` 注册进 `engine`。
pub fn register_resource_pool_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-resource-pool", register_resource_pool);
}

/// `(register-resource-pool id display-name-key shape regen-kind regen-amount)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
/// - `shape`：池的形状——本批次只支持 `"scalar"`（标量池：法力、耐力、
///   气……）。`"tiered-slots"`（法术位）留给下一批，见
///   `ll_sim::resource_pool` 模块文档「本批次范围」一节。
/// - `regen-kind`：恢复节奏——`"none"`（不自动恢复）/
///   `"on-turn-start"`（每回合恢复固定量）。`"on-rest"` 留给休息事件
///   批次。
/// - `regen-amount`：`regen-kind` 为 `"none"` 时忽略；`"on-turn-start"`
///   时是每回合恢复的数量，非负整数。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_resource_pool(
    id: String,
    display_name_key: String,
    shape: String,
    regen_kind: String,
    regen_amount: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-resource-pool 在没有活跃资源池表的窗口内被调用".to_string());
            };
            do_register_resource_pool(
                registry,
                table,
                &id,
                &display_name_key,
                &shape,
                &regen_kind,
                regen_amount,
            )
        })
    })
}

/// [`register_resource_pool`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_resource_pool(
    registry: &mut Registry,
    table: &mut ResourcePoolTable,
    id: &str,
    display_name_key: &str,
    shape: &str,
    regen_kind: &str,
    regen_amount: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;

    let shape = match shape {
        "scalar" => ResourcePoolShape::Scalar,
        _ => {
            return Err(format!(
                "未知的资源池形状 {shape:?}（本批次只支持 \"scalar\"）"
            ));
        }
    };
    let regen_rule = match regen_kind {
        "none" => RegenRule::None,
        "on-turn-start" => RegenRule::OnTurnStart {
            amount: regen_amount.max(0) as u32,
        },
        _ => return Err(format!("未知的恢复节奏 {regen_kind:?}")),
    };

    table
        .define(
            index,
            ResourcePoolAttrs {
                display_name_key,
                shape,
                regen_rule,
            },
        )
        .map(|()| true)
        .map_err(|err: ResourcePoolError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法资源池声明注册成功并写入资源池表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ResourcePoolTable::new();

        // Act
        let result = do_register_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:sorcery_points",
            "yourmod:pool.sorcery_points",
            "scalar",
            "on-turn-start",
            2,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:sorcery_points").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的资源池应能查到属性");
        assert_eq!(view.shape, ResourcePoolShape::Scalar);
        assert_eq!(view.regen_rule, RegenRule::OnTurnStart { amount: 2 });
    }

    #[test]
    fn 未知的资源池形状返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ResourcePoolTable::new();

        // Act
        let result = do_register_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:pool.x",
            "tiered-slots",
            "none",
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 未知的恢复节奏返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ResourcePoolTable::new();

        // Act
        let result = do_register_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:pool.x",
            "scalar",
            "on-rest",
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_resource_pool() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_resource_pool_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ResourcePoolTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-resource-pool "yourmod:sorcery_points" "yourmod:pool.sorcery_points" "scalar" "on-turn-start" 2)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:sorcery_points").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(
            table.get(index).unwrap().regen_rule,
            RegenRule::OnTurnStart { amount: 2 }
        );
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：未知的资源池形状——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_resource_pool_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ResourcePoolTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-resource-pool "yourmod:x" "yourmod:pool.x" "vector" "none" 0)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_trait_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
