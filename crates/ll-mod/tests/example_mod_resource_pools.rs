//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-resource-pool`/`register-trait-resource-pool` 这两个新脚本
//! API 真的能被 `mods/example_mod/gameplay.scm` 调用，且注册出来的
//! 法力池/血代价技能真的能走
//! `ll_sim::resolve::resolve_with_skills_traits_and_pools` 端到端放出
//! 对应效果——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实
//! mod 脚本为证」，本文件是资源池落地批次（第一批：法力池/血池）的
//! 那份证据，不能靠单元测试自证。
//!
//! 与 `crates/ll-mod/tests/example_mod_traits.rs` 同一个理由独立成
//! 文件、同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」手法，
//! 见该文件模块文档。

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
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve_with_skills_traits_and_pools;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_traits.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引——理由同 `example_mod_traits.rs::RealModsHandle`。
struct RealModsHandle {
    race: RaceTable,
    skill: SkillTable,
    trait_def: TraitTable,
    resource_pool: ResourcePoolTable,
    half_elf_id: ContentIndex,
    sorcery_points_id: ContentIndex,
    sorcerer_firebolt_id: ContentIndex,
    blood_bolt_id: ContentIndex,
}

fn load_real_mods() -> RealModsHandle {
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
    let mut resource_pool = ResourcePoolTable::new();
    let mut item = ll_mod::item::ItemTable::new();
    let mut formula = ll_mod::formula::FormulaTable::new();
    let mut weapon_category = ll_mod::weapon_category::WeaponCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut damage_category = ll_mod::damage_category::DamageCategoryTable::new();
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
            formula: &mut formula,
            weapon_category: &mut weapon_category,
            damage_category: &mut damage_category,
            space_profile: &mut space_profile,
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
        half_elf_id: resolve("examplemod:half_elf"),
        sorcery_points_id: resolve("examplemod:sorcery_points"),
        sorcerer_firebolt_id: resolve("examplemod:sorcerer_firebolt"),
        blood_bolt_id: resolve("examplemod:blood_bolt"),
        race,
        skill,
        trait_def,
        resource_pool,
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

/// 造一个占位实体，站在 `(5, 5)`，`resource_pools`/`unlocked_skills`
/// 由调用方给出——供法力池消耗测试摆好「当前已经有多少点法力、学过
/// 哪个技能」这两个前提，理由同
/// `crates/ll-sim/tests/resource_pool_resolve.rs::spawn_agent_with_pool`。
fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    health: i32,
    resource_pools: BTreeMap<ContentIndex, i32>,
    unlocked_skills: Vec<ContentIndex>,
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
        resource_pools,
        spent_slots: BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills,
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        script_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
    })
}

#[test]
fn 真实注册的半精灵种族在天赋表里携带术法天赋的解锁等级为一() {
    // Arrange
    let handle = load_real_mods();
    let innate_sorcery_id = handle
        .race
        .get(handle.half_elf_id)
        .expect("半精灵应当已被真实注册")
        .traits
        .iter()
        .find(|grant| grant.unlock_level == 1)
        .map(|grant| grant.trait_id);

    // Act & Assert
    assert!(innate_sorcery_id.is_some());
}

#[test]
fn 真实注册的术法天赋授予法力池二十点固定容量() {
    // Arrange
    let handle = load_real_mods();
    let view = handle
        .race
        .get(handle.half_elf_id)
        .expect("半精灵应当已被真实注册");
    let trait_id = view.traits[0].trait_id;
    let rule = ll_sim::traits::TraitCatalog::trait_rule(&handle.trait_def, trait_id)
        .expect("术法天赋应当已被真实注册");

    // Act
    let grant = &rule.granted_resource_pools[0];

    // Assert
    assert_eq!(
        grant,
        &ll_sim::resource_pool::ResourcePoolGrant {
            pool: handle.sorcery_points_id,
            capacity: ll_sim::resource_pool::CapacityFormula::Fixed(20),
        }
    );
}

