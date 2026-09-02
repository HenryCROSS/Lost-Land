//! `Intent::TendTree` 的结算：砍伐、培植、采果三条路，以及各自的闸门。
//!
//! 只用公开入口（`resolve_with_catalogs` / `apply`），不碰任何私有函数
//! ——与 `trade.rs`、`dialogue_choose.rs` 同一条纪律。
//!
//! # 本文件咬住的几条
//!
//! | 能力 | 断言 |
//! |---|---|
//! | 砍伐：树没了、木料到手 | `砍倒一棵树之后树没了木料到手` |
//! | **树种决定木料产量** | `不同树种砍出的木料数量不同` |
//! | 采果：树留着、种子到手 | `采一次果树还在种子到手` |
//! | **采过就采不动了** | `果子没长好时采不动` |
//! | 培植：种子换一棵树 | `种下一颗种子长出一棵树` |
//! | **没种子种不下** | `背包里没有种子时种不下` |
//! | **有树的格子种不下第二棵** | `已经有树的格子种不下第二棵` |
//! | 非森林地形整条不成立 | `非森林地形上三条路全都零效果` |
//! | 够不着就不成立 | `够不着的树砍不动` |
//! | **不接树木目录就零效果** | `不接树木目录时砍伐零效果` |
//! | 三条路都消耗一个回合 | `砍种采三条路都消耗一个回合` |

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::scaled::Milli;
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};
use ll_sim::apply::apply;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::combat::Penetration;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::item::{ItemCatalog, ItemRule, SlotMask, WearChannels};
use ll_sim::resolve::resolve_with_catalogs;
use ll_sim::tree::{NoTrees, TreeAction, TreeCatalog};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::item::ItemStack;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::BaseTerrainIds;
use ll_world::tree::{TreeDeviation, TreeSpecies, tree_at};
use ll_world::zone::ZoneLayout;

/// 三个索引：森林地形、木料、树种。**不能三个都随手造**——`forest_terrain`
/// 那一条要与世界里真实的 `TerrainKind` 对上，因此三条全部来自
/// [`地形与两件物品`] 那一个 `Interner`，见它的文档。
struct 树木目录 {
    forest: ContentIndex,
    timber: ContentIndex,
    seed: ContentIndex,
}

impl TreeCatalog for 树木目录 {
    fn forest_terrain(&self) -> Option<ContentIndex> {
        Some(self.forest)
    }
    fn timber(&self) -> Option<ContentIndex> {
        Some(self.timber)
    }
    fn tree_seed(&self) -> Option<ContentIndex> {
        Some(self.seed)
    }
}

/// 一份只回答堆叠上限的最小物品目录——木料与种子都要能堆起来。
struct 材料目录;

impl ItemCatalog for 材料目录 {
    fn item(&self, _item: ContentIndex) -> Option<ItemRule> {
        Some(ItemRule {
            stack_limit: 50,
            base_price: Milli(0),
            equip_mask: SlotMask::EMPTY,
            stat_bonuses: Vec::new(),
            use_effect: None,
            penetration: Penetration::NONE,
            damage_formula: None,
            damage_category: None,
            rule_modifiers: Vec::new(),
            wear_channels: WearChannels::NONE,
            max_durability: None,
            taught_recipes: Vec::new(),
            requires_identification: false,
            study_experience: 0,
            blind_box_pool: Vec::new(),
            furniture: false,
        })
    }
}

fn 目录<'a>(trees: &'a 树木目录, items: &'a 材料目录) -> ResolveCatalogs<'a> {
    ResolveCatalogs {
        items,
        trees,
        ..ResolveCatalogs::empty()
    }
}

/// 自己持 `Interner` 而不是用 `base_terrain_fixture()`：本文件要两个
/// **与全部地形索引都不同**的物品索引（木料、树种），而 `ContentIndex`
/// 没有公开的裸构造器（ADR 0015：裸 `u32` 没有任何不变式，所以它不给
/// 一个「凭空造一个索引」的入口）。先注册全部地形、再 intern 两个新
/// 名字，两条索引因此天然排在地形之后。
fn 地形与两件物品() -> (
    BaseTerrainIds,
    ll_world::terrain::TerrainTable,
    ContentIndex,
    ContentIndex,
) {
    let mut interner = Interner::new();
    let (ids, table) = ll_world::terrain::materialize_base_terrain(&mut |id| interner.intern(id))
        .expect("本体地形声明表内部一致，注册恒不失败");
    let timber = interner.intern(NamespacedId::parse("lostland:timber_log").expect("合法标识符"));
    let seed = interner.intern(NamespacedId::parse("lostland:tree_seed").expect("合法标识符"));
    (ids, table, timber, seed)
}

