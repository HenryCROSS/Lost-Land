//! 加载管线：把 [`crate::discover`]/[`crate::manifest`]/[`crate::topo`]/
//! [`crate::content_data`]/[`crate::registry`] 串成规格 §10.6 描述的完整
//! 流程，产出一份 [`crate::load_report::LoadReport`]。
//!
//! # 完整调用链
//!
//! ```text
//! discover_mods(root)              -> 候选清单路径
//!   -> parse_manifest(path)（逐个，互不影响）-> 已解析清单/Parse 阶段失败
//!   -> topo_sort(已解析清单)         -> 加载顺序，或 Topo 阶段失败（整批中止）
//!   -> 按顺序逐个 mod：
//!        load_mod_content_data(mod 目录) -> 内容数据文件（JSON5）读进各张表；
//!                                           任一文件坏了整个 mod 判 Failed，
//!                                           归入 Register 阶段
//!        全部成功                        -> Loaded
//! ```
//!
//! # 这条管线里没有虚拟机
//!
//! mod 的内容全部是数据文件（JSON5），装载就是「读文件 → 反序列化 →
//! 写进内容表」，没有任何脚本求值、没有沙箱、没有内存/时间预算。行为
//! 逻辑写在引擎里的 Rust（例如 [`crate::native_behavior`]），不由 mod
//! 提供——第三方 Rust 扩展能力（注册表/C ABI）明确推迟，不做。
//!
//! 此前这里有一整套 Steel 脚本装载：每个带 `entry_points` 的 mod 各造
//! 一个 `ScriptEngine`，逐个入口 `load_source`，`register-*` 经线程局部
//! 目标写回内容表。整套连同 `steel-core` 依赖一起拆掉了，起因是
//! [ADR 0028](../../../knowledge/decisions/0028-steel-engine-construction-memory-corruption.md)
//! 记录的那个查不出根因的上游内存破坏——完整测试套件被它以 17–33% 的
//! 概率随机打断，六条假说全部被数据否决。
//!
//! # 内容顺序的确定性（约束 C5）
//!
//! mod 之间的顺序来自 [`crate::topo::topo_sort`]（确定性拓扑序），
//! mod 之内的文件顺序来自 [`crate::content_data`] 里那张固定的
//! `CONTENT_FILES` 表，文件之内是 JSON5 数组的书写顺序。三层都没有一处
//! 依赖 `HashMap` 的迭代顺序。
//!
//! # 本体内容分两半，与这条管线的关系不一样
//!
//! - **已迁进数据文件的那一半**（`mods/lostland/*.json5`：种族/职业/
//!   技能/任务/副职/配方类别/标签）走的**就是这条管线**，与任何第三方
//!   mod 完全同一条路径，没有任何本体专属入口。它是**强制装载**的：
//!   装载完毕后 `ll_game::content::load_content` 会用
//!   [`crate::base_contract`] 的契约解析按 id 逐字段填充
//!   `ll_mod::race::BaseRaceIds` 这类句柄，缺任何一条就整批失败。
//! - **尚未迁走的那一半**（[`crate::base_terrain::register_base_terrain`]/
//!   [`crate::base_placeholder::register_base_placeholder_content`] 等
//!   仍然存在的 `base_*` 模块）**不经过这条管线**——它们是一次直接的
//!   Rust 函数调用，没有清单、没有数据文件，见各自模块文档。调用方应在
//!   跑本管线之前先调用一遍这些 `base_*` 注册函数。
//!
//! 两半共享同一个 [`crate::registry::Registry`] 与 [`GameplayTables`]
//! 里的各张内容表。

use std::path::{Path, PathBuf};

use ll_core::ident::NamespacedId;
use ll_world::culture::CultureTable;
use ll_world::resource::ResourceTable;
use ll_world::space_profile::SpaceProfileTable;
use ll_world::terrain::TerrainTable;
use ll_world::weather::WeatherTable;

use crate::behavior_binding::ClassBehaviorBindings;
use crate::class::ClassTable;
use crate::clip::ClipTable;
use crate::damage_category::DamageCategoryTable;
use crate::formula::FormulaTable;
use crate::item::ItemTable;
use crate::load_report::{LoadError, LoadReport, LoadStage, LoadStatus, SourceLocation};
use crate::manifest::{ModError, ModManifest, mod_self_id, parse_manifest};
use crate::modifier_type::ModifierTypeTable;
use crate::quest::QuestTable;
use crate::race::RaceTable;
use crate::recipe::RecipeTable;
use crate::recipe_category::RecipeCategoryTable;
use crate::registry::Registry;
use crate::resource_pool::ResourcePoolTable;
use crate::skill::SkillTable;
use crate::subclass::SubclassTable;
use crate::tag::TagTable;
use crate::trait_def::TraitTable;
use crate::weapon_category::WeaponCategoryTable;
use crate::xp_curve::{XpCurveBindings, XpCurveTable};
use crate::{content_data, discover, topo};

