//! 游戏内菜单与设置界面的状态机——`ll_ui::screen` 那块屏背后「按下这
//! 一行会发生什么」的全部逻辑。
//!
//! # 这个模块补的是哪条断线
//!
//! `GameKey::Menu` 的四样东西（枚举变体、默认键位 Tab、i18n 显示名、
//! 穷尽排序断言）从 UI 交互层批次起就齐全了，但 `crates/ll-game/**` 里
//! **没有任何消费点**——按下 Tab 什么都不会发生。阶段清算
//! （`knowledge/audit/2026-08-26-phase-reckoning-p6-p8.md` 三节）把它
//! 列成「一处标准的『声明了但没接线』」。本模块是那个消费点。
//!
//! 设置界面同理：数据侧（`GameConfig`/`KeyBindings::bindings`）早已
//! 就绪，缺的纯粹是那块屏与它背后的动作。
//!
//! # 为什么排版在这一层，不在 `ll-ui`
//!
//! 一行设置要显示成什么字，需要同时认识 `GameConfig`（当前取值）、
//! `KeyBindings`（这个动作绑着哪些键）与 `Catalog`（怎么翻译）。
//! `ll-game` 是唯一同时持有这三样的地方。`ll_ui::screen` 因此只收
//! 排好版的 `String`——与 `ll_ui::hud::action_menu`「只收已经排好版的
//! 字符串，不收领域类型」逐字同一条取舍。
//!
//! # 焦点导航：菜单用 `widget::focus`，设置用光标下标
//!
//! 两块屏用了两套导航，如实记录为什么不是一套：
//!
//! - **菜单屏**三条固定选项，用 [`ll_ui::widget::focus::navigate_focus`]
//!   与 [`ll_ui::widget::state::WidgetStateTable`]——这正是那个模块
//!   落地时说的「一组控件按视觉顺序给出 id 列表」的场景，`WidgetId`
//!   是 `&'static str`，三个静态字符串写得出来。
//! - **设置屏**二十几行，其中二十行是**按 `GameKey::all()` 现算出来
//!   的**。给它们各配一个 `&'static str` 需要手抄一张与 `GameKey`
//!   平行的静态表——那正是本项目反复踩过的「两份清单迟早只更新一份」
//!   （新增一个动作，设置界面静默漏掉它）。因此设置屏走
//!   `ll_game::player_action::PlayerMenu` 已经在用的光标下标法：行列表
//!   每帧现算，光标是一个 `usize`。
//!
//! 两者对玩家是同一套手感（上下移动、确认选中），差别只在实现。
//!
//! # 世界在这块屏底下不动
//!
//! `Demo::advance` 在模态屏开着时整段早退——不跑流式维护、不跑 AI、
//! 不跑玩家指令。见 `crate::app::Demo::advance` 与本批次计划文档 D9。

use std::path::Path;

use ll_i18n::{Catalog, FluentArgs};
use ll_platform::config::{GameConfig, ScaleFilter, save as save_config};
use ll_platform::input::{GameKey, InputState};
use ll_platform::keybind::{InputContext, KeyBinding, KeyBindings, KeyCode, Modifiers};
use ll_ui::screen::ScreenData;
use ll_ui::widget::focus::{focused_widget, navigate_focus};
use ll_ui::widget::state::{WidgetId, WidgetStateTable};

/// 菜单屏三条选项的控件 id，顺序即导航顺序（见
/// [`ll_ui::widget::focus::move_focus`] 文档「列表顺序即导航顺序」）。
pub const MENU_ITEM_IDS: [WidgetId; 3] = [
    "screen.menu.continue",
    "screen.menu.settings",
    "screen.menu.quit",
];

/// 菜单屏三条选项各自的 Fluent 键，与 [`MENU_ITEM_IDS`] 逐条对应。
pub(crate) const MENU_ITEM_KEYS: [&str; 3] = [
    "screen-menu-continue",
    "screen-menu-settings",
    "screen-menu-quit",
];

/// 模态屏当前开着哪一块。
///
/// 与 `crate::player_action::PlayerMenu` 同一条纪律：纯表现层状态，
/// 不进 `GameWorld`/`WorldState`、不进存档、不参与回放。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenState {
    /// 游戏内菜单（继续游戏 / 设置 / 退出）。
    Menu,
    /// 设置界面。
    Settings {
        /// 光标落在第几行，见模块文档「焦点导航」一节。
        cursor: usize,
        /// 是否正处于**捕获模式**——已经按下确认要给这一行重绑键位，
        /// 正等玩家按下想绑的那个物理键。
        capturing: bool,
    },
}

