//! 动作菜单面板：一列可选行 + 一个光标 + 一行提示。
//!
//! # 这块面板为什么存在
//!
//! HUD 此前是**纯只读**的（见 [`super`] 模块文档「只读，不做任何交互」
//! 一节）。那条纪律在「只读观测层」那一批成立，但它同时意味着
//! `ll_sim::intent::Intent` 里需要**先选一条**的那些玩法意图
//! （`Craft { recipe }`/`Drop { def }`/`Equip { def }`/
//! `Unequip { slot }`/`Use { def }`）在真实游戏里一个都提交不出来——
//! 玩家没有任何途径表达「我要做的是列表里的第三条」。本面板就是那条
//! 途径：它是 HUD 里第一块**有选中态**的面板。
//!
//! # 为什么只收已经排好版的字符串，不收领域类型
//!
//! 本模块刻意**不认识** `ItemStack`/`RecipeView`/`EquipSlot`：它收到的
//! 是一列 [`String`] 和一个光标下标，画出来就完事。理由是这块面板真正
//! 共用的算法只有「一列行 + 一个高亮标记 + 现算面板高度」这一段，而
//! 「一堆铁锭该显示成什么字」与「一条配方该显示成什么字」是两套完全
//! 不同的取数逻辑（一个查 `ItemTable`，一个查 `RecipeTable` 还要连带
//! 列出食材），把它们塞进同一个模块只会得到一个带 `enum` 分支的四不像
//! ——ADR 0021 的反面。排版由持有全部内容表的那一层（`ll-game`）做，
//! 与 [`super::inventory_panel`] 自己查 `ItemTable` 并不矛盾：那块是
//! **常驻只读**面板，本块是**通用选择器**，两者服务的问题不同。
//!
//! # 光标标记是一块高亮矩形，不是文字前缀（规格 W7 / F7）
//!
//! **此前是文字前缀**：光标那一行拼上 `"> "`、其余行拼上两个空格。那条
//! 取舍当时写的理由是「高亮矩形要么拿不到文字宽度、要么画成整行色块」，
//! 而它有一个说错了的前提——**内嵌字体是比例字体，`"> "` 与两个空格不
//! 等宽**（10.91px 对 6.27px）。于是光标每上下移动一格，整列文字就左右
//! 抖动 4.6px；`knowledge/design/ui-and-navigation.md` §8.5 W7 / §9.3 F7
//! 记的正是这件事，§12 把守着那条「等宽」的断言点名成反面教材——它比的
//! 是**字符数**，两个前缀字符数相同，所以从落地那天起就没有咬住过。
//!
//! 「找一个与 `"> "` 等宽的空白组合」在比例字体里没有精确解（空格
//! 3.135px，除不尽），凑近似值等于把判据从「相等」放宽成「差不多」。
//! 因此走另一条：**把光标记号从文本内容里拿出来**，改成一块高亮矩形。
//!
//! 「拿不到文字宽度」那个前提也已经消失——规格 W1 落地后本模块本来就
//! 收着一个 [`MeasureText`]（面板宽度按内容现算要用它），行矩形因此与
//! 行文字**出自同一次游标推进**（[`action_menu_content`]），不是按
//! 「第 i 行 = i × 行高」反算出来的：那条公式在长行换行之后会静悄悄
//! 错开一格，与 `crate::screen` 的 `layout_screen` 当初修的是同一个病。
//!
//! **可断言性一点没丢**（ADR 0025 禁止合成按键，程序化断言是本项目的
//! 默认验证手段）：测试改为在渲染帧里找那一块高亮 quad，断言它落在
//! `row_rects[cursor]` 上——这比看第几行以什么开头**更接近玩家真的看到
//! 了什么**，因为文字前缀只是选中态的一个代理，高亮矩形就是选中态本身。

use ll_i18n::Catalog;

use super::{PanelContent, build_panel};
use ll_text::MeasureText;

