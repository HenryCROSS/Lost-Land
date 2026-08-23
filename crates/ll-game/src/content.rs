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
//! 1. 注册**尚未迁进脚本的**本体内容（地形 → 空间层属性 → 占位内容
//!    → 动画剪辑 → 经验曲线 → 伤害公式 → 伤害类别）——一次直接的 Rust
//!    函数调用，见 [`ll_mod::pipeline`] 模块文档「本体内容分两半」一节。
//!    这几类彼此之间顺序不影响正确性（各自对应不同的 id 前缀，
//!    `Registry::intern` 天然隔离），固定一个顺序只是为了让日志读起来
//!    是线性的。
//! 2. 跑 [`ll_mod::pipeline::load_all`] 装载 `mods_root` 下的全部 mod
//!    ——**包括本体自己的 `mods/lostland/`**（种族已经迁进去了）。
//! 3. 跑本体内容契约解析（[`ll_mod::race::resolve_base_races`]）：按 id
//!    逐字段填充 Rust 侧留下的句柄结构体，缺任何一条就整批失败，见
//!    [`ll_mod::base_contract`] 模块文档。
//! 4. 跑一遍装载后内容校验（[`ll_mod::content_audit::audit_content`]）：
//!    跨表引用完整性 + 本体字段覆盖。前者是 [`load_content`] 返回 `Err`
//!    的第二个原因，后者不阻断启动、随 [`LoadedContent::audit`] 带出去，
//!    两者严重性为何不同见该模块文档。
//! 5. 跑一次 [`ll_mod::content_hash::apply_value_hashes`] 收尾。
//!
//! 第 1 步与第 2 步的先后不能颠倒：mod 内容 intern 进同一个 `Registry`，
//! 排在本体 Rust 注册之后才能保证号段不冲突。

use std::path::Path;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_mod::asset_vfs::{self, AssetVfs};
use ll_mod::base_clip::register_base_clips;
use ll_mod::base_contract::BaseContractError;
use ll_mod::base_damage_category::register_base_damage_category;
use ll_mod::base_damage_formula::register_base_damage_formula;
use ll_mod::base_placeholder::register_base_placeholder_content;
use ll_mod::base_space_profile::register_base_space_profiles;
use ll_mod::base_terrain::register_base_terrain;
use ll_mod::base_weather::register_base_weathers;
use ll_mod::base_xp_curve::register_base_xp_curve;
use ll_mod::class::{BaseClassIds, ClassTable, resolve_base_classes};
use ll_mod::clip::{BaseClipIds, ClipTable};
use ll_mod::content_audit::{
    BASE_CONTENT_AUDIT, ContentAuditReport, ReferenceIntegrityError, SubclassUnlockDeadlockError,
    audit_content,
};
use ll_mod::content_hash::{ContentValueTables, apply_value_hashes};
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::discover::discover_mods;
use ll_mod::event::EventSubscriptionTable;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
use ll_mod::item::ItemTable;
use ll_mod::load_report::{LoadReport, LoadStatus};
use ll_mod::manifest::{ModManifest, parse_manifest};
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::{BaseQuestIds, QuestError, QuestTable, RegisteredQuests, resolve_base_quests};
use ll_mod::race::{BaseRaceIds, RaceTable, resolve_base_races};
use ll_mod::recipe::{RecipeTable, RegisteredRecipes};
use ll_mod::recipe_category::RecipeCategoryTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::{BaseSkillIds, SkillError, SkillTable, resolve_base_skills};
use ll_mod::subclass::{BaseSubclassIds, SubclassTable, resolve_base_subclasses};
use ll_mod::tag::TagTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{RegistryXpCurves, XpCurveBindings, XpCurveTable};
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::exposure::AmbientSource;
use ll_world::space_profile::{BaseSpaceProfileIds, SpaceProfileTable};
use ll_world::terrain::{BaseTerrainIds, TerrainTable};
use ll_world::weather::{BaseWeatherIds, WeatherTable};

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
    /// 本体职业索引缓存（战士/法师/游侠）。
    pub class_ids: BaseClassIds,
    /// 职业表。
    pub class_table: ClassTable,
    /// 本体技能索引缓存（五条，构成一棵分支再汇聚的技能树）。
    pub skill_ids: BaseSkillIds,
    /// 技能表。
    pub skill_table: SkillTable,
    /// 本体副职索引缓存（剑舞者/学徒）。
    pub subclass_ids: BaseSubclassIds,
    /// 副职表。
    pub subclass_table: SubclassTable,
    /// 本体任务索引缓存（四条，构成一张网状任务图）。
    pub quest_ids: BaseQuestIds,
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
    /// 武器类别定义表（伤害类别/抗性接线批次新增）——
    /// `ll_mod::weapon_category::WeaponCategoryTable`，本批次没有任何
    /// `resolve` 侧消费者，见其模块文档「本批次没有给 `ItemDef` 加对应
    /// 字段」一节。
    pub weapon_category_table: WeaponCategoryTable,
    /// 本体默认伤害类别索引（`lostland:physical`，伤害类别/抗性接线
    /// 批次新增）——武器未显式声明伤害类别时的保底类别，见
    /// `ll_mod::base_damage_category` 模块文档。
    pub default_damage_category_id: ContentIndex,
    /// 伤害类别定义表。
    pub damage_category_table: DamageCategoryTable,
    /// 配方表（制作系统批次新增）——`ll_mod::recipe::RecipeTable`。
    /// 与 `recipe_category_table` 一起由
    /// [`RuntimeCatalogs::new`] 包装成 `RegisteredRecipes` 借给结算。
    pub recipe_table: RecipeTable,
    /// 配方类别表（制作系统批次新增）——副职闸门的出处，见
    /// `ll_mod::recipe_category` 模块文档。
    pub recipe_category_table: RecipeCategoryTable,

    /// 运行期事件订阅表（事件监听 API 批次新增）——`on-event` 的产物。
    ///
    /// **不是内容表**（里面没有任何 `ContentIndex`），因此不进
    /// `ContentValueTables`、不参与内容值哈希、不进存档，见
    /// `ll_mod::event` 模块文档「这不是一张内容表」一节。它是
    /// `ll_mod::script_event_source::ScriptEventSource` 的唯一输入，
    /// 由 `crate::app` 在建局时消费。
    pub event_subscriptions: EventSubscriptionTable,
    /// 标签表（耐久标签批次）——`register-tag` 的产物，物品的
    /// `wear_channels` 派生列在注册期就是查它折算出来的，见
    /// `ll_mod::tag` 模块文档。
    pub tag_table: TagTable,
    /// 本体六种天气的索引缓存（天气系统批次新增）。
    pub weather_ids: BaseWeatherIds,
    /// 天气表——`ll_world::weather::WeatherTable`。天气本身是纯派生值
    /// （不进 `WorldState`，见 `ll_world::weather` 模块文档），这张表
    /// 存的是「有哪几种天气、各自什么参数」这份**内容**，由
    /// `crate::app` 每帧调用一次 `Weather::derive` 消费。
    pub weather_table: WeatherTable,
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
    /// 已成功装载的**结算期**脚本源码：`(mod 命名空间, 源码文本)`，
    /// 来自各清单的 `event_scripts` 字段。
    ///
    /// 与 [`Self::script_sources`] 是两份互不相同的清单，不可互换：
    /// 装载期脚本里写着 `register-*`，结算期引擎上根本没有那些名字，
    /// 见 `ll_mod::manifest::ModManifest::event_scripts` 文档。这份
    /// 数据是 `ll_mod::script_event_source::ScriptEventSource::new`
    /// 的输入之一（另一个是 [`Self::event_subscriptions`]）。
    pub event_script_sources: Vec<(String, String)>,
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
    /// 本次装载的内容校验报告（装载后校验 pass 批次新增）——引用完整性
    /// 这一半已经在 [`load_content`] 里被消费掉了（不通过就根本返回不了
    /// 本结构体），这里留下的是**字段覆盖**那一半：它按
    /// `ll_mod::content_audit::ContentAuditReport::field_coverage` 文档
    /// 的裁定不阻断启动，由 `ll-game` 的门禁测试对仓库真实 `mods/`
    /// 目录断言为空。
    pub audit: ContentAuditReport,
}

