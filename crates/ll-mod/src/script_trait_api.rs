//! 把 `register-trait` 注册进脚本引擎：mod 脚本借此定义自定义天赋。
//!
//! 模式同 [`crate::script_skill_api`]（`granted-skills` 是
//! `Vec<String>`，`steel-core` 的 `Vec<T>: FromSteelVal` 逐元素转换,
//! 与该模块的 `prerequisites` 同一种手法）。
//!
//! # 为什么脚本签名只有三个参数，不是设计文档的六参数
//!
//! 见 [`crate::trait_def`] 模块文档「`register-trait` 脚本签名为什么
//! 只暴露①，不是设计文档的完整六参数」一节——`stat_modifiers`/
//! `rule_modifiers`/`granted_resource_pools` 三类效果在 Rust 结构体
//! 里已经声明好形状,但脚本层尚未为"列表套元组"/"打标签的构造子"这两
//! 种更复杂的 FFI 编码约定过怎么做,本批次不为没有 resolve 侧消费者
//! 的字段发明新约定（YAGNI）。想给种族追加天赋引用,走
//! `register-race-trait`（[`crate::script_race_api`]）；想给天赋追加
//! ②③④三类效果,留给各自真正接线的批次用同一条"新增能力用新函数"的
//! 先例补上。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::registry::Registry;
use crate::trait_def::{
    CapacityFormula, ResourcePoolGrant, RuleModifier, TraitAttrs, TraitError, TraitTable,
};

thread_local! {
    /// 当前调用窗口内，`register-trait` 应该写入的天赋表。
    static ACTIVE_TABLE: RefCell<Option<TraitTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-trait` 可写入的目标。
pub fn set_active_target(table: TraitTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `TraitTable`。
pub fn take_active_target() -> TraitTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-trait`/`register-trait-resource-pool`/
/// `register-trait-resource-pool-by-level` 注册进 `engine`。
pub fn register_trait_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-trait", register_trait);
    engine.register_fn("register-trait-resource-pool", register_trait_resource_pool);
    engine.register_fn(
        "register-trait-resource-pool-by-level",
        register_trait_resource_pool_by_level,
    );
    engine.register_fn("register-trait-resistance", register_trait_resistance);
}

/// `(register-trait id display-name-key granted-skills)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
/// - `granted-skills`：这个天赋授予的技能标识符字符串列表，空列表
///   表示这个天赋不授予任何技能——**不要求**每一项都已经通过
///   `register-skill` 注册过（只 `intern`，不跨表校验存在性，理由同
///   `crate::race::RaceTable::add_trait_grant` 文档「不校验」一节）。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
/// `stat_modifiers`/`rule_modifiers`/`granted_resource_pools` 三个
/// 字段恒填空列表——见模块文档「为什么脚本签名只有三个参数」一节。
fn register_trait(
    id: String,
    display_name_key: String,
    granted_skills: Vec<String>,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-trait 在没有活跃天赋表的窗口内被调用".to_string());
            };
            do_register_trait(registry, table, &id, &display_name_key, &granted_skills)
        })
    })
}

/// [`register_trait`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_trait(
    registry: &mut Registry,
    table: &mut TraitTable,
    id: &str,
    display_name_key: &str,
    granted_skills: &[String],
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;

    let mut granted_skill_indices = Vec::with_capacity(granted_skills.len());
    for raw in granted_skills {
        let parsed =
            NamespacedId::parse(raw).map_err(|err| format!("非法技能标识符 {raw:?}：{err}"))?;
        granted_skill_indices.push(registry.intern(parsed));
    }

    table
        .define(
            index,
            TraitAttrs {
                display_name_key,
                granted_skills: granted_skill_indices,
                stat_modifiers: Vec::new(),
                rule_modifiers: Vec::new(),
                granted_resource_pools: Vec::new(),
            },
        )
        .map(|()| true)
        .map_err(|err: TraitError| err.to_string())
}

