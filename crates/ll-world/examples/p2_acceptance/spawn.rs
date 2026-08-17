//! 出生点搜索与山脊雕刻：改动 [`ChunkGrid`] 内容的逻辑，与
//! [`crate::layout`] 里「给定数据现算现出」的纯呈现函数分开一个文件。
//!
//! 两者都不依赖 GPU，可以脱离窗口与图形适配器被单测覆盖；分开只是
//! 因为一个改数据、一个不改数据，职责不同（见 `coding-style.md`
//! 「文件组织」一节：按职责拆分而非把所有纯函数塞进一个文件）。

use ll_core::torus::{TorusPos, TorusSize};
use ll_world::chunk::ChunkGrid;
use ll_world::terrain::TerrainKind;

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
pub(crate) fn find_spawn(grid: &ChunkGrid) -> TorusPos {
    let world = grid.world();
    let center = world.wrap(world.width() as i32 / 2, world.height() as i32 / 2);

    if is_spawnable(grid, center) {
        return center;
    }

    for radius in 1..=SPAWN_SEARCH_MAX_RADIUS {
        if let Some(pos) = search_ring(grid, world, center, radius) {
            return pos;
        }
    }
    center
}

/// 该地形是否适合作为出生点：既能站立也能看见周围。
fn is_spawnable(grid: &ChunkGrid, pos: TorusPos) -> bool {
    let kind = grid.terrain_at(pos);
    !kind.blocks_move() && !kind.blocks_sight()
}

/// 在距 `center` 切比雪夫距离恰为 `radius` 的环上寻找第一个可站立格。
fn search_ring(
    grid: &ChunkGrid,
    world: TorusSize,
    center: TorusPos,
    radius: i32,
) -> Option<TorusPos> {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs().max(dy.abs()) != radius {
                continue;
            }
            let pos = world.wrap(center.x() + dx, center.y() + dy);
            if is_spawnable(grid, pos) {
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
/// 直接用 [`ChunkGrid::set_terrain`] 覆写：这是该方法的公开用途之一，
/// 不是绕过什么校验——世界生成完成后按需雕刻地标是正常操作，正如
/// 建筑内部的地形本身也要靠同一个方法逐格写入。
pub(crate) fn carve_wall_ridge(grid: &mut ChunkGrid, spawn: TorusPos) {
    let world = grid.world();
    for i in 0..WALL_RIDGE_LEN {
        let pos = world.wrap(spawn.x() + WALL_RIDGE_OFFSET + i, spawn.y());
        grid.set_terrain(pos, TerrainKind::MOUNTAIN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{BASE_SIGHT_RADIUS, WORLD_HEIGHT, WORLD_WIDTH};
    use ll_world::generate::{GenParams, generate_terrain};

    /// 测试世界尺寸：与 demo 实际使用的尺寸一致，保证测试覆盖真实路径。
    fn test_world() -> TorusSize {
        TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("demo 世界尺寸满足全部构造前置条件")
    }

    fn test_grid() -> ChunkGrid {
        generate_terrain(test_world(), &GenParams::default())
            .expect("demo 世界尺寸满足生成入口约束")
    }

    #[test]
    fn 出生点搜索结果可以站立() {
        // Arrange
        let grid = test_grid();

        // Act
        let spawn = find_spawn(&grid);

        // Assert
        assert!(is_spawnable(&grid, spawn));
    }

    #[test]
    fn 世界几乎全是深水时出生点搜索仍会终止() {
        // 极端场景：`ChunkGrid::new` 本就把全部格子初始化为 DEEP_WATER
        // （阻挡移动，见其文档），不生成任何真实地形，验证搜索函数不会
        // 死循环——它必须在 SPAWN_SEARCH_MAX_RADIUS 圈之后退回中心点，
        // 而不是无限找下去。
        // Arrange
        let world = test_world();
        let grid = ChunkGrid::new(world).expect("demo 世界尺寸满足构造前置条件");

        // Act
        let spawn = find_spawn(&grid);

        // Assert：函数确实返回了（没有死循环/panic），且落在世界范围内。
        let _ = grid.terrain_at(spawn); // 越界会直接 panic，借此断言坐标合法
    }

    #[test]
    fn 出生点旁的山脊全部变为山地() {
        // Arrange
        let mut grid = test_grid();
        let spawn = find_spawn(&grid);

        // Act
        carve_wall_ridge(&mut grid, spawn);

        // Assert
        let world = grid.world();
        for i in 0..WALL_RIDGE_LEN {
            let pos = world.wrap(spawn.x() + WALL_RIDGE_OFFSET + i, spawn.y());
            assert_eq!(grid.terrain_at(pos), TerrainKind::MOUNTAIN);
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
