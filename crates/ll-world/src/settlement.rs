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
//!
//! # 据点可以横跨区块：全局推演 + 惰性铺设
//!
//! 项目所有者的裁决：「据点可能会横跨几个区块，因为后续的 NPC 也需要
//! 自己发展，自己制作自己的建筑，放置家具等。」这推翻了上一批次的一
//! 条硬约束（「建筑绝不跨区块」）。
//!
//! **落地形状是「历史算一次，地形按需铺」**：
//!
//! - [`crate::chronicle`] 的纪元推演照旧是**全局**的一次性计算，跨据点
//!   耦合（世界总人口、首邑、承载力）原样保留——它算的是「哪里有多大
//!   的据点」，不碰地形写入。
//! - [`stamp_settlement`] 变成**按区块惰性求值**：每个区块被物化时，
//!   问「有哪些据点覆盖到我」（[`crate::chronicle::WorldChronicle::sites_touching_zone`]），
//!   只把**落在自己这一格窗口内**的那部分建筑写下来，其余部分留给
//!   邻区块自己被物化时铺。
//!
//! 两者没有张力：全局的是推演，惰性的是铺设。
//!
//! # 它怎么和 `SurfaceStore::set_terrain` 的 panic 契约共存：根本不碰它
//!
//! 上一批次给出的「不跨区块」理由是：往未常驻的邻区块写入会撞上
//! `SurfaceStore::set_terrain` 的 panic 契约。**核实结论：那条契约在
//! 本模块这条路径上从来就不生效。** `stamp_settlement` 写的是调用方
//! 递进来的那一个 [`ChunkGrid`]（`SurfaceStore::generate_and_stamp`
//! 里刚生成、尚未插进 `resident` 的那一份），用的是
//! `ChunkGrid::set_terrain`，与 `SurfaceStore::set_terrain`（按世界瓦片
//! 坐标查区块、未常驻则 panic）是两个不同的函数。惰性铺设因此**一次
//! 都不会往别的区块写**：邻区块的那一半是在**它自己**被生成时、由
//! 它自己的窗口写下的。
//!
//! # 那真正的难点在哪：能不能盖房的判定必须与「谁在铺」无关
//!
//! [`plot_is_clear`] 决定一块 5×5 的地能不能盖房。上一批次读的是**当前
//! 窗口的地形**——一栋跨界的房子在 A 区块能读到自己的左半边、在 B 区块
//! 只能读到右半边，两边可能给出相反的答案，地上就会出现半栋房子。
//!
//! 解法是把这个判定改成读**基础地形**（噪声的纯函数，
//! [`crate::generate::terrain_at_tile`]），它对任意世界瓦片坐标都有
//! 定义、与「此刻哪个区块常驻」完全无关，因此 A、B 两边算出的答案
//! 必然一致。这不改变任何既有结果：上一批次读窗口读到的也正是这份
//! 基础地形（窗口就是它生成出来的），唯一的差别是**读得到跨界的
//! 那一半**。
//!
//! 「读到自己刚铺的墙就跳过」那条副作用随之消失，但它本来就是空转：
//! 建筑按 [`BUILDING_SPACING`]（6）的方格排布、外廓 [`BUILDING_SPAN`]
//! （5），两栋建筑在几何上不可能重叠；不同据点之间又隔着
//! [`crate::chronicle::ChronicleParams::min_settlement_spacing`]，远
//! 大于两倍的 [`MAX_FOOTPRINT_RADIUS`]。
//!
//! # 实测：跨界据点现在真的存在了
//!
//! 上一批次的如实标注是「机制在、测试守着，但**本体默认参数下实测跨
//! 区块据点数为 0**（三个种子、共 740 座据点，最多覆盖区块数恒为 1）」，
//! 根因写的是「据点还太小」。**那条诊断是对的，但它只对了一半。**
//!
//! 承载力确实按一个区块窗口（≤2304 格）的陆地面积算，那确实是旧的
//! 单区块约束留下的痕迹——但把它换成整片领地的口径之后，**平均建筑数
//! 一栋都没变，仍然是 3.3**。真正卡住据点规模的是人口模型本身：每纪元
//! 的人口变动此前**只有** `[-2, +2]` 那一项噪声，期望是 0，人口是一条
//! 零漂移的随机游走——它不朝任何地方长，承载力是 19 还是 190 都一样。
//! 完整论证与实测三档对照见
//! `crate::chronicle` 的 `GROWTH_RATE_DIVISOR` 常量文档。
//!
//! 两处一起改之后（领地口径的承载力 + 与承载力挂钩的比例增长），同样
//! 三个种子、共 788 座据点：
//!
//! | | 改动前 | 改动后 |
//! |---|---|---|
//! | 平均建筑数 | 3.3 | 20.2~21.6 |
//! | 人口中位数 | — | 31~33（最大 159~175） |
//! | **跨区块据点** | **0** | **21~29**（每个世界，最多覆盖 8 个区块） |
//!
//! 也就是说本模块的跨区块能力**不再只是能力**：约一成的据点真的铺过了
//! 自己那一格区块的边界，惰性铺设那条路径每局游戏都会走到。
//!
use ll_core::ident::WorldId;
use ll_core::rng::DetRng;
use ll_core::torus::{TorusPos, TorusSize};

