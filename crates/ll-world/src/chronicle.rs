//! 世界历史生成：玩家进入之前，世界已经跑过的那一段。
//!
//! # 这个模块存在的理由
//!
//! 项目所有者的裁决：NPC「需要根据历史生成，放在文明据点或者营地或者
//! 某种合理的地点」「需要计算进历史生成器里面，类似矮人要塞那样」。
//! 「类似矮人要塞」的核心不是事件日志好看，而是**世界在玩家到场之前
//! 已经被历史塑造过**——哪里有村子、哪里只剩废墟，是几百年兴衰的
//! 结果，不是开局现掷的骰子。
//!
//! 本模块因此交付的不是一份日志，而是一条完整链路：
//!
//! ```text
//! 种子 + 地形  →  选址（哪些地方能住人）
//!              →  推演 N 个纪元（建立 / 兴盛 / 衰败 / 遗弃）
//!              →  最终快照（[`crate::settlement::SettlementSite`]）
//!              →  真的写进地形（[`crate::settlement::stamp_settlement`]）
//! ```
//!
//! 最后那一步是关键：`SettlementFounded` 这一类事件**真的改变了世界
//! 当前的样子**——一座第 2 纪元建立、至今仍有人住的村子，在地上就是
//! 一片木墙木门的屋子；一座第 5 纪元被遗弃的，在地上就是一片塌了一半
//! 的石头废墟。仓库里已经有过太多「声明了但从没接线」的东西，本模块
//! 刻意不做第 N + 1 个。
//!
//! # 落地范围（如实标注）
//!
//! **本批次交付的是最小但真的能用的一版**，不是 `world-history.md`
//! 描述的那套 500 年 × 500 聚落、王朝继承、联姻、政变、一万五千名
//! 历史人物的完整模拟。具体地说：
//!
//! - 事件只有两类：[`crate::history::HistoricalEventKind::SettlementFounded`]
//!   与 [`crate::history::HistoricalEventKind::SettlementAbandoned`]。
//! - 没有战争、没有王朝、没有具名历史人物。
//! - **没有 NPC**：据点现在是空的房子。生成居民需要一张「据点里有哪些
//!   职业、各几个」的内容表（`SettlementTemplateTable`）与一条世界
//!   生成期的 `Agent` 构造路径，两者都不在本批次——见本模块文档
//!   「下一步的牵动面」。
//!
//! # 确定性（约束 C3 / C5）
//!
//! - **选址完全不用随机**：它是 `f(world_seed, zone)` 的纯地形判定，
//!   而地形本身已经由 `TileableNoise(seed)` 完全确定（决策 0005），
//!   连通域分析（[`crate::land`]）是确定性算法。没有 RNG 就没有 C3
//!   问题。
//! - **推演用随机，但每一掷都由三元组算出**：
//!   `DetRng::for_entity(world_seed, CHRONICLE_STREAM_ID, 候选序号 * 纪元数 + 纪元号)`
//!   ——形状照抄已落地的 `crate::weather::weather_kind_at`（固定流
//!   编号 + 「第几个时间周期」），本模块只是把「时间周期号」换成
//!   「第几个候选点的第几个纪元」。同一次掷骰永远给同一个数，与调用
//!   顺序无关。
//! - 候选点按区块光栅序（`zone_y` 外层、`zone_x` 内层）收集，全程只用
//!   `Vec`，不触碰任何 `HashMap`/`HashSet` 的迭代顺序（约束 C5）。
//!
//! 逐位相同的验收见本模块测试 `同一种子两次独立生成的编年史逐字段相同`
//! 与 `不同种子产出不同的编年史`。
//!
//! # 为什么编年史不进存档
//!
//! ADR 0009「默认派生，只存偏差」。整份 [`WorldChronicle`] 是种子的
//! 纯函数，读档时重新派生即可（与 `TileableNoise` 同一条纪律，见
//! `ll_game::rebuild_noise`）。**唯一需要与存档协调的是 `WorldId`
//! 空间**：编年史分配掉的号码不能被游戏内的击杀记录再分配一次，因此
//! [`WorldChronicle::next_world_id`] 要被写进 `WorldState::next_world_id`
//! ——见 `ll_game::world::build_new_world` 的调用点。
//!
//! # 下一步的牵动面（本批次没做的那两件）
//!
//! 1. **NPC 生成**。缺两样：一张内容表（据点模板 → 职业名册），以及
//!    世界生成期构造 `Agent` 的路径。后者可以照 `ll_game::world` 的
//!    `build_player_agent` 抄，**不需要新的 `Effect`**（世界生成发生在
//!    游戏开始之前，不经 `Intent`/`resolve`/`apply`）。前者要动
//!    `GameplayTables`（27 处构造点）、`content_hash`（版本号递增）、
//!    `content_audit`、i18n 两份 `.ftl`——那是一个完整批次的量。
//! 2. **游戏中动态刷怪**需要 `Effect::SpawnActor`（ADR 0023：世界状态
//!    改动必须经 `apply` 产出的 `Effect`）。**核实：该变体当前不存在**
//!    （全仓库仅 `ll-sim/src/subclass.rs` 的一句注释提到过这个名字）。
//!    本模块走不到这条路。

use ll_core::ident::WorldId;
use ll_core::rng::DetRng;
use ll_core::time::{DAYS_PER_SEASON, SEASONS_PER_YEAR, TICKS_PER_DAY, Tick};
use ll_core::torus::TorusPos;

use crate::generate::{GenParams, generate_zone_window, zone_representative_terrain};
use crate::history::{
    HistoricalEvent, HistoricalEventKind, SettlementAbandonedRecord, SettlementFoundedRecord,
};
use crate::land::largest_walkable_component;
use crate::noise::TileableNoise;
use crate::settlement::{MAX_BUILDINGS, SettlementSite, SettlementStatus};
use crate::space::ZoneCoord;
use crate::terrain::{BaseTerrainIds, TerrainTable};
use crate::zone::ZoneLayout;

