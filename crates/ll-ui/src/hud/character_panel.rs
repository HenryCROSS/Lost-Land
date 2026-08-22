//! 角色面板：七项属性（六项主属性 + 幸运，烘焙 + 装备 + buff 之后的有效值，不是裸
//! `BaseStats`）+ 等级 + 经验 + 生效中的属性修正。
//!
//! # 七项属性走 `derive_stats`，不是裸 `Agent::stats`
//!
//! 任务书「数据从哪来」一节明确要求：显示的必须是
//! [`ll_sim::resolve::derive_stats`] 算出的有效值——裸 `BaseStats` 不
//! 反映装备加成与临时 buff，会让玩家看到「穿了护甲却没变化」的错觉。
//!
//! # 「生效中的属性修正」为什么不显示来源名字，只显示属性名 + 净增减
//!
//! `Agent::active_stat_modifiers` 的内层键是来源自身的 `ContentIndex`
//! （技能/未来的载具/天赋），但「这个 `ContentIndex` 属于哪张内容表、
//! 该查哪张表的 `display_name_key`」不是一个可以泛化处理的问题——技能、
//! 天赋、载具各有各的表，本模块要正确显示来源名字，得同时接好几张表
//! 的引用，而任务书要求的只是「生效中的属性修正」本身，不是「谁施加
//! 的」。这里选择只显示玩家最关心的信息——**哪项属性、现在净增减多少**
//! （同一属性可能有多个未过期来源，按 `derive_stats` 同样的规则求和），
//! 略去来源身份，把「解析任意来源的显示名」这个更大的功能留给需要它
//! 的后续批次。
//!
//! # 惰性到期判定与 `derive_stats` 保持同一条规则，但不复用它的返回值
//!
//! `derive_stats` 只返回「基础值 + 修正 + 装备」汇总之后的最终数字，
//! 不单独暴露「修正部分贡献了多少」这个中间量（[`ll_sim::resolve::DerivedStats`]
//! 的字段是私有的，公开访问器只有 [`ll_sim::resolve::DerivedStats::attribute`]/
//! [`ll_sim::resolve::DerivedStats::armor`]）。本模块因此自行按同一条
//! 过滤规则（`expires_at.0 > now.0`，见 `ll_sim::resolve::derive_stats`
//! 文档「惰性到期判定」一节）重新汇总一遍——三行代码的重复，换来不需要
//! `ll-sim` 公开一个专为 UI 服务的中间量访问器，属于 ADR 0021「只有算法
//! 真正可共享时才抽象」判断下更划算的一侧。

use std::collections::BTreeMap;

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_i18n::Catalog;
use ll_sim::item::ItemCatalog;
use ll_sim::resolve::derive_stats;
use ll_world::entity::{ActiveStatModifier, AttributeKind, BaseStats};
use ll_world::item::{EquipSlot, ItemStack};

use super::{PanelContent, build_panel};
use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 七项属性（六项主属性 + 幸运，幸运并入 `AttributeKind` 批次新增）
/// 按固定顺序展示——与 [`ll_world::entity::AttributeKind`] 声明顺序
/// 一致，力量在前、幸运在后。
const ATTRIBUTE_ORDER: [AttributeKind; 7] = [
    AttributeKind::Strength,
    AttributeKind::Dexterity,
    AttributeKind::Constitution,
    AttributeKind::Intelligence,
    AttributeKind::Willpower,
    AttributeKind::Charisma,
    AttributeKind::Luck,
];

/// 把七项属性变体映射到 Fluent 键——本项目当前没有为
/// `AttributeKind` 声明字符串名字的既有工具（`EquipSlot::from_name`
/// 只做「kebab 名 → 槽位」的反向解析，不是给 UI 用的展示名），这批键
/// 是本模块新增，见 `assets/locales/zh-CN.ftl` 的 `attribute-*`
/// 分组。
fn attribute_key(kind: AttributeKind) -> &'static str {
    match kind {
        AttributeKind::Strength => "lostland:attribute.strength.display_name",
        AttributeKind::Dexterity => "lostland:attribute.dexterity.display_name",
        AttributeKind::Constitution => "lostland:attribute.constitution.display_name",
        AttributeKind::Intelligence => "lostland:attribute.intelligence.display_name",
        AttributeKind::Willpower => "lostland:attribute.willpower.display_name",
        AttributeKind::Charisma => "lostland:attribute.charisma.display_name",
        AttributeKind::Luck => "lostland:attribute.luck.display_name",
    }
}

