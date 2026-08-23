//! 迷途大陆的脚本宿主层：Steel VM 封装、内存守卫、mod API 表面。
//!
//! 设计基线见 `knowledge/decisions/0001-steel-sandbox-verification.md`
//! （中断机制实测）与 `knowledge/decisions/0012-steel-capability-surface-verification.md`
//! （标准库能力面实测）——本 crate 的所有取舍都能在这两份 ADR 里找到实测
//! 依据，不是假设。

pub mod alloc_guard;
pub mod api;
pub mod behavior;
pub mod host;
pub mod modules;
pub mod whitelist;

pub use alloc_guard::ScriptAllocGuard;
pub use host::{ScriptEngine, ScriptError};
pub use modules::ModuleTable;
