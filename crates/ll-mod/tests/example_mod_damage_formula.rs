//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-damage-formula`/`register-item-damage-formula` 这两个新
//! 脚本 API 真的能被 `mods/example_mod/gameplay.scm` 调用，且两条真实
//! 注册的公式（`examplemod:iron_sword_formula`——确定性/
//! `examplemod:flame_longbow_formula`——骰子驱动）确实分别挂在两件
//! 真实注册的武器上——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要
//! 有真实 mod 脚本为证」，本文件是伤害公式引擎批次的那份证据，不能靠
//! `crates/ll-sim/src/formula.rs`/`crates/ll-mod/src/script_damage_formula_api.rs`
//! 里的单元测试自证。
//!
//! # 为什么装载整个 `mods/` 目录，不是只挑 `example_mod`
//!
//! 理由同 `example_mod_xp_curves.rs` 模块文档同一节——`load_all` 对
//! 候选目录逐个独立处理，一个失败不影响其他 mod。

use std::path::Path;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::formula::FormulaTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::formula::{FormulaInputs, eval_formula};
use ll_sim::item::ItemCatalog;
use ll_world::terrain::TerrainTable;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_xp_curves.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引，理由同 `example_mod_xp_curves.rs::load_real_mods_and_resolve`。
struct RealModsHandle {
    report: ll_mod::load_report::LoadReport,
    formula: FormulaTable,
    item: ItemTable,
    iron_sword_formula_id: ContentIndex,
    flame_longbow_formula_id: ContentIndex,
    iron_sword_id: ContentIndex,
    flame_longbow_id: ContentIndex,
}

fn load_real_mods_and_resolve() -> RealModsHandle {
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
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut tag_table = ll_mod::tag::TagTable::new();
    let mut damage_category = DamageCategoryTable::new();
    let report = load_all(
        Path::new(REAL_MODS_ROOT),
        &mut registry,
        &mut GameplayTables {
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
            space_profile: &mut space_profile,
            weather: &mut weather_table,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            tag: &mut tag_table,
        },
    );
    let examplemod_id = NamespacedId::parse("examplemod:self").unwrap();
    let examplemod_status = report
        .entries
        .iter()
        .find(|(id, _)| *id == examplemod_id)
        .map(|(_, status)| status);
    assert_eq!(
        examplemod_status,
        Some(&LoadStatus::Loaded),
        "examplemod 必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/gameplay.scm 注册"))
    };

    RealModsHandle {
        iron_sword_formula_id: resolve("examplemod:iron_sword_formula"),
        flame_longbow_formula_id: resolve("examplemod:flame_longbow_formula"),
        iron_sword_id: resolve("examplemod:iron_sword"),
        flame_longbow_id: resolve("examplemod:flame_longbow"),
        report,
        formula,
        item,
    }
}

#[test]
fn 真实mods目录装载后examplemod被判定为已加载而两个故意写错的mod失败() {
    // Arrange & Act
    let handle = load_real_mods_and_resolve();

    // Assert：与 ll-game 二进制真实运行时的基线一致——loaded=2
    // （examplemod + 本体内容 mod lostland，本体内容迁进脚本批次
    // 起后者也是一个真实的 mod）、failed=2（broken_syntax/
    // broken_whitelist），见 `ll_game::content` 模块 `tracing::info!`
    // 输出的 `loaded`/`failed` 字段。
    assert_eq!(handle.report.loaded_count(), 2);
    assert_eq!(handle.report.failed_count(), 2);
}

#[test]
fn 铁剑显式引用的伤害公式索引与真实注册的公式索引一致() {
    // Arrange
    let handle = load_real_mods_and_resolve();

    // Act
    let rule = ItemCatalog::item(&handle.item, handle.iron_sword_id)
        .expect("铁剑应当已经通过 register-item 注册");

    // Assert
    assert_eq!(rule.damage_formula, Some(handle.iron_sword_formula_id));
}