/// 加载管线一次装载会话内，mod 内容数据文件可以写入的全部内容表——
/// 地形、职业、技能、副职、任务、种族、动画剪辑、经验曲线（含绑定）、
/// 天赋、资源池、物品、伤害公式、武器类别、伤害类别、标签、空间层
/// 属性、天气、配方、配方类别。
///
/// 集中成一个结构体，而不是让 [`load_all`] 接收二十个独立的 `&mut`
/// 参数：这些表在装载管线里总是同进同出（同一个 mod 目录下的多个内容
/// 文件各写各的表），拆成二十个位置参数只会让调用点的参数顺序成为
/// 易错点，结构体把「这些表必须一起传」这条约束在类型上表达出来。
///
/// # 字段个数就是「mod 能声明几类玩法层内容」的唯一权威清单
///
/// 每个字段对应 [`crate::content_data`] 里 `CONTENT_FILES` 的一个（或
/// 几个）文件名。新增一类内容意味着三处同时落地：这里加一个字段、
/// `CONTENT_FILES` 加一行、[`crate::content_hash`] 的值哈希覆盖面加一
/// 类——少任何一处都会以「声明了但没接线」的形态留下缺口。
pub struct GameplayTables<'a> {
    /// 地形表（`terrain.json5`）。
    pub terrain: &'a mut TerrainTable,
    /// 资源表（`resources.json5`），见 `ll_world::resource` 模块文档。
    ///
    /// 与 `terrain`/`space_profile`/`weather` 同属「写进 `ll-world` 的
    /// 表」那一族；与它们不同的是这张表在装载会话开始之前是**空的**
    /// ——资源没有「本体注册入口」这一层，本体四种资源与任何 mod 的
    /// 资源走的是完全相同的 `resources.json5` 通道。
    pub resource: &'a mut ResourceTable,
    /// 文化表（`cultures.json5`），见 `ll_world::culture` 模块文档。
    ///
    /// 与 `resource` 同一族、同一条理由：类型住在 `ll-world`（选址、
    /// 铺房子、战争都发生在那里），数据由本装载器填，装完整张表注入
    /// 世界生成。同样没有「本体注册入口」这一层——本体的文化与任何
    /// mod 的文化走完全相同的 `cultures.json5` 通道。
    pub culture: &'a mut CultureTable,
    /// 职业表（`classes.json5`）。
    pub class: &'a mut ClassTable,
    /// 技能表（`skills.json5`）。
    pub skill: &'a mut SkillTable,
    /// 副职表（`subclasses.json5`）。
    pub subclass: &'a mut SubclassTable,
    /// 任务表（`quests.json5`）。
    pub quest: &'a mut QuestTable,
    /// 种族表（`races.json5`）。
    pub race: &'a mut RaceTable,
    /// 动画剪辑表（`animations.json5`）——不进 `WorldState`、不参与
    /// `WorldState::hash()`（ADR 0020 甲区，见 `crate::clip` 模块文档），
    /// 但注册路径与另外几张表完全一致，随装载会话同进同出。
    pub clip: &'a mut ClipTable,
    /// 经验曲线定义表（`xp_curves.json5`），见 `crate::xp_curve` 模块
    /// 文档。
    pub xp_curve: &'a mut XpCurveTable,
    /// 职业/种族 → 经验曲线的绑定表——由 `classes.json5`/`races.json5`
    /// 里的 `xp_curve` 字段写入。与 `xp_curve` 分成两个字段而不是一个
    /// 元组字段，是为了和其余各表同样走 `std::mem::take` 这条既有搬运
    /// 手法，不需要为这一对表单独发明搬运方式。
    pub xp_curve_bindings: &'a mut XpCurveBindings,
    /// 职业 → 行为原型的绑定表——由 `classes.json5` 里的 `behavior`
    /// 字段写入。与 `xp_curve_bindings` 同一条手法、同一条理由，见
    /// `crate::behavior_binding` 模块文档「形状」一节。
    pub class_behavior_bindings: &'a mut ClassBehaviorBindings,
    /// 天赋表（`traits.json5`），见 `crate::trait_def` 模块文档。
    pub trait_def: &'a mut TraitTable,
    /// 资源池表（`resource_pools.json5`），见 `crate::resource_pool`
    /// 模块文档。
    pub resource_pool: &'a mut ResourcePoolTable,
    /// 物品表（`items.json5`），见 `crate::item` 模块文档。
    pub item: &'a mut ItemTable,
    /// 伤害公式定义表（`damage_formulas.json5`），见 `crate::formula`
    /// 模块文档。
    pub formula: &'a mut FormulaTable,
    /// 武器类别定义表（`weapon_categories.json5`），见
    /// `crate::weapon_category` 模块文档。
    pub weapon_category: &'a mut WeaponCategoryTable,
    /// 标签表（`tags.json5`），见 `crate::tag` 模块文档（耐久标签
    /// 批次）。
    pub tag: &'a mut TagTable,
    /// 伤害类别定义表（`damage_categories.json5`），见
    /// `crate::damage_category` 模块文档。
    pub damage_category: &'a mut DamageCategoryTable,
    /// 空间层属性表（`space_profiles.json5`）。
    ///
    /// 与多数表的一处不同：这张表在装载会话开始**之前**通常已经非空
    /// （`crate::base_space_profile::register_base_space_profiles` 先注册
    /// 了本体四种空间类型），mod 是往里追加。这与地形表
    /// （`register_base_terrain` 同样先跑）是同一种情形，不是本字段独有
    /// 的例外——`SpaceProfileTable::define` 的重复定义校验保证 mod 覆盖
    /// 不掉本体已声明的那几条。
    pub space_profile: &'a mut SpaceProfileTable,
    /// 配方表（`crafting.json5`），见 `crate::recipe` 模块文档。
    pub recipe: &'a mut RecipeTable,
    /// 配方类别表（`crafting.json5`），见 `crate::recipe_category` 模块
    /// 文档。
    ///
    /// 与 `recipe` 分成两个字段而不是一个元组字段，理由同
    /// `xp_curve`/`xp_curve_bindings` 那一对：两张表各自走
    /// `std::mem::take(tables.……)` 这条既有搬运手法。
    pub recipe_category: &'a mut RecipeCategoryTable,
    /// 天气表（`weather.json5`）。
    ///
    /// 与 `space_profile` 同一种情形：这张表在装载会话开始**之前**通常
    /// 已经非空（`crate::base_weather::register_base_weathers` 先注册了
    /// 本体六种天气），mod 是往里追加，`WeatherTable::define` 的重复
    /// 定义校验保证 mod 覆盖不掉本体已声明的那几条。
    pub weather: &'a mut WeatherTable,
    /// 加值类型表（`modifier_types.json5`），见 `crate::modifier_type`
    /// 模块文档（加值类型批次）。
    pub modifier_type: &'a mut ModifierTypeTable,
}

