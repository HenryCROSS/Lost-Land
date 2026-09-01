//! 据点名册：一座还有人住的据点，住着**谁**。
//!
//! 项目所有者的裁决把这件事的边界画得很死：
//!
//! > 「有 NPC 的地方必然存在一个据点，不然 NPC 上哪吃饭睡觉呢。」
//! > 「而且要确保 NPC 在未探索区域也能正常运作。」
//! > 「这个可以参考矮人要塞的例子。」
//!
//! 本模块是前两句在代码里的落点，第三句是它的方法论。
//!
//! # 一、形状：默认派生，只存偏差（ADR 0009）
//!
//! 一座据点的人口是历史推演的产物（[`ll_world::chronicle`]），而历史
//! 推演本身是种子的纯函数、**不进存档**。本模块把这条纪律往下推一层：
//!
//! ```text
//! 派生：据点 S 人口 P  →  P 个 NPC 的身份（种族 / 职业 / 名册序号）
//!                        由 (world_seed, S.id, 序号) 完全确定
//! 存偏差：只有真的被物化过的那些据点，其 NPC 才作为 Agent 进存档
//! ```
//!
//! 「未探索区域的 NPC 正常运作」因此不需要任何后台推进：**他们根本不
//! 需要实体化**。一座玩家从没走近过的村子，它有多少人、各自是干什么的，
//! 随时可以由 [`settlement_roster`] 当场算出来（同一颗种子逐位相同），
//! 而世界状态里一个字节都不占——这正是矮人要塞对离场文明单位的做法：
//! 抽象地存在，直到你真的遇见他们。
//!
//! # 二、「重复生成」这个问题在结构上消失了吗：一半是，一半不是
//!
//! **是的那一半**：名册派生本身是纯函数，算多少次都是同一份名册，不存在
//! 「第二次算出不一样的一批人」。
//!
//! **不是的那一半，也是本批次真正要解决的那个问题**：`Agent` 一旦被
//! 物化就进了 `WorldState::actors`，也就进了存档——而它此后会被玩家改变
//! （被杀、被抢、走开）。若「哪些据点已经物化过」这件事本身不记下来，
//! 区块被淘汰再加载时就会照着同一份名册**再生成一批**，把玩家杀掉的人
//! 原样复活。
//!
//! 记这件事需要的最小状态是**一份已物化据点的 id 集合**
//! （[`ll_world::state::WorldState::materialized_settlements`]），
//! 不是一份逐 NPC 的偏差表。理由是逐 NPC 偏差表要先回答一个本批次答不上
//! 的问题：**派生出来的那个人与存档里那个 `Agent` 之间的稳定身份是什么**。
//! `Agent` 上没有「我是 S 号据点名册里的第 7 个」这样的字段，加一个就是
//! 又一次 `WorldState::hash()` 改动 + 存档 remap；而加了之后，读档路径
//! 还要每次重跑一遍派生、逐条与存档比对、把差异合并回去——**换来的能力
//! 与「据点 id 集合」完全相同**（两者都只需要回答「这座据点该不该再生成
//! 一批人」）。ADR 0021 的判据在这里给出的是「不建」。
//!
//! 代价如实标注：已物化的据点从此**不再随人口变化**。一座村子被玩家屠尽
//! 之后不会有新人搬进来，因为「搬进来」需要的是一套据点的运行期演化，而
//! 那是独立一批的工作，不是这一批顺手做半个。
//!
//! # 二之二、每座据点长得不一样：建立者种族
//!
//! 项目所有者：「设计上每个聚居地应该都不太一样吧？还有种族的分配
//! 什么的」。
//!
//! 此前种族是**逐个居民**按权重抽的，资源只把权重挪一点——后果是全大陆
//! 的村子都混居，一座「矮人多一点」的村与一座「精灵多一点」的村，差别
//! 只在统计上，玩家走进去看不出来。现在先由
//! [`settlement_founder_race`] 抽一次**建立者种族**，其余居民默认随它，
//! 只有 [`OUTSIDER_PERMILLE`] 的人是外来者。
//!
//! 实测（种子 20260826，235 座活据点、4422 人）：
//!
//! | 建立者种族 | 据点数 | 占比 |
//! |---|---|---|
//! | 精灵 | 101 | 43.0% |
//! | 人类 | 68 | 28.9% |
//! | 矮人 | 66 | 28.1% |
//!
//! 建立者种族在全部名册里占 **87.7%**（期望值 `80% + 20%/3 ≈ 86.7%`，
//! 对得上）。一座典型的 24 人据点长这样：**17 精灵 / 4 矮人 / 3 人类**
//! ——一眼看得出这是精灵开的，但不清一色。
//!
//! # 三、职业是 `ClassDef`，不是一套平行类型
//!
//! `knowledge/design/settlements-structures-and-npc-spawning.md` 已经裁定
//! 「NPC 职业与玩家职业是同一个东西」。本模块因此只**查**
//! [`crate::class::ClassTable`]，一个新注册表都不建；「猎户 / 屠夫 / 农夫 /
//! 据点管理者 / 民兵 / 铁匠」这几条是**内容**
//! （`mods/lostland/classes.json5`），与「战士 / 法师 / 游侠 / 卫兵」走
//! 同一个注册通道。
//!
//! # 四、查不到就是查不到（ADR 0015）
//!
//! [`SettlementRoles`] 的每一项都是 `Option<ContentIndex>`：某条职业内容
//! 没被装载（第三方 mod 组合掉了本体、或本体内容文件被改坏）时，那一档
//! 不参与抽取，而不是 panic、也不是凭空 intern 一条出来。全部落空时名册
//! 里的人一律带 `ContentIndex::default()`（「尚无职业」的既有诚实表达，
//! 与 `ll_game::world::build_player_agent` 对玩家的处理一致）。
//!
//! # 五、确定性（C3 / C5）
//!
//! 随机全部来自 [`DetRng::for_entity`]，三元组是
//! `(world_seed, ROSTER_STREAM_ID, 据点 id × MAX_ROSTER + 名册序号)`——
//! 形状照抄已落地的 [`ll_world::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]。
//! 全模块不含任何 `HashMap`/`HashSet`，权重表是定长数组，遍历顺序由下标
//! 决定。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, NamespacedId, WorldId};
use ll_core::rng::DetRng;
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_sim::item::{ItemCatalog, equip_mask_of, outfit_from_inventory};
use ll_world::culture::{CultureKind, CultureTable};
use ll_world::entity::{Affiliation, AffiliationKind, Agent, BaseStats, Gender, OrgRef};
use ll_world::item::{EquipSlot, ItemStack};
use ll_world::resource::{ResourceCategory, ResourceKind, ResourceTable};
use ll_world::settlement::{SettlementSite, SettlementStatus};
use ll_world::space::{Space, ZoneCoord};

use crate::class::ClassTable;
use crate::race::{RaceTable, starting_inventory};
use crate::registry::Registry;

/// 名册派生所用的随机流编号——与据点建筑铺设
/// （[`ll_world::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]）、历史推演
/// （[`ll_world::chronicle::CHRONICLE_STREAM_ID`]）各自分开：改动名册
/// 抽法不会连带改掉房子怎么铺，反之亦然。
pub const ROSTER_STREAM_ID: u64 = 0x004E_5043_5F52_0001;

/// 名册**性别**抽取专用的流标识——与 [`ROSTER_STREAM_ID`] **必须**分开。
///
/// # 为什么不复用名册那条流
///
/// [`settlement_roster`] 的循环里那句注释写得很清楚：「抽取顺序（先种族
/// 后职业）本身是这条流的一部分，调换顺序会让同一颗种子产出另一份
/// 名册」。在那条流里再插一次抽取，等于把全世界每一座据点的**种族与
/// 职业**全部重抽——那是一次远超「加一个性别字段」的世界改动，
/// 战争结果、据点存亡、人口构成会跟着全变。
///
/// 单开一条流，既有名册逐位不变，只是每个人多了一个此前不存在的属性。
pub const ROSTER_GENDER_STREAM_ID: u64 = 0x004E_5043_5F47_0001;

/// 一座据点最多派生（因而最多物化）多少个 NPC。
///
/// # 为什么需要一个上界，取值为什么是这个
///
/// 实测三个种子共 788 座据点，人口中位数 31、**最大 175**
/// （见 [`ll_world::settlement`] 模块文档「实测」一节）。而 `Agent` 是
/// 厚层实体，其类型文档写明「数百个，有界」——把一座首邑的 175 个人全部
/// 物化，光一座据点就吃掉那个预算的一大半，玩家走过三四座就再没有余量
/// 留给怪物与随从。
///
/// 取 24：一座村子里同时住着二十来个活人，在传统 roguelike 的视野
/// （`BASE_SIGHT_RADIUS` 12 格）下已经是「一眼看不完」的量级；玩家把
/// 一片大陆上的四五座据点都逛遍，总量仍在一百出头，留得住余量。
///
/// **这个上界只截断物化，不篡改人口**：[`SettlementSite::population`]
/// 仍然是历史算出来的那个数，编年史、承载力、覆灭判定读的都还是它。名册
/// 是「这座村子里你能真的遇见谁」的那一层，不是人口普查。
pub const MAX_ROSTER: u32 = 24;

