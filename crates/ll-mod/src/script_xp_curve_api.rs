//! 把 `register-xp-curve`/`register-class-xp-curve`/`register-race-xp-curve`
//! 注册进脚本引擎——`knowledge/design/level-and-experience-system.md`
//! 三、四、八节定案的编译器落点。
//!
//! # 编译器落在哪个 crate
//!
//! 与伤害公式设计文档「编译产物住在哪个 crate」一节同构（本模块是
//! 该结论在经验曲线上的首次真实落地）：编译器
//! （`SteelVal → Vec<XpCurveOp>`）落在本 crate（`ll-mod`，依赖
//! `steel-core`），`XpCurveOp`/`XpCurveOperand`/求值器落在
//! `ll-sim::xp_curve`（纯 Rust 整数运算，不依赖 `steel-core`）。
//!
//! # 为什么表达式参数的 Rust 类型是 `SteelVal`
//!
//! `register-xp-curve` 的第三个参数是 `(quote 表达式)`——`quote` 阻止
//! Steel 对表达式内容求值，脚本引擎因此把「这份数据本身」（一棵由
//! `SteelVal::ListV`/`SteelVal::SymbolV`/`SteelVal::IntV` 组成的树）
//! 原样交给注册的 Rust 函数，不是先帮你把它当成一次函数调用求值。
//! `steel-core` 的 `SteelVal` 自身实现了 `FromSteelVal`（恒等转换），
//! `register_fn` 因此可以直接声明一个 `SteelVal` 类型的参数收下这棵
//! 树，本模块的编译器再手工递归下降遍历它——这正是
//! `ll_script::behavior::tick`（`crates/ll-script/src/behavior.rs`）
//! 遍历行为树 `selector`/`sequence` 结构时已经验证过的同一种遍历手法，
//! 只是那里遍历完直接求值调用脚本函数，这里遍历完是编译成扁平指令。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use steel::rvals::SteelVal;

use crate::active_registry::with_active_registry;
use crate::xp_curve::{XpCurveBindings, XpCurveError, XpCurveTable};
use ll_sim::xp_curve::{XpCurveCond, XpCurveDef, XpCurveOp, XpCurveOperand};

thread_local! {
    /// 当前调用窗口内，`register-xp-curve`/`register-class-xp-curve`/
    /// `register-race-xp-curve` 应该写入的曲线表与绑定表。
    static ACTIVE_TABLES: RefCell<Option<(XpCurveTable, XpCurveBindings)>> = const { RefCell::new(None) };
}

/// 把 `(table, bindings)` 设为当前调用窗口内可写入的目标。
pub fn set_active_target(table: XpCurveTable, bindings: XpCurveBindings) {
    ACTIVE_TABLES.with(|cell| *cell.borrow_mut() = Some((table, bindings)));
}

/// 取回 [`set_active_target`] 放进去的 `(XpCurveTable, XpCurveBindings)`。
pub fn take_active_target() -> (XpCurveTable, XpCurveBindings) {
    ACTIVE_TABLES.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把三个函数注册进 `engine`。
pub fn register_xp_curve_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-xp-curve", register_xp_curve);
    engine.register_fn("register-class-xp-curve", register_class_xp_curve);
    engine.register_fn("register-race-xp-curve", register_race_xp_curve);
}

/// `(register-xp-curve id base-requirement (quote 表达式))`——见模块
/// 文档「为什么表达式参数的 Rust 类型是 SteelVal」一节。
fn register_xp_curve(id: String, base_requirement: i64, expr: SteelVal) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLES.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some((table, _bindings)) = slot.as_mut() else {
                return Err("register-xp-curve 在没有活跃曲线表的窗口内被调用".to_string());
            };
            let parsed_id =
                NamespacedId::parse(&id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
            let index = registry.intern(parsed_id);
            let instructions = compile_xp_curve_expression(&expr)?;
            table
                .define(
                    index,
                    XpCurveDef {
                        id: index,
                        base_requirement,
                        instructions,
                    },
                )
                .map(|()| true)
                .map_err(|err: XpCurveError| err.to_string())
        })
    })
}

/// `(register-class-xp-curve class-id curve-id)`——一档纯绑定，两个
/// 参数都必须是已经通过各自的 `register-*` 注册过的完整命名空间标识
/// 符字符串，见 [`crate::xp_curve::XpCurveBindings::bind_class`] 文档。
fn register_class_xp_curve(class_id: String, curve_id: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLES.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some((table, bindings)) = slot.as_mut() else {
                return Err("register-class-xp-curve 在没有活跃曲线表的窗口内被调用".to_string());
            };
            let class_index = resolve_registered_id(registry, &class_id)?;
            let curve_index = resolve_registered_id(registry, &curve_id)?;
            bindings
                .bind_class(table, class_index, curve_index)
                .map(|()| true)
                .map_err(|err: XpCurveError| err.to_string())
        })
    })
}

