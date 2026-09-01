//! 值哈希：把 [`crate::registry::Registry`] 的内容哈希从「只追踪 id
//! 集合」升级为「同时追踪字段值」。
//!
//! # 起因：id 集合相同不代表内容相同
//!
//! [`Registry::intern`](crate::registry::Registry::intern) 只在注册那
//! 一刻——也就是某条内容的字段值还没有被任何 `*Table::define` 写入之
//! 前——折入一次「这个命名空间贡献了这个 id」的摘要。一个 mod 版本号
//! 不变、但把某个技能的伤害从 50 改成 500，`intern` 阶段的哈希完全
//! 看不出来：id 集合一个字符没变。存档硬门禁
//! （`ll_content::load_error::check_mod_content`，该 crate 依赖本
//! crate，方向不能反过来，本文档因此只能点名、不能用 intra-doc link
//! 指过去）与内容哈希本身都会判定为"兼容"，但两次装载出来的其实是
//! 不同的世界。
//!
//! [ADR 0027](../../../../knowledge/decisions/0027-content-hash-covers-field-values.md)
//! 落地本模块时只覆盖了当时存在的六张内容表（地形/职业/技能/副职/
//! 任务/种族），并在「后果（技术债与后续）」一节明确记录了局限：
//! 「未来任何新增的第七张内容表，都需要同步在 `ll_mod::content_hash`
//! 里补一个 `write_*_fields` 分支……这条检验目前没有编译期强制手段」。
//! 本次批次（内容值哈希覆盖面扩展）核实了这条局限确实已经发生——
//! 天赋/资源池/物品/动画剪辑/空间层属性/经验曲线六张表此前全部没有
//! 参与值哈希，`RaceAttrs.xp_reward`/`traits` 两个字段（等级经验系统/
//! 天赋系统批次新增）也从未被 [`write_race_fields`] 覆盖过——补齐这些
//! 缺口，并把「未来新增内容表忘记接入」从「没有编译期强制手段」缩小成
//! 「编译期挡住一部分遗漏 + 测试期挡住剩余遗漏」两层防线，见
//! [`classify_index`] 文档「编译期强制」一节与 `ll_game::content` 模块
//! 的覆盖率回归测试。
//!
//! # 为什么不能在 `Registry::intern` 内部做
//!
//! `intern` 只知道字符串 id，不知道、也不应该知道各张内容表
//! （[`crate::class::ClassTable`]/[`crate::skill::SkillTable`]/……）的
//! 具体形状——这些类型分散在 `ll-mod`/`ll-world` 两个 crate，`Registry`
//! 本身是一个通用的字符串 ↔ 索引映射池，不应该反过来认识每一种具体
//! 内容的字段布局（那会让 `Registry` 与全部内容表的字段变化互相耦合）。
//! 更根本的问题是时序：`intern` 调用发生在 `*Table::define` **之前**
//! （先拿到索引，再用索引登记属性），那一刻字段值压根还不存在。
//!
//! 因此值哈希被设计成装载完成后的一次性收尾步骤：
//! [`apply_value_hashes`] 拿到 `&mut Registry` 与全部内容表的只读引用
//! （[`ContentValueTables`]），对 `registry.snapshot()` 里的每一个 id
//! 求一次字段值摘要，再用
//! [`Registry::fold_content_digest`](crate::registry::Registry::fold_content_digest)
//! 把它异或折叠进 `intern` 阶段已经贡献的 id 摘要之上——两次折叠互不
//! 替换、只是叠加，最终的 `content_hash_of` 结果同时覆盖「id 集合变了」
//! 与「字段值变了」两类变化。生产装载路径
//! （`ll_game::content::load_content`）在全部内容注册完毕、返回
//! `LoadedContent` 之前调用它恰好一次。
//!
//! # 哈希覆盖哪些字段：判据是「完整性优先，不做取舍」
//!
//! 每张表 `write_*_fields` 函数把该表 `*Attrs`/`*Def` 声明的**全部**
//! 字段都混入摘要，不排除任何一个——包括
//! `display_name_key`（指向 Fluent 本地化键的标识符，不是渲染出来的
//! 文本本身）、[`crate::clip::ClipTable`] 的帧序列这类"看起来只影响
//! 展示"的字段。
//!
//! 这不是没想清楚就把所有字段一股脑塞进去：ADR 0027 要修的问题恰恰是
//! "旧哈希因为只看 id 集合而漏掉了真实的内容变化"，若在这里又主观排除
//! 一部分字段或整张表（哪怕理由听起来合理，比如"这只是个显示名，不影响
//! 数值"，或"动画只是表现层，不影响模拟"），就是在换一种方式重新制造
//! 同一类盲区——排除的判据一旦掺入"这个字段/这张表看起来不重要"这类
//! 主观判断，就没有一条能站得住脚、不会被下一次新增打破的分界线。
//! 因此本模块采用「凡是内容表 `*Attrs`/`*Def` 里声明的字段，一律参与
//! 哈希」这一条不需要逐字段拍脑袋的规则，本次新纳入覆盖的六张表同样
//! 遵守它，不因为它们「不参与模拟结算」（动画剪辑、经验曲线）就降格
//! 处理。
//!
//! **例外，且是刻意的例外**：[`crate::xp_curve::XpCurveBindings`]
//! （职业/种族 → 经验曲线的绑定）与 [`crate::race::RaceTable`] 已经
//! 覆盖的「种族授予天赋」不同——后者是 `RaceAttrs` 自身声明的字段
//! （`traits: Vec<TraitGrant>`），前者是一张完全独立的映射表，不为
//! 自己的绑定关系分配 `ContentIndex`，因此不落在
//! [`Registry::snapshot`](crate::registry::Registry::snapshot) 遍历
//! 到的任何一个 id 上——[`classify_index`]/[`entry_value_digest`] 的
//! 「按 id 分派到某张表」这套机制天然覆盖不到它，需要改成「职业/种族
//! 自己的哈希函数里再多查一张绑定表」这类结构性不同的扩展，属于比
//! 本批次「新增表 + 补齐已覆盖表的漏字段」更大的一次改动，本批次不做，
//! 如实记录为已知缺口而非遗漏。
//!
//! # `ContentIndex` 字段：解析成 `NamespacedId` 字符串再混入
//!
//! `SkillDef.owning_class`/`prerequisites`、`ClassDef.traits[].trait_id`、
//! `QuestNodeDef.prerequisites`/`QuestCondition::KillCount.target_kind`、
//! `TerrainDef.opens_into`、`RaceDef.traits[].trait_id`、
//! `TraitDef.granted_skills`/`RuleModifier::Resistance.damage_category`/
//! `ResourcePoolGrant.pool` 这类字段的类型是 `ContentIndex`（或
//! `Option<ContentIndex>`/`Vec<ContentIndex>`）——但 `ContentIndex` 的
//! 具体数值由**注册顺序**决定（`ll_core::ident` 模块文档：「不可
//! 持久化——索引依赖 mod 加载顺序」）。若直接把裸索引数值混入哈希，
//! "同样的内容、只是 mod 装载顺序不同"会被误判成"内容变了"——两次
//! 装载里同一个字符串 id 分到的整数索引很可能不同。
//!
//! 本模块因此从不直接哈希 `ContentIndex` 的数值：任何出现
//! `ContentIndex` 的字段，都先用 [`Registry::resolve`](crate::registry::Registry::resolve)
//! 把它换回 `NamespacedId` 字符串,再用
//! [`StateHasher::write_namespaced_id`] 混入——这与存档头
//! （`ll_content::header::SaveHeader::content_index_map`）为什么必须
//! 存字符串而不是索引是同一条道理的另一处落点。解析失败（拿到的
//! `ContentIndex` 在当前 `registry` 里查不到对应字符串）理论上不应该
//! 发生——本模块的调用前提是 `registry`/`tables` 出自同一次装载会话,
//! 任何被表里字段引用到的索引都应该在同一个 `registry` 里能反查回来。
//! 真的撞见这种情形时,[`write_optional_resolved`] 不会 panic，而是写
//! 入一个与"有效解析"/"None"都不同的第三判别值——与
//! `Registry::intern` 模块文档「不是安全或存档完整性校验」同一条
//! 立场：值哈希只是一个尽力而为的变化探测器,不应该因为这条理论上的
//! 边界情形让整个装载流程 panic。
//!
//! `RuleModifier::Advantage`/`Disadvantage.check_context` 与
//! `SpaceProfile.reverb_tag` 例外：它们的类型是 `NamespacedId` 本身
//! （一个开放标识符，不是 `ContentIndex`），不经过 `Registry::resolve`，
//! 直接 [`StateHasher::write_namespaced_id`] 混入——与
//! `SkillDef`/`RaceDef` 的 `display_name_key` 同一条既有处理方式。
//!
//! `XpCurveDef.id` 是唯一的例外中的例外：它的类型虽是 `ContentIndex`，
//! 但语义是"这条曲线自己的索引"（`register_base_xp_curve` 恒填
//! `id: index`，即定义它自己的那个索引），本模块因此不重复哈希它——
//! [`entry_value_digest`] 顶部已经对同一个 `id` 调用过
//! [`StateHasher::write_namespaced_id`]，再哈希一次纯粹是同一份信息
//! 的冗余重复，不是遗漏；与其余表的 `*Attrs` 类型统一不含 `id` 字段
//! （只有 `*Def` 才带,`define` 实际接受的是 `Attrs`）是同一条既有
//! 纪律的另一种落点——只是 `XpCurveDef` 没有独立的 `Attrs` 类型
//! （`XpCurveTable::define` 直接接受完整 `XpCurveDef`），所以这里改为
//! 手动跳过这一个字段，而不是像其余表那样在类型层面就没有它。
//!
//! # 浮点字段：当前全部内容表里仍不存在，未来新增前必须先处理
//!
//! 项目世界状态本身禁止浮点（`ll_world::entity::stats` 模块文档「全部
//! 整数」），本次批次核实过新纳入覆盖的六张表
//! （[`crate::trait_def::TraitDef`]/[`crate::resource_pool::ResourcePoolDef`]/
//! [`crate::item::ItemDef`]/`ll_render::anim::Clip`/
//! `ll_world::space_profile::SpaceProfile`/`ll_sim::xp_curve::XpCurveDef`，
//! 含它们引用的 `ll_sim::item::{SlotMask, StatBonus, StatTarget}`/
//! `ll_sim::resource_pool::{CapacityFormula, CapacityValue, RegenRule,
//! ResourcePoolShape, RestRecoveryAmount}`/`ll_sim::combat::Penetration`/
//! `ll_sim::xp_curve::{XpCurveOp, XpCurveOperand, XpCurveCond}`）与
//! ADR 0027 原先核实过的六张表一样，**没有任何 `f32`/`f64` 字段**——
//! `ll_core::scaled::Milli` 看似是「小数」（重量/价格），实际是
//! `pub struct Milli(pub i64)` 定点整数，本模块按普通 `i64` 处理。
//! `StateHasher` 目前也确实没有 `write_f64` 这类方法，这条备忘依旧
//! 只是预防性设计，留给真的引入浮点字段的那次改动去处理规范化
//! （`NaN` 位模式不唯一、`+0.0`/`-0.0` 位不同但数值相等）。

use ll_core::hashing::StateHasher;
use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::item::{SlotMask, StatBonus, StatTarget};
use ll_sim::resource_pool::{
    CapacityFormula, CapacityValue, RegenRule, ResourcePoolShape, RestRecoveryAmount,
};
use ll_sim::skill::{ResourceCost, SkillEffect};
use ll_sim::xp_curve::{XpCurveCond, XpCurveOp, XpCurveOperand};
use ll_world::culture::{CultureKind, CultureTable};
use ll_world::entity::{AffiliationKind, BaseStats};
use ll_world::resource::{ResourceKind, ResourceTable};
use ll_world::space_profile::SpaceProfileTable;
use ll_world::terrain::{TerrainKind, TerrainTable};
use ll_world::weather::WeatherTable;

use crate::class::ClassTable;
use crate::clip::ClipTable;
use crate::damage_category::DamageCategoryTable;
use crate::dialogue::{
    AffiliationQuery, DialogueCondition, DialogueNext, DialogueNodeTable, DialogueTable,
};
use crate::formula::FormulaTable;
use crate::item::ItemTable;
use crate::modifier_type::ModifierTypeTable;
use crate::quest::{QuestCondition, QuestTable};
use crate::race::RaceTable;
use crate::recipe::RecipeTable;
use crate::recipe_category::RecipeCategoryTable;
use crate::registry::Registry;
use crate::resource_pool::ResourcePoolTable;
use crate::skill::SkillTable;
use crate::subclass::SubclassTable;
use crate::tag::TagTable;
use crate::trait_def::{RuleModifier, TraitTable, TypedRuleModifier};
use crate::weapon_category::WeaponCategoryTable;
use crate::xp_curve::XpCurveTable;
use ll_sim::formula::{FormulaCond, FormulaOp, FormulaOperand};

