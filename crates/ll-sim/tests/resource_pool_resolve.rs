//! 资源池落地批次（第一批：法力池/血池）的端到端集成测试——走真实的
//! [`resolve_with_skills_traits_and_pools`]/[`apply::apply`] 管线，不是
//! 直接构造 [`Effect`] 抄近路。覆盖四条硬要求的链路：①天赋授予法力池
//! 后角色真的有了这个池（容量从 `CapacityFormula` 算出）；②放技能真的
//! 扣法力，法力不够时技能放不出来（门四不是恒真）；③每回合真的按
//! `RegenRule::OnTurnStart` 回复；④血魔法把自己扣死，产出
//! `KillCause::Skill`、`killer == victim`、具名角色的完整
//! `HistoricalEvent`。
//!
//! 夹具（`test_world`/`spawn_agent`/`FakeSkills`/`FakeRaceTraits`/
//! `FakeTraits`）与 `crates/ll-sim/tests/trait_resolve.rs` 几乎一致——
//! 同一个理由独立成文件（复用公开入口，不需要访问任何私有函数），差异
//! 只在于本文件额外需要一个 `FakeResourcePools`（[`ResourcePoolCatalog`]
//! 的测试替身）与直接构造 `resource_pools` 初始值的 `spawn_agent_with_pool`
//! /`spawn_named_agent_with_pool` 两个专用夹具。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve_with_skills_traits_and_pools;
use ll_sim::resource_pool::{
    CapacityFormula, RegenRule, ResourcePoolCatalog, ResourcePoolGrant, ResourcePoolRule,
};
use ll_sim::skill::{ResourceCost, SkillCatalog, SkillEffect, SkillRule};
use ll_sim::traits::{TraitCatalog, TraitGrant, TraitGrantSource, TraitRule};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::history::{HistoricalEventKind, KillCause};
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

struct FakeSkills {
    skills: BTreeMap<ContentIndex, SkillRule>,
}

impl SkillCatalog for FakeSkills {
    fn skill(&self, skill: ContentIndex) -> Option<SkillRule> {
        self.skills.get(&skill).copied()
    }
}

/// 一个只认识固定种族索引的测试用天赋授予来源，理由同
/// `trait_resolve.rs::FakeRaceTraits`。
struct FakeRaceTraits {
    race: ContentIndex,
    grants: Vec<TraitGrant>,
}

impl TraitGrantSource for FakeRaceTraits {
    fn granted_traits(&self, owner: ContentIndex) -> Vec<TraitGrant> {
        if owner == self.race {
            self.grants.clone()
        } else {
            Vec::new()
        }
    }
}

struct FakeTraits {
    traits: BTreeMap<ContentIndex, TraitRule>,
}

impl TraitCatalog for FakeTraits {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
        self.traits.get(&trait_id).cloned()
    }
}

/// 一个只认识固定资源池索引的测试目录——生产代码里真正的实现是
/// `ll_mod::resource_pool::ResourcePoolTable`（依赖方向不允许本 crate
/// 依赖它，见 `ll_sim::resource_pool` 模块文档）。
struct FakeResourcePools {
    pools: BTreeMap<ContentIndex, ResourcePoolRule>,
}

impl ResourcePoolCatalog for FakeResourcePools {
    fn resource_pool(&self, pool: ContentIndex) -> Option<ResourcePoolRule> {
        self.pools.get(&pool).copied()
    }
}

fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
}

fn test_world() -> WorldState {
    let layout = test_layout();
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let spawn = layout.tile_size().wrap(0, 0);
    WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件")
}

/// 造一个占位实体，站在 `(5, 5)`，已解锁 `unlocked_skill`（若有），
/// `resource_pools` 只有 `pool` 一条、当前值为 `pool_current`——供
/// 资源池消耗/容量测试直接摆好「这个池现在还剩多少」的初始状态，不
/// 依赖回复链路先跑一遍。
#[allow(clippy::too_many_arguments)]
fn spawn_agent_with_pool(
    world: &mut WorldState,
    race: ContentIndex,
    level: i32,
    unlocked_skill: Option<ContentIndex>,
    pool: ContentIndex,
    pool_current: i32,
    health: i32,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::from([(pool, pool_current)]),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: unlocked_skill.into_iter().collect(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        script_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        stealthed: false,
    })
}

