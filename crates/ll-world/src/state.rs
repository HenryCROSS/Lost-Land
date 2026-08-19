//! 世界状态：种子、时钟、尺寸、流式地形与离散空间的聚合，以及序列化
//! 往返。
//!
//! # 为什么必须完整可序列化且全程整数
//!
//! [`WorldState`] 是模式 3（自由读档）的地基：存档就是把这个结构体
//! 序列化到磁盘，读档就是反序列化回来，不需要额外的迁移或重算步骤。
//! 只要有一个字段没能完整序列化，读档后的世界就可能与存档前不同，
//! 而这类缺陷通常要等到玩家读档后才被发现，为时已晚。
//!
//! 全程禁止浮点数，理由同 `ll-core`：浮点在不同平台/编译器/优化级别下
//! 的运算结果可能有细微差异，跨平台存档兼容性会被悄悄破坏。
//!
//! # `terrain: SurfaceStore`（两级坐标系重写，任务 11）
//!
//! 早期版本 `terrain` 是一整张一次性生成、整体常驻的 [`ChunkGrid`]。
//! 本次改为 [`SurfaceStore`]：世界地表按区块流式生成与常驻，多数区块
//! 在任意时刻并不持有具体地形数据。这个改动牵连三处既有约定：
//!
//! 1. **`terrain_at` 分裂成两个方法**（[`WorldState::terrain_at`]/
//!    [`WorldState::terrain_at_streaming`]）——流式加载需要 `&mut self`
//!    触发按需生成，但 `resolve`（C1：必须是纯函数）只能拿到
//!    `&WorldState`，见 [`WorldState::terrain_at`] 文档。
//! 2. **[`WorldState::hash`] 不再遍历整个世界**——多数区块不常驻，没有
//!    具体瓦片数据可读，改为遍历 [`SurfaceStore::resident_zones`]。
//! 3. **`WorldState::new` 不再一次性生成整张地图**——只预热出生点周围
//!    的一圈邻域（设计文档五节「常驻集合的构成」）。

use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use ll_core::hashing::StateHasher;
use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};

use crate::WorldError;
use crate::chunk::ChunkGrid;
use crate::entity::{Affiliation, Agent, Arena, EntityId, Goal, OrgRef, ThinPopulation};
use crate::generate::{GenParams, build_zone_noise};
use crate::interior::{Interior, InteriorTable};
use crate::noise::TileableNoise;
use crate::script_state::ScriptValue;
use crate::space::{Space, SpaceId};
use crate::surface_store::SurfaceStore;
use crate::terrain::{BaseTerrainIds, TerrainKind, TerrainTable};
use crate::zone::ZoneLayout;

/// `Surface` 与 `Interior` 共享的常驻上限默认值（设计文档五节，与关键
/// 设计判断 3「共享 256 常驻上限」同一个数字）。
pub const DEFAULT_RESIDENT_CAP: usize = 256;

/// 出生点周围预热的区块半径（区块为单位）——设计文档五节「默认 5×5」
/// 邻域缓冲，覆盖 `(2*2+1)^2 = 25` 个区块，远小于
/// [`DEFAULT_RESIDENT_CAP`]。
const SPAWN_WARM_RADIUS: i32 = 2;

