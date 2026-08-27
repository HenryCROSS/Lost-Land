//! 端到端验证：家具层——真实装载仓库里的 `mods/` 目录，证明
//! `mods/lostland/items.json5` 声明的 `lostland:forge`（`furniture: true`）
//! **经由 [`ll_sim::turn::TurnEngine`]**（本体二进制 `ll-game` 驱动世界
//! 的唯一路径）真的能被放到地上、真的占住那一格、真的当得了三条锻造
//! 配方的场地，且真的不随时间老化消失。
//!
//! # 验收标准（ADR 0018）
//!
//! 与 `example_mod_crafting.rs` 同一条：内容来自真实 `mods/`，**且**
//! 结算必须经由 `TurnEngine` 的公开入口发生。本文件全程只调
//! [`ll_sim::turn::TurnEngine::advance_ai`]，一次都不碰 `ll_sim::resolve`
//! 的任何入口。唯一的例外是老化回收那两条——它不是任何一次 `Intent`
//! 的后果（是系统级被动演化，见
//! `ll_world::state::WorldState::cleanup_aged_ground_items` 文档「为什么
//! 不是 `Effect`/走 `apply`」一节），因此没有经 `TurnEngine` 的路径可走，
//! 与 `ll_game::world` 里那条既有测试同一处境。
//!
//! # 反例守卫
//!
//! 正向那条（放下去 → 地上多了一堆 → 制作认它当场地）单独成立时，
//! 无法排除「其实什么前置都没判，丢什么都能丢、站哪儿都能做」。四条
//! 反例各守一条前置：
//!
//! - [`层不允许建造时家具放不下去`]——`SpaceProfile::buildable`。
//! - [`脚下地形挡路时家具放不下去`]——`TerrainDef::blocks_move`。
//! - [`这一格已经有一件家具时放不下第二件`]——一格一件。
//! - [`脚下没摆家具时锻造配方静默不产出`]——场地前置本身。
//!
//! 外加两条把「家具」与「普通物品」分开的对照：
//! [`普通物品不受放置前置约束`] 与
//! [`老化回收清掉普通物品但留下家具`]。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::recipe::{RecipeTable, RegisteredRecipes};
use ll_mod::recipe_category::RecipeCategoryTable;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::craft::RecipeCatalog;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::quest::NoQuests;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::space_profile::SpaceProfileTable;
use ll_world::state::WorldState;
use ll_world::terrain::{TerrainKind, base_terrain_fixture};
use ll_world::weather::WeatherTable;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_crafting.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 一次真实装载的产物——只留下本文件断言需要的那几张表与索引。
struct RealModsHandle {
    item: ItemTable,
    recipe: RecipeTable,
    recipe_category: RecipeCategoryTable,
    space_profile: SpaceProfileTable,
    weather: WeatherTable,
    /// `lostland:surface`，`buildable: true`。
    surface_profile: ContentIndex,
    /// `lostland:dungeon`，`buildable: false`——反例用。
    dungeon_profile: ContentIndex,
    /// `lostland:forge`，本体第一件家具。
    forge: ContentIndex,
    /// `lostland:iron_ingot`，砌锻炉的食材，也是「普通物品」那一侧的
    /// 对照组。
    iron_ingot: ContentIndex,
    /// `lostland:leather_strip`，打铁短剑的第二味食材。
    leather_strip: ContentIndex,
    /// `lostland:smith_hammer`，打铁短剑的 `required_tool`。
    smith_hammer: ContentIndex,
    /// `lostland:iron_shortsword`，场地前置那条正向断言的成品。
    iron_shortsword: ContentIndex,
    /// `lostland:forge_recipe`，唯一一条产出家具的配方。
    forge_recipe: ContentIndex,
    /// `lostland:iron_shortsword_recipe`，`required_station` 指着锻炉。
    iron_shortsword_recipe: ContentIndex,
}

