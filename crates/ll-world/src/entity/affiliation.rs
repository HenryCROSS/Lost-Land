//! 归属：势力 / 宗教 / 行会 / 文化 / 家族共用的一个结构。
//!
//! 完整语义冻结在 `knowledge/design/society-and-affiliation.md` 第一
//! 节：这几类归属向游戏逻辑提出的问题完全相同——「这个实体属于谁」
//! 「关系好到什么程度」——分成多套并行结构就要写多遍偷窃判定、多遍
//! 交易折扣、多遍行为树条件。实现阶段是 P8，本任务只建 P3 建
//! [`crate::entity::Agent`] 时必须已经存在的字段布局，理由同
//! [`crate::entity::Goal`]：归属字段必须在 P3 建实体时就预留，否则 P8
//! 落地要改遍所有实体与存档。
//!
//! # `Profession` 这个变体已经删掉了（文化批次）
//!
//! 项目所有者的裁定：
//!
//! > 「`Agent.profession` 是职业的唯一真相源。`AffiliationKind::
//! > Profession` 改名为 `Guild`。一个铁匠可以不加入铁匠行会——从属
//! > 关系表达的是行会成员身份，不是职业本身。」
//!
//! **裁定的意图照办，但「改名为 `Guild`」这一步在代码里做不到：
//! [`AffiliationKind::Guild`] 本来就已经存在**（六个变体里的第三个，
//! 一字未改地存在于本文件自 P3 起的每一个版本里）。把 `Profession`
//! 改名成 `Guild` 会撞出两个同名变体。
//!
//! 按裁定的**意图**，正确动作是**删掉 `Profession`**：所有者要的
//! 「从属关系表达的是行会成员身份」这件事，已经由现成的 `Guild` 变体
//! 表达着；而「这个人干哪一行」由 `Agent.profession: ContentIndex`
//! 单列表达。两者从此不重叠，`settlements-structures-and-npc-spawning.md`
//! 十二节 1 挂了很久的那条「职业到底有几个真相源」就此结案：**一个**。
//!
//! 删掉是安全的，不是一次存档迁移：`Profession` 这个变体**从未被构造
//! 过一次**（全仓库零构造点），而在删掉它的那一刻，生产路径上两条
//! `Agent` 构造路径的 `affiliations` 都还写死 `Vec::new()`，因此没有
//! 任何存档里可能出现这个变体名。
//!
//! # `affiliations` 现在有生产者了（文化归属与敌对判定批次）
//!
//! 上一段那句「两条构造路径都写死 `Vec::new()`」**在本批次之后只剩
//! 一半成立**，如实更新而不是留着：
//!
//! - NPC 那一条（`ll_mod::roster::build_npc_agent`）现在会挂一条
//!   [`AffiliationKind::Culture`] 归属，指向所属据点的文化。这是本
//!   字段落地以来的**第一个生产者**。
//! - 玩家那一条（`ll_game::world::build_player_agent`）仍然写死
//!   `Vec::new()`，且这是**裁定**不是遗漏：项目所有者「玩家可以没有
//!   势力归属，这个可以通过后面和据点的管理者对话加入」。玩家「没有
//!   文化」这件事由判定期回退表达（`ll_sim::ai_query::declared_hostile`
//!   回退到 `lostland:cultureless` 哨兵），不写进实体。
//!
//! # `Faction` 现在有东西可指了（势力播种批次，2026-08-29）
//!
//! 上一段那句「[`AffiliationKind::Faction`] 仍然零生产者——势力播种是
//! 另一批」**只剩一半成立**，如实更新：那一批已经落地。
//! [`crate::faction::FactionTable`] 住在
//! [`crate::state::WorldState::factions`] 里，每座活着的据点恰好归一个
//! 势力，[`crate::faction::FactionTable::faction_of`] 把据点号翻译成
//! 势力号。
//!
//! **但 `Faction` 这一类归属本身仍然零生产者**——势力播种只让势力
//! **存在**，「玩家怎么加入」是对话批次的事（`dialogue-system.md`
//! 5.1 节，那一节的「拿据点 `WorldId` 冒充」变通已随本批作废）。
//! 区别在于：此前是「没有任何东西可以加入」，现在是「有东西可以加入、
//! 只是还没有人写那句加入」。

use ll_core::ident::{ContentIndex, WorldId};

