//! 天赋注册表——落地 `knowledge/design/trait-system.md` 四节「天赋归谁
//! 所有」的核心形状：`TraitDef` 是一个独立内容类型，被种族/职业/副职/
//! 载具/buff 通过 `Vec<TraitGrant>`（`ll_sim::traits::TraitGrant`）
//! 引用，不是让每个所有者类型各自长一份效果字段（见该节「为什么——
//! 真正共享的算法是什么」一节的完整论证）。
//!
//! # 照抄 `race.rs`/`skill.rs` 已验证的模式
//!
//! 私有字段 + `TraitTable::define` 注册期完整校验（ADR 0017）+
//! `TraitView`/`TraitAttrs` 一读一写两个薄视图，与 [`crate::race`]/
//! [`crate::skill`] 同一套列式存储手法——本模块不是第一次证明这套模式
//! 好用，是第 N 次复用。
//!
//! # 本批次范围：四类效果的形状定义齐全，接线①授予技能与④授予资源池容量
//!
//! `trait-system.md` 三节列出天赋的四类效果——①授予技能、②属性修正、
//! ③改变规则本身（[`RuleModifier`]）、④授予资源池容量
//! （[`ResourcePoolGrant`]）。[`TraitDef`] 四个字段一次性按设计文档
//! 定形（省得将来加字段又要动一次已冻结的结构）——天赋系统落地批次
//! 只接线①，资源池落地批次（第一批：法力池/血池）在此基础上追加接线
//! ④：`impl TraitCatalog for TraitTable` 现在同时搬运 `granted_skills`/
//! `granted_resource_pools` 两个字段，`ll_sim::resource_pool::effective_scalar_capacity`
//! 据此聚合出角色对某个标量池的有效容量,见该函数文档。`stat_modifiers`
//! 走既有的 `Agent::active_stat_modifiers` 通道（该节②：「不是新机制,
//! 是同一份数据被两种消费方式使用」,接线是另一批的工作)；
//! `rule_modifiers` 里的 [`RuleModifier::Resistance`]（伤害类别/抗性
//! 接线批次新增）现在有真实消费者——`ll_sim::traits::resistance_multiplier_permille`
//! 通过下方 `impl TraitCatalog for TraitTable` 读到这个字段；其余三个
//! 变体（`RerollOnce`/`Advantage`/`Disadvantage`）仍然没有消费者,见
//! [`RuleModifier`] 文档「本批次接线状态」一节。
//!
//! # `register-trait` 脚本签名为什么只暴露①，不是设计文档的完整六参数
//!
//! `trait-system.md` 四节给出的 `register-trait` 示意签名带四个效果
//! 参数（granted-skills/stat-modifiers/rule-modifiers/
//! granted-resource-pools）。本模块的 `traits.json5` 只
//! 实现前两个位置参数（`id`/`display-name-key`）+ `granted-skills`
//! 列表——`stat_modifiers`/`rule_modifiers` 在 Rust 结构体里已经声明好
//! 形状（本模块），但脚本层还没有为"列表套元组"（`stat-modifiers`）与
//! "打标签的构造子"（`rule-modifiers` 的 `(resistance ...)`/
//! `(reroll-once ...)`）这两种 FFI 编码约定过怎么做——本代码库现有全部
//! `register-*` 函数（`skill`/`class`/`quest`/`race`/`clip`/`xp_curve`）
//! 都只用「扁平参数 + 字符串标签」这一种约定（见
//! `skills.json5` 模块文档「为什么这里多出两处 FFI 转换上的
//! 麻烦」一节），没有任何一处示范过"列表里每一项本身还是一个结构"要
//! 怎么从 `SteelVal` 转换。凭空发明一种新约定服务两个当前没有 resolve
//! 侧消费者的字段,不是这一批的份内工作（YAGNI）——`register-race-xp-reward`
//! 相对 `register-race`「不改既有签名,新增能力用新函数」的先例已经
//! 证明这条路走得通,②③两类效果的脚本入口留给各自真正接线的批次,用
//! 同一条先例补上。
//!
//! ④授予资源池容量走的正是这条「新增能力用新函数」先例——本模块新增
//! `register-trait-resource-pool`（`traits.json5`），与
//! `register-race-trait` 相对 `register-race` 同一个模式：追加声明
//! 「这个天赋授予某个池多少容量」，不改 `register-trait` 已有的三参数
//! 签名。容量公式（`fixed`/`by-level`）走扁平参数 + 字符串标签，理由
//! 与本节其余部分一致，见该函数文档。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
pub use ll_sim::resource_pool::{CapacityFormula, CapacityValue, ResourcePoolGrant};
pub use ll_sim::traits::RuleModifier;
use ll_sim::traits::{TraitCatalog, TraitRule};
use ll_world::entity::AttributeKind;

