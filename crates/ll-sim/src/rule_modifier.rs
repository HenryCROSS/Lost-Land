//! 规则修正（[`RuleModifier`]）的**多来源聚合点**——「一个实体此刻身上
//! 有哪些规则修正」这个问题的唯一答案处，以及在其之上的四个消费者
//! （抗性 [`resistance_damage_reduction`]、偷袭 [`sneak_attack_rule`]、
//! 盘查意愿 [`inspection_suspicion_reduction_permille`]、盘查藏匿
//! [`inspection_concealment_permille`]），连同它们共用的那一条 tie-break
//! （[`merged_across_types`]）。
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
//! - **消费者**（[`resistance_damage_reduction`]/[`sneak_attack_rule`]/
//!   [`inspection_suspicion_reduction_permille`]/[`inspection_concealment_permille`]）
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
//!    不得依赖切片顺序，见 [`merged_across_types`] 文档「约束 C5」一节）。
//!
//! 被这条裁定取代的是**判据的第一级**，不是「不取乘积」那条论证——
//! `trait-system.md` 三节③原文「不取乘积，理由是『免疫 500‰ 又免疫
//! 一次』不应该变成 25% 而不是 0%」照旧成立。
//!
//! # 加值类型：同类型取最强，不同类型相加
//!
//! 上面那条 tie-break 是**桶内**的规则。项目所有者随后裁定引入 D&D
//! 3.5e / 开拓者的**加值类型**模型，在它外面再包一层：
//!
//! ```text
//! 同一类型 → 取最强，不叠加      ← 上面那两级，一个字没改
//! 不同类型 → 相加
//! ```
//!
//! 「类型」是一张**开放注册表**（`ll_mod::modifier_type`，与
//! `damage_category`/`weapon_category` 同一套手法），不是写死的枚举，
//! 因此 mod 可以声明自己的加值类型；引用一个没注册过的类型在装载期
//! 当场报错，与 `register-recipe` 拒绝未注册类别是同一条先例。
//!
//! **不声明类型的全部落进同一个共享桶**（[`TypedRuleModifier::modifier_type`]
//! 是 `Option`，未声明即 `None`）——这条默认值是刻意的：3.5e 里「无类型」
//! 加值永远叠加，照搬会把本体与 `example_mod` 现有的每一条声明静默地
//! 从「取最强」改成「全部叠加」。分桶层因此在一份没有任何类型声明的
//! 内容上退化成恒等变换，见 [`merged_across_types`] 文档。
//!
//! # 为什么跨类型是相加，而不是相乘
//!
//! 因为**全部规则修正的量都改成了整数点数**（同一次裁定）：抗性从
//! 千分比乘数改成减伤点数、盘查意愿从乘数改成概率减点数。相加因此
//! 是唯一自然的合并方式，而且是净收益——不引入整数除法（没有舍入
//! 方向要裁定），加法可交换可结合（跨桶遍历顺序在数学上就无关，
//! 而带截断的整数乘法不满足结合律，正是约束 C5 要防的那种东西）。
//! 完整论证见 [`CrossTypeMerge::Add`]。
//!
//! **分界线**：会叠加的**规则修正**用整数点数；按比例缩放的**环境量**
//! 保持千分比，一个字都不动——天气/季节的光照系数（`ll_world::light`）、
//! 视野缩放（`WeatherDef::sight_scale`）、潜行移动开销倍率
//! （`STEALTH_MOVE_COST_PERMILLE`）、经验的等级差系数全部不在本次改型
//! 范围内。理由是它们缩放的**基数本身在变**，改成固定加减会在极值处
//! 结构性坏掉：冬季 `750‰ × 午夜光照 100 = 75` 是对的，冬季「−250」
//! `+ 午夜光照 100 = −150` 是一个负光照。`base_weight`/`base_price` 的
//! `Milli` 同理不动——那是「毫」这个单位，不是百分比。
//!
//! # 「强」的方向为什么必须逐变体声明，不能写一个通用的「取最大值」
//!
//! 强弱方向是**变体自己的属性**，不是调用点的参数。改成整数点数之后
//! 四个变体恰好都是「越大越强」（减伤点数、概率减点数、藏匿概率、
//! 追加伤害），但这是这一版模型的**结果**，不是可以依赖的前提——乘数
//! 模型下抗性与盘查意愿都是「越小越强」，一个通用的「取最大值」会让
//! 它们反过来选**最弱**的那一条。真正的风险形态是「哪天有人加第五个
//! 变体、忘了传对比较器」。
//!
//! 因此方向不由调用点携带，而是集中在 [`strength_key`] 一个**无通配
//! 分支的穷尽 `match`** 里逐变体声明：新增一个变体而不声明它的方向，
//! `cargo build` 直接不过。[`cross_type_merge`] 用完全相同的手法声明
//! 跨类型的合并方式。这与 `ll_mod::content_hash` 的
//! `ContentTableKind`/`classify_index`「编译期强制穷尽」是同一条手法
//! （见该模块文档「编译期强制：穷尽解构 tables」一节），只是这次强制
//! 的对象是「比较方向」与「合并方式」而不是「哈希覆盖面」。
//!
//! # 热路径（ADR 0016/0017）
//!
//! 与 `crate::traits` 模块文档「为什么不缓存」一节同一档：本模块的
//! 聚合每次要用时现算，调用频率是「每次攻击结算一次」，不是逐格/逐帧。
//! 全程纯 Rust，不跨脚本边界（ADR 0016 一档：抗性是静态声明）。
//! 全程整数（ADR 0020 乙区），不引入任何 `f32` 中间值——规则修正的量
//! 是点数，聚合只有取最大值与加法两种运算，连整数除法都没有。

use std::collections::{BTreeMap, btree_map};

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_world::entity::Agent;

use crate::item::{EquipSlot, ItemCatalog, ItemStack};
use crate::traits::{
    TraitCatalog, TraitGrantSource, TraitSource, agent_trait_sources, effective_traits,
};

/// 抗性减伤算完之后，一次**本来打得出伤害**的攻击至少还剩下的伤害
/// 点数——项目所有者「不允许绝对」这条裁定的落点。
///
/// # 减伤不封顶，但结果保底
///
/// 减伤（DR）本身不需要上限：它减不到负数，伤害越大被减掉的比例越小,
/// 大伤害自然穿透（见 [`RuleModifier::Resistance`] 文档「对小伤害强、
/// 对大伤害弱」一节）。需要的是另一头——**不允许出现「完全打不动」
/// 这个终局**，因此减完保底 1 点。
///
/// # 这条下限是新增的，不是把 10% 下限平移过来
///
/// 必须如实说清楚，否则下一个人会以为这里只是换了个写法：
/// `damage-formula-mod-api.md` 二十节「与 10% 下限的关系」一节在**乘数
/// 模型下**的结论恰恰相反——原文「免疫（乘数 = 0）会合法地把步骤 3 的
/// 结果打成 0，即使步骤 2 的减后伤害满足了 10% 下限」，也就是说
/// `crate::combat::damage_after_defense` 内部那条 10% 下限**从来管不到
/// 抗性这一步**，抗性可以合法归零。本常量因此不是那条下限的延续，是
/// 一条**此前不存在的新下限**，并且**推翻**了二十节那段「免疫理应打出
/// 0」的论证。更正段写在该文档二十节末尾与 `trait-system.md` 三节③。
///
/// 两条下限至今仍然不是同一条，各自独立生效：10% 下限保护「减伤链路
/// 本身」（打不打得穿盔甲），本条保护「抗性这一步」（这种伤害对这个
/// 目标还有没有意义）。二十节「两者不冲突，因为它们从来不覆盖同一个
/// 问题」那半句原样成立，变的只是后一条问题的答案。
pub const MINIMUM_DAMAGE_AFTER_RESISTANCE: i32 = 1;

/// 概率类规则修正在两端各自留出的余量（千分比）——项目所有者「两端
/// 各留一线，永远不会必定成功也永远不会必定失败」这条裁定的落点。
///
/// # 为什么是 1‰，不是更大的数
///
/// 三条理由，都指向同一个最小值：
///
/// 1. **它是这把尺子上最小的非零值**。「留一线」字面上就是留一线,
///    多留一分都是在替内容作者决定「你最多只能藏到 99%」，而所有者
///    的裁定只要求排除**必定**，没有要求压低上限。
/// 2. **它不扰动任何一条合法声明**。落在 `1..=999` 的声明值原样通过,
///    只有恰好取到 `0` / `1000` 两个绝对端点的才被拨回一格——受影响的
///    正好是、且只是裁定要排除的那两个值。
/// 3. **判定分母本来就是 1000**（`DetRng::chance(n, 1000)`，见
///    `crate::resolve::resolve_inspect` 与
///    `ll_mod::native_behavior::guard_inspect_chance`）。1‰ 是这把尺子
///    能表达的最小非零概率,不需要为了留余量去换一把更细的刻度。
pub const PROBABILITY_MARGIN_PERMILLE: i32 = 1;

