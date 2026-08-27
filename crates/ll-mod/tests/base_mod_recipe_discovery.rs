//! 端到端验证：项目所有者裁定的「**一开始什么都不会，只能乱煮**，然后
//! 通过各种途径发现了制作配方」在**本体内容**上真的成立。
//!
//! # 与 `example_mod_recipe_discovery.rs` 的分工
//!
//! 那份文件证明的是「配方发现这套机制成立，且第三方 mod 用得上」，
//! 内容全部来自 `mods/example_mod/`（那边只有一条配方声明了
//! `requires_discovery`，其余四条刻意保持「天生就会」当对照）。
//!
//! 本文件证明的是**另一件事**：`mods/lostland/crafting.json5` 的九条
//! 配方**一条不落**全部声明了 `requires_discovery`，因此一个刚出生的
//! 角色连烤肉都不会——这正是所有者要的开局，而它在此之前没有任何
//! 内容能表达（本体一条配方都没有）。
//!
//! # 为什么不去翻 schema 的默认值
//!
//! `RawRecipe::requires_discovery` 的 serde 默认值保持 `false`，理由逐条
//! 写在 `mods/lostland/crafting.json5` 的文件头。一句话版本：那是一条
//! **本体内容设计**裁定，不是一条数据格式规则；翻默认值会静默改掉每一个
//! 第三方 mod 的既有内容，还会当场弄坏 `mods/example_mod/` 那份依赖
//! 「默认 = 不需要发现」当对照的证据。
//!
//! # 手法
//!
//! 与 `example_mod_recipe_discovery.rs` 逐段同构：装载真实 `mods/` 整个
//! 目录，把装载出来的表借成 `ResolveCatalogs`，经 `TurnEngine::advance_ai`
//! 这条生产路径恰好结算一次意图。理由（为什么不只装 `mods/lostland/`、
//! 为什么经回合引擎而不是直接调 `resolve_*`）见那份文件的模块文档。

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
use ll_sim::item::{ItemCatalog, ItemStack};
use ll_sim::quest::NoQuests;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `base_mod_races.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

const NO_SKILLS: ll_sim::skill::NoSkills = ll_sim::skill::NoSkills;
const NO_RACE_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_CLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_SUBCLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_TRAITS: ll_sim::traits::NoTraits = ll_sim::traits::NoTraits;
const NO_POOLS: ll_sim::resource_pool::NoResourcePools = ll_sim::resource_pool::NoResourcePools;
const NO_FORMULAS: ll_sim::formula::NoFormulas = ll_sim::formula::NoFormulas;

/// 一次真实装载的产物——只留下本文件断言需要的那几张表与索引。
struct Handle {
    item: ItemTable,
    recipe: RecipeTable,
    recipe_category: RecipeCategoryTable,
    cooking: ContentIndex,
    advanced_forging: ContentIndex,
    roast_meat_recipe: ContentIndex,
    herb_roast_recipe: ContentIndex,
    iron_helm_recipe: ContentIndex,
    iron_rivet_batch: ContentIndex,
    raw_meat: ContentIndex,
    roast_meat: ContentIndex,
    herb_bundle: ContentIndex,
    iron_ingot: ContentIndex,
    iron_rivet: ContentIndex,
    leather_strip: ContentIndex,
    smith_hammer: ContentIndex,
    field_cookbook: ContentIndex,
    artisan: ContentIndex,
}

impl Handle {
    fn catalogs<'a>(
        &'a self,
        items: &'a dyn ItemCatalog,
        recipes: &'a dyn RecipeCatalog,
    ) -> ResolveCatalogs<'a> {
        ResolveCatalogs {
            skills: &NO_SKILLS,
            quests: &NoQuests,
            race_traits: &NO_RACE_TRAITS,
            class_traits: &NO_CLASS_TRAITS,
            subclass_traits: &NO_SUBCLASS_TRAITS,
            trait_defs: &NO_TRAITS,
            pools: &NO_POOLS,
            items,
            formulas: &NO_FORMULAS,
            damage_categories: &NoDamageCategories,
            recipes,
            ambient: AmbientSource::NONE,
            experience: &NoExperience,
            skill_tree: &NO_SKILLS,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
        }
    }

    fn real_recipes(&self) -> RegisteredRecipes<'_> {
        RegisteredRecipes {
            recipes: &self.recipe,
            categories: &self.recipe_category,
        }
    }
}

fn load_real_mods() -> Handle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
        ..
    } = session;
    let lostland_id = NamespacedId::parse("lostland:self").expect("合法标识符");
    let status = report
        .entries
        .iter()
        .find(|(id, _)| *id == lostland_id)
        .map(|(_, status)| status);
    assert_eq!(
        status,
        Some(&LoadStatus::Loaded),
        "本体内容 mod（mods/lostland/）必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).expect("合法标识符"))
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/lostland/ 的内容文件注册"))
    };

    Handle {
        cooking: resolve("lostland:cooking"),
        advanced_forging: resolve("lostland:advanced_forging"),
        roast_meat_recipe: resolve("lostland:roast_meat_recipe"),
        herb_roast_recipe: resolve("lostland:herb_roast_recipe"),
        iron_helm_recipe: resolve("lostland:iron_helm_recipe"),
        iron_rivet_batch: resolve("lostland:iron_rivet_batch"),
        raw_meat: resolve("lostland:raw_meat"),
        roast_meat: resolve("lostland:roast_meat"),
        herb_bundle: resolve("lostland:herb_bundle"),
        iron_ingot: resolve("lostland:iron_ingot"),
        iron_rivet: resolve("lostland:iron_rivet"),
        leather_strip: resolve("lostland:leather_strip"),
        smith_hammer: resolve("lostland:smith_hammer"),
        field_cookbook: resolve("lostland:field_cookbook"),
        artisan: resolve("lostland:artisan"),
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
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

/// 造一个占位实体，形状同 `example_mod_recipe_discovery.rs::spawn_agent`，
/// 多带一个 `subclasses` 参数（本文件要验类别闸门）。
fn spawn_agent(
    world: &mut WorldState,
    pos: (i32, i32),
    inventory: Vec<ItemStack>,
    known_recipes: Vec<ContentIndex>,
    subclasses: Vec<ContentIndex>,
) -> EntityId {
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
        inventory,
        equipment: BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes,
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses,
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
    })
}

/// 一次结算之后主角的完整快照。
struct Outcome {
    known_recipes: Vec<ContentIndex>,
    inventory: Vec<ItemStack>,
    /// `Tick(0)` 表示这次意图一点时间都没消耗（静默作废）。
    next_action_at: Tick,
}

/// 跑一场「主角经由 `TurnEngine` 提交恰好一次 `intent`」，手法同
/// `example_mod_recipe_discovery.rs::act_via_turn_engine`。
fn act_via_turn_engine(
    handle: &Handle,
    inventory: Vec<ItemStack>,
    known_recipes: Vec<ContentIndex>,
    subclasses: Vec<ContentIndex>,
    intent_of: impl Fn(EntityId) -> Intent,
) -> Outcome {
    let mut world = test_world();
    let hero = spawn_agent(&mut world, (5, 5), inventory, known_recipes, subclasses);
    let bystander = spawn_agent(&mut world, (9, 9), Vec::new(), Vec::new(), Vec::new());

    let mut timeline = Timeline::new();
    timeline.schedule(hero, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let recipes = handle.real_recipes();
    let catalogs = handle.catalogs(&handle.item, &recipes);
    // 主角当**受控实体**：`advance_ai` 一弹出它那一条就立刻返回（把它
    // 留在 `pending` 里），随后 `try_player_intent` 消费掉这一条。旁观者
    // 排在 `Tick(1)`，因此这一步一个人都不会被结算。
    //
    // # 为什么走玩家那条入口，不再走 `advance_ai` 那条非受控路径
    //
    // 本文件验的这几个意图（`Identify`/`Read`/`Experiment`/`Craft`）在
    // 真实游戏里**全部**由玩家从菜单提交，走的是
    // `ll_game::player_action::player_command` → `TurnEngine::try_player_intent`
    // 这一条，不是 AI 那条——此前用 `advance_ai` 只是「把一个意图推进
    // 引擎」的便利写法，并不是这些意图真实的产生地。
    //
    // 换过来还有一个必须换的理由：AI 那条路现在带着**进展保证**（结算
    // 为空时补一次「等待」，让非受控实体的时钟无论如何都往前走，见
    // `ll_sim::turn::TurnEngine::perform` 文档「进展保证」一节），于是
    // 「白做一次、时钟原地不动」在那条路上按设计不可能发生。本文件几条
    // 「静默作废不消耗回合」的断言问的正是这件事，它们属于玩家那条路，
    // 也只有在玩家那条路上才有意义。
    let mut no_ai =
        |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Wait { actor };
    engine.advance_ai(&mut world, hero, &mut no_ai, &catalogs, &mut |_, _| {});
    engine.try_player_intent(&mut world, hero, intent_of(hero), &catalogs, &mut |_, _| {});

    let after = world.actors.get(hero).expect("这些动作都不会杀死主角");
    Outcome {
        known_recipes: after.known_recipes.clone(),
        inventory: after.inventory.clone(),
        next_action_at: after.next_action_at,
    }
}

/// 背包里某种物品的总数（跨多堆求和）。
fn count_of(inventory: &[ItemStack], def: ContentIndex) -> u32 {
    inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

// ── 「一开始什么都不会」 ─────────────────────────────────────────────

#[test]
fn 刚出生的角色手握生肉也烤不出肉() {
    // 所有者裁定的开局：新角色的 known_recipes 是空的，而本体全部九条
    // 配方都声明了 requires_discovery，因此 resolve_craft 的第 4 道闸门
    // 直接把这次制作判死。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.raw_meat, 3)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Craft {
            actor,
            recipe: handle.roast_meat_recipe,
        },
    );

    // Assert：生肉一份没少，烤肉一份没多。
    assert_eq!(count_of(&outcome.inventory, handle.raw_meat), 3);
    assert_eq!(count_of(&outcome.inventory, handle.roast_meat), 0);
    assert!(outcome.known_recipes.is_empty());
}

// ── 路径一：乱煮 ────────────────────────────────────────────────────

#[test]
fn 乱煮真的能在本体烹饪类别里试出一条配方() {
    // 所有者原话「只能乱煮」的正向证据。手上只有生肉，因此候选里只有
    // 烤肉那一条食材齐全（香草烤肉还缺草药），掷骰选中的必然是它——
    // 这条断言因此不依赖任何具体的随机数。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.raw_meat, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
    );

    // Assert
    assert_eq!(outcome.known_recipes, vec![handle.roast_meat_recipe]);
}

#[test]
fn 乱煮既不消耗食材也不产出成品() {
    // `resolve_experiment` 文档里那四条论证的可执行版本：试做只花时间，
    // 不吃材料、不出东西。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.raw_meat, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
    );

    // Assert
    assert_eq!(count_of(&outcome.inventory, handle.raw_meat), 1);
    assert_eq!(count_of(&outcome.inventory, handle.roast_meat), 0);
    assert!(
        outcome.next_action_at > Tick(0),
        "试做成功要按一次普通行动计费"
    );
}

#[test]
fn 手上什么都没有时乱煮什么都试不出来也不消耗回合() {
    // 「每回合按一下试做」退化不成立的结构性理由：候选恒是「食材已经
    // 齐全的未知配方」。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(&handle, Vec::new(), Vec::new(), Vec::new(), |actor| {
        Intent::Experiment {
            actor,
            category: handle.cooking,
        }
    });

    // Assert
    assert!(outcome.known_recipes.is_empty());
    assert_eq!(outcome.next_action_at, Tick(0));
}

#[test]
fn 烹饪那两条都学会之后再乱煮什么都试不出来() {
    // 候选筛选的第二条谓词（已经知道的不必再试）在本体内容上的落点。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![
            ItemStack::new(handle.raw_meat, 2),
            ItemStack::new(handle.herb_bundle, 2),
        ],
        vec![handle.roast_meat_recipe, handle.herb_roast_recipe],
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
    );

    // Assert
    assert_eq!(
        outcome.known_recipes,
        vec![handle.roast_meat_recipe, handle.herb_roast_recipe]
    );
    assert_eq!(outcome.next_action_at, Tick(0));
}

#[test]
fn 没有工匠副职时在进阶锻造里乱煮什么都试不出来() {
    // `resolve_experiment` 第 ② 步的副职闸门——与 `resolve_craft` 第 ③
    // 步同一份判据。材料给足（三锭 + 四铆钉 + 一根皮革条正好凑得齐
    // iron_helm_recipe），唯一缺的就是副职。
    // Arrange
    let handle = load_real_mods();
    let full_materials = vec![
        ItemStack::new(handle.iron_ingot, 3),
        ItemStack::new(handle.iron_rivet, 4),
        ItemStack::new(handle.leather_strip, 1),
    ];

    // Act
    let without = act_via_turn_engine(
        &handle,
        full_materials.clone(),
        Vec::new(),
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.advanced_forging,
        },
    );
    let with = act_via_turn_engine(
        &handle,
        full_materials,
        Vec::new(),
        vec![handle.artisan],
        |actor| Intent::Experiment {
            actor,
            category: handle.advanced_forging,
        },
    );

    // Assert：同一份材料、同一个类别，差别只在有没有工匠副职。
    assert!(
        without.known_recipes.is_empty(),
        "没有工匠副职就进不了这个类别，谈不上在里面试"
    );
    assert_eq!(without.next_action_at, Tick(0));
    assert_eq!(with.known_recipes, vec![handle.iron_helm_recipe]);
}

// ── 路径二：读书 ────────────────────────────────────────────────────

#[test]
fn 读野外食谱真的学会烤肉配方且书还在() {
    // 第二条获取路径。它与乱煮互补：读书不查类别闸门，也不要求手上
    // 有料。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.field_cookbook, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Read {
            actor,
            def: handle.field_cookbook,
        },
    );

    // Assert
    assert_eq!(outcome.known_recipes, vec![handle.roast_meat_recipe]);
    assert_eq!(count_of(&outcome.inventory, handle.field_cookbook), 1);
}

// ── 学会之后 ────────────────────────────────────────────────────────

#[test]
fn 学会之后同一条烤肉配方真的做得出来() {
    // 闭环：上面那条「刚出生烤不出肉」的对照组，唯一的差别是
    // known_recipes 里多了这条配方。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.raw_meat, 3)],
        vec![handle.roast_meat_recipe],
        Vec::new(),
        |actor| Intent::Craft {
            actor,
            recipe: handle.roast_meat_recipe,
        },
    );

    // Assert
    assert_eq!(count_of(&outcome.inventory, handle.raw_meat), 2);
    assert_eq!(count_of(&outcome.inventory, handle.roast_meat), 1);
}

#[test]
fn 要工具的配方在没装备锤子时做不出来() {
    // required_tool 的判据是「**装备着**且耐久未归零」——把锤子放在
    // 背包里不算。选 iron_rivet_batch 当样本是因为它把变量隔离得最干净：
    // 不设场地前置、类别也不设副职闸门，因此唯一还可能拦住这次制作的
    // 就是工具那一条。
    // Arrange
    let handle = load_real_mods();
    let materials = vec![
        ItemStack::new(handle.iron_ingot, 2),
        ItemStack::new(handle.smith_hammer, 1),
    ];

    // Act：材料齐、配方也会，唯独锤子只躺在背包里没装备。
    let outcome = act_via_turn_engine(
        &handle,
        materials,
        vec![handle.iron_rivet_batch],
        Vec::new(),
        |actor| Intent::Craft {
            actor,
            recipe: handle.iron_rivet_batch,
        },
    );

    // Assert：铁锭一块没少，铆钉一颗没出。
    assert_eq!(count_of(&outcome.inventory, handle.iron_ingot), 2);
    assert_eq!(count_of(&outcome.inventory, handle.iron_rivet), 0);
}
