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
//! # 家具放置状态批次改了什么
//!
//! 项目所有者裁定「放置」与「丢弃」是两个动作，而「立着还是躺着」是
//! **每个地面实例上的状态**，不是物品定义上的标志（完整原话与论证见
//! `ll_world::item::GroundItemStack::placed` 与
//! `ll_sim::intent::Intent::Place` 两处文档）。本文件因此从「丢一件家具
//! 就是放置」改成分别验两条路径：
//!
//! - `Intent::Place` → 地上多出一堆 `placed == true` 的东西，四道前置。
//! - `Intent::Drop` → 地上多出一堆 `placed == false` 的东西，一道前置
//!   （这一格立没立着东西）。
//!
//! # 反例守卫
//!
//! 正向那条（立起来 → 地上多了一堆立着的 → 制作认它当场地）单独成立
//! 时，无法排除「其实什么前置都没判」。反例各守一条前置：
//!
//! - [`层不允许建造时家具立不起来`]——`SpaceProfile::buildable`。
//! - [`脚下地形挡路时家具立不起来`]——`TerrainDef::blocks_move`。
//! - [`这一格已经立着东西时立不下第二件`]——一格至多一件放置物。
//! - [`不是家具的东西立不起来`]——`ItemDef::furniture` 那一道。
//! - [`脚下没立着东西时锻造配方静默不产出`]——场地前置本身。
//! - [`脚下的锻炉只是躺着时锻造配方静默不产出`]——场地前置认的是
//!   **放置状态**，不是「这格上有没有一件带 furniture 标志的东西」。
//!
//! 外加把两种状态分开的对照：[`这一格立着东西时连普通物品也丢不下去`]、
//! [`这一格只躺着别的东西时普通物品照样丢得下去`]、
//! [`丢下去的锻炉是躺着的立着的才不老化`]。

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
            // 对话这两路（对话批次 2 新增）：本条测试与对话无关，
            // 接空实现即可。
            dialogues: &ll_sim::dialogue::NoDialogues,
            content_ids: &ll_sim::dialogue::NoContentIds,
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
    /// 先在行动者脚下那一格摆上这些东西——`(定义, 是不是立着的)`。
    ///
    /// 带上「立没立着」这一位是家具放置状态批次的要求：同一件炉子躺着
    /// 还是立着是两种不同的世界状态（见
    /// `ll_world::item::GroundItemStack::placed` 文档），而本文件的反例
    /// 恰恰要分别摆出这两种。
    already_on_ground: Vec<(ContentIndex, bool)>,
}

