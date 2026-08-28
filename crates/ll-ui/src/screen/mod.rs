//! 模态屏：盖住游戏画面的整屏 UI——游戏内菜单与设置界面。
//!
//! # 与 [`crate::hud`] 的分界
//!
//! 一句话：**`hud` 画在世界之上，`screen` 盖住世界。**
//!
//! | | [`crate::hud`] | 本模块 |
//! |---|---|---|
//! | 世界在它底下 | 照常推进（玩家一按键就结算一回合） | **一个字节都不动**（`ll_game::app::Demo::advance` 早退） |
//! | 输入上下文 | `InputContext::Gameplay` | `InputContext::Menu`（经 `ll_ui::widget::ui_mode::UiModeStack`） |
//! | 占屏 | 贴边的几块面板，中间留给地图视口 | 居中一块，外加一层压暗整屏的背板 |
//!
//! 这条分界不是审美取舍，是[`crate::widget::ui_mode`] 那套上下文切换
//! 机制**只对本模块这一类屏成立**：`UiModeStack` 一压栈就把物理键映射
//! 整体切到菜单表，游戏内主流程的移动/交互键在那之后全部解析不出来。
//! HUD 的背包/制作菜单不走这条路（它们仍在 `Gameplay` 上下文下由
//! `ll_game::player_action` 自己判「菜单开着时方向键归菜单用」），本
//! 批次刻意不动它们。
//!
//! # 为什么只有一种屏，不是「菜单屏」加「设置屏」两套
//!
//! 两块屏在**画法上**完全同构：一块居中面板、一行标题、一列行、其中
//! 一行带光标标记、一行底部提示。区别全在**行里写什么字**——菜单是三
//! 条固定选项，设置是「语言：简体中文」「移动：上 / W」这类带当前值的
//! 行。而「一条设置该显示成什么字」需要同时认识 `GameConfig`、
//! `KeyBindings` 与 `Catalog`，那是 `ll-game`（唯一持有全部这些东西的
//! 那一层）的排版职责，不是本模块的。
//!
//! 这与 [`crate::hud::action_menu`] 的取舍逐字相同（「只收已经排好版的
//! 字符串，不收领域类型」），也是 ADR 0021 的直接应用：真正共用的算法
//! 只有「一列行 + 一个高亮标记 + 现算面板高度」这一段，为两块屏各写
//! 一份只会得到两份会各自漂移的同一个算法。
//!
//! # 为什么不复用 [`crate::hud`] 的 `build_panel`
//!
//! 如实记录，这一处是**刻意的重复**：`hud::build_panel` 与本模块的
//! [`build_screen_panel`] 是同一个「游标推行 + 按行数现算面板高度」的
//! 十行算法。不复用的理由是并行批次风险——本批次落地时另一个批次正在
//! 改 `crates/ll-ui/src/hud/` 下的面板布局（居中），而 git 抓不到这类
//! 语义冲突（交接文档第一节纪律 5 记着这个教训的实例）。十行重复换来
//! 「本模块与 `hud/` 零耦合」，代价明确、可逆：两个批次都落地之后，把
//! 这一段收拢回一个共用 helper 是一次纯机械的重构。

pub mod render;

use ll_i18n::Catalog;

use crate::widget::geometry::Rect;
use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 模态屏的行高（像素）——与 [`crate::hud::DEFAULT_LINE_HEIGHT`] 取同
/// 一个值，两块屏读起来要像同一个产品。
pub const SCREEN_LINE_HEIGHT: f32 = 18.0;
/// 模态屏面板的内边距（像素）。
pub const SCREEN_PADDING: f32 = 10.0;
/// 模态屏正文字号（像素）。
pub const SCREEN_FONT_SIZE: f32 = 14.0;
/// 模态屏面板宽度（像素）——设置界面最长的一行是「某个动作 + 它绑着的
/// 几个键」，固定宽度比按内容现算更稳：现算会让面板宽度随光标移动而
/// 跳变（不同行的文字长短不一），看起来像画面在抖。
pub const SCREEN_WIDTH: f32 = 520.0;

