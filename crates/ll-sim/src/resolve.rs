//! `resolve`：把一个 [`Intent`] 结合当前世界状态，翻译成一串 [`Effect`]。
//!
//! # C1：`resolve` 必须是纯函数
//!
//! 签名 `resolve(world: &WorldState, intent: &Intent) -> Vec<Effect>`
//! 只接受 `&WorldState`（共享引用）——这不只是约定，是编译期保证：本
//! 文件里没有一处使用 `unsafe`、`Cell`、`RefCell` 或任何其他内部可变性
//! 手段，因此借用检查器直接禁止任何分支写世界，写世界唯一可能的入口
//! （`&mut WorldState`）根本不会出现在这个函数的调用树里。真正的写入
//! 全部延后到调用方对返回的 `Vec<Effect>` 逐个调用
//! [`crate::apply::apply`]（见其文档「三条纪律」）。
//!
//! 这个分离是并行结算的前提：未来成千上万个 AI 的 `resolve` 可以同时
//! 跑（各自只读世界、互不冲突），产出的 `Effect` 收集起来后再单线程
//! 依次 `apply`，读写从不交织。
//!
//! # 已知的范围边界：`Intent::Move` 不做「撞向实体即改判为攻击」的派生
//!
//! [`crate::intent`] 模块文档提到，`Intent::Move` 结合世界状态可以被
//! `resolve` 派生成攻击或开门——本文件确实把「移动目的地是关着的门」
//! 派生成开门效果（见本文件内部的 `resolve_move`），但**没有**把「移动目的地站着
//! 别的实体」派生成攻击。这不是遗漏：该派生一旦引入就要决定「同一格
//! 多个实体时打谁」这类新规则，而本批次的验收测试不需要它，贸然实现
//! 只会引入一段没有测试覆盖的行为。需要「撞人即攻击」的手感时，请把
//! 这条判定和它的打靶规则一起补上，而不是只加派生这一半。
//!
//! # `Interior` 内部移动的范围边界（任务 12）
//!
//! `Intent::Move` 在 `agent.current_space` 是 `Space::Interior` 时**不
//! 产生任何效果**——见本文件内部的 [`resolve_move`]。这是本批次刻意
//! 划定的边界，不是遗漏：`Interior` 内部漫游需要一个「楼层内位置」的
//! 独立坐标系（`ll_core::bounded::BoundedPos`），[`ll_world::entity::Agent`]
//! 当前只有 `pos: TorusPos`（世界地图坐标，进出 `Interior` 都不改变，
//! 见其文档），本批次的任务范围是「接线进出」（[`resolve_enter_space`]/
//! [`resolve_exit_space`]），不是「接线内部漫游」——验收 demo（任务 15）
//! 只需要证明「能进能出、只渲染当前层、层属性生效」，不需要玩家能在
//! `Interior` 内部走动。若放任 `resolve_move` 在 `Interior` 内继续按
//! `Space::Surface` 那套逻辑改 `agent.pos`，会直接破坏「进入 `Interior`
//! 后 `Agent.pos` 不变」这条不变式（见 `Agent::current_space` 文档），
//! 所以这里选择**静默无效**（与撞墙同一种处理），而不是放行一条会
//! 悄悄弄脏世界地图坐标的路径。
//!
//! # `Interior` 退出如何拿到地表 profile
//!
//! [`resolve_exit_space`] 重新构造 `Space::Surface { .. }` 时，`profile`
//! 字段取自 [`WorldState::surface_profile`]——这个索引依赖当前会话的
//! 注册表加载顺序，`resolve` 不能自己现造一个（那会破坏「本体即 Mod」
//! 走同一条注册路径的纪律），只能读 `WorldState` 已经缓存好的那一份，
//! 见其字段文档「为什么不参与序列化，为什么不是 `WorldState::new` 的
//! 参数」一节：调用方必须在开放 `Intent::ExitSpace` 之前显式设置好
//! 这个字段。

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_world::entity::{ActiveStatModifier, Agent, AttributeKind, EntityId};
use ll_world::history::KillCause;
use ll_world::space::{Space, SpaceId};
use ll_world::state::WorldState;

use crate::combat::{Penetration, damage_after_defense};
use crate::effect::Effect;
use crate::experience::ExperienceCatalog;
use crate::intent::{Direction, Intent};
use crate::item::{EquipSlot, ItemCatalog, ItemStack, NoItems, SlotMask, can_merge, merge_stacks};
use crate::quest::{NoQuests, QuestCatalog};
use crate::resource_pool::{
    NoResourcePools, RegenRule, ResourcePoolCatalog, ResourcePoolShape, RestRecoveryAmount,
    effective_scalar_capacity, effective_slot_tier_capacity,
};
use crate::skill::{NoSkills, ResourceCost, SkillCatalog, SkillEffect};
use crate::timeline::action_cost;
use crate::traits::{
    NoTraitGrants, NoTraits, TraitCatalog, TraitGrantSource, effective_traits, granted_skills,
};

/// 非位移动作（等待、攻击、开门）的基础代价，与平地移动同一基准
/// （草地的 `move_cost` 恰为这个值）——本批次没有武器速度、技能读条
/// 之类会让这些动作耗时不同于「一次基准行动」的系统，统一按这个基准
/// 计费，接入那些系统时按动作类型分别替换即可。
const BASE_ACTION_COST: u32 = 100;

/// 基准有效敏捷，对应 `BaseStats::BASELINE` 的敏捷值（10，调整值为零）。
///
/// 真正的「有效敏捷」需要 `derive_stats`（装备、状态效果、负重的综合
/// 结果）驱动，但那是衍生属性，规则上必须是纯函数且不进存档（见
/// `knowledge/design/attribute-system.md` 「七、衍生属性绝不进存档」），
/// 而 `derive_stats` 本身属于后续批次才落地的东西。`derive_stats` 落地
/// 后，[`effective_speed_from_dexterity`] 的函数体应替换成
/// `derive_stats(agent.stats, ..).effective_speed`，调用点不变。
const BASELINE_EFFECTIVE_SPEED: u32 = 1000;

/// `BaseStats::BASELINE` 的敏捷值——[`effective_speed_from_dexterity`]
/// 的线性映射以它为基准点：敏捷恰为这个值时，有效速度恰为
/// [`BASELINE_EFFECTIVE_SPEED`]。
const BASELINE_DEXTERITY: i64 = 10;

/// 由角色敏捷推出有效行动速度：基准敏捷（10）对应
/// [`BASELINE_EFFECTIVE_SPEED`]，此后与敏捷成正比。
///
/// # 为什么不能继续让全体角色共用同一个常量
///
/// 本函数落地前，四个 `resolve_*` 分支全部直接传入
/// [`BASELINE_EFFECTIVE_SPEED`] 这个常量本身，不读 `agent.stats.dexterity`
/// ——这是 P3 验收 demo（Task 9）排查时发现的阻断性缺陷：无论给敌人
/// 分配多高或多低的敏捷，`resolve` 算出的行动耗时都完全相同，时间轴
/// 调度器（[`crate::timeline`]）本身「敏捷高者能在同一窗口内多行动
/// 几次」这条核心手感（见其模块文档开篇）在结算层根本没有输入通道
/// 可以体现出来——`Timeline` 的排序逻辑是对的，喂给它的排期时刻却
/// 从未因敏捷不同而不同。
///
/// 这不是要提前实现完整的 `derive_stats`（装备/状态效果/负重那套还
/// 没有任何字段落地，见 [`BASELINE_EFFECTIVE_SPEED`] 文档），只是把
/// 「敏捷」这个已经存在于 [`ll_world::entity::BaseStats`] 的字段接上
/// 最朴素的线性比例，让 Intent → resolve → Effect → 时间轴这条链路
/// 真正对「敏捷不同」敏感，而不是看起来接好了、实际上分支从不读取
/// 敏捷字段。`derive_stats` 落地后应替换本函数体，调用点不必改动。
fn effective_speed_from_dexterity(dexterity: i32) -> u32 {
    let dexterity = i64::from(dexterity).max(1);
    let speed = i64::from(BASELINE_EFFECTIVE_SPEED) * dexterity / BASELINE_DEXTERITY;
    speed.clamp(1, i64::from(u32::MAX)) as u32
}

/// 给定某一项属性的裸值,查一次 [`ll_world::entity::Agent::active_stat_modifiers`]
/// ，算出这一时刻真正生效的值——供任何要读「这项属性当前实际是多少」
/// 的调用方共用，不要在各自的调用点各写一遍同样的到期判定。
///
/// # 多来源叠加：逐条过滤未过期条目再求和
///
/// `buffs-and-triggers.md` 六节裁定「不同效果能叠加」——`modifiers.get(&kind)`
/// 现在拿到的是这一项属性上「按来源」索引的一整层 `BTreeMap`，本函数
/// 遍历它的全部条目，过滤掉已过期的（惰性到期判定，见下），对剩下的
/// `delta` 求和后叠加到 `base` 上，不再是「查到一条就直接用」。合并
/// 顺序由内层 `BTreeMap<ContentIndex, _>` 自身的键序保证确定性——加法
/// 可交换，顺序不影响这里的求和结果，但选一个天然有序的容器不需要为
/// 这一点额外付出任何代价（`buffs-and-triggers.md` 六节「与二节排序
/// 规则的关系」一段）。
///
/// # 性能：`m` 是这一项属性当前生效的不同来源数，现实规模下是个位数
///
/// 原实现是一次 `Option` 查表，`O(1)`；本实现是一次外层查表（`O(1)`，
/// `AttributeKind` 只有六个变体）加一次对内层 `m` 条记录的遍历——`m`
/// 不是「这个实体全部修正」，只是「这一项属性上的不同来源数」，且这次
/// 遍历只在真正结算一次攻击时付一次（本函数只被 [`resolve_attack`]
/// 调用），不是每 tick 都要跑一遍。`buffs-and-triggers.md` 六节已经判断
/// 这个开销在当前内容规模下可接受，不需要额外优化——若未来某个内容
/// 组合让 `m` 变得可观，问题出在内容设计本身，不是本函数的算法复杂度。
///
/// 首个消费者是 [`resolve_attack`] 的攻击力（力量）；`knowledge/design/
/// combat-three-axis.md` 四节已经点名防御方将来也要从 `derive_stats`
/// 走同一条接口约定，`knowledge/design/vehicle-and-mounting.md` 六节
/// 点名载具的 `stat_modifiers` 同样落在 `active_stat_modifiers` 这份
/// 数据上——三处未来调用点共享的正是本函数这段「查表 + 到期判定」，
/// 这就是抽出一个函数而不是让 `resolve_attack` 私自内联的理由（ADR
/// 0021：抽象要有真实可共享的算法支撑，这里确实有）。
///
/// # 为什么不是完整的 `derive_stats`
///
/// `attribute-system.md` §七定义的完整签名是 `derive_stats(基础属性,
/// 装备, 状态效果, 负重) -> DerivedStats`——「装备」（`StatBonus` 累加）
/// 与「负重」两个输入目前都还没有任何字段落地（`combat-three-axis.md`
/// 四节：`StatBonus` 的正式定义与累加逻辑留给 P6 装备批次）。提前拼出
/// 一个四个入参俱全、实际只有一个入参有真数据的 `derive_stats`，只会
/// 让另外两个入参变成没有调用者会填的死参数。本函数只做「状态效果」
/// 这一个输入——[`ActiveStatModifier`] 已经存在、已经有真实写入方
/// （技能的 `SkillEffect::TemporaryStatModifier`，见 [`resolve_use_skill`]）
/// ——这是本批次唯一有真实数据支撑的部分。`derive_stats` 落地后，本
/// 函数应该被它的「状态效果」分支取代，调用点不必改动（与
/// [`effective_speed_from_dexterity`] 同一条纪律，见其文档）。
///
/// # 惰性到期判定
///
/// `expires_at.0 > now.0` 才算仍然生效——与 [`resolve_use_skill`] 冷却
/// 判定（其「门二」注释）同一条比较方向：世界时钟达到或超过到期时刻时
/// 视为已失效，直接回落到裸属性值，不做任何清理，见 [`ActiveStatModifier`]
/// 文档「惰性到期判定，不存『当前是否生效』」一节。
fn effective_attribute(
    base: i32,
    kind: AttributeKind,
    modifiers: &std::collections::BTreeMap<
        AttributeKind,
        std::collections::BTreeMap<ContentIndex, ActiveStatModifier>,
    >,
    now: Tick,
) -> i32 {
    let Some(per_source) = modifiers.get(&kind) else {
        return base;
    };
    per_source
        .values()
        .filter(|modifier| modifier.expires_at.0 > now.0)
        .fold(base, |acc, modifier| acc + modifier.delta)
}

/// 玩家每走一步，探索记忆按这个半径覆盖新位置的可见格（见
/// [`resolve_move`] 尾部、[`crate::effect::Effect::MarkExplored`] 文档）。
///
/// # 为什么是固定值，不接光照/层属性算出的真实视野半径
///
/// 渲染那一路（demo 里的 `effective_sight_radius`）会按
/// `SpaceProfile` 的环境光基准与世界时钟现算视野半径——地下城更暗，
/// 半径更小。但那份换算（`ll_world::space_profile::SpaceProfile` +
/// `ll_world::light::effective_ambient_light`）此刻只在各个 demo 的
/// `examples/*/layout.rs` 里现算，`resolve` 所在的 `ll-sim` 库代码从没
/// 有拿到一份可查询的 `SpaceProfileTable`——那是注册期内容表，走
/// `ll-mod::Registry`，而 `resolve` 按依赖顺序（规格 §5）在
/// `ll-mod` 上游，不能反过来依赖它。要让探索半径也感知光照，需要先把
/// 「层属性表」接成 `WorldState` 能查询到的东西，这是比「补上写入路径」
/// 大得多的另一件事，本次任务不做（YAGNI）。
///
/// 用固定半径也不是权宜之计——「记不记得某处地形」与「此刻这里有多暗」
/// 本就是两件事：现实里哪怕举着火把只能看清脚下几步，也不会因为这一刻
/// 昏暗就忘记白天来过这里时看清楚的布局。`minimap`/`continent_map`
/// 只消费「探不探索过」这一个是/否位（[`ll_world::exploration`] 模块
/// 文档「只存位图」一节），不消费「当时有多亮」，固定半径与这份精度
/// 完全匹配，不需要为它单独追一份光照相关的输入。
const EXPLORATION_SIGHT_RADIUS: u32 = 12;

/// 把一个 [`Intent`] 结合当前世界状态，翻译成一串 [`Effect`]。
///
/// 目标实体（`actor`/`target`）若已不在 `world.actors` 中（可能已在
/// 同一批结算里被更早的 `Effect` 销毁），一律返回空 `Vec`——这与
/// [`crate::apply::apply`] 对不存在实体的处理方式一致（静默忽略而非
/// panic 或报错），理由同样是「目标不存在不是异常状况，是结算并发/
/// 时序下的正常可能性」。
///
/// # `Intent::UseSkill` 与击杀任务进度在这个入口下恒不产出效果
///
/// 本函数是 [`resolve_with_skills_and_quests`] 在「调用方没有技能目录、
/// 也没有任务目录」时的薄封装（传入 [`crate::skill::NoSkills`]/
/// [`crate::quest::NoQuests`]）——不需要技能/任务结算的调用点（例如
/// 只测试移动/开门这类不涉及内容注册表的场景）不需要为此多构造一份
/// 目录。真正想让技能结算/击杀任务进度生效的调用方应改用
/// [`resolve_with_skills`]/[`resolve_with_skills_and_quests`]，传入
/// 实现了对应 trait 的真实目录——`ll_mod::skill::SkillTable`/
/// `ll_mod::quest::RegisteredQuests`（接线批次）现在就是这样的真实
/// 实现。
pub fn resolve(world: &WorldState, intent: &Intent) -> Vec<Effect> {
    resolve_with_skills_and_quests(world, intent, &NoSkills, &NoQuests)
}