/// 处理完这一帧输入之后，调用方该做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenOutcome {
    /// 什么都不用做，屏继续开着。
    Idle,
    /// 关掉整块屏，回到游戏。
    Close,
    /// 退出整局游戏。
    Quit,
}

/// 设置界面的一行是什么。**每帧现算**（见 [`settings_rows`]），不缓存。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRow {
    /// 显示语言，左右键在已装载的语言之间循环。
    Language,
    /// 垂直同步开关。
    Vsync,
    /// 画面缩放滤波方式。
    ScaleFilter,
    /// 键位段的分隔标题——不可操作，确认键落在它上面什么都不发生。
    KeybindsHeader,
    /// 某个动作在 `Gameplay` 上下文下的键位。
    Keybind(GameKey),
    /// 把当前配置显式写回磁盘。
    Save,
    /// 返回菜单屏。
    Back,
}

/// 设置界面这一帧的全部行，顺序固定。
///
/// 键位那一段直接遍历 [`GameKey::all()`]——**不手抄一张平行清单**：
/// 抄一份的那一刻，「以后新增的动作会不会出现在设置界面里」就变成了
/// 一件要靠人记得同步的事，而本项目已经因为同型问题（声明了但没接线）
/// 付过多次代价。
pub fn settings_rows() -> Vec<SettingsRow> {
    let mut rows = vec![
        SettingsRow::Language,
        SettingsRow::Vsync,
        SettingsRow::ScaleFilter,
        SettingsRow::KeybindsHeader,
    ];
    rows.extend(GameKey::all().iter().copied().map(SettingsRow::Keybind));
    rows.push(SettingsRow::Save);
    rows.push(SettingsRow::Back);
    rows
}

/// 设置界面里键位那一段只列 `Gameplay` 上下文。
///
/// `InputContext::Menu` 那 11 条默认绑定**刻意不开放给玩家改**：它们是
/// 「在任何模态 UI 里怎么导航」的底座，玩家一旦把菜单上下文的确认键
/// 解绑，就再也没有任何办法回到设置界面把它绑回来——一个能把自己锁死
/// 的界面比一个少一项自定义的界面糟得多。
pub const EDITABLE_CONTEXT: InputContext = InputContext::Gameplay;

/// 这一帧屏上要显示的一句状态提示（键位冲突、已保存等），`None` 表示
/// 没有话要说。
///
/// 与 `crate::player_action::Feedback` 同一条纪律：**输入层自己就能
/// 判定的**才在这里说，说不清楚的不硬编一句。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenNotice {
    /// 想绑的键已经被同一上下文下的另一个动作占着——**绑定没有发生**，
    /// 玩家的表一个字节都没变，见 [`try_rebind`]。
    Conflict(GameKey),
    /// 绑定成功。
    Bound(GameKey),
    /// 已清除某个动作的全部键位。
    Cleared(GameKey),
    /// 配置已写回磁盘。
    Saved,
    /// 配置写盘失败——本次会话内的改动仍然有效，只是没存下来。
    SaveFailed,
}

impl ScreenNotice {
    /// 这条提示对应的 Fluent 键。
    fn i18n_key(self) -> &'static str {
        match self {
            ScreenNotice::Conflict(_) => "screen-settings-conflict",
            ScreenNotice::Bound(_) => "screen-settings-bound",
            ScreenNotice::Cleared(_) => "screen-settings-cleared",
            ScreenNotice::Saved => "screen-settings-saved",
            ScreenNotice::SaveFailed => "screen-settings-save-failed",
        }
    }

    /// 解析成一句可以直接画在屏上的话。
    pub fn resolve(self, catalog: &Catalog, language: &str) -> String {
        let action = match self {
            ScreenNotice::Conflict(action)
            | ScreenNotice::Bound(action)
            | ScreenNotice::Cleared(action) => Some(action),
            ScreenNotice::Saved | ScreenNotice::SaveFailed => None,
        };
        match action {
            Some(action) => {
                let mut args = FluentArgs::new();
                args.set(
                    "action",
                    catalog.resolve(language, &action.display_name_key().to_string()),
                );
                catalog.resolve_with_args(language, self.i18n_key(), Some(&args))
            }
            None => catalog.resolve(language, self.i18n_key()),
        }
    }
}

