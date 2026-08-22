//! 背包面板：物品列表 + 数量 + 耐久。
//!
//! # 物品名字从哪查
//!
//! `ItemStack::def` 只是一个 [`ll_core::ident::ContentIndex`]，本身不
//! 携带任何可显示的文字——真正的名字要经 [`ll_mod::item::ItemTable::get`]
//! 查出 [`ll_mod::item::ItemView::display_name_key`]，再交给
//! [`ll_i18n::Catalog`] 解析成当前语言的文本。这条链路
//! （`ContentIndex` → `ItemView` → `display_name_key` → `Catalog::resolve`）
//! 是 `display_name_key` 系列字段的第一个大规模消费者——此前只有窗口
//! 标题（`WindowConfig::title_key`）一个真实消费者，见任务书「三、所有
//! 文本必须走 i18n」一节。
//!
//! # 查不到物品定义时怎么办
//!
//! `ItemTable::get` 未注册的索引返回 `None`（ADR 0015）——理论上不应该
//! 发生（背包里的物品只能来自 `resolve_pick_up`/`register-item` 一类
//! 已经过注册校验的路径），但显示层不能假设世界状态永远一致，查不到
//! 时退化显示原始索引数字而不是 panic 或悄悄跳过整个条目——悄悄跳过
//! 会让玩家的背包「少东西」而看不出原因，显示一个能定位问题的原始
//! 索引号至少能让人发现「这里有条目对不上号」。

use ll_i18n::Catalog;
use ll_mod::item::ItemTable;
use ll_world::item::ItemStack;

use super::{PanelContent, build_panel, item_display_name};
use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 单件堆叠的展示文本：`名字 x数量`，若有耐久上限再追加
/// `（耐久 当前）`——`ItemStack::durability` 是 `Option`，`None` 的物品
/// （材料、消耗品）不显示耐久这一段,`Some` 的物品（武器、装备）才显示,
/// 与 [`ll_world::item::ItemStack::durability`] 字段文档「`None`/`Some`
/// 对应」的既有区分完全一致——本函数不额外发明新的区分维度。
fn stack_line_text(
    stack: &ItemStack,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
) -> String {
    let name = item_display_name(stack.def, items, catalog, language);
    let mut text = format!("{name} x{}", stack.count);
    if let Some(durability) = stack.durability {
        let label = catalog.resolve(language, "hud-inventory-durability-label");
        text.push_str(&format!("（{label} {durability}）"));
    }
    text
}

/// 把背包面板的全部内容行写进 `cursor`/`lines`——标题 + 逐条堆叠，
/// 背包为空时显示占位行。[`inventory_panel_lines`]/[`inventory_panel`]
/// 共用的真正实现，拆分理由同
/// `crate::hud::character_panel::write_character_panel_lines` 文档。
fn write_inventory_panel_lines(
    inventory: &[ItemStack],
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
    cursor: &mut RowCursor,
    lines: &mut Vec<Label>,
) {
    cursor.push(
        lines,
        catalog.resolve(language, "hud-inventory-panel-title"),
    );

    if inventory.is_empty() {
        cursor.push(
            lines,
            format!("  {}", catalog.resolve(language, "hud-inventory-empty")),
        );
    } else {
        for stack in inventory {
            cursor.push(
                lines,
                format!("  {}", stack_line_text(stack, items, catalog, language)),
            );
        }
    }
}

/// 产出背包面板的全部文本行：标题 + 逐条堆叠，背包为空时显示占位行。
/// 纯函数，不接触 GPU。
pub fn inventory_panel_lines(
    inventory: &[ItemStack],
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
    origin: (f32, f32),
    line_height: f32,
) -> Vec<Label> {
    let mut cursor = RowCursor::new(origin, line_height);
    let mut lines = Vec::new();
    write_inventory_panel_lines(inventory, items, catalog, language, &mut cursor, &mut lines);
    lines
}

