//! 「现在盖着屏幕的是哪一层」的**唯一真相源**。
//!
//! # 这个模块补的是哪条断线
//!
//! `knowledge/design/ui-and-navigation.md` 第〇节的诊断原文：**有三套
//! 互不知情的模态系统**——
//!
//! | 模态系统 | 状态住在哪（本模块之前） | 进 `UiModeStack` 吗 |
//! |---|---|---|
//! | 模态屏 | `app::Demo::screen` | 进 |
//! | 玩家菜单 | `app::Demo::menu` | **不进** |
//! | 世界地图 | `app::Demo::world_map_open` | **不进** |
//!
//! 后果规格里逐条列着：Esc 被实现了两遍（`app` 与 `player_action` 各
//! 一处），地图那一套一遍都没有——开着地图按 Esc 弹出来的是暂停菜单，
//! 而且方向键**同时**驱动菜单光标和地图平移；每加一套新的模态 UI 就要
//! 在 `on_frame` 里手工补一条 `&& !xxx.is_open()`，**漏了不报错**。
//!
//! # 为什么是一个私有字段的结构体，不是「四个字段 + 一条纪律」
//!
//! 纪律拦不住「新加一块模态 UI 的人不知道还要压栈」。本类型把那四样
//! 东西全部收成**私有字段**，`ll_game` 里除本模块之外的任何地方都
//! **写不出** `world_map_open = true` 这种「只改自己那一份、栈不知道」
//! 的语句——那是一个编译错误，不是一条会被漏掉的约定。
//!
//! 这与本仓库另外三处先例同一种解法（存档时重算生成期 mod 集合、肉鸽
//! 模式反向升级、新世界配老槽位）：**写不出来**比**提醒一下**可靠。
//!
//! # 栈与状态并存，不是二选一
//!
//! 规格十一节否决过「把 `world_map_open`/`PlayerMenu` 直接删掉、状态
//! 全塞进 `UiModeStack`」：栈只回答「有没有、哪一类」，回答不了「地图
//! 视野中心在哪、背包光标停在第几行」。因此本类型的做法是**栈与状态
//! 并存、开关时配对**，配对本身由 [`Modal::assert_paired`] 每次改动后
//! 断言一次。
//!
//! # 唯一那个可变借用的缺口，以及它怎么被堵上
//!
//! `crate::player_action::player_command` 收 `&mut PlayerMenu` 并在内部
//! 开关它（I 键/C 键/空格/取消键四个入口都在那一层）。本类型因此
//! **不提供** `menu_mut()`：拿到裸引用就等于绕过配对。取而代之的是
//! [`Modal::with_player_menu`]——闭包拿到 `&mut PlayerMenu`，返回之后
//! 由本类型负责把栈重新对齐。拿不到裸引用 ⇒ 绕不过对齐。

use ll_platform::input::InputState;
use ll_platform::keybind::InputContext;
use ll_ui::widget::ui_mode::{UiMode, UiModeStack};

use crate::menu_screen::ScreenState;
use crate::player_action::PlayerMenu;

/// 三套模态 UI 的状态 + 那一套栈，四样东西**只能一起改**。
///
/// 见模块文档。字段全部私有，读走访问器、写走那几个方法。
#[derive(Debug, Default)]
pub struct Modal {
    /// 模态 UI 栈——驱动 `InputContext` 切换的那个真相源，见
    /// `ll_ui::widget::ui_mode` 模块文档。
    stack: UiModeStack,
    /// 模态屏当前开着哪一块（`None` = 没开）。栈管「输入上下文该切到
    /// 哪」，本字段管「这块屏里具体在显示什么、光标在哪」。
    screen: Option<ScreenState>,
    /// 玩家菜单（背包 / 制作 / 方向列表 / 交互列表）当前的状态与光标
    /// 位置。纯表现层状态，不进 `GameWorld`/`WorldState`、不进存档、
    /// 不参与回放，判据见 `crate::player_action` 模块文档。
    menu: PlayerMenu,
    /// 世界地图当前是否处于打开状态（M 键切换）。同上，纯表现层。
    world_map_open: bool,
}

impl Modal {
    /// 什么都没开——玩家直接在世界里。
    pub fn in_world() -> Modal {
        Modal::default()
    }

