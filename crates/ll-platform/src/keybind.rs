//! 键位绑定表：把「物理按键」与「抽象动作」之间的映射变成数据，而不是
//! 写死在事件循环里的 `match` 分支。
//!
//! # 要解决的问题
//!
//! [`crate::window::AppHandler`] 与 [`crate::input::GameKey`] 已经把游戏
//! 逻辑与物理按键分开了——上层只看得到 `GameKey::Up`，看不到「玩家按了
//! W」。但分界的另一侧——「W 为什么映射到 `GameKey::Up`」——此前是
//! `crate::window` 里一段硬编码的 `match`。规格 §11 已经把「按键重绑定」
//! 列进设置界面的必备项（P7），若不趁早把这张映射表抽成数据，等到做
//! 设置界面时就要去 `match` 分支里现场重构，且写死的按键只会越积越多。
//!
//! # 为什么默认绑定是数据，不是 `match`
//!
//! 按 [ADR 0016](../../../knowledge/decisions/0016-mod-performance-tiers-by-declaration.md)
//! 的分档：一份固定的「动作 → 按键」映射是**第一档静态声明**——声明期
//! 就能完全确定，不依赖任何运行时输入。第一档的做法是「注册期物化进
//! Rust 表，运行期查表」，而不是写成分支判断——`match` 本身就是一种
//! 隐式的「代码即数据」，把它显式化成 [`DEFAULT_BINDINGS`] 这张表后，
//! 未来从配置文件加载自定义绑定时，加载出来的数据与内置默认值走的是
//! 完全相同的构造路径（[`KeyBindings::from_bindings`]），不需要再维护
//! 一套平行逻辑。
//!
//! # 为什么冲突在注册时拒绝，而不是允许后报告
//!
//! 同一个物理键（含修饰键）在同一个 [`InputContext`] 下绑给两个不同的
//! 动作是一个真实的逻辑错误：按下这个键时，[`KeyBindings::resolve`]
//! 到底该返回哪个动作没有唯一答案。本项目的一贯原则是「在系统边界处
//! 验证、快速失败」（见 `coding-style.md` 输入校验一节），[`KeyBindings`]
//! 因此选择**注册时拒绝**（[`KeyBindings::try_bind`] 返回
//! `Result`）而不是「允许注册、之后再报告」：
//!
//! - 允许后报告意味着 `KeyBindings` 在被查询之前可能长期处于一个内部
//!   自相矛盾的状态，`resolve` 要么隐式地「先到先得」、要么每次查询都
//!   要重新扫描冲突——这类隐藏的歧义正是最难排查的一类缺陷。
//! - 注册时拒绝把校验成本一次性摊在「构造/修改绑定表」这个低频操作上
//!   （见 [`KeyBindings::try_bind`]），而不是摊在每次按键事件都会触发的
//!   [`KeyBindings::resolve`] 上。
//! - 未来设置界面若想允许「拖动过程中暂时冲突」这类交互，那是 UI 层
//!   自己维护一份草稿状态、确认时再整体提交给 `try_bind`/`from_bindings`
//!   的责任，不需要数据层为此放宽不变式。
//!
//! # 上下文：为什么冲突判定要按 `(键, 修饰键, 上下文)` 而不是只按键
//!
//! 有些「冲突」其实是合理的重叠：Esc 在菜单里是返回、在游戏内是暂停，
//! 同一个物理键在不同场景做不同事完全合理，不该被判定为绑定冲突。
//! [`InputContext`] 就是为此留的接缝——冲突检测按
//! `(物理键, 修饰键, 上下文)` 三元组判重，不同上下文下同一个键可以绑给
//! 不同动作。**目前只有 [`InputContext::Gameplay`] 一个取值**：设置
//! 界面与菜单状态机尚未建成（P7），现在就设计一整套上下文切换是
//! `coding-style.md` 会警告的 speculative generality——本模块只保证
//! 「新增一个上下文只需要给这个枚举加一个变体」，不提前实现菜单本身。
//!
//! # 持久化：进配置，不进存档
//!
//! 按键绑定是用户偏好，不是世界状态——绝不能进
//! `ll_world::state::WorldState`、不能参与 `hash()`、不能影响确定性
//! 重放。[`KeyBindings`] 因此完全独立于 `ll-world`/`ll-sim`（本 crate
//! 从未、也不应该反向依赖它们）。
//!
//! **本项目目前没有配置文件系统**（规格提过 JSON 配置但未落地），本模块
//! 因此只做两件事：让绑定表能从数据构造（[`KeyBindings::from_bindings`]）、
//! 能完整序列化往返（`Serialize`/`Deserialize`，见下方 `serde` 一节）。
//! **「从哪个文件、用什么格式加载」留给未来的配置系统决定**——预期的
//! 接缝是：配置系统解析出一份 `Vec<KeyBinding>`（或直接是序列化后的
//! `KeyBindings`）后，调用 [`KeyBindings::from_bindings`] 或依赖
//! `serde` 的 `Deserialize` 实现完成校验，本模块不需要再改一行。
//!
//! # serde：为什么要经过 `TryFrom` 中转而不是直接派生
//!
//! 按 [ADR 0011](../../../knowledge/decisions/0011-serde-try-from-bypasses-validating-constructors.md)：
//! `KeyBindings` 靠私有字段 + 校验构造函数（[`KeyBindings::try_bind`]）
//! 保证「无冲突」这条不变式，若直接对私有字段派生 `Deserialize`，
//! serde 会绕开校验直接把数据怼进私有字段——被篡改或手写的配置文件
//! 完全可能包含冲突绑定。因此反序列化改经 [`KeyBindingsRepr`] 中转，
//! 用 [`KeyBindings::from_bindings`] 校验后再落地。