/// [`resolve`] 的最完整入口：额外接收一份种族天赋授予来源与一份天赋
/// 目录，用于结算 [`Intent::UseSkill`] 门一时把种族天赋授予的技能也
/// 计入「有效技能」并集（`knowledge/design/trait-system.md` 三节①，
/// 天赋系统落地批次）。
///
/// 四层入口（`resolve` → `resolve_with_skills` →
/// `resolve_with_skills_and_quests` → 本函数）而不是给
/// `resolve_with_skills_and_quests` 加两个参数，理由同
/// [`resolve_with_skills`] 文档：不强迫仓库里已有的全部调用点（本文件
/// 自身的既有测试、`ll-mod`/`ll-game` 的既有接线）都多传两份目录——
/// 传 [`NoTraitGrants`]/[`NoTraits`] 与"不传"在行为上完全等价（两者
/// 都让 `granted_skills` 现算出一个空集合），本函数只服务真正想让
/// 种族天赋生效的调用方。
pub fn resolve_with_skills_and_traits(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        skills,
        &NoQuests,
        race_traits,
        traits,
        &NoResourcePools,
        &NoItems,
    )
}

/// [`resolve`] 的最完整入口：在 [`resolve_with_skills_and_traits`] 之上
/// 再额外接收一份资源池目录，用于结算标量池的消耗判定（门四，
/// [`resolve_use_skill`]）与每回合开始的自动恢复
/// （`RegenRule::OnTurnStart`，`resource-pools-and-rest.md` 二、四节，
/// 资源池落地批次，第一批：法力池/血池）。
///
/// 五层入口（`resolve` → `resolve_with_skills` →
/// `resolve_with_skills_and_quests`/`resolve_with_skills_and_traits` →
/// 本函数）而不是给某个既有入口加参数，理由同
/// [`resolve_with_skills`] 文档：不强迫仓库里已有的全部调用点都多传
/// 一份资源池目录——传 [`NoResourcePools`] 与"不传"在行为上完全等价
/// （两者都让每回合恢复现算出一个空批次），本函数只服务真正想让法力
/// 池等标量池的完整链路（消耗判定 + 每回合恢复）生效的调用方。
///
/// 血代价（`ResourceCost::Blood`）不依赖 `pools` 参数——它直接读/写
/// `Agent::health`，见 [`crate::skill::ResourceCost::Blood`] 文档；本
/// 入口对血魔法技能同样适用，只是它不消费本函数新增的这份目录。
pub fn resolve_with_skills_traits_and_pools(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        skills,
        &NoQuests,
        race_traits,
        traits,
        pools,
        &NoItems,
    )
}

/// [`resolve`] 的最完整入口：在 [`resolve_with_skills_traits_and_pools`]
/// 之上再额外接收一份物品目录，用于结算 [`Intent::PickUp`]
/// 拾取时与背包已有堆合并所需的堆叠上限查询（P6 第二批：背包与地面
/// 物品，见 [`resolve_pick_up`] 文档）。
///
/// 六层入口而不是给某个既有入口加参数，理由同
/// [`resolve_with_skills`] 文档：不强迫仓库里已有的全部调用点都多传
/// 一份物品目录——传 [`NoItems`] 与"不传"在行为上完全等价（[`resolve_pick_up`]
/// 查不到堆叠上限时按"不限量"处理，见 [`NoItems`] 文档），本函数只
/// 服务真正想让拾取时自动合并生效的调用方（`ll_mod::item::ItemTable`
/// 现在就是这样的真实实现）。
///
/// [`Intent::Drop`] 不消费 `items` 参数——丢弃不需要查堆叠上限，见
/// [`resolve_drop`] 文档。
pub fn resolve_with_skills_traits_pools_and_items(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        skills,
        &NoQuests,
        race_traits,
        traits,
        pools,
        items,
    )
}

/// [`resolve`] 的技能结算入口：额外接收一份技能目录，用于结算
/// [`Intent::UseSkill`]。等价于
/// `resolve_with_skills_and_quests(world, intent, skills, &NoQuests)`
/// ——保留这个薄封装是为了不破坏仓库里已有的全部既有调用点（`ll-sim`
/// 的技能结算测试、`ll-mod` 的接线测试等）：它们只需要技能结算，强迫
/// 它们每处都多传一个任务目录（哪怕是空的）只是无意义的噪音。
pub fn resolve_with_skills(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
) -> Vec<Effect> {
    resolve_with_skills_and_quests(world, intent, skills, &NoQuests)
}

/// [`resolve`] 的完整入口：额外接收一份技能目录与一份任务目录，用于
/// 结算 [`Intent::UseSkill`] 与击杀对任务进度的推进（P5-B 接线批次）。
///
/// 三层入口（`resolve` → `resolve_with_skills` →
/// `resolve_with_skills_and_quests`）而不是给 `resolve` 加两个参数，
/// 理由同 [`resolve_with_skills`] 文档：不强迫只需要技能、不需要任务
/// 系统的既有调用点（反之亦然）都多传一份目录。等价于
/// `resolve_dispatch(world, intent, skills, quests, &NoTraitGrants, &NoTraits)`
/// ——种族天赋这一路来源同样走「不传等价于传空」的既有纪律，见
/// [`resolve_with_skills_and_traits`] 文档。
pub fn resolve_with_skills_and_quests(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    quests: &dyn QuestCatalog,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        skills,
        quests,
        &NoTraitGrants,
        &NoTraits,
        &NoResourcePools,
        &NoItems,
    )
}

/// [`resolve_with_skills_and_quests`]/[`resolve_with_skills_and_traits`]/
/// [`resolve_with_skills_traits_and_pools`]/
/// [`resolve_with_skills_traits_pools_and_items`] 共用的核心分派逻辑——
/// 四个公开入口都只是"缺一份目录时传对应的 `No*` 空实现"的薄封装，
/// 真正的 `Intent` 匹配与效果产出只写这一份，不重复。
///
/// `#[allow(clippy::too_many_arguments)]`：八个参数分别对应七种
/// 结算需要的只读依赖（技能/任务/种族天赋来源/天赋/资源池/物品目录）
/// 加 `world`/`intent` 本身，拆分成多份目录正是「resolve 依赖倒置」
/// 这套手法刻意要做的事（见模块文档同一批目录的既有取舍），不是可以
/// 合并成一个结构体的意外堆叠——与
/// `crates/ll-sim/tests/resource_pool_resolve.rs` 的
/// `spawn_agent_with_pool` 同一条既有先例。
#[allow(clippy::too_many_arguments)]
fn resolve_dispatch(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    quests: &dyn QuestCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let mut effects = match *intent {
        Intent::Wait { actor } => resolve_wait(world, actor, race_traits, traits, pools),
        Intent::Move { actor, dir } => resolve_move(world, actor, dir),
        Intent::Attack { actor, target } => resolve_attack(world, actor, target),
        Intent::OpenDoor { actor, pos } => resolve_open_door(world, actor, pos),
        Intent::EnterSpace { actor, target } => resolve_enter_space(world, actor, target),
        Intent::ExitSpace { actor } => resolve_exit_space(world, actor),
        Intent::UseSkill {
            actor,
            skill,
            target,
        } => resolve_use_skill(world, actor, skill, target, skills, race_traits, traits),
        Intent::Rest {
            actor,
            target_ticks,
        } => resolve_rest(world, actor, target_ticks, race_traits, traits, pools),
        Intent::PickUp { actor } => resolve_pick_up(world, actor, items),
        Intent::Drop { actor, def } => resolve_drop(world, actor, def),
        Intent::Equip { actor, def } => resolve_equip(world, actor, def, items),
        Intent::Unequip { actor, slot } => resolve_unequip(world, actor, slot, items),
    };
    // 休息中断（`resource-pools-and-rest.md` 八节「中断怎么表达」一节）：
    // 任何非 `Wait`/`Rest` 意图,若发起者当前正在休息,追加一条不带恢复
    // 批次的 `Effect::ClearResting`——与 D&D 长休/短休规则"做别的事就要
    // 重新计时"一致。`Wait`/`Rest` 两个变体不在这里处理：`resolve_wait`/
    // `resolve_rest` 内部已经各自判断"是否到达 target_ticks"并按需产出
    // 带恢复的 `ClearResting`,不需要本检查再插一条。
    if !matches!(*intent, Intent::Wait { .. } | Intent::Rest { .. })
        && let Some(agent) = world.actors.get(intent.actor())
        && agent.resting.is_some()
    {
        effects.push(Effect::ClearResting {
            actor: intent.actor(),
        });
    }
    // 资源池每回合自动恢复（RegenRule::OnTurnStart,`resource-pools-and-rest.md`
    // 四节）：每次结算一个实体的意图,就是这个实体"自己的回合"（本项目
    // 的时间轴是逐实体调度,不是全体同时行动的固定回合制,见
    // `crate::timeline` 模块文档),因此在这里为全部 `Intent` 变体统一
    // 触发一次,不只是 `Intent::Wait`——一个法师每回合都在放技能同样应
    // 该按节奏回蓝,不能因为它选择了"行动"而不是"等待"就跳过恢复。
    effects.extend(resolve_resource_pool_regen(
        world,
        intent.actor(),
        race_traits,
        traits,
        pools,
    ));
    // 击杀任务进度：`Intent::Attack` 与 `Intent::UseSkill` 都可能产出
    // `Effect::Kill`（后者见 `resolve_use_skill` 的 `DealDamage` 分支，
    // 本批次修掉的缺口），两者因此共用同一条推进逻辑——`append_quest_
    // kill_progress` 本身只扫描 `effects` 里的 `Effect::Kill`，不关心
    // 是哪种 `Intent` 产出的，唯一需要从 `intent` 里取的只是「谁是
    // 击杀者」这一个字段。`crate::quest` 模块文档「只有 Intent::Attack
    // 会触发这条接线」一节记录的范围边界到此解除——该节本就说明这条
    // 边界的唯一成因是 `resolve_use_skill` 当时不产出 `Effect::Kill`，
    // 不是设计上刻意排除技能击杀。
    let kill_progress_actor = match *intent {
        Intent::Attack { actor, .. } => Some(actor),
        Intent::UseSkill { actor, .. } => Some(actor),
        _ => None,
    };
    if let Some(actor) = kill_progress_actor {
        append_quest_kill_progress(world, actor, &mut effects, quests);
    }
    // 击杀历史记录：与击杀任务进度同一个触发点（同一批 Effect::Kill），
    // 各自独立追加,互不依赖——见 append_kill_history 文档。不需要按
    // Intent 类型区分调用与否：函数本身只扫描 effects 里已经存在的
    // Effect::Kill,对没有产出击杀的意图（Wait/Move/...）是无操作。
    append_kill_history(world, &mut effects);
    effects
}

/// 资源池每回合自动恢复（`RegenRule::OnTurnStart`,
/// `resource-pools-and-rest.md` 四节，资源池落地批次，第一批）：遍历
/// `actor` 当前 [`effective_traits`] 命中的每一条天赋的
/// `granted_resource_pools`，对 `pools` 目录里恢复节奏是
/// `RegenRule::OnTurnStart` 的每一条产出一个
/// [`Effect::AdjustResourcePool`]（正值）。
///
/// # 为什么按「每条命中的授予声明」各自产出一条效果，不按池去重
///
/// 若两个不同天赋各自都授予了同一个池的容量（`trait-system.md` 三节④
/// 「聚合规则」：容量按来源求和，不是取第一条命中），本函数同样让
/// 两条来源各自贡献一次恢复量,最终效果是两条 `AdjustResourcePool`
/// 效果各自的 `delta` 相加——与容量本身"两个来源各自贡献一部分"是
/// 同一条叠加语义,不是"取一次就够"的互斥选择,理由同该节原文。
///
/// # 为什么这里不做"钳位到容量上限"
///
/// `resource-pools-and-rest.md` 三节「上限变化时怎么办」一节：容量
/// 变化只在**读取**"当前可用量"时现场钳位（`usable = min(stored_current,
/// effective_cap)`），不主动改写存储值——回合恢复只是又一处"写入"，
/// 遵守同一条纪律：写入端不做钳位，`resolve_use_skill` 门四读取时自然
/// 把超出容量的部分视为不可用，见其文档。
fn resolve_resource_pool_regen(
    world: &WorldState,
    actor: EntityId,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let mut effects = Vec::new();
    for trait_id in effective_traits(agent.race, agent.level, race_traits) {
        let Some(rule) = traits.trait_rule(trait_id) else {
            continue;
        };
        for grant in &rule.granted_resource_pools {
            let Some(pool_rule) = pools.resource_pool(grant.pool) else {
                continue;
            };
            let RegenRule::OnTurnStart { amount } = pool_rule.regen_rule else {
                continue;
            };
            // 按形状分流——`ResourcePoolShape::Scalar` 走既有的
            // `AdjustResourcePool`（法术位落地批次之前唯一存在的分支,
            // 原样保留）；`TieredSlots` 走"从最低档开始恢复"（与消耗
            // 算法"从最低阶开始取"对称），落到
            // `Effect::AdjustResourceSlot`——法术位落地批次新增,证明
            // `RegenRule::OnTurnStart` 与 `ResourcePoolShape::TieredSlots`
            // 这个"反过来的组合"（`resource-pools-and-rest.md` 四节）
            // 真的会正确恢复,不是只能被声明、实际按标量语义误处理。
            match pool_rule.shape {
                ResourcePoolShape::Scalar => {
                    effects.push(Effect::AdjustResourcePool {
                        actor,
                        pool: grant.pool,
                        delta: amount as i32,
                    });
                }
                ResourcePoolShape::TieredSlots { tier_count } => {
                    effects.extend(restore_slots_from_lowest_tier(
                        agent, actor, grant.pool, tier_count, amount,
                    ));
                }
            }
        }
    }
    effects
}

/// 从第 1 档起，按顺序清掉总计 `amount` 个已消耗槽位——与消耗算法
/// "从最低阶开始取"对称,供 [`resolve_resource_pool_regen`]
/// （`RegenRule::OnTurnStart`）与 [`tiered_slot_rest_effects`]
/// （`RegenRule::OnRest` 的 `Amount` 分支）共用同一段算法,不重复实现
/// 两遍。只对 `agent.spent_slots` 里已消耗数非零的档位产出效果。
fn restore_slots_from_lowest_tier(
    agent: &Agent,
    actor: EntityId,
    pool: ContentIndex,
    tier_count: u8,
    amount: u32,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    let mut remaining = amount;
    for tier in 1..=tier_count {
        if remaining == 0 {
            break;
        }
        let spent = agent.spent_slots.get(&(pool, tier)).copied().unwrap_or(0);
        let restore = spent.min(remaining);
        if restore > 0 {
            effects.push(Effect::AdjustResourceSlot {
                actor,
                pool,
                tier,
                delta: -(restore as i32),
            });
            remaining -= restore;
        }
    }
    effects
}

