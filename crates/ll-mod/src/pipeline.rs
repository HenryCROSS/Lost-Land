//! 加载管线：把 [`crate::discover`]/[`crate::manifest`]/[`crate::topo`]/
//! `ll_script`/[`crate::registry`] 串成规格 §10.6 描述的完整流程，产出
//! 一份 [`crate::load_report::LoadReport`]。
//!
//! # 完整调用链
//!
//! ```text
//! discover_mods(root)              -> 候选清单路径
//!   -> parse_manifest(path)（逐个，互不影响）-> 已解析清单/Parse 阶段失败
//!   -> topo_sort(已解析清单)         -> 加载顺序，或 Topo 阶段失败（整批中止）
//!   -> 按顺序逐个 mod：
//!        入口为空                   -> Loaded（纯数据 mod）
//!        否则逐个 .scm 入口：
//!          读文件                   -> IO 失败归入 LoadScript 阶段
//!          ScriptEngine::load_source -> 语法错误/白名单拒绝/超时/缺参
//!                                       归入 LoadScript 阶段；
//!                                       register-terrain 内部校验失败
//!                                       归入 Register 阶段（见
//!                                       `classify_script_stage` 文档，
//!                                       这是一处已知的简化）
//! ```
//!
//! 本体地形（[`crate::base_terrain::register_base_terrain`]）**不经过
//! 这条管线**——它是一次直接的 Rust 函数调用，没有清单、没有脚本，见
//! 该模块文档。调用方应在跑本管线之前先调用一次
//! `register_base_terrain`，两者共享同一个 [`crate::registry::Registry`]
//! 与 `ll_world::terrain::TerrainTable`。

use std::path::Path;

use ll_core::ident::NamespacedId;
use ll_script::host::{ScriptEngine, ScriptError};
use ll_world::terrain::TerrainTable;

use crate::load_report::{LoadError, LoadReport, LoadStage, LoadStatus, SourceLocation};
use crate::manifest::{ModError, ModManifest, mod_self_id, parse_manifest};
use crate::registry::Registry;
use crate::script_terrain_api::{register_terrain_api, set_active_target, take_active_target};
use crate::{discover, topo};

/// 跑一次完整的 mod 装载会话：发现 `mods_root` 下的候选、解析、拓扑
/// 排序、按序加载脚本、注册内容——写入 `registry`/`table`，返回一份
/// 报告。
///
/// `registry`/`table` 应当已经装过本体内容（`register_base_terrain`）：
/// 本函数只管 mod 目录，不知道、也不需要知道本体是怎么注册进去的——
/// 这正是「本体即 Mod」在管线层面的体现：本体的注册发生在调用本函数
/// **之前**的一次独立调用，mod 内容随后 intern 进同一个 `Registry`，
/// 两者共用同一段单调递增的 `ContentIndex` 号段（见
/// `crate::base_terrain` 模块文档与其测试）。
pub fn load_all(mods_root: &Path, registry: &mut Registry, table: &mut TerrainTable) -> LoadReport {
    let mut report = LoadReport::new();

    let candidates = discover::discover_mods(mods_root);
    let mut parsed: Vec<ModManifest> = Vec::new();
    for path in &candidates {
        match parse_manifest(path) {
            Ok(manifest) => parsed.push(manifest),
            Err(err) => {
                let mod_id = best_effort_mod_id(path);
                report.push(
                    mod_id.clone(),
                    LoadStatus::Failed(LoadError {
                        mod_id,
                        stage: LoadStage::Parse,
                        message: err.to_string(),
                        location: Some(SourceLocation {
                            file: path.clone(),
                            line: None,
                        }),
                    }),
                );
            }
        }
    }

    let order = match topo::topo_sort(&parsed) {
        Ok(order) => order,
        Err(err) => {
            attribute_topo_error(&mut report, &parsed, &err);
            return report;
        }
    };

    for idx in order {
        let manifest = &parsed[idx];
        if manifest.entry_points.is_empty() {
            // 纯数据 mod（清单允许没有脚本入口，见 manifest.rs 文档），
            // 没有脚本可跑，直接算加载成功。
            report.push(manifest.id.clone(), LoadStatus::Loaded);
            continue;
        }

        let mut failure = None;
        for entry in &manifest.entry_points {
            if let Err(err) = load_one_script(manifest, entry, registry, table) {
                failure = Some(err);
                break;
            }
        }

        match failure {
            Some(err) => report.push(manifest.id.clone(), LoadStatus::Failed(err)),
            None => report.push(manifest.id.clone(), LoadStatus::Loaded),
        }
    }

    report
}

