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
//!
//! # 隔离策略：`thread_local!`，不是进程级 `static`
//!
//! 早期版本把计数器、预算、活跃中断通道放在进程级 `static`
//! （`AtomicUsize`/`Mutex`），复审发现两个问题：
//!
//! 1. **语义上就不对**：进程里任何线程的任何分配都会计入同一份计数器。
//!    若脚本在主线程执行，同一时刻另一个线程（渲染、后台加载……）恰好在
//!    分配内存，那笔分配会被错误地记到当前脚本的预算头上——`ScriptEngine`
//!    的调用模型是「单线程主线程顺序调用」（ADR 0001），账应该按「当前
//!    是哪个线程在跑脚本」来记，不是按「进程里发生了什么分配」来记。
//! 2. **测试上会竞态**：`cargo test` 用线程池并行跑同一二进制内的全部
//!    用例，若这几个状态是进程级 `static`，两个正在不同线程上并发执行的
//!    `ScriptEngine::call_raw`/`load_source` 会互相踩踏彼此的「活跃中断
//!    通道」——A 刚把通道设成自己的引擎，B 紧接着覆盖成它自己的引擎，A
//!    这次超预算的中断真正发给的是 B 的引擎，两边的断言都会随机失败。
//!    进程级 `Mutex` 能挡住这个问题，但代价是把所有脚本调用（包括生产
//!    路径）串行化，且早期版本的 `TEST_SERIAL` 只锁住了本文件内部三条
//!    测试，锁不住 `host.rs`/`api/*.rs` 里其余几十个调用点，等于没有
//!    真正解决问题。
//!
//! `thread_local!` 同时解决了两点：libtest 的线程池模型是"一个线程一次
//! 只服务一个测试"（不会把两个不同测试函数体的字节码交错执行在同一根
//! 线程上），把这几个状态换成线程局部，天然获得「两次并发调用各自持有
//! 独立副本、互不可见」的隔离效果——不需要引入任何锁，也不依赖「测试
//! 恰好没有同时跑」这种运气；生产路径上单线程顺序调用的语义则完全不受
//! 影响（`ScriptEngine` 本来就只在主线程用）。`ll_script::api::query` 的
//! `ACTIVE_WORLD`、`ll_script::api::state` 的 `PENDING_WRITES`、
//! `ll-mod` 的 `script_terrain_api.rs` 都是同一个模式的先例。
//!
//! # 为什么不按 `ScriptEngine` 实例记账（评估结论）
//!
//! 「每个引擎实例各自持有计数器与预算」听起来更精确，但撞上一个硬限制：
//! `GlobalAlloc::alloc`/`dealloc` 是自由函数式的接口（`&self` 指向分配器
//! 本身，不是指向"当前是哪个 `ScriptEngine` 在跑"），分配器完全不知道
//! 调用它的这次分配属于哪个 Rust 值——`#[global_allocator]` 天然是进程
//! 级单例，不存在「每个引擎各自的分配器」这种东西，除非在**每一次**
//! `alloc`/`dealloc` 里都去查一个"当前活跃引擎是谁"的标记。
//!
//! 而这正是 `ACTIVE_CONTROLLER` 已经在做的事：它就是"当前（这根线程上）
//! 活跃的引擎的标识"。本模块因此退而求其次，做到「按线程」而不是「按
//! 实例」——在当前"一个线程同一时刻只跑一个引擎的一次调用"的执行模型
//! 下（`ScriptEngine::call_raw`/`load_source` 顺序执行，不重入），这两者
//! 观测到的结果完全一致：任意时刻线程局部存储里活跃的那个通道，就是
//! "当前这根线程正在执行的那次调用"，等价于"当前这个引擎实例"。
//!
//! 若未来引入真正的并行脚本执行（同一线程上交替/嵌套跑多个引擎的调用），
//! 「按线程」与「按实例」才会分道扬镳，需要重新设计（例如把标识从
//! "线程" 换成显式传递的调用上下文 token）。当前代码库不存在这种用法
//! （`register_fn` 注册的任何函数都不会反过来调用
//! `ScriptEngine::call_raw`/`load_source`，见 `host.rs` 模块文档），这个
//! 限制不构成产品功能损失。

