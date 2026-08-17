//! 按固定边长分块的地形存储。
//!
//! # 为什么分块
//!
//! 世界可达数百万格，整块 `Vec` 一次性分配会吃掉大量内存且无法按需
//! 生成。[`CHUNK_SIZE`] 取 32 是权衡点——再小则块管理开销占比过高，
//! 再大则单块内存浪费明显。

use ll_core::torus::{TorusPos, TorusSize};

use crate::WorldError;
use crate::terrain::TerrainKind;

/// 每块地形的边长（格）。
pub const CHUNK_SIZE: u32 = 32;

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

/// 一块 `CHUNK_SIZE × CHUNK_SIZE` 的地形。
///
/// 新建时全部填为 [`TerrainKind::DEEP_WATER`]：这只是分配时的占位值，
/// 真正的地形由后续批次的生成流程写入。
#[derive(Debug, Clone)]
struct Chunk {
    tiles: Vec<TerrainKind>,
}

impl Chunk {
    fn new() -> Self {
        Chunk {
            tiles: vec![TerrainKind::DEEP_WATER; (CHUNK_SIZE * CHUNK_SIZE) as usize],
        }
    }
}

/// 按 [`CHUNK_SIZE`] 分块存储的环面地形网格。
#[derive(Debug, Clone)]
pub struct ChunkGrid {
    world: TorusSize,
    chunks_x: u32,
    chunks: Vec<Chunk>,
}

impl ChunkGrid {
    /// 按世界尺寸建立分块网格。
    ///
    /// 世界任一维度小于视口跨度（[`MIN_WORLD_WIDTH`] × [`MIN_WORLD_HEIGHT`]）
    /// 时返回 [`WorldError::WorldTooSmall`]：与其让缺陷在运行时表现为
    /// 视觉异常（重复坐标、地形填不满留黑块），不如在构造点直接拒绝。
    pub fn new(world: TorusSize) -> Result<Self, WorldError> {
        if world.width() < MIN_WORLD_WIDTH || world.height() < MIN_WORLD_HEIGHT {
            return Err(WorldError::WorldTooSmall {
                width: world.width(),
                height: world.height(),
            });
        }

        // 用除法向上取整而非要求世界宽高恰好整除 CHUNK_SIZE：边缘那一块
        // 允许只用到一部分，换来对任意合法世界尺寸的支持。
        let chunks_x = world.width().div_ceil(CHUNK_SIZE);
        let chunks_y = world.height().div_ceil(CHUNK_SIZE);
        let chunk_total = (chunks_x as usize) * (chunks_y as usize);

        Ok(ChunkGrid {
            world,
            chunks_x,
            chunks: (0..chunk_total).map(|_| Chunk::new()).collect(),
        })
    }

    /// 该网格所属的世界尺寸。
    pub fn world(&self) -> TorusSize {
        self.world
    }

    /// 分块总数。
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// 读取给定坐标处的地形。
    pub fn terrain_at(&self, pos: TorusPos) -> TerrainKind {
        let (chunk_index, local_index) = self.locate(pos);
        self.chunks[chunk_index].tiles[local_index]
    }

    /// 写入给定坐标处的地形。
    pub fn set_terrain(&mut self, pos: TorusPos, kind: TerrainKind) {
        let (chunk_index, local_index) = self.locate(pos);
        self.chunks[chunk_index].tiles[local_index] = kind;
    }

    /// 把环面坐标换算为（块索引，块内偏移）。
    ///
    /// 块边界是分块存储最容易出错的地方：算错块索引会让写入落到邻块。
    /// 换算集中在这一处，供 `terrain_at`/`set_terrain` 共用，避免两处
    /// 各写一份而彼此漂移。
    fn locate(&self, pos: TorusPos) -> (usize, usize) {
        // TorusPos 的不变式保证坐标恒非负，转换成 u32 不会丢失信息。
        let x = pos.x() as u32;
        let y = pos.y() as u32;

        let chunk_x = x / CHUNK_SIZE;
        let chunk_y = y / CHUNK_SIZE;
        let local_x = x % CHUNK_SIZE;
        let local_y = y % CHUNK_SIZE;

        let chunk_index = (chunk_y * self.chunks_x + chunk_x) as usize;
        let local_index = (local_y * CHUNK_SIZE + local_x) as usize;
        (chunk_index, local_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::torus::TorusSize;

    fn grid() -> ChunkGrid {
        let world = TorusSize::new(64, 64).expect("常量非零");
        ChunkGrid::new(world).expect("64x64 大于视口跨度")
    }

    #[test]
    fn 世界小于视口跨度时构造失败() {
        // 世界任一维度小于 43×25 格时，渲染层相机会产出重复坐标，
        // 地形填不满留黑块。与其让缺陷在运行时表现为视觉异常，
        // 不如在构造点直接拒绝。
        // Arrange
        let tiny = TorusSize::new(20, 20).expect("常量非零");

        // Act
        let result = ChunkGrid::new(tiny);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 写入后可读回同一地形() {
        // Arrange
        let mut grid = grid();
        let pos = grid.world().wrap(10, 20);

        // Act
        grid.set_terrain(pos, TerrainKind::MOUNTAIN);

        // Assert
        assert_eq!(grid.terrain_at(pos), TerrainKind::MOUNTAIN);
    }

    #[test]
    fn 跨块边界的写入互不干扰() {
        // 块边界是分块存储最容易出错的地方：算错块索引会让写入落到邻块。
        // Arrange
        let mut grid = grid();
        let inside = grid.world().wrap(31, 31);
        let across = grid.world().wrap(32, 32);

        // Act
        grid.set_terrain(inside, TerrainKind::SAND);
        grid.set_terrain(across, TerrainKind::SNOW);

        // Assert
        assert_eq!(grid.terrain_at(inside), TerrainKind::SAND);
    }

    #[test]
    fn 环面绕回的坐标指向同一格() {
        // Arrange
        let mut grid = grid();
        let origin = grid.world().wrap(0, 0);
        let wrapped = grid.world().wrap(64, 64);

        // Act
        grid.set_terrain(origin, TerrainKind::FOREST);

        // Assert
        assert_eq!(grid.terrain_at(wrapped), TerrainKind::FOREST);
    }

    #[test]
    fn 山地阻挡视线() {
        // Arrange & Act & Assert
        assert!(TerrainKind::MOUNTAIN.blocks_sight());
    }

    #[test]
    fn 草地不阻挡视线() {
        // Arrange & Act & Assert
        assert!(!TerrainKind::GRASS.blocks_sight());
    }

    #[test]
    fn 不可通行地形的移动代价为最大值() {
        // 用 u32::MAX 而非 Option，让寻路算法不必对每格做分支判断。
        // Arrange & Act & Assert
        assert_eq!(TerrainKind::DEEP_WATER.move_cost(), u32::MAX);
    }
}
