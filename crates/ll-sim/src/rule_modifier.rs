//! 规则修正（[`RuleModifier`]）的**多来源聚合点**——「一个实体此刻身上
//! 有哪些规则修正」这个问题的唯一答案处，以及在其之上的五个消费者
//! （抗性 [`resistance_damage_reduction`]、易伤
//! [`vulnerability_damage_increase`]、偷袭 [`sneak_attack_rule`]、
//! 盘查意愿 [`inconspicuous_check_modifier`]、盘查藏匿
//! [`concealment_check_modifier`]），连同它们共用的那一条 tie-break
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
//!   [`inconspicuous_check_modifier`]/[`concealment_check_modifier`]）
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
//! 五个变体恰好都是「越大越强」（减伤点数、追加伤害点数、概率减点数、
//! 藏匿概率、偷袭追加伤害），但这是这一版模型的**结果**，不是可以
//! 依赖的前提——乘数
//! 模型下抗性与盘查意愿都是「越小越强」，一个通用的「取最大值」会让
//! 它们反过来选**最弱**的那一条。真正的风险形态是「哪天有人加第五个
//! 变体、忘了传对比较器」。
//!
//! 因此方向不由调用点携带，而是集中在 [`strength_key`] 一个**无通配
//! 分支的穷尽 `match`** 里逐变体声明：新增一个变体而不声明它的方向，
//! `cargo build` 直接不过。这与 `ll_mod::content_hash` 的
//! `ContentTableKind`/`classify_index`「编译期强制穷尽」是同一条手法
//! （见该模块文档「编译期强制：穷尽解构 tables」一节），只是这次强制
//! 的对象不是「哈希覆盖面」。
//!
//! # 新增一个 `RuleModifier` 变体要改哪三处
//!
//! 同一条手法在本模块用了**三次**，三个函数各回答同一个枚举上一个
//! 互不相干的问题，全部是无通配分支的穷尽 `match`：
//!
//! | 函数 | 回答的问题 | 不补分支的后果 |
//! |---|---|---|
//! | [`strength_key`] | 同一个加值类型的桶里，哪一条算强？ | 编译不过 |
//! | [`cross_type_merge`] | 跨加值类型时，几条怎么合成一条？ | 编译不过 |
//! | [`display_shape`] | 玩家在角色面板上看到什么？ | 编译不过 |
//!
//! **这三处必须留在同一个文件里。** 它们之间没有任何可共享的算法，只
//! 有形状上的对称——按 ADR 0021 这恰恰是不该抽象成「变体元数据表」的
//! 情形（见 [`display_shape`] 文档同名一节）。它们唯一共有的东西是
//! 「新增变体时三处都要补」这条纪律，而那条纪律的可见性完全来自
//! **它们在一起**：拆到三个文件里，编译器仍然会挡住，但下一个人要读
//! 三个文件才知道自己该改几处。这也是本文件行数远超编码规范 800 行
//! 上限却不拆的第一条理由（第二条见下节）。
//!
//! `display_shape` 之外还要在 `assets/locales/{zh-CN,en}.ftl` 各补一条
//! 文案——文案本来就不该出现在 Rust 里（规格 §11.3）。呈现层
//! （`ll_ui::hud::character_panel`）**零改动**：它逐行查表加格式化，一个
//! `match` 都没有。
//!
//! # 为什么这个文件这么长
//!
//! 3700 余行里**只有约 550 行是正文代码**：其余是约 1200 行的文档注释
//! （本模块的每一条裁定都连着它的理由）与约 1900 行的测试。按正文代码
//! 量排，本文件在 `crates/**/src` 里排不进前五——`ll_sim::resolve`
//! （约 2260 行正文代码）、`ll_mod::content_audit`（约 1260）、
//! `ll_mod::content_hash`（约 1000）、`ll_mod::content_schema_gear`
//! （约 990）、`ll_world::chronicle`（约 730）都在它前面，且同样把
//! `#[cfg(test)] mod tests` 留在原文件里（本仓库没有一处例外）。
//!
//! 换句话说：这个文件长是因为它解释得多、测得多，不是因为它做得多。
//! 真要按行数拆，能搬走的只有测试模块，而那既没有仓库先例、也不减少
//! 任何一处认知负担——只是把字节挪到隔壁文件。
//!
//! # 热路径（ADR 0016/0017）
//!
//! 与 `crate::traits` 模块文档「为什么不缓存」一节同一档：本模块的
//! 聚合每次要用时现算，调用频率是「每次攻击结算一次」，不是逐格/逐帧。
//! 全程纯 Rust，不跨脚本边界（ADR 0016 一档：抗性是静态声明）。
//! 全程整数（ADR 0020 乙区），不引入任何 `f32` 中间值——规则修正的量
//! 是点数，聚合只有取最大值与加法两种运算，连整数除法都没有。

use std::collections::{BTreeMap, btree_map};

use crate::check::{CheckContext, RollBias};
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

