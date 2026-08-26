//! 引擎自带的行为树——用 Rust 写的两棵 AI：哥布林与卫兵。
//!
//! # 为什么是 Rust，不是脚本、也不是 JSON5
//!
//! 这两棵树此前住在 `mods/example_mod/behavior.scm`（121 行文件、38 行
//! 有效逻辑），是仓库里唯一的真逻辑。脚本系统整体要拆（`steel-core`
//! 0.8.2 的内存破坏缺陷，ADR 0028），所以它们要搬家。
//!
//! 项目所有者的裁定是「先做 json5 就好了，其他搬迁回系统内」——**第三
//! 方 mod 的行为扩展能力是一个明确推迟的决定**，不是一个还没想清楚的
//! 问题。因此本模块**没有**节点注册表、没有按名字查表的原语、没有
//! 「树结构写成数据文件」那一层：为一个已经决定不做的东西预留扩展点，
//! 正是这个代码库反复踩过的「声明了但从没接线」。真要做第三方扩展，
//! 那时候再设计，不是现在留一个猜出来的形状。
//!
//! 接口那一层保留：[`ll_sim::behavior::BehaviorTreeSource`] 与
//! `behavior_ai_intent` → `TurnEngine::advance_ai` 这条接线一个字没
//! 改——它是 `ll-sim` 与「决策从哪来」之间的正当边界，与决策实现是
//! 脚本还是 Rust 无关。变的只是这个 trait 的实现。
//!
//! # 约束怎么由签名保证，不靠纪律
//!
//! - **ADR 0023 / C1（决策不写世界）**：[`BehaviorTreeSource::decide`]
//!   的签名是 `&WorldState`（共享引用）。本模块全部内部函数同样只接
//!   `&WorldState`，**物理上拿不到 `&mut`**，因此写不了世界。真正的
//!   写入只发生在调用方对 `resolve*` 产出的 `Effect` 调用 `apply` 之后。
//! - **C3（随机只走确定性流）**：随机数唯一的来源是 `decide` 开头用
//!   [`DetRng::for_entity`] 派生的那一条流，并作为 `&mut DetRng` 参数
//!   逐层传下去。判定分支的签名要它，本模块没有任何一处能拿到
//!   `rand::thread_rng()` 或系统时间——`ll-mod` 的依赖里根本没有
//!   `rand`，也没有引入 `std::time`。
//! - **C5（不用 `HashMap` 迭代做决策）**：找目标走
//!   [`ll_sim::ai_query`]（`Arena::iter_with_id` 固定顺序 + `EntityId`
//!   升序打破平局）；本模块自己不遍历任何容器。
//!
//! # 与脚本版的等价性
//!
//! 两棵树的分支顺序、每个分支的判据、以及**随机数的调用次数与时机**
//! 逐条对齐：
//!
//! - 哥布林 selector 三条分支：技能 → 近战 → 靠近/等待，一次随机都不
//!   掷。
//! - 卫兵 selector 两条分支：盘查 → 靠近/等待。`rng.chance` 恰好在
//!   「职业匹配**且**视野内找到目标」时调用一次，与
//!   `guard-try-inspect` 里 `(and target (rng-chance ...))` 的短路顺序
//!   逐字相同——`and` 先求值 `target`，只有非 `#f` 才会去掷骰。这一条
//!   决定了同一颗种子下的决策序列，不是可有可无的细节。
//!
//! # 跨界成本没有了
//!
//! 脚本时代每次跨界（Rust → Steel → Rust）实测 326ns，这是 ADR 0016/
//! 0017 当初判断「哪些事件/查询给得起」时的主要成本项。Rust 原语是
//! 直接函数调用，**这项成本归零**，那两条 ADR 的取舍前提因此已经放
//! 宽。**本批次不顺手扩大范围**，只做等价迁移；重新评估留给后续批次。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::rng::DetRng;
use ll_sim::ai_query::{
    NEARBY_ACTOR_VIEW_RADIUS, direction_toward, is_stealthed, nearest_hostile,
    nearest_visible_actor,
};
use ll_sim::behavior::BehaviorTreeSource;
use ll_sim::check::{CHECK_DICE, CheckSide, INSPECTION_CHECK, opposed_check};
use ll_sim::formula::attribute_modifier;
use ll_sim::intent::Intent;
use ll_sim::resolve::derive_stats;
use ll_sim::rule_modifier::{
    RuleModifierEntry, agent_rule_modifiers, check_reroll_value, check_roll_bias,
    inconspicuous_check_modifier,
};
use ll_world::entity::AttributeKind;
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

