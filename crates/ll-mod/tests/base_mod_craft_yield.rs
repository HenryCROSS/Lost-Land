//! 端到端验证：**本体四条制作副职真的给得出东西了**——持有工匠/厨师
//! 副职的角色制作对应类别的东西时，产出数量真的多一件。
//!
//! # 这份证据要顶掉的那条阻塞
//!
//! `mods/lostland/subclasses.json5` 的文件头此前写着「**没有语义对的
//! 技能可给**」，`crates/ll-mod/src/content_audit.rs` 的
//! `SubclassAttrs::traits` 豁免条目记的是同一件事。那条阻塞把问题定位在
//! `SkillEffect` 上——定位错了：「会打铁」不是玩家按下去会发生什么的
//! **动作**（玩家已经有 `Intent::Craft` 了），是「**当我制作时，结算
//! 方式不一样**」，也就是一条**被动**。落地形状是
//! `ll_sim::rule_modifier::RuleModifier::CraftYield`，完整设计与四视角
//! 自检见 `knowledge/design/crafting-subclass-rewards.md`。
//!
//! # 手法：真实 `mods/` + `TurnEngine`，不是夹具也不是直调 `resolve_*`
//!
//! 与 `base_mod_recipe_discovery.rs` 逐段同构（ADR 0018：玩法层内容必须
//! 能从 mod 注册，且要有真实 mod 内容为证）：装载仓库里真实的 `mods/`
//! 整个目录，把装载出来的表借成 `ResolveCatalogs`，经
//! `TurnEngine::advance_ai` 这条**生产路径**恰好结算一次意图。
//!
//! 与那份文件唯一的结构差异是：本文件必须把**真实的**
//! `SubclassTable`（副职天赋来源）与 `TraitTable`（天赋定义）接进
//! `ResolveCatalogs`，因为要验的正是这两张表之间那条链路。那份文件在
//! 这两格填的是 `No*` 空实现。
//!
//! # 四条反例
//!
//! 1. [`不持有副职就没有产出加成`]——证明加成确实来自副职这一路；
//! 2. [`副职天赋来源换成空实现就没有产出加成`]——**摘掉接线就变红**,
//!    若哪天有人把 `resolve_dispatch` 里那一路悄悄改回空实现，本条会
//!    当场发现；
//! 3. [`天赋定义目录换成空实现就没有产出加成`]——同上，守的是另一半;
//! 4. [`厨师做铁铆钉一件都不多`]——证明加成**按配方类别键控**，一个
//!    会做饭的不会因此打得一手好铁。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::behavior_binding::ClassBehaviorBindings;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::formula::FormulaTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::modifier_type::ModifierTypeTable;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::recipe::{RecipeTable, RegisteredRecipes};
use ll_mod::recipe_category::RecipeCategoryTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::tag::TagTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::quest::NoQuests;
use ll_sim::timeline::Timeline;
use ll_sim::traits::{NoTraitGrants, NoTraits, TraitCatalog, TraitGrantSource};
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `base_mod_recipe_discovery.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

const NO_SKILLS: ll_sim::skill::NoSkills = ll_sim::skill::NoSkills;
const NO_RACE_TRAITS: NoTraitGrants = NoTraitGrants;
const NO_CLASS_TRAITS: NoTraitGrants = NoTraitGrants;
const NO_SUBCLASS_TRAITS: NoTraitGrants = NoTraitGrants;
const NO_TRAITS: NoTraits = NoTraits;
const NO_POOLS: ll_sim::resource_pool::NoResourcePools = ll_sim::resource_pool::NoResourcePools;
const NO_FORMULAS: ll_sim::formula::NoFormulas = ll_sim::formula::NoFormulas;

/// `mods/lostland/traits.json5` 给四条制作精通填的额外产出件数。写成
/// 常量而不是把 `1` 散在断言里——它是一个**占位内容参数**（文件头写
/// 明了），改它不该让本文件的断言逐条去猜哪个数字是它。
const MASTERY_BONUS: u32 = 1;

/// `lostland:roast_meat_recipe` 声明的产出件数。
const ROAST_MEAT_PRODUCT_COUNT: u32 = 1;

/// `lostland:iron_rivet_batch` 声明的产出件数（一锭八铆钉）。
const IRON_RIVET_PRODUCT_COUNT: u32 = 8;

/// 一次真实装载的产物——只留下本文件断言需要的那几张表与索引。
struct Handle {
    item: ItemTable,
    recipe: RecipeTable,
    recipe_category: RecipeCategoryTable,
    subclass: SubclassTable,
    trait_def: TraitTable,
    artisan: ContentIndex,
    cook: ContentIndex,
    roast_meat_recipe: ContentIndex,
    iron_rivet_batch: ContentIndex,
    raw_meat: ContentIndex,
    roast_meat: ContentIndex,
    iron_ingot: ContentIndex,
    iron_rivet: ContentIndex,
    smith_hammer: ContentIndex,
}

impl Handle {
    /// 借出一份 `ResolveCatalogs`，副职天赋来源与天赋定义目录**可替换**
    /// ——两条反例（接线被摘掉）靠的就是这两个参数。
    fn catalogs<'a>(
        &'a self,
        recipes: &'a RegisteredRecipes<'a>,
        subclass_traits: &'a dyn TraitGrantSource,
        trait_defs: &'a dyn TraitCatalog,
    ) -> ResolveCatalogs<'a> {
        ResolveCatalogs {
            skills: &NO_SKILLS,
            quests: &NoQuests,
            race_traits: &NO_RACE_TRAITS,
            class_traits: &NO_CLASS_TRAITS,
            subclass_traits,
            trait_defs,
            pools: &NO_POOLS,
            items: &self.item,
            formulas: &NO_FORMULAS,
            damage_categories: &NoDamageCategories,
            recipes,
            ambient: AmbientSource::NONE,
            experience: &NoExperience,
            skill_tree: &NO_SKILLS,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
        }
    }

    fn real_recipes(&self) -> RegisteredRecipes<'_> {
        RegisteredRecipes {
            recipes: &self.recipe,
            categories: &self.recipe_category,
        }
    }
}

fn load_real_mods() -> Handle {
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
    let mut class_behavior_bindings = ClassBehaviorBindings::new();
    let mut trait_def = TraitTable::new();
    let mut resource_pool = ResourcePoolTable::new();
    let mut item = ItemTable::new();
    let mut formula = FormulaTable::new();
    let mut weapon_category = WeaponCategoryTable::new();
    let mut damage_category = DamageCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut resource_table = ll_world::resource::ResourceTable::new();
    let mut culture_table = ll_world::culture::CultureTable::new();
    let mut recipe_table = RecipeTable::new();
    let mut recipe_category_table = RecipeCategoryTable::new();
    let mut tag_table = TagTable::new();
    let mut modifier_type_table = ModifierTypeTable::new();
    let mut dialogue = ll_mod::dialogue::DialogueTable::new();
    let mut dialogue_node = ll_mod::dialogue::DialogueNodeTable::new();

    let report = load_all(
        Path::new(REAL_MODS_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut terrain,
            class: &mut class,
            skill: &mut skill,
            subclass: &mut subclass,
            quest: &mut quest,
            dialogue: &mut dialogue,
            dialogue_node: &mut dialogue_node,
            race: &mut race,
            clip: &mut clip,
            xp_curve: &mut xp_curve,
            xp_curve_bindings: &mut xp_curve_bindings,
            class_behavior_bindings: &mut class_behavior_bindings,
            trait_def: &mut trait_def,
            resource_pool: &mut resource_pool,
            item: &mut item,
            formula: &mut formula,
            weapon_category: &mut weapon_category,
            damage_category: &mut damage_category,
            space_profile: &mut space_profile,
            resource: &mut resource_table,
            culture: &mut culture_table,
            weather: &mut weather_table,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            modifier_type: &mut modifier_type_table,
            tag: &mut tag_table,
        },
    );
    let lostland_id = NamespacedId::parse("lostland:self").expect("合法标识符");
    let status = report
        .entries
        .iter()
        .find(|(id, _)| *id == lostland_id)
        .map(|(_, status)| status);
    assert_eq!(
        status,
        Some(&LoadStatus::Loaded),
        "本体内容 mod（mods/lostland/）必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).expect("合法标识符"))
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/lostland/ 的内容文件注册"))
    };

    Handle {
        artisan: resolve("lostland:artisan"),
        cook: resolve("lostland:cook"),
        roast_meat_recipe: resolve("lostland:roast_meat_recipe"),
        iron_rivet_batch: resolve("lostland:iron_rivet_batch"),
        raw_meat: resolve("lostland:raw_meat"),
        roast_meat: resolve("lostland:roast_meat"),
        iron_ingot: resolve("lostland:iron_ingot"),
        iron_rivet: resolve("lostland:iron_rivet"),
        smith_hammer: resolve("lostland:smith_hammer"),
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
        subclass,
        trait_def,
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

/// 一次制作的全部输入——参数多到该有名字，理由同
/// `base_mod_recipe_discovery.rs::spawn_agent` 那一串位置参数的教训。
struct Attempt {
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
    known_recipes: Vec<ContentIndex>,
    subclasses: Vec<ContentIndex>,
    recipe: ContentIndex,
}

fn spawn_agent(world: &mut WorldState, pos: (i32, i32), attempt: &Attempt) -> EntityId {
    let mut interner = Interner::new();
    let placeholder = interner.intern(NamespacedId::parse("lostland:tester").expect("合法"));
    let agent_pos = world.size.wrap(pos.0, pos.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(agent_pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos: agent_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: placeholder,
        goals: Vec::new(),
        race: placeholder,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory: attempt.inventory.clone(),
        equipment: attempt.equipment.clone(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: attempt.known_recipes.clone(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: attempt.subclasses.clone(),
        subclasses_ever_granted: attempt.subclasses.clone(),
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

/// 跑一场「主角经由 [`TurnEngine`] 提交恰好一次 [`Intent::Craft`]」，
/// 返回结算之后主角的背包。副职天赋来源与天赋定义目录由调用方给，
/// 反例靠替换这两个参数实现。
fn craft_via_turn_engine(
    handle: &Handle,
    attempt: &Attempt,
    subclass_traits: &dyn TraitGrantSource,
    trait_defs: &dyn TraitCatalog,
) -> Vec<ItemStack> {
    let mut world = test_world();
    let hero = spawn_agent(&mut world, (5, 5), attempt);
    let idle = Attempt {
        inventory: Vec::new(),
        equipment: BTreeMap::new(),
        known_recipes: Vec::new(),
        subclasses: Vec::new(),
        recipe: attempt.recipe,
    };
    let bystander = spawn_agent(&mut world, (9, 9), &idle);

    let mut timeline = Timeline::new();
    timeline.schedule(hero, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let recipes = handle.real_recipes();
    let catalogs = handle.catalogs(&recipes, subclass_traits, trait_defs);
    let recipe = attempt.recipe;
    let mut intent = |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Craft {
        actor,
        recipe,
    };
    engine.advance_ai(
        &mut world,
        bystander,
        &mut intent,
        &catalogs,
        &mut |_, _| {},
    );

    world
        .actors
        .get(hero)
        .expect("制作不会杀死主角")
        .inventory
        .clone()
}

/// 背包里某种物品的总数（跨多堆求和）。
fn count_of(inventory: &[ItemStack], def: ContentIndex) -> u32 {
    inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

/// 一个会做饭的角色，手里有生肉、知道烤肉这条配方。
fn cooking_attempt(handle: &Handle, subclasses: Vec<ContentIndex>) -> Attempt {
    Attempt {
        inventory: vec![ItemStack::new(handle.raw_meat, 3)],
        equipment: BTreeMap::new(),
        known_recipes: vec![handle.roast_meat_recipe],
        subclasses,
        recipe: handle.roast_meat_recipe,
    }
}

/// 一个要打铆钉的角色：手里有铁锭、腰上挂着铁匠锤、知道这条配方。
/// 铆钉那条配方**不要场地**（只要工具），因此不需要摆地形。
fn rivet_attempt(handle: &Handle, subclasses: Vec<ContentIndex>) -> Attempt {
    let mut equipment = BTreeMap::new();
    equipment.insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.smith_hammer, 1));
    Attempt {
        inventory: vec![ItemStack::new(handle.iron_ingot, 3)],
        equipment,
        known_recipes: vec![handle.iron_rivet_batch],
        subclasses,
        recipe: handle.iron_rivet_batch,
    }
}

// ── 正例：副职真的给得出东西了 ──────────────────────────────────────

#[test]
fn 厨师烤一次肉多出一份() {
    // 这是本批次要证明的那句话最短的可执行版本：`lostland:cook` 授予
    // `lostland:cooking_mastery`，那条天赋带一条指向 `lostland:cooking`
    // 的 `CraftYield`，于是烹饪类别的产出 +1。
    // Arrange
    let handle = load_real_mods();
    let attempt = cooking_attempt(&handle, vec![handle.cook]);

    // Act
    let inventory = craft_via_turn_engine(&handle, &attempt, &handle.subclass, &handle.trait_def);

    // Assert：一份配方本来出一份，厨师出两份。生肉照常只扣一份。
    assert_eq!(
        count_of(&inventory, handle.roast_meat),
        ROAST_MEAT_PRODUCT_COUNT + MASTERY_BONUS
    );
    assert_eq!(count_of(&inventory, handle.raw_meat), 2);
}

#[test]
fn 工匠打一炉铆钉多出一个() {
    // 加成是**加在配方声明的件数之上**，不是取而代之——铆钉那条配方
    // 本来一炉出八个，工匠出九个。这条同时守住「`product_count > 1`
    // 的配方也走同一条路径」。
    // Arrange
    let handle = load_real_mods();
    let attempt = rivet_attempt(&handle, vec![handle.artisan]);

    // Act
    let inventory = craft_via_turn_engine(&handle, &attempt, &handle.subclass, &handle.trait_def);

    // Assert
    assert_eq!(
        count_of(&inventory, handle.iron_rivet),
        IRON_RIVET_PRODUCT_COUNT + MASTERY_BONUS
    );
}

// ── 反例一：没有副职就没有加成 ──────────────────────────────────────

#[test]
fn 不持有副职就没有产出加成() {
    // 证明加成确实来自副职这一路，不是碰巧从别处漏进来的。
    // Arrange
    let handle = load_real_mods();
    let attempt = cooking_attempt(&handle, Vec::new());

    // Act
    let inventory = craft_via_turn_engine(&handle, &attempt, &handle.subclass, &handle.trait_def);

    // Assert
    assert_eq!(
        count_of(&inventory, handle.roast_meat),
        ROAST_MEAT_PRODUCT_COUNT
    );
}

// ── 反例二/三：把接线摘掉就变红 ─────────────────────────────────────

#[test]
fn 副职天赋来源换成空实现就没有产出加成() {
    // **这条是「接线是活的」的守门测试**：角色照旧持有厨师副职、天赋
    // 定义目录照旧是真表，只把 `ResolveCatalogs::subclass_traits` 换成
    // 空实现。若哪天有人把 `resolve_craft` 那一路参数悄悄改回
    // `NoTraitGrants`（或者干脆不传），上面那条正例仍然会绿——只有本条
    // 会红。
    // Arrange
    let handle = load_real_mods();
    let attempt = cooking_attempt(&handle, vec![handle.cook]);

    // Act
    let inventory =
        craft_via_turn_engine(&handle, &attempt, &NO_SUBCLASS_TRAITS, &handle.trait_def);

    // Assert
    assert_eq!(
        count_of(&inventory, handle.roast_meat),
        ROAST_MEAT_PRODUCT_COUNT
    );
}

#[test]
fn 天赋定义目录换成空实现就没有产出加成() {
    // 同上，守的是链路的另一半：副职说得出「我授予 cooking_mastery」，
    // 但查不到那条天赋的定义，也就读不到它带的 `CraftYield`。
    // Arrange
    let handle = load_real_mods();
    let attempt = cooking_attempt(&handle, vec![handle.cook]);

    // Act
    let inventory = craft_via_turn_engine(&handle, &attempt, &handle.subclass, &NO_TRAITS);

    // Assert
    assert_eq!(
        count_of(&inventory, handle.roast_meat),
        ROAST_MEAT_PRODUCT_COUNT
    );
}

// ── 反例四：加成按配方类别键控 ──────────────────────────────────────

#[test]
fn 厨师做铁铆钉一件都不多() {
    // 「一个铁匠不该因为会打铁就烧得一手好菜」的反向版本。厨师带着
    // 铁匠锤照样能打铆钉（`lostland:forging` 不设闸门），但他那条
    // `CraftYield` 指的是 `lostland:cooking`，命中不了这一次制作。
    // Arrange
    let handle = load_real_mods();
    let attempt = rivet_attempt(&handle, vec![handle.cook]);

    // Act
    let inventory = craft_via_turn_engine(&handle, &attempt, &handle.subclass, &handle.trait_def);

    // Assert
    assert_eq!(
        count_of(&inventory, handle.iron_rivet),
        IRON_RIVET_PRODUCT_COUNT
    );
}
