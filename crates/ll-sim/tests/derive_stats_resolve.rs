//! `derive_stats` 与装备属性接进战斗（P6 第四批）的端到端集成测试——
//! 走真实的 [`resolve_with_skills_traits_pools_and_items`]/[`apply::apply`]
//! 管线，不直接构造 [`Effect`] 抄近路，也不直接调用私有的
//! `resolve_attack`。与 `crates/ll-sim/tests/equip_resolve.rs` 同一套
//! 夹具手法（`FakeItems`/`spawn_agent`/`test_world`），差异只在于本文件
//! 的 `FakeItems` 额外携带 `stat_bonuses`，覆盖项目任务书要求的三条
//! 端到端与一条四来源叠加：
//!
//! 1. 装备一件加力量的武器 → 攻击伤害真的变高。
//! 2. 装备一件加护甲的防具 → 受到的伤害真的变低（防御端第一次真的
//!    生效——手工验证过这条会红，见 `护甲加成真的降低受到的伤害` 的
//!    测试注释）。
//! 3. 卸下装备 → 加成真的消失（证明是派生不是一次性烘焙）。
//! 4. 技能给的 `active_stat_modifiers` 与装备给的 `stat_bonuses` 同时
//!    生效且相加，不是互相覆盖。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::combat::{Penetration, damage_after_defense};
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemCatalog, ItemRule, ItemStack, StatBonus, StatTarget};
use ll_sim::resolve::{derive_stats, resolve_with_skills_traits_pools_and_items};
use ll_world::entity::{ActiveStatModifier, Agent, AttributeKind, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 一个只认识固定物品索引的测试目录——理由同 `equip_resolve.rs::FakeItems`。
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

/// 建一份认识「猛虎护腕」（力量 +6）/「铁质护甲」（护甲 +8）两种测试
/// 物品的目录，返回各自的索引与目录本身。
fn combat_items() -> (ContentIndex, ContentIndex, FakeItems) {
    let mut interner = Interner::new();
    let gauntlets =
        interner.intern(NamespacedId::parse("lostland:tiger_gauntlets").expect("合法标识符"));
    let armor = interner.intern(NamespacedId::parse("lostland:iron_armor").expect("合法标识符"));
    let items = FakeItems {
        items: BTreeMap::from([
            (
                gauntlets,
                ItemRule {
                    stack_limit: 1,
                    equip_mask: EquipSlot::HAND_L.mask(),
                    stat_bonuses: vec![StatBonus {
                        target: StatTarget::Attribute(AttributeKind::Strength),
                        amount: 6,
                    }],
                    use_effect: None,
                },
            ),
            (
                armor,
                ItemRule {
                    stack_limit: 1,
                    equip_mask: EquipSlot::BODY.mask(),
                    stat_bonuses: vec![StatBonus {
                        target: StatTarget::Armor,
                        amount: 8,
                    }],
                    use_effect: None,
                },
            ),
        ]),
    };
    (gauntlets, armor, items)
}

/// 造一个占位实体，站在 `(5, 5)`，健康值/背包/装备栏/状态效果由调用方
/// 给出——理由同 `equip_resolve.rs::spawn_agent`。
fn spawn_agent(
    world: &mut WorldState,
    health: i32,
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
    active_stat_modifiers: BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>,
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
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        luck: 0,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory,
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers,
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

/// 把 `intent` 结算并应用到 `world`——本文件全部测试共用的一步。
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
fn 装备力量武器后攻击伤害真的变高() {
    // 端到端验证：走真实 Intent::Equip 把猛虎护腕从背包穿上，再走真实
    // Intent::Attack，断言目标掉血量对应「基础力量 + 6」算出的伤害，
    // 不是裸基础力量的伤害。
    // Arrange
    let (gauntlets, _armor, items) = combat_items();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(gauntlets, 1)],
        BTreeMap::new(),
        BTreeMap::new(),
    );
    let victim = spawn_agent(
        &mut world,
        1_000,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor: attacker,
            def: gauntlets,
        },
        &items,
    );
    let expected_damage =
        damage_after_defense(BaseStats::BASELINE.strength + 6, 0, Penetration::NONE);

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &items,
    );

    // Assert
    let victim_after = world.actors.get(victim).expect("生命值远高于伤害,不会死亡");
    assert_eq!(victim_after.health, 1_000 - expected_damage);
}

