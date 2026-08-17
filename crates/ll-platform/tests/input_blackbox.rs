//! 输入状态机的黑箱属性测试。
//!
//! 输入状态是个小型状态机，最容易出的错是「某个操作序列之后状态自相
//! 矛盾」。属性测试用随机操作序列轰炸它，比手写用例更容易撞出问题。

use ll_platform::input::{GameKey, InputState, RepeatConfig};
use proptest::prelude::*;
use std::time::{Duration, Instant};

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

/// 对状态机施加的一次操作，额外覆盖自动重复的时间推进。
///
/// 与上面的 [`Op`] 分开定义：自动重复只在调用 `begin_frame` 时才推进，
/// 混进不涉及时间的旧属性测试会让它们的断言意图变得含糊。
#[derive(Debug, Clone, Copy)]
enum RepeatOp {
    Press,
    Release,
    EndFrame,
    /// 推进给定毫秒数后调用一次 `begin_frame`。
    BeginFrame(u64),
}

fn repeat_op_strategy() -> impl Strategy<Value = RepeatOp> {
    prop_oneof![
        Just(RepeatOp::Press),
        Just(RepeatOp::Release),
        Just(RepeatOp::EndFrame),
        // 上限故意覆盖初始延迟与重复间隔的好几倍，逼出跨越多个重复
        // 周期的慢帧场景。
        (0u64..2_000).prop_map(RepeatOp::BeginFrame),
    ]
}

/// 对状态机施加一次操作，`now` 是随 `BeginFrame` 单调推进的模拟墙钟。
fn apply_repeat_op(
    input: &mut InputState,
    key: GameKey,
    op: RepeatOp,
    now: &mut Instant,
    config: RepeatConfig,
) {
    match op {
        RepeatOp::Press => input.press(key),
        RepeatOp::Release => input.release(key),
        RepeatOp::EndFrame => input.end_frame(),
        RepeatOp::BeginFrame(delay_ms) => {
            *now += Duration::from_millis(delay_ms);
            input.begin_frame(*now, config);
        }
    }
}

proptest! {
    #[test]
    fn 不参与重复的键的已激活判定恒等于刚按下判定(ops in prop::collection::vec(repeat_op_strategy(), 0..64)) {
        // Confirm 未被登记为可重复键（见 GameKey::is_repeatable）：无论
        // 施加多长的模拟时间、跨越多少个重复周期，was_activated 都不该
        // 比 was_just_pressed 多触发一次——否则长按确认键会把菜单点穿。
        // Arrange
        let mut input = InputState::new();
        let key = GameKey::Confirm;
        let config = RepeatConfig::default();
        let mut now = Instant::now();

        // Act & Assert
        for op in ops {
            apply_repeat_op(&mut input, key, op, &mut now, config);
            prop_assert_eq!(input.was_activated(key), input.was_just_pressed(key));
        }
    }

    #[test]
    fn 跨越慢帧的单次重复不会在下一帧立刻补发(delay_ms in 1_000u64..5_000) {
        // 模拟一次跨越远超一个重复间隔的慢帧：无论慢帧跨越了多久，
        // 紧接着极短时间后的下一帧都不应因为「欠账」而立刻再次触发。
        // 这正是 begin_frame 用 `now + interval` 而非「旧值 + interval」
        // 的原因。
        // Arrange
        let mut input = InputState::new();
        let key = GameKey::Up;
        let config = RepeatConfig::default();
        let pressed_at = Instant::now();
        input.press(key);
        input.begin_frame(pressed_at, config);
        input.end_frame();
        let slow_frame_now = pressed_at + config.initial_delay + Duration::from_millis(delay_ms);
        input.begin_frame(slow_frame_now, config);
        input.end_frame();

        // Act
        input.begin_frame(slow_frame_now + Duration::from_millis(1), config);

        // Assert
        prop_assert!(!input.was_activated(key));
    }
}
