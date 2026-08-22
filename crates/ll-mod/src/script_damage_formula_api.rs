//! 把 `register-damage-formula` 注册进脚本引擎：装载期把一份 `quote`
//! 包住的 s-表达式编译成 [`ll_sim::formula::FormulaOp`] 扁平指令数组，
//! 落地 `knowledge/design/damage-formula-mod-api.md` 三节。
//!
//! # 编译器落在哪个 crate
//!
//! 与 `crate::script_xp_curve_api` 模块文档「编译器落在哪个 crate」
//! 一节同构（这是该结论在伤害公式上的落地）：编译器
//! （`SteelVal → Vec<FormulaOp>`）落在本 crate（`ll-mod`，依赖
//! `steel-core`），`FormulaOp`/`FormulaOperand`/求值器落在
//! `ll-sim::formula`（纯 Rust 整数运算，不依赖 `steel-core`）。设计
//! 文档一节「第三处核实」明确点出：本模块取 `ll_script::behavior::tick`
//! 遍历行为树的**数据表示**（`quote` 包起来的 s-表达式），不取它的
//! **求值方式**（遍历时回调 Steel）——本模块编译完就与 Steel VM 再无
//! 瓜葛，`ll_sim::formula::eval_formula` 全程只碰纯 Rust 数据。
//!
//! # 为什么表达式参数的 Rust 类型是 `SteelVal`
//!
//! 与 `crate::script_xp_curve_api` 同一节文档同一条理由：`quote` 阻止
//! Steel 对表达式内容求值，脚本引擎把这份数据原样交给注册的 Rust
//! 函数，本模块的编译器手工递归下降遍历它。
//!
//! # 封闭表：与经验曲线共享算术子集，追加骰子与战斗专属操作数
//!
//! 与 `crate::script_xp_curve_api` 的封闭表共享 `+`/`-`/`*`/`/`/
//! `mul-permille`/`min`/`max`/`if` 八个算子（设计文档三节表述与经验
//! 曲线一致），追加 `(d N S)` 骰子算子（任务硬要求六）与
//! `attack-power`/`defense`/`pen-flat`/`pen-permille`/`str-mod`~
//! `cha-mod`/`crit` 战斗专属操作数。**没有 `multi-hit`/`adv`/`disadv`
//! ——本批次判断它们不属于引擎核心（见 `ll_sim::formula` 模块文档
//! 「本批次排除」一节），任何引用这些符号的表达式与任何拼写错误一样，
//! 落进「未知符号即拒绝」这条统一规则，不需要专门的特判分支。**
//!
//! # 明确禁止：`lambda`/`define`/`let`/任意函数调用（任务硬要求四）
//!
//! 封闭表里压根没有这些符号——`compile_call` 遇到列表头部是一个不认识
//! 的符号时统一走 `other => Err(...)` 分支，`lambda`/`define`/`let`
//! 与任意笔误拼错的符号走同一条「拒绝并报错」的路径,不需要为
//! `lambda` 专门写一条特判「检测到 lambda,报错」——普适的「未知符号
//! 即拒绝」规则本身已经覆盖了这个情况，与设计文档三节「为什么这条
//! 禁令必须结构性」一节的论证完全一致。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use steel::rvals::SteelVal;

use crate::active_registry::with_active_registry;
use crate::formula::{FormulaError, FormulaTable};
use ll_world::entity::AttributeKind;

use ll_sim::formula::{
    DICE_COUNT_RANGE, DICE_SIDES_RANGE, FormulaCond, FormulaDef, FormulaOp, FormulaOperand,
    MAX_FORMULA_INSTRUCTIONS,
};

