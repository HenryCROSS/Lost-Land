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

use crate::ident::NamespacedId;

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
        self.write_bytes(&value.to_le_bytes());
    }

    /// 混入一段任意字节——字符串、路径这类变长数据的入口。
    ///
    /// 整数字段（[`Self::write_u64`]/[`Self::write_i64`]）转成小端字节
    /// 后落到同一条逐字节混入路径，避免「整数走一套算法、字节串走
    /// 另一套」两处实现同一件事却可能悄悄漂移。字符串本身不需要额外
    /// 声明字节序——UTF-8 编码的字节序列在任何平台上都是同一串字节。
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.digest ^= *byte as u64;
            self.digest = self.digest.wrapping_mul(FNV_PRIME);
        }
    }

    /// 混入一个有符号整数。
    pub fn write_i64(&mut self, value: i64) {
        // 直接按位重解释而非取绝对值：负数的位模式必须原样参与哈希，
        // 否则 -1 与 1 会得到相同摘要。
        self.write_u64(value as u64);
    }

    /// 混入一段带长度前缀的变长字节——字符串一类变长字段的公共写法。
    ///
    /// 不带长度前缀直接拼接会让 `("ab", "c")` 与 `("a", "bc")` 撞出同
    /// 一段字节序列（`ll_world::state` 已经在世界状态哈希里踩过这条
    /// 边界并确立了「变长字段一律长度前缀」这条约定，见该 crate
    /// `write_len_prefixed_bytes` 的文档），这里把同一条约定收进
    /// `StateHasher` 本身，供 [`Self::write_namespaced_id`] 与未来任何
    /// 需要混入变长字节的调用方复用，不必各自重新发明一份等价逻辑。
    pub fn write_len_prefixed_bytes(&mut self, bytes: &[u8]) {
        self.write_u64(bytes.len() as u64);
        self.write_bytes(bytes);
    }

    /// 混入一个 [`NamespacedId`]——命名空间与路径各自带长度前缀分别
    /// 混入，见 [`Self::write_len_prefixed_bytes`] 文档「不带长度前缀」
    /// 一节的碰撞论证。
    ///
    /// 供 `ll_mod::registry`（按命名空间折叠内容哈希）与
    /// `ll_mod::content_hash`（值哈希：把字段里出现的 `ContentIndex`
    /// 解析回 `NamespacedId` 字符串再混入，见该模块文档）共用同一份
    /// 编码，避免两处各写一遍迟早漂移出不一致的字节布局。
    pub fn write_namespaced_id(&mut self, id: &NamespacedId) {
        self.write_len_prefixed_bytes(id.namespace().as_bytes());
        self.write_len_prefixed_bytes(id.path().as_bytes());
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

    #[test]
    fn write_bytes与write_u64对同一数值产出相同摘要() {
        // write_u64 转发到 write_bytes，两条路径必须是同一份逻辑，
        // 不能各自维护一套容易漂移的实现。
        // Arrange
        let mut via_u64 = StateHasher::new();
        let mut via_bytes = StateHasher::new();

        // Act
        via_u64.write_u64(0x1234_5678);
        via_bytes.write_bytes(&0x1234_5678u64.to_le_bytes());

        // Assert
        assert_eq!(via_u64.finish(), via_bytes.finish());
    }

    #[test]
    fn write_bytes不同字节串产出不同摘要() {
        // Arrange
        let mut first = StateHasher::new();
        let mut second = StateHasher::new();

        // Act
        first.write_bytes(b"lostland");
        second.write_bytes(b"yourmod");

        // Assert
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn write_len_prefixed_bytes对不带分隔符会撞在一起的两组字段产出不同摘要() {
        // 验证长度前缀确实防住了 ("ab","c") 与 ("a","bc") 这类边界
        // 情形——若不带长度前缀，两者裸拼接后是同一段字节序列
        // "abc"，摘要会相同。
        // Arrange
        let mut first = StateHasher::new();
        let mut second = StateHasher::new();

        // Act
        first.write_len_prefixed_bytes(b"ab");
        first.write_len_prefixed_bytes(b"c");
        second.write_len_prefixed_bytes(b"a");
        second.write_len_prefixed_bytes(b"bc");

        // Assert
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn write_namespaced_id对不同标识符产出不同摘要() {
        // Arrange
        use crate::ident::NamespacedId;
        let mut first = StateHasher::new();
        let mut second = StateHasher::new();
        let fireball = NamespacedId::parse("lostland:fireball").expect("测试用标识符恒合法");
        let iceball = NamespacedId::parse("lostland:iceball").expect("测试用标识符恒合法");

        // Act
        first.write_namespaced_id(&fireball);
        second.write_namespaced_id(&iceball);

        // Assert
        assert_ne!(first.finish(), second.finish());
    }
}
