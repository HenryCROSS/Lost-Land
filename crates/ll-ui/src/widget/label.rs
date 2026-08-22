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
}

impl Label {
    /// 借出一个 [`TextRun`]——`text` 借用自 `self`,因此返回值的生命
    /// 期不能超过 `self`,这也是为什么调用方必须先把全部 `Label` 收集
    /// 进一个 `Vec` 再统一转换（不能在产出 `Label` 的同一个函数里就
    /// 地借出 `TextRun`,那是自引用,见 `load_report_view` 模块文档
    /// 「分两步，不是一步」一节的同一条限制）。
    pub fn to_text_run(
        &self,
        font_size: f32,
        line_height: f32,
        max_width: f32,
        color: Color,
    ) -> TextRun<'_> {
        TextRun {
            text: &self.text,
            x: self.x,
            y: self.y,
            font_size,
            line_height,
            max_width,
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
        };

        // Act
        let run = label.to_text_run(14.0, 18.0, 200.0, Color::rgba(255, 255, 255, 255));

        // Assert
        assert_eq!(run.text, "sample");
    }
}