/// 跑一次完整的 mod 装载会话：发现 `mods_root` 下的候选、解析、拓扑
/// 排序、按序读入各 mod 的内容数据文件——写入 `registry`/`tables`，
/// 返回一份报告。
///
/// `registry`/`tables` 应当已经装过**尚未迁进数据文件的那部分**本体
/// 内容（`register_base_terrain`/`register_base_placeholder_content`
/// 等）：本函数只管 mod 目录，不知道、也不需要知道它们是怎么注册进去
/// 的——这正是「本体即 Mod」在管线层面的体现：那部分注册发生在调用本
/// 函数**之前**的一次独立调用，mod 内容随后 intern 进同一个
/// `Registry`，两者共用同一段单调递增的 `ContentIndex` 号段（见
/// `crate::base_terrain` 模块文档与其测试）。
///
/// 已经迁进数据文件的本体内容（`mods/lostland/`）反过来是**本函数
/// 自己**装载的，与任何第三方 mod 走同一条路径——调用方随后必须跑一次
/// 契约解析（[`crate::race::resolve_base_races`]）确认它真的在，见
/// [`crate::base_contract`] 模块文档。
pub fn load_all(
    mods_root: &Path,
    registry: &mut Registry,
    tables: &mut GameplayTables,
) -> LoadReport {
    let mut report = LoadReport::new();

    let candidates = discover::discover_mods(mods_root);
    let mut parsed: Vec<ModManifest> = Vec::new();
    // 与 `parsed` 平行的「这个 mod 的根目录」——`ModManifest` 自己不记
    // 根目录，而内容数据文件是按固定文件名在 mod 目录下查找的，所以在
    // 这里顺手留一份。
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in &candidates {
        match parse_manifest(path) {
            Ok(manifest) => {
                parsed.push(manifest);
                roots.push(
                    path.parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from(".")),
                );
            }
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
        match load_mod_content(&roots[idx], manifest, registry, tables) {
            Ok(()) => report.push(manifest.id.clone(), LoadStatus::Loaded),
            Err(err) => report.push(manifest.id.clone(), LoadStatus::Failed(err)),
        }
    }

    report
}

