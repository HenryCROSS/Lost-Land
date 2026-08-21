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
/// 遍历整个世界的每一格」）。测试世界仍然只有一个区块，地形内容与
/// 迁移前完全相同，但哈希算法本身的输入构造方式变了，产出的摘要数值
/// 随之改变——这是预期之内的「基准值变了」，不是「断言结构变了」：
/// 仍然是「同一份世界产出同一个摘要」这条断言，只是摘要的计算方式
/// 换了输入顺序。人工核对：迁移前后本文件另外三条测试（序列化往返
/// 哈希不变、相同种子哈希相同、推进时钟哈希改变）全部保持通过，证明
/// 哈希仍然对种子/时钟/序列化敏感，不是退化成常量。
///
/// # 裁定 CS-6：为什么区块边长与种子都跟着换了（不只是数值变了）
///
/// 任务 11 验证黄金基准时意外发现：旧版用的 64×64（单区块）世界
/// 在**任意**种子下都是全深水地形（或至少只有一两种地形），不是某个
/// 倒霉种子的巧合——根源在 [`ll_world::noise::TileableNoise::octaves`]：
/// 大陆尺度那一层（权重最高的一层）的格点周期是
/// `period / coarse_scale`，而 `coarse_scale = max_pow2_divisor(gcd(period_x,
/// period_y))`；当世界是正方形且边长是 2 的幂时（64×64 正是这种情形，
/// `period = 64/16 = 4`），`gcd(4, 4) = 4`，`coarse_scale` 恰好等于
/// `period` 本身，大陆尺度层的格点周期因此退化成 1×1——**整层在全图
/// 范围内是常数**，只有更细的层还有起伏，起伏幅度不足以跨越从深水到
/// 山地这么宽的阈值区间。这条测试因此实际上只覆盖了
/// `height_to_terrain`（`crates/ll-world/src/generate.rs`）八条分支
/// 里的一两条，`hash()` 摘要的地形部分也几乎不携带任何区分度——很多
/// 不同的算法错误都不会让这条测试变红。
///
/// 换成边长 48（`period = 48/16 = 3` 是奇数，`gcd(3, 3) = 3` 的最大
/// 2 的幂因子是 1，大陆尺度层因此有真正的 3×3 格点起伏，不再退化）
/// 之后，种子 5 产出的地形覆盖全部八种（深水、浅水、沙滩、草地、
/// 森林、丘陵、山地、雪地），既有水域也有阻挡视线的山地——真正让
/// `height_to_terrain` 的每条分支、以及依赖地形多样性的下游逻辑都被
/// 这条测试实际跑到。48 仍然满足 [`ZoneLayout::new`] 的两条约束
/// （`CELL_SIZE` 的整数倍、不小于视口跨度 43），是丙案取消
/// `CHUNK_SIZE` 对齐要求后新解锁的合法取值（旧版甲案下 48 不是 32 的
/// 整数倍，构造会直接失败，见 `crates/ll-world/src/zone.rs` 对应测试）。
///
/// # 脚本状态存储批次（裁定 P5-9）为什么又改了这个值
///
/// [`WorldState::hash`] 新混入 `player_entity`（`Option<EntityId>`）与
/// `global_script_state`——本测试世界两者都是空/`None`，但混入本身
/// 仍然改变了字节流（`write_optional_entity` 恒写一个判别字节，
/// `write_script_state` 恒写一个长度字节），因此摘要数值随之改变。
/// 与「两级坐标系重写」那次一样：断言结构没变（仍是「同一个世界产出
/// 同一个摘要」），变的只是输入构造方式，本文件另外三条测试（序列化
/// 往返哈希不变、相同种子哈希相同、推进时钟哈希改变）迁移后仍然全部
/// 通过，证明哈希依旧对种子/时钟/序列化敏感。
///
/// # 落地探索记忆批次为什么又改了这个值
///
/// [`WorldState::hash`] 新混入 `self.exploration.write_hash(&mut hasher)`
/// （见 [`ll_world::exploration::ExplorationMemory::write_hash`]）——
/// 本测试世界从不调用
/// [`ll_world::exploration::ExplorationMemory::mark_explored`]，
/// `exploration` 全程是一份空记忆，但混入本身仍然改变字节流（先写一个
/// 恒为零的「已探索区块数」）。人工核验（真实执行）：把
/// `crates/ll-world/src/state.rs` 的 `hash()` 里
/// `self.exploration.write_hash(&mut hasher);` 这一行临时删掉重新跑
/// 这条测试，摘要精确回到本次重冻之前的旧常量
/// `3_209_542_191_240_274_209`——确认这次变化完全、只来自这一处改动，
/// 恢复后重新跑通再确认新常量 `17_388_368_992_654_069_569` 稳定复现。
///
/// # 击杀与死亡记录批次为什么又改了这个值
///
/// [`WorldState::hash`] 新混入 `self.history`（长度 + 逐条历史事件）
/// 与 `self.next_world_id`——本测试世界不涉及任何实体/击杀，两者恒是
/// 空/零，但与前两次重冻同一个理由：混入本身（哪怕只是恒定的「长度
/// 为零」「计数器为零」这两个字节）仍然改变了喂进哈希器的字节流。
/// 人工核验（真实执行）：把 `crates/ll-world/src/state.rs` 的 `hash()`
/// 里混入 `self.history`/`self.next_world_id` 的三行临时删掉重新跑
/// 这条测试，摘要精确回到本次重冻之前的旧常量
/// `17_388_368_992_654_069_569`，恢复后重新跑通再确认新常量
/// `7_807_321_984_535_558_017` 稳定复现。
///
/// # 无名单位击杀计数批次为什么又改了这个值（决策一）
///
/// [`WorldState::hash`] 新混入 `self.kill_counts`（长度 + 逐条归并计数，
/// 见其字段文档「参与 hash()」一节）——本测试世界不涉及任何击杀，
/// `kill_counts` 恒为空表，但与前三次重冻同一个理由：混入本身（哪怕
/// 只是恒定的「长度为零」这一个字节）仍然改变了喂进哈希器的字节流。
/// 人工核验（真实执行）：把 `crates/ll-world/src/state.rs` 的 `hash()`
/// 里混入 `self.kill_counts` 的那几行临时删掉重新跑这条测试，摘要精确
/// 回到本次重冻之前的旧常量 `7_807_321_984_535_558_017`，恢复后重新
/// 跑通再确认新常量 `13_774_070_666_589_385_121` 稳定复现。
const EXPECTED_WORLD_DIGEST: u64 = 13_774_070_666_589_385_121;