use crate::widget::geometry::Rect;
use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 一块动作菜单画在屏幕的什么位置。
///
/// # 为什么这条要由调用方声明，不是本层一律拍一个位置
///
/// 三块菜单（背包、制作、交互）**共用同一条渲染路径**
/// （`crate::hud::render::build_hud_frame` 的 `menu` 参数是一个
/// `Option<&ActionMenuData>`，认不出打开的是哪一块）。所有者试玩后只
/// 要求**交互**那一块挪到屏幕正中：
///
/// > 「那个互动显示的 UI 窗口，我希望是出现在屏幕正中间」
///
/// 在渲染层写死「一律居中」会把背包与制作一并挪走，那是替所有者做了他
/// 没提的决定。把位置做成**数据的一部分**，让持有 `PlayerMenu` 的那一
/// 层（`ll_game::player_action::menu_data`）逐个变体声明，是唯一既满足
/// 要求、又不牵连另外两块的形状。
///
/// 这不是「为对称而抽象」（ADR 0021）：两个变体今天各自都有真实使用
/// 者，不是一个只有一种取值的枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPlacement {
    /// 水平居中、垂直贴着屏幕上沿（留出与常驻面板同款的外边距）。
    ///
    /// **这是本字段落地之前唯一存在的行为**，背包与制作两块菜单原样
    /// 保留在这里——本次改动对它们是逐像素零变更。
    TopCenter,
    /// 水平与垂直**都**居中。所有者对交互窗口的要求。
    ScreenCenter,
}

/// 一块动作菜单这一帧要显示的全部内容。
pub struct ActionMenuData<'a> {
    /// 面板标题的 Fluent 键。
    pub title_key: &'a str,
    /// 全部可选行，已由调用方排好版（见模块文档）。
    pub rows: &'a [String],
    /// 光标落在第几行——**这一行会被画上一块高亮矩形**
    /// （[`ActionMenuContent::row_rects`]）。超出 `rows` 范围时不高亮
    /// 任何一行——不钳制、也不 panic：钳制会掩盖「调用方的光标维护有
    /// 缺陷」这个应该显形的问题，panic 则会因为一个纯显示问题拖垮整个
    /// 游戏。
    pub cursor: usize,
    /// 列表为空时显示的占位行的 Fluent 键。
    pub empty_key: &'a str,
    /// 面板底部的操作提示行的 Fluent 键（「确认=制作，取消=关闭」这类）。
    pub hint_key: &'a str,
    /// 这块菜单画在屏幕的什么位置，见 [`MenuPlacement`]。
    pub placement: MenuPlacement,
}

/// 一块动作菜单排完版之后的全部几何：面板本体 + **每一个可选行的
/// 矩形**。
///
/// 行矩形与行文字出自同一次游标推进（见模块文档最后两段），高亮画在
/// 哪儿因此不可能与文字画在哪儿对不上。形状与 `crate::screen` 的
/// `ScreenContent::row_rects` 平行。
pub struct ActionMenuContent {
    /// 面板背景矩形与全部文本行。
    pub panel: PanelContent,
    /// 每一个**可选行**的矩形，下标与 [`ActionMenuData::rows`] 一一
    /// 对应——标题行、占位行与提示行都不在里面（它们不是可选项）。
    pub row_rects: Vec<Rect>,
}

/// 建出动作菜单面板：标题 + 逐行 + 提示行，外加每一个可选行的矩形。
pub fn action_menu_content(
    data: &ActionMenuData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn MeasureText,
    origin: (f32, f32),
    width: f32,
) -> ActionMenuContent {
    let mut row_rects = Vec::new();
    let panel = build_panel(measure, origin, width, |cursor, lines| {
        write_action_menu_lines(data, catalog, language, cursor, lines, &mut row_rects);
    });
    ActionMenuContent { panel, row_rects }
}

