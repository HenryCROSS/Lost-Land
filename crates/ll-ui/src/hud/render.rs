//! HUD 的最外层薄封装：把四块面板的布局、面板背景、经验条、昼夜滑条、
//! 文本行全部接起来，一次性提交到 GPU。
//!
//! # 谁盖住谁由 [`crate::widget::layer::UiLayer`] 决定
//!
//! 本模块**不**用「谁后推入谁在上面」表达遮挡关系——那条约定在这里从
//! 来就不成立（内容被分装进纯色/贴图两个容器，跨容器的先后由渲染 pass
//! 的固定顺序决定）。每块内容显式声明自己属于哪一层，新加一块 UI 时该
//! 怎么选层见 [`crate::widget::layer`] 模块文档。
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

use ll_core::ident::ContentIndex;
use ll_i18n::Catalog;
use ll_mod::item::ItemTable;
use ll_render::wgpu;
use ll_sim::item::ItemCatalog;
use ll_text::TextRenderer;
use ll_world::item::{EquipSlot, ItemStack};

use super::action_menu::ActionMenuData;
use super::character_panel::{self, CharacterPanelData};
use super::equipment_panel;
use super::inventory_panel;
use super::status_bar::{self, StatusBarData};
use super::world_map::{self, WorldMapPanelData};
use crate::widget::anim::{DEFAULT_ANIM_DURATION_FRAMES, FrameTick};
use crate::widget::bar::{bar_quads, textured_bar_quads, textured_two_layer_bar_quads};
use crate::widget::day_night_bar::{
    day_night_bar_quads, day_night_pointer_quad, textured_day_night_bar_quads,
    textured_day_night_pointer_quad,
};
use crate::widget::geometry::Rect;
use crate::widget::label::Label;
use crate::widget::layer::{DrawBatch, LayerBatch, LayeredFrame, UiLayer};
use crate::widget::panel::{panel_quads, textured_panel_quads};
use crate::widget::quad::QuadRenderer;
use crate::widget::skin::{BarStyleId, DayNightBarStyleId, PanelStyleId, Skin};
use crate::widget::state::{WidgetStateTable, animate_experience_bar};
use crate::widget::textured_quad::TexturedQuadRenderer;
use glyphon::Color;
use std::collections::BTreeMap;

/// 面板左上角与窗口边缘的留白（像素）。
pub(super) const SCREEN_MARGIN: f32 = 16.0;
/// 三列之间、状态栏与三列之间的间隔（像素）。
pub(super) const PANEL_GAP: f32 = 10.0;
/// 状态栏通栏宽度。
pub const STATUS_WIDTH: f32 = 620.0;
/// 角色面板宽度。
pub const CHARACTER_WIDTH: f32 = 260.0;
/// 背包面板宽度。
pub const INVENTORY_WIDTH: f32 = 220.0;
/// 装备栏面板宽度。
pub const EQUIPMENT_WIDTH: f32 = 220.0;
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
/// 动作菜单面板宽度——比背包/装备两列宽一些：它的行要同时容下配方名
/// 与食材清单（见 `ll_game::player_action` 的排版），照 220 会频繁截断。
pub const ACTION_MENU_WIDTH: f32 = 360.0;
// 反馈行与按键提示行的宽度/位置常量搬去了 `super::bottom_rows`——
// 两行形状相同，放在一起才不会各写一份。这里再导出一次，
// `ll_ui::hud::render::FEEDBACK_WIDTH` 这条既有路径不变。
pub use super::bottom_rows::{FEEDBACK_WIDTH, KEY_HINT_WIDTH};
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

// 这一帧要提交的内容现在装在 `crate::widget::layer::LayeredFrame` 里
// （按层分装），不再是本模块自己的 `HudFrame`——「谁盖住谁」由层级
// 决定，而不是由「先提交完一整批纯色、再提交完一整批贴图」这个固定
// pass 顺序决定，见 `crate::widget::layer` 模块文档记录的那条实机缺陷。

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

/// 世界地图比例尺文案与面板边框的留白（像素）——比
/// [`SCREEN_MARGIN`] 小：这行字贴在地图**内侧**，留白太大就会压到
/// 第一行格子上。
const WORLD_MAP_CAPTION_MARGIN: f32 = 8.0;

/// 世界地图面板与屏幕四边的留白比例——见 [`world_map_rect`]。取
/// 10%：地图本身要足够大才有实际可读性（M 键切换的目的就是「看整个
/// 世界」，不是又开一块小面板），同时四周留出的边距足以让玩家看出
/// 这是一层覆盖在游戏画面上的浮层，而不是把整个屏幕都吃掉。
const WORLD_MAP_MARGIN_FRACTION: f32 = 0.1;

