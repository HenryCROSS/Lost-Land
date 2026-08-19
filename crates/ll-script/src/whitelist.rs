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
//!
//! # 白名单的定位：能力边界，不是语言子集（项目所有者裁定，写死在这里）
//!
//! **必须挡住的是能力**：文件系统、网络、进程、线程、墙钟、非确定性
//! 随机，以及能触达以上任意一项的反射入口（`eval!`/`run!`/
//! `require-builtin` 之类）。**必须放行的是语言本身**：闭包、递归、
//! 尾调用、宏（`define-syntax`/`syntax-rules`）、`quote`/`quasiquote`/
//! `unquote`、`let`/`let*`/`letrec`/命名 let、高阶函数
//! （`map`/`filter`/`fold`/`apply`）、列表/向量/哈希表作为数据结构、
//! 字符串与数学运算、用户自定义 `struct`——这些都是"纯的东西"，被挡住
//! 是白名单的缺陷，不是安全特性。
//!
//! **判断某个名字该不该放行的唯一标准是"它能不能到达上面那六类能力之
//! 一"，不是"我们对它有没有把握"。** 遇到不确定的名字，先去查它的
//! 实现（是否有 I/O、是否读写进程/主机状态、是否暴露随时间/运行而变的
//! 内部值如内存地址），能证明纯净就放行；只有证明得到会触达被禁能力，
//! 或者其行为在不同机器/不同次运行之间不一致（比如 `memory-address`
//! 打印的是真实指针值），才归入拒绝名单——`host.rs` 的
//! `META_DENY_LIST` 就是这样逐项审过的产物，不是"整个模块太复杂看不
//! 过来所以全清空"的偷懒决定（那是 `steel/meta` 曾经的做法，已经因为
//! 挡住了 `make-struct-type` 这种纯特性被推翻，见 ADR 0012）。
//!
//! # 与玩法层注册 API 一致性的联系（规格 §10.3，[ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)）
//!
//! 玩法层内容（技能效果、行为树等以 Steel 编写的部分）本体与 mod 走
//! 同一套 API；白名单太窄的后果不只是"mod 作者受限"，而是**本体自己
//! 也写不出内容来**——这与 ADR 0016 的守门规则同源：若本体需要一个
//! mod 够不着的东西，那是 API 缺陷，不是特性。反过来，白名单本身也要
//! 经受同一条检验：凡是"纯"的语言能力被误挡，都会先在本体自己的内容
//! 定义里被撞见，而不是等到 mod 作者抱怨。这条检验只在玩法层内成立
//! ——白名单本身不涉及引擎层（渲染、物理、寻路等）能力暴露的问题，
//! 那些系统本就不进脚本层，不受本模块约束。

use std::collections::HashSet;

use steel::parser::ast::{Atom, ExprKind, List};
use steel::parser::span::Span;

use crate::host::ScriptError;

/// 校验一组已经完整展开的顶层表达式，确认里面出现的每一个"被引用的
/// 标识符"都在 `allowed` 里。命中第一个不在白名单内的标识符就立刻
/// 拒绝并在错误信息里点名，方便 mod 作者定位。
pub fn check_whitelist(
    exprs: &[ExprKind],
    allowed: &HashSet<&'static str>,
) -> Result<(), ScriptError> {
    let mut locals = HashSet::new();
    walk_sequence(exprs, allowed, &mut locals)
}

