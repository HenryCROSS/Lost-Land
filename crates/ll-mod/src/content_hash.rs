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
//! 本模块补上缺的那一半：在全部六张内容表装载完毕后，遍历每一条已
//! 注册内容,把它的字段值也折进同一份按命名空间统计的哈希。
//!
//! # 为什么不能在 `Registry::intern` 内部做
//!
//! `intern` 只知道字符串 id，不知道、也不应该知道六张内容表
//! （[`crate::class::ClassTable`]/[`crate::skill::SkillTable`]/……）的
//! 具体形状——那些类型分散在 `ll-mod`（五张）与 `ll-world`（地形，见
//! `ll_world::terrain::TerrainTable`）两个 crate，`Registry` 本身是一个
//! 通用的字符串 ↔ 索引映射池，不应该反过来认识每一种具体内容的字段
//! 布局（那会让 `Registry` 与六张表的字段变化互相耦合）。更根本的
//! 问题是时序：`intern` 调用发生在 `*Table::define` **之前**（先拿到
//! 索引，再用索引登记属性），那一刻字段值压根还不存在。
//!
//! 因此值哈希被设计成装载完成后的一次性收尾步骤：
//! [`apply_value_hashes`] 拿到 `&mut Registry` 与全部六张表的只读引用
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
//! 每张表 `hash_entry` 系列函数把该表 `*Attrs`/`*Def` 声明的**全部**
//! 字段都混入摘要，不排除任何一个——包括
//! `display_name_key`（指向 Fluent 本地化键的标识符，不是渲染出来的
//! 文本本身）这类"看起来只影响展示"的字段。
//!
//! 这不是没想清楚就把所有字段一股脑塞进去：本次升级要修的问题恰恰是
//! "旧哈希因为只看 id 集合而漏掉了真实的内容变化"，若在这里又主观排除
//! 一部分字段（哪怕理由听起来合理，比如"这只是个显示名，不影响
//! 数值"），就是在换一种方式重新制造同一类盲区——排除的判据一旦掺入
//! "这个字段看起来不重要"这类主观判断，就没有一条能站得住脚、不会
//! 被下一个字段的加入打破的分界线。`display_name_key` 本身是内容作者
//! 显式登记进 `*Attrs` 的技术标识符（不是渲染文本，渲染文本走独立的
//! Fluent 本地化系统，见 `crate::class` 模块文档「查询接口」一节前一
//! 段），把它改名同样是一次真实的内容变更（这个技能现在指向另一条
//! Fluent 词条），玩家/mod 作者理应被这条哈希告知,而不是被静默吞掉。
//! 因此本模块采用「凡是 `*Attrs` 里声明的字段，一律参与哈希」这一条
//! 不需要逐字段拍脑袋的规则。
//!
//! # `ContentIndex` 字段：解析成 `NamespacedId` 字符串再混入
//!
//! `SkillDef.owning_class`/`SkillDef.prerequisites`/
//! `QuestNodeDef.prerequisites`/`QuestCondition::KillCount.target_kind`/
//! `TerrainDef.opens_into` 这类字段的类型是 `ContentIndex`（或
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
//! # 浮点字段：当前六张内容表里不存在，未来新增前必须先处理
//!
//! 项目世界状态本身禁止浮点（`ll_world::entity::stats` 模块文档「全部
//! 整数」），检索过六张内容表当前的全部字段（[`crate::class::ClassDef`]/
//! [`crate::skill::SkillDef`]/[`crate::subclass::SubclassDef`]/
//! [`crate::quest::QuestNodeDef`]/[`crate::race::RaceDef`]/
//! `ll_world::terrain::TerrainDef`，含它们引用的
//! `ll_world::entity::BaseStats`/`ll_world::entity::AttributeKind`/
//! `ll_sim::skill::{ResourceCost, ResourceKind, SkillEffect}`/
//! `crate::quest::QuestCondition`）确认**当前没有任何 `f32`/`f64`
//! 字段**，本模块因此没有为浮点提供任何哈希路径——`StateHasher` 目前
//! 也确实没有 `write_f64` 这类方法。
//!
//! 这不是遗漏，是刻意不做 YAGNI 之外的预防性设计：若未来某张内容表
//! 真的新增浮点字段，**不能**直接 `f64::to_bits()` 后当整数混入——
//! IEEE 754 里 `NaN` 有大量不同的位模式（都表示"不是数字"，但位不同），
//! `+0.0` 与 `-0.0` 位模式不同但数值相等，两者都会让"逻辑上相同的值"
//! 产出不同的哈希，或者"同一个 NaN 的不同位模式"被误判为不同内容。
//! 正确做法是先规范化（例如用 `f64::total_cmp` 依赖的规范排序，或者
//! 统一把 `NaN` 折叠成一个固定位模式、把 `-0.0` 规范成 `0.0`）再混入
//! 整数摘要——这条备忘留给真的引入浮点字段的那次改动去做，不在这里
//! 提前造一个当前用不到、也没有测试能验证对不对的通用浮点哈希函数。

use ll_core::hashing::StateHasher;
use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::skill::{ResourceCost, SkillEffect};
use ll_world::entity::BaseStats;
use ll_world::terrain::{TerrainKind, TerrainTable};

use crate::class::ClassTable;
use crate::quest::{QuestCondition, QuestTable};
use crate::race::RaceTable;
use crate::registry::Registry;
use crate::skill::SkillTable;
use crate::subclass::SubclassTable;

