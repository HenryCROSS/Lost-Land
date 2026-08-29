//! 探索记忆：玩家「看没看过」某个格子的记录，供 [`crate::overview`]
//! 的战争迷雾展示使用。
//!
//! # 只存位图，不存地形副本
//!
//! 未被玩法改动过的地形是确定性的——同一个种子、同一份噪声参数随时
//! 能重算出同一份地形（`crate::noise`/`crate::generate`）。探索记忆
//! 因此只需要回答「这一格看没看过」这一个是/否问题，不需要另存一份
//! 「上次看到的地形」快照：
//!
//! - **错**：每格存一份「上次看到的地形」——十万区块级的世界、十几亿
//!   格，哪怕每格只用 2 字节也逼近 GB 级，且这份快照与地形本身是两个
//!   可能漂移的真相源（本项目已经为这类重复真相源付过代价，见
//!   `crate::interior` 模块文档「单一真相源」引用的两次历史教训）。
//! - **对**：每格 1 bit「看没看过」，且只存玩家真正去过的区块——
//!   [`ZoneExploration`] 是一个 `zone_span * zone_span` 位的位图，
//!   [`ExplorationMemory`] 只为去过的区块分配一份。玩家实际走过的区块
//!   数量级是几百到几千，远小于世界总区块数（默认区块边长 48 下，一个
//!   区块的位图是 `48*48=2304` bit ≈ 288 字节）。
//!
//! 这是「默认派生，只存偏差」的第十二次复用（`knowledge/design/README.md`
//! 有完整列表）：地形本身是「默认」（噪声可重算），探索记忆只记「偏差」
//! ——这一格是否偏离了「从未被观察过」的默认状态。若某一格的地形被
//! 玩法真正改动过，那份改动是地形自己的存储职责（`crate::surface_store`），
//! 不是本模块的职责——本模块自始至终只回答「看没看过」，从不携带任何
//! 地形数据，两者关注点不重叠，不会互相踩踏。
//!
//! # 为什么读取接口要求显式传入 `&ExplorationMemory`（「谁的视角」）
//!
//! 见 [`crate::overview`] 模块文档：`minimap`/`continent_map` 曾经完全
//! 不接受任何探索记忆参数，`OverviewCell::explored` 因此只能恒为
//! `true`。现在两者都要求调用方显式传入一份 `&ExplorationMemory`，而
//! 不是隐式从某个全局单例或 `WorldState` 内部字段读取——`continent_map`
//! 尤其不能这样做：它的签名刻意不接受 `WorldState`（见其文档「不接触
//! `WorldState`/`SurfaceStore`」），没有隐式来源可读。显式参数还带来
//! 一个好处：当前一份存档只代表一个角色的视角，[`crate::state::WorldState::exploration`]
//! 因此只存一份；但接口本身不假设「探索记忆恒只有一份」——未来若真的
//! 出现多角色共享同一个世界（队伍视角、多人共享世界）的需求，调用方
//! 只需要换一份 `&ExplorationMemory` 传进来，`minimap`/`continent_map`
//! 的签名不需要再变。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use ll_core::hashing::StateHasher;
use ll_core::torus::TorusPos;

use crate::space::ZoneCoord;
use crate::zone::ZoneLayout;

/// 一个区块内的探索位图：每格 1 bit，`1` 表示已探索。
///
/// 长度恒为 `ceil(zone_span * zone_span / 64)` 个 `u64`——见
/// [`words_for_span`]。不随 `ZoneLayout` 存一份 `zone_span`：区块内的
/// 位下标（[`local_bit_index`]）由调用方（[`ExplorationMemory`]）持有
/// 的 `ZoneLayout` 统一换算，同一个 `ExplorationMemory` 内的全部区块
/// 共享同一个 `zone_span`，不需要每个区块各自记一份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ZoneExploration {
    bits: Vec<u64>,
}

/// 一个 `zone_span * zone_span` 位的位图需要多少个 `u64` 字。
fn words_for_span(zone_span: u32) -> usize {
    let cell_count = zone_span as usize * zone_span as usize;
    cell_count.div_ceil(u64::BITS as usize)
}

