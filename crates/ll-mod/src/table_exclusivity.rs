//! 跨表撞名：**一个 `ContentIndex` 至多被一张内容表 `define`**。
//!
//! # 为什么这是一条需要专门守的性质
//!
//! [`crate::registry::Registry`] 是**一个**全局的 id ↔ `ContentIndex`
//! 空间（[ADR 0015](../../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)
//! 论证了为什么「已注册」是一次解析、不是类型的不变式），而 `define`
//! 分散在二十四张**互不知情**的表上。两者合起来意味着「同一个字符串 id
//! 既是一件物品又是一种资源」在类型层面完全合法：两张表各自都答得出来，
//! 谁也不会报错，而「索引 `i` 是哪张表的」再也问不清楚。
//!
//! 本模块提供这条性质的两个层次：
//!
//! - [`tables_defining`]：一个索引被**哪些**表定义（
//!   [`crate::content_hash::classify_index`] 只取它的第一项，是它唯一的
//!   事实来源）。
//! - [`detect_table_define_collisions`]：整次装载会话里全部长度 ≥ 2 的
//!   那些，由 `ll_mod::content_audit` 接进装载后校验、由
//!   `ll_game::content::load_content` 直接 `?` 成硬失败。
//!
//! 门禁脚本是 `scripts/ci/check_content_index_table_exclusivity.sh`。
//!
//! # 为什么单独一个模块
//!
//! 判据的两半天然分居两处——「谁定义了这个索引」要问全部内容表
//! （`content_hash` 那侧的 [`ContentValueTables`]），「怎么把它报成一条
//! 可行动的错误」是装载后校验（`content_audit` 那侧）的形状。放进任何
//! 一边都会让那个文件继续膨胀（两个文件都已经超过规格 §13 的 800 行
//! 上限，`scripts/ci/check_file_size_budget.py` 的棘轮门禁当场拦下了
//! 第一版），也会让「撞名」这件事没有一个能一眼找到的家。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_world::terrain::TerrainKind;

use crate::content_audit::table_label;
use crate::content_hash::{ContentTableKind, ContentValueTables};
use crate::registry::Registry;

