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
use ll_world::terrain::base_terrain_fixture;

/// 由首次运行记录的黄金基准。修改前请阅读本文件顶部说明。
///
/// # P4 Task 8（`TerrainKind` 迁入内容注册表）为什么没有改变这个值
///
/// 地形不再是硬编码的 `u16` 枚举值（自然地形 0..8、建筑地形
/// 100..109，中间留空为未来扩展让路），而是
/// `ll_core::ident::Interner::intern` 按 `materialize_base_terrain`
/// 固定顺序发出的稠密 `ContentIndex`。但 `materialize_base_terrain`
/// 特意保持了与旧枚举完全相同的登记顺序（深水、浅水、沙、草、林、丘、
/// 山、雪……），八种自然地形因此仍然落在 `0..8`、数值与迁移前逐一
/// 相同；只有建筑地形的编号从旧版留出空隙的 `100..109` 收缩到紧接着
/// 自然地形之后的 `8..17`（命名空间字符串已经彻底解决了「未来扩展
/// 需要预留编号空间」这个旧问题，见 `ll_world::terrain` 模块文档）。
/// 这条用例的世界完全由 `generate_terrain` 生成，只会用到八种自然
/// 地形，因此迁移前后哈希实测逐位相同——**不是没有验证到位，是这条
/// 用例的场景覆盖不到发生变化的那一段编号**；建筑地形编号变化的真实
/// 影响，由 `crates/ll-sim/tests/replay.rs`（意图流里显式经过一扇门，
/// 门属于建筑地形）已经改变的黄金基准体现。
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
    let (terrain_ids, terrain_table) = base_terrain_fixture();

    // Act
    let world = WorldState::new(test_size(), &params, &terrain_ids, terrain_table)
        .expect("测试尺寸满足全部构造前置条件");

    // Assert
    assert_eq!(world.hash(), EXPECTED_WORLD_DIGEST);
}

#[test]
fn 序列化往返后世界哈希不变() {
    // Arrange
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let world = WorldState::new(
        test_size(),
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
    )
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
        seed: 7,
        ..GenParams::default()
    };
    let (first_ids, first_table) = base_terrain_fixture();
    let (second_ids, second_table) = base_terrain_fixture();

    // Act
    let first = WorldState::new(test_size(), &params, &first_ids, first_table)
        .expect("测试尺寸满足全部构造前置条件");
    let second = WorldState::new(test_size(), &params, &second_ids, second_table)
        .expect("测试尺寸满足全部构造前置条件");

    // Assert
    assert_eq!(first.hash(), second.hash());
}

#[test]
fn 推进时钟会改变世界哈希() {
    // Arrange
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let mut world = WorldState::new(
        test_size(),
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
    )
    .expect("测试尺寸满足全部构造前置条件");
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
    let (terrain_ids, _table) = base_terrain_fixture();
    let size = TorusSize::new(43, 100).expect("43x100 是合法的 TorusSize");

    // Act
    let result = ChunkGrid::new(size, terrain_ids.deep_water);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn 宽度低于视口下限时构造世界失败() {
    // Arrange
    let (terrain_ids, _table) = base_terrain_fixture();
    let size = TorusSize::new(42, 100).expect("42x100 是合法的 TorusSize");

    // Act
    let result = ChunkGrid::new(size, terrain_ids.deep_water);

    // Assert
    assert!(result.is_err());
}

#[test]
fn 高度达到视口下限时可以构造世界() {
    // Arrange
    let (terrain_ids, _table) = base_terrain_fixture();
    let size = TorusSize::new(100, 25).expect("100x25 是合法的 TorusSize");

    // Act
    let result = ChunkGrid::new(size, terrain_ids.deep_water);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn 高度低于视口下限时构造世界失败() {
    // Arrange
    let (terrain_ids, _table) = base_terrain_fixture();
    let size = TorusSize::new(100, 24).expect("100x24 是合法的 TorusSize");

    // Act
    let result = ChunkGrid::new(size, terrain_ids.deep_water);

    // Assert
    assert!(result.is_err());
}
