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
//! # 清键的判据是上下文变没变，不是栈动没动
//!
//! 上一节那条纪律**此前写成了「每一次 `push`/`pop` 都清」**。导航收敛
//! 批次（规格 N8）把判据收紧成它本来的样子：**`current_context()` 前后
//! 不同才清**。理由就是上一节自己的论证——要防的是「切换那一刻按住的键
//! 被带过边界」，没有切换就没有边界可跨。
//!
//! 这不是放松：对本条落地之前**已经存在**的四种转移（空→`Menu`、
//! `Menu`→`TextEntry`、`TextEntry`→`Menu`、`Menu`→空），每一次都真的换了
//! 上下文，因此行为逐条等价。收紧只影响下一节那两个新变体。
//!
//! # 新变体为什么不换键位表
//!
//! [`UiMode::PlayerMenu`]（背包/制作/交互列表）与 [`UiMode::Overlay`]
//! （世界地图）的 [`UiModeStack::current_context`] **仍然是
//! `InputContext::Gameplay`**。这是规格
//! `knowledge/design/ui-and-navigation.md` N8 的明文裁定：
//! `ll_game::player_action` 模块文档论证过「键位表不该换」（换了以后
//! I 键/C 键/空格在背包里全部解析不出来，「再按一次 I 关背包」这条既有
//! 行为当场消失），而「要不要进栈」是另一件事。
//!
//! 两者进栈换来的是**「现在有没有东西盖着屏幕」终于只有一个答案**：
//! 在此之前，背包开着、地图开着的时候 [`UiModeStack::is_empty`] 都返回
//! 真，于是取消键的顶层判据要靠 `&& !xxx.is_open()` 一条条手工补，而
//! 漏了不报错。配对由 `ll_game::modal::Modal` 结构性保证，见那个类型。
//!
//! # 只有 `UiMode::Menu` 一个变体，但它已经接上真实游戏循环了
//!
//! **这一节改写过**：本类型落地那一批写的是「尚未接入真实游戏循环，
//! 接线留给下一批」。那一刻已经过去——`ll_platform::window::AppHandler::on_frame`
//! 的签名早已改成 `&mut InputState`，`ll_game::app::Demo` 现在持有本
//! 类型，并把它当作「现在有没有一块模态屏盖着」的唯一真相源：平台层
//! 每次解析物理键都调一次 `AppHandler::input_context()`，那个方法返回
//! 的就是 [`UiModeStack::current_context`]。
//!
//! `Menu` 那一个变体覆盖全部模态菜单屏，是 `InputContext::Menu` 那条
//! 克制的延续（见其文档）：游戏内菜单、设置界面、游戏主菜单（首页）
//! 共用这一个变体，「现在具体是哪一块屏」由
//! `ll_game::menu_screen::ScreenState` 回答，不是本类型的职责。
//!
//! **本节标题此前写的是「只有 `UiMode::Menu` 一个变体」**，那一刻已经
//! 过去两次：文本输入批次加了 `TextEntry`，导航收敛批次（规格 N8）又加了
//! `PlayerMenu` 与 `Overlay`。判据从来不是「克制到只留一个」，而是
//! **一个变体 ⇔ 一张键位表**——见上面「新变体为什么不换键位表」一节
//! 对后两个变体为什么共用 `Gameplay` 那张表的论证。
//!
//! # 栈空不等于「在世界里」了
//!
//! 游戏主菜单（首页）落地之后，这条不变式的措辞变了，**如实记下**：
//!
//! - **栈非空 ⇔ 有一块模态屏盖着 ⇔ 按 `InputContext::Menu` 解析物理键**
//!   ——这一条原样成立，从来没变。
//! - 变的是它的反面：栈空**曾经**等价于「玩家在世界里」，因为那时候
//!   游戏一启动就直接进世界。现在启动后先停在首页，那一刻栈非空、
//!   而世界**还不存在**（`ll_game::app::Demo::session` 为 `None`）。
//!   所以现在只剩单向：**栈空 ⇒ 有世界在跑**；栈非空推不出世界的有无。
//!
//! 首页在第一帧之前就已经开着，因此它用的是 [`UiModeStack::opened`]
//! 而不是 [`UiModeStack::push`]——两者的区别见那个构造器的文档。

