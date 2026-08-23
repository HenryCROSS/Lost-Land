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
//!   -> 为每个「有脚本入口的 mod」各构造一个 ScriptEngine（全部先造齐）
//!   -> 按顺序逐个 mod：
//!        入口为空                   -> Loaded（纯数据 mod）
//!        否则逐个 .scm 入口，共用本 mod 那一个引擎：
//!          读文件                   -> IO 失败归入 LoadScript 阶段
//!          ScriptEngine::load_source -> 语法错误/白名单拒绝/超时/缺参
//!                                       归入 LoadScript 阶段；
//!                                       register-* 内部校验失败归入
//!                                       Register 阶段（见
//!                                       `classify_script_stage` 文档，
//!                                       这是一处已知的简化）
//! ```
//!
//! # 作用域单位是 mod，不是脚本文件
//!
//! 同一个 mod 的全部 `entry_points` 共用**同一个** [`ScriptEngine`]：
//! `races.scm` 里 `(define (helper) ...)` 定义的辅助函数，同一个 mod 的
//! `classes.scm` 直接可以调用。这些文件本来就是同一个作者写的同一份
//! 内容，拆成多个文件只是为了「本体的种族都写在哪」有个不用搜索就能
//! 回答的答案（见 `mods/lostland/mod.json5`），不该顺带把它们变成互相
//! 看不见的孤岛。
//!
//! **跨 mod 的命名空间隔离原样保住**：换一个 mod 就换一个全新引擎，
//! mod A 的 `define` 在 mod B 的全局环境里根本不存在。两条性质各有一条
//! 测试钉着，见本模块 `tests` 里
//! `同一个mod的后一个脚本能调用前一个脚本定义的辅助函数` 与
//! `跨mod的define互相不可见`。
//!
//! **逐文件编译这条边界保留**：同一个 mod 的多个脚本仍然是各自一次
//! `load_source`，**不拼接成一个大编译单元**。装载管线因此始终知道
//! 「当前这一条注册来自哪个文件」，后续批次的逐文件依赖声明与反向
//! 校验才有立足点。多编译几次在崩溃层面没有任何代价——危险的相邻
//! 关系是「编译 → 构造」，不是「编译 → 编译」（ADR 0028）。
//!
//! # 构造阶段先于编译阶段
//!
//! [`load_all`] 先把这一批要用的引擎**全部**构造出来，再开始编译第一
//! 个脚本。这不是优化，是 ADR 0028 那条上游内存安全缺陷的规避条件：
//! `steel-core` 0.8.2 只在「先编译、后构造」这个相邻关系上出问题。
//! 违反会被 `ll_script::host::ScriptEngine::new` 里的断言当场拦下，
//! 见该处注释。
//!
//! 本体内容分两半，与这条管线的关系**不一样**，这是本模块最容易被
//! 误读的一点：
//!
//! - **已迁进脚本的那一半**（当前是种族——`mods/lostland/races.json5`）
//!   走的**就是这条管线**，与任何第三方 mod 完全同一条路径，没有任何
//!   本体专属入口。它是**强制装载**的：装载完毕后
//!   `ll_game::content::load_content` 会用 [`crate::base_contract`] 的
//!   契约解析按 id 逐字段填充 `ll_mod::race::BaseRaceIds` 这类句柄，
//!   缺任何一条就整批失败。
//! - **尚未迁走的那一半**（[`crate::base_terrain::register_base_terrain`]/
//!   [`crate::base_placeholder::register_base_placeholder_content`] 等
//!   仍然存在的 `base_*` 模块）**不经过这条管线**——它们是一次直接的
//!   Rust 函数调用，没有清单、没有脚本，见各自模块文档。调用方应在跑
//!   本管线之前先调用一遍这些 `base_*` 注册函数。
//!
//! 两半共享同一个 [`crate::registry::Registry`] 与 [`GameplayTables`]
//! 里的各张内容表。

use std::path::{Path, PathBuf};

use ll_core::ident::NamespacedId;
use ll_script::host::{ScriptEngine, ScriptError};
use ll_world::space_profile::SpaceProfileTable;
use ll_world::terrain::TerrainTable;
use ll_world::weather::WeatherTable;

use crate::class::ClassTable;
use crate::clip::ClipTable;
use crate::item::ItemTable;
use crate::load_report::{LoadError, LoadReport, LoadStage, LoadStatus, SourceLocation};
use crate::manifest::{ModError, ModManifest, mod_self_id, parse_manifest};
use crate::module_sources::{ModuleSources, build_module_table, collect_module_sources};
use crate::quest::QuestTable;
use crate::race::RaceTable;
use crate::registry::Registry;
use crate::resource_pool::ResourcePoolTable;
use crate::skill::SkillTable;
use crate::subclass::SubclassTable;
use crate::trait_def::TraitTable;
use crate::{content_data, discover, topo};

use crate::active_registry::{set_active_registry, take_active_registry};
use crate::damage_category::DamageCategoryTable;
use crate::event::EventSubscriptionTable;
use crate::formula::FormulaTable;
use crate::recipe::RecipeTable;
use crate::recipe_category::RecipeCategoryTable;
use crate::script_class_api::{
    register_class_api, set_active_target as set_active_class_target,
    take_active_target as take_active_class_target,
};
use crate::script_clip_api::{
    register_clip_api, set_active_target as set_active_clip_target,
    take_active_target as take_active_clip_target,
};
use crate::script_damage_category_api::{
    register_damage_category_api, set_active_target as set_active_damage_category_target,
    take_active_target as take_active_damage_category_target,
};
use crate::script_damage_formula_api::{
    register_damage_formula_api, set_active_target as set_active_formula_target,
    take_active_target as take_active_formula_target,
};
use crate::script_event_api::{
    register_event_api, set_active_target as set_active_event_target,
    take_active_target as take_active_event_target,
};
use crate::script_item_api::{
    register_item_api, set_active_target as set_active_item_target,
    take_active_target as take_active_item_target,
};
use crate::script_quest_api::{
    register_quest_api, set_active_target as set_active_quest_target,
    take_active_target as take_active_quest_target,
};
use crate::script_race_api::{
    register_race_api, set_active_target as set_active_race_target,
    take_active_target as take_active_race_target,
};
use crate::script_recipe_api::{
    register_recipe_api, set_active_target as set_active_recipe_target,
    take_active_target as take_active_recipe_target,
};
use crate::script_recipe_category_api::{
    register_recipe_category_api, set_active_target as set_active_recipe_category_target,
    take_active_target as take_active_recipe_category_target,
};
use crate::script_resource_pool_api::{
    register_resource_pool_api, set_active_target as set_active_resource_pool_target,
    take_active_target as take_active_resource_pool_target,
};
use crate::script_skill_api::{
    register_skill_api, set_active_target as set_active_skill_target,
    take_active_target as take_active_skill_target,
};
use crate::script_space_profile_api::{
    register_space_profile_api, set_active_target as set_active_space_profile_target,
    take_active_target as take_active_space_profile_target,
};
use crate::script_subclass_api::{
    register_subclass_api, set_active_target as set_active_subclass_target,
    take_active_target as take_active_subclass_target,
};
use crate::script_tag_api::{
    register_tag_api, set_active_target as set_active_tag_target,
    take_active_target as take_active_tag_target,
};
use crate::script_terrain_api::{
    register_terrain_api, set_active_target as set_active_terrain_target,
    take_active_target as take_active_terrain_target,
};
use crate::script_trait_api::{
    register_trait_api, set_active_target as set_active_trait_target,
    take_active_target as take_active_trait_target,
};
use crate::script_weapon_category_api::{
    register_weapon_category_api, set_active_target as set_active_weapon_category_target,
    take_active_target as take_active_weapon_category_target,
};
use crate::script_weather_api::{
    register_weather_api, set_active_target as set_active_weather_target,
    take_active_target as take_active_weather_target,
};
use crate::script_xp_curve_api::{
    register_xp_curve_api, set_active_target as set_active_xp_curve_target,
    take_active_target as take_active_xp_curve_target,
};
use crate::tag::TagTable;
use crate::weapon_category::WeaponCategoryTable;
use crate::xp_curve::{XpCurveBindings, XpCurveTable};