/// 一个 `ContentIndex` 到底被**哪些**内容表 `define` 过——[`crate::content_hash::classify_index`]
/// 的「全部答案」版本，也是它唯一的事实来源（那个函数只是取本函数结果的
/// 第一项）。
///
/// # 一个索引至多一张表：为什么这需要被显式检查
///
/// [`crate::registry::Registry`] 是**一个** id ↔ `ContentIndex` 空间：
/// `intern` 全仓库共享一份，而 `define` 分散在二十四张互不知情的表上。
/// 两者合起来意味着「同一个 `NamespacedId` 同时是一件物品和一种资源」
/// 在类型层面完全合法——`ItemTable::get(i)` 与 `ResourceTable::get(i)`
/// 会各自答出一个不同的东西，而「索引 `i` 是哪张表的」再也问不清楚。
///
/// **这不是假想。** 树木系统批次（2026-09-01）新增的砍伐产出物品第一版
/// 就叫 `lostland:timber`，与 `mods/lostland/resources.json5` 里早已存在
/// 的据点资源「木材」逐字相同，两者因此共用同一个 `ContentIndex`
/// （详情见 [`crate::tree`] 的 `TIMBER_ID` 文档）。当时**没有任何判据
/// 拦住它**：json5 schema 校验绿、[`crate::content_audit`] 的跨表引用
/// 完整性检查绿、「本体物品清单不多不少」的名册测试也绿——抓到它的只是
/// 一个对不上的数字。这正是
/// [ADR 0022](../../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)
/// 描述的形状：判据齐全，覆盖面却漏掉了整整一类错误。
///
/// 撞名还会**顺手弄坏内容哈希**：[`crate::content_hash::classify_index`] 取第一项，于是那条
/// 内容只会按第一张表的 `write_*_fields` 求摘要，第二张表里那份字段值
/// 从此完全不进哈希——[ADR 0027](../../../../knowledge/decisions/0027-content-hash-covers-field-values.md)
/// 要堵的「改了字段值却哈希不变」缺口，会被一次撞名重新打开。
///
/// # 返回值
///
/// 按 [`ContentTableKind`] 判别值以外的一个固定顺序（即本函数体内的书写
/// 顺序，与升级前 `classify_index` 的 `else if` 链逐条一致）返回全部命中
/// 的表；一张都没命中时返回空 `Vec`，对应
/// [`ContentTableKind::Opaque`]。**长度 ≥ 2 就是一次撞名**，由
/// `ll_mod::content_audit::detect_table_define_collisions` 判定并上报。
///
/// # 编译期强制：穷尽解构 `*tables`
///
/// 与升级前的 [`crate::content_hash::classify_index`] 完全相同、且现在只剩这一处：函数体第一行
/// 对 `*tables` 做不带 `..` 的穷尽解构，给 [`ContentValueTables`] 新增字段
/// 而忘记在这里处理，`cargo build` 当场报 "pattern does not mention
/// field"。新表因此不会悄悄逃过撞名检查——这与本函数守的东西是同一条
/// 纪律：漏掉一张表的判据，等于对那张表没有判据。
pub fn tables_defining(
    index: ContentIndex,
    tables: &ContentValueTables<'_>,
) -> Vec<ContentTableKind> {
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

    let mut kinds = Vec::new();
    let mut hit = |defined: bool, kind: ContentTableKind| {
        if defined {
            kinds.push(kind);
        }
    };

    hit(
        terrain.is_defined(TerrainKind::from_index(index)),
        ContentTableKind::Terrain,
    );
    hit(class.is_defined(index), ContentTableKind::Class);
    hit(skill.is_defined(index), ContentTableKind::Skill);
    hit(subclass.is_defined(index), ContentTableKind::Subclass);
    hit(quest.is_defined(index), ContentTableKind::Quest);
    hit(race.is_defined(index), ContentTableKind::Race);
    hit(
        space_profile.is_defined(index),
        ContentTableKind::SpaceProfile,
    );
    hit(clip.is_defined(index), ContentTableKind::Clip);
    hit(trait_def.is_defined(index), ContentTableKind::Trait);
    hit(
        resource_pool.is_defined(index),
        ContentTableKind::ResourcePool,
    );
    hit(item.is_defined(index), ContentTableKind::Item);
    hit(xp_curve.get(index).is_some(), ContentTableKind::XpCurve);
    hit(formula.get(index).is_some(), ContentTableKind::Formula);
    hit(
        weapon_category.is_defined(index),
        ContentTableKind::WeaponCategory,
    );
    hit(
        damage_category.is_defined(index),
        ContentTableKind::DamageCategory,
    );
    hit(weather.is_defined(index), ContentTableKind::Weather);
    hit(recipe.is_defined(index), ContentTableKind::Recipe);
    hit(
        recipe_category.is_defined(index),
        ContentTableKind::RecipeCategory,
    );
    hit(tag.is_defined(index), ContentTableKind::Tag);
    hit(
        modifier_type.is_defined(index),
        ContentTableKind::ModifierType,
    );
    hit(resource.is_defined(index), ContentTableKind::Resource);
    hit(culture.is_defined(index), ContentTableKind::Culture);
    hit(dialogue.is_defined(index), ContentTableKind::Dialogue);
    hit(
        dialogue_node.is_defined(index),
        ContentTableKind::DialogueNode,
    );

    kinds
}

/// 一次跨表撞名：同一个 `ContentIndex`（也就是同一个 `NamespacedId`）
/// 被两张或更多内容表 `define` 过。
///
/// 详细论证见 [`detect_table_define_collisions`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDefineCollision {
    /// 撞名的内容 id。
    pub id: NamespacedId,
    /// 它在本次装载里的索引——两张表拿到的是同一个数字，这正是问题
    /// 本身。
    pub index: ContentIndex,
    /// 定义了它的全部表，按 [`tables_defining`]
    /// 的固定顺序，长度恒 ≥ 2。第一项就是
    /// [`crate::content_hash::classify_index`] 会返回的那一个——也就是**独占**了值哈希与
    /// 本模块字段审计的那一张，其余各张的字段值全部静默失联。
    pub tables: Vec<ContentTableKind>,
}

