//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! 盗贼偷袭接线批次新增的脚本 API——`register-trait-sneak-attack`——
//! 真的能被 `mods/example_mod/gameplay.scm` 调用，且真实注册的偷袭
//! 声明真的能走真实 `resolve_attack` + `apply` 追加伤害，不能靠
//! `crates/ll-sim/src/traits.rs`/`crates/ll-sim/src/resolve.rs`/
//! `crates/ll-mod/src/script_trait_api.rs` 里的单元测试自证——ADR 0018
//! 「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，本
//! 文件是盗贼偷袭接线批次的那份证据。
//!
//! 与 `crates/ll-mod/tests/example_mod_resistance.rs` 同一套「装载整个
//! `mods/` 目录，不是只挑 `example_mod`」手法，见该文件模块文档。
//!
//! # 为什么只断言严格更高，不像抗性那样断言精确数值
//!
//! 抗性测试（`example_mod_resistance.rs`）能断言精确倍率，因为两组
//! 防御方的攻击者幸运恒为零（`BaseStats::BASELINE.luck == 0`），暴击
//! 判定天然不介入。本文件的两个攻击者一个幸运为零、一个幸运为
//! `LUCKY_LUCK`——后者的有效幸运同时影响暴击判定（`crate::combat::crit_chance_permille`）
//! 与偷袭判定（两者复用同一个 `effective_luck`，见
//! `resolve_attack` 文档「偷袭接线」一节），因此不能排除这次攻击碰巧
//! 也暴击的可能——但暴击只会让伤害进一步变大，不会把它压回基准以下,
//! 严格更高这条断言在两种情形下都成立，不需要靠精确数值排除暴击这一
//! 变量,与 `crates/ll-sim/src/resolve.rs` 的
//! `幸运更高的角色暴击命中频率更高` 测试选用频率断言而非单次数值的
//! 理由同源。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
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
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::apply::apply;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::intent::Intent;
use ll_sim::item::EquipSlot;
use ll_sim::resolve::resolve_with_skills_traits_pools_items_formulas_and_damage_categories;
use ll_sim::skill::NoSkills;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_resistance.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 攻击者的幸运值——与 `gameplay.scm` 里
/// `examplemod:predatory_instinct` 声明的每点幸运 20‰ 相乘恰好等于
/// 1000（`50 × 20 = 1000`），换算出的偷袭触发率精确钳在千分之一千
/// （100%），这次攻击是否触发偷袭因此不依赖 `DetRng` 抽到的具体值——
/// 与 `crate::combat::sneak_attack_chance_permille` 文档「夹在
/// `0..=1000`」一节同一个边界，选最大边界值消灭本测试对 `world.clock`
/// 取值的依赖。
const LUCKY_LUCK: i32 = 50;

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引，理由同 `example_mod_resistance.rs::RealModsHandle`。
struct RealModsHandle {
    race: RaceTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    footpad_id: ContentIndex,
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
    let mut formula = FormulaTable::new();
    let mut weapon_category = WeaponCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut tag_table = ll_mod::tag::TagTable::new();
    let mut damage_category = DamageCategoryTable::new();

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
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            tag: &mut tag_table,
            events: &mut ll_mod::event::EventSubscriptionTable::new(),
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
        footpad_id: resolve("examplemod:footpad"),
        race,
        trait_def,
        item,
        formula,
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

/// 造一个占位实体，站在 `(5, 5)`，理由同
/// `example_mod_resistance.rs::spawn_agent`——本文件额外暴露 `luck`
/// 参数（其余六项主属性/装备/种族固定），供两个攻击者各自指定不同的
/// 有效幸运。
fn spawn_agent_with_luck(
    world: &mut WorldState,
    race: ContentIndex,
    health: i32,
    luck: i32,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats {
            luck,
            ..BaseStats::BASELINE
        },
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
        equipment: BTreeMap::<EquipSlot, ll_sim::item::ItemStack>::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
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

#[test]
fn 真实注册的迅足者种族在幸运充足时偷袭追加的伤害真实生效() {
    // 手工验证过这条会红：把 `resolve_attack` 里偷袭判定那一段整段
    // 去掉（等价于攻击者没有声明这条天赋），两个攻击者会打出完全相同
    // 的基准伤害，`sneak_damage > baseline_damage` 立即失败——
    // `crates/ll-sim/src/resolve.rs` 的统计频率测试与本文件的目标一致：
    // 前者验证判定本身受幸运影响、走 DetRng；本文件验证真实注册的 mod
    // 天赋走的是同一条真实链路，不是只在单元测试里自证。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    // 基准攻击者：迅足者种族（已被授予偷袭天赋），但幸运为零——有效
    // 幸运为零时 sneak_attack_chance_permille 恒返回零，判定分支虽然
    // 进入（种族确实声明了这条天赋）但恒不触发,是「声明了但这次判定
    // 没中」与「压根没有声明」两种情形的第一种。
    let baseline_attacker =
        spawn_agent_with_luck(&mut world, handle.footpad_id, Agent::STARTING_HEALTH, 0);
    let baseline_defender = spawn_agent_with_luck(&mut world, handle.footpad_id, 1_000, 0);
    // 幸运充足的攻击者：同一个种族，幸运恰好让触发率钳在 100%（见
    // LUCKY_LUCK 文档）。
    let sneak_attacker = spawn_agent_with_luck(
        &mut world,
        handle.footpad_id,
        Agent::STARTING_HEALTH,
        LUCKY_LUCK,
    );
    let sneak_defender = spawn_agent_with_luck(&mut world, handle.footpad_id, 1_000, 0);

    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        // 两个攻击者都是徒手（没有装备任何武器），恒退回全局默认公式
        // ——`formula_for` 只有在显式引用查不到时才会退回它,这里的
        // 默认值因此不会被真的用到。
        default_formula: ContentIndex::default(),
    };

    let attack = |world: &mut WorldState, attacker: EntityId, defender: EntityId| {
        let effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
            world,
            &Intent::Attack {
                actor: attacker,
                target: defender,
            },
            &NoSkills,
            &handle.race,
            &handle.trait_def,
            &ll_sim::resource_pool::NoResourcePools,
            &handle.item,
            &formulas,
            &NoDamageCategories,
        );
        for effect in &effects {
            apply(world, effect);
        }
    };

    // Act
    attack(&mut world, baseline_attacker, baseline_defender);
    attack(&mut world, sneak_attacker, sneak_defender);

    // Assert：幸运充足、偷袭必然触发的一侧造成的伤害严格更高——见本
    // 文件模块文档「为什么只断言严格更高」一节。
    let baseline_health = world
        .actors
        .get(baseline_defender)
        .expect("基准防御方未死亡")
        .health;
    let sneak_health = world
        .actors
        .get(sneak_defender)
        .expect("偷袭防御方未死亡")
        .health;
    let baseline_damage = 1_000 - baseline_health;
    let sneak_damage = 1_000 - sneak_health;
    assert!(
        sneak_damage > baseline_damage,
        "偷袭必然触发的攻击者应当打出严格更高的伤害（{sneak_damage} 应大于 {baseline_damage}）"
    );
}
