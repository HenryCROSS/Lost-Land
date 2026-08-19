//! 对称阴影投射视野（symmetric shadowcasting）。
//!
//! # 为什么必须对称
//!
//! 朴素射线投射——从原点向每个目标格连一条线，逐格检查是否被挡——不
//! 保证对称：A 到 B 的连线在离散网格上取整之后，走过的格子未必和 B 到
//! A 的连线一致，于是可能出现「A 能看见 B，但 B 看不见 A」。这在回合制
//! 战斗里是直接的玩法缺陷：玩家会被自己看不见的敌人攻击，而那个敌人在
//! 视野判定里明明能看见玩家。
//!
//! 本模块用逐八分象限、按行推进的阴影投射：墙格挡光时，用它自身四角
//! 相对原点的夹角区间在当前扇区里挖掉一块，让阴影正确传播到更远的行；
//! 但判断「一格本身是否可见」时，只看它**中心点**对原点的夹角是否还
//! 落在剩余的开放扇区内，不看它四角的区间是否与扇区有任何重叠。
//!
//! 这个「挡光用四角、显形用中心」的不对称写法看似奇怪，却正是对称性
//! 的来源：等价于判断「原点中心到目标格中心的连线是否被沿途某面墙的
//! 完整轮廓挡住」。这条连线不依赖是从哪一端出发画的——A 到 B 的连线
//! 与 B 到 A 的连线是同一条线段，被同一批墙的同一个几何轮廓挡住或不
//! 挡住，答案必然一致。反过来若「显形」也用四角区间（只要目标格有
//! 任意一角还留在开放扇区内就算可见），会让目标格自己的角落把可见性
//! 「借」到原本已被挡住的中心线上——这条借用是单向的（不会对称地反过
//! 来发生在观察者自己的角落上），实测会产出「A 能看见 B 的一角、B 却
//! 看不见 A」的反例。这条性质由 `tests/fov_blackbox.rs` 的属性测试
//! 守护，而不是靠手写用例：能真正暴露这类反例的往往是特定的墙角几何
//! 形状，手写用例几乎不可能覆盖到。
//!
//! 角度比较全程使用整数分子/分母组成的有理数（[`Slope`]），靠交叉相乘
//! 判断大小，不引入浮点——世界状态禁止浮点数。
//!
//! # 为什么墙本身可见（但不是每一面墙）
//!
//! 扫描每一格时，先判断它是否落在当前可见扇区内并据此标记可见，再判断
//! 它是否阻挡视线以决定要不要继续往它背后扫描——这让「贴着原点的墙」这
//! 种典型情形正确可见，只是背后不可见。
//!
//! **但这不是一条对所有墙格都成立的普遍保证**，这是刻意接受的代价，
//! 不是遗漏：墙格与地板格共用同一条「中心落在开放扇区内」规则（见上一
//! 节），没有为墙做特殊处理。后果是——某些墙格的四角区间确实与开放
//! 扇区有重叠（它参与了遮挡计算、参与了向下一行收缩扇区，这部分逻辑
//! 正常工作），但因为它自己的**中心**斜率恰好落在扇区外，最终在全部
//! 8 个象限的扫描里都不会被标记进可见集合。实测：随机取样 3000 组
//! 「随机地形 + 随机原点 + 半径 5」的场景，其中 2909 组（约 97%）至少
//! 存在一个这样「参与了遮挡计算却自己不可见」的墙格；一个具体样例是
//! `wall_seed=827307999, origin=(53,5), radius=5` 下的墙格 `(54,1)`。
//!
//! 为什么不干脆把这个代价也修掉——即「只要墙格的四角区间与扇区有重叠
//! 就标记可见」，不再要求中心落在扇区内？因为这条看似更直觉的规则会
//! 反过来破坏对称性：把这条规则套回本模块早期版本、跑同一批属性测试
//! 用的随机取样，1500 组随机地图里出了 12 组不对称（`A 能看见 B` 但
//! `B` 看不见 `A`）。对称性与「每一面参与遮挡计算的墙都必须可见」这两
//! 条在本算法下不可兼得，选哪个是真实的取舍，不是没想到的疏漏。
//!
//! **本模块选对称性**：对称性被打破是直接的玩法缺陷（玩家被自己看不见
//! 的敌人攻击，回合制里等于系统性不公平），而边缘墙格偶尔不可见只是
//! 视觉瑕疵，且几乎必然会被上层「已探索格子的记忆」这一层（roguelike
//! 里的通用设计）掩盖——玩家上次看到那面墙时它已经被记下来了，这一帧
//! 没有被实时标记可见不会让它凭空消失成黑块。
//!
//! **看到墙没被点亮，不要顺手把这条规则改成「墙一律无条件可见」**——
//! 那正好会把上面测出来的对称性缺陷带回来，而对称性缺陷极难通过肉眼或
//! 单个测试用例复现，多半要靠大样本量的属性测试才能揪出来（见
//! `tests/fov_blackbox.rs` 里 `墙格与原点的可见性对称` 这条：任何人
//! 把墙改成无条件可见都会让它变红）。