thread_local! {
    /// 当前调用窗口内，`register-damage-formula` 应该写入的公式表。
    static ACTIVE_TABLE: RefCell<Option<FormulaTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内可写入的目标。
pub fn set_active_target(table: FormulaTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 [`FormulaTable`]。
pub fn take_active_target() -> FormulaTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-damage-formula` 注册进 `engine`。
pub fn register_damage_formula_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-damage-formula", register_damage_formula);
}

/// `(register-damage-formula id (quote 表达式))`——见模块文档「为什么
/// 表达式参数的 Rust 类型是 SteelVal」一节。
fn register_damage_formula(id: String, expr: SteelVal) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-damage-formula 在没有活跃公式表的窗口内被调用".to_string());
            };
            let parsed_id =
                NamespacedId::parse(&id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
            let index = registry.intern(parsed_id);
            let instructions = compile_damage_formula_expression(&expr)?;
            let needs_rng = instructions
                .iter()
                .any(|op| matches!(op, FormulaOp::Dice { .. }));
            table
                .define(
                    index,
                    FormulaDef {
                        id: index,
                        instructions,
                        needs_rng,
                    },
                )
                .map(|()| true)
                .map_err(|err: FormulaError| err.to_string())
        })
    })
}

/// 把一份 `quote` 包住的 s-表达式（`SteelVal`）编译成扁平指令数组
/// ——装载期一次性完成，运行期 [`ll_sim::formula::eval_formula`] 零
/// 脚本参与，见模块文档「编译器落在哪个 crate」一节。
fn compile_damage_formula_expression(expr: &SteelVal) -> Result<Vec<FormulaOp>, String> {
    let mut out = Vec::new();
    let result = compile_operand(expr, &mut out)?;
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

/// 递归编译一个子表达式，返回引用它求值结果的操作数——理由同
/// `crate::script_xp_curve_api::compile_operand` 文档。
fn compile_operand(expr: &SteelVal, out: &mut Vec<FormulaOp>) -> Result<FormulaOperand, String> {
    match expr {
        SteelVal::IntV(n) => Ok(FormulaOperand::Const(*n as i64)),
        SteelVal::SymbolV(symbol) => operand_from_symbol(symbol.as_str()),
        SteelVal::ListV(list) => {
            let items: Vec<SteelVal> = list.iter().cloned().collect();
            let Some(SteelVal::SymbolV(op)) = items.first() else {
                return Err("伤害公式表达式列表的第一个元素必须是运算符符号".to_string());
            };
            compile_call(op.as_str(), &items[1..], out)
        }
        other => Err(format!("伤害公式表达式包含不支持的字面量：{other:?}")),
    }
}

/// 把一个裸符号映射到 [`FormulaOperand`]——封闭表，任何不在这张表里
/// 的符号一律拒绝（任务硬要求四：未知符号即拒绝，不静默降级）。
fn operand_from_symbol(symbol: &str) -> Result<FormulaOperand, String> {
    match symbol {
        "attack-power" => Ok(FormulaOperand::AttackPower),
        "defense" => Ok(FormulaOperand::Defense),
        "pen-flat" => Ok(FormulaOperand::PenetrationFlat),
        "pen-permille" => Ok(FormulaOperand::PenetrationPermille),
        // wis-mod 映射到 Willpower——本项目没有独立的「感知/意志」
        // 二分，见 ll_sim::formula::FormulaOperand::AttributeModifier
        // 文档。
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

/// 编译一次算子/骰子调用——`+`/`-`/`*`/`/`/`mul-permille`/`min`/`max`
/// 全部是二元算子，`if` 是三元（判据 + 两个分支），`d` 是骰子（两个
/// **字面整数常量**参数，不能是子表达式，见设计文档三节）。不在这份
/// 封闭表内的符号（含 `lambda`/`define`/`let`/`multi-hit`/`adv`/
/// `disadv`……）一律拒绝，见模块文档「明确禁止」一节。
fn compile_call(
    op: &str,
    args: &[SteelVal],
    out: &mut Vec<FormulaOp>,
) -> Result<FormulaOperand, String> {
    if op == "if" {
        let [cond_expr, if_true_expr, if_false_expr] = args else {
            return Err(format!(
                "if 需要恰好三个参数（判据/真分支/假分支），实际 {}",
                args.len()
            ));
        };
        let cond = compile_cond(cond_expr, out)?;
        let if_true = compile_operand(if_true_expr, out)?;
        let if_false = compile_operand(if_false_expr, out)?;
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

    // 先确认 op 本身是封闭表里的二元算子之一，再递归编译两个操作数——
    // 不能反过来（先假设 args 是两个操作数、递归编译完再检查 op 是否
    // 认识）：`(lambda (x) x)` 恰好也有两个"参数"（`(x)` 与 `x`），若
    // 先递归编译,会在编译 `(x)`（列表头部是符号 `x`，零参数）这一步
    // 先撞上一条与 `lambda` 毫无关系的"未知算子 x"报错,把真正的问题
    // 症状（引用了 lambda）埋没掉,错误信息不再可诊断（任务硬要求四：
    // 报错必须明确指出问题符号）。
    if !KNOWN_BINARY_OPS.contains(&op) {
        return Err(format!("伤害公式表达式引用了未知算子 {op:?}"));
    }
    let [a_expr, b_expr] = args else {
        return Err(format!(
            "算子 {op:?} 需要恰好两个操作数，实际 {}",
            args.len()
        ));
    };
    let a = compile_operand(a_expr, out)?;
    let b = compile_operand(b_expr, out)?;
    let instruction = match op {
        "+" => FormulaOp::Add(a, b),
        "-" => FormulaOp::Sub(a, b),
        "*" => FormulaOp::Mul(a, b),
        "/" => FormulaOp::Div(a, b),
        "mul-permille" => FormulaOp::MulPermille(a, b),
        "min" => FormulaOp::Min(a, b),
        "max" => FormulaOp::Max(a, b),
        // 已经在函数顶部用 KNOWN_BINARY_OPS 穷尽校验过 op 属于这七个
        // 之一,这个分支理论不可达。
        other => return Err(format!("伤害公式表达式引用了未知算子 {other:?}")),
    };
    out.push(instruction);
    Ok(FormulaOperand::Local((out.len() - 1) as u8))
}

/// 封闭的二元算子表——[`compile_call`] 在递归编译任何操作数之前，先用
/// 它确认 `op` 是不是这七个之一，理由见调用点文档。
const KNOWN_BINARY_OPS: [&str; 7] = ["+", "-", "*", "/", "mul-permille", "min", "max"];

/// 要求一个子表达式是字面整数常量，不能是任何形式的子表达式——`(d N S)`
/// 的 `N`/`S` 必须编译期已知（设计文档三节），拒绝 `(d (+ 1 1) 6)`
/// 这类写法。
fn expect_int_literal(expr: &SteelVal, what: &str) -> Result<i64, String> {
    match expr {
        SteelVal::IntV(n) => Ok(*n as i64),
        other => Err(format!("{what} 必须是字面整数常量，实际是 {other:?}")),
    }
}

/// 编译 `if` 的判据部分：`(cmp-op a b)`，理由同
/// `crate::script_xp_curve_api::compile_cond` 文档。
fn compile_cond(expr: &SteelVal, out: &mut Vec<FormulaOp>) -> Result<FormulaCond, String> {
    let SteelVal::ListV(list) = expr else {
        return Err("if 的判据必须写成 (比较符 操作数 操作数) 形式".to_string());
    };
    let items: Vec<SteelVal> = list.iter().cloned().collect();
    let Some(SteelVal::SymbolV(cmp)) = items.first() else {
        return Err("if 的判据列表第一个元素必须是比较符符号".to_string());
    };
    let [a_expr, b_expr] = &items[1..] else {
        return Err(format!(
            "比较符 {:?} 需要恰好两个操作数，实际 {}",
            cmp.as_str(),
            items.len().saturating_sub(1)
        ));
    };
    let a = compile_operand(a_expr, out)?;
    let b = compile_operand(b_expr, out)?;
    match cmp.as_str() {
        "<" => Ok(FormulaCond::Lt(a, b)),
        "<=" => Ok(FormulaCond::Le(a, b)),
        ">" => Ok(FormulaCond::Gt(a, b)),
        ">=" => Ok(FormulaCond::Ge(a, b)),
        "=" => Ok(FormulaCond::Eq(a, b)),
        "!=" => Ok(FormulaCond::Ne(a, b)),
        other => Err(format!("未知比较符 {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use ll_sim::formula::{FormulaInputs, eval_formula};

    #[test]
    fn 合法确定性公式注册成功并求值出正确结果() {
        // Arrange：铁剑纯物理风格（设计文档四节示例一精神，简化到本
        // 批次「公式只算攻击力」的语义）——(+ attack-power str-mod)。
        let mut engine = ScriptEngine::new();
        register_damage_formula_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(FormulaTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-damage-formula "yourmod:iron_sword_formula"
                 (quote (+ attack-power str-mod)))"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:iron_sword_formula").unwrap())
            .expect("刚注册的内容应能查到索引");
        let def = table.get(index).expect("刚注册的公式应能查到定义");
        assert!(!def.needs_rng);
        let mut rng = ll_core::rng::DetRng::for_entity(1, 2, 3);
        let mut inputs = FormulaInputs::new(10, 0, 0, 0, [0; 7], false);
        inputs.attribute_modifiers[AttributeKind::Strength as usize] = 4;
        assert_eq!(eval_formula(def, &inputs, &mut rng), 14);
    }

    #[test]
    fn 含骰子的公式注册成功且needs_rng为真() {
        // Arrange：(d 1 8) ——单纯验证骰子算子被正确编译且标记 needs_rng。
        let mut engine = ScriptEngine::new();
        register_damage_formula_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(FormulaTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-damage-formula "yourmod:dice_formula" (quote (d 1 8)))"#.to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:dice_formula").unwrap())
            .expect("刚注册的内容应能查到索引");
        let def = table.get(index).expect("刚注册的公式应能查到定义");
        assert!(def.needs_rng);
        let mut rng = ll_core::rng::DetRng::for_entity(1, 2, 3);
        let result_value = eval_formula(
            def,
            &FormulaInputs::new(0, 0, 0, 0, [0; 7], false),
            &mut rng,
        );
        assert!((1..=8).contains(&result_value));
    }

    #[test]
    fn 骰子个数超出上限时注册失败() {
        // Arrange：N=21 超出 [1,20] 合法范围（设计文档三节校验第 6 条）。
        let mut engine = ScriptEngine::new();
        register_damage_formula_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(FormulaTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-damage-formula "yourmod:too_many_dice" (quote (d 21 6)))"#.to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn 骰子参数是子表达式而非字面常量时注册失败() {
        // Arrange：(d (+ 1 1) 6)——设计文档三节「N/S 必须是字面整数常量,
        // 不能是子表达式」。
        let mut engine = ScriptEngine::new();
        register_damage_formula_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(FormulaTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-damage-formula "yourmod:bad_dice" (quote (d (+ 1 1) 6)))"#.to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn 表达式含lambda时注册被拒绝且错误信息可诊断() {
        // 任务硬要求四：编译器必须拒绝 lambda，装载期拒绝并明确报错，
        // 不得静默降级。
        // Arrange
        let mut engine = ScriptEngine::new();
        register_damage_formula_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(FormulaTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-damage-formula "yourmod:lambda_formula"
                 (quote (lambda (x) x)))"#
                .to_string(),
        );

        // Assert：注册被拒绝，且错误信息里能看到「lambda」这个诊断线索。
        assert!(result.is_err());
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("lambda"),
            "错误信息应当能诊断出问题符号，实际：{message}"
        );

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn 表达式引用未注册符号时注册失败而不panic() {
        // Arrange：level 是经验曲线的操作数，不在伤害公式的封闭表内。
        let mut engine = ScriptEngine::new();
        register_damage_formula_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(FormulaTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-damage-formula "yourmod:bad_formula" (quote (+ level 1)))"#.to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn if表达式按crit操作数选择对应分支() {
        // Arrange：暴击时 2d12，否则 1d12——设计文档四节示例二的暴击
        // 处理点（骰子数量翻倍,不是最终结果乘二）。
        let mut engine = ScriptEngine::new();
        register_damage_formula_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(FormulaTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-damage-formula "yourmod:crit_dice_formula"
                 (quote (if (= crit 1) (d 2 12) (d 1 12))))"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:crit_dice_formula").unwrap())
            .expect("刚注册的内容应能查到索引");
        let def = table.get(index).expect("刚注册的公式应能查到定义");
        let mut rng_crit = ll_core::rng::DetRng::for_entity(1, 2, 3);
        let crit_result = eval_formula(
            def,
            &FormulaInputs::new(0, 0, 0, 0, [0; 7], true),
            &mut rng_crit,
        );
        assert!((2..=24).contains(&crit_result), "暴击应落在 2d12 范围内");
    }
}
