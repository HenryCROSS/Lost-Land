//! 文本测量：**不建 GPU 也能问「这段字画出来多宽、断成几行」**。
//!
//! # 为什么这一层非有不可
//!
//! [`crate::layout::layout_text`] 早就是纯 CPU 的（只要一个
//! `FontSystem` 加一个 [`crate::fonts::FontCatalog`]），但在本模块之前
//! **唯一持有这两样东西的是 [`crate::render::TextRenderer`]**，而它的
//! 构造函数要 `wgpu::Device`/`Queue`。于是「量一段文字有多宽」这件事
//! 在无图形适配器的环境（CI、单元测试、门禁）里事实上做不到——
//! `ll-ui`/`ll-game` 因此一个调用点都没有，布局层只能猜宽度。
//!
//! `knowledge/design/ui-and-navigation.md` §8.2 记下了这一猜的代价：
//! HUD 全部文本的断行宽度被写死成一个 `400.0`，而六块面板的内容宽是
//! 608/248/208/208/348/408，**没有一块等于 400**——英文界面下十处溢出，
//! 其中八处中文构建里一个像素都看不见。
//!
//! # 一份度量，两个持有者
//!
//! [`MeasureText`] 是那个「量一段字」的能力本身；
//!
//! - [`TextMeasurer`]：纯 CPU，自己拥有一份 `FontSystem`。供测试、门禁
//!   与任何拿不到 GPU 的调用方。
//! - [`crate::render::TextRenderer`]：**复用它自己那份 `FontSystem`**，
//!   不额外建第二份。产品路径走这一条。
//!
//! 两条路径底下是**同一个** [`crate::layout::layout_text`]——不存在
//! 「量的时候一套度量、画的时候另一套度量」的分叉可能。这正是本仓库
//! 反复付过代价的那个形状（真相源之外的副本迟早分叉，而分叉时没有任何
//! 东西会报错），这里从结构上排除掉。

use cosmic_text::FontSystem;
use cosmic_text::fontdb::Database;

use crate::error::TextError;
use crate::fonts::FontCatalog;
use crate::layout;

/// 一段文本按给定字号/行高/最大宽度排版之后的两个尺寸事实。
///
/// 只留这两个字段：布局层要的就是「纵向占几行」（面板高度）与「最宽的
/// 一行有多宽」（放不放得下）。逐字形的信息在
/// [`crate::layout::LayoutResult`] 里，需要的人直接用那一层。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    /// 断行之后一共几行。
    ///
    /// **空串是 0 行还是 1 行**：`cosmic-text` 对空串产出 0 个
    /// `layout_run`，而布局层要的是「这个标签占几行高」——一个空标签
    /// 仍然占一行（它是列表里真实存在的一行）。因此本字段对空串**恒
    /// 返回 1**，不是 0：让每一个调用点各自写一遍 `.max(1)` 才是会漂
    /// 的那种约定。
    pub line_count: usize,
    /// 断行之后最宽那一行的像素宽度。空串为 `0.0`。
    pub max_line_width: f32,
}

/// 「量一段文字」这个能力。
///
/// 取 `&mut self` 是 `cosmic-text` 的要求：整形要写字形缓存与字体库，
/// 不是本 trait 自己的设计取舍。
pub trait MeasureText {
    /// 量 `text` 在 `max_width` 内断行之后的行数与最长行宽。
    fn measure_text(
        &mut self,
        text: &str,
        font_size: f32,
        line_height: f32,
        max_width: f32,
    ) -> TextMetrics;
}

/// 把一份 [`crate::layout::LayoutResult`] 折算成 [`TextMetrics`]——
/// [`TextMeasurer`] 与 [`crate::render::TextRenderer`] 两个实现共用的
/// 那一段，**不写两遍**。
pub(crate) fn metrics_of(result: &layout::LayoutResult) -> TextMetrics {
    TextMetrics {
        // 空串占一行，见 `TextMetrics::line_count` 文档。
        line_count: result.lines.len().max(1),
        max_line_width: result
            .lines
            .iter()
            .map(|line| line.width)
            .fold(0.0_f32, f32::max),
    }
}