/// 试着把 `key` 绑给 `action`——**冲突就整体拒绝，绝不静默覆盖**。
///
/// 冲突判定完全交给 [`KeyBindings::try_bind`]，本函数不重抄一遍判重
/// 逻辑：`crate::keybind` 模块文档写明「同一个物理键（含修饰键）在同一
/// 个上下文下绑给两个不同的动作是一个真实的逻辑错误」，而那条约束的
/// 唯一执行点就是 `try_bind`。UI 层要做的只有两件事：在**草稿**上试，
/// 以及把拒绝的理由说给玩家听。
///
/// # 为什么是追加，不是替换
///
/// 默认表里 `Up` 同时绑着 `ArrowUp` 与 `KeyW`（方向键与 WASD 双绑是
/// 刻意的，见 `DEFAULT_BINDINGS` 文档）。「替换」语义会让玩家给一个
/// 动作加一个键的同时**静默丢掉**它原有的另外几个——那正是本批次要防
/// 的「不要静默覆盖」的另一种形态。要丢就必须显式解绑
/// （[`clear_bindings`]）。
pub fn try_rebind(
    bindings: &KeyBindings,
    action: GameKey,
    key: KeyCode,
) -> Result<KeyBindings, GameKey> {
    let mut draft = bindings.clone();
    match draft.try_bind(KeyBinding {
        key,
        modifiers: Modifiers::NONE,
        context: EDITABLE_CONTEXT,
        action,
    }) {
        Ok(()) => Ok(draft),
        Err(conflict) => Err(conflict.existing_action),
    }
}

/// 清除 `action` 在 [`EDITABLE_CONTEXT`] 下的全部键位，并把它记进
/// [`GameConfig::unbound_actions`]。
///
/// 两件事必须同批做：只清绑定表，下次加载时
/// `KeyBindings::fill_missing_defaults` 会把默认键位又补回来——「玩家
/// 刻意解绑」与「文件写出时还没有这个动作」在绑定表里长得一模一样，
/// `unbound_actions` 是唯一能把两者分开的地方（见该字段文档）。
pub fn clear_bindings(config: &mut GameConfig, action: GameKey) {
    config.bindings.unbind_action(action, EDITABLE_CONTEXT);
    if !config.unbound_actions.contains(&action) {
        config.unbound_actions.push(action);
    }
}

/// 菜单屏当前聚焦的是第几行；没有任何一项聚焦时返回 `usize::MAX`
/// ——那是一个**必然越界**的下标，`ll_ui::screen` 收到越界光标时不标记
/// 任何一行（见 `ScreenData::cursor` 文档），正好等于「还没选中任何
/// 一项」这个事实。不返回 `0`：那会让屏上看起来「第一项已经被选中」，
/// 与实际状态不符。
pub fn menu_focus_index(table: &WidgetStateTable) -> usize {
    focused_widget(table, &MENU_ITEM_IDS)
        .and_then(|id| MENU_ITEM_IDS.iter().position(|candidate| *candidate == id))
        .unwrap_or(usize::MAX)
}

/// 建出这一帧要交给 `ll_ui::screen` 的数据。
pub fn screen_data<'a>(
    state: ScreenState,
    rows: &'a [String],
    focus: usize,
    notice: Option<&'a str>,
) -> ScreenData<'a> {
    match state {
        ScreenState::Menu => ScreenData {
            title_key: "screen-menu-title",
            rows,
            cursor: focus,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice,
        },
        ScreenState::Settings { capturing, .. } => ScreenData {
            title_key: "screen-settings-title",
            rows,
            cursor: focus,
            empty_key: "screen-settings-empty",
            hint_key: if capturing {
                "screen-settings-capture-hint"
            } else {
                "screen-settings-hint"
            },
            notice,
        },
    }
}

