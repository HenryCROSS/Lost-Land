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
/// 一层 300ms 的 `InterruptHandler` 超时，与本模块的内存预算共用同一条
/// `interrupt()` 通道（`classify_error` 把两者都归一化成同一个
/// `ScriptError::Interrupted`，从返回值分不出是哪个触发的）。ADR 0001
/// 实测非尾递归 1,000,000 层约 59ms、10,000,000 层约 644ms，按此换算，
/// 5,000,000 层在**完全没有并行负载**时就已经逼近 300ms 这条线；一旦
/// `cargo test --workspace` 把其余测试二进制、其余 crate 的编译一起挤上
/// CPU，「预算充足」那条调用真正等到的常常是 300ms 超时打断，而不是
/// 跑到底——它的名字承诺的是"验证预算机制不会误伤"，实际却在赌调度器
/// 是否来得及在 300ms 内把这次调用跑完，两件事完全不是一回事。
///
/// 两万层不改变两条测试各自要验证的东西：下面 4KB 预算的用例仍然会在
/// 最初几十次 `cons`（远小于两万层）内触发预算中断；`usize::MAX` 预算
/// 的用例则把总耗时压到 ADR 0001 数据换算下的毫秒级，即使叠加两个数量级
/// 的调度延迟也仍有充裕余量留在 300ms 超时线以内——用真实并行负载重复
/// 跑满 10 次验证过（见提交信息）。
const ALLOCATING_SCRIPT: &str = r#"
(define (build n acc)
  (if (= n 0)
      acc
      (build (- n 1) (cons n acc))))
(define (blow-budget)
  (build 20000 '()))
"#;

#[test]
fn 超出内存预算的脚本被真实中断而非跑到底() {
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

    // Assert：脚本必须被打断并返回 Err，而不是分配到底跑出结果。
    assert!(result.is_err());
}

#[test]
fn 预算充足时同一脚本能正常跑完() {
    // 作为上面那条测试的对照组：同样的递归结构，预算给得足够大（远超
    // 两万层链表的实际占用），应当正常返回而不是被误伤——证明「被
    // 打断」确实是预算太小导致的，不是这段脚本本身有问题。
    //
    // 注意：这条测试断言的是「预算给够时不应该被打断」，不是「脚本一定
    // 能在某个时间内跑完」——`ScriptEngine::call_raw` 内部还套了一层与
    // 本模块无关的 300ms 超时（`host.rs` 的 `InterruptHandler`），两种
    // 打断从返回值上无法区分（见 `ALLOCATING_SCRIPT` 文档）。选一个能在
    // 该超时窗口内稳定跑完的递归深度，是让这条测试名实相符的前提。
    // Arrange
    let mut engine = ScriptEngine::new();
    engine
        .load_source(ALLOCATING_SCRIPT.to_string())
        .expect("函数定义阶段不做任何大分配，不应该失败");
    set_memory_budget(usize::MAX);

    // Act
    let result = engine.call_raw("blow-budget", Vec::new());

    // Assert
    assert!(result.is_ok());
}
