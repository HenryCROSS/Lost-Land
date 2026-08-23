//! 运行期事件负载：`event-kind`/`event-actor`/`event-target`/
//! `event-amount` 四个零参查询原语。
//!
//! # 这是「被告知刚刚发生了什么」，不是「被问这一回合做什么」
//!
//! [`crate::api::actor`] 那一套服务的是行为树：宿主问「这个实体这一
//! 回合做什么」，脚本答一个意图。本模块服务的是**事件监听**：宿主告诉
//! 脚本「刚刚有一条效果要落地了」，脚本可以据此产出自己的反应。两者
//! 方向相反，活跃指针也各自独立——一次事件回调期间，`ACTIVE_EVENT` 有
//! 值而 `ACTIVE_ACTOR` 没有（事件没有「正在决策的实体」这个概念，
//! 只有「这条事件牵涉到谁」）。
//!
//! # 为什么 payload 走零参查询函数，不走处理函数的参数
//!
//! 三条理由，按重要性排列：
//!
//! 1. **不同事件种类的字段不一样**。`damaged` 有目标与伤害量，`killed`
//!    有目标与击杀者，`experience-gained` 有目标与经验量。若走参数，
//!    要么每种事件一个不同的元数（那处理函数就不能被复用、订阅表也要
//!    记住每个处理函数的元数），要么统一成一个最长的元数、缺的填哨兵
//!    （那就是 `register-race` 那种「13 个裸参数」的老路，本项目已经
//!    明确记过那条教训）。
//! 2. **加字段不破坏既有脚本**。新增一个 `(event-cause)` 只是多一个
//!    从未被旧脚本调用过的函数；给处理函数加一个位置参数会让每一份
//!    已发货的脚本当场编译失败。
//! 3. **与既有的行为树原语同构**。`self-handle`/`nearby-enemy` 也都是
//!    零参查询活跃指针，脚本作者不需要学第二套约定。
//!
//! # 调用约定
//!
//! 与 [`crate::api::rng`]/[`crate::api::actor`] 逐字相同：宿主在调用
//! 处理函数之前 [`set_active_event`]，调用窗口结束后
//! [`clear_active_event`]。不清空会让下一次忘记设置的调用悄悄读到上
//! 一条事件的数据。[`with_active_event_for`] 把这一对包成一个安全
//! 函数，调用方应当优先用它。
//!
//! # 没有活跃事件时返回哨兵值，不 panic
//!
//! 与本 crate 全部查询原语同一条降级纪律：宿主接线可能有 bug，脚本
//! 不该因此把进程打崩。`event-kind` 返回空串，两个句柄查询返回 `#f`，
//! `event-amount` 返回 `0`。

use std::cell::RefCell;

use steel::rvals::{IntoSteelVal, SteelVal};

use ll_world::entity::EntityId;

use crate::api::handle::ScriptEntityHandle;
use crate::host::ScriptEngine;

/// 一条运行期事件的全部负载——**朴素数据**（约束 C2）：只有
/// `&'static str` 与整数、[`EntityId`]，没有引用、闭包或裸指针。
///
/// `ll-script` 刻意不认识 `ll_sim::effect::Effect`：把哪一条效果翻译
/// 成哪一种事件负载是**调用方**的职责（`ll_mod::script_event_source`），
/// 与 [`crate::api::intent::parse_intent`]「脚本层只产出/消费数据，
/// 包装成 `Effect`/`Intent` 是调用方的事」是同一条既有分工。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventPayload {
    /// 事件种类的字符串形式，与 mod 在 `on-event` 里写的那个字符串
    /// 逐字相同——处理函数可以用它区分自己被哪一种事件叫醒（一个函数
    /// 订阅多种事件是合法的）。
    pub kind: &'static str,
    /// 事件的**发起方**：击杀者、伤害来源。没有发起方（环境伤害、
    /// 坠落致死）时为 `None`。
    pub actor: Option<EntityId>,
    /// 事件的**承受方**：受伤者、被杀者、获得经验者。
    pub target: Option<EntityId>,
    /// 事件的数量：伤害量、经验量。没有数量语义的事件为 `0`。
    pub amount: i64,
}

thread_local! {
    /// 当前调用窗口内，处理函数正在处理哪一条事件。
    static ACTIVE_EVENT: RefCell<Option<EventPayload>> = const { RefCell::new(None) };
}

/// 设置本次调用窗口的活跃事件。必须与 [`clear_active_event`] 成对。
pub fn set_active_event(payload: EventPayload) {
    ACTIVE_EVENT.with(|cell| *cell.borrow_mut() = Some(payload));
}