/// 光标所在那一行的前缀——与 [`crate::hud::action_menu::CURSOR_PREFIX`]
/// 取同一个记号，理由见那里的模块文档「光标标记为什么是文字前缀」
/// 一节：它可以被纯文本断言直接验证，而 ADR 0025 禁止用合成按键做验收，
/// 可断言性正是本项目挑选交互表达方式时的首要判据。
pub const CURSOR_PREFIX: &str = "> ";
/// 其余行的前缀——与 [`CURSOR_PREFIX`] **等宽**，否则整列文字会随光标
/// 上下移动而左右抖动。
pub const IDLE_PREFIX: &str = "  ";

/// 压暗整屏的背板颜色（RGBA，`0.0..=1.0`）。
///
/// 它不只是装饰：这一层是「世界现在不动了」这个事实**唯一的视觉表达**。
/// 玩家看到画面压暗，就知道按方向键不会再走路——没有它，一块居中面板
/// 看起来与 HUD 的背包面板没有区别，而两者的语义完全相反（见模块文档
/// 那张表）。
const BACKDROP_COLOR: [f32; 4] = [0.02, 0.02, 0.04, 0.72];

/// 一块模态屏这一帧要显示的全部内容——形状与
/// [`crate::hud::PanelContent`] 平行（背景矩形 + 文本行），只是多一层
/// 压暗背板。
pub struct ScreenContent {
    /// 压暗整屏的背板矩形，恒为整个窗口大小。
    pub backdrop: Rect,
    /// 居中面板的背景矩形。
    pub panel: Rect,
    /// 这一帧面板内容的全部文本行。
    pub labels: Vec<Label>,
}

/// 一块模态屏这一帧要显示的内容——调用方（`ll-game`）已经把每一行排好
/// 版，本模块只负责摆位置、加光标标记。
pub struct ScreenData<'a> {
    /// 屏幕标题的 Fluent 键。
    pub title_key: &'a str,
    /// 全部行，已由调用方排好版（见模块文档「为什么只有一种屏」）。
    pub rows: &'a [String],
    /// 光标落在第几行。超出 `rows` 范围时不标记任何一行——不钳制、也
    /// 不 panic，与 [`crate::hud::action_menu::ActionMenuData::cursor`]
    /// 同一条理由：钳制会掩盖调用方的光标维护缺陷，panic 会让一个纯
    /// 显示问题拖垮整个游戏。
    pub cursor: usize,
    /// 列表为空时显示的占位行的 Fluent 键。
    pub empty_key: &'a str,
    /// 面板底部操作提示行的 Fluent 键。
    pub hint_key: &'a str,
    /// 额外的一行状态/错误提示（**已经解析好**的字符串，`None` 表示
    /// 这一帧没有话要说）。
    ///
    /// 收已解析的字符串而不是 Fluent 键：键位冲突那句话要带参数
    /// （「这个键已经绑给了 X」中的 X 是另一个动作的显示名），参数注入
    /// 是持有 `Catalog` 与领域数据的调用方的事，与
    /// [`crate::hud::render::build_hud_frame`] 的 `feedback` 参数同一条
    /// 理由。
    pub notice: Option<&'a str>,
}

/// 居中一块 [`SCREEN_WIDTH`] 宽、高度按内容行数现算的面板。
///
/// 纵向也居中：面板高度随行数变化（菜单三行、设置二十几行），按内容
/// 高度算出来再居中，比写死一个 y 坐标更不容易在行数变化后错位。
/// `screen_height` 比面板还矮这种极端窗口尺寸下会算出负坐标——**不
/// 钳制**，与 [`crate::hud::render`] 的 `equipment_origin_x` 同一条
/// 取舍：钳制会掩盖「窗口配置改小了却没人发现面板被塞没了」这种应该
/// 显形的问题。
fn centered_origin(screen_width: f32, screen_height: f32, panel_height: f32) -> (f32, f32) {
    (
        (screen_width - SCREEN_WIDTH) / 2.0,
        (screen_height - panel_height) / 2.0,
    )
}