/// 历史推演所用的随机流编号——与
/// [`crate::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]（建筑铺法）
/// 分开，理由见后者文档。
pub const CHRONICLE_STREAM_ID: u64 = 0x0043_4852_4F4E_0001;

/// 一个纪元有多少年。取 25：12 个纪元合计 300 年，量级上与
/// `world-history.md` 设想的「几百年」一致，同时让每个纪元的兴衰在
/// 叙事上是「一代人到两代人」这个可理解的粒度。
pub const YEARS_PER_EPOCH: i64 = 25;

/// 一个纪元有多少 tick——事件时刻用的换算。
pub const TICKS_PER_EPOCH: i64 =
    TICKS_PER_DAY * DAYS_PER_SEASON * SEASONS_PER_YEAR * YEARS_PER_EPOCH;

/// 历史推演的可调参数。全部有默认值，调用方通常只用
/// [`ChronicleParams::default`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChronicleParams {
    /// 推演多少个纪元。
    pub epochs: u32,
    /// 最多对多少个区块做**完整**的连通域分析（生成整窗 + BFS）。
    ///
    /// 这是启动路径上的性能闸门：一个区块窗口 48×48 = 2304 格，完整
    /// 分析一个区块的代价与 `ll_game::world::find_spawn_site` 检查一个
    /// 区块相同。廉价预筛（[`zone_representative_terrain`]，每区块一次
    /// O(1) 噪声采样）仍会跑遍全部区块，只有通过预筛的才计入这个上限。
    ///
    /// 默认值大于本体世界的区块总数（64×48 = 3072），也就是说**默认
    /// 不再是一道会真的收紧的闸门**——它留在这里是为了让「勘察成本
    /// 有界」这条性质仍然由类型表达出来（本函数因此不含任何无界
    /// 循环），以及让内存/时间受限的调用方仍能显式收紧。取值理由与
    /// 实测数字见 [`ChronicleParams::default`]。
    pub survey_zone_budget: usize,
    /// 一个区块要被视为「能住人」，其最大连通可行走陆地至少要有多少
    /// 格。取值理由见 [`ChronicleParams::default`]。
    pub min_settlement_land_area: usize,
    /// 两座据点的锚点之间至少要隔多少格（环面切比雪夫距离）。
    ///
    /// # 为什么需要这条规则
    ///
    /// 「一个区块至多一座据点」这条既有约束**不等于**据点之间有距离：
    /// 相邻的两个区块可以各有一座，两个锚点最近能挨到 1 格——地上看
    /// 到的是连成一片的两个村子，而不是两个地方。项目所有者的裁决
    /// 「据点与据点之间你需要设定好距离」点的正是这一条。
    ///
    /// # 它同时是勘察预算的减压阀
    ///
    /// 判定「这个区块整个落在某座已接受据点的禁区内」只需要几次整数
    /// 比较（[`zone_fully_excluded`]），而完整的连通域分析要生成
    /// 2304 格窗口再跑一遍 BFS。禁区内的区块因此**既不做分析、也不
    /// 计入 `survey_zone_budget`**——这就是把预算抬到扫完全世界之后，
    /// 耗时反而回落的原因（实测见 [`ChronicleParams::default`]）。
    ///
    /// 取 0 表示不做间距筛选（此时行为与本字段引入之前完全一致）。
    pub min_settlement_spacing: u32,
}