/// 三节④「资源池容量」——一条"这个天赋授予多少这种资源池容量"的声明,
/// 按 `resource-pools-and-rest.md` 三节末尾原文落地（`trait-system.md`
/// 三节④）。**类型定义现移居 `ll_sim::resource_pool`**（资源池落地
/// 批次，第一批：法力池/血池）——`effective_scalar_capacity` 需要在
/// `resolve` 侧消费它，而 `ll-sim` 不能反过来依赖 `ll-mod`（依赖
/// 方向），本模块因此改为文件顶部 `pub use` 复用同一份声明，不再维护
/// 会漂移的副本，见 `ll_sim::resource_pool` 模块文档「为什么这些类型
/// 定义在 `ll-sim`」一节——与 `crate::skill` 现在直接复用
/// `ll_sim::skill::ResourceCost`/`SkillEffect` 是同一条先例。
///
/// 单条天赋声明：本体与 mod 注册天赋时共用的同一个输入形状——
/// 「本体即 Mod」在天赋层面的验收标的，理由同 [`crate::race::RaceDef`]
/// 文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitDef {
    /// 命名空间标识符，例如 `lostland:dwarven_resilience`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串——与 `RaceDef`/`ClassDef`
    /// 同一条既有惯例。
    pub display_name_key: NamespacedId,
    /// ①授予技能——`trait-system.md` 三节①「有效技能=并集」公式里
    /// 天赋这一路贡献的技能集合，本批次唯一接线到 `resolve` 的效果。
    pub granted_skills: Vec<ContentIndex>,
    /// ②属性修正——格式复用 `vehicle-and-mounting.md` 六节的
    /// `(属性, 增量)` 对列表；消费通道由所有者是否"可剥夺"决定（种族
    /// 走烘焙、职业/副职/装备/buff 走 `active_stat_modifiers`），见
    /// `trait-system.md` 三节②，本批次不接线，见模块文档「本批次
    /// 范围」一节。
    pub stat_modifiers: Vec<(AttributeKind, i32)>,
    /// ③改变规则本身——见 [`RuleModifier`] 文档,本批次不接线。
    pub rule_modifiers: Vec<RuleModifier>,
    /// ④授予资源池容量——见 [`ResourcePoolGrant`] 文档,本批次不接线。
    pub granted_resource_pools: Vec<ResourcePoolGrant>,
}

/// [`TraitTable::define`] 实际存进列式存储的属性子集——不含 `id`，
/// 理由同 [`crate::race::RaceAttrs`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitAttrs {
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
    /// 授予的技能。
    pub granted_skills: Vec<ContentIndex>,
    /// 属性修正。
    pub stat_modifiers: Vec<(AttributeKind, i32)>,
    /// 规则修正。
    pub rule_modifiers: Vec<RuleModifier>,
    /// 授予的资源池容量。
    pub granted_resource_pools: Vec<ResourcePoolGrant>,
}

/// 天赋注册期可能出现的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraitError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::race::RaceError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// `add_resource_pool_grant` 的目标天赋索引尚未通过 `define`
    /// 注册——理由同 [`crate::race::RaceError::NotDefined`]：追加声明
    /// 必须先有一个已登记的天赋可以追加。
    NotDefined(ContentIndex),
}