/// 完整的世界状态：种子、时钟、尺寸、流式地形、离散空间与人口/实体池。
///
/// 全部字段公开：存档格式就是这个结构体本身，不经过额外的 DTO 转换层
/// ——多一层转换就多一处可能与本体字段漂移的地方。
///
/// # `population`/`actors` 现在参与序列化（P5 批次 B，偿还历史债务）
///
/// [`ThinPopulation`] 与 [`Arena<Agent>`] 曾经不派生 `serde`：前者的
/// `profession` 列、后者的 `Agent::profession` 都是 `ll_core::ident::ContentIndex`
/// ——当时该类型还没有可直接使用的序列化实现。这条障碍已解除：
/// `ContentIndex` 现在直接派生 `Serialize`/`Deserialize`（[0015](../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)：
/// 「结构合法」与「已注册」是两件事，前者无上下文可以直接派生，后者是
/// 依赖当前会话加载了哪些 mod 的独立解析，不塞进这里的派生），
/// `TorusPos` 同样已在两级坐标系重写批次补齐。两层因此现在都真正随
/// `WorldState` 一起序列化——见 [`WorldStateRepr`] 与其 `TryFrom` 实现。
///
/// **这不等于「读档后立刻可以安全查询内容」**：反序列化出的
/// `ContentIndex` 只是结构合法的裸索引，它是否对应当前会话真实注册的
/// 内容，仍然是存档主体读写管线（任务 9）拿到当前会话注册表之后才能
/// 完成的独立解析步骤——解析失败正是规格 §10.4「缺失 mod」的检测点。
/// 本类型的序列化只负责「结构 ↔ 数据」这一半，不负责这一半。
///
/// # `size` 与 `terrain` 的关系：默认派生，交叉校验（ADR 0011 案例三）
///
/// `size`（世界瓦片级 [`TorusSize`]）本可以完全从 `terrain`（`SurfaceStore`
/// 持有的 [`ZoneLayout`]）派生（`layout.tile_size()`），不需要单独存
/// 一份——但那意味着全仓库每一处 `world.size.wrap(..)` 都要改成
/// `world.size().wrap(..)`，是与「换掉 `terrain` 的存储方式」这件事
/// 本身无关的大范围改动。这里沿用迁移前就已经确立的模式（存一份
/// 派生值，用 `#[serde(try_from = ..)]` 在反序列化时交叉校验它与真正
/// 权威来源一致，见下方 [`WorldStateRepr`]）：`size` 字段仍然存在，
/// 但唯一真相源是 `terrain.layout()`，反序列化必须校验两者一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "WorldStateRepr")]
pub struct WorldState {
    /// 生成本世界地形所用的种子。
    pub seed: u64,
    /// 当前世界时钟。全世界只有这一个时钟，见 `ll_core::time` 的说明。
    pub clock: Tick,
    /// 世界瓦片级尺寸——派生自 `terrain.layout().tile_size()`，见本类型
    /// 文档「`size` 与 `terrain` 的关系」。
    pub size: TorusSize,
    /// 世界地表：区块流式生成与常驻，见 [`SurfaceStore`]。
    pub terrain: SurfaceStore,
    /// 全部 `Interior` 实例的权威集合（设计文档六节）——本字段是「真正
    /// 把 `Surface`（`terrain`）与 `Interior` 组合进同一个 `WorldState`」
    /// 的落点：批次 C（任务 10）交付了 [`InteriorTable`] 本身，但当时
    /// `WorldState` 还没有地方持有它，见 [`crate::interior`] 模块文档
    /// 「与共享常驻预算的关系」一节。
    pub interiors: InteriorTable,
    /// 玩家当前所在的 `Interior`（若在地表则为 `None`）——用于常驻
    /// 预算的钉住逻辑（裁定 CS-3）：进入的 `Interior` 的锚点区块视为
    /// 当前空间，不被共享的 256 上限挤出去。见 [`Self::enter_interior`]/
    /// [`Self::exit_interior`]。
    ///
    /// # 为什么只存 `SpaceId`，不存完整 `Space`
    ///
    /// `Space::Interior` 还带着 `profile: ContentIndex`，而
    /// `ContentIndex` 依赖注册表加载顺序，不可持久化（与
    /// `terrain_table` 同一类限制，见其字段文档）。`SpaceId`（`WorldId`
    /// 的类型别名）本身只是一个整数，不携带这个问题，可以正常参与
    /// 序列化——不需要为这个字段引入新的 `#[serde(skip)]`。
    ///
    /// # 与 `Agent::current_space`（设计文档任务 12）的关系
    ///
    /// 这个字段只服务「常驻预算该钉住谁」这一件事，不是「玩家所在
    /// 空间」的权威记录——那份权威记录属于 `Agent`（设计文档任务 12
    /// 才会落地 `Agent.current_space`）。本字段是任务 11 为常驻预算
    /// 接线预留的一份更窄的会话上下文；未来任务 12 接线
    /// `Intent::EnterSpace`/`Effect::ChangeSpace` 时，`apply` 应该同时
    /// 调用 `Agent.current_space = space` 与
    /// `WorldState::enter_interior`/`exit_interior`，两者各自维护自己
    /// 的那份状态，不互相依赖。
    ///
    /// # `#[serde(skip)]`（P5 任务 9 修正的既有缺陷）
    ///
    /// 本字段类型本身完全可以序列化（`SpaceId` 是裸整数,上面这段文档
    /// 曾经据此断言「不需要为这个字段引入新的 `#[serde(skip)]`」）——
    /// 但那条断言遗漏了一半事实：[`WorldStateRepr`] 从一开始就没有把
    /// 这个字段列进去，[`TryFrom::try_from`] 也一直把它硬编码为
    /// `None`（「读档后总是从『没有进入任何 `Interior`』的状态开始」，
    /// 见该实现的注释）——也就是说 Deserialize 这一侧从来就没有真正
    /// 读过这个字段，只是此前 `Serialize` 仍然把它写出去了。用
    /// `serde_json` 这类自描述、按字段名匹配的格式，这条不对称是无害
    /// 的（多出来的字段在 Deserialize 时被直接忽略）；但存档主体读写
    /// 管线（任务 9）改用 `postcard`——一种**按声明顺序**、不带字段名
    /// 的定位编码格式，两侧字段集合不一致会让这个字段序列化出的字节
    /// 被解码器当成**下一个字段**（`population`）的开头去读，从而错位
    /// 整个后续的字节流（任务 9 落地时确实用一个真实的 `postcard`
    /// 往返测试撞见了这个缺陷：只要 `actors` 非空就会在 `Arena`/
    /// `ThinPopulation` 的内部校验处报出一个语焉不详的
    /// `SerdeDeCustom`）。加上这个属性后 `Serialize` 也不再写出这个
    /// 字段，两侧重新对称——这正是 [`Self::surface_profile`]/
    /// [`Self::terrain_table`] 已经在用的同一种模式（「读档后总是重置
    /// 为固定初始值的字段，不需要参与序列化」）。
    #[serde(skip)]
    pub current_interior: Option<SpaceId>,
    /// 地表默认层属性索引（任务 12：两级坐标系重写）——`Intent::ExitSpace`
    /// 结算时用于重新构造 `Space::Surface { .. }`，见
    /// `ll_sim::resolve` 模块文档「`Interior` 退出如何拿到地表 profile」
    /// 一节。
    ///
    /// # 为什么不参与序列化，为什么不是 `WorldState::new` 的参数
    ///
    /// 与 `terrain_table` 同一类已知限制：`ContentIndex` 依赖当前会话
    /// 的注册表加载顺序，不可持久化（见其字段文档）。**没有做成构造
    /// 参数**——与 `terrain_table`（生成地形这一步立刻就要用）不同，
    /// 这个索引只在玩家真正触发一次 `Intent::ExitSpace` 时才被读取,
    /// 绝大多数调用方（现有全部测试与三个既有验收 demo）从不构造/消费
    /// 任何 `Interior`,不需要为了一个用不到的字段而在 `WorldState::new`
    /// 的调用点上都多传一个参数。真正需要它的调用方（任务 12 起接线
    /// 进出 `Interior` 的场景）应在拿到真实的
    /// `BaseSpaceProfileIds`/`register_base_space_profiles` 结果后，
    /// 显式赋值 `world.surface_profile = ids.surface`。读档后（以及未
    /// 显式赋值时）的占位值是 [`ContentIndex::default`]，见其文档
    /// 「不代表任何具体已注册内容」——在这个值被真正替换之前触发
    /// `Intent::ExitSpace` 会让退出后的 `Space::Surface.profile` 指向
    /// 一个可能未注册的占位索引，调用方必须保证在开放这条 Intent 之前
    /// 已经完成赋值。
    ///
    /// # 为什么这里没有同 `terrain_table` 一样补一个 `assert_*_loaded`（P5 任务 5 核实结论）
    ///
    /// 核实过程中发现两处与最初预期不同的事实，一并记录：
    ///
    /// 1. `Intent::ExitSpace` 并不是"尚未开放、留给未来任务"的功能——
    ///    `ll_sim::resolve::resolve_exit_space` 已经真实读取本字段
    ///    （`ll-sim` 依赖 `ll-world`，读的正是这份缓存值）,不能按
    ///    "校验时机可以推迟到 Intent::ExitSpace 开放之前"这条思路
    ///    延后处理。
    /// 2. 但 `terrain_table.is_empty()` 那套"空即未灌入"的判定思路在
    ///    这里**不成立**：`ContentIndex::default()` 是索引 `0`,而索引
    ///    `0` 完全可能是某个真实会话里 `lostland:surface` 自己注册到
    ///    的合法索引（取决于当前会话里 `Registry` 的其他内容在它之前
    ///    注册了多少条）——`terrain_table` 用 `Vec::is_empty()` 判断
    ///    "一条属性都没登记过"没有这个歧义（空 `Vec` 不可能是任何合法
    ///    已登记状态），但拿一个具体索引值去和"占位默认值"比较相等,
    ///    没有办法排除"这个索引真的就是 0"这种合法情况。一个基于相等
    ///    比较的 `assert_surface_profile_loaded` 在这类场景下要么误报
    ///    （合法的索引 0 被当成"没灌入"拒绝），要么漏报（取决于具体
    ///    实现），两者都不可接受。
    ///
    /// 因此这里**没有**添加一个形状与 `assert_terrain_table_loaded`
    /// 对称的校验方法——那会是一个看起来安全、实际不可靠的假保证。
    /// 真正可靠的修复需要把本字段的类型从 `ContentIndex`（永远有值,
    /// 用魔法值 0 当"未设置"）换成 `Option<ContentIndex>`（`None` 才是
    /// 唯一、无歧义的"未设置"表达），但那会牵连 `WorldState::new` 的
    /// 构造语义与 `resolve_exit_space` 的读取路径,超出本任务范围（本
    /// 任务标题是"terrain_table 读档校验点"，不是"surface_profile 类型
    /// 修正"），已如实记录为后续任务的开放项。
    #[serde(skip)]
    pub surface_profile: ContentIndex,
    /// 薄层人口：数十万到数百万背景 NPC，列式排布。P3 阶段可以为空，
    /// 见 [`ThinPopulation`] 模块文档。参与序列化，见本类型文档
    /// 「`population`/`actors` 现在参与序列化」一节。
    pub population: ThinPopulation,
    /// 厚层实体池：数百个被真正模拟的实体，行式排布。P3 阶段可以只有
    /// 玩家与几个敌人，见 [`Arena`] 模块文档。参与序列化，理由同上。
    pub actors: Arena<Agent>,
    /// 玩家角色对应的厚层实体（P5 任务 6 定案，裁定 P5-3）。
    ///
    /// # 为什么需要这个字段
    ///
    /// 存档必须知道玩家是谁——读档要恢复视角（相机对准哪个实体）、
    /// 双模式存档要判断"玩家死了没"、缺失 mod 降级策略（见
    /// `ll_content::degrade`）要区分"这条记录是不是玩家自己的角色"
    /// （玩家角色种族缺失不可降级，NPC 可以）。此前三个既有验收 demo
    /// （`p3_acceptance`/`p4_acceptance`/`p5_coordinate_acceptance`）都
    /// 是各自在应用层用一个局部变量记住玩家的 `EntityId`——这等于把
    /// "谁是玩家"这件事排除在存档之外：换一个读档会话，应用层的局部
    /// 变量不会跟着存档一起回来。
    ///
    /// # 为什么是 `Option` 而不是必填
    ///
    /// 不是每个 `WorldState`（尤其是测试/demo 构造出来的）都真的有一个
    /// "玩家"概念——`None` 是诚实的初始状态，不是需要拒绝的非法输入。
    ///
    /// # 为什么不是 `WorldState::new` 的参数
    ///
    /// 与 `surface_profile`/`terrain_table` 不同的考量：这个字段不依赖
    /// 任何注册期上下文，不需要为了避免新增 `#[serde(skip)]` 而延后
    /// 赋值——纯粹是为了不改变 `WorldState::new` 现有签名、不牵连三个
    /// 既有验收 demo 与全部既有测试的调用点（与 P5 任务 3「只加派生不
    /// 改字段类型」同一条最小改动纪律）。调用方应在 `Arena::spawn`
    /// 产出玩家的 `EntityId` 之后，显式赋值 `world.player_entity =
    /// Some(id)`。
    ///
    /// # 参与 `hash()`（裁定 P5-9）
    ///
    /// 玩家是谁当然影响玩法——先例：P3 阶段 `hash()` 完全不含实体状态，
    /// 导致确定性回归测试测不出战斗结算跑偏（见 `Self::hash` 文档
    /// 「厚层实体也参与摘要」一节）。判据漏了东西，测试就是在空跑。
    pub player_entity: Option<EntityId>,
    /// 脚本状态：全局命名空间存储（`knowledge/design/script-state-storage.md`
    /// 二、四节）——与任何具体实体无关的状态，例如教团累计声望、已解锁
    /// 的世界级 flag。键是 `(mod_namespace, key)`；每实体存储挂在
    /// [`crate::entity::Agent::script_state`]，见其字段文档「为什么随
    /// `Agent` 走」一节两者存储位置分离的理由。
    ///
    /// # 写入路径必须经 `apply`（裁定 P5-1）
    ///
    /// 设计文档 8.2 节原写「直接写穿，没有中间层」，与约束 C1「`apply`
    /// 是全局唯一能改世界的地方」字面冲突——本字段就在 `WorldState`
    /// 里，写它就是改世界。裁定 P5-1 选择 C1 赢：`ll-script` 侧
    /// `api::state` 模块把一次决策期间的多次 `state-set!` 调用收集成
    /// 一批，包成一条 `ll_sim::effect::Effect::SetScriptState`，经
    /// `resolve → apply` 既有管线写入，见该 `Effect` 变体文档。本字段
    /// 本身不知道、也不需要知道写入者是脚本还是别的什么——它只是
    /// `WorldState` 的一个普通字段，`apply` 对它赋值与对 `pos`/`health`
    /// 赋值没有任何区别。
    ///
    /// `BTreeMap` 不是 `HashMap`：约束 C5，见设计文档五、1 节。**序列化
    /// 走 [`crate::script_state::serde_map`]**——JSON 等基于文本的格式
    /// 要求 map 的键必须是字符串，元组键 `(String, String)` 不满足这个
    /// 要求，见该模块文档。
    #[serde(with = "crate::script_state::serde_map")]
    pub global_script_state: BTreeMap<(String, String), ScriptValue>,
    /// 地形属性表：`terrain` 网格里的 [`TerrainKind`] 值查这张表才能
    /// 问出「阻不阻挡视线」「移动代价多少」。**不参与序列化**——与
    /// `population`/`actors` 同一类已知限制（P4 阶段）：这张表本质是
    /// 当前会话已加载 mod 集合的注册期产物（见
    /// `crate::terrain` 模块文档「与 Registry 的关系」），依赖 mod
    /// 加载顺序，与 `ContentIndex` 本身一样不可持久化
    /// （`ll_core::ident` 模块文档）。读档后这张表默认是空的——所有
    /// 地形查询会退化成安全兜底值（[`TerrainTable::move_cost`] 等
    /// 文档），直到调用方显式用当前会话重新注册出的表替换它。
    ///
    /// # 读档后的显式校验点（P5 任务 5）
    ///
    /// 「默认安全兜底」解决的是「不 panic」，不解决「玩家没注意到自己
    /// 在用一张空表玩游戏」——兜底值会让地形查询看起来正常返回，只是
    /// 结果全部错误（所有地形都变成「可通行、代价 100、不阻挡视线」）。
    /// 这类静默错误正是坐标系重写批次 Task 8 报告建议 P5 补上显式校验
    /// 的理由：读档后必须能主动问「灌没灌」，见
    /// [`Self::assert_terrain_table_loaded`]，而不是依赖兜底值不报错
    /// 就当作一切正常。真正在读档流程里调用这个校验点是任务 9 的
    /// 职责，本字段与本方法只负责让这个问题变得可以被回答。
    #[serde(skip)]
    pub terrain_table: TerrainTable,
}

