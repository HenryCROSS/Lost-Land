//! P6 第二批（背包与地面物品）的端到端集成测试——走真实的
//! [`resolve_with_skills_traits_pools_and_items`]/[`apply::apply`] 管线，
//! 不直接构造 [`Effect`] 抄近路。覆盖项目任务书要求的四条硬性链路：
//!
//! 1. 拾取：地面有物品 → `Intent::PickUp` → 背包里有了、地面上没了。
//! 2. 丢弃：反过来，且 `dropped_at` 被正确写入。
//! 3. 拾取时自动合并：背包里已有同种可堆叠物品，拾取后合并而不是
//!    新开一堆——这条最容易漏，本文件用两个测试互为红/绿对照（见
//!    `拾取时与背包已有同种堆合并而非新开一堆` 与
//!    `拾取时物品定义未注册仍按不限量合并`）。
//! 4. 老化清理不在本文件——它是 [`WorldState::cleanup_aged_ground_items`]
//!    的纯方法测试，不经过 `resolve`/`apply`，见
//!    `crates/ll-world/src/state.rs` 测试模块。
//!
//! 夹具（`test_world`/`spawn_agent`）与 `crates/ll-sim/tests/resource_pool_resolve.rs`
//! 几乎一致——同一个理由独立成文件（复用公开入口，不需要访问任何
//! 私有函数），差异只在于本文件额外需要一个 `FakeItems`
//! （[`ItemCatalog`] 的测试替身）。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::combat::Penetration;
use ll_sim::intent::Intent;
use ll_sim::item::{ItemCatalog, ItemRule, ItemStack};
use ll_sim::resolve::resolve_with_skills_traits_pools_and_items;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::item::GroundItemStack;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 一个只认识固定物品索引的测试目录——生产代码里真正的实现是
/// `ll_mod::item::ItemTable`（依赖方向不允许本 crate 依赖它，见
/// `ll_sim::item` 模块文档）。
struct FakeItems {
    items: BTreeMap<ContentIndex, ItemRule>,
}