/// 内容哈希算法的版本号——每当"同一份内容在旧代码与新代码下会算出不同
/// 的摘要"就必须递增。
///
/// # 修正一条被推翻的旧判断
///
/// 本字段引入时的文档曾断言「新增一张内容表、给已有内容表新增一个此前
/// 从未参与哈希的字段，都不算——那只是让哈希覆盖得更全，同一份内容在
/// 新旧代码下若字段集合没变，摘要仍然相同；真正需要递增的是……字节
/// 编码方式变了」。**本次批次核实这条判断是错的**：`StateHasher` 是
/// 顺序敏感的增量哈希器（`ll_core::hashing` 模块文档），额外调用一次
/// `write_*`——哪怕写入的是默认值——都会改变最终摘要的位模式，不存在
/// "字段集合没变、摘要就不变"这回事；`0`→`1` 那次升级本身就是一次纯粹
/// 的覆盖面扩张（从只追踪 id 集合扩到追踪字段值），却也确实递增了版本
/// 号，与那条旧判断的结论自相矛盾。本次批次新增六张表的值哈希覆盖 +
/// 补齐 `RaceDef.xp_reward`/`traits` 两个此前漏哈希的字段，因此把版本
/// 号从 `1` 递增到 `2`——不能援引已被推翻的旧判断按兵不动：若不递增，
/// 任何在这次改动之前写出、`generation_mods` 携带非空 `content_hash`
/// 的存档，读档时会在 `check_mod_content` 一步被误判成
/// `ModContentMismatch`（"你的 mod 内容变了"），而真相是"我们的哈希
/// 覆盖面变宽了"——两者在诊断意义上完全不同，`ContentHashAlgorithmUpgraded`
/// 正是为区分这两种情形而存在，见下方「为什么需要这个字段」一节。
///
/// 存档头（`ll_content::header::SaveHeader::content_hash_algorithm_version`，
/// 该 crate 依赖本 crate,方向不能反过来,本文档因此只能点名、不能用
/// intra-doc link 指过去）记录写出时的算法版本；读档时若与这里不
/// 一致，`ll_content::load_error::check_content_hash_algorithm` 判定为
/// `LoadError::ContentHashAlgorithmUpgraded`——与"mod 内容真的变了"
/// （`LoadError::ModContentMismatch`）在诊断上明确区分,不让玩家/mod
/// 作者误以为自己的 mod 坏了,见
/// `ll_content::header::SaveHeader::content_hash_algorithm_version`
/// 文档「为什么需要这个字段」一节。
///
/// **未来的准则**：任何会让"同一份内容,新旧代码算出不同摘要"的改动都
/// 要递增这个版本号——新增内容表、给已有表新增字段（无论是"此前完全
/// 没被哈希过"还是"字节编码方式本身变了"）全部包含在内；唯一不需要
/// 递增的是纯粹不改变任何已产出摘要的改动（例如本文件内部重构函数
/// 拆分，但不改变任何现有分支实际写入的字节序列——[`entry_value_digest`]
/// 把「表判别字节」的写入位置从各分支内部提到统一的一处，就是这样一次
/// 不改变字节序列的重构，六张原有表的判别值编号也原样保留，理由见
/// [`ContentTableKind`] 文档）。
///
/// 版本 3（幸运并入 `AttributeKind` 批次）：[`write_base_stats`] 新增
/// 混入 `stats.luck` 这一项，改变了每一条携带 `BaseStats` 的内容
/// （目前只有 `RaceDef.stat_modifiers`）的哈希字节序列——即便某条
/// 具体种族定义的六项主属性与幸运数值完全没变，量尺本身也已经不同。
/// 升版号让读档流程能正确识别这是「量尺换了」而不是「mod 内容真的
/// 变了」——`ll-content` 依赖本 crate（依赖方向不允许反过来），下游
/// `ll_content::load_error::LoadError::ContentHashAlgorithmUpgraded` 正是
/// 消费本常量升级信号的分支，这里只能用反引号纯文本指向，不能用
/// intra-doc link。
///
/// 版本 4（伤害公式引擎批次）：两处独立的字节序列变化叠在一起——
/// （一）新增第十三张内容表 [`ContentTableKind::Formula`]；（二）
/// [`write_item_fields`] 新增混入 `ItemDef.damage_formula`，这正是
/// 本模块文档「起因」一节点名的"老表新增字段也会漏"在本批次的真实
/// 复现——上一批（内容值哈希覆盖面扩展）修的是"过去几个批次遗留的
/// 漏哈希字段"，这一条是"新批次自己新增字段时,同一批次内当场补上"，
/// 两者都要求递增版本号，理由相同：任何在本次改动之前写出、
/// `generation_mods` 携带非空 `content_hash` 的存档，读档时都会在
/// `check_mod_content` 一步被误判成 `ModContentMismatch`。
///
/// 版本 5（伤害类别/抗性接线批次）：三处独立的字节序列变化叠在一起——
/// （一）新增第十四张内容表 [`ContentTableKind::WeaponCategory`]；
/// （二）新增第十五张内容表 [`ContentTableKind::DamageCategory`]；
/// （三）[`write_item_fields`] 新增混入 `ItemDef.damage_category`——与
/// 版本 4 「老表新增字段也会漏」同一处真实复现，理由同上一段。
///
/// 版本 6（盗贼偷袭接线批次）：[`write_rule_modifier`] 新增
/// [`RuleModifier::SneakAttack`] 判别值 4——不新增内容表（`RuleModifier`
/// 仍然挂在既有的 [`ContentTableKind::Trait`] 下），但它是一个新的枚举
/// 变体，会被现有的 `TraitDef.rule_modifiers` 字段表达出来。审慎起见
/// 与版本 5 的（一）（二）两处同一条准则对齐：`RuleModifier` 是本模块
/// 「表判别字节」同一套机制在枚举层面的复用（`write_rule_modifier` 顶部
/// 先写一个判别值再写变体自己的字段），新增判别值即使不改变"从未使用
/// 过这个变体的既有内容"的哈希输出，仍然按本模块一贯的保守纪律递增
/// 版本号，不去论证"这次改动对存量内容真的无害"这件事本身要不要成为
/// 免于升版号的理由——那条论证本身也可能出错，递增版本号的代价（读档
/// 分支多判一次 `ContentHashAlgorithmUpgraded`）远低于论证出错的代价
/// （真的漏判成 `ModContentMismatch`）。
/// 版本 7（职业授予天赋接线批次）：[`write_class_fields`] 新增混入
/// `ClassDef.traits`——先写条数再逐条写 `(trait_id, unlock_level)`，与
/// [`write_race_fields`] 的同名字段逐字节同构。这是版本 4「老表新增
/// 字段也会漏」在本批次的又一次复现，但复现的方式更隐蔽：字段与哈希
/// 覆盖确实在同一批次里一起补上了，**漏掉的是本常量自己**——提交
/// `5f6bae5` 的信息里白纸黑字写着「6 → 7」，代码里却仍是 6，直到本次
/// 补齐。当时没有立刻暴露，是因为职业表那时还没接进生产装载路径
/// （`ll_game::content::load_content` 给的是空 `ClassTable`），
/// [`write_class_fields`] 对本体内容一次都没被调用过；本体内容迁往
/// mod 脚本之后这条路径迟早走通，届时「量尺换了」会被误判成
/// `ModContentMismatch`。教训是：提交信息声称改了什么，不等于代码里
/// 真的改了——值哈希这类「错了要很久以后才发作」的机制，声称与事实
/// 之间需要机器来对齐，见 `scripts/ci/check_field_consumers.py` 的
/// `check_content_hash_gate_cross_coverage`。
///
/// 版本 8（天气系统批次）：新增第十七张内容表
/// [`ContentTableKind::Weather`]（判别值 16）——[`ContentValueTables`]
/// 多了一个 `weather` 字段，[`entry_value_digest`] 多了一条
/// [`write_weather_fields`] 分支。这是版本 4/5「新增内容表」那一类，
/// 不是「老表新增字段」：本批次没有给任何一张既有表增删字段，既有
/// 十六张表写入的字节序列逐字节不变。**但版本号仍然必须递增**——
/// `apply_value_hashes` 现在会为天气这一类 id 折进一份此前根本不存在
/// 的字段值摘要，任何在本次改动之前写出、`generation_mods` 携带非空
/// `content_hash` 的存档，读档时都会在 `check_mod_content` 一步被误判
/// 成 `ModContentMismatch`。
///
/// 本次也刻意与版本 7 那次事故对照：那一次提交信息里白纸黑字写着
/// 「6 → 7」，代码里却仍是 6，两批之后才被 `a017647` 补上。本次的
/// 递增由 `scripts/ci/check_field_consumers.py` 的
/// `check_content_hash_gate_cross_coverage` 与
/// `crates/ll-game/src/content.rs` 的覆盖率回归测试同时把守，声称与
/// 事实之间有机器对齐，不再只靠提交信息。
///
/// 版本 9（抗性多来源聚合批次）：[`write_item_fields`] 新增混入
/// `ItemDef.rule_modifiers`——先写条数，再对每条复用既有的
/// [`write_rule_modifier`]（与 [`write_trait_fields`] 混入
/// `TraitDef.rule_modifiers` 逐字节同构，同一个函数、同一套判别值）。
/// 这是版本 4/5（三）/7「老表新增字段也会漏」这一类，不是「新增内容
/// 表」：[`ContentTableKind`] 的十七个变体一个未变，[`ContentValueTables`]
/// 的字段一个未加，因此 `check_content_hash_gate_cross_coverage` 那条
/// 互校在本批次无事可做——**它只守「新增了表」这一类，守不住「老表
/// 新增字段」**，本批次是那条互校覆盖面之外的情形，靠的是本段文字与
/// `crates/ll-mod/src/content_audit.rs` 里同批次新增的
/// `ItemAttrs::rule_modifiers` 花名册条目（字段覆盖率门禁会要求真实
/// 内容覆盖它）两处一起把守。
///
/// 递增的实质理由：任何已经注册过物品的存档，其
/// `apply_value_hashes` 折进的物品条目摘要都会因为多出「规则修正条数
/// （0）」这一个 `u64` 而改变——即使没有任何物品声明规则修正，长度
/// 前缀本身也是新的哈希输入，量尺确实换了。
/// 版本 10（温度系统批次）：两处「老表新增哈希输入」，同属版本
/// 4/5（三）/7/9 那一类，不是「新增内容表」——[`ContentTableKind`] 的
/// 十七个变体一个未变，[`ContentValueTables`] 的字段一个未加，
/// `check_content_hash_gate_cross_coverage` 那条互校因此在本批次同样
/// 无事可做（它只守「新增了表」）。
///
/// 1. [`write_weather_fields`] 新增混入 `WeatherDef.temperature_offset`
///    （天气对温度的增量偏移，十分之一摄氏度）。任何已经注册过天气的
///    存档，其天气条目摘要都会因为多出这一个 `i64` 而改变——本体六种
///    天气里有五种的偏移非零，量尺确实换了。
/// 2. [`write_stat_bonus`] 新增第三个判别值 `2`
///    （[`StatTarget::Insulation`]）。这一条**只在真的有物品声明绝缘值
///    时**改变摘要（既有的 `Attribute`/`Armor` 两个判别值逐字节不变），
///    但它是一个新的可达哈希输入，与上一条同批次落地，一并计入本次
///    递增。
///
/// 守门方式与版本 9 相同：本段文字 + `content_audit` 里同批次新增的
/// `WeatherAttrs::temperature_offset` 花名册条目（字段覆盖率门禁会要求
/// 真实内容覆盖它）。
///
/// 版本 11（制作系统批次）：新增**两张**内容表——
/// [`ContentTableKind::Recipe`]（判别值 17）与
/// [`ContentTableKind::RecipeCategory`]（判别值 18）。
/// [`ContentValueTables`] 多了 `recipe`/`recipe_category` 两个字段，
/// [`entry_value_digest`] 多了 [`write_recipe_fields`]/
/// [`write_recipe_category_fields`] 两条分支。这是版本 4/5/8「新增
/// 内容表」那一类，不是「老表新增字段」：本批次没有给任何一张既有表
/// 增删字段，既有十七张表写入的字节序列逐字节不变。
///
/// **但版本号仍然必须递增**，理由与版本 8 完全相同：
/// [`apply_value_hashes`] 现在会为配方与配方类别这两类 id 折进一份此前
/// 根本不存在的字段值摘要，任何在本次改动之前写出、`generation_mods`
/// 携带非空 `content_hash` 的存档，读档时都会在 `check_mod_content`
/// 一步被误判成 `ModContentMismatch`。
///
/// 本批次是 `scripts/ci/check_field_consumers.py` 的
/// `check_content_hash_gate_cross_coverage` **真的有事可做**的一次：
/// 它只守「新增了表」这一类，[`ContentTableKind`] 一多出变体而
/// `TARGET_TYPES` 没跟上，CI 立刻变红——含义是新增哈希变体与新增门禁
/// 条目必须在同一个提交里，不能分批。
///
/// 版本 12（副职获得机制批次）：**老表新增字段**，不是新增表——
/// [`crate::subclass::SubclassTable`] 多了一列「制作计数获得条件」
/// （`register-subclass-unlock` 的写入目标），[`write_subclass_fields`]
/// 因此从「只混入 `display_name_key`」变成「再混入这一列」。既有十九
/// 张表的其余部分写入的字节序列逐字节不变，但**副职这一张表的每一条
/// 目的摘要都变了**（即便它没有声明任何获得条件——`None` 也要写一个
/// 判别字节，否则「没声明」与「声明了但恰好把三个字段写成零」会撞在
/// 一起）。
///
/// 递增的理由与版本 10（`WeatherAttrs::temperature_offset`）完全相同：
/// [`apply_value_hashes`] 现在会为每一个副职 id 折进一份此前不存在的
/// 字段值，任何在本次改动之前写出、`generation_mods` 携带非空
/// `content_hash` 的存档，读档时都会在 `check_mod_content` 一步被误判成
/// `ModContentMismatch`——版本号就是让那份存档拿到「算法换了」这条
/// 准确诊断、而不是「你的 mod 内容被改过了」这条错误诊断的东西。
///
/// **注册新的内容条目本身不需要动这个常量**：本批次同时往
/// `mods/lostland/` 里注册了四个本体副职与四个配方类别，那是内容，
/// 不是新的哈希输入。真正逼着版本号动的只有上面那一列新字段。
///
/// 版本 13（耐久标签批次）：**同时新增一张表与一列老表字段**，两条
/// 各自都足以逼着版本号动——
///
/// - 新表 [`crate::tag::TagTable`]（`register-tag` 的写入目标，
///   [`ContentTableKind::Tag`]），[`write_tag_fields`] 混入每条标签
///   声明的耐久磨损通道位；
/// - 老表新增字段 `ItemDef.tags`（`register-item-tag` 的写入目标），
///   [`write_item_fields`] 因此多混入一段「标签条数 + 逐条解析回
///   `NamespacedId` 字符串」。
///
/// 后者让**每一件物品**的条目摘要都变了（即便它一个标签都没带——
/// 长度前缀 `0` 也是一段此前不存在的字节，否则「没有标签」与「标签
/// 列表恰好编码成空」会撞在一起，理由同版本 12 那段 `None` 判别字节）。
///
/// 派生列 `wear_channels`（由 `tags` 与标签表在注册期折算而来）**刻意
/// 不混入**：它不是独立声明的内容，它的两个输入已经各自被完整覆盖
/// （物品的 `tags` 走上面这一段，标签的 `wear` 走 `write_tag_fields`），
/// 再混一遍只是把同一份信息数两次，见本模块文档「哈希覆盖哪些字段」
/// 一节「覆盖的是**声明**，不是声明的推论」同一条判据。
///
/// 版本 14（盗贼被动两分批次）：[`write_rule_modifier`] 新增
/// [`RuleModifier::InspectionSuspicion`]（判别值 5）与
/// [`RuleModifier::InspectionConcealment`]（判别值 6）——项目所有者
/// 对盗贼被动的裁定「被动可以分为 **2 种**，**不觉得可疑**，还有
/// **查不出东西**」的两个落点。
///
/// 与版本 6（`RuleModifier::SneakAttack` 判别值 4）**逐条同构**，
/// 递增的理由因此也逐字相同：不新增内容表（[`ContentTableKind`] 的
/// 二十个变体一个未变，[`ContentValueTables`] 的字段一个未加，
/// `check_content_hash_gate_cross_coverage` 那条互校在本批次同样无事
/// 可做——它只守「新增了表」），也不给任何老表新增字段（既有的
/// `TraitDef.rule_modifiers`/`ItemDef.rule_modifiers` 两个字段一个
/// 未动，从未使用过这两个新变体的存量内容写出的字节序列逐字节不变）。
///
/// **但版本号仍然必须递增**，理由与版本 6 那一段完全相同并原样适用：
/// `RuleModifier` 是本模块「表判别字节」同一套机制在枚举层面的复用
/// （[`write_rule_modifier`] 顶部先写一个判别值再写变体自己的字段），
/// 新增判别值即使不改变存量内容的哈希输出，仍然按本模块一贯的保守
/// 纪律递增，不去论证「这次改动对存量内容真的无害」这件事本身要不要
/// 成为免于升版号的理由——那条论证本身也可能出错，递增的代价（读档
/// 分支多判一次 `ContentHashAlgorithmUpgraded`）远低于论证出错的代价
/// （真的漏判成 `ModContentMismatch`）。
///
/// 守门方式同版本 9/10：本段文字 + 本模块单元测试
/// `新增的两个盘查规则修正变体各自混入不同的判别值`（两个新变体的
/// 摘要必须互不相同、也不同于既有五个变体），以及版本 7 那次事故
/// 之后立下的那条纪律——**提交信息声称改了，不等于代码里真的改了**，
/// 本行的字面值就是唯一权威。
/// 版本 15（配方发现批次）：**两张老表各新增一列字段**，两条各自都足以
/// 逼着版本号动——
///
/// - `ItemDef.taught_recipes`（`register-item-teaches-recipe` 的写入
///   目标），[`write_item_fields`] 因此多混入一段「条数 + 逐条解析回
///   `NamespacedId` 字符串」；
/// - `RecipeDef.requires_discovery`（`recipe-requires-discovery!` 的
///   写入目标），[`write_recipe_fields`] 因此多混入一个布尔字节。
///
/// 与版本 13 的 `ItemDef.tags` 逐条同构：长度前缀 `0`／布尔 `0` 也是一
/// 段此前不存在的字节，因此**每一件物品、每一条配方**的条目摘要都变了
/// （即便它一条配方都不教、也不要求发现）——否则「没有声明」与「声明恰
/// 好编码成空」会撞在一起，理由同版本 12 那段 `None` 判别字节。
///
/// 守门方式同版本 13/14：本段文字 + 本模块单元测试
/// `教配方的物品与不教配方的物品摘要不同` 与
/// `要求发现的配方与不要求发现的配方摘要不同`，以及版本 7 那次事故之后
/// 立下的那条纪律——**提交信息声称改了，不等于代码里真的改了**，本行的
/// 字面值就是唯一权威。
///
/// 版本 16（加值类型批次）：**新增一张内容表 + 一张老表新增哈希输入 +
/// 两个既有变体的载荷改了含义**，三条各自都足以逼着版本号动。
///
/// 1. **新增第二十一张内容表** [`ContentTableKind::ModifierType`]
///    （判别值 20）——[`ContentValueTables`] 多了 `modifier_type` 字段,
///    [`entry_value_digest`] 多了一条分支。这是版本 4/5/8/11「新增内容
///    表」那一类：`apply_value_hashes` 从此会为加值类型这一类 id 折进一份
///    此前根本不存在的摘要（该分支本身不写任何字段——`ModifierTypeDef`
///    是空结构体——但函数顶部那个 `kind as u64` 判别值与 `Opaque` 不同,
///    足以把「已注册的加值类型」与「只被 intern 过的裸 id」区分开）。
///    本批次因此是 `scripts/ci/check_field_consumers.py` 的
///    `check_content_hash_gate_cross_coverage` **真的有事可做**的一次:
///    `CONTENT_HASH_KIND_TO_TARGET_TYPE` 与 `TARGET_TYPES` 都补了
///    `ModifierTypeDef` 一条，否则 CI 立刻变红。
/// 2. **老表新增哈希输入**：`TraitDef.rule_modifiers` 与
///    `ItemDef.rule_modifiers` 的元素类型从 `RuleModifier` 变成
///    [`TypedRuleModifier`]（修正本身 + 它属于哪个加值类型），
///    [`write_trait_fields`]/[`write_item_fields`] 因此改走
///    [`write_typed_rule_modifier`]——**每一条规则修正前面都多了一个
///    加值类型的缺席/存在判别字节**。这是版本 4/5（三）/7/9/10/12
///    「老表新增字段」那一类：只有真的声明过规则修正的条目摘要会变
///    （`rule_modifiers` 为空的条目一个字节都没多，长度前缀早就有了）,
///    因此本次**只有 `examplemod` 命名空间的摘要变了，`lostland` 逐位
///    不变**——本体内容至今一条规则修正都没有声明。
/// 3. **两个既有变体的载荷改了含义**：`RuleModifier::Resistance` 的
///    `multiplier_permille`（千分比乘数）换成了 `damage_reduction`
///    （减伤点数），`RuleModifier::InspectionSuspicion` 的
///    `multiplier_permille` 换成了当时的 `suspicion_reduction_permille`
///    （判定系统落地批次再次改名成 `inconspicuous_modifier`）。
///    判别值 `0`/`5` 没变、写入的字节宽度也没变，但**同一段字节现在表示
///    完全不同的规则**（500 从"半伤"变成"减 500 点"）——这正是本常量
///    存在的理由：量尺换了，旧存档的比对必须走
///    `ContentHashAlgorithmUpgraded` 而不是 `ModContentMismatch`。
///
/// 守门方式同版本 13/14/15：本段文字 + 本模块单元测试
/// `声明了加值类型的规则修正与不声明的摘要不同`，加上 `content_audit`
/// 里同批次新增的 `TraitAttrs::rule_modifiers::modifier_type` /
/// `ItemAttrs::rule_modifiers::modifier_type` 两条花名册观察，以及版本 7
/// 那次事故之后立下的那条纪律——**提交信息声称改了，不等于代码里真的
/// 改了**，本行的字面值就是唯一权威。
///
/// ---
///
/// 版本 16（未鉴定物品批次 + 研究经验收窄 + 盲盒批次）：**同一张老表
/// 新增三列字段**，任一条单独都足以逼着版本号动——
///
/// - `ItemDef.requires_identification`（布尔），[`write_item_fields`]
///   因此多混入一个 0/1 字节位；
/// - `ItemDef.study_experience`（`i64`），多混入一个整数；
/// - `ItemDef.blind_box_pool`（列表），多混入一段「条数 + 逐条(产出物
///   解析回 `NamespacedId` 字符串, 数量, 权重)」。
///
/// 与版本 15 的 `ItemDef.taught_recipes` 逐条同构：布尔 `0`／整数 `0`
/// ／长度前缀 `0` 也都是一段此前不存在的字节，因此**每一件物品**的条目
/// 摘要都变了（即便它一眼就认得、研究不值经验、也不是盲盒）——否则
/// 「没有声明」与「声明恰好编码成空」会撞在一起，理由同版本 12 那段
/// `None` 判别字节。
///
/// `ContentTableKind` 的二十个变体一个未变、`ContentValueTables` 的
/// 字段一个未加，因此 `check_content_hash_gate_cross_coverage` 那条互校
/// 在本批次同样无事可做（它只守「新增了表」）。
///
/// 守门方式同版本 13/14/15：本段文字 + 本模块单元测试
/// `需要鉴定的物品与不需要鉴定的物品摘要不同`、
/// `研究经验不同的物品摘要不同` 与 `盲盒池不同的物品摘要不同`，以及
/// 版本 7 那次事故之后立下的那条纪律——**提交信息声称改了，不等于代码
/// 里真的改了**，本行的字面值就是唯一权威。
///
/// **版本 17**：上面两段所述的两批改动最终合并落地在同一个版本号上——
/// 加值类型那批与未鉴定/盲盒那批各自独立开发时都写成 16，合并时两批的
/// 哈希输入同时存在，因此实际的量尺是两者之和，版本号必须是 17。
///
/// ---
///
/// 版本 18（副职天赋接线批次）：**一张老表新增一列字段**——
/// `SubclassDef.traits`（项目所有者裁定「副职……带有技能的」的落点，
/// 见 `crate::subclass::SubclassDef::traits`），[`write_subclass_fields`]
/// 因此多混入一段「条数 + 逐条 (天赋解析回 `NamespacedId` 字符串,
/// 解锁等级)」，与 `write_class_fields`/`write_race_fields` 的同名字段
/// 逐字节同构。
///
/// 与版本 15 的 `ItemDef.taught_recipes` 逐条同构：长度前缀 `0` 也是一段
/// 此前不存在的字节，因此**每一个副职**的条目摘要都变了（即便它一条
/// 天赋都不授予）——否则「没有声明」与「声明恰好编码成空」会撞在一起,
/// 理由同版本 12 那段 `None` 判别字节。本体六条副职与 example_mod 那条
/// 的摘要因此全部改变。
///
/// `ContentTableKind` 的二十一个变体一个未变、[`ContentValueTables`] 的
/// 字段一个未加（副职表本来就在里面），因此
/// `scripts/ci/check_field_consumers.py` 的
/// `check_content_hash_gate_cross_coverage` 那条互校在本批次无事可做
/// （它只守「新增了表」）。
///
/// 守门方式同版本 13/14/15：本段文字 + 本模块单元测试
/// `授予天赋的副职与不授予天赋的副职摘要不同` 与
/// `副职天赋解锁等级不同则摘要不同`，加上 `content_audit` 里同批次新增
/// 的 `SubclassAttrs::traits` 花名册观察与跨表引用检查，以及版本 7 那次
/// 事故之后立下的那条纪律——**提交信息声称改了，不等于代码里真的
/// 改了**，本行的字面值就是唯一权威。
///
/// ---
///
/// 版本 19（制作类副职奖励批次）：**一个被哈希的封闭枚举新增一个变体**
/// ——[`ll_sim::rule_modifier::RuleModifier::CraftYield`]，
/// [`write_rule_modifier`] 因此多一条判别值 `7` 的分支（配方类别解析回
/// 完整 `NamespacedId` 字符串 + 额外产出件数）。
///
/// 与此前每一次「新增列」的版本推进**不同**，这一次的量尺变化是
/// **局部的**：既有七个变体的编码一个字节都没动（判别值 0..=6 原样
/// 保留，这是本模块「判别值接着既有档往后编号」那条纪律的收益），因此
/// 只有**真的声明了这条新修正**的天赋/物品条目摘要会变，既有内容的
/// 逐条摘要全部不变。
///
/// **那为什么还要动版本号**：ADR 0022/0027 的义务不是「摘要变了才动」,
/// 是「**哈希输入的量尺变了就得动**」。判别值 `7` 从此有了含义，一份
/// 老引擎与一份新引擎对同一条内容能算出不同的结果（老引擎根本解析不出
/// 这条修正），版本号正是用来让这种差异**当场可见**而不是伪装成内容
/// 变更。此外本批次同时新增了 `mods/lostland/traits.json5`（四条制作
/// 精通天赋）并给四条本体副职各挂一条 `traits`——那是**内容变更**，
/// 走内容哈希本身，与本版本号是两件事。
///
/// `ContentTableKind` 的二十一个变体一个未变、[`ContentValueTables`] 的
/// 字段一个未加，因此 `check_content_hash_gate_cross_coverage` 那条互校
/// 在本批次同样无事可做（它只守「新增了表」）。
///
/// 守门方式同版本 13/14/15/18：本段文字 + 本模块单元测试
/// `制作产出加成的摘要与其余变体都不同` 与
/// `制作产出加成件数不同则摘要不同`，以及版本 7 那次事故之后立下的
/// 那条纪律——**提交信息声称改了，不等于代码里真的改了**，本行的
/// 字面值就是唯一权威。
///
/// ---
///
/// 版本 21（`wt-dice` 合并批次）：**本次递增是合并本身造成的，不是任何
/// 一批改动单独造成的**。
///
/// `main` 上已发布的版本 19（上一节，制作类副职奖励）与 `wt-dice` 分支
/// 上的版本 19/20（下面①②）是**两把互不相同的量尺**——各自只含自己
/// 那一批哈希输入。合并之后三批输入**同时存在**，得到的是第三把尺子,
/// 与两者都不同：
///
/// - 沿用 `19` 会让它同时指代「只有 `CraftYield`」与「三批都有」两把
///   尺子，正是版本 7 那次事故的形状。
/// - 沿用 `20` 同样不行：`wt-dice` 的 20 里**没有** `CraftYield` 那条
///   判别值，一份按它算出来的摘要与本版本算出来的不同。
///
/// 因此取 `21`。**跳过 20 不是浪费**：这个字段是一个单调标记，它要回答
/// 的问题是「你那份存档的量尺是不是我这一把」，不是「一共改过几次」。
///
/// 合并之后判别值的**最终分配**（[`write_rule_modifier`]，九个变体
/// 九个值，互不相同）：`Resistance` = 0、`RerollOnce` = 1、
/// `Advantage` = 2、`Disadvantage` = 3、`SneakAttack` = 4、
/// `InspectionSuspicion` = 5、`InspectionConcealment` = 6、
/// `CraftYield` = 7、`Vulnerability` = 8。既有的 `0..=6` **一个字节都
/// 没动**——这正是本模块「判别值接着既有档往后编号」那条纪律要买的
/// 东西：两个分支各自往后加一条，合并时只需要给后到的那一条挪号
/// （`Vulnerability` 7 → 8，理由见 [`write_rule_modifier`] 里那段
/// 注释），既有内容的逐条摘要因此不受合并影响。
///
/// 下面①②是 `wt-dice` 带进来的两批改动原文，编号按合并后的事实订正
/// （分支上的「版本 19/20」在 `main` 上从未存在过，它们的量尺变更一并
/// 由本版本 21 表达）。
///
/// ## ① 易伤与减伤对称批次（分支上曾编作 19）
///
/// **一个既有字段的合法值域收窄，外加
/// 一个新的枚举变体**——两条各自都足以逼着版本号动。
///
/// 1. **值域收窄（这一条才是必须的）**：`RuleModifier::Resistance`
///    与 `RawItemResistance` 的 `damage_reduction` 此前允许负数,负数
///    表示「脆弱」；现在装载期一律 `.max(0)`。同一份 mod 数据里的
///    `damage_reduction: -5` 此前哈希进 `-5`、行为是「多挨 5 点」,
///    现在哈希进 `0`、行为是「什么也不做」——**同一段字节表示的规则
///    变了**，与版本 18 文档「两个既有变体的载荷改了含义」那一条是
///    完全同一类，理由也完全相同：量尺换了，旧存档的比对必须走
///    `ContentHashAlgorithmUpgraded` 而不是 `ModContentMismatch`。
/// 2. **新增变体**：[`write_rule_modifier`] 多一个判别值
///    （`RuleModifier::Vulnerability`，当时取 `7`，判定系统落地批次
///    因与 `CraftYield` 撞车改成 `8`）。这一条**单独看不改任何既有
///    摘要**（没有声明易伤的条目一个字节都没多，判别值是逐条写的,
///    不是表头），但它是（一）那条收窄的配套——脆弱从此有正规写法。
///
/// **本仓库现有内容里没有任何一条负 `damage_reduction`**（`lostland`
/// 的 `forge_apron` 是 `6`、`example_mod` 的 `acid_hide` 是 `4`、
/// `acid_ward_amulet` 是 `3`），因此（一）对**现有**摘要逐位无影响；
/// 真正改变摘要的是同批次给 `examplemod:acid_hide` 新增的那条
/// `kind: "vulnerability"` 声明（走那段新判别值字节）——只有
/// `examplemod` 命名空间的摘要变了，`lostland` 逐位不变。版本号动的
/// 理由是（一）那条**量尺变更**，不是这一条内容新增。
///
/// `ContentTableKind` 的二十一个变体一个未变、[`ContentValueTables`]
/// 的字段一个未加，因此
/// `scripts/ci/check_field_consumers.py` 的
/// `check_content_hash_gate_cross_coverage` 那条互校在本批次无事可做
/// （它只守「新增了表」）。
///
/// 守门方式同版本 13/14/15：本段文字 + 本模块单元测试
/// `易伤与同数值的减伤摘要不同`，以及版本 7 那次事故之后立下的那条
/// 纪律——**提交信息声称改了，不等于代码里真的改了**，本行的字面值
/// 就是唯一权威。
///
/// ## ② 判定系统落地批次（分支上曾编作 20）
///
/// 两条各自独立的理由，任一条单独成立都足以要求递增：
///
/// 1. **两个既有变体的载荷改了含义。**
///    `RuleModifier::InspectionSuspicion` 的载荷此前是「从盘查触发
///    概率上减掉的千分比点数」，现在是「在对抗判定里加给隐蔽方的骰子
///    点数」；`InspectionConcealment` 的载荷此前是「每件物品不被看见
///    的千分比概率」，现在是「加给藏东西那一方的骰子点数」。同一段
///    字节（`400`）此前表示「减 40 个百分点」，现在表示「加 400 点，
///    而 400 已经越过修正上限 28 会被装载期拒掉」——**量尺换了**，与
///    版本 18/19 那两条是完全同一类。
/// 2. **一个既有变体的判别值改了。** `RuleModifier::Vulnerability`
///    从 `7` 改成 `8`，因为 `main` 上合入的 `CraftYield` 已经占了 `7`
///    ——完整论证见 [`write_rule_modifier`] 里那一段注释。
///
/// 同批次新增的三个 `kind`（`advantage`/`disadvantage`/`reroll-once`）
/// **单独看不改任何既有摘要**：它们走的是本来就已经写死的判别值
/// `1`/`2`/`3`（那三个变体从一开始就在 `write_rule_modifier` 里），
/// 本批次只是让内容第一次写得出它们。没有声明它们的条目一个字节都
/// 没多。
///
/// `ContentTableKind` 的变体一个未变、[`ContentValueTables`] 的字段
/// 一个未加，因此 `scripts/ci/check_field_consumers.py` 的
/// `check_content_hash_gate_cross_coverage` 那条互校在本批次同样无事
/// 可做。
///
/// ## ③ 偷袭迁进判定系统批次
///
/// **一个既有变体的载荷改了含义**，与版本 21 的 ①/② 两条、以及版本
/// 18/19 是完全同一类。`RuleModifier::SneakAttack` 的第一个载荷此前是
/// `luck_chance_permille_per_point`（「每点有效幸运换算的千分比触发
/// 率」），现在是 `sneak_modifier`（「加给偷袭者那一侧的骰子点数」）：
/// 同一段字节 `20` 此前表示「每点幸运 +2%，幸运 50 就钳在必定触发」，
/// 现在表示「加 20 点，而 20 落在修正上限 28 之内是一条合法声明」。
/// **量尺换了**——摘要必须跟着变，否则两份语义完全不同的内容会算出
/// 同一个摘要。
///
/// 判别值 `4` **一个字未改**，字段个数与写入顺序也没变（先修正、后
/// 追加伤害），因此这次改的确实只有「同一段字节表示什么」这一件事。
///
/// 同批次 `ll_sim` 侧新增的两个判定种类标识符
/// （`lostland:critical`/`lostland:sneak-attack`）**不改任何既有摘要**：
/// `advantage`/`disadvantage` 的载荷是内容作者自己写的开放标识符，
/// 引擎认得几个与哈希无关，没有声明它们的条目一个字节都没多。
///
/// `ContentTableKind` 的变体一个未变、[`ContentValueTables`] 的字段
/// 一个未加，因此 `scripts/ci/check_field_consumers.py` 的
/// `check_content_hash_gate_cross_coverage` 那条互校在本批次同样无事
/// 可做。守门方式同版本 13/14/15/21：本段文字 + 本模块单元测试
/// `偷袭修正与同数值的其余判定修正摘要不同`，以及版本 7 那次事故之后
/// 立下的那条纪律——**提交信息声称改了，不等于代码里真的改了**，下面
/// 这一行的字面值就是唯一权威。
///
/// ---
///
/// `check_content_hash_gate_cross_coverage` 那条互校在版本 21 那一批
/// 同样无事可做。
///
/// # 版本 22（资源点批次）
///
/// **新增一张内容表**：[`ContentTableKind::Resource`]（判别值 21）——
/// [`ContentValueTables`] 多了一个 `resource` 字段，[`entry_value_digest`]
/// 多了一条 [`write_resource_fields`] 分支。这是版本 4/5/16「新增内容
/// 表」那一类：既有内容的摘要一个字节都没变，但**同一套内容在新旧两
/// 版算法下的哈希不同**（新表的条目此前落在
/// [`ContentTableKind::Opaque`] 一侧、只混 id 不混字段值），因此必须
/// 递增。
///
/// 与版本 21 那一批不同，这次 `check_content_hash_gate_cross_coverage`
/// **确实有事可做**：`scripts/ci/check_field_consumers.py` 的
/// `CONTENT_HASH_KIND_TO_TARGET_TYPE` 与 `TARGET_TYPES` 同批次各补了
/// 一条（`Resource` → `ResourceAttrs`），否则那条互校会当场把门禁
/// 变红——这正是它存在的意义。
///
/// **版本 23**：上面两批（偷袭载荷改含义、资源点新表）各自独立开发时
/// 都写成 22，合并后两批的哈希输入同时存在，量尺与任一单批都不同，
/// 因此必须是 23。版本号是单调标记不是计数器，跳号无害。
///
/// # 版本 24（资源两层分类批次）
///
/// **已覆盖的表多了一个字段**：[`ll_world::resource::ResourceAttrs`]
/// 新增 `category`（五个大类之一），[`write_resource_fields`] 因此多混
/// 一段字符串。这是版本 9/12「补齐已覆盖表的漏字段」那一类：**同一套
/// 内容在新旧两版算法下的摘要不同**（旧版根本没读这个字段），因此必须
/// 递增。
///
/// `check_content_hash_gate_cross_coverage` 这次**无事可做**：
/// `Resource` → `ResourceAttrs` 那条映射在版本 22 就补齐了，本批次只是
/// 往已在 `TARGET_TYPES` 里的结构体上加字段，`scripts/ci/
/// check_field_consumers.py` 会自动扫到它并要求一个决策层消费者——那个
/// 消费者是 `ll_mod::roster` 的名册亲和表，见该字段文档「谁读它」。
///
/// 守门方式同版本 13/14/15/21/23 那几批：本段文字 + 本模块单元测试
/// `资源大类不同的两条资源摘要不同`，以及版本 7 那次事故之后立下的
/// 那条纪律——**提交信息声称改了，不等于代码里真的改了**，下面这一行
/// 的字面值就是唯一权威。
///
/// ---
///
/// # 版本 24（伤害类别显示名字段）
///
/// **既有表新增一个字段**：[`crate::damage_category::DamageCategoryDef`]
/// 多了 `display_name_key`，[`write_damage_category_fields`] 因此多混入
/// 一条 `NamespacedId`。这是版本 9/13 那一类（不是「新增一张表」）：
/// **同一份内容在新旧两版算法下的摘要不同**，因为伤害类别的字节流从
/// 「一个 optional 公式引用」变成「一条本地化键 + 一个 optional 公式
/// 引用」。按 ADR 0027 必须递增。
///
/// 这一批不新增内容表，`check_content_hash_gate_cross_coverage` 那条
/// 互校无事可做（`CONTENT_HASH_KIND_TO_TARGET_TYPE` 里
/// `DamageCategory` → `DamageCategoryDef` 早已存在）；
/// `scripts/ci/check_field_consumers.py` 侧新增的是一条 `EXEMPTIONS`
/// （新字段的消费者在呈现层而非决策层，同 `WeatherDef.display_name_key`）。
///
/// **版本 25**：上面两批（资源大类字段、伤害类别显示名字段）各自
/// 独立开发时都写成 24，合并后两批的哈希输入同时存在，量尺与任一
/// 单批都不同，因此必须是 25。版本号是单调标记不是计数器。
///
/// ---
///
/// # 版本 24（家具层批次）
///
/// **既有表多了一个字段**：[`write_item_fields`] 末尾多混入一个
/// `ItemDef.furniture` 布尔。这是版本 16「既有表多字段」那一类，**不是**
/// 新增内容表：`ContentTableKind` 一个变体都没加，[`ContentValueTables`]
/// 与 [`entry_value_digest`] 的分支表一字未改。每一件物品（包括
/// `furniture: false` 的绝大多数）的条目摘要都因为多出的这一个字节位而
/// 改变，因此必须递增。
///
/// `check_content_hash_gate_cross_coverage` 那条互校本批次**无事可做**
/// ——它检查的是「`ContentTableKind` 的每个变体都在
/// `scripts/ci/check_field_consumers.py` 的 `CONTENT_HASH_KIND_TO_TARGET_TYPE`
/// 与 `TARGET_TYPES` 里有落点」，而 `Item` → `ItemDef` 这一条早就在，
/// 新字段落在同一个已登记的类型里。`ItemDef.furniture` 在决策层
/// （`ll_sim::resolve::resolve_drop`/`resolve_craft` 读
/// `ll_sim::item::ItemRule::furniture`，字段同名）有真实读取点，字段
/// 门禁因此也直接绿。
///
/// **版本 26**：上面这批（家具标志字段）独立开发时写成 24，而主干此时
/// 已经因为资源大类与伤害类别显示名两批走到 25。合并后三批的哈希输入
/// 同时存在，量尺与任何一批单独存在时都不同，因此必须是 26。版本号是
/// 单调标记不是计数器，跳号无害。
///
/// ---
///
/// # 版本 27（文化批次）
///
/// **新增一张内容表**：[`ContentTableKind::Culture`]（判别值 22）——
/// [`ContentValueTables`] 多了一个 `culture` 字段，[`entry_value_digest`]
/// 多了一条 [`write_culture_fields`] 分支。这是版本 4/5/16/22
/// 「**新增内容表**」那一类，不是「已有表加字段」那一类：既有内容的
/// 字段摘要一个字节都没变，但**同一套内容在新旧两版算法下的哈希不同**
/// ——`lostland:mining_hold` 这类条目此前落在
/// [`ContentTableKind::Opaque`] 一侧（只混 id、不混字段值），现在混的
/// 是完整字段流。按 ADR 0027 必须递增。
///
/// 与版本 22（资源点批次）一样，这次 `check_content_hash_gate_cross_coverage`
/// **确实有事可做**：`scripts/ci/check_field_consumers.py` 的
/// `CONTENT_HASH_KIND_TO_TARGET_TYPE` 与 `TARGET_TYPES` 各补了一条
/// （`Culture` → `CultureAttrs`），否则那条互校会当场把门禁变红。
///
/// 守门方式同前几批：本段文字 + 本模块单元测试
/// `建材不同的两条文化摘要不同`，以及版本 7 那次事故之后立下的那条
/// 纪律——**提交信息声称改了，不等于代码里真的改了**，下面这一行的
/// 字面值就是唯一权威。
///
/// ---
///
/// # 版本 28（对话内容表批次）
///
/// **新增两张内容表**：[`ContentTableKind::Dialogue`]（判别值 23）与
/// [`ContentTableKind::DialogueNode`]（判别值 24）——[`ContentValueTables`]
/// 多了 `dialogue`/`dialogue_node` 两个字段，[`entry_value_digest`] 多了
/// [`write_dialogue_fields`]/[`write_dialogue_node_fields`] 两条分支。这是
/// 版本 4/5/16/22/27「**新增内容表**」那一类，不是「已有表加字段」那一类：
/// 既有内容的字段摘要一个字节都没变，但**同一套内容在新旧两版算法下的哈希
/// 不同**——`lostland:steward_greeting` 这类条目此前落在
/// [`ContentTableKind::Opaque`] 一侧（只混 id、不混字段值），现在混的是完整
/// 字段流。按 ADR 0027 必须递增。
///
/// 与版本 22/27 一样，这次 `check_content_hash_gate_cross_coverage`
/// **确实有事可做**：`scripts/ci/check_field_consumers.py` 的
/// `CONTENT_HASH_KIND_TO_TARGET_TYPE` 与 `TARGET_TYPES` 各补了两条
/// （`Dialogue` → `DialogueDef`、`DialogueNode` → `DialogueNodeDef`），
/// 否则那条互校会当场把门禁变红。
///
/// **本表的 `text_key` 是本地化键，不是 `ContentIndex`**（见
/// [`crate::dialogue`] 模块文档末节），因此直接
/// [`StateHasher::write_namespaced_id`] 混入，不经 `Registry::resolve`——
/// 与 `SpaceProfile.reverb_tag` 同一种情形。
///
/// 守门方式同前几批：本段文字 + 本模块单元测试
/// `跳转目标不同的两个对话节点摘要不同`，以及版本 7 那次事故之后立下的
/// 那条纪律——**提交信息声称改了，不等于代码里真的改了**，下面这一行的
/// 字面值就是唯一权威。
///
/// ---
///
/// # 版本 29（据点建筑类型批次）
///
/// **既有表加字段**，不是新增表：[`ll_world::culture::CultureAttrs`] 多了
/// `buildings`（这份文化有哪几类屋子、各占多少权重、每类摆什么家具），
/// [`write_culture_fields`] 末尾多混入这一段。这是版本 16
/// （`ItemDef.furniture`）那一类——**文化表以外的条目摘要一个字节都没变**，
/// 但每一条文化的摘要都变了（哪怕它一条建筑都不声明也不可能，注册期拒了
/// 空表）。按 ADR 0027 必须递增。
///
/// 混入形状与同一函数里的 `founder_races` 逐字一致：先写条数，再逐条写
/// 权重 + 家具列表（同样先写条数再逐条写「物品 id + 件数」）。**物品索引
/// 经 [`Registry::resolve`] 换成命名空间 id 再混入**，不混裸的
/// `ContentIndex` ——裸数值当判据正是本仓库反复付过代价的那件事，且它会让
/// 「往注册表中间插一条内容」这种与文化毫无关系的改动把哈希打飞。
///
/// **本次不动 `check_field_consumers.py` 的 `CONTENT_HASH_KIND_TO_TARGET_TYPE`**：
/// `Culture` → `CultureAttrs` 那一行在版本 27 就加好了，新字段自动落进
/// 同一条互校，不需要有人记得改第二处。（新增的 `CultureAttrs.buildings`
/// 需要一条 `EXEMPTIONS`，与既有六条同一种处境——消费者在 `ll-world`/
/// `ll-game`，都不在决策层 glob 内。）
///
/// 守门方式同前几批：本段文字 + 本模块单元测试
/// `建筑类型不同的两条文化摘要不同`。
pub const CONTENT_HASH_ALGORITHM_VERSION: u32 = 29;