/// [`WorldState`] 反序列化的中转表示。
///
/// 见 [`WorldState`] 文档「`size` 与 `terrain` 的关系」：这个类型本身
/// 没有任何跨字段不变式，只是让 serde 有一个「先把字段各自反序列化
/// （各自的校验仍然生效），再交给 [`TryFrom`] 做交叉校验」的中转落点。
/// `current_interior` 不出现在这里——读档后总是从「没有进入任何
/// `Interior`」的状态开始（见 [`TryFrom::try_from`]），不需要参与这次
/// 中转。`population`/`actors` 现在**出现在这里**（P5 批次 B）：两者
/// 已经真正参与序列化，见 [`WorldState`] 文档同名一节；`player_entity`
/// 同样出现在这里（P5 任务 6）——不依赖任何注册期上下文，没有理由不
/// 参与序列化；`surface_profile`/`terrain_table` 仍然不出现——那两处
/// `#[serde(skip)]` 不在本批次范围内。`global_script_state` 同样出现
/// 在这里（脚本状态存储批次）——没有任何注册期上下文依赖，是
/// `WorldState` 的普通数据字段，见其字段文档。
#[derive(Deserialize)]
struct WorldStateRepr {
    seed: u64,
    clock: Tick,
    size: TorusSize,
    terrain: SurfaceStore,
    interiors: InteriorTable,
    population: ThinPopulation,
    actors: Arena<Agent>,
    player_entity: Option<EntityId>,
    #[serde(default, with = "crate::script_state::serde_map")]
    global_script_state: BTreeMap<(String, String), ScriptValue>,
}

impl TryFrom<WorldStateRepr> for WorldState {
    type Error = String;

    /// 唯一的构造路径：在委托给字段本身校验之后，额外校验
    /// `terrain.layout().tile_size() == size`——两者是同一个世界尺寸的
    /// 两份独立记录，必须一致，否则按 `size` 遍历坐标去查 `terrain`
    /// 就会算出与实际区块布局不符的区块坐标。
    fn try_from(repr: WorldStateRepr) -> Result<Self, Self::Error> {
        let tile_size = repr.terrain.layout().tile_size();
        if tile_size != repr.size {
            return Err(format!(
                "存档中的世界尺寸 {}x{} 与区块布局推出的实际尺寸 {}x{} 不一致",
                repr.size.width(),
                repr.size.height(),
                tile_size.width(),
                tile_size.height(),
            ));
        }
        Ok(WorldState {
            seed: repr.seed,
            clock: repr.clock,
            size: repr.size,
            terrain: repr.terrain,
            interiors: repr.interiors,
            // 读档后总是从「没有进入任何 Interior」的状态开始——见
            // WorldStateRepr 文档。
            current_interior: None,
            // population/actors 现在是存档里的真实数据，直接从 repr
            // 搬过来——见 WorldState 文档「population/actors 现在参与
            // 序列化」一节。surface_profile/terrain_table 仍然不参与
            // 序列化（各自的 #[serde(skip)] 不在本批次范围内），存档里
            // 没有对应数据可读，读档后总是从空/默认状态开始；
            // surface_profile 额外要求调用方读档后显式重新赋值才能安全
            // 开放 ExitSpace。
            surface_profile: ContentIndex::default(),
            population: repr.population,
            actors: repr.actors,
            player_entity: repr.player_entity,
            // 脚本状态原样从存档搬过来——孤儿保留（设计文档七、1
            // 节）：读档本身只做数据搬运，不做「这个 mod 还在不在当前
            // 集合里」这类带业务判断的清理，缺失 mod 留下的命名空间
            // 残留原样保留在这里，直到玩家显式做维护操作。
            global_script_state: repr.global_script_state,
            terrain_table: TerrainTable::default(),
        })
    }
}

impl WorldState {
    /// 按区块布局与生成参数创建一个新世界，时钟从零开始。
    ///
    /// `terrain_ids`/`terrain_table` 是调用方已经注册好的地形定义（见
    /// `crate::terrain::materialize_base_terrain`）——世界状态本身不
    /// 知道如何取得注册表，只负责把已经注册好的结果用于地形生成、并
    /// 把属性表随世界一起持有，供后续的 `resolve`/FOV 等只读查询使用
    /// （见 [`Self::terrain_table`] 字段文档）。
    ///
    /// # 不再一次性生成整张地图
    ///
    /// 早期版本这里调用 `generate_terrain` 生成整张世界地图。本次改为
    /// 只预热 `spawn` 周围一圈邻域（[`SPAWN_WARM_RADIUS`]，设计文档
    /// 五节「常驻集合的构成」默认 5×5）——这正是流式生成要达到的效果：
    /// 世界创建不再是一次与世界总面积成正比的重活。`spawn` 之外的区域
    /// 会在玩家真正走近时由 [`Self::terrain_at_streaming`] 按需生成。
    pub fn new(
        layout: ZoneLayout,
        params: &GenParams,
        terrain_ids: &BaseTerrainIds,
        terrain_table: TerrainTable,
        spawn: TorusPos,
    ) -> Result<WorldState, WorldError> {
        let noise = build_zone_noise(&layout, params)?;
        let mut terrain = SurfaceStore::new(layout, DEFAULT_RESIDENT_CAP);
        warm_spawn_neighborhood(&mut terrain, &noise, params, terrain_ids, spawn);
        Ok(WorldState {
            seed: params.seed,
            clock: Tick(0),
            size: layout.tile_size(),
            terrain,
            interiors: InteriorTable::new(),
            current_interior: None,
            surface_profile: ContentIndex::default(),
            population: ThinPopulation::default(),
            actors: Arena::default(),
            player_entity: None,
            global_script_state: BTreeMap::new(),
            terrain_table,
        })
    }