/// 纯 CPU 的文本测量器：自己拥有一份只含内置字体的 `FontSystem`。
///
/// # 为什么用空的 `fontdb::Database` 而不是 `FontSystem::new()`
///
/// 与 [`crate::layout`] 的测试同一条理由：`FontSystem::new()` 会扫描
/// **系统已安装字体**，而测试机/CI 机上装了哪些字体是不确定的。测量
/// 结果若受系统字体影响，「这段文字放不放得下」在两台机器上会得到两个
/// 答案，门禁立刻变成随机红。产品路径的
/// [`crate::render::TextRenderer::new`] 走的也是空库（`Database::new()`
/// 加内置三个文件），两条路径因此量出同一个数。
pub struct TextMeasurer {
    font_system: FontSystem,
    catalog: FontCatalog,
}

impl TextMeasurer {
    /// 加载内置字体，建一个可以立刻用的测量器。
    ///
    /// 失败只有一种来源：内置字体资产解析不了（[`FontCatalog::load`]），
    /// 与 [`crate::render::TextRenderer::new`] 同一条错误路径。
    pub fn new() -> Result<TextMeasurer, TextError> {
        let mut db = Database::new();
        let catalog = FontCatalog::load(&mut db)?;
        let font_system = FontSystem::new_with_locale_and_db("zh-CN".to_string(), db);
        Ok(TextMeasurer {
            font_system,
            catalog,
        })
    }
}

impl MeasureText for TextMeasurer {
    fn measure_text(
        &mut self,
        text: &str,
        font_size: f32,
        line_height: f32,
        max_width: f32,
    ) -> TextMetrics {
        let result = layout::layout_text(
            &mut self.font_system,
            &self.catalog,
            text,
            font_size,
            line_height,
            max_width,
        );
        metrics_of(&result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空串占一行且宽度为零() {
        // Arrange
        let mut measurer = TextMeasurer::new().expect("内置字体资产应能正常解析");

        // Act
        let metrics = measurer.measure_text("", 14.0, 18.0, 400.0);

        // Assert：0 行会让面板高度比内容矮一行，见 `line_count` 文档。
        assert_eq!(metrics.line_count, 1);
        assert_eq!(metrics.max_line_width, 0.0);
    }

    #[test]
    fn 汉字推进恒为一个字号宽() {
        // 思源黑体 unitsPerEm = 1000，CJK 推进恒 1000/1000 = 1.0 em，
        // 因此 14px 下每个汉字恰好 14.00px——
        // `knowledge/design/ui-and-navigation.md` §8.1 那张表的第一行，
        // 这条测试是它的实测复核。
        // Arrange
        let mut measurer = TextMeasurer::new().expect("内置字体资产应能正常解析");

        // Act：五个汉字，给一个宽到不可能换行的上限。
        let metrics = measurer.measure_text("迷途大陆的", 14.0, 18.0, 4000.0);

        // Assert
        assert_eq!(metrics.line_count, 1);
        assert!(
            (metrics.max_line_width - 70.0).abs() < 0.01,
            "五个汉字在 14px 下应恰好 70.00px，实测 {}",
            metrics.max_line_width
        );
    }

    #[test]
    fn 英文比等长中文宽() {
        // §8.1 的核心结论：**英文才是每一条散文型字符串的最坏情况**
        // （en:zh ≈ 1.44:1）。门禁只看中文正是十处溢出里八处没被发现的
        // 结构性原因，这条把「英文更宽」钉成一条会红的断言。
        // Arrange
        let mut measurer = TextMeasurer::new().expect("内置字体资产应能正常解析");

        // Act：同一句话的两种语言（取自 assets/locales 的 hint 一类文案
        // 的典型长度）。
        let zh = measurer.measure_text("上下移动，左右调整，确认键生成世界", 14.0, 18.0, 4000.0);
        let en = measurer.measure_text(
            "Up/Down to move, Left/Right to adjust, Confirm to generate",
            14.0,
            18.0,
            4000.0,
        );

        // Assert
        assert!(
            en.max_line_width > zh.max_line_width,
            "英文 {} 应宽于中文 {}",
            en.max_line_width,
            zh.max_line_width
        );
    }

    #[test]
    fn 超过最大宽度的长句真的断成多行() {
        // Arrange
        let mut measurer = TextMeasurer::new().expect("内置字体资产应能正常解析");
        let text = "迷途大陆 Lost Land 是一款像素风 Roguelike 游戏，本行文字应当在给定宽度内换行。";

        // Act
        let metrics = measurer.measure_text(text, 14.0, 18.0, 160.0);

        // Assert：断了行，且每一行都真的没有超过给定上限。
        assert!(metrics.line_count > 1);
        assert!(metrics.max_line_width <= 160.0);
    }
}
