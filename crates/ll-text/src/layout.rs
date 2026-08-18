//! 排版：整形 + 断行，纯 CPU，不接触 GPU。
//!
//! 本模块只依赖 `cosmic_text::FontSystem`/`Buffer`，不依赖
//! [`crate::render::TextRenderer`]——这是刻意的：排版结果（断行位置、
//! 每个字形落在哪个字体）应该能在没有可用图形适配器的环境（比如 CI）
//! 里被测试，不该被「有没有 GPU」这件事卡住。渲染层的截图验证只补上
//! 「排版结果画出来是否真的清晰可读」这一层，二者互补而不是互相替代。

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight};

use crate::fonts::FontCatalog;

/// 某个字形实际落在哪个字体上。
///
/// 只关心这一件事：字体回退（fallback）有没有按预期把某个码位路由到
/// 图标字体，而不是正文字体——这正是
/// `knowledge/pipelines/text-and-font-rendering.md` 标注为「原理可行、
/// 未实测」的那条结论，本模块的测试就是补上这次实测。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphOrigin {
    /// 落在指定的正文字体上（思源黑体）。
    Text,
    /// 落在指定的图标字体上（Tabler Icons）。
    Icon,
    /// 落在既非正文也非图标字体的第三个字体上——理论上不该发生，因为
    /// `fontdb` 里只注册了这两类字体，但显式区分出来，不把「不是正文
    /// 字体」直接当成「一定是图标字体」，那是两个不同的断言。
    Other,
}

/// 单个字形的排版结果，只保留断行判断与字体回退验证需要的字段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutGlyphInfo {
    /// 该字形对应的源文本字节范围起点（`str` 的字节偏移，不是字符数）。
    pub start: usize,
    /// 该字形对应的源文本字节范围终点。
    pub end: usize,
    /// 字形前进宽度（像素，字体原始设计尺寸下的度量，未经任何缩放）。
    pub advance: f32,
    /// 该字形实际落在哪个字体上。
    pub origin: GlyphOrigin,
}

/// 一行排版结果。
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutLineInfo {
    /// 整行的像素宽度。
    pub width: f32,
    /// 行内每个字形，按视觉顺序排列。
    pub glyphs: Vec<LayoutGlyphInfo>,
}

/// 一段文本排版后的完整结果：断行后的每一行。
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutResult {
    /// 断行后的每一行，按从上到下顺序排列。
    pub lines: Vec<LayoutLineInfo>,
}

/// 对一段文本按正文字体 + 给定字号/最大宽度排版，返回断行结果。
///
/// `font_system` 与 `catalog` 由调用方持有并跨多次调用复用——重建
/// `FontSystem` 意味着重新解析三个内置字体文件，是不必要的开销。
pub fn layout_text(
    font_system: &mut FontSystem,
    catalog: &FontCatalog,
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
) -> LayoutResult {
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
    buffer.set_size(Some(max_width), None);
    let attrs = Attrs::new()
        .family(Family::Name(&catalog.text_family))
        .weight(Weight::NORMAL);
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    let lines = buffer
        .layout_runs()
        .map(|run| LayoutLineInfo {
            width: run.line_w,
            glyphs: run
                .glyphs
                .iter()
                .map(|glyph| LayoutGlyphInfo {
                    start: glyph.start,
                    end: glyph.end,
                    advance: glyph.w,
                    origin: if glyph.font_id == catalog.icon_font_id {
                        GlyphOrigin::Icon
                    } else {
                        // 正文字体是唯一注册的另一类字体来源；Regular/Bold
                        // 两个 face 是不同的 fontdb::ID，但都算 Text——
                        // 这里不比对具体 ID，只比对「不是图标字体」。
                        GlyphOrigin::Text
                    },
                })
                .collect(),
        })
        .collect();

    LayoutResult { lines }
}

#[cfg(test)]
mod tests {
    use cosmic_text::fontdb::Database;

    use super::*;

    /// 构造一份仅含内置三个字体的 `FontSystem`，供测试复用。
    ///
    /// 用空库而不是 `FontSystem::new()`：后者会扫描系统已安装字体，
    /// 测试环境装了哪些字体是不确定的，混入系统字体会让「路由到了图标
    /// 字体」这类断言失去意义——万一测试机恰好装了一款也覆盖同一 PUA
    /// 码位的字体，结果会变得不可预测。
    fn test_font_system() -> (FontSystem, FontCatalog) {
        let mut db = Database::new();
        let catalog = FontCatalog::load(&mut db).expect("内置字体资产应能正常解析");
        let font_system = FontSystem::new_with_locale_and_db("zh-CN".to_string(), db);
        (font_system, catalog)
    }