/// 「据点管理者」在名册里的固定序号——每座还有人住的据点恰好一位。
const STEWARD_INDEX: u32 = 0;

/// 每多少个居民配一名守卫（向下取整，另加固定的一名）。
///
/// 取 8：24 人的满员据点因此有 4 名守卫（1 + 24/8），12 人的小村有 2 名。
/// 比例本身没有更深的依据——它要满足的只是「守卫看得见但不至于比村民多」，
/// 与 [`MAX_ROSTER`] 一样属于手感取舍，不影响任何正确性。
const RESIDENTS_PER_GUARD: u32 = 8;

/// 资源画像第一名给对应档位的权重加成，见
/// [`SettlementRoles::commoner_weights`] 文档。
const PRIMARY_RESOURCE_BONUS: u32 = 9;

/// 资源画像第二名的加成——明显小于第一名，让「主业」与「副业」在名册
/// 上真的分得开。
const SECONDARY_RESOURCE_BONUS: u32 = 3;

/// 「普通居民」权重表有几档。
const COMMONER_SLOTS: usize = 8;

/// 农夫在 [`SettlementRoles::commoner_weights`] 权重表里的下标。
const FARMER_SLOT: usize = 0;
/// 猎户的下标。
const HUNTER_SLOT: usize = 1;
/// 铁匠的下标。
const BLACKSMITH_SLOT: usize = 4;
/// 渔夫的下标。
const FISHER_SLOT: usize = 5;
/// 牧羊人的下标。
const SHEPHERD_SLOT: usize = 6;
/// 石匠的下标。
const MASON_SLOT: usize = 7;

/// 建立者种族派生所用的随机流编号——与名册成员那条流
/// （[`ROSTER_STREAM_ID`]）分开。
///
/// **必须分开，不能挤进同一条流**：名册成员的流键是
/// `据点 id × MAX_ROSTER + 序号`，那个键空间已经被 `0..MAX_ROSTER` 占满，
/// 再往上加一个「建立者」槽位就会撞进下一座据点的 0 号成员——那正是
/// [`roster_rng`] 文档里那条「互不重叠」保证要防的事。
///
/// **本常量现在住在 `ll-world`**（[`ll_world::culture::FOUNDER_RACE_STREAM_ID`]），
/// 这里只是重新导出，取值一个字节没变。搬迁的理由见那一侧的文档：
/// 世界生成期的战争结算也要问「谁建的」，而它在 `ll-world` 里。
pub use ll_world::culture::FOUNDER_RACE_STREAM_ID;

/// 名册里每个居民**不是**建立者种族的概率（千分比）。
///
/// # 取值为什么是这个
///
/// 项目所有者的关切是「每个聚居地应该都不太一样吧？还有种族的分配
/// 什么的」。此前逐个居民独立抽种族的做法，后果是**处处混居、只是
/// 比例不同**——差异只存在于统计上，玩家走进去感受不到。
///
/// 200‰ 意味着一座 24 人的据点里大约 5 个外来者：
///
/// - 走进一座矮人矿城，二十个人里十九个是矮人的**那一眼**是有的
///   （建立者种族占八成）；
/// - 但那五个外来者让它不至于清一色——一座只有一个种族的城会让世界
///   变得死板，这是协调者与所有者都点名不要的。
///
/// 再高（比如 400‰）建立者种族就压不住场面，回到「只是比例不同」；
/// 再低（比如 50‰）二十人的村子里期望只有一个外来者，与清一色没有
/// 可察觉的差别。
pub const OUTSIDER_PERMILLE: u32 = 200;

/// [`OUTSIDER_PERMILLE`] 的分母。
const PERMILLE_SCALE: u32 = 1000;

/// 一条「可抽取的档位」：内容索引 + 权重。
///
/// 权重为 0 或索引为 `None` 的档位不参与抽取——前者是取值的选择，后者是
/// 「这条内容没装载」（ADR 0015，见模块文档四节）。
#[derive(Debug, Clone, Copy)]
struct WeightedSlot {
    content: Option<ContentIndex>,
    weight: u32,
}

/// 本模块按名字引用的那几条职业内容——同时也是
/// `mods/lostland/classes.json5` 必须注册哪几条的清单。
///
/// 抽成常量而不是把字符串散在 [`SettlementRoles::resolve`] 里，理由同
/// [`crate::class`] 的 `BASE_CLASS_IDS`：集成测试要按同一份清单核对内容
/// 真的注册了它们，两处各写一份字面量迟早会分叉。
pub const SETTLEMENT_CLASS_IDS: [&str; 10] = [
    "lostland:steward",
    "lostland:guard",
    "lostland:militia",
    "lostland:farmer",
    "lostland:hunter",
    "lostland:butcher",
    "lostland:blacksmith",
    "lostland:fisher",
    "lostland:shepherd",
    "lostland:mason",
];

/// 本模块按名字引用的那条资源内容（`mods/lostland/resources.json5`）。
///
/// # 只剩一条了，这正是资源两层分类要换来的东西
///
/// 此前这里是三条（良田/木材/铁矿），因为名册亲和表按**具体种类**挂
/// 规则，每一条对口职业都要在 Rust 里按名字认一种资源。资源分出大类
/// 之后，亲和改挂在**大类**上（食物→农夫、金属→铁匠、水→渔夫……），
/// 于是第三方 mod 写一条 `mymod:copper_vein`（metal）就自动有铁匠，
/// Rust 一个字都不用改。
///
/// 剩下的这一条是唯一一处**大类不够用**的地方：良田与牧场同属食物，
/// 却要分别抬农夫与牧羊人。按具体种类认它，是这张表存在的全部理由。
pub const SETTLEMENT_RESOURCE_IDS: [&str; 1] = ["lostland:pasture"];

/// 一条资源亲和规则：这处资源命中什么，就给哪一档加权重。
///
/// # 为什么是两个变体，不是一律按大类
///
/// 大类是常态：守着**金属**的据点该有铁匠，是铁是铜无所谓——这一条让
/// 第三方 mod 新增一种矿不需要改任何 Rust 代码就拿到对口职业，是资源
/// 分出大类这一层的全部价值兑现的地方。
///
/// 但大类不总够用：良田与牧场同属**食物**，却要分别抬农夫与牧羊人。
/// 硬要只用大类，就得把「食物」再拆一层，那等于把两层变三层，为一条
/// 例外重建整套分类。[`AffinityRule::Kind`] 是那条例外的最小表达，
/// 且它**优先于**大类规则（见 [`SettlementRoles::apply_affinity`]）。
///
/// `Copy` 且不含任何容器：规则表是调用点写死的定长切片。
#[derive(Debug, Clone, Copy)]
enum AffinityRule {
    /// 按具体种类命中——`None`（那条资源内容没装载）恒不命中。
    Kind(Option<ResourceKind>, usize),
    /// 按大类命中。
    Category(ResourceCategory, usize),
}

/// 本模块认得的那几条据点职业，以及它们各自与哪种资源相配。
///
/// 全部字段是 `Option`：本结构体由 [`Self::resolve`] 从注册表**查**出来，
/// 查不到就是 `None`（ADR 0015，见模块文档四节）。
#[derive(Debug, Clone)]
pub struct SettlementRoles {
    /// 据点管理者——每座据点恰好一位（名册序号 0）。
    pub steward: Option<ContentIndex>,
    /// 守卫（`lostland:guard`）——按 [`RESIDENTS_PER_GUARD`] 配额。
    ///
    /// **这一条是本模块存在之前就有的内容**，而且它此前是一条真实的
    /// 悬空引用：[`crate::native_behavior`] 的卫兵那棵树第一句就问
    /// 「这个实体是不是 `lostland:guard`」，而全仓库没有任何路径生成过
    /// 带这个职业的实体，那个分支因此恒为假。本模块是它第一次真的可能
    /// 成立的地方。
    pub guard: Option<ContentIndex>,
    /// 民兵——平时务农、战时拿起矛的那一类，无资源亲和。
    pub militia: Option<ContentIndex>,
    /// 农夫——与良田相配。
    pub farmer: Option<ContentIndex>,
    /// 猎户——与木材（林地）相配。
    pub hunter: Option<ContentIndex>,
    /// 屠夫——无资源亲和：屠夫跟着人走，不跟着地走。
    pub butcher: Option<ContentIndex>,
    /// 铁匠——与**金属**大类相配。
    pub blacksmith: Option<ContentIndex>,
    /// 渔夫——与**水**大类相配（淡水与渔场都算）。项目所有者点名。
    ///
    /// 这一条填的是上一批实测出来的最大那个空：242 座活据点里 116 座
    /// 的主资源是水源，而当时水源不改变职业分布，因为没有对口职业。
    pub fisher: Option<ContentIndex>,
    /// 牧羊人——与**牧场**这一条具体种类相配（不是整个食物大类，
    /// 见 [`SETTLEMENT_RESOURCE_IDS`] 文档）。项目所有者点名。
    pub shepherd: Option<ContentIndex>,
    /// 石匠——与**石材**大类相配。项目所有者裁定「加入石匠也没问题，
    /// 某些物品例如艺术品可以通过石头，例如某一些建材也是」。
    ///
    /// **如实标注**：那两样产出内容当前都不存在，见
    /// `mods/lostland/classes.json5` 里这条职业的注释。
    pub mason: Option<ContentIndex>,

