//! `SurfaceStore`：区块流式加载与常驻 LRU（设计文档五节「常驻集合与
//! LRU」）。
//!
//! # 本文件是全计划风险最高的一处（C4/C5 第一次真正有代码需要遵守）
//!
//! 后台推进必须能推到一个确定的 tick（C4），淘汰顺序、常驻集合快照
//! 顺序绝不能依赖 `HashMap`/`HashSet` 的迭代顺序（C5）——这两条此前
//! 「尚无代码可违反」，本文件是第一处需要正面回答它们的地方。
//!
//! **正面设计**：`resident: HashMap<ZoneCoord, ChunkGrid>` 只用于 O(1)
//! 查找（这是 C5 允许的安全用法），淘汰候选的选择**只**经过
//! [`RecencyClock`] 内部的 `BTreeSet<(Tick, K)>`——一个与哈希表完全
//! 独立的、按 `(最近访问 tick, 键)` 排序的结构。`ZoneCoord` 借
//! `ll_core::torus::TorusPos` 新增的 `Ord` 在并列 tick 时打破平局。
//! [`SurfaceStore::resident_zones`] 这类需要输出确定顺序的地方，改用
//! 「从 `HashMap` 收集成 `Vec` 后整体 `.sort()`」——排序后的最终顺序
//! 只由元素值决定，与收集时 `HashMap` 恰好按什么顺序吐出元素无关，这
//! 同样是 C5 允许的安全用法（`HashMap` 的迭代顺序本身从未参与任何
//! 判断，只是被排序步骤原样吞掉）。
//!
//! # `RecencyClock`：可复用的确定性淘汰时钟
//!
//! [`RecencyClock`] 是本文件的核心机件，特意设计成对键类型 `K`
//! 泛型——这是关键设计判断 3（`Surface` 与 `Interior` 共享同一个 256
//! 常驻上限）要求的落点：淘汰时钟本身不知道、也不需要知道 `K` 是区块
//! 坐标还是别的什么，未来若要把 `Interior` 楼层并入同一份预算，可以
//! 换一个键类型（例如 `(SpaceId, i16)`）复用这一个类型，不需要重新
//! 发明一套淘汰逻辑。见 [`RecencyClock`] 文档「与 `Interior` 的关系」
//! 一节，也见本 crate `interior` 模块文档「与共享预算的关系」一节。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use serde::de::Error as _;
use serde::{Deserialize, Serialize, Serializer};

use crate::chronicle::WorldChronicle;
use crate::chunk::ChunkGrid;
use crate::fov::SightGrid;
use crate::generate::generate_zone_window;
use crate::noise::TileableNoise;
use crate::space::ZoneCoord;
use crate::terrain::{BaseTerrainIds, TerrainError, TerrainKind, TerrainTable};
use crate::zone::ZoneLayout;

/// 通用的确定性淘汰时钟：按「最近访问 tick」排序，同一 tick 内按键
/// 本身的 `Ord` 打破平局——不依赖任何 `HashMap`/`HashSet` 迭代顺序
/// （C5）。
///
/// # 内部结构：为什么没有一张 `HashMap<K, Tick>` 做「最近访问时间」的
/// 反查缓存
///
/// 直觉上会想再维护一张 `last_access: HashMap<K, Tick>`，让「访问某个
/// 键时，先查它上次是什么时候访问的，好把旧的 `(tick, key)` 从
/// `order` 里删掉」这一步变成 O(1)。这里刻意没有加：那张表的内容
/// 100% 可以从 `order` 反推（每个仍在时钟里的键，`order` 中恰好有一条
/// 对应的 `(tick, key)`），加了就是又一份需要手动保持同步的派生数据
/// ——本项目已经为「同一个概念存在两份、彼此可能漂移」的缺陷付过代价
/// （ADR 0010 白昼判定、`identity-and-ids.md` 的 `Affiliation.org`），
/// 这里的量级（常驻上限默认 256）也完全谈不上「O(n) 扫描不可接受」，
/// 直接从 `order` 里线性找就够用，见 [`Self::last_tick_of`]。
///
/// # 为什么可以直接派生 `Serialize`/`Deserialize`
///
/// `order: BTreeSet<(Tick, K)>` 与 `pinned: HashSet<K>` 都序列化成
/// JSON **数组**（serde 对 `BTreeSet`/`HashSet` 走的是「序列」表示，不
/// 是「映射」表示），不会撞上「JSON 对象键必须是字符串」这条限制——
/// 这条限制只对真正的 `HashMap<K, V>` 生效，见 [`SurfaceStore`] 文档
/// 「为什么需要手写序列化」一节。反序列化直接落地这两个集合即可，
/// 不需要 `try_from` 中转：`order`/`pinned` 本身没有任何跨字段不变式
/// 需要在构造时校验。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecencyClock<K: Ord + Copy + Eq + Hash> {
    /// `(访问 tick, 键)` 排序集合，最小的条目就是最久未访问的那个。
    order: BTreeSet<(Tick, K)>,
    /// 钉住的键：不参与淘汰候选，即便是全场最久未访问的一个（裁定
    /// CS-3：当前空间的层与其锚点区块钉住不淘汰）。
    pinned: HashSet<K>,
}

impl<K: Ord + Copy + Eq + Hash> RecencyClock<K> {
    /// 建立空时钟。
    pub fn new() -> Self {
        RecencyClock {
            order: BTreeSet::new(),
            pinned: HashSet::new(),
        }
    }

    /// 该键当前记录的最近访问 tick，见类型文档「为什么没有反查缓存」。
    fn last_tick_of(&self, key: K) -> Option<Tick> {
        self.order
            .iter()
            .find(|(_, k)| *k == key)
            .map(|(tick, _)| *tick)
    }

    /// 记录一次访问：把 `key` 的最近访问时间更新为 `at_tick`。
    ///
    /// 若 `key` 已有记录，先移除旧条目——`BTreeSet` 不支持「原地改
    /// 键」，必须先删后插；否则同一个键会在 `order` 里留下两条记录，
    /// 旧的那条会在它本不该被淘汰的时候被当成候选。
    pub fn touch(&mut self, key: K, at_tick: Tick) {
        if let Some(old_tick) = self.last_tick_of(key) {
            self.order.remove(&(old_tick, key));
        }
        self.order.insert((at_tick, key));
    }

    /// 完全移除一个键的记录——淘汰某个键时调用，把它从时钟里彻底
    /// 忘掉（而不是留一条陈旧记录）。
    pub fn forget(&mut self, key: K) {
        if let Some(old_tick) = self.last_tick_of(key) {
            self.order.remove(&(old_tick, key));
        }
    }