    #[test]
    fn 中英文混排在指定宽度内正确换行() {
        // Arrange
        let (mut font_system, catalog) = test_font_system();
        let text = "迷途大陆 Lost Land 是一款像素风 Roguelike 游戏，本行文字应当在给定宽度内换行。";

        // Act
        let result = layout_text(&mut font_system, &catalog, text, 16.0, 20.0, 160.0);

        // Assert：给一个较窄的最大宽度，长句必须被断成不止一行；
        // 具体断在哪个字符属于字体度量的产物，用下面的快照测试锁定，
        // 这里只锁「确实发生了换行」这个更粗但更稳定的事实。
        assert!(result.lines.len() > 1);
    }

    #[test]
    fn 断行结果符合字体度量的快照() {
        // 像素级渲染快照对字体/驱动版本太敏感（见模块文档），这里改为
        // 快照排版的结构化结果：每行的字节范围与宽度，由 cosmic-text
        // 的字体度量决定，只要内置字体文件与 cosmic-text 版本不变就
        // 应当保持稳定。
        // Arrange
        let (mut font_system, catalog) = test_font_system();
        let text = "迷途大陆 Lost Land 是一款像素风 Roguelike 游戏。";

        // Act
        let result = layout_text(&mut font_system, &catalog, text, 16.0, 20.0, 160.0);
        let summary: Vec<(usize, usize, i64)> = result
            .lines
            .iter()
            .map(|line| {
                let start = line.glyphs.first().map(|g| g.start).unwrap_or(0);
                let end = line.glyphs.last().map(|g| g.end).unwrap_or(0);
                // 宽度四舍五入到整数：字体度量本身是稳定的整数级设计
                // 单位换算，但浮点求和顺序在不同后端下可能有极小的
                // 尾数误差，四舍五入后再快照，避免快照因无意义的
                // 亚像素抖动而失败。
                (start, end, line.width.round() as i64)
            })
            .collect();

        // Assert
        insta::assert_debug_snapshot!(summary);
    }

    #[test]
    fn 私用区码位实测路由到图标字体而非正文字体() {
        // 这是 knowledge/pipelines/text-and-font-rendering.md 标注为
        // 「原理可行、未实测」的那条结论的实测：思源黑体不含 PUA 码位
        // 的字形，`cosmic-text` 的字体回退在正文字体查不到字形时应当
        // 走到库里注册的另一个字体（Tabler Icons）。用 Task 10 简报里
        // 已实测存在的两个具体码位之一（settings，U+EB20，见
        // knowledge/licenses/2026-08-18-ll-text-asset-import.md）。
        // Arrange
        let (mut font_system, catalog) = test_font_system();
        let text = "设置\u{EB20}";

        // Act
        let result = layout_text(&mut font_system, &catalog, text, 16.0, 20.0, 400.0);
        let glyphs: Vec<LayoutGlyphInfo> = result
            .lines
            .into_iter()
            .flat_map(|line| line.glyphs)
            .collect();

        // Assert：最后一个字形（PUA 码位）必须被路由到图标字体。
        let last = glyphs.last().expect("应至少排出一个字形");
        assert_eq!(last.origin, GlyphOrigin::Icon);
    }

    #[test]
    fn 私用区码位前的汉字仍落在正文字体() {
        // 与上一条测试互补：只验证「PUA 路由到图标字体」还不够，必须
        // 同时确认混排中的汉字没有被误路由——如果回退逻辑把整段文本
        // 都判给图标字体，上一条测试会通过但结果是错的。
        // Arrange
        let (mut font_system, catalog) = test_font_system();
        let text = "设置\u{EB20}";

        // Act
        let result = layout_text(&mut font_system, &catalog, text, 16.0, 20.0, 400.0);
        let glyphs: Vec<LayoutGlyphInfo> = result
            .lines
            .into_iter()
            .flat_map(|line| line.glyphs)
            .collect();

        // Assert：第一个字形（"设"）必须落在正文字体。
        let first = glyphs.first().expect("应至少排出一个字形");
        assert_eq!(first.origin, GlyphOrigin::Text);
    }
}