impl RealModsHandle {
    /// 把真实装载出来的表借成结算目录束——与 `example_mod_crafting.rs`
    /// 的同名方法逐字段同源，唯一的差别是 `ambient` 用真表而不是
    /// [`AmbientSource::NONE`]：本文件要验的 `SpaceProfile::buildable`
    /// 正是经它读出来的。
    fn catalogs<'a>(&'a self, recipes: &'a dyn RecipeCatalog) -> ResolveCatalogs<'a> {
        ResolveCatalogs {
            skills: &NO_SKILLS,
            quests: &NoQuests,
            race_traits: &NO_RACE_TRAITS,
            class_traits: &NO_CLASS_TRAITS,
            subclass_traits: &NO_SUBCLASS_TRAITS,
            trait_defs: &NO_TRAITS,
            pools: &NO_POOLS,
            items: &self.item,
            formulas: &NO_FORMULAS,
            damage_categories: &NoDamageCategories,
            recipes,
            ambient: AmbientSource::new(&self.space_profile, &self.weather),
            experience: &NoExperience,
            skill_tree: &NO_SKILLS,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
        }
    }

    fn recipes(&self) -> RegisteredRecipes<'_> {
        RegisteredRecipes {
            recipes: &self.recipe,
            categories: &self.recipe_category,
        }
    }
}

