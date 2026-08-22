//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-xp-curve`/`register-class-xp-curve`/`register-race-xp-curve`/
//! `register-race-xp-reward` 这四个新脚本 API 真的能被
//! `mods/example_mod/gameplay.scm` 调用，且两条真实注册的曲线（线性/
//! 递推指数）在同一等级上门槛确实不同——ADR 0018「玩法层内容必须能从
//! mod 脚本注册，且要有真实 mod 脚本为证」，本文件是那份证据，不能靠
//! 单元测试自证。
//!
//! # 为什么装载整个 `mods/` 目录，不是只挑 `example_mod`
//!
//! `mods/` 下还有 `broken_syntax`/`broken_whitelist` 两个刻意写错的
//! mod（P4 验收 demo 遗留）——`ll_mod::pipeline::load_all` 对候选目录
//! 逐个独立处理，一个失败不影响其他 mod（见其模块文档），本测试因此
//! 能在装载真实全部三个 mod 的同时，只关心 `example_mod` 是否成功、
//! 其余两个是否如预期失败，不需要为了「只测 example_mod」单独复制一份
//! 目录。

use std::path::Path;

use ll_core::ident::NamespacedId;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::experience::ExperienceCatalog;
use ll_sim::xp_curve::eval_xp_curve;
use ll_world::terrain::TerrainTable;

/// 仓库根目录下的真实 `mods/` 路径——与
/// `crates/ll-ui/examples/p4_acceptance/world.rs` 的 `PRIMARY_MODS_ROOT`
/// 是同一个目录，只是从 `ll-mod` 自己的 `CARGO_MANIFEST_DIR` 出发多跳
/// 一层。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，连同装载报告一起返回全部断言需要的表
/// 与已经解析好的索引——四条测试各自独立调用一次（`load_all` 消费掉
/// 的表不能跨测试共享），但都需要「装载 + 把 `"examplemod:xxx"` 换成
/// `ContentIndex`」这一整套前置步骤，这里封成一步复用。
struct RealModsHandle {
    report: ll_mod::load_report::LoadReport,
    race: RaceTable,
    xp_curve: XpCurveTable,
    bindings: XpCurveBindings,
    linear_curve_id: ll_core::ident::ContentIndex,
    recursive_curve_id: ll_core::ident::ContentIndex,
    goblin_id: ll_core::ident::ContentIndex,
    necromancer_class_id: ll_core::ident::ContentIndex,
    half_elf_race_id: ll_core::ident::ContentIndex,
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
    let mut bindings = XpCurveBindings::new();
    let mut trait_def = TraitTable::new();
    let mut resource_pool = ll_mod::resource_pool::ResourcePoolTable::new();
    let mut item = ll_mod::item::ItemTable::new();
    let mut formula = ll_mod::formula::FormulaTable::new();
    let mut weapon_category = ll_mod::weapon_category::WeaponCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut damage_category = ll_mod::damage_category::DamageCategoryTable::new();
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
            xp_curve_bindings: &mut bindings,
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
        },
    );
    // mod 自身在 LoadReport 里的标识按 `ll_mod::manifest::mod_self_id`
    // 的既有约定包装成 "<namespace>:self"（该函数是 crate 私有的，这里
    // 按其文档记录的约定原样构造，不需要它对外公开）。
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
        linear_curve_id: resolve("examplemod:linear_xp_curve"),
        recursive_curve_id: resolve("examplemod:recursive_xp_curve"),
        goblin_id: resolve("examplemod:goblin"),
        necromancer_class_id: resolve("examplemod:necromancer"),
        half_elf_race_id: resolve("examplemod:half_elf"),
        report,
        race,
        xp_curve,
        bindings,
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
fn 真实注册的哥布林种族击杀经验值为mod脚本声明的十五点() {
    // Arrange
    let handle = load_real_mods_and_resolve();

    // Act
    let reward = handle.race.xp_reward_for(handle.goblin_id);

    // Assert
    assert_eq!(reward, 15);
}

#[test]
fn 真实注册的两条经验曲线在同一等级上门槛确实不同() {
    // 直接验收设计文档四节的论证：线性曲线与递推指数曲线不是同一套
    // 公式调了系数，是两种不同的数学结构——这里不用手算的 Rust 字面量
    // 曲线（那已经在 `ll_sim::xp_curve` 的单元测试里验证过求值器本身
    // 正确），而是从 mod 脚本真实注册、真实编译出来的指令数组求值，
    // 证明「不同公式」这件事在端到端链路上也成立，不是只在孤立测试里
    // 自证。
    // Arrange
    let handle = load_real_mods_and_resolve();
    let linear = handle
        .xp_curve
        .get(handle.linear_curve_id)
        .expect("线性曲线应当已被注册");
    let recursive = handle
        .xp_curve
        .get(handle.recursive_curve_id)
        .expect("递推曲线应当已被注册");

    // Act：都在「15 级」这一点求值，输入取各自设计文档手算表已经验证
    // 过的上一级门槛（战士 15→16 用 0，因为不读 prev-requirement；
    // 法师 15→16 用手算表「14→15」行算出的 855）。
    let linear_at_15 = eval_xp_curve(linear, 15, 0);
    let recursive_at_15 = eval_xp_curve(recursive, 15, 855);

    // Assert：与设计文档四节论证②完全一致的数字——法师（1008）反超
    // 战士（700），即便法师起点（种子值 80）远低于战士（140）。
    assert_ne!(linear_at_15, recursive_at_15);
    assert_eq!(linear_at_15, 700);
    assert_eq!(recursive_at_15, 1008);
}

#[test]
fn 真实注册的职业绑定与种族绑定各自指向mod脚本声明的曲线() {
    // Arrange
    let handle = load_real_mods_and_resolve();

    // Act
    let class_bound = handle.bindings.class_curve(handle.necromancer_class_id);
    let race_bound = handle.bindings.race_curve(handle.half_elf_race_id);

    // Assert：亡灵法师职业绑定到递推曲线，半精灵种族绑定到线性曲线
    // ——与 gameplay.scm 里两行 register-class-xp-curve/
    // register-race-xp-curve 的真实参数一一对应。
    assert_eq!(class_bound, Some(handle.recursive_curve_id));
    assert_eq!(race_bound, Some(handle.linear_curve_id));
}