/// 一次成功制作**至少**产出多少件——[`RuleModifier::CraftYield`] 的
/// 加成算完之后的下限，形状与理由照
/// [`MINIMUM_DAMAGE_AFTER_RESISTANCE`]，落实在 [`craft_product_count`]。
///
/// # 它做两件事，第二件比第一件重要
///
/// 1. **兜住负值**。`bonus_product_count` 允许为负（「手艺生疏」这类
///    负面天赋），与抗性允许负值表示「脆弱」是同一条先例；负得比配方
///    自己的产出数还多时，结果钳在这里。
/// 2. **把一条既有玩法裁定机制化**。产出恒 ≥ 1 意味着「消耗了材料却
///    什么都没拿到」在机制层面**不可能**发生——而那正是
///    `crafting-system.md` 九节⑤在玩法上否决过的「制作失败」（原文：
///    一次吃掉材料、什么都不给、玩家无法通过任何决策规避的失败，是
///    纯粹的挫败感）。因此**即使将来把字段收成无符号数**，这条常量
///    仍然要留：它守的不只是负数。
pub const MINIMUM_CRAFT_PRODUCT_COUNT: u32 = 1;

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
/// [`RuleModifier::InspectionSuspicion`] 的消费者在 **AI 决策侧**
/// （`ll_mod::native_behavior` 的卫兵行为树），理由见该变体文档；
/// [`RuleModifier::CraftYield`]（制作类副职奖励批次新增）见
/// [`craft_yield_bonus`] 与 `crate::resolve::resolve_craft` 文档
/// 「产出加成接线」一节。
///
/// **本枚举从此没有任何一个死变体**——判定系统落地批次接上了最后三个：
/// - [`RuleModifier::Advantage`]/[`RuleModifier::Disadvantage`] 等的
///   就是这套判定系统。消费者 [`check_roll_bias`]，落到
///   [`crate::check::RollBias`]：优势掷两轮取较大，劣势取较小，两者
///   同时存在时互相抵消。
/// - [`RuleModifier::RerollOnce`] 此前挂在「伤害公式求值器内部的
///   `roll_one_die` 钩子」上等着，那个钩子至今没有落地——但它等错了
///   地方：重掷是**判定**的原语，不是伤害公式的原语。消费者
///   [`check_reroll_value`]，落到 [`crate::check::CheckSide::reroll_on`]。
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
    /// # 脆弱**不**用负减伤表达，它是独立的一个变体
    ///
    /// 本变体此前不禁止负数，`-5` 就是「这类伤害对我多打 5 点」。那条
    /// 表达方式已经**撤销**，理由不是风格，是一个可复现的错误结果：
    /// 桶内的合并规则是「取最强」，而本变体的「强」由 [`strength_key`]
    /// 声明成**减伤点数越大越强**。于是同一个桶里 `+3` 与 `-5` 相遇时
    /// 取的是 `+3`——**脆弱被静默丢掉**。而「没声明加值类型的全部落进
    /// 同一个共享桶」是本模块刻意选的默认值（见
    /// [`TypedRuleModifier::modifier_type`]），因此任何一条不分类的
    /// 抗性都会吃掉同一个伤害类别上全部不分类的脆弱声明。
    ///
    /// 把符号翻过来（改成「越小越强」）救不了这件事，只会把方向反过来
    /// 再吃掉抗性：一个量既要在正半轴上「越大越强」又要在负半轴上
    /// 「越小越强」，本来就不该是同一个量。脆弱因此独立成
    /// [`RuleModifier::Vulnerability`]：两个量各自取最强、各自跨类型
    /// 相加，最后在 [`damage_after_resistance`] 里一减一加，谁也吃不掉谁。
    Resistance {
        /// 伤害类别，走 `damage-formula-mod-api.md` 十七节的开放
        /// `register-damage-category` 集合。
        damage_category: ContentIndex,
        /// 减伤点数，**恒非负**（装载期钳到零，见
        /// `ll_mod::content_schema_gear::RawRuleModifier`）。减完的保底见
        /// [`MINIMUM_DAMAGE_AFTER_RESISTANCE`]；反方向见
        /// [`RuleModifier::Vulnerability`]。
        damage_reduction: i32,
    },
    /// 易伤：该伤害类别的伤害，在减伤扣完之后**再加上**一个固定点数
    /// ——[`RuleModifier::Resistance`] 的对称量，同一条「整数点数、
    /// 加减法」的形状，方向相反。
    ///
    /// # 为什么是独立变体而不是负减伤
    ///
    /// 完整论证见 [`RuleModifier::Resistance`] 文档「脆弱**不**用负
    /// 减伤表达」一节：一个量不可能在正负两个半轴上同时满足「取最强」。
    /// 拆成两个量之后每个量都只在非负半轴上活动，「越大越强」对两者
    /// 各自成立、彼此不干扰。
    ///
    /// # 与减伤的对称性是完整的
    ///
    /// 同一条 DR 论证原样适用于本变体，只是方向反过来：易伤 5 点对
    /// 12 点来伤是多挨 42%，对 50 点只多挨 10%——**对小伤害强、对大
    /// 伤害弱**。乘数模型里的 `2000‰`（双倍）做不到这件事，它对 12 点
    /// 与 50 点一视同仁地翻倍。这与本模块把抗性从乘数换成点数时给出的
    /// 理由是同一条，因此两个方向应当用同一种形状表达。
    ///
    /// 合并规则也完整对称：同一加值类型内取最强（易伤越大越强），跨
    /// 加值类型相加，声明在 [`strength_key`] 与 [`cross_type_merge`]。
    ///
    /// # 它减不穿保底
    ///
    /// 易伤只往上加，[`MINIMUM_DAMAGE_AFTER_RESISTANCE`] 那条保底因此
    /// 与它无关；真正与它有关的是**净额一次算完再钳**，见
    /// [`damage_after_resistance`] 文档「为什么是一条算式一次钳」一节。
    Vulnerability {
        /// 伤害类别，与 [`RuleModifier::Resistance::damage_category`]
        /// 同一张开放注册表。
        damage_category: ContentIndex,
        /// 追加伤害点数，**恒非负**（装载期钳到零，理由同
        /// [`RuleModifier::Resistance::damage_reduction`]：负的易伤就是
        /// 减伤，两个变体各自只在非负半轴上活动，才谈得上「取最强」）。
        damage_increase: i32,
    },
    /// 重骰：该实体判定掷骰掷出 `value` 面时,立即重掷那一颗,取新值
    /// （不再检查新值是否又是 `value` —— 「一次」是硬边界，反复重掷
    /// 需要无界迭代，那是 [`crate::check`] 模块文档「边界」一节不肯
    /// 让出的线）。
    ///
    /// 对**全部**判定生效，不分 `check_context`：它描述的是「这个人
    /// 掷骰的手气」，不是「这个人擅长哪件事」——后者是修正点数的职责。
    ///
    /// `value` 的合法范围是 `1..=N`（`N` = [`crate::check::CHECK_DICE`]
    /// 的面数），装载期校验；写一个掷不出来的面值等于什么也没声明，
    /// 那是内容作者的笔误，不该静默通过。
    RerollOnce {
        /// 触发重掷的面值。
        value: i32,
    },
    /// 优势：该实体在 `check_context` 这类判定上掷两轮取较大
    /// （[`crate::check::RollBias::Advantage`]）。
    ///
    /// `check_context` 是开放标识符，引擎当前认得三个：
    /// [`crate::check::INSPECTION_CHECK`]（盘查）、
    /// [`crate::check::CONCEALMENT_CHECK`]（藏匿）与
    /// [`crate::check::CRITICAL_CHECK`]（暴击）。指向别的标识符不是
    /// 错误，只是当前没有判定会认领它——判定种类是一个会随系统长出来
    /// 的开放集合，装载期不该把还没写的判定判成非法。
    Advantage {
        /// 判定种类的开放标识符。
        check_context: NamespacedId,
    },
    /// 劣势，语义同 [`RuleModifier::Advantage`]，方向相反（掷两轮取
    /// 较小）。同一次判定上同时存在优势与劣势时**互相抵消**，见
    /// [`check_roll_bias`]。
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
    /// 与暴击（[`crate::combat::crit_attacker_modifier`]）是「战斗结算里
    /// 现成的、幸运能挂上去的判定点」同一个思路,但刻意不是暴击本身：
    /// 暴击对**全部**攻击者恒定生效（基准偏移写死在
    /// [`crate::combat::CRIT_BASE_CHECK_MODIFIER`]），偷袭是**只有声明
    /// 了这条天赋的角色才会触发**的判定，强度由天赋声明本身携带
    /// （`sneak_modifier`）——不同天赋可以有不同的强度,
    /// 不共用暴击那个全局偏移,见
    /// [`crate::combat::sneak_attacker_modifier`] 文档。
    SneakAttack {
        /// 加在**偷袭者那一侧**掷出点数上的整数点数（越大越容易
        /// 得手）——与 [`RuleModifier::InspectionConcealment`] 同一把
        /// 尺子，装载期已校验不超过修正上限 `L`
        /// （[`crate::check::CheckDice::max_modifier`]），跨来源相加
        /// 之后的总和由 [`crate::check::CheckDice::clamp_modifier`]
        /// 在判定那一刻再兜一次。
        ///
        /// 本字段此前是 `luck_chance_permille_per_point`（每点有效幸运
        /// 贡献的触发率加成，千分比）。偷袭迁进对抗判定之后**量尺
        /// 换了**，`ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION`
        /// 随之递增，见该常量文档「③ 偷袭迁进判定系统批次」一节。
        sneak_modifier: i32,
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
    /// # 换成判定修正（判定系统落地批次）
    ///
    /// 此前这个字段是**从触发概率上减掉的千分比点数**。那个形状有一个
    /// 可复现的病，上一版文档自己记了下来：`400` 从 `500‰`（常态）上
    /// 减掉是砍掉八成，从 `50‰`（潜行中）上减掉则直接触底被
    /// 概率钳制的下界 `1‰`。同一条被动在两个档上差一个数量级，
    /// **而那两个档本身是一个 10× 的乘法档**。
    ///
    /// 病根不在「减点数」这一半，在「基数是乘法档」那一半。对均匀骰,
    /// 「把修正加在掷出的数上」与「把修正加在概率上」本来就是同一个
    /// 运算（[`crate::check`] 模块文档给了式子），所以换写法救不了它。
    /// 真正的修法是把那个乘法档消掉：潜行不再换基数，它与本变体一样
    /// 是**隐蔽方的一个修正**，两者在同一把尺子上相加。本字段的量纲
    /// 因此从千分比改成**骰子点数**。
    ///
    /// 跨加值类型仍然相加（[`cross_type_merge`]），理由不变：加法可
    /// 交换可结合，既不引入整数除法，也不依赖合并顺序（约束 C5）。
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
    /// 拿到的是本模块 [`inconspicuous_check_modifier`] 算完的**一个
    /// 数**，不是一份候选列表：多来源取哪一条这件事不下放给行为树,
    /// 理由同本模块文档「跨来源 tie-break」一节。
    InspectionSuspicion {
        /// **加在被盘查者那一侧掷出的点数上**的整数点数（越大越不
        /// 起眼，`0` = 与常人无异）。量纲是骰子点数，不是千分比——
        /// 见本变体文档「换成判定修正」一节。
        ///
        /// 装载期校验它落在 `±CHECK_DICE.max_modifier()` 内
        /// （`ll_mod::content_schema_gear::RawRuleModifier`），运行期
        /// 聚合后再钳一次（[`crate::check::CheckDice::clamp_modifier`]）。
        inconspicuous_modifier: i32,
    },
    /// 被动②**「查不出东西」**（盗贼被动两分批次）——所有者裁定里的
    /// 后一种：盘查**照常发起**，只是搜身的人看不到你身上的东西。
    /// `concealment_modifier` 是**每一件**物品各判一次的判定修正点数。
    ///
    /// # 换成判定修正（判定系统落地批次）
    ///
    /// 此前 `conceal_permille` 是「每一件物品各自不被看见的千分比
    /// 概率」，一个与任何人无关的常数：搜身的人是谁、眼神好不好，
    /// 对结果一点影响都没有。改成对抗判定之后它变成**藏东西那一方
    /// 的一个修正**，与搜身者的察觉在同一把尺子上比大小，见
    /// [`crate::check`]。
    ///
    /// 它仍然是一个**加法量**：多个加值类型各自的点数直接相加
    /// （[`cross_type_merge`]），再由
    /// [`crate::check::CheckDice::clamp_modifier`] 钳进 `±L`。绝对
    /// 藏住与绝对藏不住都不可达，但这一次不是靠在概率上钳一个下界,
    /// 而是由修正上限**证明**的，见 [`crate::check`] 模块文档
    /// 「不允许绝对」一节。
    ///
    /// 「一条也没有声明」仍然是一个与 `0` 不同的特殊状态，见
    /// [`concealment_check_modifier`] 文档「缺省与声明 0」。
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
    /// `crate::resolve::resolve_inspect` 文档「藏匿判定」一节。改成
    /// 对抗判定之后这份代价变大了（每件物品从 1 次抽取变成 `2M` 次,
    /// 主动被动各掷一轮 `MdN`），粒度的论证一个字没变。
    InspectionConcealment {
        /// **加在藏东西那一方掷出的点数上**的整数点数（越大越藏得住），
        /// 每一件物品各判一次，见本变体文档。
        ///
        /// 装载期校验它落在 `±CHECK_DICE.max_modifier()` 内，运行期
        /// 聚合后再钳一次，同
        /// [`RuleModifier::InspectionSuspicion::inconspicuous_modifier`]。
        concealment_modifier: i32,
    },
    /// 制作产出加成（制作类副职奖励批次，
    /// `knowledge/design/crafting-subclass-rewards.md`）——在 `category`
    /// 这一类配方上，每一次**成功**制作的产出数量额外增加
    /// `bonus_product_count` 件。
    ///
    /// # 为什么是一条规则修正，而不是一个 `SkillEffect`
    ///
    /// 「会打铁」不是玩家按下去会发生什么的**动作**——玩家已经有
    /// [`crate::intent::Intent::Craft`] 这个动作了。「会打铁」是「**当我
    /// 制作时，结算方式不一样**」，而这正是本枚举的定位。硬做成
    /// [`crate::skill::SkillEffect`] 会具体错在三处：那个枚举的消费者是
    /// `crate::resolve::resolve_use_skill`（入口是
    /// [`crate::intent::Intent::UseSkill`]，玩家得先主动施放一次「打铁
    /// 精通」才能去打铁，这不是被动）；[`crate::skill::SkillRule`] 强制
    /// 携带冷却时间与资源消耗（一条「我会打铁」要冷却与法力是把身份属性
    /// 硬塞进技能框子）；而唯一形状对得上的
    /// [`crate::skill::SkillEffect::TemporaryStatModifier`] 作用在六维
    /// 主属性上，制作产出不是主属性。完整论证见设计文档二节。
    ///
    /// # 为什么必须按配方类别键控
    ///
    /// 一个铁匠不该因为会打铁就烧得一手好菜。与
    /// [`RuleModifier::Resistance`] 按伤害类别键控是同一个理由的既有
    /// 先例：两者都是「一个开放集合的某一个成员」，不是新概念。一条
    /// **全局**的制作精通还会与配方类别的副职闸门
    /// （[`crate::craft::RecipeCatalog::category_required_subclasses`]）
    /// 直接打架——那道闸门按类别分，奖励却不分。
    ///
    /// # 负值 = 手艺生疏
    ///
    /// 与 [`RuleModifier::Resistance`] 的「负值 = 脆弱」是同一条先例、
    /// 同一个理由：负面天赋（「手艺生疏」「诅咒的铁砧」）是内容作者
    /// 应当能表达的东西，静默禁掉它是一次不声明的能力退化。产出因此
    /// 由消费侧的 [`craft_product_count`] 保底在
    /// [`MINIMUM_CRAFT_PRODUCT_COUNT`] 件——那条保底不是防御性编程，
    /// 它同时是 `crafting-system.md` 九节⑤「不做制作失败」那条玩法
    /// 裁定的机制化，见该常量文档。
    CraftYield {
        /// 配方类别，指向配方类别表（`crafting.json5` 的
        /// `recipe_categories`），与 [`crate::craft::RecipeRule::category`]
        /// 是同一个号段。
        category: ContentIndex,
        /// 每次成功制作额外产出的件数。可为负，见本变体文档「负值 =
        /// 手艺生疏」。
        bonus_product_count: i32,
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
/// 同时有抗性和偷袭。塞进变体要给八个变体各加一个同名字段，而且每加
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
/// 在这里多 `extend` 一次。五个消费者（[`resistance_damage_reduction`]/
/// [`vulnerability_damage_increase`]/[`sneak_attack_rule`]/
/// [`inconspicuous_check_modifier`]/
/// [`concealment_check_modifier`]）与它们在 `crate::resolve`／AI 决策
/// 侧的调用点都不需要改动一个字符——这正是 `crate::traits::agent_trait_sources` 文档
/// 「其余两路为什么不在这里」所描述的那种「调用点不需要改一行」，
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
    subclass_grants: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    items: &dyn ItemCatalog,
) -> Vec<RuleModifierEntry> {
    let mut result = trait_rule_modifiers(
        &agent_trait_sources(agent, race_grants, class_grants, subclass_grants),
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

/// 制作产出加成消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里,
/// 取 `category` 匹配的 [`RuleModifier::CraftYield`] 的**额外产出件数**;
/// 一条也没命中时返回 `0`（没有加成，按配方声明的件数产出）。
///
/// `crate::resolve::resolve_craft` 在全部前置与食材校验都通过之后、
/// 产出成品那一步（第 9 步）调用本函数，把结果交给
/// [`craft_product_count`] 算出最终件数。
///
/// # 多条命中时怎么合：同类型取最强，不同类型相加
///
/// 与 [`resistance_damage_reduction`] 逐字同构，走同一个
/// [`merged_across_types`]，一行算法都没有新写：
///
/// 1. **同一个加值类型**（含「都没声明类型」这个共享桶）内部取最强
///    ——本变体的「强」是**多产出的件数越大越强**，方向逐变体声明在
///    [`strength_key`]。两条同类型的「+1 锻造产出」不会叠成 +2。
/// 2. **不同加值类型之间相加**——声明在 [`cross_type_merge`]。天赋
///    给的 +1 与附魔铁砧给的 +1 合起来是 +2。
///
/// 判据**不依赖 `modifiers` 切片自身的顺序**（约束 C5），理由与抗性
/// 那一条完全相同：桶内两级比较只与声明值和 `ContentIndex` 有关，
/// 跨桶是整数加法。
///
/// # 装备那一路是白拿的
///
/// [`agent_rule_modifiers`] 同时汇聚天赋路与装备路
/// （[`equipment_rule_modifiers`]），因此「大师级铁砧锤」这件**装备**
/// 携带同一条修正一行代码都不用加——与
/// [`RuleModifier::Resistance`] 已经同时走这两路是同一件事。
pub fn craft_yield_bonus(modifiers: &[RuleModifierEntry], category: ContentIndex) -> i32 {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::CraftYield {
            category: candidate_category,
            bonus_product_count,
        } if *candidate_category == category => Some(*bonus_product_count),
        _ => None,
    })
    .unwrap_or(0)
}

/// 易伤消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里，取
/// `damage_category` 匹配的 [`RuleModifier::Vulnerability`] 的**追加
/// 伤害点数**；一条也没命中时返回 `0`（不额外多挨一点）。
///
/// 与 [`resistance_damage_reduction`] 逐字同构：同一个
/// [`merged_across_types`]、同一套「同类型取最强、跨类型相加」，只是
/// 认领的变体与方向相反。两者**各自独立聚合**，这正是拆成两个变体要
/// 买到的东西——它们不在同一个桶里争「谁更强」，因此谁也吃不掉谁，见
/// [`RuleModifier::Resistance`] 文档「脆弱**不**用负减伤表达」一节。
pub fn vulnerability_damage_increase(
    modifiers: &[RuleModifierEntry],
    damage_category: ContentIndex,
) -> i32 {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::Vulnerability {
            damage_category: candidate_category,
            damage_increase,
        } if *candidate_category == damage_category => Some(*damage_increase),
        _ => None,
    })
    .unwrap_or(0)
}

/// 把一条配方声明的产出件数与 [`craft_yield_bonus`] 算出的加成合起来，
/// 并落实 [`MINIMUM_CRAFT_PRODUCT_COUNT`] 这条保底。
///
/// # 为什么中间量走 `i64`
///
/// 两端的类型不一样：`declared` 是 `u32`（[`crate::craft::RecipeRule::product_count`]
/// 恒 ≥ 1），`bonus` 是 `i32`（可正可负）。两者都是内容作者填的值，
/// 注册期不禁止极端值，因此没有一个 32 位类型同时装得下
/// `u32::MAX + i32::MAX` 与 `0 + i32::MIN`。`i64` 一次装下全部组合，
/// 之后只剩两次钳制——下限是本模块的保底常量，上限是 `u32::MAX`
/// （堆的 `count` 字段本身的值域）。全程整数，没有除法、没有浮点
/// （ADR 0020）。
///
/// # 与 [`damage_after_resistance`] 的一处刻意差异
///
/// 那一条对「本来就打不出伤害」的攻击原样返回（保底不该凭空造出伤害）；
/// 本条**没有**对应的短路，因为不存在「本来就产出 0 件」的配方——
/// `product_count` 注册期恒 ≥ 1，保底因此永远只在加成把它压下去时才
/// 起作用。
pub fn craft_product_count(declared: u32, bonus: i32) -> u32 {
    let raw = i64::from(declared).saturating_add(i64::from(bonus));
    let floored = raw.max(i64::from(MINIMUM_CRAFT_PRODUCT_COUNT));
    u32::try_from(floored).unwrap_or(u32::MAX)
}

