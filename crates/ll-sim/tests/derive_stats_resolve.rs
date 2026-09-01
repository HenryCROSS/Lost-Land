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
//!
//! # 幸运并入 `AttributeKind` 批次新增的三条验收
//!
//! 项目所有者裁定幸运并入 `AttributeKind` 之后，本文件追加三条端到端
//! 验收——证明的不是"编译过了"，是"幸运真的能被装备/buff 影响，
//! 且这份影响真的反映到暴击率上"：
//!
//! 5. `装备幸运戒指后有效幸运真的变高`：走 `derive_stats`，不是裸
//!    `BaseStats.luck`。
//! 6. `临时属性修正作用于幸运时生效期内更高过期后回落`：`active_stat_modifiers`
//!    的惰性到期判定同样对幸运成立。
//! 7. `装备幸运戒指后暴击率真的更高`：本条是最终验收——`resolve_attack`
//!    读的是 `attacker_derived.attribute(AttributeKind::Luck)`，不是
//!    `attacker.stats.luck`，只有装备加成真的传导到暴击判定，这条测试
//!    才会通过；手工验证过这条会红，见测试注释。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::combat::{Penetration, damage_after_defense};
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::effect::Effect;
use ll_sim::formula::NoFormulas;
use ll_sim::intent::Intent;
use ll_sim::item::{
    EquipSlot, ItemCatalog, ItemRule, ItemStack, StatBonus, StatTarget, WearChannels,
};
use ll_sim::resolve::{
    derive_stats, resolve, resolve_with_skills_traits_pools_and_items,
    resolve_with_skills_traits_pools_items_formulas_and_damage_categories,
};
use ll_sim::resource_pool::NoResourcePools;
use ll_sim::skill::NoSkills;
use ll_sim::traits::{
    RuleModifier, TraitCatalog, TraitGrant, TraitGrantSource, TraitRule, TypedRuleModifier,
};
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
                    // 「使用」通道：这件夹具在本文件里扮演"挥出去的
                    // 武器"，因此带 on-use、**不带** on-hit——耐久标签
                    // 批次之后这是它会不会磨损的唯一判据。
                    wear_channels: WearChannels::ON_USE,
                    max_durability: None,
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
                    furniture: false,
                    stack_limit: 1,
                    equip_mask: EquipSlot::HAND_L.mask(),
                    stat_bonuses: vec![StatBonus {
                        target: StatTarget::Attribute(AttributeKind::Strength),
                        amount: 6,
                    }],
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                },
            ),
            (
                armor,
                ItemRule {
                    // 「挨打」通道：这件夹具扮演"穿在身上的甲"。
                    wear_channels: WearChannels::ON_HIT,
                    max_durability: None,
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
                    furniture: false,
                    stack_limit: 1,
                    equip_mask: EquipSlot::BODY.mask(),
                    stat_bonuses: vec![StatBonus {
                        target: StatTarget::Armor,
                        amount: 8,
                    }],
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: Vec::new(),
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
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
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
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers,
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
fn 攻击方主手武器的耐久真的减少() {
    // 武器引用与穿透接线批次（P6 第六批）——「耐久何时消耗」的结论从
    // P6 第五批的「被击中掉防御方装备耐久」改判为「攻击时掉攻击方
    // 主手武器耐久」，见 `resolve_attack` 文档「耐久消耗：为什么收窄到
    // 只有武器」一节。端到端验证：攻击方主手上挂着耐久 10 的测试物品
    // （复用 `combat_items()` 的 `armor_def`，这里只借它的 `ItemStack`
    // 形状当"武器"用，`resolve_attack` 不校验 `equip_mask` 是否真的
    // 包含主手），打出一下攻击后耐久必须精确减到 9,不是保持不变。
    // Arrange
    // 耐久标签批次：主手拿的必须是**带 on-use 标签**的那件夹具
    // （护手），不能再随手拿护甲夹具充数——判据已经从"它在主手"变成
    // "它是什么"。
    let (gauntlets, _armor_def, items) = combat_items();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::with_durability(gauntlets, 1, 10),
        )]),
        BTreeMap::new(),
    );
    let defender = spawn_agent(
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
            target: defender,
        },
        &items,
    );

    // Assert
    let stack = world
        .actors
        .get(attacker)
        .expect("攻击者仍然存活")
        .equipment
        .get(&EquipSlot::MAIN_HAND)
        .expect("武器仍在装备栏里");
    assert_eq!(stack.durability, Some(9));
}

