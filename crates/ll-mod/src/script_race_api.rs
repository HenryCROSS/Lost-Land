//! 把 `register-race` 注册进脚本引擎：mod 脚本借此定义自定义种族。
//!
//! 模式同 [`crate::script_class_api`]。种族比职业多出四个数值字段
//! （六项属性修正 + 暗视下限 + 体型两维 + 寿命），FFI 签名因此更长，
//! 但每个参数都是简单的整数，不需要像 [`crate::script_skill_api`] 那样
//! 处理带标签的枚举。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use ll_sim::traits::TraitGrant;
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

/// 把 `register-race`/`register-race-xp-reward`/`register-race-trait`/
/// `register-race-starting-item` 注册进 `engine`。
pub fn register_race_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-race", register_race);
    engine.register_fn("register-race-xp-reward", register_race_xp_reward);
    engine.register_fn("register-race-trait", register_race_trait);
    engine.register_fn("register-race-starting-item", register_race_starting_item);
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
                // register-race 的既有脚本签名不携带经验值（不能改
                // 参数个数，见 crate::race 模块文档「与
                // register-race-xp-reward 的关系」一节）——这里恒填 0，
                // 真正想声明非零经验值的 mod 作者需要额外调用
                // register-race-xp-reward。
                xp_reward: 0,
                traits: Vec::new(),
                starting_items: Vec::new(),
            },
        )
        .map(|()| true)
        .map_err(|err: RaceError| err.to_string())
}

/// `(register-race-xp-reward id amount)`——追加声明「杀死这个种族给
/// 多少经验」，见 [`crate::race`] 模块文档「与 `register-race-xp-reward`
/// 的关系」一节：不改 `register-race` 既有签名,新能力走新函数。
///
/// - `id`：已经通过 `register-race` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在（ADR 0017「注册期完整校验」），未注册的 `id`
///   在装载期报错，而不是静默创建一条只有经验值、没有其余属性的半成品
///   种族记录。
/// - `amount`：击杀经验值,允许为 0（等价于不声明）,但不允许写成
///   负数——负经验没有设计动机（杀怪倒扣经验不是本次要支持的玩法）。
///
/// 返回 `Result<bool, String>`，理由同 `register_race` 文档。
fn register_race_xp_reward(id: String, amount: i64) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-race-xp-reward 在没有活跃种族表的窗口内被调用".to_string());
            };
            do_register_race_xp_reward(registry, table, &id, amount)
        })
    })
}

/// [`register_race_xp_reward`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_race_xp_reward(
    registry: &Registry,
    table: &mut RaceTable,
    id: &str,
    amount: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("种族 {id:?} 尚未通过 register-race 注册"));
    };
    if amount < 0 {
        return Err(format!("击杀经验值不允许为负数：{amount}"));
    }
    table
        .set_xp_reward(index, amount)
        .map(|()| true)
        .map_err(|err: RaceError| err.to_string())
}

/// `(register-race-trait race-id trait-id unlock-level)`——追加声明
/// 「这个种族在某个等级授予某个天赋」（天赋系统落地批次，
/// `knowledge/design/trait-system.md` 四、六节），与
/// `register-race-xp-reward` 同一个「不改既有签名,新增能力用新函数」
/// 模式，见 [`crate::race`] 模块文档「与 `register-race-xp-reward`
/// 的关系」一节同一条先例。
///
/// - `race-id`：已经通过 `register-race` 注册过的完整命名空间标识符
///   字符串——目标必须已存在（ADR 0017「注册期完整校验」）。
/// - `trait-id`：天赋的完整命名空间标识符字符串——**不要求**已经通过
///   `register-trait` 注册过（只 `intern`，不跨表校验存在性，见
///   [`crate::race::RaceTable::add_trait_grant`] 文档「不校验」一节，
///   这是当前代码库尚未建立跨表校验基础设施的已知简化）。
/// - `unlock-level`：解锁所需等级，非负整数——种族天赋按
///   `trait-system.md` 六节的既有纪律恒传 `1`（"拥有即生效"），但本函数
///   不强制这一点（允许 mod 作者声明"N 级矮人才有抗毒"这类非典型设计,
///   校验只保证非负,不替内容作者做设计决定）。
///
/// 返回 `Result<bool, String>`，理由同 `register_race` 文档。
fn register_race_trait(
    race_id: String,
    trait_id: String,
    unlock_level: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-race-trait 在没有活跃种族表的窗口内被调用".to_string());
            };
            do_register_race_trait(registry, table, &race_id, &trait_id, unlock_level)
        })
    })
}

