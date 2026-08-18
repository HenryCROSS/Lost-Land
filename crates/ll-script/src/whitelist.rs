//! AST 级白名单：脚本能引用哪些标识符，由一份允许列表穷举，不在列表内
//! 的一律拒绝——包括我们没有专门想到、专门拉黑的名字。
//!
//! # 为什么从黑名单（`host.rs` 的 `reject_dangerous_syntax`）换成白名单
//!
//! 黑名单只能拦住写进清单里的写法；`steel/meta` 实测有 102 个导出名字，
//! `steel-core` 每次升级都可能新增没人预料到的口子。白名单反过来：
//! 只要不在允许列表里，天然拒绝，包括未来新增的、我们从没听说过的
//! 内置函数——这才是「能力不存在」而不是「已知能力被拦下」。
//!
//! # 必须校验「完整展开后」的 AST（实测验证，非假设）
//!
//! `require-builtin` 是编译期宏。展开前的 AST 只有一个 `Require` 节点，
//! 看不出脚本最终引用了什么；展开后会变成一串
//! `(define instant/now (%module-get% %-builtin-module-steel/time 'instant/now))`
//! ——真正暴露出来的引用是 `%module-get%`、`%-builtin-module-steel/time`
//! 这些名字。实测：`Engine::emit_fully_expanded_ast` 给出的正是这份
//! 已经完整展开的 AST（用 `crates/ll-script/examples/probe_whitelist.rs`
//! 验证过：对 `(require-builtin steel/time) (instant/now)` 调用它，
//! 拿到的第一个顶层节点就是上面这串 `define`）。只要 `%module-get%`/
//! `%-builtin-module-*` 不在白名单里，这类展开产物会在遇到它们的那一刻
//! 被拒绝，不需要专门认识 `steel/time` 这个模块名。
//!
//! # 为什么跳过 `quote` 包住的部分
//!
//! 脚本用符号表达数据是正常用法（本 crate `api/intent.rs` 约定脚本用
//! `(list 'move 'north)` 表达意图）——`'north` 不是「引用了一个叫
//! north 的函数」，只是符号字面量。只检查非 `quote` 上下文里的标识符，
//! 才能既堵住真正的引用，又不误伤把符号当数据用的正常写法。
//!
//! # 运行时字符串拼符号——实测不构成绕过
//!
//! `probe_whitelist.rs` 实测过：脚本用 `(string->symbol (apply
//! string-append (list "require" "-" "builtin")))` 能**造出**一个叫
//! `require-builtin` 的符号值，但这只是数据构造，并不会触发
//! `require-builtin` 的宏展开机制——宏展开只认字面出现在源码里的
//! `(require-builtin ...)` 形式。真正能让这类拼出来的符号产生效果的
//! 唯一途径是 `eval!`/`run!`/`eval-string` 这类反射入口，而这些名字本身
//! 不在白名单里，会在脚本引用它们的那一刻被拒绝——不需要额外识别「这是
//! 不是拼出来的」。

use std::collections::HashSet;

use steel::parser::ast::{Atom, ExprKind, List};

use crate::host::ScriptError;

/// 校验一组已经完整展开的顶层表达式，确认里面出现的每一个"被引用的
/// 标识符"都在 `allowed` 里。命中第一个不在白名单内的标识符就立刻
/// 拒绝并在错误信息里点名，方便 mod 作者定位。
pub fn check_whitelist(
    exprs: &[ExprKind],
    allowed: &HashSet<&'static str>,
) -> Result<(), ScriptError> {
    let no_locals = HashSet::new();
    for expr in exprs {
        walk(expr, allowed, &no_locals)?;
    }
    Ok(())
}

fn reject(name: &str) -> Result<(), ScriptError> {
    Err(ScriptError::ParseError(format!(
        "脚本引用了不在白名单内的标识符「{name}」"
    )))
}

