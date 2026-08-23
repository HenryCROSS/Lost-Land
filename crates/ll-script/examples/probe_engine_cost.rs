//! 引擎构造成本探针：量「造一个 `ScriptEngine` 多贵」——耗时与内存。
//!
//! **这是复现/测量工具，不是产品代码**，与 `probe_engine_churn.rs`
//! （ADR 0028）、ADR 0012 的 `probe.rs` 同一个定位。
//!
//! # 为什么需要这个数
//!
//! 约束 C6（见规格 §4 与 [ADR 0029]）要求「同一根线程上全部引擎构造
//! 先于全部脚本编译」，于是装载期的 N 个引擎会**同时活着**，而不是像
//! 以前那样一次只有一个。「引擎粒度按 mod（4 个）还是按脚本文件
//! （11 个）」这个取舍因此从「有没有崩溃风险」变成了纯粹的
//! 「启动耗时 + 装载期内存峰值 对 文件级隔离」——而那两个数此前没人
//! 量过。
//!
//! [ADR 0029]: ../../../knowledge/decisions/0029-engine-construction-phase-precedes-compilation.md
//!
//! # 跑法
//!
//! ```text
//! cargo run --release --example probe_engine_cost
//! ```
//!
//! debug 构建的绝对数字会明显偏大（steel-core 自身也是 debug），量级
//! 对比仍然成立；两种都跑一遍最稳妥。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use ll_script::host::ScriptEngine;

/// 进程内当前存活的堆字节数与历史峰值——只做计数，分配本身仍然交给
/// `System`。
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: 全部分配/释放原样转发给 `System`，只在旁边记两个原子计数器，
// 不改变任何内存语义。
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn live_mib() -> f64 {
    LIVE.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0)
}

/// 一个最小但真实的装载期脚本：够触发一次完整的
/// 解析 → 完整展开 → 白名单校验 → 求值。
const SAMPLE: &str = "(define (helper x) (* x 2)) (helper 21)";

fn main() {
    println!("== 一、主线程首次构造（含该线程的内核 bootstrap）==");
    let before = live_mib();
    let t0 = Instant::now();
    let first = ScriptEngine::new();
    let first_cost = t0.elapsed();
    let after_first = live_mib();
    println!("  首次 ScriptEngine::new()  {first_cost:?}");
    println!(
        "  存活堆 {before:.2} MiB -> {after_first:.2} MiB（差 {:.2} MiB）",
        after_first - before
    );

    println!();
    println!("== 二、同一线程上后续构造（内核已 bootstrap，只剩 deep_clone）==");
    let mut engines = vec![first];
    let mut costs = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        let engine = ScriptEngine::new();
        costs.push(t.elapsed());
        engines.push(engine);
    }
    let total: std::time::Duration = costs.iter().sum();
    let each = total / costs.len() as u32;
    println!("  后续 10 次合计 {total:?}，平均每次 {each:?}");
    println!(
        "  最快 {:?}，最慢 {:?}",
        costs.iter().min().unwrap(),
        costs.iter().max().unwrap()
    );
    let after_eleven = live_mib();
    println!(
        "  11 个引擎同时活着：存活堆 {after_eleven:.2} MiB（较构造前 +{:.2} MiB，每个引擎约 {:.2} MiB）",
        after_eleven - before,
        (after_eleven - before) / 11.0
    );

    println!();
    println!("== 三、另一根线程的首次构造（内核 bootstrap 是每线程一次）==");
    let cross = std::thread::spawn(|| {
        let t = Instant::now();
        let engine = ScriptEngine::new();
        let cost = t.elapsed();
        drop(engine);
        cost
    })
    .join()
    .expect("探针线程不应 panic");
    println!("  新线程首次 ScriptEngine::new()  {cross:?}");

    println!();
    println!("== 四、编译之后能不能立刻丢弃引擎 ==");
    // 约束 C6 禁的是「编译之后再构造」，析构不受限制。这里逐个编译、
    // 逐个丢弃，观察存活堆能不能退回去。
    let mut compiled = 0usize;
    for mut engine in engines.drain(..) {
        engine
            .load_source(SAMPLE.to_string())
            .expect("样例脚本应当能装载");
        compiled += 1;
        drop(engine);
    }
    let after_drop = live_mib();
    println!("  编译并丢弃 {compiled} 个引擎后：存活堆 {after_drop:.2} MiB");
    println!(
        "  较 11 个同时活着时回落 {:.2} MiB；较最初多出 {:.2} MiB",
        after_eleven - after_drop,
        after_drop - before
    );
    println!(
        "  （多出的部分含 ADR 0028 记过的两处已知泄漏：\
         compute_allowed_identifiers 的 Box::leak，与 steel-core \
         InterruptHandler::drop 不 join 的看门狗线程）"
    );

    println!();
    println!("== 五、按上面的数推算：4 个引擎（每 mod）vs 11 个（每脚本）==");
    let per_engine_time = each;
    let per_engine_mib = (after_eleven - before) / 11.0;
    println!(
        "  启动耗时差：(11-4) x {per_engine_time:?} = {:?}",
        per_engine_time * 7
    );
    println!(
        "  装载期内存峰值差：(11-4) x {per_engine_mib:.2} MiB = {:.2} MiB",
        per_engine_mib * 7.0
    );
    println!(
        "  历史峰值 {:.2} MiB",
        PEAK.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0)
    );
}
