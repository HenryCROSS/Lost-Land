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
