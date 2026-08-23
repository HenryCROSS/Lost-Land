//! 把 `register-class` 注册进脚本引擎：mod 脚本借此定义自定义职业。
//!
//! # 补的是哪个缺口
//!
//! [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
//! 裁定「玩法层」的检验是「mod 脚本能不能注册」——`crate::class` 的
//! `crate::class` 的 `ClassTable` 早就是本体与 mod 共用的同一张表、
//! 同一条 `define` 通道（见其模块文档），但在本模块之前，唯一能触达
//! 这条通道的是**纯 Rust 函数调用**——脚本没有任何注册函数可以调用来
//! 登记一个新职业，这不是注册表本身的缺陷，是脚本绑定这一半从未补上。
//! 本模块正是这一半。本体自己的四条职业现在也走这条通道
//! （`mods/lostland/classes.scm`），Rust 侧一条职业注册路径都不剩。
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
use ll_sim::traits::TraitGrant;
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

/// 把 `register-class`/`register-class-trait` 注册进 `engine`。
pub fn register_class_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-class", register_class);
    engine.register_fn("register-class-trait", register_class_trait);
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
                // 天赋走注册后追加的 `register-class-trait`，不挤进
                // `register-class` 的既有签名，见 `ClassDef::traits` 文档。
                traits: Vec::new(),
            },
        )
        .map(|()| true)
        .map_err(|err: ClassError| err.to_string())
}

/// `(register-class-trait class-id trait-id unlock-level)`——追加声明
/// 「这个职业在某个等级授予某个天赋」（职业天赋接线批次，
/// `knowledge/design/trait-system.md` 三节①、六节）。与
/// `register-race-trait` 逐字对应，只是所有者从种族换成职业——两者
/// 走的是同一个 [`ll_sim::traits::TraitGrant`] 载荷、同一段
/// `ll_sim::traits::effective_traits` 聚合算法，见
/// [`crate::class::ClassTable`] 的 `TraitGrantSource` impl 文档
/// 「与 `RaceTable` 的同名 impl 复用同一个 trait」一节。
///
/// - `class-id`：已经通过 `register-class` 注册过的完整命名空间标识符
///   字符串——目标必须已存在（ADR 0017「注册期完整校验」），否则报错。
/// - `trait-id`：天赋的完整命名空间标识符字符串——**不要求**已经通过
///   `register-trait` 注册过（只 `intern`，不跨表校验存在性，见
///   [`crate::class::ClassTable::add_trait_grant`] 文档「不校验」一节，
///   与 `register-race-trait` 是同一条已知简化）。
/// - `unlock-level`：解锁所需等级，非负整数。**这是职业天赋与种族天赋
///   在内容层面唯一的实质差异**：种族恒传 `1`（"拥有即生效"），职业
///   可以按等级曲线传更大的值（`trait-system.md` 六节原文「职业天赋按
///   实际设计填对应等级」）。引擎侧不为这条差异分流——校验只保证非负，
///   不替内容作者做设计决定。
///
/// 返回 `Result<bool, String>`，理由同 `register_class` 文档。
fn register_class_trait(
    class_id: String,
    trait_id: String,
    unlock_level: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-class-trait 在没有活跃职业表的窗口内被调用".to_string());
            };
            do_register_class_trait(registry, table, &class_id, &trait_id, unlock_level)
        })
    })
}

/// [`register_class_trait`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_class_trait(
    registry: &mut Registry,
    table: &mut ClassTable,
    class_id: &str,
    trait_id: &str,
    unlock_level: i64,
) -> Result<bool, String> {
    let parsed_class_id = NamespacedId::parse(class_id)
        .map_err(|err| format!("非法内容标识符 {class_id:?}：{err}"))?;
    let Some(class_index) = registry.get(&parsed_class_id) else {
        return Err(format!("职业 {class_id:?} 尚未通过 register-class 注册"));
    };
    let parsed_trait_id = NamespacedId::parse(trait_id)
        .map_err(|err| format!("非法内容标识符 {trait_id:?}：{err}"))?;
    if unlock_level < 0 {
        return Err(format!("解锁等级不允许为负数：{unlock_level}"));
    }
    let trait_index = registry.intern(parsed_trait_id);
    table
        .add_trait_grant(
            class_index,
            TraitGrant {
                trait_id: trait_index,
                unlock_level: unlock_level as i32,
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
    #[test]
    fn 合法职业天赋声明写入职业表且解锁等级原样保留() {
        // Arrange：先注册职业，再追加天赋——`register-class-trait` 要求
        // 目标已存在（ADR 0017）。
        let mut registry = Registry::new();
        let mut table = ClassTable::new();
        do_register_class(
            &mut registry,
            &mut table,
            "yourmod:rogue",
            "yourmod:rogue_display_name",
            "dexterity",
        )
        .expect("职业注册应当成功");

        // Act：解锁等级刻意不是 1——职业天赋与种族天赋唯一的实质差异
        // 就在这个字段能填大于 1 的值。
        let result = do_register_class_trait(
            &mut registry,
            &mut table,
            "yourmod:rogue",
            "yourmod:sneaky",
            3,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let class_index = registry
            .get(&NamespacedId::parse("yourmod:rogue").unwrap())
            .expect("刚注册的职业应能查到索引");
        let trait_index = registry
            .get(&NamespacedId::parse("yourmod:sneaky").unwrap())
            .expect("register-class-trait 应当 intern 出天赋索引");
        assert_eq!(
            table.get(class_index).expect("已注册").traits,
            &[TraitGrant {
                trait_id: trait_index,
                unlock_level: 3,
            }]
        );
    }

    #[test]
    fn 给尚未注册的职业追加天赋返回错误() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ClassTable::new();

        // Act
        let result = do_register_class_trait(
            &mut registry,
            &mut table,
            "yourmod:never_registered",
            "yourmod:sneaky",
            1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 负数解锁等级返回错误而不是被静默截断() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ClassTable::new();
        do_register_class(
            &mut registry,
            &mut table,
            "yourmod:rogue",
            "yourmod:rogue_display_name",
            "dexterity",
        )
        .expect("职业注册应当成功");

        // Act
        let result = do_register_class_trait(
            &mut registry,
            &mut table,
            "yourmod:rogue",
            "yourmod:sneaky",
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_class_trait() {
        // 端到端验证：脚本里连着写 (register-class ...) 与
        // (register-class-trait ...)，不需要脚本作者知道 Rust 侧的
        // Registry/ClassTable 是怎么接线的。
        // Arrange
        let mut engine = ScriptEngine::new();
        register_class_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ClassTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-class "yourmod:rogue" "yourmod:rogue_display_name" "dexterity")
               (register-class-trait "yourmod:rogue" "yourmod:sneaky" 3)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let class_index = registry
            .get(&NamespacedId::parse("yourmod:rogue").unwrap())
            .expect("刚注册的职业应能查到索引");
        let trait_index = registry
            .get(&NamespacedId::parse("yourmod:sneaky").unwrap())
            .expect("register-class-trait 应当 intern 出天赋索引");
        assert_eq!(
            table.get(class_index).expect("已注册").traits,
            &[TraitGrant {
                trait_id: trait_index,
                unlock_level: 3,
            }]
        );
    }
}