/// 区块内局部坐标换算成位图下标——按行主序（`y * zone_span + x`），与
/// [`crate::state::WorldState::hash`] 遍历地形格的顺序一致，不需要另起
/// 一套坐标换算习惯。
fn local_bit_index(local: TorusPos, zone_span: u32) -> usize {
    local.y() as usize * zone_span as usize + local.x() as usize
}

impl ZoneExploration {
    /// 建立一个全未探索的位图。
    fn empty(zone_span: u32) -> Self {
        ZoneExploration {
            bits: vec![0u64; words_for_span(zone_span)],
        }
    }

    /// 标记下标 `index` 对应的格子为已探索。
    fn mark(&mut self, index: usize) {
        let word = index / u64::BITS as usize;
        let bit = index % u64::BITS as usize;
        // words_for_span 与 local_bit_index 对同一个 zone_span 恒自洽
        // （下标恒小于 zone_span*zone_span，word 恒小于 bits.len()），
        // 越界只可能来自调用方传入了与本位图不匹配的 zone_span——那是
        // 编程错误，不是需要静默吞掉的正常路径，因此这里不做防御性
        // 早退，让越界访问按 Rust 的既有语义直接 panic 暴露出来。
        self.bits[word] |= 1u64 << bit;
    }

    /// 查询下标 `index` 对应的格子是否已探索。
    fn get(&self, index: usize) -> bool {
        let word = index / u64::BITS as usize;
        let bit = index % u64::BITS as usize;
        match self.bits.get(word) {
            Some(bits) => (bits >> bit) & 1 == 1,
            None => false,
        }
    }

    /// 该区块内是否至少有一格已探索——供
    /// [`ExplorationMemory::zone_has_any_explored`]（区块粒度概览，如
    /// `continent_map`）使用。
    fn any(&self) -> bool {
        self.bits.iter().any(|word| *word != 0)
    }
}

/// 一个角色的探索记忆：按角色持久化（随存档一起读写），只记录「看没
/// 看过」，不携带任何地形数据——见模块文档。
///
/// # 序列化：`Vec<(ZoneCoord, _)>`，不是 `BTreeMap` 的默认表示
///
/// `zones: BTreeMap<ZoneCoord, ZoneExploration>`——`BTreeMap` 而非
/// `HashMap`（约束 C5：禁止 `HashMap`/`HashSet` 迭代顺序参与逻辑判断，
/// 这里的判断包括 [`Self::write_hash`] 的混入顺序）。但 `ZoneCoord`
/// （即 [`TorusPos`]）是结构体，不是字符串——`serde_json` 这类要求 map
/// 键必须是字符串的格式无法直接处理 `BTreeMap<ZoneCoord, _>`，而
/// `WorldState` 的往返测试确实用 `serde_json` 覆盖（见
/// `crate::state` 模块文档）。因此走与
/// [`crate::mod_state::serde_map`] 相同的手法：序列化成有序
/// `(键, 值)` 列表，反序列化时重建 `BTreeMap`——见
/// [`zone_map`]。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplorationMemory {
    #[serde(with = "zone_map")]
    zones: BTreeMap<ZoneCoord, ZoneExploration>,
}

/// 把 `BTreeMap<ZoneCoord, ZoneExploration>` 序列化成有序条目列表——
/// 与 [`crate::mod_state::serde_map`] 同一种手法，见
/// [`ExplorationMemory`] 文档「序列化」一节。
mod zone_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{ZoneCoord, ZoneExploration};

    pub fn serialize<S>(
        map: &BTreeMap<ZoneCoord, ZoneExploration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries: Vec<(&ZoneCoord, &ZoneExploration)> = map.iter().collect();
        entries.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<BTreeMap<ZoneCoord, ZoneExploration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries: Vec<(ZoneCoord, ZoneExploration)> = Vec::deserialize(deserializer)?;
        Ok(entries.into_iter().collect())
    }
}

