//! 确定性随机 API：脚本只能消耗 `DetRng`，拿不到种子本身。
//!
//! # 为什么脚本连"重新构造"都做不到
//!
//! 见 `ll_core::rng` 的模块文档：`DetRng::for_entity(世界种子, 实体 ID,
//! 事件计数)` 是唯一构造入口。这里注册给脚本的三个函数只接收**已经
//! 构造好**的 `DetRng`——由宿主在每次调用前用 [`set_active_rng`] 设置
//! 好，脚本能调用的只是"要下一个数"。脚本没有任何函数能传入种子、
//! 实体 ID 或事件计数去重新拼一个 `DetRng`：这三个整数从未作为参数
//! 出现在任何注册给脚本的函数签名里，脚本无法伪造。
//!
//! # 为什么用 `next_u64() as i64` 而不是原样 `u64`
//!
//! Steel 的整数类型 `IntV` 内部是 `isize`（64 位平台上等同 `i64`），
//! 没有原生 `u64` 支持。原样传 `u64` 里超过 `i64::MAX` 的那一半取值
//! 会在转换时按位重新解释成负数——这是 Steel 数字系统本身的限制，不是
//! 本模块的缺陷。**这不影响确定性**：同样的位模式在任何时候转换结果都
//! 相同，只是脚本看到的显示值可能是负的。mod 作者需要"纯粹落在正区间
//! 的随机数"应该用 `rng-gen-range`/`rng-chance`，不应该直接消费
//! `rng-next-u64` 的裸值做判断。

use std::cell::RefCell;

use ll_core::rng::DetRng;

use crate::host::ScriptEngine;

thread_local! {
    /// 当前调用窗口内脚本可以消耗的随机流。
    ///
    /// 宿主必须在调用脚本前设置、调用结束后清空——不清空的话，下一次
    /// 忘记设置活跃流的调用会悄悄复用上一次的状态，产生看似"确定"实则
    /// 张冠李戴的结果（比如实体 A 的调用意外用上了实体 B 遗留的流）。
    static ACTIVE_RNG: RefCell<Option<DetRng>> = const { RefCell::new(None) };
}

/// 设置本次调用窗口内脚本能消耗的随机流。
///
/// `rng` 必须来自 `DetRng::for_entity`——本函数的签名本身就保证了这
/// 一点，它接收的是一个已经构造完成的 `DetRng` 值，脚本没有参与构造
/// 过程的任何一步。
pub fn set_active_rng(rng: DetRng) {
    ACTIVE_RNG.with(|cell| *cell.borrow_mut() = Some(rng));
}

/// 清空活跃随机流。
pub fn clear_active_rng() {
    ACTIVE_RNG.with(|cell| *cell.borrow_mut() = None);
}

/// 在活跃流上执行 `f`；没有设置活跃流时返回 `default`。
///
/// 「没有活跃流」按宿主的调用约定不应该发生（每次调用前都该设置好），
/// 但脚本调用是四道防线覆盖的边界——防御性地返回一个确定的默认值，
/// 而不是 panic，是本类型对「宿主接线可能有 bug」这种情况的降级策略。
fn with_active_rng<T>(default: T, f: impl FnOnce(&mut DetRng) -> T) -> T {
    ACTIVE_RNG.with(|cell| match cell.borrow_mut().as_mut() {
        Some(rng) => f(rng),
        None => default,
    })
}

/// 把 `rng-next-u64`/`rng-gen-range`/`rng-chance` 注册进脚本引擎。
pub fn register(engine: &mut ScriptEngine) {
    engine.register_fn("rng-next-u64", rng_next_u64);
    engine.register_fn("rng-gen-range", rng_gen_range);
    engine.register_fn("rng-chance", rng_chance);
}

fn rng_next_u64() -> i64 {
    with_active_rng(0, |rng| rng.next_u64() as i64)
}

