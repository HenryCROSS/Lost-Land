//! HUD 的最外层薄封装：把四块面板的布局、面板背景、经验条、文本行
//! 全部接起来，一次性提交到 GPU。
//!
//! # 布局：状态栏顶部通栏，其余三块并排在下方
//!
//! ```text
//! +----------------------------------------------------------+
//! | 状态栏（时间 / 生命 / 法力，常驻，见模块文档）              |
//! +----------------+----------------+--------------------------+
//! | 角色面板        | 背包面板        | 装备栏面板                |
//! | 六项属性         | 物品列表        | 22 个槽位                 |
//! | 等级/经验(条形)  |                |                          |
//! | 生效中的修正      |                |                          |
//! +----------------+----------------+--------------------------+
//! ```
//!
//! 三列宽度固定（[`CHARACTER_WIDTH`]/[`INVENTORY_WIDTH`]/
//! [`EQUIPMENT_WIDTH`]），高度各自按内容行数现算（见
//! [`super::build_panel`]）——三列不强制等高，允许装备栏（22 行）比
//! 角色面板（视生效修正条数而定，通常个位数行）长得多，这是「布局只做
//! 定位与堆叠，不做约束求解器」的直接后果（见
//! [`crate::widget::geometry`] 模块文档），不是遗漏。

use ll_i18n::Catalog;
use ll_mod::item::ItemTable;
use ll_render::wgpu;
use ll_sim::item::ItemCatalog;
use ll_text::TextRenderer;
use ll_world::item::{EquipSlot, ItemStack};

use super::character_panel::{self, CharacterPanelData};
use super::equipment_panel;
use super::inventory_panel;
use super::status_bar::{self, StatusBarData};
use crate::widget::anim::{DEFAULT_ANIM_DURATION_FRAMES, FrameTick};
use crate::widget::bar::{bar_quads, textured_bar_quads, textured_two_layer_bar_quads};
use crate::widget::geometry::Rect;
use crate::widget::label::Label;
use crate::widget::panel::{panel_quads, textured_panel_quads};
use crate::widget::quad::{QuadInstance, QuadRenderer};
use crate::widget::skin::{BarStyleId, PanelStyleId, Skin};
use crate::widget::state::{WidgetStateTable, animate_experience_bar};
use crate::widget::textured_quad::{TexturedQuadInstance, TexturedQuadRenderer};
use glyphon::Color;
use std::collections::BTreeMap;

/// 面板左上角与窗口边缘的留白（像素）。
const SCREEN_MARGIN: f32 = 16.0;
/// 三列之间、状态栏与三列之间的间隔（像素）。
const PANEL_GAP: f32 = 10.0;
/// 状态栏通栏宽度。
const STATUS_WIDTH: f32 = 620.0;
/// 角色面板宽度。
const CHARACTER_WIDTH: f32 = 260.0;
/// 背包面板宽度。
const INVENTORY_WIDTH: f32 = 220.0;
/// 装备栏面板宽度。
const EQUIPMENT_WIDTH: f32 = 220.0;
/// 经验条高度（像素）。
const EXPERIENCE_BAR_HEIGHT: f32 = 6.0;
/// 生命/法力条高度（像素）。
const RESOURCE_BAR_HEIGHT: f32 = 10.0;
/// 生命/法力条宽度（像素）——两条并排放在状态栏正下方。
const RESOURCE_BAR_WIDTH: f32 = 300.0;
/// 双层血条余晖层的过渡时长——比默认时长（立即层用）长三倍，制造
/// 「追赶」的滞后感,见 `crate::widget::bar::FlatTwoLayerBarAppearance`
/// 模块文档。
const AFTERGLOW_DURATION_FRAMES: u32 = DEFAULT_ANIM_DURATION_FRAMES * 3;
/// 文本颜色——四块面板统一用同一个颜色,不按状态区分,理由见
/// [`crate::widget::label`] 模块文档：HUD 面板没有「已加载/警告/失败」
/// 这类需要颜色分组的语义,与 [`crate::load_report_view::LoadReportLine`]
/// 的多色分组是两种不同的场景。
const TEXT_COLOR: Color = Color::rgba(235, 235, 235, 255);

