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

use ll_core::ident::ContentIndex;
use ll_i18n::{Catalog, FluentArgs};
use ll_platform::config::{GameConfig, ScaleFilter, save as save_config};
use ll_platform::input::{GameKey, InputState};
use ll_platform::keybind::{InputContext, KeyBinding, KeyBindings, KeyCode, Modifiers};
use ll_ui::screen::ScreenData;
use ll_ui::widget::focus::focused_widget;
use ll_ui::widget::state::{WidgetId, WidgetStateTable};
use ll_world::entity::EntityId;

use crate::spawn_pick::SpawnOrigin;

/// 模态屏当前开着哪一块。
///
/// 与 `crate::player_action::PlayerMenu` 同一条纪律：纯表现层状态，
/// 不进 `GameWorld`/`WorldState`、不进存档、不参与回放。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenState {
    /// 游戏主菜单，也就是首页（开始游戏 / 读取存档 / 设置 / 离开）。
    ///
    /// **它底下没有世界**——这是它与 [`ScreenState::Menu`] 唯一的、也是
    /// 全部的区别，见 `crate::app::Demo::session` 字段文档。
    Title,
    /// 游戏内菜单（继续游戏 / 设置 / 退出），底下有一局正在进行的世界。
    Menu,
    /// 角色创建（种族 / 性别 / 职业），见 [`crate::chargen`]。
    ///
    /// **它底下也没有世界**——与 [`ScreenState::Title`] 同一种状态，
    /// 只是玩家已经按下了「开始游戏」。
    CharacterCreation {
        /// 光标落在第几行。
        cursor: usize,
    },
    /// 世界（历史）生成配置，见 [`crate::world_setup`]。底下同样没有世界。
    WorldSetup {
        /// 光标落在第几行。
        cursor: usize,
    },
    /// 在世界地图上选出生地，见 [`crate::spawn_pick`]。
    ///
    /// **这一块屏底下有世界**——它必须先被生成出来，否则没有地图可看
    /// （见 `crate::spawn_pick` 模块文档「顺序」一节）。它因此是唯一
    /// 一块「世界已经存在，但玩家还没有真正入世」的屏：世界不推进
    /// （`Demo::advance` 因 `screen.is_some()` 早退），退出时也**不存档**
    /// （见 `crate::app::Demo::save_on_exit`）。
    ///
    /// 光标（选中哪一格）不在这里而在 `crate::chargen::NewGameDraft` 上：
    /// 它是一对 `(u32, u32)`，与这块屏的「哪一行」不是同一种东西。
    SpawnPick {
        /// 从哪一块屏进来的，按取消要回到那里。见
        /// [`crate::spawn_pick::SpawnOrigin`]。
        origin: SpawnOrigin,
    },
    /// 存档列表——首页的「读取存档」进这里，见 [`crate::save_list`]。
    ///
    /// **它底下没有世界**，与 [`ScreenState::Title`] 同一种状态。
    SaveList {
        /// 光标落在第几份存档上。
        cursor: usize,
    },
    /// 给这份存档起名字，见 [`crate::save_name`]。
    ///
    /// 插在**选出生地确认之后、真正进世界之前**：那一刻世界已经建好、
    /// 角色已经选好，起名是这条链的最后一步。之后每次存档都写同一个
    /// 槽位，不再问第二次。
    ///
    /// 正在输入的那串字不在这里而在 `crate::chargen::NewGameDraft` 上
    /// ——与选点光标同一条理由：`ScreenState` 是 `Copy` 的，装不下一个
    /// `String`，而且屏切走再切回来时那串字不该丢。
    SaveNaming {
        /// **选出生地屏**是从哪一块屏进来的——纯过路件，命名屏自己只有
        /// 一个入口。带着它是为了玩家从这里退回选点屏、再按一次取消时
        /// 能回到当初的来处，而不是让命名屏自己编一个。
        origin: SpawnOrigin,
    },
    /// 会话屏——跟一个 NPC 说话，见 [`crate::dialogue_screen`]。
    ///
    /// **它底下有一局正在进行的世界**，与 [`ScreenState::Menu`] 同一
    /// 种状态：世界不推进（`Demo::advance` 因 `screen.is_some()` 早退），
    /// 因此对话过程中说话人不可能走开或死掉。
    ///
    /// 会话位置（停在哪个节点、光标停在第几行）**是 UI 状态**，不进
    /// `WorldState`、不进存档、不进世界哈希（规格七节 7.1）。
    Dialogue {
        /// 跟谁说的这场话——一路带到
        /// `ll_sim::intent::Intent::DialogueChoose`，`join-settlement`
        /// 那一支要读他的 `ll_world::entity::Agent::home`
        /// （批次 21 第 1 条临时裁定的反转，理由见
        /// `crate::player_action::PlayerCommand::OpenDialogue` 文档）。
        speaker: EntityId,
        /// 现在停在哪个对话节点上。
        node: ContentIndex,
        /// 光标落在**过滤后**的第几行——注意不是选项的原始下标，
        /// 见 `crate::dialogue_screen::DialogueRow`。
        cursor: usize,
    },
    /// 交易屏——跟一个 NPC 买卖，见 [`crate::trade_screen`]。
    ///
    /// **它底下有一局正在进行的世界**，与 [`ScreenState::Dialogue`]
    /// 同一种状态：世界不推进（`Demo::advance` 因 `screen.is_some()`
    /// 早退），因此交易过程中对方不可能走开或死掉。
    ///
    /// 进这块屏的唯一入口是对话选项上的 `open-trade` 后果——**那条后果
    /// 不产任何 `Effect`，只推 UI**（规格五节 5.3）；真正的钱货两清是
    /// `ll_sim::intent::Intent::Trade`，照旧走 `apply` 唯一写入口。
    ///
    /// 退出时回到的是**世界**，不是刚才那段对话：`crate::modal::Modal`
    /// 只有一个 `Option<ScreenState>`，不是栈。如实登记在计划文档
    /// `docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md`
    /// 二节 2.3 与十一节。
    Trade {
        /// 跟谁做这笔买卖——一路带到 `ll_sim::intent::Intent::Trade`，
        /// 价格系数读的是玩家与**他所属势力**之间的声望。
        partner: EntityId,
        /// 光标落在第几行（`crate::trade_screen::trade_rows` 产出的
        /// 那张表里的下标）。
        cursor: usize,
    },
    /// 设置界面。
    Settings {
        /// 光标落在第几行，见模块文档「焦点导航」一节。
        cursor: usize,
        /// 是否正处于**捕获模式**——已经按下确认要给这一行重绑键位，
        /// 正等玩家按下想绑的那个物理键。
        capturing: bool,
        /// 从哪一块屏进来的，按取消/返回要回到那里。
        origin: SettingsOrigin,
    },
}

