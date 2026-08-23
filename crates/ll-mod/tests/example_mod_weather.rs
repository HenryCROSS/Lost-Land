//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-weather` 与 `register-space-profile` 这两个脚本 API 真的
//! 能被 `mods/example_mod/weather.scm` 调用，且注册出来的天气真的能被
//! `ll_world::weather::Weather::derive` 抽中、真的压暗
//! `ll_world::light::ambient_light_under` 算出的环境光——ADR 0018
//! 「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，本
//! 文件是那份证据，不能靠单元测试自证。
//!
//! # 为什么 `register-space-profile` 也在这里
//!
//! `register-space-profile` 落地时（空间层属性脚本注册批次）**没有留下
//! 任何已发货脚本的调用**——它是十六个注册函数里唯一一个只有单元测试、
//! 没有真实 mod 脚本证据的。`mods/example_mod/weather.scm` 顺手补上了
//! 那条调用（`examplemod:volcanic_cave`），本文件因此也顺手把那条证据
//! 钉住：两条断言合在一个文件里，是因为它们出自同一个脚本文件，拆成
//! 两个测试二进制只会让同一份 `load_all` 跑两遍。
//!
//! 与 `crates/ll-mod/tests/example_mod_traits.rs` 同一个理由独立成
//! 文件、同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」手法。

use std::path::Path;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::time::{TICKS_PER_DAY, Tick};
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
use ll_world::space_profile::SpaceProfileTable;
use ll_world::weather::{WEATHER_PERIOD_TICKS, Weather, WeatherTable};

/// 仓库根目录下的真实 `mods/` 路径。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的索引。
struct RealModsHandle {
    weather: WeatherTable,
    space_profile: SpaceProfileTable,
    ashfall_id: ContentIndex,
    volcanic_cave_id: ContentIndex,
}

fn load_real_mods() -> RealModsHandle {
    let mut registry = Registry::new();
    // 本体天气与本体空间层属性都先走生产注册路径，与
    // `ll_game::content::load_content` 的顺序一致——mod 脚本是往这两张
    // 已经非空的表里**追加**，这正是本测试要验的「同一条通道」。
    let (_base_weather_ids, mut weather) =
        ll_mod::base_weather::register_base_weathers(&mut registry)
            .expect("本体天气声明表内部一致");
    let (_base_space_ids, mut space_profile) =
        ll_mod::base_space_profile::register_base_space_profiles(&mut registry)
            .expect("本体空间层属性声明表内部一致");

    let mut terrain = ll_world::terrain::TerrainTable::new();
    let mut class = ClassTable::new();
    let mut skill = SkillTable::new();
    let mut subclass = SubclassTable::new();
    let mut quest = QuestTable::new();
    let mut race = RaceTable::new();
    let mut clip = ClipTable::new();
    let mut xp_curve = XpCurveTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut trait_def = TraitTable::new();
    let mut resource_pool = ll_mod::resource_pool::ResourcePoolTable::new();
    let mut item = ll_mod::item::ItemTable::new();
    let mut formula = ll_mod::formula::FormulaTable::new();
    let mut weapon_category = ll_mod::weapon_category::WeaponCategoryTable::new();
    let mut damage_category = ll_mod::damage_category::DamageCategoryTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut tag_table = ll_mod::tag::TagTable::new();
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
            weather: &mut weather,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            tag: &mut tag_table,
            events: &mut ll_mod::event::EventSubscriptionTable::new(),
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
            .unwrap_or_else(|| panic!("{id} 应当已经注册进 Registry"))
    };

    RealModsHandle {
        ashfall_id: resolve("examplemod:ashfall"),
        volcanic_cave_id: resolve("examplemod:volcanic_cave"),
        weather,
        space_profile,
    }
}

#[test]
fn 示例mod脚本注册的天气进了天气表且字段与声明一致() {
    // ADR 0018 的第一半：mod 脚本真的能注册天气，字段一个不落地进表。
    // Arrange
    let handle = load_real_mods();

    // Act & Assert
    assert!(
        handle.weather.is_defined(handle.ashfall_id),
        "examplemod:ashfall 应当被 WeatherTable 认领"
    );
    assert_eq!(handle.weather.light_scale(handle.ashfall_id), 550);
    assert_eq!(handle.weather.sight_scale(handle.ashfall_id), 400);
    assert_eq!(
        handle.weather.season_weights(handle.ashfall_id),
        [2, 8, 6, 0]
    );
    assert_eq!(
        handle.weather.display_name_key(handle.ashfall_id),
        Some(NamespacedId::parse("examplemod:weather.ashfall.display_name").unwrap())
    );
}

#[test]
fn 示例mod的天气与本体六种天气在同一张表同一段号段() {
    // 「本体即 Mod」：本体天气先注册，mod 天气紧随其后追加进同一张表，
    // 加权选取的注册顺序列表里两者并排，没有任何本体专属的分区。
    // Arrange
    let handle = load_real_mods();

    // Act
    let order = handle.weather.registered();

    // Assert：本体六种 + examplemod 一种。
    assert_eq!(order.len(), 7, "注册顺序列表：{order:?}");
    assert_eq!(
        order.last().copied(),
        Some(handle.ashfall_id),
        "mod 天气应当追加在本体六种之后"
    );
}