/// `(register-trait-resource-pool trait-id pool-id capacity-kind capacity-amount)`
/// ——追加声明「这个天赋授予某个资源池多少容量」（资源池落地批次，
/// `knowledge/design/trait-system.md` 三节④），与 `register-race-trait`
/// 相对 `register-race` 同一个「不改既有签名,新增能力用新函数」模式,
/// 见 [`crate::trait_def`] 模块文档「④授予资源池容量走的正是这条先例」
/// 一节。
///
/// - `trait-id`：已经通过 `register-trait` 注册过的完整命名空间标识符
///   字符串——目标必须已存在（ADR 0017「注册期完整校验」）。
/// - `pool-id`：已经通过 `register-resource-pool`
///   （[`crate::script_resource_pool_api`]）注册过的完整命名空间标识符
///   字符串——**要求**已存在（与 `granted-skills`/`register-race-trait`
///   的「只 intern 不跨表校验」不同：`resource-pools-and-rest.md` 三节
///   原文明确要求这里校验，见 `crate::trait_def` 模块文档）。
/// - `capacity-kind`：本批次只支持 `"fixed"`（容量恒定,不随等级变化）
///   ——`"by-level"`（阶梯式查表）需要一套本代码库尚未使用过的「列表套
///   元组」FFI 编码约定，留给法术位批次一起补上，见
///   [`crate::trait_def`] 模块文档「本批次范围」一节同一条 YAGNI 判断。
/// - `capacity-amount`：`capacity-kind` 为 `"fixed"` 时是容量数值,
///   非负整数。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_trait_resource_pool(
    trait_id: String,
    pool_id: String,
    capacity_kind: String,
    capacity_amount: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err(
                    "register-trait-resource-pool 在没有活跃天赋表的窗口内被调用".to_string(),
                );
            };
            do_register_trait_resource_pool(
                registry,
                table,
                &trait_id,
                &pool_id,
                &capacity_kind,
                capacity_amount,
            )
        })
    })
}

/// [`register_trait_resource_pool`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_trait_resource_pool(
    registry: &mut Registry,
    table: &mut TraitTable,
    trait_id: &str,
    pool_id: &str,
    capacity_kind: &str,
    capacity_amount: i64,
) -> Result<bool, String> {
    let parsed_trait_id = NamespacedId::parse(trait_id)
        .map_err(|err| format!("非法内容标识符 {trait_id:?}：{err}"))?;
    let Some(trait_index) = registry.get(&parsed_trait_id) else {
        return Err(format!("天赋 {trait_id:?} 尚未通过 register-trait 注册"));
    };
    let parsed_pool_id =
        NamespacedId::parse(pool_id).map_err(|err| format!("非法内容标识符 {pool_id:?}：{err}"))?;
    let Some(pool_index) = registry.get(&parsed_pool_id) else {
        return Err(format!(
            "资源池 {pool_id:?} 尚未通过 register-resource-pool 注册"
        ));
    };
    let capacity = match capacity_kind {
        "fixed" => CapacityFormula::Fixed(capacity_amount.max(0) as u32),
        _ => return Err(format!("未知的容量公式种类 {capacity_kind:?}")),
    };

    table
        .add_resource_pool_grant(
            trait_index,
            ResourcePoolGrant {
                pool: pool_index,
                capacity,
            },
        )
        .map(|()| true)
        .map_err(|err: TraitError| err.to_string())
}