#[test]
fn 真实半精灵角色放出法力箭后产出法力池扣减效果() {
    // 端到端验收：走 mod 脚本真实注册出来的 RaceTable/TraitTable/
    // SkillTable/ResourcePoolTable 四张表——不是
    // crates/ll-sim/tests/resource_pool_resolve.rs 里手搭的假目录,是
    // ADR 0018 要求的「真实 mod 脚本为证」的完整闭环。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        handle.half_elf_id,
        100,
        BTreeMap::from([(handle.sorcery_points_id, 20)]),
        vec![handle.sorcerer_firebolt_id],
    );

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill: handle.sorcerer_firebolt_id,
            target: None,
        },
        &handle.skill,
        &handle.race,
        &handle.trait_def,
        &handle.resource_pool,
    );

    // Assert：本回合同时触发了 +2 的每回合恢复(同一个池)与 -5 的
    // 施法消耗,断言恰好存在一条 delta 为 -5 的扣减效果——不要求整批
    // effects 只有这一条(regen 同时存在是既有设计,不是需要排除的噪音)。
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::AdjustResourcePool { pool, delta: -5, .. } if *pool == handle.sorcery_points_id
        )),
        "半精灵放出法力箭应当真实扣减法力池,实际 effects={effects:?}"
    );
}

#[test]
fn 真实半精灵角色法力不足时法力箭放不出来() {
    // Arrange：法力池当前只剩 3 点，法力箭需要 5 点。
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        handle.half_elf_id,
        100,
        BTreeMap::from([(handle.sorcery_points_id, 3)]),
        vec![handle.sorcerer_firebolt_id],
    );

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill: handle.sorcerer_firebolt_id,
            target: None,
        },
        &handle.skill,
        &handle.race,
        &handle.trait_def,
        &handle.resource_pool,
    );

    // Assert：技能本身不产出任何效果(门四挡住)——每回合 +2 的恢复
    // 效果仍然存在(那是"这个实体的回合开始"这件事本身触发的,不属于
    // 本次施法的产出),因此断言的是"没有伤害/没有施法消耗",不是
    // "effects 整体为空"。
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "法力不足时不应产出伤害效果,实际 effects={effects:?}"
    );
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            Effect::AdjustResourcePool { delta, .. } if *delta < 0
        )),
        "法力不足时不应产出施法消耗效果,实际 effects={effects:?}"
    );
}

#[test]
fn 真实半精灵角色每回合开始时法力池按注册的节奏回复() {
    // Arrange：gameplay.scm 把 sorcery_points 注册成
    // (regen-on-turn-start 2)，起始当前值为 0。
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        handle.half_elf_id,
        100,
        BTreeMap::new(),
        Vec::new(),
    );

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::Wait { actor },
        &handle.skill,
        &handle.race,
        &handle.trait_def,
        &handle.resource_pool,
    );

    // Assert
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::AdjustResourcePool { pool, delta: 2, .. } if *pool == handle.sorcery_points_id
        )),
        "每回合开始应当真实产出 +2 的法力池回复效果,实际 effects={effects:?}"
    );
}

#[test]
fn 真实血法师角色血量不足以支付血代价时会被自己的法术杀死() {
    // Arrange：血量 10，blood_bolt 声明的血代价是 15——血代价不设最低
    // 血量兜底，见 resource-pools-and-rest.md 五节。血代价不需要任何
    // 天赋授予"使用许可"，随便一个种族都能表达。
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        handle.half_elf_id,
        10,
        BTreeMap::new(),
        vec![handle.blood_bolt_id],
    );

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill: handle.blood_bolt_id,
            target: None,
        },
        &handle.skill,
        &handle.race,
        &handle.trait_def,
        &handle.resource_pool,
    );

    // Assert
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::Kill { target, killer: Some(killer), .. }
                if *target == actor && *killer == actor
        )),
        "血量不足以支付血代价时应当自尽,实际 effects={effects:?}"
    );
}
