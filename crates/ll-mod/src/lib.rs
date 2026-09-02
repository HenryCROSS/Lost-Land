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
//! - [`base_cultureless`] —— 同一个模式在「无文化」哨兵上的落点（文化
//!   归属与敌对判定批次）：把 `lostland:cultureless` 注册进
//!   [`registry::Registry`]，一个**只 `intern`、不 `define`** 的文化
//!   索引，让「身上没有文化归属」这件事在敌意表里有一个可被声明敌意的
//!   目标，见该模块文档。
//! - [`base_placeholder`] —— 同一个模式在「占位/未知内容」上的落点
//!   （P5-A 任务 14 补齐）：把本体的占位内容注册进
//!   [`registry::Registry`]，让 NPC 种族缺失的占位降级分支在生产读档
//!   管线里真正可达。
//! - [`race`] —— 种族注册表（P5-C 缺口修补批次）：`RaceDef`/`RaceTable`
//!   落地 `knowledge/design/race-system.md`「核心形状」一节的设计，与
//!   [`base_placeholder`] 的占位种族索引协调（互不冲突，占位索引在
//!   `RaceTable` 里查询恒返回 `None`），见其模块文档。
//! - [`base_contract`] —— 本体内容契约解析（本体内容迁进脚本批次
//!   新增）：本体内容的**定义**搬进 `mods/lostland/*.scm` 之后，
//!   `BaseRaceIds` 这类句柄结构体的**填充**改由「装载后按 id 逐字段
//!   解析」完成，缺任何一条就整批失败。种族这一路的调用点是
//!   [`race::resolve_base_races`]（原先的 `base_race` 模块随定义一并
//!   删除），见其模块文档。
//! - [`content_audit`] —— 装载后内容校验 pass（装载后校验 pass 批次
//!   新增）：接在 [`base_contract`] 之后的第二、三层。契约解析只看
//!   「Rust 点名要的那几条本体内容在不在」，本模块看内容自身的形状
//!   ——跨表 `ContentIndex` 引用是否都指向真的被定义过的条目（对全部
//!   已装载内容生效，是装载管线的硬失败条件），以及本体命名空间下
//!   每个字段是否至少被一条内容设成过非默认值（与
//!   `scripts/ci/check_field_consumers.py` 的「Rust 里有没有人读」互为
//!   另一头，见其模块文档）。表判定复用 [`content_hash::classify_index`]
//!   而不是另写一份等价的 if/else 链。
//! - [`trait_def`] —— 天赋注册表（天赋系统落地批次）：
//!   `knowledge/design/trait-system.md` 四节落地，`TraitDef` 是独立
//!   内容类型，被种族（本批次唯一接入的所有者）通过
//!   `Vec<ll_sim::traits::TraitGrant>` 引用，`TraitTable` 实现
//!   `ll_sim::traits::TraitCatalog`（依赖倒置，见该 trait 模块文档），
//!   见其模块文档「本批次范围」一节的完整裁定。
//! - [`resource_pool`] —— 资源池注册表（资源池落地批次，第一批：法力
//!   池/血池）：`ResourcePoolDef` 是独立内容类型，被
//!   `TraitDef.granted_resource_pools` 引用，`ResourcePoolTable` 实现
//!   `ll_sim::resource_pool::ResourcePoolCatalog`（依赖倒置），本批次
//!   只落地标量池形状，见其模块文档「本批次范围」一节。血池
//!   （`Agent::health` 本身）不经过这张表。
//! - [`item`] —— 物品注册表（P6 第一批：物品基础）：`ItemDef` 是独立
//!   内容类型（定义/实例分离，运行时实例
//!   `ll_sim::item::ItemStack` 定义在 `ll-sim`，依赖方向使然），本批次
//!   不接线任何 `resolve` 侧消费者（背包/装备/使用效果留给后续批次），
//!   见其模块文档「本批次范围」一节。
//! - [`clip`] —— 动画剪辑注册表（动画剪辑接线批次）：把
//!   `ll_render::anim::Clip`（此前只能写死在 Rust 里的动画帧序列/节奏/
//!   循环声明，见其模块文档起因）做成可注册内容，`ClipDef` 与 `class`/
//!   `race` 同一个理由直接落在本 crate。
//! - [`base_clip`] —— 同一个模式在动画剪辑上的生产注册入口，照
//!   [`base_terrain`]/[`base_placeholder`]；本体行走/待机两段剪辑的唯一权威
//!   数据来自 `ll_render::anim::base_hero_clips`（不是本模块自己另抄
//!   一份，见其文档）。
//! - [`content_data`] —— 内容数据文件（JSON5）的装载：每个 mod 目录下
//!   那一组固定文件名的 `*.json5` 内容名册读进上面各张表。**这是 mod
//!   声明内容的唯一通道**——Steel 脚本系统连同 `steel-core` 依赖已在
//!   脚本系统拆除批次整体移除（起因见
//!   [ADR 0028](../../../knowledge/decisions/0028-steel-engine-construction-memory-corruption.md)）。
//! - [`content_schema`]/[`content_schema_gear`]/[`content_schema_world`]
//!   —— 上述各类内容文件的 serde schema 与「写进哪张表」的落点。
//! - [`native_behavior`] —— 行为树的引擎内 Rust 实现：
//!   `ll_sim::behavior::BehaviorTreeSource` 的真实实现，把行为树求值
//!   结果翻译成 `Intent`。它取代了此前那套「行为树写在 mod 的 `.scm`
//!   里、由 Steel 求值」的实现，见其模块文档。
//! - [`pipeline`] —— 加载管线：串起发现→解析→拓扑排序→读内容数据
//!   文件，产出 [`load_report::LoadReport`]。
//! - [`load_report`] —— 加载管理界面（`ll-ui`）依赖的数据形状：按 mod
//!   归类的加载结果、失败阶段、尽力而为的源码位置（Task 11）。
//!
//! # 依赖方向
//!
//! 规格 §5：`ll-render` ← `ll-world` ← `ll-sim` ← `ll-mod` ← `ll-ui`
//! （`ll-script` 这一环已随脚本系统拆除批次整体消失）。本 crate 依赖
//! `ll-core`、`ll-world`（Task 8 新增，理由见
//! [`base_terrain`] 模块文档）、`ll-sim`（P5-B 接线批次新增：
//! [`skill::SkillTable`]/[`quest::RegisteredQuests`] 需要实现
//! `ll_sim::skill::SkillCatalog`/`ll_sim::quest::QuestCatalog` 才能真正
//! 接入 `resolve`，见两个模块的文档）与 `ll-render`（动画剪辑接线批次
//! 新增：[`clip`]/[`base_clip`] 需要 `ll_render::anim::Clip`/
//! `base_hero_clips`，见 [`clip`] 模块文档），不得被下游任何 crate
//! 反向依赖。

