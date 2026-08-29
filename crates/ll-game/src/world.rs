//! 新游戏的世界搭建：区块布局、噪声、出生点、玩家实体。
//!
//! 与 `ll-sim` 的 `p5_coordinate_acceptance::world` 同一套思路（先建
//! 布局与噪声、再强制铺一小片确定可站立的出生地，理由见其模块文档
//! 「验收 demo 需要确定性场景」），区别是这里的地形/种族索引来自
//! [`crate::content::LoadedContent`] 的真实装载结果，不是测试用的
//! `*_fixture` 便捷函数——本体二进制走的是与 mod 完全相同的注册通道。

use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_sim::timeline::Timeline;
use ll_world::chronicle::{ChronicleInput, ChronicleParams, WorldChronicle};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::{
    GenParams, build_zone_noise, generate_zone_window, zone_representative_terrain,
};
use ll_world::land::largest_walkable_component;
use ll_world::noise::TileableNoise;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::zone::ZoneLayout;
use ll_world::{
    WorldError,
    terrain::{BaseTerrainIds, TerrainTable},
};

use ll_content::world_identity::WorldIdentity;
use ll_mod::mod_set::GenerationModSet;

use crate::content::LoadedContent;

/// 新游戏的起始世界时刻——早上八点。
///
/// 选白天而不是 `Tick(0)`（午夜）的理由见 [`build_new_world`] 里设置它的那段
/// 注释：午夜的环境光会让开局画面几乎全黑。取整点只是为了让「第 0 天
/// 早八点」这个起点读起来自然，没有玩法含义。
pub const NEW_GAME_START_TICK: Tick = Tick(8 * ll_core::time::TICKS_PER_HOUR);

/// 区块边长（格）：固定 48，与 `ll_content::world_identity` 推荐预设表
/// 一致（该模块文档：区块边长固定 48）。
const ZONE_SPAN: u32 = 48;
/// 世界区块数：取推荐预设表「小陆地」一档——本体二进制目前没有开局
/// 选择尺寸的界面（P7 才有，见规格 §15），固定用最小的一档,保持新游戏
/// 启动与流式加载都足够快。
const ZONE_COUNT: (u32, u32) = (64, 48);

/// 出生点周围强制铺成草地的半径（格）——[`find_spawn_site`] 找不到
/// 合格陆地、退回 [`carve_spawn_clearing`] 兜底时用的补丁大小,不再是
/// 常规路径,见 [`carve_spawn_clearing`] 文档。半径 3 只够玩家开局立刻
/// 看到、走到的一小片范围,不是整张地图。
const SPAWN_CLEARING_RADIUS: i32 = 3;

/// [`find_spawn_site`] 判定「够大」的最小连通可行走区域格数。
///
/// # 取值理由
///
/// 旧版补丁（[`SPAWN_CLEARING_RADIUS`] = 3 的 7×7 强铺草地）只有 49
/// 格——正是项目所有者实测报告里那座「小得可怜的岛」。这里取
/// `500`：约是旧补丁面积的十倍，换算成正方形约 22×22，明显大到「能
/// 走开」，同时仍只占单个区块窗口（`ZONE_SPAN × ZONE_SPAN = 2304`
/// 格,见 [`find_spawn_site`] 「扫描粒度」一节）约 22%——普通草原/
/// 平原区块的连通陆地远超这个比例，阈值不会把搜索卡在几乎找不到解的
/// 境地，只会真正挡住「出生点只挨着一小片孤立陆地」这类情形。
const MIN_SPAWN_LAND_AREA: usize = 500;

/// [`find_spawn_site`] 最多完整检查（生成整个区块窗口 + 连通域分析）
/// 的区块数——超出后放弃搜索,不再无限找下去,见该函数文档「有界」
/// 一节。
///
/// 取值 128：本体默认区块布局（[`ZONE_COUNT`]）是 64×48，128 恰好是
/// 其中两整行区块——多数种子在几个区块内就能找到合格陆地，给两整行
/// 的余量足以吸收局部地形恰好破碎的偶然情况；又远小于区块总数
/// （`64 × 48 = 3072`），不会在几乎全是水的病态种子上退化成等价于
/// 生成整张地图的开销。
const MAX_SPAWN_SEARCH_ZONES: usize = 128;

/// 流式邻域维护半径（区块数）——与 `p5_coordinate_acceptance` 同一
/// 取值，见其 `layout::STREAM_RADIUS_ZONES` 文档。
pub const STREAM_RADIUS_ZONES: i32 = 2;

/// 画面缩放允许的最小倍率——比 [`ll_render::camera::Zoom::MIN`]（通用
/// 下限，与任何具体世界的常驻区块策略无关）更窄，是本体二进制专属的
/// 「安全」下限：拉得再远也不能让 `Camera::visible_tiles_zoomed`
/// 枚举出超出常驻区块集合覆盖范围的坐标，否则
/// `ll_world::surface_store::SurfaceWindow::terrain_at` 会因坐标所在
/// 区块尚未常驻而 panic（见其文档「前置条件」一节）。
///
/// # 推导：常驻区块集合保证的最小边距
///
/// `STREAM_RADIUS_ZONES`（本文件，取 2）与 `ZONE_SPAN`（本文件私有
/// 常量，取 48，不做成文档内链——rustdoc 默认不为私有项生成页面，
/// 链接会解析失败，同一取舍见 `build_new_world` 文档提到的
/// `warm_spawn_neighborhood`）共同决定 `SurfaceStore::stream_neighborhood`
/// 每帧维护的常驻区块
/// 集合——以玩家所在**区块**为中心、`STREAM_RADIUS_ZONES` 圈内的全部
/// 区块常驻，构成一块 `(2×2+1)×(2×2+1) = 5×5` 区块、
/// `5×48 = 240` 格见方的常驻区域，玩家所在区块位于这块区域正中。
///
/// 玩家在自己所在区块内的具体位置（`0..ZONE_SPAN` 的局部坐标）决定了
/// 玩家到常驻区域边缘的实际距离：局部坐标为 `0` 时，到那一侧边缘的
/// 距离恰好是 `STREAM_RADIUS_ZONES × ZONE_SPAN = 96` 格（两整个区块的
/// 宽度，玩家自己所在区块里一格都不占）；局部坐标为
/// `ZONE_SPAN - 1 = 47` 时，到**另一侧**边缘的距离同样是 `96` 格。
/// 无论玩家站在区块内哪个位置，到最近一侧常驻边缘的距离恒
/// **不小于** `96` 格——这是常驻集合能保证的、与玩家具体站位无关的
/// 最小边距，`Camera::visible_tiles_zoomed` 枚举出的范围必须始终
/// 落在这个边距之内。
///
/// # 为什么留出安全余量，不用满 96
///
/// 逻辑分辨率 640×360 与瓦片边长 16 未必能让缩放后的有效瓦片边长
/// 整除，浮点舍入、以及未来若调整 `ZONE_SPAN`/逻辑分辨率而忘记同步
/// 这里的常量，都值得留一点缓冲而不是卡着理论上限走。取
/// `MIN_SAFE_ZOOM = 0.3` 时 `visible_half_extent(640, 0.3) = 67`
/// （见下方测试），距 96 格的硬边界还有 29 格缓冲；`MAX_SAFE_ZOOM`
/// 则完全不受这条约束
/// （拉近只会让可见范围变小，永远在常驻区域之内），直接取
/// [`ll_render::camera::Zoom::MAX`]。
pub const MIN_SAFE_ZOOM: f32 = 0.3;

/// 画面缩放允许的最大倍率——拉近不受常驻区块集合约束，直接复用
/// [`ll_render::camera::Zoom`] 的通用上限，见 [`MIN_SAFE_ZOOM`] 文档
/// 「为什么留出安全余量」一节末句。
pub const MAX_SAFE_ZOOM: f32 = ll_render::camera::Zoom::MAX;

/// 建立本体默认使用的区块布局。
pub fn build_zone_layout() -> Result<ZoneLayout, WorldError> {
    let zone_count = ll_core::torus::TorusSize::new(ZONE_COUNT.0, ZONE_COUNT.1).ok_or(
        WorldError::WorldTooSmall {
            width: ZONE_COUNT.0,
            height: ZONE_COUNT.1,
        },
    )?;
    ZoneLayout::new(ZONE_SPAN, zone_count)
}

