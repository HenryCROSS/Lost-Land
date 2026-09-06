//! 屏幕底部那三行小面板：**反馈行**、**按键提示行**、**自动存档的痕迹**。
//!
//! # 为什么它们住在一起
//!
//! 三者形状完全相同——一句已经排好版的话、一块水平居中、贴着屏幕下沿
//! 的单行面板。此前反馈行那一段直接写在 [`super::render::build_hud_frame`]
//! 里；按键提示行（规格 F6）落地时若照抄一遍，那个函数就会有两段几乎
//! 逐字相同的代码，而它已经是本仓库行数棘轮快照里的文件。搬出来之后
//! 三者共用同一个 [`bottom_row_panel`]，`build_hud_frame` 那一侧只剩
//! 三次调用。
//!
//! # 三行分在两个层，不是同一层
//!
//! | 行 | 层 | 为什么 |
//! |---|---|---|
//! | 反馈行 | [`UiLayer::Notice`] | 它要说的正是「你刚才那一下没起作用」，被任何面板挡住就等于没说 |
//! | 自动存档痕迹 | [`UiLayer::Notice`] | 同上：一次性通告，说完自己消失 |
//! | 按键提示行 | [`UiLayer::Hud`] | 它是常驻教学，被弹窗/地图盖住是**对的**——那时候玩家看的是别的东西，而那些面板自己带着自己的提示行 |
//!
//! 规格 §9.3 F6 原文写的是「`Hud` 层底部加一行常驻提示」，没有说层；
//! 这里把它与反馈行的分层关系明确下来，记在批次 23 计划文档第八节。
//!
//! # 三行的纵向次序
//!
//! 按键提示行贴着最下沿，反馈行叠在它**上面**一格，自动存档的痕迹再叠
//! 在反馈行上面一格。此前反馈行自己贴在下沿
//! （`FEEDBACK_BOTTOM_MARGIN` = 48），提示行落地后两块会重叠——
//! 于是反馈行往上让了一格。让的是反馈行而不是提示行：提示行是常驻的，
//! 位置固定在最下沿玩家才会把它当成「窗台上的一行小字」而不是一条会
//! 跳来跳去的通知。规格 F3 的那条痕迹（批次 35）按同一条理由再往上让
//! 一格：反馈行说的是「你刚那一下没起作用」，玩家正等着看它，不该被
//! 一条背景动作挤走。

use crate::widget::layer::{LayerBatch, LayeredFrame, UiLayer};
use crate::widget::skin::Skin;

use super::{PanelContent, build_panel};

/// 反馈行面板宽度——一句话的宽度，见
/// [`super::render::build_hud_frame`] 的 `feedback` 参数文档。
pub const FEEDBACK_WIDTH: f32 = 420.0;

/// 自动存档痕迹那一行的面板宽度。
///
/// 比另外两行都窄：它只有一句四五个字的话（规格 §9.2 F3 的原话是
/// 「小字」），而且它是**背景动作的回执**，不是要玩家读的东西——面板
/// 越小越不打扰人。具体这个数由溢出门禁
/// （`crates/ll-ui/tests/i18n_text_width.rs`）实测守着，两种语言都要
/// 排得进一行。
pub const AUTOSAVE_WIDTH: f32 = 220.0;

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

/// 按键提示行的**底边**距窗口下沿多远——它贴着最下沿，走全局的屏幕
/// 边距刻度（规格 L3），不再自己定义一个同值的 `BOTTOM_MARGIN`。
const KEY_HINT_BOTTOM_MARGIN: f32 = super::render::SCREEN_MARGIN;

/// 反馈行的**底边**距窗口下沿多远——它叠在提示行**上面**一格，见模块
/// 文档。
///
/// 两行之间那一格此前用的是 `DEFAULT_PADDING`（把**内边距**当**间隔**
/// 使），规格 L3 要求间距只有 `SCREEN_MARGIN`/`PANEL_GAP` 两档，改走
/// `PANEL_GAP`——反馈行因此比收敛前上移 4px。
const FEEDBACK_BOTTOM_MARGIN: f32 =
    KEY_HINT_BOTTOM_MARGIN + ROW_PANEL_HEIGHT + super::render::PANEL_GAP;

/// 自动存档痕迹的**底边**距窗口下沿多远——它再叠在反馈行上面一格，
/// 见模块文档「三行的纵向次序」。同样由下一行的偏移派生，不写死。
const AUTOSAVE_BOTTOM_MARGIN: f32 =
    FEEDBACK_BOTTOM_MARGIN + ROW_PANEL_HEIGHT + super::render::PANEL_GAP;