pub mod asset_vfs;
pub mod base_clip;
pub mod base_contract;
pub mod base_cultureless;
pub mod base_damage_category;
pub mod base_damage_formula;
pub mod base_placeholder;
pub mod base_space_profile;
pub mod base_terrain;
pub mod base_weather;
pub mod base_xp_curve;
pub mod behavior_binding;
pub mod class;
pub mod clip;
pub mod content_audit;
pub mod content_data;
pub mod content_expr;
pub mod content_hash;
pub mod content_schema;
pub mod content_schema_dialogue;
pub mod content_schema_gear;
pub mod content_schema_world;
pub mod corpse_item;
pub mod damage_category;
pub mod dialogue;
pub mod discover;
pub mod formula;
pub mod item;
pub mod load_report;
pub mod load_session;
pub mod locale_vfs;
pub mod manifest;
pub mod mod_set;
pub mod modifier_type;
pub mod native_behavior;
pub mod npc_wallet;
pub mod pipeline;
pub(crate) mod prereq_graph;
pub mod quest;
pub mod quest_overview;
pub mod race;
pub mod recipe;
pub mod recipe_category;
pub mod registry;
pub mod resource_pool;
pub mod roster;
pub mod skill;
pub mod subclass;
/// 标签定义表（耐久标签批次）。
pub mod tag;
#[cfg(test)]
mod test_support;
pub mod topo;
pub mod trait_def;
pub mod tree;
pub mod version_constraint;
pub mod weapon_category;
pub mod xp_curve;