/// 按顺序遍历一串同层级的表达式（顶层程序，或 `begin` 块），并且**让
/// 前面出现的 `define` 对后面的兄弟表达式可见**。
///
/// 这不是可选的优化，是正确性要求：`struct` 宏展开成一个 `begin` 块，
/// 里面先 `(define struct:Point (quote uninitialized))` 占位，后面几个
/// 兄弟表达式再 `(set! struct:Point ...)` 引用它——若每个兄弟表达式都
/// 拿同一份只读的 `locals` 快照检查（早期实现的 bug），`struct:Point`
/// 在被 `set!` 引用的那一刻会被误判成"没在前面的作用域里定义过的自由
/// 引用"，进而被白名单拒绝。顶层脚本同理：`(define p (Point 1 2))`
/// 后面接着 `(define (f) (list p))` 这种最常见的写法，也要求前一条
/// 顶层 `define` 对后一条可见。
fn walk_sequence<'a>(
    exprs: &'a [ExprKind],
    allowed: &HashSet<&'static str>,
    locals: &mut HashSet<&'a str>,
) -> Result<(), ScriptError> {
    for expr in exprs {
        if let ExprKind::Define(node) = expr {
            // 先把名字收进 locals 再遍历函数体——顺序不能反：递归函数
            // 的函数体要引用自己的名字（`(define (loop) (loop))`），
            // 若先遍历函数体再收名字，函数体里那次自引用会在名字还没
            // 加入 locals 时就被检查，被误判成白名单外的自由引用。
            //
            // 累加进同一个（可变的）locals，而不是像 Let/Lambda 那样
            // 用一次性克隆——这正是"顶层/begin 序列"与"let/lambda 单个
            // 作用域"两者语义的关键区别。
            collect_bound_names(&node.name, locals);
            walk(&node.body, allowed, locals)?;
        } else {
            walk(expr, allowed, locals)?;
        }
    }
    Ok(())
}

/// 拒绝一个不在白名单内的引用，附带它在源码里的字节偏移量——
/// `steel::parser::ast::Atom`/`Require` 等节点自身携带 `SyntaxObject`，
/// 其中的 `span` 在完整展开后的 AST 上依然可用（实测，见
/// `crates/ll-script/examples/probe_span.rs`），不需要额外基础设施就能
/// 拿到「哪个字节位置」这个信息，换算成行号是调用方（Task 11 加载
/// 管理界面）的事，见 `crate::host::ScriptError` 文档。
fn reject(name: &str, span: Span) -> Result<(), ScriptError> {
    Err(ScriptError::ParseError(
        format!("脚本引用了不在白名单内的标识符「{name}」"),
        Some(span.start()),
    ))
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
    span: Span,
    allowed: &HashSet<&'static str>,
    locals: &HashSet<&'a str>,
) -> Result<(), ScriptError> {
    if locals.contains(name) || allowed.contains(name) {
        Ok(())
    } else {
        reject(name, span)
    }
}

