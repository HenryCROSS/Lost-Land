//! 一次内容装载会话的完整前半段：**引擎侧注册 + [`crate::pipeline::load_all`]**
//! ——`ll_game::content::load_content` 与全部集成测试共用的唯一入口。
//!
//! # 起因：一条真实的测试保真度缺口
//!
//! `crates/ll-mod/tests/` 下的 `load_real_mods()` 一族此前各自现造二十
//! 一张空表、直接调用 [`crate::pipeline::load_all`]，**跳过了生产路径
//! （`ll_game::content::load_content`）里排在 `load_all` 之前的那一批
//! 引擎侧注册**：地形（[`crate::base_terrain`]）、空间层属性
//! （[`crate::base_space_profile`]）、占位内容
//! （[`crate::base_placeholder`]）、动画剪辑（[`crate::base_clip`]）、
//! 默认经验曲线（[`crate::base_xp_curve`]）、默认伤害公式
//! （[`crate::base_damage_formula`]）、默认伤害类别
//! （[`crate::base_damage_category`]）、天气（[`crate::base_weather`]）。
//!
//! 后果不是「测试少断言了几条」，而是**测试跑的世界和真实游戏跑的不是
//! 同一个世界**：那些注册产出的内容在测试里根本不存在，于是任何内容
//! 数据文件**只要引用引擎注册的本体内容就会当场装载失败**。上一批想让
//! 本体武器引用 `lostland:default_damage_formula` 时正是撞上这一条，
//! 最后绕开了——绕开的是症状，本模块修的是病因。
//!
//! # 为什么收敛成一个类型，而不是在每处测试补三行
//!
//! 二十七处调用点各自补一遍注册序列，等于把「生产路径长什么样」这件
//! 事复制二十七份：下一次引擎侧多一条注册（本仓库已经发生过八次），
//! 二十七份里漏改哪一份都是同一个缺口原地重开，而且**测试仍然是绿
//! 的**——这正是本缺口第一次出现的方式。
//!
//! 因此注册序列只写一遍，写在这里，**生产路径也用同一份**：
//! `ll_game::content::load_content` 现在的开头就是
//! [`LoadSession::with_engine_registrations`] + [`LoadSession::load_all`]，
//! 测试与生产字面上跑的是同一段代码，不是「两段看起来一样的代码」。
//! 这与 `scripts/ci/run_all.sh` 头注释里那条纪律（不维护两份独立写出
//! 来的等价清单）是同一条判断。
//!
//! # 为什么落在 `ll-mod`，不是 `ll-game`
//!
//! 上面八条注册与 `load_all` 全都住在本 crate；`ll-game` 只是把它们
//! 按顺序调一遍。集成测试属于 `ll-mod` 自己的 `tests/`（外部 crate，
//! 用不到 `#[cfg(test)]` 的 `crate::test_support`），要共用就必须是
//! 本 crate 的公开 API。
//!
//! # 本类型不做的事
//!
//! 契约解析（[`crate::base_contract`]）、前置关系图校验、装载后校验
//! （[`crate::content_audit`]）、值哈希（[`crate::content_hash`]）、
//! 资产 VFS 全部**不在**本类型里：它们各自都可能失败、失败语义各不
//! 相同，`ll_game::content::load_content` 把它们串成一条带
//! `ContentLoadError` 的返回链，那是 `ll-game` 的职责。本类型只负责
//! 「把内容装进表里」这一段——它恰好也正是测试真正共用的那一段。

use std::path::Path;

use ll_core::ident::ContentIndex;
use ll_world::culture::CultureTable;
use ll_world::resource::ResourceTable;
use ll_world::space_profile::{BaseSpaceProfileIds, SpaceProfileTable};
use ll_world::terrain::{BaseTerrainIds, TerrainTable};
use ll_world::weather::{BaseWeatherIds, WeatherTable};

use crate::base_clip::register_base_clips;
use crate::base_damage_category::register_base_damage_category;
use crate::base_damage_formula::register_base_damage_formula;
use crate::base_placeholder::register_base_placeholder_content;
use crate::base_space_profile::register_base_space_profiles;
use crate::base_terrain::register_base_terrain;
use crate::base_weather::register_base_weathers;
use crate::base_xp_curve::register_base_xp_curve;
use crate::behavior_binding::ClassBehaviorBindings;
use crate::class::ClassTable;
use crate::clip::{BaseClipIds, ClipTable};
use crate::damage_category::DamageCategoryTable;
use crate::formula::FormulaTable;
use crate::item::ItemTable;
use crate::load_report::LoadReport;
use crate::modifier_type::ModifierTypeTable;
use crate::pipeline::{GameplayTables, load_all};
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