impl Default for ChronicleParams {
    /// 12 个纪元（300 年）、勘察预算 4800 个区块、最小可住面积 400 格。
    ///
    /// - **4800 个区块**：本体默认布局共 64×48 = 3072 个区块，本值大于
    ///   它，因此默认配置下预算**不会**在扫完世界之前耗尽。
    ///
    ///   **为什么从 48 抬到这个量级。** 项目所有者实测报告：据点全挤在
    ///   世界一角、其余部分空无一物。原因是预算 48 一耗尽扫描就停，而
    ///   扫描走的是区块光栅序——48 个完整勘察在本体世界里只够覆盖**前
    ///   两行区块**。实测（release，本体默认布局，三个种子各一次）：
    ///
    ///   | 预算 | 编年史耗时 | 据点数 | 覆盖到的区块行 |
    ///   |---|---|---|---|
    ///   | 48 | 15~16ms | 43~45 | `zone_y ∈ [0, 1]` |
    ///   | 480 | 158~161ms | 437~450 | `zone_y ∈ [0, 12]` |
    ///   | 4800 | 787~950ms | 2024~2391 | `zone_y ∈ [0, 47]`（全世界） |
    ///
    ///   只有 4800 这一档真的铺满了整个世界，另外两档都只是把「挤在
    ///   一角」这个毛病挪到了「挤在上面几行」。**代价是编年史生成从
    ///   16ms 涨到 0.8~0.95 秒**——这是一次性的建档路径（读档不重跑
    ///   连通域分析，见模块文档「为什么编年史不进存档」下的
    ///   `rebuild_chronicle`），不在每帧路径上，但它确实是一次真实的
    ///   开局等待。另一条同时成立的观察：2000 座据点铺在 3072 个区块
    ///   上，等于**几乎每一个可住区块都有一座村子**，相邻两个区块各
    ///   一座、实际上连成一片。这条要靠据点之间的最小间距来解，不是
    ///   靠把预算调回去。
    ///
    ///   **最小间距（[`ChronicleParams::min_settlement_spacing`]）落地
    ///   之后，上表最后一行被下表取代**（同样是 release、本体默认
    ///   布局、同样三个种子，预算恒为 4800，只变间距）：
    ///
    ///   | 间距 | 编年史耗时 | 据点数 | 覆盖到的区块行 |
    ///   |---|---|---|---|
    ///   | 0（不筛） | 787~950ms | 2024~2391 | `zone_y ∈ [0, 47]` |
    ///   | 144（默认） | 110~178ms | 232~268 | `zone_y ∈ [0, 45]` |
    ///
    ///   耗时回落到 480 档以下、覆盖面却是全世界，据点数落在两百多座
    ///   这个「一片大陆上的文明」的量级。整条 `build_new_world` 实测
    ///   160ms（其中 `WorldState::new` 含出生邻域预热 10~14ms、
    ///   `SurfaceStore::install_chronicle` 10~12ms，其余是编年史与出生
    ///   点搜索）——对照改动前的 31ms。**测量环境如实标注**：同一台机器
    ///   上另有并行编译在跑，逐次抖动可达 3 倍，上表取的是各配置五次
    ///   重复的最小值，跨多轮再取最小/最大作为区间。
    ///
    ///   顺带纠正一条旧记录：本文档此前写着「真要再快，第一步是让编
    ///   年史的勘察与 `find_spawn_site` 共用同一次区块窗口生成（两者
    ///   现在各生成一遍）」。**那条省不下什么**——`find_spawn_site`
    ///   在**第一个**合格区块上就 `return`，它的上限 128 是「找不到就
    ///   放弃」的兜底，不是它实际要跑的次数；本体默认种子下它只完整
    ///   勘察个位数个区块。共用窗口能省的是那几个区块，代价却是改动
    ///   出生点语义，不划算。
    /// - **400 格**：略低于出生点要求的 500（`MIN_SPAWN_LAND_AREA`）。
    ///   出生点的阈值管的是「玩家开局能不能走得开」，据点的阈值管的是
    ///   「一小撮人能不能在这活下来」，后者理应更宽松一点；但仍远大于
    ///   一小片碎礁石，不会让村子建在孤岛上。
    /// - **间距 144 格**（= 3 个区块边长）：三条各自独立的理由指向同一
    ///   个量级。
    ///   1. **两座据点不能长到互相咬合。** 一座据点的外廓半径上界是
    ///      [`crate::settlement::MAX_FOOTPRINT_RADIUS`]（由
    ///      [`crate::settlement::MAX_BUILDINGS`] 与建筑间距推出）。
    ///      144 必须大于它的两倍，否则两座都长满时会互相压进对方的
    ///      街区；有多少富余就是有多少荒野隔着——那段荒野要宽到走
    ///      过去得花时间，不会让人误以为是同一座村子。
    ///   2. **锚点所在的区块之间至少隔着一整个区块。** 区块边长 48，
    ///      锚点间距 ≥ 144 意味着两者的区块坐标至少差 2（`⌈144/48⌉ - 1
    ///      = 2`），中间那一整个区块必然是野地。
    ///   3. **世界仍然装得下一片文明，而不是几座孤城。** 本体世界
    ///      3072×2304 格，间距 144 的理论容量约 21×16 ≈ 340 座；扣掉
    ///      水域与不合格陆地，实际落到百来座的量级——与
    ///      `world-history.md` 设想的「几百个聚落」同一个数量级，而
    ///      不是把它压成十几座。
    ///
    ///   这三条都是**几何**理由，不是「调着好看」。要改它，改的是
    ///   上面某一条的前提（据点上界变了、区块边长变了、想要的聚落
    ///   密度变了），不是这个数字本身。
    fn default() -> Self {
        ChronicleParams {
            epochs: 12,
            survey_zone_budget: 4800,
            min_settlement_land_area: 400,
            min_settlement_spacing: 3 * 48,
        }
    }
}

/// 一次历史推演的全部产出：事件日志 + 最终的据点快照。
///
/// 不派生 `PartialEq`：`TerrainTable` 没有 `PartialEq`（它是一张属性
/// 表，比较两张表没有意义）。确定性测试逐字段比对 `events`/`sites`/
/// `next_world_id`，比整体相等更能指出分歧发生在哪一条上。
#[derive(Debug, Clone)]
pub struct WorldChronicle {
    events: Vec<HistoricalEvent>,
    sites: Vec<SettlementSite>,
    next_world_id: u32,
    epochs: u32,
    table: TerrainTable,
}

impl WorldChronicle {
    /// 从种子与地形跑出一部世界史。
    ///
    /// `params.seed` 是随机流的种子；`noise`/`terrain_ids`/`table` 用来
    /// 判断哪些区块能住人。整个函数是纯函数：同一组输入恒产出逐字段
    /// 相同的结果。
    pub fn generate(
        layout: &ZoneLayout,
        noise: &TileableNoise,
        params: &GenParams,
        terrain_ids: &BaseTerrainIds,
        table: &TerrainTable,
        chronicle_params: ChronicleParams,
    ) -> WorldChronicle {
        let candidates =
            survey_habitable_zones(layout, noise, params, terrain_ids, table, chronicle_params);
        let mut run = EpochRun::new(candidates, chronicle_params.epochs, params.seed);
        run.simulate();
        let sites = run.final_sites();
        WorldChronicle {
            next_world_id: run.next_world_id,
            events: run.events,
            sites,
            epochs: chronicle_params.epochs,
            table: table.clone(),
        }
    }

    /// 一部空的世界史——没有任何据点，也不分配任何 `WorldId`。
    ///
    /// 供不关心据点的调用方（绝大多数单元测试、既有验收 demo）显式
    /// 表达「这个世界没有历史」，避免为了构造一个 `WorldChronicle` 而
    /// 被迫准备噪声与地形表。
    pub fn empty(table: TerrainTable) -> WorldChronicle {
        WorldChronicle {
            events: Vec::new(),
            sites: Vec::new(),
            next_world_id: 0,
            epochs: 0,
            table,
        }
    }

    /// 全部历史事件，按发生顺序（纪元升序，同纪元内按候选点光栅序）。
    pub fn events(&self) -> &[HistoricalEvent] {
        &self.events
    }

    /// 全部据点的最终快照，按区块光栅序。
    pub fn sites(&self) -> &[SettlementSite] {
        &self.sites
    }

    /// 推演了多少个纪元。
    pub fn epochs(&self) -> u32 {
        self.epochs
    }

    /// 本次推演分配掉的 `WorldId` 之后的下一个可用号码——调用方必须把
    /// 它写进 `WorldState::next_world_id`，否则游戏内的击杀记录会分配
    /// 到与历史事件相同的号码。见模块文档「为什么编年史不进存档」。
    pub fn next_world_id(&self) -> u32 {
        self.next_world_id
    }

