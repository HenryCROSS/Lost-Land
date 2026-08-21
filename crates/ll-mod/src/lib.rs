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
//! - [`content_hash`] —— 内容哈希的值哈希升级：把
//!   [`registry::Registry`] 只追踪 id 集合的哈希，升级为同时追踪六张
//!   内容表的字段值。装载完成后一次性调用，见该模块文档「为什么不能
//!   在 `intern` 内部做」一节。
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
//! - [`subclass`] —— 副职注册表（P5-B 任务 4）：裁定 P5-4（主职与副职
//!   共享技能命名空间）在代码层面的落点，`SubclassDef` 本身不携带任何
//!   命名空间字段，见该模块文档。
//! - `prereq_graph`（crate 内部，不对外公开）—— [`skill`]/[`quest`] 共用
//!   的前置关系 DAG 无环校验算法（P5-B 任务 6 从任务 3 的实现里抽出）。
//! - [`quest`] —— 网状任务图注册表 + 任务进度持久化（P5-B 任务 6/7）：
//!   `QuestNodeDef` 的前置列表是单一真相源，`unlocked_by` 现算「解锁了
//!   哪些后续任务」；任务进度（"这个实体完成了哪些任务节点"）走脚本
//!   状态存储的每实体存储，不是 `Agent` 字段，见该模块文档。
//! - [`quest_overview`] —— 任务日志 UI 数据层（P5-B 任务 8）：给定
//!   `Agent`/`QuestTable`/`Registry`，返回一份纯数据的
//!   `QuestLogView`（已完成/已解锁未完成），不含任何渲染，见该模块
//!   文档。
//! - [`base_placeholder`] —— 同一个模式在「占位/未知内容」上的落点
//!   （P5-A 任务 14 补齐）：把本体的占位内容注册进
//!   [`registry::Registry`]，让 NPC 种族缺失的占位降级分支在生产读档
//!   管线里真正可达。
//! - [`race`] —— 种族注册表（P5-C 缺口修补批次）：`RaceDef`/`RaceTable`
//!   落地 `knowledge/design/race-system.md`「核心形状」一节的设计，与
//!   [`base_placeholder`] 的占位种族索引协调（互不冲突，占位索引在
//!   `RaceTable` 里查询恒返回 `None`），见其模块文档。
//! - [`base_race`] —— 同一个模式在种族上的生产注册入口，照
//!   [`base_terrain`]。
//! - [`trait_def`] —— 天赋注册表（天赋系统落地批次）：
//!   `knowledge/design/trait-system.md` 四节落地，`TraitDef` 是独立
//!   内容类型，被种族（本批次唯一接入的所有者）通过
//!   `Vec<ll_sim::traits::TraitGrant>` 引用，`TraitTable` 实现
//!   `ll_sim::traits::TraitCatalog`（依赖倒置，见该 trait 模块文档），
//!   见其模块文档「本批次范围」一节的完整裁定。
//! - [`clip`] —— 动画剪辑注册表（动画剪辑接线批次）：把
//!   `ll_render::anim::Clip`（此前只能写死在 Rust 里的动画帧序列/节奏/
//!   循环声明，见其模块文档起因）做成可注册内容，`ClipDef` 与 `class`/
//!   `race` 同一个理由直接落在本 crate；`exit_grace_frames` 是否暴露给
//!   脚本的结论见该模块文档。
//! - [`base_clip`] —— 同一个模式在动画剪辑上的生产注册入口，照
//!   [`base_terrain`]/[`base_race`]；本体行走/待机两段剪辑的唯一权威
//!   数据来自 `ll_render::anim::base_hero_clips`（不是本模块自己另抄
//!   一份，见其文档）。
//! - [`active_registry`] —— 装载会话内唯一共享的活跃 `Registry`，供
//!   全部 `register-*` 脚本注册函数在同一次脚本求值窗口内共用（P5-C
//!   接线批次新增：此前只有 `register-terrain` 一个注册函数，`Registry`
//!   可以整个打包进地形表自己的 `thread_local!`；补齐职业/技能/副职/
//!   任务/种族五类注册函数后，多个注册函数必须共享同一个 `Registry`
//!   实例才能保证 `ContentIndex` 号段不冲突，见其模块文档）。
//! - [`script_terrain_api`] —— 把 `register-terrain` 注册进
//!   `ll_script::host::ScriptEngine`，供 mod 脚本定义自定义地形（Task
//!   11/12）。
//! - [`script_class_api`]/[`script_skill_api`]/[`script_subclass_api`]/
//!   [`script_quest_api`]/[`script_race_api`] —— 同一个模式在职业/技能/
//!   副职/任务/种族上的脚本绑定（P5-C 缺口修补批次）：补上
//!   [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
//!   判定为「玩法层」、但此前只有纯 Rust 函数调用能触达、脚本完全够
//!   不到的四类 + 种族共五类注册 API。
//! - [`script_trait_api`] —— 同一个模式在天赋上的脚本绑定（天赋系统
//!   落地批次）：`register-trait`，见 [`trait_def`] 模块文档「本批次
//!   范围」一节；`register-race-trait`（给种族追加天赋引用）挂在
//!   [`script_race_api`] 而不是这里——理由同 `register-race-xp-reward`
//!   挂在同一个文件（追加对象是 `RaceTable`，不是 `TraitTable`）。
//! - [`script_clip_api`] —— 同一个模式在动画剪辑上的脚本绑定（动画
//!   剪辑接线批次）：补上此前完全漏掉的第七类可注册玩法层内容——早先
//!   的接口审计列出六种，「动画剪辑」当时不在其中。
//! - [`pipeline`] —— 加载管线：串起发现→解析→拓扑排序→加载脚本→注册
//!   内容，产出 [`load_report::LoadReport`]（Task 11/12；P5-C 批次扩展
//!   到同时接线六种 `register-*` 函数；动画剪辑接线批次扩展到七种）。
//! - [`load_report`] —— 加载管理界面（`ll-ui`）依赖的数据形状：按 mod
//!   归类的加载结果、失败阶段、尽力而为的源码位置（Task 11）。
//! - [`script_behavior_api`] —— 行为树运行期查询 `skill-ready?`（规格
//!   §10.5 接线批次）：把「这个技能现在能不能用」暴露给脚本，需要
//!   `Registry` 把字符串 ID 解析成 `ContentIndex`，理由与内容注册函数
//!   相同，但接线方式不同（一次性快照，不是活跃指针），见其模块文档。
//! - [`script_behavior_source`] —— `ll_sim::behavior::BehaviorTreeSource`
//!   的真实实现：装载行为树脚本、注册全部运行期查询 API、把求值结果
//!   翻译成 `Intent`，是「AI 真的做出决策」这一环此前缺失的最后一块
//!   拼图，见其模块文档「四步链路」一节。
//!
//! # 依赖方向
//!
//! 规格 §5：`ll-render` ← `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`
//! ← `ll-ui`。本 crate 依赖 `ll-core`、`ll-world`（Task 8 新增，理由见
//! [`base_terrain`] 模块文档）、`ll-script`（Task 11 新增，理由见
//! [`pipeline`] 模块文档）、`ll-sim`（P5-B 接线批次新增：
//! [`skill::SkillTable`]/[`quest::RegisteredQuests`] 需要实现
//! `ll_sim::skill::SkillCatalog`/`ll_sim::quest::QuestCatalog` 才能真正
//! 接入 `resolve`，见两个模块的文档）与 `ll-render`（动画剪辑接线批次
//! 新增：[`clip`]/[`base_clip`] 需要 `ll_render::anim::Clip`/
//! `base_hero_clips`，见 [`clip`] 模块文档），不得被下游任何 crate
//! 反向依赖。

pub mod active_registry;
pub mod asset_vfs;
pub mod base_clip;
pub mod base_placeholder;
pub mod base_race;
pub mod base_space_profile;
pub mod base_terrain;
pub mod base_xp_curve;
pub mod class;
pub mod clip;
pub mod content_hash;
pub mod discover;
pub mod load_report;
pub mod manifest;
pub mod mod_set;
pub mod pipeline;
pub(crate) mod prereq_graph;
pub mod quest;
pub mod quest_overview;
pub mod race;
pub mod registry;
pub mod script_behavior_api;
pub mod script_behavior_source;
pub mod script_class_api;
pub mod script_clip_api;
pub mod script_quest_api;
pub mod script_race_api;
pub mod script_skill_api;
pub mod script_subclass_api;
pub mod script_terrain_api;
pub mod script_trait_api;
pub mod script_xp_curve_api;
pub mod skill;
pub mod subclass;
#[cfg(test)]
mod test_support;
pub mod topo;
pub mod trait_def;
pub mod version_constraint;
pub mod xp_curve;