/// 屏幕最下沿那一条**底栏**有多高——规格 L1 的中段留白规则在这一条
/// 窄边上开的例外（见 `hud/render_layout_tests.rs` 那条断言）。
///
/// 取「最靠上的那一行的顶边距下沿多远」，也就是自动存档痕迹那一行的
/// 偏移。**导出这个常量而不是让测试自己抄一个数**——三行的位置将来再
/// 动一次，判据跟着动，不会分叉。
///
/// 〔批次 35〕F3 那条痕迹加进来之后这个数变高了一格。它只放宽 L1 那条
/// 留白规则在**底栏**上的例外范围，而底栏里唯一的常驻区（`Hud` 层）
/// 成员仍然只有按键提示行一条——另外两行都在 `Notice` 层，本来就不
/// 参与那条断言。
pub const BOTTOM_STRIP_HEIGHT: f32 = AUTOSAVE_BOTTOM_MARGIN + ROW_PANEL_HEIGHT;

/// 这一帧底部三行各自要说的那句话，`None` = 这一行这一帧不显示。
///
/// 打成一个结构体而不是三个位置参数：三者类型相同
/// （`Option<&str>`），排成一列位置参数时**调用点传反了编译器不会说
/// 话**——而它们分属两个层、三个高度，传反了只会在屏幕上错位。
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct BottomRowTexts<'a> {
    /// 常驻按键提示（规格 F6），画在 [`UiLayer::Hud`]。
    pub key_hint: Option<&'a str>,
    /// 反馈行，画在 [`UiLayer::Notice`]。
    pub feedback: Option<&'a str>,
    /// 自动存档的痕迹（规格 F3），画在 [`UiLayer::Notice`]。
    pub autosave: Option<&'a str>,
}

/// 把这一帧要显示的底部行推进各自的层——**三行的分层与纵向次序全部
/// 住在本模块**，见模块文档那两节。
pub(super) fn push_bottom_rows(
    frame: &mut LayeredFrame,
    measure: &mut dyn ll_text::MeasureText,
    skin: &dyn Skin,
    texts: BottomRowTexts<'_>,
    screen_width: f32,
    screen_height: f32,
) {
    if let Some(text) = texts.key_hint {
        let batch = frame.layer_mut(UiLayer::Hud);
        push_key_hint_row(batch, measure, skin, text, screen_width, screen_height);
    }
    if let Some(text) = texts.feedback {
        let batch = frame.layer_mut(UiLayer::Notice);
        push_feedback_row(batch, measure, skin, text, screen_width, screen_height);
    }
    if let Some(text) = texts.autosave {
        let batch = frame.layer_mut(UiLayer::Notice);
        push_autosave_row(batch, measure, skin, text, screen_width, screen_height);
    }
}

/// 排一块「水平居中、距下沿 `bottom_offset`」的单行小面板。
///
/// 文字仍然可能排成多行（超长翻译），那时面板会自己长高——高度走
/// [`build_panel`] 的实测行数，与批次 19 的 W2 同一条链路，不按「一行」
/// 写死。
fn bottom_row_panel(
    measure: &mut dyn ll_text::MeasureText,
    text: &str,
    width: f32,
    bottom_margin: f32,
    screen_width: f32,
    screen_height: f32,
) -> PanelContent {
    // 规格 L2：居中与贴下沿这一份算术走 `Rect::anchored`。
    //
    // 高度传 `ROW_PANEL_HEIGHT`（一行的高）而不是这块面板最终的高度：
    // 面板高度是 `build_panel` 按实际行数现算出来的，而落位得先于它。
    // 这与收敛之前的行为逐字相同——旧写法里 `bottom_offset` 本来就是
    // `bottom_margin + ROW_PANEL_HEIGHT` 拼出来的常量。
    //
    // 旧写法对 y 做过 `.max(0.0)`，**收敛后取消**：另外四处都刻意不钳，
    // 且都写了理由（钳制会掩盖「窗口配置改小了却没人发现」）；这一处
    // 是五处里唯一钳的，且没写为什么。见 `Rect::anchored` 文档。
    let rect = crate::widget::geometry::Rect::anchored(
        (screen_width, screen_height),
        crate::widget::geometry::Anchor::BottomCenter,
        (width, ROW_PANEL_HEIGHT),
        bottom_margin,
    );
    build_panel(measure, rect.origin(), width, |cursor, lines| {
        cursor.push(lines, text.to_string())
    })
}

