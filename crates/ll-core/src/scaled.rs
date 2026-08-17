//! 定标整数：用整数表达需要小数精度的世界量。
//!
//! 世界状态**禁止使用浮点数**——同一段 `f64` 运算在 Windows 与 Linux 上
//! 可能产出不同的最低位，而确定性存档与重放要求两平台逐位一致。
//! 因此凡需要小数的世界量（价格、比例、速率）一律用本类型表达。
//!
//! 浮点仅允许出现在渲染与音频层，且结果不得回流入世界状态。

use crate::error::CoreError;

/// 每个整数单位对应的定标刻度数。
///
/// 取一千是因为经济系统的价格需要到「厘」的精度：若只到「分」，
/// 大宗商品按百分比抽税时的舍入误差会在长期模拟中累积成可观偏差。
pub const SCALE: i64 = 1_000;

/// 以千分之一为单位的定标整数。
///
/// `Milli(1_500)` 表示 1.5。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Milli(pub i64);

impl Milli {
    /// 零值。
    pub const ZERO: Milli = Milli(0);

    /// 由整数构造，自动放大 [`SCALE`] 倍。
    pub const fn from_whole(whole: i64) -> Self {
        Milli(whole * SCALE)
    }

    /// 取整数部分，**向零截断**。
    ///
    /// 选择向零截断而非向下取整，是为了让正负方向对称：向下取整会使
    /// `-1.5` 变成 `-2` 而 `1.5` 变成 `1`，经济结算中的盈亏就会产生
    /// 系统性偏移。
    pub const fn whole(&self) -> i64 {
        self.0 / SCALE
    }

    /// 按分数比例缩放，溢出或分母为零时返回 [`None`]。
    ///
    /// 用分数而非小数表达比例，同样是为了避开浮点。中间乘积用 [`i128`]
    /// 承接，避免先乘后除时的假溢出。
    pub fn checked_mul_ratio(self, numerator: i64, denominator: i64) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        let product = (self.0 as i128).checked_mul(numerator as i128)?;
        let quotient = product / (denominator as i128);
        i64::try_from(quotient).ok().map(Milli)
    }

    /// 按分数比例缩放，失败时返回具体错误原因。
    ///
    /// 供需要向上层报告失败原因的调用方使用。
    pub fn mul_ratio(self, numerator: i64, denominator: i64) -> Result<Self, CoreError> {
        if denominator == 0 {
            return Err(CoreError::DivisionByZero);
        }
        self.checked_mul_ratio(numerator, denominator)
            .ok_or(CoreError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 整数转定标值时放大一千倍() {
        // Arrange
        let whole = 7_i64;

        // Act
        let scaled = Milli::from_whole(whole);

        // Assert
        assert_eq!(scaled.0, 7_000);
    }

    #[test]
    fn 取整时向零截断而非向下取整() {
        // 向下取整会让「亏损 1.5 金币」变成亏 2 金币，与正数方向不对称，
        // 经济结算会产生系统性偏移。
        // Arrange
        let negative = Milli(-1_500);

        // Act
        let whole = negative.whole();

        // Assert
        assert_eq!(whole, -1);
    }

    #[test]
    fn 按比例缩放时分母为零返回空值() {
        // Arrange
        let value = Milli(1_000);

        // Act
        let result = value.checked_mul_ratio(3, 0);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 按比例缩放在溢出时返回空值而非回绕() {
        // Arrange
        let huge = Milli(i64::MAX);

        // Act
        let result = huge.checked_mul_ratio(2, 1);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 比例缩放的错误形式区分除零与溢出() {
        // Arrange
        let value = Milli(1_000);

        // Act
        let divide = value.mul_ratio(1, 0);
        let overflow = Milli(i64::MAX).mul_ratio(2, 1);

        // Assert
        assert_eq!(divide, Err(CoreError::DivisionByZero));
        assert_eq!(overflow, Err(CoreError::Overflow));
    }
}
