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
//!                                       register-* 内部校验失败归入
//!                                       Register 阶段（见
//!                                       `classify_script_stage` 文档，
//!                                       这是一处已知的简化）
//! ```
//!
//! 本体内容（[`crate::base_terrain::register_base_terrain`]/
//! [`crate::base_race::register_base_races`] 等 `base_*` 模块）**不经过
//! 这条管线**——它们是一次直接的 Rust 函数调用，没有清单、没有脚本，
//! 见各自模块文档。调用方应在跑本管线之前先调用一遍全部 `base_*`
//! 注册函数，两者共享同一个 [`crate::registry::Registry`] 与
//! [`GameplayTables`] 里的各张内容表。

use std::path::Path;

use ll_core::ident::NamespacedId;
use ll_script::host::{ScriptEngine, ScriptError};
use ll_world::terrain::TerrainTable;

use crate::class::ClassTable;
use crate::load_report::{LoadError, LoadReport, LoadStage, LoadStatus, SourceLocation};
use crate::manifest::{ModError, ModManifest, mod_self_id, parse_manifest};
use crate::quest::QuestTable;
use crate::race::RaceTable;
use crate::registry::Registry;
use crate::skill::SkillTable;
use crate::subclass::SubclassTable;
use crate::{discover, topo};

use crate::active_registry::{set_active_registry, take_active_registry};
use crate::script_class_api::{
    register_class_api, set_active_target as set_active_class_target,
    take_active_target as take_active_class_target,
};
use crate::script_quest_api::{
    register_quest_api, set_active_target as set_active_quest_target,
    take_active_target as take_active_quest_target,
};
use crate::script_race_api::{
    register_race_api, set_active_target as set_active_race_target,
    take_active_target as take_active_race_target,
};
use crate::script_skill_api::{
    register_skill_api, set_active_target as set_active_skill_target,
    take_active_target as take_active_skill_target,
};
use crate::script_subclass_api::{
    register_subclass_api, set_active_target as set_active_subclass_target,
    take_active_target as take_active_subclass_target,
};
use crate::script_terrain_api::{
    register_terrain_api, set_active_target as set_active_terrain_target,
    take_active_target as take_active_terrain_target,
};

/// 加载管线一次装载会话内，脚本注册函数可以写入的全部内容表——地形、
/// 职业、技能、副职、任务、种族。
///
/// 集中成一个结构体，而不是让 [`load_all`]/[`load_one_script`] 各自
/// 接收六个独立的 `&mut` 参数：这六张表在装载管线里总是同进同出（同一
/// 份 mod 脚本可能在同一个文件里先后调用 `register-terrain`/
/// `register-class`/……），拆成六个位置参数只会让调用点的参数顺序成为
/// 易错点，结构体把「这六张表必须一起传」这条约束在类型上表达出来。
/// `Registry` 不在这个结构体里——它走 [`crate::active_registry`] 单独
/// 的共享目标，理由见该模块文档。
pub struct GameplayTables<'a> {
    /// 地形表。
    pub terrain: &'a mut TerrainTable,
    /// 职业表。
    pub class: &'a mut ClassTable,
    /// 技能表。
    pub skill: &'a mut SkillTable,
    /// 副职表。
    pub subclass: &'a mut SubclassTable,
    /// 任务表。
    pub quest: &'a mut QuestTable,
    /// 种族表。
    pub race: &'a mut RaceTable,
}