/// 造一个已具名（`remembered_id` 已赋值）的占位实体——供血魔法自尽的
/// 「完整历史事件」测试使用，理由同
/// `crates/ll-sim/src/resolve.rs` 测试模块 `spawn_named_agent`。
fn spawn_named_agent_with_pool(
    world: &mut WorldState,
    race: ContentIndex,
    unlocked_skill: ContentIndex,
    health: i32,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    let mut world_id_counter = 0u32;
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: vec![unlocked_skill],
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        script_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: Some(ll_core::ident::WorldId::next(&mut world_id_counter)),
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        stealthed: false,
    })
}

#[test]
fn 天赋授予的容量小于存储量时消耗判定按容量钳位而不是按存储量放行() {
    // 直接验收「①天赋授予法力池 → 角色真的有了这个池（容量从
    // CapacityFormula 算出）」：agent.resource_pools 里存储的当前值是
    // 20（远大于本次消耗），但天赋只授予了 10 点容量——若判定只看存储
    // 值不看容量，这次消耗 10 点的施法会被误判为「绰绰有余」；真正的
    // 判据是 usable = min(stored, effective_cap) = min(20, 10) = 10，
    // 恰好够付 10 点，技能应当放出。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:innate_sorcery").expect("合法标识符"));
    let skill =
        interner.intern(NamespacedId::parse("lostland:sorcerer_firebolt").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:sorcery_points").expect("合法标识符"));
    let actor = spawn_agent_with_pool(&mut world, race, 1, None, pool, 20, 100);

    let skills = FakeSkills {
        skills: BTreeMap::from([(
            skill,
            SkillRule {
                cooldown_ticks: 0,
                resource_cost: ResourceCost::PoolAmount(pool, 10),
                effect: SkillEffect::DealDamage { base: 5 },
            },
        )]),
    };
    let race_traits = FakeRaceTraits {
        race,
        grants: vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }],
    };
    let traits = FakeTraits {
        traits: BTreeMap::from([(
            trait_id,
            TraitRule {
                granted_skills: vec![skill],
                granted_resource_pools: vec![ResourcePoolGrant {
                    pool,
                    capacity: CapacityFormula::Fixed(10),
                }],
                rule_modifiers: Vec::new(),
            },
        )]),
    };
    let pools = FakeResourcePools {
        pools: BTreeMap::new(),
    };

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &race_traits,
        &traits,
        &pools,
    );

    // Assert：技能真的放出来了（容量 10 点足够付 10 点消耗）。
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::AdjustResourcePool { pool: p, delta: -10, .. } if *p == pool)),
        "容量恰好覆盖消耗量时应当真实产出扣减效果,实际 effects={effects:?}"
    );
}

#[test]
fn 法力不够时技能真的放不出来() {
    // 直接验收「②放技能真的扣法力；法力不够时技能放不出来（这条证明
    // 判定不是恒真）」的后半句——与上一条测试唯一的差异是消耗量从 10
    // 改成 11（超过容量 10），效果列表应当整体为空（连
    // ScheduleNext/冷却设置都不应该产出，与其余三道门「未通过就不产生
    // 任何效果」同一条纪律）。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:innate_sorcery").expect("合法标识符"));
    let skill =
        interner.intern(NamespacedId::parse("lostland:sorcerer_firebolt").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:sorcery_points").expect("合法标识符"));
    let actor = spawn_agent_with_pool(&mut world, race, 1, None, pool, 20, 100);

    let skills = FakeSkills {
        skills: BTreeMap::from([(
            skill,
            SkillRule {
                cooldown_ticks: 0,
                resource_cost: ResourceCost::PoolAmount(pool, 11),
                effect: SkillEffect::DealDamage { base: 5 },
            },
        )]),
    };
    let race_traits = FakeRaceTraits {
        race,
        grants: vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }],
    };
    let traits = FakeTraits {
        traits: BTreeMap::from([(
            trait_id,
            TraitRule {
                granted_skills: vec![skill],
                granted_resource_pools: vec![ResourcePoolGrant {
                    pool,
                    capacity: CapacityFormula::Fixed(10),
                }],
                rule_modifiers: Vec::new(),
            },
        )]),
    };
    let pools = FakeResourcePools {
        pools: BTreeMap::new(),
    };

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &race_traits,
        &traits,
        &pools,
    );

    // Assert：容量不够,技能整体不产出任何效果。
    assert!(
        effects.is_empty(),
        "容量不足以支付消耗时不应产出任何效果,实际 effects={effects:?}"
    );
}

