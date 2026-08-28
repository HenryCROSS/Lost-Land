//! 大陆地图与小地图的只读数据视图。
//!
//! # 为什么是只读视图，不持有状态、不缓存
//!
//! [`minimap`] 与 [`continent_map`] 每次调用都直接读 [`WorldState`] 现算
//! 现出，不保留任何跨调用的内部状态。缓存会与世界状态失同步——地形被
//! 修改后缓存没跟着刷新，玩家看到的表现是「地图上有座山，走过去却
//! 没有」。这类描述极难定位到具体缺陷（玩家说不清是地图错了还是脚下
//! 的地形错了），比起损失一点重复计算的性能，宁可每次都现算。
//!
//! # `explored` 现在接的是真实的探索记忆（落地探索记忆批次）
//!
//! `OverviewCell::explored` 曾经恒为真：`WorldState` 当时不持有任何
//! 按玩家区分的探索记忆，`continent_map` 甚至不接受任何「这是谁的
//! 视角」参数，没有数据来源可读。现在两者都要求调用方显式传入一份
//! `&ExplorationMemory`（[`crate::exploration`] 模块），据此判定每一格
//! 或每一区块是否已探索——这与 `TerrainKind` 硬编码属性表在 P4 落地
//! mod 注册表前先占住接口形状、再接入真实数据的处理方式一致，见
//! `crate::exploration` 模块文档「为什么读取接口要求显式传入」一节。

use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};

use crate::exploration::ExplorationMemory;
use crate::generate::{GenParams, terrain_at_tile};
use crate::noise::TileableNoise;
use crate::space::ZoneCoord;
use crate::state::WorldState;
use crate::terrain::{BaseTerrainIds, TerrainKind};
use crate::zone::ZoneLayout;

/// 地图视图里的一格：地形种类加是否已被探索。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverviewCell {
    /// 该格的地形种类。
    pub terrain: TerrainKind,
    /// 该格是否已被探索。现阶段恒为真，见本模块文档。
    pub explored: bool,
}

/// 取以 `center` 为中心、边长 `span` 格的小地图切片，按行主序排列。
///
/// `span` 为偶数时中心格落在正中偏左上（`center` 对应 `span / 2` 处，
/// 整数除法向下取整），这与瓦片网格「一格恰好对应一个整数坐标」的惯例
/// 一致，不引入取整之外的额外规则。`span` 为零时返回空列表。
///
/// 只走 [`ll_core::torus::TorusSize::wrap`] 换算越界坐标：环面世界四面
/// 全连通，手写坐标换算会在世界边缘产出「小地图上很近，实际却隔着半个
/// 世界」这类缺陷，详见 `ll_core::torus` 的模块文档。
///
/// # 为什么需要 `&mut WorldState`（两级坐标系重写，任务 11）
///
/// `terrain` 换成 [`crate::surface_store::SurfaceStore`] 之后，任意
/// 坐标的地形查询都可能命中尚未常驻的区块。`minimap` 不是 `resolve`
/// （C1 只约束 `resolve` 必须是纯函数），允许在需要时触发按需生成——
/// 保持「按半径给出一份稠密的 `span × span` 栅格」这个既有约定不变
/// （调用方按下标定位某个偏移量的惯例不该因为迁移悄悄变成「可能缺格」
/// 的稀疏列表），比起改成只读、跳过未常驻的格子，代价更小、行为更
/// 好预测。
/// `exploration`：调用方是谁的视角——见模块文档「`explored` 现在接的是
/// 真实的探索记忆」与 `crate::exploration` 模块文档「为什么读取接口
/// 要求显式传入」一节。本函数只读取，不写入：标记探索是视野/FOV 结算
/// 的职责，不在这里发生。
#[allow(clippy::too_many_arguments)]
pub fn minimap(
    world: &mut WorldState,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
    exploration: &ExplorationMemory,
    center: TorusPos,
    span: u32,
    at_tick: Tick,
) -> Vec<OverviewCell> {
    let half = (span / 2) as i32;
    let mut cells = Vec::with_capacity((span * span) as usize);
    let layout = *world.terrain.layout();

    for dy in 0..span as i32 {
        for dx in 0..span as i32 {
            let pos = world
                .size
                .wrap(center.x() + dx - half, center.y() + dy - half);
            cells.push(OverviewCell {
                terrain: world.terrain_at_streaming(noise, params, terrain_ids, pos, at_tick),
                explored: exploration.is_explored(&layout, pos),
            });
        }
    }

    cells
}