use crate::input::GameKey;
use serde::{Deserialize, Serialize};
use std::fmt;
use winit::event::MouseScrollDelta;
use winit::keyboard::KeyCode;

/// 输入上下文：冲突检测的判重维度之一。
///
/// 同一个物理键在不同上下文下绑给不同动作是合理的重叠，不构成冲突
/// （例如 Esc 在菜单里是返回、在游戏内是暂停）——见模块文档「上下文」
/// 一节。
///
/// # `Menu`：UI 交互层批次新增
///
/// 覆盖游戏画面的任意模态 UI（背包首页、物品详情、确认框、未来的
/// 设置界面/暂停菜单）全部共用这一个变体——哪一层具体在响应由 UI 层
/// 自己的模式栈（`ll_ui::widget::ui_mode::UiModeStack`，本 crate 不
/// 依赖 `ll-ui`，此处只是文字引用，不是可解析的文档内链，与
/// `crate::keybind` 模块文档既有的同类写法一致）决定，不是
/// `InputContext` 的职责。完整论证见
/// `knowledge/design/action-capability-and-input-context.md` 2.1/2.2
/// 节：不给每一层嵌套菜单各开一个变体（那是过度设计，嵌套深度是
/// 运行时可变的），也不让 `InputContext` 自己变成一个栈（那会把
/// `KeyBindings::resolve` 从纯函数变成有状态查询）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputContext {
    /// 游戏内主流程——角色移动、攻击等直接作用于世界的输入。
    Gameplay,
    /// 任意模态 UI 覆盖游戏画面时的输入上下文，见本类型文档「`Menu`」
    /// 一节。
    Menu,
}

/// 一次按键事件里参与判定的修饰键状态。
///
/// 独立于 winit 自身的 `ModifiersState`：本类型只保留绑定表关心的三个
/// 布尔量，且需要能在 `const` 上下文中构造（[`DEFAULT_BINDINGS`] 是一张
/// 编译期常量表），winit 的位标志类型不提供这一点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Modifiers {
    /// Shift 是否按住。
    pub shift: bool,
    /// Ctrl 是否按住。
    pub ctrl: bool,
    /// Alt 是否按住。
    pub alt: bool,
}

impl Modifiers {
    /// 不含任何修饰键——绝大多数默认绑定都用这个。
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };
}

impl From<winit::keyboard::ModifiersState> for Modifiers {
    /// 从 winit 的位标志状态换算成本模块的 `Modifiers`。
    ///
    /// 只在 [`crate::window`] 的事件循环里调用——本类型存在的意义正是
    /// 让绑定表的其余部分不需要认识 winit 的具体表示。
    fn from(state: winit::keyboard::ModifiersState) -> Self {
        Modifiers {
            shift: state.shift_key(),
            ctrl: state.control_key(),
            alt: state.alt_key(),
        }
    }
}

/// 滚轮滚动的离散方向。
///
/// # 为什么滚轮是独立于 [`KeyBinding`] 的一套抽象，而不是塞进同一个
/// 类型
///
/// 滚轮与物理按键是两种本质不同的输入：按键有「按下/抬起」两个持续
/// 状态（[`crate::input::InputState::press`]/`release`），滚轮只有
/// 瞬时的滚动事件，没有对应的「松开」——winit 从不为滚轮报告一个
/// 「停止滚动」事件。若勉强把滚轮塞进 [`KeyBinding`]（例如伪造一个
/// `KeyCode` 变体表示滚轮方向），`(key, modifiers, context)` 这套判重
/// 元组会需要一个从不存在的「滚轮的修饰键状态」概念，且滚轮事件与
/// [`crate::window`] 事件循环里 `KeyboardInput` 的按下/抬起分支语义
/// 完全对不上。因此滚轮改用一个独立的判重维度
/// `(WheelDirection, InputContext)`，融入同一套 [`KeyBindings`]、
/// 复用同一套注册期冲突检测与 [`GameKey::display_name_key`] 显示名
/// 机制（见 [`KeyBindings::try_bind_wheel`]/[`KeyBindings::resolve_wheel`]），
/// 但不与 [`KeyBinding`] 共享同一张表——两套判重维度混在一张表里，
/// 冲突检测就需要区分「这条记录是按键还是滚轮」，增加不必要的分支，
/// 拆成两张表反而更简单。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WheelDirection {
    /// 滚轮远离操作者滚动的一格——约定俗成的「放大」方向（多数地图类
    /// 应用的默认手感）。
    Away,
    /// 滚轮朝操作者滚动的一格——约定俗成的「缩小」方向。
    Toward,
}