// # 等级与经验系统落地批次：本次没有重冻，如实记录为什么不可能变
//
// `WorldState::hash` 新增混入的 `Agent::level`/`experience`/
// `xp_to_next_level` 三个字段只在 `for agent in self.actors.iter()`
// 循环体内被读取——本文件下方
// `固定种子的四十八乘四十八世界摘要跨平台稳定` 这条测试的世界完全由
// `WorldState::new(layout, &params, &terrain_ids, terrain_table,
// spawn)` 直接构造，全程没有任何 `world.actors.spawn(..)` 调用（`grep
// actors.spawn` 本文件确认为零命中），`self.actors` 因此恒是空的
// `Arena::default()`，上述循环体一次也不会执行，三个新字段自然不会
// 被喂进哈希器——这与前几次重冻的情形不同：前几次改动的是
// `WorldState` 顶层字段（`kill_counts`/`history`/`next_world_id`），
// 顶层字段的混入代码在循环体之外、每次调用 `hash()` 都会执行；这次
// 改动的是 `Agent` 的字段，只有真的存在至少一个 `Agent` 时才有机会
// 被读到。人工核验（真实执行）：本次改动落地后原样跑了这条测试，
// 摘要与改动前的常量逐位相同，未观察到任何差异，因此常量本身不需要
// 更新——这不是遗漏检查，是核实过的真实结论。

/// 测试用区块布局：边长 48（是噪声格点周期的整数倍、大于视口跨度，
/// 且刻意不是 2 的幂，避免大陆尺度噪声层退化成全图常数，见
/// [`EXPECTED_WORLD_DIGEST`] 文档「裁定 CS-6」一节），单个区块——整个
/// 测试世界落在这一个区块内，满足 [`WorldState::new`] 的全部构造
/// 前置条件。
fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
}

#[test]
fn 固定种子的四十八乘四十八世界摘要跨平台稳定() {
    // Arrange：种子 5 在 48×48 世界上产出全部八种地形（含水域与阻挡
    // 视线的山地），见 EXPECTED_WORLD_DIGEST 文档「裁定 CS-6」一节。
    let params = GenParams {
        seed: 5,
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