    /// 牧场的资源索引，查不到时为 `None`（那一条按种类的亲和恒不成
    /// 立，牧场落回它所属的食物大类，因而抬农夫）。
    pasture: Option<ResourceKind>,
    /// 资源表快照——亲和规则要按大类分派，而
    /// [`SettlementSite::resource_profile`] 给的是具体种类，两者之间
    /// 差的正是这张表的 [`ResourceTable::category`] 查询。
    ///
    /// 快照（`Clone`）而不是借用，理由与
    /// [`crate::native_behavior::BehaviorRuleCatalogs`] 逐字相同：
    /// 内容表在装载完成后不再变化，而本结构体要被物化路径长期持有，
    /// 那条链路上没有任何一处能提供借用所需的生命周期。
    resources: ResourceTable,

    /// 文化表快照——建立者种族现在从**据点的文化**里抽
    /// （[`ll_world::culture::CultureAttrs::founder_races`]），不再走
    /// 本模块里写死的种族名单。
    ///
    /// # 这一步偿还的是一笔点过名的债
    ///
    /// 在此之前，本模块有一个 `SETTLEMENT_RACE_IDS: [&str; 3] =
    /// ["lostland:human", "lostland:dwarf", "lostland:elf"]` 和一份
    /// 写死的资源亲和（食物→人类、金属→矮人、木材→精灵）。后果是
    /// **第三方 mod 加一个种族拿不到任何选址亲和，一座据点都不会属于
    /// 它**——与同一个模块里职业亲和已经做到的「按大类挂规则、加一条
    /// JSON5 就有对口职业」形成刺眼的对照。现在种族这一侧也是纯内容：
    /// 加一条 `cultures.json5` 就有自己的据点。
    ///
    /// 资源对种族的影响没有消失，只是改走了一层：资源决定**文化**
    /// （`ll_world::chronicle` 的 `culture_weights`），文化决定
    /// **建立者种族**。「守着铁矿的地方是矮人开的城」这条因果链一节
    /// 未断，但中间那一环现在是可被内容替换的。
    ///
    /// 快照（`Clone`）而不是借用，理由同上面的 `resources`。
    cultures: CultureTable,
}

impl SettlementRoles {
    /// 从注册表解析出本模块要用的那几条内容。
    ///
    /// **只查，不注册**（与 [`crate::class::resolve_base_classes`] 同一条
    /// 纪律，也与 [`crate::native_behavior`] 内部那个 `lookup` 逐字同形）：
    /// 决策层不该凭空造出内容（ADR 0015）。查不到的那一条留 `None`，对应
    /// 的档位从此不参与抽取。
    ///
    /// `classes` 用来做一次「这个索引真的是一条职业吗」的确认：注册表
    /// 里存在同名标识符不等于它被定义成了职业（`ContentIndex` 是全局
    /// 号段，地形/物品/技能共用同一个 `Interner`）。
    pub fn resolve(
        registry: &Registry,
        classes: &ClassTable,
        resources: &ResourceTable,
        cultures: &CultureTable,
    ) -> Self {
        let class_of = |id: &str| -> Option<ContentIndex> {
            let index = lookup(registry, id)?;
            classes.is_defined(index).then_some(index)
        };
        let resource_of = |id: &str| -> Option<ResourceKind> {
            let index = lookup(registry, id)?;
            resources
                .is_defined(index)
                .then(|| ResourceKind::from_index(index))
        };
        SettlementRoles {
            steward: class_of(SETTLEMENT_CLASS_IDS[0]),
            guard: class_of(SETTLEMENT_CLASS_IDS[1]),
            militia: class_of(SETTLEMENT_CLASS_IDS[2]),
            farmer: class_of(SETTLEMENT_CLASS_IDS[3]),
            hunter: class_of(SETTLEMENT_CLASS_IDS[4]),
            butcher: class_of(SETTLEMENT_CLASS_IDS[5]),
            blacksmith: class_of(SETTLEMENT_CLASS_IDS[6]),
            fisher: class_of(SETTLEMENT_CLASS_IDS[7]),
            shepherd: class_of(SETTLEMENT_CLASS_IDS[8]),
            mason: class_of(SETTLEMENT_CLASS_IDS[9]),
            pasture: resource_of(SETTLEMENT_RESOURCE_IDS[0]),
            resources: resources.clone(),
            cultures: cultures.clone(),
        }
    }

    /// 「普通居民」那几档的权重表，已按这座据点的资源画像调整过。
    ///
    /// # 基础权重与资源加成怎么定
    ///
    /// 基础权重回答的是「一座什么资源都不突出的村子里，这几种人各占
    /// 多少」：农夫最多（谁都得吃饭）、猎户与渔夫次之（打猎捕鱼是两条
    /// 到处都有的副业）、民兵再次，屠夫/铁匠/牧羊人/石匠各一份（一座
    /// 村子有一个就够了）。
    ///
    /// 资源加成回答的是项目所有者要的那条：「守着铁矿的据点该有铁匠，
    /// 守着良田的该有农夫」。加成挂在
    /// [`SettlementSite::resource_profile`] 的两个名次上，第一名
    /// [`PRIMARY_RESOURCE_BONUS`]（9）、第二名
    /// [`SECONDARY_RESOURCE_BONUS`]（3）。
    ///
    /// # 亲和挂在**大类**上，只有一条例外
    ///
    /// 见 [`AffinityRule`]。挂大类换来的是「第三方 mod 加一种铜矿就
    /// 自动有铁匠」；唯一按具体种类认的是牧场（良田与牧场同属食物，
    /// 却要分别抬农夫与牧羊人）。
    ///
    /// # 水源那条留白已经补上了——实测数字在此
    ///
    /// 上一批如实标注过：水源当时不改变职业分布，因为没有对口职业，
    /// 而**近一半据点（116/242）的主资源正是水源**。渔夫这条职业加上
    /// 之后，水成了第五个大类、[`Self::fisher`] 是它的对口职业。
    ///
    /// 种子 20260826、本体默认布局，**235 座还有人住的据点、4422 人**
    /// （据点数与上一批的 242 不同：新增三种资源改变了选址与承载力）。
    /// 下表按第一名资源分组，只列各组占比最高的那条职业与对口职业：
    ///
    /// | 主资源（大类） | 据点数 | 名册人数 | 占比最高的职业 |
    /// |---|---|---|---|
    /// | 良田（食物） | 7 | 121 | **农夫 47.1%** |
    /// | 牧场（食物） | 3 | 41 | **牧羊人 31.7%**（农夫 26.8%） |
    /// | 木材（木材） | 95 | 1816 | **猎户 32.7%** |
    /// | 铁矿（金属） | 10 | 210 | **铁匠 25.2%** |
    /// | 花岗岩（石材） | 3 | 47 | **石匠 23.4%** |
    /// | 水源（水） | 104 | 1925 | **渔夫 36.0%** |
    /// | 渔场（水） | 13 | 262 | **渔夫 41.6%** |
    ///
    /// **七种资源各自都真的当过第一名，且每一组的头名恰好是它的对口
    /// 职业**——这是「大类 + 一条按种类的例外」这套规则真的在改变输出
    /// 的直接证据。水那两条合起来 117 座（**全大陆 49.8%**），正是上
    /// 一批那半张空白表的位置。
    ///
    /// 牧场与花岗岩各自只有 3 座：它们与同源地形上的对手（良田 / 铁矿）
    /// 排名贴得很近，谁当第一名由采样噪声决定，见
    /// `mods/lostland/resources.json5` 里那两条的注释——第一版给的数值
    /// 让它们**一座都排不上第一名**，是实测之后调回来的。
    fn commoner_weights(&self, site: &SettlementSite) -> [WeightedSlot; COMMONER_SLOTS] {
        let mut slots = [
            WeightedSlot {
                content: self.farmer,
                weight: 5,
            },
            WeightedSlot {
                content: self.hunter,
                weight: 3,
            },
            WeightedSlot {
                content: self.militia,
                weight: 2,
            },
            WeightedSlot {
                content: self.butcher,
                weight: 1,
            },
            WeightedSlot {
                content: self.blacksmith,
                weight: 1,
            },
            WeightedSlot {
                content: self.fisher,
                weight: 3,
            },
            WeightedSlot {
                content: self.shepherd,
                weight: 1,
            },
            WeightedSlot {
                content: self.mason,
                weight: 1,
            },
        ];
        self.apply_affinity(
            site,
            &mut slots,
            &[
                // 唯一一条按具体种类的规则，理由见 SETTLEMENT_RESOURCE_IDS。
                AffinityRule::Kind(self.pasture, SHEPHERD_SLOT),
                AffinityRule::Category(ResourceCategory::Food, FARMER_SLOT),
                AffinityRule::Category(ResourceCategory::Timber, HUNTER_SLOT),
                AffinityRule::Category(ResourceCategory::Metal, BLACKSMITH_SLOT),
                AffinityRule::Category(ResourceCategory::Stone, MASON_SLOT),
                AffinityRule::Category(ResourceCategory::Water, FISHER_SLOT),
            ],
        );
        slots
    }

