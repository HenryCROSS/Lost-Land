//! 端到端验证：真实装载仓库里的 `mods/` 目录，证明配方发现的两条路径
//! ——**读书**（`Intent::Read`）与**试做**（`Intent::Experiment`）——
//! 经由 [`ll_sim::turn::TurnEngine`]（本体二进制 `ll-game` 驱动世界的
//! 唯一路径）真的把配方写进了 `Agent::known_recipes`，且写进去之后
//! `Intent::Craft` 真的从「做不出来」变成「做得出来」。
//!
//! # 这一整条链路落地的是哪条裁定
//!
//! 项目所有者原话：「科研可以通过加点解锁，最开始设有初始可以通过阅读
//! 获取经验，也或者通过研究其他物品获取经验。**菜谱就是通过随机丢入
//! 东西煮获取或者阅读书籍的时候获取。**」——后半句就是本文件验收的东西。
//! 它**推翻了** `knowledge/design/food-and-cooking-system.md` 五节
//! 「菜谱全部已知、不设解锁门槛」，更正记录写在那份文档五节末尾。
//!
//! # 验收标准
//!
//! 与 `example_mod_crafting.rs` 同一条（那份文档完整记录了它的由来）：
//! 内容来自真实 `mods/`（ADR 0018），**且**结算必须经由 `TurnEngine`
//! 的公开入口发生。本文件因此全程只调
//! [`ll_sim::turn::TurnEngine::advance_ai`]，一次都不碰
//! `ll_sim::resolve` 的任何入口。
//!
//! # 反例守卫（三条，各守一处不同的接线）
//!
//! - [`没读过书也没试做过时同一条配方做不出来`]：守的是**闸门本身**。
//!   谁把 `resolve_craft` 的第 4 道闸门摘掉，这条立刻变红。
//! - [`目录从回合引擎摘掉后读书学不到任何配方`]：守的是**物品目录那条
//!   接线**。同一段场景、同一个 `TurnEngine`，只把物品目录换成
//!   [`ll_sim::item::NoItems`]，读书立刻什么都学不到。
//! - [`目录从回合引擎摘掉后试做学不到任何配方`]：守的是**配方目录那条
//!   接线**，手法同上，换成 [`ll_sim::craft::NoRecipes`]。
//!
//! # 本文件不验收什么
//!
//! **玩家怎么提交一次阅读/试做。** `Intent::Read`/`Intent::Experiment`
//! 目前没有任何键位产出者：`ll_sim::intent::intent_from_input` 至今只
//! 映射 `Move`/`Wait` 两种。这与 `Craft`/`PickUp`/`Drop`/`Equip`/
//! `Rest`/`Loot`/`Use` 等既有玩法意图的处境完全相同——输入映射层整体
//! 尚未展开，不是这两个新意图特有的缺口。本文件里那个「AI 策略直接
//! 返回意图」正是既有的最小占位提交路径。

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
use ll_sim::craft::{NoRecipes, RecipeCatalog};
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::item::{ItemCatalog, ItemStack, NoItems};
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

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_crafting.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 一次真实装载的产物——只留下本文件断言需要的那几张表与索引。
struct RealModsHandle {
    item: ItemTable,
    recipe: RecipeTable,
    recipe_category: RecipeCategoryTable,
    /// `mods/example_mod/crafting.json5` 里**唯一**声明了
    /// `recipe-requires-discovery!` 的那条配方。
    herb_stew_recipe: ContentIndex,
    /// 声明了 `register-item-teaches-recipe` 的那本书。
    cookbook: ContentIndex,
    raw_meat: ContentIndex,
    wild_herb: ContentIndex,
    herb_stew: ContentIndex,
    /// 不需要发现的对照配方——用来钉住「第 4 道闸门只对声明了
    /// `requires_discovery` 的配方生效」。
    roast_meat_recipe: ContentIndex,
    roast_meat: ContentIndex,
    /// 试做时要指定的类别。
    cooking: ContentIndex,
}