    /// 判断某个区块能不能盖房时用的地形表快照。
    pub fn terrain_table(&self) -> &TerrainTable {
        &self.table
    }

    /// 查这个区块上有没有据点。`sites` 按区块光栅序排好，这里走二分，
    /// 不做线性扫描——本方法在**每个区块首次物化时**都会被调用一次
    /// （见 `crate::surface_store::SurfaceStore::admit`），是流式加载
    /// 路径上的热点。
    pub fn site_in_zone(&self, zone: ZoneCoord) -> Option<&SettlementSite> {
        let key = raster_key(zone);
        self.sites
            .binary_search_by_key(&key, |site| raster_key(site.zone))
            .ok()
            .map(|index| &self.sites[index])
    }
}

/// 区块坐标的光栅序排序键——`sites` 的排序与二分都用它，保证「排序」
/// 与「查找」用的是同一个定义。
fn raster_key(zone: ZoneCoord) -> (i32, i32) {
    (zone.y(), zone.x())
}

/// 一个「能住人」的候选点：区块 + 锚点 + 该区块最大连通陆地面积。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Candidate {
    zone: ZoneCoord,
    anchor: TorusPos,
    land_area: u32,
}

/// 按区块光栅序扫描全世界，收集「能住人」的候选点。
///
/// 两级筛选，与 `ll_game::world::find_spawn_site` 完全同构（也确实
/// 共用第二级的 [`largest_walkable_component`]）：
///
/// 1. 廉价预筛：只采样区块左上角一点，代表点不可通行（多半是水）就
///    跳过，不生成整窗。跑遍全部区块，代价是每区块一次 O(1) 噪声采样。
/// 2. **间距禁区预筛**（本函数独有，`find_spawn_site` 没有对应物）：
///    整个落在某座已接受据点禁区内的区块直接跳过，见
///    [`zone_fully_excluded`]。这一级同样是几次整数比较，且**不计入
///    `budget`**。
/// 3. 通过前两级的才生成整个区块窗口做连通域分析，并计入 `budget`。
///    预算耗尽即停止——本函数不含任何无界循环。
/// 4. 分析出的锚点若离某座已接受据点不足 `min_spacing`，本区块不产出
///    候选点。这一级的代价已经花掉了（要先算出锚点才能量距离），因此
///    它照常计入 `budget`——第 2 级存在的意义正是让绝大多数被间距挡
///    掉的区块根本走不到这里。
///
/// # 间距筛选的确定性（约束 C5）
///
/// 「先来先得」的贪心：区块按光栅序遍历，先被接受的候选点占住自己
/// 周围的禁区，后来者让路。遍历顺序、已接受集合（一个 `Vec`，按接受
/// 顺序追加）、比较运算（整数切比雪夫距离）三者都与任何
/// `HashMap`/`HashSet` 的迭代顺序无关，同一个种子恒产出同一批候选点，
/// 且顺序恒为区块光栅序（[`WorldChronicle::site_in_zone`] 的二分依赖
/// 这一点）。
///
/// # 预算耗尽即停，因此预算必须大到扫得完整个世界
///
/// 预算是一道硬闸门：一旦耗尽，扫描立刻返回，世界剩下的部分连预筛
/// 都不再跑。旧默认值 48 因此把**全部**据点压在光栅序最靠前的那两行
/// 区块里，其余四十六行一座据点都没有——项目所有者实测报告的正是这
/// 个现象。本函数的行为没有变，变的是 [`ChronicleParams::default`] 给
/// 的预算：它现在大于本体世界的区块总数，扫描恒能走完全世界。
///
/// 「预算调大会把启动期的区块窗口生成次数按世界大小线性放大」这条
/// 担心是**成立**的（实测 48 → 4800 是 16ms → 0.8s）。本批次没有靠
/// 「按需派生选址」那条重写路来解——那条路要求历史推演可按区块局部
/// 求值，而当前的纪元推演有跨据点耦合（世界总人口与首邑，见
/// [`EpochRun::simulate`]），是一次真正的重写。0.8 秒是一次性建档
/// 路径上的真实代价，如实记在这里。
///
/// 出生点仍然落在文明范围内：`ll_game::world::find_spawn_site` 用的是
/// 同一个光栅序、同一套判据，它返回的那个区块必然也在本函数的扫描
/// 范围内。
fn survey_habitable_zones(
    layout: &ZoneLayout,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
    table: &TerrainTable,
    chronicle_params: ChronicleParams,
) -> Vec<Candidate> {
    let budget = chronicle_params.survey_zone_budget;
    let min_land_area = chronicle_params.min_settlement_land_area;
    let min_spacing = chronicle_params.min_settlement_spacing;
    let zone_count = layout.zone_count();
    let span = layout.zone_span();
    let tile_size = layout.tile_size();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut fully_inspected = 0usize;

    for zone_y in 0..zone_count.height() as i32 {
        for zone_x in 0..zone_count.width() as i32 {
            if fully_inspected >= budget {
                return candidates;
            }
            let zone = zone_count.wrap(zone_x, zone_y);
            if zone_representative_terrain(noise, params, layout, zone, terrain_ids)
                .blocks_move(table)
            {
                continue;
            }
            if zone_fully_excluded(layout, zone, &candidates, min_spacing) {
                continue;
            }
            fully_inspected += 1;

            let window = generate_zone_window(noise, params, layout, zone, terrain_ids)
                .expect("layout 已在 ZoneLayout::new 中校验过，区块窗口恒能生成");
            let Some(component) =
                largest_walkable_component(&window, layout.local_size(), table, min_land_area)
            else {
                continue;
            };

            // 锚点取分量的 `center`（离窗口正中最近的那一格），不是
            // `start`——据点要往陆地中间放，才装得下整座村子，见
            // `LandComponent::center` 文档。
            let world_x = zone.x() * span as i32 + component.center.x();
            let world_y = zone.y() * span as i32 + component.center.y();
            let anchor = tile_size.wrap(world_x, world_y);
            if candidates
                .iter()
                .any(|taken| tile_size.chebyshev(taken.anchor, anchor) < min_spacing)
            {
                continue;
            }
            candidates.push(Candidate {
                zone,
                anchor,
                land_area: component.area as u32,
            });
        }
    }

    candidates
}

