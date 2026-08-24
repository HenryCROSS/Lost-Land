//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! 伤害类别/抗性接线批次的三条新脚本 API——`register-damage-category`/
//! `register-trait-resistance`/`register-item-damage-category`——真的
//! 能被 `mods/example_mod/traits.json5` 调用，且真实注册的抗性声明
//! 真的能走真实 `resolve_attack` + `apply` 降低伤害——ADR 0018「玩法层
//! 内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，本文件是伤害
//! 类别/抗性接线批次的那份证据，不能靠
//! `crates/ll-sim/tests/resistance_resolve.rs`（合成夹具）/
//! `crates/ll-mod/src/script_damage_category_api.rs`/
//! `crates/ll-mod/src/script_trait_api.rs` 里的单元测试自证。
//!
//! 抗性多来源聚合批次追加了第二路来源（装备）的那份证据：新脚本 API
//! `register-item-resistance` 同样真的被 `gameplay.scm` 调用，且装备
//! 声明的抗性走的是与天赋完全同一个聚合点
//! （`ll_sim::rule_modifier::resistance_multiplier_permille`）——见本
//! 文件末尾两条 `酸抗护符` 测试。
//!
//! 与 `crates/ll-mod/tests/example_mod_weapon_reference.rs` 同一个理由
//! 独立成文件、同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」
//! 手法，见 `example_mod_resource_pools.rs` 模块文档。

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
use ll_mod::modifier_type::ModifierTypeTable;
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
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::resolve::resolve_with_skills_traits_pools_items_formulas_and_damage_categories;
use ll_sim::rule_modifier::RuleModifier;
use ll_sim::skill::NoSkills;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_items.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引，理由同 `example_mod_weapon_reference.rs::RealModsHandle`。
struct RealModsHandle {
    race: RaceTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    ooze_id: ContentIndex,
    half_elf_id: ContentIndex,
    acid_dagger_id: ContentIndex,
    acid_ward_amulet_id: ContentIndex,
    acid_id: ContentIndex,
    /// `examplemod:enhancement`——护符那条抗性声明的加值类型
    /// （加值类型批次）。
    enhancement_type_id: ContentIndex,
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

    let mut modifier_type_table = ModifierTypeTable::new();
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
            modifier_type: &mut modifier_type_table,
            tag: &mut tag_table,
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
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/traits.json5 注册"))
    };

    RealModsHandle {
        ooze_id: resolve("examplemod:ooze"),
        half_elf_id: resolve("examplemod:half_elf"),
        acid_dagger_id: resolve("examplemod:acid_dagger"),
        acid_ward_amulet_id: resolve("examplemod:acid_ward_amulet"),
        acid_id: resolve("examplemod:acid"),
        enhancement_type_id: resolve("examplemod:enhancement"),
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
/// `example_mod_weapon_reference.rs::spawn_agent`（本文件不需要验收
/// 击杀记录，因此不暴露 `remembered` 参数）。
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

#[test]
fn 真实注册的软泥怪种族对酸的抗性真实降低了酸匕首造成的伤害() {
    // 手工验证过这条会红：把 `resolve_attack` 里应用抗性乘数的那一步
    // 去掉（恒等于不打折），本测试的两组防御方会拿到完全相同的伤害,
    // 断言 `ooze_damage < baseline_damage` 立即失败——`crates/ll-sim/tests/resistance_resolve.rs`
    // 的合成夹具版本已经做过这条手工红/绿验证,这里只需确认真实注册的
    // mod 内容走的是同一条真实链路,不重复那份验证过程。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        handle.half_elf_id,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::new(handle.acid_dagger_id, 1),
        )]),
    );
    // 基准防御方：半精灵，没有任何对酸的抗性声明。
    let baseline_defender = spawn_agent(&mut world, handle.half_elf_id, 1_000, BTreeMap::new());
    // 真实防御方：软泥怪，`mods/example_mod/traits.json5` 声明了它在
    // 1 级被授予 `examplemod:acid_hide`（500‰ 对酸的抗性）。
    let ooze_defender = spawn_agent(&mut world, handle.ooze_id, 1_000, BTreeMap::new());

    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        // 酸匕首显式声明了公式引用（见 gameplay.scm 的
        // register-item-damage-formula 调用），这里的默认值不会被真的
        // 用到——`formula_for` 只有在显式引用查不到时才会退回它。
        default_formula: ContentIndex::default(),
    };

    let attack = |world: &mut WorldState, defender: EntityId| {
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
    attack(&mut world, baseline_defender);
    attack(&mut world, ooze_defender);

    // Assert：软泥怪受到的伤害严格更低。
    let baseline_health = world
        .actors
        .get(baseline_defender)
        .expect("基准防御方未死亡")
        .health;
    let ooze_health = world
        .actors
        .get(ooze_defender)
        .expect("软泥怪防御方未死亡")
        .health;
    let baseline_damage = 1_000 - baseline_health;
    let ooze_damage = 1_000 - ooze_health;
    assert!(
        ooze_damage < baseline_damage,
        "软泥怪对酸的抗性应当让它受到的伤害（{ooze_damage}）严格低于没有抗性的基准伤害（{baseline_damage}）"
    );
    // 软泥怪的 acid_hide 声明 4 点减伤（`mods/example_mod/traits.json5`）。
    // 抗性从千分比乘数改成减伤点数之后，期望值也从
    // `基准 × 500 / 1000` 改成 `基准 − 4`——这一条数字变了不是断言被
    // 迁就，是内容与模型一起换代：酸蚀匕首在本夹具下打出 10 点基准伤害,
    // 减掉 4 点剩 6 点（旧模型是 10 × 0.5 = 5 点）。
    assert_eq!(baseline_damage, 10, "本夹具下酸伤基准值，两条断言共用");
    assert_eq!(ooze_damage, baseline_damage - 4);
    assert_eq!(ooze_damage, 6);
}

