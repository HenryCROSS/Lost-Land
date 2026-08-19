//! 有界局部地形存储——`Interior`（地下城/建筑内部楼层）用的地形网格。
//!
//! # 与 `ChunkGrid` 平行但不环绕
//!
//! [`crate::chunk::ChunkGrid`] 是环面世界地表的地形存储，按
//! [`crate::chunk::CHUNK_SIZE`] 分块、坐标绕接缝折返。本模块提供同样
//! 「存地形、按坐标读写」的接口，但坐标类型换成
//! [`ll_core::bounded::BoundedPos`]/[`BoundedSize`]——越界坐标在
//! [`BoundedSize::try_pos`] 那一步就被拒绝，根本构造不出来，因此本模块
//! 不需要、也不做任何环绕折返。
//!
//! # 为什么是单一 `Vec`，不分块
//!
//! `ChunkGrid` 按 [`crate::chunk::CHUNK_SIZE`]（32）分块存储，是为了让
//! 数百万格的世界地表能按需分配、避免一次性巨额分配。`Interior` 楼层
//! 不是这种量级——`knowledge/design/coordinate-system-and-layers.md`
//! 十节末尾明确写道「这条对齐关系不适用于 `Interior` 的楼层地图」：
//! 一栋建筑/一处地下城的单层地图整体一次性加载，没有理由为它再套一层
//! 分块管理的复杂度。单一 `Vec<TerrainKind>` 按行主序存储已经足够。

use ll_core::bounded::{BoundedPos, BoundedSize};

use crate::terrain::TerrainKind;

/// 一张有界（不环绕）局部地形图。
///
/// 新建时全部填为调用方指定的 `fill`，理由与 `Chunk` 的对应设计相同
/// （见 [`crate::chunk`] 模块文档）：这只是分配时的占位值，真正的地形
/// 由具体生成器（洞穴算法/房间走廊算法/建筑定义，见设计文档六节）
/// 写入。
#[derive(Debug, Clone)]
pub struct BoundedGrid {
    size: BoundedSize,
    tiles: Vec<TerrainKind>,
}

impl BoundedGrid {
    /// 按给定尺寸建立地形网格，全部格子初始化为 `fill`。
    pub fn new(size: BoundedSize, fill: TerrainKind) -> Self {
        let len = size.width() as usize * size.height() as usize;
        BoundedGrid {
            size,
            tiles: vec![fill; len],
        }
    }

    /// 该网格的尺寸。
    pub fn size(&self) -> BoundedSize {
        self.size
    }

    /// 读取给定坐标处的地形。
    ///
    /// `pos` 的不变式（恒落在 `size` 定义的范围内，见
    /// [`BoundedPos`] 文档）保证这里的索引恒合法——但前提是 `pos` 是
    /// 由这张网格自己的 `size` 构造出来的，与 [`crate::chunk::ChunkGrid`]
    /// 信任调用方传入匹配 `TorusPos` 的既有约定一致，本模块不重复做
    /// 一次运行时校验。
    pub fn terrain_at(&self, pos: BoundedPos) -> TerrainKind {
        self.tiles[self.index_of(pos)]
    }

    /// 写入给定坐标处的地形。
    pub fn set_terrain(&mut self, pos: BoundedPos, kind: TerrainKind) {
        let idx = self.index_of(pos);
        self.tiles[idx] = kind;
    }

    /// 把有界局部坐标换算成 `tiles` 里的下标，行主序。
    fn index_of(&self, pos: BoundedPos) -> usize {
        pos.y() as usize * self.size.width() as usize + pos.x() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::base_terrain_fixture;

    #[test]
    fn 写入后可读回同一地形() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let size = BoundedSize::new(10, 10).expect("10x10 是合法尺寸");
        let mut grid = BoundedGrid::new(size, ids.deep_water);
        let pos = size.try_pos(3, 4).expect("3,4 在 10x10 范围内");

        // Act
        grid.set_terrain(pos, ids.mountain);

        // Assert
        assert_eq!(grid.terrain_at(pos), ids.mountain);
    }

    #[test]
    fn 越界坐标构造不出可用于查询的位置() {
        // 与 ChunkGrid 的「环面绕回坐标指向同一格」测试形成对照：环面
        // 上 (10,10) 会被 TorusSize::wrap 绕回 (0,0)；有界地图没有这条
        // 退路——BoundedSize::try_pos 对越界坐标直接返回 None，
        // terrain_at/set_terrain 因此连一个越界坐标都拿不到，不存在
        // 「贴着东边写入、结果出现在西边」这种绕回效果。
        // Arrange
        let size = BoundedSize::new(10, 10).expect("10x10 是合法尺寸");

        // Act
        let out_of_bounds = size.try_pos(10, 10);

        // Assert
        assert!(out_of_bounds.is_none());
    }
}