/// 单个 mod 的「最小可行」一键重载（简报「单个 mod 一键重载」的诚实
/// 范围：重新对该 mod 跑一次「解析→加载→注册」）。
///
/// # 为什么不写回正在运行的会话 `Registry`
///
/// 若重新对同一个 `id` 调用 `register-terrain`，`Registry::intern` 本身
/// 是幂等的（返回同一个索引），但 `TerrainTable::define` **拒绝重复
/// 定义**（本任务修掉的已知缺口，见 `crate::topo` 模块文档）——这意味
/// 着任何已经成功加载过一次的 mod，只要重载就会立刻在第二次
/// `register-terrain` 调用上撞见「重复定义」，即使脚本内容完全没有
/// 问题。这不是本函数的 bug，是「重复定义拒绝」与「原地重载复用同一
/// 注册表」两条设计天然冲突——P4 明确不做真正的热重载（存盘即生效，
/// 见任务简报「本阶段范围」），本函数因此改为对一份**全新的**空
/// `Registry`/`TerrainTable` 重新跑一遍该 mod 的解析与脚本，验证「这个
/// mod 自己能不能干净地加载」，不去动正在运行的游戏会话状态——这是
/// 「最小可行版本」在两种都不完美的选项之间选出的更诚实的一个：给出
/// 「这个 mod 现在能不能加载成功」这个真实、可信的信号，而不是一个
/// 每次都因为设计原因失败的假信号。
pub fn reload_mod(manifest_path: &Path) -> LoadStatus {
    let manifest = match parse_manifest(manifest_path) {
        Ok(manifest) => manifest,
        Err(err) => {
            let mod_id = best_effort_mod_id(manifest_path);
            return LoadStatus::Failed(LoadError {
                mod_id,
                stage: LoadStage::Parse,
                message: err.to_string(),
                location: Some(SourceLocation {
                    file: manifest_path.to_path_buf(),
                    line: None,
                }),
            });
        }
    };

    if manifest.entry_points.is_empty() {
        return LoadStatus::Loaded;
    }

    let mut registry = Registry::new();
    let mut table = TerrainTable::new();
    for entry in &manifest.entry_points {
        if let Err(err) = load_one_script(&manifest, entry, &mut registry, &mut table) {
            return LoadStatus::Failed(err);
        }
    }
    LoadStatus::Loaded
}

/// 从 `mod.toml` 所在目录名推出一个尽力而为的 mod 身份，供清单本身都
/// 解析失败时仍能在报告里有个地方挂靠——与 `crate::topo` 里
/// `check_missing_dependencies` 同一套「解析恒不失败，万一失败退化成
/// 固定占位符」的降级写法（见其文档），不是本模块发明的新约定。
fn best_effort_mod_id(manifest_path: &Path) -> NamespacedId {
    let dir_name = manifest_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("invalid");
    mod_self_id(dir_name)
        .unwrap_or_else(|_| mod_self_id("invalid").expect("固定字面量 invalid 恒合法"))
}

