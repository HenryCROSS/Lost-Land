//! 把 `register-quest` 注册进脚本引擎：mod 脚本借此定义自定义任务节点。
//!
//! 模式同 [`crate::script_skill_api`]——[`crate::quest::QuestCondition`]
//! 同样是一个没有直接 Steel 表示的枚举，用「标签 + 按标签解释的参数」
//! 编码，见 [`register_quest`] 文档。`prerequisites` 同样是
//! `Vec<String>`，理由同 `crate::script_skill_api` 模块文档。

use std::cell::RefCell;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::quest::{QuestAttrs, QuestCondition, QuestError, QuestTable};
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-quest` 应该写入的任务表。
    static ACTIVE_TABLE: RefCell<Option<QuestTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-quest` 可写入的目标。
pub fn set_active_target(table: QuestTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `QuestTable`。
pub fn take_active_target() -> QuestTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-quest` 注册进 `engine`。
pub fn register_quest_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-quest", register_quest);
}

/// `(register-quest id prerequisites condition-kind condition-arg
///                   condition-count)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `prerequisites`：前置任务节点标识符字符串列表。
/// - `condition-kind`：`"kill-count"`/`"script"`（[`QuestCondition`]
///   只有这两档，见其文档「完成条件分档」一节——本批次不做二档）。
/// - `condition-arg`：`"kill-count"` 时是目标敌人类型的标识符字符串；
///   `"script"` 时是脚本回调标识符字符串。
/// - `condition-count`：`"kill-count"` 时是需要击杀的数量；`"script"`
///   时忽略（传 `0`）。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_quest(
    id: String,
    prerequisites: Vec<String>,
    condition_kind: String,
    condition_arg: String,
    condition_count: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-quest 在没有活跃任务表的窗口内被调用".to_string());
            };
            do_register_quest(
                registry,
                table,
                &id,
                &prerequisites,
                &condition_kind,
                &condition_arg,
                condition_count,
            )
        })
    })
}

/// [`register_quest`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_quest(
    registry: &mut Registry,
    table: &mut QuestTable,
    id: &str,
    prerequisites: &[String],
    condition_kind: &str,
    condition_arg: &str,
    condition_count: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let mut prerequisite_indices: Vec<ContentIndex> = Vec::with_capacity(prerequisites.len());
    for raw in prerequisites {
        let parsed =
            NamespacedId::parse(raw).map_err(|err| format!("非法前置任务标识符 {raw:?}：{err}"))?;
        prerequisite_indices.push(registry.intern(parsed));
    }

    let condition = match condition_kind {
        "kill-count" => {
            let target_id = NamespacedId::parse(condition_arg)
                .map_err(|err| format!("非法目标类型标识符 {condition_arg:?}：{err}"))?;
            QuestCondition::KillCount {
                target_kind: registry.intern(target_id),
                count: condition_count.max(0) as u32,
            }
        }
        "script" => {
            let callback_id = NamespacedId::parse(condition_arg)
                .map_err(|err| format!("非法脚本回调标识符 {condition_arg:?}：{err}"))?;
            QuestCondition::Script(callback_id)
        }
        _ => return Err(format!("未知的任务完成条件种类 {condition_kind:?}")),
    };

    table
        .define(
            index,
            QuestAttrs {
                prerequisites: prerequisite_indices,
                condition,
            },
        )
        .map(|()| true)
        .map_err(|err: QuestError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法击杀计数任务声明注册成功() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = QuestTable::new();

        // Act
        let result = do_register_quest(
            &mut registry,
            &mut table,
            "yourmod:kill_goblins",
            &[],
            "kill-count",
            "lostland:goblin",
            3,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:kill_goblins").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的任务应能查到属性");
        match view.condition {
            QuestCondition::KillCount { count, .. } => assert_eq!(*count, 3),
            other => panic!("期望 KillCount，实际 {other:?}"),
        }
    }

    #[test]
    fn 脚本回调型任务声明注册成功() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = QuestTable::new();

        // Act
        let result = do_register_quest(
            &mut registry,
            &mut table,
            "yourmod:epilogue",
            &[],
            "script",
            "yourmod:epilogue_condition",
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:epilogue").unwrap())
            .unwrap();
        assert_eq!(
            table.get(index).unwrap().condition,
            &QuestCondition::Script(NamespacedId::parse("yourmod:epilogue_condition").unwrap())
        );
    }

    #[test]
    fn 前置任务字符串被解析成前置索引() {
        // Arrange
        let mut registry = Registry::new();
        let main_quest = registry.intern(NamespacedId::parse("lostland:main_quest_1").unwrap());
        let mut table = QuestTable::new();

        // Act
        let result = do_register_quest(
            &mut registry,
            &mut table,
            "yourmod:side_quest",
            &["lostland:main_quest_1".to_string()],
            "kill-count",
            "lostland:goblin",
            1,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:side_quest").unwrap())
            .unwrap();
        assert_eq!(table.get(index).unwrap().prerequisites, &[main_quest]);
    }

    #[test]
    fn 未知的完成条件种类返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = QuestTable::new();

        // Act
        let result = do_register_quest(&mut registry, &mut table, "yourmod:x", &[], "timer", "", 0);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_quest() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_quest_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(QuestTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-quest "yourmod:kill_goblins" (list) "kill-count" "lostland:goblin" 3)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:kill_goblins").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.get(index).is_some());
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：未知的完成条件种类——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_quest_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(QuestTable::new());

        // Act
        let result =
            engine.load_source(r#"(register-quest "yourmod:x" (list) "timer" "" 0)"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_terrain_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
