//! 资源池落地批次（第二批：法术位/休息事件）的端到端集成测试——走真实
//! 的 [`resolve_with_skills_traits_and_pools`]/[`apply::apply`] 管线，
//! 不是直接构造 [`Effect`] 抄近路。覆盖任务书「硬要求一」的五条端到端
//! 链路：
//!
//! 1. 三环法术消耗一个三环位。
//! 2. 三环位用完后，同一个三环法术仍能用四环位放出（升阶支付）。
//! 3. 三环法术不能用一环位放（证明单向兑换真的是单向的）。
//! 4. 休息正常完成 → 法术位真的回满。
//! 5. 反复「休息一回合就取消」→ 资源零增长（防刷）。
//!
//! 外加两条正交性证明：`TieredSlots` 配 `OnTurnStart`（缓慢回复的
//! 法术位）与 `Scalar` 配 `OnRest`（休息回满的法力池）——两种反过来的
//! 组合结构上同样合法，见 `resource-pools-and-rest.md` 四节。
//!
//! 夹具与 `crates/ll-sim/tests/resource_pool_resolve.rs` 几乎一致（同一
//! 个理由独立成文件：复用公开入口，不需要访问任何私有函数），差异只在
//! 于本文件额外需要 `spent_slots`/`resting` 两个专用构造参数。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve_with_skills_traits_and_pools;
use ll_sim::resource_pool::{
    CapacityFormula, CapacityValue, RegenRule, ResourcePoolCatalog, ResourcePoolGrant,
    ResourcePoolRule, ResourcePoolShape, RestRecoveryAmount,
};
use ll_sim::skill::{ResourceCost, SkillCatalog, SkillEffect, SkillRule};
use ll_sim::traits::{TraitCatalog, TraitGrant, TraitGrantSource, TraitRule};
use ll_world::entity::{Agent, BaseStats, EntityId, RestState};
use ll_world::generate::GenParams;
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
/// `resource_pool_resolve.rs::FakeRaceTraits`。
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

/// 造一个占位实体，站在 `(5, 5)`，`spent_slots`/`resting` 由调用方给出
/// ——供法术位/休息测试直接摆好「各档已经花了多少、此刻在不在休息」
/// 这两个初始状态。
#[allow(clippy::too_many_arguments)]
fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    level: i32,
    unlocked_skill: Option<ContentIndex>,
    spent_slots: BTreeMap<(ContentIndex, u8), u32>,
    resource_pools: BTreeMap<ContentIndex, i32>,
    resting: Option<RestState>,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        luck: 0,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools,
        spent_slots,
        resting,
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
    })
}

/// 一个 4 档法术位天赋：一环 5 个、二环 0 个、三环 1 个、四环 1 个——
/// 全部测试共用同一张容量表,只在各测试里改 `spent_slots` 初始值。
fn wizard_slots_traits(
    race: ContentIndex,
    trait_id: ContentIndex,
    pool: ContentIndex,
) -> (FakeRaceTraits, FakeTraits) {
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
                    capacity: CapacityFormula::ByLevel(BTreeMap::from([(
                        1,
                        CapacityValue::Tiered(vec![5, 0, 1, 1]),
                    )])),
                }],
            },
        )]),
    };
    (race_traits, traits)
}

#[test]
fn 三环法术消耗一个三环位() {
    // 硬要求一·①：直接验收「消耗算法在有序档位里找空位」这条最基本
    // 路径——三环位当前完全空闲，三环法术应当占用它。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:arcane_casting").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:fireball").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:wizard_slots").expect("合法标识符"));
    let actor = spawn_agent(
        &mut world,
        race,
        1,
        Some(skill),
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    );

    let skills = FakeSkills {
        skills: BTreeMap::from([(
            skill,
            SkillRule {
                cooldown_ticks: 0,
                resource_cost: ResourceCost::SlotTier(pool, 3),
                effect: SkillEffect::DealDamage { base: 28 },
            },
        )]),
    };
    let (race_traits, traits) = wizard_slots_traits(race, trait_id, pool);
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

    // Assert
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::AdjustResourceSlot {
                pool: p,
                tier: 3,
                delta: 1,
                ..
            } if *p == pool
        )),
        "三环位完全空闲时三环法术应当真实占用它,实际 effects={effects:?}"
    );
}

