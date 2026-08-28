//! 出生点搜索与山脊雕刻：改动 [`ChunkGrid`] 内容的逻辑，与
//! [`crate::layout`] 里「给定数据现算现出」的纯呈现函数分开一个文件。
//!
//! 两者都不依赖 GPU，可以脱离窗口与图形适配器被单测覆盖；分开只是
//! 因为一个改数据、一个不改数据，职责不同（见 `coding-style.md`
//! 「文件组织」一节：按职责拆分而非把所有纯函数塞进一个文件）。

use ll_core::torus::{TorusPos, TorusSize};
use ll_world::state::WorldState;
use ll_world::terrain::BaseTerrainIds;

/// 出生点旁人工摆放的山脊，距出生点的横向偏移（格）。
///
/// 山脊本身不是「墙」这个建筑地形，而是把 [`TerrainKind::MOUNTAIN`]
/// （天然阻挡视线，见其文档）摆在出生点附近——这样「墙后不可见」这条
/// 验收点不必依赖噪声生成恰好在出生点周围长出山脉或森林那种运气，
/// 换来确定性、可重复的演示。
pub(crate) const WALL_RIDGE_OFFSET: i32 = 6;

/// 山脊长度（格）。
pub(crate) const WALL_RIDGE_LEN: i32 = 5;

/// 出生点搜索的最大环半径（格）。超过这个半径仍找不到可站立的格子就
/// 放弃搜索、退回中心点本身——理论上不应触发（默认生成参数下水域远
/// 不足以覆盖这么大的一圈），但函数必须对任何生成结果都能终止。
const SPAWN_SEARCH_MAX_RADIUS: i32 = 64;

/// 从世界中心开始按环逐圈向外搜索一格「可站立」（既不阻挡移动也不
/// 阻挡视线）的地形，作为玩家出生点。
///
/// 按环而非按行/列扫描：环上的格子到中心的切比雪夫距离相等，这样搜索
/// 结果恒是「离中心最近的可站立格」，不会因为扫描顺序偏向某个方向而
/// 找到一个明明更远却先被扫到的格子。
///
/// 搜索半径超过 [`SPAWN_SEARCH_MAX_RADIUS`] 仍未找到时退回中心点本身
/// ——默认生成参数下不会触发（水域远不足以覆盖这么大的一圈），但函数
/// 必须对任何生成结果都能终止，不能无限循环。
///
/// # 为什么接受 `&WorldState` 而不是单独的 `&ChunkGrid`（两级坐标系
/// 重写，任务 11）
///
/// 迁移前地形是一整张一次性生成、整体常驻的 `ChunkGrid`，出生点搜索
/// 只需要那一张网格加地形属性表。`terrain` 换成
/// `ll_world::surface_store::SurfaceStore` 之后不再有单一「一张网格」
/// 可传——本 demo 在 [`Demo::new`](crate::Demo::new) 里已经用
/// `SurfaceStore::warm_all` 把整个（不大的）演示世界预热成常驻，所以
/// 这里改用 [`WorldState::terrain_at`] 这个只读查询，配合
/// `.expect(..)` 断言查询到的坐标必然常驻——这个断言只在「demo 世界
/// 已经整体预热」这个前提下成立，不是通用做法。
pub(crate) fn find_spawn(world: &WorldState) -> TorusPos {
    let size = world.size;
    let center = size.wrap(size.width() as i32 / 2, size.height() as i32 / 2);

    if is_spawnable(world, center) {
        return center;
    }

    for radius in 1..=SPAWN_SEARCH_MAX_RADIUS {
        if let Some(pos) = search_ring(world, size, center, radius) {
            return pos;
        }
    }
    center
}

/// 该地形是否适合作为出生点：既能站立也能看见周围。
fn is_spawnable(world: &WorldState, pos: TorusPos) -> bool {
    let kind = world
        .terrain_at(pos)
        .expect("demo 世界已经用 warm_all 整体预热，任意坐标都应该常驻");
    !kind.blocks_move(&world.terrain_table) && !kind.blocks_sight(&world.terrain_table)
}

/// 在距 `center` 切比雪夫距离恰为 `radius` 的环上寻找第一个可站立格。
fn search_ring(
    world: &WorldState,
    size: TorusSize,
    center: TorusPos,
    radius: i32,
) -> Option<TorusPos> {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs().max(dy.abs()) != radius {
                continue;
            }
            let pos = size.wrap(center.x() + dx, center.y() + dy);
            if is_spawnable(world, pos) {
                return Some(pos);
            }
        }
    }
    None
}