/// 一次设置界面输入处理的全部产出。
///
/// # 为什么 `rebound` 是一个独立字段，而不是从别处推出来
///
/// 平台层查的绑定表与设置界面改的那一份是两份（见
/// `ll_platform::window::AppHandler::take_rebound_keys` 与
/// `crate::run_game` 里那句「克隆而不是移动」），改完必须送回去，否则
/// 「改了键位不生效」。而**送回去的代价不是零**：整表克隆一份、平台层
/// 整表替换一次、日志里多一行「键位绑定表已由上层替换」。
///
/// 这一行此前是**每帧无条件**执行的（设置屏一开，终端每帧刷一行），
/// 而它旁边的注释却写着「不是每帧」——项目所有者实机撞到。修法就是本
/// 字段：只有真的改过键位的那些帧才为真。
///
/// **不从 [`ScreenNotice`] 推导**（`Bound`/`Cleared` 恰好就是那两条
/// 路径）：那会把「屏幕上对玩家说什么」与「要不要把表送回平台层」绑成
/// 同一件事，将来一条改了键位却不说话（或说别的话）的路径会让缺陷以
/// 「改了键位不生效」的形态回来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsUpdate {
    /// 处理完这一帧输入之后，调用方该做什么。
    pub outcome: ScreenOutcome,
    /// 这一帧屏上要说的一句话，`None` 表示没有话要说。
    pub notice: Option<ScreenNotice>,
    /// 这一帧**真的**改动了 [`SettingsContext::config`] 里的键位表——
    /// 只有为真时调用方才需要把整表送回平台层，见本类型文档。
    pub rebound: bool,
}

impl SettingsUpdate {
    /// 什么都没发生。
    fn idle() -> SettingsUpdate {
        SettingsUpdate {
            outcome: ScreenOutcome::Idle,
            notice: None,
            rebound: false,
        }
    }

    /// 只有一句话要说，键位表一个字节都没动。
    fn saying(notice: ScreenNotice) -> SettingsUpdate {
        SettingsUpdate {
            outcome: ScreenOutcome::Idle,
            notice: Some(notice),
            rebound: false,
        }
    }

    /// 键位表真的改了，并说一句话——**唯一**把 `rebound` 置真的构造器。
    fn rebound(notice: ScreenNotice) -> SettingsUpdate {
        SettingsUpdate {
            outcome: ScreenOutcome::Idle,
            notice: Some(notice),
            rebound: true,
        }
    }
}

/// 一次设置界面输入处理需要摸到的全部东西。
pub struct SettingsContext<'a> {
    /// 玩家配置的**草稿**——本模块直接改它，改完由调用方决定什么时候
    /// 写盘（[`SettingsRow::Save`]）与什么时候把新绑定表送回平台层。
    pub config: &'a mut GameConfig,
    /// 配置文件路径，保存时用。
    pub config_path: &'a Path,
    /// 本地化目录，切换语言时要按它列出已装载的语言。
    pub catalog: &'a Catalog,
}

/// 处理菜单屏这一帧的输入。
pub fn update_menu(
    table: &mut WidgetStateTable,
    input: &InputState,
) -> (ScreenOutcome, Option<ScreenState>) {
    navigate_focus(table, &MENU_ITEM_IDS, input);
    if input.was_just_pressed(GameKey::Cancel) {
        return (ScreenOutcome::Close, None);
    }
    if !input.was_just_pressed(GameKey::Confirm) {
        return (ScreenOutcome::Idle, None);
    }
    match menu_focus_index(table) {
        0 => (ScreenOutcome::Close, None),
        1 => (
            ScreenOutcome::Idle,
            Some(ScreenState::Settings {
                cursor: 0,
                capturing: false,
            }),
        ),
        2 => (ScreenOutcome::Quit, None),
        // 还没选中任何一项（光标为 usize::MAX）时按确认——什么都不做，
        // 不猜一个默认项。
        _ => (ScreenOutcome::Idle, None),
    }
}

/// 处理设置界面这一帧的输入，返回这一帧产生的提示（若有）。
///
/// 拆成「捕获模式」与「常规模式」两条，是因为两者读的**根本不是同一种
/// 输入**：捕获模式读的是原始物理键（`InputState::last_physical_key`，
/// 绕过绑定表），常规模式读的是抽象动作（`GameKey`）。
pub fn update_settings(
    state: &mut ScreenState,
    input: &InputState,
    ctx: &mut SettingsContext<'_>,
) -> SettingsUpdate {
    let ScreenState::Settings { cursor, capturing } = *state else {
        return SettingsUpdate::idle();
    };
    let rows = settings_rows();
    if capturing {
        return update_capture(state, input, ctx, &rows, cursor);
    }
    update_navigation(state, input, ctx, &rows, cursor)
}