/// 世界创建时一次性生成的粗粒度地形场，按**区块**分辨率（不是逐瓦片），
/// 专供 [`continent_map`] 使用（任务 13：`continent_map` 新数据源）。
///
/// # 为什么需要独立于 `SurfaceStore` 的一份数据
///
/// `continent_map`（大陆地图概览）曾经直接遍历世界瓦片坐标、按需触发
/// `SurfaceStore` 生成——这正是流式加载要避免的事（设计文档五节：
/// 「不能为了画一张概览图就把全部区块的完整地形都生成出来」，任务 11
/// 迁移时如实记录了这处临时状态，见其文档）。`ContinentField` 是一份
/// **完全独立**的粗粒度数据：只按固定间隔采样点，不经过、也不写入
/// `SurfaceStore` 的常驻集合——这是本类型存在的唯一理由：概览图的
/// 分辨率需求与流式加载的分辨率需求（每瓦片一格）本就不同，硬要复用
/// 同一份存储只会互相拖累。
///
/// 与地表的关系是「同一份种子噪声的两种粒度采样」，不是两份可能漂移
/// 的地形真相——两者都经过
/// [`crate::generate::build_zone_noise`]/`crate::generate::terrain_at_coord`
/// （后者是模块私有函数，不能做成文档内链，见 `generate.rs` 模块文档）
/// 那同一条阈值逻辑，只是采样密度不同。
///
/// # 分辨率：每个区块每轴 [`SAMPLES_PER_ZONE_AXIS`] 个采样点（世界地图
/// 缩放批次加密）
///
/// 本类型**曾经**每个区块只存一个采样点。那个分辨率下，世界地图无论
/// 怎么缩放，一格最细也只能到「一个区块 = 48×48 瓦片」——所有者要的
/// 「放大之后看清楚是什么东西」在这份数据上根本变不出来，因为更细的
/// 信息压根没被采过。加密到子区块分辨率是「细节」这件事唯一的来源。
///
/// 加密**不改变** [`continent_map`] 的产出：区块左上角那一个子采样点
/// 对应的瓦片坐标恰好是 `(zone.x() * zone_span, zone.y() * zone_span)`
/// ——正是 [`crate::generate::zone_representative_terrain`] 采的那一点，
/// 见 [`Self::terrain_at_zone`]。这条由测试
/// `加密后的大陆场每区块代表地形与加密前逐格相同` 锁住。
#[derive(Debug, Clone)]
pub struct ContinentField {
    zone_count: TorusSize,
    /// 每个区块每轴存了多少个采样点，见 [`SAMPLES_PER_ZONE_AXIS`]。
    samples_per_zone_axis: u32,
    /// 一个采样点覆盖多少个瓦片（`zone_span / samples_per_zone_axis`）。
    sample_span: u32,
    /// 采样场的尺寸（单位是采样点，不是瓦片也不是区块）。
    ///
    /// 做成 [`TorusSize`] 而不是一对 `u32`：采样场与世界本身一样是环面
    /// 的，视野平移要绕接缝，而 `TorusSize` 是本仓库唯一被允许做环面
    /// 换算的地方（不允许在别处手写取模，见 `ll_core::torus` 模块文档
    /// 与 `docs/architecture/04-torus-topology.md`）。
    sample_size: TorusSize,
    /// 按 `(sample.y() * sample_size.width() + sample.x())` 行主序排列，
    /// 长度恒等于 `sample_size.width() * sample_size.height()`。
    cells: Vec<TerrainKind>,
}

/// 每个区块每轴采样多少个点，见 [`ContinentField`] 文档。
///
/// 取 4（默认区块边长 48 → 一个采样点覆盖 12×12 瓦片）：这是「放大到
/// 最近一档时一格代表多大一片地」的下限。再密一倍，默认世界的采样点数
/// 从 49152 涨到 196608（建局时的一次性噪声采样成本与常驻内存同步翻
/// 四倍），换来的是 6 格见方的分辨率——对「在地图上挑一个区块出生」
/// 这个用途已经远超必要（一个区块 48 格见方，12 格粒度下每个区块就有
/// 4×4 个格子可看）。
///
/// `ZoneLayout::new` 保证 `zone_span` 是 `CELL_SIZE`（16）的整数倍
/// （`crate::zone::ZoneLayout::new` 的对齐校验），因此恒能被 4 整除；
/// [`generate_continent_field`] 仍然显式处理除不尽的情形（退化成每区块
/// 一个采样点，即加密前的行为），不 panic。
pub const SAMPLES_PER_ZONE_AXIS: u32 = 4;

