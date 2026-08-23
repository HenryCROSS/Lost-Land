//! mod 清单：解析、结构、错误。
//!
//! **清单格式：JSON5。** 项目所有者 2026-08-20 裁定「全用 json5 吧,
//! 还可以写注释方便日后维护」，本仓库全部手写配置格式统一成 JSON5
//! （本地化 `.ftl` 除外，那是所有者另一条裁定「i18n 就用 FTL」）——此前
//! 本模块用的是 TOML（理由曾是与规格 §11.1「用户设置 TOML」同一类
//! 「给人手改的元数据」场景），迁移到 JSON5 不改变这条理由本身：仍然
//! 不需要先起 Steel VM 就能解析出依赖关系用于拓扑排序（清单本身不该
//! 依赖脚本求值），额外换来的是注释与尾逗号——手写清单终于能写清楚
//! 「这个字段为什么这样填」，不必只靠外部文档。解析用 [`json5::from_str`]，
//! 它只做解析、不提供序列化，但本模块从不需要把 [`ModManifest`] 写回
//! 磁盘（清单永远是 mod 作者手写的输入，不是本体生成的输出），因此
//! 「只解析」这条限制不构成问题。
//!
//! # 校验分工（[0015](../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)）
//!
//! 本模块只做「结构是否合法、字符是否合法」这类**无上下文**的校验：
//! 必填字段是否存在（serde 天然拒绝）、命名空间字符集是否合法
//! （[`ll_core::ident::NamespacedId::parse`]）、依赖版本约束的**语法**
//! 是否合法（[`crate::version_constraint::parse_constraint`]，同样不
//! 需要知道「当前发现到了哪些 mod」，纯粹是文本本身合不合语法）。「这个
//! 依赖的 mod 是否真的存在」「它实际的版本是否满足约束」都是**有上下文**
//! 的校验（依赖当前发现到了哪些 mod、它们各自的 `version` 字段），本
//! 模块不做，留给 [`crate::topo::topo_sort`]——这正是 0015 定下的分工。
//!
//! # 依赖版本约束：向后兼容
//!
//! 清单里 `dependencies` 字段支持两种写法：
//!
//! ```json5
//! dependencies: ["othermod"]           // 旧版：裸命名空间列表
//! ```
//! ```json5
//! dependencies: {                      // 新版：命名空间 -> 版本约束
//!   othermod: ">=1.0.0",
//! }
//! ```
//!
//! 旧版写法不报废、不需要迁移——每一项解析成
//! [`crate::version_constraint::VersionConstraint::Any`]（只要求依赖
//! 存在，不比较版本），与这个字段加版本约束之前的既有行为完全等价。
//! `mods/example_mod/mod.json5` 当前没有 `dependencies` 字段，两种写法
//! 对它都不构成迁移压力。两种写法在 TOML 与 JSON5 之间也是同构的
//! （数组 ↔ 数组，表 ↔ 对象），从 TOML 迁移到 JSON5 没有引入新的歧义。

