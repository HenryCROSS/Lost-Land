//! 资源点：一块地上「有什么可拿」的那一层，与地形同源、完全派生。
//!
//! # 项目所有者的裁决
//!
//! > 「生成的时候还需要**考虑资源点的分布**，我不清楚你有没有添加资源
//! > 点的设定」
//!
//! **核实结论：此前确实没有。** 本模块之前，全仓库对「资源点」这个概念
//! 零命中；唯一沾边的 `SpaceProfile.diggable` 是一个只被哈希与审计机械
//! 读取、没有任何玩法消费者的死字段。本模块是这套设定的第一次落地。
//!
//! # 形状：一个可查询的纯函数，不是一份要存起来的清单
//!
//! 一处资源点是不是在某一格上，答案由 `f(世界种子, 坐标, 资源种类)`
//! 直接算出（[`resource_node_at`]），与地形本身是同一条纪律
//! （ADR 0009「默认派生，只存偏差」、决策 0005「地形是坐标的纯函数」）：
//!
//! - **不进存档**。整层资源分布是种子的纯函数，读档时重新派生即可。
//! - **不预先物化**。没有一个「全世界资源点表」需要在建档时铺满内存；
//!   谁要问哪一格，谁现算——一次两次整数混合的代价。
//! - **不与地形争一格的所有权**。资源点是叠在地形之上的一层判定，
//!   `ChunkGrid` 每格仍然只有一个 [`TerrainKind`]，寻路/FOV/存档
//!   remap/内容哈希**一样都不用改**（这正是 `settlements-structures-
//!   and-npc-spawning.md` 三节否决 `StructureKind` 时用的同一条论证）。
//!
//! # 为什么资源种类是一张**内容表**，而不是地形上的一个字段
//!
//! ADR 0021 的判据是「有没有一份算法要被共用」，双向成立。逐条问过：
//!
//! - **做成 [`TerrainDef`](crate::terrain::TerrainDef) 的一个字段行不
//!   行？** 不行，会丢掉身份。一座山既出铁也出石料，一句「山地的资源
//!   产出是 X」表达不了两种；而编年史要写下的是「这座城死于**铁矿**
//!   枯竭」——那需要一个能被 `ContentIndex` 指着的东西，地形字段给不
//!   出来。仓库已经为「把两个概念挤进同一个字段」付过代价
//!   （ADR 0010、`Affiliation.org`）。
//! - **直接指向物品（`lostland:iron_ingot` 一类）行不行？** 不行，而且
//!   这条比上一条更清楚。物品自带一整套算法：堆叠合并、耐久、装备槽、
//!   背包并入、地面老化、配方原料。资源种类**一条都用不上**——它是
//!   「这片地每千格有多少个矿脉」这样一个标量的持有者。反过来，把两者
//!   合并也消不掉任何重复逻辑：没有任何一段代码会写成「给我一个既是
//!   物品又是资源的东西」。按 ADR 0021，没有要共用的算法就不合并。
//!   更实际的一条：`lostland:farmland`（良田）**根本没有对应物品**，
//!   而 `lostland:timber`（木材）对应的是原木/木板/木炭一族而不是某一
//!   件——硬要一一对应就得凭空发明谁也拿不起来的占位物品。
//! - **那这张表会不会是「为对称而抽象」？** 不会。它的每一个字段都在
//!   本批次就有真实消费者（见 [`ResourceAttrs`] 逐字段文档的「谁读
//!   它」），没有一个是声明先行。
//!
//! **「资源点产出哪件物品」这条链留到将来**：它要等采集真的落地
//! （`settlements-structures-and-npc-spawning.md` 十一节「将来扩展 ④」
//! 的挖矿，走 `RecipeDef.station_becomes` 那条路），那时候加一个
//! `yields_item` 字段是一行的事。现在加，就是又一个 `diggable`。
//!
//! # 谁在读这一层
//!
//! [`crate::chronicle`] 的据点推演，三处：
//!
//! 1. **选址**：守着铁矿的地方更容易建城
//!    （[`ResourceAttrs::settlement_draw`]）。
//! 2. **规模**：一片地能养活多少人，资源说了算
//!    （[`ResourceAttrs::residents_supported`]）——这是「有的据点有大有
//!    小」的来源之一。
//! 3. **覆灭**：可枯竭的资源采光了，靠它立足的城随之衰败
//!    （[`ResourceAttrs::exhaustible`]）。
//!
//! # 确定性（约束 C3 / C5）
//!
//! - 唯一的随机来源是 [`resource_node_at`] 里那一次
//!   `DetRng::for_entity`，三元组是「世界种子 / 流编号 + 资源索引 /
//!   瓦片光栅键」——与调用顺序、调用次数完全无关，同一格问一万次得到
//!   同一个答案。
//! - [`survey_resources`] 的采样按 `(dy, dx)` 光栅序，资源按
//!   [`ResourceTable::registered`] 的注册顺序，全程只用 `Vec`，不触碰
//!   任何 `HashMap`/`HashSet` 的迭代顺序。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::rng::DetRng;
use ll_core::torus::{TorusPos, TorusSize};

