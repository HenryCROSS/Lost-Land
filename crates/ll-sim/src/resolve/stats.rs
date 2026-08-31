//! `resolve::stats`：把智能体的基础属性、装备加成与生效中的临时修正，算成结算各族共用的一份 `DerivedStats`。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_world::entity::{ActiveStatModifier, AttributeKind, BaseStats};
use ll_world::temperature::Temperature;

use crate::exposure::{exposure_strength_penalty, felt_temperature};
use crate::item::{EquipSlot, ItemCatalog, ItemStack, StatTarget};

use super::{BASELINE_DEXTERITY, BASELINE_EFFECTIVE_SPEED};

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
pub(super) fn effective_speed_from_dexterity(dexterity: i32) -> u32 {
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
/// 每次 [`resolve_attack`](super::combat::resolve_attack) 结算），从不写回 [`ll_world::entity::Agent`]
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
    insulation: i32,
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

    /// 保暖绝缘值，十分之一摄氏度（温度系统批次新增）——逐件已装备
    /// 物品的 [`StatTarget::Insulation`] 求和，与 [`Self::armor`] 是
    /// 同一段算法的第二个目标（见该变体文档的 ADR 0021 一节）。
    ///
    /// 消费者是 [`crate::exposure::felt_temperature`]：`derive_stats`
    /// 自己先用它算出体感温度、把力量惩罚并进 `attributes`，随后本
    /// 访问器供调用方（以及 HUD 之类的呈现层）复查「我身上一共有多少
    /// 保暖」。
    pub fn insulation(&self) -> i32 {
        self.insulation
    }
}

/// [`AttributeKind`] 七个变体（六项主属性 + 幸运）到
/// [`DerivedStats::attributes`] 数组下标的映射——枚举变体本身没有稳定的
/// 数值表示（不依赖 `enum` 的 discriminant，那是实现细节，不是公开
/// 契约），这里显式给出，唯一的读者是 [`DerivedStats::attribute`] 与
/// [`derive_stats`] 自身。
pub(super) const fn attribute_slot(kind: AttributeKind) -> usize {
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
/// （[`resolve_attack`](super::combat::resolve_attack)）也确实不必改变调用形状（仍然是"给一个实体的
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
/// `expires_at.0 > now.0` 才算仍然生效——与 [`resolve_use_skill`](super::progression::resolve_use_skill) 冷却
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
    derive_stats_at(
        base,
        active_modifiers,
        equipment,
        items,
        now,
        Temperature::TEMPERATE_BASELINE,
    )
}

/// [`derive_stats`] 的环境感知版本：多接收一个**环境温度**，把
/// [`crate::exposure`] 的暴露惩罚作为第三条来源并进最终属性。
///
/// # 为什么是两个入口，而不是给 `derive_stats` 加一个参数
///
/// 与 [`resolve_with_skills`](super::resolve_with_skills) 之于 [`resolve_with_skills_and_quests`](super::resolve_with_skills_and_quests)
/// 是同一条既有纪律：仓库里绝大多数 `derive_stats` 调用点（单元测试、
/// 不装载任何内容表的验收 demo）根本没有空间层属性表可查，强迫它们
/// 每处都多传一个「温度这一路等于没接」的常量只是无意义的噪音。
///
/// [`derive_stats`] 因此是本函数传
/// [`Temperature::TEMPERATE_BASELINE`]（那个空对象，恒在冰点以上）的
/// 薄封装——**两条路径逐位等价**，不是「旧入口走一套旧逻辑」：
/// [`crate::exposure::exposure_strength_penalty`] 对中性温度恒返回 0，
/// 加 0 与不加在结果上不可区分。黄金基准回放（`tests/replay.rs` 走的
/// 是不带任何目录的 `resolve`）因此逐位不变，有测试钉住这条等价
/// （见 `温度这一路没接时与旧入口逐位等价`）。
///
/// # 惩罚为什么加在装备与状态效果**之后**
///
/// 绝缘值本身来自装备，必须先把装备那一轮走完才知道身上一共有多少
/// 保暖；而力量惩罚要落在「已经算完装备与 buff 的那个力量」上，才是
/// 玩家在角色面板上看到的那个数减去惩罚。顺序在这里不是可选项。
pub fn derive_stats_at(
    base: BaseStats,
    active_modifiers: &std::collections::BTreeMap<
        AttributeKind,
        std::collections::BTreeMap<ContentIndex, ActiveStatModifier>,
    >,
    equipment: &std::collections::BTreeMap<EquipSlot, ItemStack>,
    items: &dyn ItemCatalog,
    now: Tick,
    ambient: Temperature,
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
    let mut insulation = 0;

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
                // 与上一行逐字同形——绝缘值是同一段累加算法的第三个
                // 目标，不是另起的一条通道，见 `StatTarget::Insulation`
                // 文档的 ADR 0021 一节。
                StatTarget::Insulation => insulation += bonus.amount,
            }
        }
    }

    // 第三条来源：极端环境暴露。体感温度在冰点以上时恒为 0，与温度
    // 这一路完全没接线逐位等价，见本函数文档与 `crate::exposure`
    // 模块文档「只在极端条件下产生后果」一节。
    let penalty = exposure_strength_penalty(felt_temperature(ambient, insulation));
    attributes[attribute_slot(AttributeKind::Strength)] -= penalty;

    DerivedStats {
        attributes,
        armor,
        insulation,
    }
}