use crate::version_constraint::{VersionConstraint, parse_constraint};
use ll_core::error::CoreError;
use ll_core::ident::NamespacedId;
use std::collections::BTreeMap;
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
    /// 依赖的其他 mod，每项附带一条版本约束。约束的**语法**已在解析期
    /// 校验过（[`parse_manifest`]），但「依赖是否存在」「实际版本是否
    /// 满足约束」都要留给 [`crate::topo::topo_sort`] 校验——那两步都
    /// 需要「当前发现到了哪些 mod」这个上下文，本模块解析单个清单时
    /// 拿不到（0015 分工）。
    pub dependencies: Vec<ModDependency>,
    /// 脚本入口文件（`.scm`），已解析成相对清单所在目录的绝对/相对
    /// 路径——调用方不需要再自己拼目录。
    pub entry_points: Vec<PathBuf>,
    /// **结算期**脚本文件（`.scm`），路径解析方式同 [`Self::entry_points`]。
    ///
    /// # 为什么必须是另一份清单，不能复用 `entry_points`
    ///
    /// 装载期引擎与结算期引擎的能力表**刻意不兼容**：前者有
    /// `register-*` 一整套、没有任何世界查询；后者反过来。同一份
    /// 源码因此不可能在两个引擎上都编译通过——一份写着
    /// `(register-class ...)` 的文件喂给结算期引擎，会被白名单当场
    /// 判成自由标识符。这不是本字段引入的限制，是那道隔离墙本来的
    /// 样子（`mods/example_mod/behavior.scm` 之所以刻意不在
    /// `entry_points` 里，正是同一条原因，见该文件头注释）。
    ///
    /// 于是事件监听天然分成两半：**声明**（`(on-event 事件 处理函数名)`）
    /// 是装载期动作，写在 `entry_points` 的某个文件里；**实现**
    /// （`(define (处理函数) ...)`）是结算期代码，写在本字段列出的
    /// 文件里。见 `crate::event`/`crate::script_event_source` 两处
    /// 模块文档。
    ///
    /// 允许为空（绝大多数 mod 不监听任何事件）。
    pub event_scripts: Vec<PathBuf>,
}

/// 一条依赖声明：依赖哪个 mod、要求它满足什么版本约束。
///
/// 拆成独立结构体而不是继续用 `Vec<String>`——版本约束加进来之后，
/// 「依赖了谁」与「要求什么版本」是两个必须一起搬运的信息，元组或并行
/// 数组都会让调用点靠位置隐式对应，容易在重构时错位；命名字段把这层
/// 关联在类型上表达出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModDependency {
    /// 依赖的 mod 命名空间，裸字符串（不含 [`MOD_SELF_PATH`] 路径段）。
    pub namespace: String,
    /// 版本约束。旧版裸命名空间列表清单格式（见模块文档「依赖版本约束：
    /// 向后兼容」一节）产出 [`VersionConstraint::Any`]。
    pub constraint: VersionConstraint,
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
    /// 依赖存在，但版本不满足声明的约束（精确版本不相等，或版本下限
    /// 不满足）。
    ///
    /// 与 [`Self::MissingDependency`] 是同一类「这条依赖边不可用」的
    /// 失败——依赖压根不存在、与依赖存在但版本不对，都可能意味着依赖
    /// 方调用了目标 mod 里实际不存在的能力，因此按同一严重级别处理：
    /// 见 `knowledge/design/mod-package-structure.md` 五节「版本不满足
    /// 时自动降级/跳过该依赖继续加载——否决」。
    ///
    /// `Box` 是 `clippy::result_large_err` 的要求：两个 `NamespacedId`
    /// 加两段 `String` 让这个变体明显大于本枚举其余变体，装箱后
    /// `ModError` 整体大小不再被这一个变体拖累，字段访问仍然透明（`Box`
    /// 自动解引用）。
    IncompatibleDependencyVersion(Box<DependencyVersionMismatch>),
}

/// [`ModError::IncompatibleDependencyVersion`] 携带的详情。
///
/// # 为什么没有一句现成的中文/英文句子
///
/// 与 `ll_content::load_error::ModSetMismatch`（存档硬门禁的版本不匹配
/// 错误）同一条纪律（规格 §11.3）：只有结构化字段与一个 Fluent 消息 id
/// （[`Self::message_key`]），没有任何拼好的自然语言句子——[`ModError`]
/// 的 `Display` 实现只把这些字段原样打印，服务开发者/日志，真正给玩家
/// 看的文案留给 UI 层查 `message_key` 在 `locales/<lang>.ftl` 里拼出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyVersionMismatch {
    /// Fluent 消息 id——不是自然语言文案，是查 `.ftl` 用的技术标识符，
    /// UI 层负责拼出真正给玩家看的句子。当前唯一取值是
    /// [`MOD_DEPENDENCY_VERSION_MISMATCH_MESSAGE_KEY`]，独立成一个字段
    /// 是为了与 `ModSetMismatch` 的既有形状保持一致，也预留了未来若
    /// 需要按约束种类（精确/下限）拆成不同文案时的扩展点。
    pub message_key: &'static str,
    /// 声明了这条依赖约束的 mod（哪个 mod 要求）。
    pub dependent: NamespacedId,
    /// 依赖目标 mod（要求的是谁的版本）。
    pub dependency: NamespacedId,
    /// 声明要求的版本约束展示文本（如 `">=0.4"` 或 `"0.3"`）。
    pub required: String,
    /// 依赖目标实际声明的版本号。
    pub actual: String,
}

