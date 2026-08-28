//! 地形种类与其游戏规则属性——「本体即 Mod」在地形层面的落点。
//!
//! # 从硬编码 `match` 到注册表（P4 Task 8）
//!
//! 本模块曾经是一份硬编码的 `pub const` 常量集合（`TerrainKind(u16)`，
//! `blocks_sight`/`blocks_move`/`move_cost` 各自内部 `match`）。P4 把
//! 地形迁入内容注册表：地形不再是编译期常量，而是运行期
//! `ll_core::ident::Interner::intern`（或 `ll-mod::Registry::intern`）
//! 产出的 [`ContentIndex`]。这带来一个绕不开的问题：
//! `TerrainKind::MOUNTAIN` 这类编译期字面量在「数值由注册期加载顺序
//! 决定」的世界里不可能继续存在。
//!
//! # 与 Registry 的关系（依赖方向）
//!
//! 依赖顺序是 `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`（规格
//! §5）：定义 `Registry` 的 `ll-mod` 在 `ll-world` **下游**，本 crate
//! 绝不能反过来依赖它。本模块因此不认识 `Registry` 这个类型，只依赖
//! `ll-core` 已有的 [`ContentIndex`]/[`NamespacedId`]（`ll-world` 一贯
//! 就依赖 `ll-core`，这条依赖没有变化）。
//!
//! [`materialize_base_terrain`] 是本体地形注册的唯一入口，签名接受一个
//! `&mut dyn FnMut(NamespacedId) -> ContentIndex` 回调，而不是接受一个
//! 具体的 `Registry`/`Interner` 类型：
//!
//! - 生产路径（`ll-mod`）传入 `|id| registry.intern(id)`——真正调用
//!   `Registry::intern`，与 mod 注册内容走**完全相同**的一条代码路径,
//!   Registry 内部无法区分这次 `intern` 调用是本体发起的还是 mod 发起
//!   的（`Registry`/`Interner` 本身也确实不做这种区分，见
//!   `ll-mod::registry` 模块文档）。
//! - 测试/demo 路径（本 crate 与 `ll-sim`/`ll-script` 的单元测试、
//!   `p2_acceptance`/`p3_acceptance` 验收 demo）用 [`base_terrain_fixture`]
//!   ——内部现造一个空 [`ll_core::ident::Interner`]，不牵扯任何 mod
//!   加载或 `Registry`。
//!
//! 这个「接受注入的解析回调」的形状，就是本任务对简报里「依赖方向
//! 问题」给出的正面回应：把「地形定义是什么」与「谁负责发号
//! `ContentIndex`」彻底解耦，`ll-world` 只依赖前者。
//!
//! # 启动时一次性解析，运行期仍是常量级访问（裁定 P4-2）
//!
//! 旧版 13 处 `TerrainKind::MOUNTAIN` 字面量改为访问 [`BaseTerrainIds`]
//! 的对应字段——这是 [`materialize_base_terrain`] 在启动时一次性解析
//! 出来的缓存结构，运行期访问它的字段与访问旧版 `pub const` 同样是
//! 常量级开销，只是不能再写成编译期字面量。
//!
//! # 物化为列式数据，注册期完整校验（ADR 0017）
//!
//! [`TerrainTable`] 按属性分列（`move_cost: Vec<u32>` 等），不按内容
//! 分结构——批量遍历只关心一个属性时不会把用不到的其它字段一起拖进
//! 缓存行，与薄层人口存储（[0004](../../../knowledge/decisions/0004-two-layer-entity-storage.md)）
//! 同一套道理。[`TerrainTable::define`] 是注册期入口，在这里完整校验
//! 声明（见其文档），错误在加载时就能报出来，而不是等玩到某个场景才
//! 表现成怪行为。
//!
//! # `opens_into`：把「撞门即开」收拢成声明式属性（迁移中撞见的 API 洞）
//!
//! 旧版 `ll-sim::resolve::resolve_move`/`resolve_open_door` 直接硬编码
//! `terrain == TerrainKind::DOOR_CLOSED` 做恒等比较，撞见就把该格改写
//! 成 `TerrainKind::DOOR_OPEN`——这是本体独享的一条特权路径：任何 mod
//! 想注册一种「撞入即开」的地形（活板门、栅栏），都没有公开 API 能表达
//! 同样的行为，只能去改 `ll-sim` 的源码。ADR 0016 的守门规则说得很
//! 清楚：**若本体需要一个 mod 够不着的能力，那是 API 缺陷，不是特性。**
//! 因此本次迁移给 [`TerrainDef`] 加了 `opens_into: Option<NamespacedId>`
//! 字段——「撞入这格会变成另一种地形」是任意地形都可以声明的属性，不
//! 是只有 `DOOR_CLOSED` 这一个硬编码 ID 才有的特权。`ll-sim::resolve`
//! 相应地把恒等比较换成 `table.opens_into(terrain)` 查表，见其文档。

use std::collections::BTreeMap;
use std::fmt;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use serde::{Deserialize, Serialize};

use crate::chunk::ChunkGrid;

