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
use ll_world::entity::{ActiveStatModifier, Agent, AttributeKind, BaseStats, EntityId};
use ll_world::history::KillCause;
use ll_world::space::{Space, SpaceId};
use ll_world::state::WorldState;

use crate::combat::{
    Penetration, apply_crit_multiplier, crit_chance_permille, damage_after_defense,
};
use crate::damage_category::{DamageCategoryCatalog, NoDamageCategories};
use crate::effect::Effect;
use crate::experience::ExperienceCatalog;
use crate::formula::{DamageFormulaCatalog, FormulaInputs, NoFormulas, eval_formula};
use crate::intent::{Direction, Intent};
use crate::item::{
    EquipSlot, ItemCatalog, ItemStack, NoItems, SlotMask, StatTarget, can_merge, merge_stacks,
};
use crate::quest::{NoQuests, QuestCatalog};
use crate::resource_pool::{
    NoResourcePools, RegenRule, ResourcePoolCatalog, ResourcePoolShape, RestRecoveryAmount,
    effective_scalar_capacity, effective_slot_tier_capacity,
};
use crate::skill::{NoSkills, ResourceCost, SkillCatalog, SkillEffect};
use crate::timeline::action_cost;
use crate::traits::{
    NoTraitGrants, NoTraits, TraitCatalog, TraitGrantSource, effective_traits, granted_skills,
    resistance_multiplier_permille,
};

/// 非位移动作（等待、攻击、开门）的基础代价，与平地移动同一基准
/// （草地的 `move_cost` 恰为这个值）——本批次没有武器速度、技能读条
/// 之类会让这些动作耗时不同于「一次基准行动」的系统，统一按这个基准
/// 计费，接入那些系统时按动作类型分别替换即可。
const BASE_ACTION_COST: u32 = 100;

/// 攻击方每打出一下近战攻击，自己主手已装备的武器（若带耐久）损失的
/// 耐久点数——武器引用与穿透接线批次（P6 第六批）把耐久消耗从「防御方
/// 全部已装备物品」收窄到「攻击方主手武器」，见 [`resolve_attack`]
/// 文档「耐久消耗：为什么收窄到只有武器」一节完整论证。
const EQUIPMENT_DURABILITY_LOSS_PER_HIT: i32 = 1;

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
/// 范围（见项目任务书「本批次范围」一节）。[`effective_speed_from_dexterity`]
/// 因此继续吃裸 `agent.stats.dexterity`，接上 `derive_stats` 是留给
/// 未来批次的工作，届时把这个常量与本函数体一并替换即可，调用点不变。
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

/// [`derive_stats`] 的产出——`attribute-system.md` §七 `derive_stats`
/// 签名里的 `DerivedStats`：七项属性（六项主属性 + 幸运，幸运并入
/// `AttributeKind` 批次）的最终生效值（基础值 + 状态效果 + 装备）与护甲
/// （防御端的来源，P6 第四批新增）。
///
/// # 派生，不缓存——不进 `WorldState::hash()`
///
/// 这是 `attribute-system.md` 七节整节的标题：「衍生属性绝不进存档」。
/// 本类型只在 [`derive_stats`] 被调用的那一刻现算现用（典型调用点是
/// 每次 [`resolve_attack`] 结算），从不写回 [`ll_world::entity::Agent`]
/// 或 `WorldState` 的任何字段，因此**不需要**、也**不应该**出现在
/// `WorldState::hash()`——存进去必然与来源（基础属性/状态效果/装备）
/// 不同步，见该节原文「脱了装备忘了减、buff 到期忘了移除，最终属性
/// 面板显示的数字与实际结算用的数字对不上」。真正进 `hash()` 的仍然
/// 只是三个来源自身的数据：`Agent::stats`（早已进）、
/// `Agent::active_stat_modifiers`（早已进）、`Agent::equipment`（P6 第
/// 三批已进）——本类型只是把三者现算汇总的临时产物，任何一次结算都
/// 可以从这三份既有数据重新算出完全相同的 `DerivedStats`，缓存它换不
/// 来任何正确性收益，只会新增一条要手动维持同步的不变式。
///
/// # 为什么能容纳载具「替换」语义（不需要现在就实现）
///
/// `knowledge/design/vehicle-and-mounting.md` 四节③裁定：移动速度是
/// **替换**语义（骑乘时读坐骑自己的敏捷，不是给骑手敏捷加一个 delta），
/// 攻击/防御/其余属性加成是**叠加**语义。本类型不需要为这条区分新增
/// 任何字段——`derive_stats` 本身是纯函数，输入是"某一个实体自己的
/// `stats`/`active_stat_modifiers`/`equipment`"，`Armor`/`Attribute`
/// 两类目标在同一个实体内部永远是叠加（装备/状态效果各自独立生效，
/// 见 [`derive_stats`] 文档「装备加成与状态效果如何合」一节）；"替换"
/// 不是某个属性内部的合并规则，是"这一步该向哪个实体要输入"这一层
/// 决定——载具批次落地时，移动速度的计算只需要改成对坐骑（而不是
/// 骑手）调用一次 `derive_stats` 取它的 `attribute(Dexterity)`，本类型
/// 与 `derive_stats` 的签名完全不用改，`vehicle-and-mounting.md` 三节
/// 给出的 `mover_speed` 伪代码（`mover.map_or(agent.stats.dexterity, |m|
/// m.stats.dexterity)`）就是这个道理的直接体现，只是届时应换成读
/// `derive_stats(mover, ..).attribute(Dexterity)` 而不是裸
/// `m.stats.dexterity`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedStats {
    attributes: [i32; 7],
    armor: i32,
}

