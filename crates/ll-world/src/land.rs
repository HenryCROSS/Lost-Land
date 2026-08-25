//! 「一块能住人的地有多大」——区块窗口内的连通可行走陆地分析。
//!
//! # 为什么独立成一个模块，而不是留在调用方各自那里
//!
//! 这份算法**有两个消费者**：
//!
//! 1. `ll_game::world::find_spawn_site`——新游戏出生点必须落在一片
//!    连得开的陆地上（阈值 `MIN_SPAWN_LAND_AREA`）。
//! 2. [`crate::chronicle`] 的据点选址——「文明会在哪里立足」用的是
//!    完全同一条判据，只是阈值不同。
//!
//! ADR 0021 的判据（「有没有一份算法要被共用」）在这里是双向成立的：
//! 复制它就等于把「什么叫一块能住人的地」变成两个真相源，两边一旦
//! 漂移，就会出现「据点建在出生点选址判定为不合格的碎地上」这类说不
//! 清的表现。本模块因此是这条判据的**唯一**实现，`ll-game` 侧那份
//! 私有副本已经删除，改为调用这里。
//!
//! # 阈值是参数，不是常量
//!
//! 两个消费者的阈值本就不同（出生点 500 格；据点模板各自的最小面积），
//! 与 `ll_world::item::merge_stacks` 的 `stack_limit`、
//! `WorldState::cleanup_aged_ground_items` 的老化阈值同一条既有纪律：
//! 引擎不在算法体内写死玩法数值。

use std::collections::VecDeque;

use ll_core::torus::{TorusPos, TorusSize};

use crate::chunk::ChunkGrid;
use crate::terrain::TerrainTable;

/// 一个区块窗口内最大的那块连通可行走陆地。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandComponent {
    /// 该分量里按光栅序**最先被访问到**的那一格（区块内局部坐标）。
    ///
    /// 出生点选址用的就是它——这条语义是 `find_spawn_site` 从一开始
    /// 就在用的，不改，改了会让同一个种子的玩家出生点漂移。
    pub start: TorusPos,
    /// 该分量里**离窗口正中最近**的那一格（区块内局部坐标），并列时
    /// 取光栅序最先的。
    ///
    /// 据点选址用的是它：据点要往分量中间放，才有把整座村子铺在同一
    /// 个区块窗口内的余地（[`crate::settlement::stamp_settlement`]
    /// 「为什么不跨区块」）。用 `start` 会把村子钉在陆地的左上角，
    /// 大半栋房子落到窗口外被跳过。
    pub center: TorusPos,
    /// 该分量的格数。
    pub area: usize,
}

/// 在一个已生成的区块窗口内做连通域分析（BFS），返回**格数最大**的
/// 连通可行走分量。最大分量的格数不足 `min_area` 时返回 `None`。
///
/// # 区块内部坐标不做环绕
///
/// 本函数只关心「这个区块窗口内部，连通到一起的陆地有多大」，不把
/// 窗口一条边界的移动接到本窗口另一条边界（那会把两个本不相邻的世界
/// 坐标误判成相邻）——跨区块的连通性判断留给「换一个区块继续看」这
/// 一层，不在这里假装两者相邻。
///
/// # 确定性
///
/// 起点按光栅序遍历，邻居固定按上、右、下、左的顺序入队，全程只用
/// `Vec`/`VecDeque`，不触碰任何 `HashMap`/`HashSet` 的迭代顺序
/// （约束 C5）。同一个窗口与同一张地形表恒产出同一个结果。
pub fn largest_walkable_component(
    window: &ChunkGrid,
    local_size: TorusSize,
    table: &TerrainTable,
    min_area: usize,
) -> Option<LandComponent> {
    debug_assert_eq!(
        local_size.width(),
        local_size.height(),
        "区块窗口的局部坐标系恒为正方形，见 ZoneLayout::local_size 文档"
    );
    let span = local_size.width() as i32;

    let mut visited = vec![false; (span * span) as usize];
    let mut best: Option<ScannedComponent> = None;

    for start_y in 0..span {
        for start_x in 0..span {
            let start_idx = (start_y * span + start_x) as usize;
            if visited[start_idx] {
                continue;
            }
            visited[start_idx] = true;
            if window
                .terrain_at(local_size.wrap(start_x, start_y))
                .blocks_move(table)
            {
                continue;
            }

            let found = flood_fill(
                window,
                local_size,
                table,
                &mut visited,
                span,
                (start_x, start_y),
            );
            if best.is_none_or(|top| found.area > top.area) {
                best = Some(found);
            }
        }
    }

    let found = best?;
    if found.area < min_area {
        return None;
    }
    Some(LandComponent {
        start: local_size.wrap(found.start.0, found.start.1),
        center: local_size.wrap(found.center.0, found.center.1),
        area: found.area,
    })
}

/// 扫描过程中的一个连通分量——[`flood_fill`] 的返回值与
/// [`largest_walkable_component`] 内部「目前最大的那一个」共用的形状。
/// 坐标是裸的 `(x, y)` 而不是 `TorusPos`：泛洪内部要做越界比较，包成
/// 环面坐标反而要来回拆装，只在最后返回给调用方时包一次。
#[derive(Debug, Clone, Copy)]
struct ScannedComponent {
    area: usize,
    start: (i32, i32),
    center: (i32, i32),
}

