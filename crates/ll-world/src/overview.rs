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
//! # 为什么 `explored` 现阶段恒为真
//!
//! `OverviewCell::explored` 是为「已探索/未探索」这类玩家视角的战争
//! 迷雾预留的字段，但 [`WorldState`] 本身不持有任何按玩家区分的探索
//! 记忆——那是更上层（角色/存档层面）的状态，而且 [`continent_map`]
//! 甚至不接受任何「这是谁的视角」参数，没有数据来源可读。因此本批次
//! 里两个函数产出的每一格都标记为已探索，等真正的玩家探索记忆落地后
//! 再接入真实数据。这与 `TerrainKind` 硬编码属性表在 P4 落地 mod 注册表
//! 前的处理方式一致：先占住接口形状，标注清楚缺的是什么，不是遗漏。
//! 迁移债务记在 `docs/superpowers/specs/2026-08-16-lostland-design.md`
//! §15 的 P5 行，而不是留在这里的代码注释——代码 TODO 会腐烂，规格里
//! 的记录会被已生效的「每阶段收尾反向核对规格」机制自动捕获。

use ll_core::time::Tick;
use ll_core::torus::TorusPos;

use crate::generate::GenParams;
use crate::noise::TileableNoise;
use crate::state::WorldState;
use crate::terrain::{BaseTerrainIds, TerrainKind};

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
#[allow(clippy::too_many_arguments)]
pub fn minimap(
    world: &mut WorldState,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
    center: TorusPos,
    span: u32,
    at_tick: Tick,
) -> Vec<OverviewCell> {
    let half = (span / 2) as i32;
    let mut cells = Vec::with_capacity((span * span) as usize);

    for dy in 0..span as i32 {
        for dx in 0..span as i32 {
            let pos = world
                .size
                .wrap(center.x() + dx - half, center.y() + dy - half);
            cells.push(OverviewCell {
                terrain: world.terrain_at_streaming(noise, params, terrain_ids, pos, at_tick),
                explored: true,
            });
        }
    }

    cells
}

/// 取整张大陆地图的下采样概览，按行主序排列。
///
/// 每 `downsample × downsample` 格取左上角那一格作为代表，而不是做任何
/// 形式的「平均」：地形种类是离散分类值（草地、山地、深水……），中间不
/// 存在有意义的插值，硬要平均只会产出一个不对应任何真实地形的编号。
/// `downsample` 为零时会在下面的除法里退化成非法输入，故夹到最小值 1，
/// 效果等价于不做下采样——与其让调用方在这里撞见除零 panic，不如把它
/// 当成「不缩小」处理。
///
/// # 迁移后的临时状态：仍然会触发生成（两级坐标系重写，任务 11）
///
/// 这个函数目前仍然按瓦片分辨率遍历整个世界并触发按需生成——这**不是**
/// 流式加载想要的最终效果（设计文档五节明确要求「不能为了画一张概览
/// 图就把全部区块的完整地形都生成出来」），而是任务 13（`continent_map`
/// 新数据源）的范围：那里会换成世界创建时一次性生成的粗粒度
/// `ContinentField`，按区块而非瓦片分辨率，不触发任何区块的按需生成。
/// 本次迁移（任务 11）的范围只到「换型之后继续编译、继续按原有断言
/// 通过」，不提前实现任务 13 的正确行为——见任务 11 迁移策略表
/// 「`continent_map` 测试留给任务 13」。
#[allow(clippy::too_many_arguments)]
pub fn continent_map(
    world: &mut WorldState,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
    downsample: u32,
    at_tick: Tick,
) -> Vec<OverviewCell> {
    let downsample = downsample.max(1);
    let width = world.size.width();
    let height = world.size.height();
    let cols = width.div_ceil(downsample);
    let rows = height.div_ceil(downsample);

    let mut cells = Vec::with_capacity((cols * rows) as usize);
    for row in 0..rows {
        for col in 0..cols {
            let x = (col * downsample) as i32;
            let y = (row * downsample) as i32;
            let pos = world.size.wrap(x, y);
            cells.push(OverviewCell {
                terrain: world.terrain_at_streaming(noise, params, terrain_ids, pos, at_tick),
                explored: true,
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
        let center = world.size.wrap(32, 32);
        let span = 9;

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
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
        let center = world.size.wrap(32, 32);

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
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
        let center = world.size.wrap(10, 20);
        let span = 5;
        let half = (span / 2) as usize;

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
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
        let origin = world.size.wrap(0, 0);
        let span = 5;

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
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

    #[test]
    fn 大陆地图格子数按下采样倍率整除向上取整() {
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");
        let downsample = 16;

        // Act
        let cells = continent_map(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            downsample,
            Tick(0),
        );

        // Assert
        let expected =
            world.size.width().div_ceil(downsample) * world.size.height().div_ceil(downsample);
        assert_eq!(cells.len() as u32, expected);
    }

    #[test]
    fn 大陆地图下采样倍率为零时退化为原始尺寸() {
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");

        // Act
        let cells = continent_map(&mut world, &noise, &params, &terrain_ids, 0, Tick(0));

        // Assert
        assert_eq!(cells.len() as u32, world.size.width() * world.size.height());
    }

    #[test]
    fn 大陆地图每格取块内左上角地形而非平均() {
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");
        let downsample = 4;

        // Act
        let cells = continent_map(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            downsample,
            Tick(0),
        );
        let first_cell = cells[0];

        // Assert
        assert_eq!(
            first_cell.terrain,
            world
                .terrain_at(world.size.wrap(0, 0))
                .expect("测试世界只有一个区块，预热后必然常驻")
        );
    }

    #[test]
    fn 小地图格子恒标记为已探索() {
        // 现阶段 WorldState 不持有玩家探索记忆，见本模块文档顶部说明。
        // Arrange
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&test_layout(), &params).expect("test_layout 满足全部约束");
        let center = world.size.wrap(0, 0);

        // Act
        let cells = minimap(
            &mut world,
            &noise,
            &params,
            &terrain_ids,
            center,
            3,
            Tick(0),
        );

        // Assert
        assert!(cells.iter().all(|cell| cell.explored));
    }
}
