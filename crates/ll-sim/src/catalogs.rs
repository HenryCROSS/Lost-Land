//! 结算所需的全部只读内容目录打成的一束引用——[`ResolveCatalogs`]。
//!
//! # 为什么需要这一束，而 `resolve` 自己仍然收散参数
//!
//! [`crate::resolve::resolve_with_all_catalogs`] 把九份目录逐个列成
//! 参数，那是刻意的：依赖倒置的意义就在于「这一段结算到底依赖哪几份
//! 只读内容」写在签名上一目了然，`resolve_dispatch` 的文档已经明确
//! 记过「不是可以合并成一个结构体的意外堆叠」。本模块**不推翻那条
//! 结论**——`resolve` 一族入口的签名一个字都不变。
//!
//! 本束服务的是另一件事：**把目录搬过一层调用边界**。
//! [`crate::turn::TurnEngine`] 自己不消费任何一份目录，它只是把调用方
//! 给的东西原样转交给 `resolve`。让 `advance_ai`/`try_player_turn`
//! 各自多出九个参数，只会把「TurnEngine 依赖这九份内容」这个错误信号
//! 写进签名（它一份都不读），并且此后每接一份新目录都要改三处签名与
//! 全部调用点。一束引用是搬运工的正确形状：TurnEngine 的签名只说
//! 「我需要一束结算目录，原样转交」。
//!
//! # 为什么不挂到 `WorldState` 上
//!
//! `WorldState` 是运行期状态、要进存档；内容表是装载期产物，
//! `crates/ll-content/src/save_file.rs` 刻意不序列化任何 `*Table`
//! （见其文档）。把目录塞进 `WorldState` 会立刻把这两类东西的生命周期
//! 焊死，并逼着存档格式回答「这些表怎么存」这个本不该存在的问题。
//! 本束是**借用**，不持有所有权，生命周期只覆盖一次调用。
//!
//! # 依赖方向不变
//!
//! 字段全是 `&dyn` trait 对象，trait 定义都在 `ll-sim` 自己这一侧
//! （`SkillCatalog`/`TraitCatalog`/`TraitGrantSource`/……），真实实现
//! 在 `ll-mod`（`SkillTable`/`TraitTable`/`RaceTable`/`ClassTable`……）。
//! `ll-sim` 依然不认识 `ll-mod` 的任何类型，与既有的依赖倒置模式完全
//! 一致，本模块没有引入任何新的依赖边。

use crate::craft::{NoRecipes, RecipeCatalog};
use crate::damage_category::{DamageCategoryCatalog, NoDamageCategories};
use crate::exposure::AmbientSource;
use crate::formula::{DamageFormulaCatalog, NoFormulas};
use crate::item::{ItemCatalog, NoItems};
use crate::quest::{NoQuests, QuestCatalog};
use crate::resource_pool::{NoResourcePools, ResourcePoolCatalog};
use crate::skill::{NoSkills, SkillCatalog};
use crate::traits::{NoTraitGrants, NoTraits, TraitCatalog, TraitGrantSource};

/// 一次结算需要的全部只读内容目录（借用，不持有所有权）。
///
/// 字段与 [`crate::resolve::resolve_with_all_catalogs`] 的九个目录
/// 参数一一对应、同序（`trait_defs` 是唯一一个不同名的，理由见该字段
/// 自己的文档）——这是刻意的：想知道某个字段的语义与「不接这一路会
/// 怎样」，读那个入口对应参数的文档即可，本类型不重复一遍。
///
/// 不派生 `Debug`：字段都是 `&dyn`，那些 trait 都不要求 `Debug`
/// （理由同 [`crate::traits::TraitSource`] 的同一条取舍）。
#[derive(Clone, Copy)]
pub struct ResolveCatalogs<'a> {
    /// 技能目录——`Intent::UseSkill` 的定义来源。
    pub skills: &'a dyn SkillCatalog,
    /// 任务目录——击杀推进任务进度。
    pub quests: &'a dyn QuestCatalog,
    /// 种族天赋授予来源（所有者取 `Agent::race`）。
    pub race_traits: &'a dyn TraitGrantSource,
    /// 职业天赋授予来源（所有者取 `Agent::profession`）。
    pub class_traits: &'a dyn TraitGrantSource,
    /// 天赋定义目录——授予技能/资源池容量/规则修正都从这里查。
    ///
    /// 字段名是 `trait_defs` 而不是与 `resolve_with_all_catalogs` 的
    /// 参数同名的 `traits`：`scripts/ci/check_field_consumers.py` 判定
    /// 「决策层有没有读某个字段」用的是全文正则 `\.字段名`，本 crate
    /// 的 `src/*.rs` 全在它的决策层通配里，一个叫 `traits` 的字段会让
    /// `RaceDef.traits`/`ClassDef.traits` 被误判成「靠本字段接线」——
    /// 那正是该脚本头注释「已知局限」第 2 条反向的那半（字段名撞车
    /// 导致的假阴性）。错开名字比往豁免清单里写一条名不副实的理由
    /// 便宜得多。
    pub trait_defs: &'a dyn TraitCatalog,
    /// 资源池目录——消耗判定与每回合自动恢复。
    pub pools: &'a dyn ResourcePoolCatalog,
    /// 物品目录——堆叠上限、装备属性、武器引用。
    pub items: &'a dyn ItemCatalog,
    /// 伤害公式目录——攻击力数值。
    pub formulas: &'a dyn DamageFormulaCatalog,
    /// 伤害类别目录——武器没有显式声明类别时退回哪一个。
    pub damage_categories: &'a dyn DamageCategoryCatalog,
    /// 配方目录——`Intent::Craft` 的定义来源，兼答「这条配方长什么样」
    /// 与「这个配方类别要求哪些副职」两个问题（制作系统批次新增）。
    pub recipes: &'a dyn RecipeCatalog,
    /// 环境来源——空间层属性表 + 天气表，温度那一路的输入（温度系统
    /// 批次新增）。
    ///
    /// # 为什么它不是一个 `&dyn 某某Catalog`
    ///
    /// 其余九份全是 `&'a dyn` trait 对象，因为它们背后的真实实现都在
    /// **下游**的 `ll-mod`，`ll-sim` 只能靠依赖倒置够到。温度用的两张
    /// 表（`ll_world::space_profile::SpaceProfileTable`/
    /// `ll_world::weather::WeatherTable`）定义在**上游**的 `ll-world`，
    /// 可以直接借具体类型——完整论证见
    /// [`crate::exposure::AmbientSource`] 文档「为什么不是一个 trait」
    /// 一节。为了对称而多造一对没有第二个实现的 trait，正是 ADR 0021
    /// 点名要避免的那种抽象。
    ///
    /// 它是 `Copy` 值而不是引用：内部就是两个 `Option<&'a Table>`，
    /// 再套一层引用只是多一次间接。
    pub ambient: AmbientSource<'a>,
}

