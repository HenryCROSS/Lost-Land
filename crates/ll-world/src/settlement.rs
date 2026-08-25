//! 据点：历史生成留在世界当前形态上的那道痕迹。
//!
//! [`SettlementSite`] 是 [`crate::chronicle::WorldChronicle`] 跑完全部
//! 纪元之后的**最终快照**——一座还有人住的村子，或一片被遗弃的废墟。
//! [`stamp_settlement`] 把这份快照真的写进地形：这是「历史真的影响了
//! 世界现在长什么样」这条要求的落点，不是一份只能查看的事件日志。
//!
//! # 为什么据点不进存档
//!
//! ADR 0009「默认派生，只存偏差」。据点位置、规模、兴衰全部是
//! `f(world_seed, 地形)` 的纯函数（见 [`crate::chronicle`] 模块文档
//! 「确定性」一节），与地形本身同一条纪律：读档时按同一个种子重新
//! 派生即可，不需要序列化。**真正会偏离派生结果的东西**（玩家拆掉的
//! 一堵墙、NPC 的死亡与移动）本来就各自随 `SurfaceStore` 的常驻区块
//! 与 `WorldState::actors` 进存档。
//!
//! # 为什么不新建 `StructureKind`
//!
//! 墙、门、窗、地板**已经全部是地形**（`terrain.rs` 的
//! `BaseTerrainIds`）。地形层已经写好了按格存储、FOV 遮挡、寻路代价、
//! 存档 remap、内容哈希这五样；为「建筑」另起一个类型要把这五样各自
//! 重写一遍，换来零新增能力——ADR 0021 判据在这里给出的是「不建」。
//! 本模块因此只是一段**往 `ChunkGrid` 写地形**的纯函数。

use ll_core::ident::WorldId;
use ll_core::rng::DetRng;
use ll_core::torus::{TorusPos, TorusSize};

use crate::chunk::ChunkGrid;
use crate::space::ZoneCoord;
use crate::terrain::{BaseTerrainIds, TerrainTable};

/// 据点建筑铺设所用的随机流编号——与
/// [`crate::chronicle::CHRONICLE_STREAM_ID`]（历史推演）分开，两者
/// 互不干扰：改动建筑铺法不会连带改掉历史本身，反之亦然。
///
/// 形状照抄已落地的 `crate::weather::WEATHER_STREAM_ID`：一个固定的
/// 流编号 + 一个「第几号事物」的计数，喂给 `DetRng::for_entity`。
pub const SETTLEMENT_LAYOUT_STREAM_ID: u64 = 0x0053_5445_4144_0001;

/// 单栋建筑的外廓边长（格）：5×5 = 一圈 16 格墙 + 中间 3×3 地板。
///
/// 取 5 而不是 3：3×3 的「建筑」内部只有一格，进门就到底，看起来不像
/// 房子；5×5 是仍然只占一个区块窗口一小块、又能一眼认出是间屋子的
/// 最小尺寸。
const BUILDING_SPAN: i32 = 5;

/// 相邻两栋建筑锚点之间的间距（格）——比 [`BUILDING_SPAN`] 大 1，
/// 保证两栋屋子之间恒留出一格通道，不会连成一整块实心墙。
const BUILDING_SPACING: i32 = BUILDING_SPAN + 1;

/// 一座据点最多铺多少栋建筑——[`SettlementSite::building_count`] 的
/// 上界。
///
/// 取 24：按 [`BUILDING_SPACING`] 螺旋排布，24 栋恰好落在以锚点为中心
/// 约 5×5 栋的范围内（半径约 15 格），仍然稳稳装进一个 48×48 的区块
/// 窗口，不会溢出到邻区块——**建筑绝不跨区块写入**是本模块的硬约束，
/// 见 [`stamp_settlement`] 文档「为什么不跨区块」。
pub const MAX_BUILDINGS: u32 = 24;

/// 一座据点此刻的状态——历史推演跑完之后的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementStatus {
    /// 还有人住。建筑是木墙木地板，有门有窗。
    Inhabited,
    /// 曾经有人住，后来被遗弃。建筑只剩残破的石墙，没有门窗。
    Ruined,
}

