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
use ll_world::temperature::Temperature;

use crate::catalogs::ResolveCatalogs;
use crate::check::{CHECK_DICE, CONCEALMENT_CHECK, CRITICAL_CHECK, CheckSide, opposed_check};
use crate::combat::{
    Penetration, apply_crit_multiplier, crit_attacker_modifier, damage_after_defense,
    sneak_attack_chance_permille,
};
use crate::craft::{NoRecipes, RecipeCatalog, RecipeRule};
use crate::damage_category::{DamageCategoryCatalog, NoDamageCategories};
use crate::effect::{CarriedItemSlot, Effect, InspectedItem};
use crate::experience::{ExperienceCatalog, NoExperience};
use crate::exposure::{AmbientSource, exposure_strength_penalty, felt_temperature};
use crate::formula::{
    DamageFormulaCatalog, FormulaInputs, NoFormulas, attribute_modifier, eval_formula,
};
use crate::intent::{Direction, Intent};
use crate::item::{
    EquipSlot, ItemCatalog, ItemStack, NoItems, StatTarget, WearChannels, can_merge,
    conflicting_anchors, equip_mask_of, merge_stacks,
};
use crate::quest::{NoQuests, QuestCatalog};
use crate::resource_pool::{
    NoResourcePools, RegenRule, ResourcePoolCatalog, ResourcePoolShape, RestRecoveryAmount,
    effective_scalar_capacity, effective_slot_tier_capacity,
};
use crate::rule_modifier::{
    agent_rule_modifiers, check_reroll_value, check_roll_bias, concealment_check_modifier,
    craft_product_count, craft_yield_bonus, damage_after_resistance, resistance_damage_reduction,
    sneak_attack_rule, vulnerability_damage_increase,
};
use crate::skill::{NoSkills, ResourceCost, SkillCatalog, SkillEffect};
use crate::skill_overview::SkillTreeCatalog;
use crate::subclass::{NoSubclassUnlocks, SubclassUnlockCatalog, craft_progress_effects};
use crate::timeline::action_cost;
use crate::traits::{
    NO_TRAIT_GRANTS, NoTraitGrants, NoTraits, TraitCatalog, TraitGrantSource, agent_trait_sources,
    effective_traits, granted_skills,
};

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
/// 与 [`resolve_with_skills`] 之于 [`resolve_with_skills_and_quests`]
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
/// （[`resistance_damage_reduction`]）只要防御方的天赋声明了
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
        Intent::PickUp { actor } => resolve_pick_up(world, actor, items),
        Intent::Loot { actor } => resolve_loot(world, actor, items),
        Intent::Drop { actor, def } => resolve_drop(world, actor, def),
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
    append_corpse_drop(world, &mut effects);
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
/// [`Effect::ScheduleNext`]（连撞墙都算一次行动，见 [`resolve_move`]
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
fn resolve_allocate_attribute_point(
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
fn resolve_learn_skill(
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
/// **下一次制作就过不去了**（[`resolve_craft`] 第③步每次都判），而
/// 已经通过它学会的技能不受影响。两种闸门语义的差异见
/// [`Effect::RemoveSubclass`] 文档。
fn resolve_abandon_subclass(
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
/// [`resolve_craft`] 的全部失败分支都返回**空** `Vec`（查不到行动者、
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
fn append_craft_progress(
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
fn resolve_resource_pool_regen(
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
    )
}

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
fn resolve_rest(
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
fn scalar_rest_effect(
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

/// [`Intent::Inspect`] 的结算：读 `target` 此刻背包与已装备的全部
/// 物品定义，打成一份快照，产出 [`Effect::Inspect`]——卫兵职业接线
/// 批次唯一的产出者，见该效果文档「为什么 apply 不把它写进
/// WorldState::history」一节。
///
/// `actor`/`target` 任一方已经不在 `world.actors`（同一批结算里被
/// 更早的效果销毁，或调用方给的句柄已经过期）都返回空 `Vec`——与本
/// 文件其余 `resolve_*` 同一条既有纪律（见 [`resolve`] 文档）。
///
/// # 不做任何合法性判断
///
/// `Owner`/`stolen_marker` 尚未落地（见 `Effect::Inspect` 文档「为什么
/// 没有任何是否违法的判断」一节引用的设计文档）——本函数只如实读出
/// `target` 此刻持有的物品定义列表，不比较、不裁定，"这堆东西是不是
/// `target` 自己的"这个问题本批次回答不了。
///
/// # 谁来判断"该不该发起这次盘查"
///
/// 不是本函数——是否发起盘查（卫兵职业、视野内是否有目标、这一次的
/// 概率判定）全部在 AI 决策阶段（行为树脚本，`ll_script::api::rng`
/// 的 `rng-chance` 原语）完成，`Intent::Inspect` 一旦产出，本函数
/// 恒执行、不重新判断"该不该查"——与 `resolve_attack` 不重新判断
/// "这一下该不该打"是同一条既有分工：决策在决定要不要产生这个
/// `Intent` 的那一步，`resolve` 只负责把已经决定要做的事翻译成
/// `Effect`。
///
/// # 盘查消耗一个回合（进展保证，不是手感取舍）
///
/// 本函数产出的第二条效果是 [`Effect::ScheduleNext`]，与
/// [`resolve_toggle_stealth`]/[`resolve_move`] 完全同一种算法
/// （`action_cost(BASE_ACTION_COST, 有效速度)`）。
///
/// 它此前**没有**这一条，那是一个真实缺陷，只是因为 `Intent::Inspect`
/// 至今没有任何调用方经由 [`crate::turn::TurnEngine`] 产出过而一直没有
/// 暴露：`TurnEngine::perform` 结算完一次行动后按 `Agent::next_action_at`
/// 把行动者重新排回时间轴，而没有 `ScheduleNext` 就意味着这个字段原地
/// 不动——同一条时间轴记录会在**同一个 tick** 被立刻再弹出，行为树又
/// 因为世界时钟没变而抽到同一个随机数、作出同一个决策，直到耗尽
/// `MAX_STEPS_PER_ADVANCE` 才放弃。这正是
/// [`crate::turn::TurnEngine::advance_ai`] 文档「必须保证进展（曾经的
/// 真实死循环）」一节描述的那条死循环，只是这一次的成因不在 AI 策略侧
/// 而在 `resolve` 侧。
///
/// 发现它的方式就是把卫兵行为树真的接上回合引擎跑一遍——「接线断在
/// 最后一环」这类缺陷只有在真的把线接上之后才会暴露下一环。
///
/// # 藏匿判定（盗贼被动两分批次）
///
/// 所有者裁定「被动可以分为 **2 种**，**不觉得可疑**，还有**查不出
/// 东西**」——后一种落在本函数：盘查照常发起、照常消耗一个回合，
/// 只是 `items_seen` 里被藏起来的那些物品不再出现。判据是
/// **`target` 自己**（不是盘查者）身上聚合出的
/// [`crate::rule_modifier::RuleModifier::InspectionConcealment`]，走
/// [`crate::rule_modifier::agent_rule_modifiers`] 这个唯一聚合点，
/// 与 [`resolve_attack`] 读偷袭声明是同一条既有路径。
///
/// 逐件掷骰，不是一次判定决定整份快照——形状的完整论证见
/// [`crate::rule_modifier::RuleModifier::InspectionConcealment`] 文档「为什么是逐件掷骰」
/// 一节。
///
/// # 换成对抗判定（判定系统落地批次）
///
/// 每一件物品掷的不再是一次「藏匿千分比」的硬币，而是一次**对抗
/// 判定**（[`crate::check::opposed_check`]，`3d20 + 修正` 双方各一轮）：
///
/// ```text
/// 盘查者（主动）：意志调整值            察觉
/// 被盘查者（被动）：敏捷调整值 + 藏匿修正   隐蔽
/// ```
///
/// 主动方赢下这一件，这一件才留在 `items_seen` 里。
///
/// **「察觉 = 意志调整值、隐蔽 = 敏捷调整值」是项目所有者的裁定**，
/// 不是本函数发明的映射；本仓库没有独立的感知属性，
/// [`ll_world::entity::AttributeKind::Willpower`] 是六项里承担 D&D
/// 「感知」概念的那一项（见其字段文档与
/// [`crate::formula::FormulaOperand::AttributeModifier`] 对 `wis-mod`
/// 的同一条说明）。调整值公式 `(属性 − 10) / 2` 复用
/// [`crate::formula::attribute_modifier`]，零新增字段、零存档影响。
///
/// 换掉的是什么：旧形状里搜身的人是谁完全不影响结果——一个眼神再好
/// 的卫兵与一个瞎子查同一个人，查到的东西逐位相同。对抗判定把盘查者
/// 放回了式子里。
///
/// 数值后果（`3d20`，双方属性均为基准 10 因而两侧调整值均为 0，
/// 天赋声明 9 点即半颗骰子）：这一件被藏住的概率从旧值 `800‰` 变成
/// `745‰`（主动方赢面 `255‰`）——同一档，但不再是一个与任何人无关的
/// 常数。旧的 `800‰` 本身是概率模型时代的自由参数，本批次不逐字复刻
/// 它，改用骰子量尺上有内在依据的「半颗骰子」，见
/// [`crate::check::CheckDice::half_die`]。
///
/// 槽位句柄批次把 `items_seen` 的元素从裸 `ContentIndex` 换成
/// [`crate::effect::InspectedItem`]（种类 + 位置），**这一步的粒度一个
/// 字都没变**：`retain` 仍然是一条记录一次掷骰，一条记录仍然对应一堆
/// 物品。取数次数因此与换形状之前逐位相同（同一份快照、同样的元素
/// 个数、同样的顺序），既有的确定性断言与那条「出现过查到一部分的中间
/// 结果」的端到端证据（`crates/ll-mod/tests/example_mod_rogue_passives.rs`）
/// 都不需要跟着改（换成对抗判定之后取数**次数**变了——每件从 1 次
/// 变成 `2M` 次——但取数的**粒度与顺序**仍然逐字相同：一条记录一次
/// 判定，顺序仍是快照自身的顺序）。真正被这次换形状加强的是**下游**：那条被动当初就是
/// 照着「逐堆比对归属」的粒度选的（见上述变体文档），而在旧形状里
/// 「逐堆」根本表达不出来。
///
/// **约束 C3**：随机走 `DetRng::for_entity(世界种子, 实体 ID, 事件
/// 计数)`，三元组的中间一项取 **`target`**（藏东西的那一方，判定属于
/// 它的被动，不属于盘查者），事件计数用一个与本文件其余流都不同的
/// 固定标签异或世界时钟——与 [`resolve_attack`] 里暴击/骰子/偷袭三条
/// 流互不相同是同一套取法。
///
/// **约束 C5**：取数顺序就是 `items_seen` 自身的顺序（先背包原始
/// 顺序、后装备槽位升序，两者都不触碰任何 `HashMap`）。没有任何来源
/// 声明藏匿时（`concealment_check_modifier` 返回 `None`）**完全不构造
/// 这条流**，与
/// [`resolve_attack`] 「没有声明偷袭就不构造额外 `DetRng` 流」同一条
/// 既有纪律：每次判定都是现场造流、只取要用的那几个数,不是一条跨
/// 调用累进的长流,因此「这次没取数」不会让后续任何取数错位。
///
/// # 为什么不在这里判断「盘查该不该发起」
///
/// 被动①（「不觉得可疑」，[`crate::rule_modifier::RuleModifier::InspectionSuspicion`]）
/// **不在本函数**——它减的是行为树掷骰那一步，见本函数文档上一节
/// 「谁来判断该不该发起这次盘查」与该变体自己的文档。两个被动分别
/// 落在链路的两环，是所有者裁定「分为 2 种」的直接落地。
/// `#[allow(clippy::too_many_arguments)]`：多出来的那一个是副职天赋
/// 接线批次新增的第三路天赋来源（`subclass_traits`）。它与
/// `race_traits`/`class_traits` 是并列的同一类依赖，打包成一个中间
/// 类型只会在这条转发链上多一层拆包——理由同本文件其余几处同款豁免。
#[allow(clippy::too_many_arguments)]
fn resolve_inspect(
    world: &WorldState,
    actor: EntityId,
    target: EntityId,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(target_agent) = world.actors.get(target) else {
        return Vec::new();
    };
    // 每条记录带着「是什么」+「在哪」两半——背包那一半的「在哪」是
    // 下标，装备那一半是真实存储键（锚点槽位），见
    // `crate::effect::CarriedItemSlot` 文档。下标转 u32 不会截断：
    // 见该类型文档「为什么下标是 `u32`」一节。
    let mut items_seen: Vec<InspectedItem> = target_agent
        .inventory
        .iter()
        .enumerate()
        .map(|(index, stack)| InspectedItem {
            def: stack.def,
            slot: CarriedItemSlot::Inventory {
                index: index as u32,
            },
        })
        .collect();
    items_seen.extend(
        target_agent
            .equipment
            .iter()
            .map(|(slot, stack)| InspectedItem {
                def: stack.def,
                slot: CarriedItemSlot::Equipped { slot: *slot },
            }),
    );
    // 藏匿判定，见本函数文档「藏匿判定」一节。
    const INSPECT_CONCEAL_EVENT_TAG: u64 = 0x0C0A_1EA0_0000_0000;
    let target_modifiers = agent_rule_modifiers(
        target_agent,
        race_traits,
        class_traits,
        subclass_traits,
        traits,
        items,
    );
    // 一条也没有声明 → 完全跳过判定，一次抽取都不消耗（约束 C3），见
    // `concealment_check_modifier` 文档「缺省与声明 0」。显式声明成
    // `0` 是另一回事：判定照常发生，只是这一路贡献 0 点。
    if let Some(concealment) = concealment_check_modifier(&target_modifiers) {
        // 双方的属性调整值走 `derive_stats`（**派生值**，装备与状态
        // 效果加的属性在这里生效），不是裸 `BaseStats`——与
        // `resolve_attack` 读 `attacker_derived.attribute(..)` 同一条
        // 既有纪律。
        //
        // 用不带环境温度的 `derive_stats`（内部代入
        // `Temperature::TEMPERATE_BASELINE`）而不是 `derive_stats_at`：
        // 本函数没有 `ambient` 参数，而温度**只**惩罚力量一项
        // （`derive_stats_at` 里那一行 `attributes[Strength] -= penalty`），
        // 对本判定读的意志/敏捷两项逐位无影响。这不是将就，是这两项
        // 上两个函数确实等价。
        let inspector_derived = derive_stats(
            agent.stats,
            &agent.active_stat_modifiers,
            &agent.equipment,
            items,
            world.clock,
        );
        let target_derived = derive_stats(
            target_agent.stats,
            &target_agent.active_stat_modifiers,
            &target_agent.equipment,
            items,
            world.clock,
        );
        let inspector_modifiers = agent_rule_modifiers(
            agent,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
            items,
        );
        // 察觉 = 意志调整值，隐蔽 = 敏捷调整值（项目所有者裁定）。
        let active = CheckSide {
            modifier: attribute_modifier(inspector_derived.attribute(AttributeKind::Willpower)),
            bias: check_roll_bias(&inspector_modifiers, CONCEALMENT_CHECK),
            reroll_on: check_reroll_value(&inspector_modifiers),
        };
        let passive = CheckSide {
            modifier: attribute_modifier(target_derived.attribute(AttributeKind::Dexterity))
                .saturating_add(i64::from(concealment)),
            bias: check_roll_bias(&target_modifiers, CONCEALMENT_CHECK),
            reroll_on: check_reroll_value(&target_modifiers),
        };
        let mut conceal_rng = ll_core::rng::DetRng::for_entity(
            world.seed,
            target.as_u64(),
            (world.clock.0 as u64) ^ INSPECT_CONCEAL_EVENT_TAG,
        );
        // 逐件一次对抗判定：搜身的人赢下这一件才看得见它。取数顺序
        // 就是 `items_seen` 自身的顺序，每件消耗 `2M`（含优劣势时
        // `4M`、含重掷时更多）个抽取，见 `crate::check` 模块文档。
        items_seen.retain(|_| {
            opposed_check(&CHECK_DICE, &active, &passive, &mut conceal_rng).active_wins()
        });
    }
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![
        Effect::Inspect {
            inspector: actor,
            target,
            items_seen,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
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

/// [`Intent::ToggleStealth`] 的结算（潜行与盗贼被动批次）：读一次
/// 发起者当前的 [`ll_world::entity::Agent::stealthed`]，产出取反后的
/// 确定值，并按 [`BASE_ACTION_COST`] 消耗一个回合。
///
/// # 为什么切换本身要计费
///
/// 见 [`Intent::ToggleStealth`] 文档「为什么消耗一个回合」：不计费的话
/// 「每走一格之前开、走完立刻关」可以白嫖潜行的全部收益而完全绕开
/// 它唯一的代价（[`STEALTH_MOVE_COST_PERMILLE`] 的移动开销上升）。
/// 计费口径与 [`resolve_wait`] 完全相同（基础代价 × 敏捷速度），不是
/// 另起一个数字：切换姿态在时间轴上就是「这一回合我没干别的」。
///
/// # 为什么不检查任何前置条件
///
/// 没有可检查的东西：潜行不消耗资源、不要求地形、不要求技能解锁。
/// 与 [`resolve_pick_up`]「脚下没东西就静默作废」那类需要读世界才能
/// 判断的意图不同，本意图恒合法——唯一的失败路径是发起者根本不存在
/// （已被同一批效果里更早的 `Effect::Kill` 收走），那一条走本文件
/// 统一的「查不到实体就返回空效果、不消耗时间」既有降级。
fn resolve_toggle_stealth(world: &WorldState, actor: EntityId) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![
        Effect::SetStealth {
            actor,
            stealthed: !agent.stealthed,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// [`Intent::Craft`] 结算（制作系统批次，`knowledge/design/crafting-system.md`
/// 五节）：校验三道前置与食材是否齐全，齐全就逐条产出消耗效果、把成品
/// 并进背包，并按一次普通行动计费。
///
/// # 判定顺序本身是设计决定
///
/// ```text
/// 1. 查 agent，查不到 → 空
/// 2. 查配方，查不到 → 空                          （ADR 0015：未注册当作没有）
/// 3. 副职闸门：类别要求非空且与 agent.subclasses 无交集 → 空
/// 4. 已知闸门：配方声明 requires_discovery 且不在 known_recipes 里 → 空
/// 5. 场地前置：required_station 与脚下地形不符 → 空
/// 6. 工具前置：没有「def 匹配且耐久未归零」的已装备物品 → 空
/// 7. 食材校验：任意一条数量不够 → 空（不消耗任何食材）
/// 8. 逐条食材产出 Effect::ConsumeInventoryItem，重复 count 次
/// 9. 成品并进背包（Effect::MergeIntoInventory）
/// 10. 工具磨损（Effect::AdjustEquipmentDurability，工具带耐久时才产出）
/// ```
///
/// 四道前置（3/4/5/6）排在食材校验（7）之前，是因为**前四道回答的是
/// 「你能不能做这件事」，第五道回答的是「你现在够不够料」**。虽然本
/// 批次不设计任何 UI，判定顺序决定了将来制作界面能拿到的失败原因的
/// 优先级——玩家更需要先知道「我不会锻造」而不是「你缺两块铁锭」。
///
/// # 第 4 道闸门：配方发现（配方发现批次新增）
///
/// 项目所有者裁定「菜谱就是通过随机丢入东西煮获取或者阅读书籍的时候
/// 获取」，本步是那句话在制作侧的执行者。它**推翻了**
/// `food-and-cooking-system.md` 五节「菜谱全部已知、不设解锁门槛」那条
/// 裁定（更正记录写在该文档五节末尾，原文未删）。
///
/// 排在副职闸门**之后**、场地/工具**之前**，与那句「能不能 vs 够不够」
/// 的分界一致：「我不会这张图纸」和「我不是工匠」同属「你能不能做这件
/// 事」，而它比「我不是工匠」更具体，因此排后一位——玩家已经是工匠却
/// 做不出某条配方时，「你还没学会这张图纸」才是他真正需要的那条信息。
///
/// 声明 `requires_discovery == false` 的配方（既有全部内容的默认值）
/// 完全跳过本步，本函数对它们与本批次之前逐字节等价。两条把配方写进
/// `Agent::known_recipes` 的发现路径见 [`resolve_read`] 与
/// [`resolve_experiment`]。
///
/// # 全程静默失败
///
/// 任何一步不满足都返回空 `Vec<Effect>`：不产出效果、不消耗食材、
/// 不推进时间轴——与 [`resolve_use_skill`] 资源不足时静默不产出效果、
/// [`resolve_drop`]/[`resolve_equip`] 查不到物品时静默无效是同一条既有
/// 纪律。
///
/// # 坏掉的工具不算装着
///
/// 工具判定的谓词是 `def == required_tool && durability != Some(0)`，
/// **不是只比 `def` 相等**。`item-system.md` 六节裁定「耐久归零 = 损坏
/// 不可用」，[`derive_stats`] 遍历装备时已经对 `durability == Some(0)`
/// 的堆直接跳过（见其文档「耐久归零」一节）——工具前置若只比 `def`，
/// 会出现「锤子已经烂了但还能打铁」这个与既有耐久语义直接矛盾的漏洞。
///
/// # 工具磨损（耐久扩面批次）
///
/// 项目所有者原话：「修理锤子也算是一种武器，也可以是带有功能性的
/// 物品。**只要使用就会减少耐久**。」——制作正是「使用工具」这件事在
/// 本引擎里唯一已经落地的形态，本函数因此在第 9 步产出一条
/// [`crate::effect::Effect::AdjustEquipmentDurability`]，让被配方点名
/// 的那件工具损失 [`TOOL_DURABILITY_LOSS_PER_CRAFT`] 点耐久。
///
/// 这正是 `crafting-system.md` 九节⑩「工具因制作而磨损」——该表当时
/// 把它标为「与所有者『只有装备武器才有耐久』的裁定直接冲突」而推迟。
/// **那条裁定已被推翻**（见上面的原话与
/// [`resolve_attack`] 文档「耐久消耗：两条通道」一节），⑩ 的唯一阻碍
/// 因此消失，本批次把它落地。
///
/// ## 只在制作**真的发生**时磨损
///
/// 效果排在全部前置与食材校验之后——任何一步不满足时本函数早已
/// `return Vec::new()`，工具一点耐久都不掉。「白试一次也磨损」既不
/// 符合「只要使用就会减少耐久」这句话（没做成就不算用过），也会让
/// 「站错地方点了一下制作」这种纯操作失误产生真实损失。
///
/// ## 两个条件缺一不可：带耐久，且带 `on-use` 标签
///
/// 判据与 [`resolve_attack`] 的「使用」通道逐字相同：
/// `ItemStack.durability.is_some()` 回答「这一件还有多少耐久」，
/// [`crate::item::ItemRule::wear_channels`] 含
/// [`WearChannels::ON_USE`] 回答「这类东西用了会不会磨损」。内容作者
/// 因此可以声明一件永不磨损的工具——不给它填耐久上限（`-1`），或者
/// 给它挂一个不声明任何磨损通道的纯分类标签。「哪些物品该有耐久、
/// 该磨损」自此完全是内容决策，见
/// `ll_mod::script_item_api::register_item_equip_mask` 与
/// `ll_mod::script_tag_api::register_tag` 两处文档。
///
/// ## 归零之后制作**失败**
///
/// 由第 6 步的既有谓词 `durability != Some(0)` 保证，本节不新增任何
/// 判定：磨到零的锤子从此打不了铁，直到修理系统把它修回正数。这条
/// 与本节的磨损产出构成一个闭环——工具会用坏，用坏了就不能用，正是
/// 「耐久」这个词的全部含义。反例测试见
/// `ll-mod/tests/example_mod_crafting.rs`
/// 「耐久归零的工具装着也打不了铁」。
///
/// ## 为什么第 6 步改成 `find` 而不是 `any`
///
/// 产出效果需要工具的**存储键**（[`crate::effect::Effect::AdjustEquipmentDurability`]
/// 按槽位定位），`any` 只回答"有没有"、拿不到键。改成 `find` 之后
/// 判据一字未改，只是把找到的那一条留了下来。
///
/// # 成品的耐久（第 9 步）
///
/// 成品是**刚造出来的**，耐久等于它那条定义声明的上限——走
/// [`ItemStack::freshly_made`] 那条共同规则，与盲盒产出
/// （[`resolve_identify`]）用的是同一个构造器。没有耐久概念的成品
/// （烤肉、铁铆钉这类材料/消耗品）仍然是 `None`，因为它们的
/// `max_durability` 本来就是 `None`。
///
/// 这一行此前是 `ItemStack::new(rule.product, rule.product_count)`
/// ——恒 `None`：工匠打出来的铁短剑耐久是"没有耐久概念"而不是 120，
/// 从此永不磨损。那是一条真实缺陷，不是设计，完整论证见
/// [`ItemStack::freshly_made`] 文档。
///
/// **`product_count > 1` 且成品带耐久**是一个内容层面的病态组合（一堆
/// `count` 为 N 的装备共用一份耐久）。本函数**不新增**运行期分支拦它,
/// 理由是这条组合的病态与耐久无关：改动之前它同样产出一堆 `count` 为
/// N、`stack_limit` 却是 1 的装备（带耐久的物品必然 `stack_limit == 1`,
/// 注册期硬校验），只是那时耐久恰好是 `None`。本改动没有让它更坏,也
/// 没有资格在这里替内容作者做「一次只能造一件装备」这条裁定。本体九条
/// 配方里没有这种组合——唯一 `product_count > 1` 的 `iron_rivet_batch`
/// 产的是可堆叠、无耐久的铁铆钉。
///
/// # 产出加成接线（制作类副职奖励批次）
///
/// 第 9 步的件数不是配方声明的 `product_count`，而是它经
/// [`crate::rule_modifier::craft_yield_bonus`] 加成、再经
/// [`crate::rule_modifier::craft_product_count`] 保底之后的结果。这是
/// 四条制作类副职（工匠/裁缝/炼金术士/厨师）「拿到之后给什么」的唯一
/// 落点，完整设计见 `knowledge/design/crafting-subclass-rewards.md`。
///
/// 闭环因此成立：**做够 N 件锻造品 → 得到工匠副职**（第 3 步的闸门与
/// [`crate::subclass::craft_progress_effects`] 那条既有计数）**→ 此后
/// 每件锻造品多出一件**。挂钩的动作与被奖励的动作是同一个动作。
///
/// ## 加成来自哪四路
///
/// 本函数多出的四个 `&dyn` 参数就是为这一步取的，它们**不是新增依赖**
/// ——`resolve_dispatch` 的参数表里本来就有这一组（`resolve_attack`/
/// `resolve_inspect` 已经各接一份），本步只是把它们再往下传一层。
/// [`crate::rule_modifier::agent_rule_modifiers`] 把种族/职业/副职三路
/// 天赋与**装备**汇成一份候选，因此「大师级铁砧锤」这件装备携带同一条
/// 修正是白拿的。
///
/// ## 对不带这条天赋的行动者逐位不变
///
/// 一条也没命中时 `craft_yield_bonus` 返回 `0`，而
/// `craft_product_count(n, 0) == n`——本步对既有内容与既有存档的可观察
/// 结果一个字节都没变。
///
/// ## 产出恒 ≥ 1
///
/// [`crate::rule_modifier::MINIMUM_CRAFT_PRODUCT_COUNT`]。加成允许为负
/// （「手艺生疏」这类负面天赋，与抗性允许「脆弱」同一条先例），但
/// 「消耗了材料却什么都没拿到」在机制层面不可能发生——那正是本函数
/// 「全程静默失败」一节之外、`crafting-system.md` 九节⑤在玩法上否决过
/// 的「制作失败」。
///
/// # 约束核对
///
/// - C3（随机全部来自 `DetRng::for_entity`）：不涉及，本函数全程零
///   随机。产出加成接线**没有引入第一次掷骰**——`craft_yield_bonus`
///   是一次纯查表聚合，随机流的取数顺序完全不受影响。制作失败判定
///   标为将来扩展，见设计文档九节⑤。
/// - C5（逻辑决策不得依赖哈希表迭代顺序）：满足。第 6 步遍历的
///   `agent.equipment` 是 `BTreeMap`（有序），第 7/8 步遍历的
///   `recipe.ingredients`/`agent.inventory` 都是 `Vec`（保序）。
/// - C1/C2/C4：不涉及（不新增脚本状态跨帧持有、不进时间轴队列、
///   不改后台推进）。
///
/// # 已知边界（继承自 `food-and-cooking-system.md` 四节，如实重复）
///
/// 第 7 步只认**第一条** `def` 匹配的堆，不跨多堆合并计数：背包里两堆
/// 各 1 个铁锭时，需要 2 个铁锭的配方会判定为「料不够」。第 9 步若
/// `product_count` 大到需要三堆以上，[`Effect::MergeIntoInventory`] 的
/// `resulting` 目前的「最多两条」语义装不下。两条都只在数量远超
/// `stack_limit` 时才失真。
///
/// # 行动开销
///
/// 一次制作 = 一次普通行动，`action_cost(BASE_ACTION_COST, speed)`，
/// 与 [`resolve_wait`]/[`resolve_use_item`] 完全相同的计费，不新增任何
/// 常量。「打一把剑应该比切一块肉久」需要一套可中断的多回合活动机制，
/// 引擎目前没有，做成「时间轴直接前进 2000」是一个明显错误的中间态，
/// 见设计文档五节。
#[allow(clippy::too_many_arguments)]
fn resolve_craft(
    world: &WorldState,
    actor: EntityId,
    recipe: ContentIndex,
    recipes: &dyn RecipeCatalog,
    items: &dyn ItemCatalog,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Vec<Effect> {
    // ① 行动者。
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // ② 配方。
    let Some(rule) = recipes.recipe(recipe) else {
        return Vec::new();
    };
    // ③ 副职闸门——any-of：空列表即人人可做。
    let required_subclasses = recipes.category_required_subclasses(rule.category);
    if !required_subclasses.is_empty()
        && !required_subclasses
            .iter()
            .any(|needed| agent.subclasses.contains(needed))
    {
        return Vec::new();
    }
    // ④ 已知闸门（配方发现批次）——只对声明了 requires_discovery 的
    // 配方生效，见本函数文档「第 4 道闸门」一节。默认 false 的既有配方
    // 一律直接通过，这一步对它们是零成本的一次 bool 判断。
    if rule.requires_discovery && !agent.known_recipes.contains(&recipe) {
        return Vec::new();
    }
    // ⑤ 场地前置——「站在这格上」，一次 terrain_at，与 resolve_move 同款。
    if let Some(station) = rule.required_station
        && world.terrain_at(agent.pos).map(|kind| kind.index()) != Some(station)
    {
        return Vec::new();
    }
    // ⑥ 工具前置——装备着且耐久未归零，见本函数文档「坏掉的工具」一节。
    // 用 `find` 而不是 `any`：第 10 步磨损需要这件工具的存储键，判据本身
    // 一字未改，见本函数文档「为什么第 6 步改成 `find`」一节。
    // `equipment` 是 `BTreeMap`（有序），同一件工具被装在多个槽位这种
    // 情形下取哪一条是确定的（约束 C5）。
    let mut equipped_tool: Option<EquipSlot> = None;
    if let Some(tool) = rule.required_tool {
        let found = agent
            .equipment
            .iter()
            .find(|(_, stack)| stack.def == tool && stack.durability != Some(0));
        match found {
            None => return Vec::new(),
            // 第 10 步只对「带耐久」**且**「带 `on-use` 标签」的工具记下
            // 槽位——两个条件缺一不可，见本函数文档「工具磨损」一节。
            // 判据与 `resolve_attack` 的「使用」通道逐字相同：一件东西
            // 用了会不会磨损，由它带的标签回答，不由它是工具还是武器、
            // 挂在哪个槽位回答。
            Some((&slot, stack)) if stack.durability.is_some() => {
                if items
                    .item(stack.def)
                    .is_some_and(|tool_rule| tool_rule.wear_channels.contains(WearChannels::ON_USE))
                {
                    equipped_tool = Some(slot);
                }
            }
            Some(_) => {}
        }
    }
    // ⑦ 食材校验——全部齐全才继续，缺任意一条都不消耗任何东西。判定
    // 与 resolve_experiment 第③步共用同一段（见 has_all_ingredients
    // 文档：共享的不只是循环，还有「只认第一条匹配堆」那条已知边界）。
    if !has_all_ingredients(agent, &rule) {
        return Vec::new();
    }

    // ⑧ 逐条产出消耗效果。`Effect::ConsumeInventoryItem` 恒扣一（见其
    // 文档「为什么没有 amount 字段」），要扣 N 个就产出 N 条——与
    // resolve_use_item 产出单条是同一个效果，只是重复次数不同。
    let mut effects: Vec<Effect> = Vec::new();
    for ingredient in &rule.ingredients {
        let durability = agent
            .inventory
            .iter()
            .find(|stack| stack.def == ingredient.item)
            .and_then(|stack| stack.durability);
        for _ in 0..ingredient.count {
            effects.push(Effect::ConsumeInventoryItem {
                actor,
                def: ingredient.item,
                durability,
            });
        }
    }

    // ⑨ 成品并进背包，复用 pick_up/equip/unequip 三处已经共用的那段
    // 「找可合并的旧堆 → 算合并结果」逻辑。成品是**刚造出来的**，耐久
    // 走 `ItemStack::freshly_made` 那条共同规则（满耐久；没有耐久概念
    // 的成品仍是 `None`），见本函数文档「成品的耐久」一节。
    // 查不到成品定义时按「没有耐久概念」处理，与本函数其余
    // `items.item(...)` 查询同一条「查不到就是查不到」纪律（ADR 0015）。
    //
    // 件数不再直接取 `rule.product_count`：制作类副职（工匠/裁缝/炼金
    // 术士/厨师）的天赋走 `RuleModifier::CraftYield` 在这里加成，见本
    // 函数文档「产出加成接线」一节。一条也没命中时 `craft_yield_bonus`
    // 返回 0，`craft_product_count(n, 0) == n`，对不带这条天赋的行动者
    // 与本批次之前逐位相同。
    let product_max_durability = items.item(rule.product).and_then(|def| def.max_durability);
    let product_count = craft_product_count(
        rule.product_count,
        craft_yield_bonus(
            &agent_rule_modifiers(
                agent,
                race_traits,
                class_traits,
                subclass_traits,
                traits,
                items,
            ),
            rule.category,
        ),
    );
    effects.push(merge_into_inventory_effect(
        agent,
        actor,
        ItemStack::freshly_made(rule.product, product_count, product_max_durability),
        items,
    ));

    // ⑩ 工具磨损——制作确实发生了才走到这里，见本函数文档「工具磨损」
    // 一节。`equipped_tool` 只在「配方点名了工具、身上确实装着一件没坏
    // 的、且它带耐久」三条同时成立时才是 `Some`。
    if let Some(slot) = equipped_tool {
        effects.push(Effect::AdjustEquipmentDurability {
            actor,
            slot,
            delta: -TOOL_DURABILITY_LOSS_PER_CRAFT,
        });
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

/// [`Intent::Read`] 结算（配方发现批次）：读背包里那件东西声明教授的
/// 全部配方里，把行动者**还不知道的那些**写进
/// [`ll_world::entity::Agent::known_recipes`]，并按一次普通行动计费。
///
/// # 判定顺序
///
/// ```text
/// 1. 查 agent，查不到 → 空
/// 2. 背包里没有这一种东西 → 空
/// 3. 查不到物品规则 → 空                （ADR 0015：未注册当作没有）
/// 4. taught_recipes 为空（这件东西不可读）→ 空
/// 5. 逐条过滤掉已经知道的，一条不剩 → 空（「这本书我读透了」）
/// 6. 逐条产出 Effect::LearnRecipe
/// 7. study_experience > 0 时追加一条 Effect::GrantExperience，并按一次
///    普通行动计费
/// ```
///
/// # 书**不**被消耗
///
/// 与 [`resolve_use_item`] 最本质的一条差别（完整的逐步对照见
/// [`Intent::Read`] 文档「为什么是新变体」一节的那张表）：本函数
/// **一条 [`Effect::ConsumeInventoryItem`] 都不产出**。读完一本书，书
/// 还在背上——这既是物理直觉，也让「把书传给同伴读」这件事无需任何额外
/// 机制就能成立。
///
/// # 第 5 步：读透了的书不再消耗回合
///
/// 全部条目都已知时返回空 `Vec`，因此**连时间都不推进**。这不是「静默
/// 吞掉一次操作」，而是与 [`resolve_pick_up`]「脚下没东西就静默作废」
/// 完全同一条既有纪律：一次不可能产生任何结果的行动不该收费。它同时
/// 关掉了一条真实的刷取路径——经验产出（第 7 步，研究经验收窄批次已经
/// 挂上）就挂在这道闸门后面：若这一步产出效果或推进时间，反复读同一本
/// 书就会变成一台经验机器。
///
/// # 为什么效果恒施于发起者自身
///
/// 与 [`Intent::Use`]/[`Intent::Read`] 文档同一条范围裁定：读书的是
/// 自己，没有「读给别人听」这个真实场景需要表达。
///
/// # 约束核对
///
/// - C1：只产出 `Vec<Effect>`，一个字节的世界状态都不写。
/// - C3：全程零随机（与 [`resolve_experiment`] 相反，见其文档）——
///   一本书教什么是内容作者写死的事实，没有任何可掷骰的地方。
/// - C5：唯一遍历的两个容器是 `rule.taught_recipes` 与
///   `agent.known_recipes`，都是 `Vec`（保序），不涉及
///   `HashMap`/`HashSet`。
///
/// # 第二个钩子已经挂上：研读经验（研究经验收窄批次）
///
/// 项目所有者把研究类经验**收窄**成两条来源——「就收窄成通过未鉴定
/// 物品和书籍获取经验就好了」。书籍这一条就是第 7 步：读一本书值多少
/// 经验由内容字段 `ll_mod::item::ItemDef::study_experience` 声明
/// （另一条来源见 [`resolve_identify`]）。
///
/// 两件事值得点名：
///
/// - **它不是一条独立的「科研经验」数轴。** 那需要复制整台
///   [`crate::xp_curve`] 机器（自己的等级、自己的曲线、自己的升级
///   级联）而它们与既有那套逐字相同，正是 ADR 0021 点名要避免的抽象。
///   产出的就是既有的 [`crate::effect::Effect::GrantExperience`]。
/// - **防刷没有引入任何新的逐实体状态。** 第 5 步那道「一条不剩就整条
///   作废」的闸门本来就在，第 7 步只是挂在它后面——真教到新配方才有
///   产出，才谈得上经验。
///
/// # 尚未挂上的第三个钩子——如实标注
///
/// - **副职解锁**：[`crate::subclass::grant_subclass_effects`] 这个
///   共用出口已经在，缺的是 [`crate::subclass::SubclassUnlockCatalog`]
///   上的第二种触发器（当前只有「制作计数」一种，见其文档「为什么只有
///   制作这一种」）。
fn resolve_read(
    world: &WorldState,
    actor: EntityId,
    def: ContentIndex,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    // ① 行动者。
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // ② 背包里得真的有这一种东西——与 resolve_use_item 第 2 步同款。
    if !agent.inventory.iter().any(|stack| stack.def == def) {
        return Vec::new();
    }
    // ③ 物品规则。④ 空列表 = 这件东西不可读（见 ItemRule::taught_recipes
    // 文档「为什么『可不可读』不是一个独立的布尔字段」一节）。
    let Some(rule) = items.item(def) else {
        return Vec::new();
    };

    // ⑤ 只留下还不知道的。全都知道时下面的 is_empty 分支会整条作废。
    let mut effects: Vec<Effect> = Vec::new();
    for recipe in &rule.taught_recipes {
        if agent.known_recipes.contains(recipe) {
            continue;
        }
        // 同一本书把同一条配方写了两遍时（内容作者的笔误），这里会
        // 产出两条 LearnRecipe，而 apply 是无条件 push——因此在产出侧
        // 就去重，与上面那道 `known_recipes` 过滤一起，保证
        // `known_recipes` 里不会出现重复项。
        if effects.iter().any(
            |effect| matches!(effect, Effect::LearnRecipe { recipe: known, .. } if known == recipe),
        ) {
            continue;
        }
        effects.push(Effect::LearnRecipe {
            actor,
            recipe: *recipe,
        });
    }
    if effects.is_empty() {
        return Vec::new();
    }

    // ⑥ 研读经验（研究经验收窄批次）——**只有走到这里才给**：上面的
    // is_empty 分支已经把「这本书我读透了」整条挡掉，因此反复读同一本
    // 书恒零收益，不需要任何新的逐实体「读过没有」状态。
    if rule.study_experience > 0 {
        effects.push(Effect::GrantExperience {
            target: actor,
            amount: rule.study_experience,
        });
    }

    // ⑦ 计费口径与 resolve_wait/resolve_use_item/resolve_craft 完全相同
    // （基础代价 × 敏捷速度），不新增任何常量。
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

/// [`Intent::Experiment`] 结算（配方发现批次）：拿行动者手头现有的材料，
/// 在指定的配方类别里试做一次——命中某条**尚未知晓、且食材恰好齐全**的
/// 配方就学会它，按一次普通行动计费。项目所有者裁定「菜谱就是通过
/// **随机丢入东西煮**获取」的落点。
///
/// # 判定顺序
///
/// ```text
/// 1. 查 agent，查不到 → 空
/// 2. 副职闸门：类别要求非空且与 agent.subclasses 无交集 → 空
/// 3. 列出这个类别下的全部配方，逐条筛出「候选」：
///      requires_discovery == true          （不需要发现的配方无从「发现」）
///      && !known_recipes.contains(recipe)  （已经知道的不必再试）
///      && 食材全部齐全                      （手上真的有这些东西）
/// 4. 候选为空 → 空（这次什么都没试出来，也不消耗回合）
/// 5. 在候选里掷一次骰选中一条，产出 Effect::LearnRecipe 并计费
/// ```
///
/// # 为什么失败与成功都**不消耗食材**——本函数最重要的一条设计判断
///
/// `crafting-system.md` 九节⑤论证过「一次吃掉材料、玩家无法通过任何
/// 决策规避的失败只增加重复劳动」。那条论证在这里**更强**，不是更弱，
/// 四条理由各自独立成立：
///
/// 1. **玩家在做决定时手上没有任何信息。** 制作失败至少还能靠「提升
///    技能/换更好的工具」去规避；而「哪几味材料凑得成一条我还没发现的
///    配方」这个问题，在发现之前**定义上不可知**。让不可知的判断吃掉
///    真实资源，是纯粹的随机罚款，没有任何决策内容。
/// 2. **发现和制作是两件事。** 本函数成功时也**不产出任何成品**——它
///    只把一条配方写进脑子里。既然什么都没做出来，就没有什么材料「变成
///    了别的东西」。真正的消耗留在其后每一次真实的 [`Intent::Craft`]，
///    而那时玩家已经完全知道自己在做什么，消耗因此是有信息的代价。
/// 3. **消耗会让这个机制被绕开而不是被使用。** 试做要吃材料的话，最优
///    策略是囤着不试、等着捡书——一条所有者点名要的发现路径会退化成
///    没人走的路。
/// 4. **代价已经收过了。** 每次试做消耗一个完整回合（第 5 步的
///    [`Effect::ScheduleNext`]），而回合在 roguelike 里是硬通货：饥饿在
///    走、怪物在动、火把在烧。这是一条玩家能感知、也能通过「先找个安全
///    地方再试」来管理的真实代价。
///
/// # 那会不会退化成「每回合按一下试做」的无脑刷？
///
/// 不会，而且这一点由第 3 步的筛选条件**结构性**保证，不靠数值平衡：
/// 候选恒是「食材已经齐全的未知配方」。手上没有任何成套材料时候选为
/// 空，试一万次也是空；把当前手头能试出来的都试出来之后，候选同样为
/// 空。换句话说，「试做」的产出上限完全由**玩家搜集到了什么**决定，
/// 不由他按了多少次决定——刷的是探索与搜集，不是按键。
///
/// # 副职闸门（第 2 步）为什么照判
///
/// 与 [`resolve_craft`] 第 3 步同一份判据、同一个 `RecipeCatalog` 方法：
/// 做不了这一类的人，谈不上在这一类里试——不会打铁的人站在铁砧前把
/// 铁锭摆来摆去，不会「发现」出一把剑。
///
/// **这不会造出新的死锁**：`mods/lostland/crafting.json5` 与
/// `ll_mod::content_audit` 的 `detect_unlock_deadlocks` 已经共同保证
/// 「用来练出某个副职的类别」不会反过来要求那个副职（那个环装载期硬
/// 失败）。设了闸门的类别只可能是「已经有副职的人才碰得到的进阶类别」，
/// 而这正是它该有的样子。读书那条路径**不受闸门约束**（[`resolve_read`]
/// 完全不查类别）——知识可以先于资格获得，两条路径因此不是互相的备份，
/// 而是两种不同的获取方式。
///
/// # 随机流怎么构造（约束 C3）
///
/// 三元组取 `(world.seed, actor.as_u64(), world.clock.0 ^ 常量标签)`，
/// 与 [`resolve_inspect`] 的隐匿判定、[`resolve_attack`] 的骰子/偷袭两
/// 条流手法逐字相同：世界种子 + 发起者 + 当前时刻，异或一个只用来把
/// 这条流与同一 `(种子, 实体, 时刻)` 下其它流区分开的固定标签
/// （`EXPERIMENT_EVENT_TAG`，没有数值含义）。**新造一条流、只取一个
/// 数**，不是一条跨调用累进的长流，因此「这次没试成（候选为空、提前
/// 返回）」不会让后续任何取数错位。
///
/// 掷骰只用来**在多个候选之间选一个**，不用来判定「成不成功」——候选
/// 非空时必定学会一条。理由同上一节：成不成功已经由「食材齐不齐」这个
/// 玩家完全可控的条件回答了，再叠一层概率只是把可控的事重新变成不可控。
///
/// 候选列表的顺序由 [`crate::craft::RecipeCatalog::recipes_in_category`]
/// 保证按索引升序（见其文档），再经上面三条谓词过滤，全程 `Vec`，
/// 不涉及任何 `HashMap`/`HashSet` 迭代顺序（约束 C5）。
///
/// # 已知边界（与 [`resolve_craft`] 第 7 步逐字相同）
///
/// 食材齐全的判定只认**第一条** `def` 匹配的堆，不跨多堆合并计数——
/// 背包里两堆各 1 个铁锭时，需要 2 个的配方判定为「料不够」。两处共用
/// 同一段判定（[`has_all_ingredients`]），因此这条边界不会在两边漂移。
fn resolve_experiment(
    world: &WorldState,
    actor: EntityId,
    category: ContentIndex,
    recipes: &dyn RecipeCatalog,
) -> Vec<Effect> {
    // ① 行动者。
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // ② 副职闸门——与 resolve_craft 第③步同一份判据、同一个方法。
    let required_subclasses = recipes.category_required_subclasses(category);
    if !required_subclasses.is_empty()
        && !required_subclasses
            .iter()
            .any(|needed| agent.subclasses.contains(needed))
    {
        return Vec::new();
    }
    // ③ 候选筛选，三条谓词全过才算。
    let candidates: Vec<ContentIndex> = recipes
        .recipes_in_category(category)
        .into_iter()
        .filter(|index| {
            if agent.known_recipes.contains(index) {
                return false;
            }
            let Some(rule) = recipes.recipe(*index) else {
                return false;
            };
            rule.requires_discovery && has_all_ingredients(agent, &rule)
        })
        .collect();
    // ④ 一条都试不出来：不产出效果，也不消耗时间。
    if candidates.is_empty() {
        return Vec::new();
    }

    // ⑤ 掷一次骰选中一条，见本函数文档「随机流怎么构造」一节。
    let mut rng = ll_core::rng::DetRng::for_entity(
        world.seed,
        actor.as_u64(),
        (world.clock.0 as u64) ^ EXPERIMENT_EVENT_TAG,
    );
    let picked = candidates[rng.gen_range(candidates.len() as u64) as usize];

    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![
        Effect::LearnRecipe {
            actor,
            recipe: picked,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// [`Intent::Identify`] 结算（未鉴定物品批次 + 盲盒批次）：鉴定背包里
/// 的一种未鉴定物品。**两条互斥的路径**，由这件物品有没有声明盲盒池
/// （`ItemRule::blind_box_pool`）决定：
///
/// | | 普通鉴定 | 盲盒 |
/// |---|---|---|
/// | 物品去向 | **留着**（只是你现在认识它了） | **被消耗** |
/// | 产出 | 无 | **一件随机物品** |
/// | 写世界状态 | `Agent::identified_items` 多一条 | 背包换了内容 |
/// | 性质 | 揭示 | **转化** |
///
/// # 判定顺序
///
/// ```text
/// 1. 查 agent，查不到 → 空
/// 2. 背包里没有这一种东西 → 空
/// 3. 查不到物品规则 → 空                     （ADR 0015：未注册当作没有）
/// 4. requires_identification 为假 → 空       （这件东西一眼就认得）
/// 5a. 不是盲盒：已经认识过这一种 → 空         （防刷闸门，见下）
///     否则 → Effect::IdentifyItem [+ GrantExperience] + 计费
/// 5b. 是盲盒：按权重抽一条 → ConsumeInventoryItem + MergeIntoInventory
///     [+ GrantExperience] + 计费
/// ```
///
/// # 防刷：普通鉴定靠「一次性事件」，不需要任何新的逐实体状态
///
/// 第 5a 步的闸门读的是 [`ll_world::entity::Agent::identified_items`]
/// ——那**同时**是这条路径的产出目标。于是「认出一个新种类」天然是一次
/// 一次性事件：第二次鉴定同一种东西恒返回空 `Vec`，既不给经验、**也不
/// 消耗时间**（与 [`resolve_read`] 第 5 步、[`resolve_pick_up`]「脚下
/// 没东西就静默作废」同一条既有纪律：一次不可能产生任何结果的行动不该
/// 收费）。这条设计最值钱的性质就在这里——**它不需要任何新的逐实体
/// 「研究过没有」状态**，`identified_items` 本来就要存。
///
/// # ⚠ 盲盒是那条防刷原则的**有意例外**
///
/// 项目所有者裁定，原话：**「开盲盒都给吧，轻松点，这是游戏」**。第 5b
/// 步无条件给经验：不查产出物认不认识，也不查这种盒子开过没有。完整的
/// 取舍论证与那条「⚠ 给盲盒写配方会打开经验水龙头」的警告写在
/// `ll_mod::item::ItemDef::blind_box_pool` 文档里——写在**内容字段**上，
/// 是为了让日后给盲盒加配方的人在写下那条配方之前就看见它。
///
/// **普通鉴定与读书两条路径不受这条影响，一个字都没改。**
///
/// # 盲盒的随机流怎么构造（约束 C3）
///
/// 三元组取 `(world.seed, actor.as_u64(), world.clock.0 ^ 常量标签)`，
/// 与 [`resolve_experiment`]/[`resolve_inspect`]/[`resolve_attack`] 那
/// 几条流手法逐字相同：**新造一条流、只取一个数**，因此上面任何一步
/// 提前返回都不会让后续取数错位。标签是 [`BLIND_BOX_EVENT_TAG`]。
///
/// **没有盲盒声明时不构造流**——第 5a 步一行 `DetRng` 都不碰，与
/// [`resolve_attack`]「没有偷袭声明就不构造流」同一条既有纪律。
///
/// 加权选取本身**照抄** [`ll_world::weather::weather_kind_at`]：权重
/// 求和 → `gen_range(总和)` → 沿同一顺序前缀和 walk。不另发明，理由见
/// [`ll_sim::item::BlindBoxEntry`](crate::item::BlindBoxEntry) 文档。
/// 遍历的是 `Vec`（保序，约束 C5），不涉及 `HashMap`/`HashSet`。
///
/// # 产出物的耐久
///
/// 开出来的东西是**新的**：耐久等于产出物那条定义声明的上限，走
/// [`ItemStack::freshly_made`] 那条共同规则——与 [`resolve_craft`]
/// 造成品那一行**逐字相同**。盲盒刻意不在这里发明第二套答案。
///
/// 本节此前记录的是这条规则**还不存在**时的形状（两个产出点都恒把
/// 耐久设成 `None`，于是开出来的铁短剑永远不会磨损）；那条缺陷已随
/// [`ItemStack::freshly_made`] 落地一并修掉，见该构造器文档。
///
/// # 一个盲盒不能开出它自己
///
/// 由**注册期**拒绝（`ll_mod::content_schema_gear::apply_item_extras`），
/// 不在这里判。理由是效果顺序：`ConsumeInventoryItem` 与
/// `MergeIntoInventory` 都按 `(def, durability)` 定位同一堆，而后者的
/// `resulting` 是在**消耗之前**的背包上算出来的——自产出的盒子会让这
/// 两条效果互相抵消，症状是「开了个盒子，什么都没发生」。把它拦在注册
/// 期，这里就不需要一条只为一种病态内容存在的运行期分支。
///
/// # 约束核对
///
/// - C1：只产出 `Vec<Effect>`，一个字节的世界状态都不写。
/// - C3：随机只有盲盒那一路，走 `DetRng::for_entity`，见上。
/// - C5：遍历的三个容器（`agent.inventory`、`agent.identified_items`、
///   `rule.blind_box_pool`）都是 `Vec`，保序。
fn resolve_identify(
    world: &WorldState,
    actor: EntityId,
    def: ContentIndex,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    // ① 行动者。
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // ② 背包里得真的有这一种东西——与 resolve_read 第 2 步同款，但这里
    // 还要留住这一堆本身：盲盒那一路要用它的耐久来定位被消耗的堆。
    let Some(held) = agent.inventory.iter().find(|stack| stack.def == def) else {
        return Vec::new();
    };
    let held_durability = held.durability;
    // ③ 物品规则。
    let Some(rule) = items.item(def) else {
        return Vec::new();
    };
    // ④ 一眼就认得的东西没有可鉴定的。
    if !rule.requires_identification {
        return Vec::new();
    }

    // 计费口径与 resolve_read/resolve_wait/resolve_craft 完全相同
    // （基础代价 × 敏捷速度），不新增任何常量。
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    let schedule = Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    };
    let experience = (rule.study_experience > 0).then_some(Effect::GrantExperience {
        target: actor,
        amount: rule.study_experience,
    });

    if rule.blind_box_pool.is_empty() {
        // ⑤a 普通鉴定：揭示，不转化。已经认识过就整条作废（防刷闸门）。
        if agent.identified_items.contains(&def) {
            return Vec::new();
        }
        let mut effects = vec![Effect::IdentifyItem { actor, def }];
        effects.extend(experience);
        effects.push(schedule);
        return effects;
    }

    // ⑤b 盲盒：转化。加权抽一条，手法照抄 weather_kind_at。
    let total: u64 = rule
        .blind_box_pool
        .iter()
        .map(|entry| u64::from(entry.weight))
        .sum();
    // 理论不可达：注册期已经拒绝了权重为 0 的候选（`ItemError::
    // DegenerateBlindBoxEntry`），非空池的总和必然 > 0。防御性地静默
    // 作废而不是让 `gen_range(0)` 去 panic——同 `weather_kind_at` 对
    // 「总和为 0」的处理立场。
    if total == 0 {
        return Vec::new();
    }
    let mut rng = ll_core::rng::DetRng::for_entity(
        world.seed,
        actor.as_u64(),
        (world.clock.0 as u64) ^ BLIND_BOX_EVENT_TAG,
    );
    let mut roll = rng.gen_range(total);
    let mut picked = rule.blind_box_pool[rule.blind_box_pool.len() - 1];
    for entry in &rule.blind_box_pool {
        let weight = u64::from(entry.weight);
        if roll < weight {
            picked = *entry;
            break;
        }
        roll -= weight;
    }

    let mut effects = vec![
        Effect::ConsumeInventoryItem {
            actor,
            def,
            durability: held_durability,
        },
        merge_into_inventory_effect(
            agent,
            actor,
            // 开出来的东西与制作出来的东西一样是"新的"，走同一条共同
            // 规则，见本函数文档「产出物的耐久」一节。
            ItemStack::freshly_made(
                picked.item,
                picked.count,
                items.item(picked.item).and_then(|def| def.max_durability),
            ),
            items,
        ),
    ];
    effects.extend(experience);
    effects.push(schedule);
    effects
}

/// 把 [`resolve_identify`] 盲盒那条随机流与同一 `(种子, 实体, 时刻)` 下
/// 其它流区分开的固定标签，没有数值含义上的特殊性——手法同
/// [`EXPERIMENT_EVENT_TAG`]，只要求「与别的流的三元组不同」。
const BLIND_BOX_EVENT_TAG: u64 = 0x0B11_0DB0_0000_0000;

/// 把 [`resolve_experiment`] 那条随机流与同一 `(种子, 实体, 时刻)` 下
/// 其它流区分开的固定标签，没有数值含义上的特殊性——手法同
/// [`resolve_attack`] 内部的 `DAMAGE_FORMULA_DICE_EVENT_TAG`，只要求
/// 「与别的流的三元组不同」。
const EXPERIMENT_EVENT_TAG: u64 = 0x0EE0_0BEE_0000_0000;

/// 行动者的背包是否凑得齐这条配方的全部食材——[`resolve_craft`] 第 7 步
/// 与 [`resolve_experiment`] 第 3 步共用的同一段判定。
///
/// 抽成函数的理由符合 ADR 0021「有真正可共享的算法」：两处共享的不只是
/// 一个循环，还包括那条**已知边界**（只认第一条 `def` 匹配的堆，不跨堆
/// 合并计数，见两个调用点各自文档）。写两遍的真正代价不是多几行，而是
/// 那条边界会在两边各自漂移——制作说「料不够」而试做说「料够」是一个
/// 玩家可见、且极难归因的缺陷。
fn has_all_ingredients(agent: &Agent, rule: &RecipeRule) -> bool {
    rule.ingredients.iter().all(|ingredient| {
        let held = agent
            .inventory
            .iter()
            .find(|stack| stack.def == ingredient.item)
            .map_or(0, |stack| stack.count);
        held >= ingredient.count
    })
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

    // 潜行时移动开销上升（潜行与盗贼被动批次）——倍率与完整论证见
    // `STEALTH_MOVE_COST_PERMILLE`。乘在**地形开销**上、`action_cost`
    // 换算敏捷速度之前：潜行放慢的是「挪这一格本身有多费事」，敏捷高
    // 的人潜行同样比自己不潜行时慢，两者是可以叠乘的两层，不是互相
    // 替代。饱和乘法防止一个极端 `move_cost` 在这一步环绕
    // （`u32::saturating_mul`，与本文件其余「内容作者填的数值一律饱和
    // 运算」同一条既有纪律）。
    //
    // **只挂在这一条真的挪动了位置的分支上**：上面撞墙/开门两条分支
    // 各自提前返回，它们按 `BASE_ACTION_COST` 计费而不是地形开销——
    // 潜行不该让「推开一扇门」或「撞上一堵墙」也变慢，那两件事与
    // 「悄悄挪一格」不是同一个动作。
    let terrain_cost = terrain.move_cost(&world.terrain_table);
    let terrain_cost = if agent.stealthed {
        terrain_cost
            .saturating_mul(STEALTH_MOVE_COST_PERMILLE)
            .saturating_div(1000)
    } else {
        terrain_cost
    };
    let cost = action_cost(terrain_cost, speed);
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
/// `stat_bonuses` **之前**跳过整条堆，三个 [`StatTarget`] 变体
/// （`Attribute`/`Armor`/`Insulation`）没有任何一个能绕过它。一件穿
/// 破了的皮袄因此既不挡刀也不保暖，见 [`derive_stats`] 文档「耐久归零」
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
/// 有声明时：触发率由
/// [`sneak_attack_chance_permille`]（`crate::combat`）把
/// `attacker_derived.attribute(AttributeKind::Luck)`（**派生值**，同暴击
/// 判定复用的 `effective_luck`，装备/状态效果加的幸运同样生效）与天赋
/// 自带的敏感度系数换算成千分比，走独立的第三条 `DetRng` 流判定是否
/// 触发；触发则把天赋声明的固定 `extra_damage` 加到伤害上。挂载点在
/// 暴击放大之后、抗性乘数之前——追加的伤害仍然是这一下攻击的一部分，
/// 应当同样受目标抗性影响，不是绕开减伤链路凭空产出的独立效果。
///
/// # 潜行与偷袭（潜行与盗贼被动批次）
///
/// 攻击者正处于潜行状态（[`ll_world::entity::Agent::stealthed`]）时，
/// 偷袭判定**直通**：跳过上面那次幸运掷骰，直接吃到
/// `extra_damage`。这条连接刻意做在「已经有 `SneakAttack` 声明」的
/// 前提之内，不是「潜行本身就能偷袭」——两层是分开的（项目所有者
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
/// 这一下仍然吃到直通的偷袭，破除从下一次行动起才生效（经典的「一次
/// 免费背刺」）。
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
fn resolve_attack(
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

    // 偷袭判定（盗贼偷袭接线批次）：只有攻击者的有效天赋声明了
    // `RuleModifier::SneakAttack` 才会进入这个分支——没有声明时
    // `sneak_attack_rule` 返回 `None`，完全不构造额外的 `DetRng` 流,
    // 见 `RuleModifier::SneakAttack` 文档。挂载点：暴击放大之后、抗性
    // 乘数之前——与「抗性」一节同一条既有纪律，追加的伤害仍然是这一下
    // 攻击的一部分,应当同样受目标对这一伤害类别的抗性影响,不是绕开
    // 减伤链路凭空产出的独立效果。约束 C3：随机性走
    // `DetRng::for_entity(世界种子, 实体 ID, 事件计数)`,这里用一个与
    // 暴击流（恒为 `world.clock.0 as u64`）、骰子流
    // （`world.clock.0 ^ DAMAGE_FORMULA_DICE_EVENT_TAG`）都不同的第三个
    // 固定标签构造第三条独立流,三条流的三元组两两不同,互不干扰（约束
    // C5：本函数在偷袭判定这一步只消费这一次随机数,取数顺序天然确定,
    // 且固定排在暴击判定之后、伤害公式骰子求值之后，与代码里出现的
    // 先后顺序一致）。触发率读
    // `attacker_derived.attribute(AttributeKind::Luck)`（同一个
    // `effective_luck`，暴击判定复用的派生值）——装备/状态效果加的幸运
    // 同样会反映到偷袭触发率上，理由同暴击那一节「暴击：读取
    // attacker_derived.attribute」。
    const SNEAK_ATTACK_EVENT_TAG: u64 = 0x51EA_ACC0_0000_0000;
    let damage = match sneak_attack_rule(&attacker_modifiers) {
        // 潜行直通（潜行与盗贼被动批次）：攻击者正处于潜行状态时跳过
        // 掷骰，直接判定触发——见本函数文档「潜行与偷袭」一节。放在
        // `Some(rule)` 之前用守卫分支表达，而不是在下面那个分支里写
        // 一个提前 `return`：这是一个 `match` 表达式的值，提前 return
        // 会从整个 `resolve_attack` 返回而不是从这个表达式返回。
        //
        // **这一支不构造那条 `DetRng` 流**（下面那支才构造），与
        // `None` 支「没有任何来源声明偷袭时完全不构造额外的 DetRng
        // 流」同一条既有纪律：每次判定都是现场用 `DetRng::for_entity`
        // 新造一条流、只取一个数，不是一条跨调用累进的长流，因此
        // 「这次没取数」不会让后续任何取数错位（约束 C3/C5）。
        Some(rule) if attacker.stealthed => damage.saturating_add(rule.extra_damage),
        Some(rule) => {
            let mut sneak_rng = ll_core::rng::DetRng::for_entity(
                world.seed,
                actor.as_u64(),
                (world.clock.0 as u64) ^ SNEAK_ATTACK_EVENT_TAG,
            );
            let sneak_chance =
                sneak_attack_chance_permille(effective_luck, rule.luck_chance_permille_per_point);
            if sneak_rng.chance(sneak_chance.max(0) as u32, 1000) {
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
///
/// `#[allow(clippy::too_many_arguments)]`：八个参数里有两个是同一个
/// `TraitGrantSource` 接口的不同来源（种族/职业，见
/// [`crate::traits::agent_trait_sources`]）——它们没有被合并成一个
/// 结构体，理由同 [`resolve_dispatch`]（模块私有，无法作为 rustdoc
/// 链接目标）文档同一段：所有者索引因调用点而异（同一次攻击里攻击方
/// 与防御方各查各的），能被打包的只有「表」这一半，而只打包一半只会
/// 换来一个既不完整也不好读的中间类型。
#[allow(clippy::too_many_arguments)]
fn resolve_use_skill(
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
fn find_available_slot_tier(
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
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        })
    }

    /// 把 `actor` 的潜行状态置为 `stealthed`——潜行相关测试的公共
    /// Arrange 步骤。直接写字段而不是先跑一次 `Intent::ToggleStealth`：
    /// 那会让「移动开销」这类测试的断言同时依赖切换本身是否正确，两件
    /// 事应当各自独立验证（切换本身由
    /// `切换潜行产出取反后的确定状态并消耗一个回合` 单独覆盖）。
    fn set_stealthed(world: &mut WorldState, actor: EntityId, stealthed: bool) {
        world
            .actors
            .get_mut(actor)
            .expect("调用方刚生成的实体必然存在")
            .stealthed = stealthed;
    }

    #[test]
    fn 切换潜行产出取反后的确定状态并消耗一个回合() {
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);

        // Act：从「未潜行」切一次。
        let effects = resolve(&world, &Intent::ToggleStealth { actor });

        // Assert：产出确定值 true（不是「取反」这个指令本身），且排了
        // 下一次行动（消耗了一个回合）。
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetStealth {
                actor: a,
                stealthed: true
            } if *a == actor
        )));
        assert!(
            effects.iter().any(
                |effect| matches!(effect, Effect::ScheduleNext { actor: a, .. } if *a == actor)
            )
        );
    }

    #[test]
    fn 已在潜行中再次切换产出退出潜行() {
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        set_stealthed(&mut world, actor, true);

        // Act
        let effects = resolve(&world, &Intent::ToggleStealth { actor });

        // Assert
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetStealth {
                actor: a,
                stealthed: false
            } if *a == actor
        )));
    }

    #[test]
    fn 切换潜行的耗时与原地等待相同() {
        // 「消耗一个回合」这句话的准确含义：与 Intent::Wait 逐刻相同，
        // 不是另起一个数字，见 resolve_toggle_stealth 文档。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);

        // Act
        let toggle_at = next_action_tick(&resolve(&world, &Intent::ToggleStealth { actor }));
        let wait_at = next_action_tick(&resolve(&world, &Intent::Wait { actor }));

        // Assert
        assert_eq!(toggle_at, wait_at);
    }

    #[test]
    fn 潜行时移动一格比不潜行时更慢() {
        // Arrange：两个完全相同的世界，只差潜行状态。目的地显式铺成
        // 草地——`test_world` 用 `GenParams::default()` 生成，出生点
        // 东侧未必可通行，不铺的话两条断言都会落进撞墙分支而恒相等
        // （与本文件其余移动测试同一条既有做法）。
        let (mut visible_world, ids_a) = test_world();
        let visible = spawn_agent(&mut visible_world);
        visible_world
            .terrain
            .set_terrain(east_of_spawn(&visible_world), ids_a.grass);
        let (mut stealth_world, ids_b) = test_world();
        let sneaker = spawn_agent(&mut stealth_world);
        stealth_world
            .terrain
            .set_terrain(east_of_spawn(&stealth_world), ids_b.grass);
        set_stealthed(&mut stealth_world, sneaker, true);

        // Act
        let open_at = next_action_tick(&resolve(
            &visible_world,
            &Intent::Move {
                actor: visible,
                dir: Direction::East,
            },
        ));
        let sneak_at = next_action_tick(&resolve(
            &stealth_world,
            &Intent::Move {
                actor: sneaker,
                dir: Direction::East,
            },
        ));

        // Assert：STEALTH_MOVE_COST_PERMILLE 是 2000（两倍），两次都从
        // Tick(0) 起算，因此潜行那一步的下一次行动时刻应当恰好是两倍。
        assert!(sneak_at > open_at);
        assert_eq!(sneak_at, open_at * 2);
    }

    #[test]
    fn 潜行不改变撞墙的耗时() {
        // 反面覆盖 resolve_move 里「只挂在真的挪动了位置的那一条分支」
        // 这句话：撞墙走的是 BASE_ACTION_COST，不是地形开销，潜行不该
        // 让撞墙也变慢。
        // Arrange：东侧摆一堵石墙。
        let (mut visible_world, ids_a) = test_world();
        let visible = spawn_agent(&mut visible_world);
        let wall_a = east_of_spawn(&visible_world);
        visible_world.terrain.set_terrain(wall_a, ids_a.wall_stone);

        let (mut stealth_world, ids_b) = test_world();
        let sneaker = spawn_agent(&mut stealth_world);
        let wall_b = east_of_spawn(&stealth_world);
        stealth_world.terrain.set_terrain(wall_b, ids_b.wall_stone);
        set_stealthed(&mut stealth_world, sneaker, true);

        // Act
        let open_at = next_action_tick(&resolve(
            &visible_world,
            &Intent::Move {
                actor: visible,
                dir: Direction::East,
            },
        ));
        let sneak_at = next_action_tick(&resolve(
            &stealth_world,
            &Intent::Move {
                actor: sneaker,
                dir: Direction::East,
            },
        ));

        // Assert
        assert_eq!(open_at, sneak_at);
    }

    #[test]
    fn 盘查消耗一个回合() {
        // 回归：`resolve_inspect` 曾经只产出一条 `Effect::Inspect`，
        // 不产出 `Effect::ScheduleNext`——被盘查者的下一次行动时刻
        // 原地不动，`TurnEngine::perform` 会把它重新排回**同一个
        // tick**，`advance_ai` 因此对同一个卫兵反复空转直到耗尽
        // `MAX_STEPS_PER_ADVANCE`。这个缺陷一直没暴露，只是因为在
        // 行为树接进回合引擎之前，`Intent::Inspect` 从来没有经由
        // `TurnEngine` 产出过；接上之后立刻表现为整条测试挂死。
        // Arrange
        let (mut world, _ids) = test_world();
        let guard = spawn_agent(&mut world);
        let target = spawn_agent(&mut world);

        // Act
        let effects = resolve(
            &world,
            &Intent::Inspect {
                actor: guard,
                target,
            },
        );

        // Assert：盘查照旧产出，且下一次行动时刻严格晚于当前世界时钟。
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Inspect { .. })),
            "盘查本身仍然要产出"
        );
        assert!(
            next_action_tick(&effects) > world.clock.0,
            "盘查必须推进发起者的下一次行动时刻，否则时间轴会在同一 tick 空转"
        );
    }

    /// 从一批效果里取出 `Effect::ScheduleNext` 的时刻——潜行相关的
    /// 耗时断言反复需要这一步。
    fn next_action_tick(effects: &[Effect]) -> i64 {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::ScheduleNext { at, .. } => Some(at.0),
                _ => None,
            })
            .expect("这些意图都会排下一次行动")
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
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: Some(ll_core::ident::WorldId::next(&mut world_id_counter)),
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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
        let ll_world::history::HistoricalEventKind::Kill(record) = &world.history[0].kind else {
            panic!("战斗结算写进 WorldState::history 的必须是一条击杀记录");
        };
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
        // 两个幸运值代进对抗判定（被攻击者幸运取基准 0，因此净差就是
        // 攻击者一侧的修正）：见 `crate::combat::CRIT_BASE_CHECK_MODIFIER`
        // 文档「幸运怎么进式子」那张表。
        let low_luck = 5; // −23 + 5 = −18 → 9.77% 暴击率。
        let high_luck = 100; // −23 + 100 = 77，钳到上限 28 → 97.51%。
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

        // Assert：97.51% 暴击率的一侧命中次数应远多于 9.77% 的一侧——
        // 差距留了很大的安全边际（3000 次试验上期望值相差约 2630 次，
        // 这里只要求多过 100 次），避免二项分布的正常波动把测试变成
        // 偶发性失败。
        assert!(high_crits > low_crits + 100);
        // 两端都不是绝对：高幸运那一侧仍然打得出非暴击，低幸运那一侧
        // 仍然打得出暴击。这是「不允许绝对」在暴击这条链路上的可观察
        // 证据——旧的概率模型里幸运 200 以上是**必定**暴击。
        assert!(high_crits < trials, "顶格幸运也不该次次暴击");
        assert!(low_crits > 0, "低幸运也不该一次都暴不出来");
    }

    /// 造一个占位实体，站在 `pos`，除幸运外六项主属性取基准值，且
    /// `race` 由调用方直接给出（不像 [`spawn_agent_with_luck`] 那样在
    /// 函数体内部临时 intern 一份「反正只看数值,不看具体是哪个种族」
    /// 的占位种族）——偷袭判定测试需要种族索引与授予偷袭天赋的
    /// [`TraitGrantSource`] 测试替身用的是**同一个** `ContentIndex`,
    /// 若各自在互不相干的 `Interner` 里各 intern 一次,两边算出的数值
    /// 不保证相等（`ll_core::ident` 模块文档「不可持久化——索引依赖 mod
    /// 加载顺序」），因此本函数把「种族索引哪来的」这个决定权交还给
    /// 调用方,调用方在测试里只 intern 一次,两处引用同一个值。
    fn spawn_agent_with_luck_and_race(
        world: &mut WorldState,
        pos: ll_core::torus::TorusPos,
        luck: i32,
        race: ContentIndex,
    ) -> EntityId {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
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
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(world, pos),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        })
    }

    /// 一个只认识固定种族索引的测试用天赋授予来源，专供偷袭判定测试
    /// 使用——形状与 [`FixedRacePoolGrant`] 相同（只回答"这个种族授予
    /// 哪条天赋引用"），但刻意不复用它：两者服务的测试意图不同（资源池
    /// 容量钳位 vs 偷袭判定），共享同一个类型名会让两组测试的失败信息
    /// 混在一起,不利于定位。
    struct FixedSneakRaceGrant {
        race: ContentIndex,
        trait_id: ContentIndex,
    }

    impl TraitGrantSource for FixedSneakRaceGrant {
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

    /// 固定把 `trait_id` 映射到一条声明 [`crate::traits::RuleModifier::SneakAttack`]
    /// 的 `TraitRule`——供偷袭判定测试使用。
    struct FixedSneakAttackTrait {
        trait_id: ContentIndex,
        luck_chance_permille_per_point: i32,
        extra_damage: i32,
    }

    impl TraitCatalog for FixedSneakAttackTrait {
        fn trait_rule(&self, trait_id: ContentIndex) -> Option<crate::traits::TraitRule> {
            if trait_id != self.trait_id {
                return None;
            }
            Some(crate::traits::TraitRule {
                granted_skills: Vec::new(),
                granted_resource_pools: Vec::new(),
                rule_modifiers: vec![crate::traits::TypedRuleModifier {
                    modifier_type: None,
                    modifier: crate::traits::RuleModifier::SneakAttack {
                        luck_chance_permille_per_point: self.luck_chance_permille_per_point,
                        extra_damage: self.extra_damage,
                    },
                }],
            })
        }
    }

    #[test]
    fn 有效幸运更高的攻击者偷袭触发频率更高() {
        // 频率断言，不是单次结果——理由同「幸运更高的角色暴击命中频率
        // 更高」：偷袭同样只改变判定的概率形状,不保证任意一次攻击必然
        // 触发/不触发。`extra_damage` 故意取得远大于暴击单独能放大的
        // 上限（基准伤害 10，暴击最多放大到 15，见
        // `CRIT_DAMAGE_MULTIPLIER_PERMILLE` 文档）——`sneak_threshold`
        // 因此只可能被「偷袭真的触发」跨过，不会被暴击单独触发,统计
        // 频率时不需要额外剔除暴击的贡献,即使高幸运一侧的暴击也更频繁
        // （同一个 `effective_luck` 两条判定都读）。
        // Arrange
        let trials = 3_000i64;
        let low_luck = 5; // 5 × 15‰ = 75‰（7.5%）触发率。
        let high_luck = 40; // 40 × 15‰ = 600‰（60%）触发率。
        let per_point = 15;
        let extra_damage = 1_000;
        let baseline_damage =
            damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);
        let sneak_threshold = baseline_damage + 100;

        let mut interner = ll_core::ident::Interner::new();
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:rogue").expect("合法标识符"));
        let trait_id = interner.intern(
            ll_core::ident::NamespacedId::parse("lostland:sneak_attack").expect("合法标识符"),
        );
        let race_traits = FixedSneakRaceGrant { race, trait_id };
        let traits = FixedSneakAttackTrait {
            trait_id,
            luck_chance_permille_per_point: per_point,
            extra_damage,
        };

        let (mut low_world, _low_terrain_ids) = test_world();
        let low_attacker_pos = low_world.size.wrap(5, 5);
        let low_attacker =
            spawn_agent_with_luck_and_race(&mut low_world, low_attacker_pos, low_luck, race);
        let low_victim_pos = east_of_spawn(&low_world);
        let low_victim = spawn_named_agent(&mut low_world, low_victim_pos, 1_000_000);

        let (mut high_world, _high_terrain_ids) = test_world();
        let high_attacker_pos = high_world.size.wrap(5, 5);
        let high_attacker =
            spawn_agent_with_luck_and_race(&mut high_world, high_attacker_pos, high_luck, race);
        let high_victim_pos = east_of_spawn(&high_world);
        let high_victim = spawn_named_agent(&mut high_world, high_victim_pos, 1_000_000);

        // Act：只挪动世界时钟取得不同的随机流,理由同「幸运更高的角色
        // 暴击命中频率更高」。
        let mut low_sneaks = 0i64;
        let mut high_sneaks = 0i64;
        for tick in 0..trials {
            low_world.clock = Tick(tick);
            let low_effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
                &low_world,
                &Intent::Attack {
                    actor: low_attacker,
                    target: low_victim,
                },
                &NoSkills,
                &race_traits,
                &traits,
                &NoResourcePools,
                &NoItems,
                &NoFormulas,
                &NoDamageCategories,
            );
            if low_effects.iter().any(
                |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > sneak_threshold),
            ) {
                low_sneaks += 1;
            }

            high_world.clock = Tick(tick);
            let high_effects =
                resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
                    &high_world,
                    &Intent::Attack {
                        actor: high_attacker,
                        target: high_victim,
                    },
                    &NoSkills,
                    &race_traits,
                    &traits,
                    &NoResourcePools,
                    &NoItems,
                    &NoFormulas,
                    &NoDamageCategories,
                );
            if high_effects.iter().any(
                |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > sneak_threshold),
            ) {
                high_sneaks += 1;
            }
        }

        // Assert：60% 触发率的一侧命中次数应远多于 7.5% 的一侧——差距
        // 留了很大的安全边际（期望值相差约 1575 次，这里只要求多过
        // 100 次），理由同「幸运更高的角色暴击命中频率更高」。
        assert!(high_sneaks > low_sneaks + 100);
    }

    #[test]
    fn 偷袭触发时伤害真的更高() {
        // 精确数值断言，不是频率断言——利用暴击判定/伤害公式骰子的
        // `DetRng` 三元组 `(世界种子, 实体 ID, 世界时钟)` 完全不依赖
        // 调用方传入的 `race_traits`/`traits` 目录这一点：同一个世界、
        // 同一个攻击者、同一个目标、同一个 `world.clock`,两次调用
        // 之间暴击是否命中、伤害公式的骰子抽出什么值逐位相同,唯一的
        // 差异是这次传入的天赋目录有没有声明偷袭——两次的伤害差因此
        // 必须精确等于 `extra_damage`,不多不少（若偷袭判定读到了不该
        // 读的东西,或者额外消费了一次随机数导致后续判定错位,这条精确
        // 断言会立刻暴露）。幸运（50）× 每点触发率（20‰）恰好等于
        // 1000‰,触发精确钳在 100%,不依赖 `world.clock` 取值,见
        // `crate::combat::sneak_attack_chance_permille` 文档「夹在
        // 0..=1000」一节。
        // Arrange
        let luck = 50;
        let per_point = 20;
        let extra_damage = 37;
        let (mut world, _terrain_ids) = test_world();
        let attacker_pos = world.size.wrap(5, 5);
        let mut interner = ll_core::ident::Interner::new();
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:rogue").expect("合法标识符"));
        let trait_id = interner.intern(
            ll_core::ident::NamespacedId::parse("lostland:sneak_attack").expect("合法标识符"),
        );
        let attacker = spawn_agent_with_luck_and_race(&mut world, attacker_pos, luck, race);
        let victim_pos = east_of_spawn(&world);
        let victim = spawn_named_agent(&mut world, victim_pos, 1_000_000);
        let race_traits = FixedSneakRaceGrant { race, trait_id };
        let traits = FixedSneakAttackTrait {
            trait_id,
            luck_chance_permille_per_point: per_point,
            extra_damage,
        };

        let attack = |race_traits: &dyn TraitGrantSource, traits: &dyn TraitCatalog| -> i32 {
            let effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
                &world,
                &Intent::Attack {
                    actor: attacker,
                    target: victim,
                },
                &NoSkills,
                race_traits,
                traits,
                &NoResourcePools,
                &NoItems,
                &NoFormulas,
                &NoDamageCategories,
            );
            effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Damage { amount, .. } => Some(*amount),
                    _ => None,
                })
                .expect("攻击必然产出一条伤害效果")
        };

        // Act
        let damage_without_sneak = attack(&NoTraitGrants, &NoTraits);
        let damage_with_sneak = attack(&race_traits, &traits);

        // Assert
        assert_eq!(damage_with_sneak, damage_without_sneak + extra_damage);
    }

    #[test]
    fn 潜行中的攻击者零幸运也必定触发偷袭() {
        // 潜行直通（本批次）——与上一条 `偷袭触发时伤害真的更高` 恰好
        // 互补：那一条把触发率钳在 100% 来拿到确定结果，本条把幸运压到
        // **零**（触发率因此恒为 0‰，见
        // `crate::combat::sneak_attack_chance_permille`），于是掷骰这条
        // 路径**永远不可能**触发偷袭。潜行的攻击者依然吃到完整的
        // `extra_damage`，就只能是直通那条分支给的。
        // Arrange
        let luck = 0;
        let per_point = 20;
        let extra_damage = 37;
        let (mut world, _terrain_ids) = test_world();
        let attacker_pos = world.size.wrap(5, 5);
        let mut interner = ll_core::ident::Interner::new();
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:rogue").expect("合法标识符"));
        let trait_id = interner.intern(
            ll_core::ident::NamespacedId::parse("lostland:sneak_attack").expect("合法标识符"),
        );
        let attacker = spawn_agent_with_luck_and_race(&mut world, attacker_pos, luck, race);
        let victim_pos = east_of_spawn(&world);
        let victim = spawn_named_agent(&mut world, victim_pos, 1_000_000);
        let race_traits = FixedSneakRaceGrant { race, trait_id };
        let traits = FixedSneakAttackTrait {
            trait_id,
            luck_chance_permille_per_point: per_point,
            extra_damage,
        };

        let attack = |world: &WorldState| -> i32 {
            resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
                world,
                &Intent::Attack {
                    actor: attacker,
                    target: victim,
                },
                &NoSkills,
                &race_traits,
                &traits,
                &NoResourcePools,
                &NoItems,
                &NoFormulas,
                &NoDamageCategories,
            )
            .iter()
            .find_map(|effect| match effect {
                Effect::Damage { amount, .. } => Some(*amount),
                _ => None,
            })
            .expect("攻击必然产出一条伤害效果")
        };

        // Act
        let damage_visible = attack(&world);
        set_stealthed(&mut world, attacker, true);
        let damage_stealthed = attack(&world);

        // Assert：不潜行时零幸运恒不触发；潜行时精确多出 extra_damage。
        assert_eq!(damage_stealthed, damage_visible + extra_damage);
    }

    #[test]
    fn 潜行中发起攻击会破除潜行() {
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        let victim = spawn_named_agent(&mut world, victim_pos, 1_000_000);
        set_stealthed(&mut world, attacker, true);

        // Act
        let effects = resolve(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
        );

        // Assert：产出一条把攻击者潜行置假的效果，且它排在伤害之后
        // （这一下仍然算潜行中的攻击，见 resolve_attack 文档
        // 「潜行破除」一节）。
        let damage_at = effects
            .iter()
            .position(|effect| matches!(effect, Effect::Damage { .. }))
            .expect("攻击必然产出一条伤害效果");
        let break_at = effects
            .iter()
            .position(|effect| {
                matches!(
                    effect,
                    Effect::SetStealth {
                        actor: a,
                        stealthed: false
                    } if *a == attacker
                )
            })
            .expect("潜行中的攻击应当产出一条破除潜行的效果");
        assert!(break_at > damage_at);
    }

    #[test]
    fn 不在潜行中的攻击不产出破除潜行的效果() {
        // 反面：没有相关状态时不多产一条效果，见 resolve_attack
        // 「潜行破除」一节末尾。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(&mut world);
        let victim_pos = east_of_spawn(&world);
        let victim = spawn_named_agent(&mut world, victim_pos, 1_000_000);

        // Act
        let effects = resolve(
            &world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
        );

        // Assert
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::SetStealth { .. }))
        );
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
    fn 温度这一路没接时与旧入口逐位等价() {
        // `derive_stats` 是 `derive_stats_at(..., TEMPERATE_BASELINE)`
        // 的薄封装，这条测试是那句话的机器检查——黄金基准回放走的正是
        // 不带任何目录的 `resolve`，等价一旦破了，摘要就会变。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let source = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
        let modifiers = std::collections::BTreeMap::from([(
            AttributeKind::Strength,
            std::collections::BTreeMap::from([(
                source,
                ActiveStatModifier {
                    delta: 4,
                    expires_at: Tick(100),
                },
            )]),
        )]);

        // Act
        let legacy = derive_stats(
            BaseStats::BASELINE,
            &modifiers,
            &std::collections::BTreeMap::new(),
            &NoItems,
            Tick(5),
        );
        let explicit = derive_stats_at(
            BaseStats::BASELINE,
            &modifiers,
            &std::collections::BTreeMap::new(),
            &NoItems,
            Tick(5),
            Temperature::TEMPERATE_BASELINE,
        );

        // Assert
        assert_eq!(legacy, explicit);
        assert_eq!(legacy.attribute(AttributeKind::Strength), 14);
    }

    #[test]
    fn 极寒环境削弱力量且只削弱力量() {
        // 惩罚必须落在一个保证被 resolve_attack 读到的量上（力量），
        // 且不该顺手污染别的属性或护甲。
        // Arrange
        let empty = std::collections::BTreeMap::new();

        // Act
        let warm = derive_stats_at(
            BaseStats::BASELINE,
            &empty,
            &std::collections::BTreeMap::new(),
            &NoItems,
            Tick(0),
            Temperature::TEMPERATE_BASELINE,
        );
        let frozen = derive_stats_at(
            BaseStats::BASELINE,
            &empty,
            &std::collections::BTreeMap::new(),
            &NoItems,
            Tick(0),
            Temperature(-120),
        );

        // Assert
        assert!(
            frozen.attribute(AttributeKind::Strength) < warm.attribute(AttributeKind::Strength)
        );
        for kind in [
            AttributeKind::Dexterity,
            AttributeKind::Constitution,
            AttributeKind::Intelligence,
            AttributeKind::Willpower,
            AttributeKind::Charisma,
            AttributeKind::Luck,
        ] {
            assert_eq!(
                frozen.attribute(kind),
                warm.attribute(kind),
                "{kind:?} 不该被暴露惩罚牵连"
            );
        }
        assert_eq!(frozen.armor(), warm.armor());
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
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(&world, victim_pos),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: surface_space_at(&world, victim_pos),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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
        let usable = resource_pool_usable(
            agent,
            pool,
            &race_traits,
            &NO_TRAIT_GRANTS,
            &NO_TRAIT_GRANTS,
            &traits,
        );

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
        let _ = resource_pool_usable(
            agent,
            pool,
            &race_traits,
            &NO_TRAIT_GRANTS,
            &NO_TRAIT_GRANTS,
            &traits,
        );

        // Assert：存储值本身仍然是 8，没有被这次读取改写。
        assert_eq!(
            world.actors.get(actor).unwrap().resource_pools.get(&pool),
            Some(&8)
        );
    }
}
