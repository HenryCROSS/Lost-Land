//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-item-equip-mask` 这个新脚本 API 真的能被
//! `mods/example_mod/gameplay.scm` 调用，且注册出来的双手武器（战锤，
//! 同时占 `main-hand`/`off-hand`）与单手装备（木盾，只占
//! `off-hand`）真的能走
//! `ll_sim::resolve::resolve_with_skills_traits_pools_and_items`/
//! `ll_sim::apply::apply` 端到端算出占位冲突的正确结果——ADR 0018
//! 「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，本
//! 文件是装备栏位批次（P6 第三批）的那份证据，不能靠
//! `crates/ll-sim/src/resolve.rs`/`crates/ll-mod/src/item.rs` 里的单元
//! 测试自证。
//!
//! 与 `crates/ll-mod/tests/example_mod_items.rs`/`example_mod_resource_pools.rs`
//! 同一个理由独立成文件、同一套「装载整个 `mods/` 目录，不是只挑
//! `example_mod`」手法，见 `example_mod_resource_pools.rs` 模块文档。

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

/// 装载真实 `mods/` 目录一次，返回全部断言需要的物品表与已经解析好的
/// 索引——理由同 `example_mod_items.rs::RealModsHandle`。
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
    let mut weapon_category = ll_mod::weapon_category::WeaponCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut damage_category = ll_mod::damage_category::DamageCategoryTable::new();
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
            weapon_category: &mut weapon_category,
            damage_category: &mut damage_category,
            space_profile: &mut space_profile,
            weather: &mut weather_table,
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

/// 造一个占位实体，站在 `(5, 5)`，背包/装备栏初始内容由调用方给出——
/// 理由同 `example_mod_resource_pools.rs::spawn_agent`。
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
fn 真实注册的战锤装备掩码同时覆盖主手与副手() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .item
        .get(handle.war_hammer_id)
        .expect("战锤应当已被真实注册");

    // Assert
    assert!(view.equip_mask.contains_slot(EquipSlot::MAIN_HAND));
    assert!(view.equip_mask.contains_slot(EquipSlot::OFF_HAND));
}

#[test]
fn 真实注册的木盾装备掩码只覆盖副手() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .item
        .get(handle.wooden_shield_id)
        .expect("木盾应当已被真实注册");

    // Assert
    assert!(!view.equip_mask.contains_slot(EquipSlot::MAIN_HAND));
    assert!(view.equip_mask.contains_slot(EquipSlot::OFF_HAND));
}

#[test]
fn 装备战锤后从副手请求卸下也能定位到同一把武器() {
    // 占位规则的正例：双手武器只存一份，锚点在主手，但从副手（非
    // 锚点槽位）发起卸下请求也必须成功——证明"两个槽位都被占用"不是
    // 只在锚点主手那一侧成立。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        vec![ItemStack::new(handle.war_hammer_id, 1)],
        BTreeMap::new(),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor,
            def: handle.war_hammer_id,
        },
        &handle.item,
    );

    // Act：请求卸下副手（不是锚点主手）。
    resolve_and_apply(
        &mut world,
        &Intent::Unequip {
            actor,
            slot: EquipSlot::OFF_HAND,
        },
        &handle.item,
    );

    // Assert：装备栏清空，战锤回到背包——证明副手请求确实定位到了这把
    // 锚点在主手的双手武器。
    let agent = world.actors.get(actor).unwrap();
    assert!(agent.equipment.is_empty());
    assert_eq!(
        agent.inventory,
        vec![ItemStack::new(handle.war_hammer_id, 1)]
    );
}

#[test]
fn 装备战锤后副手已被占用时装备木盾会连带卸下战锤() {
    // 占位规则的正例（对应「主手和副手都不能再装别的」）：先装备战锤
    // （占满两手），再尝试装备只占副手的木盾——木盾与战锤的掩码在副手
    // 相交，战锤应当被自动卸下、木盾顶替上去，而不是两者同时挂在装备
    // 栏（那会是一处真实的状态错误：副手同时被两件不同的物品声称
    // 占用）。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        vec![
            ItemStack::new(handle.war_hammer_id, 1),
            ItemStack::new(handle.wooden_shield_id, 1),
        ],
        BTreeMap::new(),
    );
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor,
            def: handle.war_hammer_id,
        },
        &handle.item,
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor,
            def: handle.wooden_shield_id,
        },
        &handle.item,
    );

    // Assert：装备栏只剩木盾（锚点副手），战锤回到背包。
    let agent = world.actors.get(actor).unwrap();
    assert_eq!(
        agent.equipment,
        BTreeMap::from([(
            EquipSlot::OFF_HAND,
            ItemStack::new(handle.wooden_shield_id, 1)
        )])
    );
    assert_eq!(
        agent.inventory,
        vec![ItemStack::new(handle.war_hammer_id, 1)]
    );
}

#[test]
fn 副手单独装备木盾时装备战锤会连带卸下木盾() {
    // 占位规则的反例（反向）：这次从「副手已经单独被一件单槽位物品
    // 占用」这个起点出发，装备一件横跨两槽的战锤——证明冲突检查不是
    // 只在"新物品是单槽、已装备物品是多槽"这一个方向上生效，反过来
    // （已装备物品是单槽、新物品是多槽）同样能正确识别冲突并卸下木盾。
    // 手工验证过这条会红：把 resolve_equip 里扫描 agent.equipment 找
    // 冲突的那个 for 循环整体去掉后重跑本测试，装备栏会同时出现木盾
    // （副手）与战锤（主手，锚点），断言从通过变为失败——完整记录见
    // 任务报告「占位冲突两条测试」一节。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_agent(
        &mut world,
        vec![ItemStack::new(handle.war_hammer_id, 1)],
        BTreeMap::from([(
            EquipSlot::OFF_HAND,
            ItemStack::new(handle.wooden_shield_id, 1),
        )]),
    );

    // Act
    resolve_and_apply(
        &mut world,
        &Intent::Equip {
            actor,
            def: handle.war_hammer_id,
        },
        &handle.item,
    );

    // Assert：装备栏只剩战锤（锚点主手，覆盖副手），木盾回到背包。
    let agent = world.actors.get(actor).unwrap();
    assert_eq!(
        agent.equipment,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::new(handle.war_hammer_id, 1)
        )])
    );
    assert_eq!(
        agent.inventory,
        vec![ItemStack::new(handle.wooden_shield_id, 1)]
    );
}