/// [`register_race_trait`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_race_trait(
    registry: &mut Registry,
    table: &mut RaceTable,
    race_id: &str,
    trait_id: &str,
    unlock_level: i64,
) -> Result<bool, String> {
    let parsed_race_id =
        NamespacedId::parse(race_id).map_err(|err| format!("非法内容标识符 {race_id:?}：{err}"))?;
    let Some(race_index) = registry.get(&parsed_race_id) else {
        return Err(format!("种族 {race_id:?} 尚未通过 register-race 注册"));
    };
    let parsed_trait_id = NamespacedId::parse(trait_id)
        .map_err(|err| format!("非法内容标识符 {trait_id:?}：{err}"))?;
    if unlock_level < 0 {
        return Err(format!("解锁等级不允许为负数：{unlock_level}"));
    }
    let trait_index = registry.intern(parsed_trait_id);
    table
        .add_trait_grant(
            race_index,
            TraitGrant {
                trait_id: trait_index,
                unlock_level: unlock_level as i32,
            },
        )
        .map(|()| true)
        .map_err(|err: RaceError| err.to_string())
}

/// `(register-race-starting-item race-id item-id count)`——追加声明
/// 「这个种族出生携带一件物品」（NPC 生命周期批次：NPC 带物品 → 死亡
/// 掉落 → 尸体 → 老化回收），与 `register-race-trait` 同一个「不改
/// 既有签名,新增能力用新函数」模式，见 [`crate::race`] 模块文档「与
/// `register-race-xp-reward` 的关系」一节同一条先例。
///
/// - `race-id`：已经通过 `register-race` 注册过的完整命名空间标识符
///   字符串——目标必须已存在（ADR 0017「注册期完整校验」）。
/// - `item-id`：物品的完整命名空间标识符字符串——**不要求**已经通过
///   `register-item` 注册过（只 `intern`，不跨表校验存在性，理由同
///   `register-race-trait` 文档「不要求」一节，是当前代码库尚未建立
///   跨表校验基础设施的已知简化）。
/// - `count`：携带的数量，必须 `>= 1`——与 `ItemStack.count` 「恒 ≥ 1」
///   的既有不变式对齐（见 `ll_world::item::ItemStack::count` 文档），
///   `0` 没有意义（携带零个等于不携带，应当干脆不调用本函数）。
///
/// 返回 `Result<bool, String>`，理由同 `register_race` 文档。
fn register_race_starting_item(
    race_id: String,
    item_id: String,
    count: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err(
                    "register-race-starting-item 在没有活跃种族表的窗口内被调用".to_string()
                );
            };
            do_register_race_starting_item(registry, table, &race_id, &item_id, count)
        })
    })
}

