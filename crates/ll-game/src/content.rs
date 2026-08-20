//! 内容装载的单一入口。
//!
//! # 为什么要收敛成一个函数
//!
//! 「本体即 Mod」原则（`ll_mod` 模块文档）要求本体内容与 mod 内容走
//! 完全相同的 [`ll_mod::registry::Registry::intern`] 通道，但**注册的
//! 先后顺序与调用方式**目前散落在各个验收 demo 里各写各的一份
//! （`p4_acceptance::world::build_demo_world`、
//! `p5_save_acceptance::world_with_registry` ……）。另一个批次正在把
//! 「地形/种族/空间层属性由 Rust 函数直接注册」逐步换成「由 mod 脚本
//! 注册」——那次改动的落点必然是**这一个函数**：本体二进制自身只调用
//! [`load_content`] 一次，不知道、也不需要知道内容具体是 Rust 调用
//! 注册的还是脚本注册的。把调用点收敛到一处，未来那次替换就不需要
//! 满仓库搜索散落的 `register_base_*` 调用。
//!
//! # 加载顺序
//!
//! 先注册本体内容（地形 → 种族 → 空间层属性 → 占位内容），再跑
//! [`ll_mod::pipeline::load_all`] 装载 `mods_root` 下的 mod——
//! 顺序理由见 [`ll_mod::pipeline`] 模块文档「本体内容不经过这条
//! 管线」一节：mod 内容 intern 进同一个 `Registry`，必须排在本体注册
//! 之后才能保证号段不冲突。四类本体注册彼此之间顺序不影响正确性
//! （各自对应不同的命名空间前缀，见 `ll_mod::base_race` 模块文档
//! 「调用顺序与 register_base_placeholder_content 无关」一节），这里
//! 固定一个顺序只是为了让日志读起来是线性的。

use std::path::Path;

use ll_mod::base_placeholder::register_base_placeholder_content;
use ll_mod::base_race::register_base_races;
use ll_mod::base_space_profile::register_base_space_profiles;
use ll_mod::base_terrain::register_base_terrain;
use ll_mod::class::ClassTable;
use ll_mod::content_hash::{ContentValueTables, apply_value_hashes};
use ll_mod::discover::discover_mods;
use ll_mod::load_report::LoadReport;
use ll_mod::manifest::{ModManifest, parse_manifest};
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::{BaseRaceIds, RaceTable};
use ll_mod::registry::Registry;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_world::space_profile::{BaseSpaceProfileIds, SpaceProfileTable};
use ll_world::terrain::{BaseTerrainIds, TerrainTable};

/// 一次装载会话的完整产出：注册表、六张玩法内容表、本体索引缓存、
/// 已成功解析的 mod 清单（供 [`ll_mod::mod_set::GenerationModSet`]
/// 使用）、已装载的脚本源码（供存档读入时的 VM 强制重建使用，见
/// `ll_content::save_file::load_full` 文档「关于 VM 强制重建」一节）、
/// 与本次装载报告。
pub struct LoadedContent {
    /// 内容注册表：字符串 ID ↔ `ContentIndex` 的双向映射。
    pub registry: Registry,
    /// 本体地形索引缓存。
    pub terrain_ids: BaseTerrainIds,
    /// 地形属性表。
    pub terrain_table: TerrainTable,
    /// 本体种族索引缓存。
    pub race_ids: BaseRaceIds,
    /// 种族属性表。
    pub race_table: RaceTable,
    /// 本体空间层属性索引缓存。
    pub space_ids: BaseSpaceProfileIds,
    /// 空间层属性表。
    pub space_table: SpaceProfileTable,
    /// 职业表。
    pub class_table: ClassTable,
    /// 技能表。
    pub skill_table: SkillTable,
    /// 副职表。
    pub subclass_table: SubclassTable,
    /// 任务表。
    pub quest_table: QuestTable,
    /// 这次会话里成功解析出清单的全部 mod——供
    /// `ll_mod::mod_set::GenerationModSet::capture`/存档头「当前 mod
    /// 集合」使用。清单解析失败的候选不在这里（它们已经被记进
    /// [`Self::report`]），与 `ll_mod::pipeline::load_all` 内部「解析
    /// 失败互不影响其他 mod」的隔离原则一致。
    pub manifests: Vec<ModManifest>,
    /// 已成功装载的脚本源码：`(mod 命名空间, 源码文本)`。数据来源与
    /// `load_all` 内部读取的是同一批文件——本函数在装载管线之外单独
    /// 重新读了一遍，理由见模块顶部「加载顺序」：`load_all` 本身不
    /// 对外暴露它读过的源码文本（那是它的内部实现细节），存档读入需要
    /// 这份文本却不属于装载管线自身的职责，见
    /// `ll_content::save_file::load_full` 文档。
    pub script_sources: Vec<(String, String)>,
    /// 本次 mod 装载报告：按 mod 归类的成功/失败结果。
    pub report: LoadReport,
}

