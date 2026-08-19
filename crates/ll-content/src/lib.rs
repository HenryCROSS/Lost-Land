//! 迷途大陆的存档格式与内容加载层。
//!
//! 承接规格 §5 给 `ll-content` 的职责：数据表加载、存档序列化与
//! 迁移。存档头/主体的读写逻辑、schema 迁移函数链、双模式降级标记都
//! 落在这个 crate，不塞进 `ll-world`——`ll-world` 只负责「`WorldState`
//! 这个类型本身能不能序列化」，不负责「存档文件长什么样、怎么落
//! 盘」。
//!
//! # 依赖方向
//!
//! 规格 §5：`ll-core` ← … ← `ll-world` ← `ll-sim` ← `ll-script` ←
//! `ll-mod` ← `ll-content` ← `ll-ui`。本 crate 依赖 `ll-core`
//! （`NamespacedId` 解析）与 `ll-mod`（`Registry` 快照/重建），不得被
//! 两者、也不得被更下游的任何 crate 反向依赖。
//!
//! # 模块划分（本批次落地的部分）
//!
//! - [`header`] —— 存档头骨架：明文 JSON,不引用 `ContentIndex`（见
//!   其模块文档「为什么头部不能引用 `ContentIndex`」）。
//! - [`migration`] —— schema 版本迁移框架：按起始版本串联起迁移函数,
//!   与 mod 版本是两条正交的失败轴（详见其模块文档）。
//! - [`content_index_map`] —— 把 `Registry::snapshot()`/
//!   `Registry::rebuild_from()` 真正接入存档头 `content_index_map`
//!   字段的读写路径。
//!
//! 存档主体读写管线（`save_file`）、世界身份（`world_identity`）、
//! 缺失 mod 降级策略（`degrade`）等后续任务的模块留给各自任务落地,
//! 本 crate 当前只包含以上三个模块。

pub mod content_index_map;
pub mod header;
pub mod migration;
