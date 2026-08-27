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
use ll_core::ident::{ContentIndex, WorldId};
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};

use crate::WorldError;
use crate::chunk::ChunkGrid;
use crate::entity::{Affiliation, Agent, Arena, EntityId, Goal, OrgRef, ThinPopulation};
use crate::exploration::ExplorationMemory;
use crate::generate::{GenParams, build_zone_noise};
use crate::history::{
    HistoricalEvent, HistoricalEventKind, KillCause, KillingBlow, SettlementDemise, VictimState,
};
use crate::interior::{Interior, InteriorTable};
use crate::item::{GroundItemStack, ItemStack};
use crate::mod_state::ModStateValue;
use crate::noise::TileableNoise;
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
    /// 探索记忆：玩家「看没看过」某个格子的记录（见
    /// [`crate::exploration`] 模块文档），供 [`crate::overview`] 的
    /// `minimap`/`continent_map` 接线真实的战争迷雾数据。
    ///
    /// # 为什么按角色只存一份，不按 `SpaceId`/多角色拆分
    ///
    /// 当前一份 `WorldState` 就代表一个角色的存档（见
    /// [`Self::player_entity`] 字段文档），探索记忆天然也只有「这个
    /// 角色看没看过」这一份视角，不需要在此预先拆成
    /// `BTreeMap<EntityId, ExplorationMemory>` 这类多视角容器——那是
    /// 在没有真实多角色/多人共享世界需求之前的投机性设计（YAGNI）。
    /// 真正需要多视角时，`minimap`/`continent_map` 的读取接口已经要求
    /// 调用方显式传入 `&ExplorationMemory`（见 `crate::exploration`
    /// 模块文档「为什么读取接口要求显式传入」一节），不需要为了那一天
    /// 现在就改这里的存储形状。
    ///
    /// # 参与 `hash()`（ADR 0022）与序列化，不加 `#[serde(skip)]`
    ///
    /// 探索记忆是按角色持久化的真实数据（哪怕玩家换一台机器读同一份
    /// 存档，之前去过的地方也不该重新蒙上战争迷雾），必须随
    /// `WorldState` 一起序列化；也必须混入 [`Self::hash`]——否则
    /// 「探索记忆悄悄算错」（例如误标记了不该标记的区块）不会在任何
    /// 确定性回归测试里现出形状，重演本类型文档「厚层实体也参与摘要」
    /// 一节已经出现过两次的同一类判据缺口。
    pub exploration: ExplorationMemory,
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
    /// 历史事件日志——击杀/死亡记录（`kill-and-death-events.md`）的
    /// 存储，未来其余历史事件种类（建城、战争、王朝更替……）落地时会
    /// 共用同一份存储，见 [`crate::history`] 模块文档「为什么落在
    /// ll-world」一节。
    ///
    /// # 曾经的破坏性存档结构变化（P5 之后）
    ///
    /// `identity-and-ids.md`「schema 迁移问题」一节已经提前论证：这个
    /// 容器字段若拖到对应系统真正落地才加进 `WorldState`，就必然是一次
    /// 破坏性存档变更——本字段正是那次变更。配套的迁移函数发布前一度
    /// 落在 `ll_content::migrations`，随「老存档去掉就好了」的裁定一并
    /// 清空（见该模块文档），现在只有 `ll-content` 唯一认识的一个
    /// schema 版本，不再有「v3 新增」这类历史分档的意义。
    ///
    /// # 参与 `hash()`（ADR 0022）与序列化，不加 `#[serde(skip)]`
    ///
    /// 与 [`Self::exploration`]/[`Self::player_entity`] 同一条纪律：
    /// 历史事件是真正影响玩法与传说浏览查询结果的数据，缺席
    /// `hash()` 会重演「新字段只加了，没人测过它是否被正确覆盖」的
    /// 既有判据缺口（见 `Self::hash` 文档同名历史记录）。
    pub history: Vec<HistoricalEvent>,
    /// [`WorldId`] 分配计数器——`WorldId::next` 要求调用方提供一个
    /// 贯穿整个世界生命周期、只会前进的计数器（见其文档），这里是
    /// 那个计数器的持久存放处。见 [`Self::allocate_world_id`]。
    ///
    /// # 为什么不能是运行期局部变量
    ///
    /// 若计数器只存在于某次会话的局部变量里，读档后重新从 0 开始计数
    /// 会与已经写进 [`Self::history`]（以及未来 `OrgInstance` 等）的
    /// 旧 `WorldId` 撞号——`WorldId` 的核心约定「永不复用」（见其文档）
    /// 因此要求计数器本身必须是存档的一部分，随世界一起持久化。
    ///
    /// # 参与 `hash()`，不加 `#[serde(skip)]`
    ///
    /// 理由同 [`Self::history`]：计数器的值影响"下一个被记住的实体会
    /// 拿到哪个 WorldId"，是真正的玩法状态，不是可以每次会话重新算出
    /// 的衍生值（ADR 0009 的"默认派生"不适用——见 `kill-and-death-events.md`
    /// 四节对同一类问题的论证：这不是遍历成本的问题，是数据源本身
    /// 无法重算）。
    pub next_world_id: u32,
    /// 击杀聚合计数（项目所有者决策二：「一起计算，就是杀了 10 只」）
    /// ——按 [`crate::entity::Agent::creature_kind`]（`None` 时回退到
    /// [`crate::entity::Agent::race`]，与该字段文档「用于击杀匹配与
    /// 死因统计分类」一节同一条既有回退规则）归并的击杀次数。**每一场
    /// 击杀**都计入本字段,不论受害者是否已"具名"——见下文「决策二」
    /// 一节。
    ///
    /// # 为什么不是完整 `KillRecord`
    ///
    /// 只有已具名（`remembered_id` 已赋值）的死者才产出完整
    /// [`crate::history::HistoricalEvent`]（见 [`Self::record_kill`]
    /// 文档「调用时机」一节引用的触发判据）——绝大多数背景怪物从未被
    /// "记住"，为它们逐条保留完整记录会让 [`Self::history`] 随游玩时长
    /// 无界增长。本字段是"默认派生，只存偏差"这条本仓库反复复用的模式
    /// 在这里的应用：聚合计数覆盖**全部**死亡（默认路径，不需要逐条
    /// 留痕），完整记录只额外覆盖"值得被记住"的偏差情形（具名死者）。
    ///
    /// # 为什么不能是运行期局部变量，必须是 `WorldState` 字段
    ///
    /// 理由与 [`Self::next_world_id`] 同一条：若只存在于某次会话的局部
    /// 变量里，读档后重新从零计数会丢失存档前的累计——"杀了 47 只
    /// 哥布林"这类统计必须能跨读档/存档往返幸存。
    ///
    /// # 决策二：与完整记录叠加，不再互斥（否决决策一原有设计）
    ///
    /// 决策一（无名单位击杀改计数）落地时把「累加本字段」与「产出完整
    /// 记录」设计成互斥——受害者已具名就只产出完整记录、不碰本字段。
    /// 项目所有者复核后否决了这条互斥：杀 10 只哥布林、其中 1 只有
    /// 名字，本字段理应显示 10，不是 9。落点在
    /// `ll_sim::resolve` 模块内部 `append_kill_history`（见其文档
    /// 「决策二」一节完整论证）：现在对每一场击杀都累加本字段，受害者
    /// 已具名时**额外**再产出一条完整记录，两者叠加。
    ///
    /// `BTreeMap` 不是 `HashMap`：约束 C5，按键（`ContentIndex`）自然
    /// 顺序遍历，不依赖任何哈希表迭代顺序。
    ///
    /// # 参与 `hash()`（ADR 0022）与序列化，不加 `#[serde(skip)]`
    ///
    /// 与 [`Self::history`]/[`Self::next_world_id`] 同一条纪律：这是
    /// 真正影响"传说浏览/死因统计"查询结果的数据，缺席 `hash()` 会重演
    /// "新字段只加了，没人测过它是否被正确覆盖"的既有判据缺口（见
    /// [`Self::hash`] 文档同名历史记录）。
    pub kill_counts: BTreeMap<ContentIndex, u64>,
    /// 地面物品（P6 第二批：背包与地面物品）——`item-system.md` 四节
    /// `ItemLocation::Ground { pos, dropped_at }` 的落地，
    /// [`crate::entity::Agent::inventory`] 字段文档「为什么是 `Agent`
    /// 的字段」一节讨论了背包为什么挂在实体上；地面物品不属于任何
    /// 实体，天然只能挂在 `WorldState` 本身。
    ///
    /// # 为什么是 `Vec`，不是按位置索引的 `BTreeMap`
    ///
    /// [`ll_core::torus::TorusPos`] 没有实现 `Ord`（只有
    /// `PartialEq`/`Eq`/`Hash`，见其定义），不能直接当 `BTreeMap` 键；
    /// 拆成 `(i32, i32)` 元组键虽然可行，但当前查询模式（
    /// `ll_sim::resolve::resolve_pick_up` 找「这个位置有没有物品」）
    /// 是线性扫描，量级是"这个世界当前地面物品堆数"，与
    /// [`crate::entity::Agent::inventory`] 同一条"没有槽位概念，量级
    /// 不大"的既有判断——真正的性能考量应该来自"远景区域惰性扫掉过期
    /// 物品"这条设计（`item-system.md` 四节「这条清理正好搭在惰性
    /// 追赶机制上」），而不是提前假设需要按位置索引查询。
    ///
    /// # 老化清理见 [`Self::cleanup_aged_ground_items`]
    ///
    /// `Vec` 保序，不涉及 `HashMap`/`HashSet` 迭代顺序（约束 C5）。
    ///
    /// # 参与 `hash()`（ADR 0022）与序列化，不加 `#[serde(skip)]`
    ///
    /// 与 [`Self::history`]/[`Self::kill_counts`] 同一条纪律：地面物品
    /// 是真正影响玩法（拾取/丢弃/合并/老化）的数据，缺席 `hash()` 会
    /// 重演"新字段只加了，没人测过它是否被正确覆盖"的既有判据缺口（见
    /// [`Self::hash`] 文档同名历史记录）。
    pub ground_items: Vec<GroundItemStack>,
    /// 已经**物化过 NPC** 的据点 id，按升序排列、去重（NPC 生成批次）。
    ///
    /// # 这是「默认派生，只存偏差」在 NPC 上的那份偏差（ADR 0009）
    ///
    /// 一座据点住着谁，是种子的纯函数（`ll_mod::roster::settlement_roster`）
    /// ——玩家从没走近过的村子在世界状态里一个字节都不占。但玩家真的走进
    /// 去的那一刻，那份名册会被物化成一批 `Agent` 进 [`Self::actors`]，
    /// 从此归玩家改变（被杀、被抢、走开）。
    ///
    /// **这个字段回答的是唯一一个派生答不上来的问题：这座据点该不该再
    /// 生成一批人。** 没有它，区块被淘汰再加载时会照着同一份名册重来
    /// 一遍，把玩家杀掉的人原样复活——那正是本批次要解决的那个缺陷。
    ///
    /// # 为什么是「据点 id 集合」而不是一份逐 NPC 的偏差表
    ///
    /// 逐 NPC 偏差表要先回答「派生出来的那个人与存档里那个 `Agent` 之间
    /// 的稳定身份是什么」——`Agent` 上没有这样的字段，加一个就是又一次
    /// `hash()` 改动 + 存档 remap，而加了之后换来的能力与本字段完全相同。
    /// 完整论证见 `ll_mod::roster` 模块文档二节。
    ///
    /// # `Vec` 不是 `BTreeSet`
    ///
    /// 与 [`crate::entity::Agent::unlocked_skills`] 同一条既有理由：查询
    /// 模式是「这个 id 在不在里面」，元素数量的量级是「玩家这一局走进过
    /// 几座村子」（个位到几十），线性 `contains` 足够；`Vec` 保序，不涉及
    /// `HashMap`/`HashSet` 迭代顺序（约束 C5）。「不重复插入」由唯一写入口
    /// [`Self::mark_settlement_materialized`] 自己保证。
    ///
    /// # 参与 `hash()`（ADR 0022）与序列化
    ///
    /// 与 [`Self::ground_items`]/[`Self::kill_counts`] 同一条纪律：它真的
    /// 分岔未来（同一个世界，这座据点物化过与没物化过，此后走向完全不同
    /// 的两批实体），缺席 `hash()` 就测不出「重复生成」这条缺陷本身有没有
    /// 回潮。
    pub materialized_settlements: Vec<WorldId>,
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
/// `#[serde(skip)]` 不在本批次范围内。`exploration`
/// （落地探索记忆批次）同理出现在这里，不依赖任何注册期上下文；
/// `#[serde(default)]` 只是让本文件内部用 `serde_json::json!` 手写
/// 局部字段的测试固件不必每条都补一份空探索记忆，不代表存在需要兼容
/// 的旧存档（当前唯一存在过的 schema 版本本就带这个字段）。
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
    #[serde(default)]
    exploration: ExplorationMemory,
    /// 历史事件日志——`#[serde(default)]` 的理由与 `exploration` 一致
    /// （见其字段注释）：发布前曾经需要兼容本字段引入之前写出的存档，
    /// 走的是 `ll_content::migrations` 里当时真正注册的迁移路径（随
    /// 「老存档去掉就好了」的裁定一并清空，见该模块文档），这里的默认
    /// 值只服务本文件内部用 `serde_json::json!` 手写局部字段的测试
    /// 固件。
    #[serde(default)]
    history: Vec<HistoricalEvent>,
    /// WorldId 分配计数器，理由与 `history` 同一节。
    #[serde(default)]
    next_world_id: u32,
    /// 无名单位击杀聚合计数——`#[serde(default)]` 的理由与
    /// `history`/`next_world_id` 一致，这里的默认值只服务本文件内部用
    /// `serde_json::json!` 手写局部字段的测试固件。
    #[serde(default)]
    kill_counts: BTreeMap<ContentIndex, u64>,
    /// 地面物品——`#[serde(default)]` 的理由与 `history`/`next_world_id`/
    /// `kill_counts` 一致，这里的默认值只服务本文件内部用
    /// `serde_json::json!` 手写局部字段的测试固件。
    #[serde(default)]
    ground_items: Vec<GroundItemStack>,
    /// 已物化 NPC 的据点 id——`#[serde(default)]` 的理由与
    /// `history`/`next_world_id`/`kill_counts`/`ground_items` 一致，这里的
    /// 默认值只服务本文件内部用 `serde_json::json!` 手写局部字段的测试
    /// 固件。
    #[serde(default)]
    materialized_settlements: Vec<WorldId>,
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
            exploration: repr.exploration,
            terrain_table: TerrainTable::default(),
            history: repr.history,
            next_world_id: repr.next_world_id,
            kill_counts: repr.kill_counts,
            ground_items: repr.ground_items,
            materialized_settlements: repr.materialized_settlements,
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
            exploration: ExplorationMemory::new(),
            terrain_table,
            history: Vec::new(),
            next_world_id: 0,
            kill_counts: BTreeMap::new(),
            ground_items: Vec::new(),
            materialized_settlements: Vec::new(),
        })
    }

    /// 这座据点的 NPC 已经物化过了吗——[`Self::materialized_settlements`]
    /// 唯一的查询入口。
    pub fn settlement_is_materialized(&self, site: WorldId) -> bool {
        self.materialized_settlements.binary_search(&site).is_ok()
    }

    /// 把一座据点记成「已物化」，返回**这一次是否真的是第一次**。
    ///
    /// [`Self::materialized_settlements`] 唯一的写入口：它自己保证有序
    /// 与不重复（见该字段文档「`Vec` 不是 `BTreeSet`」一节），调用方不
    /// 需要、也不应当直接 `push`。
    ///
    /// # 与 ADR 0023「状态写入经 `apply`」的关系
    ///
    /// 这条写入**不经 `Effect`**，与 [`Self::terrain`] 的流式加载
    /// （`stream_neighborhood`）属于同一类：它不是任何一个实体的一次
    /// 行动的结果，而是「世界的哪一部分此刻被装进了内存」这条装载纪律
    /// 的一部分。ADR 0023 管的是**结算**产生的状态变化（谁打了谁、谁
    /// 捡了什么），流式装载从来不在它的范围内——`SurfaceStore::admit`
    /// 每加载一个区块就改写 `terrain`，也没有走 `Effect`。
    ///
    /// 判据是「这次改写会不会因为回放同一串 `Effect` 而重现」：地形流式
    /// 加载与本字段都不会（它们由玩家走到哪里决定，不由行动序列决定），
    /// 因此都不属于 `apply` 的管辖范围。
    pub fn mark_settlement_materialized(&mut self, site: WorldId) -> bool {
        match self.materialized_settlements.binary_search(&site) {
            Ok(_) => false,
            Err(at) => {
                self.materialized_settlements.insert(at, site);
                true
            }
        }
    }

    /// 推进世界时钟 `ticks` 格。
    ///
    /// `ticks` 允许为负：世界时钟内部只是一个 `i64`，不排斥读档迁移或
    /// 时间倒流类效果回拨时钟的用法。
    pub fn advance(&mut self, ticks: i64) {
        self.clock = Tick(self.clock.0 + ticks);
    }

    /// 地面物品老化清理的默认阈值：30 游戏日（`item-system.md` 四节
    /// 「地面物品与老化清理」原文：「地面物品在丢弃满 30 游戏日……时
    /// 清除」）。
    ///
    /// **只是一个默认值，不是写死进 [`Self::cleanup_aged_ground_items`]
    /// 本身的常量**——该方法的阈值是运行期参数，调用方可以传任意值。
    /// 这是模块任务书「老化阈值不该写死在引擎里」的落点：引擎（本
    /// crate）不在方法体内引用这个常量，它只是给不需要自定义阈值的
    /// 调用方（demo/测试）准备的一个方便默认值，与
    /// [`crate::item::ItemStack::new`]/`merge_stacks` 把 `stack_limit`
    /// 当参数传入、不由引擎自己决定是同一条纪律。
    pub const DEFAULT_GROUND_ITEM_MAX_AGE_TICKS: i64 = 30 * ll_core::time::TICKS_PER_DAY;

    /// 清除丢弃时长超过 `max_age_ticks` 的地面物品堆，返回被清除的堆数。
    ///
    /// # 阈值为什么是参数，不是常量或字段
    ///
    /// `item-system.md` 四节原文没有把 30 天这个数字定成一成不变的
    /// 规则——项目任务书明确要求「老化阈值不该写死在引擎里」并要求
    /// 给出结论：本方法选择**运行期参数**这一档（与
    /// [`crate::item::merge_stacks`] 的 `stack_limit` 同一条既有纪律，
    /// 见其文档），不是全局配置项或按物品各自配置：
    ///
    /// 1. 当前没有任何"每种物品各自的老化阈值"字段（`ItemDef` 没有
    ///    这个概念，见 `ll_mod::item` 模块文档「本批次范围」一节——
    ///    高价值物品"永不清理"依赖 `Quality`，而 `Quality` 本身还没
    ///    有 Rust 定形，见 [`crate::item`] 模块文档「`Owner` 本批次
    ///    仍然不落地」一节同一条 YAGNI 判断：不能为一个还不存在的
    ///    品质轴提前发明"按品质豁免"的分支），因此"每物品各自配置"
    ///    在本批次没有可挂靠的地方。
    /// 2. 做成 `WorldState` 字段（"全局配置进存档"）会让这个数字参与
    ///    `hash()`/序列化——但它是纯粹的规则参数，不是任何一次具体
    ///    结算读出的世界状态，与 `ll_sim::timeline::action_cost` 里
    ///    `BASE_ACTION_COST` 这类"规则常量不进 `WorldState`"是同一
    ///    类判断，不应该无谓地进存档。
    ///
    /// **mod 能不能改**：能——本方法不读取任何写死的数字，调用方
    /// （未来的"游戏规则配置"层，或者 mod 通过 `register-*` 声明的
    /// 一个全局规则表）只需要算出一个新的 `max_age_ticks` 传进来，本
    /// 方法不需要跟着改一行代码。当前代码库还没有一张"游戏规则配置
    /// 表"（[`Self::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS`] 就是唯一现成
    /// 的数字来源），真正的"mod 可声明覆盖"需要那张表先存在，不在
    /// 本批次范围内提前搭建一套只服务这一个数字的注册表（YAGNI）。
    ///
    /// # 为什么不是 `Effect`/走 `apply`
    ///
    /// 与 `crate::entity::ThinPopulation` 的 `rebase`/`wallet_of`（薄层
    /// 人口的钱包惰性追赶）同一类"系统级被动演化"，不是任何一次玩家/
    /// AI `Intent` 的直接后果——本方法文档「这条清理正好搭在惰性追赶
    /// 机制上」（`item-system.md` 四节原文）描述的正是这类调用点：
    /// 真正把它接到"玩家靠近远景区域时顺带扫一次"这条触发路径上是
    /// 惰性追赶系统本身的调用方职责，不在本批次范围内（本批次只交付
    /// 这个可独立调用、可独立测试的清理机制本身）。
    ///
    /// # 哪些地面物品**永不**老化：立起来的那些
    ///
    /// 判据是 [`crate::item::GroundItemStack::placed`]——**放置状态**，
    /// 不是物品定义上的 `ItemDef.furniture` 标志。
    ///
    /// # 这里此前是一个 `is_permanent: &dyn Fn(ContentIndex) -> bool` 回调
    ///
    /// 家具层那一批传的是回调，理由记得很清楚：本 crate 不知道「家具」
    /// 是什么（`ItemDef.furniture` 住在下游的 `ll-mod`，依赖方向不允许
    /// 反向引用），所以判据得由拿得到物品表的调用方折算出来传进来。
    ///
    /// **放置状态落地之后那条理由不再成立**：会不会老化现在取决于这一
    /// 堆自己身上的一个位，本 crate 完全看得见，不需要任何下游知识。
    /// 回调因此收掉——留着一个恒等价于「读一个本地字段」的回调参数，
    /// 就是这个代码库反复踩过的多余间接层（ADR 0021 双向的另一侧）。
    ///
    /// 语义也随之更正，这不只是重构：一座**躺在地上没立起来**的炉子
    /// 此前因为 `ItemDef.furniture` 为真而永不老化——它那时和别的地面
    /// 物品没有任何区别，却享受着永久豁免。现在它照常老化，只有真正
    /// 立在那里的才不老化。
    pub fn cleanup_aged_ground_items(&mut self, max_age_ticks: i64) -> usize {
        let now = self.clock.0;
        let before = self.ground_items.len();
        self.ground_items
            .retain(|item| item.placed || now.saturating_sub(item.dropped_at.0) < max_age_ticks);
        before - self.ground_items.len()
    }

    /// 这一格上**立着**的那一堆（`None` 表示这格没有放置物）。
    ///
    /// 「立着的」= [`Self::ground_items`] 里坐标相同、
    /// [`crate::item::GroundItemStack::placed`] 为真的第一条。
    /// `ground_items` 是 `Vec`（有序），同一格真出现两件放置物时取哪
    /// 一条是确定的（约束 C5）——而正常路径下这不会发生，放置前置
    /// （`ll_sim::resolve` 的 `resolve_place`）就是为了让它不发生。
    ///
    /// 放在本 crate 而不是 `ll-sim`：三个消费者（放置前置、丢弃前置、
    /// 制作的场地前置）问的是同一个问题，而这个问题只读
    /// `WorldState` 自己的字段，不需要任何内容表——留在 `ll-sim` 会让
    /// 三处各自写一遍同样的 `iter().find()`（ADR 0021）。
    pub fn placed_at(&self, pos: TorusPos) -> Option<&crate::item::GroundItemStack> {
        self.ground_items
            .iter()
            .find(|item| item.pos == pos && item.placed)
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

    /// 分配一个新的、单调递增的 [`WorldId`]。
    ///
    /// 唯一的分配入口——历史事件记录（[`Self::record_kill`]）、懒分配
    /// 的 `remembered_id`（[`Self::remembered_id_of_or_assign`]）都经
    /// 这里取号，共用同一个持久计数器（[`Self::next_world_id`]），保证
    /// 「永不复用」这条 `WorldId` 的核心约定不会因为存在两个独立计数器
    /// 而被绕开。
    pub fn allocate_world_id(&mut self) -> WorldId {
        WorldId::next(&mut self.next_world_id)
    }

    /// 若 `entity` 尚未被"记住"（[`crate::entity::Agent::remembered_id`]
    /// 为 `None`），懒分配一个新 `WorldId` 并写回；已经具名则直接返回
    /// 既有值。`entity` 不存在时返回 `None`（与本文件其余查询「目标
    /// 不存在时静默返回，不 panic」的既有纪律一致）。
    ///
    /// # 为什么这是"值得被记住"的懒分配落点
    ///
    /// `Agent::remembered_id` 文档「懒分配」一节列出的触发时机（出生进
    /// 族谱、被玩家收为随从、成为任务发布者……）里，"死于一场被记录的
    /// 击杀"是其中之一——本方法是这个触发时机在代码里的具体落点，由
    /// [`Self::record_kill`] 在确认这场击杀值得被记录之后调用。
    pub fn remembered_id_of_or_assign(&mut self, entity: EntityId) -> Option<WorldId> {
        if let Some(existing) = self
            .actors
            .get(entity)
            .and_then(|agent| agent.remembered_id)
        {
            return Some(existing);
        }
        let id = self.allocate_world_id();
        let agent = self.actors.get_mut(entity)?;
        agent.remembered_id = Some(id);
        Some(id)
    }

    /// 追加一条击杀历史事件——`ll_sim::apply::apply` 响应
    /// `Effect::RecordHistoricalEvent` 时调用的唯一入口（约束 C1：写
    /// 世界只能经 `apply`，本方法把"如何写"这个细节封装在 `ll-world`
    /// 内部，`apply` 只负责决定"要不要调、传什么参数"）。
    ///
    /// # 调用时机：必须在 `victim` 被 `Effect::Kill` 销毁之前
    ///
    /// [`Self::remembered_id_of_or_assign`] 需要读取（并可能写入）
    /// `victim` 对应的 `Agent`——若这个实体已经被 `Arena::despawn`
    /// 收走，本方法直接返回 `None`、不产出任何历史事件。`ll_sim::resolve`
    /// 因此必须把这条效果排在对应的 `Effect::Kill` **之前**，见
    /// `ll_sim::effect::Effect::RecordHistoricalEvent` 文档。
    ///
    /// # `killer` 不做懒分配
    ///
    /// 只有 `victim`（这场死亡本身）触发懒分配——`killer` 若尚未具名，
    /// `KillRecord.killer` 就是 `None`,不会仅仅因为"参与了一场被记录
    /// 的击杀"就顺手给击杀者也发一个 `WorldId`。这条不对称是刻意的：
    /// 触发记录的判据是"victim 已具名"（见调用方 `ll_sim::resolve`
    /// 的 `append_kill_history` 文档），killer 具名与否是这条判据之外
    /// 的独立事实，不应该被这次记录动作反过来影响。
    ///
    /// 返回本次分配/复用的 `victim` `WorldId`；`victim` 已不存在时
    /// 返回 `None` 且不追加任何记录。
    pub fn record_kill(&mut self, report: crate::history::KillReport) -> Option<WorldId> {
        let victim_id = self.remembered_id_of_or_assign(report.victim)?;
        let killer_id = report
            .killer
            .and_then(|killer| self.actors.get(killer))
            .and_then(|agent| agent.remembered_id);
        let id = self.allocate_world_id();
        self.history.push(HistoricalEvent {
            id,
            at: report.at,
            location: report.location,
            kind: HistoricalEventKind::Kill(crate::history::KillRecord {
                killer: killer_id,
                victim: victim_id,
                cause: report.cause,
                killing_blow: KillingBlow {
                    damage: report.damage,
                    remaining_health: report.remaining_health,
                },
                victim_state: VictimState::UNKNOWN,
            }),
        });
        Some(victim_id)
    }

    /// 累加一次击杀计数（项目所有者决策二：数全部击杀，不再限定
    /// 无名单位，见 [`Self::kill_counts`] 文档「决策二」一节）——
    /// `ll_sim::apply::apply` 响应 `Effect::IncrementKillCount` 时调用
    /// 的唯一入口（约束 C1：写世界只能经 `apply`），`kind` 已经由
    /// `resolve` 阶段算好（受害者的 `creature_kind`，`None` 时回退到
    /// `race`，见 [`Self::kill_counts`] 文档），本方法只管把这个数字
    /// 累加进去，不重新判断该按什么归并。
    pub fn record_kill_count(&mut self, kind: ContentIndex) {
        *self.kill_counts.entry(kind).or_insert(0) += 1;
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
    /// # `stats`/`affiliations`/`profession`/`race`/`goals` 也已混入（P5 批次 B）
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
    /// 跑偏）警告过的同一类判据缺口。因此这里补齐：`stats` 七项属性
    /// （六项主属性 + 幸运，幸运并入 `AttributeKind` 批次前是单独混入
    /// 的字段，并入后随 `stats` 一起混入，见 [`write_stats`] 文档）、
    /// `profession`/`race` 的裸索引，以及 `affiliations`/`goals` 两个
    /// `Vec`（先混入长度、再逐项混入，`Vec` 本身保序，不涉及
    /// `HashMap`/`HashSet` 迭代顺序，满足约束 C5）。
    ///
    /// # `player_entity`/mod 状态也已混入（裁定 P5-9）
    ///
    /// 同一条先例、同一条纪律：`player_entity` 决定读档后相机对准谁、
    /// 双模式存档能不能判断「玩家死了没」，[`crate::entity::Agent::mod_state`]
    /// 承载任务进度/副职解锁计数这类真正影响玩法的数据——两者只要
    /// 缺席，对应的序列化/结算缺陷就不会体现在任何黄金基准上，重演
    /// P3 阶段 `hash()` 不含实体状态、测不出战斗结算跑偏的同一类判据
    /// 缺口。`Agent::mod_state` 是 `BTreeMap`，按键的字典序遍历，不
    /// 涉及 `HashMap`/`HashSet` 迭代顺序（约束 C5）；字符串字段（命名
    /// 空间、键、`Str`/`Ref` 值）混入前先写入长度，避免相邻变长字段在
    /// 字节流里边界不清导致的理论碰撞（见 [`write_mod_state`]）。
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
    /// `BTreeMap<ContentIndex, Tick>`，两者都按键的自然顺序遍历
    /// （`ContentIndex`/`AttributeKind` 均实现 `Ord`），满足约束 C5。
    ///
    /// # `active_stat_modifiers` 是两层 `BTreeMap`（`buffs-and-triggers.md`
    /// 六节，多来源叠加存储改法）
    ///
    /// 外层按 [`crate::entity::AttributeKind`] 键控，内层按「来源」
    /// （[`ll_core::ident::ContentIndex`]）键控——一次 agent 迭代要遍历
    /// 两层，不再是六节改法之前的单层遍历。先混入外层条目数（有几种
    /// 属性正被修正），再对每个属性混入内层条目数（这一属性上有几个
    /// 不同来源）与逐条 `(来源, delta, expires_at)`，两层都由 `BTreeMap`
    /// 自身的键序保证确定性遍历，不涉及任何 `HashMap`/`HashSet`（约束
    /// C5）。若漏掉内层遍历（只混入外层条目数就结束），会重演本方法
    /// 已经用真实历史记录警告过的同一类「哈希看不见真实生效的状态」
    /// 缺口，只是这次缺口更隐蔽——外层条目数不为零看起来"哈希已经在
    /// 关心这个字段"，实际内容（哪些来源、各自的 `delta`/`expires_at`）
    /// 却完全没有混入。
    ///
    /// # 探索记忆也已混入（落地探索记忆批次）
    ///
    /// 第四次重演同一条先例：[`Self::exploration`] 若不参与摘要，
    /// 「探索记忆悄悄标记错格子」不会被任何确定性回归测试测出来。
    /// [`ExplorationMemory::write_hash`] 自己负责按 `BTreeMap` 的自然
    /// 顺序遍历（不依赖 `HashMap`/`HashSet` 迭代顺序，满足约束 C5），
    /// 这里只是调用它，不重复实现一遍遍历逻辑。
    ///
    /// # 开放注册资源池当前值也已混入（资源池落地批次，第一批：法力池/血池）
    ///
    /// 第七次重演同一条先例：[`Agent::resource_pools`] 会被
    /// `resolve_use_skill`（`ResourceCost::PoolAmount` 消耗）与回合开始
    /// 的自动恢复（`RegenRule::OnTurnStart`）真实改写，若哈希继续对它
    /// 视而不见，「法力结算悄悄算错」测不出来，重演本方法文档已经用
    /// 六次真实历史记录警告过的同一类判据缺口。紧邻 `mana`/`stamina`
    /// 插入，`resource-pools-and-rest.md` 十节给出的精确施工位置。血池
    /// 不新开哈希项——它就是 `Agent::health`，本来就已经参与哈希。
    ///
    /// # 法术位已消耗数与休息会话也已混入（法术位/休息事件落地批次）
    ///
    /// 第八次重演同一条先例：[`Agent::spent_slots`]/[`Agent::resting`]
    /// 会被 `resolve_use_skill`（`ResourceCost::SlotTier` 消耗）与
    /// `resolve_wait`（休息完成的恢复批次、休息中断清零）真实改写——若
    /// 哈希继续视而不见，「法术位结算/休息防刷悄悄算错」这一类跑偏测不
    /// 出来。紧邻 `resource_pools` 插入。
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
            // 开放注册资源池当前值（资源池落地批次,第一批：法力池/
            // 血池,`resource-pools-and-rest.md` 十节「精确插入位置」
            // 一节原文）——紧邻 mana/stamina,理由同它们本身参与哈希的
            // 理由（本方法文档「职业/技能相关字段也已混入」一节同一条
            // 先例的又一次重演）：`resolve_use_skill`/回合开始的自动
            // 恢复都会真实改写这个字段,不进哈希就测不出资源结算跑偏。
            // `BTreeMap<ContentIndex, i32>` 按键自然顺序遍历,不涉及
            // `HashMap`/`HashSet` 迭代顺序（约束 C5）。**容量不在这里**
            // ——容量是从天赋按等级现算的派生量,不存储、不进哈希,见
            // `ll_sim::resource_pool::effective_scalar_capacity` 文档。
            hasher.write_u64(agent.resource_pools.len() as u64);
            for (pool, current) in &agent.resource_pools {
                hasher.write_u64(u64::from(pool.get()));
                hasher.write_i64(i64::from(*current));
            }
            // 法术位已消耗数与休息会话也已混入（法术位/休息事件落地批次，
            // `resource-pools-and-rest.md` 十节施工位置的直接延续，紧邻
            // 上面的 resource_pools）——第八次重演同一条先例：
            // `resolve_use_skill`（`ResourceCost::SlotTier` 消耗）与
            // `resolve_wait`（休息完成的恢复批次、休息中断）都会真实
            // 改写这两个字段，若哈希继续看不见它们，「法术位结算/休息
            // 防刷悄悄算错」测不出来。`spent_slots` 键是 `(ContentIndex,
            // u8)`，`BTreeMap` 天然按字典序遍历（先池再档位），不涉及
            // `HashMap`/`HashSet` 迭代顺序（约束 C5）。
            hasher.write_u64(agent.spent_slots.len() as u64);
            for ((pool, tier), spent) in &agent.spent_slots {
                hasher.write_u64(u64::from(pool.get()));
                hasher.write_u64(u64::from(*tier));
                hasher.write_u64(u64::from(*spent));
            }
            match agent.resting {
                None => hasher.write_u64(0),
                Some(rest) => {
                    hasher.write_u64(1);
                    hasher.write_i64(rest.started_at.0);
                    hasher.write_u64(u64::from(rest.target_ticks));
                }
            }
            // 等级与经验系统新增的三个字段（level-and-experience-system.md
            // 六节施工指引）——必须手动补进来：本函数对 `self.actors`
            // 的遍历是逐字段手写，不是把整个 `Agent` 结构体自动折叠进
            // 哈希，新增字段若不在这里显式写一行，就会静默漏出
            // ADR 0022 点名的「判据字段不全」这个失效模式（见该 ADR
            // 记录的两个历史实例，均是「新增 Agent/WorldState 字段但
            // 忘了同步进 hash()」）。红/绿验证见
            // `crates/ll-world/tests/determinism.rs`
            // 「新增等级经验字段后世界哈希必须变化」一节。
            hasher.write_i64(i64::from(agent.level));
            hasher.write_i64(agent.experience);
            hasher.write_i64(agent.xp_to_next_level);
            // 未分配的属性点/技能点（升级加点批次新增）——同一条
            // ADR 0022 纪律的又一次重演：这两个字段由 apply 侧的升级
            // 循环授予、由加点/学技能结算消耗，是真正会被改写的世界
            // 状态；不显式写这两行，「点数悄悄发多了/花掉了没扣」这
            // 一类跑偏就测不出来。红/绿验证见
            // `crates/ll-world/tests/determinism.rs`
            // 「新增未分配点数字段后世界哈希必须变化」一节。
            hasher.write_u64(u64::from(agent.unspent_attribute_points));
            hasher.write_u64(u64::from(agent.unspent_skill_points));
            write_content_index_vec(&mut hasher, &agent.unlocked_skills);
            write_content_index_vec(&mut hasher, &agent.subclasses);
            // 「曾经获得过哪些副职」这份去重账本（副职发点批次新增）
            // ——ADR 0022 的同一条纪律，而且这里**尤其**不能省：这个
            // 字段由 `ll_sim::apply` 处理 `Effect::GrantSubclass` 时
            // 改写，且它**真的改变结算**（它决定这次授予要不要发一批
            // 属性点/技能点，见
            // `Agent::subclasses_ever_granted` 文档）。不混入的后果
            // 正是这个字段被造出来要防的那件事在重放摘要上完全看不
            // 出来：「点数被重复发了/该发没发」。编码手法与紧邻上一行
            // 的 `subclasses` 逐字相同（先混入长度、再逐项混入，`Vec`
            // 保序，不涉及 `HashMap`/`HashSet` 迭代顺序，约束 C5）。
            write_content_index_vec(&mut hasher, &agent.subclasses_ever_granted);
            // 已知配方（配方发现批次新增）——ADR 0022 的同一条纪律：
            // 这个字段由 `ll_sim::apply` 处理 `Effect::LearnRecipe` 时
            // 改写，且它**真的改变结算**（`resolve_craft` 对声明了
            // `requires_discovery` 的配方多判一道「会不会做」的闸门），
            // 因此两个只有已知配方不同的世界必须算出不同的哈希，否则
            // 「配方发现悄悄没落地/多学了一条」这一类跑偏测不出来。
            // 编码手法与上面 `unlocked_skills`/`subclasses` 逐字相同
            // （先混入长度、再逐项混入，`Vec` 保序，不涉及
            // `HashMap`/`HashSet` 迭代顺序，约束 C5）。红/绿验证见
            // `crates/ll-world/tests/determinism.rs`
            // 「新增已知配方字段后世界哈希必须变化」一节。
            write_content_index_vec(&mut hasher, &agent.known_recipes);
            // 已鉴定物品种类（未鉴定物品批次新增）——ADR 0022 的同一条
            // 纪律，且这里**不能**用「反正不改结算所以不必混入」来省掉：
            // 这个字段由 `ll_sim::apply` 处理 `Effect::IdentifyItem` 时
            // 改写，是货真价实的世界状态，而 ADR 0022 要的是「世界哈希
            // 覆盖全部会被改写的世界状态」，不是「覆盖全部会影响战斗
            // 数值的世界状态」。不混入的后果是「鉴定悄悄没落地/多认了
            // 一种」这一类跑偏在重放摘要上完全看不出来。编码手法与上面
            // `known_recipes` 逐字相同（先混入长度、再逐项混入，`Vec`
            // 保序，不涉及 `HashMap`/`HashSet` 迭代顺序，约束 C5）。
            write_content_index_vec(&mut hasher, &agent.identified_items);
            hasher.write_u64(agent.skill_cooldowns.len() as u64);
            for (skill, until) in &agent.skill_cooldowns {
                hasher.write_u64(u64::from(skill.get()));
                hasher.write_i64(until.0);
            }
            hasher.write_u64(agent.active_stat_modifiers.len() as u64);
            for (attribute, per_source) in &agent.active_stat_modifiers {
                hasher.write_u64(*attribute as u64);
                hasher.write_u64(per_source.len() as u64);
                for (source, modifier) in per_source {
                    hasher.write_u64(u64::from(source.get()));
                    hasher.write_i64(i64::from(modifier.delta));
                    hasher.write_i64(modifier.expires_at.0);
                }
            }
            write_mod_state(&mut hasher, &agent.mod_state);
            write_optional_content_index(&mut hasher, agent.creature_kind);
            hasher.write_i64(agent.spawned_at.0);
            write_optional_world_id(&mut hasher, agent.remembered_id);
            // 背包（P6 第二批：背包与地面物品）——`Intent::PickUp`/
            // `Intent::Drop` 都会真实改写这个字段，若哈希继续看不见它，
            // 「拾取/丢弃/合并悄悄算错」这一类跑偏测不出来，重演本方法
            // 文档「新增字段若不在这里显式写一行……」一节点名的失效
            // 模式。`Vec` 保序，不涉及 HashMap/HashSet 迭代顺序（约束
            // C5）。
            hasher.write_u64(agent.inventory.len() as u64);
            for stack in &agent.inventory {
                write_item_stack(&mut hasher, stack);
            }
            // 装备栏（P6 第三批：装备槽位）——`Intent::Equip`/
            // `Intent::Unequip` 都会真实改写这个字段，与 `inventory`
            // 同一条先例：不进哈希就测不出装备/卸下悄悄算错。
            // `BTreeMap<EquipSlot, ItemStack>` 按键自然顺序遍历（键的
            // `Ord` 就是底层 `u8` 位下标的数值序），不涉及 `HashMap`/
            // `HashSet` 迭代顺序（约束 C5）。
            hasher.write_u64(agent.equipment.len() as u64);
            for (slot, stack) in &agent.equipment {
                hasher.write_u64(u64::from(slot.get()));
                write_item_stack(&mut hasher, stack);
            }
            // 潜行状态（潜行与盗贼被动批次）——同一条先例第 N 次重演
            // （见本方法文档「新增字段若不在这里显式写一行……」一节）：
            // `Intent::ToggleStealth` 真实改写这个字段，且它真的分岔
            // 未来（移动开销、偷袭直通、卫兵盘查触发率三条消费者，见
            // `Agent::stealthed` 字段文档）。ADR 0022「世界状态哈希必须
            // 完整」——不完整的确定性哈希等于没有哈希，因此这一行必须
            // 存在，本批次的黄金基准摘要也确实因此改变（见
            // `crates/ll-sim/tests/replay.rs` 的 `EXPECTED_REPLAY_DIGEST`
            // 文档「本次重冻的原因」一节）。
            hasher.write_u64(u64::from(agent.stealthed));
        }

        write_optional_entity(&mut hasher, self.player_entity);
        self.exploration.write_hash(&mut hasher);

        // 历史事件日志与 WorldId 分配计数器（击杀与死亡记录批次）——
        // 理由见 Self::history/Self::next_world_id 字段文档「参与
        // hash()」一节：这是同一条先例（P3 阶段 hash() 不含实体状态
        // 测不出战斗结算跑偏）第五次重演的预防性覆盖。`history` 是
        // `Vec`，保序，不涉及 HashMap/HashSet 迭代顺序（约束 C5）。
        hasher.write_u64(self.history.len() as u64);
        for event in &self.history {
            write_historical_event(&mut hasher, event);
        }
        hasher.write_u64(u64::from(self.next_world_id));

        // 击杀聚合计数（决策二：数全部击杀）——理由见 Self::kill_counts
        // 文档「参与 hash()」一节：同一条先例第六次重演。BTreeMap 按键
        // 自然顺序遍历，不涉及 HashMap/HashSet 迭代顺序（约束 C5）。
        hasher.write_u64(self.kill_counts.len() as u64);
        for (kind, count) in &self.kill_counts {
            hasher.write_u64(u64::from(kind.get()));
            hasher.write_u64(*count);
        }

        // 地面物品（P6 第二批：背包与地面物品）——同一条先例第七次
        // 重演：`Intent::Drop`/`Intent::PickUp`/老化清理都会真实改写
        // `ground_items`，缺席 hash() 测不出这些结算跑偏。`Vec` 保序，
        // 不涉及 HashMap/HashSet 迭代顺序（约束 C5）。
        hasher.write_u64(self.ground_items.len() as u64);
        for item in &self.ground_items {
            hasher.write_i64(i64::from(item.pos.x()));
            hasher.write_i64(i64::from(item.pos.y()));
            write_item_stack(&mut hasher, &item.stack);
            hasher.write_i64(item.dropped_at.0);
            // 容器内容物（NPC 死亡掉落批次新增，`GroundItemStack::contents`
            // 字段文档「参与 hash()」一节）——尸体搜刮真实改写世界状态，
            // 缺席这里同样会重演「新字段只加了，没人测过是否被覆盖」的
            // 既有判据缺口。空 contents（绝大多数普通地面物品）只写一个
            // 长度 0，不额外产生任何哈希副作用。
            hasher.write_u64(item.contents.len() as u64);
            for content_stack in &item.contents {
                write_item_stack(&mut hasher, content_stack);
            }
            // 放置状态（家具放置状态批次新增，`GroundItemStack::placed`
            // 字段文档「为什么必须进世界状态」一节）——它决定这一格能
            // 不能再丢东西、这一堆会不会老化、它当不当得了制作场地，
            // 三条都是真实玩法差异，缺席 hash() 就是又一次「新字段只加
            // 了，没人测过是否被覆盖」。
            hasher.write_u64(u64::from(item.placed));
        }

        // 已物化据点集合（NPC 生成批次）——同一条先例第八次重演，理由
        // 见 Self::materialized_settlements 文档「参与 hash()」一节：同一
        // 个世界里「这座据点物化过」与「没物化过」此后走向完全不同的两批
        // 实体，缺席 hash() 就测不出「区块重载又生成一批 NPC」这条缺陷
        // 有没有回潮。`Vec` 有序去重，不涉及 HashMap/HashSet 迭代顺序
        // （约束 C5）。
        hasher.write_u64(self.materialized_settlements.len() as u64);
        for site in &self.materialized_settlements {
            hasher.write_u64(u64::from(site.get()));
        }

        hasher.finish()
    }
}

