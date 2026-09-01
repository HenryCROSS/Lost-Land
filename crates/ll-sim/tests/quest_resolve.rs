//! 击杀结算对任务进度接线的集成测试（P5-B 接线批次）。
//!
//! 独立于 `ll-sim/src/resolve.rs` 内部的 `#[cfg(test)]` 模块——理由同
//! `skill_resolve.rs`：复用 [`resolve_with_skills_and_quests`] 这个公开
//! 入口，不需要访问任何私有函数，把击杀→任务进度这条接线的测试独立成
//! 一个文件。
//!
//! 这里的 [`FakeQuestCatalog`] 是一个纯测试用的 [`QuestCatalog`]
//! 实现——生产代码里真正的任务目录来自 `ll-mod::quest::RegisteredQuests`
//! （依赖方向不允许本 crate 依赖它，见 `ll_sim::quest` 模块文档），本
//! 文件因此只能靠自己实现这个 trait，这正是依赖反转设计本身要验证的
//! 东西：`resolve_with_skills_and_quests` 不关心目录从哪来，只要实现了
//! `QuestCatalog`。`crates/ll-mod/src/quest_overview.rs`（任务 8）与
//! `crates/ll-content/tests/gameplay_acceptance.rs`（任务 9，2026-08-29
//! 前是 `examples/p5_gameplay_acceptance.rs`，见 ADR 0030）另有
//! 使用真实 `ll-mod` 内容的端到端验收。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::quest::{NoQuests, QuestCatalog, QuestKillRule, is_quest_completed};
use ll_sim::resolve::resolve_with_skills_and_quests;
use ll_sim::skill::{NoSkills, ResourceCost, SkillCatalog, SkillEffect, SkillRule};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 一个只认识固定几条 `KillCount` 规则的测试目录。
struct FakeQuestCatalog {
    rules: Vec<QuestKillRule>,
}

impl QuestCatalog for FakeQuestCatalog {
    fn kill_count_quests(&self) -> Vec<QuestKillRule> {
        self.rules.clone()
    }
}

/// 一个只认识一个固定技能索引的测试目录——理由同
/// `skill_resolve.rs::FakeCatalog`，本文件独立声明一份是因为集成测试
/// 文件各自编译成独立的 crate，互相看不见对方的私有测试夹具。
struct FakeSkillCatalog {
    skill: ContentIndex,
    rule: SkillRule,
}

impl SkillCatalog for FakeSkillCatalog {
    fn skill(&self, skill: ContentIndex) -> Option<SkillRule> {
        (skill == self.skill).then_some(self.rule)
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

/// 造一个占位实体，站在 `(5, 5)`，`race` 由调用方指定——击杀计数按
/// `race` 匹配（见 `ll_sim::quest` 模块文档「击杀计数」一节）。
fn spawn_agent_with_race(world: &mut WorldState, race: ContentIndex, health: i32) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
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
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        mod_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
        home: None,
    })
}

#[test]
fn 击杀达到阈值后任务被标记完成() {
    // Arrange：目标生命值 1，攻击者力量足以一击致死；一条阈值为 1 的
    // KillCount 规则匹配目标的种类。
    let mut world = test_world();
    let mut interner = Interner::new();
    let goblin = interner.intern(NamespacedId::parse("lostland:goblin").expect("合法标识符"));
    let quest = NamespacedId::parse("lostland:main_quest_1").expect("合法标识符");
    let actor = spawn_agent_with_race(&mut world, ContentIndex::default(), Agent::STARTING_HEALTH);
    let target = spawn_agent_with_race(&mut world, goblin, 1);
    let catalog = FakeQuestCatalog {
        rules: vec![QuestKillRule {
            quest: quest.clone(),
            target_kind: goblin,
            required_count: 1,
            prerequisites: Vec::new(),
        }],
    };

    // Act
    let effects = resolve_with_skills_and_quests(
        &world,
        &Intent::Attack { actor, target },
        &NoSkills,
        &catalog,
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Kill { .. })),
        "本用例的伤害应当足以致死，否则测不出击杀分支"
    );
    for effect in &effects {
        ll_sim::apply::apply(&mut world, effect);
    }

    // Assert
    let agent = world.actors.get(actor).expect("攻击者应仍存在");
    assert!(is_quest_completed(agent, &quest));
}