/// 清空活跃事件。
pub fn clear_active_event() {
    ACTIVE_EVENT.with(|cell| *cell.borrow_mut() = None);
}

/// 在设置好活跃事件的窗口内执行 `body`，结束后无条件清空。
///
/// 这是宿主应当优先使用的形式——把「设置、调用、清空」三步收进一个
/// 函数，漏掉最后一步的可能性因此不存在。
pub fn with_active_event_for<R>(payload: EventPayload, body: impl FnOnce() -> R) -> R {
    set_active_event(payload);
    let result = body();
    clear_active_event();
    result
}

/// 注册四个事件负载查询原语。
pub fn register(engine: &mut ScriptEngine) {
    engine.register_fn("event-kind", event_kind);
    engine.register_fn("event-actor", event_actor);
    engine.register_fn("event-target", event_target);
    engine.register_fn("event-amount", event_amount);
}

/// `(event-kind)`：本次事件的种类字符串；没有活跃事件时返回空串。
fn event_kind() -> SteelVal {
    with_active_event(SteelVal::StringV("".into()), |payload| {
        SteelVal::StringV(payload.kind.into())
    })
}

/// `(event-actor)`：事件发起方的句柄；没有发起方或没有活跃事件时
/// 返回 `#f`。
fn event_actor() -> SteelVal {
    with_active_event(SteelVal::BoolV(false), |payload| {
        handle_or_false(payload.actor)
    })
}

/// `(event-target)`：事件承受方的句柄；同上。
fn event_target() -> SteelVal {
    with_active_event(SteelVal::BoolV(false), |payload| {
        handle_or_false(payload.target)
    })
}

/// `(event-amount)`：事件的数量；没有活跃事件时返回 `0`。
fn event_amount() -> SteelVal {
    with_active_event(SteelVal::IntV(0), |payload| {
        SteelVal::IntV(payload.amount as isize)
    })
}

fn handle_or_false(entity: Option<EntityId>) -> SteelVal {
    match entity {
        Some(id) => ScriptEntityHandle::new(id)
            .into_steelval()
            .unwrap_or(SteelVal::BoolV(false)),
        None => SteelVal::BoolV(false),
    }
}

/// 读活跃事件；没有活跃事件时返回 `fallback`，见模块文档最后一节。
fn with_active_event<R>(fallback: R, body: impl FnOnce(&EventPayload) -> R) -> R {
    ACTIVE_EVENT.with(|cell| match cell.borrow().as_ref() {
        Some(payload) => body(payload),
        None => fallback,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_world::entity::Arena;

    fn some_entity() -> EntityId {
        let mut arena: Arena<()> = Arena::new();
        arena.spawn(())
    }

    fn payload() -> EventPayload {
        EventPayload {
            kind: "killed",
            actor: Some(some_entity()),
            target: Some(some_entity()),
            amount: 7,
        }
    }

    #[test]
    fn 没有活跃事件时四个查询都返回哨兵值而不是panic() {
        // 宿主接线可能有 bug，脚本不该因此把进程打崩。
        // Arrange
        clear_active_event();

        // Act & Assert
        assert_eq!(event_kind(), SteelVal::StringV("".into()));
        assert_eq!(event_actor(), SteelVal::BoolV(false));
        assert_eq!(event_target(), SteelVal::BoolV(false));
        assert_eq!(event_amount(), SteelVal::IntV(0));
    }

    #[test]
    fn 活跃窗口内查询读到本次事件的负载() {
        // Arrange & Act
        let (kind, amount) = with_active_event_for(payload(), || (event_kind(), event_amount()));

        // Assert
        assert_eq!(kind, SteelVal::StringV("killed".into()));
        assert_eq!(amount, SteelVal::IntV(7));
    }

    #[test]
    fn 窗口结束后活跃事件被清空不会张冠李戴() {
        // 这条守的正是模块文档「调用约定」一节说的那件事。
        // Arrange
        with_active_event_for(payload(), event_kind);

        // Act
        let after = event_kind();

        // Assert
        assert_eq!(after, SteelVal::StringV("".into()));
    }

    #[test]
    fn 没有发起方的事件里event_actor返回假而不是某个伪造句柄() {
        // 环境伤害/坠落致死没有击杀者，这不是错误状态，是常态。
        // Arrange
        let no_actor = EventPayload {
            kind: "killed",
            actor: None,
            target: Some(some_entity()),
            amount: 0,
        };

        // Act
        let (actor, target) = with_active_event_for(no_actor, || (event_actor(), event_target()));

        // Assert
        assert_eq!(actor, SteelVal::BoolV(false));
        assert_ne!(target, SteelVal::BoolV(false));
    }
}
