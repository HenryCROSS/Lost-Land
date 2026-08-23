//! 内容数据文件里的**算术表达式**——伤害公式与经验曲线共用的那一份
//! s-表达式，换成 JSON5 能原样表达的嵌套数组。
//!
//! # 为什么是嵌套数组，不是一串字符串
//!
//! 脚本时代这两条内容的表达式参数是 `(quote (+ attack-power str-mod))`
//! ——一份被 `quote` 包住、从未被 Steel 求值的 `SteelVal`，由
//! `crate::script_damage_formula_api`／`crate::script_xp_curve_api` 各自
//! 编译成扁平指令数组。搬进数据文件时有两种表达法：
//!
//! 1. **字符串**：`"(+ attack-power str-mod)"`，需要在引擎侧手写一个
//!    s-表达式词法分析器；
//! 2. **嵌套数组**：`["+", "attack-power", "str-mod"]`，`serde` 直接
//!    产出，一行分析器都不用写。
//!
//! 取第二种。JSON5 本身已经区分了三种字面量，恰好一一对应
//! s-表达式的三种叶子/枝节点：
//!
//! | JSON5 | s-表达式 | [`RawExpr`] |
//! |---|---|---|
//! | 数字 `40` | 字面整数 | [`RawExpr::Int`] |
//! | 字符串 `"level"` | 裸符号 | [`RawExpr::Symbol`] |
//! | 数组 `["+", a, b]` | 调用 | [`RawExpr::Call`] |
//!
//! 少写一个分析器就少一类分析器专属的缺陷（未闭合括号、转义、注释
//! 嵌套……），而 JSON5 那一层的语法错误本来就带行列位置。
//!
//! # 两个编译器逐字照搬，不合并
//!
//! [`compile_damage_formula`] 与 [`compile_xp_curve`] 分别对应
//! `crate::script_damage_formula_api`／`crate::script_xp_curve_api` 里
//! 那两份编译器，**逐条对齐、不合并成一份泛型实现**：两者的指令类型
//! （[`FormulaOp`] vs [`XpCurveOp`]）、操作数类型、符号表、以及「有没有
//! 骰子」都不同——伤害公式认 `d`／`attack-power` 一族并有指令数上限，
//! 经验曲线只认 `level`／`prev-requirement` 且没有骰子。硬合并需要一层
//! 只为对称而存在的抽象，正是 ADR 0021 点名要避免的那种。
//!
//! 这也正是内容值哈希逐位不变的前提：编译产出的指令数组必须与脚本
//! 时代**同一个字节**，任何"顺手统一一下"都可能让某条指令的形状漂移。

use ll_sim::formula::{
    DICE_COUNT_RANGE, DICE_SIDES_RANGE, FormulaCond, FormulaOp, FormulaOperand,
    MAX_FORMULA_INSTRUCTIONS,
};
use ll_sim::xp_curve::{XpCurveCond, XpCurveOp, XpCurveOperand};
use ll_world::entity::AttributeKind;
use serde::Deserialize;

/// 数据文件里的一个算术表达式节点。
///
/// `#[serde(untagged)]`：靠 JSON5 字面量本身的类型区分三个变体，作者
/// 不需要写 `{ kind: "call", ... }` 这类样板。这是本 crate 里唯一一处
/// 用 untagged 的地方——它成立是因为三个变体的 JSON 表示天然不重叠
/// （数字/字符串/数组），不是"能省则省"。
///
/// untagged 变体上无法施加 `deny_unknown_fields`（那是结构体字段的
/// 概念，这里根本没有具名字段）。校验由编译器接管，且**是封闭表**：
/// 不认识的符号与算子一律报错，见 [`compile_damage_formula`]。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum RawExpr {
    /// 字面整数常量。
    Int(i64),
    /// 裸符号——`"level"`／`"attack-power"` 这类，取值由各自的封闭表
    /// 决定。
    Symbol(String),
    /// 一次调用：数组第一项是算子符号，其余是参数。
    Call(Vec<RawExpr>),
}

