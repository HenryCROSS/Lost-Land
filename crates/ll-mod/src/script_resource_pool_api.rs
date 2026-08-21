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
use ll_sim::resource_pool::{RegenRule, ResourcePoolShape, RestRecoveryAmount};

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

/// `(register-resource-pool id display-name-key shape tier-count regen-kind regen-amount)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
/// - `shape`：池的形状——`"scalar"`（标量池：法力、耐力、气……）或
///   `"tiered-slots"`（法术位，法术位落地批次新增）。
/// - `tier-count`：`shape` 为 `"scalar"` 时忽略（惯例填 0）；
///   `"tiered-slots"` 时是这个池声明了几档（1..=255，档位从 1 起编号，
///   见 `ll_sim::resource_pool::ResourcePoolShape::TieredSlots` 文档）。
/// - `regen-kind`：恢复节奏——`"none"`（不自动恢复）/
///   `"on-turn-start"`（每回合恢复固定量）/`"on-rest-full"`（休息完成时
///   回满，法术位落地批次新增）/`"on-rest-amount"`（休息完成时回固定
///   量，同样新增）。
/// - `regen-amount`：`regen-kind` 为 `"on-turn-start"`/`"on-rest-amount"`
///   时是恢复的数量，非负整数；其余两种恢复节奏忽略这个参数。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_resource_pool(
    id: String,
    display_name_key: String,
    shape: String,
    tier_count: i64,
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
                tier_count,
                &regen_kind,
                regen_amount,
            )
        })
    })
}

/// [`register_resource_pool`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_resource_pool(
    registry: &mut Registry,
    table: &mut ResourcePoolTable,
    id: &str,
    display_name_key: &str,
    shape: &str,
    tier_count: i64,
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
        "tiered-slots" => {
            // 钳位到 u8 范围（1..=255）——0 或负数不是合法档位数（一个
            // 没有任何档位的法术位池毫无意义），直接拒绝而不是静默钳位
            // 成 1,那会掩盖内容作者笔误传了 0 的错误。
            if tier_count < 1 || tier_count > i64::from(u8::MAX) {
                return Err(format!("法术位档位数 {tier_count} 超出合法范围（1..=255）"));
            }
            ResourcePoolShape::TieredSlots {
                tier_count: tier_count as u8,
            }
        }
        _ => {
            return Err(format!(
                "未知的资源池形状 {shape:?}（支持 \"scalar\"/\"tiered-slots\"）"
            ));
        }
    };
    let regen_rule = match regen_kind {
        "none" => RegenRule::None,
        "on-turn-start" => RegenRule::OnTurnStart {
            amount: regen_amount.max(0) as u32,
        },
        "on-rest-full" => RegenRule::OnRest {
            amount: RestRecoveryAmount::Full,
        },
        "on-rest-amount" => RegenRule::OnRest {
            amount: RestRecoveryAmount::Amount(regen_amount.max(0) as u32),
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
    fn 合法标量池声明注册成功并写入资源池表() {
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
            0,
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
    fn 合法法术位声明注册成功且携带正确的档位数() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ResourcePoolTable::new();

        // Act
        let result = do_register_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:wizard_slots",
            "yourmod:pool.wizard_slots",
            "tiered-slots",
            9,
            "on-rest-full",
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:wizard_slots").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的资源池应能查到属性");
        assert_eq!(view.shape, ResourcePoolShape::TieredSlots { tier_count: 9 });
        assert_eq!(
            view.regen_rule,
            RegenRule::OnRest {
                amount: RestRecoveryAmount::Full
            }
        );
    }

    #[test]
    fn 休息回固定量的恢复节奏正确注册() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ResourcePoolTable::new();

        // Act
        let result = do_register_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:druid_slots",
            "yourmod:pool.druid_slots",
            "tiered-slots",
            6,
            "on-rest-amount",
            1,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:druid_slots").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(
            table.get(index).unwrap().regen_rule,
            RegenRule::OnRest {
                amount: RestRecoveryAmount::Amount(1)
            }
        );
    }

    #[test]
    fn 法术位档位数为零时返回错误而不panic() {
        // Arrange：内容作者笔误传了 0——没有任何档位的法术位池毫无意义。
        let mut registry = Registry::new();
        let mut table = ResourcePoolTable::new();

        // Act
        let result = do_register_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:pool.x",
            "tiered-slots",
            0,
            "none",
            0,
        );

        // Assert
        assert!(result.is_err());
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
            "vector",
            0,
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
            0,
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
            r#"(register-resource-pool "yourmod:sorcery_points" "yourmod:pool.sorcery_points" "scalar" 0 "on-turn-start" 2)"#
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
            r#"(register-resource-pool "yourmod:x" "yourmod:pool.x" "vector" 0 "none" 0)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_trait_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
