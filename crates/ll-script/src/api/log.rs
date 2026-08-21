//! 脚本内部错误/断言失败上报，携带来源信息，供加载管理界面（任务 11）
//! 展示。
//!
//! # 字节偏移已经补上（任务 11 落地时的回填）
//!
//! 本模块此前的文档预告了这件事、但刻意没做——「任务 11 还没有落地，
//! 此刻加这个字段只能凭空猜测界面需要什么形状的数据」。任务 11 落地
//! 时确认了形状：加载管理界面确实需要精确到行号，[`crate::host::ScriptError`]
//! 现在转发了 `SteelErr::span()`/AST 节点 `SyntaxObject::span`（见其
//! 文档与 [`crate::whitelist`]），[`ScriptDiagnostic::byte_offset`]
//! 把这份信息原样带出来——换算成行号需要原始源码文本，那是 `ll-mod`
//! 加载管理界面一侧的事（诊断类型本身不持有、也不该持有一份源码拷贝）。

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
    /// 触发错误的源码字节偏移量，来自 [`ScriptError::byte_offset`]；
    /// `None` 表示这类错误（如超时、如内存预算超限）天生没有一个能
    /// 归咎的具体位置。
    pub byte_offset: Option<u32>,
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
    /// 脚本状态写入超过配额（`knowledge/design/script-state-storage.md`
    /// 六、3 节）时构造的诊断——不是 `ScriptError`：超限是运行时会
    /// 发生的正常状况（mod 存的数据碰上了自己的配额上限），不是脚本
    /// 语法/运行时出错，因此不走 [`Self::from_error`] 那条路径，单独
    /// 给一个构造函数，级别定为 [`Severity::Warning`]（脚本引擎本身
    /// 仍然可用，游戏可以继续跑，只是这一次写入被拒绝）。
    ///
    /// 若只把写入结果降级成失败哨兵值返回给脚本、不留下诊断痕迹，
    /// mod 作者会看到"写入好像没生效"却毫无线索——这条诊断正是
    /// 设计文档要求的"必须留痕"那一步。
    pub fn quota_exceeded(mod_namespace: impl Into<String>, key: impl AsRef<str>) -> Self {
        let mod_namespace = mod_namespace.into();
        ScriptDiagnostic {
            source: mod_namespace.clone(),
            message: format!(
                "脚本状态写入被拒绝：mod 「{mod_namespace}」的键 「{}」超出存储配额",
                key.as_ref()
            ),
            severity: Severity::Warning,
            byte_offset: None,
        }
    }

    /// 从一次脚本调用失败构造诊断。
    ///
    /// `Timeout`/`MemoryBudgetExceeded` 与 `Runtime` 同归 `Warning`：
    /// 三者共同的特点是「单次调用失败，但脚本引擎本身仍然可用」——不
    /// 像语法错误/缺参那样意味着整份 mod 源码就没编译通过。拆分成两个
    /// 变体只是为了让调用方（`ll-mod` 的 `classify_script_stage`、以及
    /// mod 作者读到的诊断文本）能分辨"因为什么"失败，不改变"失败严重
    /// 到什么程度"这个判断——两者是两个独立的维度。
    pub fn from_error(source: impl Into<String>, error: &ScriptError) -> Self {
        let severity = match error {
            ScriptError::ParseError(..) | ScriptError::ArityMismatch(..) => Severity::Error,
            ScriptError::Timeout
            | ScriptError::MemoryBudgetExceeded { .. }
            | ScriptError::Runtime(..) => Severity::Warning,
        };
        ScriptDiagnostic {
            source: source.into(),
            message: error.to_string(),
            severity,
            byte_offset: error.byte_offset(),
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
        let error = ScriptError::ParseError("缺右括号".to_string(), None);

        // Act
        let diagnostic = ScriptDiagnostic::from_error("测试mod", &error);

        // Assert
        assert_eq!(diagnostic.severity, Severity::Error);
    }

    #[test]
    fn 超时错误归类为警告级严重程度() {
        // Arrange
        let error = ScriptError::Timeout;

        // Act
        let diagnostic = ScriptDiagnostic::from_error("测试mod", &error);

        // Assert
        assert_eq!(diagnostic.severity, Severity::Warning);
    }

    #[test]
    fn 内存预算超限错误归类为警告级严重程度() {
        // Arrange
        let error = ScriptError::MemoryBudgetExceeded {
            allocated_bytes: 200,
            budget_bytes: 100,
        };

        // Act
        let diagnostic = ScriptDiagnostic::from_error("测试mod", &error);

        // Assert
        assert_eq!(diagnostic.severity, Severity::Warning);
    }

    #[test]
    fn 运行时错误归类为警告级严重程度() {
        // Arrange
        let error = ScriptError::Runtime("未定义标识符".to_string(), None);

        // Act
        let diagnostic = ScriptDiagnostic::from_error("测试mod", &error);

        // Assert
        assert_eq!(diagnostic.severity, Severity::Warning);
    }

    #[test]
    fn 诊断的显示文本包含来源与消息() {
        // Arrange
        let error = ScriptError::Runtime("出错了".to_string(), None);
        let diagnostic = ScriptDiagnostic::from_error("某个mod", &error);

        // Act
        let text = diagnostic.to_string();

        // Assert
        assert!(text.contains("某个mod"));
        assert!(text.contains("出错了"));
    }

    #[test]
    fn 配额超限诊断归类为警告级严重程度且携带命名空间与键() {
        // Arrange & Act
        let diagnostic = ScriptDiagnostic::quota_exceeded("lostland", "reputation");

        // Assert
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(diagnostic.byte_offset, None);
        let text = diagnostic.to_string();
        assert!(text.contains("lostland"));
        assert!(text.contains("reputation"));
    }

    #[test]
    fn 携带字节偏移的错误诊断原样转发偏移量() {
        // Arrange：模拟 classify_error 已经从 SteelErr::span() 取到偏移量
        // 的场景——诊断类型不该在转发过程中把这份信息弄丢。
        let error = ScriptError::ParseError("未闭合的括号".to_string(), Some(42));

        // Act
        let diagnostic = ScriptDiagnostic::from_error("某个mod", &error);

        // Assert
        assert_eq!(diagnostic.byte_offset, Some(42));
    }
}
