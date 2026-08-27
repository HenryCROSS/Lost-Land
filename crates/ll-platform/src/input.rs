//! 输入状态聚合。
//!
//! 本模块把操作系统的按键事件归约为**游戏语义的动作**，而不是把物理
//! 按键直接暴露给上层。这样按键重绑定与手柄支持只需改本模块的映射表，
//! 上层逻辑一行都不用动。
//!
//! # 为什么要区分「按住」与「刚按下」
//!
//! 回合制里这两者语义完全不同：按住方向键应当连续移动，但按住确认键
//! 绝不能反复触发同一个菜单项。操作系统还会发送按键重复事件，若不去重，
//! 长按确认键会把整个菜单一路点穿。
//!
//! # 自动重复只对部分键开放
//!
//! 出于上一段同样的理由，`InputState` 的自动重复机制只对方向键与等待键
//! 生效（见 [`GameKey::is_repeatable`]）。确认/取消/菜单/地图/截图这类
//! 一次性动作键若也参与自动重复，等于把 `press()` 特意去重的问题又引入
//! 回来。
//!
//! # 计时只属于输入层
//!
//! 自动重复需要墙钟时间，因此本模块使用 [`std::time::Instant`] 与
//! [`std::time::Duration`]。这两个类型**绝不可**流入世界状态或被存档
//! 序列化——世界状态的时间只认确定性的 `Tick`，`Instant` 在不同机器、
//! 不同进程之间不可比较，混进去会破坏重放的确定性。

use ll_core::ident::NamespacedId;
use std::time::{Duration, Instant};

/// 游戏语义的动作键。
///
/// 上层只认这些，不认物理按键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GameKey {
    /// 向上移动或菜单上移。
    Up,
    /// 向下移动或菜单下移。
    Down,
    /// 向左移动或菜单左移。
    Left,
    /// 向右移动或菜单右移。
    Right,
    /// 确认。
    Confirm,
    /// 取消或返回。
    Cancel,
    /// 打开主菜单。
    Menu,
    /// 打开世界地图。
    Map,
    /// 原地等待一回合。
    Wait,
    /// 把当前画面存成视觉回归基准。
    ///
    /// 这是冻结基准的入口，不是调试功能——基准是需要被保护的资产，
    /// 详见 `crates/ll-render/tests/visual/README.md` 的处置规矩。
    Screenshot,
    /// 放大画面（拉近视角）。可由按键（长按参与自动重复）或滚轮（每次
    /// 滚动一格触发一次脉冲，见 [`InputState::pulse`]）触发，见
    /// `crate::keybind` 模块文档「滚轮滚动的离散方向」一节。
    ZoomIn,
    /// 缩小画面（拉远视角），理由同 `ZoomIn`。
    ZoomOut,
    /// 打开/关闭背包菜单（可选中一条，见 `ll_game::player_action`）。
    ///
    /// # 这一组六个键为什么存在
    ///
    /// `ll_sim::intent::Intent` 的二十三个变体里，物品链那六个
    /// （`PickUp`/`Drop`/`Equip`/`Unequip`/`Use`/`Craft`）此前**一个都
    /// 没有键位产出者**：引擎侧的结算、效果、测试全都齐全，玩家在真实
    /// 游戏里却一个都按不出来。本组键就是补上那条断线，见
    /// `ll_sim::intent::intent_from_input` 文档「六个玩法意图的产出者
    /// 在哪」一节。
    Inventory,
    /// 打开/关闭制作菜单（选一条配方，见 `Inventory` 文档同一节）。
    Craft,
    /// 捡起脚下的东西（`ll_sim::intent::Intent::PickUp`）。
    PickUp,
    /// 丢下背包菜单里选中的那一堆（`ll_sim::intent::Intent::Drop`）。
    ///
    /// **丢弃不是放置**——把一件家具丢在脚下，它是躺着的一堆普通物品；
    /// 立起来要按 [`GameKey::Place`]。两个动作分开是项目所有者的裁定，
    /// 完整论证见 `ll_sim::intent::Intent::Place` 文档。
    Drop,
    /// 装备/卸下背包菜单里选中的那一条——落在背包段就是装备
    /// （`Intent::Equip`），落在装备段就是卸下（`Intent::Unequip`），
    /// 见 `ll_game::player_action` 模块文档「为什么装备与卸下共用一个
    /// 键」一节。
    Equip,
    /// 使用背包菜单里选中的那一堆（`ll_sim::intent::Intent::Use`）。
    Use,
    /// 把背包菜单里选中的那一件**立**在脚下（`ll_sim::intent::Intent::Place`）。
    Place,
    /// 与最近的东西交互——项目所有者定的形状：
    ///
    /// > 按空格可以与最近的东西做交互，例如地上的物品，如果地上没有就
    /// > 空格加上 wasd 选择交互的方向。这些都可以修改键位的。
    ///
    /// 「最近的东西」= **脚下这一格优先**，脚下什么都没有时由玩家再按
    /// 一个方向键指出要和哪一格交互；不是「在半径 N 内搜一个最近的
    /// 目标」——半径搜索会造出「玩家不知道自己要和哪个东西交互」的歧义，
    /// 指方向没有这个问题。
    ///
    /// 「这些都可以修改键位的」这一句由本枚举的存在本身满足：本键与其余
    /// 每一个动作一样只是一个**抽象动作**，它绑在哪个物理键上由
    /// `crate::keybind::KeyBindings` 决定（默认空格，见
    /// `crate::keybind::DEFAULT_BINDINGS`），玩家可以在 `config.json5`
    /// 里改。输入处理层从不比对键码。
    Interact,
}