    /// 这座据点的**建立者种族候选档位**——由它的**文化**给出，不再由
    /// 本模块的资源亲和给出。
    ///
    /// 文化本身是按「资源 + 地形 + 邻近据点 + 一点随机」抽出来的
    /// （`ll_world::chronicle` 的 `culture_weights`），因此「守着铁矿
    /// 的地方长出矿业文化、矿业文化由矮人建立」这条链没有断——断掉的
    /// 只是本模块里那份写死的三族名单，见 [`SettlementRoles::cultures`]
    /// 字段文档。
    ///
    /// 没有文化（一条文化内容都没装载、或者据点快照来自旧路径）时返回
    /// 空 `Vec`，[`pick`] 对空档位表返回 `None` 且**一个随机数都不取**
    /// ——退化路径不推进随机流。
    fn founder_slots(&self, site: &SettlementSite) -> Vec<WeightedSlot> {
        let Some(culture) = site.culture else {
            return Vec::new();
        };
        self.cultures
            .founder_races(culture)
            .iter()
            .map(|(race, weight)| WeightedSlot {
                content: Some(*race),
                weight: *weight,
            })
            .collect()
    }

    /// 把资源画像的两个名次折算成权重加成，写进 `slots`。
    ///
    /// 每处资源**至多命中一条规则**：先扫一遍按具体种类的规则，命中就
    /// 用它并且**不再看大类**（这正是「牧场抬牧羊人而不是抬农夫」的落
    /// 点）；一条都没命中才回落到按大类的规则。
    ///
    /// 规则表是调用点写死的定长切片，按下标顺序扫描，不涉及任何哈希
    /// 容器（约束 C5）。
    fn apply_affinity(
        &self,
        site: &SettlementSite,
        slots: &mut [WeightedSlot],
        rules: &[AffinityRule],
    ) {
        for (rank, entry) in site.resource_profile.iter().enumerate() {
            let Some(kind) = *entry else {
                continue;
            };
            let bonus = if rank == 0 {
                PRIMARY_RESOURCE_BONUS
            } else {
                SECONDARY_RESOURCE_BONUS
            };
            let category = self.resources.category(kind);
            let by_kind = rules.iter().find_map(|rule| match rule {
                AffinityRule::Kind(Some(wanted), slot) if *wanted == kind => Some(*slot),
                _ => None,
            });
            let target = by_kind.or_else(|| {
                rules.iter().find_map(|rule| match rule {
                    AffinityRule::Category(wanted, slot) if Some(*wanted) == category => {
                        Some(*slot)
                    }
                    _ => None,
                })
            });
            if let Some(slot) = target
                && let Some(entry) = slots.get_mut(slot)
            {
                entry.weight = entry.weight.saturating_add(bonus);
            }
        }
    }
}

/// 查一个已知字符串对应的内容索引；没注册就是 `None`（**不 intern**
/// ——ADR 0015，与 [`crate::native_behavior`] 内部那个同名帮手逐字同形）。
fn lookup(registry: &Registry, id: &str) -> Option<ContentIndex> {
    let parsed = NamespacedId::parse(id).ok()?;
    registry.get(&parsed)
}

/// 一个 NPC 的**派生身份**：由种子与据点完全确定，不进存档。
///
/// 这不是一个 [`Agent`]——它没有位置、没有血量、没有背包，那些是物化那
/// 一刻才产生的东西（[`build_npc_agent`]）。把两者分开，「未探索区域的
/// NPC」才谈得上「不需要实体化也存在」：一份 `NpcProfile` 随时可以由
/// [`settlement_roster`] 现算出来，回答「那座村子里有几个铁匠」这类问题
/// 不需要在世界状态里放任何东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcProfile {
    /// 他住在哪座据点——[`SettlementSite::id`]。
    ///
    /// 「有 NPC 的地方必然存在一个据点」这条裁定在类型上的表达：这个
    /// 字段没有 `Option`，本模块也没有任何不带据点就产出 `NpcProfile`
    /// 的入口。
    pub home: WorldId,
    /// 在这座据点名册里的序号（`0..MAX_ROSTER`）。与 `home` 一起构成
    /// 这个人在派生世界里的稳定身份。
    pub roster_index: u32,
    /// 种族，指向注册表；全部种族内容都没装载时为
    /// `ContentIndex::default()`。
    pub race: ContentIndex,
    /// 职业，指向 [`crate::class::ClassTable`]；对应的职业内容没装载时
    /// 为 `ContentIndex::default()`（「尚无职业」的既有诚实表达）。
    pub profession: ContentIndex,
    /// 性别。
    ///
    /// 与 `race`/`profession` 一样是**派生身份的一部分**（同一
    /// `(world_seed, 据点 id, 名册序号)` 恒得到同一个），因此长在这里
    /// 而不是 [`MaterializeContext`]——后者装的是「与哪一位无关」的
    /// 那一束（见 `MaterializeContext::culture` 字段文档）。
    ///
    /// 抽取走的是**独立的一条流**（[`ROSTER_GENDER_STREAM_ID`]），
    /// 理由见那个常量的文档：在名册那条流里插一次抽取会把全世界的
    /// 种族与职业重抽一遍。
    pub gender: Gender,
}

/// 派生一座据点的完整名册。
///
/// **纯函数**：同一 `(world_seed, site)` 恒产出逐位相同的结果，不读也
/// 不写任何世界状态。废墟（[`SettlementStatus::Ruined`]）与零人口据点
/// 产出空名册——「有 NPC 的地方必然存在一个（还有人住的）据点」。
///
/// 名册长度是 `min(site.population, MAX_ROSTER)`，见 [`MAX_ROSTER`] 文档
/// 「这个上界只截断物化，不篡改人口」一节。
pub fn settlement_roster(
    site: &SettlementSite,
    roles: &SettlementRoles,
    world_seed: u64,
) -> Vec<NpcProfile> {
    if site.status != SettlementStatus::Inhabited {
        return Vec::new();
    }
    let residents = site.population.min(MAX_ROSTER);
    if residents == 0 {
        return Vec::new();
    }
    let guards = 1 + residents / RESIDENTS_PER_GUARD;
    let commoners = roles.commoner_weights(site);
    let founder_races = roles.founder_slots(site);

    let founder = settlement_founder_race(site, roles, world_seed);

    let mut roster = Vec::with_capacity(residents as usize);
    for index in 0..residents {
        let mut rng = roster_rng(world_seed, site.id, index);
        // 抽取顺序（先种族后职业）本身是这条流的一部分：调换顺序会让
        // 同一颗种子产出另一份名册。改动这里等于改动世界，不是重构。
        //
        // 种族这一步不再是「每人独立按权重抽一次」——那样每座村子都
        // 混居、只是比例不同。现在是「默认随建立者，掷一次
        // OUTSIDER_PERMILLE 决定这个人是不是外来者」，外来者才去按权重
        // 抽。资源对种族的影响因此整个上移到了建立者那一次抽取上：
        // 铁矿抬的不再是「多几个矮人」，而是「这座城是矮人开的」。
        //
        // 外来者抽的是**同一份**权重表（这座据点的文化的
        // `founder_races`），不是「全世界所有种族」——一份只列了一个
        // 种族的文化因此产出清一色的名册。那是内容决定不是退化：一个
        // 哥布林营地里不该住着矮人。多族文化（本体六条里有五条）照样
        // 出得来少数派。
        let race = if rng.chance(OUTSIDER_PERMILLE, PERMILLE_SCALE) {
            pick(&founder_races, &mut rng).unwrap_or(founder)
        } else {
            founder
        };
        let profession = if index == STEWARD_INDEX {
            roles.steward
        } else if index <= guards {
            roles.guard
        } else {
            pick(&commoners, &mut rng)
        };
        roster.push(NpcProfile {
            home: site.id,
            roster_index: index,
            race,
            profession: profession.unwrap_or_default(),
            // **不从 `rng` 里取**——见 `ROSTER_GENDER_STREAM_ID` 文档。
            gender: roster_gender(world_seed, site.id, index),
        });
    }
    roster
}

