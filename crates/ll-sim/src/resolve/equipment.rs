//! `resolve::equipment`：装备槽与消耗品：穿上、脱下、用掉。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_world::entity::EntityId;
use ll_world::history::KillCause;
use ll_world::state::WorldState;

use crate::effect::Effect;
use crate::item::{EquipSlot, ItemCatalog, conflicting_anchors, equip_mask_of};
use crate::skill::SkillEffect;
use crate::timeline::action_cost;

use super::inventory::merge_into_inventory_effect;
use super::stats::effective_speed_from_dexterity;
use super::{BASE_ACTION_COST, schedule_after};

/// [`Intent::Equip`](crate::intent::Intent::Equip) 结算（装备栏位批次，P6 第三批）：把 `actor` 背包
/// 里第一条匹配 `def` 的堆装备起来，落地
/// `knowledge/design/equipment-slots.md`「装备流程」一节——
/// 「一条规则覆盖所有特例」：装备时找出**全部**与新物品掩码相交的
/// 已装备物品,逐一卸下（写回背包）,再把新物品写入它的锚点槽位。
///
/// # 静默无效的三种情形
///
/// `actor` 不存在、背包里没有匹配 `def` 的堆、`def` 不可装备
/// （`items` 查不到这条物品的规则，或查到但 `equip_mask ==
/// SlotMask::EMPTY`）——与 [`resolve_pick_up`](super::inventory::resolve_pick_up)/[`resolve_drop`](super::inventory::resolve_drop) 同一条
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
pub(super) fn resolve_equip(
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
    let Some(anchor) = new_mask.anchor_slot() else {
        // 空掩码（不可装备）——`anchor_slot` 对 `SlotMask::EMPTY`
        // 返回 `None`，两道门合成一道。
        return Vec::new();
    };

    let mut effects = Vec::new();
    // 「什么算占位冲突」只有一个定义，与世界生成期的
    // `ll_sim::item::outfit_from_inventory` 共用，见
    // `crate::item::conflicting_anchors` 文档。
    for existing_anchor in conflicting_anchors(&agent.equipment, new_mask, items) {
        let existing_stack = agent.equipment[&existing_anchor];
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

/// [`Intent::Unequip`](crate::intent::Intent::Unequip) 结算（装备栏位批次，P6 第三批）：卸下玩家请求
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
/// [`resolve_drop`](super::inventory::resolve_drop) 同一条纪律。查不到某条已装备物品自身规则时按
/// `SlotMask::EMPTY` 处理（视为不覆盖任何槽位），理由同 [`resolve_equip`]
/// 「占位冲突」一节同一段说明。
pub(super) fn resolve_unequip(
    world: &WorldState,
    actor: EntityId,
    slot: EquipSlot,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };

    let found = agent
        .equipment
        .iter()
        .find(|(_, stack)| equip_mask_of(stack.def, items).contains_slot(slot));
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

/// [`Intent::Use`](crate::intent::Intent::Use) 结算（耐久与 `Intent::Use` 落地批次，P6 第五批）：
/// 消耗 `actor` 背包里第一条匹配 `def` 的堆一个单位，产出它的
/// `use_effect`（[`crate::item::ItemRule::use_effect`]，复用
/// [`SkillEffect`]，见其文档「为什么复用 `SkillEffect`」一节）对应的
/// `Effect`——`match` 分支与 [`resolve_use_skill`](super::progression::resolve_use_skill) 对同一个
/// `SkillEffect` 的三个变体逐字对应，唯一的区别是本函数没有冷却/资源
/// 消耗两道门（物品的"触发条件"是数量/耐久，不是冷却/资源，见
/// `ll_sim::item::ItemRule::use_effect` 文档同一节）。
///
/// # 目标恒为发起者自身
///
/// 与 [`Intent::Use`](crate::intent::Intent::Use) 文档「为什么携带 def，不携带目标」一节同一条
/// 范围裁定：本批次的物品使用效果只施于使用者自己，没有「对着别人用
/// 一件消耗品」的真实场景需要表达。
///
/// # 静默无效的三种情形
///
/// `actor` 不存在、背包里没有匹配 `def` 的堆、`def` 查不到物品规则或
/// 查到但 `use_effect` 是 `None`（材料、装备本身……不能被使用）——与
/// [`resolve_drop`](super::inventory::resolve_drop)/[`resolve_equip`] 同一条「静默无效，不是错误」
/// 纪律。
pub(super) fn resolve_use_item(
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
    let Some(effect) = rule.use_effect else {
        return Vec::new();
    };

    let mut effects = vec![Effect::ConsumeInventoryItem {
        actor,
        def,
        durability: stack.durability,
    }];

    match effect {
        SkillEffect::DealDamage { base } => {
            effects.push(Effect::Damage {
                target: actor,
                amount: base,
            });
            // 是否致死是规则判断，必须在这里做出——与 resolve_attack/
            // resolve_use_skill 完全同一条纪律（见 resolve_attack 文档）。
            // 用 KillCause::Environmental(def) 归因：一件伤害类消耗品
            // 不是近战也不是技能，是"本体死因枚举五个既有变体都覆盖
            // 不到，走注册表标注"的既有 mod 扩展死因通道，见
            // `ll_world::history::KillCause::Environmental` 文档。
            if agent.health - base <= 0 {
                effects.push(Effect::Kill {
                    target: actor,
                    killer: Some(actor),
                    cause: KillCause::Environmental(def),
                });
            }
        }
        SkillEffect::RestoreResource { resource, base } => {
            effects.push(Effect::AdjustResource {
                actor,
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
                target: actor,
                attribute,
                delta: amount,
                expires_at: Tick(world.clock.0 + i64::from(duration_ticks)),
                // 来源是这件物品自身的 ContentIndex——与
                // resolve_use_skill 传技能自身索引同一条既有纪律（见其
                // 文档），供 apply 判断"是不是同一件物品重复施加"。
                source: def,
            });
        }
    }

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