    /// 开局就停在首页：屏是 [`ScreenState::Title`]，栈里已经压着一层。
    ///
    /// 用 `UiModeStack::opened` 而不是 `push`：首页在**第一帧之前**就
    /// 已经盖在屏幕上了，那一刻还没有 `InputState` 可以清空，也不可能
    /// 有任何键正被按住——见那个构造器的文档。
    pub fn at_title() -> Modal {
        Modal {
            stack: UiModeStack::opened(UiMode::Menu),
            screen: Some(ScreenState::Title),
            menu: PlayerMenu::Closed,
            world_map_open: false,
        }
    }

    /// 模态屏当前开着哪一块。
    pub fn screen(&self) -> Option<ScreenState> {
        self.screen
    }

    /// 玩家菜单这一刻的形态。
    pub fn player_menu(&self) -> PlayerMenu {
        self.menu
    }

    /// 世界地图开没开。
    pub fn world_map_open(&self) -> bool {
        self.world_map_open
    }

    /// 现在一层都没盖着——**「取消键该开主菜单还是该退一层」的唯一
    /// 判据**（规格 N2）。
    ///
    /// 本方法落地之前，这个问题要靠 `screen.is_none() && !menu.is_open()`
    /// 这样一条**每加一套模态 UI 就要多一项、且漏了不报错**的手工合取
    /// 来回答，而世界地图那一项从来没被加进去（规格 D3）。
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// 现在盖着几层。
    pub fn depth(&self) -> usize {
        self.stack.depth()
    }

    /// 这一帧该按哪张键位表解析物理键，见
    /// `ll_ui::widget::ui_mode::UiModeStack::current_context`。
    pub fn input_context(&self) -> InputContext {
        self.stack.current_context()
    }

    /// 打开一块模态屏（此前没有屏开着）。
    ///
    /// 已经有屏开着时**换屏**走 [`Modal::set_screen`]，不重复压栈。
    pub fn open_screen(&mut self, state: ScreenState, input: &mut InputState) {
        self.set_screen(Some(state), input);
    }

    /// 把模态屏换成 `next`（`None` = 关掉整块屏），并把栈对齐。
    ///
    /// 三件事在这一个方法里一起做完，因此不可能只做其中两件：
    /// 1. 屏从无到有 → 压一层 `UiMode::Menu`；屏从有到无 → 把屏那一层
    ///    连同它上面的文本输入层一起弹掉。
    /// 2. 新屏要不要玩家打字（[`ScreenState::wants_text_entry`]，今天
    ///    只有命名屏返回真）→ 压/弹 `UiMode::TextEntry`。
    /// 3. 一致性断言。
    pub fn set_screen(&mut self, next: Option<ScreenState>, input: &mut InputState) {
        let had = self.screen.is_some();
        self.screen = next;
        match (had, self.screen.is_some()) {
            (false, true) => self.stack.push(UiMode::Menu, input),
            (true, false) => {
                // 屏那一层之上可能还压着文本输入层，一并弹掉。
                while matches!(self.stack.top(), Some(UiMode::TextEntry)) {
                    self.stack.pop(input);
                }
                self.pop_first(UiMode::Menu, input);
            }
            _ => {}
        }
        self.sync_text_entry(input);
        self.assert_paired();
    }

    /// 让栈顶的文本输入层与「当前这块屏要不要玩家打字」对齐。
    ///
    /// 判据来自 [`ScreenState::wants_text_entry`]。压上那一层之后输入
    /// 上下文变成 `InputContext::TextEntry`：WASD 在那张表里查不到任何
    /// 动作（打字不会让角色走起来），空格变回一个字符，事件循环同时
    /// 开启输入法并打开文本通道——**三件事共用这一个判据**。
    fn sync_text_entry(&mut self, input: &mut InputState) {
        let want = self.screen.is_some_and(ScreenState::wants_text_entry);
        let has = self.stack.top() == Some(UiMode::TextEntry);
        if want == has {
            return;
        }
        if want {
            self.stack.push(UiMode::TextEntry, input);
        } else {
            self.stack.pop(input);
        }
    }

    /// 世界地图开关（M 键），返回翻转之后是开着还是关着。
    pub fn toggle_world_map(&mut self, input: &mut InputState) -> bool {
        if self.world_map_open {
            self.close_world_map(input);
        } else {
            self.world_map_open = true;
            self.stack.push(UiMode::Overlay, input);
            self.assert_paired();
        }
        self.world_map_open
    }