/// 这座据点的**建立者种族**——同一 `(world_seed, site.id)` 恒产出同一
/// 个答案的纯函数。
///
/// # 为什么这件事要提到据点这一层
///
/// 项目所有者：「设计上每个聚居地应该都不太一样吧？还有种族的分配
/// 什么的」。此前种族是**逐个居民**按权重抽的，资源只是把权重挪一点
/// ——后果是全大陆的村子都混居，一座「矮人多一点」的村与一座「精灵多
/// 一点」的村，差别只在统计上，玩家走进去看不出来。
///
/// 建立者种族把资源的影响从「多几个矮人」变成「这座城是矮人开的」：
/// 抽一次，一整座据点的主体人口都随它（外来者比例见
/// [`OUTSIDER_PERMILLE`]）。
///
/// # 它仍然是一个函数，但候选名单换了来源
///
/// 本函数此前的文档记录过一条判断：「建立者种族之所以不是
/// `SettlementSite` 的字段，是因为 `ll-world` 既拿不到注册表、也没有
/// 一份候选种族名单可抽」。**那条判断的后半句已经不成立了**：
/// [`SettlementSite::culture`] 现在是 `ll-world` 侧的一个字段，而
/// 文化自带一份 [`ll_world::culture::CultureAttrs::founder_races`]
/// 候选名单——那份名单本身仍然是**内容**，只是走了
/// `TerrainTable`/`ResourceTable` 那条「类型在 `ll-world`、数据由
/// `ll-mod` 填、注入世界生成」的既有路，没有把注册表倒灌进 `ll-world`。
///
/// 前半句仍然成立，因此本函数保持是一个 `pub fn` 而不是又一个字段：
/// 「谁建的」是「信什么」的一个纯函数结果，两处都存等于两个真相源。
///
/// 据点没有文化、或文化的候选名单里一条种族都没装载时返回
/// `ContentIndex::default()`（「尚无种族」的既有诚实表达，ADR 0015）。
pub fn settlement_founder_race(
    site: &SettlementSite,
    roles: &SettlementRoles,
    world_seed: u64,
) -> ContentIndex {
    ll_world::culture::founder_race(&roles.cultures, site.culture, site.id, world_seed)
        .unwrap_or_default()
}

/// 名册第 `index` 号那一位专属的随机流（C3）。
///
/// 三元组的第三项是 `据点 id × MAX_ROSTER + 序号`——与
/// [`ll_world::settlement::stamp_settlement`] 为每栋建筑派生流时用的
/// `site.id × MAX_BUILDINGS + building` 逐字同形，保证同一座据点的不同
/// 人、不同据点的同一序号，都落在互不重叠的流上。
/// 名册第 `index` 位居民的性别——与 [`roster_rng`] 同一个坐标
/// （`据点 id × MAX_ROSTER + 名册序号`），但走
/// [`ROSTER_GENDER_STREAM_ID`] 这条**独立的流**。
///
/// 两条流用同一个坐标而不是同一条流上的相邻两次抽取，是刻意的：坐标
/// 是「这个人是谁」的稳定表达，流标识是「问的是哪件事」。这样往后再加
/// 一个派生属性时，同样只需再开一条流，既有属性一位不动。
fn roster_gender(world_seed: u64, site: WorldId, index: u32) -> Gender {
    Gender::deterministic(
        world_seed,
        ROSTER_GENDER_STREAM_ID,
        u64::from(site.get()) * u64::from(MAX_ROSTER) + u64::from(index),
    )
}

fn roster_rng(world_seed: u64, site: WorldId, index: u32) -> DetRng {
    DetRng::for_entity(
        world_seed,
        ROSTER_STREAM_ID,
        u64::from(site.get()) * u64::from(MAX_ROSTER) + u64::from(index),
    )
}

/// 按权重抽一档。全部档位都不可用（索引为 `None` 或权重为 0）时返回
/// `None`，**并且一个随机数都不取**——空抽取不该悄悄推进随机流。
fn pick(slots: &[WeightedSlot], rng: &mut DetRng) -> Option<ContentIndex> {
    let total: u64 = slots
        .iter()
        .filter(|slot| slot.content.is_some())
        .map(|slot| u64::from(slot.weight))
        .sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.gen_range(total);
    for slot in slots {
        let Some(content) = slot.content else {
            continue;
        };
        let weight = u64::from(slot.weight);
        if roll < weight {
            return Some(content);
        }
        roll -= weight;
    }
    // 理论不可达：`roll < total` 而循环恰好减掉了全部权重之和。退回第一
    // 个可用档位而不是 panic（规格 §10.2「降级而非崩溃」）。
    slots.iter().find_map(|slot| slot.content)
}

/// 物化一个 NPC 需要的、与「哪一位」无关的那一组输入。
///
/// 打包成结构体而不是继续往参数表上加，理由同
/// [`ll_world::settlement::StampContext`]：这几项恒一起出现，散着传只会
/// 让调用点更容易漏配，也会撞上 `clippy::too_many_arguments`。
pub struct MaterializeContext<'a> {
    /// 种族表——出生携带物品与属性修正都从这里查。
    pub races: &'a RaceTable,
    /// 物品目录——出生装备定耐久初值、以及穿戴决策查装备掩码。
    pub items: &'a dyn ItemCatalog,
    /// 地表空间层属性索引，写进 [`Agent::current_space`]。
    pub surface_profile: ContentIndex,
    /// 这一刻的世界时刻：`spawned_at` 与 `next_action_at` 都取它。
    pub now: Tick,
    /// 这批 NPC 所属据点的文化（[`SettlementSite::culture`]），`None`
    /// 表示这座据点没有文化——空文化表的世界，或者内容里一条文化都
    /// 没装载。
    ///
    /// 与本结构体其余各项同一条纪律：它「与哪一位无关」——物化按据点
    /// 成批进行，同一批人共用同一份文化，因此它属于这一束而不是
    /// [`NpcProfile`]。
    ///
    /// `None` 时 [`build_npc_agent`] **不挂**文化归属，判定侧回退到
    /// 「无文化」（`ll_sim::ai_query::declared_hostile`）——与 ADR 0015
    /// 「尚无内容就诚实表达尚无内容」一致，不伪造一条指向某个默认文化
    /// 的归属。
    pub culture: Option<CultureKind>,
}

/// 一个 NPC 对**自己出生的那份文化**的声望，千分比
/// （[`Affiliation::standing`]）。
///
/// 取满值 1000（= 1.0，完全认同）而不是 0：0 在千分比语义下是「毫无
/// 认同」，与「生在这个文化里、说这套话、盖这种房」自相矛盾。也不取一
/// 个居中的折中值——那需要一条「为什么是 0.5 不是 0.6」的内容依据，而
/// 本批次没有任何机制读得出这个差别（[`ll_sim::ai_query::is_hostile`]
/// 与文化敌意判据都不读 `standing`）。凭空造一个中间数只会让后来人以为
/// 它是调过的。
///
/// 真正让它离开满值的机制——个体经历、叛出、改宗——属于 P8 的声望矩阵
/// （`knowledge/design/society-and-affiliation.md`），届时这个常量是
/// 「出生时的初值」而不是「恒定值」。
pub const NATIVE_CULTURE_STANDING: i32 = 1000;

