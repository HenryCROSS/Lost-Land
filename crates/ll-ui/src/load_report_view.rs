//! 加载管理界面：把 [`ll_mod::load_report::LoadReport`] 变成一份可以
//! 交给 [`ll_text::TextRenderer`] 画到屏幕上的文字。
//!
//! # 分两步，不是一步
//!
//! [`load_report_lines`] 是纯函数——只依赖 `LoadReport`/`expanded` 这两
//! 份数据，产出一批**拥有**自己文本内容的 [`LoadReportLine`]，不接触
//! GPU，可以在没有 wgpu 设备的环境下单元测试（本模块的测试就是这么
//! 做的）。[`load_report_runs`] 再把这批行转换成
//! [`ll_text::TextRun`]——`TextRun` 的 `text` 字段是借用（`&'a str`），
//! 必须借用自某个活得比它久的 `String`，所以拆成两步：先把内容定下来
//! 并交给调用方持有，再借出去建 run，不能在一个函数里既产生
//! `String` 又返回借用它的 `TextRun`（那是自引用，Rust 做不到）。
//! [`render_load_report`] 是最外层的薄封装，把两步接起来再调用
//! `TextRenderer::render` 真正提交到 GPU——**这不是简报草稿给出的原始
//! 签名**（草稿只写了 `&mut TextRenderer`，没有 device/queue/target 等
//! 参数），任务简报本身允许「可推翻的临时方案」，实现时按 `ll-text`
//! （任务 10）已经落地的真实 `TextRenderer::render` 签名调整，见其
//! 文档。
//!
//! # 分组顺序
//!
//! 规格 §10.6「结果分组显示（已加载/有警告/失败）」——本模块按这个
//! 顺序渲染三个分组，组内保持 [`LoadReport::entries`] 自身的顺序
//! （拓扑序或发现/解析序，见其文档），不重新排序。

use std::collections::HashSet;

use glyphon::Color;
use ll_core::ident::NamespacedId;
use ll_mod::load_report::{LoadReport, LoadStatus};
use ll_text::render::TextRun;
use ll_text::{TextError, TextRenderer};

/// 一行渲染就绪的文本：内容、位置、颜色，尚未挑字号/行高/换行宽度——
/// 那些是展示样式的选择，留给 [`load_report_runs`] 的调用方决定，见
/// [`render_load_report`] 里选用的默认值。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadReportLine {
    /// 这一行要显示的文字。
    pub text: String,
    /// 左上角 x 像素坐标。
    pub x: f32,
    /// 左上角 y 像素坐标。
    pub y: f32,
    /// 文字颜色——按状态区分：已加载用中性色，警告用黄色，失败用
    /// 红色，方便玩家/mod 作者一眼扫过分组标题就知道该关注哪里。
    pub color: Color,
}

/// 已加载分组标题颜色。
const LOADED_COLOR: Color = Color::rgba(200, 220, 200, 255);
/// 警告分组标题/条目颜色。
const WARNING_COLOR: Color = Color::rgba(230, 200, 80, 255);
/// 失败分组标题/条目颜色。
const FAILED_COLOR: Color = Color::rgba(230, 90, 80, 255);
/// 折叠状态下失败详情的提示颜色，比失败本体稍暗——次要信息不该抢眼。
const HINT_COLOR: Color = Color::rgba(160, 160, 160, 255);

