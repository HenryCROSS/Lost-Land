//! HUD 的最外层薄封装：把四块面板的布局、面板背景、经验条、昼夜滑条、
//! 文本行全部接起来，一次性提交到 GPU。
//!
//! # 布局：状态栏 + 资源条 + 昼夜滑条通栏在左上，角色/背包纵向堆叠在
//! 左侧，装备栏单独锚定在屏幕右边
//!
//! 项目所有者两条追加要求，本批次落地：
//!
//! 1. 「背包先临时放在角色下面」——此前背包与角色面板并排成一列，
//!    没有问题；但装备栏原来紧跟在背包右边，三列一路向右排开，在较窄
//!    的窗口里会整个压在地图视口上（见下一条）。
//! 2. 「装备放在屏幕右边」——装备栏改成不再跟随角色/背包的列位置，
//!    而是独立锚定到窗口右边缘（[`equipment_origin_x`]），与状态栏
//!    锚定左上角是同一种「贴屏幕边缘」的 HUD 摆法，不再侵占地图视口
//!    中段。
//!
//! ```text
//! +--------------------------------+          +------------------+
//! | 状态栏（时间/季节/生命/法力）    |          | 装备栏面板         |
//! +----------------+----------------+          | 22 个槽位          |
//! | 生命条 | 法力条 |                |          |                  |
//! +--------------------------------+          |                  |
//! | 昼夜滑条（指针标当前时刻）        |          |                  |
//! +--------------------------------+          |                  |
//! | 角色面板                        |          |                  |
//! | 六项属性 / 等级 / 经验(条形)     |          |                  |
//! | 生效中的修正                     |          |                  |
//! +--------------------------------+          |                  |
//! | 背包面板（角色下方）             |          |                  |
//! +--------------------------------+          +------------------+
//! ```
//!
//! 只改了调用点的坐标算法（[`build_hud_frame`] 里各面板 `origin` 的
//! 算法），**没有改动任何控件本身**——[`Rect::stack_below`]/
//! [`Rect::stack_right`] 与四块面板各自的 `*_panel` 函数签名一行未动，
//! 见 `crate::widget::geometry` 模块文档「只做『能定位、能堆叠』」
//! 一节：这正是那份克制换来的好处，布局怎么摆是纯粹的调用点决定,不
//! 需要动几何原语本身。
//!
//! 列宽固定（[`CHARACTER_WIDTH`]/[`INVENTORY_WIDTH`]/
//! [`EQUIPMENT_WIDTH`]），高度各自按内容行数现算（见
//! [`super::build_panel`]）——这条既有纪律不受本次布局调整影响。

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
use crate::widget::day_night_bar::{
    day_night_bar_quads, day_night_pointer_quad, textured_day_night_bar_quads,
};
use crate::widget::geometry::Rect;
use crate::widget::label::Label;
use crate::widget::panel::{panel_quads, textured_panel_quads};
use crate::widget::quad::{QuadInstance, QuadRenderer};
use crate::widget::skin::{BarStyleId, DayNightBarStyleId, PanelStyleId, Skin};
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
/// 昼夜滑条高度（像素）——比资源条略高，指针需要足够的可点面积（虽然
/// 本批次不做点击，但预留出与其它条形手感一致的粗细）。
const DAY_NIGHT_BAR_HEIGHT: f32 = 14.0;
/// 昼夜滑条宽度（像素）——与生命/法力条并排后的总宽对齐，让状态栏下方
/// 这一整块（资源条 + 昼夜滑条）左右边界看起来是同一列。
const DAY_NIGHT_BAR_WIDTH: f32 = RESOURCE_BAR_WIDTH * 2.0 + PANEL_GAP;
/// 装备栏与窗口右边缘的留白（像素）——见模块文档「装备放在屏幕右边」
/// 一节，与 [`SCREEN_MARGIN`] 取同一个值，让装备栏与状态栏在视觉上
/// 是对称锚定在屏幕两侧的一对。
const EQUIPMENT_RIGHT_MARGIN: f32 = SCREEN_MARGIN;
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

