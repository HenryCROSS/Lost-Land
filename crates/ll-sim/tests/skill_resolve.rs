//! `Intent::UseSkill` 结算的集成测试（P5-B 任务 5）。
//!
//! 独立于 `ll-sim/src/resolve.rs` 内部的 `#[cfg(test)]` 模块——后者已经
//! 852 行（超出 `knowledge` 编码纪律建议的 800 行上限），本文件复用
//! [`resolve_with_skills`] 这个公开入口（不需要访问任何私有函数），把
//! 技能结算的测试独立成一个文件，而不是继续往那个已经超标的文件里堆。
//!
//! 这里的 [`FakeCatalog`] 是一个纯测试用的 [`SkillCatalog`] 实现——生产
//! 代码里真正的技能目录来自 `ll-mod`（依赖方向不允许本 crate 依赖它，
//! 见 `ll_sim::skill` 模块文档），本文件因此只能靠自己实现这个 trait，
//! 这正是依赖反转设计本身要验证的东西：`resolve_use_skill` 不关心
//! 目录从哪来，只要实现了 `SkillCatalog`。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve_with_skills;
use ll_sim::skill::{ResourceCost, ResourceKind, SkillCatalog, SkillEffect, SkillRule};
use ll_world::entity::{Agent, AttributeKind, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 一个只认识固定几个技能索引的测试目录——`skill` 用 `BTreeMap` 存储
/// 是本文件的选择（约束 C5：不使用 `HashMap`），与生产代码是否用同样
/// 的容器无关，这里只是测试夹具。
struct FakeCatalog {
    skills: BTreeMap<ContentIndex, SkillRule>,
}

impl SkillCatalog for FakeCatalog {
    fn skill(&self, skill: ContentIndex) -> Option<SkillRule> {
        self.skills.get(&skill).copied()
    }
}

/// 测试用区块布局：边长 64，单个区块，与本仓库其余测试同一常量。
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
/// 值，不携带任何已解锁技能——各测试按需再往 `unlocked_skills`/
/// `skill_cooldowns`/`mana`/`stamina` 上叠加。
fn spawn_agent(world: &mut WorldState) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
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
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        script_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
    })
}

/// 造一个只认识 `skill` 一个技能的目录，规则由调用方给出。
fn catalog_with(skill: ContentIndex, rule: SkillRule) -> FakeCatalog {
    FakeCatalog {
        skills: BTreeMap::from([(skill, rule)]),
    }
}

/// 一条无消耗、无冷却、造成 10 点伤害的技能规则——多条测试的公共起点。
fn deal_damage_rule() -> SkillRule {
    SkillRule {
        cooldown_ticks: 20,
        resource_cost: ResourceCost::None,
        effect: SkillEffect::DealDamage { base: 10 },
    }
}

#[test]
fn 使用未解锁的技能不产出任何效果() {
    // Arrange：技能在目录里存在，但没有出现在 agent.unlocked_skills 里。
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    let catalog = catalog_with(skill, deal_damage_rule());

    // Act
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &catalog,
    );

    // Assert
    assert!(effects.is_empty());
}

#[test]
fn 使用冷却中的技能不产出任何效果() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(skill);
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .skill_cooldowns
        .insert(skill, Tick(world.clock.0 + 100)); // 冷却到期时刻在未来。
    let catalog = catalog_with(skill, deal_damage_rule());

    // Act
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &catalog,
    );

    // Assert
    assert!(effects.is_empty());
}

#[test]
fn 冷却已过期的技能不再被视为冷却中() {
    // 惰性判定的直接验收：到期时刻早于当前时钟时，视为可用——不需要
    // 任何人主动清理 skill_cooldowns 这个条目。
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(skill);
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .skill_cooldowns
        .insert(skill, Tick(world.clock.0 - 1)); // 已经过期。
    let catalog = catalog_with(skill, deal_damage_rule());

    // Act
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &catalog,
    );

    // Assert
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. }))
    );
}

#[test]
fn 资源不足时不产出任何效果() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:fireball").expect("合法标识符"));
    let agent = world.actors.get_mut(actor).expect("刚生成必然存在");
    agent.unlocked_skills.push(skill);
    agent.mana = 5; // 远低于技能要求。
    let catalog = catalog_with(
        skill,
        SkillRule {
            cooldown_ticks: 10,
            resource_cost: ResourceCost::Amount(ResourceKind::Mana, 50),
            effect: SkillEffect::DealDamage { base: 20 },
        },
    );

    // Act
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &catalog,
    );

    // Assert
    assert!(effects.is_empty());
}

#[test]
fn 成功使用技能后产出伤害效果与冷却设置() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let target = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(skill);
    let catalog = catalog_with(skill, deal_damage_rule());

    // Act
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: Some(target),
        },
        &catalog,
    );

    // Assert：伤害落在显式给出的目标上，且冷却确实被设置。
    assert!(effects.iter().any(
        |effect| matches!(effect, Effect::Damage { target: t, amount } if *t == target && *amount == 10)
    ));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::SetSkillCooldown { actor: a, skill: s, .. } if *a == actor && *s == skill
    )));
}