/// 产出动作菜单的全部文本行——纯函数，不接触 GPU，供本模块测试与
/// `ll-game` 的验收测试直接断言「光标真的落在第几行」。
pub fn action_menu_lines(
    data: &ActionMenuData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn MeasureText,
    origin: (f32, f32),
    line_height: f32,
    wrap_width: f32,
) -> Vec<Label> {
    let mut cursor = RowCursor::new(
        measure,
        origin,
        line_height,
        super::DEFAULT_FONT_SIZE,
        wrap_width,
    );
    let mut lines = Vec::new();
    write_action_menu_lines(
        data,
        catalog,
        language,
        &mut cursor,
        &mut lines,
        &mut Vec::new(),
    );
    lines
}

fn write_action_menu_lines(
    data: &ActionMenuData<'_>,
    catalog: &Catalog,
    language: &str,
    cursor: &mut RowCursor,
    lines: &mut Vec<Label>,
    row_rects: &mut Vec<Rect>,
) {
    cursor.push(lines, catalog.resolve(language, data.title_key));
    if data.rows.is_empty() {
        // 占位行**不是可选行**，因此不产出行矩形——列表空的时候没有
        // 任何一行会被高亮，与 `data.cursor` 越界时同一个结果。
        cursor.push(lines, catalog.resolve(language, data.empty_key));
    } else {
        for text in data.rows {
            // 一行的矩形就是这一行推进前后的那一段，横向占满内容区
            // ——与 `crate::screen` 的 `layout_screen` 逐字同一条纪律。
            let top = cursor.cursor_y();
            let x = cursor.x();
            let width = cursor.wrap_width();
            cursor.push(lines, text.clone());
            row_rects.push(Rect::new(x, top, width, cursor.cursor_y() - top));
        }
    }
    cursor.push(lines, catalog.resolve(language, data.hint_key));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_fixture_catalog(dir: &Path) {
        std::fs::write(
            dir.join("zh-CN.ftl"),
            "menu-title = 标题\nmenu-empty = （空）\nmenu-hint = 提示\n",
        )
        .expect("测试用写入应当成功");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ll-ui-hud-action-menu-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        dir
    }

    fn fixture_catalog(name: &str) -> Catalog {
        let dir = temp_dir(name);
        write_fixture_catalog(&dir);
        Catalog::load_one(crate::TEST_LOCALE_NAMESPACE, &dir)
    }

    fn texts(data: &ActionMenuData<'_>, catalog: &Catalog) -> Vec<String> {
        action_menu_lines(
            data,
            catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            (0.0, 0.0),
            10.0,
            crate::测试断行宽,
        )
        .into_iter()
        .map(|label| label.text)
        .collect()
    }

    /// 建一块动作菜单，返回面板 + 行矩形——测试共用的那一次调用。
    fn content(data: &ActionMenuData<'_>, catalog: &Catalog) -> ActionMenuContent {
        action_menu_content(
            data,
            catalog,
            "zh-CN",
            &mut crate::测试测量器(),
            (0.0, 0.0),
            crate::测试断行宽 + super::super::DEFAULT_PADDING * 2.0,
        )
    }

    #[test]
    fn 行文字里不再有任何光标记号() {
        // 规格 W7：光标记号从**文本内容**里拿出来了，行文字就是调用方
        // 给的那一串，一个字符都不多。
        //
        // 反例验证（已实跑）：把 `write_action_menu_lines` 的
        // `text.clone()` 改回 `format!("> {text}")`，本条红在第 2 行。
        // Arrange
        let catalog = fixture_catalog("no-prefix-in-text");
        let rows = vec!["甲".to_string(), "乙".to_string(), "丙".to_string()];
        let data = ActionMenuData {
            title_key: "menu-title",
            rows: &rows,
            cursor: 1,
            empty_key: "menu-empty",
            hint_key: "menu-hint",
            placement: MenuPlacement::TopCenter,
        };

        // Act
        let lines = texts(&data, &catalog);

        // Assert：标题 + 三行 + 提示，行文字逐字等于原文。
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1], "甲");
        assert_eq!(lines[2], "乙");
        assert_eq!(lines[3], "丙");
    }

    #[test]
    fn 每一个可选行都有一块正对着它自己那行文字的矩形() {
        // **这是「拔掉文本前缀之后，哪一行被选中仍然验得出来」的地基**：
        // 高亮画在 `row_rects[cursor]` 上，而这条断言保证那一批矩形与
        // 行文字是同一次游标推进的产物。
        //
        // 反例验证（已实跑）：把 `write_action_menu_lines` 里的
        // `row_rects.push(...)` 删掉，本条红在「行矩形条数 0 ≠ 3」。
        // Arrange
        let catalog = fixture_catalog("row-rects-match-lines");
        let rows = vec!["甲".to_string(), "乙".to_string(), "丙".to_string()];
        let data = ActionMenuData {
            title_key: "menu-title",
            rows: &rows,
            cursor: 1,
            empty_key: "menu-empty",
            hint_key: "menu-hint",
            placement: MenuPlacement::TopCenter,
        };

        // Act
        let built = content(&data, &catalog);

        // Assert：先确认对象真的存在（空集会让下面的循环恒绿）。
        assert_eq!(
            built.row_rects.len(),
            rows.len(),
            "每一个可选行恰好一块矩形"
        );
        for (i, rect) in built.row_rects.iter().enumerate() {
            // 第 0 行是标题，所以第 i 个可选行的标签在下标 i + 1。
            let label = &built.panel.labels[i + 1];
            assert_eq!(label.text, rows[i]);
            assert_eq!(rect.y, label.y, "第 {i} 行的矩形顶边应对着它的文字");
            assert!(rect.height > 0.0, "第 {i} 行的矩形不该是零高");
            assert_eq!(rect.width, label.max_width, "行矩形横向占满内容区");
        }
    }

    #[test]
    fn 列表为空时一块行矩形都没有() {
        // 占位行不是可选项——没有任何一行会被高亮。
        // Arrange
        let catalog = fixture_catalog("empty-has-no-row-rects");
        let data = ActionMenuData {
            title_key: "menu-title",
            rows: &[],
            cursor: 0,
            empty_key: "menu-empty",
            hint_key: "menu-hint",
            placement: MenuPlacement::TopCenter,
        };

        // Act
        let built = content(&data, &catalog);

        // Assert
        assert!(built.row_rects.is_empty());
        assert_eq!(
            built
                .panel
                .labels
                .iter()
                .map(|label| label.text.clone())
                .collect::<Vec<_>>(),
            vec!["标题", "（空）", "提示"]
        );
    }

    #[test]
    fn 光标越界时取不到任何一块行矩形() {
        // 见 `ActionMenuData::cursor` 文档：不钳制、不 panic。此前这条
        // 验的是「没有一行以 `"> "` 开头」——那条判据随文本前缀一起
        // 作废了，换成验「取不到高亮该画的那块矩形」。
        // Arrange
        let catalog = fixture_catalog("cursor-out-of-range");
        let rows = vec!["甲".to_string()];
        let data = ActionMenuData {
            title_key: "menu-title",
            rows: &rows,
            cursor: 7,
            empty_key: "menu-empty",
            hint_key: "menu-hint",
            placement: MenuPlacement::TopCenter,
        };

        // Act
        let built = content(&data, &catalog);

        // Assert：先确认这块菜单真的有行（否则「取不到」恒真）。
        assert_eq!(built.row_rects.len(), 1);
        assert!(built.row_rects.get(data.cursor).is_none());
    }

    #[test]
    fn 空列表显示占位行且仍有标题与提示() {
        // Arrange
        let catalog = fixture_catalog("empty-list");
        let data = ActionMenuData {
            title_key: "menu-title",
            rows: &[],
            cursor: 0,
            empty_key: "menu-empty",
            hint_key: "menu-hint",
            placement: MenuPlacement::TopCenter,
        };

        // Act
        let lines = texts(&data, &catalog);

        // Assert
        assert_eq!(lines, vec!["标题", "（空）", "提示"]);
    }
}