impl Scene {
    fn new(handle: &RealModsHandle, inventory: Vec<ItemStack>) -> Scene {
        Scene {
            inventory,
            equipment: BTreeMap::new(),
            known_recipes: Vec::new(),
            profile: handle.surface_profile,
            terrain_underfoot: base_terrain_fixture().0.grass.index(),
            already_on_ground: Vec::new(),
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
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
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
        home: None,
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
    for (def, placed) in &scene.already_on_ground {
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(*def, 1),
            dropped_at: world.clock,
            contents: Vec::new(),
            placed: *placed,
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
fn 把锻炉立在脚下这一格上() {
    // 正向主干：经 TurnEngine 提交一次 Intent::Place，背包里的锻炉进到
    // 地面物品里、坐标就是行动者脚下那一格，且它是**立着**的。
    // Arrange
    let handle = load_real_mods();
    let scene = Scene::new(&handle, vec![ItemStack::new(handle.forge, 1)]);

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Place {
        actor,
        def: handle.forge,
    });

    // Assert
    assert_eq!(ground_defs_at(&world, actor), vec![handle.forge]);
    let pos = world.actors.get(actor).expect("放置不会杀死行动者").pos;
    assert_eq!(
        world.placed_at(pos).map(|ground| ground.stack.def),
        Some(handle.forge),
        "立起来的那一堆 placed 必须为真——否则它当不了场地、也挡不住别人往这格丢东西"
    );
    let inventory = &world
        .actors
        .get(actor)
        .expect("放置不会杀死行动者")
        .inventory;
    assert_eq!(count_of(inventory, handle.forge), 0, "锻炉已经不在背包里");
}

#[test]
fn 丢下去的锻炉是躺着的不是立着的() {
    // 所有者裁定的后半句：「如果家具作为一个物品而不是放置状态，就会
    // 和其他物品被丢在同一个地方」。同一件锻炉，走 Drop 而不是 Place，
    // 落地的必须是一堆**普通地面物品**。
    //
    // 这条是本批次改动的核心反例：家具层那一批把 Drop 当 Place 用，
    // 那时这条断言会红（丢下去的炉子会是立着的）。
    // Arrange
    let handle = load_real_mods();
    let scene = Scene::new(&handle, vec![ItemStack::new(handle.forge, 1)]);

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Drop {
        actor,
        def: handle.forge,
    });

    // Assert：东西在地上，但它没立起来。
    assert_eq!(ground_defs_at(&world, actor), vec![handle.forge]);
    let pos = world.actors.get(actor).expect("行动者还在").pos;
    assert!(
        world.placed_at(pos).is_none(),
        "丢下去的东西不该是立着的——立起来要按放置键走 Intent::Place"
    );
}

#[test]
fn 脚下立着锻炉时经回合引擎真的打出铁短剑() {
    // Arrange：食材、工具、已知配方、脚下**立着**的锻炉四条全给齐。
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
    scene.already_on_ground = vec![(handle.forge, true)];

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

#[test]
fn 立起来的锻炉可以再被捡回背包() {
    // 「摆下去还能收回来」这条闭环的出口——放置状态不影响能不能捡，
    // 见 `ll_sim::resolve` 的 `resolve_pick_up` 文档。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(&handle, Vec::new());
    scene.already_on_ground = vec![(handle.forge, true)];

    // Act
    // 站在炉子那一格上捡它——`Intent::PickUp` 现在点名从哪一格捡
    // （见其 `pos` 字段文档「够得着的范围」一节）。`act_via_turn_engine`
    // 把行动者生成在 (5, 5)，场景预先摆的东西也在同一格。
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::PickUp {
        actor,
        pos: (5, 5),
        def: handle.forge,
    });

    // Assert
    assert!(ground_defs_at(&world, actor).is_empty(), "地上不该还有炉子");
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.forge), 1);
}

// ─────────────────────────── 反例 ───────────────────────────

#[test]
fn 层不允许建造时家具立不起来() {
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
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Place {
        actor,
        def: handle.forge,
    });

    // Assert：地上什么都没有，背包一动不动。
    assert!(ground_defs_at(&world, actor).is_empty());
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.forge), 1);
}

#[test]
fn 脚下地形挡路时家具立不起来() {
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
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Place {
        actor,
        def: handle.forge,
    });

    // Assert
    assert!(ground_defs_at(&world, actor).is_empty());
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.forge), 1);
}

#[test]
fn 这一格已经立着东西时立不下第二件() {
    // 一格至多一件放置物——与「家具必须 stack_limit: 1」那条注册期
    // 校验守的是同一件事的两半。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(&handle, vec![ItemStack::new(handle.forge, 1)]);
    scene.already_on_ground = vec![(handle.forge, true)];

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Place {
        actor,
        def: handle.forge,
    });

    // Assert：地上仍然只有场景预先立的那一件。
    assert_eq!(ground_defs_at(&world, actor), vec![handle.forge]);
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.forge), 1);
}

#[test]
fn 不是家具的东西立不起来() {
    // `resolve_place` 第 ① 道前置：`ItemDef::furniture` 回答的是「这东西
    // **能不能**被放置」。铁锭不能，因此按放置键对它一点反应都没有。
    // 没有这一条，无法排除「放置其实什么都不问，什么都能立」。
    // Arrange
    let handle = load_real_mods();
    let scene = Scene::new(&handle, vec![ItemStack::new(handle.iron_ingot, 3)]);

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Place {
        actor,
        def: handle.iron_ingot,
    });

    // Assert：地上什么都没有，铁锭还在背包里。
    assert!(ground_defs_at(&world, actor).is_empty());
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.iron_ingot), 3);
}