#[test]
fn 技能伤害足以致死时产出kill效果() {
    // P5-C 缺口修补批次修复的缺口：resolve_use_skill 的 DealDamage
    // 分支此前不判断"这一下是否致死"，与 resolve_attack 的既有纪律
    // 不一致，技能永远打不死目标。本用例直接对照
    // `成功使用技能后产出伤害效果与冷却设置`：同一条技能规则（10 点
    // 伤害），唯一区别是目标生命值只有 5（小于 10），必须产出
    // `Effect::Kill`。
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let target = spawn_agent(&mut world);
    world.actors.get_mut(target).expect("刚生成必然存在").health = 5;
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(skill);
    let catalog = catalog_with(skill, deal_damage_rule());

    // Act
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: Some(target),
        },
        &catalog,
    );

    // Assert
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Kill { target: t, .. } if *t == target)),
        "生命值 5 的目标挨了 10 点技能伤害，理应产出 Effect::Kill"
    );
}

#[test]
fn 技能伤害不足以致死时不产出kill效果() {
    // 与上一条用例对照：目标生命值高于技能伤害时不应该产出 Kill——
    // 防止「致死判定」被写成恒真（例如漏掉 <= 判断，任何伤害都触发
    // Kill）这类退化实现。
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let target = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(skill);
    let catalog = catalog_with(skill, deal_damage_rule());

    // Act：目标满血（Agent::STARTING_HEALTH 远大于 10 点伤害）。
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: Some(target),
        },
        &catalog,
    );

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Kill { .. })),
        "满血目标挨一下 10 点伤害不应该被判定为致死"
    );
}

#[test]
fn 成功使用技能后扣减对应资源() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:fireball").expect("合法标识符"));
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(skill);
    let catalog = catalog_with(
        skill,
        SkillRule {
            cooldown_ticks: 10,
            resource_cost: ResourceCost::Amount(ResourceKind::Mana, 30),
            effect: SkillEffect::DealDamage { base: 20 },
        },
    );

    // Act
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: Some(actor),
        },
        &catalog,
    );

    // Assert
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::AdjustResource { actor: a, resource: ResourceKind::Mana, delta: -30 } if *a == actor
    )));
}

#[test]
fn 临时属性修正技能产出applystatmodifier效果() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:brace").expect("合法标识符"));
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(skill);
    let catalog = catalog_with(
        skill,
        SkillRule {
            cooldown_ticks: 15,
            resource_cost: ResourceCost::None,
            effect: SkillEffect::TemporaryStatModifier {
                attribute: AttributeKind::Constitution,
                amount: 3,
                duration_ticks: 10,
            },
        },
    );

    // Act：未显式给出 target，效果应回落到施法者自身。
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill,
            target: None,
        },
        &catalog,
    );

    // Assert
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::ApplyStatModifier { target, attribute: AttributeKind::Constitution, delta: 3, .. }
        if *target == actor
    )));
}

#[test]
fn 新增一个假想mod技能不修改resolve任何代码该技能效果依然正确产出() {
    // 「本体即 Mod」检验的直接体现：这个技能索引对 resolve_use_skill
    // 而言与任何本体技能没有任何区别——它只是恰好通过一个命名空间为
    // yourmod 的标识符登记出来的索引，resolve_use_skill 全程不对
    // ContentIndex 的具体数值做任何比较，只经由 SkillCatalog 这条通用
    // 接口读取规则。
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = Interner::new();
    // 先登记几个「本体」标识符,再登记 mod 技能——模拟真实加载顺序。
    interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    interner.intern(NamespacedId::parse("lostland:power_strike").expect("合法标识符"));
    let mod_skill = interner.intern(NamespacedId::parse("yourmod:frostbolt").expect("合法标识符"));
    world
        .actors
        .get_mut(actor)
        .expect("刚生成必然存在")
        .unlocked_skills
        .push(mod_skill);
    let catalog = catalog_with(
        mod_skill,
        SkillRule {
            cooldown_ticks: 25,
            resource_cost: ResourceCost::Amount(ResourceKind::Mana, 12),
            effect: SkillEffect::DealDamage { base: 15 },
        },
    );

    // Act
    let effects = resolve_with_skills(
        &world,
        &Intent::UseSkill {
            actor,
            skill: mod_skill,
            target: Some(actor),
        },
        &catalog,
    );

    // Assert
    assert!(effects.iter().any(
        |effect| matches!(effect, Effect::Damage { target, amount } if *target == actor && *amount == 15)
    ));
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::AdjustResource {
            resource: ResourceKind::Mana,
            delta: -12,
            ..
        }
    )));
}

#[test]
fn worldstate的hash纳入已解锁技能与冷却表的变化() {
    // 直接对应 P5-B 任务 5 的判据要求：`hash()` 若不覆盖这两个字段，
    // 技能解锁/冷却结算悄悄跑偏不会被任何确定性回归测试抓到——重演
    // P3 阶段 `WorldState::hash` 完全不含实体状态的同一类判据缺口。
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = Interner::new();
    let skill = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
    let hash_before = world.hash();

    // Act：只改 unlocked_skills 与 skill_cooldowns，不碰任何其他字段。
    let agent = world.actors.get_mut(actor).expect("刚生成必然存在");
    agent.unlocked_skills.push(skill);
    agent.skill_cooldowns.insert(skill, Tick(999));

    // Assert
    assert_ne!(world.hash(), hash_before);
}