use std::alloc::{GlobalAlloc, Layout};
use std::cell::{Cell, RefCell};

use steel::steel_vm::ThreadStateController;

thread_local! {
    /// 当前线程上，本次脚本调用累计的净分配字节数：`alloc` 加、
    /// `dealloc` 减。见模块文档「隔离策略」——按线程记账，不是按进程。
    static ALLOCATED_BYTES: Cell<usize> = const { Cell::new(0) };

    /// 当前线程允许的分配预算，字节。默认 `usize::MAX`（不限制）——
    /// 宿主必须显式调用 [`set_memory_budget`] 才会真正生效，避免忘记
    /// 配置时把引擎构造本身（分配不少内存，见 ADR 0012 的 56ms 数字）
    /// 都算作超预算。
    static MEMORY_BUDGET: Cell<usize> = const { Cell::new(usize::MAX) };

    /// 当前线程上，正在执行脚本的中断通道。
    static ACTIVE_CONTROLLER: RefCell<Option<ThreadStateController>> = const { RefCell::new(None) };
}

/// 设置当前线程的内存预算（字节）。
///
/// 是线程局部状态：同一线程上先后创建的多个 `ScriptEngine` 实例，若
/// 都不重新调用本函数，会共享同一份预算——这是「线程局部」这个手法
/// 本身的固有性质（见模块文档「为什么不按实例记账」），当前脚本执行
/// 模型下（单线程顺序调用、不重入）不构成问题：任意时刻只有一次调用
/// 在真正读写计数器，"共享"等价于"顺序复用同一份"。
pub fn set_memory_budget(bytes: usize) {
    MEMORY_BUDGET.with(|budget| budget.set(bytes));
}

/// 把当前线程的分配计数器清零。
///
/// **必须在每次脚本调用开始前调用**，否则前一次调用遗留的分配量会累加
/// 进这一次，把明明在预算内的脚本误判成超预算。
pub fn reset_alloc_counter() {
    ALLOCATED_BYTES.with(|bytes| bytes.set(0));
}

/// 读取当前线程累计分配字节数，供调用方诊断/展示用（例如加载管理界面
/// 里显示「这次调用用了多少内存」）。
pub fn allocated_bytes() -> usize {
    ALLOCATED_BYTES.with(Cell::get)
}

/// 设置当前线程「正在执行脚本」的中断通道，超预算时会调用它的
/// `interrupt()`。
///
/// 必须在 [`reset_alloc_counter`] 之后、脚本真正开始求值之前调用；脚本
/// 求值结束后应调用 [`clear_active_controller`]，避免脚本执行窗口之外的
/// 普通 Rust 分配意外触发一个已经不该被观察的中断通道。
pub fn set_active_controller(controller: ThreadStateController) {
    ACTIVE_CONTROLLER.with(|slot| *slot.borrow_mut() = Some(controller));
}

/// 清空当前线程活跃的中断通道。
pub fn clear_active_controller() {
    ACTIVE_CONTROLLER.with(|slot| *slot.borrow_mut() = None);
}

/// 超预算时的中断触发点，拆成独立函数避免 `alloc` 函数体过长，也方便
/// 单独在文档里说明「谁负责调用 `interrupt()`」。
fn trigger_interrupt_if_configured() {
    ACTIVE_CONTROLLER.with(|slot| {
        if let Some(controller) = slot.borrow().as_ref() {
            controller.interrupt();
        }
    });
}