/// [`resolve`] 的完整入口，额外接收一份经验目录，用于结算击杀产出的
/// 经验（等级与经验系统，`knowledge/design/level-and-experience-system.md`
/// 五节）。四层入口（`resolve` → `resolve_with_skills` →
/// `resolve_with_skills_and_quests` → 本函数）而不是给某个既有入口加
/// 参数，理由同 [`resolve_with_skills`] 文档：不强迫不关心经验结算的
/// 既有调用点多传一份目录。
///
/// # 为什么挂在 `Effect::Kill`，不是 `HistoricalEvent::Kill`
///
/// 设计文档五节核实过：`kill-and-death-events.md` 把击杀分三档，「无名
/// 小卒之间」完全不产出 `HistoricalEvent::Kill`——若经验产出挂在那里，
/// 绝大多数战斗击杀不会触发经验。`Effect::Kill` 由 `resolve_attack`/
/// `resolve_use_skill` 对**每一次**击杀产出，是前者的严格超集，本函数
/// 因此复用 [`append_kill_history`] 已经在扫描的同一批 `effects`，见
/// [`append_kill_experience`]。
pub fn resolve_with_skills_quests_and_experience(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    quests: &dyn QuestCatalog,
    experience: &dyn ExperienceCatalog,
) -> Vec<Effect> {
    let mut effects = resolve_with_skills_and_quests(world, intent, skills, quests);
    append_kill_experience(world, &mut effects, experience);
    effects
}

/// 击杀产出经验的接线：若 `effects` 里包含 [`Effect::Kill`] 且
/// `killer` 已知，读取（结算前仍然存在的）被击杀目标的
/// `creature_kind`/`race`（与 [`Effect::IncrementKillCount`] 完全同一
/// 个归并键，见 `append_kill_history` 文档），查询 `experience` 目录
/// 该给多少经验，非零时追加一条 [`Effect::GrantExperience`]。
///
/// # 为什么追加在末尾，不像 `RecordHistoricalEvent` 那样插在 `Kill`
/// 之前
///
/// [`Effect::GrantExperience`] 的 `target` 是击杀者，不是被击杀者——
/// `apply` 处理这条效果时不需要查询 `victim` 是否仍然存在（`victim`
/// 会不会已经被同一批效果里的 `Effect::Kill` 销毁与本效果无关），因此
/// 没有 [`append_kill_history`] 文档「为什么必须排在对应的 Effect::Kill
/// 之前」一节描述的那种时序依赖，追加在末尾（与
/// `append_quest_kill_progress` 同一个位置）即可。
fn append_kill_experience(
    world: &WorldState,
    effects: &mut Vec<Effect>,
    experience: &dyn ExperienceCatalog,
) {
    let grants: Vec<Effect> = effects
        .iter()
        .filter_map(|effect| {
            let Effect::Kill {
                target,
                killer: Some(killer),
                ..
            } = effect
            else {
                return None;
            };
            let victim = world.actors.get(*target)?;
            let kind = victim.creature_kind.unwrap_or(victim.race);
            let amount = experience.xp_reward_for(kind);
            if amount > 0 {
                Some(Effect::GrantExperience {
                    target: *killer,
                    amount,
                })
            } else {
                None
            }
        })
        .collect();
    effects.extend(grants);
}

/// 击杀结算与任务进度的接线（P5-B 接线批次）：若 `effects` 里包含
/// [`Effect::Kill`]，读取（结算前仍然存在的）被击杀目标的
/// [`ll_world::entity::Agent::race`] 作为
/// [`crate::quest::QuestKillRule::target_kind`] 的匹配依据，把击杀
/// 计数、以及可能因此达标的任务完成写入一并追加进效果列表——见
/// [`crate::quest`] 模块文档「击杀计数」一节的完整论证。调用方
/// （[`resolve_with_skills_and_quests`]）现在对 `Intent::Attack` 与
/// `Intent::UseSkill` 都会调用本函数，理由见该处注释。
///
/// 必须在 `apply` 之前读取被击杀者的 `race`：本函数只接受
/// `&WorldState`（`resolve` 必须是纯函数，C1），此刻目标仍然存在于
/// `world.actors` 里，`Effect::Kill` 还没有被应用。
fn append_quest_kill_progress(
    world: &WorldState,
    actor: EntityId,
    effects: &mut Vec<Effect>,
    quests: &dyn QuestCatalog,
) {
    let killed_kinds: Vec<ContentIndex> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Kill { target, .. } => world.actors.get(*target).map(|agent| agent.race),
            _ => None,
        })
        .collect();
    for kind in killed_kinds {
        effects.extend(crate::quest::kill_progress_effects(
            world, actor, kind, quests,
        ));
    }
}

/// 击杀历史记录与击杀计数的接线
/// （`knowledge/design/kill-and-death-events.md`）：若 `effects` 里包含
/// [`Effect::Kill`]，在对应的 `Effect::Kill` **之前**插入效果——
///
/// 1. 恒插入一条 [`Effect::IncrementKillCount`]（决策二，见下节）：
///    聚合计数按 `creature_kind`/`race` 归并，不论 `victim` 是否已
///    "具名"。
/// 2. 被击杀者已经"具名"（[`ll_world::entity::Agent::remembered_id`]
///    有值）时，**额外**再插入一条 [`Effect::RecordHistoricalEvent`]
///    （完整记录）。
///
/// # 决策二：叠加计算，不再互斥（项目所有者裁定「一起计算，就是杀了
/// 10 只」）
///
/// 决策一（无名单位击杀改计数）落地时把两条路径设计成互斥——一场
/// 击杀要么产出完整记录，要么只累加计数，不会同时产出两者。项目所有
/// 者复核后否决了这条互斥：杀 10 只哥布林、其中 1 只有名字，计数器
/// 理应显示 10，不是 9——"一起计算，就是杀了 10 只"。本函数因此改为
/// 两条路径叠加：聚合计数覆盖**全部**击杀（默认路径），完整记录只
/// 额外覆盖"值得被记住"的具名死者（偏差路径的加法，不再是替代）。
///
/// # 老存档的计数是低估，且无法从 `history` 补算
///
/// 决策二落地前产出的存档里，`kill_counts` 只计了无名击杀——具名击杀
/// 全部只进了 `history`，从未累加进 `kill_counts`。读这类旧存档不会
/// 触发新的 schema 迁移（`kill_counts` 字段本身的类型/位置都没变，见
/// `ll_world::state::WorldState::kill_counts` 文档「决策二」一节），
/// 因此**不会**被自动补算：旧存档里的 `kill_counts` 在决策二之后仍然
/// 只反映"曾经的无名击杀"，是一次性的、永久的低估，不随读档自动修复
/// ——`ll_world::history::KillRecord` 不携带 `creature_kind`/`race`
/// 这类归并键（只有 `killer`/`victim` 两个 `WorldId`，`WorldId` 是不
/// 透明整数句柄，查不回死者当时的物种），补算需要的数据在写入 `history`
/// 那一刻就已经丢失，不是遍历成本问题，是数据源本身不完整，因此如实
/// 记录为已知缺口，不假装能补算：新增的击杀从代码更新那一刻起按决策
/// 二正确计数，旧记录只能原样接受。
///
/// # 触发判据：为什么"是否额外产出完整记录"只看 `victim` 是否已具名
///
/// 设计文档三节的分级规则是"玩家相关/具名 NPC 相关"两档、任一方具名
/// 即全记。本函数把这两档收敛成一个更窄、但可以在不引入"死亡瞬间
/// 懒分配跨越 despawn 时序"这类额外复杂度的前提下正确实现的判据：
/// **只要求 `victim` 已经具名**。理由：
///
/// 1. `KillRecord.victim: WorldId` 是非 `Option` 的必填字段——若
///    `victim` 未具名，压根没有 `WorldId` 可以填进这个字段，必须先
///    有一次懒分配。懒分配本身要求在 `victim` 被 `Effect::Kill`
///    销毁**之前**执行（`WorldState::record_kill` 文档「调用时机」
///    一节），这是本函数把 `RecordHistoricalEvent` 插到 `Kill` 之前
///    （而不是像 `append_quest_kill_progress` 那样追加在末尾）的原因。
/// 2. 设计文档五节原文承认"一方不具名时，`KillRecord.killer` 或本
///    条记录本身如何处理不具名的一侧，属于实现期需要拍板的细节"——
///    本批次的拍板结果是：`victim` 未具名时不产出**完整记录**（即便
///    `killer` 已具名，例如玩家杀死一只从未被记住的哥布林）。真正做到
///    "玩家相关全记，不论对方是否具名"需要在这里对 `victim` 也做懒
///    分配，但那需要先确认懒分配发生在 `apply`（`resolve` 不能碰
///    `&mut WorldState`，C1）、且这次懒分配不会与同一批效果里其他
///    `Effect` 的 `apply` 顺序产生新的竞态——这是比"五条硬要求"更大
///    的一块工作，本批次如实记录为已知缺口，不假装已经实现了完整的
///    三档分级。
///
/// `killer` 是否具名完全独立判断——具名与否只影响
/// `KillRecord.killer` 是 `Some` 还是 `None`（见
/// `WorldState::record_kill` 文档「killer 不做懒分配」一节），不影响
/// 「要不要记录」这个判断本身，也不影响是否累加聚合计数（决策二之后
/// 聚合计数不再看具名与否）。
fn append_kill_history(world: &WorldState, effects: &mut Vec<Effect>) {
    let mut kill_index = 0;
    while kill_index < effects.len() {
        let Effect::Kill {
            target,
            killer,
            cause,
        } = &effects[kill_index]
        else {
            kill_index += 1;
            continue;
        };
        let (target, killer, cause) = (*target, *killer, *cause);
        let Some(victim_agent) = world.actors.get(target) else {
            kill_index += 1;
            continue;
        };

        // 决策二：聚合计数数全部击杀，不论 victim 是否具名——kind 取
        // 受害者的 creature_kind，为 None 时回退到 race（见
        // Effect::IncrementKillCount 文档「为什么按 kind: ContentIndex」
        // 一节，与 Agent::creature_kind 字段文档同一条既有回退规则，不
        // 是本函数新发明的判断）。必须插在 Kill 之前——理由与
        // RecordHistoricalEvent 同一条（见 Effect::IncrementKillCount
        // 文档「为什么必须排在对应的 Effect::Kill 之前」一节）。
        let kind = victim_agent.creature_kind.unwrap_or(victim_agent.race);
        effects.insert(kill_index, Effect::IncrementKillCount { kind });
        kill_index += 1; // 跳过刚插入的计数效果。

        if victim_agent.remembered_id.is_some() {
            // 具名死者在聚合计数之外额外产出一份完整记录——决策二之后
            // 两者叠加，不再互斥，见本函数文档「决策二」一节。
            //
            // 这一下的伤害量：同一批效果里，`resolve_attack`/
            // `resolve_use_skill` 恒先产出对同一 target 的
            // `Effect::Damage`，再产出 `Effect::Kill`（见两者文档）——
            // 这里从已经产出的效果里读回那个数字，而不是重新计算一遍
            // 伤害公式（那属于 resolve_attack/resolve_use_skill 各自的
            // 职责，本函数不应该重复一遍规则判断）。查不到时按 0 处理
            // ——理论上不会发生，是防御性兜底，不是设计允许的正常路径。
            let damage = effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Damage { target: t, amount } if *t == target => Some(*amount),
                    _ => None,
                })
                .unwrap_or(0);
            let record = Effect::RecordHistoricalEvent {
                at: world.clock,
                location: victim_agent.pos,
                victim: target,
                killer,
                cause,
                damage,
                remaining_health: victim_agent.health - damage,
            };
            effects.insert(kill_index, record);
            kill_index += 1; // 跳过刚插入的记录。
        }
        kill_index += 1; // 跳到真正的 Kill 之后。
    }
}

/// 算出「从现在起 `cost` 个 tick 之后」的世界时刻。
fn schedule_after(world: &WorldState, cost: u32) -> Tick {
    Tick(world.clock.0 + i64::from(cost))
}

/// 原地等待一回合：消耗基础代价；若发起者正在休息
/// （`resource-pools-and-rest.md` 七、八节），额外检查这次行动结束时
/// 是否已到达 `target_ticks`——到达则先追加恢复批次再清空休息状态，
/// 否则休息状态原样保留（继续休息，不产生任何 resting 相关效果）。
///
/// # 完成判据：`world.clock + 本次行动耗时 >= started_at + target_ticks`
///
/// 与设计文档七节原文一致——判断的是「这一步等待做完之后」是否已经
/// 到达目标时刻，不是「这一步开始时」，理由同 [`resolve_use_skill`]
/// 冷却判定的既有比较方向：世界照常推进，玩家连续提交 `Intent::Wait`
/// 直到这个比较成立为止。
///
/// # 为什么这是防刷漏洞的主防线
///
/// 恢复批次只在这个比较判定为真的**那一刻**产出——不存在任何按「已经
/// 过了多少 tick」比例发放的代码路径。「休息一回合、取消」重复任意
/// 多次，这个比较从未成立（除非 `target_ticks` 恰好等于一次基础行动
/// 的耗时），因此从不触发恢复批次，见
/// `resource-pools-and-rest.md` 八节「刷恢复漏洞——两条独立防线」
/// 一节。
fn resolve_wait(
    world: &WorldState,
    actor: EntityId,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    let next_at = schedule_after(world, cost);

    let mut effects = Vec::new();
    if let Some(rest) = agent.resting {
        let target_at = rest
            .started_at
            .0
            .saturating_add(i64::from(rest.target_ticks));
        if next_at.0 >= target_at {
            effects.extend(rest_completion_effects(
                agent,
                actor,
                race_traits,
                traits,
                pools,
            ));
            effects.push(Effect::ClearResting { actor });
        }
    }
    effects.push(Effect::ScheduleNext { actor, at: next_at });
    effects
}