#[test]
fn 护甲加成真的降低受到的伤害() {
    // 端到端验证——防御端第一次真的生效：两个初始生命值相同的目标,
    // 一个穿铁质护甲、一个不穿,承受同一个攻击者的同一次攻击,穿甲者
    // 掉血应严格少于不穿甲者。
    //
    // 手工验证过这条会红：把 `resolve_attack` 里
    // `damage_after_defense(attack_power, defender_derived.armor(), ..)`
    // 的第二个参数改回硬编码 `0`（本批次改动前的样子）重跑本测试,
    // `armored_damage`/`unarmored_damage` 变得相等,断言从通过变为
    // 失败——完整记录见任务报告「护甲加成怎么变红」一节。
    // Arrange
    let (_gauntlets, armor, items) = combat_items();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    let armored = spawn_agent(
        &mut world,
        1_000,
        Vec::new(),
        BTreeMap::from([(EquipSlot::BODY, ItemStack::new(armor, 1))]),
        BTreeMap::new(),
    );
    let unarmored = spawn_agent(
        &mut world,
        1_000,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: armored,
        },
        &items,
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: unarmored,
        },
        &items,
    );

    // Assert
    let armored_damage = 1_000 - world.actors.get(armored).expect("生命值远高于伤害").health;
    let unarmored_damage = 1_000
        - world
            .actors
            .get(unarmored)
            .expect("生命值远高于伤害")
            .health;
    assert!(armored_damage < unarmored_damage);
}

#[test]
fn 卸下装备后力量加成真的消失() {
    // 端到端验证——证明是派生不是一次性烘焙：装备→攻击→记录伤害，
    // 卸下→再攻击→伤害必须精确回落到裸基础力量算出的那个（更低的）
    // 数字，不是继续沿用装备时算出的旧值。
    // Arrange
    let (gauntlets, _armor, items) = combat_items();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(gauntlets, 1)],
        BTreeMap::new(),
        BTreeMap::new(),
    );
    let victim = spawn_agent(
        &mut world,
        10_000,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor: attacker,
            def: gauntlets,
        },
        &items,
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &items,
    );
    let health_after_equipped_attack = world.actors.get(victim).unwrap().health;

    // Act：卸下猛虎护腕，再攻击一次。
    resolve_and_apply(
        &mut world,
        &Intent::Unequip {
            actor: attacker,
            slot: EquipSlot::HAND_L,
        },
        &items,
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &items,
    );

    // Assert：第二下伤害精确等于裸基础力量算出的伤害,严格小于第一下
    // （带 +6 力量加成）的伤害。
    let unequipped_damage = health_after_equipped_attack - world.actors.get(victim).unwrap().health;
    let baseline_damage = damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);
    assert_eq!(unequipped_damage, baseline_damage);
}

#[test]
fn 技能状态效果与装备加成同时生效且相加而非互相覆盖() {
    // 端到端验证「四个来源要能叠加」的其中两个：技能类效果（模拟为
    // 直接写入 active_stat_modifiers，与真实技能释放写入同一份数据，
    // 见 ActiveStatModifier 文档）给 +4 力量，装备（猛虎护腕）给 +6
    // 力量，两者必须求和成 +10，不是只生效其中一个。
    // Arrange
    let (gauntlets, _armor, items) = combat_items();
    let mut world = test_world();
    let mut interner = Interner::new();
    let buff_source =
        interner.intern(NamespacedId::parse("lostland:battle_cry").expect("合法标识符"));
    let active_stat_modifiers = BTreeMap::from([(
        AttributeKind::Strength,
        BTreeMap::from([(
            buff_source,
            ActiveStatModifier {
                delta: 4,
                expires_at: Tick(1_000),
            },
        )]),
    )]);
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(gauntlets, 1)],
        BTreeMap::new(),
        active_stat_modifiers,
    );
    let victim = spawn_agent(
        &mut world,
        1_000,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor: attacker,
            def: gauntlets,
        },
        &items,
    );
    let expected_damage =
        damage_after_defense(BaseStats::BASELINE.strength + 4 + 6, 0, Penetration::NONE);

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &items,
    );

    // Assert
    let victim_after = world.actors.get(victim).expect("生命值远高于伤害,不会死亡");
    assert_eq!(victim_after.health, 1_000 - expected_damage);
}