    /// 钉住一个键：[`Self::evict_candidate`] 不会选中它，即便它是全场
    /// 最久未访问的一个。
    pub fn pin(&mut self, key: K) {
        self.pinned.insert(key);
    }

    /// 取消钉住。对未钉住的键调用是无操作。
    pub fn unpin(&mut self, key: K) {
        self.pinned.remove(&key);
    }

    /// 当前记录的键数量（不区分是否钉住）。
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// 时钟当前是否为空。
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// 找出最久未访问、且未被钉住的一个键，供调用方淘汰。
    ///
    /// 遍历 `order`（`BTreeSet`，按 `(Tick, K)` 升序，`K` 的 `Ord` 在
    /// 并列 tick 时打破平局）取第一个不在 `pinned` 里的条目——整个
    /// 决策过程不触碰任何 `HashMap`/`HashSet` 的迭代顺序（`pinned` 只
    /// 做 O(1) 成员测试，不被遍历，C5 允许的安全用法）。
    ///
    /// 若全部记录都被钉住（或时钟为空），返回 `None`——调用方此时无法
    /// 腾出空间，见 [`SurfaceStore::terrain_at`] 文档「淘汰失败时的
    /// 行为」一节。
    pub fn evict_candidate(&self) -> Option<K> {
        self.order
            .iter()
            .map(|(_, key)| *key)
            .find(|key| !self.pinned.contains(key))
    }
}

/// 区块流式加载的常驻存储：`terrain_at`/`set_terrain` 的形状与
/// [`ChunkGrid`] 保持一致（关键设计判断 1 的直接目的：任务 11 换型时,
/// 调用点的方法名不用改,只有构造方式变）。
///
/// # 为什么需要手写序列化
///
/// `resident: HashMap<ZoneCoord, ChunkGrid>` 不能直接派生 `Serialize`：
/// `ZoneCoord` 是 `TorusPos`，一个结构体；JSON 对象的键只能是字符串,
/// 而 serde 对结构体键的默认序列化会产出一个 JSON 对象而非字符串——
/// `serde_json` 在这种场景下会在运行时报错，不是编译期问题。
/// [`crate::chunk::ChunkGrid`] 自身的序列化实现（见 `crate::state`）
/// 已经用「摊平成 `Vec`」的手法解决过一次「内部结构不能直接派生」的
/// 问题（那次的原因是私有字段），这里是同一个手法的第二次应用，原因
/// 不同（这次是 map key 的类型限制）。见 [`SurfaceStoreData`]。
#[derive(Debug, Clone)]
pub struct SurfaceStore {
    layout: ZoneLayout,
    resident: HashMap<ZoneCoord, ChunkGrid>,
    clock: RecencyClock<ZoneCoord>,
    resident_cap: usize,
    /// 世界编年史——区块首次物化时，据点会跟着地形一起铺进去，见
    /// [`SurfaceStore::admit`]。
    ///
    /// # 为什么是 `Option`，为什么不参与序列化
    ///
    /// **不参与序列化**：编年史是种子的纯函数（ADR 0009「默认派生，
    /// 只存偏差」），与 `TileableNoise` 同一类运行期派生数据，读档后
    /// 由调用方重新派生并 [`SurfaceStore::attach_chronicle`] 装回来
    /// （`ll_game::rebuild_chronicle` 与既有的 `rebuild_noise` 是同一
    /// 条路）。手写的 [`SurfaceStoreData`] 里没有这个字段，Serialize
    /// 与 Deserialize 两侧因此天然对称——不会重演 `current_interior`
    /// 那次「只有一侧写、postcard 按声明顺序解码错位」的缺陷（见
    /// `crate::state::WorldState::current_interior` 字段文档）。
    ///
    /// **是 `Option`**：绝大多数调用方（单元测试、验收 demo、mod 集成
    /// 测试）不关心据点，`None` 表示「这个世界没有历史」，区块照常只
    /// 生成地形。这让新增本字段对既有全部 `SurfaceStore::new` 调用点
    /// 零改动，也让世界哈希的黄金基准不受影响。
    chronicle: Option<Arc<WorldChronicle>>,
}

impl SurfaceStore {
    /// 建立一个空的流式存储：尚未生成任何区块，`resident_cap` 是常驻
    /// 区块-层上限（设计文档五节默认 256，与关键设计判断 3 的「共享
    /// 预算」同一个数字，见模块文档）。
    pub fn new(layout: ZoneLayout, resident_cap: usize) -> Self {
        SurfaceStore {
            layout,
            resident: HashMap::new(),
            clock: RecencyClock::new(),
            resident_cap,
            chronicle: None,
        }
    }

    /// 装上一份世界编年史：**此后**才生成的区块会带上据点，已经常驻
    /// 的区块原样不动。
    ///
    /// 这是**读档**路径要的那一个：存档里的常驻区块早就带着据点（它们
    /// 是上次会话生成并存下来的），而且可能已经被玩家改过（拆了一堵
    /// 墙），绝不能重铺。需要连已常驻区块一起重铺的新游戏路径见
    /// [`Self::install_chronicle`]。
    pub fn attach_chronicle(&mut self, chronicle: Arc<WorldChronicle>) {
        self.chronicle = Some(chronicle);
    }

    /// 装上一份世界编年史，并把**已经常驻**的区块全部重新生成一遍，
    /// 让它们也带上据点。
    ///
    /// # 只应在新游戏构建期调用
    ///
    /// 「重新生成」意味着丢弃这些区块上的一切改写。`WorldState::new`
    /// 会先预热出生邻域的若干区块，而编年史此刻还没算出来（它需要的
    /// 噪声/地形表与 `WorldState::new` 是同一批输入，但构造顺序上排在
    /// 后面）——这几个区块是唯一需要补铺的。那一刻世界上还没有任何
    /// 玩家改动可丢，重新生成是安全的。读档路径请用
    /// [`Self::attach_chronicle`]。
    ///
    /// 重新生成而不是「在已有窗口上补铺一次」是刻意的：
    /// [`crate::settlement::stamp_settlement`] 会读地形判断哪块地能盖
    /// 房，对已铺过的窗口再铺一次不等价（见该函数文档「前置条件」）。
    /// 先回到干净的基线，再走与 [`Self::admit`] **完全同一条**铺设
    /// 路径，是让两条路径产出逐格相同结果的唯一省事办法。
    pub fn install_chronicle(
        &mut self,
        chronicle: Arc<WorldChronicle>,
        noise: &TileableNoise,
        params: &crate::generate::GenParams,
        terrain_ids: &BaseTerrainIds,
    ) {
        self.chronicle = Some(chronicle);
        for zone in self.resident_zones() {
            let grid = self.generate_and_stamp(noise, params, terrain_ids, zone);
            self.resident.insert(zone, grid);
        }
    }

