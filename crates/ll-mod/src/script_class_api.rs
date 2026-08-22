//! 把 `register-class` 注册进脚本引擎：mod 脚本借此定义自定义职业。
//!
//! # 补的是哪个缺口
//!
//! [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
//! 裁定「玩法层」的检验是「mod 脚本能不能注册」——`crate::class` 的
//! `ClassTable`/`materialize_base_classes` 早就是本体与 mod 共用的同一
//! 张表、同一条 `define` 通道（见其模块文档），但在本模块之前，唯一
//! 能触达这条通道的是**纯 Rust 函数调用**——脚本没有任何注册函数可以
//! 调用来登记一个新职业，这不是注册表本身的缺陷，是脚本绑定这一半
//! 从未补上。本模块正是这一半。
//!
//! # 照抄 `script_terrain_api.rs` 的模式
//!
//! `thread_local!` + `RefCell<Option<T>>` 把值整个移进/移出、
//! `Registry` 走 [`crate::active_registry`] 共享目标——完整论证见
//! `crate::script_terrain_api`/`crate::active_registry` 模块文档，本
//! 模块不重复。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use ll_world::entity::AttributeKind;

use crate::active_registry::with_active_registry;
use crate::class::{ClassAttrs, ClassError, ClassTable};
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-class` 应该写入的职业表。
    static ACTIVE_TABLE: RefCell<Option<ClassTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-class` 可写入的目标。
pub fn set_active_target(table: ClassTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `ClassTable`，调用约定同
/// `script_terrain_api::take_active_target`。
pub fn take_active_target() -> ClassTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-class` 注册进 `engine`。
pub fn register_class_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-class", register_class);
}

/// `(register-class id display-name-key primary-attribute)`。
///
/// - `id`：完整命名空间标识符字符串，如 `"yourmod:necromancer"`。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
/// - `primary-attribute`：主属性倾向，六选一的字符串/符号——
///   `"strength"`/`"dexterity"`/`"constitution"`/`"intelligence"`/
///   `"willpower"`/`"charisma"`（Steel 的字符串与符号都能转换成 Rust
///   `String`，见 steel-core `FromSteelVal for String`，因此脚本写
///   `'strength` 或 `"strength"` 均可）。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_class(
    id: String,
    display_name_key: String,
    primary_attribute: String,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-class 在没有活跃职业表的窗口内被调用".to_string());
            };
            do_register_class(registry, table, &id, &display_name_key, &primary_attribute)
        })
    })
}

/// [`register_class`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_class(
    registry: &mut Registry,
    table: &mut ClassTable,
    id: &str,
    display_name_key: &str,
    primary_attribute: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;
    let primary_attribute = attribute_kind_from_str(primary_attribute)
        .ok_or_else(|| format!("未知的主属性名 {primary_attribute:?}"))?;

    table
        .define(
            index,
            ClassAttrs {
                display_name_key,
                primary_attribute,
            },
        )
        .map(|()| true)
        .map_err(|err: ClassError| err.to_string())
}

/// 属性名字符串 → [`AttributeKind`]。命名沿用属性系统既有的英文小写
/// 惯例，与 `ll_script::api::intent::direction_from_symbol` 同一套
/// 「字符串对字符串直接匹配，不识别就返回 `None`」的写法。`"luck"` 是
/// 幸运并入 `AttributeKind` 批次新增，与
/// `crate::script_skill_api::attribute_kind_from_str`/
/// `crate::script_item_api::stat_target_from_str` 同步收录，保持「三份
/// 独立拷贝、同一份映射」这条既有先例不出现遗漏的一份。
fn attribute_kind_from_str(name: &str) -> Option<AttributeKind> {
    Some(match name {
        "strength" => AttributeKind::Strength,
        "dexterity" => AttributeKind::Dexterity,
        "constitution" => AttributeKind::Constitution,
        "intelligence" => AttributeKind::Intelligence,
        "willpower" => AttributeKind::Willpower,
        "charisma" => AttributeKind::Charisma,
        "luck" => AttributeKind::Luck,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法职业声明注册成功并写入职业表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ClassTable::new();

        // Act
        let result = do_register_class(
            &mut registry,
            &mut table,
            "yourmod:necromancer",
            "yourmod:necromancer_display_name",
            "willpower",
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:necromancer").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的职业应能查到属性");
        assert_eq!(view.primary_attribute, AttributeKind::Willpower);
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ClassTable::new();

        // Act
        let result = do_register_class(
            &mut registry,
            &mut table,
            "Not Valid",
            "yourmod:x",
            "strength",
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 未知的主属性名返回错误() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ClassTable::new();

        // Act："wisdom" 不是本项目任何一个属性名（智力叫
        // "intelligence"，本项目没有 D&D 式的 wisdom/intelligence 双属性
        // 拆分）——`"luck"` 幸运并入 `AttributeKind` 批次后已经是合法
        // 属性名，不能再用作"未知名称"的示例。
        let result = do_register_class(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:x_display_name",
            "wisdom",
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_class() {
        // 端到端验证：脚本里写 (register-class ...)，不需要脚本作者
        // 知道 Rust 侧的 Registry/ClassTable 是怎么接线的。
        // Arrange
        let mut engine = ScriptEngine::new();
        register_class_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ClassTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-class "yourmod:necromancer" "yourmod:necromancer_display_name" "willpower")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:necromancer").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(
            table.get(index).expect("已注册").primary_attribute,
            AttributeKind::Willpower
        );
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：未知的主属性名——脚本作者笔误，宿主必须优雅报错。
        // "wisdom" 不是本项目任何一个属性名，理由同
        // `未知的主属性名返回错误`。
        let mut engine = ScriptEngine::new();
        register_class_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ClassTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-class "yourmod:x" "yourmod:x_display_name" "wisdom")"#.to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_terrain_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
