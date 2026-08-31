//! `resolve::combat`：攻击结算，以及一次击杀之后的全部善后：经验、任务进度、史册记录、尸体掉落。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::ident::ContentIndex;
use ll_world::entity::{AttributeKind, EntityId};
use ll_world::history::KillCause;
use ll_world::state::WorldState;

use crate::check::{CHECK_DICE, CRITICAL_CHECK, CheckSide, SNEAK_ATTACK_CHECK, opposed_check};
use crate::combat::{
    Penetration, apply_crit_multiplier, crit_attacker_modifier, damage_after_defense,
    sneak_attacker_modifier,
};
use crate::damage_category::DamageCategoryCatalog;
use crate::effect::Effect;
use crate::experience::ExperienceCatalog;
use crate::exposure::AmbientSource;
use crate::formula::{DamageFormulaCatalog, FormulaInputs, attribute_modifier, eval_formula};
use crate::item::{EquipSlot, ItemCatalog, ItemStack, WearChannels};
use crate::quest::QuestCatalog;
use crate::rule_modifier::{
    agent_rule_modifiers, check_reroll_value, check_roll_bias, damage_after_resistance,
    resistance_damage_reduction, sneak_attack_rule, vulnerability_damage_increase,
};
use crate::timeline::action_cost;
use crate::traits::{TraitCatalog, TraitGrantSource};

use super::stats::{derive_stats_at, effective_speed_from_dexterity};
use super::{
    ARMOR_DURABILITY_LOSS_PER_HIT, BASE_ACTION_COST, WEAPON_DURABILITY_LOSS_PER_ATTACK,
    schedule_after,
};

