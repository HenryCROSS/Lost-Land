//! 文化那一张表的内容值哈希：[`write_culture_fields`] 与它的两条守卫
//! 测试。
//!
//! # 为什么单独一个文件
//!
//! 不是「按表切分」这条通则——本模块的其余十几张表仍然住在
//! `content_hash.rs` 里。是**文件行数棘轮**（规格 §13）逼出来的一次
//! 具体切分：NPC 姓名批次（2026-09-02，批次 34）给
//! [`ll_world::culture::CultureAttrs`] 加了 `naming`，混入段与那条新守卫
//! 测试合计约 90 行，而 `content_hash.rs` 早已在棘轮快照里超限、只许缩
//! 不许涨。**先拆再 bless**（交接纪律）：拆出来的这一份是文化表**自己**
//! 的那一段——混入函数 + 守着它的两条测试，边界清楚。
//!
//! 拆出来之后 `content_hash.rs` 净缩，棘轮不需要为它放宽。
//!
//! # 判据一个字没改
//!
//! 函数正文、文档、两条测试全部是搬运。搬运不可能改变内容摘要（摘要只
//! 取决于混入的字节序，与代码住在哪个文件无关），而这句话不是声称：
//! 本次搬运与内容改动同一批落地，由既有的
//! `content_hash 随注册内容变化而变化` 与本文件两条测试一起兜住。

use ll_core::ident::ContentIndex;
use ll_world::culture::{CultureKind, CultureTable};

use crate::registry::Registry;

use super::StateHasher;

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
pub(super) fn write_culture_fields(
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
    // 命名规则（版本 35）：音节数区间，然后按 `BTreeMap` 的键序逐条混
    // 「语言标签 + 三张表各自的长度 + 逐项字节」。
    //
    // **不经 `Registry::resolve`**，与上面每一段都不同：音素是字面字符串，
    // 不是 `ContentIndex`——它们压根不进注册表，也就不会随装载顺序漂移，
    // 本模块「`ContentIndex` 字段」那条纪律对它们不适用。
    //
    // 表长先写、再逐项写：与 `founder_races`/`buildings` 逐字同一形状，
    // 且这里的长度本身就是判据的一部分（各语言表长必须相等，
    // `ll_world::naming::CultureNaming::problem`）。
    match table.naming(kind) {
        None => hasher.write_u64(0),
        Some(naming) => {
            hasher.write_u64(1);
            hasher.write_u64(u64::from(naming.syllables.0));
            hasher.write_u64(u64::from(naming.syllables.1));
            hasher.write_u64(naming.phonemes.len() as u64);
            for (language, tables) in &naming.phonemes {
                hasher.write_len_prefixed_bytes(language.as_bytes());
                for phonemes in [&tables.onsets, &tables.nuclei, &tables.codas] {
                    hasher.write_u64(phonemes.len() as u64);
                    for phoneme in phonemes {
                        hasher.write_len_prefixed_bytes(phoneme.as_bytes());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                        naming: ll_world::naming::bare_naming_fixture(),
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

    /// 命名规则真的进了摘要：同一条文化、只换 `naming` 的某一处，摘要
    /// 必须不同（版本 35 守门，见 [`CONTENT_HASH_ALGORITHM_VERSION`]
    /// 文档「版本 35」一节）。
    ///
    /// 五份声明两两比对，各自只差一处：音节数下限、音节数上限、语言
    /// 标签、某一张表的内容、以及**只换 `codas`**。最后那一条不是凑数
    /// ——`codas` 允许为空，是三张表里最容易被漏掉的一张。
    #[test]
    fn 命名规则不同的两条文化摘要不同() {
        // Arrange
        let mut registry = Registry::new();
        let mut id = |raw: &str| {
            registry.intern(ll_core::ident::NamespacedId::parse(raw).expect("合法标识符"))
        };
        let index = id("test:folk");
        let race = id("test:race");
        let digest = |naming: ll_world::naming::CultureNaming| -> u64 {
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
                        buildings: ll_world::building::bare_building_fixture(),
                        naming,
                    },
                )
                .expect("声明自洽");
            let mut hasher = StateHasher::new();
            write_culture_fields(&mut hasher, &table, index, &registry);
            hasher.finish()
        };
        let naming = |language: &str, syllables: (u8, u8), onset: &str, coda: &str| {
            let mut phonemes = std::collections::BTreeMap::new();
            phonemes.insert(
                language.to_string(),
                ll_world::naming::PhonemeTables {
                    onsets: vec![onset.to_string()],
                    nuclei: vec!["a".to_string()],
                    codas: vec![coda.to_string()],
                },
            );
            ll_world::naming::CultureNaming {
                syllables,
                phonemes,
            }
        };

        // Act：五份声明，两两之间只差一处。
        let digests = [
            digest(naming("en", (2, 3), "k", "n")),
            // 只换音节数下限
            digest(naming("en", (1, 3), "k", "n")),
            // 只换音节数上限
            digest(naming("en", (2, 4), "k", "n")),
            // 只换语言标签
            digest(naming("zh-CN", (2, 3), "k", "n")),
            // 只换声母
            digest(naming("en", (2, 3), "t", "n")),
            // 只换韵尾
            digest(naming("en", (2, 3), "k", "r")),
        ];

        // Assert：六份两两不同。
        for left in 0..digests.len() {
            for right in (left + 1)..digests.len() {
                assert_ne!(
                    digests[left], digests[right],
                    "第 {left} 份与第 {right} 份命名声明不同，摘要却相同——\
                     write_culture_fields 少混了某一项"
                );
            }
        }
    }
}