/// 角色面板需要的全部输入——一次读五个来源，理由与
/// [`super::status_bar::StatusBarData`] 一致：打包成一个结构体，未来
/// 增删字段不需要跟着改函数签名。
pub struct CharacterPanelData<'a> {
    /// 基础属性（未烘焙装备/buff 前的七项）——喂给 `derive_stats`。
    pub base_stats: BaseStats,
    /// 正在生效的临时属性修正——`Agent::active_stat_modifiers`。
    pub active_stat_modifiers:
        &'a BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>,
    /// 装备栏——`Agent::equipment`。
    pub equipment: &'a BTreeMap<EquipSlot, ItemStack>,
    /// 角色总等级——`Agent::level`。
    pub level: i32,
    /// 当前等级内已累积的经验值——`Agent::experience`。
    pub experience: i64,
    /// 升到下一级所需的经验总量——`Agent::xp_to_next_level`。
    pub xp_to_next_level: i64,
    /// 当前世界时刻——判定哪些修正已过期。
    pub now: Tick,
}

/// 按 [`CharacterPanelData::active_stat_modifiers`] 与 `now` 求出每项
/// 属性当前未过期修正的净增减——与 `derive_stats` 内部同一条过滤规则
/// （`expires_at.0 > now.0`），见模块文档「惰性到期判定」一节。恒为零
/// 的属性（没有任何未过期修正）不出现在返回列表里。
fn active_modifier_totals(
    modifiers: &BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>,
    now: Tick,
) -> Vec<(AttributeKind, i32)> {
    let mut totals = Vec::new();
    for kind in ATTRIBUTE_ORDER {
        let Some(per_source) = modifiers.get(&kind) else {
            continue;
        };
        let delta: i32 = per_source
            .values()
            .filter(|modifier| modifier.expires_at.0 > now.0)
            .map(|modifier| modifier.delta)
            .sum();
        if delta != 0 {
            totals.push((kind, delta));
        }
    }
    totals
}

/// 把角色面板的全部内容行写进 `cursor`/`lines`——标题、七项有效属性、
/// 等级、经验、生效中的属性修正（或「无」）。是 [`character_panel_lines`]
/// 与 [`character_panel`] 共用的真正实现：前者为了不打破既有测试签名
/// 自己新建一个 `RowCursor`，后者要接入 [`super::build_panel`] 现算
/// 面板高度、需要复用调用方传入的游标，两者因此拆成「产出独立 `Vec`」
/// 与「写进调用方给定的游标」两层，不是重复实现同一段逻辑两遍。
fn write_character_panel_lines(
    data: &CharacterPanelData<'_>,
    items: &dyn ItemCatalog,
    catalog: &Catalog,
    language: &str,
    cursor: &mut RowCursor,
    lines: &mut Vec<Label>,
) {
    cursor.push(
        lines,
        catalog.resolve(language, "hud-character-panel-title"),
    );

    let derived = derive_stats(
        data.base_stats,
        data.active_stat_modifiers,
        data.equipment,
        items,
        data.now,
    );
    for kind in ATTRIBUTE_ORDER {
        let label = catalog.resolve(language, attribute_key(kind));
        cursor.push(lines, format!("{label} {}", derived.attribute(kind)));
    }

    let level_label = catalog.resolve(language, "hud-character-level-label");
    cursor.push(lines, format!("{level_label} {}", data.level));

    let xp_label = catalog.resolve(language, "hud-character-experience-label");
    cursor.push(
        lines,
        format!("{xp_label} {}/{}", data.experience, data.xp_to_next_level),
    );

    cursor.push(
        lines,
        catalog.resolve(language, "hud-character-modifiers-title"),
    );
    let totals = active_modifier_totals(data.active_stat_modifiers, data.now);
    if totals.is_empty() {
        cursor.push(
            lines,
            format!(
                "  {}",
                catalog.resolve(language, "hud-character-modifiers-empty")
            ),
        );
    } else {
        for (kind, delta) in totals {
            let label = catalog.resolve(language, attribute_key(kind));
            let sign = if delta >= 0 { "+" } else { "" };
            cursor.push(lines, format!("  {label} {sign}{delta}"));
        }
    }
}