const NO_SKILLS: ll_sim::skill::NoSkills = ll_sim::skill::NoSkills;
const NO_RACE_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_CLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_SUBCLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_TRAITS: ll_sim::traits::NoTraits = ll_sim::traits::NoTraits;
const NO_POOLS: ll_sim::resource_pool::NoResourcePools = ll_sim::resource_pool::NoResourcePools;
const NO_FORMULAS: ll_sim::formula::NoFormulas = ll_sim::formula::NoFormulas;

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        space_ids,
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
        space_profile,
        weather,
        ..
    } = session;
    let lostland_id = NamespacedId::parse("lostland:self").unwrap();
    let lostland_status = report
        .entries
        .iter()
        .find(|(id, _)| *id == lostland_id)
        .map(|(_, status)| status);
    assert_eq!(
        lostland_status,
        Some(&LoadStatus::Loaded),
        "lostland 本体必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/lostland/ 注册"))
    };

    RealModsHandle {
        surface_profile: space_ids.surface,
        dungeon_profile: space_ids.dungeon,
        forge: resolve("lostland:forge"),
        iron_ingot: resolve("lostland:iron_ingot"),
        leather_strip: resolve("lostland:leather_strip"),
        smith_hammer: resolve("lostland:smith_hammer"),
        iron_shortsword: resolve("lostland:iron_shortsword"),
        forge_recipe: resolve("lostland:forge_recipe"),
        iron_shortsword_recipe: resolve("lostland:iron_shortsword_recipe"),
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
        space_profile,
        weather,
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

/// 一次放置场景的全部输入。
struct Scene {
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
    known_recipes: Vec<ContentIndex>,
    /// 行动者所处空间的层属性索引——`buildable` 从它查出来。
    profile: ContentIndex,
    /// 行动者脚下那一格改写成这种地形。
    ///
    /// **默认值是草地，不是「不写就用世界生成出来的那一格」**：本文件
    /// 的测试世界由 `base_terrain_fixture()` + `GenParams::default()`
    /// 建成，(5, 5) 天然长出来的是深水（`blocks_move` 为真）——ADR 0022
    /// 实例三记录过同一类「测试世界的地形本身退化」的坑。不显式钉住
    /// 这一格，「放得下」与「放不下」两侧的测试会同时因为一个与被测
    /// 逻辑无关的原因而落到同一侧，反例因此变成空跑。
    terrain_underfoot: ContentIndex,
    /// `Some` 时先在行动者脚下那一格摆上这件东西（一堆地面物品）。
    already_on_ground: Option<ContentIndex>,
}

impl Scene {
    fn new(handle: &RealModsHandle, inventory: Vec<ItemStack>) -> Scene {
        Scene {
            inventory,
            equipment: BTreeMap::new(),
            known_recipes: Vec::new(),
            profile: handle.surface_profile,
            terrain_underfoot: base_terrain_fixture().0.grass.index(),
            already_on_ground: None,
        }
    }
}

/// 造一个占位实体，与 `example_mod_crafting.rs::spawn_agent` 同源，
/// 只多了「所处空间的层属性索引由场景指定」这一项。
fn spawn_agent(world: &mut WorldState, pos: (i32, i32), scene: &Scene) -> EntityId {
    let mut interner = Interner::new();
    let placeholder = interner.intern(NamespacedId::parse("lostland:tester").expect("合法"));
    let agent_pos = world.size.wrap(pos.0, pos.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(agent_pos);
    world.actors.spawn(Agent {
        pos: agent_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: placeholder,
        goals: Vec::new(),
        race: placeholder,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory: scene.inventory.clone(),
        equipment: scene.equipment.clone(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: scene.known_recipes.clone(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, scene.profile),
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
    })
}

/// 跑一场「行动者经由 `TurnEngine` 提交恰好一次 `intent`」，返回结算
/// 之后的整个世界（地面物品与背包都要断言，因此不像
/// `example_mod_crafting.rs` 那样只返回背包）。
///
/// # 为什么恰好只结算一次
///
/// 手法与 `example_mod_crafting.rs::craft_via_turn_engine_full` 完全
/// 相同：行动者排在 `Tick(0)`、旁观者（`controlled`）排在 `Tick(1)`，
/// `advance_ai` 先弹出行动者结算一次，下一次弹出的是 `controlled`，
/// 于是立即返回。
fn act_via_turn_engine(
    handle: &RealModsHandle,
    scene: &Scene,
    intent_of: impl Fn(EntityId) -> Intent,
) -> (WorldState, EntityId) {
    let mut world = test_world();
    let actor = spawn_agent(&mut world, (5, 5), scene);
    let bystander_scene = Scene::new(handle, Vec::new());
    let bystander = spawn_agent(&mut world, (9, 9), &bystander_scene);

    let pos = world.actors.get(actor).expect("刚生成").pos;
    assert!(
        world.terrain_at(pos).is_some(),
        "行动者脚下这一格必须已常驻"
    );
    world
        .terrain
        .set_terrain(pos, TerrainKind::from_index(scene.terrain_underfoot));
    if let Some(def) = scene.already_on_ground {
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(def, 1),
            dropped_at: world.clock,
            contents: Vec::new(),
        });
    }

    let mut timeline = Timeline::new();
    timeline.schedule(actor, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let recipes = handle.recipes();
    let catalogs = handle.catalogs(&recipes);
    let mut intent =
        |_world: &WorldState, acting: EntityId, _controlled: EntityId| intent_of(acting);
    engine.advance_ai(
        &mut world,
        bystander,
        &mut intent,
        &catalogs,
        &mut |_, _| {},
    );

    (world, actor)
}

/// 这一格上摆着的那些地面物品的定义索引。
fn ground_defs_at(world: &WorldState, actor: EntityId) -> Vec<ContentIndex> {
    let pos = world.actors.get(actor).expect("行动者还在").pos;
    world
        .ground_items
        .iter()
        .filter(|ground| ground.pos == pos)
        .map(|ground| ground.stack.def)
        .collect()
}

fn count_of(inventory: &[ItemStack], def: ContentIndex) -> u32 {
    inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

// ─────────────────────────── 正向 ───────────────────────────

#[test]
fn 锻炉是一件家具而不是普通物品() {
    // 内容侧的前置条件：下面全部断言的意义都建立在「本体真的有这么
    // 一件带 furniture 标志的物品」上。
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle.item.get(handle.forge).expect("锻炉必须登记在物品表");
    let ingot = handle
        .item
        .get(handle.iron_ingot)
        .expect("铁锭必须登记在物品表");

    // Assert
    assert!(view.furniture, "lostland:forge 必须声明 furniture: true");
    assert_eq!(view.stack_limit, 1, "家具不可堆叠，注册期硬校验");
    assert!(!ingot.furniture, "对照组：铁锭是普通物品，不该被当成家具");
}

#[test]
fn 把锻炉丢在脚下就是把它放置在这一格上() {
    // 正向主干：经 TurnEngine 提交一次 Intent::Drop，背包里的锻炉进到
    // 地面物品里、坐标就是行动者脚下那一格。
    // Arrange
    let handle = load_real_mods();
    let scene = Scene::new(&handle, vec![ItemStack::new(handle.forge, 1)]);

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Drop {
        actor,
        def: handle.forge,
    });

    // Assert
    assert_eq!(ground_defs_at(&world, actor), vec![handle.forge]);
    let inventory = &world
        .actors
        .get(actor)
        .expect("放置不会杀死行动者")
        .inventory;
    assert_eq!(count_of(inventory, handle.forge), 0, "锻炉已经不在背包里");
}

#[test]
fn 脚下摆着锻炉时经回合引擎真的打出铁短剑() {
    // Arrange：食材、工具、已知配方、脚下的锻炉四条全给齐。
    let handle = load_real_mods();
    let mut scene = Scene::new(
        &handle,
        vec![
            ItemStack::new(handle.iron_ingot, 2),
            ItemStack::new(handle.leather_strip, 1),
        ],
    );
    scene
        .equipment
        .insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.smith_hammer, 1));
    scene.known_recipes = vec![handle.iron_shortsword_recipe];
    scene.already_on_ground = Some(handle.forge);

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Craft {
        actor,
        recipe: handle.iron_shortsword_recipe,
    });

    // Assert
    let inventory = &world
        .actors
        .get(actor)
        .expect("制作不会杀死制作者")
        .inventory;
    assert_eq!(count_of(inventory, handle.iron_shortsword), 1);
    assert_eq!(count_of(inventory, handle.iron_ingot), 0);
}