/// `(register-race-xp-curve race-id curve-id)`，与
/// [`register_class_xp_curve`] 同构，服务种族。
fn register_race_xp_curve(race_id: String, curve_id: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLES.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some((table, bindings)) = slot.as_mut() else {
                return Err("register-race-xp-curve 在没有活跃曲线表的窗口内被调用".to_string());
            };
            let race_index = resolve_registered_id(registry, &race_id)?;
            let curve_index = resolve_registered_id(registry, &curve_id)?;
            bindings
                .bind_race(table, race_index, curve_index)
                .map(|()| true)
                .map_err(|err: XpCurveError| err.to_string())
        })
    })
}

/// 把一个字符串标识符解析成已经注册过的 [`ll_core::ident::ContentIndex`]
/// ——与 `register-race`/`register-class` 不同,这里**不**做
/// `registry.intern`（那会给一个从未真正注册过的字符串静默分配一个
/// 新索引，绕过「目标必须已存在」这条校验）,只用 `Registry::get` 查
/// 已有登记，查不到即报错。
fn resolve_registered_id(
    registry: &crate::registry::Registry,
    id: &str,
) -> Result<ll_core::ident::ContentIndex, String> {
    let parsed = NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    registry
        .get(&parsed)
        .ok_or_else(|| format!("标识符 {id:?} 尚未注册"))
}

/// 把一份 `quote` 包住的 s-表达式（`SteelVal`）编译成扁平指令数组
/// ——装载期一次性完成，运行期 [`ll_sim::xp_curve::eval_xp_curve`]
/// 零脚本参与，见 `ll-sim::xp_curve` 模块文档「求值语义」一节。
///
/// # 为什么需要顶层包一层 `Ref`
///
/// 见 [`ll_sim::xp_curve::eval_xp_curve`] 文档「为什么需要 Ref」一节
/// ——若整个表达式恰好是单个操作数（`level`/`prev-requirement`/一个
/// 字面整数），递归编译不会产生任何指令，这里补上这一层，保证
/// `instructions` 恒非空、且最后一条指令的结果就是答案。
fn compile_xp_curve_expression(expr: &SteelVal) -> Result<Vec<XpCurveOp>, String> {
    let mut out = Vec::new();
    let result = compile_operand(expr, &mut out)?;
    if out.is_empty() {
        out.push(XpCurveOp::Ref(result));
    }
    Ok(out)
}

/// 递归编译一个子表达式，返回引用它求值结果的操作数——叶子节点
/// （字面整数、`level`、`prev-requirement`）不产生任何指令，直接返回
/// 对应操作数；复合表达式（`(op a b)`/`(if (cmp a b) then else)`）
/// 先递归编译子表达式,再往 `out` 追加恰好一条指令,返回引用这条新指令
/// 的 [`XpCurveOperand::Local`]。
fn compile_operand(expr: &SteelVal, out: &mut Vec<XpCurveOp>) -> Result<XpCurveOperand, String> {
    match expr {
        SteelVal::IntV(n) => Ok(XpCurveOperand::Const(*n as i64)),
        SteelVal::SymbolV(symbol) => match symbol.as_str() {
            "level" => Ok(XpCurveOperand::Level),
            "prev-requirement" => Ok(XpCurveOperand::PrevRequirement),
            other => Err(format!("经验曲线表达式引用了未知符号 {other:?}")),
        },
        SteelVal::ListV(list) => {
            let items: Vec<SteelVal> = list.iter().cloned().collect();
            let Some(SteelVal::SymbolV(op)) = items.first() else {
                return Err("经验曲线表达式列表的第一个元素必须是运算符符号".to_string());
            };
            compile_call(op.as_str(), &items[1..], out)
        }
        other => Err(format!("经验曲线表达式包含不支持的字面量：{other:?}")),
    }
}