/// 这个区块**整个**落在某座已接受据点的间距禁区里吗——是的话，它不
/// 可能产出任何合格锚点，连通域分析可以整个跳过。
///
/// # 为什么是「整个落在」而不是「中心落在」
///
/// 判据必须是**保守**的：只要区块里还剩一格可能满足间距，就不能跳过
/// 它，否则会漏掉本该存在的据点，而且漏法依赖遍历顺序，说不清。
/// 「整个落在禁区内」是这条保守性的精确表述。
///
/// # 环面上的区间包含判定
///
/// 切比雪夫距离是两个轴向环面距离的较大者，因此「区块整个落在以锚点
/// 为中心、半径 `min_spacing - 1` 的方形禁区内」等价于「两个轴的区块
/// 坐标区间各自整个落在对应的环面区间内」——两个一维问题，见
/// [`interval_within_ring`]。不走「量四个角的距离取最大」那条：环面上
/// 一个区间的距离最大值不一定落在端点上（可能落在内部的对跖点），那
/// 条捷径在世界够大时恰好成立，但它成立的前提没写在任何地方，是一颗
/// 会在有人调小世界尺寸时才炸的雷。
fn zone_fully_excluded(
    layout: &ZoneLayout,
    zone: ZoneCoord,
    taken: &[Candidate],
    min_spacing: u32,
) -> bool {
    if min_spacing == 0 {
        return false;
    }
    let span = layout.zone_span();
    let tile_size = layout.tile_size();
    let radius = min_spacing - 1;
    let origin_x = zone.x() * span as i32;
    let origin_y = zone.y() * span as i32;
    taken.iter().any(|candidate| {
        interval_within_ring(
            tile_size.width(),
            candidate.anchor.x(),
            radius,
            origin_x,
            span,
        ) && interval_within_ring(
            tile_size.height(),
            candidate.anchor.y(),
            radius,
            origin_y,
            span,
        )
    })
}

/// 长度为 `len`、起点为 `lo` 的整数区间，是否整个落在环（周长 `size`）
/// 上以 `center` 为中心、半径 `radius` 的闭区间内。
///
/// 全程整数、无分支依赖浮点：把禁区的起点挪到原点，量一次区块起点相
/// 对它的环面偏移，再看区块尾端有没有越过禁区宽度。
fn interval_within_ring(size: u32, center: i32, radius: u32, lo: i32, len: u32) -> bool {
    let width = 2 * u64::from(radius) + 1;
    if width >= u64::from(size) {
        return true;
    }
    let start = center - radius as i32;
    let offset = u64::from((lo - start).rem_euclid(size as i32) as u32);
    offset + u64::from(len) <= width
}

/// 一个候选点在推演过程中的状态。
#[derive(Debug, Clone, Copy)]
struct SiteState {
    /// 当前这一茬定居点的 `WorldId`；无人居住时为 `None`。
    id: Option<WorldId>,
    population: u32,
    founded_epoch: u32,
    peak_population: u32,
    /// 最近一次被遗弃的纪元——用于「此处现在是废墟」这个最终状态。
    /// 从未被住过时为 `None`。
    last_ruin: Option<RuinRecord>,
}

/// 一处废墟的最终快照所需的三项。
#[derive(Debug, Clone, Copy)]
struct RuinRecord {
    id: WorldId,
    founded_epoch: u32,
    abandoned_epoch: u32,
    peak_population: u32,
}

/// 一次纪元推演的全部可变状态。
struct EpochRun {
    candidates: Vec<Candidate>,
    states: Vec<SiteState>,
    events: Vec<HistoricalEvent>,
    epochs: u32,
    seed: u64,
    next_world_id: u32,
}

/// 建立一座新据点的基础概率分子（分母 [`FOUND_DENOMINATOR`]）。
const FOUND_BASE_NUMERATOR: u32 = 3;
/// 建立概率的分母。
const FOUND_DENOMINATOR: u32 = 64;
/// 每多少格连通陆地给建立概率加一分（上限 [`MAX_FERTILITY_BONUS`]）。
const TILES_PER_FERTILITY_BONUS: u32 = 400;
/// 土地肥沃度对建立概率的加分上限。
const MAX_FERTILITY_BONUS: u32 = 6;
/// 上一纪元每多少世界总人口给建立概率加一分——**这是「历史」而不是
/// 「一串独立掷骰」的地方**：一个已经繁荣的世界会往外溢出移民，新
/// 据点因此更容易出现；一个刚被瘟疫扫过的世界则很久不再有人开拓。
const POPULATION_PER_PRESSURE_BONUS: u32 = 32;
/// 人口压力对建立概率的加分上限。
const MAX_PRESSURE_BONUS: u32 = 6;
/// 新据点的初始人口下限。
const INITIAL_POPULATION_MIN: u32 = 3;
/// 新据点初始人口的随机跨度（`MIN + gen_range(SPREAD)`）。
const INITIAL_POPULATION_SPREAD: u64 = 4;
/// 每纪元人口变动的随机跨度：掷 `[0, SPREAD)` 再减
/// [`GROWTH_BIAS`]，得到 `[-2, +2]`。
const GROWTH_SPREAD: u64 = 5;
/// 见 [`GROWTH_SPREAD`]。
const GROWTH_BIAS: i32 = 2;
/// 上一纪元人口最多的那座据点在本纪元额外获得的增长——首邑聚集效应，
/// 与人口压力一样是跨据点的耦合，不是独立掷骰。
const CAPITAL_GROWTH_BONUS: i32 = 1;
/// 每个居民需要多少格连通陆地养活；人口超过 `土地 / 本值` 之后增长
/// 额外减一（承载力）。
const TILES_PER_RESIDENT: u32 = 120;
/// 每多少居民对应一栋建筑。
const RESIDENTS_PER_BUILDING: u32 = 2;
/// 一处废墟按历史峰值人口每多少人留下一栋残破建筑。
const PEAK_RESIDENTS_PER_RUIN_BUILDING: u32 = 3;