/// 地形种类：指向 [`TerrainTable`] 中一条地形定义的索引。
///
/// 不再是编译期常量集合——数值由注册期的 [`materialize_base_terrain`]
/// （或未来 mod 注册的等价调用）决定。反序列化不做「是否已注册」的
/// 校验（ADR 0015：那是解析，不是不变式），只做结构转换，因此这里
/// 直接派生 `Serialize`/`Deserialize`，与内部的 [`ContentIndex`] 同一
/// 个理由（见其文档）。真正校验「存档里的每一个地形索引当前是否已
/// 注册」的入口是 [`TerrainTable::validate_grid`]，由持有 `ChunkGrid`
/// 整体反序列化结果与当前 `TerrainTable` 的调用方显式调用。
/// `PartialOrd`/`Ord` 按内部 [`ContentIndex`] 的数值排序——只用来当
/// `BTreeMap` 的键（[`TerrainTable::closes_into`] 的逆映射）与做确定性
/// 的平局判断，**不表示任何玩法上的先后**。派生它是 C5 的正向选择：
/// 有序容器的遍历顺序确定，`HashMap` 的不确定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TerrainKind(ContentIndex);

impl TerrainKind {
    /// 从内容索引直接构造，不做任何校验。
    ///
    /// 与旧版 `TerrainKind(pub u16)` 元组字段可以被外部随意构造是
    /// 同一种开放性——校验被推迟到查询/批量校验那一步（见模块文档），
    /// 构造本身必须无状态、无副作用（ADR 0015）。
    pub fn from_index(index: ContentIndex) -> Self {
        TerrainKind(index)
    }

    /// 取出底层内容索引。
    pub fn index(&self) -> ContentIndex {
        self.0
    }

    /// 该地形是否阻挡视线。查询未注册的索引时返回 `false`（视为
    /// 透明）——与 [`TerrainTable::move_cost`] 同一套「安全兜底」取舍,
    /// 见该方法文档。
    pub fn blocks_sight(&self, table: &TerrainTable) -> bool {
        table.blocks_sight(*self)
    }

    /// 该地形是否完全不可通行。查询未注册的索引时返回 `false`
    /// （视为可通行），理由同 [`Self::blocks_sight`]。
    pub fn blocks_move(&self, table: &TerrainTable) -> bool {
        table.blocks_move(*self)
    }

    /// 移动经过该地形的代价。查询未注册的索引时按平地基准（100）
    /// 处理，理由见 [`TerrainTable::move_cost`] 文档。
    pub fn move_cost(&self, table: &TerrainTable) -> u32 {
        table.move_cost(*self)
    }

    /// 撞入该地形时它会变成的另一种地形（例如关着的门变成开着的门）。
    /// `None` 表示这不是一格「撞入即开」的地形。见模块文档
    /// 「`opens_into`」一节。
    pub fn opens_into(&self, table: &TerrainTable) -> Option<TerrainKind> {
        table.opens_into(*self)
    }
}

/// 单条地形声明：本体与 mod 注册地形时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在地形层面的验收标的——[`materialize_base_terrain`]
/// 拿这个类型的值去调用外部传入的 `intern` 回调，本体的声明与未来 mod
/// 的声明除了 `id` 里的命名空间字符串不同之外，不存在任何结构性差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainDef {
    /// 命名空间标识符，例如 `lostland:mountain`、`yourmod:crystal`。
    pub id: NamespacedId,
    /// 该地形是否阻挡视线。
    pub blocks_sight: bool,
    /// 该地形是否完全不可通行。
    pub blocks_move: bool,
    /// 移动经过该地形的代价，以平地的 100 为基准；不可通行地形必须
    /// 声明为 `u32::MAX`（见 [`TerrainTable::define`] 的校验）。
    pub move_cost: u32,
    /// 撞入该地形时变成的另一种地形，`None` 表示不是这类地形。见模块
    /// 文档「`opens_into`」一节。
    pub opens_into: Option<NamespacedId>,
}

/// [`TerrainTable::define`] 实际存进列式存储的属性子集——不含 `id`
/// （`id` 只在注册那一刻用于换取 [`ContentIndex`]，换到之后就不再
/// 需要）,`opens_into` 也已经从字符串解析成同一张表里的 [`TerrainKind`]。
///
/// **必须公开**：这是 [`TerrainTable::define`] 唯一的参数类型，任何
/// 想直接调用 `define`（而不是走 [`materialize_base_terrain`] 那条
/// 便捷路径）的调用方——包括未来 mod 自己的地形注册函数——都需要能
/// 构造这个类型。早期草稿把它写成模块私有，导致 `define` 事实上无法
/// 从模块外调用（公开函数、私有参数类型，编译能过但外部拿不到构造
/// 入口）——这正是「本体即 Mod」验收要抓的那类 API 洞，写测试时被
/// 直接撞见，随手就改了，见本任务报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainAttrs {
    /// 该地形是否阻挡视线。
    pub blocks_sight: bool,
    /// 该地形是否完全不可通行。
    pub blocks_move: bool,
    /// 移动经过该地形的代价，以平地的 100 为基准；不可通行地形必须
    /// 声明为 `u32::MAX`（见 [`TerrainTable::define`] 的校验）。
    pub move_cost: u32,
    /// 撞入该地形时变成的另一种地形，见模块文档「`opens_into`」一节。
    pub opens_into: Option<TerrainKind>,
}

