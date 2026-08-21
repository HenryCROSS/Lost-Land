//! 迷途大陆的回合与模拟层：规格 §4「意图—结算—效果」闭环的落点。
//!
//! 实体存储（原 `entity` 模块）与名字生成（原 `naming` 模块）不在本
//! crate——两者是世界的**状态**（居民），已迁移到 `ll-world`，不是
//! **演化**逻辑，依赖方向也要求状态所在的 crate 不能反过来依赖
//! 演化所在的 crate（见规格 §5 的依赖顺序：`ll-world` ← `ll-sim`）。
//!
//! 时间轴调度器（[`timeline`]）、`Intent`（[`intent`]）、`Effect`
//! （[`effect`]）、唯一写入口 [`apply::apply`]、`resolve`（[`resolve`]，
//! 从 `Intent` 结合世界状态产出 `Effect`）与战斗数值公式（[`combat`]）
//! 均已实现，见 `docs/superpowers/specs/2026-08-17-p3-turn-combat.md`。
//! `resolve` 需要读地形与 FOV（定义在 `ll-world`），所以本 crate 依赖
//! `ll-world`，而不是反过来。
//!
//! [`skill`]（P5-B 任务 5）：`Intent::UseSkill` 结算需要的技能定义
//! 只读视图（[`skill::SkillCatalog`] trait），不是技能注册表本身——
//! 后者定义在下游的 `ll-mod`，依赖方向不允许本 crate 反过来依赖它，
//! 见该模块文档完整论证。
//!
//! [`quest`]（P5-B 接线批次）：击杀结算需要的任务完成判定接口
//! （[`quest::QuestCatalog`] trait）与任务进度基础操作，与 `skill`
//! 同一套依赖倒置手法，见该模块文档。
//!
//! [`skill_overview`]（P5-B 任务 8）：技能树 UI 数据视图——给定一个
//! `Agent`，返回哪些技能已解锁/可解锁/冷却中的一份纯数据结构，不含
//! 任何渲染，见该模块文档「明确边界」一节。
//!
//! [`behavior`]（规格 §10.5 接线批次）：AI 决策来源的依赖倒置接口
//! （[`behavior::BehaviorTreeSource`]）——行为树 tick 求值器本身需要
//! `ll-script` 的 `SteelVal`，本 crate 不能依赖 `ll-script`，与
//! `skill`/`quest` 同一套依赖倒置手法，见该模块文档。
//!
//! [`xp_curve`]（等级与经验系统）：经验需求曲线的求值机器
//! （`XpCurveDef`/`XpCurveOp`/`XpCurveOperand`，装载期由 `ll-mod` 编译
//! 出的扁平指令数组，运行期本模块零脚本参与求值）与
//! [`xp_curve::XpCurveCatalog`] trait——与 `skill`/`quest`/`behavior`
//! 同一套依赖倒置手法：曲线注册表本身定义在下游的 `ll-mod`，本 crate
//! 只声明「给我一个职业/种族，还我一条曲线」这个接口，见该模块文档
//! 「为什么不复用 `FormulaOp`」一节。
//!
//! [`experience`]：击杀产出经验值需要的「这个生物种类值多少经验」
//! 只读接口（[`experience::ExperienceCatalog`] trait）——与 `skill`
//! 同一套依赖倒置手法，见该模块文档。
//!
//! [`traits`]（天赋系统落地批次）：`resolve_use_skill` 门一需要读的
//! 「这个实体有效的天赋授予了哪些技能」聚合函数（[`traits::effective_traits`]/
//! [`traits::granted_skills`]）与两个依赖倒置接口
//! （[`traits::TraitCatalog`]/[`traits::TraitGrantSource`]）——与
//! `skill`/`quest`/`xp_curve` 同一套依赖倒置手法：完整的 `TraitDef`
//! 定义在下游的 `ll-mod::trait_def`，本 crate 只声明 resolve 侧真正
//! 要消费的最小形状，见该模块文档。
//!
//! [`resource_pool`]（资源池落地批次，第一批：法力池/血池）：法力池
//! 一类标量资源池的容量聚合（[`resource_pool::effective_scalar_capacity`]）
//! 与恢复节奏依赖倒置接口（[`resource_pool::ResourcePoolCatalog`]）——
//! 与 `skill`/`traits` 同一套依赖倒置手法，`ResourcePoolDef`/
//! `ResourcePoolTable` 定义在下游的 `ll-mod::resource_pool`，见该模块
//! 文档「本批次范围」一节。血池（`ResourceCost::Blood`）不经过这张
//! 目录——它直接读/写 `Agent::health`，不是注册表内容,见
//! [`skill::ResourceCost::Blood`] 文档。

pub mod apply;
pub mod behavior;
pub mod combat;
pub mod effect;
pub mod experience;
pub mod intent;
pub mod quest;
pub mod resolve;
pub mod resource_pool;
pub mod skill;
pub mod skill_overview;
pub mod timeline;
pub mod traits;
pub mod xp_curve;
