//! 焦点导航：不用鼠标也能在一组控件间移动焦点、触发当前聚焦项。
//!
//! # 为什么不能只支持鼠标
//!
//! 任务书原话「焦点导航不能只是摆设」——这条决定手柄将来能不能用，
//! 也决定无障碍可用性。本模块因此完全不读光标位置，只读
//! [`ll_platform::input::InputState`] 的方向/确认键——键盘、手柄映射
//! 到同一套 [`ll_platform::input::GameKey`]（见其模块文档），本模块
//! 与「输入来自哪种物理设备」无关，天然对两者都生效。
//!
//! # 「一组控件」从哪来——即时模式的老规矩
//!
//! 与 [`super::hit_test::hit_test`] 同一个立场：不维护一棵跨帧的控件
//! 树，调用方每帧按视觉顺序给出这一帧全部可聚焦控件的 id 列表
//! （`order`），本模块只在这份列表与
//! [`super::state::WidgetStateTable`] 之间移动一个「谁的 `focused` 为
//! 真」的标记。列表顺序即导航顺序——这是调用方（HUD 布局代码）已经
//! 知道、且每帧都要重新算一遍的信息，不需要再教一遍给本模块。

use super::state::{WidgetId, WidgetStateTable};
use ll_platform::input::{GameKey, InputState};

/// 焦点移动方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    /// 移到 `order` 中的下一个（方向键右/下）。
    Next,
    /// 移到 `order` 中的上一个（方向键左/上）。
    Prev,
}

/// `order` 中当前持有焦点的控件——最多一个，找不到（没有任何一个
/// `focused` 为真，或 `order` 里没有一个 id 在表里登记过）时返回
/// `None`。
pub fn focused_widget(table: &WidgetStateTable, order: &[WidgetId]) -> Option<WidgetId> {
    order
        .iter()
        .copied()
        .find(|id| table.get(id).is_some_and(|state| state.focused))
}

/// 把焦点移到 `order` 中的下一个/上一个,并把结果写回 `table`——同一
/// 时刻只有新的目标控件 `focused` 为真,原先持有焦点的（如果有）被
/// 一并清掉,保证「至多一个控件聚焦」这条不变式。
///
/// `order` 为空时是没有意义的操作,返回 `None`,不修改 `table`。
///
/// # 起点：没有任何控件聚焦时
///
/// `Next` 从第一个开始,`Prev` 从最后一个开始——这是多数菜单系统的
/// 惯例：玩家第一次按下方向键就该看到焦点出现在一个确定的位置,而
/// 不是要求玩家先按一次「随便哪个方向」去猜起点在哪。
///
/// # 到达两端后循环
///
/// 到达 `order` 末尾再按 `Next` 回到开头,反之亦然——回合制菜单的
/// 惯例手感,与本项目其余「按住方向键连续移动」的键盘手感一致（不会
/// 因为到了列表边界就停手不动,需要玩家换一个方向键才能继续）。
pub fn move_focus(
    table: &mut WidgetStateTable,
    order: &[WidgetId],
    direction: FocusDirection,
) -> Option<WidgetId> {
    if order.is_empty() {
        return None;
    }
    let current_index = order
        .iter()
        .position(|id| table.get(id).is_some_and(|state| state.focused));
    let next_index = match (current_index, direction) {
        (None, FocusDirection::Next) => 0,
        (None, FocusDirection::Prev) => order.len() - 1,
        (Some(i), FocusDirection::Next) => (i + 1) % order.len(),
        (Some(i), FocusDirection::Prev) => (i + order.len() - 1) % order.len(),
    };
    for (i, id) in order.iter().enumerate() {
        table.entry(id).focused = i == next_index;
    }
    Some(order[next_index])
}