/// 开始一段休息会话——`Intent::Rest` 只用来**开始**这段会话（模块文档
/// 「七节」，`Intent::Rest` 文档）：若发起者当前未在休息
/// （`agent.resting.is_none()`），产出 `Effect::BeginRest` +
/// 与 [`resolve_wait`] 相同的 `Effect::ScheduleNext`；若已经在休息中
/// （脚本/AI 没有切换成 `Intent::Wait`，仍然反复提交 `Intent::Rest`），
/// 按继续休息处理，直接委托给 [`resolve_wait`] 走同一条完成/中断检查
/// ——不应该因为发起者选择了哪个 `Intent` 变体而让"继续休息"这件事
/// 表现出不同的语义。
fn resolve_rest(
    world: &WorldState,
    actor: EntityId,
    target_ticks: u32,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    if agent.resting.is_some() {
        return resolve_wait(world, actor, race_traits, traits, pools);
    }
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![
        Effect::BeginRest {
            actor,
            target_ticks,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// 休息正常完成时的恢复批次——遍历 `agent` 当前 [`effective_traits`]
/// 命中的每一条天赋的 `granted_resource_pools`，对恢复节奏含
/// `RegenRule::OnRest` 的池各产出对应效果，见
/// `resource-pools-and-rest.md` 七节「休息完成时恢复什么」一节。
///
/// # 为什么按「去重后的池」而不是按「每条命中的授予声明」产出效果
///
/// 与 [`resolve_resource_pool_regen`]（`OnTurnStart`）刻意不同——那里
/// 每条命中的授予声明各自贡献一次固定恢复量，多个来源各自独立叠加是
/// 正确语义（该函数文档「为什么按每条命中的授予声明」一节）。`OnRest`
/// 不同：`RestRecoveryAmount::Full` 只有相对**这个池的总容量**才有
/// 意义（不存在"这一条授予声明各自的满"这种概念），因此这里先按池去重，
/// 对每个池只查询一次总容量、只产出一批恢复效果，不会因为同一个池被
/// 两条天赋各自授予容量就重复产出两次"回满"。
fn rest_completion_effects(
    agent: &Agent,
    actor: EntityId,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
) -> Vec<Effect> {
    let mut seen_pools: Vec<ContentIndex> = Vec::new();
    let mut effects = Vec::new();
    for trait_id in effective_traits(agent.race, agent.level, race_traits) {
        let Some(rule) = traits.trait_rule(trait_id) else {
            continue;
        };
        for grant in &rule.granted_resource_pools {
            if seen_pools.contains(&grant.pool) {
                continue;
            }
            let Some(pool_rule) = pools.resource_pool(grant.pool) else {
                continue;
            };
            let RegenRule::OnRest { amount } = pool_rule.regen_rule else {
                continue;
            };
            seen_pools.push(grant.pool);
            match pool_rule.shape {
                ResourcePoolShape::Scalar => {
                    if let Some(effect) =
                        scalar_rest_effect(agent, actor, grant.pool, amount, race_traits, traits)
                    {
                        effects.push(effect);
                    }
                }
                ResourcePoolShape::TieredSlots { tier_count } => {
                    effects.extend(tiered_slot_rest_effects(
                        agent, actor, grant.pool, tier_count, amount,
                    ));
                }
            }
        }
    }
    effects
}

/// 标量池的休息恢复——[`rest_completion_effects`] 的帮手。`Full` 恢复到
/// 当前有效容量（`delta = capacity - stored_current`，`stored_current`
/// 超过容量时不倒扣，见下方 `max(0, ..)`）；`Amount(n)` 恢复固定量，
/// 与 `RegenRule::OnTurnStart` 同一条「不做写入端钳位，容量只在读取时
/// 现场钳位」纪律（`resource-pools-and-rest.md` 三节「上限变化时怎么
/// 办」一节），不查容量。`delta` 为零时不产出效果（没有变化，不需要
/// 一条空操作的 `Effect`）。
fn scalar_rest_effect(
    agent: &Agent,
    actor: EntityId,
    pool: ContentIndex,
    amount: RestRecoveryAmount,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Option<Effect> {
    let delta = match amount {
        RestRecoveryAmount::Full => {
            let capacity =
                effective_scalar_capacity(agent.race, agent.level, pool, race_traits, traits);
            let current = agent.resource_pools.get(&pool).copied().unwrap_or(0);
            (i64::from(capacity) - i64::from(current)).max(0)
        }
        RestRecoveryAmount::Amount(n) => i64::from(n),
    };
    if delta == 0 {
        return None;
    }
    Some(Effect::AdjustResourcePool {
        actor,
        pool,
        delta: delta.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    })
}

/// 法术位池的休息恢复——[`rest_completion_effects`] 的帮手。`Full`
/// 恢复：每一档的已消耗数清零（不需要查容量,"回满"对法术位而言就是
/// "已消耗数归零",与容量无关——见 `RestRecoveryAmount::Full` 文档）。
/// `Amount(n)` 恢复：从第 1 档起,按顺序清掉总计 `n` 个已消耗槽位——与
/// 消耗算法"从最低阶开始取"对称,理由同 `RestRecoveryAmount::Amount`
/// 文档。只对 `agent.spent_slots` 里已经存在的 `(pool, tier)` 条目产出
/// 效果,已消耗数恒为零的档位不需要一条空操作的 `Effect`。
fn tiered_slot_rest_effects(
    agent: &Agent,
    actor: EntityId,
    pool: ContentIndex,
    tier_count: u8,
    amount: RestRecoveryAmount,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    match amount {
        RestRecoveryAmount::Full => {
            for tier in 1..=tier_count {
                let spent = agent.spent_slots.get(&(pool, tier)).copied().unwrap_or(0);
                if spent > 0 {
                    effects.push(Effect::AdjustResourceSlot {
                        actor,
                        pool,
                        tier,
                        delta: -(spent as i32),
                    });
                }
            }
        }
        RestRecoveryAmount::Amount(n) => {
            effects.extend(restore_slots_from_lowest_tier(
                agent, actor, pool, tier_count, n,
            ));
        }
    }
    effects
}

/// 朝某方向移动一格：按目的地的地形分三种情形处理。
///
/// - 目的地是一格「撞入即开」的地形（[`ll_world::terrain::TerrainTable::opens_into`]
///   有值，例如关着的门）：产生把该格改写成 `opens_into` 目标地形的
///   效果，而不是移动效果——门挡住了这一步，但「撞门」本身是有意义的
///   动作，不该像撞墙一样什么都不发生。**这条规则是任何地形都能声明的
///   属性，不是只对某个硬编码地形 ID 生效的特判**——见
///   `ll_world::terrain` 模块文档「`opens_into`」一节：这正是本次迁移
///   撞见并修掉的一处 API 洞，mod 现在可以给自己的地形也声明同样的
///   行为。
/// - 目的地完全不可通行（墙、窗等）：**不产生 `Effect::MoveTo`，但仍
///   产生 `Effect::ScheduleNext`**——项目所有者决策：撞墙本身也是一次
///   真实的行动尝试（伸手推了一下、发现推不开），应当消耗时间，只是
///   位置不变；耗时按 [`BASE_ACTION_COST`] 计费，不查地形的 `move_cost`
///   （那是「走完整段距离」的代价，撞墙这一步根本没有走完，用它定价
///   不成立，见 [`resolve_wait`] 同样按基准代价计费的理由）。
/// - 目的地可通行：产生移动效果，行动耗时按该地形的分级 `move_cost`
///   计算——浅水、山地这类「过得去但更慢」的地形因此耗时更长；若移动的
///   是玩家自己，额外追加一条 [`Effect::MarkExplored`]（见其文档），
///   把探索记忆的写入接到这唯一的移动落点。
///
/// # 为什么只有玩家移动才追加 `MarkExplored`
///
/// 本函数同时服务玩家与 NPC——`actor` 是任意实体。[`WorldState::exploration`]
/// 却只代表玩家一个人的视角（见其字段文档「为什么按角色只存一份」）。
/// 若不加区分地让每个 NPC 的移动都追加一条 `MarkExplored`，游荡的怪物
/// 会替玩家「看见」它们自己路过的地方——那是把探索记忆的语义换成了
/// 「世界上任意实体去过哪」，与「玩家亲眼见过哪」是两个不同的东西，
/// 后者才是战争迷雾要回答的问题。这里用 `world.player_entity ==
/// Some(actor)` 这一个比较收住范围，不需要改 `Intent`/`Effect` 的
/// 形状去区分「谁在动」。
/// [`Intent::PickUp`] 结算（P6 第二批：背包与地面物品）：捡起 `actor`
/// 脚下的第一堆地面物品（见 `Intent::PickUp` 文档「为什么不指定要捡
/// 哪一种」一节），若背包已有可合并的同种堆（[`can_merge`]），一并
/// 算出合并结果。
///
/// # 静默无效的两种情形
///
/// `actor` 不存在，或脚下没有任何地面物品——与 `resolve_attack`/
/// `resolve_open_door` 目标不存在时的既有纪律一致（见模块文档开篇
/// 「目标实体……若已不在 `world.actors` 中……一律返回空 `Vec`」），
/// 不是错误，只是这一步什么都不发生。
///
/// # 为什么合并结果由这里算好，`apply` 只做替换
///
/// 见 [`Effect::MergeIntoInventory`] 文档「为什么合并结果由 `resolve`
/// 算好」一节：`stack_limit` 查不到（`items` 没有这个 `def` 的记录）
/// 时按「不限量」处理（`u32::MAX`），理由见 [`NoItems`] 文档——没有
/// 真实的物品注册表可查不该表现成"这件物品异常地不能堆叠"。
fn resolve_pick_up(world: &WorldState, actor: EntityId, items: &dyn ItemCatalog) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(ground) = world.ground_items.iter().find(|item| item.pos == agent.pos) else {
        return Vec::new();
    };
    let picked = ground.stack;

    vec![
        Effect::RemoveGroundItem {
            pos: ground.pos,
            def: picked.def,
        },
        merge_into_inventory_effect(agent, actor, picked, items),
    ]
}

/// 把 `incoming` 这一堆物品合并进 `agent` 背包，产出对应的
/// [`Effect::MergeIntoInventory`]——[`resolve_pick_up`]/[`resolve_equip`]
/// （卸下冲突槽位时）/[`resolve_unequip`] 三处共用同一段"找可合并的
/// 旧堆→算合并结果"逻辑，理由是三者都要回答同一个问题："这一堆物品
/// 放进背包后，背包状态该变成什么样"——`resolve_pick_up` 落地时
/// （P6 第二批）这段逻辑还只有它一处调用，装备栏位批次（P6 第三批）
/// 新增两处调用点后再抽取成帮手，避免三份几乎相同的代码分别漂移。
///
/// # 已知限制：不处理"同一批效果里两个新增堆本身能互相合并"的情形
///
/// 见 [`Effect::MergeIntoInventory`] 文档「为什么合并结果由 `resolve`
/// 算好」一节：`agent` 是调用方传入的**只读快照**，若 `resolve_equip`
/// 因双手武器占位冲突要连续卸下两件本可以互相合并的同类物品（例如
/// 两个完全相同的戒指各自被不同规则挤占），本函数各自独立基于同一份
/// 背包快照判断"有没有可合并的旧堆"，不会让这两个新卸下的堆彼此合并
/// ——不产生数据错误（数量守恒，物品不会丢失或复制），只是错过一次
/// 本可以做的合并。这是一个真实但边缘的场景（要求两件不同槽位的
/// 装备恰好实例状态完全相同），本批次不为它引入"batch 内部先自我
/// 合并一遍"的额外机制（YAGNI）。
fn merge_into_inventory_effect(
    agent: &Agent,
    actor: EntityId,
    incoming: ItemStack,
    items: &dyn ItemCatalog,
) -> Effect {
    let existing = agent
        .inventory
        .iter()
        .find(|stack| can_merge(stack, &incoming));
    let (replaced, resulting) = match existing {
        Some(existing) => {
            let stack_limit = items
                .item(incoming.def)
                .map_or(u32::MAX, |rule| rule.stack_limit);
            match merge_stacks(*existing, incoming, stack_limit) {
                Ok((merged, overflow)) => {
                    let mut resulting = vec![merged];
                    resulting.extend(overflow);
                    (Some((existing.def, existing.durability)), resulting)
                }
                Err(_) => {
                    // can_merge 刚判定过真——merge_stacks 只会在 def/
                    // durability 不同时拒绝（见其文档），这里理论不可达，
                    // 保守回落到"不合并、直接追加"而不是 panic。
                    (None, vec![incoming])
                }
            }
        }
        None => (None, vec![incoming]),
    };
    Effect::MergeIntoInventory {
        actor,
        replaced,
        resulting,
    }
}

/// [`Intent::Drop`] 结算（P6 第二批：背包与地面物品）：把 `actor` 背包
/// 里第一条匹配 `def` 的整堆丢在其当前脚下（见 `Intent::Drop` 文档
/// 「为什么是整堆」一节）。
///
/// # 静默无效的两种情形
///
/// `actor` 不存在，或背包里没有匹配 `def` 的堆——与 [`resolve_pick_up`]
/// 同一条纪律。
fn resolve_drop(world: &WorldState, actor: EntityId, def: ContentIndex) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(stack) = agent.inventory.iter().find(|stack| stack.def == def) else {
        return Vec::new();
    };

    vec![
        Effect::RemoveFromInventory {
            actor,
            def,
            durability: stack.durability,
        },
        Effect::AddGroundItem {
            pos: agent.pos,
            stack: *stack,
            dropped_at: world.clock,
        },
    ]
}

/// [`Intent::Equip`] 结算（装备栏位批次，P6 第三批）：把 `actor` 背包
/// 里第一条匹配 `def` 的堆装备起来，落地
/// `knowledge/design/equipment-slots.md`「装备流程」一节——
/// 「一条规则覆盖所有特例」：装备时找出**全部**与新物品掩码相交的
/// 已装备物品,逐一卸下（写回背包）,再把新物品写入它的锚点槽位。
///
/// # 静默无效的三种情形
///
/// `actor` 不存在、背包里没有匹配 `def` 的堆、`def` 不可装备
/// （`items` 查不到这条物品的规则，或查到但 `equip_mask ==
/// SlotMask::EMPTY`）——与 [`resolve_pick_up`]/[`resolve_drop`] 同一条
/// 「静默无效，不是错误」纪律。**查不到物品规则时按"不可装备"处理，
/// 不是"不限量"**——与 `resolve_pick_up` 对 `stack_limit` 查不到时的
/// 「按不限量处理」方向相反（该函数文档已指出这条不对称本身是刻意
/// 的）：一件连规则都查不到的物品，没有任何证据证明它能装备到任何
/// 槽位，装备系统必须要求内容明确声明"占用哪些槽位"才能生效,这与
/// `NoItems`/未注册物品在其它路径上的"宽容"取向不同——装备是会产生
/// 持久世界状态变化（写入 `Agent.equipment`）的操作,`resolve_pick_up`
/// 的"不限量"只是放宽一个数量上限,两者的保守方向本就不该一致。
///
/// # 占位冲突：找出全部相交的已装备物品
///
/// 遍历 `agent.equipment` 的每一条 `(锚点槽位, 已装备堆)`，查询该堆
/// 自身的 `equip_mask`（依赖 `items` 目录——若查不到已装备物品自身的
/// 规则，保守视为 `SlotMask::EMPTY`，即"当作不占用任何槽位、不冲突"，
/// 理由是"能查到规则的物品才谈得上有冲突"，与本函数对`def`本身查不到
/// 规则时拒绝装备是不同的方向：前者是"新物品必须证明自己能装备"，
/// 后者是"老物品的冲突判定退化不应该无端阻塞新物品的装备"，两条保守
/// 方向服务的是同一个目标——装备栏状态不因为目录查询残缺而卡死）,
/// 与新物品的掩码相交即视为冲突,产出 `Effect::Unequip` +
/// [`merge_into_inventory_effect`]（卸下的物品放回背包）。
fn resolve_equip(
    world: &WorldState,
    actor: EntityId,
    def: ContentIndex,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(stack) = agent.inventory.iter().find(|s| s.def == def).copied() else {
        return Vec::new();
    };
    let Some(rule) = items.item(def) else {
        return Vec::new();
    };
    let new_mask = rule.equip_mask;
    if new_mask == SlotMask::EMPTY {
        return Vec::new();
    }
    let Some(anchor) = new_mask.anchor_slot() else {
        return Vec::new();
    };

    let mut effects = Vec::new();
    for (&existing_anchor, &existing_stack) in &agent.equipment {
        let existing_mask = items
            .item(existing_stack.def)
            .map_or(SlotMask::EMPTY, |rule| rule.equip_mask);
        if existing_mask.intersects(new_mask) {
            effects.push(Effect::Unequip {
                actor,
                slot: existing_anchor,
            });
            effects.push(merge_into_inventory_effect(
                agent,
                actor,
                existing_stack,
                items,
            ));
        }
    }

    effects.push(Effect::RemoveFromInventory {
        actor,
        def,
        durability: stack.durability,
    });
    effects.push(Effect::Equip {
        actor,
        slot: anchor,
        stack,
    });
    effects
}

/// [`Intent::Unequip`] 结算（装备栏位批次，P6 第三批）：卸下玩家请求
/// 槽位对应的已装备物品，写回背包。
///
/// # 为什么要把请求槽位翻译成锚点槽位
///
/// `Agent.equipment` 只以**锚点槽位**为键（见其文档「为什么以锚点
/// 槽位为键」一节）——玩家请求的 `slot` 若恰好是某个横跨多槽物品
/// （双手武器）的**非锚点**槽位（例如请求卸下 `OFF_HAND`，但双手武器
/// 实际存储键是 `MAIN_HAND`），直接拿 `slot` 去查
/// `agent.equipment.get(slot)` 会查不到——从玩家视角这是一个可见的
/// bug（"我副手明明有东西，为什么卸不下来"）。本函数因此不做直接查表，
/// 而是遍历全部已装备条目，用 `items` 目录现算每一条的完整 `equip_mask`，
/// 找到"掩码覆盖了请求槽位"的那一条，用它的**真实存储键**产出
/// `Effect::Unequip`。
///
/// # 静默无效的两种情形
///
/// `actor` 不存在，或没有任何已装备条目覆盖 `slot`——与
/// [`resolve_drop`] 同一条纪律。查不到某条已装备物品自身规则时按
/// `SlotMask::EMPTY` 处理（视为不覆盖任何槽位），理由同 [`resolve_equip`]
/// 「占位冲突」一节同一段说明。
fn resolve_unequip(
    world: &WorldState,
    actor: EntityId,
    slot: EquipSlot,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };

    let found = agent.equipment.iter().find(|(_, stack)| {
        items
            .item(stack.def)
            .map_or(SlotMask::EMPTY, |rule| rule.equip_mask)
            .contains_slot(slot)
    });
    let Some((&anchor, &stack)) = found else {
        return Vec::new();
    };

    vec![
        Effect::Unequip {
            actor,
            slot: anchor,
        },
        merge_into_inventory_effect(agent, actor, stack, items),
    ]
}

fn resolve_move(world: &WorldState, actor: EntityId, dir: Direction) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // Interior 内部漫游不在本批次范围内——见模块文档「Interior 内部
    // 移动的范围边界」一节。静默无效，不改 agent.pos，保住「进入
    // Interior 后 Agent.pos 不变」这条不变式。
    if !matches!(agent.current_space, Space::Surface { .. }) {
        return Vec::new();
    }
    let (dx, dy) = dir.delta();
    let dest = world.size.wrap(agent.pos.x() + dx, agent.pos.y() + dy);
    // resolve 必须是纯函数（C1），不能触发 SurfaceStore 的按需生成——
    // 见 WorldState::terrain_at 文档「resolve 只读、加载收窄到……」。
    // 目的地所属区块尚未常驻时（真正的邻域缓冲维护接线是设计文档
    // 任务 14 的范围，本次迁移之后正常游玩路径下应恒已常驻），这是防御
    // 性兜底而非玩家能在正常游玩中触发的情形，保守地不产生任何效果、
    // 也不消耗时间——不是让整个结算 panic。**与下方撞墙分支不同**：
    // 撞墙是「查得到地形、确认过不去」的确定结果，值得消耗一次行动；
    // 这里根本查不到地形，无法判断这一步「本该」耗时多久，静默作废
    // 更安全。
    let Some(terrain) = world.terrain_at(dest) else {
        return Vec::new();
    };
    let speed = effective_speed_from_dexterity(agent.stats.dexterity);

    if let Some(open_kind) = terrain.opens_into(&world.terrain_table) {
        let cost = action_cost(BASE_ACTION_COST, speed);
        return vec![
            Effect::SetTerrain {
                pos: dest,
                kind: open_kind,
            },
            Effect::ScheduleNext {
                actor,
                at: schedule_after(world, cost),
            },
        ];
    }

    if terrain.blocks_move(&world.terrain_table) {
        // 撞墙仍消耗时间——见本函数文档「目的地完全不可通行」一节。
        // 位置不变（不产生 `Effect::MoveTo`），只推进时间轴。
        let cost = action_cost(BASE_ACTION_COST, speed);
        return vec![Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        }];
    }

    let cost = action_cost(terrain.move_cost(&world.terrain_table), speed);
    let mut effects = vec![
        Effect::MoveTo { actor, pos: dest },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ];
    // 只在移动者是玩家、且这一步真的挪动了位置（本分支恒如此）时追加
    // 探索标记——见本函数文档「为什么只有玩家移动才追加」一节。没有
    // `MoveTo` 就不该有 `MarkExplored`：站着不动（`Intent::Wait`）或
    // 撞墙（上面 `blocks_move` 分支提前返回空 `Vec`）都不会走到这里，
    // 天然不会为「原地不动」重复标记同一批格子，这正是避免每帧全量
    // 重写探索位图的做法（见 `Effect::MarkExplored` 文档「何时才触发」
    // 一节）。
    if world.player_entity == Some(actor) {
        effects.push(Effect::MarkExplored {
            origin: dest,
            radius: EXPLORATION_SIGHT_RADIUS,
        });
    }
    effects
}