#[test]
fn 防御方护甲的耐久因为挨打而减少() {
    // 耐久扩面批次：项目所有者裁定「衣服要耐久，**受到攻击就会减少
    // 耐久**」，推翻了此前「只有装备武器才有耐久」那条裁定。本测试
    // 因此第三次改写——P6 第五批断言 `Some(4)`（全部装备挨打即掉），
    // 第六批收窄后断言 `Some(5)`（护甲不再掉），本批次回到 `Some(4)`,
    // 但回到的方式与第五批不同：现在只有**非武器槽位**才掉，见下一条
    // 测试与 `resolve_attack` 文档「耐久消耗：两条通道」一节。
    // Arrange
    let (_gauntlets, armor_def, items) = combat_items();
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
        BTreeMap::from([(EquipSlot::BODY, ItemStack::with_durability(armor_def, 1, 5))]),
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
        .expect("护甲仍在装备栏里——耐久归零都不自动卸下，何况只掉一点");
    assert_eq!(stack.durability, Some(4));
}

#[test]
fn 只带使用通道标签的装备挨打不减耐久() {
    // 与上一条成对的反例。**耐久标签批次改写了这条的判据**：上一版
    // 把同一件甲从 `BODY` 挪到 `OFF_HAND`，靠"副手属于武器组"来证明
    // 不磨损；项目所有者推翻了那个判据（「副手也可能拿着武器,例如
    // 双刀,双盾」），本版改成换**东西**而不是换槽位——同样放在
    // `OFF_HAND`，换成只带 `ON_USE` 通道的护手夹具，耐久必须原样保持。
    //
    // 这条钉住的是「挨打」通道那句
    // `rule.wear_channels.contains(WearChannels::ON_HIT)` 本身：去掉它，
    // 本条立即从 `Some(5)` 变成 `Some(4)` 而失败。上一版那条按槽位的
    // 断言在本版里已经不成立——同样占副手的木盾现在会磨损，证据见
    // `ll-mod/tests/turn_engine_catalogs.rs`
    // 「副手拿刀与副手拿盾在同一次挨打里结果相反」。
    // Arrange
    let (gauntlets, _armor_def, items) = combat_items();
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
        BTreeMap::from([(
            EquipSlot::OFF_HAND,
            ItemStack::with_durability(gauntlets, 1, 5),
        )]),
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
        .get(&EquipSlot::OFF_HAND)
        .expect("副手装备仍在装备栏里");
    assert_eq!(stack.durability, Some(5));
}