use crate::chunk::ChunkGrid;
use crate::resource::ResourceKind;
use crate::space::ZoneCoord;
use crate::terrain::{BaseTerrainIds, TerrainKind, TerrainTable};
use crate::zone::ZoneLayout;

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
/// # 旧理由已经失效，新理由是别的
///
/// 上一批次取 24 的理由是「24 栋恰好装进一个 48×48 的区块窗口，不会
/// 溢出到邻区块」。**建筑现在可以跨区块**（见模块文档），那条几何
/// 约束不再是上界的来源。
///
/// 现在真正决定一座据点多大的是**人口**：
/// [`crate::chronicle`] 的 `final_sites` 取
/// `1 + 人口 / RESIDENTS_PER_BUILDING`。
///
/// **这个常量已经不再是一道够不着的护栏了。** 上一批次这里写着「实测
/// 平均建筑数只有 3.3 栋，这个常量当前根本咬不到」——人口模型换成与
/// 承载力挂钩的比例增长之后，实测平均 20 栋出头，而最大的那几座**恰好
/// 抵在 80 上**（见模块文档「实测」一节的对照表）。也就是说它现在是一
/// 条真的在起作用的上界：一座三百年不断增长的首邑会撞上它，而不是自己
/// 先停下来。
///
/// 这不改变取 80 的理由，只是让那个理由第一次变成实的：
///
/// 取 80 而不是继续留 24：80 恰好用满以锚点为中心第 4 圈方环
/// （`(2×4+1)² = 81` 个格位）的前 80 个，[`MAX_FOOTPRINT_RADIUS`] 因此
/// 是 26 格——仍然远小于据点最小间距的一半
/// （[`crate::chronicle::ChronicleParams::min_settlement_spacing`] 默认
/// 144），两座长满的据点不会互相压进对方的街区。
pub const MAX_BUILDINGS: u32 = 80;

/// 一座据点外廓的半径上界（格）：从锚点到最外那一圈建筑外墙的切比
/// 雪夫距离。
///
/// **由 [`MAX_BUILDINGS`]、[`BUILDING_SPACING`]、[`BUILDING_SPAN`] 三者
/// 推出，不是一个可以独立调的数值**——改上面任何一个，这个常量跟着
/// 变。消费者是 [`crate::chronicle::ChronicleParams::min_settlement_spacing`]
/// （据点最小间距必须大于它的两倍，否则两座长满的据点会互相压进对方
/// 的街区）。
pub const MAX_FOOTPRINT_RADIUS: i32 =
    max_ring(MAX_BUILDINGS) * BUILDING_SPACING + BUILDING_SPAN / 2;

/// 铺 `count` 栋建筑时，[`spiral_offset`] 用到的最外一圈方环半径
/// （单位是「第几圈」，不是格）。
///
/// 与 [`spiral_offset`] 内部那段环容量计算是同一条公式（半径 `r` 为止
/// 累计容纳 `(2r+1)^2` 栋），刻意写成 `const fn` 让
/// [`MAX_FOOTPRINT_RADIUS`] 在编译期就跟着 [`MAX_BUILDINGS`] 走，不需
/// 要有人记得手工同步一个魔数。
const fn max_ring(count: u32) -> i32 {
    if count <= 1 {
        return 0;
    }
    let last = count - 1;
    let mut ring = 0i32;
    while ((2 * ring + 1) * (2 * ring + 1)) as u32 <= last {
        ring += 1;
    }
    ring
}

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
    /// 这座据点**靠什么吃饭**：领地里最突出的两种资源，按
    /// `资源点数 × ResourceAttrs::settlement_draw` 降序排列，不足两种
    /// 时后面补 `None`（`[0]` 为 `None` 意味着领地里一种注册资源都
    /// 没数到）。
    ///
    /// # 为什么是「点数 × 吸引力」而不是纯点数
    ///
    /// 纯点数会被地形分布压成一个常数答案：本体四种资源里良田长在草地
    /// （最普遍的可住地形，`abundance` 120‰）、水源长在浅水（300‰），
    /// 而铁矿长在山地且只有 60‰——按纯点数排，几乎每座据点的第一名都是
    /// 良田或水源，「矿城」这种形态在名册里根本不会出现。乘上
    /// `settlement_draw`（铁矿 5、木材 2、良田/水源 1，见
    /// `mods/lostland/resources.json5`）之后，排序问的才是「这地方**因为
    /// 什么**才有人来」——那正是历史推演自己选址时用的同一把尺子
    /// （[`crate::chronicle`] 的 `resource_draw_bonus`）。
    ///
    /// # 为什么是定长数组，不是 `Vec`
    ///
    /// [`SettlementSite`] 是 `Copy` 的，被逐座复制进
    /// [`crate::chronicle::WorldChronicle::sites`] 又被逐座读出；换成
    /// `Vec` 会让这个类型失去 `Copy`，牵连全部既有调用点，换来的只是
    /// 「能记住第三、第四名」——而下游（NPC 名册的职业分布）问的是
    /// 「主业是什么、副业是什么」，第三名之后不改变任何决定。
    ///
    /// # 谁读它
    ///
    /// `ll_mod::roster` 的职业/种族分布：守着铁矿的据点要有铁匠，守着
    /// 良田的要有农夫（项目所有者裁定「据点的资源应当影响职业分布」的
    /// 落点）。
    pub resource_profile: [Option<ResourceKind>; SITE_RESOURCE_SLOTS],
}

