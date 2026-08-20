//! mod 依赖的版本约束：解析、比较，与「无约束」这一向后兼容状态。
//!
//! # 支持的语法：只有两种，YAGNI
//!
//! - 精确版本，例如 `"0.3"`：依赖目标的 `version` 字段必须与这个字符串
//!   完全相等。
//! - 版本下限，例如 `">=0.4"`：依赖目标的 `version` 按点分数字组件解析
//!   后，必须不小于这里给出的组件序列。
//!
//! 刻意不支持 Cargo 那套完整语义（`^`/`~`/多段范围/预发布标签）——当前
//! 没有真实需求要求它们：项目所有者原始描述的场景只有「精确版本」与
//! 「某版本以上」两种，`compatible_game_version`（mod ↔ 游戏引擎兼容性）
//! 是完全独立的另一条轴，不在本模块范围内（见
//! `knowledge/design/mod-package-structure.md` 五节）。若未来出现真实
//! 的范围/兼容前缀需求，应该是一次独立评估，不是现在顺手多支持几种
//! 语法。
//!
//! # 为什么不用 `semver` crate
//!
//! `semver` crate 语义正确、维护良好，许可证（MIT OR Apache-2.0）也能
//! 过 `cargo deny`（已经作为传递依赖出现在 `Cargo.lock` 里）。没有选它
//! 的理由是一个更根本的不匹配：`semver::Version::parse` 强制要求恰好
//! 三段数字（`MAJOR.MINOR.PATCH`），而 [`crate::manifest::ModManifest::version`]
//! 字段的既有文档明确写着「原样保留 mod 作者填写的字符串，不做语义化
//! 版本解析」——本项目里已经存在两段式版本号的例子（任务简报的场景
//! 描述用的就是 `"0.3"`），若用 `semver` 强解析实际版本字符串，会让
//! 大量合法但非三段式的版本号在「依赖是否存在」都还没判断完就先在
//! 语法层面报废，而这本不该是版本约束这一步该管的事。手写一个只覆盖
//! 两种约束形状的解析器，代价远小于为了适配 `semver` 的严格三段式而
//! 反过来收紧本项目对版本号格式的既有宽松承诺。
//!
//! 精确匹配复用「原样字符串相等」——与
//! `ll_content::load_error::check_mod_set`（存档硬门禁）的版本号比较
//! 策略同一条纪律，不是本模块另起的新规则。下限比较需要真正的数值
//! 大小判断，因此按 `.` 切成数字组件后逐段比较，短的一边缺的组件按 0
//! 补齐（`"0.4"` 与 `"0.4.0"` 在这个意义下视为相等）。

use std::cmp::Ordering;
use std::fmt;

/// 一条依赖版本约束。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    /// 未声明版本要求——旧版裸命名空间列表清单格式（`dependencies =
    /// [...]`）的隐式含义：只要求依赖存在，不比较版本。见
    /// [`crate::manifest`] 模块文档「向后兼容」一节。
    Any,
    /// 精确版本，原样字符串比较。
    Exact(String),
    /// 版本下限（`>=`），按点分数字组件比较。
    AtLeast(Vec<u64>),
}

impl VersionConstraint {
    /// 依赖目标实际声明的版本号 `actual` 是否满足这条约束。
    ///
    /// [`Self::AtLeast`] 分支里，`actual` 若不能被解析成点分数字组件
    /// （例如作者手写了一个非数字版本号），判定为不满足——不是「无法
    /// 比较所以放行」：约束存在的意义就是让不满足的情况显式报错，一个
    /// 连数字都不是的版本号本来就不该被认为满足了任何数值下限。
    pub fn is_satisfied_by(&self, actual: &str) -> bool {
        match self {
            VersionConstraint::Any => true,
            VersionConstraint::Exact(required) => actual == required,
            VersionConstraint::AtLeast(required) => match parse_numeric_components(actual) {
                Some(actual_components) => {
                    compare_numeric_components(&actual_components, required) != Ordering::Less
                }
                None => false,
            },
        }
    }
}

impl fmt::Display for VersionConstraint {
    /// 把约束还原成人类可读的展示文本（例如
    /// [`crate::manifest::DependencyVersionMismatch::required`] 字段），
    /// 只是一个版本号/约束片段，不是面向玩家的完整语句，因此不受规格
    /// §11.3 i18n 门禁约束——该门禁管的是完整的用户可见句子，不是句子
    /// 会引用的技术性数据片段，与 `NamespacedId`、数字字段同理。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionConstraint::Any => write!(f, "*"),
            VersionConstraint::Exact(v) => write!(f, "{v}"),
            VersionConstraint::AtLeast(components) => {
                write!(f, ">=")?;
                for (i, c) in components.iter().enumerate() {
                    if i > 0 {
                        write!(f, ".")?;
                    }
                    write!(f, "{c}")?;
                }
                Ok(())
            }
        }
    }
}