impl EpochRun {
    fn new(candidates: Vec<Candidate>, epochs: u32, seed: u64) -> EpochRun {
        let states = vec![
            SiteState {
                id: None,
                population: 0,
                founded_epoch: 0,
                peak_population: 0,
                last_ruin: None,
            };
            candidates.len()
        ];
        EpochRun {
            candidates,
            states,
            events: Vec::new(),
            epochs,
            seed,
            next_world_id: 0,
        }
    }

    /// 逐纪元推演。每个纪元内部按候选点光栅序处理，纪元末尾重算两项
    /// 跨据点聚合量（世界总人口、首邑）供**下一个**纪元使用——聚合在
    /// 纪元边界上计算，本纪元内部读到的恒是上一纪元的定局，因此处理
    /// 顺序不影响结果。
    fn simulate(&mut self) {
        let mut world_population = 0u32;
        let mut capital: Option<usize> = None;

        for epoch in 0..self.epochs {
            for index in 0..self.candidates.len() {
                let mut rng = DetRng::for_entity(
                    self.seed,
                    CHRONICLE_STREAM_ID,
                    index as u64 * u64::from(self.epochs) + u64::from(epoch),
                );
                if self.states[index].population == 0 {
                    self.try_found(index, epoch, world_population, &mut rng);
                } else {
                    self.advance_settled(index, epoch, capital == Some(index), &mut rng);
                }
            }
            world_population = self.states.iter().map(|state| state.population).sum();
            capital = self.pick_capital();
        }
    }

    /// 上一纪元人口最多的候选点下标；并列时取光栅序最先的那个（`>`
    /// 而非 `>=`，不依赖任何迭代顺序，约束 C5）。全世界无人时为 `None`。
    fn pick_capital(&self) -> Option<usize> {
        let mut best: Option<(usize, u32)> = None;
        for (index, state) in self.states.iter().enumerate() {
            if state.population == 0 {
                continue;
            }
            match best {
                Some((_, top)) if state.population <= top => {}
                _ => best = Some((index, state.population)),
            }
        }
        best.map(|(index, _)| index)
    }

    /// 一处空地在本纪元有没有被拓荒。
    fn try_found(&mut self, index: usize, epoch: u32, world_population: u32, rng: &mut DetRng) {
        let candidate = self.candidates[index];
        let fertility = (candidate.land_area / TILES_PER_FERTILITY_BONUS).min(MAX_FERTILITY_BONUS);
        let pressure = (world_population / POPULATION_PER_PRESSURE_BONUS).min(MAX_PRESSURE_BONUS);
        if !rng.chance(
            FOUND_BASE_NUMERATOR + fertility + pressure,
            FOUND_DENOMINATOR,
        ) {
            return;
        }

        let population = INITIAL_POPULATION_MIN + rng.gen_range(INITIAL_POPULATION_SPREAD) as u32;
        let site_id = WorldId::next(&mut self.next_world_id);
        let event_id = WorldId::next(&mut self.next_world_id);
        self.states[index] = SiteState {
            id: Some(site_id),
            population,
            founded_epoch: epoch,
            peak_population: population,
            last_ruin: self.states[index].last_ruin,
        };
        self.events.push(HistoricalEvent {
            id: event_id,
            at: epoch_tick(epoch, self.epochs),
            location: candidate.anchor,
            kind: HistoricalEventKind::SettlementFounded(SettlementFoundedRecord {
                site: site_id,
                epoch,
                initial_population: population,
                land_area: candidate.land_area,
            }),
        });
    }

    /// 一座有人住的据点在本纪元的兴衰。人口归零即被遗弃，留下废墟。
    fn advance_settled(&mut self, index: usize, epoch: u32, is_capital: bool, rng: &mut DetRng) {
        let candidate = self.candidates[index];
        let state = self.states[index];
        let capacity = candidate.land_area / TILES_PER_RESIDENT;

        let mut delta = rng.gen_range(GROWTH_SPREAD) as i32 - GROWTH_BIAS;
        if is_capital {
            delta += CAPITAL_GROWTH_BONUS;
        }
        if state.population > capacity {
            delta -= 1;
        }

        let population = state.population.saturating_add_signed(delta);
        let peak = state.peak_population.max(population);
        if population > 0 {
            self.states[index] = SiteState {
                population,
                peak_population: peak,
                ..state
            };
            return;
        }

        let site_id = state.id.expect("人口非零的据点必然在建立时分配过 WorldId");
        let event_id = WorldId::next(&mut self.next_world_id);
        self.states[index] = SiteState {
            id: None,
            population: 0,
            founded_epoch: state.founded_epoch,
            peak_population: 0,
            last_ruin: Some(RuinRecord {
                id: site_id,
                founded_epoch: state.founded_epoch,
                abandoned_epoch: epoch,
                peak_population: state.peak_population,
            }),
        };
        self.events.push(HistoricalEvent {
            id: event_id,
            at: epoch_tick(epoch, self.epochs),
            location: candidate.anchor,
            kind: HistoricalEventKind::SettlementAbandoned(SettlementAbandonedRecord {
                site: site_id,
                epoch,
                peak_population: state.peak_population,
                epochs_inhabited: epoch - state.founded_epoch,
            }),
        });
    }

    /// 推演结束后的最终快照：仍有人住的是村子，最近一次被遗弃且此后
    /// 无人重建的是废墟，从未被住过的不产出任何东西。
    ///
    /// 结果按区块光栅序——`candidates` 本就是按这个顺序收集的，这里
    /// 只是保持它，供 [`WorldChronicle::site_in_zone`] 二分。
    fn final_sites(&self) -> Vec<SettlementSite> {
        let mut sites = Vec::new();
        for (index, state) in self.states.iter().enumerate() {
            let candidate = self.candidates[index];
            if let Some(id) = state.id {
                sites.push(SettlementSite {
                    id,
                    zone: candidate.zone,
                    anchor: candidate.anchor,
                    status: SettlementStatus::Inhabited,
                    founded_epoch: state.founded_epoch,
                    abandoned_epoch: None,
                    population: state.population,
                    peak_population: state.peak_population,
                    building_count: (1 + state.population / RESIDENTS_PER_BUILDING)
                        .min(MAX_BUILDINGS),
                });
            } else if let Some(ruin) = state.last_ruin {
                sites.push(SettlementSite {
                    id: ruin.id,
                    zone: candidate.zone,
                    anchor: candidate.anchor,
                    status: SettlementStatus::Ruined,
                    founded_epoch: ruin.founded_epoch,
                    abandoned_epoch: Some(ruin.abandoned_epoch),
                    population: 0,
                    peak_population: ruin.peak_population,
                    building_count: (ruin.peak_population / PEAK_RESIDENTS_PER_RUIN_BUILDING)
                        .clamp(1, MAX_BUILDINGS),
                });
            }
        }
        sites
    }
}