/// 一个标识符是否可以出现在这个位置：要么是白名单里的全局能力，要么是
/// 当前词法作用域内自己绑定的局部名（函数参数、`let` 绑定、递归函数
/// 自身的名字）。
///
/// **局部名必须单独判断，不能简单地"跳过所有引用检查"**：编译期的
/// 卫生宏展开会把参数名重写成 `##a2` 这样的内部记号（实测
/// `(define (add a b) (+ a b))` 展开后函数体引用的是 `##a2`/`##b2`，不是
/// 原始的 `a`/`b`），若不区分局部/全局，这类正常脚本会被误判成引用了
/// 白名单外的标识符。区分的办法是在遍历时维护"当前作用域内绑定了哪些
/// 名字"，只有**不在这个集合里**的引用才需要对照白名单——这正是编译器
/// 教科书里的自由变量分析，不是本模块发明的新概念。
fn check_reference<'a>(
    name: &'a str,
    allowed: &HashSet<&'static str>,
    locals: &HashSet<&'a str>,
) -> Result<(), ScriptError> {
    if locals.contains(name) || allowed.contains(name) {
        Ok(())
    } else {
        reject(name)
    }
}

fn walk<'a>(
    expr: &'a ExprKind,
    allowed: &HashSet<&'static str>,
    locals: &HashSet<&'a str>,
) -> Result<(), ScriptError> {
    match expr {
        ExprKind::Atom(atom) => walk_atom(atom, allowed, locals),
        ExprKind::If(node) => {
            walk(&node.test_expr, allowed, locals)?;
            walk(&node.then_expr, allowed, locals)?;
            walk(&node.else_expr, allowed, locals)
        }
        ExprKind::Let(node) => {
            // 绑定值在外层作用域求值（不能看见 let 自己引入的名字），
            // body 才看得见新绑定的名字。
            for (_binding_name, value) in &node.bindings {
                walk(value, allowed, locals)?;
            }
            let mut inner = locals.clone();
            for (binding_name, _value) in &node.bindings {
                collect_bound_names(binding_name, &mut inner);
            }
            walk(&node.body_expr, allowed, &inner)
        }
        ExprKind::Define(node) => {
            // `node.name` 可能是单个原子（`(define x ...)`）也可能是一个
            // 列表（`(define (f a b) ...)`，此时首元素是函数名、其余是
            // 形参名）。两者都不需要在白名单里；函数体则需要能看见这些
            // 名字（尤其是递归函数要引用自己的名字）。
            let mut inner = locals.clone();
            collect_bound_names(&node.name, &mut inner);
            walk(&node.body, allowed, &inner)
        }
        ExprKind::LambdaFunction(node) => {
            let mut inner = locals.clone();
            for arg in &node.args {
                collect_bound_names(arg, &mut inner);
            }
            walk(&node.body, allowed, &inner)
        }
        ExprKind::Begin(node) => {
            for e in &node.exprs {
                walk(e, allowed, locals)?;
            }
            Ok(())
        }
        ExprKind::Return(node) => walk(&node.expr, allowed, locals),
        // reader 直接产出的 quote 节点：整体是数据，不检查内部标识符。
        ExprKind::Quote(_) => Ok(()),
        ExprKind::List(list) => walk_list(list, allowed, locals),
        ExprKind::Set(node) => {
            walk(&node.variable, allowed, locals)?;
            walk(&node.expr, allowed, locals)
        }
        // 完整展开后的 AST 理论上不应该再出现 Require 节点（已经被展开
        // 成一串 define），但防御性地直接拒绝——这不该发生，发生了说明
        // 展开没有按预期完成，宁可保守拒绝。
        ExprKind::Require(_) => reject("require"),
        // 不允许脚本自定义宏：宏能在展开期生成任意新代码，是比
        // require-builtin 更难静态审查的攻击面，本阶段直接整体拒绝。
        ExprKind::Macro(_) | ExprKind::SyntaxRules(_) => reject("define-syntax/宏定义"),
        ExprKind::Vector(node) => {
            for e in &node.args {
                walk(e, allowed, locals)?;
            }
            Ok(())
        }
    }
}

