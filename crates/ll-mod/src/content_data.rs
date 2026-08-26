//! 内容数据文件（JSON5）的装载——每个 mod 目录下那一组 `*.json5`
//! 内容名册读进既有的内容表。
//!
//! schema 与「为什么内容走数据文件」见 [`crate::content_schema`] 模块
//! 文档。本模块只管三件事：**读哪些文件、按什么顺序读、读坏了怎么报**。
//!
//! # 读哪些文件：固定文件名，不进 `mod.json5`
//!
//! 内容文件不在清单里登记，靠固定文件名被发现——与矮人要塞的 raws、
//! RimWorld 的 defs 同一个形态。两条理由：
//!
//! 1. **清单里的列表会变成一条隐形约束。** 已经拆掉的 `entry_points`
//!    就是前车之鉴：那个数组的顺序曾经是真实的依赖约束（crafting 必须
//!    排在 subclasses 前面），而清单里看不出为什么，第三方作者照抄时
//!    根本不知道有这回事。文件名固定之后，顺序由本模块的
//!    [`CONTENT_FILES`] 一处决定，mod 作者不需要、也无法把它改坏。
//! 2. **「本体的种族都写在哪」有一个不需要搜索就能回答的答案。**
//!
//! 每个文件都是**可选的**：不存在就跳过（纯行为 mod 可以一个内容文件
//! 都没有），存在就必须解析成功——一个内容文件坏了，整个 mod 装载
//! 失败并点名文件与行列，不会静默少一半内容。
//!
//! # 按什么顺序读：一张固定的表，钉住真实的依赖方向
//!
//! [`CONTENT_FILES`] 的顺序不是字母序，是**依赖序**。判据统一是
//! 「谁只 get 不 intern」：只 get 的那一方必须排在被引用者之后，否则
//! 当场报「尚未注册」。逐条理由写在那个数组自己的注释里——每一条都是
//! 一个真实的引用方向，不是「看起来该这样」。
//!
//! 有一处**环**：配方类别与副职互相引用，且两边都只 get 不 intern
//! （副职的获得条件指向一个类别，类别的闸门指向一个副职）。任何单一
//! 顺序都满足不了它，因此 `crafting.json5` 被读两遍，见
//! [`crate::content_schema::apply_recipe_category_subclass_gates`]。
//!
//! 这些约束此前写在 `mods/*/mod.json5` 的注释与各脚本文件的
//! `(require ...)` 里——由**内容作者**负责维护。现在它们是引擎侧的一条
//! 常量：mod 作者写不错，也改不动。
//!
//! # 顺序确定性（约束 C5）
//!
//! 三层顺序全部是确定的，没有一处依赖 `HashMap` 迭代顺序：
//!
//! - **mod 之间**：`crate::pipeline::load_all` 的拓扑序（`crate::topo`）。
//! - **文件之间**：[`CONTENT_FILES`] 这个固定数组。
//! - **文件之内**：每个文件的顶层是一个**数组**，JSON5 数组保序，
//!   `serde` 按书写顺序产出 `Vec`。
//!
//! 顺序影响 [`crate::registry::Registry`] 分配的 `ContentIndex` 编号，
//! 但**不影响内容值哈希**——那是异或折叠、且每个 `ContentIndex` 字段
//! 在混入之前都先解析回 id 字符串（ADR 0027，见
//! [`crate::content_hash`] 模块文档「`ContentIndex` 字段」一节）。

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::content_schema::{
    ClassFile, CraftingFile, QuestFile, RaceFile, SkillFile, SubclassFile, TagFile, apply_classes,
    apply_quests, apply_races, apply_recipe_categories, apply_recipe_category_subclass_gates,
    apply_skills, apply_subclasses, apply_tags,
};
use crate::content_schema_gear::{
    DamageCategoryFile, DamageFormulaFile, ItemFile, ModifierTypeFile, ResourcePoolFile, TraitFile,
    WeaponCategoryFile, XpCurveFile, apply_damage_categories, apply_damage_formulas, apply_items,
    apply_modifier_types, apply_recipes, apply_resource_pools, apply_traits,
    apply_weapon_categories, apply_xp_curves,
};
use crate::content_schema_world::{
    AnimationFile, ResourceFile, SpaceProfileFile, TerrainFile, WeatherFile, apply_clips,
    apply_resources, apply_space_profiles, apply_terrains, apply_weathers,
};
use crate::pipeline::GameplayTables;
use crate::registry::Registry;