    /// 生成一个区块窗口，并把**覆盖到**这个区块的据点（可能不止一座、
    /// 也可能是邻区块那座据点伸过来的半条街）各自落在本窗口内的那部分
    /// 铺进去——[`Self::admit`] 与 [`Self::install_chronicle`] 共用的
    /// 那一段，保证两条路径产出逐格相同的结果。
    ///
    /// # 惰性铺设：本方法一次也不往别的区块写
    ///
    /// 据点可以横跨区块（见 [`crate::settlement`] 模块文档）。跨出去的
    /// 那部分**不在这里补写到邻区块**——邻区块自己被物化时会走同一条
    /// 路，问出同一座据点、铺出自己那一半。写入全程走
    /// `ChunkGrid::set_terrain`（本方法刚生成、尚未插进 `resident` 的
    /// 那一份），从不经过 [`Self::set_terrain`]，因此后者「写未常驻
    /// 区块就 panic」的契约在这条路径上不生效。
    fn generate_and_stamp(
        &self,
        noise: &TileableNoise,
        params: &crate::generate::GenParams,
        terrain_ids: &BaseTerrainIds,
        zone: ZoneCoord,
    ) -> ChunkGrid {
        let mut grid = generate_zone_window(noise, params, &self.layout, zone, terrain_ids)
            .expect("ZoneLayout 构造时已校验区块边长满足 ChunkGrid 的最小视口跨度，生成不应失败");
        if let Some(chronicle) = &self.chronicle {
            // 「这块地能不能盖房」读的是基础地形（噪声的纯函数），不是
            // 本窗口——一栋跨区块的建筑必须在它覆盖到的每个区块里得出
            // 同一个答案，见 `crate::settlement::StampContext::base_terrain`。
            let base_terrain =
                |pos: TorusPos| crate::generate::terrain_at_tile(noise, params, pos, terrain_ids);
            let context = crate::settlement::StampContext {
                ids: terrain_ids,
                table: chronicle.terrain_table(),
                world_seed: params.seed,
                base_terrain: &base_terrain,
            };
            for site in chronicle.sites_touching_zone(zone) {
                crate::settlement::stamp_settlement(&mut grid, &self.layout, zone, site, &context);
            }
        }
        grid
    }

    /// 当前装着的世界编年史，未装则为 `None`——供「传说浏览」这类只读
    /// 消费方查询据点与历史事件，不需要自己再跑一遍推演。
    pub fn chronicle(&self) -> Option<&WorldChronicle> {
        self.chronicle.as_deref()
    }

    /// 当前装着的世界编年史的一份**共享句柄**（`Arc` 克隆），未装则为
    /// `None`。
    ///
    /// # 为什么与 [`Self::chronicle`] 并存，不是重复
    ///
    /// [`Self::chronicle`] 返回的引用借着 `self`，因而借着整个
    /// `WorldState::terrain`——想在读编年史的同时改 `WorldState::actors`
    /// （NPC 物化路径要做的正是这件事）就会撞上借用检查器。克隆一个
    /// `Arc` 是一次引用计数递增，编年史本身不复制，代价可忽略。
    ///
    /// 两个方法各自服务一种访问模式：只读查询（传说浏览）用前者，需要
    /// 同时改世界的用后者。
    pub fn chronicle_handle(&self) -> Option<Arc<WorldChronicle>> {
        self.chronicle.clone()
    }

    /// 本存储使用的区块布局。
    pub fn layout(&self) -> &ZoneLayout {
        &self.layout
    }

    /// 常驻上限。
    pub fn resident_cap(&self) -> usize {
        self.resident_cap
    }

    /// 给定区块坐标当前是否常驻。
    pub fn is_resident(&self, zone: ZoneCoord) -> bool {
        self.resident.contains_key(&zone)
    }

    /// 钉住一个区块：不会被 LRU 淘汰（裁定 CS-3：当前空间的锚点区块
    /// 钉住不淘汰）。调用方（未来的 `WorldState`，见任务 11）负责决定
    /// 「玩家当前在哪个空间」这类上下文——`SurfaceStore` 自身不感知
    /// `Space`，只提供钉住/取消钉住这两个原语。
    pub fn pin(&mut self, zone: ZoneCoord) {
        self.clock.pin(zone);
    }

    /// 取消钉住。
    pub fn unpin(&mut self, zone: ZoneCoord) {
        self.clock.unpin(zone);
    }

    /// 读取给定瓦片坐标的地形；若所属区块未常驻，按需生成并计入常驻
    /// 集合，超出上限时淘汰最久未访问的一个。这是流式加载的唯一入口。
    ///
    /// # 与任务 8 的关系
    ///
    /// 生成本身完全委托给 [`generate_zone_window`]（任务 8 产出的
    /// 窗口化生成入口）——本方法不重新实现任何生成逻辑，只负责「要不
    /// 要生成、生成完之后放不放得下」这两个流式加载特有的问题。
    ///
    /// # 淘汰失败时的行为
    ///
    /// 若 [`RecencyClock::evict_candidate`] 返回 `None`（全部常驻条目
    /// 都被钉住），本方法**允许暂时超出 `resident_cap`**，而不是
    /// panic 或拒绝生成——钉住是为了保护「当前空间」不被挤掉,不是为了
    /// 在极端情形下让玩家卡死在一个既加载不出新区块、又不能报错的
    /// 死角。这种情形只应在调用方钉住的区块数已经逼近或超过
    /// `resident_cap` 时出现，属于配置问题，不是本方法要掩盖的缺陷。
    pub fn terrain_at(
        &mut self,
        noise: &TileableNoise,
        params: &crate::generate::GenParams,
        terrain_ids: &BaseTerrainIds,
        pos: TorusPos,
        at_tick: Tick,
    ) -> TerrainKind {
        let (zone, local) = self.layout.tile_to_zone(pos);
        self.admit(noise, params, terrain_ids, zone, at_tick);
        self.resident
            .get(&zone)
            .expect("admit 保证该区块此刻已经常驻")
            .terrain_at(local)
    }

    /// 确保 `zone` 常驻：已常驻则只刷新访问时间；否则按需生成，生成
    /// 前先按 LRU 腾出空间。
    fn admit(
        &mut self,
        noise: &TileableNoise,
        params: &crate::generate::GenParams,
        terrain_ids: &BaseTerrainIds,
        zone: ZoneCoord,
        at_tick: Tick,
    ) {
        if self.resident.contains_key(&zone) {
            self.clock.touch(zone, at_tick);
            return;
        }

        while self.resident.len() >= self.resident_cap {
            match self.clock.evict_candidate() {
                Some(victim) => {
                    self.resident.remove(&victim);
                    self.clock.forget(victim);
                }
                // 全部条目都被钉住：允许暂时超出上限，见本方法调用方
                // SurfaceStore::terrain_at 文档「淘汰失败时的行为」。
                None => break,
            }
        }

        let grid = self.generate_and_stamp(noise, params, terrain_ids, zone);
        self.resident.insert(zone, grid);
        self.clock.touch(zone, at_tick);
    }