/// 产出角色面板的全部文本行：标题、七项有效属性、等级、经验、生效中
/// 的属性修正（或「无」）。纯函数，不接触 GPU——用
/// [`crate::widget::list::RowCursor`] 逐行推进，产出
/// [`crate::widget::label::Label`]，理由见 [`crate::widget`] 模块文档
/// 「不是通用控件库,是把这五样做对」一节。
pub fn character_panel_lines(
    data: &CharacterPanelData<'_>,
    items: &dyn ItemCatalog,
    catalog: &Catalog,
    language: &str,
    origin: (f32, f32),
    line_height: f32,
) -> Vec<Label> {
    let mut cursor = RowCursor::new(origin, line_height);
    let mut lines = Vec::new();
    write_character_panel_lines(data, items, catalog, language, &mut cursor, &mut lines);
    lines
}

/// 经验条的填充比例——`experience / xp_to_next_level`，见模块文档同一
/// 批次任务书「条形」一节：这是四块面板里唯一一个有真实分母、因此
/// 能诚实地做成条形的数值（生命/法力没有真实上限，见
/// `crate::widget::bar` 模块文档）。`xp_to_next_level <= 0`
/// 是不应该出现的数据（升级门槛恒为正），防御性地当成满条而非除零
/// panic。
pub fn experience_bar_fraction(data: &CharacterPanelData<'_>) -> f32 {
    if data.xp_to_next_level <= 0 {
        return 1.0;
    }
    data.experience as f32 / data.xp_to_next_level as f32
}

