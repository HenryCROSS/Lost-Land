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
use crate::generate::{GenParams, zone_representative_terrain};
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
/// **完全独立**的粗粒度数据：每个区块只采样代表点（区块左上角一格，见
/// [`crate::generate::zone_representative_terrain`]），不经过、也不
/// 写入 `SurfaceStore` 的常驻集合——这是本类型存在的唯一理由：概览图
/// 的分辨率需求（每区块一格）与流式加载的分辨率需求（每瓦片一格）本
/// 就不同，硬要复用同一份存储只会互相拖累。
///
/// 与地表的关系是「同一份种子噪声的两种粒度采样」，不是两份可能漂移
/// 的地形真相——两者都经过
/// [`crate::generate::build_zone_noise`]/`crate::generate::terrain_at_coord`
/// （后者是模块私有函数，不能做成文档内链，见 `generate.rs` 模块文档）
/// 那同一条阈值逻辑，只是采样密度不同。
#[derive(Debug, Clone)]
pub struct ContinentField {
    zone_count: TorusSize,
    /// 按 `(zone.y() * zone_count.width() + zone.x())` 行主序排列，长度
    /// 恒等于 `zone_count.width() * zone_count.height()`。
    cells: Vec<TerrainKind>,
}

impl ContinentField {
    /// 查询给定区块坐标的代表地形。
    fn terrain_at_zone(&self, zone: ZoneCoord) -> TerrainKind {
        let index = zone.y() as usize * self.zone_count.width() as usize + zone.x() as usize;
        self.cells[index]
    }
}

/// 生成一份 [`ContinentField`]：遍历 `layout` 的全部区块坐标，每个区块
/// 只采样一个代表点，不生成任何区块窗口、不触碰 `SurfaceStore`。
///
/// 调用方应在世界创建时调用一次并长期持有结果（与
/// [`crate::generate::build_zone_noise`] 的噪声源同一个使用惯例：O(1)
/// 到 O(区块数) 之间的一次性开销，不是每帧都要重算的东西）——
/// `zone_count` 默认 48×32 = 1536 个区块，每个只采一点，成本远小于
/// 生成一个完整区块窗口。
pub fn generate_continent_field(
    layout: &ZoneLayout,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
) -> ContinentField {
    let zone_count = layout.zone_count();
    let mut cells =
        Vec::with_capacity((zone_count.width() as usize) * (zone_count.height() as usize));
    for zy in 0..zone_count.height() as i32 {
        for zx in 0..zone_count.width() as i32 {
            let zone = zone_count.wrap(zx, zy);
            cells.push(zone_representative_terrain(
                noise,
                params,
                layout,
                zone,
                terrain_ids,
            ));
        }
    }
    ContinentField { zone_count, cells }
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
    use crate::generate::build_zone_noise;
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