/// 从 `start` 起广度优先收集一个连通可行走分量——
/// [`largest_walkable_component`] 的帮手，拆出来只是为了让外层那个
/// 双重循环保持在四层嵌套以内。
///
/// `visited` 由调用方持有并跨多次调用复用：一格一旦被任何一次泛洪
/// 访问过就不会再被当作新起点，这是整个扫描保持 O(格数) 的原因。
///
/// 「离正中最近」用平方欧氏距离比较，严格小于才替换——并列时保留先
/// 访问到的那一格，而访问顺序由 BFS 的固定邻居顺序决定，因此结果与
/// 任何迭代顺序无关（约束 C5）。
fn flood_fill(
    window: &ChunkGrid,
    local_size: TorusSize,
    table: &TerrainTable,
    visited: &mut [bool],
    span: i32,
    start: (i32, i32),
) -> ScannedComponent {
    let (start_x, start_y) = start;
    let mid = span / 2;
    let distance_to_mid = |x: i32, y: i32| (x - mid) * (x - mid) + (y - mid) * (y - mid);
    let mut center = (start_x, start_y);
    let mut center_distance = distance_to_mid(start_x, start_y);
    // 邻居固定按上、右、下、左的顺序入队——不依赖任何哈希容器的迭代
    // 顺序（约束 C5）。
    let mut queue = VecDeque::new();
    let mut size = 0usize;
    queue.push_back((start_x, start_y));
    while let Some((x, y)) = queue.pop_front() {
        size += 1;
        let distance = distance_to_mid(x, y);
        if distance < center_distance {
            center_distance = distance;
            center = (x, y);
        }
        for (nx, ny) in [(x, y - 1), (x + 1, y), (x, y + 1), (x - 1, y)] {
            if nx < 0 || ny < 0 || nx >= span || ny >= span {
                continue;
            }
            let n_idx = (ny * span + nx) as usize;
            if visited[n_idx] {
                continue;
            }
            visited[n_idx] = true;
            if window
                .terrain_at(local_size.wrap(nx, ny))
                .blocks_move(table)
            {
                continue;
            }
            queue.push_back((nx, ny));
        }
    }
    ScannedComponent {
        area: size,
        start,
        center,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::base_terrain_fixture;

    /// 造一个 `span × span` 的窗口，全部填成第二个参数给的地形。
    fn filled_window(span: u32, kind: crate::terrain::TerrainKind) -> (ChunkGrid, TorusSize) {
        let size = TorusSize::new(span, span).expect("测试尺寸合法");
        let grid = ChunkGrid::new(size, kind).expect("测试尺寸满足 ChunkGrid 的视口跨度要求");
        (grid, size)
    }

    #[test]
    fn 整窗都是陆地时起点在左上角而正中格在窗口中心() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let (grid, size) = filled_window(48, ids.grass);

        // Act
        let found = largest_walkable_component(&grid, size, &table, 1).expect("整窗陆地必然合格");

        // Assert
        assert_eq!(found.start, size.wrap(0, 0));
        assert_eq!(found.center, size.wrap(24, 24));
        assert_eq!(found.area, 48 * 48);
    }

    #[test]
    fn 分量小于阈值时返回none() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let (mut grid, size) = filled_window(48, ids.deep_water);
        grid.set_terrain(size.wrap(10, 10), ids.grass);

        // Act
        let found = largest_walkable_component(&grid, size, &table, 2);

        // Assert
        assert_eq!(found, None);
    }

    #[test]
    fn 两块孤立陆地时取更大的那一块() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let (mut grid, size) = filled_window(48, ids.deep_water);
        // 小块：(2,2) 一格。
        grid.set_terrain(size.wrap(2, 2), ids.grass);
        // 大块：(20,20)-(22,20) 三格，横向连通。
        for x in 20..23 {
            grid.set_terrain(size.wrap(x, 20), ids.grass);
        }

        // Act
        let found = largest_walkable_component(&grid, size, &table, 1).expect("大块有三格");

        // Assert
        assert_eq!(found.start, size.wrap(20, 20));
        assert_eq!(found.area, 3);
        // 正中格取三格里离窗口中心 (24,24) 最近的那一格。
        assert_eq!(found.center, size.wrap(22, 20));
    }

    #[test]
    fn 窗口边界不环绕相接() {
        // Arrange：左右两条边各一格陆地，若错误地把窗口当环面就会连成
        // 一个 2 格分量。
        let (ids, table) = base_terrain_fixture();
        let (mut grid, size) = filled_window(48, ids.deep_water);
        grid.set_terrain(size.wrap(0, 5), ids.grass);
        grid.set_terrain(size.wrap(47, 5), ids.grass);

        // Act
        let found = largest_walkable_component(&grid, size, &table, 1).expect("单格分量也合格");

        // Assert
        assert_eq!(found.start, size.wrap(0, 5));
        assert_eq!(found.area, 1);
    }

    #[test]
    fn 同一个窗口两次分析结果完全相同() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let (mut grid, size) = filled_window(48, ids.deep_water);
        for y in 4..40 {
            for x in 6..30 {
                grid.set_terrain(size.wrap(x, y), ids.grass);
            }
        }

        // Act
        let first = largest_walkable_component(&grid, size, &table, 100);
        let second = largest_walkable_component(&grid, size, &table, 100);

        // Assert
        assert_eq!(first, second);
        assert!(first.is_some());
    }
}
