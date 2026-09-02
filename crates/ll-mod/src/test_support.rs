//! 测试专用的临时目录帮手，供 `discover`/`manifest`/`pipeline` 三处
//! mod 加载测试共用。
//!
//! 三份实现此前逐字重复（仅临时目录名的前缀不同：`ll-mod-discover-test`/
//! `ll-mod-test`/`ll-mod-pipeline-test`），抽成一处避免改一处逻辑
//! （例如清理策略）时漏改另外两处。前缀本身只是给人肉眼定位残留目录
//! 用的调试信息，不参与任何正确性判断——合并成一个固定前缀不改变
//! 「进程内不冲突、用完自动清理」这条行为。
#![cfg(test)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::behavior_binding::ClassBehaviorBindings;
use crate::class::ClassTable;
use crate::clip::ClipTable;
use crate::damage_category::DamageCategoryTable;
use crate::dialogue::{DialogueNodeTable, DialogueTable};
use crate::formula::FormulaTable;
use crate::item::ItemTable;
use crate::modifier_type::ModifierTypeTable;
use crate::pipeline::GameplayTables;
use crate::quest::QuestTable;
use crate::race::RaceTable;
use crate::recipe::RecipeTable;
use crate::recipe_category::RecipeCategoryTable;
use crate::resource_pool::ResourcePoolTable;
use crate::skill::SkillTable;
use crate::subclass::SubclassTable;
use crate::tag::TagTable;
use crate::trait_def::TraitTable;
use crate::weapon_category::WeaponCategoryTable;
use crate::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_world::culture::CultureTable;
use ll_world::resource::ResourceTable;
use ll_world::space_profile::SpaceProfileTable;
use ll_world::terrain::TerrainTable;
use ll_world::weather::WeatherTable;

/// 一个会在析构时自动清理的临时目录。
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 建一个进程内大概率不冲突的临时目录：进程 ID + 单调计数器拼路径，
/// 用完在 [`TempDir`] 析构时自动清理。本 crate 不为此引入 `tempfile`
/// 依赖——只有测试需要，且需求简单到手写几行就够。
pub(crate) fn tempdir() -> TempDir {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("ll-mod-test-{}-{n}", std::process::id()));
    fs::create_dir_all(&path).expect("测试临时目录创建不应失败");
    TempDir(path)
}

/// 测试帮手：现造一套全新的空内容表，供 [`crate::pipeline::GameplayTables`] 借用——
/// 各测试只关心地形（`register-terrain` 仍是既有场景里用得最多的
/// 一类），但 `load_all` 的签名要求十张表一起传，本结构体把「造出
/// 十个空表」这件事集中成一次调用，不必在每条测试里重复八行。
#[derive(Default)]
pub(crate) struct OwnedTables {
    pub(crate) terrain: TerrainTable,
    pub(crate) class: ClassTable,
    pub(crate) skill: SkillTable,
    pub(crate) subclass: SubclassTable,
    pub(crate) quest: QuestTable,
    pub(crate) dialogue: DialogueTable,
    pub(crate) dialogue_node: DialogueNodeTable,
    pub(crate) race: RaceTable,
    pub(crate) clip: ClipTable,
    pub(crate) xp_curve: XpCurveTable,
    pub(crate) xp_curve_bindings: XpCurveBindings,
    pub(crate) class_behavior_bindings: ClassBehaviorBindings,
    pub(crate) trait_def: TraitTable,
    pub(crate) resource_pool: ResourcePoolTable,
    pub(crate) item: ItemTable,
    pub(crate) formula: FormulaTable,
    pub(crate) weapon_category: WeaponCategoryTable,
    pub(crate) damage_category: DamageCategoryTable,
    pub(crate) tag: TagTable,
    pub(crate) space_profile: SpaceProfileTable,
    pub(crate) resource: ResourceTable,
    pub(crate) culture: CultureTable,
    pub(crate) weather: WeatherTable,
    pub(crate) recipe: RecipeTable,
    pub(crate) recipe_category: RecipeCategoryTable,
    pub(crate) modifier_type: ModifierTypeTable,
}

impl OwnedTables {
    pub(crate) fn as_gameplay_tables(&mut self) -> GameplayTables<'_> {
        GameplayTables {
            terrain: &mut self.terrain,
            class: &mut self.class,
            skill: &mut self.skill,
            subclass: &mut self.subclass,
            quest: &mut self.quest,
            dialogue: &mut self.dialogue,
            dialogue_node: &mut self.dialogue_node,
            race: &mut self.race,
            clip: &mut self.clip,
            xp_curve: &mut self.xp_curve,
            xp_curve_bindings: &mut self.xp_curve_bindings,
            class_behavior_bindings: &mut self.class_behavior_bindings,
            trait_def: &mut self.trait_def,
            resource_pool: &mut self.resource_pool,
            item: &mut self.item,
            formula: &mut self.formula,
            weapon_category: &mut self.weapon_category,
            damage_category: &mut self.damage_category,
            tag: &mut self.tag,
            space_profile: &mut self.space_profile,
            resource: &mut self.resource,
            culture: &mut self.culture,
            weather: &mut self.weather,
            recipe: &mut self.recipe,
            recipe_category: &mut self.recipe_category,
            modifier_type: &mut self.modifier_type,
        }
    }

    /// 同一批表的**只读**视图——值哈希与装载后校验那一侧要的形状，见
    /// [`crate::content_hash::ContentValueTables`]。与
    /// [`Self::as_gameplay_tables`] 同一条理由抽在这里：逐字段拼这个
    /// 结构体有二十四行，在每个需要它的测试模块里各写一遍必然分叉。
    ///
    /// 不含 `xp_curve_bindings`/`class_behavior_bindings`——那两张是
    /// 绑定表，不为自己的条目分配 `ContentIndex`，因此不在
    /// `ContentValueTables` 里，见 `content_hash` 模块文档「例外，且是
    /// 刻意的例外」一段。
    pub(crate) fn as_value_tables(&self) -> crate::content_hash::ContentValueTables<'_> {
        crate::content_hash::ContentValueTables {
            terrain: &self.terrain,
            class: &self.class,
            skill: &self.skill,
            subclass: &self.subclass,
            quest: &self.quest,
            dialogue: &self.dialogue,
            dialogue_node: &self.dialogue_node,
            race: &self.race,
            clip: &self.clip,
            xp_curve: &self.xp_curve,
            trait_def: &self.trait_def,
            resource_pool: &self.resource_pool,
            item: &self.item,
            formula: &self.formula,
            weapon_category: &self.weapon_category,
            damage_category: &self.damage_category,
            tag: &self.tag,
            space_profile: &self.space_profile,
            resource: &self.resource,
            culture: &self.culture,
            weather: &self.weather,
            recipe: &self.recipe,
            recipe_category: &self.recipe_category,
            modifier_type: &self.modifier_type,
        }
    }
}
