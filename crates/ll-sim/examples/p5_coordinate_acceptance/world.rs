//! 世界搭建：区块布局、地表出生点、一个 Interior 入口与它唯一的楼层。
//!
//! 这是本 demo 唯一负责搭场景的模块——地表地形完全交给真实的噪声生成
//! （不像 `p2_acceptance` 那样人工雕刻山脊），只有出生点与 Interior
//! 入口这两格地形被强制改写成已知种类，理由与
//! `p2_acceptance::spawn::carve_wall_ridge` 一致：验收 demo 需要确定性
//! 场景，不能赌噪声生成恰好在出生点附近给出可站立的地形。

use ll_core::ident::{ContentIndex, Interner, NamespacedId, WorldId};
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_world::bounded_grid::BoundedGrid;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::{GenParams, build_zone_noise};
use ll_world::interior::Interior;
use ll_world::noise::TileableNoise;
use ll_world::space::{Space, SpaceId};
use ll_world::space_profile::{BaseSpaceProfileIds, SpaceProfile, base_space_profile_fixture};
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

use crate::layout::{
    EAST_CORRIDOR_LENGTH, ENTRANCE_OFFSET_Y, INTERIOR_FLOOR_SIZE, SPAWN_X, SPAWN_Y, ZONE_COUNT_X,
    ZONE_COUNT_Y, ZONE_SPAN,
};

/// 建立本 demo 用的区块布局：见 `crate::layout::ZONE_COUNT_X`/`_Y`
/// 文档「为什么不能更小」。
pub(crate) fn build_zone_layout() -> ZoneLayout {
    let zone_count =
        ll_core::torus::TorusSize::new(ZONE_COUNT_X, ZONE_COUNT_Y).expect("6x4 是合法的 TorusSize");
    ZoneLayout::new(ZONE_SPAN, zone_count).expect("ZONE_SPAN 满足全部对齐与跨度约束")
}

/// 装载完毕的演示世界：世界状态、噪声源与生成参数（流式加载持续需要，
/// 见 `WorldState::terrain_at_streaming` 文档）、地形/层属性索引缓存、
/// 玩家实体、Interior 入口的世界坐标与实例 id。
pub(crate) struct DemoWorld {
    pub(crate) world: WorldState,
    pub(crate) noise: TileableNoise,
    pub(crate) params: GenParams,
    pub(crate) terrain_ids: BaseTerrainIds,
    pub(crate) space_ids: BaseSpaceProfileIds,
    pub(crate) player: EntityId,
    pub(crate) interior_anchor: TorusPos,
    pub(crate) interior_id: SpaceId,
    /// 完整的层属性表——只用于渲染层现算 `effective_ambient_light`
    /// （见 `crate::layout::effective_sight_radius`），不进世界状态。
    pub(crate) space_table: ll_world::space_profile::SpaceProfileTable,
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
    pub(crate) fn profile_of(&self, space: Space) -> SpaceProfile {
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
pub(crate) fn build_demo_world() -> DemoWorld {
    let layout = build_zone_layout();
    let params = GenParams::default();
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let (space_ids, space_table) = base_space_profile_fixture();
    let noise = build_zone_noise(&layout, &params).expect("build_zone_layout 满足全部约束");

    let spawn = layout.tile_size().wrap(SPAWN_X, SPAWN_Y);
    let mut world = WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn)
        .expect("演示世界布局满足生成入口的全部约束");
    world.advance(super::layout::INITIAL_CLOCK_TICKS);
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
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        luck: 0,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(zone, surface_profile),
        script_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
    })
}

#[cfg(test)]
mod tests {
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