#[test]
fn 三环位用完后同一个三环法术仍能用四环位放出() {
    // 硬要求一·②：升阶支付——三环位（容量 1）已经被占满,四环位
    // （容量 1）仍空闲,门四应当继续往上找,最终占用四环位而不是拒绝。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:arcane_casting").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:fireball").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:wizard_slots").expect("合法标识符"));
    let actor = spawn_agent(
        &mut world,
        race,
        1,
        Some(skill),
        BTreeMap::from([((pool, 3), 1)]), // 三环位已耗尽
        BTreeMap::new(),
        None,
    );

    let skills = FakeSkills {
        skills: BTreeMap::from([(
            skill,
            SkillRule {
                cooldown_ticks: 0,
                resource_cost: ResourceCost::SlotTier(pool, 3),
                effect: SkillEffect::DealDamage { base: 28 },
            },
        )]),
    };
    let (race_traits, traits) = wizard_slots_traits(race, trait_id, pool);
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

    // Assert：占用的是四环位,不是三环位（三环位已经没有空位)。
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::AdjustResourceSlot {
                pool: p,
                tier: 4,
                delta: 1,
                ..
            } if *p == pool
        )),
        "三环位耗尽后应当自动升阶占用四环位,实际 effects={effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::AdjustResourceSlot { tier: 3, .. })),
        "三环位已经没有空位,不应该再产出对三环位的扣减,实际 effects={effects:?}"
    );
}

#[test]
fn 三环法术不能用一环位放() {
    // 硬要求一·③：单向可兑换的反方向验证——一环位（容量 5）大量空闲,
    // 但三环、四环位容量都是零,三环法术应当被完全拒绝,不会因为「反正
    // 有空位」就退回去用一环位顶替。
    //
    // 手工验证会变红的做法（任务书硬要求）：把
    // `crates/ll-sim/src/resolve.rs` 的 `find_available_slot_tier`
    // 循环起点从 `min_tier` 改成 `1`（去掉"从请求档位起"这条下界),
    // 重跑本测试——一环位容量 5、已消耗 0,函数会找到 tier=1 判定为
    // 可用,下面的 `assert!(effects.is_empty())` 就会失败（会产出一条
    // `AdjustResourceSlot { tier: 1, .. }`）,证明这条断言真的在验证
    // "不许往下兑换"这件事,不是恒真。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:arcane_casting").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:fireball").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:wizard_slots").expect("合法标识符"));
    let actor = spawn_agent(
        &mut world,
        race,
        1,
        Some(skill),
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    );

    let skills = FakeSkills {
        skills: BTreeMap::from([(
            skill,
            SkillRule {
                cooldown_ticks: 0,
                resource_cost: ResourceCost::SlotTier(pool, 3),
                effect: SkillEffect::DealDamage { base: 28 },
            },
        )]),
    };
    // 容量表只给一环——三环、四环容量为零,理由同任务书原文（一环位
    // 大量空闲不能救三环法术）。
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
                    capacity: CapacityFormula::ByLevel(BTreeMap::from([(
                        1,
                        CapacityValue::Tiered(vec![5, 0, 0, 0]),
                    )])),
                }],
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

    // Assert：完全没有可用的三环或更高档位,技能整体不产出任何效果。
    assert!(
        effects.is_empty(),
        "三环、四环位容量均为零时不应放行,更不应该退回去用一环位,实际 effects={effects:?}"
    );
}