impl WheelDirection {
    /// 从 winit 的滚轮增量换算成方向；增量恰为零（winit 不保证不会
    /// 发生，但正常硬件不该产出零增量的滚动事件）时返回 `None`，
    /// 调用方应当忽略这类事件而不是猜一个方向。
    ///
    /// 只看竖直分量——多数鼠标滚轮只有这一个轴，触控板的水平滚动不映射
    /// 到任何方向。`LineDelta`（多数鼠标每次滚动上报的单位）与
    /// `PixelDelta`（触控板）统一按竖直分量的**符号**判定，不使用
    /// 幅度：这条抽象只关心「往哪个方向滚了一下」，不关心滚了多快，
    /// 一次 winit 事件即产出至多一次方向判定，不做像素级累积去抖动
    /// （`crate::window` 的事件循环因此对每个滚轮事件调用
    /// `InputState::pulse` 恰好一次，见其文档）。
    ///
    /// 竖直分量为负（内容应该向上移动，多数平台对应滚轮往前/远离操作
    /// 者滚动）判定为 [`WheelDirection::Away`]；为正判定为
    /// [`WheelDirection::Toward`]。这条符号约定是本项目自行选定的，
    /// winit 文档本身不保证跨平台/跨设备的物理方向一致，本函数只保证
    /// 「同一次事件恒定换算成同一个方向」，不保证「远离操作者」在每
    /// 种鼠标/操作系统设置下都精确对应物理上的「向前推」。
    pub fn from_scroll_delta(delta: MouseScrollDelta) -> Option<WheelDirection> {
        let vertical = match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(position) => position.y as f32,
        };
        if vertical < 0.0 {
            Some(WheelDirection::Away)
        } else if vertical > 0.0 {
            Some(WheelDirection::Toward)
        } else {
            None
        }
    }
}

/// 一条滚轮绑定：某个上下文下，滚轮往某个方向滚动一格触发某个抽象
/// 动作。与 [`KeyBinding`] 是同一套「数据而非 match」思路在滚轮这类
/// 输入上的应用，见 [`WheelDirection`] 文档「为什么滚轮是独立的一套
/// 抽象」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WheelBinding {
    /// 触发这条绑定的滚动方向。
    pub direction: WheelDirection,
    /// 这条绑定生效的输入上下文。
    pub context: InputContext,
    /// 触发后产出的抽象动作。
    pub action: GameKey,
}

/// 注册滚轮绑定时发现的冲突：同一个滚动方向在同一个上下文下已经绑给
/// 了另一个动作。与 [`KeyBindConflict`] 是同一套「注册时拒绝」纪律
/// （见模块文档「为什么冲突在注册时拒绝」一节）应用在滚轮判重维度
/// `(方向, 上下文)` 上的对应物。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelBindConflict {
    /// 发生冲突的滚动方向。
    pub direction: WheelDirection,
    /// 发生冲突的上下文。
    pub context: InputContext,
    /// 已经占用这个方向的动作。
    pub existing_action: GameKey,
    /// 试图再绑定到同一方向、因而被拒绝的动作。
    pub attempted_action: GameKey,
}

impl fmt::Display for WheelBindConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "滚轮方向 {:?}（上下文 {:?}）已绑定给 {:?}，不能再绑给 {:?}",
            self.direction, self.context, self.existing_action, self.attempted_action
        )
    }
}

impl std::error::Error for WheelBindConflict {}

/// 反序列化整张绑定表时，键位冲突与滚轮冲突可能各自独立发生，
/// [`KeyBindings`] 的 `TryFrom<KeyBindingsRepr>` 实现需要一个统一的
/// 错误类型才能用 `?` 依次校验两张表——[`KeyBindings::try_bind`] 与
/// [`KeyBindings::try_bind_wheel`] 各自的公开签名仍然保留自己专属的
/// 错误类型（[`KeyBindConflict`]/[`WheelBindConflict`]），不为了这一处
/// 内部拼接就改动这两个已经被外部调用的公开方法的签名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindConflict {
    /// 一条 [`KeyBinding`] 与已注册的绑定冲突。
    Key(KeyBindConflict),
    /// 一条 [`WheelBinding`] 与已注册的绑定冲突。
    Wheel(WheelBindConflict),
}

impl fmt::Display for BindConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindConflict::Key(conflict) => write!(f, "{conflict}"),
            BindConflict::Wheel(conflict) => write!(f, "{conflict}"),
        }
    }
}

impl std::error::Error for BindConflict {}

impl From<KeyBindConflict> for BindConflict {
    fn from(conflict: KeyBindConflict) -> Self {
        BindConflict::Key(conflict)
    }
}

impl From<WheelBindConflict> for BindConflict {
    fn from(conflict: WheelBindConflict) -> Self {
        BindConflict::Wheel(conflict)
    }
}

/// 一条键位绑定：某个上下文下，某个物理键（含修饰键）触发某个抽象动作。
///
/// 「可多绑」体现在 [`KeyBindings`] 允许同一个 `action` 出现在多条
/// `KeyBinding` 里（例如 `GameKey::Up` 同时绑 `ArrowUp` 与 `KeyW`），
/// 冲突检测只拒绝「同一个键绑给不同动作」，见模块文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// 触发这条绑定的物理键。
    pub key: KeyCode,
    /// 触发这条绑定所需的修饰键状态。
    pub modifiers: Modifiers,
    /// 这条绑定生效的输入上下文。
    pub context: InputContext,
    /// 触发后产出的抽象动作。
    pub action: GameKey,
}

impl KeyBinding {
    /// 便捷构造：无修饰键、[`InputContext::Gameplay`] 上下文下的绑定。
    ///
    /// [`DEFAULT_BINDINGS`] 里的每一条都符合这个形状，写全部字段名会
    /// 让那张表淹没在样板里，故提供这个构造函数保持它易读。
    const fn gameplay(key: KeyCode, action: GameKey) -> KeyBinding {
        KeyBinding {
            key,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action,
        }
    }
}

/// 注册键位绑定时发现的冲突：同一个物理键（含修饰键）在同一个上下文下
/// 已经绑给了另一个动作。
///
/// 为什么在注册时就拒绝而不是允许后报告，见模块文档「为什么冲突在
/// 注册时拒绝」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBindConflict {
    /// 发生冲突的物理键。
    pub key: KeyCode,
    /// 发生冲突的修饰键状态。
    pub modifiers: Modifiers,
    /// 发生冲突的上下文。
    pub context: InputContext,
    /// 已经占用这个键位的动作。
    pub existing_action: GameKey,
    /// 试图再绑定到同一键位、因而被拒绝的动作。
    pub attempted_action: GameKey,
}

