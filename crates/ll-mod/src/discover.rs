//! mod 发现：在给定目录下列出候选 mod 清单路径。
//!
//! 四道防线第④条（加载分阶段隔离）从这里开始：发现阶段只负责「这个
//! 目录下有哪些候选」，完全不打开、不校验任何文件内容——一个 mod 的
//! 清单能不能解析成功，不该反过来影响其他 mod 是否被发现。

use std::fs;
use std::path::{Path, PathBuf};

/// mod 清单文件的约定名。每个候选 mod 是 `root` 下的一个子目录，清单
/// 文件位于该子目录内、固定用这个文件名。
///
/// 格式是 JSON5，不是 JSON——项目所有者 2026-08-20 裁定「全用 json5
/// 吧,还可以写注释方便日后维护」，统一了本仓库全部手写配置格式（此前
/// 是 TOML，见提交历史）。手写清单因此可以带注释与尾逗号（示例见
/// `mods/example_mod/mod.json5`），解析走 [`crate::manifest::parse_manifest`]
/// 的 `json5::from_str`。
pub const MANIFEST_FILENAME: &str = "mod.json5";

/// 列出 `root` 目录下的所有候选 mod 清单路径。
///
/// 每个直接子目录都是一个候选，候选路径是「子目录 + [`MANIFEST_FILENAME`]」
/// ——**不检查该文件是否存在或可解析**，那是 [`crate::manifest::parse_manifest`]
/// 的职责（分阶段隔离：发现与解析互相独立）。
///
/// `root` 不存在或不可读时返回空列表而不是报错——mod 目录整体缺失是
/// 「没有 mod」这一合法状态，不是需要终止进程的错误（规格 §10.4
/// 「缺失 mod 不得崩溃」的精神在发现这一步同样适用）。
///
/// **返回顺序不保证跨平台/跨文件系统稳定**：底层是 `fs::read_dir`，
/// 遍历顺序依赖具体的文件系统实现。调用方（尤其是
/// [`crate::topo::topo_sort`]）不得依赖这个顺序作为排序结果的输入——
/// 排序结果的确定性必须由调用方自己的稳定化规则保证，不能指望发现
/// 阶段提前排好序。
pub fn discover_mods(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path().join(MANIFEST_FILENAME))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;
    use std::fs;

    #[test]
    fn 发现目录下的所有子目录候选() {
        // Arrange
        let root = tempdir();
        fs::create_dir(root.path().join("mod_a")).expect("创建子目录");
        fs::create_dir(root.path().join("mod_b")).expect("创建子目录");

        // Act
        let mut candidates = discover_mods(root.path());
        candidates.sort();

        // Assert
        let mut expected = vec![
            root.path().join("mod_a").join(MANIFEST_FILENAME),
            root.path().join("mod_b").join(MANIFEST_FILENAME),
        ];
        expected.sort();
        assert_eq!(candidates, expected);
    }

    #[test]
    fn 忽略根目录下的普通文件只保留子目录() {
        // Arrange
        let root = tempdir();
        fs::create_dir(root.path().join("mod_a")).expect("创建子目录");
        fs::write(root.path().join("readme.txt"), "not a mod").expect("写入普通文件");

        // Act
        let candidates = discover_mods(root.path());

        // Assert
        assert_eq!(
            candidates,
            vec![root.path().join("mod_a").join(MANIFEST_FILENAME)]
        );
    }

    #[test]
    fn 根目录不存在时返回空列表而不是报错() {
        // Arrange
        let root = tempdir();
        let missing = root.path().join("does_not_exist");

        // Act
        let candidates = discover_mods(&missing);

        // Assert
        assert!(candidates.is_empty());
    }
}
