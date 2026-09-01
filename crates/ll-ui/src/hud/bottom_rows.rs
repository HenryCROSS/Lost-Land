//! 屏幕底部那两行小面板：**反馈行**与**按键提示行**。
//!
//! # 为什么它们住在一起
//!
//! 两者形状完全相同——一句已经排好版的话、一块水平居中、贴着屏幕下沿
//! 的单行面板。此前反馈行那一段直接写在 [`super::render::build_hud_frame`]
//! 里；按键提示行（规格 F6）落地时若照抄一遍，那个函数就会有两段几乎
//! 逐字相同的代码，而它已经是本仓库行数棘轮快照里的文件。搬出来之后
//! 两者共用同一个 [`bottom_row_panel`]，`build_hud_frame` 那一侧只剩
//! 两次调用。
//!
//! # 两行分在两个层，不是同一层
//!
//! | 行 | 层 | 为什么 |
//! |---|---|---|
//! | 反馈行 | [`UiLayer::Notice`] | 它要说的正是「你刚才那一下没起作用」，被任何面板挡住就等于没说 |
//! | 按键提示行 | [`UiLayer::Hud`] | 它是常驻教学，被弹窗/地图盖住是**对的**——那时候玩家看的是别的东西，而那些面板自己带着自己的提示行 |
//!
//! 规格 §9.3 F6 原文写的是「`Hud` 层底部加一行常驻提示」，没有说层；
//! 这里把它与反馈行的分层关系明确下来，记在批次 23 计划文档第八节。
//!
//! # 两行的纵向次序
//!
//! 按键提示行贴着最下沿，反馈行叠在它**上面**一格。此前反馈行自己贴在
//! 下沿（`FEEDBACK_BOTTOM_MARGIN` = 48），提示行落地后两块会重叠——
//! 于是反馈行往上让了一格。让的是反馈行而不是提示行：提示行是常驻的，
//! 位置固定在最下沿玩家才会把它当成「窗台上的一行小字」而不是一条会
//! 跳来跳去的通知。

use crate::widget::layer::LayerBatch;
use crate::widget::skin::Skin;

use super::{PanelContent, build_panel};

/// 反馈行面板宽度——一句话的宽度，见
/// [`super::render::build_hud_frame`] 的 `feedback` 参数文档。
pub const FEEDBACK_WIDTH: f32 = 420.0;

/// 按键提示行面板宽度。
///
/// # 这个数从哪来
///
/// 它是**这一行在两种语言下都排得进一行**所需要的宽度，由
/// `ll_game::key_hint` 的那条断言实测守着（英文那一行最长）。取 620 而
/// 不是刚好够用：620 与状态栏 [`super::render::STATUS_WIDTH`] 同宽，
/// 屏幕上下两条通栏因此左右边界对齐。
///
/// **它不按内容伸缩**，与六块常驻 HUD 面板同一条批次 19 的取舍：伸缩
/// 会让面板宽度随玩家重绑键位而跳变。这一行的内容确实变得比物品名慢
/// 得多，但「宽度固定 + 一条实测断言」比「每帧现算」便宜也更稳。
pub const KEY_HINT_WIDTH: f32 = 620.0;

/// 单行面板的高度：一行文字加上下内边距。两个 `BOTTOM_MARGIN` 都从它
/// 派生，不各写一个魔数。
const ROW_PANEL_HEIGHT: f32 = super::DEFAULT_LINE_HEIGHT + super::DEFAULT_PADDING * 2.0;

/// 面板与窗口下边缘的留白。
const BOTTOM_MARGIN: f32 = 16.0;

/// 按键提示行的顶边距下沿多远——它贴着最下沿。
const KEY_HINT_BOTTOM_OFFSET: f32 = BOTTOM_MARGIN + ROW_PANEL_HEIGHT;

/// 反馈行的顶边距下沿多远——它叠在提示行**上面**一格，见模块文档。
const FEEDBACK_BOTTOM_OFFSET: f32 =
    KEY_HINT_BOTTOM_OFFSET + ROW_PANEL_HEIGHT + super::DEFAULT_PADDING;

/// 排一块「水平居中、距下沿 `bottom_offset`」的单行小面板。
///
/// 文字仍然可能排成多行（超长翻译），那时面板会自己长高——高度走
/// [`build_panel`] 的实测行数，与批次 19 的 W2 同一条链路，不按「一行」
/// 写死。
fn bottom_row_panel(
    measure: &mut dyn ll_text::MeasureText,
    text: &str,
    width: f32,
    bottom_offset: f32,
    screen_width: f32,
    screen_height: f32,
) -> PanelContent {
    build_panel(
        measure,
        (
            (screen_width - width) * 0.5,
            (screen_height - bottom_offset).max(0.0),
        ),
        width,
        |cursor, lines| cursor.push(lines, text.to_string()),
    )
}

/// 反馈行：一句「你刚才那一下没起作用」，压在所有东西之上。
pub(super) fn push_feedback_row(
    batch: &mut LayerBatch,
    measure: &mut dyn ll_text::MeasureText,
    skin: &dyn Skin,
    text: &str,
    screen_width: f32,
    screen_height: f32,
) {
    let panel = bottom_row_panel(
        measure,
        text,
        FEEDBACK_WIDTH,
        FEEDBACK_BOTTOM_OFFSET,
        screen_width,
        screen_height,
    );
    super::render::push_panel(batch, &panel.rect, panel.labels, skin);
}

/// 按键提示行：常驻，贴着屏幕最下沿。
pub(super) fn push_key_hint_row(
    batch: &mut LayerBatch,
    measure: &mut dyn ll_text::MeasureText,
    skin: &dyn Skin,
    text: &str,
    screen_width: f32,
    screen_height: f32,
) {
    let panel = bottom_row_panel(
        measure,
        text,
        KEY_HINT_WIDTH,
        KEY_HINT_BOTTOM_OFFSET,
        screen_width,
        screen_height,
    );
    super::render::push_panel(batch, &panel.rect, panel.labels, skin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 两行互不重叠且都在窗口内() {
        // 「贴着下沿」与「叠在上面一格」这两句话的算术。改动任何一个
        // 常量都会在这里显形。
        //
        // 反例（已实跑）：把 `FEEDBACK_BOTTOM_OFFSET` 改回
        // `KEY_HINT_BOTTOM_OFFSET`，本条红在「反馈行底边不越过提示行
        // 顶边」。
        // Arrange
        let mut measure = ll_text::TextMeasurer::new().expect("内置字体资产应能正常解析");
        let (w, h) = (1280.0, 720.0);

        // Act
        let hint = bottom_row_panel(
            &mut measure,
            "提示",
            KEY_HINT_WIDTH,
            KEY_HINT_BOTTOM_OFFSET,
            w,
            h,
        );
        let feedback = bottom_row_panel(
            &mut measure,
            "反馈",
            FEEDBACK_WIDTH,
            FEEDBACK_BOTTOM_OFFSET,
            w,
            h,
        );

        // Assert
        assert!(
            hint.rect.y + hint.rect.height <= h,
            "提示行不该掉出窗口下沿"
        );
        assert!(
            feedback.rect.y + feedback.rect.height <= hint.rect.y,
            "反馈行的底边不该越过提示行的顶边"
        );
    }
}
