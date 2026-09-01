//! 世界地图的**缩放视野**：档位、平移、以及一屏格子的归并产出。
//!
//! 与 [`crate::overview::continent_map`] 的关系：那个函数产出的是「整个
//! 世界压成一屏、每格取区块左上角一点」的固定视图，一格恒覆盖
//! `downsample` 个区块（默认 2 个，即 96×96 瓦片），且**只采一个点**。
//! 所有者的原话是「地图不再是这么大的方块，而且存在更多的细节……直接
//! 对地图做一定的缩放」——固定视图给不出这个，因此本模块在同一份
//! [`crate::overview::ContinentField`] 之上另开一条可缩放、可平移的
//! 查询路径。`continent_map` 保留不动：它仍然是「一眼看完整个世界」这个
//! 场景最直接的答案，也仍然是 p2/p5 验收 demo 的数据源。
//!
//! # 三条设计裁定
//!
//! 1. **档位是离散的**（[`ZOOM_LADDER`]，它同时决定屏上格数，见该常量
//!    文档「为什么是 `[4, 2, 1]`」一节）。连续缩放意味着「一格覆盖多少
//!    采样点」是浮点数，格边界会落在采样点中间：同一片地形在相邻两帧被
//!    归并进不同的格子，画面会抖；反向的「屏幕像素 → 区块」换算还会带上
//!    舍入漂移。离散档位下这两件事都是整数除法，不存在舍入（ADR 0002 的
//!    整数纪律）。
//! 2. **一格里有多种地形时取占比最高者**（[`dominant_terrain`]），不是取
//!    左上角一点。这条决定了「细节」到底有没有真的变多：只采一点的话，
//!    加密数据源等于白加——一片林子里恰好左上角是块草地，整格就画成草地。
//! 3. **平移在采样场这个环面上进行**，全程走 `TorusSize`，一次手写取模
//!    都没有（`docs/architecture/04-torus-topology.md`）。

use ll_core::torus::{TorusPos, TorusSize};

use crate::exploration::ExplorationMemory;
use crate::overview::{ContinentField, OverviewCell};
use crate::space::ZoneCoord;
use crate::terrain::TerrainKind;
use crate::zone::ZoneLayout;

/// 缩放档位表，单位是「一个地图格覆盖多少个**采样点**」，从最远到最近。
///
/// 第 0 档（最远）的值同时决定了视野的格数：
/// `view_cols = 采样场宽 / ZOOM_LADDER[0]`（向上取整），因为最远一档的
/// 定义就是「一屏装下整个世界」。默认布局下采样场是 256×192 个采样点，
/// 因此视野恒为 **64×48 格**。
///
/// 取 2 的幂且逐档减半：每一档「一格覆盖的瓦片数」都能被上一档整除，
/// 因此放大再缩小回来必然回到**逐位相同**的画面，不会因为取整方式攒下
/// 漂移。默认布局下三档分别是一格 48 / 24 / 12 个瓦片。
///
/// # 为什么是 `[4, 2, 1]` 而不是原来的 `[8, 4, 2, 1]`
///
/// 所有者实机反馈：「地图显示能不能细化更多，目前的方块还是太大了，
/// 不好用」。原来的 `ZOOM_LADDER[0] = 8` 把视野钉死在 32×24 格：默认
/// 1280×720 的窗口下地图面板约 1024×576 像素，一格 **24 像素见方**——
/// 那正是所有者说的「方块太大」。
///
/// 首档改成 4，视野随之变成 64×48 格，一格约 **12 像素**：同一块面板
/// 上格数翻两番（横纵各加倍），而**数据源一个字节都没加密**。
///
/// ## 另一条路（加密采样场）实测被否
///
/// 把 [`crate::overview::SAMPLES_PER_ZONE_AXIS`] 从 4 提到 8 也能让最细
/// 一档更细，但屏上的方块**一点都不会变小**——它只改变「一格代表多大
/// 一片地」，不改变一格画多少像素，因此根本不回答所有者的问题。而代价
/// 是实测的（release，默认 64×48 区块布局）：
///
/// | 项 | `SAMPLES_PER_ZONE_AXIS = 4` | `= 8` |
/// |---|---|---|
/// | 采样点数 | 49 152 | 196 608 |
/// | 建局时一次性生成 | 6.6 ms | 25.8 ms |
/// | 常驻内存 | 192 KB | 768 KB |
/// | [`world_map_slice`] 最远档**每帧** | 0.58 ms | 2.25 ms |
///
/// 最后一行是决定性的：地图开着的每一帧都要重算这一屏，2.25 ms 是 60 fps
/// 帧预算的 13%，换来的是「一格代表 6 格地」而不是 12 格——而
/// [`crate::overview::SAMPLES_PER_ZONE_AXIS`] 的文档早就论证过 12 格粒度
/// 对既定用途已经够用。本条路因此**不走**，`SAMPLES_PER_ZONE_AXIS` 一个
/// 字未改。
///
/// 改档位表这条路的同一项实测是 0.56 ms——与改动前持平（最远档恒等于
/// 「把整个采样场归并一遍」，与切成多少格无关）。
///
/// ## 「一格 = 一个区块」这条粒度没丢
///
/// 它从第 1 档挪到了**第 0 档**：`4 个采样点 × 12 瓦片 = 48 瓦片`，正是
/// 一个区块的边长。下一批「开局在地图上选重生点」要的粒度因此不但还在，
/// 而且落在**打开地图就看到的那一档**（最远档 = 整个世界一屏 = 一格一个
/// 区块），比原来还顺手。这条由 `存在一档其一格恰好覆盖一个区块` 钉住。
///
/// [`WorldMapSlice::zone_at_cell`] 的语义**一个字没改**：它回答的一直是
/// 「这一格的中心落在哪个区块」，在任何档位上都成立（更细的档位下一格
/// 落在区块内部，答案仍然是那个区块）。
pub const ZOOM_LADDER: [u32; 3] = [4, 2, 1];