/// 建出背包面板：背景矩形 + 全部文本行,接入 [`super::build_panel`]
/// 现算面板高度。
pub fn inventory_panel(
    inventory: &[ItemStack],
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
    origin: (f32, f32),
    width: f32,
) -> PanelContent {
    build_panel(origin, width, |cursor, lines| {
        write_inventory_panel_lines(inventory, items, catalog, language, cursor, lines);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_core::scaled::Milli;
    use ll_mod::item::ItemAttrs;
    use ll_sim::combat::Penetration;
    use ll_sim::item::SlotMask;
    use std::path::Path;

    fn write_fixture_catalog(dir: &Path) {
        // "lostland:item.arrow" 经 `ll_i18n::to_fluent_id` 剥离命名空间、
        // 点号换连字符后是 "item-arrow"——与 `arrow_item_table` 里
        // `display_name_key` 的取值一一对应。
        std::fs::write(dir.join("zh-CN.ftl"), "hud-inventory-panel-title = 背包\nhud-inventory-empty = （空）\nhud-inventory-durability-label = 耐久\nitem-arrow = 箭矢\n").expect("测试用写入应当成功");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ll-ui-hud-inventory-panel-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        dir
    }

    fn arrow_item_table() -> (ItemTable, ll_core::ident::ContentIndex) {
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:arrow").unwrap());
        let mut table = ItemTable::new();
        table
            .define(
                index,
                ItemAttrs {
                    display_name_key: NamespacedId::parse("lostland:item.arrow").unwrap(),
                    stack_limit: 99,
                    base_weight: Milli::ZERO,
                    base_price: Milli::ZERO,
                    max_durability: None,
                    equip_mask: SlotMask::EMPTY,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                },
            )
            .expect("测试用注册应当成功");
        (table, index)
    }

    #[test]
    fn 背包为空时显示空占位行() {
        // Arrange
        let dir = temp_dir("empty");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let (table, _) = arrow_item_table();

        // Act
        let lines = inventory_panel_lines(&[], &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("（空）"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 背包中的物品显示本地化名字与数量() {
        // Arrange：3 支箭。
        let dir = temp_dir("name-and-count");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let (table, index) = arrow_item_table();
        let inventory = vec![ItemStack::new(index, 3)];

        // Act
        let lines = inventory_panel_lines(&inventory, &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("箭矢 x3"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 带耐久的物品显示当前耐久值() {
        // Arrange
        let dir = temp_dir("durability");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let (table, index) = arrow_item_table();
        let inventory = vec![ItemStack::with_durability(index, 1, 37)];

        // Act
        let lines = inventory_panel_lines(&inventory, &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("耐久 37"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 不带耐久上限的物品不显示耐久段() {
        // Arrange：`ItemStack::new` 恒把 durability 设成 None。
        let dir = temp_dir("no-durability");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let (table, index) = arrow_item_table();
        let inventory = vec![ItemStack::new(index, 1)];

        // Act
        let lines = inventory_panel_lines(&inventory, &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(!joined.contains("耐久"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 查不到定义的物品退化显示原始索引() {
        // Arrange：内容索引来自一个从未在 table 里 define 过的标识符。
        let dir = temp_dir("unknown-item");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let mut interner = Interner::new();
        let unknown = interner.intern(NamespacedId::parse("lostland:ghost_item").unwrap());
        let table = ItemTable::new();
        let inventory = vec![ItemStack::new(unknown, 1)];

        // Act
        let lines = inventory_panel_lines(&inventory, &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains(&format!("#{}", unknown.get())));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 背包面板矩形宽度等于传入的宽度() {
        // Arrange
        let dir = temp_dir("panel-width");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let (table, _) = arrow_item_table();

        // Act
        let panel = inventory_panel(&[], &table, &catalog, "zh-CN", (0.0, 0.0), 220.0);

        // Assert
        assert_eq!(panel.rect.width, 220.0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