/// 跑一次完整的 mod 装载会话：发现 `mods_root` 下的候选、解析、拓扑
/// 排序、按序加载脚本、注册内容——写入 `registry`/`tables`，返回一份
/// 报告。
///
/// `registry`/`tables` 应当已经装过本体内容（`register_base_terrain`/
/// `register_base_races` 等）：本函数只管 mod 目录，不知道、也不需要
/// 知道本体是怎么注册进去的——这正是「本体即 Mod」在管线层面的体现：
/// 本体的注册发生在调用本函数**之前**的一次独立调用，mod 内容随后
/// intern 进同一个 `Registry`，两者共用同一段单调递增的 `ContentIndex`
/// 号段（见 `crate::base_terrain` 模块文档与其测试）。
pub fn load_all(
    mods_root: &Path,
    registry: &mut Registry,
    tables: &mut GameplayTables,
) -> LoadReport {
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
            if let Err(err) = load_one_script(manifest, entry, registry, tables) {
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
    let mut terrain = TerrainTable::new();
    let mut class = ClassTable::new();
    let mut skill = SkillTable::new();
    let mut subclass = SubclassTable::new();
    let mut quest = QuestTable::new();
    let mut race = RaceTable::new();
    let mut tables = GameplayTables {
        terrain: &mut terrain,
        class: &mut class,
        skill: &mut skill,
        subclass: &mut subclass,
        quest: &mut quest,
        race: &mut race,
    };
    for entry in &manifest.entry_points {
        if let Err(err) = load_one_script(&manifest, entry, &mut registry, &mut tables) {
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
/// 依赖方、环路上的成员、版本不兼容的依赖方与依赖目标）拿到具体原因，
/// 其余的拿到一条「因为别的 mod 导致整批中止」的说明。不让任何一个
/// 已经成功解析的候选从报告里悄悄消失——它们确实没能加载成功，报告
/// 应当如实反映。
fn attribute_topo_error(report: &mut LoadReport, parsed: &[ModManifest], err: &ModError) {
    let culprits: Vec<&NamespacedId> = match err {
        ModError::DuplicateNamespace(id) => parsed
            .iter()
            .filter(|m| m.id.namespace() == id.namespace())
            .map(|m| &m.id)
            .collect(),
        ModError::MissingDependency(missing) => parsed
            .iter()
            .filter(|m| {
                m.dependencies
                    .iter()
                    .any(|dep| dep.namespace == missing.namespace())
            })
            .map(|m| &m.id)
            .collect(),
        ModError::CyclicDependency(cycle) => parsed
            .iter()
            .filter(|m| cycle.contains(&m.id))
            .map(|m| &m.id)
            .collect(),
        // 版本不兼容牵涉两方：声明了约束的依赖方，与版本对不上的依赖
        // 目标——两者都拿到具体原因，不只是"发起方"一个人的事。
        ModError::IncompatibleDependencyVersion(detail) => parsed
            .iter()
            .filter(|m| m.id == detail.dependent || m.id == detail.dependency)
            .map(|m| &m.id)
            .collect(),
        // topo_sort 只会产出以上四种错误，其余两个变体（Io/ParseError）
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
/// `register-terrain`/`register-class`/`register-skill`/
/// `register-subclass`/`register-quest`/`register-race` 的效果已经写进
/// `registry`/`tables`。
fn load_one_script(
    manifest: &ModManifest,
    entry: &Path,
    registry: &mut Registry,
    tables: &mut GameplayTables,
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

    // 把 registry 与全部六张表整体移进各自的线程局部存储，供对应的
    // register-* 函数在脚本求值期间写入；脚本跑完（不论成功失败）都要
    // 原样移回——`ScriptEngine::load_source` 本身不会 panic（四道防线
    // ①②），这里不需要 catch_unwind 之类的补救。`Registry` 走
    // `crate::active_registry` 的共享目标（六个 register-* 函数必须
    // 共用同一个 `Registry` 实例，理由见该模块文档），六张表各自走
    // 自己模块的 `thread_local!`。
    set_active_registry(std::mem::take(registry));
    set_active_terrain_target(std::mem::take(tables.terrain));
    set_active_class_target(std::mem::take(tables.class));
    set_active_skill_target(std::mem::take(tables.skill));
    set_active_subclass_target(std::mem::take(tables.subclass));
    set_active_quest_target(std::mem::take(tables.quest));
    set_active_race_target(std::mem::take(tables.race));

    let mut engine = ScriptEngine::new();
    register_terrain_api(&mut engine);
    register_class_api(&mut engine);
    register_skill_api(&mut engine);
    register_subclass_api(&mut engine);
    register_quest_api(&mut engine);
    register_race_api(&mut engine);
    let result = engine.load_source(source.clone());

    *registry = take_active_registry();
    *tables.terrain = take_active_terrain_target();
    *tables.class = take_active_class_target();
    *tables.skill = take_active_skill_target();
    *tables.subclass = take_active_subclass_target();
    *tables.quest = take_active_quest_target();
    *tables.race = take_active_race_target();

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
/// **已知简化**：本管线注册给脚本的、会产生副作用的能力现在有六个
/// （`register-terrain`/`register-class`/`register-skill`/
/// `register-subclass`/`register-quest`/`register-race`），把
/// `ScriptError::Runtime`（任一 `register-*` 内部校验失败时都走这一
/// 类，见各自模块文档「返回 Result<bool, String>」一节）整体归为
/// Register 阶段。这会把一个与内容注册无关、纯粹是脚本自身写错的运行
/// 时错误（比如引用了一个已声明但尚未 `define` 的变量）也误标成
/// Register——原始简化写下时只有 `register-terrain` 一个注册函数，
/// 补齐另外五个之后这条简化本身没有变得更精确（六个函数的运行时错误
/// 依然与「脚本自身写错」共用同一个 `ScriptError::Runtime` 变体，无法
/// 从错误类型本身区分），仍然是一处已知的简化，不是本批次修掉的缺口
/// ——若未来需要更精确的判据，需要让每个注册函数把自己的错误包一层
/// 可辨识的前缀。
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
    use ll_world::terrain::TerrainKind;
    use std::fs;

    /// 测试帮手：现造一套全新的空内容表，供 [`GameplayTables`] 借用——
    /// 各测试只关心地形（`register-terrain` 仍是既有场景里用得最多的
    /// 一类），但 `load_all` 的签名要求六张表一起传，本结构体把「造出
    /// 六个空表」这件事集中成一次调用，不必在每条测试里重复六行。
    #[derive(Default)]
    struct OwnedTables {
        terrain: TerrainTable,
        class: ClassTable,
        skill: SkillTable,
        subclass: SubclassTable,
        quest: QuestTable,
        race: RaceTable,
    }

    impl OwnedTables {
        fn as_gameplay_tables(&mut self) -> GameplayTables<'_> {
            GameplayTables {
                terrain: &mut self.terrain,
                class: &mut self.class,
                skill: &mut self.skill,
                subclass: &mut self.subclass,
                quest: &mut self.quest,
                race: &mut self.race,
            }
        }
    }

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
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

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
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

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
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

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
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        match &report.entries[0].1 {
            LoadStatus::Failed(err) => assert_eq!(err.stage, LoadStage::Topo),
            other => panic!("期望 Topo 阶段的 Failed，实际 {other:?}"),
        }
    }

    #[test]
    fn 依赖版本不兼容导致整批加载在topo阶段中止() {
        // Arrange：needs_new_provider 要求 provider >=2.0，但 provider
        // 实际只有 1.0.0——整批中止，provider 自己虽然没有声明任何
        // 依赖，也应该出现在报告里且被标记为 Failed（见
        // attribute_topo_error 文档「整批中止」一节）。
        let root = tempdir();
        write_mod(
            root.path(),
            "needs_new_provider",
            r#"
            namespace = "needs_new_provider"
            version = "0.1.0"

            [dependencies]
            provider = ">=2.0"
            "#,
            None,
        );
        write_mod(
            root.path(),
            "provider",
            r#"
            namespace = "provider"
            version = "1.0.0"
            "#,
            None,
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert：两个 mod 都没能加载成功，都归入 Topo 阶段的 Failed。
        assert_eq!(report.entries.len(), 2);
        for (_, status) in &report.entries {
            match status {
                LoadStatus::Failed(err) => assert_eq!(err.stage, LoadStage::Topo),
                other => panic!("期望 Topo 阶段的 Failed，实际 {other:?}"),
            }
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
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

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
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

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
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

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
    fn 单个脚本内连续调用六种注册函数全部真实写进各自的表() {
        // 端到端验证：一个 mod 脚本在同一个文件里依次调用
        // register-terrain/register-class/register-skill/
        // register-subclass/register-quest/register-race，六次调用
        // 必须落在同一个 Registry 上（否则 ContentIndex 会撞车），且
        // 六张表各自都收到了正确的内容——这是 crate::active_registry
        // 模块文档论证的那个「必须共享同一个 Registry」场景的直接回归。
        //
        // 这是「mod 脚本调得到这套 API」的真正证据——本测试走的是真实
        // 的 `.scm` 源码文本经 `ScriptEngine::load_source` 解析执行，
        // 不是在 Rust 里直接调用 `Registry::intern`/`*Table::define`。
        // 与之互补的另一半是「结构等价」测试（本体注册与 mod 注册走
        // 同一条注册路径、注册表内部不给本体开后门），分布在
        // `base_placeholder.rs`/`base_race.rs`/`base_terrain.rs`/
        // `base_space_profile.rs`/`class.rs`/`quest.rs`/`race.rs`/
        // `skill.rs`/`subclass.rs`（以及 `ll-world` 的
        // `space_profile.rs`）各自的单元测试里——那批测试只证明「本体
        // 与 mod 内容在 Rust 类型层面无法区分」，不能单独证明脚本可达，
        // 两类证据合起来才是完整的「玩法层 API 完备性」验收，见本
        // 模块顶部「本体内容……不经过这条管线」一节与 ADR 0018。
        // 真实使用中的完整示例见 `mods/example_mod/gameplay.scm`。
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "gameplay",
            r#"
            namespace = "gameplay"
            version = "0.1.0"
            entry_points = ["main.scm"]
            "#,
            Some(
                r#"
                (register-terrain "gameplay:lava_floor" #f #f 350 "")
                (register-class "gameplay:necromancer" "gameplay:necromancer_display_name" "willpower")
                (register-subclass "gameplay:shadowdancer" "gameplay:shadowdancer_display_name")
                (register-skill "gameplay:frostbolt" "" (list) 25 "mana" 12 "deal-damage" "" 15 0)
                (register-quest "gameplay:kill_goblins" (list) "kill-count" "gameplay:goblin" 3)
                (register-race "gameplay:half_elf" "gameplay:half_elf_display_name" 0 1 0 0 0 1 0 1 1 150)
                "#,
            ),
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert：mod 整体加载成功。
        assert_eq!(
            report.entries,
            vec![(mod_self_id("gameplay").unwrap(), LoadStatus::Loaded)]
        );
        // 六类内容各自都能在对应的表里查到——不是只注册进了 Registry
        // 却没有写进属性表。
        let terrain_index = registry
            .get(&NamespacedId::parse("gameplay:lava_floor").unwrap())
            .expect("地形应已注册");
        assert_eq!(
            TerrainKind::from_index(terrain_index).move_cost(&owned.terrain),
            350
        );

        let class_index = registry
            .get(&NamespacedId::parse("gameplay:necromancer").unwrap())
            .expect("职业应已注册");
        assert!(owned.class.get(class_index).is_some());

        let subclass_index = registry
            .get(&NamespacedId::parse("gameplay:shadowdancer").unwrap())
            .expect("副职应已注册");
        assert!(owned.subclass.get(subclass_index).is_some());

        let skill_index = registry
            .get(&NamespacedId::parse("gameplay:frostbolt").unwrap())
            .expect("技能应已注册");
        assert!(owned.skill.get(skill_index).is_some());

        let quest_index = registry
            .get(&NamespacedId::parse("gameplay:kill_goblins").unwrap())
            .expect("任务应已注册");
        assert!(owned.quest.get(quest_index).is_some());

        let race_index = registry
            .get(&NamespacedId::parse("gameplay:half_elf").unwrap())
            .expect("种族应已注册");
        assert!(owned.race.get(race_index).is_some());
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
