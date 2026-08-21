//! 把 `register-item`/`register-item-equip-mask` 注册进脚本引擎：mod
//! 脚本借此定义自定义物品（箭矢、铁剑……）与它们的装备占位掩码，落地
//! `knowledge/design/item-system.md`/`knowledge/design/equipment-slots.md`。
//!
//! 模式同 [`crate::script_resource_pool_api`]：扁平参数,没有为
//! `Option<i32>`（`max-durability`）或 `Milli`（`base-weight`/
//! `base-price`）发明任何新的 FFI 编码方式,理由见下面两节。
//!
//! # 为什么 `equip_mask` 走独立函数（装备栏位批次，P6 第三批）
//!
//! `register-item` 已经是仓库里真实 mod 脚本
//! （`mods/example_mod/gameplay.scm`）在用的六参数签名——改参数个数
//! 会破坏已有脚本，与 `register-race-xp-reward`/
//! `register-trait-resource-pool` 「新增能力用新函数」同一条既有先例
//! （见 [`crate::item::ItemDef::equip_mask`] 文档）。`register-item-equip-mask`
//! 因此是本批次新增的第二个函数，追加对象是已经通过 `register-item`
//! 注册过的物品。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_core::scaled::Milli;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::item::{ItemAttrs, ItemError, ItemTable};
use crate::registry::Registry;
use ll_sim::item::{EquipSlot, SlotMask};

thread_local! {
    /// 当前调用窗口内，`register-item` 应该写入的物品表。
    static ACTIVE_TABLE: RefCell<Option<ItemTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-item` 可写入的目标。
pub fn set_active_target(table: ItemTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `ItemTable`。
pub fn take_active_target() -> ItemTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-item`/`register-item-equip-mask` 注册进 `engine`。
pub fn register_item_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-item", register_item);
    engine.register_fn("register-item-equip-mask", register_item_equip_mask);
}

/// `(register-item id display-name-key stack-limit base-weight base-price max-durability)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
/// - `stack-limit`：堆叠上限，必须 ≥ 1（`0` 没有意义——一堆连一个都
///   装不下的物品不该存在，直接拒绝而不是静默钳位成 1，理由同
///   `register-resource-pool` 拒绝 `tier-count == 0` 的文档）。`1`
///   表示不可堆叠。
/// - `base-weight`/`base-price`：以 `Milli` 千分之一为单位的**原始**
///   整数——`Milli(1_500)` 表示 1.5，这里的参数就是 `1500`,不是
///   "整数会被自动乘 1000"那种写法，与 `Milli` 自身文档「`Milli(1_500)`
///   表示 1.5」同一个换算关系，没有为它另外发明一层"填整数、内部
///   放大"的转换（那会让内容作者搞不清一个数字究竟是"1.5"还是
///   "填 1 会自动变 1000"，读脚本时也看不出来）。
/// - `max-durability`：耐久上限，`-1` 表示这件物品没有耐久概念
///   （`None`），`>= 0` 表示有（`Some`）——与 `register-terrain` 的
///   `opens-into` 用空串表示 `None` 是同一条"用一个该字段合法值域之外
///   的哨兵表示空"的既有约定，只是这里的字段是数值,空串哨兵不适用，
///   改用负数（耐久上限本身不该是负的，`-1` 因此是安全的哨兵）。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item(
    id: String,
    display_name_key: String,
    stack_limit: i64,
    base_weight: i64,
    base_price: i64,
    max_durability: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item(
                registry,
                table,
                &id,
                &display_name_key,
                stack_limit,
                base_weight,
                base_price,
                max_durability,
            )
        })
    })
}

/// [`register_item`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_item(
    registry: &mut Registry,
    table: &mut ItemTable,
    id: &str,
    display_name_key: &str,
    stack_limit: i64,
    base_weight: i64,
    base_price: i64,
    max_durability: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;

    if stack_limit < 1 {
        return Err(format!("堆叠上限 {stack_limit} 非法（必须 >= 1）"));
    }
    let max_durability = match max_durability {
        -1 => None,
        value if value >= 0 => Some(value as i32),
        other => {
            return Err(format!(
                "耐久上限 {other} 非法（必须 >= 0，或用 -1 表示无耐久）"
            ));
        }
    };

    table
        .define(
            index,
            ItemAttrs {
                display_name_key,
                stack_limit: stack_limit as u32,
                base_weight: Milli(base_weight),
                base_price: Milli(base_price),
                max_durability,
                // 恒为空——register-item 的六参数签名不接受装备占位
                // 掩码，真正的取值由后续 register-item-equip-mask 调用
                // 写入，见模块文档「为什么 equip_mask 走独立函数」一节。
                equip_mask: SlotMask::EMPTY,
            },
        )
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