    /// 写入给定瓦片坐标的地形。前置条件：该坐标所属区块必须已经常驻。
    ///
    /// # 未常驻时的行为：panic
    ///
    /// 调用方只应该对当前正在模拟/渲染的区块调用——这类区块按定义已经
    /// 常驻（都经过 [`Self::terrain_at`] 或显式的常驻操作）。对未常驻
    /// 区块写入意味着调用方逻辑本身有问题（例如忘了先触发一次读取），
    /// **选择 panic 而非隐式加载**：隐式加载会让「写入」这个操作偷偷
    /// 触发一次生成（可能是较重的计算），且调用方无法区分「这次写入
    /// 命中了已加载的区块」与「这次写入顺带加载了一个新区块」，两种
    /// 情形的性能特征完全不同，静默合并只会让性能问题难以定位。
    pub fn set_terrain(&mut self, pos: TorusPos, kind: TerrainKind) {
        let (zone, local) = self.layout.tile_to_zone(pos);
        let grid = self.resident.get_mut(&zone).unwrap_or_else(|| {
            panic!("写入了尚未常驻的区块 {zone:?}——调用方只应对当前正在模拟/渲染的区块写入")
        });
        grid.set_terrain(local, kind);
    }

    /// 当前常驻的区块坐标集合，按 [`ZoneCoord`] 的 `Ord` 排序返回（供
    /// hash()/序列化使用），不暴露内部 `HashMap` 的原始迭代顺序（C5）。
    pub fn resident_zones(&self) -> Vec<ZoneCoord> {
        let mut zones: Vec<ZoneCoord> = self.resident.keys().copied().collect();
        zones.sort();
        zones
    }

    /// 只读查询：假定该坐标所属区块已经常驻，不触发生成，也不刷新
    /// 访问时间（这不是一次「访问」，只是查看当前是否已加载）。
    ///
    /// # 为什么未常驻时返回 `None` 而不是 panic
    ///
    /// 与 [`Self::set_terrain`] 的 panic 选择不同——写入未常驻区块几乎
    /// 总是调用方逻辑错误（见其文档）。这里不同：调用方（`ll-sim::resolve`，
    /// 见其模块文档「`resolve` 如何在流式加载的地形上保持纯函数」）
    /// 只能查询、不能触发生成（C1：`resolve` 必须是纯函数），在真正的
    /// 邻域缓冲维护接线之前（设计文档的任务 14），玩家移动到尚未预热
    /// 的区域是可能出现的正常路径，不是编程错误——panic 会让整个游戏
    /// 在这种情况下崩溃。调用方决定 `None` 时如何降级（`resolve` 目前
    /// 选择视为不可通行，见其文档）。
    pub fn terrain_at_resident(&self, pos: TorusPos) -> Option<TerrainKind> {
        let (zone, local) = self.layout.tile_to_zone(pos);
        self.resident.get(&zone).map(|grid| grid.terrain_at(local))
    }

    /// 把全部常驻区块里的每一格地形原地重映射——存档读入后的
    /// `ContentIndex` 重映射（`ll-content` 任务 9）需要：地形格里存的
    /// [`TerrainKind`] 内部就是一个 `ContentIndex`（见其定义），存档
    /// 写出时用的是当次会话的索引，读档后必须换成当前会话的索引，
    /// 否则某一格的地形会静默变成别的东西（与 `Agent::profession` 那类
    /// 字段是同一个问题，见 `ll-content::remap` 模块文档）。
    ///
    /// 遍历 [`Self::resident_zones`]（已排序）而不是内部 `HashMap` 的
    /// 原始迭代顺序（C5），逐格调用 `remap`；只有结果与原值不同才调用
    /// [`Self::set_terrain`] 写回，避免对每一格都触发一次不必要的写入。
    ///
    /// 泛型的错误类型 `E`，理由同 [`crate::entity::ThinPopulation::try_remap_content_indices`]
    /// ——本 crate 不猜测调用方想要什么错误类型，原样透传。
    pub fn try_remap_resident_terrain<E>(
        &mut self,
        mut remap: impl FnMut(TerrainKind) -> Result<TerrainKind, E>,
    ) -> Result<(), E> {
        let span = self.layout.zone_span() as i32;
        let size = self.layout.tile_size();
        for zone in self.resident_zones() {
            for ly in 0..span {
                for lx in 0..span {
                    let pos = size.wrap(zone.x() * span + lx, zone.y() * span + ly);
                    let kind = self
                        .terrain_at_resident(pos)
                        .expect("resident_zones() 返回的区块坐标此刻必然常驻");
                    let remapped = remap(kind)?;
                    if remapped != kind {
                        self.set_terrain(pos, remapped);
                    }
                }
            }
        }
        Ok(())
    }

    /// 调整常驻上限。不会立即淘汰现有条目——只影响下一次
    /// [`Self::terrain_at`] 判断「是否需要腾位置」时用的阈值，不主动
    /// 清退已经常驻的区块（调低上限本身不应该造成数据丢失的副作用）。
    ///
    /// 供 [`crate::state::WorldState`] 接线 `Surface` 与 `Interior`
    /// 共享的 256 常驻预算使用（关键设计判断 3、裁定 CS-3）——批次 C
    /// 完成时特意把这条接线留给了任务 11，见 [`crate::interior`] 模块
    /// 文档「与共享常驻预算的关系」一节。
    pub fn set_resident_cap(&mut self, cap: usize) {
        self.resident_cap = cap;
    }

    /// 校验当前**常驻**的全部区块——不像 [`TerrainTable::validate_grid`]
    /// 那样遍历整个世界（多数区块未常驻，压根没有具体地形数据可读），
    /// 只校验已经流式加载的部分。这与 [`crate::state::WorldState::hash`]
    /// 面对同一处架构变化时选择的做法一致（见其文档「不能再遍历整个
    /// 世界的每一格」）。
    pub fn validate_resident(&self, table: &TerrainTable) -> Result<(), TerrainError> {
        for zone in self.resident_zones() {
            let grid = self
                .resident
                .get(&zone)
                .expect("resident_zones 只返回 resident 中真实存在的键");
            table.validate_grid(grid)?;
        }
        Ok(())
    }