fn walk_atom<'a>(
    atom: &'a Atom,
    allowed: &HashSet<&'static str>,
    locals: &HashSet<&'a str>,
) -> Result<(), ScriptError> {
    match atom.ident() {
        Some(name) => check_reference(name.resolve(), allowed, locals),
        // 非标识符的字面量（数字/布尔/字符串等），不是引用，不检查。
        None => Ok(()),
    }
}

fn walk_list<'a>(
    list: &'a List,
    allowed: &HashSet<&'static str>,
    locals: &HashSet<&'a str>,
) -> Result<(), ScriptError> {
    // 首元素是 quote/quasiquote 时整个列表是数据，不检查内部标识符——
    // 这是 `'x` 写法在字面 `(quote x)` 形态下的等价情况，与
    // `ExprKind::Quote` 处理的是同一件事的两种语法糖表示。
    if let Some(ExprKind::Atom(head)) = list.args.first()
        && let Some(name) = head.ident()
        && matches!(name.resolve(), "quote" | "quasiquote")
    {
        return Ok(());
    }

    for e in &list.args {
        walk(e, allowed, locals)?;
    }
    Ok(())
}

/// 把一个"绑定位置"表达式（单个变量名，或 `(函数名 形参...)` 形式的
/// 列表）里出现的每一个标识符都收进 `into`——这些都是新引入的局部名，
/// 不需要对照白名单。
fn collect_bound_names<'a>(expr: &'a ExprKind, into: &mut HashSet<&'a str>) {
    match expr {
        ExprKind::Atom(atom) => {
            if let Some(name) = atom.ident() {
                into.insert(name.resolve());
            }
        }
        ExprKind::List(list) => {
            for e in &list.args {
                collect_bound_names(e, into);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel::steel_vm::engine::Engine;

    fn expand(engine: &mut Engine, source: &str) -> Vec<ExprKind> {
        engine
            .emit_fully_expanded_ast(source, None)
            .expect("测试用源码本身应当能通过编译")
    }

    #[test]
    fn 白名单内的普通计算脚本通过校验() {
        // Arrange
        let mut engine = Engine::new_sandboxed();
        let exprs = expand(&mut engine, "(define (add a b) (+ a b)) (add 1 2)");
        let allowed: HashSet<&'static str> = ["+", "define", "add", "a", "b"].into_iter().collect();

        // Act
        let result = check_whitelist(&exprs, &allowed);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn require_builtin展开后引用的模块内部名字被拒绝() {
        // Arrange
        let mut engine = Engine::new_sandboxed();
        let exprs = expand(&mut engine, "(require-builtin steel/time) (instant/now)");
        let allowed: HashSet<&'static str> = HashSet::new();

        // Act
        let result = check_whitelist(&exprs, &allowed);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn eval感叹号本身作为被引用标识符时被拒绝() {
        // Arrange
        let mut engine = Engine::new_sandboxed();
        let exprs = expand(&mut engine, r#"(eval! "(+ 1 2)")"#);
        let allowed: HashSet<&'static str> = HashSet::new();

        // Act
        let result = check_whitelist(&exprs, &allowed);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 引号包住的符号不被当作引用检查() {
        // Arrange：'north 只是数据，不应该要求 "north" 出现在白名单里。
        let mut engine = Engine::new_sandboxed();
        let exprs = expand(&mut engine, "(list 'move 'north)");
        let allowed: HashSet<&'static str> = ["list"].into_iter().collect();

        // Act
        let result = check_whitelist(&exprs, &allowed);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn 未列入白名单的裸引用被拒绝并在错误信息里点名() {
        // Arrange
        let mut engine = Engine::new_sandboxed();
        let exprs = expand(&mut engine, "(totally-unknown-function 1)");
        let allowed: HashSet<&'static str> = HashSet::new();

        // Act
        let result = check_whitelist(&exprs, &allowed);

        // Assert
        match result {
            Err(ScriptError::ParseError(msg)) => {
                assert!(msg.contains("totally-unknown-function"));
            }
            other => panic!("期望 ParseError 且带上违规名字，实际拿到 {other:?}"),
        }
    }
}
