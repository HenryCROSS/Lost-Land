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
use ll_core::torus::{TorusPos, TorusSize};

use crate::culture::{CultureKind, CultureTable};
use crate::faction::{FactionTable, seed_factions};
use crate::generate::{
    GenParams, generate_zone_window, terrain_at_tile, zone_representative_terrain,
};
use crate::history::{
    HistoricalEvent, HistoricalEventKind, SettlementAbandonedRecord, SettlementConqueredRecord,
    SettlementDemise, SettlementFoundedRecord,
};
use crate::land::largest_walkable_component;
use crate::noise::TileableNoise;
use crate::resource::{
    ResourceContext, ResourceKind, ResourceSurvey, ResourceTable, survey_resources,
};
use crate::settlement::{MAX_BUILDINGS, SITE_RESOURCE_SLOTS, SettlementSite, SettlementStatus};
use crate::space::ZoneCoord;
use crate::terrain::{BaseTerrainIds, TerrainKind, TerrainTable};
use crate::zone::ZoneLayout;

/// 历史推演所用的随机流编号——与
/// [`crate::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]（建筑铺法）
/// 分开，理由见后者文档。
pub const CHRONICLE_STREAM_ID: u64 = 0x0043_4852_4F4E_0001;

/// 战争判定所用的随机流编号——与 [`CHRONICLE_STREAM_ID`]（一座据点
/// 自己的兴衰）分开。
///
/// # 为什么必须是另一条流，而不是同一条流上多掷一次
///
/// 战争是**跨据点**的：一次判定同时决定两座据点的命运。把它掷在
/// 「这座据点第几个纪元」那条流上，等于让攻方的一次内政掷骰与它的
/// 对外战争共用取值序列——改动增长模型（多掷一次骰子）会连带改掉
/// 全世界的战争史，两件本该正交的事就此耦合。同一条理由已经让建筑
/// 铺法（[`crate::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]）与历史
/// 推演分家。
pub const CHRONICLE_WAR_STREAM_ID: u64 = 0x0043_4852_5741_0001;

/// 文化抽取所用的随机流基编号——与历史推演
/// （[`CHRONICLE_STREAM_ID`]）、战争（[`CHRONICLE_WAR_STREAM_ID`]）
/// 分开，三者互不干扰：给某座据点换一份文化不会连带改掉它的人口曲线
/// 或战争掷骰（约束 C3）。
pub const CHRONICLE_CULTURE_STREAM_ID: u64 = 0x0043_4852_4355_0001;

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
    /// 勘察一处候选点的领地资源时，每隔多少格采一个样。
    ///
    /// 领地是一片 `min_settlement_spacing` 见方的地
    /// （[`territory_radius`]），逐格勘察要为两万格各算一次四层倍频
    /// 噪声；采样把它压到三百多次，代价是估计量的方差——而下游要的
    /// 本来就是「这地方富不富」这个量级的判断，见
    /// [`crate::resource::survey_resources`] 文档。
    ///
    /// 取 8：144 / 8 = 18，每轴十八九个采样点，够让「这片地是山还是
    /// 沼泽」这条差别稳定地体现出来，又不至于让勘察成为建档路径上的
    /// 新瓶颈（实测见 [`ChronicleParams::default`]）。必须为正，
    /// 否则 [`crate::resource::survey_resources`] 会 panic。
    pub resource_sample_stride: i32,
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
            resource_sample_stride: 8,
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
    /// 「哪个区块被哪座据点覆盖到」的倒排索引：`(区块光栅序键, sites
    /// 下标)`，按键升序排好，供 [`Self::sites_touching_zone`] 二分。
    ///
    /// # 为什么需要它，为什么不现算
    ///
    /// 据点可以横跨区块（见 [`crate::settlement`] 模块文档），因此
    /// 「这个区块上有什么要铺」不再等于「哪座据点的锚点在这个区块」。
    /// 每个区块首次物化时都要问一次这个问题（`SurfaceStore::admit`，
    /// 流式加载路径上的热点），现算意味着每次都要遍历全部据点、把每座
    /// 的每栋建筑都换算一遍坐标——本体世界两百多座据点，那是每加载一个
    /// 区块几千次整数运算。建档时算一次、此后二分，是把这笔账付在
    /// 一次性路径上。
    ///
    /// **一座据点在这里可能出现多次**（它覆盖到几个区块就有几条），
    /// 这正是它与 `sites`（一座一条）的区别。
    zone_index: Vec<((i32, i32), u32)>,
    next_world_id: u32,
    epochs: u32,
    table: TerrainTable,
    /// 推演时用的那张文化表的快照——[`crate::settlement::SettlementSite::culture`]
    /// 只是一个索引，把它翻译成「用哪种墙」需要这张表，而铺设发生在
    /// `SurfaceStore` 里、离世界生成很远。与 `table`（地形表）**逐字
    /// 同一条取舍**：跟着编年史一起走，调用方就不需要再从别处凑一张
    /// 可能已经对不上号的表。
    cultures: CultureTable,
    /// 从这部编年史的**占领链**折叠出来的势力表，见 [`crate::faction`]。
    ///
    /// 存在这里而不是让调用方自己折叠：势力号从本结构的
    /// [`Self::next_world_id`] 计数器分配，折叠必须发生在计数器还在手上
    /// 的时候。折叠本身是 `events` 的纯函数（无掷骰、无哈希容器），
    /// 算法全在 [`crate::faction::seed_factions`] 里——`chronicle.rs`
    /// 只做这一处接线。
    factions: FactionTable,
}

/// 一次历史推演要读的全部**世界形状**输入——除可调参数
/// （[`ChronicleParams`]）之外的那一整组。
///
/// 打包成结构体而不是继续往参数表上加，理由与
/// [`crate::settlement::StampContext`]、`ll_mod::roster::MaterializeContext`
/// 逐字相同：这七项**恒一起出现**，散着传只会让调用点更容易漏配，
/// 也会撞上 `clippy::too_many_arguments`（文化表落地正是压垮那条闸门
/// 的第八个参数）。
pub struct ChronicleInput<'a> {
    /// 区块布局。
    pub layout: &'a ZoneLayout,
    /// 地形噪声。
    pub noise: &'a TileableNoise,
    /// 世界生成参数——`params.seed` 同时是全部随机流的种子。
    pub params: &'a GenParams,
    /// 本体基础地形的内容索引。
    pub terrain_ids: &'a BaseTerrainIds,
    /// 地形属性表——判断哪些区块能住人。
    pub terrain_table: &'a TerrainTable,
    /// 当前会话注册的资源种类表（[`crate::resource`]），决定每处候选点
    /// 周边有什么、因此决定「这里值不值得建城」与「这里能养活多少人」。
    pub resources: &'a ResourceTable,
    /// 当前会话注册的文化表（[`crate::culture`]），决定每座据点信什么、
    /// 用什么盖房、跟谁不对付。
    pub cultures: &'a CultureTable,
}