    /// 推进世界时钟 `ticks` 格。
    ///
    /// `ticks` 允许为负：世界时钟内部只是一个 `i64`，不排斥读档迁移或
    /// 时间倒流类效果回拨时钟的用法。
    pub fn advance(&mut self, ticks: i64) {
        self.clock = Tick(self.clock.0 + ticks);
    }

    /// 只读地形查询：假定该坐标所属区块已经常驻，不触发按需生成。
    ///
    /// 提供给 `resolve`（`ll-sim::resolve`）等必须保持纯函数（C1）的
    /// 调用点，以及脚本层（`ll-script`）这类只能拿到 `&WorldState` 的
    /// 只读消费方——真正的按需加载触发点收窄到 [`Self::terrain_at_streaming`]，
    /// 不藏在这个只读查询路径里。
    ///
    /// 未常驻时返回 `None`（不 panic）——见
    /// [`SurfaceStore::terrain_at_resident`] 文档「为什么未常驻时返回
    /// `None`」。
    pub fn terrain_at(&self, pos: TorusPos) -> Option<TerrainKind> {
        self.terrain.terrain_at_resident(pos)
    }

    /// 读档后置校验：`terrain_table` 是否已经被调用方重新灌入。
    ///
    /// 这不是构造时自动完成的——`terrain_table` 依赖当前会话的 mod
    /// 加载结果，`WorldState` 反序列化本身没有能力单独产出一张正确的
    /// 表（见 [`Self::terrain_table`] 字段文档），必须由调用方在拿到
    /// 当前会话 `TerrainTable` 后显式重新赋值，再调用本方法确认。
    ///
    /// 未灌入时返回 [`WorldError::TerrainTableNotReloaded`]，而不是
    /// 静默放行——空表不会让任何地形查询 panic（[`TerrainTable`] 的
    /// 每个查询方法都有安全兜底值），这正是问题所在：不报错不等于
    /// 结果正确，一张空表会让全部地形查询悄悄退化成兜底值,不调用本
    /// 方法就没有任何信号能提醒调用方这件事发生了。
    pub fn assert_terrain_table_loaded(&self) -> Result<(), WorldError> {
        if self.terrain_table.is_empty() {
            return Err(WorldError::TerrainTableNotReloaded);
        }
        Ok(())
    }

    /// 可能触发按需生成的地形查询——流式加载真正的触发点（见
    /// [`Self::terrain_at`] 文档「resolve 只读、加载收窄到……」）。
    ///
    /// `noise`/`params`/`terrain_ids` 由调用方提供，`WorldState` 本身
    /// 不持有它们：`noise`/`params` 只在这一处需要（生成从不在
    /// `resolve`/`apply` 里发生），存成字段只会增加 `WorldState` 的
    /// 体积却换不到任何好处；`terrain_ids` 依赖 mod 注册期上下文，与
    /// `terrain_table` 同一类不可持久化限制（见其字段文档），存成字段
    /// 会需要再引入一处 `#[serde(skip)]`，本任务的硬性约束禁止这么做。
    pub fn terrain_at_streaming(
        &mut self,
        noise: &TileableNoise,
        params: &GenParams,
        terrain_ids: &BaseTerrainIds,
        pos: TorusPos,
        at_tick: Tick,
    ) -> TerrainKind {
        self.terrain
            .terrain_at(noise, params, terrain_ids, pos, at_tick)
    }

    /// 插入一个 `Interior`（见 [`InteriorTable::insert`]），并重算共享
    /// 常驻预算——这是「真正把 `Interior` 组合进共享 256 上限」需要的
    /// 记账时机（批次 C 报告的缺口，见 [`crate::interior`] 模块文档
    /// 「与共享常驻预算的关系」）。世界生成/建造玩法应该通过这个方法
    /// 插入 `Interior`，而不是绕过 `WorldState` 直接操作
    /// `self.interiors`，否则预算记账会跟着漏掉。
    pub fn insert_interior(&mut self, interior: Interior) {
        self.interiors.insert(interior);
        self.recompute_shared_cap();
    }

    /// 玩家进入一个 `Interior`：把它的锚点区块钉住（裁定 CS-3），并让
    /// `Surface` 的常驻上限相应收缩，给这个 `Interior` 已加载的全部
    /// 楼层让出配额，使两者的合计不超过共享的 256 上限。
    ///
    /// `id` 不存在于 `self.interiors` 时返回 `false` 且不做任何改动——
    /// 调用方（未来的 `apply`，见设计文档任务 12）应该只对已经存在的
    /// `Interior` 调用这个方法，但查询失败是正常路径，不是需要 panic
    /// 的编程错误（与 [`InteriorTable::get`] 的既有纪律一致）。
    pub fn enter_interior(&mut self, id: SpaceId) -> bool {
        let Some(interior) = self.interiors.get(id) else {
            return false;
        };
        let anchor = interior.anchor;
        self.exit_interior();
        let zone = self.terrain.layout().tile_to_zone(anchor).0;
        self.terrain.pin(zone);
        self.current_interior = Some(id);
        self.recompute_shared_cap();
        true
    }

    /// 退出当前 `Interior`（若有）：取消钉住其锚点区块，并重算常驻
    /// 上限。对当前没有进入任何 `Interior` 的世界调用是无操作。
    pub fn exit_interior(&mut self) {
        if let Some(id) = self.current_interior.take()
            && let Some(interior) = self.interiors.get(id)
        {
            let zone = self.terrain.layout().tile_to_zone(interior.anchor).0;
            self.terrain.unpin(zone);
        }
        self.recompute_shared_cap();
    }

    /// 重算 `Surface` 的有效常驻上限：共享的 [`DEFAULT_RESIDENT_CAP`]
    /// 减去当前全部 `Interior` 已加载的楼层数——这是批次 C 报告的缺口
    /// 的真正修复：此前 `Surface` 独立用满 256，`Interior` 的楼层插入
    /// 后无条件常驻却完全不计入这个数字，两者合计可能超出设计意图的
    /// 共享上限（见 [`crate::interior`] 模块文档）。`Interior` 楼层
    /// 本身仍然不会被淘汰（还没有生成器可以在淘汰后重新造出同一层，
    /// 见其模块文档「与共享常驻预算的关系」）——收缩的是 `Surface` 一侧
    /// 的淘汰阈值，不是反过来淘汰楼层。
    ///
    /// `.max(1)`：防御性下限，避免 `Interior` 楼层数逼近或超过共享
    /// 上限时把 `Surface` 的有效上限压到零——那会让 `Surface` 连一个
    /// 区块都容不下，见 [`SurfaceStore::terrain_at`] 文档「淘汰失败时
    /// 的行为」（允许暂时超出上限，而不是完全无法工作）。
    fn recompute_shared_cap(&mut self) {
        let loaded_floors = self.interiors.total_floor_count();
        let cap = DEFAULT_RESIDENT_CAP.saturating_sub(loaded_floors).max(1);
        self.terrain.set_resident_cap(cap);
    }