/// 世界地图当前的缩放档位与视野中心。
///
/// **纯呈现状态**：不进 `WorldState`、不进存档、不参与回放，与
/// `ll_game::app::Demo::world_map_open` 同一条纪律（见
/// `ll_game::player_action` 模块文档「菜单状态算不算跨帧隐式状态」一节
/// 给出的三条判据）。世界不因为玩家把地图拖到哪里而有任何不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldMapView {
    /// [`ZOOM_LADDER`] 的下标，0 最远。
    level: usize,
    /// 视野中心，单位是**采样点**（不是瓦片也不是区块）。
    center: TorusPos,
}

impl WorldMapView {
    /// 建一个以某个世界瓦片坐标为中心、停在最远档位的视野。
    ///
    /// 最远档位是打开地图时最有用的那一档：玩家先看见整个世界，再决定
    /// 往哪儿放大。开局就停在放大档会让玩家不知道自己在世界的哪个角落。
    pub fn centered_on_tile(field: &ContinentField, tile: TorusPos) -> Self {
        WorldMapView {
            level: 0,
            center: sample_of_tile(field, tile),
        }
    }

    /// 当前档位下标（0 最远）。
    pub fn level(&self) -> usize {
        self.level
    }

    /// 当前视野中心（采样点坐标）。
    pub fn center(&self) -> TorusPos {
        self.center
    }

    /// 一个地图格覆盖多少个采样点。
    pub fn samples_per_cell(&self) -> u32 {
        ZOOM_LADDER[self.level.min(ZOOM_LADDER.len() - 1)]
    }

    /// 一个地图格覆盖多少个瓦片——比例尺文案要显示的就是这个数。
    pub fn tiles_per_cell(&self, field: &ContinentField) -> u32 {
        self.samples_per_cell() * field.sample_span()
    }

    /// 拉近一档；已经在最近一档时原地不动（不回绕）。
    ///
    /// 不回绕是刻意的：长按放大键时回绕会让画面突然跳回最远，玩家会以为
    /// 自己按错了键。到头就停是所有缩放控件的通行预期。
    pub fn zoom_in(&mut self) {
        if self.level + 1 < ZOOM_LADDER.len() {
            self.level += 1;
        }
    }

    /// 拉远一档；已经在最远一档时原地不动，理由同 [`Self::zoom_in`]。
    pub fn zoom_out(&mut self) {
        self.level = self.level.saturating_sub(1);
    }

    /// 按**地图格**平移视野：中心移动 `dx * samples_per_cell` 个采样点。
    ///
    /// 以格为单位而不是以采样点为单位：玩家按一次方向键，期待的是画面
    /// 移动一格（看得见的一步），而不是在最远档位下动一个连半格都不到的
    /// 距离。越界由 [`TorusSize::wrap`] 处理，视野因此能一路绕过接缝转回
    /// 原地——世界是环面的，地图也就没有边。
    pub fn pan(&mut self, field: &ContinentField, dx_cells: i32, dy_cells: i32) {
        let step = self.samples_per_cell() as i32;
        self.center = field.sample_size().wrap(
            self.center.x() + dx_cells * step,
            self.center.y() + dy_cells * step,
        );
    }
}

/// 把世界瓦片坐标换算成采样点坐标。整数除法，环绕走 `TorusSize`。
fn sample_of_tile(field: &ContinentField, tile: TorusPos) -> TorusPos {
    let span = field.sample_span().max(1) as i32;
    field.sample_size().wrap(tile.x() / span, tile.y() / span)
}