/// 地形注册期可能出现的错误。
///
/// 全部由 [`TerrainTable::define`]/[`TerrainTable::validate_grid`] 产出
/// ——ADR 0017「注册期完整校验」要求这些错误在加载时就报出来，而不是
/// 等到查询某个具体地形时才表现成怪行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainError {
    /// 同一个内容索引被定义了两次。
    ///
    /// **这正是简报要求正面处理的已知缺口**：`Registry::intern` 对同一
    /// 个 `NamespacedId` 重复调用是幂等的（返回同一个索引，见
    /// `ll-mod::registry` 模块文档），但幂等的是「索引分配」，不是
    /// 「这个索引对应的地形属性」——两个不同的 mod（或某 mod 与本体）
    /// 若都尝试给同一个 `id` 定义地形属性，旧行为会让后调用的
    /// `define` 悄悄覆盖前一次的结果，玩家看到的地形行为会莫名其妙。
    /// 本类型把第二次 `define` 判成错误，而不是静默覆盖。
    DuplicateDefinition(ContentIndex),
    /// `blocks_move` 与 `move_cost` 互相矛盾：不可通行地形必须把
    /// `move_cost` 声明为 `u32::MAX`，可通行地形必须声明一个非零且
    /// 非 `u32::MAX` 的有限代价——零代价等价于「免费格」，`u32::MAX`
    /// 会让寻路算法把一格明明可通行的地形当成不可通行,两者都是填错
    /// 数据的信号，必须在注册期拦下。
    InconsistentMoveCost {
        /// 声明的 `blocks_move`。
        blocks_move: bool,
        /// 声明的 `move_cost`，与 `blocks_move` 不自洽。
        move_cost: u32,
    },
    /// [`TerrainTable::validate_grid`] 在地形网格里发现了一个当前表
    /// 未登记的内容索引——对应存档校验场景下「缺失 mod」的检测点。
    UnregisteredIndex(ContentIndex),
}

impl fmt::Display for TerrainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TerrainError::DuplicateDefinition(index) => {
                write!(f, "地形索引 {} 被重复定义", index.get())
            }
            TerrainError::InconsistentMoveCost {
                blocks_move,
                move_cost,
            } => write!(
                f,
                "blocks_move={blocks_move} 与 move_cost={move_cost} 互相矛盾"
            ),
            TerrainError::UnregisteredIndex(index) => {
                write!(f, "地形索引 {} 未在当前地形表中登记", index.get())
            }
        }
    }
}

impl std::error::Error for TerrainError {}

/// 地形属性的列式存储：按 [`ContentIndex`] 下标索引，不按内容分结构
/// （ADR 0017）。
///
/// 下标空间是**全局** `ContentIndex` 号段的一部分，不是「地形专属」的
/// 连续编号——未来技能/物品等内容类型会与地形共享同一个
/// `Interner`/`Registry`（`ll-mod::registry` 内部只有一个 `Interner`）。
/// 因此这里额外维护一份 `defined` 位图：数组下标落在表范围内不代表
/// 「这是一个地形」，只有 `defined[idx]` 为真才是。
#[derive(Debug, Default, Clone)]
pub struct TerrainTable {
    move_cost: Vec<u32>,
    blocks_sight: Vec<bool>,
    blocks_move: Vec<bool>,
    opens_into: Vec<Option<TerrainKind>>,
    /// [`Self::opens_into`] 的**逆映射**（开启形态 → 关上之后变回哪一
    /// 种），[`Self::define`] 顺手建起来，见 [`Self::closes_into`]。
    ///
    /// 不是列式存储：它的键是「开启形态」这个**值**，不是本表的下标
    /// 空间。`BTreeMap` 不是 `HashMap`——约束 C5。
    closes_into: BTreeMap<TerrainKind, TerrainKind>,
    defined: Vec<bool>,
}

