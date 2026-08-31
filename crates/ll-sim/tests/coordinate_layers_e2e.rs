//! 两级坐标系与离散层的端到端回归：跨区块流式加载、进出 Interior、
//! 世界地图探索记忆。
//!
//! # 出处（2026-08-29 批次 13）
//!
//! 本文件由 `crates/ll-sim/examples/p5_coordinate_acceptance/` 的
//! `world.rs`（场景搭建 + 3 条断言）与 `walkthrough_test.rs`（6 条断言）
//! 合并而来。所有者裁定去掉 `examples/`（见
//! `knowledge/decisions/0030-remove-examples-acceptance-demos.md`），
//! 这九条断言是全仓库**唯一**程序化走通
//! 「`stream_neighborhood` → `Intent` → `resolve` → `Effect` → `apply`
//! → `continent_map` 探索记忆」这条链的地方，逐字搬迁、一条未改。
//!
//! 原 `walkthrough_test.rs` 的模块文档解释过它为什么存在（ADR 0025：
//! 合成键盘事件无法安全隔离到目标窗口，曾把按键泄漏进宿主对话窗口，
//! 因此验收改为直接驱动与真实按键完全相同的那条链路）。那条理由在
//! 搬迁后**更强**：现在连窗口都不存在了，这就是唯一的验收方式。
//!
//! 原 demo 的开窗渲染、小地图版式、动画状态机接线随 demo 一并删除——
//! 前者搬不动（要 GPU），后两者在 `ll_render::sprite`/`ll_render::anim`
//! 自己的测试里被更强地覆盖（盘点见
//! `docs/superpowers/plans/2026-08-29-batch13-example-cleanup.md` §1.4）。

/// 单个区块的边长（格）——原 demo `layout::ZONE_SPAN`。
const ZONE_SPAN: u32 = 64;
/// 世界的区块列数——原 demo `layout::ZONE_COUNT_X`。
const ZONE_COUNT_X: u32 = 6;
/// 世界的区块行数——原 demo `layout::ZONE_COUNT_Y`。
const ZONE_COUNT_Y: u32 = 4;
/// 玩家出生点的世界格坐标 x——原 demo `layout::SPAWN_X`。
const SPAWN_X: i32 = 20;
/// 玩家出生点的世界格坐标 y——原 demo `layout::SPAWN_Y`。
const SPAWN_Y: i32 = 20;
/// 出生点向东雕刻的可通行走廊长度（格）——原 demo
/// `layout::EAST_CORRIDOR_LENGTH`。必须长到跨过好几个区块边界，
/// 「连续移动跨越多个区块边界全程无阻挡」才有内容可验。
const EAST_CORRIDOR_LENGTH: i32 = 260;
/// 向东走进出生邻域预热覆盖不到的区块所需的步数——原 demo
/// `layout::EAST_WALK_INTO_UNWARMED_ZONE`。
const EAST_WALK_INTO_UNWARMED_ZONE: i32 = 200;
/// Interior 入口相对出生点的偏移（格，向南）——原 demo
/// `layout::ENTRANCE_OFFSET_Y`。紧邻出生点，一次方向键即可走到。
const ENTRANCE_OFFSET_Y: i32 = 3;
/// Interior 楼层的边长（格）——原 demo `layout::INTERIOR_FLOOR_SIZE`。
const INTERIOR_FLOOR_SIZE: u32 = 12;
/// 地表视野基准半径（格）——原 demo `layout::BASE_SIGHT_RADIUS`。
const BASE_SIGHT_RADIUS: u32 = 12;
/// 流式邻域维护半径（区块）——原 demo `layout::STREAM_RADIUS_ZONES`。
const STREAM_RADIUS_ZONES: i32 = 2;
/// 世界地图下采样倍率——原 demo `layout::MINIMAP_DOWNSAMPLE`。
const MINIMAP_DOWNSAMPLE: u32 = 1;
/// 暗视半径加成：本文件不验暗视，恒取 0——原 demo `layout::NO_DARKVISION`。
const NO_DARKVISION: u32 = 0;
/// 世界时钟的初始刻度：正午——原 demo `layout::INITIAL_CLOCK_TICKS`。
const INITIAL_CLOCK_TICKS: i64 = 12 * ll_core::time::TICKS_PER_HOUR;