#[test]
fn 砌锻炉这条配方经回合引擎真的产出一座锻炉() {
    // 家具本身必须是**造得出来**的东西，否则这一层在真实玩法里不可达。
    // 砌锻炉是本体唯一一条 requires_discovery 为假的配方，因此这里不
    // 需要预先塞 known_recipes。
    // Arrange
    let handle = load_real_mods();
    let scene = Scene::new(&handle, vec![ItemStack::new(handle.iron_ingot, 6)]);

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Craft {
        actor,
        recipe: handle.forge_recipe,
    });

    // Assert：六块铁锭换一座炉子。
    let inventory = &world
        .actors
        .get(actor)
        .expect("制作不会杀死制作者")
        .inventory;
    assert_eq!(count_of(inventory, handle.forge), 1);
    assert_eq!(count_of(inventory, handle.iron_ingot), 0);
}

// ─────────────────────────── 反例 ───────────────────────────

#[test]
fn 层不允许建造时家具放不下去() {
    // SpaceProfile::buildable 的反例——本体 lostland:dungeon 声明
    // buildable: false。这是这个字段落地至今第一个真实玩法后果，没有
    // 这一条它仍然是一个「声明了没人读」的死字段。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(&handle, vec![ItemStack::new(handle.forge, 1)]);
    scene.profile = handle.dungeon_profile;
    assert!(
        !handle.space_profile.buildable(handle.dungeon_profile),
        "地下城必须声明 buildable: false，否则本反例无意义"
    );
    assert!(
        handle.space_profile.buildable(handle.surface_profile),
        "地表必须声明 buildable: true，否则上面那条正向测试无意义"
    );

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Drop {
        actor,
        def: handle.forge,
    });

    // Assert：地上什么都没有，背包一动不动。
    assert!(ground_defs_at(&world, actor).is_empty());
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.forge), 1);
}

#[test]
fn 脚下地形挡路时家具放不下去() {
    // TerrainDef::blocks_move 的反例——所有者原话「有些地方上已经有
    // 物品了，例如墙啊……应该就没办法再放置其他东西了」：石墙就是那
    // 一格上已经有的那件东西。
    // Arrange
    let handle = load_real_mods();
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    assert!(
        terrain_table.blocks_move(terrain_ids.wall_stone),
        "石墙必须挡路，否则本反例无意义"
    );
    let mut scene = Scene::new(&handle, vec![ItemStack::new(handle.forge, 1)]);
    scene.terrain_underfoot = terrain_ids.wall_stone.index();

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Drop {
        actor,
        def: handle.forge,
    });

    // Assert
    assert!(ground_defs_at(&world, actor).is_empty());
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.forge), 1);
}