use crate::generate::{GenParams, terrain_at_tile};
use crate::noise::TileableNoise;
use crate::terrain::{BaseTerrainIds, TerrainKind, TerrainTable};

/// 资源点判定所用的随机流基编号——与
/// [`crate::chronicle::CHRONICLE_STREAM_ID`]（历史推演）、
/// [`crate::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]（建筑铺法）分开，
/// 三者互不干扰：改动某一层不会连带改掉另外两层。
///
/// 实际喂给 `DetRng::for_entity` 的第二个参数是**本值加上资源自身的
/// 内容索引**，因此同一格上不同资源的判定互相独立，不会出现「有铁矿
/// 的地方必然也有木材」这种可察觉的关联。
pub const RESOURCE_NODE_STREAM_ID: u64 = 0x0052_4553_4F55_0001;

/// [`ResourceAttrs::abundance`] 的分母：千分比。
pub const ABUNDANCE_SCALE: u32 = 1000;

/// 一种资源的内容索引包装——与 [`TerrainKind`] 同一种手法（避免把
/// 「一个地形索引」与「一个资源索引」在类型上混为一谈）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceKind(ContentIndex);

impl ResourceKind {
    /// 从一个已经 `intern` 出来的内容索引构造。
    pub const fn from_index(index: ContentIndex) -> Self {
        ResourceKind(index)
    }

    /// 取回内部的内容索引。
    pub const fn index(self) -> ContentIndex {
        self.0
    }
}

/// 一条资源种类声明——本体与 mod 注册资源时共用的同一个输入形状
/// （「本体即 Mod」，ADR 0018）。
///
/// 每个字段的「谁读它」都在自己的文档里，没有一个是声明先行——见模块
/// 文档「那这张表会不会是『为对称而抽象』」一节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceAttrs {
    /// 展示名的本地化键，不存字面字符串——与
    /// [`crate::weather::WeatherAttrs::display_name_key`] 同一条既有
    /// 惯例。**谁读它**：编年史的覆灭原因要能说出「死于**铁矿**枯竭」
    /// （[`crate::history::SettlementDemise::ResourceExhausted`]），
    /// 呈现层由这个键取名字。
    pub display_name_key: NamespacedId,
    /// 这种资源长在哪种地形上。
    ///
    /// **谁读它**：[`resource_node_at`] 的第一道筛——地形不对，这一格
    /// 不可能有这种资源。这条让资源分布**跟着地形走**而不是自成一张
    /// 互不相干的噪声图：山里才有矿，森林里才有木材。
    pub source_terrain: TerrainKind,
    /// 源地形上每一格出现一处资源点的概率，千分比
    /// （分母 [`ABUNDANCE_SCALE`]）。
    ///
    /// **谁读它**：[`resource_node_at`] 的第二道筛。取值让「资源点」
    /// 真的是**点**而不是「整片山都是矿」——铁矿取 60‰ 意味着一片山地
    /// 里大约十六格才有一处矿脉。
    pub abundance: u32,
    /// 每一处这种资源点能额外养活多少居民。
    ///
    /// **谁读它**：[`crate::chronicle`] 的承载力
    /// （`EpochRun::advance_settled`）。这是「有的据点有大有小」的主要
    /// 来源：守着水源与良田的地方能长成城，光秃秃的高地只够一个营地。
    pub residents_supported: u32,
    /// 每一处这种资源点给「在这里建城」的概率加多少分。
    ///
    /// **谁读它**：[`crate::chronicle`] 的拓荒判定（`EpochRun::try_found`）。
    /// 铁矿给的分最高——财富吸引人来，哪怕这地方不好住。
    pub settlement_draw: u32,
    /// 这种资源会不会被采光。
    ///
    /// **谁读它**：[`crate::chronicle`] 的枯竭推演。可枯竭的资源被采
    /// 光之后，它贡献的那部分承载力消失，靠它立足的据点随之衰败——
    /// 这是项目所有者点名的三种覆灭原因之一（`资源枯竭`）。良田/木材/
    /// 水源是可再生的（`false`），矿脉不是（`true`）。
    pub exhaustible: bool,
}