/// [`SettlementSite::resource_profile`] 记几名——见该字段文档「为什么是
/// 定长数组」。
pub const SITE_RESOURCE_SLOTS: usize = 2;

/// 一栋建筑外廓的格数——[`house_tiles`]/[`ruin_tiles`] 产出的定长
/// 数组长度。
const BUILDING_TILES: usize = (BUILDING_SPAN * BUILDING_SPAN) as usize;

/// [`stamp_settlement`] 需要的、与「铺哪一个区块」无关的那一组输入。
///
/// 打包成一个结构体而不是继续往参数表上加：铺设入口现在还要一份
/// 「基础地形查询」（见 `base_terrain` 字段），散着传会撞上
/// `clippy::too_many_arguments`，更重要的是这四项**恒一起出现**，
/// 拆开传只会让调用点更容易漏配。
pub struct StampContext<'a> {
    /// 本体基础地形的内容索引。
    pub ids: &'a BaseTerrainIds,
    /// 地形属性表——`blocks_move` 判定用。
    pub table: &'a TerrainTable,
    /// 世界种子，喂给 [`SETTLEMENT_LAYOUT_STREAM_ID`] 的随机流。
    pub world_seed: u64,
    /// **基础地形**（据点还没铺上去之前，噪声算出来的那一层）在任意
    /// 世界瓦片坐标处的值。
    ///
    /// # 为什么不是「读当前窗口」
    ///
    /// 见模块文档「那真正的难点在哪」：一栋跨区块的建筑要在它覆盖到
    /// 的**每一个**区块里得出同一个「能不能盖」的答案，而当前窗口只
    /// 看得见自己那一格。生产实现是
    /// [`crate::generate::terrain_at_tile`]（噪声的纯函数）；测试可以
    /// 递一个常量闭包。
    pub base_terrain: &'a dyn Fn(TorusPos) -> TerrainKind,
}

/// 把一座据点落在 `zone` 这一格窗口内的那部分铺进 `grid`。
///
/// 据点可以横跨多个区块：本函数只写**属于 `zone` 的那些格**，跨出去
/// 的部分留给邻区块自己被物化时铺（见模块文档「全局推演 + 惰性铺设」）。
/// 因此它一次也不往别的区块写，`SurfaceStore::set_terrain` 的 panic
/// 契约在这条路径上不生效。
///
/// # 前置条件：`grid` 必须是刚生成、未经改写的窗口
///
/// 本函数**不是幂等的**，但不幂等的来源与上一批次不同了。它现在不再
/// 读 `grid`（「能不能盖房」改读 `ctx.base_terrain`），因此对同一个
/// 干净窗口连铺两次会得到同一个结果；真正的问题是对一个**玩家改过
/// 的**窗口重铺会抹掉那些改动。调用方
/// （[`crate::surface_store::SurfaceStore`]）的契约因此不变：
///
/// - `SurfaceStore::admit`：区块每次生成之后紧接着铺一次。
/// - `SurfaceStore::install_chronicle`：**先重新生成、再铺**，只允许
///   在新游戏构建期调用。
/// - `SurfaceStore::attach_chronicle`：读档路径，**只挂不铺**——存档里
///   的常驻区块可能已经被玩家拆过墙，重铺会把那些改动抹掉。
///
/// 这两个名字相近、语义相反的方法是这一带代码最容易用错的地方，改动
/// 前请先读它们各自的文档。
pub fn stamp_settlement(
    grid: &mut ChunkGrid,
    layout: &ZoneLayout,
    zone: ZoneCoord,
    site: &SettlementSite,
    ctx: &StampContext<'_>,
) {
    let tile_size = layout.tile_size();
    for building in 0..site.building_count.min(MAX_BUILDINGS) {
        let (left, top) = building_origin(site, building);
        // 先问「这栋房子跟本区块有没有关系」：绝大多数建筑与绝大多数
        // 区块无关，这一问是几次整数比较，挡在较贵的地形判定前面。
        if !footprint_touches_zone(layout, zone, left, top) {
            continue;
        }
        if !plot_is_clear(ctx, tile_size, left, top) {
            continue;
        }

        let mut rng = DetRng::for_entity(
            ctx.world_seed,
            SETTLEMENT_LAYOUT_STREAM_ID,
            u64::from(site.id.get()) * u64::from(MAX_BUILDINGS) + u64::from(building),
        );
        let tiles = match site.status {
            SettlementStatus::Inhabited => house_tiles(ctx.ids, &mut rng),
            SettlementStatus::Ruined => ruin_tiles(ctx.ids, &mut rng),
        };
        write_footprint(grid, layout, zone, (left, top), &tiles);
    }
}