/// [`DependencyVersionMismatch::message_key`] 唯一取值：mod 之间的依赖
/// 版本约束未满足。
///
/// 与存档硬门禁的
/// `ll_content::load_error::SAVE_MOD_VERSION_MISMATCH_MESSAGE_KEY` 是两
/// 个不同的键——即便中文措辞看起来相似，两者回答的问题不同（见
/// [`crate::topo`] 模块文档「与存档 mod 集合硬门禁的关系」一节），UI 层
/// 需要拼出不同的句子，因此不能共用同一个消息 id。
pub const MOD_DEPENDENCY_VERSION_MISMATCH_MESSAGE_KEY: &str = "mod-dependency-version-mismatch";

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
            // 不拼一句现成的中文/英文句子——见 DependencyVersionMismatch
            // 文档「为什么没有一句现成的中文/英文句子」一节：这里只把
            // 结构化字段原样打印，真正给玩家看的文案留给 UI 层查
            // message_key。与 `ll_content::load_error::LoadError` 对
            // `ModSetMismatch` 的处理是同一套写法。
            ModError::IncompatibleDependencyVersion(detail) => write!(f, "{detail:?}"),
        }
    }
}

impl core::error::Error for ModError {}

/// 从清单文件反序列化出的原始结构，字段与磁盘上的 JSON5 一一对应。
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
    /// 依赖的其他 mod。允许缺省为空（多数 mod 无依赖），两种 JSON5
    /// 形状都接受，见 [`RawDependencies`]。
    #[serde(default)]
    dependencies: RawDependencies,
    /// 脚本入口文件相对路径。允许缺省为空（纯数据 mod 可以没有脚本）。
    #[serde(default)]
    entry_points: Vec<String>,
    /// 结算期脚本文件相对路径。允许缺省为空（绝大多数 mod 不监听
    /// 任何运行期事件），见 [`ModManifest::event_scripts`]。
    #[serde(default)]
    event_scripts: Vec<String>,
}

/// 清单里 `dependencies` 字段的两种合法 JSON5 形状——向后兼容旧版裸命名
/// 空间列表，同时支持新版「命名空间 -> 版本约束」对象（见模块文档「依赖
/// 版本约束：向后兼容」一节）。
///
/// 用 `#[serde(untagged)]` 而不是手写 `Deserialize` 去探测输入形状：
/// 两种 JSON5 语法本身互斥（数组用 `[...]`，对象用 `{...}`），serde
/// 依次尝试两个变体本身就能无歧义地区分，不需要额外的判别逻辑。
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum RawDependencies {
    /// 旧版形式：`dependencies: ["othermod"]`。每一项只是命名空间，
    /// 不附带版本要求——解析后统一变成 [`VersionConstraint::Any`]。
    List(Vec<String>),
    /// 新版形式：`dependencies: { othermod: ">=1.0.0" }`。键是
    /// 命名空间，值是版本约束原始文案，交给
    /// [`crate::version_constraint::parse_constraint`] 解析。用
    /// `BTreeMap` 而不是 `HashMap`——遍历顺序必须是命名空间字典序这一
    /// 确定性顺序（规格 C5），`BTreeMap` 天然满足，不需要额外排序。
    Table(BTreeMap<String, String>),
}

impl Default for RawDependencies {
    /// 缺省为空列表——两种形状里，空列表是唯一同时也是空表的那个，
    /// `#[serde(default)]` 缺省值选哪个变体因此没有歧义。
    fn default() -> Self {
        RawDependencies::List(Vec::new())
    }
}