/// 加载管线一次装载会话内，脚本注册函数可以写入的全部内容表——地形、
/// 职业、技能、副职、任务、种族、动画剪辑、经验曲线（含绑定）、天赋、
/// 资源池、物品、伤害公式、武器类别、伤害类别、空间层属性、天气、
/// 配方、配方类别。
///
/// 集中成一个结构体，而不是让 [`load_all`]/[`compile_one_script`] 各自
/// 接收十九个独立的 `&mut` 参数：这些表在装载管线里总是同进同出（同一
/// 份 mod 脚本可能在同一个文件里先后调用 `register-terrain`/
/// `register-class`/……），拆成十九个位置参数只会让调用点的参数顺序成为
/// 易错点，结构体把「这些表必须一起传」这条约束在类型上表达出来。
/// `Registry` 不在这个结构体里——它走 [`crate::active_registry`] 单独
/// 的共享目标，理由见该模块文档。
///
/// # 字段个数就是「mod 能注册几类玩法层内容」的唯一权威清单
///
/// 每新增一个字段，都对应 ADR 0018「玩法层内容都能从 mod 脚本注册」
/// 这条要求上补掉的一处缺口。`space_profile` 曾是最近的一次：空间层
/// 属性早就有 `ll-world` 侧的表、有本体侧的生产注册路径、也早就进了
/// [`crate::content_hash`] 的值哈希覆盖面，**唯独脚本注册函数一直不
/// 存在**，六个字段只能由 Rust 写死——这正是「声明了但从没接线」这类
/// 缺口最典型的形态：每一环单独看都在，串起来才发现断了一节。
/// `weather` 曾是最新的一个字段（天气系统批次），与当时其余十六张表不同的
/// 是它从第一天起就是完整的：表、本体注册路径、脚本注册函数、值哈希、
/// 装载后校验、真实消费者（环境光管线）在同一个批次里一起落地。
/// `recipe`/`recipe_category` 是最新的两个字段（制作系统批次），同样
/// 从第一天起就是完整的：两张表、脚本注册函数、值哈希（版本 11）、
/// 装载后校验、真实消费者（`ll_sim::resolve::resolve_craft`）与真实
/// mod 脚本证据（`mods/example_mod/gameplay.scm`）在同一个批次里一起
/// 落地。**唯一如实存在的缺口在更上游**：`Intent::Craft` 至今没有任何
/// 产出者（没有制作界面），见该变体自己的文档。
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
    /// 动画剪辑表——不进 `WorldState`、不参与 `WorldState::hash()`
    /// （ADR 0020 甲区，见 `crate::clip` 模块文档），但注册路径与另外
    /// 八张表完全一致，随装载会话同进同出。
    pub clip: &'a mut ClipTable,
    /// 经验曲线定义表（等级与经验系统落地批次新增）——`register-xp-curve`
    /// 的写入目标，见 `crate::xp_curve` 模块文档。
    pub xp_curve: &'a mut XpCurveTable,
    /// 职业/种族 → 经验曲线的绑定表——`register-class-xp-curve`/
    /// `register-race-xp-curve` 的写入目标。与 `xp_curve` 分成两个字段
    /// 而不是一个元组字段，是为了和其余各表同样走
    /// `std::mem::take(tables.xp_curve_bindings)` 这条既有搬运手法，不
    /// 需要为这一对表单独发明搬运方式。
    pub xp_curve_bindings: &'a mut XpCurveBindings,
    /// 天赋表（天赋系统落地批次新增）——`register-trait` 的写入目标，
    /// 见 `crate::trait_def` 模块文档。
    pub trait_def: &'a mut TraitTable,
    /// 资源池表（资源池落地批次新增，第一批：法力池/血池）——
    /// `register-resource-pool` 的写入目标，见 `crate::resource_pool`
    /// 模块文档。
    pub resource_pool: &'a mut ResourcePoolTable,
    /// 物品表（P6 第一批：物品基础新增）——`register-item` 的写入
    /// 目标，见 `crate::item` 模块文档。
    pub item: &'a mut ItemTable,
    /// 伤害公式定义表（伤害公式引擎批次新增）——`register-damage-formula`
    /// 的写入目标，见 `crate::formula` 模块文档。
    pub formula: &'a mut FormulaTable,
    /// 武器类别定义表（伤害类别/抗性接线批次新增）——
    /// `register-weapon-category` 的写入目标，见 `crate::weapon_category`
    /// 模块文档。
    pub weapon_category: &'a mut WeaponCategoryTable,
    /// 伤害类别定义表（伤害类别/抗性接线批次新增）——
    /// `register-tag` 的写入目标，见 `crate::tag` 模块文档（耐久标签
    /// 批次）。
    pub tag: &'a mut TagTable,

    /// `register-damage-category` 的写入目标，见 `crate::damage_category`
    /// 模块文档。
    pub damage_category: &'a mut DamageCategoryTable,
    /// 运行期事件订阅表（事件监听 API 批次新增）——`on-event` 的写入
    /// 目标，见 `crate::event` 模块文档。
    ///
    /// **这不是一张内容表**：它里面没有任何 `ContentIndex`，因此不进
    /// `crate::content_hash::ContentValueTables`、不需要 `classify_index`
    /// 认领、不进存档 remap，`CONTENT_HASH_ALGORITHM_VERSION` 也不因它
    /// 递增。它之所以仍然住在本结构体里，是因为它的**写入通道**与
    /// 那二十张表逐字相同（装载期脚本函数、`thread_local!` 活跃目标、
    /// 每个 mod 一个调用窗口），走同一条路比另开一条平行路径少一份
    /// 需要各自维护的顺序约定。完整论证见 `crate::event` 模块文档
    /// 「这不是一张内容表」一节。
    pub events: &'a mut EventSubscriptionTable,
    /// 空间层属性表（空间层属性脚本注册批次新增）——
    /// `register-space-profile` 的写入目标，见
    /// `crate::script_space_profile_api` 模块文档。
    ///
    /// 与另外十八张表的一处不同：这张表在装载会话开始**之前**通常已经
    /// 非空（`ll_mod::base_space_profile::register_base_space_profiles`
    /// 先注册了本体四种空间类型），mod 脚本是往里追加。这与地形表
    /// （`register_base_terrain` 同样先跑）是同一种情形，不是本字段
    /// 独有的例外——`SpaceProfileTable::define` 的重复定义校验保证 mod
    /// 覆盖不掉本体已声明的那几条。
    pub space_profile: &'a mut SpaceProfileTable,
    /// 配方表（制作系统批次新增）——`register-recipe` 的写入目标，见
    /// `crate::recipe` 模块文档。
    pub recipe: &'a mut RecipeTable,
    /// 配方类别表（制作系统批次新增）——`register-recipe-category` 的
    /// 写入目标，见 `crate::recipe_category` 模块文档。
    ///
    /// 与 `recipe` 分成两个字段而不是一个元组字段，理由同
    /// `xp_curve`/`xp_curve_bindings` 那一对：两张表各自走
    /// `std::mem::take(tables.……)` 这条既有搬运手法，不需要为这一对
    /// 单独发明搬运方式。
    pub recipe_category: &'a mut RecipeCategoryTable,
    /// 天气表（天气系统批次新增）——`register-weather` 的写入目标，见
    /// `crate::script_weather_api` 模块文档。
    ///
    /// 与 `space_profile` 同一种情形：这张表在装载会话开始**之前**通常
    /// 已经非空（`ll_mod::base_weather::register_base_weathers` 先注册了
    /// 本体六种天气），mod 脚本是往里追加，`WeatherTable::define` 的重复
    /// 定义校验保证 mod 覆盖不掉本体已声明的那几条。
    pub weather: &'a mut WeatherTable,
}

