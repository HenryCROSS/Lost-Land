//! 把 `register-skill` 注册进脚本引擎：mod 脚本借此定义自定义技能。
//!
//! 模式同 [`crate::script_terrain_api`]/[`crate::script_class_api`]。
//! 技能比职业/地形多出两处 FFI 转换上的麻烦，本模块文档记录选择：
//!
//! - **`prerequisites` 是 `Vec<String>`**：steel-core 对 `Vec<T>` 提供
//!   了 `FromSteelVal`（`T: FromSteelVal` 时逐元素转换，见
//!   `steel-core::conversions`），脚本传一个字符串列表
//!   `(list "lostland:strike" "lostland:brace")` 即可，不需要额外的
//!   自定义类型。
//! - **`SkillEffect`/`ResourceCost` 这两个枚举没有直接的 Steel 表示**：
//!   FFI 参数类型必须是具体的 Rust 类型，不能是「联合体」。这里的处理
//!   与 `register-terrain` 的 `opens-into`（空串哨兵表示 `None`）同一
//!   思路的推广：用一个字符串「标签」参数 + 若干按标签解释的数值/字符
//!   串参数，未用到的槽位由调用方传占位值（`0`/空串）。签名见
//!   [`register_skill`] 文档，不是最简洁的 Scheme 写法，但避免了引入
//!   自定义 `steel::rvals::Custom` 类型的额外复杂度——技能声明是低频的
//!   注册期调用，不是性能敏感路径，FFI 参数个数多几个不影响任何既有
//!   约束。

use std::cell::RefCell;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_script::host::ScriptEngine;
use ll_world::entity::AttributeKind;

use crate::active_registry::with_active_registry;
use crate::registry::Registry;
use crate::skill::{ResourceCost, ResourceKind, SkillAttrs, SkillEffect, SkillError, SkillTable};

thread_local! {
    /// 当前调用窗口内，`register-skill` 应该写入的技能表。
    static ACTIVE_TABLE: RefCell<Option<SkillTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-skill` 可写入的目标。
pub fn set_active_target(table: SkillTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `SkillTable`。
pub fn take_active_target() -> SkillTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-skill` 注册进 `engine`。
pub fn register_skill_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-skill", register_skill);
}

/// `(register-skill id owning-class prerequisites cooldown-ticks
///                   resource-kind resource-amount
///                   effect-kind effect-tag effect-amount effect-amount2)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `owning-class`：所属职业的完整标识符字符串，空串 `""` 表示通用
///   技能（[`SkillDef::owning_class`](crate::skill::SkillDef) 为
///   `None`）——与 `register-terrain` 的 `opens-into` 同一个空串哨兵
///   约定。
/// - `prerequisites`：前置技能标识符字符串列表，空列表表示无前置。
/// - `cooldown-ticks`：冷却 tick 数，非负整数。
/// - `resource-kind`：`"none"`（不消耗资源）/`"mana"`/`"stamina"`。
/// - `resource-amount`：`resource-kind` 为 `"none"` 时忽略。
/// - `effect-kind`：`"deal-damage"`/`"restore-resource"`/
///   `"temporary-stat-modifier"`。
/// - `effect-tag`：按 `effect-kind` 解释——`"deal-damage"` 忽略（传
///   `""`）；`"restore-resource"` 时是恢复的资源种类
///   （`"mana"`/`"stamina"`）；`"temporary-stat-modifier"` 时是受影响
///   的主属性名（同 `register-class` 的 `primary-attribute`）。
/// - `effect-amount`：按 `effect-kind` 解释——基础伤害/基础恢复量/
///   属性增减量。
/// - `effect-amount2`：仅 `"temporary-stat-modifier"` 使用，持续 tick
///   数；其余 `effect-kind` 忽略（传 `0`）。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
#[allow(clippy::too_many_arguments)]
fn register_skill(
    id: String,
    owning_class: String,
    prerequisites: Vec<String>,
    cooldown_ticks: i64,
    resource_kind: String,
    resource_amount: i64,
    effect_kind: String,
    effect_tag: String,
    effect_amount: i64,
    effect_amount2: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-skill 在没有活跃技能表的窗口内被调用".to_string());
            };
            do_register_skill(
                registry,
                table,
                &id,
                &owning_class,
                &prerequisites,
                cooldown_ticks,
                &resource_kind,
                resource_amount,
                &effect_kind,
                &effect_tag,
                effect_amount,
                effect_amount2,
            )
        })
    })
}

/// [`register_skill`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_skill(
    registry: &mut Registry,
    table: &mut SkillTable,
    id: &str,
    owning_class: &str,
    prerequisites: &[String],
    cooldown_ticks: i64,
    resource_kind: &str,
    resource_amount: i64,
    effect_kind: &str,
    effect_tag: &str,
    effect_amount: i64,
    effect_amount2: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let owning_class = if owning_class.is_empty() {
        None
    } else {
        let owning_id = NamespacedId::parse(owning_class)
            .map_err(|err| format!("非法 owning-class 标识符 {owning_class:?}：{err}"))?;
        Some(registry.intern(owning_id))
    };

    let mut prerequisite_indices: Vec<ContentIndex> = Vec::with_capacity(prerequisites.len());
    for raw in prerequisites {
        let parsed =
            NamespacedId::parse(raw).map_err(|err| format!("非法前置技能标识符 {raw:?}：{err}"))?;
        prerequisite_indices.push(registry.intern(parsed));
    }

    let resource_cost = parse_resource_cost(resource_kind, resource_amount)?;
    let effect = parse_effect(effect_kind, effect_tag, effect_amount, effect_amount2)?;

    table
        .define(
            index,
            SkillAttrs {
                owning_class,
                prerequisites: prerequisite_indices,
                cooldown_ticks: cooldown_ticks.max(0) as u32,
                resource_cost,
                effect,
            },
        )
        .map(|()| true)
        .map_err(|err: SkillError| err.to_string())
}

