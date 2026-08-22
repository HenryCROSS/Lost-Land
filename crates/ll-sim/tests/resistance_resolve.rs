//! `RuleModifier::Resistance` 接入伤害管线的集成测试（伤害类别/抗性
//! 接线批次）——`knowledge/design/damage-formula-mod-api.md` 二十节
//! 「抗性在减伤链路的哪一步：减伤之后，乘数形式」与
//! `knowledge/design/trait-system.md` 三节③「抗性：挂载点已经现成」
//! 在真实 `resolve_attack` 全链路上的验收。
//!
//! 三条硬要求各自对应一条测试：
//! 1. 有火抗的角色受到火伤时，伤害真的更低。
//! 2. 有火抗的角色受到物理伤时，伤害不变——证明抗性认类别，不是无脑
//!    减伤。
//! 3. 抗性在减伤之后——高防御 + 高抗性的角色，断言结果符合「先减伤
//!    再乘抗性」的顺序，而不是反过来。
//!
//! 攻击力全程用一条恒定的 `Const` 公式（不掷骰），排除随机性；幸运恒
//! 为 `BaseStats::BASELINE.luck == 0`，暴击率恒为零（`combat::零幸运暴击率为零`），
//! 排除暴击的偶然放大——两条既有确定性纪律叠在一起，保证下面三条测试
//! 的期望值可以直接手算复现，不依赖任何具体的 `DetRng` 种子。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::combat::{Penetration, damage_after_defense};
use ll_sim::damage_category::{DamageCategoryCatalog, NoDamageCategories};
use ll_sim::formula::{DamageFormulaCatalog, FormulaDef, FormulaOp, FormulaOperand};
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemCatalog, ItemRule, ItemStack, StatBonus, StatTarget};
use ll_sim::resolve::resolve_with_skills_traits_pools_items_formulas_and_damage_categories;
use ll_sim::skill::NoSkills;
use ll_sim::traits::{RuleModifier, TraitCatalog, TraitGrant, TraitGrantSource, TraitRule};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 攻击力恒为 `value` 的确定性公式目录——不掷骰,排除随机性,理由见
/// 模块文档。
struct ConstFormula {
    value: i64,
}

impl DamageFormulaCatalog for ConstFormula {
    fn formula_for(&self, _explicit: Option<ContentIndex>) -> FormulaDef {
        FormulaDef {
            id: ContentIndex::default(),
            instructions: vec![FormulaOp::Ref(FormulaOperand::Const(self.value))],
            needs_rng: false,
        }
    }
}

/// 一个只认识固定物品索引的测试目录。
struct FakeItems {
    items: BTreeMap<ContentIndex, ItemRule>,
}

impl ItemCatalog for FakeItems {
    fn item(&self, item: ContentIndex) -> Option<ItemRule> {
        self.items.get(&item).cloned()
    }
}

/// 一个只认识固定种族索引的测试用天赋授予来源，理由同
/// `trait_resolve.rs::FakeRaceTraits`。
struct FakeRaceTraits {
    race: ContentIndex,
    grants: Vec<TraitGrant>,
}

impl TraitGrantSource for FakeRaceTraits {
    fn granted_traits(&self, owner: ContentIndex) -> Vec<TraitGrant> {
        if owner == self.race {
            self.grants.clone()
        } else {
            Vec::new()
        }
    }
}

/// 一个只认识固定天赋索引的测试目录。
struct FakeTraits {
    traits: BTreeMap<ContentIndex, TraitRule>,
}