impl fmt::Display for KeyBindConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "键位 {:?}（修饰键 {:?}，上下文 {:?}）已绑定给 {:?}，不能再绑给 {:?}",
            self.key, self.modifiers, self.context, self.existing_action, self.attempted_action
        )
    }
}

impl std::error::Error for KeyBindConflict {}

/// 键位绑定表：从物理按键解析出抽象动作，支持一个动作多条绑定。
///
/// **不变式**：表内任意两条绑定，只要 `(key, modifiers, context)` 三元组
/// 相同，`action` 就必须相同——这就是「无冲突」。唯一能修改这张表的
/// 入口（[`KeyBindings::try_bind`]、[`KeyBindings::from_bindings`]、
/// `Deserialize`）都会维持这条不变式，见模块文档 serde 一节。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "KeyBindingsRepr")]
pub struct KeyBindings {
    bindings: Vec<KeyBinding>,
    /// 滚轮绑定，判重维度与 `bindings` 完全独立（`(方向, 上下文)` 而
    /// 非 `(键, 修饰键, 上下文)`），见 [`WheelDirection`] 文档。
    wheel_bindings: Vec<WheelBinding>,
}

/// 默认键位表：第一档静态声明（见模块文档），保留传统 Roguelike 的
/// 方向键与 WASD 双绑——手部姿势偏好差异很大，强制只用一种会劝退一
/// 部分玩家（沿用自 `window.rs` 此前 `map_physical_key` 的既有布局）。
///
/// 截图（冻结视觉回归基准）放在 F2 而非字母键：功能键区不与移动、确认
/// 这些高频键争抢位置，误触覆写基准文件的代价也就不会发生。
const DEFAULT_BINDINGS: &[KeyBinding] = &[
    KeyBinding::gameplay(KeyCode::ArrowUp, GameKey::Up),
    KeyBinding::gameplay(KeyCode::KeyW, GameKey::Up),
    KeyBinding::gameplay(KeyCode::ArrowDown, GameKey::Down),
    KeyBinding::gameplay(KeyCode::KeyS, GameKey::Down),
    KeyBinding::gameplay(KeyCode::ArrowLeft, GameKey::Left),
    KeyBinding::gameplay(KeyCode::KeyA, GameKey::Left),
    KeyBinding::gameplay(KeyCode::ArrowRight, GameKey::Right),
    KeyBinding::gameplay(KeyCode::KeyD, GameKey::Right),
    KeyBinding::gameplay(KeyCode::Enter, GameKey::Confirm),
    KeyBinding::gameplay(KeyCode::Space, GameKey::Confirm),
    KeyBinding::gameplay(KeyCode::Escape, GameKey::Cancel),
    KeyBinding::gameplay(KeyCode::Tab, GameKey::Menu),
    KeyBinding::gameplay(KeyCode::KeyM, GameKey::Map),
    KeyBinding::gameplay(KeyCode::Period, GameKey::Wait),
    KeyBinding::gameplay(KeyCode::F2, GameKey::Screenshot),
    // 缩放：等号键在多数键盘上与 `+` 同键位,不需要按住 Shift,更顺手；
    // 减号键则是天然的一键对应,两者与滚轮缩放（DEFAULT_WHEEL_BINDINGS）
    // 触发同一对抽象动作,这就是「同一个抽象动作可由滚轮或按键触发」
    // 的落点。
    KeyBinding::gameplay(KeyCode::Equal, GameKey::ZoomIn),
    KeyBinding::gameplay(KeyCode::Minus, GameKey::ZoomOut),
];

/// `InputContext::Menu` 下的默认键位表：与 [`DEFAULT_BINDINGS`] 用
/// **同一组物理键**映射到方向/确认/取消四个动作——设计文档 2.2 节
/// 「共享同一份物理键映射」的直接落点：背包首页、物品详情、确认框等
/// 全部模态 UI 共用这一份表，差异只在 UI 层自己怎么"读" `GameKey`
/// （见 [`InputContext::Menu`] 文档），不是这张表要处理的事。
///
/// 与 `DEFAULT_BINDINGS` 的对应关系不是巧合：菜单导航复用玩家已经
/// 熟悉的方向键手感（方向键=导航、Enter/Space=确认、Esc=返回），不
/// 需要玩家为菜单单独学一套按键。`(键, 修饰键, 上下文)` 三元组判重
/// 保证这张表与 `DEFAULT_BINDINGS` 互不冲突——两者的 `context` 字段
/// 不同，即便物理键完全相同也不会被 [`KeyBindings::try_bind`] 拒绝。
const DEFAULT_MENU_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: KeyCode::ArrowUp,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Up,
    },
    KeyBinding {
        key: KeyCode::KeyW,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Up,
    },
    KeyBinding {
        key: KeyCode::ArrowDown,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Down,
    },
    KeyBinding {
        key: KeyCode::KeyS,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Down,
    },
    KeyBinding {
        key: KeyCode::ArrowLeft,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Left,
    },
    KeyBinding {
        key: KeyCode::KeyA,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Left,
    },
    KeyBinding {
        key: KeyCode::ArrowRight,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Right,
    },
    KeyBinding {
        key: KeyCode::KeyD,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Right,
    },
    KeyBinding {
        key: KeyCode::Enter,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Confirm,
    },
    KeyBinding {
        key: KeyCode::Space,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Confirm,
    },
    KeyBinding {
        key: KeyCode::Escape,
        modifiers: Modifiers::NONE,
        context: InputContext::Menu,
        action: GameKey::Cancel,
    },
];

