//! 验收线：**加一份 `cultures.json5` 就有自己的城镇形态。**
//!
//! 这是「建筑类型由文化在内容里声明」这条设计的判据本身。本文件因此
//! 走的是**真实解析路径**——一段 `cultures.json5` 文本 →
//! `json5::from_str::<CultureFile>` → `apply_cultures` →
//! `ll_world::culture::CultureTable` → `ll_world::building::settlement_furnishing`
//! ——一步都不跳过、不手工构造中间结构。
//!
//! # 为什么不往 `mods/example_mod/` 里真加一份文化
//!
//! 那会更直白，但它同时**改变世界生成**：多一条文化就多一个候选，
//! `chronicle` 的 `pick_culture` 会给一部分据点抽中它，整张大陆的文化
//! 分布随之改变。那是一次与「建筑类型」无关的内容改动，会把本批次的
//! 下游影响测量搅浑（战争场次、存活据点都会动）。
//!
//! 本文件因此选了更窄的证法：**同一条解析路径、同一张表、同一个抽取
//! 函数**，只是那份 `cultures.json5` 由测试自己给出。它证明的正是那句
//! 判据——引擎侧一行 Rust 都不用改，换一份文化文本就换一种城镇形态。
//!
//! 第三条测试补上另一半：**仓库里真实的
//! `mods/lostland/cultures.json5`** 也遵守同一条规则（矿邑的城里立着
//! 锻炉，哥布林营地里一座都没有）。

use ll_core::ident::{ContentIndex, Interner, NamespacedId, WorldId};
use ll_core::torus::{TorusPos, TorusSize};
use ll_mod::content_schema_world::{CultureFile, apply_cultures};
use ll_mod::registry::Registry;
use ll_world::building::settlement_furnishing;
use ll_world::culture::{CultureKind, CultureTable};
use ll_world::settlement::{SITE_RESOURCE_SLOTS, SettlementSite, SettlementStatus};
use ll_world::zone::ZoneLayout;

/// 测量用的世界尺寸：一个 48×48 的区块就够——本文件只数「屋里摆了什么」，
/// 不关心据点落在哪一格。
fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 合法");
    ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
}

/// 一座据点：锚点落在区块正中，八十栋屋子（[`MAX_BUILDINGS`] 满额），
/// 信 `culture` 这条文化。
///
/// 满额是刻意的：建筑类型是**按权重逐栋抽**的，只铺三五栋的话「矿邑
/// 作坊多」这种分布差异会被抽样噪声淹没。
fn site_of(culture: CultureKind, id: u32) -> SettlementSite {
    let layout = test_layout();
    let anchor = layout.tile_size().wrap(24, 24);
    let mut counter = id - 1;
    SettlementSite {
        id: WorldId::next(&mut counter),
        zone: layout.tile_to_zone(anchor).0,
        anchor,
        status: SettlementStatus::Inhabited,
        founded_epoch: 0,
        abandoned_epoch: None,
        population: 60,
        peak_population: 60,
        building_count: ll_world::settlement::MAX_BUILDINGS,
        resource_profile: [None; SITE_RESOURCE_SLOTS],
        culture: Some(culture),
    }
}

/// 把一份 `cultures.json5` 文本经真实解析路径变成一张表。
fn load(source: &str) -> (Registry, CultureTable, Vec<CultureKind>) {
    let file: CultureFile = json5::from_str(source).expect("这份 cultures.json5 应当解析成功");
    let mut registry = Registry::new();
    let mut table = CultureTable::new();
    apply_cultures(&mut registry, &mut table, &file.cultures).expect("这份文化声明应当自洽");
    let order = table.registered().to_vec();
    (registry, table, order)
}

/// 数一数这座据点的家具构成：`(物品完整 id, 件数)`，按 id 排序。
///
/// 排序而不是原样返回：本文件比的是「摆了些什么」，不是「先摆哪一件」，
/// 而后者由内壁八格的行主序定死、另有专门的断言守着。
fn furniture_census(
    registry: &Registry,
    table: &CultureTable,
    culture: CultureKind,
    id: u32,
) -> Vec<(String, usize)> {
    let site = site_of(culture, id);
    let plan = settlement_furnishing(&site, table, 20260831, test_layout().tile_size());
    let mut ids: Vec<String> = plan
        .iter()
        .map(|p| {
            registry
                .resolve(p.item)
                .expect("家具索引由 apply_cultures intern 出来，必查得到")
                .to_string()
        })
        .collect();
    ids.sort();
    let mut census: Vec<(String, usize)> = Vec::new();
    for id in ids {
        match census.last_mut() {
            Some((last, count)) if *last == id => *count += 1,
            _ => census.push((id, 1)),
        }
    }
    census
}