/// 把一份派生身份物化成一个真正被模拟的 [`Agent`]。
///
/// # 属性：与玩家角色同一条烘焙路径
///
/// `stats` 走 [`ll_sim::character::bake_race_stat_modifiers`]，与
/// `ll_game::world::build_player_agent` 是同一个函数——NPC 与玩家在数值
/// 上不是两套东西（`knowledge/design/race-system.md`「二、属性修正」的
/// 烘焙语义对两者一视同仁）。
///
/// # 文化归属：[`ll_world::entity::Agent::affiliations`] 的第一个生产者
///
/// 在此之前**两条 `Agent` 构造路径都写死 `Vec::new()`**，那个字段自
/// 落地起零生产者。本函数给 NPC 挂上一条指向所属据点文化的
/// [`AffiliationKind::Culture`] 归属，声望取
/// [`NATIVE_CULTURE_STANDING`]。
///
/// [`MaterializeContext::culture`] 为 `None`（据点没有文化）时**不挂**
/// ——不写一条指向「无文化」哨兵的归属。哨兵是**查询期回退**，不是写进
/// 每个实体的数据：项目所有者裁定玩家不挂归属，若这里反过来给每个 NPC
/// 写一条，同一件事就有了两种表示法，还会让每个 NPC 白白多背一条进存
/// 档与世界哈希的记录。将来要让某个具体 NPC **显式**无文化，给它写一条
/// 指向哨兵的归属即可，判定结果与不挂完全一致，不产生歧义。
///
/// **这会改变经物化路径产生的世界的摘要**：`Affiliation::standing` 进
/// `ll_world::state::WorldState::hash`。
///
/// # 装备：NPC 自行决策（项目所有者裁定）
///
/// > 「这个如果是 NPC 就是根据 NPC 自行决策，人的话就等玩家自己装备吧」
///
/// 玩家那一半已经落地（出生装备只进背包）。NPC 这一半的落点是
/// [`outfit_decision`]——它在 [`ll_sim::item::outfit_from_inventory`]
/// **之上**加了一层「这个 NPC 会挑哪件穿」，而不是把背包里能穿的一股脑
/// 全套上。
pub fn build_npc_agent(
    profile: &NpcProfile,
    pos: TorusPos,
    zone: ZoneCoord,
    roles: &SettlementRoles,
    ctx: &MaterializeContext<'_>,
) -> Agent {
    let carried = ctx
        .races
        .get(profile.race)
        .map(|view| starting_inventory(&view, ctx.items))
        .unwrap_or_default();
    let (equipment, inventory) = outfit_decision(profile, roles, carried, ctx.items);
    let stats =
        ll_sim::character::bake_race_stat_modifiers(BaseStats::BASELINE, profile.race, ctx.races);
    Agent {
        pos,
        stats,
        // 与玩家同一条纪律（见 `ll_game::world::spawn_player` 对
        // `next_action_at` 的注释）：取当前世界时钟而不是 `Tick(0)`，
        // 否则这个 NPC 一进时间轴就会把世界时钟倒拨回午夜。
        next_action_at: ctx.now,
        health: Agent::STARTING_HEALTH,
        // 性别直接从派生身份搬运，不在这里再抽一次——`NpcProfile` 才是
        // 「这个人是谁」的真相源，本函数只负责把它物化成 `Agent`。
        gender: profile.gender,
        // 见本函数文档「文化归属」一节：据点有文化就挂一条，没有就
        // 留空、由判定侧回退到「无文化」。
        affiliations: ctx
            .culture
            .map(|culture| Affiliation {
                kind: AffiliationKind::Culture,
                org: OrgRef::Def(culture.index()),
                standing: NATIVE_CULTURE_STANDING,
            })
            .into_iter()
            .collect(),
        wallet: 0,
        profession: profile.profession,
        goals: Vec::new(),
        race: profile.race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory,
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ctx.surface_profile),
        mod_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: ctx.now,
        remembered_id: None,
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
        // **把 `NpcProfile.home` 搬进实体**——本字段唯一的非 `None`
        // 生产者。`NpcProfile` 是「这个人是谁」的真相源，本函数只负责
        // 把它物化成 `Agent`（与上面 `gender` 那一行逐字同理）。
        //
        // 消费者是对话的「加入据点」：玩家跟这座据点的管理者说话时，
        // `ll_sim::resolve` 读的正是**说话人**的这个字段，见
        // `ll_world::entity::Agent::home` 文档。
        home: Some(profile.home),
    }
}

/// 「这个 NPC 会挑哪件穿」——项目所有者裁定的那一层决策，架在
/// [`ll_sim::item::outfit_from_inventory`] 之上。
///
/// 返回 `(装备栏, 留在背包里的)`，与被它包住的那个函数同一个形状。
///
/// # 规则：拿武器的只有拿武器的职业
///
/// - **武装职业**（守卫、民兵）：能穿的全部穿上——他们的工作就是站在
///   那里让人看见自己带着家伙。
/// - **其余职业**：占用手部槽位（[`EquipSlot::MAIN_HAND`] /
///   [`EquipSlot::OFF_HAND`]）的东西**留在背包**，其余照穿。一个农夫
///   身上有件外衣是常态，举着一把剑站在田里不是。
///
/// # 为什么规则只有这一条
///
/// 因为再多一条就需要 `ItemDef` 上当前不存在的信息。「铁匠该穿皮围裙」
/// 要物品带得出「这是围裙」这个语义——本仓库的物品今天只有堆叠上限、
/// 装备掩码、属性加成、耐久四样，没有任何一样答得上来。按掩码分「手上
/// 拿的 vs 身上穿的」是**用现有数据真答得出来的最强判断**，再往下就是
/// 编造。
///
/// # 一处如实标注：本体内容当前测不出这条规则的差别
///
/// 本体三族的 `starting_items`（`mods/lostland/races.json5`）是亚麻衬衫、
/// 羊毛手套、骨针这类东西，**没有一件占用手部槽位**。因此在本体默认内容
/// 下，两条分支产出的结果恰好相同。这不是规则没落地——是内容还没给它
/// 可分辨的输入；本模块的单元测试
/// `非武装职业把手部装备留在背包而武装职业穿上` 用一份合成物品目录把
/// 两条分支各走一遍，守住这条规则本身。
fn outfit_decision(
    profile: &NpcProfile,
    roles: &SettlementRoles,
    carried: Vec<ItemStack>,
    items: &dyn ItemCatalog,
) -> (BTreeMap<EquipSlot, ItemStack>, Vec<ItemStack>) {
    let profession = Some(profile.profession);
    let armed = (roles.guard.is_some() && profession == roles.guard)
        || (roles.militia.is_some() && profession == roles.militia);
    if armed {
        return outfit_from_inventory(carried, items);
    }
    let hands = EquipSlot::MAIN_HAND
        .mask()
        .union(EquipSlot::OFF_HAND.mask());
    let mut wearable = Vec::new();
    let mut stowed = Vec::new();
    for stack in carried {
        if equip_mask_of(stack.def, items).intersects(hands) {
            stowed.push(stack);
        } else {
            wearable.push(stack);
        }
    }
    let (equipment, mut rest) = outfit_from_inventory(wearable, items);
    rest.append(&mut stowed);
    (equipment, rest)
}

#[cfg(test)]
mod tests {
    use ll_core::ident::Interner;
    use ll_core::torus::TorusSize;
    use ll_sim::item::ItemRule;
    use ll_world::culture::CultureKind;
    use ll_world::item::SlotMask;
    use ll_world::settlement::SITE_RESOURCE_SLOTS;
    use ll_world::zone::ZoneLayout;

    use ll_sim::combat::Penetration;
    use ll_world::entity::AttributeKind;
    use ll_world::item::WearChannels;

    use crate::class::ClassAttrs;

    use super::*;

    /// 本单元测试里现造的那批资源：id 与大类，下标即
    /// [`sample_roles`] 返回的那个数组的下标。
    ///
    /// 用真实的本体 id 字符串（`SettlementRoles::resolve` 按名字认牧场
    /// 那一条），但由测试自己注册——本 crate 的单元测试不装载本体内容
    /// 文件，那是 `crates/ll-mod/tests/` 的集成测试的事。
    const SAMPLE_RESOURCES: [(&str, ResourceCategory); 6] = [
        ("lostland:farmland", ResourceCategory::Food),
        ("lostland:pasture", ResourceCategory::Food),
        ("lostland:timber", ResourceCategory::Timber),
        ("lostland:iron_vein", ResourceCategory::Metal),
        ("lostland:granite", ResourceCategory::Stone),
        ("lostland:fresh_water", ResourceCategory::Water),
    ];

    /// 一张现造的、与本体内容无关的角色扮演表：十条职业、三个种族、
    /// 六种资源（见 [`SAMPLE_RESOURCES`]）。
    /// 夹具文化名册：三条，各自的**主**建立者种族不同，但每条都留了
    /// 少数派档位——外来者比例（[`OUTSIDER_PERMILLE`]）抽的正是同一份
    /// 权重表，一条只有单一种族的文化会产出清一色的名册（本体的
    /// `lostland:goblin_warband` 就是那样，且那是刻意的）。
    const SAMPLE_CULTURES: [(&str, [(&str, u32); 3]); 3] = [
        (
            "test:farmfolk",
            [
                ("lostland:human", 8),
                ("lostland:dwarf", 2),
                ("lostland:elf", 2),
            ],
        ),
        (
            "test:minefolk",
            [
                ("lostland:dwarf", 8),
                ("lostland:human", 3),
                ("lostland:elf", 1),
            ],
        ),
        (
            "test:woodfolk",
            [
                ("lostland:elf", 8),
                ("lostland:human", 3),
                ("lostland:dwarf", 1),
            ],
        ),
    ];