/// 击杀产出经验的接线：若 `effects` 里包含 [`Effect::Kill`] 且
/// `killer` 已知，读取（结算前仍然存在的）被击杀目标的
/// `creature_kind`/`race`（与 [`Effect::IncrementKillCount`] 完全同一
/// 个归并键，见 `append_kill_history` 文档），查询 `experience` 目录
/// 拿到**基准值**，再连同击杀双方的等级交给
/// [`crate::experience::kill_experience`] 算出最终经验，追加一条
/// [`Effect::GrantExperience`]。
///
/// # 无条件追加，不再有「零经验就不产出」这一档
///
/// 项目所有者裁定「有个最低经验 1xp」——`kill_experience` 恒返回正
/// 数，因此每一次 `killer` 已知的击杀都恰好产出一条效果。此前那句
/// `if amount > 0` 是「基准值就是最终值」时代的产物，现在删掉不是
/// 放松判据，而是那个判据永远为真了。
///
/// # 死者的等级从哪来：`world.actors`，此刻它还活着
///
/// `knowledge/design/level-and-experience-system.md` 五节曾**否决**
/// 「按死者自身 `level` 计算经验」，理由是薄层 `ThinPopulation` 没有
/// per-instance 等级列。那条理由在本函数这里不成立，而且不是被绕开
/// 的：[`Effect::Kill`] 的 `target` 是一个 `EntityId`，指向的是
/// `world.actors` 这个**厚层**竞技场——薄层背景 NPC 根本不在其中，
/// 一个薄层实体要被攻击就必须先升格成厚层 `Agent`（`ThinPopulation::
/// promote`），升格那一刻它就有了 `level` 字段。换句话说：能被
/// `Effect::Kill` 点名的死者，恒定是有等级的。该节的否决对「薄层不
/// 需要升格就能被杀」这个假设是对的，但那个假设在当前代码里不成立
/// ——`append_kill_experience` 自接线之初就在做 `world.actors.get(target)`
/// 这次查询。设计文档该节据此更新。
///
/// 死者查不到（理论上不该发生：本函数在 `apply` 之前运行）时跳过这
/// 一次击杀的经验，不猜一个默认等级。击杀者查不到时同样跳过——经验
/// 没有收件人。
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
pub(super) fn append_kill_experience(
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
            let slayer = world.actors.get(*killer)?;
            let kind = victim.creature_kind.unwrap_or(victim.race);
            let base_reward = experience.xp_reward_for(kind);
            Some(Effect::GrantExperience {
                target: *killer,
                amount: crate::experience::kill_experience(base_reward, slayer.level, victim.level),
            })
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
/// （[`resolve_with_skills_and_quests`](super::resolve_with_skills_and_quests)）现在对 `Intent::Attack` 与
/// `Intent::UseSkill` 都会调用本函数，理由见该处注释。
///
/// 必须在 `apply` 之前读取被击杀者的 `race`：本函数只接受
/// `&WorldState`（`resolve` 必须是纯函数，C1），此刻目标仍然存在于
/// `world.actors` 里，`Effect::Kill` 还没有被应用。
pub(super) fn append_quest_kill_progress(
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
pub(super) fn append_kill_history(world: &WorldState, effects: &mut Vec<Effect>) {
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

/// 死亡掉落（NPC 生命周期批次）：若 `effects` 里包含 [`Effect::Kill`]，
/// 读取（结算前仍然存在的）被击杀目标的 `pos`/`inventory`/`equipment`，
/// 只要两者合计非空，就把死者变成一具装着这些物品的尸体——落地项目
/// 所有者裁定「死亡后就会爆出身上所有的物品……尸体也会随着时间最后
/// 消失回收」。
///
/// # 必须在 `Effect::Kill` 之前读取
///
/// 与 [`append_kill_history`] 文档「必须排在对应的 Effect::Kill 之前」
/// 同一条时序依赖：`Effect::Kill` 应用后 `target` 会被
/// `Arena::despawn` 整体收走，`inventory`/`equipment` 随之物理消失
/// （见 `Agent::inventory`/`Agent::equipment` 字段文档「为什么是 Agent
/// 字段」一节——这正是本批次要修的隐患：死亡结算此前只有
/// `world.actors.despawn(target)` 一步，背包随实体静默消失）。本函数
/// 因此必须在 `Effect::Kill` 仍然指向一个存在于 `world.actors` 的
/// 实体这一刻读出这两个字段，`resolve` 只有 `&WorldState`（C1），无法
/// 先移除背包再产出效果，只能把已经读到的物品原样打包进
/// [`Effect::AddGroundItem`]。
///
/// # 尸体不再是容器——尸体与遗物平铺进同一格（尸体平铺批次）
///
/// **这一节推翻了本函数此前的形状。** 此前一次死亡产出**一条**
/// `Effect::AddGroundItem`：尸体这件"壳"当 `stack`，死者的家当塞进
/// `contents`。那条形状撞上一个死结——
/// [`resolve_pick_up`](super::inventory::resolve_pick_up) 把 `contents` 非空的地面物品**整体排除**在拾取
/// 之外，于是生产路径上的尸体**根本捡不起来**，
/// `ll_mod::corpse_item::CORPSE_STACK_LIMIT` 至今只是一条诚实的声明。
///
/// 项目所有者的解法：
///
/// > 尸体会变成物品，然后原本的物品和尸体都会放在一格子内的掉落物
/// > 列表里。
///
/// 本函数因此改为产出 **1 + N 条**：尸体自己一条（`contents` 恒空），
/// 死者的每一堆遗物各一条，全部落在同一个 `victim.pos`。死结当场解开
/// ——尸体的 `contents` 为空 ⇒ 不再被那道容器排除挡住 ⇒ 可拾取、可
/// 堆叠，`CORPSE_STACK_LIMIT` 第一次真的生效。
///
/// **物品总数不变**：那些遗物本来就已经在世界状态里（在 `contents`
/// 这个 `Vec` 里），平铺只是把它们从嵌套一层挪到顶层。
///
/// `GroundItemStack::contents` 这个字段**不删**——箱子才是它将来的
/// 正经消费者，新分工写在该字段自己的文档里。[`resolve_loot`](super::inventory::resolve_loot)/
/// `Intent::Loot` 同样保留，但**今天没有任何生产路径会造出容器**。
///
/// # 空手死者**也**产出尸体（本批次改变了行为）
///
/// 旧行为是 `inventory`/`equipment` 合计为空时不追加任何效果，理由
/// 原文是「`contents` 非空是『这是一具容器』的唯一判据……一具打不出
/// 任何东西的尸体没有玩法意义（`resolve_loot`/`resolve_pick_up` 都不会
/// 把它当作合法目标）」。
///
/// **那条理由被本批次自己作废了**：平铺之后 `resolve_pick_up` 就是
/// 尸体的合法目标，一具空手死者的尸体是一件正常的、捡得起来的物品，
/// 有名字、有重量、有堆叠上限。守卫因此去掉，每一次死亡都产出尸体
/// ——这也让所有者那句「尸体会变成物品」在**所有**死亡路径上成立，
/// 而不只是「死者身上恰好有东西」的那一半。
///
/// 代价如实记录：`ground_items` 会比此前多——每个空手死者多一条。
/// 老化清理（[`ll_world::state::WorldState::cleanup_aged_ground_items`]）
/// 照常收，与普通丢弃物同一条通道，不需要给尸体单独发明第二套计时。
///
/// # 遗物的归属：原样搬移，不重置也不改写
///
/// 每一堆遗物的 [`owner`](ll_world::item::ItemStack::owner) 跟着
/// `ItemStack` 整体搬到地上，本函数一个字都不碰它。生产路径上它今天
/// 恒是 [`Owner::Unowned`](ll_world::ownership::Owner::Unowned)——
/// NPC 的出生装备与名册物品都是无主的。
///
/// **为什么不改写成 `Owner::Npc(死者)`**（那条路技术上走得通：死亡
/// 路径正是 `remembered_id_of_or_assign` 今天唯一的真实调用点，死者
/// 恒有 `remembered_id`）：
///
/// 1. **那会改变行为**。今天从尸体上搜刮遗物零权限检查；判成死者的
///    私产，会让「战场搜刮」在盗窃判定落地的那一刻**一次性**变成
///    盗窃——那是一条所有者没有裁定过的玩法规则（战利品权利、继承），
///    不该由本批次夹带。
/// 2. 设计文档
///    `knowledge/design/ownership-and-crime-detection.md` 1.5 把「野外
///    掉落」与「怪物尸体」并列为 `Unowned` 的两个典型场景，遗物落在
///    同一格、同一次结算里产出，与尸体同判是唯一自洽的选择。
/// 3. **最容易反转**：真要改成「死者的遗物仍属死者」，是这里一行。
///
/// 尸体本身恒 `Unowned`（[`ItemStack::new`] 的默认），设计文档 1.5
/// 直接点名了「怪物尸体」。
///
/// # 尸体的 `def`：复用死者的 `creature_kind`/`race`，不新开一张
/// # 尸体的 `def`：查 [`ItemCatalog::corpse_of`]，**不再是种族索引本身**
///
/// **这一整节推翻了本函数此前的文档。** 旧文档论证「`ll-sim` 不能依赖
/// `ll-mod`（规格 §5），拿不到注册表去 `intern` 一个尸体物品；而『区分
/// 哥布林尸体与人类尸体』当时没有真实消费场景（YAGNI）」，于是把
/// `victim.creature_kind.unwrap_or(victim.race)` 这个**种族**索引直接
/// 塞进了 [`ItemStack::def`]——那个字段本该是**物品**索引。
///
/// 那段论证在两处上是错的：
///
/// 1. **它不只是名字难看。** 所有者实机在交互列表里看到的
///    `#103 x1（搜刮）` 只是最表层的症状；真正的后果是凡是下游要查
///    `ItemDef` 的地方对尸体**全部静默落空**——没有重量、没有堆叠上限、
///    没有耐久、没有标签。这不是 YAGNI，是一次类型混淆。
/// 2. **跨 crate 依赖不是唯一出路。** 依赖倒置早就摆在这条调用链上了：
///    `resolve_dispatch` 手里现成有一个 [`ItemCatalog`]。给它加一条
///    `corpse_of` 查询（带默认实现 `None`，见其文档）就够了，一个新
///    trait 都不用开。
///
/// 所有者裁定：**「尸体也是一件可堆叠的物品才对」**。落地形状见
/// `ll_mod::corpse_item` 模块文档：全部 mod 装载完之后，**每个种族**
/// 自动获得一件真正的尸体 `ItemDef`（可堆叠、有重量、名字走 i18n 且
/// 带物种），内容作者不写一个字。
///
/// ## 归并键一个字没改
///
/// `victim.creature_kind.unwrap_or(victim.race)` 原样保留——它是四条
/// 路径里三条的既有惯例（见 [`Effect::IncrementKillCount`] 文档「为什么
/// 按 `kind: ContentIndex`」一节）。改的只是「拿这个键去查什么」。
///
/// ## 查不到尸体物品时退回旧行为，不是不产出
///
/// 两种情形会查不到：目录侧没有实现 `corpse_of`（[`NoItems`](crate::item::NoItems) 与大量
/// 只关心「查一条规则」的测试夹具，默认实现恒 `None`），或者
/// `creature_kind` 指向的**不是一个种族**（那个字段是裸
/// [`ContentIndex`]，至今没有「生物种类表」）。
///
/// 这时按归并键原样产出，与本次改动之前**逐位相同**。选这条兜底而不是
/// 「不产出尸体」，是因为尸体是死者**全部遗物**的唯一容器：查不到一条
/// 物品定义就把遗物一起吞掉，是拿一个呈现层的缺失去毁掉一份世界状态。
/// 退化的只是「这具尸体查得到多少字段」，不是「死者的东西还在不在」。
/// 这也让本次改动在没有真实内容的测试世界里是**零行为变更**——两条
/// 黄金基准因此不受影响。
///
/// `stack.durability` 恒 `None`——尸体这件"容器"本身没有耐久概念，与
/// [`ItemStack::new`] 材料/消耗品的既有语义一致，也与
/// `ll_mod::corpse_item` 给尸体 `ItemDef` 填的 `max_durability: None`
/// 对得上。
///
/// # 两具尸体现在真的会被合并
///
/// **这一节推翻了本函数此前的文档。** 旧文档说 `CORPSE_STACK_LIMIT`
/// 「今天还观察不到……不是一条现在就在跑的逻辑」——那是因为尸体恒被
/// [`resolve_pick_up`](super::inventory::resolve_pick_up) 的容器排除挡住。平铺之后那道排除对尸体不再
/// 生效：两具同物种的尸体 `def` 相同、`durability` 同为 `None`、
/// `owner` 同为 `Unowned`，[`can_merge`](crate::item::can_merge)
/// 三项全等 ⇒ 可合并，玩家捡起两具哥布林尸体，背包里就是一堆
/// `x2`（上限 8）。
///
/// 这条声明第一次真的在跑。
pub(super) fn append_corpse_drop(
    world: &WorldState,
    effects: &mut Vec<Effect>,
    items: &dyn ItemCatalog,
) {
    let drops: Vec<Effect> = effects
        .iter()
        .filter_map(|effect| {
            let Effect::Kill { target, .. } = effect else {
                return None;
            };
            let victim = world.actors.get(*target)?;
            // 归并键一个字没改，改的是拿它去查什么——见本函数文档
            // 「尸体的 `def`」一节。查不到就退回旧行为（用归并键本身）。
            let corpse_kind = victim.creature_kind.unwrap_or(victim.race);
            let corpse_def = items.corpse_of(corpse_kind).unwrap_or(corpse_kind);
            // 尸体自己一条：contents 恒空——它不再是容器，见本函数文档
            // 「尸体不再是容器」一节。
            let corpse = Effect::AddGroundItem {
                pos: victim.pos,
                stack: ItemStack::new(corpse_def, 1),
                dropped_at: world.clock,
                contents: Vec::new(),
                // 尸体是**躺**在地上的，不是被谁立起来的——它照常老化
                // （见 WorldState::cleanup_aged_ground_items），也挡不住
                // 别人往这一格丢东西。
                placed: false,
            };
            // 死者的每一堆遗物各一条，全落在同一个 victim.pos 上。
            // 归属原样搬移（不重置成 Unowned，也不改写成死者的）——见
            // 本函数文档「遗物的归属」一节。
            let loot = victim
                .inventory
                .iter()
                .chain(victim.equipment.values())
                .map(|stack| Effect::AddGroundItem {
                    pos: victim.pos,
                    stack: *stack,
                    dropped_at: world.clock,
                    contents: Vec::new(),
                    placed: false,
                })
                .collect::<Vec<_>>();
            Some(std::iter::once(corpse).chain(loot))
        })
        .flatten()
        .collect();
    effects.extend(drops);
}

/// 直接攻击一个已知目标（与 [`resolve_move`](super::movement::resolve_move) 的隐式派生分开的显式路径，
/// 供已经知道目标的调用方——例如已锁定目标的 AI ——直接使用）。
///
/// 攻击力：攻击者的 [`derive_stats`](super::stats::derive_stats) 力量项（基础值 + 状态效果 + 装备
/// 三个来源汇总后的最终生效值，技能增益/削弱与武器加成由此接线生效）。
///
/// 防御：防御方的 [`derive_stats`](super::stats::derive_stats) 护甲——**P6 第四批：这是防御端第一
/// 次真的生效**，此前恒为占位的 `0`。护甲的唯一来源目前是防御方已装备
/// 物品的 [`crate::item::StatBonus`]（见 [`derive_stats`](super::stats::derive_stats) 文档「护甲不
/// 参与状态效果通道」一节）；没有任何已装备物品提供护甲时，
/// `derive_stats` 算出的护甲仍是 `0`，与本批次之前的占位行为等价。
///
/// # 武器引用：`Intent::Attack` 为什么不改签名（武器引用与穿透接线
/// 批次，P6 第六批）
///
/// 项目所有者裁定「`Intent::Attack` 肯定还是需要有武器引用的吧，不然
/// 怎么做其他计算呢」——本批次要把这条缺口接上，有两条路：
///
/// **甲**：给 `Intent::Attack` 加一个武器字段，调用方显式传入用哪件
/// 武器攻击。
///
/// **乙**：`Intent::Attack` 签名不变，本函数结算时自己从
/// `attacker.equipment` 查询主手槽位。
///
/// **本函数选择乙**：攻击者的装备从 P6 第三批起就已经存在于
/// `Agent.equipment`（`BTreeMap<EquipSlot, ItemStack>`，锚点槽位为键，
/// 见其文档），`derive_stats` 也已经在读这份数据算攻击力/护甲——"用哪
/// 件武器攻击"根本不是一个需要调用方现场决定、随每次 `Intent` 变化的
/// 输入，是"这个实体当前主手上挂着什么"这一条**已经存在于世界状态里**
/// 的事实，`resolve_attack` 只需要多读一遍同一份数据，不需要任何新的
/// 输入通道。选甲需要把仓库里全部构造 `Intent::Attack` 的调用点（本
/// 文件的测试、`ll-mod`/`ll-game` 的既有接线）都改成显式传武器引用，
/// 但那份引用在几乎所有调用点上其实就是"去查一下 `attacker.equipment`
/// 主手槽位"这同一个值——让调用方重复算一遍 `resolve_attack` 内部本来
/// 就要读的同一份状态，只会制造"调用方传的武器引用与其装备栏实际内容
/// 不一致"这一类新的不变式（这里的 `EntityId` 是谁，装备着什么，`Agent`
/// 自己已经如实记录，不需要外部输入再确认一遍）。
///
/// 若未来要支持"用背包里某件东西砸人"（不经过装备栏、临时抄起一件未
/// 装备的物品攻击）——那才是真正需要 `Intent::Attack` 携带显式武器
/// 引用的场景，因为"用什么打"在那种手感下不再等于"当前装备着什么"，
/// 两者会分道扬镳。本批次没有这个需求（`knowledge/design` 未点名，
/// 也没有任何调用点要这个手感），届时再给 `Intent::Attack` 加一个
/// `Option<ContentIndex>` 字段（`None` 表示"用当前装备的武器"，与
/// 现在的行为向后兼容）即可，不需要现在为一个不存在的场景预留字段。
///
/// # 穿透：攻击者主手武器的 [`crate::item::ItemRule::penetration`]
///
/// 此前（P6 第四批到第五批）本函数恒传 [`Penetration::NONE`]——`ItemRule`
/// 不携带穿透字段，`Intent::Attack` 也不携带武器引用，两个缺口叠在
/// 一起使得穿透没有任何数据源。本批次同时补上了这两点（见上方「武器
/// 引用」一节与 [`crate::item::ItemRule::penetration`] 文档），穿透因此
/// 第一次真正生效：查询攻击者主手槽位的 `ItemStack`，用它的 `def` 向
/// `items` 目录要 [`crate::item::ItemRule::penetration`]；主手为空
/// （徒手）或 `items` 查不到这个 `def` 时按 [`Penetration::NONE`]
/// 处理——理由同 `derive_stats` 查不到目录时的既有纪律（不伪造数据）。
/// 已损坏（耐久归零）的武器不提供穿透，与 `derive_stats` 对属性加成
/// 的「耐久归零即跳过」是同一条纪律（见其文档「耐久归零：损坏的装备
/// 不再贡献属性加成」一节）——护甲加成与穿透都是"这件装备当前有没有
/// 在正常发挥作用"的表现，不该有一个归零后失效、另一个归零后照常。
///
/// 若这一下会让目标生命值降到零或以下，额外产出一个 [`Effect::Kill`]
/// ——是否致死是规则判断，必须在这里（`resolve`）做出，`apply` 只管
/// 照数字做加减（见 [`crate::effect::Effect::Damage`] 文档）。
///
/// # 耐久消耗：两条通道，判据是标签（耐久标签批次）
///
/// 项目所有者的裁定分两步走到今天。第一步推翻了「只有装备武器才有
/// 耐久」：
///
/// > 「衣服要耐久，受到攻击就会减少耐久。」
/// > 「修理锤子也算是一种武器，也可以是带有功能性的物品。只要使用就
/// > 会减少耐久。」
///
/// 第二步推翻了本函数**上一版按槽位分类**的做法。上一版把防御方的
/// 已装备物品按存储键分成「武器组（主手/副手）」与「其余」，只让后者
/// 挨打掉耐久。所有者指出这个判据本身是错的：
///
/// > 「副手也可能拿着武器,例如双刀,双盾」
///
/// **副手不等于盾**——双持匕首时副手是武器，双盾时两只手都是盾。
/// 槽位回答的是「这件东西挂在哪」，回答不了「这件东西是什么」。所有者
/// 给出的表达方式是标签：
///
/// > 「每个物品可以有个标签的列表,带有多个标签」
///
/// 判据因此改成**按物品是什么**，不是按它在哪个槽位：
///
/// | 通道 | 判据 | 谁磨损 | 每次多少 |
/// |---|---|---|---|
/// | **使用** | 物品带 `on-use` 标签 | 攻击方**主手**的武器 | [`WEAPON_DURABILITY_LOSS_PER_ATTACK`] |
/// | **挨打** | 物品带 `on-hit` 标签 | 防御方**每一件**已装备物品 | [`ARMOR_DURABILITY_LOSS_PER_HIT`] |
///
/// 「带某个标签」在结算侧读的是
/// [`crate::item::ItemRule::wear_channels`]——由这件物品的全部标签在
/// **注册期**折算好的位掩码（ADR 0016/0017：注册期物化、运行期查表），
/// 不是运行期遍历标签列表现算，完整论证见该字段文档。
///
/// 两条通道都仍然只作用于**带耐久的堆**
/// （`ItemStack.durability.is_some()`）：标签回答「这类东西会不会磨损」,
/// `durability` 回答「这一件具体还有多少」，两个问题都要成立才产出
/// 效果。徒手、或穿着没有耐久概念的物品时，本函数一条效果都不产出。
///
/// ## 两条通道现在**可以**重叠——这是刻意的
///
/// 上一版有一条「两组槽位刻意不重叠，没有任何一件装备被两条规则同时
/// 收费」的不变量。**本批次明确推翻它**，理由是所有者原话：
///
/// > 「有的技能像是盾击,他也会变成武器这样」
///
/// 一面既用来砸人又用来挡刀的盾，两条通道都该收费——给它同时挂上
/// 武器标签与防具标签即可（[`crate::item::ItemRule::wear_channels`]
/// 是并集）。上一版担心的「对砍时武器以护甲两倍速报废」不受影响：
/// 一把剑只带武器标签，压根进不了挨打通道；会两头磨损的只有内容作者
/// **明确声明**它两头都用的东西。
///
/// ## 为什么全部已装备物品一起扫，不挑一件
///
/// 「挑一件」要么掷骰（约束 C3/C5 又多一条随机流，且给回放添一处随机
/// 噪声），要么定一个任意的优先级顺序（"先磨外套还是先磨头盔"没有任何
/// 设计依据）。全部各扣一点是确定性的，且与"这一下打在身上"的直觉
/// 相容。代价是穿得越多磨损总量越大，但那是一个合理的权衡（护甲多 =
/// 减伤多 = 维护成本高），不是需要抵消的缺陷。
///
/// 遍历顺序由 `equipment`（`BTreeMap`）决定，确定（约束 C5）。
///
/// ## 这一步为什么可以查目录
///
/// 判据从「读存储键」变成「查 `items.item(def).wear_channels`」，每件
/// 已装备物品因此多一次目录查询。这与 ADR 0016/0017「结算是热路径」
/// 不冲突：[`derive_stats_at`] 本来就对**同一批**已装备堆逐个做同样的
/// `items.item(stack.def)` 查询（为了读 `stat_bonuses`），本函数在同一
/// 次攻击里做的是同一量级、同一形状的事，不是新引入一类开销。真正被
/// 该 ADR 挡在门外的是"运行期遍历标签列表再逐个查标签表"，那一步已经
/// 在注册期做完了。
///
/// ## 伤害为零时照样磨损
///
/// 抗性免疫（乘数 0）或减伤把这一下打成 0 时，「挨打」通道**仍然**
/// 产出效果：判据是「这一下攻击结算成立、打在了身上」，不是「实际掉了
/// 血」。反过来做会让一条免疫天赋顺带附赠"护甲永不磨损"，那是两个
/// 系统之间一条没人设计过的隐藏耦合。
///
/// ## 与击杀的先后
///
/// 「挨打」通道的效果排在 [`Effect::Kill`] **之前**——`apply` 按顺序
/// 执行，`Kill` 会把实体收走（`world.actors.get_mut` 随后落空），耐久
/// 必须先写完。这与「潜行破除排在伤害之后」是同一类"效果顺序本身是
/// 设计决定"的既有考虑。
///
/// ## 归零之后
///
/// 本函数从不产出 [`Effect::Unequip`]：耐久归零的护甲继续占着槽位,
/// 只是不再贡献任何加成——**护甲值与保温值一并失效**，因为
/// [`derive_stats_at`] 的「耐久归零即 `continue`」是在读取
/// `stat_bonuses` **之前**跳过整条堆，三个 [`StatTarget`](crate::item::StatTarget) 变体
/// （`Attribute`/`Armor`/`Insulation`）没有任何一个能绕过它。一件穿
/// 破了的皮袄因此既不挡刀也不保暖，见 [`derive_stats`](super::stats::derive_stats) 文档「耐久归零」
/// 一节。
///
/// # 暴击：读取 `attacker_derived.attribute(AttributeKind::Luck)`（幸运并入
/// `AttributeKind` 批次）
///
/// 所有者原话（针对盗贼偷袭的裁定，本批次先落地最现成的一处）：「做成
/// 技能判定吧，通过幸运值之类的属性以及一定的随机值组合一下」——暴击
/// 正是「战斗结算里现成的、幸运能挂上去的判定点」（`combat.rs` 已有
/// `damage_after_defense` 这条主干，暴击只是在它算出的伤害上再判一次
/// 是否放大，不需要新开一条结算路径）。幸运通过
/// [`crate::combat::crit_attacker_modifier`] 换算成一次对抗判定里攻击
/// 者那一侧的骰子点数修正，输入是
/// `attacker_derived.attribute(AttributeKind::Luck)`——**派生值，不是裸
/// `attacker.stats.luck`**：幸运并入 `AttributeKind` 批次之前，幸运是
/// `Agent` 上不受装备/状态效果影响的独立字段，暴击只能读裸值；并入之后
/// 幸运戒指（[`crate::item::StatTarget::Attribute`]）、祝福术/诅咒
/// （[`ll_world::entity::ActiveStatModifier`]）都要能改变它，若这里继续
/// 读裸 `attacker.stats.luck`，装备/buff 加的幸运永远不会反映到暴击率
/// 上——那就白并了。`attacker_derived` 已经是 [`derive_stats`](super::stats::derive_stats) 汇总过
/// 基础值 + 状态效果 + 装备的结果（见本函数顶部），复用同一份派生结果，
/// 不重新算一遍。`attribute-system.md`「五、幸运」一节「幸运不直接加
/// 伤害，它改变随机判定的形状」原文在这里精确成立：幸运本身从不出现在
/// `damage` 的加法项里，只出现在「这次判定要不要放大伤害」这个概率里。
///
/// 随机数严格遵守约束 C3：必须走
/// `DetRng::for_entity(世界种子, 实体 ID, 事件计数)`，不得使用任何
/// 全局随机流。三元组取 `(world.seed, actor.as_u64(), world.clock.0)`
/// ——与 `ll_mod::script_behavior_source` 的 AI 决策随机流同一套取法
/// （行为树 tick 同样用 `(世界种子, 实体 ID, 当前世界时钟)`）。约束 C5
/// （取数顺序确定）在本函数里天然满足：整条 `resolve_attack` 只在这
/// 一处消费随机数，前面的攻击力/护甲/穿透/伤害计算全部是纯算术，不
/// 存在「先掷了别的骰子再掷这个」的顺序歧义。
///
/// # 暴击换成对抗判定（判定系统迁移批次）
///
/// 掷的不再是一枚「幸运 × 5‰」的硬币，而是一次**对抗判定**
/// （[`crate::check::opposed_check`]，`3d20 + 修正` 双方各一轮）：
///
/// ```text
/// 攻击者（主动）：暴击基准偏移 −23 + 自己的幸运点数
/// 被攻击者（被动）：自己的幸运点数
/// ```
///
/// 主动方**严格大于**被动方才算暴击。基准（双方幸运都取
/// `BaseStats::BASELINE.luck` = 0）暴击率因此是 `4.84%`——项目所有者
/// 裁定的 5% 基准在 `3d20` 这把钟形骰上最接近的那一格，完整推导（含
/// 三格精确组合数与「为什么钟形骰上写不出恰好 5%」）见
/// [`crate::combat::CRIT_BASE_CHECK_MODIFIER`] 文档。
///
/// **被攻击者的幸运真的参与**，不是一侧摆设：旧模型里被打的人是谁
/// 完全不影响这一下会不会暴击，那正是 [`crate::check`] 模块文档拿来
/// 论证盘查判定该换形状的同一条毛病（「一个眼神再好的卫兵与一个瞎子
/// 查同一个人，查到的东西逐位相同」）。幸运既然「改变随机判定的
/// 形状」，被人打在要害上也是一次针对你的随机判定。
///
/// **这条改动影响每一次攻击**：零幸运不再等于零暴击率（旧模型的
/// `chance(0, ..)` 恒假），因此黄金基准
/// （`crates/ll-sim/tests/replay.rs`）与既有确定性伤害断言都可能变，
/// 变没变、为什么变，逐条写在那个常量的文档与本批次提交信息里。
/// 这次判定消费的抽取次数也从 `1` 变成 `2M = 6`（含优劣势时 `4M`、
/// 含重掷时更多）——**不会让任何后续取数错位**：这条流是现场用
/// `DetRng::for_entity` 新造的、只服务这一次判定，伤害公式骰子流与
/// 偷袭流各有各的三元组（见下面两节），三条流互不相干。
///
/// 优劣势与重掷同样接上了：攻防两侧各按
/// [`crate::check::CRITICAL_CHECK`] 查
/// [`crate::rule_modifier::check_roll_bias`] 与
/// [`crate::rule_modifier::check_reroll_value`]，与盘查/藏匿两处判定
/// 逐字同构。没有任何来源声明这三条时两侧都是
/// [`crate::check::RollBias::Normal`] + 不重掷，取数次数恒为 `2M`。
///
/// # 伤害公式接线（伤害公式引擎批次）
///
/// 攻击力数值的来源从「恒读 `attacker_derived.attribute(AttributeKind::Strength)`」
/// 改为「查 [`DamageFormulaCatalog::formula_for`]，用武器显式声明的
/// 公式（[`crate::item::ItemRule::damage_formula`]，没有声明时退回
/// 全局默认公式）算出一个攻击力数值」——**`damage_after_defense` 本身
/// 不改一个字**：公式的输出只是替换了原先直接读取的那个标量，送进
/// 这条既有减伤链路的方式完全一样，见 `crate::formula` 模块文档「公式
/// 只算『攻击力』」一节。全局默认公式
/// （[`crate::formula::default_attack_power_instructions`]）是单条
/// `Ref(AttackPower)` 指令，原样把
/// `attacker_derived.attribute(AttributeKind::Strength)` 这个输入交回
/// 去——没有任何武器/技能声明公式时，本函数因此逐行复现接入公式引擎
/// 之前的既有行为，是「行为等价」测试要验证的核心承诺。
///
/// 骰子随机流（`FormulaOp::Dice`）与暴击判定各自独立——用
/// `world.clock.0` 异或一个不同于暴击事件计数的固定标签构造第二条
/// `DetRng` 流（约束 C3：三元组身份不同，两条流互不干扰；约束 C5：
/// 骰子取数顺序完全由公式编译产物的指令数组顺序决定，见
/// `crate::formula::eval_formula` 文档）。不含骰子的公式（含全局默认
/// 公式）永远不会调用这条流的任何方法,构造它本身没有可观测的副作用,
/// 因此"要不要构造"不需要按 `needs_rng` 分支特判,见
/// `FormulaDef::needs_rng` 文档。
///
/// # 偷袭接线（盗贼偷袭接线批次）
///
/// 所有者对「盗贼偷袭」的裁定原话：「盗贼偷袭做成技能判定吧，通过幸运
/// 值之类的属性以及一定的随机值组合一下」——`trait-system.md` 此前判定
/// 盗贼偷袭表达不了（真实条件「目标旁边有我的盟友」需要一次本项目不
/// 存在的空间查询），所有者的裁定绕开了这条依赖，改成只依赖攻击者自身
/// 幸运的判定。落地成 [`crate::traits::RuleModifier::SneakAttack`]——
/// 天赋效果而不是技能效果（`crate::skill::SkillEffect` 目前只有
/// `DealDamage` 一种变体，追加"条件触发的额外伤害"需要新增一个变体并
/// 改写 `resolve_use_skill` 的效果解释器；`RuleModifier` 已经是「战斗
/// 结算按变体读取」的既有机制，`RuleModifier::Resistance` 是现成的
/// 先例——挂进已有机制，不新开一条平行的技能效果通道，YAGNI）。
///
/// 查 [`sneak_attack_rule`]（`crate::rule_modifier`，消费
/// [`agent_rule_modifiers`] 汇总出的候选列表——攻击者的有效天赋与已装备
/// 物品两路来源，合并规则同 [`resistance_damage_reduction`]）：
/// 没有任何来源声明偷袭时返回 `None`，
/// 本函数完全不进入判定分支，不额外消费一条 `DetRng` 流——与「抗性
/// 接线」一节「没有天赋声明时逐位复现既有行为」是同一条「新增判定不
/// 改变没有相关天赋的角色的既有结果」纪律。
///
/// 有声明时走一次**对抗判定**（判定系统迁移批次；此前是一枚
/// 「幸运 × 每点敏感度‰」的硬币）：
///
/// ```text
/// 攻击者（主动，隐蔽）：幸运点数 + 天赋声明的 sneak_modifier
///                       +（潜行时再加一整颗骰子 19）
/// 被攻击者（被动，察觉）：意志调整值
/// ```
///
/// 攻击者**严格大于**才触发，触发则把天赋声明的固定 `extra_damage`
/// 加到伤害上。三项修正各自的出处见
/// [`crate::combat::sneak_attacker_modifier`]；「察觉 = 意志调整值」
/// 与藏匿判定（[`resolve_inspect`](super::inventory::resolve_inspect)）里盘查者那一侧是同一条所有者裁定，
/// 攻守位置互换的理由见 [`crate::check::SNEAK_ATTACK_CHECK`]。判定走
/// 独立的第三条 `DetRng` 流。挂载点在暴击放大之后、抗性乘数之前——
/// 追加的伤害仍然是这一下攻击的一部分，应当同样受目标抗性影响，不是
/// 绕开减伤链路凭空产出的独立效果。
///
/// # 潜行与偷袭（潜行与盗贼被动批次；判定系统迁移批次去掉「必定」）
///
/// 攻击者正处于潜行状态（[`ll_world::entity::Agent::stealthed`]）时，
/// 偷袭判定此前**直通**——跳过掷骰，直接吃到 `extra_damage`。那是一条
/// 「必定成功」，与项目所有者「不允许绝对」这条红线直接冲突。所有者
/// 裁定改成一次判定，原话是「就算是概率最小都可以」。
///
/// 落地方式是**把潜行从一条分支变成一个修正**（一整颗骰子，见
/// [`crate::combat::STEALTH_SNEAK_MODIFIER`]），与判定系统落地批次对
/// 盘查判定做的事逐字相同（「潜行不再换基数，它是隐蔽方的一个修正」）。
/// 玩法效果保住了——潜行 + 一条半颗骰子的被动天赋对基准目标是
/// `97.51%`；剩下的 `2.49%` **不是另加的钳制**，是修正上限
/// `|a − p| <= 2L <= S − 1 < S` 的直接后果，见 [`crate::check`] 模块
/// 文档「不允许绝对」一节。
///
/// 这条连接仍然刻意做在「已经有 `SneakAttack` 声明」的前提之内，不是
/// 「潜行本身就能偷袭」——两层是分开的（项目所有者
/// 「潜行和盗窃或许可以安排成盗贼主职业的一种被动技能 buff」这句话
/// 的落地方式）：**潜行这个动作人人都能做**（`Intent::ToggleStealth`
/// 不查任何职业/天赋），**把它变成实打实的伤害是天赋给的**（没有任何
/// 来源声明 `RuleModifier::SneakAttack` 的角色，潜行照样不会凭空多打
/// 一点伤害）。
///
/// # 潜行破除
///
/// 攻击者在潜行中打出这一下之后，本函数追加一条
/// `Effect::SetStealth { stealthed: false }`——**排在伤害之后**，因此
/// 这一下仍然吃到潜行给的那一整颗骰子修正，破除从下一次行动起才生效
/// （经典的「一次免费背刺」，只是不再保证一定刺得中）。
///
/// **受伤不破除**，这是本批次一次显式的裁定而不是遗漏：本批次的潜行
/// 不是隐身（FOV 一个字都没改，卫兵照常看得见你，见
/// `ll_script::api::actor` 模块文档），它只影响「要不要把你当回事」
/// 这次判定；自己动手打人是当事人主动做的一次公开动作，理应破除，而
/// 被别人打中不是当事人能选择的事——让任意第三方（未来的范围伤害、
/// 陷阱、掉落物）都能无代价剥掉一个角色的潜行，是一条项目所有者没有
/// 要求、且当前没有任何反制设计（重新潜行的代价/冷却）配套的规则。
/// 技术面也指向同一个结论：伤害的产出点不止本函数一处，把「受伤破除」
/// 做对要么散布到每一个伤害生产者，要么把一条规则判断塞进
/// `crate::apply`（ADR 0023/约束 C1 明确禁止 `apply` 做规则判断）。
/// 两条理由指向同一个选择，因此本批次不做；真要做，是「潜行的反制
/// 手段」那一批的工作，届时连同重新潜行的代价一起设计。
///
/// # 抗性接线（伤害类别/抗性接线批次；来源扩展见抗性多来源聚合批次）
///
/// `damage-formula-mod-api.md` 二十节把抗性的挂载点定死在「减伤之后」
/// ——本函数在 `damage_after_defense`（含暴击放大）算完之后最后一步，
/// 用这一下的伤害类别（武器显式声明的
/// [`crate::item::ItemRule::damage_category`]，没有声明时退回
/// [`DamageCategoryCatalog::default_category`]）查
/// [`resistance_damage_reduction`]（`crate::rule_modifier`，消费
/// [`agent_rule_modifiers`] 汇总出的**防御方**候选列表：有效天赋与已
/// 装备物品两路来源，抗性多来源聚合批次接上了后者），把查到的**减伤
/// 点数**从伤害上扣掉（[`damage_after_resistance`]）——没有任何来源
/// 声明抗性时点数恒为 `0`，本函数因此逐位复现接入抗性之前的既有行为,
/// 与「伤害公式接线」一节「全局默认公式」的「行为等价」承诺是同一条
/// 纪律的第二次应用。
///
/// 形式从该节原文的「乘数」改成了减法（flat DR），见该节末尾的更正段
/// 与 [`crate::rule_modifier::RuleModifier::Resistance`] 文档。挂载点
/// 一个字没变。
///
/// 「绝对免疫」在减伤模型下不再是一个可声明的状态：减伤不封顶，但一次
/// 本来打得出伤害的攻击减完至少还剩
/// [`crate::rule_modifier::MINIMUM_DAMAGE_AFTER_RESISTANCE`] 点。这条
/// 新下限与 `damage_after_defense` 内部那条 10% 下限仍然不是同一条,
/// 各自独立生效：10% 下限保护的是「减伤链路本身不会因为
/// 防御过高而系统性压制到零」，抗性回答的是「这种伤害对这个目标有没有
/// 意义」，见 `MINIMUM_DAMAGE_AFTER_RESISTANCE` 文档「这条下限是新增
/// 的，不是把 10% 下限平移过来」一节完整论证。
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_attack(
    world: &WorldState,
    actor: EntityId,
    target: EntityId,
    items: &dyn ItemCatalog,
    formulas: &dyn DamageFormulaCatalog,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    damage_categories: &dyn DamageCategoryCatalog,
    ambient: AmbientSource<'_>,
) -> Vec<Effect> {
    let Some(attacker) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(defender) = world.actors.get(target) else {
        return Vec::new();
    };

    // 环境温度按**各自所在的空间**分别查（温度系统批次）：攻防双方
    // 完全可能一个站在暴风雪里、一个站在屋檐下，用同一个温度会让「进
    // 屋躲一躲」这条规避路径对被攻击的一方失效。`AmbientSource::NONE`
    // 时两次查询都返回中性温度，与温度这一路没接线逐位等价。
    let attacker_ambient = ambient.temperature_in(world, &attacker.current_space);
    let defender_ambient = ambient.temperature_in(world, &defender.current_space);
    let attacker_derived = derive_stats_at(
        attacker.stats,
        &attacker.active_stat_modifiers,
        &attacker.equipment,
        items,
        world.clock,
        attacker_ambient,
    );
    let defender_derived = derive_stats_at(
        defender.stats,
        &defender.active_stat_modifiers,
        &defender.equipment,
        items,
        world.clock,
        defender_ambient,
    );

    let attack_power_input = attacker_derived.attribute(AttributeKind::Strength);
    // 武器：攻击者主手槽位当前装备的物品——见本函数文档「武器引用」
    // 一节，选择乙（结算时查装备栏，不改 `Intent::Attack` 签名）。
    let weapon = attacker.equipment.get(&EquipSlot::MAIN_HAND);
    let weapon_def = weapon.map(|stack| stack.def);
    // 已损坏的武器既不提供穿透、也不提供显式公式引用——见本函数文档
    // 「穿透」一节,伤害公式与穿透走同一条"损坏即失效"的既有纪律。
    let weapon_rule = weapon
        .filter(|stack| stack.durability != Some(0))
        .and_then(|stack| items.item(stack.def));
    let penetration = weapon_rule
        .as_ref()
        .map(|rule| rule.penetration)
        .unwrap_or(Penetration::NONE);
    let explicit_formula = weapon_rule.as_ref().and_then(|rule| rule.damage_formula);

    // 攻防双方的规则修正各聚合**一次**，本函数下游全部消费者共用同一
    // 份候选列表——暴击判定的优劣势/重掷、偷袭声明、抗性与易伤读的都是
    // 同一个实体、同一时刻的同一批声明，聚合多次只会多走几遍完全相同
    // 的遍历（`agent_rule_modifiers` 是纯函数,见其文档「热路径」一节）。
    let attacker_modifiers = agent_rule_modifiers(
        attacker,
        race_traits,
        class_traits,
        subclass_traits,
        traits,
        items,
    );
    let defender_modifiers = agent_rule_modifiers(
        defender,
        race_traits,
        class_traits,
        subclass_traits,
        traits,
        items,
    );

    // 暴击判定（幸运并入 AttributeKind 批次；判定系统迁移批次换成
    // 对抗判定）：两侧的幸运都读 `attribute(AttributeKind::Luck)`——
    // 派生值，装备/状态效果加的幸运在这里生效，见本函数文档「暴击」
    // 一节。约束 C3——随机性必须走
    // `DetRng::for_entity(世界种子, 实体 ID, 事件计数)`，这里用攻击者
    // 自己的实体 ID 与当前世界时钟作三元组的后两项，与
    // `ll_mod::script_behavior_source` 的 AI 决策随机流同一套取法
    // （见其文档「C3」一节）；约束 C5——这条流是现场构造、只服务这
    // 一次判定，取数顺序由 `opposed_check` 的固定程序顺序定死（先主动
    // 方 M 颗、后被动方 M 颗，见 `crate::check` 模块文档「取数纪律」），
    // 不存在排列组合问题。判定挪到公式求值**之前**（此前挪到公式
    // 求值之后）——公式的 `Crit` 操作数需要这个结果作为输入,但这
    // 只是「谁先计算」的顺序调整,不改变这次判定本身消费哪条流、算出
    // 什么结果,见本函数文档「伤害公式接线」一节。
    let mut crit_rng =
        ll_core::rng::DetRng::for_entity(world.seed, actor.as_u64(), world.clock.0 as u64);
    let effective_luck = attacker_derived.attribute(AttributeKind::Luck);
    let crit_active = CheckSide {
        modifier: crit_attacker_modifier(effective_luck),
        bias: check_roll_bias(&attacker_modifiers, CRITICAL_CHECK),
        reroll_on: check_reroll_value(&attacker_modifiers),
    };
    let crit_passive = CheckSide {
        // 被攻击者一侧只有它自己的幸运，没有基准偏移——见
        // `crate::combat::crit_attacker_modifier` 文档。
        modifier: i64::from(defender_derived.attribute(AttributeKind::Luck)),
        bias: check_roll_bias(&defender_modifiers, CRITICAL_CHECK),
        reroll_on: check_reroll_value(&defender_modifiers),
    };
    let is_critical =
        opposed_check(&CHECK_DICE, &crit_active, &crit_passive, &mut crit_rng).active_wins();

    let formula_def = formulas.formula_for(explicit_formula);
    // 六项主属性的原始值（不是调整值）——按 `AttributeKind` 判别值
    // 下标，供 `FormulaInputs::new` 换算成 `str-mod`~`cha-mod` 六个
    // 操作数的调整值，见 `crate::formula::FormulaInputs` 文档。
    let raw_attributes = [
        attacker_derived.attribute(AttributeKind::Strength),
        attacker_derived.attribute(AttributeKind::Dexterity),
        attacker_derived.attribute(AttributeKind::Constitution),
        attacker_derived.attribute(AttributeKind::Intelligence),
        attacker_derived.attribute(AttributeKind::Willpower),
        attacker_derived.attribute(AttributeKind::Charisma),
        effective_luck,
    ];
    let formula_inputs = FormulaInputs::new(
        i64::from(attack_power_input),
        i64::from(defender_derived.armor()),
        i64::from(penetration.flat),
        i64::from(penetration.permille),
        raw_attributes,
        is_critical,
    );
    // 骰子随机流：与暴击判定各自独立的第二条 DetRng（见本函数文档
    // 「伤害公式接线」一节）——`0xD1CE_0000_0000_0000` 只是让这条流的
    // 事件计数与暴击那条（恒为 `world.clock.0 as u64`）不同的一个固定
    // 标签,没有数值含义上的特殊性,只要求"与暴击那条流的三元组不同"。
    const DAMAGE_FORMULA_DICE_EVENT_TAG: u64 = 0xD1CE_0000_0000_0000;
    let mut dice_rng = ll_core::rng::DetRng::for_entity(
        world.seed,
        actor.as_u64(),
        (world.clock.0 as u64) ^ DAMAGE_FORMULA_DICE_EVENT_TAG,
    );
    let attack_power_raw = eval_formula(&formula_def, &formula_inputs, &mut dice_rng);
    // 饱和转换到 i32——公式内部全程 i64 饱和运算（见 `eval_formula`
    // 文档），`damage_after_defense` 的入参类型是 i32,这里用饱和而不是
    // 直接 `as i32` 截断,避免一个极端公式在这一步产出静默环绕的错误
    // 数值（`as` 转换在数值超界时按位截断,不是钳位,那是比"公式确实
    // 算出一个夸张的大数"更危险的第二个错误）。
    let attack_power = attack_power_raw.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;

    let damage = damage_after_defense(attack_power, defender_derived.armor(), penetration);
    let damage = if is_critical {
        apply_crit_multiplier(damage)
    } else {
        damage
    };

    // 偷袭判定（盗贼偷袭接线批次；判定系统迁移批次换成对抗判定）：
    // 只有攻击者的有效天赋声明了 `RuleModifier::SneakAttack` 才会进入
    // 这个分支——没有声明时 `sneak_attack_rule` 返回 `None`，完全不构造
    // 额外的 `DetRng` 流,见 `RuleModifier::SneakAttack` 文档。挂载点：
    // 暴击放大之后、抗性乘数之前——与「抗性」一节同一条既有纪律，追加
    // 的伤害仍然是这一下攻击的一部分,应当同样受目标对这一伤害类别的
    // 抗性影响,不是绕开减伤链路凭空产出的独立效果。约束 C3：随机性走
    // `DetRng::for_entity(世界种子, 实体 ID, 事件计数)`,这里用一个与
    // 暴击流（恒为 `world.clock.0 as u64`）、骰子流
    // （`world.clock.0 ^ DAMAGE_FORMULA_DICE_EVENT_TAG`）都不同的第三个
    // 固定标签构造第三条独立流,三条流的三元组两两不同,互不干扰（约束
    // C5：这条流现场构造、只服务这一次判定，取数顺序由 `opposed_check`
    // 的固定程序顺序定死,且固定排在暴击判定之后、伤害公式骰子求值
    // 之后，与代码里出现的先后顺序一致）。攻击者一侧读
    // `attacker_derived.attribute(AttributeKind::Luck)`（同一个
    // `effective_luck`，暴击判定复用的派生值）——装备/状态效果加的幸运
    // 同样会反映到偷袭上，理由同暴击那一节「暴击：读取
    // attacker_derived.attribute」。
    const SNEAK_ATTACK_EVENT_TAG: u64 = 0x51EA_ACC0_0000_0000;
    let damage = match sneak_attack_rule(&attacker_modifiers) {
        Some(rule) => {
            let mut sneak_rng = ll_core::rng::DetRng::for_entity(
                world.seed,
                actor.as_u64(),
                (world.clock.0 as u64) ^ SNEAK_ATTACK_EVENT_TAG,
            );
            // 潜行不再是一条直通的守卫分支（那是「必定成功」，与所有者
            // 「不允许绝对」冲突），它是攻击者这一侧的一个修正——一整颗
            // 骰子，见 `crate::combat::STEALTH_SNEAK_MODIFIER`。
            let active = CheckSide {
                modifier: sneak_attacker_modifier(
                    effective_luck,
                    rule.sneak_modifier,
                    attacker.stealthed,
                ),
                bias: check_roll_bias(&attacker_modifiers, SNEAK_ATTACK_CHECK),
                reroll_on: check_reroll_value(&attacker_modifiers),
            };
            // 察觉 = 意志调整值——与藏匿判定（`resolve_inspect`）里
            // 盘查者那一侧是同一条所有者裁定，同一个属性、同一道
            // `attribute_modifier` 换算。攻守位置互换了（这里隐蔽方
            // 主动），见 `crate::check::SNEAK_ATTACK_CHECK` 文档。
            let passive = CheckSide {
                modifier: attribute_modifier(defender_derived.attribute(AttributeKind::Willpower)),
                bias: check_roll_bias(&defender_modifiers, SNEAK_ATTACK_CHECK),
                reroll_on: check_reroll_value(&defender_modifiers),
            };
            if opposed_check(&CHECK_DICE, &active, &passive, &mut sneak_rng).active_wins() {
                damage.saturating_add(rule.extra_damage)
            } else {
                damage
            }
        }
        None => damage,
    };

    // 抗性（伤害类别/抗性接线批次）：`damage-formula-mod-api.md` 二十节
    // 定死的挂载点是「减伤之后」——挂在减伤链路（含暴击放大，暴击与
    // 抗性都是「减伤之后」的后续放大/折扣，二十节本身不规定二者的先后,
    // 见 `RuleModifier::Resistance` 文档）算完之后，最后一步才把伤害
    // 类别的**减伤点数**扣掉。形式从该节原文的「乘数」改成了减法
    // （flat DR），见该节末尾的更正段与 `RuleModifier::Resistance` 文档
    // 「对小伤害强、对大伤害弱」一节。伤害类别的来源：武器显式声明
    // 的 `damage_category`（`weapon_rule.damage_category`），没有声明
    // 时退回 `damage_categories.default_category()`——与
    // `explicit_formula` 两层下探同一条既有纪律（见本函数文档「伤害
    // 公式接线」一节），只是这里没有「显式引用但未注册」这一档要处理
    // （`damage_category` 存的就是已经通过校验的 `ContentIndex`,见
    // `crate::item::ItemRule::damage_category` 文档）。
    let damage_category = weapon_rule
        .as_ref()
        .and_then(|rule| rule.damage_category)
        .unwrap_or_else(|| damage_categories.default_category());
    // 防御方的规则修正在本函数顶部已经聚合过**一次**，暴击判定的
    // 优劣势/重掷、减伤、易伤三个消费者共用同一份候选列表，理由见
    // 那一处注释。
    let damage_reduction = resistance_damage_reduction(&defender_modifiers, damage_category);
    // 易伤（易伤与减伤对称批次）：与减伤**各自独立聚合**，在下面那条
    // 算式里一减一加。拆成两个量的理由见
    // `ll_sim::rule_modifier::RuleModifier::Resistance` 文档「脆弱
    // **不**用负减伤表达」一节——同一个桶里「取最强」会让负减伤被正
    // 减伤静默吃掉。
    let damage_increase = vulnerability_damage_increase(&defender_modifiers, damage_category);
    // 整数加减 + 保底，全程饱和运算（点数是内容作者填的值，
    // `damage-formula-mod-api.md` 十二节「运行期溢出：饱和运算」同一条
    // 纪律）。保底的含义与边界情形见
    // `ll_sim::rule_modifier::damage_after_resistance` 与
    // `MINIMUM_DAMAGE_AFTER_RESISTANCE` 文档：减伤不封顶（大伤害自然
    // 穿透），但一次本来打得出伤害的攻击减完至少还剩 1 点——「绝对
    // 免疫」在减伤模型下不再是一个可声明的状态。净额一次算完再钳一次,
    // 不是「减完钳一次再加易伤」，理由见该函数文档「为什么是一条算式
    // 一次钳」一节。
    let damage = damage_after_resistance(damage, damage_reduction, damage_increase);

    let mut effects = vec![Effect::Damage {
        target,
        amount: damage,
    }];
    // 潜行破除（潜行与盗贼被动批次）：攻击者自己动手打人这一下就把
    // 潜行破掉——见本函数文档「潜行破除」一节。排在伤害之后：这一下
    // 的伤害**已经**吃到了上面的偷袭直通，破除从下一次行动起才生效
    // （经典的「一次免费背刺」形状）。不在潜行中时不产出这条效果，
    // 与本函数其余「没有相关状态就不多产一条效果」的既有纪律一致
    // （效果列表越短，`TurnEngine`/回放/呈现层要处理的东西越少）。
    if attacker.stealthed {
        effects.push(Effect::SetStealth {
            actor,
            stealthed: false,
        });
    }
    // 「使用」通道：攻击方主手那件**带 `on-use` 标签、且带耐久**的武器
    // 每打出这一下损失一点耐久——见本函数文档「耐久消耗：两条通道，
    // 判据是标签」一节。徒手（主手为空）、武器没有耐久概念、或这件东西
    // 压根没被声明成"用了会磨损"的类别时，都不产出任何效果。
    // `weapon_rule` 已经把耐久归零的武器滤掉（本函数上方「穿透」一节
    // 同一条"损坏即失效"纪律），坏掉的武器因此也不再继续磨损。
    let weapon_wears = weapon.is_some_and(|stack| stack.durability.is_some())
        && weapon_rule
            .as_ref()
            .is_some_and(|rule| rule.wear_channels.contains(WearChannels::ON_USE));
    if weapon_wears {
        effects.push(Effect::AdjustEquipmentDurability {
            actor,
            slot: EquipSlot::MAIN_HAND,
            delta: -WEAPON_DURABILITY_LOSS_PER_ATTACK,
        });
    }
    // 「挨打」通道：防御方每一件**带 `on-hit` 标签、且带耐久**的已装备
    // 物品各损失一点耐久——同上一节。判据是"这件东西是什么"（标签折算
    // 出的 `wear_channels`），不是"它挂在哪个槽位"：副手拿的可能是盾
    // （该磨损），也可能是副武器（不该走这条通道），槽位分不出这个差别。
    // `equipment` 是 `BTreeMap`（有序），产出顺序因此确定（约束 C5）。
    effects.extend(
        defender
            .equipment
            .iter()
            .filter(|(_, stack)| stack.durability.is_some())
            .filter(|(_, stack)| {
                items
                    .item(stack.def)
                    .is_some_and(|rule| rule.wear_channels.contains(WearChannels::ON_HIT))
            })
            .map(|(&slot, _)| Effect::AdjustEquipmentDurability {
                actor: target,
                slot,
                delta: -ARMOR_DURABILITY_LOSS_PER_HIT,
            }),
    );
    if defender.health - damage <= 0 {
        // 近战击杀——`weapon` 现在真正指向攻击者主手已装备的物品
        // （武器引用与穿透接线批次，P6 第六批），徒手攻击（主手为空）
        // 时恒 `None`，两者在类型上第一次真正区分开，见本函数文档
        // 「武器引用」一节与 `ll_world::history::KillCause::Melee` 文档。
        effects.push(Effect::Kill {
            target,
            killer: Some(actor),
            cause: KillCause::Melee { weapon: weapon_def },
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
