//! [`Effect::AddAffiliation`](ll_sim::effect::Effect::AddAffiliation) 落地时
//! 的两条语义：**同一条 `(kind, org)` 不重复叠加**、**`standing` 夹到满值**。
//!
//! # 为什么这几条测试住在这里，而不是 `apply.rs` 的内联测试模块
//!
//! `crates/ll-sim/src/apply.rs` 在 `scripts/ci/file_size_budget.json` 的行数
//! 棘轮里（它的预算理由写着：这个文件的长度就是 `Effect` 变体的数量本身，
//! 拆开会让「写世界的地方」不止一处，与约束 C1/C2 直接冲突）。**那条理由
//! 管的是实现，不管测试**——把新增的这一组测试放进独立的集成测试文件，
//! 既不动那条棘轮，也不违反它保护的东西。
//!
//! 只用公开入口（[`ll_sim::apply::apply`] + 公开的 `Effect`/`Affiliation`
//! 类型），与 `dialogue_choose.rs` 同一条纪律。
//!
//! 产出这条效果的那一侧（`join-settlement` 的五道闸门）在
//! `crates/ll-sim/tests/dialogue_choose.rs`；端到端那一半在
//! `crates/ll-game/tests/dialogue_session.rs`。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, Interner, NamespacedId, WorldId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_world::entity::{Affiliation, AffiliationKind, Agent, BaseStats, EntityId, OrgRef};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 一张最小的世界——写法与 `dialogue_choose.rs` 的同名帮手一致（集成测试
/// 之间看不见彼此的私有帮手，因此这几行在这里重来一遍）。
fn test_world() -> WorldState {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
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

/// 一个除归属之外全取默认值的实体。
fn spawn_agent(world: &mut WorldState) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        gender: ll_world::entity::Gender::default(),
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
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory: Vec::new(),
        equipment: BTreeMap::new(),
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
        spawned_at: Tick(0),
        remembered_id: None,
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
        home: None,
    })
}

/// 一条指向某个势力的归属。
fn 势力归属(faction: WorldId, standing: i32) -> Affiliation {
    Affiliation {
        kind: AffiliationKind::Faction,
        org: OrgRef::Instance(faction),
        standing,
    }
}

/// [`Effect::AddAffiliation`] 的三条语义各验一遍：**挂上去**、
/// **`standing` 夹到满值**、**同一条 `(kind, org)` 再来一次不叠加**。
///
/// 反例验证（ADR 0022，本批实测）：
///
/// - 把 `Affiliation::clamp_standing(..)` 换成 `affiliation.standing`
///   → 「夹到满值」那一段红（1001 ≠ 1000）。
/// - 把 `already` 那道闸门去掉 → 「不叠加」那一段红（归属变成两条）。
#[test]
fn addaffiliation挂上归属夹紧声望且不重复叠加() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut counter = 0u32;
    let faction = WorldId::next(&mut counter);
    // 对照组前提：挂之前一条归属都没有。
    assert!(
        world
            .actors
            .get(actor)
            .expect("刚生成的实体必然存在")
            .affiliations
            .is_empty()
    );
    // 故意写一个**超过满值**的声望，验夹紧。
    let effect = Effect::AddAffiliation {
        entity: actor,
        affiliation: 势力归属(faction, Affiliation::STANDING_FULL + 1),
    };

    // Act：同一条效果应用两次。
    apply(&mut world, &effect);
    let after_once = world
        .actors
        .get(actor)
        .expect("刚生成的实体必然存在")
        .affiliations
        .clone();
    apply(&mut world, &effect);
    let after_twice = world
        .actors
        .get(actor)
        .expect("刚生成的实体必然存在")
        .affiliations
        .clone();

    // Assert
    assert_eq!(after_once.len(), 1, "第一次必须真的挂上");
    assert_eq!(
        after_once[0].standing,
        Affiliation::STANDING_FULL,
        "超过满值的声望必须被夹到满值"
    );
    assert_eq!(
        after_twice, after_once,
        "同一条 (kind, org) 再来一次整条静默不做，不叠加也不刷新"
    );
}

/// 负方向同样夹到 `-STANDING_FULL`——对称是本批的选择，见
/// [`ll_world::entity::Affiliation::STANDING_FULL`] 文档「负方向为什么也是
/// 1000」一节。
#[test]
fn addaffiliation的负声望夹到负满值() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut counter = 0u32;
    let faction = WorldId::next(&mut counter);

    // Act
    apply(
        &mut world,
        &Effect::AddAffiliation {
            entity: actor,
            affiliation: 势力归属(faction, -Affiliation::STANDING_FULL - 7),
        },
    );

    // Assert
    let affiliations = &world
        .actors
        .get(actor)
        .expect("刚生成的实体必然存在")
        .affiliations;
    assert_eq!(affiliations.len(), 1);
    assert_eq!(affiliations[0].standing, -Affiliation::STANDING_FULL);
}

/// **不同的 `org` 各挂各的**——「不叠加」那道闸门只认「同一条
/// `(kind, org)`」，不能顺手把整类归属都挡掉。
///
/// 没有这一条，一个「已经有这一类归属就不再挂」的实现照样能让上面两条
/// 全绿，而它会让玩家永远只能加入第一个势力。
#[test]
fn 不同势力的归属各挂各的() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let mut counter = 0u32;
    let first = WorldId::next(&mut counter);
    let second = WorldId::next(&mut counter);
    assert_ne!(first, second, "两个势力号必须真的不同");

    // Act
    apply(
        &mut world,
        &Effect::AddAffiliation {
            entity: actor,
            affiliation: 势力归属(first, 250),
        },
    );
    apply(
        &mut world,
        &Effect::AddAffiliation {
            entity: actor,
            affiliation: 势力归属(second, 250),
        },
    );

    // Assert
    let affiliations = &world
        .actors
        .get(actor)
        .expect("刚生成的实体必然存在")
        .affiliations;
    assert_eq!(affiliations.len(), 2, "实际 {affiliations:?}");
    assert_eq!(affiliations[0].org, OrgRef::Instance(first));
    assert_eq!(affiliations[1].org, OrgRef::Instance(second));
}
