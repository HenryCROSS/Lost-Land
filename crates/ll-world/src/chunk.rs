//! 环面地形的扁平存储。
//!
//! # 为什么是单一 `Vec`，不再分块（丙案，取代甲案）
//!
//! 早期版本按 `CHUNK_SIZE = 32` 把地形切成一批内部子块，理由是「世界
//! 可达数百万格，整块 `Vec` 一次性分配会吃掉大量内存且无法按需生成」。
//! 这条理由在两级坐标系重写（`crate::zone`/`crate::surface_store`）之后
//! 不再成立：惰性分配与流式加载现在由
//! [`crate::surface_store::SurfaceStore`] 在区块（zone，默认 48×48）
//! 粒度上负责——它把每一个 `ChunkGrid` 实例当成不可再分的原子单位
//! 管理（常驻、淘汰、序列化全部以一整个 `ChunkGrid` 为最小粒度），本
//! 类型自己内部的 32×32 子块因此从未被外部感知，也从未被独立利用过：
//!
//! - 没有任何调用点按子块粒度做过部分生成、部分淘汰或部分序列化——
//!   `crate::state` 里的手写序列化本来就把整个 `ChunkGrid` 摊平成一个
//!   大 `Vec`，完全无视子块边界。
//! - `knowledge/design/coordinate-system-and-layers.md` 十节把「存储块
//!   层被证明只是穿透转发」列为重新评估丙案的触发条件之一——批次 C
//!   （任务 11）落地 `SurfaceStore` 后核实：条件成立。
//!
//! 子分块因此是纯粹的历史包袱：一层从未被外部感知、也从未被独立利用
//! 过的中间结构。移除它之后，本类型与
//! [`crate::bounded_grid::BoundedGrid`]（`Interior` 用的有界局部网格,
//! 本来就是单一 `Vec`）在实现形状上完全对称，唯一差异是坐标类型
//! （环绕的 [`TorusPos`] vs 不环绕的
//! [`BoundedPos`](ll_core::bounded::BoundedPos)）。
//!
//! 类型名与文件名仍然保留「chunk」字样：`ChunkGrid` 现在就是「区块的
//! 存储与生成单位」本身（`crate::zone` 模块文档「关键设计判断 1」：
//! 一个区块对应一个 `ChunkGrid` 实例），不再指代任何内部子分块。

use ll_core::torus::{TorusPos, TorusSize};

use crate::WorldError;
use crate::terrain::TerrainKind;

/// 视口能容纳的最小世界宽度（格）。
///
/// 取自渲染层 `Camera::visible_tiles` 的实际跨度：横向
/// `LOGICAL_WIDTH / TILE_SIZE / 2 + 1 = 21` 向两侧展开共 43 格。
/// 世界小于这个跨度时会产出重复坐标，地形填不满留黑块。
///
/// 此处写死数值而非依赖 `ll-render`：世界层不应反向依赖渲染层。
const MIN_WORLD_WIDTH: u32 = 43;

/// 视口能容纳的最小世界高度（格）。理由同 [`MIN_WORLD_WIDTH`]。
const MIN_WORLD_HEIGHT: u32 = 25;

/// 一张环面地形网格：区块（zone）内部使用的地形存储，按行主序存一个
/// `Vec<TerrainKind>`（见模块文档「为什么是单一 `Vec`，不再分块」）。
///
/// 新建时全部填为调用方指定的 `fill`：这只是分配时的占位值，真正的
/// 地形由后续批次的生成流程写入。
///
/// # 为什么 `fill` 是参数，不是编译期常量
///
/// 旧版直接写死 `TerrainKind::DEEP_WATER`；地形迁入注册表后
/// `TerrainKind` 不再有编译期常量（数值由注册期加载顺序决定，见
/// `crate::terrain` 模块文档），调用方必须显式传入一个已经从
/// `BaseTerrainIds`（或 mod 自己的地形表）解析出来的占位值。
#[derive(Debug, Clone)]
pub struct ChunkGrid {
    world: TorusSize,
    tiles: Vec<TerrainKind>,
}

impl ChunkGrid {
    /// 按世界尺寸建立地形网格。
    ///
    /// 世界任一维度小于视口跨度（[`MIN_WORLD_WIDTH`] × [`MIN_WORLD_HEIGHT`]）
    /// 时返回 [`WorldError::WorldTooSmall`]：与其让缺陷在运行时表现为
    /// 视觉异常（重复坐标、地形填不满留黑块），不如在构造点直接拒绝。
    ///
    /// `fill` 是全部格子的初始占位地形，见本类型文档「为什么 `fill`
    /// 是参数」一节——调用方通常传入 `BaseTerrainIds::deep_water`
    /// 或等价的 mod 定义。
    pub fn new(world: TorusSize, fill: TerrainKind) -> Result<Self, WorldError> {
        if world.width() < MIN_WORLD_WIDTH || world.height() < MIN_WORLD_HEIGHT {
            return Err(WorldError::WorldTooSmall {
                width: world.width(),
                height: world.height(),
            });
        }

        let len = (world.width() as usize) * (world.height() as usize);
        Ok(ChunkGrid {
            world,
            tiles: vec![fill; len],
        })
    }