/// 归属类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AffiliationKind {
    /// 势力：领土、法律、税收、兵役。世界生成期间造出来的实例，走
    /// [`OrgRef::Instance`]。
    Faction,
    /// 宗教：戒律、神祝、什一税。世界生成期间造出来的实例，走
    /// [`OrgRef::Instance`]。
    Religion,
    /// 行会：技艺、订单、垄断、会费。世界生成期间造出来的实例，走
    /// [`OrgRef::Instance`]。
    Guild,
    /// 文化：与生俱来，不索取任何东西，只塑造偏好。mod 装载时确定的
    /// 类型，走 [`OrgRef::Def`]。
    ///
    /// 这一类现在有了真正的内容表（[`crate::culture::CultureTable`]）、
    /// 真正的据点级生产者（[`crate::settlement::SettlementSite::culture`]），
    /// **以及个体级的生产者**（文化归属与敌对判定批次）：
    /// `ll_mod::roster::build_npc_agent` 给每个物化出来的 NPC 挂一条
    /// 指向所属据点文化的本类归属。这一条同时是整个
    /// [`Affiliation`] 字段的第一个生产者，见本模块文档
    /// 「`affiliations` 现在有生产者了」一节。
    ///
    /// 消费者是 `ll_sim::ai_query::declared_hostile`：撞格路由靠它把
    /// 「走进对方那一格」判成攻击还是互换。
    Culture,
    /// 家族：血缘与姻亲。与生俱来，可因联姻扩展。世界生成期间造出来
    /// 的实例，走 [`OrgRef::Instance`]。
    Family,
}

/// 归属指向的具体组织——类型还是实例，由 [`AffiliationKind`] 决定
/// （`Culture` 恒为 `Def`，其余恒为 `Instance`）。
///
/// 判据冻结在 `knowledge/design/identity-and-ids.md` 二：**mod 定义
/// 「种类」，世界生成造「个体」。** 用枚举而不是给 [`Affiliation`] 拆
/// 两个并列字段，是因为 `kind` 本身已经决定了 `org` 该是哪一种——枚举
/// 把这条「由 kind 决定 org 具体类型」的约束显式表达出来，调用方
/// `match` 一次就能拿到正确类型，不需要「看 kind 再决定该读哪个字段」
/// 这种隐式约定。这是本任务自己的实现判断，原文档只裁定了字段该改成
/// 什么形状的方向，未裁定具体到枚举还是双字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OrgRef {
    /// 类型：文化——mod 装载时确定，集合封闭、数量小，走
    /// [`ContentIndex`]。（`Profession` 曾经也走这一支，随文化批次
    /// 删掉，见 [`AffiliationKind`] 文档。）
    Def(ContentIndex),
    /// 实例：势力、宗教、行会、家族——世界生成期间造出来的具体个体，
    /// 数量随世界规模增长，走 [`WorldId`]。
    Instance(WorldId),
}

/// 一个实体对某个组织的归属与声望。
///
/// `org` 现在可以直接派生 `serde`（P5 批次 B）：[`OrgRef::Def`] 里的
/// [`ContentIndex`] 与 [`OrgRef::Instance`] 里的 [`WorldId`] 都已各自
/// 补齐无上下文的直接 `Serialize`/`Deserialize`（见
/// [`crate::entity::Agent`] 模块文档「可派生 `serde`」一节的完整论证）
/// ——`ContentIndex` 反序列化只做结构转换，「是否已注册」的解析留给
/// 拿到注册表之后的调用方，不塞进这里的派生。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Affiliation {
    /// 归属类别。
    pub kind: AffiliationKind,
    /// 具体组织：类型还是实例由 `kind` 决定，见 [`OrgRef`]。
    pub org: OrgRef,
    /// 声望，千分比。负值表示敌对。
    pub standing: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    #[test]
    fn culture归属的org是def变体() {
        // 文化是 mod 装载时确定的类型，集合封闭，走 ContentIndex。
        // Arrange
        let mut interner = Interner::new();
        let culture_id = interner.intern(NamespacedId::parse("lostland:mountain").expect("合法"));

        // Act
        let affiliation = Affiliation {
            kind: AffiliationKind::Culture,
            org: OrgRef::Def(culture_id),
            standing: 0,
        };

        // Assert
        assert!(matches!(affiliation.org, OrgRef::Def(_)));
    }

    #[test]
    fn faction归属的org是instance变体() {
        // 势力是世界生成期间造出来的具体个体，数量随世界规模增长，
        // 走 WorldId。
        // Arrange
        let mut counter = 0u32;
        let faction_id = WorldId::next(&mut counter);

        // Act
        let affiliation = Affiliation {
            kind: AffiliationKind::Faction,
            org: OrgRef::Instance(faction_id),
            standing: 500,
        };

        // Assert
        assert!(matches!(affiliation.org, OrgRef::Instance(_)));
    }
}