impl ScreenState {
    /// 这块屏是不是要玩家**打字**（而不是在若干项之间选一项）。
    ///
    /// # 它决定三件事，因为那三件事本来就是一件
    ///
    /// 返回真的那一帧，`crate::app::Demo` 会往
    /// `ll_ui::widget::ui_mode::UiModeStack` 压一层 `UiMode::TextEntry`，
    /// 于是：输入上下文切到 `InputContext::TextEntry`（WASD 解析不出
    /// 动作，空格变回一个字符）、事件循环开启输入法、文本通道开始
    /// 收数据。三者共用这一个判据，见
    /// `ll_platform::window::AppHandler::input_context` 文档。
    ///
    /// # 为什么是屏的属性，不是某处 `if screen == SaveNaming`
    ///
    /// 将来的角色命名、聊天、mod 搜索框都要走同一条路。写成屏自己的
    /// 属性，新屏只需在这里加一行；写成散在路由里的判等，每加一块屏
    /// 就要有人记得去改那处判等——而忘了改不会有任何东西报错，只会
    /// 表现为「那块屏打不了中文」。
    pub fn wants_text_entry(self) -> bool {
        matches!(self, ScreenState::SaveNaming { .. })
    }
}

/// 设置界面是从哪一块屏进来的。
///
/// # 为什么必须记住它
///
/// 设置屏现在有两个入口：首页与游戏内菜单（所有者要求「首页的设置和
/// 暂停菜单的设置必须是同一块屏」，因此不能靠开两份屏来区分）。而按
/// 「返回」时写死回 [`ScreenState::Menu`] 会把从首页进来的玩家扔进一个
/// **底下没有世界**的暂停菜单——那块屏的第一项是「继续游戏」，按下去
/// 会关掉整块屏，露出一个空世界。
///
/// # 为什么不是一个通用的「返回栈」
///
/// 通用返回栈是 `ll_ui::widget::ui_mode::UiModeStack` 的职责范围，但那个
/// 栈目前只表达输入上下文、不表达具体是哪一块屏（见其 `UiMode` 文档：
/// 刻意只有一个变体）。把「哪一块屏」也塞进去是一次独立的扩展，不夹带
/// 在本批次里。两个入口用一个两变体的枚举表达是当前最诚实的形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsOrigin {
    /// 从首页进来的。
    Title,
    /// 从游戏内菜单进来的。
    Menu,
}

