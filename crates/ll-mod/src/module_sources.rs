//! 把一个 mod 目录里的 `.scm` 文件读成 [`ll_script::ModuleTable`] 认的
//! 「模块名 → 源码」条目——`require` 的读盘那一半只发生在这里。
//!
//! # 为什么读盘在这里，不在解析器里
//!
//! `steel-core` 的源码模块解析器（[`ll_script::modules`]）拿到的是脚本
//! 原样写下的字符串。若让它按那个字符串去拼路径再读盘，「不许上跳、
//! 不许绝对路径」就变成一道**需要证明自己没漏**的字符串校验；漏一种
//! 写法就是一次任意文件读取（`(require "C:/…/私密文件.scm")` 实测真的
//! 读得到，见 ADR 0012 与 `ll_script::modules` 模块文档）。
//!
//! 反过来做就没有这个负担：**先遍历 mod 目录，把每一个真实存在的
//! `.scm` 文件反推成一个模块名**，脚本给的字符串只用来在这张表里查。
//! 键的集合由文件系统上真实存在的东西封顶，脚本写什么都不可能扩大它
//! ——目录穿越不是「被挡住了」，是没有那条路。
//!
//! # 全部 `.scm` 都进表，包括入口脚本自己
//!
//! 不按 `entry_points`/`event_scripts` 过滤：清单里那两份列表说的是
//! 「装载期跑什么」「结算期跑什么」，与「哪些文件可以被 require」是两
//! 个问题。少收一个文件，mod 作者就会撞上一条「文件明明在那儿却说找
//! 不到」的假错误。
//!
//! 代价是入口脚本自己也能被 require——那会让它的 `register-*` 再跑一
//! 遍，撞上「重复定义」当场报错。那是 mod 作者自己写出来的问题，报错
//! 点名清楚，不需要本模块预先禁止。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ll_script::ModuleTable;
use ll_script::modules::{MODULE_FILE_EXTENSION, parse_key};

use crate::manifest::ModManifest;

/// 一个 mod 目录下全部可作模块用的 `.scm` 文件：`(模块路径, 源码)`。
///
/// 模块路径相对 mod 根目录、不含扩展名、分隔符恒为 `/`。
pub type ModuleSources = Vec<(String, String)>;

/// 遍历 `mod_root`，把每个 `.scm` 文件读成一条 `(模块路径, 源码)`。
///
/// 读不出来的文件（权限、编码）与「文件名反推出来的模块路径不合法」的
/// 文件（例如 `my.helpers.scm`——模块路径里不许有 `.`，见
/// [`ll_script::modules::parse_key`]）一律**跳过**，不报错：它们只是
/// 无法被 `require` 而已，真有人去 require 会拿到一条「找不到模块」的
/// 点名错误，那比在装载期为一个谁都没用到的文件拒绝整个 mod 更合适。
///
/// 结果按模块路径排序，让「同一份磁盘内容 → 同一张表」这件事不依赖
/// 目录遍历顺序（约束 C5：装载过程必须是确定性的）。
pub fn collect_module_sources(mod_root: &Path, self_namespace: &str) -> ModuleSources {
    let mut found = Vec::new();
    collect_into(mod_root, mod_root, self_namespace, &mut found);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn collect_into(mod_root: &Path, dir: &Path, self_namespace: &str, into: &mut ModuleSources) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_into(mod_root, &path, self_namespace, into);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some(MODULE_FILE_EXTENSION) {
            continue;
        }
        let Some(key_path) = module_path_of(mod_root, &path) else {
            continue;
        };
        // 文件名反推出来的模块名也要过一遍语法校验：`my.helpers.scm`
        // 反推出的 `my.helpers` 里含 `.`，不是一个合法模块名。
        if parse_key(&key_path, self_namespace).is_err() {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        into.push((key_path, source));
    }
}

