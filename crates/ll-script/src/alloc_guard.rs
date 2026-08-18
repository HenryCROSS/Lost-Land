//! 内存守卫：统计脚本执行期间的分配量，超阈值时触发中断。
//!
//! # 为什么需要这个模块
//!
//! ADR 0001 实测：Steel 的死循环能被 `InterruptHandler` 稳定掐断
//! （精确到单条字节码），但**无限分配**只能靠超时侥幸救下——非尾递归
//! 100 万层 59ms、1000 万层 644ms 都正常返回，说明中断预算本身没有把
//! 分配量纳入判断依据；一个分配速度够快的脚本可能在超时触发前先把内存
//! 耗尽。这是 ADR 0001 明确指出的「原生缺口」，本模块是唯一对策。
//!
//! # 设计：复用同一条中断通道，不另开机制
//!
//! [`steel::steel_vm::ThreadStateController`] 就是 ADR 0001 里
//! `InterruptHandler` 用来掐断死循环的同一个句柄——它只是「设置一个原子
//! 标志，VM 在下一次安全点检查时观察到就返回 `Err`」，谁来设置这个标志
//! 不重要。本模块复用它：超预算时调用同一个 `interrupt()`，脚本侧看到的
//! 效果和「死循环被看门狗掐断」完全一样，调用方不需要区分「为什么被中断」。

use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use steel::steel_vm::ThreadStateController;

/// 当前净分配字节数：`alloc` 加、`dealloc` 减。
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

/// 允许的分配预算，字节。默认 `usize::MAX`（不限制）——宿主必须显式调用
/// [`set_memory_budget`] 才会真正生效，避免忘记配置时把引擎构造本身
/// （分配不少内存,见 ADR 0012 的 56ms 数字）都算作超预算。
static MEMORY_BUDGET: AtomicUsize = AtomicUsize::new(usize::MAX);

/// 当前正在执行脚本的中断通道。
///
/// 用 `Mutex` 包装：只在「设置/清空活跃通道」与「超预算触发中断」两个
/// 低频路径上访问，不在每次 `alloc`/`dealloc` 的热路径里——热路径只有
/// 一次 `fetch_add`/`fetch_sub` 加一次 `load` 比较，符合任务简报「开销为
/// 每次 alloc/dealloc 一次原子加减」的要求。
fn active_controller_slot() -> &'static Mutex<Option<ThreadStateController>> {
    static SLOT: OnceLock<Mutex<Option<ThreadStateController>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 设置内存预算（字节）。
///
/// 应用在整个进程范围——如果同时有多个 `ScriptEngine` 实例，它们共享
/// 同一份预算与计数器。这是「包装全局分配器」这个手法本身的固有限制：
/// `#[global_allocator]` 天然是进程级单例，不存在「每个引擎各自的分配
/// 器」这种东西。当前脚本执行模型是单线程主线程调用（ADR 0001「架构
/// 不需要退化」的结论），这个限制暂不构成问题；若未来引入并行脚本执行，
/// 需要重新设计（例如按线程切分预算）。
pub fn set_memory_budget(bytes: usize) {
    MEMORY_BUDGET.store(bytes, Ordering::Relaxed);
}

/// 把分配计数器清零。
///
/// **必须在每次脚本调用开始前调用**，否则前一次调用遗留的分配量会累加
/// 进这一次，把明明在预算内的脚本误判成超预算。
pub fn reset_alloc_counter() {
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
}

/// 读取当前累计分配字节数，供调用方诊断/展示用（例如加载管理界面里显示
/// 「这次调用用了多少内存」）。
pub fn allocated_bytes() -> usize {
    ALLOCATED_BYTES.load(Ordering::Relaxed)
}

/// 设置「当前正在执行脚本」的中断通道，超预算时会调用它的 `interrupt()`。
///
/// 必须在 [`reset_alloc_counter`] 之后、脚本真正开始求值之前调用；脚本
/// 求值结束后应调用 [`clear_active_controller`]，避免脚本执行窗口之外的
/// 普通 Rust 分配意外触发一个已经不该被观察的中断通道。
pub fn set_active_controller(controller: ThreadStateController) {
    *active_controller_slot().lock().unwrap() = Some(controller);
}

/// 清空当前活跃的中断通道。
pub fn clear_active_controller() {
    *active_controller_slot().lock().unwrap() = None;
}

/// 超预算时的中断触发点，拆成独立函数避免 `alloc` 函数体过长，也方便
/// 单独在文档里说明「谁负责调用 `interrupt()`」。
fn trigger_interrupt_if_configured() {
    if let Ok(guard) = active_controller_slot().lock()
        && let Some(controller) = guard.as_ref()
    {
        controller.interrupt();
    }
}

