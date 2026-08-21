//! 天赋授予技能接入 `resolve_use_skill` 门一的集成测试（天赋系统落地
//! 批次）——`knowledge/design/trait-system.md` 三节①「有效技能=并集」
//! 公式在本批次的唯一接线点：种族天赋授予的技能，即使从未出现在
//! `agent.unlocked_skills` 里，也应当能被真实放出。
//!
//! 与 `crates/ll-sim/tests/skill_resolve.rs` 同一个理由独立成文件
//! （复用公开入口 [`resolve_with_skills_and_traits`]，不需要访问任何
//! 私有函数）；夹具（`test_world`/`spawn_agent`）与该文件几乎一致，
//! 差异只在于 `spawn_agent` 这里额外暴露 `level` 参数——种族天赋的
//! `unlock_level` 门槛正是本文件要验收的核心。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve_with_skills_and_traits;
use ll_sim::skill::{ResourceCost, SkillCatalog, SkillEffect, SkillRule};
use ll_sim::traits::{TraitCatalog, TraitGrant, TraitGrantSource, TraitRule};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 一个只认识固定几个技能索引的测试目录，理由同
/// `skill_resolve.rs::FakeCatalog`。
struct FakeSkills {
    skills: BTreeMap<ContentIndex, SkillRule>,
}

impl SkillCatalog for FakeSkills {
    fn skill(&self, skill: ContentIndex) -> Option<SkillRule> {
        self.skills.get(&skill).copied()
    }
}

/// 一个只认识固定种族索引的测试用天赋授予来源——生产代码里真正的
/// 实现是 `ll_mod::race::RaceTable`（依赖方向不允许本 crate 依赖它，
/// 见 `ll_sim::traits` 模块文档），本文件因此只能靠自己实现
/// [`TraitGrantSource`]。
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

/// 一个只认识固定天赋索引的测试目录。
struct FakeTraits {
    traits: BTreeMap<ContentIndex, TraitRule>,
}

impl TraitCatalog for FakeTraits {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
        self.traits.get(&trait_id).cloned()
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

/// 造一个占位实体，站在 `(5, 5)`，六项主属性、法力、耐力均取基准占位
/// 值，不携带任何已解锁技能——`race`/`level` 由调用方给出，理由见
/// 模块文档：两者正是本文件要验收的天赋解锁判据。
fn spawn_agent(world: &mut WorldState, race: ContentIndex, level: i32) -> EntityId {
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
        resource_pools: std::collections::BTreeMap::new(),
        unlocked_skills: Vec::new(),
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

/// 一条无消耗、无冷却、造成 10 点伤害的技能规则——两条测试的公共起点。
fn deal_damage_rule() -> SkillRule {
    SkillRule {
        cooldown_ticks: 20,
        resource_cost: ResourceCost::None,
        effect: SkillEffect::DealDamage { base: 10 },
    }
}

#[test]
fn 种族天赋在等级达标时授予的技能即使未解锁也能真实放出() {
    // Arrange：龙裔在 unlock_level=1 被授予"龙裔吐息"这条天赋，该天赋
    // 授予"吐息武器"这个技能——agent.unlocked_skills 从头到尾是空的，
    // 唯一的授权来源是种族天赋。
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:dragonborn").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:draconic_breath").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:breath_weapon").expect("合法标识符"));
    let actor = spawn_agent(&mut world, race, Agent::STARTING_LEVEL);

    let skills = FakeSkills {
        skills: BTreeMap::from([(skill, deal_damage_rule())]),
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
                granted_resource_pools: Vec::new(),
            },
        )]),
    };

    // Act
    let effects = resolve_with_skills_and_traits(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &race_traits,
        &traits,
    );

    // Assert：门一放行,技能真实产出了伤害效果——不是"恒真"的空判断。
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, ll_sim::effect::Effect::Damage { .. })),
        "种族天赋授予的技能应当真实产出伤害效果,实际 effects={effects:?}"
    );
}

#[test]
fn 种族天赋解锁等级未达标时授予的技能仍然不能使用() {
    // Arrange：与上一条测试完全相同的天赋/技能声明,唯一差异是
    // unlock_level=5 而 agent 处于起始等级（低于 5）——证明等级门槛
    // 真的在生效,不是"只要种族对就恒放行"。
    let mut world = test_world();
    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:dragonborn").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:draconic_breath").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:breath_weapon").expect("合法标识符"));
    // 前提：起始等级低于天赋的解锁等级——`Agent::STARTING_LEVEL` 是编译
    // 期常量，`cargo clippy` 会把 `assert!(常量 < 5)` 判定为"永真断言"
    // 拒绝，因此这条前提改用 `const _:` 断言在编译期强制校验，而不是
    // 运行期 `assert!`（若未来 `STARTING_LEVEL` 改到 5 或以上，这里会
    // 编译失败，而不是让测试本身悄悄失去意义）。
    const _: () = assert!(ll_world::entity::Agent::STARTING_LEVEL < 5);
    let actor = spawn_agent(&mut world, race, Agent::STARTING_LEVEL);

    let skills = FakeSkills {
        skills: BTreeMap::from([(skill, deal_damage_rule())]),
    };
    let race_traits = FakeRaceTraits {
        race,
        grants: vec![TraitGrant {
            trait_id,
            unlock_level: 5,
        }],
    };
    let traits = FakeTraits {
        traits: BTreeMap::from([(
            trait_id,
            TraitRule {
                granted_skills: vec![skill],
                granted_resource_pools: Vec::new(),
            },
        )]),
    };

    // Act
    let effects = resolve_with_skills_and_traits(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &skills,
        &race_traits,
        &traits,
    );

    // Assert
    assert!(effects.is_empty());
}