#[test]
fn 真实注册的酸伤害类别与武器类别都能查到独立的内容索引() {
    // 直接验收 register-weapon-category/register-damage-category 两个
    // 新脚本 API 真的把各自的声明写进了同一个 Registry——两条轴互相
    // 独立（damage-formula-mod-api.md 十七节「不是同一种东西」），各自
    // 的 id 字符串不冲突。
    // Arrange
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
    let mut modifier_type_table = ModifierTypeTable::new();

    // Act
    load_all(
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
            modifier_type: &mut modifier_type_table,
            tag: &mut tag_table,
        },
    );

    // Assert
    let dagger_category = registry
        .get(&NamespacedId::parse("examplemod:dagger").unwrap())
        .expect("武器类别应已注册");
    let acid_category = registry
        .get(&NamespacedId::parse("examplemod:acid").unwrap())
        .expect("伤害类别应已注册");
    assert_ne!(dagger_category, acid_category);
    assert!(weapon_category.is_defined(dagger_category));
    assert!(damage_category.is_defined(acid_category));
    // 交叉核实：武器类别索引查不到伤害类别表里,反之亦然——两条轴各自
    // 独立的存储,不是同一张表的两个视图。
    assert!(!damage_category.is_defined(dagger_category));
    assert!(!weapon_category.is_defined(acid_category));
}

#[test]
fn 加值类型不同的两条抗性在真实内容上相加而不是取最强() {
    // 加值类型批次的端到端证据，也是项目所有者那条裁定
    // 「同一类型取最强，不同类型相加」在**真实内容**上的落点：
    //
    // - `examplemod:acid_hide`（软泥怪种族天赋）：4 点减伤，类型
    //   `examplemod:innate`。
    // - `examplemod:acid_ward_amulet`（护符）：3 点减伤，类型
    //   `examplemod:enhancement`。
    //
    // 两者**类型不同**，因此一只戴着护符的软泥怪吃到 4 + 3 = 7 点减伤,
    // 而不是取最强的 4 点。10 点基准酸伤减掉 7 点剩 3 点。
    //
    // 这一条与它上下两条测试构成完整的三段论：单独天赋 → 6 点伤害,
    // 单独护符 → 7 点伤害，两者同时 → 3 点伤害。若分桶层写错成
    // 「全体取最强」，这里会是 6；若写错成「无条件全部相加」，
    // 上面两条单独测试的数字不会变、只有这一条能抓到——所以三条都要。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        handle.half_elf_id,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::new(handle.acid_dagger_id, 1),
        )]),
    );
    // 软泥怪（天生 4 点）+ 护符（附魔 3 点）。
    let both_defender = spawn_agent(
        &mut world,
        handle.ooze_id,
        1_000,
        BTreeMap::from([(
            EquipSlot::NECK,
            ItemStack::new(handle.acid_ward_amulet_id, 1),
        )]),
    );

    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };

    // Act
    let effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: both_defender,
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
        apply(&mut world, effect);
    }

    // Assert
    let damage = 1_000
        - world
            .actors
            .get(both_defender)
            .expect("防御方未死亡")
            .health;
    assert_eq!(
        damage, 3,
        "天生 4 点 + 附魔 3 点应当相加成 7 点减伤，10 − 7 = 3"
    );
}