/// `(rng-gen-range lo hi)`：取 `[lo, hi)` 内的随机整数；`hi <= lo` 时
/// 返回 `lo`（与 `DetRng::gen_range` 上界为零时返回零的降级思路一致：
/// 无意义的区间不崩溃，退化成一个确定的边界值）。
fn rng_gen_range(lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let span = (hi - lo) as u64;
    lo + with_active_rng(0, |rng| rng.gen_range(span)) as i64
}

/// `(rng-chance permille)`：以 `permille`/1000 的概率返回真。
fn rng_chance(permille: i64) -> bool {
    let permille = permille.max(0) as u32;
    with_active_rng(false, |rng| rng.chance(permille, 1000))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_i64(engine: &mut ScriptEngine, name: &str) -> i64 {
        match engine.call_raw(name, Vec::new()) {
            Ok(steel::rvals::SteelVal::IntV(n)) => n as i64,
            other => panic!("期望 IntV，实际拿到 {other:?}"),
        }
    }

    #[test]
    fn 脚本连续两次调用rng_next_u64得到不同值() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register(&mut engine);
        engine
            .load_source("(define (probe) (rng-next-u64))".to_string())
            .unwrap();
        set_active_rng(DetRng::for_entity(1, 1, 1));

        // Act
        let first = call_i64(&mut engine, "probe");
        let second = call_i64(&mut engine, "probe");

        // Assert
        assert_ne!(first, second);

        // Cleanup
        clear_active_rng();
    }

    #[test]
    fn 相同实体相同事件计数的两次独立调用得到相同的随机序列() {
        // Arrange：模拟"两个不同时刻的独立调用"，每次都重新构造引擎，
        // 只有 world_seed/entity_id/event_counter 三元组相同。
        //
        // 两个引擎都在这里先造齐，再交给 `run_three_calls` 去编译——
        // 本线程「全部构造先于全部编译」这条约束（见 `ll_script::host`
        // 里 `COMPILED_ON_THIS_THREAD` 上方注释）要求如此，写成
        // 「造一个跑一个」第二次构造会直接 panic。
        let first_engine = ScriptEngine::new();
        let second_engine = ScriptEngine::new();

        // Act
        let first_sequence = run_three_calls(first_engine, 42, 7, 3);
        let second_sequence = run_three_calls(second_engine, 42, 7, 3);

        // Assert
        assert_eq!(first_sequence, second_sequence);
    }

    fn run_three_calls(
        mut engine: ScriptEngine,
        world_seed: u64,
        entity_id: u64,
        event_counter: u64,
    ) -> Vec<i64> {
        register(&mut engine);
        engine
            .load_source("(define (probe) (rng-next-u64))".to_string())
            .unwrap();
        set_active_rng(DetRng::for_entity(world_seed, entity_id, event_counter));

        let values = vec![
            call_i64(&mut engine, "probe"),
            call_i64(&mut engine, "probe"),
            call_i64(&mut engine, "probe"),
        ];

        clear_active_rng();
        values
    }

    #[test]
    fn 取值范围内的结果恒落在指定区间() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register(&mut engine);
        engine
            .load_source("(define (probe) (rng-gen-range 10 20))".to_string())
            .unwrap();
        set_active_rng(DetRng::for_entity(9, 9, 9));

        // Act & Assert
        for _ in 0..64 {
            let value = call_i64(&mut engine, "probe");
            assert!((10..20).contains(&value));
        }

        // Cleanup
        clear_active_rng();
    }

    #[test]
    fn 概率为零时恒不命中() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register(&mut engine);
        engine
            .load_source("(define (probe) (rng-chance 0))".to_string())
            .unwrap();
        set_active_rng(DetRng::for_entity(3, 3, 3));

        // Act
        let result = engine.call_raw("probe", Vec::new());

        // Assert
        assert_eq!(result, Ok(steel::rvals::SteelVal::BoolV(false)));

        // Cleanup
        clear_active_rng();
    }
}
