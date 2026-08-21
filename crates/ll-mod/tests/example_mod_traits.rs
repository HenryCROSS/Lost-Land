//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-trait`/`register-race-trait` 这两个新脚本 API 真的能被
//! `mods/example_mod/gameplay.scm` 调用，且注册出来的种族天赋真的能
//! 走 `ll_sim::resolve::resolve_with_skills_and_traits` 端到端放出对应
//! 技能——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod
//! 脚本为证」，本文件是那份证据，不能靠单元测试自证。
//!
//! 与 `crates/ll-mod/tests/example_mod_xp_curves.rs` 同一个理由独立
//! 成文件、同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」
//! 手法，见该文件模块文档。

use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve_with_skills_and_traits;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_xp_curves.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引——理由同 `example_mod_xp_curves.rs::RealModsHandle`。
struct RealModsHandle {
    report: ll_mod::load_report::LoadReport,
    race: RaceTable,
    skill: SkillTable,
    trait_def: TraitTable,
    dragonborn_id: ContentIndex,
    draconic_breath_id: ContentIndex,
    breath_weapon_id: ContentIndex,
}

fn load_real_mods_and_resolve() -> RealModsHandle {
    let mut registry = Registry::new();
    let mut terrain = ll_world::terrain::TerrainTable::new();
    let mut class = ClassTable::new();
    let mut skill = SkillTable::new();
    let mut subclass = SubclassTable::new();
    let mut quest = QuestTable::new();
    let mut race = RaceTable::new();
    let mut clip = ClipTable::new();
    let mut xp_curve = XpCurveTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut trait_def = TraitTable::new();
    let mut resource_pool = ll_mod::resource_pool::ResourcePoolTable::new();
    let mut item = ll_mod::item::ItemTable::new();

    let report = load_all(
        Path::new(REAL_MODS_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut terrain,
            class: &mut class,
            skill: &mut skill,
            subclass: &mut subclass,
            quest: &mut quest,
            race: &mut race,
            clip: &mut clip,
            xp_curve: &mut xp_curve,
            xp_curve_bindings: &mut xp_curve_bindings,
            trait_def: &mut trait_def,
            resource_pool: &mut resource_pool,
            item: &mut item,
        },
    );
    let examplemod_id = NamespacedId::parse("examplemod:self").unwrap();
    let examplemod_status = report
        .entries
        .iter()
        .find(|(id, _)| *id == examplemod_id)
        .map(|(_, status)| status);
    assert_eq!(
        examplemod_status,
        Some(&LoadStatus::Loaded),
        "examplemod 必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/gameplay.scm 注册"))
    };

    RealModsHandle {
        dragonborn_id: resolve("examplemod:dragonborn"),
        draconic_breath_id: resolve("examplemod:draconic_breath"),
        breath_weapon_id: resolve("examplemod:breath_weapon"),
        report,
        race,
        skill,
        trait_def,
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

/// 造一个占位实体，站在 `(5, 5)`，不携带任何已解锁技能——`race`/`level`
/// 由调用方给出，理由同 `crates/ll-sim/tests/trait_resolve.rs`。
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
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
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
fn 真实mods目录装载后examplemod被判定为已加载而两个故意写错的mod失败() {
    // Arrange & Act
    let handle = load_real_mods_and_resolve();

    // Assert：与 ll-game 二进制真实运行时的基线一致——loaded=1,
    // failed=2（broken_syntax/broken_whitelist）。
    assert_eq!(handle.report.loaded_count(), 1);
    assert_eq!(handle.report.failed_count(), 2);
}

#[test]
fn 真实注册的龙裔种族在天赋表里携带吐息天赋的解锁等级为一() {
    // Arrange
    let handle = load_real_mods_and_resolve();

    // Act
    let view = handle
        .race
        .get(handle.dragonborn_id)
        .expect("龙裔应当已被真实注册");

    // Assert
    assert_eq!(
        view.traits,
        &[ll_sim::traits::TraitGrant {
            trait_id: handle.draconic_breath_id,
            unlock_level: 1,
        }]
    );
}

#[test]
fn 真实注册的龙裔吐息天赋授予吐息武器技能() {
    // Arrange
    let handle = load_real_mods_and_resolve();

    // Act
    let rule =
        ll_sim::traits::TraitCatalog::trait_rule(&handle.trait_def, handle.draconic_breath_id)
            .expect("龙裔吐息应当已被真实注册");

    // Assert
    assert_eq!(rule.granted_skills, vec![handle.breath_weapon_id]);
}

#[test]
fn 真实龙裔角色在一级时能通过吐息天赋放出从未解锁过的吐息武器() {
    // 端到端验收：agent.unlocked_skills 从头到尾是空的，`resolve` 走的
    // 是 mod 脚本真实注册出来的 RaceTable/TraitTable/SkillTable 三张表
    // ——不是 crates/ll-sim/tests/trait_resolve.rs 里手搭的假目录,是
    // ADR 0018 要求的「真实 mod 脚本为证」的完整闭环。
    // Arrange
    let handle = load_real_mods_and_resolve();
    let mut world = test_world();
    let actor = spawn_agent(&mut world, handle.dragonborn_id, Agent::STARTING_LEVEL);

    // Act
    let effects = resolve_with_skills_and_traits(
        &world,
        &Intent::UseSkill {
            actor,
            skill: handle.breath_weapon_id,
            target: None,
        },
        &handle.skill,
        &handle.race,
        &handle.trait_def,
    );

    // Assert
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "龙裔吐息天赋授予的技能应当真实产出伤害效果,实际 effects={effects:?}"
    );
}