    /// 一次性预热布局里的**全部**区块——只应该用于小世界（测试、demo）
    /// 或明确需要「整张地图都可寻址」的场景，正常游玩路径应该用
    /// [`Self::terrain_at`] 按需流式加载，调用本方法会让流式加载失去
    /// 意义（把全部区块一次性生成出来，正是流式加载要避免的事）。
    ///
    /// 供本 crate 与下游的验收 demo 复用——demo 世界通常小到可以完整
    /// 常驻（远小于 `resident_cap`），且 demo 里出生点搜索、FOV 等
    /// 逻辑此前假定「任意坐标都能直接查询」，迁移到 [`SurfaceStore`]
    /// 后若不预热全部区块，这些查询会撞见「尚未常驻」而需要额外处理
    /// 一条在生产环境里才有意义的分支。
    pub fn warm_all(
        &mut self,
        noise: &TileableNoise,
        params: &crate::generate::GenParams,
        terrain_ids: &BaseTerrainIds,
        at_tick: Tick,
    ) {
        let zone_count = self.layout.zone_count();
        let span = self.layout.zone_span() as i32;
        let tile_size = self.layout.tile_size();
        for zy in 0..zone_count.height() as i32 {
            for zx in 0..zone_count.width() as i32 {
                let pos = tile_size.wrap(zx * span, zy * span);
                self.terrain_at(noise, params, terrain_ids, pos, at_tick);
            }
        }
    }

    /// 维护以 `pos` 所在区块为中心、`radius`（区块为单位）的一圈常驻
    /// 邻域——流式滚动在生产环境下真正需要的接线（设计文档任务 14，
    /// 也是 [`SurfaceWindow`] 文档「前置条件与任务 14 的关系」点名要
    /// 解决的那一步）。
    ///
    /// # 为什么调用方（渲染主循环）必须在查询视野之前调用这个方法
    ///
    /// [`SurfaceWindow`] 假定视野半径覆盖的全部坐标都已经常驻，未常驻
    /// 时直接 panic——那不是缺陷，是刻意的「过渡桥梁」设计（见其文档）：
    /// 真正保证前提成立的责任被明确甩给了调用这个方法的一方。渲染主
    /// 循环应该在**每次玩家跨越区块边界后**（或更保守地，每帧）调用
    /// 一次 `stream_neighborhood`，用一个覆盖「视野半径 + 一点余量」的
    /// `radius`，让相邻区块在相机/FOV 真正需要用到它们之前就已经生成
    /// 好——这样玩家看到的只是「地表连续无缝」,不会撞见 `SurfaceWindow`
    /// 的 panic，也不会在跨越边界的瞬间卡顿（生成发生在移动之前，不是
    /// 移动的同一帧）。
    ///
    /// # 与 [`Self::terrain_at`] 的关系：只是循环调用同一个入口
    ///
    /// 这个方法不实现任何新的生成/淘汰逻辑——它只是对邻域内每个区块的
    /// 代表坐标各调用一次 [`Self::terrain_at`]（流式加载唯一入口），
    /// 复用同一套 LRU 淘汰与确定性纪律（C4/C5）。已经常驻的区块只会被
    /// `touch` 刷新访问时间，不会重复生成——这正是 `crate::state` 模块
    /// 内 `warm_spawn_neighborhood`（模块私有函数，`WorldState::new` 的
    /// 出生点预热，不能做成文档内链）内部改为直接调用本方法的原因：
    /// 两处「预热一圈邻域」是同一个操作，不该维护两份几乎相同的双重
    /// 循环。
    pub fn stream_neighborhood(
        &mut self,
        noise: &TileableNoise,
        params: &crate::generate::GenParams,
        terrain_ids: &BaseTerrainIds,
        pos: TorusPos,
        radius: i32,
        at_tick: Tick,
    ) {
        let (center, _) = self.layout.tile_to_zone(pos);
        let span = self.layout.zone_span() as i32;
        let zone_count = self.layout.zone_count();
        let tile_size = self.layout.tile_size();
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let zone = zone_count.wrap(center.x() + dx, center.y() + dy);
                let tile = tile_size.wrap(zone.x() * span, zone.y() * span);
                self.terrain_at(noise, params, terrain_ids, tile, at_tick);
            }
        }
    }
}

/// 只读适配器：让 [`SurfaceStore`] 当前**已常驻**的部分像单张
/// [`ChunkGrid`] 一样喂给 [`crate::fov::compute_fov`]。
///
/// # 前置条件与任务 14 的关系
///
/// [`Self::terrain_at`](SightGrid::terrain_at) 假定被查询坐标所在的
/// 区块已经常驻，未常驻时 panic——任务 11 落地时这还是一座「调用方
/// 必须自己想办法保证前提成立」的过渡桥梁（当时唯一的办法是
/// [`SurfaceStore::warm_all`] 整个预热掉，只适合小型 demo 世界）。
///
/// **任务 14 已经真正解决这一步**：[`SurfaceStore::stream_neighborhood`]
/// 是生产环境下维护「玩家周围一圈区块常驻」的正确入口——渲染主循环
/// 应该在每次玩家移动后调用它（覆盖「视野半径 + 余量」的区块半径），
/// 让相邻区块在相机/FOV 真正查询到它们之前就已经生成好。调用了
/// `stream_neighborhood` 之后，本类型的 panic 前提在正常游玩路径下
/// 不会被触发——它仍然保留（而不是改成静默兜底），是为了在这条纪律
/// 被违反时（例如调用方漏调了 `stream_neighborhood`，或半径给小了）
/// 尽早、吵闹地暴露出来，而不是把一格看不见的黑块悄悄留在画面上。
pub struct SurfaceWindow<'a> {
    store: &'a SurfaceStore,
}

impl<'a> SurfaceWindow<'a> {
    /// 包装一个 [`SurfaceStore`] 引用。
    pub fn new(store: &'a SurfaceStore) -> Self {
        SurfaceWindow { store }
    }
}

impl SightGrid for SurfaceWindow<'_> {
    type Pos = TorusPos;

    fn terrain_at(&self, pos: TorusPos) -> TerrainKind {
        self.store.terrain_at_resident(pos).unwrap_or_else(|| {
            panic!(
                "SurfaceWindow 假定视野范围内的区块都已经常驻，{pos:?} 所属区块尚未加载——\
                 见 SurfaceWindow 文档「前置条件」"
            )
        })
    }

    fn offset(&self, origin: TorusPos, dx: i32, dy: i32) -> Option<TorusPos> {
        // 与 ChunkGrid 的 SightGrid 实现同理：环面没有「越界」这个概念。
        Some(
            self.store
                .layout
                .tile_size()
                .wrap(origin.x() + dx, origin.y() + dy),
        )
    }

    fn squared_euclidean(&self, a: TorusPos, b: TorusPos) -> u64 {
        self.store.layout.tile_size().squared_euclidean(a, b)
    }

    fn max_scan_row(&self, radius: u32) -> u32 {
        let world = self.store.layout.tile_size();
        radius.min(world.width() / 2).min(world.height() / 2)
    }
}