impl RawDependencies {
    /// 归一化成统一的「(命名空间, 原始约束文案)」序列——旧版列表的每
    /// 一项没有约束文案（`None`，后续解析成
    /// [`VersionConstraint::Any`]），新版表的每一项带着原始文案交给
    /// [`parse_constraint`] 解析。
    fn into_pairs(self) -> Vec<(String, Option<String>)> {
        match self {
            RawDependencies::List(names) => names.into_iter().map(|name| (name, None)).collect(),
            RawDependencies::Table(table) => table
                .into_iter()
                .map(|(name, raw)| (name, Some(raw)))
                .collect(),
        }
    }
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

    let raw: RawManifest = json5::from_str(&content)
        .map_err(|err| ModError::ParseError(format!("{}: {err}", path.display())))?;

    let id = mod_self_id(&raw.namespace).map_err(|err| {
        ModError::ParseError(format!(
            "{}: 非法命名空间 {:?}: {err}",
            path.display(),
            raw.namespace
        ))
    })?;

    // 依赖名字符集、版本约束语法都必须合法，否则 topo_sort 无法把它们
    // 当成可比较的命名空间/约束使用。这仍然是无上下文的结构校验（合不
    // 合法不依赖「当前加载了哪些 mod」），因此放在这里而不是 topo_sort。
    let mut dependencies = Vec::new();
    for (dep_namespace, raw_constraint) in raw.dependencies.into_pairs() {
        mod_self_id(&dep_namespace).map_err(|err| {
            ModError::ParseError(format!(
                "{}: 非法依赖命名空间 {dep_namespace:?}: {err}",
                path.display()
            ))
        })?;

        let constraint = match raw_constraint {
            None => VersionConstraint::Any,
            Some(text) => parse_constraint(&text).map_err(|reason| {
                ModError::ParseError(format!(
                    "{}: mod {dep_namespace:?} 的依赖版本约束非法: {reason}",
                    path.display()
                ))
            })?,
        };

        dependencies.push(ModDependency {
            namespace: dep_namespace,
            constraint,
        });
    }

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let entry_points = raw.entry_points.iter().map(|p| base_dir.join(p)).collect();
    let event_scripts = raw.event_scripts.iter().map(|p| base_dir.join(p)).collect();

    Ok(ModManifest {
        id,
        version: raw.version,
        dependencies,
        entry_points,
        event_scripts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;
    use std::fs;

    /// 在临时目录下写一个 [`crate::discover::MANIFEST_FILENAME`]
    /// 并返回其路径，供各测试复用。
    fn write_manifest(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join(crate::discover::MANIFEST_FILENAME);
        fs::write(&path, content).expect("测试临时文件写入不应失败");
        path
    }

    #[test]
    fn 合法清单解析出预期的字段() {
        // Arrange
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"{
                namespace: "yourmod",
                version: "0.1.0",
                dependencies: ["othermod"],
                entry_points: ["main.scm"],
            }"#,
        );

        // Act
        let manifest = parse_manifest(&path).expect("这是合法清单");

        // Assert
        assert_eq!(manifest.id, mod_self_id("yourmod").unwrap());
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(
            manifest.dependencies,
            vec![ModDependency {
                namespace: "othermod".to_string(),
                constraint: VersionConstraint::Any,
            }]
        );
        assert_eq!(manifest.entry_points, vec![dir.path().join("main.scm")]);
    }

