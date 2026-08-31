//! Mod 装载管线的端到端回归：真实 `mods/` 目录 + 两个失败夹具目录。
//!
//! # 出处（2026-08-29 批次 13）
//!
//! 本文件搬自 `crates/ll-ui/examples/p4_acceptance/world.rs`。所有者裁定
//! 去掉 `examples/`（见 `knowledge/decisions/0030-remove-examples-acceptance-demos.md`），
//! 那个 demo 的八条断言是全仓库**唯一**跑仓库根目录下
//! `mods_missing_dependency/` 与 `mods_duplicate_namespace/` 两个真实夹具
//! 目录的地方——`ll-mod/src/pipeline.rs` 的单测走的是临时目录造出来的
//! 失败样本，不覆盖这两个目录本身。搬迁**逐字保留了全部八条断言**，
//! 只删掉了 demo 专用的呈现字段（`example_mod_manifest`，原本喂给
//! demo 的 'R' 键一键重载）。
//!
//! 这条链路是：`register_base_terrain` → `load_all`（跑三次，分别对应
//! 正常 mod / 缺失依赖 / 重复命名空间三批目录）→ `Registry`/`TerrainTable`
//! → `WorldState` → 玩家可查询/可行走的地形属性，一条链路里没有任何
//! 为测试单开的旁路。
//!
//! # 为什么宿主在 `ll-ui` 而不是 `ll-mod`
//!
//! 沿用 demo 的宿主 crate，搬迁不改依赖方向：`ll-mod` 不依赖 `ll-ui`，
//! 而这条链路要同时用到 `ll-mod::pipeline` 与 `ll-world::state`，
//! `ll-ui` 两者都够得到。真要换宿主是另一次改动，不该混进搬迁批次。

use std::path::Path;

use ll_core::ident::{Interner, NamespacedId};
use ll_core::torus::{TorusPos, TorusSize};
use ll_mod::base_terrain::register_base_terrain;
use ll_mod::behavior_binding::ClassBehaviorBindings;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadReport;
use ll_mod::modifier_type::ModifierTypeTable;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::state::WorldState;
use ll_world::terrain::TerrainKind;
use ll_world::zone::ZoneLayout;

/// 演示世界的宽度（格）——与原 demo 的 `layout::WORLD_WIDTH` 同值。
const WORLD_WIDTH: u32 = 64;

/// 演示世界的高度（格）——与原 demo 的 `layout::WORLD_HEIGHT` 同值。
const WORLD_HEIGHT: u32 = 64;

/// 世界时钟的初始刻度：正午——与原 demo 的 `layout::INITIAL_CLOCK_TICKS`
/// 同值。地形属性与光照无关，这里保留它只是为了让搬迁前后的世界状态
/// 逐位一致。
const INITIAL_CLOCK_TICKS: i64 = 12 * ll_core::time::TICKS_PER_HOUR;

/// 区块边长（格）：取世界边长本身，demo 世界因此正好是单个区块（与
/// `WORLD_WIDTH == WORLD_HEIGHT` 一致）——理由与 `p3_acceptance` 同一
/// 常量选择一致，见其 `spawn.rs` 文档。
const ZONE_SPAN: u32 = WORLD_WIDTH;

fn build_zone_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(ZONE_SPAN, zone_count).expect("ZONE_SPAN 满足全部对齐与跨度约束")
}

/// 示例 mod 注册的熔岩地板 id。
const LAVA_FLOOR_ID: &str = "examplemod:lava_floor";

/// 出生点搜索的最大环半径——与 p2/p3_acceptance 同一算法，取世界较小
/// 维度的一半，保证除非整张地图没有一格可站立，否则恒能找到。
const SEARCH_MAX_RADIUS: i32 = (if WORLD_WIDTH < WORLD_HEIGHT {
    WORLD_WIDTH
} else {
    WORLD_HEIGHT
} / 2) as i32;

/// 装载完毕的世界：世界状态本身、本体地形索引缓存、加载报告、熔岩
/// 地板的地形索引（`None` 表示 examplemod 这次没能成功注册它——
/// 三种故意写错的 mod 都不会影响到这一个，但如实处理这个可能性，不
/// 假设它必然存在）。
struct DemoWorld {
    world: WorldState,
    report: LoadReport,
    lava_kind: Option<TerrainKind>,
    player: EntityId,
    /// `mods/example_mod/classes.json5` 声明的亡灵法师职业的主属性
    /// 倾向——P5-C 缺口修补批次新增，证明玩法层内容声明在完整装载管线
    /// （不只是孤立的单元测试）里也确实生效。`None` 表示这次没能成功
    /// 注册（如实处理，理由同 `lava_kind` 字段文档）。
    necromancer_primary_attribute: Option<ll_world::entity::AttributeKind>,
}

