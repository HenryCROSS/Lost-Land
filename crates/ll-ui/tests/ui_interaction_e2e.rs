//! UI 交互层端到端验收：直接驱动 `ll_platform::input::InputState` 的
//! 真实公开方法（`set_cursor_position`/`mouse_press`/`mouse_release`/
//! `press`），走完整的调用路径——命中测试 → 按钮状态机 → 触发——而不是
//! 只在孤立的纯函数上喂手造的参数。
//!
//! # 为什么是这种形状，不是合成键盘事件
//!
//! [ADR 0025](../../../knowledge/decisions/0025-demo-interaction-verification-forbids-sendkeys.md)
//! 禁的是**操作系统级的合成按键注入**（`SendKeys` 一类）——那类手段会
//! 把按键送去前台窗口，在自动化环境里可能锁定在错误的窗口上，曾经把
//! 按键泄漏进宿主聊天窗口。ADR 0025 明确允许、且推荐「程序化驱动同一
//! 调用路径」：真实鼠标点击最终也是 winit 把 `CursorMoved`/`MouseInput`
//! 事件喂给 `InputState` 的同一批方法（见
//! `crates/ll-platform/src/window.rs` 的 `WindowEvent::CursorMoved`/
//! `WindowEvent::MouseInput` 分支），本文件直接调用这些方法，就是在
//! 进程内调用自己的输入处理函数，不是合成系统事件、也不会有任何按键
//! 泄漏到宿主窗口的风险。
//!
//! 本文件因此**不是**「只测纯函数」的退化验收——它真实覆盖了
//! `InputState` → `hit_test` → `update_button`/`navigate_focus` 这条
//! 完整链路，与真实点击/真实按键唯一的区别只是「谁触发了
//! `set_cursor_position`/`press`」（真实场景是 winit 事件循环，这里是
//! 测试代码），链路本身完全相同。

use ll_platform::input::{GameKey, InputState, MouseButton};
use ll_ui::widget::button::update_button;
use ll_ui::widget::focus::navigate_focus;
use ll_ui::widget::geometry::Rect;
use ll_ui::widget::hit_test::hit_test;
use ll_ui::widget::skin::ButtonVisualState;
use ll_ui::widget::state::WidgetStateTable;

/// 三个测试按钮的矩形——刻意不重叠，方便用坐标区分点中了哪一个。
fn sample_buttons() -> [(&'static str, Rect); 3] {
    [
        ("panel.button_a", Rect::new(0.0, 0.0, 100.0, 40.0)),
        ("panel.button_b", Rect::new(0.0, 50.0, 100.0, 40.0)),
        ("panel.button_c", Rect::new(0.0, 100.0, 100.0, 40.0)),
    ]
}

#[test]
fn 鼠标点击命中测试后按钮真正触发() {
    // Arrange：喂给 InputState 一次真实的「移动到按钮 B 上方并按下」。
    let mut input = InputState::new();
    let mut table = WidgetStateTable::new();
    let buttons = sample_buttons();

    input.set_cursor_position((50.0, 70.0)); // 落在 button_b 范围内
    let hit = hit_test(
        input.cursor_position().expect("已经设置过光标位置"),
        buttons,
    );
    assert_eq!(
        hit,
        Some("panel.button_b"),
        "命中测试应先确认光标真的落在 button_b 上，这是后续按钮判定的前提"
    );

    input.mouse_press(MouseButton::Left);
    let (_, rect_b) = buttons[1];
    let pressed_outcome = update_button(&mut table, "panel.button_b", rect_b, &input, true);
    assert_eq!(pressed_outcome.visual, ButtonVisualState::Pressed);
    assert!(!pressed_outcome.triggered, "按下的瞬间还不该算触发");

    input.end_frame();
    input.mouse_release(MouseButton::Left);

    // Act：松开时光标仍停留在 button_b 上——完整点击序列的最后一步。
    let released_outcome = update_button(&mut table, "panel.button_b", rect_b, &input, true);

    // Assert
    assert!(
        released_outcome.triggered,
        "按下与松开都命中同一个按钮，应当真正触发"
    );
}

#[test]
fn 鼠标点击落在另一个按钮范围内不会触发未命中的按钮() {
    // 证明命中测试真的在起作用，不是随便哪个按钮都会响应——按在
    // button_a 上，只有 button_a 该触发，button_b 不该触发。
    // Arrange
    let mut input = InputState::new();
    let mut table = WidgetStateTable::new();
    let buttons = sample_buttons();

    input.set_cursor_position((50.0, 20.0)); // 落在 button_a 范围内
    input.mouse_press(MouseButton::Left);
    let (_, rect_a) = buttons[0];
    let (_, rect_b) = buttons[1];
    update_button(&mut table, "panel.button_a", rect_a, &input, true);
    update_button(&mut table, "panel.button_b", rect_b, &input, true);
    input.end_frame();
    input.mouse_release(MouseButton::Left);

    // Act
    let outcome_a = update_button(&mut table, "panel.button_a", rect_a, &input, true);
    let outcome_b = update_button(&mut table, "panel.button_b", rect_b, &input, true);

    // Assert
    assert!(outcome_a.triggered, "光标真正落在 button_a 上，应当触发");
    assert!(!outcome_b.triggered, "从未落在 button_b 范围内，不该触发");
}

#[test]
fn 纯键盘在多个按钮间移动焦点并触发确认() {
    // 全程不调用任何鼠标/光标相关方法，证明键盘（未来手柄同理，两者
    // 共用同一套 GameKey）单独也能完整操作一组按钮——这条决定手柄
    // 将来能不能用，也决定无障碍可用性。
    // Arrange
    let mut input = InputState::new();
    let mut table = WidgetStateTable::new();
    let order = ["panel.button_a", "panel.button_b", "panel.button_c"];
    let buttons = sample_buttons();

    // Act 1：第一次按下 Down，焦点应当落在第一项。
    input.press(GameKey::Down);
    let focused_first = navigate_focus(&mut table, &order, &input);
    assert_eq!(focused_first, Some("panel.button_a"));
    input.end_frame();
    input.release(GameKey::Down);

    // Act 2：再按一次 Down，焦点移到第二项。
    input.press(GameKey::Down);
    let focused_second = navigate_focus(&mut table, &order, &input);
    assert_eq!(focused_second, Some("panel.button_b"));
    input.end_frame();
    input.release(GameKey::Down);

    // Act 3：按下确认键，触发当前聚焦的 button_b——全程没有设置过
    // 光标位置，`update_button` 内部的 `cursor_position()` 恒为
    // `None`，证明触发确实是走键盘路径,不是意外命中了鼠标判定。
    assert_eq!(input.cursor_position(), None);
    input.press(GameKey::Confirm);
    let (_, rect_b) = buttons[1];
    let outcome = update_button(&mut table, "panel.button_b", rect_b, &input, true);

    // Assert：键盘确认键没有"物理按住"这个概念,视觉状态停在 Hovered
    // （焦点的可见反馈,见 `update_button` 文档「焦点也要有可见反馈」
    // 一节）,不会像鼠标点击那样先经过 Pressed。
    assert!(outcome.triggered, "聚焦的按钮按下确认键应当触发");
    assert_eq!(outcome.visual, ButtonVisualState::Hovered);

    // Assert：未聚焦的按钮即便同一帧按了确认键也不该触发。
    let (_, rect_a) = buttons[0];
    let outcome_a = update_button(&mut table, "panel.button_a", rect_a, &input, true);
    assert!(!outcome_a.triggered, "焦点不在 button_a 上，不该触发");
}