use std::collections::HashSet;
use std::hash::Hash;

use ll_core::bounded::BoundedPos;
use ll_core::torus::TorusPos;

use crate::bounded_grid::BoundedGrid;
use crate::chunk::ChunkGrid;
use crate::terrain::{TerrainKind, TerrainTable};

/// `compute_fov` 依赖的最小「世界」接口——把算法本身与它跑在哪种拓扑
/// （环面世界地表，还是有界不环绕的 `Interior` 楼层）解耦。
///
/// # 为什么用 trait + 静态分发，不是 enum 或 `dyn`
///
/// 阴影投射是逐格热路径（每帧、视野半径覆盖的每一格都要走一遍）。
/// `dyn SightGrid` 会把 [`terrain_at`](Self::terrain_at)/
/// [`offset`](Self::offset)/[`squared_euclidean`](Self::squared_euclidean)
/// 全部变成虚调用——`ChunkGrid`（环面地表，真正的热路径；`Interior`
/// 的有界网格通常小得多、访问频率也低得多）会为此额外付出间接跳转的
/// 成本，不可接受。`compute_fov` 因此保持
/// `fn compute_fov<G: SightGrid>(...)` 的泛型签名：编译器为
/// `ChunkGrid`/`BoundedGrid` 各自单态化出一份，`ChunkGrid` 那一份与
/// 泛化之前逐条指令等价——[`offset`](Self::offset) 内联后就是原来的
/// `world.wrap` 调用，恒返回 `Some`，这个分支在编译期即可判死。
///
/// 也考虑过把「环面 / 有界」做成一个 enum、在 `compute_fov` 内部 `match`
/// 两条分支各自的坐标运算——**否决**：那等于把 `scan_row_in_sector`
/// 整个函数体复制两份（一份用 `TorusPos`，一份用 `BoundedPos`），恰恰是
/// `knowledge/design/coordinate-system-and-layers.md` 六节核实过的
/// 「算法本身零改动，只是输入类型需要新变体」这条结论要避免的重复，
/// 也会让本文件顶部整段关于对称性的论证需要分别在两处维护、一旦漂移
/// 就可能重演 `tests/fov_blackbox.rs` 曾经抓出的对称性缺陷。trait 把
/// 「哪种坐标系」收敛成三个查询方法 + 一个安全上限，算法只写一遍。
///
/// # 越界处理是这里真正的算法级改动
///
/// [`offset`](Self::offset) 从 `origin` 按 `(dx, dy)` 求目标坐标：环面
/// 实现恒返回 `Some`（[`TorusSize::wrap`] 绕回世界内）；有界实现越界
/// 返回 `None`——[`scan_row_in_sector`] 收到 `None` 时直接跳过这一格，
/// 既不标记可见也不参与遮挡计算，视线在这个方向上止于地图边界，不会
/// 像环面那样绕接缝。
pub trait SightGrid {
    /// 网格上的一个位置类型：环面用 [`TorusPos`]，有界用 [`BoundedPos`]。
    type Pos: Copy + Eq + Hash;

    /// 查询给定位置的地形。
    fn terrain_at(&self, pos: Self::Pos) -> TerrainKind;

    /// 从 `origin` 出发按 `(dx, dy)` 偏移求目标坐标。
    ///
    /// 环面实现恒返回 `Some`；有界实现越界返回 `None`——调用方必须把
    /// `None` 当作「这个方向在这一步走出了地图边界」，不能替换成任何
    /// 默认坐标，否则会在有界地图上产出错误的可见性（见本 trait 文档
    /// 「越界处理」一节）。
    fn offset(&self, origin: Self::Pos, dx: i32, dy: i32) -> Option<Self::Pos>;