/// `topo_sort` 失败时整批加载中止——把失败原因分摊到 `parsed` 里的每
/// 一个清单：直接牵涉的（重复命名空间的两个命名空间、缺失依赖的
/// 依赖方、环路上的成员）拿到具体原因，其余的拿到一条「因为别的 mod
/// 导致整批中止」的说明。不让任何一个已经成功解析的候选从报告里
/// 悄悄消失——它们确实没能加载成功，报告应当如实反映。
fn attribute_topo_error(report: &mut LoadReport, parsed: &[ModManifest], err: &ModError) {
    let culprits: Vec<&NamespacedId> = match err {
        ModError::DuplicateNamespace(id) => parsed
            .iter()
            .filter(|m| m.id.namespace() == id.namespace())
            .map(|m| &m.id)
            .collect(),
        ModError::MissingDependency(missing) => parsed
            .iter()
            .filter(|m| m.dependencies.iter().any(|dep| dep == missing.namespace()))
            .map(|m| &m.id)
            .collect(),
        ModError::CyclicDependency(cycle) => parsed
            .iter()
            .filter(|m| cycle.contains(&m.id))
            .map(|m| &m.id)
            .collect(),
        // topo_sort 只会产出以上三种错误，其余两个变体（Io/ParseError）
        // 只可能来自 parse_manifest，这里防御性地不牵连任何清单。
        ModError::Io(_) | ModError::ParseError(_) => Vec::new(),
    };

    for manifest in parsed {
        let is_culprit = culprits.iter().any(|id| **id == manifest.id);
        let message = if is_culprit {
            err.to_string()
        } else {
            format!("因其他 mod 的依赖拓扑排序失败而中止加载：{err}")
        };
        report.push(
            manifest.id.clone(),
            LoadStatus::Failed(LoadError {
                mod_id: manifest.id.clone(),
                stage: LoadStage::Topo,
                message,
                location: None,
            }),
        );
    }
}

/// 加载单个脚本文件：读文件、跑 `ScriptEngine::load_source`，成功时
/// `register-terrain` 的效果已经写进 `registry`/`table`。
fn load_one_script(
    manifest: &ModManifest,
    entry: &Path,
    registry: &mut Registry,
    table: &mut TerrainTable,
) -> Result<(), LoadError> {
    let source = std::fs::read_to_string(entry).map_err(|io_err| LoadError {
        mod_id: manifest.id.clone(),
        stage: LoadStage::LoadScript,
        message: format!("读取脚本文件失败：{io_err}"),
        location: Some(SourceLocation {
            file: entry.to_path_buf(),
            line: None,
        }),
    })?;

    // 把 registry/table 整体移进线程局部存储，供 register-terrain 在
    // 脚本求值期间写入；脚本跑完（不论成功失败）都要原样移回——
    // `ScriptEngine::load_source` 本身不会 panic（四道防线①②），这里
    // 不需要 catch_unwind 之类的补救。
    let owned_registry = std::mem::take(registry);
    let owned_table = std::mem::take(table);
    set_active_target(owned_registry, owned_table);

    let mut engine = ScriptEngine::new();
    register_terrain_api(&mut engine);
    let result = engine.load_source(source.clone());

    let (restored_registry, restored_table) = take_active_target();
    *registry = restored_registry;
    *table = restored_table;

    result.map_err(|script_err| LoadError {
        mod_id: manifest.id.clone(),
        stage: classify_script_stage(&script_err),
        message: script_err.to_string(),
        location: Some(SourceLocation {
            file: entry.to_path_buf(),
            line: script_err
                .byte_offset()
                .map(|offset| line_number(&source, offset)),
        }),
    })
}

/// 把 [`ScriptError`] 归到 [`LoadStage::LoadScript`] 还是
/// [`LoadStage::Register`]。
///
/// **已知简化**：本管线当前唯一注册给脚本的、会产生副作用的能力就是
/// `register-terrain`（见 `crate::script_terrain_api`），因此把
/// `ScriptError::Runtime`（`register-terrain` 内部校验失败时正是走这一
/// 类，见其文档「返回 Result<bool, String>」一节）整体归为 Register
/// 阶段。这会把一个与内容注册无关、纯粹是脚本自身写错的运行时错误
/// （比如引用了一个已声明但尚未 `define` 的变量）也误标成 Register——
/// 只要脚本能引用的副作用函数还只有 `register-terrain` 一个，这个简化
/// 就不会产生错误归类；一旦本项目后续给脚本注册第二个有副作用的函数
/// （比如未来的技能/物品注册），这里需要回来改成更精确的判据（例如让
/// 每个注册函数把自己的错误包一层可辨识的前缀）。
fn classify_script_stage(err: &ScriptError) -> LoadStage {
    match err {
        ScriptError::Runtime(..) => LoadStage::Register,
        ScriptError::ParseError(..) | ScriptError::ArityMismatch(..) | ScriptError::Interrupted => {
            LoadStage::LoadScript
        }
    }
}