/// 资源注册期可能出现的错误。ADR 0017「注册期完整校验」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceError {
    /// 同一个内容索引被定义了两次——纪律同
    /// [`crate::weather::WeatherError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// [`ResourceAttrs::abundance`] 超出 `1..=ABUNDANCE_SCALE`。
    ///
    /// 下界卡在 1 而不是 0：丰度为 0 的资源永远不会出现在任何一格上，
    /// 那是一条**写了等于没写**的声明，比起静默地什么都不做，注册期
    /// 当场拒掉更能让内容作者看见自己漏填了什么。
    AbundanceOutOfRange(u32),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceError::DuplicateDefinition(index) => {
                write!(f, "资源索引 {} 被重复定义", index.get())
            }
            ResourceError::AbundanceOutOfRange(value) => {
                write!(
                    f,
                    "资源丰度 {value} 超出 1..={ABUNDANCE_SCALE} 的合法千分比范围"
                )
            }
        }
    }
}

impl std::error::Error for ResourceError {}

/// 资源的列式存储：按 [`ContentIndex`] 下标索引（ADR 0017），形状照抄
/// [`crate::weather::WeatherTable`]——包括那份 [`Self::registered`]
/// 注册顺序列表，理由也逐字相同：资源是需要**遍历全表**的内容表
/// （每一格都要问「有哪些资源可能长在这里」），而 `defined` 位图的
/// 下标顺序会随「同一次装载里别的表 intern 了多少条」漂移。
#[derive(Debug, Default, Clone)]
pub struct ResourceTable {
    display_name_key: Vec<Option<NamespacedId>>,
    source_terrain: Vec<Option<TerrainKind>>,
    abundance: Vec<u32>,
    residents_supported: Vec<u32>,
    settlement_draw: Vec<u32>,
    exhaustible: Vec<bool>,
    defined: Vec<bool>,
    order: Vec<ResourceKind>,
}

