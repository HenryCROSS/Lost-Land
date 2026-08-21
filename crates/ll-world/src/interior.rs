//! `Interior` 存储与锚点：地下城/建筑内部的有界局部空间集合。
//!
//! 落地设计文档六节「稀疏性：拆成两条」与四节「锚定关系：单一真相源」。
//!
//! # 单一真相源：`anchor` 只存在 `Interior` 自己身上
//!
//! `anchor: TorusPos`（这个空间在世界地图上显示为哪一格）是**唯一真相
//! 源**。「这一格有哪些入口」的反向索引（[`InteriorTable::entries_at`]）
//! 是**派生视图，永不权威**——本模块的类型设计上体现这条纪律：反向
//! 索引没有独立的 `set`/`insert` 方法，[`InteriorTable`] 只维护
//! `interiors: HashMap<SpaceId, Interior>` 这一份权威数据，
//! `entries_at` 每次调用都从它现算，不缓存、不可能与权威数据不同步。
//! 本项目已经为「同一个概念被独立定义两次」的缺陷付过两次代价（白昼
//! 判定 ADR 0010、`identity-and-ids.md` 的 `Affiliation.org`），这里是
//! 第三处需要提前避免的地方。
//!
//! # 反向索引的重建时机：从不重建，因为它从不被存储
//!
//! 「反向索引什么时候需要重建」这个问题在这里没有答案，因为它根本
//! 不存在需要重建的时刻——[`InteriorTable::entries_at`] 不缓存任何
//! 东西，每次调用都是对 `interiors` 的一次全量线性扫描 + 排序。这不是
//! 回避问题：一份「按需现算、从不缓存」的视图不可能过期，也就没有
//! 「何时重建」这个问题——增删 `Interior`（[`InteriorTable::insert`]）
//! 或读档（反序列化）之后，下一次 `entries_at` 调用看到的都是当时最新
//! 的权威数据，天然正确。若未来查询频率高到需要一份缓存（例如渲染
//! 每帧都要问「附近有没有入口」），那份缓存的更新规则必须单向——
//! `Interior.anchor` 变了就重建对应的索引条目，索引本身永远不能被
//! 独立编辑（设计文档四节原话）——但**这不是本任务的范围**：当前批次
//! 只交付「现算,不缓存」这一个版本，因为量级（几百到几千个 `Interior`
//! 实例，见设计文档六节）下线性扫描完全够用，加一层缓存只是在没有
//! 性能压力的地方提前引入一个需要维护「不能独立编辑」这条纪律的
//! 结构，属于投机性设计（YAGNI）。
//!
//! # 与共享常驻预算的关系（关键设计判断 3）
//!
//! 设计文档五节「常驻集合的构成」把「当前所在建筑/地下城的全部层」
//! 算进与 `Surface` 共享的同一个 256 上限；`crate::surface_store` 的
//! `RecencyClock<K>` 特意做成对键类型泛型，正是为了让这份预算将来能
//! 被两边共用（见其模块文档）。**本任务不做这份接线**：`Interior` 的
//! `floors` 一旦插入就无条件常驻，不参与任何淘汰——这与设计文档「当前
//! 所在建筑/地下城的全部层一旦进入就应整体常驻」（五节「常驻集合的
//! 构成」第 2 条）在效果上是一致的（尚未接线之前，「无条件常驻」是
//! 「优先常驻、不被挤掉」的一个正确子集）。真正的「与 `SurfaceStore`
//! 共享同一份 256 计数、当前空间之外的旧 `Interior` 也可能被挤掉」需要
//! 知道「玩家当前在哪个 `Space`」这类会话上下文——`InteriorTable` 本身
//! 不持有这个上下文（它甚至不知道自己被哪个 `WorldState` 拥有），这项
//! 接线要等到 `Surface`/`Interior` 被真正组合进 `WorldState`（任务 11
//! 及之后）才有地方落笔，本任务只保证共享预算需要的机件（`RecencyClock`
//! 的泛型设计）已经就绪。
//!
//! # 任务 11/12–15 的后续裁定：楼层本身仍然不参与淘汰，是明确的、暂
//! 不打算修的技术债
//!
//! 任务 11 落地了共享预算的**记账**那一半——`WorldState::insert_interior`/
//! `enter_interior`/`exit_interior` 会按已加载楼层数收缩 `Surface` 的
//! 有效常驻上限（[`crate::state::WorldState::recompute_shared_cap`]），
//! 保证两边合计不超过共享的 256。**但楼层本身依然「插入后无条件常驻，
//! 永不淘汰」**——批次 E（任务 12–15）评审过这一点，裁定维持现状，
//! 不在本批次修：淘汰一个 `Interior` 楼层若没有生成器能在需要时重新
//! 造出同一层，就等于永久丢失玩法数据（地板上的物品、玩家改动过的
//! 布局……），是不可接受的行为，不是「性能换正确性」的正常取舍。真正
//! 能安全淘汰楼层需要以下两者之一，两者都不在本批次范围：
//!
//! 1. **一个真正的 `Interior` 生成器**，淘汰后能按需重新造出同一层
//!    （或至少造出一个玩法上可接受的替代）——这是设计文档明确列在
//!    「有意留给后续阶段的缺口」里的 P7 工作，本批次不代为实现。
//! 2. **淘汰前把楼层序列化写盘、需要时再读回**——一种缓存到磁盘的
//!    差量存储策略，性质上更接近 P5 冻结存档格式时要解决的问题（如何
//!    存、存多少、何时失效），提前在本批次实现会绑定一套还没有定案的
//!    存档格式细节。
//!
//! 在两者之一落地前，「无条件常驻」是唯一安全的选择——本批次的验收
//! demo（`ll-sim/examples/p5_coordinate_acceptance`）只放了一个
//! `Interior`（远小于 256 的预算），这条限制在当前阶段不构成实际压力。

