//! 鼠标点在模态屏的第几行上——**四条新约定的落点**。
//!
//! # 这个模块补的是哪条断线
//!
//! 项目所有者 2026-08-29 的原话：「我觉得应该要预选一项，同时我希望能
//! 有按钮选项，鼠标点击也有反应。」前半是规格 N10（见
//! `crate::app::Demo::open_menu` 的文档），后半是本模块。
//!
//! 地基 P7 那一批就全部落地了——`ll_ui::widget::hit_test`、
//! `ll_platform::input::InputState` 的光标与鼠标键、
//! `ll_ui::widget::button::update_button`——**只是新加的那批屏一块都
//! 没接上**。本模块接的是模态屏那一列行；世界地图上的选点那一处早已
//! 接上（`crate::app::Demo::clicked_spawn_zone`）。
//!
//! # 四条约定（交接文档第〇之二节「其余记在案的债」点名要一并定）
//!
//! 原文：「接鼠标不只是『加个点击判定』，它会引出『焦点由键盘还是指针
//! 驱动』『hover 要不要改变焦点』这类新约定，落地批次要一并定。」
//!
//! ## 一、指针**悬停**不改变键盘焦点
//!
//! 鼠标划过一行只把它记进 [`PointerState::hovered_row`]（画一块淡一档的
//! 高亮），**不动焦点**。
//!
//! 反面那条做法（hover 即聚焦）会让「键盘走到第 3 项、手不小心碰了下
//! 鼠标、焦点跳回第 1 项」——玩家没做任何操作，选中项却变了。
//!
//! 而它的代价「高亮在 A、点击生效在 B」由约定三堵住：**按下**那一刻
//! 焦点就跳到指针所在行，玩家在触发之前一定先看见高亮跟了过去。
//!
//! 仓库既有的形状也支持这条分工：`ll_ui::widget::state::WidgetState`
//! 的 `hovered` 与 `focused` 本来就是两个独立字段，
//! `ll_ui::widget::button` 模块文档「焦点也要有可见反馈」一节论证过
//! 「悬停」与「聚焦」在纯键盘操作下必须是两件事。
//!
//! ## 二、点在没有条目的空白上：**什么都不做**
//!
//! 不改焦点、不触发、不关屏。
//!
//! 「点空白关掉这块屏」是常见的另一种做法，**这里刻意不选**：它会在
//! 规格 N2（取消键只退一层）之外多出第二条退层路径，而在角色创建 /
//! 世界配置这类多步流程上误关一层的代价是玩家白填一遍。这条也是最容易
//! 反转的——反转它只需要在 [`resolve_row_pointer`] 里给 `None` 那一支
//! 加一个返回值。
//!
//! ## 三、点中的正好是已经聚焦的那一项：**照样确认**
//!
//! 与点别的项完全一样。「按下 = 聚焦，松开 = 确认」是同一个手势的两半，
//! 拆不开；若已聚焦的那项要点两下才生效，玩家的行为就取决于一条他看不
//! 见的历史状态（上一次焦点停在哪）。
//!
//! ## 四、拖动：**按下与松开必须落在同一行**
//!
//! 按下在 A、松开在 B 或空白 → 不触发，焦点留在按下时的 A。这不是新
//! 发明的手感——`ll_ui::widget::button::update_button` 早就是这么做的
//! （模块文档「按下与触发」一节：「按下时武装，松开时若仍悬停在同一个
//! 控件上才算一次点击」），本模块只是把同一条规则用在「行」这种没有
//! 静态控件 id 的东西上。
//!
//! # 为什么不是给每一行发一个 `WidgetId`
//!
//! `ll_ui::widget::hit_test` 模块文档已经定过调：即时模式，不维护跨帧
//! 控件树，「这一帧点的是哪一个」由「这一帧算出的第几个 `Rect` 包含
//! 点击坐标」现算得出。模态屏的行数每帧现算（设置屏二十几行、暂停菜单
//! 四到五行随模式变），发静态 id 就要先有一张静态清单，那张清单与
//! `rows` 会是两份迟早分叉的同一个东西。行的 id 因此就是**第几行**，
//! 走的是同一个 `hit_test`（本批把它泛型化了）。
//!
//! # 测得到与测不到（ADR 0025）
//!
//! **不合成任何操作系统级事件。** [`resolve_row_pointer`] 是纯函数，
//! 输入是直接构造的 `InputState`（`set_cursor_position`/`mouse_press`/
//! `mouse_release` 是既有公开构造 API，`ll_platform::input` 自己的单元
//! 测试就在用）——这正是 ADR 0025 要求的「程序化驱动同一条调用路径」，
//! 与它禁止的「系统级按键注入」是两件事。
//!
//! **测不到的一段如实记录**：winit 的 `CursorMoved`/`MouseInput` 事件
//! 到 `InputState` 那几个 setter 之间的平台回调，以及「窗口坐标与
//! `Rect` 坐标系是不是真的同一套」——前者要真实事件循环，后者要真实
//! 窗口。本模块测不到它们。