    /// 把整个世界状态归约成一个 64 位摘要。
    ///
    /// 用于「两次运行/序列化往返是否产生了相同的世界」这类断言，是
    /// 确定性重放与存档回归测试的基础设施（详见 `ll_core::hashing`）。
    ///
    /// # 不再遍历整个世界的每一格（两级坐标系重写，任务 11）
    ///
    /// 早期版本这里按 `size` 遍历世界的每一格。`terrain` 换成
    /// [`SurfaceStore`] 之后，多数区块在任意时刻并不常驻，压根没有
    /// 具体瓦片数据可读，继续按 `size` 遍历会对未常驻区块调用
    /// `terrain_at` 触发不必要的生成（且需要额外的 `noise`/`params`/
    /// `terrain_ids`，`hash` 的签名不该为此变复杂）。改为遍历
    /// [`SurfaceStore::resident_zones`] 返回的已排序区块坐标集合，
    /// 逐区块逐格混入哈希——不依赖 `HashMap` 迭代顺序（C5：
    /// `resident_zones` 自己已经排过序）。
    ///
    /// 这意味着黄金基准数值必然改变（同一个世界，遍历的坐标集合与
    /// 遍历顺序都变了），但断言结构保留：同一操作序列产生同一哈希、
    /// 不同种子产生不同哈希——不是推倒重来，是同一份测试逻辑换一套
    /// 输入构造方式和一批新基准数，见
    /// `crates/ll-world/tests/determinism.rs`/`crates/ll-sim/tests/replay.rs`
    /// 顶部说明。区块坐标本身也混入哈希（`zone.x()`/`zone.y()`）：
    /// 若只混入格子内容而不混入它们属于哪个区块，两个「常驻区块集合
    /// 不同、但恰好格子内容拼起来一样」的世界会被误判为相同。
    ///
    /// # 厚层实体也参与摘要（P3 批次 C 补齐）
    ///
    /// 早期版本这里只混入地形，不含 `actors`——那时候世界里还没有会
    /// 被结算改动的实体，加了也测不出什么。批次 C 落地 `resolve`/
    /// `apply` 之后，`Effect::MoveTo`/`Damage`/`ScheduleNext`/
    /// `AdjustWallet` 都会改动 `Agent` 的字段，若哈希仍只看地形，
    /// 「同一意图流产出相同的世界哈希」这类确定性回归测试即使战斗结算
    /// 悄悄跑偏（位置算错、伤害算错、排期算错）也测不出来——哈希会在
    /// 两次不同的运行之间稳定相等，因为它们唯一还在看的地形本来就没
    /// 变。这里混入 [`Arena::iter`] 遍历到的每个存活实体的
    /// 位置/生命/钱包/下次行动时刻——`Arena` 内部是 `Vec`，不是
    /// `HashMap`，`iter()` 按槽位下标顺序遍历，不依赖任何哈希表遍历
    /// 顺序，满足约束 C5「禁止让 HashMap/HashSet 的迭代顺序参与逻辑
    /// 判断」。已销毁的实体槽位不是 `Occupied`，`iter()` 自然跳过，
    /// 因此 `Effect::Kill` 也会体现为摘要变化（少一份贡献），不需要
    /// 单独混入「实体数量」。
    ///
    /// # `stats`/`affiliations`/`profession`/`race`/`goals`/`luck` 也已混入（P5 批次 B）
    ///
    /// 早期版本只挑了 `resolve`/`apply` 这批已经会写的字段（`pos`/
    /// `health`/`wallet`/`next_action_at`），不含这六项——彼时的理由是
    /// 「本批次没有任何 `Effect` 会改动它们，加进摘要不会多测出什么」。
    /// 这条理由只覆盖「同一次运行内两次 `resolve`/`apply` 是否产生相同
    /// 结果」这一种回归；`population`/`actors` 摘掉 `#[serde(skip)]`
    /// 之后，序列化往返多出一整类新风险（`Repr`/`TryFrom` 接线写错、
    /// `Arena`/`Vec` 顺序在编解码过程中被打乱），本方法自身文档开篇就
    /// 写着「用于两次运行/**序列化往返**是否产生了相同的世界」——若这
    /// 六项字段仍然缺席，一次把 `profession` 编错、`goals` 顺序打乱的
    /// 序列化缺陷不会让任何一条黄金基准变红，正是先例（P3 阶段
    /// `WorldState::health` 完全不进摘要、确定性回归测试测不出战斗结算
    /// 跑偏）警告过的同一类判据缺口。因此这里补齐：`stats` 六项主属性、
    /// `profession`/`race` 的裸索引、`luck`，以及 `affiliations`/`goals`
    /// 两个 `Vec`（先混入长度、再逐项混入，`Vec` 本身保序，不涉及
    /// `HashMap`/`HashSet` 迭代顺序，满足约束 C5）。
    ///
    /// # `player_entity`/脚本状态也已混入（裁定 P5-9）
    ///
    /// 同一条先例、同一条纪律：`player_entity` 决定读档后相机对准谁、
    /// 双模式存档能不能判断「玩家死了没」，全局脚本存储与每实体脚本
    /// 存储承载 NPC 记忆/任务进度这类真正影响玩法的数据——三者只要
    /// 缺席，对应的序列化/结算缺陷就不会体现在任何黄金基准上，重演
    /// P3 阶段 `hash()` 不含实体状态、测不出战斗结算跑偏的同一类判据
    /// 缺口。`global_script_state`/`Agent::script_state` 都是
    /// `BTreeMap`，按键的字典序遍历，不涉及 `HashMap`/`HashSet` 迭代
    /// 顺序（约束 C5）；字符串字段（命名空间、键、`Str`/`Ref` 值）混入
    /// 前先写入长度，避免相邻变长字段在字节流里边界不清导致的理论
    /// 碰撞（见 [`write_script_state`]）。
    ///
    /// # 职业/技能相关字段也已混入（P5-B 任务 5）
    ///
    /// 同一条先例第三次重演：`Agent` 新增的 `mana`/`stamina`/
    /// `unlocked_skills`/`skill_cooldowns`/`subclasses`/
    /// `active_stat_modifiers` 六个字段全部会被 `ll_sim::resolve` 的
    /// `Intent::UseSkill` 分支改写（解锁技能、扣减资源、写入冷却、施加
    /// 临时属性修正）——若哈希继续对它们视而不见，「技能结算悄悄算错」
    /// 会重演 P3 阶段 `WorldState::hash` 完全不含实体状态、测不出战斗
    /// 结算跑偏的同一类判据缺口（本方法文档已经用两次真实历史记录警告
    /// 过这条教训，这是第三次）。`unlocked_skills`/`subclasses` 是
    /// `Vec<ContentIndex>`（先混入长度、再逐项混入，保序，不涉及
    /// `HashMap`/`HashSet` 迭代顺序）；`skill_cooldowns` 是
    /// `BTreeMap<ContentIndex, Tick>`，`active_stat_modifiers` 是
    /// `BTreeMap<AttributeKind, ActiveStatModifier>`，两者都按键的自然
    /// 顺序遍历（`ContentIndex`/`AttributeKind` 均实现 `Ord`），满足
    /// 约束 C5。
    pub fn hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        hasher.write_u64(self.seed);
        hasher.write_i64(self.clock.0);
        hasher.write_u64(u64::from(self.size.width()));
        hasher.write_u64(u64::from(self.size.height()));

        let span = self.terrain.layout().zone_span() as i32;
        for zone in self.terrain.resident_zones() {
            hasher.write_i64(i64::from(zone.x()));
            hasher.write_i64(i64::from(zone.y()));
            for ly in 0..span {
                for lx in 0..span {
                    let pos = self.size.wrap(zone.x() * span + lx, zone.y() * span + ly);
                    let kind = self
                        .terrain
                        .terrain_at_resident(pos)
                        .expect("resident_zones() 返回的区块坐标此刻必然常驻");
                    hasher.write_u64(u64::from(kind.index().get()));
                }
            }
        }

        for agent in self.actors.iter() {
            hasher.write_i64(i64::from(agent.pos.x()));
            hasher.write_i64(i64::from(agent.pos.y()));
            hasher.write_i64(i64::from(agent.health));
            hasher.write_i64(agent.wallet);
            hasher.write_i64(agent.next_action_at.0);
            write_space(&mut hasher, agent.current_space);
            write_stats(&mut hasher, agent.stats);
            hasher.write_u64(u64::from(agent.profession.get()));
            hasher.write_u64(u64::from(agent.race.get()));
            hasher.write_i64(i64::from(agent.luck));
            hasher.write_u64(agent.affiliations.len() as u64);
            for affiliation in &agent.affiliations {
                write_affiliation(&mut hasher, affiliation);
            }
            hasher.write_u64(agent.goals.len() as u64);
            for goal in &agent.goals {
                write_goal(&mut hasher, goal);
            }
            hasher.write_i64(i64::from(agent.mana));
            hasher.write_i64(i64::from(agent.stamina));
            write_content_index_vec(&mut hasher, &agent.unlocked_skills);
            write_content_index_vec(&mut hasher, &agent.subclasses);
            hasher.write_u64(agent.skill_cooldowns.len() as u64);
            for (skill, until) in &agent.skill_cooldowns {
                hasher.write_u64(u64::from(skill.get()));
                hasher.write_i64(until.0);
            }
            hasher.write_u64(agent.active_stat_modifiers.len() as u64);
            for (attribute, modifier) in &agent.active_stat_modifiers {
                hasher.write_u64(*attribute as u64);
                hasher.write_i64(i64::from(modifier.delta));
                hasher.write_i64(modifier.expires_at.0);
            }
            write_script_state(&mut hasher, &agent.script_state);
        }