impl ResourceTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上资源属性。
    ///
    /// # 校验（ADR 0017「注册期完整校验」）
    ///
    /// 1. **不得重复定义**——见 [`ResourceError::DuplicateDefinition`]。
    /// 2. **丰度必须落在 `1..=1000`**——见
    ///    [`ResourceError::AbundanceOutOfRange`]。
    ///
    /// 另外三个数值字段不校验：任意 `u32` 都是合法取值（含 0，表示
    /// 「这种资源不贡献承载力/不吸引拓荒」，是有意义的答案）。
    pub fn define(
        &mut self,
        index: ContentIndex,
        attrs: ResourceAttrs,
    ) -> Result<(), ResourceError> {
        if !(1..=ABUNDANCE_SCALE).contains(&attrs.abundance) {
            return Err(ResourceError::AbundanceOutOfRange(attrs.abundance));
        }

        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.source_terrain.resize(new_len, None);
            self.abundance.resize(new_len, 0);
            self.residents_supported.resize(new_len, 0);
            self.settlement_draw.resize(new_len, 0);
            self.exhaustible.resize(new_len, false);
        }

        if self.defined[idx] {
            return Err(ResourceError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.source_terrain[idx] = Some(attrs.source_terrain);
        self.abundance[idx] = attrs.abundance;
        self.residents_supported[idx] = attrs.residents_supported;
        self.settlement_draw[idx] = attrs.settlement_draw;
        self.exhaustible[idx] = attrs.exhaustible;
        self.order.push(ResourceKind::from_index(index));
        Ok(())
    }

    /// 给定索引当前是否已经登记为一种资源。
    pub fn is_defined(&self, index: ContentIndex) -> bool {
        self.defined
            .get(index.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 全部已注册资源，**按注册顺序**——遍历唯一允许的来源（约束 C5），
    /// 理由见类型文档。
    pub fn registered(&self) -> &[ResourceKind] {
        &self.order
    }

    /// 展示名的本地化键。返回 `Option` 只是因为列式存储需要一个「这一格
    /// 还没被定义」的表示，理由同
    /// [`crate::weather::WeatherTable::display_name_key`]。
    pub fn display_name_key(&self, kind: ResourceKind) -> Option<NamespacedId> {
        self.display_name_key
            .get(kind.index().get() as usize)
            .cloned()
            .flatten()
    }

    /// 这种资源长在哪种地形上。未登记索引返回 `None`（不长在任何地形
    /// 上——安全侧：坏数据不该凭空产出资源）。
    pub fn source_terrain(&self, kind: ResourceKind) -> Option<TerrainKind> {
        self.source_terrain
            .get(kind.index().get() as usize)
            .copied()
            .flatten()
    }

    /// 源地形上每格出现的概率，千分比。未登记索引兜底为 0（永不出现，
    /// 同上条的安全侧取舍）。
    pub fn abundance(&self, kind: ResourceKind) -> u32 {
        self.abundance
            .get(kind.index().get() as usize)
            .copied()
            .unwrap_or(0)
    }

    /// 每处资源点额外养活多少居民。未登记索引兜底为 0。
    pub fn residents_supported(&self, kind: ResourceKind) -> u32 {
        self.residents_supported
            .get(kind.index().get() as usize)
            .copied()
            .unwrap_or(0)
    }

    /// 每处资源点给拓荒概率加多少分。未登记索引兜底为 0。
    pub fn settlement_draw(&self, kind: ResourceKind) -> u32 {
        self.settlement_draw
            .get(kind.index().get() as usize)
            .copied()
            .unwrap_or(0)
    }

    /// 这种资源会不会被采光。未登记索引兜底为 `false`（不枯竭——安全
    /// 侧：坏数据不该凭空灭掉一座城）。
    pub fn exhaustible(&self, kind: ResourceKind) -> bool {
        self.exhaustible
            .get(kind.index().get() as usize)
            .copied()
            .unwrap_or(false)
    }
}

/// 查询资源点所需的、与「问哪一格」无关的那一组输入。
///
/// 打包成一个结构体而不是散着传八个参数：与
/// [`crate::settlement::StampContext`] 完全同一种处理，理由也逐字相同
/// ——这几项**恒一起出现**（离了任何一项都答不出「这格有没有矿」），
/// 拆开传只会让调用点更容易漏配，而且会直接撞上
/// `clippy::too_many_arguments`。
pub struct ResourceContext<'a> {
    /// 世界种子，喂给 [`RESOURCE_NODE_STREAM_ID`] 的随机流。
    pub seed: u64,
    /// 地形噪声源——资源点判定要先知道这一格的**基础地形**。
    pub noise: &'a TileableNoise,
    /// 地形生成参数，与 `noise` 配套。
    pub params: &'a GenParams,
    /// 本体基础地形的内容索引。
    pub terrain_ids: &'a BaseTerrainIds,
    /// 地形属性表——[`survey_resources`] 顺带数陆地时的 `blocks_move`
    /// 判定用。
    pub terrain_table: &'a TerrainTable,
    /// 当前会话注册的资源种类表。
    pub resources: &'a ResourceTable,
    /// 世界瓦片尺寸——瓦片光栅键与环面环绕用。
    pub tile_size: TorusSize,
}

/// 这一格上有没有一处这种资源的资源点。
///
/// 两道筛，都是纯函数：
///
/// 1. 这一格的**基础地形**（噪声算出来的那一层，不含据点等后续写入）
///    必须等于 [`ResourceAttrs::source_terrain`]。
/// 2. 按 [`ResourceAttrs::abundance`] 掷一次由
///    `(世界种子, RESOURCE_NODE_STREAM_ID + 资源索引, 瓦片光栅键)`
///    三元组完全确定的骰子。
///
/// 「掷骰」这个词容易引起误会：**这里没有任何随机流状态**。同一格
/// 问一万次得到同一个答案，问的顺序也完全不影响结果（约束 C3）。
pub fn resource_node_at(ctx: &ResourceContext<'_>, pos: TorusPos, kind: ResourceKind) -> bool {
    let Some(source) = ctx.resources.source_terrain(kind) else {
        return false;
    };
    if terrain_at_tile(ctx.noise, ctx.params, pos, ctx.terrain_ids) != source {
        return false;
    }
    node_roll(ctx, pos, kind)
}

/// [`resource_node_at`] 的第二道筛，拆出来是为了让
/// [`survey_resources`] 能在**已经取过地形**的情况下复用它，不必为了
/// 同一格重新算一遍噪声。
fn node_roll(ctx: &ResourceContext<'_>, pos: TorusPos, kind: ResourceKind) -> bool {
    let abundance = ctx.resources.abundance(kind);
    if abundance == 0 {
        return false;
    }
    let tile_key =
        u64::from(pos.y() as u32) * u64::from(ctx.tile_size.width()) + u64::from(pos.x() as u32);
    let stream = RESOURCE_NODE_STREAM_ID.wrapping_add(u64::from(kind.index().get()));
    DetRng::for_entity(ctx.seed, stream, tile_key).chance(abundance, ABUNDANCE_SCALE)
}

/// 一处候选点周边的资源清点结果——[`survey_resources`] 的产出。
///
/// 不派生 `Copy`：条目数随注册的资源种类数变化，是一个 `Vec`。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceSurvey {
    counts: Vec<ResourceCount>,
    land_samples: u32,
    stride_area: u32,
}