/// 默认滚轮绑定：与 [`DEFAULT_BINDINGS`] 里的缩放键位绑给同一对抽象
/// 动作——`GameKey::ZoomIn`/`ZoomOut` 因此能同时由滚轮与按键触发，
/// 上层游戏逻辑（`ll-game` 的 `Demo::advance`）只需要查询
/// `InputState::was_activated`，不需要关心这次触发来自哪种物理输入。
const DEFAULT_WHEEL_BINDINGS: &[WheelBinding] = &[
    WheelBinding {
        direction: WheelDirection::Away,
        context: InputContext::Gameplay,
        action: GameKey::ZoomIn,
    },
    WheelBinding {
        direction: WheelDirection::Toward,
        context: InputContext::Gameplay,
        action: GameKey::ZoomOut,
    },
];

impl KeyBindings {
    /// 内置默认绑定表，从 [`DEFAULT_BINDINGS`]/[`DEFAULT_WHEEL_BINDINGS`]
    /// 这两张静态数据构造。
    ///
    /// `expect` 而不是返回 `Result`：两张表都是编译期常量，若内部自相
    /// 冲突，那是表本身写错了，应当在开发期就地修正，不该把「内置
    /// 默认值可能非法」这种可能性泄漏给调用方处理。
    pub fn default_bindings() -> KeyBindings {
        let mut table = KeyBindings::from_bindings(
            DEFAULT_BINDINGS
                .iter()
                .copied()
                .chain(DEFAULT_MENU_BINDINGS.iter().copied()),
        )
        .expect("DEFAULT_BINDINGS/DEFAULT_MENU_BINDINGS 是内置常量表，不应自相冲突");
        for binding in DEFAULT_WHEEL_BINDINGS.iter().copied() {
            table
                .try_bind_wheel(binding)
                .expect("DEFAULT_WHEEL_BINDINGS 是内置常量表，不应自相冲突");
        }
        table
    }

    /// 从一组绑定逐条校验后构造绑定表，任意一条与已注册的绑定冲突就
    /// 整体失败。只接受按键绑定——滚轮绑定走独立的
    /// [`Self::try_bind_wheel`]，理由见 [`WheelDirection`] 文档。
    pub fn from_bindings(
        bindings: impl IntoIterator<Item = KeyBinding>,
    ) -> Result<KeyBindings, KeyBindConflict> {
        let mut table = KeyBindings {
            bindings: Vec::new(),
            wheel_bindings: Vec::new(),
        };
        for binding in bindings {
            table.try_bind(binding)?;
        }
        Ok(table)
    }

    /// 追加一条绑定；若与表内已有绑定冲突（同一 `(key, modifiers,
    /// context)` 绑给了不同的 `action`）则拒绝，且不修改表。
    ///
    /// 同一个 `action` 多次出现（多绑）不算冲突，这正是「一个动作可以
    /// 绑多个键」的落点。
    pub fn try_bind(&mut self, binding: KeyBinding) -> Result<(), KeyBindConflict> {
        if let Some(existing) = self.bindings.iter().find(|existing| {
            existing.key == binding.key
                && existing.modifiers == binding.modifiers
                && existing.context == binding.context
                && existing.action != binding.action
        }) {
            return Err(KeyBindConflict {
                key: binding.key,
                modifiers: binding.modifiers,
                context: binding.context,
                existing_action: existing.action,
                attempted_action: binding.action,
            });
        }
        self.bindings.push(binding);
        Ok(())
    }

    /// 解析一次物理按键事件：给定当前按住的修饰键与所处上下文，查出
    /// 对应的抽象动作。
    ///
    /// 这是输入层与游戏逻辑之间真正的分界——[`crate::window`] 的事件
    /// 循环只调用这一个方法，游戏逻辑与本表的存在无关（它们只认
    /// [`GameKey`]）。
    pub fn resolve(
        &self,
        key: KeyCode,
        modifiers: Modifiers,
        context: InputContext,
    ) -> Option<GameKey> {
        self.bindings
            .iter()
            .find(|binding| {
                binding.key == key && binding.modifiers == modifiers && binding.context == context
            })
            .map(|binding| binding.action)
    }

    /// 追加一条滚轮绑定；若与表内已有滚轮绑定冲突（同一
    /// `(direction, context)` 绑给了不同的 `action`）则拒绝，且不修改
    /// 表。与 [`Self::try_bind`] 是完全平行的两套逻辑，只是判重维度
    /// 换成了滚轮的 `(方向, 上下文)`，理由见 [`WheelDirection`] 文档。
    pub fn try_bind_wheel(&mut self, binding: WheelBinding) -> Result<(), WheelBindConflict> {
        if let Some(existing) = self.wheel_bindings.iter().find(|existing| {
            existing.direction == binding.direction
                && existing.context == binding.context
                && existing.action != binding.action
        }) {
            return Err(WheelBindConflict {
                direction: binding.direction,
                context: binding.context,
                existing_action: existing.action,
                attempted_action: binding.action,
            });
        }
        self.wheel_bindings.push(binding);
        Ok(())
    }