/// 跑一次完整的 mod 装载会话：发现 `mods_root` 下的候选、解析、拓扑
/// 排序、按序加载脚本、注册内容——写入 `registry`/`tables`，返回一份
/// 报告。
///
/// `registry`/`tables` 应当已经装过**尚未迁进脚本的那部分**本体内容
/// （`register_base_terrain`/`register_base_placeholder_content` 等）：
/// 本函数只管 mod 目录，不知道、也不需要知道它们是怎么注册进去的
/// ——这正是「本体即 Mod」在管线层面的体现：那部分注册发生在调用本
/// 函数**之前**的一次独立调用，mod 内容随后 intern 进同一个
/// `Registry`，两者共用同一段单调递增的 `ContentIndex` 号段（见
/// `crate::base_terrain` 模块文档与其测试）。
///
/// 已经迁进脚本的本体内容（`mods/lostland/`）反过来是**本函数自己**
/// 装载的，与任何第三方 mod 走同一条路径——调用方随后必须跑一次契约
/// 解析（[`crate::race::resolve_base_races`]）确认它真的在，见
/// [`crate::base_contract`] 模块文档。
pub fn load_all(
    mods_root: &Path,
    registry: &mut Registry,
    tables: &mut GameplayTables,
) -> LoadReport {
    let mut report = LoadReport::new();

    let candidates = discover::discover_mods(mods_root);
    let mut parsed: Vec<ModManifest> = Vec::new();
    // 与 `parsed` 平行的「这个 mod 的根目录」——`ModManifest` 自己不
    // 记根目录（`entry_points` 已经是解析好的路径），而模块表要遍历
    // 整个目录，所以在这里顺手留一份。纯数据 mod 没有入口脚本，从
    // `entry_points` 反推不出根目录，只能来自清单路径本身。
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

    // 阶段一：把这一批要用到的引擎**全部**构造出来，一个脚本都还没
    // 编译。`order` 来自 `topo_sort`，是确定性序列（约束 C5），因此
    // 「第几个引擎归第几个 mod」也是确定的。
    //
    // 构造引擎不需要任何装载期数据：清单是 JSON5，不经 Steel 编译器，
    // 「读完清单 → 知道有几个带脚本的 mod → 造几个引擎」这个顺序成立。
    // `register_*_api` 也在这一阶段完成——它只往符号表里塞 Rust 函数
    // 指针，不编译任何 Steel 源码。
    let scripted: Vec<usize> = order
        .iter()
        .copied()
        .filter(|idx| !parsed[*idx].entry_points.is_empty())
        .collect();
    // 模块表要在**构造引擎之前**备好：解析器是构造期装上去的，晚一步
    // 这一批脚本就没有模块系统。收集是纯 IO，不碰 Steel 编译器，放在
    // 构造阶段之前不违反 C6。
    let sources_by_namespace: Vec<(String, ModuleSources)> = parsed
        .iter()
        .zip(&roots)
        .map(|(manifest, root)| {
            let namespace = manifest.id.namespace().to_string();
            let sources = collect_module_sources(root, &namespace);
            (namespace, sources)
        })
        .collect();
    let engines: Vec<ScriptEngine> = scripted
        .iter()
        .map(|idx| new_load_engine(&parsed[*idx], &sources_by_namespace))
        .collect();

    // 阶段二：逐个 mod 编译它自己的全部入口脚本。每个引擎在它那个 mod
    // 编译完之后就地析构——约束 C6 禁的是「编译之后再构造」，析构不受
    // 限制，因此内存峰值只存在于装载期间的前半段。
    let mut engines = engines.into_iter();
    for idx in order {
        let manifest = &parsed[idx];

        // 内容数据文件（`crate::content_data`）排在这个 mod 自己的脚本
        // 之前：**声明先于逻辑**——同一个 mod 的行为脚本因此可以引用
        // 它自己刚刚声明的内容，反过来不成立（数据文件里没有任何能
        // 调用脚本的东西）。跨 mod 的先后仍然是外层这个拓扑序，与脚本
        // 共用同一份，见 `crate::content_data` 模块文档「顺序确定性」。
        //
        // 一个内容文件坏了，整个 mod 判 `Failed` 并跳过它的脚本——与
        // 「脚本编译失败就整个 mod 失败」同一档严重性：半份内容比没有
        // 内容更难查（症状是运行期某条引用悬空，不是启动期一条错误）。
        if let Err(err) = content_data::load_mod_content_data(&roots[idx], registry, tables) {
            report.push(
                manifest.id.clone(),
                LoadStatus::Failed(LoadError {
                    mod_id: manifest.id.clone(),
                    stage: LoadStage::Register,
                    message: err.message.clone(),
                    location: Some(SourceLocation {
                        file: err.file.clone(),
                        // 行号在 `err.message` 里（json5 的
                        // `... at line N column M`）。`SourceLocation::line`
                        // 是 `Option<usize>`，要填进去得把那串文案反过来
                        // 解析一遍——从结构化错误退化成文本再解析回结构，
                        // 是一条只会引入分歧的路，不走。
                        line: None,
                    }),
                }),
            );
            // 这个 mod 不跑脚本了，但**它那台引擎必须照样从迭代器里取
            // 走**：`engines` 是按 `scripted` 过滤条件、以同一个拓扑序
            // 造出来的，跳过一个而不消费它，后面每一个 scripted mod 都
            // 会拿到别人的引擎（症状是「另一个 mod 的脚本报了本 mod 的
            // 错」，极难查）。取出来立刻析构，不违反 C6（C6 禁的是编译
            // 之后再构造，析构不受限制）。
            if !manifest.entry_points.is_empty() {
                drop(engines.next());
            }
            continue;
        }

        if manifest.entry_points.is_empty() {
            // 纯数据 mod（清单允许没有脚本入口，见 manifest.rs 文档），
            // 没有脚本可跑，直接算加载成功。
            report.push(manifest.id.clone(), LoadStatus::Loaded);
            continue;
        }

        let mut engine = engines
            .next()
            .expect("阶段一按同一个 scripted 过滤条件造了同样多的引擎");
        let mut failure = None;
        for entry in &manifest.entry_points {
            if let Err(err) = compile_one_script(manifest, entry, &mut engine, registry, tables) {
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

/// 构造一个装载期脚本引擎，并把全部 `register-*` 注册函数挂上去。
///
/// **只构造、只注册，绝不编译任何脚本源码**——这是「同一根线程上全部
/// 引擎构造必须先于全部脚本编译」这条约束（见 `ll_script::host` 里
/// `COMPILED_ON_THIS_THREAD` 上方注释与 ADR 0028）在本管线里的落点：
/// [`load_all`] 先把这个函数调 N 次，再开始调 [`compile_one_script`]。
///
/// 权威清单是本函数里 `register_*_api` 的调用序列与 [`GameplayTables`]
/// 的字段，不是任何一段文档。
/// 扫一遍 `mods_root` 下的全部 mod，收集「命名空间 → 该 mod 目录里的
/// 全部 `.scm` 源码」。
///
/// 清单解析失败的目录直接跳过：它本来就装不上，它的模块也不该被别人
/// require 到。
fn collect_session_module_sources(mods_root: &Path) -> Vec<(String, ModuleSources)> {
    discover::discover_mods(mods_root)
        .into_iter()
        .filter_map(|manifest_path| {
            let manifest = parse_manifest(&manifest_path).ok()?;
            let root = manifest_path.parent()?.to_path_buf();
            let namespace = manifest.id.namespace().to_string();
            let sources = collect_module_sources(&root, &namespace);
            Some((namespace, sources))
        })
        .collect()
}

fn new_load_engine(
    manifest: &ModManifest,
    sources_by_namespace: &[(String, ModuleSources)],
) -> ScriptEngine {
    let table = build_module_table(manifest, sources_by_namespace);
    let mut engine = ScriptEngine::with_modules(std::sync::Arc::new(table));
    register_terrain_api(&mut engine);
    register_class_api(&mut engine);
    register_skill_api(&mut engine);
    register_subclass_api(&mut engine);
    register_quest_api(&mut engine);
    register_race_api(&mut engine);
    register_clip_api(&mut engine);
    register_xp_curve_api(&mut engine);
    register_trait_api(&mut engine);
    register_resource_pool_api(&mut engine);
    register_item_api(&mut engine);
    register_damage_formula_api(&mut engine);
    register_weapon_category_api(&mut engine);
    register_damage_category_api(&mut engine);
    register_tag_api(&mut engine);
    register_space_profile_api(&mut engine);
    register_recipe_api(&mut engine);
    register_recipe_category_api(&mut engine);
    register_weather_api(&mut engine);
    register_event_api(&mut engine);
    engine
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
    let mut clip = ClipTable::new();
    let mut xp_curve = XpCurveTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
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
    let mut weather = WeatherTable::new();
    // 一键重载不消费订阅表——它只回答「这个 mod 现在能不能干净地
    // 加载」，不写回正在运行的会话（见本函数文档）。
    let mut events = EventSubscriptionTable::new();
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
        trait_def: &mut trait_def,
        resource_pool: &mut resource_pool,
        item: &mut item,
        formula: &mut formula,
        weapon_category: &mut weapon_category,
        damage_category: &mut damage_category,
        tag: &mut tag,
        space_profile: &mut space_profile,
        weather: &mut weather,
        recipe: &mut recipe,
        recipe_category: &mut recipe_category,
        events: &mut events,
    };
    // 与 `load_all` 同一条作用域规则：一个 mod 一个引擎，该 mod 的全部
    // 入口脚本共用它。构造在全部编译之前完成（这里只有一个 mod，天然
    // 满足）。
    //
    // 模块表要覆盖本 mod **与它声明的依赖**，所以得把 mods 根目录下的
    // 清单都扫一遍——只看本 mod 目录的话，跨 mod require 会在重载时报
    // 「找不到模块」，而同一份 mod 走 `load_all` 却装得上，重载给出的
    // 信号就成了假的。
    let mods_root = manifest_path.parent().and_then(Path::parent);
    let sources_by_namespace = match mods_root {
        Some(root) => collect_session_module_sources(root),
        None => Vec::new(),
    };
    let mut engine = new_load_engine(&manifest, &sources_by_namespace);
    for entry in &manifest.entry_points {
        if let Err(err) =
            compile_one_script(&manifest, entry, &mut engine, &mut registry, &mut tables)
        {
            return LoadStatus::Failed(err);
        }
    }
    LoadStatus::Loaded
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

/// 在 `engine`（本 mod 专属、已经注册好全部 `register-*` 的引擎）上
/// 编译单个脚本文件：读文件、跑 `ScriptEngine::load_source`，成功时
/// `register-terrain`/`register-class`/`register-skill`/
/// `register-subclass`/`register-quest`/`register-race`/
/// `register-space-profile`/…… 的效果已经写进 `registry`/`tables`
/// ——权威清单是 [`GameplayTables`] 的字段与本函数里 `register_*_api`
/// 的调用序列，不是这段文档。
fn compile_one_script(
    manifest: &ModManifest,
    entry: &Path,
    engine: &mut ScriptEngine,
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

    // 把 registry 与 GameplayTables 的全部各表整体移进各自的线程局部
    // 存储，供对应的 register-* 函数在脚本求值期间写入；脚本跑完（不论
    // 成功失败）都要原样移回——`ScriptEngine::load_source` 本身不会
    // panic（四道防线①②），这里不需要 catch_unwind 之类的补救。
    // `Registry` 走 `crate::active_registry` 的共享目标（全部 register-*
    // 函数必须共用同一个 `Registry` 实例，理由见该模块文档），各张表
    // 各自走自己模块的 `thread_local!`。
    set_active_registry(std::mem::take(registry));
    set_active_terrain_target(std::mem::take(tables.terrain));
    set_active_class_target(std::mem::take(tables.class));
    set_active_skill_target(std::mem::take(tables.skill));
    set_active_subclass_target(std::mem::take(tables.subclass));
    set_active_quest_target(std::mem::take(tables.quest));
    set_active_race_target(std::mem::take(tables.race));
    set_active_clip_target(std::mem::take(tables.clip));
    set_active_xp_curve_target(
        std::mem::take(tables.xp_curve),
        std::mem::take(tables.xp_curve_bindings),
    );
    set_active_trait_target(std::mem::take(tables.trait_def));
    set_active_resource_pool_target(std::mem::take(tables.resource_pool));
    set_active_item_target(std::mem::take(tables.item));
    set_active_formula_target(std::mem::take(tables.formula));
    set_active_weapon_category_target(std::mem::take(tables.weapon_category));
    set_active_damage_category_target(std::mem::take(tables.damage_category));
    set_active_tag_target(std::mem::take(tables.tag));
    set_active_space_profile_target(std::mem::take(tables.space_profile));
    set_active_recipe_target(std::mem::take(tables.recipe));
    set_active_recipe_category_target(std::mem::take(tables.recipe_category));
    set_active_weather_target(std::mem::take(tables.weather));
    // 事件订阅表比其余各表多带一份「这个窗口属于哪个 mod」——订阅方
    // 是谁不能由脚本自己说了算，见 `crate::script_event_api` 模块文档
    // 最后一段。
    set_active_event_target(std::mem::take(tables.events), manifest.id.namespace());

    let result = engine.load_source(source.clone());

    *registry = take_active_registry();
    *tables.terrain = take_active_terrain_target();
    *tables.class = take_active_class_target();
    *tables.skill = take_active_skill_target();
    *tables.subclass = take_active_subclass_target();
    *tables.quest = take_active_quest_target();
    *tables.race = take_active_race_target();
    *tables.clip = take_active_clip_target();
    let (xp_curve, xp_curve_bindings) = take_active_xp_curve_target();
    *tables.xp_curve = xp_curve;
    *tables.xp_curve_bindings = xp_curve_bindings;
    *tables.trait_def = take_active_trait_target();
    *tables.resource_pool = take_active_resource_pool_target();
    *tables.item = take_active_item_target();
    *tables.formula = take_active_formula_target();
    *tables.weapon_category = take_active_weapon_category_target();
    *tables.damage_category = take_active_damage_category_target();
    *tables.tag = take_active_tag_target();
    *tables.space_profile = take_active_space_profile_target();
    *tables.recipe = take_active_recipe_target();
    *tables.recipe_category = take_active_recipe_category_target();
    *tables.weather = take_active_weather_target();
    *tables.events = take_active_event_target();

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
/// **已知简化**：本管线注册给脚本的、会产生副作用的能力现在有十六个
/// （`register-terrain`/`register-class`/`register-skill`/
/// `register-subclass`/`register-quest`/`register-race`/
/// `register-animation-clip`/`register-xp-curve`/
/// `register-class-xp-curve`/`register-race-xp-curve`/`register-trait`/
/// `register-race-trait`/`register-resource-pool`/
/// `register-trait-resource-pool`/`register-item`/
/// `register-space-profile`），把
/// `ScriptError::Runtime`（任一 `register-*` 内部校验失败时都走这一类，
/// 见各自模块文档「返回 Result<bool, String>」一节）整体归为 Register
/// 阶段。这会把一个与内容注册无关、纯粹是脚本自身写错的运行时错误
/// （比如引用了一个已声明但尚未 `define` 的变量）也误标成 Register
/// ——原始简化写下时只有 `register-terrain` 一个注册函数，补齐其余
/// 十一个之后这条简化本身没有变得更精确（十二个函数的运行时错误依然与
/// 「脚本自身写错」共用同一个 `ScriptError::Runtime` 变体，无法从错误
/// 类型本身区分），仍然是一处已知的简化，不是本批次修掉的缺口——若
/// 未来需要更精确的判据，需要让每个注册函数把自己的错误包一层可辨识
/// 的前缀。
fn classify_script_stage(err: &ScriptError) -> LoadStage {
    match err {
        ScriptError::Runtime(..) => LoadStage::Register,
        ScriptError::ParseError(..)
        | ScriptError::ArityMismatch(..)
        | ScriptError::Timeout
        | ScriptError::MemoryBudgetExceeded { .. } => LoadStage::LoadScript,
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
    use crate::test_support::{OwnedTables, tempdir};
    use ll_world::terrain::TerrainKind;
    use std::fs;

    /// 在 `root` 下建一个候选 mod 子目录，写入清单与（可选）脚本。
    fn write_mod(root: &Path, dir_name: &str, manifest_json5: &str, script: Option<&str>) {
        let mod_dir = root.join(dir_name);
        fs::create_dir_all(&mod_dir).expect("创建 mod 子目录");
        fs::write(
            mod_dir.join(crate::discover::MANIFEST_FILENAME),
            manifest_json5,
        )
        .expect("写入清单");
        if let Some(source) = script {
            fs::write(mod_dir.join("main.scm"), source).expect("写入脚本");
        }
    }

    /// [`write_mod`] 的多脚本版本：一个 mod 目录下写入任意多个 `.scm`
    /// 文件，供「同一个 mod 的多个入口点」相关测试使用。
    fn write_mod_scripts(
        root: &Path,
        dir_name: &str,
        manifest_json5: &str,
        scripts: &[(&str, &str)],
    ) {
        let mod_dir = root.join(dir_name);
        fs::create_dir_all(&mod_dir).expect("创建 mod 子目录");
        fs::write(
            mod_dir.join(crate::discover::MANIFEST_FILENAME),
            manifest_json5,
        )
        .expect("写入清单");
        for (name, source) in scripts {
            fs::write(mod_dir.join(name), source).expect("写入脚本");
        }
    }

    /// 作用域单位是 **mod**，不是脚本文件：同一个 mod 的多个入口点共用
    /// 同一个 `ScriptEngine`，前一个文件里 `define` 出来的辅助函数，后一个
    /// 文件真的能调用。
    ///
    /// 这一条此前不成立——每个脚本文件各拿一个全新引擎，`races.scm` 里
    /// 的 `define` 对同一个 mod 的 `classes.scm` 不可见。
    #[test]
    fn 同一个mod的后一个脚本能调用前一个脚本定义的辅助函数() {
        // Arrange
        let root = tempdir();
        write_mod_scripts(
            root.path(),
            "sharemod",
            r#"{
                namespace: "sharemod",
                version: "0.1.0",
                entry_points: ["helpers.scm", "content.scm"],
            }"#,
            &[
                ("helpers.scm", r#"(define (lava-id) "sharemod:lava_floor")"#),
                (
                    "content.scm",
                    r#"(register-terrain (lava-id) #f #t 4294967295 "")"#,
                ),
            ],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        assert_eq!(
            report.entries,
            vec![(mod_self_id("sharemod").unwrap(), LoadStatus::Loaded)],
            "同一个 mod 内的辅助函数应当跨文件可见"
        );
        assert!(
            registry
                .get(&NamespacedId::parse("sharemod:lava_floor").unwrap())
                .is_some()
        );
    }

    /// 模块系统在**生产装载路径**上的六条判据（同 mod / 跨 mod 放行、
    /// 跨 mod 拒绝、路径拒绝、未导出不可见、环 import 不挂死）各写一条。
    ///
    /// 与 `ll_script::host` 里那批单元测试的分工：那边钉的是引擎自己的
    /// 行为（拿一张手工灌好的表），这边钉的是「表真的由 mod 目录与
    /// mod.json5 生成，并真的装到了那个 mod 的引擎上」——两边都绿才
    /// 说明这条链是通的。
    fn 装载报告里那条失败的消息(report: &LoadReport) -> String {
        report
            .entries
            .iter()
            .find_map(|(_, status)| match status {
                LoadStatus::Failed(err) => Some(err.message.clone()),
                LoadStatus::Loaded | LoadStatus::Warning(_) => None,
            })
            .unwrap_or_else(|| format!("整批装载没有任何失败：{:?}", report.entries))
    }

    #[test]
    fn 同mod的require在真实装载路径上能用() {
        // Arrange
        let root = tempdir();
        write_mod_scripts(
            root.path(),
            "modmod",
            r#"{
                namespace: "modmod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            &[
                (
                    "helpers.scm",
                    r#"(provide lava-id) (define (lava-id) "modmod:lava_floor")"#,
                ),
                (
                    "main.scm",
                    "(require \"helpers\")\n(register-terrain (lava-id) #f #t 4294967295 \"\")",
                ),
            ],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        assert_eq!(
            report.entries,
            vec![(mod_self_id("modmod").unwrap(), LoadStatus::Loaded)],
            "实际 {:?}",
            report.entries
        );
        assert!(
            registry
                .get(&NamespacedId::parse("modmod:lava_floor").unwrap())
                .is_some()
        );
    }

    #[test]
    fn 跨mod带前缀的require在声明过依赖时能用() {
        // Arrange
        let root = tempdir();
        write_mod_scripts(
            root.path(),
            "basemod",
            r#"{ namespace: "basemod", version: "0.1.0" }"#,
            &[(
                "ids.scm",
                r#"(provide base-id) (define (base-id 名字) (string-append "usermod:" 名字))"#,
            )],
        );
        write_mod_scripts(
            root.path(),
            "usermod",
            r#"{
                namespace: "usermod",
                version: "0.1.0",
                dependencies: ["basemod"],
                entry_points: ["main.scm"],
            }"#,
            &[(
                "main.scm",
                "(require \"basemod:ids\")\n(register-terrain (base-id \"lava\") #f #t 4294967295 \"\")",
            )],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        assert!(
            report
                .entries
                .iter()
                .all(|(_, status)| *status == LoadStatus::Loaded),
            "实际 {:?}",
            report.entries
        );
        assert!(
            registry
                .get(&NamespacedId::parse("usermod:lava").unwrap())
                .is_some(),
            "跨 mod 拿来的辅助函数应当真的参与了注册"
        );
    }

    #[test]
    fn 跨mod未声明依赖时require被拒绝并点名去补依赖() {
        // Arrange：源码明明在盘上，差的只是 mod.json5 里那一行声明。
        let root = tempdir();
        write_mod_scripts(
            root.path(),
            "basemod",
            r#"{ namespace: "basemod", version: "0.1.0" }"#,
            &[("ids.scm", r#"(provide base-id) (define (base-id) "x")"#)],
        );
        write_mod_scripts(
            root.path(),
            "usermod",
            r#"{
                namespace: "usermod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            &[("main.scm", "(require \"basemod:ids\")")],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        let message = 装载报告里那条失败的消息(&report);
        assert!(
            message.contains("dependencies"),
            "该告诉 mod 作者去清单里补依赖，实际是「{message}」"
        );
    }

    #[test]
    fn 绝对路径与上跳目录的require在真实装载路径上被拒绝() {
        for (标记, 模块名, 关键词) in [
            ("abs", "C:/Windows/win.ini", "绝对路径"),
            ("up", "../../secret", "上跳目录"),
        ] {
            // Arrange
            let root = tempdir();
            write_mod_scripts(
                root.path(),
                标记,
                &format!(
                    r#"{{ namespace: "{标记}", version: "0.1.0", entry_points: ["main.scm"] }}"#
                ),
                &[("main.scm", &format!("(require \"{模块名}\")"))],
            );
            let mut registry = Registry::new();
            let mut owned = OwnedTables::default();

            // Act
            let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

            // Assert
            let message = 装载报告里那条失败的消息(&report);
            assert!(
                message.contains(关键词),
                "「{模块名}」该被点名为{关键词}，实际是「{message}」"
            );
        }
    }

    #[test]
    fn 没写进provide的名字在真实装载路径上拿不到() {
        // Arrange
        let root = tempdir();
        write_mod_scripts(
            root.path(),
            "hidemod",
            r#"{
                namespace: "hidemod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            &[
                (
                    "helpers.scm",
                    r#"(provide 公开) (define (公开) "hidemod:a") (define (私有) "hidemod:b")"#,
                ),
                (
                    "main.scm",
                    "(require \"helpers\")\n(register-terrain (私有) #f #t 4294967295 \"\")",
                ),
            ],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        let message = 装载报告里那条失败的消息(&report);
        assert!(
            message.contains("私有"),
            "该点名那个没导出的名字，实际是「{message}」"
        );
        assert!(
            registry
                .get(&NamespacedId::parse("hidemod:b").unwrap())
                .is_none()
        );
    }

    #[test]
    fn 环import在真实装载路径上干净报错不挂死() {
        // 本条测试跑完本身就是「没挂死」的证据——挂死的话整批测试会
        // 一直等下去，而不是给出一条失败。
        // Arrange
        let root = tempdir();
        write_mod_scripts(
            root.path(),
            "cyclemod",
            r#"{
                namespace: "cyclemod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            &[
                ("a.scm", "(require \"b\")\n(provide fa)\n(define (fa) 1)"),
                ("b.scm", "(require \"a\")\n(provide fb)\n(define (fb) 2)"),
                ("main.scm", "(require \"a\")"),
            ],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        let message = 装载报告里那条失败的消息(&report);
        assert!(
            message.contains("circular"),
            "该报环依赖，实际是「{message}」"
        );
    }

    #[test]
    fn 模块源码里的requirebuiltin在真实装载路径上被拒绝() {
        // 模块体不经过 `ScriptEngine::load_source` 的文本层检查，白名单
        // 又拦不住它（见 `ll_script::modules::ModuleTable::insert` 文档）
        // ——唯一挡得住的是灌表那一刻的检查。这条测试钉的就是那条链在
        // 生产路径上真的接着。
        // Arrange
        let root = tempdir();
        write_mod_scripts(
            root.path(),
            "evilmod",
            r#"{
                namespace: "evilmod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            &[
                (
                    "evil.scm",
                    "(require-builtin steel/time)\n(provide 现在)\n(define (现在) (instant/now))",
                ),
                ("main.scm", "(require \"evil\")\n(现在)"),
            ],
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        let message = 装载报告里那条失败的消息(&report);
        assert!(message.contains("require-builtin"), "实际是「{message}」");
    }

    /// 跨 **mod** 的命名空间隔离必须原样保住：mod A 的 `define` 绝不能被
    /// mod B 看见。
    ///
    /// 隔离靠「换一个引擎」实现——每个 mod 一个全新 `ScriptEngine`，新
    /// 引擎的全局环境里没有 A 的任何绑定，白名单的「本引擎已定义名字」
    /// 集合也是空的。本用例让 `peeker` 显式依赖 `definer`（拓扑排序因此
    /// 保证 `definer` 先装载，`define` 确实已经执行过），再引用它的辅助
    /// 函数——必须失败。
    #[test]
    fn 跨mod的define互相不可见() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "definer",
            r#"{
                namespace: "definer",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            Some(
                r#"(define (shared-id) "definer:rock")
                   (register-terrain (shared-id) #f #t 4294967295 "")"#,
            ),
        );
        write_mod(
            root.path(),
            "peeker",
            r#"{
                namespace: "peeker",
                version: "0.1.0",
                dependencies: ["definer"],
                entry_points: ["main.scm"],
            }"#,
            Some(r#"(register-terrain (shared-id) #f #t 4294967295 "")"#),
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert：definer 装载成功（证明 `shared-id` 这个 define 真的执行
        // 过），peeker 因为引用不到它而失败。
        let definer = report
            .entries
            .iter()
            .find(|(id, _)| *id == mod_self_id("definer").unwrap())
            .map(|(_, status)| status)
            .expect("definer 应当出现在报告里");
        assert_eq!(definer, &LoadStatus::Loaded);

        let peeker = report
            .entries
            .iter()
            .find(|(id, _)| *id == mod_self_id("peeker").unwrap())
            .map(|(_, status)| status)
            .expect("peeker 应当出现在报告里");
        match peeker {
            LoadStatus::Failed(err) => assert!(
                err.message.contains("shared-id"),
                "失败原因应当点名这个跨 mod 不可见的标识符，实际：{}",
                err.message
            ),
            other => panic!("跨 mod 的 define 绝不能可见，实际状态：{other:?}"),
        }
        assert!(
            registry
                .get(&NamespacedId::parse("peeker:rock").unwrap())
                .is_none()
        );
    }

    #[test]
    fn 纯数据mod没有脚本入口时直接判定为已加载() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "puredata",
            r#"{ namespace: "puredata", version: "0.1.0" }"#,
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
            r#"{
                namespace: "examplemod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
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
    fn mod脚本能注册自定义空间层属性并与本体已注册的四种共存() {
        // 这是「空间层属性脚本注册」这条通道的端到端验收：一个真实
        // 落在磁盘上的 mod（mod.json5 + main.scm）经过完整的
        // 发现→解析→拓扑排序→脚本求值→写回内容表流程，注册出一种
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
            r#"{
                namespace: "spacemod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            Some(
                r#"(register-space-profile "spacemod:abyss" 0 #f -40 #t #f "")
                   (register-space-profile "spacemod:greenhouse" 900 #t 260 #f #t "spacemod:glass")"#,
            ),
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
            vec![(mod_self_id("spacemod").unwrap(), LoadStatus::Loaded)]
        );
        let abyss = registry
            .get(&NamespacedId::parse("spacemod:abyss").unwrap())
            .expect("脚本注册的空间层属性应当进了 Registry");
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
            "非空 reverb-tag 应当按字面标识符存下"
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
    fn 脚本注册的非露天空间经真实装载后环境光不随世界时钟变化() {
        // 本测试钉住的是「脚本注册出来的层属性与 Rust 注册出来的语义
        // 逐字相同」这件事——`ll_world::space_profile::effective_ambient_light`
        // 那条组合规则（露天转发昼夜曲线、非露天取地板值）不知道也不
        // 需要知道这条内容是从哪条通道进来的。这正是 ADR 0018「本体
        // 与 mod 用同一套 API」在这张表上的可执行形式。
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "spacemod",
            r#"{
                namespace: "spacemod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            Some(r#"(register-space-profile "spacemod:abyss" 30 #f 0 #t #f "")"#),
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();
        load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());
        let index = registry
            .get(&NamespacedId::parse("spacemod:abyss").unwrap())
            .expect("脚本注册的空间层属性应当进了 Registry");
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
    fn 脚本注册的空间层属性被内容值哈希认领而不是判成无归属() {
        // 值哈希覆盖面的回归：`classify_index` 通过
        // `SpaceProfileTable::is_defined` 认领这条内容——若哪天有人给
        // GameplayTables 加了 space_profile 却忘了把同一张表传给
        // ContentValueTables，这条断言会变红（判成 Opaque）。
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "spacemod",
            r#"{
                namespace: "spacemod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            Some(r#"(register-space-profile "spacemod:abyss" 0 #f 0 #t #f "")"#),
        );
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();
        load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());
        let index = registry
            .get(&NamespacedId::parse("spacemod:abyss").unwrap())
            .expect("脚本注册的空间层属性应当进了 Registry");

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
                weather: &owned.weather,
                recipe: &owned.recipe,
                recipe_category: &owned.recipe_category,
                tag: &owned.tag,
            },
        );

        // Assert
        assert_eq!(kind, crate::content_hash::ContentTableKind::SpaceProfile);
    }

    #[test]
    fn 语法错误脚本归入loadscript阶段且不影响其它mod() {
        // Arrange：broken 语法错误，good 完全正常——验证阶段隔离。
        let root = tempdir();
        write_mod(
            root.path(),
            "broken",
            r#"{
                namespace: "broken",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            Some("(+ 1 2"),
        );
        write_mod(
            root.path(),
            "good",
            r#"{ namespace: "good", version: "0.1.0" }"#,
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
            r#"{
                namespace: "needs_ghost",
                version: "0.1.0",
                dependencies: ["ghost"],
            }"#,
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
            r#"{
                namespace: "needs_new_provider",
                version: "0.1.0",
                dependencies: { provider: ">=2.0" },
            }"#,
            None,
        );
        write_mod(
            root.path(),
            "provider",
            r#"{ namespace: "provider", version: "1.0.0" }"#,
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
            r#"{ namespace: "BadNamespace", version: "0.1.0" }"#,
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
            r#"{
                namespace: "sneaky",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
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
            r#"{
                namespace: "twoline",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
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
    fn 单个脚本内连续调用七种注册函数全部真实写进各自的表() {
        // 端到端验证：一个 mod 脚本在同一个文件里依次调用
        // register-terrain/register-class/register-skill/
        // register-subclass/register-quest/register-race/
        // register-animation-clip，七次调用必须落在同一个 Registry 上
        // （否则 ContentIndex 会撞车），且九张表各自都收到了正确的
        // 内容——这是 crate::active_registry 模块文档论证的那个「必须
        // 共享同一个 Registry」场景的直接回归。
        //
        // 这是「mod 脚本调得到这套 API」的真正证据——本测试走的是真实
        // 的 `.scm` 源码文本经 `ScriptEngine::load_source` 解析执行，
        // 不是在 Rust 里直接调用 `Registry::intern`/`*Table::define`。
        // 与之互补的另一半是「结构等价」测试（本体注册与 mod 注册走
        // 同一条注册路径、注册表内部不给本体开后门），分布在
        // `base_placeholder.rs`/`base_race.rs`/`base_terrain.rs`/
        // `base_space_profile.rs`/`base_clip.rs`/`class.rs`/`quest.rs`/
        // `race.rs`/`skill.rs`/`subclass.rs`/`clip.rs`（以及 `ll-world`
        // 的 `space_profile.rs`）各自的单元测试里——那批测试只证明
        // 「本体与 mod 内容在 Rust 类型层面无法区分」，不能单独证明
        // 脚本可达，两类证据合起来才是完整的「玩法层 API 完备性」验收，
        // 见本模块顶部「本体内容……不经过这条管线」一节与 ADR 0018。
        // 真实使用中的完整示例见 `mods/example_mod/gameplay.scm`/
        // `mods/example_mod/animation.scm`。
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "gameplay",
            r#"{
                namespace: "gameplay",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            Some(
                r#"
                (register-terrain "gameplay:lava_floor" #f #f 350 "")
                (register-class "gameplay:necromancer" "gameplay:necromancer_display_name" "willpower")
                (register-subclass "gameplay:shadowdancer" "gameplay:shadowdancer_display_name")
                (register-skill "gameplay:frostbolt" "" (list) 25 "mana" 12 "deal-damage" "" 15 0)
                (register-quest "gameplay:kill_goblins" (list) "kill-count" "gameplay:goblin" 3)
                (register-race "gameplay:half_elf" "gameplay:half_elf_display_name" 0 1 0 0 0 1 0 0 1 1 150)
                (register-animation-clip "gameplay:slime_squish" (list "slime_0" "slime_1") 6 #t 0)
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
        // 七类内容各自都能在对应的表里查到——不是只注册进了 Registry
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

        let clip_index = registry
            .get(&NamespacedId::parse("gameplay:slime_squish").unwrap())
            .expect("动画剪辑应已注册");
        assert!(owned.clip.get(clip_index).is_some());
    }

    #[test]
    fn reload_mod对干净的mod返回loaded() {
        // Arrange
        let root = tempdir();
        write_mod(
            root.path(),
            "examplemod",
            r#"{
                namespace: "examplemod",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            Some(r#"(register-terrain "examplemod:lava_floor" #f #t 4294967295 "")"#),
        );

        // Act
        let status = reload_mod(
            &root
                .path()
                .join("examplemod")
                .join(crate::discover::MANIFEST_FILENAME),
        );

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
            r#"{
                namespace: "broken",
                version: "0.1.0",
                entry_points: ["main.scm"],
            }"#,
            Some("(+ 1 2"),
        );

        // Act
        let status = reload_mod(
            &root
                .path()
                .join("broken")
                .join(crate::discover::MANIFEST_FILENAME),
        );

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