/// 一种资源在一处领地内被数到多少处。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceCount {
    /// 哪种资源。
    pub kind: ResourceKind,
    /// 数到多少处资源点（**采样口径**，见 [`survey_resources`] 文档
    /// 「数出来的是采样点不是真实格数」）。
    pub nodes: u32,
}

impl ResourceSurvey {
    /// 清点结果，按 [`ResourceTable::registered`] 的注册顺序；数到 0 处
    /// 的资源**不出现在这里**（绝大多数领地只有一两种资源，留着一长串
    /// 零条目只会让下游每次都要跳过它们）。
    pub fn counts(&self) -> &[ResourceCount] {
        &self.counts
    }

    /// 领地内可行走陆地的**估算格数**——采样命中数乘上每个采样点代表
    /// 的格数。
    ///
    /// # 这个字段为什么长在资源勘察上
    ///
    /// 因为它是同一次采样白拿的：判断「这一格有没有铁矿」本来就要先
    /// 取一次地形，顺手数一下这格能不能走，代价是一次布尔判断。分成
    /// 两趟扫描等于把整个领地的噪声采样做两遍——那是本模块最贵的一步。
    ///
    /// 消费者是 [`crate::chronicle`] 的承载力：它此前按**一个区块窗口**
    /// （至多 2304 格）的陆地面积算，而据点实际拥有的是一片
    /// `min_settlement_spacing` 见方的领地，那个旧口径是单区块约束留下
    /// 的痕迹。
    pub fn land_area(&self) -> u32 {
        self.land_samples.saturating_mul(self.stride_area)
    }

    /// 某种资源数到多少处；没数到返回 0。
    ///
    /// 线性扫描而不是查表：条目数是「注册了几种资源」这个个位数量级，
    /// 而线性扫描不引入任何哈希容器（约束 C5）。
    pub fn nodes_of(&self, kind: ResourceKind) -> u32 {
        self.counts
            .iter()
            .find(|count| count.kind == kind)
            .map_or(0, |count| count.nodes)
    }
}

