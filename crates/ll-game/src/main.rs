//! 迷途大陆游戏本体二进制入口。
//!
//! # `#[global_allocator]`：alloc_guard 第三道防线的唯一生效场所
//!
//! `ll_script::alloc_guard` 模块文档明确写着：脚本内存执行预算这道
//! 防线（规格 §10.2 第三道防线）需要进程真的把
//! [`ScriptAllocGuard`] 声明成 `#[global_allocator]` 才会生效——单元
//! 测试二进制不声明全局分配器，`crates/ll-script/tests/memory_budget_enforced.rs`
//! 是本仓库目前唯一装了它的进程（一个独立的集成测试二进制），但那不是
//! 生产路径。本文件是**生产二进制**第一次、也是唯一一次声明它——从
//! 这一行开始，脚本执行期间的每一次分配都会被记账，超预算真的会中断
//! 脚本，不再只是「代码写了但从没在真实进程里跑过」。
//!
//! 真正干活的仍是 [`System`]——本类型只是在它外面包一层线程局部记账
//! （见 `ScriptAllocGuard` 文档），不改变任何分配语义。

use std::alloc::System;

use ll_script::ScriptAllocGuard;

#[global_allocator]
static ALLOC: ScriptAllocGuard<System> = ScriptAllocGuard(System);

fn main() {
    ll_game::run_game();
}