/// 一个 mod 的内容数据文件没能装载。
///
/// `file` 是出问题的那个文件的完整路径，`message` 里带着 `json5` 给出的
/// 行列位置（反序列化错误）或被点名的内容 id（引用解析错误）——两者
/// 都是可行动的，见 [`crate::content_schema`] 模块文档「具名字段顺手
/// 消灭了一整类失败模式」一节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentDataError {
    /// 出问题的内容文件。
    pub file: PathBuf,
    /// 人话错误原因。
    pub message: String,
}

impl std::fmt::Display for ContentDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file.display(), self.message)
    }
}

impl std::error::Error for ContentDataError {}

/// 一类内容数据文件。
///
/// 枚举而不是「文件名 → 处理函数」的表：处理函数的**类型各不相同**
/// （每一类反序列化成不同的 `*File`），一张同构的表装不下它们，硬要
/// 装就得引入一层 trait 对象——而那正是 ADR 0021 点名要避免的「为了
/// 对称而抽象」。`match` 一次把全部列全，编译器负责不漏。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentFileKind {
    Tags,
    ModifierTypes,
    Terrains,
    Resources,
    SpaceProfiles,
    Weathers,
    AnimationClips,
    ResourcePools,
    DamageFormulas,
    DamageCategories,
    WeaponCategories,
    XpCurves,
    Crafting,
    Subclasses,
    /// `crafting.json5` 的**第二遍**：只写副职闸门，见
    /// [`crate::content_schema::apply_recipe_category_subclass_gates`]
    /// 文档「为什么要分两遍」。
    CraftingSubclassGates,
    Items,
    Races,
    Classes,
    Skills,
    Traits,
    Quests,
}

impl ContentFileKind {
    /// 这一类内容在 mod 目录下的固定文件名。
    ///
    /// [`ContentFileKind::Crafting`] 与
    /// [`ContentFileKind::CraftingSubclassGates`] **共用同一个文件名**
    /// ——那是刻意的，不是笔误，见后者的文档。
    fn file_name(self) -> &'static str {
        match self {
            ContentFileKind::Tags => "tags.json5",
            ContentFileKind::ModifierTypes => "modifier_types.json5",
            ContentFileKind::Terrains => "terrain.json5",
            ContentFileKind::Resources => "resources.json5",
            ContentFileKind::SpaceProfiles => "space_profiles.json5",
            ContentFileKind::Weathers => "weather.json5",
            ContentFileKind::AnimationClips => "animations.json5",
            ContentFileKind::ResourcePools => "resource_pools.json5",
            ContentFileKind::DamageFormulas => "damage_formulas.json5",
            ContentFileKind::DamageCategories => "damage_categories.json5",
            ContentFileKind::WeaponCategories => "weapon_categories.json5",
            ContentFileKind::XpCurves => "xp_curves.json5",
            ContentFileKind::Crafting => "crafting.json5",
            ContentFileKind::Subclasses => "subclasses.json5",
            ContentFileKind::CraftingSubclassGates => "crafting.json5",
            ContentFileKind::Items => "items.json5",
            ContentFileKind::Races => "races.json5",
            ContentFileKind::Classes => "classes.json5",
            ContentFileKind::Skills => "skills.json5",
            ContentFileKind::Traits => "traits.json5",
            ContentFileKind::Quests => "quests.json5",
        }
    }
}

