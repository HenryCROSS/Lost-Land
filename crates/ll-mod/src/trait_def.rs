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
//! # 本批次范围：四类效果的形状定义齐全，只接线①授予技能
//!
//! `trait-system.md` 三节列出天赋的四类效果——①授予技能、②属性修正、
//! ③改变规则本身（[`RuleModifier`]）、④授予资源池容量
//! （[`ResourcePoolGrant`]）。[`TraitDef`] 四个字段因此一次性按设计
//! 文档定形（省得将来加字段又要动一次已冻结的结构），但本批次的
//! `resolve` 侧真正读取、真正有端到端测试验收的只有 `granted_skills`
//! 一项——`stat_modifiers` 走既有的 `Agent::active_stat_modifiers`
//! 通道（该节②：「不是新机制,是同一份数据被两种消费方式使用」,接线
//! 是另一批的工作)；`rule_modifiers`/`granted_resource_pools` 现在没有
//! 任何消费者（③需要的抗性机制、④需要的资源池系统都还是纯设计，见
//! 该文档十节「等什么」清单第 1/10 两项）——本模块只负责让这两类效果
//! 能被 mod 作者以正确的形状**声明**下来,不假装它们已经在游戏里生效。
//!
//! # `register-trait` 脚本签名为什么只暴露①，不是设计文档的完整六参数
//!
//! `trait-system.md` 四节给出的 `register-trait` 示意签名带四个效果
//! 参数（granted-skills/stat-modifiers/rule-modifiers/
//! granted-resource-pools）。本模块的 [`crate::script_trait_api`] 只
//! 实现前两个位置参数（`id`/`display-name-key`）+ `granted-skills`
//! 列表——`stat_modifiers`/`rule_modifiers`/`granted_resource_pools`
//! 在 Rust 结构体里已经声明好形状（本模块），但脚本层还没有为
//! "列表套元组"（`stat-modifiers`）与"打标签的构造子"
//! （`rule-modifiers` 的 `(resistance ...)`/`(reroll-once ...)`）这两种
//! FFI 编码约定过怎么做——本代码库现有全部 `register-*` 函数（`skill`/
//! `class`/`quest`/`race`/`clip`/`xp_curve`）都只用「扁平参数 + 字符串
//! 标签」这一种约定（见 `crate::script_skill_api` 模块文档「为什么
//! 这里多出两处 FFI 转换上的麻烦」一节），没有任何一处示范过"列表里
//! 每一项本身还是一个结构"要怎么从 `SteelVal` 转换。凭空发明一种新
//! 约定服务两个当前没有 resolve 侧消费者的字段,不是这一批的份内工作
//! （YAGNI）——`register-race-xp-reward` 相对 `register-race`「不改
//! 既有签名,新增能力用新函数」的先例已经证明这条路走得通,②③④三类
//! 效果的脚本入口留给各自真正接线的批次,用同一条先例补上。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::traits::{TraitCatalog, TraitRule};
use ll_world::entity::AttributeKind;

/// 三节③「改变规则本身」——封闭枚举，走注册表第一档（声明式），
/// 理由与档位判据见 `trait-system.md` 三节③「三步判据核对」一节。
///
/// **当前没有任何 `resolve` 侧消费者**（见模块文档「本批次范围」
/// 一节）——`Resistance` 需要的抗性乘数挂载点、`RerollOnce` 需要的
/// `roll_one_die` 钩子、`Advantage`/`Disadvantage` 需要的判定系统，
/// 三者都还是纯设计（该文档十节「等什么」清单第 1/4/5 项）。本枚举
/// 只负责把"这个天赋声明了哪种规则修正"这份数据在注册期存下来,不
/// 假装它已经在战斗结算里生效。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleModifier {
    /// 抗性：该伤害类别的伤害，在既有减伤链路算完之后再打一个千分比
    /// 折扣——0=免疫，500=半伤，2000=双倍。
    Resistance {
        /// 伤害类别，走 `damage-formula-mod-api.md` 十七节已经开放的
        /// `register-damage-category` 集合。
        damage_category: ContentIndex,
        /// 千分比乘数。
        multiplier_permille: i32,
    },
    /// 重骰：该实体掷骰抽出 `value` 时,立即重抽一次,取新值（不再检查
    /// 新值是否又是 `value`）。
    RerollOnce {
        /// 触发重骰的点数。
        value: i32,
    },
    /// 优势：该实体在 `check_context` 这类判定上默认套用优势——占位
    /// 变体，当前无消费者（本项目没有判定/检定系统,见模块文档）。
    Advantage {
        /// 判定种类的开放标识符,具体值域留给判定系统落地时定案。
        check_context: NamespacedId,
    },
    /// 劣势，语义同 [`RuleModifier::Advantage`]，方向相反。
    Disadvantage {
        /// 判定种类的开放标识符。
        check_context: NamespacedId,
    },
}

/// 三节④「资源池容量」——一条"这个天赋授予多少这种资源池容量"的声明,
/// 按 `resource-pools-and-rest.md` 三节末尾原文落地（`trait-system.md`
/// 三节④）。**当前没有任何消费者**：`effective_capacity` 与它依赖的
/// `ResourcePoolDef`/`Agent.resource_pools` 都还是下一批的工作（该
/// 文档十节「等什么」第 10 项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePoolGrant {
    /// 指向 `ResourcePoolDef`（`resource-pools-and-rest.md` 二节，
    /// 该类型尚未落地，这里先存一个不透明的 `ContentIndex`）。
    pub pool: ContentIndex,
    /// 容量公式。
    pub capacity: CapacityFormula,
}

/// [`ResourcePoolGrant::capacity`] 的两种计算方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapacityFormula {
    /// 容量恒定，不随等级变化——血魔法许可、多数标量池。
    Fixed(u32),
    /// 随等级查表，阶梯式增长；未覆盖的等级取小于等于它的最大已声明
    /// 等级对应的值。键是等级，用 `BTreeMap` 而不是
    /// `HashMap`——查询"小于等于某个等级的最大已声明键"需要键的自然
    /// 顺序（约束 C5：不依赖 `HashMap` 迭代顺序，也不能，因为这里根本
    /// 不是在遍历，是在做有序范围查询）。
    ByLevel(std::collections::BTreeMap<u32, CapacityValue>),
}

/// [`CapacityFormula::ByLevel`] 某一级对应的容量值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapacityValue {
    /// 标量池的容量。
    Scalar(u32),
    /// 分级池（法术位）各档的容量，索引 0 = 第 1 档。
    Tiered(Vec<u32>),
}

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
}

impl fmt::Display for TraitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraitError::DuplicateDefinition(index) => {
                write!(f, "天赋索引 {} 被重复定义", index.get())
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
}

/// `ll_sim::traits::TraitCatalog` 的真实实现——`resolve_use_skill`
/// 门一通过这个 impl 真正查到种族天赋授予的技能，见
/// `ll_sim::traits` 模块文档「本任务选择的解法」一节同一套依赖倒置
/// 手法。只搬运 `granted_skills` 一个字段——`TraitRule` 本身就只
/// 声明这一个字段，见其文档。
impl TraitCatalog for TraitTable {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
        self.get(trait_id).map(|view| TraitRule {
            granted_skills: view.granted_skills.to_vec(),
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
}