impl RealModsHandle {
    /// 把真实装载出来的表借成结算目录束——本体二进制交给 `TurnEngine`
    /// 的是同一个形状、同一批表。
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
            // 对话这两路（对话批次 2 新增）：本条测试与对话无关，
            // 接空实现即可。
            dialogues: &ll_sim::dialogue::NoDialogues,
            content_ids: &ll_sim::dialogue::NoContentIds,
        }
    }

    /// 本文件正向场景用的真实配方目录。
    fn real_recipes(&self) -> RegisteredRecipes<'_> {
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
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
        ..
    } = session;
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
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/crafting.json5 注册"))
    };

    RealModsHandle {
        herb_stew_recipe: resolve("examplemod:herb_stew_recipe"),
        cookbook: resolve("examplemod:cookbook"),
        raw_meat: resolve("examplemod:raw_meat"),
        wild_herb: resolve("examplemod:wild_herb"),
        herb_stew: resolve("examplemod:herb_stew"),
        roast_meat_recipe: resolve("examplemod:roast_meat_recipe"),
        roast_meat: resolve("examplemod:roast_meat"),
        cooking: resolve("examplemod:cooking"),
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

/// 造一个占位实体：本文件只关心背包与已知配方两项，其余全部取与
/// `example_mod_crafting.rs::spawn_agent` 相同的中性默认值。
fn spawn_agent(
    world: &mut WorldState,
    pos: (i32, i32),
    inventory: Vec<ItemStack>,
    known_recipes: Vec<ContentIndex>,
) -> EntityId {
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
        inventory,
        equipment: BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes,
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
    })
}

/// 一次结算之后主角的完整快照——本文件的三条断言维度。
struct Outcome {
    known_recipes: Vec<ContentIndex>,
    inventory: Vec<ItemStack>,
    /// 主角的下次行动时刻。`Tick(0)` 表示这次意图**一点时间都没消耗**
    /// （静默作废），供「读透了的书不再消耗回合」那条断言使用。
    next_action_at: Tick,
}

