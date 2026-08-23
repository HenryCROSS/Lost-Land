//! 端到端验证：暗视这条链路从**真实 `mods/` 目录**一路通到视野半径。
//!
//! # 这份测试为什么必须存在
//!
//! 暗视此前的形态（`darkvision_floor`，光照千分比下限）在本作的量纲下
//! **永远不可能生效**：本体矮人声明的是 4，而午夜环境光是 100
//! （`ll_core::light::MIDNIGHT_LIGHT`），最暗的冬夜下雨也还有约 52，
//! `max(52, 4)` 恒等于 52。当时两道门禁都抓不到——字段有内容赋值 ✓、
//! 字段有决策层读取 ✓——因为它是「机制对、数值错」。既有的单元测试
//! 同样抓不到：`ll_game::layout` 的夹具当时必须写成
//! `FixedDarkvision(600)`，把本体数值放大 150 倍才测得出差异，那条
//! 测试因此只证明了「函数里的 `max` 生效」，没证明「本体矮人在夜里
//! 真的看得更远」。
//!
//! 本文件补的正是那一条：**经真实 `mods/` 装载 + 真实种族内容**，问
//! 「矮人夜里到底看得见几格」。任何只在单元测试里造夹具的验收都不算数
//! ——旧形态在单元测试里一直是绿的。
//!
//! # 与 `base_mod_races.rs` 的分工
//!
//! 那份文件钉的是「字段值是多少」（`darkvision_cells == 7`），本文件
//! 钉的是「这个值经过 `sight_radius_for_race` 之后变成几格视野」。两者
//! 缺一不可：前者防止内容被无意改动，后者防止这个数字再一次被下游的
//! 某个下限整个吃掉。
//!
//! # 与 `example_mod_*.rs` 同一套手法
//!
//! 装载**整个** `mods/` 目录（不是只挑 `mods/lostland/`），理由同
//! `base_mod_races.rs` 模块文档：真实装载路径就是整目录装载。本文件
//! 也因此能同时覆盖 `mods/example_mod/` 里那个声明**低于默认值**的
//! 暗视（`examplemod:ooze`，2 格）——ADR 0018「玩法层内容必须能从 mod
//! 脚本注册，且要有真实 mod 脚本为证」。

use std::path::Path;

use ll_core::ident::NamespacedId;
use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR, Tick};
use ll_mod::base_weather::register_base_weathers;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::formula::FormulaTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::{RaceTable, resolve_base_races};
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::vision::sight_radius_for_race;
use ll_world::light::{DEFAULT_NIGHT_SIGHT_RADIUS, ambient_light_under};
use ll_world::terrain::TerrainTable;
use ll_world::weather::{BaseWeatherIds, Weather, WeatherTable};

/// 仓库根目录下的真实 `mods/` 路径，理由同 `base_mod_races.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 与 `ll_game::layout::BASE_SIGHT_RADIUS` 同值——本 crate 不依赖
/// `ll-game`（依赖方向不允许），复制那个取值并在此说明，理由同
/// `ll_world::light` 测试模块的同名常量。两者若哪天分叉，本文件的结论
/// 仍然对这套换算成立，只是不再直接代表实机画面。
const PLAYER_BASE_SIGHT_RADIUS: u32 = 12;

/// `mods/lostland/races.scm` 声明的矮人暗视格数。本文件断言的是「这个
/// 数字真的变成了这么多格视野」，不是「字段里存着这个数字」（后者由
/// `base_mod_races.rs` 钉住）。
const DWARF_CELLS: u32 = 7;

/// 同上，精灵。
const ELF_CELLS: u32 = 6;

/// `mods/example_mod/gameplay.scm` 声明的软泥怪暗视格数——**低于**
/// [`DEFAULT_NIGHT_SIGHT_RADIUS`]，见该文件对应注释。
const OOZE_CELLS: u32 = 2;