/// 捕获模式：只看原始物理键。
fn update_capture(
    state: &mut ScreenState,
    input: &InputState,
    ctx: &mut SettingsContext<'_>,
    rows: &[SettingsRow],
    cursor: usize,
) -> SettingsUpdate {
    // 这一帧没按任何物理键——捕获模式绝大多数帧走的都是这一条，也正是
    // 「屏开着但玩家什么都不按」那些帧不该产生任何键位表克隆的原因。
    let Some(key) = input.last_physical_key() else {
        return SettingsUpdate::idle();
    };
    let Some(SettingsRow::Keybind(action)) = rows.get(cursor).copied() else {
        // 光标不在键位行上却进了捕获模式——不该发生，但退出捕获比
        // panic 好（一个纯 UI 状态问题不该拖垮整局）。
        *state = leave_capture(cursor);
        return SettingsUpdate::idle();
    };
    // Esc 取消、退格解绑——两个键因此不可绑，代价写进本批次计划文档
    // D5。走原始物理键而不是 `GameKey::Cancel`：捕获模式的整个语义就是
    // 「这一刻不查绑定表」，为这两个键破例去查表会自相矛盾。
    match key {
        KeyCode::Escape => {
            *state = leave_capture(cursor);
            SettingsUpdate::idle()
        }
        KeyCode::Backspace => {
            // 解绑：键位表真的变了，两处改键位入口之一。
            clear_bindings(ctx.config, action);
            *state = leave_capture(cursor);
            SettingsUpdate::rebound(ScreenNotice::Cleared(action))
        }
        key => apply_capture(state, ctx, action, key, cursor),
    }
}

/// 退出捕获模式、光标留在原处。
fn leave_capture(cursor: usize) -> ScreenState {
    ScreenState::Settings {
        cursor,
        capturing: false,
    }
}

/// 玩家按下的是一个普通物理键：试着绑上去。
fn apply_capture(
    state: &mut ScreenState,
    ctx: &mut SettingsContext<'_>,
    action: GameKey,
    key: KeyCode,
    cursor: usize,
) -> SettingsUpdate {
    match try_rebind(&ctx.config.bindings, action, key) {
        Ok(bindings) => {
            // 重绑成功：键位表真的变了，两处改键位入口之二。
            ctx.config.bindings = bindings;
            // 重新绑上了，「刻意解绑」这个意图随之作废。
            ctx.config.unbound_actions.retain(|it| *it != action);
            *state = leave_capture(cursor);
            SettingsUpdate::rebound(ScreenNotice::Bound(action))
        }
        // 冲突：**留在捕获模式**，玩家可以直接再按一个别的键，不用重新
        // 进一次。表一个字节都没变，`rebound` 因此为假。
        Err(occupied) => SettingsUpdate::saying(ScreenNotice::Conflict(occupied)),
    }
}

/// 常规模式：上下移动光标、左右改取值、确认触发这一行的动作。
fn update_navigation(
    state: &mut ScreenState,
    input: &InputState,
    ctx: &mut SettingsContext<'_>,
    rows: &[SettingsRow],
    cursor: usize,
) -> SettingsUpdate {
    if input.was_just_pressed(GameKey::Cancel) {
        *state = ScreenState::Menu;
        return SettingsUpdate::idle();
    }
    if let Some(next) = moved_cursor(input, cursor, rows.len()) {
        *state = ScreenState::Settings {
            cursor: next,
            capturing: false,
        };
        return SettingsUpdate::idle();
    }
    let Some(row) = rows.get(cursor).copied() else {
        return SettingsUpdate::idle();
    };
    if input.was_just_pressed(GameKey::Left) || input.was_just_pressed(GameKey::Right) {
        let forward = input.was_just_pressed(GameKey::Right);
        adjust_value(row, ctx, forward);
        return SettingsUpdate::idle();
    }
    if !input.was_just_pressed(GameKey::Confirm) {
        return SettingsUpdate::idle();
    }
    match row {
        SettingsRow::Keybind(_) => {
            *state = ScreenState::Settings {
                cursor,
                capturing: true,
            };
            SettingsUpdate::idle()
        }
        // 保存写的是磁盘，不动内存里那张表——`rebound` 因此为假。
        SettingsRow::Save => SettingsUpdate::saying(save_settings(ctx)),
        SettingsRow::Back => {
            *state = ScreenState::Menu;
            SettingsUpdate::idle()
        }
        // 三个取值行按确认等价于「往前拨一格」，与左右键一致；分隔标题
        // 什么都不做。
        SettingsRow::KeybindsHeader => SettingsUpdate::idle(),
        other => {
            adjust_value(other, ctx, true);
            SettingsUpdate::idle()
        }
    }
}