use std::collections::HashMap;

use ll_core::ident::ContentIndex;
use ll_core::torus::TorusPos;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::bounded_grid::BoundedGrid;
use crate::space::SpaceId;

/// 生成器类型标签——决定某个 [`Interior`] 该用哪一套生成算法重算
/// （ADR 0024，裁定 P5-7）。
///
/// 三个变体直接复用 [`Interior`] 类型文档已经写明的三种空间实例
/// （「地下城、洞窟或建筑内部」），不是凭空新造一套分类——本次只是给
/// 这三种已知类型各自留一个可以在存档里区分的标签，具体每种标签对应
/// 什么生成算法属于生成器本身的实现，不在本次范围（见 [`GeneratorParams`]
/// 文档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeneratorKind {
    /// 地下城。
    Dungeon,
    /// 洞窟。
    Cave,
    /// 建筑内部。
    Building,
}

/// `Interior` 生成来源的最小占位形状（ADR 0024，裁定 P5-7）。
///
/// # 为什么现在只有种子和类型标签
///
/// 本次只补「能不能重算」这一位信息的存档接口形状——真正的房间/地下城
/// 生成算法属于世界生成阶段（P7）的范围，不在本次任务内实现，与
/// `Interior::origin` 字段文档「为什么现在补，不等生成器落地时再补」
/// 一节是同一条最小改动纪律。字段以后会随生成器一起长大（例如加入
/// 房间数量、难度参数之类的具体控制项），当前的最小形状只保证「存
/// 起来、能序列化、能在未来被生成器读取」这几件事，不预先猜测生成器
/// 具体需要哪些参数（YAGNI）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorParams {
    /// 生成用的种子——与地表 [`crate::generate::GenParams::seed`] 是
    /// 同一种角色：同一个种子、同一套生成算法应当能重算出同一层内容。
    pub seed: u64,
    /// 生成器类型标签，决定用哪一套算法。
    pub kind: GeneratorKind,
}