    /// 该网格所属的世界尺寸。
    pub fn world(&self) -> TorusSize {
        self.world
    }

    /// 读取给定坐标处的地形。
    pub fn terrain_at(&self, pos: TorusPos) -> TerrainKind {
        self.tiles[self.index_of(pos)]
    }

    /// 写入给定坐标处的地形。
    pub fn set_terrain(&mut self, pos: TorusPos, kind: TerrainKind) {
        let idx = self.index_of(pos);
        self.tiles[idx] = kind;
    }

    /// 把环面坐标换算成 `tiles` 里的下标，行主序。
    ///
    /// 换算集中在这一处，供 [`Self::terrain_at`]/[`Self::set_terrain`]
    /// 共用，避免两处各写一份而彼此漂移。
    fn index_of(&self, pos: TorusPos) -> usize {
        // TorusPos 的不变式保证坐标恒非负，转换成 u32 不会丢失信息。
        let x = pos.x() as u32;
        let y = pos.y() as u32;
        (y * self.world.width() + x) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{BaseTerrainIds, base_terrain_fixture};
    use ll_core::torus::TorusSize;

    fn grid_with(fill: TerrainKind) -> ChunkGrid {
        let world = TorusSize::new(64, 64).expect("常量非零");
        ChunkGrid::new(world, fill).expect("64x64 大于视口跨度")
    }

    fn fixture() -> (BaseTerrainIds, ChunkGrid) {
        let (ids, _table) = base_terrain_fixture();
        let grid = grid_with(ids.deep_water);
        (ids, grid)
    }

    #[test]
    fn 世界小于视口跨度时构造失败() {
        // 世界任一维度小于 43×25 格时，渲染层相机会产出重复坐标，
        // 地形填不满留黑块。与其让缺陷在运行时表现为视觉异常，
        // 不如在构造点直接拒绝。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let tiny = TorusSize::new(20, 20).expect("常量非零");

        // Act
        let result = ChunkGrid::new(tiny, ids.deep_water);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 写入后可读回同一地形() {
        // Arrange
        let (ids, mut grid) = fixture();
        let pos = grid.world().wrap(10, 20);

        // Act
        grid.set_terrain(pos, ids.mountain);

        // Assert
        assert_eq!(grid.terrain_at(pos), ids.mountain);
    }

    #[test]
    fn 写入某坐标不会污染另一坐标() {
        // 行主序下标换算是这类扁平存储最容易出错的地方：算错下标会让
        // 写入落到别的格子。
        // Arrange
        let (ids, mut grid) = fixture();
        let a = grid.world().wrap(31, 31);
        let b = grid.world().wrap(32, 32);

        // Act
        grid.set_terrain(a, ids.sand);
        grid.set_terrain(b, ids.snow);

        // Assert
        assert_eq!(grid.terrain_at(a), ids.sand);
    }

    #[test]
    fn 写入某坐标后能在另一坐标正确读回() {
        // 只验前一条「本格未被污染」还不够：下标算错也可能让写入落到
        // 第三个格子而非目标格子，那样 a 依然不受影响，此测试才能
        // 揭出这类错误。
        // Arrange
        let (ids, mut grid) = fixture();
        let a = grid.world().wrap(31, 31);
        let b = grid.world().wrap(32, 32);

        // Act
        grid.set_terrain(a, ids.sand);
        grid.set_terrain(b, ids.snow);

        // Assert
        assert_eq!(grid.terrain_at(b), ids.snow);
    }

    #[test]
    fn 环面绕回的坐标指向同一格() {
        // Arrange
        let (ids, mut grid) = fixture();
        let origin = grid.world().wrap(0, 0);
        let wrapped = grid.world().wrap(64, 64);

        // Act
        grid.set_terrain(origin, ids.forest);

        // Assert
        assert_eq!(grid.terrain_at(wrapped), ids.forest);
    }

    #[test]
    fn 山地阻挡视线() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(ids.mountain.blocks_sight(&table));
    }

    #[test]
    fn 草地不阻挡视线() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!ids.grass.blocks_sight(&table));
    }

    #[test]
    fn 不可通行地形的移动代价为最大值() {
        // 用 u32::MAX 而非 Option，让寻路算法不必对每格做分支判断。
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert_eq!(ids.deep_water.move_cost(&table), u32::MAX);
    }
}