/// 求某个空间在给定世界时刻的有效视野半径——原 demo
/// `layout::effective_sight_radius`，**换算一个字未改**。
///
/// 恒传 `Weather::CLEAR`：本文件验的是坐标系与层属性，不是天气；
/// 让随机天气参与只会给「地下比地表暗」这条断言多一个无关变量。
fn effective_sight_radius(
    profile: &ll_world::space_profile::SpaceProfile,
    clock: ll_core::time::Tick,
) -> u32 {
    let light = ll_world::space_profile::effective_ambient_light(
        profile,
        clock,
        ll_world::weather::Weather::CLEAR,
    );
    ll_world::light::sight_radius_at(BASE_SIGHT_RADIUS, light, NO_DARKVISION)
}

use ll_core::ident::{ContentIndex, Interner, NamespacedId, WorldId};
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_world::bounded_grid::BoundedGrid;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::{GenParams, TerrainShape, build_zone_noise};
use ll_world::interior::Interior;
use ll_world::noise::TileableNoise;
use ll_world::space::{Space, SpaceId};
use ll_world::space_profile::{BaseSpaceProfileIds, SpaceProfile, base_space_profile_fixture};
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

/// 建立本 demo 用的区块布局：见 `crate::layout::ZONE_COUNT_X`/`_Y`
/// 文档「为什么不能更小」。
fn build_zone_layout() -> ZoneLayout {
    let zone_count =
        ll_core::torus::TorusSize::new(ZONE_COUNT_X, ZONE_COUNT_Y).expect("6x4 是合法的 TorusSize");
    ZoneLayout::new(ZONE_SPAN, zone_count).expect("ZONE_SPAN 满足全部对齐与跨度约束")
}

/// 装载完毕的演示世界：世界状态、噪声源与生成参数（流式加载持续需要，
/// 见 `WorldState::terrain_at_streaming` 文档）、地形/层属性索引缓存、
/// 玩家实体、Interior 入口的世界坐标与实例 id。
struct DemoWorld {
    world: WorldState,
    noise: TileableNoise,
    params: GenParams,
    terrain_ids: BaseTerrainIds,
    space_ids: BaseSpaceProfileIds,
    player: EntityId,
    interior_anchor: TorusPos,
    interior_id: SpaceId,
    /// 完整的层属性表——只用于渲染层现算 `effective_ambient_light`
    /// （见 `crate::layout::effective_sight_radius`），不进世界状态。
    space_table: ll_world::space_profile::SpaceProfileTable,
}

impl DemoWorld {
    /// 拿到给定 `Space` 对应的完整 [`SpaceProfile`]——`Space::profile`
    /// 只携带一个 `ContentIndex`，渲染层要用的是完整字段（`exposed_to_sky`/
    /// `ambient_light_floor`），从 `space_table` 现查现拼，不缓存（与
    /// `ll_world::overview` 模块文档「为什么不缓存」同一个理由：world
    /// 状态本身不持有这张表，缓存会有失同步的风险）。
    ///
    /// `id` 字段只用于展示，`effective_ambient_light` 根本不读它（见其
    /// 文档），这里填一个占位标识符即可。
    fn profile_of(&self, space: Space) -> SpaceProfile {
        let index = space.profile();
        SpaceProfile {
            id: NamespacedId::parse("lostland:runtime_profile").expect("字面量恒合法"),
            ambient_light_floor: self.space_table.ambient_light_floor(index),
            exposed_to_sky: self.space_table.exposed_to_sky(index),
            base_temperature: self.space_table.base_temperature(index),
            diggable: self.space_table.diggable(index),
            buildable: self.space_table.buildable(index),
            reverb_tag: self.space_table.reverb_tag(index),
        }
    }
}

