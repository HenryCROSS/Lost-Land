//! 存档反序列化 fuzz target（任务 11）。
//!
//! 目标函数：对任意字节序列调用
//! [`ll_content::save_file::load_full_from_bytes`]，断言不 panic——
//! 规格 §14.3 判定标准：任何输入都不得 panic、不得 OOM、不得无限
//! 循环，只允许返回 `Err`（这里体现为 `LoadOutcome::Rejected`）。
//!
//! 当前会话（`current_registry`/`current_terrain_table`）用与本 crate
//! 其余测试同一套本体地形声明表构造——这不是为了让任意输入都能走到
//! `Playable`（几乎不可能，fuzz 输入绝大多数字节根本不是合法存档），
//! 是为了让「mod 内容哈希校验」「`ContentIndex` 重映射」这些依赖当前
//! 会话状态的代码路径也有机会被真正跑到，而不是让每次调用都在读头部
//! 那一步就提前返回——只测最外层的字节解析,会漏掉更深处的缺陷（本任务
//! 落地过程中，`crates/ll-content/tests/fuzz_save_load.rs` 用等价的
//! `proptest` 版本正是在「重映射之后、地形重建」这条更深的路径上撞见
//! 过一次真实 panic，见该文件与 `crates/ll-world/src/surface_store.rs`
//! `Deserialize` 实现新增的区块坐标范围校验）。
//!
//! 运行：`cargo fuzz run save_load`（需要 nightly 工具链 + `clang`，见
//! `../Cargo.toml` 顶部说明）。

#![no_main]

use ll_mod::registry::Registry;
use ll_world::terrain::materialize_base_terrain;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut registry = Registry::new();
    let (_ids, terrain_table) = materialize_base_terrain(&mut |id| registry.intern(id))
        .expect("本体地形声明表内部一致，注册恒不失败");

    let _ = ll_content::save_file::load_full_from_bytes(data, &registry, &[], terrain_table, &[]);
});
