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
use crate::entity::{Agent, Arena, ThinPopulation};
use crate::generate::{GenParams, generate_terrain};
use crate::terrain::TerrainKind;

/// 完整的世界状态：种子、时钟、尺寸、地形、薄层人口与厚层实体池。
///
/// 全部字段公开：存档格式就是这个结构体本身，不经过额外的 DTO 转换层
/// ——多一层转换就多一处可能与本体字段漂移的地方。
///
/// # `population`/`actors` 暂不参与序列化（P3 阶段的已知限制）
///
/// [`ThinPopulation`] 与 [`Arena<Agent>`] 目前不派生 `serde`：前者的
/// `profession` 列、后者的 `Agent::profession` 都是 `ll_core::ident::ContentIndex`
/// ——该类型在 `ll_core::ident` 模块文档里被明确标记为不可持久化（依赖
/// mod 加载顺序，存档必须写字符串 ID 而非裸索引）；`Agent::pos` 是
/// `TorusPos`，其唯一构造路径 `TorusSize::wrap` 需要世界尺寸上下文，
/// `ll-core` 里也没有为它提供可脱离该上下文使用的 `serde` 实现。两者
/// 真正落地需要先给内容注册表与位置反序列化补上校验通道，这属于后续
/// 批次（存档格式在 P5 冻结前）的工作，本任务只建两层的结构与操作，
/// 用 `#[serde(skip)]` 如实标记这个已知缺口，而不是假装已经完整可
/// 序列化。
///
/// # 反序列化必须交叉校验 `size` 与 `terrain` 的尺寸（裁定 P2-6 的同源修复）
///
/// `size`（[`TorusSize`]）与 `terrain`（[`ChunkGrid`]）各自的反序列化都已
/// 自证合法——前者不为零且不超过上限，后者的瓦片数与自带的宽高字段
/// 匹配——但**两者互不知道对方的存在**。存档若被篡改或损坏成
/// `size=512×320` 而 `terrain` 实际只有 `64×64` 格，两个字段各自看都
/// 合法，唯独合在一起不自洽：[`Self::hash`]（或任何按 `size` 遍历坐标、
/// 用 [`ChunkGrid::terrain_at`] 取值的调用）会用 512×320 的坐标去索引
/// 一个按 64×64 分块的网格，直接越界 panic。
///
/// 这与 [`TorusSize`] 本身「反序列化必须重新经过 `new` 的校验」
/// （裁定 P2-6，见 `ll_core::torus` 的说明）是同一类缺陷、同一个修法：
/// 存档是外部不可信输入，任何输入都不得 panic，只允许返回 `Err`
/// （规格 §14.3）。因此这里同样用 `#[serde(try_from = "WorldStateRepr")]`
/// 让反序列化必经一次交叉校验，而不是让两个字段的合法性各自证明、
/// 合在一起却可能矛盾。`Serialize` 不受影响，仍是直接派生。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "WorldStateRepr")]
pub struct WorldState {
    /// 生成本世界地形所用的种子。
    pub seed: u64,
    /// 当前世界时钟。全世界只有这一个时钟，见 `ll_core::time` 的说明。
    pub clock: Tick,
    /// 世界尺寸。
    pub size: TorusSize,
    /// 世界地形。
    pub terrain: ChunkGrid,
    /// 薄层人口：数十万到数百万背景 NPC，列式排布。P3 阶段可以为空，
    /// 见 [`ThinPopulation`] 模块文档。不参与序列化，理由见本类型文档。
    #[serde(skip)]
    pub population: ThinPopulation,
    /// 厚层实体池：数百个被真正模拟的实体，行式排布。P3 阶段可以只有
    /// 玩家与几个敌人，见 [`Arena`] 模块文档。不参与序列化，理由同上。
    #[serde(skip)]
    pub actors: Arena<Agent>,
}

/// [`WorldState`] 反序列化的中转表示。
///
/// 见 [`WorldState`] 文档「反序列化必须交叉校验」一节：这个类型本身没有
/// 任何跨字段不变式，只是让 serde 有一个「先把四个字段各自反序列化
/// （各自的校验仍然生效），再交给 [`TryFrom`] 做交叉校验」的中转落点。
#[derive(Deserialize)]
struct WorldStateRepr {
    seed: u64,
    clock: Tick,
    size: TorusSize,
    terrain: ChunkGrid,
}

impl TryFrom<WorldStateRepr> for WorldState {
    type Error = String;

    /// 唯一的构造路径：在委托给字段本身校验之后，额外校验
    /// `terrain.world() == size`——两者是同一个世界尺寸的两份独立记录，
    /// 必须一致，否则按 `size` 遍历坐标去查 `terrain` 就会越界。
    fn try_from(repr: WorldStateRepr) -> Result<Self, Self::Error> {
        if repr.terrain.world() != repr.size {
            return Err(format!(
                "存档中的世界尺寸 {}x{} 与地形网格的实际尺寸 {}x{} 不一致",
                repr.size.width(),
                repr.size.height(),
                repr.terrain.world().width(),
                repr.terrain.world().height(),
            ));
        }
        Ok(WorldState {
            seed: repr.seed,
            clock: repr.clock,
            size: repr.size,
            terrain: repr.terrain,
            // 两者当前不参与序列化（见 WorldState 文档），存档里没有
            // 对应数据可读，读档后总是从空状态开始。
            population: ThinPopulation::default(),
            actors: Arena::default(),
        })
    }
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
            population: ThinPopulation::default(),
            actors: Arena::default(),
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

    // 「序列化往返后哈希不变」「相同种子与尺寸生成的哈希相同」
    // 「推进时钟会改变哈希」这三条曾经在本文件与
    // `tests/determinism.rs` 里逐字重复。保留在集成测试
    // （`tests/determinism.rs`）而不是这里：那边本就收着黄金基准哈希，
    // 用的是真实 `serde_json` 格式与公开 API，是这几条行为实际生效的
    // 层级；这里的单元测试只留 [`WorldState::advance`] 本身的边界行为
    // （负值回拨）与本次新增的 `try_from` 交叉校验，两组关注点不重叠。

    #[test]
    fn 世界尺寸与地形尺寸不一致的存档无法反序列化() {
        // 模拟被篡改或损坏的存档：地形网格实际是测试尺寸（64x64），
        // 但 size 字段被改成了另一个尺寸——两个字段各自反序列化都
        // 合法，只有合在一起才不自洽，必须靠交叉校验拦住。
        // Arrange
        let world = WorldState::new(test_size(), &GenParams::default())
            .expect("测试尺寸满足全部构造前置条件");
        let mut tampered: serde_json::Value =
            serde_json::to_value(&world).expect("WorldState 全部字段可序列化");
        tampered["size"] = serde_json::json!({ "width": 128, "height": 128 });

        // Act
        let result: Result<WorldState, _> = serde_json::from_value(tampered);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 尺寸一致的存档可以正常往返() {
        // 与上一条相反的分支：size 与 terrain 尺寸一致时，交叉校验
        // 必须放行，不能误伤合法存档。
        // Arrange
        let world = WorldState::new(test_size(), &GenParams::default())
            .expect("测试尺寸满足全部构造前置条件");
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");

        // Act
        let result: Result<WorldState, _> = serde_json::from_slice(&encoded);

        // Assert
        assert!(result.is_ok());
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
