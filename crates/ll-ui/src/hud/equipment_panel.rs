//! 装备栏：22 个引擎具名槽位各自当前装了什么。
//!
//! # 为什么遍历全部 22 个槽位，不是只列出 `Agent::equipment` 里已有
//! 的键
//!
//! `Agent::equipment`（[`ll_world::entity::Agent::equipment`] 文档
//! 「为什么以锚点槽位为键」一节）只存**锚点**槽位——双手武器占用
//! `MAIN_HAND`+`OFF_HAND` 两个物理槽位，但只在 `MAIN_HAND` 这一个键下
//! 存一份。若本模块只遍历 `equipment` 的键,双手武器占用的 `OFF_HAND`
//! 会被错误地显示成「空」,而实际上那个槽位确实被占着（只是不能再单独
//! 装一件副手武器）。任务书要求「各槽位当前装了什么」——这里选择按
//! 生物解剖结构的全部 22 个槽位逐一判断「这个槽位有没有被某件已装备
//! 物品的 `equip_mask` 覆盖」，而不是简单地把 `equipment` 的键当成
//! 「已占用槽位」的全集，见 [`occupant_of`] 的实现。
//!
//! # 空槽位与「查不到物品定义」是两种不同的空
//!
//! 空槽位显示 `hud-equipment-empty-slot`；槽位被占用但对应的
//! `ItemStack::def` 查不到 `ItemView`（数据不一致,理论上不应该发生）
//! 显示退化索引——两条路径复用
//! [`super::item_display_name`]/`hud-equipment-empty-slot`，不混为
//! 一谈。

use std::collections::BTreeMap;

use ll_i18n::Catalog;
use ll_mod::item::ItemTable;
use ll_world::item::{EquipSlot, ItemStack};

use super::{PanelContent, build_panel, item_display_name};
use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 全部 22 个引擎具名槽位，配上各自的展示名 Fluent 键——顺序与
/// `ll_world::item::EquipSlot` 的常量声明顺序一致（主手在前，右戒指
/// 在后），与 `assets/locales/zh-CN.ftl` 的 `equip_slot-*` 分组一一
/// 对应。
const SLOTS: [(EquipSlot, &str); 22] = [
    (
        EquipSlot::MAIN_HAND,
        "lostland:equip_slot.main_hand.display_name",
    ),
    (
        EquipSlot::OFF_HAND,
        "lostland:equip_slot.off_hand.display_name",
    ),
    (EquipSlot::HEAD, "lostland:equip_slot.head.display_name"),
    (EquipSlot::FACE, "lostland:equip_slot.face.display_name"),
    (EquipSlot::EYES, "lostland:equip_slot.eyes.display_name"),
    (EquipSlot::NECK, "lostland:equip_slot.neck.display_name"),
    (EquipSlot::BODY, "lostland:equip_slot.body.display_name"),
    (EquipSlot::OUTER, "lostland:equip_slot.outer.display_name"),
    (EquipSlot::BACK, "lostland:equip_slot.back.display_name"),
    (
        EquipSlot::SHOULDER_L,
        "lostland:equip_slot.shoulder_l.display_name",
    ),
    (
        EquipSlot::SHOULDER_R,
        "lostland:equip_slot.shoulder_r.display_name",
    ),
    (EquipSlot::ARM_L, "lostland:equip_slot.arm_l.display_name"),
    (EquipSlot::ARM_R, "lostland:equip_slot.arm_r.display_name"),
    (EquipSlot::HAND_L, "lostland:equip_slot.hand_l.display_name"),
    (EquipSlot::HAND_R, "lostland:equip_slot.hand_r.display_name"),
    (EquipSlot::BELT, "lostland:equip_slot.belt.display_name"),
    (EquipSlot::TASSET, "lostland:equip_slot.tasset.display_name"),
    (EquipSlot::LEGS, "lostland:equip_slot.legs.display_name"),
    (EquipSlot::BOOT_L, "lostland:equip_slot.boot_l.display_name"),
    (EquipSlot::BOOT_R, "lostland:equip_slot.boot_r.display_name"),
    (EquipSlot::RING_L, "lostland:equip_slot.ring_l.display_name"),
    (EquipSlot::RING_R, "lostland:equip_slot.ring_r.display_name"),
];

