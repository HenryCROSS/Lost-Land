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
//! # 本体三个基础职业的定义已经搬进 `mods/lostland/classes.json5`
//!
//! 本模块此前还有一对 `materialize_base_classes`/`base_class_fixture`：
//! 前者把战士/法师/游侠三条声明的字段值写死在 Rust 字面量里，后者是
//! 它的测试夹具。**那个函数从来没有进过生产装载路径**——
//! `ll_game::content::load_content` 给出的是一张空 `ClassTable::new()`，
//! 它的唯一调用方是 `ll-content` 的 p5 验收 demo 与本模块自己的单元
//! 测试，也就是说真实游戏里一条职业内容都没有过。项目所有者裁定
//! 「迁移吧，工作要做好」之后，两者一并删除，三条职业改由
//! `mods/lostland/classes.json5` 调用与任何第三方 mod 完全相同的
//! `register-class` 注册，并第一次真正进到生产装载路径里。
//!
//! 留下来的是 [`BaseClassIds`]（句柄，保住使用点的编译期安全）与
//! [`resolve_base_classes`]（装载后按 id 逐字段解析这个句柄，缺一条就
//! 整批失败）——见 [`crate::base_contract`] 模块文档。
//!
//! # 哪些内容进 [`BaseClassIds`]
//!
//! 判据是「Rust 代码有没有按名字引用它」，不是「它是不是本体内容」。
//! 战士/法师/游侠三条进：p5 验收链路与未来的建档界面按字段名引用它们。
//! 卫兵（`lostland:guard`）不进：Rust 侧一行代码都没提过它，它只被
//! `ll_mod::native_behavior` 的卫兵那棵树按索引
//! 引用。给一条没有 Rust 使用点的内容加一个句柄字段，只会造出一条
//! 「声明了但从没接线」——本项目已经发现三十处同形缺陷。它仍然受
//! [`crate::content_audit`] 的字段覆盖与内容值哈希两道检查覆盖。
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

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::traits::{TraitGrant, TraitGrantSource};
use ll_world::entity::AttributeKind;
use std::fmt;

use crate::base_contract::{BaseContractError, BaseContractResolver};
use crate::registry::Registry;

/// 单条职业声明：本体与 mod 注册职业时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在职业层面的验收标的——本体的声明与第三方 mod
/// 的声明除了 `id` 里的命名空间字符串不同之外，不存在任何结构性差异，
/// 两者走的是同一个 `register-class` 脚本入口（见
/// [`crate::script_class_api`]）。
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
    /// 空列表表示这个职业不授予任何天赋——`mods/lostland/classes.json5`
    /// 里四条本体职业目前都是空的，见 `ll_mod::content_audit` 里
    /// `ClassAttrs::traits` 那条豁免：本体内容不为了让字段覆盖检查
    /// 变绿硬塞一条天赋。字段本身不是死的，`mods/example_mod/` 的
    /// `examplemod:rogue` 真的用了它。
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
/// 直接调用 `define`（而不是走 `register-class` 那条脚本路径）的
/// 调用方——包括未来 mod 自己的职业注册函数——都需要能构造这个
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

/// 本体基础职业在当前注册表里的索引缓存——**句柄，不是内容**。
///
/// 三条职业的字段值（显示名键、主属性倾向）已经搬进
/// `mods/lostland/classes.json5`，本结构体只保住**使用点的编译期安全**：
/// `content.class_ids.warrior` 这行代码里字段没了就编译不过，没有任何
/// 字符串拼写错误的空间。填充由 [`resolve_base_classes`] 在装载完成后
/// 按 id 逐字段解析完成，缺任何一条整批失败。
///
/// 只有三条基础职业——真正的职业数值平衡与内容设计不在本任务范围
/// （`knowledge/design/class-skill-quest-system.md` 文档开篇已声明：
/// 本文档与本任务只交付系统骨架，不交付具体职业该有什么数值）。
/// `lostland:guard`（卫兵）刻意**不**在本结构体里：Rust 侧没有任何
/// 代码按名字引用它，它只被 `ll_mod::native_behavior` 的
/// `self-has-profession?` 按字符串引用，句柄结构体的存在理由（保住
/// Rust 使用点的编译期安全）对它不成立——见模块文档「哪些内容进
/// [`BaseClassIds`]」一节。
#[derive(Debug, Clone, Copy)]
pub struct BaseClassIds {
    /// 战士：力量倾向。
    pub warrior: ContentIndex,
    /// 法师：智力倾向。
    pub mage: ContentIndex,
    /// 游侠：敏捷倾向。
    pub ranger: ContentIndex,
}

