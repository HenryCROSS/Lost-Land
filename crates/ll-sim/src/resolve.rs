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
//! 那条判定与打靶规则**现在补上了，但仍然不在本文件**：它住在
//! `crate::turn` 的 `route_move_into_occupant`——目的地站着别人时，按
//! [`crate::ai_query::declared_hostile`] 改判成 [`Intent::Attack`]
//! （敌对）或 [`Intent::Swap`]（非敌对，所有者裁定「非敌对就互换位置」）。
//! 分层没有变：**本文件仍然不从 `Intent::Move` 派生任何针对实体的
//! 动作**，`resolve_move` 一个字都没改；它只是多了一条结算已经路由好的
//! [`Intent::Swap`] 的函数（[`resolve_swap`]），与 `resolve_attack` 是
//! 同一个位置的两个兄弟。
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

use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_world::state::WorldState;

use crate::catalogs::ResolveCatalogs;
use crate::craft::{NoRecipes, RecipeCatalog};
use crate::damage_category::{DamageCategoryCatalog, NoDamageCategories};
use crate::dialogue::{ContentIdLookup, DialogueCatalog, NoContentIds, NoDialogues};
use crate::effect::Effect;
use crate::experience::{ExperienceCatalog, NoExperience};
use crate::exposure::AmbientSource;
use crate::formula::{DamageFormulaCatalog, NoFormulas};
use crate::intent::Intent;
use crate::item::{ItemCatalog, NoItems};
use crate::quest::{NoQuests, QuestCatalog};
use crate::resource_pool::{NoResourcePools, ResourcePoolCatalog};
use crate::skill::{NoSkills, SkillCatalog};
use crate::skill_overview::SkillTreeCatalog;
use crate::subclass::{NoSubclassUnlocks, SubclassUnlockCatalog};
use crate::traits::{NO_TRAIT_GRANTS, NoTraitGrants, NoTraits, TraitCatalog, TraitGrantSource};

// 按意图族拆开的九个子模块。分派表（`resolve_dispatch`）仍在本文件，
// 每加一族新意图 = 加一个模块 + 在分派表上加一条 arm。
mod combat;
mod crafting;
mod dialogue;
mod equipment;
mod inventory;
mod movement;
mod portal;
mod progression;
mod stats;
mod upkeep;

// 搬出去的项在这里重新引进本模块的作用域：对外的公开路径
// （`ll_sim::resolve::derive_stats` 等）与 `#[cfg(test)] use super::*`
// 因此一个字都不用改。
use self::dialogue::resolve_dialogue_choose;
pub(crate) use self::movement::step_destination;
// `occupant_at` 从 `pub(crate)` 开成 `pub`（对话批次 2）：`ll-game` 的
// 交互列表要问「这一格上站着谁」，而那正是本函数文档
// 「为什么必须只有这一份实现」里已经论证过的同一个问题。在输入层另写
// 一份查找，那条平局打破规则（同一格站着多于一个单位时取谁）会各自
// 漂移——正是 ADR 0021 点名要拦的形状。
pub use self::movement::occupant_at;
// 关门那两道前置的公开判据——`ll-game` 的输入层要在提交意图之前问同一
// 个问题，见 `portal::door_close_blocker` 文档「一份判据，两个调用点」。
pub use self::portal::{DoorCloseBlocker, door_close_blocker};
pub use self::stats::{DerivedStats, derive_stats, derive_stats_at};
// 只有断言用得到的项：`resolve_tests.rs` 里的 `use super::*` 靠这一行看见它，
// 因此那 45 条断言一个字都不用改。非测试构建下它不存在，也就不会有未用导入。
use self::combat::{
    append_corpse_drop, append_kill_experience, append_kill_history, append_quest_kill_progress,
    resolve_attack,
};
use self::crafting::{resolve_craft, resolve_experiment, resolve_identify, resolve_read};
use self::equipment::{resolve_equip, resolve_unequip, resolve_use_item};
use self::inventory::{
    resolve_drop, resolve_inspect, resolve_loot, resolve_pick_up, resolve_place,
};
use self::movement::{resolve_move, resolve_swap, resolve_toggle_stealth};
use self::portal::{
    resolve_close_door, resolve_enter_space, resolve_exit_space, resolve_open_door,
};
#[cfg(test)]
use self::progression::resource_pool_usable;
use self::progression::{
    append_craft_progress, resolve_abandon_subclass, resolve_allocate_attribute_point,
    resolve_learn_skill, resolve_resource_pool_regen, resolve_use_skill,
};
use self::upkeep::{resolve_rest, resolve_wait};

