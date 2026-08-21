//! `resolve` 侧需要的资源池聚合接口（`knowledge/design/resource-pools-and-rest.md`
//! 二、三、四节；`knowledge/design/trait-system.md` 三节④）——本批次
//! 只接线 [`ResourcePoolShape::Scalar`]（法力池一类的标量池），见模块
//! 文档「本批次范围」一节。
//!
//! # 为什么这些类型定义在 `ll-sim`，不是 `ll-mod::trait_def`
//!
//! `ResourcePoolGrant`/`CapacityFormula`/`CapacityValue` 此前（天赋系统
//! 落地批次）声明在 `ll_mod::trait_def`——当时 `TraitDef.granted_resource_pools`
//! 只需要「形状定形」，没有任何 `resolve` 侧消费者，放在 `ll-mod` 是
//! 当时唯一能落地的选择。本批次要让 `resolve_use_skill`/`resolve_wait`
//! 真正读它,而 `ll-sim` 不能依赖 `ll-mod`（依赖方向,见 `crate::skill`
//! 模块文档「为什么这里重新声明了一遍」一节同一条约束）——这三个类型
//! 因此挪到本模块,与 [`crate::traits::TraitRule`] 的 `granted_resource_pools`
//! 字段共用同一份声明。`ll_mod::trait_def` 改为 `pub use` 本模块的定义
//! （与 `ll_mod::skill` 现在直接 `use` 本 crate 的 `ResourceCost`/
//! `SkillEffect` 是同一条先例，见该模块文档「接线批次的更新」一节）,
//! 不再维护一份会漂移的副本。
//!
//! # 本批次范围：只有 `Scalar` 形状与 `OnTurnStart` 恢复节奏
//!
//! `resource-pools-and-rest.md` 二节的完整设计有两种池形状
//! （`Scalar`/`TieredSlots`）与四种恢复节奏（`None`/`OnTurnStart`/
//! `OnRest`/`ResetOnLeaveCombat`）——本批次的验收范围只要求法力池
//! （标量池）与「每回合回复固定量」这一条链路能端到端跑通（见项目
//! 任务书「本次范围」一节的明确裁定：法术位与休息事件是下一批的工作，
//! 混在一起两条都验不透）。[`ResourcePoolShape`]/[`RegenRule`] 因此
//! 只声明本批次真正实现、真正有测试覆盖的变体，不像
//! `ll_mod::trait_def::RuleModifier` 那样把设计文档的全部变体一次性
//! 声明完（那份声明服务的是"形状已经在纯设计阶段冻结、只是还没有实现
//! 期消费者"这种情形；本批次不同——`TieredSlots`/`OnRest`/
//! `ResetOnLeaveCombat` 依赖的法术位存储/休息事件/脱战判定都还没有
//! 设计出对应的消费算法，提前声明变体只会制造一批不知道该怎么处理的
//! `match` 分支）。下一批实现法术位/休息事件时,在这里追加对应变体即可,
//! 不需要改动本批次已落地的 `Scalar`/`OnTurnStart` 路径。

use std::collections::BTreeMap;

use ll_core::ident::ContentIndex;

use crate::traits::{TraitCatalog, TraitGrantSource, effective_traits};

/// 一种可注册的资源池——法力、耐力、气……的共同身份里,`resolve` 真正
/// 要读的那一半（形状），见 `resource-pools-and-rest.md` 二节。本批次
/// 只支持 [`Self::Scalar`]，见模块文档「本批次范围」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcePoolShape {
    /// 标量池：单个当前值，消耗算法是「减去固定数量」——法力、耐力、
    /// 气……的共同形状。
    Scalar,
}

/// [`ResourcePoolGrant::capacity`] 的两种计算方式（`trait-system.md`
/// 三节④，`resource-pools-and-rest.md` 三节原文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapacityFormula {
    /// 容量恒定，不随等级变化——血魔法许可、多数标量池。
    Fixed(u32),
    /// 随 `Agent::level` 查表，阶梯式增长；未覆盖的等级取小于等于它的
    /// 最大已声明等级对应的值。键是等级，用 `BTreeMap` 而不是
    /// `HashMap`——查询「小于等于某个等级的最大已声明键」需要键的自然
    /// 顺序（约束 C5）。
    ByLevel(BTreeMap<u32, CapacityValue>),
}

