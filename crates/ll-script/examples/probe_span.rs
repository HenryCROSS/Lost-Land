//! 探针：验证 Steel 的错误对象是否携带源码位置（`SteelErr::span()`），
//! 供 Task 11（加载管理界面）判断错误详情能显示到什么粒度——行号级别
//! 还是只能到"哪个文件加载失败"。用完即扔，不是长期维护的代码。

use steel::steel_vm::engine::Engine;
use steel::steel_vm::register_fn::RegisterFn;

fn line_of(source: &str, byte_offset: u32) -> usize {
    source[..byte_offset as usize].matches('\n').count() + 1
}

fn main() {
    println!("=== 1. 语法错误（缺右括号）经 emit_fully_expanded_ast ===");
    {
        let mut engine = Engine::new_sandboxed();
        let source = "(define x 1)\n(+ 1 2";
        match engine.emit_fully_expanded_ast(source, None) {
            Ok(_) => println!("  意外成功"),
            Err(e) => {
                println!("  错误: {e}");
                println!("  kind: {:?}", e.kind());
                println!("  span: {:?}", e.span());
                if let Some(span) = e.span() {
                    println!("  推算行号: {}", line_of(source, span.start()));
                }
            }
        }
    }

    println!("\n=== 2. 运行时错误（调用未定义函数）经 engine.run ===");
    {
        let mut engine = Engine::new_sandboxed();
        let source = "(define x 1)\n(some-undefined-function x)";
        match engine.run(source) {
            Ok(_) => println!("  意外成功"),
            Err(e) => {
                println!("  错误: {e}");
                println!("  kind: {:?}", e.kind());
                println!("  span: {:?}", e.span());
                if let Some(span) = e.span() {
                    println!("  推算行号: {}", line_of(source, span.start()));
                }
            }
        }
    }

    println!("\n=== 3. arity 不匹配经 engine.run ===");
    {
        let mut engine = Engine::new_sandboxed();
        engine.register_fn("needs-two", |a: i64, b: i64| a + b);
        let source = "(define y 2)\n(needs-two 1)";
        match engine.run(source) {
            Ok(_) => println!("  意外成功"),
            Err(e) => {
                println!("  错误: {e}");
                println!("  kind: {:?}", e.kind());
                println!("  span: {:?}", e.span());
                if let Some(span) = e.span() {
                    println!("  推算行号: {}", line_of(source, span.start()));
                }
            }
        }
    }

    println!("\n=== 4. 白名单拒绝场景：require-builtin steel/time 展开后 ===");
    {
        let mut engine = Engine::new_sandboxed();
        let source = "(require-builtin steel/time)\n(instant/now)";
        // 白名单拒绝不是 SteelErr，是 ll-script 自己的 ScriptError，
        // 这里只探测 emit_fully_expanded_ast 本身在这个场景下是否成功
        // （预期成功——展开没有语法错误，白名单是在展开成功之后另外
        // 检查的，见 whitelist.rs），确认「白名单拒绝」这类错误天生就
        // 没有 SteelErr::span 可用，只能靠 whitelist.rs 自己在遍历 AST
        // 时记录被拒绝节点的位置（如果 AST 节点本身携带 span）。
        match engine.emit_fully_expanded_ast(source, None) {
            Ok(exprs) => {
                println!("  展开成功，{} 个顶层节点", exprs.len());
                for expr in &exprs {
                    println!("  节点: {}", expr.to_pretty(80));
                }
            }
            Err(e) => println!("  展开失败: {e}"),
        }
    }
}