/// 三个 mod 根目录相对本 crate `Cargo.toml` 的路径。分成三个独立目录
/// 的理由见各自 `mod.json5` 里的注释：拓扑排序对缺失依赖/成环/重复
/// 命名空间是「整批中止」的，混进同一个目录会让其他示例 mod 一起被
/// 判失败。
const PRIMARY_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");
const MISSING_DEPENDENCY_ROOT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods_missing_dependency");
const DUPLICATE_NAMESPACE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../mods_duplicate_namespace"
);

/// 搭建演示世界：注册本体地形、跑三次装载管线（分别对应「正常 mod」
/// 「缺失依赖」「重复命名空间」三批目录）、生成地形、出生玩家、把
/// 熔岩地板铺在玩家出生点附近。
fn build_demo_world() -> DemoWorld {
    let mut registry = Registry::new();
    let (terrain_ids, mut table) =
        register_base_terrain(&mut registry).expect("本体地形声明表内部一致，注册恒不失败");
    // P4 demo 本身只演示地形（P5 才引入职业/技能/副职/任务/种族/动画
    // 剪辑），但 `load_all` 的签名要求七张表一起传——见 `ll_mod::pipeline::
    // GameplayTables` 文档：六张非地形表在这里只是陪跑的空表，不影响
    // demo 的验收范围。
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
    let mut resource_pool = ll_mod::resource_pool::ResourcePoolTable::new();
    let mut item = ItemTable::new();
    let mut formula = ll_mod::formula::FormulaTable::new();
    let mut weapon_category = ll_mod::weapon_category::WeaponCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut resource_table = ll_world::resource::ResourceTable::new();
    let mut culture_table = ll_world::culture::CultureTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut tag_table = ll_mod::tag::TagTable::new();
    let mut damage_category = ll_mod::damage_category::DamageCategoryTable::new();
    let mut modifier_type_table = ModifierTypeTable::new();

    let mut report = load_all(
        Path::new(PRIMARY_MODS_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut table,
            class: &mut class,
            skill: &mut skill,
            subclass: &mut subclass,
            quest: &mut quest,
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
    let missing_dependency_report = load_all(
        Path::new(MISSING_DEPENDENCY_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut table,
            class: &mut class,
            skill: &mut skill,
            subclass: &mut subclass,
            quest: &mut quest,
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
    let duplicate_namespace_report = load_all(
        Path::new(DUPLICATE_NAMESPACE_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut table,
            class: &mut class,
            skill: &mut skill,
            subclass: &mut subclass,
            quest: &mut quest,
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
    report.entries.extend(missing_dependency_report.entries);
    report.entries.extend(duplicate_namespace_report.entries);

    let lava_kind = registry
        .get(&NamespacedId::parse(LAVA_FLOOR_ID).expect("字面量恒合法"))
        .map(TerrainKind::from_index);
    let necromancer_primary_attribute = registry
        .get(&NamespacedId::parse("examplemod:necromancer").expect("字面量恒合法"))
        .and_then(|index| class.get(index))
        .map(|view| view.primary_attribute);

    let layout = build_zone_layout();
    let placeholder_spawn = layout.tile_size().wrap(0, 0);
    let mut world = WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        table,
        placeholder_spawn,
    )
    .expect("演示世界布局满足生成入口的全部约束");
    world.advance(INITIAL_CLOCK_TICKS);

    // 交叉引用校验（规格 §10.6 六阶段的最后一步）：整张地图上出现的
    // 每一个地形索引，此刻是否都能在 world.terrain_table 里查到定义。
    // 这一步必须放在铺熔岩地板**之后**才有意义——校验的正是「刚刚
    // 手动写进网格的那些索引」也在表里登记过，不是走个过场。
    let player_pos = find_walkable_near(&world, world.size.wrap(world.size.width() as i32 / 2, 1));
    if let Some(lava) = lava_kind {
        place_lava_patch(&mut world, player_pos, lava);
    }
    // WorldState::hash 面对同一处架构变化选择「只校验常驻区块」，见其
    // 文档；这里同理改用 SurfaceStore::validate_resident——demo 世界是
    // 单区块布局，此刻整体常驻（WorldState::new 的出生点邻域预热已
    // 覆盖），实际校验范围与迁移前遍历整个世界等价。
    report.cross_validate = Some(
        world
            .terrain
            .validate_resident(&world.terrain_table)
            .map_err(|err| err.to_string()),
    );

    let player = spawn_player(&mut world, player_pos);

    DemoWorld {
        world,
        report,
        lava_kind,
        player,
        necromancer_primary_attribute,
    }
}

/// 从玩家出生点**正东紧邻一格**开始铺一小片（3×2）熔岩地板。
///
/// **实测撞见的真实缺陷**：早期版本从东偏移 3 格开始铺（见提交历史），
/// 出生点与熔岩地板之间隔着 `find_walkable_near` 从未检查过的地形——
/// 出生点搜索只保证出生点本身可站立，不保证它与任意远处的另一格之间
/// 存在一条可通行路径；这次生成的地图上两者之间恰好隔着深水，验收
/// demo 用真实窗口驱动方向键截图时，连续按右方向键三次玩家始终停在
/// 原地（P3 交接强调的纪律再次应验：只有真正跑起来才会暴露断链，
/// 单元测试测的是"熔岩地板本身属性正确"，测不出"玩家是否真的够得到
/// 它"）。现在从 `dx = 1`（出生点正东紧邻一格）开始铺，不再依赖中间
/// 地形是否可通行——覆盖掉的无论原来是什么地形，写入后都会变成可通行
/// 的熔岩地板，且因为紧邻出生点，一次方向键按下就能走上去。
fn place_lava_patch(world: &mut WorldState, origin: TorusPos, lava: TerrainKind) {
    for dy in 0..2 {
        for dx in 1..4 {
            let pos = world.size.wrap(origin.x() + dx, origin.y() + dy);
            world.terrain.set_terrain(pos, lava);
        }
    }
}

/// 该地形是否可站立。
///
/// # 为什么接受 `&WorldState`（两级坐标系重写，任务 11）
///
/// 见 `p2_acceptance::spawn::is_spawnable` 文档同一节：demo 世界是
/// 单区块布局（[`ZONE_SPAN`] = 世界边长），`WorldState::new` 自带的
/// 出生点邻域预热已经让它整体常驻，`.expect(..)` 因此总能成立。
fn is_walkable(world: &WorldState, pos: TorusPos) -> bool {
    let kind = world
        .terrain_at(pos)
        .expect("demo 世界是单区块布局，WorldState::new 的出生点邻域预热已让它整体常驻");
    !kind.blocks_move(&world.terrain_table)
}

/// 从 `target` 起按环逐圈向外搜索一格可站立的地形——与 p2/p3_acceptance
/// 的 `find_spawn`/`find_walkable_near` 同一算法。
fn find_walkable_near(world: &WorldState, target: TorusPos) -> TorusPos {
    let size = world.size;
    if is_walkable(world, target) {
        return target;
    }
    for radius in 1..=SEARCH_MAX_RADIUS {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs().max(dy.abs()) != radius {
                    continue;
                }
                let pos = size.wrap(target.x() + dx, target.y() + dy);
                if is_walkable(world, pos) {
                    return pos;
                }
            }
        }
    }
    target
}

/// 生成玩家单位，写入 `world.actors`。
///
/// `current_space` 取地表——demo 世界本次（P4）不放置任何 `Interior`
/// 入口，进出 `Interior` 是任务 15 验收 demo 才展示的场景（见其
/// `world.rs`），层属性索引这里用占位值即可。
fn spawn_player(world: &mut WorldState, pos: TorusPos) -> EntityId {
    let mut interner = Interner::new();
    let profession =
        interner.intern(NamespacedId::parse("lostland:wanderer").expect("demo 内置标识符恒合法"));
    let race =
        interner.intern(NamespacedId::parse("lostland:human").expect("demo 内置标识符恒合法"));
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: ll_core::time::Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: ll_world::space::Space::surface(
            zone,
            ll_core::ident::ContentIndex::default(),
        ),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

mod tests {
    use super::*;
    use ll_mod::load_report::LoadStatus;

    #[test]
    fn 示例mod的熔岩地板成功注册且能在世界里查到属性() {
        // Arrange & Act
        let demo = build_demo_world();

        // Assert
        let lava = demo.lava_kind.expect("examplemod:lava_floor 应当成功注册");
        assert!(!lava.blocks_move(&demo.world.terrain_table));
        assert!(!lava.blocks_sight(&demo.world.terrain_table));
        assert!(lava.move_cost(&demo.world.terrain_table) > 100);
    }

    #[test]
    fn 故意写错的mod归入失败分组() {
        // Arrange & Act
        let demo = build_demo_world();

        // Assert：brokendependency 应该能在报告里找到，且是 Failed。
        //
        // 此前这里还有 brokensyntax/brokenwhitelist 两个夹具（脚本语法
        // 错误、白名单拒绝）。脚本系统整体拆除后这两类失败不再存在，
        // 两个夹具目录连同它们的 `.scm` 一起删掉了；「加载管理界面能
        // 展示失败条目」这件事由拓扑排序失败这一档继续担着。
        let status = demo
            .report
            .entries
            .iter()
            .find(|(id, _)| id.namespace() == "brokendependency")
            .map(|(_, status)| status)
            .expect("报告里应当有 brokendependency 的条目");
        assert!(
            matches!(status, LoadStatus::Failed(_)),
            "brokendependency 应当归入失败分组，实际 {status:?}"
        );
    }

    #[test]
    fn 示例mod的亡灵法师职业通过完整装载管线成功注册() {
        // P5-C 缺口修补批次：证明 register-class 不只是在孤立的单元
        // 测试里能被脚本调用，在真实的「发现→解析→拓扑排序→加载脚本→
        // 注册内容」完整管线里同样生效。
        // Arrange & Act
        let demo = build_demo_world();

        // Assert
        assert_eq!(
            demo.necromancer_primary_attribute,
            Some(ll_world::entity::AttributeKind::Willpower)
        );
    }

    #[test]
    fn 正常mod加载成功不受其余目录里的错误mod连累() {
        // Arrange & Act
        let demo = build_demo_world();

        // Assert
        let status = demo
            .report
            .entries
            .iter()
            .find(|(id, _)| id.namespace() == "examplemod")
            .map(|(_, status)| status);
        assert_eq!(status, Some(&LoadStatus::Loaded));
    }

    #[test]
    fn 重复命名空间的两个mod都归入失败分组() {
        // Arrange & Act
        let demo = build_demo_world();

        // Assert
        let dup_entries: Vec<_> = demo
            .report
            .entries
            .iter()
            .filter(|(id, _)| id.namespace() == "dup")
            .collect();
        assert_eq!(dup_entries.len(), 2);
        for (_, status) in dup_entries {
            assert!(matches!(status, LoadStatus::Failed(_)));
        }
    }

    #[test]
    fn 交叉引用校验通过() {
        // 铺熔岩地板之后整张地图的地形索引都应当能在当前表里查到——
        // 这是规格 §10.6 六阶段最后一步的直接回归。
        // Arrange & Act
        let demo = build_demo_world();

        // Assert
        assert_eq!(demo.report.cross_validate, Some(Ok(())));
    }

    #[test]
    fn 玩家出生点可以站立() {
        // Arrange & Act
        let demo = build_demo_world();
        let player_pos = demo
            .world
            .actors
            .get(demo.player)
            .expect("刚生成的玩家必然存在")
            .pos;

        // Assert
        let kind = demo
            .world
            .terrain_at(player_pos)
            .expect("demo 世界是单区块布局，已整体常驻");
        assert!(!kind.blocks_move(&demo.world.terrain_table));
    }

    #[test]
    fn 玩家出生点正东紧邻一格就是熔岩地板一步可达() {
        // 这是真实撞见过的回归：早期版本把熔岩地板铺在出生点东偏移
        // 3~6 格处，中间隔着的地形不保证可通行——用真实窗口驱动方向键
        // 截图时，玩家连续按右三次纹丝不动（隔着深水）。这条断言直接
        // 钉住"出生点正东紧邻一格"这个约束，任何人若把偏移量改回一个
        // 不紧邻出生点的值，这里会立刻变红，而不必再靠一次手工截图
        // 才能发现。
        // Arrange & Act
        let demo = build_demo_world();
        let player_pos = demo
            .world
            .actors
            .get(demo.player)
            .expect("刚生成的玩家必然存在")
            .pos;
        let east_neighbor = demo.world.size.wrap(player_pos.x() + 1, player_pos.y());

        // Assert
        let kind = demo
            .world
            .terrain_at(east_neighbor)
            .expect("demo 世界是单区块布局，已整体常驻");
        assert_eq!(kind, demo.lava_kind.unwrap());
        assert!(!kind.blocks_move(&demo.world.terrain_table));
    }
}
