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
/// - `resource-kind`：`"none"`（不消耗资源）/`"mana"`/`"stamina"`（既有
///   内置资源，`ResourceCost::Amount`）/`"blood"`（血代价,直接扣
///   `health`,绕开减伤,`ResourceCost::Blood`,资源池落地批次新增,见
///   `ll_sim::skill::ResourceCost::Blood` 文档）/`"slot-tier:<pool-id>"`
///   （法术位落地批次新增：冒号后接完整命名空间标识符,消耗该
///   `ResourcePoolShape::TieredSlots` 池、`resource-amount` 档或更高档
///   的一个槽位,`ResourceCost::SlotTier`——前缀约定理由见
///   [`parse_resource_cost`] 文档）/其它任意字符串——按完整命名空间
///   标识符解析,引用一个已经通过 `register-resource-pool` 注册的开放
///   标量资源池（`ResourceCost::PoolAmount`,资源池落地批次新增，要求
///   目标池已注册,理由同 `register-trait-resource-pool` 对 `pool-id`
///   的校验）。
/// - `resource-amount`：`resource-kind` 为 `"none"` 时忽略；
///   `"slot-tier:<pool-id>"` 时是 `min_tier`（最低可用档位,1 起编号）。
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

    let resource_cost = parse_resource_cost(registry, resource_kind, resource_amount)?;
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
///
/// `"none"`/`"mana"`/`"stamina"`/`"blood"` 四个保留字之外的任意字符串
/// 按完整命名空间标识符解析，引用一个已注册的标量资源池（资源池落地
/// 批次新增，见本函数文档所属的 [`register_skill`] 文档「resource-kind」
/// 一节）——与 `register-trait-resource-pool` 对 `pool-id` 的校验同一条
/// 纪律：目标池**要求**已经通过 `register-resource-pool` 注册。
///
/// # 为什么法术位走 `"slot-tier:<pool-id>"` 前缀，不是新增一个位置参数
///
/// `resource-kind`/`resource-amount` 这一对参数已经用「字符串标签 +
/// 解释规则」表达了四种资源通道（`none`/内置/血代价/开放标量池），第五
/// 种（法术位）需要额外携带一个「哪个池」的标识符——若给
/// `register_skill` 再加一个位置参数，既有内容（迁进 `mods/example_mod/skills.json5` 之前
/// 的 `frostbolt`/`sorcerer_firebolt`/`blood_bolt`）与既有测试全部要
/// 补一个从不使用的哨兵参数。`"slot-tier:"` 前缀把「这是哪一类资源
/// 通道」与「具体是哪个池」编码进同一个字符串参数，`resource-amount`
/// 复用为 `min_tier`（本就是该参数在其余四种通道各自的解释规则一样，
/// 按 `kind` 决定含义)——不引入新参数，也不需要改动任何既有调用点。
fn parse_resource_cost(
    registry: &Registry,
    kind: &str,
    amount: i64,
) -> Result<ResourceCost, String> {
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
        "blood" => Ok(ResourceCost::Blood(amount.max(0) as u32)),
        _ if kind.starts_with("slot-tier:") => {
            let pool_id = &kind["slot-tier:".len()..];
            let parsed_pool = NamespacedId::parse(pool_id)
                .map_err(|err| format!("未知的法术位资源池 {pool_id:?}：{err}"))?;
            let pool = registry.get(&parsed_pool).ok_or_else(|| {
                format!("资源池 {pool_id:?} 尚未通过 register-resource-pool 注册")
            })?;
            let min_tier = amount.clamp(0, i64::from(u8::MAX)) as u8;
            Ok(ResourceCost::SlotTier(pool, min_tier))
        }
        _ => {
            let parsed_pool = NamespacedId::parse(kind)
                .map_err(|err| format!("未知的资源种类 {kind:?}：{err}"))?;
            let pool = registry
                .get(&parsed_pool)
                .ok_or_else(|| format!("资源池 {kind:?} 尚未通过 register-resource-pool 注册"))?;
            Ok(ResourceCost::PoolAmount(pool, amount.max(0) as u32))
        }
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
/// 独立拷贝——两个模块目前都足够小，为七个固定分支的 `match` 单独抽出
/// 一个共享帮手模块并不划算，重复这一份比引入一层间接更直接。`"luck"`
/// 是幸运并入 `AttributeKind` 批次新增——`(register-skill ..
/// "temporary-stat-modifier" "luck" ..)` 正是祝福术/诅咒这类技能能
/// 临时改变幸运的 authoring 入口。
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

    #[test]
    fn resource_kind为blood时解析成血代价() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SkillTable::new();

        // Act
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:blood_bolt",
            "",
            &[],
            10,
            "blood",
            15,
            "deal-damage",
            "",
            30,
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:blood_bolt").unwrap())
            .unwrap();
        assert_eq!(
            table.get(index).unwrap().resource_cost,
            ResourceCost::Blood(15)
        );
    }

    #[test]
    fn resource_kind引用已注册资源池时解析成池消耗() {
        // Arrange
        let mut registry = Registry::new();
        let pool = registry.intern(NamespacedId::parse("yourmod:sorcery_points").unwrap());
        let mut table = SkillTable::new();

        // Act
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:sorcerer_firebolt",
            "",
            &[],
            10,
            "yourmod:sorcery_points",
            5,
            "deal-damage",
            "",
            12,
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:sorcerer_firebolt").unwrap())
            .unwrap();
        assert_eq!(
            table.get(index).unwrap().resource_cost,
            ResourceCost::PoolAmount(pool, 5)
        );
    }

    #[test]
    fn resource_kind引用未注册资源池时返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SkillTable::new();

        // Act：从未 register-resource-pool 过 "yourmod:never_registered"。
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:x",
            "",
            &[],
            0,
            "yourmod:never_registered",
            0,
            "deal-damage",
            "",
            0,
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn resource_kind为slot_tier前缀时解析成法术位消耗() {
        // Arrange
        let mut registry = Registry::new();
        let pool = registry.intern(NamespacedId::parse("yourmod:wizard_slots").unwrap());
        let mut table = SkillTable::new();

        // Act
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:fireball",
            "",
            &[],
            10,
            "slot-tier:yourmod:wizard_slots",
            3,
            "deal-damage",
            "",
            28,
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:fireball").unwrap())
            .unwrap();
        assert_eq!(
            table.get(index).unwrap().resource_cost,
            ResourceCost::SlotTier(pool, 3)
        );
    }

    #[test]
    fn resource_kind为slot_tier前缀但目标资源池未注册时返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SkillTable::new();

        // Act：从未 register-resource-pool 过 "yourmod:never_registered"。
        let result = do_register_skill(
            &mut registry,
            &mut table,
            "yourmod:x",
            "",
            &[],
            0,
            "slot-tier:yourmod:never_registered",
            1,
            "deal-damage",
            "",
            0,
            0,
        );

        // Assert
        assert!(result.is_err());
    }
}