/// 全部动作键，顺序必须与 [`GameKey`] 的变体声明顺序一致。
///
/// 存在的意义是让「新增变体」变成编译期错误：下方的穷尽 match 测试会在
/// 漏登记时拒绝编译。若只靠手抄一个数字常量，新增变体后代码照常编译、
/// 测试照常通过，直到运行时数组越界才暴露。
///
/// `InputState::begin_frame` 也遍历它来推进每个键的重复计时。
const ALL_KEYS: [GameKey; KEY_COUNT] = [
    GameKey::Up,
    GameKey::Down,
    GameKey::Left,
    GameKey::Right,
    GameKey::Confirm,
    GameKey::Cancel,
    GameKey::Menu,
    GameKey::Map,
    GameKey::Wait,
    GameKey::Screenshot,
    GameKey::ZoomIn,
    GameKey::ZoomOut,
    GameKey::Inventory,
    GameKey::Craft,
    GameKey::PickUp,
    GameKey::Drop,
    GameKey::Equip,
    GameKey::Use,
    GameKey::Place,
    GameKey::Interact,
];

/// 动作键总数，用于状态数组定长。
const KEY_COUNT: usize = 20;

impl GameKey {
    /// 在状态数组中的下标。
    const fn index(self) -> usize {
        self as usize
    }

    /// 该键是否参与按键自动重复。
    ///
    /// 方向键与等待键长按连续触发是回合制的刚需；缩放键同理——长按
    /// 放大/缩小键应当连续变化视野，而不是像菜单操作那样一次只响应
    /// 一格。确认/取消/菜单/地图/截图这类一次性动作键则相反——按住若
    /// 反复触发，会把整个菜单一路点穿、把视觉回归基准反复覆写，这正是
    /// [`InputState::press`] 对操作系统按键重复事件去重要防的问题，
    /// 让这些键参与自动重复等于开倒车。
    ///
    /// **滚轮触发的缩放不受这条自动重复机制影响**：滚轮从不调用
    /// `press`/`release`，而是每次滚动调用一次 [`InputState::pulse`]
    /// （见其文档），`pulse` 不置起 `held`，因此不会进入
    /// [`InputState::begin_frame`] 的自动重复计时——滚一格就只触发
    /// 一次，多滚才多触发，这与滚轮天然的「离散步进」手感一致；只有
    /// **按键**长按缩放键时才会经这条自动重复机制连续触发。
    ///
    /// 物品链那几个键（`Inventory`/`Craft`/`PickUp`/`Drop`/`Place`/
    /// `Equip`/`Use`）与确认/取消同一侧：**不参与**自动重复。它们每一次激活
    /// 要么翻转一块菜单、要么提交一次真正消耗回合的
    /// `ll_sim::intent::Intent`，长按反复触发会把整背包一路丢光、把
    /// 食材一路烧完，正是上一段点名要防的那类问题。
    pub const fn is_repeatable(self) -> bool {
        matches!(
            self,
            GameKey::Up
                | GameKey::Down
                | GameKey::Left
                | GameKey::Right
                | GameKey::Wait
                | GameKey::ZoomIn
                | GameKey::ZoomOut
        )
    }

    /// 这个动作在设置界面等 UI 场景下的显示名，走 i18n 而不是硬编码
    /// 中文/英文字符串——与 `ll-mod` 的 `ClassDef`/`SubclassDef` 用
    /// `display_name_key: NamespacedId` 表达显示名同一个做法（见
    /// `crates/ll-mod/src/class.rs`）、与 `crate::window::WindowConfig`
    /// 的 `title_key` 同一个理由：显示字符串是用户可见内容，必须能被
    /// 翻译，硬编码任何一种语言都会在做本地化时被迫回头重构。
    ///
    /// 调用 `expect`：这里的每个键都是编译期写死的合法字面量，解析
    /// 失败只可能是本方法自身写错了命名空间格式，属于开发期就该
    /// 发现的缺陷，不是需要向调用方传播的运行期错误。
    pub fn display_name_key(self) -> NamespacedId {
        let raw = match self {
            GameKey::Up => "lostland:keybind.action.up",
            GameKey::Down => "lostland:keybind.action.down",
            GameKey::Left => "lostland:keybind.action.left",
            GameKey::Right => "lostland:keybind.action.right",
            GameKey::Confirm => "lostland:keybind.action.confirm",
            GameKey::Cancel => "lostland:keybind.action.cancel",
            GameKey::Menu => "lostland:keybind.action.menu",
            GameKey::Map => "lostland:keybind.action.map",
            GameKey::Wait => "lostland:keybind.action.wait",
            GameKey::Screenshot => "lostland:keybind.action.screenshot",
            GameKey::ZoomIn => "lostland:keybind.action.zoom_in",
            GameKey::ZoomOut => "lostland:keybind.action.zoom_out",
            GameKey::Inventory => "lostland:keybind.action.inventory",
            GameKey::Craft => "lostland:keybind.action.craft",
            GameKey::PickUp => "lostland:keybind.action.pick_up",
            GameKey::Drop => "lostland:keybind.action.drop",
            GameKey::Equip => "lostland:keybind.action.equip",
            GameKey::Use => "lostland:keybind.action.use",
            GameKey::Place => "lostland:keybind.action.place",
            GameKey::Interact => "lostland:keybind.action.interact",
        };
        NamespacedId::parse(raw).expect("硬编码的 i18n 键必然是合法的命名空间标识符")
    }
}

