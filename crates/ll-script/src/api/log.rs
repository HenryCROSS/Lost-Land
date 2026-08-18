//! 脚本内部错误/断言失败上报，携带来源信息，供加载管理界面（任务 11）
//! 展示。
//!
//! # 能带多少信息，取决于 ADR 0012 的探针结果
//!
//! `SteelErr::span()` 返回 `Option<Span>`（字节偏移区间，不是行列号）
//! ——ADR 0012 实测：对 `(+ 1 undefined-identifier)` 求值，确实拿到了
//! `Some(Span { start: 5, end: 25, .. })`，精确框住了触发错误的那段
//! 源码文本。但 [`crate::host::ScriptError`] 目前只保留了 `SteelErr`
//! 的 `Display` 字符串，没有转发 `span()`——本模块提供的诊断类型因此
//! 也只能携带 `ScriptError` 已经暴露出来的信息（错误分类 + 文本消息），
//! 暂不含字节偏移。若加载管理界面确实需要精确定位到源码位置，需要回头
//! 给 `ScriptError` 补一个 `span: Option<(u32, u32)>` 字段，把
//! `classify_error` 里已经能拿到的 `err.span()` 转发出来——这是一次
//! 很小的改动，但本任务不预先做：任务 11 还没有落地，此刻加这个字段
//! 只能凭空猜测界面需要什么形状的数据。

use crate::host::ScriptError;

/// mod 加载/运行期间的一条诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDiagnostic {
    /// 出问题的 mod 来源标识（通常是 mod 的命名空间或文件名），由调用方
    /// 在构造诊断时提供——本类型不知道自己来自哪个 mod，脚本引擎本身
    /// 不追踪"当前在跑哪个 mod 的源码"这类元信息。
    pub source: String,
    /// 面向 mod 作者的错误消息。
    pub message: String,
    /// 严重程度。
    pub severity: Severity,
}

/// 诊断严重程度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 语法错误或缺参一类——通常意味着这个 mod 根本没法加载，需要 mod
    /// 作者修复后才能继续。
    Error,
    /// 运行时错误（比如引用了未定义的标识符）或被中断——单次调用失败，
    /// 但脚本引擎本身仍然可用，游戏可以继续跑，只是这一次调用被降级。
    Warning,
}

impl ScriptDiagnostic {
    /// 从一次脚本调用失败构造诊断。
    pub fn from_error(source: impl Into<String>, error: &ScriptError) -> Self {
        let severity = match error {
            ScriptError::ParseError(_) | ScriptError::ArityMismatch(_) => Severity::Error,
            ScriptError::Interrupted | ScriptError::Runtime(_) => Severity::Warning,
        };
        ScriptDiagnostic {
            source: source.into(),
            message: error.to_string(),
            severity,
        }
    }
}

impl std::fmt::Display for ScriptDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self.severity {
            Severity::Error => "错误",
            Severity::Warning => "警告",
        };
        write!(f, "[{label}] {}：{}", self.source, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 语法错误归类为错误级严重程度() {
        // Arrange
        let error = ScriptError::ParseError("缺右括号".to_string());

        // Act
        let diagnostic = ScriptDiagnostic::from_error("测试mod", &error);

        // Assert
        assert_eq!(diagnostic.severity, Severity::Error);
    }

    #[test]
    fn 运行时错误归类为警告级严重程度() {
        // Arrange
        let error = ScriptError::Runtime("未定义标识符".to_string());

        // Act
        let diagnostic = ScriptDiagnostic::from_error("测试mod", &error);

        // Assert
        assert_eq!(diagnostic.severity, Severity::Warning);
    }

    #[test]
    fn 诊断的显示文本包含来源与消息() {
        // Arrange
        let error = ScriptError::Runtime("出错了".to_string());
        let diagnostic = ScriptDiagnostic::from_error("某个mod", &error);

        // Act
        let text = diagnostic.to_string();

        // Assert
        assert!(text.contains("某个mod"));
        assert!(text.contains("出错了"));
    }
}