/// 装载真实 `mods/` 目录一次，返回注册表、种族表与天气表/句柄。
///
/// 本体天气在 mod 装载**之前**由 `register_base_weathers` 注册，与
/// `ll_game::content::load_content` 的生产顺序一致——本文件需要真实的
/// 雾/雪乘数，不能自己编两个数字，否则「恶劣天气吃不掉暗视」这条断言
/// 验的就不是本体真实内容。
fn load_real_mods() -> (Registry, RaceTable, BaseWeatherIds, WeatherTable) {
    let mut registry = Registry::new();
    let (weather_ids, mut weather_table) =
        register_base_weathers(&mut registry).expect("本体天气声明表内部一致，注册恒不失败");

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
            events: &mut ll_mod::event::EventSubscriptionTable::new(),
        },
    );

    let lostland_id = NamespacedId::parse("lostland:self").expect("合法标识符");
    let status = report
        .entries
        .iter()
        .find(|(id, _)| *id == lostland_id)
        .map(|(_, status)| status);
    assert_eq!(
        status,
        Some(&LoadStatus::Loaded),
        "本体内容 mod（mods/lostland/）必须成功加载，否则下面的断言毫无意义"
    );

    (registry, race, weather_ids, weather_table)
}

/// 按本体天气表里的真实乘数构造一个 [`Weather`]。
fn weather_of(table: &WeatherTable, index: ll_core::ident::ContentIndex) -> Weather {
    Weather {
        kind: Some(index),
        light_scale: table.light_scale(index),
        sight_scale: table.sight_scale(index),
        temperature_offset: 0,
    }
}

/// 夏季（第 30 天，季节缩放不折损）的午夜——一年里夜最亮的时候。
/// 断言选它而不是冬夜，是取**对暗视最不利**的采样点：夏夜光照最高，
/// 若连这里都能看出暗视的差别，更暗的时段只会差得更多。
fn summer_midnight() -> Tick {
    Tick(30 * TICKS_PER_DAY)
}

/// 同上，夏季正午——一年里最亮的时刻。
fn summer_noon() -> Tick {
    Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR)
}

#[test]
fn 夏夜矮人的视野严格大于人类() {
    // **这条断言在本批次之前是失败的**，那正是本批次的意义：旧公式下
    // 矮人声明的 4 是光照下限，抬不动午夜的 100，矮人与人类的夜间视野
    // 完全相同（都撞在 4 格这个下游下限上）。
    // Arrange
    let (registry, race, _weather_ids, _weather) = load_real_mods();
    let ids = resolve_base_races(&registry, &race).expect("本体 mod 装载后契约必须解析成功");
    let light = ambient_light_under(summer_midnight(), Weather::CLEAR);

    // Act
    let dwarf = sight_radius_for_race(
        PLAYER_BASE_SIGHT_RADIUS,
        light,
        Weather::CLEAR,
        ids.dwarf,
        &race,
    );
    let human = sight_radius_for_race(
        PLAYER_BASE_SIGHT_RADIUS,
        light,
        Weather::CLEAR,
        ids.human,
        &race,
    );
    let elf = sight_radius_for_race(
        PLAYER_BASE_SIGHT_RADIUS,
        light,
        Weather::CLEAR,
        ids.elf,
        &race,
    );

    // Assert
    assert!(
        dwarf > human,
        "夏夜矮人视野 {dwarf} 格并不比人类的 {human} 格远——暗视又一次被某个下限吃掉了"
    );
    assert_eq!(dwarf, DWARF_CELLS);
    assert_eq!(elf, ELF_CELLS);
    assert_eq!(human, DEFAULT_NIGHT_SIGHT_RADIUS);
}