impl SettingsOrigin {
    /// 按取消/返回该回到哪一块屏。
    pub fn screen(self) -> ScreenState {
        match self {
            SettingsOrigin::Title => ScreenState::Title,
            SettingsOrigin::Menu => ScreenState::Menu,
        }
    }
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
    /// 建一局全新的世界并进去——首页的「开始游戏」。
    ///
    /// **本批次它就是「直接建新档进世界」**；角色创建、世界配置、选
    /// 重生点是后续批次，衔接点就是调用方对这个变体的处理，见本批次
    /// 计划文档第七节。
    StartNewGame,
    /// 读回磁盘上那份存档并进去——首页的「读取存档」。
    ///
    /// 只有在存档确实存在时才可能产出（见 [`crate::title_screen::update_title`]）：存档不
    /// 存在时那一行按下去只会得到 [`ScreenNotice::NoSave`]，**绝不**
    /// 悄悄改成开一局新游戏。
    LoadSave,
    /// 手动存一次档，存完**留在菜单里**——暂停菜单的「保存」。
    ///
    /// 存完不自动关菜单：玩家按「保存」的意图是「把进度落盘」，不是
    /// 「回到游戏」；顺手关掉会让他看不到那句「已保存」，而写盘失败时
    /// 更会把唯一一次报错一并关掉。
    SaveNow,
    /// 回到游戏主菜单（首页）——暂停菜单的「返回主菜单」。
    ///
    /// **调用方必须先存一次再回去**，见 `crate::app::Demo::back_to_title`
    /// 文档「未保存的进度怎么办」一节。
    BackToTitle,
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

impl crate::nav_row::NavRow for SettingsRow {
    /// 「返回」是**返回**——退回 `origin` 那一层（首页或游戏内菜单），
    /// 不是关到底。见 `crate::nav_row` 模块文档。
    fn nav_role(self) -> Option<crate::nav_row::NavRole> {
        match self {
            SettingsRow::Back => Some(crate::nav_row::NavRole::Back),
            SettingsRow::Language
            | SettingsRow::Vsync
            | SettingsRow::ScaleFilter
            | SettingsRow::KeybindsHeader
            | SettingsRow::Keybind(_)
            | SettingsRow::Save => None,
        }
    }
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
    /// 玩家死了：存档**保留**，这个世界的模式转为普通，请重新创建一个
    /// 角色。
    ///
    /// 所有者的修正原话：「死亡后变成一般模式，可以再创建角色然后选择在
    /// 某个地方出生。」
    PlayerDied,
    /// 游戏进度已写回磁盘（暂停菜单的「保存」）。
    ///
    /// 与 [`Self::Saved`] 刻意分开：一个说的是设置，一个说的是存档，
    /// 玩家在同一块屏上会先后看到这两句，混用一句会让他分不清刚才存的
    /// 到底是什么。
    GameSaved,
    /// 游戏进度写盘失败——**进度还在内存里，什么都没丢**，但没存下来。
    GameSaveFailed,
    /// 配置写盘失败——本次会话内的改动仍然有效，只是没存下来。
    SaveFailed,
    /// 首页按了「读取存档」，但磁盘上没有存档。
    NoSave,
    /// 首页按了「读取存档」，存档存在但读不回来（损坏，或因缺失内容
    /// 降级为只读）。**留在首页**，不悄悄退回新游戏。
    LoadFailed,
    /// 世界配置屏上的这次调整会让形态参数越界，**整体被丢弃**——判据
    /// 是 `ll_world::terrain_shape::TerrainShape::validate`，UI 层不抄
    /// 第二份，见 `crate::world_setup` 模块文档。
    InvalidTerrainShape,
    /// 选出生地屏：玩家点的那个区块里没有任何可站立的格子（全是水/
    /// 全是山），请重选。
    ///
    /// **刻意不自动换到邻近区块**，理由见
    /// `crate::spawn_pick::pick_spawn_in_zone` 文档「退化策略」一节。
    NoLandInZone,
}

impl ScreenNotice {
    /// 这条提示对应的 Fluent 键。
    fn i18n_key(self) -> &'static str {
        match self {
            ScreenNotice::Conflict(_) => "screen-settings-conflict",
            ScreenNotice::Bound(_) => "screen-settings-bound",
            ScreenNotice::Cleared(_) => "screen-settings-cleared",
            ScreenNotice::Saved => "screen-settings-saved",
            ScreenNotice::PlayerDied => "screen-chargen-player-died",
            ScreenNotice::GameSaved => "screen-menu-game-saved",
            ScreenNotice::GameSaveFailed => "screen-menu-game-save-failed",
            ScreenNotice::SaveFailed => "screen-settings-save-failed",
            ScreenNotice::NoSave => "screen-title-no-save",
            ScreenNotice::LoadFailed => "screen-title-load-failed",
            ScreenNotice::InvalidTerrainShape => "screen-worldsetup-invalid",
            ScreenNotice::NoLandInZone => "screen-spawnpick-no-land",
        }
    }