#[test]
fn 示例mod的天气在夏季会被真正抽中并压暗环境光() {
    // ADR 0018 的第二半，也是本文件真正的价值：注册进去只是写进了一张
    // 表，「真的会发生」需要 Weather::derive 抽得到它，「真的有影响」
    // 需要它算出来的环境光低于晴天。灰烬雨的夏季权重是全表最高之一
    // （8），扫过若干种子的整个夏季必然抽中。
    // Arrange
    let handle = load_real_mods();
    // 夏季是每年的第 30..60 天。
    let summer_start = 30 * TICKS_PER_DAY;
    let summer_periods = 30 * TICKS_PER_DAY / WEATHER_PERIOD_TICKS;

    // Act
    let mut ashfall_tick = None;
    for seed in 0..8u64 {
        for period in 0..summer_periods {
            let tick = Tick(summer_start + period * WEATHER_PERIOD_TICKS);
            if Weather::derive(seed, tick, &handle.weather).kind == Some(handle.ashfall_id) {
                ashfall_tick = Some((seed, tick));
                break;
            }
        }
        if ashfall_tick.is_some() {
            break;
        }
    }

    // Assert
    let (seed, tick) = ashfall_tick.expect("扫过八个种子的整个夏季都没抽中灰烬雨");
    let weather = Weather::derive(seed, tick, &handle.weather);
    let ashfall_light = ll_world::light::ambient_light_under(tick, weather);
    let clear_light = ll_world::light::ambient_light(tick);
    assert!(
        ashfall_light < clear_light,
        "灰烬雨（{}‰）必须真的压暗环境光：{ashfall_light:?} vs {clear_light:?}",
        weather.light_scale
    );
}

#[test]
fn 示例mod的天气在冬季权重为零因此绝不出现() {
    // 「季节倾向」这条配置维度在 mod 侧真的生效——weather.scm 把灰烬雨
    // 的冬季权重写成 0，扫过整个冬季一次都不该抽中。
    // Arrange
    let handle = load_real_mods();
    // 冬季是每年的第 90..120 天。
    let winter_start = 90 * TICKS_PER_DAY;
    let winter_periods = 30 * TICKS_PER_DAY / WEATHER_PERIOD_TICKS;

    // Act & Assert
    for seed in 0..8u64 {
        for period in 0..winter_periods {
            let tick = Tick(winter_start + period * WEATHER_PERIOD_TICKS);
            assert_ne!(
                Weather::derive(seed, tick, &handle.weather).kind,
                Some(handle.ashfall_id),
                "冬季权重为 0 的灰烬雨不该被抽中（种子 {seed}，周期 {period}）"
            );
        }
    }
}

#[test]
fn 示例mod脚本注册的空间层属性进了层属性表且不受天气影响() {
    // 补上 register-space-profile 一直缺的已发货脚本证据（见模块文档）。
    // 同时钉住「非露天空间不受天气影响」这条语义：火山洞窟的环境光恒
    // 等于它自己的地板值，外面下不下灰烬雨都一样。
    // Arrange
    let handle = load_real_mods();
    let index = handle.volcanic_cave_id;
    let profile = ll_world::space_profile::SpaceProfile {
        id: NamespacedId::parse("examplemod:volcanic_cave").unwrap(),
        ambient_light_floor: handle.space_profile.ambient_light_floor(index),
        exposed_to_sky: handle.space_profile.exposed_to_sky(index),
        base_temperature: handle.space_profile.base_temperature(index),
        diggable: handle.space_profile.diggable(index),
        buildable: handle.space_profile.buildable(index),
        reverb_tag: handle.space_profile.reverb_tag(index),
    };
    let ashfall = Weather {
        kind: Some(handle.ashfall_id),
        light_scale: handle.weather.light_scale(handle.ashfall_id),
        sight_scale: handle.weather.sight_scale(handle.ashfall_id),
        temperature_offset: handle.weather.temperature_offset(handle.ashfall_id),
    };

    // Act
    let noon = Tick(30 * TICKS_PER_DAY + TICKS_PER_DAY / 2);
    let clear_light =
        ll_world::space_profile::effective_ambient_light(&profile, noon, Weather::CLEAR);
    let ashfall_light = ll_world::space_profile::effective_ambient_light(&profile, noon, ashfall);

    // Assert：脚本声明的字段进了表……
    assert!(handle.space_profile.is_defined(index));
    assert!(!profile.exposed_to_sky);
    assert_eq!(profile.ambient_light_floor, 90);
    assert_eq!(profile.base_temperature, 480);
    assert!(profile.diggable);
    assert!(!profile.buildable);
    // ……且非露天空间的环境光恒为地板值，与天气无关。
    assert_eq!((clear_light.0, ashfall_light.0), (90, 90));
}
