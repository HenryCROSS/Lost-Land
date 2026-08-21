//! 内存执行预算的端到端验证：真的装上 `#[global_allocator]`。
//!
//! # 为什么单独放一个集成测试文件，而不是塞进 `src/alloc_guard.rs`
//!
//! `alloc_guard.rs` 单元测试里的三条用例只是直接调用
//! `ScriptAllocGuard::alloc`/`dealloc`，从没有真的把它装成进程的
//! `#[global_allocator]`——见该文件模块文档：装上之后，*整个测试二进制*
//! 里所有测试线程的所有分配都会经过这份记账逻辑。
//!
//! `cargo test` 里 `tests/` 目录下每一个 `.rs` 文件都会被编译成**独立的
//! 二进制、独立的进程**，互不共享内存空间。把 `#[global_allocator]` 装
//! 在这一个文件里，只会影响这一个进程——`src/` 下的库单元测试（另一个
//! 二进制）、`api/*.rs` 里那几十个测试调用点，都在别的进程里跑，完全不
//! 受影响。这是「不需要靠 `TEST_SERIAL` 之类的机制手动隔离」的真正原因：
//! 操作系统的进程边界本身就是隔离边界，比任何进程内的锁/线程局部变量
//! 都更彻底。
//!
//! 本文件因此是本项目里唯一一处「内存执行预算」这道防线处于真正生效
//! 状态的地方——生产二进制（未来的游戏本体）想要这道防线生效，同样
//! 需要在自己的 `main.rs` 里装一次 `#[global_allocator]`，见
//! `ScriptAllocGuard` 文档。
//!
//! 同一个理由让本文件成为「内存超预算」与「执行超时」这两种中断能否
//! 被正确区分（`ScriptError::MemoryBudgetExceeded`/`ScriptError::Timeout`，
//! 见 `host.rs::classify_error` 文档）的唯一权威验证场所：只有这里的
//! 内存记账是真实生效的，别处的测试二进制里内存预算永远不会被真正
//! 触发，「预算超限的错误确实归类成了那个变体」这件事只能在这里断言。

use std::alloc::System;

use ll_script::alloc_guard::set_memory_budget;
use ll_script::{ScriptAllocGuard, ScriptEngine};

/// 本进程的全局分配器：真实系统分配器外面套一层记账。装上之后，本文件
/// 里从 `ScriptEngine::new()` 开始的每一次分配都会被 `alloc_guard` 计入
/// 线程局部计数器——包括引擎构造本身，这正是 `set_memory_budget` 默认
/// `usize::MAX` 的原因（构造期的分配不该被算作"脚本超预算"）。
#[global_allocator]
static ALLOC: ScriptAllocGuard<System> = ScriptAllocGuard(System);

/// 一段会不停累积分配的脚本：递归构造一个长链表，每一步 `cons` 一次。
///
/// # 为什么是两万层，不是最初版本的五百万层
///
/// 最初版本用 5,000,000 层，实测在 `cargo test --workspace` 并行跑时
/// 偶发让下面「预算充足」那条测试失败——排查发现根因不是内存记账算错，
/// 而是这个深度本身选得太大：`ScriptEngine`（见 `host.rs`）每次调用都套了
/// 一层 300ms 的 `InterruptHandler` 超时。ADR 0001 实测非尾递归 1,000,000
/// 层约 59ms、10,000,000 层约 644ms，按此换算，5,000,000 层在**完全没有
/// 并行负载**时就已经逼近 300ms 这条线；一旦 `cargo test --workspace`
/// 把其余测试二进制、其余 crate 的编译一起挤上 CPU，「预算充足」那条
/// 调用真正等到的常常是 300ms 超时打断，而不是跑到底——它的名字承诺的
/// 是"验证预算机制不会误伤"，实际却在赌调度器是否来得及在 300ms 内把
/// 这次调用跑完，两件事完全不是一回事。
///
/// **`ScriptError` 拆分之后，这个坑现在能被直接看见了**（见
/// `host.rs::classify_error` 文档「`interrupt()` 通道的两个调用点」一
/// 节——早年内存预算与执行超时共用同一个 `ScriptError::Interrupted`
/// 变体时，「预算充足」这条测试即使真的撞上 300ms 超时，拿到的错误也
/// 和"预算真的超了"长得一模一样，只能靠 `is_err()`/`is_ok()` 这种粗
/// 粒度断言，看不出真相；现在撞上超时会明确拿到
/// `ScriptError::Timeout`，不会被误当成 `MemoryBudgetExceeded`）。但
/// **拆分本身不能替代把深度选对**：拆分只是让"分类对不对"这件事本身
/// 可验证了，不代表可以放任一条名为"预算充足"的测试真的去赌 300ms
/// 超时——即使分类分对了，`assert!(result.is_ok())` 依然会因为拿到
/// `Err(Timeout)` 而失败，测试红了但原因和"预算机制"毫无关系。两万层
/// 这个选择本身仍然是必要的（不是被拆分取代的旧措施）：下面 4KB 预算
/// 的用例仍然会在最初几十次 `cons`（远小于两万层）内触发预算中断；
/// `usize::MAX` 预算的用例则把总耗时压到 ADR 0001 数据换算下的毫秒级，
/// 即使叠加两个数量级的调度延迟也仍有充裕余量留在 300ms 超时线以内——
/// 用真实并行负载重复跑满 10 次验证过（见提交信息）。
const ALLOCATING_SCRIPT: &str = r#"
(define (build n acc)
  (if (= n 0)
      acc
      (build (- n 1) (cons n acc))))
