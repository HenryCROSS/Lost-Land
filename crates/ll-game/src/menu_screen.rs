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
const MENU_ITEM_KEYS: [&str; 3] = [
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

/// 某个动作在 [`EDITABLE_CONTEXT`] 下当前绑着哪些键，排好版成一行。
///
/// 键名用 `KeyCode` 的 `Debug` 形态（`KeyW`/`Space`/`ArrowUp`）——如实
/// 记录这是一处**临时取舍**：给两百多个 `KeyCode` 变体各配一条 i18n
/// 键是一笔与本批次目标无关的大工程，而物理键名在绝大多数游戏里本来
/// 就不翻译（键帽上印的就是这些字母）。真要本地化，加法是在
/// `ll-platform` 给 `KeyCode` 配一张 `display_name_key` 表，本函数改成
/// 查那张表，其余一行不动。
pub fn binding_summary(
    bindings: &KeyBindings,
    action: GameKey,
    catalog: &Catalog,
    language: &str,
) -> String {
    let keys: Vec<String> = bindings
        .bindings_for(action)
        .filter(|binding| binding.context == EDITABLE_CONTEXT)
        .map(|binding| format!("{:?}", binding.key))
        .collect();
    if keys.is_empty() {
        return catalog.resolve(language, "screen-settings-unbound");
    }
    keys.join(" / ")
}

/// 某种语言在**它自己的语言**里叫什么（endonym）。
///
/// 查的是 `language-name` 这条键在**那一份** `.ftl` 里的取值，不是在
/// 当前显示语言里的取值——语言选单上每一项都用自己的文字写，是这类
/// 界面的通行做法（玩家看不懂当前语言时，恰恰要靠这一列找回自己的
/// 语言）。查不到时退回语言标签本身（`Catalog::resolve` 找不到键会
/// 原样返回键名，那个键名对玩家毫无意义，退回标签更诚实）。
pub fn language_display_name(catalog: &Catalog, tag: &str) -> String {
    let name = catalog.resolve(tag, "language-name");
    if name == "language-name" {
        tag.to_string()
    } else {
        name
    }
}

/// 把一行排成「标签：取值」——分隔符走 i18n 模板（`screen-settings-row`）
/// 而不是在代码里拼一个冒号：中文用全角「：」、英文用半角「: 」，写死
/// 任何一种都会在另一种语言下看起来是错的。
fn labeled_row(catalog: &Catalog, language: &str, label_key: &str, value: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("label", catalog.resolve(language, label_key));
    args.set("value", value.to_string());
    catalog.resolve_with_args(language, "screen-settings-row", Some(&args))
}

/// 把设置界面这一帧的每一行排好版。
pub fn settings_row_texts(
    rows: &[SettingsRow],
    config: &GameConfig,
    catalog: &Catalog,
    capturing: bool,
    capture_row: usize,
) -> Vec<String> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            settings_row_text(*row, config, catalog, capturing && index == capture_row)
        })
        .collect()
}

fn settings_row_text(
    row: SettingsRow,
    config: &GameConfig,
    catalog: &Catalog,
    capturing_this_row: bool,
) -> String {
    let language = config.language.as_str();
    match row {
        SettingsRow::Language => labeled_row(
            catalog,
            language,
            "screen-settings-language",
            &language_display_name(catalog, language),
        ),
        SettingsRow::Vsync => {
            let value = catalog.resolve(
                language,
                if config.display.vsync {
                    "screen-settings-on"
                } else {
                    "screen-settings-off"
                },
            );
            let restart = catalog.resolve(language, "screen-settings-restart-required");
            labeled_row(
                catalog,
                language,
                "screen-settings-vsync",
                &format!("{value} {restart}"),
            )
        }
        SettingsRow::ScaleFilter => {
            let value = catalog.resolve(
                language,
                match config.display.scale_filter {
                    ScaleFilter::Nearest => "screen-settings-filter-nearest",
                    ScaleFilter::SharpBilinear => "screen-settings-filter-sharp-bilinear",
                },
            );
            labeled_row(catalog, language, "screen-settings-scale-filter", &value)
        }
        SettingsRow::KeybindsHeader => catalog.resolve(language, "screen-settings-keybinds-header"),
        SettingsRow::Keybind(action) => {
            let value = if capturing_this_row {
                catalog.resolve(language, "screen-settings-capturing")
            } else {
                binding_summary(&config.bindings, action, catalog, language)
            };
            labeled_row(
                catalog,
                language,
                &action.display_name_key().to_string(),
                &value,
            )
        }
        SettingsRow::Save => catalog.resolve(language, "screen-settings-save"),
        SettingsRow::Back => catalog.resolve(language, "screen-settings-back"),
    }
}

