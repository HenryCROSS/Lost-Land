//! 本体默认伤害公式注册——`knowledge/design/damage-formula-mod-api.md`
//! 十九节「没有声明任何分项时……解析必然落到第四层：全局默认」的落地，
//! 退化到本批次的两层模型时就是「全局默认」这唯一一层。
//!
//! # 为什么走与 [`crate::base_xp_curve`] 完全相同的模式
//!
//! 与「本体即 Mod」既有先例同一条纪律：本体默认公式与 mod 通过
//! `register-damage-formula` 声明的公式共用**完全相同**的
//! [`crate::formula::FormulaTable::define`] 调用，没有任何本体专属的
//! 特权通道——本模块只是把这次调用挪到 Rust 侧直接执行（本体注册向来
//! 不经过脚本管线，见 `crate::pipeline` 模块文档「本体内容不经过这条
//! 管线」一节），不是发明一条新的注册路径。
//!
//! # 为什么默认公式是 `(quote attack-power)`
//!
//! 任务硬要求二「全局默认公式必须逐行复现现在的行为」——`resolve_attack`
//! 接入公式引擎之前，`attack_power = attacker_derived.attribute(AttributeKind::Strength)`
//! 是唯一的攻击力来源，公式引擎接入后这个值原样作为
//! [`ll_sim::formula::FormulaOperand::AttackPower`] 输入喂给公式，全局
//! 默认公式因此只需要把这个输入原样交回去
//! （[`ll_sim::formula::default_attack_power_instructions`]：单条
//! `Ref(AttackPower)` 指令），不做任何变换——这是唯一能保证「接入
//! 引擎前后没有任何 mod 指定公式时，伤害数值逐位相同」的公式，见
//! `crate::formula` 模块文档与 `ll_sim::resolve` 模块「行为等价」
//! 测试。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::formula::{FormulaDef, default_attack_power_instructions};

use crate::formula::{FormulaError, FormulaTable};

/// 本体默认伤害公式的完整命名空间标识符。
pub const DEFAULT_DAMAGE_FORMULA_ID: &str = "lostland:default_damage_formula";

/// 本体默认公式注册的唯一入口：`intern` 是外部传入的解析回调，理由同
/// [`crate::base_xp_curve::register_base_xp_curve`] 文档。
pub fn register_base_damage_formula(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(ContentIndex, FormulaTable), FormulaError> {
    let mut table = FormulaTable::new();
    let index = intern(
        NamespacedId::parse(DEFAULT_DAMAGE_FORMULA_ID).expect("本体默认公式 id 字面量恒合法"),
    );
    table.define(
        index,
        FormulaDef {
            id: index,
            instructions: default_attack_power_instructions(),
            needs_rng: false,
        },
    )?;
    Ok((index, table))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use ll_sim::formula::{FormulaInputs, eval_formula};

    #[test]
    fn 本体默认公式注册成功且原样返回攻击力输入() {
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (index, table) = register_base_damage_formula(&mut |id| registry.intern(id))
            .expect("本体默认公式注册恒不失败");

        // Assert
        let def = table.get(index).expect("刚注册的公式应能查到定义");
        let mut rng = ll_core::rng::DetRng::for_entity(1, 2, 3);
        let inputs = FormulaInputs::new(88, 0, 0, 0, [0; 7], false);
        assert_eq!(eval_formula(def, &inputs, &mut rng), 88);
    }
}