/// 搭建演示世界：区块布局→噪声→出生点→Interior 入口与楼层→玩家实体。
fn build_demo_world() -> DemoWorld {
    let layout = build_zone_layout();
    // 气候条带（规格 §7.1）在本 demo 里刻意关掉：本 demo 验的是 P5 那一
    // 批的事（两级坐标系、Interior 楼层、流式加载），世界地形只是背景。
    // `climate_band_width: 0` 是**精确恒等**（见
    // `ll_world::generate::TerrainShape::climate_band_width`），本 demo
    // 的世界因此与气候条带落地之前逐格相同，`layout.rs` 那张只认十种
    // 地形的映射表不需要跟着多认沙漠与冻原，遗留共享画布也就不必凭空
    // 多两个条目（那张画布是五个更早批次 demo 的冻结像素基准）。
    //
    // 本体二进制 `ll-game` 走的是真正的默认值，气候条带在真游戏里开着。
    let params = GenParams {
        shape: TerrainShape {
            climate_band_width: 0,
            ..TerrainShape::default()
        },
        ..GenParams::default()
    };
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let (space_ids, space_table) = base_space_profile_fixture();
    let noise = build_zone_noise(&layout, &params).expect("build_zone_layout 满足全部约束");

    let spawn = layout.tile_size().wrap(SPAWN_X, SPAWN_Y);
    let mut world = WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn)
        .expect("演示世界布局满足生成入口的全部约束");
    world.advance(INITIAL_CLOCK_TICKS);
    world.surface_profile = space_ids.surface;

    // 出生点正东一条强制可通行走廊——不依赖噪声生成恰好给出连续可通行
    // 地形（默认种子下这一带以深水为主，见 EAST_CORRIDOR_LENGTH 文档），
    // 覆盖到跨越第 3 列区块边界所需的距离，验证①②与
    // walkthrough_test.rs 都要用到。先用 terrain_at 触发按需生成（走廊
    // 远端可能落在出生点预热半径覆盖不到的区块），再 set_terrain 覆写
    // ——这正是流式加载的正常使用方式，不是绕过它。
    for dx in 0..=EAST_CORRIDOR_LENGTH {
        let pos = layout.tile_size().wrap(spawn.x() + dx, spawn.y());
        world
            .terrain
            .terrain_at(&noise, &params, &terrain_ids, pos, Tick(0));
        world.terrain.set_terrain(pos, terrain_ids.grass);
    }

    // Interior 入口：出生点正南 ENTRANCE_OFFSET_Y 格。入口本身改写成
    // 沙地（与周围草地区分开、肉眼可辨认「这是一个特殊的格子」），
    // 中间经过的每一格也强制改写成草地——只铺入口那一格、不铺中间
    // 路径，正是 p4_acceptance 世界搭建文档记录过的真实缺陷（「出生点
    // 与熔岩地板之间隔着从未检查过的地形」），这里不重演同一个坑。
    let entrance = layout
        .tile_size()
        .wrap(spawn.x(), spawn.y() + ENTRANCE_OFFSET_Y);
    for dy in 1..ENTRANCE_OFFSET_Y {
        let pos = layout.tile_size().wrap(spawn.x(), spawn.y() + dy);
        world.terrain.set_terrain(pos, terrain_ids.grass);
    }
    world.terrain.set_terrain(entrance, terrain_ids.sand);

    let mut world_id_counter = 0u32;
    let interior_id = WorldId::next(&mut world_id_counter);
    let mut interior = Interior::new(interior_id, entrance, space_ids.dungeon);
    interior.set_floor(0, build_interior_floor(&terrain_ids));
    world.insert_interior(interior);

    let (zone, _) = layout.tile_to_zone(spawn);
    let player = spawn_player(&mut world, spawn, zone, space_ids.surface);
    // 必须显式赋值——见 WorldState::player_entity 字段文档「调用方应在
    // spawn 产出玩家的 EntityId 之后，显式赋值」一节。探索记忆的写入
    // 路径（ll_sim::resolve::resolve_move 追加的 Effect::MarkExplored）
    // 按这个字段区分「谁在动」，只给玩家标记探索——见该函数文档「为
    // 什么只有玩家移动才追加」一节。这里若漏赋值，`world.player_entity`
    // 恒为 `None`，`resolve_move` 的 `world.player_entity == Some(actor)`
    // 恒假，探索记忆永远收不到任何写入，小地图会一直是战争迷雾全黑——
    // 这正是本 demo 曾经出现过的表现（探索记忆批次交付了存储与读取，
    // 却没有接上这处写入的前置条件）。
    world.player_entity = Some(player);

    DemoWorld {
        world,
        noise,
        params,
        terrain_ids,
        space_ids,
        player,
        interior_anchor: entrance,
        interior_id,
        space_table,
    }
}