impl ExplorationMemory {
    /// 建立一份空记忆：还没有任何区块被探索过。
    pub fn new() -> Self {
        Self::default()
    }

    /// 建立一份「**全图已探索**」的记忆——每个区块都标上一格。
    ///
    /// # 谁要它，为什么不是一个 `reveal_all` 标志
    ///
    /// 开局的**选出生地界面**需要全图可见（玩家还没进世界，谈不上
    /// 「去过哪」）。而 `crate::world_map::world_map_slice` 与
    /// `crate::overview::continent_map` 都**显式要求调用方传一份**
    /// `&ExplorationMemory`（见本模块文档「为什么读取接口要求显式传入」
    /// 一节）——那条设计正是为这种场合准备的：选点界面传一份全部已探索
    /// 的记忆进去，`explored` 就恒为真，**同一份呈现代码**自然变成全图
    /// 可见。
    ///
    /// 加一个 `reveal_all: bool` 标志会走上相反的路：每一处读探索状态的
    /// 地方都要多一条分支，而那些分支只有一处调用方会走成 `true`，其余
    /// 全部永远是 `false`——一条长期存在、几乎不被执行、却必须被每个
    /// 后来人绕过的死代码。
    ///
    /// # 粒度：每个区块一格就够
    ///
    /// 两个消费者判「这一片黑不黑」用的都是区块粒度的
    /// [`Self::zone_has_any_explored`]，一格与全铺的效果完全一样，而
    /// 全铺要写 `区块数 × zone_span²` 个位（默认世界约一千四百万位）。
    ///
    /// # 它绝不该被写进 `WorldState`
    ///
    /// 这份记忆只活在选点界面的草稿里。写进世界状态等于永久摧毁战争
    /// 迷雾——玩家一进游戏整张地图就是亮的。
    pub fn fully_explored(layout: &ZoneLayout) -> Self {
        let zone_count = layout.zone_count();
        let mut memory = Self::new();
        let span = layout.zone_span() as i32;
        for y in 0..zone_count.height() as i32 {
            for x in 0..zone_count.width() as i32 {
                // 取该区块左上角那一格的世界坐标，交给 `mark_explored`
                // 自己换算回 (区块, 局部) ——**不手写位图下标**：那会在
                // 本模块内造出第二处「局部坐标怎么变成位下标」的知识，
                // 与 `local_bit_index` 分叉的那一天谁都发现不了。
                memory.mark_explored(layout, layout.tile_size().wrap(x * span, y * span));
            }
        }
        memory
    }

    /// 把 `pos`（世界瓦片坐标）标记为已探索。
    ///
    /// `layout` 决定该坐标落在哪个区块、区块内哪一格——同一份
    /// `ExplorationMemory` 的全部调用必须使用同一个 `ZoneLayout`（即
    /// 拥有它的 `WorldState` 的 `terrain.layout()`），混用不同布局会让
    /// 已记录的位图与新的 `zone_span` 对不上。
    ///
    /// # 谁来调用、什么时候调用
    ///
    /// 本方法只是一次纯粹的位图写入，不涉及「什么时候该标记探索」这个
    /// 判断——那属于视野/FOV 结算的职责（`crate::fov`、
    /// 未来 `ll_sim::resolve`/`apply` 的接线），且必须遵守约束 C1
    /// （`apply` 是唯一写入口）。本次任务只交付探索记忆自身的存储形状
    /// 与读取接口，游戏循环何时调用本方法不在本次范围内——与
    /// `crate::interior::Interior::origin` 「本次只补接口形状，不实现
    /// 生成器」是同一种最小改动纪律。
    pub fn mark_explored(&mut self, layout: &ZoneLayout, pos: TorusPos) {
        let (zone, local) = layout.tile_to_zone(pos);
        let zone_span = layout.zone_span();
        let entry = self
            .zones
            .entry(zone)
            .or_insert_with(|| ZoneExploration::empty(zone_span));
        entry.mark(local_bit_index(local, zone_span));
    }