/// 新游戏的完整世界：世界状态、噪声源与生成参数（流式加载持续需要）、
/// 玩家实体 id、回合时间轴。
pub struct GameWorld {
    /// 世界状态本身——存档写出/读入的对象。
    pub world: WorldState,
    /// 地形噪声源，流式加载持续需要。
    pub noise: TileableNoise,
    /// 地形生成参数，流式加载持续需要。
    pub params: GenParams,
    /// 玩家实体 id。
    pub player: EntityId,
    /// 回合时间轴——喂给 [`ll_sim::turn::TurnEngine::new`] 驱动世界
    /// 时钟推进,见 [`rebuild_timeline`] 文档「为什么时间轴不进存档」
    /// 一节：与 `noise`/`params` 同一类「运行期派生数据」,不是
    /// `WorldState` 的一部分,不参与序列化。
    pub timeline: Timeline,
    /// 这个世界的身份：种子 + 尺寸 + 地形形态 + **生成期 mod 集合**。
    ///
    /// # 为什么它必须住在这里，而不是每次存档现算
    ///
    /// 生成期 mod 集合是「这个世界当初是用哪一批 mod 生成的」这条事实
    /// 的**唯一记录**，只存在于存档头里。存档时若从当前会话已装载的
    /// 内容重新算一遍（修复前 `crate::save::save_game` 正是这么做的），
    /// 玩家中途新装的任何一个 mod 都会永久混进这个世界的生成期名单，
    /// 而原始记录被覆盖后追不回来——种子分享、缺陷复现、回归测试全部
    /// 失效（`knowledge/handoff/p4-to-p5.md` 二节原话）。
    ///
    /// 因此它在**建新世界那一刻**由 [`build_new_world`] 绑定一次
    /// （[`ll_content::world_identity::WorldIdentity::bind`]），读档时
    /// 由 `crate::save::load_game` 从存档头**原样接回来**
    /// （`WorldIdentity::restore_from_header`），存档时原样写回。
    /// `GameWorld` 是这份身份在运行期的落脚点：存档函数只拿得到
    /// `&GameWorld`，也就只拿得到这一份身份，没有第二个来源。
    pub identity: WorldIdentity,
}

/// 从当前世界状态重建时间轴：按 [`WorldState::actors`] 里每个存活
/// 实体各自持久化的 [`Agent::next_action_at`]，把它排回一条全新的
/// [`Timeline`]。
///
/// # 为什么时间轴不进存档，也不需要进
///
/// `Timeline` 本身完全可以派生自 `WorldState` 已经持久化的数据——每个
/// `Agent` 都随身带着自己的 `next_action_at`（`ll_sim::apply` 的
/// `Effect::ScheduleNext` 分支唯一的写入点），这正是「谁下次什么时候
/// 行动」这条信息的权威来源。存档只需要照常序列化 `WorldState`（见
/// `crate::save::save_game`），读档后用本函数重建出的时间轴与存档前
/// 那一条在弹出顺序上完全等价——不需要 `Timeline` 自己再序列化一份
/// 冗余状态,也就不需要为它另开一个 `#[serde(skip)]` 字段。这与
/// `crate::lib::rebuild_noise`（噪声同样是「按已持久化的种子随时能
/// 重新派生」的运行期数据）是同一套取舍。
///
/// # 弹出顺序与迭代顺序无关，不违反约束 C5
///
/// [`ll_world::entity::Arena::iter_with_id`] 按槽位下标顺序
/// 迭代，不依赖任何哈希容器——但即使换一种迭代顺序调用
/// [`Timeline::schedule`]，弹出顺序也不会变：`Timeline` 内部是按
/// `(Tick, EntityId)` 排序的堆（见其模块文档「同刻打破平局」一节），
/// 弹出顺序只由条目的值决定,与入堆顺序无关。这里选 `iter_with_id`
/// 只是因为它恰好是最省事的「遍历全部存活实体」的写法,不是为了保证
/// 顺序。
pub fn rebuild_timeline(world: &WorldState) -> Timeline {
    let mut timeline = Timeline::new();
    for (id, agent) in world.actors.iter_with_id() {
        timeline.schedule(id, agent.next_action_at);
    }
    timeline
}

/// 建立一局新游戏：区块布局 → 噪声 → 世界状态 → 出生点铺地 → 玩家
/// 实体。`params.seed` 决定地形具体分布——不同种子产出不同世界,同一
/// 种子加同一批内容必然复现同一个世界（世界身份三要素之一,见
/// `ll_content::world_identity` 模块文档）。
///
/// # 为什么参数是整个 [`GenParams`]，不再只是一个 `seed`
///
/// `params.shape`（[`ll_world::generate::TerrainShape`]）决定这个世界
/// 是大陆、群岛还是山地——它与种子一样是玩家在建档那一刻做出的、事后
/// 无法从任何其它数据反推的选择。此前本函数只接收 `seed`、形态部分
/// 硬取 `GenParams::default()`，于是那三个形态字段虽然在类型上可调，
/// 却没有任何生产路径能真正调到它们。把整个 `GenParams` 收进签名，
/// 「玩家选了什么」与「世界怎么生成」之间就只剩这一条通路，
/// [`WorldState::terrain_shape`] 再把它一路带进存档。
pub fn build_new_world(
    content: &LoadedContent,
    params: GenParams,
) -> Result<GameWorld, WorldError> {
    let layout = build_zone_layout()?;
    let noise = build_zone_noise(&layout, &params)?;

    // 出生点必须落在真实、连得开的陆地上——不能再像旧版那样把它硬
    // 钉在 `(0, 0)`，赌噪声场恰好在那一点给出可站立地形（项目所有者
    // 实测报告：那一赌就是玩家看到的那座「小得可怜的岛」，见
    // `find_spawn_site` 文档）。搜索失败（`None`，见该函数文档「有界」
    // 一节）时退回旧版硬编码坐标 + [`carve_spawn_clearing`] 强铺兜底，
    // 并记一条警告日志，让这种情形可见而不是静默发生。
    let located_spawn = find_spawn_site(
        &layout,
        &noise,
        &params,
        &content.terrain_ids,
        &content.terrain_table,
    );
    let (spawn, spawn_land_area) = match located_spawn {
        Some((pos, area)) => (pos, area),
        None => (layout.tile_size().wrap(0, 0), 0),
    };

    // TerrainTable 不是 Copy——WorldState::new 需要拿走它的所有权,
    // 出生点铺地之后本函数还要用 &content.terrain_ids（不需要表本身），
    // 故这里克隆一份而不是尝试在 new 之后再找回同一张表。地形属性表
    // 本身很小（每种地形一条定长记录），克隆开销可忽略。
    let mut world = WorldState::new(
        layout,
        &params,
        &content.terrain_ids,
        content.terrain_table.clone(),
        spawn,
    )?;
    world.surface_profile = content.space_ids.surface;

    // 世界历史：玩家进入之前，这块大陆已经跑过三百年兴衰。据点、废墟
    // 就是那段历史留在地形上的痕迹，见 `ll_world::chronicle` 模块文档。
    //
    // 顺序上必须排在 `WorldState::new` 之后（它才是噪声与地形表真正
    // 就位的地方），因此出生邻域的那几个区块是先生成、后补铺的——
    // `install_chronicle` 会把它们重新生成一遍再铺，与后续流式加载进来
    // 的区块走完全同一条路径，见该方法文档。
    let chronicle = std::sync::Arc::new(WorldChronicle::generate(
        &ChronicleInput {
            layout: &layout,
            noise: &noise,
            params: &params,
            terrain_ids: &content.terrain_ids,
            terrain_table: &content.terrain_table,
            resources: &content.resource_table,
            cultures: &content.culture_table,
        },
        ChronicleParams::default(),
    ));
    tracing::info!(
        seed = params.seed,
        epochs = chronicle.epochs(),
        events = chronicle.events().len(),
        settlements = chronicle.sites().len(),
        "世界历史生成完成"
    );
    // 编年史分配掉的 WorldId 不能被游戏内的击杀记录再发一次——历史
    // 事件与据点用的是同一个号码空间（`identity-and-ids.md`）。
    world.next_world_id = world.next_world_id.max(chronicle.next_world_id());
    world
        .terrain
        .install_chronicle(chronicle, &noise, &params, &content.terrain_ids);
    // 新游戏从早晨开始，而不是 `Tick(0)`（午夜）。
    //
    // 三条各自正确的规则叠在一起会让午夜开局变成纯黑屏：午夜环境光是
    // 千分之一百（`ll_world::light` 刻意不取零，有测试守着），
    // `sight_radius_at` 把 `BASE_SIGHT_RADIUS`(12) 缩成 1，
    // `effective_tint` 又把整幅画面乘上 0.1，而三层可见性里「从未探索」
    // 是完全不画、露出黑色清屏底。结果就是一片黑加正中央五个格子、
    // 亮度一成——项目所有者实测报告「运行以后出来的是黑屏」。
    //
    // 每一块单独看都有测试、都按设计工作，缺的是「开局那一刻玩家看到
    // 什么」这条组合断言，见本模块测试 `新游戏起始时刻的视野半径不至于
    // 只剩最小值`。
    world.clock = NEW_GAME_START_TICK;

    if located_spawn.is_some() {
        tracing::info!(
            seed = params.seed,
            spawn_x = spawn.x(),
            spawn_y = spawn.y(),
            connected_land_area = spawn_land_area,
            min_required_area = MIN_SPAWN_LAND_AREA,
            "出生点选址完成：落在一片连通陆地上"
        );
    } else {
        tracing::warn!(
            seed = params.seed,
            spawn_x = spawn.x(),
            spawn_y = spawn.y(),
            max_zones_inspected = MAX_SPAWN_SEARCH_ZONES,
            min_required_area = MIN_SPAWN_LAND_AREA,
            "出生点搜索在有界步数内未找到满足最小连通陆地面积的候选,退回强制铺地兜底"
        );
        carve_spawn_clearing(&mut world, &noise, &params, content, spawn);
    }

    let (zone, _) = layout.tile_to_zone(spawn);
    let player = spawn_player(&mut world, spawn, zone, content);
    // 必须显式赋值——见 `ll_world::state::WorldState::player_entity`
    // 字段文档「调用方应在 spawn 产出玩家的 EntityId 之后显式赋值」
    // 一节：漏掉这一步，探索记忆的写入路径会永远收不到任何写入（同一
    // 缺陷 P5-A 阶段在 `p5_coordinate_acceptance` 里出现过一次，本体
    // 二进制不重演）。
    world.player_entity = Some(player);
    // 玩家的初次行动排进时间轴——见 `rebuild_timeline` 文档「为什么时间轴
    // 不进存档」一节：时间轴完全从 `Agent::next_action_at` 派生,
    // `spawn_player` 已经把玩家的这个字段设成 `world.clock`（新游戏起始
    // 时刻，而不是字面量 `Tick(0)`,否则玩家第一次行动时 `TurnEngine`
    // 会把世界时钟倒拨回午夜,见该字段赋值处的注释），此处重建即可拿到
    // 一条与「玩家现在就能行动」一致的时间轴。
    let timeline = rebuild_timeline(&world);

    // 世界身份在这里、也只在这里绑定一次——`GenerationModSet::capture`
    // 的**唯一生产调用点**就是这一行（见 `ll_mod::mod_set` 模块文档
    // 「绑定时机」一节：世界创建那一刻一次性调用，写入存档头后永久
    // 不变）。存档路径上不再有第二次 capture，见 `GameWorld::identity`
    // 字段文档。
    let identity = WorldIdentity::bind(
        params.seed,
        layout,
        params.shape,
        GenerationModSet::capture(&content.registry, &content.manifests),
    );

    Ok(GameWorld {
        world,
        noise,
        params,
        player,
        timeline,
        identity,
    })
}

