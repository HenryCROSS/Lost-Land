//! 职业注册表——「本体即 Mod」在职业系统上的落点（P5-B 任务 2）。
//!
//! # 照抄 `terrain.rs`/`space_profile.rs` 已验证的模式
//!
//! `ll_world::terrain`/`ll_world::space_profile` 验证过一套「私有字段 +
//! `Table::define` 注册期校验 + `materialize_base_*` 本体注册入口 +
//! `*_fixture` 测试夹具」的模式（见 `ll_world::terrain` 模块文档）。
//! 本模块走同一条路径。
//!
//! # 为什么定义本身直接落在 `ll-mod`，不像地形那样拆成两处
//!
//! 地形/层属性的定义（`TerrainDef`/`SpaceProfileDef`）之所以落在
//! `ll-world`、只让 `ll-mod` 做一层 `Registry::intern` 的薄封装，是
//! 因为它们要与 `ChunkGrid`/`Space` 等世界存储结构直接打交道，必须
//! 留在 `ll-world` 内部才不会让 `ll-world` 反向依赖 `ll-mod`（依赖顺序
//! `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`，规格 §5）。
//!
//! `ClassDef` 不依赖任何「世界空间」概念——它只是一条静态声明（主属性
//! 倾向、本地化键），`ll-mod` 本身就是可以直接持有
//! [`crate::registry::Registry`] 的那一层，没有必要为了套用地形那套
//! 「定义在下游、封装在上游」的分层，平白多出一次跨 crate 的间接。
//! 见 `knowledge/design/class-skill-quest-system.md`「与既有架构的
//! 接线点」一节。
//!
//! # 与 `Agent.profession` 的关系
//!
//! `ClassDef` 不是给 `Agent` 添加第二套「职业」概念，而是给 P3 阶段就
//! 已经建好的 `Agent.profession: ContentIndex` 字段配一张真正的注册表
//! ——`society-and-affiliation.md` 早已把 `AffiliationKind::Profession`
//! 定为「恒为 `Def`」，职业本身是类型不是实例，与 `ContentIndex` 语义
//! 完全吻合。见设计文档第一节。
//!
//! # 查询接口：`Option`，不是「安全兜底默认值」
//!
//! [`TerrainTable::blocks_sight`](ll_world::terrain::TerrainTable::blocks_sight)
//! 一类查询对未注册索引返回一个「安全默认值」（如 `false`），因为地形
//! 判定是逐格逐帧的热路径，不能为了一次查询就返回 `Option` 强迫调用方
//! 到处 `unwrap_or`。职业查询不在任何逐帧热路径上（建档选择职业、UI
//! 展示，都是低频调用），因此本模块选择更明确的 [`ClassTable::get`]
//! ——查不到就是查不到，返回 `None`，不用一个可能被误当成「真实数据」
//! 的默认值掩盖「这个索引其实没注册」这个事实（呼应 ADR 0015「注册
//! 校验是解析」——`Registry::get` 同样是查不到就返回 `None`，不创建、
//! 不兜底）。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_sim::traits::{TraitGrant, TraitGrantSource};
use ll_world::entity::AttributeKind;
use std::fmt;