    #[test]
    fn 带注释与尾逗号的清单解析出与紧凑写法相同的字段() {
        // JSON5 相对 JSON 的两项核心增益——项目所有者选它正是为了这两样
        // （见模块文档「清单格式：JSON5」一节）：手写清单能加解释性注释，
        // 结尾多余的逗号不会让解析报错。本测试直接验证两者都被
        // `json5::from_str` 正确处理，不只是文档里空口宣称。
        // Arrange
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"{
                // 命名空间必须全小写，见 NamespacedId::parse。
                namespace: "yourmod",
                version: "0.1.0", // 版本号原样保留，不做语义化解析
            }"#,
        );

        // Act
        let manifest = parse_manifest(&path).expect("注释与尾逗号不应导致解析失败");

        // Assert
        assert_eq!(manifest.id, mod_self_id("yourmod").unwrap());
        assert_eq!(manifest.version, "0.1.0");
    }

    #[test]
    fn 旧版裸命名空间依赖列表解析出any约束保持向后兼容() {
        // 简报要求的核心结论：旧版 `dependencies: [...]` 写法不报废、
        // 不需要迁移——每一项解析成 VersionConstraint::Any（只要求依赖
        // 存在，不比较版本）。
        // Arrange
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"{
                namespace: "yourmod",
                version: "0.1.0",
                dependencies: ["othermod"],
            }"#,
        );

        // Act
        let manifest = parse_manifest(&path).expect("旧版格式不应报废");

        // Assert
        assert_eq!(
            manifest.dependencies,
            vec![ModDependency {
                namespace: "othermod".to_string(),
                constraint: VersionConstraint::Any,
            }]
        );
    }

    #[test]
    fn 新版依赖表清单解析出对应的精确与下限约束() {
        // Arrange：BTreeMap 遍历顺序恒是键的字典序（"atleastmod" <
        // "exactmod"），因此断言不需要额外排序。
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"{
                namespace: "yourmod",
                version: "0.1.0",
                dependencies: {
                    exactmod: "0.3",
                    atleastmod: ">=0.4",
                },
            }"#,
        );

        // Act
        let manifest = parse_manifest(&path).expect("这是合法的新版依赖表清单");

        // Assert
        assert_eq!(
            manifest.dependencies,
            vec![
                ModDependency {
                    namespace: "atleastmod".to_string(),
                    constraint: VersionConstraint::AtLeast(vec![0, 4]),
                },
                ModDependency {
                    namespace: "exactmod".to_string(),
                    constraint: VersionConstraint::Exact("0.3".to_string()),
                },
            ]
        );
    }

    #[test]
    fn 依赖版本约束写法不受支持时清单解析失败() {
        // Arrange：波浪号前缀不在支持范围内（见 version_constraint
        // 模块文档「支持的语法：只有两种，YAGNI」一节）。
        let dir = tempdir();
        let path = write_manifest(
            dir.path(),
            r#"{
                namespace: "yourmod",
                version: "0.1.0",
                dependencies: { othermod: "~1.0.0" },
            }"#,
        );

        // Act
        let result = parse_manifest(&path);

        // Assert
        assert!(matches!(result, Err(ModError::ParseError(_))));
    }

    #[test]
    fn 缺少版本号字段时解析失败() {
        // Arrange
        let dir = tempdir();
        let path = write_manifest(dir.path(), r#"{ namespace: "yourmod" }"#);

        // Act
        let result = parse_manifest(&path);

        // Assert
        assert!(matches!(result, Err(ModError::ParseError(_))));
    }

    #[test]
    fn 缺少命名空间字段时解析失败() {
        // Arrange
        let dir = tempdir();
        let path = write_manifest(dir.path(), r#"{ version: "0.1.0" }"#);

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
        let path = write_manifest(dir.path(), r#"{ namespace: "YourMod", version: "0.1.0" }"#);

        // Act
        let result = parse_manifest(&path);

        // Assert
        assert!(matches!(result, Err(ModError::ParseError(_))));
    }

    #[test]
    fn 清单文件不存在时返回io错误() {
        // Arrange
        let dir = tempdir();
        let missing = dir.path().join(crate::discover::MANIFEST_FILENAME);

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
            r#"{ namespace: "barebones", version: "0.0.1" }"#,
        );

        // Act
        let manifest = parse_manifest(&path).expect("依赖与入口应可缺省");

        // Assert
        assert!(manifest.dependencies.is_empty() && manifest.entry_points.is_empty());
    }
}