/// 一个独立的离散空间实例：地下城、洞窟或建筑内部（设计文档六节）。
///
/// `id`/`anchor`/`profile`/`origin` 公开——这几个都是这个空间实例的
/// 身份/位置/生成来源，没有需要保护的不变式。`floors` 私有：只能通过
/// [`Self::set_floor`] 写入，避免调用方绕过本模块直接塞入与 `id` 不
/// 一致的楼层映射之类的误用（虽然 `floors` 本身也没有跨字段不变式，
/// 私有只是防御性的封装习惯，不是本类型真正的正确性保证来源）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interior {
    /// 这个空间实例的持久标识，复用 [`ll_core::ident::WorldId`]。
    pub id: SpaceId,
    /// 锚点：这个空间在世界地图上显示为哪一格——唯一真相源，见模块
    /// 文档。
    pub anchor: TorusPos,
    /// 指向 `crate::space_profile` 注册表的层属性。
    pub profile: ContentIndex,
    /// 生成来源：`None` 表示这个空间实例是玩家所建（或来源不可重算），
    /// 全靠偏差（`floors` 里已经生成/改动过的具体内容）常驻；`Some`
    /// 表示它原则上可以从这份 [`GeneratorParams`] 重新生成。
    ///
    /// # 为什么现在补，不等生成器落地时再补（ADR 0024，裁定 P5-7）
    ///
    /// ADR 0024 的判据：真正的分界不是「地表 vs 室内」，而是「有没有
    /// 生成器」——能重算的才能在常驻预算紧张时淘汰，不能重算的必须
    /// 常驻（见 `crate::interior` 模块文档「楼层本身仍然不参与淘汰」
    /// 一节，那正是「当前没有生成器，所以只能保守地全部当成不可重算」
    /// 的现状）。但存档格式冻结时这个字段不在里面——`Interior` 曾经只
    /// 有 `id`/`anchor`/`profile`/`floors`，即按「整层快照」表达空间，
    /// 不区分「可重算部分 + 偏差」。这不是无关紧要的疏漏：格式一旦
    /// 冻结成「整层快照」，将来真的做出生成器时也无法用它反推「这层
    /// 是不是当初由某个生成器造出来的」——没有这个字段，旧存档里的
    /// 每一层都只能被当成「玩家所建」（`origin = None`），永远无法安全
    /// 淘汰重算，即便生成器已经就绪。现在补只需要一次 schema 迁移
    /// （见 `ll_content::migration` 的迁移链框架，本次配套注册了一条
    /// 真实迁移函数），且当前**还没有任何真实存档**，是补这个字段成本
    /// 最低的时刻——过了这个窗口，每晚一天都会多一批只能视为
    /// `origin = None` 的存量存档。
    ///
    /// # 本次只补接口形状，不实现生成器
    ///
    /// 与「不做游戏循环接线」的探索记忆批次同一条最小改动纪律：本字段
    /// 只保证「能存、能序列化、能在未来被读取」，`Interior` 的插入点
    /// （[`crate::state::WorldState::insert_interior`]、[`Interior::new`]）
    /// 目前恒传 `None`——没有生成器就没有任何调用方能诚实地传出
    /// `Some`，见 [`Self::with_origin`] 文档。
    pub origin: Option<GeneratorParams>,
    /// 稀疏：一栋楼可能只有 `{0, 1, 2, -1}` 四个 floor（设计文档六节
    /// 「存在性稀疏」）。用 [`crate::bounded_grid::BoundedGrid`]（不是
    /// [`crate::chunk::ChunkGrid`]）——`Interior` 是有界不环绕的局部
    /// 地图,原因见 `bounded_grid` 模块文档。
    floors: HashMap<i16, BoundedGrid>,
}

impl Interior {
    /// 建立一个尚无任何楼层、来源不可重算（`origin = None`）的空间
    /// 实例。楼层由 [`Self::set_floor`] 按需加入——不要求构造时就补齐
    /// 全部楼层，这正是「稀疏」的含义。
    ///
    /// 现存全部调用方（世界生成/建造玩法的既有测试与验收 demo）都没有
    /// 真正的生成器可用，`origin = None` 是唯一诚实的选择——见
    /// [`Self::origin`] 字段文档「本次只补接口形状」一节。
    pub fn new(id: SpaceId, anchor: TorusPos, profile: ContentIndex) -> Self {
        Interior {
            id,
            anchor,
            profile,
            origin: None,
            floors: HashMap::new(),
        }
    }

    /// 建立一个来源可重算的空间实例：记录 `origin`，使得这个空间实例
    /// 理论上可以在未来由匹配的生成器从同一份参数重新造出来。
    ///
    /// 当前没有任何调用方（本次任务范围内不实现生成器，见
    /// [`Self::origin`] 字段文档），预留给 P7 世界生成器落地时使用。
    pub fn with_origin(
        id: SpaceId,
        anchor: TorusPos,
        profile: ContentIndex,
        origin: GeneratorParams,
    ) -> Self {
        Interior {
            id,
            anchor,
            profile,
            origin: Some(origin),
            floors: HashMap::new(),
        }
    }