/// 四块面板 + 三条条形这一帧全部需要提交给 GPU 的内容：见
/// [`render_hud`] 文档「三道 pass」一节。[`build_hud_frame`] 内部先把
/// 它们**算**出来（不接触 GPU，可脱离窗口单元测试，见本模块测试）。
pub struct HudFrame {
    /// 皮肤给出纯色回退时的全部填色矩形。
    pub quads: Vec<QuadInstance>,
    /// 皮肤给出真实贴图外观时的全部贴图矩形——见
    /// [`crate::widget::skin`] 模块文档「`Skin` trait 的两层方法」
    /// 一节：同一块面板/同一条条形，只会落进 `quads` 或
    /// `textured_quads` 其中一个，不会同时出现在两边。
    pub textured_quads: Vec<TexturedQuadInstance>,
    /// 四块面板的全部文本行。
    pub labels: Vec<Label>,
}

/// 现算这一帧 HUD 需要的全部填色矩形/贴图矩形与文本行——纯函数,不
/// 接触 GPU,是 [`render_hud`] 与本模块测试共用的核心逻辑。
///
/// `anim`/`now` 只驱动条形的**视觉宽度**（[`AnimatedValue`] 的显示值）
/// ——面板/文本内容与真实世界状态之间没有经过这张表,数字瞬时,见
/// `crate::widget::anim` 模块文档「数字瞬时,条形动画」一节。
///
/// [`AnimatedValue`]: crate::widget::anim::AnimatedValue
#[allow(clippy::too_many_arguments)]
pub fn build_hud_frame(
    status: &StatusBarData,
    character: &CharacterPanelData<'_>,
    inventory: &[ItemStack],
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    items: &dyn ItemCatalog,
    item_table: &ItemTable,
    catalog: &Catalog,
    language: &str,
    skin: &dyn Skin,
    anim: &mut WidgetStateTable,
    now: FrameTick,
) -> HudFrame {
    let mut quads = Vec::new();
    let mut textured_quads = Vec::new();
    let mut labels = Vec::new();

    let status_origin = (SCREEN_MARGIN, SCREEN_MARGIN);
    let status_panel =
        status_bar::status_bar_panel(status, catalog, language, status_origin, STATUS_WIDTH);
    push_panel(
        &mut quads,
        &mut textured_quads,
        &mut labels,
        &status_panel.rect,
        status_panel.labels,
        skin,
    );

    // 生命/法力双层条：紧贴在状态栏下方,并排放置——立即层瞬间反映
    // 真实值,余晖层用更长的时长追赶,见
    // `crate::widget::bar::FlatTwoLayerBarAppearance` 模块文档。数字
    // 本身（状态栏文本）从不经过 `anim`,见本函数文档。
    let health_rect = Rect::new(
        status_panel.rect.x,
        status_panel.rect.bottom() + PANEL_GAP,
        RESOURCE_BAR_WIDTH,
        RESOURCE_BAR_HEIGHT,
    );
    let health_real = status_bar::health_bar_fraction(status.health);
    let health_immediate = anim.animate("hud.health_immediate", health_real, now);
    let health_lagging = anim.animate_with_duration(
        "hud.health_afterglow",
        health_real,
        now,
        AFTERGLOW_DURATION_FRAMES,
    );
    push_two_layer_bar(
        &mut quads,
        &mut textured_quads,
        health_rect,
        health_immediate,
        health_lagging,
        skin,
    );

    let mana_rect = health_rect.stack_right(PANEL_GAP, RESOURCE_BAR_WIDTH);
    let mana_real = status_bar::mana_bar_fraction(status.mana);
    let mana_immediate = anim.animate("hud.mana_immediate", mana_real, now);
    let mana_lagging = anim.animate_with_duration(
        "hud.mana_afterglow",
        mana_real,
        now,
        AFTERGLOW_DURATION_FRAMES,
    );
    push_two_layer_bar(
        &mut quads,
        &mut textured_quads,
        mana_rect,
        mana_immediate,
        mana_lagging,
        skin,
    );

    let row_origin = (SCREEN_MARGIN, health_rect.bottom() + PANEL_GAP);

    let character_panel_content = character_panel::character_panel(
        character,
        items,
        catalog,
        language,
        row_origin,
        CHARACTER_WIDTH,
    );
    push_panel(
        &mut quads,
        &mut textured_quads,
        &mut labels,
        &character_panel_content.rect,
        character_panel_content.labels,
        skin,
    );

    // 经验条：紧贴在角色面板下方,是唯一有真实分母（见
    // `character_panel::experience_bar_fraction` 文档）、因此能诚实做成
    // 条形的数值；升级时的「填满→清零→继续填」由
    // `animate_experience_bar` 负责,见其文档。
    let bar_rect = character_panel_content
        .rect
        .stack_below(4.0, EXPERIENCE_BAR_HEIGHT);
    let real_fraction = character_panel::experience_bar_fraction(character);
    let displayed_fraction =
        animate_experience_bar(anim, "hud.xp_bar", character.level, real_fraction, now);
    push_bar(
        &mut quads,
        &mut textured_quads,
        bar_rect,
        displayed_fraction,
        skin,
    );

    let inventory_origin = character_panel_content
        .rect
        .stack_right(PANEL_GAP, INVENTORY_WIDTH)
        .origin();
    let inventory_panel_content = inventory_panel::inventory_panel(
        inventory,
        item_table,
        catalog,
        language,
        inventory_origin,
        INVENTORY_WIDTH,
    );
    push_panel(
        &mut quads,
        &mut textured_quads,
        &mut labels,
        &inventory_panel_content.rect,
        inventory_panel_content.labels,
        skin,
    );

    let equipment_origin = inventory_panel_content
        .rect
        .stack_right(PANEL_GAP, EQUIPMENT_WIDTH)
        .origin();
    let equipment_panel_content = equipment_panel::equipment_panel(
        equipment,
        item_table,
        catalog,
        language,
        equipment_origin,
        EQUIPMENT_WIDTH,
    );
    push_panel(
        &mut quads,
        &mut textured_quads,
        &mut labels,
        &equipment_panel_content.rect,
        equipment_panel_content.labels,
        skin,
    );

    HudFrame {
        quads,
        textured_quads,
        labels,
    }
}