fn spawn_agent(world: &mut WorldState, at: TorusPos) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let (zone, _) = world.terrain.layout().tile_to_zone(at);
    world.actors.spawn(Agent {
        gender: ll_world::entity::Gender::default(),
        pos: at,
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
        inventory: Vec::new(),
        equipment: BTreeMap::new(),
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

/// 一个玩家站在 `(5,5)`，脚下与身边那一格都被**强制铺成森林**。
///
/// 强制铺地形而不是「在世界里找一格森林」：这个 64×64 的测试世界是不是
/// 恰好在 `(5,5)` 附近有森林，取决于噪声——依赖那种巧合的测试会在换一次
/// 生成参数之后静默失去覆盖面（[ADR 0022] 的「判据适用面被绕过」）。
///
/// [ADR 0022]: ../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md
fn 一个站在林子里的人() -> (WorldState, EntityId, TorusPos, 树木目录) {
    let (ids, terrain_table, timber, seed) = 地形与两件物品();
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
    let spawn = layout.tile_size().wrap(0, 0);
    let mut world = WorldState::new(layout, &GenParams::default(), &ids, terrain_table, spawn)
        .expect("测试布局满足全部构造前置条件");
    let here = world.size.wrap(5, 5);
    let actor = spawn_agent(&mut world, here);
    world.player_entity = Some(actor);
    for (dx, dy) in [(0, 0), (1, 0), (0, 1)] {
        let pos = world.size.wrap(5 + dx, 5 + dy);
        world.terrain.set_terrain(pos, ids.forest);
    }
    let catalog = 树木目录 {
        forest: ids.forest.index(),
        timber,
        seed,
    };
    (world, actor, here, catalog)
}

/// 把 `pos` 那一格强行变成一棵指定树种、果子已长好的树。
///
/// 写偏差记录而不是「找一格派生出这个树种的位置」——同上，不依赖噪声
/// 的巧合。这也顺带演示了「单棵想特殊化可以」那条能力。
fn 摆一棵树(world: &mut WorldState, pos: TorusPos, species: TreeSpecies) {
    world.trees.set(
        pos,
        TreeDeviation {
            species: Some(species),
            harvested_at: None,
        },
    );
}

fn 跑一次(
    world: &mut WorldState,
    actor: EntityId,
    pos: TorusPos,
    action: TreeAction,
    trees: &树木目录,
) -> Vec<Effect> {
    let items = 材料目录;
    let effects = resolve_with_catalogs(
        world,
        &Intent::TendTree {
            actor,
            pos: (pos.x(), pos.y()),
            action,
        },
        &目录(trees, &items),
    );
    for effect in &effects {
        apply(world, effect);
    }
    effects
}

fn 背包里的(world: &WorldState, actor: EntityId, def: ContentIndex) -> u32 {
    world
        .actors
        .get(actor)
        .expect("实体存在")
        .inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

// ───────────────────────────── 砍伐 ─────────────────────────────

#[test]
fn 砍倒一棵树之后树没了木料到手() {
    // Arrange
    let (mut world, actor, here, catalog) = 一个站在林子里的人();
    摆一棵树(&mut world, here, TreeSpecies::Oak);
    assert!(
        tree_at(
            &world,
            here,
            ll_world::terrain::TerrainKind::from_index(catalog.forest)
        )
        .is_some(),
        "前提：动手之前这一格真的有树"
    );

    // Act
    跑一次(&mut world, actor, here, TreeAction::Fell, &catalog);

    // Assert
    assert!(
        tree_at(
            &world,
            here,
            ll_world::terrain::TerrainKind::from_index(catalog.forest)
        )
        .is_none(),
        "砍完树还在"
    );
    assert_eq!(
        背包里的(&world, actor, catalog.timber),
        TreeSpecies::Oak.timber_yield(),
        "橡树该出 3 份木料"
    );
}

#[test]
fn 不同树种砍出的木料数量不同() {
    // **「多树种」在玩法上唯一真实的差异**（贴图之外）。反例验证
    // （已实跑）：把 `TreeSpecies::timber_yield` 三支改成同一个数，本条
    // 当场红。
    //
    // 遍历 `TreeSpecies::ALL` 而不是点名三个：加第四种树时这条自动开始
    // 管它（ADR 0022 的「判据适用面被新代码绕过」）。
    let mut yields = Vec::new();
    for species in TreeSpecies::ALL {
        let (mut world, actor, here, catalog) = 一个站在林子里的人();
        摆一棵树(&mut world, here, species);
        跑一次(&mut world, actor, here, TreeAction::Fell, &catalog);
        yields.push((species, 背包里的(&world, actor, catalog.timber)));
    }
    for (species, got) in &yields {
        assert_eq!(*got, species.timber_yield(), "{species:?} 的木料产量对不上");
        assert!(*got > 0, "{species:?} 砍下来一份木料都没有");
    }
    let 各不相同: std::collections::BTreeSet<u32> = yields.iter().map(|(_, n)| *n).collect();
    assert_eq!(
        各不相同.len(),
        TreeSpecies::ALL.len(),
        "三种树砍出的木料数量应当互不相同，实测 {yields:?}"
    );
}

// ───────────────────────────── 采果 ─────────────────────────────

#[test]
fn 采一次果树还在种子到手() {
    // Arrange
    let (mut world, actor, here, catalog) = 一个站在林子里的人();
    摆一棵树(&mut world, here, TreeSpecies::Palm);
    world.clock = Tick(500);

    // Act
    跑一次(&mut world, actor, here, TreeAction::Harvest, &catalog);

    // Assert：**树还在**（采果不是砍伐——这是本函数最容易写错的一处）。
    let forest = ll_world::terrain::TerrainKind::from_index(catalog.forest);
    let tree = tree_at(&world, here, forest).expect("采完果树该还在");
    assert_eq!(tree.species, TreeSpecies::Palm);
    assert!(!tree.fruit_ready, "刚采过，果子不该立刻又长好");
    assert_eq!(背包里的(&world, actor, catalog.seed), 1, "该拿到一颗种子");
}

#[test]
fn 果子没长好时采不动() {
    // Arrange
    let (mut world, actor, here, catalog) = 一个站在林子里的人();
    摆一棵树(&mut world, here, TreeSpecies::Palm);
    world.clock = Tick(500);
    跑一次(&mut world, actor, here, TreeAction::Harvest, &catalog);
    assert_eq!(
        背包里的(&world, actor, catalog.seed),
        1,
        "前提：第一次采到了"
    );

    // Act：立刻再采一次。
    let effects = 跑一次(&mut world, actor, here, TreeAction::Harvest, &catalog);

    // Assert：**零效果**，不是「采到第二颗」也不是 panic。
    assert!(effects.is_empty(), "果子没长好时该零效果，实际 {effects:?}");
    assert_eq!(背包里的(&world, actor, catalog.seed), 1);

    // 对照组：等满一个周期就又采得到——否则上面那条可以被一个
    // 「采果永远失败」的实现满足（ADR 0022 的判据退化）。
    world.clock = Tick(500 + ll_world::tree::FRUIT_REGROW_TICKS);
    跑一次(&mut world, actor, here, TreeAction::Harvest, &catalog);
    assert_eq!(背包里的(&world, actor, catalog.seed), 2, "长好之后该采得到");
}

// ───────────────────────────── 培植 ─────────────────────────────

#[test]
fn 种下一颗种子长出一棵树() {
    // Arrange：一格空森林 + 背包里一颗种子。
    let (mut world, actor, here, catalog) = 一个站在林子里的人();
    world.trees.set(here, TreeDeviation::felled());
    world
        .actors
        .get_mut(actor)
        .expect("实体存在")
        .inventory
        .push(ItemStack::new(catalog.seed, 1));
    world.clock = Tick(7);

    // Act
    跑一次(&mut world, actor, here, TreeAction::Plant, &catalog);

    // Assert
    let forest = ll_world::terrain::TerrainKind::from_index(catalog.forest);
    let tree = tree_at(&world, here, forest).expect("种下之后该有树");
    // **长出什么树由那块地的气候决定**——与派生层同一个函数，因此这里
    // 拿派生层的答案作对照，不在测试里另抄一份规则（ADR 0021）。
    assert_eq!(
        tree.species,
        ll_world::tree::derived_species_at(
            world.seed,
            here,
            world.size.height(),
            world.terrain_shape.climate_band_width
        ),
        "种出来的树种应当等于那块地的气候树种"
    );
    assert_eq!(背包里的(&world, actor, catalog.seed), 0, "种子该被消耗掉");
}

#[test]
fn 背包里没有种子时种不下() {
    let (mut world, actor, here, catalog) = 一个站在林子里的人();
    world.trees.set(here, TreeDeviation::felled());

    let effects = 跑一次(&mut world, actor, here, TreeAction::Plant, &catalog);

    assert!(effects.is_empty(), "没有种子时该零效果，实际 {effects:?}");
    let forest = ll_world::terrain::TerrainKind::from_index(catalog.forest);
    assert!(tree_at(&world, here, forest).is_none(), "凭空长出了一棵树");
}

#[test]
fn 已经有树的格子种不下第二棵() {
    let (mut world, actor, here, catalog) = 一个站在林子里的人();
    摆一棵树(&mut world, here, TreeSpecies::Oak);
    world
        .actors
        .get_mut(actor)
        .expect("实体存在")
        .inventory
        .push(ItemStack::new(catalog.seed, 1));

    let effects = 跑一次(&mut world, actor, here, TreeAction::Plant, &catalog);

    assert!(effects.is_empty(), "有树的格子该零效果，实际 {effects:?}");
    assert_eq!(背包里的(&world, actor, catalog.seed), 1, "种子不该被白吃掉");
}

// ───────────────────────────── 共用闸门 ─────────────────────────────

#[test]
fn 非森林地形上三条路全都零效果() {
    // 「`forest` 地形保留当底图」是项目所有者的要求原话。
    let (mut world, actor, here, catalog) = 一个站在林子里的人();
    let (ids, ..) = 地形与两件物品();
    摆一棵树(&mut world, here, TreeSpecies::Oak);
    world
        .actors
        .get_mut(actor)
        .expect("实体存在")
        .inventory
        .push(ItemStack::new(catalog.seed, 1));
    // 把脚下改成草地——**偏差记录仍然在**，也就是说这条闸门挡的确实是
    // 地形，不是「那里本来就没树」。
    world.terrain.set_terrain(here, ids.grass);

    for action in [TreeAction::Fell, TreeAction::Harvest, TreeAction::Plant] {
        let effects = 跑一次(&mut world, actor, here, action, &catalog);
        assert!(
            effects.is_empty(),
            "草地上的 {action:?} 该零效果，实际 {effects:?}"
        );
    }
}

#[test]
fn 够不着的树砍不动() {
    // 范围判据与 `Intent::PickUp` **调的是同一个函数**（ADR 0021），
    // 本条只确认它真的被调到了。
    let (mut world, actor, _here, catalog) = 一个站在林子里的人();
    let 远处 = world.size.wrap(30, 30);
    let (ids, ..) = 地形与两件物品();
    world.terrain.set_terrain(远处, ids.forest);
    摆一棵树(&mut world, 远处, TreeSpecies::Pine);

    let effects = 跑一次(&mut world, actor, 远处, TreeAction::Fell, &catalog);

    assert!(effects.is_empty(), "够不着的树该零效果，实际 {effects:?}");
    // 对照组：**同一棵树**搬到身边就砍得动——否则上面那条可以被一个
    // 「砍伐永远失败」的实现满足。
    let 身边 = world.size.wrap(6, 5);
    摆一棵树(&mut world, 身边, TreeSpecies::Pine);
    let effects = 跑一次(&mut world, actor, 身边, TreeAction::Fell, &catalog);
    assert!(!effects.is_empty(), "身边那棵该砍得动");
}

#[test]
fn 不接树木目录时砍伐零效果() {
    // `NoTrees` 三条查询全 `None` ⇒ 这个世界里没有树可以砍。
    // **这条不是形式主义**：`ll-game` 那一行接线漏掉时就是这个状态，
    // 而那种漏接正是仓库记过三次的「只在测试里成立的接线」。
    let (mut world, actor, here, _catalog) = 一个站在林子里的人();
    摆一棵树(&mut world, here, TreeSpecies::Oak);
    let items = 材料目录;

    let effects = resolve_with_catalogs(
        &world,
        &Intent::TendTree {
            actor,
            pos: (here.x(), here.y()),
            action: TreeAction::Fell,
        },
        &ResolveCatalogs {
            items: &items,
            trees: &NoTrees,
            ..ResolveCatalogs::empty()
        },
    );

    assert!(
        effects.is_empty(),
        "不接树木目录时该零效果，实际 {effects:?}"
    );
}

#[test]
fn 砍种采三条路都消耗一个回合() {
    // 不计费的话，玩家可以在一个回合内把整片林子砍光。反例验证
    // （已实跑）：删掉 `resolve_tend_tree` 里那条 `Effect::ScheduleNext`，
    // 本条当场红。
    for action in [TreeAction::Fell, TreeAction::Harvest, TreeAction::Plant] {
        let (mut world, actor, here, catalog) = 一个站在林子里的人();
        match action {
            TreeAction::Plant => {
                world.trees.set(here, TreeDeviation::felled());
                world
                    .actors
                    .get_mut(actor)
                    .expect("实体存在")
                    .inventory
                    .push(ItemStack::new(catalog.seed, 1));
            }
            _ => 摆一棵树(&mut world, here, TreeSpecies::Oak),
        }
        let effects = 跑一次(&mut world, actor, here, action, &catalog);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::ScheduleNext { .. })),
            "{action:?} 没有产出 ScheduleNext，实际 {effects:?}"
        );
    }
}