/// 查 `slot` 是否被 `equipment` 里的某一件已装备堆覆盖——遍历全部
/// 已装备条目，查它们各自的 `equip_mask` 是否与 `slot` 相交（见模块
/// 文档「为什么遍历全部 22 个槽位」一节）。命中时返回那一堆物品的
/// `ItemStack`；`items` 查不到某件已装备物品的定义时，视为不覆盖任何
/// 槽位（一件连自己占用哪些槽位都查不到定义的物品，不该被当成任何
/// 槽位的占用者）。
fn occupant_of<'a>(
    slot: EquipSlot,
    equipment: &'a BTreeMap<EquipSlot, ItemStack>,
    items: &ItemTable,
) -> Option<&'a ItemStack> {
    equipment.values().find(|stack| {
        items
            .get(stack.def)
            .is_some_and(|view| view.equip_mask.intersects(slot.mask()))
    })
}

/// 把装备栏面板的全部内容行写进 `cursor`/`lines`——标题 + 22 个槽位
/// 各自的占用情况。拆分理由同
/// `crate::hud::character_panel::write_character_panel_lines` 文档。
fn write_equipment_panel_lines(
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
    cursor: &mut RowCursor,
    lines: &mut Vec<Label>,
) {
    cursor.push(
        lines,
        catalog.resolve(language, "hud-equipment-panel-title"),
    );

    for (slot, key) in SLOTS {
        let slot_label = catalog.resolve(language, key);
        let occupant_text = match occupant_of(slot, equipment, items) {
            Some(stack) => item_display_name(stack.def, items, catalog, language),
            None => catalog.resolve(language, "hud-equipment-empty-slot"),
        };
        cursor.push(lines, format!("{slot_label}: {occupant_text}"));
    }
}

/// 产出装备栏面板的全部文本行。纯函数，不接触 GPU。
pub fn equipment_panel_lines(
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
    origin: (f32, f32),
    line_height: f32,
) -> Vec<Label> {
    let mut cursor = RowCursor::new(origin, line_height);
    let mut lines = Vec::new();
    write_equipment_panel_lines(equipment, items, catalog, language, &mut cursor, &mut lines);
    lines
}