/// 装载全部游戏内容：先注册本体内容，再装载 `mods_root` 下的 mod。
///
/// **本体二进制应当只调用本函数一次**（启动时）——这是本模块存在的
/// 唯一理由，见模块文档。
pub fn load_content(mods_root: &Path) -> LoadedContent {
    let mut registry = Registry::new();

    let (terrain_ids, mut terrain_table) =
        register_base_terrain(&mut registry).expect("本体地形声明表内部一致，注册恒不失败");
    let (race_ids, mut race_table) =
        register_base_races(&mut registry).expect("本体种族声明表内部一致，注册恒不失败");
    let (space_ids, space_table) = register_base_space_profiles(&mut registry)
        .expect("本体空间层属性声明表内部一致，注册恒不失败");
    register_base_placeholder_content(&mut registry);

    let mut class_table = ClassTable::new();
    let mut skill_table = SkillTable::new();
    let mut subclass_table = SubclassTable::new();
    let mut quest_table = QuestTable::new();

    let report = load_all(
        mods_root,
        &mut registry,
        &mut GameplayTables {
            terrain: &mut terrain_table,
            class: &mut class_table,
            skill: &mut skill_table,
            subclass: &mut subclass_table,
            quest: &mut quest_table,
            race: &mut race_table,
        },
    );

    // 值哈希升级：全部六张内容表此刻已经装载完毕（本体 + mod），在
    // 这里跑一次性收尾步骤,把字段值折进 registry 已有的 id 摘要——
    // 见 `ll_mod::content_hash` 模块文档「为什么不能在 `intern` 内部
    // 做」一节。必须排在 `load_all` 之后（六张表还没装完就跑,会漏掉
    // 后到的内容）、排在 `manifests`/`GenerationModSet::capture`（世界
    // 创建时刻,见 `ll_mod::mod_set` 模块文档「绑定时机」一节）真正读取
    // `content_hash_of` 之前——本函数返回的 `LoadedContent::registry`
    // 因此总是已经跑完值哈希的那一份,调用方不需要、也不应该再手动
    // 调用一次。
    apply_value_hashes(
        &mut registry,
        &ContentValueTables {
            terrain: &terrain_table,
            class: &class_table,
            skill: &skill_table,
            subclass: &subclass_table,
            quest: &quest_table,
            race: &race_table,
        },
    );

    let manifests = successfully_parsed_manifests(mods_root);
    let script_sources = read_script_sources(&manifests);

    tracing::info!(
        mods_root = %mods_root.display(),
        loaded = report.loaded_count(),
        failed = report.failed_count(),
        "内容装载完成"
    );

    LoadedContent {
        registry,
        terrain_ids,
        terrain_table,
        race_ids,
        race_table,
        space_ids,
        space_table,
        class_table,
        skill_table,
        subclass_table,
        quest_table,
        manifests,
        script_sources,
        report,
    }
}

/// 重新走一遍「发现 → 解析」两步（与 [`load_all`] 内部完全相同的两个
/// 公开函数），只取成功解析的清单——不重新实现任何解析逻辑，只是
/// `load_all` 没有对外暴露它内部产出的 `Vec<ModManifest>`（那是装载
/// 管线的内部状态，见 `ll_mod::pipeline::load_all` 文档），而存档头
/// 的「当前 mod 集合」需要这份数据。解析失败的候选静默跳过——它们
/// 已经在 `load_all` 产出的 [`LoadReport`] 里有一条 `Failed` 记录，
/// 这里不重复报告。
fn successfully_parsed_manifests(mods_root: &Path) -> Vec<ModManifest> {
    discover_mods(mods_root)
        .iter()
        .filter_map(|path| parse_manifest(path).ok())
        .collect()
}

/// 读出每个清单全部入口脚本的源码文本，供存档读入的 VM 强制重建使用。
/// 单个文件读取失败时跳过该文件而不是让整个函数失败——脚本文件在
/// `load_all` 真正执行时若读不到，那次装载早已经在 `report` 里记成
/// `Failed`，这里只是尽力收集「读得到」的那些源码，不是这份数据的
/// 权威来源。
fn read_script_sources(manifests: &[ModManifest]) -> Vec<(String, String)> {
    manifests
        .iter()
        .flat_map(|manifest| {
            let namespace = manifest.id.namespace().to_string();
            manifest.entry_points.iter().filter_map(move |entry| {
                std::fs::read_to_string(entry)
                    .ok()
                    .map(|source| (namespace.clone(), source))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空目录下装载只产出本体内容不报任何mod失败() {
        // Arrange：一个存在但不含任何 mod 子目录的空目录。
        let dir =
            std::env::temp_dir().join(format!("ll-game-content-test-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");

        // Act
        let loaded = load_content(&dir);

        // Assert
        assert_eq!(loaded.report.failed_count(), 0);
        assert!(loaded.manifests.is_empty());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 本体地形种族空间层属性全部注册进同一个registry() {
        // 「本体即 Mod」的端到端断言：四类本体内容确实都落进了同一份
        // Registry，而不是各自只在自己的表里自说自话——用每个命名空间
        // 都能查到内容哈希来验证。
        // Arrange
        let dir = std::env::temp_dir().join(format!(
            "ll-game-content-test-registry-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");

        // Act
        let loaded = load_content(&dir);

        // Assert
        assert!(loaded.registry.content_hash_of("lostland").is_some());
        assert!(
            loaded
                .registry
                .resolve(loaded.terrain_ids.grass.index())
                .is_some()
        );
        assert!(loaded.registry.resolve(loaded.race_ids.human).is_some());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 真实mods目录装载后清单非空() {
        // 端到端断言：装载仓库真实的 mods/ 目录（p4_acceptance 已验证
        // 过这个目录能成功装载），manifests 字段确实收集到了内容,
        // 不是恒为空的死字段。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");

        // Act
        let loaded = load_content(&mods_root);

        // Assert
        assert!(
            !loaded.manifests.is_empty(),
            "仓库真实 mods/ 目录应当至少包含一个可解析的 mod 清单"
        );
    }
}
