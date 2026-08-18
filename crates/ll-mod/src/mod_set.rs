//! mod 集合双记录：生成期集合与当前集合，类型层面强制区分。
//!
//! 落地 `knowledge/design/identity-and-ids.md` 「存档与 mod 集合」一节
//! 最要紧的一条：世界由种子完全决定这句话**只在内容集合相同的前提下
//! 成立**。种族分布场、聚落选址、势力形成全部读取 mod 注册的定义；
//! 同一个种子换一批 mod，生成出来的是完全不同的世界。存档因此必须
//! 分开记录两组 mod 集合：
//!
//! ```text
//! 生成期 mod 集合   ← 这个世界是用这一批 mod 生成的，写入后永久不变
//! 当前 mod 集合     ← 玩家现在实际开着的这一批
//! ```
//!
//! 若只存一份，玩家中途装了个新 mod，那个世界就再也复现不出来——
//! **种子分享、缺陷复现、回归测试全部失效**。这个区分一旦等到 P5
//! 存档格式冻结后再补，就是一次追不回旧档的存档迁移，因此本任务在
//! 类型层面提前把这条区分钉死。
//!
//! # 本阶段范围（裁定 P4-3）
//!
//! P4 还没有世界生成器（P6 才落地），「生成期 mod 集合」目前没有真正
//! 的生成事件可以绑定。本模块**只做类型层面的区分**，不接入任何真实
//! 的存档读写（存档格式本身在 P5 冻结）：[`GenerationModSet`] 与
//! [`CurrentModSet`] 是两个不同的类型，编译期即可发现「把当前集合错
//! 当成生成期集合传参」这类混用，但「世界生成时刻绑定生成期集合」这
//! 条真正的语义要等 P6 世界生成落地才有意义——**这不是完整实现，是
//! 接口占位**。
//!
//! 故意尝试混用两种集合会编译失败，用于把这条约束钉在文档里而不是
//! 只写在注释中：
//!
//! ```compile_fail
//! # use ll_mod::mod_set::{CurrentModSet, GenerationModSet};
//! fn needs_generation_set(_set: GenerationModSet) {}
//!
//! let current = CurrentModSet(Vec::new());
//! needs_generation_set(current); // 类型不匹配，编译失败
//! ```

use crate::manifest::ModManifest;
use crate::registry::Registry;
use ll_core::ident::NamespacedId;

/// 一次 mod 装载的快照：命名空间、版本、内容哈希。
///
/// 内容哈希取自 [`Registry::content_hash_of`]（见 Task 7）——存档比对
/// 时，版本号相同但内容哈希不同就是「mod 作者改内容没改版本号」的
/// 信号，见 `knowledge/design/identity-and-ids.md` 「存档与 mod
/// 集合」①。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModSetEntry {
    /// mod 自己的标识符（沿用 [`crate::manifest`] 「namespace:self」
    /// 的约定）。
    pub id: NamespacedId,
    /// mod 作者填写的版本号，原样保留。
    pub version: String,
    /// 该 mod 命名空间贡献的内容哈希，`0` 表示该 mod 本次装载未贡献
    /// 任何内容（或注册表里确实查不到——两者当前不做区分，属于本任务
    /// 未处理的边界，见模块级 rustdoc 之外的实现说明）。
    pub content_hash: u64,
}

/// 生成期 mod 集合：世界是用这一批 mod 生成的，写入存档后永久不变。
///
/// 只有这一份能用来复现世界——种子分享、缺陷复现、回归测试都依赖
/// 「同一个种子 + 同一批内容 ⇒ 同一个世界」这条前提，而这份集合正是
/// 「同一批内容」的锚点。真正的绑定时机（世界生成那一刻，把当时的
/// mod 集合封存进这个类型）留给 P6 世界生成器，本任务只定形状。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationModSet(pub Vec<ModSetEntry>);

/// 当前 mod 集合：玩家现在实际开着的这一批，会随时间漂移。
///
/// 玩家中途装卸 mod 是常见操作，这份集合因此**不满足**
/// [`GenerationModSet`] 「同一个种子 + 同一批内容 ⇒ 同一个世界」那条
/// 前提——两者不能互相替代，这正是本模块要把它们做成不同类型的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentModSet(pub Vec<ModSetEntry>);

impl CurrentModSet {
    /// 从「本次装载已排序的清单」与「装载过程中产出的注册表」派生出
    /// 当前 mod 集合。
    ///
    /// 之所以两个都要——清单（[`ModManifest`]，Task 6 产物）知道
    /// 「命名空间」与「版本号」，注册表（[`Registry`]，Task 7 产物）
    /// 知道「这个命名空间实际贡献了哪些内容、哈希是多少」，`mod_set`
    /// 本身不重新解析任何一方已经算出的东西，只做归并。
    pub fn derive_from(registry: &Registry, manifests: &[ModManifest]) -> Self {
        let entries = manifests
            .iter()
            .map(|manifest| ModSetEntry {
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                content_hash: registry
                    .content_hash_of(manifest.id.namespace())
                    .unwrap_or(0),
            })
            .collect();
        CurrentModSet(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::mod_self_id;
    use ll_core::ident::NamespacedId;
    use std::path::PathBuf;

    fn manifest(namespace: &str, version: &str) -> ModManifest {
        ModManifest {
            id: mod_self_id(namespace).expect("测试用命名空间恒合法"),
            version: version.to_string(),
            dependencies: Vec::new(),
            entry_points: Vec::<PathBuf>::new(),
        }
    }

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    #[test]
    fn 当前装载的mod集合可以从registry派生出currentmodset() {
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("yourmod:fireball"));
        let manifests = vec![manifest("yourmod", "0.1.0")];

        // Act
        let current = CurrentModSet::derive_from(&registry, &manifests);

        // Assert
        assert_eq!(
            current,
            CurrentModSet(vec![ModSetEntry {
                id: mod_self_id("yourmod").unwrap(),
                version: "0.1.0".to_string(),
                content_hash: registry.content_hash_of("yourmod").unwrap(),
            }])
        );
    }

    #[test]
    fn 未贡献内容的mod派生出的条目哈希为零() {
        // Arrange：mod 已发现、已排序，但本次装载没有注册任何内容
        // （例如清单声明了脚本入口，但脚本尚未被求值/尚未调用注册
        // 函数——这属于 ll-script 的职责，不在本任务范围内）。
        let registry = Registry::new();
        let manifests = vec![manifest("emptymod", "1.0.0")];

        // Act
        let current = CurrentModSet::derive_from(&registry, &manifests);

        // Assert
        assert_eq!(current.0[0].content_hash, 0);
    }

    #[test]
    fn 多个mod各自派生出独立的条目且顺序与清单一致() {
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("a:one"));
        registry.intern(id("b:two"));
        let manifests = vec![manifest("a", "1.0.0"), manifest("b", "2.0.0")];

        // Act
        let current = CurrentModSet::derive_from(&registry, &manifests);

        // Assert
        let namespaces: Vec<&str> = current.0.iter().map(|e| e.id.namespace()).collect();
        assert_eq!(namespaces, vec!["a", "b"]);
    }
}