/// 单条职业声明：本体与 mod 注册职业时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在职业层面的验收标的——[`materialize_base_classes`]
/// 拿这个类型的值去调用外部传入的 `intern` 回调，本体的声明与未来 mod
/// 的声明除了 `id` 里的命名空间字符串不同之外，不存在任何结构性差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDef {
    /// 命名空间标识符，例如 `lostland:warrior`、`yourmod:necromancer`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串——本地化是独立系统的职责，
    /// 不是内容注册表的。
    pub display_name_key: NamespacedId,
    /// 主属性倾向：六项主属性之一，供职业选择界面展示、以及后续批次
    /// 职业相关数值计算的输入。P5 阶段只是分类字段，不驱动结算逻辑。
    pub primary_attribute: AttributeKind,
    /// 这个职业授予的天赋引用列表——`knowledge/design/trait-system.md`
    /// 三节①「有效天赋 = 种族天赋 ∪ **职业天赋** ∪ ……」里职业这一路
    /// 来源的声明处，与 [`crate::race::RaceDef::traits`] 是同一个类型、
    /// 同一套语义，只是所有者从种族换成职业。
    ///
    /// 空列表表示这个职业不授予任何天赋（[`materialize_base_classes`]
    /// 注册的四种本体职业目前都是空的——项目所有者已裁定「本体 = 框架，
    /// 内容 = mod」，本体游戏内容将迁往一个本体自己的 mod 脚本目录，
    /// 该迁移是独立批次的工作；在它之前，本模块只提供通道，不再往
    /// `materialize_base_classes` 里新增硬编码内容）。
    ///
    /// **与种族天赋的唯一实质差异在 `unlock_level`**：种族/副职/装备/
    /// buff 恒填 `1`（"拥有即生效"，这些来源本身不随等级变化），职业
    /// 天赋则按职业自己的等级曲线填对应等级（`trait-system.md` 六节
    /// 原文：「职业天赋按实际设计填对应等级」）——但这是**内容作者填
    /// 什么值**的差异，不是**引擎怎么处理**的差异：
    /// `ll_sim::traits::effective_traits` 对两路来源跑的是同一段
    /// `level >= unlock_level` 比较，没有任何按来源分流的分支。
    ///
    /// 不出现在 [`ClassTable::define`] 的参数里，走注册后追加的
    /// [`ClassTable::add_trait_grant`]（脚本入口 `register-class-trait`，
    /// 见 [`crate::script_class_api`]）——理由同
    /// [`crate::race::RaceTable::add_trait_grant`]：`register-class`
    /// 的既有脚本签名不能改参数个数。
    pub traits: Vec<TraitGrant>,
}

/// [`ClassTable::define`] 实际存进列式存储的属性子集——不含 `id`（`id`
/// 只在注册那一刻用于换取 [`ContentIndex`]，换到之后就不再需要）。
///
/// **必须公开**：这是 [`ClassTable::define`] 唯一的参数类型，任何想
/// 直接调用 `define`（而不是走 [`materialize_base_classes`] 那条便捷
/// 路径）的调用方——包括未来 mod 自己的职业注册函数——都需要能构造这个
/// 类型。地形迁移时曾把等价类型写成模块私有，导致公开的 `define`
/// 事实上无法从模块外调用（见 `ll_world::terrain::TerrainAttrs`
/// 模块文档「必须公开」一节），这里直接吸取那次教训。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassAttrs {
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
    /// 主属性倾向。
    pub primary_attribute: AttributeKind,
    /// 这个职业授予的天赋引用列表，见 [`ClassDef::traits`] 文档。
    /// [`ClassTable::define`] 之后仍可通过
    /// [`ClassTable::add_trait_grant`] 继续追加。
    pub traits: Vec<TraitGrant>,
}

/// 职业注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来，而不是等到查询某个具体职业时才表现成怪行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassError {
    /// 同一个内容索引被定义了两次——与
    /// [`ll_world::terrain::TerrainError::DuplicateDefinition`] 同一条
    /// 纪律：`intern` 对同一个 `NamespacedId` 重复调用是幂等的（返回
    /// 同一个索引），但幂等的是「索引分配」，不是「这个索引对应的职业
    /// 属性」——两个不同的 mod（或某 mod 与本体）若都尝试给同一个 `id`
    /// 定义职业，第二次必须报错，不能静默覆盖第一次的结果。
    DuplicateDefinition(ContentIndex),
    /// 目标索引尚未 `define` 过就被 [`ClassTable::add_trait_grant`]
    /// 这类「注册后追加」的入口引用——ADR 0017「注册期完整校验」要求
    /// 在装载期就报出来，而不是静默把天赋挂在一个不存在的职业上。
    /// 与 [`crate::race::RaceError::NotDefined`] 同一条纪律。
    NotDefined(ContentIndex),
}

impl fmt::Display for ClassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClassError::DuplicateDefinition(index) => {
                write!(f, "职业索引 {} 被重复定义", index.get())
            }
            ClassError::NotDefined(index) => {
                write!(f, "职业索引 {} 尚未定义，无法追加声明", index.get())
            }
        }
    }
}

impl std::error::Error for ClassError {}

/// 一次职业查询命中的完整结果——把 [`ClassAttrs`] 的字段按引用/值
/// 打包，避免调用方对每个字段分别处理「查不到怎么办」（见模块文档
/// 「查询接口」一节，本类型只在 [`ClassTable::get`] 命中时才会出现，
/// 因此内部字段本身不需要再包一层 `Option`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassView<'a> {
    /// 指向 Fluent 本地化键。
    pub display_name_key: &'a NamespacedId,
    /// 主属性倾向。
    pub primary_attribute: AttributeKind,
    /// 这个职业授予的天赋引用列表，见 [`ClassDef::traits`] 文档。
    pub traits: &'a [TraitGrant],
}