/// 把一次装载会话的产出转成结算能直接消费的形状，供
/// [`RuntimeCatalogs::as_resolve_catalogs`] 借出一束
/// [`ResolveCatalogs`] 交给 [`ll_sim::turn::TurnEngine`]。
///
/// # 为什么需要这个中间类型
///
/// 各路目录里有四份不是「某张表自己就实现了 trait」：
/// [`RegisteredQuests`] 要把 [`QuestTable`] 与 [`Registry`] 绑在一起，
/// [`RegistryFormulas`] 要把 [`FormulaTable`] 与保底默认公式索引绑在
/// 一起，[`RegisteredRecipes`] 与 [`RegistryXpCurves`] 同理（各自的
/// 理由见它们自己的文档）。四者都是**借着 `LoadedContent`
/// 现造**的值，而 `ResolveCatalogs` 的字段是 `&dyn`——不能指向一个
/// 函数返回时就消失的临时值。本类型就是这四个值的落脚处：调用方先
/// 让它活着（一个局部变量），再从它借出目录束。
///
/// # 为什么目录不挂进 `WorldState`
///
/// 见 [`ll_sim::catalogs`] 模块文档「为什么不挂到 `WorldState` 上」
/// 一节：`WorldState` 是运行期状态、要进存档，内容表是装载期产物，
/// `ll_content::save_file` 刻意不序列化任何 `*Table`。本类型只是借用
/// 的搬运容器，不持有任何表的所有权，也不进任何存档。
pub struct RuntimeCatalogs<'a> {
    content: &'a LoadedContent,
    quests: RegisteredQuests<'a>,
    formulas: RegistryFormulas<'a>,
    recipes: RegisteredRecipes<'a>,
    /// 经验曲线目录（升级加点批次）——第四个「不是某张表自己实现
    /// trait」的目录：`RegistryXpCurves` 要把曲线定义表、职业/种族
    /// 绑定表与保底默认曲线索引三样绑在一起，见其文档。
    xp_curves: RegistryXpCurves<'a>,
}

