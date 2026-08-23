//! 探针：定位 Steel 引擎构造期的偶发内存破坏（见 ADR 0028）。
//!
//! # 这个探针回答的问题
//!
//! `cargo test -p ll-mod --tests` 有约三分之一的概率被一次进程级崩溃
//! 打断（`STATUS_ACCESS_VIOLATION` / 野指针解引用），崩溃点固定在
//! `Engine::new_sandboxed()` 内部——steel-core 自己的引导编译期，与本
//! 仓库的 mod 脚本无关。本探针把这条路径上的每一个因素拆开单独加压，
//! 用来回答「到底哪一步是触发条件」。
//!
//! 每种模式的实测结果记在 ADR 0028 的实验表里，**不要在这里重复记数字**
//! ——数字会随 steel-core 版本变化，ADR 才是它们的单一来源。
//!
//! # 用法
//!
//! ```text
//! probe_engine_churn <每线程周期数> <模式> <线程栈字节;0=用默认> <线程数> [脚本文件路径]
//! ```
//!
//! 第五个参数给了就用那个文件的内容当被编译的脚本，不给就用内置的
//! [`SCRIPT`]。用来回答「是不是某个特定 `.scm` 每次都炸」——若某个脚本
//! 真的是稳定触发的畸形输入，单独反复编译它应当 100% 失败。
//!
//! 单次进程要么正常打印 `ok`、要么直接崩溃退出（野指针崩溃不会给出
//! `Err`，所以判定标准是**进程退出码**，不是返回值）。统计方式是反复
//! 启动这个进程、数有多少次退出码非零，见 ADR 0028「怎么复现」一节。
//!
//! # 模式
//!
//! 按「是否经过本仓库代码」分成两组，用来区分「上游缺陷」与「我们的
//! 用法有问题」：
//!
//! **纯 steel-core 组**（完全不碰 `ll_script`，可直接作为上游复现用例）：
//! - `pure`：只反复 `Engine::new_sandboxed()`。
//! - `purerun`：构造引擎 + `run()`。
//! - `pureast`：构造引擎 + `emit_fully_expanded_ast()`。
//! - `pureload`：构造引擎 + `emit_fully_expanded_ast()` + `run()`——
//!   这是 [`ll_script::ScriptEngine::load_source`] 真实做的两步（先展开
//!   拿 AST 给白名单看，再真正执行），也是**唯一复现出崩溃**的纯上游组合。
//! - `threadper`：与 `pureload` 同样的工作量，但每个周期独占一根新线程
//!   ——steel-core 的 kernel 镜像是 `thread_local!` 的，换线程等于换一份
//!   全新的 kernel，用来检验「污染是否累积在线程局部 kernel 上」。
//!
//! **经本仓库组**：
//! - `hardened`：反复 [`ll_script::ScriptEngine::new`]（毒化 + 中断看门狗）。
//! - `load`：`ScriptEngine::new()` + `load_source()`，最贴近 `ll-mod`
//!   的 `load_one_script` 真实做的事。
//! - `reuse`：**只构造一次**引擎，然后反复 `load_source()`——用来区分
//!   「反复构造」与「反复编译」哪一个是触发条件。
//!
//! **另有一个容错模式** `tolerant`：与 `pureload` 做同样的事，但**不断言
//! 编译成功**。真实 `.scm` 引用了 `register-race` 这类由宿主注册的函数，
//! 纯 steel-core 编译它必然报 `FreeIdentifier`——那是预期结果，不是缺陷。
//! 这个模式下任何非零退出码都只可能来自 steel-core 自己崩掉或 panic，
//! 于是「逐个脚本单独加压」的统计才有意义。

/// 探针脚本：只用最基本的语言构造，不碰任何被白名单拒绝的能力。
///
/// 内容本身不重要（换成别的脚本一样能复现），重要的是它必须**能编译
/// 通过**——`load`/`reuse` 模式会 `assert!` 加载成功，好让「脚本本身
/// 写错了」不会被误当成崩溃统计进去。
const SCRIPT: &str = r#"
(define (adjust base bonus)
  (let ([total (+ base bonus)])
    (if (> total 10) 10 total)))
