//! 端到端验证：项目所有者裁定的「**加入未鉴定物品，通过鉴定获取属性和
//! 说明，同时就能获得经验**」「**收窄成通过未鉴定物品和书籍获取经验**」
//! 与「**加入盲盒这种物品，鉴定了可以获取经验，同时会随机获得一件物品
//! 或者武器装备**」三条裁定，在**本体内容**上真的成立。
//!
//! # 与 `base_mod_recipe_discovery.rs` 的分工
//!
//! 那份文件证明的是「配方发现（乱煮/读书）这条链路成立」；本文件证明
//! 的是另外三件事：
//!
//! 1. **鉴定**：`Intent::Identify` 真的把一个物品**种类**写进
//!    `Agent::identified_items`，并给一次经验；重复鉴定恒零收益。
//! 2. **读书给经验**：`resolve_read` 真的产出
//!    `Effect::GrantExperience`，且**只在真的教到新配方那一次**给。
//! 3. **盲盒**：开盒真的消耗盒子、按权重产出一件物品、给一次经验，
//!    且同种子下**逐位可复现**。
//!
//! # 反例在哪（「摘掉接线就变红」）
//!
//! 这份文件里有四条测试是刻意的反例，任何一处接线被摘掉都会让它们由
//! 绿转红，而不是悄悄放行：
//!
//! - `不需要鉴定的东西鉴定不了也不消耗时间`：摘掉 `resolve_identify`
//!   第 ④ 步那道 `requires_identification` 闸门，铁锭就会被「鉴定」出
//!   来，这条立刻红。
//! - `重复鉴定同一种东西零收益也不消耗时间`：摘掉第 ⑤a 步那道
//!   `identified_items.contains` 闸门，经验就会被刷出来，这条立刻红。
//! - `读透了的书再读一遍不给经验也不消耗时间`：摘掉 `resolve_read`
//!   第 5 步那道 `is_empty` 早退，同一本书就会变成经验机器，这条立刻红。
//! - `不在背包里的东西鉴定不了`：摘掉第 ② 步，隔空鉴定就成立了。
//!
//! # 手法
//!
//! 与 `base_mod_recipe_discovery.rs` 逐段同构：装载真实 `mods/` 整个
//! 目录，把装载出来的表借成 `ResolveCatalogs`，经 `TurnEngine::advance_ai`
//! 这条**生产路径**恰好结算一次意图——不直接调 `resolve_*`。

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
    roast_meat_recipe: ContentIndex,
    iron_ingot: ContentIndex,
    linen_cloth: ContentIndex,
    herb_bundle: ContentIndex,
    iron_shortsword: ContentIndex,
    amber_pendant: ContentIndex,
    unmarked_phial: ContentIndex,
    sealed_relic_box: ContentIndex,
    field_cookbook: ContentIndex,
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
            // 对话这两路（对话批次 2 新增）：本条测试与对话无关，接空实现。
            dialogues: &ll_sim::dialogue::NoDialogues,
            content_ids: &ll_sim::dialogue::NoContentIds,
            // 树木这一路（树木批次新增）：本条测试不砍树，接空实现。
            trees: &ll_sim::tree::NoTrees,
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
        roast_meat_recipe: resolve("lostland:roast_meat_recipe"),
        iron_ingot: resolve("lostland:iron_ingot"),
        linen_cloth: resolve("lostland:linen_cloth"),
        herb_bundle: resolve("lostland:herb_bundle"),
        iron_shortsword: resolve("lostland:iron_shortsword"),
        amber_pendant: resolve("lostland:amber_pendant"),
        unmarked_phial: resolve("lostland:unmarked_phial"),
        sealed_relic_box: resolve("lostland:sealed_relic_box"),
        field_cookbook: resolve("lostland:field_cookbook"),
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

/// 造一个占位实体，形状同 `base_mod_recipe_discovery.rs::spawn_agent`，
/// 但末位参数换成 `identified_items`（本文件要验鉴定，不验类别闸门）。
fn spawn_agent(
    world: &mut WorldState,
    pos: (i32, i32),
    inventory: Vec<ItemStack>,
    known_recipes: Vec<ContentIndex>,
    identified_items: Vec<ContentIndex>,
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
        identified_items,
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
        home: None,
    })
}