/// 内容数据文件的**装载顺序**——依赖序，不是字母序，理由见模块文档
/// 「按什么顺序读」一节。
///
/// 数组长度由类型钉死：新增一类内容忘了加进来，改的是这个数字，改动
/// 会出现在 diff 里；漏了某一类的 `file_name` 分支则编译不过。
///
/// 每一条排在这个位置的理由都是一条**真实的引用方向**，逐条列在下面
/// 的注释里。判据统一是「谁只 get 不 intern」：只 get 的那一方必须
/// 排在被引用者之后。
const CONTENT_FILES: [ContentFileKind; 21] = [
    // 标签没有任何前置依赖，而物品会引用它（只 get 不 intern）。
    ContentFileKind::Tags,
    // 加值类型同样没有任何前置依赖，而天赋与物品的规则修正会引用它
    // （只 get 不 intern，见 content_schema_gear 的 required_modifier_type）。
    ContentFileKind::ModifierTypes,
    // 地形/空间层/天气/动画剪辑四类互不引用，也不被后面任何一类
    // 「只 get」地引用（配方的场地是 intern），位置本身无约束——排在
    // 前面只是因为它们是世界的底座。
    ContentFileKind::Terrains,
    // 资源引用地形（`source_terrain`），必须排在地形之后——不是因为
    // 注册表会拒绝（`intern` 对谁先提到一个 id 一视同仁），而是因为
    // 「资源长在一种从没被声明过的地形上」这条内容错误只有在地形先
    // 装载完之后才可能被后续消费发现，顺序颠倒会让症状漂到更远的地方。
    ContentFileKind::Resources,
    ContentFileKind::SpaceProfiles,
    ContentFileKind::Weathers,
    ContentFileKind::AnimationClips,
    // 资源池必须排在技能（施放代价的 pool/slot-tier）与天赋（授予的
    // 池容量）前面。
    ContentFileKind::ResourcePools,
    // 伤害公式必须排在伤害类别/武器类别（默认公式）与物品（显式公式）
    // 前面。
    ContentFileKind::DamageFormulas,
    // 伤害类别必须排在物品（伤害类别与抗性）与天赋（抗性）前面。
    ContentFileKind::DamageCategories,
    ContentFileKind::WeaponCategories,
    // 经验曲线必须排在职业与种族（各自的 xp_curve 绑定）前面。
    ContentFileKind::XpCurves,
    // 配方类别必须排在副职（获得条件的 target）与配方（所属类别）
    // 前面；配方与类别同住 crafting.json5，一遍读完。
    ContentFileKind::Crafting,
    ContentFileKind::Subclasses,
    // 副职闸门反过来要求副职已定义——环，因此 crafting.json5 读两遍。
    ContentFileKind::CraftingSubclassGates,
    // 物品必须排在配方（可教授的配方只 get 不 intern）之后。
    ContentFileKind::Items,
    // 种族必须排在物品（出生装备只 get 不 intern）之后。
    ContentFileKind::Races,
    // 职业必须排在技能前面（技能的 owning_class 虽是 intern，但装载
    // 末尾的引用完整性校验会把顺序反了判成一条违规）。
    ContentFileKind::Classes,
    ContentFileKind::Skills,
    // 天赋只被「intern」地引用（种族/职业的天赋授予），位置无约束；
    // 它自己引用的资源池与伤害类别都已经在前面。
    ContentFileKind::Traits,
    ContentFileKind::Quests,
];

/// 装载一个 mod 目录下的全部内容数据文件。
///
/// 每个文件都是可选的（不存在即跳过）；存在就必须解析成功，否则整个
/// mod 装载失败。调用点在 [`crate::pipeline::load_all`]，**排在这个
/// mod 自己的脚本编译之前**——声明先于逻辑，行为脚本因此可以引用同一
/// 个 mod 声明的内容。
pub fn load_mod_content_data(
    mod_root: &Path,
    registry: &mut Registry,
    tables: &mut GameplayTables<'_>,
) -> Result<(), ContentDataError> {
    for kind in CONTENT_FILES {
        let path = mod_root.join(kind.file_name());
        if !path.is_file() {
            continue;
        }
        apply_one(kind, &path, registry, tables)?;
    }
    Ok(())
}