impl fmt::Display for TraitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraitError::DuplicateDefinition(index) => {
                write!(f, "天赋索引 {} 被重复定义", index.get())
            }
            TraitError::NotDefined(index) => {
                write!(f, "天赋索引 {} 尚未通过 register-trait 注册", index.get())
            }
        }
    }
}

impl std::error::Error for TraitError {}

/// 一次天赋查询命中的完整结果，理由同 [`crate::race::RaceView`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitView<'a> {
    /// 指向 Fluent 本地化键。
    pub display_name_key: &'a NamespacedId,
    /// 授予的技能。
    pub granted_skills: &'a [ContentIndex],
    /// 属性修正。
    pub stat_modifiers: &'a [(AttributeKind, i32)],
    /// 规则修正。
    pub rule_modifiers: &'a [RuleModifier],
    /// 授予的资源池容量。
    pub granted_resource_pools: &'a [ResourcePoolGrant],
}

/// 天赋属性的列式存储：按 [`ContentIndex`] 下标索引，与
/// [`crate::race::RaceTable`] 同一套道理——下标空间是全局
/// `ContentIndex` 号段的一部分，因此同样维护一份 `defined` 位图。
#[derive(Debug, Default, Clone)]
pub struct TraitTable {
    display_name_key: Vec<Option<NamespacedId>>,
    granted_skills: Vec<Vec<ContentIndex>>,
    stat_modifiers: Vec<Vec<(AttributeKind, i32)>>,
    rule_modifiers: Vec<Vec<RuleModifier>>,
    granted_resource_pools: Vec<Vec<ResourcePoolGrant>>,
    defined: Vec<bool>,
}