/// 把 `report` 变成一批带位置、颜色的文本行。
///
/// `origin` 是第一行的左上角像素坐标，`line_height` 是相邻两行之间的
/// 垂直间距（像素）——两者都由调用方决定，本函数不内置任何布局假设。
///
/// # 展开 vs 折叠
///
/// `expanded` 里的失败 mod 会额外展开出「阶段」「消息」「位置」三行
/// 详情；不在 `expanded` 里的失败 mod 只显示一行「命名空间：失败（按
/// 键展开）」的折叠提示——这正是规格 §10.6「可展开查看含文件名与行号
/// 的详细错误」的落点。
pub fn load_report_lines(
    report: &LoadReport,
    expanded: &HashSet<NamespacedId>,
    origin: (f32, f32),
    line_height: f32,
) -> Vec<LoadReportLine> {
    let mut lines = Vec::new();
    let mut cursor_y = origin.1;

    push_line(
        &mut lines,
        &mut cursor_y,
        origin.0,
        line_height,
        LOADED_COLOR,
        "[已加载]".to_string(),
    );
    for (id, status) in report.entries_with(|status| matches!(status, LoadStatus::Loaded)) {
        let _ = status;
        push_line(
            &mut lines,
            &mut cursor_y,
            origin.0,
            line_height,
            LOADED_COLOR,
            format!("  {id}"),
        );
    }

    push_line(
        &mut lines,
        &mut cursor_y,
        origin.0,
        line_height,
        WARNING_COLOR,
        "[有警告]".to_string(),
    );
    for (id, status) in report.entries_with(|status| matches!(status, LoadStatus::Warning(_))) {
        if let LoadStatus::Warning(message) = status {
            push_line(
                &mut lines,
                &mut cursor_y,
                origin.0,
                line_height,
                WARNING_COLOR,
                format!("  {id}：{message}"),
            );
        }
    }

    push_line(
        &mut lines,
        &mut cursor_y,
        origin.0,
        line_height,
        FAILED_COLOR,
        "[失败]".to_string(),
    );
    for (id, status) in report.entries_with(|status| matches!(status, LoadStatus::Failed(_))) {
        let LoadStatus::Failed(error) = status else {
            continue;
        };
        if expanded.contains(id) {
            push_line(
                &mut lines,
                &mut cursor_y,
                origin.0,
                line_height,
                FAILED_COLOR,
                format!("  {id}（{:?} 阶段，点击折叠）", error.stage),
            );
            push_line(
                &mut lines,
                &mut cursor_y,
                origin.0,
                line_height,
                FAILED_COLOR,
                format!("    原因：{}", truncate_for_panel(&error.message)),
            );
            let location_text = match &error.location {
                Some(loc) => match loc.line {
                    Some(line) => format!("    位置：{}:{line}", short_path(&loc.file)),
                    None => format!("    位置：{}（无法定位具体行）", short_path(&loc.file)),
                },
                None => "    位置：未知".to_string(),
            };
            push_line(
                &mut lines,
                &mut cursor_y,
                origin.0,
                line_height,
                HINT_COLOR,
                location_text,
            );
        } else {
            push_line(
                &mut lines,
                &mut cursor_y,
                origin.0,
                line_height,
                FAILED_COLOR,
                format!("  {id}：失败（按键展开查看详情）"),
            );
        }
    }

    if let Some(result) = &report.cross_validate {
        let (color, text) = match result {
            Ok(()) => (
                LOADED_COLOR,
                "[交叉引用校验] 通过：地图上的地形索引全部已登记".to_string(),
            ),
            Err(message) => (FAILED_COLOR, format!("[交叉引用校验] 失败：{message}")),
        };
        push_line(
            &mut lines,
            &mut cursor_y,
            origin.0,
            line_height,
            color,
            text,
        );
    }

    lines
}

/// 把面板上单行展示的文字截到一个不容易触发 `cosmic-text` 自动换行
/// 的长度，超出部分换成省略号。
///
/// 与 [`short_path`] 缓解的是同一类真实撞见的问题（见其文档）：错误
/// 消息本身也可能很长（`ll-script` 的 `reject_dangerous_syntax` 在
/// 实测验收 demo 里就产出过一条中英夹杂近 60 字的解释，导致这一行
/// 在面板宽度内换行、压住下一行文字）。按**字符数**而不是字节数截断
/// ——`str` 按字节切片可能切在多字节字符中间导致 panic，
/// `chars().take(n)` 不会有这个问题。这不是根治（真正的根治见
/// [`short_path`] 文档最后一段），只是让面板在当前项目的错误消息长度
/// 分布下保持可读，本身也促使错误消息的作者把话说得更精炼——这与
/// 「代价可见，人才会去省」是同一条精神在文案篇幅上的体现。
const PANEL_MESSAGE_MAX_CHARS: usize = 44;

