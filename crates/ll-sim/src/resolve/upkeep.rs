//! `resolve::upkeep`：不做事的那两族意图——等待与休息，以及休息完成时的恢复结算。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::ident::ContentIndex;
use ll_world::entity::{Agent, EntityId};
use ll_world::state::WorldState;

use crate::effect::Effect;
use crate::resource_pool::{
    RegenRule, ResourcePoolCatalog, ResourcePoolShape, RestRecoveryAmount,
    effective_scalar_capacity,
};
use crate::timeline::action_cost;
use crate::traits::{TraitCatalog, TraitGrantSource, agent_trait_sources, effective_traits};

use super::progression::restore_slots_from_lowest_tier;
use super::stats::effective_speed_from_dexterity;
use super::{BASE_ACTION_COST, schedule_after};

/// 原地等待一回合：消耗基础代价；若发起者正在休息
/// （`resource-pools-and-rest.md` 七、八节），额外检查这次行动结束时
/// 是否已到达 `target_ticks`——到达则先追加恢复批次再清空休息状态，
/// 否则休息状态原样保留（继续休息，不产生任何 resting 相关效果）。
///
/// # 完成判据：`world.clock + 本次行动耗时 >= started_at + target_ticks`
///
/// 与设计文档七节原文一致——判断的是「这一步等待做完之后」是否已经
/// 到达目标时刻，不是「这一步开始时」，理由同 [`resolve_use_skill`](super::progression::resolve_use_skill)
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
pub(super) fn resolve_wait(
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
                class_traits,
                subclass_traits,
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
/// `#[allow(clippy::too_many_arguments)]`：多出来的那一个是副职天赋
/// 接线批次新增的第三路天赋来源（`subclass_traits`）。它与
/// `race_traits`/`class_traits` 是并列的同一类依赖，打包成一个中间
/// 类型只会在这条转发链上多一层拆包——理由同本文件其余几处同款豁免。
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_rest(
    world: &WorldState,
    actor: EntityId,
    target_ticks: u32,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    if agent.resting.is_some() {
        return resolve_wait(
            world,
            actor,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
            pools,
        );
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
/// 与 [`resolve_resource_pool_regen`](super::progression::resolve_resource_pool_regen)（`OnTurnStart`）刻意不同——那里
/// 每条命中的授予声明各自贡献一次固定恢复量，多个来源各自独立叠加是
/// 正确语义（该函数文档「为什么按每条命中的授予声明」一节）。`OnRest`
/// 不同：`RestRecoveryAmount::Full` 只有相对**这个池的总容量**才有
/// 意义（不存在"这一条授予声明各自的满"这种概念），因此这里先按池去重，
/// 对每个池只查询一次总容量、只产出一批恢复效果，不会因为同一个池被
/// 两条天赋各自授予容量就重复产出两次"回满"。
pub(super) fn rest_completion_effects(
    agent: &Agent,
    actor: EntityId,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
) -> Vec<Effect> {
    let mut seen_pools: Vec<ContentIndex> = Vec::new();
    let mut effects = Vec::new();
    for trait_id in effective_traits(
        &agent_trait_sources(agent, race_traits, class_traits, subclass_traits),
        agent.level,
    ) {
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
                    if let Some(effect) = scalar_rest_effect(
                        agent,
                        actor,
                        grant.pool,
                        amount,
                        race_traits,
                        class_traits,
                        subclass_traits,
                        traits,
                    ) {
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
/// `#[allow(clippy::too_many_arguments)]`：多出来的那一个是副职天赋
/// 接线批次新增的第三路天赋来源（`subclass_traits`）。它与
/// `race_traits`/`class_traits` 是并列的同一类依赖，打包成一个中间
/// 类型只会在这条转发链上多一层拆包——理由同本文件其余几处同款豁免。
#[allow(clippy::too_many_arguments)]
pub(super) fn scalar_rest_effect(
    agent: &Agent,
    actor: EntityId,
    pool: ContentIndex,
    amount: RestRecoveryAmount,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Option<Effect> {
    let delta = match amount {
        RestRecoveryAmount::Full => {
            let capacity = effective_scalar_capacity(
                &agent_trait_sources(agent, race_traits, class_traits, subclass_traits),
                agent.level,
                pool,
                traits,
            );
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
pub(super) fn tiered_slot_rest_effects(
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