#[test]
fn 脚下没立着东西时锻造配方静默不产出() {
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

#[test]
fn 脚下的锻炉只是躺着时锻造配方静默不产出() {
    // 家具放置状态批次最关键的一条反例：场地前置认的是**放置状态**，
    // 不是「这一格上有没有一件带 furniture 标志的东西」。
    //
    // 与上一条的唯一差别是脚下这一格上确实有一座锻炉，只是它躺着。
    // 把 `resolve_craft` 第 ⑤ 步换回旧判据（找一件带 furniture 标志的
    // 地面物品），本条立刻变红。
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
    scene.already_on_ground = vec![(handle.forge, false)];

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Craft {
        actor,
        recipe: handle.iron_shortsword_recipe,
    });

    // Assert
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.iron_shortsword), 0);
    assert_eq!(count_of(inventory, handle.iron_ingot), 2);
}

// ────────────────────── 立着 vs 躺着的分界 ──────────────────────

#[test]
fn 这一格立着东西时连普通物品也丢不下去() {
    // 所有者原话的前半句：「家具如果是放置在那个地方，那物品就无法被
    // 丢在那」，另加一句「普通物品和第一点应该是一样的」——**受约束的
    // 是这一格立着东西这个事实，与手里丢的是什么无关**。
    //
    // 家具层那一批的规则恰好相反（普通物品完全不受约束，锻炉那一格
    // 照样能丢铁锭），本条就是那条旧规则的反例。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(&handle, vec![ItemStack::new(handle.iron_ingot, 3)]);
    scene.already_on_ground = vec![(handle.forge, true)];

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Drop {
        actor,
        def: handle.iron_ingot,
    });

    // Assert：地上仍然只有那座立着的炉子，铁锭还在背包里。
    assert_eq!(ground_defs_at(&world, actor), vec![handle.forge]);
    let inventory = &world.actors.get(actor).expect("行动者还在").inventory;
    assert_eq!(count_of(inventory, handle.iron_ingot), 3);
}

#[test]
fn 这一格只躺着别的东西时普通物品照样丢得下去() {
    // 上一条的对照组：拦住丢弃的是「立着」这一位，不是「这格上已经有
    // 东西」。没有这一条，无法排除「其实是只要这格上有东西就丢不了」。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(&handle, vec![ItemStack::new(handle.iron_ingot, 3)]);
    scene.already_on_ground = vec![(handle.forge, false)];

    // Act
    let (world, actor) = act_via_turn_engine(&handle, &scene, |actor| Intent::Drop {
        actor,
        def: handle.iron_ingot,
    });

    // Assert：两堆并存。
    let mut defs = ground_defs_at(&world, actor);
    defs.sort_by_key(|def| def.get());
    let mut expected = vec![handle.forge, handle.iron_ingot];
    expected.sort_by_key(|def| def.get());
    assert_eq!(defs, expected);
}

#[test]
fn 丢下去的锻炉是躺着的立着的才不老化() {
    // `WorldState::cleanup_aged_ground_items` 的「哪些永不老化」那一半。
    //
    // 判据从 `ItemDef.furniture`（内容标志）换成了 `placed`（实例状态）
    // ——**这是一次语义更正，不只是重构**：一座躺在地上没立起来的炉子
    // 此前也享受永久豁免，现在照常老化。场景里因此摆三堆：立着的炉子、
    // 躺着的炉子、一堆铁锭，只有第一堆该留下。
    //
    // 这一条不经 `TurnEngine`：老化回收不是任何一次 `Intent` 的后果，
    // 是系统级被动演化（见该方法文档「为什么不是 `Effect`/走 `apply`」
    // 一节），没有经引擎的路径可走。生产调用点是
    // `ll_game::world::cleanup_aged_ground_items`，它在
    // `ll_game::app::Demo::advance` 每帧真跑一遍。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let standing = world.size.wrap(3, 3);
    let lying = world.size.wrap(5, 3);
    let ingot_pos = world.size.wrap(7, 3);
    for (pos, def, placed) in [
        (standing, handle.forge, true),
        (lying, handle.forge, false),
        (ingot_pos, handle.iron_ingot, false),
    ] {
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(def, 1),
            dropped_at: world.clock,
            contents: Vec::new(),
            placed,
        });
    }
    world.advance(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS + 1);

    // Act
    let removed = world.cleanup_aged_ground_items(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS);

    // Assert：躺着的两堆都被清掉，只剩立着的那一座。
    assert_eq!(removed, 2);
    assert_eq!(
        world
            .ground_items
            .iter()
            .map(|ground| (ground.pos, ground.stack.def))
            .collect::<Vec<_>>(),
        vec![(standing, handle.forge)]
    );
}