#[test]
fn 休息正常完成后法术位真的回满() {
    // 硬要求一·④：RegenRule::OnRest(Full) 对法术位的落地——三环、四环
    // 位都已耗尽,休息完成（世界时钟推进到已经超过 target_ticks）后,
    // 两档的已消耗数都应当清零。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:arcane_casting").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:wizard_slots").expect("合法标识符"));
    let actor = spawn_agent(
        &mut world,
        race,
        1,
        None,
        BTreeMap::from([((pool, 3), 1), ((pool, 4), 1)]),
        BTreeMap::new(),
        Some(RestState {
            started_at: Tick(0),
            target_ticks: 100,
        }),
    );
    world.clock = Tick(500); // 远超 target_ticks,确保这一步 Wait 必定完成休息

    let skills = FakeSkills {
        skills: BTreeMap::new(),
    };
    let (race_traits, traits) = wizard_slots_traits(race, trait_id, pool);
    let pools = FakeResourcePools {
        pools: BTreeMap::from([(
            pool,
            ResourcePoolRule {
                regen_rule: RegenRule::OnRest {
                    amount: RestRecoveryAmount::Full,
                },
                shape: ResourcePoolShape::TieredSlots { tier_count: 4 },
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

    // Assert：两档的已消耗数都清零（键干脆不再出现,或值为零）。
    let agent = world.actors.get(actor).expect("刚生成必然存在");
    assert_eq!(agent.spent_slots.get(&(pool, 3)).copied().unwrap_or(0), 0);
    assert_eq!(agent.spent_slots.get(&(pool, 4)).copied().unwrap_or(0), 0);
    assert_eq!(agent.resting, None, "休息完成后应当清空 resting 状态");
}

#[test]
fn 反复休息一回合就取消资源零增长() {
    // 硬要求一·⑤：防刷验证——反复「开始休息 → 一回合后取消」五次,
    // `target_ticks` 远大于每次推进的量,从未真正到达完成判据,累计
    // 恢复必须恒为零。
    //
    // 手工验证会变红的做法（任务书硬要求）：把
    // `crates/ll-sim/src/resolve.rs` 的 `resolve_wait` 改成"按
    // `(已过 tick 数 / target_ticks)` 比例发放恢复"（例如
    // `delta = spent * elapsed / target_ticks`），重跑本测试——五次
    // 「开始又取消」各自贡献一点非零的部分恢复,下面
    // `assert_eq!(final_spent, initial_spent)` 就会失败（`final_spent`
    // 会小于 `initial_spent`),证明这条断言真的在验证"不存在按比例
    // 发放"这件事,不是恒真。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:arcane_casting").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:wizard_slots").expect("合法标识符"));
    let never_registered_skill =
        interner.intern(NamespacedId::parse("lostland:never_registered").expect("合法标识符"));
    let actor = spawn_agent(
        &mut world,
        race,
        1,
        None,
        BTreeMap::from([((pool, 3), 1)]),
        BTreeMap::new(),
        None,
    );

    let skills = FakeSkills {
        skills: BTreeMap::new(),
    };
    let (race_traits, traits) = wizard_slots_traits(race, trait_id, pool);
    let pools = FakeResourcePools {
        pools: BTreeMap::from([(
            pool,
            ResourcePoolRule {
                regen_rule: RegenRule::OnRest {
                    amount: RestRecoveryAmount::Full,
                },
                shape: ResourcePoolShape::TieredSlots { tier_count: 4 },
            },
        )]),
    };

    let initial_spent = world
        .actors
        .get(actor)
        .unwrap()
        .spent_slots
        .get(&(pool, 3))
        .copied()
        .unwrap_or(0);

    // Act：五次「开始休息 → 一回合后取消」。target_ticks 是
    // 一百万,一次行动的基础代价（数十~数百 tick）远远不足以让任何一次
    // 迭代真正到达完成判据。
    for _ in 0..5 {
        let begin_effects = resolve_with_skills_traits_and_pools(
            &world,
            &Intent::Rest {
                actor,
                target_ticks: 1_000_000,
            },
            &skills,
            &race_traits,
            &traits,
            &pools,
        );
        for effect in &begin_effects {
            apply(&mut world, effect);
        }
        assert!(
            world.actors.get(actor).unwrap().resting.is_some(),
            "BeginRest 之后应当真的进入休息状态"
        );

        // 取消：提交一个非 Wait/Rest 的意图（技能查不到,本身不产出
        // 任何效果,但顶层的休息中断检查仍然会追加不带恢复的
        // ClearResting）。
        let cancel_effects = resolve_with_skills_traits_and_pools(
            &world,
            &Intent::UseSkill {
                actor,
                skill: never_registered_skill,
                target: None,
            },
            &skills,
            &race_traits,
            &traits,
            &pools,
        );
        assert!(
            !cancel_effects
                .iter()
                .any(|effect| matches!(effect, Effect::AdjustResourceSlot { .. })),
            "取消休息不应该产出任何恢复效果,实际 effects={cancel_effects:?}"
        );
        for effect in &cancel_effects {
            apply(&mut world, effect);
        }
        assert!(
            world.actors.get(actor).unwrap().resting.is_none(),
            "取消之后应当清空休息状态"
        );
    }

    // Assert：五轮「开始又取消」之后,已消耗数与起始时完全相同。
    let final_spent = world
        .actors
        .get(actor)
        .unwrap()
        .spent_slots
        .get(&(pool, 3))
        .copied()
        .unwrap_or(0);
    assert_eq!(
        final_spent, initial_spent,
        "反复开始又取消休息不应该让资源产生任何净增长"
    );
}

#[test]
fn 缓慢恢复的法术位每回合真的按onturnstart回复一个已消耗档位() {
    // 正交性证明·①：TieredSlots 配 OnTurnStart（不是 OnRest）——同样是
    // 法术位形状,恢复节奏走"每回合缓慢恢复"而不是"休息回满",证明
    // RegenRule 与 ResourcePoolShape 真的正交,不是引擎写死的对应关系
    // （`resource-pools-and-rest.md` 四节反过来的组合一）。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:druidic_casting").expect("合法标识符"));
    let pool = interner.intern(NamespacedId::parse("lostland:druid_slots").expect("合法标识符"));
    let actor = spawn_agent(
        &mut world,
        race,
        1,
        None,
        BTreeMap::from([((pool, 1), 1)]),
        BTreeMap::new(),
        None,
    );

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
                    capacity: CapacityFormula::ByLevel(BTreeMap::from([(
                        1,
                        CapacityValue::Tiered(vec![3, 0, 0]),
                    )])),
                }],
            },
        )]),
    };
    let pools = FakeResourcePools {
        pools: BTreeMap::from([(
            pool,
            ResourcePoolRule {
                regen_rule: RegenRule::OnTurnStart { amount: 1 },
                shape: ResourcePoolShape::TieredSlots { tier_count: 3 },
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

    // Assert：`resolve_resource_pool_regen`（RegenRule::OnTurnStart 的
    // 落点）按 `pool_rule.shape` 分流——TieredSlots 池产出
    // `Effect::AdjustResourceSlot`（从最低档开始恢复,与消耗算法"从最低
    // 阶开始取"对称），不是标量池那条 `Effect::AdjustResourcePool` 语义
    // （那条语义对一个记录"已消耗数"而不是"当前值"的池没有意义）。
    // 一环位当前已消耗 1、每回合恢复 1,这一次 Wait 之后应当清零。
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::AdjustResourceSlot {
                pool: p,
                tier: 1,
                delta: -1,
                ..
            } if *p == pool
        )),
        "TieredSlots 池的每回合恢复应当落到 AdjustResourceSlot,不是标量语义,实际 effects={effects:?}"
    );
    assert_eq!(
        world
            .actors
            .get(actor)
            .expect("刚生成必然存在")
            .spent_slots
            .get(&(pool, 1))
            .copied()
            .unwrap_or(0),
        0,
        "一环位的已消耗数应当被这一次回合恢复清零"
    );
}

#[test]
fn 休息完成后标量法力池真的回满() {
    // 正交性证明·②：Scalar 配 OnRest（不是 OnTurnStart）——法力池
    // 休息回满,是"回满的法力池"这个反过来的组合（四节反过来的组合
    // 二）的机制级证明。
    // Arrange
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:disciplined_sorcery").expect("合法标识符"));
    let pool =
        interner.intern(NamespacedId::parse("lostland:disciplined_points").expect("合法标识符"));
    let actor = spawn_agent(
        &mut world,
        race,
        1,
        None,
        BTreeMap::new(),
        BTreeMap::from([(pool, 5)]), // 当前只剩 5 点,容量是 20
        Some(RestState {
            started_at: Tick(0),
            target_ticks: 100,
        }),
    );
    world.clock = Tick(500);

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
            },
        )]),
    };
    let pools = FakeResourcePools {
        pools: BTreeMap::from([(
            pool,
            ResourcePoolRule {
                regen_rule: RegenRule::OnRest {
                    amount: RestRecoveryAmount::Full,
                },
                shape: ResourcePoolShape::Scalar,
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

    // Assert：当前值真的变成了容量上限 20。
    assert_eq!(
        world
            .actors
            .get(actor)
            .expect("刚生成必然存在")
            .resource_pools[&pool],
        20
    );
}
