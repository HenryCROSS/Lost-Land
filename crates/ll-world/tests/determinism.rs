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
use ll_world::zone::ZoneLayout;

/// 由首次运行记录的黄金基准。修改前请阅读本文件顶部说明。
///
/// # 两级坐标系重写（任务 11）为什么改变了这个值
///
/// `WorldState.terrain` 从一次性生成、整体常驻的 [`ChunkGrid`] 换成
/// 按区块流式生成与常驻的 `SurfaceStore`，[`WorldState::hash`] 相应地
/// 从「按 `size` 遍历世界每一格」改为「按 `resident_zones()` 排序后的
/// 区块坐标集合遍历，且额外把区块坐标本身混入哈希」（见其文档「不再
/// 遍历整个世界的每一格」）。测试世界仍然只有一个区块（64×64 的单区块
/// 布局，见 [`test_layout`]），地形内容与迁移前完全相同，但哈希算法
/// 本身的输入构造方式变了，产出的摘要数值随之改变——这是预期之内的
/// 「基准值变了」，不是「断言结构变了」：仍然是「同一份世界产出同一个
/// 摘要」这条断言，只是摘要的计算方式换了输入顺序。人工核对：迁移
/// 前后本文件另外三条测试（序列化往返哈希不变、相同种子哈希相同、
/// 推进时钟哈希改变）全部保持通过，证明哈希仍然对种子/时钟/序列化
/// 敏感，不是退化成常量。
const EXPECTED_WORLD_DIGEST: u64 = 2_466_608_231_210_883_991;

/// 测试用区块布局：边长 64（是噪声格点周期的整数倍，且大于视口跨度），
/// 单个区块——整个测试世界落在这一个区块内，满足
/// [`WorldState::new`] 的全部构造前置条件。
fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
}

#[test]
fn 固定种子的六十四乘六十四世界摘要跨平台稳定() {
    // Arrange
    let params = GenParams {
        seed: 20260817,
        ..GenParams::default()
    };
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let layout = test_layout();
    let spawn = layout.tile_size().wrap(0, 0);

    // Act
    let world = WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn)
        .expect("测试布局满足全部构造前置条件");

    // Assert
    assert_eq!(world.hash(), EXPECTED_WORLD_DIGEST);
}

#[test]
fn 序列化往返后世界哈希不变() {
    // Arrange
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let layout = test_layout();
    let spawn = layout.tile_size().wrap(0, 0);
    let world = WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件");
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
    let layout = test_layout();
    let spawn = layout.tile_size().wrap(0, 0);

    // Act
    let first = WorldState::new(layout, &params, &first_ids, first_table, spawn)
        .expect("测试布局满足全部构造前置条件");
    let second = WorldState::new(layout, &params, &second_ids, second_table, spawn)
        .expect("测试布局满足全部构造前置条件");

    // Assert
    assert_eq!(first.hash(), second.hash());
}

#[test]
fn 推进时钟会改变世界哈希() {
    // Arrange
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let layout = test_layout();
    let spawn = layout.tile_size().wrap(0, 0);
    let mut world = WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件");
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