/// 建出装备栏面板：背景矩形 + 全部文本行,接入 [`super::build_panel`]
/// 现算面板高度。
pub fn equipment_panel(
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    items: &ItemTable,
    catalog: &Catalog,
    language: &str,
    origin: (f32, f32),
    width: f32,
) -> PanelContent {
    build_panel(origin, width, |cursor, lines| {
        write_equipment_panel_lines(equipment, items, catalog, language, cursor, lines);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{ContentIndex, Interner, NamespacedId};
    use ll_core::scaled::Milli;
    use ll_mod::item::ItemAttrs;
    use ll_sim::combat::Penetration;
    use ll_sim::item::SlotMask;
    use std::path::Path;

    fn write_fixture_catalog(dir: &Path) {
        // 末尾额外带两条测试用物品译名（`item-iron_sword`/`item-great_axe`）
        // ——与其在个别测试里读出旧内容再拼接新内容重新写一次（那样会
        // 让含中文字面量的行单独落在一行、脱离本行的 `.expect(` 豁免标记，
        // 见 `check_i18n_strings.py` 模块文档「怎么判定豁免」一节），
        // 不如从一开始就把全部测试可能用到的键放进同一次单行写入。
        std::fs::write(dir.join("zh-CN.ftl"), "hud-equipment-panel-title = 装备\nhud-equipment-empty-slot = （空）\nequip_slot-main_hand-display_name = 主手\nequip_slot-off_hand-display_name = 副手\nequip_slot-head-display_name = 头部\nequip_slot-face-display_name = 面部\nequip_slot-eyes-display_name = 眼部\nequip_slot-neck-display_name = 颈部\nequip_slot-body-display_name = 躯干\nequip_slot-outer-display_name = 外袍\nequip_slot-back-display_name = 背部\nequip_slot-shoulder_l-display_name = 左肩\nequip_slot-shoulder_r-display_name = 右肩\nequip_slot-arm_l-display_name = 左臂\nequip_slot-arm_r-display_name = 右臂\nequip_slot-hand_l-display_name = 左手\nequip_slot-hand_r-display_name = 右手\nequip_slot-belt-display_name = 腰带\nequip_slot-tasset-display_name = 腿甲\nequip_slot-legs-display_name = 双腿\nequip_slot-boot_l-display_name = 左靴\nequip_slot-boot_r-display_name = 右靴\nequip_slot-ring_l-display_name = 左戒指\nequip_slot-ring_r-display_name = 右戒指\nitem-iron_sword = 铁剑\nitem-great_axe = 巨斧\n").expect("测试用写入应当成功");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ll-ui-hud-equipment-panel-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        dir
    }

    fn define_item(
        table: &mut ItemTable,
        index: ContentIndex,
        display_name_key: &str,
        equip_mask: SlotMask,
    ) {
        table
            .define(
                index,
                ItemAttrs {
                    display_name_key: NamespacedId::parse(display_name_key).unwrap(),
                    stack_limit: 1,
                    base_weight: Milli::ZERO,
                    base_price: Milli::ZERO,
                    max_durability: Some(100),
                    equip_mask,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                },
            )
            .expect("测试用注册应当成功");
    }

    #[test]
    fn 装备栏产出的行数恒为标题加二十二个槽位() {
        // Arrange
        let dir = temp_dir("line-count");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let table = ItemTable::new();
        let equipment = BTreeMap::new();

        // Act
        let lines = equipment_panel_lines(&equipment, &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);

        // Assert
        assert_eq!(lines.len(), 23);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 未装备的槽位显示空占位() {
        // Arrange
        let dir = temp_dir("empty-slot");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let table = ItemTable::new();
        let equipment = BTreeMap::new();

        // Act
        let lines = equipment_panel_lines(&equipment, &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("主手: （空）"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 单手装备的槽位显示物品名字() {
        // Arrange
        let dir = temp_dir("single-slot-item");
        write_fixture_catalog(&dir);
        let mut interner = Interner::new();
        let sword = interner.intern(NamespacedId::parse("lostland:iron_sword").unwrap());
        let mut table = ItemTable::new();
        define_item(
            &mut table,
            sword,
            "lostland:item.iron_sword",
            EquipSlot::MAIN_HAND.mask(),
        );
        let mut equipment = BTreeMap::new();
        equipment.insert(
            EquipSlot::MAIN_HAND,
            ItemStack::with_durability(sword, 1, 100),
        );
        let catalog = Catalog::load_dir(&dir);

        // Act
        let lines = equipment_panel_lines(&equipment, &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("主手: 铁剑"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 双手武器占用的副手槽位也显示为已占用而非空() {
        // Arrange：双手武器的 equip_mask 同时覆盖 MAIN_HAND 与
        // OFF_HAND,但 Agent::equipment 只在 MAIN_HAND 这一个键下存
        // 一份——见模块文档「为什么遍历全部 22 个槽位」一节,这条测试
        // 正是验证该节论证的行为。
        let dir = temp_dir("two-handed-off-hand");
        write_fixture_catalog(&dir);
        let mut interner = Interner::new();
        let great_axe = interner.intern(NamespacedId::parse("lostland:great_axe").unwrap());
        let mut table = ItemTable::new();
        let two_handed_mask = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());
        define_item(
            &mut table,
            great_axe,
            "lostland:item.great_axe",
            two_handed_mask,
        );
        let mut equipment = BTreeMap::new();
        equipment.insert(
            EquipSlot::MAIN_HAND,
            ItemStack::with_durability(great_axe, 1, 100),
        );
        let catalog = Catalog::load_dir(&dir);

        // Act
        let lines = equipment_panel_lines(&equipment, &table, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("副手: 巨斧"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 装备栏面板矩形宽度等于传入的宽度() {
        // Arrange
        let dir = temp_dir("panel-width");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let table = ItemTable::new();
        let equipment = BTreeMap::new();

        // Act
        let panel = equipment_panel(&equipment, &table, &catalog, "zh-CN", (0.0, 0.0), 240.0);

        // Assert
        assert_eq!(panel.rect.width, 240.0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