use crate::class::ClassTable;
use crate::item::ItemTable;
use crate::race::RaceTable;
use crate::registry::Registry;
use crate::subclass::SubclassTable;
use crate::trait_def::TraitTable;

/// 目标处于潜行状态时，**加在被盘查者那一侧**的判定修正点数（负值
/// 减在主动方赢面上，因为它是加在被动方的点数上）。
///
/// 取一整颗骰子的跨度（[`ll_sim::check::CheckDice::whole_die`]，
/// `3d20` 下是 `19`）。
///
/// # 这个常量替掉了什么
///
/// 此前这里是**两个**常量：`GUARD_INSPECT_CHANCE_PERMILLE`（500‰）与
/// `GUARD_INSPECT_CHANCE_PERMILLE_STEALTHED`（50‰）。潜行的作用是在
/// 两个基数之间二选一——一个 10× 的**乘法档**，而同时作用在结果上的
/// 「不起眼」被动是一个**加法量**。两个量不在同一把尺子上，后果是同
/// 一条被动在两个档上的效果差一个数量级（`−400‰` 从 `500‰` 上减是
/// 砍掉八成，从 `50‰` 上减直接触底被钳在 `1‰`）。完整论证见
/// `ll_sim::check` 模块文档「这个模块治的是什么病」一节。
///
/// 换成对抗判定之后潜行不再换基数，它与「不起眼」一样是隐蔽方的
/// **一个修正**，两者直接相加。旧的两个基数因此一起消失：判定的
/// 「基数」现在是双方各掷一轮 `MdN`，不是一个写死的概率。
///
/// # 为什么是一整颗骰子
///
/// 需要一个**有内在依据**的档位，不是照着旧的 `50‰` 拟合出来的数。
/// 「一整颗骰子」是这把尺子上唯一自明的大单位；被动天赋取它的一半
/// （[`ll_sim::check::CheckDice::half_die`]），于是「主动藏起来」严格
/// 强于「天生不起眼」，而两者叠加恰好顶到修正上限
/// （`19 + 9 = 28 = L`）不触发钳制，见 `ll_sim::check::CHECK_DICE`
/// 文档「为什么是 3 颗」。
///
/// 潜行**仍然不改可见性**——卫兵照常看得见潜行中的目标
/// （[`nearest_visible_actor`] 一个字都没改），落下去的仍然是「要不要
/// 把这个人当回事」这一次判定，见 `ll_sim::ai_query::is_stealthed`。
pub const GUARD_INSPECT_STEALTH_MODIFIER: i64 = CHECK_DICE.whole_die();

/// 哥布林那棵树优先施放的技能 id。
pub const GOBLIN_SKILL_ID: &str = "examplemod:frostbolt";

/// 卫兵那棵树认的职业 id。
pub const GUARD_PROFESSION_ID: &str = "lostland:guard";