    /// `pos`（世界瓦片坐标）是否已被探索过。未去过的区块（不在
    /// `zones` 里）视为未探索，不是错误——绝大多数区块玩家从未涉足，
    /// 这是最常见的正常路径。
    pub fn is_explored(&self, layout: &ZoneLayout, pos: TorusPos) -> bool {
        let (zone, local) = layout.tile_to_zone(pos);
        match self.zones.get(&zone) {
            Some(exploration) => exploration.get(local_bit_index(local, layout.zone_span())),
            None => false,
        }
    }

    /// 给定区块内是否至少有一格已探索——供区块粒度的概览（如
    /// `continent_map`）使用：那个粒度只关心「这个区块去没去过」，不
    /// 需要问某一格。
    pub fn zone_has_any_explored(&self, zone: ZoneCoord) -> bool {
        match self.zones.get(&zone) {
            Some(exploration) => exploration.any(),
            None => false,
        }
    }

    /// 已经至少探索过一格的区块数——供测试与「这份记忆占多大」这类
    /// 诊断信息使用。
    pub fn visited_zone_count(&self) -> usize {
        self.zones.len()
    }

    /// 把本记忆混入哈希——[`crate::state::WorldState::hash`] 的帮手
    /// （硬性约束「判据漏了东西，测试就是在空跑」，ADR 0022）。
    ///
    /// 先混入区块数，再按 `zones`（`BTreeMap`，自然按 `ZoneCoord`
    /// 排序）遍历——不依赖任何 `HashMap`/`HashSet` 迭代顺序（约束
    /// C5）。每个区块先混入坐标、再混入位图字数与逐个位图字，理由与
    /// `state::write_mod_state`（模块私有，无法作为 rustdoc 链接
    /// 目标）一致：变长数据混入前先写长度，避免相邻字段在字节流里
    /// 边界不清导致的理论碰撞。
    pub(crate) fn write_hash(&self, hasher: &mut StateHasher) {
        hasher.write_u64(self.zones.len() as u64);
        for (zone, exploration) in &self.zones {
            hasher.write_i64(i64::from(zone.x()));
            hasher.write_i64(i64::from(zone.y()));
            hasher.write_u64(exploration.bits.len() as u64);
            for word in &exploration.bits {
                hasher.write_u64(*word);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn 全图已探索的记忆里每个区块都算去过() {
        // 选出生地屏靠它变成全图可见（`world_map_slice` 的 `explored`
        // 判据走的是区块粒度的 `zone_has_any_explored`）。若这个构造器
        // 退化成一份空记忆，整张选点地图会全黑，玩家无从下手。
        // Arrange
        let zone_count = ll_core::torus::TorusSize::new(4, 3).expect("4x3 是合法尺寸");
        let layout = ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束");

        // Act
        let memory = ExplorationMemory::fully_explored(&layout);

        // Assert：一个区块都不能漏。
        assert_eq!(
            memory.visited_zone_count(),
            (zone_count.width() * zone_count.height()) as usize,
            "全图已探索的记忆漏掉了区块" // i18n-exempt：测试断言的失败消息
        );
        for y in 0..zone_count.height() as i32 {
            for x in 0..zone_count.width() as i32 {
                assert!(
                    memory.zone_has_any_explored(zone_count.wrap(x, y)),
                    "区块 ({x}, {y}) 没有被标记为已探索" // i18n-exempt：测试断言的失败消息
                );
            }
        }
    }

    #[test]
    fn 空记忆里没有任何区块算去过() {
        // 反例：证明上一条不是「无论如何都返回真」。
        let zone_count = ll_core::torus::TorusSize::new(4, 3).expect("4x3 是合法尺寸");
        let memory = ExplorationMemory::new();
        assert_eq!(memory.visited_zone_count(), 0);
        assert!(!memory.zone_has_any_explored(zone_count.wrap(0, 0)));
    }
    use super::*;
    use ll_core::torus::TorusSize;

    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(4, 4).expect("4x4 是合法尺寸");
        ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
    }

    #[test]
    fn 新建的探索记忆里任何坐标都未探索() {
        // Arrange
        let layout = test_layout();
        let memory = ExplorationMemory::new();
        let pos = layout.tile_size().wrap(10, 10);

        // Act & Assert
        assert!(!memory.is_explored(&layout, pos));
    }

    #[test]
    fn 标记过的坐标查询结果为已探索() {
        // Arrange
        let layout = test_layout();
        let mut memory = ExplorationMemory::new();
        let pos = layout.tile_size().wrap(10, 10);

        // Act
        memory.mark_explored(&layout, pos);

        // Assert
        assert!(memory.is_explored(&layout, pos));
    }

    #[test]
    fn 标记一格不影响同一区块内的其他格() {
        // Arrange
        let layout = test_layout();
        let mut memory = ExplorationMemory::new();
        let marked = layout.tile_size().wrap(10, 10);
        let untouched = layout.tile_size().wrap(11, 10);

        // Act
        memory.mark_explored(&layout, marked);

        // Assert
        assert!(!memory.is_explored(&layout, untouched));
    }

    #[test]
    fn 标记一个区块后该区块判定为存在已探索格() {
        // Arrange
        let layout = test_layout();
        let mut memory = ExplorationMemory::new();
        let pos = layout.tile_size().wrap(5, 5);
        let (zone, _local) = layout.tile_to_zone(pos);

        // Act
        memory.mark_explored(&layout, pos);

        // Assert
        assert!(memory.zone_has_any_explored(zone));
    }

    #[test]
    fn 从未去过的区块判定为不存在已探索格() {
        // Arrange
        let layout = test_layout();
        let memory = ExplorationMemory::new();
        let zone = layout.zone_count().wrap(2, 2);

        // Act & Assert
        assert!(!memory.zone_has_any_explored(zone));
    }

    #[test]
    fn 已探索区块数随不同区块的首次标记递增() {
        // Arrange
        let layout = test_layout();
        let mut memory = ExplorationMemory::new();
        let first = layout.tile_size().wrap(1, 1); // 区块 (0, 0)
        let second = layout.tile_size().wrap(49, 1); // 区块 (1, 0)

        // Act
        memory.mark_explored(&layout, first);
        assert_eq!(memory.visited_zone_count(), 1);
        memory.mark_explored(&layout, second);

        // Assert
        assert_eq!(memory.visited_zone_count(), 2);
    }

    #[test]
    fn 同一区块内重复标记不同格子不新增区块计数() {
        // Arrange
        let layout = test_layout();
        let mut memory = ExplorationMemory::new();
        let a = layout.tile_size().wrap(1, 1);
        let b = layout.tile_size().wrap(2, 1);

        // Act
        memory.mark_explored(&layout, a);
        memory.mark_explored(&layout, b);

        // Assert
        assert_eq!(memory.visited_zone_count(), 1);
    }

    #[test]
    fn 经序列化格式往返后探索状态不变() {
        // Arrange
        let layout = test_layout();
        let mut memory = ExplorationMemory::new();
        memory.mark_explored(&layout, layout.tile_size().wrap(3, 3));
        let json = serde_json::to_string(&memory).expect("ExplorationMemory 必然可序列化");

        // Act
        let decoded: ExplorationMemory =
            serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert!(decoded.is_explored(&layout, layout.tile_size().wrap(3, 3)));
        assert!(!decoded.is_explored(&layout, layout.tile_size().wrap(4, 3)));
    }

    #[test]
    fn 空记忆写入哈希与非空记忆写入哈希产出不同摘要() {
        // Arrange
        let layout = test_layout();
        let mut hasher_empty = StateHasher::new();
        let mut hasher_marked = StateHasher::new();
        let empty = ExplorationMemory::new();
        let mut marked = ExplorationMemory::new();
        marked.mark_explored(&layout, layout.tile_size().wrap(0, 0));

        // Act
        empty.write_hash(&mut hasher_empty);
        marked.write_hash(&mut hasher_marked);

        // Assert
        assert_ne!(hasher_empty.finish(), hasher_marked.finish());
    }
}
