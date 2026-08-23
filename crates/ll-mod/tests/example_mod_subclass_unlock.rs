//! 端到端验证：真实装载仓库里的 `mods/` 目录，证明「反复制作 → 达标
//! 获得副职 → 副职把守的配方类别随之解锁 → 放弃副职后立刻重新锁上」
//! 这一整条链路**经由 [`ll_sim::turn::TurnEngine`]**（本体二进制
//! `ll-game` 驱动世界的唯一路径）真的发生。
//!
//! # 这条链路补的是什么缺口
//!
//! `ll_world::entity::Agent::subclasses` 在本批次之前**没有任何写入
//! 路径**（`grep GrantSubclass` 全仓库零命中，唯一的命中全是测试夹具
//! 里的 `Vec::new()`）。而 `08cdeb0` 落地的
//! `RecipeCategoryDef::required_subclasses` 闸门每次制作都要读它——
//! 也就是说那道闸门在结构上等价于「凡是声明了副职要求的配方类别，谁
//! 都做不了」。本文件是那个缺口被补上之后的守卫。
//!
//! # 内容来源（ADR 0018）
//!
//! 全部来自真实 `mods/example_mod/gameplay.scm`，一行都不是测试现造：
//!
//! - `(register-subclass "examplemod:shadowdancer" …)`
//! - `(register-recipe-category "examplemod:cooking" …)`——**不设闸门**
//! - `(register-subclass-unlock "examplemod:shadowdancer" "items-crafted"
//!    "examplemod:cooking" 3)`
//! - `(register-recipe-category "examplemod:forging" …)` +
//!   `(recipe-category-requires-subclass! "examplemod:forging"
//!    "examplemod:shadowdancer")`
//!
//! 「从一个不设闸门的类别里练出副职，用它去开另一个设了闸门的类别的
//! 门」正是这套机制唯一不死锁的用法，见那个脚本文件里的说明。
//!
//! # 反例守卫
//!
//! [`获得条件目录从回合引擎摘掉后烤再多次肉也拿不到副职`] 是那份守
//! 卫：同一段场景、同一个 `TurnEngine`，只把
//! [`ll_sim::catalogs::ResolveCatalogs::subclass_unlocks`] 换成
//! [`ll_sim::subclass::NoSubclassUnlocks`]，副职立刻拿不到、连一条
//! 计数都不写。谁把这一路从 `TurnEngine` 那条链路上摘掉（比如把
//! `resolve_dispatch` 里那行 `append_craft_progress` 删掉，或者把
//! `ll_game::content::RuntimeCatalogs::as_resolve_catalogs` 里
//! `subclass_unlocks` 那一行改回不接），正向测试就会拿到与本条完全
//! 一样的结果而变红。
//!
//! # 本文件不验收什么
//!
//! **玩家怎么提交这些意图。** `ll_sim::intent::intent_from_input` 至今
//! 只映射 `Move`/`Wait` 两个意图——`Craft` 与本批次新增的
//! `AbandonSubclass` 同样没有绑定按键，输入映射层整体尚未展开。本文件
//! 里「AI 策略直接返回那条意图」是最小占位提交路径，不假装制作与放弃
//! 副职在真实玩法里已经可以用键盘够到。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::formula::FormulaTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::recipe::{RecipeTable, RegisteredRecipes};
use ll_mod::recipe_category::RecipeCategoryTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::tag::TagTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::item::ItemStack;
use ll_sim::quest::NoQuests;
use ll_sim::subclass::{MAX_SUBCLASSES, NoSubclassUnlocks, SubclassUnlockCatalog, craft_count_key};
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::script_state::ScriptValue;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `turn_engine_catalogs.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// `mods/example_mod/gameplay.scm` 里那条 `register-subclass-unlock`
/// 声明的阈值。写成常量并在 [`阈值与真实脚本声明一致`] 里对着真实
/// 装载出来的表核对一次——本文件其余每条断言都以它为准，脚本改了数
/// 而测试没跟上时应该是那一条先红，不是别的条目莫名其妙地红。
const COOKING_THRESHOLD: u32 = 3;

// 本文件只关心制作与副职那两路，其余目录一律接空实现。
const NO_SKILLS: ll_sim::skill::NoSkills = ll_sim::skill::NoSkills;
const NO_RACE_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_CLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_TRAITS: ll_sim::traits::NoTraits = ll_sim::traits::NoTraits;
const NO_POOLS: ll_sim::resource_pool::NoResourcePools = ll_sim::resource_pool::NoResourcePools;
const NO_FORMULAS: ll_sim::formula::NoFormulas = ll_sim::formula::NoFormulas;

/// 一次真实装载的产物——只留下本文件断言需要的那几张表与索引。
struct RealModsHandle {
    item: ItemTable,
    recipe: RecipeTable,
    recipe_category: RecipeCategoryTable,
    subclass: SubclassTable,
    roast_meat_recipe: ContentIndex,
    iron_sword_recipe: ContentIndex,
    raw_meat: ContentIndex,
    roast_meat: ContentIndex,
    iron_ingot: ContentIndex,
    iron_sword: ContentIndex,
    war_hammer: ContentIndex,
    lava_floor: ContentIndex,
    shadowdancer: ContentIndex,
    cooking: ContentIndex,
}

impl RealModsHandle {
    /// 把真实装载出来的表借成结算目录束——本体二进制
    /// （`ll_game::content::RuntimeCatalogs::as_resolve_catalogs`）交给
    /// `TurnEngine` 的是同一个形状、同一批表。
    ///
    /// `unlocks` 由调用方传入，正是为了让反例守卫能在**其余一切都不
    /// 变**的前提下只把这一路换成空实现。
    fn catalogs<'a>(
        &'a self,
        recipes: &'a RegisteredRecipes<'a>,
        unlocks: &'a dyn SubclassUnlockCatalog,
    ) -> ResolveCatalogs<'a> {
        ResolveCatalogs {
            skills: &NO_SKILLS,
            quests: &NoQuests,
            race_traits: &NO_RACE_TRAITS,
            class_traits: &NO_CLASS_TRAITS,
            trait_defs: &NO_TRAITS,
            pools: &NO_POOLS,
            items: &self.item,
            formulas: &NO_FORMULAS,
            damage_categories: &NoDamageCategories,
            recipes,
            ambient: AmbientSource::NONE,
            experience: &NoExperience,
            skill_tree: &NO_SKILLS,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: unlocks,
        }
    }

    fn real_recipes(&self) -> RegisteredRecipes<'_> {
        RegisteredRecipes {
            recipes: &self.recipe,
            categories: &self.recipe_category,
        }
    }
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
    let mut formula = FormulaTable::new();
    let mut weapon_category = WeaponCategoryTable::new();
    let mut damage_category = DamageCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = RecipeTable::new();
    let mut recipe_category_table = RecipeCategoryTable::new();
    let mut tag_table = TagTable::new();

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
    // 本体与示例 mod 都必须装上：副职获得条件来自 example_mod，本体
    // 那四个制作副职来自 lostland，两边任何一边没装上，下面的索引
    // 解析都毫无意义。
    for id in ["examplemod:self", "lostland:self"] {
        let parsed = NamespacedId::parse(id).unwrap();
        let status = report
            .entries
            .iter()
            .find(|(entry, _)| *entry == parsed)
            .map(|(_, status)| status);
        assert_eq!(status, Some(&LoadStatus::Loaded), "{id} 必须成功加载");
    }

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被真实 mods/ 注册"))
    };

    RealModsHandle {
        roast_meat_recipe: resolve("examplemod:roast_meat_recipe"),
        iron_sword_recipe: resolve("examplemod:iron_sword_recipe"),
        raw_meat: resolve("examplemod:raw_meat"),
        roast_meat: resolve("examplemod:roast_meat"),
        iron_ingot: resolve("examplemod:iron_ingot"),
        iron_sword: resolve("examplemod:iron_sword"),
        war_hammer: resolve("examplemod:war_hammer"),
        lava_floor: resolve("examplemod:lava_floor"),
        shadowdancer: resolve("examplemod:shadowdancer"),
        cooking: resolve("examplemod:cooking"),
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
        subclass,
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

/// 造一个占位实体，形状与 `example_mod_crafting.rs::spawn_agent` 同源。
fn spawn_agent(
    world: &mut WorldState,
    pos: (i32, i32),
    inventory: Vec<ItemStack>,
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
        skill_cooldowns: BTreeMap::new(),
        subclasses,
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

/// 一场完整的「烤 N 次肉」跑完之后，制作者的那份 `Agent` 快照。
///
/// # 为什么恰好结算 N 次
///
/// 手法与 `example_mod_crafting.rs::craft_via_turn_engine` 同源，只是
/// 意图闭包改成「前 N 次提交 `Intent::Craft`，之后一律 `Wait`」：制作
/// 会产出 `Effect::ScheduleNext`（一次普通行动的开销），制作者因此排
/// 在旁观者之前反复轮到自己，直到闭包开始返回 `Wait` 为止。
fn cook_n_times_via_turn_engine(
    handle: &RealModsHandle,
    unlocks: &dyn SubclassUnlockCatalog,
    times: u32,
    start_subclasses: Vec<ContentIndex>,
) -> Agent {
    let mut world = test_world();
    // 生肉给足：每次制作扣一块，多给几块保证「没做成」只可能是闸门
    // 或计数的原因，不可能是食材不够。
    let crafter = spawn_agent(
        &mut world,
        (5, 5),
        vec![ItemStack::new(handle.raw_meat, times + 5)],
        start_subclasses,
    );
    let bystander = spawn_agent(&mut world, (9, 9), Vec::new(), Vec::new());

    let mut timeline = Timeline::new();
    timeline.schedule(crafter, Tick(0));
    // 旁观者排得足够晚，保证制作者能连着轮到自己 `times` 次。
    timeline.schedule(bystander, Tick(1_000_000));
    let mut engine = TurnEngine::new(timeline);

    let recipes = handle.real_recipes();
    let catalogs = handle.catalogs(&recipes, unlocks);
    let mut remaining = times;
    let mut intent = |_world: &WorldState, actor: EntityId, _controlled: EntityId| {
        if actor != crafter || remaining == 0 {
            return Intent::Wait { actor };
        }
        remaining -= 1;
        Intent::Craft {
            actor,
            recipe: handle.roast_meat_recipe,
        }
    };
    engine.advance_ai(
        &mut world,
        bystander,
        &mut intent,
        &catalogs,
        &mut |_, _| {},
    );

    world
        .actors
        .get(crafter)
        .expect("烤肉不会杀死制作者")
        .clone()
}

/// 跑一场「先烤够 N 次拿到副职，再提交一次 `Intent::AbandonSubclass`，
/// 最后提交一次锻造」，返回最后那一刻制作者的 `Agent`。
///
/// 三个阶段全程经同一个 [`TurnEngine`]，中途一次都不碰
/// `ll_sim::resolve`/`ll_sim::apply`。
fn cook_then_abandon_then_forge(handle: &RealModsHandle, abandon: bool) -> Agent {
    let mut world = test_world();
    let crafter = spawn_agent(
        &mut world,
        (5, 5),
        vec![
            ItemStack::new(handle.raw_meat, COOKING_THRESHOLD + 2),
            ItemStack::new(handle.iron_ingot, 4),
            ItemStack::new(handle.war_hammer, 1),
        ],
        Vec::new(),
    );
    // 锻造那条配方要求站在熔岩地板上并装备战锤，见
    // mods/example_mod/gameplay.scm 配方②。
    let pos = world.actors.get(crafter).expect("刚生成").pos;
    assert!(world.terrain_at(pos).is_some(), "脚下这一格必须已常驻");
    world.terrain.set_terrain(
        pos,
        ll_world::terrain::TerrainKind::from_index(handle.lava_floor),
    );
    {
        let agent = world.actors.get_mut(crafter).expect("刚生成");
        agent
            .inventory
            .retain(|stack| stack.def != handle.war_hammer);
        agent.equipment.insert(
            ll_sim::item::EquipSlot::MAIN_HAND,
            ItemStack::new(handle.war_hammer, 1),
        );
    }
    let bystander = spawn_agent(&mut world, (9, 9), Vec::new(), Vec::new());

    let mut timeline = Timeline::new();
    timeline.schedule(crafter, Tick(0));
    timeline.schedule(bystander, Tick(1_000_000));
    let mut engine = TurnEngine::new(timeline);

    let recipes = handle.real_recipes();
    let catalogs = handle.catalogs(&recipes, &handle.subclass);
    // 阶段机：烤够阈值 → （可选）放弃副职 → 锻造一次 → 之后一律等待。
    let mut cooked = 0_u32;
    let mut abandoned = !abandon;
    let mut forged = false;
    let mut intent = |_world: &WorldState, actor: EntityId, _controlled: EntityId| {
        if actor != crafter {
            return Intent::Wait { actor };
        }
        if cooked < COOKING_THRESHOLD {
            cooked += 1;
            return Intent::Craft {
                actor,
                recipe: handle.roast_meat_recipe,
            };
        }
        if !abandoned {
            abandoned = true;
            return Intent::AbandonSubclass {
                actor,
                subclass: handle.shadowdancer,
            };
        }
        if !forged {
            forged = true;
            return Intent::Craft {
                actor,
                recipe: handle.iron_sword_recipe,
            };
        }
        Intent::Wait { actor }
    };
    engine.advance_ai(
        &mut world,
        bystander,
        &mut intent,
        &catalogs,
        &mut |_, _| {},
    );

    world.actors.get(crafter).expect("制作不会杀死人").clone()
}

/// 背包里某种物品的总数（跨多堆求和）。
fn count_of(inventory: &[ItemStack], def: ContentIndex) -> u32 {
    inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

/// 读出制作者当前在某个类别上的累计制作次数。
fn count_in(agent: &Agent, category: &str) -> i64 {
    let id = NamespacedId::parse(category).expect("测试用标识符恒合法");
    match agent
        .script_state
        .get(&("lostland".to_string(), craft_count_key(&id)))
    {
        Some(ScriptValue::Int(n)) => *n,
        _ => 0,
    }
}

#[test]
fn 阈值与真实脚本声明一致() {
    // 本文件其余断言全部以 COOKING_THRESHOLD 为准。脚本改了数而这里
    // 没跟上时，应该是这一条先红。
    // Arrange
    let handle = load_real_mods();

    // Act
    let unlock = handle
        .subclass
        .craft_unlock(handle.shadowdancer)
        .expect("gameplay.scm 里那条 register-subclass-unlock 必须已注册");

    // Assert
    assert_eq!(unlock.threshold, COOKING_THRESHOLD);
    assert_eq!(unlock.category, handle.cooking);
    assert_eq!(unlock.category_id.to_string(), "examplemod:cooking");
}

#[test]
fn 烤肉未达阈值时只累加计数不授予副职() {
    // Arrange
    let handle = load_real_mods();

    // Act：只烤 阈值-1 次。
    let after =
        cook_n_times_via_turn_engine(&handle, &handle.subclass, COOKING_THRESHOLD - 1, Vec::new());

    // Assert：肉真的烤了（证明制作确实发生），计数到位，副职还没到手。
    assert_eq!(
        count_of(&after.inventory, handle.roast_meat),
        COOKING_THRESHOLD - 1
    );
    assert_eq!(
        count_in(&after, "examplemod:cooking"),
        i64::from(COOKING_THRESHOLD - 1)
    );
    assert!(after.subclasses.is_empty(), "还差一次，不该拿到副职");
}

#[test]
fn 烤肉达到阈值时经回合引擎真的授予副职() {
    // ADR 0018 的正向证据：内容来自真实 mods/example_mod/gameplay.scm
    // 的 register-subclass-unlock，授予经由 TurnEngine::advance_ai
    // 这条生产路径发生。这是 Agent::subclasses 在本仓库里第一次被真
    // 正写入。
    // Arrange
    let handle = load_real_mods();

    // Act
    let after =
        cook_n_times_via_turn_engine(&handle, &handle.subclass, COOKING_THRESHOLD, Vec::new());

    // Assert
    assert_eq!(after.subclasses, vec![handle.shadowdancer]);
    assert_eq!(
        count_in(&after, "examplemod:cooking"),
        i64::from(COOKING_THRESHOLD)
    );
}

#[test]
fn 获得条件目录从回合引擎摘掉后烤再多次肉也拿不到副职() {
    // 反例守卫：同一段场景、同一个 TurnEngine，只把获得条件目录换成
    // 空实现——副职立刻拿不到，而且**一条计数都不写**。上一条与本条
    // 的差值就是「真实 mod 内容确实被读到了」这句话的全部证据。
    // Arrange
    let handle = load_real_mods();

    // Act：烤得比阈值还多。
    let after = cook_n_times_via_turn_engine(
        &handle,
        &NoSubclassUnlocks,
        COOKING_THRESHOLD + 2,
        Vec::new(),
    );

    // Assert：肉照样烤了（制作本身与副职无关），但副职与计数都没有。
    assert_eq!(
        count_of(&after.inventory, handle.roast_meat),
        COOKING_THRESHOLD + 2
    );
    assert!(after.subclasses.is_empty());
    assert_eq!(count_in(&after, "examplemod:cooking"), 0);
}

#[test]
fn 重复达标不会把同一个副职授予第二次() {
    // 去重那道闸：判据是「累计 >= 阈值」，因此每一次后续制作都会再次
    // 命中这条规则。没有去重的话 Agent::subclasses 会越堆越长。
    // Arrange
    let handle = load_real_mods();

    // Act：烤到阈值的两倍。
    let after =
        cook_n_times_via_turn_engine(&handle, &handle.subclass, COOKING_THRESHOLD * 2, Vec::new());

    // Assert
    assert_eq!(after.subclasses, vec![handle.shadowdancer]);
}

#[test]
fn 副职满员时达标被拒绝但计数照常累加() {
    // 「被拒绝时不要吞掉任何东西」——上限拒绝的是**授予**，不是计数。
    // 玩家在满员状态下继续制作，进度不会白做。
    // Arrange：先塞满 MAX_SUBCLASSES 个**别的**副职。用的是
    // mods/lostland/subclasses.scm 真实注册的那四个制作类副职里的前
    // 三个——靠 SubclassTable 的获得条件列反查索引，顺带证明本体那四
    // 条真的注册上了。
    let handle = load_real_mods();
    let held: Vec<ContentIndex> = handle
        .subclass
        .craft_unlocks()
        .into_iter()
        .filter(|rule| rule.category_id.namespace() == "lostland")
        .map(|rule| rule.subclass)
        .take(MAX_SUBCLASSES)
        .collect();
    assert_eq!(
        held.len(),
        MAX_SUBCLASSES,
        "mods/lostland/subclasses.scm 至少要注册 {MAX_SUBCLASSES} 个带获得条件的副职"
    );

    // Act：满员状态下烤够阈值。
    let after =
        cook_n_times_via_turn_engine(&handle, &handle.subclass, COOKING_THRESHOLD, held.clone());

    // Assert：副职一个没多，计数一次没少。
    assert_eq!(after.subclasses, held);
    assert_eq!(
        count_in(&after, "examplemod:cooking"),
        i64::from(COOKING_THRESHOLD)
    );
}

#[test]
fn 拿到副职之后设了闸门的锻造类别真的解锁了() {
    // 这一条把两套机制接在一起：使用计数授予的副职，正是
    // recipe-category-requires-subclass! 那道闸门认的那一个。
    // Arrange & Act：烤够 → 不放弃 → 锻造一次。
    let handle = load_real_mods();
    let after = cook_then_abandon_then_forge(&handle, false);

    // Assert
    assert_eq!(after.subclasses, vec![handle.shadowdancer]);
    assert_eq!(
        count_of(&after.inventory, handle.iron_sword),
        1,
        "持有暗影舞者时锻造类别应当放行"
    );
}

#[test]
fn 放弃副职之后同一条锻造配方立刻做不了了() {
    // **本批次最重要的一条语义**：`resolve_craft` 的副职闸门是每次制作
    // 都判一遍的，所以放弃副职**不追溯**已学会的技能，却会让被把守的
    // 配方类别**立刻**关门。这与技能那一路的「学会了就永远能用」相反
    // ——两种闸门的语义本来就不同，不是缺陷。
    //
    // 与上一条的唯一差别是中间多提交了一次 Intent::AbandonSubclass。
    // Arrange & Act
    let handle = load_real_mods();
    let after = cook_then_abandon_then_forge(&handle, true);

    // Assert：副职没了，剑没打出来，铁锭一块没少（静默失败不消耗食材）。
    assert!(after.subclasses.is_empty(), "放弃之后不该还持有这个副职");
    assert_eq!(
        count_of(&after.inventory, handle.iron_sword),
        0,
        "放弃副职之后锻造类别应当立刻重新锁上"
    );
    assert_eq!(
        count_of(&after.inventory, handle.iron_ingot),
        4,
        "制作静默失败时一条食材都不该被消耗"
    );
}

#[test]
fn 放弃一个从未持有的副职不产生任何变化() {
    // resolve_abandon_subclass 的第二道闸门。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(&mut world, (5, 5), Vec::new(), Vec::new());
    let bystander = spawn_agent(&mut world, (9, 9), Vec::new(), Vec::new());
    let mut timeline = Timeline::new();
    timeline.schedule(actor, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);
    let recipes = handle.real_recipes();
    let catalogs = handle.catalogs(&recipes, &handle.subclass);

    // Act：只提交**一次**，之后一律等待——放弃副职刻意不产出
    // `Effect::ScheduleNext`（自由动作，不花回合），行动者会立刻又轮
    // 到自己，`Wait` 负责把这一场收束到「恰好提交了一次」。
    let mut submitted = false;
    let mut intent = |_world: &WorldState, acting: EntityId, _controlled: EntityId| {
        if submitted {
            return Intent::Wait { actor: acting };
        }
        submitted = true;
        Intent::AbandonSubclass {
            actor: acting,
            subclass: handle.shadowdancer,
        }
    };
    engine.advance_ai(
        &mut world,
        bystander,
        &mut intent,
        &catalogs,
        &mut |_, _| {},
    );

    // Assert
    let after = world.actors.get(actor).expect("放弃副职不会杀死任何人");
    assert!(after.subclasses.is_empty());
}
