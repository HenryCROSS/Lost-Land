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
//! 先注册本体内容（地形 → 种族 → 空间层属性 → 占位内容 → 动画剪辑），
//! 再跑 [`ll_mod::pipeline::load_all`] 装载 `mods_root` 下的 mod——
//! 顺序理由见 [`ll_mod::pipeline`] 模块文档「本体内容不经过这条
//! 管线」一节：mod 内容 intern 进同一个 `Registry`，必须排在本体注册
//! 之后才能保证号段不冲突。五类本体注册彼此之间顺序不影响正确性
//! （各自对应不同的命名空间前缀，见 `ll_mod::base_race` 模块文档
//! 「调用顺序与 register_base_placeholder_content 无关」一节），这里
//! 固定一个顺序只是为了让日志读起来是线性的。

use std::path::Path;

use ll_core::ident::ContentIndex;
use ll_mod::asset_vfs::{self, AssetVfs};
use ll_mod::base_clip::register_base_clips;
use ll_mod::base_damage_formula::register_base_damage_formula;
use ll_mod::base_placeholder::register_base_placeholder_content;
use ll_mod::base_race::register_base_races;
use ll_mod::base_space_profile::register_base_space_profiles;
use ll_mod::base_terrain::register_base_terrain;
use ll_mod::base_xp_curve::register_base_xp_curve;
use ll_mod::class::ClassTable;
use ll_mod::clip::{BaseClipIds, ClipTable};
use ll_mod::content_hash::{ContentValueTables, apply_value_hashes};
use ll_mod::discover::discover_mods;
use ll_mod::formula::FormulaTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::{LoadReport, LoadStatus};
use ll_mod::manifest::{ModManifest, parse_manifest};
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::{BaseRaceIds, RaceTable};
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_world::space_profile::{BaseSpaceProfileIds, SpaceProfileTable};
use ll_world::terrain::{BaseTerrainIds, TerrainTable};

/// 本体自己的命名空间——「本体即 Mod」原则下，本体的资产也走
/// `ll_mod::asset_vfs` 同一套解析（见其模块文档），需要一个固定的
/// 命名空间字符串区分「这是本体自己声明的资产」与「这是某个 mod
/// 声明的资产」。与 `registry.content_hash_of("lostland")`
/// （既有测试用到的同一个字符串）保持一致。
pub const BASE_NAMESPACE: &str = "lostland";

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
    /// 本体动画剪辑索引缓存（行走/待机）。
    pub clip_ids: BaseClipIds,
    /// 动画剪辑表——纯表现层内容，不进 `WorldState`、不参与
    /// `WorldState::hash()`（ADR 0020 甲区，见 `ll_mod::clip` 模块
    /// 文档），只被渲染层（`crate::animation`/`crate::app`）读取。
    pub clip_table: ClipTable,
    /// 本体默认经验曲线索引（`lostland:default_xp_curve`）——未被职业/
    /// 种族显式绑定时的保底曲线，见 `ll_mod::base_xp_curve` 模块文档。
    pub default_xp_curve_id: ContentIndex,
    /// 经验曲线定义表。
    pub xp_curve_table: XpCurveTable,
    /// 职业/种族 → 经验曲线的绑定表。
    pub xp_curve_bindings: XpCurveBindings,
    /// 天赋表（天赋系统落地批次新增）——`ll_mod::trait_def::TraitTable`
    /// 实现 `ll_sim::traits::TraitCatalog`，与 `race_table`（实现
    /// `ll_sim::traits::TraitGrantSource`）一起供
    /// `ll_sim::resolve::resolve_with_skills_and_traits` 消费,见
    /// `ll_mod::trait_def` 模块文档。
    pub trait_table: TraitTable,
    /// 资源池表（资源池落地批次新增，第一批：法力池/血池）——
    /// `ll_mod::resource_pool::ResourcePoolTable` 实现
    /// `ll_sim::resource_pool::ResourcePoolCatalog`，与 `trait_table`
    /// 一起供 `ll_sim::resolve` 的资源消耗/回复分支消费，见
    /// `ll_mod::resource_pool` 模块文档。
    pub resource_pool_table: ResourcePoolTable,
    /// 物品表（P6 第一批：物品基础新增）——`ll_mod::item::ItemTable`，
    /// 本批次没有任何 `resolve` 侧消费者，见其模块文档「本批次范围」
    /// 一节。
    pub item_table: ItemTable,
    /// 本体默认伤害公式索引（`lostland:default_damage_formula`，伤害
    /// 公式引擎批次新增）——未被内容显式声明时的保底公式，见
    /// `ll_mod::base_damage_formula` 模块文档。
    pub default_damage_formula_id: ContentIndex,
    /// 伤害公式定义表。
    pub formula_table: FormulaTable,
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
    /// 本次 mod 装载报告：按 mod 归类的成功/失败结果。资产覆盖冲突
    /// （见 [`asset_vfs`] 模块文档）已经并入这份报告，作为额外的
    /// [`LoadStatus::Warning`] 条目——调用方不需要另外单独处理资产
    /// 冲突的展示，加载管理界面按既有的「按状态分组展示」逻辑即可
    /// 覆盖到。
    pub report: LoadReport,
    /// 已解析完覆盖规则的资产 VFS——本体贴图与全部 mod 贴图（含已经
    /// 生效的覆盖）打包前的最终来源，供 [`crate::app`] 喂给
    /// `ll_render::atlas_pack::pack_atlas`。
    pub asset_vfs: AssetVfs,
}