/// 建一层 [`INTERIOR_FLOOR_SIZE`] 见方的楼层：四周一圈石墙（阻挡移动与
/// 视线），内部铺满石地板——给 FOV 一个边界可以演示「视野半径明显
/// 变小」时画面确实只照亮很小一圈，而不是「反正整层都能看见」。
fn build_interior_floor(terrain_ids: &BaseTerrainIds) -> BoundedGrid {
    let size = ll_core::bounded::BoundedSize::new(INTERIOR_FLOOR_SIZE, INTERIOR_FLOOR_SIZE)
        .expect("INTERIOR_FLOOR_SIZE 是合法的正数尺寸");
    let mut grid = BoundedGrid::new(size, terrain_ids.floor_stone);
    let edge = INTERIOR_FLOOR_SIZE as i32 - 1;
    for i in 0..=edge {
        for &(x, y) in &[(i, 0), (i, edge), (0, i), (edge, i)] {
            if let Some(pos) = size.try_pos(x, y) {
                grid.set_terrain(pos, terrain_ids.wall_stone);
            }
        }
    }
    grid
}

/// 生成玩家单位，写入 `world.actors`，`current_space` 取地表。
fn spawn_player(
    world: &mut WorldState,
    pos: TorusPos,
    zone: ll_world::space::ZoneCoord,
    surface_profile: ContentIndex,
) -> EntityId {
    let mut interner = Interner::new();
    let profession =
        interner.intern(NamespacedId::parse("lostland:wanderer").expect("demo 内置标识符恒合法"));
    let race =
        interner.intern(NamespacedId::parse("lostland:human").expect("demo 内置标识符恒合法"));
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
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
        current_space: Space::surface(zone, surface_profile),
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

mod world_tests {
    use super::*;

    #[test]
    fn 出生点可以站立() {
        // Arrange & Act
        let demo = build_demo_world();

        // Assert
        let pos = demo
            .world
            .actors
            .get(demo.player)
            .expect("刚生成必然存在")
            .pos;
        let kind = demo
            .world
            .terrain_at(pos)
            .expect("出生点所在区块已被出生邻域预热");
        assert!(!kind.blocks_move(&demo.world.terrain_table));
    }

    #[test]
    fn interior入口就是出生点正南entrance_offset_y格且可被反向查询到() {
        // Arrange
        let demo = build_demo_world();
        let spawn = demo
            .world
            .actors
            .get(demo.player)
            .expect("刚生成必然存在")
            .pos;

        // Act
        let expected_entrance = demo
            .world
            .size
            .wrap(spawn.x(), spawn.y() + ENTRANCE_OFFSET_Y);

        // Assert
        assert_eq!(demo.interior_anchor, expected_entrance);
        assert_eq!(
            demo.world.interiors.entries_at(demo.interior_anchor),
            vec![demo.interior_id]
        );
    }

    #[test]
    fn interior楼层四周是阻挡视线的石墙() {
        // Arrange
        let demo = build_demo_world();
        let interior = demo
            .world
            .interiors
            .get(demo.interior_id)
            .expect("刚插入必然存在");
        let floor = interior.floor(0).expect("刚插入 0 层必然存在");
        let size = floor.size();

        // Act & Assert
        let corner = size.try_pos(0, 0).expect("0,0 在范围内");
        assert!(
            floor
                .terrain_at(corner)
                .blocks_sight(&demo.world.terrain_table)
        );
    }
}

use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_sim::intent::{Direction, Intent};
use ll_sim::resolve::resolve;
use ll_world::overview::{continent_map, generate_continent_field};

/// 走一步：先维护流式邻域（与 `Demo::maintain_streaming` 相同的调用），
/// 再 `resolve`+`apply` 一次 `Intent::Move`，返回这一步是否真的移动了
/// （`resolve_move` 在撞墙/撞水/目标区块未常驻时都会产出空效果）。
fn step(demo: &mut DemoWorld, dir: Direction) -> bool {
    let actor = demo.player;
    let pos = demo.world.actors.get(actor).expect("玩家必然存在").pos;
    demo.world.terrain.stream_neighborhood(
        &demo.noise,
        &demo.params,
        &demo.terrain_ids,
        pos,
        STREAM_RADIUS_ZONES,
        demo.world.clock,
    );
    let intent = Intent::Move { actor, dir };
    let effects = resolve(&demo.world, &intent);
    let moved = effects
        .iter()
        .any(|effect| matches!(effect, Effect::MoveTo { .. }));
    for effect in &effects {
        apply(&mut demo.world, effect);
    }
    moved
}

#[test]
fn 沿东向走廊连续移动跨越多个区块边界全程无阻挡() {
    // 验收点①的程序化证据：连续 260 步 Move::East，每一步都必须真的
    // 移动（走廊全程强制可通行，见 EAST_CORRIDOR_LENGTH 文档）——若
    // 中途任意一步被判定为「撞墙」，要么是走廊没铺够远，要么是流式
    // 加载在某个区块边界处掉了链子（目标区块未常驻，resolve_move 保守
    // 地视为不可通行），两者都是需要立刻查的缺陷,不是「反正走不到那么
    // 远也无所谓」。
    // Arrange
    let mut demo = build_demo_world();
    let start_zone = demo
        .world
        .terrain
        .layout()
        .tile_to_zone(demo.world.actors.get(demo.player).expect("必然存在").pos)
        .0;

    // Act & Assert
    for i in 0..EAST_CORRIDOR_LENGTH {
        assert!(step(&mut demo, Direction::East), "第 {i} 步向东移动被阻挡");
    }

    // 落脚区块必须与出生区块不同——否则这条测试只是在原地打转,没有
    // 真的跨越任何边界。
    let end_pos = demo.world.actors.get(demo.player).expect("必然存在").pos;
    let end_zone = demo.world.terrain.layout().tile_to_zone(end_pos).0;
    assert_ne!(start_zone, end_zone);
}

#[test]
fn 出生点邻域预热本身覆盖不到第3列区块() {
    // 验收点①的第二层证据的前半段：先独立证明「出生点一次性预热」这
    // 件事本身覆盖不到第 3 列——不经过 build_demo_world（它为了铺
    // 走廊会提前用 terrain_at 把整条路径都流式加载进来，见其文档，
    // 那样就没法把「出生预热覆盖了哪里」与「走廊铺设覆盖了哪里」这
    // 两件事分开看），直接用同样的区块布局构造一个没有任何走廊/入口
    // 改写的 WorldState，单独检查 SPAWN_WARM_RADIUS 那一圈邻域本身的
    // 覆盖范围。
    // Arrange
    let layout = build_zone_layout();
    let params = ll_world::generate::GenParams::default();
    let (terrain_ids, terrain_table) = ll_world::terrain::base_terrain_fixture();
    let spawn = layout.tile_size().wrap(SPAWN_X, SPAWN_Y);

    // Act
    let world =
        ll_world::state::WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn)
            .expect("demo 区块布局满足全部构造前置条件");
    let (third_column_zone, _) = layout.tile_to_zone(
        layout
            .tile_size()
            .wrap(SPAWN_X + EAST_WALK_INTO_UNWARMED_ZONE, SPAWN_Y),
    );

    // Assert
    assert!(
        !world.terrain.is_resident(third_column_zone),
        "出生点预热不该覆盖到第 3 列区块 {third_column_zone:?}——若这条断言变红，\
         说明 SPAWN_WARM_RADIUS 或世界区块数被改动过，EAST_WALK_INTO_UNWARMED_ZONE\
         的取值需要跟着重新核算"
    );
}

