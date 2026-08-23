//! 迷途大陆游戏本体二进制入口。
//!
//! # 这里此前有一个 `#[global_allocator]`
//!
//! 脚本内存执行预算（规格 §10.2 第三道防线）要求进程把
//! `ll_script::ScriptAllocGuard` 声明成 `#[global_allocator]` 才会生效，
//! 本文件是生产二进制唯一声明它的地方。脚本系统整体拆除之后既没有
//! 脚本、也没有需要记账的执行窗口，那一行连同 `ll-script` crate 一起
//! 没了——现在这里没有任何分配器包装，进程用的就是 Rust 默认全局
//! 分配器。

fn main() {
    ll_game::run_game();
}