/// 这座据点的建筑覆盖到哪些区块——按区块光栅序去重后返回。
///
/// [`crate::chronicle::WorldChronicle`] 在推演结束时用它建一份「区块 →
/// 覆盖到该区块的据点」索引，让 `SurfaceStore` 在每个区块首次物化时
/// 只需一次二分就问出「有哪些据点铺到我这里」（那是流式加载路径上的
/// 热点，见 [`crate::chronicle::WorldChronicle::sites_touching_zone`]）。
///
/// 结果**逐栋建筑精确算出**，不是「按半径圈一个方块」的保守估计：
/// 保守估计会让邻区块白跑一遍 `stamp_settlement`（虽然铺不出任何东西，
/// 但要把每栋建筑都判一遍），而精确算一次的代价是
/// `building_count × 25` 次整数换算，只在建档时发生一次。
///
/// 全程只用 `Vec` + 排序去重，不碰任何 `HashSet`（约束 C5）。
pub fn footprint_zones(site: &SettlementSite, layout: &ZoneLayout) -> Vec<ZoneCoord> {
    let tile_size = layout.tile_size();
    let mut zones = Vec::new();
    for building in 0..site.building_count.min(MAX_BUILDINGS) {
        let (left, top) = building_origin(site, building);
        for dy in 0..BUILDING_SPAN {
            for dx in 0..BUILDING_SPAN {
                let pos = tile_size.wrap(left + dx, top + dy);
                zones.push(layout.tile_to_zone(pos).0);
            }
        }
    }
    zones.sort_by_key(|zone| (zone.y(), zone.x()));
    zones.dedup();
    zones
}

/// 第 `building` 栋建筑外廓左上角的**世界瓦片**坐标（未环绕的原始
/// 整数，调用方按需 `wrap`）。锚点是第 0 栋的中心。
fn building_origin(site: &SettlementSite, building: u32) -> (i32, i32) {
    let (ox, oy) = spiral_offset(building);
    (
        site.anchor.x() + ox * BUILDING_SPACING - BUILDING_SPAN / 2,
        site.anchor.y() + oy * BUILDING_SPACING - BUILDING_SPAN / 2,
    )
}

/// 这栋建筑的 5×5 外廓有没有任何一格落在 `zone` 里。
///
/// 逐格问 [`ZoneLayout::tile_to_zone`]，不做「按坐标区间算区块号」的
/// 捷径：外廓只有 25 格，而捷径要自己处理环面接缝，两处各写一份换算
/// 正是本仓库反复付过代价的那类分歧。
fn footprint_touches_zone(layout: &ZoneLayout, zone: ZoneCoord, left: i32, top: i32) -> bool {
    let tile_size = layout.tile_size();
    for dy in 0..BUILDING_SPAN {
        for dx in 0..BUILDING_SPAN {
            let pos = tile_size.wrap(left + dx, top + dy);
            if layout.tile_to_zone(pos).0 == zone {
                return true;
            }
        }
    }
    false
}

/// 把一栋建筑的 25 格写进 `grid`，**跳过不属于 `zone` 的那些格**。
///
/// `tiles` 按 `dy * BUILDING_SPAN + dx` 的行主序排列，与
/// [`house_tiles`]/[`ruin_tiles`] 的产出顺序一致。
fn write_footprint(
    grid: &mut ChunkGrid,
    layout: &ZoneLayout,
    zone: ZoneCoord,
    origin: (i32, i32),
    tiles: &[TerrainKind; BUILDING_TILES],
) {
    let tile_size = layout.tile_size();
    let (left, top) = origin;
    for dy in 0..BUILDING_SPAN {
        for dx in 0..BUILDING_SPAN {
            let pos = tile_size.wrap(left + dx, top + dy);
            let (owner, local) = layout.tile_to_zone(pos);
            if owner != zone {
                continue;
            }
            grid.set_terrain(local, tiles[(dy * BUILDING_SPAN + dx) as usize]);
        }
    }
}