/// 把一个千分比概率钳进 `[PROBABILITY_MARGIN_PERMILLE, 1000 −
/// PROBABILITY_MARGIN_PERMILLE]`——见 [`PROBABILITY_MARGIN_PERMILLE`]。
///
/// # 「没有这条被动」不走这里
///
/// 本函数只钳**已经有人声明**的概率。「一条也没声明」这件事由各消费者
/// 自己的缺省值表达（[`inspection_concealment_permille`] 返回 `0`,
/// 调用方据此完全跳过判定、一次随机数都不消耗），那个 `0` 不是「概率
/// 为零」而是「这条规则不在场」，因此不该被钳成 1‰。两者的区别见
/// [`inspection_concealment_permille`] 文档「缺省 0 与声明 0」一节。
pub fn clamp_probability_permille(permille: i32) -> i32 {
    permille.clamp(
        PROBABILITY_MARGIN_PERMILLE,
        1000 - PROBABILITY_MARGIN_PERMILLE,
    )
}

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
/// [`resistance_damage_reduction`] 与
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
    /// 抗性：该伤害类别的伤害，在既有减伤链路算完之后**直接减掉一个
    /// 固定点数**（flat damage reduction，D&D 3.5e 的 DR 模型）。挂载
    /// 点仍是 `damage-formula-mod-api.md` 二十节定死的「减伤之后」那一
    /// 步，只是形式从乘数换成了减法，见该节末尾的更正段。
    ///
    /// # 对小伤害强、对大伤害弱——这是选它的理由，不是它的缺陷
    ///
    /// 减伤 5 点：挨 12 点伤害等于减了 42%，挨 50 点只减了 10%。同一条
    /// 声明对不同量级的来伤给出**不同的**减免比例，这正是 DR 比百分比
    /// 抗性更有战术味的地方——重甲挡得住乱刀,挡不住巨龙一口。百分比
    /// 抗性做不到这件事：`500‰` 对 12 点和对 50 点一视同仁地砍一半。
    ///
    /// # 与「跨类型相加」的关系
    ///
    /// 这是本次改型的另一半理由：减伤是**点数**，多个加值类型的减伤
    /// 直接相加（`3 + 2 = 5`），全程没有整数除法，因此既没有舍入方向
    /// 问题，也不存在「先乘谁后乘谁截断差 1」这类顺序依赖（约束 C5
    /// 要防的正是后者）。乘数模型两个问题都有。合并方式逐变体声明在
    /// [`cross_type_merge`]。
    ///
    /// # 负值 = 脆弱
    ///
    /// 减伤点数不禁止负数：`-5` 表示「这类伤害对我多打 5 点」，是乘数
    /// 模型里 `2000‰`（双倍）那一档的表达方式在新模型下的对应物。刻意
    /// 保留它，是因为旧模型能表达脆弱，静默丢掉这个能力会是一次不声明
    /// 的退化。
    Resistance {
        /// 伤害类别，走 `damage-formula-mod-api.md` 十七节的开放
        /// `register-damage-category` 集合。
        damage_category: ContentIndex,
        /// 减伤点数：正数抵挡、负数放大，见本变体文档。减完的保底见
        /// [`MINIMUM_DAMAGE_AFTER_RESISTANCE`]。
        damage_reduction: i32,
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
    /// 意愿**，从那一次判定的触发概率上**直接减掉**一个千分比点数
    /// （`0` = 与常人无异，`400` = 触发概率减 40 个百分点）。
    ///
    /// # 为什么是减点数而不是乘数
    ///
    /// 与 [`RuleModifier::Resistance`] 同一次改型、同一条理由：加法
    /// 可交换可结合，跨加值类型合并因此既不引入整数除法（没有舍入
    /// 方向要裁定），也不依赖合并顺序（约束 C5）。合并方式声明在
    /// [`cross_type_merge`]。
    ///
    /// 代价要如实说明，它与抗性那一条**方向相反**：减伤点数对小伤害
    /// 强、对大伤害弱；概率减点数则是**对低基础概率强、对高基础概率
    /// 弱**——`400` 从 `500‰` 上减掉是砍掉八成，从 `50‰`（潜行中的
    /// 基础盘查率）上减掉则直接触底，被
    /// [`clamp_probability_permille`] 钳在 `1‰`。乘数模型下后者是
    /// `50 × 600 / 1000 = 30‰`。这是模型换代的真实后果，不是 bug。
    ///
    /// # 消费者在 AI 决策侧，不在 `resolve` 侧
    ///
    /// 这是本变体与本枚举其余全部变体的唯一实质差异，也是它必须与
    /// [`RuleModifier::InspectionConcealment`] 分成两个变体、而不是
    /// 合成一个的理由：「要不要发起盘查」这个决策**根本不经过
    /// `resolve`**——它整个发生在 AI 决策阶段
    /// （`ll_mod::native_behavior::guard_inspect_chance` 的那一次
    /// 掷骰），`Intent::Inspect` 一旦产出，
    /// `crate::resolve::resolve_inspect` 恒执行、不重新判断「该不该
    /// 查」（见该函数文档「谁来判断该不该发起这次盘查」一节）。
    ///
    /// 聚合与 tie-break 仍然完全走 [`agent_rule_modifiers`]——行为树
    /// 拿到的是本模块 [`inspection_suspicion_reduction_permille`] 算完
    /// 的**一个数**，不是一份候选列表：多来源取哪一条这件事不下放给
    /// 行为树，理由同本模块文档「跨来源 tie-break」一节。
    InspectionSuspicion {
        /// **从盘查触发概率上直接减掉**的千分比点数（越大越不起眼,
        /// `0` = 与常人无异）。是一个加法量，不是乘数——见本变体文档
        /// 「为什么是减点数而不是乘数」一节。
        suspicion_reduction_permille: i32,
    },
    /// 被动②**「查不出东西」**（盗贼被动两分批次）——所有者裁定里的
    /// 后一种：盘查**照常发起**，只是搜身的人看不到你身上的东西。
    /// `conceal_permille` 是**每一件**物品各自不被看见的千分比概率。
    ///
    /// 它是一个**加法量**：多个加值类型各自的藏匿点数直接相加，再由
    /// [`clamp_probability_permille`] 钳进两端各留一线的区间——`1000`
    /// 因此不是「什么都查不出来」而是「钳到 `999‰`」，绝对成功与绝对
    /// 失败都不可达，见 [`PROBABILITY_MARGIN_PERMILLE`]。`0` 保留
    /// 「没有这条被动」这个特殊含义，见
    /// [`inspection_concealment_permille`] 文档「缺省 0 与声明 0」。
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
    /// 这条修正属于哪个**加值类型**，`None` = 未分类，见
    /// [`TypedRuleModifier::modifier_type`]。
    pub modifier_type: Option<ContentIndex>,
    /// 修正本身。
    pub modifier: RuleModifier,
}

