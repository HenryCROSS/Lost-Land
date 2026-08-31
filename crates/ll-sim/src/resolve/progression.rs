//! `resolve::progression`：角色成长与技能施放：属性点、学技能、弃子职业、放技能、资源池读写。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_world::entity::{Agent, AttributeKind, BaseStats, EntityId};
use ll_world::history::KillCause;
use ll_world::state::WorldState;

use crate::craft::RecipeCatalog;
use crate::effect::Effect;
use crate::intent::Intent;
use crate::resource_pool::{
    RegenRule, ResourcePoolCatalog, ResourcePoolShape, effective_scalar_capacity,
    effective_slot_tier_capacity,
};
use crate::skill::{ResourceCost, SkillCatalog, SkillEffect};
use crate::skill_overview::SkillTreeCatalog;
use crate::subclass::{SubclassUnlockCatalog, craft_progress_effects};
use crate::timeline::action_cost;
use crate::traits::{
    TraitCatalog, TraitGrantSource, agent_trait_sources, effective_traits, granted_skills,
};

use super::stats::effective_speed_from_dexterity;
use super::{BASE_ACTION_COST, schedule_after};

/// [`Intent::AllocateAttributePoint`] 结算（升级加点批次）：三道闸门
/// 全过才产出一条 [`Effect::AllocateAttributePoint`]，否则空列表。
///
/// 1. 发起者存在于世界里；
/// 2. 未分配属性点余额大于零；
/// 3. 目标属性当前的**基础值**尚未达到
///    [`BaseStats::HARD_CAP`]。
///
/// # 为什么是「拒绝」而不是「加到上限为止」
///
/// 已经在上限的属性上再加一点，钳位后属性一点没变、点数却少了一
/// 点——那是凭空吞掉玩家的点数。空效果列表意味着这次行动什么都没
/// 发生，玩家的余额原样保留，可以改加别的属性。
///
/// # 为什么不产出 `Effect::ScheduleNext`：加点是自由动作，不花回合
///
/// 本仓库几乎每个意图都会顺带产出一条
/// [`Effect::ScheduleNext`]（连撞墙都算一次行动，见 [`resolve_move`](super::movement::resolve_move)
/// 文档），本函数与 [`resolve_learn_skill`] 是**刻意的例外**：加点
/// 与学技能是角色面板上的决定，不是角色在世界里做的动作。若它们花
/// 掉一个回合，玩家每分配一点属性就要挨怪物一下——传统 roguelike 里
/// 没有任何一款会因为玩家打开角色面板而让怪物白打一轮。
///
/// 引擎侧的后果是明确的、也是想要的：[`crate::turn::TurnEngine::perform`]
/// 用行动者**未被改写**的 `next_action_at` 把它排回时间轴，于是这个
/// 角色立刻又轮到自己——正是「花点数不推进时间」这句话在逐实体时间
/// 轴上的准确表达。（AI 若反复提交这类意图会原地空转，由
/// `advance_ai` 的 `MAX_STEPS_PER_ADVANCE` 兜底；当前没有任何 AI 会
/// 提交它们，行为树只产出移动/攻击/等待。）
///
/// # 为什么比的是基础值，不是 `derive_stats` 的有效值
///
/// [`BaseStats::HARD_CAP`] 只约束基础值，装备与限时修正**可以突破**
/// （`knowledge/design/attribute-system.md`「成长上限」一节，
/// 见该常量文档）。若这里比的是有效值，一件 +5 力量的武器会让玩家
/// 无法再往力量里加点，脱下武器又能加——加点能不能加，取决于此刻手
/// 里拿着什么，那既不是设计要的，也会让玩家为了加点而反复穿脱装备。
pub(super) fn resolve_allocate_attribute_point(
    world: &WorldState,
    actor: EntityId,
    attribute: AttributeKind,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    if agent.unspent_attribute_points == 0 {
        return Vec::new();
    }
    if agent.stats.value(attribute) >= BaseStats::HARD_CAP {
        return Vec::new();
    }
    vec![Effect::AllocateAttributePoint { actor, attribute }]
}