/// 以 `anchor` 为中心、边长 `2 * radius + 1` 的一片领地内，每隔
/// `stride` 格采一个样，清点各种资源各有多少处。
///
/// # 数出来的是采样点，不是真实格数
///
/// 每 `stride × stride` 格只看一个点。返回的 `nodes` 因此是**采样口径
/// 的处数**，不是「领地里真的有这么多处矿脉」——它是一个与真实密度成
/// 正比、且完全确定的估计量。
///
/// **为什么不逐格数。** 一片 144×144 的领地是 20736 格，逐格数要为每
/// 格算一次噪声（四层倍频）；两百多座据点就是四百多万次。采样把它压到
/// 每座三百多次，代价是估计量的方差——而下游要的本来就是「这地方富不
/// 富」这个量级的判断，不是精确到个位的矿脉数。
///
/// # 确定性
///
/// 采样点按 `(dy, dx)` 光栅序、资源按注册顺序，全程只用 `Vec`；每一格
/// 的判定是 [`resource_node_at`] 那个与顺序无关的纯函数。同一组输入恒
/// 产出逐字段相同的结果（约束 C3 / C5）。
///
/// # panic
///
/// `stride` 为 0 时 panic——那会让采样循环永不前进。这是调用方的编程
/// 错误（`stride` 是代码里的常量，不是内容数据），不是运行期可能出现
/// 的输入。
pub fn survey_resources(
    ctx: &ResourceContext<'_>,
    anchor: TorusPos,
    radius: i32,
    stride: i32,
) -> ResourceSurvey {
    assert!(stride > 0, "采样步长必须为正，否则采样循环永不前进");
    let registered = ctx.resources.registered();
    let mut counts: Vec<ResourceCount> = registered
        .iter()
        .map(|kind| ResourceCount {
            kind: *kind,
            nodes: 0,
        })
        .collect();
    let mut land_samples = 0u32;

    let mut dy = -radius;
    while dy <= radius {
        let mut dx = -radius;
        while dx <= radius {
            let pos = ctx.tile_size.wrap(anchor.x() + dx, anchor.y() + dy);
            let terrain = terrain_at_tile(ctx.noise, ctx.params, pos, ctx.terrain_ids);
            if !terrain.blocks_move(ctx.terrain_table) {
                land_samples += 1;
            }
            for slot in counts.iter_mut() {
                if ctx.resources.source_terrain(slot.kind) != Some(terrain) {
                    continue;
                }
                if node_roll(ctx, pos, slot.kind) {
                    slot.nodes += 1;
                }
            }
            dx += stride;
        }
        dy += stride;
    }

    counts.retain(|count| count.nodes > 0);
    ResourceSurvey {
        counts,
        land_samples,
        stride_area: (stride as u32).saturating_mul(stride as u32),
    }
}

