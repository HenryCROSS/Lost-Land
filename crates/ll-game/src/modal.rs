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
//!
//! # 深度上限为什么也落在这里（规格 N4）
//!
//! 规格 `knowledge/design/ui-and-navigation.md` N4 的判据原文写的是
//! 「`UiModeStack::push` 在深度已达 3 时 `debug_assert!` 并拒绝压入」。
//! **本批（批次 23）把它落在本类型，不落在 `UiModeStack`**，理由是规格
//! 自己在十节 P0 表下面已经记过的那一条：
//!
//! > 「此刻加一条『超 3 就拒绝压入』会让 `Modal` 的配对不变式当场被
//! > 自己破坏。」
//!
//! 那条顾虑成立，而且它同时指出了正确落点。`UiModeStack` 只持有栈，
//! 它拒绝得了压栈、拒绝不了 [`Modal::world_map_open`] 那半个字段的
//! 改动——于是「栈少一层、字段却已经翻转」，[`Modal::assert_paired`]
//! 下一行就红。**拒绝必须是原子的：状态字段与栈一起不动**，而只有
//! 同时持有两者的本类型做得到。
//!
//! 上限值 3 的理由照抄规格：设置屏 → 键位捕获已经是 2 层，再加一个
//! 确认框就是 3；**第 4 层意味着流程设计出了问题，应当在开发期就红，
//! 而不是在玩家那里变成一堆退不完的屏**。

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
    /// 模态栈允许的最大深度——规格 N4，见模块文档「深度上限为什么也
    /// 落在这里」一节。
    ///
    /// 超过它的那一次开屏/开菜单/开地图**整个被拒绝**：状态字段与栈
    /// 一起不动，因此配对不变式照旧成立。
    pub const MAX_DEPTH: usize = 3;

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

    /// 这一块屏自己要占几层——屏那一层，加上「它要不要玩家打字」那一层。
    ///
    /// [`Modal::set_screen`] 的层数变化不是无脑 ±1：从菜单屏换到命名屏
    /// 是 1 → 2，关掉命名屏是 2 → 0。上限判定必须按这个数算，否则
    /// **换屏与关屏也会被上限挡住**，而那两件事根本不加层。
    fn screen_layers(screen: Option<ScreenState>) -> usize {
        match screen {
            None => 0,
            Some(state) => 1 + usize::from(state.wants_text_entry()),
        }
    }

    /// 这一次改动之后总层数会是多少，超了没有——规格 N4 的唯一判据。
    ///
    /// `replaced`（本次替换掉的层数）/ `needed`（本次要占的层数） 一起收：地图与玩家菜单
    /// 各是 0 → 1，模态屏那一路则可能是 1 → 2 或 2 → 0，见
    /// [`Modal::screen_layers`]。
    fn would_exceed(&self, replaced: usize, needed: usize) -> bool {
        let others = self.stack.depth().saturating_sub(replaced);
        others + needed > Modal::MAX_DEPTH
    }

    /// 上限拦下一次压入时统一走这里。**两种构建下是两种行为，都是
    /// 规格 N4 要的**：
    ///
    /// - **开发期（debug，含 `cargo test`）当场红**——规格原文「第 4 层
    ///   意味着流程设计出了问题，应当在开发期就红」。
    /// - **发布构建下安静地拒绝**——规格原文的后半句「而不是在玩家那里
    ///   变成一堆退不完的屏」。`debug_assert` 而不是 `assert`，与
    ///   [`Modal::assert_paired`] 同一条既有取舍：一个纯 UI 状态问题
    ///   不该在玩家机器上拖垮整局。
    ///
    /// 因此「拒绝」这一半在 debug 下**观察不到**（那一刻已经 panic 了）。
    /// 守它的判据是 [`Modal::would_exceed`] 这个纯判据本身，见本模块
    /// 测试里那两条。
    fn reject_push(what: &str) {
        debug_assert!(
            false,
            "模态栈已经 {} 层，再压一层就是第 {} 层（{what}）：规格 N4",
            Modal::MAX_DEPTH,
            Modal::MAX_DEPTH + 1
        );
        tracing::warn!(
            max_depth = Modal::MAX_DEPTH,
            what,
            "模态栈已达深度上限，这一次压入被拒绝"
        );
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
        // 规格 N4：超上限就整个拒绝——`self.screen` 一个字节都还没改，
        // 栈也没动过，配对因此不可能破。**这一句必须在任何赋值之前。**
        if self.would_exceed(
            Modal::screen_layers(self.screen),
            Modal::screen_layers(next),
        ) {
            Modal::reject_push("再开一块模态屏");
            return;
        }
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
            if self.would_exceed(0, 1) {
                Modal::reject_push("再开世界地图");
                return false;
            }
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
        let menu_before = self.menu;
        let result = f(&mut self.menu, input);
        match (was_open, self.menu.is_open()) {
            (false, true) if self.would_exceed(0, 1) => {
                // 闭包已经把 `menu` 改掉了才知道会超——**整个还原**，
                // 状态与栈一起不动。`PlayerMenu` 是 `Copy`，还原是一次
                // 赋值，不需要闭包自己知道有上限这回事。
                Modal::reject_push("再开一块玩家菜单");
                self.menu = menu_before;
            }
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

    /// 把栈叠到恰好 [`Modal::MAX_DEPTH`] 层：地图 + 玩家菜单 + 模态屏。
    ///
    /// 这三样是今天**真的能同时存在**的三层（规格十节 P0 表下面那一段
    /// 点名的组合），不是为了凑数造出来的。
    fn 叠到上限() -> (Modal, InputState) {
        let mut modal = Modal::in_world();
        let mut input = InputState::new();
        modal.toggle_world_map(&mut input);
        modal.with_player_menu(&mut input, |menu, _input| {
            *menu = PlayerMenu::Inventory { cursor: 0 }
        });
        modal.open_screen(ScreenState::Menu, &mut input);
        assert_eq!(modal.depth(), Modal::MAX_DEPTH, "Arrange：恰好叠到上限");
        (modal, input)
    }

    #[test]
    fn 深度上限的判据在第四层为真在第三层为假() {
        // 规格 N4 的算术本体。它是纯判据，不 panic——「拒绝」那一半在
        // debug 构建下观察不到（`reject_push` 当场 panic），因此这条
        // 断言守的就是判据自己。
        //
        // 反例（已实跑）：把 `would_exceed` 改成恒 `false`，本条当场红，
        // 红在「叠满之后再要一层仍然被判为不超」这一句。
        // Arrange
        let (modal, _input) = 叠到上限();

        // Act & Assert
        assert!(
            modal.would_exceed(0, 1),
            "已经 {} 层，再要一层就是第 {} 层，必须判为超限",
            Modal::MAX_DEPTH,
            Modal::MAX_DEPTH + 1
        );
        assert!(
            !modal.would_exceed(1, 1),
            "换掉一层再占一层，总数不变，不该被上限挡住"
        );
        assert!(!modal.would_exceed(1, 0), "关掉一层永远不该被上限挡住");
    }

    #[test]
    #[should_panic(expected = "规格 N4")]
    fn 叠到第四层当场被拒() {
        // 规格 N4 判据：深度已达上限时**拒绝压入**。debug 构建（含
        // `cargo test`）下这一拒绝表现为 `debug_assert` 当场红，正是
        // 规格要的「应当在开发期就红」。
        //
        // 反例（已实跑）：把 `would_exceed` 改成恒 `false`，本条不再
        // panic 而是安静地叠到第 4 层，`#[should_panic]` 因此失败——
        // 红的原因确实是「第 4 层压进去了」。
        // Arrange
        let (mut modal, mut input) = 叠到上限();

        // Act：命名屏要屏那一层 + 文本输入层，是最容易撞上限的一路。
        modal.set_screen(
            Some(ScreenState::SaveNaming {
                origin: SpawnOrigin::WorldSetup,
            }),
            &mut input,
        );
    }

    #[test]
    fn 上限之内的换屏与关屏一概不受影响() {
        // 上限判据按「换完之后一共几层」算，不是无脑 +1——按 +1 算的话
        // 叠满之后连**关屏**都会被挡住，那是把一条防护做成了死锁。
        // Arrange
        let (mut modal, mut input) = 叠到上限();

        // Act：同深度换一块屏
        modal.set_screen(Some(ScreenState::Title), &mut input);

        // Assert
        assert_eq!(modal.screen(), Some(ScreenState::Title));
        assert_eq!(modal.depth(), Modal::MAX_DEPTH);

        // Act：关掉屏那一层
        modal.set_screen(None, &mut input);

        // Assert
        assert_eq!(modal.depth(), Modal::MAX_DEPTH - 1);
        assert!(modal.world_map_open() && modal.player_menu().is_open());
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