/// 表种类判别——混入每条内容摘要判别字节的枚举形式，避免"一个地形的
/// 字段值"与"一个种族的字段值"凑巧编码成同一段字节流时被误判成同一份
/// 内容（不同表的字段数量、取值范围都可能在某些边界组合下重叠）。
///
/// 判别值编号延续 ADR 0027 落地时定下的 `0..=6`（`Opaque`/六张原有
/// 表），新纳入覆盖的六张表接着往后编号（`7..=12`）——不打乱已有编号，
/// 理由同 [`crate::content_hash::write_resource_cost`] 文档「判别值
/// 接着既有两档往后编号,不打乱……已经写死的 0/1」一节同一条既有纪律。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentTableKind {
    /// 不落在任何一张表里的纯 id 引用——例如
    /// `base_placeholder::PLACEHOLDER_RACE_ID`，或
    /// `QuestCondition::KillCount::target_kind` 指向的"敌人类型"占位
    /// 标识符（当前代码库还没有敌人类型注册表，见 `crate::quest`
    /// 模块文档「跨表引用」一节）。
    Opaque = 0,
    /// 地形表（定义在 `ll-world`，见 [`crate::base_terrain`] 模块文档
    /// 「与 Registry 的关系」一节）。
    Terrain = 1,
    /// 职业表。
    Class = 2,
    /// 技能表。
    Skill = 3,
    /// 副职表。
    Subclass = 4,
    /// 任务表。
    Quest = 5,
    /// 种族表。
    Race = 6,
    /// 空间层属性表（内容值哈希覆盖面扩展批次新增）。
    SpaceProfile = 7,
    /// 动画剪辑表（内容值哈希覆盖面扩展批次新增）——纯表现层内容
    /// （ADR 0020 甲区，`crate::clip` 模块文档），但按本模块文档
    /// 「哈希覆盖哪些字段」一节的判据，不因为"不参与模拟结算"就排除
    /// 在值哈希之外。
    Clip = 8,
    /// 天赋表（内容值哈希覆盖面扩展批次新增）。
    Trait = 9,
    /// 资源池表（内容值哈希覆盖面扩展批次新增）。
    ResourcePool = 10,
    /// 物品表（内容值哈希覆盖面扩展批次新增）。
    Item = 11,
    /// 经验曲线表（内容值哈希覆盖面扩展批次新增）——不含
    /// [`crate::xp_curve::XpCurveBindings`]，见本模块文档「哈希覆盖哪些
    /// 字段」一节「例外，且是刻意的例外」一段。
    XpCurve = 12,
    /// 伤害公式表（伤害公式引擎批次新增）。
    Formula = 13,
    /// 武器类别表（伤害类别/抗性接线批次新增）。
    WeaponCategory = 14,
    /// 伤害类别表（伤害类别/抗性接线批次新增）。
    DamageCategory = 15,
    /// 天气表（天气系统批次新增，定义在 `ll-world`）。
    Weather = 16,
    /// 配方表（制作系统批次新增）。
    Recipe = 17,
    /// 配方类别表（制作系统批次新增）。
    RecipeCategory = 18,
    /// 标签表（耐久标签批次新增）——`register-tag` 的写入目标，见
    /// [`crate::tag`] 模块文档。
    Tag = 19,
    /// 加值类型表（加值类型批次新增）——规则修正合并时「同一类型取
    /// 最强、不同类型相加」里的**类型**，见 [`crate::modifier_type`]
    /// 模块文档。
    ModifierType = 20,
    /// 资源表（资源点批次新增，定义在 `ll-world`）——见
    /// [`ll_world::resource`] 模块文档。
    Resource = 21,
    /// 文化表（文化批次新增，定义在 `ll-world`）——见
    /// [`ll_world::culture`] 模块文档。
    Culture = 22,
    /// 会话入口表（对话内容表批次新增）——见 [`crate::dialogue`] 模块文档。
    Dialogue = 23,
    /// 对话节点表（对话内容表批次新增）——与 `Dialogue` 同住
    /// `dialogues.json5`，但它们是两张独立的表（节点有自己的 id、自己的
    /// `ContentIndex`），因此各占一个判别值。
    DialogueNode = 24,
}

/// 全部内容表的只读引用集合——供 [`apply_value_hashes`]/[`classify_index`]
/// 遍历使用。
///
/// 只读引用（不像 `ll_mod::pipeline::GameplayTables` 那样是
/// `&mut`）：值哈希是装载完成后的收尾读取步骤，不应该、也不需要再
/// 改动任何一张表。
pub struct ContentValueTables<'a> {
    /// 地形表（定义在 `ll-world`，见 [`crate::base_terrain`] 模块文档
    /// 「与 Registry 的关系」一节）。
    pub terrain: &'a TerrainTable,
    /// 职业表。
    pub class: &'a ClassTable,
    /// 技能表。
    pub skill: &'a SkillTable,
    /// 副职表。
    pub subclass: &'a SubclassTable,
    /// 任务表。
    pub quest: &'a QuestTable,
    /// 种族表。
    pub race: &'a RaceTable,
    /// 空间层属性表（内容值哈希覆盖面扩展批次新增，定义在 `ll-world`）。
    pub space_profile: &'a SpaceProfileTable,
    /// 动画剪辑表（内容值哈希覆盖面扩展批次新增）。
    pub clip: &'a ClipTable,
    /// 天赋表（内容值哈希覆盖面扩展批次新增）。
    pub trait_def: &'a TraitTable,
    /// 资源池表（内容值哈希覆盖面扩展批次新增）。
    pub resource_pool: &'a ResourcePoolTable,
    /// 物品表（内容值哈希覆盖面扩展批次新增）。
    pub item: &'a ItemTable,
    /// 经验曲线表（内容值哈希覆盖面扩展批次新增）——不含
    /// [`crate::xp_curve::XpCurveBindings`]，理由同 [`ContentTableKind::XpCurve`]
    /// 文档。
    pub xp_curve: &'a XpCurveTable,
    /// 伤害公式表（伤害公式引擎批次新增）。
    pub formula: &'a FormulaTable,
    /// 武器类别表（伤害类别/抗性接线批次新增）。
    pub weapon_category: &'a WeaponCategoryTable,
    /// 伤害类别表（伤害类别/抗性接线批次新增）。
    pub damage_category: &'a DamageCategoryTable,
    /// 天气表（天气系统批次新增，定义在 `ll-world`，理由见
    /// `ll_world::weather` 模块文档「为什么天气表定义在 `ll-world`」）。
    pub weather: &'a WeatherTable,
    /// 配方表（制作系统批次新增）。
    pub recipe: &'a RecipeTable,
    /// 配方类别表（制作系统批次新增）。
    pub recipe_category: &'a RecipeCategoryTable,
    /// 标签表（耐久标签批次新增）。
    pub tag: &'a TagTable,
    /// 加值类型表（加值类型批次新增）。
    pub modifier_type: &'a ModifierTypeTable,
    /// 资源表（资源点批次新增，定义在 `ll-world`，理由与天气表逐字
    /// 相同：唯一的强制消费者 `ll_world::chronicle` 就在那个 crate，
    /// 把表放进下游的 `ll-mod` 会要求 `ll-world` 反向依赖它）。
    pub resource: &'a ResourceTable,
    /// 文化表（文化批次新增，定义在 `ll-world`，理由与资源表逐字
    /// 相同：强制消费者 `ll_world::chronicle`/`ll_world::settlement`
    /// 就在那个 crate）。
    pub culture: &'a CultureTable,
    /// 会话入口表（对话内容表批次新增）。
    pub dialogue: &'a DialogueTable,
    /// 对话节点表（对话内容表批次新增）。
    pub dialogue_node: &'a DialogueNodeTable,
}

/// 判定一个 `ContentIndex` 归属哪张内容表——[`entry_value_digest`] 用它
/// 决定该按哪个 `write_*_fields` 分支求摘要,`ll_game::content` 的覆盖率
/// 回归测试也复用同一个函数（而不是自己重新写一份等价的 if/else 链,
/// 那会制造两处判断随时间漂移的风险）。
///
/// # 编译期强制：穷尽解构 `*tables`
///
/// 函数体第一行对 `*tables` 做穷尽字段解构（不带 `..`）——理由与
/// `ll_content::remap` 模块文档「模块内穷尽解构」一节完全相同：给
/// [`ContentValueTables`] 新增一个字段而忘记在这里显式处理它，会让
/// `cargo build` 在这一行报 "pattern does not mention field `……`"
/// 编译错误,逼着下一个新增内容表的人必须先决定新表要不要参与值哈希,
/// 而不是像 ADR 0027 升级前那样只能寄希望于代码评审。函数末尾对
/// [`ContentTableKind`] 的 `match` 同样不带通配分支——给这个枚举新增
/// 一个判别值而忘记在 [`entry_value_digest`] 里补一个 `write_*_fields`
/// 调用，同样会编译失败，是第二道独立的编译期防线。
///
/// # 局限：不堵"压根忘了给 `ContentValueTables` 加字段"这一步
///
/// 上面两道防线保护的是"字段/判别值已经加了,但没有接满全部使用点"，
/// 不保护"新增的 `*Table` 类型本身"——编译器无法自动发现仓库里新出现
/// 了一个具备 `is_defined(ContentIndex) -> bool` 方法的结构体,并强迫
/// 某处把它接进本函数,这一步没有通用的编译期手段,与 ADR 0027「后果
/// （技术债与后续）」一节记录的局限同构。`ll_game::content` 模块的
/// 覆盖率回归测试是这一步的测试期兜底：用仓库真实的 `mods/` 目录+本体
/// 内容跑一遍完整装载,断言"落在 [`ContentTableKind::Opaque`] 的 id
/// 集合"恰好等于已知的例外集合,不多不少——新增一张表却忘记接进本函数,
/// 那张表全部条目会被判定成 `Opaque`,从而让这条测试变红,而不是像升级
/// 前那样只能靠代码评审肉眼发现。
pub fn classify_index(index: ContentIndex, tables: &ContentValueTables<'_>) -> ContentTableKind {
    let ContentValueTables {
        terrain,
        class,
        skill,
        subclass,
        quest,
        race,
        space_profile,
        clip,
        trait_def,
        resource_pool,
        item,
        xp_curve,
        formula,
        weapon_category,
        damage_category,
        weather,
        recipe,
        recipe_category,
        tag,
        modifier_type,
        resource,
        culture,
        dialogue,
        dialogue_node,
    } = *tables;

    let terrain_kind = TerrainKind::from_index(index);
    if terrain.is_defined(terrain_kind) {
        ContentTableKind::Terrain
    } else if class.is_defined(index) {
        ContentTableKind::Class
    } else if skill.is_defined(index) {
        ContentTableKind::Skill
    } else if subclass.is_defined(index) {
        ContentTableKind::Subclass
    } else if quest.is_defined(index) {
        ContentTableKind::Quest
    } else if race.is_defined(index) {
        ContentTableKind::Race
    } else if space_profile.is_defined(index) {
        ContentTableKind::SpaceProfile
    } else if clip.is_defined(index) {
        ContentTableKind::Clip
    } else if trait_def.is_defined(index) {
        ContentTableKind::Trait
    } else if resource_pool.is_defined(index) {
        ContentTableKind::ResourcePool
    } else if item.is_defined(index) {
        ContentTableKind::Item
    } else if xp_curve.get(index).is_some() {
        ContentTableKind::XpCurve
    } else if formula.get(index).is_some() {
        ContentTableKind::Formula
    } else if weapon_category.is_defined(index) {
        ContentTableKind::WeaponCategory
    } else if damage_category.is_defined(index) {
        ContentTableKind::DamageCategory
    } else if weather.is_defined(index) {
        ContentTableKind::Weather
    } else if recipe.is_defined(index) {
        ContentTableKind::Recipe
    } else if recipe_category.is_defined(index) {
        ContentTableKind::RecipeCategory
    } else if tag.is_defined(index) {
        ContentTableKind::Tag
    } else if modifier_type.is_defined(index) {
        ContentTableKind::ModifierType
    } else if resource.is_defined(index) {
        ContentTableKind::Resource
    } else if culture.is_defined(index) {
        ContentTableKind::Culture
    } else if dialogue.is_defined(index) {
        ContentTableKind::Dialogue
    } else if dialogue_node.is_defined(index) {
        ContentTableKind::DialogueNode
    } else {
        ContentTableKind::Opaque
    }
}

/// 全部内容装载完毕后调用一次：把全部内容表的字段值折进 `registry`
/// 已有的按命名空间内容哈希——见模块文档「为什么不能在 `intern` 内部
/// 做」一节。
///
/// `registry`/`tables` 必须出自同一次装载会话（同一批 `intern`/
/// `define` 调用的产物）——否则字段值与 id 对不上号，`ContentIndex`
/// 解析也可能失败（见模块文档「`ContentIndex` 字段」一节的兜底行为）。
///
/// # 契约：对同一个 `registry` 只调用一次
///
/// 折叠用的是异或（[`Registry::fold_content_digest`] 文档），对同一
/// 个命名空间重复折叠**同一个**字段值摘要会把它异或抵消掉——若在
/// 同一个 `registry` 上先后调用本函数两次（例如误以为"装载了更多
/// 内容后要重新跑一遍"），第一次已经处理过的旧内容会被摘要两次、
/// 抵消归零，只有第二次新增的内容才会正确留下痕迹，产出一个看似
/// 合理实则错误的哈希。正确用法是等全部内容（本体 + 全部 mod）都
/// 装载完毕后，对这次会话最终的 `registry`/`tables` 调用恰好一次
/// ——`ll_game::content::load_content` 是生产环境唯一的调用点。
pub fn apply_value_hashes(registry: &mut Registry, tables: &ContentValueTables<'_>) {
    for id in registry.snapshot() {
        let Some(index) = registry.get(&id) else {
            // 理论不可达：`id` 刚从 `registry.snapshot()` 取出，`get`
            // 查询的正是同一个 `registry`。留一条防御分支而非 `expect`
            // ——与本模块「不是安全或存档完整性校验」的一贯立场一致，
            // 跳过这一条而不是让整次装载 panic。
            continue;
        };
        let digest = entry_value_digest(&id, index, registry, tables);
        registry.fold_content_digest(id.namespace(), digest);
    }
}