/// 非位移动作（等待、攻击、开门）的基础代价，与平地移动同一基准
/// （草地的 `move_cost` 恰为这个值）——本批次没有武器速度、技能读条
/// 之类会让这些动作耗时不同于「一次基准行动」的系统，统一按这个基准
/// 计费，接入那些系统时按动作类型分别替换即可。
const BASE_ACTION_COST: u32 = 100;

/// 潜行时移动开销的千分比倍率（潜行与盗贼被动批次）——`2000` = 两倍。
///
/// # 为什么是两倍，为什么用千分比整数
///
/// 千分比整数：ADR 0020 浮点分区，判定/系数一律走乙区的千分比整数，
/// 与 [`crate::combat::CRIT_DAMAGE_MULTIPLIER_PERMILLE`] 同一套既有
/// 惯例，不引入浮点。
///
/// 本常量属于「按比例缩放的环境量」那一档，**刻意不随规则修正一起改成
/// 整数点数**（加值类型批次）——理由见 `crate::rule_modifier` 模块文档
/// 「为什么跨类型是相加，而不是相乘」一节末尾的分界线：缩放量的基数
/// 本身在变，改成固定加减会在极值处结构性坏掉。
///
/// 两倍：潜行必须有一个**玩家能感觉到**的代价，否则「一直开着潜行」
/// 是严格占优策略，那个「可切换」的状态就退化成一次性开关、不再是一个
/// 需要权衡的选择（所有者裁定「潜行需要时可切换状态的」——「需要时」
/// 三个字预设了存在不需要的时候）。取两倍而不是 1.5 倍/3 倍的理由是
/// 它同时满足三条：**整除**（`move_cost` 是整数，两倍不引入任何舍入
/// 讨论，而 1.5 倍会）、与传统 roguelike「潜行约等于半速」的手感一致、
/// 且在回合制里读数直观（潜行走一格 = 别人走两格的时间，敌人多一次
/// 行动机会）。这个数字本身没有更深的推导，是一次拍板——它落在
/// `ll-sim` 而不是内容表，与 `BASE_ACTION_COST` 同一条既有边界
/// （`ll_world::state::WorldState` 文档「`BASE_ACTION_COST` 这类规则
/// 常量不进 `WorldState`」）：本批次没有任何 mod 要按内容调它的真实
/// 需求，提前开放注册通道是 YAGNI。
const STEALTH_MOVE_COST_PERMILLE: u32 = 2000;

/// 攻击方每打出一下近战攻击，自己主手那件**带 `on-use` 标签、且带
/// 耐久**的武器损失的耐久点数——「使用」这条通道在近战攻击上的落点，
/// 见 [`resolve_attack`] 文档「耐久消耗：两条通道，判据是标签」一节。
const WEAPON_DURABILITY_LOSS_PER_ATTACK: i32 = 1;