    /// 插入或替换一层楼层地图。楼层号允许不连续（见模块文档），也
    /// 允许覆盖已存在的楼层（例如重新生成）。
    pub fn set_floor(&mut self, floor: i16, grid: BoundedGrid) {
        self.floors.insert(floor, grid);
    }

    /// 只读访问指定楼层，尚不存在时返回 `None`（不是错误——中间楼层
    /// 号本来就允许缺失，见模块文档「存在性稀疏」）。
    pub fn floor(&self, floor: i16) -> Option<&BoundedGrid> {
        self.floors.get(&floor)
    }

    /// 可写访问指定楼层。
    pub fn floor_mut(&mut self, floor: i16) -> Option<&mut BoundedGrid> {
        self.floors.get_mut(&floor)
    }

    /// 当前已生成的楼层号，按数值升序排列——不依赖 `HashMap` 的迭代
    /// 顺序（C5）：从 `HashMap` 收集成 `Vec` 后整体排序，最终顺序只由
    /// 楼层号本身决定。
    pub fn floor_numbers(&self) -> Vec<i16> {
        let mut floors: Vec<i16> = self.floors.keys().copied().collect();
        floors.sort();
        floors
    }

    /// 当前已生成的楼层数量。
    ///
    /// 供 [`InteriorTable::total_floor_count`] 汇总——不走
    /// [`Self::floor_numbers`]（那个方法要排序，这里只要计数，没必要
    /// 多做一次排序）。
    pub fn floor_count(&self) -> usize {
        self.floors.len()
    }
}

/// 全部 `Interior` 实例的权威集合：按 [`SpaceId`] 索引的稀疏表（设计
/// 文档六节：数量与聚落/建筑同量级，几百到几千）。
#[derive(Debug, Clone, Default)]
pub struct InteriorTable {
    /// 权威数据：按 `SpaceId` 索引。整个模块只有这一份存储——
    /// [`InteriorTable::entries_at`] 的反向索引从这里现算，见模块文档。
    interiors: HashMap<SpaceId, Interior>,
}

impl InteriorTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 插入一个空间实例，键取自 `interior.id`。已存在同一个 `id` 时
    /// 直接覆盖——`Interior` 不像 `TerrainDef`/`SpaceProfile` 那样有
    /// 「重复定义必须报错」的注册期校验纪律：`SpaceId` 来自
    /// `WorldId::next`，由世界生成/建造玩法的调用方保证唯一，不是
    /// mod 之间可能互相冲突的命名空间字符串,没有「两个 mod 声明了同一
    /// 个内容」这类场景。
    pub fn insert(&mut self, interior: Interior) {
        self.interiors.insert(interior.id, interior);
    }

    /// 按 `id` 查询，不存在返回 `None` 而非 panic——`SpaceId` 可能来自
    /// 一次已经失效的引用（例如被摧毁的地下城,或存档记录的历史事件
    /// 指向的空间），查询失败是正常路径，不是缺陷。
    pub fn get(&self, id: SpaceId) -> Option<&Interior> {
        self.interiors.get(&id)
    }

    /// 可写查询，同上但返回可变引用。
    pub fn get_mut(&mut self, id: SpaceId) -> Option<&mut Interior> {
        self.interiors.get_mut(&id)
    }

    /// 派生视图：现算，不缓存（见模块文档「反向索引的重建时机」）。
    /// 返回结果按 [`SpaceId`] 排序，不依赖内部 `HashMap` 迭代顺序
    /// （C5）。
    pub fn entries_at(&self, pos: TorusPos) -> Vec<SpaceId> {
        let mut ids: Vec<SpaceId> = self
            .interiors
            .values()
            .filter(|interior| interior.anchor == pos)
            .map(|interior| interior.id)
            .collect();
        ids.sort();
        ids
    }

    /// 可写遍历全部 `Interior` 实例，不带顺序保证——供存档读入后的
    /// `ContentIndex` 重映射（`ll-content` 任务 9）使用：每个 `Interior`
    /// 的 `profile` 字段各自独立重映射，互不依赖，重映射的正确性不受
    /// 遍历顺序影响（与 [`Self::total_floor_count`] 用 `values()` 求和
    /// 同一类安全用法，见其文档），因此不需要像 [`Self::entries_at`]
    /// 那样收集后再排序。
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Interior> {
        self.interiors.values_mut()
    }

    /// 全部 `Interior` 已加载楼层数之和——供
    /// [`crate::state::WorldState`] 计算 `Surface` 与 `Interior` 共享的
    /// 常驻预算（关键设计判断 3、裁定 CS-3）。不需要排序（只是求和，
    /// 不像 `entries_at` 那样要产出一份稳定顺序的结果），求和结果与
    /// `HashMap` 遍历顺序无关，不违反 C5。
    pub fn total_floor_count(&self) -> usize {
        self.interiors.values().map(Interior::floor_count).sum()
    }
}