/// `resource-kind`/`resource-amount` → [`ResourceCost`]。
fn parse_resource_cost(kind: &str, amount: i64) -> Result<ResourceCost, String> {
    match kind {
        "none" => Ok(ResourceCost::None),
        "mana" => Ok(ResourceCost::Amount(
            ResourceKind::Mana,
            amount.max(0) as u32,
        )),
        "stamina" => Ok(ResourceCost::Amount(
            ResourceKind::Stamina,
            amount.max(0) as u32,
        )),
        _ => Err(format!("未知的资源种类 {kind:?}")),
    }
}

/// `effect-kind`/`effect-tag`/`effect-amount`/`effect-amount2` →
/// [`SkillEffect`]。
fn parse_effect(kind: &str, tag: &str, amount: i64, amount2: i64) -> Result<SkillEffect, String> {
    match kind {
        "deal-damage" => Ok(SkillEffect::DealDamage {
            base: amount as i32,
        }),
        "restore-resource" => {
            let resource = match tag {
                "mana" => ResourceKind::Mana,
                "stamina" => ResourceKind::Stamina,
                _ => return Err(format!("未知的资源种类 {tag:?}")),
            };
            Ok(SkillEffect::RestoreResource {
                resource,
                base: amount as i32,
            })
        }
        "temporary-stat-modifier" => {
            let attribute =
                attribute_kind_from_str(tag).ok_or_else(|| format!("未知的主属性名 {tag:?}"))?;
            Ok(SkillEffect::TemporaryStatModifier {
                attribute,
                amount: amount as i32,
                duration_ticks: amount2.max(0) as u32,
            })
        }
        _ => Err(format!("未知的技能效果种类 {kind:?}")),
    }
}

/// 属性名字符串 → [`AttributeKind`]，与
/// `crate::script_class_api::attribute_kind_from_str` 是同一份映射的
/// 独立拷贝——两个模块目前都足够小，为六个固定分支的 `match` 单独抽出
/// 一个共享帮手模块并不划算，重复这一份比引入一层间接更直接。
fn attribute_kind_from_str(name: &str) -> Option<AttributeKind> {
    Some(match name {
        "strength" => AttributeKind::Strength,
        "dexterity" => AttributeKind::Dexterity,
        "constitution" => AttributeKind::Constitution,
        "intelligence" => AttributeKind::Intelligence,
        "willpower" => AttributeKind::Willpower,
        "charisma" => AttributeKind::Charisma,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法技能声明注册成功并写入技能表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SkillTable::new();

        // Act
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:frostbolt",
            "",
            &[],
            25,
            "mana",
            12,
            "deal-damage",
            "",
            15,
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:frostbolt").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的技能应能查到属性");
        assert_eq!(view.effect, SkillEffect::DealDamage { base: 15 });
        assert_eq!(
            view.resource_cost,
            ResourceCost::Amount(ResourceKind::Mana, 12)
        );
    }

    #[test]
    fn 前置技能字符串被解析成前置索引() {
        // Arrange
        let mut registry = Registry::new();
        let strike = registry.intern(NamespacedId::parse("lostland:strike").unwrap());
        let mut table = SkillTable::new();

        // Act
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:combo",
            "",
            &["lostland:strike".to_string()],
            10,
            "none",
            0,
            "deal-damage",
            "",
            5,
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:combo").unwrap())
            .unwrap();
        assert_eq!(table.get(index).unwrap().prerequisites, &[strike]);
    }

    #[test]
    fn 临时属性修正效果解析出正确的属性与持续时间() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SkillTable::new();

        // Act
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:brace",
            "",
            &[],
            15,
            "stamina",
            5,
            "temporary-stat-modifier",
            "constitution",
            3,
            10,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:brace").unwrap())
            .unwrap();
        assert_eq!(
            table.get(index).unwrap().effect,
            SkillEffect::TemporaryStatModifier {
                attribute: AttributeKind::Constitution,
                amount: 3,
                duration_ticks: 10,
            }
        );
    }

    #[test]
    fn 未知的技能效果种类返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SkillTable::new();

        // Act
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:x",
            "",
            &[],
            0,
            "none",
            0,
            "explode",
            "",
            0,
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_skill() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_skill_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(SkillTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-skill "yourmod:frostbolt" "" (list) 25 "mana" 12 "deal-damage" "" 15 0)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:frostbolt").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(
            table.get(index).unwrap().effect,
            SkillEffect::DealDamage { base: 15 }
        );
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：未知的资源种类——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_skill_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(SkillTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-skill "yourmod:x" "" (list) 0 "gold" 0 "deal-damage" "" 0 0)"#.to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_terrain_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