#[test]
fn 恶劣天气不把暗视削回默认值() {
    // 守住「两处调用点都换成了暗视版本」：夜间下限在
    // `sight_radius_under_weather` 里被应用两次（`sight_radius_at`
    // 内部一次、天气乘数之后再一次）。只改前一处的话，雾（sight_scale
    // 700）与雪（800）会把矮人从 7 格削回默认的 4 格——暗视在最需要它
    // 的场合失效。
    // Arrange：本体全部六种天气的真实乘数。
    let (registry, race, weather_ids, weather_table) = load_real_mods();
    let ids = resolve_base_races(&registry, &race).expect("本体 mod 装载后契约必须解析成功");
    let harsh = [
        ("雾", weather_ids.fog),
        ("雪", weather_ids.snow),
        ("雨", weather_ids.rain),
        ("阴", weather_ids.overcast),
    ];

    // Act & Assert
    for (name, index) in harsh {
        let weather = weather_of(&weather_table, index);
        let light = ambient_light_under(summer_midnight(), weather);
        let dwarf =
            sight_radius_for_race(PLAYER_BASE_SIGHT_RADIUS, light, weather, ids.dwarf, &race);
        let human =
            sight_radius_for_race(PLAYER_BASE_SIGHT_RADIUS, light, weather, ids.human, &race);
        assert_eq!(
            dwarf, DWARF_CELLS,
            "{name}天把矮人的暗视从 {DWARF_CELLS} 格削到了 {dwarf} 格"
        );
        assert!(dwarf > human, "{name}天里矮人不再比人类看得远");
    }
}

#[test]
fn 正午矮人与人类视野相同() {
    // 暗视是暗处的能力，不是无条件加成：正午满光照下 12 格远高于任何
    // 一个种族声明的暗视格数，夜间下限根本不参与取值。
    // Arrange
    let (registry, race, _weather_ids, _weather) = load_real_mods();
    let ids = resolve_base_races(&registry, &race).expect("本体 mod 装载后契约必须解析成功");
    let light = ambient_light_under(summer_noon(), Weather::CLEAR);

    // Act
    let dwarf = sight_radius_for_race(
        PLAYER_BASE_SIGHT_RADIUS,
        light,
        Weather::CLEAR,
        ids.dwarf,
        &race,
    );
    let human = sight_radius_for_race(
        PLAYER_BASE_SIGHT_RADIUS,
        light,
        Weather::CLEAR,
        ids.human,
        &race,
    );

    // Assert
    assert_eq!(dwarf, human);
    assert_eq!(dwarf, PLAYER_BASE_SIGHT_RADIUS);
}

#[test]
fn 声明低于默认值的mod种族夜里真的更瞎() {
    // 「不能写成 `max(默认值, 声明值)`」这条语义在**已发货内容**上的
    // 唯一证据：`mods/example_mod/gameplay.scm` 的软泥怪声明 2 格，
    // 低于未声明时的默认 4 格。若换成 `max`，这个 2 会被默默抬回 4，
    // 「夜视比常人差」这一整类设定根本无法表达，而且写它的人得不到
    // 任何提示。
    // Arrange
    let (registry, race, weather_ids, weather_table) = load_real_mods();
    let base_ids = resolve_base_races(&registry, &race).expect("本体 mod 装载后契约必须解析成功");
    let ooze = registry
        .get(&NamespacedId::parse("examplemod:ooze").expect("合法标识符"))
        .expect("example_mod 应当已注册 ooze");
    let light = ambient_light_under(summer_midnight(), Weather::CLEAR);

    // Act
    let ooze_radius =
        sight_radius_for_race(PLAYER_BASE_SIGHT_RADIUS, light, Weather::CLEAR, ooze, &race);
    let human_radius = sight_radius_for_race(
        PLAYER_BASE_SIGHT_RADIUS,
        light,
        Weather::CLEAR,
        base_ids.human,
        &race,
    );
    // 雾里同样不得被抬回默认值——天气那一处的下限也必须认声明值。
    let fog = weather_of(&weather_table, weather_ids.fog);
    let foggy_light = ambient_light_under(summer_midnight(), fog);
    let ooze_in_fog =
        sight_radius_for_race(PLAYER_BASE_SIGHT_RADIUS, foggy_light, fog, ooze, &race);

    // Assert
    assert_eq!(ooze_radius, OOZE_CELLS);
    assert!(
        ooze_radius < human_radius,
        "软泥怪夜里的 {ooze_radius} 格没有低于人类的 {human_radius} 格——声明值被 max() 抬回默认值了"
    );
    assert_eq!(ooze_in_fog, OOZE_CELLS);
}