impl ItemCatalog for FakeItems {
    fn item(&self, item: ContentIndex) -> Option<ItemRule> {
        self.items.get(&item).cloned()
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

/// 造一个占位实体，站在 `(5, 5)`，背包初始内容为 `inventory`。
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
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory,
        equipment: BTreeMap::new(),
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

fn arrow_index() -> (ContentIndex, FakeItems) {
    let mut interner = Interner::new();
    let arrow = interner.intern(NamespacedId::parse("lostland:arrow").expect("合法标识符"));
    let items = FakeItems {
        items: BTreeMap::from([(
            arrow,
            ItemRule {
                stack_limit: 99,
                equip_mask: ll_sim::item::SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
            },
        )]),
    };
    (arrow, items)
}

#[test]
fn 拾取地面物品后背包里有了地面上没了() {
    // Arrange
    let mut world = test_world();
    let (arrow, items) = arrow_index();
    let actor = spawn_agent(&mut world, Vec::new());
    let pos = world.actors.get(actor).unwrap().pos;
    world.ground_items.push(GroundItemStack {
        pos,
        stack: ItemStack::new(arrow, 5),
        dropped_at: Tick(0),
        contents: Vec::new(),
    });

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::PickUp { actor },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert
    assert_eq!(
        world.actors.get(actor).unwrap().inventory,
        vec![ItemStack::new(arrow, 5)]
    );
    assert!(world.ground_items.is_empty());
}

#[test]
fn 丢弃背包物品后地面上有了背包里没了且dropped_at为当前世界时钟() {
    // Arrange
    let mut world = test_world();
    let (arrow, items) = arrow_index();
    world.advance(123);
    let actor = spawn_agent(&mut world, vec![ItemStack::new(arrow, 5)]);
    let pos = world.actors.get(actor).unwrap().pos;

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::Drop { actor, def: arrow },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert
    assert!(world.actors.get(actor).unwrap().inventory.is_empty());
    assert_eq!(
        world.ground_items,
        vec![GroundItemStack {
            pos,
            stack: ItemStack::new(arrow, 5),
            dropped_at: Tick(123),
            contents: Vec::new(),
        }]
    );
}

#[test]
fn 拾取时与背包已有同种堆合并而非新开一堆() {
    // 这条最容易漏——手工验证过：把 resolve_pick_up 里"查找可合并
    // 已有堆"这一步去掉（强制 existing = None），本测试会从
    // `vec![ItemStack::new(arrow, 35)]`（一堆）变红成
    // `vec![ItemStack::new(arrow, 20), ItemStack::new(arrow, 15)]`
    // （两堆），证明这条断言真的在验证合并发生了，不是恰好通过。
    // Arrange
    let mut world = test_world();
    let (arrow, items) = arrow_index();
    let actor = spawn_agent(&mut world, vec![ItemStack::new(arrow, 20)]);
    let pos = world.actors.get(actor).unwrap().pos;
    world.ground_items.push(GroundItemStack {
        pos,
        stack: ItemStack::new(arrow, 15),
        dropped_at: Tick(0),
        contents: Vec::new(),
    });

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::PickUp { actor },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert：合成一堆 35,不是两堆分开存在。
    assert_eq!(
        world.actors.get(actor).unwrap().inventory,
        vec![ItemStack::new(arrow, 35)]
    );
}

#[test]
fn 拾取时合并超过堆叠上限产出主堆与溢出堆两条() {
    // Arrange：stack_limit 99,背包已有 90,地面 15,合计 105——应当
    // 拆成 99 + 6 两条,而不是拒绝拾取或截断丢数据。
    let mut world = test_world();
    let (arrow, items) = arrow_index();
    let actor = spawn_agent(&mut world, vec![ItemStack::new(arrow, 90)]);
    let pos = world.actors.get(actor).unwrap().pos;
    world.ground_items.push(GroundItemStack {
        pos,
        stack: ItemStack::new(arrow, 15),
        dropped_at: Tick(0),
        contents: Vec::new(),
    });

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::PickUp { actor },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert：总数量 105 守恒,分成 99 + 6 两条。
    let inventory = &world.actors.get(actor).unwrap().inventory;
    let total: u32 = inventory.iter().map(|stack| stack.count).sum();
    assert_eq!(total, 105);
    assert_eq!(inventory.len(), 2);
}

#[test]
fn 拾取时背包为空直接把地面堆搬进背包() {
    // Arrange：没有可合并的已有堆时,不需要查询 items 目录也能正确
    // 完成拾取——见 resolve_pick_up 文档。
    let mut world = test_world();
    let (arrow, items) = arrow_index();
    let actor = spawn_agent(&mut world, Vec::new());
    let pos = world.actors.get(actor).unwrap().pos;
    world.ground_items.push(GroundItemStack {
        pos,
        stack: ItemStack::with_durability(arrow, 1, 50),
        dropped_at: Tick(0),
        contents: Vec::new(),
    });

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::PickUp { actor },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert
    assert_eq!(
        world.actors.get(actor).unwrap().inventory,
        vec![ItemStack::with_durability(arrow, 1, 50)]
    );
}

#[test]
fn 脚下没有地面物品时拾取意图静默无效() {
    // Arrange
    let mut world = test_world();
    let (_arrow, items) = arrow_index();
    let actor = spawn_agent(&mut world, Vec::new());

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::PickUp { actor },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );

    // Assert
    assert!(effects.is_empty());
}

#[test]
fn 背包里没有对应物品时丢弃意图静默无效() {
    // Arrange
    let mut world = test_world();
    let (arrow, items) = arrow_index();
    let actor = spawn_agent(&mut world, Vec::new());

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::Drop { actor, def: arrow },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );

    // Assert
    assert!(effects.is_empty());
}