/// 推入一块面板的背景——皮肤给出真实贴图外观
/// （[`Skin::textured_panel`]）就走贴图路径,否则回退到
/// [`Skin::panel`] 的纯色路径,两条路径互斥（见 [`HudFrame::textured_quads`]
/// 文档）。
fn push_panel(
    quads: &mut Vec<QuadInstance>,
    textured_quads: &mut Vec<TexturedQuadInstance>,
    labels: &mut Vec<Label>,
    rect: &Rect,
    panel_labels: Vec<Label>,
    skin: &dyn Skin,
) {
    match skin.textured_panel(PanelStyleId::Window) {
        Some(appearance) => textured_quads.extend(textured_panel_quads(*rect, &appearance)),
        None => quads.extend(panel_quads(*rect, &skin.panel(PanelStyleId::Window))),
    }
    labels.extend(panel_labels);
}

/// 推入一条单层条形（经验条），分支逻辑同 [`push_panel`]。
fn push_bar(
    quads: &mut Vec<QuadInstance>,
    textured_quads: &mut Vec<TexturedQuadInstance>,
    rect: Rect,
    fraction: f32,
    skin: &dyn Skin,
) {
    match skin.textured_bar(BarStyleId::Progress) {
        Some(appearance) => textured_quads.extend(textured_bar_quads(rect, fraction, &appearance)),
        None => quads.extend(bar_quads(rect, fraction, &skin.bar(BarStyleId::Progress))),
    }
}

/// 推入一条双层条形（生命/法力），分支逻辑同 [`push_panel`]。
fn push_two_layer_bar(
    quads: &mut Vec<QuadInstance>,
    textured_quads: &mut Vec<TexturedQuadInstance>,
    rect: Rect,
    immediate_fraction: f32,
    lagging_fraction: f32,
    skin: &dyn Skin,
) {
    match skin.textured_two_layer_bar(BarStyleId::HealthMana) {
        Some(appearance) => textured_quads.extend(textured_two_layer_bar_quads(
            rect,
            immediate_fraction,
            lagging_fraction,
            &appearance,
        )),
        None => {
            // 纯色回退没有专门的"双层"外观数据（`FlatBarAppearance`
            // 只有背景/前景两色）,复用同一份外观：背景当轨道,前景色
            // 同时充当立即层与余晖层——纯色场景下没有真实贴图可供
            // 区分两层的明暗,这是可接受的简化,不影响真实贴图路径
            // （`NineSliceSkin` 已经用 `afterglow_tint` 正确区分）。
            let appearance = skin.bar(BarStyleId::HealthMana);
            quads.extend(crate::widget::bar::two_layer_bar_quads(
                rect,
                immediate_fraction,
                lagging_fraction,
                &crate::widget::bar::FlatTwoLayerBarAppearance {
                    background_color: appearance.background_color,
                    afterglow_color: appearance.fill_color,
                    fill_color: appearance.fill_color,
                },
            ));
        }
    }
}

