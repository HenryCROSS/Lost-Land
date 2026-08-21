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
//! `SkillDef.owning_class`/`prerequisites`、
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
use ll_world::entity::BaseStats;
use ll_world::space_profile::SpaceProfileTable;
use ll_world::terrain::{TerrainKind, TerrainTable};

use crate::class::ClassTable;
use crate::clip::ClipTable;
use crate::item::ItemTable;
use crate::quest::{QuestCondition, QuestTable};
use crate::race::RaceTable;
use crate::registry::Registry;
use crate::resource_pool::ResourcePoolTable;
use crate::skill::SkillTable;
use crate::subclass::SubclassTable;
use crate::trait_def::{RuleModifier, TraitTable};
use crate::xp_curve::XpCurveTable;

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
pub const CONTENT_HASH_ALGORITHM_VERSION: u32 = 2;

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
        ContentTableKind::Class => write_class_fields(&mut hasher, tables.class, index),
        ContentTableKind::Skill => {
            write_skill_fields(&mut hasher, tables.skill, index, registry);
        }
        ContentTableKind::Subclass => write_subclass_fields(&mut hasher, tables.subclass, index),
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
        ContentTableKind::Item => write_item_fields(&mut hasher, tables.item, index),
        ContentTableKind::XpCurve => write_xp_curve_fields(&mut hasher, tables.xp_curve, index),
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

/// 把一份 [`BaseStats`] 混入哈希——六项主属性逐一混入，顺序与字段
/// 声明顺序一致。与 `ll_world::state::write_stats`（该 crate 内部
/// 私有）是同一种写法的独立实现：两者服务不同的哈希（世界状态摘要
/// vs. 内容值哈希），不适合跨 crate 共享同一个私有帮手函数。
fn write_base_stats(hasher: &mut StateHasher, stats: BaseStats) {
    hasher.write_i64(i64::from(stats.strength));
    hasher.write_i64(i64::from(stats.dexterity));
    hasher.write_i64(i64::from(stats.constitution));
    hasher.write_i64(i64::from(stats.intelligence));
    hasher.write_i64(i64::from(stats.willpower));
    hasher.write_i64(i64::from(stats.charisma));
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
fn write_class_fields(hasher: &mut StateHasher, table: &ClassTable, index: ContentIndex) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    hasher.write_u64(view.primary_attribute as u64);
}

/// 混入 [`crate::subclass::SubclassDef`] 的全部字段。
fn write_subclass_fields(hasher: &mut StateHasher, table: &SubclassTable, index: ContentIndex) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
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
    hasher.write_i64(i64::from(view.darkvision_floor));
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
    for modifier in view.rule_modifiers {
        write_rule_modifier(hasher, modifier, registry);
    }
    hasher.write_u64(view.granted_resource_pools.len() as u64);
    for grant in view.granted_resource_pools {
        write_optional_resolved(hasher, Some(grant.pool), registry);
        write_capacity_formula(hasher, &grant.capacity);
    }
}

/// 混入一个 [`RuleModifier`]，理由同 [`write_resource_cost`]。
/// `Resistance.damage_category` 解析成 `NamespacedId` 字符串；
/// `Advantage`/`Disadvantage.check_context` 是字面 `NamespacedId`，
/// 直接混入。
fn write_rule_modifier(hasher: &mut StateHasher, modifier: &RuleModifier, registry: &Registry) {
    match modifier {
        RuleModifier::Resistance {
            damage_category,
            multiplier_permille,
        } => {
            hasher.write_u64(0);
            write_optional_resolved(hasher, Some(*damage_category), registry);
            hasher.write_i64(i64::from(*multiplier_permille));
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
/// 批次新增）——`base_weight`/`base_price` 是 `Milli` 定点整数
/// （`pub struct Milli(pub i64)`），按普通 `i64` 处理；`use_effect`
/// 复用既有的 [`write_skill_effect`]（与 `SkillDef.effect` 共用同一个
/// 类型，见 [`crate::item::ItemDef::use_effect`] 文档「为什么复用
/// `SkillEffect`」一节）。不接受 `&Registry` 参数——核实过
/// `ItemDef` 当前没有任何 `ContentIndex` 字段（`equip_mask`/
/// `stat_bonuses`/`use_effect`/`penetration` 均不携带），没有需要
/// `Registry::resolve` 的字段，与其余表统一接受 `registry` 参数不同，
/// 如实按实际需要签名，不为了"看起来一致"而假装用到。
fn write_item_fields(hasher: &mut StateHasher, table: &ItemTable, index: ContentIndex) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::RaceAttrs;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    /// 一份内部自洽、全部字段为固定占位值的种族属性——测试只关心
    /// `darkvision_floor` 这一个字段是否驱动哈希变化时，用它避免每个
    /// 测试都重复拼一遍其余字段。
    fn race_attrs(display_name: &str, darkvision_floor: i32) -> RaceAttrs {
        RaceAttrs {
            display_name_key: id(display_name),
            stat_modifiers: BaseStats {
                strength: 0,
                dexterity: 0,
                constitution: 0,
                intelligence: 0,
                willpower: 0,
                charisma: 0,
            },
            darkvision_floor,
            footprint: (1, 1),
            lifespan_years: 80,
            xp_reward: 0,
            traits: Vec::new(),
            starting_items: Vec::new(),
        }
    }

    /// 空的十一张非种族表——测试只关心其中一张表时，用它填满
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
        )
    }

    #[test]
    fn 只改字段值不改id集合时命名空间哈希改变() {
        // 本次升级的核心验收：旧版"只追踪 id 集合"的哈希对这个场景
        // 完全无感（id 一个字符没变），值哈希必须能看见这条差异。
        // Arrange：两个 registry 各自注册同一个种族 id，唯一的差异是
        // darkvision_floor 的取值。
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
        let (terrain, class, skill, subclass, quest, space, clip, trait_def, pool, item, xp) =
            empty_non_race_tables();
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
            },
        );

        // Assert
        assert!(registry.content_hash_of("lostland").is_some());
    }

    #[test]
    fn classify_index对不落在任何表里的索引返回opaque() {
        // 直接验收 classify_index 的兜底分支——覆盖率回归测试
        // （ll_game::content）依赖的正是这条行为：只有 Opaque 才是"可以
        // 接受的遗漏"，本测试确认空表集合下任意索引确实分类为 Opaque，
        // 不是别的判别值。
        // Arrange
        let (terrain, class, skill, subclass, quest, space, clip, trait_def, pool, item, xp) =
            empty_non_race_tables();
        let race = RaceTable::new();
        let tables = ContentValueTables {
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
        };

        // Act
        let kind = classify_index(ContentIndex::default(), &tables);

        // Assert
        assert_eq!(kind, ContentTableKind::Opaque);
    }
}