/// 一次装载会话持有的全部东西：注册表、引擎侧注册产出的那几组句柄/
/// 默认索引，以及 [`GameplayTables`] 要求的全部内容表。
///
/// 字段全部 `pub`：调用方（`ll_game::content::load_content` 与各集成
/// 测试）装载完之后要把这些表**搬走**（`let LoadSession { registry,
/// race, .. } = session;`），不是隔着方法读几个值——本类型是一次会话的
/// 落脚处，不是一层封装。
pub struct LoadSession {
    /// 内容注册表——引擎注册与全部 mod 内容共用同一段单调递增的
    /// [`ContentIndex`] 号段，见 [`crate::base_terrain`] 模块文档。
    pub registry: Registry,

    /// 本体地形句柄（[`register_base_terrain`] 的产出）。
    pub terrain_ids: BaseTerrainIds,
    /// 本体空间层属性句柄（[`register_base_space_profiles`] 的产出）。
    pub space_ids: BaseSpaceProfileIds,
    /// 本体动画剪辑句柄（[`register_base_clips`] 的产出）。
    pub clip_ids: BaseClipIds,
    /// 本体天气句柄（[`register_base_weathers`] 的产出）。
    pub weather_ids: BaseWeatherIds,
    /// 本体默认经验曲线索引（[`register_base_xp_curve`] 的产出）。
    pub default_xp_curve_id: ContentIndex,
    /// 本体默认伤害公式索引（[`register_base_damage_formula`] 的产出）。
    pub default_damage_formula_id: ContentIndex,
    /// 本体默认伤害类别索引（[`register_base_damage_category`] 的产出）
    /// ——包成 [`crate::damage_category::RegistryDamageCategories`] 才是
    /// 结算侧真正消费它的形状。
    pub default_damage_category_id: ContentIndex,
    /// 本体占位内容索引（[`register_base_placeholder_content`] 的产出）
    /// ——不落在任何一张内容表里，见 [`crate::base_placeholder`]。
    pub placeholder_race_id: ContentIndex,

    /// 地形表。
    pub terrain: TerrainTable,
    /// 职业表。
    pub class: ClassTable,
    /// 技能表。
    pub skill: SkillTable,
    /// 副职表。
    pub subclass: SubclassTable,
    /// 任务表。
    pub quest: QuestTable,
    /// 种族表。
    pub race: RaceTable,
    /// 动画剪辑表。
    pub clip: ClipTable,
    /// 经验曲线定义表。
    pub xp_curve: XpCurveTable,
    /// 职业/种族 → 经验曲线绑定表。
    pub xp_curve_bindings: XpCurveBindings,
    /// 职业 → 行为原型绑定表（`crate::behavior_binding`）。
    pub class_behavior_bindings: ClassBehaviorBindings,
    /// 天赋表。
    pub trait_def: TraitTable,
    /// 资源池表。
    pub resource_pool: ResourcePoolTable,
    /// 物品表。
    pub item: ItemTable,
    /// 伤害公式定义表。
    pub formula: FormulaTable,
    /// 武器类别定义表。
    pub weapon_category: WeaponCategoryTable,
    /// 伤害类别定义表。
    pub damage_category: DamageCategoryTable,
    /// 空间层属性表。
    pub space_profile: SpaceProfileTable,
    /// 资源表（`resources.json5`），见 `ll_world::resource` 模块文档。
    pub resource: ResourceTable,
    /// 文化表（`cultures.json5`），见 `ll_world::culture` 模块文档。
    pub culture: CultureTable,
    /// 天气表。
    pub weather: WeatherTable,
    /// 配方表。
    pub recipe: RecipeTable,
    /// 配方类别表。
    pub recipe_category: RecipeCategoryTable,
    /// 标签表。
    pub tag: TagTable,
    /// 加值类型表。
    pub modifier_type: ModifierTypeTable,
}