/// 第 `epoch` 个纪元（共 `epochs` 个）对应的世界时刻。
///
/// 历史发生在**游戏开始之前**，因此全部是负数 tick：最后一个纪元距
/// 开局 [`YEARS_PER_EPOCH`] 年，第 0 个纪元距开局
/// `epochs * YEARS_PER_EPOCH` 年。`Tick` 内部就是一个 `i64`，不排斥
/// 负值（见 `WorldState::advance` 文档「`ticks` 允许为负」）。
fn epoch_tick(epoch: u32, epochs: u32) -> Tick {
    Tick(-i64::from(epochs - epoch) * TICKS_PER_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::base_terrain_fixture;
    use ll_core::torus::TorusSize;

    /// 测试用世界：16×16 个区块、区块边长 48（768×768 格）。比本体默认
    /// 的 64×48 小得多，但足够让噪声产出成片的陆地与水域。
    ///
    /// **为什么不能更小。** 默认最小间距是 144 格
    /// （[`ChronicleParams::min_settlement_spacing`]），世界每个轴至少要
    /// 装得下几个间距，间距筛选才是在「筛」而不是在「只留一座」——
    /// 768 / 144 ≈ 5.3，两个轴合起来的理论容量二十几座，与本体世界
    /// 两百多座是同一个性质的小号样本。此前这里是 8×8（384 格），
    /// 每轴只有 2.67 个间距，间距一落地就会把候选点压到个位数，
    /// `不同种子产出不同的编年史` 这类比规模的断言会退化成掷硬币。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(16, 16).expect("16x16 合法");
        ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
    }

    fn chronicle_for(seed: u64) -> WorldChronicle {
        chronicle_with(seed, ChronicleParams::default())
    }

    fn chronicle_with(seed: u64, chronicle_params: ChronicleParams) -> WorldChronicle {
        let layout = test_layout();
        let params = GenParams {
            seed,
            ..GenParams::default()
        };
        let noise = crate::generate::build_zone_noise(&layout, &params).expect("布局合法");
        let (ids, table) = base_terrain_fixture();
        WorldChronicle::generate(&layout, &noise, &params, &ids, &table, chronicle_params)
    }

    /// 一部编年史里挨得最近的两座据点相距多少格（环面切比雪夫）。
    /// 少于两座时返回 `None`。
    fn closest_pair_distance(chronicle: &WorldChronicle) -> Option<u32> {
        let tile_size = test_layout().tile_size();
        let sites = chronicle.sites();
        let mut closest: Option<u32> = None;
        for (index, a) in sites.iter().enumerate() {
            for b in sites.iter().skip(index + 1) {
                let distance = tile_size.chebyshev(a.anchor, b.anchor);
                closest = Some(closest.map_or(distance, |best: u32| best.min(distance)));
            }
        }
        closest
    }

    #[test]
    fn 同一种子两次独立生成的编年史逐字段相同() {
        // Arrange & Act
        let first = chronicle_for(0xC0FF_EE12);
        let second = chronicle_for(0xC0FF_EE12);

        // Assert：逐条事件、逐座据点、逐个字段比对，不只比一个摘要。
        assert_eq!(first.epochs(), second.epochs());
        assert_eq!(first.next_world_id(), second.next_world_id());
        assert_eq!(first.events().len(), second.events().len());
        for (a, b) in first.events().iter().zip(second.events()) {
            assert_eq!(a, b, "同一种子的历史事件出现分歧");
        }
        assert_eq!(first.sites().len(), second.sites().len());
        for (a, b) in first.sites().iter().zip(second.sites()) {
            assert_eq!(a, b, "同一种子的据点快照出现分歧");
        }
    }

    #[test]
    fn 编年史真的产出了据点与事件() {
        // Arrange & Act
        let chronicle = chronicle_for(0xC0FF_EE12);

        // Assert
        assert!(
            !chronicle.events().is_empty(),
            "300 年里一座据点都没建立，说明推演没跑起来"
        );
        assert!(
            !chronicle.sites().is_empty(),
            "推演结束时世界上一座据点都不剩"
        );
    }

    #[test]
    fn 据点快照按区块光栅序排列且可二分查到() {
        // Arrange
        let chronicle = chronicle_for(0xC0FF_EE12);

        // Act & Assert
        let mut previous: Option<(i32, i32)> = None;
        for site in chronicle.sites() {
            let key = raster_key(site.zone);
            if let Some(prev) = previous {
                assert!(prev < key, "据点快照没有按区块光栅序排列");
            }
            previous = Some(key);
            assert_eq!(
                chronicle.site_in_zone(site.zone).map(|found| found.id),
                Some(site.id),
                "二分查不到自己排进去的据点"
            );
        }
    }

    #[test]
    fn 不同种子产出不同的编年史() {
        // Arrange & Act
        let first = chronicle_for(1);
        let second = chronicle_for(2);

        // Assert
        assert_ne!(
            (first.events().len(), first.sites().len()),
            (second.events().len(), second.sites().len()),
            "两个种子的世界史规模完全一致，随机流可能没有真的用上种子"
        );
    }

    #[test]
    fn 每座据点的建筑数不超过上限() {
        // Arrange
        let chronicle = chronicle_for(0xC0FF_EE12);

        // Act & Assert
        for site in chronicle.sites() {
            assert!(
                site.building_count >= 1 && site.building_count <= MAX_BUILDINGS,
                "据点 {:?} 的建筑数 {} 越界",
                site.id,
                site.building_count
            );
        }
    }

    #[test]
    fn 历史事件的时刻全部早于开局() {
        // Arrange
        let chronicle = chronicle_for(0xC0FF_EE12);

        // Act & Assert
        for event in chronicle.events() {
            assert!(event.at.0 < 0, "历史事件不该发生在开局之后：{event:?}");
        }
    }

    #[test]
    fn 废墟的建立纪元早于被遗弃的纪元() {
        // Arrange
        let chronicle = chronicle_for(0xC0FF_EE12);

        // Act & Assert
        for site in chronicle.sites() {
            if let Some(abandoned) = site.abandoned_epoch {
                assert_eq!(site.status, SettlementStatus::Ruined);
                assert!(
                    site.founded_epoch <= abandoned,
                    "据点 {:?} 在建立之前就被遗弃了",
                    site.id
                );
            }
        }
    }

    #[test]
    fn 任意两座据点之间不短于设定的最小间距() {
        // Arrange
        let spacing = ChronicleParams::default().min_settlement_spacing;

        // Act & Assert：多个种子，避免只在某一张地形上碰巧成立。
        for seed in [1u64, 2, 0xC0FF_EE12] {
            let chronicle = chronicle_for(seed);
            assert!(
                chronicle.sites().len() >= 2,
                "种子 {seed} 只产出了 {} 座据点，这条断言测不到间距",
                chronicle.sites().len()
            );
            let closest = closest_pair_distance(&chronicle).expect("至少两座");
            assert!(
                closest >= spacing,
                "种子 {seed} 有两座据点只隔 {closest} 格，低于最小间距 {spacing}"
            );
        }
    }

    /// 间距规则真的在筛：关掉它，同一张地形上立刻出现挨在一起的据点。
    /// 没有这条对照，上面那条断言可能只是「这张地形本来就稀疏」。
    #[test]
    fn 关掉间距筛选后同一张地形上立刻出现挨在一起的据点() {
        // Arrange
        let spacing = ChronicleParams::default().min_settlement_spacing;
        let without = ChronicleParams {
            min_settlement_spacing: 0,
            ..ChronicleParams::default()
        };

        // Act
        let filtered = chronicle_for(0xC0FF_EE12);
        let unfiltered = chronicle_with(0xC0FF_EE12, without);

        // Assert
        assert!(
            unfiltered.sites().len() > filtered.sites().len(),
            "关掉间距筛选之后据点数没有变多，筛选没在起作用"
        );
        let closest = closest_pair_distance(&unfiltered).expect("至少两座");
        assert!(
            closest < spacing,
            "不筛的世界里最近两座据点也隔了 {closest} 格，这张地形本来就稀疏，对照不成立"
        );
    }

    /// 间距筛选不得依赖任何迭代顺序（约束 C5）——同一种子跑两次，
    /// 被筛掉与被留下的必须完全一致。上面那条「逐字段相同」覆盖的是
    /// 整部编年史，这条把镜头对准候选点集合本身。
    #[test]
    fn 间距筛选两次给出完全相同的候选点集合() {
        // Arrange
        let layout = test_layout();
        let params = GenParams {
            seed: 0xC0FF_EE12,
            ..GenParams::default()
        };
        let noise = crate::generate::build_zone_noise(&layout, &params).expect("布局合法");
        let (ids, table) = base_terrain_fixture();
        let defaults = ChronicleParams::default();
        let survey = || survey_habitable_zones(&layout, &noise, &params, &ids, &table, defaults);

        // Act
        let first = survey();
        let second = survey();

        // Assert
        assert_eq!(first, second);
        assert!(first.len() >= 2, "候选点太少，这条断言测不到什么");
    }

    #[test]
    fn 环面区间包含判定在跨越接缝时仍然正确() {
        // Arrange：周长 100 的环，中心 5、半径 10 的禁区是 [95, 15]，
        // 跨越接缝。
        let size = 100u32;

        // Act & Assert
        assert!(
            interval_within_ring(size, 5, 10, 96, 4),
            "[96,99] 应在禁区内"
        );
        assert!(
            interval_within_ring(size, 5, 10, 0, 16),
            "[0,15] 应在禁区内"
        );
        assert!(
            !interval_within_ring(size, 5, 10, 0, 17),
            "[0,16] 越过了禁区右端"
        );
        assert!(
            !interval_within_ring(size, 5, 10, 94, 4),
            "[94,97] 的 94 在禁区之外"
        );
        // 禁区宽度覆盖整个环时恒为真。
        assert!(interval_within_ring(size, 5, 60, 0, 100));
    }

    #[test]
    fn 整个落在禁区内的区块不做连通域分析也不计预算() {
        // Arrange：把预算压到 1。若禁区跳过没有生效，第二个可住区块
        // 会耗尽预算、扫描立刻停止，据点数会掉到个位数。
        let tight = ChronicleParams {
            survey_zone_budget: 1,
            ..ChronicleParams::default()
        };
        let loose = ChronicleParams::default();

        // Act
        let with_tight_budget = chronicle_with(0xC0FF_EE12, tight);
        let with_loose_budget = chronicle_with(0xC0FF_EE12, loose);

        // Assert：预算 1 只允许一次完整分析，因此最多留下一座据点的
        // 候选；而放开预算能留下多座——两者不等，正说明预算确实只被
        // 「真的做了分析」的区块消耗，禁区内的区块没有偷偷计数。
        assert!(with_tight_budget.sites().len() <= 1);
        assert!(with_loose_budget.sites().len() > with_tight_budget.sites().len());
    }

    #[test]
    fn 空编年史不分配任何世界id() {
        // Arrange
        let (_, table) = base_terrain_fixture();

        // Act
        let chronicle = WorldChronicle::empty(table);

        // Assert
        assert_eq!(chronicle.next_world_id(), 0);
        assert!(chronicle.sites().is_empty());
        assert!(chronicle.events().is_empty());
    }
}
