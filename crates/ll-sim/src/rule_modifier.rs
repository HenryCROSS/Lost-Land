//! 规则修正（[`RuleModifier`]）的**多来源聚合点**——「一个实体此刻身上
//! 有哪些规则修正」这个问题的唯一答案处，以及在其之上的四个消费者
//! （抗性 [`resistance_multiplier_permille`]、偷袭 [`sneak_attack_rule`]、
//! 盘查意愿 [`inspection_suspicion_permille`]、盘查藏匿
//! [`inspection_concealment_permille`]），连同它们共用的那一条 tie-break
//! （[`strongest_by_origin`]）。
//!
//! # 为什么单独立一个模块：项目所有者对抗性来源的裁定
//!
//! 所有者原话：「抗性肯定会来自天赋，以及装备，还有各种药品，或者
//! 技能」——**四路来源**。抗性接线批次落地时只有天赋一路，聚合逻辑
//! 因此直接长在 `crate::traits` 里、直接收 `&[TraitSource]`：那个形状
//! 把「天赋是唯一来源」写死在了函数签名上，接第二路来源必须改签名、
//! 改全部调用点。本模块把那一层拆开：
//!
//! - **每一路来源**各有一个「收集器」，把自己那一路的声明摊平成
//!   [`RuleModifierEntry`]（修正本身 + 它来自哪个内容条目）——天赋走
//!   [`trait_rule_modifiers`]，装备走 [`equipment_rule_modifiers`]。
//! - **消费者**（[`resistance_multiplier_permille`]/[`sneak_attack_rule`]/
//!   [`inspection_suspicion_permille`]/[`inspection_concealment_permille`]）
//!   只收一个 `&[RuleModifierEntry]` 切片，**完全不知道有几路来源、
//!   分别是什么**。
//!
//! 接第三路（技能）、第四路（药品/限时 buff）因此只需要：新写一个
//! 收集器函数，在 [`agent_rule_modifiers`] 里多 `extend` 一次——
//! **全部消费者的签名一个字都不用改**，它们已有的调用点也不用改。
//! 这与 `crate::traits::TraitSource` 当初把「多传一个参数」落成「切片
//! 里多一个元素」是同一条手法，只是这次上升了一层：那一层解决的是
//! 「天赋归谁所有」有几路，这一层解决的是「规则修正本身」有几路。
//! 本模块的单元测试 `第三路来源不改任何消费者签名就能接进聚合结果`
//! 直接把这条主张钉成可执行的断言。
//!
//! # ADR 0021 复核：为什么是「一个聚合点」，不是四套并列实现
//!
//! ADR 0021 的判据是「有没有一份算法要被多种来源共用」，不是对称性。
//! 这里确实有：**「多条命中时取哪一条」这条 tie-break 规则对四路来源
//! 是同一段代码**——`trait-system.md` 三节③原文「按 `ContentIndex`
//! 升序取第一条……不取乘积，理由是『免疫 500‰ 又免疫一次』不应该变成
//! 25% 而不是 0%」这条论证与「这条抗性是天赋给的还是护符给的」完全
//! 无关。既然规则本身与来源无关，就不该让每一路来源各自实现一遍。
//!
//! # 为什么不复用 `crate::resolve::derive_stats`
//!
//! `derive_stats` 是本项目**已经走通**的另一个多来源聚合点（同时吃
//! 装备的 `StatBonus` 与 `active_stat_modifiers` 的限时修正），形状
//! 上很像，但**算法不同**，按 ADR 0021 不该合并：
//!
//! - `derive_stats` 的合并规则是**求和**——两件装备各加 3 点力量就是
//!   加 6 点，两条来源互不排斥。
//! - 抗性的合并规则是**取一条**（见上）——两条 500‰ 抗性不是 250‰，
//!   也不是 1000‰，就是 500‰。
//!
//! 两者的输出形状也不同：`derive_stats` 产出一个定长数组
//! （七项属性 + 护甲，编译期已知），抗性按 `damage_category` 分类，
//! 而伤害类别是 `damage-formula-mod-api.md` 十七节开放注册的
//! `ContentIndex` 集合，没有定长数组可言。硬合并只会得到一个内部
//! 分两条互不相干路径的函数，那不是复用，是把两件事塞进一个名字。
//!
//! # 顺序为什么确定（约束 C5）
//!
//! [`agent_rule_modifiers`] 全程只遍历 `Vec` 与 `BTreeMap`：天赋这一路
//! 的顺序由 `crate::traits::effective_traits` 决定（各来源表内部的
//! `Vec` 顺序，见其文档「多路来源之间的顺序」一节），装备这一路按
//! `Agent::equipment` 这个 `BTreeMap<EquipSlot, _>` 的键升序，两路之间
//! 按本函数里写死的 `extend` 顺序——全程不触碰任何 `HashMap`/`HashSet`。
//!
//! 更进一步：全部消费者的 tie-break **强度相同时显式按 `origin` 升序**
//! 取第一条，因此即便未来某一路来源的内部顺序发生变化，结果也不会跟着
//! 变——顺序确定性在这里是双保险，不是只靠遍历顺序。
//!
//! # 跨来源 tie-break：先取最强，同强度才按 `origin` 升序
//!
//! `origin` 是内容条目自己的 [`ContentIndex`]，天赋与物品共用同一个
//! 全局号段（`ll_core::ident::Interner`），因此「天赋给的抗性」与
//! 「装备给的抗性」落在同一把尺子上比大小，结果确定、可复现。抗性接线
//! 批次落地时这把尺子是**唯一**判据（按 `origin` 升序取第一条），当时
//! 就在本节如实记录了它的问题：这把尺子的数值只反映**注册顺序**，不
//! 反映「哪一条更强」——同一个伤害类别上同时有天赋抗性与装备抗性时，
//! 谁生效取决于谁先被 intern，一枚平庸护符可以压过一条强天赋。
//!
//! **项目所有者已就此裁定：改成取最强的一条。** 本模块因此把判据分成
//! 两级：
//!
//! 1. **先比强度**——由 [`strength_key`] 逐变体声明「哪边算强」，见下节。
//! 2. **强度完全相同才比 `origin`**，仍然升序取第一条（约束 C5：结果
//!    不得依赖切片顺序，见 [`strongest_by_origin`] 文档「约束 C5」一节）。
//!
//! 被这条裁定取代的是**判据的第一级**，不是「不取乘积」那条论证——
//! `trait-system.md` 三节③原文「不取乘积，理由是『免疫 500‰ 又免疫
//! 一次』不应该变成 25% 而不是 0%」照旧成立：本模块仍然**只挑一条**，
//! 从不合并两条。裁定改的是「挑哪一条」，不是「挑几条」。
//!
//! # 「强」的方向为什么必须逐变体声明，不能写一个通用的「取最大值」
//!
//! 四个消费者的强弱方向**互不相同**：[`RuleModifier::Resistance`] 与
//! [`RuleModifier::InspectionSuspicion`] 的千分比**越小越强**（`500‰`
//! 抗性是只受一半伤害、`0` 是免疫），[`RuleModifier::SneakAttack`] 的
//! 追加伤害与 [`RuleModifier::InspectionConcealment`] 的藏匿概率**越大
//! 越强**。一个通用的「取最大值」会让前两个变体反过来选**最弱**的那
//! 一条——这不是一个能靠调用点小心传对比较器来规避的问题，而是「哪天
//! 有人加第五个变体、忘了传」的形态。
//!
//! 因此方向不由调用点携带，而是集中在 [`strength_key`] 一个**无通配
//! 分支的穷尽 `match`** 里逐变体声明：新增一个变体而不声明它的方向，
//! `cargo build` 直接不过。这与 `ll_mod::content_hash` 的
//! `ContentTableKind`/`classify_index`「编译期强制穷尽」是同一条手法
//! （见该模块文档「编译期强制：穷尽解构 tables」一节），只是这次强制
//! 的对象是「比较方向」而不是「哈希覆盖面」。
//!
//! # 热路径（ADR 0016/0017）
//!
//! 与 `crate::traits` 模块文档「为什么不缓存」一节同一档：本模块的
//! 聚合每次要用时现算，调用频率是「每次攻击结算一次」，不是逐格/逐帧。
//! 全程纯 Rust，不跨脚本边界（ADR 0016 一档：抗性是静态声明）。
//! 全程整数（千分比，ADR 0020 乙区），不引入任何 `f32` 中间值。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_world::entity::Agent;

use crate::item::{EquipSlot, ItemCatalog, ItemStack};
use crate::traits::{
    TraitCatalog, TraitGrantSource, TraitSource, agent_trait_sources, effective_traits,
};

/// 千分比乘数的分母——`RuleModifier::Resistance::multiplier_permille`
/// 与 `resistance_multiplier_permille` 返回值共用同一把刻度尺：`1000`
/// 表示「无抗性」（乘数 1.0），`0` 表示免疫，`500` 表示半伤，`2000`
/// 表示双倍，与 `crate::combat` 模块「所有数值一律整数，百分比一律用
/// 千分比」的既有惯例同一条纪律。
pub const RESISTANCE_MULTIPLIER_SCALE: i32 = 1000;