impl ContinentField {
    /// 查询给定区块坐标的代表地形——取该区块**左上角**那一个子采样点。
    ///
    /// 与加密前逐位相同：子采样 `(zx * spa, zy * spa)` 对应的瓦片坐标是
    /// `(zx * zone_span, zy * zone_span)`，正是
    /// [`crate::generate::zone_representative_terrain`] 采的那一点。
    fn terrain_at_zone(&self, zone: ZoneCoord) -> TerrainKind {
        let sample = self.sample_size.wrap(
            zone.x() * self.samples_per_zone_axis as i32,
            zone.y() * self.samples_per_zone_axis as i32,
        );
        self.terrain_at_sample(sample)
    }

    /// 采样场的尺寸（单位是采样点）。
    pub fn sample_size(&self) -> TorusSize {
        self.sample_size
    }

    /// 一个采样点覆盖多少个瓦片。
    pub fn sample_span(&self) -> u32 {
        self.sample_span
    }

    /// 每个区块每轴有多少个采样点。
    pub fn samples_per_zone_axis(&self) -> u32 {
        self.samples_per_zone_axis
    }

    /// 世界有多少个区块。
    pub fn zone_count(&self) -> TorusSize {
        self.zone_count
    }

    /// 查询给定采样点坐标的地形。
    ///
    /// `sample` 必须是**已经过 [`Self::sample_size`] 环绕**的坐标——
    /// 与 [`crate::generate::terrain_at_tile`] 同一条既有约定：环绕由
    /// `TorusSize::wrap` 统一负责，本函数不再重复一遍环面语义。
    pub fn terrain_at_sample(&self, sample: TorusPos) -> TerrainKind {
        let index = sample.y() as usize * self.sample_size.width() as usize + sample.x() as usize;
        self.cells[index]
    }
}

/// 生成一份 [`ContinentField`]：按 [`SAMPLES_PER_ZONE_AXIS`] 的密度遍历
/// 整个世界的采样点，每个点只问一次噪声，不生成任何区块窗口、不触碰
/// `SurfaceStore`。
///
/// 调用方应在世界创建时调用一次并长期持有结果（与
/// [`crate::generate::build_zone_noise`] 的噪声源同一个使用惯例：一次性
/// 开销，不是每帧都要重算的东西）——默认布局 64×48 个区块、每区块
/// 4×4 个采样点 = 49152 次噪声采样，仍然远小于生成哪怕一小把完整区块
/// 窗口（一个 48×48 的窗口就是 2304 次，且还要建整份网格）。
pub fn generate_continent_field(
    layout: &ZoneLayout,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
) -> ContinentField {
    let zone_count = layout.zone_count();
    // 除不尽（当前 `ZoneLayout` 的对齐约束下不可能，见
    // `SAMPLES_PER_ZONE_AXIS` 文档）或采样场尺寸构造不出来时，退化成
    // 每区块一个采样点——那正是加密前的行为，是一条已知能工作的路径，
    // 比 panic 或产出半份数据都好。
    let samples_per_zone_axis = if layout.zone_span().is_multiple_of(SAMPLES_PER_ZONE_AXIS) {
        SAMPLES_PER_ZONE_AXIS
    } else {
        1
    };
    let (samples_per_zone_axis, sample_size) = TorusSize::new(
        zone_count.width() * samples_per_zone_axis,
        zone_count.height() * samples_per_zone_axis,
    )
    .map(|size| (samples_per_zone_axis, size))
    .unwrap_or((1, zone_count));

    let sample_span = layout.zone_span() / samples_per_zone_axis;
    let tile_size = layout.tile_size();
    let mut cells =
        Vec::with_capacity((sample_size.width() as usize) * (sample_size.height() as usize));
    for sy in 0..sample_size.height() as i32 {
        for sx in 0..sample_size.width() as i32 {
            let tile = tile_size.wrap(sx * sample_span as i32, sy * sample_span as i32);
            cells.push(terrain_at_tile(noise, params, tile, terrain_ids));
        }
    }
    ContinentField {
        zone_count,
        samples_per_zone_axis,
        sample_span,
        sample_size,
        cells,
    }
}