/// 一次结算之后主角的完整快照。
struct Outcome {
    known_recipes: Vec<ContentIndex>,
    /// 已经认得的物品**种类**（未鉴定物品批次）。
    identified_items: Vec<ContentIndex>,
    inventory: Vec<ItemStack>,
    /// 当前累计经验——`Effect::GrantExperience` 经 `apply` 落地的结果。
    experience: i64,
    /// `Tick(0)` 表示这次意图一点时间都没消耗（静默作废）。
    next_action_at: Tick,
}

/// 跑一场「主角经由 `TurnEngine` 提交恰好一次 `intent`」，手法同
/// `base_mod_recipe_discovery.rs::act_via_turn_engine`。
fn act_via_turn_engine(
    handle: &Handle,
    inventory: Vec<ItemStack>,
    known_recipes: Vec<ContentIndex>,
    identified_items: Vec<ContentIndex>,
    intent_of: impl Fn(EntityId) -> Intent,
) -> Outcome {
    let mut world = test_world();
    let hero = spawn_agent(
        &mut world,
        (5, 5),
        inventory,
        known_recipes,
        identified_items,
    );
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
        identified_items: after.identified_items.clone(),
        inventory: after.inventory.clone(),
        experience: after.experience,
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

// ── 鉴定：揭示（东西还在，只是你现在认得它了） ──────────────────────

#[test]
fn 鉴定一件未鉴定的饰品会认得这个种类并拿到经验() {
    // 所有者原话「通过鉴定获取属性和说明，同时就能获得经验」的正向
    // 证据。琥珀坠在 mods/lostland/items.json5 里声明了
    // requires_identification: true / study_experience: 30。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.amber_pendant, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Identify {
            actor,
            def: handle.amber_pendant,
        },
    );

    // Assert：认得了这个种类、拿到了 30 点经验、东西**还在背包里**
    // （鉴定是揭示不是转化），并且消耗了一次行动。
    assert_eq!(outcome.identified_items, vec![handle.amber_pendant]);
    assert_eq!(outcome.experience, 30);
    assert_eq!(count_of(&outcome.inventory, handle.amber_pendant), 1);
    assert!(outcome.next_action_at > Tick(0));
}

#[test]
fn 鉴定的粒度是种类而不是某一堆() {
    // `Agent::identified_items` 文档「粒度是种类，不是某一堆」的可执行
    // 版本：背包里有五瓶无标小瓶，鉴定一次之后写进去的是**一条**种类
    // 索引，而不是五条、也不是某个堆的句柄。此后捡到的同种小瓶一样
    // 认得（呈现层按 def 查这个列表，见 ll_ui::hud::item_display_name）。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.unmarked_phial, 5)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Identify {
            actor,
            def: handle.unmarked_phial,
        },
    );

    // Assert
    assert_eq!(outcome.identified_items, vec![handle.unmarked_phial]);
    assert_eq!(count_of(&outcome.inventory, handle.unmarked_phial), 5);
    assert_eq!(outcome.experience, 15);
}

#[test]
fn 重复鉴定同一种东西零收益也不消耗时间() {
    // **反例**：这条守的是整套研究经验设计最值钱的那条性质——
    // 「只有真的学到新东西才给经验」。摘掉 resolve_identify 第 ⑤a 步
    // 那道 identified_items.contains 闸门，经验就能被反复刷出来，这条
    // 立刻由绿转红。
    // Arrange
    let handle = load_real_mods();

    // Act：开局就已经认得琥珀坠。
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.amber_pendant, 1)],
        Vec::new(),
        vec![handle.amber_pendant],
        |actor| Intent::Identify {
            actor,
            def: handle.amber_pendant,
        },
    );

    // Assert：一条都没多、一点经验都没给、连时间都没推进。
    assert_eq!(outcome.identified_items, vec![handle.amber_pendant]);
    assert_eq!(outcome.experience, 0);
    assert_eq!(outcome.next_action_at, Tick(0));
}

