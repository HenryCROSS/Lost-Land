//! 游戏主菜单（首页）的行为断言。
//!
//! # 为什么住在 `tests/` 而不是 `title_screen.rs` 里
//!
//! 与 `menu_and_settings.rs` 同一条理由（本仓库 800 行的文件上限），外加
//! 同一个额外好处：这里只摸得到 `pub` 的东西，断言因此**必须走玩家真正
//! 走的那条公开路径**（`update_title`），而不是抄近路去调内部函数。
//!
//! ADR 0025 禁止用合成按键做验收，这里的每一条都是程序化驱动同一条调用
//! 路径，不模拟任何键盘事件。

use std::path::Path;

use ll_game::menu_screen::{ScreenNotice, ScreenOutcome, ScreenState, SettingsOrigin};
use ll_game::settings_view::title_row_texts;
use ll_game::title_screen::{TITLE_ITEM_IDS, TITLE_LOAD_ROW, title_focus_index, update_title};
use ll_i18n::Catalog;
use ll_platform::input::{GameKey, InputState};
use ll_ui::widget::state::WidgetStateTable;

fn 测试目录() -> Catalog {
    Catalog::load_dir(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/locales"
    )))
}

fn 按下(keys: &[GameKey]) -> InputState {
    let mut input = InputState::new();
    for key in keys {
        input.press(*key);
    }
    input
}

/// 把焦点向下移到第 `index` 行——首页的焦点刻意不预置在任何一项上
/// （与菜单屏同一条约定），第一次按下会落在第 0 行。
fn 移到第几行(table: &mut WidgetStateTable, index: usize) {
    for _ in 0..=index {
        update_title(table, &按下(&[GameKey::Down]), true);
    }
    assert_eq!(title_focus_index(table), index, "Arrange 的焦点没落对地方");
}

#[test]
fn 首页有且只有四项() {
    // 所有者点名的四项：开始游戏 / 读取存档 / 设置 / 离开。少一项就
    // 是漏功能，多一项就是本批次越界了。
    // Arrange & Act
    let 行数 = TITLE_ITEM_IDS.len();

    // Assert
    assert_eq!(行数, 4);
}

#[test]
fn 首页四行文字全部解析成真正的译文() {
    // `Catalog::resolve` 查不到键会**原样返回键名**——那是一条静默
    // 降级路径，屏上会直接显示 `screen-title-new-game` 这种字符串。
    // 本条钉死四行都真的进了两份 .ftl。
    // Arrange
    let catalog = 测试目录();

    // Act
    let rows = title_row_texts(&catalog, "zh-CN", true);

    // Assert
    assert_eq!(rows.len(), 4);
    for row in &rows {
        assert!(
            !row.starts_with("screen-title-"),
            "这一行回落成了 Fluent 键名，说明 .ftl 里没有它：{row}"
        );
    }
}

#[test]
fn 没有存档时读取存档那一行换成另一句文案() {
    // 玩家必须能一眼看出这一行现在按不了，而不是按下去毫无反应。
    // Arrange
    let catalog = 测试目录();

    // Act
    let 有档 = title_row_texts(&catalog, "zh-CN", true);
    let 无档 = title_row_texts(&catalog, "zh-CN", false);

    // Assert
    assert_ne!(有档[TITLE_LOAD_ROW], 无档[TITLE_LOAD_ROW]);
    assert_eq!(
        无档[TITLE_LOAD_ROW],
        catalog.resolve("zh-CN", "screen-title-load-empty")
    );
    // 其余三行不该受影响。
    for index in 0..有档.len() {
        if index != TITLE_LOAD_ROW {
            assert_eq!(有档[index], 无档[index], "第 {index} 行不该跟着变");
        }
    }
}

#[test]
fn 没有存档时按读取存档只说一句话不进世界() {
    // **绝不**退而求其次地开一局新游戏：玩家点的是「读取存档」。
    // Arrange
    let mut table = WidgetStateTable::new();
    移到第几行(&mut table, TITLE_LOAD_ROW);

    // Act
    let update = update_title(&mut table, &按下(&[GameKey::Confirm]), false);

    // Assert
    assert_eq!(update.outcome, ScreenOutcome::Idle, "不该产生任何动作");
    assert_eq!(update.next, None, "不该切屏");
    assert_eq!(update.notice, Some(ScreenNotice::NoSave));
}

#[test]
fn 有存档时按读取存档要求调用方去读档() {
    // Arrange
    let mut table = WidgetStateTable::new();
    移到第几行(&mut table, TITLE_LOAD_ROW);

    // Act
    let update = update_title(&mut table, &按下(&[GameKey::Confirm]), true);

    // Assert
    assert_eq!(update.outcome, ScreenOutcome::LoadSave);
    assert_eq!(update.notice, None);
}

#[test]
fn 按开始游戏要求调用方建一局新世界() {
    // Arrange
    let mut table = WidgetStateTable::new();
    移到第几行(&mut table, 0);

    // Act
    let update = update_title(&mut table, &按下(&[GameKey::Confirm]), false);

    // Assert：**没有存档也能开始新游戏**——这一项与存档无关。
    assert_eq!(update.outcome, ScreenOutcome::StartNewGame);
}

#[test]
fn 按离开退出整局() {
    // Arrange
    let mut table = WidgetStateTable::new();
    移到第几行(&mut table, 3);

    // Act
    let update = update_title(&mut table, &按下(&[GameKey::Confirm]), true);

    // Assert
    assert_eq!(update.outcome, ScreenOutcome::Quit);
}

#[test]
fn 首页按取消什么都不做() {
    // 首页没有「上一层」可退。刻意不让取消键退出游戏——那正是上一批
    // 刚从游戏内改掉的行为（顶层按一下 Esc 整局就没了）。
    // Arrange
    let mut table = WidgetStateTable::new();
    移到第几行(&mut table, 3);

    // Act
    let update = update_title(&mut table, &按下(&[GameKey::Cancel]), true);

    // Assert
    assert_eq!(update.outcome, ScreenOutcome::Idle);
    assert_eq!(update.next, None);
}

#[test]
fn 还没选中任何一项时按确认什么都不做() {
    // 不猜一个默认项——猜错就是「我只是按了一下确认，游戏就把我的档
    // 覆盖了」。
    // Arrange：一次方向键都不按，焦点为空。
    let mut table = WidgetStateTable::new();
    assert_eq!(title_focus_index(&table), usize::MAX);

    // Act
    let update = update_title(&mut table, &按下(&[GameKey::Confirm]), true);

    // Assert
    assert_eq!(update.outcome, ScreenOutcome::Idle);
    assert_eq!(update.next, None);
    assert_eq!(update.notice, None);
}

#[test]
fn 从首页进的设置屏记着要回首页() {
    // 写死回 `ScreenState::Menu` 会把玩家扔进一个**底下没有世界**的
    // 暂停菜单，那块屏第一项是「继续游戏」，按下去会露出一个空世界。
    // Arrange
    let mut table = WidgetStateTable::new();
    移到第几行(&mut table, 2);

    // Act
    let update = update_title(&mut table, &按下(&[GameKey::Confirm]), true);

    // Assert
    assert_eq!(
        update.next,
        Some(ScreenState::Settings {
            cursor: 0,
            capturing: false,
            origin: SettingsOrigin::Title,
        })
    );
}