/// [`SurfaceStore`] 序列化用的扁平表示，见其文档「为什么需要手写
/// 序列化」。`resident` 按 [`SurfaceStore::resident_zones`] 的排序结果
/// 构造，让序列化输出本身也有确定顺序——不是正确性要求（反序列化
/// 重建 `HashMap` 后顺序不再重要），但排除了一个「为什么两次存档字节
/// 不同」的困惑来源，成本接近零。
#[derive(Serialize, Deserialize)]
struct SurfaceStoreData {
    layout: ZoneLayout,
    resident: Vec<(ZoneCoord, ChunkGrid)>,
    clock: RecencyClock<ZoneCoord>,
    resident_cap: usize,
}

impl Serialize for SurfaceStore {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let resident = self
            .resident_zones()
            .into_iter()
            .map(|zone| {
                let grid = self
                    .resident
                    .get(&zone)
                    .expect("resident_zones 只返回 resident 中真实存在的键")
                    .clone();
                (zone, grid)
            })
            .collect();
        SurfaceStoreData {
            layout: self.layout,
            resident,
            clock: self.clock.clone(),
            resident_cap: self.resident_cap,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SurfaceStore {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let data = SurfaceStoreData::deserialize(deserializer)?;
        let local_size = data.layout.local_size();
        let zone_count = data.layout.zone_count();
        let mut resident = HashMap::new();
        for (zone, grid) in data.resident {
            // `ZoneCoord`（即 `TorusPos`）自身没有「这是区块坐标」这层
            // 上下文——它的 `Deserialize` 只保证坐标非负，不保证落在
            // 这份 `layout` 的区块数范围内。一个被篡改的存档完全可以
            // 塞进一个数值巨大的坐标（例如把某个字节位翻转成 `x =
            // 999999999`）：这样的键插进 `resident` 之后，
            // `resident_zones()`/`terrain_at_resident` 等下游代码按
            // `zone.x() * zone_span + 局部偏移` 反推瓦片坐标、再用
            // `TorusSize::wrap` 环绕回一个合法瓦片坐标时，会落到与这个
            // 越界键完全不对应的另一个瓦片，查询该瓦片所属的（正确）
            // 区块键在 `resident` 里往往不存在——这正是任务 11 模糊
            // 测试撞见的真实缺陷：下游多处（`Self::try_remap_resident_terrain`/
            // `WorldState::hash`）依赖「`resident_zones()` 报出的坐标此刻
            // 必然能查到地形」这条不变式，在这个前提被打破后触发
            // `.expect()` panic，而不是本该发生的、体面的反序列化失败。
            // 在最早能拦住的地方（反序列化）校验区块坐标本身落在
            // `zone_count` 范围内，把这类畸形输入变成一次干净的 `Err`。
            if zone.x() as u32 >= zone_count.width() || zone.y() as u32 >= zone_count.height() {
                return Err(D::Error::custom(
                    "存档中某个常驻区块的坐标超出了区块布局的区块数范围",
                ));
            }
            if grid.world() != local_size {
                return Err(D::Error::custom(
                    "存档中某个常驻区块的地形网格尺寸与区块布局不一致",
                ));
            }
            if resident.insert(zone, grid).is_some() {
                return Err(D::Error::custom("存档中出现重复的常驻区块坐标"));
            }
        }
        Ok(SurfaceStore {
            layout: data.layout,
            resident,
            clock: data.clock,
            resident_cap: data.resident_cap,
            // 编年史不进存档（见字段文档）：读档后由调用方重新派生并
            // `attach_chronicle` 装回来。
            chronicle: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{GenParams, build_zone_noise};
    use crate::terrain::base_terrain_fixture;
    use ll_core::torus::TorusSize;

    /// 测试用区块布局：边长 64（满足最小视口跨度、是 16 与 32 的
    /// 倍数），3×3 个区块——够放下本文件全部淘汰场景需要的区块数量。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(3, 3).expect("3x3 是合法尺寸");
        ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
    }

    /// 第 `n` 个区块（沿 x 轴排开）左上角对应的世界瓦片坐标——用于
    /// 构造 `terrain_at` 的入参，n 取 0..zone_count.width()。
    fn tile_pos_of_zone(layout: &ZoneLayout, n: i32) -> TorusPos {
        layout.tile_size().wrap(n * layout.zone_span() as i32, 0)
    }

    /// 二维版本：第 `(nx, ny)` 个区块左上角对应的世界瓦片坐标——
    /// `test_layout` 是 3×3 个区块，单靠沿 x 轴排开的 `tile_pos_of_zone`
    /// 最多只能取到 3 个互不相同的区块，某些测试（例如需要五个以上
    /// 并列候选的平局测试）需要用到 y 轴撑开更多候选。
    fn tile_pos_of_zone_2d(layout: &ZoneLayout, nx: i32, ny: i32) -> TorusPos {
        let span = layout.zone_span() as i32;
        layout.tile_size().wrap(nx * span, ny * span)
    }

    #[test]
    fn 读取未常驻区块的坐标会触发按需生成() {
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone(&layout, 0);

        // Act
        store.terrain_at(&noise, &params, &terrain_ids, pos, Tick(1));

        // Assert
        assert_eq!(store.resident_zones().len(), 1);
    }

    #[test]
    fn 常驻区块数超过上限时淘汰最久未访问的一个() {
        // Arrange：上限 2，依次访问三个不同的区块。
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 2);
        let zone_a = tile_pos_of_zone(&layout, 0);
        let zone_b = tile_pos_of_zone(&layout, 1);
        let zone_c = tile_pos_of_zone(&layout, 2);

        // Act
        store.terrain_at(&noise, &params, &terrain_ids, zone_a, Tick(1));
        store.terrain_at(&noise, &params, &terrain_ids, zone_b, Tick(2));
        store.terrain_at(&noise, &params, &terrain_ids, zone_c, Tick(3));

        // Assert：最早访问的 zone_a 所在区块被淘汰,常驻数保持在上限。
        let (zone_a_coord, _) = layout.tile_to_zone(zone_a);
        assert!(!store.resident_zones().contains(&zone_a_coord));
    }

    #[test]
    fn 刚被访问过的区块不会被淘汰() {
        // Arrange：上限 2，A、B 各访问一次后重新访问 A，再访问 C——
        // 若 LRU 正确，此时最久未访问的是 B，不是被刷新过的 A。
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 2);
        let zone_a = tile_pos_of_zone(&layout, 0);
        let zone_b = tile_pos_of_zone(&layout, 1);
        let zone_c = tile_pos_of_zone(&layout, 2);

        // Act
        store.terrain_at(&noise, &params, &terrain_ids, zone_a, Tick(1));
        store.terrain_at(&noise, &params, &terrain_ids, zone_b, Tick(2));
        store.terrain_at(&noise, &params, &terrain_ids, zone_a, Tick(3)); // 刷新 A
        store.terrain_at(&noise, &params, &terrain_ids, zone_c, Tick(4)); // 挤掉最久未访问者

        // Assert
        let (zone_a_coord, _) = layout.tile_to_zone(zone_a);
        assert!(store.resident_zones().contains(&zone_a_coord));
    }

    #[test]
    fn 相同的访问序列在两次独立运行中产出相同的淘汰顺序() {
        // 确定性核心断言（C4/C5 的直接体现）：两个完全独立构造的
        // SurfaceStore（各自拥有独立的、随机种子不同的内部 HashMap）
        // 跑同一串访问序列，最终的常驻快照必须逐位相同——若淘汰顺序
        // 偷偷依赖了 HashMap 的桶序，这条测试有很高概率变红（两个
        // HashMap 的随机哈希种子在同一进程内也是各自独立的）。
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let sequence = [
            (tile_pos_of_zone(&layout, 0), Tick(1)),
            (tile_pos_of_zone(&layout, 1), Tick(2)),
            (tile_pos_of_zone(&layout, 2), Tick(3)),
            (tile_pos_of_zone(&layout, 0), Tick(4)),
            (tile_pos_of_zone(&layout, 1), Tick(5)),
        ];
        let run = || {
            let mut store = SurfaceStore::new(layout, 2);
            for (pos, tick) in sequence {
                store.terrain_at(&noise, &params, &terrain_ids, pos, tick);
            }
            store.resident_zones()
        };

        // Act
        let first_run = run();
        let second_run = run();

        // Assert
        assert_eq!(first_run, second_run);
    }

    #[test]
    fn 并列访问tick的区块淘汰顺序由区块坐标ord打破平局不受写入顺序影响() {
        // 五个区块在同一个 tick 被访问（全部并列，见下方 cap=5 迫使
        // 淘汰其中恰好一个），用五种不同的写入顺序（正序、倒序与若干
        // 次循环旋转）各构造一个独立的 SurfaceStore——独立构造意味着
        // 每个 store 内部 HashMap 的随机哈希种子也各自独立（Rust
        // std::collections::HashMap 的默认 RandomState 每次 `new()`
        // 都会派生一组新的哈希密钥）。
        //
        // 只用两个候选时曾经出现「淘汰逻辑碰巧依赖 HashMap 迭代顺序,
        // 但两个候选只有约 50% 概率能在单次运行内露出马脚」的情况——
        // 本任务实施期间把 evict_candidate 换成一个依赖 HashMap 迭代
        // 顺序打破平局的版本来验证这条测试是否真的有效,两个候选的
        // 版本没能稳定测红,五个候选、五种写入顺序才能在反复运行下
        // 稳定测红。
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let tied_positions: Vec<TorusPos> = [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1)]
            .into_iter()
            .map(|(nx, ny)| tile_pos_of_zone_2d(&layout, nx, ny))
            .collect();
        let forcing_pos = tile_pos_of_zone_2d(&layout, 2, 1);
        let tied_coords: Vec<ZoneCoord> = tied_positions
            .iter()
            .map(|pos| layout.tile_to_zone(*pos).0)
            .collect();
        let expected_evicted = *tied_coords.iter().min().expect("五个候选非空");

        let run_with_order = |order: &[usize]| -> Vec<ZoneCoord> {
            let mut store = SurfaceStore::new(layout, tied_positions.len());
            for &i in order {
                store.terrain_at(&noise, &params, &terrain_ids, tied_positions[i], Tick(5));
            }
            store.terrain_at(&noise, &params, &terrain_ids, forcing_pos, Tick(6));
            store.resident_zones()
        };
        let orders: [[usize; 5]; 5] = [
            [0, 1, 2, 3, 4],
            [4, 3, 2, 1, 0],
            [2, 0, 4, 1, 3],
            [1, 3, 0, 4, 2],
            [3, 4, 1, 2, 0],
        ];

        // Act
        let results: Vec<Vec<ZoneCoord>> =
            orders.iter().map(|order| run_with_order(order)).collect();

        // Assert：五种写入顺序全部一致地淘汰 Ord 最小的那个候选,一个
        // 都没有意外保留它。
        assert!(
            results
                .iter()
                .all(|residents| !residents.contains(&expected_evicted))
        );
    }

