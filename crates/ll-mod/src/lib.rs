//! 迷途大陆的 mod 框架核心。
//!
//! 承接规格 §10「脚本层与 Mod 框架」的加载管线前半段（发现 → 解析清单
//! → 依赖拓扑排序）与「本体即 Mod」原则落地所需的内容注册表。本体的
//! 全部内容——职业、技能、物品、地形——都通过与 mod 完全相同的注册
//! 通道登记，注册表本身不区分「这是本体注册的还是 mod 注册的」，只认
//! 命名空间字符串（[`registry`]）。
//!
//! # 模块划分
//!
//! - [`discover`] / [`manifest`] / [`topo`] —— 加载管线的发现、解析、
//!   排序三个阶段。规格 §10.2 第四道防线（加载分阶段隔离）要求这三个
//!   阶段互相独立：一个 mod 的清单解析失败，不影响其他 mod 被发现和
//!   尝试解析。
//! - [`registry`] —— 内容注册表核心：字符串 ID ↔ 紧凑整数索引
//!   （[`ll_core::ident::ContentIndex`]）的双向映射，以及按 mod
//!   命名空间统计的内容哈希。
//! - [`mod_set`] —— 存档需要的「生成期 mod 集合」与「当前 mod 集合」
//!   两种记录，类型层面强制区分，防止只存一份导致种子分享/缺陷复现/
//!   回归测试失效（见 `knowledge/design/identity-and-ids.md`
//!   「存档与 mod 集合」）。
//! - [`base_terrain`] —— 「本体即 Mod」的第一次真实验收（P4 Task 8）：
//!   把 `ll_world::terrain` 定义的地形声明表，经 [`registry::Registry::intern`]
//!   注册成本体的第一批内容。本体地形与 mod 未来注册的地形走的是
//!   完全相同的这一条 `Registry::intern` 调用路径，见该模块文档。
//! - [`base_space_profile`] —— 同一个模式在 `ll_world::space_profile`
//!   上的落点（两级坐标系重写批次 C 补齐）：把本体的地表/洞窟/地下城/
//!   建筑内部四种基础空间类型注册进 [`registry::Registry`]。
//! - [`class`] —— 职业注册表（P5-B 任务 2）：`ClassDef` 的定义直接落在
//!   本 crate（不像地形那样拆成 `ll-world` 定义 + `ll-mod` 薄封装两处
//!   ——职业不依赖任何世界空间概念，见该模块文档「为什么定义本身直接
//!   落在 `ll-mod`」一节）。
//! - [`skill`] —— 技能注册表 + 前置关系 DAG 校验（P5-B 任务 3）：技能树
//!   的「解锁」判定要求前置关系无环，注册期用拓扑着色法检测环并报告
//!   具体环路，见该模块文档。
//! - [`base_placeholder`] —— 同一个模式在「占位/未知内容」上的落点
//!   （P5-A 任务 14 补齐）：把本体的占位内容注册进
//!   [`registry::Registry`]，让 NPC 种族缺失的占位降级分支在生产读档
//!   管线里真正可达。
//! - [`script_terrain_api`] —— 把 `register-terrain` 注册进
//!   `ll_script::host::ScriptEngine`，供 mod 脚本定义自定义地形（Task
//!   11/12）。
//! - [`pipeline`] —— 加载管线：串起发现→解析→拓扑排序→加载脚本→注册
//!   内容，产出 [`load_report::LoadReport`]（Task 11/12）。
//! - [`load_report`] —— 加载管理界面（`ll-ui`）依赖的数据形状：按 mod
//!   归类的加载结果、失败阶段、尽力而为的源码位置（Task 11）。
//!
//! # 依赖方向
//!
//! 规格 §5：`ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod` ← `ll-ui`。
//! 本 crate 依赖 `ll-core`、`ll-world`（Task 8 新增，理由见
//! [`base_terrain`] 模块文档）与 `ll-script`（Task 11 新增，理由见
//! [`pipeline`] 模块文档），不得被下游任何 crate 反向依赖。

pub mod base_placeholder;
pub mod base_space_profile;
pub mod base_terrain;
pub mod class;
pub mod discover;
pub mod load_report;
pub mod manifest;
pub mod mod_set;
pub mod pipeline;
pub mod registry;
pub mod script_terrain_api;
pub mod skill;
pub mod topo;