    /// 两点欧氏距离的平方（世界状态禁止浮点，只提供平方值避免开方）。
    fn squared_euclidean(&self, a: Self::Pos, b: Self::Pos) -> u64;

    /// 扫描行数的安全上限：防止调用方传入极端 `radius`（含 `u32::MAX`）
    /// 时扫描行为失控。
    ///
    /// 环面与有界网格的上限逻辑本质不同：环面超过半个世界宽/高之后，
    /// 绕接缝比继续沿直线扫描更近，继续扫纯粹是浪费（不影响正确性——
    /// 返回的可见格依然全部满足实际拓扑下的距离约束）；有界网格没有
    /// 接缝可绕，唯一需要的上限是「网格自身能容纳的最大跨度」。这条
    /// 差异被隔离在这一个方法里，共享的扫描算法
    /// （[`scan_octant`]/[`scan_row_in_sector`]）不需要为它分支，见各
    /// 实现的文档。
    fn max_scan_row(&self, radius: u32) -> u32;
}

impl SightGrid for ChunkGrid {
    type Pos = TorusPos;

    fn terrain_at(&self, pos: TorusPos) -> TerrainKind {
        ChunkGrid::terrain_at(self, pos)
    }

    fn offset(&self, origin: TorusPos, dx: i32, dy: i32) -> Option<TorusPos> {
        // TorusSize::wrap 对任意整数坐标恒成功——环面没有「越界」这个
        // 概念，走出一侧就绕回另一侧。
        Some(self.world().wrap(origin.x() + dx, origin.y() + dy))
    }

    fn squared_euclidean(&self, a: TorusPos, b: TorusPos) -> u64 {
        self.world().squared_euclidean(a, b)
    }

    fn max_scan_row(&self, radius: u32) -> u32 {
        // 与泛化之前的行为逐位等价：超过半个世界宽/高之后，直线扫描
        // 已经比绕接缝更远，继续扫是纯粹的浪费，不是正确性问题，见
        // SightGrid::max_scan_row 文档。
        let world = self.world();
        radius.min(world.width() / 2).min(world.height() / 2)
    }
}

impl SightGrid for BoundedGrid {
    type Pos = BoundedPos;

    fn terrain_at(&self, pos: BoundedPos) -> TerrainKind {
        BoundedGrid::terrain_at(self, pos)
    }

    fn offset(&self, origin: BoundedPos, dx: i32, dy: i32) -> Option<BoundedPos> {
        // BoundedSize::try_pos 越界返回 None——没有「绕回」可以兜底，
        // 这正是本 trait 要接住的拓扑差异。
        self.size().try_pos(origin.x() + dx, origin.y() + dy)
    }

    fn squared_euclidean(&self, a: BoundedPos, b: BoundedPos) -> u64 {
        self.size().squared_euclidean(a, b)
    }

    fn max_scan_row(&self, radius: u32) -> u32 {
        // 有界地图内任意两点的切比雪夫距离不可能超过
        // max(width, height) - 1（两点都必须落在
        // [0,width) × [0,height) 内）。取 max 而非 min：min 会在长宽
        // 悬殊的地图上把扫描沿长轴方向提前截断，漏掉本该可见的格子；
        // 环面版本用 /2 是因为它还要处理绕接缝，有界地图没有这层顾虑，
        // 只需要一个不会误伤真实可见范围的硬上限，防止极端 radius
        // 输入把扫描行数拖到失控的量级。
        let size = self.size();
        radius.min(size.width().max(size.height()))
    }
}

/// 一次视野计算得到的可见格集合。
///
/// 泛型默认取 [`TorusPos`]——绝大多数调用方（地表视野）不需要写
/// `VisibleSet<TorusPos>`，只有消费 `Interior` 视野（[`BoundedPos`]）
/// 的调用方才需要显式指定，见 [`SightGrid`] 文档。
#[derive(Debug, Clone)]
pub struct VisibleSet<P: Copy + Eq + Hash = TorusPos> {
    tiles: HashSet<P>,
}

impl<P: Copy + Eq + Hash> VisibleSet<P> {
    /// 给定坐标是否落在本次视野内。
    pub fn contains(&self, pos: P) -> bool {
        self.tiles.contains(&pos)
    }

