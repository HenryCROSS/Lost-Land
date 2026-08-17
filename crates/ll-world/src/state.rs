//! 世界状态：种子、时钟、尺寸与地形的聚合，以及序列化往返。
//!
//! # 为什么必须完整可序列化且全程整数
//!
//! [`WorldState`] 是模式 3（自由读档）的地基：存档就是把这个结构体
//! 序列化到磁盘，读档就是反序列化回来，不需要额外的迁移或重算步骤。
//! 只要有一个字段没能完整序列化，读档后的世界就可能与存档前不同，
//! 而这类缺陷通常要等到玩家读档后才被发现，为时已晚。
//!
//! 全程禁止浮点数，理由同 `ll-core`：浮点在不同平台/编译器/优化级别下
//! 的运算结果可能有细微差异，跨平台存档兼容性会被悄悄破坏。

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use ll_core::hashing::StateHasher;
use ll_core::time::Tick;
use ll_core::torus::TorusSize;

use crate::WorldError;
use crate::chunk::ChunkGrid;
use crate::generate::{GenParams, generate_terrain};
use crate::terrain::TerrainKind;

/// 完整的世界状态：种子、时钟、尺寸与地形。
///
/// 全部字段公开且可序列化：存档格式就是这个结构体本身，不经过额外的
/// DTO 转换层——多一层转换就多一处可能与本体字段漂移的地方。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    /// 生成本世界地形所用的种子。
    pub seed: u64,
    /// 当前世界时钟。全世界只有这一个时钟，见 `ll_core::time` 的说明。
    pub clock: Tick,
    /// 世界尺寸。
    pub size: TorusSize,
    /// 世界地形。
    pub terrain: ChunkGrid,
}

impl WorldState {
    /// 按尺寸与生成参数创建一个新世界，时钟从零开始。
    pub fn new(size: TorusSize, params: &GenParams) -> Result<WorldState, WorldError> {
        let terrain = generate_terrain(size, params)?;
        Ok(WorldState {
            seed: params.seed,
            clock: Tick(0),
            size,
            terrain,
        })
    }

    /// 推进世界时钟 `ticks` 格。
    ///
    /// `ticks` 允许为负：世界时钟内部只是一个 `i64`，不排斥读档迁移或
    /// 时间倒流类效果回拨时钟的用法。
    pub fn advance(&mut self, ticks: i64) {
        self.clock = Tick(self.clock.0 + ticks);
    }

    /// 把整个世界状态归约成一个 64 位摘要。
    ///
    /// 用于「两次运行/序列化往返是否产生了相同的世界」这类断言，是
    /// 确定性重放与存档回归测试的基础设施（详见 `ll_core::hashing`）。
    /// 地形按世界的规范行主序逐格混入，顺序固定，保证同一世界恒产出
    /// 同一摘要。
    pub fn hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        hasher.write_u64(self.seed);
        hasher.write_i64(self.clock.0);
        hasher.write_u64(u64::from(self.size.width()));
        hasher.write_u64(u64::from(self.size.height()));
        for y in 0..self.size.height() as i32 {
            for x in 0..self.size.width() as i32 {
                let pos = self.size.wrap(x, y);
                hasher.write_u64(u64::from(self.terrain.terrain_at(pos).0));
            }
        }
        hasher.finish()
    }
}

/// [`ChunkGrid`] 序列化用的扁平化表示：尺寸加按行主序排列的全部地形格。
///
/// 不直接在 `chunk.rs` 里给 [`ChunkGrid`] 派生 `Serialize`/`Deserialize`：
/// 那个文件是本批次明确不允许改动的既有代码。改为在本文件借
/// [`ChunkGrid`] 已公开的 `world`/`terrain_at`/`set_terrain` 接口手写
/// 序列化实现——`ChunkGrid` 是本 crate 的本地类型，为它实现外部 trait
/// 不违反孤儿规则，因此可以在任意模块完成，不必触碰 `chunk.rs`。
#[derive(Serialize, Deserialize)]
struct ChunkGridData {
    width: u32,
    height: u32,
    tiles: Vec<TerrainKind>,
}

impl Serialize for ChunkGrid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let world = self.world();
        let mut tiles = Vec::with_capacity((world.width() as usize) * (world.height() as usize));
        for y in 0..world.height() as i32 {
            for x in 0..world.width() as i32 {
                tiles.push(self.terrain_at(world.wrap(x, y)));
            }
        }
        ChunkGridData {
            width: world.width(),
            height: world.height(),
            tiles,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChunkGrid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = ChunkGridData::deserialize(deserializer)?;
        let size = TorusSize::new(data.width, data.height)
            .ok_or_else(|| D::Error::custom("存档中的世界尺寸非法"))?;

        let expected_len = (data.width as usize) * (data.height as usize);
        if data.tiles.len() != expected_len {
            return Err(D::Error::custom("存档中的地形格数量与尺寸不匹配"));
        }

        let mut grid = ChunkGrid::new(size).map_err(|err| D::Error::custom(err.to_string()))?;
        let mut tiles = data.tiles.into_iter();
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                let kind = tiles.next().expect("长度已在上面校验与预期长度相等");
                grid.set_terrain(size.wrap(x, y), kind);
            }
        }
        Ok(grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试世界尺寸：64 是噪声格点周期的整数倍，且大于视口跨度，
    /// 满足 [`WorldState::new`] 的全部构造前置条件。
    fn test_size() -> TorusSize {
        TorusSize::new(64, 64).expect("64x64 满足整除与视口跨度两条约束")
    }

    #[test]
    fn 序列化往返后世界哈希不变() {
        // Arrange
        let world = WorldState::new(test_size(), &GenParams::default())
            .expect("测试尺寸满足全部构造前置条件");
        let original_hash = world.hash();

        // Act
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
        let decoded: WorldState = serde_json::from_slice(&encoded).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded.hash(), original_hash);
    }

    #[test]
    fn 相同种子与尺寸生成的世界哈希相同() {
        // Arrange
        let params = GenParams {
            seed: 42,
            ..GenParams::default()
        };

        // Act
        let first = WorldState::new(test_size(), &params).expect("测试尺寸满足全部构造前置条件");
        let second = WorldState::new(test_size(), &params).expect("测试尺寸满足全部构造前置条件");

        // Assert
        assert_eq!(first.hash(), second.hash());
    }

    #[test]
    fn 推进时钟会改变世界哈希() {
        // Arrange
        let mut world = WorldState::new(test_size(), &GenParams::default())
            .expect("测试尺寸满足全部构造前置条件");
        let hash_before = world.hash();

        // Act
        world.advance(1);

        // Assert
        assert_ne!(world.hash(), hash_before);
    }

    #[test]
    fn 时钟可以倒拨() {
        // 读档迁移或时间倒流类效果可能需要回拨时钟，advance 不应拒绝
        // 负值。
        // Arrange
        let mut world = WorldState::new(test_size(), &GenParams::default())
            .expect("测试尺寸满足全部构造前置条件");
        world.advance(100);

        // Act
        world.advance(-100);

        // Assert
        assert_eq!(world.clock, Tick(0));
    }
}