/// 天赋效果③「改变规则本身」——`knowledge/design/trait-system.md` 三节
/// 定形的封闭枚举，走注册表第一档（声明式）。
///
/// 类型定义现居 `ll-sim`（伤害类别/抗性接线批次从 `ll_mod::trait_def`
/// 挪过来）——`crate::resolve` 需要在决策层直接 `match`/引用
/// [`RuleModifier::Resistance`] 才能把它接进伤害管线，而依赖方向
/// （`ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`，规格 §5）不允许
/// `ll-sim` 反过来依赖 `ll-mod`——与 [`crate::resource_pool::ResourcePoolGrant`] 当初「挪到
/// `ll-sim`，`ll_mod::trait_def` 改为 `pub use` 复用同一份声明」是同一条
/// 先例（见该类型在 `ll_mod::trait_def` 里的文档「类型定义现移居
/// `ll_sim::resource_pool`」一节），本次是它在 `RuleModifier` 上的第二次
/// 应用。
///
/// # 本批次接线状态
///
/// [`RuleModifier::Resistance`] 现在有真实的 `resolve` 侧消费者——见
/// [`resistance_multiplier_permille`] 与
/// `crate::resolve::resolve_attack` 文档「抗性接线」一节；
/// [`RuleModifier::SneakAttack`] 见同一函数「偷袭接线」一节；
/// [`RuleModifier::InspectionConcealment`] 见
/// `crate::resolve::resolve_inspect` 文档「藏匿判定」一节；
/// [`RuleModifier::InspectionSuspicion`] 的消费者在**脚本侧**
/// （`ll_mod::script_behavior_api` 的 `actor-inspection-suspicion`），
/// 理由见该变体文档。**仍然没有任何消费者的只剩下面这三个**：
/// - [`RuleModifier::RerollOnce`] 需要 `roll_one_die` 钩子（伤害公式
///   引擎求值器内部的骰子取数原语），本批次不改写该求值器签名，见
///   `trait-system.md` 三节③「重骰」一节「代价诚实标注」段落。
/// - [`RuleModifier::Advantage`]/[`RuleModifier::Disadvantage`] 需要
///   本项目当前不存在的判定/检定系统,见同节「占位变体」说明。
///
/// # 与 10% 下限的关系
///
/// `damage-formula-mod-api.md` 二十节「与 10% 下限的关系：不冲突，但要
/// 如实说明『免疫』能让下限之后的结果归零」——`crate::combat::damage_after_defense`
/// 内部的 10% 下限保证的是**减伤链路本身**不会因为防御过高被压到零以
/// 下，抗性乘数作用在这条链路**之后**（见 `crate::resolve::resolve_attack`
/// 文档「抗性接线」一节），下限的保证只覆盖到减伤链路那一步：免疫
/// （乘数 0）会合法地把抗性这一步的结果打成 0，即使上一步的减后伤害
/// 满足了 10% 下限——两者不冲突，因为它们从来不覆盖同一个问题：10%
/// 下限回答「打不打得穿盔甲」，抗性回答「这种伤害对这个目标有没有
/// 意义」。免疫因此产出 0 伤害，不是钳到下限伤害——那样"免疫"这个词
/// 就名不副实了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleModifier {
    /// 抗性：该伤害类别的伤害，在既有减伤链路算完之后再打一个千分比
    /// 折扣——挂载点见 `damage-formula-mod-api.md` 二十节（减伤之后、
    /// 乘数形式）。`0`=免疫，`500`=半伤，`2000`=双倍。
    Resistance {
        /// 伤害类别，走 `damage-formula-mod-api.md` 十七节的开放
        /// `register-damage-category` 集合。
        damage_category: ContentIndex,
        /// 千分比乘数，见 [`RESISTANCE_MULTIPLIER_SCALE`]。
        multiplier_permille: i32,
    },
    /// 重骰：该实体掷骰抽出 `value` 时,立即重抽一次,取新值（不再检查
    /// 新值是否又是 `value`）。
    RerollOnce {
        /// 触发重骰的点数。
        value: i32,
    },
    /// 优势：该实体在 `check_context` 这类判定上默认套用优势——占位
    /// 变体，当前无消费者（本项目没有判定/检定系统）。
    Advantage {
        /// 判定种类的开放标识符,具体值域留给判定系统落地时定案。
        check_context: NamespacedId,
    },
    /// 劣势，语义同 [`RuleModifier::Advantage`]，方向相反。
    Disadvantage {
        /// 判定种类的开放标识符。
        check_context: NamespacedId,
    },
    /// 偷袭（盗贼偷袭接线批次新增）——所有者对「盗贼偷袭」的裁定原话：
    /// 「盗贼偷袭做成技能判定吧，通过幸运值之类的属性以及一定的随机值
    /// 组合一下」。`trait-system.md` 曾判定盗贼偷袭表达不了：真实条件
    /// 「目标旁边有我的盟友」需要一次本项目当前不存在的空间查询（`fov`/
    /// `light` 两个决策层文件都不回答「谁站在谁旁边」这个问题）。所有者
    /// 的裁定绕开了这条依赖——改成幸运影响的判定，不需要知道周围有谁,
    /// 与暴击（[`crate::combat::crit_chance_permille`]）是「战斗结算里
    /// 现成的、幸运能挂上去的判定点」同一个思路,但刻意不是暴击本身：
    /// 暴击对**全部**攻击者恒定生效（系数写死在
    /// [`crate::combat::LUCK_CRIT_BONUS_PERMILLE`]），偷袭是**只有声明
    /// 了这条天赋的角色才会触发**的判定，系数由天赋声明本身携带
    /// （`luck_chance_permille_per_point`）——不同天赋可以有不同的幸运
    /// 敏感度,不共用暴击那个全局系数,见
    /// [`crate::combat::sneak_attack_chance_permille`] 文档。
    SneakAttack {
        /// 每点有效幸运贡献的触发率加成，千分比——与
        /// [`crate::combat::LUCK_CRIT_BONUS_PERMILLE`] 同一套"幸运→
        /// 千分比概率"换算手法，但这里的系数是天赋自己的声明值，不是
        /// 硬编码进 `combat.rs` 的全局常量。
        luck_chance_permille_per_point: i32,
        /// 触发后追加的固定伤害——挂载点见
        /// `crate::resolve::resolve_attack` 文档「偷袭接线」一节：加在
        /// 暴击放大之后、抗性乘数之前,与暴击、抗性同一条"减伤链路本身
        /// 不变,后续效果各自在它的结果上再叠一层"既有纪律。
        extra_damage: i32,
    },
    /// 被动①**「不觉得可疑」**（盗贼被动两分批次）——项目所有者裁定
    /// 原话：「被动可以分为 **2 种**，**不觉得可疑**，还有**查不出
    /// 东西**」。本变体是前一种：**降低别人对这个实体发起盘查的
    /// 意愿**，千分比乘数，刻度尺与 [`RESISTANCE_MULTIPLIER_SCALE`]
    /// 共用（`1000` = 与常人无异，`0` = 永远不会被怀疑，`500` = 只有
    /// 一半的概率被盯上）。
    ///
    /// # 消费者在脚本侧，不在 `resolve` 侧
    ///
    /// 这是本变体与本枚举其余全部变体的唯一实质差异，也是它必须与
    /// [`RuleModifier::InspectionConcealment`] 分成两个变体、而不是
    /// 合成一个的理由：「要不要发起盘查」这个决策**根本不经过
    /// `resolve`**——它整个发生在 AI 决策阶段（`guard-ai-tree` 的
    /// 那一次掷骰，见 `ll_mod::native_behavior::guard_inspect_chance`），
    /// `Intent::Inspect` 一旦产出，`crate::resolve::resolve_inspect`
    /// 恒执行、不重新判断「该不该查」（见该函数文档「谁来判断该不该
    /// 发起这次盘查」一节）。本变体的值因此经
    /// `ll_mod::script_behavior_api` 的 `actor-inspection-suspicion`
    /// 暴露给行为树，由脚本自己决定怎么把它乘进那次掷骰的概率——
    /// 与「盘查触发率本身是一条具名常量」（同一个模块的
    /// `GUARD_INSPECT_CHANCE_PERMILLE`）是同一条可编辑性纪律。
    ///
    /// 聚合与 tie-break 仍然完全走 [`agent_rule_modifiers`]——脚本
    /// 拿到的是本模块 [`inspection_suspicion_permille`] 算完的**一个
    /// 数**，不是一份候选列表：多来源取哪一条这件事不下放给脚本，
    /// 理由同本模块文档「跨来源 tie-break」一节。
    InspectionSuspicion {
        /// 千分比乘数，见本变体文档；与
        /// [`RESISTANCE_MULTIPLIER_SCALE`] 同一把刻度尺。
        multiplier_permille: i32,
    },
    /// 被动②**「查不出东西」**（盗贼被动两分批次）——所有者裁定里的
    /// 后一种：盘查**照常发起**，只是搜身的人看不到你身上的东西。
    /// `conceal_permille` 是**每一件**物品各自不被看见的千分比概率
    /// （`0` = 藏不住任何东西，`1000` = 什么都查不出来）。
    ///
    /// # 为什么是逐件掷骰，不是「全藏」也不是「藏固定几件」
    ///
    /// 三种形状都能表达「查不出东西」，选逐件概率的理由是
    /// [`crate::effect::Effect::Inspect`] 已经写死的那个未来消费者：
    /// 该效果文档「为什么没有任何是否违法的判断」一节说明，等
    /// `Owner`/`stolen_marker` 落地之后，下游要做的事是**逐堆**比对
    /// `items_seen` 与各堆的 `owner`。那条比对的粒度是「单件物品」，
    /// 因此本被动的粒度也必须是单件——
    ///
    /// - 「全藏」（一次掷骰决定整次盘查看不看得见）会让这条被动退化
    ///   成一枚「本次犯罪是否被发现」的硬币：赃物与十件干净的杂物
    ///   共享同一次判定，玩家带多少东西完全不影响结果，未来那条逐堆
    ///   比对拿到的永远是「全部」或「空」两种输入。
    /// - 「藏固定 N 件」需要回答「藏哪 N 件」，而 `items_seen` 的顺序
    ///   （先背包原始顺序、后装备槽位升序）是一条存储顺序，不带任何
    ///   「哪件更该被藏」的语义——按它取前 N 件是一条看起来确定、
    ///   实则任意的规则。
    ///
    /// 逐件掷骰两个问题都没有：粒度对得上未来的消费者，且不需要发明
    /// 任何「哪件更该被藏」的排序依据。代价是它消费随机数——判定走
    /// `DetRng::for_entity`（约束 C3），取数顺序即 `items_seen` 自身
    /// 的确定顺序（约束 C5），见
    /// `crate::resolve::resolve_inspect` 文档「藏匿判定」一节。
    InspectionConcealment {
        /// 每一件物品各自不被看见的千分比概率，见本变体文档。
        conceal_permille: i32,
    },
}

