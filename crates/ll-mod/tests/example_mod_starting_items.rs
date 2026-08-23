//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-race-starting-item` 这个新脚本 API 真的能被
//! `mods/example_mod/gameplay.scm` 调用，且真实注册的出生物品：
//!
//! 1. 能被 [`ll_mod::race::starting_inventory`] 转换成背包物品；
//! 2. 一旦发给一个真实存在的实体（哥布林），死亡结算真的会把它们
//!    （连同已装备物品）打包进一具尸体（[`Intent::Attack`] →
//!    `resolve_with_skills_traits_pools_and_items` →
//!    `crate::resolve` 内部的 `append_corpse_drop`，本文件看不到那个
//!    私有函数，只能通过端到端的公开入口验证它的效果）；
//! 3. 尸体不会被普通 [`Intent::PickUp`] 吞掉，只能通过
//!    [`Intent::Loot`] 搜刮。
//!
//! ——NPC 生命周期批次（NPC 带物品 → 死亡掉落 → 尸体 → 老化回收）
//! 端到端的那份证据,与 `crates/ll-mod/tests/example_mod_equipment.rs`
//! 同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」手法（见其
//! 模块文档），ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实
//! mod 脚本为证」。

use std::collections::BTreeMap;
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
use ll_mod::race::{RaceTable, starting_inventory};
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::apply::apply;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::resolve::resolve_with_skills_traits_pools_and_items;
use ll_sim::skill::NoSkills;
use ll_sim::traits::{NoTraitGrants, NoTraits};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_items.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引——理由同 `example_mod_equipment.rs::RealModsHandle`。
struct RealModsHandle {
    item: ItemTable,
    race: RaceTable,
    goblin_id: ContentIndex,
    crude_dagger_id: ContentIndex,
    arrow_id: ContentIndex,
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
        goblin_id: resolve("examplemod:goblin"),
        crude_dagger_id: resolve("examplemod:crude_dagger"),
        arrow_id: resolve("examplemod:arrow"),
        item,
        race,
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

/// 造一个占位实体——理由同 `example_mod_equipment.rs::spawn_agent`，
/// 额外携带 `race`/`health`/`stats`（死亡掉落测试需要真的能被杀死、
/// 真的能在攻击公式里算出非零伤害）。
#[allow(clippy::too_many_arguments)]
fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    stats: BaseStats,
    health: i32,
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
    pos_offset: (i32, i32),
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5 + pos_offset.0, 5 + pos_offset.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        pos,
        stats,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory,
        equipment,
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
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

/// 把 `intent` 结算并应用到 `world`——本文件全部测试共用的一步。
fn resolve_and_apply(world: &mut WorldState, intent: &Intent, items: &ItemTable) {
    let effects = resolve_with_skills_traits_pools_and_items(
        world,
        intent,
        &NoSkills,
        &NoTraitGrants,
        &NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        items,
    );
    for effect in &effects {
        apply(world, effect);
    }
}

#[test]
fn 真实注册的哥布林出生物品是粗制匕首一把与箭两支() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");

    // Assert
    assert_eq!(
        view.starting_items,
        &[(handle.crude_dagger_id, 1), (handle.arrow_id, 2)]
    );
}

#[test]
fn 真实哥布林出生物品转换成对应的两条物品堆() {
    // Arrange
    let handle = load_real_mods();
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");

    // Act
    let inventory = starting_inventory(&view);

    // Assert
    assert_eq!(
        inventory,
        vec![
            ItemStack::new(handle.crude_dagger_id, 1),
            ItemStack::new(handle.arrow_id, 2),
        ]
    );
}