/// 记一次分配，越界则触发中断。拆出来是为了能在测试里直接调用——不需要
/// 真的把进程的 `#[global_allocator]` 切换成本类型（单元测试二进制没有
/// 声明 `#[global_allocator]`，见 [`ScriptAllocGuard`] 文档；真正验证
/// 「装上全局分配器后确实生效」的测试在
/// `crates/ll-script/tests/memory_budget_enforced.rs`，那是独立的集成
/// 测试二进制，`cargo test` 每个 `tests/*.rs` 文件都编译成独立进程，装
/// 全局分配器只影响那一个进程，不影响本文件所在的库测试二进制）。
///
/// 用 `saturating_add`，不用 `+`：真的装上 `#[global_allocator]` 之后
/// 见到过实测 panic——`InterruptHandler` 自己内部会 `std::thread::spawn`
/// 一个看门狗线程，`crossbeam_channel` 的内部缓冲区可能在一个线程分配、
/// 在另一个线程释放（channel 内部引用计数归零发生在接收端），这类
/// 「跨线程配对」在任何非平凡的多线程 Rust 程序里都存在，本模块管不到、
/// 也不该假装管得到。见 [`record_dealloc`] 的 `saturating_sub`——两者
/// 配合，即使某个线程收到了它自己从未记过账的释放（计数器被顶到 0），
/// 也不会把计数器顶到 `usize` 上界附近导致下一次分配的加法整数溢出。
fn record_alloc(size: usize) {
    let total = ALLOCATED_BYTES.with(|bytes| {
        let updated = bytes.get().saturating_add(size);
        bytes.set(updated);
        updated
    });
    if total > MEMORY_BUDGET.with(Cell::get) {
        trigger_interrupt_if_configured();
    }
}

/// 记一次释放。见 [`record_alloc`] 的拆分理由与 `saturating_add`/
/// `saturating_sub` 配套使用的理由。
///
/// 用 `saturating_sub`，不用 `wrapping_sub`：若某次释放对应的分配没有
/// 被这根线程的计数器记过账（典型场景见 [`record_alloc`] 文档的跨线程
/// 释放；另一种场景是计数器刚被 [`reset_alloc_counter`] 清零，但还有
/// 上一次调用申请、这一次才释放的内存），把计数器减到 0 封顶，而不是
/// 无符号下溢回绕到接近 `usize::MAX`——回绕出的巨大值会让下一次
/// `record_alloc` 立刻（错误地）判定"已超预算"，对一个完全无辜、还没
/// 分配几个字节的线程造成误伤；封顶在 0 则只是让这一次记账"少算了"，
/// 不会污染后续判断的方向。
fn record_dealloc(size: usize) {
    ALLOCATED_BYTES.with(|bytes| bytes.set(bytes.get().saturating_sub(size)));
}

/// 包装系统分配器，用线程局部计数器统计脚本执行期间的分配量。
///
/// 用法：作为进程的 `#[global_allocator]`，包装真正干活的分配器（通常是
/// [`std::alloc::System`]）。本类型自身不分配、不持有状态，状态都在上面
/// 的线程局部变量里——`GlobalAlloc` 的实现要求 `&self` 而不是
/// `&mut self`（分配器要能被多线程共享调用），线程局部变量是唯一选择：
/// 既不需要 `&mut self` 也不需要跨线程同步（`thread_local!` 本身就是
/// 「每根线程各有一份，互不可见」）。
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

    use steel::steel_vm::engine::Engine;

    /// 测试结束时统一复位线程局部状态，避免一个测试的收尾状态污染
    /// 「同一根线程」上后续跑的下一个测试——libtest 的线程池会复用线程
    /// 跑多个测试，线程局部状态不会随单个测试函数返回而自动清零。
    fn reset_globals() {
        reset_alloc_counter();
        set_memory_budget(usize::MAX);
        clear_active_controller();
    }

    #[test]
    fn 分配量超过预算后触发中断() {
        // Arrange：不需要互斥锁——见模块文档「隔离策略」，这几个状态是
        // 线程局部的，与其他测试线程上发生的 reset/set/clear 互不可见。
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
        reset_globals();
        let alloc_guard = ScriptAllocGuard(System);
        let layout = Layout::from_size_align(256, 8).unwrap();
        let ptr = unsafe { alloc_guard.alloc(layout) };
        assert!(allocated_bytes() >= 256);

        // Act：模拟「下一次调用开始前」的重置。
        reset_alloc_counter();

        // Assert：前一次调用的分配量不会带进这一次。
        assert_eq!(allocated_bytes(), 0);

        // Cleanup：计数器已经清零，这次 dealloc 对应的分配从未被计过账，
        // 会被 saturating_sub 封顶在 0（见 record_dealloc 文档）——测试
        // 收尾不断言封顶后的值。
        unsafe { alloc_guard.dealloc(ptr, layout) };
        reset_globals();
    }
}