#[test]
fn 不需要鉴定的东西鉴定不了也不消耗时间() {
    // **反例**：铁锭没有声明 requires_identification——没有人需要「鉴定」
    // 一块铁锭。摘掉 resolve_identify 第 ④ 步那道闸门，这条立刻红。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.iron_ingot, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Identify {
            actor,
            def: handle.iron_ingot,
        },
    );

    // Assert
    assert!(outcome.identified_items.is_empty());
    assert_eq!(outcome.experience, 0);
    assert_eq!(outcome.next_action_at, Tick(0));
}

#[test]
fn 不在背包里的东西鉴定不了() {
    // **反例**：摘掉 resolve_identify 第 ② 步，隔空鉴定就成立了。
    // Arrange
    let handle = load_real_mods();

    // Act：背包里只有铁锭，却想鉴定琥珀坠。
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.iron_ingot, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Identify {
            actor,
            def: handle.amber_pendant,
        },
    );

    // Assert
    assert!(outcome.identified_items.is_empty());
    assert_eq!(outcome.experience, 0);
    assert_eq!(outcome.next_action_at, Tick(0));
}

// ── 读书给经验（研究经验收窄的第二条来源） ──────────────────────────

#[test]
fn 读一本真的教到新配方的书会同时拿到配方与经验() {
    // 所有者「收窄成通过未鉴定物品和书籍获取经验」里书籍那一条的正向
    // 证据。野外食谱声明了 study_experience: 40。
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

    // Assert：配方学到了、经验拿到了、书**还在**（读书不消耗书）。
    assert_eq!(outcome.known_recipes, vec![handle.roast_meat_recipe]);
    assert_eq!(outcome.experience, 40);
    assert_eq!(count_of(&outcome.inventory, handle.field_cookbook), 1);
}

#[test]
fn 读透了的书再读一遍不给经验也不消耗时间() {
    // **反例**：这条守的是读书那一路的防刷。摘掉 resolve_read 第 5 步
    // 那道 is_empty 早退，同一本书就会变成一台经验机器，这条立刻红。
    // Arrange
    let handle = load_real_mods();

    // Act：开局就已经会烤肉。
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.field_cookbook, 1)],
        vec![handle.roast_meat_recipe],
        Vec::new(),
        |actor| Intent::Read {
            actor,
            def: handle.field_cookbook,
        },
    );

    // Assert
    assert_eq!(outcome.known_recipes, vec![handle.roast_meat_recipe]);
    assert_eq!(outcome.experience, 0);
    assert_eq!(outcome.next_action_at, Tick(0));
}

#[test]
fn 鉴定不会顺手学会配方读书也不会顺手认得种类() {
    // 两条路径写的是两个**不同的字段**（Effect::IdentifyItem →
    // identified_items，Effect::LearnRecipe → known_recipes），这条把
    // 「复用同一个效果变体」那种退化形态钉死。
    // Arrange
    let handle = load_real_mods();

    // Act
    let identified = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.amber_pendant, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Identify {
            actor,
            def: handle.amber_pendant,
        },
    );
    let read = act_via_turn_engine(
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
    assert!(identified.known_recipes.is_empty());
    assert!(read.identified_items.is_empty());
}

// ── 盲盒：转化（盒子没了，换成里面那件） ────────────────────────────

/// 封蜡遗物匣池子里的四档产出——与 mods/lostland/items.json5 的声明
/// 一一对应，供下面几条断言判定「开出来的是不是池子里的东西」。
fn box_prizes(handle: &Handle) -> [ContentIndex; 4] {
    [
        handle.iron_ingot,
        handle.linen_cloth,
        handle.herb_bundle,
        handle.iron_shortsword,
    ]
}