fn truncate_for_panel(text: &str) -> String {
    let char_count = text.chars().count();
    if char_count <= PANEL_MESSAGE_MAX_CHARS {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(PANEL_MESSAGE_MAX_CHARS).collect();
    truncated.push('…');
    truncated
}

/// 只取路径最后两段（通常是「mod 目录名/文件名」，如
/// `broken_syntax/main.scm`）——**实测撞见的真实缺陷**：本地开发环境
/// 的仓库路径可能很长（尤其是嵌套目录名含中文时，`cosmic-text` 按
/// 显示宽度而不是字符数断行，中文字符更宽，更容易撞上
/// [`DEFAULT_MAX_WIDTH`]），完整绝对路径拼上其余文字后会在面板宽度内
/// 自动折行成两行，而 [`load_report_lines`] 是按「一条逻辑行 = 一个
/// 固定行高」推进光标的，断行发生时下一条目会与它重叠——用真实验收
/// demo 截图撞见的（`.superpowers/sdd/2026-08-18-p4-script-and-mod/
/// task-11-12-report.md` 有截图记录）。缩短成两段既足够定位「哪个 mod
/// 的哪个文件」，又大幅降低撞上断行阈值的概率——这不是从根本上解决
/// 「文本可能超宽」这个问题（真正的根治需要 `load_report_lines` 感知
/// `cosmic-text` 的实际断行结果动态推进 `cursor_y`，那是一次不小的
/// 重构，本任务的诚实范围内先用这个更便宜的缓解手段），但对本项目
/// 当前的路径深度和面板宽度是有效的。
fn short_path(path: &std::path::Path) -> String {
    let components: Vec<_> = path.components().collect();
    let tail: Vec<String> = components
        .iter()
        .rev()
        .take(2)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    tail.into_iter().rev().collect::<Vec<_>>().join("/")
}

/// 追加一行并把光标下移一行高——抽成帮手避免上面五处分组各自重复
/// 「构造 LoadReportLine + 推进 cursor_y」这套样板。
fn push_line(
    lines: &mut Vec<LoadReportLine>,
    cursor_y: &mut f32,
    x: f32,
    line_height: f32,
    color: Color,
    text: String,
) {
    lines.push(LoadReportLine {
        text,
        x,
        y: *cursor_y,
        color,
    });
    *cursor_y += line_height;
}

/// 把 `lines` 转换成 [`TextRun`]，供 [`TextRenderer::render`] 消费。
///
/// `font_size`/`line_height`/`max_width` 对每一行取相同值——加载管理
/// 界面是等宽信息面板，不需要逐行变化字号，与 P3 验收 demo 的时间轴
/// 侧栏（同一批次风格）保持一致。
pub fn load_report_runs<'a>(
    lines: &'a [LoadReportLine],
    font_size: f32,
    line_height: f32,
    max_width: f32,
) -> Vec<TextRun<'a>> {
    lines
        .iter()
        .map(|line| TextRun {
            text: &line.text,
            x: line.x,
            y: line.y,
            font_size,
            line_height,
            max_width,
            color: line.color,
            bold: false,
        })
        .collect()
}

/// 默认字号（像素）。
pub const DEFAULT_FONT_SIZE: f32 = 14.0;
/// 默认行高（像素）。
pub const DEFAULT_LINE_HEIGHT: f32 = 18.0;
/// 默认换行宽度（像素）——足够宽以容纳一整条错误消息，不强制折行。
pub const DEFAULT_MAX_WIDTH: f32 = 600.0;

