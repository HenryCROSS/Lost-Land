//! `Intent::Use` 的端到端集成测试（耐久与 `Intent::Use` 落地批次，P6
//! 第五批）——走真实的
//! [`resolve_with_skills_traits_pools_and_items`]/[`apply::apply`] 管线，
//! 不直接构造 [`Effect`] 抄近路。夹具手法与
//! `crates/ll-sim/tests/item_resolve.rs` 一致（`FakeItems`/`spawn_agent`/
//! `test_world`），差异只在于本文件的 `FakeItems` 额外携带
//! `use_effect`（复用 [`SkillEffect`]，见
//! `ll_sim::item::ItemRule::use_effect` 文档「为什么复用 SkillEffect」
//! 一节）。
//!
//! 覆盖项目任务书要求的核心链路：
//!
//! 1. 使用一件恢复资源的消耗品 → 数量减一、资源真的恢复。
//! 2. 用掉最后一件 → 整条堆从背包消失（不留 `count == 0` 的死堆）。
//! 3. 使用一件造成伤害的消耗品 → 真的掉血，致死时产出 `Effect::Kill`。
//! 4. 使用一件临时属性修正的消耗品 → 写入 `active_stat_modifiers`。
//! 5. 三种「静默无效」情形：物品没有 `use_effect`、背包里没有这件
//!    物品、`def` 查不到物品规则。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::combat::Penetration;
use ll_sim::intent::Intent;
use ll_sim::item::{ItemCatalog, ItemRule, ItemStack, SlotMask};
use ll_sim::resolve::resolve_with_skills_traits_pools_and_items;
use ll_sim::skill::{ResourceKind, SkillEffect};
use ll_world::entity::{Agent, AttributeKind, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 一个只认识固定物品索引的测试目录——理由同 `item_resolve.rs::FakeItems`。
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

/// 造一个占位实体，站在 `(5, 5)`，健康值/背包由调用方给出。
fn spawn_agent(world: &mut WorldState, health: i32, inventory: Vec<ItemStack>) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: 0,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
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
    })
}

/// 造一件带指定 `use_effect` 的测试物品，返回其索引与目录。
fn potion_with_effect(effect: SkillEffect) -> (ContentIndex, FakeItems) {
    let mut interner = Interner::new();
    let potion = interner.intern(NamespacedId::parse("lostland:test_potion").expect("合法标识符"));
    let items = FakeItems {
        items: BTreeMap::from([(
            potion,
            ItemRule {
                stack_limit: 10,
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: Some(effect),
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
            },
        )]),
    };
    (potion, items)
}

/// 把 `intent` 结算并应用到 `world`，返回产出的效果列表（部分测试要
/// 断言"没有产出任何效果"，需要拿到这份列表本身，不能只看 apply 后的
/// 世界状态）。
fn resolve_and_apply(
    world: &mut WorldState,
    intent: &Intent,
    items: &FakeItems,
) -> Vec<ll_sim::effect::Effect> {
    let effects = resolve_with_skills_traits_pools_and_items(
        world,
        intent,
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        items,
    );
    for effect in &effects {
        apply(world, effect);
    }
    effects
}

#[test]
fn 使用恢复资源的消耗品后数量减一且资源真的恢复() {
    // Arrange
    let (potion, items) = potion_with_effect(SkillEffect::RestoreResource {
        resource: ResourceKind::Mana,
        base: 30,
    });
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(potion, 3)],
    );

    // Act
    resolve_and_apply(&mut world, &Intent::Use { actor, def: potion }, &items);

    // Assert
    let agent = world.actors.get(actor).expect("刚生成的实体必然存在");
    assert_eq!(agent.mana, 30);
    let stack = agent
        .inventory
        .iter()
        .find(|s| s.def == potion)
        .expect("还剩两瓶,堆本身仍在背包里");
    assert_eq!(stack.count, 2);
}