#[test]
fn 这一格已经有一件家具时放不下第二件() {
    // 一格一件——与「家具必须 stack_limit: 1」那条注册期校验守的是
    // 同一件事的两半。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(&handle, vec![ItemStack::new(handle.forge, 1)]);
    scene.already_on_ground = Some(handle.forge);

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Drop {
        actor,
        def: handle.forge,
    });

    // Assert：地上仍然只有场景预先摆的那一件。
    assert_eq!(ground_defs_at(&world, actor), vec![handle.forge]);
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.forge), 1);
}

#[test]
fn 脚下没摆家具时锻造配方静默不产出() {
    // 场地前置本身的反例：食材、工具、已知配方三条都满足，只是脚下
    // 空着。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(
        &handle,
        vec![
            ItemStack::new(handle.iron_ingot, 2),
            ItemStack::new(handle.leather_strip, 1),
        ],
    );
    scene
        .equipment
        .insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.smith_hammer, 1));
    scene.known_recipes = vec![handle.iron_shortsword_recipe];

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Craft {
        actor,
        recipe: handle.iron_shortsword_recipe,
    });

    // Assert：食材一块没少——静默失败不消耗任何东西。
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.iron_shortsword), 0);
    assert_eq!(count_of(inventory, handle.iron_ingot), 2);
}

// ────────────────────── 家具 vs 普通物品的分界 ──────────────────────

#[test]
fn 普通物品不受放置前置约束() {
    // 对照组：同一段场景（脚下已经摆着一件家具、层照样是地表），丢的
    // 换成一堆铁锭——普通丢弃一条前置都不判，照样落地。没有这一条，
    // 上面三条反例无法排除「放置前置其实把所有丢弃都拦了」。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(&handle, vec![ItemStack::new(handle.iron_ingot, 3)]);
    scene.already_on_ground = Some(handle.forge);

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Drop {
        actor,
        def: handle.iron_ingot,
    });

    // Assert：锻炉那一格上现在有两堆——原来的锻炉，加上刚丢下的铁锭。
    let mut defs = ground_defs_at(&world, actor);
    defs.sort_by_key(|def| def.get());
    let mut expected = vec![handle.forge, handle.iron_ingot];
    expected.sort_by_key(|def| def.get());
    assert_eq!(defs, expected);
}

#[test]
fn 老化回收清掉普通物品但留下家具() {
    // `WorldState::cleanup_aged_ground_items` 的 `is_permanent` 那一半。
    //
    // 这一条不经 `TurnEngine`：老化回收不是任何一次 `Intent` 的后果，
    // 是系统级被动演化（见该方法文档「为什么不是 `Effect`/走 `apply`」
    // 一节），没有经引擎的路径可走。生产调用点是
    // `ll_game::world::cleanup_aged_ground_items`，它在
    // `ll_game::app::Demo::advance` 每帧真跑一遍，并且正是在那里把本
    // 测试用的这个谓词按 `ItemDef.furniture` 折算出来。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let pos = world.size.wrap(3, 3);
    for def in [handle.forge, handle.iron_ingot] {
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(def, 1),
            dropped_at: world.clock,
            contents: Vec::new(),
        });
    }
    world.advance(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS + 1);

    // Act：谓词与 `ll_game::world::cleanup_aged_ground_items` 逐字相同。
    let removed = world
        .cleanup_aged_ground_items(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS, &|def| {
            handle.item.get(def).is_some_and(|item| item.furniture)
        });

    // Assert：铁锭被清掉，锻炉留下。
    assert_eq!(removed, 1);
    assert_eq!(
        world
            .ground_items
            .iter()
            .map(|ground| ground.stack.def)
            .collect::<Vec<_>>(),
        vec![handle.forge]
    );
}