    /// 解析成一句可以直接画在屏上的话。
    pub fn resolve(self, catalog: &Catalog, language: &str) -> String {
        let action = match self {
            ScreenNotice::Conflict(action)
            | ScreenNotice::Bound(action)
            | ScreenNotice::Cleared(action) => Some(action),
            ScreenNotice::Saved
            | ScreenNotice::PlayerDied
            | ScreenNotice::GameSaved
            | ScreenNotice::GameSaveFailed
            | ScreenNotice::SaveFailed
            | ScreenNotice::NoSave
            | ScreenNotice::LoadFailed
            | ScreenNotice::InvalidTerrainShape
            | ScreenNotice::NoLandInZone => None,
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

/// 建一张**第一项已经聚焦**的焦点表——规格 N10 的落点。
///
/// # 为什么复用 `move_focus` 而不是直接写 `entry(ids[0]).focused = true`
///
/// 「一组控件里第一项是哪个、聚焦时其余项要不要一并清掉」这条算法
/// `ll_ui::widget::focus::move_focus` 已经有了（冷启动 `Next` → 第 0
/// 项，并保证「至多一个控件聚焦」这条不变式）。自己写一遍就是同一个
/// 算法的第二份，而两份迟早分叉。
///
/// `ids` 为空时返回一张空表——`move_focus` 对空列表返回 `None` 且不
/// 修改表，这里照样不特殊处理。
pub fn preselected_focus(ids: &[WidgetId]) -> WidgetStateTable {
    let mut table = WidgetStateTable::new();
    ll_ui::widget::focus::move_focus(&mut table, ids, ll_ui::widget::focus::FocusDirection::Next);
    table
}

/// `ids` 这一组控件里当前聚焦的是第几个；没有任何一项聚焦时返回
/// `usize::MAX` ——那是一个**必然越界**的下标，`ll_ui::screen` 收到越界
/// 光标时不标记任何一行（见 `ScreenData::cursor` 文档），正好等于「还没
/// 选中任何一项」这个事实。不返回 `0`：那会让屏上看起来「第一项已经被
/// 选中」，与实际状态不符。
///
/// 首页与菜单屏共用本函数——两块屏的导航是同一套，只是 id 表不同。
pub fn focus_index(table: &WidgetStateTable, ids: &[WidgetId]) -> usize {
    focused_widget(table, ids)
        .and_then(|id| ids.iter().position(|candidate| *candidate == id))
        .unwrap_or(usize::MAX)
}

/// 建出这一帧要交给 `ll_ui::screen` 的数据。
/// `title_key` 只被 [`ScreenState::Dialogue`] 那一支读——其余各支的
/// 标题是一个写死的字面量键。收一个参数而不是让本函数自己去查内容表，
/// 与本模块「只收已经排好版的字符串，不收领域类型」那条既有分工一致
/// （见模块文档「为什么只有一种屏」一节）。
pub fn screen_data<'a>(
    state: ScreenState,
    rows: &'a [String],
    focus: usize,
    notice: Option<&'a str>,
    title_key: &'a str,
) -> ScreenData<'a> {
    match state {
        ScreenState::Title => ScreenData {
            title_key: "screen-title-title",
            rows,
            cursor: focus,
            empty_key: "screen-title-empty",
            hint_key: "screen-title-hint",
            notice,
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
        },
        ScreenState::Menu => ScreenData {
            title_key: "screen-menu-title",
            rows,
            cursor: focus,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice,
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
        },
        ScreenState::CharacterCreation { .. } => ScreenData {
            title_key: "screen-chargen-title",
            rows,
            cursor: focus,
            empty_key: "screen-chargen-empty",
            hint_key: "screen-chargen-hint",
            notice,
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
        },
        ScreenState::WorldSetup { .. } => ScreenData {
            title_key: "screen-worldsetup-title",
            rows,
            cursor: focus,
            empty_key: "screen-chargen-empty",
            hint_key: "screen-worldsetup-hint",
            notice,
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
        },
        // 选出生地屏**不走这块居中面板**：它的「屏」就是整张世界地图，
        // 一块盖在正中央的面板会挡住玩家要点的地方。调用方
        // （`crate::app::draw_screen`）为这个变体整块跳过，本函数因此
        // 永远不该收到它——但仍然给一个诚实的退化产出而不是 panic，
        // 与本模块其余降级路径一致。
        ScreenState::SpawnPick { .. } => ScreenData {
            title_key: "screen-spawnpick-title",
            rows,
            cursor: focus,
            empty_key: "screen-chargen-empty",
            hint_key: "screen-spawnpick-hint",
            notice,
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
        },
        ScreenState::SaveList { cursor } => ScreenData {
            title_key: "screen-savelist-title",
            rows,
            cursor,
            empty_key: "screen-savelist-empty",
            hint_key: "screen-savelist-hint",
            notice,
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
        },
        ScreenState::SaveNaming { .. } => ScreenData {
            title_key: "screen-savename-title",
            rows,
            // 命名屏没有「选中哪一行」这回事——两行都是给玩家看的，
            // 光标是那串字尾巴上的下划线。`usize::MAX` 是本仓库既有的
            // 「一行都没选中」表示（见 `focus_index`）。
            cursor: usize::MAX,
            empty_key: "screen-savelist-empty",
            hint_key: "screen-savename-hint",
            notice,
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
        },
        // 会话屏的**标题就是 NPC 说的那一句**——它本身是一条 Fluent
        // 键，见 `crate::dialogue_screen` 模块文档那张表。因此这一支
        // 不能像其余各支那样写一个字面量键，键由调用方现算好传进来。
        ScreenState::Dialogue { .. } => ScreenData {
            title_key,
            rows,
            cursor: focus,
            empty_key: "screen-dialogue-empty",
            hint_key: "screen-dialogue-hint",
            notice,
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
        },
        // 交易屏：行是两边的货，标题是一个写死的字面量键（只有会话屏
        // 的标题是现算的）。见 `crate::trade_screen` 模块文档那张表。
        ScreenState::Trade { .. } => ScreenData {
            title_key: "screen-trade-title",
            rows,
            cursor: focus,
            empty_key: "screen-trade-empty",
            hint_key: "screen-trade-hint",
            notice,
            hovered: None,
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
            // 悬停行由调用方（`app::draw_screen`）在拿到这份数据之后
            // 补上——它是**指针**这一帧的事实，不是屏状态的一部分。
            hovered: None,
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

/// 把这一帧的指针动作落到一张**焦点表**上（首页与暂停菜单用它）。
///
/// 只做「把焦点挪到指针那一行」这一件事——「要不要因此触发」由调用方
/// 与键盘确认键并起来判，两条路径因此走同一个动作分派分支。
///
/// `row` 越界时什么都不做：行矩形与 `ids` 同源现算，越界只可能是两者
/// 在两帧之间不同步，那时候不动焦点比猜一个安全。
pub fn apply_row_pointer(
    table: &mut WidgetStateTable,
    ids: &[WidgetId],
    pointer: crate::pointer::RowPointer,
) {
    let Some(row) = pointer.focus_row() else {
        return;
    };
    if row >= ids.len() {
        return;
    }
    for (index, id) in ids.iter().enumerate() {
        table.entry(id).focused = index == row;
    }
}

/// 处理设置界面这一帧的输入，返回这一帧产生的提示（若有）。
///
/// 拆成「捕获模式」与「常规模式」两条，是因为两者读的**根本不是同一种
/// 输入**：捕获模式读的是原始物理键（`InputState::last_physical_key`，
/// 绕过绑定表），常规模式读的是抽象动作（`GameKey`）。
///
/// `pointer` 见 `crate::pointer` 模块文档——它在**捕获模式下一律不
/// 生效**：那一刻整块屏的语义是「按下你想绑的那个物理键」，鼠标点一行
/// 没有任何可对应的动作，而把光标挪走会让玩家绑到另一个动作上。
pub fn update_settings(
    state: &mut ScreenState,
    input: &InputState,
    pointer: crate::pointer::RowPointer,
    ctx: &mut SettingsContext<'_>,
) -> SettingsUpdate {
    let ScreenState::Settings {
        cursor,
        capturing,
        origin,
    } = *state
    else {
        return SettingsUpdate::idle();
    };
    let rows = settings_rows();
    if capturing {
        // **捕获模式下指针一律不生效**：那一刻整块屏的语义是「按下你
        // 想绑的那个物理键」，鼠标点一行没有任何可对应的动作，而把光标
        // 挪走会让玩家绑到另一个动作上。
        return update_capture(state, input, ctx, &rows, cursor, origin);
    }
    // 指针按下把光标挪过去；越界不动（行矩形与 `rows` 同源现算）。
    let cursor = match pointer.focus_row() {
        Some(row) if row < rows.len() => row,
        _ => cursor,
    };
    update_navigation(state, input, pointer, ctx, &rows, cursor, origin)
}

/// 捕获模式：只看原始物理键。
fn update_capture(
    state: &mut ScreenState,
    input: &InputState,
    ctx: &mut SettingsContext<'_>,
    rows: &[SettingsRow],
    cursor: usize,
    origin: SettingsOrigin,
) -> SettingsUpdate {
    // 这一帧没按任何物理键——捕获模式绝大多数帧走的都是这一条，也正是
    // 「屏开着但玩家什么都不按」那些帧不该产生任何键位表克隆的原因。
    let Some(key) = input.last_physical_key() else {
        return SettingsUpdate::idle();
    };
    let Some(SettingsRow::Keybind(action)) = rows.get(cursor).copied() else {
        // 光标不在键位行上却进了捕获模式——不该发生，但退出捕获比
        // panic 好（一个纯 UI 状态问题不该拖垮整局）。
        *state = leave_capture(cursor, origin);
        return SettingsUpdate::idle();
    };
    // Esc 取消、退格解绑——两个键因此不可绑，代价写进本批次计划文档
    // D5。走原始物理键而不是 `GameKey::Cancel`：捕获模式的整个语义就是
    // 「这一刻不查绑定表」，为这两个键破例去查表会自相矛盾。
    match key {
        KeyCode::Escape => {
            *state = leave_capture(cursor, origin);
            SettingsUpdate::idle()
        }
        KeyCode::Backspace => {
            // 解绑：键位表真的变了，两处改键位入口之一。
            clear_bindings(ctx.config, action);
            *state = leave_capture(cursor, origin);
            SettingsUpdate::rebound(ScreenNotice::Cleared(action))
        }
        key => apply_capture(state, ctx, action, key, cursor, origin),
    }
}

/// 退出捕获模式、光标留在原处，**入口来源原样带着**——退出捕获不是
/// 换一块屏，来源不该在这里被改写。
fn leave_capture(cursor: usize, origin: SettingsOrigin) -> ScreenState {
    ScreenState::Settings {
        cursor,
        capturing: false,
        origin,
    }
}

/// 玩家按下的是一个普通物理键：试着绑上去。
fn apply_capture(
    state: &mut ScreenState,
    ctx: &mut SettingsContext<'_>,
    action: GameKey,
    key: KeyCode,
    cursor: usize,
    origin: SettingsOrigin,
) -> SettingsUpdate {
    match try_rebind(&ctx.config.bindings, action, key) {
        Ok(bindings) => {
            // 重绑成功：键位表真的变了，两处改键位入口之二。
            ctx.config.bindings = bindings;
            // 重新绑上了，「刻意解绑」这个意图随之作废。
            ctx.config.unbound_actions.retain(|it| *it != action);
            *state = leave_capture(cursor, origin);
            SettingsUpdate::rebound(ScreenNotice::Bound(action))
        }
        // 冲突：**留在捕获模式**，玩家可以直接再按一个别的键，不用重新
        // 进一次。表一个字节都没变，`rebound` 因此为假。
        Err(occupied) => SettingsUpdate::saying(ScreenNotice::Conflict(occupied)),
    }
}

/// 常规模式：上下移动光标、左右改取值、确认触发这一行的动作。
///
/// # 确认键只做一件事：激活当前焦点（规格 N1）
///
/// 本函数此前还有第二件事：光标停在**取值行**（语言 / 垂直同步 /
/// 滤波）上按确认等价于「把这个值往前拨一格」，与左右键一致。
///
/// **那条特例已经删掉。** 它与同一个代码库里另外两块屏直接冲突——
/// 角色创建（`crate::chargen`）与世界配置（`crate::world_setup`）的
/// 取值行按确认**刻意是空操作**，而且 `chargen` 那处有注释说明这是
/// 有意的。同一个物理键在三块长得一样的屏上做两件不同的事，玩家学不
/// 到任何规律（规格 I7）。
///
/// 裁定是**统一到「确认键绝不改变数值、绝不切换开关」**：数值一律用
/// 左右键改。代价如实记录——设置屏切语言从此非用左右键不可，少了一条
/// 冗余路径；换来的是「确认 = 激活当前焦点」这条规律在全部十块屏上
/// 无例外，鼠标点击（`crate::pointer`）也就能安全地复用它。
#[allow(clippy::too_many_arguments)]
fn update_navigation(
    state: &mut ScreenState,
    input: &InputState,
    pointer: crate::pointer::RowPointer,
    ctx: &mut SettingsContext<'_>,
    rows: &[SettingsRow],
    cursor: usize,
    origin: SettingsOrigin,
) -> SettingsUpdate {
    if input.was_just_pressed(GameKey::Cancel) {
        // 回到进来的那一块屏，不是写死的菜单屏——见 `SettingsOrigin`。
        *state = origin.screen();
        return SettingsUpdate::idle();
    }
    if let Some(next) = moved_cursor(input, cursor, rows.len()) {
        *state = ScreenState::Settings {
            cursor: next,
            capturing: false,
            origin,
        };
        return SettingsUpdate::idle();
    }
    // 指针挪过光标但这一帧没有触发：把新光标写回屏状态就返回。
    if matches!(pointer, crate::pointer::RowPointer::Focus(_)) {
        *state = ScreenState::Settings {
            cursor,
            capturing: false,
            origin,
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
    if !input.was_just_pressed(GameKey::Confirm) && !pointer.activated() {
        return SettingsUpdate::idle();
    }
    match row {
        SettingsRow::Keybind(_) => {
            *state = ScreenState::Settings {
                cursor,
                capturing: true,
                origin,
            };
            SettingsUpdate::idle()
        }
        // 保存写的是磁盘，不动内存里那张表——`rebound` 因此为假。
        SettingsRow::Save => SettingsUpdate::saying(save_settings(ctx)),
        SettingsRow::Back => {
            *state = origin.screen();
            SettingsUpdate::idle()
        }
        // 分隔标题与**三个取值行**（语言 / 垂直同步 / 滤波）按确认一律
        // 什么都不做——见本函数文档「确认键只做一件事」一节。
        SettingsRow::KeybindsHeader
        | SettingsRow::Language
        | SettingsRow::Vsync
        | SettingsRow::ScaleFilter => SettingsUpdate::idle(),
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