/// [`CapacityFormula::ByLevel`] 某一级对应的容量值。
///
/// `Tiered` 变体本批次没有消费者（法术位是下一批的工作），但类型本身
/// 在天赋系统批次已经冻结定形，这里原样保留形状,不因为本批次不用就
/// 拆掉它——`granted_resource_pools` 里引用 `ByLevel` 的池若声明了
/// `Tiered` 值,[`effective_scalar_capacity`] 会按「形状不匹配,贡献零」
/// 处理（见其文档），不是编译期就拒绝这种声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapacityValue {
    /// 标量池的容量。
    Scalar(u32),
    /// 分级池（法术位）各档的容量，索引 0 = 第 1 档——本批次无消费者。
    Tiered(Vec<u32>),
}

/// 一条「这个天赋授予多少这种资源池容量」的声明（`trait-system.md`
/// 三节④）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePoolGrant {
    /// 指向 [`ResourcePoolShape`] 对应的池定义。
    pub pool: ContentIndex,
    /// 容量公式。
    pub capacity: CapacityFormula,
}

/// 资源池的恢复节奏（`resource-pools-and-rest.md` 四节）——与
/// [`ResourcePoolShape`] 正交，同一个形状可以配任意恢复节奏。本批次
/// 只有 [`Self::OnTurnStart`]，见模块文档「本批次范围」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegenRule {
    /// 不自动恢复，只能靠主动效果（技能自身的 `RestoreResource`/
    /// `AdjustResourcePool` 效果）。
    None,
    /// 每次该实体自己的回合开始时恢复固定量。
    OnTurnStart {
        /// 每回合恢复的数量。
        amount: u32,
    },
}

/// `resolve` 侧需要的一条资源池定义的最小只读视图——本批次只需要恢复
/// 节奏（消耗判定不需要查这张表，容量走 [`effective_scalar_capacity`]
/// 单独的天赋聚合路径），与 [`crate::skill::SkillRule`] 只收敛
/// `resolve_use_skill` 真正要读的字段是同一个理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourcePoolRule {
    /// 恢复节奏。
    pub regen_rule: RegenRule,
}

/// `resolve` 依赖的最小「资源池定义来源」接口——与
/// [`crate::skill::SkillCatalog`]/[`crate::traits::TraitCatalog`] 同一套
/// 依赖倒置手法：真正的 `ResourcePoolDef`/`ResourcePoolTable` 定义在
/// 下游的 `ll-mod`，本 crate 只声明「给我一个池索引，还我它的恢复
/// 节奏」这个接口。
pub trait ResourcePoolCatalog {
    /// 查询一条资源池定义；未注册的索引返回 `None`（ADR 0015）。
    fn resource_pool(&self, pool: ContentIndex) -> Option<ResourcePoolRule>;
}

/// 空资源池目录：查询任何索引恒返回 `None`——理由同 [`crate::skill::NoSkills`]。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoResourcePools;

impl ResourcePoolCatalog for NoResourcePools {
    fn resource_pool(&self, _pool: ContentIndex) -> Option<ResourcePoolRule> {
        None
    }
}