/// 一座据点的最终快照。
///
/// 字段全部是历史推演的**结果**，不是输入：`founded_epoch` 与
/// `peak_population` 决定了这座村子现在有多大、废墟有多少堵墙还立着。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettlementSite {
    /// 永久标识，与历史事件、势力、家族共用 `WorldId` 空间。由
    /// [`crate::chronicle::WorldChronicle`] 的计数器分配。
    pub id: WorldId,
    /// 所在区块。一个区块至多一座据点（见 [`stamp_settlement`]）。
    pub zone: ZoneCoord,
    /// 锚点：据点正中那一格的世界瓦片坐标，恒落在 `zone` 内部。
    pub anchor: TorusPos,
    /// 现状。
    pub status: SettlementStatus,
    /// 建立于第几个纪元。
    pub founded_epoch: u32,
    /// 被遗弃于第几个纪元；仍有人住时为 `None`。
    pub abandoned_epoch: Option<u32>,
    /// 当前人口。[`SettlementStatus::Ruined`] 时恒为 0。
    pub population: u32,
    /// 历史峰值人口——废墟的规模由它决定（一座曾经的大城留下的废墟
    /// 比一个短命营地大）。
    pub peak_population: u32,
    /// 实际要铺的建筑栋数，已按 [`MAX_BUILDINGS`] 截断。
    pub building_count: u32,
}

/// 把一座据点铺进它所在区块的地形窗口。
///
/// # 前置条件：`grid` 必须是刚生成、未经改写的窗口
///
/// 本函数会读 `grid` 判断「这块地能不能盖房」（水面、山体、以及**已经
/// 铺好的前一栋房子**都会让当前这栋被跳过），因此它**不是幂等的**：
/// 对同一个窗口连铺两次，第二次会因为读到第一次写下的墙而跳过全部
/// 建筑。调用方（[`crate::surface_store::SurfaceStore`]）的契约是
/// 「区块每次生成之后紧接着铺一次」——重铺时先重新生成窗口，不在已
/// 铺过的窗口上再铺。这条契约写在 `SurfaceStore::admit` 与
/// `SurfaceStore::install_chronicle` 两处调用点上。
///
/// # 为什么不跨区块
///
/// 区块是流式加载的：铺设时邻区块可能根本不常驻，往它写入要么 panic
/// （`SurfaceStore::set_terrain` 的既有契约），要么偷偷触发一次生成
/// （该方法文档明确拒绝的隐式加载）。把一座据点整个约束在它自己的
/// 区块窗口内，是让「据点按需派生」与「区块按需加载」这两件事互不
/// 干涉的唯一省事办法——代价是据点规模有上界（[`MAX_BUILDINGS`]），
/// 而这个上界远大于最小可用形状需要的九间屋。
pub fn stamp_settlement(
    grid: &mut ChunkGrid,
    local_size: TorusSize,
    zone_origin: (i32, i32),
    site: &SettlementSite,
    ids: &BaseTerrainIds,
    table: &TerrainTable,
    world_seed: u64,
) {
    let anchor_x = site.anchor.x() - zone_origin.0;
    let anchor_y = site.anchor.y() - zone_origin.1;
    let span = local_size.width() as i32;

    for building in 0..site.building_count.min(MAX_BUILDINGS) {
        let (ox, oy) = spiral_offset(building);
        // 建筑外廓左上角（局部坐标）。锚点是这栋屋子的中心。
        let left = anchor_x + ox * BUILDING_SPACING - BUILDING_SPAN / 2;
        let top = anchor_y + oy * BUILDING_SPACING - BUILDING_SPAN / 2;
        if !fits_in_window(left, top, span) {
            continue;
        }
        if !plot_is_clear(grid, local_size, table, left, top) {
            continue;
        }

        let mut rng = DetRng::for_entity(
            world_seed,
            SETTLEMENT_LAYOUT_STREAM_ID,
            u64::from(site.id.get()) * u64::from(MAX_BUILDINGS) + u64::from(building),
        );
        match site.status {
            SettlementStatus::Inhabited => {
                raise_house(grid, local_size, ids, left, top, &mut rng);
            }
            SettlementStatus::Ruined => {
                raise_ruin(grid, local_size, ids, left, top, &mut rng);
            }
        }
    }
}

/// 一栋 5×5 的房子完整落在窗口内吗——不做环绕，见 [`stamp_settlement`]
/// 「为什么不跨区块」。
fn fits_in_window(left: i32, top: i32, span: i32) -> bool {
    left >= 0 && top >= 0 && left + BUILDING_SPAN <= span && top + BUILDING_SPAN <= span
}

/// 这块 5×5 的地能不能盖房：25 格全部可通行才算能。
///
/// 水面、山体因此被排除；**已经铺好的前一栋房子**也会让这块地不合格
/// （墙 `blocks_move`），这正是建筑之间不重叠的机制，不需要另记一份
/// 已占用格的集合。
fn plot_is_clear(
    grid: &ChunkGrid,
    local_size: TorusSize,
    table: &TerrainTable,
    left: i32,
    top: i32,
) -> bool {
    for dy in 0..BUILDING_SPAN {
        for dx in 0..BUILDING_SPAN {
            if grid
                .terrain_at(local_size.wrap(left + dx, top + dy))
                .blocks_move(table)
            {
                return false;
            }
        }
    }
    true
}