impl RawExpr {
    /// 取出「这是一次调用」时的算子符号与参数，否则 `None`。
    fn as_call(&self) -> Option<(&str, &[RawExpr])> {
        let RawExpr::Call(items) = self else {
            return None;
        };
        let Some(RawExpr::Symbol(op)) = items.first() else {
            return None;
        };
        Some((op.as_str(), &items[1..]))
    }
}

// ─────────────────────────── 伤害公式 ───────────────────────────

/// 封闭的二元算子表——在递归编译任何操作数**之前**先用它确认 `op`
/// 是不是这七个之一，理由同 `crate::script_damage_formula_api::compile_call`
/// 那一段：先递归会让错误信息指向一个与真正问题无关的子符号。
const KNOWN_BINARY_OPS: [&str; 7] = ["+", "-", "*", "/", "mul-permille", "min", "max"];

/// 把一个表达式编译成伤害公式的扁平指令数组——装载期一次性完成，
/// 运行期 [`ll_sim::formula::eval_formula`] 零解释器参与。
pub fn compile_damage_formula(expr: &RawExpr) -> Result<Vec<FormulaOp>, String> {
    let mut out = Vec::new();
    let result = formula_operand(expr, &mut out)?;
    if out.is_empty() {
        out.push(FormulaOp::Ref(result));
    }
    if out.len() > MAX_FORMULA_INSTRUCTIONS {
        return Err(format!(
            "伤害公式表达式编译后产生 {} 条指令，超出上限 {MAX_FORMULA_INSTRUCTIONS}",
            out.len()
        ));
    }
    Ok(out)
}

/// 递归编译一个子表达式，返回引用它求值结果的操作数——叶子节点不产生
/// 指令，复合表达式往 `out` 追加恰好一条并返回指向它的 `Local`。
fn formula_operand(expr: &RawExpr, out: &mut Vec<FormulaOp>) -> Result<FormulaOperand, String> {
    match expr {
        RawExpr::Int(n) => Ok(FormulaOperand::Const(*n)),
        RawExpr::Symbol(symbol) => formula_operand_from_symbol(symbol),
        RawExpr::Call(items) => {
            let Some((op, args)) = expr.as_call() else {
                return Err(format!(
                    "伤害公式表达式数组的第一个元素必须是运算符符号，实际是 {:?}",
                    items.first()
                ));
            };
            formula_call(op, args, out)
        }
    }
}

/// 把一个裸符号映射到 [`FormulaOperand`]——封闭表，任何不在表里的符号
/// 一律拒绝，不静默降级。
fn formula_operand_from_symbol(symbol: &str) -> Result<FormulaOperand, String> {
    match symbol {
        "attack-power" => Ok(FormulaOperand::AttackPower),
        "defense" => Ok(FormulaOperand::Defense),
        "pen-flat" => Ok(FormulaOperand::PenetrationFlat),
        "pen-permille" => Ok(FormulaOperand::PenetrationPermille),
        // wis-mod 映射到 Willpower——本项目没有独立的「感知/意志」二分，
        // 见 ll_sim::formula::FormulaOperand::AttributeModifier 文档。
        "str-mod" => Ok(FormulaOperand::AttributeModifier(AttributeKind::Strength)),
        "dex-mod" => Ok(FormulaOperand::AttributeModifier(AttributeKind::Dexterity)),
        "con-mod" => Ok(FormulaOperand::AttributeModifier(
            AttributeKind::Constitution,
        )),
        "int-mod" => Ok(FormulaOperand::AttributeModifier(
            AttributeKind::Intelligence,
        )),
        "wis-mod" => Ok(FormulaOperand::AttributeModifier(AttributeKind::Willpower)),
        "cha-mod" => Ok(FormulaOperand::AttributeModifier(AttributeKind::Charisma)),
        "crit" => Ok(FormulaOperand::Crit),
        other => Err(format!("伤害公式表达式引用了未知符号 {other:?}")),
    }
}