/// 在出生点以东 [`WALL_RIDGE_OFFSET`] 格处摆一条长
/// [`WALL_RIDGE_LEN`] 格的山脊，作为「墙后不可见」验收点的确定性场景
/// ——理由见 [`WALL_RIDGE_OFFSET`] 文档。
///
/// 直接用 [`ll_world::surface_store::SurfaceStore::set_terrain`] 覆写：
/// 世界生成完成后按需雕刻地标是正常操作，正如建筑内部的地形本身也要
/// 靠同一个方法逐格写入。出生点所在区块已经在 `Demo::new` 里被
/// `warm_all` 预热过，`set_terrain` 因此不会撞见「未常驻」的 panic。
pub(crate) fn carve_wall_ridge(
    world: &mut WorldState,
    spawn: TorusPos,
    terrain_ids: &BaseTerrainIds,
) {
    let size = world.size;
    for i in 0..WALL_RIDGE_LEN {
        let pos = size.wrap(spawn.x() + WALL_RIDGE_OFFSET + i, spawn.y());
        world.terrain.set_terrain(pos, terrain_ids.mountain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::BASE_SIGHT_RADIUS;
    use ll_core::time::Tick;
    use ll_world::generate::build_zone_noise;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    /// 测试世界：与 demo 实际使用的尺寸、区块布局一致
    /// （`crate::build_zone_layout`），保证测试覆盖真实路径；整体预热
    /// （`warm_all`），匹配 `Demo::new` 的前提——见 `find_spawn`/
    /// `is_spawnable` 文档「为什么接受 &WorldState」。
    fn test_world() -> WorldState {
        let layout = crate::build_zone_layout();
        let params = crate::layout::demo_gen_params();
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let noise = build_zone_noise(&layout, &params).expect("build_zone_layout 满足全部约束");
        let placeholder_spawn = layout.tile_size().wrap(0, 0);
        let mut world = WorldState::new(
            layout,
            &params,
            &terrain_ids,
            terrain_table,
            placeholder_spawn,
        )
        .expect("demo 世界布局满足全部构造前置条件");
        world
            .terrain
            .warm_all(&noise, &params, &terrain_ids, Tick(0));
        world
    }

    #[test]
    fn 出生点搜索结果可以站立() {
        // Arrange
        let world = test_world();

        // Act
        let spawn = find_spawn(&world);

        // Assert
        assert!(is_spawnable(&world, spawn));
    }

    #[test]
    fn 世界几乎全是深水时出生点搜索仍会终止() {
        // 极端场景：整个世界覆写成深水（阻挡移动，见其文档），不依赖
        // 生成算法凑出这个场景，验证搜索函数不会死循环——它必须在
        // SPAWN_SEARCH_MAX_RADIUS 圈之后退回中心点，而不是无限找下去。
        // 用比 demo 实际尺寸小得多的独立世界（单个区块）：这条测试只
        // 关心搜索函数的终止性，不需要复用完整的 demo 世界。
        // Arrange
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let params = crate::layout::demo_gen_params();
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn_pos = layout.tile_size().wrap(0, 0);
        let mut world = WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn_pos)
            .expect("layout 满足全部构造前置条件");
        let size = world.size;
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                world
                    .terrain
                    .set_terrain(size.wrap(x, y), terrain_ids.deep_water);
            }
        }

        // Act
        let spawn = find_spawn(&world);

        // Assert：函数确实返回了（没有死循环/panic），且落在已常驻的
        // 区块范围内——产出越界坐标会让这里返回 None 而不是 panic。
        assert!(world.terrain_at(spawn).is_some());
    }

    #[test]
    fn 出生点旁的山脊全部变为山地() {
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let spawn = find_spawn(&world);

        // Act
        carve_wall_ridge(&mut world, spawn, &terrain_ids);

        // Assert
        let size = world.size;
        for i in 0..WALL_RIDGE_LEN {
            let pos = size.wrap(spawn.x() + WALL_RIDGE_OFFSET + i, spawn.y());
            assert_eq!(
                world
                    .terrain_at(pos)
                    .expect("demo 世界已经用 warm_all 整体预热"),
                terrain_ids.mountain
            );
        }
    }

    #[test]
    fn 山脊落在出生点的基准视野半径之内() {
        // 「墙后不可见」这条验收点要求玩家在出生点附近就能看见这道
        // 山脊本身，否则无从演示「看不见山脊背后」。
        // Arrange
        let ridge_end_x = WALL_RIDGE_OFFSET + WALL_RIDGE_LEN - 1;

        // Act & Assert
        assert!((ridge_end_x as u32) < BASE_SIGHT_RADIUS);
    }
}
