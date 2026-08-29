//! 游戏内暂停菜单（继续 / 保存 / 设置 / 返回主菜单 / 退出）的状态机。
//!
//! # 为什么从 [`crate::menu_screen`] 里搬出来
//!
//! 与批次 6 把首页搬进 [`crate::title_screen`] 逐字同一条理由：
//! `menu_screen.rs` 已经逼近本仓库 800 行的文件上限，本批次给暂停菜单
//! 加两项（「保存」与「返回主菜单」）之后它到了 910 行，越过上限。
//!
//! 共用的类型（[`ScreenState`]、[`ScreenOutcome`]、`ScreenNotice`、
//! [`SettingsOrigin`]）仍然只有一份，住在 `menu_screen`——**拆的是文件，
//! 不是职责**。
//!
//! # 这块屏与首页的分界
//!
//! 暂停菜单**底下有一局正在进行的世界**，首页没有。这个差别决定了这里
//! 才有「保存」与「返回主菜单」两项，而首页有「开始游戏」与「读取存档」。

use ll_platform::input::{GameKey, InputState};
use ll_ui::widget::focus::navigate_focus;
use ll_ui::widget::state::{WidgetId, WidgetStateTable};

use crate::menu_screen::{ScreenOutcome, ScreenState, SettingsOrigin, focus_index};

/// 暂停菜单的一行是什么。**每帧现算**（见 [`menu_rows`]），不缓存。
///
/// 从一张编译期静态数组改成一个枚举 + 现算列表，是因为
/// [`MenuRow::Save`] 这一行**在肉鸽模式下根本不出现**（所有者裁定：
/// 肉鸽只有自动保存），行数因此不再固定。形状照
/// [`crate::menu_screen::settings_rows`] 的既有做法，不发明第二种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRow {
    /// 关掉菜单回到游戏。
    Continue,
    /// 手动存一次档——**只有普通模式才有这一行**。
    Save,
    /// 打开设置屏。
    Settings,
    /// 回到游戏主菜单（首页）。**回去之前会先存一次**，见
    /// `crate::app::Demo::back_to_title`。
    BackToTitle,
    /// 退出整个进程。
    Quit,
}

impl MenuRow {
    /// 这一行的控件 id——顺序即导航顺序（见
    /// [`ll_ui::widget::focus::move_focus`] 文档「列表顺序即导航顺序」）。
    pub fn widget_id(self) -> WidgetId {
        match self {
            MenuRow::Continue => "screen.menu.continue",
            MenuRow::Save => "screen.menu.save",
            MenuRow::Settings => "screen.menu.settings",
            MenuRow::BackToTitle => "screen.menu.back-to-title",
            MenuRow::Quit => "screen.menu.quit",
        }
    }

    /// 这一行的 Fluent 键。
    pub fn text_key(self) -> &'static str {
        match self {
            MenuRow::Continue => "screen-menu-continue",
            MenuRow::Save => "screen-menu-save",
            MenuRow::Settings => "screen-menu-settings",
            MenuRow::BackToTitle => "screen-menu-back-to-title",
            MenuRow::Quit => "screen-menu-quit",
        }
    }
}

/// 暂停菜单这一帧的全部行，顺序固定。
///
/// `can_save_manually` 应当由 [`ll_content::world_identity::WorldIdentity::allows_manual_save`]
/// 给出——**UI 层不自己 `match` 存档模式**，判据只有那一处。
///
/// # 为什么「保存」是整行消失，不是置灰
///
/// `ll_ui::screen::ScreenData` 今天没有「逐行禁用样式」这个概念（批次 6
/// 第 4.2 节论证过，加它要动数据形状与配色）。而这一项的**缺席本身**
/// 就是模式的可见后果：肉鸽玩家看不到手动存档，正是这个模式的全部意思。
pub fn menu_rows(can_save_manually: bool) -> Vec<MenuRow> {
    let mut rows = vec![MenuRow::Continue];
    if can_save_manually {
        rows.push(MenuRow::Save);
    }
    rows.push(MenuRow::Settings);
    rows.push(MenuRow::BackToTitle);
    rows.push(MenuRow::Quit);
    rows
}

/// 这一帧菜单屏的控件 id 列表，与 [`menu_rows`] 逐条对应。
pub fn menu_item_ids(can_save_manually: bool) -> Vec<WidgetId> {
    menu_rows(can_save_manually)
        .into_iter()
        .map(MenuRow::widget_id)
        .collect()
}

/// 菜单屏当前聚焦的是第几行，见 [`focus_index`]。
pub fn menu_focus_index(table: &WidgetStateTable, can_save_manually: bool) -> usize {
    focus_index(table, &menu_item_ids(can_save_manually))
}

/// 处理菜单屏这一帧的输入。
pub fn update_menu(
    table: &mut WidgetStateTable,
    input: &InputState,
    can_save_manually: bool,
) -> (ScreenOutcome, Option<ScreenState>) {
    let rows = menu_rows(can_save_manually);
    let ids = menu_item_ids(can_save_manually);
    navigate_focus(table, &ids, input);
    if input.was_just_pressed(GameKey::Cancel) {
        return (ScreenOutcome::Close, None);
    }
    if !input.was_just_pressed(GameKey::Confirm) {
        return (ScreenOutcome::Idle, None);
    }
    // 按**行的语义**分支，不按下标——行数随模式变化，写死下标就是
    // 「肉鸽模式下按『设置』结果退出了游戏」这种缺陷的形状。
    let Some(row) = rows.get(focus_index(table, &ids)) else {
        // 还没选中任何一项（光标为 usize::MAX）时按确认——什么都不做，
        // 不猜一个默认项。
        return (ScreenOutcome::Idle, None);
    };
    match row {
        MenuRow::Continue => (ScreenOutcome::Close, None),
        MenuRow::Save => (ScreenOutcome::SaveNow, None),
        MenuRow::Settings => (
            ScreenOutcome::Idle,
            Some(ScreenState::Settings {
                cursor: 0,
                capturing: false,
                origin: SettingsOrigin::Menu,
            }),
        ),
        MenuRow::BackToTitle => (ScreenOutcome::BackToTitle, None),
        MenuRow::Quit => (ScreenOutcome::Quit, None),
    }
}