    /// 关掉世界地图——取消键那一层的落点（规格 N2/N8：地图开着按 Esc
    /// 关地图，**不开菜单**）。地图本来就没开时什么都不做。
    pub fn close_world_map(&mut self, input: &mut InputState) {
        if !self.world_map_open {
            return;
        }
        self.world_map_open = false;
        self.pop_first(UiMode::Overlay, input);
        self.assert_paired();
    }

    /// 借出 `&mut PlayerMenu` 跑一段逻辑，**回来时栈一定已经对齐**。
    ///
    /// 见模块文档最后一节：不提供 `menu_mut()`，因为拿到裸引用就等于
    /// 绕过配对。`crate::player_action::player_command` 是唯一的调用方。
    ///
    /// 闭包**同时**收到 `&InputState`：`input` 在本方法里是 `&mut`
    /// （对齐那一步要它），调用方没法自己再借一份，所以由本方法转交。
    pub fn with_player_menu<R>(
        &mut self,
        input: &mut InputState,
        f: impl FnOnce(&mut PlayerMenu, &InputState) -> R,
    ) -> R {
        let was_open = self.menu.is_open();
        let result = f(&mut self.menu, input);
        match (was_open, self.menu.is_open()) {
            (false, true) => self.stack.push(UiMode::PlayerMenu, input),
            (true, false) => self.pop_first(UiMode::PlayerMenu, input),
            _ => {}
        }
        self.assert_paired();
        result
    }

    /// 把栈里**最上面那一层** `mode` 弹掉，保留它上面/下面的其余层。
    ///
    /// 不直接 `pop()`：地图那一层未必在栈顶（玩家可以先开背包再开
    /// 地图，也可以反过来），弹错层会让配对当场断掉。
    fn pop_first(&mut self, mode: UiMode, input: &mut InputState) {
        self.stack.remove_topmost(mode, input);
    }