/// 把 [`build_hud_frame`] 算出的内容真正提交到屏幕：先画纯色面板背景
/// （[`QuadRenderer`]），再画真实贴图面板背景/条形
/// （[`TexturedQuadRenderer`]），最后画全部文本（[`TextRenderer`]）——
/// 三道 pass 都用 `LoadOp::Load`，不清屏，叠加在调用方已经画好的世界层
/// 之上（见 [`crate::widget::quad`] 模块文档「为什么不复用 SpriteBatch」
/// 一节）。同一块面板/条形只会落进前两道 pass 中的一个（见
/// [`HudFrame::textured_quads`] 文档），两道 pass 因此互不遮挡对方的
/// 内容，先后顺序不影响最终画面。
#[allow(clippy::too_many_arguments)]
pub fn render_hud(
    quad_renderer: &mut QuadRenderer,
    textured_quad_renderer: &mut TexturedQuadRenderer,
    text_renderer: &mut TextRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::TextureView,
    resolution_width: u32,
    resolution_height: u32,
    status: &StatusBarData,
    character: &CharacterPanelData<'_>,
    inventory: &[ItemStack],
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    items: &dyn ItemCatalog,
    item_table: &ItemTable,
    catalog: &Catalog,
    language: &str,
    skin: &dyn Skin,
    anim: &mut WidgetStateTable,
    now: FrameTick,
) {
    let frame = build_hud_frame(
        status, character, inventory, equipment, items, item_table, catalog, language, skin, anim,
        now,
    );

    quad_renderer.render(
        device,
        queue,
        target,
        resolution_width,
        resolution_height,
        &frame.quads,
    );
    textured_quad_renderer.render(
        device,
        queue,
        target,
        resolution_width,
        resolution_height,
        &frame.textured_quads,
    );

    let runs: Vec<_> = frame
        .labels
        .iter()
        .map(|label| {
            label.to_text_run(
                super::DEFAULT_FONT_SIZE,
                super::DEFAULT_LINE_HEIGHT,
                400.0,
                TEXT_COLOR,
            )
        })
        .collect();
    if let Err(error) = text_renderer.render(
        device,
        queue,
        target,
        resolution_width,
        resolution_height,
        &runs,
    ) {
        tracing::error!(%error, "HUD 文本渲染失败");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::skin::FlatColorSkin;
    use ll_core::time::Tick;
    use ll_sim::item::NoItems;
    use ll_world::entity::BaseStats;
    use std::path::Path;

    fn write_fixture_catalog(dir: &Path) {
        std::fs::write(dir.join("zh-CN.ftl"), "hud-status-time-label = 时间\nhud-status-health-label = 生命\nhud-status-mana-label = 法力\nhud-character-panel-title = 角色\nhud-character-level-label = 等级\nhud-character-experience-label = 经验\nhud-character-modifiers-title = 生效中的属性修正\nhud-character-modifiers-empty = 无\nattribute-strength-display_name = 力量\nattribute-dexterity-display_name = 敏捷\nattribute-constitution-display_name = 体质\nattribute-intelligence-display_name = 智力\nattribute-willpower-display_name = 意志\nattribute-charisma-display_name = 魅力\nhud-inventory-panel-title = 背包\nhud-inventory-empty = （空）\nhud-inventory-durability-label = 耐久\nhud-equipment-panel-title = 装备\nhud-equipment-empty-slot = （空）\nequip_slot-main_hand-display_name = 主手\nequip_slot-off_hand-display_name = 副手\nequip_slot-head-display_name = 头部\nequip_slot-face-display_name = 面部\nequip_slot-eyes-display_name = 眼部\nequip_slot-neck-display_name = 颈部\nequip_slot-body-display_name = 躯干\nequip_slot-outer-display_name = 外袍\nequip_slot-back-display_name = 背部\nequip_slot-shoulder_l-display_name = 左肩\nequip_slot-shoulder_r-display_name = 右肩\nequip_slot-arm_l-display_name = 左臂\nequip_slot-arm_r-display_name = 右臂\nequip_slot-hand_l-display_name = 左手\nequip_slot-hand_r-display_name = 右手\nequip_slot-belt-display_name = 腰带\nequip_slot-tasset-display_name = 腿甲\nequip_slot-legs-display_name = 双腿\nequip_slot-boot_l-display_name = 左靴\nequip_slot-boot_r-display_name = 右靴\nequip_slot-ring_l-display_name = 左戒指\nequip_slot-ring_r-display_name = 右戒指\n").expect("测试用写入应当成功");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ll-ui-hud-render-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        dir
    }

    fn sample_character_data<'a>(
        modifiers: &'a BTreeMap<
            ll_world::entity::AttributeKind,
            BTreeMap<ll_core::ident::ContentIndex, ll_world::entity::ActiveStatModifier>,
        >,
        equipment: &'a BTreeMap<EquipSlot, ItemStack>,
    ) -> CharacterPanelData<'a> {
        CharacterPanelData {
            base_stats: BaseStats::BASELINE,
            active_stat_modifiers: modifiers,
            equipment,
            level: 1,
            experience: 0,
            xp_to_next_level: 100,
            now: Tick(0),
        }
    }

    #[test]
    fn build_hud_frame产出的纯色矩形数等于四块面板背景加三条条形() {
        // Arrange：每块面板背景恒是 9 块（见 `widget::panel::panel_quads`
        // 「panel_quads恒产出九块」测试），与面板内部有多少行文字无关
        // ——四块面板 4*9=36,生命/法力双层条各 3 块（背景+余晖+立即）
        // 共 6 块,经验单层条 2 块,合计 44。`FlatColorSkin` 的
        // `textured_*` 方法恒返回 `None`（trait 默认实现),因此全部内容
        // 都落进 `quads`,`textured_quads` 恒为空,见后一条测试。
        let dir = temp_dir("quad-count");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        // Act
        let frame = build_hud_frame(
            &status,
            &character,
            &[],
            &equipment,
            &NoItems,
            &item_table,
            &catalog,
            "zh-CN",
            &FlatColorSkin,
            &mut anim,
            0,
        );

        // Assert：四块面板各 9 块背景 + 双层条 2*3 块 + 单层条 2 块。
        assert_eq!(frame.quads.len(), 4 * 9 + 2 * 3 + 2);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_hud_frame对纯色皮肤产出的贴图矩形恒为空() {
        // Arrange
        let dir = temp_dir("textured-quad-empty");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        // Act
        let frame = build_hud_frame(
            &status,
            &character,
            &[],
            &equipment,
            &NoItems,
            &item_table,
            &catalog,
            "zh-CN",
            &FlatColorSkin,
            &mut anim,
            0,
        );

        // Assert
        assert!(frame.textured_quads.is_empty());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_hud_frame产出的文本行数与四块面板行数之和一致() {
        // Arrange
        let dir = temp_dir("label-count");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        // Act
        let frame = build_hud_frame(
            &status,
            &character,
            &[],
            &equipment,
            &NoItems,
            &item_table,
            &catalog,
            "zh-CN",
            &FlatColorSkin,
            &mut anim,
            0,
        );

        // Assert
        assert_eq!(frame.labels.len(), 1 + 11 + 2 + 23);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 生命值下降时状态栏数字立即反映新值不受动画影响() {
        // 这是「数字瞬时,条形动画」硬规则的直接验证——见
        // `crate::widget::anim` 模块文档。构造两次调用,健康值从满值
        // 掉到 30,断言状态栏文本行（而非条形）里的数字在下一帧就已经
        // 是 30,不是正在从 100 往下滑的中间值。
        // Arrange
        let dir = temp_dir("instant-number");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();
        let full_status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
        };
        build_hud_frame(
            &full_status,
            &character,
            &[],
            &equipment,
            &NoItems,
            &item_table,
            &catalog,
            "zh-CN",
            &FlatColorSkin,
            &mut anim,
            0,
        );

        // Act：紧接着下一帧,生命值已经掉到 30。
        let damaged_status = StatusBarData {
            clock: Tick(0),
            health: 30,
            mana: 50,
        };
        let frame = build_hud_frame(
            &damaged_status,
            &character,
            &[],
            &equipment,
            &NoItems,
            &item_table,
            &catalog,
            "zh-CN",
            &FlatColorSkin,
            &mut anim,
            1,
        );

        // Assert：状态栏文本行里应该已经是 30,不是 100 附近的过渡值。
        let status_line = &frame.labels[0].text;
        assert!(status_line.contains("30"));
        assert!(!status_line.contains("100"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