/// 防御方每挨一下近战攻击，自己**每一件**带 `on-hit` 标签、且带耐久的
/// 已装备物品（护甲/衣物）损失的耐久点数——「挨打」这条通道的落点，
/// 见 [`resolve_attack`] 文档「耐久消耗：两条通道，判据是标签」一节。
///
/// # 为什么是固定值，不随伤害缩放
///
/// 与 [`WEAPON_DURABILITY_LOSS_PER_ATTACK`] 同一个数、同一条理由：
/// 玩家能心算的规则才是能被规划的规则（「这件甲还能挨 40 下」比
/// 「这件甲还能扛 800 点伤害、但要先知道对面攻击力」直观得多）。
/// 随伤害缩放还会引出一条没人要求的耦合——护甲寿命被攻击方的力量
/// 决定，一次高伤害就能报废整套装备，而玩家对此毫无决策空间
/// （YAGNI：没有真实需求驱动这个平衡旋钮）。ADR 0020 乙区的整数
/// 算术也因此保持平凡，不需要任何千分比换算。
///
/// 与 `WEAPON_DURABILITY_LOSS_PER_ATTACK` **刻意分成两个常量而不是
/// 复用同一个**：它们服务两条不同的规则（使用 / 挨打），今天数值相同
/// 是巧合不是约束，将来任一条要单独调整时不该被迫牵动另一条。**一件
/// 两条标签都带的东西（盾）在同一次交换里可以两条都吃到**，那正是
/// 预期行为，见 `resolve_attack` 文档「两条通道现在可以重叠」一节。
const ARMOR_DURABILITY_LOSS_PER_HIT: i32 = 1;

/// 每完成一次制作，制作者身上那件被配方点名的工具（若带耐久）损失的
/// 耐久点数——见 [`resolve_craft`] 文档「工具磨损」一节。
///
/// 取值与 [`WEAPON_DURABILITY_LOSS_PER_ATTACK`] 相同、理由相同（一次
/// 制作 = 一次普通行动 = 一点磨损，与「一次攻击 = 一点磨损」同一套
/// 直觉），同样刻意是独立常量而非复用。
const TOOL_DURABILITY_LOSS_PER_CRAFT: i32 = 1;

/// 基准有效敏捷，对应 `BaseStats::BASELINE` 的敏捷值（10，调整值为零）。
///
/// 真正的「有效敏捷」需要 [`derive_stats`]（装备、状态效果、负重的
/// 综合结果）驱动，但那是衍生属性，规则上必须是纯函数且不进存档（见
/// `knowledge/design/attribute-system.md` 「七、衍生属性绝不进存档」）。
/// [`derive_stats`] 本身已经在 P6 第四批落地（基础属性 + 状态效果 +
/// 装备），但**移动速度本批次仍未接上它**——这不是遗漏，是刻意划定的
/// 范围边界：`derive_stats(...).attribute(Dexterity)` 现在确实能算出
/// 「叠加状态效果/装备加成后的敏捷」，但把它接进移动速度公式需要先
/// 决定"跑腿类装备"要不要提供敏捷加成这类内容设计问题，本批次任务书
/// 只要求接通战斗（`resolve_attack` 的攻防两端），未把移动速度列进
/// 范围（见项目任务书「本批次范围」一节）。[`effective_speed_from_dexterity`](self::stats::effective_speed_from_dexterity)
/// 因此继续吃裸 `agent.stats.dexterity`，接上 `derive_stats` 是留给
/// 未来批次的工作，届时把这个常量与本函数体一并替换即可，调用点不变。
const BASELINE_EFFECTIVE_SPEED: u32 = 1000;