/// 卫兵那棵树查「盘查意愿」需要的那四张内容表的**一次性快照**。
///
/// # 为什么是快照（`Clone`），不是借用
///
/// [`NativeBehaviorSource`] 要作为
/// [`ll_sim::behavior::BehaviorTreeSource`] 被
/// `TurnEngine::advance_ai` 的 `&mut dyn FnMut` 持有，那条链路上没有
/// 任何一处能提供借用所需的生命周期。
///
/// 快照与实时读在这里**语义上无差别**：`Registry` 与这四张表都在 mod
/// 装载完成后就不再变化（运行期不会有新 mod 中途注册新天赋），不存在
/// 两份真相漂移的可能。
///
/// 整张表留着、不折叠成一份静态映射，是因为本查询的答案依赖**实体
/// 运行期的状态**（种族/职业/等级/已装备物品）。
///
/// # 为什么打包成一个结构体，不是四个参数
///
/// 与 `ll_sim::catalogs::ResolveCatalogs` 同一条既有手法：这几张表是
/// 「聚合规则修正」这一件事的完整输入，接新一路来源时只需要给本结构体
/// 加字段，不必改全部调用点的签名——副职天赋接线批次新增
/// [`BehaviorRuleCatalogs::subclass`] 就是这条预言的第一次兑现（本结构
/// 体多一个字段，当时的 `suspicion_reduction_permille_of` 多传一个参数，
/// `BehaviorRuleCatalogs::default()` 那批调用点一个字都没改）。
#[derive(Debug, Clone, Default)]
pub struct BehaviorRuleCatalogs {
    /// 种族这一路天赋来源。
    pub race: RaceTable,
    /// 职业这一路天赋来源——`examplemod:cutpurse_training` 正是走这
    /// 一路（`mods/example_mod/classes.json5` 的盗贼，3 级解锁）。
    pub class: ClassTable,
    /// 副职这一路天赋来源（副职天赋接线批次新增）——所有者逐个取
    /// `Agent::subclasses`，见
    /// `ll_sim::traits::agent_trait_sources` 文档。
    pub subclass: SubclassTable,
    /// 天赋定义表。
    pub traits: TraitTable,
    /// 物品定义表——规则修正的第二路来源（装备）。
    pub items: ItemTable,
}

impl BehaviorRuleCatalogs {
    /// 从调用方持有的四张表各克隆一份，理由见类型文档「为什么是快照」。
    pub fn snapshot(
        race: &RaceTable,
        class: &ClassTable,
        subclass: &SubclassTable,
        traits: &TraitTable,
        items: &ItemTable,
    ) -> Self {
        Self {
            race: race.clone(),
            class: class.clone(),
            subclass: subclass.clone(),
            traits: traits.clone(),
            items: items.clone(),
        }
    }

    /// 一个实体此刻的规则修正候选列表——[`agent_rule_modifiers`] 在
    /// 这份快照上的应用。查不到实体时返回空列表，与本模块其余查询
    /// 同一条降级纪律。
    ///
    /// 返回整份列表而不是某一个算好的数：盘查判定要从同一份列表里读
    /// **三样**东西（不起眼修正、优劣势、重掷面值），聚合一次读三遍
    /// 比聚合三遍便宜，也保证三者读的是同一时刻的同一批声明。
    pub fn rule_modifiers_of(
        &self,
        world: &WorldState,
        target: EntityId,
    ) -> Vec<RuleModifierEntry> {
        match world.actors.get(target) {
            Some(agent) => agent_rule_modifiers(
                agent,
                &self.race,
                &self.class,
                &self.subclass,
                &self.traits,
                &self.items,
            ),
            None => Vec::new(),
        }
    }

    /// 一个实体此刻某一项属性的**派生**调整值 `(属性 − 10) / 2`——
    /// 装备与状态效果加的属性在这里生效，与
    /// `ll_sim::resolve::resolve_attack` 读
    /// `attacker_derived.attribute(..)` 是同一条既有纪律。
    ///
    /// 用不带环境温度的 [`derive_stats`]（内部代入中性温度）：行为树
    /// 这一层拿不到 `AmbientSource`，而温度**只**惩罚力量一项，对本
    /// 模块要读的意志/敏捷两项逐位无影响。
    ///
    /// 查不到实体时返回 `0`（等同基准属性 10）。
    fn attribute_modifier_of(
        &self,
        world: &WorldState,
        entity: EntityId,
        kind: AttributeKind,
    ) -> i64 {
        match world.actors.get(entity) {
            Some(agent) => attribute_modifier(
                derive_stats(
                    agent.stats,
                    &agent.active_stat_modifiers,
                    &agent.equipment,
                    &self.items,
                    world.clock,
                )
                .attribute(kind),
            ),
            None => 0,
        }
    }
}