/// [`Intent::LearnSkill`] 结算（升级加点批次）：四道闸门全过才产出
/// 一条 [`Effect::LearnSkill`]，否则空列表。
///
/// 1. 发起者存在于世界里；
/// 2. 未分配技能点余额大于零；
/// 3. 这个技能尚未解锁（重复学习不该再花一点）；
/// 4. 这个技能已注册，且它的前置技能全部已经解锁。
///
/// # 第 4 道闸门为什么要「已注册」这半句
///
/// [`SkillTreeCatalog::prerequisites`] 对未注册的索引返回空列表
/// （见其文档），单看前置判定，一个根本不存在的技能会「前置全部满
/// 足」而被学会——那会把一个查不到定义的索引写进
/// [`ll_world::entity::Agent::unlocked_skills`]，此后
/// [`crate::skill_overview`] 与存档重映射都要处理一个指向虚空的解锁
/// 记录。因此这里额外要求它出现在
/// [`SkillTreeCatalog::all_skills`] 里，与 ADR 0015「查不到就是查不
/// 到」一致。
///
/// # 不产出 `Effect::ScheduleNext`
///
/// 与 [`resolve_allocate_attribute_point`] 同一条理由（见其文档「加点
/// 是自由动作，不花回合」一节）：学技能是角色面板上的决定，不是角色
/// 在世界里做的动作。
///
/// # 前置判据与技能树面板同源
///
/// 用的是 [`crate::skill_overview::build_skill_tree_view`] 算
/// 「available」那一档时同一个目录、同一条规则（前置全部在已解锁集合
/// 里）——面板上显示为「可解锁」的技能，就是这里学得会的技能，两处
/// 不会漂移。
pub(super) fn resolve_learn_skill(
    world: &WorldState,
    actor: EntityId,
    skill: ContentIndex,
    skill_tree: &dyn SkillTreeCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    if agent.unspent_skill_points == 0 {
        return Vec::new();
    }
    if agent.unlocked_skills.contains(&skill) {
        return Vec::new();
    }
    if !skill_tree.all_skills().contains(&skill) {
        return Vec::new();
    }
    let unlocked: std::collections::BTreeSet<ContentIndex> =
        agent.unlocked_skills.iter().copied().collect();
    if !skill_tree
        .prerequisites(skill)
        .iter()
        .all(|prerequisite| unlocked.contains(prerequisite))
    {
        return Vec::new();
    }
    vec![Effect::LearnSkill { actor, skill }]
}

/// [`Intent::AbandonSubclass`] 结算（副职获得机制批次）：两道闸门全过
/// 才产出一条 [`Effect::RemoveSubclass`]，否则空列表。
///
/// 1. 发起者存在于世界里；
/// 2. 它确实持有这个副职（放弃一个没有的副职不该在存档里留下痕迹）。
///
/// # 不产出 `Effect::ScheduleNext`
///
/// 与 [`resolve_allocate_attribute_point`]/[`resolve_learn_skill`] 同一
/// 条理由（见前者文档「加点是自由动作，不花回合」一节）：放弃副职是
/// 角色面板上的决定，不是角色在世界里做的动作。
///
/// # 放弃的真实代价在闸门语义里，不在这个函数里
///
/// 本函数不扣任何资源。放弃之后立刻发生的事是：该副职把守的配方类别
/// **下一次制作就过不去了**（[`resolve_craft`](super::crafting::resolve_craft) 第③步每次都判），而
/// 已经通过它学会的技能不受影响。两种闸门语义的差异见
/// [`Effect::RemoveSubclass`] 文档。
pub(super) fn resolve_abandon_subclass(
    world: &WorldState,
    actor: EntityId,
    subclass: ContentIndex,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    if !agent.subclasses.contains(&subclass) {
        return Vec::new();
    }
    vec![Effect::RemoveSubclass { actor, subclass }]
}