impl<'a> RuntimeCatalogs<'a> {
    /// 从一次装载会话的产出借出全部结算目录。
    pub fn new(content: &'a LoadedContent) -> RuntimeCatalogs<'a> {
        RuntimeCatalogs {
            content,
            quests: RegisteredQuests {
                table: &content.quest_table,
                registry: &content.registry,
            },
            formulas: RegistryFormulas {
                formulas: &content.formula_table,
                default_formula: content.default_damage_formula_id,
            },
            // 配方这一路（制作系统批次）：`RecipeCatalog` 要回答的两个
            // 问题分别落在两张表上（配方本体表与配方类别表），因此与
            // `RegisteredQuests` 同一种情形——需要一个把两者绑在一起的
            // 轻量借用类型，见 `ll_mod::recipe::RegisteredRecipes` 文档。
            recipes: RegisteredRecipes {
                recipes: &content.recipe_table,
                categories: &content.recipe_category_table,
            },
            xp_curves: RegistryXpCurves {
                curves: &content.xp_curve_table,
                bindings: &content.xp_curve_bindings,
                default_curve: content.default_xp_curve_id,
            },
        }
    }

    /// 借出交给 [`ll_sim::turn::TurnEngine`] 的目录束。
    ///
    /// # 伤害类别这一路为什么仍是空实现
    ///
    /// [`ll_sim::damage_category::DamageCategoryCatalog`] 目前在
    /// `ll-mod` 侧**还没有任何真实实现**（仓库里唯一的实现是 `ll-sim`
    /// 自己的 `NoDamageCategories`），`LoadedContent::damage_category_table`
    /// 与 `default_damage_category_id` 还没有对应的目录类型可以包装。
    /// 这一路只影响「武器没有显式声明伤害类别时退回哪个默认类别」，
    /// **不影响抗性生不生效**（防御方天赋声明了 `RuleModifier::Resistance`
    /// 就会命中，见
    /// `ll_sim::resolve::resolve_with_skills_traits_pools_items_formulas_and_damage_categories`
    /// 文档「本函数不改变抗性本身生不生效」一节）。等 `ll-mod` 侧补上
    /// 那个实现，只需要改本方法这一行。
    pub fn as_resolve_catalogs(&self) -> ResolveCatalogs<'_> {
        ResolveCatalogs {
            skills: &self.content.skill_table,
            quests: &self.quests,
            race_traits: &self.content.race_table,
            class_traits: &self.content.class_table,
            trait_defs: &self.content.trait_table,
            pools: &self.content.resource_pool_table,
            items: &self.content.item_table,
            formulas: &self.formulas,
            damage_categories: &NO_DAMAGE_CATEGORIES,
            recipes: &self.recipes,
            // 温度这一路（温度系统批次）：把装载好的空间层属性表与天气
            // 表借进来，`ll_sim::exposure::AmbientSource` 随后在每次结算
            // 里按**当时**的 `world.clock` 现派生天气、按行动者所在空间
            // 查出环境温度（见其 `temperature_in` 文档「天气在这里现
            // 派生」一节）。这一行是保暖系统在真实游戏里唯一的接线点：
            // `ll-game` 全程只经 `TurnEngine` 驱动世界，本方法就是那条
            // 链路的入口。
            ambient: AmbientSource::new(&self.content.space_table, &self.content.weather_table),
            // 击杀经验这一路（升级加点批次）：`RaceTable` 本就登记了
            // 每个种族的 `xp_reward` 基准值，直接充当经验目录，见
            // `ll_mod::xp_curve` 里那条 `impl ExperienceCatalog for
            // RaceTable`。这一行与下面两行合起来是「打怪升级」在真实
            // 游戏里唯一的接线点：`ll-game` 全程只经 `TurnEngine`
            // 驱动世界。
            experience: &self.content.race_table,
            // 技能树这一路：与 `skills` 是同一张 `SkillTable` 的另一个
            // 视角（前置关系 + 全部已注册技能），见
            // `ll_sim::catalogs::ResolveCatalogs::skill_tree` 文档。
            skill_tree: &self.content.skill_table,
            // 经验曲线这一路：由 `apply` 侧的升级循环消费，不是
            // `resolve`——理由见该字段文档「为什么一个 `apply` 侧的
            // 目录也在这一束里」一节。
            xp_curves: &self.xp_curves,
            // 副职获得条件这一路（副职获得机制批次）：`SubclassTable`
            // 本就按副职索引登记了 `register-subclass-unlock` 声明的
            // 获得条件，直接充当目录，见 `ll_mod::subclass` 里那条
            // `impl SubclassUnlockCatalog for SubclassTable`。
            //
            // **这一行是「制作能长出副职」在真实游戏里唯一的接线点**：
            // `ll-game` 全程只经 `TurnEngine` 驱动世界，漏掉这一行，
            // `Agent::subclasses` 就会退回本批次之前那个「有闸门、没
            // 钥匙」的状态，而全部证据仍能在集成测试里成立——与击杀
            // 经验那次「只在测试里成立的接线」是同一类缺陷。
            subclass_unlocks: &self.content.subclass_table,
        }
    }
}

/// [`RuntimeCatalogs::as_resolve_catalogs`] 借出的伤害类别空实现实例，
/// 理由见该方法文档「伤害类别这一路为什么仍是空实现」一节。
const NO_DAMAGE_CATEGORIES: NoDamageCategories = NoDamageCategories;

/// [`load_content`] 会失败的全部原因。
///
/// # 为什么是一个枚举而不是继续只返回 `BaseContractError`
///
/// 本体内容迁进脚本之后，「装载出来的这批内容自洽不自洽」不止一条
/// 判据。第一条是契约解析（Rust 点名引用的那几条本体内容在不在，
/// `ll_mod::base_contract`）；装载后校验 pass 批次补上了第二条：跨表
/// 引用完整性（任何内容的 `ContentIndex` 字段都得指向一条真的被定义过
/// 的条目，`ll_mod::content_audit`）。两者是**不同的失败**，读者要做的
/// 事也不同——前者指向"本体内容目录不完整"，后者指向"某个 mod（也可能
/// 是本体自己）写了一条指向不存在内容的引用"。合并成一个字符串会把这
/// 个区别抹掉，因此按失败原因分成各自独立的变体，各自的 `Display`
/// 完整保留。第三个变体（副职获得条件死锁）同理：它指向的是"内容里
/// 有一个副职被自己的获得条件锁死了"，读者要改的是 `.scm` 里那两行
/// 声明，与前两者都不是一回事。
///
/// 字段覆盖那一半**不在**这里：它按
/// [`ll_mod::content_audit::ContentAuditReport::field_coverage`] 文档的
/// 裁定不阻断启动，产物挂在 [`LoadedContent::audit`] 上。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentLoadError {
    /// 本体内容契约没解析成功——Rust 侧句柄结构体点名要的本体内容
    /// 缺了至少一条，见 `ll_mod::base_contract` 模块文档。
    BaseContract(BaseContractError),
    /// 跨表引用完整性校验失败——至少一处 `ContentIndex` 字段指向了
    /// 不存在的内容，见 `ll_mod::content_audit` 模块文档。
    ReferenceIntegrity(ReferenceIntegrityError),
    /// 技能前置关系成环，或某条前置指向一个谁都没注册过的技能。
    ///
    /// # 这条检查此前是一条死代码
    ///
    /// `ll_mod::skill::validate_no_cycles` 唯一的调用点在
    /// `materialize_base_skills` 内部，而那个函数**从来不在生产装载
    /// 路径上**（见 `ll_mod::class` 模块文档同名一节）——也就是说
    /// **mod 注册的技能一次都没有被环检查覆盖过**。本体技能迁进脚本
    /// 的批次顺手把它接到了真正的装载管线上：它是「整张表」的性质
    /// （一个 mod 完全可以把自己的技能挂在本体技能之后再在自己那一侧
    /// 成环），因此必须在全部 mod 装载完毕之后跑，不能塞进本体契约
    /// 解析里。
    ///
    /// `involved` 是 `error` 牵涉到的那几个索引在本次装载的注册表里
    /// 反查回来的 id——`SkillError` 自己打不出这些字符串（它不持有
    /// `Registry`，见 `ll_mod::skill::SkillError::involved_indices`
    /// 文档），而「技能索引 32 声明的前置索引 33 未登记」对写
    /// `"yourmod:frostbolt"` 的 mod 作者近乎无用。反查不到的索引
    /// 不进这份列表（理论上不可达：能出现在错误里就说明它被 intern
    /// 过）。
    SkillGraph {
        /// 原始错误，保留类型以便调用方按变体分流。
        error: SkillError,
        /// 牵涉到的内容 id，顺序同 `error.involved_indices()`。
        involved: Vec<NamespacedId>,
    },
    /// 任务前置关系成环，或某条前置指向一个谁都没注册过的任务节点。
    /// 理由与 [`ContentLoadError::SkillGraph`] 逐字相同。
    QuestGraph {
        /// 原始错误。
        error: QuestError,
        /// 牵涉到的内容 id。
        involved: Vec<NamespacedId>,
    },
    /// 副职获得条件可达性校验失败——至少一个副职被自己的获得条件与
    /// 配方类别的副职闸门锁死，永远拿不到。为什么它与引用完整性一样
    /// 阻断启动（而字段覆盖不阻断），见
    /// `ll_mod::content_audit::ContentAuditReport::subclass_unlock_reachability`
    /// 文档。
    SubclassUnlockDeadlock(SubclassUnlockDeadlockError),
}