/// 编译一次算子调用——`+`/`-`/`*`/`/`/`mul-permille`/`min`/`max` 全部
/// 是二元算子（恰好两个操作数），`if` 是三元（判据 + 两个分支）。不在
/// 这份封闭表内的符号一律拒绝，与伤害公式「未知符号即拒绝」同一条
/// 纪律（`damage-formula-mod-api.md` 三节）——mod 作者若在经验曲线里
/// 写了骰子/优势劣势之类的算子，会在这里直接报错，不会静默产出一条
/// 错误的曲线。
fn compile_call(
    op: &str,
    args: &[SteelVal],
    out: &mut Vec<XpCurveOp>,
) -> Result<XpCurveOperand, String> {
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
    let a = compile_operand(a_expr, out)?;
    let b = compile_operand(b_expr, out)?;
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

/// 编译 `if` 的判据部分：`(cmp-op a b)`，`cmp-op` 是 `<`/`<=`/`>`/`>=`/
/// `=`/`!=` 之一。
fn compile_cond(expr: &SteelVal, out: &mut Vec<XpCurveOp>) -> Result<XpCurveCond, String> {
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
    use crate::registry::Registry;
    use ll_sim::xp_curve::eval_xp_curve;

    #[test]
    fn 合法曲线声明注册成功并写入曲线表() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_xp_curve_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(XpCurveTable::new(), XpCurveBindings::new());

        // Act：战士曲线（设计文档四节示例一）—— 100 + 40 * level。
        let result = engine.load_source(
            r#"(register-xp-curve "yourmod:warrior_xp_curve" 140 (quote (+ 100 (* level 40))))"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let (table, _bindings) = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:warrior_xp_curve").unwrap())
            .expect("刚注册的内容应能查到索引");
        let curve = table.get(index).expect("刚注册的曲线应能查到定义");
        assert_eq!(eval_xp_curve(curve, 1, curve.base_requirement), 140);
    }

    #[test]
    fn 递推式曲线注册后求值结果与手算表一致() {
        // Arrange：法师曲线（设计文档四节示例二）——
        // max(prev-requirement + 20, prev-requirement * 1.18)。
        let mut engine = ScriptEngine::new();
        register_xp_curve_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(XpCurveTable::new(), XpCurveBindings::new());

        // Act
        let result = engine.load_source(
            r#"(register-xp-curve "yourmod:mage_xp_curve" 80
                 (quote (max (+ prev-requirement 20) (mul-permille prev-requirement 1180))))"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let (table, _bindings) = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:mage_xp_curve").unwrap())
            .expect("刚注册的内容应能查到索引");
        let curve = table.get(index).expect("刚注册的曲线应能查到定义");
        // 设计文档四节手算表「2→3」行：门槛 100 → 120。
        assert_eq!(eval_xp_curve(curve, 2, 100), 120);
    }

    #[test]
    fn 表达式引用未知符号时注册失败而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_xp_curve_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(XpCurveTable::new(), XpCurveBindings::new());

        // Act：`attack-power` 是伤害公式的操作数，不在经验曲线的封闭表内。
        let result = engine.load_source(
            r#"(register-xp-curve "yourmod:bad_curve" 100 (quote (+ attack-power 1)))"#.to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn if表达式按判据选择对应分支() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_xp_curve_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(XpCurveTable::new(), XpCurveBindings::new());

        // Act：等级低于 5 时门槛固定 50，否则固定 200。
        let result = engine.load_source(
            r#"(register-xp-curve "yourmod:tiered_curve" 50
                 (quote (if (< level 5) 50 200)))"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let (table, _bindings) = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:tiered_curve").unwrap())
            .expect("刚注册的内容应能查到索引");
        let curve = table.get(index).expect("刚注册的曲线应能查到定义");
        assert_eq!(eval_xp_curve(curve, 3, 0), 50);
        assert_eq!(eval_xp_curve(curve, 7, 0), 200);
    }

    #[test]
    fn 绑定职业到已注册曲线成功() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_xp_curve_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(XpCurveTable::new(), XpCurveBindings::new());
        engine
            .load_source(
                r#"(register-xp-curve "yourmod:warrior_xp_curve" 140 (quote (+ 100 (* level 40))))"#
                    .to_string(),
            )
            .expect("先注册曲线本体");
        // 职业本身不需要真的存在于 ClassTable——`register-class-xp-curve`
        // 只要求两个参数各自在 Registry 里有过一次 intern（这里借用
        // register-xp-curve 顺带完成了曲线那一侧的 intern，职业侧另外
        // 手动 intern 一个占位标识符）。
        crate::active_registry::with_active_registry(|registry| {
            registry.intern(NamespacedId::parse("yourmod:warrior_class").unwrap());
            Ok::<(), String>(())
        })
        .expect("窗口内存在活跃 registry");

        // Act
        let result = engine.load_source(
            r#"(register-class-xp-curve "yourmod:warrior_class" "yourmod:warrior_xp_curve")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let (_table, bindings) = take_active_target();
        let class_index = registry
            .get(&NamespacedId::parse("yourmod:warrior_class").unwrap())
            .unwrap();
        let curve_index = registry
            .get(&NamespacedId::parse("yourmod:warrior_xp_curve").unwrap())
            .unwrap();
        assert_eq!(bindings.class_curve(class_index), Some(curve_index));
    }

    #[test]
    fn 绑定到不存在的曲线id时返回err() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_xp_curve_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(XpCurveTable::new(), XpCurveBindings::new());
        crate::active_registry::with_active_registry(|registry| {
            registry.intern(NamespacedId::parse("yourmod:warrior_class").unwrap());
            Ok::<(), String>(())
        })
        .expect("窗口内存在活跃 registry");

        // Act：从未注册过 "yourmod:never_registered_curve"。
        let result = engine.load_source(
            r#"(register-class-xp-curve "yourmod:warrior_class" "yourmod:never_registered_curve")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