#[test]
fn 每回合开始时法力池按onturnstart真实回复() {
    // 直接验收「③每回合真的回复（RegenRule::OnTurnStart）」：走真实的
    // resolve + apply，不是直接构造 Effect::AdjustResourcePool 抄近路
    // ——resolve_dispatch 对任意 Intent（这里用 Wait）都会为发起者触发
    // 一次回合开始的资源池恢复检查，apply 后 agent.resource_pools 里
    // 这个池的当前值应当真实增加。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:innate_sorcery").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:sorcery_points").expect("合法标识符"));
    let actor = spawn_agent_with_pool(&mut world, race, 1, None, pool, 0, 100);

    let skills = FakeSkills {
        skills: BTreeMap::new(),
    };
    let race_traits = FakeRaceTraits {
        race,
        grants: vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }],
    };
    let traits = FakeTraits {
        traits: BTreeMap::from([(
            trait_id,
            TraitRule {
                granted_skills: Vec::new(),
                granted_resource_pools: vec![ResourcePoolGrant {
                    pool,
                    capacity: CapacityFormula::Fixed(20),
                }],
                rule_modifiers: Vec::new(),
            },
        )]),
    };
    let pools = FakeResourcePools {
        pools: BTreeMap::from([(
            pool,
            ResourcePoolRule {
                regen_rule: RegenRule::OnTurnStart { amount: 3 },
                shape: ll_sim::resource_pool::ResourcePoolShape::Scalar,
            },
        )]),
    };

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::Wait { actor },
        &skills,
        &race_traits,
        &traits,
        &pools,
    );
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert：起始值 0，每回合 +3，一次 Wait 之后应当真实变成 3。
    assert_eq!(
        world
            .actors
            .get(actor)
            .expect("刚生成必然存在")
            .resource_pools[&pool],
        3
    );
}

/// 一条无冷却、无自身效果消耗的血代价技能——四条血魔法测试的公共起点。
fn blood_bolt_rule(cost: u32) -> SkillRule {
    SkillRule {
        cooldown_ticks: 0,
        resource_cost: ResourceCost::Blood(cost),
        effect: SkillEffect::TemporaryStatModifier {
            attribute: ll_world::entity::AttributeKind::Strength,
            amount: 0,
            duration_ticks: 0,
        },
    }
}

#[test]
fn 血代价扣血量不受角色属性影响仍等于声明值() {
    // 「血代价必须绕开减伤」的实际验证方式（当前 resolve_attack 的
    // defense 恒为 0，没有防御来源可设，见任务书「硬要求二」的替代
    // 方案）：给角色一个远超正常范围的力量/体质，血代价扣除的量必须
    // 原样等于技能声明的数值，不因任何属性而打折——证明这条通道确实
    // 不查任何减伤/抗性表。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:blood_bolt").expect("合法标识符"));
    let actor = spawn_named_agent_with_pool(&mut world, race, skill, 100);
    // 力量/体质拉满到远超基准值——若血代价走了减伤链路,这里会产生
    // 影响;若真的绕开了,扣除量应该照样是声明的 15。
    if let Some(agent) = world.actors.get_mut(actor) {
        agent.stats.strength = 999;
        agent.stats.constitution = 999;
    }
    let skills = FakeSkills {
        skills: BTreeMap::from([(skill, blood_bolt_rule(15))]),
    };
    let no_traits = ll_sim::traits::NoTraitGrants;
    let no_trait_catalog = ll_sim::traits::NoTraits;
    let pools = FakeResourcePools {
        pools: BTreeMap::new(),
    };

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &no_traits,
        &no_trait_catalog,
        &pools,
    );

    // Assert
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::SpendBloodCost { target, amount: 15 } if *target == actor)),
        "血代价扣除量应恒等于声明值 15,不受任何属性影响,实际 effects={effects:?}"
    );
}

