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

use ll_core::torus::TorusPos;

use crate::state::WorldState;
use crate::terrain::TerrainKind;

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
pub fn minimap(world: &WorldState, center: TorusPos, span: u32) -> Vec<OverviewCell> {
    let half = (span / 2) as i32;
    let mut cells = Vec::with_capacity((span * span) as usize);

    for dy in 0..span as i32 {
        for dx in 0..span as i32 {
            let pos = world
                .size
                .wrap(center.x() + dx - half, center.y() + dy - half);
            cells.push(OverviewCell {
                terrain: world.terrain.terrain_at(pos),
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
pub fn continent_map(world: &WorldState, downsample: u32) -> Vec<OverviewCell> {
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
                terrain: world.terrain.terrain_at(pos),
                explored: true,
            });
        }
    }

    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::GenParams;

    /// 测试世界尺寸：64 是噪声格点周期的整数倍，且大于视口跨度，满足
    /// `WorldState::new` 的全部构造前置条件。
    fn test_world() -> WorldState {
        let size =
            ll_core::torus::TorusSize::new(64, 64).expect("64x64 满足整除与视口跨度两条约束");
        WorldState::new(size, &GenParams::default()).expect("测试尺寸满足全部构造前置条件")
    }

    #[test]
    fn 小地图格子数等于跨度的平方() {
        // Arrange
        let world = test_world();
        let center = world.size.wrap(32, 32);
        let span = 9;

        // Act
        let cells = minimap(&world, center, span);

        // Assert
        assert_eq!(cells.len(), (span * span) as usize);
    }

    #[test]
    fn 小地图跨度为零时返回空列表() {
        // Arrange
        let world = test_world();
        let center = world.size.wrap(32, 32);

        // Act
        let cells = minimap(&world, center, 0);

        // Assert
        assert!(cells.is_empty());
    }

    #[test]
    fn 小地图中心格的地形与直接查询世界一致() {
        // 小地图不该重算或篡改地形，只是把世界现有数据搬到另一种排列。
        // Arrange
        let world = test_world();
        let center = world.size.wrap(10, 20);
        let span = 5;
        let half = (span / 2) as usize;

        // Act
        let cells = minimap(&world, center, span);
        let center_cell = cells[half * span as usize + half];

        // Assert
        assert_eq!(center_cell.terrain, world.terrain.terrain_at(center));
    }

    #[test]
    fn 小地图跨越世界边缘时正确环绕() {
        // 原点附近的小地图会向西/向北探出世界边界，必须走 wrap 绕回
        // 对侧，而不是产出越界坐标或手写的错误换算。
        // Arrange
        let world = test_world();
        let origin = world.size.wrap(0, 0);
        let span = 5;

        // Act
        let cells = minimap(&world, origin, span);
        // span=5 时 half=2，(0,0) 左上角第一格对应 (-2,-2)，
        // 环绕后应等于世界最后两行两列的那一格。
        let wrapped_expected = world.terrain.terrain_at(world.size.wrap(-2, -2));

        // Assert
        assert_eq!(cells[0].terrain, wrapped_expected);
    }

    #[test]
    fn 大陆地图格子数按下采样倍率整除向上取整() {
        // Arrange
        let world = test_world();
        let downsample = 16;

        // Act
        let cells = continent_map(&world, downsample);

        // Assert
        let expected =
            world.size.width().div_ceil(downsample) * world.size.height().div_ceil(downsample);
        assert_eq!(cells.len() as u32, expected);
    }

    #[test]
    fn 大陆地图下采样倍率为零时退化为原始尺寸() {
        // Arrange
        let world = test_world();

        // Act
        let cells = continent_map(&world, 0);

        // Assert
        assert_eq!(cells.len() as u32, world.size.width() * world.size.height());
    }

    #[test]
    fn 大陆地图每格取块内左上角地形而非平均() {
        // Arrange
        let world = test_world();
        let downsample = 4;

        // Act
        let cells = continent_map(&world, downsample);
        let first_cell = cells[0];

        // Assert
        assert_eq!(
            first_cell.terrain,
            world.terrain.terrain_at(world.size.wrap(0, 0))
        );
    }

    #[test]
    fn 小地图格子恒标记为已探索() {
        // 现阶段 WorldState 不持有玩家探索记忆，见本模块文档顶部说明。
        // Arrange
        let world = test_world();
        let center = world.size.wrap(0, 0);

        // Act
        let cells = minimap(&world, center, 3);

        // Assert
        assert!(cells.iter().all(|cell| cell.explored));
    }
}