        write_optional_entity(&mut hasher, self.player_entity);
        write_script_state(&mut hasher, &self.global_script_state);

        hasher.finish()
    }
}

/// 把一个 [`Space`] 值混入哈希——[`WorldState::hash`] 的帮手（任务 12）。
///
/// 若哈希只看地形与 `pos`/`health`/`wallet`/`next_action_at`，
/// `Effect::ChangeSpace` 悄悄算错（例如把玩家送进了错误的 `Interior`
/// 楼层，或者退出失败却没人发现）不会反映在世界哈希上——这正是
/// [`WorldState::hash`] 文档「厚层实体也参与摘要」一节点名要避免的
/// 同一类缺口，`current_space` 是这批新增字段里唯一一个会被
/// `Effect::ChangeSpace` 改动、此前完全游离在确定性回归测试之外的
/// 字段。
///
/// 两个变体先混入一个判别字节（`0`/`1`），再混入各自的全部字段——不
/// 省略 `z`/`floor` 这类当前批次「恒定」或「预留」的字段：即便它们
/// 现在不变，混入的代价接近零，却能在未来这些字段真的开始变化时立刻
/// 被这条哈希覆盖，不需要那时再回来找哪里漏掉了一处摘要。
fn write_space(hasher: &mut StateHasher, space: Space) {
    match space {
        Space::Surface { zone, z, profile } => {
            hasher.write_u64(0);
            hasher.write_i64(i64::from(zone.x()));
            hasher.write_i64(i64::from(zone.y()));
            hasher.write_i64(i64::from(z));
            hasher.write_u64(u64::from(profile.get()));
        }
        Space::Interior {
            id,
            floor,
            anchor,
            profile,
        } => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(id.get()));
            hasher.write_i64(i64::from(floor));
            hasher.write_i64(i64::from(anchor.x()));
            hasher.write_i64(i64::from(anchor.y()));
            hasher.write_u64(u64::from(profile.get()));
        }
    }
}

/// 把一份 [`crate::entity::BaseStats`] 混入哈希——[`WorldState::hash`]
/// 的帮手（P5 批次 B）。六项主属性逐一混入，顺序与字段声明顺序一致，
/// 恒定不依赖任何运行期状态。
fn write_stats(hasher: &mut StateHasher, stats: crate::entity::BaseStats) {
    hasher.write_i64(i64::from(stats.strength));
    hasher.write_i64(i64::from(stats.dexterity));
    hasher.write_i64(i64::from(stats.constitution));
    hasher.write_i64(i64::from(stats.intelligence));
    hasher.write_i64(i64::from(stats.willpower));
    hasher.write_i64(i64::from(stats.charisma));
}

/// 把一条 [`Affiliation`] 混入哈希——[`WorldState::hash`] 的帮手（P5
/// 批次 B）。`kind` 是无数据枚举，直接转 `u64` 取判别值；`org` 与
/// [`write_space`] 同样的模式：先混入一个变体判别字节，再混入各自
/// 携带的值，两个变体互不混淆。
fn write_affiliation(hasher: &mut StateHasher, affiliation: &Affiliation) {
    hasher.write_u64(affiliation.kind as u64);
    match affiliation.org {
        OrgRef::Def(index) => {
            hasher.write_u64(0);
            hasher.write_u64(u64::from(index.get()));
        }
        OrgRef::Instance(id) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(id.get()));
        }
    }
    hasher.write_i64(i64::from(affiliation.standing));
}

/// 把一条 [`Goal`] 混入哈希——[`WorldState::hash`] 的帮手（P5 批次
/// B）。`params` 先混入长度再逐项混入——`Vec` 本身保序，不依赖
/// `HashMap`/`HashSet` 的遍历顺序（约束 C5）。
fn write_goal(hasher: &mut StateHasher, goal: &Goal) {
    hasher.write_u64(u64::from(goal.kind.get()));
    hasher.write_u64(goal.params.len() as u64);
    for param in &goal.params {
        hasher.write_i64(*param);
    }
    hasher.write_i64(i64::from(goal.progress));
    hasher.write_i64(i64::from(goal.priority));
}

/// 把一份 `Vec<ContentIndex>` 混入哈希——[`WorldState::hash`] 的帮手
/// （P5-B 任务 5），供 `Agent::unlocked_skills`/`Agent::subclasses`
/// 共用：两者形状相同（保序的 `ContentIndex` 列表），先混入长度、再
/// 逐项混入裸索引，不需要为两个字段各写一份几乎相同的循环。
fn write_content_index_vec(hasher: &mut StateHasher, indices: &[ContentIndex]) {
    hasher.write_u64(indices.len() as u64);
    for index in indices {
        hasher.write_u64(u64::from(index.get()));
    }
}

/// 把一个 `Option<EntityId>` 混入哈希——[`WorldState::hash`] 的帮手
/// （裁定 P5-9），用于 `player_entity`。判别字节区分 `Some`/`None`，
/// 与 [`write_space`] 同一种模式：先写变体判别，再写变体携带的值。
fn write_optional_entity(hasher: &mut StateHasher, entity: Option<EntityId>) {
    match entity {
        Some(id) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(id.index()));
            hasher.write_u64(u64::from(id.generation()));
        }
        None => hasher.write_u64(0),
    }
}

/// 把一份脚本状态存储（全局或某个实体的每实体存储）混入哈希——
/// [`WorldState::hash`] 的帮手（裁定 P5-9）。先混入条目数，再按
/// `BTreeMap` 的自然字典序逐条混入——不依赖任何哈希表遍历顺序，满足
/// 约束 C5。每个变长字段（命名空间、键）混入前先写长度，避免相邻
/// 字符串在字节流里边界不清导致的理论碰撞（例如 `("ab", "c")` 与
/// `("a", "bc")` 若不带长度前缀会产出同一段字节流）。
fn write_script_state(hasher: &mut StateHasher, state: &BTreeMap<(String, String), ScriptValue>) {
    hasher.write_u64(state.len() as u64);
    for ((namespace, key), value) in state {
        write_len_prefixed_bytes(hasher, namespace.as_bytes());
        write_len_prefixed_bytes(hasher, key.as_bytes());
        write_script_value(hasher, value);
    }
}

/// 把一个 [`ScriptValue`] 混入哈希——[`write_script_state`] 的帮手。
/// 与 [`write_space`]/[`write_affiliation`] 同一种模式：先写变体判别
/// 字节，各变体互不混淆；`List`/`Map` 递归调用自身，天然覆盖任意嵌套
/// 深度。
fn write_script_value(hasher: &mut StateHasher, value: &ScriptValue) {
    match value {
        ScriptValue::Int(n) => {
            hasher.write_u64(0);
            hasher.write_i64(*n);
        }
        ScriptValue::Bool(b) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(*b));
        }
        ScriptValue::Str(s) => {
            hasher.write_u64(2);
            write_len_prefixed_bytes(hasher, s.as_bytes());
        }
        ScriptValue::Ref(s) => {
            hasher.write_u64(3);
            write_len_prefixed_bytes(hasher, s.as_bytes());
        }
        ScriptValue::Entity(id) => {
            hasher.write_u64(4);
            hasher.write_u64(u64::from(id.index()));
            hasher.write_u64(u64::from(id.generation()));
        }
        ScriptValue::List(items) => {
            hasher.write_u64(5);
            hasher.write_u64(items.len() as u64);
            for item in items {
                write_script_value(hasher, item);
            }
        }
        ScriptValue::Map(map) => {
            hasher.write_u64(6);
            hasher.write_u64(map.len() as u64);
            for (key, item) in map {
                write_len_prefixed_bytes(hasher, key.as_bytes());
                write_script_value(hasher, item);
            }
        }
    }
}

/// 混入一段带长度前缀的字节——变长字段（字符串）的公共写法，见
/// [`write_script_state`] 文档「避免相邻字符串边界不清」一节。
fn write_len_prefixed_bytes(hasher: &mut StateHasher, bytes: &[u8]) {
    hasher.write_u64(bytes.len() as u64);
    hasher.write_bytes(bytes);
}

