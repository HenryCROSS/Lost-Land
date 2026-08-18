//! 目标栈：`Agent` 想要什么。
//!
//! 完整的目标链、需求分解、任务发布机制冻结在
//! `knowledge/design/agent-goals-and-economy.md` 第二节，实现阶段是
//! P8。本任务只建 P3 建 [`crate::entity::Agent`] 时必须已经存在的字段
//! 布局——P3 阶段这个栈可以留空，但字段必须现在就有，否则 P8 补上就要
//! 写存档迁移链（存档格式在 P5 冻结，P3 加是零成本，P8 加不是）。

use ll_core::ident::ContentIndex;

/// 一条目标：类型、参数、进度、优先级。
///
/// `kind` 用 [`ContentIndex`] 指向注册表，与 [`crate::entity::Agent::profession`]
/// 同理——不派生 `serde`：`ContentIndex` 是运行时紧凑索引，依赖 mod
/// 加载顺序，`ll_core::ident` 模块文档明确写着「不可持久化」。真正持久化
/// 目标栈需要把 `kind` 解析回 [`ll_core::ident::NamespacedId`] 字符串
/// 再重新登记，这属于内容注册表的存档格式，不在本任务范围内。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    /// 目标类型，指向注册表（mod 可扩展）。
    pub kind: ContentIndex,
    /// 目标参数（开哪种店、要多少铁）。
    pub params: Vec<i64>,
    /// 已推进进度，千分比。
    pub progress: i32,
    /// 优先级，决定钱包紧张时先满足哪个。
    pub priority: i32,
}
