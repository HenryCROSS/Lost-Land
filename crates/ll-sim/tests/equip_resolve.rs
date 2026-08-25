//! 装备栏位批次（P6 第三批）的端到端集成测试——走真实的
//! [`resolve_with_skills_traits_pools_and_items`]/[`apply::apply`] 管线，
//! 不直接构造 [`Effect`] 抄近路。与
//! `crates/ll-sim/tests/item_resolve.rs` 同一个理由独立成文件、同一套
//! 夹具手法（差异只在于本文件的 `FakeItems` 额外携带 `equip_mask`），
//! 覆盖项目任务书要求的核心链路：
//!
//! 1. 基础装备/卸下：单槽位物品从背包进装备栏、再卸回背包。
//! 2. 占位冲突正例：装备双手武器后，从任一手（含非锚点的副手）请求
//!    卸下都能定位到同一件武器。
//! 3. 占位冲突反例（双向）：双手武器与单槽位物品互相顶替时,占位冲突
//!    检查在两个方向上都生效——`crates/ll-mod/tests/example_mod_equipment.rs`
//!    用真实 mod 内容覆盖了这两条,本文件用假物品目录覆盖更多边界
//!    情形（静默无效的各种触发条件）。
//! 4. 静默无效：`actor` 不存在、背包没有对应物品、物品不可装备、
//!    请求卸下的槽位没有任何东西覆盖。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::combat::Penetration;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemCatalog, ItemRule, ItemStack, SlotMask, WearChannels};
use ll_sim::resolve::resolve_with_skills_traits_pools_and_items;
use ll_world::entity::{Agent, BaseStats, EntityId};
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

/// 造一个占位实体，站在 `(5, 5)`，背包/装备栏初始内容由调用方给出——
/// 理由同 `item_resolve.rs::spawn_agent`。
fn spawn_agent(
    world: &mut WorldState,
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
) -> EntityId {
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
        spent_slots: BTreeMap::new(),
        inventory,
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
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

/// 建一份认识铁剑（单手，主手）/木盾（单手，副手）/双手剑（主手+副手）
/// 三种测试物品的目录，返回各自的索引与目录本身。
fn equip_items() -> (ContentIndex, ContentIndex, ContentIndex, FakeItems) {
    let mut interner = Interner::new();
    let sword = interner.intern(NamespacedId::parse("lostland:iron_sword").expect("合法标识符"));
    let shield =
        interner.intern(NamespacedId::parse("lostland:wooden_shield").expect("合法标识符"));
    let greatsword =
        interner.intern(NamespacedId::parse("lostland:greatsword").expect("合法标识符"));
    let items = FakeItems {
        items: BTreeMap::from([
            (
                sword,
                ItemRule {
                    wear_channels: WearChannels::NONE,
                    max_durability: None,
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
                    stack_limit: 1,
                    equip_mask: EquipSlot::MAIN_HAND.mask(),
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                },
            ),
            (
                shield,
                ItemRule {
                    wear_channels: WearChannels::NONE,
                    max_durability: None,
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
                    stack_limit: 1,
                    equip_mask: EquipSlot::OFF_HAND.mask(),
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                },
            ),
            (
                greatsword,
                ItemRule {
                    wear_channels: WearChannels::NONE,
                    max_durability: None,
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
                    stack_limit: 1,
                    equip_mask: EquipSlot::MAIN_HAND
                        .mask()
                        .union(EquipSlot::OFF_HAND.mask()),
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                },
            ),
        ]),
    };
    (sword, shield, greatsword, items)
}

fn resolve_and_apply(world: &mut WorldState, intent: &Intent, items: &FakeItems) {
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
}

#[test]
fn 装备背包里的单手武器后进入装备栏且离开背包() {
    // Arrange
    let mut world = test_world();
    let (sword, _shield, _greatsword, items) = equip_items();
    let actor = spawn_agent(&mut world, vec![ItemStack::new(sword, 1)], BTreeMap::new());

    // Act
    resolve_and_apply(&mut world, &Intent::Equip { actor, def: sword }, &items);

    // Assert
    let agent = world.actors.get(actor).unwrap();
    assert!(agent.inventory.is_empty());
    assert_eq!(
        agent.equipment,
        BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(sword, 1))])
    );
}