/// 一份**第三方 mod 的** `cultures.json5`：两条文化，除了 `buildings`
/// 之外逐字段相同。
///
/// 逐字段相同是本条测试的全部要害：两座城唯一的区别只有「建筑类型的
/// 声明」，因此形态差异**只可能**来自那一处。
const THIRD_PARTY_CULTURES: &str = r#"{
  cultures: [
    // 陶工镇：只有一类屋子，摆四只桶。
    {
      id: "mymod:potters",
      display_name_key: "mymod:culture.potters.display_name",
      economy: "stone",
      home_terrain: "lostland:hill",
      wall_terrain: "lostland:wall_stone",
      founder_races: [ { race: "lostland:human", weight: 1 } ],
      buildings: [
        { weight: 1, furniture: [ { item: "mymod:clay_pot", count: 4 } ] },
      ],
    },
    // 书院：只有一类屋子，摆两面书柜加一把椅子。
    {
      id: "mymod:scribes",
      display_name_key: "mymod:culture.scribes.display_name",
      economy: "stone",
      home_terrain: "lostland:hill",
      wall_terrain: "lostland:wall_stone",
      founder_races: [ { race: "lostland:human", weight: 1 } ],
      buildings: [
        { weight: 1, furniture: [
          { item: "mymod:scroll_rack", count: 2 },
          { item: "mymod:reading_stool", count: 1 },
        ] },
      ],
    },
  ],
}"#;

#[test]
fn 加一份文化文本就有自己的城镇形态() {
    // Arrange
    let (registry, table, order) = load(THIRD_PARTY_CULTURES);
    assert_eq!(order.len(), 2, "这份文本声明了两条文化");

    // Act：两座据点除了信哪条文化之外**其余入参完全相同**（同一个锚点、
    // 同样八十栋屋子、同一颗种子）。
    let potters = furniture_census(&registry, &table, order[0], 1);
    let scribes = furniture_census(&registry, &table, order[1], 1);

    // Assert：两座城摆的东西完全不同，而且各自逐字对得上那份文本。
    assert_eq!(
        potters,
        vec![("mymod:clay_pot".to_string(), 4 * 80)],
        "陶工镇八十栋屋子各摆四只桶"
    );
    assert_eq!(
        scribes,
        vec![
            ("mymod:reading_stool".to_string(), 80),
            ("mymod:scroll_rack".to_string(), 2 * 80),
        ],
        "书院八十栋屋子各摆两面书柜加一把椅子"
    );
    assert_ne!(potters, scribes, "两份文化文本必须产出两种城镇形态");
}

#[test]
fn 家具件数超过一栋屋子摆得下的上限当场拒绝() {
    // Arrange：九件——正好比 MAX_FURNITURE_PER_BUILDING（8）多一件。
    let source = r#"{
  cultures: [
    {
      id: "mymod:hoarders",
      display_name_key: "mymod:culture.hoarders.display_name",
      economy: "stone",
      home_terrain: "lostland:hill",
      wall_terrain: "lostland:wall_stone",
      founder_races: [ { race: "lostland:human", weight: 1 } ],
      buildings: [
        { weight: 1, furniture: [ { item: "mymod:crate", count: 9 } ] },
      ],
    },
  ],
}"#;
    let file: CultureFile = json5::from_str(source).expect("解析成功");
    let mut registry = Registry::new();
    let mut table = CultureTable::new();

    // Act
    let result = apply_cultures(&mut registry, &mut table, &file.cultures);

    // Assert：装载期当场点名，不是静默丢掉第九件（ADR 0017）。
    let err = result.expect_err("九件家具摆不进一栋 5×5 的屋子，必须当场拒绝");
    assert!(
        err.contains('9')
            && err.contains(&ll_world::building::MAX_FURNITURE_PER_BUILDING.to_string()),
        "错误信息要说清声明了几件、上限是几件，实际是：{err}"
    );
}

#[test]
fn 一条建筑类型都不声明的文化当场拒绝() {
    // Arrange
    let source = r#"{
  cultures: [
    {
      id: "mymod:roofless",
      display_name_key: "mymod:culture.roofless.display_name",
      economy: "stone",
      home_terrain: "lostland:hill",
      wall_terrain: "lostland:wall_stone",
      founder_races: [ { race: "lostland:human", weight: 1 } ],
      buildings: [],
    },
  ],
}"#;
    let file: CultureFile = json5::from_str(source).expect("解析成功");
    let mut registry = Registry::new();
    let mut table = CultureTable::new();

    // Act & Assert
    let err = apply_cultures(&mut registry, &mut table, &file.cultures)
        .expect_err("没有任何建筑类型的文化必须当场拒绝");
    assert!(err.contains("建筑类型"), "错误信息应当点名建筑类型：{err}");
}