    /// 解析一次滚轮事件：给定滚动方向与所处上下文，查出对应的抽象
    /// 动作。与 [`Self::resolve`] 是完全平行的两套查询，`crate::window`
    /// 的事件循环处理 `WindowEvent::MouseWheel` 时调用这一个方法，
    /// 与处理 `WindowEvent::KeyboardInput` 时调用 `resolve` 是同一层级
    /// 的两个独立入口——两者共用同一套 [`GameKey`] 输出空间，上层
    /// （`ll-game`）看到的只是一次 `GameKey::ZoomIn` 被激活，分不出、
    /// 也不需要分出它来自按键还是滚轮。
    pub fn resolve_wheel(
        &self,
        direction: WheelDirection,
        context: InputContext,
    ) -> Option<GameKey> {
        self.wheel_bindings
            .iter()
            .find(|binding| binding.direction == direction && binding.context == context)
            .map(|binding| binding.action)
    }

    /// 列出全部绑定，供设置界面展示、导出配置等只读场景使用。
    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    /// 列出全部滚轮绑定，理由与 [`Self::bindings`] 相同。
    pub fn wheel_bindings(&self) -> &[WheelBinding] {
        &self.wheel_bindings
    }

    /// 列出绑给某个动作的全部按键——设置界面展示「这个动作当前绑了
    /// 哪些键」会用到。
    pub fn bindings_for(&self, action: GameKey) -> impl Iterator<Item = &KeyBinding> {
        self.bindings
            .iter()
            .filter(move |binding| binding.action == action)
    }
}

/// [`KeyBindings`] 反序列化的中转表示：字段公开、不带任何不变式。
///
/// 按 [ADR 0011](../../../knowledge/decisions/0011-serde-try-from-bypasses-validating-constructors.md)
/// 的模式，反序列化先落地成这个无校验的结构，再经
/// [`TryFrom`] 委托给 [`KeyBindings::from_bindings`]，使冲突校验对
/// 反序列化路径同样生效。
#[derive(Deserialize)]
struct KeyBindingsRepr {
    bindings: Vec<KeyBinding>,
    /// `#[serde(default)]`：旧版本（本字段引入之前）写出的配置文件不含
    /// 这个键，读回时应当退回空列表而不是解析失败——与 `GameConfig`
    /// 一贯的「新增字段用 `#[serde(default = ...)]` 兜底旧配置文件」
    /// 模式一致（见 `crate::config` 模块文档）。
    #[serde(default)]
    wheel_bindings: Vec<WheelBinding>,
}

impl TryFrom<KeyBindingsRepr> for KeyBindings {
    type Error = BindConflict;