#[test]
fn 卸下已装备的单手武器后回到背包且离开装备栏() {
    // Arrange
    let mut world = test_world();
    let (sword, _shield, _greatsword, items) = equip_items();
    let actor = spawn_agent(
        &mut world,
        Vec::new(),
        BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(sword, 1))]),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Unequip {
            actor,
            slot: EquipSlot::MAIN_HAND,
        },
        &items,
    );

    // Assert
    let agent = world.actors.get(actor).unwrap();
    assert!(agent.equipment.is_empty());
    assert_eq!(agent.inventory, vec![ItemStack::new(sword, 1)]);
}

#[test]
fn 装备双手剑后从副手请求卸下也能定位到同一把武器() {
    // 占位冲突正例：双手剑只存一份、锚点在主手，但从副手（非锚点）
    // 发起卸下请求也必须成功——证明"两个槽位都被占用"不是只在锚点
    // 那一侧成立。
    // Arrange
    let mut world = test_world();
    let (_sword, _shield, greatsword, items) = equip_items();
    let actor = spawn_agent(
        &mut world,
        vec![ItemStack::new(greatsword, 1)],
        BTreeMap::new(),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor,
            def: greatsword,
        },
        &items,
    );

    // Act：从副手卸,不是锚点主手。
    resolve_and_apply(
        &mut world,
        &Intent::Unequip {
            actor,
            slot: EquipSlot::OFF_HAND,
        },
        &items,
    );

    // Assert
    let agent = world.actors.get(actor).unwrap();
    assert!(agent.equipment.is_empty());
    assert_eq!(agent.inventory, vec![ItemStack::new(greatsword, 1)]);
}

#[test]
fn 装备双手剑后再装备副手物品会连带卸下双手剑() {
    // 占位冲突正例（正向）：双手剑占满两手,再装备只占副手的木盾——
    // 木盾应当顶替双手剑,双手剑回到背包。
    // Arrange
    let mut world = test_world();
    let (_sword, shield, greatsword, items) = equip_items();
    let actor = spawn_agent(
        &mut world,
        vec![ItemStack::new(greatsword, 1), ItemStack::new(shield, 1)],
        BTreeMap::new(),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor,
            def: greatsword,
        },
        &items,
    );

    // Act
    resolve_and_apply(&mut world, &Intent::Equip { actor, def: shield }, &items);

    // Assert
    let agent = world.actors.get(actor).unwrap();
    assert_eq!(
        agent.equipment,
        BTreeMap::from([(EquipSlot::OFF_HAND, ItemStack::new(shield, 1))])
    );
    assert_eq!(agent.inventory, vec![ItemStack::new(greatsword, 1)]);
}

#[test]
fn 副手已装备物品时装备双手剑会连带卸下副手物品() {
    // 占位冲突反例（反向）：与上一条相反的起点——副手已经单独被木盾
    // 占用,这次装备双手剑,证明冲突检查双向都生效,不是只识别"新物品
    // 单槽、已装备多槽"这一种组合。
    // Arrange
    let mut world = test_world();
    let (_sword, shield, greatsword, items) = equip_items();
    let actor = spawn_agent(
        &mut world,
        vec![ItemStack::new(greatsword, 1)],
        BTreeMap::from([(EquipSlot::OFF_HAND, ItemStack::new(shield, 1))]),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor,
            def: greatsword,
        },
        &items,
    );

    // Assert
    let agent = world.actors.get(actor).unwrap();
    assert_eq!(
        agent.equipment,
        BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(greatsword, 1))])
    );
    assert_eq!(agent.inventory, vec![ItemStack::new(shield, 1)]);
}