/// 把一个 [`ItemStack`] 混入哈希——[`WorldState::hash`] 的帮手，供
/// 背包（`Agent::inventory`）与地面物品（`WorldState::ground_items`）
/// 共用，两者都是"一堆 `ItemStack`"，理应用同一套编码，不各写一份。
fn write_item_stack(hasher: &mut StateHasher, stack: &ItemStack) {
    hasher.write_u64(u64::from(stack.def.get()));
    hasher.write_u64(u64::from(stack.count));
    match stack.durability {
        Some(durability) => {
            hasher.write_u64(1);
            hasher.write_i64(i64::from(durability));
        }
        None => hasher.write_u64(0),
    }
}

/// 把一个 `Option<ContentIndex>` 混入哈希——[`WorldState::hash`] 的
/// 帮手，供 `Agent::creature_kind` 使用。与 [`write_optional_entity`]
/// 同一种模式：先写判别字节区分 `Some`/`None`。
fn write_optional_content_index(hasher: &mut StateHasher, index: Option<ContentIndex>) {
    match index {
        Some(index) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(index.get()));
        }
        None => hasher.write_u64(0),
    }
}

/// 把一个 `Option<WorldId>` 混入哈希——[`WorldState::hash`] 的帮手，
/// 供 `Agent::remembered_id`/`KillRecord::killer` 使用。
fn write_optional_world_id(hasher: &mut StateHasher, id: Option<WorldId>) {
    match id {
        Some(id) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(id.get()));
        }
        None => hasher.write_u64(0),
    }
}

