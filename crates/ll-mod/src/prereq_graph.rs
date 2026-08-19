//! 前置关系 DAG 的通用无环校验——`SkillTable`（P5-B 任务 3）与
//! `QuestTable`（P5-B 任务 6）共用同一套算法。
//!
//! # 为什么现在才抽出来
//!
//! 任务 3 落地时，[`crate::skill::validate_no_cycles`] 直接耦合了
//! `SkillTable`/`SkillError` 两个具体类型——当时只有一张需要无环校验
//! 的表，没有抽象的必要（YAGNI）。任务 6（本模块的落点）引入第二张
//! 需要同一种校验的表（`QuestTable`），实施计划任务 6 一节明确要求
//! 「若任务 3 的 `validate_no_cycles` 设计得足够通用，本任务应该直接
//! 复用同一个函数，只是换一张表」——当时的实现并不通用，因此这里把
//! 核心的三色 DFS 算法抽成一个只依赖 [`PrerequisiteGraph`] trait 的
//! 通用函数，`skill.rs`/`quest.rs` 各自只保留「怎么把自己的表适配成
//! 这个 trait」「怎么把通用错误映射回自己的 Error 类型」这两层薄
//! 包装，不再各自维护一份 DFS。
//!
//! # 遍历顺序确定性（约束 C5）
//!
//! 与 [`crate::topo::topo_sort`]/原 `skill.rs` 实现同一条纪律：DFS 起点
//! 按 [`ContentIndex::get`] 数值升序排列，不依赖调用方传入
//! `defined_ids` 的原始顺序（`define` 的调用顺序）——即便两次调用之间
//! 注册顺序不同，只要最终登记的节点集合与前置关系相同，报告出来的
//! 具体环路也恒定不变。三色标记用 `BTreeMap<ContentIndex, Color>`
//! 而不是按下标索引的 `Vec`——不要求调用方额外暴露"槽位总数"这个内部
//! 存储细节，颜色状态本身也天然只需要按 `ContentIndex` 键查找，不依赖
//! 任何 `HashMap`/`HashSet` 迭代顺序。

use std::collections::BTreeMap;

use ll_core::ident::ContentIndex;

/// [`validate_no_cycles`] 需要从具体注册表读到的最小信息：一个节点是否
/// 已注册、它的前置列表是什么。
///
/// `SkillTable`/`QuestTable` 各自在自己的模块里实现这个 trait——两者的
/// 列式存储内部结构完全不同，但都能回答这两个问题，这正是可以共用同一
/// 套图算法的原因。
pub(crate) trait PrerequisiteGraph {
    /// 给定索引当前是否已经登记过属性。
    fn is_defined(&self, node: ContentIndex) -> bool;
    /// 给定节点的前置列表——调用前提是 `is_defined(node)` 为真。
    fn prerequisites(&self, node: ContentIndex) -> &[ContentIndex];
}

/// [`validate_no_cycles`] 发现的问题，与具体是哪张表无关——调用方把
/// 这个通用错误映射回自己的 `SkillError`/`QuestError`。
pub(crate) enum CycleError {
    /// 某节点的前置列表里引用了一个当前图里从未登记过的索引。
    UnregisteredPrerequisite {
        /// 声明了这条悬空前置的节点。
        node: ContentIndex,
        /// 被引用但未登记的索引。
        missing: ContentIndex,
    },
    /// 前置关系构成环，附带环路上具体的节点（按环路顺序）。
    Cycle(Vec<ContentIndex>),
}

/// 三色标记：灰色是当前递归路径上尚未退栈的节点，再次访问到灰色节点
/// 即找到一条环。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    White,
    Gray,
    Black,
}

/// 给定全部已注册节点（`defined_ids`）与它们的前置关系（经 `graph`
/// 查询），是否存在环；顺带校验每条前置是否指向一个真实登记过的节点。
///
/// 见模块文档「遍历顺序确定性」一节：起点按 [`ContentIndex::get`]
/// 数值升序尝试，与 `defined_ids` 的原始顺序无关。
pub(crate) fn validate_no_cycles<G: PrerequisiteGraph>(
    graph: &G,
    defined_ids: &[ContentIndex],
) -> Result<(), CycleError> {
    let mut order = defined_ids.to_vec();
    order.sort_by_key(ContentIndex::get);

    let mut color: BTreeMap<ContentIndex, Color> = BTreeMap::new();
    let mut path: Vec<ContentIndex> = Vec::new();

    for start in order {
        if color_of(&color, start) == Color::White {
            visit(start, graph, &mut color, &mut path)?;
        }
    }

    Ok(())
}

/// 查询某节点当前的三色标记，未出现在映射里即视为白色（尚未访问）——
/// 用一个不存在的键代表"还没访问过"，避免为全部节点预先插入白色条目。
fn color_of(color: &BTreeMap<ContentIndex, Color>, node: ContentIndex) -> Color {
    color.get(&node).copied().unwrap_or(Color::White)
}

/// [`validate_no_cycles`] 的递归帮手：对 `node` 做一次 DFS。
fn visit<G: PrerequisiteGraph>(
    node: ContentIndex,
    graph: &G,
    color: &mut BTreeMap<ContentIndex, Color>,
    path: &mut Vec<ContentIndex>,
) -> Result<(), CycleError> {
    color.insert(node, Color::Gray);
    path.push(node);

    for &prereq in graph.prerequisites(node) {
        if !graph.is_defined(prereq) {
            return Err(CycleError::UnregisteredPrerequisite {
                node,
                missing: prereq,
            });
        }
        match color_of(color, prereq) {
            Color::White => visit(prereq, graph, color, path)?,
            Color::Gray => {
                let start = path
                    .iter()
                    .position(|&x| x == prereq)
                    .expect("灰色节点必然仍在当前递归路径上");
                return Err(CycleError::Cycle(path[start..].to_vec()));
            }
            Color::Black => {}
        }
    }

    path.pop();
    color.insert(node, Color::Black);
    Ok(())
}