/// 按键自动重复触发前的默认等待时间。
///
/// 太短会让「只想走一格」的轻点变成连续移动好几格——玩家按下与松开
/// 之间总有几十毫秒的生理延迟，这个值必须盖过它。
const DEFAULT_INITIAL_DELAY: Duration = Duration::from_millis(350);

/// 按键自动重复触发后，后续每次重复的默认间隔。
///
/// 太长则长按移动的手感迟钝像卡了一拍；太短则会在慢帧补发重复时
/// （见 [`InputState::begin_frame`]）放大瞬移的视觉冲击。
const DEFAULT_REPEAT_INTERVAL: Duration = Duration::from_millis(90);

/// 按键自动重复的时序参数。
#[derive(Debug, Clone, Copy)]
pub struct RepeatConfig {
    /// 首次自动重复前的等待时间。
    pub initial_delay: Duration,
    /// 之后每次重复的间隔。
    pub interval: Duration,
}

impl Default for RepeatConfig {
    fn default() -> Self {
        RepeatConfig {
            initial_delay: DEFAULT_INITIAL_DELAY,
            interval: DEFAULT_REPEAT_INTERVAL,
        }
    }
}

/// 鼠标按键——UI 交互层批次新增，见模块顶层「光标与鼠标按键」一节。
///
/// 只收三个：`winit::event::MouseButton` 还有 `Back`/`Forward`/
/// `Other(u16)`，本批次的按钮控件（[`crate::window`] 事件循环的
/// `WindowEvent::MouseInput` 分支）只需要能区分左右中三键即可判断
/// 「这是不是一次点击」，额外的侧键当前没有任何消费者，不提前为它们
/// 建索引——这与 `GameKey`「只登记游戏语义需要的动作」是同一条克制。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// 左键——点击按钮的主键。
    Left,
    /// 右键。
    Right,
    /// 中键（滚轮按下）。
    Middle,
}

/// 鼠标按键总数，用于状态数组定长，理由同 [`KEY_COUNT`]。
const MOUSE_BUTTON_COUNT: usize = 3;

impl MouseButton {
    const fn index(self) -> usize {
        match self {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
        }
    }
}

/// 一帧内的输入状态。
///
/// 用定长数组而非哈希集合：动作键数量固定且很少，数组查询是一次下标
/// 访问，而哈希查询涉及哈希计算与可能的冲突处理。输入查询在每帧的 UI
/// 与逻辑中被调用数十次，这个差别值得。
#[derive(Debug, Clone)]
pub struct InputState {
    held: [bool; KEY_COUNT],
    just_pressed: [bool; KEY_COUNT],
    /// 每个键的下一次自动重复时刻；`None` 表示当前未按住，或该键
    /// 尚未经过一次 `begin_frame` 建立计时基准。
    ///
    /// 只在输入层内部使用，`Instant` 绝不可流入世界状态或被存档序列化。
    repeat_next_at: [Option<Instant>; KEY_COUNT],
    /// 本帧是否由自动重复触发。随 `just_pressed` 一并在 `end_frame` 清除。
    repeated: [bool; KEY_COUNT],
    /// 光标在窗口原生像素坐标系下的当前位置——与
    /// `crate::window::WindowConfig`/`ll_ui::widget::geometry::Rect` 同一
    /// 套坐标系（窗口原生分辨率像素，不是 640×360 逻辑分辨率），命中
    /// 测试因此可以直接拿这个值去比较控件的 `Rect` 而不需要任何换算。
    ///
    /// `None` 表示「本次会话还没收到过一次光标位置」或「光标已经离开
    /// 窗口」（见 [`Self::clear_cursor_position`]）——命中测试对 `None`
    /// 的正确反应是「什么都没被点中」，不是猜一个默认坐标。
    cursor_position: Option<(f32, f32)>,
    mouse_held: [bool; MOUSE_BUTTON_COUNT],
    mouse_just_pressed: [bool; MOUSE_BUTTON_COUNT],
    /// 本帧是否刚松开——鼠标点击的语义是「按下时落在控件上，松开时
    /// 仍落在同一个控件上」（见 `ll_ui::widget::button` 模块文档），
    /// 因此松开这一刻本身也需要能被查询到，不能只查 `held` 变成假。
    mouse_just_released: [bool; MOUSE_BUTTON_COUNT],
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    /// 建立全部松开的初始状态。
    pub const fn new() -> Self {
        InputState {
            held: [false; KEY_COUNT],
            just_pressed: [false; KEY_COUNT],
            repeat_next_at: [None; KEY_COUNT],
            repeated: [false; KEY_COUNT],
            cursor_position: None,
            mouse_held: [false; MOUSE_BUTTON_COUNT],
            mouse_just_pressed: [false; MOUSE_BUTTON_COUNT],
            mouse_just_released: [false; MOUSE_BUTTON_COUNT],
        }
    }

    /// 记录一次按下。
    ///
    /// 若该键本已按住，则不重新置起「刚按下」标志——这就是对操作系统
    /// 按键重复事件的去重。
    pub fn press(&mut self, key: GameKey) {
        let index = key.index();
        if !self.held[index] {
            self.just_pressed[index] = true;
        }
        self.held[index] = true;
    }