/// 每帧读一次 `input`，若方向键本帧激活（`was_activated`，支持长按
/// 连续移动，与方向键在 `Gameplay` 上下文下驱动移动是同一条自动重复
/// 机制，见 [`ll_platform::input::InputState::was_activated`] 文档），
/// 就据此移动焦点；否则原样返回当前聚焦项，不做任何改动。
///
/// 上/左归为 [`FocusDirection::Prev`]、下/右归为
/// [`FocusDirection::Next`]——覆盖纵向列表（大多数菜单）与横向排列
/// （例如一排工具栏按钮）两种常见布局，调用方若只用得到其中一个轴，
/// 简单地不把另一轴的方向键接进这个函数的调用点即可（本函数不区分
/// 布局是纵向还是横向,那是调用方的布局知识）。
pub fn navigate_focus(
    table: &mut WidgetStateTable,
    order: &[WidgetId],
    input: &InputState,
) -> Option<WidgetId> {
    if input.was_activated(GameKey::Down) || input.was_activated(GameKey::Right) {
        return move_focus(table, order, FocusDirection::Next);
    }
    if input.was_activated(GameKey::Up) || input.was_activated(GameKey::Left) {
        return move_focus(table, order, FocusDirection::Prev);
    }
    focused_widget(table, order)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空列表移动焦点返回空值() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order: [WidgetId; 0] = [];

        // Act
        let focused = move_focus(&mut table, &order, FocusDirection::Next);

        // Assert
        assert_eq!(focused, None);
    }

    #[test]
    fn 无人聚焦时向下移动落在第一项() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b", "c"];

        // Act
        let focused = move_focus(&mut table, &order, FocusDirection::Next);

        // Assert
        assert_eq!(focused, Some("a"));
    }

    #[test]
    fn 无人聚焦时向上移动落在最后一项() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b", "c"];

        // Act
        let focused = move_focus(&mut table, &order, FocusDirection::Prev);

        // Assert
        assert_eq!(focused, Some("c"));
    }

    #[test]
    fn 连续向下移动依次经过每一项() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b", "c"];
        move_focus(&mut table, &order, FocusDirection::Next);

        // Act
        let focused = move_focus(&mut table, &order, FocusDirection::Next);

        // Assert
        assert_eq!(focused, Some("b"));
    }

    #[test]
    fn 到达末尾后再向下移动回到开头() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b", "c"];
        move_focus(&mut table, &order, FocusDirection::Prev); // 落在 c

        // Act
        let focused = move_focus(&mut table, &order, FocusDirection::Next);

        // Assert
        assert_eq!(focused, Some("a"));
    }

    #[test]
    fn 到达开头后再向上移动回到末尾() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b", "c"];
        move_focus(&mut table, &order, FocusDirection::Next); // 落在 a

        // Act
        let focused = move_focus(&mut table, &order, FocusDirection::Prev);

        // Assert
        assert_eq!(focused, Some("c"));
    }

    #[test]
    fn 移动焦点后旧的聚焦项不再聚焦() {
        // 至多一个控件聚焦这条不变式的直接验证。
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b", "c"];
        move_focus(&mut table, &order, FocusDirection::Next); // 落在 a

        // Act
        move_focus(&mut table, &order, FocusDirection::Next); // 落在 b

        // Assert
        assert!(!table.get("a").expect("a 已经登记过状态").focused);
    }

    #[test]
    fn focused_widget能查到当前聚焦项() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b"];
        move_focus(&mut table, &order, FocusDirection::Next);

        // Act
        let focused = focused_widget(&table, &order);

        // Assert
        assert_eq!(focused, Some("a"));
    }

    #[test]
    fn 没有任何控件聚焦时focused_widget返回空值() {
        // Arrange
        let table = WidgetStateTable::new();
        let order = ["a", "b"];

        // Act
        let focused = focused_widget(&table, &order);

        // Assert
        assert_eq!(focused, None);
    }

    #[test]
    fn 向下方向键激活时导航移到下一项() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b"];
        let mut input = InputState::new();
        input.press(GameKey::Down);

        // Act
        let focused = navigate_focus(&mut table, &order, &input);

        // Assert
        assert_eq!(focused, Some("a"));
    }

    #[test]
    fn 没有方向键激活时导航不改变当前聚焦项() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b"];
        move_focus(&mut table, &order, FocusDirection::Next); // 落在 a
        let input = InputState::new();

        // Act
        let focused = navigate_focus(&mut table, &order, &input);

        // Assert
        assert_eq!(focused, Some("a"));
    }

    #[test]
    fn 左方向键激活时导航等价于向上移动() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let order = ["a", "b", "c"];
        move_focus(&mut table, &order, FocusDirection::Next); // 落在 a
        let mut input = InputState::new();
        input.press(GameKey::Left);

        // Act
        let focused = navigate_focus(&mut table, &order, &input);

        // Assert：从 a 向上移动应该回绕到最后一项 c。
        assert_eq!(focused, Some("c"));
    }
}
