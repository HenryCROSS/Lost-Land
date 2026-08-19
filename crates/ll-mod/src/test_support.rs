//! 测试专用的临时目录帮手，供 `discover`/`manifest`/`pipeline` 三处
//! mod 加载测试共用。
//!
//! 三份实现此前逐字重复（仅临时目录名的前缀不同：`ll-mod-discover-test`/
//! `ll-mod-test`/`ll-mod-pipeline-test`），抽成一处避免改一处逻辑
//! （例如清理策略）时漏改另外两处。前缀本身只是给人肉眼定位残留目录
//! 用的调试信息，不参与任何正确性判断——合并成一个固定前缀不改变
//! 「进程内不冲突、用完自动清理」这条行为。
#![cfg(test)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 一个会在析构时自动清理的临时目录。
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 建一个进程内大概率不冲突的临时目录：进程 ID + 单调计数器拼路径，
/// 用完在 [`TempDir`] 析构时自动清理。本 crate 不为此引入 `tempfile`
/// 依赖——只有测试需要，且需求简单到手写几行就够。
pub(crate) fn tempdir() -> TempDir {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("ll-mod-test-{}-{n}", std::process::id()));
    fs::create_dir_all(&path).expect("测试临时目录创建不应失败");
    TempDir(path)
}