/// 出生点周围 [`SPAWN_CLEARING_RADIUS`] 格内强制改写成草地：先用
/// `terrain_at` 触发按需生成（半径外沿可能落在出生邻域预热半径覆盖
/// 不到的区块），再 `set_terrain` 覆写——这正是流式加载的正常使用
/// 方式,不是绕过它,见 `p5_coordinate_acceptance::world` 模块文档同一
/// 段说明。
///
/// # 现在只是兜底,不是常规路径（处置结论）
///
/// 这个函数曾经是出生点选址的全部逻辑——不管噪声场在 `(0, 0)` 生成的
/// 是什么，都在原地强行铺出一块 7×7 的草地。项目所有者实测报告里那座
/// 「小得可怜的岛」正是这块补丁：出生点落在水里，`carve_spawn_clearing`
/// 没有换个地方出生，而是在海上打了个补丁。
///
/// **没有删掉它**——[`find_spawn_site`] 本身是有界搜索（见其文档「有
/// 界」一节），一定存在找不到合格候选、必须放弃的情形（例如某个种子
/// 的世界几乎全是水）。这种情形下仍然需要一个「保证出生点这一格能站
/// 人」的最后手段，否则退回的硬编码坐标 `(0, 0)` 可能连玩家自己站的
/// 那一格都是水——这就是本函数继续存在的唯一职责：**只在
/// [`find_spawn_site`] 放弃之后调用**，不再是出生点选址的默认路径,
/// 调用点见 [`build_new_world`]。
///
/// 也没有缩小职责（例如只清出生点正下方一格）：搜索已经放弃、真实
/// 地形大概率不可靠的情况下，7×7 这块小小的确定性安全区仍然比单独
/// 一格更经受得住「玩家出生时紧挨着的那格恰好是墙/树」这类边缘情形，
/// 维持原有半径不额外增加风险，只是收窄了触发它的条件。
fn carve_spawn_clearing(
    world: &mut WorldState,
    noise: &TileableNoise,
    params: &GenParams,
    content: &LoadedContent,
    spawn: TorusPos,
) {
    let size = world.terrain.layout().tile_size();
    for dy in -SPAWN_CLEARING_RADIUS..=SPAWN_CLEARING_RADIUS {
        for dx in -SPAWN_CLEARING_RADIUS..=SPAWN_CLEARING_RADIUS {
            let pos = size.wrap(spawn.x() + dx, spawn.y() + dy);
            world
                .terrain
                .terrain_at(noise, params, &content.terrain_ids, pos, Tick(0));
            world.terrain.set_terrain(pos, content.terrain_ids.grass);
        }
    }
}

/// 在整张世界地形里按固定顺序搜索一块「足够大」的连通可行走陆地，
/// 作为新游戏出生点。成功时返回 `(出生点, 该点所在连通分量的格数)`。
///
/// # 为什么不能只检查候选格本身能不能走
///
/// 旧版直接把出生点定在 `(0, 0)`，靠 [`carve_spawn_clearing`] 在原地
/// 强行铺出一块 7×7 的草地——`(0, 0)` 落在海里时,玩家看到的就是被
/// 人工补丁包住、周围全是水的一座「小岛」（项目所有者实测报告）。只
/// 检查候选格本身能不能走同样会踩坑：一格可通行的礁石被大片深水包围
/// 时依旧能通过这条检查,玩家还是走不远。必须验证出生点所在的连通可
/// 行走区域本身够大（阈值见 [`MIN_SPAWN_LAND_AREA`]）。
///
/// # 扫描顺序与扫描粒度
///
/// 1. 按区块坐标光栅序（`zone_y` 从 0 递增，每行内 `zone_x` 从 0 递
///    增，从 `(0, 0)` 起步）遍历全部区块——不依赖任何
///    `HashMap`/`HashSet` 迭代顺序（约束 C5）。
/// 2. 每个区块先用 [`zone_representative_terrain`]（O(1)，只采样区块
///    左上角一点）做便宜的预筛——代表点本身不可通行（多半是水）的
///    区块直接跳过，不生成整个区块窗口。
/// 3. 预筛通过的区块才用 [`generate_zone_window`] 生成完整的
///    `ZONE_SPAN × ZONE_SPAN` 窗口，对窗口内全部格做连通域分析（见
///    [`ll_world::land::largest_walkable_component`]——与据点选址**共用
///    同一份算法**，见该模块文档），取窗口内最大的连通可行走
///    分量——分析范围只在单个区块窗口内部，不跨区块边界，见该函数
///    文档。
/// 4. 该分量格数 ≥ [`MIN_SPAWN_LAND_AREA`] 时,取分量内光栅序意义上
///    最先被访问到的格,换算成世界坐标返回,搜索结束。
///
/// # 有界：最多完整检查 [`MAX_SPAWN_SEARCH_ZONES`] 个区块
///
/// 预筛本身遍历全部区块（代价是每区块一次 O(1) 噪声采样，便宜，恒
/// 会跑完，不会提前退出），但只有通过预筛、进入第 3 步完整检查的
/// 区块计入 [`MAX_SPAWN_SEARCH_ZONES`] 这个上限——达到上限仍未找到
/// 合格陆地就返回 `None`，调用方（[`build_new_world`]）退回
/// [`carve_spawn_clearing`] 兜底并记一条警告日志。整个函数不含任何
/// 无界循环，也不会 panic。
///
/// # 确定性
///
/// 全程只依赖区块/区块内局部坐标的数值大小与噪声场的纯函数采样，不
/// 引入任何随机性，也不触碰任何 `HashMap`/`HashSet` 的迭代顺序——同一
/// 个种子（连同同一份地形注册结果）永远走完全相同的一条搜索路径，
/// 产出完全相同的出生点（世界身份三要素之一，见
/// `ll_content::world_identity` 模块文档）。
fn find_spawn_site(
    layout: &ZoneLayout,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
    table: &TerrainTable,
) -> Option<(TorusPos, usize)> {
    let zone_count = layout.zone_count();
    let span = layout.zone_span();
    let mut zones_fully_inspected: usize = 0;

    for zone_y in 0..zone_count.height() as i32 {
        for zone_x in 0..zone_count.width() as i32 {
            let zone = zone_count.wrap(zone_x, zone_y);

            let representative =
                zone_representative_terrain(noise, params, layout, zone, terrain_ids);
            if representative.blocks_move(table) {
                continue;
            }

            if zones_fully_inspected >= MAX_SPAWN_SEARCH_ZONES {
                return None;
            }
            zones_fully_inspected += 1;

            let window = generate_zone_window(noise, params, layout, zone, terrain_ids)
                .expect("layout 已在 build_zone_layout 中校验过，区块窗口恒能生成");
            let Some(component) = largest_walkable_component(
                &window,
                layout.local_size(),
                table,
                MIN_SPAWN_LAND_AREA,
            ) else {
                continue;
            };

            // 出生点取分量的 `start`（光栅序最先那一格），不是 `center`
            // ——这是本函数一开始就在用的语义，换成 `center` 会让同一个
            // 种子的玩家出生点漂移，见 `LandComponent::start` 文档。
            let world_x = zone.x() * span as i32 + component.start.x();
            let world_y = zone.y() * span as i32 + component.start.y();
            return Some((layout.tile_size().wrap(world_x, world_y), component.area));
        }
    }

    None
}

