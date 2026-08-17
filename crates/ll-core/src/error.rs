//! 基础层的错误类型。
//!
//! 刻意不引入 `thiserror` 等派生宏库：`ll-core` 必须保持零运行时依赖，
//! 而手写 `Display` 与 `Error` 的成本远低于让整个项目多背一个依赖。

use core::fmt;

/// 基础层可能产生的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// 命名空间标识符不合法，附带原始输入以便定位。
    InvalidIdentifier(String),
    /// 除数为零。
    DivisionByZero,
    /// 整数运算溢出。
    Overflow,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 此处文案面向开发者与日志，不面向玩家，故不走 i18n
        // （规格 §11.3 约束的是用户可见字符串）。
        match self {
            CoreError::InvalidIdentifier(raw) => {
                write!(f, "invalid namespaced identifier: {raw:?}")
            }
            CoreError::DivisionByZero => write!(f, "division by zero"),
            CoreError::Overflow => write!(f, "integer overflow"),
        }
    }
}

impl core::error::Error for CoreError {}
