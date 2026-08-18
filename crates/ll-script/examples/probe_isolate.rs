//! 隔离复现：为什么覆盖 steel/time 模块挡不住 instant/now，而覆盖
//! steel/random 能挡住 rng->gen-usize？两边用完全相同的手法，各自独立
//! 一个全新进程/全新引擎，排除任何跨引擎、跨进程的干扰。

use steel::steel_vm::builtin::BuiltInModule;
use steel::steel_vm::engine::Engine;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();

    match arg.as_str() {
        "time" => {
            let mut engine = Engine::new_sandboxed();
            engine.register_module(BuiltInModule::new("steel/time"));
            let names = engine.builtin_modules().get("steel/time").unwrap().names();
            println!("steel/time 覆盖后导出数量：{}", names.len());
            let result = engine.run("(require-builtin steel/time) (instant/now)".to_string());
            println!("time 结果：{result:?}");
        }
        "random" => {
            let mut engine = Engine::new_sandboxed();
            engine.register_module(BuiltInModule::new("steel/random"));
            let names = engine
                .builtin_modules()
                .get("steel/random")
                .unwrap()
                .names();
            println!("steel/random 覆盖后导出数量：{}", names.len());
            let result = engine.run("(require-builtin steel/random) (rng->gen-usize)".to_string());
            println!("random 结果：{result:?}");
        }
        "meta" => {
            let engine = Engine::new_sandboxed();
            let names = engine.builtin_modules().get("steel/meta").unwrap().names();
            println!("steel/meta 导出数量：{}", names.len());
            for n in &names {
                println!("  {n}");
            }
        }
        "process-names" => {
            let engine = Engine::new_sandboxed();
            let names = engine
                .builtin_modules()
                .get("steel/process")
                .unwrap()
                .names();
            println!("steel/process 导出数量：{}", names.len());
            for n in &names {
                println!("  {n}");
            }
        }
        "threads-names" => {
            let engine = Engine::new_sandboxed();
            let names = engine
                .builtin_modules()
                .get("steel/threads")
                .unwrap()
                .names();
            println!("steel/threads 导出数量：{}", names.len());
            for n in &names {
                println!("  {n}");
            }
        }
        "meta-poison-test" => {
            // 验证「枚举 steel/meta 全部导出名字并逐个用毒值覆盖」这个手法
            // 是否真的挡得住 eval!/run!/Engine::new 这类逃逸入口。
            let mut engine = Engine::new_sandboxed();
            let names = engine.builtin_modules().get("steel/meta").unwrap().names();
            for name in names {
                let leaked: &'static str = Box::leak(name.into_boxed_str());
                engine.register_value(leaked, steel::rvals::SteelVal::Void);
            }
            let result = engine.run(r#"(eval! "(+ 1 2)")"#.to_string());
            println!("eval! 覆盖后调用结果：{result:?}");
            let result2 = engine.run(r#"(run! (Engine::new) "(+ 1 2)")"#.to_string());
            println!("run!/Engine::new 覆盖后调用结果：{result2:?}");
        }
        other => println!("未知参数：{other}"),
    }
}
