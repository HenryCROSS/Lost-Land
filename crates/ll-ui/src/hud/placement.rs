//! 两块**不按固定分区平铺**的面板该画在哪：动作菜单的两种落位。
//!
//! # 为什么从 [`super::render`] 里搬出来
//!
//! `render.rs` 在本仓库行数棘轮的快照里（`scripts/ci/file_size_budget.json`），
//! 而它承担的其实是两件事：**一张固定分区的平铺表**（六块常驻面板各在
//! 屏幕的哪一角）与**若干块要现算落位的浮动面板**。前者是那个文件的主线，
//! 后者是一段自成一体的几何运算。批次 23 往底部加一行常驻提示时，把后者
//! 搬了出来——所有者要的方向是「重构让代码为之后的开发做准备」，不是为了
//! 压行数而砍。
//!
//! 屏幕底部那两行小面板（反馈行 / 按键提示行）同期搬进了
//! [`super::bottom_rows`]，两者分界：本模块管**居中/浮动**的落位，
//! `bottom_rows` 管**贴着下沿**的那两行。

use super::PanelContent;
use crate::widget::geometry::Rect;
use super::action_menu::{self, ActionMenuData, MenuPlacement};
use super::render::{ACTION_MENU_WIDTH, PANEL_GAP, SCREEN_MARGIN};
use ll_i18n::Catalog;

/// # 屏幕比面板还矮时
///
/// `ScreenCenter` 算出来的 `y` 会是负数，面板顶部被截在屏幕外——**这是
/// 刻意不钳制的**：钳到 0 会让面板底部改为超出屏幕，同样看不全，却把
/// 「窗口比屏幕高」这个真问题藏起来。与
/// `ActionMenuData::cursor` 越界时「不钳制、也不 panic」同一条纪律。
pub(super) fn placed_action_menu(
    menu: &ActionMenuData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn ll_text::MeasureText,
    screen_width: f32,
    screen_height: f32,
) -> PanelContent {
    let x = (screen_width - ACTION_MENU_WIDTH) * 0.5;
    let top = SCREEN_MARGIN + PANEL_GAP;
    let panel = action_menu::action_menu_panel(
        menu,
        catalog,
        language,
        measure,
        (x, top),
        ACTION_MENU_WIDTH,
    );
    match menu.placement {
        MenuPlacement::TopCenter => panel,
        MenuPlacement::ScreenCenter => {
            let centered_top = (screen_height - panel.rect.height) * 0.5;
            translate_panel(panel, centered_top - top)
        }
    }
}

/// 把一块已经建好的面板整体沿 y 轴平移 `dy` 像素——背景矩形与每一行
/// 文字一起动，见 [`placed_action_menu`] 文档「先建一次、再整体平移」。
fn translate_panel(mut panel: PanelContent, dy: f32) -> PanelContent {
    panel.rect.y += dy;
    for label in &mut panel.labels {
        label.y += dy;
    }
    panel
}

/// 世界地图比例尺文案与面板边框的留白（像素）——比
/// [`SCREEN_MARGIN`](super::render::SCREEN_MARGIN) 小：这行字贴在地图**内侧**，留白太大就会压到
/// 第一行格子上。
pub(crate) const WORLD_MAP_CAPTION_MARGIN: f32 = 8.0;

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
/// 按这块菜单声明的 [`MenuPlacement`](super::action_menu::MenuPlacement) 把它摆到屏幕上。
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