/// `(register-trait-resource-pool-by-level trait-id pool-id level tier-amounts)`
/// ——追加声明「这个天赋在 `level` 级授予某个法术位池这一档分布」
/// （法术位落地批次），服务
/// `ResourcePoolShape::TieredSlots`：`register-trait-resource-pool`
/// 的 `"fixed"` 容量公式无法表达法术位（一个不分档的固定数回答不了
/// 「第几档有多少」这个问题，见 `ll_sim::resource_pool::eval_tier_formula`
/// 文档「形状不匹配」一节）——本函数是`ResourcePoolShape::TieredSlots`
/// 唯一的容量声明入口。
///
/// - `trait-id`/`pool-id`：同 `register-trait-resource-pool`。
/// - `level`：这一档分布从几级开始生效——阶梯式，不需要每级都调用一次
///   （`CapacityFormula::ByLevel` 未覆盖的等级取小于等于它的最大已声明
///   等级对应的值）。
/// - `tier-amounts`：这一级各档的容量,索引 0 = 第 1 档
///   （`ResourcePoolShape::TieredSlots` 文档），非负整数列表。
///
/// **多次调用同一个 `(trait-id, pool-id)` 组合会累积进同一张表**，不是
/// 各自新开一条独立的授予声明——见
/// [`crate::trait_def::TraitTable::add_resource_pool_grant_tiered_level`]
/// 文档「为什么不新开一条」一节：法术位一族典型要按等级声明好几个
/// 断点（"5 级 4 个一环位、9 级追加五环位"这类阶梯式增长），mod 脚本
/// 因此按等级从低到高多次调用本函数，各自追加同一张表里的一个断点。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_trait_resource_pool_by_level(
    trait_id: String,
    pool_id: String,
    level: i64,
    tier_amounts: Vec<i64>,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err(
                    "register-trait-resource-pool-by-level 在没有活跃天赋表的窗口内被调用"
                        .to_string(),
                );
            };
            do_register_trait_resource_pool_by_level(
                registry,
                table,
                &trait_id,
                &pool_id,
                level,
                &tier_amounts,
            )
        })
    })
}

/// [`register_trait_resource_pool_by_level`] 的纯函数核心，方便单元
/// 测试不必绕过 `thread_local!`。
fn do_register_trait_resource_pool_by_level(
    registry: &mut Registry,
    table: &mut TraitTable,
    trait_id: &str,
    pool_id: &str,
    level: i64,
    tier_amounts: &[i64],
) -> Result<bool, String> {
    let parsed_trait_id = NamespacedId::parse(trait_id)
        .map_err(|err| format!("非法内容标识符 {trait_id:?}：{err}"))?;
    let Some(trait_index) = registry.get(&parsed_trait_id) else {
        return Err(format!("天赋 {trait_id:?} 尚未通过 register-trait 注册"));
    };
    let parsed_pool_id =
        NamespacedId::parse(pool_id).map_err(|err| format!("非法内容标识符 {pool_id:?}：{err}"))?;
    let Some(pool_index) = registry.get(&parsed_pool_id) else {
        return Err(format!(
            "资源池 {pool_id:?} 尚未通过 register-resource-pool 注册"
        ));
    };
    let level = u32::try_from(level).map_err(|_| format!("非法等级断点 {level}（必须非负）"))?;
    let tiers: Vec<u32> = tier_amounts
        .iter()
        .map(|&amount| amount.max(0) as u32)
        .collect();

    table
        .add_resource_pool_grant_tiered_level(trait_index, pool_index, level, tiers)
        .map(|()| true)
        .map_err(|err: TraitError| err.to_string())
}