/// 这块 5×5 的地能不能盖房：25 格的**基础地形**全部可通行才算能。
///
/// 水面、山体因此被排除。判定只读基础地形，不读任何已经铺下去的东西
/// ——这正是它对「谁在铺、铺到第几个区块」完全无关的原因，见模块文档
/// 「那真正的难点在哪」。
///
/// 上一批次靠「读到自己刚铺的墙就跳过」来保证建筑之间不重叠，那条
/// 副作用现在没有了，**但它本来就是空转**：建筑按 [`BUILDING_SPACING`]
/// （6）的方格排布而外廓只有 [`BUILDING_SPAN`]（5）宽，两栋在几何上
/// 不可能重叠；不同据点之间又隔着据点最小间距，远大于两倍的
/// [`MAX_FOOTPRINT_RADIUS`]。本模块测试 `螺旋偏移前二十五个互不重复`
/// 与 `同一份输入铺两次逐格相同` 守着这条。
fn plot_is_clear(ctx: &StampContext<'_>, tile_size: TorusSize, left: i32, top: i32) -> bool {
    for dy in 0..BUILDING_SPAN {
        for dx in 0..BUILDING_SPAN {
            let pos = tile_size.wrap(left + dx, top + dy);
            if (ctx.base_terrain)(pos).blocks_move(ctx.table) {
                return false;
            }
        }
    }
    true
}

/// 一栋有人住的屋子的 25 格：一圈木墙 + 中间木地板 + 一扇门 + 一扇窗。
///
/// 产出一个定长数组而不是直接写进网格：跨区块的建筑要被**多个**区块
/// 各写一部分，而每一格是什么必须与「哪个区块在写」无关。先把整栋算
/// 出来、再由调用方挑自己那一部分写下去，是让这条性质显而易见的写法
/// ——也顺带保证了随机流的消耗次数与顺序不随裁剪而变。
fn house_tiles(ids: &BaseTerrainIds, rng: &mut DetRng) -> [TerrainKind; BUILDING_TILES] {
    let mut tiles = [ids.floor_wood; BUILDING_TILES];
    for dy in 0..BUILDING_SPAN {
        for dx in 0..BUILDING_SPAN {
            if on_edge(dx, dy) {
                tiles[(dy * BUILDING_SPAN + dx) as usize] = ids.wall_wood;
            }
        }
    }

    // 门开在四条边中点之一，窗开在另外三个中点里的一个——两者互不
    // 重叠，一栋屋子恒有一个出入口。
    let door_side = rng.gen_range(4) as usize;
    let window_side = (door_side + 1 + rng.gen_range(3) as usize) % 4;
    let (dx, dy) = edge_midpoint(door_side);
    tiles[(dy * BUILDING_SPAN + dx) as usize] = ids.door_closed;
    let (wx, wy) = edge_midpoint(window_side);
    tiles[(wy * BUILDING_SPAN + wx) as usize] = ids.window;
    tiles
}

/// 一处废墟的 25 格：石墙，没有门窗，且每堵墙都有塌掉的可能——塌掉的
/// 那格变回草地。
///
/// 塌掉的概率不随机到「整栋都没了」：只掷外圈那 16 格，中间的地板
/// 原样保留（石地板是废墟仍然认得出是建筑的那部分）。掷骰顺序恒为
/// `(dy, dx)` 行主序，与裁剪无关，见 [`house_tiles`] 文档。
fn ruin_tiles(ids: &BaseTerrainIds, rng: &mut DetRng) -> [TerrainKind; BUILDING_TILES] {
    let mut tiles = [ids.floor_stone; BUILDING_TILES];
    for dy in 0..BUILDING_SPAN {
        for dx in 0..BUILDING_SPAN {
            if !on_edge(dx, dy) {
                continue;
            }
            tiles[(dy * BUILDING_SPAN + dx) as usize] =
                if rng.chance(RUIN_COLLAPSE_NUMERATOR, RUIN_COLLAPSE_DENOMINATOR) {
                    ids.grass
                } else {
                    ids.wall_stone
                };
        }
    }
    tiles
}

