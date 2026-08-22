//! 模态 UI 栈：驱动 `ll_platform::keybind::InputContext` 在
//! `Gameplay`/`Menu` 之间切换，并保证每次切换都清空
//! `ll_platform::input::InputState`——见
//! `knowledge/design/action-capability-and-input-context.md` 2.1/2.3
//! 节完整论证。
//!
//! # 为什么栈活在 ll-ui，不是 ll-platform
//!
//! `InputContext`（`ll_platform::keybind`）是 `KeyBindings` 冲突检测的
//! 判重维度，一个无状态的分类标签——设计文档 2.1 节的结论：不该让它
//! 自己变成一个 `Vec<InputContext>`，嵌套语义（游戏中 → 背包 → 物品
//! 详情 → 确认框）是 UI 导航层自己的状态，`KeyBindings::resolve` 不该
//! 关心"之前发生过什么"。[`UiModeStack`] 因此活在 `ll-ui`（本 crate 现
//! 在依赖 `ll-platform`，见本 crate `Cargo.toml` 的说明），不是
//! `ll-platform`。
//!
//! # 上下文切换时按住的键：第三种「隐式全键松开」边界
//!
//! `InputState` 已经暴露过完全同构的一个 bug 并修好了它——**窗口
//! 失焦**：玩家按住方向键时切到别的窗口，操作系统只把按键事件送给
//! 有焦点的窗口，对应的松开事件永远送不到，若不清空，`held` 永久为
//! 真。`InputContext` 切换是同一类 bug 的另一个实例：`held`/
//! `repeat_next_at` 按 `GameKey`（`resolve` 之后的抽象动作）索引，不
//! 是按 `(KeyCode, InputContext)` 索引，`W` 在 `Gameplay` 与 `Menu`
//! 两个上下文下解析到同一个 `GameKey::Up`（见 `InputContext::Menu`
//! 文档「共享同一份物理键映射」），于是 `held[GameKey::Up]` 在切换前后
//! 是同一个数组槽位，不会自动归零——打开菜单时若正按着 W，背包一打开
//! 就会立刻读到「已按住」，用一份为移动场景建立的重复计时基准触发
//! 菜单光标的自动重复；反过来，关闭菜单时若仍按着方向键，回到
//! `Gameplay` 后角色会立刻开始移动，即使玩家从未在 `Gameplay` 上下文
//! 下按过这个键。
//!
//! **结论（设计文档已经钉死，直接复用，不新增方法）**：`InputContext`
//! 每一次切换都必须调用一次 `InputState::clear()`，与失焦时完全同一个
//! 函数、同一套语义——[`UiModeStack::push`]/[`UiModeStack::pop`] 因此
//! 都要求调用方传入 `&mut InputState`，内部只是老老实实调用
//! `clear()`，不重新发明一套「上下文专用」的清空逻辑。
//!
//! # 本批次只有 `UiMode::Menu` 一个变体，且尚未接入真实游戏循环
//!
//! 与 `InputContext::Menu` 同一条克制（见其文档）：背包/确认框等具体
//! 交互界面明确排在下一批（任务书「不做」一节），本类型现在只是把
//! 「push/pop 必须清空 InputState」这条不变式做实、做测，真正驱动它
//! 的调用点（`ll_game::app::Demo` 在什么时候 push `UiMode::Menu`）留给
//! 下一批——那需要 `ll_platform::window::AppHandler::on_frame` 能把
//! `&mut InputState` 交给上层（当前签名是 `&InputState`，见其文档），
//! 而这个签名被六个跨 crate 的验收 demo 共用（`ll-world`/`ll-render`/
//! `ll-sim`/`ll-platform`/`ll-ui` 的 `p0`~`p5_coordinate_acceptance`
//! 示例），改动它的正确时机是下一批真正有内容要 push 到菜单里的时候，
//! 不是本批次——本批次交付的是这台机器本身，接线留给有真实负载的
//! 那一刻。

use ll_platform::input::InputState;
use ll_platform::keybind::InputContext;

/// 覆盖游戏画面的模态 UI 种类——本批次只有 `Menu` 一种，对应
/// `InputContext::Menu`（背包/物品详情/确认框等尚未建成的场景全部共用
/// 这一个变体，见其文档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    /// 任意模态菜单类 UI。
    Menu,
}

