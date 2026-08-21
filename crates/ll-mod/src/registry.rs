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
//!
//! # 内容哈希覆盖 id 集合与字段值两部分（[0027](../../../knowledge/decisions/0027-content-hash-covers-field-values.md)）
//!
//! [`Registry::content_hash_of`] 对外呈现一个统一的按命名空间摘要，
//! 但它的产生分两步：[`Registry::intern`] 只知道字符串 id（见本节
//! 「校验分工」一段与 [`Registry::content_hash_of`] 文档「覆盖范围」一节）；
//! 字段值那一半由 [`crate::content_hash`] 模块在全部六张内容表装载
//! 完毕后通过 [`Registry::fold_content_digest`] 补上。本类型自身不
//! 认识任何一张具体内容表——那会让 `Registry` 与六张表各自的字段
//! 变化互相耦合，见 [`crate::content_hash`] 模块文档「为什么不能在
//! `intern` 内部做」一节。

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
    /// 值是该命名空间贡献的全部内容的一个顺序无关摘要——[`Self::intern`]
    /// 折入「贡献了哪些内容 id」这一半，[`Self::fold_content_digest`]
    /// 折入「这些内容各自的字段值是什么」这一半（值哈希升级，见
    /// [`Self::content_hash_of`] 文档「覆盖范围」一节）。两半用同一种
    /// 异或折叠手法叠加，结果仍是一个不依赖折叠顺序的单一摘要。
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
    /// 发生变化——本方法折入的哈希摘要的是「这个命名空间贡献了哪些
    /// 内容 ID」这个集合，不是「intern 被调用了多少次」。这只是完整
    /// 内容哈希的一半（id 集合），字段值那一半由
    /// [`Self::fold_content_digest`] 在装载完成后补上，见
    /// [`Self::content_hash_of`] 文档「覆盖范围」一节。
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
    /// # 覆盖范围：id 集合 + 字段值（值哈希升级）
    ///
    /// [`Self::intern`] 只把「这个命名空间贡献了哪些内容 id」折进这个
    /// 摘要——它在注册那一刻发生，那时候某条内容具体的字段值（伤害、
    /// 冷却、属性修正……）还没有被任何 `*Table::define` 写入,压根不
    /// 存在,无法参与哈希。真正覆盖字段值的那一半由
    /// [`Self::fold_content_digest`] 补上——`ll_mod::content_hash`
    /// 模块在全部六张内容表装载完毕后调用它一次,把每条内容的字段值
    /// 摘要按命名空间异或折叠进这里已经有的 id 摘要之上。两次折叠
    /// 用的是同一种异或手法（可交换,不依赖调用顺序）,因此最终结果
    /// 同时覆盖「id 集合变了」与「某条内容的字段值变了」两类变化——
    /// 只调用了 `intern`、还没跑值哈希那一步的调用方（本模块自身的
    /// 单元测试、`base_*_fixture` 一类测试夹具）看到的是前一半（id
    /// 集合），这是历史行为，不是缺陷；生产装载路径
    /// （`ll_game::content::load_content`）总会在返回前跑完值哈希那
    /// 一步,见 `ll_mod::content_hash` 模块文档。
    ///
    /// 该命名空间从未贡献过任何内容（未被任何 `intern` 调用触及）时
    /// 返回 `None`，与「贡献了内容但哈希恰好是 0」的合法情况区分开。
    pub fn content_hash_of(&self, namespace: &str) -> Option<u64> {
        self.content_hash.get(namespace).copied()
    }

    /// 把一份「字段值摘要」按命名空间异或折叠进已有的内容哈希。
    ///
    /// **不是替换,是叠加**——供 `ll_mod::content_hash::apply_value_hashes`
    /// 在全部六张内容表装载完毕后，对每一条已注册内容调用一次：
    /// [`Self::intern`] 早于任何 `*Table::define` 发生,那一刻还没有
    /// 字段值可以哈希（见 [`Self::content_hash_of`] 文档「覆盖范围」
    /// 一节），本方法因此不在 `intern` 内部调用,而是留给装载完成后
    /// 的一次性收尾步骤。
    ///
    /// 用的是与 [`Self::intern`] 内部完全相同的异或折叠手法：可交换、
    /// 不依赖调用顺序——同一批内容不论以什么顺序被 `fold_content_digest`
    /// 折入,同一个命名空间最终摘要恒相同（这正是「不同装载顺序产出
    /// 相同哈希」这条不变式在 API 层面的落点，测试见
    /// `ll_mod::content_hash` 模块）。命名空间此前从未在 `content_hash`
    /// 里出现过（`intern` 阶段就没贡献过任何 id）时,这里同样用
    /// `or_insert` 让它首次出现,不强求调用方先手动初始化一条零值
    /// 记录。
    pub fn fold_content_digest(&mut self, namespace: &str, digest: u64) {
        self.content_hash
            .entry(namespace.to_owned())
            .and_modify(|existing| *existing ^= digest)
            .or_insert(digest);
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
/// 直接委托 [`StateHasher::write_namespaced_id`]（长度前缀编码，见其
/// 文档「不带长度前缀」一节的碰撞论证）——`ll_mod::content_hash` 需要
/// 把字段里出现的 `ContentIndex` 解析回 `NamespacedId` 再混入,与这里
/// 「注册期只知道 id 本身」是同一份编码的两个调用点,两处共用
/// `ll-core` 这一份实现,不各自维护一套容易漂移的字节布局。
fn hash_namespaced_id(id: &NamespacedId) -> u64 {
    let mut hasher = StateHasher::new();
    hasher.write_namespaced_id(id);
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

    #[test]
    fn fold_content_digest叠加在intern已有的id摘要之上而非替换() {
        // 值哈希升级的核心不变式：fold_content_digest 是"再叠一层"，
        // 不是"重新赋值"——同一个命名空间先 intern 一个 id、再
        // fold_content_digest 一次，结果必须与"只 intern、不额外折叠"
        // 不同（新一层确实生效了），也不等于"只折叠、不 intern"（旧
        // 一层没有被覆盖丢弃）。
        // Arrange
        let mut only_intern = Registry::new();
        only_intern.intern(id("yourmod:fireball"));
        let id_only_hash = only_intern
            .content_hash_of("yourmod")
            .expect("已 intern 过内容");

        let mut interned_then_folded = Registry::new();
        interned_then_folded.intern(id("yourmod:fireball"));
        interned_then_folded.fold_content_digest("yourmod", 0xABCD);

        // Act
        let combined_hash = interned_then_folded
            .content_hash_of("yourmod")
            .expect("已 intern 且已折叠");

        // Assert：叠加后的结果与"只有 id 摘要"不同（值那一半确实生效），
        // 且异或折叠具体值可验证（不是随便什么不同的值）。
        assert_ne!(combined_hash, id_only_hash);
        assert_eq!(combined_hash, id_only_hash ^ 0xABCD);
    }

    #[test]
    fn fold_content_digest对从未intern过的命名空间也能建立记录() {
        // Registry::intern 内部 or_insert 的同一条纪律在这里同样成立
        // ——命名空间此前完全没有出现过时，第一次 fold_content_digest
        // 就应该让它出现，不强求调用方先手动插入一条零值占位。
        // Arrange
        let mut registry = Registry::new();

        // Act
        registry.fold_content_digest("brandnew", 42);

        // Assert
        assert_eq!(registry.content_hash_of("brandnew"), Some(42));
    }
}