impl DerivedStats {
    /// 七项属性（六项主属性 + 幸运，幸运并入 `AttributeKind` 批次）里
    /// 指定一项的最终生效值——`resolve_attack` 攻击力（力量）与暴击率
    /// 输入（幸运）的读取入口，未来三轴战斗结算的魔法/精神攻击力同样
    /// 从这里读（`Intelligence`/`Willpower`）。
    pub fn attribute(&self, kind: AttributeKind) -> i32 {
        self.attributes[attribute_slot(kind)]
    }

    /// 护甲——`resolve_attack` 防御端的来源（P6 第四批：`derive_stats`
    /// 与装备属性接进战斗，这是防御端第一次真的生效）。
    pub fn armor(&self) -> i32 {
        self.armor
    }
}

/// [`AttributeKind`] 七个变体（六项主属性 + 幸运）到
/// [`DerivedStats::attributes`] 数组下标的映射——枚举变体本身没有稳定的
/// 数值表示（不依赖 `enum` 的 discriminant，那是实现细节，不是公开
/// 契约），这里显式给出，唯一的读者是 [`DerivedStats::attribute`] 与
/// [`derive_stats`] 自身。
const fn attribute_slot(kind: AttributeKind) -> usize {
    match kind {
        AttributeKind::Strength => 0,
        AttributeKind::Dexterity => 1,
        AttributeKind::Constitution => 2,
        AttributeKind::Intelligence => 3,
        AttributeKind::Willpower => 4,
        AttributeKind::Charisma => 5,
        AttributeKind::Luck => 6,
    }
}