/// 把一条图校验错误牵涉到的索引反查成 id——[`ContentLoadError`] 两个
/// 图校验变体共用，见其 `involved` 字段文档。
fn resolve_involved(registry: &Registry, indices: &[ContentIndex]) -> Vec<NamespacedId> {
    indices
        .iter()
        .filter_map(|index| registry.resolve(*index).cloned())
        .collect()
}

/// 把 `involved` 渲染成错误文案里的那一段，空列表时返回空串。
fn involved_suffix(involved: &[NamespacedId]) -> String {
    if involved.is_empty() {
        return String::new();
    }
    let ids: Vec<String> = involved.iter().map(ToString::to_string).collect();
    format!("（涉及：{}）", ids.join("、"))
}

impl From<BaseContractError> for ContentLoadError {
    fn from(error: BaseContractError) -> Self {
        ContentLoadError::BaseContract(error)
    }
}

impl From<ReferenceIntegrityError> for ContentLoadError {
    fn from(error: ReferenceIntegrityError) -> Self {
        ContentLoadError::ReferenceIntegrity(error)
    }
}

impl From<SubclassUnlockDeadlockError> for ContentLoadError {
    fn from(error: SubclassUnlockDeadlockError) -> Self {
        ContentLoadError::SubclassUnlockDeadlock(error)
    }
}