/// [`register_race_starting_item`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_race_starting_item(
    registry: &mut Registry,
    table: &mut RaceTable,
    race_id: &str,
    item_id: &str,
    count: i64,
) -> Result<bool, String> {
    let parsed_race_id =
        NamespacedId::parse(race_id).map_err(|err| format!("非法内容标识符 {race_id:?}：{err}"))?;
    let Some(race_index) = registry.get(&parsed_race_id) else {
        return Err(format!("种族 {race_id:?} 尚未通过 register-race 注册"));
    };
    let parsed_item_id =
        NamespacedId::parse(item_id).map_err(|err| format!("非法内容标识符 {item_id:?}：{err}"))?;
    if count < 1 {
        return Err(format!("出生物品数量必须 >= 1：{count}"));
    }
    let item_index = registry.intern(parsed_item_id);
    table
        .add_starting_item(race_index, item_index, count as u32)
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

    #[test]
    fn 追加声明经验值对已注册种族生效() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();
        do_register_race(
            &mut registry,
            &mut table,
            "yourmod:goblin",
            "yourmod:goblin_display_name",
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
            20,
        )
        .expect("先注册种族本体");

        // Act
        let result = do_register_race_xp_reward(&registry, &mut table, "yourmod:goblin", 15);

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:goblin").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).unwrap().xp_reward, 15);
    }

    #[test]
    fn 对尚未注册的种族追加声明经验值返回err() {
        // Arrange
        let registry = Registry::new();
        let mut table = RaceTable::new();

        // Act
        let result = do_register_race_xp_reward(&registry, &mut table, "yourmod:never_seen", 10);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 负数经验值返回err而不写入表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();
        do_register_race(
            &mut registry,
            &mut table,
            "yourmod:goblin",
            "yourmod:goblin_display_name",
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
            20,
        )
        .expect("先注册种族本体");

        // Act
        let result = do_register_race_xp_reward(&registry, &mut table, "yourmod:goblin", -5);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_race_xp_reward() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_race_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RaceTable::new());
        engine
            .load_source(
                r#"(register-race "yourmod:goblin" "yourmod:goblin_display_name" 0 0 0 0 0 0 0 1 1 20)"#
                    .to_string(),
            )
            .expect("先注册种族本体");

        // Act
        let result =
            engine.load_source(r#"(register-race-xp-reward "yourmod:goblin" 15)"#.to_string());

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:goblin").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).unwrap().xp_reward, 15);
    }

    #[test]
    fn 合法天赋引用声明追加成功并写入种族表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();
        do_register_race(
            &mut registry,
            &mut table,
            "yourmod:dragonborn",
            "yourmod:dragonborn_display_name",
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
        )
        .expect("先注册种族本体");

        // Act
        let result = do_register_race_trait(
            &mut registry,
            &mut table,
            "yourmod:dragonborn",
            "yourmod:draconic_breath",
            1,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let race_index = registry
            .get(&NamespacedId::parse("yourmod:dragonborn").unwrap())
            .expect("刚注册的内容应能查到索引");
        let trait_index = registry
            .get(&NamespacedId::parse("yourmod:draconic_breath").unwrap())
            .expect("register-race-trait 应当 intern 出天赋索引");
        let grants = table.get(race_index).unwrap().traits;
        assert_eq!(
            grants,
            &[TraitGrant {
                trait_id: trait_index,
                unlock_level: 1,
            }]
        );
    }

    #[test]
    fn 对尚未注册的种族追加声明天赋引用返回err() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();

        // Act
        let result = do_register_race_trait(
            &mut registry,
            &mut table,
            "yourmod:never_seen",
            "yourmod:some_trait",
            1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 负数解锁等级返回err而不写入表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();
        do_register_race(
            &mut registry,
            &mut table,
            "yourmod:dragonborn",
            "yourmod:dragonborn_display_name",
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
        )
        .expect("先注册种族本体");

        // Act
        let result = do_register_race_trait(
            &mut registry,
            &mut table,
            "yourmod:dragonborn",
            "yourmod:draconic_breath",
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 同一个种族可以被追加多条不同的天赋引用() {
        // Arrange：验证 add_trait_grant 是"追加"而不是"覆盖"——两次
        // 调用 register-race-trait 都应当真实生效，不是后者覆盖前者。
        let mut registry = Registry::new();
        let mut table = RaceTable::new();
        do_register_race(
            &mut registry,
            &mut table,
            "yourmod:dwarf",
            "yourmod:dwarf_display_name",
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
            250,
        )
        .expect("先注册种族本体");
        do_register_race_trait(
            &mut registry,
            &mut table,
            "yourmod:dwarf",
            "yourmod:dwarven_resilience",
            1,
        )
        .expect("首次追加应当成功");

        // Act
        do_register_race_trait(
            &mut registry,
            &mut table,
            "yourmod:dwarf",
            "yourmod:stonecunning",
            1,
        )
        .expect("第二次追加应当成功");

        // Assert
        let race_index = registry
            .get(&NamespacedId::parse("yourmod:dwarf").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(race_index).unwrap().traits.len(), 2);
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_race_trait() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_race_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RaceTable::new());
        engine
            .load_source(
                r#"(register-race "yourmod:dragonborn" "yourmod:dragonborn_display_name" 0 0 0 0 0 0 0 1 1 80)"#
                    .to_string(),
            )
            .expect("先注册种族本体");

        // Act
        let result = engine.load_source(
            r#"(register-race-trait "yourmod:dragonborn" "yourmod:draconic_breath" 1)"#.to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let race_index = registry
            .get(&NamespacedId::parse("yourmod:dragonborn").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(race_index).unwrap().traits.len(), 1);
    }

    #[test]
    fn 合法出生物品声明追加成功并写入种族表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();
        do_register_race(
            &mut registry,
            &mut table,
            "yourmod:goblin",
            "yourmod:goblin_display_name",
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
            5,
        )
        .expect("先注册种族本体");

        // Act
        let result = do_register_race_starting_item(
            &mut registry,
            &mut table,
            "yourmod:goblin",
            "yourmod:crude_dagger",
            1,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let race_index = registry
            .get(&NamespacedId::parse("yourmod:goblin").unwrap())
            .expect("刚注册的内容应能查到索引");
        let item_index = registry
            .get(&NamespacedId::parse("yourmod:crude_dagger").unwrap())
            .expect("register-race-starting-item 应当 intern 出物品索引");
        assert_eq!(
            table.get(race_index).unwrap().starting_items,
            &[(item_index, 1)]
        );
    }

    #[test]
    fn 对尚未注册的种族追加声明出生物品返回err() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();

        // Act
        let result = do_register_race_starting_item(
            &mut registry,
            &mut table,
            "yourmod:never_seen",
            "yourmod:crude_dagger",
            1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 数量小于一的出生物品声明返回err而不写入表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = RaceTable::new();
        do_register_race(
            &mut registry,
            &mut table,
            "yourmod:goblin",
            "yourmod:goblin_display_name",
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
            5,
        )
        .expect("先注册种族本体");

        // Act
        let result = do_register_race_starting_item(
            &mut registry,
            &mut table,
            "yourmod:goblin",
            "yourmod:crude_dagger",
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_race_starting_item() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_race_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(RaceTable::new());
        engine
            .load_source(
                r#"(register-race "yourmod:goblin" "yourmod:goblin_display_name" 0 0 0 0 0 0 0 1 1 5)"#
                    .to_string(),
            )
            .expect("先注册种族本体");

        // Act
        let result = engine.load_source(
            r#"(register-race-starting-item "yourmod:goblin" "yourmod:crude_dagger" 1)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let race_index = registry
            .get(&NamespacedId::parse("yourmod:goblin").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(race_index).unwrap().starting_items.len(), 1);
    }
}