/// `(register-item-equip-mask id slot-names)`——追加声明「这件物品占用
/// 哪些装备槽位」（装备栏位批次，P6 第三批），见
/// [`crate::item::ItemDef::equip_mask`] 文档「为什么不是 `register-item`
/// 的参数」一节。
///
/// - `id`：已经通过 `register-item` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在（ADR 0017「注册期完整校验」），未注册的 `id`
///   在装载期报错，而不是静默创建一条只有占位掩码、没有其余属性的
///   半成品物品记录，与 `register-race-xp-reward` 同一条纪律。
/// - `slot-names`：`knowledge/design/equipment-slots.md` 槽位表的
///   kebab-case 名称列表（`"main-hand"`/`"off-hand"`/……22 个引擎槽位
///   之一，见 [`ll_sim::item::EquipSlot::from_name`]）——不可为空列表
///   （空列表没有意义：一件"不占用任何槽位"的物品不该调用本函数,
///   `SlotMask::EMPTY` 已经是 `register-item` 注册时的默认值）。多个
///   名称按位或合并成最终掩码——双手武器传
///   `(list "main-hand" "off-hand")`，全身板甲传七个槽位名称的列表。
///   任意一个名称不在 22 个引擎槽位表内即拒绝整次调用（不静默忽略
///   未知名称,理由同 `register-item` 拒绝非法内容标识符）。
///
/// **覆盖，不是追加**——多次调用同一个 `id` 以最后一次为准，见
/// [`crate::item::ItemTable::set_equip_mask`] 文档「覆盖，不是追加」
/// 一节。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item_equip_mask(id: String, slot_names: Vec<String>) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item-equip-mask 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item_equip_mask(registry, table, &id, &slot_names)
        })
    })
}

/// [`register_item_equip_mask`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_item_equip_mask(
    registry: &Registry,
    table: &mut ItemTable,
    id: &str,
    slot_names: &[String],
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("物品 {id:?} 尚未通过 register-item 注册"));
    };
    if slot_names.is_empty() {
        return Err("装备占位掩码不能是空列表".to_string());
    }

    let mut mask = SlotMask::EMPTY;
    for name in slot_names {
        let Some(slot) = EquipSlot::from_name(name) else {
            return Err(format!("未知的装备槽位名称 {name:?}"));
        };
        mask = mask.union(slot.mask());
    }

    table
        .set_equip_mask(index, mask)
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法可堆叠物品声明注册成功并写入物品表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:arrow").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的物品应能查到属性");
        assert_eq!(view.stack_limit, 99);
        assert_eq!(view.base_price, Milli(2000));
        assert_eq!(view.max_durability, None);
    }

    #[test]
    fn 合法不可堆叠物品声明携带耐久上限() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:iron_sword",
            "yourmod:item.iron_sword",
            1,
            3000,
            50000,
            100,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:iron_sword").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的物品应能查到属性");
        assert_eq!(view.stack_limit, 1);
        assert_eq!(view.max_durability, Some(100));
    }

    #[test]
    fn 堆叠上限为零时返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:item.x",
            0,
            0,
            0,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 耐久上限小于负一时返回错误而不panic() {
        // Arrange：-2 不是合法的"无耐久"哨兵（只有 -1 是）。
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:item.x",
            1,
            0,
            0,
            -2,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "InvalidNamespace:foo",
            "yourmod:item.foo",
            1,
            0,
            0,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 重复定义同一个物品索引返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        )
        .expect("首次注册应当成功");

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-item "yourmod:arrow" "yourmod:item.arrow" 99 50 2000 -1)"#.to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:arrow").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).unwrap().stack_limit, 99);
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：堆叠上限为零——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());

        // Act
        let result = engine
            .load_source(r#"(register-item "yourmod:x" "yourmod:item.x" 0 0 0 -1)"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_trait_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    /// 建一张已经注册过一件不可堆叠武器（模拟大斧）的物品表 + 对应的
    /// registry——`register-item-equip-mask` 的测试共用这份前置状态。
    fn registry_and_table_with_great_axe() -> (Registry, ItemTable) {
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        do_register_item(
            &mut registry,
            &mut table,
            "yourmod:great_axe",
            "yourmod:item.great_axe",
            1,
            5000,
            8000,
            120,
        )
        .expect("大斧注册应当成功");
        (registry, table)
    }

    #[test]
    fn 多个槽位名称按位或合并成最终掩码() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result = do_register_item_equip_mask(
            &registry,
            &mut table,
            "yourmod:great_axe",
            &["main-hand".to_string(), "off-hand".to_string()],
        );

        // Assert
        assert_eq!(result, Ok(true));
        let expected = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());
        assert_eq!(table.get(index).unwrap().equip_mask, expected);
    }

    #[test]
    fn 未注册的物品id追加装备掩码返回错误() {
        // Arrange
        let registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item_equip_mask(
            &registry,
            &mut table,
            "yourmod:never_registered",
            &["head".to_string()],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 未知槽位名称返回错误而不panic() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();

        // Act
        let result = do_register_item_equip_mask(
            &registry,
            &mut table,
            "yourmod:great_axe",
            &["tail".to_string()],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 空槽位名称列表返回错误而不panic() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();

        // Act
        let result = do_register_item_equip_mask(&registry, &mut table, "yourmod:great_axe", &[]);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item_equip_mask() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());
        engine
            .load_source(
                r#"(register-item "yourmod:great_axe" "yourmod:item.great_axe" 1 5000 8000 120)"#
                    .to_string(),
            )
            .expect("大斧基础注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(register-item-equip-mask "yourmod:great_axe" (list "main-hand" "off-hand"))"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");
        let expected = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());
        assert_eq!(table.get(index).unwrap().equip_mask, expected);
    }
}