/// 记一次分配，越界则触发中断。拆出来是为了能在测试里直接调用——不需要
/// 真的把进程的 `#[global_allocator]` 切换成本类型。真的切换会让**整个
/// 测试二进制里所有测试的所有分配**都过一遍这份记账逻辑，在并行测试下
/// 引入无法控制的噪声（另一个测试线程分配的内存会计入这个测试的预算）。
fn record_alloc(size: usize) {
    let total = ALLOCATED_BYTES.fetch_add(size, Ordering::Relaxed) + size;
    if total > MEMORY_BUDGET.load(Ordering::Relaxed) {
        trigger_interrupt_if_configured();
    }
}

/// 记一次释放。见 [`record_alloc`] 的拆分理由。
fn record_dealloc(size: usize) {
    ALLOCATED_BYTES.fetch_sub(size, Ordering::Relaxed);
}

/// 包装系统分配器，用原子计数器统计脚本执行期间的分配量。
///
/// 用法：作为进程的 `#[global_allocator]`，包装真正干活的分配器（通常是
/// [`std::alloc::System`]）。本类型自身不分配、不持有状态，状态都在上面
/// 的进程级静态变量里——`GlobalAlloc` 的实现要求 `&self` 而不是
/// `&mut self`（分配器要能被多线程共享调用），静态变量是唯一选择。
pub struct ScriptAllocGuard<A>(pub A);

// Safety: `alloc`/`dealloc` 只是在调用内部分配器 `A` 的同名方法前后各插
// 一段记账逻辑，不改变分配/释放的语义、不跳过任何调用、不篡改布局或
// 指针——分配器契约（返回的指针必须能且只能被同一分配器的 `dealloc`
// 以同样的 `Layout` 释放）完全由 `A` 保证，本类型不引入新的不安全性。
unsafe impl<A: GlobalAlloc> GlobalAlloc for ScriptAllocGuard<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Safety: 转发给内部分配器，参数原样传递。
        let ptr = unsafe { self.0.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record_dealloc(layout.size());
        // Safety: 转发给内部分配器，参数原样传递。
        unsafe { self.0.dealloc(ptr, layout) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::System;
    use std::sync::Mutex as StdMutex;

    use steel::steel_vm::engine::Engine;

    /// 三个测试都会读写同一组进程级静态变量，用锁把它们彼此串行化——
    /// 不需要处理跨文件（比如 `host.rs`）的干扰，因为没有任何其他代码
    /// 会调用 `record_alloc`/`record_dealloc`（本文件没有声明
    /// `#[global_allocator]`，进程的真实分配器没有被替换）。
    static TEST_SERIAL: StdMutex<()> = StdMutex::new(());

    /// 测试结束时统一复位全局状态，避免一个测试的收尾状态污染下一个。
    fn reset_globals() {
        reset_alloc_counter();
        set_memory_budget(usize::MAX);
        clear_active_controller();
    }

    #[test]
    fn 分配量超过预算后触发中断() {
        // Arrange
        let _serial = TEST_SERIAL.lock().unwrap();
        reset_globals();
        set_memory_budget(100);
        let mut engine = Engine::new();
        set_active_controller(engine.get_thread_state_controller());
        let alloc_guard = ScriptAllocGuard(System);
        let layout = Layout::from_size_align(1000, 8).unwrap();

        // Act：这一次分配远超 100 字节的预算，应当触发中断。
        let ptr = unsafe { alloc_guard.alloc(layout) };
        assert!(!ptr.is_null());
        let result = engine.run("(+ 1 2)".to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup
        unsafe { alloc_guard.dealloc(ptr, layout) };
        reset_globals();
    }

    #[test]
    fn 预算内的正常脚本不受影响() {
        // Arrange
        let _serial = TEST_SERIAL.lock().unwrap();
        reset_globals();
        set_memory_budget(1_000_000);
        let mut engine = Engine::new();
        set_active_controller(engine.get_thread_state_controller());
        let alloc_guard = ScriptAllocGuard(System);
        let layout = Layout::from_size_align(64, 8).unwrap();

        // Act：分配量远小于预算。
        let ptr = unsafe { alloc_guard.alloc(layout) };
        let result = engine.run("(+ 1 2)".to_string());

        // Assert
        assert!(result.is_ok());

        // Cleanup
        unsafe { alloc_guard.dealloc(ptr, layout) };
        reset_globals();
    }

    #[test]
    fn 计数器在两次调用之间正确重置() {
        // Arrange
        let _serial = TEST_SERIAL.lock().unwrap();
        reset_globals();
        let alloc_guard = ScriptAllocGuard(System);
        let layout = Layout::from_size_align(256, 8).unwrap();
        let ptr = unsafe { alloc_guard.alloc(layout) };
        assert!(allocated_bytes() >= 256);

        // Act：模拟「下一次调用开始前」的重置。
        reset_alloc_counter();

        // Assert：前一次调用的分配量不会带进这一次。
        assert_eq!(allocated_bytes(), 0);

        // Cleanup：计数器已经清零，这次 dealloc 会让计数器变成
        // 「负数回绕」吗？不会——`fetch_sub` 在 usize 下溢时会 wrapping，
        // 但这里只是测试收尾，不断言 dealloc 之后的值，无影响。
        unsafe { alloc_guard.dealloc(ptr, layout) };
        reset_globals();
    }
}