use ll_platform::input::{InputState, MouseButton};
use ll_ui::widget::geometry::Rect;
use ll_ui::widget::hit_test::hit_test;

/// 指针这一帧对模态屏的行做了什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowPointer {
    /// 什么都没做——绝大多数帧都是这一支。
    #[default]
    Idle,
    /// 把焦点移到第几行（左键按下的那一刻），但**还没有**触发它。
    Focus(usize),
    /// 触发第几行——等价于玩家在这一行上按了确认键。
    Activate(usize),
}

impl RowPointer {
    /// 这一帧指针指定了焦点该落在第几行；`None` 表示焦点不动。
    ///
    /// [`RowPointer::Activate`] 也算：松开触发的那一行同时就是焦点该
    /// 落的那一行（约定三——点哪一行，哪一行就既被选中又被触发）。
    pub fn focus_row(self) -> Option<usize> {
        match self {
            RowPointer::Idle => None,
            RowPointer::Focus(row) | RowPointer::Activate(row) => Some(row),
        }
    }

    /// 这一帧指针是不是要求「确认」——调用方把它与
    /// `input.was_just_pressed(GameKey::Confirm)` 并起来，**两条路径走
    /// 同一个分支**，不为鼠标另写一套动作分派。
    pub fn activated(self) -> bool {
        matches!(self, RowPointer::Activate(_))
    }
}

/// 指针在行列表上的跨帧状态。
///
/// 纯表现层，与 `crate::app::Demo::screen_focus`/`hud_anim` 同一条纪律
/// （`ll_ui::widget::state` 模块文档「为什么是旁表」）：不进
/// `WorldState`、不进存档、不参与回放。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PointerState {
    /// 左键**按下**时落在第几行——约定四要的那条跨帧记忆（「这次按下
    /// 是不是从这一行开始的」）。松开时清掉。
    armed_row: Option<usize>,
    /// 这一刻指针悬停在第几行——只用来画那块淡高亮，**不改焦点**
    /// （约定一）。
    hovered_row: Option<usize>,
}

impl PointerState {
    /// 这一刻指针悬停在第几行，供渲染侧画高亮。
    pub fn hovered_row(self) -> Option<usize> {
        self.hovered_row
    }
}

