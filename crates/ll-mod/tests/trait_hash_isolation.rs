//! 天赋是纯派生，不进 `WorldState::hash()`——
//! `knowledge/design/trait-system.md` 八节「派生还是存储」的直接验收：
//! `TraitTable`/`RaceDef.traits` 是注册表数据的一部分（与
//! `RaceTable`/`SkillTable` 等既有内容表同一类），`crates/ll-world/src/state.rs`
//! 的 `hash()` 只哈希 `Agent` 的实例字段（`race`/`level`/……），从不
//! 哈希任何 `*Table` 注册表本身——`hash()` 的签名 `fn hash(&self) -> u64`
//! 甚至不接受任何天赋相关的参数，架构上不可能把天赋内容混进去。
//!
//! 本文件用两个 `RaceTable`/`TraitTable` 配置完全不同（一个种族授予
//! 天赋、另一个不授予任何天赋）、但 `WorldState` 存储内容（`Agent` 的
//! 全部字段，包括 `race`/`level`）完全相同的世界，断言两者 `hash()`
//! 相同——直接对应任务验收要求的「两个天赋不同但存储状态相同的世界，
//! 哈希相同」。
//!
//! # 与 `Agent.level` 进哈希不矛盾
//!
//! `d54c780` 已经把 `agent.level` 编进 `hash()`（等级与经验系统落地
//! 批次）——本文件两个世界的 `Agent.level` 取的是**同一个**数值,天赋
//! 系统不改变这一点：天赋本身（`TraitDef`/`TraitTable`/`RaceDef.traits`）
//! 不进哈希,但它引用的 `Agent.level` 进哈希,两者是不同的东西,不要
//! 混淆——本文件故意保持 `level` 相同,只改变外部 `RaceTable`/
//! `TraitTable` 的内容,精确隔离出"天赋定义本身"这一个变量。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::race::{RaceAttrs, RaceTable};
use ll_mod::trait_def::{TraitAttrs, TraitTable};
use ll_sim::traits::TraitGrant;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

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

/// 造一个占位实体，`race`/`level` 由调用方给出——两个世界会传入完全
/// 相同的值，唯一变化的是外部 `RaceTable`/`TraitTable` 的内容,后者
/// 从不被写进 `Agent` 本身,也就从不被写进 `WorldState`。
fn spawn_agent(world: &mut WorldState, race: ContentIndex, level: i32) -> EntityId {
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: ContentIndex::default(),
        goals: Vec::new(),
        race,
        luck: 0,
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
        spawned_at: Tick(0),
        remembered_id: None,
        level,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
    })
}

#[test]
fn 天赋定义不同但世界存储状态相同时哈希相同() {
    // Arrange：两套完全独立的 Interner/Registry/RaceTable/TraitTable——
    // 世界 A 的种族授予一条天赋（该天赋授予一个技能），世界 B 的同名
    // 种族不授予任何天赋。两个世界各自 spawn 一个 Agent，`race` 索引
    // 与 `level` 完全相同（`Interner` 从零开始、按相同顺序 intern 相同
    // 数量的字符串，两边分配到的 `ContentIndex` 数值因此恒等——这不是
    // 巧合，是 `ll_core::ident::Interner`「索引来自插入顺序」的既有
    // 契约）。
    let mut interner_a = Interner::new();
    let race_a = interner_a.intern(NamespacedId::parse("lostland:dragonborn").unwrap());
    let trait_a = interner_a.intern(NamespacedId::parse("lostland:draconic_breath").unwrap());
    let skill_a = interner_a.intern(NamespacedId::parse("lostland:breath_weapon").unwrap());
    let mut race_table_a = RaceTable::new();
    race_table_a
        .define(
            race_a,
            RaceAttrs {
                display_name_key: NamespacedId::parse("lostland:race.dragonborn").unwrap(),
                stat_modifiers: BaseStats::BASELINE,
                darkvision_floor: 0,
                footprint: (1, 1),
                lifespan_years: 80,
                xp_reward: 0,
                traits: vec![TraitGrant {
                    trait_id: trait_a,
                    unlock_level: 1,
                }],
                starting_items: Vec::new(),
            },
        )
        .expect("世界 A 种族声明内部自洽");
    let mut trait_table_a = TraitTable::new();
    trait_table_a
        .define(
            trait_a,
            TraitAttrs {
                display_name_key: NamespacedId::parse("lostland:trait.draconic_breath").unwrap(),
                granted_skills: vec![skill_a],
                stat_modifiers: Vec::new(),
                rule_modifiers: Vec::new(),
                granted_resource_pools: Vec::new(),
            },
        )
        .expect("世界 A 天赋声明内部自洽");

    let mut interner_b = Interner::new();
    let race_b = interner_b.intern(NamespacedId::parse("lostland:dragonborn").unwrap());
    // 世界 B 也 intern 出与世界 A 数量相同的两个额外字符串——保证
    // Interner 的号段分配与世界 A 完全对齐（即使这两个 id 在世界 B
    // 里从未被任何 RaceTable/TraitTable 引用），使 race_b 的
    // ContentIndex 数值与 race_a 恒等。
    let _unused_trait_b =
        interner_b.intern(NamespacedId::parse("lostland:draconic_breath").unwrap());
    let _unused_skill_b = interner_b.intern(NamespacedId::parse("lostland:breath_weapon").unwrap());
    let mut race_table_b = RaceTable::new();
    race_table_b
        .define(
            race_b,
            RaceAttrs {
                display_name_key: NamespacedId::parse("lostland:race.dragonborn").unwrap(),
                stat_modifiers: BaseStats::BASELINE,
                darkvision_floor: 0,
                footprint: (1, 1),
                lifespan_years: 80,
                xp_reward: 0,
                // 世界 B 的关键差异：这个种族不授予任何天赋。
                traits: Vec::new(),
                starting_items: Vec::new(),
            },
        )
        .expect("世界 B 种族声明内部自洽");
    let trait_table_b = TraitTable::new(); // 世界 B 完全没有注册任何天赋。

    assert_eq!(
        race_a, race_b,
        "两个 Interner 按相同顺序 intern 相同字符串,索引必须恒等——\
         这是本测试能只比较 WorldState::hash() 就隔离出天赋差异的前提"
    );

    let mut world_a = test_world();
    spawn_agent(&mut world_a, race_a, 5);
    let mut world_b = test_world();
    spawn_agent(&mut world_b, race_b, 5);

    // Act
    let hash_a = world_a.hash();
    let hash_b = world_b.hash();

    // Assert：两个世界的天赋定义（`race_table_a`/`trait_table_a` 授予
    // 一条天赋和一个技能，`race_table_b`/`trait_table_b` 什么都不授予）
    // 完全不同，但从未被读取进 `hash()`——两者哈希必须相同,证明天赋是
    // 纯派生,不进存档摘要。
    assert_eq!(hash_a, hash_b);
    // 防呆：确认 `race_table_a`/`trait_table_a` 确实携带着与 `_b`
    // 不同的内容,而不是本测试不小心把两套表都造成了空的——否则上面的
    // assert_eq! 会变成一句恒真的废话。
    assert!(!race_table_a.get(race_a).unwrap().traits.is_empty());
    assert!(race_table_b.get(race_b).unwrap().traits.is_empty());
    assert!(trait_table_a.is_defined(trait_a));
    assert!(!trait_table_b.is_defined(trait_a));
}