/// 测试/demo 用的资源表夹具：本体四种资源，配 [`BaseTerrainIds`] 里
/// 已经注册好的地形索引。
///
/// 与 [`crate::terrain::base_terrain_fixture`]/
/// [`crate::weather::base_weather_fixture`] 同一条既有惯例：让不关心
/// 内容装载的测试不必为了拿一张表而跑一遍 mod 管线。
///
/// **取值与 `mods/lostland/resources.json5` 保持一致是刻意的**，但两处
/// 不是同一个真相源——生产路径读的恒是内容文件（资源表没有「本体注册
/// 入口」这一层，本体四种资源与任何 mod 的资源走完全相同的
/// `resources.json5` 通道），本夹具只服务测试与 demo。
pub fn base_resource_fixture(
    interner: &mut ll_core::ident::Interner,
    terrain_ids: &BaseTerrainIds,
) -> (Vec<ResourceKind>, ResourceTable) {
    let mut table = ResourceTable::new();
    let mut kinds = Vec::new();
    let declarations: [(&str, TerrainKind, u32, u32, u32, bool); 4] = [
        ("lostland:farmland", terrain_ids.grass, 120, 3, 1, false),
        ("lostland:timber", terrain_ids.forest, 200, 1, 2, false),
        ("lostland:iron_vein", terrain_ids.mountain, 60, 1, 5, true),
        (
            "lostland:fresh_water",
            terrain_ids.shallow_water,
            300,
            2,
            3,
            false,
        ),
    ];
    for (id, source_terrain, abundance, residents_supported, settlement_draw, exhaustible) in
        declarations
    {
        let index = interner.intern(NamespacedId::parse(id).expect("夹具用标识符恒合法"));
        let name_key = NamespacedId::parse(&format!("{id}_name")).expect("夹具用标识符恒合法");
        table
            .define(
                index,
                ResourceAttrs {
                    display_name_key: name_key,
                    source_terrain,
                    abundance,
                    residents_supported,
                    settlement_draw,
                    exhaustible,
                },
            )
            .expect("夹具声明内部自洽");
        kinds.push(ResourceKind::from_index(index));
    }
    (kinds, table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::base_terrain_fixture;
    use ll_core::ident::Interner;

    fn fixture() -> (
        Interner,
        BaseTerrainIds,
        TerrainTable,
        ResourceTable,
        Vec<ResourceKind>,
    ) {
        let (ids, terrain_table) = base_terrain_fixture();
        let mut interner = Interner::new();
        let (kinds, table) = base_resource_fixture(&mut interner, &ids);
        (interner, ids, terrain_table, table, kinds)
    }

    /// 把散着的几项拼成一个 [`ResourceContext`]——测试里的样板。
    fn ctx<'a>(
        ids: &'a BaseTerrainIds,
        terrain_table: &'a TerrainTable,
        table: &'a ResourceTable,
        noise: &'a TileableNoise,
        params: &'a GenParams,
        size: TorusSize,
    ) -> ResourceContext<'a> {
        ResourceContext {
            seed: params.seed,
            noise,
            params,
            terrain_ids: ids,
            terrain_table,
            resources: table,
            tile_size: size,
        }
    }

    fn world() -> (TileableNoise, GenParams, TorusSize) {
        let size = TorusSize::new(256, 256).expect("256x256 合法");
        let params = GenParams {
            seed: 0xBEEF_1234,
            ..GenParams::default()
        };
        let noise = TileableNoise::new(params.seed, 16, 16).expect("周期合法");
        (noise, params, size)
    }

    #[test]
    fn 同一格问两次得到同一个答案() {
        // Arrange
        let (_interner, ids, terrain, table, kinds) = fixture();
        let (noise, params, size) = world();
        let ctx = ctx(&ids, &terrain, &table, &noise, &params, size);
        let pos = size.wrap(37, 91);

        // Act & Assert
        for kind in &kinds {
            let first = resource_node_at(&ctx, pos, *kind);
            let second = resource_node_at(&ctx, pos, *kind);
            assert_eq!(first, second, "同一格同一种资源两次判定不一致");
        }
    }

    #[test]
    fn 资源只长在自己的源地形上() {
        // Arrange
        let (_interner, ids, terrain_table, table, kinds) = fixture();
        let (noise, params, size) = world();
        let ctx = ctx(&ids, &terrain_table, &table, &noise, &params, size);

        // Act & Assert：扫一片地，凡是判定为「有资源」的格，地形必须
        // 就是这种资源声明的源地形。
        let mut hits = 0u32;
        for y in 0..64 {
            for x in 0..64 {
                let pos = size.wrap(x, y);
                let terrain = terrain_at_tile(&noise, &params, pos, &ids);
                for kind in &kinds {
                    if resource_node_at(&ctx, pos, *kind) {
                        hits += 1;
                        assert_eq!(
                            table.source_terrain(*kind),
                            Some(terrain),
                            "资源出现在了非源地形上"
                        );
                    }
                }
            }
        }
        assert!(hits > 0, "四千格里一处资源都没有，判定可能整个没跑起来");
    }

    #[test]
    fn 丰度越高的资源在同一片源地形上出现得越多() {
        // Arrange：两种资源共用同一种源地形，只有丰度不同——把「丰度
        // 真的在起作用」与「这片地形本来就多」分开。
        let (ids, terrain_table) = base_terrain_fixture();
        let mut interner = Interner::new();
        let mut table = ResourceTable::new();
        let sparse = ResourceKind::from_index(
            interner.intern(NamespacedId::parse("test:sparse").expect("合法")),
        );
        let dense = ResourceKind::from_index(
            interner.intern(NamespacedId::parse("test:dense").expect("合法")),
        );
        let attrs = |abundance| ResourceAttrs {
            display_name_key: NamespacedId::parse("test:name").expect("合法"),
            source_terrain: ids.grass,
            abundance,
            residents_supported: 1,
            settlement_draw: 1,
            exhaustible: false,
        };
        table.define(sparse.index(), attrs(50)).expect("声明自洽");
        table.define(dense.index(), attrs(800)).expect("声明自洽");
        let (noise, params, size) = world();

        // Act
        let ctx = ctx(&ids, &terrain_table, &table, &noise, &params, size);
        let mut sparse_hits = 0u32;
        let mut dense_hits = 0u32;
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                let pos = size.wrap(x, y);
                if resource_node_at(&ctx, pos, sparse) {
                    sparse_hits += 1;
                }
                if resource_node_at(&ctx, pos, dense) {
                    dense_hits += 1;
                }
            }
        }

        // Assert
        assert!(
            dense_hits > sparse_hits * 4,
            "丰度 800‰ 只数到 {dense_hits} 处，50‰ 数到 {sparse_hits} 处，丰度没有真的在起作用"
        );
    }

    #[test]
    fn 勘察两次给出逐字段相同的结果() {
        // Arrange
        let (_interner, ids, terrain_table, table, _kinds) = fixture();
        let (noise, params, size) = world();
        let anchor = size.wrap(120, 96);
        let ctx = ctx(&ids, &terrain_table, &table, &noise, &params, size);
        let survey = || survey_resources(&ctx, anchor, 72, 8);

        // Act
        let first = survey();
        let second = survey();

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 勘察顺带数出的陆地面积不超过领地总面积() {
        // Arrange
        let (_interner, ids, terrain_table, table, _kinds) = fixture();
        let (noise, params, size) = world();
        let anchor = size.wrap(30, 30);
        let radius = 72;
        let stride = 8;

        // Act
        let ctx = ctx(&ids, &terrain_table, &table, &noise, &params, size);
        let survey = survey_resources(&ctx, anchor, radius, stride);

        // Assert：采样点数 × 每点代表的格数，上界是采样网格覆盖的面积。
        let samples_per_axis = (2 * radius / stride + 1) as u32;
        let covered = samples_per_axis * samples_per_axis * (stride as u32) * (stride as u32);
        assert!(survey.land_area() <= covered);
    }

    #[test]
    fn 丰度越界时注册期就被拒掉() {
        // Arrange
        let (ids, _terrain_table) = base_terrain_fixture();
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("test:bad").expect("合法"));
        let mut table = ResourceTable::new();

        // Act
        let result = table.define(
            index,
            ResourceAttrs {
                display_name_key: NamespacedId::parse("test:name").expect("合法"),
                source_terrain: ids.grass,
                abundance: 0,
                residents_supported: 1,
                settlement_draw: 1,
                exhaustible: false,
            },
        );

        // Assert
        assert_eq!(result, Err(ResourceError::AbundanceOutOfRange(0)));
    }

    #[test]
    fn 重复定义同一个资源索引返回错误() {
        // Arrange
        let (ids, _terrain_table) = base_terrain_fixture();
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("test:dup").expect("合法"));
        let mut table = ResourceTable::new();
        let attrs = ResourceAttrs {
            display_name_key: NamespacedId::parse("test:name").expect("合法"),
            source_terrain: ids.grass,
            abundance: 100,
            residents_supported: 1,
            settlement_draw: 1,
            exhaustible: false,
        };
        table
            .define(index, attrs.clone())
            .expect("首次定义应当成功");

        // Act
        let result = table.define(index, attrs);

        // Assert
        assert_eq!(result, Err(ResourceError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的资源索引查询全部走安全侧兜底() {
        // Arrange
        let mut interner = Interner::new();
        let never = ResourceKind::from_index(
            interner.intern(NamespacedId::parse("test:never").expect("合法")),
        );
        let table = ResourceTable::new();

        // Act & Assert
        assert!(!table.is_defined(never.index()));
        assert_eq!(table.source_terrain(never), None);
        assert_eq!(table.abundance(never), 0);
        assert_eq!(table.residents_supported(never), 0);
        assert_eq!(table.settlement_draw(never), 0);
        assert!(!table.exhaustible(never));
        assert_eq!(table.display_name_key(never), None);
    }

    #[test]
    fn 空资源表勘察出零条目但仍然数得出陆地() {
        // Arrange
        let (ids, terrain_table) = base_terrain_fixture();
        let table = ResourceTable::new();
        let (noise, params, size) = world();

        // Act
        let ctx = ctx(&ids, &terrain_table, &table, &noise, &params, size);
        let survey = survey_resources(&ctx, size.wrap(64, 64), 32, 8);

        // Assert
        assert!(survey.counts().is_empty());
        assert_eq!(
            survey.nodes_of(ResourceKind::from_index(ContentIndex::default())),
            0
        );
    }
}