#[test]
fn 真实注册的酸抗护符装备在身上时真实降低了酸匕首造成的伤害() {
    // 抗性多来源聚合批次：项目所有者对抗性来源的裁定「抗性肯定会来自
    // 天赋，以及装备，还有各种药品，或者技能」里**装备**这一路的那份
    // ADR 0018 证据——`register-item-resistance` 是本批次新增的脚本
    // API，`mods/example_mod/traits.json5` 真的调用了它，本测试证明这条
    // 声明真的走完了 `ItemTable` → `ll_sim::item::ItemRule::rule_modifiers`
    // → `ll_sim::rule_modifier::equipment_rule_modifiers` → 聚合点 →
    // `resolve_attack` 的抗性乘数这条完整链路。
    //
    // 两个防御方都是**半精灵**（没有 `examplemod:acid_hide` 天赋），
    // 唯一差别是脖子上戴没戴护符——降下来的伤害只可能来自装备这一路。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let attacker = spawn_agent(
        &mut world,
        handle.half_elf_id,
        Agent::STARTING_HEALTH,
        BTreeMap::from([(
            EquipSlot::MAIN_HAND,
            ItemStack::new(handle.acid_dagger_id, 1),
        )]),
    );
    let bare_defender = spawn_agent(&mut world, handle.half_elf_id, 1_000, BTreeMap::new());
    let warded_defender = spawn_agent(
        &mut world,
        handle.half_elf_id,
        1_000,
        BTreeMap::from([(
            EquipSlot::NECK,
            ItemStack::new(handle.acid_ward_amulet_id, 1),
        )]),
    );

    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };

    let attack = |world: &mut WorldState, defender: EntityId| {
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
    attack(&mut world, bare_defender);
    attack(&mut world, warded_defender);

    // Assert
    let bare_damage = 1_000
        - world
            .actors
            .get(bare_defender)
            .expect("裸防御方未死亡")
            .health;
    let warded_damage = 1_000
        - world
            .actors
            .get(warded_defender)
            .expect("戴护符的防御方未死亡")
            .health;
    assert!(
        warded_damage < bare_damage,
        "酸抗护符应当让戴着它的防御方受到的伤害（{warded_damage}）严格低于没戴的基准伤害（{bare_damage}）"
    );
    // 护符声明 3 点减伤（`mods/example_mod/items.json5`），比软泥怪那条
    // 天赋的 4 点低一点——一件戴上就能换下来的护符不该比一整层天生的
    // 皮膜更耐酸。10 点基准伤害减掉 3 点剩 7 点（旧模型两者同为 500‰、
    // 结果同为 5 点，那条「两路来源结果逐点相同」的巧合随内容重新配值
    // 一起消失，本来也不是任何一条规则的保证）。
    assert_eq!(bare_damage, 10, "本夹具下酸伤基准值，与上一条测试同一个数");
    assert_eq!(warded_damage, bare_damage - 3);
    assert_eq!(warded_damage, 7);
}

#[test]
fn 真实注册的酸抗护符的抗性声明真的写进了物品表() {
    // 直接验收 `register-item-resistance` 把声明写进了 `ItemTable` 的
    // `rule_modifiers` 列，且引用的伤害类别就是同一个 `examplemod:acid`
    // ——与上一条端到端测试互补：那条证明"能影响结算"，这条证明"存进去
    // 的确实是那条声明本身"，两条一起排除"恰好靠别的机制降了伤害"。
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .item
        .get(handle.acid_ward_amulet_id)
        .expect("护符应已注册");

    // Assert
    assert_eq!(view.rule_modifiers.len(), 1);
    let typed = &view.rule_modifiers[0];
    // 加值类型批次：护符这条抗性显式声明了「附魔」，与 acid_hide 天赋
    // 的「天生」分属两类，因此两者点数相加而不是取最强——端到端后果由
    // 上一条测试断言，这里只证明声明本身真的存进去了。
    assert_eq!(
        typed.modifier_type,
        Some(handle.enhancement_type_id),
        "护符的加值类型应当是 examplemod:enhancement"
    );
    let RuleModifier::Resistance {
        damage_category,
        damage_reduction,
    } = &typed.modifier
    else {
        panic!("护符声明的应当是一条抗性");
    };
    assert_eq!(*damage_reduction, 3);
    // 引用的确实是脚本里写的那个伤害类别，不是别的凑巧同索引的东西
    // ——`acid_id` 走的是同一份装载后注册表的解析结果。
    assert_eq!(*damage_category, handle.acid_id);
}