    /// 记录一次松开。
    ///
    /// **刻意不清除「刚按下」标志**：`just_pressed` 只能由 `end_frame` 清除。
    /// 「本帧内曾按下过」与「当前是否按住」是两个独立事实——若在此处一并
    /// 清除，同一帧内按下又松开的快速点击就会被静默丢弃。本项目主循环会在
    /// 玩家思考的空窗期推进离屏世界模拟，慢帧属预期常态，届时丢输入的概率
    /// 恰恰最高。
    ///
    /// **但顺带清空该键的重复计时基准 `repeat_next_at`。** 这与上面「不清
    /// `just_pressed`」看似矛盾，实则相反：`just_pressed` 记录的是「本帧内
    /// 发生过的事实」，清掉会丢失事实、丢输入；`repeat_next_at` 只是「下次
    /// 该在什么时刻重新触发」的**调度状态**，不是事实记录，清掉不丢失任何
    /// 输入。不清的后果是真实的竞态：winit 可能在同一轮事件泵里把
    /// `press → release → press` 都处理完才触发一次 `RedrawRequested`，
    /// 这三步会挤在两次 `begin_frame` 之间。`begin_frame` 只看调用时刻的
    /// `held` 快照，那一刻它只看到「一直按住」，于是保留旧的
    /// `repeat_next_at`——重按后第一次重复的等待时间就被错误压缩成
    /// `interval`（90ms）而不是应有的 `initial_delay`（350ms），快速二连
    /// 按会被误判为「从未松开过」，角色突然窜出去。在此处清空，重按后
    /// `begin_frame` 看到的必是「计时基准为空」，从而正确重新等满
    /// `initial_delay`。
    pub fn release(&mut self, key: GameKey) {
        self.held[key.index()] = false;
        self.repeat_next_at[key.index()] = None;
    }

    /// 触发一次「瞬时脉冲」：本帧视为该键刚激活一次，但不进入「按住」
    /// 状态。
    ///
    /// # 为什么滚轮不能复用 `press`
    ///
    /// 滚轮没有物理上的「按住」概念——每次滚动只是一个瞬间信号，不像
    /// 键盘那样有对应的松开事件（`crate::window` 的事件循环从
    /// `WindowEvent::MouseWheel` 只能拿到「滚动了一次」，从没有、也
    /// 永远不会有一个「滚轮松开了」的事件）。若复用 `press()`，该键
    /// 会被标记为 `held = true`，此后永远等不到 `release()` 来解除；
    /// [`Self::begin_frame`] 的自动重复机制还会把它当成「一直按住」
    /// 持续触发——这不是滚轮输入该有的语义，滚一下应当只触发一次。
    ///
    /// `pulse` 只置起 `just_pressed`，完全不触碰 `held`：`is_held`
    /// 对这个键恒返回 `false`，`begin_frame` 的自动重复分支因为看到
    /// `!self.held[index]` 而直接跳过，不会把这次脉冲错误地纳入连续
    /// 重复计时。
    pub fn pulse(&mut self, key: GameKey) {
        self.just_pressed[key.index()] = true;
    }

    /// 该键当前是否被按住。
    pub fn is_held(&self, key: GameKey) -> bool {
        self.held[key.index()]
    }

    /// 该键是否在本帧刚刚被按下。
    pub fn was_just_pressed(&self, key: GameKey) -> bool {
        self.just_pressed[key.index()]
    }

    /// 本帧该键是否应触发一次动作：首次按下，或自动重复触发。
    ///
    /// 游戏逻辑应当用这个查询，而不是 `was_just_pressed`——后者对参与
    /// 自动重复的键（方向键、等待键）会让长按只移动一格。
    pub fn was_activated(&self, key: GameKey) -> bool {
        self.was_just_pressed(key) || self.repeated[key.index()]
    }

    /// 推进到新的一帧，按 `now` 判定哪些按住的键应触发自动重复。
    ///
    /// 必须在每帧逻辑处理**之前**调用，与 `end_frame` 一头一尾夹住逻辑。
    ///
    /// 每个参与重复的键维护一个「下次重复时刻」：键未按住则清空计时；
    /// 键刚开始按住（计时为空）则记下 `now + initial_delay` 作为下次
    /// 触发时刻；键持续按住且已到达该时刻，则本帧标记一次重复触发，
    /// 并把下次触发时刻重设为 **`now + interval`**。
    ///
    /// # 为什么是 `now + interval` 而不是「上次时刻 + interval」
    ///
    /// 本项目主循环会在玩家思考的空窗期推进离屏世界模拟，慢帧是设计
    /// 预期内的常态——一帧可能耗时上百毫秒。若按「上次时刻 + interval」
    /// 累加，一次慢帧结束后下次触发时刻仍停留在早已过去的过去，接下来
    /// 连续好几帧都会发现「已到时刻」而连续补发重复，玩家会看到角色
    /// 突然窜出去好几格。改用 `now + interval` 后，无论这一帧实际跨越
    /// 了多久，下次触发时刻永远以*当前*时间为基准重新起算，每帧最多
    /// 触发一次重复，慢帧只是让下一次重复顺延，而不会欠账后一次性补发。
    pub fn begin_frame(&mut self, now: Instant, config: RepeatConfig) {
        for key in ALL_KEYS {
            if !key.is_repeatable() {
                continue;
            }
            let index = key.index();
            if !self.held[index] {
                self.repeat_next_at[index] = None;
                continue;
            }
            match self.repeat_next_at[index] {
                None => {
                    self.repeat_next_at[index] = Some(now + config.initial_delay);
                }
                Some(next_at) if now >= next_at => {
                    self.repeated[index] = true;
                    self.repeat_next_at[index] = Some(now + config.interval);
                }
                Some(_) => {}
            }
        }
    }

