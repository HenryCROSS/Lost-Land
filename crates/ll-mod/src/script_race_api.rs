//! 把 `register-race` 注册进脚本引擎：mod 脚本借此定义自定义种族。
//!
//! 模式同 [`crate::script_class_api`]。种族比职业多出四个数值字段
//! （六项属性修正 + 暗视下限 + 体型两维 + 寿命），FFI 签名因此更长，
//! 但每个参数都是简单的整数，不需要像 [`crate::script_skill_api`] 那样
//! 处理带标签的枚举。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use ll_world::entity::BaseStats;

use crate::active_registry::with_active_registry;
use crate::race::{RaceAttrs, RaceError, RaceTable};
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-race` 应该写入的种族表。
    static ACTIVE_TABLE: RefCell<Option<RaceTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-race` 可写入的目标。
pub fn set_active_target(table: RaceTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `RaceTable`。
pub fn take_active_target() -> RaceTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-race` 注册进 `engine`。
pub fn register_race_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-race", register_race);
}

/// `(register-race id display-name-key
///                  strength-mod dexterity-mod constitution-mod
///                  intelligence-mod willpower-mod charisma-mod
///                  darkvision-floor footprint-width footprint-height
///                  lifespan-years)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
/// - 六个 `*-mod` 参数：六项主属性的固定增减量（可为负），见
///   [`crate::race`] 模块文档「属性修正」一节——**不是**千分比。
/// - `darkvision-floor`：暗视下限。
/// - `footprint-width`/`footprint-height`：占位格数，非负整数，钳位到
///   `u8` 范围。
/// - `lifespan-years`：寿命（年），非负整数。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
#[allow(clippy::too_many_arguments)]
fn register_race(
    id: String,
    display_name_key: String,
    strength_mod: i64,
    dexterity_mod: i64,
    constitution_mod: i64,
    intelligence_mod: i64,
    willpower_mod: i64,
    charisma_mod: i64,
    darkvision_floor: i64,
    footprint_width: i64,
    footprint_height: i64,
    lifespan_years: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-race 在没有活跃种族表的窗口内被调用".to_string());
            };
            do_register_race(
                registry,
                table,
                &id,
                &display_name_key,
                BaseStats {
                    strength: strength_mod as i32,
                    dexterity: dexterity_mod as i32,
                    constitution: constitution_mod as i32,
                    intelligence: intelligence_mod as i32,
                    willpower: willpower_mod as i32,
                    charisma: charisma_mod as i32,
                },
                darkvision_floor as i32,
                (
                    footprint_width.max(0).min(i64::from(u8::MAX)) as u8,
                    footprint_height.max(0).min(i64::from(u8::MAX)) as u8,
                ),
                lifespan_years.max(0) as u32,
            )
        })
    })
}

/// [`register_race`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_race(
    registry: &mut Registry,
    table: &mut RaceTable,
    id: &str,
    display_name_key: &str,
    stat_modifiers: BaseStats,
    darkvision_floor: i32,
    footprint: (u8, u8),
    lifespan_years: u32,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;

    table
        .define(
            index,
            RaceAttrs {
                display_name_key,
                stat_modifiers,
                darkvision_floor,
                footprint,
                lifespan_years,
            },
        )
        .map(|()| true)
        .map_err(|err: RaceError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法种族声明注册成功并写入种族表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();

        // Act
        let result = do_register_race(
            &mut registry,
            &mut table,
            "yourmod:half_elf",
            "yourmod:half_elf_display_name",
            BaseStats {
                strength: 0,
                dexterity: 1,
                constitution: 0,
                intelligence: 0,
                willpower: 0,
                charisma: 1,
            },
            0,
            (1, 1),
            150,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:half_elf").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的种族应能查到属性");
        assert_eq!(view.stat_modifiers.dexterity, 1);
        assert_eq!(view.lifespan_years, 150);
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();

        // Act
        let result = do_register_race(
            &mut registry,
            &mut table,
            "Not Valid",
            "yourmod:x",
            BaseStats {
                strength: 0,
                dexterity: 0,
                constitution: 0,
                intelligence: 0,
                willpower: 0,
                charisma: 0,
            },
            0,
            (1, 1),
            80,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_race() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_race_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RaceTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-race "yourmod:half_elf" "yourmod:half_elf_display_name" 0 1 0 0 0 1 0 1 1 150)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:half_elf").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).unwrap().lifespan_years, 150);
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：非法命名空间——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_race_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RaceTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-race "Not Valid" "yourmod:x" 0 0 0 0 0 0 0 1 1 80)"#.to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_terrain_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