/// 内容表里存的一条规则修正：**修正本身 + 它属于哪个加值类型**。
///
/// # 为什么类型是一个独立的字段，不是塞进 [`RuleModifier`] 的每个变体
///
/// 「这条修正属于哪个加值类型」与「这条修正是什么」是两个正交的问题：
/// 同一条抗性可以是附魔给的，也可以是药水给的；同一个加值类型底下可以
/// 同时有抗性和偷袭。塞进变体要给七个变体各加一个同名字段，而且每加
/// 一个新变体就得记得再加一次——正是 [`strength_key`] 文档「无通配分支」
/// 那一节要避免的那种「靠人记得」。
///
/// # 为什么是 `Option`，不是一个「未分类」的保留 `ContentIndex`
///
/// `None` 表达的是「内容作者没有声明类型」，它不是一个内容作者可以
/// 引用的 id：保留一个真实注册的「未分类」类别，会让内容作者能显式
/// 写出它，于是「没声明」与「声明成未分类」在数据上无法区分，而它们
/// 在语义上本来就是同一件事。用 `Option` 把这条歧义从类型上消掉。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedRuleModifier {
    /// 这条修正属于哪个**加值类型**（`ll_mod::modifier_type` 那张开放
    /// 注册表里的一条），`None` 表示内容作者没有声明。
    ///
    /// # 默认值为什么必须是「一个共享的未分类桶」
    ///
    /// D&D 3.5e 里「无类型」加值是**永远叠加**的。照搬那一条会静默改掉
    /// 本体与 `example_mod` 现有的每一条声明——它们一个字都没改，行为
    /// 却会从「取最强」变成「全部叠加」。项目所有者因此裁定：**不声明
    /// 类型的全部落进同一个桶**，桶内照旧取最强，行为与分桶之前**逐位
    /// 相同**；只有显式声明了类型的才各自分桶、跨桶相加。这条承诺由
    /// 本模块测试 `不声明类型时结算结果与分桶之前逐位相同` 钉住。
    pub modifier_type: Option<ContentIndex>,
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
        for typed in rule.rule_modifiers {
            result.push(RuleModifierEntry {
                origin: trait_id,
                modifier_type: typed.modifier_type,
                modifier: typed.modifier,
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
        for typed in rule.rule_modifiers {
            result.push(RuleModifierEntry {
                origin: stack.def,
                modifier_type: typed.modifier_type,
                modifier: typed.modifier,
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
/// 在这里多 `extend` 一次。四个消费者（[`resistance_damage_reduction`]/
/// [`sneak_attack_rule`]/[`inspection_suspicion_reduction_permille`]/
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
/// `damage_category` 匹配的 [`RuleModifier::Resistance`] 的**减伤点数**；
/// 一条也没命中时返回 `0`（没有抗性，一点也不减）。
///
/// `crate::resolve::resolve_attack`（伤害类别/抗性接线批次）在减伤链路
/// 算完之后调用本函数，拿到的点数按 `damage-formula-mod-api.md` 二十节
/// 挂在「减伤之后」，减完的保底见 [`MINIMUM_DAMAGE_AFTER_RESISTANCE`]。
///
/// # 多条命中时怎么合：同类型取最强，不同类型相加
///
/// 两级，见 [`merged_across_types`]：
///
/// 1. **同一个加值类型**（含「都没声明类型」这个共享桶）内部取最强
///    ——`trait-system.md` 三节③「不取乘积」那半句的现代表述，理由
///    原样成立：「免疫一次又免疫一次」不该变成两倍效果。抗性的「强」
///    是**减伤点数越大越强**，方向在 [`strength_key`] 里逐变体声明。
/// 2. **不同加值类型之间相加**——D&D 3.5e 的加值类型模型，声明在
///    [`cross_type_merge`]。附魔的 3 点减伤 + 炼金的 2 点减伤 = 5 点。
///
/// 判据**不依赖 `modifiers` 切片自身的顺序**（约束 C5）：桶内两级比较
/// 只与声明值和 `ContentIndex` 有关；跨桶是整数加法，可交换可结合,
/// 顺序在数学上就无关（这正是所有者把跨类型合并从相乘改成相加时点名
/// 的收益之一——整数乘法带截断是顺序敏感的）。
pub fn resistance_damage_reduction(
    modifiers: &[RuleModifierEntry],
    damage_category: ContentIndex,
) -> i32 {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::Resistance {
            damage_category: candidate_category,
            damage_reduction,
        } if *candidate_category == damage_category => Some(*damage_reduction),
        _ => None,
    })
    .unwrap_or(0)
}

/// 把一次攻击已经算好的伤害，按 `damage_reduction` 点减伤扣掉，并落实
/// [`MINIMUM_DAMAGE_AFTER_RESISTANCE`] 这条保底。
///
/// # 为什么保底只对「本来就打得出伤害」的那一下生效
///
/// `damage <= 0` 时原样返回：保底的意思是「挡不成绝对免疫」，不是
/// 「凭空造出一点伤害」。一次本来就打不出伤害的攻击（例如攻击力为零
/// 的占位公式）不该因为目标碰巧声明过抗性而反倒开始掉血——那会让本条
/// 保底变成一个隐蔽的伤害来源。
pub fn damage_after_resistance(damage: i32, damage_reduction: i32) -> i32 {
    if damage <= 0 {
        return damage;
    }
    damage
        .saturating_sub(damage_reduction)
        .max(MINIMUM_DAMAGE_AFTER_RESISTANCE)
}

/// 一条规则修正的**强度比较键**——把「哪边算强」这件逐变体不同的事，
/// 规范化成一个统一的「越大越强」的整数键，好让
/// [`merged_across_types`] 只剩「取键最大的一条」这一件事要做。
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
/// 全部规则修正改成整数点数（本批次）之后，比较键不再需要取负——
/// 每个变体的方向都可以直接用「越大越强」表达，`smaller_is_stronger`
/// 因而随乘数模型一起删掉了。`i64` 仍然保留：[`cross_type_merge`]
/// 声明的跨类型合并是**相加**，多个加值类型的点数求和可以越出 `i32`
/// （声明值本身就是 `i32`，注册期不禁止极端值），比较键与合并结果
/// 落在同一个值域上才不会出现「合并完比不了」。全程整数，不引入任何
/// 浮点（ADR 0002/0020）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct StrengthKey(i64, i64);

impl StrengthKey {
    /// 「这个变体没有声明比较方向」——恒等的键，任意两条都判定为同强度，
    /// 于是完全退回 `origin` 升序，也就是本次裁定之前的旧行为。
    ///
    /// 只用于**当前没有任何消费者**的那几个变体（见 [`strength_key`]
    /// 文档「没有消费者的变体」一节）：它们从来不会进入
    /// [`merged_across_types`]（没有 `select` 会认领它们），这个值因此
    /// 不是「随便填的默认」，而是「等真正的消费者落地时，由接线的那一批
    /// 一并裁定方向」的诚实占位。
    const INDISTINGUISHABLE: StrengthKey = StrengthKey(0, 0);

    /// 声明「这个数越大越强」。
    const fn larger_is_stronger(value: i32) -> Self {
        StrengthKey(value as i64, 0)
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
        // 减伤点数，越大越强：挡掉的伤害越多。
        R::Resistance {
            damage_reduction, ..
        } => StrengthKey::larger_is_stronger(*damage_reduction),
        // 概率减点数，越大越强：从盘查触发率上减掉得越多越不起眼。
        R::InspectionSuspicion {
            suspicion_reduction_permille,
        } => StrengthKey::larger_is_stronger(*suspicion_reduction_permille),
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

/// 一个变体在**跨加值类型**之间怎么合并——本模块唯一回答这个问题的
/// 地方，与 [`strength_key`] 回答「同类型里哪边算强」是同一套手法的
/// 另一半。
///
/// 只有两档，不是四档：项目所有者把跨类型合并从「按量的性质分别相乘／
/// 相加／取最强」收敛成了**一律相加**，代价是全部规则修正的量都改成
/// 整数点数（抗性从千分比乘数改成减伤点数、盘查意愿从乘数改成概率
/// 减点数）。收益写在 [`CrossTypeMerge::Add`] 文档里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrossTypeMerge {
    /// **相加**：不同加值类型各自的点数直接求和。
    ///
    /// # 为什么相加是净收益，不只是「更简单」
    ///
    /// 加法可交换、可结合，于是两类问题一起消失：
    ///
    /// - **舍入方向**：没有整数除法就没有截断，不需要为每一条量各自
    ///   裁定一个向上/向下/向零的方向,也不需要为那个方向写守门测试。
    /// - **顺序依赖**：`a + b + c` 与 `c + a + b` 逐位相同。带截断的
    ///   整数乘法**不满足**结合律（`(a×b)/1000×c/1000` 与
    ///   `a×(b×c)/1000/1000` 可以差 1），那正是约束 C5 要防的东西——
    ///   一条结果取决于遍历顺序的规则。
    Add,
    /// **不裁定**：当前没有任何消费者的那几个变体（见 [`strength_key`]
    /// 文档「没有消费者的变体」一节）。
    ///
    /// 它们既没有 `select` 会认领，也就永远走不到跨类型合并这一步；
    /// 这个值因此不是「随便填的默认」，而是「等真正的消费者落地时，
    /// 由接线的那一批连同强弱方向一起裁定」的诚实占位——与
    /// [`StrengthKey::INDISTINGUISHABLE`] 是同一条纪律的两半。
    ///
    /// 落到行为上：不分桶，退回「全体取最强」，也就是本批次之前的
    /// 旧行为。
    Undecided,
}

/// 逐变体声明跨加值类型的合并方式——无通配分支的穷尽 `match`，新增
/// 一个 [`RuleModifier`] 变体而不在这里补一条，`cargo build` 直接不过。
///
/// `R` 别名的理由与 [`strength_key`] 完全相同（不让
/// `scripts/ci/check_field_consumers.py` 把三个死变体误判成「已接线」），
/// 见该函数文档最后一节。
fn cross_type_merge(modifier: &RuleModifier) -> CrossTypeMerge {
    use crate::rule_modifier::RuleModifier as R;
    match modifier {
        // 减伤点数：附魔 3 点 + 炼金 2 点 = 5 点。
        R::Resistance { .. } => CrossTypeMerge::Add,
        // 概率减点数：两个类型各减 100‰ 就是减 200‰，钳制在消费者那一侧。
        R::InspectionSuspicion { .. } => CrossTypeMerge::Add,
        // 逐件藏匿概率：同上，相加后钳进两端各留一线的区间。
        R::InspectionConcealment { .. } => CrossTypeMerge::Add,
        // 追加伤害与幸运敏感度两个字段各自相加，见 `AddAcrossTypes for SneakAttackRule`。
        R::SneakAttack { .. } => CrossTypeMerge::Add,
        // 以下三个当前没有消费者，见 `CrossTypeMerge::Undecided` 文档。
        R::RerollOnce { .. } | R::Advantage { .. } | R::Disadvantage { .. } => {
            CrossTypeMerge::Undecided
        }
    }
}

/// 能跨加值类型相加的投影值——[`cross_type_merge`] 判为
/// [`CrossTypeMerge::Add`] 的变体，其消费者的投影类型必须实现本 trait。
///
/// # 为什么值得一个 trait（ADR 0021 复核）
///
/// 判据是「有没有一份算法要被多种类型共用」，这里有：[`merged_across_types`]
/// 那一段分桶 + 折叠的逻辑对 `i32` 与 [`SneakAttackRule`] 逐字相同,
/// 差别只在「两个值怎么加起来」这一步。把这一步做成 trait 方法，
/// [`cross_type_merge`] 声明的 `Add` 就是**有约束力的**（声明了相加,
/// 却没有一个能相加的实现，编译不过），而不是一句写在文档里、靠调用点
/// 自觉遵守的话。
trait AddAcrossTypes: Sized {
    /// 把另一个加值类型的贡献加进来。饱和运算，理由同
    /// `damage-formula-mod-api.md` 十二节「运行期溢出：饱和运算」——
    /// 点数是内容作者填的值，注册期不禁止极端值。
    fn add_across_types(self, other: Self) -> Self;
}

impl AddAcrossTypes for i32 {
    fn add_across_types(self, other: Self) -> Self {
        self.saturating_add(other)
    }
}

impl AddAcrossTypes for SneakAttackRule {
    /// 两个字段**各自**相加：追加伤害加追加伤害，幸运敏感度加幸运
    /// 敏感度。刻意不相乘、不取其中一个作主——两个字段回答的是不同的
    /// 问题（触发之后打多少 / 多容易触发），没有一个把另一个吸收掉的
    /// 自然方式，见 [`strength_key`] 文档「偷袭那两个字段」一节同一条
    /// 论证的另一面。
    fn add_across_types(self, other: Self) -> Self {
        SneakAttackRule {
            luck_chance_permille_per_point: self
                .luck_chance_permille_per_point
                .saturating_add(other.luck_chance_permille_per_point),
            extra_damage: self.extra_damage.saturating_add(other.extra_damage),
        }
    }
}

/// 全部消费者共用的聚合：**按加值类型分桶 → 桶内取最强 → 跨桶相加**。
///
/// 只看 `select` 认领的那些条目。一条也没有认领时返回 `None`。
///
/// # 两级规则，各自的出处
///
/// 1. **桶内取最强**（`trait-system.md` 三节③「不取乘积」+ 项目所有者
///    「同一类型取最强」的裁定）：强度由 [`strength_key`] 逐变体声明；
///    强度完全相同的多条之间，取 `origin`（[`ContentIndex`]）**最小**
///    的那一条。这一级与本批次之前逐字相同。
/// 2. **跨桶相加**（D&D 3.5e / 开拓者的加值类型模型）：合并方式由
///    [`cross_type_merge`] 逐变体声明。
///
/// # 「没声明类型」是一个桶，不是「每条各成一桶」
///
/// 这是本批次最容易出事、也最要紧的一条默认值，完整论证见
/// [`TypedRuleModifier::modifier_type`] 文档。落到代码上就是桶的键是
/// `Option<ContentIndex>`：全部未声明的条目共享 `None` 这一个键，于是
/// 一份一条类型都没声明的内容（本体与 `example_mod` 现状）只会得到
/// **一个桶**，折叠退化成「取这个桶的最强」——与分桶之前逐位相同。
///
/// # 约束 C5：结果与切片顺序无关
///
/// 桶内两级判据都是严格比较才替换，只与声明值和 `ContentIndex` 有关。
/// 跨桶是整数加法，可交换可结合，顺序在数学上就无关。桶本身还是走
/// `BTreeMap`（按 `Option<ContentIndex>` 升序）——加法虽然不需要这条
/// 保证，但 [`CrossTypeMerge::Undecided`] 那一支要在桶之间再比一次
/// 强弱，而且这里不该留一个「今天恰好不需要确定顺序」的隐患。
///
/// 剩下的唯一可能「谁先出现谁赢」的情形是**同一个桶里强度键与 `origin`
/// 同时相等**——那意味着两条来自同一个内容条目、数值逐字相同、类型也
/// 相同，谁赢在可观察结果上没有任何差别。
fn merged_across_types<T: AddAcrossTypes>(
    modifiers: &[RuleModifierEntry],
    mut select: impl FnMut(&RuleModifier) -> Option<T>,
) -> Option<T> {
    let mut buckets: BTreeMap<Option<ContentIndex>, (StrengthKey, ContentIndex, T)> =
        BTreeMap::new();
    // 同一个 `select` 只认领同一个变体（抗性那一条还额外比对
    // `damage_category`，但仍然只认领 `Resistance`），因此全部被认领的
    // 条目的合并方式必然一致，取第一条的即可。
    let mut merge_rule: Option<CrossTypeMerge> = None;
    for entry in modifiers {
        let Some(value) = select(&entry.modifier) else {
            continue;
        };
        let rule = cross_type_merge(&entry.modifier);
        debug_assert!(
            merge_rule.is_none_or(|previous| previous == rule),
            "同一个 select 只认领一个变体，跨类型合并方式必然一致"
        );
        merge_rule = Some(rule);
        let key = strength_key(&entry.modifier);
        match buckets.entry(entry.modifier_type) {
            btree_map::Entry::Vacant(slot) => {
                slot.insert((key, entry.origin, value));
            }
            btree_map::Entry::Occupied(mut slot) => {
                let (best_key, best_origin, _) = slot.get();
                if key > *best_key || (key == *best_key && entry.origin < *best_origin) {
                    slot.insert((key, entry.origin, value));
                }
            }
        }
    }

    let rule = merge_rule?;
    let mut winners = buckets.into_values();
    let first = winners.next().expect("merge_rule 有值即至少有一个桶");
    match rule {
        CrossTypeMerge::Add => Some(winners.fold(first.2, |accumulated, (_, _, value)| {
            accumulated.add_across_types(value)
        })),
        // 不裁定：不合并，退回「全体取最强」——与分桶之前逐位相同。
        CrossTypeMerge::Undecided => {
            let mut best = first;
            for (key, origin, value) in winners {
                if key > best.0 || (key == best.0 && origin < best.1) {
                    best = (key, origin, value);
                }
            }
            Some(best.2)
        }
    }
}

/// 被动①消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里取
/// [`RuleModifier::InspectionSuspicion`] 的**概率减点数**（千分比）；
/// 一条也没有时返回 `0`（与常人无异，一点也不减）。
///
/// 真正的消费点在 **AI 决策侧**（`ll_mod::native_behavior` 的
/// `guard_inspect_chance`），不是 `crate::resolve`——理由见
/// [`RuleModifier::InspectionSuspicion`] 文档「消费者在 AI 决策侧」
/// 一节。聚合仍然留在这里：行为树拿到的是算完的一个数。
///
/// # 本函数不钳制，钳制在调用点
///
/// 返回的是**要减掉多少**，不是**减完剩多少**——被减的那个基础概率
/// （潜行与否两档，见 `ll_mod::native_behavior::GUARD_INSPECT_CHANCE_PERMILLE`）
/// 本函数看不见。两端各留一线那条裁定因此落在调用点：调用方减完之后
/// 过一遍 [`clamp_probability_permille`]。
///
/// 合并规则同 [`resistance_damage_reduction`]（[`merged_across_types`]）：
/// 同类型取最强、跨类型相加。本变体的「强」是**减得越多越强**，方向
/// 声明在 [`strength_key`]。
pub fn inspection_suspicion_reduction_permille(modifiers: &[RuleModifierEntry]) -> i32 {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::InspectionSuspicion {
            suspicion_reduction_permille,
        } => Some(*suspicion_reduction_permille),
        _ => None,
    })
    .unwrap_or(0)
}

/// 被动②消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里取
/// [`RuleModifier::InspectionConcealment`] 的逐件藏匿千分比概率；
/// 一条也没有时返回 `0`。
///
/// 消费点是 `crate::resolve::resolve_inspect`，见其文档「藏匿判定」
/// 一节。合并规则同 [`resistance_damage_reduction`]：同类型取最强、
/// 跨类型相加；本变体的「强」是**概率越大越强**（藏得越严实）。
///
/// # 缺省 0 与声明 0：两个不同的意思，刻意不合并
///
/// - **一条也没有声明** → 返回 `0`，含义是「**这条被动不在场**」。
///   调用方据此完全跳过藏匿判定，一次随机数都不消耗（约束 C3：不该
///   为一条不存在的规则空转一次确定性随机流）。
/// - **声明了，合并结果落在两端** → 走 [`clamp_probability_permille`]，
///   钳进 `1..=999`。绝对藏住与绝对藏不住都不可达,见
///   [`PROBABILITY_MARGIN_PERMILLE`]。
///
/// 也就是说 `0` 这个返回值只可能来自前一种情形。一个显式声明成 `0`
/// 的内容条目会得到 `1‰`——「你声明了这条被动，只是弱到几乎没用」，
/// 与「你根本没有这条被动」是两回事,后者连骰都不掷。
///
/// 跨类型相加还有一个直接后果：两个类型各 `800‰` 相加是 `1600‰`，
/// 钳成 `999‰`。乘数模型下同样两条会算成 `1 − 0.2 × 0.2 = 960‰`。
/// 加法在高位更容易触顶，这是模型换代的真实后果。
pub fn inspection_concealment_permille(modifiers: &[RuleModifierEntry]) -> i32 {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::InspectionConcealment { conceal_permille } => Some(*conceal_permille),
        _ => None,
    })
    .map(clamp_probability_permille)
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
/// # 多条命中时取哪一条：与 [`resistance_damage_reduction`] 同一条
/// tie-break 纪律
///
/// 取最强的一条、同强度才按 `origin`（[`ContentIndex`]）升序，**不叠加**
/// 多条偷袭声明的伤害/概率——理由同 [`resistance_damage_reduction`]
/// 文档「多条命中时取哪一条」一节：多条各自贡献一次判定会让「偷袭」
/// 变成可以无限堆叠的加法游戏，不是设计意图；哪条生效必须是与切片顺序
/// 无关的确定性规则（约束 C5）。
///
/// 偷袭的「强」是**两个数都越大越强**（追加伤害作主键，幸运敏感度作
/// 第二级）——它是本枚举唯一携带两个数值字段的变体，取舍见
/// [`strength_key`] 文档「偷袭那两个字段」一节。
pub fn sneak_attack_rule(modifiers: &[RuleModifierEntry]) -> Option<SneakAttackRule> {
    merged_across_types(modifiers, |modifier| match modifier {
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
    struct FixedItems(Vec<(ContentIndex, Vec<TypedRuleModifier>)>);
    impl ItemCatalog for FixedItems {
        fn item(&self, item: ContentIndex) -> Option<ItemRule> {
            self.0
                .iter()
                .find(|(id, _)| *id == item)
                .map(|(_, modifiers)| ItemRule {
                    wear_channels: crate::item::WearChannels::NONE,
                    max_durability: None,
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
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

    fn resistance(damage_category: ContentIndex, damage_reduction: i32) -> RuleModifier {
        RuleModifier::Resistance {
            damage_category,
            damage_reduction,
        }
    }

    /// 测试用帮手：把一条修正包成**不声明加值类型**的内容表条目。
    ///
    /// 绝大多数测试都该走这一条：不声明类型是内容作者的默认写法，也是
    /// 「分桶层在现有内容上必须是恒等变换」那条承诺覆盖的情形。
    fn untyped(modifier: RuleModifier) -> TypedRuleModifier {
        TypedRuleModifier {
            modifier_type: None,
            modifier,
        }
    }

    /// 测试用帮手：把一条修正包成**不声明加值类型**的候选条目。
    fn entry(origin: ContentIndex, modifier: RuleModifier) -> RuleModifierEntry {
        RuleModifierEntry {
            origin,
            modifier_type: None,
            modifier,
        }
    }

    /// 测试用帮手：把一条修正包成**声明了加值类型**的候选条目。
    fn typed_entry(
        origin: ContentIndex,
        modifier_type: ContentIndex,
        modifier: RuleModifier,
    ) -> RuleModifierEntry {
        RuleModifierEntry {
            origin,
            modifier_type: Some(modifier_type),
            modifier,
        }
    }

    #[test]
    fn 没有任何来源时减伤恒为零() {
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");

        // Act
        let reduction = resistance_damage_reduction(&[], fire);

        // Assert
        assert_eq!(reduction, 0);
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
                rule_modifiers: vec![untyped(resistance(fire, 5))],
                ..TraitRule::default()
            },
        )]);

        // Act
        let entries = trait_rule_modifiers(&[TraitSource::new(race, &grants)], 1, &traits);

        // Assert
        assert_eq!(entries, vec![entry(trait_id, resistance(fire, 5))]);
        assert_eq!(resistance_damage_reduction(&entries, fire), 5);
    }

    #[test]
    fn 抗性只对匹配的伤害类别生效不对其它类别生效() {
        // Arrange
        let mut interner = Interner::new();
        let origin = index(&mut interner, "lostland:fire_hide");
        let fire = index(&mut interner, "lostland:fire");
        let cold = index(&mut interner, "lostland:cold");
        let entries = vec![entry(origin, resistance(fire, 5))];

        // Act & Assert
        assert_eq!(resistance_damage_reduction(&entries, fire), 5);
        assert_eq!(resistance_damage_reduction(&entries, cold), 0);
    }

    #[test]
    fn 装备这一路声明的抗性被收集进候选列表() {
        // Arrange
        let mut interner = Interner::new();
        let amulet = index(&mut interner, "lostland:ward_amulet");
        let fire = index(&mut interner, "lostland:fire");
        let items = FixedItems(vec![(amulet, vec![untyped(resistance(fire, 3))])]);
        let equipment = BTreeMap::from([(EquipSlot::NECK, ItemStack::new(amulet, 1))]);

        // Act
        let entries = equipment_rule_modifiers(&equipment, &items);

        // Assert
        assert_eq!(entries, vec![entry(amulet, resistance(fire, 3))]);
        assert_eq!(resistance_damage_reduction(&entries, fire), 3);
    }

    #[test]
    fn 耐久归零的装备不贡献任何规则修正() {
        // Arrange
        let mut interner = Interner::new();
        let amulet = index(&mut interner, "lostland:ward_amulet");
        let fire = index(&mut interner, "lostland:fire");
        let items = FixedItems(vec![(amulet, vec![untyped(resistance(fire, 3))])]);
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
        assert_eq!(resistance_damage_reduction(&from_items, fire), 0);
    }

    #[test]
    fn 同一个类型桶里的多条命中只挑一条而不是相加() {
        // Arrange：5 点与 8 点若相加会得到 13 点，而它们**同属一个桶**
        // （两条都不声明类型 = 同一个共享的未分类桶），因此只取最强的
        // 8 点。`trait-system.md` 三节③「不取乘积」那半句在加法模型下的
        // 对应表述就是「同类型不相加」。这里两级判据恰好同向（8 点既是
        // 最强的一条、origin 也更大——方向本身由
        // `抗性取最强的一条而不是注册顺序在先的那条` 单独钉）。
        let mut interner = Interner::new();
        let low = index(&mut interner, "lostland:aaa_low");
        let high = index(&mut interner, "lostland:zzz_high");
        let fire = index(&mut interner, "lostland:fire");
        assert!(low < high, "intern 顺序决定索引大小，low 必须更小");
        // 刻意把强的那条放在切片前面：结果必须与切片顺序无关（约束 C5）。
        let entries = vec![
            entry(high, resistance(fire, 8)),
            entry(low, resistance(fire, 5)),
        ];

        // Act
        let reduction = resistance_damage_reduction(&entries, fire);

        // Assert
        assert_eq!(reduction, 8);
    }

    #[test]
    fn 天赋与装备两路来源拼在一起时取最强的一条跨来源生效() {
        // Arrange：天赋那一条先 intern（origin 更小）、但**更弱**
        //（只减 1 点），装备那一条 origin 更大、却更强（减 5 点）。
        // 旧规则（按 origin 升序）会选天赋那条弱的；「取最强」裁定之后
        // 应当选装备那条强的——这正是所有者要修的形态在两个真实收集器
        // 之间的端到端版本。两条都不声明加值类型，因此同桶竞争。
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
                rule_modifiers: vec![untyped(resistance(acid, 1))],
                ..TraitRule::default()
            },
        )]);
        let items = FixedItems(vec![(amulet, vec![untyped(resistance(acid, 5))])]);
        let equipment = BTreeMap::from([(EquipSlot::NECK, ItemStack::new(amulet, 1))]);

        // Act：与 `agent_rule_modifiers` 内部完全相同的拼法，只是这里不
        // 需要造一个 `Agent`（构造成本见 `crate::traits::effective_traits`
        // 文档「为什么参数是 `&[TraitSource]`」一节）。
        let mut entries = trait_rule_modifiers(&[TraitSource::new(race, &grants)], 1, &traits);
        entries.extend(equipment_rule_modifiers(&equipment, &items));

        // Assert：两路来源都进了候选列表，tie-break 跨来源生效——装备
        // 那条更强（5 点 > 1 点，减伤越大越强），尽管它 origin 更大；
        // 也不是两者相加的 6 点（同桶不相加）。
        assert_eq!(entries.len(), 2);
        assert_eq!(resistance_damage_reduction(&entries, acid), 5);
    }

    #[test]
    fn 第三路来源不改任何消费者签名就能接进聚合结果() {
        // 这条测试把模块文档「接第三、第四路来源」那句主张钉成可执行的
        // 断言：模拟一路本批次**尚未实现**的来源（药品/限时 buff——它
        // 未来会产出的同样是一串 `RuleModifierEntry`），直接拼进切片，
        // `resistance_damage_reduction` 的签名与调用写法一个字符都
        // 不用改，就把这一路纳入了聚合。
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
                rule_modifiers: vec![untyped(resistance(acid, 1))],
                ..TraitRule::default()
            },
        )]);
        let mut entries = trait_rule_modifiers(&[TraitSource::new(race, &grants)], 1, &traits);
        assert_eq!(resistance_damage_reduction(&entries, acid), 1);

        // Act：第三路来源——本批次没有任何代码会产出它，测试直接构造。
        entries.push(entry(potion, resistance(acid, 9)));

        // Assert：药品那条更强，按同一条规则胜出——消费者完全不知道
        // 多了一路来源。
        assert!(potion < trait_id);
        assert_eq!(resistance_damage_reduction(&entries, acid), 9);
    }

    #[test]
    fn 没有偷袭声明时聚合结果为none() {
        // Act & Assert
        assert_eq!(sneak_attack_rule(&[]), None);
    }

    #[test]
    fn 装备声明的偷袭与天赋声明的偷袭走同一个聚合点() {
        // Arrange：偷袭这一路的内容注册入口目前只开放给天赋，但聚合点
        // 对全部变体一视同仁——本测试在 Rust 层证明装备声明的偷袭同样
        // 能被消费，说明 `equipment_rule_modifiers` 不是"只给抗性开的
        // 特例通道"。
        let mut interner = Interner::new();
        let dagger = index(&mut interner, "lostland:backstab_dagger");
        let items = FixedItems(vec![(
            dagger,
            vec![untyped(RuleModifier::SneakAttack {
                luck_chance_permille_per_point: 12,
                extra_damage: 7,
            })],
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
    fn 同一个类型桶里的多条偷袭取最强的一条而不是叠加() {
        // Arrange：origin 更大的那条追加伤害更高（999 > 5），应当由它
        // 胜出；两条都不声明类型，因此同桶，无论如何都不该是两者相加
        // （1004）。
        let mut interner = Interner::new();
        let low = index(&mut interner, "lostland:aaa_low");
        let high = index(&mut interner, "lostland:zzz_high");
        let entries = vec![
            entry(
                high,
                RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: 999,
                    extra_damage: 999,
                },
            ),
            entry(
                low,
                RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: 10,
                    extra_damage: 5,
                },
            ),
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
            entry(charm, resistance(fire, 4)),
            entry(
                dagger,
                RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: 3,
                    extra_damage: 4,
                },
            ),
        ];

        // Act & Assert：抗性消费者只看抗性那条（且只在类别匹配时），
        // 偷袭消费者只看偷袭那条。
        assert_eq!(resistance_damage_reduction(&entries, fire), 4);
        assert_eq!(resistance_damage_reduction(&entries, cold), 0);
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
        // 两个缺省值现在同为 `0`，但含义仍然不同：意愿的 `0` 是「一点
        // 也不减」，藏匿的 `0` 是「这条被动不在场」（调用方据此完全跳过
        // 判定，见 `inspection_concealment_permille` 文档「缺省 0 与
        // 声明 0」一节）。
        // Act & Assert
        assert_eq!(inspection_suspicion_reduction_permille(&[]), 0);
        assert_eq!(inspection_concealment_permille(&[]), 0);
    }

    #[test]
    fn 两个盘查规则修正各自被对应的消费者取到() {
        // Arrange
        let mut interner = Interner::new();
        let training = index(&mut interner, "lostland:cutpurse_training");
        let entries = vec![
            entry(
                training,
                RuleModifier::InspectionSuspicion {
                    suspicion_reduction_permille: 400,
                },
            ),
            entry(
                training,
                RuleModifier::InspectionConcealment {
                    conceal_permille: 800,
                },
            ),
        ];

        // Act & Assert：同一条天赋上的两个被动互不干扰——这正是所有者
        // 「被动可以分为 2 种」那句裁定在聚合层的形状。
        assert_eq!(inspection_suspicion_reduction_permille(&entries), 400);
        assert_eq!(inspection_concealment_permille(&entries), 800);
    }

    #[test]
    fn 两个盘查消费者不误判彼此的变体() {
        // 反例：只声明其中一个，另一个必须落回缺省值——证明上一条不是
        // 「随便有条规则修正就返回它的数」。
        // Arrange
        let mut interner = Interner::new();
        let origin = index(&mut interner, "lostland:only_suspicion");
        let entries = vec![entry(
            origin,
            RuleModifier::InspectionSuspicion {
                suspicion_reduction_permille: 250,
            },
        )];

        // Act & Assert
        assert_eq!(inspection_suspicion_reduction_permille(&entries), 250);
        assert_eq!(inspection_concealment_permille(&entries), 0);
    }

    #[test]
    fn 同强度的多条盘查声明按origin升序取第一条而不是相加也不依赖切片顺序() {
        // 这条钉的是 `merged_across_types` 那份被四个消费者共用的桶内
        // tie-break 的**第二级**：两条减 300‰ 的「不觉得可疑」强度完全
        // 相同，此时退回 origin 升序；结果仍然是 300‰ 而不是 600‰
        //（同一个桶不相加），且哪条胜出与调用方按什么顺序拼切片无关
        //（约束 C5）。
        // Arrange
        let mut interner = Interner::new();
        let early = index(&mut interner, "lostland:aaa_early");
        let late = index(&mut interner, "lostland:zzz_late");
        assert!(early < late);
        let early_entry = entry(
            early,
            RuleModifier::InspectionSuspicion {
                suspicion_reduction_permille: 300,
            },
        );
        let late_entry = entry(
            late,
            RuleModifier::InspectionSuspicion {
                suspicion_reduction_permille: 300,
            },
        );

        // Act：同样两条，两种拼接顺序。
        let forward =
            inspection_suspicion_reduction_permille(&[early_entry.clone(), late_entry.clone()]);
        let backward = inspection_suspicion_reduction_permille(&[late_entry, early_entry]);

        // Assert：都是 300（不是 600，也不是 0），且两种顺序一致。
        assert_eq!(forward, 300);
        assert_eq!(backward, 300);
    }

    #[test]
    fn 抗性取最强的一条而不是注册顺序在先的那条() {
        // 项目所有者裁定的直接落点，也是它要修的那个具体形态：一枚平庸
        // 护符（先被 intern，origin 小）不该压过一条强天赋（后被 intern，
        // origin 大）。减伤「越大越强」：8 点比 2 点强。
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let trinket = index(&mut interner, "lostland:aaa_mediocre_trinket");
        let talent = index(&mut interner, "lostland:zzz_strong_talent");
        assert!(
            trinket < talent,
            "护符必须先被 intern，才构成本条要修的形态"
        );
        let weak = entry(trinket, resistance(fire, 2));
        let strong = entry(talent, resistance(fire, 8));

        // Act：两种拼接顺序。
        let forward = resistance_damage_reduction(&[weak.clone(), strong.clone()], fire);
        let backward = resistance_damage_reduction(&[strong, weak], fire);

        // Assert：都取 8 点（最强），不是 2 点（origin 在先的那条），
        // 也不是 10 点（两者相加——同桶不相加）。
        assert_eq!(forward, 8);
        assert_eq!(backward, 8);
    }

    #[test]
    fn 三个消费者各自的强弱方向互不相同() {
        // 改成整数点数之后三个方向恰好同为「越大越强」，但那是这一版
        // 模型的**结果**，不是可以省掉声明的理由——本条把三个方向一次
        // 性钉住，是 `strength_key` 逐变体声明方向这件事的可执行断言。
        // 三组都让「较强的那条」origin 在后，于是「按 origin 升序」那条
        // 旧规则会给出与现规则相反的答案。
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let first = index(&mut interner, "lostland:aaa_first");
        let second = index(&mut interner, "lostland:zzz_second");
        assert!(first < second);

        // Act & Assert：抗性——减伤点数越大越强。
        assert_eq!(
            resistance_damage_reduction(
                &[
                    entry(first, resistance(fire, 1)),
                    entry(second, resistance(fire, 9)),
                ],
                fire,
            ),
            9,
        );

        // 盘查意愿——概率减点数越大越强。
        assert_eq!(
            inspection_suspicion_reduction_permille(&[
                entry(
                    first,
                    RuleModifier::InspectionSuspicion {
                        suspicion_reduction_permille: 100,
                    }
                ),
                entry(
                    second,
                    RuleModifier::InspectionSuspicion {
                        suspicion_reduction_permille: 900,
                    }
                ),
            ]),
            900,
        );

        // 盘查藏匿——藏匿概率越大越强。
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
    fn 桶内强度完全相同时退回origin升序且与切片顺序无关() {
        // 保住 C5 的那一条：这里直接对 `merged_across_types` 下断言，
        // 而不是经某个公开消费者——四个真实消费者的投影值都由强度键
        // 完全决定，一旦强度相同，返回值也必然相同，「哪一条胜出」在
        // 它们身上根本不可观察。探针挑的是 `strength_key` **不看**的那
        // 个字段：抗性的强度只由 damage_reduction 决定，damage_category
        // 完全不参与——于是两条减伤相同、类别不同的抗性强度键恒等，
        // 投影出类别就能读出胜者是谁。
        //
        // 两条都不声明类型，因此落在同一个桶里——本条钉的是桶内那一级,
        // 跨桶那一级由 `不同加值类型的减伤点数相加` 单独钉。
        // Arrange
        let mut interner = Interner::new();
        let early = index(&mut interner, "lostland:aaa_early");
        let late = index(&mut interner, "lostland:zzz_late");
        let fire = index(&mut interner, "lostland:fire");
        let cold = index(&mut interner, "lostland:cold");
        assert!(early < late);
        let early_entry = entry(early, resistance(fire, 5));
        let late_entry = entry(late, resistance(cold, 5));
        // 投影成 `i32`（类别索引的裸值）而不是 `ContentIndex` 本身:
        // `merged_across_types` 要求投影值实现 `AddAcrossTypes`，而
        // 「两个内容索引相加」没有任何意义、不该有这个实现。本条只有
        // 一个桶（两条都不声明类型），折叠一次都不会跑，裸值只是用来
        // 读出胜者是谁。
        let probe = |modifier: &RuleModifier| match modifier {
            RuleModifier::Resistance {
                damage_category, ..
            } => Some(damage_category.get() as i32),
            _ => None,
        };

        // Act：同样两条，两种拼接顺序。
        let forward = merged_across_types(&[early_entry.clone(), late_entry.clone()], probe);
        let backward = merged_across_types(&[late_entry, early_entry], probe);

        // Assert：两种顺序都取 origin 小的那条（火抗那一条）。
        assert_eq!(forward, Some(fire.get() as i32));
        assert_eq!(backward, Some(fire.get() as i32));
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
            vec![untyped(RuleModifier::InspectionConcealment {
                conceal_permille: 300,
            })],
        )]);
        let equipment = BTreeMap::from([(EquipSlot::OUTER, ItemStack::new(cloak, 1))]);

        // Act
        let entries = equipment_rule_modifiers(&equipment, &items);

        // Assert
        assert_eq!(inspection_concealment_permille(&entries), 300);
    }

    // ===================== 加值类型（分桶）=====================

    #[test]
    fn 不声明类型时结算结果与分桶之前逐位相同() {
        // **本批次最要紧的一条测试**，钉的是项目所有者点名的那个风险：
        // 3.5e 里「无类型」加值永远叠加，照搬会把本体与 example_mod 现有
        // 的每一条声明静默地从「取最强」改成「全部叠加」。所有者的裁定
        // 是「不声明类型的全部落进同一个共享桶」，于是分桶层在一份一条
        // 类型都没声明的内容上必须是**恒等变换**。
        //
        // 断言方式：对四个消费者各造一组「多条、全部不声明类型」的输入,
        // 逐个断言结果等于**桶内取最强**这条分桶之前就有的规则算出的值
        //（而不是相加的值）。四组的「相加会得到什么」都写在断言里，
        // 一旦默认值哪天被改成「每条各成一桶」，四条断言会同时变红。
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let a = index(&mut interner, "lostland:aaa_source");
        let b = index(&mut interner, "lostland:zzz_source");

        // Act & Assert（抗性）：取最强 6，不是相加的 10。
        assert_eq!(
            resistance_damage_reduction(
                &[entry(a, resistance(fire, 4)), entry(b, resistance(fire, 6))],
                fire
            ),
            6,
        );

        // （盘查意愿）：取最强 400，不是相加的 700。
        assert_eq!(
            inspection_suspicion_reduction_permille(&[
                entry(
                    a,
                    RuleModifier::InspectionSuspicion {
                        suspicion_reduction_permille: 300,
                    }
                ),
                entry(
                    b,
                    RuleModifier::InspectionSuspicion {
                        suspicion_reduction_permille: 400,
                    }
                ),
            ]),
            400,
        );

        // （盘查藏匿）：取最强 800，不是相加后钳顶的 999。
        assert_eq!(
            inspection_concealment_permille(&[
                entry(
                    a,
                    RuleModifier::InspectionConcealment {
                        conceal_permille: 700,
                    }
                ),
                entry(
                    b,
                    RuleModifier::InspectionConcealment {
                        conceal_permille: 800,
                    }
                ),
            ]),
            800,
        );

        // （偷袭）：取最强那一条整体，不是两条各字段相加的 (30, 12)。
        assert_eq!(
            sneak_attack_rule(&[
                entry(
                    a,
                    RuleModifier::SneakAttack {
                        luck_chance_permille_per_point: 20,
                        extra_damage: 5,
                    }
                ),
                entry(
                    b,
                    RuleModifier::SneakAttack {
                        luck_chance_permille_per_point: 10,
                        extra_damage: 7,
                    }
                ),
            ]),
            Some(SneakAttackRule {
                luck_chance_permille_per_point: 10,
                extra_damage: 7,
            }),
        );
    }

    #[test]
    fn 不同加值类型的减伤点数相加同一类型仍然取最强() {
        // 加值类型模型的核心断言，一条测试里两级都覆盖：
        // 附魔桶里有 3 与 2 两条（取最强 3），炼金桶里有 4 一条，
        // 结果是 3 + 4 = 7——既不是全部相加的 9，也不是全体取最强的 4。
        // Arrange
        let mut interner = Interner::new();
        let acid = index(&mut interner, "lostland:acid");
        let enhancement = index(&mut interner, "lostland:enhancement");
        let alchemical = index(&mut interner, "lostland:alchemical");
        let amulet = index(&mut interner, "lostland:amulet");
        let ring = index(&mut interner, "lostland:ring");
        let potion = index(&mut interner, "lostland:potion");
        let entries = vec![
            typed_entry(amulet, enhancement, resistance(acid, 3)),
            typed_entry(ring, enhancement, resistance(acid, 2)),
            typed_entry(potion, alchemical, resistance(acid, 4)),
        ];

        // Act
        let reduction = resistance_damage_reduction(&entries, acid);

        // Assert
        assert_eq!(reduction, 7);
    }

    #[test]
    fn 未分类桶与具名类型桶并列时同样相加() {
        // 未分类**是一个桶**，不是「不参与分桶」：它与任何具名类型之间
        // 照样相加。这条与上一条合起来说明 `None` 在算法里就是一个普通
        // 的桶键，只是全部未声明的条目共享它。
        // Arrange
        let mut interner = Interner::new();
        let acid = index(&mut interner, "lostland:acid");
        let enhancement = index(&mut interner, "lostland:enhancement");
        let hide = index(&mut interner, "lostland:hide");
        let scales = index(&mut interner, "lostland:scales");
        let amulet = index(&mut interner, "lostland:amulet");
        let entries = vec![
            // 未分类桶里两条：取最强 5。
            entry(hide, resistance(acid, 5)),
            entry(scales, resistance(acid, 1)),
            // 附魔桶里一条：3。
            typed_entry(amulet, enhancement, resistance(acid, 3)),
        ];

        // Act
        let reduction = resistance_damage_reduction(&entries, acid);

        // Assert：5 + 3，不是 9（全部相加），也不是 5（全体取最强）。
        assert_eq!(reduction, 8);
    }

    #[test]
    fn 跨类型相加与切片顺序无关() {
        // 约束 C5 在跨桶这一级的断言。加法可交换可结合，因此这条在
        // 数学上恒成立——写出来是为了钉住「实现里没有引入任何顺序敏感
        // 的步骤」（例如哪天有人把相加换回带截断的相乘）。
        // Arrange
        let mut interner = Interner::new();
        let acid = index(&mut interner, "lostland:acid");
        let first_type = index(&mut interner, "lostland:aaa_type");
        let second_type = index(&mut interner, "lostland:zzz_type");
        let a = index(&mut interner, "lostland:aaa_origin");
        let b = index(&mut interner, "lostland:zzz_origin");
        let one = typed_entry(a, first_type, resistance(acid, 3));
        let two = typed_entry(b, second_type, resistance(acid, 4));

        // Act
        let forward = resistance_damage_reduction(&[one.clone(), two.clone()], acid);
        let backward = resistance_damage_reduction(&[two, one], acid);

        // Assert
        assert_eq!(forward, 7);
        assert_eq!(backward, 7);
    }

    #[test]
    fn 偷袭跨类型时两个字段各自相加() {
        // `AddAcrossTypes for SneakAttackRule` 的可执行断言：两个字段
        // 各加各的，不相乘、也不是只加主键。
        // Arrange
        let mut interner = Interner::new();
        let innate = index(&mut interner, "lostland:innate");
        let enhancement = index(&mut interner, "lostland:enhancement");
        let talent = index(&mut interner, "lostland:talent");
        let dagger = index(&mut interner, "lostland:dagger");
        let entries = vec![
            typed_entry(
                talent,
                innate,
                RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: 20,
                    extra_damage: 15,
                },
            ),
            typed_entry(
                dagger,
                enhancement,
                RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: 5,
                    extra_damage: 4,
                },
            ),
        ];

        // Act
        let rule = sneak_attack_rule(&entries);

        // Assert
        assert_eq!(
            rule,
            Some(SneakAttackRule {
                luck_chance_permille_per_point: 25,
                extra_damage: 19,
            })
        );
    }

    #[test]
    fn 藏匿概率跨类型相加之后被钳在上界之下() {
        // 两个类型各 800‰ 相加是 1600‰，钳成 999‰——「永远不会必定
        // 成功」那条裁定的直接落点。乘数模型下同样两条会算成 960‰,
        // 加法在高位更容易触顶，这是模型换代的真实后果。
        // Arrange
        let mut interner = Interner::new();
        let innate = index(&mut interner, "lostland:innate");
        let enhancement = index(&mut interner, "lostland:enhancement");
        let hide = index(&mut interner, "lostland:hide");
        let cloak = index(&mut interner, "lostland:cloak");
        let entries = vec![
            typed_entry(
                hide,
                innate,
                RuleModifier::InspectionConcealment {
                    conceal_permille: 800,
                },
            ),
            typed_entry(
                cloak,
                enhancement,
                RuleModifier::InspectionConcealment {
                    conceal_permille: 800,
                },
            ),
        ];

        // Act & Assert
        assert_eq!(
            inspection_concealment_permille(&entries),
            1000 - PROBABILITY_MARGIN_PERMILLE
        );
    }

    #[test]
    fn 显式声明的零藏匿被钳到下界而不是当成没有这条被动() {
        // 「缺省 0」与「声明 0」是两个不同的意思，见
        // `inspection_concealment_permille` 文档同名一节：前者返回 0
        //（调用方完全跳过判定，一次随机数都不消耗），后者是一条真的
        // 存在、只是弱到几乎没用的被动，被钳到 1‰。
        // Arrange
        let mut interner = Interner::new();
        let origin = index(&mut interner, "lostland:threadbare_cloak");
        let entries = vec![entry(
            origin,
            RuleModifier::InspectionConcealment {
                conceal_permille: 0,
            },
        )];

        // Act & Assert
        assert_eq!(
            inspection_concealment_permille(&entries),
            PROBABILITY_MARGIN_PERMILLE
        );
        assert_eq!(inspection_concealment_permille(&[]), 0);
    }

    #[test]
    fn 概率钳制两端各留一线() {
        // 直接对钳制函数下断言：两个绝对端点都不可达，中间的合法值原样
        // 通过（证明这不是一条"把所有值都往中间挤"的规则）。
        // Act & Assert
        assert_eq!(clamp_probability_permille(0), PROBABILITY_MARGIN_PERMILLE);
        assert_eq!(
            clamp_probability_permille(1000),
            1000 - PROBABILITY_MARGIN_PERMILLE
        );
        assert_eq!(
            clamp_probability_permille(-5_000),
            PROBABILITY_MARGIN_PERMILLE
        );
        assert_eq!(
            clamp_probability_permille(5_000),
            1000 - PROBABILITY_MARGIN_PERMILLE
        );
        assert_eq!(clamp_probability_permille(1), 1);
        assert_eq!(clamp_probability_permille(500), 500);
        assert_eq!(clamp_probability_permille(999), 999);
    }

    // ===================== 减伤保底 =====================

    #[test]
    fn 减伤减不到零至少剩一点() {
        // 「不允许绝对免疫」那条裁定的直接落点，见
        // `MINIMUM_DAMAGE_AFTER_RESISTANCE` 文档。
        // Act & Assert
        assert_eq!(damage_after_resistance(10, 3), 7);
        assert_eq!(
            damage_after_resistance(10, 10),
            MINIMUM_DAMAGE_AFTER_RESISTANCE
        );
        assert_eq!(
            damage_after_resistance(10, 9_999),
            MINIMUM_DAMAGE_AFTER_RESISTANCE
        );
        // 负减伤 = 脆弱：多挨 5 点。
        assert_eq!(damage_after_resistance(10, -5), 15);
    }

    #[test]
    fn 本来就打不出伤害的那一下不会因为保底反倒开始掉血() {
        // 保底的意思是「挡不成绝对免疫」，不是「凭空造出一点伤害」——
        // 见 `damage_after_resistance` 文档「为什么保底只对本来就打得出
        // 伤害的那一下生效」一节。
        // Act & Assert
        assert_eq!(damage_after_resistance(0, 3), 0);
        assert_eq!(damage_after_resistance(0, 0), 0);
        assert_eq!(damage_after_resistance(-2, 3), -2);
    }

    #[test]
    fn 没有任何抗性声明时伤害逐位不变() {
        // 分桶层与减伤模型对「没有任何抗性声明」这条最常见的路径必须是
        // 恒等变换：减伤 0 点、保底不介入。
        // Act & Assert
        for damage in [1, 7, 100, 9_999] {
            assert_eq!(damage_after_resistance(damage, 0), damage);
        }
    }
}