/// 一屏世界地图：格子加上还原坐标所需的全部元数据。
///
/// 带着 `origin`/`samples_per_cell` 一起返回，而不是只给一把格子：
/// 「屏幕上点了哪个区块」这个反向问题（下一批「开局在地图上选重生点」
/// 要用）需要它们才能算，而让调用方自己去重新推导一遍视野原点，就是在
/// 制造第二份真相源——两份一旦分叉，玩家点的地方和实际选中的区块会错开
/// 一整格，且错得毫无规律。
#[derive(Debug, Clone)]
pub struct WorldMapSlice {
    /// 按行主序排列的格子，长度恒等于 `cols * rows`。
    pub cells: Vec<OverviewCell>,
    /// 列数。
    pub cols: u32,
    /// 行数。
    pub rows: u32,
    /// 视野左上角那一格对应的采样点坐标（已环绕）。
    pub origin: TorusPos,
    /// 一格覆盖多少个采样点。
    pub samples_per_cell: u32,
    /// 采样场的尺寸（采样点数）——反向换算要靠它绕接缝。
    pub sample_size: TorusSize,
    /// 一个采样点覆盖多少个瓦片。
    pub sample_span: u32,
}

impl WorldMapSlice {
    /// 某一格中心落在哪个区块。列行越界时返回 `None`。
    ///
    /// # 为什么取格子**中心**而不是左上角
    ///
    /// 一格覆盖 `samples_per_cell²` 个采样点，而**视野原点未必对齐到区块
    /// 边界**（它由视野中心减去半屏算出，中心来自玩家所在的瓦片，奇偶
    /// 任意）——因此哪怕最远档一格恰好一个区块见方，它的左上角与中心
    /// 仍可能分属相邻两个区块。玩家点一格时心里指的是「这一片」，取中心
    /// 比取角落更接近那个意思；取左上角还会让整张图系统性地偏向左上
    /// 半格。
    ///
    /// **这条语义与档位表无关**，因此改 [`ZOOM_LADDER`] 不影响本方法回答
    /// 的问题：任何档位下它都返回「这一格中心所在的那个区块」，粒度恒是
    /// 区块——`ll_ui::hud::world_map::world_map_zone_at_pixel` 与下一批
    /// 「开局在地图上选重生点」依赖的正是这一条。
    ///
    /// 全程整数：格子中心的采样点偏移是 `samples_per_cell / 2`（整数
    /// 除法，`samples_per_cell` 为 1 时就是 0，即格子本身），采样点 →
    /// 瓦片是一次乘法，瓦片 → 区块走 [`ZoneLayout::tile_to_zone`]。
    /// 没有任何一步是浮点（ADR 0002）。
    pub fn zone_at_cell(&self, layout: &ZoneLayout, col: u32, row: u32) -> Option<ZoneCoord> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        let half = (self.samples_per_cell / 2) as i32;
        let sample = self.sample_size.wrap(
            self.origin.x() + (col * self.samples_per_cell) as i32 + half,
            self.origin.y() + (row * self.samples_per_cell) as i32 + half,
        );
        let span = self.sample_span.max(1) as i32;
        let tile = layout
            .tile_size()
            .wrap(sample.x() * span, sample.y() * span);
        Some(layout.tile_to_zone(tile).0)
    }

    /// 某个世界瓦片坐标落在本屏的哪一格；不在本屏视野内时返回 `None`。
    ///
    /// 这是 [`Self::zone_at_cell`] 的反向，玩家位置标记与据点标记共用
    /// 它——**同一个换算的正反两向写在同一个类型上**，两者因此不可能
    /// 各自对「视野原点在哪」有不同的理解而在缩放或接缝处错开一格。
    ///
    /// # 环面：这里要的是**正向**偏移，不是最短偏移
    ///
    /// 视野是从 `origin` 起**朝正方向**铺开的一个窗口，因此「这一点落在
    /// 第几格」问的是「从原点正向数过去多少个采样点」，而不是「离原点
    /// 最近有多远」。两者在环面上不是一回事：
    ///
    /// [`TorusSize::delta`] 给的是**最短带符号**偏移（`ll_core::torus`
    /// 模块文档），超过半周就折回负数。最远档位下视野正好覆盖整个采样
    /// 场，视野右半边的格子距原点超过半周，`delta` 对它们一律返回负数
    /// ——这些格子会被判成「不在视野内」而整块消失。**这不是设想出来的
    /// 风险：本模块第一版就是这么写的，被测试
    /// `格子与瓦片的正反换算互为逆运算` 当场抓住**（视野右下角那一格
    /// 换算回来得到 `None`）。
    ///
    /// 正向偏移由 [`TorusSize::wrap`] 给出：把 `目标 − 原点` 这个可能为
    /// 负、也可能超出一周的裸差值交给它，得到的坐标各分量恒落在
    /// `[0, 尺寸)` 内，正是「正向数过去多少」。**仍然一次手写取模都
    /// 没有**——`wrap` 就是本仓库为这件事提供的那个方法。
    pub fn cell_of_tile(&self, tile: TorusPos) -> Option<(u32, u32)> {
        let span = self.sample_span.max(1) as i32;
        let sample = self.sample_size.wrap(tile.x() / span, tile.y() / span);
        let forward = self
            .sample_size
            .wrap(sample.x() - self.origin.x(), sample.y() - self.origin.y());
        let col = forward.x() as u32 / self.samples_per_cell;
        let row = forward.y() as u32 / self.samples_per_cell;
        if col >= self.cols || row >= self.rows {
            return None;
        }
        Some((col, row))
    }
}