#[test]
fn 主手物品没有耐久概念时攻击不产出耐久调整效果() {
    // 反例，与「攻击方主手武器的耐久真的减少」成对：主手上挂着的物品
    // 若没有耐久概念（`ItemStack::new` 恒 `durability: None`），攻击
    // 不该凭空产出一个耐久调整效果——resolve_attack 只对
    // `durability.is_some()` 的主手堆产出
    // `Effect::AdjustEquipmentDurability`,证明这条判定不是恒真。
    // 耐久扩面批次追加：本场景的防御方装备栏是**空的**，「挨打」通道
    // 因此同样一条效果都不产出——`assert!(!effects.iter().any(..))`
    // 覆盖的是全部产出点，不只是「使用」通道那一条。
    // Arrange
    let (_gauntlets, armor_def, items) = combat_items();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(armor_def, 1))]),
        BTreeMap::new(),
    );
    let defender = spawn_agent(
        &mut world,
        1_000,
        Vec::new(),
        BTreeMap::new(),
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

/// 建一份认识「幸运戒指」（幸运 +20）的目录，返回它的索引与目录本身
/// ——幸运并入 `AttributeKind` 批次新增，与 [`combat_items`] 分开是
/// 因为后者的调用点数量已经不小，不给它的返回元组再加一个字段。
fn luck_ring_item() -> (ContentIndex, FakeItems) {
    let mut interner = Interner::new();
    let ring = interner.intern(NamespacedId::parse("lostland:luck_ring").expect("合法标识符"));
    let items = FakeItems {
        items: BTreeMap::from([(
            ring,
            ItemRule {
                wear_channels: WearChannels::NONE,
                max_durability: None,
                taught_recipes: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                blind_box_pool: Vec::new(),
                furniture: false,
                stack_limit: 1,
                equip_mask: EquipSlot::RING_L.mask(),
                stat_bonuses: vec![StatBonus {
                    target: StatTarget::Attribute(AttributeKind::Luck),
                    amount: 20,
                }],
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
            },
        )]),
    };
    (ring, items)
}

#[test]
fn 装备幸运戒指后有效幸运真的变高() {
    // derive_stats 层面的验收：不装备时有效幸运等于裸 BaseStats.luck
    // （0），装备幸运戒指（+20）后有效幸运必须真的变成 20，不是继续
    // 停在 0——这是「幸运并入 AttributeKind」换来的直接能力：装备加成
    // 现在能通过 StatTarget::Attribute(AttributeKind::Luck) 这条通道
    // 影响幸运，此前 luck 是 Agent 上独立字段，这条通道完全碰不到它。
    // Arrange
    let (ring, items) = luck_ring_item();
    let equipped = BTreeMap::from([(EquipSlot::RING_L, ItemStack::new(ring, 1))]);

    // Act
    let derived = derive_stats(
        BaseStats::BASELINE,
        &BTreeMap::new(),
        &equipped,
        &items,
        Tick(0),
    );

    // Assert
    assert_eq!(derived.attribute(AttributeKind::Luck), 20);
}

#[test]
fn 临时属性修正作用于幸运时生效期内更高过期后回落() {
    // active_stat_modifiers 惰性到期判定对幸运同样成立——与力量/体质等
    // 其余六项走同一条 derive_stats 过滤规则（expires_at.0 > now.0）。
    // 生效期内（now < expires_at）有效幸运必须包含这条临时修正，过期
    // 后（now >= expires_at）必须精确回落到裸基础值，不是继续沿用
    // 过期前的旧值。
    // Arrange
    let mut interner = Interner::new();
    let blessing =
        interner.intern(NamespacedId::parse("lostland:blessing_of_fortune").expect("合法标识符"));
    let modifiers = BTreeMap::from([(
        AttributeKind::Luck,
        BTreeMap::from([(
            blessing,
            ActiveStatModifier {
                delta: 15,
                expires_at: Tick(100),
            },
        )]),
    )]);
    let no_items = FakeItems {
        items: BTreeMap::new(),
    };

    // Act
    let during = derive_stats(
        BaseStats::BASELINE,
        &modifiers,
        &BTreeMap::new(),
        &no_items,
        Tick(50),
    );
    let after = derive_stats(
        BaseStats::BASELINE,
        &modifiers,
        &BTreeMap::new(),
        &no_items,
        Tick(100),
    );

    // Assert
    assert_eq!(during.attribute(AttributeKind::Luck), 15);
    assert_eq!(after.attribute(AttributeKind::Luck), 0);
}

#[test]
fn 装备幸运戒指后暴击率真的更高() {
    // 最终验收——频率断言，不是单次结果（同 `ll_sim::resolve` 测试模块
    // `幸运更高的角色暴击命中频率更高` 的既有纪律：幸运只改变判定的
    // 概率形状，单次攻击测不出这条效果）。两个攻击者基础幸运恒为 0
    // （BaseStats::BASELINE），唯一差异是其中一个装备了幸运戒指
    // （+20 幸运 → 100‰ = 10% 暴击率），另一个不装备（0% 暴击率）。
    //
    // 手工验证过这条会红：把 `resolve_attack` 里
    // `let effective_luck = attacker_derived.attribute(AttributeKind::Luck);`
    // 改回读裸 `attacker.stats.luck`（本批次改动前，`resolve_attack`
    // 唯一读取幸运的地方是 `Agent.luck` 字段本身），两个攻击者的
    // `stats.luck` 都是 0（`BaseStats::BASELINE`），戒指的 +20 加成
    // 完全不参与暴击判定，`ringed_crits`/`unringed_crits` 变得相等
    // （都精确为 0）——完整记录见任务报告「第 1、3 条怎么变红」一节。
    // Arrange
    let trials = 3_000i64;
    let baseline_damage = damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);
    let (ring, items) = luck_ring_item();

    let mut ringed_world = test_world();
    let ringed_attacker = spawn_agent(
        &mut ringed_world,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::from([(EquipSlot::RING_L, ItemStack::new(ring, 1))]),
        BTreeMap::new(),
    );
    let ringed_victim = spawn_agent(
        &mut ringed_world,
        1_000_000,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );

    let mut unringed_world = test_world();
    let unringed_attacker = spawn_agent(
        &mut unringed_world,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    let unringed_victim = spawn_agent(
        &mut unringed_world,
        1_000_000,
        Vec::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );

    // Act：只挪动世界时钟取得不同的随机流，不真正 apply 任何效果——
    // 每次试验都在同一份满血目标上独立重打一次，理由同
    // `ll_sim::resolve` 测试模块「幸运更高的角色暴击命中频率更高」。
    let mut ringed_crits = 0i64;
    let mut unringed_crits = 0i64;
    for tick in 0..trials {
        ringed_world.clock = Tick(tick);
        let ringed_effects = resolve_with_skills_traits_pools_and_items(
            &ringed_world,
            &Intent::Attack {
                actor: ringed_attacker,
                target: ringed_victim,
            },
            &ll_sim::skill::NoSkills,
            &ll_sim::traits::NoTraitGrants,
            &ll_sim::traits::NoTraits,
            &ll_sim::resource_pool::NoResourcePools,
            &items,
        );
        if ringed_effects.iter().any(
            |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > baseline_damage),
        ) {
            ringed_crits += 1;
        }

        unringed_world.clock = Tick(tick);
        let unringed_effects = resolve(
            &unringed_world,
            &Intent::Attack {
                actor: unringed_attacker,
                target: unringed_victim,
            },
        );
        if unringed_effects.iter().any(
            |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > baseline_damage),
        ) {
            unringed_crits += 1;
        }
    }

    // Assert：戴戒指一侧的暴击次数应明显多于不戴的一侧——0 幸运恒为
    // 0% 暴击率（见 `crate_chance_permille` 文档「没有独立的『基础暴击
    // 率』常量」一节），unringed_crits 理应精确为 0；用一个较大的余量
    // （100）而不是直接比较 `> 0`，与既有同类频率测试的判据风格一致。
    assert!(ringed_crits > unringed_crits + 100);
}