/// 引擎自带的两棵行为树。
///
/// 枚举而不是 trait 对象：一共两棵，且**第三方加不了新的**（那要改
/// 引擎源码）。`match` 一次列全，编译器负责不漏。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeBehaviorTree {
    /// 哥布林：附近有敌人就优先放技能，技能不可用就近战，都不行就
    /// 走近一步或原地等待。
    Goblin {
        /// 优先施放的技能；注册表里查不到（没装那个 mod）时为 `None`，
        /// 这一支恒不成立，树自然降级为近战——与脚本时代
        /// `skill-ready?` 拿到一个未知名字返回 `#f` 逐字同义。
        skill: Option<ContentIndex>,
    },
    /// 卫兵：按概率盘查视野内最近的目标，否则走近一步或原地等待。
    Guard {
        /// 这棵树只对这个职业的实体生效；注册表里查不到时为 `None`，
        /// 盘查那一支恒不成立。
        profession: Option<ContentIndex>,
    },
}

impl NativeBehaviorTree {
    /// 从注册表解析出哥布林那棵树需要的技能索引。
    pub fn goblin(registry: &Registry) -> Self {
        NativeBehaviorTree::Goblin {
            skill: lookup(registry, GOBLIN_SKILL_ID),
        }
    }

    /// 从注册表解析出卫兵那棵树需要的职业索引。
    pub fn guard(registry: &Registry) -> Self {
        NativeBehaviorTree::Guard {
            profession: lookup(registry, GUARD_PROFESSION_ID),
        }
    }
}

/// 查一个已知字符串对应的内容索引；没注册就是 `None`（不 intern——
/// 决策来源不该凭空造出内容，ADR 0015）。
fn lookup(registry: &Registry, id: &str) -> Option<ContentIndex> {
    let parsed = NamespacedId::parse(id).ok()?;
    registry.get(&parsed)
}

/// 引擎自带行为树的 [`BehaviorTreeSource`] 实现。
///
/// 与它取代的 `ScriptBehaviorSource` 相比少了两样东西：没有
/// `ScriptEngine`（因此没有「构造必须先于编译」那条 ADR 0028 规避
/// 条件，也没有 `PreparedBehaviorEngine`），也没有脚本状态读写通道。
pub struct NativeBehaviorSource {
    tree: NativeBehaviorTree,
    /// 聚合规则修正要的四张内容表快照——卫兵那棵树查目标的「盘查
    /// 意愿」，见 [`BehaviorRuleCatalogs`]。
    catalogs: BehaviorRuleCatalogs,
    /// 喂给 [`DetRng::for_entity`] 的世界种子（C3）。
    world_seed: u64,
}

impl NativeBehaviorSource {
    /// 造一个跑 `tree` 的决策来源。
    pub fn new(tree: NativeBehaviorTree, catalogs: BehaviorRuleCatalogs, world_seed: u64) -> Self {
        Self {
            tree,
            catalogs,
            world_seed,
        }
    }
}

impl BehaviorTreeSource for NativeBehaviorSource {
    /// 求值一次行为树。
    ///
    /// # C3：随机性走 `DetRng::for_entity`
    ///
    /// `event_counter` 取当前世界时钟——同一个实体在同一个世界时刻只会
    /// 决策一次（回合制），用世界时钟当计数器天然满足「同一实体不同
    /// 决策事件要给出不同的流」，且不需要在 `Agent` 上新增一个决策
    /// 计数器字段。与脚本时代 `ScriptBehaviorSource::decide` 逐字相同
    /// 的派生方式。
    fn decide(&mut self, world: &WorldState, actor: EntityId) -> Option<Intent> {
        let mut rng = DetRng::for_entity(self.world_seed, actor.as_u64(), world.clock.0 as u64);
        match self.tree {
            NativeBehaviorTree::Goblin { skill } => goblin_tick(world, actor, skill),
            NativeBehaviorTree::Guard { profession } => {
                guard_tick(world, actor, profession, &self.catalogs, &mut rng)
            }
        }
    }
}

