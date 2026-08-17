//! 世界状态摘要。
//!
//! 用途是把整个世界状态归约成一个 64 位数字，使「两次运行是否产生了
//! 相同的世界」可以被一行断言检验。这是确定性重放与跨平台一致性回归
//! 的基础设施。
//!
//! # 为什么不用标准库的 Hasher
//!
//! `std::collections::hash_map::DefaultHasher` 的算法**不保证跨版本
//! 稳定**，标准库文档明确说明它可能在任何 Rust 版本变更。用它做黄金
//! 基准，会在某次工具链升级后集体失效，而那时无法区分是升级导致的
//! 还是真的引入了缺陷。
//!
//! 因此这里手写 FNV-1a：算法极简、完全由整数运算构成、由规范唯一确定，
//! 因而跨平台跨版本恒定。它不适合做哈希表（抗碰撞性一般），但用于
//! 检测「状态是否改变」完全足够。

/// FNV-1a 64 位的初始值，由算法规范定义。
const FNV_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;

/// FNV-1a 64 位的质数，由算法规范定义。
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// 世界状态的增量哈希器。
#[derive(Debug, Clone)]
pub struct StateHasher {
    digest: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl StateHasher {
    /// 建立初始哈希器。
    pub const fn new() -> Self {
        StateHasher {
            digest: FNV_OFFSET_BASIS,
        }
    }

    /// 混入一个无符号整数。
    ///
    /// 按小端序逐字节混入。**必须显式指定字节序**——依赖本机字节序会让
    /// 大端平台产出不同的哈希，正好破坏本模块存在的意义。
    pub fn write_u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.digest ^= byte as u64;
            self.digest = self.digest.wrapping_mul(FNV_PRIME);
        }
    }

    /// 混入一个有符号整数。
    pub fn write_i64(&mut self, value: i64) {
        // 直接按位重解释而非取绝对值：负数的位模式必须原样参与哈希，
        // 否则 -1 与 1 会得到相同摘要。
        self.write_u64(value as u64);
    }

    /// 取当前摘要。可在中途多次调用，不影响后续混入。
    pub const fn finish(&self) -> u64 {
        self.digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空哈希器返回算法规定的初值() {
        // Arrange
        let hasher = StateHasher::new();

        // Act
        let digest = hasher.finish();

        // Assert
        assert_eq!(digest, 0xCBF2_9CE4_8422_2325);
    }

    #[test]
    fn 混入顺序不同则摘要不同() {
        // 顺序敏感是必需的：若 (移动,攻击) 与 (攻击,移动) 摘要相同，
        // 就检测不出事件顺序被打乱这类确定性缺陷。
        // Arrange
        let mut first = StateHasher::new();
        let mut second = StateHasher::new();

        // Act
        first.write_u64(1);
        first.write_u64(2);
        second.write_u64(2);
        second.write_u64(1);

        // Assert
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn 正负数摘要不同() {
        // Arrange
        let mut positive = StateHasher::new();
        let mut negative = StateHasher::new();

        // Act
        positive.write_i64(1);
        negative.write_i64(-1);

        // Assert
        assert_ne!(positive.finish(), negative.finish());
    }
}