/// `(register-trait-resistance trait-id damage-category-id multiplier-permille)`
/// ——追加声明「这个天赋携带对某个伤害类别的抗性」（伤害类别/抗性接线
/// 批次新增，`knowledge/design/trait-system.md` 三节③），与
/// [`register_trait_resource_pool`] 同一个「新增能力用新函数」模式：
/// 不改 `register-trait` 已有的三参数签名。
///
/// - `trait-id`：已经通过 `register-trait` 注册过的完整命名空间标识符
///   字符串——目标必须已存在，同 [`register_trait_resource_pool`] 一条
///   ADR 0017「注册期完整校验」纪律。
/// - `damage-category-id`：已经通过 `register-damage-category`
///   （`crate::script_damage_category_api`）注册过的完整命名空间标识符
///   字符串——与 `pool-id` 未注册即拒绝同一条纪律，不允许静默创建一个
///   指向不存在类别的悬空抗性声明。
/// - `multiplier-permille`：千分比乘数（`0`=免疫，`500`=半伤，
///   `2000`=双倍），见 [`RuleModifier::Resistance`] 文档。允许为负？
///   不允许——负的抗性乘数没有设计依据（比"负伤害"更没有意义），钳到
///   零而不是拒绝整次调用，与本代码库其余"数值层面取舍而非拒绝"的既有
///   纪律一致（见 `crate::script_terrain_api::do_register_terrain`
///   对 `move_cost` 负值的处理）。
///
/// **追加，不是覆盖**——一个天赋可以同时声明对多个伤害类别的抗性，见
/// [`crate::trait_def::TraitTable::add_rule_modifier`] 文档。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_trait_resistance(
    trait_id: String,
    damage_category_id: String,
    multiplier_permille: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-trait-resistance 在没有活跃天赋表的窗口内被调用".to_string());
            };
            do_register_trait_resistance(
                registry,
                table,
                &trait_id,
                &damage_category_id,
                multiplier_permille,
            )
        })
    })
}

