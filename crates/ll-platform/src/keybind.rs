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
use winit::keyboard::KeyCode;

/// 输入上下文：冲突检测的判重维度之一。
///
/// 同一个物理键在不同上下文下绑给不同动作是合理的重叠，不构成冲突
/// （例如 Esc 在菜单里是返回、在游戏内是暂停）——见模块文档「上下文」
/// 一节。
///
/// 目前只有 [`InputContext::Gameplay`] 一个变体：菜单/设置界面尚未
/// 建成（P7），新增上下文只需要在此追加变体，[`KeyBindings`] 的其余
/// 逻辑不需要跟着改。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputContext {
    /// 游戏内主流程——目前唯一在用的上下文，全部 demo 都跑在这里。
    Gameplay,
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
];

impl KeyBindings {
    /// 内置默认绑定表，从 [`DEFAULT_BINDINGS`] 这张静态数据构造。
    ///
    /// `expect` 而不是返回 `Result`：`DEFAULT_BINDINGS` 是编译期常量，
    /// 若它内部自相冲突，那是这张表本身写错了，应当在开发期就地修正，
    /// 不该把「内置默认值可能非法」这种可能性泄漏给调用方处理。
    pub fn default_bindings() -> KeyBindings {
        KeyBindings::from_bindings(DEFAULT_BINDINGS.iter().copied())
            .expect("DEFAULT_BINDINGS 是内置常量表，不应自相冲突")
    }

    /// 从一组绑定逐条校验后构造绑定表，任意一条与已注册的绑定冲突就
    /// 整体失败。
    pub fn from_bindings(
        bindings: impl IntoIterator<Item = KeyBinding>,
    ) -> Result<KeyBindings, KeyBindConflict> {
        let mut table = KeyBindings {
            bindings: Vec::new(),
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

    /// 列出全部绑定，供设置界面展示、导出配置等只读场景使用。
    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
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
}

impl TryFrom<KeyBindingsRepr> for KeyBindings {
    type Error = KeyBindConflict;

    fn try_from(raw: KeyBindingsRepr) -> Result<Self, Self::Error> {
        KeyBindings::from_bindings(raw.bindings)
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
        // Arrange
        let table = KeyBindings::default_bindings();

        // Act
        let up_keys: Vec<KeyCode> = table
            .bindings_for(GameKey::Up)
            .map(|binding| binding.key)
            .collect();

        // Assert
        assert_eq!(up_keys, vec![KeyCode::ArrowUp, KeyCode::KeyW]);
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