/// 生成玩家单位，写入 `world.actors`，`current_space` 取地表。
fn spawn_player(
    world: &mut WorldState,
    pos: TorusPos,
    zone: ll_world::space::ZoneCoord,
    content: &LoadedContent,
) -> EntityId {
    // 玩家的初次可行动时刻取当前世界时钟，不是字面量 `Tick(0)`——
    // `build_new_world` 在调用本函数之前已经把 `world.clock` 设成
    // `NEW_GAME_START_TICK`（早八点，见其赋值处注释）。若这里仍写
    // `Tick(0)`（午夜），`rebuild_timeline` 派生出的时间轴会让玩家
    // 第一次行动时 `TurnEngine::perform` 把 `world.clock` 倒拨回午夜
    // ——时钟不但没有前进，反而先开局倒退了 8 小时。
    let next_action_at = world.clock;
    // 玩家的种族与职业在这条路径上取本体默认值（人类 + 战士）——
    // 选种族/性别/职业是角色创建界面的工作，那条路径不经过本函数，
    // 而是自己调 [`build_player_agent`] 并把玩家选的三项传进去，见
    // `crate::chargen` 模块文档「接缝」一节。
    //
    // 本函数因此**不是**「玩家怎么被造出来」的唯一入口，只是「没有
    // 界面时的默认那一份」。
    //
    // # 默认职业为什么是 `class_ids.warrior`，不是「第一个已注册的职业」
    //
    // 「第一个已注册的」依赖注册顺序，而注册顺序是 **mod 装载顺序**的
    // 函数——第三方 mod 装在本体前面就会让本体玩家的默认职业变成别人
    // 的职业。这是交接文档第五节第 13 条（`ContentIndex` 的裸数值在
    // 任何跨边界场景下都不可作为判据）的同型问题。
    //
    // 也不写字符串字面量 `"lostland:warrior"`：引擎里不按内容 id 分支
    // 是本仓库既有纪律（见 `crate::surface_draw::npc_draws` 文档
    // 「引擎里没有任何一处按种族/职业 id 分支」一节）。`BaseClassIds`
    // 正是为「Rust 侧确实需要按名字引用某条内容」准备的那个句柄，
    // 缺了它装载期就整批失败，不会静默退化。
    world.actors.spawn(build_player_agent(
        pos,
        zone,
        content,
        content.race_ids.human,
        content.class_ids.warrior,
        next_action_at,
    ))
}

/// 按给定种族与职业构造一份厚层玩家快照——`spawn_player` 实际的生成
/// 逻辑参数化在这里，而不是把 `content.race_ids.human` 焊死进字段
/// 字面量：换一个 `race` 就能生成一个属性/出生物品都不同的角色，测试
/// 直接用这一点验证「种族修正真的接线了」，不需要等待选种族 UI 落地
/// 才能验收这条链路。
///
/// # 为什么是公开的（`pub`）
///
/// 它是**「一个角色进入世界」这件事唯一的构造器**，而这件事有两个
/// 调用方，第二个还没写：
///
/// 1. 新游戏（[`spawn_player`]，本模块内部）；
/// 2. 角色创建界面（`crate::chargen`）——玩家选完种族/性别/职业之后
///    造出来的就是这一份快照。
///
/// 还有一个已经被所有者裁定、但属于存档批次的第三个调用方：**肉鸽
/// 模式死亡之后重新入世**（「死亡后变成一般模式，可以再创建角色然后
/// 选择在某个地方出生」）。那条路径要做的与前两条完全一样——拿一份
/// 角色选择 + 一个选好的格子造一个新 `Agent`——把它留在私有作用域里
/// 只会逼那一批把同一段字段字面量抄第三份。
///
/// # 属性修正：一次性烘焙，见 `ll_sim::character` 模块文档
///
/// `stats` 字段不再写死 `BaseStats::BASELINE`——`race` 声明的六项固定
/// 增减量经 [`ll_sim::character::bake_race_stat_modifiers`] 一次性叠加
/// 到基线上，产出的值直接写进 `Agent.stats`，此后不再与 `race_table`
/// 挂钩（烘焙语义，见 `knowledge/design/race-system.md`「二、属性修正」
/// 一节）。未注册的种族索引（正常运行不该发生）退化成裸基线，不是
/// panic——见该函数文档「查不到就是查不到」纪律。
pub fn build_player_agent(
    pos: TorusPos,
    zone: ll_world::space::ZoneCoord,
    content: &LoadedContent,
    race: ll_core::ident::ContentIndex,
    profession: ll_core::ident::ContentIndex,
    next_action_at: Tick,
) -> Agent {
    // 出生携带物品（NPC 生命周期批次：NPC 带物品 → 死亡掉落 → 尸体 →
    // 老化回收，本行是「带物品」这一半在真实生产路径上唯一的接线点
    // ——见 `ll_mod::race::starting_inventory` 文档）：本体三族现在各自
    // 声明了一套开局装备（`mods/lostland/races.json5` 的
    // `starting_items`），因此这条路径对本体内容不再是空 `Vec`；
    // 第三方 mod 通过种族数据文件追加的声明走的是同一行代码。
    //
    // `item_table` 传进去是为了给每件出生装备定耐久初值（满耐久，见
    // `ItemStack::freshly_made`）——出生装备与制作成品、盲盒产出是同
    // 一类东西，共用同一条「新造出来的物品带多少耐久」的规则。
    let starting_items = content
        .race_table
        .get(race)
        .map(|view| ll_mod::race::starting_inventory(&view, &content.item_table))
        .unwrap_or_default();
    // **出生装备只进背包，不自动穿上**（项目所有者裁定，推翻 3191884）。
    // 那一批让本函数调用 `ll_sim::item::outfit_from_inventory` 把可装备
    // 的那几件直接穿上；所有者的新裁定是：
    //
    // > 「这个如果是 NPC 就是根据 NPC 自行决策，人的话就等玩家自己装备吧」
    //
    // 「人」指玩家角色，也就是本函数唯一服务的那一个。**穿什么是玩家的
    // 决定**，世界生成不替他做——他自己翻背包穿上的那一次会走
    // `resolve_equip`，那条路径本来就在，也本来就是玩家表达这个决定的
    // 唯一入口。
    //
    // `outfit_from_inventory` **没有删**：所有者裁定的另一半（「NPC 就是
    // 根据 NPC 自行决策」）还没有落地——本仓库今天没有任何 NPC 生成期的
    // 穿戴决策，那需要一套「这个 NPC 会挑哪件穿」的决策，是独立一批的
    // 工作。那一批要的正是这个函数（把一份背包按装备规则拆成两半），
    // 删掉只会让下一批重写一遍。它连同它的四条单元测试一起留在
    // `ll_sim::item`，见该函数文档「等谁来用」一节。
    let stats =
        ll_sim::character::bake_race_stat_modifiers(BaseStats::BASELINE, race, &content.race_table);
    Agent {
        pos,
        stats,
        next_action_at,
        health: Agent::STARTING_HEALTH,
        // **玩家不挂任何归属，这是裁定不是遗漏。** 项目所有者：「玩家
        // 可以没有势力归属，这个可以通过后面和据点的管理者对话加入。」
        //
        // 文化那一侧同样刻意留空：玩家「没有文化」由判定期回退表达
        // （`ll_sim::ai_query::declared_hostile` 找不到 `Culture` 归属
        // 就回退到 `lostland:cultureless` 哨兵），不写一条指向哨兵的
        // 归属——那会让同一件事有两种表示法，还会给玩家平白增加一条
        // 进存档与世界哈希的记录。NPC 那一条路径（`ll_mod::roster::
        // build_npc_agent`）会挂，两者不对称是有意的。
        affiliations: Vec::new(),
        wallet: 0,
        // 职业由调用方给出，**不再是占位索引**。
        //
        // # 这句注释此前写的是什么，为什么它双重过期
        //
        // 原文是「本体目前没有注册任何职业内容（职业只经 mod 脚本
        // `register-class` 注册）——占位索引是诚实的『尚无职业』表达，
        // 不是缺陷」。两句都不再成立：
        //
        // 1. **脚本系统已被整体拆除**（ADR 0028），`register-class`
        //    这条注册通道今天根本不存在，职业改从内容数据文件装载；
        // 2. **本体有 13 个职业**（`mods/lostland/classes.json5`），
        //    其中三个还被 `ll_mod::class::resolve_base_classes` 解析进
        //    了 `content.class_ids`。
        //
        // 于是那个「诚实的表达」变成了一个真实缺陷：
        // `ContentIndex::default()` 是 0 号索引（`lostland:placeholder_race`
        // 占着），`class_table.is_defined(0)` 为假，因此
        // `class_table.get(玩家职业)` 恒为 `None`——角色面板的主属性
        // 倾向那一行永远不出现，`crate::surface_draw` 的职业挂件对玩家
        // 永远查不到图。玩家不是「看起来没有职业」，是**真的没有职业**。
        //
        // 注释按规格 §13「文档与代码不一致即视为缺陷」重写而不是删掉。
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: starting_items,
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(zone, content.space_ids.surface),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    }
}

/// 物化 NPC 时，从据点锚点向外最多搜多少格找可站立的位置。
///
/// 取 26 = [`ll_world::settlement::MAX_FOOTPRINT_RADIUS`] 的量级：一座
/// 长满的据点（80 栋建筑）外廓半径正是这个数，搜到这里就等于「把整座
/// 村子找了一遍」。再往外就会走进荒野，NPC 会站在离家很远的地方。
const NPC_PLACEMENT_RADIUS: i32 = 26;