/// 建出角色面板：背景矩形 + 全部文本行——接入 [`super::build_panel`]
/// 现算面板高度。经验条本身（[`crate::widget::bar::bar_quads`]）由
/// [`super::render::render_hud`] 用 [`experience_bar_fraction`] 另外
/// 叠加在这块面板矩形之内，不在这里产出——[`PanelContent`] 目前只携带
/// 背景矩形 + 文本行两类内容，条形走独立的 quad 列表，见 `render_hud`
/// 的组装逻辑。
pub fn character_panel(
    data: &CharacterPanelData<'_>,
    items: &dyn ItemCatalog,
    catalog: &Catalog,
    language: &str,
    origin: (f32, f32),
    width: f32,
) -> PanelContent {
    build_panel(origin, width, |cursor, lines| {
        write_character_panel_lines(data, items, catalog, language, cursor, lines);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_sim::item::NoItems;
    use std::path::Path;

    fn write_fixture_catalog(dir: &Path) {
        std::fs::write(dir.join("zh-CN.ftl"), "hud-character-panel-title = 角色\nhud-character-level-label = 等级\nhud-character-experience-label = 经验\nhud-character-modifiers-title = 生效中的属性修正\nhud-character-modifiers-empty = 无\nattribute-strength-display_name = 力量\nattribute-dexterity-display_name = 敏捷\nattribute-constitution-display_name = 体质\nattribute-intelligence-display_name = 智力\nattribute-willpower-display_name = 意志\nattribute-charisma-display_name = 魅力\nattribute-luck-display_name = 幸运\n").expect("测试用写入应当成功");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ll-ui-hud-character-panel-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        dir
    }

    #[test]
    fn 角色面板文本包含七项有效属性数值() {
        // Arrange
        let dir = temp_dir("seven-attributes");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let data = CharacterPanelData {
            base_stats: BaseStats {
                strength: 12,
                dexterity: 10,
                constitution: 14,
                intelligence: 8,
                willpower: 9,
                charisma: 7,
                luck: 0,
            },
            active_stat_modifiers: &modifiers,
            equipment: &equipment,
            level: 3,
            experience: 40,
            xp_to_next_level: 200,
            now: Tick(0),
        };

        // Act
        let lines = character_panel_lines(&data, &NoItems, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("力量 12"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 角色面板文本包含幸运这一项() {
        // Arrange：幸运并入 AttributeKind 批次——角色面板的
        // ATTRIBUTE_ORDER 新增了 Luck，核实它真的渲染成一行，不是
        // 枚举变体加了、展示层却漏掉。
        let dir = temp_dir("luck-attribute-row");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let data = CharacterPanelData {
            base_stats: BaseStats {
                luck: 6,
                ..BaseStats::BASELINE
            },
            active_stat_modifiers: &modifiers,
            equipment: &equipment,
            level: 1,
            experience: 0,
            xp_to_next_level: 100,
            now: Tick(0),
        };

        // Act
        let lines = character_panel_lines(&data, &NoItems, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("幸运 6"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 角色面板文本包含等级与经验进度() {
        // Arrange
        let dir = temp_dir("level-xp");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let data = CharacterPanelData {
            base_stats: BaseStats::BASELINE,
            active_stat_modifiers: &modifiers,
            equipment: &equipment,
            level: 5,
            experience: 40,
            xp_to_next_level: 800,
            now: Tick(0),
        };

        // Act
        let lines = character_panel_lines(&data, &NoItems, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("40/800"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 未过期修正的属性名与净增减出现在生效中的修正一栏() {
        // Arrange：力量有一条未过期的 +50 修正。
        let dir = temp_dir("active-modifier");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let mut interner = Interner::new();
        let source = interner.intern(NamespacedId::parse("lostland:test_buff").unwrap());
        let mut modifiers = BTreeMap::new();
        modifiers.insert(
            AttributeKind::Strength,
            BTreeMap::from([(
                source,
                ActiveStatModifier {
                    delta: 50,
                    expires_at: Tick(100),
                },
            )]),
        );
        let equipment = BTreeMap::new();
        let data = CharacterPanelData {
            base_stats: BaseStats::BASELINE,
            active_stat_modifiers: &modifiers,
            equipment: &equipment,
            level: 1,
            experience: 0,
            xp_to_next_level: 100,
            now: Tick(10),
        };

        // Act
        let lines = character_panel_lines(&data, &NoItems, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("力量 +50"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 已过期修正不出现在生效中的修正一栏() {
        // Arrange：修正在 Tick(100) 过期，查询时刻已是 Tick(200)。
        let dir = temp_dir("expired-modifier");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let mut interner = Interner::new();
        let source = interner.intern(NamespacedId::parse("lostland:test_buff").unwrap());
        let mut modifiers = BTreeMap::new();
        modifiers.insert(
            AttributeKind::Strength,
            BTreeMap::from([(
                source,
                ActiveStatModifier {
                    delta: 50,
                    expires_at: Tick(100),
                },
            )]),
        );
        let equipment = BTreeMap::new();
        let data = CharacterPanelData {
            base_stats: BaseStats::BASELINE,
            active_stat_modifiers: &modifiers,
            equipment: &equipment,
            level: 1,
            experience: 0,
            xp_to_next_level: 100,
            now: Tick(200),
        };

        // Act
        let lines = character_panel_lines(&data, &NoItems, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(!joined.contains("+50"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 没有任何未过期修正时显示无() {
        // Arrange
        let dir = temp_dir("no-modifiers");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let data = CharacterPanelData {
            base_stats: BaseStats::BASELINE,
            active_stat_modifiers: &modifiers,
            equipment: &equipment,
            level: 1,
            experience: 0,
            xp_to_next_level: 100,
            now: Tick(0),
        };

        // Act
        let lines = character_panel_lines(&data, &NoItems, &catalog, "zh-CN", (0.0, 0.0), 16.0);
        let joined = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(joined.contains("无"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 经验条比例等于经验除以下一级门槛() {
        // Arrange
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let data = CharacterPanelData {
            base_stats: BaseStats::BASELINE,
            active_stat_modifiers: &modifiers,
            equipment: &equipment,
            level: 1,
            experience: 40,
            xp_to_next_level: 200,
            now: Tick(0),
        };

        // Act
        let fraction = experience_bar_fraction(&data);

        // Assert
        assert_eq!(fraction, 0.2);
    }

    #[test]
    fn 角色面板矩形宽度等于传入的宽度() {
        // Arrange
        let dir = temp_dir("panel-width");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let data = CharacterPanelData {
            base_stats: BaseStats::BASELINE,
            active_stat_modifiers: &modifiers,
            equipment: &equipment,
            level: 1,
            experience: 0,
            xp_to_next_level: 100,
            now: Tick(0),
        };

        // Act
        let panel = character_panel(&data, &NoItems, &catalog, "zh-CN", (0.0, 0.0), 260.0);

        // Assert
        assert_eq!(panel.rect.width, 260.0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