impl WorldChronicle {
    /// 从种子与地形跑出一部世界史。
    ///
    /// `params.seed` 是随机流的种子；`noise`/`terrain_ids`/`table` 用来
    /// 判断哪些区块能住人；`resources` 是当前会话注册的资源种类表
    /// （[`crate::resource`]），决定每处候选点周边有什么、因此决定
    /// 「这里值不值得建城」与「这里能养活多少人」。整个函数是纯函数：
    /// 同一组输入恒产出逐字段相同的结果。
    ///
    /// 传一张**空**的资源表或空的文化表都是合法的：那分别等于「这个
    /// 世界没有资源这一层」与「没有文化这一层」——选址与承载力退回到
    /// 只看陆地面积、建材退回引擎默认、战争敌意恒为 0，都不会 panic，
    /// 也不会静默变成别的行为，见本模块测试 `空资源表下仍然产出据点`
    /// 与 `全表敌意为零时战争结果与空文化表逐位相同`。
    pub fn generate(
        input: &ChronicleInput<'_>,
        chronicle_params: ChronicleParams,
    ) -> WorldChronicle {
        let ChronicleInput {
            layout,
            noise,
            params,
            terrain_ids,
            terrain_table: table,
            resources,
            cultures,
        } = *input;
        let candidates = survey_habitable_zones(
            layout,
            noise,
            params,
            terrain_ids,
            table,
            resources,
            chronicle_params,
        );
        let mut run = EpochRun::new(
            candidates,
            chronicle_params.epochs,
            params.seed,
            layout.tile_size(),
            NeighbourRanges {
                war: war_range(chronicle_params),
                culture: culture_neighbor_range(chronicle_params),
            },
            resources.clone(),
            cultures.clone(),
        );
        run.simulate();
        let sites = run.final_sites();
        let zone_index = build_zone_index(&sites, layout);
        // 势力播种：把刚推演出来的占领链折叠成势力表。必须排在这里
        // ——`run.next_world_id` 还在手上，势力号从同一个计数器继续
        // 分配（`crate::faction` 模块文档「`WorldId` 从哪来」）。
        let mut next_world_id = run.next_world_id;
        let factions = seed_factions(&run.events, &mut next_world_id);
        WorldChronicle {
            next_world_id,
            events: run.events,
            sites,
            zone_index,
            epochs: chronicle_params.epochs,
            table: table.clone(),
            cultures: cultures.clone(),
            factions,
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
            zone_index: Vec::new(),
            next_world_id: 0,
            epochs: 0,
            table,
            // 没有据点就没有文化可查；空表的语义见 `CultureTable::new`。
            cultures: CultureTable::new(),
            // 没有据点就没有势力可立，见 `FactionTable::new`。
            factions: FactionTable::new(),
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

    /// 从占领链折叠出来的势力表——**「谁统治谁」这层关系的物化**，见
    /// [`crate::faction`]。
    ///
    /// 调用方（`ll_game::world::build_world`）要把它搬进
    /// [`crate::state::WorldState::factions`]：与整部编年史不同，势力
    /// **进存档**（项目所有者裁定「被占领后肯定会有变化的」），因此读档
    /// 时不重跑这一步，直接读存档里那一份。
    pub fn factions(&self) -> &FactionTable {
        &self.factions
    }

    /// 判断某个区块能不能盖房时用的地形表快照。
    pub fn terrain_table(&self) -> &TerrainTable {
        &self.table
    }

    /// 推演时用的那张文化表快照——铺设路径要靠它把
    /// [`crate::settlement::SettlementSite::culture`] 翻译成建材，见
    /// [`crate::settlement::StampContext::cultures`]。
    pub fn culture_table(&self) -> &CultureTable {
        &self.cultures
    }

    /// 查**锚点**落在这个区块的那座据点。`sites` 按区块光栅序排好，
    /// 这里走二分，不做线性扫描。
    ///
    /// # 这不是「区块上要铺什么」的问题
    ///
    /// 据点可以横跨区块（见 [`crate::settlement`] 模块文档），一个区块
    /// 上可能有邻居据点的半条街，也可能自己没有锚点却要铺东西。铺设
    /// 路径要问的是 [`Self::sites_touching_zone`]，不是本方法。本方法
    /// 回答的是「这个区块是谁的中心」，供只关心据点归属的消费方
    /// （传说浏览、测试）使用。
    pub fn site_in_zone(&self, zone: ZoneCoord) -> Option<&SettlementSite> {
        let key = raster_key(zone);
        self.sites
            .binary_search_by_key(&key, |site| raster_key(site.zone))
            .ok()
            .map(|index| &self.sites[index])
    }

    /// 有哪些据点的建筑覆盖到这个区块——按据点的区块光栅序返回。
    ///
    /// 这是**铺设路径**要问的那个问题：每个区块首次物化时调用一次
    /// （见 `crate::surface_store::SurfaceStore::admit`），是流式加载
    /// 路径上的热点，因此走的是一次二分 + 一段连续切片，不遍历据点表。
    ///
    /// 返回值可能为空（绝大多数区块），也可能有多座（一个区块同时被
    /// 两座据点的边缘扫到——最小间距保证它们的建筑不会重叠，但没有
    /// 也不需要保证它们的**外廓**不共用区块）。
    pub fn sites_touching_zone(&self, zone: ZoneCoord) -> impl Iterator<Item = &SettlementSite> {
        let key = raster_key(zone);
        let start = self.zone_index.partition_point(|(at, _)| *at < key);
        self.zone_index[start..]
            .iter()
            .take_while(move |(at, _)| *at == key)
            .map(|(_, position)| &self.sites[*position as usize])
    }
}

/// 建一份「区块 → 覆盖到该区块的据点」倒排索引，见
/// [`WorldChronicle::zone_index`] 字段文档。
///
/// 排序键是区块光栅序（[`raster_key`]），并列时按 `sites` 下标——
/// 也就是据点自身的光栅序，因此同一个区块上的多座据点恒按同一个顺序
/// 铺（约束 C5）。全程只用 `Vec` + `sort`，不碰任何哈希容器。
fn build_zone_index(sites: &[SettlementSite], layout: &ZoneLayout) -> Vec<((i32, i32), u32)> {
    let mut index = Vec::new();
    for (position, site) in sites.iter().enumerate() {
        for zone in crate::settlement::footprint_zones(site, layout) {
            index.push((raster_key(zone), position as u32));
        }
    }
    index.sort_unstable();
    index
}

/// 区块坐标的光栅序排序键——`sites` 的排序与二分都用它，保证「排序」
/// 与「查找」用的是同一个定义。
fn raster_key(zone: ZoneCoord) -> (i32, i32) {
    (zone.y(), zone.x())
}

/// 一个「能住人」的候选点：区块 + 锚点 + 两份规模判据。
///
/// **两份陆地面积是两回事，不要合并。** `land_area` 是**锚点所在那一个
/// 区块窗口**里最大连通可行走分量的格数——它回答的是「这里是不是一块
/// 连得开的实地」，是选址的门槛判据，也是
/// [`SettlementFoundedRecord::land_area`] 这个既有字段的语义。
/// `survey` 里那份 [`ResourceSurvey::land_area`] 是**整片领地**
/// （[`territory_radius`] 见方）的采样估计——它回答的是「这片地能养活
/// 多少人」，是承载力判据。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    zone: ZoneCoord,
    anchor: TorusPos,
    land_area: u32,
    /// 锚点那一格的**基础地形**（噪声算出来的那一层，还没铺任何房子）。
    ///
    /// 只为文化抽取的「地形」那一项存在（`EpochRun::culture_weights`）
    /// ——项目所有者定的四条依据里的第二条。存下来而不是每次现算：
    /// 一处候选点在整段推演里可能被反复拓荒，而 `terrain_at_tile` 是
    /// 四层倍频噪声，勘察那一趟本来就已经把这一格算过一次了。
    anchor_terrain: TerrainKind,
    /// 领地资源勘察结果，见 [`crate::resource::survey_resources`]。
    survey: ResourceSurvey,
}

/// 一处据点的**领地**半径（格）：到最近的可能邻居的一半路。
///
/// 由 [`ChronicleParams::min_settlement_spacing`] 推出，不是一个可以
/// 独立调的数值——两座据点至少隔着 `min_settlement_spacing`，各自往外
/// 圈一半就恰好不重叠，「这片地归谁」因此没有歧义。间距为 0（不筛）
/// 时退回一个区块边长的一半，让领地至少是一个可算的东西。
fn territory_radius(params: ChronicleParams, zone_span: u32) -> i32 {
    let from_spacing = params.min_settlement_spacing / 2;
    let fallback = zone_span / 2;
    from_spacing.max(fallback) as i32
}

/// 一座据点能打到多远（格，环面切比雪夫）。
///
/// 同样由最小间距推出：邻居至少在 `min_settlement_spacing` 之外，
/// 本值取它的 [`WAR_RANGE_IN_SPACINGS`] 倍，也就是「隔着两三片荒野的
/// 邻居仍然够得着，隔着半个大陆的够不着」。写成派生量而不是一个独立
/// 常量，是为了让「世界变稀疏时战争也跟着变稀疏」自动成立。
fn war_range(params: ChronicleParams) -> u32 {
    params
        .min_settlement_spacing
        .saturating_mul(WAR_RANGE_IN_SPACINGS)
}

/// 抽文化时「邻近据点」算到多远（格，环面切比雪夫）。
///
/// 与 [`war_range`] 同一种推法（由最小间距推出，不是一个可以独立调的
/// 数值），但**刻意用一个更小的倍数**：打得到的地方不等于文化会扩散
/// 过去的地方。取 [`CULTURE_NEIGHBOR_RANGE_IN_SPACINGS`]（2）意味着
/// 「隔着最多一座据点的距离」，本体默认间距下是 288 格——走过去要花
/// 时间，但仍在同一片地方。
fn culture_neighbor_range(params: ChronicleParams) -> u32 {
    params
        .min_settlement_spacing
        .saturating_mul(CULTURE_NEIGHBOR_RANGE_IN_SPACINGS)
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
    resources: &ResourceTable,
    chronicle_params: ChronicleParams,
) -> Vec<Candidate> {
    let budget = chronicle_params.survey_zone_budget;
    let min_land_area = chronicle_params.min_settlement_land_area;
    let min_spacing = chronicle_params.min_settlement_spacing;
    let zone_count = layout.zone_count();
    let span = layout.zone_span();
    let tile_size = layout.tile_size();
    let territory = territory_radius(chronicle_params, span);
    let resource_ctx = ResourceContext {
        seed: params.seed,
        noise,
        params,
        terrain_ids,
        terrain_table: table,
        resources,
        tile_size,
    };
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
            // 领地资源勘察只对**已经通过全部筛选**的锚点做一次——
            // 它是本函数里第二贵的一步（三百多次噪声采样），绝不能
            // 落在每个被间距挡掉的区块上。
            let survey = survey_resources(
                &resource_ctx,
                anchor,
                territory,
                chronicle_params.resource_sample_stride,
            );
            candidates.push(Candidate {
                zone,
                anchor,
                land_area: component.area as u32,
                anchor_terrain: terrain_at_tile(noise, params, anchor, terrain_ids),
                survey,
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
    /// 这一茬定居点累计从可枯竭资源里采走了多少——每纪元按当时的人口
    /// 累加（人越多挖得越快）。与储量
    /// （[`EpochRun::exhaustible_reserve`]）比较，越过就是采光了。
    ///
    /// **随「这一茬」清零**：重新拓荒是另一批人重新开采，而剩下的储量
    /// 由勘察结果决定、不随时间恢复——这条在
    /// [`EpochRun::try_found`] 里体现为把它重置为上一茬的值而不是 0，
    /// 见那里的注释。
    extracted: u32,
    /// 这一茬定居点赖以立足的可枯竭资源已经采光了吗。
    depleted: bool,
    /// 最近一次被遗弃的纪元——用于「此处现在是废墟」这个最终状态。
    /// 从未被住过时为 `None`。
    last_ruin: Option<RuinRecord>,
    /// 这一茬人信的那份文化，见 [`crate::culture`]。
    ///
    /// **随「这一茬」重抽，不随地块继承**：与 `extracted`/`depleted`
    /// （矿是这片地的属性）方向相反——文化是**这批人**的属性，一座
    /// 矿城废弃之后在原地重新扎营的可能是另一族另一种文化。「哥布林
    /// 在人类矿城的废墟上重新扎营」这句话之所以说得通，正是因为这一
    /// 条与那一条分开。
    ///
    /// 覆灭时**不清零**：废墟的建材要照着最后住在这里的那批人来铺
    /// （见 [`crate::settlement`] 的 `ruin_tiles`），清零会让全大陆的废墟
    /// 又长得一模一样。
    culture: Option<CultureKind>,
}

/// 一处废墟的最终快照所需的三项。
#[derive(Debug, Clone, Copy)]
struct RuinRecord {
    id: WorldId,
    founded_epoch: u32,
    abandoned_epoch: u32,
    peak_population: u32,
}

/// 两条「多远算邻居」的射程，都由
/// [`ChronicleParams::min_settlement_spacing`] 推出，不是可以独立调的
/// 数值。合成一个结构体而不是 `EpochRun` 上的两个平列字段：它们同源、
/// 恒一起构造，且拆开传会把 `EpochRun::new` 顶过
/// `clippy::too_many_arguments`。
#[derive(Debug, Clone, Copy)]
struct NeighbourRanges {
    /// 一座据点能打到多远，见 [`war_range`]。
    war: u32,
    /// 抽文化时「邻近据点」算到多远，见 [`culture_neighbor_range`]。
    culture: u32,
}

/// 一次纪元推演的全部可变状态。
struct EpochRun {
    candidates: Vec<Candidate>,
    states: Vec<SiteState>,
    events: Vec<HistoricalEvent>,
    epochs: u32,
    seed: u64,
    /// 世界瓦片尺寸——战争配对要量环面切比雪夫距离。
    tile_size: TorusSize,
    /// 两条由最小据点间距推出来的射程，见 [`NeighbourRanges`]。
    ranges: NeighbourRanges,
    /// 当前会话注册的资源种类表。持有一份克隆而不是借用：`EpochRun`
    /// 在 [`WorldChronicle::generate`] 里跨越整段推演存活，借用会把
    /// 调用方的生命周期钉在这上面，而这张表本身很小（每种资源一条
    /// 定长记录），克隆开销可忽略——与 `WorldChronicle::table`
    /// （地形表）的既有取舍逐字相同。
    resources: ResourceTable,
    /// 当前会话注册的文化表（[`crate::culture`]）。持有一份克隆，理由
    /// 与 `resources` 逐字相同。
    cultures: CultureTable,
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
///
/// **这一项只是噪声，不是增长本身**——它的期望是 0。真正让据点长大的
/// 是 [`GROWTH_RATE_DIVISOR`] 那条与承载力挂钩的自然增长，见那里的
/// 文档「为什么必须有这一条」。
const GROWTH_SPREAD: u64 = 5;
/// 见 [`GROWTH_SPREAD`]。
const GROWTH_BIAS: i32 = 2;
/// 上一纪元人口最多的那座据点在本纪元额外获得的增长——首邑聚集效应，
/// 与人口压力一样是跨据点的耦合，不是独立掷骰。
const CAPITAL_GROWTH_BONUS: i32 = 1;
/// 承载力还有富余时，一个纪元自然增长掉多少分之一的现有人口。
///
/// # 为什么必须有这一条（本批次修掉的一个真实缺陷）
///
/// 此前每纪元的人口变动**只有** `[-2, +2]` 那一项噪声，期望是 0——
/// 也就是说人口是一条零漂移的随机游走，它不朝任何地方长，只是随机
/// 抖动直到某次抖到 0 就没了。承载力在这套模型里从来没有真正咬合过
/// （实测：把承载力上界从 19 抬到 50 之后，平均建筑数**一栋都没变**，
/// 仍然是 3.3）。「据点有大有小」在那种模型下只能是掷骰的结果，
/// 与这片地是富是贫毫无关系。
///
/// 换成按比例增长之后，人口是一条**朝承载力收敛**的曲线：一座五人的
/// 营地一个纪元多一两个人，一座五十人的镇子多十来个，直到抵住这片地
/// 能养活的上限为止。「守着水源与良田的地方长成城、光秃秃的高地只够
/// 一个营地」这句话从此是数值上成立的，不是一句说明。
///
/// # 取值是玩法裁定，不是人口学
///
/// 取 3（每纪元三分之一，一个纪元 25 年，合每年约 1.2%）。这**高于**
/// 前工业时代的真实人口增长率（那是每年千分之几的量级），是刻意的：
/// 整部编年史只有 300 年，按真实速率一座村子三百年后仍然是一座村子，
/// 「据点有大有小」与「跨区块据点要真的出现」两条裁决都落不了地。
///
/// 这个数字是实测挑出来的，不是估的（release，本体默认布局，三个种子）：
///
/// | 每纪元增长 | 人口中位 | 平均建筑 | 跨区块据点 |
/// |---|---|---|---|
/// | 0（此前：零漂移随机游走） | — | 3.3 | **0** |
/// | 四分之一 | 19 | 12.5~12.9 | 2~3 |
/// | 三分之一（本值） | 31~33 | 20.2~21.6 | **21~29** |
///
/// 中间那一档虽然已经让「跨区块」从不可能变成可能，但一个世界里只有
/// 两三座、玩家几乎撞不上；本值让约一成的据点真的跨出自己那一格区块，
/// 同时人口中位数（三十出头）与最大值（一百七十余）之间仍有五倍以上
/// 的差距——大小分化没有被抹平。
const GROWTH_RATE_DIVISOR: u32 = 3;

/// 每个居民需要多少格领地养活；人口超过承载力之后增长额外减一。
///
/// # 为什么从 120 抬到这个值：口径换了，不是数值调优
///
/// 承载力此前按**一个区块窗口**（至多 48×48 = 2304 格）的陆地面积算，
/// 于是上界恒在 19 上下、建筑数平均只有 3.3 栋——那是旧的「据点绝不
/// 跨区块」约束留下的痕迹，而据点现在实际拥有的是一片
/// [`territory_radius`] 见方的领地（默认 144×144 = 20736 格）。分母跟着
/// 分子一起换：口径放大约九倍，本常量随之放大约三倍，净效果是据点规模
/// 上界抬高约三倍——项目所有者裁决的「有的据点有大有小」与「跨界据点
/// 要真的出现」两条都落在这里。
///
/// 400 这个具体取值的推导：一片全是陆地的默认领地约两万格，承载力
/// 上界因此约 50 人、约 26 栋建筑，外廓半径 20 格
/// （[`crate::settlement::MAX_FOOTPRINT_RADIUS`] 的推导同一条公式）——
/// 大于「锚点到区块边界」的典型距离，因此长得开的据点**真的会**跨出
/// 自己那一格区块；而它仍然远小于据点最小间距的一半（72），两座长满
/// 的据点不会互相压进对方的街区。
const TILES_PER_RESIDENT: u32 = 400;

/// 资源对承载力的贡献上限（人）——防止一片资源极密的领地把据点撑到
/// 建筑数上界之外去。
///
/// 取 [`MAX_BUILDINGS`] × [`RESIDENTS_PER_BUILDING`]：正好是「建筑数
/// 恰好用满上界」所需的人口，再多的人口一栋房子都换不出来，是这条
/// 上限唯一有意义的位置。
const MAX_RESOURCE_CAPACITY: u32 = MAX_BUILDINGS * RESIDENTS_PER_BUILDING;

/// 资源吸引力每多少「分」给拓荒概率加一分（分子，配
/// [`FOUND_DENOMINATOR`]）。「分」是
/// `Σ 资源点数 × ResourceAttrs::settlement_draw`。
const DRAW_SCORE_PER_FOUND_BONUS: u32 = 20;

/// 资源吸引力对建立概率的加分上限——与
/// [`MAX_FERTILITY_BONUS`]/[`MAX_PRESSURE_BONUS`] 同一个量级，让「守着
/// 铁矿」是一条**显著但不压倒**的理由：三者全满时分子是
/// `3 + 6 + 6 + 8 = 23`，仍然不到分母 64 的一半，拓荒始终是件不确定的事。
const MAX_RESOURCE_DRAW_BONUS: u32 = 8;

/// 每一处可枯竭资源点能被采走多少「人·纪元」。
///
/// 采出速度按人口算（人越多挖得越快），因此这个数的量纲是「人 × 纪元」：
/// 40 意味着一处矿脉够四十个人挖一个纪元，或者十个人挖四个纪元。取这个
/// 量级是为了让枯竭在 12 个纪元的推演里**真的发生在一部分据点身上**，
/// 而不是永远够用（那等于没这条规则）或者第二个纪元就全空（那等于
/// 世界上没有矿业城市）。
const EXTRACTION_PER_NODE: u32 = 40;

/// 资源采光那一刻，有多大比例的人离开——分母。取 3：矿一空先走掉三分
/// 之一，剩下的人还得再撑几个纪元才散干净（见
/// [`DEPLETION_DECLINE_DIVISOR`]），不是由这条规则直接判死。
const DEPLETION_EXODUS_DIVISOR: u32 = 3;

/// 已经枯竭的据点每纪元流失掉多少分之一的人口。
///
/// # 为什么枯竭之后不能只是「承载力变小」
///
/// 这是本批次第二个必须写下来的模型缺陷。只把可枯竭资源那部分承载力
/// 摘掉的话，据点会**收敛到一个更小的规模然后稳住**——矿业城市变成
/// 农业村，永远不会真的没。那在现实里也不算错（很多矿镇正是这么活到
/// 今天的），但它意味着「资源枯竭」永远不会成为一条**覆灭**原因，
/// 而项目所有者点名要的正是覆灭。
///
/// 落地形状是：矿一空，这个地方就没有理由再留人——自然增长整条关掉，
/// 改成按比例外流。取 3（每纪元走三分之一）让一座三十人的矿镇在五六
/// 个纪元里散干净，而不是拖过整部编年史。
const DEPLETION_DECLINE_DIVISOR: u32 = 3;

/// 已经枯竭的据点每纪元至少流失多少人——[`DEPLETION_DECLINE_DIVISOR`]
/// 按比例算到个位数时会退化成 0，这条保证最后那几户人也会走完。
const MIN_DEPLETION_DECLINE: i32 = 2;

/// 瘟疫爆发概率：每多少居民给分子加一分（配
/// [`PLAGUE_DENOMINATOR`]）。人挤人才有大疫，一个十几人的村子几乎不会
/// 被单独记上一笔。
const RESIDENTS_PER_PLAGUE_RISK: u32 = 8;

/// 瘟疫爆发概率的分子上限。
const MAX_PLAGUE_RISK: u32 = 8;

/// 瘟疫爆发概率的分母。
const PLAGUE_DENOMINATOR: u32 = 512;

/// 一场瘟疫**至少**带走多少分之一的人口。
///
/// 取 2：黑死病一类的大疫在单个村镇的致死率就是这个量级。它同时决定
/// 了「疫病能不能灭掉一座据点」——致死率若只有一两成，瘟疫就只是一次
/// 人口回调，永远不会成为一条覆灭原因；取一半起步，则大约有一半的
/// 爆发会把整座据点带走，另一半留下一个元气大伤的幸存者聚落。
const PLAGUE_MIN_LETHALITY_DIVISOR: u32 = 2;

/// 一场瘟疫过后，幸存者少于多少人就干脆散了。
///
/// 没有这一条，瘟疫**几乎不可能**成为一条覆灭原因：致死率再高，只要
/// 掷骰没恰好取到最大值就总会剩下人，而剩下的人在下一个纪元又开始
/// 按比例增长——实测三个种子、七百多座据点，一次死于瘟疫的都没有。
/// 而「一场大疫之后只剩两三户人家，于是那几户也走了」正是村庄被疫病
/// 抹掉的真实过程：直接死绝反而是少数。取 3 与
/// [`INITIAL_POPULATION_MIN`] 同一个量级——低于「一伙人能开拓一处
/// 新地方」所需的人数，这地方就维持不下去了。
const PLAGUE_ABANDON_FLOOR: u32 = 3;

/// 一座据点要有多少人才出得起兵。低于此数的据点既不发动战争，也不会
/// **被**当作值得打的目标（打一个五户人家的营地不是战争，是劫掠——
/// 那是另一套系统）。
const WAR_MIN_POPULATION: u32 = 12;

/// 攻方人口要是守方的多少倍才动手。取 2：势均力敌的两座城互相耗着，
/// 只有明显的强弱差才会变成一次吞并——这让战争成为「大城吃小城」这条
/// 可读的因果，而不是随机对撞。
const WAR_DOMINANCE_RATIO: u32 = 2;

/// 满足全部前置条件之后，一个纪元里真的开战的概率分子。
const WAR_NUMERATOR: u32 = 1;

/// 见 [`WAR_NUMERATOR`]。
const WAR_DENOMINATOR: u32 = 8;

/// 攻方能打多远，单位是「几个最小间距」，见 [`war_range`]。
const WAR_RANGE_IN_SPACINGS: u32 = 3;

/// 攻灭一座据点后，守方有多少人被并进攻方——分母。取 2：一半的人被
/// 掳走或投降，另一半死了或散了。这条让战争**真的在世界人口上留下
/// 痕迹**（吞并不是零和的），不是一次纯粹的删除。
const WAR_SPOILS_DIVISOR: u32 = 2;

/// 一场战争以**占领**（而不是毁灭）收场的概率分母，见
/// [`SAME_RACE_OCCUPATION_NUMERATOR`]。
const OCCUPATION_DENOMINATOR: u32 = 8;

/// 攻守双方**同族**时，这一仗以占领收场的概率分子（6/8 = 75%）。
///
/// # 这条判据的来源
///
/// 项目所有者，逐字：「同种族的话更倾向于占领而不是毁灭」。上一批实测
/// 报告给出的背景是：开启文化后战争从 25/18/15 场涨到 35/43/25 场，而
/// 存活据点从 235/238/243 掉到 231/225/238——**因为当时战争只有一种
/// 结局**（[`SettlementDemise`] 四个变体全是「这座据点没了」），于是
/// 「战争变多」必然等于「据点被灭得更多」。缺的那样东西就是占领。
///
/// # 为什么是「倾向」而不是「必然」
///
/// 所有者说的是「更倾向」。同族之间也有屠城，异族之间也有留下来收税
/// 的征服者；把任何一侧写成 0 或 1 都是在把一句概率判断读成一条规则。
/// 6/8 与 [`CROSS_RACE_OCCUPATION_NUMERATOR`] 的 1/8 相差六倍，足够让
/// 「同族相攻多半是换个主子」在三百年里稳定成型（实测见本批次报告的
/// 对照表），又都留着另一侧的可能。
///
/// # 为什么分母复用 8
///
/// 与 [`WAR_DENOMINATOR`] 相同不是巧合，也不是耦合：战争的「打不打」
/// 与「怎么收场」是同一次结算里的两掷，让它们共用一个可读的分母，
/// 报告里「1/8 概率开战，其中同族 6/8 占领」念得出来。两者互不引用，
/// 改一个不牵动另一个。
const SAME_RACE_OCCUPATION_NUMERATOR: u32 = 6;

/// 攻守双方**异族**时，这一仗以占领收场的概率分子（1/8 = 12.5%）。
///
/// 不是 0：一个哥布林部落偶尔也会把打下来的矿城占着不走。但七成八的
/// 异族战争仍以毁灭收场——上一批的验收线「异族攻灭产出真废墟」因此
/// 一个字都没被削弱（本批次的端到端验收仍然找得到「矮人矿城被哥布林
/// 部落攻灭」并逐格数出石墙）。
const CROSS_RACE_OCCUPATION_NUMERATOR: u32 = 1;

/// 一次占领让守方损失掉多少分之一的人口。
///
/// 取 4（四分之一）。与毁灭那一侧的 [`WAR_SPOILS_DIVISOR`] 对照着读：
/// 铲平一座城，世界上少掉它一半的人（另一半被掳进攻方）；占领一座城
/// 只少掉四分之一，剩下的人原地继续过日子、继续按承载力增长。**这是
/// 「战争多了不等于世界被打空」在数值上成立的地方**。
///
/// 它同时保证守方不会被占成空城：能当目标的据点人口恒 ≥
/// [`WAR_MIN_POPULATION`]（12），四分之一损失之后至少还剩 9 人。
const OCCUPATION_CASUALTY_DIVISOR: u32 = 4;

/// 每多少居民对应一栋建筑。
/// 抽文化时每份文化的**基础权重**：谁也不占优时大家机会均等。
///
/// 取 4 而不是 1：三条加分项（资源 9 / 3、地形 5、邻居 6 每座）要能
/// 把某一份文化抬到明显占优，同时基础分又要大到「一点随机」这一项真
/// 的还在——项目所有者定的四条依据里的第四条。基础 4 意味着一份什么
/// 都不占的文化在一份满分文化面前仍有大约六分之一的机会。
const CULTURE_BASE_WEIGHT: u32 = 4;
/// 资源画像**第一名**命中文化的 [`crate::culture::CultureAttrs::economy`]
/// 时加多少分。取值与 `ll_mod::roster` 的 `PRIMARY_RESOURCE_BONUS`
/// 相同（9），理由也相同：一处据点「因为什么才有人来」这件事应当是
/// 抽取里最重的一项。
const CULTURE_PRIMARY_RESOURCE_BONUS: u32 = 9;
/// 资源画像**第二名**命中时加多少分。
const CULTURE_SECONDARY_RESOURCE_BONUS: u32 = 3;
/// 锚点地形命中文化的 [`crate::culture::CultureAttrs::home_terrain`]
/// 时加多少分。
///
/// 比资源第一名低：一块地「出什么」比它「长什么样」更能决定谁来住
/// ——守着铁矿的丘陵仍然会长出矿业文化，而不是丘陵文化。
const CULTURE_TERRAIN_BONUS: u32 = 5;
/// 射程内每有一座**同文化**的邻居据点加多少分。
///
/// 文化会连成片而不是每座村子各信各的：邻居项是本抽取里唯一带正反馈
/// 的一项，一族站住脚之后周围更容易也是同一族。
const CULTURE_NEIGHBOR_BONUS: u32 = 6;
/// 邻居加分的上限（折合 [`CULTURE_NEIGHBOR_BONUS`] 的倍数）。
///
/// 不设上限的话，一片已经铺满同族据点的地方会把权重推到「另一种文化
/// 数值上不可能被抽中」——那等于把「一点随机」这条依据取消掉，也让
/// 文化边界永远长不出来。取 3 让邻居项最多贡献 18 分，与「资源第一名
/// + 地形」同一个量级。
const MAX_CULTURE_NEIGHBOR_BONUS: u32 = 3;
/// 见 [`culture_neighbor_range`]。
const CULTURE_NEIGHBOR_RANGE_IN_SPACINGS: u32 = 2;

const RESIDENTS_PER_BUILDING: u32 = 2;
/// 一处废墟按历史峰值人口每多少人留下一栋残破建筑。
const PEAK_RESIDENTS_PER_RUIN_BUILDING: u32 = 3;

impl EpochRun {
    fn new(
        candidates: Vec<Candidate>,
        epochs: u32,
        seed: u64,
        tile_size: TorusSize,
        ranges: NeighbourRanges,
        resources: ResourceTable,
        cultures: CultureTable,
    ) -> EpochRun {
        let states = vec![
            SiteState {
                id: None,
                population: 0,
                founded_epoch: 0,
                peak_population: 0,
                extracted: 0,
                depleted: false,
                last_ruin: None,
                culture: None,
            };
            candidates.len()
        ];
        EpochRun {
            candidates,
            states,
            events: Vec::new(),
            epochs,
            seed,
            tile_size,
            ranges,
            resources,
            cultures,
            next_world_id: 0,
        }
    }

    /// 这处候选点的资源给拓荒概率加多少分，见
    /// [`DRAW_SCORE_PER_FOUND_BONUS`]。
    fn resource_draw_bonus(&self, index: usize) -> u32 {
        let mut score = 0u32;
        for count in self.candidates[index].survey.counts() {
            score = score.saturating_add(
                count
                    .nodes
                    .saturating_mul(self.resources.settlement_draw(count.kind)),
            );
        }
        (score / DRAW_SCORE_PER_FOUND_BONUS).min(MAX_RESOURCE_DRAW_BONUS)
    }

    /// 这处候选点当前能养活多少人。
    ///
    /// 两项相加：领地陆地面积换算出的基础承载力，加上各种资源点的
    /// 贡献。已经枯竭（[`SiteState::depleted`]）时，可枯竭资源那部分
    /// **不再计入**——这正是「矿采光了，城养不起这么多人了」这条因果
    /// 在数值上的落点。
    fn capacity(&self, index: usize) -> u32 {
        let candidate = &self.candidates[index];
        let depleted = self.states[index].depleted;
        let mut capacity = candidate.survey.land_area() / TILES_PER_RESIDENT;
        let mut from_resources = 0u32;
        for count in candidate.survey.counts() {
            if depleted && self.resources.exhaustible(count.kind) {
                continue;
            }
            from_resources = from_resources.saturating_add(
                count
                    .nodes
                    .saturating_mul(self.resources.residents_supported(count.kind)),
            );
        }
        capacity = capacity.saturating_add(from_resources.min(MAX_RESOURCE_CAPACITY));
        capacity
    }

    /// 这处候选点的可枯竭资源总储量（「人·纪元」，见
    /// [`EXTRACTION_PER_NODE`]）。全是可再生资源时为 0——那样的据点
    /// 永远不会因枯竭而衰败。
    fn exhaustible_reserve(&self, index: usize) -> u32 {
        let mut reserve = 0u32;
        for count in self.candidates[index].survey.counts() {
            if !self.resources.exhaustible(count.kind) {
                continue;
            }
            reserve = reserve.saturating_add(count.nodes.saturating_mul(EXTRACTION_PER_NODE));
        }
        reserve
    }

    /// 这处候选点最主要的那种可枯竭资源——枯竭事件要指名道姓说出「死于
    /// **哪一种**资源枯竭」。
    ///
    /// 「最主要」取资源点数最多的那一种；并列时取
    /// [`ResourceTable::registered`] 顺序最先的（`>` 而非 `>=`，不依赖
    /// 任何迭代顺序，约束 C5）。
    fn dominant_exhaustible(&self, index: usize) -> Option<ResourceKind> {
        let mut best: Option<(ResourceKind, u32)> = None;
        for count in self.candidates[index].survey.counts() {
            if !self.resources.exhaustible(count.kind) {
                continue;
            }
            match best {
                Some((_, top)) if count.nodes <= top => {}
                _ => best = Some((count.kind, count.nodes)),
            }
        }
        best.map(|(kind, _)| kind)
    }

    /// 这处候选点最突出的那几种资源——[`SettlementSite::resource_profile`]
    /// 的产地，见该字段文档「为什么是『点数 × 吸引力』而不是纯点数」。
    ///
    /// 排序键是 `资源点数 × ResourceAttrs::settlement_draw`（与
    /// [`Self::resource_draw_bonus`] 逐项相同的那个乘积，只是这里不求和
    /// 而是排名）。并列时取 [`ResourceTable::registered`] 顺序最先的
    /// （`>` 而非 `>=`，与 [`Self::dominant_exhaustible`] 同一条不依赖
    /// 任何迭代顺序的写法，约束 C5）；`counts()` 本身就按注册顺序排列，
    /// 因此整个函数不含任何哈希容器。
    ///
    /// 乘积为 0 的资源**不入榜**：`settlement_draw` 为 0 的资源等于
    /// 「有它跟没它一样」，把它排进画像只会让下游误以为这地方靠它吃饭。
    fn resource_profile(&self, index: usize) -> [Option<ResourceKind>; SITE_RESOURCE_SLOTS] {
        let mut top: [Option<(ResourceKind, u32)>; SITE_RESOURCE_SLOTS] =
            [None; SITE_RESOURCE_SLOTS];
        for count in self.candidates[index].survey.counts() {
            let score = count
                .nodes
                .saturating_mul(self.resources.settlement_draw(count.kind));
            if score == 0 {
                continue;
            }
            // 插入排序：名次只有两三个，插入排序比排一整个 Vec 更直白，
            // 也不需要额外分配。
            let mut carried = Some((count.kind, score));
            for slot in top.iter_mut() {
                let Some(entry) = carried else {
                    break;
                };
                match *slot {
                    Some((_, held)) if held >= entry.1 => {}
                    _ => {
                        carried = *slot;
                        *slot = Some(entry);
                    }
                }
            }
        }
        top.map(|entry| entry.map(|(kind, _)| kind))
    }

    /// 这处候选点在本纪元该长出哪一种文化——项目所有者定的四条依据
    /// 「资源 + 地形 + 邻近据点 + 一点随机」的落地。
    ///
    /// # 四条依据分别落在哪
    ///
    /// | 依据 | 实现 |
    /// |---|---|
    /// | 资源 | 资源画像前两名的**大类**命中文化的 `economy` 就加分（[`CULTURE_PRIMARY_RESOURCE_BONUS`]/[`CULTURE_SECONDARY_RESOURCE_BONUS`]） |
    /// | 地形 | 锚点基础地形命中文化的 `home_terrain` 就加分（[`CULTURE_TERRAIN_BONUS`]） |
    /// | 邻近据点 | 射程内每有一座同文化的邻居就加分，有上限（[`CULTURE_NEIGHBOR_BONUS`]/[`MAX_CULTURE_NEIGHBOR_BONUS`]） |
    /// | 一点随机 | 全部加分只是**权重**，最终由 `rng` 抽一次（[`CULTURE_BASE_WEIGHT`] 保证冷门文化仍有机会） |
    ///
    /// **一个字节的种族数据都不读**——这正是所有者「文化由据点建立时
    /// 决定，**不看种族**」那一句的字面落实。反过来的那一半（种族给
    /// 文化加权）落在 [`crate::culture::CultureAttrs::founder_races`]
    /// 上，方向与所有者原话相反，理由记在那个字段的文档里。
    ///
    /// # 确定性（约束 C3 / C5）
    ///
    /// 遍历只走 [`CultureTable::registered`]（注册顺序的 `Vec`）；邻居
    /// 统计按候选点光栅序；加权抽取线性扫描同一个顺序。全程不碰任何
    /// 哈希容器。随机来自调用方递进来的、由
    /// [`CHRONICLE_CULTURE_STREAM_ID`] 完全确定的那条流。
    ///
    /// 一条文化都没注册时返回 `None`（ADR 0015「尚无内容」的诚实
    /// 表达），下游全部退化到文化落地之前的行为。
    fn pick_culture(&self, index: usize, rng: &mut DetRng) -> Option<CultureKind> {
        let registered = self.cultures.registered();
        if registered.is_empty() {
            return None;
        }
        let candidate = &self.candidates[index];
        let mut total = 0u64;
        // 两趟线性扫描而不是先物化一张权重表：文化条数是个位数量级，
        // 第二趟重算一遍比分配一个 `Vec` 便宜，也不需要 `EpochRun`
        // 多一个可变缓冲（约束 C1「不留跨帧隐式状态」在这里表现为
        // 「不为一次抽取留一块长命内存」）。
        for kind in registered {
            total += u64::from(self.culture_weight(index, *kind));
        }
        if total == 0 {
            return None;
        }
        let mut roll = rng.gen_range(total);
        for kind in registered {
            let weight = u64::from(self.culture_weight(index, *kind));
            if roll < weight {
                return Some(*kind);
            }
            roll -= weight;
        }
        // 理论不可达（`roll < total` 而循环恰好减掉了全部权重之和）。
        // 退回第一份文化而不是 panic，规格 §10.2「降级而非崩溃」。
        let _ = candidate;
        registered.first().copied()
    }

    /// 一份文化在这处候选点上的权重，见 [`Self::pick_culture`] 的四条
    /// 依据表。
    fn culture_weight(&self, index: usize, kind: CultureKind) -> u32 {
        let candidate = &self.candidates[index];
        let mut weight = CULTURE_BASE_WEIGHT;

        // ① 资源：画像前两名的大类命中这份文化靠什么吃饭。
        if let Some(economy) = self.cultures.economy(kind) {
            for (rank, entry) in self.resource_profile(index).into_iter().enumerate() {
                let Some(resource) = entry else {
                    continue;
                };
                if self.resources.category(resource) != Some(economy) {
                    continue;
                }
                weight = weight.saturating_add(if rank == 0 {
                    CULTURE_PRIMARY_RESOURCE_BONUS
                } else {
                    CULTURE_SECONDARY_RESOURCE_BONUS
                });
            }
        }

        // ② 地形：锚点那一格。
        if self.cultures.home_terrain(kind) == Some(candidate.anchor_terrain) {
            weight = weight.saturating_add(CULTURE_TERRAIN_BONUS);
        }

        // ③ 邻近据点：射程内还有人住、且已经是这份文化的那些。
        let mut neighbours = 0u32;
        for (other, state) in self.states.iter().enumerate() {
            if other == index || state.population == 0 || state.culture != Some(kind) {
                continue;
            }
            let distance = self
                .tile_size
                .chebyshev(candidate.anchor, self.candidates[other].anchor);
            if distance <= self.ranges.culture {
                neighbours += 1;
                if neighbours >= MAX_CULTURE_NEIGHBOR_BONUS {
                    break;
                }
            }
        }
        weight.saturating_add(neighbours.saturating_mul(CULTURE_NEIGHBOR_BONUS))
    }

    /// `attacker` 那座据点的文化对 `defender` 那座的敌意分。
    ///
    /// 两侧任意一方没有文化（或表里查不到这一对）就是 0——**全表敌意
    /// 为 0 时战争行为与文化落地之前逐位相同**，见
    /// [`crate::culture::CultureAttrs::hostility`]。
    fn hostility_between(&self, attacker: usize, defender: usize) -> u32 {
        self.cultures
            .hostility(self.states[attacker].culture, self.states[defender].culture)
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
            self.wage_wars(epoch);
            world_population = self.states.iter().map(|state| state.population).sum();
            capital = self.pick_capital();
        }
    }

    /// 本纪元的战争：在**内政推演跑完之后**，按候选点光栅序让每一座
    /// 够强的据点找一次架打。
    ///
    /// # 为什么单独一趟，而不是塞进 [`Self::advance_settled`]
    ///
    /// 战争是**跨据点**的：它同时改写两座据点的状态。塞进逐座推进的
    /// 那趟里，一座据点会在「自己这一纪元还没过完」的时候被邻居抹掉，
    /// 于是「这一纪元的人口」这个量在同一趟里对不同据点意义不同。分成
    /// 两趟之后语义是清楚的：**先各自过日子，再互相打**。
    ///
    /// # 确定性（约束 C5）
    ///
    /// 攻方按候选点光栅序遍历；守方取「本纪元结束时仍有人住、且在射程
    /// 内、离得最近」的那一座，并列时取光栅序最先的（`<` 而非 `<=`）；
    /// 每次开战判定用一条由 `(种子, [`CHRONICLE_WAR_STREAM_ID`],
    /// 攻方序号 × 纪元数 + 纪元号)` 完全确定的流。三者都与任何
    /// `HashMap`/`HashSet` 的迭代顺序无关。
    ///
    /// 一座据点在本纪元里被攻灭之后人口即为 0，因此它此后既不会再作为
    /// 攻方出场，也不会再被别人当作目标——「同一个纪元被打两次」不可能
    /// 发生，不需要额外的已处理标记。
    fn wage_wars(&mut self, epoch: u32) {
        if self.ranges.war == 0 {
            return;
        }
        for attacker in 0..self.candidates.len() {
            if self.states[attacker].population < WAR_MIN_POPULATION {
                continue;
            }
            let Some(defender) = self.pick_target(attacker) else {
                continue;
            };
            if self.states[attacker].population
                <= self.states[defender].population * WAR_DOMINANCE_RATIO
            {
                continue;
            }
            let mut rng = DetRng::for_entity(
                self.seed,
                CHRONICLE_WAR_STREAM_ID,
                attacker as u64 * u64::from(self.epochs) + u64::from(epoch),
            );
            // 敌对是**加分项**，不是替代品：人口阈值与优势比两条闸门
            // 一个字都没动，敌意只是把已有的 1/8 掷骰抬高——与
            // `try_found` 已经在用的「四条加分互相独立地推高同一个概率
            // 分子」是同一个手法。上界 `MAX_HOSTILITY`（7）由注册期
            // 校验守着，分子因此恒 < 分母，「够强就必然开战」不可能
            // 发生。敌意恒为 0（空文化表）时这一行与旧代码逐位相同。
            let hostility = self.hostility_between(attacker, defender);
            if !rng.chance(WAR_NUMERATOR + hostility, WAR_DENOMINATOR) {
                continue;
            }
            // 打不打已经定了，剩下的是**怎么收场**：占领还是毁灭。
            // 这一掷取自同一条流的下一个数，不新开一条——`rng` 是
            // 每 (攻方, 纪元) 由 `DetRng::for_entity` 现造、用完即弃
            // 的，在开战判定之后多取一个数不影响任何别的判定（同一条
            // 观察记在 `全表敌意为零时战争结果与空文化表逐位相同` 的
            // 「另一处试过但不成立的改坏」一节里）。ADR 0021 拦的正是
            // 「为了对称再开一条流」这种加法。
            let occupied = match self.occupation_numerator(attacker, defender) {
                Some(numerator) => rng.chance(numerator, OCCUPATION_DENOMINATOR),
                None => false,
            };
            if occupied {
                self.occupy(attacker, defender, epoch);
            } else {
                self.conquer(attacker, defender, epoch);
            }
        }
    }

    /// 这一仗以**占领**收场的概率分子；`None` 表示这个世界里没有
    /// 「归属」可换，只可能毁灭。
    ///
    /// # 判据只有一条：攻守双方是不是同一个种族
    ///
    /// 项目所有者：「同种族的话更倾向于占领而不是毁灭」。种族取的是
    /// [`crate::culture::founder_race`] ——**与 `ll_mod::roster` 给
    /// 这座据点排名册时用的是同一个函数、同一条随机流**，因此编年史
    /// 里说「这是一场同族战争」时，名册那一侧点开两座城看到的确实是
    /// 同一个种族。那个函数本来住在 `ll-mod`，为了这一条判据搬到了
    /// `ll-world`（理由见
    /// [`crate::culture::FOUNDER_RACE_STREAM_ID`]）。
    ///
    /// # 为什么「没有文化」等于「只可能毁灭」
    ///
    /// 占领改掉的是守方的文化（见 [`Self::occupy`]）。一个没有文化
    /// 这一层的世界（空文化表）里没有任何东西可以易主，「占领」在那里
    /// 无从表达。返回 `None` 让那样的世界**逐位退回本批次之前的行为**
    /// ——这既是 ADR 0015「查不到就是查不到」的既有表达，也是「把改动
    /// 关掉能精确回到旧值」这条纪律在本批次的落点，见本模块测试
    /// `空文化表下战争仍然只有毁灭一种结局`。
    fn occupation_numerator(&self, attacker: usize, defender: usize) -> Option<u32> {
        let attacker_race = self.founder_race_of(attacker)?;
        let defender_race = self.founder_race_of(defender)?;
        Some(if attacker_race == defender_race {
            SAME_RACE_OCCUPATION_NUMERATOR
        } else {
            CROSS_RACE_OCCUPATION_NUMERATOR
        })
    }

    /// 这座据点**现在**由哪一族当家——文化决定种族，因此它会随占领
    /// 一起变（见 [`Self::occupy`]）。无人居住或没有文化时为 `None`。
    fn founder_race_of(&self, index: usize) -> Option<ll_core::ident::ContentIndex> {
        let state = &self.states[index];
        crate::culture::founder_race(&self.cultures, state.culture, state.id?, self.seed)
    }

    /// `attacker` 这一纪元打谁：射程内、仍有人住、且值得打（人口不低于
    /// [`WAR_MIN_POPULATION`]）的那些据点里，**敌意最高的**；敌意并列
    /// 时取最近的；再并列时取候选点光栅序最先的。
    ///
    /// # 为什么排序键要加上敌意这一维
    ///
    /// 本函数此前叫 `nearest_rival`，只按距离取最近。部落与文明复用
    /// 同一套据点推演之后，那条判据会产出语义错误的历史：一个哥布林
    /// 营地因为**隔壁那个哥布林营地更近**而去灭了自己人，而两格之外
    /// 的矮人矿城相安无事。加上敌意这一维之后，「矮人矿城被哥布林部落
    /// 攻灭」才有可能真的发生。
    ///
    /// # 这不是「敌对才打」
    ///
    /// 全部据点敌意都是 0 时（空文化表、或者内容里没声明任何敌对），
    /// 排序键退化成「距离，光栅序并列」——**与改名之前逐位相同的行为**。
    /// 敌对没有成为开战的必要条件，只是改变了打谁；打不打仍由人口阈值、
    /// 优势比、掷骰三条决定，见 [`Self::wage_wars`]。
    ///
    /// # 确定性（约束 C5）
    ///
    /// 遍历按候选点光栅序，比较是「敌意 `>` / 距离 `<`」的严格不等号，
    /// 因此并列恒由光栅序决出，不依赖任何迭代顺序。
    fn pick_target(&self, attacker: usize) -> Option<usize> {
        let origin = self.candidates[attacker].anchor;
        let mut best: Option<(usize, u32, u32)> = None;
        for (index, state) in self.states.iter().enumerate() {
            if index == attacker || state.population < WAR_MIN_POPULATION {
                continue;
            }
            let distance = self
                .tile_size
                .chebyshev(origin, self.candidates[index].anchor);
            if distance > self.ranges.war {
                continue;
            }
            let hostility = self.hostility_between(attacker, index);
            let better = match best {
                None => true,
                Some((_, top_hostility, closest)) => {
                    hostility > top_hostility || (hostility == top_hostility && distance < closest)
                }
            };
            if better {
                best = Some((index, hostility, distance));
            }
        }
        best.map(|(index, _, _)| index)
    }

    /// `attacker` **占领** `defender`：守方活下来，换了主子。
    ///
    /// # 换掉的是文化，这是本批次的实现判断
    ///
    /// 项目所有者只说了「同种族的话更倾向于占领而不是毁灭」，没有说
    /// 「占领换掉的是什么」。这里的选择是**文化**，理由是它是当前世界
    /// 模型里唯一一个说得出三个真实消费者的归属属性：
    ///
    /// | 换文化之后，哪里跟着变 | 在哪一行 |
    /// |---|---|
    /// | 这座城用什么建材盖房 | [`crate::settlement`] 的 `wall_terrain` |
    /// | 这座城住的是哪一族 | [`crate::culture::founder_race`] |
    /// | 这座城此后跟谁不对付 | [`Self::hostility_between`] |
    ///
    /// 另造一个 `faction: WorldId` 字段则一个消费者都没有——那正是本
    /// 仓库已经数出三十一处的「声明了但从没接线」。
    ///
    /// # 不变的那些，同样是判断
    ///
    /// - **`id` 不变**：同一座城换了主子，不是旧城没了新城建起来了。
    ///   编年史因此读得出「建于第 2 纪元、第 6 纪元易主、至今仍有人住」。
    /// - **`founded_epoch` 不变**：它没有被重建过。
    /// - **`peak_population` 不变**：一座城的历史峰值不因易主而改写。
    /// - **`extracted` / `depleted` 不变**：矿是这片地的属性，不是这批
    ///   人的（判据与 [`Self::abandon`] 里那一条逐字相同）。
    ///
    /// # 攻方一个人都没多
    ///
    /// 与毁灭那一侧（[`WAR_SPOILS_DIVISOR`]，守方一半人被掳进攻方）
    /// 刻意不同：占领得到的是一座城，不是一批人——人还在原地，只是
    /// 换了主子。这条让占领在世界人口上是**保住人**而不是搬运人。
    fn occupy(&mut self, attacker: usize, defender: usize, epoch: u32) {
        let conqueror = self.states[attacker]
            .id
            .expect("人口非零的据点必然在建立时分配过 WorldId");
        let site_id = self.states[defender]
            .id
            .expect("人口非零的据点必然在建立时分配过 WorldId");
        let (Some(former), Some(new)) =
            (self.states[defender].culture, self.states[attacker].culture)
        else {
            unreachable!("occupation_numerator 已经保证两侧都有文化");
        };
        let before = self.states[defender].population;
        let survivors = before - before / OCCUPATION_CASUALTY_DIVISOR;
        debug_assert!(
            survivors > 0,
            "被占领的据点必须活下来——人被打光的城是毁灭，不是占领"
        );
        self.states[defender].population = survivors;
        self.states[defender].culture = Some(new);

        let event_id = WorldId::next(&mut self.next_world_id);
        self.events.push(HistoricalEvent {
            id: event_id,
            at: epoch_tick(epoch, self.epochs),
            location: self.candidates[defender].anchor,
            kind: HistoricalEventKind::SettlementConquered(SettlementConqueredRecord {
                site: site_id,
                epoch,
                conqueror,
                former_culture: former.index(),
                new_culture: new.index(),
                survivors,
            }),
        });
    }

    /// `attacker` 攻灭 `defender`：守方就此成为废墟，一半人口被并进
    /// 攻方（[`WAR_SPOILS_DIVISOR`]）。
    ///
    /// 这是战争的**另一种**结局，见 [`Self::occupy`]。两者共用
    /// [`Self::wage_wars`] 那一整套配对与闸门，分岔只发生在最后一掷
    /// 之后——不存在两条平行的战争管线（ADR 0021）。
    fn conquer(&mut self, attacker: usize, defender: usize, epoch: u32) {
        let spoils = self.states[defender].population / WAR_SPOILS_DIVISOR;
        let aggressor = self.states[attacker]
            .id
            .expect("人口非零的据点必然在建立时分配过 WorldId");
        self.abandon(
            defender,
            epoch,
            SettlementDemise::War { aggressor },
            self.states[defender].population,
        );
        let grown = self.states[attacker].population.saturating_add(spoils);
        self.states[attacker].population = grown;
        self.states[attacker].peak_population = self.states[attacker].peak_population.max(grown);
    }

    /// 把 `index` 这座据点变成废墟：清空人口、记下这一茬的墓志铭、
    /// 追加一条 [`HistoricalEventKind::SettlementAbandoned`] 事件。
    ///
    /// 全部四种覆灭原因走同一条出口——「怎么没的」是参数，「没了之后
    /// 状态怎么变」只有一份实现。此前遗弃只有一种原因，这段逻辑内联在
    /// [`Self::advance_settled`] 里；三种新原因落地时把它抽出来，正是
    /// ADR 0021 的另一半（**有一份算法要被四处共用**）。
    ///
    /// `final_population` 是「归零之前那一刻有多少人」——事件本身记的是
    /// 历史峰值，这个参数只用来让调用方把「这一场疫病死了多少人」这类
    /// 载荷算对，不写进状态。
    fn abandon(
        &mut self,
        index: usize,
        epoch: u32,
        cause: SettlementDemise,
        final_population: u32,
    ) {
        debug_assert!(
            final_population > 0 || matches!(cause, SettlementDemise::Depopulation),
            "只有自然凋零才允许在人口已经是 0 的情况下走到这里"
        );
        let state = self.states[index];
        let site_id = state.id.expect("人口非零的据点必然在建立时分配过 WorldId");
        let event_id = WorldId::next(&mut self.next_world_id);
        self.states[index] = SiteState {
            id: None,
            population: 0,
            founded_epoch: state.founded_epoch,
            peak_population: 0,
            // 采掘进度与枯竭标记**不随这一茬清零**：矿是这片地的属性，
            // 不是这批人的。下一批人来重新拓荒时接手的是同一个已经被
            // 挖过的矿脉，见 SiteState::extracted 文档。
            extracted: state.extracted,
            depleted: state.depleted,
            // 文化**不随覆灭清零**：废墟的建材要照着最后住在这里的那批
            // 人来铺，见 SiteState::culture 文档。
            culture: state.culture,
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
            location: self.candidates[index].anchor,
            kind: HistoricalEventKind::SettlementAbandoned(SettlementAbandonedRecord {
                site: site_id,
                epoch,
                peak_population: state.peak_population,
                epochs_inhabited: epoch - state.founded_epoch,
                cause,
            }),
        });
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
    ///
    /// 四条加分互相独立地推高同一个概率分子：土地肥沃（本区块的连通
    /// 陆地）、世界人口压力（上一纪元的跨据点耦合）、**资源吸引力**
    /// （本批次新增，见 [`Self::resource_draw_bonus`]），加上一个基础分。
    /// 「守着铁矿的地方更容易建城」就是第三条。
    fn try_found(&mut self, index: usize, epoch: u32, world_population: u32, rng: &mut DetRng) {
        let land_area = self.candidates[index].land_area;
        let anchor = self.candidates[index].anchor;
        let fertility = (land_area / TILES_PER_FERTILITY_BONUS).min(MAX_FERTILITY_BONUS);
        let pressure = (world_population / POPULATION_PER_PRESSURE_BONUS).min(MAX_PRESSURE_BONUS);
        let draw = self.resource_draw_bonus(index);
        if !rng.chance(
            FOUND_BASE_NUMERATOR + fertility + pressure + draw,
            FOUND_DENOMINATOR,
        ) {
            return;
        }

        let population = INITIAL_POPULATION_MIN + rng.gen_range(INITIAL_POPULATION_SPREAD) as u32;
        let site_id = WorldId::next(&mut self.next_world_id);
        let event_id = WorldId::next(&mut self.next_world_id);
        let previous = self.states[index];
        // 文化在**拓荒这一刻**抽一次，此后这一茬人一直用它（项目所有者
        // 「文化由据点建立时决定」）。走一条独立的流
        // （[`CHRONICLE_CULTURE_STREAM_ID`]）而不是复用 `rng`：复用会
        // 让「这个世界有几种文化」改变后面每一处拓荒/增长的掷骰，
        // 加一条 cultures.json5 就等于换了一个世界。
        let mut culture_rng = DetRng::for_entity(
            self.seed,
            CHRONICLE_CULTURE_STREAM_ID,
            index as u64 * u64::from(self.epochs) + u64::from(epoch),
        );
        let culture = self.pick_culture(index, &mut culture_rng);
        self.states[index] = SiteState {
            id: Some(site_id),
            population,
            founded_epoch: epoch,
            peak_population: population,
            culture,
            // 接手上一茬挖剩的矿：储量是这片地的属性，不随「换了一批
            // 人」恢复。一处已经被挖空的矿脉旁边可以再建村子，但那座
            // 村子从第一天起就没有矿业可依。
            extracted: previous.extracted,
            depleted: previous.depleted,
            last_ruin: previous.last_ruin,
        };
        self.events.push(HistoricalEvent {
            id: event_id,
            at: epoch_tick(epoch, self.epochs),
            location: anchor,
            kind: HistoricalEventKind::SettlementFounded(SettlementFoundedRecord {
                site: site_id,
                epoch,
                initial_population: population,
                land_area,
            }),
        });
    }

    /// 一座有人住的据点在本纪元的兴衰。人口归零即被遗弃，留下废墟。
    ///
    /// 一个纪元里依次发生四件事，顺序是刻意的：
    ///
    /// 1. **开采**：按当时的人口消耗可枯竭资源的储量；刚好在本纪元采光
    ///    的，此刻走人一批（[`DEPLETION_EXODUS_DIVISOR`]），承载力里
    ///    属于那种资源的部分从此不再计入。
    /// 2. **增长**：既有的随机涨落 + 首邑加成 + 承载力惩罚。
    /// 3. **瘟疫**：人越密越容易爆发，爆发就随机带走一批人。
    /// 4. **结算**：人口归零就是覆灭，死因取决于上面哪一步把它推到零。
    ///
    /// 战争不在这里——它是跨据点的，单独一趟，见 [`Self::wage_wars`]。
    fn advance_settled(&mut self, index: usize, epoch: u32, is_capital: bool, rng: &mut DetRng) {
        // ① 开采与枯竭。
        let just_depleted = self.extract(index);

        let state = self.states[index];
        let capacity = self.capacity(index);

        // ② 增长：噪声（期望 0）+ 与承载力挂钩的自然增长 + 首邑加成。
        let mut delta = rng.gen_range(GROWTH_SPREAD) as i32 - GROWTH_BIAS;
        if is_capital {
            delta += CAPITAL_GROWTH_BONUS;
        }
        if state.depleted {
            // 矿一空，这地方就没有理由再留人：自然增长整条关掉，改成
            // 按比例外流，见 DEPLETION_DECLINE_DIVISOR 文档。
            delta -=
                ((state.population / DEPLETION_DECLINE_DIVISOR) as i32).max(MIN_DEPLETION_DECLINE);
        } else if state.population < capacity {
            // 按比例增长，且不越过这片地能养活的上限——见
            // GROWTH_RATE_DIVISOR 文档「为什么必须有这一条」。至少 +1：
            // 一座三五个人的据点若按比例算成 0，就永远迈不出第一步。
            let room = capacity - state.population;
            delta += (state.population / GROWTH_RATE_DIVISOR).max(1).min(room) as i32;
        } else if state.population > capacity {
            delta -= 1;
        }
        let mut population = state.population.saturating_add_signed(delta);

        // ③ 瘟疫：分子随人口密度上升，爆发一次带走 1..=当前人口 的人。
        let risk = (population / RESIDENTS_PER_PLAGUE_RISK).min(MAX_PLAGUE_RISK);
        let mut plague_dead = 0u32;
        if population > 0 && rng.chance(risk, PLAGUE_DENOMINATOR) {
            // 致死率从「一半」起跳，掷骰只决定它有多接近「全灭」——
            // 见 PLAGUE_MIN_LETHALITY_DIVISOR 文档。
            let floor = population / PLAGUE_MIN_LETHALITY_DIVISOR;
            plague_dead = floor + 1 + rng.gen_range(u64::from(population - floor)) as u32;
            population = population.saturating_sub(plague_dead);
            if population < PLAGUE_ABANDON_FLOOR {
                // 剩下的那两三户也走了，见 PLAGUE_ABANDON_FLOOR 文档。
                plague_dead = plague_dead.saturating_add(population);
                population = 0;
            }
        }

        let peak = state.peak_population.max(population);
        if population > 0 {
            self.states[index] = SiteState {
                population,
                peak_population: peak,
                ..self.states[index]
            };
            return;
        }

        // ④ 结算：死因取上面最后一个把人口推到零的原因。判定顺序就是
        //    发生顺序的逆序——瘟疫是压垮它的最后一根稻草时算瘟疫，
        //    没有瘟疫而这一纪元刚好采光时算枯竭，其余都是自然凋零。
        let cause = if plague_dead > 0 {
            SettlementDemise::Plague { dead: plague_dead }
        } else if just_depleted || self.states[index].depleted {
            match self.dominant_exhaustible(index) {
                Some(kind) => SettlementDemise::ResourceExhausted {
                    resource: kind.index(),
                },
                // 已经标记枯竭却找不到可枯竭资源，只可能是内容表在推演
                // 中途被换过（生产路径不会发生）。退回自然凋零而不是
                // panic：一条说不清来源的死因不值得让整个建世界失败。
                None => SettlementDemise::Depopulation,
            }
        } else {
            SettlementDemise::Depopulation
        };
        self.abandon(index, epoch, cause, state.population);
    }

    /// 本纪元的开采：把当时的人口累加进 [`SiteState::extracted`]，越过
    /// 储量就把这一茬标记为枯竭并让一批人离开。
    ///
    /// 返回「**本纪元**刚好采光了吗」——调用方用它区分「这一纪元矿没
    /// 了」与「早就没了、只是还没死透」，两者的死因归属不同。
    fn extract(&mut self, index: usize) -> bool {
        let reserve = self.exhaustible_reserve(index);
        if reserve == 0 || self.states[index].depleted {
            return false;
        }
        let population = self.states[index].population;
        let extracted = self.states[index].extracted.saturating_add(population);
        self.states[index].extracted = extracted;
        if extracted <= reserve {
            return false;
        }
        self.states[index].depleted = true;
        let leaving = population / DEPLETION_EXODUS_DIVISOR;
        self.states[index].population = population.saturating_sub(leaving);
        true
    }

    /// 推演结束后的最终快照：仍有人住的是村子，最近一次被遗弃且此后
    /// 无人重建的是废墟，从未被住过的不产出任何东西。
    ///
    /// 结果按区块光栅序——`candidates` 本就是按这个顺序收集的，这里
    /// 只是保持它，供 [`WorldChronicle::site_in_zone`] 二分。
    fn final_sites(&self) -> Vec<SettlementSite> {
        let mut sites = Vec::new();
        for (index, state) in self.states.iter().enumerate() {
            let candidate = &self.candidates[index];
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
                    resource_profile: self.resource_profile(index),
                    culture: state.culture,
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
                    // 废墟照样带画像：它记录的是**这片地上有什么**，与
                    // 「还有没有人住」无关。将来的废墟叙事（这里曾经是
                    // 一座矿城）要的正是这一条。
                    resource_profile: self.resource_profile(index),
                    // 废墟带的是**最后住在这里的那批人**的文化，见
                    // SiteState::culture 文档「覆灭时不清零」。
                    culture: state.culture,
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
    use crate::culture::CultureAttrs;
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
        let (_kinds, resources) = test_resources(&ids);
        WorldChronicle::generate(
            &ChronicleInput {
                layout: &layout,
                noise: &noise,
                params: &params,
                terrain_ids: &ids,
                terrain_table: &table,
                resources: &resources,
                cultures: &CultureTable::new(),
            },
            chronicle_params,
        )
    }

    /// 测试用资源表：本体那四种（[`crate::resource::base_resource_fixture`]），
    /// 取值与 `mods/lostland/resources.json5` 一致。
    fn test_resources(
        ids: &crate::terrain::BaseTerrainIds,
    ) -> (Vec<crate::resource::ResourceKind>, ResourceTable) {
        let mut interner = ll_core::ident::Interner::new();
        crate::resource::base_resource_fixture(&mut interner, ids)
    }

    /// 测试用文化表：两条，形状照抄 `mods/lostland/cultures.json5` 的
    /// `lostland:mining_hold` 与 `lostland:goblin_warband`。
    ///
    /// `hostility` 是「部落 → 矿业」那一个方向的敌意分（反向恒为 0，
    /// 刻意不对称）。传 0 就得到一张**除了敌对什么都一样**的表——
    /// `全表敌意为零时战争结果与空文化表逐位相同` 与
    /// `敌意抬高了战争导致的覆灭次数` 两条测试就靠这个参数把「敌对」
    /// 这一个变量单独隔离出来。
    fn test_cultures(
        ids: &crate::terrain::BaseTerrainIds,
        hostility: u32,
    ) -> (CultureTable, [CultureKind; 2]) {
        test_cultures_with_races(ids, hostility, true)
    }

    /// 与 [`test_cultures`] 相同，外加一个「两条文化是不是同一个建立者
    /// 种族」的开关——占领批次靠它把「同族 / 异族」这一个变量单独隔离
    /// 出来（`同族更倾向占领异族更倾向毁灭`）。
    ///
    /// `same_race == true` 时两条都用 `test:folk`（与本开关引入之前的
    /// 行为逐字相同，因此既有测试一个字都不用改）；为 `false` 时部落那
    /// 一条换成 `test:otherfolk`。**权重结构两边完全一样**（各一条、
    /// 权重 10），所以 `crate::culture::founder_race` 消耗的随机数也
    /// 一样——两个世界的差别只有「这两条是不是同一族」。
    fn test_cultures_with_races(
        ids: &crate::terrain::BaseTerrainIds,
        hostility: u32,
        same_race: bool,
    ) -> (CultureTable, [CultureKind; 2]) {
        use ll_core::ident::{Interner, NamespacedId};
        let mut interner = Interner::new();
        let mut table = CultureTable::new();
        let mining = interner.intern(NamespacedId::parse("test:mining_hold").expect("合法"));
        let tribe = interner.intern(NamespacedId::parse("test:warband").expect("合法"));
        let race = interner.intern(NamespacedId::parse("test:folk").expect("合法"));
        let other_race = interner.intern(NamespacedId::parse("test:otherfolk").expect("合法"));
        let tribe_race = if same_race { race } else { other_race };
        table
            .define(
                mining,
                CultureAttrs {
                    display_name_key: NamespacedId::parse("test:culture.mining.display_name")
                        .expect("合法"),
                    economy: crate::resource::ResourceCategory::Metal,
                    home_terrain: ids.mountain,
                    wall_terrain: ids.wall_stone,
                    founder_races: vec![(race, 10)],
                    hostility: Vec::new(),
                    buildings: crate::building::bare_building_fixture(),
                },
            )
            .expect("首次定义");
        table
            .define(
                tribe,
                CultureAttrs {
                    display_name_key: NamespacedId::parse("test:culture.tribe.display_name")
                        .expect("合法"),
                    economy: crate::resource::ResourceCategory::Timber,
                    home_terrain: ids.hill,
                    wall_terrain: ids.wall_wood,
                    founder_races: vec![(tribe_race, 10)],
                    hostility: vec![(mining, hostility)],
                    buildings: crate::building::bare_building_fixture(),
                },
            )
            .expect("首次定义");
        (
            table,
            [
                CultureKind::from_index(mining),
                CultureKind::from_index(tribe),
            ],
        )
    }

    /// 与 [`chronicle_with`] 相同，但递一张指定的文化表进去。
    fn chronicle_with_cultures(seed: u64, cultures: &CultureTable) -> WorldChronicle {
        let layout = test_layout();
        let params = GenParams {
            seed,
            ..GenParams::default()
        };
        let noise = crate::generate::build_zone_noise(&layout, &params).expect("布局合法");
        let (ids, table) = base_terrain_fixture();
        let (_kinds, resources) = test_resources(&ids);
        WorldChronicle::generate(
            &ChronicleInput {
                layout: &layout,
                noise: &noise,
                params: &params,
                terrain_ids: &ids,
                terrain_table: &table,
                resources: &resources,
                cultures,
            },
            ChronicleParams::default(),
        )
    }

    /// 这部编年史里因战争覆灭的据点有几座。
    fn war_demises(chronicle: &WorldChronicle) -> usize {
        chronicle
            .events()
            .iter()
            .filter(|event| {
                matches!(
                    &event.kind,
                    HistoricalEventKind::SettlementAbandoned(record)
                        if matches!(record.cause, SettlementDemise::War { .. })
                )
            })
            .count()
    }

    /// 这部编年史里发生过几场战争——两种结局都算。**这才是「打了几
    /// 仗」**；[`war_demises`] 只数其中以毁灭收场的那一半。
    fn wars(chronicle: &WorldChronicle) -> usize {
        war_demises(chronicle) + conquests(chronicle).len()
    }

    /// 这部编年史里全部易主事件的可比对摘要。
    fn conquests(chronicle: &WorldChronicle) -> Vec<&SettlementConqueredRecord> {
        chronicle
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                HistoricalEventKind::SettlementConquered(record) => Some(record),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn 空文化表下战争仍然只有毁灭一种结局() {
        // 这条守的是「把改动关掉能精确回到旧行为」：占领改掉的是守方的
        // **文化**，而一个没有文化这一层的世界里没有东西可以易主，因此
        // 那样的世界里每一场战争都只能以毁灭收场——与占领落地之前逐位
        // 相同的行为（见 `EpochRun::occupation_numerator` 文档）。
        //
        // 它取代了此前那条 `全表敌意为零时战争结果与空文化表逐位相同`。
        // **那条性质被本批次真正推翻了，不是测试写坏了**：它断言的是
        // 「文化只经敌意这一个通道影响战争」，而占领是第二条通道——同一
        // 场战争，有文化的世界里可能以易主收场，没文化的世界里只能是
        // 废墟。留着它等于要求本批次不生效。
        // Arrange & Act
        let without = chronicle_with_cultures(0xC0FF_EE12, &CultureTable::new());

        // Assert
        assert!(
            without.sites().iter().all(|site| site.culture.is_none()),
            "空文化表下不该有任何据点带文化"
        );
        assert!(
            war_demises(&without) > 0,
            "本条要有意义，前提是这个世界真的打过仗"
        );
        assert!(
            conquests(&without).is_empty(),
            "没有文化这一层的世界里不该出现任何一次易主，实测 {} 次",
            conquests(&without).len()
        );

        // # 故意改坏的反例（人工核验，真实执行）
        //
        // 把 `occupation_numerator` 里两句 `?` 之后的分支改成「查不到
        // 种族就当作同族」（即 `let a = self.founder_race_of(attacker);
        // let d = self.founder_race_of(defender); Some(if a == d { ... })`
        // ——注意 `None == None` 为真），本条当场红：空文化表的世界里
        // 冒出 6 次易主。恢复后重新跑通。
    }

    /// 一个只用来问 [`EpochRun::occupation_numerator`] 的最小推演器：
    /// 借用测试世界真实勘察出来的前两处候选点（那两处的资源画像与本条
    /// 无关，但 `Candidate` 拿不到别的构造路径），把 0 号与 1 号两座
    /// 据点的文化按参数摆好，其余状态一概不动。
    fn two_site_run(
        cultures: &CultureTable,
        first: Option<CultureKind>,
        second: Option<CultureKind>,
    ) -> EpochRun {
        let layout = test_layout();
        let params = GenParams {
            seed: 0xC0FF_EE12,
            ..GenParams::default()
        };
        let noise = crate::generate::build_zone_noise(&layout, &params).expect("布局合法");
        let (ids, table) = base_terrain_fixture();
        let (_kinds, resources) = test_resources(&ids);
        let chronicle_params = ChronicleParams::default();
        let mut candidates = survey_habitable_zones(
            &layout,
            &noise,
            &params,
            &ids,
            &table,
            &resources,
            chronicle_params,
        );
        assert!(
            candidates.len() >= 2,
            "测试世界至少要勘察出两处候选点，实测 {}",
            candidates.len()
        );
        candidates.truncate(2);
        let mut run = EpochRun::new(
            candidates,
            chronicle_params.epochs,
            params.seed,
            layout.tile_size(),
            NeighbourRanges {
                war: war_range(chronicle_params),
                culture: culture_neighbor_range(chronicle_params),
            },
            resources,
            cultures.clone(),
        );
        for (index, culture) in [first, second].into_iter().enumerate() {
            run.states[index].id = Some(WorldId::next(&mut run.next_world_id));
            run.states[index].population = WAR_MIN_POPULATION;
            run.states[index].culture = culture;
        }
        run
    }

    #[test]
    fn 结局判据只看双方是不是同一个建立者种族() {
        // 这是「旋钮真的接上了」那条断言，且不靠统计——直接问
        // `occupation_numerator` 三种输入下给什么。上面那条
        // `同族更倾向占领异族更倾向毁灭` 数的是它在三百年推演里涌现出
        // 来的后果，两条各守一半。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let (kin, [kin_mining, kin_tribe]) = test_cultures_with_races(&ids, 0, true);
        let (strangers, [far_mining, far_tribe]) = test_cultures_with_races(&ids, 0, false);

        // Act & Assert：① 同族——两条文化的建立者种族是同一个。
        let same = two_site_run(&kin, Some(kin_tribe), Some(kin_mining));
        assert_eq!(
            same.occupation_numerator(0, 1),
            Some(SAME_RACE_OCCUPATION_NUMERATOR),
            "同族的两座据点，这一仗应当**更倾向**以占领收场"
        );

        // ② 异族——两条文化的建立者种族不同。
        let cross = two_site_run(&strangers, Some(far_tribe), Some(far_mining));
        assert_eq!(
            cross.occupation_numerator(0, 1),
            Some(CROSS_RACE_OCCUPATION_NUMERATOR),
            "异族的两座据点，这一仗应当**更倾向**以毁灭收场"
        );
        const {
            assert!(
                SAME_RACE_OCCUPATION_NUMERATOR > CROSS_RACE_OCCUPATION_NUMERATOR,
                "项目所有者的方向：同种族更倾向占领"
            )
        };

        // ③ 没有文化这一层——没有东西可以易主，只可能毁灭。
        let void = two_site_run(&CultureTable::new(), None, None);
        assert_eq!(
            void.occupation_numerator(0, 1),
            None,
            "空文化表下不该有任何占领倾向，哪怕 None == None"
        );
        let half = two_site_run(&kin, Some(kin_tribe), None);
        assert_eq!(
            half.occupation_numerator(0, 1),
            None,
            "只有一侧有文化时同样无从表达「易主」"
        );

        // # 故意改坏的反例（人工核验，真实执行）
        //
        // 把 `occupation_numerator` 里的 `attacker_race == defender_race`
        // 改成 `!=`，①②两条当场红（6 与 1 对调）。把两个 `?` 改成
        // `unwrap_or_default()`，③的第一句当场红（空文化表下返回
        // `Some(6)`）。逐个恢复后重新跑通。
    }

    #[test]
    fn 同族更倾向占领异族更倾向毁灭() {
        // 这条守的是项目所有者给本批次定的方向，逐字：「同种族的话更
        // 倾向于占领而不是毁灭」。
        //
        // 两张表**只差一个数**：两条文化的 `founder_races` 里那个种族
        // 的内容索引是不是同一个。权重结构完全一样（各一条、权重 10），
        // 因此 `founder_race` 消耗的随机数一模一样，抽出的建立者种族
        // 在两边恒是各自表里的那唯一一条——差别只有「这两条是不是同一
        // 族」。文化抽取、敌意、建材、人口曲线一个字节都没变。
        // Arrange：一个 16×16 的测试世界一颗种子只打三五仗，样本太小
        // 会让本条的结论落在噪声里。二十四颗种子合起来数，是「小号样
        // 本」这条既有取舍（见 `test_layout` 文档）在统计上的对应做法。
        //
        // **这条数的是涌现出来的后果，不是判据本身**：判据由
        // `结局判据只看双方是不是同一个建立者种族` 直接问
        // `occupation_numerator` 守着。两边的比例都不会精确等于
        // 6/8 与 1/8——异族那张表里绝大多数战争其实发生在**同文化**的
        // 两座据点之间（文化靠邻居加分连成片，邻居多半跟自己同族），
        // 而同文化必然同族。实测二十四颗种子：同族表 29/41 场以占领
        // 收场，异族表 18/42 场，其中那 18 场里只有 1 场真的跨文化。
        let (ids, _table) = base_terrain_fixture();
        let (kin, _) = test_cultures_with_races(&ids, 0, true);
        let (strangers, _) = test_cultures_with_races(&ids, 0, false);
        let seeds: Vec<u64> = (0..24u64).map(|n| 0xC0FF_EE12 + n).collect();

        // Act
        let mut same_occupied = 0usize;
        let mut same_wars = 0usize;
        let mut cross_occupied = 0usize;
        let mut cross_wars = 0usize;
        for seed in seeds {
            let same = chronicle_with_cultures(seed, &kin);
            let cross = chronicle_with_cultures(seed, &strangers);
            same_occupied += conquests(&same).len();
            same_wars += wars(&same);
            cross_occupied += conquests(&cross).len();
            cross_wars += wars(&cross);
        }

        // Assert
        assert!(
            same_wars > 0 && cross_wars > 0,
            "两边都要真的打过仗，否则本条是空转：同族 {same_wars} 场、异族 {cross_wars} 场"
        );
        // 交叉相乘比较两个比例，不引入浮点（ADR 0020）。
        assert!(
            same_occupied * cross_wars > cross_occupied * same_wars,
            "同族战争以占领收场的比例必须明显高于异族：             同族 {same_occupied}/{same_wars}，异族 {cross_occupied}/{cross_wars}"
        );

        // # 故意改坏的反例（人工核验，真实执行）
        //
        // 把 `occupation_numerator` 的两个分子都改成
        // `SAME_RACE_OCCUPATION_NUMERATOR`（即种族不再改变结局），本条
        // 当场红：两边的占领比例落到同一个量级、不等式不再成立。恢复后
        // 重新跑通。
    }

    #[test]
    fn 被占领的据点活下来且换了主子() {
        // 验收线的单元测试版：据点**不死**，换主人。端到端那一半（那座
        // 城在地上仍然有门有人）在 `crates/ll-game/tests/culture_and_war.rs`。
        //
        // # 为什么要专挑「换掉的文化真的和原来那份不同」的那一次
        //
        // 本条第一版只断言「被占领的据点仍然有人住、且文化等于记录里的
        // `new_culture`」——那条**故意改坏也不会红**：绝大多数战争发生
        // 在同文化的两座据点之间（文化靠邻居加分连成片），此时
        // `former_culture == new_culture`，于是「占了但没换主子」这个
        // 改坏版本照样满足断言。挑一次真的换了文化的占领，这条才咬得住。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let (kin, _) = test_cultures_with_races(&ids, 0, true);

        // Act：扫种子直到找到一次「文化真的换了、而且那座城活到了最后」
        // 的占领。一颗种子上这样的事件是稀有的（十六格见方的测试世界
        // 一共才打三五仗），扫一批是把「小号样本」这条既有取舍补齐。
        let mut checked = 0usize;
        let mut witness = None;
        for seed in (0..24u64).map(|n| 0xC0FF_EE12 + n) {
            let chronicle = chronicle_with_cultures(seed, &kin);
            for record in conquests(&chronicle) {
                checked += 1;
                assert!(
                    record.survivors > 0,
                    "被占领的据点必须活下来——人被打光的城是毁灭，不是占领"
                );
                assert_ne!(record.conqueror, record.site, "一座城不能占领自己");
                assert!(
                    chronicle.events().iter().any(|event| matches!(
                        &event.kind,
                        HistoricalEventKind::SettlementFounded(founded)
                            if founded.site == record.conqueror
                    )),
                    "占领方必须是编年史里真的建立过的一座据点"
                );
                if record.former_culture == record.new_culture {
                    continue;
                }
                let alive = chronicle.sites().iter().find(|site| {
                    site.id == record.site && site.status == SettlementStatus::Inhabited
                });
                if let Some(site) = alive {
                    witness = Some((*record, *site));
                    break;
                }
            }
            if witness.is_some() {
                break;
            }
        }

        // Assert
        assert!(checked > 0, "本条要有意义，前提是这批世界真的出现过易主");
        let (record, site) = witness.expect(
            "应当至少有一次「归属真的换了、而且那座城活到了最后」的占领——             这是本批次验收线的前半句",
        );
        assert_eq!(
            site.culture.map(CultureKind::index),
            Some(record.new_culture),
            "活下来的那座城，文化必须已经换成占领方的那一份"
        );
        assert_ne!(
            site.culture.map(CultureKind::index),
            Some(record.former_culture),
            "它不该还信着易主之前那一份"
        );
        assert!(site.population > 0, "它必须还有人");
        assert_eq!(
            site.abandoned_epoch, None,
            "被占领**不是**被遗弃：最终快照里它不该带遗弃纪元"
        );
        assert!(
            site.founded_epoch <= record.epoch,
            "它的建立纪元不该被易主改写——同一座城换了主子，不是重建"
        );

        // # 故意改坏的反例（人工核验，真实执行）
        //
        // 把 `EpochRun::occupy` 里的 `self.states[defender].culture =
        // Some(new);` 那一行删掉（也就是「占了但没换主子」），本条当场
        // 红：`witness` 找不到任何一次。恢复后重新跑通。
        //
        // 另一处：把 `occupy` 里的 `survivors` 改成 `0`，第一条断言当场
        // 红。恢复后重新跑通。
    }

    #[test]
    fn 敌意抬高了开战次数() {
        // Arrange：两张表只差一个数——「部落 → 矿业」的敌意分。
        let (ids, _table) = base_terrain_fixture();
        let (peaceful, _) = test_cultures(&ids, 0);
        let (hostile, _) = test_cultures(&ids, crate::culture::MAX_HOSTILITY);

        // Act：同一颗种子跑两遍。
        let calm = wars(&chronicle_with_cultures(0xC0FF_EE12, &peaceful));
        let bloody = wars(&chronicle_with_cultures(0xC0FF_EE12, &hostile));

        // Assert：数的是**开战次数**（两种结局都算），不是其中以毁灭
        // 收场的那一半——占领落地之后，只数覆灭会把「敌意抬高了开战
        // 概率」与「这些仗怎么收场」两件事混在一个数里，本条就不再是
        // 在测敌意了。
        assert!(
            bloody > calm,
            "敌意应当抬高开战概率：平和 {calm} 场，敌对 {bloody} 场"
        );

        // # 故意改坏的反例（人工核验，真实执行）
        //
        // 把 `wage_wars` 的 `rng.chance(WAR_NUMERATOR + hostility, ..)`
        // 改回 `rng.chance(WAR_NUMERATOR, ..)`（也就是敌对不接线），
        // 本条当场红：平和 3 场、敌对也是 3 场。恢复后重新跑通。
    }

    #[test]
    fn 文化抽取跟着资源走() {
        // Arrange：矿业文化守金属、住山里；部落守木材、住丘陵。
        let (ids, _table) = base_terrain_fixture();
        let (cultures, [mining, tribe]) = test_cultures(&ids, 0);
        let (_kinds, resources) = test_resources(&ids);

        // Act：单颗种子的测试世界（16×16 区块）里守着金属的据点只有
        // 个位数，样本太小；四颗种子累计起来才谈得上「更可能」。
        let (mut metal_mining, mut metal_tribe) = (0usize, 0usize);
        let (mut timber_mining, mut timber_tribe) = (0usize, 0usize);
        for seed in [0xC0FF_EE12u64, 0x1234_5678, 7, 99] {
            let chronicle = chronicle_with_cultures(seed, &cultures);
            for site in chronicle.sites() {
                let (Some(primary), Some(culture)) = (site.resource_profile[0], site.culture)
                else {
                    continue;
                };
                match resources.category(primary) {
                    Some(crate::resource::ResourceCategory::Metal) if culture == mining => {
                        metal_mining += 1
                    }
                    Some(crate::resource::ResourceCategory::Metal) if culture == tribe => {
                        metal_tribe += 1
                    }
                    Some(crate::resource::ResourceCategory::Timber) if culture == mining => {
                        timber_mining += 1
                    }
                    Some(crate::resource::ResourceCategory::Timber) if culture == tribe => {
                        timber_tribe += 1
                    }
                    _ => {}
                }
            }
        }

        // Assert：先确认样本非空（否则下面两条是空转），再比大小。
        // 守着金属的据点更可能是矿业文化，守着木材的更可能是部落——
        // 这正是项目所有者定的第一条依据「资源」。
        assert!(
            metal_mining + metal_tribe > 0 && timber_mining + timber_tribe > 0,
            "四颗种子里既没有守金属的据点也没有守木材的据点，本条测试是空转"
        );
        assert!(
            metal_mining > metal_tribe,
            "守着金属的据点里矿业文化 {metal_mining} 应多于部落 {metal_tribe}"
        );
        assert!(
            timber_tribe > timber_mining,
            "守着木材的据点里部落 {timber_tribe} 应多于矿业文化 {timber_mining}"
        );
    }

    #[test]
    fn 同一颗种子的文化派生逐位相同() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let (cultures, _) = test_cultures(&ids, 3);

        // Act
        let first = chronicle_with_cultures(0x1234_5678, &cultures);
        let second = chronicle_with_cultures(0x1234_5678, &cultures);

        // Assert：文化是派生量（ADR 0009），读档时靠这条性质重算出
        // 逐位相同的结果。
        assert_eq!(first.sites().len(), second.sites().len());
        for (a, b) in first.sites().iter().zip(second.sites()) {
            assert_eq!(a.culture, b.culture, "同一种子的文化派生出现分歧");
        }
    }

    /// 把六样东西拼成一个不带文化的 [`ChronicleInput`]——只关心资源
    /// 那一层的几条测试用它，省得每处都写一遍空文化表。
    fn chronicle_input<'a>(
        layout: &'a ZoneLayout,
        noise: &'a TileableNoise,
        params: &'a GenParams,
        terrain_ids: &'a BaseTerrainIds,
        terrain_table: &'a TerrainTable,
        resources: &'a ResourceTable,
    ) -> ChronicleInput<'a> {
        ChronicleInput {
            layout,
            noise,
            params,
            terrain_ids,
            terrain_table,
            resources,
            cultures: EMPTY_CULTURES.get_or_init(CultureTable::new),
        }
    }

    /// [`chronicle_input`] 用的那张常驻空文化表。
    static EMPTY_CULTURES: std::sync::OnceLock<CultureTable> = std::sync::OnceLock::new();

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
        let (_kinds, resources) = test_resources(&ids);
        let defaults = ChronicleParams::default();
        let survey =
            || survey_habitable_zones(&layout, &noise, &params, &ids, &table, &resources, defaults);

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

    /// 倒排索引与逐栋建筑算出的覆盖清单必须一致——
    /// 是流式加载路径上唯一的「这个区块要铺什么」真相源，它一旦漏掉
    /// 某个区块，那半条街就永远不会出现在地上。
    #[test]
    fn 覆盖索引与逐栋建筑算出的清单一致() {
        // Arrange
        let layout = test_layout();
        let chronicle = chronicle_for(0xC0FF_EE12);
        assert!(
            !chronicle.sites().is_empty(),
            "没有据点，这条断言测不到什么"
        );

        // Act & Assert：每座据点覆盖到的每个区块，都要能从索引里查回它。
        for site in chronicle.sites() {
            for zone in crate::settlement::footprint_zones(site, &layout) {
                assert!(
                    chronicle
                        .sites_touching_zone(zone)
                        .any(|found| found.id == site.id),
                    "据点 {:?} 铺到了区块 {zone:?}，索引却查不到它",
                    site.id
                );
            }
        }

        // 反向：索引查出来的据点，必然真的覆盖到那个区块。
        let zone_count = layout.zone_count();
        for zone_y in 0..zone_count.height() as i32 {
            for zone_x in 0..zone_count.width() as i32 {
                let zone = zone_count.wrap(zone_x, zone_y);
                for site in chronicle.sites_touching_zone(zone) {
                    assert!(
                        crate::settlement::footprint_zones(site, &layout).contains(&zone),
                        "索引说据点 {:?} 铺到了区块 {zone:?}，实际没有",
                        site.id
                    );
                }
            }
        }
    }

    /// 锚点所在的区块必然在覆盖清单里——第 0 栋建筑就在锚点上。
    #[test]
    fn 每座据点至少覆盖自己锚点所在的区块() {
        // Arrange
        let chronicle = chronicle_for(0xC0FF_EE12);

        // Act & Assert
        for site in chronicle.sites() {
            assert!(
                chronicle
                    .sites_touching_zone(site.zone)
                    .any(|found| found.id == site.id),
                "据点 {:?} 查不到自己的锚点区块",
                site.id
            );
        }
    }

    /// 资源真的在改变结果：同一张地形、同一个种子，换一张空资源表
    /// 之后据点规模必然缩水（少了资源那部分承载力）。
    ///
    /// 没有这条对照，「选址与规模考虑了资源」只是一句说明——它可能整条
    /// 接错了地方而没有任何断言会红。
    #[test]
    fn 资源丰富的世界据点规模明显更大() {
        // Arrange
        let layout = test_layout();
        let params = GenParams {
            seed: 0xC0FF_EE12,
            ..GenParams::default()
        };
        let noise = crate::generate::build_zone_noise(&layout, &params).expect("布局合法");
        let (ids, table) = base_terrain_fixture();
        let (_kinds, resources) = test_resources(&ids);
        let empty = ResourceTable::new();
        let defaults = ChronicleParams::default();

        // Act
        let with_resources = WorldChronicle::generate(
            &chronicle_input(&layout, &noise, &params, &ids, &table, &resources),
            defaults,
        );
        let without = WorldChronicle::generate(
            &chronicle_input(&layout, &noise, &params, &ids, &table, &empty),
            defaults,
        );

        // Assert
        let buildings = |chronicle: &WorldChronicle| -> u64 {
            chronicle
                .sites()
                .iter()
                .map(|site| u64::from(site.building_count))
                .sum()
        };
        let rich = buildings(&with_resources);
        let poor = buildings(&without);
        assert!(
            rich > poor,
            "有资源的世界总建筑数 {rich} 不多于没资源的 {poor}，资源没有接进承载力"
        );
    }

    /// 空资源表是合法输入：那等于「这个世界没有资源这一层」，选址与
    /// 承载力退回到只看陆地面积，不 panic、也不静默变成别的行为。
    #[test]
    fn 空资源表下仍然产出据点() {
        // Arrange
        let layout = test_layout();
        let params = GenParams {
            seed: 0xC0FF_EE12,
            ..GenParams::default()
        };
        let noise = crate::generate::build_zone_noise(&layout, &params).expect("布局合法");
        let (ids, table) = base_terrain_fixture();
        let empty = ResourceTable::new();

        // Act
        let chronicle = WorldChronicle::generate(
            &chronicle_input(&layout, &noise, &params, &ids, &table, &empty),
            ChronicleParams::default(),
        );

        // Assert
        assert!(!chronicle.sites().is_empty());
        assert!(!chronicle.events().is_empty());
    }

    /// 三种新覆灭原因**真的会发生**——项目所有者点名的「资源 / 打仗 /
    /// 疾病」三条，各自至少在这批种子里出现过一次。
    ///
    /// # 为什么是「四十八个种子合起来至少各一次」而不是「每个种子各一次」
    ///
    /// 三者都是低概率事件，而测试世界（16×16 个区块）只有本体默认世界
    /// （64×48）的十二分之一大：本体世界一局里瘟疫覆灭一两次，按比例
    /// 折算到测试世界就是每十几局才有一次。逐种子断言会让这条测试变成
    /// 掷硬币；把种子数开到四十八、合起来断言，仍然能抓住「某条原因整条
    /// 接错了、永远走不到」这个真正要防的缺陷——那才是这条测试的靶子。
    ///
    /// 四十八局 16×16 的推演在 debug 下合计不到一秒，代价可以接受。
    #[test]
    fn 推演里三种新覆灭原因都真的发生过() {
        // Arrange
        let mut saw_resource = false;
        let mut saw_war = false;
        let mut saw_plague = false;

        // Act
        for seed in 1..=48u64 {
            for event in chronicle_for(seed).events() {
                let HistoricalEventKind::SettlementAbandoned(record) = &event.kind else {
                    continue;
                };
                match record.cause {
                    SettlementDemise::Depopulation => {}
                    SettlementDemise::ResourceExhausted { .. } => saw_resource = true,
                    SettlementDemise::War { .. } => saw_war = true,
                    SettlementDemise::Plague { .. } => saw_plague = true,
                }
            }
        }

        // Assert
        assert!(saw_resource, "四十八个种子里一次资源枯竭导致的覆灭都没有");
        assert!(saw_war, "四十八个种子里一次战争导致的覆灭都没有");
        assert!(saw_plague, "四十八个种子里一次瘟疫导致的覆灭都没有");
    }

    /// 战争事件记下的攻方必须是**另一座真实存在过的据点**——「谁灭的」
    /// 这条因果要能顺着号码查回去，否则编年史里只剩一句「它被打没了」。
    #[test]
    fn 战争覆灭记下的攻方是另一座真实据点() {
        // Arrange：把全部被分配过的据点号收进一个集合（含已经变成废墟
        // 的——攻方后来自己也可能覆灭）。
        let chronicle = chronicle_for(2);
        let mut known: Vec<WorldId> = Vec::new();
        for event in chronicle.events() {
            if let HistoricalEventKind::SettlementFounded(record) = &event.kind {
                known.push(record.site);
            }
        }

        // Act & Assert
        let mut checked = 0usize;
        for event in chronicle.events() {
            let HistoricalEventKind::SettlementAbandoned(record) = &event.kind else {
                continue;
            };
            let SettlementDemise::War { aggressor } = record.cause else {
                continue;
            };
            checked += 1;
            assert_ne!(aggressor, record.site, "一座据点把自己打没了");
            assert!(
                known.contains(&aggressor),
                "攻方 {aggressor:?} 不是任何一座建立过的据点"
            );
        }
        assert!(checked > 0, "这个种子里没有战争，这条断言测不到什么");
    }

    /// 资源枯竭的覆灭必须指名道姓说出**哪一种**资源，而且那种资源必须
    /// 真的是可枯竭的。
    #[test]
    fn 枯竭覆灭指向一种真的可枯竭的资源() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let (_kinds, resources) = test_resources(&ids);

        // Act & Assert
        let mut checked = 0usize;
        for seed in [1u64, 2, 3, 7, 0xC0FF_EE12] {
            for event in chronicle_for(seed).events() {
                let HistoricalEventKind::SettlementAbandoned(record) = &event.kind else {
                    continue;
                };
                let SettlementDemise::ResourceExhausted { resource } = record.cause else {
                    continue;
                };
                checked += 1;
                let kind = crate::resource::ResourceKind::from_index(resource);
                assert!(
                    resources.is_defined(resource),
                    "枯竭事件指向一个没注册过的资源索引"
                );
                assert!(
                    resources.exhaustible(kind),
                    "枯竭事件指向一种根本不会枯竭的资源"
                );
            }
        }
        assert!(checked > 0, "五个种子里一次枯竭都没有，这条断言测不到什么");
    }

    /// 领地口径的承载力真的比旧的「一个区块窗口」口径宽——`territory_radius`
    /// 由最小间距推出，不是一个可以独立调的数值。
    #[test]
    fn 领地半径由最小间距推出且不小于半个区块() {
        // Arrange & Act & Assert
        let defaults = ChronicleParams::default();
        assert_eq!(territory_radius(defaults, 48), 72);

        // 间距为 0（不筛）时退回半个区块，领地仍然是一个可算的东西。
        let unfiltered = ChronicleParams {
            min_settlement_spacing: 0,
            ..defaults
        };
        assert_eq!(territory_radius(unfiltered, 48), 24);
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