/// 读一个内容文件并把它写进内容表。
fn apply_one(
    kind: ContentFileKind,
    path: &Path,
    registry: &mut Registry,
    tables: &mut GameplayTables<'_>,
) -> Result<(), ContentDataError> {
    let fail = |message: String| ContentDataError {
        file: path.to_path_buf(),
        message,
    };
    let source = std::fs::read_to_string(path).map_err(|err| fail(err.to_string()))?;

    // 反序列化与「写进表」分成两句：前者的错误带 json5 的行列位置，
    // 后者的错误带被点名的内容 id，两类信息都不该被对方的包装吃掉。
    match kind {
        ContentFileKind::Tags => {
            let file: TagFile = parse(&source).map_err(fail)?;
            apply_tags(registry, tables.tag, &file.tags)
        }
        ContentFileKind::ModifierTypes => {
            let file: ModifierTypeFile = parse(&source).map_err(fail)?;
            apply_modifier_types(registry, tables.modifier_type, &file.modifier_types)
        }
        ContentFileKind::Terrains => {
            let file: TerrainFile = parse(&source).map_err(fail)?;
            apply_terrains(registry, tables.terrain, &file.terrains)
        }
        ContentFileKind::Resources => {
            let file: ResourceFile = parse(&source).map_err(fail)?;
            apply_resources(registry, tables.resource, &file.resources)
        }
        ContentFileKind::SpaceProfiles => {
            let file: SpaceProfileFile = parse(&source).map_err(fail)?;
            apply_space_profiles(registry, tables.space_profile, &file.space_profiles)
        }
        ContentFileKind::Weathers => {
            let file: WeatherFile = parse(&source).map_err(fail)?;
            apply_weathers(registry, tables.weather, &file.weathers)
        }
        ContentFileKind::AnimationClips => {
            let file: AnimationFile = parse(&source).map_err(fail)?;
            apply_clips(registry, tables.clip, &file.clips)
        }
        ContentFileKind::ResourcePools => {
            let file: ResourcePoolFile = parse(&source).map_err(fail)?;
            apply_resource_pools(registry, tables.resource_pool, &file.resource_pools)
        }
        ContentFileKind::DamageFormulas => {
            let file: DamageFormulaFile = parse(&source).map_err(fail)?;
            apply_damage_formulas(registry, tables.formula, &file.damage_formulas)
        }
        ContentFileKind::DamageCategories => {
            let file: DamageCategoryFile = parse(&source).map_err(fail)?;
            apply_damage_categories(registry, tables.damage_category, &file.damage_categories)
        }
        ContentFileKind::WeaponCategories => {
            let file: WeaponCategoryFile = parse(&source).map_err(fail)?;
            apply_weapon_categories(registry, tables.weapon_category, &file.weapon_categories)
        }
        ContentFileKind::XpCurves => {
            let file: XpCurveFile = parse(&source).map_err(fail)?;
            apply_xp_curves(registry, tables.xp_curve, &file.xp_curves)
        }
        ContentFileKind::Crafting => {
            let file: CraftingFile = parse(&source).map_err(fail)?;
            apply_recipe_categories(registry, tables.recipe_category, &file.recipe_categories)
                .and_then(|()| apply_recipes(registry, tables.recipe, &file.recipes))
        }
        ContentFileKind::Subclasses => {
            let file: SubclassFile = parse(&source).map_err(fail)?;
            apply_subclasses(registry, tables.subclass, &file.subclasses)
        }
        ContentFileKind::CraftingSubclassGates => {
            let file: CraftingFile = parse(&source).map_err(fail)?;
            apply_recipe_category_subclass_gates(
                registry,
                tables.recipe_category,
                &file.recipe_categories,
            )
        }
        ContentFileKind::Items => {
            let file: ItemFile = parse(&source).map_err(fail)?;
            apply_items(
                registry,
                tables.item,
                tables.tag,
                tables.recipe,
                tables.modifier_type,
                &file.items,
            )
        }
        ContentFileKind::Races => {
            let file: RaceFile = parse(&source).map_err(fail)?;
            apply_races(
                registry,
                tables.race,
                tables.xp_curve,
                tables.xp_curve_bindings,
                &file.races,
            )
        }
        ContentFileKind::Classes => {
            let file: ClassFile = parse(&source).map_err(fail)?;
            apply_classes(
                registry,
                tables.class,
                tables.xp_curve,
                tables.xp_curve_bindings,
                &file.classes,
            )
        }
        ContentFileKind::Skills => {
            let file: SkillFile = parse(&source).map_err(fail)?;
            apply_skills(registry, tables.skill, &file.skills)
        }
        ContentFileKind::Traits => {
            let file: TraitFile = parse(&source).map_err(fail)?;
            apply_traits(
                registry,
                tables.trait_def,
                tables.modifier_type,
                &file.traits,
            )
        }
        ContentFileKind::Quests => {
            let file: QuestFile = parse(&source).map_err(fail)?;
            apply_quests(registry, tables.quest, &file.quests)
        }
    }
    .map_err(fail)
}