#[test]
fn 本体六条文化各自的城镇形态互不相同() {
    // Arrange：读仓库里**真实的**那一份，不抄一份副本进测试。
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/lostland/cultures.json5");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("读不到 {}：{err}", path.display()));
    let (registry, table, order) = load(&source);
    assert_eq!(order.len(), 6, "本体名册当前是六条文化");

    // Act：六条各铺一座满额据点。
    let censuses: Vec<Vec<(String, usize)>> = order
        .iter()
        .enumerate()
        .map(|(i, kind)| furniture_census(&registry, &table, *kind, i as u32 + 1))
        .collect();
    let names: Vec<String> = order
        .iter()
        .map(|k| registry.resolve(k.index()).expect("已注册").to_string())
        .collect();

    // Assert ①：六座城两两不同。
    for left in 0..censuses.len() {
        for right in (left + 1)..censuses.len() {
            assert_ne!(
                censuses[left], censuses[right],
                "{} 与 {} 的城镇形态完全相同——建筑类型没有起作用",
                names[left], names[right]
            );
        }
    }

    // Assert ②：矿邑的城里有锻炉，哥布林营地里一座都没有。
    // 这是「按类型填家具」最直白的一条可观测结果。
    let census_of = |id: &str| -> &Vec<(String, usize)> {
        let idx = names
            .iter()
            .position(|n| n == id)
            .expect("本体名册里有这条");
        &censuses[idx]
    };
    let count_of = |id: &str, item: &str| -> usize {
        census_of(id)
            .iter()
            .find(|(name, _)| name == item)
            .map(|(_, count)| *count)
            .unwrap_or(0)
    };
    assert!(
        count_of("lostland:mining_hold", "lostland:forge") > 0,
        "矿邑靠打铁吃饭，城里必须立着锻炉"
    );
    assert_eq!(
        count_of("lostland:goblin_warband", "lostland:forge"),
        0,
        "哥布林不打铁，营地里不该有锻炉"
    );
    assert_eq!(
        count_of("lostland:goblin_warband", "lostland:oak_bookshelf"),
        0,
        "书柜是识字人家的东西，部落营地里不该有"
    );
    assert!(
        count_of("lostland:farmstead", "lostland:fur_bed") > 0,
        "农庄以住宅为主，卧铺必须出现"
    );
}

#[test]
fn 同一份声明抽两次逐件相同() {
    // 确定性（约束 C3）：建筑类型抽取走 DetRng::for_entity，同一座据点
    // 抽两次必须逐件相同——否则「读档后世界不一样」。
    // Arrange
    let (registry, table, order) = load(THIRD_PARTY_CULTURES);

    // Act
    let first = furniture_census(&registry, &table, order[0], 3);
    let second = furniture_census(&registry, &table, order[0], 3);

    // Assert
    assert_eq!(first, second);
}

#[test]
fn 废墟不摆家具() {
    // Arrange
    let (_registry, table, order) = load(THIRD_PARTY_CULTURES);
    let mut site = site_of(order[0], 7);
    site.status = SettlementStatus::Ruined;

    // Act
    let plan = settlement_furnishing(&site, &table, 20260831, test_layout().tile_size());

    // Assert：没人住的地方没有人的东西。
    assert!(
        plan.is_empty(),
        "废墟里不该摆家具，实际摆了 {} 件",
        plan.len()
    );
}

#[test]
fn 空文化表下一件家具都不摆() {
    // 这条是黄金基准「把改动关掉」那一步依赖的性质：一条文化都没装载的
    // 世界里，本批次一个字节都不改变。
    // Arrange
    let empty = CultureTable::new();
    let mut interner = Interner::new();
    let index = interner.intern(NamespacedId::parse("test:nobody").expect("合法"));
    let site = site_of(CultureKind::from_index(index), 9);

    // Act
    let plan = settlement_furnishing(&site, &empty, 20260831, test_layout().tile_size());

    // Assert
    assert!(plan.is_empty());
    let _: ContentIndex = index;
}

/// 内壁八格的行主序是定死的：第一件家具恒落在左上那一格
/// （外廓局部坐标 `(1,1)`），正中那一格恒空。
#[test]
fn 每栋屋子的正中恒留空() {
    // Arrange
    let (_registry, table, order) = load(THIRD_PARTY_CULTURES);
    // 陶工镇每栋摆四件，占内壁前四格。
    let site = site_of(order[0], 11);
    let tile_size = test_layout().tile_size();

    // Act
    let plan = settlement_furnishing(&site, &table, 20260831, tile_size);

    // Assert：每栋屋子的正中格都不在计划里。
    let span = ll_world::settlement::BUILDING_SPAN;
    let mid = span / 2;
    for building in 0..site.building_count {
        let (left, top) = ll_world::settlement::building_origin(&site, building);
        let centre: TorusPos = tile_size.wrap(left + mid, top + mid);
        assert!(
            !plan.iter().any(|p| p.pos == centre),
            "第 {building} 栋屋子的正中被家具占了"
        );
    }
}