/// 铺一栋有人住的屋子：一圈木墙 + 中间木地板 + 一扇门 + 一扇窗。
fn raise_house(
    grid: &mut ChunkGrid,
    local_size: TorusSize,
    ids: &BaseTerrainIds,
    left: i32,
    top: i32,
    rng: &mut DetRng,
) {
    for dy in 0..BUILDING_SPAN {
        for dx in 0..BUILDING_SPAN {
            let on_edge = dx == 0 || dy == 0 || dx == BUILDING_SPAN - 1 || dy == BUILDING_SPAN - 1;
            let kind = if on_edge {
                ids.wall_wood
            } else {
                ids.floor_wood
            };
            grid.set_terrain(local_size.wrap(left + dx, top + dy), kind);
        }
    }

    // 门开在四条边中点之一，窗开在另外三个中点里的一个——两者互不
    // 重叠，一栋屋子恒有一个出入口。
    let door_side = rng.gen_range(4) as usize;
    let window_side = (door_side + 1 + rng.gen_range(3) as usize) % 4;
    let (dx, dy) = edge_midpoint(door_side);
    grid.set_terrain(local_size.wrap(left + dx, top + dy), ids.door_closed);
    let (wx, wy) = edge_midpoint(window_side);
    grid.set_terrain(local_size.wrap(left + wx, top + wy), ids.window);
}

/// 铺一处废墟：石墙，没有门窗，且每堵墙都有塌掉的可能——塌掉的那格
/// 变回草地。
///
/// 塌掉的概率不随机到「整栋都没了」：只掷外圈那 16 格，中间的地板
/// 原样保留（石地板是废墟仍然认得出是建筑的那部分）。
fn raise_ruin(
    grid: &mut ChunkGrid,
    local_size: TorusSize,
    ids: &BaseTerrainIds,
    left: i32,
    top: i32,
    rng: &mut DetRng,
) {
    for dy in 0..BUILDING_SPAN {
        for dx in 0..BUILDING_SPAN {
            let on_edge = dx == 0 || dy == 0 || dx == BUILDING_SPAN - 1 || dy == BUILDING_SPAN - 1;
            let kind = if !on_edge {
                ids.floor_stone
            } else if rng.chance(RUIN_COLLAPSE_NUMERATOR, RUIN_COLLAPSE_DENOMINATOR) {
                ids.grass
            } else {
                ids.wall_stone
            };
            grid.set_terrain(local_size.wrap(left + dx, top + dy), kind);
        }
    }
}

/// 废墟外圈每一格塌掉的概率分子（配 [`RUIN_COLLAPSE_DENOMINATOR`]）：
/// 十分之四。取这个量级是为了让废墟一眼看上去「破了但还立着」——
/// 塌太少与完好的房子分不开，塌太多就只剩零星石块、认不出是建筑。
const RUIN_COLLAPSE_NUMERATOR: u32 = 4;
/// 见 [`RUIN_COLLAPSE_NUMERATOR`]。
const RUIN_COLLAPSE_DENOMINATOR: u32 = 10;

/// 第 `side` 条边的中点在 5×5 外廓里的局部偏移：0 上、1 右、2 下、
/// 3 左。固定顺序，不依赖任何迭代顺序（约束 C5）。
fn edge_midpoint(side: usize) -> (i32, i32) {
    let mid = BUILDING_SPAN / 2;
    match side {
        0 => (mid, 0),
        1 => (BUILDING_SPAN - 1, mid),
        2 => (mid, BUILDING_SPAN - 1),
        _ => (0, mid),
    }
}