/// 副职使用计数的接线：一次**成功**的 [`Intent::Craft`] 之后，把对应
/// 配方类别的累计制作次数推进一格，达标就追加
/// [`Effect::GrantSubclass`]（副职获得机制批次）。
///
/// # 「成功」怎么判断
///
/// [`resolve_craft`](super::crafting::resolve_craft) 的全部失败分支都返回**空** `Vec`（查不到行动者、
/// 查不到配方、副职闸门不过、场地/工具前置不满足、食材不够），成功
/// 时至少会产出一条 [`Effect::MergeIntoInventory`] 与一条
/// [`Effect::ScheduleNext`]。因此在 `resolve_dispatch` 那一处、
/// **`match` 刚返回、其余 `append_*` 都还没往里追加任何东西**的时刻，
/// `effects.is_empty()` 恰好就是「这次制作没做成」。本函数因此必须在
/// 那个位置调用，挪到别的 `append_*` 之后会让这个判据失效——这条位置
/// 约束写在这里，因为它不是从函数签名能看出来的。
///
/// # 为什么要再查一次配方
///
/// 计数按**配方类别**记（不是按具体配方），而 [`Intent::Craft`] 携带
/// 的是配方索引。`resolve_craft` 内部虽然已经查过一次，但它返回的是
/// `Vec<Effect>`，不带出 `rule`——为了让计数拿到 `rule.category` 而给
/// 那个函数加一个输出参数，会让「制作结算」这个职责被计数这件事污染。
/// 一次 `recipes.recipe(...)` 是一次表查询，代价可忽略。
pub(super) fn append_craft_progress(
    world: &WorldState,
    intent: &Intent,
    effects: &mut Vec<Effect>,
    recipes: &dyn RecipeCatalog,
    unlocks: &dyn SubclassUnlockCatalog,
) {
    let Intent::Craft { actor, recipe } = *intent else {
        return;
    };
    if effects.is_empty() {
        return;
    }
    let Some(rule) = recipes.recipe(recipe) else {
        return;
    };
    effects.extend(craft_progress_effects(world, actor, rule.category, unlocks));
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
pub(super) fn resolve_resource_pool_regen(
    world: &WorldState,
    actor: EntityId,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let mut effects = Vec::new();
    for trait_id in effective_traits(
        &agent_trait_sources(agent, race_traits, class_traits, subclass_traits),
        agent.level,
    ) {
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
/// （`RegenRule::OnTurnStart`）与 [`tiered_slot_rest_effects`](super::upkeep::tiered_slot_rest_effects)
/// （`RegenRule::OnRest` 的 `Amount` 分支）共用同一段算法,不重复实现
/// 两遍。只对 `agent.spent_slots` 里已消耗数非零的档位产出效果。
pub(super) fn restore_slots_from_lowest_tier(
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
/// ——与 [`resolve_attack`](super::combat::resolve_attack) 完全同一条纪律（见其文档）：是否致死是
/// 规则判断，必须在这里（`resolve`）做出，`apply` 只管照数字做加减。
/// 这一步此前缺失，技能永远打不死目标，也永远不会推进
/// [`append_quest_kill_progress`](super::combat::append_quest_kill_progress) 依赖的击杀任务进度——两处结算同属
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
///
/// `#[allow(clippy::too_many_arguments)]`：八个参数里有两个是同一个
/// `TraitGrantSource` 接口的不同来源（种族/职业，见
/// [`crate::traits::agent_trait_sources`]）——它们没有被合并成一个
/// 结构体，理由同 [`resolve_dispatch`](super::resolve_dispatch)（模块私有，无法作为 rustdoc
/// 链接目标）文档同一段：所有者索引因调用点而异（同一次攻击里攻击方
/// 与防御方各查各的），能被打包的只有「表」这一半，而只打包一半只会
/// 换来一个既不完整也不好读的中间类型。
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_use_skill(
    world: &WorldState,
    actor: EntityId,
    skill: ContentIndex,
    target: Option<EntityId>,
    skills: &dyn SkillCatalog,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
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
        && !granted_skills(
            &agent_trait_sources(agent, race_traits, class_traits, subclass_traits),
            agent.level,
            traits,
        )
        .contains(&skill)
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
            if resource_pool_usable(
                agent,
                pool,
                race_traits,
                class_traits,
                subclass_traits,
                traits,
            ) < i64::from(amount)
            {
                return Vec::new();
            }
        }
        ResourceCost::SlotTier(pool, min_tier) => {
            if find_available_slot_tier(
                agent,
                pool,
                min_tier,
                race_traits,
                class_traits,
                subclass_traits,
                traits,
            )
            .is_none()
            {
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
            if let Some(tier) = find_available_slot_tier(
                agent,
                pool,
                min_tier,
                race_traits,
                class_traits,
                subclass_traits,
                traits,
            ) {
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
pub(super) fn current_resource(
    agent: &ll_world::entity::Agent,
    kind: crate::skill::ResourceKind,
) -> i64 {
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
pub(super) fn resource_pool_usable(
    agent: &ll_world::entity::Agent,
    pool: ContentIndex,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> i64 {
    let stored = agent.resource_pools.get(&pool).copied().unwrap_or(0);
    let cap = effective_scalar_capacity(
        &agent_trait_sources(agent, race_traits, class_traits, subclass_traits),
        agent.level,
        pool,
        traits,
    );
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
pub(super) fn find_available_slot_tier(
    agent: &ll_world::entity::Agent,
    pool: ContentIndex,
    min_tier: u8,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Option<u8> {
    for tier in min_tier..=u8::MAX {
        let capacity = effective_slot_tier_capacity(
            &agent_trait_sources(agent, race_traits, class_traits, subclass_traits),
            agent.level,
            pool,
            tier,
            traits,
        );
        let spent = agent.spent_slots.get(&(pool, tier)).copied().unwrap_or(0);
        if spent < capacity {
            return Some(tier);
        }
    }
    None
}
