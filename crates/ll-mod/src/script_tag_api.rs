//! 把 `register-tag` 注册进脚本引擎（耐久标签批次）。
//!
//! 与 [`crate::script_damage_category_api`] 同一套 `thread_local!` +
//! `ACTIVE_TABLE` 手法，模块文档不重复其论证。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use ll_sim::item::WearChannels;

use crate::active_registry::with_active_registry;
use crate::registry::Registry;
use crate::tag::{TagDef, TagError, TagTable};

thread_local! {
    /// 当前调用窗口内，`register-tag` 应该写入的标签表。
    static ACTIVE_TABLE: RefCell<Option<TagTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内可写入的目标。
pub fn set_active_target(table: TagTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 [`TagTable`]。
pub fn take_active_target() -> TagTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 借用当前调用窗口内的活跃标签表（只读）——供
/// [`crate::script_item_api`] 的 `register-item-tag` 在写物品表的同时
/// 查一下「这个 id 真的是个标签吗、它声明了哪些磨损通道」。
///
/// # 为什么跨模块借另一张表的 `thread_local!`
///
/// `register-item-tag` 是**唯一**需要同时看两张表的注册函数：它要把
/// 标签挂到物品上（写物品表），并把这条标签声明的磨损通道折算进物品的
/// 派生列（读标签表，见 `ll_mod::item::ItemTable::add_tag` 文档「为什么
/// 在这里折算」一节）。两张表各自的 `ACTIVE_TABLE` 是**两个独立的
/// `RefCell`**，同一个线程上一个 `borrow_mut()`、一个 `borrow()` 不会
/// 互相冲突；两者也都由 `crate::pipeline::compile_one_script` 在同一个
/// 窗口里成对 `set`/`take`，生命周期完全对齐。
///
/// 另一条路是把标签表也塞进 `script_item_api` 自己的 `thread_local!`,
/// 那会让「谁拥有标签表」这个问题有两个答案,并且 `register-tag` 与
/// `register-item-tag` 会写到两份不同的表里——那是真正的 bug 温床。
pub(crate) fn with_active_tag_table<R>(f: impl FnOnce(Option<&TagTable>) -> R) -> R {
    ACTIVE_TABLE.with(|cell| f(cell.borrow().as_ref()))
}

/// 把 `register-tag` 注册进 `engine`。
pub fn register_tag_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-tag", register_tag);
}

/// `(register-tag id wear-channel-names)`——声明一个标签存在，见
/// [`crate::tag`] 模块文档。
///
/// - `id`：完整命名空间标识符字符串。同一个 id 重复注册即拒绝整次调用
///   （[`TagError::DuplicateDefinition`]），与其余 `register-*` 一致。
/// - `wear-channel-names`：这个标签给物品带来哪些**耐久磨损通道**的
///   kebab-case 名称列表——`"on-hit"`（挨打时磨损，防具/衣物）与
///   `"on-use"`（使用时磨损，武器/工具），见
///   [`WearChannels::from_name`]。多个名称按位或合并。任意一个名称不在
///   这两个之内即拒绝整次调用（不静默忽略未知名称，理由同
///   `register-item-equip-mask` 对未知槽位名的处理）。
///
/// # 为什么**允许**空列表，而 `register-item-equip-mask` 不允许
///
/// 两者的空列表含义完全不同。一件"不占用任何槽位"的物品不该调用
/// `register-item-equip-mask`（`SlotMask::EMPTY` 已经是注册时的默认
/// 值，调用它只能是写错了）；而一个"与耐久无关"的**纯分类标签**
/// （将来的「可燃」「金属」「贵重」）是标签最常见的形态——标签这个
/// 机制本身不是为耐久发明的，耐久只是它今天唯一接上的后果。强迫每个
/// 标签都声明一条磨损通道，等于把「标签」偷换成「磨损标签」。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_tag(id: String, wear_channel_names: Vec<String>) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-tag 在没有活跃标签表的窗口内被调用".to_string());
            };
            do_register_tag(registry, table, &id, &wear_channel_names)
        })
    })
}

/// [`register_tag`] 的纯函数核心，方便单元测试不必绕过 `thread_local!`。
fn do_register_tag(
    registry: &mut Registry,
    table: &mut TagTable,
    id: &str,
    wear_channel_names: &[String],
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let mut wear = WearChannels::NONE;
    for name in wear_channel_names {
        let Some(channel) = WearChannels::from_name(name) else {
            return Err(format!(
                "未知的耐久磨损通道名称 {name:?}（只认 \"on-hit\" 与 \"on-use\"）"
            ));
        };
        wear = wear.union(channel);
    }

    table
        .define(index, TagDef { wear })
        .map(|()| true)
        .map_err(|err: TagError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 声明两条通道的标签注册成功且通道并起来() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TagTable::new();

        // Act
        let result = do_register_tag(
            &mut registry,
            &mut table,
            "lostland:shield",
            &["on-hit".to_string(), "on-use".to_string()],
        );

        // Assert：盾既挡刀又砸人——项目所有者原话「有的技能像是盾击,
        // 他也会变成武器这样」在注册期这一侧的落点。
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("lostland:shield").unwrap())
            .expect("刚注册的标签应能查到索引");
        let def = table.get(index).expect("刚注册的标签应能查到定义");
        assert!(def.wear.contains(WearChannels::ON_HIT));
        assert!(def.wear.contains(WearChannels::ON_USE));
    }

    #[test]
    fn 空通道列表的纯分类标签注册成功且不带任何通道() {
        // 反例，与上一条成对：空列表合法（标签不是为耐久发明的），
        // 见 `register_tag` 文档「为什么**允许**空列表」一节。
        // Arrange
        let mut registry = Registry::new();
        let mut table = TagTable::new();

        // Act
        let result = do_register_tag(&mut registry, &mut table, "lostland:flammable", &[]);

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("lostland:flammable").unwrap())
            .unwrap();
        assert_eq!(
            table.get(index).map(|def| def.wear),
            Some(WearChannels::NONE)
        );
    }

    #[test]
    fn 未知通道名称拒绝整次调用() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TagTable::new();

        // Act
        let result = do_register_tag(
            &mut registry,
            &mut table,
            "lostland:armor",
            &["on-block".to_string()],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 重复注册同一个标签返回错误() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = TagTable::new();
        do_register_tag(&mut registry, &mut table, "lostland:armor", &[]).expect("首次应成功");

        // Act
        let result = do_register_tag(&mut registry, &mut table, "lostland:armor", &[]);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 真的能从脚本源码调用() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_tag_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(TagTable::new());

        // Act
        let result =
            engine.load_source(r#"(register-tag "lostland:armor" (list "on-hit"))"#.to_string());

        // Assert
        assert!(result.is_ok(), "{result:?}");
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("lostland:armor").unwrap())
            .expect("脚本注册的标签应能查到索引");
        assert_eq!(
            table.get(index).map(|def| def.wear),
            Some(WearChannels::ON_HIT)
        );
    }
}