(define (blow-budget)
  (build 20000 '()))
"#;

#[test]
fn 超出内存预算的脚本被真实中断且归类为内存预算超限变体() {
    // 这条测试历史上就是这次拆分要修的坏典型：拆分之前，无论中断
    // 是内存超预算还是撞上 300ms 超时触发的，返回值都长得一样,断言
    // 只能停在 `is_err()`——这条测试因此“一直因为错误的原因通过”,
    // 从未真正验证过内存预算这道防线本身（见文件顶部模块文档与
    // `host.rs::classify_error` 文档「两次真实误诊」一节）。拆分之后
    // 这里必须钉住具体变体,而不只是“出错了”。
    // Arrange：先在默认预算（无限制）下把两个函数定义加载好——定义阶段
    // 本身的分配不应该受到接下来设的小预算影响，所以预算要等定义加载
    // 完、真正要执行 blow-budget 之前才调小。
    let mut engine = ScriptEngine::new();
    engine
        .load_source(ALLOCATING_SCRIPT.to_string())
        .expect("函数定义阶段不做任何大分配，不应该失败");

    // 4KB 的预算：构造一个两万层的链表所需内存是这个数字的几个数量级
    // 以上，越界几乎必然发生在最初的几十次 cons 之内。
    set_memory_budget(4096);

    // Act
    let result = engine.call_raw("blow-budget", Vec::new());

    // Assert：不仅要是 Err，还要具体是内存预算变体，且携带的诊断数据
    // 要真实自洽（预算确实是刚设的 4096，累计分配确实超过了它）。
    match result {
        Err(ll_script::ScriptError::MemoryBudgetExceeded {
            allocated_bytes,
            budget_bytes,
        }) => {
            assert_eq!(budget_bytes, 4096);
            assert!(allocated_bytes > budget_bytes);
        }
        other => panic!("期望 MemoryBudgetExceeded，实际拿到 {other:?}"),
    }
}

#[test]
fn 预算充足时同一脚本能正常跑完而不是撞上超时() {
    // 作为上面那条测试的对照组：同样的递归结构，预算给得足够大（远超
    // 两万层链表的实际占用），应当正常返回而不是被误伤——证明「被
    // 打断」确实是预算太小导致的，不是这段脚本本身有问题。
    //
    // 拆分之前这条测试历史上真的“因为错误的原因失败”过：不是预算
    // 出了问题，是脚本跑太久撞上了与内存预算完全无关的 300ms 超时（见
    // 文件顶部 `ALLOCATING_SCRIPT` 文档「为什么是两万层」），当时的
    // `assert!(result.is_ok())` 只会告诉你“失败了”，看不出撞的是哪
    // 一堵墙。这里改用 `unwrap_or_else` 把具体的错误变体带进 panic
    // 消息——万一这条测试将来又在某种极端负载下抖动，第一眼就能确认
    // 是不是历史那个坑重演（`Timeout`），而不是内存记账真的算错了
    // （`MemoryBudgetExceeded`）。
    // Arrange
    let mut engine = ScriptEngine::new();
    engine
        .load_source(ALLOCATING_SCRIPT.to_string())
        .expect("函数定义阶段不做任何大分配，不应该失败");
    set_memory_budget(usize::MAX);

    // Act
    let result = engine.call_raw("blow-budget", Vec::new());

    // Assert
    result.unwrap_or_else(|err| {
        panic!("预算给够时不应该被中断（更不应该是历史上撞过的那个 300ms 超时坑）：{err:?}")
    });
}

#[test]
fn 超时中断的脚本被归类为超时变体而不是内存预算超限变体() {
    // 与上面两条互补：拆分的意义就是两条路各测各的——这里专门覆盖
    // 300ms 看门狗超时那条路径，脚本本身故意不做任何有意义的分配
    // （纯粹自我调用、不接参数的死循环），即使把预算显式设成
    // `usize::MAX` 也一样会被打断，证明打断原因确实是墙钟超时,不是
    // 内存记账,`classify_error` 靠 `alloc_guard` 的线程局部中断原因
    // 记号区分这两条路（见其文档），不是靠猜测经过了多久。
    // Arrange：显式把预算设回 usize::MAX——不能依赖默认值，
    // `MEMORY_BUDGET` 是线程局部状态，libtest 的线程池可能复用了跑过
    // 上面「预算不足」用例（预算设成 4096）的同一根线程。
    let mut engine = ScriptEngine::new();
    set_memory_budget(usize::MAX);

    // Act：纯自我调用、不接收参数、不做任何分配的死循环——300ms 内一定
    // 跑不完，也一定不会撞上任何合理的内存预算。
    let result = engine.load_source("(define (loop) (loop)) (loop)".to_string());

    // Assert
    assert_eq!(result, Err(ll_script::ScriptError::Timeout));
}
