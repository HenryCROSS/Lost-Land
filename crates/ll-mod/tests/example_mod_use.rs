//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-item-use-effect` 这个新脚本 API 真的能被
//! `mods/example_mod/items.json5` 调用，且注册出来的消耗品（治疗药水，
//! 使用后恢复 40 点法力）真的能走
//! `ll_sim::resolve::resolve_with_skills_traits_pools_and_items`/
//! `ll_sim::apply::apply` 端到端产生效果——ADR 0018「玩法层内容必须能
//! 从 mod 脚本注册，且要有真实 mod 脚本为证」，本文件是耐久与
//! `Intent::Use` 落地批次（P6 第五批）的那份证据，不能靠
//! `crates/ll-sim/src/resolve.rs`/`crates/ll-mod/src/item.rs` 里的单元
//! 测试自证。
//!
//! 与 `crates/ll-mod/tests/example_mod_equipment.rs`/
//! `example_mod_combat.rs` 同一个理由独立成文件、同一套「装载整个
//! `mods/` 目录，不是只挑 `example_mod`」手法。

use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::item::ItemTable;
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
use ll_sim::apply::apply;
use ll_sim::intent::Intent;
use ll_sim::item::ItemStack;
use ll_sim::resolve::resolve_with_skills_traits_pools_and_items;
use ll_sim::skill::NoSkills;
use ll_sim::traits::{NoTraitGrants, NoTraits};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_equipment.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的物品表与已经解析好的
/// 索引——理由同 `example_mod_equipment.rs::RealModsHandle`。
struct RealModsHandle {
    item: ItemTable,
    healing_potion_id: ContentIndex,
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
    let mut item = ItemTable::new();
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
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/items.json5 注册"))
    };

    RealModsHandle {
        healing_potion_id: resolve("examplemod:healing_potion"),
        item,
    }
}

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

/// 造一个占位实体，站在 `(5, 5)`，背包初始内容由调用方给出——理由同
/// `example_mod_equipment.rs::spawn_agent`。
fn spawn_agent(world: &mut WorldState, inventory: Vec<ItemStack>) -> EntityId {
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
        // 起始法力恒为零：让「用完药水后法力恰好变成 40」这条断言不需要
        // 再减去一个基线值,直接验证效果本身。
        mana: 0,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory,
        equipment: BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
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
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

#[test]
fn 真实注册的治疗药水使用后恢复法力且数量减一() {
    // Arrange
    let RealModsHandle {
        item,
        healing_potion_id,
    } = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(&mut world, vec![ItemStack::new(healing_potion_id, 3)]);

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::Use {
            actor,
            def: healing_potion_id,
        },
        &NoSkills,
        &NoTraitGrants,
        &NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &item,
    );
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert
    let agent = world.actors.get(actor).expect("刚生成的实体必然存在");
    assert_eq!(agent.mana, 40);
    let stack = agent
        .inventory
        .iter()
        .find(|s| s.def == healing_potion_id)
        .expect("还剩两瓶,堆本身仍在背包里");
    assert_eq!(stack.count, 2);
}

#[test]
fn 真实注册的治疗药水堆叠上限大于一且不携带耐久() {
    // 核实「可堆叠物品不该有耐久」这条注册期约束在真实内容上成立——
    // 治疗药水堆叠上限 10，且没有携带耐久上限（若两者同时声明，
    // mods/example_mod/items.json5 装载会在 register-item 这一步直接
    // 失败，`load_real_mods` 已经断言过 `LoadStatus::Loaded`，本测试
    // 额外直接核实 `max_durability` 确实是 `None`，不只是"没报错"）。
    // Arrange
    let RealModsHandle {
        item,
        healing_potion_id,
    } = load_real_mods();

    // Act
    let view = item.get(healing_potion_id).expect("治疗药水应当已经注册");

    // Assert
    assert_eq!((view.stack_limit, view.max_durability), (10, None));
}