/// 读入单个 mod 目录下的全部内容数据文件。
///
/// 一个内容文件坏了，整个 mod 判 `Failed`：半份内容比没有内容更难查
/// （症状是运行期某条引用悬空，不是启动期一条错误）。
fn load_mod_content(
    root: &Path,
    manifest: &ModManifest,
    registry: &mut Registry,
    tables: &mut GameplayTables,
) -> Result<(), LoadError> {
    content_data::load_mod_content_data(root, registry, tables).map_err(|err| LoadError {
        mod_id: manifest.id.clone(),
        stage: LoadStage::Register,
        message: err.message.clone(),
        location: Some(SourceLocation {
            file: err.file.clone(),
            // 行号在 `err.message` 里（json5 的 `... at line N column M`）。
            // 要填进 `SourceLocation::line` 得把那串文案反过来解析一遍
            // ——从结构化错误退化成文本再解析回结构，是一条只会引入
            // 分歧的路，不走。
            line: None,
        }),
    })
}

/// 单个 mod 的「最小可行」一键重载（简报「单个 mod 一键重载」的诚实
/// 范围：重新对该 mod 跑一次「解析→读内容数据文件」）。
///
/// # 为什么不写回正在运行的会话 `Registry`
///
/// 若重新对同一个 `id` 注册内容，`Registry::intern` 本身是幂等的
/// （返回同一个索引），但各内容表的 `define` **拒绝重复定义**——这意味
/// 着任何已经成功加载过一次的 mod，只要重载就会立刻在第二条内容上撞见
/// 「重复定义」，即使内容本身完全没有问题。这不是本函数的 bug，是
/// 「重复定义拒绝」与「原地重载复用同一注册表」两条设计天然冲突——P4
/// 明确不做真正的热重载（存盘即生效，见任务简报「本阶段范围」），本
/// 函数因此改为对一份**全新的**空 `Registry`/内容表重新跑一遍该 mod 的
/// 解析与内容装载，验证「这个 mod 自己能不能干净地加载」，不去动正在
/// 运行的游戏会话状态。
///
/// # 已知局限：跨 mod 引用会在这里失败
///
/// 全新的空 `Registry` 意味着**别的 mod 声明的内容都不在场**。一个
/// 引用了依赖方内容的 mod（例如 `mods/example_mod/items.json5` 里的
/// `tags` 字段指向 `lostland:weapon`）在这里会拿到一条「尚未注册」的
/// 失败，而同一份 mod 走 [`load_all`] 是装得上的。这是「最小可行版本」
/// 的诚实边界，不是缺陷被掩盖：要修得让重载先把依赖链上的 mod 也装进
/// 这份临时注册表，那已经是「重跑半个 `load_all`」，属于真正的热重载
/// 范围。
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

    let root = match manifest_path.parent() {
        Some(root) => root.to_path_buf(),
        None => PathBuf::from("."),
    };

    let mut registry = Registry::new();
    let mut terrain = TerrainTable::new();
    let mut class = ClassTable::new();
    let mut skill = SkillTable::new();
    let mut subclass = SubclassTable::new();
    let mut quest = QuestTable::new();
    let mut race = RaceTable::new();
    let mut clip = ClipTable::new();
    let mut xp_curve = XpCurveTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut class_behavior_bindings = ClassBehaviorBindings::new();
    let mut trait_def = TraitTable::new();
    let mut resource_pool = ResourcePoolTable::new();
    let mut item = ItemTable::new();
    let mut formula = FormulaTable::new();
    let mut weapon_category = WeaponCategoryTable::new();
    let mut damage_category = DamageCategoryTable::new();
    let mut tag = TagTable::new();
    let mut space_profile = SpaceProfileTable::new();
    let mut recipe = RecipeTable::new();
    let mut recipe_category = RecipeCategoryTable::new();
    let mut resource = ResourceTable::new();
    let mut culture = CultureTable::new();
    let mut weather = WeatherTable::new();
    let mut modifier_type = ModifierTypeTable::new();
    let mut tables = GameplayTables {
        terrain: &mut terrain,
        class: &mut class,
        skill: &mut skill,
        subclass: &mut subclass,
        quest: &mut quest,
        race: &mut race,
        clip: &mut clip,
        xp_curve: &mut xp_curve,
        xp_curve_bindings: &mut xp_curve_bindings,
        class_behavior_bindings: &mut class_behavior_bindings,
        trait_def: &mut trait_def,
        resource_pool: &mut resource_pool,
        item: &mut item,
        formula: &mut formula,
        weapon_category: &mut weapon_category,
        damage_category: &mut damage_category,
        tag: &mut tag,
        space_profile: &mut space_profile,
        resource: &mut resource,
        culture: &mut culture,
        weather: &mut weather,
        recipe: &mut recipe,
        recipe_category: &mut recipe_category,
        modifier_type: &mut modifier_type,
    };

    match load_mod_content(&root, &manifest, &mut registry, &mut tables) {
        Ok(()) => LoadStatus::Loaded,
        Err(err) => LoadStatus::Failed(err),
    }
}