/// `locals` 是**可变**的，不是只读快照——这是能正确处理"顶层/`begin`
/// 序列里，前一条 `define` 要对后一条兄弟表达式可见"这条规则的关键
/// （见 [`walk_sequence`] 文档）。`Let`/`Define`/`LambdaFunction` 各自
/// 需要一个**不泄漏到调用方**的临时作用域时，做法是显式 `clone()` 一份
/// 再传下去，而不是指望这个函数自己去分辨"这次调用该不该泄漏"。
fn walk<'a>(
    expr: &'a ExprKind,
    allowed: &HashSet<&'static str>,
    locals: &mut HashSet<&'a str>,
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
            // body 才看得见新绑定的名字。用克隆的临时作用域：let 引入
            // 的绑定不该泄漏到 let 表达式之外。
            for (_binding_name, value) in &node.bindings {
                walk(value, allowed, locals)?;
            }
            let mut inner = locals.clone();
            for (binding_name, _value) in &node.bindings {
                collect_bound_names(binding_name, &mut inner);
            }
            walk(&node.body_expr, allowed, &mut inner)
        }
        ExprKind::Define(node) => {
            // 单独出现（不在 walk_sequence 序列里）的 define——例如
            // `if`/`let` 分支体本身就是一条孤立 define 的场景。`node.name`
            // 可能是单个原子（`(define x ...)`）也可能是一个列表
            // （`(define (f a b) ...)`，此时首元素是函数名、其余是形参
            // 名）。两者都不需要在白名单里；函数体则需要能看见这些名字
            // （尤其是递归函数要引用自己的名字）。用克隆的临时作用域，
            // 不通过这条路径让定义的名字泄漏出去——真正需要"泄漏给后续
            // 兄弟表达式"的顶层/begin 场景，走的是 walk_sequence 那条
            // 专门路径，不经过这里。
            let mut inner = locals.clone();
            collect_bound_names(&node.name, &mut inner);
            walk(&node.body, allowed, &mut inner)
        }
        ExprKind::LambdaFunction(node) => {
            let mut inner = locals.clone();
            for arg in &node.args {
                collect_bound_names(arg, &mut inner);
            }
            walk(&node.body, allowed, &mut inner)
        }
        ExprKind::Begin(node) => {
            // **不克隆**，直接复用调用方传进来的可变作用域：`begin`
            // 块内先出现的 define 必须对块内后出现的兄弟表达式可见
            // （见 walk_sequence 文档），而"这份新增绑定要不要继续泄漏
            // 到 begin 块之外"完全取决于调用方传进来的 locals 本身是否
            // 可以被继续观察——例如顶层脚本的 begin（`struct` 宏在顶层
            // 展开成的那个 begin）就应该让内部的 define 对同一份顶层
            // locals 生效，因为顶层 begin 在真实 Scheme 语义里本来就是
            // "拼接"进外层作用域,不是新开一层；而嵌套在 lambda 里的
            // begin，因为 lambda 早已经克隆过一份 inner 传进来，begin
            // 往这份 inner 里加的名字自然也只在这个 lambda 内部可见,
            // 出了 lambda 那份 inner 就被丢弃了。两种场景用的是同一行
            // 代码，靠调用链上"谁克隆过、谁没克隆过"自然分流，不需要
            // begin 自己判断"我现在是不是在顶层"。
            walk_sequence(&node.exprs, allowed, locals)
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
        // 展开没有按预期完成，宁可保守拒绝。`node.location` 是这个
        // Require 节点自身的 `SyntaxObject`，同样带 span。
        ExprKind::Require(node) => reject("require", node.location.span),
        // 允许脚本自定义宏（define-syntax/syntax-rules）——这是 Lisp 的
        // 核心能力，不能挡。安全性不靠"不让脚本写宏"，靠"校验的是宏
        // 展开之后的树"：实测（probe_whitelist.rs 第 7 节）宏定义本身
        // 在 emit_fully_expanded_ast 的输出里完全消失，宏的每一次使用
        // 都被替换成它展开出的普通代码——那些代码会照常被这个 walk()
        // 检查，宏本身不提供任何绕过白名单的额外能力。这两个变体在
        // "完整展开后"的树里理论上不应该再出现（宏定义只在展开期起
        // 作用，不留下运行期节点）；万一出现，不检查其内部的模式变量
        // （那些是模式匹配占位符，不是真实引用），直接放行。
        ExprKind::Macro(_) | ExprKind::SyntaxRules(_) => Ok(()),
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
        Some(name) => check_reference(name.resolve(), atom.syn.span, allowed, locals),
        // 非标识符的字面量（数字/布尔/字符串等），不是引用，不检查。
        None => Ok(()),
    }
}

fn walk_list<'a>(
    list: &'a List,
    allowed: &HashSet<&'static str>,
    locals: &mut HashSet<&'a str>,
) -> Result<(), ScriptError> {
    // 首元素是 quote/quasiquote 时整个列表是数据，不检查内部标识符——
    // 这是 `'x` 写法在字面 `(quote x)` 形态下的等价情况，与
    // `ExprKind::Quote` 处理的是同一件事的两种语法糖表示。
    //
    // `` `(a ,(+ 1 2) c) `` 这类带 unquote 的准引用不需要单独处理：
    // 实测（probe_whitelist.rs 第 6 节）编译器在完整展开阶段就已经把
    // 它降级成 `(cons 'a (cons (+ 1 2) (cons 'c '())))`——unquote 里的
    // `(+ 1 2)` 在展开后的树里就是一个普通的、未被 quote 包住的函数
    // 调用，会被下面的递归正常检查到，不会被这条 quote 分支误伤。
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
            Err(ScriptError::ParseError(msg, offset)) => {
                assert!(msg.contains("totally-unknown-function"));
                assert!(offset.is_some(), "违规引用应当携带源码字节偏移量");
            }
            other => panic!("期望 ParseError 且带上违规名字，实际拿到 {other:?}"),
        }
    }
}