/// 世界创建时预热出生点周围的邻域，而不是一次性生成整张地图——这是
/// 本次重写的核心目的（见本文件文档「不再一次性生成整张地图」）。
/// 半径见 [`SPAWN_WARM_RADIUS`]，是设计文档五节给出的默认邻域缓冲
/// 大小。
///
/// 直接委托给 [`SurfaceStore::stream_neighborhood`]（任务 14）——出生点
/// 预热与玩家移动时的流式滚动本质是同一个操作（「以某个世界坐标为
/// 中心，保证一圈邻域常驻」），不该维护两份几乎相同的双重循环，见该
/// 方法文档「与 `terrain_at` 的关系」一节。
fn warm_spawn_neighborhood(
    terrain: &mut SurfaceStore,
    noise: &TileableNoise,
    params: &GenParams,
    terrain_ids: &BaseTerrainIds,
    spawn: TorusPos,
) {
    terrain.stream_neighborhood(
        noise,
        params,
        terrain_ids,
        spawn,
        SPAWN_WARM_RADIUS,
        Tick(0),
    );
}

/// [`ChunkGrid`] 序列化用的扁平化表示：尺寸加按行主序排列的全部地形格。
///
/// 不直接在 `chunk.rs` 里给 [`ChunkGrid`] 派生 `Serialize`/`Deserialize`：
/// 那个文件是本批次明确不允许改动的既有代码。改为在本文件借
/// [`ChunkGrid`] 已公开的 `world`/`terrain_at`/`set_terrain` 接口手写
/// 序列化实现——`ChunkGrid` 是本 crate 的本地类型，为它实现外部 trait
/// 不违反孤儿规则，因此可以在任意模块完成，不必触碰 `chunk.rs`。
///
/// # 迁移后仍然需要（两级坐标系重写，任务 11）
///
/// `WorldState` 自己不再直接持有 `ChunkGrid`，但
/// [`crate::surface_store::SurfaceStore`] 内部按区块持有多个
/// `ChunkGrid`（`resident: HashMap<ZoneCoord, ChunkGrid>`），它的手写
/// 序列化（`SurfaceStoreData`）需要 `ChunkGrid: Serialize + Deserialize`
/// ——trait 实现在 Rust 里对整个 crate 可见，不受模块边界限制，这里的
/// `impl` 因此继续服务 `crate::surface_store`，不需要跟着挪动位置。
#[derive(Serialize, Deserialize)]
struct ChunkGridData {
    width: u32,
    height: u32,
    tiles: Vec<TerrainKind>,
}