/// 一条候选规则修正：修正本身 + **它来自哪个内容条目**。
///
/// # 为什么要带 `origin`，不是裸 [`RuleModifier`] 列表
///
/// 全部消费者的 tie-break 规则（`trait-system.md` 三节③「按
/// `ContentIndex` 升序取第一条」）要的正是这个值——聚合之前它还能从
/// 「第几条天赋」隐式推出来，聚合之后来源被摊平成一个列表，若不随身
/// 带上就彻底丢了。带上之后还有第二个好处：跨来源比较有了统一的键
/// （天赋 id 与物品 id 共用同一个全局 [`ContentIndex`] 号段），不需要
/// 为「天赋给的」与「装备给的」各发明一套排序依据，见模块文档
/// 「跨来源 tie-break」一节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleModifierEntry {
    /// 声明这条修正的内容条目——天赋这一路是天赋自己的索引，装备这
    /// 一路是物品定义（`ItemDef`）的索引。
    pub origin: ContentIndex,
    /// 修正本身。
    pub modifier: RuleModifier,
}

/// 来源①**天赋**：把一个实体全部有效天赋（`crate::traits::effective_traits`）
/// 声明的 [`RuleModifier`] 摊平成候选列表，`origin` 取天赋自己的索引。
///
/// 查不到定义的天赋索引直接跳过（ADR 0015：查不到就是查不到），与
/// `crate::traits::granted_skills` 同一条既有纪律。
pub fn trait_rule_modifiers(
    sources: &[TraitSource<'_>],
    level: i32,
    traits: &dyn TraitCatalog,
) -> Vec<RuleModifierEntry> {
    let mut result = Vec::new();
    for trait_id in effective_traits(sources, level) {
        let Some(rule) = traits.trait_rule(trait_id) else {
            continue;
        };
        for modifier in rule.rule_modifiers {
            result.push(RuleModifierEntry {
                origin: trait_id,
                modifier,
            });
        }
    }
    result
}

/// 来源②**装备**（本批次新增的第二路来源，落地项目所有者「抗性……来自
/// ……装备」的裁定）：把一个实体已装备物品声明的 [`RuleModifier`]
/// （`crate::item::ItemRule::rule_modifiers`）摊平成候选列表，`origin`
/// 取物品定义的索引。
///
/// # 耐久归零的装备不贡献任何规则修正
///
/// `item-system.md` 六节裁定「归零 = 损坏不可用，但不消失」——
/// `crate::resolve::derive_stats` 已经把这句话落在「不再贡献属性加成」
/// 上（见其文档「耐久归零」一节），本函数是同一句话在规则修正这条
/// 通道上的落点：一件耐久归零的护符仍然戴在脖子上，但它声明的抗性
/// 不再生效。两处用完全相同的判据（`durability == Some(0)`）——`None`
/// （没有耐久概念的物品）与 `Some(正数)` 都照常生效。
///
/// # 顺序（约束 C5）
///
/// `equipment` 是 `BTreeMap<EquipSlot, _>`，遍历按槽位升序，不依赖
/// 插入顺序、不涉及任何 `HashMap`。
pub fn equipment_rule_modifiers(
    equipment: &BTreeMap<EquipSlot, ItemStack>,
    items: &dyn ItemCatalog,
) -> Vec<RuleModifierEntry> {
    let mut result = Vec::new();
    for stack in equipment.values() {
        if stack.durability == Some(0) {
            continue;
        }
        let Some(rule) = items.item(stack.def) else {
            continue;
        };
        for modifier in rule.rule_modifiers {
            result.push(RuleModifierEntry {
                origin: stack.def,
                modifier,
            });
        }
    }
    result
}

/// 把一个实体身上**已接线的全部来源**汇总成一个候选列表——本模块的
/// 聚合点本身，见模块文档。
///
/// # 接第三、第四路来源时改哪里
///
/// 只改本函数：新写一个收集器（形如 [`equipment_rule_modifiers`]），
/// 在这里多 `extend` 一次。四个消费者（[`resistance_multiplier_permille`]/
/// [`sneak_attack_rule`]/[`inspection_suspicion_permille`]/
/// [`inspection_concealment_permille`]）与它们在 `crate::resolve`／脚本
/// 侧的调用点都不需要改动一个字符——这正是 `crate::traits::agent_trait_sources` 文档
/// 「其余三路为什么不在这里」所描述的那种「调用点不需要改一行」，
/// 只是这次覆盖的是「规则修正有几路来源」这一层。
///
/// 尚未接线的两路，以及各自真正缺的东西：
/// - **技能**（来源③）：`ll_mod::skill::SkillDef` 当前没有
///   `rule_modifiers` 字段（已核实），技能因此还无法声明任何规则修正。
///   需要区分「被动技能常驻」与「主动技能施加限时 buff」两件事——前者
///   与本函数已有的两路同构（一个收集器即可），后者属于来源④。
/// - **药品/限时 buff**（来源④）：`ll_world::entity::Agent` 当前唯一的
///   限时效果容器是 `active_stat_modifiers`，它按 `AttributeKind` 分类
///   （见其字段文档），而抗性按 `damage_category`（一个开放注册表）
///   分类，两者的键空间不同，装不进同一个容器。需要一个新的、按
///   `(damage_category, 来源)` 做键的限时容器，且它是世界状态的一部分
///   （ADR 0022：要进 `WorldState::hash()`、要进存档）——这是四路里
///   唯一牵动存储层的一路，成本最高，本批次不做。
pub fn agent_rule_modifiers(
    agent: &Agent,
    race_grants: &dyn TraitGrantSource,
    class_grants: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    items: &dyn ItemCatalog,
) -> Vec<RuleModifierEntry> {
    let mut result = trait_rule_modifiers(
        &agent_trait_sources(agent, race_grants, class_grants),
        agent.level,
        traits,
    );
    result.extend(equipment_rule_modifiers(&agent.equipment, items));
    result
}

/// 抗性消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里，取
/// `damage_category` 匹配的 [`RuleModifier::Resistance`] 的乘数；一条
/// 也没命中时返回 [`RESISTANCE_MULTIPLIER_SCALE`]（乘数 1.0，等价于
/// 「没有抗性」）。
///
/// `crate::resolve::resolve_attack`（伤害类别/抗性接线批次）在减伤链路
/// 算完之后调用本函数，拿到的乘数按 `damage-formula-mod-api.md` 二十节
/// 挂在「减伤之后」，见其文档「抗性接线」一节。
///
/// # 多条命中时取哪一条：取最强（乘数最小）的一条，同强度才按 `origin` 升序
///
/// `trait-system.md` 三节③「抗性：挂载点已经现成」一节原文：「按
/// `TraitGrant` 的 `ContentIndex` 升序取第一条……不取乘积,理由是『免疫
/// 500‰ 又免疫一次』不应该变成 25% 而不是 0%」。**「不取乘积」这半句
/// 照旧**——本函数仍然只挑一条，从不合并；「按 `ContentIndex` 升序取
/// 第一条」那半句已被项目所有者裁定取代为「取最强的一条」，完整论证
/// 见模块文档「跨来源 tie-break」一节。
///
/// 抗性的「强」是**乘数越小越强**（`500‰` = 只受一半伤害，`0` = 免疫），
/// 方向在 [`strength_key`] 里逐变体声明,不由本函数携带。
///
/// 两级判据都**不依赖 `modifiers` 切片自身的顺序**（约束 C5：结果只与
/// 声明值与 `ContentIndex` 数值有关，与调用方按什么顺序把各路来源拼进
/// 切片无关）。
pub fn resistance_multiplier_permille(
    modifiers: &[RuleModifierEntry],
    damage_category: ContentIndex,
) -> i32 {
    strongest_by_origin(modifiers, |modifier| match modifier {
        RuleModifier::Resistance {
            damage_category: candidate_category,
            multiplier_permille,
        } if *candidate_category == damage_category => Some(*multiplier_permille),
        _ => None,
    })
    .unwrap_or(RESISTANCE_MULTIPLIER_SCALE)
}

/// 一条规则修正的**强度比较键**——把「哪边算强」这件逐变体不同的事，
/// 规范化成一个统一的「越大越强」的整数键，好让
/// [`strongest_by_origin`] 只剩「取键最大的一条」这一件事要做。
///
/// # 为什么是两级，不是一个数
///
/// [`RuleModifier::SneakAttack`] 携带**两个**数（追加伤害与幸运敏感度），
/// 两个都是「越大越强」，谁作主键见 [`strength_key`] 文档「偷袭那两个
/// 字段」一节。其余变体只用得上第一级，第二级恒为 `0`。派生的 [`Ord`]
/// 对元组结构体按字段声明顺序做字典序比较，正是这里要的语义。
///
/// # 为什么是 `i64` 而不是 `i32`
///
/// [`StrengthKey::smaller_is_stronger`] 要取负——`i32::MIN` 取负在
/// `i32` 里溢出（这是一个真实可达的声明值，`multiplier_permille` 的
/// 类型就是 `i32`，注册期不禁止负数）。先拓宽到 `i64` 再取负,值域上
/// 不可能溢出。全程整数，不引入任何浮点（ADR 0002/0020）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct StrengthKey(i64, i64);

impl StrengthKey {
    /// 「这个变体没有声明比较方向」——恒等的键，任意两条都判定为同强度，
    /// 于是完全退回 `origin` 升序，也就是本次裁定之前的旧行为。
    ///
    /// 只用于**当前没有任何消费者**的那几个变体（见 [`strength_key`]
    /// 文档「没有消费者的变体」一节）：它们从来不会进入
    /// [`strongest_by_origin`]（没有 `select` 会认领它们），这个值因此
    /// 不是「随便填的默认」，而是「等真正的消费者落地时，由接线的那一批
    /// 一并裁定方向」的诚实占位。
    const INDISTINGUISHABLE: StrengthKey = StrengthKey(0, 0);

    /// 声明「这个数越大越强」。
    const fn larger_is_stronger(value: i32) -> Self {
        StrengthKey(value as i64, 0)
    }

    /// 声明「这个数越小越强」——抗性/盘查意愿那两条千分比乘数
    /// （`0` = 免疫 / 永远不会被怀疑）。取负之后与
    /// [`StrengthKey::larger_is_stronger`] 落在同一把「越大越强」的
    /// 尺子上，比较逻辑因此只有一份。
    const fn smaller_is_stronger(value: i32) -> Self {
        StrengthKey(-(value as i64), 0)
    }

    /// 追加第二级比较键（越大越强），主键相同时才用得上。
    const fn then_larger_is_stronger(self, value: i32) -> Self {
        StrengthKey(self.0, value as i64)
    }
}

/// 逐变体声明「哪边算强」——本模块**唯一**回答这个问题的地方。
///
/// # 无通配分支：新增变体不可能忘了声明方向
///
/// 下面这个 `match` 刻意**没有** `_ =>` 兜底：新增一个 [`RuleModifier`]
/// 变体而不在这里补一条，`cargo build` 直接不过。这正是项目所有者要求
/// 「每个变体各自声明比较方向，不要靠调用点记得传对比较器」的落点——
/// 方向不是调用点的参数，是变体自己的属性，完整论证见模块文档
/// 「『强』的方向为什么必须逐变体声明」一节。
///
/// # 偷袭那两个字段
///
/// [`RuleModifier::SneakAttack`] 是唯一携带两个数值字段的变体。主键取
/// `extra_damage`（项目所有者本次裁定里点名的那一个），
/// `luck_chance_permille_per_point` 作第二级——**这是对所有者裁定的
/// 细化，不是改写**：所有者说的「追加伤害越大越强」原样成立，第二级
/// 只在追加伤害完全相同时才起作用，而那正是所有者那句话没有覆盖、
/// 原本会直接掉进「谁先被 intern 谁赢」的区间。两个字段都是「越大越
/// 强」，方向上没有歧义；刻意**不**把两者相乘成「期望额外伤害」——那
/// 需要知道这次判定的有效幸运值（[`crate::combat::sneak_attack_chance_permille`]
/// 的输入），聚合层拿不到，硬造一个模型只会是一条看起来精确、实则
/// 凭空发明的规则。
///
/// # 没有消费者的变体
///
/// [`RuleModifier::RerollOnce`]/[`RuleModifier::Advantage`]/
/// [`RuleModifier::Disadvantage`] 当前没有任何消费者（见 [`RuleModifier`]
/// 文档「本批次接线状态」一节），也就没有任何 `select` 会认领它们，
/// 因此永远不会真的参与一次强度比较。它们取
/// [`StrengthKey::INDISTINGUISHABLE`]：不假装已经裁定过方向,等真正
/// 接线的那一批连同消费者一起决定。
///
/// # 为什么这里用 `R` 别名而不是写全 `RuleModifier::变体名`
///
/// `scripts/ci/check_field_consumers.py` 这道门禁按
/// 「决策层文件里有没有出现 `RuleModifier::变体名` 字面量」判定一个变体
/// 是否已被游戏逻辑消费,上面三个没有消费者的变体正显式登记在它的
/// `EXEMPTIONS` 清单里。本函数**不读它们的任何字段**（返回的是
/// [`StrengthKey::INDISTINGUISHABLE`]），只是为了穷尽性必须点到名字；
/// 若在这里写出 `RuleModifier::RerollOnce` 这样的字面量，那道门禁会把
/// 三个死变体误判成「已接线」，从此对它们形同虚设。用别名 `R` 写这个
/// `match`,与 `RaceDef.stat_modifiers` 当初刻意把 trait 方法命名成
/// `race_stat_modifiers`（见该门禁 `EXEMPTIONS` 里那一条的理由文字）
/// 是同一条既有纪律：不为了让门禁看起来更绿而换来一份实际更弱的门禁。
fn strength_key(modifier: &RuleModifier) -> StrengthKey {
    use crate::rule_modifier::RuleModifier as R;
    match modifier {
        // 千分比乘数，越小越强：`0` = 免疫。
        R::Resistance {
            multiplier_permille,
            ..
        } => StrengthKey::smaller_is_stronger(*multiplier_permille),
        // 千分比乘数，越小越强：`0` = 永远不会被怀疑。
        R::InspectionSuspicion {
            multiplier_permille,
        } => StrengthKey::smaller_is_stronger(*multiplier_permille),
        // 千分比概率，越大越强：`1000` = 什么都查不出来。
        R::InspectionConcealment { conceal_permille } => {
            StrengthKey::larger_is_stronger(*conceal_permille)
        }
        // 两个字段都越大越强，主键是追加伤害，见本函数文档「偷袭那两个字段」。
        R::SneakAttack {
            luck_chance_permille_per_point,
            extra_damage,
        } => StrengthKey::larger_is_stronger(*extra_damage)
            .then_larger_is_stronger(*luck_chance_permille_per_point),
        // 以下三个当前没有消费者，见本函数文档「没有消费者的变体」一节。
        R::RerollOnce { .. } | R::Advantage { .. } | R::Disadvantage { .. } => {
            StrengthKey::INDISTINGUISHABLE
        }
    }
}

/// 全部消费者共用的 tie-break：在候选列表里，只看 `select` 认领的那些
/// 条目，返回**最强**的那一条的投影值——强度由 [`strength_key`] 逐变体
/// 声明；强度完全相同的多条之间，取 `origin`（[`ContentIndex`]）**最小**
/// 的那一条。一条也没有认领时返回 `None`。
///
/// # ADR 0021 复核：为什么这一层值得抽出来
///
/// ADR 0021 的判据是「有没有一份算法要被多种消费者共用」，不是对称。
/// 这里确实有，而且是**同一段代码在本模块内被逐字重复了四次**：
/// [`resistance_multiplier_permille`]、[`sneak_attack_rule`]、
/// [`inspection_suspicion_permille`]、[`inspection_concealment_permille`]
/// 四个消费者对「多条命中时取哪一条」的回答完全相同——
/// `trait-system.md` 三节③原文「按 `ContentIndex` 升序取第一条……不取
/// 乘积」——那条论证与「这条修正是抗性、偷袭、还是盘查减免」完全无关，
/// 与「它来自天赋还是装备」同样无关（后者正是本模块存在的理由）。
///
/// 抽出来之前只有两个消费者，两份三行的循环还谈不上「一份算法」；
/// 盗贼被动两分批次要再添两个，四份逐字相同的副本已经越过了 ADR 0021
/// 的门槛：真正的风险不是行数，是**四份副本各自漂移**——任何一份把
/// `<=` 写成 `<`（tie 时取后一条而不是前一条）都会让那一路悄悄依赖
/// 切片顺序，而切片顺序恰恰是约束 C5 要求结果**不得**依赖的东西。
///
/// 「取最强」裁定落地之后这条理由**更强了，不是更弱**：判据从一级
/// （`origin`）变成两级（强度 → `origin`），每一路要自己写对的东西从
/// 「一个比较符的方向」变成「一个比较符的方向 + 这个变体的强弱方向 +
/// 同强度时的退化规则」，四份副本各自漂移的面积恰好翻了三倍。方向本身
/// 也没有下放给这里的调用点——它在 [`strength_key`] 那个穷尽 `match`
/// 里逐变体声明，本函数一视同仁只比键（见该函数文档）。
///
/// # 为什么投影用闭包，不是让四个变体实现同一个 trait
///
/// 四个消费者的返回类型互不相同（`i32`/[`SneakAttackRule`]/`i32`/
/// `i32`），且各自的「认领条件」也不同（抗性还要比对
/// `damage_category`，其余三个只看变体本身）。用一个
/// `FnMut(&RuleModifier) -> Option<T>` 把这两件事一起交给调用方，
/// 本函数就只剩下「取 `origin` 最小的一条」这一件事——那正是要共用的
/// 那一份算法，不多不少。
///
/// # 约束 C5
///
/// 两级判据**都**与 `modifiers` 切片自身的顺序无关：先取强度键最大的，
/// 强度相同再取 `origin` 最小的，两级都是严格比较才替换。剩下的唯一
/// 可能「谁先出现谁赢」的情形是**强度键与 `origin` 同时相等**——那意味
/// 着两条来自同一个内容条目、且数值逐字相同（同一条天赋把同一条修正
/// 声明了两遍），谁赢在可观察结果上没有任何差别；即便如此，先后仍由
/// 该条目自己的声明顺序决定，同样是注册期写死的确定顺序。
fn strongest_by_origin<T>(
    modifiers: &[RuleModifierEntry],
    mut select: impl FnMut(&RuleModifier) -> Option<T>,
) -> Option<T> {
    let mut best: Option<(StrengthKey, ContentIndex, T)> = None;
    for entry in modifiers {
        let Some(value) = select(&entry.modifier) else {
            continue;
        };
        let key = strength_key(&entry.modifier);
        let wins = match &best {
            None => true,
            Some((best_key, best_origin, _)) => {
                key > *best_key || (key == *best_key && entry.origin < *best_origin)
            }
        };
        if wins {
            best = Some((key, entry.origin, value));
        }
    }
    best.map(|(_, _, value)| value)
}

/// 「与常人无异」的盘查意愿刻度——[`RuleModifier::InspectionSuspicion`]
/// 与 [`inspection_suspicion_permille`] 返回值共用的分母，数值上与
/// [`RESISTANCE_MULTIPLIER_SCALE`] 相同（都是千分比的 `1000`），但
/// **刻意不复用同一个常量名**：两者回答的是完全不同的两个问题（「这种
/// 伤害对我有多少意义」vs「别人有多想搜我的身」），共用一个名字只会
/// 让将来任何一边想换刻度时误以为必须一起换。
pub const INSPECTION_SUSPICION_SCALE: i32 = 1000;

/// 被动①消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里取
/// [`RuleModifier::InspectionSuspicion`] 的乘数；一条也没有时返回
/// [`INSPECTION_SUSPICION_SCALE`]（与常人无异）。
///
/// 真正的消费点在**脚本侧**（`ll_mod::script_behavior_api` 的
/// 「盘查意愿」查询 → `ll_mod::native_behavior` 的
/// `guard-inspect-chance`），不是 `crate::resolve`——理由见
/// [`RuleModifier::InspectionSuspicion`] 文档「消费者在脚本侧」一节。
/// 聚合与 tie-break 仍然留在这里：脚本拿到的是算完的一个数。
///
/// tie-break 与 [`resistance_multiplier_permille`] 同一条纪律
/// （[`strongest_by_origin`]）：取最强的一条、同强度才按 `origin` 升序，
/// 不取乘积——「不觉得可疑 500‰ 又不觉得可疑一次」不该变成 250‰，同
/// `trait-system.md` 三节③对「免疫两次」的原始论证。本变体的「强」与
/// 抗性同向（**乘数越小越强**，`0` = 永远不会被怀疑），方向声明在
/// [`strength_key`]。
pub fn inspection_suspicion_permille(modifiers: &[RuleModifierEntry]) -> i32 {
    strongest_by_origin(modifiers, |modifier| match modifier {
        RuleModifier::InspectionSuspicion {
            multiplier_permille,
        } => Some(*multiplier_permille),
        _ => None,
    })
    .unwrap_or(INSPECTION_SUSPICION_SCALE)
}

/// 被动②消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里取
/// [`RuleModifier::InspectionConcealment`] 的千分比；一条也没有时返回
/// `0`（藏不住任何东西，等价于「没有这条被动」）。
///
/// 消费点是 `crate::resolve::resolve_inspect`，见其文档「藏匿判定」
/// 一节。tie-break 同 [`inspection_suspicion_permille`]，但**方向相反**：
/// 这是一个概率，**越大越强**（`1000` = 什么都查不出来）——正是模块
/// 文档「『强』的方向为什么必须逐变体声明」一节点名的那个反例，方向
/// 声明在 [`strength_key`]。
///
/// # 为什么缺省是 `0` 而不是某个刻度
///
/// 与抗性/盘查意愿两个**乘数**不同：这是一个**概率**，「没有这条
/// 被动」的自然表达就是「概率为零」，不需要一把「无效果」刻度尺。
pub fn inspection_concealment_permille(modifiers: &[RuleModifierEntry]) -> i32 {
    strongest_by_origin(modifiers, |modifier| match modifier {
        RuleModifier::InspectionConcealment { conceal_permille } => Some(*conceal_permille),
        _ => None,
    })
    .unwrap_or(0)
}

/// [`sneak_attack_rule`] 的返回值——一次偷袭判定需要的两个数：幸运
/// 敏感度（换算触发率）与触发后追加的固定伤害。两个数打包成一个小
/// 结构体而不是元组，理由同 `crate::formula::FormulaInputs` 之类既有
/// 惯例：调用点按字段名读取，不必记住元组位置的含义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SneakAttackRule {
    /// 每点有效幸运贡献的触发率加成，千分比。
    pub luck_chance_permille_per_point: i32,
    /// 触发后追加的固定伤害。
    pub extra_damage: i32,
}

