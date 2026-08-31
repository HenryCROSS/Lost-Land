//! 本地化目录发现：回答「哪些命名空间各自有一个 `locales/` 目录」。
//!
//! # 与 [`crate::asset_vfs`] 的分工：共用发现，不共用覆盖
//!
//! 本模块与 `asset_vfs` 复用同三件公共设施——[`crate::discover::discover_mods`]
//! 发现候选、[`crate::manifest::parse_manifest`] 拿命名空间、
//! [`crate::topo::topo_sort`] 排出确定性总序。**但不复用 `asset_vfs`
//! 自身**，理由是 ADR 0021 的判据（抽象需要共享算法，不是对称性）：
//!
//! `asset_vfs` 的主体是**覆盖解析**——`overrides/<目标命名空间>/`、同路径
//! 冲突产出 `LoadStatus::Warning`、`ResolvedSprite` 的 id/路径双索引。
//! 本地化按 `(命名空间, 语言)` 分桶之后**结构上不存在覆盖这件事**：两个
//! mod 的同名消息 id 落在两个不同的桶里，没有谁盖谁（这正是
//! `ll_i18n::Catalog` 这一批要修的东西）。把本地化塞进 `asset_vfs`，
//! 就要在一个专为覆盖而生的数据结构里加一条「这一类不覆盖」的例外，
//! 共享的只是「遍历 mod 目录」这个语法，不是算法。
//!
//! **本批不提供本地化覆盖机制**（「mod A 改写 mod B 的某条译文」）：今天
//! 没有需求（YAGNI），且要做也得先回答「覆盖粒度是文件还是条目」——那是
//! 独立一批。
//!
//! # 目录约定
//!
//! ```text
//! <mod 目录>/locales/<语言标签>.ftl
//! ```
//!
//! 固定目录名，不经清单声明——与 `asset_vfs` 的 `assets/` 同一条理由
//! （`knowledge/design/mod-package-structure.md`「为什么这样分」一节：
//! 「资产与本地化需要固定目录名，理由是 VFS 与发现机制必须有一个不依赖
//! 清单声明的锚点」）。
//!
//! # 本体不在本模块的视野里
//!
//! 本体的 `.ftl` 住在 `assets/locales/`，不在 `mods_root` 下，因此由
//! 调用方（`ll_game`）把它作为**又一条同构的来源**加进去，而不是本模块
//! 特判。这与 `asset_vfs::build(mods_root, base_assets_dir, base_namespace)`
//! 把本体资产根目录并列传入是同一形状。

use std::path::{Path, PathBuf};

use crate::manifest::ModManifest;
use crate::{discover, manifest, topo};

/// mod 目录下固定的本地化子目录名，见模块文档「目录约定」一节。
pub const LOCALES_DIR: &str = "locales";