/// 内容哈希算法的版本号——每当「字段值具体怎么编码进哈希」这件事发生
/// 不兼容变化就必须递增（新增一张内容表、给已有内容表新增一个此前从
/// 未参与哈希的字段，都不算——那只是让哈希覆盖得更全，同一份内容在
/// 新旧代码下若字段集合没变，摘要仍然相同；真正需要递增的是"同一份
/// 内容,字节编码方式变了",本次从「只追踪 id 集合」升级到「追踪字段
/// 值」正是这一类）。
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
/// 当前值 `1`：`0` 专门留作「早于本字段存在」的哨兵（存档 JSON 缺这个
/// 键时 `#[serde(default)]` 补 `0`），不代表任何真实算法，因此第一个
/// 真实算法从 `1` 开始编号,不是 `0`。
pub const CONTENT_HASH_ALGORITHM_VERSION: u32 = 1;

/// 六张玩法内容表的只读引用集合——供 [`apply_value_hashes`] 遍历使用。
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
}

/// 表种类判别字节——混入每条内容摘要的最前面，避免"一个地形的字段值"
/// 与"一个种族的字段值"凑巧编码成同一段字节流时被误判成同一份内容
/// （不同表的字段数量、取值范围都可能在某些边界组合下重叠）。
const TABLE_OPAQUE: u64 = 0;
const TABLE_TERRAIN: u64 = 1;
const TABLE_CLASS: u64 = 2;
const TABLE_SKILL: u64 = 3;
const TABLE_SUBCLASS: u64 = 4;
const TABLE_QUEST: u64 = 5;
const TABLE_RACE: u64 = 6;

/// 全部内容装载完毕后调用一次：把六张内容表的字段值折进 `registry`
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

    let terrain_kind = TerrainKind::from_index(index);
    if tables.terrain.is_defined(terrain_kind) {
        hasher.write_u64(TABLE_TERRAIN);
        write_terrain_fields(&mut hasher, tables.terrain, terrain_kind, registry);
    } else if tables.class.is_defined(index) {
        hasher.write_u64(TABLE_CLASS);
        write_class_fields(&mut hasher, tables.class, index);
    } else if tables.skill.is_defined(index) {
        hasher.write_u64(TABLE_SKILL);
        write_skill_fields(&mut hasher, tables.skill, index, registry);
    } else if tables.subclass.is_defined(index) {
        hasher.write_u64(TABLE_SUBCLASS);
        write_subclass_fields(&mut hasher, tables.subclass, index);
    } else if tables.quest.is_defined(index) {
        hasher.write_u64(TABLE_QUEST);
        write_quest_fields(&mut hasher, tables.quest, index, registry);
    } else if tables.race.is_defined(index) {
        hasher.write_u64(TABLE_RACE);
        write_race_fields(&mut hasher, tables.race, index);
    } else {
        // 不落在任何一张表里的纯 id 引用——例如
        // `base_placeholder::PLACEHOLDER_RACE_ID`，或
        // `QuestCondition::KillCount::target_kind` 指向的"敌人类型"
        // 占位标识符（当前代码库还没有敌人类型注册表，见
        // `crate::quest` 模块文档「跨表引用」一节）。没有字段可哈希，
        // 只哈希 id 本身——与升级前的行为一致，这类内容"是否存在"
        // 仍然能被检测到，只是没有字段值可比对。
        hasher.write_u64(TABLE_OPAQUE);
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

/// 混入 [`crate::race::RaceDef`] 的全部字段。
fn write_race_fields(hasher: &mut StateHasher, table: &RaceTable, index: ContentIndex) {
    let view = table
        .get(index)
        .expect("调用方已确认 is_defined，get 必返回 Some");
    hasher.write_namespaced_id(view.display_name_key);
    write_base_stats(hasher, view.stat_modifiers);
    hasher.write_i64(i64::from(view.darkvision_floor));
    hasher.write_u64(u64::from(view.footprint.0));
    hasher.write_u64(u64::from(view.footprint.1));
    hasher.write_u64(u64::from(view.lifespan_years));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::RaceAttrs;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    /// 一份内部自洽、全部字段为固定占位值的种族属性——测试只关心
    /// `darkvision_floor` 这一个字段是否驱动哈希变化时，用它避免每个
    /// 测试都重复拼一遍其余四个字段。
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
        }
    }

    /// 空的五张表（地形/职业/技能/副职/任务）——测试只关心种族表时，
    /// 用它填满 [`ContentValueTables`] 剩余字段。
    fn empty_non_race_tables() -> (
        TerrainTable,
        ClassTable,
        SkillTable,
        SubclassTable,
        QuestTable,
    ) {
        (
            TerrainTable::new(),
            ClassTable::new(),
            SkillTable::new(),
            SubclassTable::new(),
            QuestTable::new(),
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
        let (terrain_a, class_a, skill_a, subclass_a, quest_a) = empty_non_race_tables();

        let mut registry_b = Registry::new();
        let index_b = registry_b.intern(id("yourmod:dwarf"));
        let mut race_b = RaceTable::new();
        race_b
            .define(index_b, race_attrs("yourmod:dwarf_name", 9))
            .expect("测试用声明内部自洽");
        let (terrain_b, class_b, skill_b, subclass_b, quest_b) = empty_non_race_tables();

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
            },
        );

        // Assert
        assert_ne!(
            registry_a.content_hash_of("yourmod"),
            registry_b.content_hash_of("yourmod")
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
        let (terrain_f, class_f, skill_f, subclass_f, quest_f) = empty_non_race_tables();

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
        let (terrain_r, class_r, skill_r, subclass_r, quest_r) = empty_non_race_tables();

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
        let (terrain_before, class_before, skill_before, subclass_before, quest_before) =
            empty_non_race_tables();

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
        let (terrain_after, class_after, skill_after, subclass_after, quest_after) =
            empty_non_race_tables();

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
        let (terrain, class, skill, subclass, quest) = empty_non_race_tables();
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
            },
        );

        // Assert
        assert!(registry.content_hash_of("lostland").is_some());
    }
}
