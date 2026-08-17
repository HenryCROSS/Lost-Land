//! 跨平台确定性回归。
//!
//! 本文件里的期望值是**黄金基准**：它们由算法定义唯一确定，在 Windows
//! 与 Linux 上必须逐位相同。
//!
//! # 测试失败意味着什么
//!
//! 若某次改动让这里的摘要变了，只有两种可能：
//!
//! 1. 有意修改了算法或常量——那么更新期望值，并在提交信息里说明为什么。
//! 2. **无意引入了平台相关行为**（最常见的是浮点运算，或依赖了哈希表
//!    的遍历顺序）。这是必须立刻修复的缺陷。
//!
//! **绝不允许「测试挂了就把期望值改成实际值」**——那等于删掉这道防线。

use ll_core::hashing::StateHasher;
use ll_core::rng::DetRng;
use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR, Tick};
use ll_core::torus::TorusSize;

/// 由首次运行记录的黄金基准。修改前请阅读本文件顶部说明。
const EXPECTED_RNG_DIGEST: u64 = 7_219_837_048_615_413_302;

/// 由首次运行记录的黄金基准。
const EXPECTED_TORUS_DIGEST: u64 = 5_790_311_870_083_093_695;

/// 由首次运行记录的黄金基准。
const EXPECTED_TIME_DIGEST: u64 = 11_375_461_100_615_141_029;

#[test]
fn 随机序列的摘要跨平台稳定() {
    // 这是整个确定性体系的守门测试。
    // Arrange
    let mut rng = DetRng::for_entity(0x1234_5678, 42, 0);
    let mut hasher = StateHasher::new();

    // Act
    for _ in 0..1_000 {
        hasher.write_u64(rng.next_u64());
    }

    // Assert
    assert_eq!(hasher.finish(), EXPECTED_RNG_DIGEST);
}

#[test]
fn 环面距离序列的摘要跨平台稳定() {
    // Arrange
    let world = TorusSize::new(4096, 4096).expect("宽高均不为零");
    let mut hasher = StateHasher::new();

    // Act
    for i in 0..500_i32 {
        let a = world.wrap(i * 7, i * 13);
        let b = world.wrap(4096 - i * 3, i * 29);
        hasher.write_u64(world.chebyshev(a, b) as u64);
        hasher.write_u64(world.squared_euclidean(a, b));
    }

    // Assert
    assert_eq!(hasher.finish(), EXPECTED_TORUS_DIGEST);
}

#[test]
fn 季节推进的摘要跨平台稳定() {
    // Arrange
    let mut hasher = StateHasher::new();

    // Act
    for day in 0..365_i64 {
        // 加小时偏移，使 hour_of_day 与 is_daylight 真正参与摘要。
        // 原先采样点恒为当日 0 点，这两列是常量，等于没有被覆盖。
        let tick = Tick(day * TICKS_PER_DAY + (day % 24) * TICKS_PER_HOUR);
        hasher.write_i64(tick.day_of_year());
        hasher.write_i64(tick.season() as i64);
        hasher.write_i64(tick.hour_of_day());
        hasher.write_i64(tick.is_daylight() as i64);
    }

    // Assert
    assert_eq!(hasher.finish(), EXPECTED_TIME_DIGEST);
}