/// 一格里出现了多种地形时显示哪一种：**出现次数最多的那一种**；并列时
/// 取 [`TerrainKind`] 排序序最小的那一种。
///
/// # 为什么是众数，不是取左上角一点
///
/// 取一点等于把加密后的数据源浪费掉：一片林子里恰好左上角那个采样点是
/// 块草地，整格就画成草地——玩家放大之后看到的「细节」是假的。众数
/// 回答的是「这一片**最像**什么」，那正是所有者要的「更清晰地看清楚是
/// 什么东西」。
///
/// # 为什么不用 `HashMap` 计数（约束 C5）
///
/// 哈希容器的迭代顺序不保证跨运行/跨平台一致，一旦拿它的顺序去破并列，
/// 同一份世界在两台机器上会画出不同的地图。这里改为**排序后线性扫最长
/// 等值游程**：`TerrainKind` 已派生 `Ord`（其文档写明「派生它是 C5 的
/// 正向选择」），排序是确定性的，扫描也是。
///
/// # 并列怎么破
///
/// 只在**严格大于**当前最长游程时才替换答案，因此已排序序列里先出现的
/// （即 `Ord` 更小的）那一种胜出——规则固定、可复现，不依赖输入顺序。
///
/// `samples` 为空时返回 `None`（调用方保证不会发生：每格至少一个采样）。
///
/// 取 `&mut [TerrainKind]` 而不是 `&mut Vec`：本函数只需要能就地排序，
/// 不改变长度（`clippy::ptr_arg`）。调用方因此可以复用同一个 `Vec` 的
/// 缓冲区逐格调用，不必每格新分配一次。
pub fn dominant_terrain(samples: &mut [TerrainKind]) -> Option<TerrainKind> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_unstable();
    let mut best = samples[0];
    let mut best_run = 0usize;
    let mut current = samples[0];
    let mut current_run = 0usize;
    for &kind in samples.iter() {
        if kind == current {
            current_run += 1;
        } else {
            current = kind;
            current_run = 1;
        }
        if current_run > best_run {
            best_run = current_run;
            best = current;
        }
    }
    Some(best)
}

/// 按 `view` 取一屏世界地图。
///
/// 与 [`crate::overview::continent_map`] 共享的两条既有保证：**只读**
/// （签名不接受 `&mut WorldState`，因此结构上不可能触发流式加载或改动
/// 世界状态），以及**战争迷雾由消费者过滤**（本函数如实给出真实地形与
/// `explored` 标志，遮不遮是 `ll_ui::hud::world_map` 的事，见那边的模块
/// 文档）。
///
/// `explored` 的粒度：一格覆盖的那一片瓦片里**只要有一格去过**，这一格
/// 就算已探索——与 `continent_map` 在区块粒度上用
/// [`ExplorationMemory::zone_has_any_explored`] 是同一条取舍（地图格比
/// 记忆的粒度粗时，「去过一点」比「全部去过」更接近玩家的心理预期：
/// 走过村口不该让整个村子还黑着）。
pub fn world_map_slice(
    field: &ContinentField,
    layout: &ZoneLayout,
    exploration: &ExplorationMemory,
    view: &WorldMapView,
) -> WorldMapSlice {
    let sample_size = field.sample_size();
    let samples_per_cell = view.samples_per_cell().max(1);
    let cols = sample_size.width().div_ceil(ZOOM_LADDER[0]);
    let rows = sample_size.height().div_ceil(ZOOM_LADDER[0]);

    // 视野左上角：中心往左上各退半个视野，单位是采样点。整数除法，
    // 环绕走 TorusSize——手写取模在这里会正好在接缝上出错。
    let half_w = (cols * samples_per_cell / 2) as i32;
    let half_h = (rows * samples_per_cell / 2) as i32;
    let origin = sample_size.wrap(view.center.x() - half_w, view.center.y() - half_h);

    let mut cells = Vec::with_capacity((cols * rows) as usize);
    let mut bucket: Vec<TerrainKind> =
        Vec::with_capacity((samples_per_cell * samples_per_cell) as usize);
    for row in 0..rows {
        for col in 0..cols {
            bucket.clear();
            let mut explored = false;
            for dy in 0..samples_per_cell as i32 {
                for dx in 0..samples_per_cell as i32 {
                    let sample = sample_size.wrap(
                        origin.x() + (col * samples_per_cell) as i32 + dx,
                        origin.y() + (row * samples_per_cell) as i32 + dy,
                    );
                    bucket.push(field.terrain_at_sample(sample));
                    if !explored && sample_explored(field, layout, exploration, sample) {
                        explored = true;
                    }
                }
            }
            // bucket 恒非空（samples_per_cell 已钳到至少 1），unwrap_or
            // 只是为了不在纯呈现路径上 panic。
            let terrain =
                dominant_terrain(&mut bucket).unwrap_or_else(|| field.terrain_at_sample(origin));
            cells.push(OverviewCell { terrain, explored });
        }
    }

    WorldMapSlice {
        cells,
        cols,
        rows,
        origin,
        samples_per_cell,
        sample_size,
        sample_span: field.sample_span(),
    }
}