/// [`register_trait_resistance`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_trait_resistance(
    registry: &Registry,
    table: &mut TraitTable,
    trait_id: &str,
    damage_category_id: &str,
    multiplier_permille: i64,
) -> Result<bool, String> {
    let parsed_trait_id = NamespacedId::parse(trait_id)
        .map_err(|err| format!("非法内容标识符 {trait_id:?}：{err}"))?;
    let Some(trait_index) = registry.get(&parsed_trait_id) else {
        return Err(format!("天赋 {trait_id:?} 尚未通过 register-trait 注册"));
    };
    let parsed_category_id = NamespacedId::parse(damage_category_id)
        .map_err(|err| format!("非法内容标识符 {damage_category_id:?}：{err}"))?;
    let Some(category_index) = registry.get(&parsed_category_id) else {
        return Err(format!(
            "伤害类别 {damage_category_id:?} 尚未通过 register-damage-category 注册"
        ));
    };

    table
        .add_rule_modifier(
            trait_index,
            RuleModifier::Resistance {
                damage_category: category_index,
                multiplier_permille: multiplier_permille.max(0) as i32,
            },
        )
        .map(|()| true)
        .map_err(|err: TraitError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法天赋声明注册成功并写入天赋表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TraitTable::new();

        // Act
        let result = do_register_trait(
            &mut registry,
            &mut table,
            "yourmod:draconic_breath",
            "yourmod:draconic_breath_display_name",
            &["yourmod:breath_weapon".to_string()],
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:draconic_breath").unwrap())
            .expect("刚注册的内容应能查到索引");
        let skill_index = registry
            .get(&NamespacedId::parse("yourmod:breath_weapon").unwrap())
            .expect("register-trait 应当 intern 出技能索引");
        assert_eq!(table.get(index).unwrap().granted_skills, &[skill_index]);
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TraitTable::new();

        // Act
        let result = do_register_trait(
            &mut registry,
            &mut table,
            "InvalidNamespace:foo",
            "yourmod:foo_display_name",
            &[],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 重复定义同一个天赋索引返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TraitTable::new();
        do_register_trait(
            &mut registry,
            &mut table,
            "yourmod:halfling_luck",
            "yourmod:halfling_luck_display_name",
            &[],
        )
        .expect("首次注册应当成功");

        // Act
        let result = do_register_trait(
            &mut registry,
            &mut table,
            "yourmod:halfling_luck",
            "yourmod:halfling_luck_display_name",
            &[],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_trait_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(TraitTable::new());

        // Act：非法命名空间字符串。
        let result = engine.load_source(
            r#"(register-trait "Invalid Namespace" "yourmod:foo" (list))"#.to_string(),
        );

        // Assert
        assert!(result.is_err());
        // 清理线程局部状态，避免污染同一进程内的其它测试。
        let _ = crate::active_registry::take_active_registry();
        let _ = take_active_target();
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_trait() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_trait_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(TraitTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-trait "yourmod:draconic_breath" "yourmod:draconic_breath_display_name" (list "yourmod:breath_weapon"))"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:draconic_breath").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).unwrap().granted_skills.len(), 1);
    }

    #[test]
    fn 合法资源池容量声明追加成功且fixed公式数值正确() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TraitTable::new();
        do_register_trait(
            &mut registry,
            &mut table,
            "yourmod:innate_sorcery",
            "yourmod:innate_sorcery_display_name",
            &[],
        )
        .expect("先注册天赋本体");
        let pool = registry.intern(NamespacedId::parse("yourmod:sorcery_points").unwrap());

        // Act
        let result = do_register_trait_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:innate_sorcery",
            "yourmod:sorcery_points",
            "fixed",
            20,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:innate_sorcery").unwrap())
            .unwrap();
        let grants = &table.get(index).unwrap().granted_resource_pools;
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].pool, pool);
        assert_eq!(grants[0].capacity, CapacityFormula::Fixed(20));
    }

    #[test]
    fn 目标天赋尚未注册时资源池容量声明返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TraitTable::new();
        registry.intern(NamespacedId::parse("yourmod:sorcery_points").unwrap());

        // Act
        let result = do_register_trait_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:never_registered",
            "yourmod:sorcery_points",
            "fixed",
            10,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 目标资源池尚未注册时容量声明返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TraitTable::new();
        do_register_trait(
            &mut registry,
            &mut table,
            "yourmod:innate_sorcery",
            "yourmod:innate_sorcery_display_name",
            &[],
        )
        .expect("先注册天赋本体");

        // Act：pool-id 从未被 register-resource-pool 注册过。
        let result = do_register_trait_resource_pool(
            &mut registry,
            &mut table,
            "yourmod:innate_sorcery",
            "yourmod:never_registered_pool",
            "fixed",
            10,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_trait_resource_pool() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_trait_api(&mut engine);
        let mut registry = Registry::new();
        let trait_index = registry.intern(NamespacedId::parse("yourmod:innate_sorcery").unwrap());
        let pool = registry.intern(NamespacedId::parse("yourmod:sorcery_points").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                trait_index,
                TraitAttrs {
                    display_name_key: NamespacedId::parse("yourmod:trait.innate_sorcery").unwrap(),
                    granted_skills: Vec::new(),
                    stat_modifiers: Vec::new(),
                    rule_modifiers: Vec::new(),
                    granted_resource_pools: Vec::new(),
                },
            )
            .expect("先注册天赋本体");
        crate::active_registry::set_active_registry(registry);
        set_active_target(table);

        // Act
        let result = engine.load_source(
            r#"(register-trait-resource-pool "yourmod:innate_sorcery" "yourmod:sorcery_points" "fixed" 20)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:innate_sorcery").unwrap())
            .unwrap();
        let grants = &table.get(index).unwrap().granted_resource_pools;
        assert_eq!(
            grants,
            &[ResourcePoolGrant {
                pool,
                capacity: CapacityFormula::Fixed(20),
            }]
        );
    }

    #[test]
    fn 合法法术位分布追加成功且tiered公式档位数值正确() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TraitTable::new();
        do_register_trait(
            &mut registry,
            &mut table,
            "yourmod:arcane_casting",
            "yourmod:arcane_casting_display_name",
            &[],
        )
        .expect("先注册天赋本体");
        let pool = registry.intern(NamespacedId::parse("yourmod:wizard_slots").unwrap());

        // Act
        let result = do_register_trait_resource_pool_by_level(
            &mut registry,
            &mut table,
            "yourmod:arcane_casting",
            "yourmod:wizard_slots",
            5,
            &[4, 3, 2],
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:arcane_casting").unwrap())
            .unwrap();
        let grants = &table.get(index).unwrap().granted_resource_pools;
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].pool, pool);
        assert_eq!(
            grants[0].capacity,
            CapacityFormula::ByLevel(std::collections::BTreeMap::from([(
                5,
                crate::trait_def::CapacityValue::Tiered(vec![4, 3, 2])
            )]))
        );
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_trait_resource_pool_by_level() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_trait_api(&mut engine);
        let mut registry = Registry::new();
        let trait_index = registry.intern(NamespacedId::parse("yourmod:arcane_casting").unwrap());
        let pool = registry.intern(NamespacedId::parse("yourmod:wizard_slots").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                trait_index,
                TraitAttrs {
                    display_name_key: NamespacedId::parse("yourmod:trait.arcane_casting").unwrap(),
                    granted_skills: Vec::new(),
                    stat_modifiers: Vec::new(),
                    rule_modifiers: Vec::new(),
                    granted_resource_pools: Vec::new(),
                },
            )
            .expect("先注册天赋本体");
        crate::active_registry::set_active_registry(registry);
        set_active_target(table);

        // Act：两次调用,累积进同一张 ByLevel 表。
        let first = engine.load_source(
            r#"(register-trait-resource-pool-by-level "yourmod:arcane_casting" "yourmod:wizard_slots" 1 (list 2 0 0))"#
                .to_string(),
        );
        let second = engine.load_source(
            r#"(register-trait-resource-pool-by-level "yourmod:arcane_casting" "yourmod:wizard_slots" 3 (list 4 2 0))"#
                .to_string(),
        );

        // Assert
        assert!(first.is_ok());
        assert!(second.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:arcane_casting").unwrap())
            .unwrap();
        let grants = &table.get(index).unwrap().granted_resource_pools;
        assert_eq!(grants.len(), 1, "两次调用应当合并进同一条授予声明");
        assert_eq!(grants[0].pool, pool);
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_trait_resistance() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_trait_api(&mut engine);
        let mut registry = Registry::new();
        let trait_index = registry.intern(NamespacedId::parse("yourmod:fire_resistance").unwrap());
        let fire = registry.intern(NamespacedId::parse("yourmod:fire").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                trait_index,
                TraitAttrs {
                    display_name_key: NamespacedId::parse("yourmod:trait.fire_resistance").unwrap(),
                    granted_skills: Vec::new(),
                    stat_modifiers: Vec::new(),
                    rule_modifiers: Vec::new(),
                    granted_resource_pools: Vec::new(),
                },
            )
            .expect("先注册天赋本体");
        crate::active_registry::set_active_registry(registry);
        set_active_target(table);

        // Act
        let result = engine.load_source(
            r#"(register-trait-resistance "yourmod:fire_resistance" "yourmod:fire" 500)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:fire_resistance").unwrap())
            .unwrap();
        assert_eq!(
            table.get(index).unwrap().rule_modifiers,
            &[RuleModifier::Resistance {
                damage_category: fire,
                multiplier_permille: 500,
            }]
        );
    }

    #[test]
    fn 伤害类别未注册时register_trait_resistance失败而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_trait_api(&mut engine);
        let mut registry = Registry::new();
        let trait_index = registry.intern(NamespacedId::parse("yourmod:fire_resistance").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                trait_index,
                TraitAttrs {
                    display_name_key: NamespacedId::parse("yourmod:trait.fire_resistance").unwrap(),
                    granted_skills: Vec::new(),
                    stat_modifiers: Vec::new(),
                    rule_modifiers: Vec::new(),
                    granted_resource_pools: Vec::new(),
                },
            )
            .expect("先注册天赋本体");
        crate::active_registry::set_active_registry(registry);
        set_active_target(table);

        // Act
        let result = engine.load_source(
            r#"(register-trait-resistance "yourmod:fire_resistance" "yourmod:never_registered" 500)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn 目标天赋尚未注册时register_trait_resistance失败而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_trait_api(&mut engine);
        let mut registry = Registry::new();
        registry.intern(NamespacedId::parse("yourmod:fire").unwrap());
        crate::active_registry::set_active_registry(registry);
        set_active_target(TraitTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-trait-resistance "yourmod:never_registered_trait" "yourmod:fire" 500)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