/// 跨表撞名校验失败：至少一个 `ContentIndex` 被不止一张内容表定义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDefineCollisionError {
    /// **全部**撞名，不是第一条——理由同
    /// [`crate::content_audit::ReferenceIntegrityError::violations`]。
    pub collisions: Vec<TableDefineCollision>,
}

impl fmt::Display for TableDefineCollisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "跨表撞名校验失败：{} 个内容 id 同时被不止一张内容表定义。\
             注册表是**一个** id ↔ ContentIndex 空间，两张表因此拿到同一个索引，\
             「这个索引是哪张表的」再也问不清楚。",
            self.collisions.len()
        )?;
        for collision in &self.collisions {
            let tables = collision
                .tables
                .iter()
                .map(|kind| table_label(*kind))
                .collect::<Vec<_>>()
                .join("、");
            let winner = collision
                .tables
                .first()
                .map(|kind| table_label(*kind))
                .unwrap_or("<空>");
            writeln!(
                f,
                "  - {}（索引 {}）同时被 {} 定义；值哈希与内容审计只会当它是{}，\
                 其余各张表里那份字段值从此不进内容哈希、也不被字段覆盖审计看到。",
                collision.id,
                collision.index.get(),
                tables,
                winner,
            )?;
        }
        write!(
            f,
            "两条出路：一是给其中一方改名（先来的那张表保留原 id，\
             后加的那一方改名——`lostland:timber`/`lostland:timber_log` 就是这么分开的，\
             见 `ll_mod::tree` 的 TIMBER_ID 文档）；二是这两条内容本来就该是同一条，\
             那就删掉其中一处 `define`。**没有第三条**：让它们继续共用一个索引，\
             等于让内容哈希对其中一张表永久失明。"
        )
    }
}

impl std::error::Error for TableDefineCollisionError {}

/// 找出全部「同一个 `ContentIndex` 被多张内容表 `define`」的内容。
///
/// # 为什么需要这条判据：注册与定义是两个不同的空间
///
/// [`Registry::intern`](crate::registry::Registry::intern) 是**一个**
/// 全局的 id ↔ `ContentIndex` 空间（[ADR 0015](../../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)
/// 论证了为什么它必须是解析而不是不变式），而 `define` 分散在二十几张
/// **互不知情**的表上。两者合起来意味着「同一个字符串 id 既是一件物品
/// 又是一种资源」在类型层面完全合法：`ItemTable::get(i)` 与
/// `ResourceTable::get(i)` 会各自答出一个不同的东西，谁也不会报错。
///
/// # 三条既有判据为什么一条都拦不住它
///
/// 树木系统批次（2026-09-01）新增的砍伐产出物品第一版就叫
/// `lostland:timber`，与 `mods/lostland/resources.json5` 里早已存在的
/// 据点资源「木材」逐字相同（见 [`crate::tree`] 的 `TIMBER_ID` 文档）：
///
/// - **json5 schema 校验**只看单个文件内部的形状，看不见另一张表。
/// - **本模块的跨表引用完整性**（[`crate::content_audit::ContentAuditReport::reference_integrity`]）
///   查的是「引用指向的东西存不存在」——撞名恰恰让它**更容易**通过：
///   那个 id 确实存在，而且存在两次。
/// - **「本体清单不多不少」的名册测试**按表各查各的，两张表各自都
///   「不多不少」。
///
/// 当时真正抓到它的只是一个对不上的数字（新增两条内容、后续
/// `ContentIndex` 却只平移了 1）。这正是
/// [ADR 0022](../../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)
/// 说的形状：判据看着齐全，覆盖面却漏掉了整整一类错误，于是「测试全绿」
/// 不再等价于「内容真的自洽」。
///
/// # 撞名顺手弄坏内容哈希（不只是「名字难看」）
///
/// [`crate::content_hash::classify_index`] 取 [`tables_defining`] 的
/// **第一项**。撞名之后，那条内容只按第一张表的 `write_*_fields` 求
/// 摘要——第二张表里那份字段值从此完全不进内容哈希，
/// [ADR 0027](../../../../knowledge/decisions/0027-content-hash-covers-field-values.md)
/// 要堵的「改了字段值却哈希不变」缺口被一次撞名重新打开。本模块的字段
/// 覆盖审计走的是同一个 [`crate::content_hash::classify_index`] 分派，同样对第二张表失明。
///
/// # 返回顺序
///
/// 按 [`Registry::snapshot`](crate::registry::Registry::snapshot) 的顺序
/// （即注册顺序，确定性见 [`crate::content_data`] 模块文档「顺序确定性」
/// 一节），不依赖任何 `HashMap` 迭代顺序（约束 C5）。
pub fn detect_table_define_collisions(
    registry: &Registry,
    tables: &ContentValueTables<'_>,
) -> TableExclusivityReport {
    let mut collisions = Vec::new();
    for id in registry.snapshot() {
        let Some(index) = registry.get(&id) else {
            // 理论不可达：`id` 刚从同一个 `registry` 的快照里取出。
            // 与 `audit_content`/`content_hash::apply_value_hashes`
            // 同一条防御立场。
            continue;
        };
        let defining = tables_defining(index, tables);
        if defining.len() > 1 {
            collisions.push(TableDefineCollision {
                id,
                index,
                tables: defining,
            });
        }
    }
    TableExclusivityReport {
        collisions,
        indices_checked: registry.snapshot().len(),
    }
}