#[test]
fn 走到出生邻域预热覆盖不到的区块查询地形不panic证明真流式加载生效() {
    // 验收点①的第二层证据的后半段：真实玩过一遍——build_demo_world
    // 为了保证走廊可通行,构造时就已经用 terrain_at 把整条走廊（含第
    // 3 列）提前流式加载过一遍（这本身也是流式加载在正常发挥作用，
    // 不是绕过它，见 build_demo_world 文档「先用 terrain_at 触发按需
    // 生成」一节）。这条测试验证的是走到那里之后，resolve/apply 这条
    // 真实结算链路能不能正常查到地形、正常移动进去——不会撞见
    // resolve_move 因为「目标区块未常驻」而保守拒绝移动这种断链。
    // Arrange
    let mut demo = build_demo_world();

    // Act：走到第 3 列区块内部。
    for _ in 0..EAST_WALK_INTO_UNWARMED_ZONE {
        assert!(
            step(&mut demo, Direction::East),
            "走向第 3 列区块的一步被阻挡"
        );
    }
    let end_pos = demo.world.actors.get(demo.player).expect("必然存在").pos;
    let layout = *demo.world.terrain.layout();
    let (end_zone, _) = layout.tile_to_zone(end_pos);

    // Assert：落脚区块确实是预热覆盖不到的第 3 列，且此刻查询地形能
    // 正常拿到值（不是 None，说明真被流式加载进来了，不是巧合常驻）。
    assert_eq!(end_zone.x(), 3);
    assert!(demo.world.terrain_at(end_pos).is_some());
}

