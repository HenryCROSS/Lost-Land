//! 测试专用的临时路径帮手，供本 crate 各测试模块共用。
//!
//! 此前各测试模块都自己用「`std::env::temp_dir()` + `std::process::id()`」
//! 拼临时路径。问题在于：同一个进程里所有测试线程拿到的进程 ID 完全
//! 相同，而 `cargo test` 默认就是多线程并行跑。于是同一个帮手被多个
//! 测试并发调用时，它们拿到的是**同一个路径**——一个测试的
//! `remove_dir_all` 会删掉另一个测试正在读的目录，表现为
//! `world.rs` 那类偶发的「创建测试目录应当成功: PermissionDenied」：
//! 单独重跑必过，只在并行时才炸，而且换一次运行换一个测试炸。
//!
//! 修法是在路径里再拼一个进程内单调递增的计数器：进程 ID 隔离不同
//! 进程，计数器隔离同一进程内的不同调用，两者合起来让每一次调用都
//! 拿到独占的路径，测试之间不再共享任何文件系统状态。计数器取自
//! `AtomicU64`，与 `ll-mod` 的 `test_support::tempdir` 同一个手法——
//! 时间戳（哪怕纳秒）做不到这一点：两次并发调用完全可能读到同一个
//! 值，而计数器的唯一性是由 `fetch_add` 保证的，不靠时钟精度。
#![cfg(test)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// 产出一个本进程内独占的临时路径：`<系统临时目录>/<prefix>-<进程 ID>-<序号>`。
///
/// 只产出路径，不创建任何东西——调用方的需求并不一致：有的要一个真实
/// 目录（自行 `create_dir_all`），有的恰恰要一个**确定不存在**的路径
/// （数据目录探测那几条测试），还有的要的是文件名（`.llsave`）。把
/// 创建与清理留给调用方，这个帮手只负责「名字不撞」这一件事。
pub(crate) fn unique_temp_path(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()))
}

/// 仓库真实的 `mods/` 目录路径——`ll-game` 到仓库根固定隔两级 `../..`。
///
/// 本体游戏内容（当前是三个种族）住在 `mods/lostland/*.scm`，不再
/// 硬编码在 Rust 里，因此**任何调用 `crate::content::load_content` 的
/// 测试都必须能看到这个目录**：临时空目录下装载会（正确地）在本体
/// 内容契约解析那一步失败，见 `ll_mod::base_contract` 模块文档。
///
/// 与 `unique_temp_path` 的分工：临时目录仍然用来隔离**写**（存档、
/// 配置），mods_root 是只读输入，共享仓库里那一份即可，不需要每个
/// 测试各拷一份。
pub(crate) fn repo_mods_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 仓库真实mods目录存在且含本体内容目录() {
        // 守卫：本体内容目录一旦被删/改名，先在这里红,而不是等到某个
        // 装载测试报出一条难以定位的契约解析失败。
        // Arrange & Act
        let mods = repo_mods_dir();

        // Assert
        assert!(mods.join("lostland").join("mod.json5").is_file());
    }

    #[test]
    fn 同一个前缀连续取两次得到不同路径() {
        // Arrange & Act
        let 甲 = unique_temp_path("ll-game-test-support-连续");
        let 乙 = unique_temp_path("ll-game-test-support-连续");

        // Assert
        assert_ne!(甲, 乙, "同前缀的两次调用必须拿到互不相同的路径");
    }

    #[test]
    fn 多线程并发取路径时全部互不相同() {
        // 这条测试正是本模块存在的理由：旧的「进程 ID」写法在这里会
        // 让所有线程拿到同一个路径，从而全部撞车。
        // Arrange
        const 线程数: usize = 16;
        const 每线程取数: usize = 64;

        // Act
        let 各线程结果: Vec<Vec<PathBuf>> = std::thread::scope(|scope| {
            let 句柄: Vec<_> = (0..线程数)
                .map(|_| {
                    scope.spawn(|| {
                        (0..每线程取数)
                            .map(|_| unique_temp_path("ll-game-test-support-并发"))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            句柄
                .into_iter()
                .map(|h| h.join().expect("取路径的线程不应 panic"))
                .collect()
        });

        // Assert
        let 全部: std::collections::HashSet<PathBuf> = 各线程结果.into_iter().flatten().collect();
        assert_eq!(
            全部.len(),
            线程数 * 每线程取数,
            "并发取到的路径出现了重复，临时目录仍会在并行测试下互相踩踏"
        );
    }
}
