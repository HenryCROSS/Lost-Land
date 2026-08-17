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
    fn 刚按下为真时必然处于按住状态(ops in prop::collection::vec(op_strategy(), 0..64)) {
        // 「刚按下但没按住」是自相矛盾的状态，任何操作序列都不该产生它。
        // Arrange
        let mut input = InputState::new();
        let key = GameKey::Confirm;

        // Act & Assert
        for op in ops {
            apply(&mut input, key, op);
            if input.was_just_pressed(key) {
                prop_assert!(input.is_held(key));
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