/// 直接攻击一个已知目标（与 [`resolve_move`] 的隐式派生分开的显式路径，
/// 供已经知道目标的调用方——例如已锁定目标的 AI ——直接使用）。
///
/// 攻击力：裸力量值经 [`effective_attribute`] 叠加
/// [`ll_world::entity::Agent::active_stat_modifiers`] 里力量项的临时
/// 修正（技能增益/削弱由此接线生效，见该函数文档）。
///
/// 防御与穿透：本批次 `Agent` 还没有护甲字段（护甲属于装备系统，
/// P5 才落地；`AttributeKind` 六个变体里也没有对应「护甲/防御」的
/// 一项，见 `knowledge/design/vehicle-and-mounting.md` 六节），故这里
/// 固定传 `defense = 0`、`pen = Penetration::NONE`。[`damage_after_defense`]
/// 本身的穿透/下限行为已经由 `combat.rs` 的单元测试独立验证正确，这里
/// 只是先把攻击端接线接上；防御端要等 `derive_stats`「装备」输入落地、
/// 产出真实 `DerivedStats.armor` 之后才有值可读，不是这一批次能造的
/// 数据。
///
/// 若这一下会让目标生命值降到零或以下，额外产出一个 [`Effect::Kill`]
/// ——是否致死是规则判断，必须在这里（`resolve`）做出，`apply` 只管
/// 照数字做加减（见 [`crate::effect::Effect::Damage`] 文档）。
fn resolve_attack(world: &WorldState, actor: EntityId, target: EntityId) -> Vec<Effect> {
    let Some(attacker) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(defender) = world.actors.get(target) else {
        return Vec::new();
    };

    let attack_power = effective_attribute(
        attacker.stats.strength,
        AttributeKind::Strength,
        &attacker.active_stat_modifiers,
        world.clock,
    );
    let damage = damage_after_defense(attack_power, 0, Penetration::NONE);

    let mut effects = vec![Effect::Damage {
        target,
        amount: damage,
    }];
    if defender.health - damage <= 0 {
        // 近战击杀——本批次没有武器系统（护甲/武器均属 P5 装备落地
        // 之后，见本函数文档「防御与穿透」一节），`weapon` 恒
        // `None`，与「徒手」在类型上无法区分，是当前已知的诚实简化，
        // 见 `ll_world::history::KillCause::Melee` 文档。
        effects.push(Effect::Kill {
            target,
            killer: Some(actor),
            cause: KillCause::Melee { weapon: None },
        });
    }

    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(attacker.stats.dexterity),
    );
    effects.push(Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    });
    effects
}

/// 开启某处的门：目的地不是一格「撞入即开」的地形时，位置与地形都不
/// 变，但仍消耗一次行动的时间——与 [`resolve_move`] 撞墙时的处理是
/// 同一类判断（都是「查得到目标、确认这个动作在此处不成立」的确定
/// 结果，值得消耗一次行动，而不是像目标区块未常驻那样彻底放弃判断）,
/// 见 [`resolve_move`] 文档「目的地完全不可通行」一节；这里同样查表，
/// 不再恒等比较某个硬编码地形 ID，见其「`opens_into`」一节。
///
/// 目的地所属区块尚未常驻（`world.terrain_at` 落空）是另一种情形，
/// 与 [`resolve_move`] 对应分支同一条纪律：无法判断这一步「本该」耗时
/// 多久，静默作废、不消耗时间。
fn resolve_open_door(world: &WorldState, actor: EntityId, pos: (i32, i32)) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let door_pos = world.size.wrap(pos.0, pos.1);
    // 同 resolve_move：只读查询，未常驻时无法判断耗时，静默作废、不
    // panic、不触发生成、不消耗时间——见本函数文档。
    let Some(terrain) = world.terrain_at(door_pos) else {
        return Vec::new();
    };
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    let Some(open_kind) = terrain.opens_into(&world.terrain_table) else {
        // 目标不是（或已经不是）一扇能开的门——仍消耗时间,见本函数
        // 文档。位置与地形都不变,只产出排期效果。
        return vec![Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        }];
    };

    vec![
        Effect::SetTerrain {
            pos: door_pos,
            kind: open_kind,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// 尝试进入 `target` 这个具体的 `Interior` 空间实例。
///
/// 三重校验，任一失败都静默作废（不产生效果，与撞墙同一种处理）：
/// 1. `actor` 当前必须在地表——已经在某个 `Interior` 里时不允许直接
///    「传送」进另一个（不支持 `Interior` 嵌套 `Interior`，本批次范围
///    之外）。
/// 2. `target` 必须真实存在于 `world.interiors`。
/// 3. `target` 的入口锚点必须等于 `actor` 当前所在的世界格——玩家必须
///    真的站在入口上，不能隔空进入。
///
/// 通过校验后，进入哪一层由 [`entry_floor`] 决定。
fn resolve_enter_space(world: &WorldState, actor: EntityId, target: SpaceId) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    if !matches!(agent.current_space, Space::Surface { .. }) {
        return Vec::new();
    }
    let Some(interior) = world.interiors.get(target) else {
        return Vec::new();
    };
    if interior.anchor != agent.pos {
        return Vec::new();
    }
    let Some(floor) = entry_floor(interior) else {
        return Vec::new();
    };
    vec![Effect::ChangeSpace {
        actor,
        space: Space::Interior {
            id: target,
            floor,
            anchor: interior.anchor,
            profile: interior.profile,
        },
    }]
}

/// 从入口进入 `Interior` 时应该落在哪一层：优先取 0 层（约定俗成的
/// 「地面层」），若这个 `Interior` 恰好没有 0 层（稀疏楼层，见
/// [`ll_world::interior`] 模块文档「稀疏性」一节），退而取已生成楼层里
/// 编号最小的一个。若一层都还没生成，返回 `None`——这不是编程错误
/// （`Interior` 允许先插入实例、楼层由生成器按需补齐，见其模块文档
/// 「与共享常驻预算的关系」），只是这一步无法进入，与撞墙同一种
/// 「静默作废」处理。
fn entry_floor(interior: &ll_world::interior::Interior) -> Option<i16> {
    let floors = interior.floor_numbers();
    if floors.contains(&0) {
        Some(0)
    } else {
        floors.first().copied()
    }
}

/// 退出当前所在的 `Interior`，返回地表。
///
/// 在地表触发（`agent.current_space` 不是 `Interior`）时静默作废——见
/// 模块文档「已知的范围边界」一节的同一套处理方式。
///
/// 产出两个效果：把 `current_space` 换回地表（`profile` 取自
/// [`WorldState::surface_profile`]，见模块文档「`Interior` 退出如何
/// 拿到地表 profile」一节），以及把 `pos` 显式写回 `Interior` 的锚点
/// ——`Interior` 内部漫游本批次不接线（见模块文档），`pos` 理论上从
/// 进入起就没变过，这里仍然显式写一遍而不是依赖「反正没人动过它」：
/// 显式写入让这条不变式不依赖调用方是否恰好遵守了另一条完全不同的
/// 规则（`resolve_move` 对 `Interior` 静默无效），两条防线互相独立更
/// 安全。
fn resolve_exit_space(world: &WorldState, actor: EntityId) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Space::Interior { anchor, .. } = agent.current_space else {
        return Vec::new();
    };
    let (zone, _) = world.terrain.layout().tile_to_zone(anchor);
    vec![
        Effect::ChangeSpace {
            actor,
            space: Space::surface(zone, world.surface_profile),
        },
        Effect::MoveTo { actor, pos: anchor },
    ]
}