impl TerrainTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上地形属性。
    ///
    /// # 校验（ADR 0017「注册期完整校验」）
    ///
    /// 1. **不得重复定义**——见 [`TerrainError::DuplicateDefinition`]
    ///    文档，这是本任务修掉的已知缺口。
    /// 2. **`blocks_move` 与 `move_cost` 必须自洽**——见
    ///    [`TerrainError::InconsistentMoveCost`] 文档。
    ///
    /// 两条校验都在这里、且只在这里做一次：调用方（[`materialize_base_terrain`]
    /// 或未来的 mod 注册函数）不需要各自重复实现。
    pub fn define(&mut self, index: ContentIndex, attrs: TerrainAttrs) -> Result<(), TerrainError> {
        if attrs.blocks_move {
            if attrs.move_cost != u32::MAX {
                return Err(TerrainError::InconsistentMoveCost {
                    blocks_move: true,
                    move_cost: attrs.move_cost,
                });
            }
        } else if attrs.move_cost == 0 || attrs.move_cost == u32::MAX {
            return Err(TerrainError::InconsistentMoveCost {
                blocks_move: false,
                move_cost: attrs.move_cost,
            });
        }

        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.blocks_sight.resize(new_len, false);
            self.blocks_move.resize(new_len, false);
            self.move_cost.resize(new_len, 0);
            self.opens_into.resize(new_len, None);
        }

        if self.defined[idx] {
            return Err(TerrainError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.blocks_sight[idx] = attrs.blocks_sight;
        self.blocks_move[idx] = attrs.blocks_move;
        self.move_cost[idx] = attrs.move_cost;
        self.opens_into[idx] = attrs.opens_into;
        // 顺手把逆映射建起来，见 `closes_into`。一对多时取索引最小的
        // 那一个（确定性兜底，理由与写法见那个方法的文档）；`define`
        // 的调用顺序不参与判断，只比索引。
        if let Some(open_kind) = attrs.opens_into {
            let closed_kind = TerrainKind(index);
            let entry = self.closes_into.entry(open_kind).or_insert(closed_kind);
            if closed_kind.index() < entry.index() {
                *entry = closed_kind;
            }
        }
        Ok(())
    }

    /// 给定的地形索引当前是否已经登记过属性。
    pub fn is_defined(&self, kind: TerrainKind) -> bool {
        self.defined
            .get(kind.0.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 这张表当前是否一条地形属性都还没登记过。
    ///
    /// 供 [`crate::state::WorldState::assert_terrain_table_loaded`] 这类
    /// 读档后校验点使用——一张刚 `default()` 出来的表（读档后、调用方
    /// 尚未用当前会话重新注册的表）`defined` 恒为空 `Vec`，与「确实
    /// 登记过至少一条属性」在这里可以被可靠区分，不像 `ContentIndex`
    /// 的默认值（0）那样可能和一个合法索引撞车——`Vec::is_empty` 不存
    /// 在这类"占位值恰好等于某个合法值"的歧义。
    pub fn is_empty(&self) -> bool {
        self.defined.is_empty()
    }

    /// 该地形是否阻挡视线。
    ///
    /// 未登记的索引（可能来自被篡改的存档，或引用了当前会话没有加载
    /// 的 mod）用 `debug_assert!` 提示开发期疏漏，release 构建安全
    /// 兜底为 `false`（视为透明）——这是热路径（FOV 逐格调用），不能
    /// 无条件 panic 或打日志，与旧版 `TerrainKind::blocks_sight` 的
    /// 兜底策略一致，见其历史文档。
    pub fn blocks_sight(&self, kind: TerrainKind) -> bool {
        debug_assert!(self.is_defined(kind), "查询未注册的地形: {kind:?}");
        self.blocks_sight
            .get(kind.0.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 该地形是否完全不可通行。未登记索引兜底为 `false`（视为可通行），
    /// 理由同 [`Self::blocks_sight`]。
    pub fn blocks_move(&self, kind: TerrainKind) -> bool {
        debug_assert!(self.is_defined(kind), "查询未注册的地形: {kind:?}");
        self.blocks_move
            .get(kind.0.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 移动经过该地形的代价。未登记索引按平地基准（100）处理——对
    /// 扩展 ID 最安全的兜底，既不无故挡路也不无故挡视线,理由同旧版
    /// `TerrainKind::move_cost` 文档。
    pub fn move_cost(&self, kind: TerrainKind) -> u32 {
        let idx = kind.0.get() as usize;
        if self.blocks_move.get(idx).copied().unwrap_or(false) {
            return u32::MAX;
        }
        self.move_cost.get(idx).copied().unwrap_or(100)
    }

    /// 撞入该地形时变成的另一种地形，见模块文档「`opens_into`」一节。
    pub fn opens_into(&self, kind: TerrainKind) -> Option<TerrainKind> {
        self.opens_into
            .get(kind.0.get() as usize)
            .copied()
            .flatten()
    }

    /// `kind` 这种地形**关上**之后会变成哪一种；不是任何一种「已打开
    /// 形态」时返回 `None`。
    ///
    /// # 为什么是反查 `opens_into`，不是新加一条 `closes_into` 字段
    ///
    /// 「关门」这条能力（交互列表批次）需要的正是 [`Self::opens_into`]
    /// 的**逆映射**：`门关闭 --开--> 门开启`，反过来就是
    /// `门开启 --关--> 门关闭`。新加一条内容字段会
    ///
    /// - 造出第二个真相源（内容作者可以把 `opens_into`/`closes_into`
    ///   写成互相对不上的一对，而没有任何东西会拦），
    /// - 改动 `TerrainDef` 的字段集合，连带要递增
    ///   `ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION`。
    ///
    /// 反查两样都不要，而且**对 mod 自己声明的门自动成立**：任何地形
    /// 只要声明了 `opens_into`，它的目标地形就自动可以被关回去。
    ///
    /// # 一对多时取索引最小的那一个（约束 C5）
    ///
    /// 内容上完全可能有两种不同的关门形态（木门、石门）打开成**同一种**
    /// 开启形态。这时「关上」该变回哪一种没有唯一答案；本方法取
    /// [`ContentIndex`] 最小的那一个——遍历走 `Vec` 下标升序，不经任何
    /// 哈希容器，同一份内容两次调用恒得同一个答案。这不是一条设计裁定，
    /// 是一条**确定性兜底**：真要区分木门石门，正确的修法是让开启形态
    /// 也分成两种（内容侧就能解决），不是在这里猜。
    ///
    /// # 逆映射在 [`Self::define`] 那一刻就建好
    ///
    /// 不是每次查询现扫一遍：`define` 是唯一的写入口，顺手往一张
    /// `BTreeMap` 里记一条即可，查询是一次 `O(log n)` 查表。这也让
    /// 「一对多取最小」那条兜底只在写入侧实现一次。
    pub fn closes_into(&self, kind: TerrainKind) -> Option<TerrainKind> {
        self.closes_into.get(&kind).copied()
    }

    /// 批量校验一整张地形网格：网格里出现的每一个地形索引都必须能在
    /// 本表里查到定义。
    ///
    /// # 与「反序列化」的分工（ADR 0015 / 简报「serde 的 context 张力」）
    ///
    /// [`TerrainKind`] 自身的 `Deserialize` 不做这项校验——`try_from`
    /// 是无状态的静态函数，拿不到「当前注册表里到底有哪些地形」这个
    /// 运行期状态。这个校验因此被推迟到这里：`ChunkGrid` 整体完成
    /// 反序列化之后，调用方显式传入当前会话的 `TerrainTable`，一次性
    /// 校验存档里的每一格。查不到的索引对应规格 §10.4「缺失 mod」这一
    /// 检测点——存档引用了一个当前会话没有加载的地形定义,应该判定
    /// 存档不兼容，而不是静默把它当成某种默认地形继续玩下去。
    pub fn validate_grid(&self, grid: &ChunkGrid) -> Result<(), TerrainError> {
        let world = grid.world();
        for y in 0..world.height() as i32 {
            for x in 0..world.width() as i32 {
                let kind = grid.terrain_at(world.wrap(x, y));
                if !self.is_defined(kind) {
                    return Err(TerrainError::UnregisteredIndex(kind.0));
                }
            }
        }
        Ok(())
    }
}

/// 本体 19 个固定地形在当前注册表里的索引缓存。
///
/// 由 [`materialize_base_terrain`] 在启动时一次性物化。旧版
/// `TerrainKind::MOUNTAIN` 一类编译期字面量的替代——调用方现在写
/// `terrain_ids.mountain`，仍是常量级的字段访问，只是数值不再能在
/// 编译期写死。
#[derive(Debug, Clone, Copy)]
pub struct BaseTerrainIds {
    /// 深水：不可通行，不阻挡视线（水面开阔）。
    pub deep_water: TerrainKind,
    /// 浅水：可通行但比平地慢，不阻挡视线。
    pub shallow_water: TerrainKind,
    /// 沙地：可通行，比平地略慢，不阻挡视线。
    pub sand: TerrainKind,
    /// 草地：可通行，移动代价为基准值，不阻挡视线。
    pub grass: TerrainKind,
    /// 森林：可通行但较慢，树冠阻挡视线。
    pub forest: TerrainKind,
    /// 丘陵：可通行但较慢，不阻挡视线。
    pub hill: TerrainKind,
    /// 山地：可通行但代价极高，山体阻挡视线。
    pub mountain: TerrainKind,
    /// 雪地：可通行但较慢，不阻挡视线。**雪线以上的峰顶**，不是低地
    /// 冻土——后者是 [`Self::tundra`]。
    pub snow: TerrainKind,
    /// 沙漠：可通行，比海岸沙地更费力，不阻挡视线。
    ///
    /// 与 [`Self::sand`] 是**两种地形**，不是同一种的两个名字：`sand`
    /// 是紧贴海平面的海滩（由高度决定），`desert` 是干热带低地（由
    /// 纬度决定，见 [`crate::climate`]）。合并它们会让「骆驼人的家」
    /// 与「渔村的沙滩」在数据上无法区分。
    pub desert: TerrainKind,
    /// 冻原：可通行，比草地费力但比高山积雪好走，不阻挡视线。
    ///
    /// 与 [`Self::snow`] 是**两种地形**，理由同 [`Self::desert`]：`snow`
    /// 是雪线以上的峰顶（由高度决定），`tundra` 是极地带低地的冻土
    /// （由纬度决定）。
    pub tundra: TerrainKind,
    /// 木地板：可通行，移动代价为基准值，不阻挡视线。
    pub floor_wood: TerrainKind,
    /// 石地板：可通行，移动代价为基准值，不阻挡视线。
    pub floor_stone: TerrainKind,
    /// 木墙：不可通行，阻挡视线。
    pub wall_wood: TerrainKind,
    /// 石墙：不可通行，阻挡视线。
    pub wall_stone: TerrainKind,
    /// 关着的门：不可通行，阻挡视线，撞入后变为 [`Self::door_open`]。
    pub door_closed: TerrainKind,
    /// 开着的门：可通行，不阻挡视线。
    pub door_open: TerrainKind,
    /// 窗：不可通行，但不阻挡视线（可隔窗放箭/被看见，刻意设计）。
    pub window: TerrainKind,
    /// 上楼梯：可通行，比平地略慢，不阻挡视线。
    pub stairs_up: TerrainKind,
    /// 下楼梯：可通行，比平地略慢，不阻挡视线。
    pub stairs_down: TerrainKind,
}

/// 本体地形注册的唯一入口：本体与 mod 共用的注册路径。
///
/// `intern` 是外部传入的解析回调（生产路径是 `|id| registry.intern(id)`，
/// 测试/demo 路径是本模块的 [`base_terrain_fixture`]）——本函数只管
/// 「拿到一个索引后，声明它的地形属性」，不关心索引从哪个具体类型来,
/// 这正是保持 `ll-world` 不反向依赖 `ll-mod` 的关键（见模块文档
/// 「与 Registry 的关系」）。
///
/// 19 个地形按固定顺序依次注册（前 17 个与旧版枚举同一顺序，气候条带
/// 新增的沙漠/冻原追加在末尾）——`门关闭` 在
/// `门打开` 之前，但 `opens_into` 引用没有先后限制：`intern` 本身是
/// 幂等的，先为 `lostland:door_open` 换取一个索引、稍后再回来给它
/// `define` 完整属性，两次调用互不冲突。
pub fn materialize_base_terrain(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseTerrainIds, TerrainTable), TerrainError> {
    let mut table = TerrainTable::new();

    let deep_water = define_base(
        &mut table,
        intern,
        "lostland:deep_water",
        false,
        true,
        u32::MAX,
        None,
    )?;
    let shallow_water = define_base(
        &mut table,
        intern,
        "lostland:shallow_water",
        false,
        false,
        200,
        None,
    )?;
    let sand = define_base(&mut table, intern, "lostland:sand", false, false, 120, None)?;
    let grass = define_base(
        &mut table,
        intern,
        "lostland:grass",
        false,
        false,
        100,
        None,
    )?;
    let forest = define_base(
        &mut table,
        intern,
        "lostland:forest",
        true,
        false,
        150,
        None,
    )?;
    let hill = define_base(&mut table, intern, "lostland:hill", false, false, 150, None)?;
    // 刻意不设为不可通行：保留翻山的可能性比一刀切更有玩法空间，极高
    // 的移动代价已经足以让寻路算法在有替代路线时绕开它。
    let mountain = define_base(
        &mut table,
        intern,
        "lostland:mountain",
        true,
        false,
        400,
        None,
    )?;
    let snow = define_base(&mut table, intern, "lostland:snow", false, false, 150, None)?;
    let floor_wood = define_base(
        &mut table,
        intern,
        "lostland:floor_wood",
        false,
        false,
        100,
        None,
    )?;
    let floor_stone = define_base(
        &mut table,
        intern,
        "lostland:floor_stone",
        false,
        false,
        100,
        None,
    )?;
    let wall_wood = define_base(
        &mut table,
        intern,
        "lostland:wall_wood",
        true,
        true,
        u32::MAX,
        None,
    )?;
    let wall_stone = define_base(
        &mut table,
        intern,
        "lostland:wall_stone",
        true,
        true,
        u32::MAX,
        None,
    )?;
    // 撞入即开：opens_into 指向 lostland:door_open，把旧版硬编码在
    // ll-sim::resolve 里的特判收拢成声明式属性（见模块文档）。
    let door_closed = define_base(
        &mut table,
        intern,
        "lostland:door_closed",
        true,
        true,
        u32::MAX,
        Some("lostland:door_open"),
    )?;
    let door_open = define_base(
        &mut table,
        intern,
        "lostland:door_open",
        false,
        false,
        100,
        None,
    )?;
    // 不可通行但不阻挡视线：可以隔窗放箭、也会被隔窗看见，是刻意设计
    // 的战术要素——不要把这一格「修」成阻挡视线。
    let window = define_base(
        &mut table,
        intern,
        "lostland:window",
        false,
        true,
        u32::MAX,
        None,
    )?;
    let stairs_up = define_base(
        &mut table,
        intern,
        "lostland:stairs_up",
        false,
        false,
        150,
        None,
    )?;
    let stairs_down = define_base(
        &mut table,
        intern,
        "lostland:stairs_down",
        false,
        false,
        150,
        None,
    )?;
    // 沙漠与冻原是**气候条带**（规格 §7.1）的落点，不是高度阈值的产物。
    // 两者都追加在既有 17 种之后而不是插在 sand/snow 旁边：插队会平移
    // 其后每一种地形的 ContentIndex，与批次 2「lostland:cultureless 必须
    // 追加在末尾」是同一条纪律。
    //
    // 移动代价的量级：海岸 sand 是 120（浅浅一层沙），沙漠取 140——松软
    // 的深沙更费力，但还不到丘陵/森林那一档（150）。冻原取 130——冻土
    // 是硬的，比草地（100）难走，却比雪线上的松雪 snow（150）好走。
    // 两者都不阻挡视线：与 sand/snow 同属开阔地貌。
    let desert = define_base(
        &mut table,
        intern,
        "lostland:desert",
        false,
        false,
        140,
        None,
    )?;
    let tundra = define_base(
        &mut table,
        intern,
        "lostland:tundra",
        false,
        false,
        130,
        None,
    )?;

    Ok((
        BaseTerrainIds {
            deep_water,
            shallow_water,
            sand,
            grass,
            forest,
            hill,
            mountain,
            snow,
            desert,
            tundra,
            floor_wood,
            floor_stone,
            wall_wood,
            wall_stone,
            door_closed,
            door_open,
            window,
            stairs_up,
            stairs_down,
        },
        table,
    ))
}

/// [`materialize_base_terrain`] 的内部帮手：把一条声明的字面量字段
/// 拆开传入，换取一次 `intern` + 一次 [`TerrainTable::define`]。
///
/// 抽成函数而不是在 [`materialize_base_terrain`] 里内联 19 遍同样的
/// 三步逻辑（解析 id、可选解析 opens_into、写入表），避免十九份几乎
/// 相同的样板代码互相漂移。
#[allow(clippy::too_many_arguments)]
fn define_base(
    table: &mut TerrainTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    id: &str,
    blocks_sight: bool,
    blocks_move: bool,
    move_cost: u32,
    opens_into: Option<&str>,
) -> Result<TerrainKind, TerrainError> {
    let index = intern(NamespacedId::parse(id).expect("本体地形 id 字面量恒合法"));
    let opens_into = opens_into.map(|target| {
        TerrainKind(intern(
            NamespacedId::parse(target).expect("本体地形 id 字面量恒合法"),
        ))
    });
    table.define(
        index,
        TerrainAttrs {
            blocks_sight,
            blocks_move,
            move_cost,
            opens_into,
        },
    )?;
    Ok(TerrainKind(index))
}

/// 供测试与验收 demo 使用：现造一个空 [`Interner`]，注册本体全部 19
/// 个地形，返回可用的 `(BaseTerrainIds, TerrainTable)`。
///
/// **不是生产路径**——生产路径必须经过 `ll-mod::Registry::intern`（见
/// 模块文档「与 Registry 的关系」）。这个函数只是让 `ll-world` 自身、
/// 以及下游 `ll-sim`/`ll-script` 的单元测试与验收 demo，不必先搭一整套
/// mod 加载流程就能拿到一份内部自洽的地形表。
pub fn base_terrain_fixture() -> (BaseTerrainIds, TerrainTable) {
    let mut interner = Interner::new();
    materialize_base_terrain(&mut |id| interner.intern(id))
        .expect("本体地形声明表内部一致，注册恒不失败")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 新建的地形表是空的() {
        // Arrange & Act
        let table = TerrainTable::new();

        // Assert
        assert!(table.is_empty());
    }

    #[test]
    fn 登记过至少一条属性的地形表不再是空的() {
        // Arrange
        let (_ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!table.is_empty());
    }

    #[test]
    fn 山地阻挡视线() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(ids.mountain.blocks_sight(&table));
    }

    #[test]
    fn 草地不阻挡视线() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!ids.grass.blocks_sight(&table));
    }

    #[test]
    fn 深水不可通行() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(ids.deep_water.blocks_move(&table));
    }

    #[test]
    fn 浅水可以通行() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!ids.shallow_water.blocks_move(&table));
    }

    #[test]
    fn 浅水的移动代价高于草地() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act
        let shallow_water_cost = ids.shallow_water.move_cost(&table);
        let grass_cost = ids.grass.move_cost(&table);

        // Assert
        assert!(shallow_water_cost > grass_cost);
    }

    #[test]
    fn 山可以通行() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!ids.mountain.blocks_move(&table));
    }

    #[test]
    fn 山的移动代价远高于平地() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act
        let mountain_cost = ids.mountain.move_cost(&table);
        let grass_cost = ids.grass.move_cost(&table);

        // Assert
        assert!(mountain_cost > grass_cost * 2);
    }

    #[test]
    fn 森林阻挡视线() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(ids.forest.blocks_sight(&table));
    }

    #[test]
    fn 森林可以通行() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!ids.forest.blocks_move(&table));
    }

    #[test]
    fn 窗不可通行() {
        // 这是刻意设计而非疏漏：窗户可以隔窗放箭、也会被隔窗看见，
        // 详见模块文档。不要把这条断言删掉或改成 assert!(!...)——
        // 那意味着有人把窗「修」成了墙。
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(ids.window.blocks_move(&table));
    }

    #[test]
    fn 窗不阻挡视线() {
        // 与上一条断言配对：窗挡路但不挡视线，这是刻意设计而非疏漏。
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!ids.window.blocks_sight(&table));
    }

    #[test]
    fn 关着的门不可通行() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(ids.door_closed.blocks_move(&table));
    }

    #[test]
    fn 关着的门阻挡视线() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(ids.door_closed.blocks_sight(&table));
    }

    #[test]
    fn 关着的门撞入后变为开着的门() {
        // 新增断言：opens_into 把旧版硬编码在 ll-sim::resolve 里的
        // 「撞门即开」特判收拢成了声明式属性，这里钉住这条声明本身
        // 是对的。
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act
        let opens_into = ids.door_closed.opens_into(&table);

        // Assert
        assert_eq!(opens_into, Some(ids.door_open));
    }

    #[test]
    fn 开着的门可以通行() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!ids.door_open.blocks_move(&table));
    }

    #[test]
    fn 开着的门不阻挡视线() {
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert!(!ids.door_open.blocks_sight(&table));
    }

    #[test]
    fn 不可通行地形的移动代价为最大值() {
        // 用 u32::MAX 而非 Option，让寻路算法不必对每格做分支判断。
        // Arrange
        let (ids, table) = base_terrain_fixture();

        // Act & Assert
        assert_eq!(ids.deep_water.move_cost(&table), u32::MAX);
    }

    #[test]
    fn 本体地形可以正常往返() {
        // Arrange
        let (ids, _table) = base_terrain_fixture();
        let json = serde_json::to_string(&ids.mountain).expect("本体地形必然可序列化");

        // Act
        let decoded: TerrainKind = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, ids.mountain);
    }

    #[test]
    fn 未在注册表登记的地形反序列化本身不报错() {
        // ADR 0015：serde 只管结构 ↔ 字符串，不做「是否已注册」的
        // 校验——那是运行期、有注册表可查时才能回答的问题。裸整数
        // 9999 结构上就是一个合法的 TerrainKind。
        // Arrange
        let json = "9999";

        // Act
        let result: Result<TerrainKind, _> = serde_json::from_str(json);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // 这是简报要求正面处理的已知缺口：两个 mod（或某 mod 与本体）
        // 都尝试给同一个内容索引定义地形属性时，第二次必须报错，不能
        // 静默覆盖第一次的结果——否则玩家看到的地形行为会莫名其妙。
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:grass").expect("合法"));
        let mut table = TerrainTable::new();
        table
            .define(
                index,
                TerrainAttrs {
                    blocks_sight: false,
                    blocks_move: false,
                    move_cost: 100,
                    opens_into: None,
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            TerrainAttrs {
                blocks_sight: true,
                blocks_move: true,
                move_cost: u32::MAX,
                opens_into: None,
            },
        );

        // Assert
        assert_eq!(result, Err(TerrainError::DuplicateDefinition(index)));
    }

    #[test]
    fn 声明不可通行却给出有限移动代价时注册失败() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("yourmod:broken").expect("合法"));
        let mut table = TerrainTable::new();

        // Act
        let result = table.define(
            index,
            TerrainAttrs {
                blocks_sight: false,
                blocks_move: true,
                move_cost: 100,
                opens_into: None,
            },
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 声明可通行却给出零移动代价时注册失败() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("yourmod:broken").expect("合法"));
        let mut table = TerrainTable::new();

        // Act
        let result = table.define(
            index,
            TerrainAttrs {
                blocks_sight: false,
                blocks_move: false,
                move_cost: 0,
                opens_into: None,
            },
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    #[should_panic(expected = "查询未注册的地形")]
    fn 未登记的地形索引查询在调试构建下触发断言() {
        // 对应旧版「debug_assert! 只在 debug 构建生效，用于提示本体
        // 新增地形却忘了登记，或调用方传入了垃圾索引」的开发期安全网
        // ——`cargo test` 默认是 debug 构建（`debug_assertions` 开启），
        // 这里直接验证断言确实会触发,而不是试图观测 release 构建下才
        // 会走到的兜底数值（同一进程内无法既触发 debug_assert 又继续
        // 往下执行到达那行兜底代码，两者互斥，见 blocks_move 文档）。
        // Arrange
        let mut interner = Interner::new();
        let unregistered = TerrainKind(
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法")),
        );
        let table = TerrainTable::new();

        // Act
        let _ = unregistered.blocks_move(&table);
    }

    #[test]
    fn 地形网格里出现未登记索引时批量校验失败() {
        // 对应「未在当前注册表里出现的地形索引，反序列化/校验时被
        // 拒绝」这条 TDD 要求——校验入口见 TerrainTable::validate_grid
        // 文档「与反序列化的分工」一节。
        // Arrange：必须用同一个 Interner 先注册本体 19 个地形、再追加
        // 一个「表里从未登记过」的索引——另起一个全新 Interner 会从 0
        // 开始重新分配，恰好撞上本体已经登记过的下标（0 号是
        // deep_water），那样测出的就不是「未登记」而是巧合撞号。
        let mut interner = Interner::new();
        let (_ids, table) = materialize_base_terrain(&mut |id| interner.intern(id))
            .expect("本体地形声明表内部一致");
        // 真实场景对应存档引用了当前会话没有加载的 mod 内容。
        let unregistered =
            TerrainKind(interner.intern(NamespacedId::parse("yourmod:ghost").expect("合法")));
        let world = ll_core::torus::TorusSize::new(64, 64).expect("64x64 合法");
        let mut grid = ChunkGrid::new(world, unregistered).expect("64x64 满足视口跨度");
        grid.set_terrain(world.wrap(5, 5), unregistered);

        // Act
        let result = table.validate_grid(&grid);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 地形网格全部索引已登记时批量校验通过() {
        // Arrange
        let (ids, table) = base_terrain_fixture();
        let world = ll_core::torus::TorusSize::new(64, 64).expect("64x64 合法");
        let mut grid = ChunkGrid::new(world, ids.deep_water).expect("64x64 满足视口跨度");
        grid.set_terrain(world.wrap(5, 5), ids.mountain);

        // Act
        let result = table.validate_grid(&grid);

        // Assert
        assert!(result.is_ok());
    }
}