/// 世界地图面板这一帧的矩形——以屏幕为参照居中，四边各留
/// [`WORLD_MAP_MARGIN_FRACTION`] 的屏幕尺寸,理由见该常量文档。与
/// [`equipment_origin_x`] 同一种「按屏幕原生像素尺寸现算,不写死常量」
/// 的取舍——窗口尺寸由 `ll_platform::window::WindowConfig` 固定给定
/// （见 [`equipment_origin_x`] 文档同一段说明),按比例现算仍然比写死
/// 像素常量更不容易在窗口配置调整后悄悄错位。
/// 按这块菜单声明的 [`MenuPlacement`] 把它摆到屏幕上。
///
/// # 为什么要「先建一次、再整体平移」
///
/// 面板高度是**内容现算**的（[`super::build_panel`]：行数 × 行高 + 上下
/// 内边距），行数取决于这一帧有几行可选项——`ScreenCenter` 要垂直居中
/// 就必须先知道这个高度。两条可选路：把行数算法在这里再写一遍（迟早
/// 与 `write_action_menu_lines` 分叉），或者建完之后整体平移。选后者：
/// 平移是纯几何、没有第二份真相源，而且 `PanelContent` 只有一个矩形
/// 加一列标签，平移的代价是一次线性遍历。
///
/// 水平方向两个变体都居中（这是本函数落地之前就有的行为），差别只在
/// 垂直：`TopCenter` 贴上沿，`ScreenCenter` 也居中。
///
/// 世界地图面板的**外框**矩形：屏幕四周各留一成边距，居中一块。
///
/// # 为什么是公开的
///
/// 「玩家点的像素落在哪个区块」这条反算
/// （[`crate::hud::world_map::world_map_zone_at_pixel`]）要的正是这一份
/// 矩形——**必须与画图时用的那一份逐字相同**，否则玩家点的地方与选中
/// 的区块会系统性偏移一个边距，而这种偏差小到肉眼看不出来，只会表现为
/// 「偶尔点到隔壁那格」。开放它，是为了让选出生地屏（`ll_game::app`）
/// 拿到同一个真相源，而不是在那边照着这里的公式再抄一份。
pub fn world_map_rect(screen_width: f32, screen_height: f32) -> Rect {
    let margin_x = screen_width * WORLD_MAP_MARGIN_FRACTION;
    let margin_y = screen_height * WORLD_MAP_MARGIN_FRACTION;
    Rect::new(
        margin_x,
        margin_y,
        (screen_width - margin_x * 2.0).max(0.0),
        (screen_height - margin_y * 2.0).max(0.0),
    )
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
/// `screen_width`/`screen_height` 是窗口原生像素尺寸——前者此前就已
/// 存在，只用来算装备栏的锚定坐标（见 [`equipment_origin_x`]）；后者
/// 是世界地图批次新增的，只用来算世界地图面板的居中矩形（见
/// [`world_map_rect`]），同样不影响其余面板的布局。
///
/// `world_map` 为 `None` 时（M 键未按下/已关闭）世界地图整块不参与本次
/// `quads`/`textured_quads` 的产出——不是「画出来但透明」，是压根不
/// 调用 [`world_map::world_map_frame`]，见
/// `ll_platform::input::GameKey::Map` 文档与 `ll_game::app` 模块文档
/// 「地图开关」一节。为 `Some` 时按其中的格子数据画出整块面板，见
/// [`world_map::world_map_frame`]。
///
/// `menu` 为 `None` 时（没有任何动作菜单打开）那块面板整块不参与本次
/// 产出，与 `world_map` 同一条纪律。为 `Some` 时按
/// [`super::action_menu::action_menu_panel`] 画出，**画在哪由那块菜单
/// 自己声明的 [`MenuPlacement`] 决定**（见 [`placed_action_menu`]）：
/// 背包与制作贴屏幕上沿、水平居中（本参数落地以来一直如此），交互列表
/// 与方向列表水平垂直**都居中**——所有者裁定「那个互动显示的 UI 窗口，
/// 我希望是出现在屏幕正中间」。
///
/// 两者水平方向都居中，因此都不与左侧那一列常驻面板重叠，玩家挑东西时
/// 仍看得见自己的属性与背包。
///
/// `feedback` 是一句**已经解析好**的反馈文字（`None` 表示这一帧没有
/// 反馈要说）。它存在的唯一理由是：`ll_sim::resolve` 判定「这一步什么
/// 都不发生」时静默返回空效果——对 AI 无所谓，对玩家不行，按了键屏幕
/// 纹丝不动会被当成游戏卡死，见 `ll_sim::turn::PlayerTurnOutcome` 文档。
/// 收的是**已解析的字符串**而不是 Fluent 键：这一句可能需要参数
/// （「背包里没有这一种东西」之类未来的细化），参数注入是调用方的事,
/// 本层不该为此再引一套参数表。
///
/// [`AnimatedValue`]: crate::widget::anim::AnimatedValue
#[allow(clippy::too_many_arguments)]
pub fn build_hud_frame(
    status: &StatusBarData<'_>,
    character: &CharacterPanelData<'_>,
    inventory: &[ItemStack],
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    // 观察者已经认得的物品种类——未鉴定的东西在两块物品面板上显示成
    // 「未鉴定的物品」，见 `super::item_display_name`。
    identified: &[ContentIndex],
    items: &dyn ItemCatalog,
    item_table: &ItemTable,
    catalog: &Catalog,
    language: &str,
    skin: &dyn Skin,
    // 「这段字画出来多宽、断成几行」的唯一来源。产品路径传的是
    // `ll_text::TextRenderer` 自己（它复用自己那份 `FontSystem`），
    // 测试与门禁传纯 CPU 的 `ll_text::TextMeasurer`——两条路径底下是
    // 同一个 `layout_text`，见 `ll_text::measure` 模块文档。
    measure: &mut dyn ll_text::MeasureText,
    anim: &mut WidgetStateTable,
    now: FrameTick,
    screen_width: f32,
    screen_height: f32,
    world_map: Option<&WorldMapPanelData<'_>>,
    menu: Option<&ActionMenuData<'_>>,
    feedback: Option<&str>,
    // 世界里那一行常驻的按键提示（规格 F6），`None` = 这一刻不显示
    // （有模态屏盖着，或者压根没有世界）。与 `feedback` 完全同构：
    // 本层只收**已经排好版的一句话**，键名怎么从当前键位表现查是
    // `ll_game::key_hint` 的事，见那个模块。
    key_hint: Option<&str>,
) -> LayeredFrame {
    let mut frame = LayeredFrame::default();
    // 常驻 HUD 全部落在最底层，见 `crate::widget::layer` 模块文档的
    // 选层判据表。
    let hud = frame.layer_mut(UiLayer::Hud);

    let status_origin = (SCREEN_MARGIN, SCREEN_MARGIN);
    let status_panel = status_bar::status_bar_panel(
        status,
        catalog,
        language,
        measure,
        status_origin,
        STATUS_WIDTH,
    );
    push_panel(hud, &status_panel.rect, status_panel.labels, skin);

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
        hud,
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
        hud,
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
    push_day_night_bar(hud, day_night_rect, pointer_displayed, skin);

    // 角色面板：紧贴在昼夜滑条下方——此前直接在生命/法力条下方,现在
    // 多插了一条昼夜滑条,角色面板跟着往下挪一格,列宽/内容不变。
    let row_origin = (SCREEN_MARGIN, day_night_rect.bottom() + PANEL_GAP);

    let character_panel_content = character_panel::character_panel(
        character,
        items,
        catalog,
        language,
        measure,
        row_origin,
        CHARACTER_WIDTH,
    );
    push_panel(
        hud,
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
    push_bar(hud, bar_rect, displayed_fraction, skin);

    // 背包面板：紧贴在经验条（角色面板的一部分）下方——项目所有者原话
    // 「背包先临时放在角色下面」,与此前「背包在角色右边」的并排布局
    // 不同,见模块文档「布局」一节。
    let inventory_origin = bar_rect.stack_below(PANEL_GAP, INVENTORY_WIDTH).origin();
    let inventory_panel_content = inventory_panel::inventory_panel(
        inventory,
        item_table,
        catalog,
        language,
        identified,
        measure,
        inventory_origin,
        INVENTORY_WIDTH,
    );
    push_panel(
        hud,
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
        identified,
        measure,
        equipment_origin,
        EQUIPMENT_WIDTH,
    );
    push_panel(
        hud,
        &equipment_panel_content.rect,
        equipment_panel_content.labels,
        skin,
    );

    // 世界地图：M 键切换的近全屏覆盖层，`None` 时整块不参与本次产出
    // ——见本函数文档「`world_map` 为 `None` 时」一节。
    //
    // 它落在 `UiLayer::Overlay`，**恒盖住整个常驻 HUD**。此前它只是
    // 「在同一个 `quads` 里最后追加」，而常驻 HUD 在真实贴图皮肤下走
    // 的是另一个容器、另一道 pass，于是血条与四块面板反过来压在地图
    // 上——所有者实机反馈的那条缺陷，根因与修法见
    // `crate::widget::layer` 模块文档。
    if let Some(world_map) = world_map {
        let rect = world_map_rect(screen_width, screen_height);
        let overlay = frame.layer_mut(UiLayer::Overlay);
        let map_frame = world_map::world_map_frame(world_map, rect, skin);
        overlay.quads.extend(map_frame.quads);
        // 玩家标记的贴图矩形：与地图格同一层，层内贴图恒画在纯色之上,
        // 标记因此压住地形格与据点标记,见
        // `world_map::WorldMapFrame::textured_quads` 文档。
        overlay.textured_quads.extend(map_frame.textured_quads);
        // 比例尺与操作提示：贴在地图面板的左上角内侧。走本层自己的
        // 文本批次——文本不再是「全屏最后一道 pass」，而是**层内**最后
        // 一道，因此常驻 HUD 的文字不会浮在地图上面。
        //
        // `tiles_per_cell` 为 0 表示调用方还没接缩放（例如只想画一张
        // 固定视图），此时整行不出现——与 `world_map` 为 `None` 时整块
        // 不产出是同一条「没有就不画，不留占位」的纪律。
        if world_map.tiles_per_cell > 0 {
            overlay.labels.push(Label {
                text: world_map::scale_caption(world_map.tiles_per_cell, catalog, language),
                x: rect.x + WORLD_MAP_CAPTION_MARGIN,
                y: rect.y + WORLD_MAP_CAPTION_MARGIN,
                // 这一行的「面板」就是地图面板本身，断行宽度是它去掉
                // 两侧同一个留白之后的宽——与其余每一块面板同一条
                // 派生规则，见 `crate::widget::label::Label::max_width`。
                max_width: rect.width - WORLD_MAP_CAPTION_MARGIN * 2.0,
            });
        }
    }

    // 动作菜单：与世界地图同一条「`None` 就整块不产出」的纪律，见本
    // 函数文档。落在 `UiLayer::Popup`，恒压在地图之上——两者理论上可以
    // 同时打开，此时玩家正在菜单里选东西，地图只是背景。
    if let Some(menu) = menu {
        let panel = super::placement::placed_action_menu(
            menu,
            catalog,
            language,
            measure,
            screen_width,
            screen_height,
        );
        push_panel(
            frame.layer_mut(UiLayer::Popup),
            &panel.rect,
            panel.labels,
            skin,
        );
    }

    // 屏幕底部那两行——形状相同、分层不同，见 `super::bottom_rows`
    // 模块文档那张表。
    if let Some(text) = key_hint {
        let batch = frame.layer_mut(UiLayer::Hud);
        super::bottom_rows::push_key_hint_row(
            batch,
            measure,
            skin,
            text,
            screen_width,
            screen_height,
        );
    }
    if let Some(text) = feedback {
        let batch = frame.layer_mut(UiLayer::Notice);
        super::bottom_rows::push_feedback_row(
            batch,
            measure,
            skin,
            text,
            screen_width,
            screen_height,
        );
    }

    frame
}

/// 推入一块面板的背景——皮肤给出真实贴图外观
/// （[`Skin::textured_panel`]）就走贴图路径,否则回退到
/// [`Skin::panel`] 的纯色路径,两条路径互斥（同一块面板只会落进
/// [`LayerBatch::quads`] 或 [`LayerBatch::textured_quads`] 其中一个）。
pub(super) fn push_panel(
    batch: &mut LayerBatch,
    rect: &Rect,
    panel_labels: Vec<Label>,
    skin: &dyn Skin,
) {
    match skin.textured_panel(PanelStyleId::Window) {
        Some(appearance) => batch
            .textured_quads
            .extend(textured_panel_quads(*rect, &appearance)),
        None => batch
            .quads
            .extend(panel_quads(*rect, &skin.panel(PanelStyleId::Window))),
    }
    batch.labels.extend(panel_labels);
}

/// 推入一条单层条形（经验条），分支逻辑同 [`push_panel`]。
fn push_bar(batch: &mut LayerBatch, rect: Rect, fraction: f32, skin: &dyn Skin) {
    match skin.textured_bar(BarStyleId::Progress) {
        Some(appearance) => {
            batch
                .textured_quads
                .extend(textured_bar_quads(rect, fraction, &appearance))
        }
        None => batch
            .quads
            .extend(bar_quads(rect, fraction, &skin.bar(BarStyleId::Progress))),
    }
}

/// 推入一条双层条形（生命/法力），分支逻辑同 [`push_panel`]。`style`
/// 由调用方指定是 [`BarStyleId::Health`] 还是 [`BarStyleId::Mana`]——
/// 两者外观（含贴图 tint）在 [`crate::widget::skin`] 里各自独立解析,
/// 是「两条资源条能分清哪条是哪条」这条修复的直接落点,见
/// `crate::widget::skin::BarStyleId::Health` 文档。
fn push_two_layer_bar(
    batch: &mut LayerBatch,
    rect: Rect,
    immediate_fraction: f32,
    lagging_fraction: f32,
    style: BarStyleId,
    skin: &dyn Skin,
) {
    match skin.textured_two_layer_bar(style) {
        Some(appearance) => batch.textured_quads.extend(textured_two_layer_bar_quads(
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
            batch.quads.extend(crate::widget::bar::two_layer_bar_quads(
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

/// 推入昼夜滑条：整条底图 + 滑块，分支逻辑同 [`push_panel`]。
///
/// # 底图与滑块必须落进**同一个**容器
///
/// 两者恒是「底图先、滑块后」推入同一个 `Vec`：贴图路径两块都进
/// `textured_quads`，纯色回退两块都进 `quads`。**不允许一边贴图一边
/// 纯色**——那正是所有者实机反馈「少了滑条，只显示了背景条」的根因：
/// 纯色滑块与贴图底图分处层内两道 pass，底图恒后提交、把滑块整个盖住，
/// 见 `crate::widget::day_night_bar` 模块文档「曾经的缺陷」一节。
///
/// 这个不变式由皮肤层保证而不是靠这里的调用纪律：
/// `crate::widget::skin::NineSliceSkin::textured_day_night_bar` 里底图与
/// 滑块的 UV 各带一个 `?`，任一张查不到就整条返回 `None`，本函数因此
/// 只能整条走贴图或整条走纯色。
fn push_day_night_bar(batch: &mut LayerBatch, rect: Rect, pointer_fraction: f32, skin: &dyn Skin) {
    match skin.textured_day_night_bar(DayNightBarStyleId::Clock) {
        Some(appearance) => {
            batch
                .textured_quads
                .extend(textured_day_night_bar_quads(rect, &appearance));
            batch.textured_quads.push(textured_day_night_pointer_quad(
                rect,
                &appearance,
                pointer_fraction,
            ));
        }
        None => {
            let appearance = skin.day_night_bar(DayNightBarStyleId::Clock);
            batch.quads.extend(day_night_bar_quads(rect, &appearance));
            batch.quads.push(day_night_pointer_quad(
                rect,
                appearance.pointer_color,
                pointer_fraction,
            ));
        }
    }
}

/// 把 [`build_hud_frame`] 算出的内容真正提交到屏幕。
///
/// # 提交顺序 = [`LayeredFrame::draw_batches`]，不是「三道固定 pass」
///
/// 本函数**逐条遍历** [`LayeredFrame::draw_batches`]：按层升序，层内
/// 纯色（[`QuadRenderer`]）→ 贴图（[`TexturedQuadRenderer`]）→ 文本
/// （[`TextRenderer`]）。每条批次各自开一道 `LoadOp::Load` 的 pass，
/// 不清屏，叠加在调用方已经画好的世界层之上（见
/// [`crate::widget::quad`] 模块文档「为什么不复用 SpriteBatch」一节）。
///
/// **本函数不自己决定任何遮挡关系**——顺序全部来自 `draw_batches`，
/// 那也正是测试断言的对象，两者因此不可能分叉。
///
/// 此前这里是「先提交完一整批纯色、再提交完一整批贴图、最后全部文本」
/// 三道固定 pass。那个形状下，一块内容画在另一块之上与否取决于**皮肤
/// 给不给贴图**，与调用点的推入顺序无关——世界地图被血条压住、昼夜
/// 滑条的指针被自己的底图吞掉，都是它的直接后果，见
/// [`crate::widget::layer`] 模块文档。
#[allow(clippy::too_many_arguments)]
pub fn render_hud(
    quad_renderer: &mut QuadRenderer,
    textured_quad_renderer: &mut TexturedQuadRenderer,
    text_renderer: &mut TextRenderer,
    // 测量器与绘制器分开收：调用方（`ll_game::app::Demo`）持有的那一个
    // **同时**服务输入侧（模态屏行矩形）与渲染侧，两侧因此不可能对同
    // 一行算出不同的高度，见 `ll_text::measure` 模块文档。
    measure: &mut dyn ll_text::MeasureText,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::TextureView,
    resolution_width: u32,
    resolution_height: u32,
    status: &StatusBarData<'_>,
    character: &CharacterPanelData<'_>,
    inventory: &[ItemStack],
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    // 观察者已经认得的物品种类——未鉴定的东西在两块物品面板上显示成
    // 「未鉴定的物品」，见 `super::item_display_name`。
    identified: &[ContentIndex],
    items: &dyn ItemCatalog,
    item_table: &ItemTable,
    catalog: &Catalog,
    language: &str,
    skin: &dyn Skin,
    anim: &mut WidgetStateTable,
    now: FrameTick,
    world_map: Option<&WorldMapPanelData<'_>>,
    // `menu`/`feedback` 见 `build_hud_frame` 同名参数文档。
    menu: Option<&ActionMenuData<'_>>,
    feedback: Option<&str>,
    key_hint: Option<&str>,
) {
    let frame = build_hud_frame(
        status,
        character,
        inventory,
        equipment,
        identified,
        items,
        item_table,
        catalog,
        language,
        skin,
        measure,
        anim,
        now,
        resolution_width as f32,
        resolution_height as f32,
        world_map,
        menu,
        feedback,
        key_hint,
    );

    for batch in frame.draw_batches() {
        match batch {
            DrawBatch::Quads(quads) => quad_renderer.render(
                device,
                queue,
                target,
                resolution_width,
                resolution_height,
                quads,
            ),
            DrawBatch::Textured(textured) => textured_quad_renderer.render(
                device,
                queue,
                target,
                resolution_width,
                resolution_height,
                textured,
            ),
            DrawBatch::Labels(labels) => {
                let runs: Vec<_> = labels
                    .iter()
                    .map(|label| {
                        label.to_text_run(
                            super::DEFAULT_FONT_SIZE,
                            super::DEFAULT_LINE_HEIGHT,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::action_menu::MenuPlacement;
    use super::super::placement::placed_action_menu;
    use super::*;
    use crate::widget::skin::FlatColorSkin;
    use ll_core::time::Tick;
    use ll_sim::item::NoItems;
    use ll_world::entity::BaseStats;
    use std::path::Path;

    /// 本模块建一帧 HUD 的共用入口。
    ///
    /// [`build_hud_frame`] 有十八个参数，其中**八个在本模块每一条断言
    /// 里都取同一个值**（空背包、空已鉴定表、[`NoItems`]、中文、720 高、
    /// 无菜单、无反馈）。逐条抄十八行是这个测试模块最大的一块噪声，也是
    /// 每次给 `build_hud_frame` 加一个参数时最大的一块机械改动——本批次
    /// 加测量器参数时正是它把这个文件推过了行数棘轮门禁。
    #[allow(clippy::too_many_arguments)]
    fn 建帧(
        status: &StatusBarData<'_>,
        character: &CharacterPanelData<'_>,
        equipment: &BTreeMap<EquipSlot, ItemStack>,
        item_table: &ItemTable,
        catalog: &Catalog,
        skin: &dyn Skin,
        anim: &mut WidgetStateTable,
        now: FrameTick,
        screen_width: f32,
        world_map: Option<&WorldMapPanelData<'_>>,
    ) -> LayeredFrame {
        build_hud_frame(
            status,
            character,
            &[],
            equipment,
            &[],
            &NoItems,
            item_table,
            catalog,
            "zh-CN",
            skin,
            &mut crate::测试测量器(),
            anim,
            now,
            screen_width,
            720.0,
            world_map,
            None,
            None,
            None,
        )
    }

    // 底部两行（反馈行 / 按键提示行）的断言住在隔壁文件，用 `#[path]`
    // 挂成本模块的子模块——手法与 `ll_game` 的 `app_tests.rs` 一样。
    // 本文件已经在行数棘轮的快照里（`scripts/ci/file_size_budget.json`），
    // 而那条断言要用本模块这一整套夹具（`建帧`/`write_fixture_catalog`/
    // `sample_character_data`），搬去 `tests/` 就够不着它们了。
    #[path = "../../render_bottom_rows_tests.rs"]
    mod bottom_rows_tests;

    fn write_fixture_catalog(dir: &Path) {
        std::fs::write(dir.join("zh-CN.ftl"), "hud-status-time-label = 时间\nhud-status-health-label = 生命\nhud-status-mana-label = 法力\nhud-status-fps-label = 帧率\nseason-spring-display_name = 春\nseason-summer-display_name = 夏\nseason-autumn-display_name = 秋\nseason-winter-display_name = 冬\nhud-character-panel-title = 角色\nhud-character-level-label = 等级\nhud-character-experience-label = 经验\nhud-character-modifiers-title = 生效中的属性修正\nhud-character-modifiers-empty = 无\nhud-character-rule-modifiers-title = 生效中的规则修正\nhud-character-rule-modifiers-empty = 无\nattribute-strength-display_name = 力量\nattribute-dexterity-display_name = 敏捷\nattribute-constitution-display_name = 体质\nattribute-intelligence-display_name = 智力\nattribute-willpower-display_name = 意志\nattribute-charisma-display_name = 魅力\nattribute-luck-display_name = 幸运\nhud-inventory-panel-title = 背包\nhud-inventory-empty = （空）\nhud-inventory-durability-label = 耐久\nhud-equipment-panel-title = 装备\nhud-equipment-empty-slot = （空）\nequip_slot-main_hand-display_name = 主手\nequip_slot-off_hand-display_name = 副手\nequip_slot-head-display_name = 头部\nequip_slot-face-display_name = 面部\nequip_slot-eyes-display_name = 眼部\nequip_slot-neck-display_name = 颈部\nequip_slot-body-display_name = 躯干\nequip_slot-outer-display_name = 外袍\nequip_slot-back-display_name = 背部\nequip_slot-shoulder_l-display_name = 左肩\nequip_slot-shoulder_r-display_name = 右肩\nequip_slot-arm_l-display_name = 左臂\nequip_slot-arm_r-display_name = 右臂\nequip_slot-hand_l-display_name = 左手\nequip_slot-hand_r-display_name = 右手\nequip_slot-belt-display_name = 腰带\nequip_slot-tasset-display_name = 腿甲\nequip_slot-legs-display_name = 双腿\nequip_slot-boot_l-display_name = 左靴\nequip_slot-boot_r-display_name = 右靴\nequip_slot-ring_l-display_name = 左戒指\nequip_slot-ring_r-display_name = 右戒指\n").expect("测试用写入应当成功");
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
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            primary_attribute: None,
            now: Tick(0),
            // 规则修正：本文件的测试只关心几何（面板块数、条形块数），
            // 不关心面板里写了什么字，空切片就够——非空的渲染由
            // `character_panel` 自己的测试覆盖。
            rule_modifiers: &[],
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
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        // Act
        let frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            1280.0,
            None,
        );

        // Assert：四块面板各 9 块背景 + 双层条 2*3 块 + 单层条 2 块 +
        // 昼夜滑条 2 块（整条背景 + 指针）。
        assert_eq!(frame.layer(UiLayer::Hud).quads.len(), 4 * 9 + 2 * 3 + 2 + 2);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 世界地图为none时不产出任何世界地图矩形() {
        // 程序化验证「地图关着不画」：与上一条测试同样的输入,唯一区别
        // 是 world_map 传 None,矩形总数应该恰好回落到没有世界地图批次
        // 之前的既有数字——见上一条测试的算式说明。
        // Arrange
        let dir = temp_dir("world-map-closed");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        // Act
        let frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            1280.0,
            None,
        );

        // Assert
        assert_eq!(frame.layer(UiLayer::Hud).quads.len(), 4 * 9 + 2 * 3 + 2 + 2);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 世界地图为some时产出的矩形数比none时多出地图本身的矩形() {
        // 程序化验证「地图开着真的被加入渲染帧」——这是 M 键切换验收
        // 两层里的第一层（另一层是实机截图，见 `ll_game::app` 模块
        // 文档），不依赖任何合成按键事件,只断言 build_hud_frame 的
        // 纯函数产出。
        // Arrange
        let dir = temp_dir("world-map-open");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();
        let (terrain_ids, _terrain_table) = ll_world::terrain::base_terrain_fixture();
        let cells = [
            ll_world::overview::OverviewCell {
                terrain: terrain_ids.grass,
                explored: true,
            },
            ll_world::overview::OverviewCell {
                terrain: terrain_ids.deep_water,
                explored: false,
            },
        ];
        let world_map_data = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 1,
            terrain_ids: &terrain_ids,
            player: None,
            sites: &[],
            tiles_per_cell: 0,
        };

        // Act
        let closed_frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            1280.0,
            None,
        );
        let open_frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            1,
            1280.0,
            Some(&world_map_data),
        );

        // Assert：世界地图边框恒 4 块（见
        // `crate::hud::world_map::border_only_quads` 文档）加上本例
        // 2 个格子，共 6 块——全部落在覆盖层，常驻 HUD 那一层一块不多。
        assert_eq!(open_frame.layer(UiLayer::Overlay).quads.len(), 6);
        assert!(closed_frame.layer(UiLayer::Overlay).quads.is_empty());
        assert_eq!(
            open_frame.layer(UiLayer::Hud).quads.len(),
            closed_frame.layer(UiLayer::Hud).quads.len()
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_hud_frame对纯色皮肤产出的贴图矩形恒为空() {
        // Arrange
        let dir = temp_dir("textured-quad-empty");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        // Act
        let frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            1280.0,
            None,
        );

        // Assert
        assert!(frame.layer(UiLayer::Hud).textured_quads.is_empty());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_hud_frame产出的文本行数与四块面板行数之和一致() {
        // Arrange
        let dir = temp_dir("label-count");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        // Act
        let frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            1280.0,
            None,
        );

        // Assert：昼夜滑条不产出文本行（时间数字仍然只在状态栏那一行
        // 里,见 `crate::hud::status_bar` 模块文档「昼夜滑条指针位置」
        // 一节),四块面板的行数之和不受影响。角色面板的行数演进：11 行
        // （标题 1 + 六项属性 6 + 等级 1 + 经验 1 + 修正标题 1 +
        // 「无」占位 1）→ 幸运并入 `AttributeKind` 批次多一行属性 →
        // 12 行 → 升级加点批次再多两行（属性点余额、技能点余额,两者
        // 恒常显示即便为零,见 `character_panel` 里那段注释）→ 14 行
        // → 规则修正批次再多两行（段落标题 + 「无」占位,同样恒常显示,
        // 理由同上）→ 16 行。本用例的 `sample_character_data` 把
        // `rule_modifiers` 留成空切片,因此这里数到的是「无」那一行,
        // 不是九条修正各一行。
        // **主属性倾向那一行不在其中**：本用例的
        // `sample_character_data` 把 `primary_attribute` 留成 `None`
        // （查不到职业定义），整行不出现——这条断言同时是那个分支的
        // 覆盖。有职业时那一行出现的证据在
        // `crate::hud::character_panel` 的对应测试。
        assert_eq!(frame.layer(UiLayer::Hud).labels.len(), 1 + 16 + 2 + 23);

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
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();
        let full_status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        建帧(
            &full_status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            1280.0,
            None,
        );

        // Act：紧接着下一帧,生命值已经掉到 30。
        let damaged_status = StatusBarData {
            clock: Tick(0),
            health: 30,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let frame = 建帧(
            &damaged_status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            1,
            1280.0,
            None,
        );

        // Assert：状态栏文本行里应该已经是 30,不是 100 附近的过渡值。
        let status_line = &frame.layer(UiLayer::Hud).labels[0].text;
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
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();
        let screen_width = 1280.0;

        // Act
        let frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            screen_width,
            None,
        );

        // Assert：`build_hud_frame` 按固定顺序推入面板/条形——状态栏
        // （9 块）、生命条（3）、法力条（3）、昼夜滑条（2）、角色面板
        // （9）、经验条（2）、背包面板（9），装备栏是第 8 个、也是最后
        // 一个推入的面板，前面共 9+3+3+2+9+2+9=37 块。装备栏九宫格的
        // 第一块（左上角）位置即等于它的 `origin`，直接核实这一块的
        // x 坐标等于 [`equipment_origin_x`]。
        let equipment_first_quad = &frame.layer(UiLayer::Hud).quads[37];
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
    fn build_frame_for_inventory_layout_test(dir_name: &str) -> (LayeredFrame, std::path::PathBuf) {
        let dir = temp_dir(dir_name);
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        let frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            1280.0,
            None,
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
        let hud = frame.layer(UiLayer::Hud);
        assert_eq!(hud.quads[17].position[0], hud.quads[28].position[0]);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 背包面板位于角色面板下方() {
        // 见上一条测试的索引说明。
        // Arrange & Act
        let (frame, dir) = build_frame_for_inventory_layout_test("inventory-below");

        // Assert
        let hud = frame.layer(UiLayer::Hud);
        assert!(hud.quads[28].position[1] > hud.quads[17].position[1]);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 动作菜单的位置（所有者裁定：交互窗口居中） ──────────────────

    /// 造一块给位置测试用的菜单数据——内容无关紧要，只要行数固定。
    fn placement_menu(rows: &[String], placement: MenuPlacement) -> ActionMenuData<'_> {
        ActionMenuData {
            title_key: "menu-title",
            rows,
            cursor: 0,
            empty_key: "menu-empty",
            hint_key: "menu-hint",
            placement,
        }
    }

    /// 位置测试用的空目录目录——这几条只看几何，不看文字，键查不到时
    /// `Catalog::resolve` 退回键名本身，行数不变。
    fn placement_catalog(name: &str) -> (std::path::PathBuf, Catalog) {
        let dir = temp_dir(name);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        (dir, catalog)
    }

    #[test]
    fn 交互菜单在屏幕上水平垂直都居中() {
        // **所有者裁定**：「那个互动显示的 UI 窗口，我希望是出现在屏幕
        // 正中间」。
        //
        // 故意改坏的反例（人工核验）：把 `placed_action_menu` 里
        // `MenuPlacement::ScreenCenter` 那一支改成直接返回 `panel`
        // （退回旧的贴上沿行为），本条当场变红。
        // Arrange
        let (dir, catalog) = placement_catalog("interact-centered");
        let rows: Vec<String> = (0..4).map(|n| format!("行{n}")).collect();
        let menu = placement_menu(&rows, MenuPlacement::ScreenCenter);
        let (width, height) = (1280.0_f32, 720.0_f32);

        // Act
        let panel = placed_action_menu(
            &menu,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            width,
            height,
        );

        // Assert：面板中心与屏幕中心重合（浮点，允许半像素）。
        let center_x = panel.rect.x + panel.rect.width * 0.5;
        let center_y = panel.rect.y + panel.rect.height * 0.5;
        assert!(
            (center_x - width * 0.5).abs() < 0.5,
            "水平未居中：面板中心 {center_x}，屏幕中心 {}",
            width * 0.5
        );
        assert!(
            (center_y - height * 0.5).abs() < 0.5,
            "垂直未居中：面板中心 {center_y}，屏幕中心 {}",
            height * 0.5
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 交互菜单在任意窗口尺寸下都居中() {
        // 不许把 1280×720 写死——真实窗口可以是任意尺寸。三种差别很大的
        // 尺寸各算一次。
        //
        // 故意改坏的反例（人工核验）：把 `placed_action_menu` 里的
        // `screen_height` 换成字面量 `720.0`，第一与第三组当场变红。
        // Arrange
        let (dir, catalog) = placement_catalog("interact-any-size");
        let rows: Vec<String> = (0..3).map(|n| format!("行{n}")).collect();
        let menu = placement_menu(&rows, MenuPlacement::ScreenCenter);

        for (width, height) in [(800.0_f32, 600.0_f32), (1280.0, 720.0), (2560.0, 1440.0)] {
            // Act
            let panel = placed_action_menu(
                &menu,
                &catalog,
                "zh-CN",
                &mut crate::测试测量器(),
                width,
                height,
            );

            // Assert
            let center_y = panel.rect.y + panel.rect.height * 0.5;
            assert!(
                (center_y - height * 0.5).abs() < 0.5,
                "{width}×{height} 下垂直未居中：面板中心 {center_y}"
            );
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 背包与制作菜单原位不动() {
        // **所有者只要求交互那一块居中。** 三块菜单共用同一条渲染路径，
        // 这条守的是「本次改动没有把另外两块一并挪走」——它们仍然贴着
        // 屏幕上沿，与改动之前逐像素相同。
        //
        // 故意改坏的反例（人工核验）：把 `placed_action_menu` 改成无视
        // `menu.placement` 一律居中，本条当场变红。
        // Arrange
        let (dir, catalog) = placement_catalog("top-center-unmoved");
        let rows: Vec<String> = (0..4).map(|n| format!("行{n}")).collect();
        let menu = placement_menu(&rows, MenuPlacement::TopCenter);

        // Act
        let panel = placed_action_menu(
            &menu,
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert：贴上沿的那个 y 是本次改动之前唯一存在的取值。
        assert_eq!(panel.rect.y, SCREEN_MARGIN + PANEL_GAP);
        assert_eq!(panel.rect.x, (1280.0 - ACTION_MENU_WIDTH) * 0.5);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 居中之后每一行文字跟着面板一起挪() {
        // 平移必须是**整体**的：只挪背景矩形而不挪文字，玩家会看到一块
        // 空面板加一列悬空的字。
        //
        // 故意改坏的反例（人工核验）：把 `translate_panel` 里那个
        // `for label in &mut panel.labels` 循环删掉，本条当场变红。
        // Arrange
        let (dir, catalog) = placement_catalog("labels-follow");
        let rows: Vec<String> = (0..4).map(|n| format!("行{n}")).collect();
        let top = placed_action_menu(
            &placement_menu(&rows, MenuPlacement::TopCenter),
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Act
        let centered = placed_action_menu(
            &placement_menu(&rows, MenuPlacement::ScreenCenter),
            &catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert：每一行相对面板顶部的偏移逐条不变。
        let dy = centered.rect.y - top.rect.y;
        assert!(dy.abs() > 1.0, "两种摆法应当真的落在不同的高度");
        assert_eq!(centered.labels.len(), top.labels.len());
        for (moved, original) in centered.labels.iter().zip(top.labels.iter()) {
            assert_eq!(moved.x, original.x, "水平位置不该变");
            assert!(
                (moved.y - (original.y + dy)).abs() < 0.001,
                "文字没有跟着面板一起挪：{} 应当是 {}",
                moved.y,
                original.y + dy
            );
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 一套「贴图全部查得到」的假皮肤——真实运行期
    /// (`crate::widget::skin::NineSliceSkin` 配上完整的 `assets/`) 就是
    /// 这个形状：常驻 HUD 的面板与条形全部走**贴图**批次，而世界地图
    /// 恒只产出纯色矩形。
    ///
    /// 下面两条遮挡测试必须用它而不是
    /// [`crate::widget::skin::FlatColorSkin`]：纯色皮肤下所有东西都落
    /// 进同一个容器，跨批次的遮挡问题**根本不会出现**——所有者实机
    /// 撞到的那条缺陷，正因为仓库里既有的 HUD 测试全都跑在纯色皮肤上
    /// 而一路绿着。
    struct AllTexturedSkin;

    /// 假图集里昼夜滑条底图的 UV——与滑块取不同的值，测试据此分辨
    /// 「这一块是底图还是滑块」。
    const FAKE_DAYNIGHT_TRACK_UV: [f32; 4] = [0.5, 0.5, 0.25, 0.25];
    /// 假图集里昼夜滑条滑块的 UV，理由同 [`FAKE_DAYNIGHT_TRACK_UV`]。
    const FAKE_DAYNIGHT_POINTER_UV: [f32; 4] = [0.75, 0.5, 0.05, 0.25];

    impl Skin for AllTexturedSkin {
        fn panel(&self, style: PanelStyleId) -> crate::widget::panel::FlatPanelAppearance {
            FlatColorSkin.panel(style)
        }

        fn bar(&self, style: BarStyleId) -> crate::widget::bar::FlatBarAppearance {
            FlatColorSkin.bar(style)
        }

        fn day_night_bar(
            &self,
            style: DayNightBarStyleId,
        ) -> crate::widget::day_night_bar::FlatDayNightBarAppearance {
            FlatColorSkin.day_night_bar(style)
        }

        fn button(
            &self,
            style: crate::widget::skin::ButtonStyleId,
            visual: crate::widget::skin::ButtonVisualState,
        ) -> crate::widget::button::FlatButtonAppearance {
            FlatColorSkin.button(style, visual)
        }

        fn textured_panel(
            &self,
            _style: PanelStyleId,
        ) -> Option<crate::widget::panel::TexturedPanelAppearance> {
            Some(crate::widget::panel::TexturedPanelAppearance {
                border_uv: [0.0, 0.0, 0.1, 0.1],
                fill_uv: [0.1, 0.0, 0.1, 0.1],
                border_tint: [1.0, 1.0, 1.0, 1.0],
                fill_tint: [1.0, 1.0, 1.0, 1.0],
                border_thickness: 4.0,
            })
        }

        fn textured_bar(
            &self,
            style: BarStyleId,
        ) -> Option<crate::widget::bar::TexturedBarAppearance> {
            match style {
                BarStyleId::Progress => Some(crate::widget::bar::TexturedBarAppearance {
                    track_uv: [0.2, 0.0, 0.1, 0.1],
                    fill_uv: [0.3, 0.0, 0.1, 0.1],
                    track_tint: [1.0, 1.0, 1.0, 1.0],
                    fill_tint: [1.0, 1.0, 1.0, 1.0],
                }),
                BarStyleId::Health | BarStyleId::Mana => None,
            }
        }

        fn textured_two_layer_bar(
            &self,
            style: BarStyleId,
        ) -> Option<crate::widget::bar::TexturedTwoLayerBarAppearance> {
            match style {
                BarStyleId::Progress => None,
                _ => Some(crate::widget::bar::TexturedTwoLayerBarAppearance {
                    track_uv: [0.2, 0.0, 0.1, 0.1],
                    fill_uv: [0.3, 0.0, 0.1, 0.1],
                    track_tint: [1.0, 1.0, 1.0, 1.0],
                    afterglow_tint: [0.8, 0.8, 0.8, 1.0],
                    fill_tint: [1.0, 1.0, 1.0, 1.0],
                }),
            }
        }

        fn textured_day_night_bar(
            &self,
            _style: DayNightBarStyleId,
        ) -> Option<crate::widget::day_night_bar::TexturedDayNightBarAppearance> {
            Some(
                crate::widget::day_night_bar::TexturedDayNightBarAppearance {
                    track_uv: FAKE_DAYNIGHT_TRACK_UV,
                    track_tint: [1.0, 1.0, 1.0, 1.0],
                    pointer_uv: FAKE_DAYNIGHT_POINTER_UV,
                    pointer_tint: [1.0, 1.0, 1.0, 1.0],
                },
            )
        }
    }

    /// 建一帧「贴图皮肤 + 世界地图打开」的 HUD——本模块两条遮挡测试
    /// 共用。地图那两格**刻意都未探索**，因此格子恒是
    /// `crate::hud::world_map::FOG_COLOR`，在整帧里独一无二，测试据此
    /// 认出「哪一批是地图」而不必去数下标。
    fn build_textured_frame_with_map(dir_name: &str) -> (LayeredFrame, std::path::PathBuf) {
        let dir = temp_dir(dir_name);
        write_fixture_catalog(&dir);
        let existing = std::fs::read_to_string(dir.join("zh-CN.ftl")).expect("夹具应当已写入");
        std::fs::write(
            dir.join("zh-CN.ftl"),
            format!("{existing}hud-world-map-scale-label = 比例尺\nhud-world-map-hint = 提示\n"),
        )
        .expect("测试用写入应当成功");
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let cells = [
            ll_world::overview::OverviewCell {
                terrain: ids.grass,
                explored: false,
            },
            ll_world::overview::OverviewCell {
                terrain: ids.grass,
                explored: false,
            },
        ];
        let map = WorldMapPanelData {
            cells: &cells,
            cols: 2,
            rows: 1,
            player: Some((0, 0)),
            sites: &[],
            terrain_ids: &ids,
            tiles_per_cell: 48,
        };
        let frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &AllTexturedSkin,
            &mut anim,
            0,
            1280.0,
            Some(&map),
        );
        (frame, dir)
    }

    /// 本帧里「哪一批是世界地图」——按迷雾色认，见
    /// [`build_textured_frame_with_map`]。
    fn map_batch_index(batches: &[DrawBatch<'_>]) -> usize {
        batches
            .iter()
            .position(|batch| match batch {
                DrawBatch::Quads(quads) => {
                    quads.iter().any(|quad| quad.color == world_map::FOG_COLOR)
                }
                _ => false,
            })
            .expect("世界地图的迷雾格必须出现在某一批纯色矩形里")
    }

    #[test]
    fn 世界地图的绘制批次恒排在常驻hud的全部批次之后() {
        // 所有者实机反馈「血条之类的 UI 会覆盖地图」的直接回归。判据走
        // 数据层（`draw_batches` 就是 `render_hud` 逐条提交的那个序列），
        // 不走合成按键（ADR 0025）。
        // Arrange
        let (frame, dir) = build_textured_frame_with_map("layer-order-map");

        // Act
        let batches = frame.draw_batches();
        let map_index = map_batch_index(&batches);
        let last_hud_index = batches
            .iter()
            .rposition(|batch| matches!(batch, DrawBatch::Textured(_)))
            .expect("贴图皮肤下常驻 HUD 的面板与条形必须落进贴图批次");

        // Assert：地图那一批严格排在常驻 HUD 最后一批之后。
        assert!(
            map_index > last_hud_index,
            "世界地图排在第 {map_index} 批，常驻 HUD 最后一批是第 {last_hud_index} 批——地图被 HUD 盖住了"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 常驻hud的文本行也排在世界地图之前() {
        // 文本此前是「全屏最后一道 pass」，因此角色/背包面板的每一行字
        // 都会浮在地图上面。分层之后文本是**层内**最后一道，这条钉住它
        // 不再回到全屏级别。
        // Arrange
        let (frame, dir) = build_textured_frame_with_map("layer-order-labels");

        // Act
        let batches = frame.draw_batches();
        let map_index = map_batch_index(&batches);
        let hud_label_index = batches
            .iter()
            .position(|batch| match batch {
                DrawBatch::Labels(labels) => labels.iter().any(|label| label.text.contains("生命")),
                _ => false,
            })
            .expect("状态栏那一行必须出现在某一批文本里");

        // Assert
        assert!(
            hud_label_index < map_index,
            "常驻 HUD 的文本排在第 {hud_label_index} 批，世界地图第 {map_index} 批——HUD 文字浮在地图上面"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 贴图皮肤下昼夜滑块与底图同批且排在底图之后() {
        // 所有者实机反馈「时间调，少了滑条……目前只显示了背景条」的直接
        // 回归：滑块此前是纯色矩形，落进层内更早的那一道 pass，被贴图
        // 底图整个盖住。判据走数据层，不走截图（ADR 0025）。
        // Arrange
        let (frame, dir) = build_textured_frame_with_map("daynight-pointer-order");

        // Act
        let textured = &frame.layer(UiLayer::Hud).textured_quads;
        let track_index = textured
            .iter()
            .position(|quad| quad.uv_rect == FAKE_DAYNIGHT_TRACK_UV)
            .expect("贴图皮肤下昼夜滑条底图必须落进贴图批次");
        let pointer_index = textured
            .iter()
            .position(|quad| quad.uv_rect == FAKE_DAYNIGHT_POINTER_UV)
            .expect("贴图皮肤下昼夜滑块必须与底图同在贴图批次——落进纯色批次就会被底图盖住");

        // Assert
        assert!(
            pointer_index > track_index,
            "滑块排在第 {pointer_index} 块、底图第 {track_index} 块——滑块被自己的底图盖住了"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 纯色皮肤下昼夜滑块与底图同批且排在底图之后() {
        // 纯色回退路径的同一条性质——它此前恰好是对的（两块都在
        // `quads` 里），钉住它别在将来某次改动里也被拆到两个容器。
        // Arrange
        let dir = temp_dir("daynight-pointer-flat");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir);
        let status = StatusBarData {
            clock: Tick(0),
            health: 100,
            mana: 50,
            fps: 0.0,
            weather_display_name_key: None,
        };
        let modifiers = BTreeMap::new();
        let equipment = BTreeMap::new();
        let character = sample_character_data(&modifiers, &equipment);
        let item_table = ItemTable::new();
        let mut anim = WidgetStateTable::new();

        // Act
        let frame = 建帧(
            &status,
            &character,
            &equipment,
            &item_table,
            &catalog,
            &FlatColorSkin,
            &mut anim,
            0,
            1280.0,
            None,
        );

        // Assert：底图整条、滑块只有 `POINTER_WIDTH` 宽，按宽度认。
        let appearance = FlatColorSkin.day_night_bar(DayNightBarStyleId::Clock);
        let quads = &frame.layer(UiLayer::Hud).quads;
        let track_index = quads
            .iter()
            .position(|quad| {
                quad.color == appearance.track_color && quad.size[0] == DAY_NIGHT_BAR_WIDTH
            })
            .expect("纯色回退下昼夜滑条底图必须在纯色批次里");
        let pointer_index = quads
            .iter()
            .position(|quad| {
                quad.color == appearance.pointer_color
                    && quad.size[0] == crate::widget::day_night_bar::POINTER_WIDTH
            })
            .expect("纯色回退下昼夜滑块必须在纯色批次里");
        assert!(pointer_index > track_index);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