/// 本体三个基础职业的 id 字面量——[`resolve_base_classes`] 的契约
/// 清单，同时也是 `mods/lostland/classes.json5` 必须注册哪几条内容的
/// 唯一权威来源。
///
/// 抽成常量而不是把字符串直接写在 [`resolve_base_classes`] 里，理由同
/// [`crate::race`] 的 `BASE_RACE_IDS`：集成测试
/// （`crates/ll-mod/tests/base_mod_class_skill_quest.rs`）要按同一份
/// 清单核对脚本真的注册了它们，两处各写一份字面量迟早会分叉。
const BASE_CLASS_IDS: [(&str, &str); 3] = [
    ("BaseClassIds::warrior", "lostland:warrior"),
    ("BaseClassIds::mage", "lostland:mage"),
    ("BaseClassIds::ranger", "lostland:ranger"),
];

/// 装载完成后解析本体职业契约：按 id 逐字段填充 [`BaseClassIds`]，
/// 缺任何一条就整批失败。
///
/// 取代了原先的 `materialize_base_classes`/`base_class_fixture`，理由
/// 与 [`crate::race::resolve_base_races`] 逐字相同（见其文档「这个函数
/// 取代了原先的 `materialize_base_races`」与「失败是常态分支」两节）：
/// 本函数**不注册任何内容**，只查询——本体职业与第三方 mod 职业现在
/// 走的是完全相同的 `register-class` 脚本通道。
pub fn resolve_base_classes(
    registry: &Registry,
    table: &ClassTable,
) -> Result<BaseClassIds, BaseContractError> {
    let mut resolver = BaseContractResolver::new("本体职业", registry);
    let mut resolved = BASE_CLASS_IDS
        .iter()
        .map(|(field, id)| resolver.require(field, id, |index| table.is_defined(index)));
    // 顺序与 BASE_CLASS_IDS 的声明顺序一一对应；长度由类型（`[_; 3]`）
    // 钉死，少一条就编译不过。
    let warrior = resolved.next().expect("BASE_CLASS_IDS 恒有三条");
    let mage = resolved.next().expect("BASE_CLASS_IDS 恒有三条");
    let ranger = resolved.next().expect("BASE_CLASS_IDS 恒有三条");
    drop(resolved);
    resolver.finish()?;

    Ok(BaseClassIds {
        warrior,
        mage,
        ranger,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_contract::MissingReason;

    /// 一张现造的、与本体内容无关的职业表。
    ///
    /// 本模块的单元测试验的是 [`ClassTable`] 这套**机制**（`define`/
    /// `get`/追加天赋声明/[`TraitGrantSource`] 依赖倒置 impl），不是
    /// 「本体有哪几个职业、主属性各是什么」——后者的定义已经搬进
    /// `mods/lostland/classes.json5`，由
    /// `crates/ll-mod/tests/base_mod_class_skill_quest.rs` 端到端逐字段
    /// 核对。这里刻意用 `testmod:` 命名空间现造两条测试数据，理由同
    /// [`crate::race`] 的 `sample_table`：在 Rust 里再埋一份本体内容
    /// 字面量，恰恰是本次迁移要消除的那种「同一份内容存在两处」。
    fn sample_table() -> (Registry, ContentIndex, ContentIndex, ClassTable) {
        let mut registry = Registry::new();
        let mut table = ClassTable::new();

        let define = |registry: &mut Registry,
                      table: &mut ClassTable,
                      id: &str,
                      primary_attribute: AttributeKind| {
            let index = registry.intern(NamespacedId::parse(id).expect("合法标识符"));
            table
                .define(
                    index,
                    ClassAttrs {
                        display_name_key: NamespacedId::parse("testmod:display_name")
                            .expect("合法标识符"),
                        primary_attribute,
                        traits: Vec::new(),
                    },
                )
                .expect("首次定义应当成功");
            index
        };

        let bruiser = define(
            &mut registry,
            &mut table,
            "testmod:bruiser",
            AttributeKind::Strength,
        );
        let scholar = define(
            &mut registry,
            &mut table,
            "testmod:scholar",
            AttributeKind::Intelligence,
        );

        (registry, bruiser, scholar, table)
    }

    /// 把 [`BASE_CLASS_IDS`] 三条全部注册进一张表——[`resolve_base_classes`]
    /// 成功路径的最小前置。**不是**本体内容的第二份定义：这里只用到
    /// id，字段值全部填测试占位值，真实字段值只存在于
    /// `mods/lostland/classes.json5`。
    fn registry_with_all_base_classes() -> (Registry, ClassTable) {
        let mut registry = Registry::new();
        let mut table = ClassTable::new();
        for (_, id) in BASE_CLASS_IDS {
            let index = registry.intern(NamespacedId::parse(id).expect("合法标识符"));
            table
                .define(
                    index,
                    ClassAttrs {
                        display_name_key: NamespacedId::parse("testmod:display_name")
                            .expect("合法标识符"),
                        primary_attribute: AttributeKind::Strength,
                        traits: Vec::new(),
                    },
                )
                .expect("首次定义应当成功");
        }
        (registry, table)
    }

    #[test]
    fn 新建的职业表查询任意索引均为未注册() {
        // Arrange
        let table = ClassTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 已定义的职业能查回它声明的主属性倾向() {
        // Arrange
        let (_registry, bruiser, scholar, table) = sample_table();

        // Act & Assert
        assert_eq!(
            table.get(bruiser).expect("已定义").primary_attribute,
            AttributeKind::Strength
        );
        assert_eq!(
            table.get(scholar).expect("已定义").primary_attribute,
            AttributeKind::Intelligence
        );
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // 对齐 ADR 0015 的解析纪律：查不到就是查不到，不返回一个可能
        // 被误当成真实数据的兜底值。
        // Arrange
        let mut registry = Registry::new();
        let never_defined =
            registry.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let table = ClassTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let (_registry, bruiser, _scholar, mut table) = sample_table();

        // Act
        let result = table.define(
            bruiser,
            ClassAttrs {
                display_name_key: NamespacedId::parse("testmod:display_name").expect("合法"),
                primary_attribute: AttributeKind::Intelligence,
                traits: Vec::new(),
            },
        );

        // Assert
        assert_eq!(result, Err(ClassError::DuplicateDefinition(bruiser)));
    }

    #[test]
    fn 本体职业与mod职业共用同一个单调递增号段没有预留区间() {
        // 本体注册与 mod 注册除了命名空间字符串不同之外，没有任何
        // 结构性差异——本体职业现在走 `mods/lostland/classes.json5` 的
        // `register-class`，与任何第三方 mod 逐字节是同一条通道，这条
        // 测试守住的是索引分配这一半：没有为本体预留任何特殊区间。
        // Arrange
        let (mut registry, _bruiser, scholar, mut table) = sample_table();

        // Act
        let mod_index =
            registry.intern(NamespacedId::parse("yourmod:necromancer").expect("合法标识符"));
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
            .expect("mod 职业与先注册的职业调用同一个公开 define 函数,理应同样成功");

        // Assert
        assert_eq!(mod_index.get(), scholar.get() + 1);
        let view = table.get(mod_index).expect("mod 职业已通过 define 登记");
        assert_eq!(view.primary_attribute, AttributeKind::Willpower);
    }

    #[test]
    fn 追加天赋声明是追加而不是覆盖() {
        // 与 RaceTable::add_trait_grant 同一条纪律：一个职业可以被多次
        // 调用授予多条不同的天赋，后一次不覆盖前一次。
        // Arrange
        let (mut registry, bruiser, _scholar, mut table) = sample_table();
        let first = registry.intern(NamespacedId::parse("yourmod:trait_a").expect("合法"));
        let second = registry.intern(NamespacedId::parse("yourmod:trait_b").expect("合法"));

        // Act
        table
            .add_trait_grant(
                bruiser,
                TraitGrant {
                    trait_id: first,
                    unlock_level: 1,
                },
            )
            .expect("目标职业已定义，追加应当成功");
        table
            .add_trait_grant(
                bruiser,
                TraitGrant {
                    trait_id: second,
                    unlock_level: 4,
                },
            )
            .expect("目标职业已定义，追加应当成功");

        // Assert：两条都在，且顺序就是调用顺序（约束 C5：聚合顺序由
        // 注册期写死的静态顺序决定）。
        let view = table.get(bruiser).expect("已定义");
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
        let mut registry = Registry::new();
        let never_defined =
            registry.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));
        let trait_id = registry.intern(NamespacedId::parse("yourmod:some_trait").expect("合法"));
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
        let (mut registry, _bruiser, _scholar, table) = sample_table();
        let never_defined =
            registry.intern(NamespacedId::parse("yourmod:never_defined").expect("合法标识符"));

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
        let (mut registry, _bruiser, scholar, mut table) = sample_table();
        let trait_id = registry.intern(NamespacedId::parse("yourmod:sneaky").expect("合法"));
        table
            .add_trait_grant(
                scholar,
                TraitGrant {
                    trait_id,
                    unlock_level: 3,
                },
            )
            .expect("目标职业已定义，追加应当成功");

        // Act
        let grants = TraitGrantSource::granted_traits(&table, scholar);

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
    fn 三条本体职业都在时契约解析成功且返回真实索引() {
        // Arrange
        let (registry, table) = registry_with_all_base_classes();

        // Act
        let ids = resolve_base_classes(&registry, &table).expect("三条都在，解析应当成功");

        // Assert
        assert_eq!(
            registry.resolve(ids.warrior).map(|id| id.to_string()),
            Some("lostland:warrior".to_string())
        );
        assert_eq!(
            registry.resolve(ids.mage).map(|id| id.to_string()),
            Some("lostland:mage".to_string())
        );
        assert_eq!(
            registry.resolve(ids.ranger).map(|id| id.to_string()),
            Some("lostland:ranger".to_string())
        );
    }

    #[test]
    fn 本体职业一条都没注册时契约解析一次列出全部三条() {
        // 这正是「玩家误删 mods/lostland/」的表现——一次列全，不是
        // 补一条重启再被告知缺下一条，见 crate::base_contract 模块文档。
        // Arrange
        let registry = Registry::new();
        let table = ClassTable::new();

        // Act
        let error = resolve_base_classes(&registry, &table).expect_err("空注册表必须解析失败");

        // Assert
        assert_eq!(error.contract, "本体职业");
        assert_eq!(error.required, 3);
        assert_eq!(
            error
                .missing
                .iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
            vec!["lostland:warrior", "lostland:mage", "lostland:ranger"]
        );
    }

    #[test]
    fn 职业id只被intern没被define时契约解析报notdefined() {
        // 「别的脚本把这个 id 当跨表引用写了字符串，却没有人真的注册
        // 它」——两层判定的第二层，见 crate::base_contract 模块文档。
        // Arrange
        let mut registry = Registry::new();
        for (_, id) in BASE_CLASS_IDS {
            registry.intern(NamespacedId::parse(id).expect("合法标识符"));
        }
        let table = ClassTable::new();

        // Act
        let error =
            resolve_base_classes(&registry, &table).expect_err("只 intern 未 define 必须失败");

        // Assert
        assert!(
            error
                .missing
                .iter()
                .all(|entry| entry.reason == MissingReason::NotDefined)
        );
    }
}