    /// 结束当前帧，清空「刚按下」与「本帧重复触发」标志。
    ///
    /// 必须在每帧逻辑处理**之后**调用。放在处理之前会让所有「刚按下」
    /// 判定永远为假。
    pub fn end_frame(&mut self) {
        self.just_pressed = [false; KEY_COUNT];
        self.repeated = [false; KEY_COUNT];
        self.mouse_just_pressed = [false; MOUSE_BUTTON_COUNT];
        self.mouse_just_released = [false; MOUSE_BUTTON_COUNT];
    }

    /// 清空全部按键状态（含鼠标按键，不含光标位置，见下方说明）。
    ///
    /// 窗口失去焦点时必须调用。操作系统只把按键事件送给焦点窗口，
    /// 玩家按住方向键时切走，对应的松开事件永远不会送达——不清空的话
    /// `held` 会永久为真，切回来后角色持续移动且没有任何按键能解除。
    ///
    /// 「刚按下」与重复计时状态一并清空：失焦瞬间尚未被消费的输入、
    /// 以及切走前积累的重复计时基准，切回来后都已经失去意义。
    ///
    /// # 鼠标按键同理，光标位置不在此列
    ///
    /// 失焦时按住的鼠标键与按住的键盘键是完全同一类 bug——玩家在按住
    /// 鼠标左键拖拽/长按一个控件时切走窗口，操作系统同样不会把松开
    /// 事件送到本窗口，`mouse_held` 不清空就会永久为真，切回来后
    /// 控件会读到一个「鼠标一直按着」的幽灵状态。
    ///
    /// **光标位置刻意不清空**：一个过期的坐标不会造成「按键永远解除
    /// 不了」这类结构性 bug（下一次 `WindowEvent::CursorMoved` 会带来
    /// 新坐标覆盖它），而清空成 `None` 反而更糟——多数平台失焦重新
    /// 聚焦时，如果鼠标物理位置没有移动，操作系统不保证会补发一次
    /// `CursorMoved`，这段时间内命中测试会把「光标其实还停在控件上」
    /// 误判成「什么都没指」，导致悬停高亮短暂消失又无预兆恢复。保留
    /// 最后已知位置是更安全的默认值。
    pub fn clear(&mut self) {
        self.held = [false; KEY_COUNT];
        self.just_pressed = [false; KEY_COUNT];
        self.repeat_next_at = [None; KEY_COUNT];
        self.repeated = [false; KEY_COUNT];
        self.mouse_held = [false; MOUSE_BUTTON_COUNT];
        self.mouse_just_pressed = [false; MOUSE_BUTTON_COUNT];
        self.mouse_just_released = [false; MOUSE_BUTTON_COUNT];
    }

    /// 记录一次光标移动——`position` 是窗口原生像素坐标系下的新位置，
    /// 见 [`Self::cursor_position`] 字段文档。
    pub fn set_cursor_position(&mut self, position: (f32, f32)) {
        self.cursor_position = Some(position);
    }

    /// 光标离开窗口——`WindowEvent::CursorLeft` 触发。命中测试见到
    /// `None` 后不会再判定任何控件被悬停/点中，这是「光标确实不在
    /// 窗口范围内」这个事实的正确反映；与 [`Self::clear`] 刻意保留
    /// 光标位置不同：那里是「窗口失焦但光标可能仍在窗口范围内、只是
    /// 暂时收不到事件」，这里是「光标已经确认离开了窗口范围」，两者是
    /// 不同的事实，不该用同一个处理方式。
    pub fn clear_cursor_position(&mut self) {
        self.cursor_position = None;
    }

    /// 光标当前位置，`None` 表示还没有过一次移动事件或已经离开窗口。
    pub fn cursor_position(&self) -> Option<(f32, f32)> {
        self.cursor_position
    }

    /// 记录一次鼠标按键按下，去重逻辑与 [`Self::press`] 完全一致。
    pub fn mouse_press(&mut self, button: MouseButton) {
        let index = button.index();
        if !self.mouse_held[index] {
            self.mouse_just_pressed[index] = true;
        }
        self.mouse_held[index] = true;
    }

    /// 记录一次鼠标按键松开——与 [`Self::release`] 不清「刚按下」标志
    /// 同一条纪律对称：这里也不清 `mouse_just_pressed`，「本帧内曾按下
    /// 过」与「当前是否按住」是两个独立事实。松开这一刻本身置起
    /// `mouse_just_released`，供点击判定查询（见字段文档）。
    pub fn mouse_release(&mut self, button: MouseButton) {
        let index = button.index();
        self.mouse_held[index] = false;
        self.mouse_just_released[index] = true;
    }

    /// 该鼠标按键当前是否被按住。
    pub fn is_mouse_held(&self, button: MouseButton) -> bool {
        self.mouse_held[button.index()]
    }

