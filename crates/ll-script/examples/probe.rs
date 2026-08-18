//! 一次性探针：核实 `steel-core` 0.8.2 的标准库范围与排除能力。
//!
//! 不是产品代码，跑完即弃——见任务简报「先测再建」。运行方式：
//! `cargo run --example probe -p ll-script --release`
//!
//! 每一节独立 try/catch 风格，输出真实结果而非假设。

use std::process::Command as StdCommand;
use std::time::{Duration, Instant};
use steel::rvals::SteelVal;
use steel::steel_vm::builtin::BuiltInModule;
use steel::steel_vm::engine::Engine;
use steel::steel_vm::interrupt::InterruptHandler;
use steel::steel_vm::register_fn::RegisterFn;

fn section(title: &str) {
    println!("\n=== {title} ===");
}

fn try_run(engine: &mut Engine, label: &str, src: &str) {
    match engine.run(src.to_string()) {
        Ok(vals) => println!("  [OK]  {label} -> {vals:?}"),
        Err(e) => println!("  [ERR] {label} -> {e}"),
    }
}

fn main() {
    section("1. Engine::new() 默认标准库范围——进程");
    {
        let mut engine = Engine::new();
        // steel/process 在 ALL_MODULES 中未加前缀 require-builtin，
        // 理论上 `command`/`spawn-process!` 已直接绑定在全局作用域。
        try_run(
            &mut engine,
            "command 是否已绑定（不 require 直接调用）",
            r#"(command "cmd" (list "/c" "echo" "hi"))"#,
        );
        try_run(
            &mut engine,
            "spawn-process! 是否真的能拉起系统进程",
            r#"
            (define proc (spawn-process (command "cmd" (list "/c" "echo" "probe-ok"))))
            (define out (wait->stdout proc))
            out
            "#,
        );
    }

    section("2. Engine::new_sandboxed() ——进程模块是否也被暴露");
    {
        let mut engine = Engine::new_sandboxed();
        try_run(
            &mut engine,
            "sandboxed 引擎里 command 是否仍已绑定",
            r#"(command "cmd" (list "/c" "echo" "hi"))"#,
        );
        try_run(
            &mut engine,
            "sandboxed 引擎里 spawn-process! 是否仍能真的拉起进程",
            r#"
            (define proc (spawn-process (command "cmd" (list "/c" "echo" "sandbox-probe-ok"))))
            (define out (wait->stdout proc))
            out
            "#,
        );
    }

    section("3. 文件系统——常规引擎 vs sandboxed 引擎");
    {
        let mut engine = Engine::new();
        try_run(
            &mut engine,
            "常规引擎 path-exists? 能否读到真实文件系统",
            r#"(path-exists? "Cargo.toml")"#,
        );

        let mut sandboxed = Engine::new_sandboxed();
        try_run(
            &mut sandboxed,
            "sandboxed 引擎 path-exists? 是否变成未绑定标识符",
            r#"(path-exists? "Cargo.toml")"#,
        );
    }

    section("4. 系统时间/墙钟——即使 sandboxed 也未必排除");
    {
        let mut sandboxed = Engine::new_sandboxed();
        try_run(
            &mut sandboxed,
            "sandboxed 引擎显式 require-builtin steel/time 后能否读墙钟",
            r#"
            (require-builtin steel/time)
            (instant/now)
            "#,
        );
    }

    section("5. 非 DetRng 随机源——即使 sandboxed 也未必排除");
    {
        let mut sandboxed = Engine::new_sandboxed();
        try_run(
            &mut sandboxed,
            "sandboxed 引擎显式 require-builtin steel/random 后能否取 OS 随机数",
            r#"
            (require-builtin steel/random)
            (rng->gen-usize)
            "#,
        );
    }

    section("6. 网络——sandboxed 引擎是否连模块都没注册");
    {
        let mut sandboxed = Engine::new_sandboxed();
        try_run(
            &mut sandboxed,
            "sandboxed 引擎 require-builtin steel/tcp 是否直接失败（模块未注册）",
            r#"(require-builtin steel/tcp)"#,
        );

        let mut regular = Engine::new();
        try_run(
            &mut regular,
            "常规引擎 require-builtin steel/tcp 是否能成功（模块已注册，只是未自动 require）",
            r#"(require-builtin steel/tcp)"#,
        );
    }

    section("7. 跨帧隐式可变状态——线程/互斥量是否默认暴露");
    {
        let mut engine = Engine::new_sandboxed();
        try_run(
            &mut engine,
            "sandboxed 引擎 spawn-native-thread 是否已直接绑定",
            r#"
            (spawn-native-thread (lambda () (displayln "thread ran")))
            #t
            "#,
        );
    }

    section("8. 无序容器迭代顺序——steel 内置 hash 的遍历稳定性");
    {
        let mut engine = Engine::new();
        try_run(
            &mut engine,
            "同一进程内两次构建相同哈希表，keys 顺序是否一致",
            r#"
            (define h1 (hash 'a 1 'b 2 'c 3 'd 4 'e 5))
            (define h2 (hash 'a 1 'b 2 'c 3 'd 4 'e 5))
            (list (equal? (hash-keys->list h1) (hash-keys->list h2))
                  (hash-keys->list h1))
            "#,
        );
    }

    section("9. 缓解手段——能否用公开 API 把已注册模块/绑定重新清空");
    {
        let mut engine = Engine::new_sandboxed();

        // 用空模块覆盖 steel/random、steel/time、steel/threads、steel/process，
        // 阻断脚本之后再显式 require-builtin 这几个模块的路径。
        engine.register_module(BuiltInModule::new("steel/random"));
        engine.register_module(BuiltInModule::new("steel/time"));
        engine.register_module(BuiltInModule::new("steel/threads"));
        engine.register_module(BuiltInModule::new("steel/process"));

        try_run(
            &mut engine,
            "覆盖模块后，再 require-builtin steel/random 是否仍能拿到 rng->gen-usize",
            r#"
            (require-builtin steel/random)
            (rng->gen-usize)
            "#,
        );

        // steel/threads 在构造期已被 ALL_MODULES 无前缀 require 过一次，
        // spawn-native-thread 这个名字已经在全局作用域绑定成真实函数值，
        // 覆盖 module 注册表不会追溯撤销这个已完成的绑定——需要额外验证。
        try_run(
            &mut engine,
            "覆盖 steel/threads 模块后，构造期已绑定的 spawn-native-thread 名字是否仍可调用",
            r#"(spawn-native-thread (lambda () #t))"#,
        );

        // 额外手段：直接用毒值覆盖已经绑定的全局名字。
        engine.register_value("spawn-native-thread", SteelVal::Void);
        try_run(
            &mut engine,
            "额外用 register_value 把 spawn-native-thread 覆盖成 Void 后，再调用是否报错",
            r#"(spawn-native-thread (lambda () #t))"#,
        );
    }

    section("10. 完整跨 VM 调用延迟——不是原子读探针，是真实注册函数往返");
    {
        let mut engine = Engine::new();
        engine.register_fn("probe-add", |a: i64, b: i64| a + b);

        // 预热一次，避开首次调用的编译/解析开销。
        engine.run("(probe-add 1 2)".to_string()).unwrap();

        let iterations = 100_000u32;
        let start = Instant::now();
        for i in 0..iterations {
            let src = format!("(probe-add {i} 1)");
            let _ = engine.run(src).unwrap();
        }
        let elapsed = start.elapsed();
        println!(
            "  每次调用（含 run() 的编译 + 求值 + 返回值解析）均摊耗时：{:?}（共 {} 次，总耗时 {:?}）",
            elapsed / iterations,
            iterations,
            elapsed
        );

        // 用预编译好的 Executable，仅测求值+参数编解码，排除每次都重新解析源码的成本。
        let program = engine.emit_raw_program_no_path("(probe-add 41 1)").unwrap();
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = engine.run_raw_program(program.clone()).unwrap();
        }
        let elapsed = start.elapsed();
        println!(
            "  仅重放已编译字节码（跳过源码解析/编译）均摊耗时：{:?}（共 {} 次，总耗时 {:?}）",
            elapsed / iterations,
            iterations,
            elapsed
        );
    }

    section("11. Engine 求值环境重置/重建的实测开销");
    {
        let build_start = Instant::now();
        let _engine = Engine::new();
        println!("  单次 Engine::new() 耗时：{:?}", build_start.elapsed());

        let iterations = 200u32;
        let start = Instant::now();
        for _ in 0..iterations {
            let _engine = Engine::new();
        }
        let elapsed = start.elapsed();
        println!(
            "  {} 次 Engine::new()（全新重建）均摊耗时：{:?}（总耗时 {:?}）",
            iterations,
            elapsed / iterations,
            elapsed
        );
    }

    section("12. 中断后 Engine 复用——确认 ADR 0001 结论在本地依旧成立");
    {
        let mut engine = Engine::new();
        let handler = InterruptHandler::new(&mut engine, Duration::from_millis(300));
        let res = handler.run_with_timeout(|| {
            engine.run(
                r#"
                (define (loop) (loop))
                (loop)
                "#
                .to_string(),
            )
        });
        println!("  死循环脚本结果：{}", if res.is_err() { "Err（未 panic）" } else { "意外 Ok" });

        let res2 = engine.run("(+ 1 2 3)".to_string());
        println!("  中断后同一 Engine 继续处理下一次调用：{res2:?}");
    }

    section("10b. 完整跨 VM 调用延迟——call_function_by_name_with_args（无重复解析源码）");
    {
        let mut engine = Engine::new();
        engine.register_fn("probe-add2", |a: i64, b: i64| a + b);
        // 预热：让 `probe-add2` 完成一次真正的函数值绑定与首次调用路径。
        engine
            .call_function_by_name_with_args("probe-add2", vec![SteelVal::IntV(1), SteelVal::IntV(2)])
            .unwrap();

        let iterations = 100_000u32;
        let start = Instant::now();
        for i in 0..iterations {
            let args = vec![SteelVal::IntV(i as isize), SteelVal::IntV(1)];
            let _ = engine
                .call_function_by_name_with_args("probe-add2", args)
                .unwrap();
        }
        let elapsed = start.elapsed();
        println!(
            "  call_function_by_name_with_args 均摊耗时：{:?}（共 {} 次，总耗时 {:?}）——这是 host.rs 里 ScriptEngine::call 打算走的真实路径",
            elapsed / iterations,
            iterations,
            elapsed
        );
    }

    section("11b. 显式重置全局绑定（checkpoint/rollback）vs 整引擎重建");
    {
        let mut engine = Engine::new();
        let checkpoint = engine.environment_offset();

        // 模拟脚本运行期间新增了一批全局定义（脚本里的 top-level define）。
        engine
            .run("(define script-global-1 1) (define script-global-2 2)".to_string())
            .unwrap();

        let iterations = 5_000u32;
        let start = Instant::now();
        for _ in 0..iterations {
            engine
                .run("(define script-global-x 1)".to_string())
                .unwrap();
            engine.rollback_to_checkpoint(checkpoint).unwrap();
        }
        let elapsed = start.elapsed();
        println!(
            "  {} 次「新增全局绑定 + rollback_to_checkpoint」均摊耗时：{:?}（总耗时 {:?}）",
            iterations,
            elapsed / iterations,
            elapsed
        );
        println!(
            "  对照：单次 Engine::new() 全量重建耗时约 55ms（见上一节），checkpoint/rollback 是否明显更便宜见上行数字"
        );
    }

    section("10c. call_function_by_name_with_args 套上 InterruptHandler::run_with_timeout 的真实开销");
    {
        // ScriptEngine::call（任务 3 产出）打算给每次调用都包一层中断防线，
        // 上面 10b 测的是裸调用，这里补测「防线包装后」的真实每次调用成本，
        // 不能拿裸调用的 75ns 当作最终结论。
        let mut engine = Engine::new();
        engine.register_fn("probe-add3", |a: i64, b: i64| a + b);
        engine
            .call_function_by_name_with_args("probe-add3", vec![SteelVal::IntV(1), SteelVal::IntV(2)])
            .unwrap();

        let handler = InterruptHandler::new(&mut engine, Duration::from_millis(300));

        let iterations = 100_000u32;
        let start = Instant::now();
        for i in 0..iterations {
            let args = vec![SteelVal::IntV(i as isize), SteelVal::IntV(1)];
            let _ = handler
                .run_with_timeout(|| engine.call_function_by_name_with_args("probe-add3", args))
                .unwrap();
        }
        let elapsed = start.elapsed();
        println!(
            "  包了 InterruptHandler::run_with_timeout 之后，均摊耗时：{:?}（共 {} 次，总耗时 {:?}）",
            elapsed / iterations,
            iterations,
            elapsed
        );
    }

    section("12b. 错误对象是否携带源码位置——决定 log.rs 能上报多少信息");
    {
        let mut engine = Engine::new();
        match engine.run("(+ 1 undefined-identifier)".to_string()) {
            Ok(v) => println!("  意外 Ok：{v:?}"),
            Err(e) => {
                println!("  错误种类：{:?}", e.kind());
                println!("  错误 span：{:?}", e.span());
                println!("  错误 Display：{e}");
            }
        }
    }

    section("13. 佐证——直接用 std::process::Command 验证探针环境本身可拉起子进程");
    {
        let output = StdCommand::new("cmd")
            .args(["/c", "echo", "host-side-ok"])
            .output();
        println!("  宿主侧对照组：{output:?}");
    }
}