/// 偷袭消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里找
/// [`RuleModifier::SneakAttack`]；一条也没有时返回 `None`（调用方
/// `crate::resolve::resolve_attack` 完全不进入偷袭判定分支，不额外
/// 消费一条 `DetRng` 流，见其文档「偷袭接线」一节）。
///
/// # 多条命中时取哪一条：与 [`resistance_multiplier_permille`] 同一条
/// tie-break 纪律
///
/// 取最强的一条、同强度才按 `origin`（[`ContentIndex`]）升序，**不叠加**
/// 多条偷袭声明的伤害/概率——理由同 [`resistance_multiplier_permille`]
/// 文档「多条命中时取哪一条」一节：多条各自贡献一次判定会让「偷袭」
/// 变成可以无限堆叠的加法游戏，不是设计意图；哪条生效必须是与切片顺序
/// 无关的确定性规则（约束 C5）。
///
/// 偷袭的「强」是**两个数都越大越强**（追加伤害作主键，幸运敏感度作
/// 第二级）——它是本枚举唯一携带两个数值字段的变体，取舍见
/// [`strength_key`] 文档「偷袭那两个字段」一节。
pub fn sneak_attack_rule(modifiers: &[RuleModifierEntry]) -> Option<SneakAttackRule> {
    strongest_by_origin(modifiers, |modifier| match modifier {
        RuleModifier::SneakAttack {
            luck_chance_permille_per_point,
            extra_damage,
        } => Some(SneakAttackRule {
            luck_chance_permille_per_point: *luck_chance_permille_per_point,
            extra_damage: *extra_damage,
        }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemRule, SlotMask};
    use crate::traits::{NoTraitGrants, NoTraits, TraitGrant, TraitRule};
    use ll_core::ident::{Interner, NamespacedId};

    /// 测试用帮手：intern 一个索引——聚合逻辑只关心索引之间"相不相等"
    /// 与它们的大小顺序，不关心具体指向哪条标识符，理由同
    /// `crate::traits` 测试模块同名帮手。
    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    /// 测试用天赋授予来源：固定返回构造时传入的列表，理由同
    /// `crate::traits` 测试模块的同名帮手。
    struct FixedGrants(Vec<TraitGrant>);
    impl TraitGrantSource for FixedGrants {
        fn granted_traits(&self, _owner: ContentIndex) -> Vec<TraitGrant> {
            self.0.clone()
        }
    }

    /// 测试用天赋定义来源：固定的 `trait_id -> TraitRule` 映射。
    struct FixedTraits(Vec<(ContentIndex, TraitRule)>);
    impl TraitCatalog for FixedTraits {
        fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
            self.0
                .iter()
                .find(|(id, _)| *id == trait_id)
                .map(|(_, rule)| rule.clone())
        }
    }

    /// 测试用物品目录：固定的 `def -> rule_modifiers` 映射，其余字段取
    /// 对本模块无影响的占位值（聚合只读 `rule_modifiers` 一个字段）。
    struct FixedItems(Vec<(ContentIndex, Vec<RuleModifier>)>);
    impl ItemCatalog for FixedItems {
        fn item(&self, item: ContentIndex) -> Option<ItemRule> {
            self.0
                .iter()
                .find(|(id, _)| *id == item)
                .map(|(_, modifiers)| ItemRule {
                    wear_channels: crate::item::WearChannels::NONE,
                    taught_recipes: Vec::new(),
                    stack_limit: 1,
                    equip_mask: SlotMask::EMPTY,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: crate::combat::Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: modifiers.clone(),
                })
        }
    }

    fn resistance(damage_category: ContentIndex, multiplier_permille: i32) -> RuleModifier {
        RuleModifier::Resistance {
            damage_category,
            multiplier_permille,
        }
    }

    #[test]
    fn 没有任何来源时乘数恒为无抗性刻度() {
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");

        // Act
        let multiplier = resistance_multiplier_permille(&[], fire);

        // Assert
        assert_eq!(multiplier, RESISTANCE_MULTIPLIER_SCALE);
    }

    #[test]
    fn 天赋这一路声明的抗性被收集进候选列表并命中匹配类别() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:dwarf");
        let trait_id = index(&mut interner, "lostland:fire_hide");
        let fire = index(&mut interner, "lostland:fire");
        let grants = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }]);
        let traits = FixedTraits(vec![(
            trait_id,
            TraitRule {
                rule_modifiers: vec![resistance(fire, 500)],
                ..TraitRule::default()
            },
        )]);

        // Act
        let entries = trait_rule_modifiers(&[TraitSource::new(race, &grants)], 1, &traits);

        // Assert
        assert_eq!(
            entries,
            vec![RuleModifierEntry {
                origin: trait_id,
                modifier: resistance(fire, 500),
            }]
        );
        assert_eq!(resistance_multiplier_permille(&entries, fire), 500);
    }

    #[test]
    fn 抗性只对匹配的伤害类别生效不对其它类别生效() {
        // Arrange
        let mut interner = Interner::new();
        let origin = index(&mut interner, "lostland:fire_hide");
        let fire = index(&mut interner, "lostland:fire");
        let cold = index(&mut interner, "lostland:cold");
        let entries = vec![RuleModifierEntry {
            origin,
            modifier: resistance(fire, 500),
        }];

        // Act & Assert
        assert_eq!(resistance_multiplier_permille(&entries, fire), 500);
        assert_eq!(
            resistance_multiplier_permille(&entries, cold),
            RESISTANCE_MULTIPLIER_SCALE
        );
    }

    #[test]
    fn 装备这一路声明的抗性被收集进候选列表() {
        // Arrange
        let mut interner = Interner::new();
        let amulet = index(&mut interner, "lostland:ward_amulet");
        let fire = index(&mut interner, "lostland:fire");
        let items = FixedItems(vec![(amulet, vec![resistance(fire, 250)])]);
        let equipment = BTreeMap::from([(EquipSlot::NECK, ItemStack::new(amulet, 1))]);

        // Act
        let entries = equipment_rule_modifiers(&equipment, &items);

        // Assert
        assert_eq!(
            entries,
            vec![RuleModifierEntry {
                origin: amulet,
                modifier: resistance(fire, 250),
            }]
        );
        assert_eq!(resistance_multiplier_permille(&entries, fire), 250);
    }

    #[test]
    fn 耐久归零的装备不贡献任何规则修正() {
        // Arrange
        let mut interner = Interner::new();
        let amulet = index(&mut interner, "lostland:ward_amulet");
        let fire = index(&mut interner, "lostland:fire");
        let items = FixedItems(vec![(amulet, vec![resistance(fire, 250)])]);
        let broken = BTreeMap::from([(EquipSlot::NECK, ItemStack::with_durability(amulet, 1, 0))]);
        let intact = BTreeMap::from([(EquipSlot::NECK, ItemStack::with_durability(amulet, 1, 1))]);

        // Act
        let broken_entries = equipment_rule_modifiers(&broken, &items);
        let intact_entries = equipment_rule_modifiers(&intact, &items);

        // Assert：这条判定不是恒真——耐久为正的同一件护符照常生效。
        assert!(broken_entries.is_empty());
        assert_eq!(intact_entries.len(), 1);
    }

    #[test]
    fn 查不到定义的装备与天赋都被跳过() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:human");
        let unknown_item = index(&mut interner, "lostland:mystery");
        let fire = index(&mut interner, "lostland:fire");
        let equipment = BTreeMap::from([(EquipSlot::NECK, ItemStack::new(unknown_item, 1))]);

        // Act
        let from_items = equipment_rule_modifiers(&equipment, &crate::item::NoItems);
        let from_traits =
            trait_rule_modifiers(&[TraitSource::new(race, &NoTraitGrants)], 1, &NoTraits);

        // Assert
        assert!(from_items.is_empty());
        assert!(from_traits.is_empty());
        assert_eq!(
            resistance_multiplier_permille(&from_items, fire),
            RESISTANCE_MULTIPLIER_SCALE
        );
    }

    #[test]
    fn 多条命中时只挑一条而不是取乘积() {
        // Arrange：500‰ 与 800‰ 若取乘积会得到 400‰，若相加会得到
        // 1300‰，两者都不是本项目的规则（`trait-system.md` 三节③
        // 「不取乘积」那半句——「取最强」裁定改的是「挑哪一条」，不是
        // 「挑几条」，这条钉的正是没被改动的那半句）。这里两级判据恰好
        // 同向（500‰ 既是最强的一条，origin 也更小），方向本身由
        // `三个消费者各自的强弱方向互不相同` 单独钉。
        let mut interner = Interner::new();
        let low = index(&mut interner, "lostland:aaa_low");
        let high = index(&mut interner, "lostland:zzz_high");
        let fire = index(&mut interner, "lostland:fire");
        assert!(low < high, "intern 顺序决定索引大小，low 必须更小");
        // 刻意把 high 放在切片前面：结果必须与切片顺序无关（约束 C5）。
        let entries = vec![
            RuleModifierEntry {
                origin: high,
                modifier: resistance(fire, 800),
            },
            RuleModifierEntry {
                origin: low,
                modifier: resistance(fire, 500),
            },
        ];

        // Act
        let multiplier = resistance_multiplier_permille(&entries, fire);

        // Assert
        assert_eq!(multiplier, 500);
    }

    #[test]
    fn 天赋与装备两路来源拼在一起时取最强的一条跨来源生效() {
        // Arrange：天赋那一条先 intern（origin 更小）、但**更弱**
        //（900‰ 只减一成伤），装备那一条 origin 更大、却更强（500‰
        // 半伤）。旧规则（按 origin 升序）会选天赋那条弱的；本次裁定
        // 之后应当选装备那条强的——这正是所有者要修的形态在两个真实
        // 收集器之间的端到端版本。
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:ooze");
        let trait_id = index(&mut interner, "lostland:acid_hide");
        let amulet = index(&mut interner, "lostland:ward_amulet");
        let acid = index(&mut interner, "lostland:acid");
        let grants = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }]);
        let traits = FixedTraits(vec![(
            trait_id,
            TraitRule {
                rule_modifiers: vec![resistance(acid, 900)],
                ..TraitRule::default()
            },
        )]);
        let items = FixedItems(vec![(amulet, vec![resistance(acid, 500)])]);
        let equipment = BTreeMap::from([(EquipSlot::NECK, ItemStack::new(amulet, 1))]);

        // Act：与 `agent_rule_modifiers` 内部完全相同的拼法，只是这里不
        // 需要造一个 `Agent`（构造成本见 `crate::traits::effective_traits`
        // 文档「为什么参数是 `&[TraitSource]`」一节）。
        let mut entries = trait_rule_modifiers(&[TraitSource::new(race, &grants)], 1, &traits);
        entries.extend(equipment_rule_modifiers(&equipment, &items));

        // Assert：两路来源都进了候选列表，tie-break 跨来源生效——装备
        // 那条更强（500‰ < 900‰，抗性越小越强），尽管它 origin 更大。
        assert_eq!(entries.len(), 2);
        assert_eq!(resistance_multiplier_permille(&entries, acid), 500);
    }

    #[test]
    fn 第三路来源不改任何消费者签名就能接进聚合结果() {
        // 这条测试把模块文档「接第三、第四路来源」那句主张钉成可执行的
        // 断言：模拟一路本批次**尚未实现**的来源（药品/限时 buff——它
        // 未来会产出的同样是一串 `RuleModifierEntry`），直接拼进切片，
        // `resistance_multiplier_permille` 的签名与调用写法一个字符都
        // 不用改，就把这一路纳入了 tie-break。
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:human");
        let potion = index(&mut interner, "lostland:acid_ward_potion");
        let trait_id = index(&mut interner, "lostland:zzz_late_trait");
        let acid = index(&mut interner, "lostland:acid");
        let grants = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }]);
        let traits = FixedTraits(vec![(
            trait_id,
            TraitRule {
                rule_modifiers: vec![resistance(acid, 900)],
                ..TraitRule::default()
            },
        )]);
        let mut entries = trait_rule_modifiers(&[TraitSource::new(race, &grants)], 1, &traits);
        assert_eq!(resistance_multiplier_permille(&entries, acid), 900);

        // Act：第三路来源——本批次没有任何代码会产出它，测试直接构造。
        entries.push(RuleModifierEntry {
            origin: potion,
            modifier: resistance(acid, 200),
        });

        // Assert：药品那条 origin 更小（先 intern），按同一条 tie-break
        // 胜出——消费者完全不知道多了一路来源。
        assert!(potion < trait_id);
        assert_eq!(resistance_multiplier_permille(&entries, acid), 200);
    }

    #[test]
    fn 没有偷袭声明时聚合结果为none() {
        // Act & Assert
        assert_eq!(sneak_attack_rule(&[]), None);
    }

    #[test]
    fn 装备声明的偷袭与天赋声明的偷袭走同一个聚合点() {
        // Arrange：偷袭这一路的脚本注册入口目前只开放给天赋
        // （`register-trait-sneak-attack`），但聚合点对五个变体一视同仁
        // ——本测试在 Rust 层证明装备声明的偷袭同样能被消费，说明
        // `equipment_rule_modifiers` 不是"只给抗性开的特例通道"。
        let mut interner = Interner::new();
        let dagger = index(&mut interner, "lostland:backstab_dagger");
        let items = FixedItems(vec![(
            dagger,
            vec![RuleModifier::SneakAttack {
                luck_chance_permille_per_point: 12,
                extra_damage: 7,
            }],
        )]);
        let equipment = BTreeMap::from([(EquipSlot::MAIN_HAND, ItemStack::new(dagger, 1))]);

        // Act
        let entries = equipment_rule_modifiers(&equipment, &items);

        // Assert
        assert_eq!(
            sneak_attack_rule(&entries),
            Some(SneakAttackRule {
                luck_chance_permille_per_point: 12,
                extra_damage: 7,
            })
        );
    }

    #[test]
    fn 多条偷袭命中时取最强的一条而不是叠加() {
        // Arrange：origin 更大的那条追加伤害更高（999 > 5），本次裁定
        // 之后应当由它胜出；无论如何都不该是两者相加（1004）。
        let mut interner = Interner::new();
        let low = index(&mut interner, "lostland:aaa_low");
        let high = index(&mut interner, "lostland:zzz_high");
        let entries = vec![
            RuleModifierEntry {
                origin: high,
                modifier: RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: 999,
                    extra_damage: 999,
                },
            },
            RuleModifierEntry {
                origin: low,
                modifier: RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: 10,
                    extra_damage: 5,
                },
            },
        ];

        // Act
        let rule = sneak_attack_rule(&entries);

        // Assert：high 那条（更强），不是 low（origin 在先的那条），
        // 也不是两者相加。
        assert_eq!(
            rule,
            Some(SneakAttackRule {
                luck_chance_permille_per_point: 999,
                extra_damage: 999,
            })
        );
    }

    #[test]
    fn 两个消费者各自只认自己那一个变体互不误判() {
        // Arrange：一条偷袭声明与一条抗性声明混在同一个候选列表里。
        // 刻意不用 `RerollOnce`/`Advantage` 这类"当前无任何消费者"的
        // 变体来做这条反例——本文件属于 `scripts/ci/check_field_consumers.py`
        // 的决策层扫描范围，在这里写出那些变体名会让门禁把它们误判成
        // "已接线"，见该脚本「已知局限」第 2 条。
        let mut interner = Interner::new();
        let charm = index(&mut interner, "lostland:lucky_charm");
        let dagger = index(&mut interner, "lostland:backstab_dagger");
        let fire = index(&mut interner, "lostland:fire");
        let cold = index(&mut interner, "lostland:cold");
        let entries = vec![
            RuleModifierEntry {
                origin: charm,
                modifier: resistance(fire, 400),
            },
            RuleModifierEntry {
                origin: dagger,
                modifier: RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: 3,
                    extra_damage: 4,
                },
            },
        ];

        // Act & Assert：抗性消费者只看抗性那条（且只在类别匹配时），
        // 偷袭消费者只看偷袭那条。
        assert_eq!(resistance_multiplier_permille(&entries, fire), 400);
        assert_eq!(
            resistance_multiplier_permille(&entries, cold),
            RESISTANCE_MULTIPLIER_SCALE
        );
        assert_eq!(
            sneak_attack_rule(&entries),
            Some(SneakAttackRule {
                luck_chance_permille_per_point: 3,
                extra_damage: 4,
            })
        );
    }

    #[test]
    fn 没有任何来源声明时两个盘查消费者各自返回自己的缺省值() {
        // 缺省值刻意不同：意愿是**乘数**（1000 = 与常人无异），藏匿是
        // **概率**（0 = 藏不住任何东西），见两个消费者各自的文档。
        // Act & Assert
        assert_eq!(
            inspection_suspicion_permille(&[]),
            INSPECTION_SUSPICION_SCALE
        );
        assert_eq!(inspection_concealment_permille(&[]), 0);
    }

    #[test]
    fn 两个盘查规则修正各自被对应的消费者取到() {
        // Arrange
        let mut interner = Interner::new();
        let training = index(&mut interner, "lostland:cutpurse_training");
        let entries = vec![
            RuleModifierEntry {
                origin: training,
                modifier: RuleModifier::InspectionSuspicion {
                    multiplier_permille: 200,
                },
            },
            RuleModifierEntry {
                origin: training,
                modifier: RuleModifier::InspectionConcealment {
                    conceal_permille: 800,
                },
            },
        ];

        // Act & Assert：同一条天赋上的两个被动互不干扰——这正是所有者
        // 「被动可以分为 2 种」那句裁定在聚合层的形状。
        assert_eq!(inspection_suspicion_permille(&entries), 200);
        assert_eq!(inspection_concealment_permille(&entries), 800);
    }

    #[test]
    fn 两个盘查消费者不误判彼此的变体() {
        // 反例：只声明其中一个，另一个必须落回缺省值——证明上一条不是
        // 「随便有条规则修正就返回它的数」。
        // Arrange
        let mut interner = Interner::new();
        let origin = index(&mut interner, "lostland:only_suspicion");
        let entries = vec![RuleModifierEntry {
            origin,
            modifier: RuleModifier::InspectionSuspicion {
                multiplier_permille: 0,
            },
        }];

        // Act & Assert
        assert_eq!(inspection_suspicion_permille(&entries), 0);
        assert_eq!(inspection_concealment_permille(&entries), 0);
    }

    #[test]
    fn 同强度的多条盘查声明按origin升序取第一条而不是取乘积也不依赖切片顺序() {
        // 这条钉的是 `strongest_by_origin` 那份被四个消费者共用的
        // tie-break 的**第二级**：两条 500‰ 的「不觉得可疑」强度完全
        // 相同，此时退回 origin 升序；结果仍然是 500‰ 而不是 250‰
        //（那正是 trait-system.md 三节③对「免疫两次」的原始论证——
        // 「取最强」裁定改的是「挑哪一条」，不是「挑几条」），且哪条
        // 胜出与调用方按什么顺序拼切片无关（约束 C5）。
        // Arrange
        let mut interner = Interner::new();
        let early = index(&mut interner, "lostland:aaa_early");
        let late = index(&mut interner, "lostland:zzz_late");
        assert!(early < late);
        let early_entry = RuleModifierEntry {
            origin: early,
            modifier: RuleModifier::InspectionSuspicion {
                multiplier_permille: 500,
            },
        };
        let late_entry = RuleModifierEntry {
            origin: late,
            modifier: RuleModifier::InspectionSuspicion {
                multiplier_permille: 500,
            },
        };

        // Act：同样两条，两种拼接顺序。
        let forward = inspection_suspicion_permille(&[early_entry.clone(), late_entry.clone()]);
        let backward = inspection_suspicion_permille(&[late_entry, early_entry]);

        // Assert：都是 500（不是 250，也不是 1000），且两种顺序一致。
        assert_eq!(forward, 500);
        assert_eq!(backward, 500);
    }

    /// 测试用帮手：把一条修正包成候选条目，省掉四条测试里逐次写结构体
    /// 字面量的噪音。
    fn entry(origin: ContentIndex, modifier: RuleModifier) -> RuleModifierEntry {
        RuleModifierEntry { origin, modifier }
    }

    #[test]
    fn 抗性取最强的一条而不是注册顺序在先的那条() {
        // 项目所有者裁定的直接落点，也是它要修的那个具体形态：一枚平庸
        // 护符（先被 intern，origin 小）不该压过一条强天赋（后被 intern，
        // origin 大）。抗性「越小越强」：200‰ 比 800‰ 强。
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let trinket = index(&mut interner, "lostland:aaa_mediocre_trinket");
        let talent = index(&mut interner, "lostland:zzz_strong_talent");
        assert!(
            trinket < talent,
            "护符必须先被 intern，才构成本条要修的形态"
        );
        let weak = entry(
            trinket,
            RuleModifier::Resistance {
                damage_category: fire,
                multiplier_permille: 800,
            },
        );
        let strong = entry(
            talent,
            RuleModifier::Resistance {
                damage_category: fire,
                multiplier_permille: 200,
            },
        );

        // Act：两种拼接顺序。
        let forward = resistance_multiplier_permille(&[weak.clone(), strong.clone()], fire);
        let backward = resistance_multiplier_permille(&[strong, weak], fire);

        // Assert：都取 200‰（最强），不是 800‰（origin 在先的那条），
        // 也不是 160‰（两者乘积）。
        assert_eq!(forward, 200);
        assert_eq!(backward, 200);
    }

    #[test]
    fn 三个消费者各自的强弱方向互不相同() {
        // 一个通用的「取最大值」会让抗性/盘查意愿反过来选最弱的那条——
        // 这条把三个方向一次性钉住，是 `strength_key` 逐变体声明方向
        // 这件事的可执行断言。三组都让「较强的那条」origin 在后，
        // 于是旧规则（origin 升序）会给出与新规则相反的答案。
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let first = index(&mut interner, "lostland:aaa_first");
        let second = index(&mut interner, "lostland:zzz_second");
        assert!(first < second);

        // Act & Assert：抗性——越小越强。
        assert_eq!(
            resistance_multiplier_permille(
                &[
                    entry(
                        first,
                        RuleModifier::Resistance {
                            damage_category: fire,
                            multiplier_permille: 900,
                        }
                    ),
                    entry(
                        second,
                        RuleModifier::Resistance {
                            damage_category: fire,
                            multiplier_permille: 100,
                        }
                    ),
                ],
                fire,
            ),
            100,
        );

        // 盘查意愿——同样越小越强。
        assert_eq!(
            inspection_suspicion_permille(&[
                entry(
                    first,
                    RuleModifier::InspectionSuspicion {
                        multiplier_permille: 900,
                    }
                ),
                entry(
                    second,
                    RuleModifier::InspectionSuspicion {
                        multiplier_permille: 100,
                    }
                ),
            ]),
            100,
        );

        // 盘查藏匿——反过来，越大越强。
        assert_eq!(
            inspection_concealment_permille(&[
                entry(
                    first,
                    RuleModifier::InspectionConcealment {
                        conceal_permille: 100,
                    }
                ),
                entry(
                    second,
                    RuleModifier::InspectionConcealment {
                        conceal_permille: 900,
                    }
                ),
            ]),
            900,
        );
    }

    #[test]
    fn 偷袭取追加伤害最大的一条追加伤害相同时再比幸运敏感度() {
        // 偷袭是唯一携带两个数值字段的变体，两级键都要钉：主键
        // extra_damage（所有者点名的那一个），相同时才比
        // luck_chance_permille_per_point。两组都让胜出者 origin 在后。
        // Arrange
        let mut interner = Interner::new();
        let first = index(&mut interner, "lostland:aaa_first");
        let second = index(&mut interner, "lostland:zzz_second");
        assert!(first < second);
        let low_damage_high_luck = entry(
            first,
            RuleModifier::SneakAttack {
                luck_chance_permille_per_point: 90,
                extra_damage: 3,
            },
        );
        let high_damage_low_luck = entry(
            second,
            RuleModifier::SneakAttack {
                luck_chance_permille_per_point: 10,
                extra_damage: 7,
            },
        );

        // Act & Assert（主键）：追加伤害更大的那条胜出，即便它幸运
        // 敏感度更低、origin 更大。
        assert_eq!(
            sneak_attack_rule(&[low_damage_high_luck, high_damage_low_luck]),
            Some(SneakAttackRule {
                luck_chance_permille_per_point: 10,
                extra_damage: 7,
            }),
        );

        // Act & Assert（第二级）：追加伤害相同，改由幸运敏感度决胜——
        // 这一档正是所有者那句「追加伤害越大越强」没有覆盖、原本会掉进
        // 「谁先被 intern 谁赢」的区间。
        assert_eq!(
            sneak_attack_rule(&[
                entry(
                    first,
                    RuleModifier::SneakAttack {
                        luck_chance_permille_per_point: 10,
                        extra_damage: 5,
                    }
                ),
                entry(
                    second,
                    RuleModifier::SneakAttack {
                        luck_chance_permille_per_point: 40,
                        extra_damage: 5,
                    }
                ),
            ]),
            Some(SneakAttackRule {
                luck_chance_permille_per_point: 40,
                extra_damage: 5,
            }),
        );
    }

    #[test]
    fn 强度完全相同时退回origin升序且与切片顺序无关() {
        // 保住 C5 的那一条：这里直接对 `strongest_by_origin` 下断言，
        // 而不是经某个公开消费者——四个真实消费者的投影值都由强度键
        // 完全决定，一旦强度相同，返回值也必然相同，「哪一条胜出」在
        // 它们身上根本不可观察。探针挑的是 `strength_key` **不看**的那
        // 个字段：抗性的强度只由 multiplier_permille 决定，damage_category
        // 完全不参与——于是两条乘数相同、类别不同的抗性强度键恒等，
        // 投影出类别就能读出胜者是谁。
        // Arrange
        let mut interner = Interner::new();
        let early = index(&mut interner, "lostland:aaa_early");
        let late = index(&mut interner, "lostland:zzz_late");
        let fire = index(&mut interner, "lostland:fire");
        let cold = index(&mut interner, "lostland:cold");
        assert!(early < late);
        let early_entry = entry(
            early,
            RuleModifier::Resistance {
                damage_category: fire,
                multiplier_permille: 500,
            },
        );
        let late_entry = entry(
            late,
            RuleModifier::Resistance {
                damage_category: cold,
                multiplier_permille: 500,
            },
        );
        let probe = |modifier: &RuleModifier| match modifier {
            RuleModifier::Resistance {
                damage_category, ..
            } => Some(*damage_category),
            _ => None,
        };

        // Act：同样两条，两种拼接顺序。
        let forward = strongest_by_origin(&[early_entry.clone(), late_entry.clone()], probe);
        let backward = strongest_by_origin(&[late_entry, early_entry], probe);

        // Assert：两种顺序都取 origin 小的那条（火抗那一条）。
        assert_eq!(forward, Some(fire));
        assert_eq!(backward, Some(fire));
    }

    #[test]
    fn 装备这一路也能声明盘查藏匿并被同一个消费者取到() {
        // 「被动②只可能来自天赋」不是本模块的假设——聚合点对来源一无
        // 所知，一件贼帽同样能声明它。这条与
        // `装备这一路声明的抗性被收集进候选列表` 是同一条主张在新变体
        // 上的复用。
        // Arrange
        let mut interner = Interner::new();
        let cloak = index(&mut interner, "lostland:smugglers_cloak");
        let items = FixedItems(vec![(
            cloak,
            vec![RuleModifier::InspectionConcealment {
                conceal_permille: 300,
            }],
        )]);
        let equipment = BTreeMap::from([(EquipSlot::OUTER, ItemStack::new(cloak, 1))]);

        // Act
        let entries = equipment_rule_modifiers(&equipment, &items);

        // Assert
        assert_eq!(inspection_concealment_permille(&entries), 300);
    }
}