/// 装载全部游戏内容：先注册本体内容，再装载 `mods_root` 下的 mod，
/// 最后解析 `assets_root` 下本体与全部 mod 的资产 VFS。
///
/// `assets_root` 是本体自己的 `assets/` 目录（内含
/// `sprites/manifest.json5`），与 `mods_root` 是两个独立的目录树——
/// 本体资产不属于任何一个 mod 目录，见 [`ll_mod::asset_vfs`] 模块
/// 文档「为什么本体资产也要走这条路径」一节。
///
/// **本体二进制应当只调用本函数一次**（启动时）——这是本模块存在的
/// 唯一理由，见模块文档。
pub fn load_content(mods_root: &Path, assets_root: &Path) -> LoadedContent {
    let mut registry = Registry::new();

    let (terrain_ids, mut terrain_table) =
        register_base_terrain(&mut registry).expect("本体地形声明表内部一致，注册恒不失败");
    let (race_ids, mut race_table) =
        register_base_races(&mut registry).expect("本体种族声明表内部一致，注册恒不失败");
    let (space_ids, space_table) = register_base_space_profiles(&mut registry)
        .expect("本体空间层属性声明表内部一致，注册恒不失败");
    register_base_placeholder_content(&mut registry);
    let (clip_ids, mut clip_table) =
        register_base_clips(&mut registry).expect("本体剪辑声明表内部一致，注册恒不失败");
    let (default_xp_curve_id, mut xp_curve_table) =
        register_base_xp_curve(&mut |id| registry.intern(id))
            .expect("本体默认经验曲线声明内部一致，注册恒不失败");
    let (default_damage_formula_id, mut formula_table) =
        register_base_damage_formula(&mut |id| registry.intern(id))
            .expect("本体默认伤害公式声明内部一致，注册恒不失败");

    let mut class_table = ClassTable::new();
    let mut skill_table = SkillTable::new();
    let mut subclass_table = SubclassTable::new();
    let mut quest_table = QuestTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut trait_table = TraitTable::new();
    let mut resource_pool_table = ResourcePoolTable::new();
    let mut item_table = ItemTable::new();

    let mut report = load_all(
        mods_root,
        &mut registry,
        &mut GameplayTables {
            terrain: &mut terrain_table,
            class: &mut class_table,
            skill: &mut skill_table,
            subclass: &mut subclass_table,
            quest: &mut quest_table,
            race: &mut race_table,
            clip: &mut clip_table,
            xp_curve: &mut xp_curve_table,
            xp_curve_bindings: &mut xp_curve_bindings,
            trait_def: &mut trait_table,
            resource_pool: &mut resource_pool_table,
            item: &mut item_table,
            formula: &mut formula_table,
        },
    );

    // 值哈希升级：全部内容表此刻已经装载完毕（本体 + mod），在
    // 这里跑一次性收尾步骤,把字段值折进 registry 已有的 id 摘要——
    // 见 `ll_mod::content_hash` 模块文档「为什么不能在 `intern` 内部
    // 做」一节。必须排在 `load_all` 之后（内容表还没装完就跑,会漏掉
    // 后到的内容）、排在 `manifests`/`GenerationModSet::capture`（世界
    // 创建时刻,见 `ll_mod::mod_set` 模块文档「绑定时机」一节）真正读取
    // `content_hash_of` 之前——本函数返回的 `LoadedContent::registry`
    // 因此总是已经跑完值哈希的那一份,调用方不需要、也不应该再手动
    // 调用一次。
    //
    // `ContentValueTables` 现在覆盖十二张表（内容值哈希覆盖面扩展批次：
    // 新增天赋/资源池/物品/动画剪辑/空间层属性/经验曲线六张,详见
    // `ll_mod::content_hash` 模块文档「起因」一节）——仍不含
    // `xp_curve_bindings`：那是一张只做 id → id 映射、自己不持有任何
    // `ContentIndex` 条目的绑定表，`classify_index` 那套「按 id 归属
    // 哪张表」的机制天然覆盖不到它，见 `ll_mod::content_hash` 模块
    // 文档「哈希覆盖哪些字段」一节「例外，且是刻意的例外」一段——这是
    // 本批次已知、显式记录的缺口，不是疏漏。
    apply_value_hashes(
        &mut registry,
        &ContentValueTables {
            terrain: &terrain_table,
            class: &class_table,
            skill: &skill_table,
            subclass: &subclass_table,
            quest: &quest_table,
            race: &race_table,
            space_profile: &space_table,
            clip: &clip_table,
            trait_def: &trait_table,
            resource_pool: &resource_pool_table,
            item: &item_table,
            xp_curve: &xp_curve_table,
            formula: &formula_table,
        },
    );

    let manifests = successfully_parsed_manifests(mods_root);
    let script_sources = read_script_sources(&manifests);

    let asset_result = asset_vfs::build(mods_root, assets_root, BASE_NAMESPACE);
    for (mod_id, message) in asset_result.conflicts {
        // 这正是 `LoadStatus::Warning` 此前「声明了但从没被构造过」的
        // 产出路径——见 `ll_mod::load_report` 模块文档与
        // `ll_mod::asset_vfs` 模块文档「确定性」一节。追加而不是
        // `replace`：这个 mod 本身的脚本装载结果（`Loaded`/`Failed`）
        // 已经有一条独立的记录，资产冲突是另一件事，两条记录并存，
        // 加载管理界面按状态分组展示时天然都能看到。
        report.push(mod_id, LoadStatus::Warning(message));
    }

    tracing::info!(
        mods_root = %mods_root.display(),
        assets_root = %assets_root.display(),
        loaded = report.loaded_count(),
        failed = report.failed_count(),
        sprites = asset_result.vfs.sprites.len(),
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
        clip_ids,
        clip_table,
        default_xp_curve_id,
        xp_curve_table,
        xp_curve_bindings,
        trait_table,
        resource_pool_table,
        item_table,
        default_damage_formula_id,
        formula_table,
        manifests,
        script_sources,
        report,
        asset_vfs: asset_result.vfs,
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
    use std::path::PathBuf;

    /// 仓库真实的 `assets/` 目录——`ll-game` 到仓库根固定隔两级
    /// `../..`，与既有的「真实 mods/ 目录」测试同一套推导。
    fn repo_assets_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    #[test]
    fn 空目录下装载只产出本体内容不报任何mod失败() {
        // Arrange：一个存在但不含任何 mod 子目录的空目录。资产目录
        // 也不存在——`asset_vfs::build` 应当优雅处理，不需要真的存在。
        let dir = crate::test_support::unique_temp_path("ll-game-content-test-empty");
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");

        // Act
        let loaded = load_content(&dir, &dir.join("assets"));

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
        let dir = crate::test_support::unique_temp_path("ll-game-content-test-registry");
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");

        // Act
        let loaded = load_content(&dir, &dir.join("assets"));

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
        let loaded = load_content(&mods_root, &repo_assets_dir());

        // Assert
        assert!(
            !loaded.manifests.is_empty(),
            "仓库真实 mods/ 目录应当至少包含一个可解析的 mod 清单"
        );
    }

    #[test]
    fn 真实资产目录装载后本体精灵已注册进资产vfs() {
        // 端到端断言：装载仓库真实的 assets/ 目录，资产 VFS 里应当能
        // 找到本体的精灵条目——不是恒为空的死字段。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");

        // Act
        let loaded = load_content(&mods_root, &repo_assets_dir());

        // Assert
        assert!(
            !loaded.asset_vfs.sprites.is_empty(),
            "仓库真实 assets/ 目录应当至少包含一份本体精灵声明"
        );
    }

    #[test]
    fn 真实mod资产覆盖本体地形后examplemod的精灵可按完整命名空间id查到() {
        // 端到端断言：`mods/example_mod` 自带的 lava_floor 精灵确实
        // 进了资产 VFS，且条目名是完整命名空间 ID——与
        // `examplemod:lava_floor` 这个地形注册 ID 完全一致，供
        // `crate::layout` 的地形回退查图集直接复用（见其模块文档）。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");

        // Act
        let loaded = load_content(&mods_root, &repo_assets_dir());

        // Assert
        assert!(
            loaded
                .asset_vfs
                .sprites
                .iter()
                .any(|sprite| sprite.atlas_name == "examplemod:lava_floor"),
            "example_mod 应当自带一份 lava_floor 精灵声明"
        );
    }

    #[test]
    fn 真实mod覆盖本体地形贴图后源文件指向mod的覆盖文件() {
        // 端到端断言：`mods/example_mod` 自带的
        // `assets/overrides/lostland/sprites/terrain_dirt.png` 确实
        // 生效——本体 `terrain_dirt` 条目的最终来源文件应指向 mod 的
        // 覆盖文件，而不是本体自己的 `assets/sprites/terrain_dirt.png`。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");

        // Act
        let loaded = load_content(&mods_root, &repo_assets_dir());

        // Assert
        let terrain_dirt = loaded
            .asset_vfs
            .sprites
            .iter()
            .find(|sprite| sprite.atlas_name == "lostland:terrain_dirt")
            .expect("本体应声明 terrain_dirt 精灵");
        assert!(
            terrain_dirt
                .source_file
                .components()
                .any(|c| c.as_os_str() == "example_mod"),
            "terrain_dirt 的源文件应指向 example_mod 的覆盖文件，实际是 {}",
            terrain_dirt.source_file.display()
        );
    }

    #[test]
    fn 真实mods目录装载后examplemod的动画剪辑已注册() {
        // ADR 0018「API 完备性判据要求有真实 mod 脚本为证，不能靠单元
        // 测试自证」——本测试装载仓库真实的 mods/example_mod/animation.scm
        // （不是临时构造的测试脚本文本），断言其中的
        // `register-animation-clip` 调用确实通过完整的
        // 「发现 → 解析 → 拓扑排序 → 加载脚本 → 注册内容」链路把
        // `examplemod:slime_squish` 写进了 `clip_table`。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");

        // Act
        let loaded = load_content(&mods_root, &repo_assets_dir());

        // Assert
        let clip_index = loaded
            .registry
            .get(&ll_core::ident::NamespacedId::parse("examplemod:slime_squish").unwrap())
            .expect("examplemod:slime_squish 应已注册");
        let clip = loaded
            .clip_table
            .get(clip_index)
            .expect("已注册的剪辑索引应能查回剪辑内容");
        assert_eq!(
            clip.frames,
            vec!["slime_0".to_string(), "slime_1".to_string()]
        );
    }

    #[test]
    fn 真实内容装载后仅本体占位种族被值哈希判定为无归属表() {
        // 内容值哈希覆盖面扩展批次新增的覆盖率回归测试——
        // `ll_mod::content_hash` 模块文档「编译期强制」一节明确点出的
        // 局限："新增的 `*Table` 类型本身不会被编译器自动关联"到值
        // 哈希覆盖，需要测试期兜底。本测试用仓库真实的 mods/ 目录+
        // 本体内容跑一遍完整装载,断言"被 classify_index 判定成
        // ContentTableKind::Opaque 的 id 集合"恰好等于已知的例外集合
        // （当前只有本体占位种族一个,见 `ll_mod::base_placeholder`
        // 模块文档）,不多不少——新增一张内容表却忘记让 classify_index
        // 认领它,那张表全部条目会被判定成 Opaque,从而让下面的
        // assert_eq! 断言变红,而不是像升级前那样只能靠代码评审肉眼
        // 发现（ADR 0027「后果（技术债与后续）」一节记录的正是这条
        // 局限）。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
        let loaded = load_content(&mods_root, &repo_assets_dir());
        let tables = ContentValueTables {
            terrain: &loaded.terrain_table,
            class: &loaded.class_table,
            skill: &loaded.skill_table,
            subclass: &loaded.subclass_table,
            quest: &loaded.quest_table,
            race: &loaded.race_table,
            space_profile: &loaded.space_table,
            clip: &loaded.clip_table,
            trait_def: &loaded.trait_table,
            resource_pool: &loaded.resource_pool_table,
            item: &loaded.item_table,
            xp_curve: &loaded.xp_curve_table,
            formula: &loaded.formula_table,
        };

        // Act
        let mut opaque_ids: Vec<String> = loaded
            .registry
            .snapshot()
            .iter()
            .filter(|entry_id| {
                let index = loaded
                    .registry
                    .get(entry_id)
                    .expect("snapshot 里的 id 恒能在同一个 registry 查回索引");
                ll_mod::content_hash::classify_index(index, &tables)
                    == ll_mod::content_hash::ContentTableKind::Opaque
            })
            .map(ToString::to_string)
            .collect();
        opaque_ids.sort();

        // Assert
        assert_eq!(opaque_ids, vec!["lostland:placeholder_race".to_string()]);
    }
}
