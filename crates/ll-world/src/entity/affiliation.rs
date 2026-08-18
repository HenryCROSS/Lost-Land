//! 归属：势力 / 宗教 / 行会 / 文化 / 家族 / 职业共用的一个结构。
//!
//! 完整语义冻结在 `knowledge/design/society-and-affiliation.md` 第一
//! 节：这六类归属向游戏逻辑提出的问题完全相同——「这个实体属于谁」
//! 「关系好到什么程度」——分成多套并行结构就要写多遍偷窃判定、多遍
//! 交易折扣、多遍行为树条件。实现阶段是 P8，本任务只建 P3 建
//! [`crate::entity::Agent`] 时必须已经存在的字段布局，理由同
//! [`crate::entity::Goal`]：归属字段必须在 P3 建实体时就预留，否则 P8
//! 落地要改遍所有实体与存档。

use ll_core::ident::{ContentIndex, WorldId};

/// 归属类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Culture,
    /// 家族：血缘与姻亲。与生俱来，可因联姻扩展。世界生成期间造出来
    /// 的实例，走 [`OrgRef::Instance`]。
    Family,
    /// 职业：你实际在干什么。`standing` 在此表示熟练度 / 资历。mod
    /// 装载时确定的类型，走 [`OrgRef::Def`]。
    Profession,
}

/// 归属指向的具体组织——类型还是实例，由 [`AffiliationKind`] 决定
/// （`Culture`/`Profession` 恒为 `Def`，其余恒为 `Instance`）。
///
/// 判据冻结在 `knowledge/design/identity-and-ids.md` 二：**mod 定义
/// 「种类」，世界生成造「个体」。** 用枚举而不是给 [`Affiliation`] 拆
/// 两个并列字段，是因为 `kind` 本身已经决定了 `org` 该是哪一种——枚举
/// 把这条「由 kind 决定 org 具体类型」的约束显式表达出来，调用方
/// `match` 一次就能拿到正确类型，不需要「看 kind 再决定该读哪个字段」
/// 这种隐式约定。这是本任务自己的实现判断，原文档只裁定了字段该改成
/// 什么形状的方向，未裁定具体到枚举还是双字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrgRef {
    /// 类型：文化、职业——mod 装载时确定，集合封闭、数量小，走
    /// [`ContentIndex`]。
    Def(ContentIndex),
    /// 实例：势力、宗教、行会、家族——世界生成期间造出来的具体个体，
    /// 数量随世界规模增长，走 [`WorldId`]。
    Instance(WorldId),
}

/// 一个实体对某个组织的归属与声望。
///
/// `org` 不派生 `serde`：[`OrgRef::Def`] 里的 [`ContentIndex`] 依赖
/// mod 加载顺序，`ll_core::ident` 模块文档明确写着不可持久化，理由同
/// [`crate::entity::Goal::kind`]。[`OrgRef::Instance`] 里的 [`WorldId`]
/// 本身是可持久化的（永不复用、构造不依赖加载顺序），但只要 `OrgRef`
/// 整体还兼容 `Def` 分支，就不能绕过前者直接对整个枚举派生 `serde`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