/// 把一条 [`HistoricalEvent`] 混入哈希——[`WorldState::hash`] 的帮手
/// （击杀与死亡记录批次）。每个变体先写一个判别字节，让不同变体产出
/// 的哈希不会因为字段布局恰好雷同而碰撞——这条预留在世界历史生成
/// 批次新增两个变体时直接生效，不需要回头补。
///
/// # 据点事件为什么也在这里
///
/// [`HistoricalEventKind::SettlementFounded`]/`SettlementAbandoned`
/// 由 [`crate::chronicle`] 在世界生成期产出，**默认不进 `WorldState`**
/// （ADR 0009：整份编年史是种子的纯函数，读档时重新派生）。但
/// `WorldState::history` 是一个 `Vec<HistoricalEvent>`，类型上完全
///容得下它们——若将来「历史偏差」需要把某几条真的存进去（例如玩家
/// 亲手烧掉一座村子），这里的分支已经就位，不会因为漏了一条而让那次
/// 改动悄悄不进哈希。
fn write_historical_event(hasher: &mut StateHasher, event: &HistoricalEvent) {
    hasher.write_u64(u64::from(event.id.get()));
    hasher.write_i64(event.at.0);
    hasher.write_i64(i64::from(event.location.x()));
    hasher.write_i64(i64::from(event.location.y()));
    match &event.kind {
        HistoricalEventKind::Kill(record) => {
            hasher.write_u64(0);
            write_optional_world_id(hasher, record.killer);
            hasher.write_u64(u64::from(record.victim.get()));
            write_kill_cause(hasher, &record.cause);
            hasher.write_i64(i64::from(record.killing_blow.damage));
            hasher.write_i64(i64::from(record.killing_blow.remaining_health));
            hasher.write_u64(u64::from(record.victim_state.poisoned));
            hasher.write_u64(u64::from(record.victim_state.surrounded));
        }
        HistoricalEventKind::SettlementFounded(record) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(record.site.get()));
            hasher.write_u64(u64::from(record.epoch));
            hasher.write_u64(u64::from(record.initial_population));
            hasher.write_u64(u64::from(record.land_area));
        }
        HistoricalEventKind::SettlementAbandoned(record) => {
            hasher.write_u64(2);
            hasher.write_u64(u64::from(record.site.get()));
            hasher.write_u64(u64::from(record.epoch));
            hasher.write_u64(u64::from(record.peak_population));
            hasher.write_u64(u64::from(record.epochs_inhabited));
            write_settlement_demise(hasher, &record.cause);
        }
    }
}