/// 对单条内容（`id`/`index` 指向同一条）求一个包含字段值的确定性摘要。
fn entry_value_digest(
    id: &NamespacedId,
    index: ContentIndex,
    registry: &Registry,
    tables: &ContentValueTables<'_>,
) -> u64 {
    let mut hasher = StateHasher::new();
    hasher.write_namespaced_id(id);

    let kind = classify_index(index, tables);
    hasher.write_u64(kind as u64);

    match kind {
        ContentTableKind::Terrain => {
            let terrain_kind = TerrainKind::from_index(index);
            write_terrain_fields(&mut hasher, tables.terrain, terrain_kind, registry);
        }
        ContentTableKind::Class => write_class_fields(&mut hasher, tables.class, index, registry),
        ContentTableKind::Skill => {
            write_skill_fields(&mut hasher, tables.skill, index, registry);
        }
        ContentTableKind::Subclass => {
            write_subclass_fields(&mut hasher, tables.subclass, index, registry)
        }
        ContentTableKind::Quest => write_quest_fields(&mut hasher, tables.quest, index, registry),
        ContentTableKind::Race => write_race_fields(&mut hasher, tables.race, index, registry),
        ContentTableKind::SpaceProfile => {
            write_space_profile_fields(&mut hasher, tables.space_profile, index);
        }
        ContentTableKind::Clip => write_clip_fields(&mut hasher, tables.clip, index),
        ContentTableKind::Trait => {
            write_trait_fields(&mut hasher, tables.trait_def, index, registry);
        }
        ContentTableKind::ResourcePool => {
            write_resource_pool_fields(&mut hasher, tables.resource_pool, index);
        }
        ContentTableKind::Item => {
            write_item_fields(&mut hasher, tables.item, index, registry);
        }
        ContentTableKind::XpCurve => write_xp_curve_fields(&mut hasher, tables.xp_curve, index),
        ContentTableKind::Formula => write_formula_fields(&mut hasher, tables.formula, index),
        ContentTableKind::WeaponCategory => {
            write_weapon_category_fields(&mut hasher, tables.weapon_category, index, registry);
        }
        ContentTableKind::DamageCategory => {
            write_damage_category_fields(&mut hasher, tables.damage_category, index, registry);
        }
        ContentTableKind::Weather => {
            write_weather_fields(&mut hasher, tables.weather, index);
        }
        ContentTableKind::Recipe => {
            write_recipe_fields(&mut hasher, tables.recipe, index, registry);
        }
        ContentTableKind::RecipeCategory => {
            write_recipe_category_fields(&mut hasher, tables.recipe_category, index, registry);
        }
        ContentTableKind::Tag => {
            write_tag_fields(&mut hasher, tables.tag, index);
        }
        ContentTableKind::Resource => {
            write_resource_fields(&mut hasher, tables.resource, index, registry);
        }
        ContentTableKind::Culture => {
            write_culture_fields(&mut hasher, tables.culture, index, registry);
        }
        ContentTableKind::Dialogue => {
            write_dialogue_fields(&mut hasher, tables.dialogue, index, registry);
        }
        ContentTableKind::DialogueNode => {
            write_dialogue_node_fields(&mut hasher, tables.dialogue_node, index, registry);
        }
        ContentTableKind::ModifierType => {
            // 加值类型没有任何字段（`ModifierTypeDef` 是空结构体，理由
            // 见 `crate::modifier_type` 模块文档「为什么一个字段都没
            // 有」一节），因此这里与 `Opaque` 一样只有 id 本身进哈希。
            // **但它与 `Opaque` 仍然不同**：函数顶部已经写过的那个
            // `kind as u64` 判别值不一样，于是「lostland:enhancement 是
            // 一条已注册的加值类型」与「它只是一个被 intern 过、谁也
            // 没定义的裸 id」折出的摘要不同——注册表被整个删掉这件事
            // 因此仍然可检测。
        }
        ContentTableKind::Opaque => {
            // 没有字段可哈希，只哈希 id 本身（已经在函数顶部写过）——
            // 与升级前的行为一致，这类内容"是否存在"仍然能被检测到,
            // 只是没有字段值可比对。
        }
    }

    hasher.finish()
}

/// 把一个可能不存在、也可能解析失败的 `ContentIndex` 引用混入哈希。
///
/// 三种判别值：`0` = `None`；`1` = `Some` 且成功解析成
/// `NamespacedId`（正常情形，混入解析出的字符串）；`2` = `Some` 但
/// 解析失败（理论不可达的边界情形，见模块文档「`ContentIndex` 字段」
/// 一节，不 panic，只是产出一个与其余两种情形都不同的摘要）。
fn write_optional_resolved(
    hasher: &mut StateHasher,
    index: Option<ContentIndex>,
    registry: &Registry,
) {
    match index {
        None => hasher.write_u64(0),
        Some(index) => match registry.resolve(index) {
            Some(id) => {
                hasher.write_u64(1);
                hasher.write_namespaced_id(id);
            }
            None => hasher.write_u64(2),
        },
    }
}

/// 把一份 `[ContentIndex]`（前置列表一类保序的引用集合）混入哈希——
/// 先混入长度，再逐项复用 [`write_optional_resolved`]，理由同
/// `ll_world::state::write_content_index_vec` 文档：`Vec`/slice 本身
/// 保序，不依赖任何哈希表遍历顺序（约束 C5）。
fn write_resolved_content_index_slice(
    hasher: &mut StateHasher,
    indices: &[ContentIndex],
    registry: &Registry,
) {
    hasher.write_u64(indices.len() as u64);
    for index in indices {
        write_optional_resolved(hasher, Some(*index), registry);
    }
}

/// 混入 [`crate::tag::TagDef`] 的全部字段（耐久标签批次新增）——
/// 目前只有一个字段：这条标签给物品带来的耐久磨损通道集合
/// （[`ll_sim::item::WearChannels`]），按它的底层位表示混入，与
/// [`write_slot_mask`] 混入 `SlotMask::bits` 完全同构。
///
/// 不接受 `&Registry` 参数：`TagDef` 一个 `ContentIndex` 字段都没有
/// （标签不引用任何其它内容），因此没有需要解析回 `NamespacedId` 的
/// 东西——与 `write_item_fields` 文档记的那条教训不冲突：那里的教训是
/// 「老表新增了 `ContentIndex` 字段就必须补上这个参数」，不是「所有
/// `write_*_fields` 都必须先要一个 `registry`」。真给 `TagDef` 加了引用
/// 别的内容的字段时，跟着补即可（编译器会在这里报参数不匹配）。
fn write_tag_fields(hasher: &mut StateHasher, table: &TagTable, index: ContentIndex) {
    let def = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_u64(u64::from(def.wear.bits()));
}

/// 把一份 [`BaseStats`] 混入哈希——七项属性（六项主属性 + 幸运，幸运
/// 并入 `AttributeKind` 批次新增字段）逐一混入，顺序与字段声明顺序
/// 一致。与 `ll_world::state::write_stats`（该 crate 内部私有）是同一种
/// 写法的独立实现：两者服务不同的哈希（世界状态摘要 vs. 内容值哈希），
/// 不适合跨 crate 共享同一个私有帮手函数。
fn write_base_stats(hasher: &mut StateHasher, stats: BaseStats) {
    hasher.write_i64(i64::from(stats.strength));
    hasher.write_i64(i64::from(stats.dexterity));
    hasher.write_i64(i64::from(stats.constitution));
    hasher.write_i64(i64::from(stats.intelligence));
    hasher.write_i64(i64::from(stats.willpower));
    hasher.write_i64(i64::from(stats.charisma));
    hasher.write_i64(i64::from(stats.luck));
}

/// 混入 [`ll_world::terrain::TerrainDef`] 的全部字段——`opens_into`
/// 解析成 `NamespacedId` 字符串（见模块文档「`ContentIndex` 字段」
/// 一节）。
fn write_terrain_fields(
    hasher: &mut StateHasher,
    table: &TerrainTable,
    kind: TerrainKind,
    registry: &Registry,
) {
    hasher.write_u64(u64::from(table.blocks_sight(kind)));
    hasher.write_u64(u64::from(table.blocks_move(kind)));
    hasher.write_u64(u64::from(table.move_cost(kind)));
    write_optional_resolved(
        hasher,
        table.opens_into(kind).map(|opens_into| opens_into.index()),
        registry,
    );
}

/// 混入 [`crate::class::ClassDef`] 的全部字段。
fn write_class_fields(
    hasher: &mut StateHasher,
    table: &ClassTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    hasher.write_u64(view.primary_attribute as u64);
    // `traits`（职业天赋接线批次新增）：与 `write_race_fields` 的同名
    // 字段逐字节同构——先写条数再逐条写 `(trait_id, unlock_level)`，
    // `trait_id` 解析成 `NamespacedId` 字符串（`ContentIndex` 数值本身
    // 依赖注册顺序，不是稳定的跨会话身份，见模块文档
    // 「`ContentIndex` 字段」一节）。本字段与本函数在同一批次里一起
    // 新增，不是又一次「老表新增字段忘了同步值哈希」的事后补救。
    hasher.write_u64(view.traits.len() as u64);
    for grant in view.traits {
        write_optional_resolved(hasher, Some(grant.trait_id), registry);
        hasher.write_i64(i64::from(grant.unlock_level));
    }
}

/// 混入 [`crate::subclass::SubclassDef`] 的全部字段——`traits[].trait_id`
/// 解析成 `NamespacedId` 字符串。
fn write_subclass_fields(
    hasher: &mut StateHasher,
    table: &SubclassTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    // `traits`（副职天赋接线批次新增）：与 `write_class_fields`/
    // `write_race_fields` 的同名字段逐字节同构——先写条数再逐条写
    // `(trait_id, unlock_level)`，`trait_id` 解析成 `NamespacedId`
    // 字符串（`ContentIndex` 数值本身依赖注册顺序，不是稳定的跨会话
    // 身份，见模块文档「`ContentIndex` 字段」一节）。长度前缀 `0` 也是
    // 一段此前不存在的字节，因此**每一个副职**的条目摘要都变了（即便
    // 它一条天赋都不授予）——理由同版本 15 那段。
    hasher.write_u64(view.traits.len() as u64);
    for grant in view.traits {
        write_optional_resolved(hasher, Some(grant.trait_id), registry);
        hasher.write_i64(i64::from(grant.unlock_level));
    }
    // 副职获得机制批次新增的第二列。`category` 混入的是**解析后的
    // `NamespacedId` 字符串**而不是 `ContentIndex` 的数值——与
    // `write_recipe_fields` 处理 `category` 同一条纪律（模块文档
    // 「`ContentIndex` 字段」一节）：数值会随 mod 集合变化重编号，
    // 用它当哈希输入会让「内容一个字没改、只是装载顺序变了」被误判成
    // 内容不一致。这里更省事——`CraftUnlockRule` 在注册期就把标识符
    // 一并存下来了（理由见 `ll_sim::subclass` 模块文档「计数键」
    // 一节），不需要回头查 `Registry`。
    match table.craft_unlock(index) {
        None => hasher.write_u64(0),
        Some(unlock) => {
            hasher.write_u64(1);
            hasher.write_namespaced_id(&unlock.category_id);
            hasher.write_u64(u64::from(unlock.threshold));
        }
    }
}

/// 混入 [`crate::race::RaceDef`] 的全部字段——`traits[].trait_id`
/// 解析成 `NamespacedId` 字符串。
///
/// `xp_reward`/`traits` 两个字段（等级与经验系统/天赋系统批次新增）是
/// 本次批次新补上的：ADR 0027 落地时 `RaceAttrs` 还没有这两个字段，
/// 此后两个批次给它加了字段，却都没有回来同步更新本函数——这正是
/// 「已覆盖的表也会随字段新增而漂移出真正的覆盖面」的真实案例，见
/// 模块文档最上方「起因」一节。
fn write_race_fields(
    hasher: &mut StateHasher,
    table: &RaceTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    write_base_stats(hasher, view.stat_modifiers);
    // 刻意保持 `write_i64`（而不是随字段类型改成 `write_u64`）：暗视
    // 从 `i32` 光照下限改成 `u32` 视野格数时，字段的**语义**变了，但
    // 混进哈希的字节形状必须逐字节不变——同一个数值经 `i64::from` 与
    // `u64::from` 写出的八个字节在非负区间上相同，而 `darkvision_cells`
    // 恒非负。这让本批次不构成「新增哈希输入」，
    // `CONTENT_HASH_ALGORITHM_VERSION` 因此不必递增（本批次改变的是
    // 内容取值，那是 `content_hash_of("lostland")` 该变的东西，不是量
    // 尺该变的东西——见该常量文档版本 12 末尾「注册新的内容条目本身
    // 不需要动这个常量」一段同一条判据）。
    hasher.write_i64(i64::from(view.darkvision_cells));
    hasher.write_u64(u64::from(view.footprint.0));
    hasher.write_u64(u64::from(view.footprint.1));
    hasher.write_u64(u64::from(view.lifespan_years));
    hasher.write_i64(view.xp_reward);
    hasher.write_u64(view.traits.len() as u64);
    for grant in view.traits {
        write_optional_resolved(hasher, Some(grant.trait_id), registry);
        hasher.write_i64(i64::from(grant.unlock_level));
    }
    // `starting_items`（NPC 生命周期批次新增）：与上面 `traits` 同一条
    // 覆盖纪律——`RaceAttrs` 新增字段而忘记回来同步本函数,正是本函数
    // 文档点名的既有漂移案例本身,这里补的是"新增字段的同时立刻同步值
    // 哈希",不是又一次事后补救。`(def, count)` 里的 `def` 同样要解析成
    // `NamespacedId` 字符串（与 `trait_id` 同一条理由，`ContentIndex`
    // 数值本身不是稳定的跨会话身份）。
    hasher.write_u64(view.starting_items.len() as u64);
    for &(def, count) in view.starting_items {
        write_optional_resolved(hasher, Some(def), registry);
        hasher.write_u64(u64::from(count));
    }
}

/// 混入 [`crate::skill::SkillDef`] 的全部字段——`owning_class`/
/// `prerequisites` 解析成 `NamespacedId` 字符串。
fn write_skill_fields(
    hasher: &mut StateHasher,
    table: &SkillTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    write_optional_resolved(hasher, view.owning_class, registry);
    write_resolved_content_index_slice(hasher, view.prerequisites, registry);
    hasher.write_u64(u64::from(view.cooldown_ticks));
    write_resource_cost(hasher, view.resource_cost, registry);
    write_skill_effect(hasher, view.effect);
}

/// 混入一个 [`ResourceCost`]——先写变体判别字节，两个变体互不混淆，
/// 与 `ll_world::state` 已确立的枚举哈希写法（先判别、后字段）同一
/// 种模式。
fn write_resource_cost(hasher: &mut StateHasher, cost: ResourceCost, registry: &Registry) {
    match cost {
        ResourceCost::None => hasher.write_u64(0),
        ResourceCost::Amount(kind, amount) => {
            hasher.write_u64(1);
            hasher.write_u64(kind as u64);
            hasher.write_u64(u64::from(amount));
        }
        // 资源池落地批次新增两个变体——判别值接着既有两档往后编号,不
        // 打乱 `None`/`Amount` 已经写死的 0/1,理由同模块文档「凡是
        // `*Attrs` 里声明的字段,一律参与哈希」一节:新变体同样是真实
        // 的内容变化,理应被这份哈希感知到。`PoolAmount` 携带
        // `ContentIndex`,按模块文档「`ContentIndex` 字段」一节解析成
        // 字符串再混入,不直接哈希裸索引数值。
        ResourceCost::PoolAmount(pool, amount) => {
            hasher.write_u64(2);
            write_optional_resolved(hasher, Some(pool), registry);
            hasher.write_u64(u64::from(amount));
        }
        ResourceCost::Blood(amount) => {
            hasher.write_u64(3);
            hasher.write_u64(u64::from(amount));
        }
        // 法术位落地批次新增第五个变体——判别值接着继续往后编号（4）,
        // 理由同上方 PoolAmount/Blood 注释。`SlotTier` 同样携带
        // `ContentIndex`,按同一条规则解析成字符串再混入；`min_tier`
        // 是纯数值,直接混入。
        ResourceCost::SlotTier(pool, min_tier) => {
            hasher.write_u64(4);
            write_optional_resolved(hasher, Some(pool), registry);
            hasher.write_u64(u64::from(min_tier));
        }
    }
}

/// 混入一个 [`SkillEffect`]，理由同 [`write_resource_cost`]。
fn write_skill_effect(hasher: &mut StateHasher, effect: SkillEffect) {
    match effect {
        SkillEffect::DealDamage { base } => {
            hasher.write_u64(0);
            hasher.write_i64(i64::from(base));
        }
        SkillEffect::RestoreResource { resource, base } => {
            hasher.write_u64(1);
            hasher.write_u64(resource as u64);
            hasher.write_i64(i64::from(base));
        }
        SkillEffect::TemporaryStatModifier {
            attribute,
            amount,
            duration_ticks,
        } => {
            hasher.write_u64(2);
            hasher.write_u64(attribute as u64);
            hasher.write_i64(i64::from(amount));
            hasher.write_u64(u64::from(duration_ticks));
        }
    }
}

/// 混入 [`crate::quest::QuestNodeDef`] 的全部字段——`prerequisites`
/// 解析成 `NamespacedId` 字符串。
fn write_quest_fields(
    hasher: &mut StateHasher,
    table: &QuestTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    write_resolved_content_index_slice(hasher, view.prerequisites, registry);
    write_quest_condition(hasher, view.condition, registry);
}

/// 混入一个 [`QuestCondition`]，理由同 [`write_resource_cost`]。
/// `KillCount::target_kind` 解析成 `NamespacedId` 字符串。
fn write_quest_condition(
    hasher: &mut StateHasher,
    condition: &QuestCondition,
    registry: &Registry,
) {
    match condition {
        QuestCondition::KillCount { target_kind, count } => {
            hasher.write_u64(0);
            write_optional_resolved(hasher, Some(*target_kind), registry);
            hasher.write_u64(u64::from(*count));
        }
        QuestCondition::Script(id) => {
            hasher.write_u64(1);
            hasher.write_namespaced_id(id);
        }
    }
}

/// 混入 [`crate::dialogue::DialogueAttrs`] 的全部字段（对话内容表批次
/// 新增）。
///
/// `speaker.profession`/`speaker.culture` 与 `root` 都是 `ContentIndex`，
/// 按 ADR 0027 的既有纪律**先解析回 id 字符串再混入**——否则哈希会依赖
/// mod 装载顺序。
fn write_dialogue_fields(
    hasher: &mut StateHasher,
    table: &DialogueTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    write_optional_resolved(hasher, Some(view.speaker.profession), registry);
    write_optional_resolved(hasher, view.speaker.culture, registry);
    write_optional_resolved(hasher, Some(view.root), registry);
}

/// 混入 [`crate::dialogue::DialogueNodeAttrs`] 的全部字段（对话内容表批次
/// 新增）。
///
/// `text_key` 与选项的 `text_key` 都是**本地化键**（字面
/// [`NamespacedId`]，不是 `ContentIndex`，见 [`crate::dialogue`] 模块文档
/// 末节），直接混入、不经 [`Registry::resolve`]——与
/// [`write_space_profile_fields`] 的 `reverb_tag` 同一种情形。
///
/// 选项按**书写顺序**逐条混入并先写条数：顺序本身是内容的一部分（玩家看到
/// 的第几行），两份只有选项次序不同的内容必须算出不同的摘要。
fn write_dialogue_node_fields(
    hasher: &mut StateHasher,
    table: &DialogueNodeTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.text_key);
    hasher.write_u64(view.options.len() as u64);
    for option in view.options {
        hasher.write_namespaced_id(&option.text_key);
        match option.next {
            DialogueNext::End => hasher.write_u64(0),
            DialogueNext::Node(target) => {
                hasher.write_u64(1);
                write_optional_resolved(hasher, Some(target), registry);
            }
        }
        hasher.write_u64(option.conditions.len() as u64);
        for condition in &option.conditions {
            write_dialogue_condition(hasher, condition, registry);
        }
    }
}

/// 混入一条 [`DialogueCondition`]，理由同 [`write_quest_condition`]：判别
/// 值 + 各自的参数，`ContentIndex` 一律先解析回 id 字符串。
///
/// 判别值 `0..=9` 与 [`DialogueCondition`] 的变体声明顺序一致，**新增谓词
/// 必须往后接、不挪既有值**（同 [`ContentTableKind`] 那条纪律）。
fn write_dialogue_condition(
    hasher: &mut StateHasher,
    condition: &DialogueCondition,
    registry: &Registry,
) {
    match condition {
        DialogueCondition::Affiliated(query) => {
            hasher.write_u64(0);
            write_affiliation_query(hasher, query, registry);
        }
        DialogueCondition::NotAffiliated(query) => {
            hasher.write_u64(1);
            write_affiliation_query(hasher, query, registry);
        }
        DialogueCondition::StandingAtLeast { query, value } => {
            hasher.write_u64(2);
            write_affiliation_query(hasher, query, registry);
            hasher.write_u64(*value as u64);
        }
        DialogueCondition::QuestCompleted(quest) => {
            hasher.write_u64(3);
            write_optional_resolved(hasher, Some(*quest), registry);
        }
        DialogueCondition::QuestNotCompleted(quest) => {
            hasher.write_u64(4);
            write_optional_resolved(hasher, Some(*quest), registry);
        }
        DialogueCondition::FlagSet(flag) => {
            hasher.write_u64(5);
            hasher.write_namespaced_id(flag);
        }
        DialogueCondition::FlagNotSet(flag) => {
            hasher.write_u64(6);
            hasher.write_namespaced_id(flag);
        }
        DialogueCondition::HasItem { item, count } => {
            hasher.write_u64(7);
            write_optional_resolved(hasher, Some(*item), registry);
            hasher.write_u64(u64::from(*count));
        }
        DialogueCondition::WalletAtLeast(value) => {
            hasher.write_u64(8);
            hasher.write_u64(*value as u64);
        }
        DialogueCondition::IsRace(race) => {
            hasher.write_u64(9);
            write_optional_resolved(hasher, Some(*race), registry);
        }
    }
}

/// 混入一条归属查询（谓词参数的公共一半）。
fn write_affiliation_query(
    hasher: &mut StateHasher,
    query: &AffiliationQuery,
    registry: &Registry,
) {
    // `AffiliationKind` 是 `ll-world` 的公开枚举、没有显式判别值，这里按
    // 声明顺序显式编号，**不用 `as u64`**：那会把「往枚举中间插一个变体」
    // 这件事悄悄变成一次哈希语义改动，而这里看得见。
    hasher.write_u64(match query.kind {
        AffiliationKind::Faction => 0,
        AffiliationKind::Religion => 1,
        AffiliationKind::Guild => 2,
        AffiliationKind::Culture => 3,
        AffiliationKind::Family => 4,
    });
    write_optional_resolved(hasher, query.org, registry);
}

/// 混入 [`ll_world::space_profile::SpaceProfile`] 的全部字段
/// （内容值哈希覆盖面扩展批次新增）——`reverb_tag` 是字面
/// `NamespacedId`（不是 `ContentIndex`），直接混入，不经过
/// `Registry::resolve`，理由见模块文档「`ContentIndex` 字段」一节
/// 倒数第二段。
fn write_space_profile_fields(
    hasher: &mut StateHasher,
    table: &SpaceProfileTable,
    index: ContentIndex,
) {
    hasher.write_i64(i64::from(table.ambient_light_floor(index)));
    hasher.write_u64(u64::from(table.exposed_to_sky(index)));
    hasher.write_i64(i64::from(table.base_temperature(index)));
    hasher.write_u64(u64::from(table.diggable(index)));
    hasher.write_u64(u64::from(table.buildable(index)));
    match table.reverb_tag(index) {
        None => hasher.write_u64(0),
        Some(tag) => {
            hasher.write_u64(1);
            hasher.write_namespaced_id(&tag);
        }
    }
}

/// 混入 [`ll_world::weather::WeatherDef`] 的全部字段（天气系统批次
/// 新增）——`display_name_key` 是字面 `NamespacedId`（不是
/// `ContentIndex`），直接混入，不经过 `Registry::resolve`，与
/// [`write_space_profile_fields`] 的 `reverb_tag` 同一条理由（模块文档
/// 「`ContentIndex` 字段」一节倒数第二段）。
///
/// 四个季节权重**逐个**混入，不先求和：两种权重分布 `[10,0,0,0]` 与
/// `[0,10,0,0]` 的和相同，玩法上却是「只在春天下」与「只在夏天下」两
/// 种截然不同的内容，求和会让它们的摘要撞在一起，正是 ADR 0022/0027
/// 「哈希覆盖字段值，不只 id 集合」这条要求想避免的那类塌缩。
fn write_weather_fields(hasher: &mut StateHasher, table: &WeatherTable, index: ContentIndex) {
    match table.display_name_key(index) {
        None => hasher.write_u64(0),
        Some(key) => {
            hasher.write_u64(1);
            hasher.write_namespaced_id(&key);
        }
    }
    hasher.write_i64(i64::from(table.light_scale(index)));
    hasher.write_i64(i64::from(table.sight_scale(index)));
    // 温度系统批次新增的第四列——见 CONTENT_HASH_ALGORITHM_VERSION 的
    // 「版本 10」一段。位置排在两个乘数之后、季节权重之前，与
    // `WeatherAttrs` 的字段声明顺序一致：混入顺序本身就是哈希输入，
    // 让它跟着结构体走可以让「读结构体就知道混入了什么、按什么顺序」
    // 这件事不需要额外记忆。
    hasher.write_i64(i64::from(table.temperature_offset(index)));
    for weight in table.season_weights(index) {
        hasher.write_u64(u64::from(weight));
    }
}

