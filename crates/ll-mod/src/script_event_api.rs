//! 把 `on-event` 注册进**装载期**脚本引擎：mod 借此声明它关心哪些
//! 运行期事件。
//!
//! # 为什么订阅是装载期动作，而事件本身是运行期的
//!
//! 「先声明才回调」是这套机制的性能形状（见 [`crate::event`] 模块文档
//! 「为什么必须先声明才回调」一节）。声明这件事本身没有任何运行期
//! 输入——它只是一张 `(事件种类, 处理函数名)` 的表，因此它属于装载期，
//! 与 `register-*` 那一整套走完全相同的通道：同一个装载期引擎、同一套
//! `thread_local!` 活跃目标、同一条「注册期完整校验」纪律（ADR 0017：
//! 事件种类写错在装载期就报，不留到结算期变成一条永远不触发的订阅）。
//!
//! 处理函数**不在**装载期引擎上被调用——它在结算期由
//! [`crate::script_event_source`] 在另一个引擎上按名字调用。两个引擎的
//! 白名单能力表刻意不兼容（见 `mods/example_mod/mod.json5` 里
//! `entry_points` 上方的注释），因此这里只能记名字，记不了函数值，
//! 理由见 [`crate::event::EventSubscription::handler`] 文档。
//!
//! # 模式同 `crate::script_class_api`
//!
//! `thread_local!` + `RefCell<Option<T>>` 把值整体移进/移出，完整论证
//! 见 `crate::script_terrain_api` 模块文档。唯一的额外一份状态是**当前
//! mod 的命名空间**：订阅方是谁不能由脚本自己说了算（那样 A mod 就能
//! 冒充 B mod 订阅、并以 B 的名义写脚本状态），必须由宿主在打开调用
//! 窗口时固化，与 `ll_script::api::state::register` 固化
//! `mod_namespace` 是同一条纪律。

use std::cell::RefCell;

use ll_script::host::ScriptEngine;

use crate::event::{EventSubscription, EventSubscriptionTable, GameEventKind};

thread_local! {
    /// 当前调用窗口内，`on-event` 应该写入的订阅表。
    static ACTIVE_TABLE: RefCell<Option<EventSubscriptionTable>> = const { RefCell::new(None) };

    /// 当前调用窗口属于哪个 mod——见模块文档最后一段。
    static ACTIVE_NAMESPACE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `on-event` 可写入的目标，并固化
/// 「这个窗口属于哪个 mod」。
pub fn set_active_target(table: EventSubscriptionTable, mod_namespace: impl Into<String>) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
    ACTIVE_NAMESPACE.with(|cell| *cell.borrow_mut() = Some(mod_namespace.into()));
}

/// 取回 [`set_active_target`] 放进去的订阅表，调用约定同
/// `script_terrain_api::take_active_target`。
pub fn take_active_target() -> EventSubscriptionTable {
    ACTIVE_NAMESPACE.with(|cell| cell.borrow_mut().take());
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `on-event` 注册进 `engine`。
pub fn register_event_api(engine: &mut ScriptEngine) {
    engine.register_fn("on-event", on_event);
}

/// `(on-event event-kind handler-name)`。
///
/// - `event-kind`：`"damaged"`/`"killed"`/`"experience-gained"` 三选一
///   （见 [`GameEventKind`]）。写别的会当场报错并列出全部合法取值。
/// - `handler-name`：本 mod 脚本里 `define` 出来的**零参**函数名。事件
///   数据不走参数，走 `(event-*)` 一族查询函数——理由见
///   [`crate::script_event_source`] 模块文档「为什么 payload 走查询
///   函数而不是参数」一节。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
///
/// # 这里不校验「这个函数真的存在」
///
/// 装载期引擎与结算期引擎不是同一个，处理函数是在结算期引擎上按名字
/// 查的；装载期引擎的符号表里有没有这个名字，**说明不了**结算期查不
/// 查得到（同一个 mod 的多份脚本共享一个装载期引擎，但结算期引擎是
/// 另外重建的）。真正的「这个名字查不到」由
/// [`crate::script_event_source::ScriptEventSource::new`] 在结算期引擎
/// 建好之后一次性校验——那里才是**能**回答这个问题的地方，把校验放在
/// 这里只会得到一条既漏报又误报的假检查。
fn on_event(event_kind: String, handler_name: String) -> Result<bool, String> {
    ACTIVE_TABLE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let Some(table) = slot.as_mut() else {
            return Err("on-event 在没有活跃事件订阅表的窗口内被调用".to_string());
        };
        let namespace = ACTIVE_NAMESPACE
            .with(|ns| ns.borrow().clone())
            .ok_or_else(|| "on-event 在没有活跃 mod 命名空间的窗口内被调用".to_string())?;
        do_on_event(table, &namespace, &event_kind, &handler_name)
    })
}

