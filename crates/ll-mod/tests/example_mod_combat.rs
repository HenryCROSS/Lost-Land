//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-item-stat-bonus` 这个新脚本 API 真的能被
//! `mods/example_mod/gameplay.scm` 调用，且注册出来的力量加成（战锤）/
//! 护甲加成（木盾）真的能走
//! `ll_sim::resolve::resolve_with_skills_traits_pools_and_items`/
//! `ll_sim::apply::apply` 端到端改变结算出的伤害——ADR 0018「玩法层
//! 内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，本文件是
//! P6 第四批（`derive_stats` 与装备属性接进战斗）的那份证据，不能靠
//! `crates/ll-sim/src/resolve.rs`/`crates/ll-mod/src/item.rs` 里的单元
//! 测试自证。
//!
//! 与 `crates/ll-mod/tests/example_mod_equipment.rs` 同一个理由独立成
//! 文件、同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」手法，
//! 见 `example_mod_resource_pools.rs` 模块文档。覆盖项目任务书要求的
//! 三条端到端：
//!
//! 1. 装备一件加力量的武器（战锤）→ 攻击伤害真的变高。
//! 2. 装备一件加护甲的防具（木盾）→ 受到的伤害真的变低（防御端第一次
//!    真的生效）。
//! 3. 卸下装备 → 加成真的消失（证明是派生不是一次性烘焙）。

use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::apply::apply;
use ll_sim::combat::{Penetration, damage_after_defense};
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::resolve::resolve_with_skills_traits_pools_and_items;
use ll_sim::skill::NoSkills;
use ll_sim::traits::{NoTraitGrants, NoTraits};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use std::collections::BTreeMap;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_items.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// `mods/example_mod/gameplay.scm` 给战锤声明的力量加成——与该文件
/// `(register-item-stat-bonus "examplemod:war_hammer" "strength" 6)`
/// 保持同步,断言里复用这个常量,不重复写字面量 `6`。
const WAR_HAMMER_STRENGTH_BONUS: i32 = 6;

/// 装载真实 `mods/` 目录一次，返回全部断言需要的物品表与已经解析好的
/// 索引——理由同 `example_mod_equipment.rs::RealModsHandle`。
struct RealModsHandle {
    item: ItemTable,
    war_hammer_id: ContentIndex,
    wooden_shield_id: ContentIndex,
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
        },
    );
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
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/gameplay.scm 注册"))
    };

    RealModsHandle {
        war_hammer_id: resolve("examplemod:war_hammer"),
        wooden_shield_id: resolve("examplemod:wooden_shield"),
        item,
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

/// 造一个占位实体，站在 `(5, 5)`，健康值/背包/装备栏初始内容由调用方
/// 给出——理由同 `example_mod_equipment.rs::spawn_agent`。
fn spawn_agent(
    world: &mut WorldState,
    health: i32,
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

/// 把 `intent` 结算并应用到 `world`——本文件全部测试共用的一步。
fn resolve_and_apply(world: &mut WorldState, intent: &Intent, items: &ItemTable) {
    let effects = resolve_with_skills_traits_pools_and_items(
        world,
        intent,
        &NoSkills,
        &NoTraitGrants,
        &NoTraits,
        &ll_sim::resource_pool::NoResourcePools,
        items,
    );
    for effect in &effects {
        apply(world, effect);
    }
}

#[test]
fn 真实注册的战锤装备后攻击伤害真的变高() {
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(handle.war_hammer_id, 1)],
        BTreeMap::new(),
    );
    let victim = spawn_agent(&mut world, 1_000, Vec::new(), BTreeMap::new());
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor: attacker,
            def: handle.war_hammer_id,
        },
        &handle.item,
    );
    let expected_damage = damage_after_defense(
        BaseStats::BASELINE.strength + WAR_HAMMER_STRENGTH_BONUS,
        0,
        Penetration::NONE,
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );

    // Assert
    let victim_after = world.actors.get(victim).expect("生命值远高于伤害,不会死亡");
    assert_eq!(victim_after.health, 1_000 - expected_damage);
}

#[test]
fn 真实注册的木盾装备后受到的伤害真的变低() {
    // Arrange：两个初始生命值相同的目标，一个装备木盾、一个不装备，
    // 承受同一个攻击者的同一次攻击。
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        Vec::new(),
        BTreeMap::new(),
    );
    let shielded = spawn_agent(
        &mut world,
        1_000,
        Vec::new(),
        BTreeMap::from([(
            EquipSlot::OFF_HAND,
            ItemStack::new(handle.wooden_shield_id, 1),
        )]),
    );
    let unshielded = spawn_agent(&mut world, 1_000, Vec::new(), BTreeMap::new());

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: shielded,
        },
        &handle.item,
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: unshielded,
        },
        &handle.item,
    );

    // Assert
    let shielded_damage = 1_000 - world.actors.get(shielded).expect("生命值远高于伤害").health;
    let unshielded_damage = 1_000
        - world
            .actors
            .get(unshielded)
            .expect("生命值远高于伤害")
            .health;
    assert!(shielded_damage < unshielded_damage);
}

#[test]
fn 卸下真实注册的战锤后攻击伤害真的回落() {
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        vec![ItemStack::new(handle.war_hammer_id, 1)],
        BTreeMap::new(),
    );
    let victim = spawn_agent(&mut world, 10_000, Vec::new(), BTreeMap::new());
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor: attacker,
            def: handle.war_hammer_id,
        },
        &handle.item,
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );
    let health_after_equipped_attack = world.actors.get(victim).unwrap().health;

    // Act：卸下战锤（同时占主手与副手，见 example_mod_equipment.rs），
    // 再攻击一次。
    resolve_and_apply(
        &mut world,
        &Intent::Unequip {
            actor: attacker,
            slot: EquipSlot::MAIN_HAND,
        },
        &handle.item,
    );
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &handle.item,
    );

    // Assert：第二下伤害精确等于裸基础力量算出的伤害。
    let unequipped_damage = health_after_equipped_attack - world.actors.get(victim).unwrap().health;
    let baseline_damage = damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);
    assert_eq!(unequipped_damage, baseline_damage);
}