#[test]
fn 不相交槽位的两件单手装备可以同时装备互不影响() {
    // 反面对照：铁剑（主手）与木盾（副手）不冲突,应当能同时装备,
    // 证明占位冲突判定不会"误伤"真正不相交的槽位组合。
    // Arrange
    let mut world = test_world();
    let (sword, shield, _greatsword, items) = equip_items();
    let actor = spawn_agent(
        &mut world,
        vec![ItemStack::new(sword, 1), ItemStack::new(shield, 1)],
        BTreeMap::new(),
    );
    resolve_and_apply(&mut world, &Intent::Equip { actor, def: sword }, &items);

    // Act
    resolve_and_apply(&mut world, &Intent::Equip { actor, def: shield }, &items);

    // Assert：两件都在装备栏,背包清空。
    let agent = world.actors.get(actor).unwrap();
    assert_eq!(
        agent.equipment,
        BTreeMap::from([
            (EquipSlot::MAIN_HAND, ItemStack::new(sword, 1)),
            (EquipSlot::OFF_HAND, ItemStack::new(shield, 1)),
        ])
    );
    assert!(agent.inventory.is_empty());
}

#[test]
fn 实体不存在时装备意图静默无效() {
    // Arrange
    let mut world = test_world();
    let (sword, _shield, _greatsword, items) = equip_items();
    let ghost = {
        let mut arena: ll_world::entity::Arena<()> = ll_world::entity::Arena::new();
        let id = arena.spawn(());
        arena.despawn(id);
        id
    };

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::Equip {
            actor: ghost,
            def: sword,
        },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );

    // Assert
    let _ = &mut world;
    assert!(effects.is_empty());
}

#[test]
fn 背包里没有对应物品时装备意图静默无效() {
    // Arrange
    let mut world = test_world();
    let (sword, _shield, _greatsword, items) = equip_items();
    let actor = spawn_agent(&mut world, Vec::new(), BTreeMap::new());

    // Act
    resolve_and_apply(&mut world, &Intent::Equip { actor, def: sword }, &items);

    // Assert
    let agent = world.actors.get(actor).unwrap();
    assert!(agent.equipment.is_empty());
}

#[test]
fn 物品不可装备时装备意图静默无效() {
    // Arrange：一件在目录里注册过、但 equip_mask 为空的物品（材料类）。
    let mut world = test_world();
    let mut interner = Interner::new();
    let ore = interner.intern(NamespacedId::parse("lostland:iron_ore").expect("合法标识符"));
    let items = FakeItems {
        items: BTreeMap::from([(
            ore,
            ItemRule {
                wear_channels: WearChannels::NONE,
                max_durability: None,
                taught_recipes: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                blind_box_pool: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
                stack_limit: 99,
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
            },
        )]),
    };
    let actor = spawn_agent(&mut world, vec![ItemStack::new(ore, 1)], BTreeMap::new());

    // Act
    resolve_and_apply(&mut world, &Intent::Equip { actor, def: ore }, &items);

    // Assert：物品仍在背包,没有被移进装备栏。
    let agent = world.actors.get(actor).unwrap();
    assert!(agent.equipment.is_empty());
    assert_eq!(agent.inventory, vec![ItemStack::new(ore, 1)]);
}

#[test]
fn 物品目录查不到时装备意图静默无效() {
    // Arrange：物品在背包里,但目录里完全没有登记（模拟"没有真实物品
    // 注册表可查"的场景）——与 resolve_pick_up 对 stack_limit 查不到
    // 时"按不限量处理"的宽容方向相反,装备是持久世界状态变化,必须要求
    // 明确的规则才能生效,见 resolve_equip 文档。
    let mut world = test_world();
    let mut interner = Interner::new();
    let unknown = interner.intern(NamespacedId::parse("lostland:mystery").expect("合法标识符"));
    let items = FakeItems {
        items: BTreeMap::new(),
    };
    let actor = spawn_agent(
        &mut world,
        vec![ItemStack::new(unknown, 1)],
        BTreeMap::new(),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor,
            def: unknown,
        },
        &items,
    );

    // Assert
    let agent = world.actors.get(actor).unwrap();
    assert!(agent.equipment.is_empty());
    assert_eq!(agent.inventory, vec![ItemStack::new(unknown, 1)]);
}

#[test]
fn 请求卸下没有任何装备覆盖的槽位时静默无效() {
    // Arrange
    let mut world = test_world();
    let (_sword, _shield, _greatsword, items) = equip_items();
    let actor = spawn_agent(&mut world, Vec::new(), BTreeMap::new());

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Unequip {
            actor,
            slot: EquipSlot::HEAD,
        },
        &items,
    );

    // Assert
    let agent = world.actors.get(actor).unwrap();
    assert!(agent.equipment.is_empty());
}