impl LoadSession {
    /// 跑完**全部引擎侧注册**，产出一个「mod 还没装、但引擎自己那部分
    /// 本体内容已经在了」的会话。
    ///
    /// # 注册顺序是这条链路的一部分
    ///
    /// 下面八次调用的先后顺序决定了各条内容拿到哪个 [`ContentIndex`]
    /// （[`ll_core::ident::Interner::intern`] 分配的就是插入顺序下标）。
    /// 索引本身不进存档（存档写字符串 id），因此顺序不是兼容性契约；
    /// 但它确实决定了 `ContentIndex::default()`（索引 0）落在谁头上
    /// ——本序列里是**第一条本体地形**，一批「查不到就退回默认值」的
    /// 保底路径依赖这一点表现成「谁都不命中」，见
    /// `ll_sim::damage_category::NoDamageCategories` 文档。改动顺序前
    /// 请先读那一段。
    ///
    /// # 为什么这里可以 `expect`
    ///
    /// 八条注册的输入全都是写死在 Rust 里的字面量声明表，内部一致性
    /// 由各自的单元测试守着——这与已经迁进 `mods/lostland/` 的那部分
    /// 本体内容不同（那部分玩家可以误删/改名，因此走
    /// `ll_mod::base_contract` 那条**会失败**的契约解析）。逐字沿用
    /// `ll_game::content::load_content` 原有的 `expect` 文案。
    pub fn with_engine_registrations() -> LoadSession {
        let mut registry = Registry::new();

        let (terrain_ids, terrain) =
            register_base_terrain(&mut registry).expect("本体地形声明表内部一致，注册恒不失败");
        let (space_ids, space_profile) = register_base_space_profiles(&mut registry)
            .expect("本体空间层属性声明表内部一致，注册恒不失败");
        let placeholder_race_id = register_base_placeholder_content(&mut registry);
        let (clip_ids, clip) =
            register_base_clips(&mut registry).expect("本体剪辑声明表内部一致，注册恒不失败");
        let (default_xp_curve_id, xp_curve) = register_base_xp_curve(&mut |id| registry.intern(id))
            .expect("本体默认经验曲线声明内部一致，注册恒不失败");
        let (default_damage_formula_id, formula) =
            register_base_damage_formula(&mut |id| registry.intern(id))
                .expect("本体默认伤害公式声明内部一致，注册恒不失败");
        let (default_damage_category_id, damage_category) =
            register_base_damage_category(&mut |id| registry.intern(id))
                .expect("本体默认伤害类别声明内部一致，注册恒不失败");
        let (weather_ids, weather) =
            register_base_weathers(&mut registry).expect("本体天气声明表内部一致，注册恒不失败");

        LoadSession {
            registry,
            terrain_ids,
            space_ids,
            clip_ids,
            weather_ids,
            default_xp_curve_id,
            default_damage_formula_id,
            default_damage_category_id,
            placeholder_race_id,
            terrain,
            class: ClassTable::new(),
            skill: SkillTable::new(),
            subclass: SubclassTable::new(),
            quest: QuestTable::new(),
            race: RaceTable::new(),
            clip,
            xp_curve,
            xp_curve_bindings: XpCurveBindings::new(),
            class_behavior_bindings: ClassBehaviorBindings::new(),
            trait_def: TraitTable::new(),
            resource_pool: ResourcePoolTable::new(),
            item: ItemTable::new(),
            formula,
            weapon_category: WeaponCategoryTable::new(),
            damage_category,
            space_profile,
            resource: ResourceTable::new(),
            culture: CultureTable::new(),
            weather,
            recipe: RecipeTable::new(),
            recipe_category: RecipeCategoryTable::new(),
            tag: TagTable::new(),
            modifier_type: ModifierTypeTable::new(),
        }
    }