/// `BaseStats::BASELINE` 的敏捷值——[`effective_speed_from_dexterity`](self::stats::effective_speed_from_dexterity)
/// 的线性映射以它为基准点：敏捷恰为这个值时，有效速度恰为
/// [`BASELINE_EFFECTIVE_SPEED`]。
const BASELINE_DEXTERITY: i64 = 10;

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
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        traits,
        &NoResourcePools,
        &NoItems,
        &NoFormulas,
        &NoDamageCategories,
        &NoRecipes,
        AmbientSource::NONE,
        &NoExperience,
        &NoSkills,
        &NoSubclassUnlocks,
        &NoDialogues,
        &NoContentIds,
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
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        traits,
        pools,
        &NoItems,
        &NoFormulas,
        &NoDamageCategories,
        &NoRecipes,
        AmbientSource::NONE,
        &NoExperience,
        &NoSkills,
        &NoSubclassUnlocks,
        &NoDialogues,
        &NoContentIds,
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
/// [`Intent::Drop`] 从家具层批次起也消费 `items`——不是为了堆叠上限
/// （丢弃仍然不查它），是为了问「这件东西是不是家具」，见
/// [`resolve_drop`] 文档。传 [`NoItems`] 时它恒答「不是」，放置前置
/// 因此整条不生效，与家具层落地之前逐位等价。
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
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        traits,
        pools,
        items,
        &NoFormulas,
        &NoDamageCategories,
        &NoRecipes,
        AmbientSource::NONE,
        &NoExperience,
        &NoSkills,
        &NoSubclassUnlocks,
        &NoDialogues,
        &NoContentIds,
    )
}

/// [`resolve`] 的最完整入口：在
/// [`resolve_with_skills_traits_pools_and_items`] 之上再额外接收一份
/// 伤害公式目录，用于结算 [`Intent::Attack`] 时按武器显式声明的公式
/// （或没有声明时的全局默认公式）算出攻击力数值（伤害公式引擎批次
/// 新增，见 [`resolve_attack`] 文档「伤害公式接线」一节）。
///
/// 七层入口而不是给某个既有入口加参数，理由同 [`resolve_with_skills`]
/// 文档：不强迫仓库里已有的全部调用点都多传一份公式目录——传
/// [`NoFormulas`] 与"不传"在行为上完全等价（两者都让
/// `resolve_attack` 使用同一条全局默认公式，逐行复现接入公式引擎之前
/// 的既有行为，见 `crate::formula` 模块文档「公式只算『攻击力』」
/// 一节与本模块「行为等价」测试），本函数只服务真正想让武器显式声明
/// 的公式生效的调用方（`ll_mod::formula::RegistryFormulas` 现在就是
/// 这样的真实实现）。
#[allow(clippy::too_many_arguments)]
pub fn resolve_with_skills_traits_pools_items_and_formulas(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
    items: &dyn ItemCatalog,
    formulas: &dyn DamageFormulaCatalog,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        skills,
        &NoQuests,
        race_traits,
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        traits,
        pools,
        items,
        formulas,
        &NoDamageCategories,
        &NoRecipes,
        AmbientSource::NONE,
        &NoExperience,
        &NoSkills,
        &NoSubclassUnlocks,
        &NoDialogues,
        &NoContentIds,
    )
}

/// [`resolve`] 的最完整入口：在
/// [`resolve_with_skills_traits_pools_items_and_formulas`] 之上再额外
/// 接收一份伤害类别目录，用于结算 [`Intent::Attack`] 时查这一下攻击
/// 没有显式声明伤害类别时该用哪个默认类别（伤害类别/抗性接线批次
/// 新增，见 [`resolve_attack`] 文档「抗性接线」一节）。
///
/// 八层入口而不是给某个既有入口加参数，理由同 [`resolve_with_skills`]
/// 文档：不强迫仓库里已有的全部调用点都多传一份伤害类别目录——传
/// [`NoDamageCategories`] 与"不传"在行为上完全等价（两者都让默认伤害
/// 类别恒为 [`ContentIndex::default()`](ll_core::ident::ContentIndex::default)，与任何真实注册的伤害类别都
/// 不会撞上，见 [`NoDamageCategories`] 文档），本函数只服务真正想让
/// "武器没声明伤害类别时退回哪个默认类别"生效的调用方
/// （`ll_mod::damage_category` 落地对应的真实目录实现后即可接入）。
///
/// **本函数不改变抗性本身生不生效**——抗性查询
/// （[`resistance_damage_reduction`](crate::rule_modifier::resistance_damage_reduction)）只要防御方的天赋声明了
/// `RuleModifier::Resistance` 就会命中，与本函数是否接了真实的伤害
/// 类别目录无关；本函数只影响"武器没有显式声明伤害类别"这一种情形
/// 下退回的默认类别是哪一个。
#[allow(clippy::too_many_arguments)]
pub fn resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
    items: &dyn ItemCatalog,
    formulas: &dyn DamageFormulaCatalog,
    damage_categories: &dyn DamageCategoryCatalog,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        skills,
        &NoQuests,
        race_traits,
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        traits,
        pools,
        items,
        formulas,
        damage_categories,
        &NoRecipes,
        AmbientSource::NONE,
        &NoExperience,
        &NoSkills,
        &NoSubclassUnlocks,
        &NoDialogues,
        &NoContentIds,
    )
}