/// 产出一块模态屏的全部文本行——纯函数，不接触 GPU，供本模块测试与
/// `ll-game` 的验收测试直接断言「光标真的落在第几行」「某一行真的写着
/// 什么」。
///
/// 与 [`crate::hud::action_menu::action_menu_lines`] 同一条纪律：把
/// 「算出要显示什么」与「把它提交给 GPU」拆成两步，前一半才测得动
/// （ADR 0025 禁止用合成按键验收，程序化断言是本项目的默认验证手段）。
pub fn screen_lines(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    origin: (f32, f32),
    line_height: f32,
) -> Vec<Label> {
    let mut cursor = RowCursor::new(origin, line_height);
    let mut lines = Vec::new();
    write_screen_lines(data, catalog, language, &mut cursor, &mut lines);
    lines
}

fn write_screen_lines(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    cursor: &mut RowCursor,
    lines: &mut Vec<Label>,
) {
    cursor.push(lines, catalog.resolve(language, data.title_key));
    if data.rows.is_empty() {
        cursor.push(
            lines,
            format!("{IDLE_PREFIX}{}", catalog.resolve(language, data.empty_key)),
        );
    } else {
        for (row, text) in data.rows.iter().enumerate() {
            let prefix = if row == data.cursor {
                CURSOR_PREFIX
            } else {
                IDLE_PREFIX
            };
            cursor.push(lines, format!("{prefix}{text}"));
        }
    }
    if let Some(notice) = data.notice {
        cursor.push(lines, notice.to_string());
    }
    cursor.push(lines, catalog.resolve(language, data.hint_key));
}

/// 建出整块模态屏：压暗背板 + 居中面板 + 全部文本行。
pub fn build_screen_panel(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    screen_width: f32,
    screen_height: f32,
) -> ScreenContent {
    // 先按原点 (0, 0) 排一遍只为量出高度，再把量出来的高度拿去居中、
    // 用真正的原点重排一遍。两遍的代价是几十次字符串格式化，换来的是
    // 「面板高度恒等于内容高度」这条不变式不需要任何调用方去手算行数。
    let probe = screen_lines(data, catalog, language, (0.0, 0.0), SCREEN_LINE_HEIGHT);
    let panel_height = probe.len() as f32 * SCREEN_LINE_HEIGHT + SCREEN_PADDING * 2.0;
    let origin = centered_origin(screen_width, screen_height, panel_height);
    let content_origin = (origin.0 + SCREEN_PADDING, origin.1 + SCREEN_PADDING);
    let labels = screen_lines(data, catalog, language, content_origin, SCREEN_LINE_HEIGHT);
    ScreenContent {
        backdrop: Rect::new(0.0, 0.0, screen_width, screen_height),
        panel: Rect::new(origin.0, origin.1, SCREEN_WIDTH, panel_height),
        labels,
    }
}