#[test]
fn 火焰长弓显式引用的伤害公式索引与真实注册的公式索引一致() {
    // Arrange
    let handle = load_real_mods_and_resolve();

    // Act
    let rule = ItemCatalog::item(&handle.item, handle.flame_longbow_id)
        .expect("火焰长弓应当已经通过 register-item 注册");

    // Assert
    assert_eq!(rule.damage_formula, Some(handle.flame_longbow_formula_id));
}

#[test]
fn 铁剑公式是确定性的不消耗随机流且求值结果可手算复现() {
    // 直接验收设计文档四节的论证：铁剑公式（+ attack-power str-mod）
    // 恒定确定,同样的输入永远算出同样的结果，不含任何随机性。
    // Arrange
    let handle = load_real_mods_and_resolve();
    let def = handle
        .formula
        .get(handle.iron_sword_formula_id)
        .expect("铁剑公式应当已被注册");
    assert!(!def.needs_rng, "纯代数公式不应该被标记为需要随机流");

    // Act：攻击力 10、力量调整值 3，两次独立求值。
    let mut inputs = FormulaInputs::new(10, 0, 0, 0, [0; 7], false);
    inputs.attribute_modifiers[ll_world::entity::AttributeKind::Strength as usize] = 3;
    let mut rng_a = ll_core::rng::DetRng::for_entity(1, 2, 3);
    let mut rng_b = ll_core::rng::DetRng::for_entity(9, 9, 9);
    let first = eval_formula(def, &inputs, &mut rng_a);
    let second = eval_formula(def, &inputs, &mut rng_b);

    // Assert：10 + 3 = 13，与用哪条（未被使用的）随机流无关。
    assert_eq!(first, 13);
    assert_eq!(second, 13);
}

#[test]
fn 火焰长弓公式是骰子驱动的且暴击时骰子数量翻倍() {
    // 直接验收设计文档四节论证：随机性来源、暴击处理点与铁剑公式截然
    // 不同——火焰长弓公式非暴击时是 1d10 + dex-mod，暴击时骰子数量翻倍
    // 为 2d10（不是把最终结果乘二），且不同种子/两次独立求值都落在
    // 各自应有的区间内（确定性由 crate::formula 的单元测试单独覆盖，
    // 这里只验收「真实注册的这条公式」符合设计意图）。
    // Arrange
    let handle = load_real_mods_and_resolve();
    let def = handle
        .formula
        .get(handle.flame_longbow_formula_id)
        .expect("火焰长弓公式应当已被注册");
    assert!(def.needs_rng, "骰子驱动的公式应当被标记为需要随机流");
    let mut inputs = FormulaInputs::new(0, 0, 0, 0, [0; 7], false);
    inputs.attribute_modifiers[ll_world::entity::AttributeKind::Dexterity as usize] = 2;

    // Act：非暴击（1d10+2，区间 3..=12）。
    let mut rng_normal = ll_core::rng::DetRng::for_entity(1, 2, 3);
    let normal = eval_formula(def, &inputs, &mut rng_normal);

    // Act：暴击（2d10+2，区间 4..=22）。
    inputs.crit = true;
    let mut rng_crit = ll_core::rng::DetRng::for_entity(4, 5, 6);
    let crit = eval_formula(def, &inputs, &mut rng_crit);

    // Assert
    assert!((3..=12).contains(&normal), "非暴击应落在 1d10+2 的区间");
    assert!((4..=22).contains(&crit), "暴击应落在 2d10+2 的区间");
}

#[test]
fn 两条公式的needs_rng标记不同证明这是两套不同的规则() {
    // 最直接的「不是调系数」证据：一条公式在编译期就被判定为需要
    // 随机流，另一条不需要——这个布尔本身就是"表达方式截然不同"的
    // 结构性证明，不是数值层面的差异。
    // Arrange
    let handle = load_real_mods_and_resolve();

    // Act
    let iron_sword_def = handle
        .formula
        .get(handle.iron_sword_formula_id)
        .expect("铁剑公式应当已被注册");
    let flame_longbow_def = handle
        .formula
        .get(handle.flame_longbow_formula_id)
        .expect("火焰长弓公式应当已被注册");

    // Assert
    assert_ne!(iron_sword_def.needs_rng, flame_longbow_def.needs_rng);
}