/// [`on_event`] 的纯函数核心，方便单元测试不必绕过 `thread_local!`。
fn do_on_event(
    table: &mut EventSubscriptionTable,
    mod_namespace: &str,
    event_kind: &str,
    handler_name: &str,
) -> Result<bool, String> {
    let kind = GameEventKind::parse(event_kind).ok_or_else(|| {
        crate::event::EventSubscriptionError::UnknownKind {
            raw: event_kind.to_string(),
        }
        .to_string()
    })?;
    table
        .subscribe(EventSubscription {
            mod_namespace: mod_namespace.to_string(),
            kind,
            handler: handler_name.to_string(),
        })
        .map(|()| true)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法订阅登记成功并写入订阅表() {
        // Arrange
        let mut table = EventSubscriptionTable::new();

        // Act
        let result = do_on_event(&mut table, "examplemod", "killed", "on-kill");

        // Assert
        assert_eq!(result, Ok(true));
        assert_eq!(
            table.all(),
            &[EventSubscription {
                mod_namespace: "examplemod".to_string(),
                kind: GameEventKind::Killed,
                handler: "on-kill".to_string(),
            }]
        );
    }

    #[test]
    fn 未知事件种类当场报错并列出合法取值() {
        // ADR 0017：装载期就报，不留到结算期变成一条永远不触发的订阅。
        // Arrange
        let mut table = EventSubscriptionTable::new();

        // Act
        let result = do_on_event(&mut table, "examplemod", "moved", "on-move");

        // Assert
        let message = result.expect_err("未知事件种类必须失败");
        assert!(message.contains("moved"));
        assert!(message.contains("killed"));
        assert!(table.all().is_empty(), "失败的订阅不得留下任何痕迹");
    }

    #[test]
    fn 订阅方命名空间由宿主固化脚本参数里没有它() {
        // 这条守的是「A mod 不能冒充 B mod 订阅」：`on-event` 的脚本
        // 签名只有两个参数，命名空间不在其中。
        // Arrange
        let mut table = EventSubscriptionTable::new();

        // Act
        do_on_event(&mut table, "modb", "damaged", "on-damage").expect("登记应当成功");

        // Assert
        assert_eq!(table.all()[0].mod_namespace, "modb");
    }

    #[test]
    fn 重复订阅同一条返回错误() {
        // Arrange
        let mut table = EventSubscriptionTable::new();
        do_on_event(&mut table, "examplemod", "killed", "on-kill").expect("首次登记应当成功");

        // Act
        let result = do_on_event(&mut table, "examplemod", "killed", "on-kill");

        // Assert
        assert!(result.is_err());
        assert_eq!(table.all().len(), 1);
    }

    #[test]
    fn 没有活跃订阅表时调用报错而不是panic() {
        // 与其余 register-* 同一条纪律：脚本永远不该把宿主打崩。
        // Arrange：不调用 set_active_target。

        // Act
        let result = on_event("killed".to_string(), "on-kill".to_string());

        // Assert
        assert!(result.is_err());
    }
}