/// [`resolve`] 的全目录入口：在
/// [`resolve_with_skills_traits_pools_items_formulas_and_damage_categories`]
/// 之上再额外接收**职业**与**副职**两份天赋授予来源，让
/// `trait-system.md` 三节①「有效天赋 = 种族天赋 ∪ 职业天赋 ∪ 副职天赋
/// ∪ ……」里这两路真正参与结算（职业那一路来自职业天赋接线批次，
/// `ll_mod::class::ClassTable` 是它的真实实现；副职那一路来自副职天赋
/// 接线批次，`ll_mod::subclass::SubclassTable` 是它的真实实现，且它是
/// 唯一一路会被 [`crate::traits::agent_trait_sources`] 展开成**多个**
/// 来源的——`Agent::subclasses` 是 `Vec` 而不是单值）。
///
/// # 为什么名字不再继续拼接
///
/// 前八层入口按「新增了哪份目录」逐层拼接命名，到上一层
/// （`..._formulas_and_damage_categories`）已经是 62 个字符；再拼一段
/// `_and_class_traits`（再往后还有 `_and_subclass_traits`）只会得到一个
/// 没人读得完、也无法在文档里换行的
/// 名字。这一层因此改用描述性的「全目录」命名：它是这条链条的终点，
/// `resolve_dispatch` 当前需要的每一份只读依赖都由调用方显式给出，
/// 没有任何一份被替换成空实现——名字要传达的正是这件事，而不是
/// 「比上一层多了哪一个」。
///
/// 其余八层入口保持原签名不变（职业与副职两路都传
/// [`NO_TRAIT_GRANTS`]），与它们当初逐层新增目录时「不强迫既有调用点
/// 都多传一份目录」是同一条纪律：传空来源与「这一路没接」在行为上
/// 完全等价（`effective_traits` 对空来源现算出空集合）。
#[allow(clippy::too_many_arguments)]
pub fn resolve_with_all_catalogs(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    quests: &dyn QuestCatalog,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
    items: &dyn ItemCatalog,
    formulas: &dyn DamageFormulaCatalog,
    damage_categories: &dyn DamageCategoryCatalog,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        skills,
        quests,
        race_traits,
        class_traits,
        subclass_traits,
        traits,
        pools,
        items,
        formulas,
        damage_categories,
        &NoRecipes,
        AmbientSource::NONE,
        &NoExperience,
        &NoSkills,
        &NoSubclassUnlocks,
        &NoDialogues,
        &NoContentIds,
    )
}