#[test]
fn 使用技能击杀目标同样能推进击杀任务进度() {
    // P5-C 缺口修补批次的核心验收：`resolve_use_skill` 此前不判断
    // 「这一下是否致死」，技能永远不产出 `Effect::Kill`，任务系统也就
    // 永远看不到技能击杀——本用例证明两处修复（致死判定本身 +
    // `Intent::UseSkill` 接入 `append_quest_kill_progress`）都真的生效。
    // Arrange：与 `击杀达到阈值后任务被标记完成` 同一套场景，只是把
    // 攻击换成技能。
    let mut world = test_world();
    let mut interner = Interner::new();
    let goblin = interner.intern(NamespacedId::parse("lostland:goblin").expect("合法标识符"));
    let skill = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    let quest = NamespacedId::parse("lostland:main_quest_1").expect("合法标识符");
    let actor = spawn_agent_with_race(&mut world, ContentIndex::default(), Agent::STARTING_HEALTH);
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(skill);
    let target = spawn_agent_with_race(&mut world, goblin, 1);
    let quest_catalog = FakeQuestCatalog {
        rules: vec![QuestKillRule {
            quest: quest.clone(),
            target_kind: goblin,
            required_count: 1,
            prerequisites: Vec::new(),
        }],
    };
    let skill_catalog = FakeSkillCatalog {
        skill,
        rule: SkillRule {
            cooldown_ticks: 0,
            resource_cost: ResourceCost::None,
            effect: SkillEffect::DealDamage { base: 10 },
        },
    };

    // Act
    let effects = resolve_with_skills_and_quests(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: Some(target),
        },
        &skill_catalog,
        &quest_catalog,
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Kill { .. })),
        "本用例的技能伤害（10）应当足以致死生命值为 1 的目标，否则测不出击杀分支"
    );
    for effect in &effects {
        ll_sim::apply::apply(&mut world, effect);
    }

    // Assert：目标真的死了，且任务因此被标记完成。
    assert!(world.actors.get(target).is_none(), "目标应已被击杀");
    let agent = world.actors.get(actor).expect("攻击者应仍存在");
    assert!(is_quest_completed(agent, &quest));
}

#[test]
fn 击杀未达阈值时计数增加但任务不完成() {
    // Arrange：阈值为 3，只杀 1 次。
    let mut world = test_world();
    let mut interner = Interner::new();
    let goblin = interner.intern(NamespacedId::parse("lostland:goblin").expect("合法标识符"));
    let quest = NamespacedId::parse("lostland:main_quest_1").expect("合法标识符");
    let actor = spawn_agent_with_race(&mut world, ContentIndex::default(), Agent::STARTING_HEALTH);
    let target = spawn_agent_with_race(&mut world, goblin, 1);
    let catalog = FakeQuestCatalog {
        rules: vec![QuestKillRule {
            quest: quest.clone(),
            target_kind: goblin,
            required_count: 3,
            prerequisites: Vec::new(),
        }],
    };

    // Act
    let effects = resolve_with_skills_and_quests(
        &world,
        &Intent::Attack { actor, target },
        &NoSkills,
        &catalog,
    );
    for effect in &effects {
        ll_sim::apply::apply(&mut world, effect);
    }

    // Assert：计数已经写入（否则第二次、第三次击杀无从累加），但任务
    // 尚未完成。
    let agent = world.actors.get(actor).expect("攻击者应仍存在");
    assert!(!is_quest_completed(agent, &quest));
    assert_eq!(
        agent.mod_state.get(&(
            "lostland".to_string(),
            format!("kill_count:{}", goblin.get())
        )),
        Some(&ll_world::mod_state::ModStateValue::Int(1))
    );
}

#[test]
fn 击杀种类不匹配的目标不推进任务() {
    // Arrange：目标种类与规则的 target_kind 不同。
    let mut world = test_world();
    let mut interner = Interner::new();
    let goblin = interner.intern(NamespacedId::parse("lostland:goblin").expect("合法标识符"));
    let wolf = interner.intern(NamespacedId::parse("lostland:wolf").expect("合法标识符"));
    let quest = NamespacedId::parse("lostland:main_quest_1").expect("合法标识符");
    let actor = spawn_agent_with_race(&mut world, ContentIndex::default(), Agent::STARTING_HEALTH);
    let target = spawn_agent_with_race(&mut world, wolf, 1);
    let catalog = FakeQuestCatalog {
        rules: vec![QuestKillRule {
            quest: quest.clone(),
            target_kind: goblin,
            required_count: 1,
            prerequisites: Vec::new(),
        }],
    };

    // Act
    let effects = resolve_with_skills_and_quests(
        &world,
        &Intent::Attack { actor, target },
        &NoSkills,
        &catalog,
    );
    for effect in &effects {
        ll_sim::apply::apply(&mut world, effect);
    }

    // Assert
    let agent = world.actors.get(actor).expect("攻击者应仍存在");
    assert!(!is_quest_completed(agent, &quest));
}