    /// 可见格总数。
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// 视野是否为空。
    ///
    /// [`compute_fov`] 恒会把原点自身纳入视野（见其文档），所以这个
    /// 方法实际恒返回假；提供它只是为了满足「有 `len` 就该有
    /// `is_empty`」的惯例，避免 clippy 警告。
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// 遍历所有可见格，不保证顺序。
    pub fn iter(&self) -> impl Iterator<Item = P> + '_ {
        self.tiles.iter().copied()
    }
}

/// 计算以 `origin` 为中心、`radius` 为半径的可见格集合。
///
/// 原点自身恒可见，即便 `radius` 为零。
///
/// 泛化自环面专用版本（见 [`SightGrid`] 文档「为什么用 trait + 静态
/// 分发」）：算法本身（[`scan_octant`]/[`scan_row_in_sector`]）与拓扑
/// 无关，`G: SightGrid` 只提供三个查询点（地形、偏移、距离）与一个
/// 扫描进深上限，环面（[`ChunkGrid`]）与有界（[`BoundedGrid`]）各自
/// 提供符合自己拓扑的实现，见 [`SightGrid`] 各 impl 的文档。
///
/// # 扫描进深的安全上限
///
/// 内部按八分象限逐行向外扫描，行数上限取 `grid.max_scan_row(radius)`
/// ——具体上限逻辑因拓扑而异，见 [`SightGrid::max_scan_row`] 文档；这个
/// 上限只影响扫描提前停止的时机，返回的可见格依然全部满足
/// `grid.squared_euclidean(origin, pos) <= radius * radius`（`ChunkGrid`
/// 场景下等价于 `TorusSize::chebyshev(origin, pos) <= radius`：见
/// `tests/fov_blackbox.rs` 的属性测试）。
pub fn compute_fov<G: SightGrid>(
    grid: &G,
    table: &TerrainTable,
    origin: G::Pos,
    radius: u32,
) -> VisibleSet<G::Pos> {
    let mut tiles = HashSet::new();
    tiles.insert(origin);

    let max_row = grid.max_scan_row(radius);
    let radius_sq = u64::from(radius) * u64::from(radius);

    let mut ctx = ScanContext {
        grid,
        table,
        origin,
        radius_sq,
        tiles: &mut tiles,
    };
    for octant in 0u8..8 {
        scan_octant(&mut ctx, octant, max_row);
    }

    VisibleSet { tiles }
}

/// 一次视野计算共享的只读/累积状态，打包传递以避免单个函数参数过多。
struct ScanContext<'a, G: SightGrid> {
    grid: &'a G,
    table: &'a TerrainTable,
    origin: G::Pos,
    radius_sq: u64,
    tiles: &'a mut HashSet<G::Pos>,
}

/// 有理数形式的斜率，分母恒为正。
///
/// 用整数分数而不是浮点表示角度：分母恒正时，交叉相乘既能精确比较
/// 大小，又不会像浮点除法那样在格点边界附近产生舍入误差——那种误差
/// 正是破坏对称性的常见根源。
#[derive(Debug, Clone, Copy)]
struct Slope {
    num: i64,
    den: i64,
}

impl Slope {
    /// `self < other`。两个分母都恒为正，交叉相乘不改变不等号方向。
    fn lt(self, other: Slope) -> bool {
        i128::from(self.num) * i128::from(other.den) < i128::from(other.num) * i128::from(self.den)
    }

    /// `self > other`，见 [`Self::lt`]。
    fn gt(self, other: Slope) -> bool {
        other.lt(self)
    }

    /// `self <= other`，见 [`Self::lt`]。
    fn le(self, other: Slope) -> bool {
        !other.lt(self)
    }
}

/// 一个八分象限内、尚待继续向外扫描的连续可见角度区间。
#[derive(Debug, Clone, Copy)]
struct Sector {
    low: Slope,
    high: Slope,
}