/// 菜单屏这一帧的三行文字。
pub fn menu_row_texts(catalog: &Catalog, language: &str) -> Vec<String> {
    MENU_ITEM_KEYS
        .iter()
        .map(|key| catalog.resolve(language, key))
        .collect()
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
) -> (ScreenOutcome, Option<ScreenNotice>) {
    let ScreenState::Settings { cursor, capturing } = *state else {
        return (ScreenOutcome::Idle, None);
    };
    let rows = settings_rows();
    if capturing {
        let notice = update_capture(state, input, ctx, &rows, cursor);
        return (ScreenOutcome::Idle, notice);
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
) -> Option<ScreenNotice> {
    let key = input.last_physical_key()?;
    let Some(SettingsRow::Keybind(action)) = rows.get(cursor).copied() else {
        // 光标不在键位行上却进了捕获模式——不该发生，但退出捕获比
        // panic 好（一个纯 UI 状态问题不该拖垮整局）。
        *state = ScreenState::Settings {
            cursor,
            capturing: false,
        };
        return None;
    };
    // Esc 取消、退格解绑——两个键因此不可绑，代价写进本批次计划文档
    // D5。走原始物理键而不是 `GameKey::Cancel`：捕获模式的整个语义就是
    // 「这一刻不查绑定表」，为这两个键破例去查表会自相矛盾。
    match key {
        KeyCode::Escape => {
            *state = ScreenState::Settings {
                cursor,
                capturing: false,
            };
            None
        }
        KeyCode::Backspace => {
            clear_bindings(ctx.config, action);
            *state = ScreenState::Settings {
                cursor,
                capturing: false,
            };
            Some(ScreenNotice::Cleared(action))
        }
        key => match try_rebind(&ctx.config.bindings, action, key) {
            Ok(bindings) => {
                ctx.config.bindings = bindings;
                // 重新绑上了，「刻意解绑」这个意图随之作废。
                ctx.config.unbound_actions.retain(|it| *it != action);
                *state = ScreenState::Settings {
                    cursor,
                    capturing: false,
                };
                Some(ScreenNotice::Bound(action))
            }
            // 冲突：**留在捕获模式**，玩家可以直接再按一个别的键，
            // 不用重新进一次。表一个字节都没变。
            Err(occupied) => Some(ScreenNotice::Conflict(occupied)),
        },
    }
}

/// 常规模式：上下移动光标、左右改取值、确认触发这一行的动作。
fn update_navigation(
    state: &mut ScreenState,
    input: &InputState,
    ctx: &mut SettingsContext<'_>,
    rows: &[SettingsRow],
    cursor: usize,
) -> (ScreenOutcome, Option<ScreenNotice>) {
    if input.was_just_pressed(GameKey::Cancel) {
        *state = ScreenState::Menu;
        return (ScreenOutcome::Idle, None);
    }
    if let Some(next) = moved_cursor(input, cursor, rows.len()) {
        *state = ScreenState::Settings {
            cursor: next,
            capturing: false,
        };
        return (ScreenOutcome::Idle, None);
    }
    let Some(row) = rows.get(cursor).copied() else {
        return (ScreenOutcome::Idle, None);
    };
    if input.was_just_pressed(GameKey::Left) || input.was_just_pressed(GameKey::Right) {
        let forward = input.was_just_pressed(GameKey::Right);
        adjust_value(row, ctx, forward);
        return (ScreenOutcome::Idle, None);
    }
    if !input.was_just_pressed(GameKey::Confirm) {
        return (ScreenOutcome::Idle, None);
    }
    match row {
        SettingsRow::Keybind(_) => {
            *state = ScreenState::Settings {
                cursor,
                capturing: true,
            };
            (ScreenOutcome::Idle, None)
        }
        SettingsRow::Save => (ScreenOutcome::Idle, Some(save_settings(ctx))),
        SettingsRow::Back => {
            *state = ScreenState::Menu;
            (ScreenOutcome::Idle, None)
        }
        // 三个取值行按确认等价于「往前拨一格」，与左右键一致；分隔标题
        // 什么都不做。
        SettingsRow::KeybindsHeader => (ScreenOutcome::Idle, None),
        other => {
            adjust_value(other, ctx, true);
            (ScreenOutcome::Idle, None)
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

#[cfg(test)]
mod tests {
    use super::*;

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

    fn 设置状态(cursor: usize) -> ScreenState {
        ScreenState::Settings {
            cursor,
            capturing: false,
        }
    }

    fn 某行下标(target: SettingsRow) -> usize {
        settings_rows()
            .iter()
            .position(|row| *row == target)
            .expect("这一行必然存在")
    }

    #[test]
    fn 每个动作键在设置界面都占一行() {
        // 「新增动作后设置界面静默漏掉它」是本模块最想防的缺陷。
        // Arrange & Act
        let rows = settings_rows();

        // Assert
        for key in GameKey::all() {
            assert!(
                rows.contains(&SettingsRow::Keybind(*key)),
                "{key:?} 在设置界面里没有对应的行"
            );
        }
    }

    #[test]
    fn 菜单里向下移动焦点落在第一项() {
        // Arrange
        let mut table = WidgetStateTable::new();

        // Act
        update_menu(&mut table, &按下(&[GameKey::Down]));

        // Assert
        assert_eq!(menu_focus_index(&table), 0);
    }

    #[test]
    fn 菜单里没有任何一项聚焦时光标越界不标记任何行() {
        // Arrange
        let table = WidgetStateTable::new();

        // Act
        let index = menu_focus_index(&table);

        // Assert
        assert_eq!(index, usize::MAX);
    }

    #[test]
    fn 菜单里选中设置项后进入设置界面() {
        // Arrange：向下一次落在「继续游戏」，再一次落在「设置」。
        let mut table = WidgetStateTable::new();
        update_menu(&mut table, &按下(&[GameKey::Down]));
        update_menu(&mut table, &按下(&[GameKey::Down]));

        // Act
        let (outcome, next) = update_menu(&mut table, &按下(&[GameKey::Confirm]));

        // Assert
        assert_eq!(outcome, ScreenOutcome::Idle);
        assert_eq!(
            next,
            Some(ScreenState::Settings {
                cursor: 0,
                capturing: false
            })
        );
    }

    #[test]
    fn 菜单里选中退出项返回退出() {
        // Arrange
        let mut table = WidgetStateTable::new();
        for _ in 0..3 {
            update_menu(&mut table, &按下(&[GameKey::Down]));
        }

        // Act
        let (outcome, _) = update_menu(&mut table, &按下(&[GameKey::Confirm]));

        // Assert
        assert_eq!(outcome, ScreenOutcome::Quit);
    }

    #[test]
    fn 菜单里按取消关掉整块屏() {
        // Arrange
        let mut table = WidgetStateTable::new();

        // Act
        let (outcome, _) = update_menu(&mut table, &按下(&[GameKey::Cancel]));

        // Assert
        assert_eq!(outcome, ScreenOutcome::Close);
    }

    #[test]
    fn 把已经被别的动作占着的键绑过来会被拒绝且原表不变() {
        // 空格默认绑给 Interact；试图把它绑给 Confirm 必须被拒。
        // Arrange
        let bindings = KeyBindings::default_bindings();
        let 原来的空格 = bindings.resolve(KeyCode::Space, Modifiers::NONE, EDITABLE_CONTEXT);

        // Act
        let result = try_rebind(&bindings, GameKey::Confirm, KeyCode::Space);

        // Assert
        assert_eq!(result.err(), Some(GameKey::Interact));
        assert_eq!(
            bindings.resolve(KeyCode::Space, Modifiers::NONE, EDITABLE_CONTEXT),
            原来的空格,
            "被拒绝的重绑不该改动原表"
        );
    }

    #[test]
    fn 解绑之后空格可以改回确认键() {
        // 交接文档第四节第 18 条的直接验收：Interact 从 Confirm 手里
        // 拿走了空格，所有者要求「配置合并落地后要能让玩家改回来」。
        // Arrange
        let mut config = GameConfig::default();
        clear_bindings(&mut config, GameKey::Interact);

        // Act
        let rebound = try_rebind(&config.bindings, GameKey::Confirm, KeyCode::Space)
            .expect("空格已经解绑，重绑不该冲突");

        // Assert
        assert_eq!(
            rebound.resolve(KeyCode::Space, Modifiers::NONE, EDITABLE_CONTEXT),
            Some(GameKey::Confirm)
        );
    }

    #[test]
    fn 解绑会把动作记进刻意解绑清单() {
        // 不记的话，下次加载 fill_missing_defaults 会把默认键补回来。
        // Arrange
        let mut config = GameConfig::default();

        // Act
        clear_bindings(&mut config, GameKey::Interact);

        // Assert
        assert!(config.unbound_actions.contains(&GameKey::Interact));
    }

    #[test]
    fn 重新绑上之后刻意解绑的记号被撤销() {
        // 否则玩家「解绑再绑别的键」之后，下次加载会以为他还想解绑。
        // Arrange
        let mut config = GameConfig::default();
        clear_bindings(&mut config, GameKey::Interact);
        let mut state = ScreenState::Settings {
            cursor: 某行下标(SettingsRow::Keybind(GameKey::Interact)),
            capturing: true,
        };
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-rebind");
        let mut input = InputState::new();
        input.record_physical_key(KeyCode::KeyN);
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        let notice = update_capture(
            &mut state,
            &input,
            &mut ctx,
            &settings_rows(),
            某行下标(SettingsRow::Keybind(GameKey::Interact)),
        );

        // Assert
        assert_eq!(notice, Some(ScreenNotice::Bound(GameKey::Interact)));
        assert!(!config.unbound_actions.contains(&GameKey::Interact));
    }

    #[test]
    fn 捕获模式下按退格解绑当前这一行() {
        // Arrange
        let mut config = GameConfig::default();
        let cursor = 某行下标(SettingsRow::Keybind(GameKey::Map));
        let mut state = ScreenState::Settings {
            cursor,
            capturing: true,
        };
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-clear");
        let mut input = InputState::new();
        input.record_physical_key(KeyCode::Backspace);
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        let notice = update_capture(&mut state, &input, &mut ctx, &settings_rows(), cursor);

        // Assert
        assert_eq!(notice, Some(ScreenNotice::Cleared(GameKey::Map)));
        assert_eq!(config.bindings.bindings_for(GameKey::Map).count(), 0);
    }

    #[test]
    fn 捕获模式下按esc取消不改动任何绑定() {
        // Arrange
        let mut config = GameConfig::default();
        let 改前 = config.bindings.bindings_for(GameKey::Map).count();
        let cursor = 某行下标(SettingsRow::Keybind(GameKey::Map));
        let mut state = ScreenState::Settings {
            cursor,
            capturing: true,
        };
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-cancel");
        let mut input = InputState::new();
        input.record_physical_key(KeyCode::Escape);
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        let notice = update_capture(&mut state, &input, &mut ctx, &settings_rows(), cursor);

        // Assert
        assert_eq!(notice, None);
        assert_eq!(config.bindings.bindings_for(GameKey::Map).count(), 改前);
        assert_eq!(
            state,
            ScreenState::Settings {
                cursor,
                capturing: false
            }
        );
    }

    #[test]
    fn 冲突时留在捕获模式让玩家直接再按一个键() {
        // Arrange
        let mut config = GameConfig::default();
        let cursor = 某行下标(SettingsRow::Keybind(GameKey::Confirm));
        let mut state = ScreenState::Settings {
            cursor,
            capturing: true,
        };
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-conflict");
        let mut input = InputState::new();
        input.record_physical_key(KeyCode::Space);
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        let notice = update_capture(&mut state, &input, &mut ctx, &settings_rows(), cursor);

        // Assert
        assert_eq!(notice, Some(ScreenNotice::Conflict(GameKey::Interact)));
        assert_eq!(
            state,
            ScreenState::Settings {
                cursor,
                capturing: true
            }
        );
    }

    #[test]
    fn 左右键切换语言当场改变配置里的语言标签() {
        // Arrange
        let mut config = GameConfig::default();
        let 原语言 = config.language.clone();
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-language");
        let mut state = 设置状态(某行下标(SettingsRow::Language));
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        update_settings(&mut state, &按下(&[GameKey::Right]), &mut ctx);

        // Assert
        assert_ne!(config.language, 原语言);
    }

    #[test]
    fn 切换语言后同一个键解析出另一种语言的文字() {
        // 「当场生效」的实质验证：不是只改了一个字符串字段。
        // Arrange
        let catalog = 测试目录();

        // Act
        let 中文 = catalog.resolve("zh-CN", "screen-menu-title");
        let 英文 = catalog.resolve("en", "screen-menu-title");

        // Assert
        assert_ne!(中文, 英文);
    }

    #[test]
    fn 垂直同步行左右键翻转开关() {
        // Arrange
        let mut config = GameConfig::default();
        let 原值 = config.display.vsync;
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-vsync");
        let mut state = 设置状态(某行下标(SettingsRow::Vsync));
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        update_settings(&mut state, &按下(&[GameKey::Right]), &mut ctx);

        // Assert
        assert_eq!(config.display.vsync, !原值);
    }

    #[test]
    fn 缩放滤波行左右键在两档之间循环() {
        // Arrange
        let mut config = GameConfig::default();
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-filter");
        let mut state = 设置状态(某行下标(SettingsRow::ScaleFilter));
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        update_settings(&mut state, &按下(&[GameKey::Right]), &mut ctx);

        // Assert
        assert_eq!(config.display.scale_filter, ScaleFilter::SharpBilinear);
    }

    #[test]
    fn 保存写出的配置能被重新加载且键位一致() {
        // Arrange：先改一处键位，再保存，再读回。
        let mut config = GameConfig::default();
        clear_bindings(&mut config, GameKey::Interact);
        config.bindings = try_rebind(&config.bindings, GameKey::Confirm, KeyCode::Space)
            .expect("空格已解绑，不该冲突");
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-save").join("config.json5");
        let mut state = 设置状态(某行下标(SettingsRow::Save));
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        let (_, notice) = update_settings(&mut state, &按下(&[GameKey::Confirm]), &mut ctx);
        let 读回 = ll_platform::config::load_or_default(&path);

        // Assert
        assert_eq!(notice, Some(ScreenNotice::Saved));
        assert_eq!(
            读回
                .bindings
                .resolve(KeyCode::Space, Modifiers::NONE, EDITABLE_CONTEXT),
            Some(GameKey::Confirm),
            "存盘再读回之后，空格仍然是确认键"
        );
    }

    #[test]
    fn 设置界面按取消返回菜单屏() {
        // Arrange
        let mut config = GameConfig::default();
        let catalog = 测试目录();
        let path = crate::test_support::unique_temp_path("menu-screen-back");
        let mut state = 设置状态(0);
        let mut ctx = SettingsContext {
            config: &mut config,
            config_path: &path,
            catalog: &catalog,
        };

        // Act
        update_settings(&mut state, &按下(&[GameKey::Cancel]), &mut ctx);

        // Assert
        assert_eq!(state, ScreenState::Menu);
    }

    #[test]
    fn 未绑定的动作在设置界面显示成未绑定而不是空白() {
        // 空白会让玩家分不清「没绑」与「这一行坏了」。
        // Arrange
        let mut config = GameConfig::default();
        clear_bindings(&mut config, GameKey::Map);
        let catalog = 测试目录();

        // Act
        let text = binding_summary(&config.bindings, GameKey::Map, &catalog, &config.language);

        // Assert
        assert_eq!(
            text,
            catalog.resolve(&config.language, "screen-settings-unbound")
        );
    }

    #[test]
    fn 键位行同时列出一个动作的多个绑定() {
        // Up 默认同时绑着 ArrowUp 与 KeyW，只显示一个会让玩家以为丢了。
        // Arrange
        let config = GameConfig::default();
        let catalog = 测试目录();

        // Act
        let text = binding_summary(&config.bindings, GameKey::Up, &catalog, &config.language);

        // Assert
        assert!(text.contains("ArrowUp"), "实际是：{text}");
        assert!(text.contains("KeyW"), "实际是：{text}");
    }
}