/// 职业属性的列式存储：按 [`ContentIndex`] 下标索引，不按内容分结构
/// （ADR 0017）。
///
/// 下标空间是**全局** `ContentIndex` 号段的一部分，不是「职业专属」的
/// 连续编号——地形、空间层属性、职业、技能共享同一个
/// `Interner`/`Registry`。因此这里额外维护一份 `defined` 位图：数组
/// 下标落在表范围内不代表「这是一个职业」，只有 `defined[idx]` 为真
/// 才是。
#[derive(Debug, Default, Clone)]
pub struct ClassTable {
    display_name_key: Vec<Option<NamespacedId>>,
    primary_attribute: Vec<AttributeKind>,
    /// 每个职业授予的天赋引用列表——扁平列的一列，`Vec<Vec<..>>` 而不是
    /// `HashMap<ContentIndex, Vec<..>>`：与其余各列同一个下标空间，
    /// 遍历顺序由下标决定，不引入任何依赖哈希迭代顺序的判定（约束
    /// C5），与 `crate::race::RaceTable` 的同名列同一条理由。
    traits: Vec<Vec<TraitGrant>>,
    defined: Vec<bool>,
}

impl ClassTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上职业属性。
    ///
    /// 唯一的校验是「不得重复定义」（见 [`ClassError::DuplicateDefinition`]
    /// 文档）——职业不像地形那样有 `blocks_move`/`move_cost` 之间的
    /// 自洽性约束需要检查，主属性倾向本身没有互相矛盾的组合。
    pub fn define(&mut self, index: ContentIndex, attrs: ClassAttrs) -> Result<(), ClassError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.traits.resize(new_len, Vec::new());
            // AttributeKind 没有语义上的「默认值」，这里用 Strength
            // 只是一个占位——与 TerrainTable::move_cost 在扩容时填 0
            // 同一个理由：未定义的槽位永远被 `defined` 位图挡住，不会
            // 被外部查询实际读到。
            self.primary_attribute
                .resize(new_len, AttributeKind::Strength);
        }

        if self.defined[idx] {
            return Err(ClassError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.primary_attribute[idx] = attrs.primary_attribute;
        self.traits[idx] = attrs.traits;
        Ok(())
    }

    /// 给定的职业索引当前是否已经登记过属性。
    pub fn is_defined(&self, class: ContentIndex) -> bool {
        self.defined
            .get(class.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个职业的完整属性，未注册的索引返回 `None`（见模块文档
    /// 「查询接口」一节，对齐 ADR 0015 的解析纪律：`Registry::get`
    /// 同样是查不到就返回 `None`）。
    pub fn get(&self, class: ContentIndex) -> Option<ClassView<'_>> {
        if !self.is_defined(class) {
            return None;
        }
        let idx = class.get() as usize;
        Some(ClassView {
            display_name_key: self.display_name_key[idx]
                .as_ref()
                .expect("defined 为真时 display_name_key 必已写入"),
            primary_attribute: self.primary_attribute[idx],
            traits: &self.traits[idx],
        })
    }

    /// 追加声明「这个职业在某个等级授予某个天赋」——`register-class`
    /// 的既有脚本签名不能改参数个数（[`ClassDef::traits`] 文档），因此
    /// 天赋引用走这条独立的、注册后追加的路径，与
    /// [`crate::race::RaceTable::add_trait_grant`] 是同一个模式的第二次
    /// 应用。**追加，不是覆盖**：一个职业可以被多次调用授予多条不同的
    /// 天赋（每次调用 push 一条 `TraitGrant`）。目标索引必须已经
    /// `define` 过，否则返回 [`ClassError::NotDefined`]（ADR 0017
    /// 「注册期完整校验」）。**不校验 `grant.trait_id` 是否已经在
    /// `TraitTable` 里注册过**——与 `RaceTable::add_trait_grant` 对同一个
    /// 字段的既有处理方式一致（只 `intern` 不跨表校验存在性，是当前
    /// 代码库尚未建立跨表校验基础设施的已知简化，不是本次新引入的
    /// 松懈）。
    pub fn add_trait_grant(
        &mut self,
        class: ContentIndex,
        grant: TraitGrant,
    ) -> Result<(), ClassError> {
        if !self.is_defined(class) {
            return Err(ClassError::NotDefined(class));
        }
        self.traits[class.get() as usize].push(grant);
        Ok(())
    }
}

/// `ll_sim::traits::TraitGrantSource` 的真实实现——
/// `ll_sim::traits::effective_traits` 通过这个 impl 真正查到**职业**
/// 授予的天赋引用，是 `trait-system.md` 三节①五路来源公式里职业那一路
/// 的落点。
///
/// # 与 `RaceTable` 的同名 impl 复用同一个 trait，不是各自新开一个
///
/// ADR 0021 的判据是「有没有一份算法要被两种类型共用」，不是「两种
/// 类型对称所以该抽象」。这里确实有：`effective_traits` 那段「按
/// `unlock_level` 过滤 + 按声明顺序去重」的聚合，以及
/// `granted_skills`/`resistance_multiplier_permille`/`sneak_attack_rule`/
/// `effective_scalar_capacity` 四个建立在它之上的查询，对种族天赋与
/// 职业天赋**逐字节是同一段代码**——`TraitGrant` 的两个字段
/// （`trait_id`/`unlock_level`）在两种所有者下语义完全相同，唯一的
/// 差异是内容作者给 `unlock_level` 填什么值（种族恒填 1，职业按等级
/// 曲线填），而那是数据差异，不是算法差异。给职业新开一个
/// `ClassTraitGrantSource` trait 只会得到一份签名相同、实现相同、
/// 被同一段算法以完全相同方式调用的重复声明。
///
/// 未注册的职业索引返回空列表——与
/// `TraitGrantSource::granted_traits` 文档「查不到就是查不到」的既有
/// 纪律一致，不是 panic 或特殊分支。
impl TraitGrantSource for ClassTable {
    fn granted_traits(&self, owner: ContentIndex) -> Vec<TraitGrant> {
        self.get(owner)
            .map(|view| view.traits.to_vec())
            .unwrap_or_default()
    }
}

/// 本体基础职业在当前注册表里的索引缓存。
///
/// 只注册占位性质的少数几种基础职业——真正的职业数值平衡与内容设计
/// 不在本任务范围（`knowledge/design/class-skill-quest-system.md`
/// 文档开篇已声明：本文档与本任务只交付系统骨架，不交付具体职业该有
/// 什么数值）。
#[derive(Debug, Clone, Copy)]
pub struct BaseClassIds {
    /// 战士：力量倾向。
    pub warrior: ContentIndex,
    /// 法师：智力倾向。
    pub mage: ContentIndex,
    /// 游侠：敏捷倾向。
    pub ranger: ContentIndex,
}

/// 本体职业注册的唯一入口：本体与 mod 共用的注册路径。
///
/// `intern` 是外部传入的解析回调（生产路径是 `|id| registry.intern(id)`
/// ，测试路径是本模块的 [`base_class_fixture`]）——本函数只管「拿到一个
/// 索引后，声明它的职业属性」，不关心索引从哪个具体类型来，与
/// `ll_world::terrain::materialize_base_terrain` 同一个理由（该函数
/// 文档「与 Registry 的关系」一节）。
pub fn materialize_base_classes(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseClassIds, ClassTable), ClassError> {
    let mut table = ClassTable::new();

    let warrior = define_base(
        &mut table,
        intern,
        "lostland:warrior",
        "lostland:class.warrior.display_name",
        AttributeKind::Strength,
    )?;
    let mage = define_base(
        &mut table,
        intern,
        "lostland:mage",
        "lostland:class.mage.display_name",
        AttributeKind::Intelligence,
    )?;
    let ranger = define_base(
        &mut table,
        intern,
        "lostland:ranger",
        "lostland:class.ranger.display_name",
        AttributeKind::Dexterity,
    )?;
    Ok((
        BaseClassIds {
            warrior,
            mage,
            ranger,
        },
        table,
    ))
}

/// [`materialize_base_classes`] 的内部帮手：把一条声明的字面量字段
/// 拆开传入，换取一次 `intern` + 一次 [`ClassTable::define`]。
fn define_base(
    table: &mut ClassTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    id: &str,
    display_name_key: &str,
    primary_attribute: AttributeKind,
) -> Result<ContentIndex, ClassError> {
    let index = intern(NamespacedId::parse(id).expect("本体职业 id 字面量恒合法"));
    table.define(
        index,
        ClassAttrs {
            display_name_key: NamespacedId::parse(display_name_key)
                .expect("本体职业本地化键字面量恒合法"),
            primary_attribute,
            // 本体职业目前不授予任何天赋——项目所有者裁定「本体 = 框架，
            // 内容 = mod」，本体内容迁往脚本是独立批次的工作；在它之前
            // 这里不再新增硬编码内容，mod 通过 `register-class-trait`
            // 追加声明，见 `ClassDef::traits` 文档。
            traits: Vec::new(),
        },
    )?;
    Ok(index)
}

/// 供测试使用：现造一个空 [`Interner`]，注册本体全部基础职业，返回
/// 可用的 `(BaseClassIds, ClassTable)`。
///
/// **不是生产路径**——生产路径必须经过 [`crate::registry::Registry::intern`]（见模块
/// 文档）。这个函数只是让本 crate 的单元测试不必先搭一整套 mod 加载
/// 流程就能拿到一份内部自洽的职业表。
pub fn base_class_fixture() -> (BaseClassIds, ClassTable) {
    let mut interner = Interner::new();
    materialize_base_classes(&mut |id| interner.intern(id))
        .expect("本体职业声明表内部一致，注册恒不失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    #[test]
    fn 新建的职业表查询任意索引均为未注册() {
        // Arrange
        let table = ClassTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 战士的主属性倾向是力量() {
        // Arrange
        let (ids, table) = base_class_fixture();

        // Act
        let view = table.get(ids.warrior).expect("战士已在本体注册");

        // Assert
        assert_eq!(view.primary_attribute, AttributeKind::Strength);
    }

    #[test]
    fn 法师的主属性倾向是智力() {
        // Arrange
        let (ids, table) = base_class_fixture();

        // Act
        let view = table.get(ids.mage).expect("法师已在本体注册");

        // Assert
        assert_eq!(view.primary_attribute, AttributeKind::Intelligence);
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // 对齐 ADR 0015 的解析纪律：查不到就是查不到，不返回一个可能
        // 被误当成真实数据的兜底值。
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let table = ClassTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:warrior").expect("合法"));
        let mut table = ClassTable::new();
        table
            .define(
                index,
                ClassAttrs {
                    display_name_key: NamespacedId::parse("lostland:class.warrior.display_name")
                        .expect("合法"),
                    primary_attribute: AttributeKind::Strength,
                    traits: Vec::new(),
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            ClassAttrs {
                display_name_key: NamespacedId::parse("lostland:class.warrior.display_name")
                    .expect("合法"),
                primary_attribute: AttributeKind::Intelligence,
                traits: Vec::new(),
            },
        );

        // Assert
        assert_eq!(result, Err(ClassError::DuplicateDefinition(index)));
    }

    #[test]
    fn 本体职业与mod注册的自定义职业调用同一个公开define函数完成注册() {
        // 本体注册与 mod 注册除了命名空间字符串不同之外，没有任何
        // 结构性差异——都只是往同一个 Registry::intern 里塞一个
        // NamespacedId，再用完全相同的公开 ClassTable::define 函数
        // 登记属性，没有任何一条只对本体开放的旁路。
        //
        // 边界：本测试只证明本体与 mod 走同一条注册路径（结构等价），
        // 不能证明 mod 脚本调得到这套 API。真正的证据在
        // crate::pipeline 的脚本装载测试与 mods/example_mod/gameplay.scm。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (class_ids, mut table) = materialize_base_classes(&mut |id| registry.intern(id))
            .expect("本体职业声明表内部一致");
        let mod_id = NamespacedId::parse("yourmod:necromancer").expect("合法标识符");
        let mod_index = registry.intern(mod_id);
        table
            .define(
                mod_index,
                ClassAttrs {
                    display_name_key: NamespacedId::parse("yourmod:necromancer_display_name")
                        .expect("合法"),
                    primary_attribute: AttributeKind::Willpower,
                    traits: Vec::new(),
                },
            )
            .expect("mod 职业与本体职业调用同一个公开 define 函数,理应同样成功");

        // Assert：mod 内容紧接在本体三种职业之后分配到索引，说明两者
        // 共用同一个单调递增的号段，没有为本体预留任何特殊区间；且
        // mod 注册的职业确实能通过 get 查到正确属性。
        //
        // 卫兵（原本的第四条）已经迁进 mods/lostland/classes.scm，不再
        // 由本函数注册——见该文件头「为什么本文件里只有卫兵一条」。
        assert_eq!(mod_index.get(), class_ids.ranger.get() + 1);
        let view = table.get(mod_index).expect("mod 职业已通过 define 登记");
        assert_eq!(view.primary_attribute, AttributeKind::Willpower);
    }
    #[test]
    fn 追加天赋声明是追加而不是覆盖() {
        // 与 RaceTable::add_trait_grant 同一条纪律：一个职业可以被多次
        // 调用授予多条不同的天赋，后一次不覆盖前一次。
        // Arrange
        let mut interner = Interner::new();
        let (ids, mut table) = base_class_fixture();
        let first = interner.intern(NamespacedId::parse("yourmod:trait_a").expect("合法"));
        let second = interner.intern(NamespacedId::parse("yourmod:trait_b").expect("合法"));

        // Act
        table
            .add_trait_grant(
                ids.warrior,
                TraitGrant {
                    trait_id: first,
                    unlock_level: 1,
                },
            )
            .expect("目标职业已定义，追加应当成功");
        table
            .add_trait_grant(
                ids.warrior,
                TraitGrant {
                    trait_id: second,
                    unlock_level: 4,
                },
            )
            .expect("目标职业已定义，追加应当成功");

        // Assert：两条都在，且顺序就是调用顺序（约束 C5：聚合顺序由
        // 注册期写死的静态顺序决定）。
        let view = table.get(ids.warrior).expect("战士已在本体注册");
        assert_eq!(
            view.traits,
            &[
                TraitGrant {
                    trait_id: first,
                    unlock_level: 1,
                },
                TraitGrant {
                    trait_id: second,
                    unlock_level: 4,
                },
            ]
        );
    }

    #[test]
    fn 给尚未定义的职业索引追加天赋返回notdefined() {
        // ADR 0017「注册期完整校验」：装载期就报出来，不静默把天赋挂在
        // 一个不存在的职业上。
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let trait_id = interner.intern(NamespacedId::parse("yourmod:some_trait").expect("合法"));
        let mut table = ClassTable::new();

        // Act
        let result = table.add_trait_grant(
            never_defined,
            TraitGrant {
                trait_id,
                unlock_level: 1,
            },
        );

        // Assert
        assert_eq!(result, Err(ClassError::NotDefined(never_defined)));
    }

    #[test]
    fn 职业表作为天赋授予来源对未注册索引返回空列表() {
        // 「查不到就是查不到」——不是 panic，也不是某种兜底默认值。
        // Arrange
        let mut interner = Interner::new();
        let never_defined =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let (_, table) = base_class_fixture();

        // Act
        let grants = TraitGrantSource::granted_traits(&table, never_defined);

        // Assert
        assert!(grants.is_empty());
    }

    #[test]
    fn 职业表作为天赋授予来源返回追加进去的那条声明() {
        // 这条 impl 是 `ll_sim::traits::effective_traits` 真正查到职业
        // 天赋的唯一路径，不是一个只被测试自己调用的方法。
        // Arrange
        let mut interner = Interner::new();
        let (ids, mut table) = base_class_fixture();
        let trait_id = interner.intern(NamespacedId::parse("yourmod:sneaky").expect("合法"));
        table
            .add_trait_grant(
                ids.ranger,
                TraitGrant {
                    trait_id,
                    unlock_level: 3,
                },
            )
            .expect("目标职业已定义，追加应当成功");

        // Act
        let grants = TraitGrantSource::granted_traits(&table, ids.ranger);

        // Assert
        assert_eq!(
            grants,
            vec![TraitGrant {
                trait_id,
                unlock_level: 3,
            }]
        );
    }

    #[test]
    fn 本体注册的三种基础职业都不授予任何天赋() {
        // 本体内容迁往脚本是独立批次的工作；materialize_base_classes
        // 在那之前只提供通道、不堆内容——这条测试守住的是「别再往这里
        // 加硬编码内容」，不是「职业永远不该有天赋」。
        // Arrange
        let (ids, table) = base_class_fixture();

        // Act & Assert
        for class in [ids.warrior, ids.mage, ids.ranger] {
            assert!(
                table.get(class).expect("本体职业已注册").traits.is_empty(),
                "本体职业不应在 Rust 里硬编码天赋声明"
            );
        }
    }
}
