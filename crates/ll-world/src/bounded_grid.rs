//! 有界局部地形存储——`Interior`（地下城/建筑内部楼层）用的地形网格。
//!
//! # 与 `ChunkGrid` 平行但不环绕
//!
//! [`crate::chunk::ChunkGrid`] 是环面世界地表（区块内部）的地形存储，
//! 坐标绕接缝折返。本模块提供同样「存地形、按坐标读写」的接口，但
//! 坐标类型换成 [`ll_core::bounded::BoundedPos`]/[`BoundedSize`]——
//! 越界坐标在 [`BoundedSize::try_pos`] 那一步就被拒绝，根本构造不出
//! 来，因此本模块不需要、也不做任何环绕折返。
//!
//! # 为什么是单一 `Vec`
//!
//! 与 [`crate::chunk::ChunkGrid`]（丙案取消存储块层之后，两者在实现
//! 形状上完全对称，见其模块文档）同理：`Interior` 楼层的惰性分配与
//! 流式加载由更外层的结构（`Interior::floors` 的 `HashMap<i16,
//! BoundedGrid>` 稀疏索引）负责，`BoundedGrid` 自己不需要再为单个楼层
//! 内部套一层分块管理的复杂度。单一 `Vec<TerrainKind>` 按行主序存储
//! 已经足够。

use ll_core::bounded::{BoundedPos, BoundedSize};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::terrain::TerrainKind;

/// 一张有界（不环绕）局部地形图。
///
/// 新建时全部填为调用方指定的 `fill`，理由与 [`crate::chunk::ChunkGrid`]
/// 的对应设计相同（见其文档「为什么 `fill` 是参数」一节）：这只是
/// 分配时的占位值，真正的地形由具体生成器（洞穴算法/房间走廊算法/
/// 建筑定义，见设计文档六节）写入。
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

/// [`BoundedGrid`] 序列化用的扁平化表示：尺寸加按行主序排列的全部
/// 地形格。
///
/// 与 `crate::state` 里 [`crate::chunk::ChunkGrid`] 的序列化实现同一个
/// 手法（私有字段不能直接派生，借公开的 `size`/`terrain_at` 接口手写）
/// ——批次 C（`Interior` 楼层，见 `crate::interior`）第一次真正需要
/// `BoundedGrid` 独立于任何更大结构完整序列化，这里补上。不同于
/// `ChunkGrid` 的序列化实现放在 `state.rs`（因为 `chunk.rs` 在那一批次
/// 被冻结，见其文档），本批次 `bounded_grid.rs` 没有被冻结，实现直接
/// 放在类型自己的文件里，不必绕道。
#[derive(Serialize, Deserialize)]
struct BoundedGridData {
    width: u32,
    height: u32,
    tiles: Vec<TerrainKind>,
}

impl Serialize for BoundedGrid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let size = self.size();
        let mut tiles = Vec::with_capacity(size.width() as usize * size.height() as usize);
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                let pos = size.try_pos(x, y).expect("行主序遍历范围内的坐标恒合法");
                tiles.push(self.terrain_at(pos));
            }
        }
        BoundedGridData {
            width: size.width(),
            height: size.height(),
            tiles,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BoundedGrid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = BoundedGridData::deserialize(deserializer)?;
        let size = BoundedSize::new(data.width, data.height)
            .ok_or_else(|| D::Error::custom("存档中的有界地图尺寸非法"))?;

        let expected_len = data.width as usize * data.height as usize;
        if data.tiles.len() != expected_len {
            return Err(D::Error::custom("存档中的地形格数量与尺寸不匹配"));
        }

        // fill 只是 BoundedGrid::new 分配时的占位值，下面的双重循环会
        // 把每一格都覆写一遍——与 ChunkGrid 反序列化实现同一个理由，
        // 见 crate::state 的对应注释。
        let fill = *data
            .tiles
            .first()
            .ok_or_else(|| D::Error::custom("存档中的地形格数据为空"))?;
        let mut grid = BoundedGrid::new(size, fill);
        let mut tiles = data.tiles.into_iter();
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                let kind = tiles.next().expect("长度已在上面校验与预期长度相等");
                let pos = size.try_pos(x, y).expect("行主序遍历范围内的坐标恒合法");
                grid.set_terrain(pos, kind);
            }
        }
        Ok(grid)
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