/// `attribute-system.md` §七 `derive_stats(基础属性, 装备, 状态效果,
/// 负重) -> DerivedStats` 签名在 P6 第四批的落地——**单一聚合入口**：
/// 把基础属性、状态效果（[`ll_world::entity::Agent::active_stat_modifiers`]）
/// 与装备（已装备物品的 [`crate::item::ItemRule::stat_bonuses`]）三者汇总
/// 成 [`DerivedStats`]。旧的 `effective_attribute`（本文件此前的私有
/// 函数，只读状态效果这一个输入）已被本函数取代并删除——`98621f5`
/// 建它时就说明了「将来 `derive_stats` 落地后应该用它的对应分支替换
/// 这个函数体，调用点不变」，本函数是那句话的执行，调用点
/// （[`resolve_attack`]）也确实不必改变调用形状（仍然是"给一个实体的
/// 三份数据，要一个数"），只是数据来源从两份（基础值 + 状态效果）变成
/// 了三份（基础值 + 状态效果 + 装备）。ADR 0021：只有算法真正可共享时
/// 才抽象——旧函数与新函数做的是**同一件事**（把多个来源汇总成一个
/// 最终生效值），不是表面相似的两件事，因此是替换而不是并存两条聚合
/// 路径。
///
/// **本批次不做**：`负重`——`ll_world::item` 模块文档已核实
/// `Agent`/`ItemStack` 都还没有负重相关字段（背包物品的重量从未被
/// 累加过），提前给这个入参一个假的默认值（例如恒 0）只会制造一个
/// 看起来接了、实际上永远不生效的参数，与 `ll_mod::item` 模块文档
/// 「本批次范围」一节同一条 YAGNI 判断。真正落地负重系统的批次照
/// `equip_mask`/`stat_bonuses` 的先例，在 `derive_stats` 的签名上加一
/// 个新参数即可，调用点跟着加一个入参,不需要改动本函数已有的三段
/// 逻辑。
///
/// # 状态效果：逐条过滤未过期条目再求和，异源叠加、同源已在写入时合并
///
/// `buffs-and-triggers.md` 六节裁定「不同效果能叠加」——`active_modifiers`
/// 外层按 [`AttributeKind`] 索引，内层按「来源」的 `ContentIndex` 索引，
/// 本函数遍历内层全部条目，过滤掉已过期的（惰性到期判定，见下），对
/// 剩下的 `delta` 求和。"同源刷新"发生在写入 `active_stat_modifiers`
/// 的那一刻（[`ActiveStatModifier::merge_same_source`]），本函数只管
/// 读取已经合并好的数据，不重复判断"是否同源"。
///
/// # 装备：逐件已装备物品的静态加成求和——异源叠加，没有"刷新"这个概念
///
/// 遍历 `equipment`（[`ll_world::entity::Agent::equipment`]，锚点槽位
/// 为键，多槽物品只存一份，见其文档）的每一件已装备堆，查 `items`
/// 目录拿到这件物品的 [`crate::item::ItemRule::stat_bonuses`]，按
/// [`crate::item::StatTarget`] 分派累加到对应的主属性或护甲上。
///
/// # 装备加成与状态效果如何合：两条独立的数据通道，在这里第一次真正
/// 汇合
///
/// 装备加成（[`crate::item::StatBonus`]，静态数据，随 `ItemDef` 走）
/// 与状态效果（[`ActiveStatModifier`]，带 `expires_at` 的临时数据，随
/// `Agent::active_stat_modifiers` 走）**不是同一套存储，也不需要互相
/// 转换成对方的形状**——装备加成没有"过期"这个概念（穿没穿在身上是
/// 二元状态，不需要惰性到期判定那一套），状态效果没有"物品堆"这个概念
/// （技能/天赋/载具都不对应任何 `ItemStack`）。两条通道各自按自己的
/// 规则算出一个 delta 之和,`derive_stats` 只是把两个和数**相加**到
/// 同一个基础值上——这正是「四个来源要叠加」的字面含义：技能/天赋/
/// 载具三者共享 `active_stat_modifiers` 这一条通道（内部按来源各自
/// 独立），装备独占 `equipment` 这另一条通道，两条通道的结果在
/// `derive_stats` 这一层、也只在这一层相加，不早于此（不会有任何一条
/// 通道提前把另一条通道的贡献也算进自己的和里）,也不晚于此（不存在
/// 第三处再次合并两者的地方——`resolve_attack` 只读 `DerivedStats` 现成
/// 的最终值)。
///
/// # 护甲不参与状态效果通道（本批次）
///
/// `AttributeKind` 七个变体里没有对应"护甲"的一项（`vehicle-and-mounting.md`
/// 一节已核实），本批次因此没有任何技能/天赋能通过 `active_stat_modifiers`
/// 直接加护甲——护甲目前只有装备一条来源。这不是遗漏：
/// `combat-three-axis.md` 四节把这条留给了"届时再定案"，本批次的任务
/// 范围明确写着"（技能/天赋/载具）与装备两个通道怎么合"，不是"要不要
/// 让技能也能加护甲"这个内容设计问题——如实沿用现状即可。
///
/// # 耐久归零：损坏的装备不再贡献属性加成（耐久与 `Intent::Use` 落地
/// 批次，P6 第五批）
///
/// `item-system.md` 六节裁定「归零 = 损坏不可用，但不消失，可修复」
/// ——本函数遍历 `equipment` 时,`durability == Some(0)` 的堆直接跳过,
/// 不查询它的 `stat_bonuses`，见下方实现里的 `continue` 分支。这正是
/// "不可用"在结算侧的落点：装备仍然穿在身上（不自动卸下，见下一节），
/// 只是不再提供任何攻防加成，与一件从未装备过的物品在 `derive_stats`
/// 眼里等价。
///
/// # 耐久归零为什么不触发自动卸下
///
/// `resolve_attack`/`resolve_use_item` 只产出
/// [`crate::effect::Effect::AdjustEquipmentDurability`]，从不产出
/// [`crate::effect::Effect::Unequip`]——损坏的装备继续占着槽位（玩家
/// 仍然看得到"这个槽位穿着一件坏掉的甲"，可修复系统落地后原地修好即可
/// 继续生效，不需要重新装备）。这与
/// `resolve_equip` 的占位冲突逻辑（换装时主动卸下冲突槽位）是两件不
/// 同的事：那里卸下是因为"这个槽位要让给别的物品"，这里"槽位没有变，
/// 只是这件物品暂时不生效"，没有任何理由把它请出槽位。
///
/// # 惰性到期判定
///
/// `expires_at.0 > now.0` 才算仍然生效——与 [`resolve_use_skill`] 冷却
/// 判定（其「门二」注释）同一条比较方向：世界时钟达到或超过到期时刻时
/// 视为已失效，直接回落到裸属性值，不做任何清理，见 [`ActiveStatModifier`]
/// 文档「惰性到期判定，不存『当前是否生效』」一节。
pub fn derive_stats(
    base: BaseStats,
    active_modifiers: &std::collections::BTreeMap<
        AttributeKind,
        std::collections::BTreeMap<ContentIndex, ActiveStatModifier>,
    >,
    equipment: &std::collections::BTreeMap<EquipSlot, ItemStack>,
    items: &dyn ItemCatalog,
    now: Tick,
) -> DerivedStats {
    let mut attributes = [
        base.strength,
        base.dexterity,
        base.constitution,
        base.intelligence,
        base.willpower,
        base.charisma,
        base.luck,
    ];
    let mut armor = 0;

    for (&kind, per_source) in active_modifiers {
        let delta: i32 = per_source
            .values()
            .filter(|modifier| modifier.expires_at.0 > now.0)
            .map(|modifier| modifier.delta)
            .sum();
        attributes[attribute_slot(kind)] += delta;
    }

    for stack in equipment.values() {
        // 耐久归零 = 损坏不可用（`item-system.md` 六节：「归零 = 损坏
        // 不可用，但不消失」），本函数是"不可用"这句话在结算侧唯一的
        // 落点——一件耐久归零的装备仍然占着槽位（不会被自动卸下，见
        // 本函数文档「耐久归零为什么不触发自动卸下」一节），只是不再
        // 贡献任何属性加成。`durability == Some(0)` 才算耗尽；`None`
        // （没有耐久概念的物品）与 `Some(正数)` 都照常生效——这条判定
        // 因此不是恒真：耐久未耗尽时（`Some(正数)` 或 `None`）不会走
        // 这条 `continue`,见 `derive_stats` 的反例测试。
        if stack.durability == Some(0) {
            continue;
        }
        let Some(rule) = items.item(stack.def) else {
            continue;
        };
        for bonus in &rule.stat_bonuses {
            match bonus.target {
                StatTarget::Attribute(kind) => attributes[attribute_slot(kind)] += bonus.amount,
                StatTarget::Armor => armor += bonus.amount,
            }
        }
    }

    DerivedStats { attributes, armor }
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
        &NoFormulas,
        &NoDamageCategories,
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
        &NoFormulas,
        &NoDamageCategories,
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
        &NoFormulas,
        &NoDamageCategories,
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
        traits,
        pools,
        items,
        formulas,
        &NoDamageCategories,
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
/// 类别恒为 [`ContentIndex::default()`]，与任何真实注册的伤害类别都
/// 不会撞上，见 [`NoDamageCategories`] 文档），本函数只服务真正想让
/// "武器没声明伤害类别时退回哪个默认类别"生效的调用方
/// （`ll_mod::damage_category` 落地对应的真实目录实现后即可接入）。
///
/// **本函数不改变抗性本身生不生效**——抗性查询
/// （[`resistance_multiplier_permille`]）只要防御方的天赋声明了
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
        traits,
        pools,
        items,
        formulas,
        damage_categories,
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
        &NoFormulas,
        &NoDamageCategories,
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
    traits: &dyn TraitCatalog,
    pools: &dyn ResourcePoolCatalog,
    items: &dyn ItemCatalog,
    formulas: &dyn DamageFormulaCatalog,
    damage_categories: &dyn DamageCategoryCatalog,
) -> Vec<Effect> {
    let mut effects = match *intent {
        Intent::Wait { actor } => resolve_wait(world, actor, race_traits, traits, pools),
        Intent::Move { actor, dir } => resolve_move(world, actor, dir),
        Intent::Attack { actor, target } => resolve_attack(
            world,
            actor,
            target,
            items,
            formulas,
            race_traits,
            traits,
            damage_categories,
        ),
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
        Intent::Loot { actor } => resolve_loot(world, actor, items),
        Intent::Drop { actor, def } => resolve_drop(world, actor, def),
        Intent::Equip { actor, def } => resolve_equip(world, actor, def, items),
        Intent::Unequip { actor, slot } => resolve_unequip(world, actor, slot, items),
        Intent::Use { actor, def } => resolve_use_item(world, actor, def, items),
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
    // 死亡掉落（NPC 生命周期批次）：与击杀历史记录同一个触发点（同一批
    // Effect::Kill），各自独立追加,互不依赖——见 append_corpse_drop
    // 文档。不需要按 Intent 类型区分调用与否,理由同 append_kill_history。
    append_corpse_drop(world, &mut effects);
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
/// # 空手死者不产出尸体
///
/// `inventory`/`equipment` 合计为空时不追加任何效果——`GroundItemStack::contents`
/// 非空是"这是一具容器"的唯一判据（见其文档），一具打不出任何东西的
/// 尸体没有玩法意义（[`resolve_loot`]/[`resolve_pick_up`] 都不会把它
/// 当作合法目标），提前占一个 `ground_items` 条目只会增加后续老化清理
/// 与存档体积的无谓开销。
///
/// # 尸体的 `def`：复用死者的 `creature_kind`/`race`，不新开一张
/// "尸体物品"注册表
///
/// `ll-sim` 不能依赖 `ll-mod`（依赖方向，规格 §5），本函数因此拿不到
/// 任何 `ItemCatalog`/`Registry` 去 `intern` 一个专门的
/// `lostland:corpse` 内容 ID——即便能拿到，也需要每个 mod 各自声明
/// "我的种族死了要用哪个尸体物品"这类新的注册表，而当前没有任何真实
/// 消费场景需要区分"哥布林尸体"与"人类尸体"这两件事本身是两种不同的
/// 可堆叠物品（YAGNI，同一条判断见 `ll_world::item` 模块文档「`Owner`
/// 本批次仍然不落地」一节）。`victim_agent.creature_kind.unwrap_or(victim_agent.race)`
/// ——与 [`Effect::IncrementKillCount`] 归并键完全同一套既有回退规则
/// （见其文档「为什么按 `kind: ContentIndex`」一节）——天然给出一个
/// "这具尸体是什么生物"的身份，不需要新的注册表或跨 crate 依赖：
/// 一具哥布林的尸体，`def` 就是"哥布林"这个种族/生物类型索引本身。
///
/// `stack.durability` 恒 `None`——尸体这件"容器"本身没有耐久概念，与
/// [`ItemStack::new`] 材料/消耗品的既有语义一致。
///
/// # 两具尸体不会被静默合并
///
/// [`resolve_pick_up`] 已经把 `contents` 非空的地面物品整体排除在
/// 合并/拾取路径之外（见其文档「为什么跳过容器」一节）——`can_merge`
/// 只比较 `ItemStack` 的 `def`/`durability`，两具同种生物的尸体确实会
/// 在这两个字段上相等（`can_merge` 会判定为"可合并"），但这条判定
/// 永远不会被触发到：尸体从不作为 [`Intent::PickUp`] 的目标进入
/// `merge_into_inventory_effect`，真正阻止"两具尸体的战利品被静默
/// 混进同一个背包堆"的是这道路径排除，不是 `stack_limit`（`stack_limit`
/// 查不到该 `def` 对应的 `ItemDef` 时按"不限量"处理，见
/// [`resolve_pick_up`] 文档，本身并不能阻止 `can_merge` 判真——两具
/// 尸体的地面条目本身也从不会被本函数或任何既有代码路径互相合并，
/// `AddGroundItem` 的 `apply` 分支恒是无条件 `push`，见其文档）。
fn append_corpse_drop(world: &WorldState, effects: &mut Vec<Effect>) {
    let drops: Vec<Effect> = effects
        .iter()
        .filter_map(|effect| {
            let Effect::Kill { target, .. } = effect else {
                return None;
            };
            let victim = world.actors.get(*target)?;
            let mut loot = victim.inventory.clone();
            loot.extend(victim.equipment.values().copied());
            if loot.is_empty() {
                return None;
            }
            let corpse_def = victim.creature_kind.unwrap_or(victim.race);
            Some(Effect::AddGroundItem {
                pos: victim.pos,
                stack: ItemStack::new(corpse_def, 1),
                dropped_at: world.clock,
                contents: loot,
            })
        })
        .collect();
    effects.extend(drops);
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
/// 脚下的第一堆**非容器**地面物品（见 `Intent::PickUp` 文档「为什么不
/// 指定要捡哪一种」一节），若背包已有可合并的同种堆（[`can_merge`]），
/// 一并算出合并结果。
///
/// # 静默无效的三种情形
///
/// `actor` 不存在，脚下没有任何地面物品，或脚下只有容器（尸体，见下
/// 「为什么跳过容器」一节）——与 `resolve_attack`/`resolve_open_door`
/// 目标不存在时的既有纪律一致（见模块文档开篇「目标实体……若已不在
/// `world.actors` 中……一律返回空 `Vec`」），不是错误，只是这一步什么都
/// 不发生。
///
/// # 为什么跳过容器（NPC 死亡掉落批次）
///
/// 容器（[`ll_world::item::GroundItemStack::contents`] 非空,典型是
/// 尸体）不是[`Intent::PickUp`]的合法目标——本函数只会把 `ground.stack`
/// 这一个字段拿去合并进背包，容器真正的价值（`contents` 里的战利品）
/// 会被原样丢在地上、永久不可达,这不是"物品异常地不能堆叠"那类可以
/// 接受的降级，是真实的数据丢失。搜刮容器走专门的
/// [`Intent::Loot`]（[`resolve_loot`]），本函数因此显式过滤掉
/// `!item.contents.is_empty()` 的地面物品，与 `GroundItemStack::contents`
/// 字段文档「`resolve_pick_up` 用这条判据把尸体排除在普通拾取目标
/// 之外」一节相互印证。
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
    let Some(ground) = world
        .ground_items
        .iter()
        .find(|item| item.pos == agent.pos && item.contents.is_empty())
    else {
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

/// [`Intent::Loot`] 结算（NPC 死亡掉落批次）：把 `actor` 脚下第一具
/// 容器（[`ll_world::item::GroundItemStack::contents`] 非空,典型是
/// 尸体）的全部内容物移进背包，容器本身随后从地面移除——「搜刮」是
/// 一次性、全部拿走，不支持挑拣部分战利品,与 `Intent::Drop`「不支持
/// 部分数量」同一条范围裁定（见其文档）：本批次的验收范围不需要战利品
/// 挑选 UI,提前引入只会制造一个当前没有测试覆盖的分支。
///
/// # 静默无效的两种情形
///
/// `actor` 不存在，或脚下没有任何容器——与 [`resolve_pick_up`] 同一条
/// 纪律。
///
/// # 为什么容器本身用 [`Effect::RemoveGroundItem`]，不新开一个变体
///
/// 与 [`resolve_pick_up`] 移除已拾取的普通地面物品是同一个机械操作
/// （按 `(pos, def)` 定位并移除），没有理由为"移除的这一条恰好是容器"
/// 单独发明一个效果变体——`apply` 侧的写入逻辑完全相同。
///
/// # 已知限制：容器按 `(pos, def)` 定位，多具同 `def` 容器共存一格时
/// 可能误删
///
/// 与 [`Effect::RemoveGroundItem`] 文档「为什么按 `(pos, def)` 定位」
/// 一节同一条既有限制：若同一格恰好摞着两具"生物种类相同"的尸体
/// （`def` 相同，见 [`append_corpse_drop`] 文档「尸体的 `def`」一节），
/// `Effect::RemoveGroundItem` 按 `(pos, def)` 匹配到的不保证是本函数
/// 读到的那一具——这是"第一条匹配"既有纪律（`Intent::PickUp` 文档
/// 「为什么不指定要捡哪一种」一节同一先例）在容器场景下的延伸,不是本
/// 批次新引入的缺陷,如实记录为已知边界情形。
///
/// # 已知限制：不处理"搜刮的多条战利品本可以互相合并"的情形
///
/// 与 [`merge_into_inventory_effect`] 文档「已知限制」一节同一条既有
/// 局限：每条内容物各自基于同一份背包快照判断"有没有可合并的旧堆"，
/// 不产生数据错误（数量守恒），只是可能错过一次本可以做的合并。
fn resolve_loot(world: &WorldState, actor: EntityId, items: &dyn ItemCatalog) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(container) = world
        .ground_items
        .iter()
        .find(|item| item.pos == agent.pos && !item.contents.is_empty())
    else {
        return Vec::new();
    };

    let mut effects = vec![Effect::RemoveGroundItem {
        pos: container.pos,
        def: container.stack.def,
    }];
    effects.extend(
        container
            .contents
            .iter()
            .map(|loot| merge_into_inventory_effect(agent, actor, *loot, items)),
    );
    effects
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
            // 普通丢弃恒不带容器内容物——contents 非空是尸体专属的
            // 判据，见 GroundItemStack::contents 文档。
            contents: Vec::new(),
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

/// [`Intent::Use`] 结算（耐久与 `Intent::Use` 落地批次，P6 第五批）：
/// 消耗 `actor` 背包里第一条匹配 `def` 的堆一个单位，产出它的
/// `use_effect`（[`crate::item::ItemRule::use_effect`]，复用
/// [`SkillEffect`]，见其文档「为什么复用 `SkillEffect`」一节）对应的
/// `Effect`——`match` 分支与 [`resolve_use_skill`] 对同一个
/// `SkillEffect` 的三个变体逐字对应，唯一的区别是本函数没有冷却/资源
/// 消耗两道门（物品的"触发条件"是数量/耐久，不是冷却/资源，见
/// `ll_sim::item::ItemRule::use_effect` 文档同一节）。
///
/// # 目标恒为发起者自身
///
/// 与 [`Intent::Use`] 文档「为什么携带 def，不携带目标」一节同一条
/// 范围裁定：本批次的物品使用效果只施于使用者自己，没有「对着别人用
/// 一件消耗品」的真实场景需要表达。
///
/// # 静默无效的三种情形
///
/// `actor` 不存在、背包里没有匹配 `def` 的堆、`def` 查不到物品规则或
/// 查到但 `use_effect` 是 `None`（材料、装备本身……不能被使用）——与
/// [`resolve_drop`]/[`resolve_equip`] 同一条「静默无效，不是错误」
/// 纪律。
fn resolve_use_item(
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
/// 攻击力：攻击者的 [`derive_stats`] 力量项（基础值 + 状态效果 + 装备
/// 三个来源汇总后的最终生效值，技能增益/削弱与武器加成由此接线生效）。
///
/// 防御：防御方的 [`derive_stats`] 护甲——**P6 第四批：这是防御端第一
/// 次真的生效**，此前恒为占位的 `0`。护甲的唯一来源目前是防御方已装备
/// 物品的 [`crate::item::StatBonus`]（见 [`derive_stats`] 文档「护甲不
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
/// # 耐久消耗：为什么收窄到只有武器（武器引用与穿透接线批次，P6 第六批）
///
/// P6 第五批曾把"攻击时掉武器耐久"还是"被击中掉护甲耐久"选了后者
/// （挨打的防御方所有已装备物品都掉耐久），原因是当时 `Intent::Attack`
/// 无法把耐久损耗记到攻击方任何具体装备上——见本文件此前版本的记录。
/// 项目所有者随后裁定「装备武器才有耐久，其余物品我倾向于没有」：一旦
/// 「武器」这个引用已经能查到（见上方「武器引用」一节），当初选择
/// 「被击中掉耐久」的前提（"打人这一方无法归因"）已经不成立，应当回到
/// 更符合直觉的规则——**本函数现在改为：攻击方每打出这一下，若自己
/// 主手已装备的武器带耐久（`ItemStack.durability.is_some()`），这件
/// 武器损失 [`EQUIPMENT_DURABILITY_LOSS_PER_HIT`] 点耐久；防御方的
/// 护甲/其余已装备物品不再因为挨打而损耗耐久**——耐久磨损现在只发生在
/// 「正在被使用的武器」这一件事上，与所有者的裁定完全对应：装备武器
/// 才有耐久，其余（包括护甲）不再有耐久这个概念本该走的路径是
/// `register-item` 注册期本身就不该给非武器物品声明耐久上限，见
/// `ll_mod::script_item_api::register_item_equip_mask` 文档「为什么在
/// 这里校验耐久与武器槽位的组合」一节；本函数只负责"武器耐久确实会
/// 随攻击减少、其余装备确实不再减少"这个结算侧的行为，不重复注册期的
/// 校验职责。
///
/// # 暴击：读取 `attacker_derived.attribute(AttributeKind::Luck)`（幸运并入
/// `AttributeKind` 批次）
///
/// 所有者原话（针对盗贼偷袭的裁定，本批次先落地最现成的一处）：「做成
/// 技能判定吧，通过幸运值之类的属性以及一定的随机值组合一下」——暴击
/// 正是「战斗结算里现成的、幸运能挂上去的判定点」（`combat.rs` 已有
/// `damage_after_defense` 这条主干，暴击只是在它算出的伤害上再判一次
/// 是否放大，不需要新开一条结算路径）。幸运通过
/// [`crate::combat::crit_chance_permille`] 换算成千分比暴击率，输入是
/// `attacker_derived.attribute(AttributeKind::Luck)`——**派生值，不是裸
/// `attacker.stats.luck`**：幸运并入 `AttributeKind` 批次之前，幸运是
/// `Agent` 上不受装备/状态效果影响的独立字段，暴击只能读裸值；并入之后
/// 幸运戒指（[`crate::item::StatTarget::Attribute`]）、祝福术/诅咒
/// （[`ll_world::entity::ActiveStatModifier`]）都要能改变它，若这里继续
/// 读裸 `attacker.stats.luck`，装备/buff 加的幸运永远不会反映到暴击率
/// 上——那就白并了。`attacker_derived` 已经是 [`derive_stats`] 汇总过
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
/// 零幸运（本仓库全部现存测试夹具的默认值）换算出的暴击率精确为零
/// （见 [`crate::combat::crit_chance_permille`] 文档「没有独立的
/// 『基础暴击率』常量」一节）——这保证了本次接线不会让任何一条既有
/// 的确定性伤害断言或黄金基准哈希（`crates/ll-sim/tests/replay.rs`）
/// 变成依赖随机数的赌博：即便这里确实调用了 `DetRng`，`chance(0, ..)`
/// 恒返回 `false`，`damage` 恒等于 `damage_after_defense` 的原始结果。
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
/// # 抗性接线（伤害类别/抗性接线批次）
///
/// `damage-formula-mod-api.md` 二十节把抗性的挂载点定死在「减伤之后、
/// 乘数形式」——本函数在 `damage_after_defense`（含暴击放大）算完之后
/// 最后一步，用这一下的伤害类别（武器显式声明的
/// [`crate::item::ItemRule::damage_category`]，没有声明时退回
/// [`DamageCategoryCatalog::default_category`]）查
/// [`resistance_multiplier_permille`]（`crate::traits`，遍历**防御方**
/// 的有效天赋收集 `RuleModifier::Resistance`），把查到的千分比乘数乘
/// 在伤害上——没有任何天赋声明抗性时，乘数恒为
/// [`crate::traits::RESISTANCE_MULTIPLIER_SCALE`]（1.0），本函数因此
/// 逐位复现接入抗性之前的既有行为，与「伤害公式接线」一节「全局默认
/// 公式」的「行为等价」承诺是同一条纪律的第二次应用。
///
/// 免疫（乘数 0）能合法地把这一步的结果打成 0，即使
/// `damage_after_defense` 内部的 10% 下限已经让上一步的 `damage` 不低于
/// 攻击力的一成——两者不冲突：10% 下限保护的是「减伤链路本身不会因为
/// 防御过高而系统性压制到零」，抗性回答的是「这种伤害对这个目标有没有
/// 意义」，见 `RuleModifier::Resistance` 文档「与 10% 下限的关系」
/// 一节完整论证。
#[allow(clippy::too_many_arguments)]
fn resolve_attack(
    world: &WorldState,
    actor: EntityId,
    target: EntityId,
    items: &dyn ItemCatalog,
    formulas: &dyn DamageFormulaCatalog,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    damage_categories: &dyn DamageCategoryCatalog,
) -> Vec<Effect> {
    let Some(attacker) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(defender) = world.actors.get(target) else {
        return Vec::new();
    };

    let attacker_derived = derive_stats(
        attacker.stats,
        &attacker.active_stat_modifiers,
        &attacker.equipment,
        items,
        world.clock,
    );
    let defender_derived = derive_stats(
        defender.stats,
        &defender.active_stat_modifiers,
        &defender.equipment,
        items,
        world.clock,
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

    // 暴击判定（幸运并入 AttributeKind 批次）：读
    // `attacker_derived.attribute(AttributeKind::Luck)`——派生值，装备/
    // 状态效果加的幸运在这里生效，见本函数文档「暴击」一节。约束 C3
    // ——随机性必须走 `DetRng::for_entity(世界种子, 实体 ID, 事件计数)`，
    // 这里用攻击者自己的实体 ID 与当前世界时钟作三元组的后两项，与
    // `ll_mod::script_behavior_source` 的 AI 决策随机流同一套取法
    // （见其文档「C3」一节）；约束 C5——本函数在暴击判定这一步只消费
    // 这一次随机数，取数顺序天然确定，不存在「先读了别的随机数再读
    // 这个」的排列组合问题。判定挪到公式求值**之前**（此前挪到公式
    // 求值之后）——公式的 `Crit` 操作数需要这个结果作为输入,但这
    // 只是「谁先计算」的顺序调整,不改变这次判定本身消费哪条流、算出
    // 什么结果,见本函数文档「伤害公式接线」一节。
    let mut crit_rng =
        ll_core::rng::DetRng::for_entity(world.seed, actor.as_u64(), world.clock.0 as u64);
    // 分母 1000：千分比运算的分母，与 `combat::crit_chance_permille`
    // 返回值同一个刻度（见该函数文档「夹在 0..=1000」）。
    let effective_luck = attacker_derived.attribute(AttributeKind::Luck);
    let is_critical = crit_rng.chance(crit_chance_permille(effective_luck).max(0) as u32, 1000);

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

    // 抗性（伤害类别/抗性接线批次）：`damage-formula-mod-api.md` 二十节
    // 「减伤之后、乘数形式」——挂在减伤链路（含暴击放大，暴击与抗性都
    // 是「减伤之后」的后续放大/折扣，二十节本身不规定二者的先后，见
    // `RuleModifier::Resistance` 文档「抗性接线」一节）算完之后，最后
    // 一步才把伤害类别的抗性乘数乘上去。伤害类别的来源：武器显式声明
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
    let resistance_multiplier = resistance_multiplier_permille(
        defender.race,
        defender.level,
        race_traits,
        traits,
        damage_category,
    );
    // 千分比乘法，向零截断——与 `FormulaOp::MulPermille`/
    // `apply_crit_multiplier` 同一条既有惯例，全程 i64 饱和运算防止
    // 极端乘数溢出 i32（`multiplier_permille` 是内容作者填的数值，
    // `damage-formula-mod-api.md` 十二节「运行期溢出：饱和运算」同一条
    // 纪律）。免疫（乘数 0）会合法地把这一步打成 0，即使上一步的
    // `damage` 满足了 10% 下限——`damage_after_defense` 的下限只保护
    // 「减伤链路本身」，不保护抗性之后的结果，见
    // `RuleModifier::Resistance` 文档「与 10% 下限的关系」一节。
    let damage = ((i64::from(damage) * i64::from(resistance_multiplier)) / 1000)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;

    let mut effects = vec![Effect::Damage {
        target,
        amount: damage,
    }];
    // 攻击方主手武器（若带耐久）每打出这一下损失一点耐久——见本函数
    // 文档「耐久消耗」一节；徒手（主手为空）或武器没有耐久概念时不
    // 产出任何效果。
    effects.extend(weapon.filter(|stack| stack.durability.is_some()).map(|_| {
        Effect::AdjustEquipmentDurability {
            actor,
            slot: EquipSlot::MAIN_HAND,
            delta: -EQUIPMENT_DURABILITY_LOSS_PER_HIT,
        }
    }));
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

    /// 造一个占位实体，站在 `pos`，除幸运外六项主属性取基准值——供
    /// 暴击率频率测试指定一个非零的幸运值，与 [`spawn_agent_with_dexterity`]
    /// 同一个模式。
    fn spawn_agent_with_luck(
        world: &mut WorldState,
        pos: ll_core::torus::TorusPos,
        luck: i32,
    ) -> EntityId {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats {
                luck,
                ..BaseStats::BASELINE
            },
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
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
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
        // derive_stats 的文档。
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

    /// 任务硬要求二「全局默认公式必须逐行复现现在的行为」的验收——不
    /// 走 [`NoFormulas`] 这条「没接目录」的短路便利类型，而是构造一个
    /// 真正实现 [`DamageFormulaCatalog`] 的公式目录（其 `formula_for`
    /// 恒返回 [`crate::formula::default_attack_power_instructions`]
    /// 这条全局默认公式——与 `ll_mod::base_damage_formula::register_base_damage_formula`
    /// 生产环境真正注册出来的那条公式逐字同构），证明"即便真的经过公式
    /// 求值这条代码路径，没有任何 mod 指定公式时算出的伤害仍然与接入
    /// 公式引擎之前完全一致"，不是因为走了某条特殊的空实现快捷路径才
    /// 凑巧相等。
    struct DefaultOnlyFormulas;

    impl DamageFormulaCatalog for DefaultOnlyFormulas {
        fn formula_for(
            &self,
            _explicit: Option<ll_core::ident::ContentIndex>,
        ) -> crate::formula::FormulaDef {
            crate::formula::FormulaDef {
                id: ll_core::ident::ContentIndex::default(),
                instructions: crate::formula::default_attack_power_instructions(),
                needs_rng: false,
            }
        }
    }

    #[test]
    fn 全局默认公式接入公式引擎后伤害数值与接入前逐位相同() {
        // Arrange：真实经过 DamageFormulaCatalog 这条代码路径（不是
        // NoFormulas 的短路），且没有任何武器显式声明公式（NoItems 恒
        // 让 explicit_formula 为 None）。
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        let victim = spawn_named_agent(&mut world, victim_pos, 1_000);
        // 期望伤害：接入公式引擎之前的既有实现——攻击力恒等于
        // BaseStats::BASELINE.strength，无穿透，防御为零。
        let expected_damage =
            damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);

        // Act
        let effects = resolve_with_skills_traits_pools_items_and_formulas(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
            &NoSkills,
            &NoTraitGrants,
            &NoTraits,
            &NoResourcePools,
            &NoItems,
            &DefaultOnlyFormulas,
        );
        for effect in &effects {
            crate::apply::apply(&mut world, effect);
        }

        // Assert
        let victim_after = world.actors.get(victim).expect("生命值远高于伤害,不会死亡");
        assert_eq!(victim_after.health, 1_000 - expected_damage);
    }

    #[test]
    fn 幸运更高的角色暴击命中频率更高() {
        // 频率断言，不是单次结果（见任务纪律：幸运只改变判定的概率
        // 形状，不保证任意一次攻击必然暴击/不暴击，单次断言测不出这
        // 条效果，只有在足够多次独立试验上比较命中频率才能）。用固定
        // 世界种子、固定的两个幸运值，让 `world.clock` 在一段范围内
        // 变化以取得一串不同的 `DetRng` 事件计数（见 `resolve_attack`
        // 文档「暴击」一节：三元组是 `(世界种子, 实体 ID, 世界时钟)`），
        // 统计两侧「伤害超过零暴击基准值」的次数。
        // Arrange
        let trials = 3_000i64;
        let low_luck = 5; // 5 × 5‰ = 25‰（2.5%）暴击率。
        let high_luck = 100; // 100 × 5‰ = 500‰（50%）暴击率。
        let baseline_damage =
            damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);

        let (mut low_world, _low_terrain_ids) = test_world();
        let low_attacker_pos = low_world.size.wrap(5, 5);
        let low_attacker = spawn_agent_with_luck(&mut low_world, low_attacker_pos, low_luck);
        let low_victim_pos = east_of_spawn(&low_world);
        let low_victim = spawn_named_agent(&mut low_world, low_victim_pos, 1_000_000);

        let (mut high_world, _high_terrain_ids) = test_world();
        let high_attacker_pos = high_world.size.wrap(5, 5);
        let high_attacker = spawn_agent_with_luck(&mut high_world, high_attacker_pos, high_luck);
        let high_victim_pos = east_of_spawn(&high_world);
        let high_victim = spawn_named_agent(&mut high_world, high_victim_pos, 1_000_000);

        // Act：只挪动世界时钟取得不同的随机流，不真正推进回合/不
        // `apply` 任何效果——每次试验都在同一份「满血目标」上独立重
        // 打一次，伤害是否超过基准值只取决于这一次判定是否暴击。
        let mut low_crits = 0i64;
        let mut high_crits = 0i64;
        for tick in 0..trials {
            low_world.clock = Tick(tick);
            let low_effects = resolve(
                &low_world,
                &Intent::Attack {
                    actor: low_attacker,
                    target: low_victim,
                },
            );
            if low_effects.iter().any(
                |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > baseline_damage),
            ) {
                low_crits += 1;
            }

            high_world.clock = Tick(tick);
            let high_effects = resolve(
                &high_world,
                &Intent::Attack {
                    actor: high_attacker,
                    target: high_victim,
                },
            );
            if high_effects.iter().any(
                |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > baseline_damage),
            ) {
                high_crits += 1;
            }
        }

        // Assert：50% 暴击率的一侧命中次数应远多于 2.5% 的一侧——差距
        // 留了很大的安全边际（期望值相差约 950 次，这里只要求多过
        // 100 次），避免二项分布的正常波动把测试变成偶发性失败。
        assert!(high_crits > low_crits + 100);
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
        let derived = derive_stats(
            BaseStats::BASELINE,
            &modifiers,
            &std::collections::BTreeMap::new(),
            &NoItems,
            Tick(5),
        );

        // Assert：世界时钟已达到 expires_at,回落到裸值（BASELINE 力量
        // 为 10）,不叠加 delta。
        assert_eq!(derived.attribute(AttributeKind::Strength), 10);
    }

    #[test]
    fn 不同来源的属性修正在生效值上求和而非互相覆盖() {
        // 规则①「不同效果能叠加」在 derive_stats 这一层的直接验证：
        // 两个不同来源（source_a、source_b）各自给同一属性 +5、+7，
        // 有效值必须是 base + 5 + 7，不是只看到其中一条。
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
        let derived = derive_stats(
            BaseStats::BASELINE,
            &modifiers,
            &std::collections::BTreeMap::new(),
            &NoItems,
            Tick(0),
        );

        // Assert：10（base） + 5 + 7 = 22，两条修正都参与了求和。
        assert_eq!(derived.attribute(AttributeKind::Strength), 22);
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
        let derived = derive_stats(
            BaseStats::BASELINE,
            &modifiers,
            &std::collections::BTreeMap::new(),
            &NoItems,
            Tick(10),
        );

        // Assert：只有 source_b 的 +7 参与求和，source_a 已被过滤。
        assert_eq!(derived.attribute(AttributeKind::Strength), 17);
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
                rule_modifiers: Vec::new(),
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
