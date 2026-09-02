//! 树木结算那三个索引的**生产解析点**：把 `lostland:forest`、
//! `lostland:timber`、`lostland:tree_seed` 三个标识符解析成
//! [`ContentIndex`]，实现 [`ll_sim::tree::TreeCatalog`]。
//!
//! 依赖倒置在树木上的落点，与 [`crate::quest::RegisteredQuests`]、
//! [`crate::recipe::RegisteredRecipes`] 完全同一种形状：`ll-sim` 声明接口，
//! 真正的 [`Registry`](crate::registry::Registry) 住在这里。
//!
//! # 为什么树种在引擎侧、这三个索引却在内容侧
//!
//! 两件不同的事，分界在「它有没有内容表」：
//!
//! - **树种**（橡/松/棕榈）是 `ll_world::tree::TreeSpecies`，一个引擎侧
//!   静态枚举——它没有内容表，也**不该有**：派生层是每帧上千次的纯函数，
//!   让它查注册表就毁掉了那条性质（论证见该枚举文档）。
//! - **木料与树种（种子）是物品**，物品有内容表，因此它们是
//!   `mods/lostland/items.json5` 里两条货真价实的内容，与其余三十六件
//!   一样走注册表。
//!
//! # 三个 id 拼错了会怎样：**静默失效**，本模块如实登记这一点
//!
//! [`RegisteredTrees`] 三条查询任何一条返回 `None`，
//! `ll_sim::resolve` 的树木结算就恒产出**空效果**——玩家按下「砍伐」
//! 什么都不发生，**不报错、不打日志**。这与家具贴图查不到时静默退回
//! 通用记号是同一类失效模式。
//!
//! **本模块刻意没有把这三条升级成 `base_contract` 的硬性要求**（那会让
//! 缺内容变成一次响亮的装载失败）。理由是范围：`base_contract` 是一份
//! 「Rust 点名引用了哪些本体内容」的清单，往里加东西要同时改它的错误
//! 类型与玩家可见的报错文案，属另一个批次。**代价换成了一条会红的
//! 断言**：`crates/ll-game/tests/tree_end_to_end.rs` 的
//! `真实内容装载之后三个树木索引都解析得到` 走生产装载路径逐条确认。
//! 拼错任何一个 id，那条当场红。
//!
//! **下一个读到这里的人**：若要把这三条搬进 `base_contract`，本段就是
//! 那件事的出发点，同时把上面那条断言改成「装载直接失败」。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::tree::TreeCatalog;

use crate::registry::Registry;

/// `lostland:forest` ——树的派生层只在这种地形上长树。
const FOREST_ID: &str = "lostland:forest";
/// `lostland:timber_log` ——砍伐产出。
///
/// # 为什么不叫 `lostland:timber`
///
/// **那个 id 已经被占了**：`mods/lostland/resources.json5` 里的据点资源
/// 「木材」就叫 `lostland:timber`。注册表是**一个** id ↔ 索引空间，
/// 两张不同的表用同一个 id 会让它们共享同一个 `ContentIndex`
/// ——`ItemTable::get(那个索引)` 与 `ResourceTable::get(那个索引)` 从此
/// 都答得出来，而「这个索引到底是哪张表的」再也问不清楚。
///
/// **本批真的踩了这一脚**：第一版就叫 `lostland:timber`，实测两者同为
/// 索引 40（探针输出记在计划文档十节）。发现它的不是 schema 校验、也
/// 不是 `content_audit`——是「新增两条内容却只让后续索引平移了 1」这个
/// 对不上的数字。**没有任何门禁拦住它**，这笔账如实记在这里。
const TIMBER_ID: &str = "lostland:timber_log";
/// `lostland:tree_seed` ——采果产出，培植消耗。
const TREE_SEED_ID: &str = "lostland:tree_seed";

/// 把一份 [`Registry`] 包成 [`TreeCatalog`]。
///
/// # 为什么每次查询都现解析，而不是构造时缓存三个索引
///
/// 三条查询各自只是一次 `Registry::get`（一次哈希表**单键查找**，不遍历
/// ——约束 C5 允许的那一种用法），而它们的调用点是
/// `resolve_tend_tree`：**一次玩家操作调一次**，不是渲染热路径（树的
/// 渲染走 `ll_world::tree::tree_at`，一个索引都不查）。缓存换来的是
/// 「构造时 mod 还没装完怎么办」这个新问题，换不到任何可测的收益。
pub struct RegisteredTrees<'a> {
    /// 与内容出自同一次装载会话的注册表。
    pub registry: &'a Registry,
}

impl RegisteredTrees<'_> {
    /// 解析一个字面 id；没注册就是 `None`（ADR 0015：「结构合法」与
    /// 「已注册」是两件事，查不到就是查不到，不 panic）。
    fn index_of(&self, id: &str) -> Option<ContentIndex> {
        self.registry.get(&NamespacedId::parse(id).ok()?)
    }
}

impl TreeCatalog for RegisteredTrees<'_> {
    fn forest_terrain(&self) -> Option<ContentIndex> {
        self.index_of(FOREST_ID)
    }

    fn timber(&self) -> Option<ContentIndex> {
        self.index_of(TIMBER_ID)
    }

    fn tree_seed(&self) -> Option<ContentIndex> {
        self.index_of(TREE_SEED_ID)
    }
}