    /// 栈里的层与三个状态字段逐条配对——本类型全部存在的意义。
    ///
    /// `debug_assert` 而不是 `assert`：配对破了是**开发期缺陷**，在
    /// 玩家机器上让整局崩掉换不来任何东西（与本仓库「一个纯 UI 状态
    /// 问题不该拖垮整局」的既有取舍一致）。测试与 debug 构建下它会红。
    fn assert_paired(&self) {
        debug_assert_eq!(
            self.stack.count(UiMode::Overlay),
            usize::from(self.world_map_open),
            "世界地图开着 ⇔ 栈里恰有一层 Overlay"
        );
        debug_assert_eq!(
            self.stack.count(UiMode::PlayerMenu),
            usize::from(self.menu.is_open()),
            "玩家菜单开着 ⇔ 栈里恰有一层 PlayerMenu"
        );
        debug_assert_eq!(
            self.stack.count(UiMode::Menu),
            usize::from(self.screen.is_some()),
            "模态屏开着 ⇔ 栈里恰有一层 Menu"
        );
        debug_assert_eq!(
            self.stack.count(UiMode::TextEntry),
            usize::from(
                self.screen
                    .is_some_and(crate::menu_screen::ScreenState::wants_text_entry)
            ),
            "命名屏 ⇔ 栈里恰有一层 TextEntry"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn_pick::SpawnOrigin;
    use ll_platform::input::GameKey;

    #[test]
    fn 开局停在首页时栈里恰有一层() {
        // Arrange & Act
        let modal = Modal::at_title();

        // Assert
        assert_eq!(modal.depth(), 1);
        assert_eq!(modal.input_context(), InputContext::Menu);
        assert!(!modal.is_empty());
    }

    #[test]
    fn 地图开关与栈严格配对() {
        // 规格 N8 判据 3 与 4：开着的时候栈深为 1（今天是 0），关掉之后
        // 栈必空。
        // Arrange
        let mut modal = Modal::in_world();
        let mut input = InputState::new();

        // Act & Assert
        assert!(modal.toggle_world_map(&mut input));
        assert_eq!(modal.depth(), 1);
        assert!(!modal.is_empty());
        assert!(!modal.toggle_world_map(&mut input));
        assert!(modal.is_empty(), "关掉之后栈必空，防 push/pop 不配对");
    }

    #[test]
    fn 玩家菜单开关与栈严格配对() {
        // Arrange
        let mut modal = Modal::in_world();
        let mut input = InputState::new();

        // Act
        modal.with_player_menu(&mut input, |menu, _input| {
            *menu = PlayerMenu::Inventory { cursor: 0 }
        });

        // Assert
        assert_eq!(modal.depth(), 1, "背包开着时栈深为 1（此前是 0）");
        assert_eq!(
            modal.input_context(),
            InputContext::Gameplay,
            "只进栈，不换键位表"
        );

        // Act：关掉
        modal.with_player_menu(&mut input, |menu, _input| *menu = PlayerMenu::Closed);

        // Assert
        assert!(modal.is_empty());
    }

    #[test]
    fn 玩家菜单换一块形态不重复压栈() {
        // 背包 → 制作是同一层里换内容，不是又盖了一层。
        // Arrange
        let mut modal = Modal::in_world();
        let mut input = InputState::new();
        modal.with_player_menu(&mut input, |menu, _input| {
            *menu = PlayerMenu::Inventory { cursor: 0 }
        });

        // Act
        modal.with_player_menu(&mut input, |menu, _input| {
            *menu = PlayerMenu::Craft { cursor: 0 }
        });

        // Assert
        assert_eq!(modal.depth(), 1);
    }

    #[test]
    fn 地图压在玩家菜单之上时关地图只弹地图那一层() {
        // 地图那一层未必在栈顶，也未必在栈底——弹错层配对当场断掉，
        // 而 `assert_paired` 会在这条测试里红。
        // Arrange
        let mut modal = Modal::in_world();
        let mut input = InputState::new();
        modal.with_player_menu(&mut input, |menu, _input| {
            *menu = PlayerMenu::Inventory { cursor: 0 }
        });
        modal.toggle_world_map(&mut input);
        assert_eq!(modal.depth(), 2);

        // Act
        modal.close_world_map(&mut input);

        // Assert
        assert_eq!(modal.depth(), 1);
        assert!(modal.player_menu().is_open(), "背包那一层不该被顺手弹掉");
    }

    #[test]
    fn 开屏关屏与栈严格配对() {
        // Arrange
        let mut modal = Modal::in_world();
        let mut input = InputState::new();

        // Act
        modal.open_screen(ScreenState::Menu, &mut input);

        // Assert
        assert_eq!(modal.depth(), 1);
        assert_eq!(modal.input_context(), InputContext::Menu);

        // Act
        modal.set_screen(None, &mut input);

        // Assert
        assert!(modal.is_empty());
        assert_eq!(modal.input_context(), InputContext::Gameplay);
    }

    #[test]
    fn 切到命名屏压一层文本输入切走再弹掉() {
        // Arrange
        let mut modal = Modal::at_title();
        let mut input = InputState::new();

        // Act
        modal.set_screen(
            Some(ScreenState::SaveNaming {
                origin: SpawnOrigin::WorldSetup,
            }),
            &mut input,
        );

        // Assert
        assert_eq!(modal.depth(), 2, "菜单层 + 文本输入层，就两层");
        assert_eq!(modal.input_context(), InputContext::TextEntry);

        // Act：切走
        modal.set_screen(Some(ScreenState::Menu), &mut input);

        // Assert
        assert_eq!(modal.depth(), 1);
        assert_eq!(modal.input_context(), InputContext::Menu);
    }

    #[test]
    fn 从命名屏直接整块关掉之后栈必空() {
        // 文本输入层压在屏那一层之上，关整块屏必须把两层一起弹掉。
        // Arrange
        let mut modal = Modal::at_title();
        let mut input = InputState::new();
        modal.set_screen(
            Some(ScreenState::SaveNaming {
                origin: SpawnOrigin::WorldSetup,
            }),
            &mut input,
        );

        // Act
        modal.set_screen(None, &mut input);

        // Assert
        assert!(modal.is_empty());
    }

    #[test]
    fn 玩家菜单那一层不清空按住的方向键() {
        // 上下文没变（前后都是 Gameplay），清键就等于把玩家正按着的
        // 方向键无故吞掉一次，见 `ll_ui::widget::ui_mode` 模块文档。
        // Arrange
        let mut modal = Modal::in_world();
        let mut input = InputState::new();
        input.press(GameKey::Up);

        // Act
        modal.with_player_menu(&mut input, |menu, _input| {
            *menu = PlayerMenu::Inventory { cursor: 0 }
        });

        // Assert
        assert!(input.is_held(GameKey::Up));
    }
}