/// 从 `mod.json5` 所在目录名推出一个尽力而为的 mod 身份，供清单本身都
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{OwnedTables, tempdir};
    use ll_world::terrain::TerrainKind;
    use std::fs;

    /// 在 `root` 下建一个候选 mod 子目录，写入清单与任意多个内容数据
    /// 文件。
    fn write_mod(root: &Path, dir_name: &str, manifest_json5: &str, content: &[(&str, &str)]) {
        let mod_dir = root.join(dir_name);
        fs::create_dir_all(&mod_dir).expect("创建 mod 子目录");
        fs::write(
            mod_dir.join(crate::discover::MANIFEST_FILENAME),
            manifest_json5,
        )
        .expect("写入清单");
        for (name, body) in content {
            fs::write(mod_dir.join(name), body).expect("写入内容数据文件");
        }
    }

    #[test]
    fn 一个内容文件都没有的mod直接判定为已加载() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "puredata",
            r#"{ namespace: "puredata", version: "0.1.0" }"#,
            &[],
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
    fn 内容数据文件里的地形真的写进了注册表与地形表() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "examplemod",
            r#"{ namespace: "examplemod", version: "0.1.0" }"#,
            &[(
                "terrain.json5",
                r#"{ terrains: [
                    { id: "examplemod:lava_floor", blocks_sight: false,
                      blocks_move: false, move_cost: 350 },
                ] }"#,
            )],
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
        let index = registry
            .get(&NamespacedId::parse("examplemod:lava_floor").unwrap())
            .expect("地形应当进了 Registry");
        assert_eq!(
            TerrainKind::from_index(index).move_cost(&owned.terrain),
            350
        );
    }

    #[test]
    fn 同一个mod的多类内容共用同一个registry全部真实写进各自的表() {
        // 端到端验证：一个 mod 目录下七个内容文件依次读入，七次
        // intern 必须落在同一个 Registry 上（否则 ContentIndex 会撞车），
        // 且七张表各自都收到了正确的内容——这是 `load_all` 一次会话内
        // 「注册表只有一个」这条性质的直接回归。
        //
        // 与之互补的另一半是「结构等价」测试（本体注册与 mod 注册走
        // 同一条注册路径、注册表内部不给本体开后门），分布在
        // `base_placeholder.rs`/`base_terrain.rs`/`base_space_profile.rs`/
        // `base_clip.rs`/`class.rs`/`quest.rs`/`race.rs`/`skill.rs`/
        // `subclass.rs`/`clip.rs`（以及 `ll-world` 的 `space_profile.rs`）
        // 各自的单元测试里。真实使用中的完整示例见
        // `mods/example_mod/*.json5`。
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "gameplay",
            r#"{ namespace: "gameplay", version: "0.1.0" }"#,
            &[
                (
                    "terrain.json5",
                    r#"{ terrains: [ { id: "gameplay:lava_floor", blocks_sight: false,
                        blocks_move: false, move_cost: 350 } ] }"#,
                ),
                (
                    "classes.json5",
                    r#"{ classes: [ { id: "gameplay:necromancer",
                        display_name_key: "gameplay:necromancer_display_name",
                        primary_attribute: "willpower" } ] }"#,
                ),
                (
                    "subclasses.json5",
                    r#"{ subclasses: [ { id: "gameplay:shadowdancer",
                        display_name_key: "gameplay:shadowdancer_display_name" } ] }"#,
                ),
                (
                    "skills.json5",
                    r#"{ skills: [ { id: "gameplay:frostbolt", cooldown_ticks: 25,
                        resource_cost: { kind: "mana", amount: 12 },
                        effect: { kind: "deal-damage", amount: 15 } } ] }"#,
                ),
                (
                    "quests.json5",
                    r#"{ quests: [ { id: "gameplay:kill_goblins",
                        condition: { kind: "kill-count", target: "gameplay:goblin",
                                     count: 3 } } ] }"#,
                ),
                (
                    "races.json5",
                    r#"{ races: [ { id: "gameplay:half_elf",
                        display_name_key: "gameplay:half_elf_display_name",
                        stat_modifiers: { dexterity: 1, charisma: 1, luck: 1 },
                        lifespan_years: 150 } ] }"#,
                ),
                (
                    "animations.json5",
                    r#"{ clips: [ { id: "gameplay:slime_squish",
                        frames: ["slime_0", "slime_1"], frames_per_step: 6,
                        looping: true, exit_grace_frames: 0 } ] }"#,
                ),
            ],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert：mod 整体加载成功。
        assert_eq!(
            report.entries,
            vec![(mod_self_id("gameplay").unwrap(), LoadStatus::Loaded)],
            "实际 {:?}",
            report.entries
        );
        // 七类内容各自都能在对应的表里查到——不是只 intern 进了
        // Registry 却没有写进属性表。
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

        let clip_index = registry
            .get(&NamespacedId::parse("gameplay:slime_squish").unwrap())
            .expect("动画剪辑应已注册");
        assert!(owned.clip.get(clip_index).is_some());
    }

    #[test]
    fn mod能声明自定义空间层属性并与本体已注册的四种共存() {
        // 这是「空间层属性」这条通道的端到端验收：一个真实落在磁盘上
        // 的 mod（mod.json5 + space_profiles.json5）经过完整的
        // 发现→解析→拓扑排序→读内容文件→写回内容表流程，声明出一种
        // 本体没有的空间类型。
        //
        // 同时验证「与本体共存」：装载会话开始前先把本体四种空间类型
        // 注册进同一个 Registry/同一张表（生产路径 `load_content` 正是
        // 这个顺序），确认 mod 的追加不会覆盖、也不会被覆盖。
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "spacemod",
            r#"{ namespace: "spacemod", version: "0.1.0" }"#,
            &[(
                "space_profiles.json5",
                r#"{ space_profiles: [
                    { id: "spacemod:abyss", ambient_light_floor: 0,
                      exposed_to_sky: false, base_temperature: -40,
                      diggable: true, buildable: false },
                    { id: "spacemod:greenhouse", ambient_light_floor: 900,
                      exposed_to_sky: true, base_temperature: 260,
                      diggable: false, buildable: true,
                      reverb_tag: "spacemod:glass" },
                ] }"#,
            )],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();
        let (base_ids, base_table) =
            crate::base_space_profile::register_base_space_profiles(&mut registry)
                .expect("本体空间层属性声明表内部一致");
        owned.space_profile = base_table;

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        assert_eq!(
            report.entries,
            vec![(mod_self_id("spacemod").unwrap(), LoadStatus::Loaded)],
            "实际 {:?}",
            report.entries
        );
        let abyss = registry
            .get(&NamespacedId::parse("spacemod:abyss").unwrap())
            .expect("mod 声明的空间层属性应当进了 Registry");
        assert!(owned.space_profile.is_defined(abyss));
        assert!(!owned.space_profile.exposed_to_sky(abyss));
        assert_eq!(owned.space_profile.ambient_light_floor(abyss), 0);
        assert_eq!(owned.space_profile.base_temperature(abyss), -40);
        assert!(owned.space_profile.diggable(abyss));
        assert!(!owned.space_profile.buildable(abyss));

        let greenhouse = registry
            .get(&NamespacedId::parse("spacemod:greenhouse").unwrap())
            .expect("第二条声明同样应当进了 Registry");
        assert!(owned.space_profile.exposed_to_sky(greenhouse));
        assert_eq!(
            owned.space_profile.reverb_tag(greenhouse),
            Some(NamespacedId::parse("spacemod:glass").unwrap()),
            "非空 reverb_tag 应当按字面标识符存下"
        );

        // 本体四种一条都没被 mod 的追加动作破坏。
        assert!(owned.space_profile.exposed_to_sky(base_ids.surface));
        assert!(!owned.space_profile.exposed_to_sky(base_ids.cave));
        assert!(!owned.space_profile.exposed_to_sky(base_ids.dungeon));
        assert!(
            !owned
                .space_profile
                .exposed_to_sky(base_ids.building_interior)
        );
    }

    #[test]
    fn mod声明的非露天空间经真实装载后环境光不随世界时钟变化() {
        // 本测试钉住的是「mod 声明出来的层属性与 Rust 注册出来的语义
        // 逐字相同」这件事——`ll_world::space_profile::effective_ambient_light`
        // 那条组合规则（露天转发昼夜曲线、非露天取地板值）不知道也不
        // 需要知道这条内容是从哪条通道进来的。这正是 ADR 0018 修订版
        // 「本体与 mod 用同一套声明格式」在这张表上的可执行形式。
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "spacemod",
            r#"{ namespace: "spacemod", version: "0.1.0" }"#,
            &[(
                "space_profiles.json5",
                r#"{ space_profiles: [
                    { id: "spacemod:abyss", ambient_light_floor: 30,
                      exposed_to_sky: false, base_temperature: 0,
                      diggable: true, buildable: false },
                ] }"#,
            )],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();
        load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());
        let index = registry
            .get(&NamespacedId::parse("spacemod:abyss").unwrap())
            .expect("mod 声明的空间层属性应当进了 Registry");
        let profile = ll_world::space_profile::SpaceProfile {
            id: NamespacedId::parse("spacemod:abyss").unwrap(),
            ambient_light_floor: owned.space_profile.ambient_light_floor(index),
            exposed_to_sky: owned.space_profile.exposed_to_sky(index),
            base_temperature: owned.space_profile.base_temperature(index),
            diggable: owned.space_profile.diggable(index),
            buildable: owned.space_profile.buildable(index),
            reverb_tag: owned.space_profile.reverb_tag(index),
        };

        // Act
        let midnight = ll_world::space_profile::effective_ambient_light(
            &profile,
            ll_core::time::Tick(0),
            ll_world::weather::Weather::CLEAR,
        );
        let noon = ll_world::space_profile::effective_ambient_light(
            &profile,
            ll_core::time::Tick(ll_core::time::TICKS_PER_DAY / 2),
            ll_world::weather::Weather::CLEAR,
        );

        // Assert
        assert_eq!(midnight, noon);
        assert_eq!(midnight.0, 30);
    }

    #[test]
    fn mod声明的空间层属性被内容值哈希认领而不是判成无归属() {
        // 值哈希覆盖面的回归：`classify_index` 通过
        // `SpaceProfileTable::is_defined` 认领这条内容——若哪天有人给
        // GameplayTables 加了 space_profile 却忘了把同一张表传给
        // ContentValueTables，这条断言会变红（判成 Opaque）。
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "spacemod",
            r#"{ namespace: "spacemod", version: "0.1.0" }"#,
            &[(
                "space_profiles.json5",
                r#"{ space_profiles: [
                    { id: "spacemod:abyss", ambient_light_floor: 0,
                      exposed_to_sky: false, base_temperature: 0,
                      diggable: true, buildable: false },
                ] }"#,
            )],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();
        load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());
        let index = registry
            .get(&NamespacedId::parse("spacemod:abyss").unwrap())
            .expect("mod 声明的空间层属性应当进了 Registry");

        // Act
        let kind = crate::content_hash::classify_index(
            index,
            &crate::content_hash::ContentValueTables {
                terrain: &owned.terrain,
                class: &owned.class,
                skill: &owned.skill,
                subclass: &owned.subclass,
                quest: &owned.quest,
                race: &owned.race,
                space_profile: &owned.space_profile,
                clip: &owned.clip,
                trait_def: &owned.trait_def,
                resource_pool: &owned.resource_pool,
                item: &owned.item,
                xp_curve: &owned.xp_curve,
                formula: &owned.formula,
                weapon_category: &owned.weapon_category,
                damage_category: &owned.damage_category,
                resource: &owned.resource,
                culture: &owned.culture,
                weather: &owned.weather,
                recipe: &owned.recipe,
                recipe_category: &owned.recipe_category,
                tag: &owned.tag,
                modifier_type: &owned.modifier_type,
            },
        );

        // Assert
        assert_eq!(kind, crate::content_hash::ContentTableKind::SpaceProfile);
    }

    #[test]
    fn 坏掉的内容文件归入register阶段且不影响其它mod() {
        // Arrange：broken 的内容文件 JSON5 语法错误，good 完全正常
        // ——验证一个 mod 的失败不牵连另一个。
        let root = tempdir();
        write_mod(
            root.path(),
            "broken",
            r#"{ namespace: "broken", version: "0.1.0" }"#,
            &[("terrain.json5", r#"{ terrains: [ { id: "broken:rock" "#)],
        );
        write_mod(
            root.path(),
            "good",
            r#"{ namespace: "good", version: "0.1.0" }"#,
            &[(
                "terrain.json5",
                r#"{ terrains: [ { id: "good:rock", blocks_sight: true,
                    blocks_move: true, move_cost: 4294967295 } ] }"#,
            )],
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
            Some(LoadStatus::Failed(err)) => {
                assert_eq!(err.stage, LoadStage::Register);
                assert!(
                    err.location
                        .as_ref()
                        .is_some_and(|loc| loc.file.ends_with("terrain.json5")),
                    "错误必须点名出问题的那个内容文件，实际 {:?}",
                    err.location
                );
            }
            other => panic!("期望 Register 阶段的 Failed，实际 {other:?}"),
        }
        let good_status = report
            .entries
            .iter()
            .find(|(id, _)| id.namespace() == "good")
            .map(|(_, status)| status);
        assert_eq!(good_status, Some(&LoadStatus::Loaded));
        assert!(
            registry
                .get(&NamespacedId::parse("good:rock").unwrap())
                .is_some()
        );
    }

    #[test]
    fn 缺失依赖导致整批加载在topo阶段中止() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "needs_ghost",
            r#"{
                namespace: "needs_ghost",
                version: "0.1.0",
                dependencies: ["ghost"],
            }"#,
            &[],
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
            r#"{
                namespace: "needs_new_provider",
                version: "0.1.0",
                dependencies: { provider: ">=2.0" },
            }"#,
            &[],
        );
        write_mod(
            root.path(),
            "provider",
            r#"{ namespace: "provider", version: "1.0.0" }"#,
            &[],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert：两个 mod 都在报告里，且都是 Topo 阶段失败。
        assert_eq!(report.entries.len(), 2, "实际 {:?}", report.entries);
        for (id, status) in &report.entries {
            match status {
                LoadStatus::Failed(err) => assert_eq!(err.stage, LoadStage::Topo),
                other => panic!("{id:?} 期望 Topo 阶段的 Failed，实际 {other:?}"),
            }
        }
    }

    #[test]
    fn 清单本身解析失败归入parse阶段() {
        // Arrange：清单缺 version 字段。
        let root = tempdir();
        write_mod(
            root.path(),
            "no_version",
            r#"{ namespace: "no_version" }"#,
            &[],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        match &report.entries[0].1 {
            LoadStatus::Failed(err) => assert_eq!(err.stage, LoadStage::Parse),
            other => panic!("期望 Parse 阶段的 Failed，实际 {other:?}"),
        }
    }

    #[test]
    fn reload_mod对干净的mod返回loaded() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "reloadable",
            r#"{ namespace: "reloadable", version: "0.1.0" }"#,
            &[(
                "terrain.json5",
                r#"{ terrains: [ { id: "reloadable:rock", blocks_sight: true,
                    blocks_move: true, move_cost: 4294967295 } ] }"#,
            )],
        );

        // Act
        let status = reload_mod(
            &root
                .path()
                .join("reloadable")
                .join(crate::discover::MANIFEST_FILENAME),
        );

        // Assert
        assert_eq!(status, LoadStatus::Loaded);
    }

    #[test]
    fn reload_mod对坏掉的内容文件返回failed而不影响调用方状态() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "broken",
            r#"{ namespace: "broken", version: "0.1.0" }"#,
            &[("terrain.json5", r#"{ terrains: [ { id: "broken:rock" "#)],
        );

        // Act
        let status = reload_mod(
            &root
                .path()
                .join("broken")
                .join(crate::discover::MANIFEST_FILENAME),
        );

        // Assert
        match status {
            LoadStatus::Failed(err) => assert_eq!(err.stage, LoadStage::Register),
            other => panic!("期望 Register 阶段的 Failed，实际 {other:?}"),
        }
    }
}