/// 反馈行：一句「你刚才那一下没起作用」，压在所有东西之上。
fn push_feedback_row(
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
        FEEDBACK_BOTTOM_MARGIN,
        screen_width,
        screen_height,
    );
    super::render::push_panel(batch, &panel.rect, panel.labels, skin);
}

/// 自动存档的痕迹（规格 F3）：一句「已自动保存」，叠在反馈行上面一格。
///
/// **只管画**——这一帧到底该不该画由 `ll_game::autosave_notice` 那个
/// 纯函数按帧计数决定，见那个模块的文档。
fn push_autosave_row(
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
        AUTOSAVE_WIDTH,
        AUTOSAVE_BOTTOM_MARGIN,
        screen_width,
        screen_height,
    );
    super::render::push_panel(batch, &panel.rect, panel.labels, skin);
}

/// 按键提示行：常驻，贴着屏幕最下沿。
fn push_key_hint_row(
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
        KEY_HINT_BOTTOM_MARGIN,
        screen_width,
        screen_height,
    );
    super::render::push_panel(batch, &panel.rect, panel.labels, skin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 底部两行改走anchored之后水平居中且底边距下沿恰好一个边距() {
        // 规格 L2 第 4 处的「改写前后逐像素相同」回归断言。旧写法是
        // `((screen_width - width) * 0.5, (screen_height - bottom_offset).max(0.0))`，
        // 其中 `bottom_offset = bottom_margin + ROW_PANEL_HEIGHT`。
        //
        // **`.max(0.0)` 收敛后取消**，见 `bottom_row_panel` 里那段注释与
        // `Rect::anchored` 文档「一律不钳制」。那一处钳制在 720 高的窗口
        // 上本来也不生效（它只在窗口比面板还矮时才有区别），因此这条
        // 逐像素断言不受影响。
        //
        // 反例验证（已实跑）：把 `Anchor::BottomCenter` 换成
        // `Anchor::TopCenter`，本条红在 y 上。
        // Arrange
        let mut measure = ll_text::TextMeasurer::new().expect("内置字体资产应能正常解析");
        let (w, h) = (1280.0_f32, 720.0_f32);

        // Act
        let hint = bottom_row_panel(
            &mut measure,
            "提示",
            KEY_HINT_WIDTH,
            KEY_HINT_BOTTOM_MARGIN,
            w,
            h,
        );

        // Assert：x 与旧算术逐像素相同；y 与旧的 `h - bottom_offset` 相同。
        assert_eq!(hint.rect.x, (w - KEY_HINT_WIDTH) * 0.5);
        assert_eq!(hint.rect.y, h - (KEY_HINT_BOTTOM_MARGIN + ROW_PANEL_HEIGHT));
    }

    #[test]
    fn 三行互不重叠且都在窗口内() {
        // 「贴着下沿」与「各叠在上面一格」这几句话的算术。改动任何一个
        // 常量都会在这里显形。
        //
        // 反例（已实跑）：把 `FEEDBACK_BOTTOM_MARGIN` 改回
        // `KEY_HINT_BOTTOM_MARGIN`，本条红在「反馈行底边不越过提示行
        // 顶边」。另一条（已实跑）：把 `AUTOSAVE_BOTTOM_MARGIN` 改成
        // 与 `FEEDBACK_BOTTOM_MARGIN` 相同，红在「痕迹行底边不越过
        // 反馈行顶边」。
        // Arrange
        let mut measure = ll_text::TextMeasurer::new().expect("内置字体资产应能正常解析");
        let (w, h) = (1280.0, 720.0);

        // Act
        let hint = bottom_row_panel(
            &mut measure,
            "提示",
            KEY_HINT_WIDTH,
            KEY_HINT_BOTTOM_MARGIN,
            w,
            h,
        );
        let feedback = bottom_row_panel(
            &mut measure,
            "反馈",
            FEEDBACK_WIDTH,
            FEEDBACK_BOTTOM_MARGIN,
            w,
            h,
        );

        let autosave = bottom_row_panel(
            &mut measure,
            "已自动保存",
            AUTOSAVE_WIDTH,
            AUTOSAVE_BOTTOM_MARGIN,
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
        assert!(
            autosave.rect.y + autosave.rect.height <= feedback.rect.y,
            "自动存档痕迹的底边不该越过反馈行的顶边"
        );
        assert!(
            autosave.rect.y >= h - BOTTOM_STRIP_HEIGHT,
            "三行都该落在 BOTTOM_STRIP_HEIGHT 说的那条底栏之内——L1 的             留白例外就是照这个常量开的"
        );
    }
}
