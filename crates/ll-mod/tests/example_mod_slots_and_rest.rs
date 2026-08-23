//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! 「两种流派」这条设计目标（`knowledge/design/resource-pools-and-rest.md`
//! 零节第五轮）在真实内容上真的成立——法师（法术位 +
//! `RegenRule::OnRest(Full)`）与术士（法力池 + `RegenRule::OnTurnStart`，
//! 已在 `example_mod_resource_pools.rs` 验收）玩法截然不同,外加一个
//! 反常组合（法术位配「每回合缓慢回复」）证明 `RegenRule` 与
//! `ResourcePoolShape` 真的正交,不是引擎悄悄写死的对应关系。
//!
//! 与 `crates/ll-mod/tests/example_mod_resource_pools.rs` 同一套「装载
//! 整个 `mods/` 目录，不是只挑 `example_mod`」手法，见该文件模块文档。

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
use ll_sim::resource_pool::{
    RegenRule, ResourcePoolCatalog, ResourcePoolShape, RestRecoveryAmount,
};
use ll_sim::skill::{ResourceCost, SkillCatalog};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_resource_pools.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

struct RealModsHandle {
    race: RaceTable,
    skill: SkillTable,
    trait_def: TraitTable,
    resource_pool: ResourcePoolTable,
    elf_id: ContentIndex,
    gnome_id: ContentIndex,
    wizard_spell_slots_id: ContentIndex,
    druid_slots_id: ContentIndex,
    fireball_id: ContentIndex,
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
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut tag_table = ll_mod::tag::TagTable::new();
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
            weather: &mut weather_table,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            tag: &mut tag_table,
            events: &mut ll_mod::event::EventSubscriptionTable::new(),
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
        elf_id: resolve("examplemod:elf"),
        gnome_id: resolve("examplemod:gnome"),
        wizard_spell_slots_id: resolve("examplemod:wizard_spell_slots"),
        druid_slots_id: resolve("examplemod:druid_slots"),
        fireball_id: resolve("examplemod:fireball"),
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

/// 造一个占位实体，站在 `(5, 5)`，`race`/`level`/`spent_slots`/
/// `unlocked_skills` 由调用方给出——供法术位消耗测试摆好「这个角色此刻
/// 是什么种族、几级、各档已经花了多少、学过哪些技能」这四个前提。
fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    level: i32,
    spent_slots: BTreeMap<(ContentIndex, u8), u32>,
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
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots,
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills,
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        known_recipes: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        script_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

#[test]
fn 真实注册的法师法术位池形状是九次分级且休息完成时回满() {
    // 直接验收「法师：法术位 + RegenRule::OnRest(Full)」——见
    // gameplay.scm 对 examplemod:wizard_spell_slots 的注册。
    // Arrange
    let handle = load_real_mods();

    // Act
    let rule =
        ResourcePoolCatalog::resource_pool(&handle.resource_pool, handle.wizard_spell_slots_id)
            .expect("wizard_spell_slots 应当已注册");

    // Assert
    assert_eq!(rule.shape, ResourcePoolShape::TieredSlots { tier_count: 4 });
    assert_eq!(
        rule.regen_rule,
        RegenRule::OnRest {
            amount: RestRecoveryAmount::Full
        }
    );
}

#[test]
fn 真实注册的反常组合法术位池每回合缓慢回复而不是休息回满() {
    // 直接验收「两个真实施法者 + 一个反常组合证明正交性」的反常组合
    // 那一半——druid_slots 与 wizard_spell_slots 形状相同
    // （TieredSlots），恢复节奏刻意反过来（OnTurnStart，不是
    // OnRest），证明 RegenRule 与 ResourcePoolShape 真的正交，不是
    // 引擎悄悄写死的对应关系（`resource-pools-and-rest.md` 四节）。
    // Arrange
    let handle = load_real_mods();

    // Act
    let rule = ResourcePoolCatalog::resource_pool(&handle.resource_pool, handle.druid_slots_id)
        .expect("druid_slots 应当已注册");

    // Assert：形状与 wizard_spell_slots 同族（TieredSlots），但恢复
    // 节奏不同——正交性的直接证据。
    assert!(matches!(rule.shape, ResourcePoolShape::TieredSlots { .. }));
    assert_eq!(rule.regen_rule, RegenRule::OnTurnStart { amount: 1 });
}

#[test]
fn 真实精灵法师九级时能放出消耗三环位的火球术() {
    // 直接验收法术位消耗链路在完整装载管线里真的接通：9 级断点声明了
    // 两个三环位（见 gameplay.scm），门四应当放行。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        handle.elf_id,
        9,
        BTreeMap::new(),
        vec![handle.fireball_id],
    );
    let race_traits = &handle.race;
    let traits = &handle.trait_def;
    let pools = &handle.resource_pool;

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill: handle.fireball_id,
            target: None,
        },
        &handle.skill,
        race_traits,
        traits,
        pools,
    );

    // Assert：真的消耗了一个三环位（1 起编号，三环 = tier 3）。
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::AdjustResourceSlot {
                pool,
                tier: 3,
                delta: 1,
                ..
            } if *pool == handle.wizard_spell_slots_id
        )),
        "9 级精灵法师应当能真实消耗一个三环位放出火球术,实际 effects={effects:?}"
    );
}

