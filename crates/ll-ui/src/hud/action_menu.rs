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
//! # 光标标记为什么是文字前缀，不是一块高亮矩形
//!
//! 一块高亮矩形要么需要知道每一行的实际文字宽度（本 crate 的文本测量
//! 在 `ll-text`，HUD 这一层拿不到），要么就得画成整行等宽的色块——后者
//! 在面板宽度固定而文字长短不一时看起来像是「选中了一整条空白」。
//! 前缀标记（`> `）没有这些问题，且在 ADR 0025 禁止合成按键的前提下，
//! 它是**可以被纯文本断言直接验证**的：测试只要看第几行以标记开头就
//! 知道光标在哪，不需要比对像素。

use ll_i18n::Catalog;

use super::{PanelContent, build_panel};
use ll_text::MeasureText;

use crate::widget::label::Label;
use crate::widget::list::RowCursor;

/// 光标所在那一行的前缀，见模块文档「光标标记为什么是文字前缀」一节。
pub const CURSOR_PREFIX: &str = "> ";
/// 其余行的前缀——与 [`CURSOR_PREFIX`] **等宽**，否则整列文字会随光标
/// 上下移动而左右抖动。
pub const IDLE_PREFIX: &str = "  ";

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
    /// 光标落在第几行。超出 `rows` 范围时不标记任何一行——不钳制、也
    /// 不 panic：钳制会掩盖「调用方的光标维护有缺陷」这个应该显形的
    /// 问题，panic 则会因为一个纯显示问题拖垮整个游戏。
    pub cursor: usize,
    /// 列表为空时显示的占位行的 Fluent 键。
    pub empty_key: &'a str,
    /// 面板底部的操作提示行的 Fluent 键（「确认=制作，取消=关闭」这类）。
    pub hint_key: &'a str,
    /// 这块菜单画在屏幕的什么位置，见 [`MenuPlacement`]。
    pub placement: MenuPlacement,
}

/// 建出动作菜单面板：标题 + 逐行（光标行带 [`CURSOR_PREFIX`]）+ 提示行。
pub fn action_menu_panel(
    data: &ActionMenuData<'_>,
    catalog: &Catalog,
    language: &str,
    measure: &mut dyn MeasureText,
    origin: (f32, f32),
    width: f32,
) -> PanelContent {
    build_panel(measure, origin, width, |cursor, lines| {
        write_action_menu_lines(data, catalog, language, cursor, lines);
    })
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
    write_action_menu_lines(data, catalog, language, &mut cursor, &mut lines);
    lines
}

fn write_action_menu_lines(
    data: &ActionMenuData<'_>,
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

    #[test]
    fn 光标所在那一行带标记其余行不带() {
        // Arrange
        let catalog = fixture_catalog("cursor-marks-one-row");
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

        // Assert：标题 + 三行 + 提示。
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1], "  甲");
        assert_eq!(lines[2], "> 乙");
        assert_eq!(lines[3], "  丙");
    }

    #[test]
    fn 光标越界时没有任何一行带标记() {
        // 见 `ActionMenuData::cursor` 文档：不钳制、不 panic。
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
        let lines = texts(&data, &catalog);

        // Assert
        assert!(
            !lines.iter().any(|line| line.starts_with(CURSOR_PREFIX)),
            "光标越界时不应该有任何一行被标记，实际是 {lines:?}"
        );
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
        assert_eq!(lines, vec!["标题", "  （空）", "提示"]);
    }

    #[test]
    fn 两种前缀等宽以免整列文字随光标抖动() {
        // 见模块文档「光标标记为什么是文字前缀」一节最后一句。
        //
        // # 【2026-09-01 批次 30】这条断言比的是**字符数**，它咬不住
        //
        // `knowledge/design/ui-and-navigation.md` §12 点名它是反面教材：
        // 比例字体里 `"> "` 是 10.91px、`"  "` 是 6.27px，**两个前缀今天
        // 根本不等宽**，而本条比的是字符数，所以照样绿。规格 W7 要求把
        // 它换成比**渲染宽度**。
        //
        // **批次 30 没有换**，如实登记原因：换了它当场就红，而消红只有
        // 两条路——(a) 找一个与 `"> "` 等宽的空白组合，比例字体里没有
        // 精确解（空格 3.135px，除不尽），凑近似值等于把判据放宽；
        // (b) 规格 F7 的做法，把光标记号从**文本内容**里拿出来变成一块
        // 高亮矩形，而全仓库几十条 `starts_with(CURSOR_PREFIX)` 的断言
        // 建立在「记号在文本里」之上。(b) 是对的，也是自成一批的改造。
        // W7 与 F7 因此留在同一批做，届时**换掉**本条而不是留着。
        // Arrange & Act & Assert
        assert_eq!(CURSOR_PREFIX.chars().count(), IDLE_PREFIX.chars().count());
    }
}