    fn try_from(raw: KeyBindingsRepr) -> Result<Self, Self::Error> {
        let mut table = KeyBindings::from_bindings(raw.bindings)?;
        for binding in raw.wheel_bindings {
            table.try_bind_wheel(binding)?;
        }
        Ok(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认绑定表能解析方向键() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::Up));
    }

    #[test]
    fn 默认绑定表里字母键位也映射到同一方向() {
        // 传统 Roguelike 玩家习惯 WASD，方向键与字母键应解析到同一动作。
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::KeyW, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::Up));
    }

    #[test]
    fn 默认绑定表能解析截图键() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::F2, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::Screenshot));
    }

    #[test]
    fn 默认绑定表能解析取消键() {
        // demo 与后续的「退出游戏」菜单都依赖这条映射，它曾经是全项目
        // 唯一映射却无人消费的死映射（见 `window.rs` 此前的同名测试）。
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::Escape, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::Cancel));
    }

    #[test]
    fn 未绑定的键解析为空值() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::F13, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, None);
    }

    #[test]
    fn 不同上下文下未注册的组合解析为空值() {
        // 目前只有一个上下文取值，这里验证「上下文不匹配」本身确实会
        // 让 resolve 查不到——防止未来新增上下文变体时，判重逻辑悄悄
        // 退化成只比较键位而忽略上下文字段。
        // Arrange
        let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
        table
            .try_bind(KeyBinding {
                key: KeyCode::KeyQ,
                modifiers: Modifiers::NONE,
                context: InputContext::Gameplay,
                action: GameKey::Menu,
            })
            .expect("首次绑定不冲突");

        // Act：同一个键，但没有为之注册过的假想上下文用不同修饰键代替
        // 以制造一个确定查不到的组合。
        let action = table.resolve(
            KeyCode::KeyQ,
            Modifiers {
                shift: true,
                ctrl: false,
                alt: false,
            },
            InputContext::Gameplay,
        );

        // Assert
        assert_eq!(action, None);
    }

    #[test]
    fn 同一个键绑给两个不同动作时注册被拒绝() {
        // Arrange
        let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
        table
            .try_bind(KeyBinding {
                key: KeyCode::KeyQ,
                modifiers: Modifiers::NONE,
                context: InputContext::Gameplay,
                action: GameKey::Menu,
            })
            .expect("首次绑定不冲突");

        // Act
        let result = table.try_bind(KeyBinding {
            key: KeyCode::KeyQ,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action: GameKey::Map,
        });

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 冲突被拒绝后表内容不变() {
        // Arrange
        let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
        table
            .try_bind(KeyBinding {
                key: KeyCode::KeyQ,
                modifiers: Modifiers::NONE,
                context: InputContext::Gameplay,
                action: GameKey::Menu,
            })
            .expect("首次绑定不冲突");

        // Act
        let _ = table.try_bind(KeyBinding {
            key: KeyCode::KeyQ,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action: GameKey::Map,
        });

        // Assert
        assert_eq!(table.bindings().len(), 1);
    }

    #[test]
    fn 同一个键在不同修饰键下绑给不同动作不算冲突() {
        // 修饰键是判重维度之一：Ctrl+S 与 S 完全可以各自独立绑定。
        // Arrange
        let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
        table
            .try_bind(KeyBinding {
                key: KeyCode::KeyS,
                modifiers: Modifiers::NONE,
                context: InputContext::Gameplay,
                action: GameKey::Down,
            })
            .expect("首次绑定不冲突");

        // Act
        let result = table.try_bind(KeyBinding {
            key: KeyCode::KeyS,
            modifiers: Modifiers {
                shift: false,
                ctrl: true,
                alt: false,
            },
            context: InputContext::Gameplay,
            action: GameKey::Screenshot,
        });

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn 同一个动作绑定两个键属于多绑不算冲突() {
        // Arrange
        let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
        table
            .try_bind(KeyBinding {
                key: KeyCode::ArrowUp,
                modifiers: Modifiers::NONE,
                context: InputContext::Gameplay,
                action: GameKey::Up,
            })
            .expect("首次绑定不冲突");

        // Act
        let result = table.try_bind(KeyBinding {
            key: KeyCode::KeyW,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action: GameKey::Up,
        });

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn 多绑后两个键都能解析出同一动作() {
        // Arrange
        let table =
            KeyBindings::from_bindings([KeyBinding::gameplay(KeyCode::ArrowUp, GameKey::Up)])
                .expect("单条绑定不冲突");
        let mut table = table;
        table
            .try_bind(KeyBinding::gameplay(KeyCode::KeyW, GameKey::Up))
            .expect("多绑同一动作不冲突");

        // Act
        let via_arrow = table.resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Gameplay);
        let via_letter = table.resolve(KeyCode::KeyW, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(
            (via_arrow, via_letter),
            (Some(GameKey::Up), Some(GameKey::Up))
        );
    }

    #[test]
    fn bindings_for只返回指定动作的绑定() {
        // `bindings_for` 按 `action` 过滤，不按 `context` 过滤——
        // `GameKey::Up` 现在同时被 `DEFAULT_BINDINGS`（Gameplay）与
        // `DEFAULT_MENU_BINDINGS`（Menu）各绑了 `ArrowUp`/`KeyW` 两个
        // 物理键，因此这里只筛出 Gameplay 上下文那一半，理由见
        // `bindings_for` 文档「设置界面展示这个动作当前绑了哪些键」——
        // 展示界面天然是按上下文分别展示的，不会把两个上下文的绑定
        // 混在一起呈现。
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let up_keys: Vec<KeyCode> = table
            .bindings_for(GameKey::Up)
            .filter(|binding| binding.context == InputContext::Gameplay)
            .map(|binding| binding.key)
            .collect();

        // Assert
        assert_eq!(up_keys, vec![KeyCode::ArrowUp, KeyCode::KeyW]);
    }

    #[test]
    fn bindings_for涵盖菜单上下文下的绑定() {
        // 上一条测试只看 Gameplay 那一半，这条测试核实 Menu 那一半也
        // 确实被 `bindings_for` 看到（防止未来有人误以为 Menu 绑定表
        // 是另一套没有接进同一张 `bindings` 表的平行数据）。
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let menu_up_keys: Vec<KeyCode> = table
            .bindings_for(GameKey::Up)
            .filter(|binding| binding.context == InputContext::Menu)
            .map(|binding| binding.key)
            .collect();

        // Assert
        assert_eq!(menu_up_keys, vec![KeyCode::ArrowUp, KeyCode::KeyW]);
    }

    #[test]
    fn 修饰键状态从winit状态换算正确() {
        // Arrange
        let mut state = winit::keyboard::ModifiersState::empty();
        state.insert(winit::keyboard::ModifiersState::SHIFT);

        // Act
        let modifiers = Modifiers::from(state);

        // Assert
        assert_eq!(
            modifiers,
            Modifiers {
                shift: true,
                ctrl: false,
                alt: false,
            }
        );
    }

    #[test]
    fn 绑定表能序列化后再反序列化出等价内容() {
        // 验证的是「经过一种真实的 serde 格式往返」而不只是 derive 能
        // 编译，见 ADR 0011「验证方式」一节的同款要求。
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let json = serde_json::to_string(&table).expect("默认绑定表应能序列化");
        let restored: KeyBindings = serde_json::from_str(&json).expect("刚序列化的数据应能读回");

        // Assert
        assert_eq!(restored.bindings(), table.bindings());
    }

    #[test]
    fn 默认绑定表能解析放大键() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::Equal, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::ZoomIn));
    }

    #[test]
    fn 默认绑定表能解析缩小键() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::Minus, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::ZoomOut));
    }

    #[test]
    fn 默认滚轮绑定能解析出放大动作() {
        // 滚轮与按键绑给同一对抽象动作,是「同一个抽象动作可由滚轮或
        // 按键触发」的直接验证。
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve_wheel(WheelDirection::Away, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::ZoomIn));
    }

    #[test]
    fn 默认滚轮绑定能解析出缩小动作() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve_wheel(WheelDirection::Toward, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::ZoomOut));
    }

    #[test]
    fn 同一个滚动方向绑给两个不同动作时注册被拒绝() {
        // Arrange
        let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
        table
            .try_bind_wheel(WheelBinding {
                direction: WheelDirection::Away,
                context: InputContext::Gameplay,
                action: GameKey::ZoomIn,
            })
            .expect("首次绑定不冲突");

        // Act
        let result = table.try_bind_wheel(WheelBinding {
            direction: WheelDirection::Away,
            context: InputContext::Gameplay,
            action: GameKey::ZoomOut,
        });

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 滚轮与按键分属不同判重维度互不冲突() {
        // 一个动作同时被按键与滚轮绑定,不该被判定为冲突——两者是完全
        // 独立的判重表,见 WheelDirection 文档「为什么滚轮是独立的一套
        // 抽象」一节。
        // Arrange
        let mut table =
            KeyBindings::from_bindings([KeyBinding::gameplay(KeyCode::Equal, GameKey::ZoomIn)])
                .expect("单条绑定不冲突");

        // Act
        let result = table.try_bind_wheel(WheelBinding {
            direction: WheelDirection::Away,
            context: InputContext::Gameplay,
            action: GameKey::ZoomIn,
        });

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn 未绑定的滚动方向解析为空值() {
        // Arrange
        let table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");

        // Act
        let action = table.resolve_wheel(WheelDirection::Away, InputContext::Gameplay);

        // Assert
        assert_eq!(action, None);
    }

    #[test]
    fn 绑定表含滚轮绑定时仍能序列化后再反序列化出等价内容() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let json = serde_json::to_string(&table).expect("默认绑定表应能序列化");
        let restored: KeyBindings = serde_json::from_str(&json).expect("刚序列化的数据应能读回");

        // Assert
        assert_eq!(restored.wheel_bindings(), table.wheel_bindings());
    }

    #[test]
    fn 旧版本不含滚轮字段的配置文件仍能反序列化() {
        // 兜底旧配置文件——本字段引入之前写出的 JSON 不含 wheel_bindings
        // 键,`#[serde(default)]` 应当把它当成空列表处理,而不是解析失败。
        // Arrange
        let json = r#"{"bindings":[
            {"key":"KeyQ","modifiers":{"shift":false,"ctrl":false,"alt":false},"context":"Gameplay","action":"Menu"}
        ]}"#;

        // Act
        let table: KeyBindings =
            serde_json::from_str(json).expect("缺失 wheel_bindings 字段应当兜底");

        // Assert
        assert!(table.wheel_bindings().is_empty());
    }

    #[test]
    fn 滚轮反序列化遇到冲突时拒绝而不是绕过校验() {
        // 与按键版本的 ADR 0011 测试同一类攻击面。
        // Arrange
        let json = r#"{"bindings":[],"wheel_bindings":[
            {"direction":"Away","context":"Gameplay","action":"ZoomIn"},
            {"direction":"Away","context":"Gameplay","action":"ZoomOut"}
        ]}"#;

        // Act
        let result: Result<KeyBindings, _> = serde_json::from_str(json);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 竖直负增量换算成远离方向() {
        // Arrange & Act
        let direction = WheelDirection::from_scroll_delta(MouseScrollDelta::LineDelta(0.0, -1.0));

        // Assert
        assert_eq!(direction, Some(WheelDirection::Away));
    }

    #[test]
    fn 竖直正增量换算成靠近方向() {
        // Arrange & Act
        let direction = WheelDirection::from_scroll_delta(MouseScrollDelta::LineDelta(0.0, 1.0));

        // Assert
        assert_eq!(direction, Some(WheelDirection::Toward));
    }

    #[test]
    fn 零增量换算为空值() {
        // Arrange & Act
        let direction = WheelDirection::from_scroll_delta(MouseScrollDelta::LineDelta(0.0, 0.0));

        // Assert
        assert_eq!(direction, None);
    }

    #[test]
    fn 触控板像素增量同样按竖直分量符号判定() {
        // Arrange & Act
        let direction = WheelDirection::from_scroll_delta(MouseScrollDelta::PixelDelta(
            winit::dpi::PhysicalPosition::new(0.0, -5.0),
        ));

        // Assert
        assert_eq!(direction, Some(WheelDirection::Away));
    }

    #[test]
    fn 默认绑定表在菜单上下文下能解析方向键() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Menu);

        // Assert
        assert_eq!(action, Some(GameKey::Up));
    }

    #[test]
    fn 默认绑定表在菜单上下文下能解析确认键() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::Enter, Modifiers::NONE, InputContext::Menu);

        // Assert
        assert_eq!(action, Some(GameKey::Confirm));
    }

    #[test]
    fn 默认绑定表在菜单上下文下能解析取消键() {
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::Escape, Modifiers::NONE, InputContext::Menu);

        // Assert
        assert_eq!(action, Some(GameKey::Cancel));
    }

    #[test]
    fn 只在游戏内上下文绑定的键在菜单上下文下解析为空值() {
        // 截图键（F2）只出现在 DEFAULT_BINDINGS（Gameplay），不在
        // DEFAULT_MENU_BINDINGS 里——这条测试核实两张表确实是按上下文
        // 隔离的，不是不小心共用了同一份判重逻辑而让所有键都跨上下文
        // 生效。
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let action = table.resolve(KeyCode::F2, Modifiers::NONE, InputContext::Menu);

        // Assert
        assert_eq!(action, None);
    }

    #[test]
    fn 反序列化遇到冲突绑定时拒绝而不是绕过校验() {
        // 这是 ADR 0011 要防的事：手改的配置文件可能包含冲突绑定，
        // 若直接派生 Deserialize 会绕开 try_bind 的校验，把非法状态
        // 直接怼进私有字段。
        // Arrange：手写一份把同一个键绑给两个不同动作的 JSON。
        let json = r#"{"bindings":[
            {"key":"KeyQ","modifiers":{"shift":false,"ctrl":false,"alt":false},"context":"Gameplay","action":"Menu"},
            {"key":"KeyQ","modifiers":{"shift":false,"ctrl":false,"alt":false},"context":"Gameplay","action":"Map"}
        ]}"#;

        // Act
        let result: Result<KeyBindings, _> = serde_json::from_str(json);

        // Assert
        assert!(result.is_err());
    }
}