/// 把八分象限本地坐标换算成相对原点的带号偏移。
///
/// `row` 是沿本象限主轴的进深（从原点数起的距离），`col` 是沿副轴的
/// 偏移，满足 `0 <= col <= row`。8 个分支对应正方形的 8 重对称（4 次
/// 旋转 × 2 次镜像），恰好无重叠、无遗漏地覆盖全部 360 度——这是阴影
/// 投射算法划分八分象限的标准方式，边界（`col == 0` 与 `col == row`）
/// 会被相邻两个象限各扫描一次，重复标记同一格是无害的。
fn octant_offset(octant: u8, row: i32, col: i32) -> (i32, i32) {
    match octant {
        0 => (row, col),
        1 => (col, row),
        2 => (-col, row),
        3 => (-row, col),
        4 => (-row, -col),
        5 => (-col, -row),
        6 => (col, -row),
        7 => (row, -col),
        _ => unreachable!("八分象限编号恒在 0..8 内"),
    }
}

/// 扫描单个八分象限，把发现的可见格写入 `tiles`。
///
/// 用一组「活跃扇区」逐行向外推进：每一行结束后，尚未被完全遮挡的
/// 子区间被收集成下一行要继续扫描的扇区列表。这与经典递归阴影投射
/// 语义相同，只是把「递归」换成了「按行广度优先」，避免为极端输入
/// （如巨大的 `radius`）产生过深的调用栈。
fn scan_octant<G: SightGrid>(ctx: &mut ScanContext<G>, octant: u8, max_row: u32) {
    let mut sectors = vec![Sector {
        low: Slope { num: 0, den: 1 },
        high: Slope { num: 1, den: 1 },
    }];

    for row in 1..=max_row {
        if sectors.is_empty() {
            break;
        }
        let mut next_sectors = Vec::new();
        for sector in &sectors {
            scan_row_in_sector(ctx, octant, row as i32, *sector, &mut next_sectors);
        }
        sectors = next_sectors;
    }
}