/// [`resolve_with_all_catalogs`] 的「一束引用」版本：九份目录改由
/// [`ResolveCatalogs`] 一次性带进来，其余行为逐字相同（本函数就是把
/// 那一束拆开转发给同一个 `resolve_dispatch`）。
///
/// # 为什么两个入口并存
///
/// [`resolve_with_all_catalogs`] 的散参数签名是给**直接调用结算**的
/// 代码用的：签名把「这段结算依赖哪几份只读内容」写在脸上，是依赖
/// 倒置这套手法刻意要留的信号（见 `resolve_dispatch` 文档「不是可以
/// 合并成一个结构体的意外堆叠」一节）。本函数服务的是另一类调用方：
/// **把目录搬过一层边界、自己一份都不读**的中间层——目前唯一的这类
/// 调用方是 [`crate::turn::TurnEngine`]，见 [`crate::catalogs`] 模块
/// 文档「为什么需要这一束」一节。
pub fn resolve_with_catalogs(
    world: &WorldState,
    intent: &Intent,
    catalogs: &ResolveCatalogs<'_>,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        catalogs.skills,
        catalogs.quests,
        catalogs.race_traits,
        catalogs.class_traits,
        catalogs.subclass_traits,
        catalogs.trait_defs,
        catalogs.pools,
        catalogs.items,
        catalogs.formulas,
        catalogs.damage_categories,
        catalogs.recipes,
        catalogs.ambient,
        catalogs.experience,
        catalogs.skill_tree,
        catalogs.subclass_unlocks,
        catalogs.dialogues,
        catalogs.content_ids,
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
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        &NoTraits,
        &NoResourcePools,
        &NoItems,
        &NoFormulas,
        &NoDamageCategories,
        &NoRecipes,
        AmbientSource::NONE,
        &NoExperience,
        &NoSkills,
        &NoSubclassUnlocks,
        &NoDialogues,
        &NoContentIds,
    )
}