/// 编译一次算子/骰子调用。
fn formula_call(
    op: &str,
    args: &[RawExpr],
    out: &mut Vec<FormulaOp>,
) -> Result<FormulaOperand, String> {
    if op == "if" {
        let [cond_expr, if_true_expr, if_false_expr] = args else {
            return Err(format!(
                "if 需要恰好三个参数（判据/真分支/假分支），实际 {}",
                args.len()
            ));
        };
        let cond = formula_cond(cond_expr, out)?;
        let if_true = formula_operand(if_true_expr, out)?;
        let if_false = formula_operand(if_false_expr, out)?;
        out.push(FormulaOp::Select {
            cond,
            if_true,
            if_false,
        });
        return Ok(FormulaOperand::Local((out.len() - 1) as u8));
    }

    if op == "d" {
        let [count_expr, sides_expr] = args else {
            return Err(format!(
                "骰子算子 d 需要恰好两个参数（个数/面数），实际 {}",
                args.len()
            ));
        };
        let count = expect_int_literal(count_expr, "d 的骰子个数")?;
        let sides = expect_int_literal(sides_expr, "d 的骰子面数")?;
        let count_u32 =
            u32::try_from(count).map_err(|_| format!("骰子个数 {count} 必须是正整数"))?;
        let sides_u32 =
            u32::try_from(sides).map_err(|_| format!("骰子面数 {sides} 必须是正整数"))?;
        if !DICE_COUNT_RANGE.contains(&count_u32) {
            return Err(format!(
                "骰子个数 {count_u32} 超出合法范围 {}..={}",
                DICE_COUNT_RANGE.start(),
                DICE_COUNT_RANGE.end()
            ));
        }
        if !DICE_SIDES_RANGE.contains(&sides_u32) {
            return Err(format!(
                "骰子面数 {sides_u32} 超出合法范围 {}..={}",
                DICE_SIDES_RANGE.start(),
                DICE_SIDES_RANGE.end()
            ));
        }
        out.push(FormulaOp::Dice {
            count: count_u32,
            sides: sides_u32,
        });
        return Ok(FormulaOperand::Local((out.len() - 1) as u8));
    }

    if !KNOWN_BINARY_OPS.contains(&op) {
        return Err(format!("伤害公式表达式引用了未知算子 {op:?}"));
    }
    let [a_expr, b_expr] = args else {
        return Err(format!(
            "算子 {op:?} 需要恰好两个操作数，实际 {}",
            args.len()
        ));
    };
    let a = formula_operand(a_expr, out)?;
    let b = formula_operand(b_expr, out)?;
    let instruction = match op {
        "+" => FormulaOp::Add(a, b),
        "-" => FormulaOp::Sub(a, b),
        "*" => FormulaOp::Mul(a, b),
        "/" => FormulaOp::Div(a, b),
        "mul-permille" => FormulaOp::MulPermille(a, b),
        "min" => FormulaOp::Min(a, b),
        "max" => FormulaOp::Max(a, b),
        // 上面的 KNOWN_BINARY_OPS 已经穷尽校验过，这个分支理论不可达。
        other => return Err(format!("伤害公式表达式引用了未知算子 {other:?}")),
    };
    out.push(instruction);
    Ok(FormulaOperand::Local((out.len() - 1) as u8))
}

