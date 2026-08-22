//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! 武器引用与穿透接线批次（P6 第六批）的三条硬要求真的在真实注册的
//! 内容上成立——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有
//! 真实 mod 脚本为证」，本文件是这一批的那份证据，不能靠
//! `crates/ll-sim/src/resolve.rs`/`crates/ll-mod/src/item.rs` 里的单元
//! 测试自证。
//!
//! 与 `crates/ll-mod/tests/example_mod_combat.rs` 同一个理由独立成
//! 文件、同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」手法，
//! 见 `example_mod_resource_pools.rs` 模块文档。覆盖项目任务书要求的
//! 三条端到端：
//!
//! 1. 装备一件真实注册的武器（战锤）攻击致死 → `KillCause::Melee` 的
//!    `weapon` 字段真的指向战锤的 `ContentIndex`，不是 `None`。
//! 2. 攻击方主手已装备的武器（若带耐久）→ 打出一下攻击后耐久真的
//!    减少。
//! 3. 防御方已装备的护甲（木盾，带耐久）→ 挨打后耐久不再减少，证明
//!    P6 第五批「被击中掉防御方装备耐久」的旧规则已经被收窄。
//!
//! 另外用真实注册的战锤穿透值（`register-item-penetration`）验证
//! `resolve_attack` 真的把它传给了 `damage_after_defense`——穿透此前
//! （P6 第四批到第五批）没有任何数据源，恒为 `Penetration::NONE`。
//!
//! 「徒手攻击时 `weapon` 恒为 `None`」这条反例不在本文件重复：
//! `crates/ll-sim/src/resolve.rs` 的
//! `近战攻击致死已具名目标后历史事件记录着近战死因` 单元测试已经
//! 用一个完全不装备任何物品的攻击者验证过这一点（该测试早于本批次
//! 就存在且持续通过，见其断言 `KillCause::Melee { weapon: None }`）,
//! 与本文件互补，不必在两处各写一份同样的反例。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId, WorldId};
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
use ll_world::history::{HistoricalEventKind, KillCause};
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_items.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// `mods/example_mod/gameplay.scm` 给战锤声明的力量加成——与该文件
/// `(register-item-stat-bonus "examplemod:war_hammer" "strength" 6)`
/// 保持同步，理由同 `example_mod_combat.rs::WAR_HAMMER_STRENGTH_BONUS`。
const WAR_HAMMER_STRENGTH_BONUS: i32 = 6;

/// `mods/example_mod/gameplay.scm` 给战锤声明的穿透——与该文件
/// `(register-item-penetration "examplemod:war_hammer" 3 100)` 保持
/// 同步，断言里复用这个常量，不重复写字面量。
const WAR_HAMMER_PENETRATION: Penetration = Penetration {
    flat: 3,
    permille: 100,
};

/// `mods/example_mod/gameplay.scm` 给战锤声明的耐久上限——与该文件
/// `(register-item "examplemod:war_hammer" ... 150)` 保持同步。
const WAR_HAMMER_MAX_DURABILITY: i32 = 150;

/// `mods/example_mod/gameplay.scm` 给木盾声明的护甲加成/耐久上限——
/// 与该文件的 `register-item-stat-bonus`/`register-item` 调用保持同步。
const WOODEN_SHIELD_ARMOR_BONUS: i32 = 8;
const WOODEN_SHIELD_MAX_DURABILITY: i32 = 80;

/// 装载真实 `mods/` 目录一次，返回全部断言需要的物品表与已经解析好的
/// 索引——理由同 `example_mod_combat.rs::RealModsHandle`。
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
    let mut formula = ll_mod::formula::FormulaTable::new();

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