// ─────────────────────────── 哥布林那棵树 ───────────────────────────

/// 哥布林 selector：技能 → 近战 → 靠近/等待，第一个成立的分支胜出。
///
/// 三条分支各自重新问一次「附近有没有敌人」，与脚本时代三个 `define`
/// 各自调一次 `(nearby-enemy)` 逐字相同——那不是冗余，是 selector 语义
/// 的直接后果（每个分支都是独立可复用的判断，不共享上一条分支的中间
/// 结果）。查询本身不写世界、也不掷骰，重复调用没有可观察差别。
fn goblin_tick(world: &WorldState, actor: EntityId, skill: Option<ContentIndex>) -> Option<Intent> {
    goblin_try_skill(world, actor, skill)
        .or_else(|| goblin_try_attack(world, actor))
        .or_else(|| goblin_try_approach(world, actor))
}

/// 分支一：附近有敌人且技能可用 → 施放技能。
fn goblin_try_skill(
    world: &WorldState,
    actor: EntityId,
    skill: Option<ContentIndex>,
) -> Option<Intent> {
    let enemy = nearest_hostile(world, actor)?;
    let skill = skill?;
    skill_ready(world, actor, skill).then_some(Intent::UseSkill {
        actor,
        skill,
        target: Some(enemy),
    })
}

/// 分支二：附近有敌人（技能不可用）→ 普通近战攻击。
fn goblin_try_attack(world: &WorldState, actor: EntityId) -> Option<Intent> {
    let target = nearest_hostile(world, actor)?;
    Some(Intent::Attack { actor, target })
}

/// 分支三（兜底，恒成立）：有敌人就走近一步，否则原地等待。
fn goblin_try_approach(world: &WorldState, actor: EntityId) -> Option<Intent> {
    let approach = nearest_hostile(world, actor)
        .and_then(|enemy| direction_toward(world, actor, enemy))
        .map(|dir| Intent::Move { actor, dir });
    Some(approach.unwrap_or(Intent::Wait { actor }))
}

/// 这个实体此刻能不能用 `skill`：已解锁，且不在冷却中。
///
/// 与 `ll_sim::resolve::resolve_use_skill` 的门一/门二完全同一条判断
/// （惰性冷却判定，现比对世界时钟）——本函数不重复发明规则，只是把同
/// 一条规则提前问一次。
///
/// **不检查资源是否充足**：那需要技能定义（消耗多少），本模块没有拿
/// `SkillTable`。资源不足时 `resolve_use_skill` 仍会静默拒绝，行为树
/// 因此可能白白选中一个用不起的技能，但不会产生错误效果——这是脚本
/// 时代 `skill-ready?` 原样承继的已知简化，不是本批次新引入的缺口。
fn skill_ready(world: &WorldState, actor: EntityId, skill: ContentIndex) -> bool {
    world.actors.get(actor).is_some_and(|agent| {
        agent.unlocked_skills.contains(&skill)
            && !agent
                .skill_cooldowns
                .get(&skill)
                .is_some_and(|until| until.0 > world.clock.0)
    })
}

// ──────────────────────────── 卫兵那棵树 ────────────────────────────

/// 卫兵 selector：盘查 → 靠近/等待。
fn guard_tick(
    world: &WorldState,
    actor: EntityId,
    profession: Option<ContentIndex>,
    catalogs: &BehaviorRuleCatalogs,
    rng: &mut DetRng,
) -> Option<Intent> {
    guard_try_inspect(world, actor, profession, catalogs, rng)
        .or_else(|| guard_try_approach(world, actor))
}

