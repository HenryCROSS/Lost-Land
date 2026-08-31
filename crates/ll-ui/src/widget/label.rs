//! 文本行：一段带位置的文字，可以转换成 [`ll_text::TextRun`] 交给
//! [`ll_text::TextRenderer::render`] 画到屏幕上。
//!
//! 形状与 [`crate::load_report_view::LoadReportLine`] 一致（同样是
//! 「内容 + 位置」，颜色留给调用方决定要不要按状态区分）——本类型是
//! `load_report_view` 那套「先产出拥有文本的行，再借出去建
//! `TextRun`」两步拆分模式（见其模块文档「分两步，不是一步」一节）在
//! `ll-ui` 完整控件层里的正式命名：加载管理界面当时还没有一个跨模块
//! 共享的「文本行」概念，各自内联了一个 `LoadReportLine`；HUD 现在有
//! 四块面板都需要同一件事，值得抽成本模块正式的 [`Label`] 控件。

use glyphon::Color;
use ll_text::TextRun;

/// 一行渲染就绪的文本：内容 + 左上角像素位置，均为窗口原生分辨率
/// 坐标（见 [`crate::widget::geometry::Rect`] 文档「坐标系」一节）。
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// 要显示的文字。
    pub text: String,
    /// 左上角 x 像素坐标。
    pub x: f32,
    /// 左上角 y 像素坐标。
    pub y: f32,
    /// 这一行的**断行宽度**（像素）——超过它就换行。
    ///
    /// # 为什么这条信息住在 `Label` 上，而不是提交给 GPU 时才补
    ///
    /// 此前它是提交时补的一个与面板无关的常数：HUD 全部文本一律
    /// `400.0`（`crate::hud::render`），模态屏一律 `SCREEN_WIDTH`
    /// （连自己的内边距都没减）。而六块 HUD 面板的内容宽是
    /// 608/248/208/208/348/408，**没有一块等于 400**——英文界面下
    /// 十处溢出，其中八处中文构建里一个像素都看不见
    /// （`knowledge/design/ui-and-navigation.md` §8）。
    ///
    /// 现在这个字段的**唯一**写入点是
    /// [`crate::widget::list::RowCursor`]，而它的值由面板宽度派生
    /// （`面板宽 - 2 × 内边距`）。于是「面板有多宽」成了唯一真相源，
    /// 断行宽度是它的派生值——**任何人再想写死一个断行宽度，都得先
    /// 绕过 `RowCursor`**。这是把纪律变成结构，不是靠注释提醒。
    pub max_width: f32,
}

impl Label {
    /// 借出一个 [`TextRun`]——`text` 借用自 `self`,因此返回值的生命
    /// 期不能超过 `self`,这也是为什么调用方必须先把全部 `Label` 收集
    /// 进一个 `Vec` 再统一转换（不能在产出 `Label` 的同一个函数里就
    /// 地借出 `TextRun`,那是自引用,见 `load_report_view` 模块文档
    /// 「分两步，不是一步」一节的同一条限制）。
    /// 断行宽度不再是参数——它是 [`Label::max_width`]，由产出这一行的
    /// 面板决定，见该字段文档。
    pub fn to_text_run(&self, font_size: f32, line_height: f32, color: Color) -> TextRun<'_> {
        TextRun {
            text: &self.text,
            x: self.x,
            y: self.y,
            font_size,
            line_height,
            max_width: self.max_width,
            color,
            bold: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_text_run借用同一份文本内容() {
        // Arrange
        let label = Label {
            text: "sample".to_string(),
            x: 1.0,
            y: 2.0,
            max_width: 200.0,
        };

        // Act
        let run = label.to_text_run(14.0, 18.0, Color::rgba(255, 255, 255, 255));

        // Assert
        assert_eq!(run.text, "sample");
    }

    #[test]
    fn to_text_run的断行宽度取自标签自己而不是调用方() {
        // 这条盯的是「断行宽度是面板宽度的派生值」这条不变式的最后
        // 一环：`TextRun` 的 `max_width` 必须原样来自 `Label`，中途
        // 没有任何一处能再塞一个与面板无关的常数进去。
        //
        // 反例验证（已实跑）：把 `to_text_run` 里的 `self.max_width`
        // 改回一个字面量 `400.0`，本条立刻变红。
        // Arrange
        let label = Label {
            text: "sample".to_string(),
            x: 1.0,
            y: 2.0,
            max_width: 208.0,
        };

        // Act
        let run = label.to_text_run(14.0, 18.0, Color::rgba(255, 255, 255, 255));

        // Assert
        assert_eq!(run.max_width, 208.0);
    }
}