/// [`ResolveCatalogs::empty`] 借出的九个目录空实现的 `'static` 实例。
/// 第十项（环境来源）不在这里：它是 `Copy` 值，空对象就是
/// [`AmbientSource::NONE`] 这个常量本身，不需要借出引用。
///
/// 逐个具名而不是在 `empty` 里写 `&NoSkills`：零大小类型的临时值虽然
/// 会被常量提升，但具名常量让「空目录只有这一组实例」这件事在阅读时
/// 不需要依赖对提升规则的记忆——与 [`crate::traits::NO_TRAIT_GRANTS`]
/// 当初被单独具名是同一条理由。
const NO_SKILLS: NoSkills = NoSkills;
const NO_QUESTS: NoQuests = NoQuests;
const NO_RACE_TRAIT_GRANTS: NoTraitGrants = NoTraitGrants;
const NO_CLASS_TRAIT_GRANTS: NoTraitGrants = NoTraitGrants;
const NO_TRAITS: NoTraits = NoTraits;
const NO_RESOURCE_POOLS: NoResourcePools = NoResourcePools;
const NO_ITEMS: NoItems = NoItems;
const NO_FORMULAS: NoFormulas = NoFormulas;
const NO_DAMAGE_CATEGORIES: NoDamageCategories = NoDamageCategories;
const NO_RECIPES: NoRecipes = NoRecipes;

impl ResolveCatalogs<'static> {
    /// 十路全空的一束——与「一份目录都没接」在行为上完全等价
    /// （[`crate::resolve::resolve`] 就是这个形状）。
    ///
    /// 供两类调用点使用：一是本身没有任何内容表的场景（`ll-sim` 的
    /// `examples/` 验收 demo 自己合成世界，从不装载 `mods/`），二是
    /// 测试里作为对照组——「同一段结算，只把这一束换成空的，结果就
    /// 回到接线之前」正是接线本身有没有生效的判据，见
    /// `crates/ll-mod/tests/turn_engine_catalogs.rs`。
    pub const fn empty() -> ResolveCatalogs<'static> {
        ResolveCatalogs {
            skills: &NO_SKILLS,
            quests: &NO_QUESTS,
            race_traits: &NO_RACE_TRAIT_GRANTS,
            class_traits: &NO_CLASS_TRAIT_GRANTS,
            trait_defs: &NO_TRAITS,
            pools: &NO_RESOURCE_POOLS,
            items: &NO_ITEMS,
            formulas: &NO_FORMULAS,
            damage_categories: &NO_DAMAGE_CATEGORIES,
            recipes: &NO_RECIPES,
            ambient: AmbientSource::NONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::ContentIndex;

    #[test]
    fn 空目录束的每一路都查不到任何内容() {
        // 本条守住的是「`empty()` 真的九路全空」——它是接线验收测试的
        // 对照组，对照组若有任何一路悄悄不空，那份验收就失去意义。
        // Arrange
        let catalogs = ResolveCatalogs::empty();
        let any = ContentIndex::default();

        // Act & Assert
        assert!(catalogs.skills.skill(any).is_none());
        assert!(catalogs.trait_defs.trait_rule(any).is_none());
        assert!(catalogs.race_traits.granted_traits(any).is_empty());
        assert!(catalogs.class_traits.granted_traits(any).is_empty());
        assert!(catalogs.pools.resource_pool(any).is_none());
        assert!(catalogs.items.item(any).is_none());
        assert!(catalogs.quests.kill_count_quests().is_empty());
        assert_eq!(catalogs.damage_categories.default_category(), any);
        assert!(catalogs.recipes.recipe(any).is_none());
        assert!(
            catalogs
                .recipes
                .category_required_subclasses(any)
                .is_empty()
        );
    }
}