/// 把一次攻击已经算好的伤害，减掉 `damage_reduction` 点减伤、加上
/// `damage_increase` 点易伤，并落实 [`MINIMUM_DAMAGE_AFTER_RESISTANCE`]
/// 这条保底：
///
/// ```text
/// 结果 = max(1, 伤害 − 减伤 + 易伤)
/// ```
///
/// # 为什么是一条算式一次钳，不是「先减完钳一次、再加易伤」
///
/// 两种写法在「减伤远大于来伤、同时又有易伤」这一格上给出完全不同的
/// 答案，必须明确裁定，不能靠代码顺序偶然决定：来伤 10、减伤 100、
/// 易伤 50——
///
/// - **一条算式一次钳**（本实现）：`max(1, 10 − 100 + 50) = 1`。
/// - 先钳后加：`max(1, 10 − 100) + 50 = 51`，比**没有任何抗性**时的
///   10 点还高出四倍。
///
/// 后者显然错：一件让目标「特别抗火」的装备不该因为目标同时「有点
/// 怕火」而把它挨的火伤放大。根因是那条保底把一个负得很深的中间值
/// 抬回 1，于是丢掉了「减伤还有多少富余」这个信息，易伤便加在了一个
/// 被人为抬高过的基数上。净额一次算完就没有这个中间值可丢——这也正是
/// 全整数加减法相对乘数链的一个具体好处：`a − b + c` 只有一个答案,
/// 不存在「先算哪一步」。
///
/// # 为什么保底只对「本来就打得出伤害」的那一下生效
///
/// `damage <= 0` 时原样返回：保底的意思是「挡不成绝对免疫」，不是
/// 「凭空造出一点伤害」。一次本来就打不出伤害的攻击（例如攻击力为零
/// 的占位公式）不该因为目标碰巧声明过抗性而反倒开始掉血——那会让本条
/// 保底变成一个隐蔽的伤害来源。
///
/// 这条提前返回同样覆盖易伤：一次打不出伤害的攻击不会因为目标怕火
/// 就开始打得出伤害。易伤是**放大既有伤害**的量，不是伤害来源。
pub fn damage_after_resistance(damage: i32, damage_reduction: i32, damage_increase: i32) -> i32 {
    if damage <= 0 {
        return damage;
    }
    damage
        .saturating_sub(damage_reduction)
        .saturating_add(damage_increase)
        .max(MINIMUM_DAMAGE_AFTER_RESISTANCE)
}

/// 一条规则修正的**强度比较键**——把「哪边算强」这件逐变体不同的事，
/// 规范化成一个统一的「越大越强」的整数键，好让
/// [`merged_across_types`] 只剩「取键最大的一条」这一件事要做。
///
/// # 为什么是两级，不是一个数
///
/// [`RuleModifier::SneakAttack`] 携带**两个**数（追加伤害与判定修正），
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

    /// 声明「这个数越**小**越强」——判定系统落地批次重新引入。
    ///
    /// 上一段文档说「比较键不再需要取负」，那句话在当时成立：那一批
    /// 全部有消费者的变体都是「点数越大效果越强」。
    /// [`RuleModifier::RerollOnce`] 接线后不再成立——重掷的是**掷出的
    /// 面值**，而重掷一个 `1` 是好事，重掷一个 `20` 是灾难，所以
    /// 「面值越小，这条重掷声明越强」。取负而不是让调用点传比较器，
    /// 仍然是本模块「方向是变体自己的属性」那条纪律。
    ///
    /// `value as i64` 先扩宽再取负：`i32::MIN` 直接取负会溢出，扩宽
    /// 之后不会。
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
/// `sneak_modifier` 作第二级——**这是对所有者裁定的
/// 细化，不是改写**：所有者说的「追加伤害越大越强」原样成立，第二级
/// 只在追加伤害完全相同时才起作用，而那正是所有者那句话没有覆盖、
/// 原本会直接掉进「谁先被 intern 谁赢」的区间。两个字段都是「越大越
/// 强」，方向上没有歧义；刻意**不**把两者相乘成「期望额外伤害」——那
/// 需要知道这次判定的有效幸运值、对手的察觉、双方有没有优劣势
/// （[`crate::combat::sneak_attacker_modifier`] 的输入只是其中一项），
/// 聚合层一样都拿不到，硬造一个模型只会是一条看起来精确、实则凭空
/// 发明的规则。判定系统迁移之后这条论证只增不减：修正到触发率的换算
/// 是钟形的，「多一点修正值多少触发率」本身就依赖当前的净差。
///
/// # 优势/劣势为什么不比强弱
///
/// [`RuleModifier::Advantage`]/[`RuleModifier::Disadvantage`] 接线之后
/// 仍然取 [`StrengthKey::INDISTINGUISHABLE`]，但理由变了：不再是
/// 「还没有消费者」，而是**它们的消费者不比强弱**。
/// [`check_roll_bias`] 问的是存在性（有没有人声明了优势 / 有没有人
/// 声明了劣势），两条优势与一条优势的结果逐位相同，因此没有一个
/// 「更强的优势」可言，也就没有方向要裁定。它们不进
/// [`merged_across_types`]。
///
/// # 为什么这里用 `R` 别名而不是写全 `RuleModifier::变体名`
///
/// `scripts/ci/check_field_consumers.py` 这道门禁按
/// 「决策层文件里有没有出现 `RuleModifier::变体名` 字面量」判定一个变体
/// 是否已被游戏逻辑消费。本函数对优势/劣势**不读它们的任何字段**，
/// 只是为了穷尽性必须点到名字——若在这里写出字面量，就等于用一次
/// 「点名」冒充一次「消费」。真正的消费在 [`check_roll_bias`] 与
/// [`check_reroll_value`]，那两处写的是全名。别名 `R` 因此保留：不为了
/// 让门禁看起来更绿而换来一份实际更弱的门禁，与
/// `RaceDef.stat_modifiers` 当初刻意换名是同一条既有纪律。
fn strength_key(modifier: &RuleModifier) -> StrengthKey {
    use crate::rule_modifier::RuleModifier as R;
    match modifier {
        // 减伤点数，越大越强：挡掉的伤害越多。
        R::Resistance {
            damage_reduction, ..
        } => StrengthKey::larger_is_stronger(*damage_reduction),
        // 追加伤害点数，越大越强：**「强」指这条修正本身有多强,不是
        // 它对谁有利**。易伤 6 比易伤 4 更强地表达了「怕火」这件事,
        // 于是同一个加值类型里取 6——与「两条免疫不叠成四分之一伤害」
        // 是同一条纪律的另一半，见 `merged_across_types` 文档。
        R::Vulnerability {
            damage_increase, ..
        } => StrengthKey::larger_is_stronger(*damage_increase),
        // 判定修正点数，越大越强：加在隐蔽方那一侧的点数越多越不起眼。
        R::InspectionSuspicion {
            inconspicuous_modifier,
        } => StrengthKey::larger_is_stronger(*inconspicuous_modifier),
        // 判定修正点数，越大越强：藏东西那一侧加得越多越藏得住。
        R::InspectionConcealment {
            concealment_modifier,
        } => StrengthKey::larger_is_stronger(*concealment_modifier),
        // 两个字段都越大越强，主键是追加伤害，见本函数文档「偷袭那两个字段」。
        R::SneakAttack {
            sneak_modifier,
            extra_damage,
        } => {
            StrengthKey::larger_is_stronger(*extra_damage).then_larger_is_stronger(*sneak_modifier)
        }
        // 额外产出件数，越大越强：一炉出得越多越好。**刻意写全名而不用
        // `R` 别名**——本变体有真实消费者（`craft_yield_bonus` →
        // `crate::resolve::resolve_craft` 第 9 步），
        // `scripts/ci/check_field_consumers.py` 该判它绿，不该被别名遮住。
        // 别名纪律只适用于下面优势/劣势那两条「只为穷尽性点名、不读任何
        // 字段」的分支，见本函数文档最后一节。
        RuleModifier::CraftYield {
            bonus_product_count,
            ..
        } => StrengthKey::larger_is_stronger(*bonus_product_count),
        // 重掷的**面值**越小越强：重掷一个 1 是好事，重掷一个 20 是
        // 灾难。这是本枚举唯一一个「越小越强」的量，方向声明见
        // `StrengthKey::smaller_is_stronger`。
        R::RerollOnce { value } => StrengthKey::smaller_is_stronger(*value),
        // 优劣势不比强弱，见本函数文档「优势/劣势为什么不比强弱」一节。
        R::Advantage { .. } | R::Disadvantage { .. } => StrengthKey::INDISTINGUISHABLE,
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
    /// **只取最强的一条**：跨加值类型也不相加，全体比一次强弱。
    ///
    /// 给的是那些**加起来没有意义**的量。当前只有
    /// [`RuleModifier::RerollOnce`]：它的载荷是一个**面值**，把
    /// 「重掷 1」与「重掷 5」加成「重掷 6」是把两个坐标当成了两段
    /// 长度，纯属胡说。真要同时支持多个重掷面值，需要一个面值集合
    /// 加一套「一颗骰最多重掷几次」的规则，是另一个批次的设计问题；
    /// 在那之前取最强的一条（面值最小的那条）是唯一不发明规则的选择。
    ///
    /// 判定系统落地批次之前这个变体叫 `Undecided`，含义是「还没有
    /// 消费者，等接线的那一批裁定」。三个死变体接线之后不再有「还没
    /// 裁定」的量，名字随之改成它实际做的事。
    StrongestOnly,
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
        // 追加伤害点数：与减伤逐字对称，诅咒 3 点 + 天生 4 点 = 7 点。
        R::Vulnerability { .. } => CrossTypeMerge::Add,
        // 判定修正点数：两个类型各 5 点就是 10 点，钳制在消费者那一侧。
        R::InspectionSuspicion { .. } => CrossTypeMerge::Add,
        // 逐件藏匿修正点数：同上，相加后钳进 ±L。
        R::InspectionConcealment { .. } => CrossTypeMerge::Add,
        // 追加伤害与判定修正两个字段各自相加，见 `AddAcrossTypes for SneakAttackRule`。
        R::SneakAttack { .. } => CrossTypeMerge::Add,
        // 额外产出件数：天赋 +1、附魔铁砧 +1，合起来 +2。全程整数加法，
        // 没有整数除法、没有截断、没有顺序依赖（约束 C5）。写全名的
        // 理由同 `strength_key` 里那一条：本变体有真实消费者。
        RuleModifier::CraftYield { .. } => CrossTypeMerge::Add,
        // 重掷面值加不得，见 `CrossTypeMerge::StrongestOnly` 文档。
        R::RerollOnce { .. } => CrossTypeMerge::StrongestOnly,
        // 优劣势不走这条链路（消费者是存在性判断，不是聚合），这里
        // 只是穷尽性必须点到名字，见 `strength_key` 文档同名一节。
        R::Advantage { .. } | R::Disadvantage { .. } => CrossTypeMerge::StrongestOnly,
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

/// 面板呈现用的数值向量（[`rule_modifier_displays`] 的 `T`）：逐位相加。
///
/// 同一次合并里的全部条目来自**同一个变体**（认领判据是文案键相同，
/// 而文案键与变体一一对应），因此两个向量的长度与含义逐位对齐——第
/// `i` 位在两边指的是同一个字段。偷袭那两个数因此与
/// [`AddAcrossTypes for SneakAttackRule`](AddAcrossTypes) 算出同样的
/// 结果，只是这里不必为每个变体各写一个结构体。
///
/// 长度真的不齐时按较短的那个截断（`zip` 的语义）而不是 panic：这只
/// 可能出于本模块内部的编程错误，而面板少显示一个数远好过让整个进程
/// 在绘制 HUD 时崩掉。
impl AddAcrossTypes for Vec<i32> {
    fn add_across_types(self, other: Self) -> Self {
        self.into_iter()
            .zip(other)
            .map(|(left, right)| left.saturating_add(right))
            .collect()
    }
}

impl AddAcrossTypes for SneakAttackRule {
    /// 两个字段**各自**相加：追加伤害加追加伤害，判定修正加判定修正。
    /// 刻意不相乘、不取其中一个作主——两个字段回答的是不同的问题
    /// （触发之后打多少 / 多容易触发），没有一个把另一个吸收掉的
    /// 自然方式，见 [`strength_key`] 文档「偷袭那两个字段」一节同一条
    /// 论证的另一面。
    ///
    /// 相加的结果**可能越过修正上限 `L`**，这是对的：装载期只校验
    /// 单条声明，跨来源相加之后的总和由
    /// [`crate::check::CheckDice::clamp_modifier`] 在判定那一刻兜底，
    /// 见该函数文档「这是『不允许绝对』的运行期执行点」一节。
    fn add_across_types(self, other: Self) -> Self {
        SneakAttackRule {
            sneak_modifier: self.sneak_modifier.saturating_add(other.sneak_modifier),
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
/// 保证，但 [`CrossTypeMerge::StrongestOnly`] 那一支要在桶之间再比一次
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
        // 只取最强：不合并，全体再比一次强弱。
        CrossTypeMerge::StrongestOnly => {
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
/// [`RuleModifier::InspectionSuspicion`] 的**判定修正点数**；一条也
/// 没有时返回 `0`（与常人无异，一点也不加）。
///
/// 真正的消费点在 **AI 决策侧**（`ll_mod::native_behavior` 的卫兵
/// 行为树），不是 `crate::resolve`——理由见
/// [`RuleModifier::InspectionSuspicion`] 文档「消费者在 AI 决策侧」
/// 一节。聚合仍然留在这里：行为树拿到的是算完的一个数。
///
/// # 本函数不钳制，钳制在判定里
///
/// 返回的只是这一路来源的贡献，调用方还要把属性调整值、潜行加成加
/// 上去，那个**总和**才是要钳的东西。钳制因此统一落在
/// [`crate::check::opposed_check`] 内部
/// （[`crate::check::CheckDice::clamp_modifier`]），不在这里。
///
/// # 缺省 0 与声明 0：这里刻意**不**区分
///
/// 与 [`concealment_check_modifier`] 相反（见其文档同名一节）：盘查
/// 判定**恒发生**——卫兵要不要拦下一个人，与这个人有没有「不起眼」
/// 这条被动无关。没有声明就是「这一路贡献 0 点」，不是「跳过判定」，
/// 因此不需要 `Option` 那一档。
///
/// 合并规则同 [`resistance_damage_reduction`]（[`merged_across_types`]）：
/// 同类型取最强、跨类型相加。本变体的「强」是**加得越多越强**，方向
/// 声明在 [`strength_key`]。
pub fn inconspicuous_check_modifier(modifiers: &[RuleModifierEntry]) -> i32 {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::InspectionSuspicion {
            inconspicuous_modifier,
        } => Some(*inconspicuous_modifier),
        _ => None,
    })
    .unwrap_or(0)
}

/// 优势/劣势消费者——这个实体在 `context` 这类判定上的掷骰偏向。
///
/// # 抵消，不叠加，不计数
///
/// 同时声明了优势与劣势 → [`crate::check::RollBias::Normal`]，与
/// D&D 5e 同一条规则。理由不是致敬：**它让结果与来源条数无关**。
/// 「三条优势对一条劣势」若按条数净算，结果就取决于聚合出来的候选
/// 列表里各有几条，而那份列表的长度依赖于天赋/装备的枚举路径——正是
/// 约束 C5 要防的那类隐性顺序/计数依赖。存在性判断没有这个问题。
///
/// 因此本函数**不走** [`merged_across_types`]：那套「分桶 → 桶内取
/// 最强 → 跨桶合并」是为「有大小的量」准备的，而优势没有大小。加值
/// 类型（`modifier_type`）对它同样没有意义——两条不同类型的优势与
/// 两条同类型的优势，结果一样。
pub fn check_roll_bias(modifiers: &[RuleModifierEntry], context: CheckContext) -> RollBias {
    let mut has_advantage = false;
    let mut has_disadvantage = false;
    for entry in modifiers {
        match &entry.modifier {
            RuleModifier::Advantage { check_context } if context.matches(check_context) => {
                has_advantage = true;
            }
            RuleModifier::Disadvantage { check_context } if context.matches(check_context) => {
                has_disadvantage = true;
            }
            _ => {}
        }
    }
    match (has_advantage, has_disadvantage) {
        (true, false) => RollBias::Advantage,
        (false, true) => RollBias::Disadvantage,
        // 两者都有 → 抵消；两者都无 → 本来就没有偏向。
        (true, true) | (false, false) => RollBias::Normal,
    }
}

/// 重掷消费者——这个实体判定掷骰时，掷出哪个面值要重掷一次；一条也
/// 没有声明时返回 `None`（不重掷，一个随机数都不多取）。
///
/// 与优势/劣势不同，重掷**有大小**（面值越小越强，见 [`strength_key`]），
/// 因此照常走 [`merged_across_types`]：同类型取最强、跨类型也取最强
/// （[`CrossTypeMerge::StrongestOnly`]，理由见该变体文档）。
///
/// 不分 `check_context`：见 [`RuleModifier::RerollOnce`] 文档。
pub fn check_reroll_value(modifiers: &[RuleModifierEntry]) -> Option<i32> {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::RerollOnce { value } => Some(*value),
        _ => None,
    })
}

/// 被动②消费者——在 [`agent_rule_modifiers`] 汇总出的候选列表里取
/// [`RuleModifier::InspectionConcealment`] 的逐件藏匿判定修正点数；
/// 一条也没有时返回 `None`。
///
/// 消费点是 `crate::resolve::resolve_inspect`，见其文档「藏匿判定」
/// 一节。合并规则同 [`resistance_damage_reduction`]：同类型取最强、
/// 跨类型相加；本变体的「强」是**点数越大越强**（藏得越严实）。
///
/// # 缺省与声明 0：两个不同的意思，用 `Option` 而不是 `0` 区分
///
/// - **一条也没有声明** → `None`，含义是「**这条被动不在场**」。
///   调用方据此完全跳过藏匿判定，一次随机数都不消耗（约束 C3：不该
///   为一条不存在的规则空转一次确定性随机流）。这一档不能省：藏匿
///   判定是**逐件**的，一次盘查跳过的是 `2M × 件数` 次抽取，不是一次。
/// - **显式声明成 `0`** → `Some(0)`，含义是「你有这条被动，只是它一
///   点忙也帮不上」。判定照常发生（并因此照常消耗随机数），只是这一
///   路贡献 0 点，胜负交给双方的属性调整值。
///
/// 旧实现用返回值 `0` 兼表两件事，靠「显式 0 会被概率钳制抬成 `1‰`」
/// 这条副作用把它们分开——那条兜底随概率模型一起走了（`两端各留一线`
/// 现在由 [`crate::check::CheckDice::max_modifier`] 的推导保证，不再靠
/// 在结果上钳一个下界），`Option` 是同一个区分的直说法。
pub fn concealment_check_modifier(modifiers: &[RuleModifierEntry]) -> Option<i32> {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::InspectionConcealment {
            concealment_modifier,
        } => Some(*concealment_modifier),
        _ => None,
    })
}