/// 编译 `if` 的判据部分：`[比较符, a, b]`。
fn formula_cond(expr: &RawExpr, out: &mut Vec<FormulaOp>) -> Result<FormulaCond, String> {
    let Some((cmp, args)) = expr.as_call() else {
        return Err("if 的判据必须写成 [比较符, 操作数, 操作数] 形式".to_string());
    };
    let [a_expr, b_expr] = args else {
        return Err(format!(
            "比较符 {cmp:?} 需要恰好两个操作数，实际 {}",
            args.len()
        ));
    };
    let a = formula_operand(a_expr, out)?;
    let b = formula_operand(b_expr, out)?;
    match cmp {
        "<" => Ok(FormulaCond::Lt(a, b)),
        "<=" => Ok(FormulaCond::Le(a, b)),
        ">" => Ok(FormulaCond::Gt(a, b)),
        ">=" => Ok(FormulaCond::Ge(a, b)),
        "=" => Ok(FormulaCond::Eq(a, b)),
        "!=" => Ok(FormulaCond::Ne(a, b)),
        other => Err(format!("未知比较符 {other:?}")),
    }
}

/// 要求一个子表达式是字面整数常量——`["d", N, S]` 的 `N`／`S` 必须
/// 编译期已知，拒绝 `["d", ["+", 1, 1], 6]` 这类写法。
fn expect_int_literal(expr: &RawExpr, what: &str) -> Result<i64, String> {
    match expr {
        RawExpr::Int(n) => Ok(*n),
        other => Err(format!("{what} 必须是字面整数常量，实际是 {other:?}")),
    }
}

// ─────────────────────────── 经验曲线 ───────────────────────────

/// 把一个表达式编译成经验曲线的扁平指令数组。
///
/// 顶层补一层 `Ref` 的理由见 [`ll_sim::xp_curve::eval_xp_curve`] 文档
/// 「为什么需要 Ref」：若整个表达式恰好是单个操作数，递归编译不产生
/// 任何指令，补这一层保证 `instructions` 恒非空。
pub fn compile_xp_curve(expr: &RawExpr) -> Result<Vec<XpCurveOp>, String> {
    let mut out = Vec::new();
    let result = xp_curve_operand(expr, &mut out)?;
    if out.is_empty() {
        out.push(XpCurveOp::Ref(result));
    }
    Ok(out)
}

/// 递归编译一个子表达式，语义同 [`formula_operand`]。
fn xp_curve_operand(expr: &RawExpr, out: &mut Vec<XpCurveOp>) -> Result<XpCurveOperand, String> {
    match expr {
        RawExpr::Int(n) => Ok(XpCurveOperand::Const(*n)),
        RawExpr::Symbol(symbol) => match symbol.as_str() {
            "level" => Ok(XpCurveOperand::Level),
            "prev-requirement" => Ok(XpCurveOperand::PrevRequirement),
            other => Err(format!("经验曲线表达式引用了未知符号 {other:?}")),
        },
        RawExpr::Call(items) => {
            let Some((op, args)) = expr.as_call() else {
                return Err(format!(
                    "经验曲线表达式数组的第一个元素必须是运算符符号，实际是 {:?}",
                    items.first()
                ));
            };
            xp_curve_call(op, args, out)
        }
    }
}

/// 编译一次算子调用——经验曲线没有骰子（成长节奏必须确定性可预测），
/// 因此比 [`formula_call`] 少一个 `d` 分支。
fn xp_curve_call(
    op: &str,
    args: &[RawExpr],
    out: &mut Vec<XpCurveOp>,
) -> Result<XpCurveOperand, String> {
    if op == "if" {
        let [cond_expr, if_true_expr, if_false_expr] = args else {
            return Err(format!(
                "if 需要恰好三个参数（判据/真分支/假分支），实际 {}",
                args.len()
            ));
        };
        let cond = xp_curve_cond(cond_expr, out)?;
        let if_true = xp_curve_operand(if_true_expr, out)?;
        let if_false = xp_curve_operand(if_false_expr, out)?;
        out.push(XpCurveOp::Select {
            cond,
            if_true,
            if_false,
        });
        return Ok(XpCurveOperand::Local((out.len() - 1) as u8));
    }

    let [a_expr, b_expr] = args else {
        return Err(format!(
            "算子 {op:?} 需要恰好两个操作数，实际 {}",
            args.len()
        ));
    };
    let a = xp_curve_operand(a_expr, out)?;
    let b = xp_curve_operand(b_expr, out)?;
    let instruction = match op {
        "+" => XpCurveOp::Add(a, b),
        "-" => XpCurveOp::Sub(a, b),
        "*" => XpCurveOp::Mul(a, b),
        "/" => XpCurveOp::Div(a, b),
        "mul-permille" => XpCurveOp::MulPermille(a, b),
        "min" => XpCurveOp::Min(a, b),
        "max" => XpCurveOp::Max(a, b),
        other => return Err(format!("经验曲线表达式引用了未知算子 {other:?}")),
    };
    out.push(instruction);
    Ok(XpCurveOperand::Local((out.len() - 1) as u8))
}