#[test]
fn 用掉最后一瓶消耗品后整条堆从背包消失() {
    // 反例：与上一条测试成对——数量恰好是一时,用掉之后不该留一个
    // count == 0 的死堆,整条从背包移除。
    // Arrange
    let (potion, items) = potion_with_effect(SkillEffect::RestoreResource {
        resource: ResourceKind::Mana,
        base: 30,
    });
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(potion, 1)],
    );

    // Act
    resolve_and_apply(&mut world, &Intent::Use { actor, def: potion }, &items);

    // Assert
    let agent = world.actors.get(actor).expect("刚生成的实体必然存在");
    assert!(!agent.inventory.iter().any(|s| s.def == potion));
}

#[test]
fn 使用造成伤害的消耗品后真的扣血() {
    // Arrange
    let (potion, items) = potion_with_effect(SkillEffect::DealDamage { base: 15 });
    let mut world = test_world();
    let actor = spawn_agent(&mut world, 1_000, vec![ItemStack::new(potion, 1)]);

    // Act
    resolve_and_apply(&mut world, &Intent::Use { actor, def: potion }, &items);

    // Assert
    assert_eq!(
        world
            .actors
            .get(actor)
            .expect("生命值远高于伤害,不会死亡")
            .health,
        1_000 - 15
    );
}

#[test]
fn 使用造成伤害的消耗品致死时实体真的被销毁() {
    // Arrange：生命值恰好等于伤害量,这一下必死。
    let (potion, items) = potion_with_effect(SkillEffect::DealDamage { base: 15 });
    let mut world = test_world();
    let actor = spawn_agent(&mut world, 15, vec![ItemStack::new(potion, 1)]);

    // Act
    resolve_and_apply(&mut world, &Intent::Use { actor, def: potion }, &items);

    // Assert
    assert!(world.actors.get(actor).is_none());
}

#[test]
fn 使用临时属性修正的消耗品后写入活跃属性修正表() {
    // Arrange
    let (potion, items) = potion_with_effect(SkillEffect::TemporaryStatModifier {
        attribute: AttributeKind::Strength,
        amount: 5,
        duration_ticks: 100,
    });
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(potion, 1)],
    );

    // Act
    resolve_and_apply(&mut world, &Intent::Use { actor, def: potion }, &items);

    // Assert
    let agent = world.actors.get(actor).expect("刚生成的实体必然存在");
    let per_source = agent
        .active_stat_modifiers
        .get(&AttributeKind::Strength)
        .expect("应当已经写入力量项的修正表");
    let modifier = per_source
        .get(&potion)
        .expect("来源应当是这件药水自身的 ContentIndex");
    assert_eq!(modifier.delta, 5);
}

#[test]
fn 没有use_effect的物品使用后不产出任何效果() {
    // 反例：材料/装备本身（use_effect 恒 None）不能被 Intent::Use
    // 使用,证明"有 use_effect 才产出效果"这条判定不是恒真。
    // Arrange
    let mut interner = Interner::new();
    let ore = interner.intern(NamespacedId::parse("lostland:iron_ore").expect("合法标识符"));
    let items = FakeItems {
        items: BTreeMap::from([(
            ore,
            ItemRule {
                stack_limit: 99,
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
            },
        )]),
    };
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(ore, 1)],
    );

    // Act
    let effects = resolve_and_apply(&mut world, &Intent::Use { actor, def: ore }, &items);

    // Assert
    assert!(effects.is_empty());
}

#[test]
fn 背包里没有这件物品时使用不产出任何效果() {
    // Arrange
    let (potion, items) = potion_with_effect(SkillEffect::RestoreResource {
        resource: ResourceKind::Mana,
        base: 30,
    });
    let mut world = test_world();
    let actor = spawn_agent(&mut world, Agent::STARTING_HEALTH, Vec::new());

    // Act
    let effects = resolve_and_apply(&mut world, &Intent::Use { actor, def: potion }, &items);

    // Assert
    assert!(effects.is_empty());
}