/// 混入 [`ll_world::resource::ResourceAttrs`] 的全部字段（资源点批次
/// 新增）。
///
/// `display_name_key` 是字面 `NamespacedId`，直接混入；
/// `source_terrain` 是 `ContentIndex`，**必须经 `Registry::resolve`
/// 换成命名空间字符串**再混入——索引本身依赖 mod 装载顺序，把它当成
/// 哈希输入会让「同一批内容、装载顺序不同」产出不同的哈希，那是本
/// 模块「`ContentIndex` 字段」一节点名的那类错误。解析不出来时混入一个
/// 与任何合法 id 都不可能相等的判别字节（`0`），与其余几处同一条
/// 兜底纪律。
fn write_resource_fields(
    hasher: &mut StateHasher,
    table: &ResourceTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let kind = ResourceKind::from_index(index);
    match table.display_name_key(kind) {
        None => hasher.write_u64(0),
        Some(key) => {
            hasher.write_u64(1);
            hasher.write_namespaced_id(&key);
        }
    }
    match table
        .source_terrain(kind)
        .and_then(|terrain| registry.resolve(terrain.index()))
    {
        None => hasher.write_u64(0),
        Some(id) => {
            hasher.write_u64(1);
            hasher.write_namespaced_id(id);
        }
    }
    // 大类（资源两层分类批次新增）：混的是内容文件里写的那个**字符串**
    // （`ResourceCategory::as_str`），不是枚举的判别数值——与本模块对
    // `ContentIndex` 的处理同一条道理（ADR 0027「哈希混字符串不混会
    // 漂移的整数」）：将来在枚举中间插一个大类不该被误判成「全世界的
    // 资源都变了」。
    match table.category(kind) {
        None => hasher.write_u64(0),
        Some(category) => {
            hasher.write_u64(1);
            hasher.write_len_prefixed_bytes(category.as_str().as_bytes());
        }
    }
    hasher.write_u64(u64::from(table.abundance(kind)));
    hasher.write_u64(u64::from(table.residents_supported(kind)));
    hasher.write_u64(u64::from(table.settlement_draw(kind)));
    hasher.write_u64(u64::from(table.exhaustible(kind)));
}

/// 混入 [`ll_world::culture::CultureAttrs`] 的全部字段（文化批次
/// 新增）。
///
/// 三类字段三种处理，判据全部是本模块「`ContentIndex` 字段」一节那条
/// ——**会随装载顺序漂移的整数一律先换回命名空间字符串**：
///
/// - `display_name_key` 是字面 `NamespacedId`，直接混；
/// - `economy` 混的是内容文件里写的那个**字符串**
///   （`ResourceCategory::as_str`），不是枚举判别值，与
///   [`write_resource_fields`] 的同名处理逐字相同；
/// - `home_terrain`/`wall_terrain`/`founder_races[].race`/
///   `hostility[].culture` 都是 `ContentIndex`，一律经
///   `Registry::resolve` 换成 id 再混。解析不出来时混一个与任何合法
///   id 都不可能相等的判别字节（`0`）。
///
/// 两个列表**先混长度再逐项混**，顺序取声明顺序——那是内容文件里的
/// 书写顺序，不来自任何哈希容器（约束 C5）。刻意**不排序**：调换
/// `founder_races` 里两条的先后会改变加权抽取的取值序列，也就是改变
/// 世界，那本来就该被算成一次内容改动。
fn write_culture_fields(
    hasher: &mut StateHasher,
    table: &CultureTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let kind = CultureKind::from_index(index);
    match table.display_name_key(kind) {
        None => hasher.write_u64(0),
        Some(key) => {
            hasher.write_u64(1);
            hasher.write_namespaced_id(&key);
        }
    }
    match table.economy(kind) {
        None => hasher.write_u64(0),
        Some(economy) => {
            hasher.write_u64(1);
            hasher.write_len_prefixed_bytes(economy.as_str().as_bytes());
        }
    }
    for terrain in [table.home_terrain(kind), table.wall_terrain(kind)] {
        match terrain.and_then(|terrain| registry.resolve(terrain.index())) {
            None => hasher.write_u64(0),
            Some(id) => {
                hasher.write_u64(1);
                hasher.write_namespaced_id(id);
            }
        }
    }
    let founders = table.founder_races(kind);
    hasher.write_u64(founders.len() as u64);
    for (race, weight) in founders {
        match registry.resolve(*race) {
            None => hasher.write_u64(0),
            Some(id) => {
                hasher.write_u64(1);
                hasher.write_namespaced_id(id);
            }
        }
        hasher.write_u64(u64::from(*weight));
    }
    // 敌对表按注册顺序逐条查，不能直接拿一个 `&[..]`——`CultureTable`
    // 只暴露 `hostility(攻, 守)` 这个查询（它是有向表的正确形状），
    // 因此这里遍历全表、把这一行完整地混进去。条数由 `registered()`
    // 定死，与哈希容器无关。
    hasher.write_u64(table.registered().len() as u64);
    for target in table.registered() {
        match registry.resolve(target.index()) {
            None => hasher.write_u64(0),
            Some(id) => {
                hasher.write_u64(1);
                hasher.write_namespaced_id(id);
            }
        }
        hasher.write_u64(u64::from(table.hostility(Some(kind), Some(*target))));
    }
    // 建筑类型（版本 29）：形状与上面的 `founder_races` 逐字一致——先条数
    // 再逐条。家具那一层也一样：先条数，再逐条写「物品 id + 件数」。
    let buildings = table.buildings(kind);
    hasher.write_u64(buildings.len() as u64);
    for template in buildings {
        hasher.write_u64(u64::from(template.weight));
        hasher.write_u64(template.furniture.len() as u64);
        for (item, count) in &template.furniture {
            match registry.resolve(*item) {
                None => hasher.write_u64(0),
                Some(id) => {
                    hasher.write_u64(1);
                    hasher.write_namespaced_id(id);
                }
            }
            hasher.write_u64(u64::from(*count));
        }
    }
}

/// 混入一段 [`ll_render::anim::Clip`] 的全部字段（内容值哈希覆盖面
/// 扩展批次新增）——
/// 与另外几张表结构性不同（[`crate::clip`] 模块文档）：`ClipTable`
/// 直接存 `ll_render::anim::Clip`，没有独立的 `*Attrs`/`*View` 类型，
/// 本函数因此直接对 `Clip` 本身取字段。
fn write_clip_fields(hasher: &mut StateHasher, table: &ClipTable, index: ContentIndex) {
    let clip = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_u64(clip.frames.len() as u64);
    for frame in &clip.frames {
        hasher.write_len_prefixed_bytes(frame.as_bytes());
    }
    hasher.write_u64(u64::from(clip.frames_per_step));
    hasher.write_u64(u64::from(clip.looping));
    hasher.write_u64(u64::from(clip.exit_grace_frames));
}

/// 混入 [`crate::trait_def::TraitDef`] 的全部字段（内容值哈希覆盖面
/// 扩展批次新增）——`granted_skills` 解析成 `NamespacedId` 字符串列表，
/// `rule_modifiers`/`granted_resource_pools` 递归混入各自的变体与
/// `ContentIndex` 字段。
fn write_trait_fields(
    hasher: &mut StateHasher,
    table: &TraitTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    write_resolved_content_index_slice(hasher, view.granted_skills, registry);
    hasher.write_u64(view.stat_modifiers.len() as u64);
    for (attribute, amount) in view.stat_modifiers {
        hasher.write_u64(*attribute as u64);
        hasher.write_i64(i64::from(*amount));
    }
    hasher.write_u64(view.rule_modifiers.len() as u64);
    for typed in view.rule_modifiers {
        write_typed_rule_modifier(hasher, typed, registry);
    }
    hasher.write_u64(view.granted_resource_pools.len() as u64);
    for grant in view.granted_resource_pools {
        write_optional_resolved(hasher, Some(grant.pool), registry);
        write_capacity_formula(hasher, &grant.capacity);
    }
}

/// 混入一个 [`TypedRuleModifier`]——**先写加值类型，再写修正本身**
/// （加值类型批次新增，见 [`CONTENT_HASH_ALGORITHM_VERSION`] 文档
/// 「版本 16」一节）。
///
/// 类型走 [`write_optional_resolved`]（解析成 `NamespacedId` 字符串,
/// 不混入 `ContentIndex` 数值本身——注册顺序不该进哈希），`None`
/// 与「声明了某个类型」因此天然可区分：前者写的是缺席判别字节。
fn write_typed_rule_modifier(
    hasher: &mut StateHasher,
    typed: &TypedRuleModifier,
    registry: &Registry,
) {
    write_optional_resolved(hasher, typed.modifier_type, registry);
    write_rule_modifier(hasher, &typed.modifier, registry);
}

/// 混入一个 [`RuleModifier`]，理由同 [`write_resource_cost`]。
/// `Resistance.damage_category` 解析成 `NamespacedId` 字符串；
/// `Advantage`/`Disadvantage.check_context` 是字面 `NamespacedId`，
/// 直接混入。
fn write_rule_modifier(hasher: &mut StateHasher, modifier: &RuleModifier, registry: &Registry) {
    match modifier {
        RuleModifier::Resistance {
            damage_category,
            damage_reduction,
        } => {
            hasher.write_u64(0);
            write_optional_resolved(hasher, Some(*damage_category), registry);
            hasher.write_i64(i64::from(*damage_reduction));
        }
        RuleModifier::RerollOnce { value } => {
            hasher.write_u64(1);
            hasher.write_i64(i64::from(*value));
        }
        RuleModifier::Advantage { check_context } => {
            hasher.write_u64(2);
            hasher.write_namespaced_id(check_context);
        }
        RuleModifier::Disadvantage { check_context } => {
            hasher.write_u64(3);
            hasher.write_namespaced_id(check_context);
        }
        RuleModifier::SneakAttack {
            sneak_modifier,
            extra_damage,
        } => {
            hasher.write_u64(4);
            hasher.write_i64(i64::from(*sneak_modifier));
            hasher.write_i64(i64::from(*extra_damage));
        }
        // 判别值 5/6（盗贼被动两分批次）：接着既有的 0..=4 往后编号，
        // 不打乱任何已经写死的判别值，同本模块「判别值接着既有档往后
        // 编号」的一贯纪律。
        //
        // 判定系统落地批次**只改了载荷的含义，没有改判别值**：同一段
        // 字节此前表示「从触发概率上减掉 400‰」，现在表示「在判定里给
        // 隐蔽方加 400 点」——同一个量纲换代，与同一次合并里收窄
        // `damage_reduction` 值域（版本 21 ①）是同一类，因此该由
        // `CONTENT_HASH_ALGORITHM_VERSION` 递增来表达，不该靠挪判别值
        // 冒充（挪判别值会让「同一条内容在新旧两版下算出不同摘要」这件
        // 事发生两次，一次来自版本号一次来自判别值，反而看不清是哪一次
        // 改动引起的）。
        RuleModifier::InspectionSuspicion {
            inconspicuous_modifier,
        } => {
            hasher.write_u64(5);
            hasher.write_i64(i64::from(*inconspicuous_modifier));
        }
        RuleModifier::InspectionConcealment {
            concealment_modifier,
        } => {
            hasher.write_u64(6);
            hasher.write_i64(i64::from(*concealment_modifier));
        }
        // 判别值 8（易伤与减伤对称批次新增，取 7；本批次改成 8）。
        // 写入的两个字段与判别值 0（`Resistance`）逐字同构——**判别值
        // 本身就是两者的唯一区分**，这也正是它必须是一个独立变体而不是
        // 负减伤的哈希侧后果：`减伤 -4` 与 `易伤 4` 此前会编码成同一条
        // 规则的两个数值，现在是两条不同的规则。
        //
        // # 为什么改号
        //
        // 与本变体自身无关，是一次**撞车**：本变体在分支 `wt-dice` 上
        // 取了 7，而同一时间 `main` 合入的 `wt-craftyield` 给
        // `RuleModifier` 加的 `CraftYield` 也取了 7。两个判别值必须
        // 互不相同——相同的话，一条「易伤 4 点」与一条「产出加成 4」
        // 会编码成逐位相同的字节，内容摘要再也分不开它们，而分开它们
        // 正是判别值存在的全部理由。
        //
        // 改的是本变体而不是 `CraftYield`：后者已经在 `main` 上，改它
        // 要动一条已经发布的编号；本变体还在分支上没合，改它只动本分支
        // 自己。「后到的那个让路」是唯一不牵动别人的选择。
        //
        // 代价如实记下：`examplemod:acid_hide` 声明了易伤，它的条目
        // 摘要因此改变。这是正当变化——它记录的正是「这条规则的编码
        // 变了」。
        RuleModifier::Vulnerability {
            damage_category,
            damage_increase,
        } => {
            hasher.write_u64(8);
            write_optional_resolved(hasher, Some(*damage_category), registry);
            hasher.write_i64(i64::from(*damage_increase));
        }
        // 判别值 7（制作类副职奖励批次）：接着既有的 0..=6 往后编号,
        // 不打乱任何已经写死的判别值。`category` 指向配方类别表,
        // 与 `Resistance.damage_category` 同一条处理——解析回完整
        // `NamespacedId` 字符串再混入，不混 `ContentIndex` 数值本身
        // （ADR 0027 + 本模块「`ContentIndex` 字段」一节）。
        RuleModifier::CraftYield {
            category,
            bonus_product_count,
        } => {
            hasher.write_u64(7);
            write_optional_resolved(hasher, Some(*category), registry);
            hasher.write_i64(i64::from(*bonus_product_count));
        }
    }
}

/// 混入一个 [`CapacityFormula`]，理由同 [`write_resource_cost`]。
fn write_capacity_formula(hasher: &mut StateHasher, formula: &CapacityFormula) {
    match formula {
        CapacityFormula::Fixed(amount) => {
            hasher.write_u64(0);
            hasher.write_u64(u64::from(*amount));
        }
        // `BTreeMap` 遍历按键升序，不依赖插入顺序（约束 C5）。
        CapacityFormula::ByLevel(levels) => {
            hasher.write_u64(1);
            hasher.write_u64(levels.len() as u64);
            for (level, value) in levels {
                hasher.write_u64(u64::from(*level));
                write_capacity_value(hasher, value);
            }
        }
    }
}

/// 混入一个 [`CapacityValue`]，理由同 [`write_resource_cost`]。
fn write_capacity_value(hasher: &mut StateHasher, value: &CapacityValue) {
    match value {
        CapacityValue::Scalar(amount) => {
            hasher.write_u64(0);
            hasher.write_u64(u64::from(*amount));
        }
        CapacityValue::Tiered(tiers) => {
            hasher.write_u64(1);
            hasher.write_u64(tiers.len() as u64);
            for tier in tiers {
                hasher.write_u64(u64::from(*tier));
            }
        }
    }
}

/// 混入 [`crate::resource_pool::ResourcePoolDef`] 的全部字段（内容值
/// 哈希覆盖面扩展批次新增）。
fn write_resource_pool_fields(
    hasher: &mut StateHasher,
    table: &ResourcePoolTable,
    index: ContentIndex,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    write_resource_pool_shape(hasher, view.shape);
    write_regen_rule(hasher, view.regen_rule);
}

/// 混入一个 [`ResourcePoolShape`]，理由同 [`write_resource_cost`]。
fn write_resource_pool_shape(hasher: &mut StateHasher, shape: ResourcePoolShape) {
    match shape {
        ResourcePoolShape::Scalar => hasher.write_u64(0),
        ResourcePoolShape::TieredSlots { tier_count } => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(tier_count));
        }
    }
}

/// 混入一个 [`RegenRule`]，理由同 [`write_resource_cost`]。
fn write_regen_rule(hasher: &mut StateHasher, rule: RegenRule) {
    match rule {
        RegenRule::None => hasher.write_u64(0),
        RegenRule::OnTurnStart { amount } => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(amount));
        }
        RegenRule::OnRest { amount } => {
            hasher.write_u64(2);
            write_rest_recovery_amount(hasher, amount);
        }
    }
}

/// 混入一个 [`RestRecoveryAmount`]，理由同 [`write_resource_cost`]。
fn write_rest_recovery_amount(hasher: &mut StateHasher, amount: RestRecoveryAmount) {
    match amount {
        RestRecoveryAmount::Full => hasher.write_u64(0),
        RestRecoveryAmount::Amount(value) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(value));
        }
    }
}

/// 混入 [`crate::item::ItemDef`] 的全部字段（内容值哈希覆盖面扩展
/// 批次新增，伤害公式引擎批次追加 `damage_formula`，伤害类别/抗性
/// 接线批次追加 `damage_category`）——`base_weight`/
/// `base_price` 是 `Milli` 定点整数（`pub struct Milli(pub i64)`），
/// 按普通 `i64` 处理；`use_effect` 复用既有的 [`write_skill_effect`]
/// （与 `SkillDef.effect` 共用同一个类型，见
/// [`crate::item::ItemDef::use_effect`] 文档「为什么复用 `SkillEffect`」
/// 一节）。
///
/// # 现在接受 `&Registry` 参数——上一批「老表新增字段也会漏」的直接
/// 教训
///
/// 升级前的文档曾断言"`ItemDef` 当前没有任何 `ContentIndex` 字段……
/// 与其余表统一接受 `registry` 参数不同，如实按实际需要签名"——伤害
/// 公式引擎批次给 `ItemDef` 新增了 `damage_formula: Option<ContentIndex>`
/// 字段，这条断言的前提不再成立：本函数现在与其余表一样需要
/// `Registry::resolve` 把 `ContentIndex` 换回 `NamespacedId` 字符串再
/// 混入（模块文档「`ContentIndex` 字段」一节），不能再省略这个参数。
fn write_item_fields(
    hasher: &mut StateHasher,
    table: &ItemTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    hasher.write_u64(u64::from(view.stack_limit));
    hasher.write_i64(view.base_weight.0);
    hasher.write_i64(view.base_price.0);
    match view.max_durability {
        None => hasher.write_u64(0),
        Some(value) => {
            hasher.write_u64(1);
            hasher.write_i64(i64::from(value));
        }
    }
    write_slot_mask(hasher, view.equip_mask);
    hasher.write_u64(view.stat_bonuses.len() as u64);
    for bonus in view.stat_bonuses {
        write_stat_bonus(hasher, bonus);
    }
    match view.use_effect {
        None => hasher.write_u64(0),
        Some(effect) => {
            hasher.write_u64(1);
            write_skill_effect(hasher, effect);
        }
    }
    hasher.write_i64(i64::from(view.penetration.flat));
    hasher.write_i64(i64::from(view.penetration.permille));
    write_optional_resolved(hasher, view.damage_formula, registry);
    write_optional_resolved(hasher, view.damage_category, registry);
    // 耐久标签批次新增的 `ItemDef.tags`——先写条数再逐条把 `ContentIndex`
    // 解析回 `NamespacedId` 字符串混入（模块文档「`ContentIndex` 字段」
    // 一节：绝不混入下标本身，下标依赖装载顺序）。长度前缀即便是 0 也要
    // 写，理由同上面 `max_durability` 的 `None` 判别字节。
    //
    // 派生列 `wear_channels` 刻意不混入——见
    // `CONTENT_HASH_ALGORITHM_VERSION` 文档「版本 13」一节。
    write_resolved_content_index_slice(hasher, view.tags, registry);
    // 抗性多来源聚合批次新增的 `ItemDef.rule_modifiers`——先写条数再逐条
    // 递归，与 `write_trait_fields` 混入 `TraitDef.rule_modifiers` 完全
    // 同构（同一个 `write_rule_modifier`、同一套变体判别值）：装备与
    // 天赋声明的是同一种载荷，没有理由为它们各写一套编码。
    hasher.write_u64(view.rule_modifiers.len() as u64);
    for typed in view.rule_modifiers {
        write_typed_rule_modifier(hasher, typed, registry);
    }
    // 配方发现批次新增的 `ItemDef.taught_recipes`——与上面 `tags` 那条
    // 逐字同构（先条数、再逐条把 `ContentIndex` 解析回 `NamespacedId`
    // 字符串；绝不混入下标本身，下标依赖装载顺序）。长度前缀即便是 0
    // 也要写，理由同 `max_durability` 的 `None` 判别字节。
    write_resolved_content_index_slice(hasher, view.taught_recipes, registry);
    // 未鉴定物品批次新增的 `ItemDef.requires_identification` 与
    // `ItemDef.study_experience`——布尔混成 0/1 一个字节位（手法同
    // `write_recipe_fields` 的 `requires_discovery`），经验值按普通
    // `i64`。两条即便取默认值（`false` / `0`）也照写，理由同上面
    // `max_durability` 的 `None` 判别字节：「没有声明」与「声明成默认
    // 值」在值哈希里必须是同一段字节（它们本来就是同一件事），但这一
    // 段本身此前**不存在**，因此每一件物品的条目摘要都会变——那正是
    // `CONTENT_HASH_ALGORITHM_VERSION` 必须递增到 16 的原因。
    hasher.write_u64(u64::from(view.requires_identification));
    hasher.write_i64(view.study_experience);
    // 盲盒批次新增的 `ItemDef.blind_box_pool`——先写条数，再逐条把产出
    // 物的 `ContentIndex` 解析回 `NamespacedId` 字符串（绝不混入下标
    // 本身，下标依赖装载顺序），后接数量与权重两个整数。长度前缀即便
    // 是 0 也要写，理由同上。
    hasher.write_u64(view.blind_box_pool.len() as u64);
    for entry in view.blind_box_pool {
        write_optional_resolved(hasher, Some(entry.item), registry);
        hasher.write_u64(u64::from(entry.count));
        hasher.write_u64(u64::from(entry.weight));
    }
    // 家具层批次新增的 `ItemDef.furniture`——布尔混成 0/1 一个字节位
    // （手法同上面 `requires_identification`）。取默认值 `false` 的
    // 物品也照写，理由同上：「没有声明」与「声明成 false」在值哈希里
    // 必须是同一段字节，但这一段本身此前**不存在**，因此每一件物品的
    // 条目摘要都会变——那正是 `CONTENT_HASH_ALGORITHM_VERSION` 必须
    // 递增到 24 的原因。
    hasher.write_u64(u64::from(view.furniture));
}

/// 混入一个 [`SlotMask`]——直接混入底层位表示
/// （[`SlotMask::bits`](ll_world::item::SlotMask::bits)），装备占位
/// 掩码不含 `ContentIndex`，不需要经过 `Registry::resolve`。
fn write_slot_mask(hasher: &mut StateHasher, mask: SlotMask) {
    hasher.write_u64(u64::from(mask.bits()));
}

/// 混入一个 [`StatBonus`]，理由同 [`write_resource_cost`]。
fn write_stat_bonus(hasher: &mut StateHasher, bonus: &StatBonus) {
    match bonus.target {
        StatTarget::Attribute(attribute) => {
            hasher.write_u64(0);
            hasher.write_u64(attribute as u64);
        }
        StatTarget::Armor => hasher.write_u64(1),
        // 判别值 2，温度系统批次新增——见 CONTENT_HASH_ALGORITHM_VERSION
        // 的「版本 10」一段。新增判别值而不是复用 0/1：一件绝缘值 90
        // 的斗篷与一件护甲 90 的胸甲若折出同一个摘要，换装的内容变化
        // 就会在值哈希里塌缩掉，正是 ADR 0022/0027 要防的那类。
        StatTarget::Insulation => hasher.write_u64(2),
    }
    hasher.write_i64(i64::from(bonus.amount));
}

/// 混入 [`ll_sim::xp_curve::XpCurveDef`] 的全部字段（内容值哈希覆盖面
/// 扩展批次新增）——**除 `id` 外**：`id` 字段的语义是"这条曲线自己的
/// 索引"，与 [`entry_value_digest`] 顶部已经写过的同一个 id 完全重复，
/// 见模块文档「`ContentIndex` 字段」一节倒数第一段。
fn write_xp_curve_fields(hasher: &mut StateHasher, table: &XpCurveTable, index: ContentIndex) {
    let def = table
        .get(index)
        .expect("调用方已确认 classify_index 判定为 XpCurve，get 必返回 Some");
    hasher.write_i64(def.base_requirement);
    hasher.write_u64(def.instructions.len() as u64);
    for op in &def.instructions {
        write_xp_curve_op(hasher, op);
    }
}

/// 混入一个 [`XpCurveOperand`]，理由同 [`write_resource_cost`]——四个
/// 变体均不含 `ContentIndex`/浮点，直接混入。
fn write_xp_curve_operand(hasher: &mut StateHasher, operand: &XpCurveOperand) {
    match operand {
        XpCurveOperand::Const(value) => {
            hasher.write_u64(0);
            hasher.write_i64(*value);
        }
        XpCurveOperand::Local(slot) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(*slot));
        }
        XpCurveOperand::Level => hasher.write_u64(2),
        XpCurveOperand::PrevRequirement => hasher.write_u64(3),
    }
}

/// 混入一个 [`XpCurveCond`]，理由同 [`write_resource_cost`]。
fn write_xp_curve_cond(hasher: &mut StateHasher, cond: &XpCurveCond) {
    let (discriminant, a, b) = match cond {
        XpCurveCond::Lt(a, b) => (0, a, b),
        XpCurveCond::Le(a, b) => (1, a, b),
        XpCurveCond::Gt(a, b) => (2, a, b),
        XpCurveCond::Ge(a, b) => (3, a, b),
        XpCurveCond::Eq(a, b) => (4, a, b),
        XpCurveCond::Ne(a, b) => (5, a, b),
    };
    hasher.write_u64(discriminant);
    write_xp_curve_operand(hasher, a);
    write_xp_curve_operand(hasher, b);
}