impl std::fmt::Display for ContentLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentLoadError::BaseContract(error) => write!(f, "{error}"),
            ContentLoadError::ReferenceIntegrity(error) => write!(f, "{error}"),
            ContentLoadError::SkillGraph { error, involved } => {
                write!(
                    f,
                    "技能前置关系校验失败：{error}{}",
                    involved_suffix(involved)
                )
            }
            ContentLoadError::QuestGraph { error, involved } => {
                write!(
                    f,
                    "任务前置关系校验失败：{error}{}",
                    involved_suffix(involved)
                )
            }
            ContentLoadError::SubclassUnlockDeadlock(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ContentLoadError {}

/// 装载全部游戏内容：先注册尚未迁进脚本的本体内容，再装载 `mods_root`
/// 下的全部 mod（含本体自己的 `mods/lostland/`），跑一次本体内容契约
/// 解析，最后解析 `assets_root` 下本体与全部 mod 的资产 VFS。
///
/// # 返回 `Err` 的三个原因
///
/// 见 [`ContentLoadError`]：本体内容契约没解析成功、跨表引用完整性
/// 校验没通过，或副职获得条件被配方类别的闸门锁死。下面这一段讲的是
/// 第一个。
///
/// ## 本体内容契约没解析成功
///
/// 本体内容（当前是三个种族）已经搬进 `mods/lostland/*.scm`，Rust 侧
/// 只留下 `ll_mod::race::BaseRaceIds` 这类句柄结构体。句柄的填充要靠
/// 「装载完毕后按 id 去注册表里查」，这一步是**真的可能失败**的：玩家
/// 误删/改名了 `mods/lostland/`、脚本语法出错、内容改了 id。本函数
/// 因此返回 `Result` 而不是像其余 `register_base_*` 那样 `expect`
/// ——那几个 `expect` 的理由（"字面量写死在 Rust 里，不可能缺"）对
/// 迁走的内容不再成立。调用方（`crate::run_game`）负责把这条失败
/// 响亮地报给玩家，见 `ll_mod::base_contract` 模块文档。
///
/// `assets_root` 是本体自己的 `assets/` 目录（内含
/// `sprites/manifest.json5`），与 `mods_root` 是两个独立的目录树——
/// 本体资产不属于任何一个 mod 目录，见 [`ll_mod::asset_vfs`] 模块
/// 文档「为什么本体资产也要走这条路径」一节。
///
/// **本体二进制应当只调用本函数一次**（启动时）——这是本模块存在的
/// 唯一理由，见模块文档。
pub fn load_content(
    mods_root: &Path,
    assets_root: &Path,
) -> Result<LoadedContent, ContentLoadError> {
    let mut registry = Registry::new();

    let (terrain_ids, mut terrain_table) =
        register_base_terrain(&mut registry).expect("本体地形声明表内部一致，注册恒不失败");
    let (space_ids, mut space_table) = register_base_space_profiles(&mut registry)
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
    let (default_damage_category_id, mut damage_category_table) =
        register_base_damage_category(&mut |id| registry.intern(id))
            .expect("本体默认伤害类别声明内部一致，注册恒不失败");
    let (weather_ids, mut weather_table) =
        register_base_weathers(&mut registry).expect("本体天气声明表内部一致，注册恒不失败");

    let mut race_table = RaceTable::new();
    let mut class_table = ClassTable::new();
    let mut skill_table = SkillTable::new();
    let mut subclass_table = SubclassTable::new();
    let mut quest_table = QuestTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut trait_table = TraitTable::new();
    let mut resource_pool_table = ResourcePoolTable::new();
    let mut item_table = ItemTable::new();
    let mut weapon_category_table = WeaponCategoryTable::new();
    let mut recipe_table = RecipeTable::new();
    let mut recipe_category_table = RecipeCategoryTable::new();
    let mut tag_table = TagTable::new();
    let mut event_subscriptions = EventSubscriptionTable::new();

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
            weapon_category: &mut weapon_category_table,
            damage_category: &mut damage_category_table,
            space_profile: &mut space_table,
            weather: &mut weather_table,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            tag: &mut tag_table,
            events: &mut event_subscriptions,
        },
    );

    // 本体内容契约解析：`mods/lostland/` 里的本体内容此刻应当已经
    // 装载完毕（它与任何第三方 mod 走同一条 `load_all` 路径）。这一步
    // 按 id 逐字段填充 Rust 侧留下的句柄结构体，**缺任何一条就整批
    // 失败**——见 `ll_mod::base_contract` 模块文档：内容定义搬进脚本
    // 之后，这是把「Rust 代码引用的本体内容此刻在不在」重新变成一条
    // 会失败的检查的唯一手段，也是「玩家误删 mods/lostland/ 会响亮
    // 报错、而不是进到一个残破的游戏里」这条保证的落点。
    //
    // 必须排在 `load_all` 之后（脚本还没跑，内容当然不在）、排在
    // `apply_value_hashes` 之前（契约都不成立就没必要再算哈希）。
    let race_ids = resolve_base_races(&registry, &race_table)?;
    let class_ids = resolve_base_classes(&registry, &class_table)?;
    let skill_ids = resolve_base_skills(&registry, &skill_table)?;
    let subclass_ids = resolve_base_subclasses(&registry, &subclass_table)?;
    let quest_ids = resolve_base_quests(&registry, &quest_table)?;

    // 前置关系图校验：技能树与任务图都不许成环，每条前置都得指向一条
    // 真的被定义过的条目。
    //
    // **必须在这里跑，不能塞进上面那四条契约解析里**：成环是「整张
    // 表」的性质，不是「本体那几条」的性质——一个 mod 完全可以把自己的
    // 技能挂在本体技能之后、再在自己那一侧成环。因此它排在全部 mod
    // 装载完毕之后，看的是本体 + 全部 mod 的合并结果。
    //
    // 这两行接线是本批次补的：两个 `validate_no_cycles` 此前唯一的
    // 调用点分别在 `materialize_base_skills`/`materialize_base_quests`
    // 内部，而那两个函数从来不在生产装载路径上——于是 mod 注册的技能
    // 与任务**一次都没有被环检查覆盖过**（ADR 0017「注册期完整校验」
    // 在这两张表上事实落空）。见 `ContentLoadError::SkillGraph` 文档。
    if let Err(error) = ll_mod::skill::validate_no_cycles(&skill_table) {
        let involved = resolve_involved(&registry, &error.involved_indices());
        return Err(ContentLoadError::SkillGraph { error, involved });
    }
    if let Err(error) = ll_mod::quest::validate_no_cycles(&quest_table) {
        let involved = resolve_involved(&registry, &error.involved_indices());
        return Err(ContentLoadError::QuestGraph { error, involved });
    }

    // 全部内容表的只读引用束——装载后校验（本处）与值哈希（下面）共用
    // 同一份，不各建一份：两处若各写一份字段清单，`ContentValueTables`
    // 新增字段时只改一处就会静默漂移，与 `ll_mod::content_hash` 模块
    // 文档记录过的那类漂移同源。
    let value_tables = ContentValueTables {
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
        weapon_category: &weapon_category_table,
        damage_category: &damage_category_table,
        weather: &weather_table,
        recipe: &recipe_table,
        recipe_category: &recipe_category_table,
        tag: &tag_table,
    };

    // 装载后校验 pass（`ll_mod::content_audit`）：契约解析只看"Rust 点名
    // 要的那几条内容在不在"，看不见内容自身的形状。这一步把跨表引用
    // 完整性与本体字段覆盖两件事一次走完。
    //
    // 必须排在 `load_all` 与契约解析之后（内容都还没装完就查引用毫无
    // 意义），排在 `apply_value_hashes` 之前（内容都不自洽就没必要
    // 再算哈希——与契约解析排在哈希之前是同一条理由）。
    //
    // 两半的严重性不同，见 `ContentLoadError` 与
    // `ll_mod::content_audit::ContentAuditReport::field_coverage` 文档：
    // 引用完整性在这里直接 `?` 掉（一条指向不存在内容的引用是真的会在
    // 运行期表现成损坏），字段覆盖只随 `LoadedContent` 带出去。
    let audit = audit_content(&registry, &value_tables, &BASE_CONTENT_AUDIT);
    audit.reference_integrity()?;
    // 与上一行同一档严重性（都是内容自身的错误，都对全部已装载内容
    // 一视同仁）——理由见
    // `ll_mod::content_audit::ContentAuditReport::subclass_unlock_reachability`
    // 文档「为什么归在②」一节。
    audit.subclass_unlock_reachability()?;

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
    apply_value_hashes(&mut registry, &value_tables);

    let manifests = successfully_parsed_manifests(mods_root);
    let script_sources = read_script_sources(&manifests);
    let event_script_sources = read_event_script_sources(&manifests);

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

    Ok(LoadedContent {
        registry,
        terrain_ids,
        terrain_table,
        race_ids,
        race_table,
        space_ids,
        space_table,
        class_ids,
        class_table,
        skill_ids,
        skill_table,
        subclass_ids,
        subclass_table,
        quest_ids,
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
        weapon_category_table,
        default_damage_category_id,
        damage_category_table,
        recipe_table,
        recipe_category_table,
        tag_table,
        event_subscriptions,
        weather_ids,
        weather_table,
        manifests,
        script_sources,
        event_script_sources,
        report,
        asset_vfs: asset_result.vfs,
        audit,
    })
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
    read_sources(manifests, |manifest| &manifest.entry_points)
}