/// `json5::from_str` 的薄包装——把错误换成字符串，顺便留一处集中说明
/// 「位置信息是从哪来的」。
///
/// `json5` 1.3.1 的 `Error` 的 `Display` 在有位置时输出
/// `<原因> at line N column M`，且**serde 层的错误也带位置**（反序列化
/// 器把 `serde::de::Error::custom` 产出的错误也套上了当前偏移量）——
/// 「未知字段」「缺必填字段」两类因此都能定位到行列。这一条有实测
/// 钉子，见 [`crate::content_schema`] 的 `未知字段报错且错误信息带行列位置`。
fn parse<T: DeserializeOwned>(source: &str) -> Result<T, String> {
    json5::from_str(source).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{OwnedTables, TempDir, tempdir};
    use ll_core::ident::NamespacedId;

    /// 在一个临时目录里放一组内容文件，跑一遍装载。
    ///
    /// 临时目录随返回值一起交出去——它一析构就删目录，测试还要用
    /// `dir.path()` 核对错误里的文件名。
    fn load_from(files: &[(&str, &str)]) -> (Registry, TempDir, Result<(), ContentDataError>) {
        let dir = tempdir();
        for (name, body) in files {
            std::fs::write(dir.path().join(name), body).expect("写内容文件");
        }
        let mut registry = Registry::new();
        let mut owned = OwnedTables::default();
        let result = {
            let mut tables = owned.as_gameplay_tables();
            load_mod_content_data(dir.path(), &mut registry, &mut tables)
        };
        (registry, dir, result)
    }

    #[test]
    fn 一个内容文件都没有的mod装载成功() {
        // 纯行为 mod 是合法的——内容文件全部可选。
        // Arrange & Act
        let (_registry, _dir, result) = load_from(&[]);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn 配方类别与副职跨文件的依赖方向由固定顺序保证() {
        // 这是 CONTENT_FILES 那张表存在的理由：subclasses.json5 引用
        // crafting.json5 里的类别，而两个文件名的字母序恰好是反的
        // （crafting < subclasses 是对的，但文件表不能靠巧合）。
        // Arrange & Act
        let (registry, _dir, result) = load_from(&[
            (
                "crafting.json5",
                r#"{ recipe_categories: [ { id: "m:forging", display_name_key: "m:forging.name" } ] }"#,
            ),
            (
                "subclasses.json5",
                r#"{ subclasses: [ { id: "m:artisan", display_name_key: "m:artisan.name",
                     unlock: { kind: "items-crafted", target: "m:forging", threshold: 20 } } ] }"#,
            ),
        ]);

        // Assert
        assert_eq!(result, Ok(()));
        assert!(
            registry
                .get(&NamespacedId::parse("m:artisan").expect("合法标识符"))
                .is_some()
        );
    }

    #[test]
    fn 内容文件坏了整个mod失败并点名文件() {
        // Arrange & Act
        let (_registry, dir, result) = load_from(&[("tags.json5", "{ tags: [ { id: 3 } ] }")]);

        // Assert
        let error = result.expect_err("非法内容必须让装载失败");
        assert_eq!(error.file, dir.path().join("tags.json5"));
    }
}