/// 混入一个 [`XpCurveOp`]，理由同 [`write_resource_cost`]。
fn write_xp_curve_op(hasher: &mut StateHasher, op: &XpCurveOp) {
    match op {
        XpCurveOp::Ref(operand) => {
            hasher.write_u64(0);
            write_xp_curve_operand(hasher, operand);
        }
        XpCurveOp::Add(a, b) => {
            hasher.write_u64(1);
            write_xp_curve_operand(hasher, a);
            write_xp_curve_operand(hasher, b);
        }
        XpCurveOp::Sub(a, b) => {
            hasher.write_u64(2);
            write_xp_curve_operand(hasher, a);
            write_xp_curve_operand(hasher, b);
        }
        XpCurveOp::Mul(a, b) => {
            hasher.write_u64(3);
            write_xp_curve_operand(hasher, a);
            write_xp_curve_operand(hasher, b);
        }
        XpCurveOp::Div(a, b) => {
            hasher.write_u64(4);
            write_xp_curve_operand(hasher, a);
            write_xp_curve_operand(hasher, b);
        }
        XpCurveOp::MulPermille(a, b) => {
            hasher.write_u64(5);
            write_xp_curve_operand(hasher, a);
            write_xp_curve_operand(hasher, b);
        }
        XpCurveOp::Min(a, b) => {
            hasher.write_u64(6);
            write_xp_curve_operand(hasher, a);
            write_xp_curve_operand(hasher, b);
        }
        XpCurveOp::Max(a, b) => {
            hasher.write_u64(7);
            write_xp_curve_operand(hasher, a);
            write_xp_curve_operand(hasher, b);
        }
        XpCurveOp::Select {
            cond,
            if_true,
            if_false,
        } => {
            hasher.write_u64(8);
            write_xp_curve_cond(hasher, cond);
            write_xp_curve_operand(hasher, if_true);
            write_xp_curve_operand(hasher, if_false);
        }
    }
}

/// 混入 [`ll_sim::formula::FormulaDef`] 的全部字段（伤害公式引擎批次
/// 新增）——**除 `id` 外**，理由同 [`write_xp_curve_fields`] 对
/// `XpCurveDef.id` 的处理（`entry_value_digest` 顶部已经写过同一个
/// id，重复哈希只是冗余）。`needs_rng` 是编译期从 `instructions` 派生
/// 出的布尔（`damage_formulas.json5`
/// 文档），不独立混入——它是 `instructions` 内容的纯函数，`instructions`
/// 本身变化时它自动同步变化，不混入不会漏掉任何真实的内容差异，混入
/// 反而是「同一份信息哈希两次」。
fn write_formula_fields(hasher: &mut StateHasher, table: &FormulaTable, index: ContentIndex) {
    let def = table
        .get(index)
        .expect("调用方已确认 classify_index 判定为 Formula，get 必返回 Some");
    hasher.write_u64(def.instructions.len() as u64);
    for op in &def.instructions {
        write_formula_op(hasher, op);
    }
}

/// 混入一个 [`FormulaOperand`]，理由同 [`write_resource_cost`]——七个
/// 变体均不含浮点；`AttributeModifier` 携带 [`ll_world::entity::AttributeKind`]，
/// 是一个封闭枚举（不是 `ContentIndex`），直接混入判别值,不需要经过
/// `Registry::resolve`。
fn write_formula_operand(hasher: &mut StateHasher, operand: &FormulaOperand) {
    match operand {
        FormulaOperand::Const(value) => {
            hasher.write_u64(0);
            hasher.write_i64(*value);
        }
        FormulaOperand::Local(slot) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(*slot));
        }
        FormulaOperand::AttackPower => hasher.write_u64(2),
        FormulaOperand::Defense => hasher.write_u64(3),
        FormulaOperand::PenetrationFlat => hasher.write_u64(4),
        FormulaOperand::PenetrationPermille => hasher.write_u64(5),
        FormulaOperand::AttributeModifier(kind) => {
            hasher.write_u64(6);
            hasher.write_u64(*kind as u64);
        }
        FormulaOperand::Crit => hasher.write_u64(7),
    }
}

/// 混入一个 [`FormulaCond`]，理由同 [`write_resource_cost`]。
fn write_formula_cond(hasher: &mut StateHasher, cond: &FormulaCond) {
    let (discriminant, a, b) = match cond {
        FormulaCond::Lt(a, b) => (0, a, b),
        FormulaCond::Le(a, b) => (1, a, b),
        FormulaCond::Gt(a, b) => (2, a, b),
        FormulaCond::Ge(a, b) => (3, a, b),
        FormulaCond::Eq(a, b) => (4, a, b),
        FormulaCond::Ne(a, b) => (5, a, b),
    };
    hasher.write_u64(discriminant);
    write_formula_operand(hasher, a);
    write_formula_operand(hasher, b);
}

/// 混入一个 [`FormulaOp`]，理由同 [`write_resource_cost`]。
fn write_formula_op(hasher: &mut StateHasher, op: &FormulaOp) {
    match op {
        FormulaOp::Ref(operand) => {
            hasher.write_u64(0);
            write_formula_operand(hasher, operand);
        }
        FormulaOp::Add(a, b) => {
            hasher.write_u64(1);
            write_formula_operand(hasher, a);
            write_formula_operand(hasher, b);
        }
        FormulaOp::Sub(a, b) => {
            hasher.write_u64(2);
            write_formula_operand(hasher, a);
            write_formula_operand(hasher, b);
        }
        FormulaOp::Mul(a, b) => {
            hasher.write_u64(3);
            write_formula_operand(hasher, a);
            write_formula_operand(hasher, b);
        }
        FormulaOp::Div(a, b) => {
            hasher.write_u64(4);
            write_formula_operand(hasher, a);
            write_formula_operand(hasher, b);
        }
        FormulaOp::MulPermille(a, b) => {
            hasher.write_u64(5);
            write_formula_operand(hasher, a);
            write_formula_operand(hasher, b);
        }
        FormulaOp::Min(a, b) => {
            hasher.write_u64(6);
            write_formula_operand(hasher, a);
            write_formula_operand(hasher, b);
        }
        FormulaOp::Max(a, b) => {
            hasher.write_u64(7);
            write_formula_operand(hasher, a);
            write_formula_operand(hasher, b);
        }
        FormulaOp::Select {
            cond,
            if_true,
            if_false,
        } => {
            hasher.write_u64(8);
            write_formula_cond(hasher, cond);
            write_formula_operand(hasher, if_true);
            write_formula_operand(hasher, if_false);
        }
        FormulaOp::Dice { count, sides } => {
            hasher.write_u64(9);
            hasher.write_u64(u64::from(*count));
            hasher.write_u64(u64::from(*sides));
        }
    }
}

/// 混入 [`crate::weapon_category::WeaponCategoryDef`] 的全部字段
/// （伤害类别/抗性接线批次新增）——`default_formula` 解析成
/// `NamespacedId` 字符串（见模块文档「`ContentIndex` 字段」一节）。
fn write_weapon_category_fields(
    hasher: &mut StateHasher,
    table: &WeaponCategoryTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let def = table
        .get(index)
        .expect("调用方已确认 classify_index 判定为 WeaponCategory，get 必返回 Some");
    write_optional_resolved(hasher, def.default_formula, registry);
}

/// 混入 [`crate::damage_category::DamageCategoryDef`] 的全部字段
/// （伤害类别/抗性接线批次新增），理由同 [`write_weapon_category_fields`]。
///
/// `display_name_key` 是**版本 24 新增的哈希输入**（此前本表只有
/// `default_formula` 一个字段）——写法与
/// [`write_recipe_category_fields`] 逐字相同：字面 `NamespacedId`，
/// 不经 `registry` 解析，直接混入。它进摘要的理由同本文件顶部
/// 「`display_name_key` 也混入」那条既有判断：本地化键换了值，玩家看到
/// 的名字就换了，那是内容真的变了。
fn write_damage_category_fields(
    hasher: &mut StateHasher,
    table: &DamageCategoryTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let def = table
        .get(index)
        .expect("调用方已确认 classify_index 判定为 DamageCategory，get 必返回 Some");
    hasher.write_namespaced_id(&def.display_name_key);
    write_optional_resolved(hasher, def.default_formula, registry);
}

