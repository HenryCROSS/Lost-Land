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
//! # 绑定时机：世界创建时刻（P5 任务 4 定案）
//!
//! 这条注释曾经写「留给 P6 世界生成器」——规格插入新 P6（物品与装备）
//! 后，真正的历史世界生成器现排到了 P7，若继续按这句话字面理解，
//! [`GenerationModSet::capture`] 就要一直等到 P7 才有地方调用，但那样
//! 会让存档格式在 P5 冻结时缺一块本该有的东西。真正需要绑定的时刻是
//! **世界创建**（`WorldState::new` 之前的建档流程）,不是「世界生成器
//! 跑完」——两者不是同一件事：本体自带的默认地形生成从 P2 起就存在,
//! 「世界生成器」特指 P7 的历史/势力生成,不生成也可以先有一个世界。
//! [`GenerationModSet::capture`] 因此在 P5 任务 4 就已落地并可调用，
//! 调用点是任意「新建世界」流程紧接着 `Registry` 装载完成之后,一次性
//! 调用、写入存档头后永久不变——不是每次读档都重新计算。
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
    /// 该 mod 命名空间贡献的内容哈希，取自
    /// [`Registry::content_hash_of`]——`None` 表示该命名空间从未贡献过
    /// 任何内容，`Some(hash)` 表示贡献了内容且哈希是 `hash`（`hash`
    /// 本身理论上也可能恰好是 `0`，与「从未贡献」是两件不同的事）。
    ///
    /// # 为什么不是 `u64`（P5 任务 4 修复的债务）
    ///
    /// `Registry::content_hash_of` 自己已经用 `Option<u64>` 区分「从未
    /// 贡献」与「贡献了内容、哈希恰好是 0」这两种情况——旧版本这里用
    /// `u64` 加 `.unwrap_or(0)` 把两者折叠成同一个值，导致「mod 彻底
    /// 失效、从有内容变成无内容」与「mod 从始至终都没贡献过任何内容」
    /// 在存档比对时看起来完全一样，削弱了任务 7 mod 内容变化判定的
    /// 精度。这里保留 `content_hash_of` 已经提供的区分，不在下游提前
    /// 丢弃它。
    pub content_hash: Option<u64>,
}

/// 生成期 mod 集合：世界是用这一批 mod 生成的，写入存档后永久不变。
///
/// 只有这一份能用来复现世界——种子分享、缺陷复现、回归测试都依赖
/// 「同一个种子 + 同一批内容 ⇒ 同一个世界」这条前提，而这份集合正是
/// 「同一批内容」的锚点。绑定时机是世界创建时刻（见模块文档「绑定
/// 时机」一节），由 [`Self::capture`] 落地。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationModSet(pub Vec<ModSetEntry>);

impl GenerationModSet {
    /// 世界创建时刻调用一次：把当次 mod 装载的 `Registry` 与清单封存为
    /// 生成期集合，此后永久不变。
    ///
    /// 与 [`CurrentModSet::derive_from`] 完全同构（内部直接委托，不重复
    /// 一份归并逻辑）——两者的区别只在**调用时机**的语义,不在归并算法
    /// 本身：`derive_from` 允许随时重新调用反映"当前"状态,`capture`
    /// 只应该在世界创建那一刻调用一次,调用方（未来的建档流程）必须把
    /// 结果写入存档头后不再重新计算，见模块文档。
    pub fn capture(registry: &Registry, manifests: &[ModManifest]) -> Self {
        let CurrentModSet(entries) = CurrentModSet::derive_from(registry, manifests);
        GenerationModSet(entries)
    }
}

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
                content_hash: registry.content_hash_of(manifest.id.namespace()),
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
                content_hash: registry.content_hash_of("yourmod"),
            }])
        );
    }

    #[test]
    fn 未贡献内容的mod派生出的条目哈希为空() {
        // Arrange：mod 已发现、已排序，但本次装载没有注册任何内容
        // （例如清单声明了脚本入口，但脚本尚未被求值/尚未调用注册
        // 函数——这属于 ll-script 的职责，不在本任务范围内）。
        //
        // 断言的是 None 而不是零——「从未贡献任何内容」与「贡献了内容
        // 但哈希恰好是零」是两件不同的事（P5 任务 4 修复的债务，见
        // ModSetEntry::content_hash 文档）。
        let registry = Registry::new();
        let manifests = vec![manifest("emptymod", "1.0.0")];

        // Act
        let current = CurrentModSet::derive_from(&registry, &manifests);

        // Assert
        assert_eq!(current.0[0].content_hash, None);
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

    #[test]
    fn capture与derive_from对同一份输入产出相同条目() {
        // capture 内部直接委托 derive_from（同一份归并逻辑），这里锁住
        // 这条委托关系本身——两者对相同的 registry/manifests 必须产出
        // 逐字段相等的结果。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let manifests = vec![manifest("lostland", "0.1.0")];

        // Act
        let generation = GenerationModSet::capture(&registry, &manifests);
        let CurrentModSet(current_entries) = CurrentModSet::derive_from(&registry, &manifests);

        // Assert
        assert_eq!(generation.0, current_entries);
    }

    #[test]
    fn 生成期集合封存后不受registry后续变化影响() {
        // 落地"绑定时机是世界创建时刻"这条语义：capture 按值拷贝当时的
        // 归并结果，后续继续往 registry 里注册新内容,不会让已经封存的
        // GenerationModSet 跟着变。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let manifests = vec![manifest("lostland", "0.1.0")];
        let generation = GenerationModSet::capture(&registry, &manifests);
        let hash_at_capture = generation.0[0].content_hash;

        // Act：世界创建之后,玩家继续加载内容,registry 的内容哈希改变。
        registry.intern(id("lostland:river"));
        let current_after_change = CurrentModSet::derive_from(&registry, &manifests);

        // Assert：封存的那份没有跟着变,当前集合已经不同。
        assert_eq!(generation.0[0].content_hash, hash_at_capture);
        assert_ne!(current_after_change.0[0].content_hash, hash_at_capture);
    }
}