/// 分支一：**是卫兵职业**、视野内有目标、且这一次对抗判定卫兵赢 →
/// 盘查。
///
/// 三个条件的求值顺序与脚本时代 `guard-try-inspect` 逐字相同，而这
/// 不只是风格问题：判定只在前两个条件都成立时才发生，随机流因此只在
/// 「真的要判一次」时才前进。顺序改了，同一颗种子下的决策序列就变了。
///
/// **取数次数变了**：此前是一次 `rng.chance`（一个抽取），现在是一次
/// `2M` 抽取的对抗判定（`3d20` 双方各一轮 = 6 个；任一方有优劣势时
/// 那一方翻倍，有重掷时命中面值再多取）。这一条对确定性无害——这条
/// 流是 `decide` 开头现造的、只服务这一次决策的流，不是跨调用累进的
/// 长流，「这次多取了几个」不会让别处错位。
fn guard_try_inspect(
    world: &WorldState,
    actor: EntityId,
    profession: Option<ContentIndex>,
    catalogs: &BehaviorRuleCatalogs,
    rng: &mut DetRng,
) -> Option<Intent> {
    if !has_profession(world, actor, profession?) {
        return None;
    }
    let target = nearest_visible_actor(world, actor, NEARBY_ACTOR_VIEW_RADIUS)?;
    guard_inspection_check(world, actor, target, catalogs, rng)
        .then_some(Intent::Inspect { actor, target })
}

/// 分支二（兜底，恒成立）：视野内有目标就走近一步，否则原地等待。
fn guard_try_approach(world: &WorldState, actor: EntityId) -> Option<Intent> {
    let approach = nearest_visible_actor(world, actor, NEARBY_ACTOR_VIEW_RADIUS)
        .and_then(|target| direction_toward(world, actor, target))
        .map(|dir| Intent::Move { actor, dir });
    Some(approach.unwrap_or(Intent::Wait { actor }))
}

/// 这一次盘查的**对抗判定**：卫兵（察觉）主动，目标（隐蔽）被动，
/// 卫兵赢就发起盘查。
///
/// ```text
/// 卫兵（主动）：意志调整值
/// 目标（被动）：敏捷调整值 + 潜行修正 + 不起眼修正
/// ```
///
/// 「察觉 = 意志调整值、隐蔽 = 敏捷调整值」是项目所有者的裁定；本
/// 仓库没有独立的感知属性，`AttributeKind::Willpower` 是六项里承担
/// D&D「感知」概念的那一项。调整值公式 `(属性 − 10) / 2` 复用
/// `ll_sim::formula::attribute_modifier`，零新增字段、零存档影响。
///
/// # 潜行与「不起眼」仍然不是二选一
///
/// 它们回答的是不同的问题（这一刻我藏没藏起来 vs 我这个人天生多不
/// 起眼），一个盗贼在潜行时理应两者都生效。变的是**它们终于在同一把
/// 尺子上**：此前潜行换基数（乘法档）、不起眼减概率（加法量），现在
/// 两者都是加在被动方点数上的整数，直接相加。
///
/// # 数值换算（`3d20`，双方属性均为基准 10 因而两侧调整值均为 0）
///
/// | 情形 | 旧模型 | 新模型 |
/// | --- | --- | --- |
/// | 常态 | `500‰` | `486‰` |
/// | 仅潜行 | `50‰` | `86‰` |
/// | 仅「不起眼」天赋（9 点） | `100‰` | `255‰` |
/// | 潜行 + 天赋（28 点，恰好顶到上限） | `1‰`（触底） | `21‰` |
///
/// 常态那一档几乎逐字对上（`486‰` vs `500‰`）——这不是巧合：两个势均
/// 力敌的人各掷一轮同样的骰子，赢面本来就该是接近一半，旧的 `500‰`
/// 当初写的就是这个意思。真正变的是另外三档，而它们变的方向正是本次
/// 改型要的：旧模型里「潜行 + 天赋」触底被钳在 `1‰`，那个 `1‰` 不是
/// 设计，是旧模型那条「两端各留一线」的概率兜底留下的痕迹；新模型
/// 给出 `21‰`，一个由两个修正真正算出来的数。那条概率兜底
/// （`clamp_probability_permille`）随本批次一并删除——它的最后一个
/// 消费者就是这里，而「不允许绝对」现在由
/// `ll_sim::check::CheckDice::max_modifier` 的推导保证。
///
/// 全程整数（ADR 0020 乙区），一次整数除法都没有。
fn guard_inspection_check(
    world: &WorldState,
    actor: EntityId,
    target: EntityId,
    catalogs: &BehaviorRuleCatalogs,
    rng: &mut DetRng,
) -> bool {
    let guard_modifiers = catalogs.rule_modifiers_of(world, actor);
    let target_modifiers = catalogs.rule_modifiers_of(world, target);
    let stealth = if is_stealthed(world, target) {
        GUARD_INSPECT_STEALTH_MODIFIER
    } else {
        0
    };
    let active = CheckSide {
        modifier: catalogs.attribute_modifier_of(world, actor, AttributeKind::Willpower),
        bias: check_roll_bias(&guard_modifiers, INSPECTION_CHECK),
        reroll_on: check_reroll_value(&guard_modifiers),
    };
    let passive = CheckSide {
        modifier: catalogs
            .attribute_modifier_of(world, target, AttributeKind::Dexterity)
            .saturating_add(stealth)
            .saturating_add(i64::from(inconspicuous_check_modifier(&target_modifiers))),
        bias: check_roll_bias(&target_modifiers, INSPECTION_CHECK),
        reroll_on: check_reroll_value(&target_modifiers),
    };
    opposed_check(&CHECK_DICE, &active, &passive, rng).active_wins()
}