/// 这一帧指针对行列表做了什么，并把跨帧状态推进一格。
///
/// `row_rects` 由 `ll_ui::screen::screen_row_rects` 现算——行的几何只有
/// 那一个产出点，渲染侧画的是同一份。
///
/// 四条约定逐条落在这里，读法见模块文档。
pub fn resolve_row_pointer(
    state: &mut PointerState,
    input: &InputState,
    row_rects: &[Rect],
) -> RowPointer {
    // 约定一：悬停只记下来，不产出任何 `RowPointer`。
    let hit = input
        .cursor_position()
        .and_then(|point| hit_test(point, row_rects.iter().copied().enumerate()));
    state.hovered_row = hit;

    let mut outcome = RowPointer::Idle;
    // 按下：武装，并把焦点挪过去（约定三的前半）。
    // 约定二：`hit` 为 `None`（点在空白上）时 `armed_row` 也变成 `None`
    // ——焦点不动，且之后无论在哪儿松开都不会触发。
    if input.was_mouse_just_pressed(MouseButton::Left) {
        state.armed_row = hit;
        if let Some(row) = hit {
            outcome = RowPointer::Focus(row);
        }
    }
    // 松开：只有「按下与松开落在同一行」才算一次点击（约定四）。
    // 与 `if` 而不是 `else if`：`InputState` 的两个一次性标志都要到
    // `end_frame` 才清（见其文档），一次完整的点击可能整个发生在两帧
    // 之间，那一帧里按下与松开同时为真——那也是一次合法的点击。
    if input.was_mouse_just_released(MouseButton::Left) {
        let armed = state.armed_row.take();
        if let (Some(armed), Some(hit)) = (armed, hit)
            && armed == hit
        {
            outcome = RowPointer::Activate(armed);
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 三行() -> Vec<Rect> {
        vec![
            Rect::new(0.0, 0.0, 100.0, 20.0),
            Rect::new(0.0, 20.0, 100.0, 20.0),
            Rect::new(0.0, 40.0, 100.0, 20.0),
        ]
    }

    /// 把指针放到第 `row` 行的正中。
    fn 指到(input: &mut InputState, row: usize) {
        input.set_cursor_position((50.0, row as f32 * 20.0 + 10.0));
    }

    #[test]
    fn 悬停不改焦点也不触发() {
        // 约定一：划过去只记 hover，不产出任何动作。
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        指到(&mut input, 1);

        // Act
        let outcome = resolve_row_pointer(&mut state, &input, &三行());

        // Assert
        assert_eq!(outcome, RowPointer::Idle);
        assert_eq!(outcome.focus_row(), None, "焦点不该被悬停挪走");
        assert_eq!(state.hovered_row(), Some(1), "但高亮要跟着走");
    }

    #[test]
    fn 按下把焦点挪到指针那一行但还不触发() {
        // 约定三的前半：玩家在触发之前一定先看见高亮跟了过去。
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        指到(&mut input, 2);
        input.mouse_press(MouseButton::Left);

        // Act
        let outcome = resolve_row_pointer(&mut state, &input, &三行());

        // Assert
        assert_eq!(outcome, RowPointer::Focus(2));
        assert!(!outcome.activated());
    }

    #[test]
    fn 按下再松开在同一行算一次确认() {
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        指到(&mut input, 1);
        input.mouse_press(MouseButton::Left);
        resolve_row_pointer(&mut state, &input, &三行());

        // Act：下一帧松开，指针没动
        input.end_frame();
        input.mouse_release(MouseButton::Left);
        let outcome = resolve_row_pointer(&mut state, &input, &三行());

        // Assert
        assert_eq!(outcome, RowPointer::Activate(1));
        assert!(outcome.activated());
        assert_eq!(outcome.focus_row(), Some(1));
    }

    #[test]
    fn 按下第零行松开第二行不触发任何一行() {
        // 约定四：拖出去再松手是取消，不是「触发松手那一行」，也不是
        // 「触发按下那一行」。
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        指到(&mut input, 0);
        input.mouse_press(MouseButton::Left);
        let pressed = resolve_row_pointer(&mut state, &input, &三行());
        assert_eq!(pressed, RowPointer::Focus(0));

        // Act：拖到第二行才松手
        input.end_frame();
        指到(&mut input, 2);
        input.mouse_release(MouseButton::Left);
        let outcome = resolve_row_pointer(&mut state, &input, &三行());

        // Assert
        assert_eq!(outcome, RowPointer::Idle);
    }

    #[test]
    fn 按在行上拖到空白松手同样不触发() {
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        指到(&mut input, 0);
        input.mouse_press(MouseButton::Left);
        resolve_row_pointer(&mut state, &input, &三行());

        // Act
        input.end_frame();
        input.set_cursor_position((500.0, 500.0));
        input.mouse_release(MouseButton::Left);
        let outcome = resolve_row_pointer(&mut state, &input, &三行());

        // Assert
        assert_eq!(outcome, RowPointer::Idle);
        assert_eq!(state.hovered_row(), None);
    }

    #[test]
    fn 点在空白上什么都不做() {
        // 约定二：不改焦点、不触发、不关屏（本函数产不出「关屏」这种
        // 结果，那正是这条约定的结构性表达）。
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        input.set_cursor_position((500.0, 500.0));
        input.mouse_press(MouseButton::Left);
        input.mouse_release(MouseButton::Left);

        // Act
        let outcome = resolve_row_pointer(&mut state, &input, &三行());

        // Assert
        assert_eq!(outcome, RowPointer::Idle);
        assert_eq!(outcome.focus_row(), None);
    }

    #[test]
    fn 同一帧内按下并松开算一次完整点击() {
        // `InputState` 的两个一次性标志都要到 `end_frame` 才清，一次
        // 快点击可能整个落在两帧之间。
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        指到(&mut input, 2);
        input.mouse_press(MouseButton::Left);
        input.mouse_release(MouseButton::Left);

        // Act
        let outcome = resolve_row_pointer(&mut state, &input, &三行());

        // Assert
        assert_eq!(outcome, RowPointer::Activate(2));
    }

    #[test]
    fn 光标不在窗口里时一律不动() {
        // `cursor_position` 为 `None`（光标离开了窗口）——命中测试没有
        // 坐标可用，见 `InputState::clear_cursor_position` 文档。
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        input.mouse_press(MouseButton::Left);

        // Act
        let outcome = resolve_row_pointer(&mut state, &input, &三行());

        // Assert
        assert_eq!(outcome, RowPointer::Idle);
        assert_eq!(state.hovered_row(), None);
    }

    #[test]
    fn 行列表为空时点哪儿都不触发() {
        // 列表为空的屏（没有存档时的存档列表）只显示一行占位文字，那
        // 一行不是按钮，见 `ll_ui::screen::ScreenContent::row_rects`。
        // Arrange
        let mut state = PointerState::default();
        let mut input = InputState::new();
        指到(&mut input, 0);
        input.mouse_press(MouseButton::Left);
        input.mouse_release(MouseButton::Left);

        // Act
        let outcome = resolve_row_pointer(&mut state, &input, &[]);

        // Assert
        assert_eq!(outcome, RowPointer::Idle);
    }
}