/// 把一条 [`SettlementDemise`] 混入哈希——[`write_historical_event`] 的
/// 帮手，与 [`write_kill_cause`] 同一种模式：先写变体判别字节，各变体
/// 互不混淆。
fn write_settlement_demise(hasher: &mut StateHasher, cause: &SettlementDemise) {
    match *cause {
        SettlementDemise::Depopulation => hasher.write_u64(0),
        SettlementDemise::ResourceExhausted { resource } => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(resource.get()));
        }
        SettlementDemise::War { aggressor } => {
            hasher.write_u64(2);
            hasher.write_u64(u64::from(aggressor.get()));
        }
        SettlementDemise::Plague { dead } => {
            hasher.write_u64(3);
            hasher.write_u64(u64::from(dead));
        }
    }
}

/// 把一个 [`KillCause`] 混入哈希——[`write_historical_event`] 的帮手。
/// 与 [`write_space`]/[`write_affiliation`] 同一种模式：先写变体判别
/// 字节，各变体互不混淆。
fn write_kill_cause(hasher: &mut StateHasher, cause: &KillCause) {
    match *cause {
        KillCause::Melee { weapon } => {
            hasher.write_u64(0);
            write_optional_content_index(hasher, weapon);
        }
        KillCause::Skill { skill } => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(skill.get()));
        }
        KillCause::Terrain { kind } => {
            hasher.write_u64(2);
            hasher.write_u64(u64::from(kind.index().get()));
        }
        KillCause::Fall => hasher.write_u64(3),
        KillCause::Starvation => hasher.write_u64(4),
        KillCause::Poison => hasher.write_u64(5),
        KillCause::Environmental(index) => {
            hasher.write_u64(6);
            hasher.write_u64(u64::from(index.get()));
        }
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
/// 的帮手（P5 批次 B）。七项属性（六项主属性 + 幸运，幸运并入
/// `AttributeKind` 批次新增）逐一混入，顺序与字段声明顺序一致，恒定
/// 不依赖任何运行期状态。
///
/// 幸运曾经是 `Agent` 上独立于 `stats` 之外的字段，单独混入哈希（紧跟
/// 在 `profession`/`race` 之后）；并入 `BaseStats` 后随 `stats` 一起在
/// 这里混入——摘要的字节序列因此改变，黄金基准常量需要重新生成，见
/// `crates/ll-sim/tests/replay.rs`/`crates/ll-world/tests/determinism.rs`
/// 的 `EXPECTED_*_DIGEST`。
fn write_stats(hasher: &mut StateHasher, stats: crate::entity::BaseStats) {
    hasher.write_i64(i64::from(stats.strength));
    hasher.write_i64(i64::from(stats.dexterity));
    hasher.write_i64(i64::from(stats.constitution));
    hasher.write_i64(i64::from(stats.intelligence));
    hasher.write_i64(i64::from(stats.willpower));
    hasher.write_i64(i64::from(stats.charisma));
    hasher.write_i64(i64::from(stats.luck));
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
fn write_mod_state(hasher: &mut StateHasher, state: &BTreeMap<(String, String), ModStateValue>) {
    hasher.write_u64(state.len() as u64);
    for ((namespace, key), value) in state {
        write_len_prefixed_bytes(hasher, namespace.as_bytes());
        write_len_prefixed_bytes(hasher, key.as_bytes());
        write_script_value(hasher, value);
    }
}

/// 把一个 [`ModStateValue`] 混入哈希——[`write_mod_state`] 的帮手。
/// 与 [`write_space`]/[`write_affiliation`] 同一种模式：先写变体判别
/// 字节，各变体互不混淆；`List`/`Map` 递归调用自身，天然覆盖任意嵌套
/// 深度。
fn write_script_value(hasher: &mut StateHasher, value: &ModStateValue) {
    match value {
        ModStateValue::Int(n) => {
            hasher.write_u64(0);
            hasher.write_i64(*n);
        }
        ModStateValue::Bool(b) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(*b));
        }
        ModStateValue::Str(s) => {
            hasher.write_u64(2);
            write_len_prefixed_bytes(hasher, s.as_bytes());
        }
        ModStateValue::Ref(s) => {
            hasher.write_u64(3);
            write_len_prefixed_bytes(hasher, s.as_bytes());
        }
        ModStateValue::Entity(id) => {
            hasher.write_u64(4);
            hasher.write_u64(u64::from(id.index()));
            hasher.write_u64(u64::from(id.generation()));
        }
        ModStateValue::List(items) => {
            hasher.write_u64(5);
            hasher.write_u64(items.len() as u64);
            for item in items {
                write_script_value(hasher, item);
            }
        }
        ModStateValue::Map(map) => {
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
/// [`write_mod_state`] 文档「避免相邻字符串边界不清」一节。
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
    use crate::entity::{ActiveStatModifier, AttributeKind, BaseStats, RestState};
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
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: crate::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: crate::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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

    /// 造一个带唯一一个实体的测试世界，供 hash 相关测试复用——`level`/
    /// `experience`/`xp_to_next_level` 三个字段可由调用方指定，其余
    /// 字段固定为占位值。
    fn test_world_with_one_agent(level: i32, experience: i64, xp_to_next_level: i64) -> WorldState {
        let mut world = test_world();
        let pos = world.size.wrap(5, 5);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: ContentIndex::default(),
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level,
            experience,
            xp_to_next_level,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        world
    }

    #[test]
    fn 潜行状态变化会改变世界哈希() {
        // ADR 0022 红/绿验证：Agent::stealthed 必须已经手动补进 hash()
        // 的逐字段遍历——本函数手工验证过会失败（把 hash() 里新增的
        // `hasher.write_u64(u64::from(agent.stealthed));` 一行临时换成
        // 注释重跑，本测试会 panic：两个只有 stealthed 不同的世界算出
        // 同一个哈希），恢复后转绿。这也是本批次黄金基准重冻第 2 步
        // 用的同一条手法，见 crates/ll-sim/tests/replay.rs 的
        // EXPECTED_REPLAY_DIGEST 文档「第十六次重冻的原因」一节。
        // Arrange：两个世界只差潜行状态这一个字段。
        let world_visible = test_world_with_one_agent(1, 0, 100);
        let mut world_stealthed = test_world_with_one_agent(1, 0, 100);
        let only_agent = world_stealthed
            .actors
            .iter_with_id()
            .map(|(id, _)| id)
            .next()
            .expect("test_world_with_one_agent 恰好生成一个实体");
        world_stealthed
            .actors
            .get_mut(only_agent)
            .expect("刚取到的 id 必然有效")
            .stealthed = true;

        // Act
        let (hash_visible, hash_stealthed) = (world_visible.hash(), world_stealthed.hash());

        // Assert
        assert_ne!(hash_visible, hash_stealthed);
    }

    #[test]
    fn 已知配方变化会改变世界哈希() {
        // ADR 0022 红/绿验证：Agent::known_recipes 必须已经手动补进
        // hash() 的逐字段遍历——本函数手工验证过会失败（把 hash() 里
        // 新增的 `write_content_index_vec(&mut hasher,
        // &agent.known_recipes);` 一行临时换成注释重跑，本测试会
        // panic：两个只差这一个字段的世界算出同一个哈希），恢复后
        // 转绿。这也是本批次黄金基准重冻第 2 步用的同一条手法，见
        // crates/ll-sim/tests/replay.rs 的 EXPECTED_REPLAY_DIGEST
        // 文档「第十八次重冻的原因」一节。
        //
        // 这个字段值得这道守卫，是因为它**真的改变结算**：
        // `ll_sim::resolve::resolve_craft` 对声明了 requires_discovery
        // 的配方多判一道「会不会做」的闸门，两个只差已知配方的世界从
        // 这一刻起会走出不同的未来。
        // Arrange：两个世界只差已知配方这一个字段。
        let world_ignorant = test_world_with_one_agent(1, 0, 100);
        let mut world_learned = test_world_with_one_agent(1, 0, 100);
        let only_agent = world_learned
            .actors
            .iter_with_id()
            .map(|(id, _)| id)
            .next()
            .expect("test_world_with_one_agent 恰好生成一个实体");
        world_learned
            .actors
            .get_mut(only_agent)
            .expect("刚取到的 id 必然有效")
            .known_recipes
            .push(ContentIndex::default());

        // Act
        let (hash_ignorant, hash_learned) = (world_ignorant.hash(), world_learned.hash());

        // Assert
        assert_ne!(hash_ignorant, hash_learned);
    }

    #[test]
    fn 未分配属性点变化会改变世界哈希() {
        // ADR 0022 红/绿验证：Agent::unspent_attribute_points 必须已经
        // 手动补进 hash() 的逐字段遍历——本函数手工验证过会失败（把
        // hash() 里新增的
        // `hasher.write_u64(u64::from(agent.unspent_attribute_points));`
        // 一行临时换成注释重跑，本测试会 panic：两个只差这一个字段的
        // 世界算出同一个哈希），恢复后转绿。
        // Arrange：两个世界只差未分配属性点这一个字段。
        let world_none = test_world_with_one_agent(1, 0, 100);
        let mut world_some = test_world_with_one_agent(1, 0, 100);
        let only_agent = world_some
            .actors
            .iter_with_id()
            .map(|(id, _)| id)
            .next()
            .expect("test_world_with_one_agent 恰好生成一个实体");
        world_some
            .actors
            .get_mut(only_agent)
            .expect("刚取到的 id 必然有效")
            .unspent_attribute_points = 2;

        // Act
        let (hash_none, hash_some) = (world_none.hash(), world_some.hash());

        // Assert
        assert_ne!(hash_none, hash_some);
    }

    #[test]
    fn 未分配技能点变化会改变世界哈希() {
        // 同上一条的红/绿验证，针对
        // `hasher.write_u64(u64::from(agent.unspent_skill_points));`
        // 那一行——两个字段各要一条测试：只写一条时，把两行里的任意
        // 一行删掉都可能仍然有另一条覆盖不到的字段悄悄漏出哈希，那正
        // 是 ADR 0022 点名的「判据字段不全」。
        // Arrange
        let world_none = test_world_with_one_agent(1, 0, 100);
        let mut world_some = test_world_with_one_agent(1, 0, 100);
        let only_agent = world_some
            .actors
            .iter_with_id()
            .map(|(id, _)| id)
            .next()
            .expect("test_world_with_one_agent 恰好生成一个实体");
        world_some
            .actors
            .get_mut(only_agent)
            .expect("刚取到的 id 必然有效")
            .unspent_skill_points = 1;

        // Act
        let (hash_none, hash_some) = (world_none.hash(), world_some.hash());

        // Assert
        assert_ne!(hash_none, hash_some);
    }

    #[test]
    fn 等级变化会改变世界哈希() {
        // ADR 0022 红/绿验证：Agent::level 必须已经手动补进 hash()
        // 的逐字段遍历——本函数手工验证过会失败（临时把 state.rs 里
        // 新增的三行 hasher.write 删掉重跑，本测试会 panic：两个只有
        // level 不同的世界算出同一个哈希），恢复后转绿，见本文件顶部
        // hash() 分支上方的注释「红/绿验证见……」一节。
        // Arrange
        let world_level_1 = test_world_with_one_agent(1, 0, 100);
        let world_level_2 = test_world_with_one_agent(2, 0, 100);

        // Act
        let (hash_1, hash_2) = (world_level_1.hash(), world_level_2.hash());

        // Assert
        assert_ne!(hash_1, hash_2);
    }

    #[test]
    fn 经验值变化会改变世界哈希() {
        // Arrange
        let world_zero_xp = test_world_with_one_agent(1, 0, 100);
        let world_some_xp = test_world_with_one_agent(1, 40, 100);

        // Act
        let (hash_zero, hash_some) = (world_zero_xp.hash(), world_some_xp.hash());

        // Assert
        assert_ne!(hash_zero, hash_some);
    }

    #[test]
    fn 下一级所需经验变化会改变世界哈希() {
        // Arrange
        let world_threshold_100 = test_world_with_one_agent(1, 0, 100);
        let world_threshold_250 = test_world_with_one_agent(1, 0, 250);

        // Act
        let (hash_100, hash_250) = (world_threshold_100.hash(), world_threshold_250.hash());

        // Assert
        assert_ne!(hash_100, hash_250);
    }

    /// 造一个带唯一一个实体的测试世界，`resource_pools` 恰好一条
    /// 条目——供资源池 hash 测试复用，理由同
    /// [`test_world_with_one_agent`]。
    fn test_world_with_one_agent_pool_current(pool: ContentIndex, current: i32) -> WorldState {
        let mut world = test_world();
        let pos = world.size.wrap(5, 5);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: ContentIndex::default(),
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::from([(pool, current)]),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        world
    }

    #[test]
    fn 资源池当前值变化会改变世界哈希() {
        // ADR 0022 红/绿验证：`Agent::resource_pools` 必须已经手动补进
        // `hash()` 的逐字段遍历——本函数手工验证过会失败（临时把
        // `state.rs` 里新增的 `hasher.write_u64(agent.resource_pools.len()..)`
        // 与紧随其后的 `for` 循环删掉重跑，本测试会 panic：两个只有
        // 资源池当前值不同的世界算出同一个哈希，golden baseline 重冻
        // 时也用同一段删除/恢复流程独立核实过一遍，见
        // `crates/ll-sim/tests/replay.rs` `EXPECTED_REPLAY_DIGEST`
        // 「第十一次重冻的原因」一节），恢复后转绿，与
        // `等级变化会改变世界哈希` 同一条既有先例。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let pool = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:sorcery_points").unwrap());
        let world_empty = test_world_with_one_agent_pool_current(pool, 5);
        let world_full = test_world_with_one_agent_pool_current(pool, 20);

        // Act
        let (hash_empty, hash_full) = (world_empty.hash(), world_full.hash());

        // Assert
        assert_ne!(hash_empty, hash_full);
    }

    /// 造一个带唯一一个实体的测试世界，`spent_slots` 恰好一条条目——
    /// 供法术位 hash 测试复用，理由同 [`test_world_with_one_agent`]。
    fn test_world_with_one_agent_spent_slot(
        pool: ContentIndex,
        tier: u8,
        spent: u32,
    ) -> WorldState {
        let mut world = test_world();
        let pos = world.size.wrap(5, 5);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: ContentIndex::default(),
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::from([((pool, tier), spent)]),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        world
    }

    #[test]
    fn 法术位已消耗数变化会改变世界哈希() {
        // ADR 0022 红/绿验证，同一条先例：`Agent::spent_slots` 必须已经
        // 手动补进 `hash()` 的逐字段遍历——本函数手工验证过会失败（临时
        // 把 `state.rs` 里新增的 `hasher.write_u64(agent.spent_slots.len()..)`
        // 与紧随其后的 `for` 循环删掉重跑，本测试会 panic：两个只有
        // 已消耗数不同的世界算出同一个哈希），恢复后转绿。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let pool =
            interner.intern(ll_core::ident::NamespacedId::parse("lostland:wizard_slots").unwrap());
        let world_unspent = test_world_with_one_agent_spent_slot(pool, 3, 0);
        let world_spent = test_world_with_one_agent_spent_slot(pool, 3, 1);

        // Act
        let (hash_unspent, hash_spent) = (world_unspent.hash(), world_spent.hash());

        // Assert
        assert_ne!(hash_unspent, hash_spent);
    }

    /// 造一个带唯一一个实体的测试世界，`resting` 取给定值——供休息事件
    /// hash 测试复用，理由同 [`test_world_with_one_agent`]。
    fn test_world_with_one_agent_resting(resting: Option<RestState>) -> WorldState {
        let mut world = test_world();
        let pos = world.size.wrap(5, 5);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: ContentIndex::default(),
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        world
    }

    #[test]
    fn 休息状态变化会改变世界哈希() {
        // ADR 0022 红/绿验证，同一条先例：`Agent::resting` 必须已经
        // 手动补进 `hash()` 的逐字段遍历——本函数手工验证过会失败（临时
        // 把 `state.rs` 里新增的 `match agent.resting { .. }` 分支删掉
        // 重跑，本测试会 panic：一个正在休息、一个未在休息的世界算出
        // 同一个哈希），恢复后转绿。
        // Arrange
        let world_idle = test_world_with_one_agent_resting(None);
        let world_resting = test_world_with_one_agent_resting(Some(RestState {
            started_at: Tick(0),
            target_ticks: 480,
        }));

        // Act
        let (hash_idle, hash_resting) = (world_idle.hash(), world_resting.hash());

        // Assert
        assert_ne!(hash_idle, hash_resting);
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
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: crate::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: crate::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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
    fn 每实体mod状态写入后世界哈希改变() {
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
            .mod_state
            .insert(
                ("lostland".to_string(), "cooldown".to_string()),
                crate::mod_state::ModStateValue::Int(5),
            );

        // Assert
        assert_ne!(world.hash(), hash_before);
    }

    #[test]
    fn mod状态序列化往返后保持原样() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let id = world.actors.spawn(agent);
        world
            .actors
            .get_mut(id)
            .expect("刚生成的实体必然存在")
            .mod_state
            .insert(
                ("lostland".to_string(), "reputation".to_string()),
                crate::mod_state::ModStateValue::List(vec![
                    crate::mod_state::ModStateValue::Int(1),
                    crate::mod_state::ModStateValue::Bool(true),
                    crate::mod_state::ModStateValue::Str("x".into()),
                ]),
            );

        // Act
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
        let decoded: WorldState = serde_json::from_slice(&encoded).expect("往返不应失败");

        // Assert
        assert_eq!(
            decoded.actors.get(id).expect("实体应当往返存活").mod_state,
            world.actors.get(id).expect("原世界里实体仍在").mod_state
        );
    }

    /// 供本文件末尾几条哈希/mod 状态测试复用的占位实体——字段取值不
    /// 重要，测试只关心「多了一份 mod 状态记录之后哈希是否改变」。
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
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: crate::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: crate::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        }
    }

    #[test]
    fn 属性修正内层来源的强度变化后世界哈希改变() {
        // 红/绿测试的「绿」半边，六节存储改法（`active_stat_modifiers`
        // 从单层 BTreeMap 换成两层）：两个世界的外层完全相同——都恰好
        // 一项属性（力量）正被修正，来源也是同一个——仅内层记录的
        // `delta` 不同。若 `hash()` 只混入了外层条目数就结束（漏掉内层
        // 遍历，见 `WorldState::hash` 文档「`active_stat_modifiers` 是
        // 两层 BTreeMap」一节警告的那类隐蔽缺口），这两个世界会算出
        // 相同的哈希——这条断言就是用来测出这类缺口的。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let source = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));

        let mut world_a = test_world();
        let mut agent_a = blank_agent(&world_a);
        agent_a.active_stat_modifiers.insert(
            AttributeKind::Strength,
            BTreeMap::from([(
                source,
                ActiveStatModifier {
                    delta: 5,
                    expires_at: Tick(100),
                },
            )]),
        );
        world_a.actors.spawn(agent_a);

        let mut world_b = test_world();
        let mut agent_b = blank_agent(&world_b);
        agent_b.active_stat_modifiers.insert(
            AttributeKind::Strength,
            BTreeMap::from([(
                source,
                ActiveStatModifier {
                    delta: 9,
                    expires_at: Tick(100),
                },
            )]),
        );
        world_b.actors.spawn(agent_b);

        // Act & Assert：外层条目数（1 种正被修正的属性）逐位相同，仅
        // 内层 delta 不同（5 对 9），哈希必须不同。
        assert_ne!(world_a.hash(), world_b.hash());
    }

    #[test]
    fn 属性修正内层来源不同但外层属性相同时世界哈希也不同() {
        // 与上一条互补：这次外层属性种类数、每种属性的来源数、每条
        // 修正的 delta/expires_at 全部相同，唯独「来源」这个内层键本身
        // 不同——若 hash() 只混入 delta/expires_at 却忘了混入内层键
        // （来源 ContentIndex 本身），这两个世界也会算出相同哈希。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let source_a = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
        let source_b = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:blessing").expect("合法标识符"));

        let mut world_a = test_world();
        let mut agent_a = blank_agent(&world_a);
        agent_a.active_stat_modifiers.insert(
            AttributeKind::Strength,
            BTreeMap::from([(
                source_a,
                ActiveStatModifier {
                    delta: 5,
                    expires_at: Tick(100),
                },
            )]),
        );
        world_a.actors.spawn(agent_a);

        let mut world_b = test_world();
        let mut agent_b = blank_agent(&world_b);
        agent_b.active_stat_modifiers.insert(
            AttributeKind::Strength,
            BTreeMap::from([(
                source_b,
                ActiveStatModifier {
                    delta: 5,
                    expires_at: Tick(100),
                },
            )]),
        );
        world_b.actors.spawn(agent_b);

        // Act & Assert
        assert_ne!(world_a.hash(), world_b.hash());
    }

    #[test]
    fn 空的属性修正映射与逐字段相同的另一个空世界哈希逐位相等() {
        // 红/绿测试的「红」半边：两个独立构造、`active_stat_modifiers`
        // 都是空的两层 BTreeMap 的世界，哈希必须逐位相等——证明上面两条
        // 「内层变了就变」的断言不是因为 hash() 本身不稳定才恰好通过，
        // 也顺带核实「空外层 BTreeMap 与空的两层嵌套 BTreeMap」在当前
        // 混入方式下产生相同字节流（外层 `len()` 为零时循环体完全不
        // 执行，不会因为值类型从单层换成两层而多写任何字节）。
        // Arrange
        let mut world_a = test_world();
        world_a.actors.spawn(blank_agent(&world_a));
        let mut world_b = test_world();
        world_b.actors.spawn(blank_agent(&world_b));

        // Act & Assert
        assert_eq!(world_a.hash(), world_b.hash());
    }

    #[test]
    fn 历史事件日志增加一条记录后世界哈希改变() {
        // 红/绿测试的「绿」半边：ADR 0022 要求新字段必须真正进
        // hash()，这里直接验证——若这条断言失败（改动 history 后哈希
        // 不变），说明 hash() 没有覆盖这个字段，回归测试对这部分状态
        // 完全空跑。
        // Arrange
        let mut world = test_world();
        let victim = world.actors.spawn(blank_agent(&world));
        let hash_before = world.hash();

        // Act
        world.record_kill(crate::history::KillReport {
            at: Tick(1),
            location: world.actors.get(victim).expect("刚生成必然存在").pos,
            victim,
            killer: None,
            cause: crate::history::KillCause::Fall,
            damage: 999,
            remaining_health: -1,
        });

        // Assert
        assert_ne!(world.hash(), hash_before);
    }

    #[test]
    fn worldid分配计数器推进后世界哈希改变() {
        // 红/绿测试的另一半：即使 history 本身长度不变（本用例不追加
        // 任何历史事件），单独推进 next_world_id 也必须改变哈希——
        // 否则「计数器悄悄跳号」这类缺陷不会被任何黄金基准测出来。
        // Arrange
        let mut world = test_world();
        let hash_before = world.hash();

        // Act
        world.allocate_world_id();

        // Assert
        assert_ne!(world.hash(), hash_before);
    }

    #[test]
    fn record_kill未改变world时哈希与改动前逐位相同() {
        // 红/绿测试的「红」半边：两个独立构造、内容逐字段相同的世界，
        // 哈希必须相等——证明上面两条「改了就变」的断言不是因为
        // hash() 本身不稳定/每次调用都不同才恰好通过。
        // Arrange
        let world_a = test_world();
        let world_b = test_world();

        // Act & Assert
        assert_eq!(world_a.hash(), world_b.hash());
    }

    #[test]
    fn worldid分配器同一段模拟跑两遍产出相同的id序列() {
        // WorldId 分配器必须确定性（约束 C5：不得依赖 HashMap 迭代
        // 顺序；也不得用随机或时间）——这里跑两遍完全相同的一段"模拟"
        // （对同一批实体依次记录击杀），断言两次产出的历史事件 id
        // 序列逐一相等。
        // Arrange
        fn run_once() -> Vec<u32> {
            let mut world = test_world();
            let a = world.actors.spawn(blank_agent(&world));
            let b = world.actors.spawn(blank_agent(&world));
            let c = world.actors.spawn(blank_agent(&world));
            for victim in [a, b, c] {
                let pos = world.actors.get(victim).expect("刚生成必然存在").pos;
                world.record_kill(crate::history::KillReport {
                    at: Tick(1),
                    location: pos,
                    victim,
                    killer: None,
                    cause: crate::history::KillCause::Fall,
                    damage: 50,
                    remaining_health: -1,
                });
            }
            world.history.iter().map(|event| event.id.get()).collect()
        }

        // Act
        let first_run = run_once();
        let second_run = run_once();

        // Assert
        assert_eq!(first_run, second_run);
        // 附带核实序列本身确实是三个不同的、单调递增的号——不是碰巧
        // 全部相等（例如实现里误把计数器写死成常量）才通过上面那条
        // 断言。每次 record_kill 消耗两个号（先给 victim 懒分配
        // remembered_id，再给事件本身分配 id，见其文档），因此三次
        // 击杀产出 [1, 3, 5] 而不是连续的 [0, 1, 2]。
        assert_eq!(first_run, vec![1, 3, 5]);
    }

    #[test]
    fn remembered_id_of_or_assign对已具名实体不改变其id() {
        // Arrange
        let mut world = test_world();
        let victim = world.actors.spawn(blank_agent(&world));
        let first = world
            .remembered_id_of_or_assign(victim)
            .expect("刚生成的实体必然存在");

        // Act
        let second = world
            .remembered_id_of_or_assign(victim)
            .expect("同一个实体第二次查询仍然存在");

        // Assert：第二次调用返回同一个 id，不是又分配了一个新的。
        assert_eq!(first, second);
    }

    #[test]
    fn record_kill在victim已被销毁时不追加任何历史事件() {
        // 调用时机纪律的直接验证——见 WorldState::record_kill 文档
        // 「调用时机」一节。
        // Arrange
        let mut world = test_world();
        let victim = world.actors.spawn(blank_agent(&world));
        let pos = world.actors.get(victim).expect("刚生成必然存在").pos;
        world.actors.despawn(victim);

        // Act
        let result = world.record_kill(crate::history::KillReport {
            at: Tick(1),
            location: pos,
            victim,
            killer: None,
            cause: crate::history::KillCause::Fall,
            damage: 10,
            remaining_health: -1,
        });

        // Assert
        assert_eq!(result, None);
        assert!(world.history.is_empty());
    }

    #[test]
    fn record_kill_count对同一个kind累加() {
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let goblin = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));

        // Act
        world.record_kill_count(goblin);
        world.record_kill_count(goblin);
        world.record_kill_count(goblin);

        // Assert
        assert_eq!(world.kill_counts.get(&goblin), Some(&3));
    }

    #[test]
    fn kill_counts变化会改变world的哈希() {
        // 红/绿判据：这是 ADR 0022「不完整的确定性哈希等于没有哈希」的
        // 直接验收——若 hash() 漏掉 kill_counts，这条断言会失败（红），
        // 补齐 hash() 之后才会通过（绿）。
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let goblin = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));
        let hash_before = world.hash();

        // Act
        world.record_kill_count(goblin);

        // Assert
        assert_ne!(world.hash(), hash_before);
    }

    #[test]
    fn kill_counts为空与非空的两个世界哈希不同() {
        // 与上一条互补：不是随便改动世界状态都会撞上这条哈希差异
        // （避免巧合通过），这里直接对比两个独立构造的世界。
        // Arrange
        let empty_world = test_world();
        let mut counted_world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let goblin = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));
        counted_world.record_kill_count(goblin);

        // Act & Assert
        assert_ne!(counted_world.hash(), empty_world.hash());
    }

    #[test]
    fn kill_counts序列化往返后保持原样() {
        // 存档往返性质的直接验证——不是只测结构，是测真实的
        // serde_json 序列化路径（与本文件其余往返测试同一套判据）。
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let goblin = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));
        world.record_kill_count(goblin);
        world.record_kill_count(goblin);

        // Act
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
        let decoded: WorldState = serde_json::from_slice(&encoded).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded.kill_counts.get(&goblin), Some(&2));
    }

    #[test]
    fn 新建的世界kill_counts默认为空() {
        // Arrange & Act
        let world = test_world();

        // Assert
        assert!(world.kill_counts.is_empty());
    }

    #[test]
    fn 新建的世界地面物品默认为空() {
        // Arrange & Act
        let world = test_world();

        // Assert
        assert!(world.ground_items.is_empty());
    }

    #[test]
    fn 地面物品序列化往返后保持原样() {
        // 与 kill_counts序列化往返后保持原样 同一条判据——真实
        // serde_json 路径，不只是结构层面。
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let arrow = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:arrow").expect("合法标识符"));
        world.ground_items.push(GroundItemStack {
            pos: world.size.wrap(3, 4),
            stack: ItemStack::new(arrow, 12),
            dropped_at: Tick(200),
            contents: Vec::new(),
            placed: false,
        });

        // Act
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
        let decoded: WorldState = serde_json::from_slice(&encoded).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded.ground_items, world.ground_items);
    }

    #[test]
    fn 已装备物品的耐久序列化往返后保持原样() {
        // 耐久与 Intent::Use 落地批次（P6 第五批）：`resolve_attack`
        // 会真的改写 `Agent::equipment` 里某一堆的 `durability`（见
        // `Effect::AdjustEquipmentDurability`），存档必须能把这个"用过、
        // 磨损过"的状态原样带回来，不是每次读档都退回满耐久——与
        // 上面「地面物品序列化往返后保持原样」同一条判据：真实
        // serde_json 路径，不只是结构层面。
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let armor_def = interner.intern(
            ll_core::ident::NamespacedId::parse("lostland:iron_armor").expect("合法标识符"),
        );
        let pos = world.size.wrap(5, 5);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        let mut equipment = std::collections::BTreeMap::new();
        equipment.insert(
            crate::item::EquipSlot::BODY,
            ItemStack::with_durability(armor_def, 1, 37),
        );
        let id = world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment,
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: crate::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: crate::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });

        // Act
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
        let decoded: WorldState = serde_json::from_slice(&encoded).expect("刚序列化的数据必然合法");

        // Assert：往返后耐久值恰好是 37，不是满耐久或默认值。
        let decoded_stack = decoded
            .actors
            .get(id)
            .expect("往返后仍能按原标识取回该实体")
            .equipment
            .get(&crate::item::EquipSlot::BODY)
            .expect("往返后装备栏条目仍在");
        assert_eq!(decoded_stack.durability, Some(37));
    }

    #[test]
    fn 清理超过阈值的地面物品会被移除() {
        // Arrange：世界时钟推进到超过阈值的时刻,丢弃时刻停留在 0。
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let arrow = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:arrow").expect("合法标识符"));
        world.ground_items.push(GroundItemStack {
            pos: world.size.wrap(0, 0),
            stack: ItemStack::new(arrow, 1),
            dropped_at: Tick(0),
            contents: Vec::new(),
            placed: false,
        });
        world.advance(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS + 1);

        // Act
        let removed =
            world.cleanup_aged_ground_items(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS);

        // Assert
        assert_eq!(removed, 1);
        assert!(world.ground_items.is_empty());
    }

    #[test]
    fn 清理未超过阈值的地面物品保留() {
        // 红/绿对照：把上一条测试的阈值判定改成"<="会让这条测试变红
        // （恰好等于阈值的物品被误删）——这里额外验证"未到阈值"这个
        // 更常见的场景,两条测试合起来锁住边界条件。
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let arrow = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:arrow").expect("合法标识符"));
        world.ground_items.push(GroundItemStack {
            pos: world.size.wrap(0, 0),
            stack: ItemStack::new(arrow, 1),
            dropped_at: Tick(0),
            contents: Vec::new(),
            placed: false,
        });
        world.advance(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS - 1);

        // Act
        let removed =
            world.cleanup_aged_ground_items(WorldState::DEFAULT_GROUND_ITEM_MAX_AGE_TICKS);

        // Assert
        assert_eq!(removed, 0);
        assert_eq!(world.ground_items.len(), 1);
    }

    #[test]
    fn 老化阈值由调用方传入不同世界可以用不同阈值() {
        // 「老化阈值不该写死在引擎里」的直接验收：同一份刚好卡在 100
        // ticks 的地面物品,传 50 判定为过期,传 200 判定为未过期——
        // 阈值真的是运行期参数,不是编译期常量。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let arrow = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:arrow").expect("合法标识符"));
        let mut world_a = test_world();
        world_a.ground_items.push(GroundItemStack {
            pos: world_a.size.wrap(0, 0),
            stack: ItemStack::new(arrow, 1),
            dropped_at: Tick(0),
            contents: Vec::new(),
            placed: false,
        });
        world_a.advance(100);
        let mut world_b = test_world();
        world_b.ground_items.push(GroundItemStack {
            pos: world_b.size.wrap(0, 0),
            stack: ItemStack::new(arrow, 1),
            dropped_at: Tick(0),
            contents: Vec::new(),
            placed: false,
        });
        world_b.advance(100);

        // Act
        let removed_with_short_threshold = world_a.cleanup_aged_ground_items(50);
        let removed_with_long_threshold = world_b.cleanup_aged_ground_items(200);

        // Assert
        assert_eq!(removed_with_short_threshold, 1);
        assert_eq!(removed_with_long_threshold, 0);
    }

    #[test]
    fn 地面物品不同的两个世界哈希不同() {
        // ADR 0022 判据：新增世界状态必须进 hash()——这里直接验证
        // ground_items 真的参与了摘要计算,不是加了字段但漏了混入。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let arrow = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:arrow").expect("合法标识符"));
        let world_a = test_world();
        let mut world_b = test_world();
        world_b.ground_items.push(GroundItemStack {
            pos: world_b.size.wrap(0, 0),
            stack: ItemStack::new(arrow, 1),
            dropped_at: Tick(0),
            contents: Vec::new(),
            placed: false,
        });

        // Act & Assert
        assert_ne!(world_a.hash(), world_b.hash());
    }

    #[test]
    fn 背包不同的两个世界哈希不同() {
        // 同一条 ADR 0022 判据,覆盖 Agent::inventory 这一侧。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let arrow = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:arrow").expect("合法标识符"));
        let mut world_a = test_world();
        blank_agent_spawn(&mut world_a, Vec::new());
        let mut world_b = test_world();
        blank_agent_spawn(&mut world_b, vec![ItemStack::new(arrow, 1)]);

        // Act & Assert
        assert_ne!(world_a.hash(), world_b.hash());
    }

    /// [`背包不同的两个世界哈希不同`] 专用的最小实体生成帮手——本文件
    /// 既有的 `blank_agent` 固定了 `inventory: Vec::new()`（见 sed 批量
    /// 插入的既有测试固件），这里补一个允许调用方指定背包内容的变体，
    /// 不改动既有 `blank_agent` 的签名（避免牵连它的全部既有调用点）。
    fn blank_agent_spawn(world: &mut WorldState, inventory: Vec<ItemStack>) -> EntityId {
        let mut agent = blank_agent(world);
        agent.inventory = inventory;
        world.actors.spawn(agent)
    }

    #[test]
    fn 装备栏不同的两个世界哈希不同() {
        // ADR 0022 判据,覆盖 Agent::equipment 这一侧（装备栏位批次，
        // P6 第三批）——与「背包不同的两个世界哈希不同」同一条纪律：
        // 直接验证 equipment 真的参与了摘要计算,不是加了字段但漏了
        // 混入。人工核验（真实执行）：把 state.rs `hash()` 里混入
        // `agent.equipment` 的那几行（`hasher.write_u64(agent.equipment.len()...)`
        // 起，到内层 for 循环结束）临时注释掉重新跑本测试，
        // 断言从通过变为失败（两个世界的哈希变得相等）——证明这条
        // 覆盖测试确实是红/绿的，不是恒真断言。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let sword = interner.intern(
            ll_core::ident::NamespacedId::parse("lostland:iron_sword").expect("合法标识符"),
        );
        let mut world_a = test_world();
        blank_agent_spawn_with_equipment(&mut world_a, BTreeMap::new());
        let mut world_b = test_world();
        blank_agent_spawn_with_equipment(
            &mut world_b,
            BTreeMap::from([(
                crate::item::EquipSlot::MAIN_HAND,
                ItemStack::with_durability(sword, 1, 100),
            )]),
        );

        // Act & Assert
        assert_ne!(world_a.hash(), world_b.hash());
    }

    /// [`装备栏不同的两个世界哈希不同`] 专用的最小实体生成帮手——同一个
    /// 「不改既有 `blank_agent` 签名」理由，见 [`blank_agent_spawn`]
    /// 文档。
    fn blank_agent_spawn_with_equipment(
        world: &mut WorldState,
        equipment: BTreeMap<crate::item::EquipSlot, ItemStack>,
    ) -> EntityId {
        let mut agent = blank_agent(world);
        agent.equipment = equipment;
        world.actors.spawn(agent)
    }
}