/// [`sneak_attack_rule`] 的返回值——一次偷袭判定需要的两个数：加给
/// 偷袭者那一侧的判定修正与触发后追加的固定伤害。两个数打包成一个小
/// 结构体而不是元组，理由同 `crate::formula::FormulaInputs` 之类既有
/// 惯例：调用点按字段名读取，不必记住元组位置的含义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SneakAttackRule {
    /// 加在偷袭者那一侧掷出点数上的整数点数。
    pub sneak_modifier: i32,
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
/// 偷袭的「强」是**两个数都越大越强**（追加伤害作主键，判定修正作
/// 第二级）——它是本枚举唯一携带两个数值字段的变体，取舍见
/// [`strength_key`] 文档「偷袭那两个字段」一节。
pub fn sneak_attack_rule(modifiers: &[RuleModifierEntry]) -> Option<SneakAttackRule> {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::SneakAttack {
            sneak_modifier,
            extra_damage,
        } => Some(SneakAttackRule {
            sneak_modifier: *sneak_modifier,
            extra_damage: *extra_damage,
        }),
        _ => None,
    })
}

/// 一条规则修正在角色面板上的呈现数据——**合并之后**的一行，不是一条
/// 原始声明。
///
/// # 为什么面板拿到的是合并值
///
/// 玩家问面板的问题是「我现在每次多产出几件」「这一刀我少挨几点」，
/// 这两个问题只有合并值答得上。逐条列原始声明会**主动误导**：制作
/// 精通天赋 `+1` 与附魔铁砧 `+1` 分两行读成「两次 +1」，而实际生效的
/// 是 `+2`（跨加值类型相加，见 [`CrossTypeMerge::Add`]）；反过来两枚
/// 同款护符各写 `+1`、同属一个加值类型时实际只生效 `+1`（桶内取最强，
/// 见 [`strength_key`]），逐条列会读成 `+2`。两个方向都错，而且错得
/// 与合并规则本身有关，不是显示精度问题。
///
/// 合并值走的是**既有**那条路径（[`merged_across_types`]），与
/// [`resistance_damage_reduction`]、[`craft_yield_bonus`] 等结算消费者
/// 同一个函数，不另算一遍——面板上的数与结算时用的数因此不可能分叉。
///
/// # 纯合并值丢掉的那件事由 `source_count` 补回来
///
/// 只显示 `+1` 的话，戴两枚同款护符的玩家无从知道第二枚为什么没生效。
/// [`Self::source_count`] 是**合并前**落进这一行的原始声明条数，于是
/// 那种情况显示成「产出 +1（2 项来源）」：数字仍然是真实生效的那个，
/// 而「有两条声明、只算出 +1」这件事本身可见。这里刻意**不**列来源
/// 明细（哪件装备、哪条天赋）——那会把一行变成一段，也会让面板高度
/// 随装备变化剧烈跳动（`ll_ui::hud::build_panel` 按行数现算高度）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleModifierDisplay {
    /// 修正种类的文案键，逐变体声明在 [`display_shape`]。
    ///
    /// 是 `&'static str` 常量而不是文案本身：规格 §11.3 要求用户可见
    /// 文本一律走 `.ftl`，本层不认识任何一种自然语言。
    pub name_key: &'static str,
    /// 主语的文案键（抗性/易伤的伤害类别、制作产出的配方类别、优劣势
    /// 的判定种类），`None` 表示这个变体没有主语。
    ///
    /// # 内容表里的主语读字段，判定种类拼键
    ///
    /// 伤害类别与配方类别都有自己的内容表，两张表各自声明了一个真正的
    /// `display_name_key` 字段（`ll_mod::damage_category::DamageCategoryDef`
    /// 与 `ll_mod::recipe_category::RecipeCategoryDef`）——本字段直接
    /// 装的就是内容作者写进去的那个键，**本层不拼、不猜**。装配点用
    /// [`rule_modifier_displays`] 的 `subject_name_key` 回调跨过
    /// 「`ll-sim` 不认识内容表」这条依赖边界。
    ///
    /// 判定种类（[`RuleModifier::Advantage`]/[`RuleModifier::Disadvantage`]
    /// 的 `check_context`）是**唯一**仍然按
    /// `命名空间:check_context.路径.display_name` 现拼的一处，理由不是
    /// 省事而是没有别的答案：判定种类不是内容，是引擎侧的开放标识符
    /// （`ll_sim::check::CheckContext` 那三条 `&'static str` 常量），
    /// 没有一张表可以读，见 [`subject_key`] 文档。
    pub subject_key: Option<String>,
    /// 数值实参，按 `.ftl` 消息里 `{ $名 }` 的变量名成对给出。
    ///
    /// **空表是合法值**，表示这条修正没有数值可显示（优势/劣势只有
    /// 「有没有」，见 [`RuleModifier::Advantage`]）。用「名字→值」的
    /// 序列而不是固定字段，是为了让呈现层一个 `match` 都不需要写：
    /// 它只是把每一对塞进 Fluent 实参表，元数（偷袭两个、抗性一个、
    /// 优势零个）由本层声明，将来第十个变体带三个数也不必改呈现层。
    pub amounts: Vec<(&'static str, i64)>,
    /// 合并**前**落进这一行的原始声明条数，恒 `>= 1`。语义见本结构体
    /// 文档「纯合并值丢掉的那件事」一节。
    pub source_count: usize,
}

/// 主语的原始形式——[`display_shape`] 的内部返回形状，不对外。
///
/// 两个变体的区别是「主语背后有没有一张内容表」，不只是「索引还是
/// 标识符」：
///
/// * [`Self::Content`] 的主语在内容表里，那张表自己声明了
///   `display_name_key`——文案键**读出来**，走
///   [`rule_modifier_displays`] 的 `subject_name_key` 回调。
/// * [`Self::Id`] 的主语是判定种类，引擎侧的开放标识符，没有表可读
///   ——文案键只能**拼出来**，见 [`subject_key`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplaySubject<'a> {
    /// 内容表里的主语——`registry` 说的是「去哪张表读」。
    Content {
        registry: SubjectRegistry,
        index: ContentIndex,
    },
    /// 判定种类，`registry` 是拼文案键时用的注册表段名。
    Id {
        registry: &'static str,
        id: &'a NamespacedId,
    },
}

/// 一个主语所属的内容表——[`rule_modifier_displays`] 的
/// `subject_name_key` 回调靠它决定去哪张表读 `display_name_key`。
///
/// 只有两个变体，因为只有两个变体的主语在内容表里（抗性/易伤指伤害
/// 类别，制作产出指配方类别）。判定种类**刻意不在这里**：它没有表，
/// 加一个查不到东西的变体只会让回调多一条永远返回 `None` 的分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectRegistry {
    /// `ll_mod::damage_category::DamageCategoryTable`。
    DamageCategory,
    /// `ll_mod::recipe_category::RecipeCategoryTable`。
    RecipeCategory,
}

/// 一条原始声明在面板上的形状——[`display_shape`] 的返回值。
struct DisplayShape<'a> {
    /// 见 [`RuleModifierDisplay::name_key`]。
    name_key: &'static str,
    /// 主语的原始形式，`None` 表示这个变体没有主语。
    subject: Option<DisplaySubject<'a>>,
    /// 这一条声明自己的数值（合并之前），名字与
    /// [`RuleModifierDisplay::amounts`] 同一套。
    amounts: Vec<(&'static str, i32)>,
}

/// 抗性一行的文案键。
pub const RESISTANCE_NAME_KEY: &str = "rule-modifier-resistance";
/// 易伤一行的文案键。
pub const VULNERABILITY_NAME_KEY: &str = "rule-modifier-vulnerability";
/// 重掷一行的文案键。
pub const REROLL_ONCE_NAME_KEY: &str = "rule-modifier-reroll_once";
/// 优势一行的文案键。
pub const ADVANTAGE_NAME_KEY: &str = "rule-modifier-advantage";
/// 劣势一行的文案键。
pub const DISADVANTAGE_NAME_KEY: &str = "rule-modifier-disadvantage";
/// 偷袭一行的文案键。
pub const SNEAK_ATTACK_NAME_KEY: &str = "rule-modifier-sneak_attack";
/// 盘查减免一行的文案键。
pub const INSPECTION_SUSPICION_NAME_KEY: &str = "rule-modifier-inspection_suspicion";
/// 藏匿一行的文案键。
pub const INSPECTION_CONCEALMENT_NAME_KEY: &str = "rule-modifier-inspection_concealment";
/// 制作产出一行的文案键。
pub const CRAFT_YIELD_NAME_KEY: &str = "rule-modifier-craft_yield";