#[test]
fn 携带出生物品的哥布林被杀死后背包物品完整进入尸体() {
    // 死亡掉落端到端验收（NPC 生命周期批次）：给一个真实存在的哥布林
    // 发真实 mod 注册的出生物品 → 用真实的 Intent::Attack 结算杀死它
    // → 断言地上真的出现了尸体，且尸体里装的战利品与死者结算前的背包
    // 完全一致——数量守恒,不多不少。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");
    let loadout = starting_inventory(&view);
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1, // 一击必杀,不依赖具体伤害公式的精确取值。
        loadout.clone(),
        BTreeMap::new(),
        (0, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50, // 调整值 (50-10)/2 = 20,远高于 1 点血量。
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (0, 0),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );

    // Assert：受害者已死，地面上出现恰好一具尸体，装着的战利品与出生
    // 时的背包逐条相等（总数守恒：既没有物品凭空消失，也没有多出）。
    assert!(world.actors.get(victim).is_none());
    assert_eq!(world.ground_items.len(), 1);
    let corpse = &world.ground_items[0];
    assert!(!corpse.contents.is_empty());
    assert_eq!(corpse.contents, loadout);
}

#[test]
fn 携带已装备物品的哥布林被杀死后装备也进入尸体() {
    // 装备也要掉：死者身上穿着的物品（Agent::equipment）同样要出现在
    // 尸体的战利品里，不只是背包（Agent::inventory）。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let equipped = ItemStack::new(handle.crude_dagger_id, 1);
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1,
        Vec::new(),
        BTreeMap::from([(EquipSlot::MAIN_HAND, equipped)]),
        (1, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50,
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (1, 0),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );

    // Assert：已装备的匕首出现在尸体战利品里。
    assert_eq!(world.ground_items.len(), 1);
    assert_eq!(world.ground_items[0].contents, vec![equipped]);
}

#[test]
fn 空手死者不产出可搜刮的尸体() {
    // 背包与装备栏都空的死者不应该占一个地面物品条目——见
    // append_corpse_drop 文档「空手死者不产出尸体」一节。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1,
        Vec::new(),
        BTreeMap::new(),
        (2, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50,
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (2, 0),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );

    // Assert
    assert!(world.ground_items.is_empty());
}

#[test]
fn 普通拾取跳过尸体不吞掉其战利品() {
    // Intent::PickUp 不是尸体的合法目标——见 resolve_pick_up 文档
    // 「为什么跳过容器」一节：普通拾取只会搬走 GroundItemStack.stack
    // 这个壳，contents 里的真实战利品会被丢在地上永久不可达,这是必须
    // 避免的数据丢失,不是可接受的降级。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");
    let loadout = starting_inventory(&view);
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1,
        loadout.clone(),
        BTreeMap::new(),
        (3, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50,
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (3, 0),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );
    assert_eq!(world.ground_items.len(), 1, "前置条件：尸体已经产出");

    // Act：攻击者站在尸体所在格,尝试普通拾取。
    resolve_and_apply(
        &mut world,
        &Intent::PickUp { actor: attacker },
        &handle.item,
    );

    // Assert：尸体原封不动地留在地面上,攻击者背包没有多出任何东西。
    assert_eq!(world.ground_items.len(), 1);
    assert_eq!(world.ground_items[0].contents, loadout);
    assert!(world.actors.get(attacker).unwrap().inventory.is_empty());
}

#[test]
fn 搜刮尸体后战利品进入背包且尸体从地面消失() {
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let view = handle
        .race
        .get(handle.goblin_id)
        .expect("哥布林应当已被真实注册");
    let loadout = starting_inventory(&view);
    let victim = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats::BASELINE,
        1,
        loadout.clone(),
        BTreeMap::new(),
        (4, 0),
    );
    let attacker = spawn_agent(
        &mut world,
        handle.goblin_id,
        BaseStats {
            strength: 50,
            ..BaseStats::BASELINE
        },
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        (4, 0),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );
    assert_eq!(world.ground_items.len(), 1, "前置条件：尸体已经产出");

    // Act
    resolve_and_apply(&mut world, &Intent::Loot { actor: attacker }, &handle.item);

    // Assert：尸体从地面消失,战利品原样进了攻击者背包。
    assert!(world.ground_items.is_empty());
    assert_eq!(world.actors.get(attacker).unwrap().inventory, loadout);
}