/// 外廓局部坐标 `(dx, dy)` 落在 5×5 的那一圈墙上吗。
fn on_edge(dx: i32, dy: i32) -> bool {
    dx == 0 || dy == 0 || dx == BUILDING_SPAN - 1 || dy == BUILDING_SPAN - 1
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

    /// 测试用区块布局：3×3 个区块、边长 48（144×144 格）。**必须多于
    /// 一个区块**——本模块要验的正是「据点横跨区块」，一个区块的世界
    /// 里根本没有边界可跨。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(3, 3).expect("3x3 合法");
        ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
    }

    /// 锚点落在给定世界瓦片坐标的一座据点。
    fn site_at(status: SettlementStatus, building_count: u32, at: (i32, i32)) -> SettlementSite {
        let layout = test_layout();
        let mut counter = 0u32;
        let anchor = layout.tile_size().wrap(at.0, at.1);
        SettlementSite {
            id: WorldId::next(&mut counter),
            zone: layout.tile_to_zone(anchor).0,
            anchor,
            status,
            founded_epoch: 0,
            abandoned_epoch: None,
            population: 12,
            peak_population: 12,
            building_count,
            // 本模块只验「往网格里铺地形」，铺法不读资源画像；给一份
            // 空画像是最诚实的夹具（不假装这里数到了什么）。
            resource_profile: [None; SITE_RESOURCE_SLOTS],
        }
    }

    /// 一座锚点稳稳落在 (1,1) 号区块正中的据点——不跨界。
    fn site(status: SettlementStatus, building_count: u32) -> SettlementSite {
        site_at(status, building_count, (48 + 24, 48 + 24))
    }

    fn blank_window(ids: &BaseTerrainIds) -> ChunkGrid {
        let size = TorusSize::new(48, 48).expect("48x48 合法");
        ChunkGrid::new(size, ids.grass).expect("48 满足视口跨度")
    }

    /// 数一个窗口里某种地形有多少格。
    fn count_of(grid: &ChunkGrid, kind: TerrainKind) -> usize {
        let size = grid.world();
        let mut found = 0;
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                if grid.terrain_at(size.wrap(x, y)) == kind {
                    found += 1;
                }
            }
        }
        found
    }

    #[test]
    fn 有人住的据点铺出的每栋屋子都恰有一扇门() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let mut grid = blank_window(&ids);
        let site = site(SettlementStatus::Inhabited, 9);
        let grass = |_: TorusPos| ids.grass;
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 7,
            base_terrain: &grass,
        };

        // Act
        stamp_settlement(&mut grid, &layout, site.zone, &site, &ctx);

        // Assert
        assert_eq!(
            count_of(&grid, ids.door_closed),
            9,
            "九栋屋子应该恰好九扇门"
        );
    }

    #[test]
    fn 废墟不铺门也不铺窗() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let mut grid = blank_window(&ids);
        let site = site(SettlementStatus::Ruined, 6);
        let grass = |_: TorusPos| ids.grass;
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 7,
            base_terrain: &grass,
        };

        // Act
        stamp_settlement(&mut grid, &layout, site.zone, &site, &ctx);

        // Assert
        assert_eq!(count_of(&grid, ids.door_closed), 0);
        assert_eq!(count_of(&grid, ids.window), 0);
        assert!(
            count_of(&grid, ids.wall_stone) > 0,
            "废墟至少要留下几堵石墙"
        );
    }

    #[test]
    fn 同一份输入铺两次逐格相同() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let site = site(SettlementStatus::Inhabited, 12);
        let grass = |_: TorusPos| ids.grass;
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 99,
            base_terrain: &grass,
        };
        let mut first = blank_window(&ids);
        let mut second = blank_window(&ids);

        // Act
        stamp_settlement(&mut first, &layout, site.zone, &site, &ctx);
        stamp_settlement(&mut second, &layout, site.zone, &site, &ctx);

        // Assert
        let size = first.world();
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

    /// 铺设已经不读窗口了（地基判定改读基础地形），因此对**同一个干净
    /// 窗口**连铺两次也必须逐格相同——上一批次这里会因为读到自己刚铺
    /// 的墙而跳过全部建筑。这条守的是「不幂等的来源变了」这个事实本身，
    /// 见 [`stamp_settlement`] 文档「前置条件」。
    #[test]
    fn 对同一个窗口连铺两次与铺一次结果相同() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let site = site(SettlementStatus::Inhabited, 12);
        let grass = |_: TorusPos| ids.grass;
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 99,
            base_terrain: &grass,
        };
        let mut once = blank_window(&ids);
        let mut twice = blank_window(&ids);

        // Act
        stamp_settlement(&mut once, &layout, site.zone, &site, &ctx);
        stamp_settlement(&mut twice, &layout, site.zone, &site, &ctx);
        stamp_settlement(&mut twice, &layout, site.zone, &site, &ctx);

        // Assert
        let size = once.world();
        for y in 0..48 {
            for x in 0..48 {
                assert_eq!(
                    once.terrain_at(size.wrap(x, y)),
                    twice.terrain_at(size.wrap(x, y)),
                    "({x}, {y}) 连铺两次与铺一次不同"
                );
            }
        }
    }

    #[test]
    fn 建筑不会铺到水面上() {
        // Arrange：把世界坐标 x >= 72 的基础地形当成深水，它对应
        // (1,1) 号区块窗口的右半边。
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let mut grid = blank_window(&ids);
        let water_right = |pos: TorusPos| {
            if pos.x() >= 72 {
                ids.deep_water
            } else {
                ids.grass
            }
        };
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 3,
            base_terrain: &water_right,
        };
        let site = site(SettlementStatus::Inhabited, MAX_BUILDINGS);

        // Act
        stamp_settlement(&mut grid, &layout, site.zone, &site, &ctx);

        // Assert：窗口局部 x >= 24 对应世界 x >= 72，一格都不该被写。
        let size = grid.world();
        for y in 0..48 {
            for x in 24..48 {
                assert_eq!(
                    grid.terrain_at(size.wrap(x, y)),
                    ids.grass,
                    "({x}, {y}) 落在水上，本不该盖房"
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

    /// [`MAX_FOOTPRINT_RADIUS`] 是据点最小间距那条几何论证的前提，
    /// 它必须真的是上界：铺满 [`MAX_BUILDINGS`] 栋时没有任何一格伸出
    /// 这个半径。
    #[test]
    fn 外廓半径上界真的是上界() {
        // Arrange
        let site = site(SettlementStatus::Inhabited, MAX_BUILDINGS);
        let anchor_x = site.anchor.x();
        let anchor_y = site.anchor.y();

        // Act & Assert
        for building in 0..MAX_BUILDINGS {
            let (left, top) = building_origin(&site, building);
            let far_x = (left - anchor_x)
                .abs()
                .max((left + BUILDING_SPAN - 1 - anchor_x).abs());
            let far_y = (top - anchor_y)
                .abs()
                .max((top + BUILDING_SPAN - 1 - anchor_y).abs());
            assert!(
                far_x <= MAX_FOOTPRINT_RADIUS && far_y <= MAX_FOOTPRINT_RADIUS,
                "第 {building} 栋伸到了 ({far_x}, {far_y})，超过外廓半径上界 {MAX_FOOTPRINT_RADIUS}"
            );
        }
    }

    // ---- 跨区块 ----

    /// 锚点贴着区块边界时，据点确实横跨两个区块，且**两边的建筑加起来
    /// 与不跨界时一样多**——跨界不再让外圈那些房子凭空消失（上一批次
    /// 的 `fits_in_window` 会把它们整栋跳过）。
    #[test]
    fn 锚点贴着边界时据点真的横跨两个区块() {
        // Arrange：锚点放在 (0,0) 号区块的右边缘，据点必然伸进 (1,0)。
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let site = site_at(SettlementStatus::Inhabited, 9, (46, 24));
        let grass = |_: TorusPos| ids.grass;
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 11,
            base_terrain: &grass,
        };
        let zone_count = layout.zone_count();

        // Act
        let mut left_zone = blank_window(&ids);
        let mut right_zone = blank_window(&ids);
        stamp_settlement(&mut left_zone, &layout, zone_count.wrap(0, 0), &site, &ctx);
        stamp_settlement(&mut right_zone, &layout, zone_count.wrap(1, 0), &site, &ctx);

        // Assert：两个区块各铺到了东西，两边的门加起来恰好九扇。
        assert!(count_of(&left_zone, ids.wall_wood) > 0, "左区块什么都没铺");
        assert!(count_of(&right_zone, ids.wall_wood) > 0, "右区块什么都没铺");
        assert_eq!(
            count_of(&left_zone, ids.door_closed) + count_of(&right_zone, ids.door_closed),
            9,
            "跨界之后门的总数变了，说明有房子被整栋丢掉或重复铺了"
        );
    }

    /// 跨界的**那一栋**房子在两个区块里拼起来正好是完整的 5×5——
    /// 一边铺左半、另一边铺右半，没有缺口也没有重叠。
    #[test]
    fn 跨界的那一栋房子在两个区块里拼起来是完整的() {
        // Arrange：只铺一栋，锚点让它正好骑在 x = 48 这条界上。
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let site = site_at(SettlementStatus::Inhabited, 1, (48, 24));
        let grass = |_: TorusPos| ids.grass;
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 5,
            base_terrain: &grass,
        };
        let zone_count = layout.zone_count();

        // Act
        let mut left_zone = blank_window(&ids);
        let mut right_zone = blank_window(&ids);
        stamp_settlement(&mut left_zone, &layout, zone_count.wrap(0, 0), &site, &ctx);
        stamp_settlement(&mut right_zone, &layout, zone_count.wrap(1, 0), &site, &ctx);

        // Assert：外廓左上角在世界 (46, 22)，两个区块各承担 x = 46..47
        // 与 x = 48..50 这两段，25 格没有一格该留成草地。
        let size = left_zone.world();
        let mut written = 0;
        for dy in 0..BUILDING_SPAN {
            for dx in 0..BUILDING_SPAN {
                let world_x = 46 + dx;
                let world_y = 22 + dy;
                let (grid, local_x) = if world_x < 48 {
                    (&left_zone, world_x)
                } else {
                    (&right_zone, world_x - 48)
                };
                if grid.terrain_at(size.wrap(local_x, world_y)) != ids.grass {
                    written += 1;
                }
            }
        }
        assert_eq!(written, 25, "跨界的房子有 {} 格没被铺", 25 - written);
    }

    /// 「这块地能不能盖房」的判定必须与「谁在铺」无关：把跨界那栋房子
    /// 右半边的基础地形改成水，**两个区块都不该铺它**——若判定还在读
    /// 各自的窗口，左区块看不见右边的水，就会铺出半栋房子。
    #[test]
    fn 跨界房子的地基判定在两个区块里给出同一个答案() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let site = site_at(SettlementStatus::Inhabited, 1, (48, 24));
        let water_right = |pos: TorusPos| {
            if pos.x() >= 48 {
                ids.deep_water
            } else {
                ids.grass
            }
        };
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 5,
            base_terrain: &water_right,
        };
        let zone_count = layout.zone_count();

        // Act
        let mut left_zone = blank_window(&ids);
        let mut right_zone = blank_window(&ids);
        stamp_settlement(&mut left_zone, &layout, zone_count.wrap(0, 0), &site, &ctx);
        stamp_settlement(&mut right_zone, &layout, zone_count.wrap(1, 0), &site, &ctx);

        // Assert
        assert_eq!(
            count_of(&left_zone, ids.wall_wood),
            0,
            "左区块铺出了半栋房子"
        );
        assert_eq!(count_of(&right_zone, ids.wall_wood), 0);
    }

    /// 据点横跨环面接缝（世界坐标 0 那条线）时同样成立。
    #[test]
    fn 据点跨越环面接缝时两侧仍然拼得完整() {
        // Arrange：锚点放在世界左边缘，据点向左伸出去会绕到最右边。
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let site = site_at(SettlementStatus::Inhabited, 9, (1, 24));
        let grass = |_: TorusPos| ids.grass;
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 13,
            base_terrain: &grass,
        };
        let zone_count = layout.zone_count();

        // Act：世界是 3×3 个区块，左邻居是 x = 2 那一列。
        let mut first_zone = blank_window(&ids);
        let mut wrapped_zone = blank_window(&ids);
        stamp_settlement(&mut first_zone, &layout, zone_count.wrap(0, 0), &site, &ctx);
        stamp_settlement(
            &mut wrapped_zone,
            &layout,
            zone_count.wrap(2, 0),
            &site,
            &ctx,
        );

        // Assert
        assert!(
            count_of(&wrapped_zone, ids.wall_wood) > 0,
            "接缝另一侧什么都没铺"
        );
        assert_eq!(
            count_of(&first_zone, ids.door_closed) + count_of(&wrapped_zone, ids.door_closed),
            9,
            "跨接缝之后门的总数变了"
        );
    }

    #[test]
    fn 覆盖区块清单与实际铺到东西的区块一致() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let layout = test_layout();
        let site = site_at(SettlementStatus::Inhabited, 9, (46, 46));
        let grass = |_: TorusPos| ids.grass;
        let ctx = StampContext {
            ids: &ids,
            table: &table,
            world_seed: 17,
            base_terrain: &grass,
        };

        // Act
        let zones = footprint_zones(&site, &layout);

        // Assert：清单之外的区块一格都铺不出来。
        let zone_count = layout.zone_count();
        for zone_y in 0..3 {
            for zone_x in 0..3 {
                let zone = zone_count.wrap(zone_x, zone_y);
                let mut grid = blank_window(&ids);
                stamp_settlement(&mut grid, &layout, zone, &site, &ctx);
                let touched = count_of(&grid, ids.grass) < 48 * 48;
                assert_eq!(
                    zones.contains(&zone),
                    touched,
                    "区块 {zone:?} 的覆盖清单与实际铺设结果不符"
                );
            }
        }
        assert!(zones.len() > 1, "这座据点本该横跨多个区块");
    }

    #[test]
    fn 覆盖区块清单按光栅序排列且不重复() {
        // Arrange
        let layout = test_layout();
        let site = site_at(SettlementStatus::Inhabited, MAX_BUILDINGS, (46, 46));

        // Act
        let zones = footprint_zones(&site, &layout);

        // Assert
        let mut previous: Option<(i32, i32)> = None;
        for zone in &zones {
            let key = (zone.y(), zone.x());
            if let Some(prev) = previous {
                assert!(prev < key, "覆盖清单没有按光栅序排列或出现重复");
            }
            previous = Some(key);
        }
    }
}