/// 读出每个清单全部**结算期**脚本（`event_scripts`）的源码文本，供
/// `ll_mod::script_event_source::ScriptEventSource` 使用。
///
/// 与 [`read_script_sources`] 是同一段逻辑的两个字段——它们读的是两份
/// 互不相同、也不可互换的清单，见 `ll_mod::manifest::ModManifest::event_scripts`
/// 文档「为什么必须是另一份清单」一节。
fn read_event_script_sources(manifests: &[ModManifest]) -> Vec<(String, String)> {
    read_sources(manifests, |manifest| &manifest.event_scripts)
}

/// [`read_script_sources`]/[`read_event_script_sources`] 共用的读取
/// 循环——两者只差「读哪个字段」，抽出来避免两份逐字相同的代码在改
/// 一处降级策略时漂移。
fn read_sources(
    manifests: &[ModManifest],
    pick: impl Fn(&ModManifest) -> &Vec<std::path::PathBuf>,
) -> Vec<(String, String)> {
    manifests
        .iter()
        .flat_map(|manifest| {
            let namespace = manifest.id.namespace().to_string();
            pick(manifest).iter().filter_map(move |entry| {
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
    use ll_core::ident::NamespacedId;
    use std::path::PathBuf;

    /// 仓库真实的 `assets/` 目录——`ll-game` 到仓库根固定隔两级
    /// `../..`，与既有的「真实 mods/ 目录」测试同一套推导。
    fn repo_assets_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    /// 仓库真实的 `mods/` 目录——本体内容（`mods/lostland/`）住在这里，
    /// 见 `crate::test_support::repo_mods_dir` 文档。
    fn repo_mods_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods")
    }

    #[test]
    fn 空目录下装载因本体内容契约解析失败而整批失败() {
        // 这条测试的旧版本断言的是「空目录下装载只产出本体内容、不报
        // 任何 mod 失败」——本体内容当时硬编码在 Rust 里，空 mods 目录
        // 因此是一个完全合法的状态。本体内容迁进 `mods/lostland/` 之后
        // 这个前提不再成立：空目录意味着本体内容根本不在场，继续产出
        // 一个「装载成功」的 LoadedContent 才是错的。
        //
        // 这正是玩家误删 mods/lostland/ 那一幕的自动化证据：响亮失败，
        // 不是静默进到一个建不出角色的残破游戏里。
        // Arrange：一个存在但不含任何 mod 子目录的空目录。
        let dir = crate::test_support::unique_temp_path("ll-game-content-test-empty");
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");

        // Act
        let result = load_content(&dir, &dir.join("assets"));

        // Assert：三条本体种族一条都不在，且错误里逐条点名。
        // 用 let-else 而不是 `expect_err`：`LoadedContent` 刻意不实现
        // `Debug`（它装着整个注册表与十几张内容表，打印出来毫无用处）。
        let Err(error) = result else {
            panic!("本体内容不在场时装载必须失败");
        };
        // 逐变体断言而不是只看"失败了"：装载后校验 pass 批次给
        // `load_content` 加了第二种失败原因（跨表引用完整性），空目录
        // 这一幕必须仍然是**契约解析**失败——否则说明失败被别的检查
        // 抢先报了，这条测试就不再守着它本来要守的那件事。
        let ContentLoadError::BaseContract(error) = &error else {
            panic!("空目录下必须是本体内容契约解析失败，实际是：{error}");
        };
        assert_eq!(error.required, 3);
        assert_eq!(error.missing.len(), 3);
        let text = error.to_string();
        assert!(text.contains("lostland:human"), "{text}");
        assert!(text.contains("lostland:dwarf"), "{text}");
        assert!(text.contains("lostland:elf"), "{text}");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 本体内容mod在真实装载里被成功加载() {
        // 旧版「空目录下装载只产出本体内容不报任何 mod 失败」那一半仍
        // 然值得守，只是标的换了：本体内容现在**也是**一个 mod，它必须
        // 出现在装载报告里且状态是 Loaded。
        //
        // 不断言 `failed_count() == 0`：仓库真实的 mods/ 目录里刻意
        // 放着 broken_syntax/broken_whitelist 两个"故意坏掉"的夹具
        // （管线容错测试的证据），它们失败是预期行为。这里逐条点名
        // 断言"本体与 example_mod 都成功"，比一个会被无关夹具带偏的
        // 计数更准确。
        // Arrange & Act
        let loaded = load_content(&repo_mods_dir(), &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");

        // Assert
        let status_of = |namespace: &str| {
            loaded
                .report
                .entries
                .iter()
                .find(|(id, _)| id.namespace() == namespace)
                .map(|(_, status)| status.clone())
        };
        assert_eq!(
            status_of("lostland"),
            Some(LoadStatus::Loaded),
            "本体内容 mod（mods/lostland/）必须成功加载"
        );
        assert_eq!(status_of("examplemod"), Some(LoadStatus::Loaded));
        assert!(!loaded.manifests.is_empty());
    }

    #[test]
    fn 本体地形种族空间层属性全部注册进同一个registry() {
        // 「本体即 Mod」的端到端断言：四类本体内容确实都落进了同一份
        // Registry，而不是各自只在自己的表里自说自话——用每个命名空间
        // 都能查到内容哈希来验证。
        // Arrange
        // Act
        let loaded = load_content(&repo_mods_dir(), &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");

        // Assert
        assert!(loaded.registry.content_hash_of("lostland").is_some());
        assert!(
            loaded
                .registry
                .resolve(loaded.terrain_ids.grass.index())
                .is_some()
        );
        // 种族这一路现在是「脚本注册 + 契约解析」的产物，不再是 Rust
        // 直接注册——能反查回字符串，说明契约填的是真索引不是占位值。
        assert_eq!(
            loaded
                .registry
                .resolve(loaded.race_ids.human)
                .map(|id| id.to_string()),
            Some("lostland:human".to_string())
        );
    }

    #[test]
    fn 交给回合引擎的目录束真的携带装载出来的内容表() {
        // 接线守卫（本体二进制这一侧）：`Demo::advance` 每帧把
        // `RuntimeCatalogs::as_resolve_catalogs` 的产物交给
        // `ll_sim::turn::TurnEngine`,天赋能不能在真实游戏里生效完全
        // 取决于这一束里装的是真表还是空实现。谁把它换成
        // `ResolveCatalogs::empty()`（或漏填其中一路），本条立刻变红。
        //
        // 结算侧的端到端证据（真实天赋经由 `TurnEngine` 改变结算结果）
        // 在 `crates/ll-mod/tests/turn_engine_catalogs.rs`——那里能直接
        // 拿到 `ll-mod` 的表；本条守的是本体二进制这一侧「有没有把真表
        // 交出去」，两者合起来才是完整的一条链。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
        let loaded = load_content(&mods_root, &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
        let index = |id: &str| {
            loaded
                .registry
                .get(&NamespacedId::parse(id).expect("合法标识符"))
                .unwrap_or_else(|| panic!("{id} 应当已被 mods/example_mod/gameplay.scm 注册"))
        };

        // Act
        let runtime = RuntimeCatalogs::new(&loaded);
        let catalogs = runtime.as_resolve_catalogs();

        // Assert：逐路验收——每一路都用一条真实注册的内容确认它不是空
        // 实现（空实现对任何索引恒返回 `None`/空列表）。
        assert!(
            !catalogs
                .race_traits
                .granted_traits(index("examplemod:ooze"))
                .is_empty(),
            "种族天赋来源必须是真实 RaceTable"
        );
        assert!(
            !catalogs
                .class_traits
                .granted_traits(index("examplemod:rogue"))
                .is_empty(),
            "职业天赋来源必须是真实 ClassTable"
        );
        assert!(
            catalogs
                .trait_defs
                .trait_rule(index("examplemod:acid_hide"))
                .is_some(),
            "天赋目录必须是真实 TraitTable"
        );
        assert!(
            catalogs
                .skills
                .skill(index("examplemod:backstab"))
                .is_some(),
            "技能目录必须是真实 SkillTable"
        );
        assert!(
            catalogs
                .items
                .item(index("examplemod:acid_dagger"))
                .is_some(),
            "物品目录必须是真实 ItemTable"
        );
        assert!(
            catalogs
                .pools
                .resource_pool(index("examplemod:sorcery_points"))
                .is_some(),
            "资源池目录必须是真实 ResourcePoolTable"
        );
        assert!(
            !catalogs.quests.kill_count_quests().is_empty(),
            "任务目录必须是真实 RegisteredQuests"
        );
        assert_eq!(
            catalogs
                .formulas
                .formula_for(Some(index("examplemod:iron_sword_formula")))
                .id,
            index("examplemod:iron_sword_formula"),
            "公式目录必须是真实 RegistryFormulas"
        );
    }

    #[test]
    fn 真实mods目录装载后清单非空() {
        // 端到端断言：装载仓库真实的 mods/ 目录（p4_acceptance 已验证
        // 过这个目录能成功装载），manifests 字段确实收集到了内容,
        // 不是恒为空的死字段。
        // Arrange
        let mods_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");

        // Act
        let loaded = load_content(&mods_root, &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");

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
        let loaded = load_content(&mods_root, &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");

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
        let loaded = load_content(&mods_root, &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");

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
        let loaded = load_content(&mods_root, &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");

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
        let loaded = load_content(&mods_root, &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");

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
        let loaded = load_content(&mods_root, &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
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
            weapon_category: &loaded.weapon_category_table,
            damage_category: &loaded.damage_category_table,
            weather: &loaded.weather_table,
            recipe: &loaded.recipe_table,
            recipe_category: &loaded.recipe_category_table,
            tag: &loaded.tag_table,
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

        // Assert：两条已知例外，不多不少。
        //
        // - `lostland:placeholder_race`：「种族未知/缺失」这个降级状态的
        //   占位索引，刻意不定义任何 `RaceDef`，见
        //   `ll_mod::base_placeholder` 模块文档。
        // - `lostland:goblin`：`mods/lostland/quests.scm` 里三条
        //   `kill-count` 任务的 `target_kind`。它指向「敌人类型」，而
        //   代码库至今没有敌人类型注册表——这正是
        //   `ll_mod::content_audit::ReferenceExpectation::UntypedIdSpace`
        //   那条豁免说的情形（把它按「必须在某张内容表里已定义」检查
        //   会把一条正确的设计判成错误），见 `ll_mod::quest` 模块文档
        //   「跨表引用」一节。本体任务迁进脚本的批次之前，这三条任务
        //   根本不在生产装载路径上，所以这个 id 此前也不在注册表里。
        assert_eq!(
            opaque_ids,
            vec![
                "lostland:goblin".to_string(),
                "lostland:placeholder_race".to_string(),
            ]
        );
    }
    #[test]
    fn 真实内容的跨表引用完整性通过且不是空转() {
        // 装载后校验 pass 批次的门禁之一（引用完整性）。装载能返回
        // `Ok` 本身已经蕴含"零违规"（`load_content` 直接 `?` 掉了它），
        // 本条真正守的是**非空转**：一处引用都没检查到的校验会恒为绿、
        // 而且绿得完全无声。谁把某个 `inspect_*` 里的 `reference` 调用
        // 删掉、或把整张表的分派接错，`references_checked` 会掉下来，
        // 本条变红。
        // Arrange & Act
        let loaded = load_content(&repo_mods_dir(), &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下内容校验必须通过");

        // Assert
        assert_eq!(loaded.audit.reference_violations, Vec::new());
        assert!(
            loaded.audit.references_checked >= 1,
            "仓库真实内容里至少有一处跨表引用（mods/example_mod/gameplay.scm \
             的 register-item-damage-formula 等），一处都没检查到说明校验空转了"
        );
    }

    #[test]
    fn 真实内容的副职获得条件可达且不是空转() {
        // 装载后校验 pass 的第三条硬失败（副职获得条件死锁）。装载能
        // 返回 `Ok` 本身已经蕴含"零死锁"（`load_content` 直接 `?` 掉了
        // 它），本条真正守的是**非空转**：`detect_unlock_deadlocks` 若
        // 一条获得条件都没拿到（`craft_unlocks()` 接错、副职表从
        // `ContentValueTables` 上被摘掉），报告恒为"零死锁"且完全无声。
        //
        // 仓库真实内容确实喂得出量：`mods/lostland/subclasses.scm` 的
        // 四个本体副职各有一条制作获得条件，`mods/example_mod/gameplay.scm`
        // 的 shadowdancer 还有一条——而且 example_mod 那条正是「从不设闸
        // 的类别（烹饪）练出副职，用它去开设了闸的类别（锻造）的门」这个
        // 正确形状，本条同时也是"正确形状不被误报"在真实内容上的验收。
        // Arrange & Act
        let loaded = load_content(&repo_mods_dir(), &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下内容校验必须通过");

        // Assert
        assert_eq!(loaded.audit.unlock_deadlocks, Vec::new());
        assert!(
            loaded.audit.unlock_rules_checked >= 1,
            "仓库真实内容里至少有一条副职获得条件（mods/lostland/subclasses.scm \
             与 mods/example_mod/gameplay.scm 都有），一条都没看到说明检查空转了"
        );
    }

    #[test]
    fn 真实本体内容的字段覆盖不留缺口() {
        // 装载后校验 pass 批次的门禁之二（字段覆盖）。它按
        // `ll_mod::content_audit::ContentAuditReport::field_coverage`
        // 文档的裁定**不阻断启动**（本体内容被玩家改过不是"游戏坏了"
        // 的信号），严重性接在这条测试上：本体内容里新增一个从没有
        // 任何内容赋过非默认值的字段，或者花名册/豁免清单自身失效
        // （某张表的内容迁进 mods/lostland/ 了却还挂在 deferred 里、
        // 某条豁免的字段其实已经被覆盖了），本条立刻变红。
        // Arrange & Act
        let loaded = load_content(&repo_mods_dir(), &repo_assets_dir())
            .expect("仓库真实 mods/ 目录下内容校验必须通过");

        // Assert
        if let Err(error) = loaded.audit.field_coverage() {
            panic!("{error}");
        }
        assert!(
            loaded.audit.fields_observed >= 1,
            "本体内容至少覆盖七张表的字段，一个字段槽都没观察到说明校验空转了"
        );
    }

    /// 递归复制一个目录树——建"本体内容 + 一个坏 mod"这类临时 mods
    /// 根目录时用。本 crate 不为此引入 `fs_extra` 依赖：只有测试需要，
    /// 需求简单到手写几行就够（与 `ll_mod::test_support::tempdir` 同一
    /// 条既有取舍）。
    fn copy_dir_all(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).expect("创建目标目录应当成功");
        for entry in std::fs::read_dir(src).expect("读取源目录应当成功") {
            let entry = entry.expect("读取目录项应当成功");
            let target = dst.join(entry.file_name());
            if entry.file_type().expect("读取文件类型应当成功").is_dir() {
                copy_dir_all(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("复制文件应当成功");
            }
        }
    }

    #[test]
    fn mod把物品公式指向一个只被intern过的id时装载整批失败() {
        // 引用完整性这一层堵的**具体**那个洞：脚本注册 API
        // （`register-item-damage-formula`）对目标 id 只做
        // `Registry::get`（"这个字符串被 intern 过吗"），不做"伤害公式
        // 表真的定义过它吗"——这正是 `ll_mod::base_contract` 模块文档
        // 「两层判定」一节说的 `NotInterned` 与 `NotDefined` 的区别。
        // 一个 mod 只要先用别的方式把某个 id intern 出来（这里用
        // register-quest 的击杀目标，那个参数按设计只 intern 不定义），
        // 就能造出一条注册期检查放行、运行期静默失效的悬空引用。
        //
        // 本条是这条检查的端到端证据：真实脚本、真实装载管线、真实
        // 失败，不是只在单元测试里自证。
        // Arrange：临时 mods 根目录 = 本体内容（契约解析要用）+ 一个
        // 故意写坏引用的 mod。
        let root = crate::test_support::unique_temp_path("ll-game-audit-dangling-ref");
        std::fs::create_dir_all(&root).expect("创建测试目录应当成功");
        copy_dir_all(&repo_mods_dir().join("lostland"), &root.join("lostland"));
        let broken = root.join("brokenref");
        std::fs::create_dir_all(&broken).expect("创建测试目录应当成功");
        std::fs::write(
            broken.join("mod.json5"),
            "{ namespace: \"brokenref\", version: \"0.1.0\", entry_points: [\"main.scm\"] }",
        )
        .expect("写入清单应当成功");
        std::fs::write(
            broken.join("main.scm"),
            ";; brokenref:phantom 只被击杀计数条件 intern 出来，从来没有\n\
             ;; 任何 register-damage-formula 定义过它。\n\
             (register-quest \"brokenref:quest\" (list) \"kill-count\" \"brokenref:phantom\" 1)\n\
             (register-item \"brokenref:sword\" \"brokenref:sword_name\" 1 1000 1000 -1)\n\
             (register-item-damage-formula \"brokenref:sword\" \"brokenref:phantom\")\n",
        )
        .expect("写入脚本应当成功");

        // Act
        let result = load_content(&root, &repo_assets_dir());

        // Assert
        let Err(error) = result else {
            panic!("悬空引用必须让整批装载失败");
        };
        let ContentLoadError::ReferenceIntegrity(error) = &error else {
            panic!("必须是引用完整性失败，实际是：{error}");
        };
        assert_eq!(error.violations.len(), 1);
        assert_eq!(error.violations[0].field, "ItemAttrs::damage_formula");
        assert_eq!(
            error.violations[0]
                .target_id
                .as_ref()
                .map(ToString::to_string),
            Some("brokenref:phantom".to_string())
        );
        let text = error.to_string();
        assert!(text.contains("brokenref:sword"), "{text}");
        assert!(text.contains("brokenref:phantom"), "{text}");

        // Cleanup
        let _ = std::fs::remove_dir_all(&root);
    }
}
