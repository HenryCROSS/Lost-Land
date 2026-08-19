//! 内容注册表核心。
//!
//! 「本体即 Mod」落地的枢纽：注册表本身不区分「这是本体注册的还是
//! mod 注册的」，只认命名空间字符串。本体的地形、职业、技能与 mod
//! 新增的同类内容走完全相同的这一条注册通道。
//!
//! # 注册期 / 运行期分界线（[0016](../../../knowledge/decisions/0016-mod-performance-tiers-by-declaration.md)）
//!
//! 本类型只负责注册期的字符串 ↔ 索引双向映射与内容哈希，是「一档
//! （零开销）：声明静态值 → 注册期物化进按 [`ll_core::ident::ContentIndex`]
//! 索引的平铺列」这条分档的**地基**——`Interner` 保证 `ContentIndex`
//! 从 0 开始稠密单调递增（见 `ll_core::ident::Interner::intern`
//! 的实现），因此按属性分列的物化结构（例如
//! `move_cost: Vec<u32>`，[0017](../../../knowledge/decisions/0017-tiered-declarations-materialize-columnar.md)
//! 描述的形状）可以直接用 `ContentIndex::get()` 当数组下标，不需要
//! 额外的稠密化步骤。真正把某类内容（如地形）物化成平铺列，是后续
//! 任务（P4 Task 8）在这份地基之上做的事，不属于本模块范围。
//!
//! # 校验分工（[0015](../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)）
//!
//! [`Registry::intern`] 是**注册**——由 `ll-script` 的注册函数在 Steel
//! 求值某个 mod 的定义脚本时触发，产出一个新的 `ContentIndex` 或返回
//! 已存在的。[`Registry::get`] 是**解析**——查一个字符串 ID 当前是否
//! 已注册，查不到就是规格 §10.4「缺失 mod」的检测点，不会像 `intern`
//! 那样顺手创建一条新记录。两者是完全不同的操作，混在一起会让「引用
//! 了不存在的内容」静默变成「凭空注册出这条内容」。

use ll_core::hashing::StateHasher;
use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use std::collections::HashMap;

/// 内容注册表：字符串 ID ↔ 紧凑索引，外加按 mod 命名空间统计的内容
/// 哈希。
#[derive(Debug, Default)]
pub struct Registry {
    /// 复用 `ll-core` 已有的双向映射池，不重新发明一份等价逻辑。
    interner: Interner,
    /// 按 mod 命名空间统计的内容哈希。键是命名空间（不含路径部分），
    /// 值是该命名空间贡献的全部内容 ID 的一个顺序无关摘要（见
    /// [`Self::intern`] 的说明）。
    content_hash: HashMap<String, u64>,
}

impl Registry {
    /// 建立空注册表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期调用——由 `ll-script` 的注册函数在 Steel 求值时触发。
    ///
    /// 同一个 `id` 重复注册返回相同索引（复用
    /// [`Interner::intern`](ll_core::ident::Interner::intern) 已有的
    /// 这条不变式），且**不会**因为重复注册而让该命名空间的内容哈希
    /// 发生变化——哈希摘要的是「这个命名空间贡献了哪些内容 ID」这个
    /// 集合，不是「intern 被调用了多少次」。
    pub fn intern(&mut self, id: NamespacedId) -> ContentIndex {
        let namespace = id.namespace().to_owned();
        let already_registered = self.interner.get(&id).is_some();
        let index = self.interner.intern(id.clone());

        if !already_registered {
            let item_digest = hash_namespaced_id(&id);
            // 用异或折叠而非拼接后整体哈希：折叠是可交换的，同一批
            // 内容不论以何种顺序被 intern，最终摘要恒相同——这与规格
            // C5「禁止 HashMap/HashSet 迭代顺序参与逻辑判断」是同一条
            // 精神在这里的体现：不能让「这个 mod 贡献了哪些内容」这个
            // 与顺序无关的事实,被意外做成了一个顺序敏感的计算。代价是
            // 抗碰撞性弱于把整批内容拼接后统一哈希，但这里只是「内容
            // 是否变化」的警告用途（见模块文档），不是安全或存档完整
            // 性校验，可接受。
            self.content_hash
                .entry(namespace)
                .and_modify(|digest| *digest ^= item_digest)
                .or_insert(item_digest);
        }

        index
    }

    /// 由索引反查标识符。存档写出时依赖此方法。
    pub fn resolve(&self, index: ContentIndex) -> Option<&NamespacedId> {
        self.interner.resolve(index)
    }

    /// 由标识符查索引，**不注册**。查不到就是规格 §10.4「缺失 mod」的
    /// 检测点——例如某技能声明引用的职业 ID，在跨内容校验时应当调用
    /// 这个方法而不是 [`Self::intern`]，否则「引用了不存在的内容」会
    /// 静默变成「凭空注册出这条内容」。
    pub fn get(&self, id: &NamespacedId) -> Option<ContentIndex> {
        self.interner.get(id)
    }

    /// 供加载管理界面与存档头使用：本次装载会话里，某个 mod 命名空间
    /// 贡献的全部内容的哈希——版本号相同但内容变了时用于警告（见
    /// `knowledge/design/identity-and-ids.md` 「存档与 mod 集合」①）。
    ///
    /// 该命名空间从未贡献过任何内容（未被任何 `intern` 调用触及）时
    /// 返回 `None`，与「贡献了内容但哈希恰好是 0」的合法情况区分开。
    pub fn content_hash_of(&self, namespace: &str) -> Option<u64> {
        self.content_hash.get(namespace).copied()
    }

