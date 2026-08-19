//! mod 清单：解析、结构、错误。
//!
//! **清单格式：TOML。** 规格没有明文规定 mod 清单格式，这是本任务的
//! 实现假设，理由是与规格 §11.1「用户设置 TOML」同一类「给人手改的
//! 元数据」场景，且不需要先起 Steel VM 就能解析出依赖关系用于拓扑
//! 排序（清单本身不该依赖脚本求值）。若与预期不符，评审时可调整。
//!
//! # 校验分工（[0015](../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)）
//!
//! 本模块只做「结构是否合法、字符是否合法」这类**无上下文**的校验：
//! 必填字段是否存在（serde 天然拒绝）、命名空间字符集是否合法
//! （[`ll_core::ident::NamespacedId::parse`]）。「这个依赖的 mod 是否
//! 真的存在」是**有上下文**的校验（依赖当前发现到了哪些 mod），本模块
//! 不做，留给 [`crate::topo::topo_sort`]——这正是 0015 定下的分工。

use ll_core::error::CoreError;
use ll_core::ident::NamespacedId;
use std::fmt;
use std::path::{Path, PathBuf};

/// mod 自我标识时使用的保留路径段。
///
/// [`ModManifest::id`] 的类型是 [`NamespacedId`]（`命名空间:路径`），
/// 复用它是为了让「mod 本身」与「mod 内部的一件具体内容」共享同一套
/// 标识符机制与校验规则（本体即 Mod 的又一处体现：连"这是一个 mod"
/// 这件事本身也走命名空间 ID，不是另起一套专属类型）。但 mod 清单里
/// 天然只有一个裸命名空间字符串（例如 `yourmod`），没有「路径」这个
/// 概念——`self` 是为此保留的固定路径段，意为「这个 ID 指的是 mod
/// 自己，不是它内部注册的某件具体内容」。依赖声明（[`ModManifest::dependencies`]
/// 里的每一项）引用的也是「某个 mod 整体」，因此报错时（
/// [`ModError::MissingDependency`]）同样按这个约定拼出 `NamespacedId`。
pub(crate) const MOD_SELF_PATH: &str = "self";

/// 把一个裸命名空间字符串（如 `"yourmod"`）按 [`MOD_SELF_PATH`] 约定
/// 拼成「指代 mod 自身」的 [`NamespacedId`]。
///
/// 复用 [`NamespacedId::parse`] 而不是自己重写字符合法性规则——命名
/// 空间字符集的定义只应该有一处，多处各自实现同一套规则迟早会漂移。
pub(crate) fn mod_self_id(namespace: &str) -> Result<NamespacedId, CoreError> {
    NamespacedId::parse(&format!("{namespace}:{MOD_SELF_PATH}"))
}

/// 一个 mod 的清单：身份、版本、依赖、脚本入口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModManifest {
    /// mod 自己的命名空间，按 [`MOD_SELF_PATH`] 约定包装成
    /// [`NamespacedId`]。
    pub id: NamespacedId,
    /// 版本号，原样保留 mod 作者填写的字符串，不做语义化版本解析——
    /// 版本比较不是本任务范围，[0017](../../../knowledge/decisions/0017-tiered-declarations-materialize-columnar.md)
    /// 之外的存档/内容哈希才是判断「内容是否变化」的可靠依据（见
    /// `knowledge/design/identity-and-ids.md` 「存档与 mod 集合」①）。
    pub version: String,
    /// 依赖的其他 mod，裸命名空间字符串（不含 `MOD_SELF_PATH` 路径
    /// 段）。是否存在留给 [`crate::topo::topo_sort`] 校验（0015 分工）。
    pub dependencies: Vec<String>,
    /// 脚本入口文件（`.scm`），已解析成相对清单所在目录的绝对/相对
    /// 路径——调用方不需要再自己拼目录。
    pub entry_points: Vec<PathBuf>,
}

/// mod 清单相关操作可能产生的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModError {
    /// 读取清单文件失败，附带路径与原始错误文案。
    ///
    /// 不直接存 [`std::io::Error`]：它不实现 `PartialEq`/`Eq`，会让
    /// 本枚举无法派生它们，而测试断言「哪个 mod 失败」需要比较
    /// `ModError` 值——手写 `Display`/`Error`（而非引入 `thiserror`）
    /// 是本项目一贯的错误类型写法（`ll-core::error::CoreError`
    /// 同理）。
    Io(String),
    /// 清单结构或字符不合法（缺字段、非法命名空间字符等）。
    ParseError(String),
    /// 依赖成环，附带环路上具体的 mod（按环路顺序），供加载管理界面
    /// （任务 11）逐条展示。
    CyclicDependency(Vec<NamespacedId>),
    /// 依赖的某个 mod 未被发现，附带该依赖自身的标识符。
    MissingDependency(NamespacedId),
    /// 两个（或更多）已发现的清单声明了同一个命名空间。
    ///
    /// **这是简报要求正面处理的已知缺口**：`topo_sort` 旧版实现用
    /// `HashMap<&str, usize>` 把命名空间映射到清单下标，重复命名空间
    /// 会静默地「后者覆盖前者」——玩家看到的是某个 mod 的内容莫名其妙
    /// 被另一个同名 mod 顶替，作者本人却毫无察觉。见 [`crate::topo`]
    /// 模块文档「重复命名空间」一节。
    DuplicateNamespace(NamespacedId),
}

