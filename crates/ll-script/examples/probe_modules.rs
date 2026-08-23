//! 探针：Steel 源码模块（`require` + `provide`）在本项目沙箱下的真实行为。
//!
//! 要回答的问题：
//! 1. `SourceModuleResolver::resolve` 供源的模块，`require` 能不能用、
//!    `provide` 的名字是不是唯一可见的那批？
//! 2. **要求方脚本**的 `emit_fully_expanded_ast` 里出现哪些标识符——
//!    白名单能不能看见模块体？（决定模块体要不要单独过白名单）
//! 3. 模块体里的 `require-builtin steel/time` 会不会绕过白名单？
//! 4. 环 import 是报错还是挂死？
//! 5. 未 provide 的名字拿不到时报什么错？

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use steel::compiler::modules::SourceModuleResolver;
use steel::steel_vm::engine::Engine;

struct TableResolver {
    table: HashMap<String, String>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl SourceModuleResolver for TableResolver {
    fn resolve(&self, key: &str) -> Option<String> {
        self.asked.lock().unwrap().push(format!("resolve:{key}"));
        self.table.get(key).cloned()
    }
    fn exists(&self, key: &str) -> bool {
        self.asked.lock().unwrap().push(format!("exists:{key}"));
        true
    }
}

fn engine_with(table: &[(&str, &str)]) -> (Engine, Arc<Mutex<Vec<String>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut engine = Engine::new_sandboxed();
    engine.register_source_module_resolver(TableResolver {
        table: table
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        asked: Arc::clone(&asked),
    });
    (engine, asked)
}

fn section(title: &str) {
    println!("\n=== {title} ===");
}

fn show_ast(engine: &mut Engine, source: &str) {
    match engine.emit_fully_expanded_ast(source, None) {
        Ok(exprs) => {
            println!("  顶层节点数：{}", exprs.len());
            for expr in &exprs {
                let text = expr.to_pretty(100);
                let text = if text.len() > 2000 {
                    format!("{}…（截断，共 {} 字节）", &text[..2000], text.len())
                } else {
                    text
                };
                println!("  --- {text}");
            }
        }
        Err(e) => println!("  展开出错：{e}"),
    }
}

fn main() {
    section("1. 基本 require/provide 能不能用");
    {
        let (mut engine, asked) = engine_with(&[(
            "lostland:helpers",
            "(provide double) (define (double x) (* 2 x)) (define (secret) 42)",
        )]);
        match engine.run(r#"(require "lostland:helpers") (double 21)"#.to_string()) {
            Ok(v) => println!("  结果：{v:?}"),
            Err(e) => println!("  出错：{e}"),
        }
        println!("  resolver 被问过：{:?}", asked.lock().unwrap());
    }

    section("2. 未 provide 的名字");
    {
        let (mut engine, _) = engine_with(&[(
            "lostland:helpers",
            "(provide double) (define (double x) (* 2 x)) (define (secret) 42)",
        )]);
        match engine.run(r#"(require "lostland:helpers") (secret)"#.to_string()) {
            Ok(v) => println!("  结果（不该成功）：{v:?}"),
            Err(e) => println!("  出错（预期）：{e}"),
        }
    }

    section("3. 完全没有 provide 的模块");
    {
        let (mut engine, _) = engine_with(&[("m:none", "(define (f) 1)")]);
        match engine.run(r#"(require "m:none") (f)"#.to_string()) {
            Ok(v) => println!("  结果（不该成功）：{v:?}"),
            Err(e) => println!("  出错（预期）：{e}"),
        }
    }

    section("4. 要求方脚本的完整展开 AST 长什么样");
    {
        let (mut engine, _) = engine_with(&[(
            "lostland:helpers",
            "(provide double) (define (double x) (* 2 x))",
        )]);
        show_ast(&mut engine, r#"(require "lostland:helpers") (double 21)"#);
    }

    section("5. 模块体里的 require-builtin steel/time 会不会穿过来");
    {
        let (mut engine, _) = engine_with(&[(
            "evil:mod",
            "(require-builtin steel/time) (provide now) (define (now) (instant/now))",
        )]);
        show_ast(&mut engine, r#"(require "evil:mod") (now)"#);
        let (mut engine2, _) = engine_with(&[(
            "evil:mod",
            "(require-builtin steel/time) (provide now) (define (now) (instant/now))",
        )]);
        match engine2.run(r#"(require "evil:mod") (now)"#.to_string()) {
            Ok(v) => println!("  运行结果（说明模块体绕过了白名单）：{v:?}"),
            Err(e) => println!("  运行出错：{e}"),
        }
    }

    section("6. 环 import");
    {
        let (mut engine, _) = engine_with(&[
            ("a", r#"(require "b") (provide fa) (define (fa) 1)"#),
            ("b", r#"(require "a") (provide fb) (define (fb) 2)"#),
        ]);
        match engine.run(r#"(require "a") (fa)"#.to_string()) {
            Ok(v) => println!("  结果：{v:?}"),
            Err(e) => println!("  出错：{e}"),
        }
    }

    section("7. resolve 返回 None 时的错误文本");
    {
        let (mut engine, _) = engine_with(&[]);
        match engine.run(r#"(require "C:/Windows/win.ini")"#.to_string()) {
            Ok(v) => println!("  结果（不该成功）：{v:?}"),
            Err(e) => println!("  出错（预期）：{e}"),
        }
    }

    section("8. 跨模块传递性：a require b，主脚本 require a，能不能看见 b 的导出");
    {
        let (mut engine, _) = engine_with(&[
            ("a", r#"(require "b") (provide fa) (define (fa) (fb))"#),
            ("b", r#"(provide fb) (define (fb) 7)"#),
        ]);
        match engine.run(r#"(require "a") (fb)"#.to_string()) {
            Ok(v) => println!("  fb 可见（不该）：{v:?}"),
            Err(e) => println!("  fb 不可见（预期）：{e}"),
        }
        let (mut engine2, _) = engine_with(&[
            ("a", r#"(require "b") (provide fa) (define (fa) (fb))"#),
            ("b", r#"(provide fb) (define (fb) 7)"#),
        ]);
        match engine2.run(r#"(require "a") (fa)"#.to_string()) {
            Ok(v) => println!("  fa 可用：{v:?}"),
            Err(e) => println!("  fa 出错：{e}"),
        }
    }

    section("9. 宿主注册的 Rust 函数在模块体里可不可见");
    {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let mut engine = Engine::new_sandboxed();
        engine.register_source_module_resolver(TableResolver {
            table: [(
                "m:x".to_string(),
                "(provide use-host) (define (use-host) (host-add 1 2))".to_string(),
            )]
            .into_iter()
            .collect(),
            asked: Arc::clone(&asked),
        });
        use steel::steel_vm::register_fn::RegisterFn;
        engine.register_fn("host-add", |a: i64, b: i64| a + b);
        match engine.run(r#"(require "m:x") (use-host)"#.to_string()) {
            Ok(v) => println!("  结果：{v:?}"),
            Err(e) => println!("  出错：{e}"),
        }
    }
    section("10. 模块展开产物的名字在不在 globals()");
    probe_globals();

    section("11. 模块体副作用的次数与顺序");
    probe_side_effects();

    section("12. emit 之后再 run");
    probe_emit_then_run();

    section("13. 只 emit 的引擎的符号可见性");
    probe_emit_only_symbol_visibility();
}

/// 附加：模块展开产物用到的那几个名字在不在 `Engine::globals()` 里。
/// 由 main 末尾调用（见文件底部）。
pub fn probe_globals() {
    let engine = Engine::new_sandboxed();
    let names: std::collections::HashSet<String> = engine
        .globals()
        .iter()
        .map(|i| i.resolve().to_string())
        .collect();
    for probe in [
        "#%prim.#%push-module-context",
        "#%prim.#%pop-module-context",
        "#%push-module-context",
        "#%pop-module-context",
        "%proto-hash%",
        "%proto-hash-get%",
        "#%void",
        "%module-get%",
        "%-builtin-module-steel/time",
        "%-builtin-module-steel/process",
    ] {
        println!("  {probe}: {}", names.contains(probe));
    }
}

/// 附加：模块体的副作用发生几次、按什么顺序；跨两次 load_source 的缓存行为。
pub fn probe_side_effects() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use steel::steel_vm::register_fn::RegisterFn;
    static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static N: AtomicUsize = AtomicUsize::new(0);

    let table = [
        (
            "crafting",
            r#"(provide 类别) (define 类别 1) (mark "crafting")"#,
        ),
        (
            "subclasses",
            r#"(require "crafting") (provide 副职) (define 副职 类别) (mark "subclasses")"#,
        ),
        ("tags", r#"(mark "tags")"#),
    ];
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut engine = Engine::new_sandboxed();
    engine.register_source_module_resolver(TableResolver {
        table: table
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        asked: Arc::clone(&asked),
    });
    engine.register_fn("mark", |s: String| {
        let i = N.fetch_add(1, Ordering::SeqCst);
        LOG.lock().unwrap().push(format!("{i}:{s}"));
    });

    // main.scm 故意把 subclasses 写在 crafting 前面——若 require 图生效，
    // crafting 仍然必须先跑。
    let main = r#"(require "subclasses") (require "crafting") (require "tags") 副职"#;
    match engine.run(main.to_string()) {
        Ok(v) => println!("  第一次 run：{v:?}"),
        Err(e) => println!("  第一次 run 出错：{e}"),
    }
    println!("  副作用顺序：{:?}", LOG.lock().unwrap());

    // 同一个引擎第二份脚本再 require 同一批模块：副作用应当不再发生。
    match engine.run(r#"(require "crafting") 类别"#.to_string()) {
        Ok(v) => println!("  第二次 run：{v:?}"),
        Err(e) => println!("  第二次 run 出错：{e}"),
    }
    println!("  副作用顺序（第二次之后）：{:?}", LOG.lock().unwrap());
    println!("  resolver 被问过：{:?}", asked.lock().unwrap());
}

/// 附加：同一个引擎上「先 emit_fully_expanded_ast 再 run」会怎样。
pub fn probe_emit_then_run() {
    let (mut engine, _) =
        engine_with(&[("helpers", "(provide double) (define (double x) (* 2 x))")]);
    let src = r#"(require "helpers") (double 21)"#;
    println!("  第一次 emit：");
    show_ast(&mut engine, src);
    println!("  第二次 emit（缓存已热）：");
    show_ast(&mut engine, src);
    match engine.run(src.to_string()) {
        Ok(v) => println!("  run：{v:?}"),
        Err(e) => println!("  run 出错：{e}"),
    }

    // 对照：先 run 再 emit
    let (mut engine2, _) =
        engine_with(&[("helpers", "(provide double) (define (double x) (* 2 x))")]);
    match engine2.run(src.to_string()) {
        Ok(v) => println!("  先 run：{v:?}"),
        Err(e) => println!("  先 run 出错：{e}"),
    }
    match engine2.run(src.to_string()) {
        Ok(v) => println!("  再 run 一次：{v:?}"),
        Err(e) => println!("  再 run 出错：{e}"),
    }
}

/// 附加：只 emit 不 run 的引擎，后一份脚本能不能看见前一份的顶层 define
/// ——决定「校验引擎 + 执行引擎」两台分工可不可行。
pub fn probe_emit_only_symbol_visibility() {
    let (mut engine, _) = engine_with(&[]);
    match engine.emit_fully_expanded_ast("(define (f) 1)", None) {
        Ok(_) => println!("  第一份 emit 成功"),
        Err(e) => println!("  第一份 emit 出错：{e}"),
    }
    match engine.emit_fully_expanded_ast("(f)", None) {
        Ok(exprs) => println!("  第二份 emit 成功，节点数 {}", exprs.len()),
        Err(e) => println!("  第二份 emit 出错：{e}"),
    }
    // 宏呢？
    match engine.emit_fully_expanded_ast(
        "(define-syntax 翻倍 (syntax-rules () ((_ x) (* 2 x))))",
        None,
    ) {
        Ok(_) => println!("  宏定义 emit 成功"),
        Err(e) => println!("  宏定义 emit 出错：{e}"),
    }
    match engine.emit_fully_expanded_ast("(翻倍 21)", None) {
        Ok(exprs) => println!(
            "  用宏 emit 成功：{}",
            exprs.first().map(|e| e.to_pretty(60)).unwrap_or_default()
        ),
        Err(e) => println!("  用宏 emit 出错：{e}"),
    }
    // 未定义的名字呢？
    match engine.emit_fully_expanded_ast("(根本没定义过)", None) {
        Ok(_) => println!("  未定义名字 emit 竟然成功（说明 emit 不查自由标识符）"),
        Err(e) => println!("  未定义名字 emit 出错：{e}"),
    }
}