    /// `ContentIndex` ↔ 字符串 ID 映射快照，按 `ContentIndex` 顺序
    /// （即注册顺序）排列。
    ///
    /// **为 P5 预留**：`ContentIndex` 不可持久化（依赖 mod 加载顺序，
    /// 见 `ll_core::ident` 模块文档），存档头需要写出这份快照，读档时
    /// 用它把索引换回字符串，再按当前 mod 加载顺序重新 [`Self::intern`]
    /// 一遍（[`Self::rebuild_from`]）。P4 只需要保证produce/consume 这份
    /// 快照的能力就绪，不需要真正接入存档读写——存档格式本身在 P5
    /// 冻结（`knowledge/design/identity-and-ids.md` 「存档与 mod
    /// 集合」）。
    pub fn snapshot(&self) -> Vec<NamespacedId> {
        self.interner.ids().to_vec()
    }

    /// 从一份快照重建注册表——按快照顺序依次 `intern`，因此重建后
    /// `ContentIndex` 的分配与快照顺序一一对应：`snapshot[i]` 重建后
    /// 得到的索引就是 `i`。这是 P5 读档重放的核心操作，本任务只保证
    /// 这个往返关系成立，不接入真实的存档读写流程。
    pub fn rebuild_from(snapshot: &[NamespacedId]) -> Self {
        let mut registry = Self::new();
        for id in snapshot {
            registry.intern(id.clone());
        }
        registry
    }
}

/// 对一个 `NamespacedId` 求一个确定性摘要，供 [`Registry::intern`] 累积
/// 进命名空间级别的内容哈希。
///
/// 复用 `ll-core` 已有的 [`StateHasher`]（手写 FNV-1a，跨平台跨版本
/// 恒定），不在本 crate 里另起一套哈希实现——[0017](../../../knowledge/decisions/0017-tiered-declarations-materialize-columnar.md)
/// 与本模块都需要「同一份内容在任何机器上算出同一个摘要」这条性质，
/// 两处各写一遍迟早会漂移出不一致的算法。
fn hash_namespaced_id(id: &NamespacedId) -> u64 {
    let mut hasher = StateHasher::new();
    hasher.write_bytes(id.namespace().as_bytes());
    // 命名空间与路径之间混入一个不出现在合法段落字符集里的分隔字节
    // （合法字符只有小写字母/数字/`_`/`-`/`.`），避免 `("ab", "c")` 与
    // `("a", "bc")` 这类不同的 (namespace, path) 拼接后撞出同一段字节
    // 序列。
    hasher.write_bytes(b":");
    hasher.write_bytes(id.path().as_bytes());
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    #[test]
    fn 同一命名空间字符串重复注册返回相同索引() {
        // Arrange
        let mut registry = Registry::new();
        let fireball = id("lostland:fireball");

        // Act
        let first = registry.intern(fireball.clone());
        let second = registry.intern(fireball);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 不同mod的相同路径名不冲突() {
        // 命名空间前缀天然隔离——两个 mod 各自的 "fireball" 是两个
        // 不同的内容。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let lostland_fireball = registry.intern(id("lostland:fireball"));
        let yourmod_fireball = registry.intern(id("yourmod:fireball"));

        // Assert
        assert_ne!(lostland_fireball, yourmod_fireball);
    }

    #[test]
    fn content_hash随注册内容变化而变化() {
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("yourmod:fireball"));
        let hash_before = registry.content_hash_of("yourmod");

        // Act
        registry.intern(id("yourmod:iceball"));
        let hash_after = registry.content_hash_of("yourmod");

        // Assert
        assert_ne!(hash_before, hash_after);
    }

    #[test]
    fn 重复注册同一id不改变content_hash() {
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("yourmod:fireball"));
        let hash_before = registry.content_hash_of("yourmod");

        // Act
        registry.intern(id("yourmod:fireball"));
        let hash_after = registry.content_hash_of("yourmod");

        // Assert
        assert_eq!(hash_before, hash_after);
    }

    #[test]
    fn 未贡献任何内容的命名空间查询哈希返回none() {
        // Arrange
        let registry = Registry::new();

        // Act
        let hash = registry.content_hash_of("nobody");

        // Assert
        assert_eq!(hash, None);
    }

    #[test]
    fn get查询未注册内容返回none且不注册它() {
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:fireball"));

        // Act
        let found = registry.get(&id("yourmod:never_registered"));

        // Assert
        assert_eq!(found, None);
        assert_eq!(registry.content_hash_of("yourmod"), None);
    }

    #[test]
    fn snapshot与rebuild_from往返后索引对应关系不变() {
        // Arrange
        let mut original = Registry::new();
        let mountain = original.intern(id("lostland:mountain"));
        let fireball = original.intern(id("yourmod:fireball"));
        let snapshot = original.snapshot();

        // Act
        let rebuilt = Registry::rebuild_from(&snapshot);

        // Assert：同一份 snapshot 顺序下，同一个字符串 ID 换回同一个
        // ContentIndex。
        assert_eq!(rebuilt.get(&id("lostland:mountain")), Some(mountain));
        assert_eq!(rebuilt.get(&id("yourmod:fireball")), Some(fireball));
    }

    #[test]
    fn snapshot与rebuild_from往返后内容哈希不变() {
        // 往返关系不能只在索引层面成立——存档头要靠内容哈希判断
        // 「版本号没变但内容变了」，若重建后哈希漂移，这条能力本身就
        // 不可靠。
        // Arrange
        let mut original = Registry::new();
        original.intern(id("yourmod:fireball"));
        original.intern(id("yourmod:iceball"));
        let snapshot = original.snapshot();

        // Act
        let rebuilt = Registry::rebuild_from(&snapshot);

        // Assert
        assert_eq!(
            rebuilt.content_hash_of("yourmod"),
            original.content_hash_of("yourmod")
        );
    }
}