impl fmt::Display for ModError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModError::Io(msg) => write!(f, "读取 mod 清单失败: {msg}"),
            ModError::ParseError(msg) => write!(f, "解析 mod 清单失败: {msg}"),
            ModError::CyclicDependency(cycle) => {
                write!(f, "mod 依赖成环: ")?;
                for (i, id) in cycle.iter().enumerate() {
                    if i > 0 {
                        write!(f, " -> ")?;
                    }
                    write!(f, "{id}")?;
                }
                Ok(())
            }
            ModError::MissingDependency(id) => write!(f, "缺失依赖 mod: {id}"),
            ModError::DuplicateNamespace(id) => write!(
                f,
                "命名空间 {} 被多个已发现的 mod 重复声明，拒绝加载以避免不确定选中哪一份定义",
                id.namespace()
            ),
        }
    }
}

impl core::error::Error for ModError {}

/// 从清单文件反序列化出的原始结构，字段与磁盘上的 TOML 一一对应。
///
/// 与 [`ModManifest`] 分开是 0015 分工的直接体现：本结构只负责
/// 「字符串 ↔ 结构」这一层，`namespace` 字段还没有被解析成
/// [`NamespacedId`]、`dependencies` 里的字符串也还没有被拿去核对
/// 「是否已发现」——那些是下一步的事。
#[derive(Debug, serde::Deserialize)]
struct RawManifest {
    /// mod 的裸命名空间，必填——mod 没有名字就无法被其他 mod 依赖，
    /// 也无法在内容哈希表里定位自己贡献的内容。
    namespace: String,
    /// 版本号，必填。
    version: String,
    /// 依赖的其他 mod 命名空间。允许缺省为空（多数 mod 无依赖）。
    #[serde(default)]
    dependencies: Vec<String>,
    /// 脚本入口文件相对路径。允许缺省为空（纯数据 mod 可以没有脚本）。
    #[serde(default)]
    entry_points: Vec<String>,
}

/// 解析单个 mod 的清单文件。
///
/// 只做结构与字符合法性校验（见模块文档的分工说明），不校验依赖是否
/// 真实存在——那一步依赖「当前发现到了哪些 mod」，属于
/// [`crate::topo::topo_sort`] 的职责。这个划分正是四道防线第④条
/// 「加载分阶段隔离」在解析这一步的落点：单个清单解析失败，不影响
/// 调用方继续尝试解析其他 mod 的清单。
pub fn parse_manifest(path: &Path) -> Result<ModManifest, ModError> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| ModError::Io(format!("{}: {err}", path.display())))?;

    let raw: RawManifest = toml::from_str(&content)
        .map_err(|err| ModError::ParseError(format!("{}: {err}", path.display())))?;

    let id = mod_self_id(&raw.namespace).map_err(|err| {
        ModError::ParseError(format!(
            "{}: 非法命名空间 {:?}: {err}",
            path.display(),
            raw.namespace
        ))
    })?;

    // 依赖名字符集必须合法，否则 topo_sort 无法把它当成一个可比较的
    // 命名空间使用。这仍然是无上下文的结构校验（字符是否合法不依赖
    // 「当前加载了哪些 mod」），因此放在这里而不是 topo_sort。
    for dep in &raw.dependencies {
        mod_self_id(dep).map_err(|err| {
            ModError::ParseError(format!(
                "{}: 非法依赖命名空间 {dep:?}: {err}",
                path.display()
            ))
        })?;
    }

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let entry_points = raw.entry_points.iter().map(|p| base_dir.join(p)).collect();

    Ok(ModManifest {
        id,
        version: raw.version,
        dependencies: raw.dependencies,
        entry_points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;
    use std::fs;

    /// 在临时目录下写一个 `mod.toml` 并返回其路径，供各测试复用。
    fn write_manifest(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("mod.toml");
        fs::write(&path, content).expect("测试临时文件写入不应失败");
        path
    }

    #[test]
    fn 合法清单解析出预期的字段() {
        // Arrange
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"
            namespace = "yourmod"
            version = "0.1.0"
            dependencies = ["othermod"]
            entry_points = ["main.scm"]
            "#,
        );

        // Act
        let manifest = parse_manifest(&path).expect("这是合法清单");

        // Assert
        assert_eq!(manifest.id, mod_self_id("yourmod").unwrap());
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.dependencies, vec!["othermod".to_string()]);
        assert_eq!(manifest.entry_points, vec![dir.path().join("main.scm")]);
    }

    #[test]
    fn 缺少版本号字段时解析失败() {
        // Arrange
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"
            namespace = "yourmod"
            "#,
        );

        // Act
        let result = parse_manifest(&path);

        // Assert
        assert!(matches!(result, Err(ModError::ParseError(_))));
    }

    #[test]
    fn 缺少命名空间字段时解析失败() {
        // Arrange
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"
            version = "0.1.0"
            "#,
        );

        // Act
        let result = parse_manifest(&path);

        // Assert
        assert!(matches!(result, Err(ModError::ParseError(_))));
    }

    #[test]
    fn 命名空间含大写字母时解析失败() {
        // 复用 NamespacedId 的字符合法性规则（强制小写），不重新定义
        // 一套自己的规则。
        // Arrange
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"
            namespace = "YourMod"
            version = "0.1.0"
            "#,
        );

        // Act
        let result = parse_manifest(&path);

        // Assert
        assert!(matches!(result, Err(ModError::ParseError(_))));
    }

    #[test]
    fn 清单文件不存在时返回io错误() {
        // Arrange
        let dir = tempdir();
        let missing = dir.path().join("mod.toml");

        // Act
        let result = parse_manifest(&missing);

        // Assert
        assert!(matches!(result, Err(ModError::Io(_))));
    }

    #[test]
    fn 不含依赖与入口时缺省为空列表() {
        // Arrange
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"
            namespace = "barebones"
            version = "0.0.1"
            "#,
        );

        // Act
        let manifest = parse_manifest(&path).expect("依赖与入口应可缺省");

        // Assert
        assert!(manifest.dependencies.is_empty() && manifest.entry_points.is_empty());
    }
}