    #[test]
    fn 窗口化生成的结果与任务8的区块窗口生成函数一致() {
        // SurfaceStore 不能自己另外实现一套生成逻辑——terrain_at 的
        // 结果必须与直接调用 generate_zone_window 一致。
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone(&layout, 1);
        let (zone, local) = layout.tile_to_zone(pos);
        let expected_grid = generate_zone_window(&noise, &params, &layout, zone, &terrain_ids)
            .expect("test_layout 满足全部约束");

        // Act
        let from_store = store.terrain_at(&noise, &params, &terrain_ids, pos, Tick(1));

        // Assert
        assert_eq!(from_store, expected_grid.terrain_at(local));
    }

    #[test]
    fn 写入已常驻区块的地形能被读回() {
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone(&layout, 0);
        store.terrain_at(&noise, &params, &terrain_ids, pos, Tick(1));

        // Act
        store.set_terrain(pos, terrain_ids.mountain);

        // Assert
        assert_eq!(
            store.terrain_at(&noise, &params, &terrain_ids, pos, Tick(2)),
            terrain_ids.mountain
        );
    }

    #[test]
    #[should_panic(expected = "写入了尚未常驻的区块")]
    fn 写入尚未常驻的区块会panic() {
        // Arrange
        let layout = test_layout();
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone(&layout, 0);
        let (_ids, _table) = base_terrain_fixture();
        let dummy_kind = base_terrain_fixture().0.grass;

        // Act
        store.set_terrain(pos, dummy_kind);
    }

