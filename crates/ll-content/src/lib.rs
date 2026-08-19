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
//! - [`world_identity`] —— 世界身份三要素（种子/尺寸/生成期 mod
//!   集合）的推荐预设与校验，生成期 mod 集合的绑定时机。
//! - [`degrade`] —— 缺失 mod 降级策略：按内容类型分级 + 只读模式。
//! - [`load_error`] —— schema 版本与 mod 版本两条正交失败轴的分类
//!   报错。
//! - [`mode`] —— 双模式存档（纯永久死亡 / 自由读档）与单向降级
//!   （任务 10）。
//! - [`remap`] —— 存档主体读入后的 `ContentIndex` 重映射（任务 9）。
//! - [`save_file`] —— 存档主体读写管线：把以上模块串成一条完整的
//!   存档 → 读档路径（任务 9）。

pub mod content_index_map;
pub mod degrade;
pub mod header;
pub mod load_error;
pub mod migration;
pub mod mode;
pub mod remap;
pub mod save_file;
pub mod world_identity;