/// 装备栏面板左上角的 x 坐标——恒锚定在窗口右边缘往里缩
/// [`EQUIPMENT_RIGHT_MARGIN`] + [`EQUIPMENT_WIDTH`]，不随角色/背包列
/// 挪动，见模块文档「装备放在屏幕右边」一节。`screen_width` 小于面板
/// 本身宽度这种极端窗口尺寸下会算出负坐标——不钳制：本批次窗口尺寸
/// 由 `ll_platform::window::WindowConfig` 固定给定（不可拖拽缩放，见
/// 其文档；本 crate 不依赖 `ll-platform`，此处只是文字引用，不是可
/// 解析的文档内链，与 `crate::widget::mod` 模块文档同一条既有写法），
/// 不会真的出现这种尺寸，钳制反而会掩盖「窗口配置改小了却没人发现
/// 装备栏被塞没了」这种应该显形的问题。
fn equipment_origin_x(screen_width: f32) -> f32 {
    screen_width - EQUIPMENT_RIGHT_MARGIN - EQUIPMENT_WIDTH
}

/// 现算这一帧 HUD 需要的全部填色矩形/贴图矩形与文本行——纯函数,不
/// 接触 GPU,是 [`render_hud`] 与本模块测试共用的核心逻辑。
///
/// `anim`/`now` 只驱动条形（含昼夜滑条指针）的**视觉位置**
/// （[`AnimatedValue`] 的显示值）——面板/文本内容与真实世界状态之间
/// 没有经过这张表,数字瞬时,见 `crate::widget::anim` 模块文档「数字
/// 瞬时,条形动画」一节；昼夜滑条同一条纪律的延伸见
/// `crate::widget::day_night_bar` 模块文档「数字瞬时，指针平滑」一节。
///
/// `screen_width` 是窗口原生像素宽度，只用来算装备栏的锚定坐标（见
/// [`equipment_origin_x`]），不影响其余面板的布局。
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
    screen_width: f32,
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
    // 本身（状态栏文本）从不经过 `anim`,见本函数文档。两条颜色不同
    // （[`BarStyleId::Health`]/[`BarStyleId::Mana`]）——此前共用同一个
    // `HealthMana` 样式,两条外观恒相同,是「分不清哪条是哪条」的真实
    // 截图问题的根因,见 `crate::widget::skin::BarStyleId::Health` 文档。
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
        BarStyleId::Health,
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
        BarStyleId::Mana,
        skin,
    );

    // 昼夜滑条：紧贴在生命/法力条下方，指针位置经 `anim` 平滑过渡,
    // 真实归一化位置本身（`day_night_pointer_fraction`）是瞬时值,见
    // `crate::hud::status_bar` 模块文档「昼夜滑条指针位置」一节与本
    // 函数文档。
    let day_night_rect = Rect::new(
        health_rect.x,
        health_rect.bottom() + PANEL_GAP,
        DAY_NIGHT_BAR_WIDTH,
        DAY_NIGHT_BAR_HEIGHT,
    );
    let pointer_real = status_bar::day_night_pointer_fraction(status.clock);
    let pointer_displayed = anim.animate("hud.day_night_pointer", pointer_real, now);
    push_day_night_bar(
        &mut quads,
        &mut textured_quads,
        day_night_rect,
        pointer_displayed,
        skin,
    );

    // 角色面板：紧贴在昼夜滑条下方——此前直接在生命/法力条下方,现在
    // 多插了一条昼夜滑条,角色面板跟着往下挪一格,列宽/内容不变。
    let row_origin = (SCREEN_MARGIN, day_night_rect.bottom() + PANEL_GAP);

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

    // 背包面板：紧贴在经验条（角色面板的一部分）下方——项目所有者原话
    // 「背包先临时放在角色下面」,与此前「背包在角色右边」的并排布局
    // 不同,见模块文档「布局」一节。
    let inventory_origin = bar_rect.stack_below(PANEL_GAP, INVENTORY_WIDTH).origin();
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

    // 装备栏：不再跟随背包列的位置,独立锚定到窗口右边缘——项目所有者
    // 原话「装备放在屏幕右边」,见 [`equipment_origin_x`] 与模块文档。
    let equipment_origin = (equipment_origin_x(screen_width), SCREEN_MARGIN);
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

