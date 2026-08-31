//! 游戏主菜单（首页）的状态机：开始游戏 / 读取存档 / 设置 / 离开。
//!
//! # 这个模块补的是哪条断线
//!
//! 项目所有者原话：「既然是 P7，那么就需要进入游戏的首页，例如开始
//! 游戏，读取存档，设置，离开」「我需要一个游戏的主菜单，而不是开始
//! 直接进入存档」。此前游戏一启动就直接进世界（有存档读档、没有就
//! 建新档），玩家没有任何机会在进世界之前做选择。
//!
//! # 与 [`crate::menu_screen`] 的分界
//!
//! 首页与游戏内菜单在画法与导航上完全同构（都走
//! [`ll_ui::widget::focus::navigate_focus`] + 一张静态 id 表），区别
//! 只有一个，但那一个是根本性的：**首页底下没有世界**
//! （`crate::app::Demo::session` 为 `None`）。
//!
//! 拆成两个文件不是因为职责不同，是因为 `menu_screen.rs` 已经逼近本
//! 仓库 800 行的文件上限——两块屏的状态机塞在一个文件里会越过它。共用
//! 的类型（[`ScreenState`]、[`ScreenOutcome`]、[`ScreenNotice`]、
//! [`SettingsOrigin`]）仍然只有一份，住在 `menu_screen`。
//!
//! # 设置屏只有一块
//!
//! 首页的「设置」与暂停菜单的「设置」进的是**同一块**
//! [`ScreenState::Settings`]（所有者的硬要求：不要写第二份）。两者的
//! 唯一区别是 [`SettingsOrigin`]——按返回时回哪儿。

use ll_platform::input::{GameKey, InputState};
use ll_ui::widget::focus::navigate_focus;
use ll_ui::widget::state::{WidgetId, WidgetStateTable};

use crate::menu_screen::{
    ScreenNotice, ScreenOutcome, ScreenState, SettingsOrigin, apply_row_pointer, focus_index,
};
use crate::pointer::RowPointer;

/// 游戏主菜单（首页）四条选项的控件 id，顺序即导航顺序（同
/// [`crate::pause_menu::menu_item_ids`]）。
pub const TITLE_ITEM_IDS: [WidgetId; 4] = [
    "screen.title.new-game",
    "screen.title.load",
    "screen.title.settings",
    "screen.title.quit",
];

/// 首页四条选项各自的 Fluent 键，与 [`TITLE_ITEM_IDS`] 逐条对应。
pub(crate) const TITLE_ITEM_KEYS: [&str; 4] = [
    "screen-title-new-game",
    "screen-title-load",
    "screen-title-settings",
    "screen-title-quit",
];

/// 「读取存档」在首页里是第几行。
///
/// 做成常量而不是在两处各写一个 `1`：状态机（这一行可不可以按）与排版
/// （这一行显示成什么字）必须指的是同一行，两处各写一个字面量正是本
/// 项目反复付过代价的「两份清单迟早只更新一份」。
pub const TITLE_LOAD_ROW: usize = 1;

/// 首页当前聚焦的是第几行，见 [`focus_index`]。
pub fn title_focus_index(table: &WidgetStateTable) -> usize {
    focus_index(table, &TITLE_ITEM_IDS)
}

/// 处理首页这一帧输入之后，调用方该做什么。
///
/// 比 [`crate::pause_menu::update_menu`] 的裸元组多一格「这一帧要说的话」：首页有一条
/// 需要说话的路径（没有存档时按「读取存档」），而菜单屏一条都没有。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleUpdate {
    /// 处理完这一帧输入之后，调用方该做什么。
    pub outcome: ScreenOutcome,
    /// 要切到哪一块屏，`None` 表示留在首页。
    pub next: Option<ScreenState>,
    /// 这一帧屏上要说的一句话。
    pub notice: Option<ScreenNotice>,
}

impl TitleUpdate {
    /// 什么都没发生，留在首页。
    fn idle() -> TitleUpdate {
        TitleUpdate {
            outcome: ScreenOutcome::Idle,
            next: None,
            notice: None,
        }
    }

    /// 让调用方去做一件事（建新档 / 读档 / 退出）。
    fn acting(outcome: ScreenOutcome) -> TitleUpdate {
        TitleUpdate {
            outcome,
            next: None,
            notice: None,
        }
    }

    /// 切到另一块屏。
    fn going(next: ScreenState) -> TitleUpdate {
        TitleUpdate {
            outcome: ScreenOutcome::Idle,
            next: Some(next),
            notice: None,
        }
    }

    /// 只说一句话，什么都不做。
    fn saying(notice: ScreenNotice) -> TitleUpdate {
        TitleUpdate {
            outcome: ScreenOutcome::Idle,
            next: None,
            notice: Some(notice),
        }
    }
}

/// 处理首页这一帧的输入。
///
/// `has_save` 是「存档目录里有没有东西」。它仍然是一个布尔而不是一张
/// 列表：首页只需要回答「这一行按不按得动」，**哪一份**由下一块屏
/// （[`ScreenState::SaveList`]）负责。
///
/// # 取消键在首页什么都不做
///
/// 首页没有「上一层」可退。刻意**不**让它退出游戏：那正是上一批刚从
/// 游戏内改掉的行为（顶层按一下 Esc 整局就没了，所有者实机撞到并要求
/// 改掉），在首页重演一次同样不可接受。想离开就选「离开」那一行。
pub fn update_title(
    table: &mut WidgetStateTable,
    input: &InputState,
    pointer: RowPointer,
    has_save: bool,
) -> TitleUpdate {
    navigate_focus(table, &TITLE_ITEM_IDS, input);
    // 指针**按下**那一刻把焦点挪过去；松开在同一行才算确认。两条路径
    // 在这里汇成同一个分支——不为鼠标另写一套动作分派，见
    // `crate::pointer` 模块文档。
    apply_row_pointer(table, &TITLE_ITEM_IDS, pointer);
    if !input.was_just_pressed(GameKey::Confirm) && !pointer.activated() {
        return TitleUpdate::idle();
    }
    match title_focus_index(table) {
        0 => TitleUpdate::acting(ScreenOutcome::StartNewGame),
        // 多槽位之后这一行不再「直接读那一份」，而是进存档列表让玩家
        // 挑——他点「读取存档」时想的本来就是「读**哪**一份」。
        TITLE_LOAD_ROW if has_save => TitleUpdate::going(ScreenState::SaveList { cursor: 0 }),
        // 没有存档：这一行按下去只说一句话。**绝不**退而求其次地开一局
        // 新游戏——玩家点的是「读取存档」，给他一个新世界是答非所问。
        TITLE_LOAD_ROW => TitleUpdate::saying(ScreenNotice::NoSave),
        2 => TitleUpdate::going(ScreenState::Settings {
            cursor: 0,
            capturing: false,
            // 从首页进来的，按返回要回首页——写死回 `Menu` 会把玩家扔进
            // 一个底下没有世界的暂停菜单，见 `SettingsOrigin` 文档。
            origin: SettingsOrigin::Title,
        }),
        3 => TitleUpdate::acting(ScreenOutcome::Quit),
        // 还没选中任何一项（光标为 usize::MAX）时按确认——什么都不做，
        // 不猜一个默认项，与 `update_menu` 同一条纪律。
        _ => TitleUpdate::idle(),
    }
}