    fn sample_roles() -> (
        SettlementRoles,
        Registry,
        [ResourceKind; 6],
        [CultureKind; 3],
    ) {
        let mut registry = Registry::new();
        let mut classes = ClassTable::new();
        for id in SETTLEMENT_CLASS_IDS {
            let index = registry.intern(NamespacedId::parse(id).expect("合法标识符"));
            classes
                .define(
                    index,
                    ClassAttrs {
                        display_name_key: NamespacedId::parse("lostland:x").expect("合法标识符"),
                        primary_attribute: AttributeKind::Strength,
                        traits: Vec::new(),
                    },
                )
                .expect("首次定义");
        }

        let mut table = ResourceTable::new();
        let mut kinds = [ResourceKind::from_index(ContentIndex::default()); 6];
        for (slot, (id, category)) in SAMPLE_RESOURCES.iter().enumerate() {
            let index = registry.intern(NamespacedId::parse(id).expect("合法标识符"));
            table
                .define(
                    index,
                    ll_world::resource::ResourceAttrs {
                        display_name_key: NamespacedId::parse("lostland:x").expect("合法标识符"),
                        category: *category,
                        source_terrain: ll_world::terrain::TerrainKind::from_index(
                            ContentIndex::default(),
                        ),
                        abundance: 100,
                        residents_supported: 1,
                        settlement_draw: 1,
                        exhaustible: false,
                    },
                )
                .expect("首次定义");
            kinds[slot] = ResourceKind::from_index(index);
        }
        let mut cultures = ll_world::culture::CultureTable::new();
        let mut culture_kinds = [CultureKind::from_index(ContentIndex::default()); 3];
        for (slot, (culture_id, founders)) in SAMPLE_CULTURES.iter().enumerate() {
            let founder_races = founders
                .iter()
                .map(|(race_id, weight)| {
                    (
                        registry.intern(NamespacedId::parse(race_id).expect("合法标识符")),
                        *weight,
                    )
                })
                .collect::<Vec<_>>();
            let index = registry.intern(NamespacedId::parse(culture_id).expect("合法标识符"));
            cultures
                .define(
                    index,
                    ll_world::culture::CultureAttrs {
                        display_name_key: NamespacedId::parse("lostland:x").expect("合法标识符"),
                        economy: ResourceCategory::Food,
                        home_terrain: ll_world::terrain::TerrainKind::from_index(
                            ContentIndex::default(),
                        ),
                        wall_terrain: ll_world::terrain::TerrainKind::from_index(
                            ContentIndex::default(),
                        ),
                        founder_races,
                        hostility: Vec::new(),
                        buildings: ll_world::building::bare_building_fixture(),
                    },
                )
                .expect("首次定义");
            culture_kinds[slot] = CultureKind::from_index(index);
        }
        let roles = SettlementRoles::resolve(&registry, &classes, &table, &cultures);
        (roles, registry, kinds, culture_kinds)
    }

    /// 一座人口 `population`、资源画像为 `profile` 的据点。
    fn site(
        population: u32,
        profile: [Option<ResourceKind>; SITE_RESOURCE_SLOTS],
    ) -> SettlementSite {
        let layout = ZoneLayout::new(48, TorusSize::new(2, 2).expect("2x2 合法"))
            .expect("48 满足全部对齐与跨度约束");
        let size = layout.tile_size();
        let mut counter = 3u32;
        SettlementSite {
            id: WorldId::next(&mut counter),
            zone: layout.tile_to_zone(size.wrap(10, 10)).0,
            anchor: size.wrap(10, 10),
            status: SettlementStatus::Inhabited,
            founded_epoch: 0,
            abandoned_epoch: None,
            population,
            peak_population: population,
            building_count: 1 + population / 4,
            resource_profile: profile,
            culture: None,
        }
    }

    /// 与 [`site`] 相同，但带上一份文化——建立者种族现在由文化给出，
    /// 因此凡是关心种族的用例都要走这一条。
    fn site_with_culture(
        population: u32,
        profile: [Option<ResourceKind>; SITE_RESOURCE_SLOTS],
        culture: CultureKind,
    ) -> SettlementSite {
        SettlementSite {
            culture: Some(culture),
            ..site(population, profile)
        }
    }