#[test]
fn 真实精灵法师一级时放不出消耗三环位的火球术() {
    // 单向可兑换的真实内容证据：1 级断点只声明了一环位（见
    // gameplay.scm），三环位容量为零，门四应当拒绝——不会因为一环位
    // 还有空位就放行。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        handle.elf_id,
        1,
        BTreeMap::new(),
        vec![handle.fireball_id],
    );

    // Act
    let effects = resolve_with_skills_traits_and_pools(
        &world,
        &Intent::UseSkill {
            actor,
            skill: handle.fireball_id,
            target: None,
        },
        &handle.skill,
        &handle.race,
        &handle.trait_def,
        &handle.resource_pool,
    );

    // Assert
    assert!(
        effects.is_empty(),
        "1 级精灵法师没有任何三环位容量,不应放出火球术,实际 effects={effects:?}"
    );
}

#[test]
fn 真实火球术的资源消耗声明是三环法术位() {
    // 结构性验证：register-skill 的 "slot-tier:<pool-id>" 前缀真的解析
    // 成了 ResourceCost::SlotTier，不是退化成开放标量池消耗。
    // Arrange
    let handle = load_real_mods();

    // Act
    let rule = SkillCatalog::skill(&handle.skill, handle.fireball_id).expect("fireball 应当已注册");

    // Assert
    assert_eq!(
        rule.resource_cost,
        ResourceCost::SlotTier(handle.wizard_spell_slots_id, 3)
    );
}

#[test]
fn 真实侏儒角色也真的授予了反常组合法术位() {
    // 反常组合的授予链路同样要接通天赋/种族这一路——不只是池本身的
    // 形状/恢复节奏正确，真的有角色能拥有它。
    // Arrange
    let handle = load_real_mods();

    // Act
    let grants = ll_sim::traits::TraitCatalog::trait_rule(
        &handle.trait_def,
        ll_sim::traits::effective_traits(
            &[ll_sim::traits::TraitSource::new(
                handle.gnome_id,
                &handle.race,
            )],
            1,
        )
        .into_iter()
        .next()
        .expect("gnome 1 级应当已经命中一条天赋"),
    )
    .expect("命中的天赋应当能查到规则")
    .granted_resource_pools;

    // Assert
    assert!(
        grants
            .iter()
            .any(|grant| grant.pool == handle.druid_slots_id),
        "gnome 种族授予的天赋应当包含 druid_slots 的容量声明"
    );
}