    /// 该鼠标按键是否在本帧刚刚被按下。
    pub fn was_mouse_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_just_pressed[button.index()]
    }

    /// 该鼠标按键是否在本帧刚刚被松开。
    pub fn was_mouse_just_released(&self, button: MouseButton) -> bool {
        self.mouse_just_released[button.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 按下后处于按住状态() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.press(GameKey::Confirm);

        // Assert
        assert!(input.is_held(GameKey::Confirm));
    }

    #[test]
    fn 刚按下的判定在帧结束后失效() {
        // 回合制里「刚按下」与「按住」必须区分：按住方向键应连续移动，
        // 但按住确认键不能反复触发同一个菜单项。
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        input.end_frame();

        // Assert
        assert!(!input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 帧结束后按住状态依然保留() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Right);

        // Act
        input.end_frame();

        // Assert
        assert!(input.is_held(GameKey::Right));
    }

    #[test]
    fn 松开后不再处于按住状态() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Left);

        // Act
        input.release(GameKey::Left);

        // Assert
        assert!(!input.is_held(GameKey::Left));
    }

    #[test]
    fn 重复按下不会重新触发刚按下判定() {
        // 操作系统的按键重复事件会连续发送按下，若不去重，长按确认键会
        // 把整个菜单一路点穿。
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);
        input.end_frame();

        // Act
        input.press(GameKey::Confirm);

        // Assert
        assert!(!input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 不同按键的状态互不干扰() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.press(GameKey::Up);

        // Assert
        assert!(!input.is_held(GameKey::Down));
    }

    #[test]
    fn 松开不会撤销本帧的刚按下判定() {
        // 同一帧内按下又松开的快速点击必须仍被游戏看到。
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        input.release(GameKey::Confirm);

        // Assert
        assert!(input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 清空后不再有任何按键处于按住状态() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Right);

        // Act
        input.clear();

        // Assert
        assert!(!input.is_held(GameKey::Right));
    }

    #[test]
    fn 清空后刚按下的判定一并失效() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        input.clear();

        // Assert
        assert!(!input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 每个动作键都已登记且下标互不重复() {
        // 这个测试的真正作用在编译期：下面的 match 是穷尽的，新增 GameKey
        // 变体而忘记同步 ALL_KEYS 与 KEY_COUNT 时，它会拒绝编译，
        // 而不是等到运行时数组越界。
        // Arrange
        let mut seen = [false; KEY_COUNT];

        // Act
        for key in ALL_KEYS {
            // 穷尽 match：新增变体时此处必须补齐，否则编译失败。
            let expected_index = match key {
                GameKey::Up => 0,
                GameKey::Down => 1,
                GameKey::Left => 2,
                GameKey::Right => 3,
                GameKey::Confirm => 4,
                GameKey::Cancel => 5,
                GameKey::Menu => 6,
                GameKey::Map => 7,
                GameKey::Wait => 8,
                GameKey::Screenshot => 9,
                GameKey::ZoomIn => 10,
                GameKey::ZoomOut => 11,
                GameKey::Inventory => 12,
                GameKey::Craft => 13,
                GameKey::PickUp => 14,
                GameKey::Drop => 15,
                GameKey::Equip => 16,
                GameKey::Use => 17,
                GameKey::Place => 18,
                GameKey::Interact => 19,
            };
            assert_eq!(key.index(), expected_index);
            seen[key.index()] = true;
        }

        // Assert
        assert!(seen.iter().all(|slot| *slot));
    }

    #[test]
    fn 每个动作键的显示名i18n键各不相同() {
        // 设置界面要能分别展示每个动作的名字；若两个动作共用同一个
        // i18n 键，翻译文件里就没法给它们分配不同的显示文本。
        // Arrange
        let keys: Vec<NamespacedId> = ALL_KEYS.iter().map(|key| key.display_name_key()).collect();

        // Act
        let unique_count = {
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            sorted.len()
        };

        // Assert
        assert_eq!(unique_count, keys.len());
    }

    #[test]
    fn 动作键的显示名走命名空间标识而不是裸字符串() {
        // 显示名键必须能解析成 NamespacedId（"命名空间:路径"），这就是
        // 「走 i18n」在类型层面的保证——裸的中文/英文字面量不具备这个
        // 形状，无法通过 NamespacedId::parse。
        // Arrange & Act
        let key = GameKey::Up.display_name_key();

        // Assert
        assert_eq!(key.namespace(), "lostland");
    }

    #[test]
    fn 刚按下的一帧本身就应被视为已激活() {
        // 首次按下无需等待任何自动重复计时，本帧就该触发一次动作。
        // Arrange
        let mut input = InputState::new();

        // Act
        input.press(GameKey::Up);

        // Assert
        assert!(input.was_activated(GameKey::Up));
    }

    #[test]
    fn 按住方向键在初始延迟内不触发重复() {
        // Arrange：按下后先经过一帧建立计时基准，再清掉「刚按下」标志，
        // 这样后续查询到的 was_activated 只反映自动重复本身。
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(GameKey::Up);
        input.begin_frame(pressed_at, config);
        input.end_frame();

        // Act：时间推进到刚好不足初始延迟
        input.begin_frame(
            pressed_at + config.initial_delay - Duration::from_millis(1),
            config,
        );

        // Assert
        assert!(!input.was_activated(GameKey::Up));
    }

    #[test]
    fn 超过初始延迟后触发一次重复() {
        // Arrange：按下后先经过一帧建立计时基准
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(GameKey::Up);
        input.begin_frame(pressed_at, config);
        input.end_frame();

        // Act：时间推进到恰好达到初始延迟
        input.begin_frame(pressed_at + config.initial_delay, config);

        // Assert
        assert!(input.was_activated(GameKey::Up));
    }

    #[test]
    fn 重复间隔未到时不再次触发() {
        // Arrange：按下、建立计时基准，并让第一次重复先行触发
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(GameKey::Up);
        input.begin_frame(pressed_at, config);
        input.end_frame();
        input.begin_frame(pressed_at + config.initial_delay, config);
        input.end_frame();

        // Act：时间只推进到不足下一个重复间隔
        let almost_next_repeat =
            pressed_at + config.initial_delay + config.interval - Duration::from_millis(1);
        input.begin_frame(almost_next_repeat, config);

        // Assert
        assert!(!input.was_activated(GameKey::Up));
    }

    #[test]
    fn 松开后重新按下要重新等满初始延迟() {
        // Arrange：按下、触发过一次重复，松开并让状态机看到这次松开，
        // 然后重新按下并建立新的计时基准
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(GameKey::Up);
        input.begin_frame(pressed_at, config);
        input.end_frame();
        input.begin_frame(pressed_at + config.initial_delay, config);
        input.end_frame();

        input.release(GameKey::Up);
        let released_seen_at = pressed_at + config.initial_delay + Duration::from_millis(1);
        input.begin_frame(released_seen_at, config);
        input.end_frame();

        input.press(GameKey::Up);
        let repressed_at = released_seen_at + Duration::from_millis(1);
        input.begin_frame(repressed_at, config);
        input.end_frame();

        // Act：只推进到不足新一轮的初始延迟——若重复间隔被错误地沿用
        // （而非重新等满初始延迟），这里就会被误判为已触发
        input.begin_frame(repressed_at + config.interval, config);

        // Assert
        assert!(!input.was_activated(GameKey::Up));
    }

    #[test]
    fn 同帧内松开再重按仍需等满初始延迟() {
        // winit 可能在同一轮事件泵里把 press → release → press 三件事
        // 全处理完才触发一次 RedrawRequested，这三步会挤在两次
        // begin_frame 之间——本测试故意不在松开与重按之间插入 begin_frame，
        // 复现这个真实的竞态窗口。若 release() 不清空 repeat_next_at，
        // begin_frame 只看到「一直按住」，会误沿用旧的计时基准，让重按后
        // 第一次重复被压缩成 interval 而不是应有的 initial_delay。
        // Arrange：按下并触发过一次重复，建立一个「旧的」repeat_next_at
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(GameKey::Up);
        input.begin_frame(pressed_at, config);
        input.end_frame();
        input.begin_frame(pressed_at + config.initial_delay, config);
        input.end_frame();

        // 松开与重按之间不调用 begin_frame，复现两者挤在同一轮事件泵、
        // 只触发一次 RedrawRequested 的真实时序
        input.release(GameKey::Up);
        input.press(GameKey::Up);
        let repress_frame_at = pressed_at + config.initial_delay + config.interval;
        input.begin_frame(repress_frame_at, config);
        // 本帧 was_activated 为真是合理的——它来自这次重按自身的
        // just_pressed，与自动重复的计时基准是否被污染无关，先清掉它
        // 不干扰下面才是本测试真正要观测的时刻。
        input.end_frame();

        // Act：时间只推进到超过 interval 但不足新一轮 initial_delay。
        // 若 repeat_next_at 被错误沿用旧值（未被 release 清空），此刻
        // 早已过了旧基准的下一次触发时刻，会被误判为已触发。
        input.begin_frame(repress_frame_at + config.interval, config);

        // Assert
        assert!(!input.was_activated(GameKey::Up));
    }

    #[test]
    fn 确认键按住不触发重复() {
        // 确认键不参与自动重复，属于设计明确排除的一类——长按它反复
        // 触发会把整个菜单一路点穿。
        // Arrange
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(GameKey::Confirm);
        input.begin_frame(pressed_at, config);
        input.end_frame();

        // Act：时间推进到远超初始延迟与多个重复间隔
        input.begin_frame(
            pressed_at + config.initial_delay + config.interval * 10,
            config,
        );

        // Assert
        assert!(!input.was_activated(GameKey::Confirm));
    }

    #[test]
    fn 截图键按住不触发重复() {
        // 截图是冻结视觉回归基准的一次性动作：长按若反复触发，会把同一
        // 个基准文件反复覆写，与长按确认键把菜单点穿是同一类问题。
        // Arrange
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(GameKey::Screenshot);
        input.begin_frame(pressed_at, config);
        input.end_frame();

        // Act：时间推进到远超初始延迟与多个重复间隔
        input.begin_frame(
            pressed_at + config.initial_delay + config.interval * 10,
            config,
        );

        // Assert
        assert!(!input.was_activated(GameKey::Screenshot));
    }

    #[test]
    fn 脉冲触发本帧刚按下判定() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.pulse(GameKey::ZoomIn);

        // Assert
        assert!(input.was_activated(GameKey::ZoomIn));
    }

    #[test]
    fn 脉冲不会让该键进入按住状态() {
        // 这是滚轮语义的核心：滚一下不该被当成「按住」，否则自动重复
        // 机制会在后续帧里持续触发同一个动作。
        // Arrange
        let mut input = InputState::new();

        // Act
        input.pulse(GameKey::ZoomIn);

        // Assert
        assert!(!input.is_held(GameKey::ZoomIn));
    }

    #[test]
    fn 脉冲触发的刚按下判定在帧结束后失效() {
        // Arrange
        let mut input = InputState::new();
        input.pulse(GameKey::ZoomIn);

        // Act
        input.end_frame();

        // Assert
        assert!(!input.was_activated(GameKey::ZoomIn));
    }

    #[test]
    fn 脉冲之后不会经自动重复机制在后续帧继续触发() {
        // 若脉冲错误地建立了重复计时基准，long 之后的 begin_frame 会
        // 让它像长按一样持续触发——这正是不能复用 press() 的理由。
        // Arrange
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pulsed_at = Instant::now();
        input.pulse(GameKey::ZoomIn);
        input.begin_frame(pulsed_at, config);
        input.end_frame();

        // Act：时间推进到远超初始延迟与多个重复间隔
        input.begin_frame(
            pulsed_at + config.initial_delay + config.interval * 10,
            config,
        );

        // Assert
        assert!(!input.was_activated(GameKey::ZoomIn));
    }

    #[test]
    fn 慢帧下一帧最多触发一次重复() {
        // 模拟一帧内跨越了远超一个重复间隔的慢帧：重复应当照常触发，
        // 但紧接着极短时间后的下一帧不应因为「欠账」而立刻再次触发。
        // Arrange
        let mut input = InputState::new();
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(GameKey::Up);
        input.begin_frame(pressed_at, config);
        input.end_frame();
        let slow_frame_now = pressed_at + config.initial_delay + config.interval * 10;
        input.begin_frame(slow_frame_now, config);
        input.end_frame();

        // Act：紧接着的下一帧只推进极短时间，远不足一个重复间隔
        input.begin_frame(slow_frame_now + Duration::from_millis(1), config);

        // Assert
        assert!(!input.was_activated(GameKey::Up));
    }

    #[test]
    fn 新建状态下没有光标位置() {
        // Arrange & Act
        let input = InputState::new();

        // Assert
        assert_eq!(input.cursor_position(), None);
    }

    #[test]
    fn 设置光标位置后能查询到同一个坐标() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.set_cursor_position((12.0, 34.0));

        // Assert
        assert_eq!(input.cursor_position(), Some((12.0, 34.0)));
    }

    #[test]
    fn 光标离开窗口后位置变为空值() {
        // Arrange
        let mut input = InputState::new();
        input.set_cursor_position((12.0, 34.0));

        // Act
        input.clear_cursor_position();

        // Assert
        assert_eq!(input.cursor_position(), None);
    }

    #[test]
    fn 鼠标按下后处于按住状态() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.mouse_press(MouseButton::Left);

        // Assert
        assert!(input.is_mouse_held(MouseButton::Left));
    }

    #[test]
    fn 鼠标按下的一帧本身就应被视为刚按下() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.mouse_press(MouseButton::Left);

        // Assert
        assert!(input.was_mouse_just_pressed(MouseButton::Left));
    }

    #[test]
    fn 鼠标刚按下的判定在帧结束后失效() {
        // Arrange
        let mut input = InputState::new();
        input.mouse_press(MouseButton::Left);

        // Act
        input.end_frame();

        // Assert
        assert!(!input.was_mouse_just_pressed(MouseButton::Left));
    }

    #[test]
    fn 鼠标松开后不再处于按住状态() {
        // Arrange
        let mut input = InputState::new();
        input.mouse_press(MouseButton::Left);

        // Act
        input.mouse_release(MouseButton::Left);

        // Assert
        assert!(!input.is_mouse_held(MouseButton::Left));
    }

    #[test]
    fn 鼠标松开的一帧能查询到刚松开() {
        // Arrange
        let mut input = InputState::new();
        input.mouse_press(MouseButton::Left);

        // Act
        input.mouse_release(MouseButton::Left);

        // Assert
        assert!(input.was_mouse_just_released(MouseButton::Left));
    }

    #[test]
    fn 鼠标刚松开的判定在帧结束后失效() {
        // Arrange
        let mut input = InputState::new();
        input.mouse_press(MouseButton::Left);
        input.mouse_release(MouseButton::Left);

        // Act
        input.end_frame();

        // Assert
        assert!(!input.was_mouse_just_released(MouseButton::Left));
    }

    #[test]
    fn 不同鼠标按键的状态互不干扰() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.mouse_press(MouseButton::Left);

        // Assert
        assert!(!input.is_mouse_held(MouseButton::Right));
    }

    #[test]
    fn 重复按下鼠标左键不会重新触发刚按下判定() {
        // 与键盘按键同一条去重纪律——防止某些平台可能出现的重复按下
        // 事件被误判成两次独立点击。
        // Arrange
        let mut input = InputState::new();
        input.mouse_press(MouseButton::Left);
        input.end_frame();

        // Act
        input.mouse_press(MouseButton::Left);

        // Assert
        assert!(!input.was_mouse_just_pressed(MouseButton::Left));
    }

    #[test]
    fn 清空后鼠标按键不再处于按住状态() {
        // 失焦时按住的鼠标键与按住的键盘键是同一类 bug，见 `clear`
        // 文档「鼠标按键同理」一节。
        // Arrange
        let mut input = InputState::new();
        input.mouse_press(MouseButton::Left);

        // Act
        input.clear();

        // Assert
        assert!(!input.is_mouse_held(MouseButton::Left));
    }

    #[test]
    fn 清空后光标位置保持不变() {
        // 见 `clear` 文档「光标位置刻意不清空」一节：过期坐标会被下一次
        // 移动事件覆盖，清成空值反而会让命中测试短暂误判悬停消失。
        // Arrange
        let mut input = InputState::new();
        input.set_cursor_position((5.0, 6.0));

        // Act
        input.clear();

        // Assert
        assert_eq!(input.cursor_position(), Some((5.0, 6.0)));
    }

    #[test]
    fn 清空后鼠标刚按下与刚松开的判定一并失效() {
        // Arrange
        let mut input = InputState::new();
        input.mouse_press(MouseButton::Left);

        // Act
        input.clear();

        // Assert
        assert!(!input.was_mouse_just_pressed(MouseButton::Left));
    }
}