#[test]
fn 血代价产出的效果是spendbloodcost而不是damage() {
    // 结构性证明「血代价与 Effect::Damage 是不同的处理路径」：本次
    // 施法没有对任何其他目标造成伤害（effect 是无意义的
    // TemporaryStatModifier),唯一应当出现的与「扣血」相关的效果是
    // Effect::SpendBloodCost,不应该出现任何 Effect::Damage——若血代价
    // 误接了 Effect::Damage 这条路径,这里就会假阳性出现一条 Damage。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:blood_bolt").expect("合法标识符"));
    let actor = spawn_named_agent_with_pool(&mut world, race, skill, 100);
    let skills = FakeSkills {
        skills: BTreeMap::from([(skill, blood_bolt_rule(15))]),
    };
    let no_traits = ll_sim::traits::NoTraitGrants;
    let no_trait_catalog = ll_sim::traits::NoTraits;
    let pools = FakeResourcePools {
        pools: BTreeMap::new(),
    };

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &no_traits,
        &no_trait_catalog,
        &pools,
    );

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "血代价不应该产出任何 Effect::Damage,实际 effects={effects:?}"
    );
}

#[test]
fn 血代价扣到零以下时产出kill效果且killer与target都是施法者自己() {
    // 直接验收「④血魔法把自己扣死」的核心判据：health=10,血代价
    // 20,10-20=-10<=0,应当追加 Effect::Kill{target:actor,
    // killer:Some(actor), cause:KillCause::Skill{skill}}。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:blood_bolt").expect("合法标识符"));
    let actor = spawn_named_agent_with_pool(&mut world, race, skill, 10);
    let skills = FakeSkills {
        skills: BTreeMap::from([(skill, blood_bolt_rule(20))]),
    };
    let no_traits = ll_sim::traits::NoTraitGrants;
    let no_trait_catalog = ll_sim::traits::NoTraits;
    let pools = FakeResourcePools {
        pools: BTreeMap::new(),
    };

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &no_traits,
        &no_trait_catalog,
        &pools,
    );

    // Assert
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::Kill {
                target,
                killer: Some(killer),
                cause: KillCause::Skill { skill: cause_skill },
            } if *target == actor && *killer == actor && *cause_skill == skill
        )),
        "血代价扣到零以下应产出自尽的 Kill 效果,实际 effects={effects:?}"
    );
}

#[test]
fn 血代价致死后角色真的从世界里被销毁() {
    // 与上一条测试同样的施法,这次真的 apply 整批效果,断言角色不再
    // 存在于 world.actors——不是只在效果列表里"看起来会死"。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:blood_bolt").expect("合法标识符"));
    let actor = spawn_named_agent_with_pool(&mut world, race, skill, 10);
    let skills = FakeSkills {
        skills: BTreeMap::from([(skill, blood_bolt_rule(20))]),
    };
    let no_traits = ll_sim::traits::NoTraitGrants;
    let no_trait_catalog = ll_sim::traits::NoTraits;
    let pools = FakeResourcePools {
        pools: BTreeMap::new(),
    };
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &no_traits,
        &no_trait_catalog,
        &pools,
    );

    // Act
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert
    assert!(world.actors.get(actor).is_none());
}

#[test]
fn 具名角色血魔法自尽后产出完整历史事件且killer与victim相同() {
    // 直接验收「④……具名角色产出完整 HistoricalEvent」——`spawn_named_agent_with_pool`
    // 已经赋过 remembered_id,`append_kill_history` 的既有判据（只看
    // victim 是否已具名,不看 killer 是谁,见其文档）天然落在 killer==
    // victim 这个特殊情形内,不需要任何新分支。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:blood_bolt").expect("合法标识符"));
    let actor = spawn_named_agent_with_pool(&mut world, race, skill, 10);
    let skills = FakeSkills {
        skills: BTreeMap::from([(skill, blood_bolt_rule(20))]),
    };
    let no_traits = ll_sim::traits::NoTraitGrants;
    let no_trait_catalog = ll_sim::traits::NoTraits;
    let pools = FakeResourcePools {
        pools: BTreeMap::new(),
    };
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &no_traits,
        &no_trait_catalog,
        &pools,
    );

    // Act
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert：完整记录真的写进了 world.history,killer 与 victim 解析出
    // 同一个 WorldId（自尽,不是被别人杀）。
    assert_eq!(world.history.len(), 1);
    let HistoricalEventKind::Kill(record) = &world.history[0].kind;
    assert_eq!(record.killer, Some(record.victim));
}