    /// 装载 `mods_root` 下的全部 mod（含本体的 `mods/lostland/`）——
    /// 直接转调 [`crate::pipeline::load_all`]，只是把「二十二张表怎么
    /// 拼成一个 [`GameplayTables`]」这份清单收在一处。
    ///
    /// 可以对同一个会话调用多次（例如先装一个目录再装另一个），语义
    /// 与连续两次 `load_all` 完全相同——本方法不持有任何跨调用状态。
    pub fn load_all(&mut self, mods_root: &Path) -> LoadReport {
        // 逐字段解构而不是 `&mut self.xxx` 逐个写：借用检查器需要看到
        // 这些是**互不重叠**的字段借用（`registry` 与各表同时可变借出）。
        let LoadSession {
            registry,
            terrain,
            class,
            skill,
            subclass,
            quest,
            race,
            clip,
            xp_curve,
            xp_curve_bindings,
            class_behavior_bindings,
            trait_def,
            resource_pool,
            item,
            formula,
            weapon_category,
            damage_category,
            space_profile,
            resource,
            culture,
            weather,
            recipe,
            recipe_category,
            tag,
            modifier_type,
            ..
        } = self;

        load_all(
            mods_root,
            registry,
            &mut GameplayTables {
                terrain,
                class,
                skill,
                subclass,
                quest,
                race,
                clip,
                xp_curve,
                xp_curve_bindings,
                class_behavior_bindings,
                trait_def,
                resource_pool,
                item,
                formula,
                weapon_category,
                damage_category,
                space_profile,
                resource,
                culture,
                weather,
                recipe,
                recipe_category,
                modifier_type,
                tag,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_damage_category::DEFAULT_DAMAGE_CATEGORY_ID;
    use crate::base_damage_formula::DEFAULT_DAMAGE_FORMULA_ID;
    use ll_core::ident::NamespacedId;

    fn index(session: &LoadSession, raw: &str) -> Option<ContentIndex> {
        session
            .registry
            .get(&NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    #[test]
    fn 引擎侧注册跑完之后八类本体内容都已经在注册表里() {
        // Arrange & Act
        let session = LoadSession::with_engine_registrations();

        // Assert：这一条守的是「测试与生产跑同一段注册」——少任何一条，
        // 引用它的内容数据文件在测试里就会装不上，而在真实游戏里装得上。
        assert!(session.terrain.is_defined(session.terrain_ids.grass));
        assert!(session.space_profile.is_defined(session.space_ids.surface));
        assert!(session.clip.get(session.clip_ids.hero_idle).is_some());
        assert!(session.weather.is_defined(session.weather_ids.clear));
        assert!(session.xp_curve.get(session.default_xp_curve_id).is_some());
        assert!(
            session
                .formula
                .is_defined(session.default_damage_formula_id)
        );
        assert!(
            session
                .damage_category
                .is_defined(session.default_damage_category_id)
        );
        assert_eq!(
            index(&session, DEFAULT_DAMAGE_FORMULA_ID),
            Some(session.default_damage_formula_id)
        );
        assert_eq!(
            index(&session, DEFAULT_DAMAGE_CATEGORY_ID),
            Some(session.default_damage_category_id)
        );
        assert!(index(&session, crate::base_placeholder::PLACEHOLDER_RACE_ID).is_some());
    }

    /// 一个只声明了一件物品的最小 mod，那件物品**同时引用两条引擎注册
    /// 的本体内容**：`lostland:default_damage_formula`（伤害公式）与
    /// `lostland:physical`（伤害类别）。两条字段在注册期都是「只 get 不
    /// intern」的硬校验（ADR 0017），因此引用不到就是整批装载失败。
    fn write_mod_referencing_engine_content(root: &Path) {
        let dir = root.join("engine_ref_mod");
        std::fs::create_dir_all(&dir).expect("测试临时 mod 目录创建不应失败");
        std::fs::write(
            dir.join("mod.json5"),
            "{ namespace: \"engineref\", version: \"0.1.0\" }",
        )
        .expect("写清单不应失败");
        std::fs::write(
            dir.join("items.json5"),
            r#"{
  items: [
    {
      id: "engineref:brand",
      display_name_key: "engineref:item.brand.display_name",
      stack_limit: 1,
      base_weight: 1000,
      base_price: 1000,
      damage_formula: "lostland:default_damage_formula",
      damage_category: "lostland:physical",
    },
  ],
}"#,
        )
        .expect("写内容文件不应失败");
    }

    #[test]
    fn 内容文件可以引用引擎注册的本体内容() {
        // Arrange
        let root = crate::test_support::tempdir();
        write_mod_referencing_engine_content(root.path());
        let mut session = LoadSession::with_engine_registrations();

        // Act
        let report = session.load_all(root.path());

        // Assert：这一条是本模块存在的理由——在 `LoadSession` 之前，
        // `crates/ll-mod/tests/` 里的帮手只调 `load_all`、不跑引擎注册，
        // 于是**同一份内容在真实游戏里装得上、在测试里装不上**。
        let id = NamespacedId::parse("engineref:self").expect("合法标识符");
        let status = report
            .entries
            .iter()
            .find(|(entry_id, _)| *entry_id == id)
            .map(|(_, status)| status);
        assert_eq!(
            status,
            Some(&crate::load_report::LoadStatus::Loaded),
            "引用引擎注册内容的 mod 必须装载成功，实际报告：{:?}",
            report.entries
        );
    }

    #[test]
    fn 不跑引擎注册就装载同一份内容会失败() {
        // Arrange：这是上一条的反例——直接用一套空表调 `load_all`，
        // 正是本批次修掉的那二十七处帮手此前的形状。少了这一条，上一条
        // 断言的可能只是「这份内容本来就装得上」。
        let root = crate::test_support::tempdir();
        write_mod_referencing_engine_content(root.path());
        let mut registry = Registry::new();
        let mut owned = crate::test_support::OwnedTables::default();

        // Act
        let report = load_all(root.path(), &mut registry, &mut owned.as_gameplay_tables());

        // Assert
        let id = NamespacedId::parse("engineref:self").expect("合法标识符");
        let status = report
            .entries
            .iter()
            .find(|(entry_id, _)| *entry_id == id)
            .map(|(_, status)| status);
        assert!(
            matches!(status, Some(crate::load_report::LoadStatus::Failed(_))),
            "没跑引擎注册时，引用引擎内容的 mod 必须装载失败，实际报告：{:?}",
            report.entries
        );
    }
}
