//! `Intent::Experiment`（试做发现配方）的集成测试（配方发现批次）——
//! 走真实的 [`ll_sim::resolve::resolve_with_catalogs`] 管线，不直接构造
//! [`ll_sim::effect::Effect`] 抄近路。
//!
//! # 这里测什么，`ll-mod` 那份端到端测试不测什么
//!
//! `crates/ll-mod/tests/example_mod_recipe_discovery.rs` 用真实 `mods/`
//! 内容经 `TurnEngine` 验收整条链路（ADR 0018），但真实内容里**只有
//! 一条**需要发现的配方，因此那份测试碰不到「多个候选之间怎么选」这
//! 一半。本文件用假目录造出**两条**同时满足条件的候选，专门验收：
//!
//! 1. 约束 C3：同一个 `(世界种子, 实体, 时刻)` 恒选同一条（确定性）；
//! 2. 不同种子确实会选出不同的那一条（掷骰真的在起作用，不是恒取第
//!    一条然后被上一条测试误判成「确定」）；
//! 3. 副职闸门对试做同样生效。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::craft::{RecipeCatalog, RecipeIngredient, RecipeRule};
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::item::ItemStack;
use ll_sim::resolve::resolve_with_catalogs;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 本文件用到的全部内容索引——一次 intern 出来，供夹具与断言共享。
struct Ids {
    cooking: ContentIndex,
    stew: ContentIndex,
    pie: ContentIndex,
    meat: ContentIndex,
    herb: ContentIndex,
    product: ContentIndex,
    chef_subclass: ContentIndex,
}

fn ids() -> Ids {
    let mut interner = Interner::new();
    let mut make = |raw: &str| interner.intern(NamespacedId::parse(raw).expect("合法标识符"));
    Ids {
        cooking: make("lostland:cooking"),
        stew: make("lostland:stew_recipe"),
        pie: make("lostland:pie_recipe"),
        meat: make("lostland:meat"),
        herb: make("lostland:herb"),
        product: make("lostland:dish"),
        chef_subclass: make("lostland:chef"),
    }
}

/// 一个假配方目录：烹饪类别下两条**都需要发现、食材都只要一样手上有
/// 的东西**的配方，因此两条恒同时是候选。
struct FakeRecipes {
    ids: Ids,
    /// 非空时给烹饪类别设一道副职闸门。
    gate: Vec<ContentIndex>,
}

impl RecipeCatalog for FakeRecipes {
    fn recipe(&self, recipe: ContentIndex) -> Option<RecipeRule> {
        let ingredient = if recipe == self.ids.stew {
            self.ids.meat
        } else if recipe == self.ids.pie {
            self.ids.herb
        } else {
            return None;
        };
        Some(RecipeRule {
            category: self.ids.cooking,
            ingredients: vec![RecipeIngredient {
                item: ingredient,
                count: 1,
            }],
            product: self.ids.product,
            product_count: 1,
            required_station: None,
            required_tool: None,
            requires_discovery: true,
        })
    }

    fn category_required_subclasses(&self, category: ContentIndex) -> Vec<ContentIndex> {
        if category == self.ids.cooking {
            self.gate.clone()
        } else {
            Vec::new()
        }
    }

    fn recipes_in_category(&self, category: ContentIndex) -> Vec<ContentIndex> {
        if category == self.ids.cooking {
            // 按索引升序，与 `RecipeTable::in_category` 的真实契约一致
            // （约束 C5）。
            let mut both = vec![self.ids.stew, self.ids.pie];
            both.sort_by_key(ContentIndex::get);
            both
        } else {
            Vec::new()
        }
    }
}

fn test_world(seed: u64) -> WorldState {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let spawn = layout.tile_size().wrap(0, 0);
    let params = GenParams {
        seed,
        ..GenParams::default()
    };
    WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn)
        .expect("测试布局满足全部构造前置条件")
}

/// 造一个手上同时有肉与香草（因此两条配方恒同时是候选）的实体。
fn spawn_cook(world: &mut WorldState, ids: &Ids, subclasses: Vec<ContentIndex>) -> EntityId {
    let mut interner = Interner::new();
    let placeholder = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
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
        inventory: vec![ItemStack::new(ids.meat, 1), ItemStack::new(ids.herb, 1)],
        equipment: BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
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
        home: None,
    })
}

/// 在给定种子的世界里试做一次，返回学会的那条配方（没学会时 `None`）。
fn experiment_once(
    seed: u64,
    gate: Vec<ContentIndex>,
    held: Vec<ContentIndex>,
) -> Option<ContentIndex> {
    let all_ids = ids();
    let cooking = all_ids.cooking;
    let recipes = FakeRecipes { ids: ids(), gate };
    let mut world = test_world(seed);
    let cook = spawn_cook(&mut world, &all_ids, held);

    let catalogs = ResolveCatalogs {
        recipes: &recipes,
        ..ResolveCatalogs::empty()
    };
    let effects = resolve_with_catalogs(
        &world,
        &Intent::Experiment {
            actor: cook,
            category: cooking,
        },
        &catalogs,
    );
    effects.iter().find_map(|effect| match effect {
        Effect::LearnRecipe { recipe, .. } => Some(*recipe),
        _ => None,
    })
}

#[test]
fn 同一种子下试做恒选出同一条配方() {
    // 约束 C3：随机由 `(世界种子, 实体 ID, 事件计数)` 三元组**算出**，
    // 不是从共享流里取出——同一个三元组在任何时候都得到同一个结果。
    // Arrange & Act
    let first = experiment_once(20260823, Vec::new(), Vec::new());
    let second = experiment_once(20260823, Vec::new(), Vec::new());

    // Assert
    assert!(first.is_some(), "两条候选都满足条件，必定学会其中一条");
    assert_eq!(first, second);
}

#[test]
fn 不同种子下试做会选出不同的配方() {
    // 上一条只证明「稳定」，不能排除「恒取候选列表第一条」这种把掷骰
    // 完全短路掉的实现——本条补上那一半：换一批种子，选中的配方必须
    // 真的出现过两种取值。
    // Arrange
    let ids = ids();

    // Act：逐个种子试，直到收集到两种不同的结果为止。
    let mut seen: Vec<ContentIndex> = Vec::new();
    for seed in 1..200u64 {
        if let Some(picked) = experiment_once(seed, Vec::new(), Vec::new())
            && !seen.contains(&picked)
        {
            seen.push(picked);
        }
        if seen.len() == 2 {
            break;
        }
    }

    // Assert
    assert_eq!(
        seen.len(),
        2,
        "199 个种子里必须两条候选都被选中过，否则掷骰没有真的在起作用"
    );
    assert!(seen.contains(&ids.stew) && seen.contains(&ids.pie));
}

#[test]
fn 没有闸门要求的副职时试做静默无效() {
    // resolve_experiment 第②步：与 resolve_craft 第③步同一份判据。
    // 做不了这一类的人，谈不上在这一类里试。
    // Arrange
    let ids = ids();

    // Act：类别要求 chef 副职，而实体一个副职都没有。
    let picked = experiment_once(20260823, vec![ids.chef_subclass], Vec::new());

    // Assert
    assert_eq!(picked, None);
}

#[test]
fn 持有闸门要求的副职时试做照常发现() {
    // 与上一条构成红/绿对照：唯一的差别是实体持有了那个副职。
    // Arrange
    let ids = ids();

    // Act
    let picked = experiment_once(20260823, vec![ids.chef_subclass], vec![ids.chef_subclass]);

    // Assert
    assert!(picked.is_some());
}