    #[test]
    fn 同一颗种子同一座据点派生出逐位相同的名册() {
        // Arrange
        let (roles, _registry, _resources, _cultures) = sample_roles();
        let site = site(20, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let first = settlement_roster(&site, &roles, 0xABCD_1234);
        let second = settlement_roster(&site, &roles, 0xABCD_1234);

        // Assert
        assert_eq!(first, second);
        assert_eq!(first.len(), 20);
    }

    #[test]
    fn 换一颗种子名册就不同() {
        // Arrange
        let (roles, _registry, _resources, _cultures) = sample_roles();
        let site = site(24, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let first = settlement_roster(&site, &roles, 1);
        let second = settlement_roster(&site, &roles, 2);

        // Assert
        assert_ne!(first, second);
    }

    #[test]
    fn 废墟派生出空名册() {
        // Arrange
        let (roles, _registry, _resources, _cultures) = sample_roles();
        let mut ruin = site(0, [None; SITE_RESOURCE_SLOTS]);
        ruin.status = SettlementStatus::Ruined;
        ruin.peak_population = 90;

        // Act
        let roster = settlement_roster(&ruin, &roles, 7);

        // Assert
        assert!(roster.is_empty());
    }

    #[test]
    fn 名册长度被max_roster截断而人口本身不变() {
        // Arrange
        let (roles, _registry, _resources, _cultures) = sample_roles();
        let big = site(175, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let roster = settlement_roster(&big, &roles, 11);

        // Assert
        assert_eq!(roster.len(), MAX_ROSTER as usize);
        assert_eq!(big.population, 175);
    }

    #[test]
    fn 每座还有人住的据点恰好一位据点管理者() {
        // Arrange
        let (roles, _registry, _resources, _cultures) = sample_roles();
        let village = site(15, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let roster = settlement_roster(&village, &roles, 5);
        let stewards = roster
            .iter()
            .filter(|npc| Some(npc.profession) == roles.steward)
            .count();

        // Assert
        assert_eq!(stewards, 1);
        assert_eq!(
            roster[0].profession,
            roles.steward.expect("夹具注册了管理者")
        );
    }

    #[test]
    fn 守着铁矿的据点铁匠比守着良田的多() {
        // Arrange
        let (roles, _registry, resources, _cultures) = sample_roles();
        let farmland = resources[0];
        let iron = resources[3];
        let mining = site(24, [Some(iron), None]);
        let farming = site(24, [Some(farmland), None]);

        // Act：同一颗种子、同一个人口，唯一的差别是资源画像。
        let mining_smiths = count_of(&settlement_roster(&mining, &roles, 99), roles.blacksmith);
        let farming_smiths = count_of(&settlement_roster(&farming, &roles, 99), roles.blacksmith);
        let mining_farmers = count_of(&settlement_roster(&mining, &roles, 99), roles.farmer);
        let farming_farmers = count_of(&settlement_roster(&farming, &roles, 99), roles.farmer);

        // Assert
        assert!(
            mining_smiths > farming_smiths,
            "矿城的铁匠 {mining_smiths} 应多于农业村的 {farming_smiths}"
        );
        assert!(
            farming_farmers > mining_farmers,
            "农业村的农夫 {farming_farmers} 应多于矿城的 {mining_farmers}"
        );
    }

    #[test]
    fn 守着水的据点渔夫比守着良田的多() {
        // Arrange：**这一条守的是资源大类那一层真的通了**——水这个大类
        // 是本批次新加的，渔夫是它的对口职业，而上一批实测「近一半据点
        // 的主资源是水源」时它们都还不存在。
        let (roles, _registry, resources, _cultures) = sample_roles();
        let farmland = resources[0];
        let water = resources[5];
        let riverside = site(24, [Some(water), None]);
        let farming = site(24, [Some(farmland), None]);

        // Act
        let riverside_fishers = count_of(&settlement_roster(&riverside, &roles, 77), roles.fisher);
        let farming_fishers = count_of(&settlement_roster(&farming, &roles, 77), roles.fisher);

        // Assert
        assert!(
            riverside_fishers > farming_fishers,
            "临水据点的渔夫 {riverside_fishers} 应多于农业村的 {farming_fishers}"
        );
    }

    #[test]
    fn 牧场抬的是牧羊人而不是同属食物大类的农夫() {
        // Arrange：**这一条守的是「按具体种类的规则优先于按大类的规则」**
        // ——牧场与良田同属食物大类，若那条优先级断了，牧场就会去抬农夫，
        // 牧羊人永远抽不出来。
        let (roles, _registry, resources, _cultures) = sample_roles();
        let farmland = resources[0];
        let pasture = resources[1];
        let grazing = site(24, [Some(pasture), None]);
        let farming = site(24, [Some(farmland), None]);

        // Act
        let grazing_shepherds = count_of(&settlement_roster(&grazing, &roles, 55), roles.shepherd);
        let farming_shepherds = count_of(&settlement_roster(&farming, &roles, 55), roles.shepherd);
        let grazing_farmers = count_of(&settlement_roster(&grazing, &roles, 55), roles.farmer);
        let farming_farmers = count_of(&settlement_roster(&farming, &roles, 55), roles.farmer);

        // Assert
        assert!(
            grazing_shepherds > farming_shepherds,
            "牧场据点的牧羊人 {grazing_shepherds} 应多于良田据点的 {farming_shepherds}"
        );
        assert!(
            farming_farmers > grazing_farmers,
            "良田据点的农夫 {farming_farmers} 应多于牧场据点的 {grazing_farmers}             （牧场的加成不该落到农夫头上）"
        );
    }

    #[test]
    fn 守着石材的据点石匠比无资源的据点多() {
        // Arrange：石材大类此前一条内容行都没有（因此石匠也没有对口
        // 资源）。花岗岩是它的第一条，本条守住那条 `Category(Stone)`
        // 规则真的接上了。
        let (roles, _registry, resources, _cultures) = sample_roles();
        let granite = resources[4];
        let quarry = site(24, [Some(granite), None]);
        let plain = site(24, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let quarry_masons = count_of(&settlement_roster(&quarry, &roles, 31), roles.mason);
        let plain_masons = count_of(&settlement_roster(&plain, &roles, 31), roles.mason);

        // Assert
        assert!(
            quarry_masons > plain_masons,
            "石场据点的石匠 {quarry_masons} 应多于无资源据点的 {plain_masons}"
        );
    }

    #[test]
    fn 建立者种族是种子与据点的纯函数且名册以它为主() {
        // Arrange
        let (roles, _registry, resources, cultures) = sample_roles();
        let iron = resources[3];
        // 建立者种族现在由**文化**给出，因此这条用例必须走带文化的
        // 那个夹具——`site` 造出来的据点 `culture: None`，那是「一条
        // 文化内容都没装载」那条退化路径，另有用例守着。
        let mining = site_with_culture(24, [Some(iron), None], cultures[1]);

        // Act
        let founder_a = settlement_founder_race(&mining, &roles, 4242);
        let founder_b = settlement_founder_race(&mining, &roles, 4242);
        let roster = settlement_roster(&mining, &roles, 4242);
        let same_as_founder = roster.iter().filter(|npc| npc.race == founder_a).count();

        // Assert
        assert_eq!(
            founder_a, founder_b,
            "同一 (种子, 据点) 必须恒产出同一个建立者种族"
        );
        assert!(
            same_as_founder * 2 > roster.len(),
            "名册应当以建立者种族为主，实际 {same_as_founder}/{}",
            roster.len()
        );
        assert!(
            same_as_founder < roster.len(),
            "名册不该清一色——外来少数族裔必须存在，实际 {same_as_founder}/{}",
            roster.len()
        );
    }

    #[test]
    fn 没有文化的据点名册仍然产出但种族是占位索引() {
        // Arrange：一条文化都没装载（或据点快照来自空文化表的世界）。
        let (roles, _registry, _resources, _cultures) = sample_roles();
        let site = site(12, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let founder = settlement_founder_race(&site, &roles, 99);
        let roster = settlement_roster(&site, &roles, 99);

        // Assert：降级而非崩溃（规格 §10.2）——名册照样有人，只是
        // 种族退回「尚无种族」的占位索引（ADR 0015）。
        assert_eq!(founder, ContentIndex::default());
        assert_eq!(roster.len(), 12);
        assert!(roster.iter().all(|npc| npc.race == ContentIndex::default()));
    }

    #[test]
    fn 单一建立者种族的文化产出清一色的名册() {
        // Arrange：本体的 lostland:goblin_warband 就是这个形状——一个
        // 哥布林营地里不该住着矮人，这是内容决定，不是退化。
        let mut registry = Registry::new();
        let goblin = registry.intern(NamespacedId::parse("test:goblin").expect("合法标识符"));
        let index = registry.intern(NamespacedId::parse("test:warband").expect("合法标识符"));
        let mut cultures = ll_world::culture::CultureTable::new();
        cultures
            .define(
                index,
                ll_world::culture::CultureAttrs {
                    display_name_key: NamespacedId::parse("test:x").expect("合法标识符"),
                    economy: ResourceCategory::Timber,
                    home_terrain: ll_world::terrain::TerrainKind::from_index(
                        ContentIndex::default(),
                    ),
                    wall_terrain: ll_world::terrain::TerrainKind::from_index(
                        ContentIndex::default(),
                    ),
                    founder_races: vec![(goblin, 10)],
                    hostility: Vec::new(),
                    buildings: ll_world::building::bare_building_fixture(),
                },
            )
            .expect("首次定义");
        let roles = SettlementRoles::resolve(
            &registry,
            &ClassTable::new(),
            &ResourceTable::new(),
            &cultures,
        );
        let camp = site_with_culture(
            20,
            [None; SITE_RESOURCE_SLOTS],
            CultureKind::from_index(index),
        );

        // Act
        let roster = settlement_roster(&camp, &roles, 77);

        // Assert
        assert_eq!(roster.len(), 20);
        assert!(roster.iter().all(|npc| npc.race == goblin));
    }

    fn count_of(roster: &[NpcProfile], class: Option<ContentIndex>) -> usize {
        roster
            .iter()
            .filter(|npc| class.is_some() && Some(npc.profession) == class)
            .count()
    }

    #[test]
    fn 一条职业内容都没装载时名册仍然产出但职业是占位索引() {
        // Arrange：空注册表 + 空职业表——第三方 mod 组合掉本体的情形。
        let registry = Registry::new();
        let classes = ClassTable::new();
        let roles = SettlementRoles::resolve(
            &registry,
            &classes,
            &ResourceTable::new(),
            &CultureTable::new(),
        );
        let village = site(6, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let roster = settlement_roster(&village, &roles, 3);

        // Assert
        assert_eq!(roster.len(), 6);
        assert!(
            roster
                .iter()
                .all(|npc| npc.profession == ContentIndex::default())
        );
    }

    /// 一份只回答装备掩码的合成物品目录。
    struct MaskCatalog(BTreeMap<ContentIndex, SlotMask>);

    impl ItemCatalog for MaskCatalog {
        fn item(&self, item: ContentIndex) -> Option<ItemRule> {
            self.0.get(&item).map(|mask| ItemRule {
                stack_limit: 1,
                equip_mask: *mask,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                max_durability: None,
                wear_channels: WearChannels::default(),
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                blind_box_pool: Vec::new(),
                furniture: false,
                taught_recipes: Vec::new(),
            })
        }
    }

    #[test]
    fn 非武装职业把手部装备留在背包而武装职业穿上() {
        // Arrange
        let (roles, _registry, _resources, _cultures) = sample_roles();
        let mut interner = Interner::new();
        let sword = interner.intern(NamespacedId::parse("testmod:sword").expect("合法标识符"));
        let shirt = interner.intern(NamespacedId::parse("testmod:shirt").expect("合法标识符"));
        let mut masks = BTreeMap::new();
        masks.insert(sword, EquipSlot::MAIN_HAND.mask());
        masks.insert(shirt, EquipSlot::BODY.mask());
        let catalog = MaskCatalog(masks);
        let carried = vec![ItemStack::new(sword, 1), ItemStack::new(shirt, 1)];
        let mut counter = 1u32;
        let home = WorldId::next(&mut counter);
        let farmer = NpcProfile {
            // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
            gender: ll_world::entity::Gender::default(),
            home,
            roster_index: 4,
            race: ContentIndex::default(),
            profession: roles.farmer.expect("夹具注册了农夫"),
        };
        let guard = NpcProfile {
            profession: roles.guard.expect("夹具注册了守卫"),
            ..farmer
        };

        // Act
        let (farmer_worn, farmer_packed) =
            outfit_decision(&farmer, &roles, carried.clone(), &catalog);
        let (guard_worn, _guard_packed) = outfit_decision(&guard, &roles, carried, &catalog);

        // Assert
        assert!(
            !farmer_worn.contains_key(&EquipSlot::MAIN_HAND),
            "农夫不该举着剑"
        );
        assert!(farmer_worn.contains_key(&EquipSlot::BODY), "衣服照穿");
        assert!(
            farmer_packed.iter().any(|stack| stack.def == sword),
            "剑应当留在背包里"
        );
        assert!(
            guard_worn.contains_key(&EquipSlot::MAIN_HAND),
            "守卫该带着家伙"
        );
    }
}