#[test]
fn 世界地图标记随玩家移动更新到新的区块坐标() {
    // 验收点②的程序化证据：continent_map 展示的是区块坐标，标记位置
    // 就是 tile_to_zone(agent.pos) ——这里直接断言这个换算本身随移动
    // 更新，真正的像素绘制由 main.rs::push_minimap 消费，不在这条
    // 测试范围内（那部分是纯粹的坐标到像素换算，layout.rs 已有独立
    // 测试覆盖）。
    // Arrange
    let mut demo = build_demo_world();
    let layout = *demo.world.terrain.layout();
    let start_zone = layout.tile_to_zone(demo.world.actors.get(demo.player).expect("必然存在").pos);

    // Act：走到跨越至少一个区块边界的距离。
    for _ in 0..100 {
        step(&mut demo, Direction::East);
    }
    let end_zone = layout.tile_to_zone(demo.world.actors.get(demo.player).expect("必然存在").pos);

    // Assert
    assert_ne!(start_zone, end_zone);
}

#[test]
fn 进出interior的完整调用链走通且层属性生效() {
    // 验收点③④的程序化证据：站在入口格触发 EnterSpace、只有
    // current_space 变化、退出后 pos 精确回到锚点、且地下层的视野半径
    // 明显小于地表——与 ll-sim/src/resolve.rs 的单元测试断言的是同一批
    // 事实，这里额外验证的是"用本 demo 实际搭建的世界（真实的
    // WorldState::new + insert_interior + spawn_player 组合）跑，而不是
    // resolve.rs 测试里简化的夹具世界"，是两套独立构造路径对同一组
    // 不变式的交叉验证。
    // Arrange
    let mut demo = build_demo_world();
    let player = demo.player;
    let spawn_pos = demo.world.actors.get(player).expect("必然存在").pos;
    let clock = demo.world.clock;
    let surface_profile = demo.profile_of(Space::surface(
        demo.world.terrain.layout().tile_to_zone(spawn_pos).0,
        demo.space_ids.surface,
    ));
    let surface_radius = effective_sight_radius(&surface_profile, clock);

    // Act 1：走到入口（正南 ENTRANCE_OFFSET_Y 格）。
    for _ in 0..ENTRANCE_OFFSET_Y {
        assert!(step(&mut demo, Direction::South), "走向入口的一步被阻挡");
    }
    let on_entrance = demo.world.actors.get(player).expect("必然存在").pos;
    assert_eq!(on_entrance, demo.interior_anchor);

    // Act 2：进入。
    let entries = demo.world.interiors.entries_at(on_entrance);
    assert_eq!(entries, vec![demo.interior_id]);
    let enter_effects = resolve(
        &demo.world,
        &Intent::EnterSpace {
            actor: player,
            target: demo.interior_id,
        },
    );
    assert!(!enter_effects.is_empty(), "站在入口格必须能产出进入效果");
    for effect in &enter_effects {
        apply(&mut demo.world, effect);
    }

    // Assert：pos 不变，只有 current_space 变化，且只渲染当前层——即
    // 地下 profile 的视野半径明显小于地表（层属性生效，验收点④）。
    let agent = demo.world.actors.get(player).expect("必然存在");
    assert_eq!(agent.pos, on_entrance);
    let interior_space = agent.current_space;
    assert!(matches!(interior_space, Space::Interior { .. }));
    let interior_profile = demo.profile_of(interior_space);
    let interior_radius = effective_sight_radius(&interior_profile, clock);
    assert!(
        interior_radius < surface_radius,
        "地下视野半径 {interior_radius} 应明显小于地表 {surface_radius}"
    );

    // Act 3：退出。
    let exit_effects = resolve(&demo.world, &Intent::ExitSpace { actor: player });
    assert!(!exit_effects.is_empty(), "在 Interior 内必须能产出退出效果");
    for effect in &exit_effects {
        apply(&mut demo.world, effect);
    }

    // Assert：退出后 pos 精确回到锚点，current_space 回到地表。
    let agent = demo.world.actors.get(player).expect("必然存在");
    assert_eq!(agent.pos, demo.interior_anchor);
    assert!(matches!(agent.current_space, Space::Surface { .. }));
}