/// 上下键移动光标——`was_activated` 而非 `was_just_pressed`：方向键参与
/// 自动重复（`GameKey::is_repeatable`），二十几行的列表长按滚动是刚需。
/// 与 `crate::player_action::moved_cursor` 同一套语义。
fn moved_cursor(input: &InputState, cursor: usize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if input.was_activated(GameKey::Down) {
        return Some((cursor + 1) % len);
    }
    if input.was_activated(GameKey::Up) {
        return Some((cursor + len - 1) % len);
    }
    None
}

/// 把这一行的取值往前/往后拨一格。
fn adjust_value(row: SettingsRow, ctx: &mut SettingsContext<'_>, forward: bool) {
    match row {
        SettingsRow::Language => cycle_language(ctx, forward),
        SettingsRow::Vsync => ctx.config.display.vsync = !ctx.config.display.vsync,
        SettingsRow::ScaleFilter => {
            ctx.config.display.scale_filter = match ctx.config.display.scale_filter {
                ScaleFilter::Nearest => ScaleFilter::SharpBilinear,
                ScaleFilter::SharpBilinear => ScaleFilter::Nearest,
            }
        }
        _ => {}
    }
}

/// 在已装载的语言之间循环。
///
/// 语言清单来自 [`Catalog::languages`]，那个方法**保证已排序**（C5：
/// 内部是 `HashMap`，直接遍历会让「按右键切到哪一种语言」依赖哈希桶
/// 序）。当前语言不在清单里（例如配置文件写了一个没装载的标签）时从
/// 头开始，不 panic。
fn cycle_language(ctx: &mut SettingsContext<'_>, forward: bool) {
    let languages = ctx.catalog.languages();
    if languages.is_empty() {
        return;
    }
    let current = languages
        .iter()
        .position(|tag| *tag == ctx.config.language)
        .unwrap_or(0);
    let next = if forward {
        (current + 1) % languages.len()
    } else {
        (current + languages.len() - 1) % languages.len()
    };
    ctx.config.language = languages[next].clone();
}

/// 显式保存：把当前配置草稿写回磁盘。
///
/// # 这会抹掉玩家手写的 JSON5 注释——如实说，不粉饰
///
/// `ll_platform::config::save` 走的是 `serde_json::to_string_pretty`
/// （`json5` crate 只提供解析、不提供序列化，见该模块文档「格式：
/// JSON5，读写不对称」一节）。写出的是普通 JSON，玩家手写在
/// `config.json5` 里的注释与尾逗号**会全部丢失**。
///
/// 本批次没有解决它，只做了三件降低代价的事：
///
/// 1. **只在玩家显式按下这一行时才写盘**，绝不自动触发（这正是所有者
///    裁定「不回写，等设置界面落地后由界面显式保存」的落点）；
/// 2. 屏幕上那句「已保存」的文案里**直接写明注释会丢失**
///    （`screen-settings-saved`），不让玩家事后才发现；
/// 3. 写盘前后各记一条日志。
///
/// 真正的解法是引入一个保序、保注释的 JSON5 写出（读进 CST、只改叶子
/// 值、原样写回），那是独立的一批，已写进本批次计划文档第八节留给
/// 所有者裁定。
fn save_settings(ctx: &mut SettingsContext<'_>) -> ScreenNotice {
    tracing::warn!(
        path = %ctx.config_path.display(),
        "即将写出配置文件：写出的是普通 JSON，玩家手写的 JSON5 注释会丢失" // i18n-exempt（日志）
    );
    match save_config(ctx.config_path, ctx.config) {
        Ok(()) => {
            tracing::info!(path = %ctx.config_path.display(), "配置已保存");
            ScreenNotice::Saved
        }
        Err(error) => {
            tracing::warn!(%error, path = %ctx.config_path.display(), "配置写入失败");
            ScreenNotice::SaveFailed
        }
    }
}