/// 造一个占位实体，站在 `(5, 5)`，`race`/装备由调用方直接给出——理由
/// 同 `ll_sim::resolve` 测试模块的 `spawn_agent_with_luck_and_race`：
/// 偷袭判定测试需要种族索引与授予偷袭天赋的 [`TraitGrantSource`] 测试
/// 替身用**同一个** `ContentIndex`，若各自在互不相干的 `Interner` 里
/// 各 intern 一次，两边算出的数值不保证相等，因此不复用本文件已有的
/// `spawn_agent`（它在函数体内部临时 intern 一份种族，调用方拿不到那
/// 个索引）。
fn spawn_agent_with_race_and_equipment(
    world: &mut WorldState,
    race: ContentIndex,
    health: i32,
    equipment: BTreeMap<EquipSlot, ItemStack>,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory: Vec::new(),
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
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
        home: None,
    })
}

/// 一个只认识固定种族索引的测试用天赋授予来源，专供本文件的偷袭判定
/// 测试使用——形状同 `ll_sim::resolve` 测试模块的 `FixedSneakRaceGrant`
/// （不跨文件复用私有测试类型，两个 crate 测试目标本就各自独立编译）。
struct FixedSneakRaceGrant {
    race: ContentIndex,
    trait_id: ContentIndex,
}

impl TraitGrantSource for FixedSneakRaceGrant {
    fn granted_traits(&self, owner: ContentIndex) -> Vec<TraitGrant> {
        if owner == self.race {
            vec![TraitGrant {
                trait_id: self.trait_id,
                unlock_level: 1,
            }]
        } else {
            Vec::new()
        }
    }
}

/// 固定把 `trait_id` 映射到一条声明 [`RuleModifier::SneakAttack`] 的
/// `TraitRule`——供本文件的偷袭判定测试使用。
struct FixedSneakAttackTrait {
    trait_id: ContentIndex,
    sneak_modifier: i32,
    extra_damage: i32,
}

impl TraitCatalog for FixedSneakAttackTrait {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
        if trait_id != self.trait_id {
            return None;
        }
        Some(TraitRule {
            granted_skills: Vec::new(),
            granted_resource_pools: Vec::new(),
            rule_modifiers: vec![TypedRuleModifier {
                modifier_type: None,
                modifier: RuleModifier::SneakAttack {
                    sneak_modifier: self.sneak_modifier,
                    extra_damage: self.extra_damage,
                },
            }],
        })
    }
}