/// 聚合一个实体当前对某个标量池的有效容量——`trait-system.md` 三节④
/// 「聚合规则」：遍历 [`effective_traits`]（种族这一路来源，天赋系统
/// 落地批次的既有范围裁定，见 [`crate::traits`] 模块文档），对全部
/// 命中 `pool` 的 [`ResourcePoolGrant`] **求和**（不是 `Resistance` 那种
/// 取第一条命中的语义,理由见该节原文）。
///
/// `CapacityFormula::ByLevel` 查不到 `<= level` 的已声明等级、或对应的
/// [`CapacityValue`] 是 `Tiered`（形状不匹配——这个 `pool` 按标量消耗，
/// 但某条授予声明的是分级值）时，这一条贡献按零处理，不是整体返回
/// `None`/报错——与 `granted_skills` 「查不到就是没有」的既有纪律
/// 一致，形状不匹配是内容作者的声明错误，不该让引擎 panic。
pub fn effective_scalar_capacity(
    race: ContentIndex,
    level: i32,
    pool: ContentIndex,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> u32 {
    let mut total: u32 = 0;
    for trait_id in effective_traits(race, level, race_traits) {
        let Some(rule) = traits.trait_rule(trait_id) else {
            continue;
        };
        for grant in &rule.granted_resource_pools {
            if grant.pool != pool {
                continue;
            }
            total = total.saturating_add(eval_scalar_formula(&grant.capacity, level));
        }
    }
    total
}

/// [`CapacityFormula`] 在给定等级下的标量求值——[`effective_scalar_capacity`]
/// 的帮手，见其文档「形状不匹配」一节的处理方式。
fn eval_scalar_formula(formula: &CapacityFormula, level: i32) -> u32 {
    match formula {
        CapacityFormula::Fixed(amount) => *amount,
        CapacityFormula::ByLevel(table) => {
            let Ok(level) = u32::try_from(level) else {
                // 负等级：不存在任何 `<= level` 的已声明键（等级恒
                // 非负,`u32` 的键空间不含负数)——按零处理,理由同模块
                // 文档「形状不匹配」一节。
                return 0;
            };
            match table.range(..=level).next_back() {
                Some((_, CapacityValue::Scalar(amount))) => *amount,
                Some((_, CapacityValue::Tiered(_))) | None => 0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::TraitGrant;
    use crate::traits::TraitRule;
    use ll_core::ident::{Interner, NamespacedId};

    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    struct FixedGrants(Vec<TraitGrant>);
    impl TraitGrantSource for FixedGrants {
        fn granted_traits(&self, _owner: ContentIndex) -> Vec<TraitGrant> {
            self.0.clone()
        }
    }

    struct FixedTraits(Vec<(ContentIndex, TraitRule)>);
    impl TraitCatalog for FixedTraits {
        fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
            self.0
                .iter()
                .find(|(id, _)| *id == trait_id)
                .map(|(_, rule)| rule.clone())
        }
    }

    #[test]
    fn fixed容量公式无论等级多少都返回同一个值() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:human");
        let trait_id = index(&mut interner, "lostland:innate_sorcery");
        let pool = index(&mut interner, "lostland:sorcery_points");
        let source = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }]);
        let traits = FixedTraits(vec![(
            trait_id,
            TraitRule {
                granted_skills: Vec::new(),
                granted_resource_pools: vec![ResourcePoolGrant {
                    pool,
                    capacity: CapacityFormula::Fixed(20),
                }],
            },
        )]);

        // Act
        let result = effective_scalar_capacity(race, 99, pool, &source, &traits);

        // Assert
        assert_eq!(result, 20);
    }

    #[test]
    fn bylevel容量公式取小于等于当前等级的最大已声明档位() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:human");
        let trait_id = index(&mut interner, "lostland:arcane_casting");
        let pool = index(&mut interner, "lostland:wizard_mana");
        let source = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }]);
        let traits = FixedTraits(vec![(
            trait_id,
            TraitRule {
                granted_skills: Vec::new(),
                granted_resource_pools: vec![ResourcePoolGrant {
                    pool,
                    capacity: CapacityFormula::ByLevel(BTreeMap::from([
                        (1, CapacityValue::Scalar(10)),
                        (5, CapacityValue::Scalar(30)),
                    ])),
                }],
            },
        )]);

        // Act：7 级没有精确命中的档位，取 <= 7 的最大已声明档位（5 级）。
        let result = effective_scalar_capacity(race, 7, pool, &source, &traits);

        // Assert
        assert_eq!(result, 30);
    }

    #[test]
    fn 两个不同天赋授予同一个池的容量会相加() {
        // Arrange：容量是求和语义，不是取第一条命中（与 Resistance 区分）。
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:human");
        let trait_a = index(&mut interner, "lostland:trait_a");
        let trait_b = index(&mut interner, "lostland:trait_b");
        let pool = index(&mut interner, "lostland:shared_pool");
        let source = FixedGrants(vec![
            TraitGrant {
                trait_id: trait_a,
                unlock_level: 1,
            },
            TraitGrant {
                trait_id: trait_b,
                unlock_level: 1,
            },
        ]);
        let traits = FixedTraits(vec![
            (
                trait_a,
                TraitRule {
                    granted_skills: Vec::new(),
                    granted_resource_pools: vec![ResourcePoolGrant {
                        pool,
                        capacity: CapacityFormula::Fixed(10),
                    }],
                },
            ),
            (
                trait_b,
                TraitRule {
                    granted_skills: Vec::new(),
                    granted_resource_pools: vec![ResourcePoolGrant {
                        pool,
                        capacity: CapacityFormula::Fixed(5),
                    }],
                },
            ),
        ]);

        // Act
        let result = effective_scalar_capacity(race, 1, pool, &source, &traits);

        // Assert
        assert_eq!(result, 15);
    }

    #[test]
    fn 未被任何天赋授予的池容量为零() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:human");
        let pool = index(&mut interner, "lostland:never_granted");
        let source = FixedGrants(Vec::new());
        let traits = FixedTraits(Vec::new());

        // Act
        let result = effective_scalar_capacity(race, 10, pool, &source, &traits);

        // Assert
        assert_eq!(result, 0);
    }

    #[test]
    fn 空资源池目录查询任意索引返回none() {
        // Arrange
        let mut interner = Interner::new();
        let pool = index(&mut interner, "lostland:never_registered");
        let catalog = NoResourcePools;

        // Act & Assert
        assert_eq!(catalog.resource_pool(pool), None);
    }
}