#[test]
fn 开一个盲盒会消耗盒子换来池子里的一件东西并拿到经验() {
    // 所有者原话「鉴定了可以获取经验，同时会随机获得一件物品或者武器
    // 装备」的正向证据，也是「盲盒是转化不是揭示」那张对照表的可执行
    // 版本。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.sealed_relic_box, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Identify {
            actor,
            def: handle.sealed_relic_box,
        },
    );

    // Assert：盒子没了（转化，不是揭示）、背包里多了池子里的某一件、
    // 拿到 20 点经验、消耗了一次行动。
    assert_eq!(count_of(&outcome.inventory, handle.sealed_relic_box), 0);
    assert_eq!(outcome.experience, 20);
    assert!(outcome.next_action_at > Tick(0));
    let gained: u32 = box_prizes(&handle)
        .iter()
        .map(|prize| count_of(&outcome.inventory, *prize))
        .sum();
    assert!(
        gained > 0,
        "开盒必须产出池子里的某一档，实际背包：{:?}",
        outcome.inventory
    );
}

#[test]
fn 开盒产出的东西带着它自己那条定义声明的满耐久() {
    // 「新造出来的物品带多少耐久」这条共同规则在**盲盒**这一路的端到端
    // 证据（`ll_world::item::ItemStack::freshly_made`）：此前这一行是
    // `ItemStack::new(...)`，开出来的铁短剑耐久恒为 None——永不磨损。
    //
    // 断言不写死开出哪一档（那是随机的，四档权重 50/30/15/5），改成
    // 对**背包里的每一堆**逐条核对「这堆的耐久 == 它那条定义声明的
    // 上限」——四档里三档是可堆叠材料（恒 None）、一档是铁短剑
    // （Some(120)），无论抽中哪一档这条断言都成立，且都是真断言。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.sealed_relic_box, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Identify {
            actor,
            def: handle.sealed_relic_box,
        },
    );

    // Assert
    assert!(!outcome.inventory.is_empty(), "开盒必然产出一堆东西");
    for stack in &outcome.inventory {
        let expected = ItemCatalog::item(&handle.item, stack.def)
            .expect("产出物必然是已注册的物品")
            .max_durability;
        assert_eq!(
            stack.durability, expected,
            "产出物 {:?} 的耐久应当等于它的耐久上限",
            stack.def
        );
    }
    // 并且这条断言在铁短剑那一档上确实是非平凡的：本体铁短剑声明了
    // 耐久上限，若开出的是它，上面那条比的就是 Some(120) 而不是 None。
    assert_eq!(
        ItemCatalog::item(&handle.item, handle.iron_shortsword)
            .expect("铁短剑已注册")
            .max_durability,
        Some(120)
    );
}

#[test]
fn 开盒不把盒子写进已鉴定种类() {
    // 盒子被消耗了，「认识一种已经不在世上的东西」没有意义——见
    // Effect::IdentifyItem 文档「盲盒不走这条效果」一节。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = act_via_turn_engine(
        &handle,
        vec![ItemStack::new(handle.sealed_relic_box, 1)],
        Vec::new(),
        Vec::new(),
        |actor| Intent::Identify {
            actor,
            def: handle.sealed_relic_box,
        },
    );

    // Assert
    assert!(outcome.identified_items.is_empty());
}

#[test]
fn 同种子下开同一个盲盒两次产出逐位相同() {
    // 约束 C3 的验收：随机走 DetRng::for_entity(种子, 实体, 时刻 ^ 标签)
    // 这条三元组流，因此同一局面重放必然给出同一个结果——这正是整套
    // 重放能力的基础。
    // Arrange
    let handle = load_real_mods();
    let run = || {
        act_via_turn_engine(
            &handle,
            vec![ItemStack::new(handle.sealed_relic_box, 1)],
            Vec::new(),
            Vec::new(),
            |actor| Intent::Identify {
                actor,
                def: handle.sealed_relic_box,
            },
        )
    };

    // Act
    let first = run();
    let second = run();

    // Assert：逐档数量完全一致（不是「都开出了东西」这种弱断言）。
    for prize in box_prizes(&handle) {
        assert_eq!(
            count_of(&first.inventory, prize),
            count_of(&second.inventory, prize),
            "同种子两次开盒的产出必须逐位相同"
        );
    }
    assert_eq!(first.experience, second.experience);
    assert_eq!(first.next_action_at, second.next_action_at);
}