#[test]
fn 装备幸运戒指后偷袭触发频率真的更高() {
    // 频率断言，不是单次结果，理由同「装备幸运戒指后暴击率真的更高」
    // ——本条是偷袭判定这一侧的对应验收：两个攻击者裸 `BaseStats.luck`
    // 恒为 0（`BaseStats::BASELINE`），唯一差异是其中一个装备了幸运
    // 戒指（+20 有效幸运 → 偷袭判定净差 +20 → 91.42% 触发率），另一个
    // 不装备（净差 0 → 48.62%）。天赋自己那一路的修正取 0，好让这条
    // 测试只观察幸运那一路的贡献，理由同 `ll_sim::resolve` 测试模块
    // 「有效幸运更高的攻击者偷袭触发频率更高」。
    // `extra_damage` 取得远大于暴击单独能放大的上限
    // （基准伤害 10，暴击最多放大到 15），`sneak_threshold` 因此只可能
    // 被「偷袭真的触发」跨过，见 `ll_sim::resolve` 测试模块「有效幸运
    // 更高的攻击者偷袭触发频率更高」同一条阈值设计。
    //
    // 手工验证过这条会红：把 `resolve_attack` 里
    // `sneak_attacker_modifier(effective_luck, ..)` 的第一个实参改成读裸
    // `attacker.stats.luck`（模拟"偷袭判定沿用了幸运并入
    // `AttributeKind` 之前的写法"），两个攻击者的 `stats.luck` 都是 0
    // （`BaseStats::BASELINE`），戒指的 +20 加成完全不参与偷袭判定，
    // `ringed_sneaks`/`unringed_sneaks` 变得相等——完整记录见任务报告
    // 「第 2 条怎么变红」一节。
    // Arrange
    let trials = 3_000i64;
    let per_point = 0;
    let extra_damage = 1_000;
    let baseline_damage = damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);
    let sneak_threshold = baseline_damage + 100;
    let (ring, items) = luck_ring_item();

    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:rogue").expect("合法标识符"));
    let trait_id =
        interner.intern(NamespacedId::parse("lostland:sneak_attack").expect("合法标识符"));
    let race_traits = FixedSneakRaceGrant { race, trait_id };
    let traits = FixedSneakAttackTrait {
        trait_id,
        sneak_modifier: per_point,
        extra_damage,
    };

    let mut ringed_world = test_world();
    let ringed_attacker = spawn_agent_with_race_and_equipment(
        &mut ringed_world,
        race,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(EquipSlot::RING_L, ItemStack::new(ring, 1))]),
    );
    let ringed_victim =
        spawn_agent_with_race_and_equipment(&mut ringed_world, race, 1_000_000, BTreeMap::new());

    let mut unringed_world = test_world();
    let unringed_attacker = spawn_agent_with_race_and_equipment(
        &mut unringed_world,
        race,
        Agent::STARTING_HEALTH,
        BTreeMap::new(),
    );
    let unringed_victim =
        spawn_agent_with_race_and_equipment(&mut unringed_world, race, 1_000_000, BTreeMap::new());

    // Act：只挪动世界时钟取得不同的随机流，理由同「装备幸运戒指后暴击
    // 率真的更高」。
    let mut ringed_sneaks = 0i64;
    let mut unringed_sneaks = 0i64;
    for tick in 0..trials {
        ringed_world.clock = Tick(tick);
        let ringed_effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
            &ringed_world,
            &Intent::Attack {
                actor: ringed_attacker,
                target: ringed_victim,
            },
            &NoSkills,
            &race_traits,
            &traits,
            &NoResourcePools,
            &items,
            &NoFormulas,
            &NoDamageCategories,
        );
        if ringed_effects.iter().any(
            |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > sneak_threshold),
        ) {
            ringed_sneaks += 1;
        }

        unringed_world.clock = Tick(tick);
        let unringed_effects =
            resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
                &unringed_world,
                &Intent::Attack {
                    actor: unringed_attacker,
                    target: unringed_victim,
                },
                &NoSkills,
                &race_traits,
                &traits,
                &NoResourcePools,
                &items,
                &NoFormulas,
                &NoDamageCategories,
            );
        if unringed_effects.iter().any(
            |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > sneak_threshold),
        ) {
            unringed_sneaks += 1;
        }
    }

    // Assert：戴戒指一侧的触发次数应明显多于不戴的一侧（3000 轮上
    // 期望值相差约 1284 次）；用一个较大的余量（100）而不是直接比较
    // `>`，与既有同类频率测试的判据风格一致。
    assert!(ringed_sneaks > unringed_sneaks + 100);
    // 两端都不封顶：戴戒指也不是必定触发，不戴也不是必定不触发。
    assert!(ringed_sneaks < trials, "顶格修正也不该次次触发");
    assert!(unringed_sneaks > 0, "零有效幸运也不该一次都触发不了");
}
