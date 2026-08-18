//! 归属：势力 / 宗教 / 行会 / 文化 / 家族 / 职业共用的一个结构。
//!
//! 完整语义冻结在 `knowledge/design/society-and-affiliation.md` 第一
//! 节：这六类归属向游戏逻辑提出的问题完全相同——「这个实体属于谁」
//! 「关系好到什么程度」——分成多套并行结构就要写多遍偷窃判定、多遍
//! 交易折扣、多遍行为树条件。实现阶段是 P8，本任务只建 P3 建
//! [`crate::entity::Agent`] 时必须已经存在的字段布局，理由同
//! [`crate::entity::Goal`]：归属字段必须在 P3 建实体时就预留，否则 P8
//! 落地要改遍所有实体与存档。

use ll_core::ident::ContentIndex;

/// 归属类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffiliationKind {
    /// 势力：领土、法律、税收、兵役。
    Faction,
    /// 宗教：戒律、神祝、什一税。
    Religion,
    /// 行会：技艺、订单、垄断、会费。
    Guild,
    /// 文化：与生俱来，不索取任何东西，只塑造偏好。
    Culture,
    /// 家族：血缘与姻亲。与生俱来，可因联姻扩展。
    Family,
    /// 职业：你实际在干什么。`standing` 在此表示熟练度 / 资历。
    Profession,
}

/// 一个实体对某个组织的归属与声望。
///
/// `org` 不派生 `serde`，理由同 [`crate::entity::Goal::kind`]：
/// [`ContentIndex`] 依赖 mod 加载顺序，`ll_core::ident` 模块文档明确
/// 写着不可持久化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Affiliation {
    /// 归属类别。
    pub kind: AffiliationKind,
    /// 具体组织，指向注册表。
    pub org: ContentIndex,
    /// 声望，千分比。负值表示敌对。
    pub standing: i32,
}
