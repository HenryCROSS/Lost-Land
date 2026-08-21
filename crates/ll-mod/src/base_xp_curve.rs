//! 本体默认经验曲线注册——`knowledge/design/level-and-experience-system.md`
//! 八节「未绑定的职业/种族退回 `lostland:default_xp_curve`」的落地。
//!
//! # 为什么走与 [`crate::base_race`] 完全相同的模式
//!
//! 与「本体即 Mod」既有先例同一条纪律：本体默认曲线与 mod 通过
//! `register-xp-curve` 声明的曲线共用**完全相同**的
//! [`crate::xp_curve::XpCurveTable::define`] 调用，没有任何本体专属的
//! 特权通道——本模块只是把这次调用挪到 Rust 侧直接执行（本体注册向来
//! 不经过脚本管线，见 `crate::pipeline` 模块文档「本体内容不经过这条
//! 管线」一节），不是发明一条新的注册路径。
//!
//! # 为什么默认曲线选「平曲线」
//!
//! 与 [`ll_sim::xp_curve::FlatXpCurve`] 同一个形状（每级固定同一笔
//! 经验）——设计文档八节明确「具体职业/种族的曲线数值……本文档不代为
//! 选定」,本体默认曲线因此只需要「存在且能跑」，不需要是一个经过
//! 平衡设计的成长曲线（那是内容设计批次的职责，不是本次「系统骨架
//! 能否端到端跑通」批次的职责）。数值选用
//! [`ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL`] 同一个量级
//! （100），与新角色出生时的占位门槛保持一致,不是巧合选取的数字。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::xp_curve::{XpCurveDef, XpCurveOp, XpCurveOperand};

use crate::xp_curve::{XpCurveError, XpCurveTable};

/// 本体默认经验曲线的完整命名空间标识符。
pub const DEFAULT_XP_CURVE_ID: &str = "lostland:default_xp_curve";

/// 本体默认曲线注册的唯一入口：`intern` 是外部传入的解析回调，理由同
/// [`crate::race::materialize_base_races`] 文档。
pub fn register_base_xp_curve(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(ContentIndex, XpCurveTable), XpCurveError> {
    let mut table = XpCurveTable::new();
    let index =
        intern(NamespacedId::parse(DEFAULT_XP_CURVE_ID).expect("本体默认曲线 id 字面量恒合法"));
    let seed = ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL;
    table.define(
        index,
        XpCurveDef {
            id: index,
            base_requirement: seed,
            instructions: vec![XpCurveOp::Ref(XpCurveOperand::Const(seed))],
        },
    )?;
    Ok((index, table))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use ll_sim::xp_curve::eval_xp_curve;

    #[test]
    fn 本体默认曲线注册成功且门槛恒为固定值() {
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (index, table) = register_base_xp_curve(&mut |id| registry.intern(id))
            .expect("本体默认曲线注册恒不失败");

        // Assert
        let curve = table.get(index).expect("刚注册的曲线应能查到定义");
        assert_eq!(eval_xp_curve(curve, 1, curve.base_requirement), 100);
    }
}