/// 单数值变体的 Fluent 实参名。
const AMOUNT_ARG: &str = "amount";
/// 偷袭第二个数值（追加伤害）的 Fluent 实参名。
const EXTRA_ARG: &str = "extra";

/// 判定种类在文案键里的注册表段名——本模块**唯一**剩下的一条拼键约定，
/// 理由见 [`subject_key`]。伤害类别与配方类别此前也各有一条同类常量，
/// 两张表都声明了真正的 `display_name_key` 字段之后它们被删掉了：那两
/// 处现在读字段，不拼键。
const CHECK_CONTEXT_REGISTRY: &str = "check_context";

/// 逐变体声明「这条修正在面板上长什么样」——本模块**第三个**逐变体
/// 穷尽 `match`，与 [`strength_key`]、[`cross_type_merge`] 并列，同一
/// 条纪律：**没有通配分支**。
///
/// # 为什么是第三条并列声明而不是新抽象
///
/// 三个函数回答的是同一个枚举上三个互不相干的问题——「同一个桶里谁更
/// 强」「跨桶怎么合」「玩家看到什么」。它们之间**没有可共享的算法**，
/// 只有形状上的对称（都是逐变体穷尽），按 ADR 0021
/// （`knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md`）
/// 这恰恰是不该抽象的情形：把三者塞进一个「变体元数据表」trait 只会让
/// 每个问题的答案离它的理由更远，换不来任何一行共享逻辑。
///
/// # 新增第十个变体要改哪里
///
/// Rust 侧**一处**：本函数。不补分支编译不过（无通配分支），与
/// [`strength_key`]/[`cross_type_merge`] 是同一条保证。另外要在
/// `assets/locales/zh-CN.ftl` 与 `assets/locales/en.ftl` 各补一条新
/// `name_key` 的文案——文案本来就不该出现在 Rust 里（规格 §11.3）。
/// 呈现层（`ll_ui::hud::character_panel`）**零改动**：它只是逐行查表
/// 加格式化，一个 `match` 都没有。
///
/// # 九个变体全部要显示
///
/// 三个曾经的死变体（[`RuleModifier::RerollOnce`]、
/// [`RuleModifier::Advantage`]、[`RuleModifier::Disadvantage`]）在判定
/// 系统落地时已经全部接上消费者（[`check_reroll_value`]、
/// [`check_roll_bias`]），因此本函数**没有**「返回空表示不显示」这条
/// 通道：一条修正只要在实体身上生效，玩家就该看得见。主语 `None` 只
/// 表示这个变体的语义里没有主语（盘查减免与藏匿是实体自身的属性，不
/// 针对某一类东西），不是「没接线」的标记。
fn display_shape(modifier: &RuleModifier) -> DisplayShape<'_> {
    use crate::rule_modifier::RuleModifier as R;
    match modifier {
        // 减伤点数，主语是伤害类别。
        R::Resistance {
            damage_category,
            damage_reduction,
        } => DisplayShape {
            name_key: RESISTANCE_NAME_KEY,
            subject: Some(DisplaySubject::Content {
                registry: SubjectRegistry::DamageCategory,
                index: *damage_category,
            }),
            amounts: vec![(AMOUNT_ARG, *damage_reduction)],
        },
        // 追加伤害点数，与减伤逐字对称。
        R::Vulnerability {
            damage_category,
            damage_increase,
        } => DisplayShape {
            name_key: VULNERABILITY_NAME_KEY,
            subject: Some(DisplaySubject::Content {
                registry: SubjectRegistry::DamageCategory,
                index: *damage_category,
            }),
            amounts: vec![(AMOUNT_ARG, *damage_increase)],
        },
        // 重掷面值：这里的数不是「加了多少」而是「掷出几点会重掷」,
        // 单位差别由 `.ftl` 文案表达，不由本层再加一个单位枚举——
        // 呈现层看到的都是「一个数」，怎么念是文案的事。
        R::RerollOnce { value } => DisplayShape {
            name_key: REROLL_ONCE_NAME_KEY,
            subject: None,
            amounts: vec![(AMOUNT_ARG, *value)],
        },
        // 优势没有数值，只有「在哪类判定上有」——`amounts` 空表。
        R::Advantage { check_context } => DisplayShape {
            name_key: ADVANTAGE_NAME_KEY,
            subject: Some(DisplaySubject::Id {
                registry: CHECK_CONTEXT_REGISTRY,
                id: check_context,
            }),
            amounts: Vec::new(),
        },
        // 劣势，与优势逐字对称。
        R::Disadvantage { check_context } => DisplayShape {
            name_key: DISADVANTAGE_NAME_KEY,
            subject: Some(DisplaySubject::Id {
                registry: CHECK_CONTEXT_REGISTRY,
                id: check_context,
            }),
            amounts: Vec::new(),
        },
        // 偷袭是本枚举唯一带两个数的变体，两个数各自跨类型相加
        // （`AddAcrossTypes for SneakAttackRule`），这里的两项实参与
        // 那条合并规则一一对应。
        R::SneakAttack {
            sneak_modifier,
            extra_damage,
        } => DisplayShape {
            name_key: SNEAK_ATTACK_NAME_KEY,
            subject: None,
            amounts: vec![(AMOUNT_ARG, *sneak_modifier), (EXTRA_ARG, *extra_damage)],
        },
        // 盘查减免：加在隐蔽方那一侧，没有主语。
        R::InspectionSuspicion {
            inconspicuous_modifier,
        } => DisplayShape {
            name_key: INSPECTION_SUSPICION_NAME_KEY,
            subject: None,
            amounts: vec![(AMOUNT_ARG, *inconspicuous_modifier)],
        },
        // 逐件藏匿修正：同上，没有主语。
        R::InspectionConcealment {
            concealment_modifier,
        } => DisplayShape {
            name_key: INSPECTION_CONCEALMENT_NAME_KEY,
            subject: None,
            amounts: vec![(AMOUNT_ARG, *concealment_modifier)],
        },
        // 额外产出件数，主语是配方类别。**刻意写全名而不用 `R` 别名**,
        // 理由同 `strength_key` 里那一条：本变体有真实消费者,
        // `scripts/ci/check_field_consumers.py` 不该被别名遮住。
        RuleModifier::CraftYield {
            category,
            bonus_product_count,
        } => DisplayShape {
            name_key: CRAFT_YIELD_NAME_KEY,
            subject: Some(DisplaySubject::Content {
                registry: SubjectRegistry::RecipeCategory,
                index: *category,
            }),
            amounts: vec![(AMOUNT_ARG, *bonus_product_count)],
        },
    }
}

/// 求一个主语的文案键。
///
/// # 两条来路，不是两种拼法
///
/// * [`DisplaySubject::Content`]：**读**内容表声明的
///   `display_name_key`，`subject_name_key` 回调负责跨过依赖边界去查
///   那张表。本层对键长什么样没有任何要求——内容作者写
///   `examplemod:damage_category_acid_display_name` 也好、写
///   `examplemod:酸.名` 也好，原样交给 `Catalog::resolve`。
/// * [`DisplaySubject::Id`]：**拼** `命名空间:check_context.路径.display_name`。
///   判定种类是引擎侧的开放标识符（`crate::check::CheckContext`），
///   没有内容表可读，而内容作者可以在 `check_context` 字段里写任何
///   标识符（[`RuleModifier::Advantage`] 文档：「指向别的标识符不是
///   错误」），所以这里除了按约定拼一条键没有别的答案。这条约定的
///   代价照旧：`.ftl` 里漏了这条键，面板上显示的是键名本身。
///
/// 返回 `None` 表示这一行没有可显示的主语——内容索引查不到定义（装载期
/// 本不该放过）。此时调用方跳过整行，而不是显示一个半截的主语。
fn subject_key(
    subject: DisplaySubject<'_>,
    subject_name_key: &dyn Fn(SubjectRegistry, ContentIndex) -> Option<NamespacedId>,
) -> Option<String> {
    match subject {
        DisplaySubject::Content { registry, index } => {
            Some(subject_name_key(registry, index)?.to_string())
        }
        DisplaySubject::Id { registry, id } => Some(format!(
            "{}:{registry}.{}.display_name",
            id.namespace(),
            id.path()
        )),
    }
}