/// 把源码里的字节偏移量换算成从 1 开始的行号。
///
/// 按字节而非按字符扫描（`source.as_bytes()`），不做字符串切片——切片
/// 要求偏移量落在合法的 UTF-8 字符边界上，而 `byte_offset` 来自 Steel
/// 内部的 token/AST 节点位置，理论上恒是合法边界，但这里不依赖这个
/// 假设：越界或落在字符中间时用 `min` 钳位、按字节比较 `\n`，两种写法
/// 都不会 panic。
fn line_number(source: &str, byte_offset: u32) -> u32 {
    let end = (byte_offset as usize).min(source.len());
    source.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;
    use std::fs;

    /// 在 `root` 下建一个候选 mod 子目录，写入清单与（可选）脚本。
    fn write_mod(root: &Path, dir_name: &str, manifest_toml: &str, script: Option<&str>) {
        let mod_dir = root.join(dir_name);
        fs::create_dir_all(&mod_dir).expect("创建 mod 子目录");
        fs::write(mod_dir.join("mod.toml"), manifest_toml).expect("写入清单");
        if let Some(source) = script {
            fs::write(mod_dir.join("main.scm"), source).expect("写入脚本");
        }
    }

    #[test]
    fn 纯数据mod没有脚本入口时直接判定为已加载() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "puredata",
            r#"
            namespace = "puredata"
            version = "0.1.0"
            "#,
            None,
        );
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let report = load_all(root.path(), &mut registry, &mut table);

        // Assert
        assert_eq!(
            report.entries,
            vec![(mod_self_id("puredata").unwrap(), LoadStatus::Loaded)]
        );
    }

    #[test]
    fn 合法脚本mod加载成功且内容真的写进注册表() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "examplemod",
            r#"
            namespace = "examplemod"
            version = "0.1.0"
            entry_points = ["main.scm"]
            "#,
            Some(r#"(register-terrain "examplemod:lava_floor" #f #t 4294967295 "")"#),
        );
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let report = load_all(root.path(), &mut registry, &mut table);

        // Assert
        assert_eq!(
            report.entries,
            vec![(mod_self_id("examplemod").unwrap(), LoadStatus::Loaded)]
        );
        assert!(
            registry
                .get(&NamespacedId::parse("examplemod:lava_floor").unwrap())
                .is_some()
        );
    }

    #[test]
    fn 语法错误脚本归入loadscript阶段且不影响其它mod() {
        // Arrange：broken 语法错误，good 完全正常——验证阶段隔离。
        let root = tempdir();
        write_mod(
            root.path(),
            "broken",
            r#"
            namespace = "broken"
            version = "0.1.0"
            entry_points = ["main.scm"]
            "#,
            Some("(+ 1 2"),
        );
        write_mod(
            root.path(),
            "good",
            r#"
            namespace = "good"
            version = "0.1.0"
            "#,
            None,
        );
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let report = load_all(root.path(), &mut registry, &mut table);

        // Assert
        let broken_status = report
            .entries
            .iter()
            .find(|(id, _)| id.namespace() == "broken")
            .map(|(_, status)| status);
        match broken_status {
            Some(LoadStatus::Failed(err)) => assert_eq!(err.stage, LoadStage::LoadScript),
            other => panic!("期望 broken 归入 LoadScript 阶段的 Failed，实际 {other:?}"),
        }
        let good_status = report
            .entries
            .iter()
            .find(|(id, _)| id.namespace() == "good")
            .map(|(_, status)| status);
        assert_eq!(good_status, Some(&LoadStatus::Loaded));
    }

    #[test]
    fn 缺失依赖导致整批加载在topo阶段中止() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "needs_ghost",
            r#"
            namespace = "needs_ghost"
            version = "0.1.0"
            dependencies = ["ghost"]
            "#,
            None,
        );
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let report = load_all(root.path(), &mut registry, &mut table);

        // Assert
        match &report.entries[0].1 {
            LoadStatus::Failed(err) => assert_eq!(err.stage, LoadStage::Topo),
            other => panic!("期望 Topo 阶段的 Failed，实际 {other:?}"),
        }
    }

    #[test]
    fn 清单本身解析失败归入parse阶段() {
        // Arrange：namespace 含非法大写字符。
        let root = tempdir();
        write_mod(
            root.path(),
            "BadNamespace",
            r#"
            namespace = "BadNamespace"
            version = "0.1.0"
            "#,
            None,
        );
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let report = load_all(root.path(), &mut registry, &mut table);

        // Assert
        assert_eq!(report.entries.len(), 1);
        match &report.entries[0].1 {
            LoadStatus::Failed(err) => assert_eq!(err.stage, LoadStage::Parse),
            other => panic!("期望 Parse 阶段的 Failed，实际 {other:?}"),
        }
    }

    #[test]
    fn 白名单拒绝的脚本归入loadscript阶段() {
        // Arrange：尝试 require-builtin steel/time——文本层前置优化与
        // AST 白名单都会拦这个，落点都是 ParseError -> LoadScript。
        let root = tempdir();
        write_mod(
            root.path(),
            "sneaky",
            r#"
            namespace = "sneaky"
            version = "0.1.0"
            entry_points = ["main.scm"]
            "#,
            Some("(require-builtin steel/time)\n(instant/now)"),
        );
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let report = load_all(root.path(), &mut registry, &mut table);

        // Assert
        match &report.entries[0].1 {
            LoadStatus::Failed(err) => assert_eq!(err.stage, LoadStage::LoadScript),
            other => panic!("期望 LoadScript 阶段的 Failed，实际 {other:?}"),
        }
    }

    #[test]
    fn 语法错误的行号定位到脚本第二行() {
        // Arrange：第一行合法，第二行少一个右括号。
        let root = tempdir();
        write_mod(
            root.path(),
            "twoline",
            r#"
            namespace = "twoline"
            version = "0.1.0"
            entry_points = ["main.scm"]
            "#,
            Some("(define x 1)\n(+ 1 2"),
        );
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();

        // Act
        let report = load_all(root.path(), &mut registry, &mut table);

        // Assert
        match &report.entries[0].1 {
            LoadStatus::Failed(err) => {
                let line = err.location.as_ref().and_then(|loc| loc.line);
                assert_eq!(line, Some(2));
            }
            other => panic!("期望带行号的 Failed，实际 {other:?}"),
        }
    }

    #[test]
    fn reload_mod对干净的mod返回loaded() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "examplemod",
            r#"
            namespace = "examplemod"
            version = "0.1.0"
            entry_points = ["main.scm"]
            "#,
            Some(r#"(register-terrain "examplemod:lava_floor" #f #t 4294967295 "")"#),
        );

        // Act
        let status = reload_mod(&root.path().join("examplemod").join("mod.toml"));

        // Assert
        assert_eq!(status, LoadStatus::Loaded);
    }

    #[test]
    fn reload_mod对语法错误的mod返回failed而不影响调用方状态() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "broken",
            r#"
            namespace = "broken"
            version = "0.1.0"
            entry_points = ["main.scm"]
            "#,
            Some("(+ 1 2"),
        );

        // Act
        let status = reload_mod(&root.path().join("broken").join("mod.toml"));

        // Assert
        assert!(matches!(status, LoadStatus::Failed(_)));
    }

    #[test]
    fn line_number在偏移量为零时返回第一行() {
        // Arrange & Act & Assert
        assert_eq!(line_number("abc", 0), 1);
    }

    #[test]
    fn line_number越界偏移量不panic并钳位到最后一行() {
        // Arrange & Act & Assert
        assert_eq!(line_number("a\nb", 999), 2);
    }
}