impl TraitTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上天赋属性。
    pub fn define(&mut self, index: ContentIndex, attrs: TraitAttrs) -> Result<(), TraitError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.granted_skills.resize(new_len, Vec::new());
            self.stat_modifiers.resize(new_len, Vec::new());
            self.rule_modifiers.resize(new_len, Vec::new());
            self.granted_resource_pools.resize(new_len, Vec::new());
        }

        if self.defined[idx] {
            return Err(TraitError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.granted_skills[idx] = attrs.granted_skills;
        self.stat_modifiers[idx] = attrs.stat_modifiers;
        self.rule_modifiers[idx] = attrs.rule_modifiers;
        self.granted_resource_pools[idx] = attrs.granted_resource_pools;
        Ok(())
    }

    /// 给定的天赋索引当前是否已经登记过属性。
    pub fn is_defined(&self, trait_id: ContentIndex) -> bool {
        self.defined
            .get(trait_id.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个天赋的完整属性，未注册的索引返回 `None`（ADR 0015）。
    pub fn get(&self, trait_id: ContentIndex) -> Option<TraitView<'_>> {
        if !self.is_defined(trait_id) {
            return None;
        }
        let idx = trait_id.get() as usize;
        Some(TraitView {
            display_name_key: self.display_name_key[idx]
                .as_ref()
                .expect("defined 为真时 display_name_key 必已写入"),
            granted_skills: &self.granted_skills[idx],
            stat_modifiers: &self.stat_modifiers[idx],
            rule_modifiers: &self.rule_modifiers[idx],
            granted_resource_pools: &self.granted_resource_pools[idx],
        })
    }

    /// 追加声明「这个天赋授予某个资源池多少容量」——`register-trait-resource-pool`
    /// 的写入目标，与 [`crate::race::RaceTable::add_trait_grant`] 同一个
    /// 「新增能力用新函数」模式：不改 `define`/`register-trait` 已有的
    /// 签名，一个天赋可以被多次调用追加多条 `ResourcePoolGrant`（同一个
    /// 天赋同时授予多个不同池的容量，或者——虽然内容作者通常不会这样
    /// 声明——同一个池被同一个天赋声明两次,`effective_scalar_capacity`
    /// 会把两条都计入求和,不在这里去重,理由同 `stat_modifiers` 允许
    /// 重复声明同一属性两次各自叠加）。
    pub fn add_resource_pool_grant(
        &mut self,
        trait_id: ContentIndex,
        grant: ResourcePoolGrant,
    ) -> Result<(), TraitError> {
        if !self.is_defined(trait_id) {
            return Err(TraitError::NotDefined(trait_id));
        }
        self.granted_resource_pools[trait_id.get() as usize].push(grant);
        Ok(())
    }

    /// 追加声明「这个天赋在 `level` 级授予某个法术位池这一档分布」
    /// （法术位落地批次）——与 [`Self::add_resource_pool_grant`] 服务
    /// 同一件事的不同容量公式：法术位需要按等级查一张多档表
    /// （`CapacityFormula::ByLevel` + `CapacityValue::Tiered`），不是
    /// 单个 `Fixed` 数值,`register-trait-resource-pool-by-level`
    /// （`traits.json5`）每调用一次追加表里的一个等级
    /// 断点,而不是每次都新开一条独立的 `ResourcePoolGrant`——若做成
    /// 后者,同一个池的多条断点会各自独立参与
    /// `effective_slot_tier_capacity` 的「跨授予来源求和」,在任意等级
    /// 上会把多个断点的值错误地加在一起,而不是取「≤ 当前等级的最大
    /// 断点」这一条设计要求的值（`resource-pools-and-rest.md` 三节
    /// `CapacityFormula::ByLevel` 文档）。因此这里按「同一个天赋对
    /// 同一个池、且容量公式已经是 `ByLevel`」找已有的那一条授予声明,
    /// 找到则把新断点插入它的表里；找不到则新建一条只含这一个断点的
    /// 授予声明,后续调用继续往里插。
    pub fn add_resource_pool_grant_tiered_level(
        &mut self,
        trait_id: ContentIndex,
        pool: ContentIndex,
        level: u32,
        tiers: Vec<u32>,
    ) -> Result<(), TraitError> {
        if !self.is_defined(trait_id) {
            return Err(TraitError::NotDefined(trait_id));
        }
        let grants = &mut self.granted_resource_pools[trait_id.get() as usize];
        let existing = grants.iter_mut().find(|grant| {
            grant.pool == pool && matches!(grant.capacity, CapacityFormula::ByLevel(_))
        });
        match existing {
            Some(grant) => {
                let CapacityFormula::ByLevel(table) = &mut grant.capacity else {
                    unreachable!("上面的 find 已经用 matches! 筛过,capacity 恒是 ByLevel");
                };
                table.insert(level, CapacityValue::Tiered(tiers));
            }
            None => {
                grants.push(ResourcePoolGrant {
                    pool,
                    capacity: CapacityFormula::ByLevel(BTreeMap::from([(
                        level,
                        CapacityValue::Tiered(tiers),
                    )])),
                });
            }
        }
        Ok(())
    }

    /// 追加声明「这个天赋携带一条规则修正」（伤害类别/抗性接线批次
    /// 新增）——与 [`Self::add_resource_pool_grant`] 同一个「新增能力用
    /// 新函数」模式：不改 `define`/`register-trait` 已有的签名，一个
    /// 天赋可以被多次调用追加多条 [`RuleModifier`]（例如同一个天赋
    /// 同时声明对火与冰的抗性）。**追加，不是覆盖**——理由同
    /// [`Self::add_resource_pool_grant`]，规则修正天然是一个可以携带
    /// 任意多条的列表。
    pub fn add_rule_modifier(
        &mut self,
        trait_id: ContentIndex,
        modifier: RuleModifier,
    ) -> Result<(), TraitError> {
        if !self.is_defined(trait_id) {
            return Err(TraitError::NotDefined(trait_id));
        }
        self.rule_modifiers[trait_id.get() as usize].push(modifier);
        Ok(())
    }
}

/// `ll_sim::traits::TraitCatalog` 的真实实现——`resolve_use_skill`
/// 门一通过这个 impl 真正查到种族天赋授予的技能，
/// `ll_sim::resource_pool::effective_scalar_capacity` 通过同一个 impl
/// 查到天赋授予的资源池容量声明，`ll_sim::traits::resistance_multiplier_permille`
/// （伤害类别/抗性接线批次新增）通过同一个 impl 查到天赋声明的规则
/// 修正，见 `ll_sim::traits` 模块文档「本任务选择的解法」一节同一套
/// 依赖倒置手法。搬运 `granted_skills`/`granted_resource_pools`/
/// `rule_modifiers` 三个字段——`TraitRule` 目前只声明这三个字段，见其
/// 文档。
impl TraitCatalog for TraitTable {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
        self.get(trait_id).map(|view| TraitRule {
            granted_skills: view.granted_skills.to_vec(),
            granted_resource_pools: view.granted_resource_pools.to_vec(),
            rule_modifiers: view.rule_modifiers.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    fn no_effects(display_name_key: NamespacedId) -> TraitAttrs {
        TraitAttrs {
            display_name_key,
            granted_skills: Vec::new(),
            stat_modifiers: Vec::new(),
            rule_modifiers: Vec::new(),
            granted_resource_pools: Vec::new(),
        }
    }

    #[test]
    fn 新建的天赋表查询任意索引均为未注册() {
        // Arrange
        let table = TraitTable::new();

        // Act & Assert
        assert!(!table.is_defined(ContentIndex::default()));
    }

    #[test]
    fn 注册后查询能拿到完整的授予技能列表() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:draconic_breath").unwrap());
        let skill = registry.intern(NamespacedId::parse("lostland:breath_weapon").unwrap());
        let mut table = TraitTable::new();

        // Act
        table
            .define(
                index,
                TraitAttrs {
                    granted_skills: vec![skill],
                    ..no_effects(NamespacedId::parse("lostland:trait.draconic_breath").unwrap())
                },
            )
            .expect("首次定义应当成功");

        // Assert
        let view = table.get(index).expect("已注册");
        assert_eq!(view.granted_skills, &[skill]);
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:halfling_luck").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                index,
                no_effects(NamespacedId::parse("lostland:trait.halfling_luck").unwrap()),
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            no_effects(NamespacedId::parse("lostland:trait.halfling_luck").unwrap()),
        );

        // Assert
        assert_eq!(result, Err(TraitError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined = registry.intern(NamespacedId::parse("yourmod:never_defined").unwrap());
        let table = TraitTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn traitcatalog实现查询已注册天赋返回授予的技能() {
        // 直接验收 `impl TraitCatalog for TraitTable`——`resolve_use_skill`
        // 真正依赖的正是这个 trait 方法，不是 `get`/`TraitView` 本身。
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(NamespacedId::parse("lostland:draconic_breath").unwrap());
        let skill = registry.intern(NamespacedId::parse("lostland:breath_weapon").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                index,
                TraitAttrs {
                    granted_skills: vec![skill],
                    ..no_effects(NamespacedId::parse("lostland:trait.draconic_breath").unwrap())
                },
            )
            .expect("首次定义应当成功");

        // Act
        let rule = TraitCatalog::trait_rule(&table, index).expect("已注册");

        // Assert
        assert_eq!(rule.granted_skills, vec![skill]);
    }

    #[test]
    fn traitcatalog实现查询未注册天赋返回none() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined = registry.intern(NamespacedId::parse("yourmod:never_defined").unwrap());
        let table = TraitTable::new();

        // Act & Assert
        assert_eq!(TraitCatalog::trait_rule(&table, never_defined), None);
    }

    #[test]
    fn 多次追加同一个天赋同一个池的按级法术位分布合并进同一张表() {
        // 直接验收 add_resource_pool_grant_tiered_level 文档「为什么不
        // 新开一条」一节：两次调用应当各自往同一条 ResourcePoolGrant 的
        // ByLevel 表里插入一个断点，而不是产出两条独立的 ResourcePoolGrant
        // ——后者会让 effective_slot_tier_capacity 在任意等级上把两个
        // 断点的值错误地加在一起。
        // Arrange
        let mut registry = Registry::new();
        let trait_id = registry.intern(NamespacedId::parse("lostland:arcane_casting").unwrap());
        let pool = registry.intern(NamespacedId::parse("lostland:wizard_slots").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                trait_id,
                no_effects(NamespacedId::parse("lostland:trait.arcane_casting").unwrap()),
            )
            .expect("首次定义应当成功");

        // Act：两次调用，分别声明 1 级与 3 级的断点。
        table
            .add_resource_pool_grant_tiered_level(trait_id, pool, 1, vec![2, 0, 0])
            .expect("首次追加应当成功");
        table
            .add_resource_pool_grant_tiered_level(trait_id, pool, 3, vec![4, 2, 0])
            .expect("第二次追加应当成功");

        // Assert：只有一条 ResourcePoolGrant,ByLevel 表里两个断点都在。
        let grants = &table.get(trait_id).unwrap().granted_resource_pools;
        assert_eq!(grants.len(), 1);
        let CapacityFormula::ByLevel(levels) = &grants[0].capacity else {
            panic!("容量公式应当是 ByLevel");
        };
        assert_eq!(
            levels,
            &BTreeMap::from([
                (1, CapacityValue::Tiered(vec![2, 0, 0])),
                (3, CapacityValue::Tiered(vec![4, 2, 0])),
            ])
        );
    }

    #[test]
    fn 目标天赋尚未注册时法术位分布追加返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let pool = registry.intern(NamespacedId::parse("lostland:wizard_slots").unwrap());
        let mut table = TraitTable::new();

        // Act
        let result =
            table.add_resource_pool_grant_tiered_level(ContentIndex::default(), pool, 1, vec![1]);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 追加规则修正后查询能拿到完整的抗性声明() {
        // Arrange
        let mut registry = Registry::new();
        let trait_id = registry.intern(NamespacedId::parse("lostland:fire_resistance").unwrap());
        let fire = registry.intern(NamespacedId::parse("lostland:fire").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                trait_id,
                no_effects(NamespacedId::parse("lostland:trait.fire_resistance").unwrap()),
            )
            .expect("首次定义应当成功");

        // Act
        table
            .add_rule_modifier(
                trait_id,
                RuleModifier::Resistance {
                    damage_category: fire,
                    multiplier_permille: 500,
                },
            )
            .expect("追加规则修正应当成功");

        // Assert
        let view = table.get(trait_id).unwrap();
        assert_eq!(
            view.rule_modifiers,
            &[RuleModifier::Resistance {
                damage_category: fire,
                multiplier_permille: 500,
            }]
        );
    }

    #[test]
    fn 多次追加规则修正累积成多条而不互相覆盖() {
        // Arrange：同一个天赋声明对火与冰各自的抗性——追加语义,不是
        // 单值覆盖。
        let mut registry = Registry::new();
        let trait_id = registry.intern(NamespacedId::parse("lostland:elemental_hide").unwrap());
        let fire = registry.intern(NamespacedId::parse("lostland:fire").unwrap());
        let cold = registry.intern(NamespacedId::parse("lostland:cold").unwrap());
        let mut table = TraitTable::new();
        table
            .define(
                trait_id,
                no_effects(NamespacedId::parse("lostland:trait.elemental_hide").unwrap()),
            )
            .expect("首次定义应当成功");

        // Act
        table
            .add_rule_modifier(
                trait_id,
                RuleModifier::Resistance {
                    damage_category: fire,
                    multiplier_permille: 500,
                },
            )
            .expect("第一次追加应当成功");
        table
            .add_rule_modifier(
                trait_id,
                RuleModifier::Resistance {
                    damage_category: cold,
                    multiplier_permille: 0,
                },
            )
            .expect("第二次追加应当成功");

        // Assert
        assert_eq!(table.get(trait_id).unwrap().rule_modifiers.len(), 2);
    }

    #[test]
    fn 目标天赋尚未注册时规则修正追加返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let fire = registry.intern(NamespacedId::parse("lostland:fire").unwrap());
        let mut table = TraitTable::new();

        // Act
        let result = table.add_rule_modifier(
            ContentIndex::default(),
            RuleModifier::Resistance {
                damage_category: fire,
                multiplier_permille: 500,
            },
        );

        // Assert
        assert!(result.is_err());
    }
}
