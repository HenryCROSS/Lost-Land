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

use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use serde::de::Error as _;
use serde::{Deserialize, Serialize, Serializer};

use crate::chunk::ChunkGrid;
use crate::generate::generate_zone_window;
use crate::noise::TileableNoise;
use crate::space::ZoneCoord;
use crate::terrain::{BaseTerrainIds, TerrainKind};
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
        }
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

        let grid = generate_zone_window(noise, params, &self.layout, zone, terrain_ids)
            .expect("ZoneLayout 构造时已校验区块边长满足 ChunkGrid 的最小视口跨度，生成不应失败");
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
        let mut resident = HashMap::new();
        for (zone, grid) in data.resident {
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