/// 列出 `mods_root` 下每个**真的带了 `locales/` 目录**的 mod，产出
/// `(命名空间, 该目录的绝对路径)`。
///
/// # 为什么过滤掉没有 `locales/` 的 mod
///
/// 不过滤的话，每一个不做本地化的 mod 都会在
/// `ll_i18n::Catalog::load` 里刷一条「本地化目录不存在」的 `warn`。
/// 那条 warn 存在的意义是「你以为装载了但没有」，让它对**从来就没打算
/// 有**的情形也刷，等于把真正需要被看见的那一条淹掉。
///
/// # 顺序（C5）
///
/// 返回顺序是 [`topo::topo_sort`] 的确定性总序。**它其实不参与任何判断**
/// ——每个命名空间各自落进自己的桶，谁先谁后结果相同（`ll_i18n` 那边有
/// 一条断言专门咬住这点）。仍然走 `topo_sort` 而不是
/// [`discover::discover_mods`] 的原始顺序，是因为后者是 `fs::read_dir`
/// 的文件系统顺序，跨平台不保证一致；一个不确定的顺序即使今天不影响
/// 结果，也不该出现在返回值里（C5 的判据是「会不会被用来决定处理顺序」，
/// 而调用方将来完全可能把它写进日志或按序处理）。
///
/// 清单解析失败的候选**静默跳过**：它的 `Failed` 记录已经在
/// [`crate::pipeline`] 产出的报告里，此处重复报告只会让同一个错误在
/// 日志里出现两次（与 `asset_vfs::build` 同一条纪律）。`topo_sort`
/// 整批失败（成环/缺依赖/重复命名空间）时返回空表，理由同上：那批错误
/// 也已经有人报告了，而在依赖关系不成立的情况下装载一半的本地化只会
/// 让故障现场更难看懂。
pub fn discover_locale_dirs(mods_root: &Path) -> Vec<(String, PathBuf)> {
    let mut parsed: Vec<(PathBuf, ModManifest)> = Vec::new();
    for path in discover::discover_mods(mods_root) {
        if let Ok(parsed_manifest) = manifest::parse_manifest(&path) {
            parsed.push((path, parsed_manifest));
        }
    }

    let manifests_only: Vec<ModManifest> = parsed.iter().map(|(_, m)| m.clone()).collect();
    let Ok(order) = topo::topo_sort(&manifests_only) else {
        return Vec::new();
    };

    let mut sources = Vec::new();
    for index in order {
        let (manifest_path, manifest) = &parsed[index];
        let mod_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        let locales_dir = mod_dir.join(LOCALES_DIR);
        if !locales_dir.is_dir() {
            continue;
        }
        sources.push((manifest.id.namespace().to_string(), locales_dir));
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;
    use std::fs;

    /// 在 `root/<namespace>/` 下写一份最小清单；`languages` 非空时同时
    /// 建出 `locales/` 与对应的 `.ftl`。
    fn write_mod(root: &Path, namespace: &str, languages: &[&str]) {
        let mod_dir = root.join(namespace);
        fs::create_dir_all(&mod_dir).expect("建 mod 目录");
        fs::write(
            mod_dir.join(discover::MANIFEST_FILENAME),
            format!(r#"{{ namespace: "{namespace}", version: "0.1.0" }}"#),
        )
        .expect("写清单");
        if languages.is_empty() {
            return;
        }
        let locales = mod_dir.join(LOCALES_DIR);
        fs::create_dir_all(&locales).expect("建 locales 目录");
        for language in languages {
            fs::write(locales.join(format!("{language}.ftl")), "greeting = x\n").expect("写 ftl");
        }
    }

    #[test]
    fn 带了本地化目录的模组被发现() {
        // Arrange
        let root = tempdir();
        write_mod(root.path(), "amod", &["zh-CN", "en"]);

        // Act
        let sources = discover_locale_dirs(root.path());

        // Assert
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].0, "amod");
        assert!(sources[0].1.ends_with(LOCALES_DIR));
        assert!(sources[0].1.is_dir());
    }

    #[test]
    fn 没有本地化目录的模组不出现在结果里() {
        // 不过滤的话它会在 Catalog::load 里刷一条「目录不存在」的 warn，
        // 把真正需要被看见的那一条淹掉——见 `discover_locale_dirs` 文档。
        // Arrange
        let root = tempdir();
        write_mod(root.path(), "amod", &["zh-CN"]);
        write_mod(root.path(), "bmod", &[]);

        // Act
        let sources = discover_locale_dirs(root.path());

        // Assert
        assert_eq!(
            sources
                .iter()
                .map(|(ns, _)| ns.as_str())
                .collect::<Vec<_>>(),
            vec!["amod"]
        );
    }

    #[test]
    fn 多个模组各自带自己的本地化目录互不影响() {
        // Arrange
        let root = tempdir();
        write_mod(root.path(), "amod", &["zh-CN"]);
        write_mod(root.path(), "bmod", &["en"]);

        // Act
        let sources = discover_locale_dirs(root.path());

        // Assert
        let mut namespaces: Vec<&str> = sources.iter().map(|(ns, _)| ns.as_str()).collect();
        namespaces.sort_unstable();
        assert_eq!(namespaces, vec!["amod", "bmod"]);
        for (namespace, dir) in &sources {
            assert!(
                dir.ends_with(LOCALES_DIR) && dir.parent().is_some_and(|p| p.ends_with(namespace)),
                "{namespace} 的目录必须是它自己那一个：{dir:?}"
            );
        }
    }

    #[test]
    fn 同一份目录两次发现给出逐条相同的结果() {
        // C5：`discover_mods` 底下是 `fs::read_dir`，顺序不保证；本函数
        // 必须把它稳定化之后再交出去。
        // Arrange
        let root = tempdir();
        for namespace in ["amod", "bmod", "cmod", "dmod"] {
            write_mod(root.path(), namespace, &["zh-CN"]);
        }

        // Act
        let 第一次 = discover_locale_dirs(root.path());
        let 第二次 = discover_locale_dirs(root.path());

        // Assert
        assert_eq!(第一次, 第二次);
    }

    #[test]
    fn 清单解析失败的候选被跳过而不是让整批失败() {
        // Arrange
        let root = tempdir();
        write_mod(root.path(), "amod", &["zh-CN"]);
        let broken = root.path().join("broken");
        fs::create_dir_all(broken.join(LOCALES_DIR)).expect("建目录");
        fs::write(broken.join(discover::MANIFEST_FILENAME), "{ 这不是合法清单").expect("写坏清单");

        // Act
        let sources = discover_locale_dirs(root.path());

        // Assert
        assert_eq!(
            sources
                .iter()
                .map(|(ns, _)| ns.as_str())
                .collect::<Vec<_>>(),
            vec!["amod"]
        );
    }

    #[test]
    fn 模组根目录不存在时返回空表而不是恐慌() {
        // Arrange
        let root = tempdir();
        let missing = root.path().join("没有这个目录");

        // Act & Assert
        assert!(discover_locale_dirs(&missing).is_empty());
    }

    #[test]
    fn 仓库真实的示例模组带着自己的本地化目录() {
        // 这一条把本批的验收标的钉在真实文件上：`mods/example_mod/` 是
        // 「本体即 Mod」唯一的活证据，本批要让它第一次真的带上自己的
        // `.ftl`。删掉那个目录，本条必红。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");

        // Act
        let sources = discover_locale_dirs(&mods_root);

        // Assert
        let namespaces: Vec<&str> = sources.iter().map(|(ns, _)| ns.as_str()).collect();
        assert!(
            namespaces.contains(&"examplemod"),
            "示例 mod 必须带自己的 locales/，实测发现的是 {namespaces:?}"
        );
    }
}