/// 一个采样点所覆盖的那一小片瓦片里，有没有去过的格子。
///
/// 走的是区块粒度的 [`ExplorationMemory::zone_has_any_explored`]：采样点
/// （默认 12 格见方）比区块（48 格见方）细，因此这是一个**偏宽松**的
/// 近似——同一个区块内的采样点探索状态相同。选宽松而不是逐瓦片精确查询
/// 的理由是代价：精确查询要对一格里最多 64 个采样点、每个再展开 12×12
/// 个瓦片各查一次，每帧 768 格算下来是七百万次查询；而这份数据的用途只是
/// 「这一片黑不黑」。
fn sample_explored(
    field: &ContinentField,
    layout: &ZoneLayout,
    exploration: &ExplorationMemory,
    sample: TorusPos,
) -> bool {
    let span = field.sample_span().max(1) as i32;
    let tile = layout
        .tile_size()
        .wrap(sample.x() * span, sample.y() * span);
    let (zone, _local) = layout.tile_to_zone(tile);
    exploration.zone_has_any_explored(zone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{GenParams, build_zone_noise};
    use crate::overview::generate_continent_field;
    use crate::terrain::base_terrain_fixture;
    use ll_core::torus::TorusSize;

    /// 测试布局：8×8 个区块、区块边长 48（与生产默认值一致）。采样场
    /// 因此是 32×32 个采样点，最远档位（一格 8 个采样点）下视野是
    /// 4×4 格——小到能在断言里逐格数清楚，又大到平移/接缝不退化成平凡。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(8, 8).expect("8x8 是合法尺寸");
        ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
    }

    fn test_field() -> (ZoneLayout, ContinentField) {
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let field = generate_continent_field(&layout, &noise, &params, &terrain_ids);
        (layout, field)
    }

    #[test]
    fn 归并取占比最高的地形而不是第一个() {
        // Arrange：3 个 A、1 个 B，且 B 排在最前面——取「第一个」会答错。
        let (ids, _table) = base_terrain_fixture();
        let mut samples = vec![ids.deep_water, ids.grass, ids.grass, ids.grass];

        // Act
        let winner = dominant_terrain(&mut samples);

        // Assert
        assert_eq!(winner, Some(ids.grass));
    }

    #[test]
    fn 归并平局时取terrainkind排序序最小的那一种() {
        // Arrange：2 比 2 打平。平局破法必须固定，且与输入顺序无关——
        // 两种排列必须给出同一个答案（约束 C5）。
        let (ids, _table) = base_terrain_fixture();
        let (small, large) = if ids.grass < ids.deep_water {
            (ids.grass, ids.deep_water)
        } else {
            (ids.deep_water, ids.grass)
        };
        let mut forward = vec![small, small, large, large];
        let mut backward = vec![large, large, small, small];

        // Act
        let a = dominant_terrain(&mut forward);
        let b = dominant_terrain(&mut backward);

        // Assert
        assert_eq!(a, Some(small));
        assert_eq!(b, Some(small), "平局破法必须与输入顺序无关（C5）");
    }

    #[test]
    fn 归并空输入返回空而不是panic() {
        // Arrange
        let mut samples: Vec<TerrainKind> = Vec::new();

        // Act & Assert
        assert_eq!(dominant_terrain(&mut samples), None);
    }

    #[test]
    fn 存在一档其一格恰好覆盖一个区块() {
        // 下一批「开局在地图上选重生点」依赖这条粒度（所有者裁定：玩家点
        // 一个区块，然后在那个区块内随机挑一格陆地出生）。档位表改动最
        // 容易悄悄弄丢的就是它——格数与像素尺寸的变化肉眼看得见，这条
        // 粒度不见了却要等到下一批才发现。
        // Arrange
        let (layout, field) = test_field();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));

        // Act：逐档走一遍，收集每一档「一格覆盖多少瓦片」。
        let mut tiles_per_cell = vec![view.tiles_per_cell(&field)];
        for _ in 1..ZOOM_LADDER.len() {
            view.zoom_in();
            tiles_per_cell.push(view.tiles_per_cell(&field));
        }

        // Assert
        assert!(
            tiles_per_cell.contains(&layout.zone_span()),
            "没有任何一档是「一格一个区块」（区块边长 {}），实测各档为 {:?}",
            layout.zone_span(),
            tiles_per_cell
        );
    }

    #[test]
    fn 最远档一格恰好一个区块因此打开地图就是选重生点要的粒度() {
        // 上一条只要求「存在」这一档；这一条钉住它就在**第 0 档**——
        // 打开地图看到的那一档同时是「整个世界一屏」与「一格一个区块」，
        // 选重生点界面因此不需要先替玩家缩放到某一档。
        // Arrange
        let (layout, field) = test_field();

        // Act
        let view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));

        // Assert
        assert_eq!(view.level(), 0);
        assert_eq!(view.tiles_per_cell(&field), layout.zone_span());
    }

    #[test]
    fn 视野格数是每个区块一格() {
        // 「屏上方块更小」这条需求的程序化判据：视野格数恒等于世界的区块
        // 数（最远档一格一个区块），面板尺寸不变的前提下格数越多、每格
        // 像素越小。改档位表首档会直接改变这个数，这条把它钉在明处。
        // Arrange
        let (layout, field) = test_field();
        let view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        let exploration = ExplorationMemory::new();

        // Act
        let slice = world_map_slice(&field, &layout, &exploration, &view);

        // Assert
        assert_eq!(slice.cols, layout.zone_count().width());
        assert_eq!(slice.rows, layout.zone_count().height());
    }

    #[test]
    fn 每一档都是二的幂且逐档减半() {
        // 「放大再缩小逐位回到原样」依赖这条。既有测试从行为侧
        // （`放大再缩小回来得到逐位相同的一屏`）守它，这一条从常量侧
        // 守它——档位表被改成非 2 的幂时，这里立刻红，而不是等某条
        // 行为断言在某个偏移下偶然抓到。
        // Arrange & Act & Assert
        for window in ZOOM_LADDER.windows(2) {
            assert_eq!(
                window[0],
                window[1] * 2,
                "档位表必须逐档减半：{window:?} 不满足"
            );
        }
        for level in ZOOM_LADDER {
            assert!(level.is_power_of_two(), "档位 {level} 不是 2 的幂");
        }
    }

    #[test]
    fn 最远档位一屏恰好装下整个采样场() {
        // Arrange
        let (layout, field) = test_field();
        let view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        let exploration = ExplorationMemory::new();

        // Act
        let slice = world_map_slice(&field, &layout, &exploration, &view);

        // Assert：一格覆盖的采样点数乘格数恰好等于整个采样场。
        assert_eq!(view.samples_per_cell(), ZOOM_LADDER[0]);
        assert_eq!(
            slice.cols * slice.samples_per_cell,
            field.sample_size().width()
        );
        assert_eq!(
            slice.rows * slice.samples_per_cell,
            field.sample_size().height()
        );
        assert_eq!(slice.cells.len() as u32, slice.cols * slice.rows);
    }

    #[test]
    fn 放大之后同一片世界被切成更多格因此细节真的变多了() {
        // 这是「更多细节」这条需求的程序化判据：格数不变（视野格数恒定），
        // 但每格覆盖的瓦片数必须严格变小——同一块屏幕面积上，世界被切得
        // 更细。
        // Arrange
        let (layout, field) = test_field();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        let exploration = ExplorationMemory::new();
        let far = world_map_slice(&field, &layout, &exploration, &view);
        let far_tiles = view.tiles_per_cell(&field);

        // Act
        view.zoom_in();
        let near = world_map_slice(&field, &layout, &exploration, &view);
        let near_tiles = view.tiles_per_cell(&field);

        // Assert
        assert_eq!((near.cols, near.rows), (far.cols, far.rows), "视野格数恒定");
        assert!(
            near_tiles < far_tiles,
            "放大后一格必须覆盖更少瓦片：{near_tiles} 应小于 {far_tiles}"
        );
        assert_eq!(far_tiles, near_tiles * 2, "档位逐档减半");
    }

    #[test]
    fn 缩放档位到头就停不回绕() {
        // Arrange
        let (layout, field) = test_field();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));

        // Act：一路拉近，超出档位数很多次。
        for _ in 0..(ZOOM_LADDER.len() * 3) {
            view.zoom_in();
        }
        let deepest = view.level();
        for _ in 0..(ZOOM_LADDER.len() * 3) {
            view.zoom_out();
        }

        // Assert
        assert_eq!(deepest, ZOOM_LADDER.len() - 1);
        assert_eq!(view.level(), 0);
    }

    #[test]
    fn 放大再缩小回来得到逐位相同的一屏() {
        // 档位取 2 的幂且逐档整除，因此来回缩放不该攒下任何漂移。
        // Arrange
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(100, 100));
        let before = world_map_slice(&field, &layout, &exploration, &view);

        // Act
        view.zoom_in();
        view.zoom_in();
        view.zoom_out();
        view.zoom_out();
        let after = world_map_slice(&field, &layout, &exploration, &view);

        // Assert
        assert_eq!(before.origin, after.origin);
        assert_eq!(before.cells, after.cells);
    }

    #[test]
    fn 平移绕世界一整圈后逐位回到起点() {
        // 环面上，视野一路往西平移必然绕回世界东侧。若哪一步手写了取模
        // （或者干脆忘了环绕），原点会跑到负数或越界，这一屏要么 panic
        // 要么画出一片错误的地形。
        // Arrange
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        let start = world_map_slice(&field, &layout, &exploration, &view);
        let cells_per_lap = field.sample_size().width() / view.samples_per_cell();

        // Act
        for _ in 0..cells_per_lap {
            view.pan(&field, -1, 0);
        }
        let full_lap = world_map_slice(&field, &layout, &exploration, &view);

        // Assert：绕满一圈必须逐位回到起点——这同时证明中途每一步都没有
        // 越界，也没有因为接缝而丢格或重复。
        assert_eq!(start.origin, full_lap.origin);
        assert_eq!(start.cells, full_lap.cells);
    }

    #[test]
    fn 从原点往西平移一格视野中心绕到世界东侧() {
        // 这是接缝上最直接的一条：中心在采样场原点，往**负**方向挪一格。
        // 任何形式的钳制（`max(0)`）或忘记环绕都会让中心卡在 0 不动。
        //
        // 为什么单独要这一条：`平移绕世界一整圈后逐位回到起点` 从原点
        // 出发绕整圈，钳制实现下中心恒为 0，一圈之后「碰巧」也回到起点
        // ——那条测试因此咬不住钳制。**这不是设想：本批做 ADR 0022 反例
        // 验证时，把 `wrap` 换成 `max(0)` 后那条测试照样全绿，这一条是
        // 补上去堵这个洞的。**
        // Arrange
        let (layout, field) = test_field();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        assert_eq!(view.center().x(), 0, "前置：中心确实在采样场原点");
        let step = view.samples_per_cell();

        // Act
        view.pan(&field, -1, 0);

        // Assert：绕到世界东侧，恰好一格远。
        assert_eq!(
            view.center().x(),
            (field.sample_size().width() - step) as i32
        );
        assert_eq!(view.center().y(), 0, "只往西挪，纵向不该动");
    }

    #[test]
    fn 往西平移一格之后新的第一列等于旧的第零列() {
        // 与 `平移一格之后新的第零列等于旧的第一列` 方向相反的一条：
        // 往负方向挪同样必须是「整体挪一格」，而不是卡住不动。从原点
        // 出发，因此这一条同时走的是接缝。
        // Arrange
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        let before = world_map_slice(&field, &layout, &exploration, &view);

        // Act
        view.pan(&field, -1, 0);
        let after = world_map_slice(&field, &layout, &exploration, &view);

        // Assert
        assert_ne!(before.origin, after.origin, "往西挪一格必须真的挪动");
        for row in 0..after.rows {
            let new_cell = after.cells[(row * after.cols + 1) as usize];
            let old_cell = before.cells[(row * before.cols) as usize];
            assert_eq!(new_cell, old_cell, "第 {row} 行往西平移后没有对齐");
        }
    }

    #[test]
    fn 平移一格之后新的第零列等于旧的第一列() {
        // 「连续」的另一面：往东挪一格，画面应当整体左移一格，而不是
        // 跳一大段或原地不动。
        // Arrange
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        let before = world_map_slice(&field, &layout, &exploration, &view);

        // Act
        view.pan(&field, 1, 0);
        let after = world_map_slice(&field, &layout, &exploration, &view);

        // Assert：新图第 (0, row) 格 == 旧图第 (1, row) 格。
        for row in 0..after.rows {
            let new_cell = after.cells[(row * after.cols) as usize];
            let old_cell = before.cells[(row * before.cols + 1) as usize];
            assert_eq!(new_cell, old_cell, "第 {row} 行平移后没有对齐");
        }
    }

    #[test]
    fn 格子与瓦片的正反换算互为逆运算() {
        // `cell_of_tile` 与 `zone_at_cell` 是同一个换算的两个方向。随便
        // 挑一格，取它中心所在的区块，再把那个区块的锚点瓦片喂回去，
        // 必须回到同一格。
        // Arrange
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        let slice = world_map_slice(&field, &layout, &exploration, &view);

        // Act & Assert
        for row in 0..slice.rows {
            for col in 0..slice.cols {
                let zone = slice
                    .zone_at_cell(&layout, col, row)
                    .expect("列行来自本切片自身的遍历，恒在范围内");
                let tile = layout.tile_size().wrap(
                    zone.x() * layout.zone_span() as i32,
                    zone.y() * layout.zone_span() as i32,
                );
                assert_eq!(
                    slice.cell_of_tile(tile),
                    Some((col, row)),
                    "格 ({col}, {row}) 的中心区块换算回来错位了"
                );
            }
        }
    }

    #[test]
    fn 格子中心所在区块随格子推进而单调推进() {
        // 「取中心」不该退化成「整屏一个区块」：最远档位下相邻两格必须
        // 落在不同的区块上，否则反向换算给不出可用的选点粒度。
        // Arrange
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        let slice = world_map_slice(&field, &layout, &exploration, &view);

        // Act
        let first = slice.zone_at_cell(&layout, 0, 0).expect("0,0 恒在范围内");
        let second = slice.zone_at_cell(&layout, 1, 0).expect("1,0 恒在范围内");

        // Assert
        assert_ne!(first.x(), second.x());
    }

    #[test]
    fn 视野外的瓦片换算为空而不是钳到边缘() {
        // 钳到边缘会让玩家标记贴在地图边上不动，看起来像「我一直在世界
        // 尽头」——比不画更容易误导。
        // Arrange：放到最近一档，视野只覆盖世界的一小块。
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let mut view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));
        for _ in 0..ZOOM_LADDER.len() {
            view.zoom_in();
        }
        let slice = world_map_slice(&field, &layout, &exploration, &view);

        // Act：取世界正对面那一点（环面上离视野中心最远）。
        let far = layout.tile_size().wrap(
            layout.tile_size().width() as i32 / 2,
            layout.tile_size().height() as i32 / 2,
        );

        // Assert
        assert!(slice.cell_of_tile(far).is_none());
    }

    #[test]
    fn 视野跨接缝时接缝对侧的瓦片仍然落在正确的格子上() {
        // 这是环面上最容易错的那一处：视野原点靠近世界东缘，视野本身
        // 绕过接缝伸到西侧。用朴素的坐标相减会算出一个巨大的负偏移，
        // 接缝西侧的目标会被判成「不在视野内」而整个消失。
        // Arrange：把视野中心放在世界最东边一列，最近档位（视野最小，
        // 绕接缝的效果最明显）。
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let tile_size = layout.tile_size();
        let mut view = WorldMapView::centered_on_tile(&field, tile_size.wrap(-1, 0));
        for _ in 0..ZOOM_LADDER.len() {
            view.zoom_in();
        }
        let slice = world_map_slice(&field, &layout, &exploration, &view);

        // Act：接缝东侧最后一格、与它在环面上紧邻的西侧第一格。
        let east = slice.cell_of_tile(tile_size.wrap(-1, 0));
        let west = slice.cell_of_tile(tile_size.wrap(0, 0));

        // Assert：两者都必须在视野内，且列号相差不超过一格——环面上
        // 它们是紧邻的两格瓦片，在屏幕上也必须紧邻。
        let east = east.expect("视野中心那一格必然在视野内");
        let west = west.expect("接缝对侧紧邻的一格也必须在视野内，不能因为接缝而消失");
        assert!(
            west.0.abs_diff(east.0) <= 1,
            "接缝两侧紧邻的瓦片被画到了相隔 {} 格的地方",
            west.0.abs_diff(east.0)
        );
    }

    #[test]
    fn 一屏格子在同一份输入下逐位可复现() {
        // C5 的正面断言：归并不碰任何哈希容器，因此同一份输入必然产出
        // 逐位相同的一屏。
        // Arrange
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(77, 33));

        // Act
        let a = world_map_slice(&field, &layout, &exploration, &view);
        let b = world_map_slice(&field, &layout, &exploration, &view);

        // Assert
        assert_eq!(a.cells, b.cells);
    }

    #[test]
    fn 未探索时一屏全部标记为未探索() {
        // Arrange
        let (layout, field) = test_field();
        let exploration = ExplorationMemory::new();
        let view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));

        // Act
        let slice = world_map_slice(&field, &layout, &exploration, &view);

        // Assert
        assert!(slice.cells.iter().all(|cell| !cell.explored));
    }

    #[test]
    fn 一格覆盖范围内探索过一点该格就算已探索() {
        // Arrange：只标记一个瓦片。
        let (layout, field) = test_field();
        let mut exploration = ExplorationMemory::new();
        exploration.mark_explored(&layout, layout.tile_size().wrap(3, 3));
        let view = WorldMapView::centered_on_tile(&field, layout.tile_size().wrap(0, 0));

        // Act
        let slice = world_map_slice(&field, &layout, &exploration, &view);

        // Assert：至少一格已探索，且不是全部——「宽松但不是恒真」。
        assert!(slice.cells.iter().any(|cell| cell.explored));
        assert!(slice.cells.iter().any(|cell| !cell.explored));
    }
}