use ll_platform::input::InputState;
use ll_platform::keybind::InputContext;

/// 覆盖游戏画面的模态 UI 种类——**一个变体对应一张键位表**，见模块
/// 文档「新变体为什么不换键位表」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    /// 任意模态菜单屏（首页、暂停菜单、设置、角色创建……），对应
    /// `InputContext::Menu`。「现在具体是哪一块屏」由
    /// `ll_game::menu_screen::ScreenState` 回答。
    Menu,
    /// 玩家正在往一个输入框里打字（存档命名，将来的角色命名/聊天/
    /// 搜索）——对应 `ll_platform::keybind::InputContext::TextEntry`。
    ///
    /// # 它为什么值得一个独立变体，而 13 块菜单屏共用一个
    ///
    /// `Menu` 那一个变体覆盖所有模态菜单，是因为它们**共用同一张键位
    /// 表**（方向键导航、Enter/Space 确认、Esc 返回），「具体是哪一块
    /// 屏」由 `ll_game::menu_screen::ScreenState` 回答。
    ///
    /// 文本输入态不同：它需要的是**另一张表**——空格必须是一个字符
    /// 而不是确认，WASD 必须解析不出任何动作（否则玩家打字会让角色
    /// 走起来）。换一张表就是换一个 `InputContext`，换一个
    /// `InputContext` 就要在这里有一个变体。
    ///
    /// 它同时是「输入法开不开」的判据，见
    /// `ll_platform::window::AppHandler::input_context` 文档。
    TextEntry,
    /// 玩家菜单（背包 / 制作 / 方向列表 / 交互列表）——跑在
    /// `InputContext::Gameplay` 上，见 [`UiModeStack::current_context`]
    /// 与本模块「新变体为什么不换键位表」一节。
    PlayerMenu,
    /// 覆盖世界的观测浮层（今天只有世界地图）——同样跑在
    /// `InputContext::Gameplay` 上。
    Overlay,
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

    /// 建一个**开局就压着一层**的栈。
    ///
    /// # 为什么它不要 `&mut InputState`，而 [`Self::push`] 要
    ///
    /// 游戏主菜单（首页）在**第一帧之前**就已经盖在屏幕上了：
    /// `ll_game::app::Demo` 一构造出来就停在首页，那一刻平台层的事件
    /// 循环还没启动，既没有 `InputState` 可以传进来，也不可能有任何键
    /// 正被按住。
    ///
    /// [`Self::push`]/[`Self::pop`] 要求 `&mut InputState` 是因为它们
    /// 表达的是**运行期的一次上下文切换**——切换那一刻玩家可能正按着
    /// 键，不清空就会把「已按住」带过边界（模块文档「上下文切换时按住
    /// 的键」一节）。本构造器表达的是**初始状态**，没有「切换前」可言，
    /// 因此不是那条纪律的旁路，而是它压根不适用的另一种情形。
    ///
    /// **不要用它替代 `push`**：运行期任何一次真正的开屏都必须走
    /// `push`，否则那条纪律就被绕过了。
    pub fn opened(mode: UiMode) -> UiModeStack {
        UiModeStack { stack: vec![mode] }
    }

    /// **栈顶**决定当前用哪个 `InputContext` 查表——见模块文档 2.1 节
    /// 引用的设计结论：具体是哪一层菜单由调用方自己的路由逻辑决定，
    /// 不是 `InputContext` 的职责。
    ///
    /// 本方法此前写的是「空则 `Gameplay`，否则 `Menu`」。文本输入批次
    /// 之后必须**真的看栈顶**：一块文本输入屏压在菜单屏上时，两者要
    /// 查的是不同的表（空格在菜单里是确认、在输入框里是一个字符），
    /// 二分法答不出这个区别。
    pub fn current_context(&self) -> InputContext {
        match self.stack.last() {
            None => InputContext::Gameplay,
            Some(UiMode::Menu) => InputContext::Menu,
            Some(UiMode::TextEntry) => InputContext::TextEntry,
            // 这两层**刻意仍然是 `Gameplay`**，见模块文档
            // 「新变体为什么不换键位表」一节。
            Some(UiMode::PlayerMenu | UiMode::Overlay) => InputContext::Gameplay,
        }
    }

    /// 栈顶那一层；栈空时为 `None`。
    ///
    /// 调用方（`ll_game::app::Demo::sync_text_entry_mode`）用它判断
    /// 「现在这层是不是已经是想要的那层」，避免重复压栈。
    pub fn top(&self) -> Option<UiMode> {
        self.stack.last().copied()
    }

    /// 栈是否为空。
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// 当前栈深度。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// 压入一层新的模态 UI；**若这一压真的换了 `InputContext`**，把这一刻
    /// 按住的键视为隐式全部松开——见模块文档「上下文切换时按住的键」与
    /// 「清键的判据是上下文变没变，不是栈动没动」两节。
    pub fn push(&mut self, mode: UiMode, input: &mut InputState) {
        let before = self.current_context();
        self.stack.push(mode);
        self.clear_if_context_changed(before, input);
    }

    /// 弹出最上层模态 UI，理由同 [`Self::push`]。栈已空时不做任何事、
    /// 也不清空 `InputState`——没有发生真正的上下文切换（栈从空到空），
    /// 不该清空玩家正在 `Gameplay` 上下文里按着的键。
    pub fn pop(&mut self, input: &mut InputState) -> Option<UiMode> {
        let before = self.current_context();
        let popped = self.stack.pop();
        if popped.is_some() {
            self.clear_if_context_changed(before, input);
        }
        popped
    }

    /// 栈里有几层是 `mode`——配对断言的判据，见
    /// `ll_game::modal::Modal` 的一致性断言：每一类模态 UI 在栈里恰好
    /// 有零层或一层，多一层少一层都是 push/pop 没配对。
    pub fn count(&self, mode: UiMode) -> usize {
        self.stack.iter().filter(|it| **it == mode).count()
    }

    /// 把栈里**最上面那一层** `mode` 抽掉，其余各层原样保留；栈里没有
    /// 这一类时什么都不做，返回是否真的抽掉了一层。
    ///
    /// # 为什么不是 [`Self::pop`]
    ///
    /// 要关掉的那一层未必在栈顶：玩家可以先开背包再开地图，也可以反
    /// 过来，而关地图只该关地图。`pop` 在这种场景下会弹错层，配对当场
    /// 断掉——而那正是本方法存在的场景。
    ///
    /// 清键的判据仍然是 [`Self::clear_if_context_changed`]：抽掉的若是
    /// 栈顶那一层且上下文因此改变，才清；抽掉栈中间那一层不改变栈顶，
    /// 也就没有跨边界可言。
    pub fn remove_topmost(&mut self, mode: UiMode, input: &mut InputState) -> bool {
        let before = self.current_context();
        let Some(index) = self.stack.iter().rposition(|it| *it == mode) else {
            return false;
        };
        self.stack.remove(index);
        self.clear_if_context_changed(before, input);
        true
    }

    /// 清键的**唯一**判据：`current_context()` 前后不同。
    ///
    /// 见模块文档「清键的判据是上下文变没变，不是栈动没动」一节——这条
    /// 对本方法落地之前已有的四种转移（空→`Menu`、`Menu`→`TextEntry`、
    /// `TextEntry`→`Menu`、`Menu`→空）逐条等价，因为那四种转移每一次都
    /// **真的**换了上下文。
    fn clear_if_context_changed(&self, before: InputContext, input: &mut InputState) {
        if self.current_context() != before {
            input.clear();
        }
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
    fn 文本输入层压在菜单层上时上下文按栈顶算() {
        // 本方法此前是「空/非空」二分，二分法答不出「菜单屏上盖着一块
        // 输入框」这个区别——而那正是存档命名屏的真实形状：空格在菜单
        // 里是确认，在输入框里是一个字符。
        // Arrange
        let mut stack = UiModeStack::opened(UiMode::Menu);
        let mut input = InputState::new();

        // Act & Assert：压上去
        stack.push(UiMode::TextEntry, &mut input);
        assert_eq!(stack.current_context(), InputContext::TextEntry);
        assert_eq!(stack.top(), Some(UiMode::TextEntry));
        assert_eq!(stack.depth(), 2);

        // Act & Assert：弹回来，菜单层还在
        stack.pop(&mut input);
        assert_eq!(stack.current_context(), InputContext::Menu);
        assert_eq!(stack.top(), Some(UiMode::Menu));
    }

    #[test]
    fn 开局就压着一层的栈深度为一且上下文是菜单() {
        // 游戏主菜单（首页）在第一帧之前就已经开着——平台层解析第一批
        // 物理键时就必须按菜单那张表查，否则首页的第一批按键会按
        // Gameplay 表解析。
        // Arrange & Act
        let stack = UiModeStack::opened(UiMode::Menu);

        // Assert
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.current_context(), InputContext::Menu);
    }

    #[test]
    fn 开局就压着一层的栈弹一次就空() {
        // 首页那一层被弹掉的时刻就是玩家真正进世界的时刻，弹完必须
        // 回到 Gameplay，否则进了世界还在按菜单表解析方向键。
        // Arrange
        let mut stack = UiModeStack::opened(UiMode::Menu);
        let mut input = InputState::new();

        // Act
        let popped = stack.pop(&mut input);

        // Assert
        assert_eq!(popped, Some(UiMode::Menu));
        assert!(stack.is_empty());
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
    fn 玩家菜单层与浮层不换键位表() {
        // 规格 N8 的明文裁定：这两层只进栈，不换 `InputContext`——换了
        // 以后 I/C/空格在背包里全部解析不出来。
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();

        // Act & Assert
        stack.push(UiMode::PlayerMenu, &mut input);
        assert_eq!(stack.current_context(), InputContext::Gameplay);
        assert_eq!(stack.depth(), 1, "不换表不等于不进栈");
        stack.push(UiMode::Overlay, &mut input);
        assert_eq!(stack.current_context(), InputContext::Gameplay);
        assert_eq!(stack.depth(), 2);
    }

    #[test]
    fn 压入不换上下文的一层时不清空按住的键() {
        // 清键的理由是「切换那一刻按住的键被带过边界」；玩家菜单那一层
        // 压根没跨边界（前后都是 Gameplay），清掉就等于把玩家正按着的
        // 方向键无故吞掉一次。
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();
        input.press(GameKey::Up);

        // Act
        stack.push(UiMode::PlayerMenu, &mut input);

        // Assert
        assert!(input.is_held(GameKey::Up));
    }

    #[test]
    fn 弹出不换上下文的一层时同样不清空按住的键() {
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();
        stack.push(UiMode::Overlay, &mut input);
        input.press(GameKey::Up);

        // Act
        stack.pop(&mut input);

        // Assert
        assert!(input.is_held(GameKey::Up));
    }

    #[test]
    fn 在玩家菜单层之上压入菜单屏仍然清空按住的键() {
        // 反面：这一次上下文真的从 Gameplay 变成了 Menu，纪律照旧生效。
        // Arrange
        let mut stack = UiModeStack::new();
        let mut input = InputState::new();
        stack.push(UiMode::PlayerMenu, &mut input);
        input.press(GameKey::Up);

        // Act
        stack.push(UiMode::Menu, &mut input);

        // Assert
        assert!(!input.is_held(GameKey::Up));
        assert_eq!(stack.current_context(), InputContext::Menu);
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
