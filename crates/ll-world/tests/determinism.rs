//! 跨平台确定性回归。
//!
//! 本文件里的期望值是**黄金基准**：它们由算法定义唯一确定，在 Windows
//! 与 Linux 上必须逐位相同。
//!
//! # 测试失败意味着什么
//!
//! 若某次改动让这里的摘要变了，只有两种可能：
//!
//! 1. 有意修改了算法或常量——那么更新期望值，并在提交信息里说明为什么。
//! 2. **无意引入了平台相关行为**（最常见的是浮点运算，或依赖了哈希表
//!    的遍历顺序）。这是必须立刻修复的缺陷。
//!
//! **绝不允许「测试挂了就把期望值改成实际值」**——那等于删掉这道防线。
//! 与 `crates/ll-core/tests/determinism.rs` 保持同一条规矩。

use ll_core::torus::TorusSize;
use ll_world::chunk::ChunkGrid;
use ll_world::generate::GenParams;
use ll_world::state::WorldState;

/// 由首次运行记录的黄金基准。修改前请阅读本文件顶部说明。
///
/// # 本次重冻的原因（P2 最终评审 Important 3）
///
/// `ll_world::noise::TileableNoise::octaves` 从「只在 `CELL_SIZE`（16
/// 格）基础上叠加更高频」改为「先叠加与世界尺寸同量级的大陆尺度层，
/// 再叠加到 `CELL_SIZE` 及更细」——这是地形生成算法本身的有意变更
/// （详见 `noise.rs` 模块文档），不是平台相关行为泄漏，因此这里的
/// 期望值随之更新为新算法在同一种子、同一尺寸下的真实产出。
const EXPECTED_WORLD_DIGEST: u64 = 17_645_793_944_024_546_775;

/// 测试世界尺寸：64 是噪声格点周期的整数倍，且大于视口跨度，满足
/// [`WorldState::new`] 的全部构造前置条件。
fn test_size() -> TorusSize {
    TorusSize::new(64, 64).expect("64x64 满足整除与视口跨度两条约束")
}

#[test]
fn 固定种子的六十四乘六十四世界摘要跨平台稳定() {
    // Arrange
    let params = GenParams {
        seed: 20260817,
        ..GenParams::default()
    };

    // Act
    let world = WorldState::new(test_size(), &params).expect("测试尺寸满足全部构造前置条件");

    // Assert
    assert_eq!(world.hash(), EXPECTED_WORLD_DIGEST);
}

#[test]
fn 序列化往返后世界哈希不变() {
    // Arrange
    let world =
        WorldState::new(test_size(), &GenParams::default()).expect("测试尺寸满足全部构造前置条件");
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
        seed: 7,
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
    let mut world =
        WorldState::new(test_size(), &GenParams::default()).expect("测试尺寸满足全部构造前置条件");
    let hash_before = world.hash();

    // Act
    world.advance(1);

    // Assert
    assert_ne!(world.hash(), hash_before);
}

/// 世界尺寸下限常量（43×25）从渲染层 `Camera::visible_tiles` 的跨度
/// 手抄进 `chunk.rs`，世界层不应反向依赖渲染层。这四条测试直接命中
/// `ChunkGrid::new` 的边界，是防止这两处数值悄悄漂移的唯一机制——
/// `chunk.rs` 里的 `MIN_WORLD_WIDTH`/`MIN_WORLD_HEIGHT` 是私有常量，
/// 集成测试看不到它们，只能靠黑盒边界断言间接钉住。
#[test]
fn 宽度达到视口下限时可以构造世界() {
    // Arrange
    let size = TorusSize::new(43, 100).expect("43x100 是合法的 TorusSize");

    // Act
    let result = ChunkGrid::new(size);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn 宽度低于视口下限时构造世界失败() {
    // Arrange
    let size = TorusSize::new(42, 100).expect("42x100 是合法的 TorusSize");

    // Act
    let result = ChunkGrid::new(size);

    // Assert
    assert!(result.is_err());
}

#[test]
fn 高度达到视口下限时可以构造世界() {
    // Arrange
    let size = TorusSize::new(100, 25).expect("100x25 是合法的 TorusSize");

    // Act
    let result = ChunkGrid::new(size);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn 高度低于视口下限时构造世界失败() {
    // Arrange
    let size = TorusSize::new(100, 24).expect("100x24 是合法的 TorusSize");

    // Act
    let result = ChunkGrid::new(size);

    // Assert
    assert!(result.is_err());
}