/// 一行的身份：文案键 + 主语键。同一个变体、同一个主语的全部声明合成
/// 一行。
type DisplayRowKey = (&'static str, Option<String>);

/// 把一份规则修正清单折叠成角色面板要显示的若干行。
///
/// `subject_name_key` 回答「这张表的这一条，显示名文案键是什么」——本体
/// 里就是从 `ll_mod::damage_category::DamageCategoryTable` /
/// `ll_mod::recipe_category::RecipeCategoryTable` 里取出那条内容自己
/// 声明的 `display_name_key`。本层不持有内容表，用回调跨这条依赖边界
/// 是本仓库的既有写法，见
/// `ll_mod::base_damage_category::register_base_damage_category`。
///
/// 它**替代**了此前那个 `resolve_id: Fn(ContentIndex) -> NamespacedId`
/// 回调：那一版拿索引还原出的是内容**自己的 id**（`lostland:fire`），
/// 再按约定拼成一条文案键。约定拼键的代价写在
/// `ll_mod::damage_category` 模块文档里——mod 作者没有任何提示知道该在
/// `locales/` 补哪条键，漏了就在面板上看到键名本身。现在键是内容表里
/// 的真字段，漏写在装载期就报错。
///
/// # 行的顺序是确定的
///
/// 结果按（文案键，主语键）字典序排列。这两者都是编译期常量或内容
/// 标识符派生出来的字符串，与获得顺序、装载顺序、`ContentIndex` 的
/// 数值都无关——同一套修正在任何一次运行里都排成同样的顺序（约束 C1
/// 那条确定性在呈现层的对应物）。按获得顺序排也是确定的，但那会让
/// 「先捡到哪件装备」决定面板行序，玩家看不出规律。
///
/// # 每一行的数值怎么来
///
/// 走 [`merged_across_types`]，与 [`resistance_damage_reduction`]、
/// [`craft_yield_bonus`] 等结算消费者同一个函数：桶内取最强
/// （[`strength_key`]）、跨桶按 [`cross_type_merge`] 合并。面板因此
/// 不可能与结算算出不同的数。
///
/// 认领同一行的判据是「文案键相同且主语键相同」；文案键与变体一一
/// 对应（[`display_shape`] 逐变体各给一个常量），所以这等价于
/// [`merged_across_types`] 文档要求的「同一个 `select` 只认领同一个
/// 变体」。
///
/// # 复杂度
///
/// 分组一趟、每组再扫一遍全表，即 `O(行数 × 声明数)`。一个实体身上的
/// 规则修正是十几条的量级（天赋 + 装备），面板每帧重建一次也远谈不上
/// 热点；换来的是数值与结算共用同一条合并链路，不必把
/// [`merged_across_types`] 拆开重写一个批量版本。
pub fn rule_modifier_displays(
    modifiers: &[RuleModifierEntry],
    subject_name_key: &dyn Fn(SubjectRegistry, ContentIndex) -> Option<NamespacedId>,
) -> Vec<RuleModifierDisplay> {
    // 第一趟：分组、数原始声明条数、记下这一组的实参名。实参名逐变体
    // 固定，同一组每条都一样，取第一条的即可。
    let mut rows: BTreeMap<DisplayRowKey, (Vec<&'static str>, usize)> = BTreeMap::new();
    for entry in modifiers {
        let shape = display_shape(&entry.modifier);
        let key = match shape.subject {
            None => None,
            Some(subject) => {
                let Some(resolved) = subject_key(subject, subject_name_key) else {
                    tracing::warn!(
                        name_key = shape.name_key,
                        "规则修正的主语索引在内容表里查不到显示名键，本行跳过" // i18n-exempt：面向开发者的诊断信息，不是玩家会看到的文本
                    );
                    continue;
                };
                Some(resolved)
            }
        };
        let names: Vec<&'static str> = shape.amounts.iter().map(|(name, _)| *name).collect();
        let slot = rows.entry((shape.name_key, key)).or_insert((names, 0));
        slot.1 += 1;
    }

    // 第二趟：每一行各走一次既有的合并链路。
    rows.into_iter()
        .map(|((name_key, row_subject_key), (names, source_count))| {
            let merged = merged_across_types(modifiers, |modifier| {
                let shape = display_shape(modifier);
                if shape.name_key != name_key {
                    return None;
                }
                let candidate = match shape.subject {
                    None => None,
                    Some(subject) => subject_key(subject, subject_name_key),
                };
                if candidate != row_subject_key {
                    return None;
                }
                Some(
                    shape
                        .amounts
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect::<Vec<i32>>(),
                )
            })
            .expect("这一行是从某条真实声明来的，合并必然认领到它");
            RuleModifierDisplay {
                name_key,
                subject_key: row_subject_key,
                amounts: names
                    .into_iter()
                    .zip(merged)
                    .map(|(name, value)| (name, i64::from(value)))
                    .collect(),
                source_count,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{CONCEALMENT_CHECK, INSPECTION_CHECK};
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
                    base_price: ll_core::scaled::Milli::ZERO,
                    wear_channels: crate::item::WearChannels::NONE,
                    max_durability: None,
                    taught_recipes: Vec::new(),
                    requires_identification: false,
                    study_experience: 0,
                    blind_box_pool: Vec::new(),
                    furniture: false,
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
                sneak_modifier: 12,
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
                sneak_modifier: 12,
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
                    sneak_modifier: 999,
                    extra_damage: 999,
                },
            ),
            entry(
                low,
                RuleModifier::SneakAttack {
                    sneak_modifier: 10,
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
                sneak_modifier: 999,
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
                    sneak_modifier: 3,
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
                sneak_modifier: 3,
                extra_damage: 4,
            })
        );
    }

    #[test]
    fn 优势与劣势同时声明时互相抵消() {
        // 这条钉的是 `check_roll_bias` 文档「抵消，不叠加，不计数」
        // 那一节：抵消不是致敬 D&D，是它让结果与来源条数无关，因而与
        // 聚合顺序无关（约束 C5）。
        // Arrange
        let mut interner = Interner::new();
        let a = index(&mut interner, "lostland:a");
        let b = index(&mut interner, "lostland:b");
        let c = index(&mut interner, "lostland:c");
        let advantage = |origin| {
            entry(
                origin,
                RuleModifier::Advantage {
                    check_context: NamespacedId::parse("lostland:inspection")
                        .expect("测试用标识符恒合法"),
                },
            )
        };
        let disadvantage = |origin| {
            entry(
                origin,
                RuleModifier::Disadvantage {
                    check_context: NamespacedId::parse("lostland:inspection")
                        .expect("测试用标识符恒合法"),
                },
            )
        };

        // Act & Assert
        assert_eq!(check_roll_bias(&[], INSPECTION_CHECK), RollBias::Normal);
        assert_eq!(
            check_roll_bias(&[advantage(a)], INSPECTION_CHECK),
            RollBias::Advantage
        );
        assert_eq!(
            check_roll_bias(&[disadvantage(a)], INSPECTION_CHECK),
            RollBias::Disadvantage
        );
        // 三条优势 + 一条劣势 → 抵消，不是「优势净胜两条」。
        assert_eq!(
            check_roll_bias(
                &[advantage(a), advantage(b), advantage(c), disadvantage(a)],
                INSPECTION_CHECK
            ),
            RollBias::Normal
        );
        // 两种拼接顺序结果一致。
        assert_eq!(
            check_roll_bias(&[advantage(a), disadvantage(b)], INSPECTION_CHECK),
            check_roll_bias(&[disadvantage(b), advantage(a)], INSPECTION_CHECK)
        );
    }

    #[test]
    fn 优势只对声明的那一类判定生效() {
        // Arrange
        let mut interner = Interner::new();
        let origin = index(&mut interner, "lostland:cutpurse_training");
        let entries = vec![entry(
            origin,
            RuleModifier::Advantage {
                check_context: NamespacedId::parse("lostland:inspection")
                    .expect("测试用标识符恒合法"),
            },
        )];

        // Act & Assert：盘查有优势，藏匿没有——两环是两条独立的判定。
        assert_eq!(
            check_roll_bias(&entries, INSPECTION_CHECK),
            RollBias::Advantage
        );
        assert_eq!(
            check_roll_bias(&entries, CONCEALMENT_CHECK),
            RollBias::Normal
        );
    }

    #[test]
    fn 重掷取面值最小的一条且跨加值类型也只取最强() {
        // 重掷是本枚举唯一「越小越强」的量：重掷一个 1 是好事，重掷
        // 一个 20 是灾难。跨类型**不相加**（面值加不得），见
        // `CrossTypeMerge::StrongestOnly` 文档。
        // Arrange
        let mut interner = Interner::new();
        let innate = index(&mut interner, "lostland:innate");
        let enhancement = index(&mut interner, "lostland:enhancement");
        let a = index(&mut interner, "lostland:a");
        let b = index(&mut interner, "lostland:b");

        // Act & Assert：一条也没有 → None，一次随机数都不多取。
        assert_eq!(check_reroll_value(&[]), None);

        // 同一个（未分类）桶里取面值更小的那条。
        assert_eq!(
            check_reroll_value(&[
                entry(a, RuleModifier::RerollOnce { value: 5 }),
                entry(b, RuleModifier::RerollOnce { value: 2 }),
            ]),
            Some(2)
        );

        // 跨加值类型：取 2，不是 5 + 2 = 7 这种把面值当长度加的胡说。
        assert_eq!(
            check_reroll_value(&[
                typed_entry(a, innate, RuleModifier::RerollOnce { value: 5 }),
                typed_entry(b, enhancement, RuleModifier::RerollOnce { value: 2 }),
            ]),
            Some(2)
        );
    }

    #[test]
    fn 没有任何来源声明时两个盘查消费者各自返回自己的缺省值() {
        // 两个缺省值的**类型**不同，而这正是它们含义不同的落点：意愿
        // 的 `0` 是「这一路贡献 0 点」（盘查判定照常发生），藏匿的
        // `None` 是「这条被动不在场」（调用方据此完全跳过逐件判定，
        // 一次随机数都不消耗）。判定系统落地批次把后者从「返回 0，靠
        // 概率钳制把显式 0 抬成 1‰ 来区分」改成了直说的 `Option`，见
        // `concealment_check_modifier` 文档「缺省与声明 0」一节。
        // Act & Assert
        assert_eq!(inconspicuous_check_modifier(&[]), 0);
        assert_eq!(concealment_check_modifier(&[]), None);
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
                    inconspicuous_modifier: 9,
                },
            ),
            entry(
                training,
                RuleModifier::InspectionConcealment {
                    concealment_modifier: 9,
                },
            ),
        ];

        // Act & Assert：同一条天赋上的两个被动互不干扰——这正是所有者
        // 「被动可以分为 2 种」那句裁定在聚合层的形状。两个 9 取的是
        // `mods/example_mod/traits.json5` 里扒手训练的真实声明值（半颗
        // 骰子，见 `crate::check::CheckDice::half_die`），不再是旧模型
        // 的千分比 400/800。
        assert_eq!(inconspicuous_check_modifier(&entries), 9);
        assert_eq!(concealment_check_modifier(&entries), Some(9));
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
                inconspicuous_modifier: 250,
            },
        )];

        // Act & Assert
        assert_eq!(inconspicuous_check_modifier(&entries), 250);
        assert_eq!(concealment_check_modifier(&entries), None);
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
                inconspicuous_modifier: 300,
            },
        );
        let late_entry = entry(
            late,
            RuleModifier::InspectionSuspicion {
                inconspicuous_modifier: 300,
            },
        );

        // Act：同样两条，两种拼接顺序。
        let forward = inconspicuous_check_modifier(&[early_entry.clone(), late_entry.clone()]);
        let backward = inconspicuous_check_modifier(&[late_entry, early_entry]);

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
            inconspicuous_check_modifier(&[
                entry(
                    first,
                    RuleModifier::InspectionSuspicion {
                        inconspicuous_modifier: 100,
                    }
                ),
                entry(
                    second,
                    RuleModifier::InspectionSuspicion {
                        inconspicuous_modifier: 900,
                    }
                ),
            ]),
            900,
        );

        // 盘查藏匿——藏匿概率越大越强。
        assert_eq!(
            concealment_check_modifier(&[
                entry(
                    first,
                    RuleModifier::InspectionConcealment {
                        concealment_modifier: 100,
                    }
                ),
                entry(
                    second,
                    RuleModifier::InspectionConcealment {
                        concealment_modifier: 900,
                    }
                ),
            ]),
            Some(900),
        );
    }

    #[test]
    fn 偷袭取追加伤害最大的一条追加伤害相同时再比判定修正() {
        // 偷袭是唯一携带两个数值字段的变体，两级键都要钉：主键
        // extra_damage（所有者点名的那一个），相同时才比
        // sneak_modifier。两组都让胜出者 origin 在后。
        // Arrange
        let mut interner = Interner::new();
        let first = index(&mut interner, "lostland:aaa_first");
        let second = index(&mut interner, "lostland:zzz_second");
        assert!(first < second);
        let low_damage_high_luck = entry(
            first,
            RuleModifier::SneakAttack {
                sneak_modifier: 90,
                extra_damage: 3,
            },
        );
        let high_damage_low_luck = entry(
            second,
            RuleModifier::SneakAttack {
                sneak_modifier: 10,
                extra_damage: 7,
            },
        );

        // Act & Assert（主键）：追加伤害更大的那条胜出，即便它幸运
        // 判定修正更低、origin 更大。
        assert_eq!(
            sneak_attack_rule(&[low_damage_high_luck, high_damage_low_luck]),
            Some(SneakAttackRule {
                sneak_modifier: 10,
                extra_damage: 7,
            }),
        );

        // Act & Assert（第二级）：追加伤害相同，改由判定修正决胜——
        // 这一档正是所有者那句「追加伤害越大越强」没有覆盖、原本会掉进
        // 「谁先被 intern 谁赢」的区间。
        assert_eq!(
            sneak_attack_rule(&[
                entry(
                    first,
                    RuleModifier::SneakAttack {
                        sneak_modifier: 10,
                        extra_damage: 5,
                    }
                ),
                entry(
                    second,
                    RuleModifier::SneakAttack {
                        sneak_modifier: 40,
                        extra_damage: 5,
                    }
                ),
            ]),
            Some(SneakAttackRule {
                sneak_modifier: 40,
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
                concealment_modifier: 300,
            })],
        )]);
        let equipment = BTreeMap::from([(EquipSlot::OUTER, ItemStack::new(cloak, 1))]);

        // Act
        let entries = equipment_rule_modifiers(&equipment, &items);

        // Assert
        assert_eq!(concealment_check_modifier(&entries), Some(300));
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
            inconspicuous_check_modifier(&[
                entry(
                    a,
                    RuleModifier::InspectionSuspicion {
                        inconspicuous_modifier: 300,
                    }
                ),
                entry(
                    b,
                    RuleModifier::InspectionSuspicion {
                        inconspicuous_modifier: 400,
                    }
                ),
            ]),
            400,
        );

        // （盘查藏匿）：取最强 800，不是相加的 1500。
        assert_eq!(
            concealment_check_modifier(&[
                entry(
                    a,
                    RuleModifier::InspectionConcealment {
                        concealment_modifier: 700,
                    }
                ),
                entry(
                    b,
                    RuleModifier::InspectionConcealment {
                        concealment_modifier: 800,
                    }
                ),
            ]),
            Some(800),
        );

        // （偷袭）：取最强那一条整体，不是两条各字段相加的 (30, 12)。
        assert_eq!(
            sneak_attack_rule(&[
                entry(
                    a,
                    RuleModifier::SneakAttack {
                        sneak_modifier: 20,
                        extra_damage: 5,
                    }
                ),
                entry(
                    b,
                    RuleModifier::SneakAttack {
                        sneak_modifier: 10,
                        extra_damage: 7,
                    }
                ),
            ]),
            Some(SneakAttackRule {
                sneak_modifier: 10,
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
                    sneak_modifier: 20,
                    extra_damage: 15,
                },
            ),
            typed_entry(
                dagger,
                enhancement,
                RuleModifier::SneakAttack {
                    sneak_modifier: 5,
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
                sneak_modifier: 25,
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
                    concealment_modifier: 800,
                },
            ),
            typed_entry(
                cloak,
                enhancement,
                RuleModifier::InspectionConcealment {
                    concealment_modifier: 800,
                },
            ),
        ];

        // Act & Assert：两个加值类型直接相加，**这里不钳**——钳制在
        // 判定里（`CheckDice::clamp_modifier`），聚合层如实交出总和。
        // 旧实现在这里就把 `800 + 800` 钳成了 `999‰`，那是概率模型时代
        // 「值域就是 0..=1000」留下的；判定修正没有那个天然上界，钳制
        // 的依据是骰子跨度，而聚合层看不见是哪把骰子。
        assert_eq!(concealment_check_modifier(&entries), Some(1600));
    }

    #[test]
    fn 显式声明的零藏匿与根本没有这条被动是两回事() {
        // 「缺省」与「声明 0」是两个不同的意思，见
        // `concealment_check_modifier` 文档同名一节：前者返回 `None`
        //（调用方完全跳过判定，一次随机数都不消耗），后者返回
        // `Some(0)`——一条真的存在、只是一点忙也帮不上的被动，判定照常
        // 发生（并因此照常消耗随机数），胜负交给双方的属性调整值。
        // Arrange
        let mut interner = Interner::new();
        let origin = index(&mut interner, "lostland:threadbare_cloak");
        let entries = vec![entry(
            origin,
            RuleModifier::InspectionConcealment {
                concealment_modifier: 0,
            },
        )];

        // Act & Assert
        assert_eq!(concealment_check_modifier(&entries), Some(0));
        assert_eq!(concealment_check_modifier(&[]), None);
    }

    // ===================== 减伤保底 =====================

    #[test]
    fn 减伤减不到零至少剩一点() {
        // 「不允许绝对免疫」那条裁定的直接落点，见
        // `MINIMUM_DAMAGE_AFTER_RESISTANCE` 文档。
        // Act & Assert
        assert_eq!(damage_after_resistance(10, 3, 0), 7);
        assert_eq!(
            damage_after_resistance(10, 10, 0),
            MINIMUM_DAMAGE_AFTER_RESISTANCE
        );
        assert_eq!(
            damage_after_resistance(10, 9_999, 0),
            MINIMUM_DAMAGE_AFTER_RESISTANCE
        );
    }

    #[test]
    fn 易伤把伤害加回去且与减伤方向严格对称() {
        // 易伤是减伤的对称量：同一个点数，一个减一个加，见
        // `RuleModifier::Vulnerability` 文档。
        // Act & Assert
        // 只有易伤：10 + 5 = 15（此前用 `damage_reduction: -5` 表达的
        // 那一格，现在有了正规写法，数值逐位相同）。
        assert_eq!(damage_after_resistance(10, 0, 5), 15);
        // 减伤与易伤同时在场：净额 10 − 3 + 5 = 12。两者互不吞噬——
        // 这正是拆成两个变体买到的东西。
        assert_eq!(damage_after_resistance(10, 3, 5), 12);
        // 严格对称：同一个点数一减一加抵消回原值。
        for point in [1, 4, 7, 1_000] {
            assert_eq!(damage_after_resistance(50, point, point), 50);
        }
    }

    #[test]
    fn 净额一次算完再钳而不是减完先钳再加易伤() {
        // 见 `damage_after_resistance` 文档「为什么是一条算式一次钳」
        // 一节：来伤 10、减伤 100、易伤 50。
        // - 一条算式一次钳（本实现）：max(1, 10 − 100 + 50) = 1。
        // - 先钳后加（错的那种）：max(1, 10 − 100) + 50 = 51,比完全
        //   没有抗性时的 10 点还高四倍。
        // Act
        let damage = damage_after_resistance(10, 100, 50);

        // Assert
        assert_eq!(damage, MINIMUM_DAMAGE_AFTER_RESISTANCE);
        assert_ne!(damage, 51);
    }

    #[test]
    fn 本来就打不出伤害的那一下不会因为保底反倒开始掉血() {
        // 保底的意思是「挡不成绝对免疫」，不是「凭空造出一点伤害」——
        // 见 `damage_after_resistance` 文档「为什么保底只对本来就打得出
        // 伤害的那一下生效」一节。
        // Act & Assert
        assert_eq!(damage_after_resistance(0, 3, 0), 0);
        assert_eq!(damage_after_resistance(0, 0, 0), 0);
        assert_eq!(damage_after_resistance(-2, 3, 0), -2);
        // 易伤同样不是伤害来源：一次打不出伤害的攻击不会因为目标怕火
        // 就开始打得出伤害。
        assert_eq!(damage_after_resistance(0, 0, 5), 0);
        assert_eq!(damage_after_resistance(-2, 0, 5), -2);
    }

    #[test]
    fn 没有任何抗性声明时伤害逐位不变() {
        // 分桶层与减伤模型对「没有任何抗性声明」这条最常见的路径必须是
        // 恒等变换：减伤 0 点、易伤 0 点、保底不介入。
        // Act & Assert
        for damage in [1, 7, 100, 9_999] {
            assert_eq!(damage_after_resistance(damage, 0, 0), damage);
        }
    }

    // ── 制作产出加成（制作类副职奖励批次）──────────────────────────

    #[test]
    fn 制作产出加成只对匹配的配方类别生效不对其它类别生效() {
        // 「一个铁匠不该因为会打铁就烧得一手好菜」——本变体必须按配方
        // 类别键控那条论证的可执行版本。
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let cooking = index(&mut interner, "lostland:cooking");
        let mastery = index(&mut interner, "lostland:forging_mastery");
        let entries = vec![entry(
            mastery,
            RuleModifier::CraftYield {
                category: forging,
                bonus_product_count: 1,
            },
        )];

        // Act & Assert
        assert_eq!(craft_yield_bonus(&entries, forging), 1);
        assert_eq!(craft_yield_bonus(&entries, cooking), 0);
    }

    #[test]
    fn 没有任何制作产出加成时返回零() {
        // 缺省值就是「按配方声明的件数产出」，`craft_product_count(n, 0)
        // == n`，因此不带这条天赋的角色与本批次之前逐位相同。
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");

        // Act & Assert
        assert_eq!(craft_yield_bonus(&[], forging), 0);
    }

    #[test]
    fn 同一加值类型的两条制作产出加成取最强而不是相加() {
        // 桶内取最强，与抗性同一条规则、同一段代码（`merged_across_types`）。
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let enhancement = index(&mut interner, "lostland:enhancement");
        let anvil = index(&mut interner, "lostland:masterwork_anvil");
        let hammer = index(&mut interner, "lostland:masterwork_hammer");
        let entries = vec![
            typed_entry(
                anvil,
                enhancement,
                RuleModifier::CraftYield {
                    category: forging,
                    bonus_product_count: 2,
                },
            ),
            typed_entry(
                hammer,
                enhancement,
                RuleModifier::CraftYield {
                    category: forging,
                    bonus_product_count: 1,
                },
            ),
        ];

        // Act & Assert
        assert_eq!(craft_yield_bonus(&entries, forging), 2);
    }

    #[test]
    fn 不同加值类型的制作产出加成相加() {
        // 跨桶相加：天赋给的 +1 与附魔铁砧给的 +1 合起来 +2。
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let innate = index(&mut interner, "lostland:innate");
        let enhancement = index(&mut interner, "lostland:enhancement");
        let mastery = index(&mut interner, "lostland:forging_mastery");
        let anvil = index(&mut interner, "lostland:masterwork_anvil");
        let entries = vec![
            typed_entry(
                mastery,
                innate,
                RuleModifier::CraftYield {
                    category: forging,
                    bonus_product_count: 1,
                },
            ),
            typed_entry(
                anvil,
                enhancement,
                RuleModifier::CraftYield {
                    category: forging,
                    bonus_product_count: 1,
                },
            ),
        ];

        // Act & Assert
        assert_eq!(craft_yield_bonus(&entries, forging), 2);
    }

    #[test]
    fn 制作产出加成的结果与切片顺序无关() {
        // 约束 C5：两条同类型 + 一条另一类型，正序与逆序必须逐位相同。
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let innate = index(&mut interner, "lostland:innate");
        let enhancement = index(&mut interner, "lostland:enhancement");
        let a = index(&mut interner, "lostland:a");
        let b = index(&mut interner, "lostland:b");
        let c = index(&mut interner, "lostland:c");
        let make = |origin, kind, bonus| {
            typed_entry(
                origin,
                kind,
                RuleModifier::CraftYield {
                    category: forging,
                    bonus_product_count: bonus,
                },
            )
        };
        let mut forward = vec![
            make(a, innate, 1),
            make(b, enhancement, 3),
            make(c, enhancement, 2),
        ];

        // Act
        let ordered = craft_yield_bonus(&forward, forging);
        forward.reverse();
        let reversed = craft_yield_bonus(&forward, forging);

        // Assert
        assert_eq!(ordered, 4);
        assert_eq!(ordered, reversed);
    }

    #[test]
    fn 负的制作产出加成合法但产出保底一件() {
        // 项目所有者裁定「允许为负，但产出保底 1 件」——照 `Resistance`
        // 允许「脆弱」的先例。选择器如实返回负数（它只负责聚合），
        // 保底落在 `craft_product_count`。
        // Arrange
        let mut interner = Interner::new();
        let forging = index(&mut interner, "lostland:forging");
        let cursed = index(&mut interner, "lostland:cursed_anvil");
        let entries = vec![entry(
            cursed,
            RuleModifier::CraftYield {
                category: forging,
                bonus_product_count: -5,
            },
        )];

        // Act
        let bonus = craft_yield_bonus(&entries, forging);

        // Assert
        assert_eq!(bonus, -5);
        assert_eq!(craft_product_count(1, bonus), MINIMUM_CRAFT_PRODUCT_COUNT);
        assert_eq!(craft_product_count(8, bonus), 3);
    }

    #[test]
    fn 制作件数保底恰好是一件而不是零件() {
        // 「消耗了材料却什么都没拿到」在机制层面不可能发生——
        // `crafting-system.md` 九节⑤那条玩法裁定的机制化。
        // Act & Assert
        assert_eq!(craft_product_count(1, -1), MINIMUM_CRAFT_PRODUCT_COUNT);
        assert_eq!(
            craft_product_count(1, i32::MIN),
            MINIMUM_CRAFT_PRODUCT_COUNT
        );
        assert_eq!(MINIMUM_CRAFT_PRODUCT_COUNT, 1);
    }

    #[test]
    fn 制作件数在两端极值上都不溢出() {
        // 声明值与加成都是内容作者填的，注册期不禁止极端值——中间量走
        // `i64`，两端各钳一次（ADR 0020：全整数，无浮点无除法）。
        // Act & Assert
        assert_eq!(craft_product_count(u32::MAX, 1), u32::MAX);
        assert_eq!(craft_product_count(u32::MAX, i32::MAX), u32::MAX);
        assert_eq!(
            craft_product_count(0, i32::MIN),
            MINIMUM_CRAFT_PRODUCT_COUNT
        );
        assert_eq!(craft_product_count(7, 0), 7);
    }

    // ===================== 易伤聚合 =====================

    /// 造一条易伤候选，理由同本模块其余测试帮手。
    fn vuln_entry(
        origin: ContentIndex,
        modifier_type: Option<ContentIndex>,
        damage_category: ContentIndex,
        damage_increase: i32,
    ) -> RuleModifierEntry {
        RuleModifierEntry {
            origin,
            modifier_type,
            modifier: RuleModifier::Vulnerability {
                damage_category,
                damage_increase,
            },
        }
    }

    /// 造一条抗性候选——与 [`vuln_entry`] 成对，好让「一抗一怕落在
    /// 同一个桶」那条守门测试读起来是对称的。
    fn res_entry(
        origin: ContentIndex,
        modifier_type: Option<ContentIndex>,
        damage_category: ContentIndex,
        damage_reduction: i32,
    ) -> RuleModifierEntry {
        RuleModifierEntry {
            origin,
            modifier_type,
            modifier: resistance(damage_category, damage_reduction),
        }
    }

    #[test]
    fn 同一个加值类型里的两条易伤取最强() {
        // 桶内规则与抗性逐字相同：不叠加，取最强的那一条。
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let innate = index(&mut interner, "examplemod:innate");
        let first = index(&mut interner, "test:a");
        let second = index(&mut interner, "test:b");
        let modifiers = vec![
            vuln_entry(first, Some(innate), fire, 4),
            vuln_entry(second, Some(innate), fire, 6),
        ];

        // Act & Assert
        assert_eq!(vulnerability_damage_increase(&modifiers, fire), 6);
    }

    #[test]
    fn 不同加值类型的两条易伤相加() {
        // 跨桶规则与抗性逐字相同：相加。
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let innate = index(&mut interner, "examplemod:innate");
        let curse = index(&mut interner, "test:curse");
        let first = index(&mut interner, "test:a");
        let second = index(&mut interner, "test:b");
        let modifiers = vec![
            vuln_entry(first, Some(innate), fire, 4),
            vuln_entry(second, Some(curse), fire, 3),
        ];

        // Act & Assert
        assert_eq!(vulnerability_damage_increase(&modifiers, fire), 7);
    }

    #[test]
    fn 易伤只认自己那个伤害类别() {
        // Arrange
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let acid = index(&mut interner, "examplemod:acid");
        let origin = index(&mut interner, "test:a");
        let modifiers = vec![vuln_entry(origin, None, fire, 4)];

        // Act & Assert
        assert_eq!(vulnerability_damage_increase(&modifiers, fire), 4);
        assert_eq!(vulnerability_damage_increase(&modifiers, acid), 0);
    }

    #[test]
    fn 同一个未分类桶里的减伤不再吞掉易伤() {
        // 本批次修掉的那条真实错误结果的守门测试：负减伤表达脆弱时,
        // 「同类型取最强」会让 `-5` 被同桶的 `+3` 静默吃掉,而**不声明
        // 加值类型的全部落进同一个共享桶**是本模块刻意选的默认值,
        // 因此这不是一个边角情形。完整论证见
        // `RuleModifier::Resistance` 文档「脆弱**不**用负减伤表达」。
        // Arrange：两条都不声明类型，同一个伤害类别，一抗一怕。
        let mut interner = Interner::new();
        let fire = index(&mut interner, "lostland:fire");
        let armor = index(&mut interner, "test:armor");
        let flesh = index(&mut interner, "test:flesh");
        let modifiers = vec![
            res_entry(armor, None, fire, 3),
            vuln_entry(flesh, None, fire, 5),
        ];

        // Act
        let reduction = resistance_damage_reduction(&modifiers, fire);
        let increase = vulnerability_damage_increase(&modifiers, fire);

        // Assert：两条都活着，各自被自己的消费者认领。
        assert_eq!(reduction, 3);
        assert_eq!(increase, 5);
        // 净额：来伤 10 − 3 + 5 = 12。旧模型（负减伤 `-5` 与 `+3` 同桶
        // 取最强）在这里会算出 10 − 3 = 7,脆弱整条消失。
        assert_eq!(damage_after_resistance(10, reduction, increase), 12);
    }

    /// 测试用帮手：把 `Interner` 包成 [`rule_modifier_displays`] 要的
    /// 显示名键回调。
    ///
    /// **这是夹具，不是生产规则。** 真实装载路径里这个回调从内容表里
    /// **读** `display_name_key` 字段（`ll_mod::damage_category::DamageCategoryDef`
    /// 与 `ll_mod::recipe_category::RecipeCategoryDef`，装配点在
    /// `ll_game::app::draw_hud`）。单元测试里没有内容表，于是照
    /// `mods/lostland` 那几条**实际声明**的键的形状现造一条同形的键
    /// ——下面各条断言里的期望值因此与真实内容逐字相同。
    fn name_keys(
        interner: &Interner,
    ) -> impl Fn(SubjectRegistry, ContentIndex) -> Option<NamespacedId> + '_ {
        |registry, index| {
            let id = interner.resolve(index)?;
            let table = match registry {
                SubjectRegistry::DamageCategory => "damage_category",
                SubjectRegistry::RecipeCategory => "recipe_category",
            };
            NamespacedId::parse(&format!(
                "{}:{table}.{}.display_name",
                id.namespace(),
                id.path()
            ))
            .ok()
        }
    }

    /// 测试用帮手：取某一行的某个数值实参。
    fn amount_of(display: &RuleModifierDisplay, name: &str) -> Option<i64> {
        display
            .amounts
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    }

    #[test]
    fn 九个变体每一个都产出一行面板数据() {
        // Arrange：一条修正一个变体，九条全上——这条测试是
        // `display_shape` 那个无通配 match「每个变体都真的能显示」的
        // 机器检查。第十个变体加进来时它不会自动变红（新变体没被这里
        // 构造），但 `display_shape` 会编译不过，那才是第一道防线。
        let mut interner = Interner::new();
        let source = index(&mut interner, "testmod:source");
        let fire = index(&mut interner, "lostland:fire");
        let forging = index(&mut interner, "lostland:forging");
        let inspection = NamespacedId::parse("lostland:inspection").expect("测试用标识符恒合法");
        let critical = NamespacedId::parse("lostland:critical").expect("测试用标识符恒合法");
        let modifiers = vec![
            entry(source, resistance(fire, 3)),
            entry(
                source,
                RuleModifier::Vulnerability {
                    damage_category: fire,
                    damage_increase: 4,
                },
            ),
            entry(source, RuleModifier::RerollOnce { value: 1 }),
            entry(
                source,
                RuleModifier::Advantage {
                    check_context: inspection.clone(),
                },
            ),
            entry(
                source,
                RuleModifier::Disadvantage {
                    check_context: critical,
                },
            ),
            entry(
                source,
                RuleModifier::SneakAttack {
                    sneak_modifier: 9,
                    extra_damage: 15,
                },
            ),
            entry(
                source,
                RuleModifier::InspectionSuspicion {
                    inconspicuous_modifier: 5,
                },
            ),
            entry(
                source,
                RuleModifier::InspectionConcealment {
                    concealment_modifier: 6,
                },
            ),
            entry(
                source,
                RuleModifier::CraftYield {
                    category: forging,
                    bonus_product_count: 1,
                },
            ),
        ];

        // Act
        let displays = rule_modifier_displays(&modifiers, &name_keys(&interner));

        // Assert：九行，且文案键两两不同——文案键就是「哪个变体」的
        // 身份，撞车会让两个变体在面板上合成一行。
        assert_eq!(displays.len(), 9);
        let mut keys: Vec<&str> = displays.iter().map(|display| display.name_key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 9);
    }

    #[test]
    fn 跨加值类型相加的合并值上面板而不是两条原始声明() {
        // Arrange：天赋 +1（无类型桶）与附魔 +1（enhancement 桶）——
        // 结算算出的是 +2，面板要说的也必须是 +2，不是两行 +1。
        let mut interner = Interner::new();
        let trait_source = index(&mut interner, "testmod:mastery");
        let anvil = index(&mut interner, "testmod:anvil");
        let enhancement = index(&mut interner, "testmod:enhancement");
        let forging = index(&mut interner, "lostland:forging");
        let craft = |bonus| RuleModifier::CraftYield {
            category: forging,
            bonus_product_count: bonus,
        };
        let modifiers = vec![
            entry(trait_source, craft(1)),
            typed_entry(anvil, enhancement, craft(1)),
        ];

        // Act
        let displays = rule_modifier_displays(&modifiers, &name_keys(&interner));

        // Assert
        assert_eq!(displays.len(), 1);
        assert_eq!(displays[0].name_key, CRAFT_YIELD_NAME_KEY);
        assert_eq!(amount_of(&displays[0], "amount"), Some(2));
        assert_eq!(displays[0].source_count, 2);
        // 与结算走的是同一条链路，两个数必然相等——这条断言是那句话的
        // 机器检查，不是重复。
        assert_eq!(
            i64::from(craft_yield_bonus(&modifiers, forging)),
            amount_of(&displays[0], "amount").expect("这一行恒有 amount 实参")
        );
    }

    #[test]
    fn 同一加值类型取最强时来源计数仍然数出两条() {
        // Arrange：两枚同款护符，同属 gear 桶，各 +1——实际只生效 +1。
        // 纯合并值会让玩家以为第二枚坏了；来源计数把「有两条声明」这
        // 件事显式说出来，见 `RuleModifierDisplay` 文档。
        let mut interner = Interner::new();
        let left = index(&mut interner, "testmod:amulet_a");
        let right = index(&mut interner, "testmod:amulet_b");
        let gear = index(&mut interner, "lostland:gear");
        let fire = index(&mut interner, "lostland:fire");
        let modifiers = vec![
            typed_entry(left, gear, resistance(fire, 1)),
            typed_entry(right, gear, resistance(fire, 1)),
        ];

        // Act
        let displays = rule_modifier_displays(&modifiers, &name_keys(&interner));

        // Assert
        assert_eq!(displays.len(), 1);
        assert_eq!(amount_of(&displays[0], "amount"), Some(1));
        assert_eq!(displays[0].source_count, 2);
    }

    #[test]
    fn 主语键与内容表自己声明的显示名键逐字相同() {
        // Arrange：`mods/lostland/crafting.json5` 里锻造那条写的是
        // `display_name_key: "lostland:recipe_category.forging.display_name"`，
        // `mods/lostland/damage_categories.json5` 里火那条写的是
        // `"lostland:damage_category.fire.display_name"`——面板拿到的必须
        // 就是这两条，否则查的是另一条不存在的键，玩家看到键名本身。
        let mut interner = Interner::new();
        let source = index(&mut interner, "testmod:source");
        let forging = index(&mut interner, "lostland:forging");
        let fire = index(&mut interner, "lostland:fire");
        let modifiers = vec![
            entry(
                source,
                RuleModifier::CraftYield {
                    category: forging,
                    bonus_product_count: 1,
                },
            ),
            entry(source, resistance(fire, 2)),
        ];

        // Act
        let displays = rule_modifier_displays(&modifiers, &name_keys(&interner));

        // Assert
        let craft = displays
            .iter()
            .find(|display| display.name_key == CRAFT_YIELD_NAME_KEY)
            .expect("制作产出那一行必然在");
        assert_eq!(
            craft.subject_key.as_deref(),
            Some("lostland:recipe_category.forging.display_name")
        );
        let resist = displays
            .iter()
            .find(|display| display.name_key == RESISTANCE_NAME_KEY)
            .expect("抗性那一行必然在");
        assert_eq!(
            resist.subject_key.as_deref(),
            Some("lostland:damage_category.fire.display_name")
        );
    }

    #[test]
    fn 优势有主语但没有数值实参() {
        // Arrange：优劣势只有「有没有」，`amounts` 恒空——呈现层因此
        // 一个数都不会往那条消息里填。
        let mut interner = Interner::new();
        let source = index(&mut interner, "testmod:source");
        let inspection = NamespacedId::parse("lostland:inspection").expect("测试用标识符恒合法");
        let modifiers = vec![entry(
            source,
            RuleModifier::Advantage {
                check_context: inspection,
            },
        )];

        // Act
        let displays = rule_modifier_displays(&modifiers, &name_keys(&interner));

        // Assert
        assert_eq!(displays.len(), 1);
        assert_eq!(displays[0].name_key, ADVANTAGE_NAME_KEY);
        assert!(displays[0].amounts.is_empty());
        assert_eq!(
            displays[0].subject_key.as_deref(),
            Some("lostland:check_context.inspection.display_name")
        );
    }

    #[test]
    fn 偷袭两个数各自跨类型相加与结算一致() {
        // Arrange
        let mut interner = Interner::new();
        let trait_source = index(&mut interner, "testmod:instinct");
        let dagger = index(&mut interner, "testmod:dagger");
        let gear = index(&mut interner, "lostland:gear");
        let modifiers = vec![
            entry(
                trait_source,
                RuleModifier::SneakAttack {
                    sneak_modifier: 9,
                    extra_damage: 15,
                },
            ),
            typed_entry(
                dagger,
                gear,
                RuleModifier::SneakAttack {
                    sneak_modifier: 2,
                    extra_damage: 3,
                },
            ),
        ];

        // Act
        let displays = rule_modifier_displays(&modifiers, &name_keys(&interner));
        let rule = sneak_attack_rule(&modifiers).expect("有声明就有规则");

        // Assert
        assert_eq!(displays.len(), 1);
        assert_eq!(amount_of(&displays[0], "amount"), Some(11));
        assert_eq!(amount_of(&displays[0], "extra"), Some(18));
        assert_eq!(i64::from(rule.sneak_modifier), 11);
        assert_eq!(i64::from(rule.extra_damage), 18);
    }

    #[test]
    fn 同一个变体的不同主语分成两行() {
        // Arrange：火抗与物理抗是两件事，不该合成一行。
        let mut interner = Interner::new();
        let source = index(&mut interner, "testmod:source");
        let fire = index(&mut interner, "lostland:fire");
        let physical = index(&mut interner, "lostland:physical");
        let modifiers = vec![
            entry(source, resistance(fire, 3)),
            entry(source, resistance(physical, 2)),
        ];

        // Act
        let displays = rule_modifier_displays(&modifiers, &name_keys(&interner));

        // Assert
        assert_eq!(displays.len(), 2);
        for display in &displays {
            assert_eq!(display.source_count, 1);
        }
    }

    #[test]
    fn 行序不随声明先后改变() {
        // Arrange：同一套修正，两种获得顺序——面板行序必须一样，见
        // `rule_modifier_displays` 文档「行的顺序是确定的」。
        let mut interner = Interner::new();
        let source = index(&mut interner, "testmod:source");
        let fire = index(&mut interner, "lostland:fire");
        let forging = index(&mut interner, "lostland:forging");
        let craft = RuleModifier::CraftYield {
            category: forging,
            bonus_product_count: 1,
        };
        let forward = vec![
            entry(source, resistance(fire, 3)),
            entry(source, craft.clone()),
        ];
        let backward = vec![entry(source, craft), entry(source, resistance(fire, 3))];

        // Act
        let left = rule_modifier_displays(&forward, &name_keys(&interner));
        let right = rule_modifier_displays(&backward, &name_keys(&interner));

        // Assert
        assert_eq!(left, right);
    }

    #[test]
    fn 主语索引查不到标识符时整行跳过() {
        // Arrange：模拟一个还原不出标识符的索引——装载期本不该放过，
        // 真出现时面板宁可少一行，也不显示一个半截的主语。
        let mut interner = Interner::new();
        let source = index(&mut interner, "testmod:source");
        let fire = index(&mut interner, "lostland:fire");
        let orphan = index(&mut interner, "testmod:orphan");
        let modifiers = vec![
            entry(source, resistance(fire, 3)),
            entry(source, resistance(orphan, 99)),
        ];

        // Act：回调对 `orphan` 交白卷（模拟「这条索引在内容表里查不到
        // 定义」），其余照常。
        let lookup = name_keys(&interner);
        let displays = rule_modifier_displays(&modifiers, &|registry, index| {
            if index == orphan {
                None
            } else {
                lookup(registry, index)
            }
        });

        // Assert
        assert_eq!(displays.len(), 1);
        assert_eq!(
            displays[0].subject_key.as_deref(),
            Some("lostland:damage_category.fire.display_name")
        );
    }

    #[test]
    fn 主语键完全由内容决定不受任何拼键约定约束() {
        // Arrange：本仓库本体的键碰巧都长成
        // `命名空间:注册表.路径.display_name`，那只是本体的写法。本测试
        // 的回调返回一条**刻意不按那个形状**的键——这正是「读字段」与
        // 「按约定现拼」的可观测差别：旧实现会从 `yourmod:acid` 拼出
        // `yourmod:damage_category.acid.display_name`，与内容作者真正
        // 声明的键对不上，面板于是显示键名本身。
        let mut interner = Interner::new();
        let source = index(&mut interner, "yourmod:vial");
        let acid = index(&mut interner, "yourmod:acid");
        let declared = "yourmod:acid_is_called_this";
        let modifiers = vec![entry(source, resistance(acid, 4))];

        // Act
        let displays = rule_modifier_displays(&modifiers, &|registry, index| {
            assert_eq!(registry, SubjectRegistry::DamageCategory);
            assert_eq!(index, acid);
            Some(NamespacedId::parse(declared).expect("测试用标识符恒合法"))
        });

        // Assert
        assert_eq!(displays.len(), 1);
        assert_eq!(displays[0].subject_key.as_deref(), Some(declared));
    }

    #[test]
    fn 判定种类没有内容表因此仍然按约定拼键() {
        // Arrange：优势/劣势的 `check_context` 是引擎侧的开放标识符
        // （`crate::check::CheckContext`），内容作者可以写任何标识符，
        // 没有一张表能读——这是本模块仅剩的一处拼键，见 `subject_key`
        // 文档。回调在这条路径上**一次都不会被调用**。
        let mut interner = Interner::new();
        let source = index(&mut interner, "testmod:cloak");
        let modifiers = vec![entry(
            source,
            RuleModifier::Advantage {
                check_context: NamespacedId::parse("yourmod:haggling").expect("测试用标识符恒合法"),
            },
        )];

        // Act
        let displays = rule_modifier_displays(&modifiers, &|_registry, _index| {
            panic!("判定种类不查内容表")
        });

        // Assert
        assert_eq!(displays.len(), 1);
        assert_eq!(
            displays[0].subject_key.as_deref(),
            Some("yourmod:check_context.haggling.display_name")
        );
    }

    #[test]
    fn 一条修正都没有时一行都不产出() {
        // Arrange
        let interner = Interner::new();

        // Act
        let displays = rule_modifier_displays(&[], &name_keys(&interner));

        // Assert：空表——「无」那一行是呈现层的事，不是本层的事。
        assert!(displays.is_empty());
    }
}