/// 一次跨表撞名普查的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableExclusivityReport {
    /// 全部撞名，按注册顺序。空 `Vec` 就是通过。
    pub collisions: Vec<TableDefineCollision>,
    /// 本次普查实际看过多少个 `ContentIndex`。
    ///
    /// # 这个计数是干什么用的：防「空转通过」
    ///
    /// 一个索引都没看过的撞名普查会**恒为绿**，而且绿得完全无声——谁
    /// 把下面那个循环接到一个空注册表上、或者把
    /// [`ContentValueTables`] 换成一份全空的表，结果仍然是「零撞名」。
    /// 这正是本仓库反复吃亏的那类静默缺口
    /// （[ADR 0022](../../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)）。
    /// 把「看过多少个」如实带出来，门禁测试就能断言真实内容确实喂了
    /// 这条检查非零的量，而不是只断言它没报错。
    pub indices_checked: usize,
}

impl TableExclusivityReport {
    /// 生产装载路径的**硬失败**判定。
    ///
    /// # 为什么撞名阻断启动
    ///
    /// 与 `ll_mod::content_audit` 里引用完整性、副职获得条件可达性那两
    /// 条同一档，判据是同一条：**有没有一种合法的内容配置会触发它**。
    /// 撞名没有合法版本——不存在任何一份内容设计，其意图是「让这条资源
    /// 和这件物品共用一个索引」：共用不带来任何能力（两张表各查各的，
    /// 从来不会因为索引相同而互通），只带来两样损失——身份问不清楚，
    /// 以及 [`detect_table_define_collisions`] 文档「撞名顺手弄坏内容
    /// 哈希」一节那半张表的字段值永久失明。
    ///
    /// 另外两条性质也与引用完整性完全同构：本检查**对全部已装载内容
    /// 一视同仁**（第三方 mod 撞上本体的 id 与本体自己撞上自己同样要
    /// 报），而且它的运行期症状是**静默的**——没有任何日志、没有任何
    /// 报错，只有一份悄悄少了一半覆盖面的内容哈希。
    pub fn result(&self) -> Result<(), TableDefineCollisionError> {
        if self.collisions.is_empty() {
            return Ok(());
        }
        Err(TableDefineCollisionError {
            collisions: self.collisions.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier_type::ModifierTypeDef;
    use crate::tag::TagDef;
    use crate::test_support::OwnedTables;
    use ll_sim::item::WearChannels;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    #[test]
    fn 同一个索引被两张表定义时撞名检查报出两张表() {
        // 撞名判据的**正面**证据：本仓库既有的三条判据（json5 schema、
        // 跨表引用完整性、「清单不多不少」的名册测试）对下面这份内容
        // 全部是绿的——它没有任何悬空引用，两张表各自也都「不多不少」。
        // 见 [`detect_table_define_collisions`] 文档「三条既有判据为什么
        // 一条都拦不住它」一节。
        // Arrange：一个 id，两张表各 define 一次。
        let mut registry = Registry::new();
        let mut tables = OwnedTables::default();
        let index = registry.intern(id("test:collide"));
        tables
            .tag
            .define(
                index,
                TagDef {
                    wear: WearChannels::NONE,
                },
            )
            .expect("测试用标签定义内部自洽");
        tables
            .modifier_type
            .define(index, ModifierTypeDef {})
            .expect("测试用加值类型定义内部自洽");

        // Act
        let report = detect_table_define_collisions(&registry, &tables.as_value_tables());

        // Assert：报出来，且两张表**都**被点名（不是只报第一张）。
        assert_eq!(
            report.collisions,
            vec![TableDefineCollision {
                id: id("test:collide"),
                index,
                tables: vec![ContentTableKind::Tag, ContentTableKind::ModifierType],
            }]
        );
    }

    #[test]
    fn 每个索引各归一张表时撞名检查通过且不是空转() {
        // 与上一条配对的**反面**证据：同样两条内容、同样两张表，只是
        // 各用各的 id，就必须是绿的。少了这一条，上一条无法区分「判据
        // 真的在看表」与「判据恒报红」。
        // Arrange
        let mut registry = Registry::new();
        let mut tables = OwnedTables::default();
        let tag_index = registry.intern(id("test:a_tag"));
        tables
            .tag
            .define(
                tag_index,
                TagDef {
                    wear: WearChannels::NONE,
                },
            )
            .expect("测试用标签定义内部自洽");
        let modifier_index = registry.intern(id("test:a_modifier"));
        tables
            .modifier_type
            .define(modifier_index, ModifierTypeDef {})
            .expect("测试用加值类型定义内部自洽");

        // Act
        let value_tables = tables.as_value_tables();
        let report = detect_table_define_collisions(&registry, &value_tables);

        // Assert
        assert_eq!(report.collisions, Vec::new());
        assert!(report.result().is_ok());
        assert_eq!(report.indices_checked, 2);
        // 非空转：两个索引真的各被一张表认领了，不是两张表都空、
        // `tables_defining` 对谁都返回空 `Vec` 那种「零撞名」。
        assert_eq!(
            tables_defining(tag_index, &value_tables),
            vec![ContentTableKind::Tag]
        );
        assert_eq!(
            tables_defining(modifier_index, &value_tables),
            vec![ContentTableKind::ModifierType]
        );
    }

    #[test]
    fn 撞名错误文案点名两张表并给出改名与删定义两条出路() {
        // 判据报红时说的话必须是可行动的——否则内容作者只知道「坏了」，
        // 不知道坏在哪、也不知道怎么修，与 `content_audit` 模块「错误
        // 呈现」一节对引用完整性的要求同一条标准。
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(id("lostland:timber"));
        let error = TableDefineCollisionError {
            collisions: vec![TableDefineCollision {
                id: id("lostland:timber"),
                index,
                tables: vec![ContentTableKind::Item, ContentTableKind::Resource],
            }],
        };

        // Act
        let text = error.to_string();

        // Assert
        assert!(text.contains("lostland:timber"), "{text}");
        assert!(text.contains("物品表"), "{text}");
        assert!(text.contains("资源表"), "{text}");
        assert!(text.contains("改名"), "{text}");
        assert!(text.contains("删掉其中一处 `define`"), "{text}");
    }
}
