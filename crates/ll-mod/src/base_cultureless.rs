//! 本体「无文化」哨兵内容注册——项目所有者裁定「那文化设定一个独特的
//! 东西叫无文化，这东西颗粒度小到具体某个 NPC」的落点。
//!
//! # 它是一个**只 `intern`、不 `define`** 的文化索引
//!
//! 本模块只往 [`Registry`] 里 `intern` 一个固定标识符，**不**给它调
//! [`ll_world::culture::CultureTable::define`]。这一条是整个「无文化」
//! 设计能这么便宜的原因，逐条说明：
//!
//! - **敌意目标只要求 `intern`**：`crate::content_schema_world::apply_cultures`
//!   处理 `hostility[].culture` 走的是 `intern` 而不是「只 get」（见该
//!   函数文档「敌对目标尤其需要 `intern`」一段），因此一个从未被
//!   `define` 过的索引**照样可以被别的文化声明敌意**——本体的
//!   `mods/lostland/cultures.json5` 正是这么给哥布林部落写上「对无文化
//!   敌意 6」的。
//! - **它永远不会被选为建城文化**：`ll_world::chronicle` 的 `pick_culture`
//!   只遍历 [`ll_world::culture::CultureTable::registered`]，而后者返回的
//!   `order` 只装 `define` 成功过的索引。因此世界生成的权重与掷骰序列
//!   一个字节都不受影响，`pick_culture` 一个字都不用改。
//! - **不必松动 `define` 的三条注册期校验**：尤其「至少要有一个权重为正
//!   的建立者种族」那一条——「无文化」按定义没有建立者种族，真去
//!   `define` 它反而要把那条校验挖个洞。
//! - **不必递增 `CONTENT_HASH_ALGORITHM_VERSION`**：内容表一个字段都没
//!   加，改的只是内容取值，见 `crate::content_hash` 那段区分「量尺」与
//!   「取值」的注释。
//!
//! # 为什么走与 [`crate::base_placeholder`] 完全相同的三件套
//!
//! 形状上「无文化」与 `lostland:placeholder_race` 完全同类：都是一条
//! **不落在任何一张内容表里**的固定本体标识符，都靠 `Registry::intern`
//! 这条与 mod 内容毫无二致的通道注册，因此这里逐字沿用那份「常量 +
//! 注册函数 + 查询函数」的写法，不另发明一套。
//!
//! # 这不是在重犯 `SETTLEMENT_RACE_IDS` 那个错误
//!
//! `knowledge/design/race-system.md` 批评硬编码 `SETTLEMENT_RACE_IDS`，
//! 理由是它**把 mod 内容排除在机制之外**——第三方种族拿不到任何选址
//! 亲和，机制对本体清单之外的内容视而不见。而一个「缺席」哨兵索引
//! 不排除任何人：mod 照样可以在自己的 `cultures.json5` 里声明对
//! `lostland:cultureless` 的敌意，也照样可以让自己的 NPC 无文化（不挂
//! 文化归属即可，见 `ll_sim::ai_query::declared_hostile`）。它扩大机制
//! 的适用面，不缩小。

use ll_core::ident::{ContentIndex, NamespacedId};

use crate::registry::Registry;

/// 本体「无文化」哨兵的固定标识符。
///
/// 命名空间刻意用 `lostland:`（本体）而不是某个引擎前缀：ADR 0015/0016
/// 「本体即 Mod」——这条内容与任何 mod 内容走同一条注册通道，没有特权
/// 号段，也没有特权命名空间。
pub const CULTURELESS_CULTURE_ID: &str = "lostland:cultureless";

/// 把本体「无文化」哨兵注册进 `registry`，返回它拿到的 [`ContentIndex`]。
///
/// # 调用时机：必须在**全部 mod 装载完成之后**
///
/// [`Registry::intern`] 按调用顺序分配 [`ContentIndex`]，插在中间会
/// 平移它之后每一条内容的索引。因此本函数的生产调用点是
/// [`crate::load_session::LoadSession::load_all`] 的**末尾**，不是
/// [`crate::load_session::LoadSession::with_engine_registrations`]
/// 那一串引擎侧注册里——后者跑在 mod 装载之前，从那里注册会把全部 mod
/// 内容的索引整体后移一位。
///
/// 本函数是幂等的（[`Registry::intern`] 自身的幂等性）：本体的
/// `mods/lostland/cultures.json5` 在声明哥布林对无文化的敌意时已经
/// `intern` 过同一个标识符，这里再调一次拿到的是**同一个**索引，不会
/// 产生第二条记录。
pub fn register_base_cultureless_culture(registry: &mut Registry) -> ContentIndex {
    let id = NamespacedId::parse(CULTURELESS_CULTURE_ID).expect("固定字面量标识符恒合法");
    registry.intern(id)
}

/// 查询 `registry` 是否已经注册过「无文化」哨兵，返回其索引。
///
/// 与 [`register_base_cultureless_culture`] 的区别同
/// [`crate::base_placeholder::base_placeholder_index`]：本函数只
/// **解析**（[`Registry::get`]，不创建新记录），供只持有 `&Registry`
/// 的调用方使用。
///
/// 从未注册过时返回 `None`——不是错误：那样的会话里「无文化」这条判据
/// 整个不生效，[`ll_sim::ai_query::declared_hostile`] 退回文化落地之前
/// 的势力判据，这是 ADR 0015「尚无内容」的诚实表达，不伪造索引。
pub fn base_cultureless_culture_index(registry: &Registry) -> Option<ContentIndex> {
    let id = NamespacedId::parse(CULTURELESS_CULTURE_ID).expect("固定字面量标识符恒合法");
    registry.get(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 无文化哨兵与mod内容共用registry同一段连续递增的索引号段() {
        // 与 `crate::base_placeholder` 那条同型：证明「无文化」走的是同
        // 一条 Registry::intern 通道，没有为它预留任何特殊区间。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let cultureless = register_base_cultureless_culture(&mut registry);
        let mod_index = registry.intern(NamespacedId::parse("yourmod:nomad").expect("合法标识符"));

        // Assert
        assert_eq!(mod_index.get(), cultureless.get() + 1);
    }

    #[test]
    fn 无文化哨兵重复注册返回相同索引() {
        // 幂等性是生产路径真正依赖的性质：cultures.json5 的敌意目标已经
        // intern 过一次，load_all 末尾还会再调一次。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let first = register_base_cultureless_culture(&mut registry);
        let second = register_base_cultureless_culture(&mut registry);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 未注册无文化哨兵的registry查询返回none而非伪造索引() {
        // Arrange
        let registry = Registry::new();

        // Act
        let looked_up = base_cultureless_culture_index(&registry);

        // Assert
        assert_eq!(looked_up, None);
    }

    #[test]
    fn 先由内容文件intern过的无文化哨兵再注册仍是同一个索引() {
        // 这条钉住生产路径的真实顺序：cultures.json5 的
        // `hostility[].culture` 先 intern，load_all 末尾的注册后到。
        // Arrange
        let mut registry = Registry::new();
        let from_content =
            registry.intern(NamespacedId::parse(CULTURELESS_CULTURE_ID).expect("合法标识符"));

        // Act
        let registered = register_base_cultureless_culture(&mut registry);

        // Assert
        assert_eq!(registered, from_content);
    }
}