(define lookup (hash "a" 1 "b" 2))
(define (pick key) (hash-ref lookup key))
"#;

/// 纯 steel-core 侧的一个周期。见模块文档「纯 steel-core 组」。
fn pure_cycle(mode: &str, script: &str) {
    let mut engine = steel::steel_vm::engine::Engine::new_sandboxed();
    if mode == "pureast" || mode == "pureload" {
        let ast = engine
            .emit_fully_expanded_ast(script, None)
            .expect("探针脚本应当能完成宏展开");
        std::hint::black_box(&ast);
    }
    if mode == "purerun" || mode == "pureload" {
        engine
            .run(script.to_string())
            .expect("探针脚本应当能执行成功");
    }
    std::hint::black_box(&engine);
}

/// 容错版的 `pureload`：见模块文档 `tolerant` 一节，编译失败是预期结果，
/// 只有 steel-core 自己崩掉/panic 才会让进程退出码非零。
fn tolerant_cycle(script: &str) {
    let mut engine = steel::steel_vm::engine::Engine::new_sandboxed();
    if let Ok(ast) = engine.emit_fully_expanded_ast(script, None) {
        std::hint::black_box(&ast);
    }
    let _ = engine.run(script.to_string());
    std::hint::black_box(&engine);
}

/// 经本仓库 [`ll_script::ScriptEngine`] 的一个周期。见模块文档「经本仓库组」。
fn hardened_cycle(mode: &str, script: &str) {
    let mut engine = ll_script::ScriptEngine::new();
    if mode == "load" {
        let result = engine.load_source(script.to_string());
        assert!(result.is_ok(), "探针脚本应当加载成功：{result:?}");
    }
    std::hint::black_box(&engine);
}

fn run_mode(mode: &str, cycles: usize, script: &str) {
    // reuse 与 threadper 的循环结构与其余模式不同，各自单独成一支。
    if mode == "reuse" {
        let mut engine = ll_script::ScriptEngine::new();
        for _ in 0..cycles {
            let result = engine.load_source(script.to_string());
            assert!(result.is_ok(), "探针脚本应当加载成功：{result:?}");
        }
        return;
    }
    if mode == "threadper" {
        for _ in 0..cycles {
            let owned = script.to_string();
            std::thread::spawn(move || pure_cycle("pureload", &owned))
                .join()
                .expect("探针线程不应 panic");
        }
        return;
    }
    for _ in 0..cycles {
        match mode {
            "pure" | "purerun" | "pureast" | "pureload" => pure_cycle(mode, script),
            "tolerant" => tolerant_cycle(script),
            "hardened" | "load" => hardened_cycle(mode, script),
            other => panic!("未知模式：{other}"),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cycles: usize = args
        .get(1)
        .map(|s| s.parse().expect("周期数必须是整数"))
        .unwrap_or(60);
    let mode = args.get(2).cloned().unwrap_or_else(|| "pureload".into());
    let stack_bytes: usize = args
        .get(3)
        .map(|s| s.parse().expect("栈字节数必须是整数"))
        .unwrap_or(0);
    let threads: usize = args
        .get(4)
        .map(|s| s.parse().expect("线程数必须是整数"))
        .unwrap_or(1);

    let script = match args.get(5) {
        Some(path) => std::fs::read_to_string(path).expect("脚本文件应当可读"),
        None => SCRIPT.to_string(),
    };

    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let mode = mode.clone();
            let script = script.clone();
            let mut builder = std::thread::Builder::new();
            if stack_bytes > 0 {
                builder = builder.stack_size(stack_bytes);
            }
            builder
                .spawn(move || run_mode(&mode, cycles, &script))
                .expect("探针线程应当能创建")
        })
        .collect();
    for handle in handles {
        handle.join().expect("探针线程不应 panic");
    }
    println!("ok");
}