#[test]
fn 玩家走过的区块在世界地图上标记为已探索而未去过的区块不是() {
    // 补上探索记忆写入路径批次的程序化证据：探索记忆曾经只交付了存储
    // 与读取，`mark_explored` 没有任何调用方（见 `ll_world::exploration`
    // 模块与 `Effect::MarkExplored` 文档），小地图因此会一直显示全部
    // 未探索。这条测试走的是 `main.rs::push_minimap` 消费的同一条生产
    // 代码路径——`build_demo_world` 搭世界、`generate_continent_field`
    // 建大陆场、`continent_map` 现算概览——而不是直接摆弄
    // `ExplorationMemory` 的内部方法，因此比 `ll_sim::apply` 单元测试
    // 多证明一层：写入路径真的接到了小地图消费的这个函数上，不是两边
    // 各自正确却没连起来。用 `SendKeys` 键盘注入 + 真实窗口截图逐像素
    // 比对 minimap 区域不在本次可行范围内——理由见本文件顶部「为什么
    // 需要这个文件」一节（ADR 0025：这台机器上的合成按键无法可靠地
    // 只送达目标窗口）；这里改用同一份「不模拟按键，直接驱动真实调用
    // 链路」的方法论，程序化验证 `continent_map` 的输出而不是肉眼看
    // 截图。
    // Arrange
    let mut demo = build_demo_world();
    let layout = *demo.world.terrain.layout();
    let field = generate_continent_field(&layout, &demo.noise, &demo.params, &demo.terrain_ids);
    let start_pos = demo.world.actors.get(demo.player).expect("必然存在").pos;
    let (start_zone, _) = layout.tile_to_zone(start_pos);
    let zone_count = layout.zone_count();
    let cols = zone_count.width().div_ceil(MINIMAP_DOWNSAMPLE);
    // 世界另一端、玩家全程走不到的区块——用来确认下面的标记不是恒真
    // （即不是「continent_map 随便什么都判已探索」这种退化实现）。
    let untouched_zone = zone_count.wrap(
        start_zone.x() + zone_count.width() as i32 / 2,
        start_zone.y() + zone_count.height() as i32 / 2,
    );

    // Act：沿走廊走几步——每一步都会触发 `resolve_move` 追加的
    // `Effect::MarkExplored`（见其文档），`apply` 据此把玩家视野内的
    // 格子写进 `world.exploration`。
    for _ in 0..10 {
        step(&mut demo, Direction::East);
    }

    // Assert：出生区块（玩家确定去过、且视野半径覆盖了它)已探索，
    // 世界另一端从未涉足的区块仍是战争迷雾。
    let cells = continent_map(&field, &layout, &demo.world.exploration, MINIMAP_DOWNSAMPLE);
    let start_index = (start_zone.y() as u32 * cols + start_zone.x() as u32) as usize;
    let untouched_index = (untouched_zone.y() as u32 * cols + untouched_zone.x() as u32) as usize;
    assert!(
        cells[start_index].explored,
        "出生区块 {start_zone:?} 应当已探索"
    );
    assert!(
        !cells[untouched_index].explored,
        "从未去过的区块 {untouched_zone:?} 不应该被标记为已探索"
    );
}

