//! 迷途大陆的回合与模拟层。
//!
//! 本批次是占位 crate：实体存储（原 `entity` 模块）与名字生成（原
//! `naming` 模块）已迁移到 `ll-world`——两者是世界的**状态**（居民），
//! 不是**演化**逻辑，依赖方向也要求状态所在的 crate 不能反过来依赖
//! 演化所在的 crate（见规格 §5 的依赖顺序：`ll-world` ← `ll-sim`）。
//!
//! 时间轴调度器（[`timeline`]）、`Intent`（[`intent`]）、`Effect`
//! （[`effect`]）、唯一写入口 [`apply::apply`]、`resolve`（[`resolve`]，
//! 从 `Intent` 结合世界状态产出 `Effect`）与战斗数值公式（[`combat`]）
//! 均已实现，见 `docs/superpowers/specs/2026-08-17-p3-turn-combat.md`。
//! `resolve` 需要读地形与 FOV（定义在 `ll-world`），所以本 crate 依赖
//! `ll-world`，而不是反过来。

pub mod apply;
pub mod combat;
pub mod effect;
pub mod intent;
pub mod resolve;
pub mod timeline;
