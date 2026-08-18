//! 探针：验证「语法树层面白名单」这条路是否可行。
//!
//! 三个可行性问题：
//! 1. steel-core 0.8.2 是否暴露「先解析后求值」的 API？
//! 2. 宏展开会不会绕过白名单（校验必须在展开之后）？
//! 3. 运行时能否构造出调用（字符串拼符号再求值）绕过白名单？

use steel::steel_vm::engine::Engine;

fn section(title: &str) {
    println!("\n=== {title} ===");
}

fn main() {
    section("1. emit_fully_expanded_ast 能否拿到 require-builtin 展开后的 AST");
    {
        let mut engine = Engine::new_sandboxed();
        match engine.emit_fully_expanded_ast("(require-builtin steel/time) (instant/now)", None) {
            Ok(exprs) => {
                for expr in &exprs {
                    println!("  ---节点---");
                    println!("{}", expr.to_pretty(80));
                }
            }
            Err(e) => println!("  emit_fully_expanded_ast 出错：{e}"),
        }
    }

    section("2. 未经 require 的裸符号 instant/now 展开后长什么样");
    {
        let mut engine = Engine::new_sandboxed();
        match engine.emit_fully_expanded_ast("(instant/now)", None) {
            Ok(exprs) => {
                for expr in &exprs {
                    println!("{}", expr.to_pretty(80));
                }
            }
            Err(e) => println!("  出错（预期，因为没 require）：{e}"),
        }
    }

    section("3. eval! 是否在展开后的 AST 里以可识别的名字出现");
    {
        let mut engine = Engine::new_sandboxed();
        match engine.emit_fully_expanded_ast(r#"(eval! "(+ 1 2)")"#, None) {
            Ok(exprs) => {
                for expr in &exprs {
                    println!("{}", expr.to_pretty(80));
                }
            }
            Err(e) => println!("  出错：{e}"),
        }
    }

    section("4. 字符串拼符号再求值——试图绕过静态白名单");
    {
        // 若 eval! 本身不在白名单里，这条路应该在「eval! 这个名字本身
        // 出现在 AST 里」这一步就被挡下,不需要真的执行到这里。但这里
        // 仍然实测一次「假设白名单漏掉了 eval!」的最坏情况,验证 eval!
        // 内部动态构造的代码是否绕开了外层脚本静态文本里没写
        // "require-builtin" 这件事。
        let mut engine = Engine::new_sandboxed();
        let src = r#"
        (define parts (list "require" "-" "builtin"))
        (define word (string->symbol (apply string-append parts)))
        (displayln word)
        "#;
        match engine.run(src.to_string()) {
            Ok(v) => println!("  字符串拼接构造出 require-builtin 符号：{v:?}"),
            Err(e) => println!("  字符串拼接失败：{e}"),
        }
    }

    section("5b. 普通计算脚本 fully-expanded 之后长什么样");
    {
        let mut engine = Engine::new_sandboxed();
        match engine.emit_fully_expanded_ast("(define (add a b) (+ a b)) (add 1 2)", None) {
            Ok(exprs) => {
                for expr in &exprs {
                    println!("  ---节点---");
                    println!("{}", expr.to_pretty(80));
                }
            }
            Err(e) => println!("  出错：{e}"),
        }
    }

    section("5c. 先 emit_fully_expanded_ast 校验，再对同一份源码 run，是否安全");
    {
        let mut engine = Engine::new_sandboxed();
        let src = "(define (add a b) (+ a b)) (add 1 2)";
        let _exprs = engine.emit_fully_expanded_ast(src, None).unwrap();
        let result = engine.run(src.to_string());
        println!("  先 expand 再 run 同一份源码：{result:?}");

        // 再跑一次，确认重复 define 不会累积出错。
        let result2 = engine.run(src.to_string());
        println!("  同一个 engine 上再跑一次：{result2:?}");
    }

    section("5. 顶层 (require-builtin ...) 在 fully-expanded AST 里的原始形状（未展开对照组）");
    {
        match Engine::emit_ast_to_string("(require-builtin steel/time)") {
            Ok(s) => println!("{s}"),
            Err(e) => println!("  出错：{e}"),
        }
    }
}
