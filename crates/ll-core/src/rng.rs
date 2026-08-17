//! 确定性随机数。
//!
//! # 为什么禁止全局随机数流
//!
//! 全局流的取值结果取决于**谁先取**。一旦引入多线程并行结算，或读档后
//! 实体的处理顺序发生细微变化，整条序列就会错位，世界随之走向另一个
//! 平行宇宙——这会同时摧毁自由读档与确定性重放两项能力。
//!
//! 本模块的做法是让随机数由 `(世界种子, 实体 ID, 事件计数)` 三元组
//! **计算得出**而非从共享流中取出。同一个三元组在任何时候、任何线程、
//! 任何平台上都得到相同结果，因此并行结算天然安全，无需任何同步。
//!
//! 算法采用 splitmix64。选它的原因：实现只有几行、无需任何依赖
//! （`ll-core` 必须零依赖）、雪崩性质良好、且全部是整数运算，因而
//! 跨平台逐位一致。

/// splitmix64 的混合函数。
///
/// 常量取自算法原始定义，**不可随意更改**——它们经过雪崩性质验证，
/// 换成别的数会显著降低输出质量。
const fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 由 `(世界种子, 实体 ID, 事件计数)` 派生的确定性随机数发生器。
#[derive(Debug, Clone)]
pub struct DetRng {
    state: u64,
}

impl DetRng {
    /// 为某个实体在某个事件时刻派生一条随机流。
    ///
    /// 三个输入逐级混合而非简单异或：直接异或会让 `(种子=1, 实体=2)` 与
    /// `(种子=2, 实体=1)` 得到同一条流，造成不同实体之间出现可察觉的
    /// 行为关联。
    pub const fn for_entity(world_seed: u64, entity_id: u64, event_counter: u64) -> Self {
        let a = splitmix64(world_seed);
        let b = splitmix64(entity_id ^ a);
        let c = splitmix64(event_counter ^ b);
        DetRng { state: c }
    }

    /// 取下一个 64 位随机数。
    pub fn next_u64(&mut self) -> u64 {
        self.state = splitmix64(self.state);
        self.state
    }

    /// 取 `[0, exclusive_upper)` 内的随机数；上界为零时返回零。
    ///
    /// 采用 Lemire 的乘法取余法：比取余更快，且**无模偏差**。朴素的
    /// `next_u64() % n` 会让较小的值出现概率略高，这种偏差在百万次经济
    /// 模拟中会累积成可观的系统性倾斜。
    pub fn gen_range(&mut self, exclusive_upper: u64) -> u64 {
        if exclusive_upper == 0 {
            return 0;
        }
        let product = (self.next_u64() as u128) * (exclusive_upper as u128);
        (product >> 64) as u64
    }

    /// 以 `numerator / denominator` 的概率返回真。
    ///
    /// 分母为零时恒返回假——与其除零崩溃，不如把无意义的概率当作永不
    /// 发生，让上层的脚本错误降级策略能够接管。
    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        if denominator == 0 {
            return false;
        }
        self.gen_range(denominator as u64) < numerator as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 相同三元组产出相同序列() {
        // 这是确定性重放与跨平台一致的基石。
        // Arrange
        let mut first = DetRng::for_entity(42, 7, 3);
        let mut second = DetRng::for_entity(42, 7, 3);

        // Act
        let a: Vec<u64> = (0..8).map(|_| first.next_u64()).collect();
        let b: Vec<u64> = (0..8).map(|_| second.next_u64()).collect();

        // Assert
        assert_eq!(a, b);
    }

    #[test]
    fn 不同实体在同一时刻产出不同序列() {
        // 若不同实体共享序列，一群怪物会做出完全相同的决策。
        // Arrange
        let mut first = DetRng::for_entity(42, 7, 0);
        let mut second = DetRng::for_entity(42, 8, 0);

        // Act
        let a = first.next_u64();
        let b = second.next_u64();

        // Assert
        assert_ne!(a, b);
    }

    #[test]
    fn 交换种子与实体号得到不同序列() {
        // 若三个输入是简单异或合成，(种子=1,实体=2) 与 (种子=2,实体=1)
        // 会得到同一条流，不同实体之间将出现可察觉的行为关联。
        // Arrange
        let mut first = DetRng::for_entity(1, 2, 0);
        let mut second = DetRng::for_entity(2, 1, 0);

        // Act & Assert
        assert_ne!(first.next_u64(), second.next_u64());
    }

    #[test]
    fn 取值范围上界为零时返回零() {
        // Arrange
        let mut rng = DetRng::for_entity(1, 1, 1);

        // Act
        let value = rng.gen_range(0);

        // Assert
        assert_eq!(value, 0);
    }

    #[test]
    fn 概率判定分母为零时恒为假() {
        // 与其除零崩溃，不如把无意义的概率当作永不发生。
        // Arrange
        let mut rng = DetRng::for_entity(1, 1, 1);

        // Act
        let hit = rng.chance(1, 0);

        // Assert
        assert!(!hit);
    }

    #[test]
    fn 概率为百分之百时恒为真() {
        // Arrange
        let mut rng = DetRng::for_entity(9, 9, 9);

        // Act & Assert
        for _ in 0..64 {
            assert!(rng.chance(1, 1));
        }
    }

    #[test]
    fn 取值恒落在指定范围内() {
        // Arrange
        let mut rng = DetRng::for_entity(0xDEAD_BEEF, 1, 1);
        let upper = 37_u64;

        // Act & Assert
        for _ in 0..10_000 {
            assert!(rng.gen_range(upper) < upper);
        }
    }

    #[test]
    fn 概率判定的实际频率接近标称值() {
        // 验证无模偏差。若用朴素的取余，此测试在小分母下会偏。
        // Arrange
        let mut rng = DetRng::for_entity(7, 7, 7);
        let trials = 100_000_usize;

        // Act
        let hits = (0..trials).filter(|_| rng.chance(1, 3)).count();

        // Assert
        let expected = trials / 3;
        let tolerance = trials / 50; // 允许 2% 偏差
        assert!(hits.abs_diff(expected) < tolerance);
    }
}