#[test]
fn 使用noquests目录时击杀不产生任何任务相关写入() {
    // Arrange：与 resolve（不接收任务目录的默认入口）行为一致的验收。
    let mut world = test_world();
    let actor = spawn_agent_with_race(&mut world, ContentIndex::default(), Agent::STARTING_HEALTH);
    let target = spawn_agent_with_race(&mut world, ContentIndex::default(), 1);

    // Act
    let effects = resolve_with_skills_and_quests(
        &world,
        &Intent::Attack { actor, target },
        &NoSkills,
        &NoQuests,
    );

    // Assert：仍然产出 Kill/Damage/ScheduleNext,但没有 SetModState
    // ——NoQuests 不知道任何规则,kill_progress_effects 只会写入击杀
    // 计数本身，这里改为验证「不完成任何任务」这条更贴合意图的性质。
    for effect in &effects {
        ll_sim::apply::apply(&mut world, effect);
    }
    assert!(world.actors.get(target).is_none(), "目标应已被击杀");
}

#[test]
fn 非attack意图不触发任务进度接线() {
    // Arrange：Wait 意图不会产出 Kill，即使传入了会匹配一切的目录,
    // append_quest_kill_progress 也不应该被触发——它现在对
    // Attack/UseSkill 两种可能产出 Effect::Kill 的意图生效（见
    // resolve.rs 文档），但 Wait 恒不在其中。
    let mut world = test_world();
    let actor = spawn_agent_with_race(&mut world, ContentIndex::default(), Agent::STARTING_HEALTH);
    let catalog = FakeQuestCatalog { rules: Vec::new() };

    // Act
    let effects =
        resolve_with_skills_and_quests(&world, &Intent::Wait { actor }, &NoSkills, &catalog);

    // Assert：Wait 只产出 ScheduleNext，没有 SetModState。
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::SetModState { .. }))
    );
}

#[test]
fn 前置任务未完成时即使达到击杀阈值也不标记完成() {
    // 回归测试：P5-B 任务 9 验收 demo 实测抓出的真实缺陷——一个任务
    // 节点自己的 KillCount 阈值达标,不代表这个节点已经"解锁"（它的
    // 前置任务可能压根还没完成）。修复前，`finale`（前置是
    // branch_a/branch_b，自身条件只要求击杀 1 个哥布林）会在玩家杀满
    // 3 个哥布林、凑够 main_quest_1 的阈值时被一起标记完成——即便
    // finale 从未在任务日志里出现过。
    // Arrange：quest 的前置是 prerequisite，且 prerequisite 尚未完成。
    let mut world = test_world();
    let mut interner = Interner::new();
    let goblin = interner.intern(NamespacedId::parse("lostland:goblin").expect("合法标识符"));
    let prerequisite = NamespacedId::parse("lostland:main_quest_1").expect("合法标识符");
    let quest = NamespacedId::parse("lostland:finale").expect("合法标识符");
    let actor = spawn_agent_with_race(&mut world, ContentIndex::default(), Agent::STARTING_HEALTH);
    let target = spawn_agent_with_race(&mut world, goblin, 1);
    let catalog = FakeQuestCatalog {
        rules: vec![QuestKillRule {
            quest: quest.clone(),
            target_kind: goblin,
            required_count: 1,
            prerequisites: vec![prerequisite],
        }],
    };

    // Act：一次击杀就足以达到 required_count,但前置从未被标记完成。
    let effects = resolve_with_skills_and_quests(
        &world,
        &Intent::Attack { actor, target },
        &NoSkills,
        &catalog,
    );
    for effect in &effects {
        ll_sim::apply::apply(&mut world, effect);
    }

    // Assert
    let agent = world.actors.get(actor).expect("攻击者应仍存在");
    assert!(
        !is_quest_completed(agent, &quest),
        "前置未完成时,即使击杀阈值达标也不应该标记完成"
    );
}