/// 第 `n` 栋建筑相对锚点的**格位**偏移（单位是「第几栋」，还要乘上
/// [`BUILDING_SPACING`] 才是格数）。
///
/// 按方环由内向外排：第 0 栋在锚点上，第 1..8 栋在半径 1 的一圈上，
/// 第 9..24 栋在半径 2 的一圈上……同一圈内按 `(dy, dx)` 光栅序。纯
/// 算术，无随机、无迭代顺序依赖——同一个 `n` 恒给同一个偏移。
fn spiral_offset(n: u32) -> (i32, i32) {
    if n == 0 {
        return (0, 0);
    }
    // 半径 r 的方环恰好容纳 (2r+1)^2 - (2r-1)^2 = 8r 个格位；
    // 累计到半径 r 为止共 (2r+1)^2 个。
    let mut ring = 1i32;
    while (2 * ring + 1) * (2 * ring + 1) <= n as i32 {
        ring += 1;
    }
    let inner = (2 * ring - 1) * (2 * ring - 1);
    let mut index = n as i32 - inner;
    for dy in -ring..=ring {
        for dx in -ring..=ring {
            if dx.abs() != ring && dy.abs() != ring {
                continue;
            }
            if index == 0 {
                return (dx, dy);
            }
            index -= 1;
        }
    }
    // 理论不可达：上面的循环恰好遍历 8*ring 个格位，而 index 在进入
    // 循环时严格小于 8*ring。走到这里说明环容量算错了，与其 panic，
    // 不如退回锚点——多铺一栋在锚点上会被 plot_is_clear 挡掉，不会
    // 破坏世界。
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::base_terrain_fixture;

    fn site(status: SettlementStatus, building_count: u32) -> SettlementSite {
        let mut counter = 0u32;
        SettlementSite {
            id: WorldId::next(&mut counter),
            zone: TorusSize::new(64, 48).expect("合法").wrap(0, 0),
            anchor: TorusSize::new(3072, 2304).expect("合法").wrap(24, 24),
            status,
            founded_epoch: 0,
            abandoned_epoch: None,
            population: 12,
            peak_population: 12,
            building_count,
        }
    }

    fn blank_window(ids: &BaseTerrainIds) -> (ChunkGrid, TorusSize) {
        let size = TorusSize::new(48, 48).expect("48x48 合法");
        let grid = ChunkGrid::new(size, ids.grass).expect("48 满足视口跨度");
        (grid, size)
    }

    #[test]
    fn 有人住的据点铺出的每栋屋子都恰有一扇门() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let (mut grid, size) = blank_window(&ids);
        let site = site(SettlementStatus::Inhabited, 9);

        // Act
        stamp_settlement(&mut grid, size, (0, 0), &site, &ids, &table, 7);

        // Assert
        let mut doors = 0;
        for y in 0..48 {
            for x in 0..48 {
                if grid.terrain_at(size.wrap(x, y)) == ids.door_closed {
                    doors += 1;
                }
            }
        }
        assert_eq!(doors, 9, "九栋屋子应该恰好九扇门");
    }

    #[test]
    fn 废墟不铺门也不铺窗() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let (mut grid, size) = blank_window(&ids);
        let site = site(SettlementStatus::Ruined, 6);

        // Act
        stamp_settlement(&mut grid, size, (0, 0), &site, &ids, &table, 7);

        // Assert
        let mut stone_walls = 0;
        for y in 0..48 {
            for x in 0..48 {
                let kind = grid.terrain_at(size.wrap(x, y));
                assert_ne!(kind, ids.door_closed);
                assert_ne!(kind, ids.window);
                if kind == ids.wall_stone {
                    stone_walls += 1;
                }
            }
        }
        assert!(stone_walls > 0, "废墟至少要留下几堵石墙");
    }

    #[test]
    fn 同一份输入铺两次逐格相同() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let site = site(SettlementStatus::Inhabited, 12);
        let (mut first, size) = blank_window(&ids);
        let (mut second, _) = blank_window(&ids);

        // Act
        stamp_settlement(&mut first, size, (0, 0), &site, &ids, &table, 99);
        stamp_settlement(&mut second, size, (0, 0), &site, &ids, &table, 99);

        // Assert
        for y in 0..48 {
            for x in 0..48 {
                assert_eq!(
                    first.terrain_at(size.wrap(x, y)),
                    second.terrain_at(size.wrap(x, y)),
                    "({x}, {y}) 两次铺设结果不同"
                );
            }
        }
    }

    #[test]
    fn 建筑不会铺到水面上() {
        // Arrange：把窗口右半边全改成深水。
        let (ids, table) = base_terrain_fixture();
        let (mut grid, size) = blank_window(&ids);
        for y in 0..48 {
            for x in 24..48 {
                grid.set_terrain(size.wrap(x, y), ids.deep_water);
            }
        }
        let site = site(SettlementStatus::Inhabited, MAX_BUILDINGS);

        // Act
        stamp_settlement(&mut grid, size, (0, 0), &site, &ids, &table, 3);

        // Assert
        for y in 0..48 {
            for x in 24..48 {
                assert_eq!(
                    grid.terrain_at(size.wrap(x, y)),
                    ids.deep_water,
                    "({x}, {y}) 本该仍是水"
                );
            }
        }
    }

    #[test]
    fn 螺旋偏移前二十五个互不重复() {
        // Arrange & Act
        let offsets: Vec<(i32, i32)> = (0..25).map(spiral_offset).collect();

        // Assert
        for (i, a) in offsets.iter().enumerate() {
            for b in offsets.iter().skip(i + 1) {
                assert_ne!(a, b, "螺旋偏移出现重复：{a:?}");
            }
        }
    }
}