/// 编译 `if` 的判据部分，语义同 [`formula_cond`]。
fn xp_curve_cond(expr: &RawExpr, out: &mut Vec<XpCurveOp>) -> Result<XpCurveCond, String> {
    let Some((cmp, args)) = expr.as_call() else {
        return Err("if 的判据必须写成 [比较符, 操作数, 操作数] 形式".to_string());
    };
    let [a_expr, b_expr] = args else {
        return Err(format!(
            "比较符 {cmp:?} 需要恰好两个操作数，实际 {}",
            args.len()
        ));
    };
    let a = xp_curve_operand(a_expr, out)?;
    let b = xp_curve_operand(b_expr, out)?;
    match cmp {
        "<" => Ok(XpCurveCond::Lt(a, b)),
        "<=" => Ok(XpCurveCond::Le(a, b)),
        ">" => Ok(XpCurveCond::Gt(a, b)),
        ">=" => Ok(XpCurveCond::Ge(a, b)),
        "=" => Ok(XpCurveCond::Eq(a, b)),
        "!=" => Ok(XpCurveCond::Ne(a, b)),
        other => Err(format!("未知比较符 {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 嵌套数组表达式能从json5直接反序列化() {
        // 这是本模块存在的前提：untagged 枚举在 json5 上真的能按
        // 「数字/字符串/数组」分派。空口宣称不算，这条是实测。
        // Arrange & Act
        let expr: RawExpr =
            json5::from_str(r#"["+", 100, ["*", "level", 40]]"#).expect("合法表达式");

        // Assert
        assert_eq!(
            expr,
            RawExpr::Call(vec![
                RawExpr::Symbol("+".to_string()),
                RawExpr::Int(100),
                RawExpr::Call(vec![
                    RawExpr::Symbol("*".to_string()),
                    RawExpr::Symbol("level".to_string()),
                    RawExpr::Int(40),
                ]),
            ])
        );
    }

    #[test]
    fn 伤害公式未知符号当场报错并点名它() {
        // Arrange
        let expr: RawExpr = json5::from_str(r#"["+", "attack-power", "luck-mod"]"#).expect("合法");

        // Act
        let result = compile_damage_formula(&expr);

        // Assert
        assert!(result.is_err_and(|err| err.contains("luck-mod")));
    }

    #[test]
    fn 经验曲线不认识骰子算子() {
        // 经验曲线的成长节奏必须确定性可预测——`d` 在这里不是"还没
        // 支持"，是不该支持。
        // Arrange
        let expr: RawExpr = json5::from_str(r#"["d", 1, 6]"#).expect("合法");

        // Act
        let result = compile_xp_curve(&expr);

        // Assert
        assert!(result.is_err_and(|err| err.contains("\"d\"")));
    }

    #[test]
    fn 骰子的个数与面数必须是字面常量() {
        // Arrange
        let expr: RawExpr = json5::from_str(r#"["d", ["+", 1, 1], 6]"#).expect("合法");

        // Act
        let result = compile_damage_formula(&expr);

        // Assert
        assert!(result.is_err_and(|err| err.contains("字面整数常量")));
    }
}