/// 使用一个技能（P5-B 任务 5）：四道门都不通过，静默作废（不产生任何
/// 效果），与本文件其余分支「动作在这个世界里无意义」的既有纪律一致
/// ——「技能不存在」「未解锁」「冷却中」「资源不足」四种情形对调用方
/// 而言是同一件事（这一次施放没有发生），不需要用不同的返回形状区分。
///
/// # 「本体即 Mod」检验：不对 `skill` 做任何 `if == 某个具体 ID` 判断
///
/// 全部四道门都只读 `agent`/`skills.skill(skill)` 返回的通用数据，产出
/// 效果那一步同样只是对 [`SkillEffect`] 的变体做 `match`——不出现任何
/// 硬编码的技能 `ContentIndex` 比较。一个从未被本文件认识过的、由假想
/// mod 注册的技能，只要能通过调用方提供的 [`SkillCatalog`] 查到，就会
/// 被这条完全相同的通用路径正确处理，见
/// `本体技能与假想mod技能走同一条resolve通用路径` 测试。
///
/// # `DealDamage` 与 `resolve_attack` 共享同一条致死判定纪律
///
/// 若这一下会让目标生命值降到零或以下，额外产出一个 [`Effect::Kill`]
/// ——与 [`resolve_attack`] 完全同一条纪律（见其文档）：是否致死是
/// 规则判断，必须在这里（`resolve`）做出，`apply` 只管照数字做加减。
/// 这一步此前缺失，技能永远打不死目标，也永远不会推进
/// [`append_quest_kill_progress`] 依赖的击杀任务进度——两处结算同属
/// 引擎侧，死亡判定没有设计自由度，属于纯实现缺口，不是分层错误。
///
/// # 性能：门一的 `granted_skills` 现算，不缓存——调用频率核实
///
/// `crate::traits::granted_skills` 每次门一判定都现场遍历一遍种族的
/// `TraitGrant` 列表 + 命中天赋各自的 `granted_skills`，不做任何缓存。
/// 这条路径**不是**逐 tick 热路径：`resolve_use_skill` 只在
/// `Intent::UseSkill` 被结算时调用一次，而 `Intent::UseSkill` 只在
/// 一个实体主动选择使用技能的那个回合才会出现（与 `Intent::Wait`/
/// `Intent::Move` 这类每回合恒有的意图不同）——一场战斗里一个实体
/// 一回合最多用一次技能，量级与 `resolve_attack` 每次普通攻击查询
/// 一次减伤公式相同，不是 `ll_world::fov`/地形查询那种逐格/逐 tick
/// 路径。种族目前最多声明个位数天赋、一个天赋最多声明个位数
/// `granted_skills`，`Vec::contains`/`Vec` 遍历在这个规模下的常数
/// 开销可以忽略——若未来某个种族/天赋的列表规模显著增长（远超「一个
/// 内容作者手写的静态声明」这个量级），届时再考虑缓存，本批次不为
/// 一个尚不存在的性能问题预先设计缓存策略（YAGNI）。
fn resolve_use_skill(
    world: &WorldState,
    actor: EntityId,
    skill: ContentIndex,
    target: Option<EntityId>,
    skills: &dyn SkillCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // 门一：技能必须已解锁，或者是种族天赋授予的（`granted_skills`
    // 惰性现算，不缓存，见 `crate::traits` 模块文档「为什么不缓存」
    // 一节）——`knowledge/design/trait-system.md` 三节①「有效技能=
    // 并集」公式在本批次的唯一接线点：种族这一路来源（职业/副职/
    // 载具/buff 四路仍是 `granted_skills(agent.race)` 之外的空集合，
    // 见 `crate::traits` 模块文档「天赋归谁所有」一节的范围裁定）。
    if !agent.unlocked_skills.contains(&skill)
        && !granted_skills(agent.race, agent.level, race_traits, traits).contains(&skill)
    {
        return Vec::new();
    }
    // 门二：冷却判定——惰性判定，读取时现比对世界时钟，不要求
    // `skill_cooldowns` 主动清理过期条目（见 `Agent::skill_cooldowns`
    // 文档「有意留给后续阶段的缺口」一节）。
    if let Some(until) = agent.skill_cooldowns.get(&skill)
        && until.0 > world.clock.0
    {
        return Vec::new();
    }
    // 门三：技能必须能在调用方提供的目录里查到——查不到与「不满足任何
    // 使用条件」同等对待（ADR 0015：查不到就是查不到）。
    let Some(rule) = skills.skill(skill) else {
        return Vec::new();
    };
    // 门四：资源是否充足——`Amount`/`PoolAmount` 走同一条纪律（不足则
    // 整个技能静默不产出任何效果，与其余三道门一致）；`Blood` 代价
    // 刻意不设这道门,允许把施法者打死,理由见
    // `resource-pools-and-rest.md` 五节「不设 1 点血兜底」与
    // `crate::skill::ResourceCost::Blood` 文档。这条判定不是恒真：
    // `PoolAmount` 分支真的会在 `usable < amount` 时拒绝——法力不够时
    // 技能确实放不出来。
    match rule.resource_cost {
        ResourceCost::Amount(kind, amount) => {
            let current = current_resource(agent, kind);
            if current < i64::from(amount) {
                return Vec::new();
            }
        }
        ResourceCost::PoolAmount(pool, amount) => {
            if resource_pool_usable(agent, pool, race_traits, traits) < i64::from(amount) {
                return Vec::new();
            }
        }
        ResourceCost::SlotTier(pool, min_tier) => {
            if find_available_slot_tier(agent, pool, min_tier, race_traits, traits).is_none() {
                return Vec::new();
            }
        }
        ResourceCost::Blood(_) | ResourceCost::None => {}
    }

    // 四道门都通过：产出资源扣减（若有）、技能效果映射出的效果、冷却
    // 设置、以及与其余动作一致的排期效果。
    let mut effects = Vec::new();
    match rule.resource_cost {
        ResourceCost::Amount(kind, amount) => {
            effects.push(Effect::AdjustResource {
                actor,
                resource: kind,
                delta: -(amount as i32),
            });
        }
        ResourceCost::PoolAmount(pool, amount) => {
            effects.push(Effect::AdjustResourcePool {
                actor,
                pool,
                delta: -(amount as i32),
            });
        }
        ResourceCost::SlotTier(pool, min_tier) => {
            // 门四已经确认存在一个可用档位——这里重新查一次（`resolve`
            // 是纯函数，两次调用之间世界状态不会变化，重算不会得到不同
            // 结果，只是与既有 `Amount`/`PoolAmount` 分支同一种"门里只判
            // 断、效果产出时才真正决定写什么"的写法一致）。找不到（理论
            // 上不会发生，门四已经拦过）时静默不产出扣减，不 panic——
            // 与其余分支「防御性处理不可能到达但也不该崩溃的分支」是
            // 同一条既有纪律。
            if let Some(tier) = find_available_slot_tier(agent, pool, min_tier, race_traits, traits)
            {
                effects.push(Effect::AdjustResourceSlot {
                    actor,
                    pool,
                    tier,
                    delta: 1,
                });
            }
        }
        ResourceCost::Blood(amount) => {
            // 直接扣血,绕开减伤/抗性——见 `Effect::SpendBloodCost`/
            // `crate::skill::ResourceCost::Blood` 文档，**刻意不产出
            // `Effect::Damage`**：血代价链路必须从一开始就不经过
            // `damage_after_defense`,这里与 `resolve_attack`/
            // `DealDamage` 分支唯一的区别就是这一点。
            let cost = amount as i32;
            effects.push(Effect::SpendBloodCost {
                target: actor,
                amount: cost,
            });
            // 用血施法致死：与 `resolve_attack`/`DealDamage` 分支完全
            // 同构的既有纪律——结算前读 `caster.health - cost <= 0`,
            // 是否致死是规则判断，必须在这里（resolve）做出。不设 1 点
            // 血兜底，不在施法前拒绝——项目所有者的明确裁定，见
            // `resource-pools-and-rest.md` 五节。`killer` 填施法者自己
            // 而非 `None`：自尽的责任方明确是施法者本人。
            if agent.health - cost <= 0 {
                effects.push(Effect::Kill {
                    target: actor,
                    killer: Some(actor),
                    cause: KillCause::Skill { skill },
                });
            }
        }
        ResourceCost::None => {}
    }
    // 默认目标：未显式给出目标的技能施于自身（自我增益/恢复类技能的
    // 常见形状），见 `Intent::UseSkill::target` 文档。
    let effect_target = target.unwrap_or(actor);
    match rule.effect {
        SkillEffect::DealDamage { base } => {
            effects.push(Effect::Damage {
                target: effect_target,
                amount: base,
            });
            // 是否致死是规则判断，必须在这里（resolve）做出，`apply`
            // 只管照数字做加减——与 `resolve_attack` 同一条纪律（见其
            // 文档），此前这里漏掉了这一步：技能伤害因此永远不会真正
            // 杀死目标，也永远不会推进依赖 `Effect::Kill` 的击杀任务
            // 进度（`append_quest_kill_progress` 只扫描 `Effect::Kill`）。
            // 目标若已不在 `world.actors` 中（例如同一批效果里已被更早
            // 的 `Effect::Kill` 移除），静默跳过——与本文件其余分支对
            // 「目标不存在」的处理方式一致。
            if let Some(defender) = world.actors.get(effect_target)
                && defender.health - base <= 0
            {
                effects.push(Effect::Kill {
                    target: effect_target,
                    killer: Some(actor),
                    cause: KillCause::Skill { skill },
                });
            }
        }
        SkillEffect::RestoreResource { resource, base } => {
            effects.push(Effect::AdjustResource {
                actor: effect_target,
                resource,
                delta: base,
            });
        }
        SkillEffect::TemporaryStatModifier {
            attribute,
            amount,
            duration_ticks,
        } => {
            effects.push(Effect::ApplyStatModifier {
                target: effect_target,
                attribute,
                delta: amount,
                expires_at: Tick(world.clock.0 + i64::from(duration_ticks)),
                // 来源就是这次施放的技能自身——调用方（本函数）已经持有
                // `skill: ContentIndex` 这个参数，原样传入，不需要新查表
                // （`buffs-and-triggers.md` 六节①：来源是「施加这条修正
                // 的那份内容定义自己的 ContentIndex」，本函数正是这份
                // 定义的施加者）。
                source: skill,
            });
        }
    }
    effects.push(Effect::SetSkillCooldown {
        actor,
        skill,
        until: Tick(world.clock.0 + i64::from(rule.cooldown_ticks)),
    });
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    effects.push(Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    });
    effects
}

/// 读取 `agent` 当前某项资源的值——`resolve_use_skill` 的帮手，把
/// [`crate::skill::ResourceKind`] 到 `Agent` 具体字段的映射收敛在一处。
fn current_resource(agent: &ll_world::entity::Agent, kind: crate::skill::ResourceKind) -> i64 {
    match kind {
        crate::skill::ResourceKind::Mana => i64::from(agent.mana),
        crate::skill::ResourceKind::Stamina => i64::from(agent.stamina),
    }
}