/// [`resolve_with_skills_and_quests`]/[`resolve_with_skills_and_traits`]/
/// [`resolve_with_skills_traits_and_pools`]/
/// [`resolve_with_skills_traits_pools_and_items`]/
/// [`resolve_with_skills_traits_pools_items_and_formulas`]/
/// [`resolve_with_skills_traits_pools_items_formulas_and_damage_categories`]
/// 共用的核心分派逻辑——六个公开入口都只是"缺一份目录时传对应的 `No*`
/// 空实现"的薄封装，真正的 `Intent` 匹配与效果产出只写这一份，不重复。
///
/// `#[allow(clippy::too_many_arguments)]`：十个参数分别对应九种
/// 结算需要的只读依赖（技能/任务/种族天赋来源/天赋/资源池/物品/伤害
/// 公式/伤害类别目录）加 `world`/`intent` 本身，拆分成多份目录正是
/// 「resolve 依赖倒置」这套手法刻意要做的事（见模块文档同一批目录的
/// 既有取舍），不是可以合并成一个结构体的意外堆叠——与
/// `crates/ll-sim/tests/resource_pool_resolve.rs` 的
/// `spawn_agent_with_pool` 同一条既有先例。
#[allow(clippy::too_many_arguments)]
fn resolve_dispatch(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    quests: &dyn QuestCatalog,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
    items: &dyn ItemCatalog,
    formulas: &dyn DamageFormulaCatalog,
    damage_categories: &dyn DamageCategoryCatalog,
    recipes: &dyn RecipeCatalog,
    ambient: AmbientSource<'_>,
    experience: &dyn ExperienceCatalog,
    skill_tree: &dyn SkillTreeCatalog,
    subclass_unlocks: &dyn SubclassUnlockCatalog,
    // 对话批次 2 新增的两路。**追加在参数表末尾**，不插在中间：
    // 九个调用点的实参是按位置对上的，插在中间会让每一处静默错位。
    dialogues: &dyn DialogueCatalog,
    content_ids: &dyn ContentIdLookup,
) -> Vec<Effect> {
    let mut effects = match *intent {
        Intent::Wait { actor } => resolve_wait(
            world,
            actor,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
            pools,
        ),
        Intent::Move { actor, dir } => resolve_move(world, actor, dir),
        Intent::Swap { actor, with } => resolve_swap(world, actor, with),
        Intent::Attack { actor, target } => resolve_attack(
            world,
            actor,
            target,
            items,
            formulas,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
            damage_categories,
            ambient,
        ),
        Intent::OpenDoor { actor, pos } => resolve_open_door(world, actor, pos),
        Intent::CloseDoor { actor, pos } => resolve_close_door(world, actor, pos),
        Intent::EnterSpace { actor, target } => resolve_enter_space(world, actor, target),
        Intent::ExitSpace { actor } => resolve_exit_space(world, actor),
        Intent::UseSkill {
            actor,
            skill,
            target,
        } => resolve_use_skill(
            world,
            actor,
            skill,
            target,
            skills,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
        ),
        Intent::Rest {
            actor,
            target_ticks,
        } => resolve_rest(
            world,
            actor,
            target_ticks,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
            pools,
        ),
        Intent::PickUp { actor, pos, def } => resolve_pick_up(world, actor, pos, def, items),
        Intent::Loot { actor, pos } => resolve_loot(world, actor, pos, items),
        Intent::Drop { actor, def } => resolve_drop(world, actor, def),
        Intent::Place { actor, def } => resolve_place(world, actor, def, items, ambient),
        Intent::Equip { actor, def } => resolve_equip(world, actor, def, items),
        Intent::Unequip { actor, slot } => resolve_unequip(world, actor, slot, items),
        Intent::Use { actor, def } => resolve_use_item(world, actor, def, items),
        Intent::Inspect { actor, target } => resolve_inspect(
            world,
            actor,
            target,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
            items,
        ),
        Intent::ToggleStealth { actor } => resolve_toggle_stealth(world, actor),
        Intent::Craft { actor, recipe } => resolve_craft(
            world,
            actor,
            recipe,
            recipes,
            items,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
        ),
        Intent::AllocateAttributePoint { actor, attribute } => {
            resolve_allocate_attribute_point(world, actor, attribute)
        }
        Intent::LearnSkill { actor, skill } => resolve_learn_skill(world, actor, skill, skill_tree),
        Intent::AbandonSubclass { actor, subclass } => {
            resolve_abandon_subclass(world, actor, subclass)
        }
        Intent::Read { actor, def } => resolve_read(world, actor, def, items),
        Intent::Identify { actor, def } => resolve_identify(world, actor, def, items),
        Intent::Experiment { actor, category } => {
            resolve_experiment(world, actor, category, recipes)
        }
        Intent::DialogueChoose {
            actor,
            speaker,
            node,
            option,
        } => resolve_dialogue_choose(
            world,
            actor,
            speaker,
            node,
            option,
            dialogues,
            content_ids,
            items,
        ),
    };
    // 副职使用计数（副职获得机制批次）：一次**成功**的制作把对应配方
    // 类别的累计次数推进一格，达标就产出 `Effect::GrantSubclass`。
    //
    // # 为什么挂在这里，而不是塞进 `resolve_craft` 内部
    //
    // 与 `append_quest_kill_progress`/`append_kill_experience` 同一个
    // 位置、同一个理由：`resolve_craft` 回答的是「这次制作做不做得
    // 成」，计数回答的是「做成了之后顺带发生什么」。两者混在一个函数
    // 里会让那个函数同时对两件事负责，而分开之后「全部入口都会经过
    // 这一处」这条保证由 `resolve_dispatch` 本身给出——正是击杀经验
    // 那次「只在测试里成立的接线」的修法。
    append_craft_progress(world, intent, &mut effects, recipes, subclass_unlocks);
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
        class_traits,
        subclass_traits,
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
    // 死亡掉落（NPC 生命周期批次）：与击杀历史记录同一个触发点（同一批
    // Effect::Kill），各自独立追加,互不依赖——见 append_corpse_drop
    // 文档。不需要按 Intent 类型区分调用与否,理由同 append_kill_history。
    append_corpse_drop(world, &mut effects, items);
    // 击杀经验：与击杀历史记录/死亡掉落同一个触发点（同一批
    // `Effect::Kill`），各自独立追加，互不依赖——见
    // `append_kill_experience` 文档。
    //
    // # 为什么现在挪进 `resolve_dispatch`，不再只挂在一个专用入口上
    //
    // 接线批次当初把它挂在 `resolve_with_skills_quests_and_experience`
    // 这个第四层专用入口里，而生产路径（`ll-game` 全程只经
    // `crate::turn::TurnEngine` 驱动世界）走的是
    // `resolve_with_catalogs` → 本函数——两条路从不相交，于是**真正
    // 能跑起来的游戏里，击杀从来没有产出过任何经验**，全部证据都止步
    // 于集成测试直接调那个专用入口。这与 `TurnEngine` 文档记的天赋
    // 系统那次「只在测试里成立的接线」是同一类缺陷的第三次复发，修法
    // 同样是把它放到全部入口都会经过的那一处（本函数），而不是再补
    // 一份只在测试里成立的证据。不传经验目录的既有入口传
    // `&NoExperience`，与接线之前逐字等价。
    append_kill_experience(world, &mut effects, experience);
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
/// `resolve_use_skill` 对**每一次**击杀产出，是前者的严格超集，接线
/// 因此复用 [`append_kill_history`] 已经在扫描的同一批 `effects`，见
/// [`append_kill_experience`]。
///
/// # 本入口现在只是 `resolve_dispatch` 的薄封装
///
/// 升级加点批次把 `append_kill_experience` 的调用挪进了
/// `resolve_dispatch` 本身（理由见那里的注释：挂在这个专用入口上意味
/// 着走 [`crate::turn::TurnEngine`] 的生产路径永远拿不到经验）。本
/// 函数因此不再自己追加任何东西，只是「除经验目录外其余全接空实现」
/// 的那一层薄封装，与 [`resolve_with_skills_and_quests`] 完全同构。
/// 保留它是为了不破坏既有调用点。
pub fn resolve_with_skills_quests_and_experience(
    world: &WorldState,
    intent: &Intent,
    skills: &dyn SkillCatalog,
    quests: &dyn QuestCatalog,
    experience: &dyn ExperienceCatalog,
) -> Vec<Effect> {
    resolve_dispatch(
        world,
        intent,
        skills,
        quests,
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        &NoTraits,
        &NoResourcePools,
        &NoItems,
        &NoFormulas,
        &NoDamageCategories,
        &NoRecipes,
        AmbientSource::NONE,
        experience,
        &NoSkills,
        &NoSubclassUnlocks,
        &NoDialogues,
        &NoContentIds,
    )
}

/// 算出「从现在起 `cost` 个 tick 之后」的世界时刻。
fn schedule_after(world: &WorldState, cost: u32) -> Tick {
    Tick(world.clock.0 + i64::from(cost))
}

/// 「伸手够得着」的范围：切比雪夫距离 1，即脚下加相邻八格。
///
/// # 这个数字从哪来
///
/// 项目所有者定的交互形状是「按空格 → 扫一圈 → 选一格 → 选这格上的
/// 哪一样」。那一圈就是本常量：**与移动的方向数一致**（[`Direction`](crate::intent::Direction)
/// 是八向，含四条对角线），一个「伸手够得着的一圈」若只认正交四向，
/// 玩家会遇到「斜前方那堆东西看得见、走一步就到，却伸手够不着」这种
/// 毫无道理的不一致。
///
/// 不做成内容字段：它不是「某件东西有多远能够到」这种随内容变化的量，
/// 是「一个人伸手能够到多远」这条全局规则，本仓库今天也没有任何需要
/// 它变化的场景（YAGNI）。真要变（长柄工具？），加法是给
/// `ll_mod::item::ItemDef` 加一条 `reach`，本常量退化成默认值。
const INTERACT_REACH: u32 = 1;

/// `origin` 伸手够不够得着 `target`。
///
/// 用 [`ll_core::torus::TorusSize::chebyshev`]，不是自己减坐标：世界是
/// 环面，跨接缝时裸减法会算出一个绕整圈的巨大距离。切比雪夫而不是曼
/// 哈顿/欧氏：八向移动一步的代价相同，"够得着"的形状因此是一个正方形
/// 邻域，不是菱形也不是圆。
fn within_reach(world: &WorldState, origin: TorusPos, target: TorusPos) -> bool {
    world.size.chebyshev(origin, target) <= INTERACT_REACH
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