/// 把版本约束的原始文案解析成 [`VersionConstraint`]。
///
/// 只认识两种前缀：`>=`（版本下限）与「没有任何前缀」（精确版本，原样
/// 字符串）。其余看起来像版本约束运算符的前缀（`>`/`<`/`<=`/`~`/`^`）
/// 显式拒绝而不是被误当成精确字符串的一部分——静默把 `">1.0.0"` 当成
/// 「版本号恰好是这个奇怪字符串」的精确匹配，会让作者的约束永远无法
/// 满足却看不出原因，宁可在解析阶段就报错。
///
/// 出错时返回的 `String` 是面向 mod 作者的原因说明，由调用方
/// （[`crate::manifest::parse_manifest`]）负责包进带文件路径上下文的
/// [`crate::manifest::ModError::ParseError`]——与该函数处理
/// `NamespacedId` 解析错误时的既有分层一致：本函数只管「这段文本本身
/// 合不合法」，不知道也不需要知道它来自哪个文件。
pub fn parse_constraint(raw: &str) -> Result<VersionConstraint, String> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix(">=") {
        let components = parse_numeric_components(rest.trim())
            .ok_or_else(|| format!("非法版本下限约束 {trimmed:?}：>= 之后必须是点分数字版本号"))?;
        return Ok(VersionConstraint::AtLeast(components));
    }
    if trimmed.starts_with('>')
        || trimmed.starts_with('<')
        || trimmed.starts_with('~')
        || trimmed.starts_with('^')
    {
        return Err(format!(
            "不支持的版本约束写法 {trimmed:?}：目前只支持精确版本（如 \"0.3\"）与版本下限（如 \">=0.4\"）"
        ));
    }
    if trimmed.is_empty() {
        return Err("版本约束不能是空字符串".to_string());
    }
    Ok(VersionConstraint::Exact(trimmed.to_string()))
}

/// 把一个点分版本字符串（如 `"0.4.1"`）解析成数字组件序列。任一段不是
/// 合法的非负整数就整体判定为解析失败。
fn parse_numeric_components(raw: &str) -> Option<Vec<u64>> {
    raw.split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

/// 逐段比较两个数字组件序列的大小，长度不同的一边缺的组件按 0 补齐
/// （`[0, 4]` 与 `[0, 4, 0]` 视为相等）。
fn compare_numeric_components(a: &[u64], b: &[u64]) -> Ordering {
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 精确约束的版本号完全相同时判定满足() {
        // Arrange
        let constraint = VersionConstraint::Exact("0.3".to_string());

        // Act
        let satisfied = constraint.is_satisfied_by("0.3");

        // Assert
        assert!(satisfied);
    }

    #[test]
    fn 精确约束的版本号不同时判定不满足() {
        // Arrange
        let constraint = VersionConstraint::Exact("0.3".to_string());

        // Act
        let satisfied = constraint.is_satisfied_by("0.4");

        // Assert
        assert!(!satisfied);
    }

    #[test]
    fn 下限约束的实际版本高于要求时判定满足() {
        // Arrange
        let constraint = VersionConstraint::AtLeast(vec![0, 4]);

        // Act
        let satisfied = constraint.is_satisfied_by("0.5");

        // Assert
        assert!(satisfied);
    }

    #[test]
    fn 下限约束的实际版本等于要求时判定满足() {
        // Arrange
        let constraint = VersionConstraint::AtLeast(vec![0, 4]);

        // Act
        let satisfied = constraint.is_satisfied_by("0.4");

        // Assert
        assert!(satisfied);
    }

    #[test]
    fn 下限约束的实际版本低于要求时判定不满足() {
        // Arrange
        let constraint = VersionConstraint::AtLeast(vec![0, 4]);

        // Act
        let satisfied = constraint.is_satisfied_by("0.3");

        // Assert
        assert!(!satisfied);
    }

    #[test]
    fn 下限约束遇到实际版本缺省组件按零补齐后判定满足() {
        // Arrange：要求 >=0.4，实际版本 0.4.1——补齐后 [0,4,0] 与
        // [0,4,1] 比较，后者更大，满足。
        let constraint = VersionConstraint::AtLeast(vec![0, 4]);

        // Act
        let satisfied = constraint.is_satisfied_by("0.4.1");

        // Assert
        assert!(satisfied);
    }

    #[test]
    fn 下限约束遇到无法解析成数字的实际版本时判定不满足() {
        // Arrange
        let constraint = VersionConstraint::AtLeast(vec![1, 0]);

        // Act
        let satisfied = constraint.is_satisfied_by("release-42");

        // Assert
        assert!(!satisfied);
    }

    #[test]
    fn any约束对任意版本号恒判定满足() {
        // Arrange
        let constraint = VersionConstraint::Any;

        // Act & Assert
        assert!(constraint.is_satisfied_by("0.0.1"));
        assert!(constraint.is_satisfied_by("anything"));
    }

    #[test]
    fn 解析大于等于前缀得到下限约束() {
        // Arrange & Act
        let constraint = parse_constraint(">=0.4").expect("合法的下限约束");

        // Assert
        assert_eq!(constraint, VersionConstraint::AtLeast(vec![0, 4]));
    }

    #[test]
    fn 解析不带前缀的字符串得到精确约束() {
        // Arrange & Act
        let constraint = parse_constraint("0.3").expect("合法的精确约束");

        // Assert
        assert_eq!(constraint, VersionConstraint::Exact("0.3".to_string()));
    }

    #[test]
    fn 解析大于等于前缀后跟非数字文本时返回错误() {
        // Arrange & Act
        let result = parse_constraint(">=abc");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 解析不支持的比较运算符前缀时返回错误() {
        // Arrange & Act
        let result = parse_constraint(">1.0.0");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 解析波浪号前缀时返回错误() {
        // Arrange & Act
        let result = parse_constraint("~1.0.0");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 解析空字符串时返回错误() {
        // Arrange & Act
        let result = parse_constraint("");

        // Assert
        assert!(result.is_err());
    }
}