    #[test]
    fn 只读查询未常驻区块返回none() {
        // Arrange
        let layout = test_layout();
        let store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone(&layout, 0);

        // Act
        let result = store.terrain_at_resident(pos);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn 只读查询已常驻区块返回与terrain_at一致的地形() {
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone(&layout, 0);
        let loaded = store.terrain_at(&noise, &params, &terrain_ids, pos, Tick(1));

        // Act
        let readonly = store.terrain_at_resident(pos);

        // Assert
        assert_eq!(readonly, Some(loaded));
    }

    #[test]
    fn 调低常驻上限后已有条目不会被立即清退() {
        // Arrange：cap=3 装满三个区块。
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 3);
        for n in 0..3 {
            store.terrain_at(
                &noise,
                &params,
                &terrain_ids,
                tile_pos_of_zone(&layout, n),
                Tick(n as i64),
            );
        }

        // Act：把上限调到 1——不应该主动清退已经常驻的三个。
        store.set_resident_cap(1);

        // Assert
        assert_eq!(store.resident_zones().len(), 3);
    }

    #[test]
    fn 调低常驻上限后下一次准入会依据新上限淘汰() {
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 3);
        for n in 0..3 {
            store.terrain_at(
                &noise,
                &params,
                &terrain_ids,
                tile_pos_of_zone(&layout, n),
                Tick(n as i64),
            );
        }
        store.set_resident_cap(1);

        // Act：准入第四个区块——新上限是 1，应该淘汰到只剩一个。
        store.terrain_at(
            &noise,
            &params,
            &terrain_ids,
            tile_pos_of_zone_2d(&layout, 0, 1),
            Tick(10),
        );

        // Assert
        assert_eq!(store.resident_zones().len(), 1);
    }

    #[test]
    fn 预热全部区块后每个区块都常驻() {
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);

        // Act
        store.warm_all(&noise, &params, &terrain_ids, Tick(0));

        // Assert：3x3 布局共 9 个区块。
        assert_eq!(store.resident_zones().len(), 9);
    }

    #[test]
    fn 流式邻域维护后中心周围的区块全部常驻() {
        // Arrange：5x5 布局，中心区块 (2,2)，半径 1 应覆盖 3x3 = 9 个
        // 区块。
        let zone_count = TorusSize::new(5, 5).expect("5x5 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("布局满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone_2d(&layout, 2, 2);

        // Act
        store.stream_neighborhood(&noise, &params, &terrain_ids, pos, 1, Tick(0));

        // Assert
        for dy in -1..=1 {
            for dx in -1..=1 {
                let zone = zone_count.wrap(2 + dx, 2 + dy);
                assert!(
                    store.is_resident(zone),
                    "区块 {zone:?} 应在半径 1 的流式邻域内常驻"
                );
            }
        }
    }

    #[test]
    fn 流式邻域维护不影响半径外的区块() {
        // Arrange：5x5 布局，半径 1 覆盖不到最外圈的角区块 (4,4)。
        let zone_count = TorusSize::new(5, 5).expect("5x5 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("布局满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone_2d(&layout, 2, 2);

        // Act
        store.stream_neighborhood(&noise, &params, &terrain_ids, pos, 1, Tick(0));

        // Assert
        assert!(!store.is_resident(zone_count.wrap(4, 4)));
        assert_eq!(store.resident_zones().len(), 9);
    }

    #[test]
    fn 重复调用流式邻域维护不重复生成已常驻的区块() {
        // 幂等性：第二次调用同一个中心与半径,常驻区块集合不应变化——
        // stream_neighborhood 只应刷新访问时间,不应该产生任何副作用
        // 之外的行为（例如意外触发淘汰或重新生成）。
        // Arrange
        let zone_count = TorusSize::new(5, 5).expect("5x5 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("布局满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone_2d(&layout, 2, 2);
        store.stream_neighborhood(&noise, &params, &terrain_ids, pos, 1, Tick(0));
        let first = store.resident_zones();

        // Act
        store.stream_neighborhood(&noise, &params, &terrain_ids, pos, 1, Tick(1));
        let second = store.resident_zones();

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 校验全部常驻区块时未注册地形返回错误() {
        // Arrange：只注册一个空表（不含 warm_all 生成用到的任何地形）。
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        store.warm_all(&noise, &params, &terrain_ids, Tick(0));
        let empty_table = TerrainTable::default();

        // Act
        let result = store.validate_resident(&empty_table);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 校验全部常驻区块时地形均已注册返回成功() {
        // Arrange
        let layout = test_layout();
        let (terrain_ids, table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        store.warm_all(&noise, &params, &terrain_ids, Tick(0));

        // Act
        let result = store.validate_resident(&table);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn surfacewindow喂给compute_fov在预热区域内产出与直接查chunkgrid一致的可见集() {
        // SurfaceWindow 存在的唯一理由：让既有 compute_fov 调用点在
        // SurfaceStore 换型后不必改算法——这里验证它对同一份数据产出
        // 与直接对 generate_zone_window 结果调用 compute_fov 相同的
        // 可见集合（只要视野半径不越出预热的单个区块）。
        // Arrange
        let layout = test_layout();
        let (terrain_ids, table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        store.warm_all(&noise, &params, &terrain_ids, Tick(0));
        let origin = tile_pos_of_zone_2d(&layout, 1, 1);
        let radius = 5;

        // Act
        let via_window =
            crate::fov::compute_fov(&SurfaceWindow::new(&store), &table, origin, radius);
        let (zone, local) = layout.tile_to_zone(origin);
        let direct_grid = generate_zone_window(&noise, &params, &layout, zone, &terrain_ids)
            .expect("test_layout 满足全部约束");
        let via_grid = crate::fov::compute_fov(&direct_grid, &table, local, radius);

        // Assert：视野半径 5 小于区块边长 64 的一半,不会跨出这个区块,
        // 两种查询路径应产出相同数量的可见格。
        assert_eq!(via_window.len(), via_grid.len());
    }

    #[test]
    fn surfacestore经serde格式往返后常驻内容不变() {
        // 满足硬性约束「SurfaceStore/Interior 从一开始就要求完整可
        // 序列化往返」——用真实的 serde_json 格式验证，而不只是停留在
        // derive/手写 impl 能编译这一层面。
        // Arrange
        let layout = test_layout();
        let (terrain_ids, _table) = base_terrain_fixture();
        let params = GenParams::default();
        let noise = build_zone_noise(&layout, &params).expect("test_layout 满足全部约束");
        let mut store = SurfaceStore::new(layout, 256);
        let pos = tile_pos_of_zone(&layout, 1);
        store.terrain_at(&noise, &params, &terrain_ids, pos, Tick(1));
        let json = serde_json::to_string(&store).expect("SurfaceStore 必然可序列化");

        // Act
        let mut decoded: SurfaceStore =
            serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(
            decoded.terrain_at(&noise, &params, &terrain_ids, pos, Tick(2)),
            store.terrain_at(&noise, &params, &terrain_ids, pos, Tick(2))
        );
    }
}