/// 把 `report` 画进 `target`：布局 + 提交 GPU 的一站式封装。
///
/// `origin` 是面板左上角坐标；`resolution_width`/`resolution_height`
/// 必须是 `target` 的真实原生像素尺寸（见 [`TextRenderer::render`]
/// 文档，本函数原样转发这条约束，不做任何折算）。
#[allow(clippy::too_many_arguments)]
pub fn render_load_report(
    text_renderer: &mut TextRenderer,
    device: &ll_render::wgpu::Device,
    queue: &ll_render::wgpu::Queue,
    target: &ll_render::wgpu::TextureView,
    resolution_width: u32,
    resolution_height: u32,
    report: &LoadReport,
    expanded: &HashSet<NamespacedId>,
    origin: (f32, f32),
) -> Result<(), TextError> {
    let lines = load_report_lines(report, expanded, origin, DEFAULT_LINE_HEIGHT);
    let runs = load_report_runs(
        &lines,
        DEFAULT_FONT_SIZE,
        DEFAULT_LINE_HEIGHT,
        DEFAULT_MAX_WIDTH,
    );
    text_renderer.render(
        device,
        queue,
        target,
        resolution_width,
        resolution_height,
        &runs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_mod::load_report::{LoadError, LoadStage, SourceLocation};
    use std::path::PathBuf;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    #[test]
    fn truncate_for_panel对短文本原样返回() {
        // Arrange & Act & Assert
        assert_eq!(truncate_for_panel("短消息"), "短消息");
    }

    #[test]
    fn truncate_for_panel对超长文本截断并加省略号() {
        // Arrange：故意超过 PANEL_MESSAGE_MAX_CHARS 字符数。
        let long_text = "字".repeat(PANEL_MESSAGE_MAX_CHARS + 10);

        // Act
        let truncated = truncate_for_panel(&long_text);

        // Assert
        assert_eq!(truncated.chars().count(), PANEL_MESSAGE_MAX_CHARS + 1);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn short_path只保留最后两段路径() {
        // Arrange：模拟真实撞见的过长路径（含中文目录名）。
        //
        // 用 `join` 逐段拼，而不是写死一个带反斜杠的字面量：反斜杠在 Unix 上
        // 是合法的文件名字符，那样的字面量在 Linux 上会被 `Path::components`
        // 当成**单独一段**普通文件名，「取最后两段」等于取它自己，断言必然
        // 失败。这条测试此前只在 Windows 上通过，CI 的 ubuntu 任务一直红着。
        //
        // 同族问题另见 `ll_mod::asset_vfs::validate_relative_asset_path` 的
        // 测试——那里的反斜杠字面量是**故意**的，测的正是这类字符串会被拒绝，
        // 两者不要混为一谈。
        let path = PathBuf::from("迷途大陆")
            .join("LostLand")
            .join("mods")
            .join("broken_syntax")
            .join("main.scm");

        // Act
        let short = short_path(&path);

        // Assert
        assert_eq!(short, "broken_syntax/main.scm");
    }

    #[test]
    fn short_path对只有一段的路径原样返回() {
        // Arrange
        let path = PathBuf::from("main.scm");

        // Act
        let short = short_path(&path);

        // Assert
        assert_eq!(short, "main.scm");
    }

    #[test]
    fn 已加载的mod出现在已加载分组的行文本里() {
        // Arrange
        let mut report = LoadReport::new();
        report.push(id("good:self"), LoadStatus::Loaded);
        let expanded = HashSet::new();

        // Act
        let lines = load_report_lines(&report, &expanded, (0.0, 0.0), 18.0);

        // Assert
        assert!(lines.iter().any(|line| line.text.contains("good:self")));
    }

    #[test]
    fn 折叠状态下失败详情不显示阶段与原因() {
        // Arrange
        let mut report = LoadReport::new();
        report.push(
            id("bad:self"),
            LoadStatus::Failed(LoadError {
                mod_id: id("bad:self"),
                stage: LoadStage::LoadScript,
                message: "缺右括号".to_string(),
                location: None,
            }),
        );
        let expanded = HashSet::new();

        // Act
        let lines = load_report_lines(&report, &expanded, (0.0, 0.0), 18.0);

        // Assert：折叠状态只应该看到一行提示，不应该泄漏"缺右括号"这类
        // 详情文本。
        assert!(
            lines
                .iter()
                .any(|line| line.text.contains("按键展开查看详情"))
        );
        assert!(!lines.iter().any(|line| line.text.contains("缺右括号")));
    }

    #[test]
    fn 展开状态下失败详情显示阶段原因与位置() {
        // Arrange
        let mut report = LoadReport::new();
        let bad_id = id("bad:self");
        report.push(
            bad_id.clone(),
            LoadStatus::Failed(LoadError {
                mod_id: bad_id.clone(),
                stage: LoadStage::LoadScript,
                message: "缺右括号".to_string(),
                location: Some(SourceLocation {
                    file: PathBuf::from("mods/bad/main.scm"),
                    line: Some(3),
                }),
            }),
        );
        let mut expanded = HashSet::new();
        expanded.insert(bad_id);

        // Act
        let lines = load_report_lines(&report, &expanded, (0.0, 0.0), 18.0);

        // Assert：展开状态渲染出比折叠状态更详细的内容——原因文本、
        // 具体行号都应该出现。
        assert!(lines.iter().any(|line| line.text.contains("缺右括号")));
        assert!(lines.iter().any(|line| line.text.contains("main.scm:3")));
    }

    #[test]
    fn 展开状态比折叠状态渲染出更多行() {
        // Arrange：同一份失败条目，只是 expanded 集合不同。
        let mut report = LoadReport::new();
        let bad_id = id("bad:self");
        report.push(
            bad_id.clone(),
            LoadStatus::Failed(LoadError {
                mod_id: bad_id.clone(),
                stage: LoadStage::Register,
                message: "重复定义".to_string(),
                location: None,
            }),
        );

        // Act
        let collapsed = load_report_lines(&report, &HashSet::new(), (0.0, 0.0), 18.0);
        let mut expanded_set = HashSet::new();
        expanded_set.insert(bad_id);
        let expanded = load_report_lines(&report, &expanded_set, (0.0, 0.0), 18.0);

        // Assert
        assert!(expanded.len() > collapsed.len());
    }

    #[test]
    fn 每一行的y坐标随行高递增() {
        // Arrange
        let mut report = LoadReport::new();
        report.push(id("a:self"), LoadStatus::Loaded);
        report.push(id("b:self"), LoadStatus::Loaded);
        let expanded = HashSet::new();

        // Act
        let lines = load_report_lines(&report, &expanded, (10.0, 20.0), 16.0);

        // Assert：至少两行 y 值互不相同且递增（分组标题 + 两个条目）。
        for pair in lines.windows(2) {
            assert!(pair[1].y > pair[0].y);
        }
    }

    #[test]
    fn 交叉引用校验通过时出现一行成功提示() {
        // Arrange
        let mut report = LoadReport::new();
        report.cross_validate = Some(Ok(()));
        let expanded = HashSet::new();

        // Act
        let lines = load_report_lines(&report, &expanded, (0.0, 0.0), 18.0);

        // Assert
        assert!(
            lines
                .iter()
                .any(|line| line.text.contains("交叉引用校验") && line.text.contains("通过"))
        );
    }

    #[test]
    fn load_report_runs产出的run数量与line数量一致且借用同一份文本() {
        // Arrange
        let mut report = LoadReport::new();
        report.push(id("a:self"), LoadStatus::Loaded);
        let expanded = HashSet::new();
        let lines = load_report_lines(&report, &expanded, (0.0, 0.0), 18.0);

        // Act
        let runs = load_report_runs(&lines, 14.0, 18.0, 400.0);

        // Assert
        assert_eq!(runs.len(), lines.len());
        assert_eq!(runs[0].text, lines[0].text);
    }
}