/// 当前打开的模态 UI 栈——见模块文档。
#[derive(Debug, Clone, Default)]
pub struct UiModeStack {
    stack: Vec<UiMode>,
}

impl UiModeStack {
    /// 建一个空栈——空栈时 [`Self::current_context`] 恒为
    /// `InputContext::Gameplay`。
    pub fn new() -> UiModeStack {
        UiModeStack::default()
    }

    /// 栈空则为 `Gameplay`，否则为 `Menu`——见模块文档 2.1 节引用的
    /// 设计结论：栈顶决定当前用哪个 `InputContext` 查表，具体是哪一层
    /// 菜单由调用方自己的路由逻辑决定，不是 `InputContext` 的职责。
    pub fn current_context(&self) -> InputContext {
        if self.stack.is_empty() {
            InputContext::Gameplay
        } else {
            InputContext::Menu
        }
    }

    /// 栈是否为空。
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// 当前栈深度。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 压入一层新的模态 UI，并把这一刻按住的键视为隐式全部松开——见
    /// 模块文档「上下文切换时按住的键」一节。
    pub fn push(&mut self, mode: UiMode, input: &mut InputState) {
        self.stack.push(mode);
        input.clear();
    }

    /// 弹出最上层模态 UI，理由同 [`Self::push`]。栈已空时不做任何事、
    /// 也不清空 `InputState`——没有发生真正的上下文切换（栈从空到空），
    /// 不该清空玩家正在 `Gameplay` 上下文里按着的键。
    pub fn pop(&mut self, input: &mut InputState) -> Option<UiMode> {
        let popped = self.stack.pop();
        if popped.is_some() {
            input.clear();
        }
        popped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_platform::input::GameKey;

    #[test]
    fn 新建的栈上下文是游戏内主流程() {
        // Arrange & Act
        let stack = UiModeStack::new();

        // Assert
        assert_eq!(stack.current_context(), InputContext::Gameplay);
    }

    #[test]
    fn 压入菜单后上下文变为菜单() {
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();

        // Act
        stack.push(UiMode::Menu, &mut input);

        // Assert
        assert_eq!(stack.current_context(), InputContext::Menu);
    }

    #[test]
    fn 压入菜单时按住的键在压入后不再按住() {
        // 这是本模块要保证的核心不变式：打开菜单时正按着 W，不该带着
        // 「已按住」的状态进入菜单上下文。
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();
        input.press(GameKey::Up);

        // Act
        stack.push(UiMode::Menu, &mut input);

        // Assert
        assert!(!input.is_held(GameKey::Up));
    }

    #[test]
    fn 弹出后栈空时上下文回到游戏内主流程() {
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();
        stack.push(UiMode::Menu, &mut input);

        // Act
        stack.pop(&mut input);

        // Assert
        assert_eq!(stack.current_context(), InputContext::Gameplay);
    }

    #[test]
    fn 弹出菜单时按住的键在弹出后不再按住() {
        // 对称场景：玩家在菜单里按着方向键选东西还没松手就关了菜单，
        // 回到 Gameplay 后不该立刻开始移动。
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();
        stack.push(UiMode::Menu, &mut input);
        input.press(GameKey::Up);

        // Act
        stack.pop(&mut input);

        // Assert
        assert!(!input.is_held(GameKey::Up));
    }

    #[test]
    fn 栈已空时弹出不panic且返回空值() {
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();

        // Act
        let popped = stack.pop(&mut input);

        // Assert
        assert_eq!(popped, None);
    }

    #[test]
    fn 栈已空时弹出不清空正在游戏内按住的键() {
        // 没有发生真正的上下文切换（栈从空到空），不该误清空玩家正在
        // Gameplay 上下文里按着的键。
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();
        input.press(GameKey::Up);

        // Act
        stack.pop(&mut input);

        // Assert
        assert!(input.is_held(GameKey::Up));
    }

    #[test]
    fn 弹出后返回被弹出的模式() {
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();
        stack.push(UiMode::Menu, &mut input);

        // Act
        let popped = stack.pop(&mut input);

        // Assert
        assert_eq!(popped, Some(UiMode::Menu));
    }
}