/// 把**当前常驻区块里、还有人住、且此前从未物化过**的据点，各自的名册
/// 物化成真正的 `Agent`。
///
/// 返回这一次新生成的实体，调用方负责把它们排进时间轴
/// （[`ll_sim::turn::TurnEngine::schedule`]）——本函数不碰时间轴，与
/// `ll-world`/`ll-sim` 的分层一致（世界状态归 `WorldState`，调度归回合
/// 引擎）。
///
/// # 时机：跟着流式加载走，不另起一套触发机制
///
/// 调用点是 [`crate::app::Demo::maintain_streaming`]——**已经存在的、
/// 每帧真跑一遍的那个钩子**，与 `cleanup_aged_ground_items` 并列（同一
/// 条「复用已有的每帧钩子，比新建一套框架更小、更诚实」的既有理由）。
/// 它排在 `stream_neighborhood` **之后**：物化要读地形判断「这一格能不
/// 能站人」，而那些区块正是上一行刚刚装进来的。
///
/// # 为什么不是「区块首次物化时生成」
///
/// 那是最直觉的时机，也是上一批标记为「唯一没有现成先例」的那个设计
/// 问题——它的麻烦在于**一座据点可以横跨最多 8 个区块**（见
/// `ll_world::settlement` 模块文档「实测」一节）：按区块触发，同一座
/// 据点会被触发多次，每次只该生成「属于这个区块的那几个人」，而名册
/// 本身是按据点派生的、不按区块切分。按**据点**触发则一次到位：一座
/// 据点物化一次，全部人一起出现，`materialized_settlements` 里也只记
/// 一条。
///
/// 代价如实标注：玩家走到一座跨界据点的边缘时，据点另一头（可能还在
/// 未加载区块里）的那几个人会因为找不到常驻地形而被跳过——见
/// [`place_roster`]。他们不会在下次靠近时补上（这座据点已经记成「物化
/// 过」了）。修它需要把「物化」从一次性事件改成可增量的，那要的正是
/// 上面否掉的逐 NPC 身份，属独立批次。
///
/// # 确定性
///
/// 本函数不取任何随机数：名册来自 [`ll_mod::roster::settlement_roster`]
/// （自己走 `DetRng::for_entity`），位置来自一次确定性的方环扫描。据点
/// 的处理顺序由 [`ll_world::chronicle::WorldChronicle::sites_touching_zone`]
/// 与区块光栅序共同决定，不依赖任何 `HashMap` 迭代顺序（约束 C5）。
pub fn materialize_nearby_settlements(
    world: &mut WorldState,
    content: &LoadedContent,
    roles: &ll_mod::roster::SettlementRoles,
) -> Vec<EntityId> {
    let Some(chronicle) = world.terrain.chronicle_handle() else {
        // 没装编年史就没有据点，因此也不该有任何 NPC——「有 NPC 的地方
        // 必然存在一个据点」这条裁定在这里表现为一次直接返回。
        return Vec::new();
    };
    let layout = *world.terrain.layout();

    // 先把「附近有哪些据点」收齐再动世界。
    //
    // 「附近」的口径取**当前常驻的区块集合**，不是「以玩家为中心画一个
    // 方框」：两者在正常游玩时几乎重合（常驻集合就是流式加载按玩家位置
    // 维护出来的），但常驻集合额外满足一条本函数真正需要的性质——
    // [`place_roster`] 只在常驻地形上摆人，非常驻区块里的据点就算被算作
    // 「附近」也一个人都摆不出来，却会被记成「已物化」而**永久错过**。
    // 按常驻集合取，这种情形从源头上不会发生。
    //
    // 一座跨区块的据点会被它覆盖到的每个常驻区块各报一次，按 id 去重
    // （排序 + dedup，不碰任何 HashSet，约束 C5）。
    let mut nearby: Vec<ll_world::settlement::SettlementSite> = Vec::new();
    for zone in world.terrain.resident_zones() {
        for site in chronicle.sites_touching_zone(zone) {
            nearby.push(*site);
        }
    }
    nearby.sort_by_key(|site| site.id.get());
    nearby.dedup_by_key(|site| site.id.get());

    let mut spawned = Vec::new();
    for site in nearby {
        if world.settlement_is_materialized(site.id) {
            continue;
        }
        let roster = ll_mod::roster::settlement_roster(&site, roles, world.seed);
        if roster.is_empty() {
            // 废墟与空据点：名册本来就是空的，**照样记成已物化**——否则
            // 每一帧都会为同一片废墟重跑一次名册派生。
            world.mark_settlement_materialized(site.id);
            continue;
        }
        let spots = place_roster(world, &layout, site.anchor, roster.len());
        let ctx = ll_mod::roster::MaterializeContext {
            races: &content.race_table,
            items: &content.item_table,
            surface_profile: content.space_ids.surface,
            now: world.clock,
            // 据点的文化直接转发给物化——`Agent::affiliations` 的第一个
            // 生产者，见 `ll_mod::roster::build_npc_agent` 文档「文化
            // 归属」一节。
            culture: site.culture,
        };
        let mut made = Vec::new();
        for (profile, pos) in roster.iter().zip(spots) {
            let zone = layout.tile_to_zone(pos).0;
            made.push(ll_mod::roster::build_npc_agent(
                profile, pos, zone, roles, &ctx,
            ));
        }
        for agent in made {
            spawned.push(world.actors.spawn(agent));
        }
        world.mark_settlement_materialized(site.id);
        tracing::info!(
            site = site.id.get(),
            population = site.population,
            roster = roster.len(),
            spawned = spawned.len(),
            "据点 NPC 物化完成"
        );
    }
    spawned
}

/// 为一份名册找 `wanted` 个可站立、互不重叠的位置。
///
/// 从锚点开始按方环由内向外扫描（半径 0、1、2……直到
/// [`NPC_PLACEMENT_RADIUS`]），每一环内按行主序——完全确定，不取随机数。
///
/// 只认**已常驻**的地形（[`ll_world::surface_store::SurfaceStore::terrain_at_resident`]）：
/// 落在尚未加载区块里的那些格一律跳过，绝不为了摆一个 NPC 去触发一次
/// 区块生成（那会让物化路径悄悄把整座跨界据点的邻区块都拉进内存）。
/// 找不到足够多的位置就返回少于 `wanted` 个，调用方按 `zip` 自然截断
/// ——多出来的那几个人这一局就不出现，见
/// [`materialize_nearby_settlements`] 文档「代价如实标注」一节。
///
/// 已被其他实体占着的格子也跳过：本仓库的移动结算不允许两个实体站在
/// 同一格，物化时就摞在一起会让他们互相卡死。
fn place_roster(
    world: &WorldState,
    layout: &ZoneLayout,
    anchor: TorusPos,
    wanted: usize,
) -> Vec<TorusPos> {
    let tile_size = layout.tile_size();
    let occupied: Vec<TorusPos> = world.actors.iter().map(|agent| agent.pos).collect();
    let mut spots = Vec::with_capacity(wanted);
    for ring in 0..=NPC_PLACEMENT_RADIUS {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if spots.len() >= wanted {
                    return spots;
                }
                // 只取这一环最外圈的格子，内圈上一轮已经看过了。
                if ring > 0 && dx.abs() != ring && dy.abs() != ring {
                    continue;
                }
                let pos = tile_size.wrap(anchor.x() + dx, anchor.y() + dy);
                let Some(kind) = world.terrain.terrain_at_resident(pos) else {
                    continue;
                };
                if kind.blocks_move(&world.terrain_table) {
                    continue;
                }
                if occupied.contains(&pos) || spots.contains(&pos) {
                    continue;
                }
                spots.push(pos);
            }
        }
    }
    spots
}

/// 供渲染/存档使用的一张干净地形表克隆——存档读入（`load_full`）需要
/// 「当前会话按同一次装载重新注册出的表」，与写出时的表逐字段相同但
/// 不是同一个实例（读档之后 `WorldState` 是另一份反序列化出的世界，
/// 见 `ll_content::save_file::load_full` 文档）。
pub fn cloned_terrain_table(content: &LoadedContent) -> TerrainTable {
    content.terrain_table.clone()
}

