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
//! [`experience`]：击杀产出经验值需要的「这个生物种类的**基准**经验
//! 值是多少」只读接口（[`experience::ExperienceCatalog`] trait，与
//! `skill` 同一套依赖倒置手法），以及项目所有者裁定的那条经验公式
//! 本身（[`experience::kill_experience`]：最低 1 点保底 + 等级差
//! 倍率）。基准值是公式的输入，不是玩家最终拿到的数字，见该模块
//! 文档。
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
//!
//! [`item`]（P6 第二批：背包与地面物品）：`ItemStack`/`GroundItemStack`
//! 与堆叠/合并/拆分（P6 第一批落地）已挪到 `ll_world::item`——背包与
//! 地面物品是世界状态（`ll_world::entity::Agent::inventory`/
//! `ll_world::state::WorldState::ground_items`），`ll-world` 不能依赖
//! `ll-sim`，见该模块（挪动后的）文档「为什么从 `ll-sim` 挪到本模块」
//! 一节。本模块现在 `pub use` 它们，并新增 `resolve` 侧需要的堆叠上限
//! 依赖倒置接口（[`item::ItemCatalog`]）——静态定义（`ItemDef`）仍然
//! 留在下游的 `ll-mod::item`，本 crate 只声明「给我一个物品索引，还我
//! 它的堆叠上限」这个最小接口，与 `skill`/`traits`/`resource_pool`
//! 同一套依赖倒置手法。
//!
//! [`turn`]（世界时钟推进接线批次）：把「弹出时间轴 → 设世界时钟 →
//! resolve → apply → 清理死者 → 重新排期」这条与具体游戏无关的核心
//! 回合引擎（[`turn::TurnEngine`]）搬到本 crate——此前它只存在于
//! `p3_acceptance` 验收 demo 里，`ll-game`（本体二进制）从未接线，
//! 导致真实游玩时 `world.clock` 永不推进（见该模块文档「为什么这段
//! 逻辑必须挪进 `ll-sim`」一节的完整论证）。`p3_acceptance` 与
//! `ll-game` 现在共用同一份实现，各自只保留自己独有的策略。
//!
//! [`apply`]：`Effect` 的唯一写入口。
//!
//! [`character`]（种族属性修正接线批次）：角色/NPC 创建那一刻把种族
//! 声明的六项固定增减量烘焙进 `BaseStats` 的入口
//! （[`character::bake_race_stat_modifiers`]）与依赖倒置接口
//! （[`character::RaceStatModifierSource`]）——与 `traits`/`quest`/
//! `xp_curve` 同一套手法：完整的 `RaceDef` 定义在下游的
//! `ll_mod::race`，本 crate 只声明「给我一个种族索引，还我它的六项
//! 修正」这个最小接口，真实实现（`ll_mod::race::RaceTable`）在下游
//! 补齐。见该模块文档「为什么放在 `ll-sim`，不是 `ll-game`」一节。
//!
//! [`vision`]（暗视接线批次，暗视语义改版批次重写）：把
//! `RaceDef::darkvision_cells` 真正接进视野半径计算
//! （[`vision::sight_radius_for_race`]）与依赖倒置接口
//! （[`vision::RaceDarkvisionSource`]）——与 `character` 同一套手法，
//! 见该模块文档「为什么定义在 `ll-sim`」一节。暗视此前的形态是「光照
//! 千分比下限」（`effective_light = max(实际光照, darkvision_floor)`），
//! 在本作的光照量纲下永远不可能生效，已连同 `effective_light_for_race`
//! 一并删除，理由见该模块文档「缺口是什么」一节。

pub mod ai_query;
pub mod apply;
pub mod behavior;
pub mod catalogs;
pub mod character;
pub mod check;
pub mod combat;
pub mod craft;
pub mod damage_category;
pub mod effect;
pub mod experience;
pub mod exposure;
pub mod formula;
pub mod intent;
pub mod item;
pub mod quest;
pub mod resolve;
pub mod resource_pool;
pub mod rule_modifier;
pub mod skill;
pub mod skill_overview;
pub mod subclass;
pub mod timeline;
pub mod traits;
pub mod turn;
pub mod vision;
pub mod xp_curve;