/// 层属性 → 有效光照 → 视野半径这条换算本身的回归。
///
/// # 出处（2026-08-29 批次 13）
///
/// 三条断言搬自 `p5_coordinate_acceptance/layout.rs`，逐字未改。它们
/// 落在本文件而不是 `ll-world/tests/` 下，是为了不复制第二份
/// [`effective_sight_radius`]——本仓库反复吃亏的正是「真相源之外的
/// 副本当判据」。同一个包装函数，一处定义，上面的 Interior 验收与
/// 下面这三条共用。
mod space_profile_tests {
    use super::{INITIAL_CLOCK_TICKS, effective_sight_radius};
    use ll_core::time::Tick;
    use ll_world::space_profile::{SpaceProfile, base_space_profile_fixture};

    fn surface_profile() -> SpaceProfile {
        SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:surface").expect("合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 200,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        }
    }

    fn dungeon_profile() -> SpaceProfile {
        SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("合法"),
            ambient_light_floor: 0,
            exposed_to_sky: false,
            base_temperature: 80,
            diggable: false,
            buildable: false,
            reverb_tag: None,
        }
    }

    #[test]
    fn 地下城的视野半径明显小于地表正午() {
        // Arrange
        let noon = Tick(INITIAL_CLOCK_TICKS);
        let surface = surface_profile();
        let dungeon = dungeon_profile();

        // Act
        let surface_radius = effective_sight_radius(&surface, noon);
        let dungeon_radius = effective_sight_radius(&dungeon, noon);

        // Assert
        assert!(dungeon_radius < surface_radius);
    }

    #[test]
    fn 地下城的视野半径不随时钟变化() {
        // Arrange
        let dungeon = dungeon_profile();

        // Act
        let midnight_radius = effective_sight_radius(&dungeon, Tick(0));
        let noon_radius = effective_sight_radius(&dungeon, Tick(INITIAL_CLOCK_TICKS));

        // Assert
        assert_eq!(midnight_radius, noon_radius);
    }

    #[test]
    fn 本体注册的地下城profile在正午视野半径小于地表() {
        // 与上面两条不同：这里直接走 base_space_profile_fixture 注册出
        // 的真实数值（而不是本文件手写的测试用 profile），确认本体实际
        // 会用到的 BaseSpaceProfileIds 组合同样成立。
        // Arrange
        let (_ids, table) = base_space_profile_fixture();
        let (ids2, _table2) = base_space_profile_fixture();
        let surface = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:surface").expect("合法"),
            ambient_light_floor: table.ambient_light_floor(ids2.surface),
            exposed_to_sky: table.exposed_to_sky(ids2.surface),
            base_temperature: table.base_temperature(ids2.surface),
            diggable: table.diggable(ids2.surface),
            buildable: table.buildable(ids2.surface),
            reverb_tag: table.reverb_tag(ids2.surface),
        };
        let dungeon = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("合法"),
            ambient_light_floor: table.ambient_light_floor(ids2.dungeon),
            exposed_to_sky: table.exposed_to_sky(ids2.dungeon),
            base_temperature: table.base_temperature(ids2.dungeon),
            diggable: table.diggable(ids2.dungeon),
            buildable: table.buildable(ids2.dungeon),
            reverb_tag: table.reverb_tag(ids2.dungeon),
        };
        let noon = Tick(INITIAL_CLOCK_TICKS);

        // Act & Assert
        assert!(effective_sight_radius(&dungeon, noon) < effective_sight_radius(&surface, noon));
    }
}