#[test]
fn 受到近战攻击后已装备物品耐久真的减少() {
    // 耐久与 Intent::Use 落地批次（P6 第五批）——「耐久何时消耗」的
    // 结论：被击中掉防御方装备耐久，见 `resolve_attack` 文档「耐久
    // 消耗」一节。端到端验证：防御方穿着耐久 5 的铁质护甲,挨一下近战
    // 攻击后耐久必须精确减到 4,不是保持不变。
    // Arrange
    let (_gauntlets, armor, items) = combat_items();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    let defender = spawn_agent(
        &mut world,
        1_000,
        Vec::new(),
        BTreeMap::from([(EquipSlot::BODY, ItemStack::with_durability(armor, 1, 5))]),
        BTreeMap::new(),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: defender,
        },
        &items,
    );

    // Assert
    let stack = world
        .actors
        .get(defender)
        .expect("生命值远高于伤害,不会死亡")
        .equipment
        .get(&EquipSlot::BODY)
        .expect("护甲仍在装备栏里");
    assert_eq!(stack.durability, Some(4));
}

#[test]
fn 没有耐久概念的已装备物品被击中后不产出耐久调整效果() {
    // 反例：耐久与 Intent::Use 落地批次之前既有的装备（`ItemStack::new`
    // 恒 `durability: None`）挨打时不该凭空长出一个耐久值——
    // resolve_attack 只对 `durability.is_some()` 的堆产出
    // `Effect::AdjustEquipmentDurability`,证明这条判定不是恒真。
    // Arrange
    let (_gauntlets, armor, items) = combat_items();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    let defender = spawn_agent(
        &mut world,
        1_000,
        Vec::new(),
        BTreeMap::from([(EquipSlot::BODY, ItemStack::new(armor, 1))]),
        BTreeMap::new(),
    );

    // Act
    let effects = resolve_with_skills_traits_pools_and_items(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: defender,
        },
        &ll_sim::skill::NoSkills,
        &ll_sim::traits::NoTraitGrants,
        &ll_sim::traits::NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        &items,
    );

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::AdjustEquipmentDurability { .. }))
    );
}

#[test]
fn 耐久归零的护甲不再贡献护甲加成() {
    // 耐久与 Intent::Use 落地批次（P6 第五批）——「耐久归零怎么办」的
    // 结论：损坏不可用但不消失（`item-system.md` 六节）。derive_stats
    // 是这句话在结算侧的落点：直接调用 derive_stats（不经完整战斗
    // 流程），装备一件耐久已经归零的护甲,断言护甲加成没有生效。
    // Arrange
    let (_gauntlets, armor_def, items) = combat_items();
    let equipment =
        BTreeMap::from([(EquipSlot::BODY, ItemStack::with_durability(armor_def, 1, 0))]);

    // Act
    let derived = derive_stats(
        BaseStats::BASELINE,
        &BTreeMap::new(),
        &equipment,
        &items,
        Tick(0),
    );

    // Assert
    assert_eq!(derived.armor(), 0);
}

#[test]
fn 耐久未耗尽的护甲仍然贡献护甲加成() {
    // 反例：与上一条测试成对——耐久为正（未耗尽）时,同一件护甲必须
    // 照常生效,证明「归零跳过」这条判定不是恒真,而是真的在读
    // durability 的具体取值。
    // Arrange
    let (_gauntlets, armor_def, items) = combat_items();
    let equipment =
        BTreeMap::from([(EquipSlot::BODY, ItemStack::with_durability(armor_def, 1, 5))]);

    // Act
    let derived = derive_stats(
        BaseStats::BASELINE,
        &BTreeMap::new(),
        &equipment,
        &items,
        Tick(0),
    );

    // Assert：combat_items() 里铁质护甲的加成是 +8。
    assert_eq!(derived.armor(), 8);
}