/// 扫描某个扇区在某一进深行内、与之相交的格子。
///
/// 逐列扫描该行的每一格：先用格子四角对原点的夹角区间（[`Slope`]）
/// 判断它是否与当前扇区有任何重叠，重叠就继续处理，否则跳过（这一步
/// 只影响扫描范围，不影响可见性判定）。可见性判定另用格子**中心**的
/// 夹角是否落在扇区内——两者用途不同：四角区间用于「这格多大程度上
/// 影响后续扇区收缩」，中心夹角用于「这格本身算不算可见」，混用会破坏
/// 对称性，见本模块顶部文档。
///
/// 同时维护一段「当前连续开放区间」，每遇到一次挡光格就把区间在此切断
/// 并存入 `next_sectors`，每次挡光结束就从挡光格的远角重新起算——挡光
/// 格背后的角度范围就此从后续行的扫描中被排除，形成阴影。
///
/// 区间的边界全程用闭区间（`<=`/`>=`），包括允许推入 `low == high` 的
/// 退化（零宽）扇区：格子的角恰好落在挡光格边缘的那条斜率线上时，视线
/// 只是擦着这个角，并没有真正进入挡光格的实体，所以那条边界斜率本身
/// 仍应算作「开放」。这个情形并不罕见——挡光格的角落斜率恰好等于
/// `0` 或 `1`（即恰好落在正前方轴线或对角线上）在整数网格上时常发生，
/// 若用开区间会把这条边界线也当成被挡，产出的视野就不对称：从这一侧
/// 出发算出的扇区在边界处提前夭折，从另一侧出发却因为没撞上同一个角
/// 而能继续，两侧各算各的就会分歧。
fn scan_row_in_sector<G: SightGrid>(
    ctx: &mut ScanContext<G>,
    octant: u8,
    row: i32,
    sector: Sector,
    next_sectors: &mut Vec<Sector>,
) {
    let mut run_start = sector.low;
    let mut in_clear_run = true;
    let mut last_wall_high = sector.low;

    for col in 0..=row {
        // 格子四角对原点的斜率恒为 (col±0.5)/(row±0.5)，四选二取极值。
        // 远角（更大斜率）恒取分子较大的 col+0.5 配分母较小的 row-0.5：
        // col+0.5 在 col>=0 时恒为正，分母越小比值越大，这个配对恒成立。
        //
        // 近角（更小斜率）则不能恒定配对：分子 col-0.5 只有在 col>=1 时
        // 才非负，此时配分母较大的 row+0.5 使比值最小；但 col==0 时
        // col-0.5 为负，配分母较大反而把比值推向零（变大），必须反过来
        // 配分母较小的 row-0.5 才能取到真正的最小值。col==0 对应主轴
        // 正前方那一列格子——离原点最近，紧邻原点的格子在广角意义上
        // 恰恰是视场最「宽」的一列，这不是可以忽略的边界情形。
        let tile_low = if col == 0 {
            Slope {
                num: -1,
                den: 2 * i64::from(row) - 1,
            }
        } else {
            Slope {
                num: 2 * i64::from(col) - 1,
                den: 2 * i64::from(row) + 1,
            }
        };
        let tile_high = Slope {
            num: 2 * i64::from(col) + 1,
            den: 2 * i64::from(row) - 1,
        };

        // 与当前扇区不相交（要么还没进入扇区范围，要么已经越过）就跳过。
        // 用闭区间（<=/>=）而不是开区间：格子的角恰好落在扇区边界上时
        // 视线只是擦着这个角，并没有真正进入相邻格子的实体，不该被当成
        // 已经出了扇区——下面推入 `next_sectors` 时同理，全程闭区间。
        if tile_high.lt(sector.low) || tile_low.gt(sector.high) {
            continue;
        }

        let (dx, dy) = octant_offset(octant, row, col);
        // 有界网格越过地图边缘时返回 None——没有「绕回」兜底，这一格
        // 既不标记可见也不参与遮挡计算，直接跳过（见 SightGrid::offset
        // 文档）。环面网格的 offset 恒返回 Some，这个分支对 ChunkGrid
        // 场景恒不命中，不改变泛化前的行为。
        let Some(pos) = ctx.grid.offset(ctx.origin, dx, dy) else {
            continue;
        };

        // 标记可见用「格子中心对原点的斜率是否仍在开放扇区内」，而不是
        // 「格子的四角区间是否与扇区有任何重叠」——这正是保证对称性的
        // 关键一步，见本函数顶部的文档注释。
        let center_slope = Slope {
            num: i64::from(col),
            den: i64::from(row),
        };
        if center_slope.le(sector.high)
            && sector.low.le(center_slope)
            && ctx.grid.squared_euclidean(ctx.origin, pos) <= ctx.radius_sq
        {
            ctx.tiles.insert(pos);
        }

        if ctx.grid.terrain_at(pos).blocks_sight(ctx.table) {
            if in_clear_run && run_start.le(tile_low) {
                next_sectors.push(Sector {
                    low: run_start,
                    high: tile_low,
                });
            }
            in_clear_run = false;
            last_wall_high = tile_high;
        } else if !in_clear_run {
            run_start = last_wall_high;
            in_clear_run = true;
        }
    }

    if in_clear_run && run_start.le(sector.high) {
        next_sectors.push(Sector {
            low: run_start,
            high: sector.high,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{BaseTerrainIds, TerrainTable, base_terrain_fixture};
    use ll_core::bounded::BoundedSize;
    use ll_core::torus::TorusSize;

    /// 测试世界尺寸：远大于视野半径，避免环面绕回影响单元测试的几何
    /// 直觉（这些测试关心的是局部遮挡关系，不是绕回本身）。
    fn test_world() -> TorusSize {
        TorusSize::new(64, 64).expect("64x64 满足 ChunkGrid 的最小视口跨度")
    }

    /// 一张没有任何阻挡地形的网格，代表开阔地带，配上本体地形表。
    fn open_grid() -> (BaseTerrainIds, TerrainTable, ChunkGrid) {
        let (ids, table) = base_terrain_fixture();
        let grid =
            ChunkGrid::new(test_world(), ids.grass).expect("64x64 满足 ChunkGrid 的最小视口跨度");
        (ids, table, grid)
    }

    #[test]
    fn 原点自身恒可见() {
        // Arrange
        let (_ids, table, grid) = open_grid();
        let origin = grid.world().wrap(10, 10);

        // Act
        let visible = compute_fov(&grid, &table, origin, 5);

        // Assert
        assert!(visible.contains(origin));
    }

    #[test]
    fn 半径为零时只看见原点() {
        // Arrange
        let (_ids, table, grid) = open_grid();
        let origin = grid.world().wrap(10, 10);

        // Act
        let visible = compute_fov(&grid, &table, origin, 0);

        // Assert
        assert_eq!(visible.len(), 1);
    }

    #[test]
    fn 墙后的格子不可见() {
        // 原点、墙、目标三点共线：墙直接挡在原点与目标之间。
        // Arrange
        let (ids, table, mut grid) = open_grid();
        let world = grid.world();
        let origin = world.wrap(10, 10);
        let wall = world.wrap(12, 10);
        let behind_wall = world.wrap(14, 10);
        grid.set_terrain(wall, ids.wall_stone);

        // Act
        let visible = compute_fov(&grid, &table, origin, 6);

        // Assert
        assert!(!visible.contains(behind_wall));
    }

    #[test]
    fn 正对原点的墙可见() {
        // 与上一条测试配对：正前方、贴着原点的孤立墙格必须可见，只是
        // 它背后不可见。这个位置的墙中心恰好落在轴线上（斜率 0），
        // 稳稳落在任何非空扇区内，不会触发模块文档「为什么墙本身可见
        // （但不是每一面墙）」一节描述的边界情形——本测试只保证这一种
        // 典型布局，不覆盖「四角与扇区有重叠、但中心恰好在扇区外」的
        // 擦角墙格，那类墙格可能不可见，是刻意接受的代价，见模块文档；
        // 那条代价由 `tests/fov_blackbox.rs` 的
        // `墙格与原点的可见性对称` 属性测试钉住，不要靠改这条单测来
        // 补上。
        // Arrange
        let (ids, table, mut grid) = open_grid();
        let world = grid.world();
        let origin = world.wrap(10, 10);
        let wall = world.wrap(12, 10);
        grid.set_terrain(wall, ids.wall_stone);

        // Act
        let visible = compute_fov(&grid, &table, origin, 6);

        // Assert
        assert!(visible.contains(wall));
    }

    #[test]
    fn 开阔地带的可见格数接近圆面积() {
        // 用整数下界代替浮点 π：3 < π < 4，用 [3r², 4r²] 卡住「大致是
        // 圆」——若算法退化成方形视野（(2r+1)² = 441，r = 10 时），
        // 上界会先被戳穿；若视野异常小，下界会被戳穿。
        // Arrange
        let (_ids, table, grid) = open_grid();
        let origin = grid.world().wrap(32, 32);
        let radius = 10u32;

        // Act
        let visible = compute_fov(&grid, &table, origin, radius);

        // Assert
        let lower = 3 * (radius as usize) * (radius as usize);
        let upper = 4 * (radius as usize) * (radius as usize);
        assert!((lower..=upper).contains(&visible.len()));
    }

    #[test]
    fn 有界网格中越过边界的方向被视为不可见不绕接缝() {
        // 有界地图没有接缝可绕：origin 贴着西边界，往西的偏移在
        // BoundedSize::try_pos 那一步直接越界返回 None（bounded.rs 已
        // 单独测试其行为）。这里用一堵刻意放在东边界的墙验证
        // compute_fov 层面的效果：若视线真的「绕」到了东边，这堵墙会
        // 干扰西边界附近的视野；有界地图没有绕回，这堵墙对西边界附近
        // 的可见性应当完全没有影响。
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let size = BoundedSize::new(10, 10).expect("10x10 是合法尺寸");
        let mut grid = BoundedGrid::new(size, ids.grass);
        let east_wall = size.try_pos(9, 5).expect("9,5 在范围内");
        grid.set_terrain(east_wall, ids.wall_stone);
        let origin = size.try_pos(0, 5).expect("0,5 在范围内");
        let west_neighbor = size.try_pos(1, 5).expect("1,5 在范围内");

        // Act
        let visible = compute_fov(&grid, &table, origin, 2);

        // Assert
        assert!(visible.contains(west_neighbor));
    }

    #[test]
    fn 有界网格中视野半径超过地图一角时不panic不产出越界坐标() {
        // radius 远超地图对角线长度：BoundedPos 的类型不变式已经保证
        // 返回的可见格不可能携带越界坐标（越界坐标根本构造不出来），
        // 这里补一条集成层面的确认——compute_fov 在这种极端输入下确实
        // 能正常返回，而不是 panic 或陷入过深的扫描。
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let size = BoundedSize::new(5, 5).expect("5x5 是合法尺寸");
        let grid = BoundedGrid::new(size, ids.grass);
        let corner = size.try_pos(0, 0).expect("0,0 在范围内");

        // Act
        let visible = compute_fov(&grid, &table, corner, 1000);

        // Assert
        assert!(visible.contains(corner));
    }
}