/// 取整份 [`ContinentField`] 的下采样概览，按行主序排列。
///
/// `downsample` 在**区块**这一级生效（`field` 本身已经是区块分辨率）：
/// 每 `downsample × downsample` 个区块取左上角那一个作为代表，理由与
/// 迁移前「每格取块内左上角地形而非平均」一致（地形是离散分类值，
/// 平均没有意义）。`downsample` 为零时夹到最小值 1，效果等价于不做
/// 下采样，与其在这里撞见除零 panic，不如当成「不缩小」处理。
///
/// # 不接触 `WorldState`/`SurfaceStore`
///
/// 签名不再需要 `&mut WorldState`——`field` 已经是生成好的静态数据，
/// 这正是 [`ContinentField`] 存在的意义：`continent_map` 因此**不可能**
/// 触发任何区块的按需生成，这条约束由函数签名本身保证，不需要靠调用
/// 纪律维持（类型系统能保证的地方，不留给运行时约定）。
///
/// # `exploration`：区块粒度，不是瓦片粒度
///
/// 本函数的分辨率是「每区块一格」（见 [`ContinentField`] 文档），因此
/// 每格的 `explored` 取的是
/// [`ExplorationMemory::zone_has_any_explored`]（该区块内是否至少有
/// 一格被探索过），不是 [`ExplorationMemory::is_explored`] 那种瓦片级
/// 精确查询——与 [`minimap`] 用同一份 `ExplorationMemory` 类型、不同
/// 粒度的查询方法，理由同 [`ContinentField`] 与 `SurfaceStore` 分辨率
/// 不同的既有设计（模块文档）。
pub fn continent_map(
    field: &ContinentField,
    layout: &ZoneLayout,
    exploration: &ExplorationMemory,
    downsample: u32,
) -> Vec<OverviewCell> {
    let downsample = downsample.max(1);
    let zone_count = layout.zone_count();
    let cols = zone_count.width().div_ceil(downsample);
    let rows = zone_count.height().div_ceil(downsample);

    let mut cells = Vec::with_capacity((cols * rows) as usize);
    for row in 0..rows {
        for col in 0..cols {
            let zx = (col * downsample) as i32;
            let zy = (row * downsample) as i32;
            let zone = zone_count.wrap(zx, zy);
            cells.push(OverviewCell {
                terrain: field.terrain_at_zone(zone),
                explored: exploration.zone_has_any_explored(zone),
            });
        }
    }

    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{build_zone_noise, zone_representative_terrain};
    use crate::terrain::base_terrain_fixture;
    use crate::zone::ZoneLayout;
    use ll_core::torus::TorusSize;

    /// 测试用区块布局：边长 64，单个区块——整个测试世界落在一个区块
    /// 内，`WorldState::new` 预热出生点邻域时就会把它整个装进常驻
    /// 集合，`minimap`/`continent_map` 的按需生成调用因此总是命中已
    /// 常驻的区块，不需要在每条测试里操心流式加载本身（那部分是
    /// `surface_store.rs` 测试的关注点）。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
    }

    fn test_world() -> WorldState {
        let layout = test_layout();
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    #[test]
    fn 小地图格子数等于跨度的平方() {
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");
        let exploration = ExplorationMemory::new();
        let center = world.size.wrap(32, 32);
        let span = 9;

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            &exploration,
            center,
            span,
            Tick(0),
        );

        // Assert
        assert_eq!(cells.len(), (span * span) as usize);
    }

    #[test]
    fn 小地图跨度为零时返回空列表() {
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");
        let exploration = ExplorationMemory::new();
        let center = world.size.wrap(32, 32);

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            &exploration,
            center,
            0,
            Tick(0),
        );

        // Assert
        assert!(cells.is_empty());
    }

    #[test]
    fn 小地图中心格的地形与直接查询世界一致() {
        // 小地图不该重算或篡改地形，只是把世界现有数据搬到另一种排列。
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");
        let exploration = ExplorationMemory::new();
        let center = world.size.wrap(10, 20);
        let span = 5;
        let half = (span / 2) as usize;

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            &exploration,
            center,
            span,
            Tick(0),
        );
        let center_cell = cells[half * span as usize + half];

        // Assert
        assert_eq!(
            center_cell.terrain,
            world
                .terrain_at(center)
                .expect("测试世界只有一个区块，预热后必然常驻")
        );
    }

    #[test]
    fn 小地图跨越世界边缘时正确环绕() {
        // 原点附近的小地图会向西/向北探出世界边界，必须走 wrap 绕回
        // 对侧，而不是产出越界坐标或手写的错误换算。
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");
        let exploration = ExplorationMemory::new();
        let origin = world.size.wrap(0, 0);
        let span = 5;

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            &exploration,
            origin,
            span,
            Tick(0),
        );
        // span=5 时 half=2，(0,0) 左上角第一格对应 (-2,-2)，
        // 环绕后应等于世界最后两行两列的那一格。
        let wrapped_expected = world
            .terrain_at(world.size.wrap(-2, -2))
            .expect("测试世界只有一个区块，预热后必然常驻");

        // Assert
        assert_eq!(cells[0].terrain, wrapped_expected);
    }

    /// 大陆地图测试用的多区块布局：4×4 个区块（边长同样是 64），比
    /// [`test_layout`] 的 1×1 有意义得多——下采样/代表点这类断言需要
    /// 真的有多个区块可比较，1×1 布局下这些断言会退化成平凡恒真。
    fn test_continent_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(4, 4).expect("4x4 是合法尺寸");
        ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
    }

    #[test]
    fn 大陆地图格子数按下采样倍率整除向上取整() {
        // Arrange
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
        let exploration = ExplorationMemory::new();
        let downsample = 3;

        // Act
        let cells = continent_map(&field, &layout, &exploration, downsample);

        // Assert：4x4 个区块，downsample=3 时每维向上取整为 2。
        let zone_count = layout.zone_count();
        let expected =
            zone_count.width().div_ceil(downsample) * zone_count.height().div_ceil(downsample);
        assert_eq!(cells.len() as u32, expected);
    }

    #[test]
    fn 大陆地图下采样倍率为零时退化为区块原始尺寸() {
        // Arrange
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
        let exploration = ExplorationMemory::new();

        // Act
        let cells = continent_map(&field, &layout, &exploration, 0);

        // Assert
        let zone_count = layout.zone_count();
        assert_eq!(cells.len() as u32, zone_count.width() * zone_count.height());
    }

    #[test]
    fn 大陆地图每格取区块左上角地形而非平均() {
        // Arrange
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
        let exploration = ExplorationMemory::new();
        let downsample = 2;

        // Act
        let cells = continent_map(&field, &layout, &exploration, downsample);
        let first_cell = cells[0];

        // Assert：第一格对应区块坐标 (0,0)，应等于该区块窗口左上角地形
        // ——generate.rs 自己已经有一条测试锁定
        // `zone_representative_terrain` 与 `generate_zone_window` 左上角
        // 一致（`区块代表地形与该区块窗口左上角地形一致`），这里只需要
        // 确认 `continent_map` 第一格确实读到的是同一个函数的产出，而
        // 不是重新验证 `zone_representative_terrain` 本身的正确性。
        let zone = layout.zone_count().wrap(0, 0);
        let expected = zone_representative_terrain(&noise, &params, &layout, zone, &terrain_ids);
        assert_eq!(first_cell.terrain, expected);
    }

    #[test]
    fn 加密后的大陆场每区块代表地形与加密前逐格相同() {
        // 本批把 `ContinentField` 从「每区块一个采样点」加密到「每区块
        // 4x4 个采样点」。加密**不许**改变 `continent_map` 的产出——
        // p2/p5 验收 demo 与视觉回归基准都吃这条产出。区块左上角那一个
        // 子采样点对应的瓦片坐标恰是 `zone_representative_terrain` 采的
        // 那一点，因此逐格相同；这条测试直接把两者对起来，防止将来有人
        // 改动采样起点（例如改成采区块中心）而悄悄挪动整张概览图。
        // Arrange
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
        // 全部标记为已探索无关紧要：本条只比 `terrain`，不比 `explored`。
        let exploration = ExplorationMemory::new();

        // Act
        let cells = continent_map(&field, &layout, &exploration, 1);

        // Assert：逐个区块比对。
        let zone_count = layout.zone_count();
        for zy in 0..zone_count.height() {
            for zx in 0..zone_count.width() {
                let zone = zone_count.wrap(zx as i32, zy as i32);
                let expected =
                    zone_representative_terrain(&noise, &params, &layout, zone, &terrain_ids);
                let actual = cells[(zy * zone_count.width() + zx) as usize].terrain;
                assert_eq!(actual, expected, "区块 ({zx}, {zy}) 的代表地形被加密改动了");
            }
        }
    }

    #[test]
    fn 加密后的大陆场按每区块四个采样点排布() {
        // 锁住「细节真的被采下来了」这条：采样场的尺寸必须是区块数的
        // SAMPLES_PER_ZONE_AXIS 倍，一个采样点覆盖的瓦片数必须是区块
        // 边长除以同一个倍数。若哪天有人把加密退回每区块一点，这条会
        // 立刻红——而只看 `continent_map` 的产出是看不出来的（上一条
        // 测试恰恰要求那份产出不变）。
        // Arrange
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");

        // Act
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);

        // Assert
        let zone_count = layout.zone_count();
        assert_eq!(field.samples_per_zone_axis(), SAMPLES_PER_ZONE_AXIS);
        assert_eq!(
            field.sample_size().width(),
            zone_count.width() * SAMPLES_PER_ZONE_AXIS
        );
        assert_eq!(
            field.sample_size().height(),
            zone_count.height() * SAMPLES_PER_ZONE_AXIS
        );
        assert_eq!(
            field.sample_span(),
            layout.zone_span() / SAMPLES_PER_ZONE_AXIS
        );
    }

    #[test]
    fn 加密后的采样点地形与直接问噪声一致() {
        // `terrain_at_sample` 不该重算或篡改地形，只是把同一条噪声阈值
        // 逻辑的产出按另一种排列存下来。
        // Arrange
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);

        // Act：挑一个不在任何区块左上角的采样点（第 (1,2) 个子采样）。
        let sample = field.sample_size().wrap(1, 2);
        let actual = field.terrain_at_sample(sample);

        // Assert
        let tile = layout
            .tile_size()
            .wrap(field.sample_span() as i32, 2 * field.sample_span() as i32);
        let expected = crate::generate::terrain_at_tile(&noise, &params, tile, &terrain_ids);
        assert_eq!(actual, expected);
    }

    #[test]
    fn continent_map不触发任何区块的按需生成() {
        // 这是本任务最重要的正确性验证：generate_continent_field/
        // continent_map 全程不接触 SurfaceStore，调用前后一个独立
        // SurfaceStore 的常驻集合必须逐位不变——直接证明「不能为了画
        // 一张概览图就把全部区块的完整地形都生成出来」这条约束成立。
        // Arrange
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");
        let store = crate::surface_store::SurfaceStore::new(layout, 256);
        let resident_before = store.resident_zones();
        let exploration = ExplorationMemory::new();

        // Act
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
        let _cells = continent_map(&field, &layout, &exploration, 1);

        // Assert
        assert_eq!(store.resident_zones(), resident_before);
        // 顺手确认前面两步真的产出了看起来合理的数据，而不是因为一次
        // 提前 return 而"碰巧"没碰 SurfaceStore。
        assert!(!store.is_resident(layout.zone_count().wrap(0, 0)));
    }

    #[test]
    fn 小地图未探索格子标记为未探索() {
        // 落地探索记忆批次：explored 不再恒为真，一份空探索记忆下
        // 全部格子都应标记为未探索。
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");
        let exploration = ExplorationMemory::new();
        let center = world.size.wrap(0, 0);

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            &exploration,
            center,
            3,
            Tick(0),
        );

        // Assert
        assert!(cells.iter().all(|cell| !cell.explored));
    }

    #[test]
    fn 小地图已标记探索的格子反映在explored里() {
        // Arrange
        let layout = test_layout();
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let center = world.size.wrap(0, 0);
        let mut exploration = ExplorationMemory::new();
        exploration.mark_explored(&layout, center);

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            &exploration,
            center,
            3,
            Tick(0),
        );
        let half = 1usize; // span=3 时中心格下标
        let center_cell = cells[half * 3 + half];

        // Assert
        assert!(center_cell.explored);
    }

    #[test]
    fn 大陆地图未探索区块标记为未探索() {
        // Arrange
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
        let exploration = ExplorationMemory::new();

        // Act
        let cells = continent_map(&field, &layout, &exploration, 1);

        // Assert
        assert!(cells.iter().all(|cell| !cell.explored));
    }

    #[test]
    fn 大陆地图已探索区块内任意一格都会让该区块标记为已探索() {
        // Arrange：只标记区块 (0,0) 内的一个瓦片，而不是整块。
        let layout = test_continent_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_continent_layout 满足全部约束");
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
        let mut exploration = ExplorationMemory::new();
        exploration.mark_explored(&layout, layout.tile_size().wrap(3, 3));

        // Act
        let cells = continent_map(&field, &layout, &exploration, 1);

        // Assert：区块坐标 (0,0) 对应 continent_map 输出的第一格
        // （downsample=1 时按行主序，与 zone_count 遍历顺序一致）。
        assert!(cells[0].explored);
    }
}