/// 造一个占位实体，站在 `(5, 5)`，健康值/装备栏由调用方给出——理由同
/// `example_mod_combat.rs::spawn_agent`。`remembered` 控制这个实体是否
/// "具名"（`remembered_id`）：只有具名实体被击杀时才会在
/// `world.history` 里留下 `KillRecord`（见
/// `ll_sim::resolve` 模块「未具名目标被击杀时不产生历史事件记录」
/// 一节既有纪律），本文件验证击杀记录里的 `weapon` 字段时需要一个
/// 具名受害者才查得到记录。
fn spawn_agent(
    world: &mut WorldState,
    health: i32,
    equipment: BTreeMap<EquipSlot, ItemStack>,
    remembered: bool,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    let mut world_id_counter = 0u32;
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
        remembered_id: remembered.then(|| WorldId::next(&mut world_id_counter)),
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
fn 装备真实注册的战锤致死后击杀记录的武器字段指向战锤定义() {
    // 手工验证过这条会红：把本函数体里 `resolve_attack` 结算出的
    // `weapon_def` 改回恒 `None`（P6 第六批之前的行为），本测试立即
    // 从 `Some(handle.war_hammer_id)` 变成 `None` 而失败——武器引用
    // 与「徒手」在类型上现在真正区分开，见 `resolve_attack` 文档
    // 「武器引用」一节。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::with_durability(handle.war_hammer_id, 1, WAR_HAMMER_MAX_DURABILITY),
        )]),
        false,
    );
    // 生命值 1：BASELINE 力量 + 战锤力量加成算出的攻击力必然远大于 1
    // （见 combat::damage_after_defense 的单元测试），一击必死。受害者
    // 必须"具名"（remembered = true）才会在 world.history 留下记录。
    let victim = spawn_agent(&mut world, 1, BTreeMap::new(), true);

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
    assert_eq!(world.history.len(), 1, "致死一击必须留下唯一一条历史事件");
    let HistoricalEventKind::Kill(record) = &world.history[0].kind;
    assert_eq!(
        record.cause,
        KillCause::Melee {
            weapon: Some(handle.war_hammer_id)
        }
    );
}

#[test]
fn 装备真实注册的战锤攻击护甲目标时伤害精确匹配穿透公式计算值() {
    // 端到端验证穿透真的接进了伤害结算：战锤的穿透（flat 3、千分比
    // 100）与木盾的护甲加成（+8）都来自 mods/example_mod/gameplay.scm
    // 真实注册的内容，期望伤害用 damage_after_defense 独立算出，不是
    // 从结算结果反推。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::with_durability(handle.war_hammer_id, 1, WAR_HAMMER_MAX_DURABILITY),
        )]),
        false,
    );
    let defender = spawn_agent(
        &mut world,
        1_000,
        BTreeMap::from([(
            EquipSlot::OFF_HAND,
            ItemStack::with_durability(handle.wooden_shield_id, 1, WOODEN_SHIELD_MAX_DURABILITY),
        )]),
        false,
    );
    let expected_damage = damage_after_defense(
        BaseStats::BASELINE.strength + WAR_HAMMER_STRENGTH_BONUS,
        WOODEN_SHIELD_ARMOR_BONUS,
        WAR_HAMMER_PENETRATION,
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: defender,
        },
        &handle.item,
    );

    // Assert
    let defender_after = world
        .actors
        .get(defender)
        .expect("生命值远高于伤害,不会死亡");
    assert_eq!(defender_after.health, 1_000 - expected_damage);
}

#[test]
fn 装备真实注册的战锤攻击后攻击方战锤的耐久真的减少() {
    // 手工验证过这条会红：把 resolve_attack 里那条
    // `AdjustEquipmentDurability` 效果的产出去掉（不改任何其它逻辑），
    // 本测试立即从 `Some(WAR_HAMMER_MAX_DURABILITY - 1)` 变成
    // `Some(WAR_HAMMER_MAX_DURABILITY)` 而失败。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::with_durability(handle.war_hammer_id, 1, WAR_HAMMER_MAX_DURABILITY),
        )]),
        false,
    );
    let victim = spawn_agent(&mut world, 1_000, BTreeMap::new(), false);

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
    let weapon = world
        .actors
        .get(attacker)
        .expect("攻击者仍然存活")
        .equipment
        .get(&EquipSlot::MAIN_HAND)
        .expect("战锤仍在装备栏里");
    assert_eq!(weapon.durability, Some(WAR_HAMMER_MAX_DURABILITY - 1));
}

#[test]
fn 装备真实注册的木盾的防御方挨打后木盾耐久不再减少() {
    // 与上一条测试成对，证明 P6 第五批「被击中掉防御方装备耐久」的
    // 旧规则已经被收窄——防御方即便穿着带耐久的木盾，挨打后耐久也
    // 保持原样，不像本批次之前那样掉到 `WOODEN_SHIELD_MAX_DURABILITY
    // - 1`。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(&mut world, Agent::STARTING_HEALTH, BTreeMap::new(), false);
    let defender = spawn_agent(
        &mut world,
        1_000,
        BTreeMap::from([(
            EquipSlot::OFF_HAND,
            ItemStack::with_durability(handle.wooden_shield_id, 1, WOODEN_SHIELD_MAX_DURABILITY),
        )]),
        false,
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Attack {
            actor: attacker,
            target: defender,
        },
        &handle.item,
    );

    // Assert
    let shield = world
        .actors
        .get(defender)
        .expect("生命值远高于伤害,不会死亡")
        .equipment
        .get(&EquipSlot::OFF_HAND)
        .expect("木盾仍在装备栏里");
    assert_eq!(shield.durability, Some(WOODEN_SHIELD_MAX_DURABILITY));
}