/// [`InteriorTable`] 序列化用的扁平表示：把 `interiors`（`HashMap<SpaceId,
/// Interior>`）摊平成 `Vec<Interior>`。
///
/// # 为什么不直接对 `HashMap<SpaceId, Interior>` 派生序列化
///
/// `SpaceId` 是 [`ll_core::ident::WorldId`] 的类型别名，一个 newtype 元组结构体——虽然
/// 多数 serde 实现会把单字段 newtype 在 map key 位置「透明化」成内层
/// 的整数，但这依赖具体序列化格式的实现细节，不是 serde 数据模型本身
/// 保证的行为。为了不让 `InteriorTable` 的可序列化性悄悄绑定在某个
/// 格式的实现细节上,这里统一采用与 [`crate::surface_store::SurfaceStore`]
/// 同一个「摊平成 `Vec`」手法——`Interior` 自带 `id` 字段，`Vec<Interior>`
/// 已经带着重建 `HashMap` 需要的全部信息，不需要额外存一份 `(SpaceId,
/// Interior)` 元组。
#[derive(Serialize, Deserialize)]
struct InteriorTableData {
    interiors: Vec<Interior>,
}

impl Serialize for InteriorTable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut interiors: Vec<Interior> = self.interiors.values().cloned().collect();
        // 输出顺序不依赖 HashMap 迭代顺序：收集之后整体按 id 排序，
        // 最终顺序只由键值决定,与内部 HashMap 恰好按什么顺序吐出
        // 元素无关（C5 允许的安全用法，同 SurfaceStore::resident_zones）。
        interiors.sort_by_key(|interior| interior.id);
        InteriorTableData { interiors }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for InteriorTable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = InteriorTableData::deserialize(deserializer)?;
        let mut interiors = HashMap::new();
        for interior in data.interiors {
            let id = interior.id;
            if interiors.insert(id, interior).is_some() {
                return Err(D::Error::custom("存档中出现重复的 Interior id"));
            }
        }
        Ok(InteriorTable { interiors })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::base_terrain_fixture;
    use ll_core::bounded::BoundedSize;
    use ll_core::ident::{Interner, NamespacedId, WorldId};
    use ll_core::torus::TorusSize;

    fn profile_index() -> ContentIndex {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse("lostland:dungeon").expect("合法"))
    }

    fn anchor_at(x: i32, y: i32) -> TorusPos {
        TorusSize::new(48, 32)
            .expect("48x32 是合法的区块尺寸")
            .wrap(x, y)
    }

    #[test]
    fn 锚点相同的两个interior都能被反向查询找到() {
        // Arrange
        let mut counter = 0u32;
        let anchor = anchor_at(3, 5);
        let profile = profile_index();
        let mut table = InteriorTable::new();
        let dungeon = Interior::new(WorldId::next(&mut counter), anchor, profile);
        let cellar = Interior::new(WorldId::next(&mut counter), anchor, profile);
        table.insert(dungeon.clone());
        table.insert(cellar.clone());

        // Act
        let entries = table.entries_at(anchor);

        // Assert
        assert!(entries.contains(&dungeon.id) && entries.contains(&cellar.id));
    }

    #[test]
    fn 反向查询结果按spaceid排序多次调用顺序稳定() {
        // Arrange
        let mut counter = 0u32;
        let anchor = anchor_at(1, 1);
        let profile = profile_index();
        let mut table = InteriorTable::new();
        // 故意按「后分配的 id 先插入」的顺序插入,验证输出顺序与插入
        // 顺序、HashMap 迭代顺序都无关,只由 SpaceId 大小决定。
        let first = WorldId::next(&mut counter);
        let second = WorldId::next(&mut counter);
        table.insert(Interior::new(second, anchor, profile));
        table.insert(Interior::new(first, anchor, profile));

        // Act
        let entries = table.entries_at(anchor);

        // Assert
        assert_eq!(entries, vec![first, second]);
    }

    #[test]
    fn interior的楼层可以是不连续的整数集合() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let mut counter = 0u32;
        let mut interior = Interior::new(
            WorldId::next(&mut counter),
            anchor_at(0, 0),
            profile_index(),
        );
        let size = BoundedSize::new(10, 10).expect("10x10 是合法尺寸");
        interior.set_floor(0, BoundedGrid::new(size, ids.floor_stone));
        interior.set_floor(2, BoundedGrid::new(size, ids.floor_wood));
        interior.set_floor(-1, BoundedGrid::new(size, ids.floor_stone));

        // Act
        let floors = interior.floor_numbers();

        // Assert
        assert_eq!(floors, vec![-1, 0, 2]);
    }

    #[test]
    fn 全部interior已加载楼层数之和正确计数() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let mut counter = 0u32;
        let size = BoundedSize::new(4, 4).expect("4x4 是合法尺寸");
        let mut first = Interior::new(
            WorldId::next(&mut counter),
            anchor_at(0, 0),
            profile_index(),
        );
        first.set_floor(0, BoundedGrid::new(size, ids.floor_stone));
        first.set_floor(1, BoundedGrid::new(size, ids.floor_stone));
        let mut second = Interior::new(
            WorldId::next(&mut counter),
            anchor_at(1, 1),
            profile_index(),
        );
        second.set_floor(0, BoundedGrid::new(size, ids.floor_wood));
        let mut table = InteriorTable::new();
        table.insert(first);
        table.insert(second);

        // Act
        let total = table.total_floor_count();

        // Assert
        assert_eq!(total, 3);
    }

    #[test]
    fn 不存在的spaceid查询返回none而非panic() {
        // Arrange
        let mut counter = 0u32;
        let table = InteriorTable::new();
        let unknown = WorldId::next(&mut counter);

        // Act
        let result = table.get(unknown);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn interiortable经serde格式往返后反向查询结果不变() {
        // 满足硬性约束「SurfaceStore/Interior 从一开始就要求完整可
        // 序列化往返」。
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let mut counter = 0u32;
        let anchor = anchor_at(9, 9);
        let mut interior = Interior::new(WorldId::next(&mut counter), anchor, profile_index());
        let size = BoundedSize::new(6, 6).expect("6x6 是合法尺寸");
        interior.set_floor(0, BoundedGrid::new(size, ids.floor_stone));
        let mut table = InteriorTable::new();
        table.insert(interior.clone());
        let json = serde_json::to_string(&table).expect("InteriorTable 必然可序列化");

        // Act
        let decoded: InteriorTable = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded.entries_at(anchor), table.entries_at(anchor));
    }

    #[test]
    fn new构造的interior来源标记为不可重算() {
        // Arrange
        let mut counter = 0u32;

        // Act
        let interior = Interior::new(
            WorldId::next(&mut counter),
            anchor_at(0, 0),
            profile_index(),
        );

        // Assert
        assert!(interior.origin.is_none());
    }

    #[test]
    fn with_origin构造的interior保留传入的生成参数() {
        // Arrange
        let mut counter = 0u32;
        let params = GeneratorParams {
            seed: 7,
            kind: GeneratorKind::Dungeon,
        };

        // Act
        let interior = Interior::with_origin(
            WorldId::next(&mut counter),
            anchor_at(0, 0),
            profile_index(),
            params,
        );

        // Assert
        assert_eq!(interior.origin, Some(params));
    }

    #[test]
    fn interior经serde格式往返后origin字段保留() {
        // Arrange
        let mut counter = 0u32;
        let params = GeneratorParams {
            seed: 99,
            kind: GeneratorKind::Cave,
        };
        let interior = Interior::with_origin(
            WorldId::next(&mut counter),
            anchor_at(2, 2),
            profile_index(),
            params,
        );
        let json = serde_json::to_string(&interior).expect("Interior 必然可序列化");

        // Act
        let decoded: Interior = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded.origin, Some(params));
    }
}