impl TraitCatalog for FakeTraits {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
        self.traits.get(&trait_id).cloned()
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

/// 造一个占位实体，站在 `(5, 5)`，理由同 `trait_resolve.rs::spawn_agent`
/// ——本文件额外暴露 `equipment`，供防御方装备提供护甲的测试使用。
fn spawn_agent(
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

/// 一件不提供穿透、显式声明伤害类别的武器——`formula` 由调用方通过
/// [`ConstFormula`] 控制，这里的 `damage_formula` 字段本身留空
/// （`ConstFormula::formula_for` 不理会 `explicit` 参数，见其文档）。
fn weapon_rule(damage_category: ContentIndex) -> ItemRule {
    ItemRule {
        stack_limit: 1,
        equip_mask: EquipSlot::MAIN_HAND.mask(),
        stat_bonuses: Vec::new(),
        use_effect: None,
        penetration: Penetration::NONE,
        damage_formula: None,
        damage_category: Some(damage_category),
        rule_modifiers: Vec::new(),
    }
}

/// 一件只提供护甲加成的防具。
fn armor_rule(amount: i32) -> ItemRule {
    ItemRule {
        stack_limit: 1,
        equip_mask: EquipSlot::OFF_HAND.mask(),
        stat_bonuses: vec![StatBonus {
            target: StatTarget::Armor,
            amount,
        }],
        use_effect: None,
        penetration: Penetration::NONE,
        damage_formula: None,
        damage_category: None,
        rule_modifiers: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn attack_and_apply(
    world: &mut WorldState,
    attacker: EntityId,
    defender: EntityId,
    items: &dyn ItemCatalog,
    formulas: &dyn DamageFormulaCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    damage_categories: &dyn DamageCategoryCatalog,
) {
    let effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
        world,
        &Intent::Attack {
            actor: attacker,
            target: defender,
        },
        &NoSkills,
        race_traits,
        traits,
        &ll_sim::resource_pool::NoResourcePools,
        items,
        formulas,
        damage_categories,
    );
    for effect in &effects {
        ll_sim::apply::apply(world, effect);
    }
}

#[test]
fn 有火抗的角色受到火伤时伤害真的更低() {
    // Arrange：攻击力恒 1000，无防御无穿透——不抗性时的裸伤害就是
    // `damage_after_defense(1000, 0, NONE)`；防御方声明了对火（500‰半
    // 伤）的抗性，武器造成的正是火伤。
    let mut world = test_world();
    let mut interner = Interner::new();
    let attacker_race = interner.intern(NamespacedId::parse("lostland:human").unwrap());
    let defender_race = interner.intern(NamespacedId::parse("lostland:fire_elemental").unwrap());
    let weapon = interner.intern(NamespacedId::parse("lostland:flame_sword").unwrap());
    let fire = interner.intern(NamespacedId::parse("lostland:fire").unwrap());
    let fire_resistance_trait =
        interner.intern(NamespacedId::parse("lostland:fire_resistance").unwrap());

    let attacker = spawn_agent(
        &mut world,
        attacker_race,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(weapon, 1))]),
    );
    let defender = spawn_agent(&mut world, defender_race, 1_000, BTreeMap::new());

    let items = FakeItems {
        items: BTreeMap::from([(weapon, weapon_rule(fire))]),
    };
    let race_traits = FakeRaceTraits {
        race: defender_race,
        grants: vec![TraitGrant {
            trait_id: fire_resistance_trait,
            unlock_level: 1,
        }],
    };
    let traits = FakeTraits {
        traits: BTreeMap::from([(
            fire_resistance_trait,
            TraitRule {
                granted_skills: Vec::new(),
                granted_resource_pools: Vec::new(),
                rule_modifiers: vec![RuleModifier::Resistance {
                    damage_category: fire,
                    multiplier_permille: 500,
                }],
            },
        )]),
    };
    let formulas = ConstFormula { value: 1000 };
    let unmitigated_no_resistance = damage_after_defense(1000, 0, Penetration::NONE);

    // Act
    attack_and_apply(
        &mut world,
        attacker,
        defender,
        &items,
        &formulas,
        &race_traits,
        &traits,
        &NoDamageCategories,
    );

    // Assert：真实受到的伤害严格小于"没有抗性"时的裸伤害。
    let defender_after = world.actors.get(defender).expect("防御方未死亡");
    let actual_damage = 1_000 - defender_after.health;
    assert!(
        actual_damage < unmitigated_no_resistance,
        "有火抗时受到的火伤（{actual_damage}）应当严格低于无抗性裸伤害（{unmitigated_no_resistance}）"
    );
    assert_eq!(actual_damage, unmitigated_no_resistance * 500 / 1000);
}

#[test]
fn 有火抗的角色受到物理伤害时伤害不变() {
    // Arrange：与上一条测试完全相同的防御方（同一份火抗声明），唯一
    // 差异是武器这次造成的是物理伤害——证明抗性按类别匹配，不是"只要
    // 声明了抗性就无脑打折"。
    let mut world = test_world();
    let mut interner = Interner::new();
    let attacker_race = interner.intern(NamespacedId::parse("lostland:human").unwrap());
    let defender_race = interner.intern(NamespacedId::parse("lostland:fire_elemental").unwrap());
    let weapon = interner.intern(NamespacedId::parse("lostland:iron_sword").unwrap());
    let fire = interner.intern(NamespacedId::parse("lostland:fire").unwrap());
    let physical = interner.intern(NamespacedId::parse("lostland:physical").unwrap());
    let fire_resistance_trait =
        interner.intern(NamespacedId::parse("lostland:fire_resistance").unwrap());

    let attacker = spawn_agent(
        &mut world,
        attacker_race,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(weapon, 1))]),
    );
    // 生命值取 2_000（而非另外两条测试用的 1_000）——本测试的物理伤害
    // 不打折扣，精确等于攻击力 1000，与生命值 1_000 相等会触发致死
    // 分支（Effect::Kill 移除实体），生命值必须严格高于伤害才能在
    // 结算后仍查得到防御方。
    let defender = spawn_agent(&mut world, defender_race, 2_000, BTreeMap::new());

    let items = FakeItems {
        items: BTreeMap::from([(weapon, weapon_rule(physical))]),
    };
    let race_traits = FakeRaceTraits {
        race: defender_race,
        grants: vec![TraitGrant {
            trait_id: fire_resistance_trait,
            unlock_level: 1,
        }],
    };
    let traits = FakeTraits {
        traits: BTreeMap::from([(
            fire_resistance_trait,
            TraitRule {
                granted_skills: Vec::new(),
                granted_resource_pools: Vec::new(),
                rule_modifiers: vec![RuleModifier::Resistance {
                    damage_category: fire,
                    multiplier_permille: 500,
                }],
            },
        )]),
    };
    let formulas = ConstFormula { value: 1000 };
    let expected_damage = damage_after_defense(1000, 0, Penetration::NONE);

    // Act
    attack_and_apply(
        &mut world,
        attacker,
        defender,
        &items,
        &formulas,
        &race_traits,
        &traits,
        &NoDamageCategories,
    );

    // Assert：物理伤害精确等于裸伤害，火抗完全不影响它。
    let defender_after = world.actors.get(defender).expect("防御方未死亡");
    assert_eq!(2_000 - defender_after.health, expected_damage);
}