/// 每帧维护：清理老化超过默认阈值的地面物品
/// （[`WorldState::cleanup_aged_ground_items`]，第二批已实装但当时
/// 「零生产调用方」——NPC 生命周期批次给它接上真正的调用点）。
///
/// # 为什么挂在这里，不是新建一套惰性追赶系统
///
/// 第二批当时的设想是接到「玩家靠近远景区域时顺带扫一次」这条惰性
/// 追赶路径上——但那套系统当前不存在（`ll_world::surface_store` 的
/// `stream_neighborhood` 只惰性生成地形，不涉及任何「扫一遍列表」这类
/// 通用惰性追赶框架，新建一套只为这一个清理调用点服务是过度设计）。
/// [`crate::app::Demo::advance`] 每帧都会调用
/// [`WorldState::terrain::stream_neighborhood`](ll_world::surface_store::SurfaceStore::stream_neighborhood)
/// 维护流式邻域——这正是当前代码库里**已经存在**的、每帧真正跑一遍的
/// 位置，本函数与它并列调用，理由同 `crate::app` 模块文档「不做…… 聚焦
/// 『能玩、能存』这条最小闭环」：复用已有的每帧钩子，比新建一套框架
/// 更小、更诚实。
///
/// # 开销：每帧一次 `O(n)` 线性扫描，`n` 是地面物品堆数
///
/// `WorldState::cleanup_aged_ground_items` 内部是 `Vec::retain`——对每
/// 条地面物品堆做一次减法+比较，n 在真实游玩场景里是"当前世界还没被
/// 拾取/搜刮/老化清理掉的地面物品堆总数"，量级是几十到几百（背包/尸体
/// 批次都没有引入任何会让这个数字失控增长的机制：拾取、搜刮、丢弃互相
/// 抵消，老化本身也持续把这个数字往下拉），比同一帧紧挨着调用的
/// `stream_neighborhood`（要遍历一整圈区块窗口的地形瓦片）便宜一到
/// 两个数量级——不需要额外的节流（按帧计数器跳过若干帧）机制,那类
/// 节流本身需要新增状态与测试,收益在这个开销量级下不成比例。
///
/// # 为什么不是"只在世界时钟真正推进时才清理"
///
/// 严格来说，`world.clock` 在当前 `Demo` 这套最小闭环里还没有任何
/// 生产代码会推进它（`ll_world::state::WorldState::advance` 目前只在
/// `build_new_world` 建局那一刻调用一次，见 `crate::world` 模块「新
/// 游戏起始时钟」相关常量）——这是先于本批次就存在的独立缺口，不在本
/// 批次修复范围内。本函数只负责"给 `cleanup_aged_ground_items` 一个
/// 真实的、每帧都会被调用的生产入口"这一件事,不去动"世界时钟该如何
/// 推进"这个更大的、独立的问题——一旦时钟推进被接上,地面物品就会在
/// 真实游玩里按预期老化,不需要再改这里一行代码。
///
/// # 这里此前多一个 `content` 参数，用来折算「哪些不老化」
///
/// 家具层那一批要给 [`WorldState::cleanup_aged_ground_items`] 传一个
/// `is_permanent` 谓词——`ll-world` 不知道「家具」是什么
/// （`ItemDef.furniture` 住在下游的 `ll-mod`），所以判据只能由本函数
/// （生产路径上唯一同时拿得到世界与物品表的地方）折算出来传进去。
///
/// **家具放置状态落地之后那条理由消失了**：会不会老化现在取决于这一堆
/// 自己身上的 `GroundItemStack::placed`，`ll-world` 完全看得见，谓词
/// 因此在那一侧一并收掉（见该方法文档）。本函数随之不再需要 `content`。
///
/// 语义也随之更正：一座**躺在地上没立起来**的炉子此前因为
/// `ItemDef.furniture` 为真而永不老化，现在照常老化——只有真正立在那里
/// 的才不老化。
pub fn cleanup_aged_ground_items(world: &mut WorldState) -> usize {
    world.cleanup_aged_ground_items(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::torus::TorusSize;
    use std::collections::VecDeque;

    fn test_content() -> LoadedContent {
        let dir = crate::test_support::unique_temp_path("ll-game-world-test-content");
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
        // mods_root 指向仓库真实的 mods/ 目录（本体内容住在
        // mods/lostland/，临时空目录下契约解析必然失败）；assets_root
        // 仍指向临时目录，本文件的测试不需要真实贴图。
        let content = crate::content::load_content(
            &crate::test_support::repo_mods_dir(),
            &dir.join("assets"),
        )
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
        let _ = std::fs::remove_dir_all(&dir);
        content
    }

    #[test]
    fn 出生点在世界建成后立刻可站立() {
        // Arrange & Act
        let content = test_content();
        let game_world = build_new_world(
            &content,
            GenParams {
                seed: 1,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");

        // Assert
        let pos = game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家刚生成，必然存在")
            .pos;
        let kind = game_world
            .world
            .terrain_at(pos)
            .expect("出生点所在区块已被出生邻域预热");
        assert!(!kind.blocks_move(&game_world.world.terrain_table));
    }

    #[test]
    fn 真实生产入口清理老化地面物品而保留未老化的() {
        // 验证 crate::world::cleanup_aged_ground_items——crate::app::Demo::advance
        // 每帧真正调用的同一个函数（见其文档「为什么挂在这里」一节）
        // ——在一个由 build_new_world 建出的真实 GameWorld（真实注册表/
        // 地形，不是手搭的裸 WorldState）上生效：老化超过阈值的地面
        // 物品被移除，未超过阈值的保留。不验证 Demo::advance 本身
        // （需要 GPU/窗口，本测试模块没有那套环境）,验证的是它每帧
        // 调用的同一个生产函数在真实世界上确实生效。
        // Arrange
        use ll_core::ident::{Interner, NamespacedId};
        use ll_world::item::{GroundItemStack, ItemStack};
        let content = test_content();
        let mut game_world = build_new_world(
            &content,
            GenParams {
                seed: 1,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let mut interner = Interner::new();
        let arrow = interner.intern(NamespacedId::parse("lostland:arrow").expect("合法标识符"));
        let pos = game_world.world.size.wrap(0, 0);
        let stale_dropped_at = game_world.world.clock;
        game_world.world.ground_items.push(GroundItemStack {
            pos,
            stack: ItemStack::new(arrow, 1),
            dropped_at: stale_dropped_at,
            contents: Vec::new(),
            placed: false,
        });
        game_world
            .world
            .advance(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS + 1);
        // 第二堆在时钟推进之后才丢下,此刻的 age 是 0,应当保留。
        game_world.world.ground_items.push(GroundItemStack {
            pos,
            stack: ItemStack::new(arrow, 1),
            dropped_at: game_world.world.clock,
            contents: Vec::new(),
            placed: false,
        });

        // Act
        let removed = cleanup_aged_ground_items(&mut game_world.world);

        // Assert：老化的一条被清掉,同一时刻新丢的一条（dropped_at 恰好
        // 等于清理前的 clock,推进后未超阈值）保留。
        assert_eq!(removed, 1);
        assert_eq!(game_world.world.ground_items.len(), 1);
    }

    #[test]
    fn player_entity字段被显式赋值而非保持默认空值() {
        // 直接对应模块文档强调的那处已知易漏坑：漏赋值会让探索记忆
        // 永远收不到写入,见 build_new_world 文档。
        // Arrange & Act
        let content = test_content();
        let game_world = build_new_world(
            &content,
            GenParams {
                seed: 1,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");

        // Assert
        assert_eq!(game_world.world.player_entity, Some(game_world.player));
    }

    #[test]
    fn 最小安全缩放下的可见范围不超出常驻区块集合保证的边距() {
        // 直接对应 MIN_SAFE_ZOOM 文档「推导」一节的核心断言：
        // STREAM_RADIUS_ZONES × ZONE_SPAN = 2 × 48 = 96 格是常驻区块
        // 集合保证的、与玩家具体站位无关的最小边距,MIN_SAFE_ZOOM 换算
        // 出的可见半径必须严格小于它(不能只是"不大于",否则玩家恰好
        // 站在区块边界时会撞见未常驻的坐标)。
        // Arrange
        let resident_margin = STREAM_RADIUS_ZONES * ZONE_SPAN as i32;

        // Act
        let half_extent_x = ll_render::camera::visible_half_extent(
            640,
            ll_render::camera::Zoom::new(MIN_SAFE_ZOOM),
        );
        let half_extent_y = ll_render::camera::visible_half_extent(
            360,
            ll_render::camera::Zoom::new(MIN_SAFE_ZOOM),
        );

        // Assert
        assert!(half_extent_x < resident_margin);
        assert!(half_extent_y < resident_margin);
    }

    #[test]
    fn 最大安全缩放不改变最小安全缩放的钳制上限() {
        // 拉近永远不会让可见范围超出常驻区块集合——这里只锁住
        // MAX_SAFE_ZOOM 确实复用了 Zoom 的通用上限,没有被本文件意外
        // 收窄或放宽。
        // Arrange & Act & Assert
        assert_eq!(MAX_SAFE_ZOOM, ll_render::camera::Zoom::MAX);
    }

    #[test]
    fn 相同种子产出的两个世界出生点地形一致() {
        // 世界身份三要素之一（种子）在「世界创建」这一步的直接体现：
        // 同一种子、同一批内容,出生点地形必须逐位一致。
        // Arrange
        let content = test_content();

        // Act
        let first = build_new_world(
            &content,
            GenParams {
                seed: 42,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let second = build_new_world(
            &content,
            GenParams {
                seed: 42,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");

        // Assert
        assert_eq!(first.world.terrain_at(first.world.size.wrap(10, 10)), {
            second.world.terrain_at(second.world.size.wrap(10, 10))
        });
    }

    #[test]
    fn 相同种子两次搜索产出完全相同的出生点坐标() {
        // 世界身份三要素之一（种子）在「出生点选址」这一步的直接体现
        // ——find_spawn_site 不引入任何随机性、不依赖任何哈希容器迭代
        // 顺序（约束 C5），同一种子必须永远选中同一个出生点，不能只是
        // 「地形一致」，坐标本身也必须逐位相同。
        // Arrange
        let content = test_content();

        // Act
        let first = build_new_world(
            &content,
            GenParams {
                seed: 42,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let second = build_new_world(
            &content,
            GenParams {
                seed: 42,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let first_pos = first
            .world
            .actors
            .get(first.player)
            .expect("玩家刚生成，必然存在")
            .pos;
        let second_pos = second
            .world
            .actors
            .get(second.player)
            .expect("玩家刚生成，必然存在")
            .pos;

        // Assert
        assert_eq!(first_pos, second_pos);
    }

    #[test]
    fn 世界几乎全是水时出生点搜索在有界步数内放弃而不panic() {
        // 直接对应「有界」这条硬约束：把海平面阈值调到噪声输出区间
        // （0..=1000，见 `ll_world::noise` SCALE_MAX）完全够不到的高度,
        // 保证全世界逐格都是深水,find_spawn_site 找不到任何合格候选。
        // 这条测试真正验证的是「函数确实会返回 None 收场,而不是死循环
        // 或 panic」——用一个远小于本体默认区块数（64×48 = 3072）的
        // 小布局把测试跑快，不代表搜索上限本身与布局大小有关。
        // Arrange
        let content = test_content();
        let zone_count = TorusSize::new(4, 4).expect("4x4 是合法尺寸");
        let layout = ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束");
        let params = GenParams {
            seed: 5,
            shape: ll_world::generate::TerrainShape {
                sea_level: 100_000,
                ..ll_world::generate::TerrainShape::default()
            },
        };
        let noise = build_zone_noise(&layout, &params).expect("布局满足生成入口的约束");

        // Act
        let result = find_spawn_site(
            &layout,
            &noise,
            &params,
            &content.terrain_ids,
            &content.terrain_table,
        );

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn 出生点周围连通可行走区域超过最小陆地面积阈值() {
        // 这是本模块最重要的一条断言,直接编码项目所有者的实测抱怨：
        // 出生点四周得真能走开,不能只是「出生点本身这一格能走」。用
        // 洪水填充（BFS）实测出生点所在连通可行走区域的大小,断言不小
        // 于 find_spawn_site 选址时要求的阈值——若把出生点强行改回旧版
        // 硬编码坐标 `(0, 0)`（不经过 find_spawn_site 搜索）,这条断言
        // 会在出生点落在水域的种子上失败,已手工验证过（见任务报告）。
        // Arrange
        let content = test_content();
        let game_world = build_new_world(
            &content,
            GenParams {
                seed: 1,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let spawn = game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家刚生成，必然存在")
            .pos;

        // Act：有界洪水填充，上限给到阈值的数倍——只需要证明「远超
        // 阈值」，不需要真的数完整片连通区域。
        let area = flood_fill_walkable_area(&game_world.world, spawn, MIN_SPAWN_LAND_AREA * 4);

        // Assert
        assert!(
            area >= MIN_SPAWN_LAND_AREA,
            "出生点周围连通可行走区域只有 {area} 格，未达到阈值 {MIN_SPAWN_LAND_AREA}"
        );
    }

    /// 测试专用：从 `start` 出发做有界洪水填充，统计连通可行走区域
    /// 大小，达到 `cap` 格后提前停止。
    ///
    /// 用 `Vec<TorusPos>` 线性查找记录已访问坐标，不用 `HashSet`——
    /// 这不是热路径（只在测试里跑一次，`cap` 通常只有几千），换取的是
    /// 不必为「区块内局部坐标」之外的、跨越任意区块边界的世界坐标
    /// 另外设计一套下标方案；`ll_world::land::largest_walkable_component`（生产
    /// 路径）仍然用数组下标,这里的取舍只服务测试代码本身。
    fn flood_fill_walkable_area(world: &WorldState, start: TorusPos, cap: usize) -> usize {
        let mut visited: Vec<TorusPos> = vec![start];
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(pos) = queue.pop_front() {
            if visited.len() >= cap {
                break;
            }
            for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let neighbor = world.size.wrap(pos.x() + dx, pos.y() + dy);
                if visited.contains(&neighbor) {
                    continue;
                }
                let Some(kind) = world.terrain_at(neighbor) else {
                    // 未常驻的区块视作探索边界，不计入——出生邻域预热
                    // 半径足够覆盖单个区块窗口内的连通分量，真正撞到
                    // 这个分支只会让统计结果偏保守，不会误报。
                    continue;
                };
                if kind.blocks_move(&world.terrain_table) {
                    continue;
                }
                visited.push(neighbor);
                queue.push_back(neighbor);
                if visited.len() >= cap {
                    break;
                }
            }
        }

        visited.len()
    }

    /// 本批次（世界历史生成）的核心验收：**历史真的改变了世界当前的
    /// 样子**，不是只产出了一份谁也不看的事件日志。
    ///
    /// 断言链条完整走一遍：编年史里有据点 → 据点所在区块流式加载进来
    /// → 那个区块的地形里真的立着建筑（墙/地板），而这些地形在纯噪声
    /// 生成的世界里绝不可能出现（`terrain_at_coord` 只产出八种自然
    /// 地形，见 `ll_world::generate`）。
    #[test]
    fn 历史生成的据点真的立在世界地形里() {
        // Arrange
        let content = test_content();
        let game_world = build_new_world(
            &content,
            GenParams {
                seed: 20260825,
                ..GenParams::default()
            },
        )
        .expect("默认布局满足全部前置条件");
        let site = *game_world
            .world
            .terrain
            .chronicle()
            .expect("新游戏必然装上了编年史")
            .sites()
            .first()
            .expect("三百年历史里至少建起过一座据点");

        // Act：把据点所在区块流式加载进来。
        let mut world = game_world.world;
        let span = world.terrain.layout().zone_span() as i32;
        let mut building_tiles = 0usize;
        for dy in 0..span {
            for dx in 0..span {
                let pos = world
                    .size
                    .wrap(site.zone.x() * span + dx, site.zone.y() * span + dy);
                let kind = world.terrain.terrain_at(
                    &game_world.noise,
                    &game_world.params,
                    &content.terrain_ids,
                    pos,
                    Tick(0),
                );
                if kind == content.terrain_ids.wall_wood
                    || kind == content.terrain_ids.wall_stone
                    || kind == content.terrain_ids.floor_wood
                    || kind == content.terrain_ids.floor_stone
                {
                    building_tiles += 1;
                }
            }
        }

        // Assert：一栋 5×5 的屋子就有 25 格，据点至少有一栋。
        assert!(
            building_tiles >= 25,
            "据点 {:?} 所在区块只有 {building_tiles} 格建筑地形，历史没有真的落到地形上",
            site.id
        );
    }

    /// 同一个种子建两次世界，据点与历史事件必须逐字段相同——确定性
    /// （约束 C3）在**整条启动路径**上的验收，不只是编年史模块内部。
    #[test]
    fn 同一种子两次建世界产出相同的据点与历史() {
        // Arrange
        let content = test_content();

        // Act
        let first = build_new_world(
            &content,
            GenParams {
                seed: 4242,
                ..GenParams::default()
            },
        )
        .expect("默认布局满足全部前置条件");
        let second = build_new_world(
            &content,
            GenParams {
                seed: 4242,
                ..GenParams::default()
            },
        )
        .expect("默认布局满足全部前置条件");

        // Assert
        let a = first.world.terrain.chronicle().expect("必然装上了编年史");
        let b = second.world.terrain.chronicle().expect("必然装上了编年史");
        assert_eq!(a.next_world_id(), b.next_world_id());
        assert_eq!(a.sites().len(), b.sites().len());
        for (x, y) in a.sites().iter().zip(b.sites()) {
            assert_eq!(x, y, "同一种子的据点出现分歧");
        }
        assert_eq!(a.events().len(), b.events().len());
        for (x, y) in a.events().iter().zip(b.events()) {
            assert_eq!(x, y, "同一种子的历史事件出现分歧");
        }
        // 世界哈希覆盖了地形本身——据点已经铺进出生邻域的话，这条
        // 断言同时守住了「铺设结果逐格相同」。
        assert_eq!(first.world.hash(), second.world.hash());
    }

    /// **跨区块据点真的出现了**——项目所有者裁决「要」的那一条，在
    /// 本体默认世界（不是测试用的小世界）上的验收。
    ///
    /// # 为什么这条测试必须在这里，而不在 `ll_world::chronicle` 里
    ///
    /// 「据点会不会跨出自己那一格区块」取决于**本体默认布局的区块边长**
    /// （48）与**本体资源名册**给出的承载力——两者都只有在真实
    /// `LoadedContent` + `build_zone_layout` 这条生产路径上才成立。
    /// `ll-world` 的单元测试用的是自己拼的小世界与夹具资源表，它能验收
    /// 「机制在」，验收不了「本体默认参数下真的发生」——而上一批次留下
    /// 的正是这个缺口：机制在、测试全绿、实测跨区块据点数为 0。
    ///
    /// 实测（release，三个种子）：每个世界 21~29 座据点跨区块，最多的
    /// 一座覆盖 8 个区块。这里只断言「至少一座」——具体数字是玩法数值
    /// 调整的结果，钉死它会让每次调参都要改测试；而「至少一座」是那条
    /// 裁决本身，它不该因为调参而失效。
    #[test]
    fn 本体默认世界里真的存在跨区块的据点() {
        // Arrange
        let content = test_content();
        let layout = build_zone_layout().expect("默认布局满足全部前置条件");
        let game_world = build_new_world(
            &content,
            GenParams {
                seed: 4242,
                ..GenParams::default()
            },
        )
        .expect("默认布局满足全部前置条件");
        let chronicle = game_world
            .world
            .terrain
            .chronicle()
            .expect("必然装上了编年史");

        // Act
        let crossing = chronicle
            .sites()
            .iter()
            .filter(|site| ll_world::settlement::footprint_zones(site, &layout).len() > 1)
            .count();

        // Assert
        assert!(
            crossing > 0,
            "{} 座据点里一座都没有跨出自己那一格区块——据点规模又缩回去了，             见 ll_world::settlement 模块文档「实测」一节",
            chronicle.sites().len()
        );
    }

    /// 编年史分配掉的 `WorldId` 不会被游戏内的击杀记录再发一次。
    #[test]
    fn 世界id计数器越过了编年史已经分配掉的号段() {
        // Arrange
        let content = test_content();
        let game_world = build_new_world(
            &content,
            GenParams {
                seed: 20260825,
                ..GenParams::default()
            },
        )
        .expect("默认布局满足全部前置条件");

        // Act
        let reserved = game_world
            .world
            .terrain
            .chronicle()
            .expect("必然装上了编年史")
            .next_world_id();

        // Assert
        assert!(
            game_world.world.next_world_id >= reserved,
            "WorldState 的计数器 {} 低于编年史已经分配到的 {reserved}，两者会撞号",
            game_world.world.next_world_id
        );
    }

    /// 测试帮手：借 `build_new_world` 建一局真实世界（内部会用
    /// `content.race_ids.human` 走一遍 `spawn_player`），只取它已经验证
    /// 过「可站立」的出生 `pos`/`zone`——本批次新增的测试要验证的是
    /// `build_player_agent` 换一个种族后属性是否正确，不是出生点选址
    /// 算法本身，复用已验证过的坐标而不是手搭一对可能非法的坐标。
    fn spawn_pos_and_zone(content: &LoadedContent) -> (TorusPos, ll_world::space::ZoneCoord) {
        let game_world = build_new_world(
            content,
            GenParams {
                seed: 1,
                ..GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let agent = game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家刚生成，必然存在");
        let zone = match agent.current_space {
            Space::Surface { zone, .. } => zone,
            _ => panic!("build_new_world 生成的玩家 current_space 恒为地表"),
        };
        (agent.pos, zone)
    }

    #[test]
    fn 用带非零属性修正的种族生成的角色属性真的包含了修正() {
        // 端到端验收本批次的核心接线：build_player_agent（spawn_player
        // 实际的生成逻辑）用一个声明了「+2 体质 +1 力量」修正的种族
        // （本体自带的矮人，见 ll_mod::race::BaseRaceIds 文档）生成角色
        // 后，角色的 stats 字段必须真的包含这份修正，不能仍是裸基线。
        // Arrange
        let content = test_content();
        let (pos, zone) = spawn_pos_and_zone(&content);

        // Act
        let dwarf_agent = build_player_agent(
            pos,
            zone,
            &content,
            content.race_ids.dwarf,
            content.class_ids.warrior,
            Tick(0),
        );

        // Assert
        assert_eq!(
            dwarf_agent.stats.constitution,
            BaseStats::BASELINE.constitution + 2
        );
        assert_eq!(dwarf_agent.stats.strength, BaseStats::BASELINE.strength + 1);
    }

    #[test]
    fn 三个本体种族生成的角色出生时背包里真的有各自那套开局装备() {
        // 出生装备这条接线在**真实生产路径**上的端到端证据：
        // mods/lostland/races.json5 给三族各写了一套 starting_items，
        // build_player_agent（spawn_player 实际的生成逻辑）必须把它们
        // 一件不落地放进**背包**，且 equipment 保持为空。
        //
        // 「不自动穿上」是项目所有者的裁定（推翻 3191884）：
        // 「人的话就等玩家自己装备吧」。本条正面钉住这一点——若有人把
        // `outfit_from_inventory` 那一步加回 build_player_agent，
        // `agent.equipment.is_empty()` 立刻变红。
        //
        // 断言比的是「多少堆」这个形状而不是具体 id 列表——逐条 id 的
        // 裁定由 ll-mod/tests/base_mod_races.rs 钉住（那里能直接拿到
        // Registry 反查 id），本条只负责证明「声明真的流到了
        // Agent::inventory」，不重复钉同一份内容。
        // Arrange
        let content = test_content();
        let (pos, zone) = spawn_pos_and_zone(&content);

        for (race, expected_stacks) in [
            (content.race_ids.human, 2),
            (content.race_ids.dwarf, 3),
            (content.race_ids.elf, 4),
        ] {
            // Act
            let agent = build_player_agent(
                pos,
                zone,
                &content,
                race,
                content.class_ids.warrior,
                Tick(0),
            );

            // Assert：进的是背包，不是装备栏——出生装备是「行囊里有
            // 什么」，玩家自己决定穿哪件（见 races.json5「语义」一节）。
            assert_eq!(agent.inventory.len(), expected_stacks);
            assert!(
                agent.equipment.is_empty(),
                "出生装备不自动穿上，装备栏必须是空的（项目所有者裁定）"
            );
            // 每一堆都是**全新**的：耐久等于它那条定义声明的上限
            // （`ItemStack::freshly_made`），不是 None。三套里各有一件
            // 声明了耐久上限的东西（衬衣 50 / 手套 40 / 骨针 60）。
            for stack in &agent.inventory {
                let expected = ll_sim::item::ItemCatalog::item(&content.item_table, stack.def)
                    .expect("出生装备必然是已注册的物品")
                    .max_durability;
                assert_eq!(stack.durability, expected);
            }
            assert!(
                agent
                    .inventory
                    .iter()
                    .any(|stack| stack.durability.is_some()),
                "三族各自那套开局装备里都该有一件会磨损的东西"
            );
        }
    }

    #[test]
    fn 新游戏生成的玩家真的有一个查得到定义的职业() {
        // 交接文档待裁定第 13 条（「玩家永远是钢蓝色英雄图，哪怕他是
        // 铁匠」）的真正根因：他根本没有职业。`spawn_player` 此前把
        // `profession` 写死成 `ContentIndex::default()`（0 号占位索引），
        // 于是 `class_table.get(..)` 恒为 `None`。
        //
        // 断言刻意**不比裸整数**（交接文档第五节第 13 条：`ContentIndex`
        // 的裸数值在跨边界场景下不可作判据），而是问「这个索引在职业表
        // 里查得到定义吗」——那才是缺陷的实质。
        // Arrange
        let content = test_content();

        // Act
        let game_world = build_new_world(
            &content,
            GenParams {
                seed: 20260828,
                ..GenParams::default()
            },
        )
        .expect("默认布局满足全部前置条件");
        let player = game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家刚生成，必然存在");

        // Assert
        assert!(
            content.class_table.is_defined(player.profession),
            "新游戏生成的玩家职业索引在职业表里查不到定义——他没有职业"
        );
        assert_ne!(
            player.profession,
            ll_core::ident::ContentIndex::default(),
            "玩家职业仍然是占位索引"
        );
    }

    #[test]
    fn 修正为零的人类种族生成的角色属性等于基线() {
        // 反例：证明上一条测试不是「无论如何都加点什么」——零修正的
        // 人类种族，生成结果的 stats 必须原样等于基线。
        // Arrange
        let content = test_content();
        let (pos, zone) = spawn_pos_and_zone(&content);

        // Act
        let human_agent = build_player_agent(
            pos,
            zone,
            &content,
            content.race_ids.human,
            content.class_ids.warrior,
            Tick(0),
        );

        // Assert
        assert_eq!(human_agent.stats, BaseStats::BASELINE);
    }

    #[test]
    fn 真实mod种族half_elf生成的角色属性包含敏捷与魅力与幸运修正() {
        // ADR 0018「API 完备性判据要求有真实 mod 脚本为证」——本测试
        // 装载仓库真实的 mods/example_mod/races.json5
        // （`register-race "examplemod:half_elf" ... 0 1 0 0 0 1 1 6 1 1 150`，
        // 第 3~9 个参数是七项属性修正：力量0/敏捷1/体质0/智力0/意志0/
        // 魅力1/幸运1），断言用这个真实 mod 种族生成的角色属性确实带上
        // 了敏捷 +1、魅力 +1、幸运 +1，其余四项不变——不是靠临时构造的
        // 测试脚本文本自证。
        //
        // 幸运那一条是 `luck-mod` 这个本批次新增参数在**已发货脚本**上
        // 的证据：把 races.json5 里那个 1 改回 0，这条测试立刻变红。
        // Arrange
        let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
        let assets_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let content = crate::content::load_content(&mods_root, &assets_root)
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
        let half_elf = content
            .registry
            .get(&ll_core::ident::NamespacedId::parse("examplemod:half_elf").expect("合法标识符"))
            .expect("example_mod 应已注册 half_elf 种族");
        let (pos, zone) = spawn_pos_and_zone(&content);

        // Act
        let half_elf_agent = build_player_agent(
            pos,
            zone,
            &content,
            half_elf,
            content.class_ids.warrior,
            Tick(0),
        );

        // Assert
        assert_eq!(
            half_elf_agent.stats.dexterity,
            BaseStats::BASELINE.dexterity + 1
        );
        assert_eq!(
            half_elf_agent.stats.charisma,
            BaseStats::BASELINE.charisma + 1
        );
        assert_eq!(half_elf_agent.stats.luck, BaseStats::BASELINE.luck + 1);
        assert_eq!(half_elf_agent.stats.strength, BaseStats::BASELINE.strength);
        assert_eq!(
            half_elf_agent.stats.constitution,
            BaseStats::BASELINE.constitution
        );
        assert_eq!(
            half_elf_agent.stats.intelligence,
            BaseStats::BASELINE.intelligence
        );
        assert_eq!(
            half_elf_agent.stats.willpower,
            BaseStats::BASELINE.willpower
        );
    }
}