/// 这个实体的 `Agent.profession` 是否等于 `class`。
fn has_profession(world: &WorldState, actor: EntityId, class: ContentIndex) -> bool {
    world
        .actors
        .get(actor)
        .is_some_and(|agent| agent.profession == class)
}

#[cfg(test)]
mod tests {
    use ll_core::ident::Interner;
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::space::Space;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    use super::*;

    fn test_world() -> WorldState {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    fn spawn_agent_at(
        world: &mut WorldState,
        x: i32,
        y: i32,
        unlocked: Vec<ContentIndex>,
    ) -> EntityId {
        let mut interner = Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(x, y);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
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
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: unlocked,
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        })
    }

    #[test]
    fn 哥布林技能可用时选技能而不是近战() {
        // Arrange
        let mut registry = Registry::new();
        let skill = registry.intern(NamespacedId::parse(GOBLIN_SKILL_ID).expect("合法标识符"));
        let mut world = test_world();
        let goblin = spawn_agent_at(&mut world, 5, 5, vec![skill]);
        let prey = spawn_agent_at(&mut world, 7, 5, Vec::new());
        let mut source = NativeBehaviorSource::new(
            NativeBehaviorTree::goblin(&registry),
            BehaviorRuleCatalogs::default(),
            1,
        );

        // Act
        let intent = source.decide(&world, goblin);

        // Assert
        assert_eq!(
            intent,
            Some(Intent::UseSkill {
                actor: goblin,
                skill,
                target: Some(prey),
            })
        );
    }

    #[test]
    fn 哥布林技能未解锁时降级为近战() {
        // selector 的分支优先级真的在起作用，不是恰好只测了一条路径。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(NamespacedId::parse(GOBLIN_SKILL_ID).expect("合法标识符"));
        let mut world = test_world();
        let goblin = spawn_agent_at(&mut world, 5, 5, Vec::new());
        let prey = spawn_agent_at(&mut world, 7, 5, Vec::new());
        let mut source = NativeBehaviorSource::new(
            NativeBehaviorTree::goblin(&registry),
            BehaviorRuleCatalogs::default(),
            1,
        );

        // Act
        let intent = source.decide(&world, goblin);

        // Assert
        assert_eq!(
            intent,
            Some(Intent::Attack {
                actor: goblin,
                target: prey,
            })
        );
    }

    #[test]
    fn 哥布林附近没有目标时兜底为等待() {
        // Arrange
        let registry = Registry::new();
        let mut world = test_world();
        let goblin = spawn_agent_at(&mut world, 5, 5, Vec::new());
        let mut source = NativeBehaviorSource::new(
            NativeBehaviorTree::goblin(&registry),
            BehaviorRuleCatalogs::default(),
            1,
        );

        // Act
        let intent = source.decide(&world, goblin);

        // Assert
        assert_eq!(intent, Some(Intent::Wait { actor: goblin }));
    }

    #[test]
    fn 不是卫兵职业的实体永远不发起盘查() {
        // 这棵树只对卫兵职业生效——判据在树里，不在调用方。
        // Arrange：注册表里有 lostland:guard，但实体的职业是别的。
        // 职业**显式赋值**，不靠 spawn_agent_at 的默认值——那个默认值
        // 来自一个各自独立的 Interner，索引恰好可能与 registry 里的
        // 卫兵撞上（两个 0 号），一撞这条测试就会假绿/假红。
        let mut registry = Registry::new();
        registry.intern(NamespacedId::parse(GUARD_PROFESSION_ID).expect("合法标识符"));
        let civilian =
            registry.intern(NamespacedId::parse("lostland:civilian").expect("合法标识符"));
        let mut world = test_world();
        let bystander = spawn_agent_at(&mut world, 5, 5, Vec::new());
        spawn_agent_at(&mut world, 6, 5, Vec::new());
        world.actors.get_mut(bystander).expect("刚生成").profession = civilian;
        let mut source = NativeBehaviorSource::new(
            NativeBehaviorTree::guard(&registry),
            BehaviorRuleCatalogs::default(),
            1,
        );

        // Act：跑足够多帧，若职业判定失效必然会撞上一次盘查。
        let mut intents = Vec::new();
        for tick in 0..60i64 {
            world.clock = Tick(tick);
            intents.push(source.decide(&world, bystander));
        }

        // Assert
        assert!(
            !intents
                .iter()
                .flatten()
                .any(|intent| matches!(intent, Intent::Inspect { .. })),
            "非卫兵职业的实体不该发起任何盘查"
        );
    }

    #[test]
    fn 潜行让盘查次数显著下降() {
        // 潜行不改可见性，改的是这一次判定的成功率：500‰ → 50‰。
        // Arrange
        let mut registry = Registry::new();
        let guard_class =
            registry.intern(NamespacedId::parse(GUARD_PROFESSION_ID).expect("合法标识符"));
        let tree = NativeBehaviorTree::guard(&registry);

        let count_inspections = |stealthed: bool| -> usize {
            let mut world = test_world();
            let guard = spawn_agent_at(&mut world, 5, 5, Vec::new());
            let target = spawn_agent_at(&mut world, 6, 5, Vec::new());
            world.actors.get_mut(guard).expect("刚生成").profession = guard_class;
            world.actors.get_mut(target).expect("刚生成").stealthed = stealthed;
            let mut source = NativeBehaviorSource::new(tree, BehaviorRuleCatalogs::default(), 1);
            let mut hits = 0;
            for tick in 0..200i64 {
                world.clock = Tick(tick);
                if matches!(source.decide(&world, guard), Some(Intent::Inspect { .. })) {
                    hits += 1;
                }
            }
            hits
        };

        // Act
        let open = count_inspections(false);
        let hidden = count_inspections(true);

        // Assert：断言的是统计性质，不是逐帧序列——200 帧、500‰ vs
        // 50‰，两者相差一个数量级，中间留足余量。
        assert!(open > 60, "非潜行目标 200 帧应当被盘查很多次，实际 {open}");
        assert!(
            hidden < open / 3,
            "潜行目标的盘查次数应当显著低于非潜行（{hidden} vs {open}）"
        );
    }
}