/// 混入 [`crate::recipe::RecipeDef`] 的全部字段（制作系统批次新增）
/// ——`id` 除外，理由同 [`write_xp_curve_fields`] 跳过同名字段：`id`
/// 已经在 [`entry_value_digest`] 顶部混过一次。
///
/// 两处需要留意的编码细节，都是 ADR 0027「覆盖字段值，不只 id 集合」
/// 在变长/可选字段上的具体落法：
///
/// 1. `ingredients` 是变长 `Vec`——**先写条数再逐条写**，与
///    [`write_class_fields`] 处理 `traits` 逐字节同构。不写长度前缀的话
///    `[(A,1),(B,2)]` 与 `[(A,1),(B,2),(C,3)]` 有撞哈希的可能。
/// 2. `required_station`/`required_tool` 是 `Option<ContentIndex>`——
///    走 [`write_optional_resolved`]，`None` 与 `Some` 因此写入不同的
///    判别字节（`0` vs `1`），不会被混为一谈。
fn write_recipe_fields(
    hasher: &mut StateHasher,
    table: &RecipeTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let view = table
        .get(index)
        .expect("调用方已确认 classify_index 判定为 Recipe，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    write_optional_resolved(hasher, Some(view.category), registry);
    hasher.write_u64(view.ingredients.len() as u64);
    for ingredient in view.ingredients {
        write_optional_resolved(hasher, Some(ingredient.item), registry);
        hasher.write_u64(u64::from(ingredient.count));
    }
    write_optional_resolved(hasher, Some(view.product), registry);
    hasher.write_u64(u64::from(view.product_count));
    write_optional_resolved(hasher, view.required_station, registry);
    write_optional_resolved(hasher, view.required_tool, registry);
    // 配方发现批次新增的 `RecipeDef.requires_discovery`——一个布尔，
    // 不含 `ContentIndex`，直接混入 0/1。它**真的改变结算**
    // （`ll_sim::resolve::resolve_craft` 因此多判一道「会不会做」的
    // 闸门），按 ADR 0027「内容哈希覆盖字段值」必须进摘要：否则把一条
    // 人人会做的配方悄悄改成需要发现，玩家的存档不会察觉内容变了。
    hasher.write_u64(u64::from(view.requires_discovery));
}

/// 混入 [`crate::recipe_category::RecipeCategoryDef`] 的全部字段
/// （制作系统批次新增）——`required_subclasses` 是变长 `Vec`，走
/// [`write_resolved_content_index_slice`]（先长度、再逐条解析成
/// `NamespacedId` 字符串），理由同 [`write_recipe_fields`] 的
/// `ingredients` 一条。
fn write_recipe_category_fields(
    hasher: &mut StateHasher,
    table: &RecipeCategoryTable,
    index: ContentIndex,
    registry: &Registry,
) {
    let def = table
        .get(index)
        .expect("调用方已确认 classify_index 判定为 RecipeCategory，get 必返回 Some");
    hasher.write_namespaced_id(&def.display_name_key);
    write_resolved_content_index_slice(hasher, &def.required_subclasses, registry);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::RaceAttrs;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    /// 一份内部自洽、全部字段为固定占位值的种族属性——测试只关心
    /// `darkvision_cells` 这一个字段是否驱动哈希变化时，用它避免每个
    /// 测试都重复拼一遍其余字段。
    fn race_attrs(display_name: &str, darkvision_cells: u32) -> RaceAttrs {
        RaceAttrs {
            display_name_key: id(display_name),
            stat_modifiers: BaseStats {
                strength: 0,
                dexterity: 0,
                constitution: 0,
                intelligence: 0,
                willpower: 0,
                charisma: 0,
                luck: 0,
            },
            darkvision_cells,
            footprint: (1, 1),
            lifespan_years: 80,
            xp_reward: 0,
            traits: Vec::new(),
            starting_items: Vec::new(),
        }
    }

    /// 空的十四张非种族表——测试只关心其中一张表时，用它填满
    /// [`ContentValueTables`] 剩余字段，避免每个测试各自重复拼一遍。
    #[allow(clippy::type_complexity)]
    fn empty_non_race_tables() -> (
        TerrainTable,
        ClassTable,
        SkillTable,
        SubclassTable,
        QuestTable,
        SpaceProfileTable,
        ClipTable,
        TraitTable,
        ResourcePoolTable,
        ItemTable,
        XpCurveTable,
        FormulaTable,
        WeaponCategoryTable,
        DamageCategoryTable,
        WeatherTable,
        RecipeTable,
        RecipeCategoryTable,
    ) {
        (
            TerrainTable::new(),
            ClassTable::new(),
            SkillTable::new(),
            SubclassTable::new(),
            QuestTable::new(),
            SpaceProfileTable::new(),
            ClipTable::new(),
            TraitTable::new(),
            ResourcePoolTable::new(),
            ItemTable::new(),
            XpCurveTable::new(),
            FormulaTable::new(),
            WeaponCategoryTable::new(),
            DamageCategoryTable::new(),
            WeatherTable::new(),
            RecipeTable::new(),
            RecipeCategoryTable::new(),
        )
    }

    #[test]
    fn 只改字段值不改id集合时命名空间哈希改变() {
        // 本次升级的核心验收：旧版"只追踪 id 集合"的哈希对这个场景
        // 完全无感（id 一个字符没变），值哈希必须能看见这条差异。
        // Arrange：两个 registry 各自注册同一个种族 id，唯一的差异是
        // darkvision_cells 的取值。
        let mut registry_a = Registry::new();
        let index_a = registry_a.intern(id("yourmod:dwarf"));
        let mut race_a = RaceTable::new();
        race_a
            .define(index_a, race_attrs("yourmod:dwarf_name", 5))
            .expect("测试用声明内部自洽");
        let (
            terrain_a,
            class_a,
            skill_a,
            subclass_a,
            quest_a,
            space_a,
            clip_a,
            trait_a,
            pool_a,
            item_a,
            xp_a,
            formula_a,
            weapon_category_a,
            damage_category_a,
            weather_a,
            recipe_a,
            recipe_category_a,
        ) = empty_non_race_tables();

        let mut registry_b = Registry::new();
        let index_b = registry_b.intern(id("yourmod:dwarf"));
        let mut race_b = RaceTable::new();
        race_b
            .define(index_b, race_attrs("yourmod:dwarf_name", 9))
            .expect("测试用声明内部自洽");
        let (
            terrain_b,
            class_b,
            skill_b,
            subclass_b,
            quest_b,
            space_b,
            clip_b,
            trait_b,
            pool_b,
            item_b,
            xp_b,
            formula_b,
            weapon_category_b,
            damage_category_b,
            weather_b,
            recipe_b,
            recipe_category_b,
        ) = empty_non_race_tables();

        // Act
        apply_value_hashes(
            &mut registry_a,
            &ContentValueTables {
                terrain: &terrain_a,
                class: &class_a,
                skill: &skill_a,
                subclass: &subclass_a,
                quest: &quest_a,
                race: &race_a,
                space_profile: &space_a,
                clip: &clip_a,
                trait_def: &trait_a,
                resource_pool: &pool_a,
                item: &item_a,
                xp_curve: &xp_a,
                formula: &formula_a,
                weapon_category: &weapon_category_a,
                damage_category: &damage_category_a,
                weather: &weather_a,
                recipe: &recipe_a,
                recipe_category: &recipe_category_a,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );
        apply_value_hashes(
            &mut registry_b,
            &ContentValueTables {
                terrain: &terrain_b,
                class: &class_b,
                skill: &skill_b,
                subclass: &subclass_b,
                quest: &quest_b,
                race: &race_b,
                space_profile: &space_b,
                clip: &clip_b,
                trait_def: &trait_b,
                resource_pool: &pool_b,
                item: &item_b,
                xp_curve: &xp_b,
                formula: &formula_b,
                weapon_category: &weapon_category_b,
                damage_category: &damage_category_b,
                weather: &weather_b,
                recipe: &recipe_b,
                recipe_category: &recipe_category_b,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );

        // Assert
        assert_ne!(
            registry_a.content_hash_of("yourmod"),
            registry_b.content_hash_of("yourmod")
        );
    }

    #[test]
    fn 此前遗漏的种族击杀经验值字段变化时命名空间哈希也改变() {
        // 直接验收本次批次修的一个真实漏洞：write_race_fields 升级前
        // 从未混入 xp_reward——两份种族声明除 xp_reward 外逐字段相同，
        // 值哈希必须能看见这条差异，否则等于白补。
        // Arrange
        let mut registry_a = Registry::new();
        let index_a = registry_a.intern(id("yourmod:goblin"));
        let mut race_a = RaceTable::new();
        race_a
            .define(
                index_a,
                RaceAttrs {
                    xp_reward: 10,
                    ..race_attrs("yourmod:goblin_name", 0)
                },
            )
            .expect("测试用声明内部自洽");
        let (
            terrain_a,
            class_a,
            skill_a,
            subclass_a,
            quest_a,
            space_a,
            clip_a,
            trait_a,
            pool_a,
            item_a,
            xp_a,
            formula_a,
            weapon_category_a,
            damage_category_a,
            weather_a,
            recipe_a,
            recipe_category_a,
        ) = empty_non_race_tables();

        let mut registry_b = Registry::new();
        let index_b = registry_b.intern(id("yourmod:goblin"));
        let mut race_b = RaceTable::new();
        race_b
            .define(
                index_b,
                RaceAttrs {
                    xp_reward: 99,
                    ..race_attrs("yourmod:goblin_name", 0)
                },
            )
            .expect("测试用声明内部自洽");
        let (
            terrain_b,
            class_b,
            skill_b,
            subclass_b,
            quest_b,
            space_b,
            clip_b,
            trait_b,
            pool_b,
            item_b,
            xp_b,
            formula_b,
            weapon_category_b,
            damage_category_b,
            weather_b,
            recipe_b,
            recipe_category_b,
        ) = empty_non_race_tables();

        // Act
        apply_value_hashes(
            &mut registry_a,
            &ContentValueTables {
                terrain: &terrain_a,
                class: &class_a,
                skill: &skill_a,
                subclass: &subclass_a,
                quest: &quest_a,
                race: &race_a,
                space_profile: &space_a,
                clip: &clip_a,
                trait_def: &trait_a,
                resource_pool: &pool_a,
                item: &item_a,
                xp_curve: &xp_a,
                formula: &formula_a,
                weapon_category: &weapon_category_a,
                damage_category: &damage_category_a,
                weather: &weather_a,
                recipe: &recipe_a,
                recipe_category: &recipe_category_a,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );
        apply_value_hashes(
            &mut registry_b,
            &ContentValueTables {
                terrain: &terrain_b,
                class: &class_b,
                skill: &skill_b,
                subclass: &subclass_b,
                quest: &quest_b,
                race: &race_b,
                space_profile: &space_b,
                clip: &clip_b,
                trait_def: &trait_b,
                resource_pool: &pool_b,
                item: &item_b,
                xp_curve: &xp_b,
                formula: &formula_b,
                weapon_category: &weapon_category_b,
                damage_category: &damage_category_b,
                weather: &weather_b,
                recipe: &recipe_b,
                recipe_category: &recipe_category_b,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );

        // Assert
        assert_ne!(
            registry_a.content_hash_of("yourmod"),
            registry_b.content_hash_of("yourmod")
        );
    }

    #[test]
    fn 种族出生物品字段变化时命名空间哈希也改变() {
        // NPC 生命周期批次新增 RaceDef::starting_items——与上面
        // xp_reward 那条测试同一条判据：写 write_race_fields 时若忘了
        // 混入这个新字段,两份"除出生物品外逐字段相同"的种族声明会算出
        // 相同的值哈希,静默漏判两份不同的内容。
        // Arrange
        let mut registry_a = Registry::new();
        let index_a = registry_a.intern(id("yourmod:goblin"));
        let item_a_id = registry_a.intern(id("yourmod:crude_dagger"));
        let mut race_a = RaceTable::new();
        race_a
            .define(index_a, race_attrs("yourmod:goblin_name", 0))
            .expect("测试用声明内部自洽");
        race_a
            .add_starting_item(index_a, item_a_id, 1)
            .expect("追加出生物品应当成功");
        let (
            terrain_a,
            class_a,
            skill_a,
            subclass_a,
            quest_a,
            space_a,
            clip_a,
            trait_a,
            pool_a,
            item_a,
            xp_a,
            formula_a,
            weapon_category_a,
            damage_category_a,
            weather_a,
            recipe_a,
            recipe_category_a,
        ) = empty_non_race_tables();

        let mut registry_b = Registry::new();
        let index_b = registry_b.intern(id("yourmod:goblin"));
        let mut race_b = RaceTable::new();
        race_b
            .define(index_b, race_attrs("yourmod:goblin_name", 0))
            .expect("测试用声明内部自洽");
        // 不追加任何出生物品——两份种族声明除 starting_items 外逐字段
        // 相同。
        let (
            terrain_b,
            class_b,
            skill_b,
            subclass_b,
            quest_b,
            space_b,
            clip_b,
            trait_b,
            pool_b,
            item_b,
            xp_b,
            formula_b,
            weapon_category_b,
            damage_category_b,
            weather_b,
            recipe_b,
            recipe_category_b,
        ) = empty_non_race_tables();

        // Act
        apply_value_hashes(
            &mut registry_a,
            &ContentValueTables {
                terrain: &terrain_a,
                class: &class_a,
                skill: &skill_a,
                subclass: &subclass_a,
                quest: &quest_a,
                race: &race_a,
                space_profile: &space_a,
                clip: &clip_a,
                trait_def: &trait_a,
                resource_pool: &pool_a,
                item: &item_a,
                xp_curve: &xp_a,
                formula: &formula_a,
                weapon_category: &weapon_category_a,
                damage_category: &damage_category_a,
                weather: &weather_a,
                recipe: &recipe_a,
                recipe_category: &recipe_category_a,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );
        apply_value_hashes(
            &mut registry_b,
            &ContentValueTables {
                terrain: &terrain_b,
                class: &class_b,
                skill: &skill_b,
                subclass: &subclass_b,
                quest: &quest_b,
                race: &race_b,
                space_profile: &space_b,
                clip: &clip_b,
                trait_def: &trait_b,
                resource_pool: &pool_b,
                item: &item_b,
                xp_curve: &xp_b,
                formula: &formula_b,
                weapon_category: &weapon_category_b,
                damage_category: &damage_category_b,
                weather: &weather_b,
                recipe: &recipe_b,
                recipe_category: &recipe_category_b,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );

        // Assert
        assert_ne!(
            registry_a.content_hash_of("yourmod"),
            registry_b.content_hash_of("yourmod")
        );
    }

    #[test]
    fn 新纳入覆盖的物品表字段值改变时命名空间哈希也改变() {
        // 直接验收本次批次新增的表覆盖之一（物品表）：两份物品声明除
        // stack_limit 外逐字段相同，值哈希必须能看见这条差异。
        // Arrange
        use crate::item::ItemAttrs;
        use ll_core::scaled::Milli;
        use ll_sim::combat::Penetration;
        use ll_sim::item::SlotMask;

        fn item_attrs(stack_limit: u32) -> ItemAttrs {
            ItemAttrs {
                display_name_key: id("yourmod:item.pebble"),
                stack_limit,
                base_weight: Milli::ZERO,
                base_price: Milli::ZERO,
                max_durability: None,
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
                tags: Vec::new(),
                taught_recipes: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                blind_box_pool: Vec::new(),
                furniture: false,
            }
        }

        let mut registry_a = Registry::new();
        let index_a = registry_a.intern(id("yourmod:pebble"));
        let mut item_a = ItemTable::new();
        item_a
            .define(index_a, item_attrs(50))
            .expect("测试用声明内部自洽");
        let (
            terrain_a,
            class_a,
            skill_a,
            subclass_a,
            quest_a,
            space_a,
            clip_a,
            trait_a,
            pool_a,
            race_a,
            xp_a,
            formula_a,
            weapon_category_a,
            damage_category_a,
            weather_a,
            recipe_a,
            recipe_category_a,
        ) = (
            TerrainTable::new(),
            ClassTable::new(),
            SkillTable::new(),
            SubclassTable::new(),
            QuestTable::new(),
            SpaceProfileTable::new(),
            ClipTable::new(),
            TraitTable::new(),
            ResourcePoolTable::new(),
            RaceTable::new(),
            XpCurveTable::new(),
            FormulaTable::new(),
            WeaponCategoryTable::new(),
            DamageCategoryTable::new(),
            WeatherTable::new(),
            RecipeTable::new(),
            RecipeCategoryTable::new(),
        );

        let mut registry_b = Registry::new();
        let index_b = registry_b.intern(id("yourmod:pebble"));
        let mut item_b = ItemTable::new();
        item_b
            .define(index_b, item_attrs(99))
            .expect("测试用声明内部自洽");
        let (
            terrain_b,
            class_b,
            skill_b,
            subclass_b,
            quest_b,
            space_b,
            clip_b,
            trait_b,
            pool_b,
            race_b,
            xp_b,
            formula_b,
            weapon_category_b,
            damage_category_b,
            weather_b,
            recipe_b,
            recipe_category_b,
        ) = (
            TerrainTable::new(),
            ClassTable::new(),
            SkillTable::new(),
            SubclassTable::new(),
            QuestTable::new(),
            SpaceProfileTable::new(),
            ClipTable::new(),
            TraitTable::new(),
            ResourcePoolTable::new(),
            RaceTable::new(),
            XpCurveTable::new(),
            FormulaTable::new(),
            WeaponCategoryTable::new(),
            DamageCategoryTable::new(),
            WeatherTable::new(),
            RecipeTable::new(),
            RecipeCategoryTable::new(),
        );

        // Act
        apply_value_hashes(
            &mut registry_a,
            &ContentValueTables {
                terrain: &terrain_a,
                class: &class_a,
                skill: &skill_a,
                subclass: &subclass_a,
                quest: &quest_a,
                race: &race_a,
                space_profile: &space_a,
                clip: &clip_a,
                trait_def: &trait_a,
                resource_pool: &pool_a,
                item: &item_a,
                xp_curve: &xp_a,
                formula: &formula_a,
                weapon_category: &weapon_category_a,
                damage_category: &damage_category_a,
                weather: &weather_a,
                recipe: &recipe_a,
                recipe_category: &recipe_category_a,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );
        apply_value_hashes(
            &mut registry_b,
            &ContentValueTables {
                terrain: &terrain_b,
                class: &class_b,
                skill: &skill_b,
                subclass: &subclass_b,
                quest: &quest_b,
                race: &race_b,
                space_profile: &space_b,
                clip: &clip_b,
                trait_def: &trait_b,
                resource_pool: &pool_b,
                item: &item_b,
                xp_curve: &xp_b,
                formula: &formula_b,
                weapon_category: &weapon_category_b,
                damage_category: &damage_category_b,
                weather: &weather_b,
                recipe: &recipe_b,
                recipe_category: &recipe_category_b,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );

        // Assert
        assert_ne!(
            registry_a.content_hash_of("yourmod"),
            registry_b.content_hash_of("yourmod")
        );
    }

    #[test]
    fn 新纳入覆盖的物品表在装载顺序不同时产出相同的命名空间哈希() {
        // 约束 C5 的直接验收，覆盖本次批次新增的表——与既有的种族版本
        // 「相同内容不同装载顺序产出相同的命名空间哈希」同一条判据：
        // 两个 registry 以相反顺序 intern + define 完全相同的两件物品，
        // 值哈希必须不受这条纯粹因加载顺序而产生的差异影响。
        // Arrange
        use crate::item::ItemAttrs;
        use ll_core::scaled::Milli;
        use ll_sim::combat::Penetration;
        use ll_sim::item::SlotMask;

        fn item_attrs(name: &str) -> ItemAttrs {
            ItemAttrs {
                display_name_key: id(name),
                stack_limit: 1,
                base_weight: Milli::from_whole(1),
                base_price: Milli::from_whole(2),
                max_durability: Some(10),
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
                tags: Vec::new(),
                taught_recipes: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                blind_box_pool: Vec::new(),
                furniture: false,
            }
        }

        let mut registry_forward = Registry::new();
        let arrow_forward = registry_forward.intern(id("yourmod:arrow"));
        let sword_forward = registry_forward.intern(id("yourmod:sword"));
        let mut item_forward = ItemTable::new();
        item_forward
            .define(arrow_forward, item_attrs("yourmod:item.arrow"))
            .expect("测试用声明内部自洽");
        item_forward
            .define(sword_forward, item_attrs("yourmod:item.sword"))
            .expect("测试用声明内部自洽");

        let mut registry_reversed = Registry::new();
        let sword_reversed = registry_reversed.intern(id("yourmod:sword"));
        let arrow_reversed = registry_reversed.intern(id("yourmod:arrow"));
        let mut item_reversed = ItemTable::new();
        item_reversed
            .define(sword_reversed, item_attrs("yourmod:item.sword"))
            .expect("测试用声明内部自洽");
        item_reversed
            .define(arrow_reversed, item_attrs("yourmod:item.arrow"))
            .expect("测试用声明内部自洽");

        // Act：两边分配到的 ContentIndex 确实互相对调，证明这不是一次
        // 巧合的"顺序没变"。
        assert_ne!(arrow_forward, arrow_reversed);
        let empty_forward = empty_non_race_tables();
        let empty_reversed = empty_non_race_tables();
        apply_value_hashes(
            &mut registry_forward,
            &ContentValueTables {
                terrain: &empty_forward.0,
                class: &empty_forward.1,
                skill: &empty_forward.2,
                subclass: &empty_forward.3,
                quest: &empty_forward.4,
                race: &RaceTable::new(),
                space_profile: &empty_forward.5,
                clip: &empty_forward.6,
                trait_def: &empty_forward.7,
                resource_pool: &empty_forward.8,
                item: &item_forward,
                xp_curve: &empty_forward.10,
                formula: &empty_forward.11,
                weapon_category: &empty_forward.12,
                damage_category: &empty_forward.13,
                weather: &empty_forward.14,
                recipe: &empty_forward.15,
                recipe_category: &empty_forward.16,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );
        apply_value_hashes(
            &mut registry_reversed,
            &ContentValueTables {
                terrain: &empty_reversed.0,
                class: &empty_reversed.1,
                skill: &empty_reversed.2,
                subclass: &empty_reversed.3,
                quest: &empty_reversed.4,
                race: &RaceTable::new(),
                space_profile: &empty_reversed.5,
                clip: &empty_reversed.6,
                trait_def: &empty_reversed.7,
                resource_pool: &empty_reversed.8,
                item: &item_reversed,
                xp_curve: &empty_reversed.10,
                formula: &empty_reversed.11,
                weapon_category: &empty_reversed.12,
                damage_category: &empty_reversed.13,
                weather: &empty_reversed.14,
                recipe: &empty_reversed.15,
                recipe_category: &empty_reversed.16,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );

        // Assert
        assert_eq!(
            registry_forward.content_hash_of("yourmod"),
            registry_reversed.content_hash_of("yourmod")
        );
    }

    #[test]
    fn 相同内容不同装载顺序产出相同的命名空间哈希() {
        // 约束 C5 的直接验收：两个 registry 以相反顺序 intern + define
        // 完全相同的两个种族——不同顺序意味着两边分配到的 ContentIndex
        // 互相对调（先注册的拿到更小的索引），值哈希必须不受这条纯粹
        // 因加载顺序而产生的差异影响。
        // Arrange
        let mut registry_forward = Registry::new();
        let orc_forward = registry_forward.intern(id("yourmod:orc"));
        let troll_forward = registry_forward.intern(id("yourmod:troll"));
        let mut race_forward = RaceTable::new();
        race_forward
            .define(orc_forward, race_attrs("yourmod:orc_name", 3))
            .expect("测试用声明内部自洽");
        race_forward
            .define(troll_forward, race_attrs("yourmod:troll_name", 7))
            .expect("测试用声明内部自洽");
        let (
            terrain_f,
            class_f,
            skill_f,
            subclass_f,
            quest_f,
            space_f,
            clip_f,
            trait_f,
            pool_f,
            item_f,
            xp_f,
            formula_f,
            weapon_category_f,
            damage_category_f,
            weather_f,
            recipe_f,
            recipe_category_f,
        ) = empty_non_race_tables();

        let mut registry_reversed = Registry::new();
        let troll_reversed = registry_reversed.intern(id("yourmod:troll"));
        let orc_reversed = registry_reversed.intern(id("yourmod:orc"));
        let mut race_reversed = RaceTable::new();
        race_reversed
            .define(troll_reversed, race_attrs("yourmod:troll_name", 7))
            .expect("测试用声明内部自洽");
        race_reversed
            .define(orc_reversed, race_attrs("yourmod:orc_name", 3))
            .expect("测试用声明内部自洽");
        let (
            terrain_r,
            class_r,
            skill_r,
            subclass_r,
            quest_r,
            space_r,
            clip_r,
            trait_r,
            pool_r,
            item_r,
            xp_r,
            formula_r,
            weapon_category_r,
            damage_category_r,
            weather_r,
            recipe_r,
            recipe_category_r,
        ) = empty_non_race_tables();

        // Act：两边分配到的 ContentIndex 确实互相对调，证明这不是一次
        // 巧合的"顺序没变"。
        assert_ne!(orc_forward, orc_reversed);
        apply_value_hashes(
            &mut registry_forward,
            &ContentValueTables {
                terrain: &terrain_f,
                class: &class_f,
                skill: &skill_f,
                subclass: &subclass_f,
                quest: &quest_f,
                race: &race_forward,
                space_profile: &space_f,
                clip: &clip_f,
                trait_def: &trait_f,
                resource_pool: &pool_f,
                item: &item_f,
                xp_curve: &xp_f,
                formula: &formula_f,
                weapon_category: &weapon_category_f,
                damage_category: &damage_category_f,
                weather: &weather_f,
                recipe: &recipe_f,
                recipe_category: &recipe_category_f,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );
        apply_value_hashes(
            &mut registry_reversed,
            &ContentValueTables {
                terrain: &terrain_r,
                class: &class_r,
                skill: &skill_r,
                subclass: &subclass_r,
                quest: &quest_r,
                race: &race_reversed,
                space_profile: &space_r,
                clip: &clip_r,
                trait_def: &trait_r,
                resource_pool: &pool_r,
                item: &item_r,
                xp_curve: &xp_r,
                formula: &formula_r,
                weapon_category: &weapon_category_r,
                damage_category: &damage_category_r,
                weather: &weather_r,
                recipe: &recipe_r,
                recipe_category: &recipe_category_r,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );

        // Assert
        assert_eq!(
            registry_forward.content_hash_of("yourmod"),
            registry_reversed.content_hash_of("yourmod")
        );
    }

    #[test]
    fn 只增加一个新id不改变已有id字段值时命名空间哈希仍然改变() {
        // 老行为不能丢：值哈希是在 id 摘要之上叠加，不是替换掉它——
        // 单纯扩大 id 集合（不触碰任何已有内容的字段值）依然必须让
        // 哈希改变，理由同 registry.rs 的
        // `content_hash随注册内容变化而变化`，这里额外验证"跑完值哈希
        // 那一步之后"这条老行为依然成立。
        //
        // 用两个独立的 registry（各自只调用一次 apply_value_hashes）
        // 对比，而不是在同一个 registry 上调用两次
        // apply_value_hashes——[`apply_value_hashes`] 的契约是"全部
        // 内容装载完毕后调用恰好一次"（见其文档），在同一个 registry
        // 上重复调用会把已经折叠过的字段值摘要再折一次、异或抵消掉，
        // 不是这里要验证的场景。
        // Arrange
        let mut registry_before = Registry::new();
        let dwarf_before = registry_before.intern(id("yourmod:dwarf"));
        let mut race_before = RaceTable::new();
        race_before
            .define(dwarf_before, race_attrs("yourmod:dwarf_name", 5))
            .expect("测试用声明内部自洽");
        let (
            terrain_before,
            class_before,
            skill_before,
            subclass_before,
            quest_before,
            space_before,
            clip_before,
            trait_before,
            pool_before,
            item_before,
            xp_before,
            formula_before,
            weapon_category_before,
            damage_category_before,
            weather_before,
            recipe_before,
            recipe_category_before,
        ) = empty_non_race_tables();

        let mut registry_after = Registry::new();
        let dwarf_after = registry_after.intern(id("yourmod:dwarf"));
        let mut race_after = RaceTable::new();
        race_after
            .define(dwarf_after, race_attrs("yourmod:dwarf_name", 5))
            .expect("测试用声明内部自洽");
        // 新增一个种族 id，不改动 dwarf 已有的字段值。
        let elf_after = registry_after.intern(id("yourmod:elf"));
        race_after
            .define(elf_after, race_attrs("yourmod:elf_name", 0))
            .expect("测试用声明内部自洽");
        let (
            terrain_after,
            class_after,
            skill_after,
            subclass_after,
            quest_after,
            space_after,
            clip_after,
            trait_after,
            pool_after,
            item_after,
            xp_after,
            formula_after,
            weapon_category_after,
            damage_category_after,
            weather_after,
            recipe_after,
            recipe_category_after,
        ) = empty_non_race_tables();

        // Act
        apply_value_hashes(
            &mut registry_before,
            &ContentValueTables {
                terrain: &terrain_before,
                class: &class_before,
                skill: &skill_before,
                subclass: &subclass_before,
                quest: &quest_before,
                race: &race_before,
                space_profile: &space_before,
                clip: &clip_before,
                trait_def: &trait_before,
                resource_pool: &pool_before,
                item: &item_before,
                xp_curve: &xp_before,
                formula: &formula_before,
                weapon_category: &weapon_category_before,
                damage_category: &damage_category_before,
                weather: &weather_before,
                recipe: &recipe_before,
                recipe_category: &recipe_category_before,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );
        apply_value_hashes(
            &mut registry_after,
            &ContentValueTables {
                terrain: &terrain_after,
                class: &class_after,
                skill: &skill_after,
                subclass: &subclass_after,
                quest: &quest_after,
                race: &race_after,
                space_profile: &space_after,
                clip: &clip_after,
                trait_def: &trait_after,
                resource_pool: &pool_after,
                item: &item_after,
                xp_curve: &xp_after,
                formula: &formula_after,
                weapon_category: &weapon_category_after,
                damage_category: &damage_category_after,
                weather: &weather_after,
                recipe: &recipe_after,
                recipe_category: &recipe_category_after,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );

        // Assert
        assert_ne!(
            registry_before.content_hash_of("yourmod"),
            registry_after.content_hash_of("yourmod")
        );
    }

    #[test]
    fn 不落在任何内容表里的纯id引用仍然参与哈希() {
        // base_placeholder 一类纯 id 引用（没有对应的 *Table::define
        // 调用）不应该被值哈希悄悄忽略——它的"是否存在"仍然是内容集合
        // 的一部分。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:placeholder_race"));
        let (
            terrain,
            class,
            skill,
            subclass,
            quest,
            space,
            clip,
            trait_def,
            pool,
            item,
            xp,
            formula,
            weapon_category,
            damage_category,
            weather,
            recipe,
            recipe_category,
        ) = empty_non_race_tables();
        let race = RaceTable::new();

        // Act
        apply_value_hashes(
            &mut registry,
            &ContentValueTables {
                terrain: &terrain,
                class: &class,
                skill: &skill,
                subclass: &subclass,
                quest: &quest,
                race: &race,
                space_profile: &space,
                clip: &clip,
                trait_def: &trait_def,
                resource_pool: &pool,
                item: &item,
                xp_curve: &xp,
                formula: &formula,
                weapon_category: &weapon_category,
                damage_category: &damage_category,
                weather: &weather,
                recipe: &recipe,
                recipe_category: &recipe_category,
                tag: &TagTable::new(),
                modifier_type: &ModifierTypeTable::new(),
                resource: &ResourceTable::new(),
                culture: &CultureTable::new(),
                dialogue: &DialogueTable::new(),
                dialogue_node: &DialogueNodeTable::new(),
            },
        );

        // Assert
        assert!(registry.content_hash_of("lostland").is_some());
    }

    /// 盗贼被动两分批次：两个新变体各自混入**互不相同**的判别值，
    /// 也与既有五个变体互不相同——`CONTENT_HASH_ALGORITHM_VERSION`
    /// 文档「版本 14」一节点名的那条守门断言。
    ///
    /// 若有人把 `InspectionSuspicion`/`InspectionConcealment` 的
    /// `hasher.write_u64(5)`/`(6)` 写成同一个数（或复用既有的
    /// `0..=4`），本测试立刻变红——那正是「两条不同的内容声明折出同一
    /// 份摘要」这类事故的形式，本模块「表判别字节」机制存在的全部
    /// 理由。
    #[test]
    fn 新增的两个盘查规则修正变体各自混入不同的判别值() {
        // Arrange：同一个数值 500 喂给两个新变体，摘要仍必须不同——
        // 差别只可能来自判别值本身。
        let registry = Registry::new();
        let digest = |modifier: &RuleModifier| -> u64 {
            let mut hasher = StateHasher::new();
            write_rule_modifier(&mut hasher, modifier, &registry);
            hasher.finish()
        };

        // Act
        let suspicion = digest(&RuleModifier::InspectionSuspicion {
            inconspicuous_modifier: 500,
        });
        let concealment = digest(&RuleModifier::InspectionConcealment {
            concealment_modifier: 500,
        });
        let reroll = digest(&RuleModifier::RerollOnce { value: 500 });
        let sneak = digest(&RuleModifier::SneakAttack {
            sneak_modifier: 500,
            extra_damage: 0,
        });

        // Assert
        assert_ne!(suspicion, concealment);
        assert_ne!(suspicion, reroll);
        assert_ne!(concealment, reroll);
        assert_ne!(suspicion, sneak);
        assert_ne!(concealment, sneak);
    }

    /// 制作类副职奖励批次：新变体 `CraftYield` 混入的判别值 `7` 与既有
    /// 七个变体互不相同——`CONTENT_HASH_ALGORITHM_VERSION` 文档「版本 19」
    /// 一节点名的两条守门断言之一。
    ///
    /// 若有人把 `hasher.write_u64(7)` 写成既有的 `0..=6` 之一，本测试
    /// 立刻变红。
    #[test]
    fn 制作产出加成的摘要与其余变体都不同() {
        // Arrange：同一个数值、同一个 `ContentIndex` 喂给带索引的两个
        // 变体，摘要仍必须不同——差别只可能来自判别值本身。
        let registry = Registry::new();
        let digest = |modifier: &RuleModifier| -> u64 {
            let mut hasher = StateHasher::new();
            write_rule_modifier(&mut hasher, modifier, &registry);
            hasher.finish()
        };
        let category = ContentIndex::default();

        // Act
        let craft_yield = digest(&RuleModifier::CraftYield {
            category,
            bonus_product_count: 4,
        });
        let others = [
            digest(&RuleModifier::Resistance {
                damage_category: category,
                damage_reduction: 4,
            }),
            digest(&RuleModifier::RerollOnce { value: 4 }),
            digest(&RuleModifier::SneakAttack {
                sneak_modifier: 4,
                extra_damage: 4,
            }),
            digest(&RuleModifier::InspectionSuspicion {
                inconspicuous_modifier: 4,
            }),
            digest(&RuleModifier::InspectionConcealment {
                concealment_modifier: 4,
            }),
            // 合并批次补进对照组：`Vulnerability` 与本变体在两个分支上
            // 各自取了判别值 `7`，合并时后到的那个挪到 `8`。两者的载荷
            // 又恰好同构（一个 `ContentIndex` + 一个 `i32`），因此**只
            // 有判别值把它们分开**——挪号若哪天被改回去，这一条会红。
            digest(&RuleModifier::Vulnerability {
                damage_category: category,
                damage_increase: 4,
            }),
        ];

        // Assert
        for other in others {
            assert_ne!(craft_yield, other);
        }
    }

    /// 制作类副职奖励批次的第二条守门断言：`bonus_product_count` 真的
    /// 进了摘要，不是只混了判别值与配方类别。**负值与正值也必须可分**
    /// ——本字段刻意允许为负（「手艺生疏」），若哪天有人在这里写
    /// `unsigned_abs()` 之类，本条会变红。
    #[test]
    fn 制作产出加成件数不同则摘要不同() {
        // Arrange
        let registry = Registry::new();
        let digest = |bonus: i32| -> u64 {
            let mut hasher = StateHasher::new();
            write_rule_modifier(
                &mut hasher,
                &RuleModifier::CraftYield {
                    category: ContentIndex::default(),
                    bonus_product_count: bonus,
                },
                &registry,
            );
            hasher.finish()
        };

        // Act & Assert
        assert_ne!(digest(1), digest(2));
        assert_ne!(digest(1), digest(-1));
        assert_ne!(digest(0), digest(1));
    }

    /// 同一个变体、不同的数值，摘要必须不同——证明新变体混入的不只是
    /// 判别值，字段本身也真的进了哈希（版本 9 那条「长度前缀本身也是
    /// 新的哈希输入」同一类断言的变体级形式）。
    #[test]
    fn 两个盘查规则修正变体的数值字段真的进了摘要() {
        // Arrange
        let registry = Registry::new();
        let digest = |modifier: &RuleModifier| -> u64 {
            let mut hasher = StateHasher::new();
            write_rule_modifier(&mut hasher, modifier, &registry);
            hasher.finish()
        };

        // Act & Assert
        assert_ne!(
            digest(&RuleModifier::InspectionSuspicion {
                inconspicuous_modifier: 0,
            }),
            digest(&RuleModifier::InspectionSuspicion {
                inconspicuous_modifier: 1000,
            })
        );
        assert_ne!(
            digest(&RuleModifier::InspectionConcealment {
                concealment_modifier: 0,
            }),
            digest(&RuleModifier::InspectionConcealment {
                concealment_modifier: 1000,
            })
        );
    }

    /// 偷袭迁进判定系统批次：`SneakAttack` 的第一个载荷改了含义
    /// （每点幸运的千分比触发率 → 加给偷袭者的骰子点数），
    /// [`CONTENT_HASH_ALGORITHM_VERSION`] 文档「③ 偷袭迁进判定系统
    /// 批次」一节点名的那条守门断言。
    ///
    /// 本条守两件事：那个载荷**真的进了摘要**（否则改含义时摘要不变，
    /// 两份语义不同的内容会撞成同一份），以及它**没有与其余三条判定
    /// 修正撞车**（同一个点数喂给四个变体，摘要必须两两不同——差别
    /// 只可能来自判别值）。
    #[test]
    fn 偷袭修正与同数值的其余判定修正摘要不同() {
        // Arrange
        let registry = Registry::new();
        let digest = |modifier: &RuleModifier| -> u64 {
            let mut hasher = StateHasher::new();
            write_rule_modifier(&mut hasher, modifier, &registry);
            hasher.finish()
        };

        // Act & Assert：同一个变体、不同的点数 → 摘要不同。
        assert_ne!(
            digest(&RuleModifier::SneakAttack {
                sneak_modifier: 9,
                extra_damage: 15,
            }),
            digest(&RuleModifier::SneakAttack {
                sneak_modifier: 19,
                extra_damage: 15,
            })
        );

        // 同一个点数、不同的变体 → 摘要仍必须两两不同。
        let point = 9;
        let same_point = [
            digest(&RuleModifier::SneakAttack {
                sneak_modifier: point,
                extra_damage: 0,
            }),
            digest(&RuleModifier::InspectionSuspicion {
                inconspicuous_modifier: point,
            }),
            digest(&RuleModifier::InspectionConcealment {
                concealment_modifier: point,
            }),
            digest(&RuleModifier::RerollOnce { value: point }),
        ];
        for (left_at, left) in same_point.iter().enumerate() {
            for right in &same_point[left_at + 1..] {
                assert_ne!(left, right);
            }
        }
    }

    /// 加值类型批次新增的 `TypedRuleModifier::modifier_type` 真的进了
    /// 摘要——见 [`CONTENT_HASH_ALGORITHM_VERSION`] 文档「版本 16」一节
    /// 第 2 条。
    ///
    /// 这条守的是一个具体的失效模式：把一条抗性悄悄从「附魔」改成
    /// 「天生」，结算结果会变（它从此不与别的附魔加值竞争、改为与天生
    /// 加值竞争，跨类型还会相加），而存档若察觉不到内容变了，会报
    /// 「内容没变」。
    #[test]
    fn 声明了加值类型的规则修正与不声明的摘要不同() {
        // Arrange：同一条抗性，三种加值类型声明（不声明 / 甲 / 乙）。
        let mut registry = Registry::new();
        let acid = registry.intern(id("yourmod:acid"));
        let enhancement = registry.intern(id("yourmod:enhancement"));
        let alchemical = registry.intern(id("yourmod:alchemical"));
        let modifier = RuleModifier::Resistance {
            damage_category: acid,
            damage_reduction: 3,
        };
        let digest = |modifier_type: Option<ContentIndex>| -> u64 {
            let mut hasher = StateHasher::new();
            write_typed_rule_modifier(
                &mut hasher,
                &TypedRuleModifier {
                    modifier_type,
                    modifier: modifier.clone(),
                },
                &registry,
            );
            hasher.finish()
        };

        // Act
        let untyped = digest(None);
        let as_enhancement = digest(Some(enhancement));
        let as_alchemical = digest(Some(alchemical));

        // Assert：三者两两不同——「没声明」与「声明了某个类型」要分开
        //（缺席判别字节本身就是一段哈希输入），声明成哪个类型也要分开。
        assert_ne!(untyped, as_enhancement);
        assert_ne!(untyped, as_alchemical);
        assert_ne!(as_enhancement, as_alchemical);
    }

    /// 易伤与减伤在摘要里必须分得开——见
    /// [`CONTENT_HASH_ALGORITHM_VERSION`] 文档「版本 19」一节。
    ///
    /// 这条守的是一个具体的失效模式：`Vulnerability` 写入的两个字段与
    /// `Resistance` 逐字同构（伤害类别 + 一个 `i64`），**判别值是两者
    /// 唯一的区分**。忘了写判别值、或两个变体误用同一个判别值，一件
    /// 「抗火 4」的围裙与一件「怕火 4」的围裙就会摘要相同，存档校验会
    /// 报「内容没变」，玩家读档后拿到一件行为完全相反的装备。
    #[test]
    fn 易伤与同数值的减伤摘要不同() {
        // Arrange：同一个伤害类别、同一个点数、同一个加值类型。
        let mut registry = Registry::new();
        let fire = registry.intern(id("yourmod:fire"));
        let innate = registry.intern(id("yourmod:innate"));
        let digest = |modifier: RuleModifier| -> u64 {
            let mut hasher = StateHasher::new();
            write_typed_rule_modifier(
                &mut hasher,
                &TypedRuleModifier {
                    modifier_type: Some(innate),
                    modifier,
                },
                &registry,
            );
            hasher.finish()
        };

        // Act
        let as_resistance = digest(RuleModifier::Resistance {
            damage_category: fire,
            damage_reduction: 4,
        });
        let as_vulnerability = digest(RuleModifier::Vulnerability {
            damage_category: fire,
            damage_increase: 4,
        });
        // 同一个变体、不同点数：确认这两个字段本身也真的进了摘要,
        // 否则上面那条断言可能只是判别值在起作用。
        let stronger_vulnerability = digest(RuleModifier::Vulnerability {
            damage_category: fire,
            damage_increase: 6,
        });

        // Assert
        assert_ne!(as_resistance, as_vulnerability);
        assert_ne!(as_vulnerability, stronger_vulnerability);
    }

    /// 配方发现批次新增的 `ItemDef.taught_recipes` 真的进了摘要——
    /// 见 [`CONTENT_HASH_ALGORITHM_VERSION`] 文档「版本 15」一节。
    ///
    /// 这条守的是一个具体的失效模式：把一本书悄悄改成什么都不教（或
    /// 改成教另一条配方），存档若察觉不到内容变了，玩家读档后会拿到一
    /// 个「我明明学过」却做不出来的角色，而校验会报「内容没变」。
    #[test]
    fn 教配方的物品与不教配方的物品摘要不同() {
        // Arrange：两件除了 taught_recipes 之外逐字段相同的物品。
        use crate::item::ItemAttrs;
        use ll_core::scaled::Milli;
        use ll_sim::combat::Penetration;
        use ll_sim::item::SlotMask;

        let mut registry = Registry::new();
        let book = registry.intern(id("yourmod:book"));
        let recipe_one = registry.intern(id("yourmod:recipe_one"));
        let recipe_two = registry.intern(id("yourmod:recipe_two"));

        let attrs = |taught: Vec<ContentIndex>| ItemAttrs {
            display_name_key: id("yourmod:item.book"),
            stack_limit: 1,
            base_weight: Milli::from_whole(1),
            base_price: Milli::from_whole(2),
            max_durability: None,
            equip_mask: SlotMask::EMPTY,
            stat_bonuses: Vec::new(),
            use_effect: None,
            penetration: Penetration::NONE,
            damage_formula: None,
            damage_category: None,
            rule_modifiers: Vec::new(),
            tags: Vec::new(),
            taught_recipes: taught,
            requires_identification: false,
            study_experience: 0,
            blind_box_pool: Vec::new(),
            furniture: false,
        };

        let digest = |taught: Vec<ContentIndex>| -> u64 {
            let mut table = ItemTable::new();
            table
                .define(book, attrs(taught))
                .expect("测试用声明内部自洽");
            let mut hasher = StateHasher::new();
            write_item_fields(&mut hasher, &table, book, &registry);
            hasher.finish()
        };

        // Act
        let teaches_nothing = digest(Vec::new());
        let teaches_one = digest(vec![recipe_one]);
        let teaches_two = digest(vec![recipe_two]);
        let teaches_both = digest(vec![recipe_one, recipe_two]);

        // Assert：四种声明两两不同——空列表也要与「教一条」区分开
        // （长度前缀 0 本身就是一段哈希输入），教哪一条也要区分开
        // （逐条解析回 NamespacedId 字符串混入）。
        assert_ne!(teaches_nothing, teaches_one);
        assert_ne!(teaches_one, teaches_two);
        assert_ne!(teaches_one, teaches_both);
        assert_ne!(teaches_nothing, teaches_both);
    }

    /// 未鉴定物品批次与盲盒批次新增的三列 `ItemDef` 字段真的各自进了
    /// 摘要——见 [`CONTENT_HASH_ALGORITHM_VERSION`] 文档「版本 16」一节。
    ///
    /// 三条各守一个具体的失效模式：
    ///
    /// - `requires_identification`：把一件东西悄悄改成「不用鉴定」，
    ///   存档若察觉不到，玩家读档后会发现自己「早就认得」一件本该先
    ///   鉴定的东西。
    /// - `study_experience`：把经验值悄悄改掉，是一次静默的数值平衡
    ///   改动，正是 ADR 0022/0027 要求值哈希覆盖的那一类。
    /// - `blind_box_pool`：把盒子的产出档位或权重悄悄改掉——这是最难
    ///   察觉的一类（玩家只会觉得「今天手气不好」）。
    #[test]
    fn 鉴定相关三列字段各自都进了物品摘要() {
        // Arrange：一件基准物品，逐列改一处再比。
        use crate::item::ItemAttrs;
        use ll_core::scaled::Milli;
        use ll_sim::combat::Penetration;
        use ll_sim::item::{BlindBoxEntry, SlotMask};

        let mut registry = Registry::new();
        let thing = registry.intern(id("yourmod:thing"));
        let prize_one = registry.intern(id("yourmod:prize_one"));
        let prize_two = registry.intern(id("yourmod:prize_two"));

        let digest = |requires: bool, xp: i64, pool: Vec<BlindBoxEntry>| -> u64 {
            let mut table = ItemTable::new();
            table
                .define(
                    thing,
                    ItemAttrs {
                        display_name_key: id("yourmod:item.thing"),
                        stack_limit: 1,
                        base_weight: Milli::from_whole(1),
                        base_price: Milli::from_whole(2),
                        max_durability: None,
                        equip_mask: SlotMask::EMPTY,
                        stat_bonuses: Vec::new(),
                        use_effect: None,
                        penetration: Penetration::NONE,
                        damage_formula: None,
                        damage_category: None,
                        rule_modifiers: Vec::new(),
                        tags: Vec::new(),
                        taught_recipes: Vec::new(),
                        requires_identification: requires,
                        study_experience: xp,
                        blind_box_pool: pool,
                        furniture: false,
                    },
                )
                .expect("测试用声明内部自洽");
            let mut hasher = StateHasher::new();
            write_item_fields(&mut hasher, &table, thing, &registry);
            hasher.finish()
        };
        let entry = |item: ContentIndex, count: u32, weight: u32| BlindBoxEntry {
            item,
            count,
            weight,
        };

        // Act
        let baseline = digest(false, 0, Vec::new());

        // Assert：三列各自单独改动都必须改变摘要，盲盒池的三个分量
        // （产出物 / 数量 / 权重）也要各自可分辨。
        assert_ne!(baseline, digest(true, 0, Vec::new()));
        assert_ne!(baseline, digest(false, 1, Vec::new()));
        assert_ne!(baseline, digest(false, 0, vec![entry(prize_one, 1, 1)]));
        assert_ne!(
            digest(false, 0, vec![entry(prize_one, 1, 1)]),
            digest(false, 0, vec![entry(prize_two, 1, 1)])
        );
        assert_ne!(
            digest(false, 0, vec![entry(prize_one, 1, 1)]),
            digest(false, 0, vec![entry(prize_one, 2, 1)])
        );
        assert_ne!(
            digest(false, 0, vec![entry(prize_one, 1, 1)]),
            digest(false, 0, vec![entry(prize_one, 1, 2)])
        );
    }

    /// 家具层批次新增的 `ItemDef.furniture` 真的进了物品摘要——见
    /// [`CONTENT_HASH_ALGORITHM_VERSION`] 文档「版本 24」一节。
    ///
    /// 守的失效模式与上面三条同形：把一件家具悄悄改成普通物品（或反
    /// 过来），存档若察觉不到，玩家读档后会发现自己摆在营地里的锻炉
    /// 三十个游戏日后自己没了，而版本校验会报「内容没变」。
    #[test]
    fn 家具标志进了物品摘要() {
        // Arrange：两件除了 furniture 之外逐字段相同的物品。
        use crate::item::ItemAttrs;
        use ll_core::scaled::Milli;
        use ll_sim::combat::Penetration;
        use ll_sim::item::SlotMask;

        let mut registry = Registry::new();
        let thing = registry.intern(id("yourmod:thing"));

        let digest = |furniture: bool| -> u64 {
            let mut table = ItemTable::new();
            table
                .define(
                    thing,
                    ItemAttrs {
                        display_name_key: id("yourmod:item.thing"),
                        stack_limit: 1,
                        base_weight: Milli::from_whole(1),
                        base_price: Milli::from_whole(2),
                        max_durability: None,
                        equip_mask: SlotMask::EMPTY,
                        stat_bonuses: Vec::new(),
                        use_effect: None,
                        penetration: Penetration::NONE,
                        damage_formula: None,
                        damage_category: None,
                        rule_modifiers: Vec::new(),
                        tags: Vec::new(),
                        taught_recipes: Vec::new(),
                        requires_identification: false,
                        study_experience: 0,
                        blind_box_pool: Vec::new(),
                        furniture,
                    },
                )
                .expect("测试用声明内部自洽");
            let mut hasher = StateHasher::new();
            write_item_fields(&mut hasher, &table, thing, &registry);
            hasher.finish()
        };

        // Act & Assert
        assert_ne!(digest(false), digest(true));
    }

    /// 配方发现批次新增的 `RecipeDef.requires_discovery` 真的进了摘要
    /// ——见 [`CONTENT_HASH_ALGORITHM_VERSION`] 文档「版本 15」一节。
    ///
    /// 失效模式与上一条对称：把一条人人会做的配方悄悄改成需要发现，
    /// 存档若察觉不到，玩家读档后会突然做不出一直在做的东西，而校验
    /// 会报「内容没变」。
    #[test]
    fn 要求发现的配方与不要求发现的配方摘要不同() {
        // Arrange：两条除了 requires_discovery 之外逐字段相同的配方。
        use crate::recipe::{RecipeAttrs, RecipeIngredient};

        let mut registry = Registry::new();
        let stew = registry.intern(id("yourmod:stew_recipe"));
        let cooking = registry.intern(id("yourmod:cooking"));
        let meat = registry.intern(id("yourmod:meat"));
        let bowl = registry.intern(id("yourmod:bowl"));

        let digest = |requires_discovery: bool| -> u64 {
            let mut table = RecipeTable::new();
            table
                .define(
                    stew,
                    RecipeAttrs {
                        display_name_key: id("yourmod:recipe.stew"),
                        category: cooking,
                        ingredients: vec![RecipeIngredient {
                            item: meat,
                            count: 1,
                        }],
                        product: bowl,
                        product_count: 1,
                    },
                )
                .expect("测试用声明内部自洽");
            if requires_discovery {
                table
                    .set_requires_discovery(stew)
                    .expect("刚 define 过，必然成功");
            }
            let mut hasher = StateHasher::new();
            write_recipe_fields(&mut hasher, &table, stew, &registry);
            hasher.finish()
        };

        // Act & Assert
        assert_ne!(digest(false), digest(true));
    }

    /// 副职天赋接线批次：`CONTENT_HASH_ALGORITHM_VERSION` 文档
    /// 「版本 18」一节点名的两条守门断言之一——授予天赋与不授予天赋
    /// 的同一个副职必须折出不同的摘要。若有人给
    /// `SubclassAttrs` 加了 `traits` 却忘了同步
    /// [`write_subclass_fields`]（那正是版本 15 之前 `RaceAttrs` 真实
    /// 发生过的漂移），本条立刻变红。
    #[test]
    fn 授予天赋的副职与不授予天赋的副职摘要不同() {
        // Arrange：两条除了 traits 之外逐字段相同的副职。
        use crate::subclass::SubclassAttrs;
        use ll_sim::traits::TraitGrant;

        let mut registry = Registry::new();
        let artisan = registry.intern(id("yourmod:artisan"));
        let lore = registry.intern(id("yourmod:smithing_lore"));

        let digest = |traits: Vec<TraitGrant>| -> u64 {
            let mut table = SubclassTable::new();
            table
                .define(
                    artisan,
                    SubclassAttrs {
                        display_name_key: id("yourmod:subclass.artisan"),
                        traits,
                    },
                )
                .expect("测试用声明内部自洽");
            let mut hasher = StateHasher::new();
            write_subclass_fields(&mut hasher, &table, artisan, &registry);
            hasher.finish()
        };

        // Act & Assert
        assert_ne!(
            digest(Vec::new()),
            digest(vec![TraitGrant {
                trait_id: lore,
                unlock_level: 1,
            }])
        );
    }

    /// 版本 18 的第二条守门断言：`unlock_level` 真的进了哈希输入——
    /// 只写 `trait_id`、漏写等级的话，「1 级就给」与「10 级才给」这两份
    /// 语义完全不同的内容会折出同一份摘要。
    #[test]
    fn 副职天赋解锁等级不同则摘要不同() {
        // Arrange
        use crate::subclass::SubclassAttrs;
        use ll_sim::traits::TraitGrant;

        let mut registry = Registry::new();
        let artisan = registry.intern(id("yourmod:artisan"));
        let lore = registry.intern(id("yourmod:smithing_lore"));

        let digest = |unlock_level: i32| -> u64 {
            let mut table = SubclassTable::new();
            table
                .define(
                    artisan,
                    SubclassAttrs {
                        display_name_key: id("yourmod:subclass.artisan"),
                        traits: vec![TraitGrant {
                            trait_id: lore,
                            unlock_level,
                        }],
                    },
                )
                .expect("测试用声明内部自洽");
            let mut hasher = StateHasher::new();
            write_subclass_fields(&mut hasher, &table, artisan, &registry);
            hasher.finish()
        };

        // Act & Assert
        assert_ne!(digest(1), digest(10));
    }

    #[test]
    fn classify_index对不落在任何表里的索引返回opaque() {
        // 直接验收 classify_index 的兜底分支——覆盖率回归测试
        // （ll_game::content）依赖的正是这条行为：只有 Opaque 才是"可以
        // 接受的遗漏"，本测试确认空表集合下任意索引确实分类为 Opaque，
        // 不是别的判别值。
        // Arrange
        let (
            terrain,
            class,
            skill,
            subclass,
            quest,
            space,
            clip,
            trait_def,
            pool,
            item,
            xp,
            formula,
            weapon_category,
            damage_category,
            weather,
            recipe,
            recipe_category,
        ) = empty_non_race_tables();
        let race = RaceTable::new();
        let dialogue = DialogueTable::new();
        let dialogue_node = DialogueNodeTable::new();
        let tables = ContentValueTables {
            dialogue: &dialogue,
            dialogue_node: &dialogue_node,
            terrain: &terrain,
            class: &class,
            skill: &skill,
            subclass: &subclass,
            quest: &quest,
            race: &race,
            space_profile: &space,
            clip: &clip,
            trait_def: &trait_def,
            resource_pool: &pool,
            item: &item,
            xp_curve: &xp,
            formula: &formula,
            weapon_category: &weapon_category,
            damage_category: &damage_category,
            weather: &weather,
            recipe: &recipe,
            recipe_category: &recipe_category,
            tag: &TagTable::new(),
            modifier_type: &ModifierTypeTable::new(),
            resource: &ResourceTable::new(),
            culture: &CultureTable::new(),
        };

        // Act
        let kind = classify_index(ContentIndex::default(), &tables);

        // Assert
        assert_eq!(kind, ContentTableKind::Opaque);
    }
    /// 建筑类型真的进了摘要：同一条文化、只换 `buildings`，摘要必须不同
    /// （版本 29 守门，见 [`CONTENT_HASH_ALGORITHM_VERSION`] 文档
    /// 「版本 29」一节）。
    ///
    /// 三份声明两两比对，各自只差一处：权重、家具种类、家具件数。只比
    /// 其中两份是不够的——混入代码里漏写 `count` 那一行的话，「换件数」
    /// 那一对会静默相等，而那正是最容易漏的一行。
    ///
    /// # 顺带如实记录一处**已经存在的**文档—代码分歧
    ///
    /// [`CONTENT_HASH_ALGORITHM_VERSION`] 文档「版本 27」一节写着守门的
    /// 是「本模块单元测试 `建材不同的两条文化摘要不同`」——**那条测试从
    /// 来没有存在过**（`grep 建材不同的两条文化摘要不同 crates/` 只命中
    /// 那句注释自己）。这正是同一段文字警告过的那件事：「提交信息声称
    /// 改了，不等于代码里真的改了」。本条测试同时补上那个缺口：它构造的
    /// 三份声明只在 `buildings` 上不同，但走的是 [`write_culture_fields`]
    /// 的完整字段流，任何一处混入被删掉都会让某一对当场相等。
    #[test]
    fn 建筑类型不同的两条文化摘要不同() {
        // Arrange
        //
        // **索引必须来自 `registry` 本身**，不能来自一个旁边的 `Interner`：
        // `write_culture_fields` 混的是 `registry.resolve(索引)` 换出来的
        // 命名空间 id，而一个查不到的索引一律写 0——那样「换家具种类」
        // 这一对会静默相等。第一版正是这么写的，本条测试当场把它咬住了。
        let mut registry = Registry::new();
        let mut id = |raw: &str| {
            registry.intern(ll_core::ident::NamespacedId::parse(raw).expect("合法标识符"))
        };
        let index = id("test:folk");
        let race = id("test:race");
        let chair = id("test:chair");
        let bed = id("test:bed");
        let digest = |buildings: Vec<ll_world::building::BuildingTemplate>| -> u64 {
            let mut table = CultureTable::new();
            table
                .define(
                    index,
                    ll_world::culture::CultureAttrs {
                        display_name_key: ll_core::ident::NamespacedId::parse("test:name")
                            .expect("合法标识符"),
                        economy: ll_world::resource::ResourceCategory::Food,
                        home_terrain: ll_world::terrain::TerrainKind::from_index(
                            ContentIndex::default(),
                        ),
                        wall_terrain: ll_world::terrain::TerrainKind::from_index(
                            ContentIndex::default(),
                        ),
                        founder_races: vec![(race, 1)],
                        hostility: Vec::new(),
                        buildings,
                    },
                )
                .expect("声明自洽");
            let mut hasher = StateHasher::new();
            write_culture_fields(&mut hasher, &table, index, &registry);
            hasher.finish()
        };
        let template = |weight: u32, item, count| ll_world::building::BuildingTemplate {
            weight,
            furniture: vec![(item, count)],
        };

        // Act：三份声明，两两之间只差一处。
        let digests = [
            digest(vec![template(1, chair, 1)]),
            // 只换权重
            digest(vec![template(2, chair, 1)]),
            // 只换家具种类
            digest(vec![template(1, bed, 1)]),
            // 只换件数
            digest(vec![template(1, chair, 2)]),
        ];

        // Assert：四份两两不同。
        for left in 0..digests.len() {
            for right in (left + 1)..digests.len() {
                assert_ne!(
                    digests[left], digests[right],
                    "第 {left} 份与第 {right} 份建筑声明不同，摘要却相同——                     write_culture_fields 少混了某一项"
                );
            }
        }
    }

    /// 资源的大类真的进了摘要：同一条资源、只换 `category`，摘要必须
    /// 不同（版本 24 守门，见 [`CONTENT_HASH_ALGORITHM_VERSION`] 文档
    /// 「版本 24」一节）。
    ///
    /// 五个大类两两比对而不是只比其中两个：本函数混的是
    /// `ResourceCategory::as_str` 的字符串，两个大类的字面量若不慎写
    /// 重（复制粘贴改漏一处），只比两个是发现不了的。
    #[test]
    fn 资源大类不同的两条资源摘要不同() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let index =
            interner.intern(ll_core::ident::NamespacedId::parse("test:ore").expect("合法标识符"));
        let registry = Registry::new();
        let digest = |category: ll_world::resource::ResourceCategory| -> u64 {
            let mut table = ll_world::resource::ResourceTable::new();
            table
                .define(
                    index,
                    ll_world::resource::ResourceAttrs {
                        display_name_key: ll_core::ident::NamespacedId::parse("test:name")
                            .expect("合法标识符"),
                        category,
                        source_terrain: ll_world::terrain::TerrainKind::from_index(
                            ContentIndex::default(),
                        ),
                        abundance: 100,
                        residents_supported: 1,
                        settlement_draw: 1,
                        exhaustible: false,
                    },
                )
                .expect("声明自洽");
            let mut hasher = StateHasher::new();
            write_resource_fields(&mut hasher, &table, index, &registry);
            hasher.finish()
        };

        // Act
        let digests: Vec<u64> = ll_world::resource::ResourceCategory::ALL
            .into_iter()
            .map(digest)
            .collect();

        // Assert：五个两两不同。
        let categories = ll_world::resource::ResourceCategory::ALL;
        for left in 0..digests.len() {
            for right in (left + 1)..digests.len() {
                assert_ne!(
                    digests[left], digests[right],
                    "大类 {} 与 {} 的摘要撞了",
                    categories[left], categories[right]
                );
            }
        }
    }

    /// 对话节点的跳转目标真的进了摘要：同一个节点、只换选项的 `next`，
    /// 摘要必须不同（版本 28 守门，见 [`CONTENT_HASH_ALGORITHM_VERSION`]
    /// 文档「版本 28」一节）。
    ///
    /// 三种形态两两比对：`end`、跳到 A、跳到 B。只比前两种发现不了
    /// 「`Node` 那一支忘了把目标索引混进去」——那正是最容易漏的一处，
    /// 因为它是唯一需要 `Registry::resolve` 的一步。
    #[test]
    fn 跳转目标不同的两个对话节点摘要不同() {
        // Arrange
        let mut registry = Registry::new();
        let node = registry.intern(NamespacedId::parse("test:root").expect("合法标识符"));
        let a = registry.intern(NamespacedId::parse("test:a").expect("合法标识符"));
        let b = registry.intern(NamespacedId::parse("test:b").expect("合法标识符"));
        let digest = |next: DialogueNext| -> u64 {
            let mut table = DialogueNodeTable::new();
            table
                .define(
                    node,
                    crate::dialogue::DialogueNodeAttrs {
                        text_key: NamespacedId::parse("test:dialogue.root").expect("合法标识符"),
                        options: vec![crate::dialogue::DialogueOption {
                            text_key: NamespacedId::parse("test:dialogue.go").expect("合法标识符"),
                            conditions: Vec::new(),
                            next,
                        }],
                    },
                )
                .expect("声明自洽");
            let mut hasher = StateHasher::new();
            write_dialogue_node_fields(&mut hasher, &table, node, &registry);
            hasher.finish()
        };

        // Act
        let digests = [
            digest(DialogueNext::End),
            digest(DialogueNext::Node(a)),
            digest(DialogueNext::Node(b)),
        ];

        // Assert
        for left in 0..digests.len() {
            for right in (left + 1)..digests.len() {
                assert_ne!(
                    digests[left], digests[right],
                    "跳转目标 {left} 与 {right} 的摘要撞了"
                );
            }
        }
    }

    /// 十条谓词的判别值两两不同，且带参数的那几条真的把参数混进去了
    /// （版本 28 守门）。
    ///
    /// 判别值撞车的后果是「两条语义完全不同的条件算出同一个摘要」——
    /// 内容改了而哈希没变，正是内容哈希这套机制要拦的那件事。
    #[test]
    fn 十条对话谓词的摘要两两不同() {
        // Arrange
        let mut registry = Registry::new();
        let target = registry.intern(NamespacedId::parse("test:target").expect("合法标识符"));
        let other = registry.intern(NamespacedId::parse("test:other").expect("合法标识符"));
        let flag = NamespacedId::parse("test:flag").expect("合法标识符");
        let query = AffiliationQuery {
            kind: AffiliationKind::Faction,
            org: None,
        };
        let conditions = [
            DialogueCondition::Affiliated(query),
            DialogueCondition::NotAffiliated(query),
            DialogueCondition::StandingAtLeast { query, value: 250 },
            DialogueCondition::QuestCompleted(target),
            DialogueCondition::QuestNotCompleted(target),
            DialogueCondition::FlagSet(flag.clone()),
            DialogueCondition::FlagNotSet(flag),
            DialogueCondition::HasItem {
                item: target,
                count: 1,
            },
            DialogueCondition::WalletAtLeast(250),
            DialogueCondition::IsRace(target),
            // 参数敏感性：与上面几条只差一个参数。
            DialogueCondition::StandingAtLeast { query, value: 251 },
            DialogueCondition::QuestCompleted(other),
            DialogueCondition::HasItem {
                item: target,
                count: 2,
            },
            DialogueCondition::Affiliated(AffiliationQuery {
                kind: AffiliationKind::Guild,
                org: None,
            }),
            DialogueCondition::Affiliated(AffiliationQuery {
                kind: AffiliationKind::Faction,
                org: Some(target),
            }),
        ];

        // Act
        let digests: Vec<u64> = conditions
            .iter()
            .map(|condition| {
                let mut hasher = StateHasher::new();
                write_dialogue_condition(&mut hasher, condition, &registry);
                hasher.finish()
            })
            .collect();

        // Assert
        for left in 0..digests.len() {
            for right in (left + 1)..digests.len() {
                assert_ne!(
                    digests[left], digests[right],
                    "条件 {:?} 与 {:?} 的摘要撞了",
                    conditions[left], conditions[right]
                );
            }
        }
    }
}