/// 把一个 `.scm` 文件的路径反推成模块路径：相对 `mod_root`、去掉扩展
/// 名、分隔符统一成 `/`。
fn module_path_of(mod_root: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(mod_root).ok()?;
    let with_extension = relative.to_str()?;
    let stripped = with_extension.strip_suffix(&format!(".{MODULE_FILE_EXTENSION}"))?;
    Some(stripped.replace('\\', "/"))
}

/// 给 `manifest` 造一张模块表：本 mod 自己的全部模块（无前缀），加上它
/// **声明过依赖**的每个 mod 的全部模块（带 `<mod id>:` 前缀）。
///
/// `sources_by_namespace` 是本次装载会话里全部 mod 的模块源码，由调用方
/// 一次性收集好——跨 mod require 要的是依赖方目录里的源码，本函数不再
/// 自己去读盘。
///
/// **没声明的依赖压根不进表**：权限判定因此有两道——表里没有（本函数）
/// 与 [`ll_script::ModuleTable::check`] 里那条显式的依赖检查。后者存在
/// 的意义是给出一句准确的话（「去 mod.json5 里补依赖」），不是这道隔离
/// 的依据。
pub fn build_module_table(
    manifest: &ModManifest,
    sources_by_namespace: &[(String, ModuleSources)],
) -> ModuleTable {
    let self_namespace = manifest.id.namespace().to_string();
    let declared: HashSet<String> = manifest
        .dependencies
        .iter()
        .map(|dependency| dependency.namespace.clone())
        .collect();
    let mut table = ModuleTable::new(self_namespace.clone(), declared.clone());

    for (namespace, sources) in sources_by_namespace {
        let prefix = if *namespace == self_namespace {
            None
        } else if declared.contains(namespace) {
            Some(namespace.as_str())
        } else {
            continue;
        };
        for (path, source) in sources {
            table.insert(prefix, path, source.clone());
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;
    use std::fs;

    #[test]
    fn 收集子目录里的scm文件并去掉扩展名() {
        // Arrange
        let root = tempdir();
        fs::write(root.path().join("helpers.scm"), "(provide f)").expect("写文件");
        fs::create_dir(root.path().join("content")).expect("建目录");
        fs::write(root.path().join("content").join("races.scm"), "(provide g)").expect("写文件");
        fs::write(root.path().join("mod.json5"), "{}").expect("写文件");

        // Act
        let sources = collect_module_sources(root.path(), "mymod");

        // Assert
        let keys: Vec<&str> = sources.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, vec!["content/races", "helpers"]);
    }

    #[test]
    fn 文件名反推不出合法模块名的文件被跳过() {
        // Arrange
        let root = tempdir();
        fs::write(root.path().join("my.helpers.scm"), "(provide f)").expect("写文件");
        fs::write(root.path().join("ok.scm"), "(provide g)").expect("写文件");

        // Act
        let sources = collect_module_sources(root.path(), "mymod");

        // Assert
        let keys: Vec<&str> = sources.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, vec!["ok"]);
    }

    #[test]
    fn 未声明依赖的mod的模块压根不进表() {
        // Arrange
        let manifest = ModManifest {
            id: ll_core::ident::NamespacedId::parse("mymod:self").expect("测试用命名空间恒合法"),
            version: "0.1.0".to_string(),
            dependencies: vec![crate::manifest::ModDependency {
                namespace: "dep".to_string(),
                constraint: crate::version_constraint::VersionConstraint::Any,
            }],
            entry_points: Vec::new(),
            event_scripts: Vec::new(),
        };
        let sources = vec![
            (
                "dep".to_string(),
                vec![("h".to_string(), "(provide a) (define a 1)".to_string())],
            ),
            (
                "other".to_string(),
                vec![("h".to_string(), "(provide b) (define b 2)".to_string())],
            ),
        ];

        // Act
        let table = build_module_table(&manifest, &sources);

        // Assert
        assert!(table.check("dep:h").is_ok(), "声明过的依赖应当可见");
        assert!(table.check("other:h").is_err(), "没声明的依赖不该可见");
    }
}