#[test]
fn 抗性在减伤之后而不是减伤之前生效() {
    // Arrange：攻击力恒 1000，防御方装备 +100 护甲（中等防御，既不是
    // 零也不足以触发 10% 下限——保证这条测试验的是「减伤的比例项」，
    // 不是下限那条独立的安全网），并声明 500‰ 的火抗。
    //
    // 手算两种顺序的结果：
    //   正确顺序（先减伤再乘抗性）：
    //     减后伤害 = damage_after_defense(1000, 100, NONE) = 818
    //     最终伤害 = 818 * 500 / 1000 = 409
    //   错误顺序（先把攻击力按抗性打对折,再送进减伤链路——即抗性介入
    //   防御计算之前）：
    //     打折攻击力 = 1000 * 500 / 1000 = 500
    //     最终伤害 = damage_after_defense(500, 100, NONE) = 363
    // 两者不同（409 ≠ 363），断言真实结果落在正确顺序那一侧，直接证明
    // 抗性没有被错误地接在减伤链路前面。
    let mut world = test_world();
    let mut interner = Interner::new();
    let attacker_race = interner.intern(NamespacedId::parse("lostland:human").unwrap());
    let defender_race =
        interner.intern(NamespacedId::parse("lostland:armored_salamander").unwrap());
    let weapon = interner.intern(NamespacedId::parse("lostland:flame_sword").unwrap());
    let armor = interner.intern(NamespacedId::parse("lostland:scale_mail").unwrap());
    let fire = interner.intern(NamespacedId::parse("lostland:fire").unwrap());
    let fire_resistance_trait =
        interner.intern(NamespacedId::parse("lostland:fire_resistance").unwrap());

    let attacker = spawn_agent(
        &mut world,
        attacker_race,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(weapon, 1))]),
    );
    let defender = spawn_agent(
        &mut world,
        defender_race,
        1_000,
        BTreeMap::from([(EquipSlot::OFF_HAND, ItemStack::new(armor, 1))]),
    );

    let items = FakeItems {
        items: BTreeMap::from([(weapon, weapon_rule(fire)), (armor, armor_rule(100))]),
    };
    let race_traits = FakeRaceTraits {
        race: defender_race,
        grants: vec![TraitGrant {
            trait_id: fire_resistance_trait,
            unlock_level: 1,
        }],
    };
    let traits = FakeTraits {
        traits: BTreeMap::from([(
            fire_resistance_trait,
            TraitRule {
                granted_skills: Vec::new(),
                granted_resource_pools: Vec::new(),
                rule_modifiers: vec![RuleModifier::Resistance {
                    damage_category: fire,
                    multiplier_permille: 500,
                }],
            },
        )]),
    };
    let formulas = ConstFormula { value: 1000 };

    let correct_order_expected = damage_after_defense(1000, 100, Penetration::NONE) * 500 / 1000;
    let wrong_order_would_be = damage_after_defense(500, 100, Penetration::NONE);
    assert_ne!(
        correct_order_expected, wrong_order_would_be,
        "测试前提：两种顺序必须给出不同的期望值，否则本测试无法区分对错"
    );

    // Act
    attack_and_apply(
        &mut world,
        attacker,
        defender,
        &items,
        &formulas,
        &race_traits,
        &traits,
        &NoDamageCategories,
    );

    // Assert
    let defender_after = world.actors.get(defender).expect("防御方未死亡");
    let actual_damage = 1_000 - defender_after.health;
    assert_eq!(actual_damage, correct_order_expected);
    assert_eq!(actual_damage, 409);
}