/// 跑一场「主角经由 `TurnEngine` 提交恰好一次 `intent`」，返回结算之后
/// 主角的快照。
///
/// # 为什么恰好只结算一次
///
/// 手法与 `example_mod_crafting.rs::craft_via_turn_engine` 完全相同：
/// 主角排在 `Tick(0)`、旁观者（`controlled`）排在 `Tick(1)`，
/// `advance_ai` 先弹出主角结算一次，下一次弹出的是 `controlled`，于是
/// 立即返回。
fn act_via_turn_engine(
    handle: &RealModsHandle,
    inventory: Vec<ItemStack>,
    known_recipes: Vec<ContentIndex>,
    intent_of: impl Fn(EntityId) -> Intent,
    items: &dyn ItemCatalog,
    recipes: &dyn RecipeCatalog,
) -> Outcome {
    let mut world = test_world();
    let hero = spawn_agent(&mut world, (5, 5), inventory, known_recipes);
    let bystander = spawn_agent(&mut world, (9, 9), Vec::new(), Vec::new());

    let mut timeline = Timeline::new();
    timeline.schedule(hero, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let catalogs = handle.catalogs(items, recipes);
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

// ── 路径一：读书 ────────────────────────────────────────────────────

#[test]
fn 读一本真实mod注册的菜谱书经回合引擎真的学会那条配方() {
    // ADR 0018 的正向证据：书与配方都来自真实
    // mods/example_mod/crafting.json5，结算经由 TurnEngine::advance_ai
    // 这条生产路径发生。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.cookbook, 1)],
        Vec::new(),
        |actor| Intent::Read {
            actor,
            def: handle.cookbook,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(outcome.known_recipes, vec![handle.herb_stew_recipe]);
}

#[test]
fn 书读完之后仍然在背包里没有被消耗() {
    // 这是 Intent::Read 与 Intent::Use 最本质的一条差别（见
    // ll_sim::intent::Intent::Read 文档那张逐步对照表）：resolve_read
    // 一条 ConsumeInventoryItem 都不产出。没有这一条断言，把读书实现成
    // 「复用 resolve_use_item 再加一个分支」的改动不会被任何测试拦下。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.cookbook, 1)],
        Vec::new(),
        |actor| Intent::Read {
            actor,
            def: handle.cookbook,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(count_of(&outcome.inventory, handle.cookbook), 1);
}

#[test]
fn 已经读透的书再读一次既不重复记录也不消耗回合() {
    // 守两件事：① known_recipes 不出现重复项（resolve_read 的产出侧
    // 去重）；② 一次不可能产生任何结果的行动不收费（时间轴不推进）。
    // 后者同时关掉一条真实的刷取路径，见 resolve_read 文档「读透了的
    // 书不再消耗回合」一节。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.cookbook, 1)],
        vec![handle.herb_stew_recipe],
        |actor| Intent::Read {
            actor,
            def: handle.cookbook,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(outcome.known_recipes, vec![handle.herb_stew_recipe]);
    assert_eq!(
        outcome.next_action_at,
        Tick(0),
        "读透了的书不该消耗任何时间"
    );
}

#[test]
fn 读一件不可读的普通物品什么都不会发生() {
    // taught_recipes 为空 = 这件东西不可读，见
    // ll_sim::item::ItemRule::taught_recipes 文档「为什么『可不可读』
    // 不是一个独立的布尔字段」一节。生肉不是书。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.raw_meat, 1)],
        Vec::new(),
        |actor| Intent::Read {
            actor,
            def: handle.raw_meat,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert!(outcome.known_recipes.is_empty());
    assert_eq!(count_of(&outcome.inventory, handle.raw_meat), 1);
}

#[test]
fn 目录从回合引擎摘掉后读书学不到任何配方() {
    // 反例守卫：同一段场景、同一个 TurnEngine，只把物品目录换成空的，
    // 读书立刻什么都学不到。没有这一条，上面那条正向测试无法排除
    // 「配方是别的什么东西塞进 known_recipes 的」。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.cookbook, 1)],
        Vec::new(),
        |actor| Intent::Read {
            actor,
            def: handle.cookbook,
        },
        &NoItems,
        &handle.real_recipes(),
    );

    // Assert
    assert!(outcome.known_recipes.is_empty());
}

// ── 路径二：试做 ────────────────────────────────────────────────────

#[test]
fn 手上材料齐全时试做经回合引擎真的发现那条配方() {
    // 项目所有者裁定「菜谱就是通过随机丢入东西煮获取」的正向证据。
    // Arrange：肉 + 香草恰好凑齐 herb_stew_recipe 的两味食材。
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![
            ItemStack::new(handle.raw_meat, 1),
            ItemStack::new(handle.wild_herb, 1),
        ],
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(outcome.known_recipes, vec![handle.herb_stew_recipe]);
}

#[test]
fn 试做既不消耗食材也不产出成品() {
    // resolve_experiment 文档「为什么失败与成功都不消耗食材」一节的
    // 直接落点，也是「发现和制作是两件事」这句话唯一的可执行判据：
    // 学会了配方，但锅里什么都没有，材料一样不少。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![
            ItemStack::new(handle.raw_meat, 2),
            ItemStack::new(handle.wild_herb, 2),
        ],
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(outcome.known_recipes, vec![handle.herb_stew_recipe]);
    assert_eq!(count_of(&outcome.inventory, handle.raw_meat), 2);
    assert_eq!(count_of(&outcome.inventory, handle.wild_herb), 2);
    assert_eq!(count_of(&outcome.inventory, handle.herb_stew), 0);
}

#[test]
fn 材料不齐时试做什么都发现不了也不消耗回合() {
    // 「刷不动」那条结构性保证的可执行版本：候选恒是「食材已经齐全的
    // 未知配方」，手上没有成套材料时按一万次也是空，见
    // resolve_experiment 文档「那会不会退化成每回合按一下试做」一节。
    // Arrange：只有肉，没有香草。
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.raw_meat, 5)],
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert!(outcome.known_recipes.is_empty());
    assert_eq!(outcome.next_action_at, Tick(0), "白试一次不该消耗时间");
}

#[test]
fn 已经知道的配方不会被试做重复发现() {
    // 第 3 步筛选里 !known_recipes.contains(...) 那一条谓词的守卫：
    // 全部候选都已知时，试做与「材料不齐」同一个结果——什么都不发生。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![
            ItemStack::new(handle.raw_meat, 1),
            ItemStack::new(handle.wild_herb, 1),
        ],
        vec![handle.herb_stew_recipe],
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(outcome.known_recipes, vec![handle.herb_stew_recipe]);
    assert_eq!(outcome.next_action_at, Tick(0));
}

#[test]
fn 试做发现不了不需要发现的配方() {
    // 第 3 步 requires_discovery 那一条谓词的守卫：烤肉配方人人天生会
    // 做（从未调用 recipe-requires-discovery!），因此它不可能出现在
    // 「发现」的候选里——否则 known_recipes 会攒下一堆毫无作用的记录。
    // Arrange：只给生肉，恰好凑齐烤肉配方、凑不齐炖菜配方。
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.raw_meat, 3)],
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert!(outcome.known_recipes.is_empty());
}