/// 推入一条双层条形（生命/法力），分支逻辑同 [`push_panel`]。`style`
/// 由调用方指定是 [`BarStyleId::Health`] 还是 [`BarStyleId::Mana`]——
/// 两者外观（含贴图 tint）在 [`crate::widget::skin`] 里各自独立解析,
/// 是「两条资源条能分清哪条是哪条」这条修复的直接落点,见
/// `crate::widget::skin::BarStyleId::Health` 文档。
fn push_two_layer_bar(
    quads: &mut Vec<QuadInstance>,
    textured_quads: &mut Vec<TexturedQuadInstance>,
    rect: Rect,
    immediate_fraction: f32,
    lagging_fraction: f32,
    style: BarStyleId,
    skin: &dyn Skin,
) {
    match skin.textured_two_layer_bar(style) {
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
            let appearance = skin.bar(style);
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

/// 推入昼夜滑条：整条背景（分支逻辑同 [`push_panel`]）+ 指针——指针
/// **恒是纯色矩形**（不区分纯色/贴图两条路径），颜色取自实际生效的那
/// 份外观（贴图皮肤用
/// `crate::widget::day_night_bar::TexturedDayNightBarAppearance::pointer_color`，
/// 纯色回退用
/// `crate::widget::day_night_bar::FlatDayNightBarAppearance::pointer_color`），
/// 见 `crate::widget::day_night_bar` 模块文档「颜色走皮肤层」一节。
fn push_day_night_bar(
    quads: &mut Vec<QuadInstance>,
    textured_quads: &mut Vec<TexturedQuadInstance>,
    rect: Rect,
    pointer_fraction: f32,
    skin: &dyn Skin,
) {
    let pointer_color = match skin.textured_day_night_bar(DayNightBarStyleId::Clock) {
        Some(appearance) => {
            textured_quads.extend(textured_day_night_bar_quads(rect, &appearance));
            appearance.pointer_color
        }
        None => {
            let appearance = skin.day_night_bar(DayNightBarStyleId::Clock);
            quads.extend(day_night_bar_quads(rect, &appearance));
            appearance.pointer_color
        }
    };
    quads.push(day_night_pointer_quad(
        rect,
        pointer_color,
        pointer_fraction,
    ));
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
        status,
        character,
        inventory,
        equipment,
        items,
        item_table,
        catalog,
        language,
        skin,
        anim,
        now,
        resolution_width as f32,
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
        std::fs::write(dir.join("zh-CN.ftl"), "hud-status-time-label = 时间\nhud-status-health-label = 生命\nhud-status-mana-label = 法力\nseason-spring-display_name = 春\nseason-summer-display_name = 夏\nseason-autumn-display_name = 秋\nseason-winter-display_name = 冬\nhud-character-panel-title = 角色\nhud-character-level-label = 等级\nhud-character-experience-label = 经验\nhud-character-modifiers-title = 生效中的属性修正\nhud-character-modifiers-empty = 无\nattribute-strength-display_name = 力量\nattribute-dexterity-display_name = 敏捷\nattribute-constitution-display_name = 体质\nattribute-intelligence-display_name = 智力\nattribute-willpower-display_name = 意志\nattribute-charisma-display_name = 魅力\nhud-inventory-panel-title = 背包\nhud-inventory-empty = （空）\nhud-inventory-durability-label = 耐久\nhud-equipment-panel-title = 装备\nhud-equipment-empty-slot = （空）\nequip_slot-main_hand-display_name = 主手\nequip_slot-off_hand-display_name = 副手\nequip_slot-head-display_name = 头部\nequip_slot-face-display_name = 面部\nequip_slot-eyes-display_name = 眼部\nequip_slot-neck-display_name = 颈部\nequip_slot-body-display_name = 躯干\nequip_slot-outer-display_name = 外袍\nequip_slot-back-display_name = 背部\nequip_slot-shoulder_l-display_name = 左肩\nequip_slot-shoulder_r-display_name = 右肩\nequip_slot-arm_l-display_name = 左臂\nequip_slot-arm_r-display_name = 右臂\nequip_slot-hand_l-display_name = 左手\nequip_slot-hand_r-display_name = 右手\nequip_slot-belt-display_name = 腰带\nequip_slot-tasset-display_name = 腿甲\nequip_slot-legs-display_name = 双腿\nequip_slot-boot_l-display_name = 左靴\nequip_slot-boot_r-display_name = 右靴\nequip_slot-ring_l-display_name = 左戒指\nequip_slot-ring_r-display_name = 右戒指\n").expect("测试用写入应当成功");
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
        // 共 6 块,经验单层条 2 块,昼夜滑条 2 块（整条背景+指针）,合计
        // 46。`FlatColorSkin` 的 `textured_*` 方法恒返回 `None`（trait
        // 默认实现),因此全部内容都落进 `quads`,`textured_quads` 恒为
        // 空,见后一条测试。
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
            1280.0,
        );

        // Assert：四块面板各 9 块背景 + 双层条 2*3 块 + 单层条 2 块 +
        // 昼夜滑条 2 块（整条背景 + 指针）。
        assert_eq!(frame.quads.len(), 4 * 9 + 2 * 3 + 2 + 2);

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
            1280.0,
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
            1280.0,
        );

        // Assert：昼夜滑条不产出文本行（时间数字仍然只在状态栏那一行
        // 里,见 `crate::hud::status_bar` 模块文档「昼夜滑条指针位置」
        // 一节),四块面板的行数之和不受影响。
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
            1280.0,
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
            1280.0,
        );

        // Assert：状态栏文本行里应该已经是 30,不是 100 附近的过渡值。
        let status_line = &frame.labels[0].text;
        assert!(status_line.contains("30"));
        assert!(!status_line.contains("100"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn equipment_origin_x等于窗口宽度减去右边距与装备栏宽度() {
        // Arrange & Act
        let x = equipment_origin_x(1280.0);

        // Assert
        assert_eq!(x, 1280.0 - EQUIPMENT_RIGHT_MARGIN - EQUIPMENT_WIDTH);
    }

    #[test]
    fn 装备栏面板锚定在窗口右边缘不跟随背包列() {
        // 项目所有者原话「装备放在屏幕右边」——这条测试直接核实装备栏
        // 矩形的右边界贴着窗口右边缘（减去留白），而不是像此前那样跟在
        // 背包面板右边、随内容宽度漂移。
        // Arrange
        let dir = temp_dir("equipment-right-edge");
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
        let screen_width = 1280.0;

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
            screen_width,
        );

        // Assert：`build_hud_frame` 按固定顺序推入面板/条形——状态栏
        // （9 块）、生命条（3）、法力条（3）、昼夜滑条（2）、角色面板
        // （9）、经验条（2）、背包面板（9），装备栏是第 8 个、也是最后
        // 一个推入的面板，前面共 9+3+3+2+9+2+9=37 块。装备栏九宫格的
        // 第一块（左上角）位置即等于它的 `origin`，直接核实这一块的
        // x 坐标等于 [`equipment_origin_x`]。
        let equipment_first_quad = &frame.quads[37];
        assert_eq!(
            equipment_first_quad.position[0],
            equipment_origin_x(screen_width)
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 构造一份最小可用的 HUD 帧，供下面两条「背包紧跟角色列」测试
    /// 共用——两条测试只是对同一帧取不同的断言，抽出这个帮手避免
    /// 复制粘贴整段 Arrange/Act。
    fn build_frame_for_inventory_layout_test(dir_name: &str) -> (HudFrame, std::path::PathBuf) {
        let dir = temp_dir(dir_name);
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
            1280.0,
        );
        (frame, dir)
    }

    #[test]
    fn 背包面板与角色面板左边界对齐() {
        // 项目所有者原话「背包先临时放在角色下面」——这条测试核实背包
        // 面板与角色面板共用同一个左边界（同一列），而不是像此前那样在
        // 角色面板右边另起一列。角色面板九宫格从第 17 块开始
        // （9+3+3+2），背包面板从第 28 块开始（再加角色面板 9 块 + 经验
        // 条 2 块），两者的第一块（左上角）就是各自面板的 `origin`。
        // Arrange & Act
        let (frame, dir) = build_frame_for_inventory_layout_test("inventory-left-aligned");

        // Assert
        assert_eq!(frame.quads[17].position[0], frame.quads[28].position[0]);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 背包面板位于角色面板下方() {
        // 见上一条测试的索引说明。
        // Arrange & Act
        let (frame, dir) = build_frame_for_inventory_layout_test("inventory-below");

        // Assert
        assert!(frame.quads[28].position[1] > frame.quads[17].position[1]);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
