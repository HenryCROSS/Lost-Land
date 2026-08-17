//! 输入状态机的黑箱属性测试。
//!
//! 输入状态是个小型状态机，最容易出的错是「某个操作序列之后状态自相
//! 矛盾」。属性测试用随机操作序列轰炸它，比手写用例更容易撞出问题。

use ll_platform::input::{GameKey, InputState};
use proptest::prelude::*;

/// 对状态机施加的一次操作。
#[derive(Debug, Clone, Copy)]
enum Op {
    Press,
    Release,
    EndFrame,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![Just(Op::Press), Just(Op::Release), Just(Op::EndFrame)]
}

/// 对状态机施加一次操作。
fn apply(input: &mut InputState, key: GameKey, op: Op) {
    match op {
        Op::Press => input.press(key),
        Op::Release => input.release(key),
        Op::EndFrame => input.end_frame(),
    }
}

proptest! {
    #[test]
    fn 刚按下的判定只能由结束帧清除(ops in prop::collection::vec(op_strategy(), 0..64)) {
        // just_pressed 是「本帧内曾按下过」这一事实的记录，与当前是否按住无关。
        // 除 end_frame 外，任何操作都不得让它由真变假——否则同一帧内按下又
        // 松开的快速点击会被静默丢弃。
        // Arrange
        let mut input = InputState::new();
        let key = GameKey::Confirm;

        // Act & Assert
        for op in ops {
            let before = input.was_just_pressed(key);
            apply(&mut input, key, op);
            let after = input.was_just_pressed(key);

            if before && !after {
                prop_assert!(matches!(op, Op::EndFrame));
            }
        }
    }

    #[test]
    fn 结束帧后刚按下恒为假(ops in prop::collection::vec(op_strategy(), 0..64)) {
        // Arrange
        let mut input = InputState::new();
        let key = GameKey::Up;
        for op in ops {
            apply(&mut input, key, op);
        }

        // Act
        input.end_frame();

        // Assert
        prop_assert!(!input.was_just_pressed(key));
    }
}