/// 压暗背板这一帧的填色矩形——恒一块，恒 [`BACKDROP_COLOR`]。
pub fn backdrop_quad(rect: Rect) -> crate::widget::quad::QuadInstance {
    crate::widget::quad::QuadInstance {
        position: [rect.x, rect.y],
        size: [rect.width, rect.height],
        color: BACKDROP_COLOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn 测试目录() -> Catalog {
        Catalog::load_dir(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/locales"
        )))
    }

    fn 测试数据<'a>(rows: &'a [String], cursor: usize) -> ScreenData<'a> {
        ScreenData {
            title_key: "screen-menu-title",
            rows,
            cursor,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice: None,
        }
    }

    #[test]
    fn 光标那一行带标记其余行不带() {
        // Arrange
        let rows = vec!["甲".to_string(), "乙".to_string(), "丙".to_string()];
        let data = 测试数据(&rows, 1);

        // Act
        let lines = screen_lines(&data, &测试目录(), "zh-CN", (0.0, 0.0), 18.0);

        // Assert：第 0 行是标题，所以第 1 条行内容在下标 1。
        assert!(lines[1].text.starts_with(IDLE_PREFIX));
        assert!(lines[2].text.starts_with(CURSOR_PREFIX));
        assert!(lines[3].text.starts_with(IDLE_PREFIX));
    }

    #[test]
    fn 光标越界时没有任何一行带标记() {
        // Arrange
        let rows = vec!["甲".to_string()];
        let data = 测试数据(&rows, 9);

        // Act
        let lines = screen_lines(&data, &测试目录(), "zh-CN", (0.0, 0.0), 18.0);

        // Assert
        assert!(
            !lines
                .iter()
                .any(|line| line.text.starts_with(CURSOR_PREFIX))
        );
    }

    #[test]
    fn 提示文字为空时不占一行() {
        // Arrange
        let rows = vec!["甲".to_string()];
        let 无提示 = 测试数据(&rows, 0);
        let mut 有提示 = 测试数据(&rows, 0);
        有提示.notice = Some("出事了");

        // Act
        let 无 = screen_lines(&无提示, &测试目录(), "zh-CN", (0.0, 0.0), 18.0);
        let 有 = screen_lines(&有提示, &测试目录(), "zh-CN", (0.0, 0.0), 18.0);

        // Assert
        assert_eq!(有.len(), 无.len() + 1);
    }

    #[test]
    fn 面板高度随行数增长() {
        // 「面板背景比文字短一截」是这条不变式没守住时的典型症状。
        // Arrange
        let 少 = vec!["甲".to_string()];
        let 多 = vec!["甲".to_string(), "乙".to_string(), "丙".to_string()];

        // Act
        let 矮 = build_screen_panel(&测试数据(&少, 0), &测试目录(), "zh-CN", 1280.0, 720.0);
        let 高 = build_screen_panel(&测试数据(&多, 0), &测试目录(), "zh-CN", 1280.0, 720.0);

        // Assert
        assert_eq!(高.panel.height - 矮.panel.height, 2.0 * SCREEN_LINE_HEIGHT);
    }

    #[test]
    fn 面板在屏幕上居中() {
        // Arrange
        let rows = vec!["甲".to_string()];

        // Act
        let content = build_screen_panel(&测试数据(&rows, 0), &测试目录(), "zh-CN", 1280.0, 720.0);

        // Assert：左右边距相等即为横向居中。
        let 右边距 = 1280.0 - (content.panel.x + content.panel.width);
        assert!((content.panel.x - 右边距).abs() < f32::EPSILON);
    }

    #[test]
    fn 背板铺满整个窗口() {
        // 这是「世界现在不动了」唯一的视觉表达，见 BACKDROP_COLOR 文档。
        // Arrange
        let rows = vec!["甲".to_string()];

        // Act
        let content = build_screen_panel(&测试数据(&rows, 0), &测试目录(), "zh-CN", 1280.0, 720.0);

        // Assert
        assert_eq!(content.backdrop.width, 1280.0);
        assert_eq!(content.backdrop.height, 720.0);
    }

    #[test]
    fn 背板是半透明的不是全遮() {
        // 全遮会让玩家彻底看不见自己站在哪；全透明则看不出世界停了。
        // Act
        let quad = backdrop_quad(Rect::new(0.0, 0.0, 10.0, 10.0));

        // Assert
        assert!(quad.color[3] > 0.0 && quad.color[3] < 1.0);
    }

    #[test]
    fn 行为空时显示占位行而不是什么都不画() {
        // Arrange
        let rows: Vec<String> = Vec::new();
        let data = 测试数据(&rows, 0);

        // Act
        let lines = screen_lines(&data, &测试目录(), "zh-CN", (0.0, 0.0), 18.0);

        // Assert：标题 + 占位行 + 提示行。
        assert_eq!(lines.len(), 3);
    }
}