/// 读取 `agent` 当前对某个开放注册标量池的「可用量」——
/// `resolve_use_skill` 门四的帮手，与 [`current_resource`] 是同一件事
/// 在开放资源池这条通道上的对应物,但多一步容量钳位：
/// `resource-pools-and-rest.md` 三节「上限变化时怎么办」一节裁定容量
/// 变化只在**读取**这一刻现场钳位，不主动改写存储值——
/// `usable = min(stored_current, effective_cap)`,不足则技能放不出来,
/// 这条判定因此不是恒真（容量降到低于已消耗量时,`usable` 会真的比
/// `stored_current` 小）。
fn resource_pool_usable(
    agent: &ll_world::entity::Agent,
    pool: ContentIndex,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> i64 {
    let stored = agent.resource_pools.get(&pool).copied().unwrap_or(0);
    let cap = effective_scalar_capacity(agent.race, agent.level, pool, race_traits, traits);
    i64::from(stored).min(i64::from(cap)).max(0)
}

/// 门四/效果产出共用的帮手：从 `min_tier` 起往上找第一个「上限 >
/// 已消耗数」的档位——`resource-pools-and-rest.md` 二节"从最低阶开始
/// 取"的引擎规则,见 [`crate::skill::ResourceCost::SlotTier`] 文档。
/// 找不到时返回 `None`（技能静默不产出效果，与门四其余判定同一条
/// 纪律）。**单向可兑换天然成立**：查询从 `min_tier` 起，从不往下看
/// 低于 `min_tier` 的档位——三环法术（`min_tier = 3`）永远不会被路由
/// 去占用一环位的空位，不需要任何额外的"不许往下兑换"检查,这条限制
/// 就写在循环的起点里。
///
/// # 上界为什么是 `u8::MAX`，不是查询 `ResourcePoolShape::TieredSlots`
/// 的 `tier_count`
///
/// 本函数不接收资源池目录参数——`resolve_use_skill` 因此不需要为了
/// 这一条路径多接一份 `pools: &dyn ResourcePoolCatalog`（既有调用点
/// `resolve_with_skills_traits_and_pools`/`resolve_with_skills_and_traits`
/// 的层次已经足够深，见 `resolve_with_skills_and_traits` 文档）。任何
/// 未被声明容量的档位，`effective_slot_tier_capacity` 天然算出零,不会
/// 被误判为"可用"——循环最多跑 255 次,与 `resolve_use_skill` 门一
/// 文档「性能」一节同一条判断：不是逐 tick 热路径,一场战斗一个实体
/// 一回合最多用一次技能，这个量级的循环开销可以忽略不计。
fn find_available_slot_tier(
    agent: &ll_world::entity::Agent,
    pool: ContentIndex,
    min_tier: u8,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Option<u8> {
    for tier in min_tier..=u8::MAX {
        let capacity =
            effective_slot_tier_capacity(agent.race, agent.level, pool, tier, race_traits, traits);
        let spent = agent.spent_slots.get(&(pool, tier)).copied().unwrap_or(0);
        if spent < capacity {
            return Some(tier);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use ll_core::torus::TorusSize;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
    use ll_world::zone::ZoneLayout;

    use super::*;

    /// 测试用区块布局：边长 64，单个区块——是噪声格点周期的整数倍，
    /// 满足 `WorldState::new` 的前置条件（与 `ll-sim`/`ll-world` 既有
    /// 测试同一常量），整个测试世界落在这一个区块内。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
    }

    /// 返回值附带 [`BaseTerrainIds`]：`terrain_ids` 与
    /// `world.terrain_table` 必须来自同一次 [`base_terrain_fixture`]
    /// 调用——`ContentIndex` 只在产出它的那个 `Interner` 里有意义
    /// （`ll_core::ident` 模块文档），两次独立调用各自的索引分配虽然
    /// 因为固定顺序而恰好数值相同，但把它们当成「必须配对」处理更不
    /// 容易在将来注册顺序调整时踩坑。
    fn test_world() -> (WorldState, BaseTerrainIds) {
        let layout = test_layout();
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        let world = WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件");
        (world, terrain_ids)
    }

    /// 造一个占位实体，站在 `(5, 5)`，六项主属性取基准值，`current_space`
    /// 取地表（占位层属性索引——本文件的移动/攻击/开门测试不消费空间
    /// 层属性，见 `Space::surface` 文档）。
    fn spawn_agent(world: &mut WorldState) -> EntityId {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(5, 5);
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        })
    }

    /// 造一份「站在 `pos` 上」的地表空间——`current_space` 的
    /// `profile` 用一个占位 `ContentIndex`（本文件测试不消费空间层
    /// 属性），`zone` 由测试世界自身的区块布局推出。
    fn surface_space_at(world: &WorldState, pos: ll_core::torus::TorusPos) -> Space {
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        Space::surface(zone, ll_core::ident::ContentIndex::default())
    }

    /// 从 `(5, 5)` 向东（`dx = 1`）走一步的目的地，与 [`spawn_agent`]
    /// 的出生点配套——测试只需要一个已知、可控的目的地格。
    fn east_of_spawn(world: &WorldState) -> ll_core::torus::TorusPos {
        world.size.wrap(6, 5)
    }

    /// 造一个占位实体，站在 `(5, 5)`，除敏捷外六项主属性取基准值——
    /// 供敏捷相关测试指定一个非基准的敏捷值。
    fn spawn_agent_with_dexterity(world: &mut WorldState, dexterity: i32) -> EntityId {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(5, 5);
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats {
                dexterity,
                ..BaseStats::BASELINE
            },
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        })
    }

    #[test]
    fn 结算不修改世界() {
        // resolve 的签名只接受 &WorldState，编译期已经不允许它写世界；
        // 这条测试是这个保证的行为级回归——即使产出了效果，调用 resolve
        // 本身也绝不应改变世界的哈希（哈希已覆盖地形与实体状态，见
        // WorldState::hash 文档）。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        world
            .terrain
            .set_terrain(east_of_spawn(&world), terrain_ids.grass);
        let intent = Intent::Move {
            actor,
            dir: Direction::East,
        };
        let hash_before = world.hash();

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(!effects.is_empty(), "本用例应产生效果，否则测不出意义");
        assert_eq!(world.hash(), hash_before);
    }

    #[test]
    fn 移动到不可通行地形不产生移动效果() {
        // 项目所有者决策：撞墙仍要消耗时间（见 resolve_move 文档「目的地
        // 完全不可通行」一节），本用例只锁定「不产生 MoveTo」这一件事
        // ——时间是否推进、位置是否不变分别由下面两条测试独立断言。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        world
            .terrain
            .set_terrain(east_of_spawn(&world), terrain_ids.wall_stone);
        let intent = Intent::Move {
            actor,
            dir: Direction::East,
        };

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::MoveTo { .. }))
        );
    }

    #[test]
    fn 撞墙仍产生排期效果推进行动时间() {
        // 撞墙本身是一次真实的行动尝试（伸手推了一下、发现推不开），
        // 应当消耗时间——这是本次缺陷交接记录明确记录的项目所有者决策。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        world
            .terrain
            .set_terrain(east_of_spawn(&world), terrain_ids.wall_stone);
        let intent = Intent::Move {
            actor,
            dir: Direction::East,
        };

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(
            effects.iter().any(
                |effect| matches!(effect, Effect::ScheduleNext { actor: a, .. } if *a == actor)
            )
        );
    }

    #[test]
    fn 撞墙结算后应用效果位置不变() {
        // 与上一条互补：确认「消耗时间」没有连带着悄悄移动位置——两件
        // 事分别断言,不合并进同一个测试。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        world
            .terrain
            .set_terrain(east_of_spawn(&world), terrain_ids.wall_stone);
        let pos_before = world
            .actors
            .get(actor)
            .expect("刚 spawn 的实体必然存在")
            .pos;
        let intent = Intent::Move {
            actor,
            dir: Direction::East,
        };

        // Act
        let effects = resolve(&world, &intent);
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert
        let pos_after = world.actors.get(actor).expect("apply 不会移除实体").pos;
        assert_eq!(pos_after, pos_before);
    }

    #[test]
    fn 移动到浅水的行动耗时高于草地() {
        // Arrange
        let (mut grass_world, grass_ids) = test_world();
        let grass_actor = spawn_agent(&mut grass_world);
        grass_world
            .terrain
            .set_terrain(east_of_spawn(&grass_world), grass_ids.grass);

        let (mut water_world, water_ids) = test_world();
        let water_actor = spawn_agent(&mut water_world);
        water_world
            .terrain
            .set_terrain(east_of_spawn(&water_world), water_ids.shallow_water);

        // Act
        let grass_effects = resolve(
            &grass_world,
            &Intent::Move {
                actor: grass_actor,
                dir: Direction::East,
            },
        );
        let water_effects = resolve(
            &water_world,
            &Intent::Move {
                actor: water_actor,
                dir: Direction::East,
            },
        );

        // Assert
        let grass_cost = schedule_next_at(&grass_effects).0 - grass_world.clock.0;
        let water_cost = schedule_next_at(&water_effects).0 - water_world.clock.0;
        assert!(water_cost > grass_cost);
    }

    #[test]
    fn 攻击关着的门产生开门效果而非伤害效果() {
        // 「攻击关着的门」在这套设计里就是朝它的方向移动一步——门不是
        // 实体，Intent::Attack 的 target 必须是 EntityId，指向不了一格
        // 地形；玩家的「攻击」输入落到 resolve 这里，撞见关着的门时
        // 被派生成开门而不是造成伤害。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        world
            .terrain
            .set_terrain(east_of_spawn(&world), terrain_ids.door_closed);
        let intent = Intent::Move {
            actor,
            dir: Direction::East,
        };

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetTerrain { kind, .. } if *kind == terrain_ids.door_open
        )));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Damage { .. }))
        );
    }

    #[test]
    fn 撞入即开不是只对关着的门生效的特判() {
        // 这是本次迁移撞见并修掉的 API 洞的直接验收：opens_into 是
        // 任意地形都能声明的属性，不是只有 lostland:door_closed 才有
        // 的硬编码特权——一个假想 mod 注册的「活板门」同样应该走这条
        // 通用路径，而不需要去改 ll-sim 的源码。
        //
        // 用同一个 Interner 先注册本体 17 个地形、再追加两个自定义地形
        // ——不能各自新起一个 Interner：ContentIndex 只在产出它的那个
        // Interner 里有意义，另起一个会与本体的 0..17 撞号。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let (terrain_ids, mut table) =
            ll_world::terrain::materialize_base_terrain(&mut |id| interner.intern(id))
                .expect("本体地形声明表内部一致");
        let hatch_open = ll_world::terrain::TerrainKind::from_index(
            interner
                .intern(ll_core::ident::NamespacedId::parse("yourmod:hatch_open").expect("合法")),
        );
        let hatch_closed = ll_world::terrain::TerrainKind::from_index(
            interner
                .intern(ll_core::ident::NamespacedId::parse("yourmod:hatch_closed").expect("合法")),
        );
        table
            .define(
                hatch_open.index(),
                ll_world::terrain::TerrainAttrs {
                    blocks_sight: false,
                    blocks_move: false,
                    move_cost: 100,
                    opens_into: None,
                },
            )
            .expect("测试声明内部自洽");
        table
            .define(
                hatch_closed.index(),
                ll_world::terrain::TerrainAttrs {
                    blocks_sight: false,
                    blocks_move: true,
                    move_cost: u32::MAX,
                    opens_into: Some(hatch_open),
                },
            )
            .expect("测试声明内部自洽");

        let layout = test_layout();
        let spawn = layout.tile_size().wrap(0, 0);
        let mut world = WorldState::new(layout, &GenParams::default(), &terrain_ids, table, spawn)
            .expect("测试布局满足全部构造前置条件");
        world
            .terrain
            .set_terrain(east_of_spawn(&world), hatch_closed);
        let actor = spawn_agent(&mut world);

        // Act
        let effects = resolve(
            &world,
            &Intent::Move {
                actor,
                dir: Direction::East,
            },
        );

        // Assert
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetTerrain { kind, .. } if *kind == hatch_open
        )));
    }

    #[test]
    fn 对着不能开的地形使用开门意图仍消耗行动时间() {
        // 与 resolve_move 撞墙同一条决策：`Intent::OpenDoor` 对着一格
        // 并非「撞入即开」的地形（这里直接用普通草地）时，仍是一次
        // 「查得到目标、确认这个动作在此处不成立」的确定结果，应当
        // 消耗时间——见 resolve_open_door 文档。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        let target = east_of_spawn(&world);
        world.terrain.set_terrain(target, terrain_ids.grass);
        let intent = Intent::OpenDoor {
            actor,
            pos: (target.x(), target.y()),
        };

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(
            effects.iter().any(
                |effect| matches!(effect, Effect::ScheduleNext { actor: a, .. } if *a == actor)
            )
        );
    }

    #[test]
    fn 对着不能开的地形使用开门意图不改写地形() {
        // 与上一条互补：确认「消耗时间」没有连带着悄悄把目标地形改写成
        // 别的东西——两件事分别断言，不合并进同一个测试。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        let target = east_of_spawn(&world);
        world.terrain.set_terrain(target, terrain_ids.grass);
        let intent = Intent::OpenDoor {
            actor,
            pos: (target.x(), target.y()),
        };

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::SetTerrain { .. }))
        );
    }

    #[test]
    fn 敏捷更高的角色等待耗时更短() {
        // 这是 P3 验收 demo（Task 9）排查出的阻断性缺陷的回归测试：
        // 修复前 resolve 的四个分支全部直接传常量 BASELINE_EFFECTIVE_SPEED，
        // 不读 agent.stats.dexterity，敏捷高低对行动耗时毫无影响——时间轴
        // 调度器「敏捷高者能在同一窗口内多行动几次」这条核心手感因此在
        // 结算层根本不成立。
        // Arrange
        let (mut slow_world, _slow_ids) = test_world();
        let slow_actor = spawn_agent_with_dexterity(&mut slow_world, 5);
        let (mut fast_world, _fast_ids) = test_world();
        let fast_actor = spawn_agent_with_dexterity(&mut fast_world, 40);

        // Act
        let slow_effects = resolve(&slow_world, &Intent::Wait { actor: slow_actor });
        let fast_effects = resolve(&fast_world, &Intent::Wait { actor: fast_actor });

        // Assert
        let slow_cost = schedule_next_at(&slow_effects).0 - slow_world.clock.0;
        let fast_cost = schedule_next_at(&fast_effects).0 - fast_world.clock.0;
        assert!(fast_cost < slow_cost);
    }

    /// 在 `world` 里插入一个锚定在 `anchor` 的 `Interior`，带一层 0 层
    /// 楼层（4x4 石地板）——task 12 进出空间测试的公共夹具。
    fn insert_interior_at(
        world: &mut WorldState,
        anchor: ll_core::torus::TorusPos,
    ) -> ll_core::ident::WorldId {
        let mut counter = 0u32;
        let mut interner = ll_core::ident::Interner::new();
        let profile = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("字面量恒合法"));
        let id = ll_core::ident::WorldId::next(&mut counter);
        let mut interior = ll_world::interior::Interior::new(id, anchor, profile);
        let (ids, _table) = base_terrain_fixture();
        let size = ll_core::bounded::BoundedSize::new(4, 4).expect("4x4 是合法尺寸");
        interior.set_floor(
            0,
            ll_world::bounded_grid::BoundedGrid::new(size, ids.floor_stone),
        );
        world.insert_interior(interior);
        id
    }

    #[test]
    fn 站在有interior入口的格子上触发进入意图产出changespace效果() {
        // Arrange
        let (mut world, _ids) = test_world();
        let actor = spawn_agent(&mut world);
        let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
        let interior_id = insert_interior_at(&mut world, anchor);

        // Act
        let effects = resolve(
            &world,
            &Intent::EnterSpace {
                actor,
                target: interior_id,
            },
        );

        // Assert
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::ChangeSpace { space: Space::Interior { id, .. }, .. } if *id == interior_id
        )));
    }

    #[test]
    fn 站在没有interior入口的格子上触发进入意图不产生任何空间切换() {
        // Arrange：Interior 锚定在离玩家很远的一格,玩家当前所在格没有
        // 任何入口。
        let (mut world, _ids) = test_world();
        let actor = spawn_agent(&mut world);
        let far_anchor = world.size.wrap(40, 40);
        let interior_id = insert_interior_at(&mut world, far_anchor);

        // Act
        let effects = resolve(
            &world,
            &Intent::EnterSpace {
                actor,
                target: interior_id,
            },
        );

        // Assert
        assert!(effects.is_empty());
    }

    #[test]
    fn 进入interior后agent的pos不变只有当前空间变化() {
        // Arrange
        let (mut world, _ids) = test_world();
        let actor = spawn_agent(&mut world);
        let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
        let interior_id = insert_interior_at(&mut world, anchor);
        let effects = resolve(
            &world,
            &Intent::EnterSpace {
                actor,
                target: interior_id,
            },
        );

        // Act
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert
        let agent = world.actors.get(actor).expect("刚生成必然存在");
        assert_eq!(agent.pos, anchor);
        assert!(matches!(agent.current_space, Space::Interior { id, .. } if id == interior_id));
    }

    #[test]
    fn 退出interior后agent的pos恢复为interior的锚点() {
        // Arrange：先进入,把玩家「弄脏」成一个非锚点位置不需要——本批次
        // Interior 内部移动本就静默无效（见模块文档），这里直接验证
        // 退出后 pos 仍精确等于锚点,而不是随便一个值。
        let (mut world, _ids) = test_world();
        let actor = spawn_agent(&mut world);
        let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
        let interior_id = insert_interior_at(&mut world, anchor);
        for effect in &resolve(
            &world,
            &Intent::EnterSpace {
                actor,
                target: interior_id,
            },
        ) {
            crate::apply::apply(&mut world, effect);
        }

        // Act
        let exit_effects = resolve(&world, &Intent::ExitSpace { actor });
        for effect in &exit_effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert
        let agent = world.actors.get(actor).expect("刚生成必然存在");
        assert_eq!(agent.pos, anchor);
        assert!(matches!(agent.current_space, Space::Surface { .. }));
    }

    #[test]
    fn worldstate的hash纳入current_space的变化() {
        // Arrange
        let (mut world, _ids) = test_world();
        let actor = spawn_agent(&mut world);
        let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
        let interior_id = insert_interior_at(&mut world, anchor);
        let hash_before = world.hash();
        let effects = resolve(
            &world,
            &Intent::EnterSpace {
                actor,
                target: interior_id,
            },
        );

        // Act
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert：只有 current_space 变了（pos/health/wallet/
        // next_action_at 均未受这条 Intent 影响),哈希仍必须不同——否则
        // 说明 hash() 没有真正混入 current_space。
        assert_ne!(world.hash(), hash_before);
    }

    /// 从一批效果里取出 [`Effect::ScheduleNext`] 的排期时刻——上面几条
    /// 移动耗时测试都要读这个字段，抽成小工具避免重复的
    /// `iter().find_map(...)`。
    fn schedule_next_at(effects: &[Effect]) -> Tick {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::ScheduleNext { at, .. } => Some(*at),
                _ => None,
            })
            .expect("本文件的移动类测试用例都应产生 ScheduleNext 效果")
    }

    /// 造一个已具名（`remembered_id` 已赋值）的占位实体，站在 `pos`,
    /// 生命值可由调用方指定——供击杀历史记录的端到端测试构造"低血量
    /// 但已经被记住"的目标。
    fn spawn_named_agent(
        world: &mut WorldState,
        pos: ll_core::torus::TorusPos,
        health: i32,
    ) -> EntityId {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));
        let mut world_id_counter = 0u32;
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: Some(ll_core::ident::WorldId::next(&mut world_id_counter)),
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        })
    }

    #[test]
    fn 近战攻击致死已具名目标后历史事件记录着近战死因() {
        // 端到端验证（不是结构往返）：从 Intent::Attack 造成致死伤害
        // 开始，一路断言到 apply 真的把这条击杀写进
        // world.history——KillCause 必须精确到「近战」这一级，而不是
        // 只有一句"A 杀了 B"。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        // 生命值 1：BASELINE 力量算出的攻击力必然大于 1（见
        // combat::damage_after_defense 的单元测试），一击必死。
        let victim = spawn_named_agent(&mut world, victim_pos, 1);

        // Act
        let effects = resolve(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
        );
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert：目标真的被销毁（不是只造出了记录、目标却还活着）。
        assert!(world.actors.get(victim).is_none());
        // 历史事件真的被写入了,不是只在效果列表里飘过。
        assert_eq!(world.history.len(), 1);
        let ll_world::history::HistoricalEventKind::Kill(record) = &world.history[0].kind;
        // 致死手段精确到「近战」——不是笼统的"被杀"。
        assert!(matches!(
            record.cause,
            ll_world::history::KillCause::Melee { weapon: None }
        ));
        // 攻击者没有被记住（remembered_id 为 None），记录里的
        // killer 因此如实为 None——不是伪造出一个不存在的具名击杀者。
        assert_eq!(record.killer, None);
        // 致命一击确实造成了伤害、结算后生命值不高于零。
        assert!(record.killing_blow.damage > 0);
        assert!(record.killing_blow.remaining_health <= 0);
    }

    /// 恒对任意生物种类返回同一个固定经验值的测试用经验目录——真实
    /// 实现（`ll-mod` 的 `RaceTable::xp_reward`）会按种类区分，这里的
    /// 测试只关心「经验真的被授予了」这条链路本身是否接通，不关心具体
    /// 种族与经验值的对应关系，用固定值足够、也更不脆弱（不依赖攻击者
    /// /受害者各自 `Interner` 分配出的具体 `ContentIndex` 数值）。
    struct FixedReward(i64);

    impl crate::experience::ExperienceCatalog for FixedReward {
        fn xp_reward_for(&self, _kind: ll_core::ident::ContentIndex) -> i64 {
            self.0
        }
    }

    #[test]
    fn 完整管线结算一次致死击杀后击杀者的经验真的增加() {
        // 端到端验证：从 Intent::Attack 造成致死伤害开始，走
        // resolve_with_skills_quests_and_experience（真实的四层入口，
        // 不是直接构造 Effect::GrantExperience 抄近路）+
        // apply_with_xp_curves，断言击杀者身上的 experience 字段确实
        // 变化了——这是设计文档五节「Effect::Kill 是正确的挂载点」
        // 落地后必须成立的最基本一条链路。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        // 生命值 1：一击必死，见「近战攻击致死……」测试同一注释。
        let victim = spawn_named_agent(&mut world, victim_pos, 1);
        let reward_amount = 30; // 小于 Agent::STARTING_XP_TO_NEXT_LEVEL（100），这条测试不涉及升级。

        // Act
        let effects = resolve_with_skills_quests_and_experience(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
            &NoSkills,
            &NoQuests,
            &FixedReward(reward_amount),
        );
        for effect in &effects {
            crate::apply::apply_with_xp_curves(
                &mut world,
                effect,
                &crate::xp_curve::FlatXpCurve::DEFAULT,
            );
        }

        // Assert：击杀者的经验值真的从零涨到了这次击杀应得的数额。
        assert_eq!(
            world
                .actors
                .get(attacker)
                .expect("攻击者仍然存活")
                .experience,
            reward_amount
        );
    }

    #[test]
    fn 经验积累超过门槛时击杀者的等级真的提升且门槛真的重新求值() {
        // 端到端验证：这次击杀产出的经验足以跨过默认门槛
        // （Agent::STARTING_XP_TO_NEXT_LEVEL = 100），断言 apply 侧的
        // 升级循环真的把 level 加了一、真的用曲线目录重新算出了新的
        // xp_to_next_level（而不是原样保留旧值 100）——升级判定整段
        // 放进 apply 一次算完，见 apply::apply_with_xp_curves 文档。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        let victim = spawn_named_agent(&mut world, victim_pos, 1);
        let reward_amount = 150; // 150 > 100（默认门槛），恰好触发一次升级，剩余 50 点经验。
        // 升级后重算门槛用的曲线与 apply() 默认的保底曲线（100）取不同
        // 的固定值（250），这样"门槛真的被重新求值"这件事才能通过
        // "新值既不等于升级前的旧门槛，也不等于任何巧合相同的默认值"
        // 来验证，而不是巧合蒙对。
        let level_up_curve = crate::xp_curve::FlatXpCurve { amount: 250 };

        // Act
        let effects = resolve_with_skills_quests_and_experience(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
            &NoSkills,
            &NoQuests,
            &FixedReward(reward_amount),
        );
        for effect in &effects {
            crate::apply::apply_with_xp_curves(&mut world, effect, &level_up_curve);
        }

        // Assert：等级真的从 1 涨到了 2，新门槛真的等于曲线目录重新
        // 求值的结果（250），不是升级前的旧值（100）原样保留。
        let attacker_agent = world.actors.get(attacker).expect("攻击者仍然存活");
        assert_eq!(attacker_agent.level, Agent::STARTING_LEVEL + 1);
        assert_eq!(attacker_agent.xp_to_next_level, 250);
    }

    #[test]
    fn 攻击者力量的生效中临时修正会改变结算出的伤害() {
        // 端到端验证（不是结构往返）：给攻击者的 active_stat_modifiers
        // 塞一条真实的力量修正 → 走真实的 resolve(Intent::Attack) +
        // apply → 断言目标掉血量确实随之变化。这条链路此前断在
        // resolve_attack 只读裸 attacker.stats.strength，从不看
        // active_stat_modifiers——两端各自都有测试覆盖（ActiveStatModifier
        // 的序列化往返、Effect::ApplyStatModifier 的 apply 单测），却没
        // 有一条测试穿过中间那根线，见 resolve_attack 与
        // effective_attribute 的文档。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        // 生命值给够大的余量,这条测试只关心「伤害数值变了多少」，不
        // 关心目标是否被打死——致死路径已由上一条测试单独覆盖。
        let victim = spawn_named_agent(&mut world, victim_pos, 1_000);
        let mut interner = ll_core::ident::Interner::new();
        let source = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
        world
            .actors
            .get_mut(attacker)
            .expect("刚生成必然存在")
            .active_stat_modifiers
            .insert(
                AttributeKind::Strength,
                std::collections::BTreeMap::from([(
                    source,
                    ActiveStatModifier {
                        delta: 20,
                        expires_at: Tick(100),
                    },
                )]),
            );
        // 期望伤害直接复用 combat::damage_after_defense（该公式本身已
        // 有独立单测覆盖，这里只用它算出「修正后的力量」应得的伤害，
        // 不是重新验证公式本身）——BASELINE 力量为 10，加上本测试
        // 施加的 +20 修正，应得力量 30。
        let expected_damage =
            damage_after_defense(BaseStats::BASELINE.strength + 20, 0, Penetration::NONE);

        // Act
        let effects = resolve(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
        );
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert：目标生命值精确反映了「叠加修正后的力量」算出的伤害，
        // 不是裸力量值算出的那个（更低的）数字。
        let victim_after = world.actors.get(victim).expect("生命值远高于伤害,不会死亡");
        assert_eq!(victim_after.health, 1_000 - expected_damage);
    }

    #[test]
    fn 已过期的属性修正不再叠加到有效值() {
        // Arrange：到期时刻早于当前世界时钟——惰性到期判定要求这类
        // 条目在读取时被当作已失效处理,即使它仍然留在
        // active_stat_modifiers 里没被清理（见 ActiveStatModifier 文档
        // 「惰性到期判定」一节）。
        let mut interner = ll_core::ident::Interner::new();
        let source = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
        let modifiers = std::collections::BTreeMap::from([(
            AttributeKind::Strength,
            std::collections::BTreeMap::from([(
                source,
                ActiveStatModifier {
                    delta: 20,
                    expires_at: Tick(5),
                },
            )]),
        )]);

        // Act
        let effective = effective_attribute(10, AttributeKind::Strength, &modifiers, Tick(5));

        // Assert：世界时钟已达到 expires_at,回落到裸值,不叠加 delta。
        assert_eq!(effective, 10);
    }

    #[test]
    fn 不同来源的属性修正在生效值上求和而非互相覆盖() {
        // 规则①「不同效果能叠加」在 effective_attribute 这一层的直接
        // 验证：两个不同来源（source_a、source_b）各自给同一属性 +5、
        // +7，有效值必须是 base + 5 + 7，不是只看到其中一条。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let source_a = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
        let source_b = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:blessing").expect("合法标识符"));
        let modifiers = std::collections::BTreeMap::from([(
            AttributeKind::Strength,
            std::collections::BTreeMap::from([
                (
                    source_a,
                    ActiveStatModifier {
                        delta: 5,
                        expires_at: Tick(100),
                    },
                ),
                (
                    source_b,
                    ActiveStatModifier {
                        delta: 7,
                        expires_at: Tick(100),
                    },
                ),
            ]),
        )]);

        // Act
        let effective = effective_attribute(10, AttributeKind::Strength, &modifiers, Tick(0));

        // Assert：10（base） + 5 + 7 = 22，两条修正都参与了求和。
        assert_eq!(effective, 22);
    }

    #[test]
    fn 一条来源过期后另一条来源的修正仍然独立生效() {
        // 规则②③强调「各条修正各自到期」——这里验证的正是这一点：
        // source_a 已过期，source_b 未过期，聚合结果应只包含 source_b。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let source_a = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
        let source_b = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:blessing").expect("合法标识符"));
        let modifiers = std::collections::BTreeMap::from([(
            AttributeKind::Strength,
            std::collections::BTreeMap::from([
                (
                    source_a,
                    ActiveStatModifier {
                        delta: 5,
                        expires_at: Tick(10),
                    },
                ),
                (
                    source_b,
                    ActiveStatModifier {
                        delta: 7,
                        expires_at: Tick(100),
                    },
                ),
            ]),
        )]);

        // Act：世界时钟已经越过 source_a 的到期时刻，但仍早于 source_b。
        let effective = effective_attribute(10, AttributeKind::Strength, &modifiers, Tick(10));

        // Assert：只有 source_b 的 +7 参与求和，source_a 已被过滤。
        assert_eq!(effective, 17);
    }

    #[test]
    fn 未具名目标被击杀时不产生历史事件记录() {
        // 与上一条对照：victim 从未被"记住"（remembered_id 恒
        // None）——分级判据要求 victim 已具名才产出完整记录（见
        // append_kill_history 文档「触发判据」一节），这里验证「不产出
        // 完整记录」也是真实生效的分支，不是恰好每次都触发。决策一
        // 落地后，这类击杀改为产出聚合计数而不是"什么都不产生"——那条
        // 断言由下面 未具名目标被击杀时按生物类型归并计数加一 单独
        // 覆盖，这里只关注"没有完整记录"这一件事。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        let victim = world.actors.spawn(Agent {
            pos: victim_pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: 1,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: ContentIndex::default(),
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(&world, victim_pos),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        });

        // Act
        let effects = resolve(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
        );
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert：目标依旧真的死了，但没有产生历史事件——分级判据把
        // 「击杀发生」与「值不值得记录」分开，两者不能混为一谈。
        assert!(world.actors.get(victim).is_none());
        assert!(world.history.is_empty());
    }

    #[test]
    fn 未具名目标被击杀时按生物类型归并计数加一() {
        // 决策一端到端验证：杀死一个无名单位（remembered_id 恒
        // None）——从 Intent::Attack 一路到 apply,断言 world.kill_counts
        // 里对应 race 的计数恰好 +1,且没有产生完整历史事件（两件事
        // 同时成立,互不替代）。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        let mut interner = ll_core::ident::Interner::new();
        let goblin_race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));
        let victim = world.actors.spawn(Agent {
            pos: victim_pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: 1,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: goblin_race,
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(&world, victim_pos),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        });

        // Act
        let effects = resolve(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
        );
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert
        assert!(world.actors.get(victim).is_none());
        assert!(world.history.is_empty());
        assert_eq!(world.kill_counts.get(&goblin_race), Some(&1));
    }

    #[test]
    fn 具名目标被击杀时按生物类型归并计数加一() {
        // 与「未具名目标被击杀时按生物类型归并计数加一」对照,同时与
        // 「近战攻击致死已具名目标后历史事件记录着近战死因」互补——
        // 后者已经单独证明了具名死者仍会产出完整历史记录,本测试只
        // 补上另一半：项目所有者裁定否决了决策一原有的互斥设计（「一
        // 起计算,就是杀了 10 只」,见 append_kill_history 文档「决策二」
        // 一节）之后,具名死者的击杀现在也照常累加聚合计数,不再因为
        // 已经产出完整记录就被排除在计数之外。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        let victim = spawn_named_agent(&mut world, victim_pos, 1);
        let victim_race = world.actors.get(victim).expect("刚生成必然存在").race;

        // Act
        let effects = resolve(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
        );
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert
        assert_eq!(world.kill_counts.get(&victim_race), Some(&1));
    }

    /// 一个只认识固定种族索引的测试用天赋授予来源，供
    /// [`resource_pool_usable`] 的钳位测试使用——理由同本文件其余
    /// `Fake*` 测试替身。
    struct FixedRacePoolGrant {
        race: ContentIndex,
        trait_id: ContentIndex,
    }

    impl TraitGrantSource for FixedRacePoolGrant {
        fn granted_traits(&self, owner: ContentIndex) -> Vec<crate::traits::TraitGrant> {
            if owner == self.race {
                vec![crate::traits::TraitGrant {
                    trait_id: self.trait_id,
                    unlock_level: 1,
                }]
            } else {
                Vec::new()
            }
        }
    }

    /// 固定把 `trait_id` 映射到一条授予 `pool` 某个固定容量的
    /// `TraitRule`——供 [`resource_pool_usable`] 的钳位测试使用。
    struct FixedPoolCapacity {
        trait_id: ContentIndex,
        pool: ContentIndex,
        capacity: u32,
    }

    impl TraitCatalog for FixedPoolCapacity {
        fn trait_rule(&self, trait_id: ContentIndex) -> Option<crate::traits::TraitRule> {
            if trait_id != self.trait_id {
                return None;
            }
            Some(crate::traits::TraitRule {
                granted_skills: Vec::new(),
                granted_resource_pools: vec![crate::resource_pool::ResourcePoolGrant {
                    pool: self.pool,
                    capacity: crate::resource_pool::CapacityFormula::Fixed(self.capacity),
                }],
            })
        }
    }

    #[test]
    fn 容量从十降到五时存储值八读出来被钳位为五而存储本身不改写() {
        // 直接验收「容量变化时读时钳位,不主动改写存储值」
        // （`resource-pools-and-rest.md` 三节）：先构造一个天赋只授予
        // 5 点容量（模拟"容量已经从 10 降到 5"这一刻），但
        // agent.resource_pools 里存储的当前值仍是掉容量之前留下的 8——
        // usable 必须被钳位为 5,而 agent.resource_pools 这份存储数据
        // 本身完全不受这次读取影响。
        // Arrange
        let (mut world, _ids) = test_world();
        let actor = spawn_agent(&mut world);
        let mut interner = ll_core::ident::Interner::new();
        let race = world.actors.get(actor).expect("刚生成必然存在").race;
        let trait_id = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:diminished_sorcery").unwrap());
        let pool = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:sorcery_points").unwrap());
        if let Some(agent) = world.actors.get_mut(actor) {
            agent.resource_pools.insert(pool, 8);
        }
        let race_traits = FixedRacePoolGrant { race, trait_id };
        let traits = FixedPoolCapacity {
            trait_id,
            pool,
            capacity: 5,
        };

        // Act
        let agent = world.actors.get(actor).expect("刚生成必然存在");
        let usable = resource_pool_usable(agent, pool, &race_traits, &traits);

        // Assert：读出来的可用量被钳位为容量（5），不是原始存储值（8）。
        assert_eq!(usable, 5);
    }

    #[test]
    fn 容量钳位不改写存储值本身() {
        // 与上一条测试同一份构造,断言的对象换成「存储值」而不是
        // 「读出来的可用量」——钳位只发生在读取这一刻,agent.resource_pools
        // 里的原始 8 必须原封不动,不会被这次查询悄悄砍成 5。
        // Arrange
        let (mut world, _ids) = test_world();
        let actor = spawn_agent(&mut world);
        let mut interner = ll_core::ident::Interner::new();
        let race = world.actors.get(actor).expect("刚生成必然存在").race;
        let trait_id = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:diminished_sorcery").unwrap());
        let pool = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:sorcery_points").unwrap());
        if let Some(agent) = world.actors.get_mut(actor) {
            agent.resource_pools.insert(pool, 8);
        }
        let race_traits = FixedRacePoolGrant { race, trait_id };
        let traits = FixedPoolCapacity {
            trait_id,
            pool,
            capacity: 5,
        };

        // Act：查询一次可用量（钳位只应该发生在这次读取的返回值上）。
        let agent = world.actors.get(actor).expect("刚生成必然存在");
        let _ = resource_pool_usable(agent, pool, &race_traits, &traits);

        // Assert：存储值本身仍然是 8，没有被这次读取改写。
        assert_eq!(
            world.actors.get(actor).unwrap().resource_pools.get(&pool),
            Some(&8)
        );
    }
}