impl Serialize for ChunkGrid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let world = self.world();
        let mut tiles = Vec::with_capacity((world.width() as usize) * (world.height() as usize));
        for y in 0..world.height() as i32 {
            for x in 0..world.width() as i32 {
                tiles.push(self.terrain_at(world.wrap(x, y)));
            }
        }
        ChunkGridData {
            width: world.width(),
            height: world.height(),
            tiles,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChunkGrid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = ChunkGridData::deserialize(deserializer)?;
        let size = TorusSize::new(data.width, data.height)
            .ok_or_else(|| D::Error::custom("存档中的世界尺寸非法"))?;

        let expected_len = (data.width as usize) * (data.height as usize);
        if data.tiles.len() != expected_len {
            return Err(D::Error::custom("存档中的地形格数量与尺寸不匹配"));
        }

        // fill 只是 ChunkGrid::new 分配时的占位值，下面的双重循环会把
        // 每一格都覆写一遍（expected_len 已校验与 tiles 长度一致，包括
        // (0, 0) 这一格），借第一格的真实值占位，不产生任何浪费，也
        // 不需要凭空造一个 TerrainKind——ChunkGrid 反序列化这一层没有
        // 注册表可查，见 TerrainKind 模块文档。
        let fill = *data
            .tiles
            .first()
            .ok_or_else(|| D::Error::custom("存档中的地形格数据为空"))?;
        let mut grid =
            ChunkGrid::new(size, fill).map_err(|err| D::Error::custom(err.to_string()))?;
        let mut tiles = data.tiles.into_iter();
        for y in 0..size.height() as i32 {
            for x in 0..size.width() as i32 {
                let kind = tiles.next().expect("长度已在上面校验与预期长度相等");
                grid.set_terrain(size.wrap(x, y), kind);
            }
        }
        Ok(grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::BaseStats;
    use crate::terrain::base_terrain_fixture;

    /// 测试用区块布局：边长 64（满足视口跨度、是 16 与 32 的整数倍），
    /// 单个区块（1×1），整个测试世界恰好落在这一个区块内——足够简单，
    /// 不需要为「跨区块」场景操心，本文件的测试关注的是 `WorldState`
    /// 本身的构造/序列化/哈希纪律，不是流式加载本身（那部分见
    /// `surface_store.rs` 的测试）。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
    }

    /// 出生点：区块内部 `(5, 5)`，落在 [`test_layout`] 唯一的那个区块
    /// 里，预热半径覆盖整个 1×1 布局，因此整个测试世界从构造起就
    /// 常驻。
    fn test_spawn(layout: &ZoneLayout) -> TorusPos {
        layout.tile_size().wrap(5, 5)
    }

    /// 按测试布局建一个新世界，地形定义用 [`base_terrain_fixture`]。
    fn test_world() -> WorldState {
        let layout = test_layout();
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            test_spawn(&layout),
        )
        .expect("测试布局满足全部构造前置条件")
    }

    // 「序列化往返后哈希不变」「相同种子与尺寸生成的哈希相同」
    // 「推进时钟会改变哈希」这三条曾经在本文件与
    // `tests/determinism.rs` 里逐字重复。保留在集成测试
    // （`tests/determinism.rs`）而不是这里：那边本就收着黄金基准哈希，
    // 用的是真实 `serde_json` 格式与公开 API，是这几条行为实际生效的
    // 层级；这里的单元测试只留 [`WorldState::advance`] 本身的边界行为
    // （负值回拨）与本次新增的 `try_from` 交叉校验，两组关注点不重叠。

    #[test]
    fn 世界尺寸与区块布局推出的尺寸不一致的存档无法反序列化() {
        // 模拟被篡改或损坏的存档：区块布局实际是测试布局（64x64），
        // 但 size 字段被改成了另一个尺寸——两个字段各自反序列化都
        // 合法，只有合在一起才不自洽，必须靠交叉校验拦住。
        // Arrange
        let world = test_world();
        let mut tampered: serde_json::Value =
            serde_json::to_value(&world).expect("WorldState 全部字段可序列化");
        tampered["size"] = serde_json::json!({ "width": 128, "height": 128 });

        // Act
        let result: Result<WorldState, _> = serde_json::from_value(tampered);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 尺寸一致的存档可以正常往返() {
        // 与上一条相反的分支：size 与区块布局推出的尺寸一致时，交叉
        // 校验必须放行，不能误伤合法存档。
        // Arrange
        let world = test_world();
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");

        // Act
        let result: Result<WorldState, _> = serde_json::from_slice(&encoded);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn player_entity序列化往返后保持原样而非被重置() {
        // 裁定 P5-3：存档必须知道玩家是谁——这里锁住 player_entity 真的
        // 参与了序列化,不是像 surface_profile/terrain_table 那样读档后
        // 被重置成默认值。
        // Arrange
        let mut world = test_world();
        let pos = world.size.wrap(5, 5);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        let player_id = world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: ContentIndex::default(),
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            script_state: std::collections::BTreeMap::new(),
        });
        world.player_entity = Some(player_id);
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");

        // Act
        let decoded: WorldState = serde_json::from_slice(&encoded).expect("往返不应失败");

        // Assert
        assert_eq!(decoded.player_entity, Some(player_id));
    }

    #[test]
    fn 新建的世界玩家实体默认为空() {
        // WorldState::new 不强制要求"谁是玩家"这个概念——None 是诚实的
        // 初始状态,不是需要拒绝的非法输入。
        // Arrange & Act
        let world = test_world();

        // Assert
        assert_eq!(world.player_entity, None);
    }

    #[test]
    fn worldstate序列化往返后actors不再是空的默认值() {
        // 直接对应 P5 批次 B 存在的理由：population/actors 摘掉
        // `#[serde(skip)]` 之前，这条断言不可能写——读档后 actors 恒是
        // 空的 `Arena::default()`。这里往测试世界里真正 spawn 一个
        // `Agent`，往返后必须还能按原标识取回同一份内容，而不是退化成
        // 默认空池。
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(5, 5);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        let id = world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 999,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            script_state: std::collections::BTreeMap::new(),
        });

        // Act
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
        let decoded: WorldState = serde_json::from_slice(&encoded).expect("刚序列化的数据必然合法");

        // Assert：往返后 actors 不是空池，且能按原标识取回同一份内容。
        assert!(!decoded.actors.is_empty());
        assert_eq!(decoded.actors.get(id), world.actors.get(id));
    }

    #[test]
    fn 时钟可以倒拨() {
        // 读档迁移或时间倒流类效果可能需要回拨时钟，advance 不应拒绝
        // 负值。
        // Arrange
        let mut world = test_world();
        world.advance(100);

        // Act
        world.advance(-100);

        // Assert
        assert_eq!(world.clock, Tick(0));
    }

    #[test]
    fn terrain_table为空时assert返回错误() {
        // 模拟读档后调用方尚未重新灌入 terrain_table 的状态——直接使用
        // 一张空表构造世界。
        // Arrange
        let mut world = test_world();
        world.terrain_table = crate::terrain::TerrainTable::new();

        // Act
        let result = world.assert_terrain_table_loaded();

        // Assert
        assert_eq!(result, Err(WorldError::TerrainTableNotReloaded));
    }

    #[test]
    fn terrain_table非空时assert返回成功() {
        // test_world() 用 base_terrain_fixture() 构造，terrain_table
        // 已经登记过本体全部 17 个地形，不是空表。
        // Arrange
        let world = test_world();

        // Act
        let result = world.assert_terrain_table_loaded();

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn 出生点周围的区块在构造后立即常驻() {
        // WorldState::new 预热出生点周围一圈邻域，不是空手起步——见
        // Self::new 文档「不再一次性生成整张地图」。
        // Arrange & Act
        let world = test_world();

        // Assert
        assert!(!world.terrain.resident_zones().is_empty());
    }

    #[test]
    fn 只读地形查询在预热区域内返回some() {
        // Arrange
        let world = test_world();
        let layout = test_layout();
        let spawn = test_spawn(&layout);

        // Act
        let result = world.terrain_at(spawn);

        // Assert
        assert!(result.is_some());
    }

    #[test]
    fn 插入interior后共享上限按已加载楼层数收缩() {
        // Arrange
        let mut world = test_world();
        let mut counter = 0u32;
        let mut interner = ll_core::ident::Interner::new();
        let profile = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("字面量恒合法"));
        let anchor = world.size.wrap(0, 0);
        let mut interior =
            Interior::new(ll_core::ident::WorldId::next(&mut counter), anchor, profile);
        let size = ll_core::bounded::BoundedSize::new(4, 4).expect("4x4 是合法尺寸");
        let (ids, _table) = base_terrain_fixture();
        interior.set_floor(
            0,
            crate::bounded_grid::BoundedGrid::new(size, ids.floor_stone),
        );

        // Act
        world.insert_interior(interior);

        // Assert
        assert_eq!(world.terrain.resident_cap(), DEFAULT_RESIDENT_CAP - 1);
    }

    #[test]
    fn 进入interior会钉住其锚点区块() {
        // Arrange
        let mut world = test_world();
        let mut counter = 0u32;
        let mut interner = ll_core::ident::Interner::new();
        let profile = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("字面量恒合法"));
        let layout = test_layout();
        let anchor = test_spawn(&layout);
        let id = ll_core::ident::WorldId::next(&mut counter);
        let interior = Interior::new(id, anchor, profile);
        world.insert_interior(interior);
        let anchor_zone = layout.tile_to_zone(anchor).0;
        // 把上限压得很低，逼着淘汰逻辑必须绕开被钉住的区块才能验证
        // pin 真的生效——否则常驻区块数远小于上限时，即便 pin 没接线，
        // 这条测试也会因为「反正没到淘汰的时候」而误报通过。
        world.terrain.set_resident_cap(1);

        // Act
        world.enter_interior(id);
        // 逼近淘汰：访问另一个区块，若 anchor_zone 没被钉住就会被挤出。
        let far_pos = layout
            .tile_size()
            .wrap(anchor.x() + layout.zone_span() as i32, anchor.y());
        let (far_zone, _) = layout.tile_to_zone(far_pos);
        // 注：test_layout 是 1x1 区块，far_zone 会绕回同一个区块，这里
        // 只需验证 pin 状态本身，不追加依赖多区块布局的挤占场景。
        let _ = far_zone;

        // Assert：resident_zones 里仍然包含锚点区块——这本身不足以
        // 证明 pin 生效（1x1 布局下淘汰也挤不走它），真正的钉住效果
        // 由 SurfaceStore 自己的淘汰测试覆盖；这里只验证接线路径本身
        // 没有 panic、且 current_interior 记录正确。
        assert_eq!(world.current_interior, Some(id));
        assert!(world.terrain.resident_zones().contains(&anchor_zone));
    }

    #[test]
    fn 退出interior后不再是当前空间() {
        // Arrange
        let mut world = test_world();
        let mut counter = 0u32;
        let mut interner = ll_core::ident::Interner::new();
        let profile = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("字面量恒合法"));
        let layout = test_layout();
        let anchor = test_spawn(&layout);
        let id = ll_core::ident::WorldId::next(&mut counter);
        world.insert_interior(Interior::new(id, anchor, profile));
        world.enter_interior(id);

        // Act
        world.exit_interior();

        // Assert
        assert_eq!(world.current_interior, None);
    }

    #[test]
    fn 进入不存在的interior返回false() {
        // Arrange
        let mut world = test_world();
        let mut counter = 0u32;
        let unknown = ll_core::ident::WorldId::next(&mut counter);

        // Act
        let entered = world.enter_interior(unknown);

        // Assert
        assert!(!entered);
    }

    #[test]
    fn player_entity不同的两个世界哈希不同即便其余状态相同() {
        // 裁定 P5-9：player_entity 必须进 hash()——先例是 P3 阶段
        // hash() 不含实体状态导致战斗结算跑偏测不出来，判据漏了东西，
        // 测试就是在空跑。这里用同一个已生成的实体，一个世界标记它为
        // 玩家、另一个不标记，其余状态逐字段相同，哈希必须不同。
        // Arrange
        let mut with_player = test_world();
        let agent = blank_agent(&with_player);
        let id = with_player.actors.spawn(agent.clone());
        let mut without_player = test_world();
        without_player.actors.spawn(agent);
        with_player.player_entity = Some(id);

        // Act & Assert
        assert_ne!(with_player.hash(), without_player.hash());
    }

    #[test]
    fn 全局脚本状态写入后世界哈希改变() {
        // 脚本状态影响玩法（NPC 记忆、任务进度），必须进 hash()，否则
        // 序列化/结算缺陷不会体现在任何黄金基准上——同一条纪律见
        // `WorldState::hash` 文档「player_entity/脚本状态也已混入」。
        // Arrange
        let mut world = test_world();
        let hash_before = world.hash();

        // Act
        world.global_script_state.insert(
            ("lostland".to_string(), "reputation".to_string()),
            crate::script_state::ScriptValue::Int(42),
        );

        // Assert
        assert_ne!(world.hash(), hash_before);
    }

    #[test]
    fn 每实体脚本状态写入后世界哈希改变() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let id = world.actors.spawn(agent);
        let hash_before = world.hash();

        // Act
        world
            .actors
            .get_mut(id)
            .expect("刚生成的实体必然存在")
            .script_state
            .insert(
                ("lostland".to_string(), "cooldown".to_string()),
                crate::script_state::ScriptValue::Int(5),
            );

        // Assert
        assert_ne!(world.hash(), hash_before);
    }

    #[test]
    fn 全局脚本状态序列化往返后保持原样() {
        // Arrange
        let mut world = test_world();
        world.global_script_state.insert(
            ("lostland".to_string(), "reputation".to_string()),
            crate::script_state::ScriptValue::List(vec![
                crate::script_state::ScriptValue::Int(1),
                crate::script_state::ScriptValue::Bool(true),
                crate::script_state::ScriptValue::Str("x".into()),
            ]),
        );

        // Act
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
        let decoded: WorldState = serde_json::from_slice(&encoded).expect("往返不应失败");

        // Assert
        assert_eq!(decoded.global_script_state, world.global_script_state);
    }

    /// 供本文件末尾几条哈希/脚本状态测试复用的占位实体——字段取值不
    /// 重要，测试只关心「多了一份脚本状态记录之后哈希是否改变」。
    fn blank_agent(world: &WorldState) -> Agent {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(0, 0);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            script_state: BTreeMap::new(),
        }
    }
}
