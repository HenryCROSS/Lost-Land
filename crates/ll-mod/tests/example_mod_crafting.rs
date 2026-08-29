//! 端到端验证：真实装载仓库里的 `mods/` 目录，证明
//! `mods/example_mod/crafting.json5` 里注册的配方**经由
//! [`ll_sim::turn::TurnEngine`]**（本体二进制 `ll-game` 驱动世界的唯一
//! 路径）真的产出了成品、真的消耗了食材——不是靠测试直接调
//! `resolve_with_*` 自证。
//!
//! # 验收标准
//!
//! 与 `turn_engine_catalogs.rs` 同一条（那份文档完整记录了它的由来）：
//! 内容来自真实 `mods/`（ADR 0018），**且**结算必须经由 `TurnEngine`
//! 的公开入口发生。本文件因此全程只调
//! [`ll_sim::turn::TurnEngine::advance_ai`]，一次都不碰
//! `ll_sim::resolve` 的任何入口。
//!
//! # 反例守卫
//!
//! [`目录从回合引擎摘掉后同一场景里制作不再发生`] 是那份守卫：同一段
//! 场景、同一个 `TurnEngine`，只把配方目录换成
//! [`ll_sim::craft::NoRecipes`]，制作立刻不发生、背包一动不动。谁把
//! 配方目录从 `TurnEngine` 那条链路上摘掉（比如把 `resolve_dispatch`
//! 的 `Intent::Craft` 分支改回不查目录），上面那条正向测试就会拿到与
//! 本条完全一样的结果而变红。
//!
//! # 本文件不验收什么
//!
//! **玩家怎么提交一次制作。** `Intent::Craft` 目前没有任何产出者：
//! `ll_sim::intent::intent_from_input` 只映射 `Move`/`Wait` 两种（本行
//! 此前写的是「三种，含 `ToggleStealth`」，与该函数的实际代码不符，
//! 配方发现批次核实后更正），制作界面（`UiMode` 模式栈）是纯设计零
//! 实现。
//! 这与 `PickUp`/`Drop`/`Equip`/`Rest`/`Loot`/`Use` 六个既有玩法意图
//! 的处境完全相同——输入映射层整体尚未展开。本文件里那个「AI 策略
//! 直接返回 `Intent::Craft`」正是设计文档八节⑦点名的最小占位提交
//! 路径，不假装制作在真实玩法里已经可达。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::recipe::{RecipeTable, RegisteredRecipes};
use ll_mod::recipe_category::RecipeCategoryTable;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::craft::{NoRecipes, RecipeCatalog};
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::quest::NoQuests;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `turn_engine_catalogs.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 一次真实装载的产物——只留下本文件断言需要的那几张表与索引。
struct RealModsHandle {
    item: ItemTable,
    recipe: RecipeTable,
    recipe_category: RecipeCategoryTable,
    roast_meat_recipe: ContentIndex,
    iron_sword_recipe: ContentIndex,
    arrow_batch_recipe: ContentIndex,
    raw_meat: ContentIndex,
    roast_meat: ContentIndex,
    iron_ingot: ContentIndex,
    iron_sword: ContentIndex,
    arrow: ContentIndex,
    war_hammer: ContentIndex,
    /// 家具层批次：两条锻造配方的 required_station 现在指着它（一件
    /// **家具物品**），不再是 lava_floor 那种地形。
    portable_anvil: ContentIndex,
    /// 反例用的另一件家具——「摆着的家具不是配方要的那一件」。本体的
    /// 锻炉与 example_mod 的铁砧是两件不同的家具，正好当这个反例。
    forge: ContentIndex,
    shadowdancer: ContentIndex,
}

impl RealModsHandle {
    /// 把真实装载出来的表借成结算目录束——本体二进制
    /// （`ll_game::content::RuntimeCatalogs::as_resolve_catalogs`）交给
    /// `TurnEngine` 的是同一个形状、同一批表。
    fn catalogs<'a>(&'a self, recipes: &'a dyn RecipeCatalog) -> ResolveCatalogs<'a> {
        ResolveCatalogs {
            skills: &NO_SKILLS,
            quests: &NoQuests,
            race_traits: &NO_RACE_TRAITS,
            class_traits: &NO_CLASS_TRAITS,
            subclass_traits: &NO_SUBCLASS_TRAITS,
            trait_defs: &NO_TRAITS,
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

    /// 本文件正向场景用的真实配方目录。
    fn real_recipes(&self) -> RegisteredRecipes<'_> {
        RegisteredRecipes {
            recipes: &self.recipe,
            categories: &self.recipe_category,
        }
    }
}

// 本文件只关心制作那一路，其余目录一律接空实现——与
// `ResolveCatalogs::empty()` 里各路空实现逐字同源。具名常量而不是
// 临时值，理由同 `ll_sim::catalogs` 里那一组常量。
const NO_SKILLS: ll_sim::skill::NoSkills = ll_sim::skill::NoSkills;
const NO_RACE_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_CLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_SUBCLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_TRAITS: ll_sim::traits::NoTraits = ll_sim::traits::NoTraits;
const NO_POOLS: ll_sim::resource_pool::NoResourcePools = ll_sim::resource_pool::NoResourcePools;
const NO_FORMULAS: ll_sim::formula::NoFormulas = ll_sim::formula::NoFormulas;

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
        ..
    } = session;
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
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/crafting.json5 注册"))
    };

    RealModsHandle {
        roast_meat_recipe: resolve("examplemod:roast_meat_recipe"),
        iron_sword_recipe: resolve("examplemod:iron_sword_recipe"),
        arrow_batch_recipe: resolve("examplemod:arrow_batch_recipe"),
        raw_meat: resolve("examplemod:raw_meat"),
        roast_meat: resolve("examplemod:roast_meat"),
        iron_ingot: resolve("examplemod:iron_ingot"),
        iron_sword: resolve("examplemod:iron_sword"),
        arrow: resolve("examplemod:arrow"),
        war_hammer: resolve("examplemod:war_hammer"),
        portable_anvil: resolve("examplemod:portable_anvil"),
        forge: resolve("lostland:forge"),
        shadowdancer: resolve("examplemod:shadowdancer"),
        item,
        recipe: recipe_table,
        recipe_category: recipe_category_table,
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

/// 一次制作场景的全部输入——每条测试只改其中一两项，其余保持不变。
struct Scene {
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
    subclasses: Vec<ContentIndex>,
    /// `Some` 时把制作者脚下那一格改写成这种地形。
    station_underfoot: Option<ContentIndex>,
}

impl Scene {
    fn new(inventory: Vec<ItemStack>) -> Scene {
        Scene {
            inventory,
            equipment: BTreeMap::new(),
            subclasses: Vec::new(),
            station_underfoot: None,
        }
    }
}

/// 造一个占位实体：本文件只关心背包/装备/副职/位置四项，其余全部取
/// 与 `turn_engine_catalogs.rs::spawn_agent` 相同的中性默认值。
fn spawn_agent(world: &mut WorldState, pos: (i32, i32), scene: &Scene) -> EntityId {
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
        inventory: scene.inventory.clone(),
        equipment: scene.equipment.clone(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: scene.subclasses.clone(),
        subclasses_ever_granted: scene.subclasses.clone(),
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

/// 跑一场「制作者经由 `TurnEngine` 提交恰好一次 `Intent::Craft`」，
/// 返回制作结束后制作者的背包。
///
/// # 为什么恰好只结算一次
///
/// 手法与 `turn_engine_catalogs.rs::damage_dealt_via_turn_engine`
/// 完全相同：制作者排在 `Tick(0)`、旁观者（`controlled`）排在
/// `Tick(1)`，`advance_ai` 先弹出制作者结算一次，下一次弹出的是
/// `controlled`，于是立即返回。
fn craft_via_turn_engine(
    handle: &RealModsHandle,
    scene: &Scene,
    recipe: ContentIndex,
    recipes: &dyn RecipeCatalog,
) -> Vec<ItemStack> {
    craft_via_turn_engine_full(handle, scene, recipe, recipes).0
}

/// [`craft_via_turn_engine`] 的完整版本：除背包之外**还返回制作者结算
/// 之后的装备栏**，供「工具磨损」那一路断言使用（耐久扩面批次）。
///
/// 两个入口而不是改既有那个的返回类型，理由同
/// `ll_sim::resolve::derive_stats`/`derive_stats_at` 那一对：本文件既有
/// 的七条制作断言一条都不看装备栏，多返回一个它们全要写 `.0` 只是噪音。
fn craft_via_turn_engine_full(
    handle: &RealModsHandle,
    scene: &Scene,
    recipe: ContentIndex,
    recipes: &dyn RecipeCatalog,
) -> (Vec<ItemStack>, BTreeMap<EquipSlot, ItemStack>) {
    let mut world = test_world();
    let crafter = spawn_agent(&mut world, (5, 5), scene);
    let bystander = spawn_agent(&mut world, (9, 9), &Scene::new(Vec::new()));

    if let Some(station) = scene.station_underfoot {
        // 家具放置状态批次：工作台是**立在脚下那一格上**的一件东西
        // （`placed: true`），不再只是「一件带 furniture 标志的地面
        // 物品」。这里直接往 `ground_items` 里放一条立着的——与玩家走
        // `Intent::Place` 产出的是同一个 `GroundItemStack`（放置的四道
        // 前置在 `furniture_placement.rs` 有自己的端到端证据，本文件只
        // 关心「立着了之后制作认不认」）。
        //
        // `placed: true` 不是可以省略的细节：躺在脚下、没立起来的同一件
        // 东西当不了场地，那正是 `furniture_placement.rs` 里
        // 「脚下的锻炉只是躺着时锻造配方静默不产出」那条反例。
        let pos = world.actors.get(crafter).expect("刚生成").pos;
        assert!(
            world.terrain_at(pos).is_some(),
            "制作者脚下这一格必须已常驻"
        );
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(station, 1),
            dropped_at: world.clock,
            contents: Vec::new(),
            placed: true,
        });
    }

    let mut timeline = Timeline::new();
    timeline.schedule(crafter, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let catalogs = handle.catalogs(recipes);
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

    let crafter_after = world.actors.get(crafter).expect("制作不会杀死制作者");
    (
        crafter_after.inventory.clone(),
        crafter_after.equipment.clone(),
    )
}

/// 背包里某种物品的总数（跨多堆求和）。
fn count_of(inventory: &[ItemStack], def: ContentIndex) -> u32 {
    inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

#[test]
fn 无前置的烹饪配方经回合引擎真的把生肉变成烤肉() {
    // ADR 0018 的正向证据：内容来自真实 mods/example_mod/crafting.json5，
    // 结算经由 TurnEngine::advance_ai 这条生产路径发生。
    // Arrange
    let handle = load_real_mods();
    let scene = Scene::new(vec![ItemStack::new(handle.raw_meat, 3)]);

    // Act
    let inventory = craft_via_turn_engine(
        &handle,
        &scene,
        handle.roast_meat_recipe,
        &handle.real_recipes(),
    );

    // Assert：食材扣一、成品进包。
    assert_eq!(count_of(&inventory, handle.raw_meat), 2);
    assert_eq!(count_of(&inventory, handle.roast_meat), 1);
}

#[test]
fn 目录从回合引擎摘掉后同一场景里制作不再发生() {
    // 反例守卫：同一段场景、同一个 TurnEngine，只把配方目录换成空的，
    // 背包必须一动不动。没有这一条，上面那条正向测试无法排除
    // 「成品是别的什么东西塞进去的」。
    // Arrange
    let handle = load_real_mods();
    let scene = Scene::new(vec![ItemStack::new(handle.raw_meat, 3)]);

    // Act
    let inventory = craft_via_turn_engine(&handle, &scene, handle.roast_meat_recipe, &NoRecipes);

    // Assert
    assert_eq!(count_of(&inventory, handle.raw_meat), 3);
    assert_eq!(count_of(&inventory, handle.roast_meat), 0);
}

#[test]
fn 三条前置全开的锻造配方在全部满足时产出铁剑() {
    // 副职闸门 + 场地 + 工具三条同时满足的完整路径。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 5)]);
    scene.subclasses = vec![handle.shadowdancer];
    scene
        .equipment
        .insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.war_hammer, 1));
    scene.station_underfoot = Some(handle.portable_anvil);

    // Act
    let inventory = craft_via_turn_engine(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert：两块铁锭换一把剑。
    assert_eq!(count_of(&inventory, handle.iron_ingot), 3);
    assert_eq!(count_of(&inventory, handle.iron_sword), 1);
}

#[test]
fn 刚打出来的剑是满耐久而不是没有耐久概念() {
    // 「新造出来的物品带多少耐久」这条共同规则在**制作**这一路的端到端
    // 证据（`ll_world::item::ItemStack::freshly_made`）：此前这一行是
    // `ItemStack::new(...)`，打出来的剑耐久恒为 None——它此后永远不会
    // 磨损，「武器会坏所以要反复找工匠」这条设计在工匠自己造的装备上
    // 直接落空。同一条规则的另外两个产出点（盲盒、出生装备）各有自己的
    // 端到端证据，见 example_mod_starting_items.rs 与
    // base_mod_items_and_crafting.rs。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 5)]);
    scene.subclasses = vec![handle.shadowdancer];
    scene
        .equipment
        .insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.war_hammer, 1));
    scene.station_underfoot = Some(handle.portable_anvil);

    // Act
    let inventory = craft_via_turn_engine(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert：mods/example_mod/items.json5 的 examplemod:iron_sword
    // 声明 max_durability: 100，刚打出来的这一把就该是 Some(100)。
    let sword = inventory
        .iter()
        .find(|stack| stack.def == handle.iron_sword)
        .expect("上一条断言已经证明剑真的产出了");
    assert_eq!(sword.durability, Some(100));
    // 反面：同一次结算里剩下的铁锭是可堆叠材料，仍然没有耐久概念。
    let ingot = inventory
        .iter()
        .find(|stack| stack.def == handle.iron_ingot)
        .expect("还剩三块铁锭");
    assert_eq!(ingot.durability, None);
}

#[test]
fn 缺少副职时同一条锻造配方静默不产出() {
    // 副职闸门的反例：其余两条前置照旧满足，只把副职拿掉。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 5)]);
    scene
        .equipment
        .insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.war_hammer, 1));
    scene.station_underfoot = Some(handle.portable_anvil);

    // Act
    let inventory = craft_via_turn_engine(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert：食材一块没少——静默失败不消耗任何东西。
    assert_eq!(count_of(&inventory, handle.iron_ingot), 5);
    assert_eq!(count_of(&inventory, handle.iron_sword), 0);
}

#[test]
fn 脚下摆的不是配方要的那件家具时同一条锻造配方静默不产出() {
    // 场地前置的反例：副职与工具都有，只是脚下摆着的是**另一件**家具。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 5)]);
    scene.subclasses = vec![handle.shadowdancer];
    scene
        .equipment
        .insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.war_hammer, 1));
    // 显式摆一件**确定不是**这条配方要的家具，而不是「不写就算了」：
    // 「脚下什么都没摆」与「脚下摆着别的东西」是两条不同的失败路径，
    // 后者才真正证明 `furniture_at` 比的是**这一件**，不是「有没有」。
    scene.station_underfoot = Some(handle.forge);
    assert_ne!(
        handle.forge, handle.portable_anvil,
        "本体锻炉与示例铁砧必须是两件不同的家具，否则本反例无意义"
    );

    // Act
    let inventory = craft_via_turn_engine(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(count_of(&inventory, handle.iron_ingot), 5);
    assert_eq!(count_of(&inventory, handle.iron_sword), 0);
}

/// `mods/example_mod/crafting.json5` 里 `examplemod:war_hammer` 的
/// `register-item` 第六个参数——本文件唯一被配方点名的工具。
const WAR_HAMMER_MAX_DURABILITY: i32 = 150;

#[test]
fn 制作一次之后被点名的工具真的掉了一点耐久() {
    // 耐久扩面批次落地 `crafting-system.md` 九节⑩「工具因制作而磨损」
    // ——该表当初把它标为「与所有者『只有装备武器才有耐久』的裁定直接
    // 冲突」而推迟，那条裁定已被所有者新的裁定推翻：「修理锤子也算是
    // 一种武器，也可以是带有功能性的物品。**只要使用就会减少耐久**。」
    //
    // 走的是 `TurnEngine::advance_ai` 这条生产路径（ADR 0018）。
    // 反例（手工验证过会红）：把 `resolve_craft` 第 9 步那段
    // `if let Some(slot) = equipped_tool { ... }` 删掉，本条立即从
    // `Some(149)` 变回 `Some(150)` 而失败。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 5)]);
    scene.subclasses = vec![handle.shadowdancer];
    scene.equipment.insert(
        EquipSlot::MAIN_HAND,
        ItemStack::with_durability(handle.war_hammer, 1, WAR_HAMMER_MAX_DURABILITY),
    );
    scene.station_underfoot = Some(handle.portable_anvil);

    // Act
    let (inventory, equipment) = craft_via_turn_engine_full(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert：制作确实发生了（否则「没掉耐久」会因为压根没制作而假绿），
    // 且锤子恰好掉了一点。
    assert_eq!(count_of(&inventory, handle.iron_sword), 1);
    assert_eq!(
        equipment
            .get(&EquipSlot::MAIN_HAND)
            .expect("锤子仍在装备栏里——耐久归零都不自动卸下，何况只掉一点")
            .durability,
        Some(WAR_HAMMER_MAX_DURABILITY - 1),
    );
}

#[test]
fn 制作没有发生时工具一点耐久都不掉() {
    // 上一条的反例：同一套场景、同一个 `TurnEngine`，只把副职拿掉,
    // 制作在第 3 步就静默失败。「只要使用就会减少耐久」的前提是**真的
    // 用了**——站错地方、缺副职、缺料这类白试一次不该产生任何损失,
    // 见 `resolve_craft` 文档「只在制作**真的发生**时磨损」一节。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 5)]);
    scene.equipment.insert(
        EquipSlot::MAIN_HAND,
        ItemStack::with_durability(handle.war_hammer, 1, WAR_HAMMER_MAX_DURABILITY),
    );
    scene.station_underfoot = Some(handle.portable_anvil);

    // Act
    let (inventory, equipment) = craft_via_turn_engine_full(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(count_of(&inventory, handle.iron_sword), 0);
    assert_eq!(
        equipment.get(&EquipSlot::MAIN_HAND).unwrap().durability,
        Some(WAR_HAMMER_MAX_DURABILITY),
    );
}

#[test]
fn 没有耐久概念的工具制作后不会被凭空赋予耐久() {
    // 第三条反例：工具堆的 `durability == None` 时「工具磨损」这一步
    // 完全不产出效果（`equipped_tool` 恒 `None`），`None` 必须原样保持
    // `None`——内容作者完全可以声明一件永不磨损的工具。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 5)]);
    scene.subclasses = vec![handle.shadowdancer];
    scene
        .equipment
        .insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.war_hammer, 1));
    scene.station_underfoot = Some(handle.portable_anvil);

    // Act
    let (inventory, equipment) = craft_via_turn_engine_full(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(count_of(&inventory, handle.iron_sword), 1);
    assert_eq!(
        equipment.get(&EquipSlot::MAIN_HAND).unwrap().durability,
        None
    );
}

#[test]
fn 耐久归零的工具装着也打不了铁() {
    // 工具前置里最容易写错的那一半：只比 def 相等会让「锤子烂了还能
    // 打铁」成立。这条把 `durability != Some(0)` 这半个谓词钉住，与
    // `derive_stats`「耐久归零的装备不再贡献加成」是同一条既有语义。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 5)]);
    scene.subclasses = vec![handle.shadowdancer];
    scene.equipment.insert(
        EquipSlot::MAIN_HAND,
        ItemStack::with_durability(handle.war_hammer, 1, 0),
    );
    scene.station_underfoot = Some(handle.portable_anvil);

    // Act
    let inventory = craft_via_turn_engine(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(count_of(&inventory, handle.iron_ingot), 5);
    assert_eq!(count_of(&inventory, handle.iron_sword), 0);
}

#[test]
fn 食材不足时不消耗任何食材也不产出() {
    // ⑥食材校验必须在⑦扣减之前整体做完：缺料时一块铁锭都不能少。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 1)]);
    scene.subclasses = vec![handle.shadowdancer];
    scene
        .equipment
        .insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.war_hammer, 1));
    scene.station_underfoot = Some(handle.portable_anvil);

    // Act：这条配方要两块铁锭，只有一块。
    let inventory = craft_via_turn_engine(
        &handle,
        &scene,
        handle.iron_sword_recipe,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(count_of(&inventory, handle.iron_ingot), 1);
    assert_eq!(count_of(&inventory, handle.iron_sword), 0);
}

#[test]
fn 一次产出多份的配方真的产出多份() {
    // product_count > 1 的路径——箭一次出五支。
    // Arrange
    let handle = load_real_mods();
    let mut scene = Scene::new(vec![ItemStack::new(handle.iron_ingot, 2)]);
    scene.subclasses = vec![handle.shadowdancer];

    // Act
    let inventory = craft_via_turn_engine(
        &handle,
        &scene,
        handle.arrow_batch_recipe,
        &handle.real_recipes(),
    );

    // Assert
    assert_eq!(count_of(&inventory, handle.iron_ingot), 1);
    assert_eq!(count_of(&inventory, handle.arrow), 5);
}