#[test]
fn 目录从回合引擎摘掉后试做学不到任何配方() {
    // 反例守卫，手法同读书那条：只把配方目录换成空的。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![
            ItemStack::new(handle.raw_meat, 1),
            ItemStack::new(handle.wild_herb, 1),
        ],
        Vec::new(),
        |actor| Intent::Experiment {
            actor,
            category: handle.cooking,
        },
        &handle.item,
        &NoRecipes,
    );

    // Assert
    assert!(outcome.known_recipes.is_empty());
}

// ── 闸门：发现之后才做得出来 ────────────────────────────────────────

#[test]
fn 没读过书也没试做过时同一条配方做不出来() {
    // 反例守卫，守的是 resolve_craft 的第 4 道闸门本身。食材齐全、
    // 类别不设副职闸门、没有场地/工具要求——唯一拦住这次制作的就是
    // 「你还没学会这张图纸」。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![
            ItemStack::new(handle.raw_meat, 1),
            ItemStack::new(handle.wild_herb, 1),
        ],
        Vec::new(),
        |actor| Intent::Craft {
            actor,
            recipe: handle.herb_stew_recipe,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert：食材一点没动，成品一份没有。
    assert_eq!(count_of(&outcome.inventory, handle.raw_meat), 1);
    assert_eq!(count_of(&outcome.inventory, handle.wild_herb), 1);
    assert_eq!(count_of(&outcome.inventory, handle.herb_stew), 0);
}

#[test]
fn 学会之后同一条配方经回合引擎真的做得出来() {
    // 与上一条构成红/绿对照：**唯一**的差别是 known_recipes 里多了这
    // 一条。两条合起来证明第 4 道闸门既真的拦得住、也真的放得开。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![
            ItemStack::new(handle.raw_meat, 1),
            ItemStack::new(handle.wild_herb, 1),
        ],
        vec![handle.herb_stew_recipe],
        |actor| Intent::Craft {
            actor,
            recipe: handle.herb_stew_recipe,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert：两味食材各扣一、成品进包。
    assert_eq!(count_of(&outcome.inventory, handle.raw_meat), 0);
    assert_eq!(count_of(&outcome.inventory, handle.wild_herb), 0);
    assert_eq!(count_of(&outcome.inventory, handle.herb_stew), 1);
}

#[test]
fn 不需要发现的配方不受第四道闸门影响() {
    // 守的是「本批次对既有内容零影响」这条兼容性承诺：烤肉配方从未
    // 调用 recipe-requires-discovery!，因此 known_recipes 为空时它仍然
    // 照做不误——第 4 道闸门只对显式声明过的配方生效。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.raw_meat, 3)],
        Vec::new(),
        |actor| Intent::Craft {
            actor,
            recipe: handle.roast_meat_recipe,
        },
        &handle.item,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(count_of(&outcome.inventory, handle.raw_meat), 2);
    assert_eq!(count_of(&outcome.inventory, handle.roast_meat), 1);
}
