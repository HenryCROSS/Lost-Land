//! 势力播种的端到端判据：**一部真实推演出来的编年史，其占领链真的长出
//! 了「一个势力下属多个据点」。**
//!
//! 单元测试（`ll_world::faction` 模块内）用手工拼的事件流验证折叠规则；
//! 本文件问的是另一个问题——**规则接到真编年史上之后还成立吗**。这条区分
//! 与本仓库既有的「黑盒集成测试」惯例一致：单元测试证明算法对，集成测试
//! 证明它真的被接上了。

use ll_core::ident::{Interner, NamespacedId};
use ll_core::torus::TorusSize;
use ll_world::chronicle::{ChronicleInput, ChronicleParams, WorldChronicle};
use ll_world::culture::{CultureTable, base_culture_fixture};
use ll_world::faction::{Faction, FactionStatus, display_name_key, founder_race_of, seat_culture};
use ll_world::generate::GenParams;
use ll_world::history::HistoricalEventKind;
use ll_world::resource::base_resource_fixture;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 与 `ll_world::chronicle` 的内部测试布局同尺寸：16×16 个区块、区块边长
/// 48。这是本仓库跑一部「像那么回事」的历史时的既有量级。
fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(16, 16).expect("16x16 合法");
    ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
}

/// 跑一部真编年史：真地形、真资源表、真文化表（含互相敌对的两种文化，
/// 因此真的会打起来、真的会有占领）。
fn chronicle(seed: u64) -> (WorldChronicle, CultureTable) {
    let layout = test_layout();
    let params = GenParams {
        seed,
        ..GenParams::default()
    };
    let noise = ll_world::generate::build_zone_noise(&layout, &params).expect("布局合法");
    let (ids, table) = base_terrain_fixture();
    let mut interner = Interner::new();
    let (_kinds, resources) = base_resource_fixture(&mut interner, &ids);
    let metal_race = interner.intern(NamespacedId::parse("fixture:metalfolk").expect("合法"));
    let tribal_race = interner.intern(NamespacedId::parse("fixture:tribalfolk").expect("合法"));
    let (cultures, _culture_kinds) = base_culture_fixture(
        |raw| interner.intern(NamespacedId::parse(raw).expect("合法")),
        metal_race,
        tribal_race,
        ids.wall_stone,
        ids.wall_wood,
        ids.mountain,
        ids.grass,
    );
    let chronicle = WorldChronicle::generate(
        &ChronicleInput {
            layout: &layout,
            noise: &noise,
            params: &params,
            terrain_ids: &ids,
            terrain_table: &table,
            resources: &resources,
            cultures: &cultures,
        },
        ChronicleParams::default(),
    );
    (chronicle, cultures)
}

/// 这部编年史里发生过几次占领。
fn conquests(chronicle: &WorldChronicle) -> usize {
    chronicle
        .events()
        .iter()
        .filter(|event| matches!(event.kind, HistoricalEventKind::SettlementConquered(_)))
        .count()
}

/// 找一颗真的打出过占领的种子——战争本来就是少数派，逐颗试而不是钉死
/// 一个数字（钉死的那个数字会在任何一次推演判据改动之后静默失效）。
fn seed_with_conquest() -> (WorldChronicle, CultureTable) {
    for seed in 1u64..=40 {
        let (chronicle, cultures) = chronicle(seed);
        if conquests(&chronicle) > 0 {
            return (chronicle, cultures);
        }
    }
    panic!("四十颗种子里一次占领都没有——战争判据多半被改坏了");
}

#[test]
fn 真编年史的占领链长出下属多座据点的势力() {
    // 本批的整个由来（交接文档第〇之二第 3 条的后果一节）：
    // 「一条占领链天然就是『一个势力下属多个据点』」。这一条把那句话
    // 从设想变成可执行的断言。
    //
    // 反例验证（ADR 0018）：把 `seed_factions` 里 `SettlementConquered`
    // 那一支改成 `continue`，本条立刻红——每个势力都只剩自己一座城。
    // Arrange
    let (chronicle, _cultures) = seed_with_conquest();

    // Act
    let table = chronicle.factions();
    let biggest = table
        .factions()
        .iter()
        .map(|faction| faction.members.len())
        .max()
        .expect("有据点就有势力");

    // Assert
    assert!(!table.is_empty(), "跑出了据点却一个势力都没立");
    assert!(
        biggest >= 2,
        "发生过占领的世界里必须存在一个下属多座据点的势力，最大的却只有 {biggest} 座"
    );
}

#[test]
fn 每座活着的据点恰好归一个势力而废墟不归任何势力() {
    // 裁定 1 与裁定 3 合起来的端到端判据。
    // 反例验证：让占领时不从旧势力移除，`FactionTable::rebuild` 直接
    // 报 `SiteRuledTwice`，整部推演在 `seed_factions` 的 expect 处炸。
    // Arrange
    let (chronicle, _cultures) = seed_with_conquest();
    let table = chronicle.factions();

    // Act & Assert
    for site in chronicle.sites() {
        let owner = table.faction_of(site.id);
        match site.status {
            ll_world::settlement::SettlementStatus::Inhabited => {
                let owner = owner.expect("活着的据点必须有势力——「无势力」不合法");
                let faction = table.get(owner).expect("势力号必须解析得回来");
                assert!(faction.members.contains(&site.id));
                assert!(faction.is_active());
            }
            ll_world::settlement::SettlementStatus::Ruined => {
                assert_eq!(owner, None, "废墟不该归任何势力——它没有人");
            }
        }
    }
}

#[test]
fn 势力号与据点号在同一号段里互不相等() {
    // 所有者否掉的那条变通「拿据点的 WorldId 冒充势力」，在端到端这一层
    // 也要成立。
    // Arrange
    let (chronicle, _cultures) = seed_with_conquest();
    let mut site_ids: Vec<_> = chronicle.sites().iter().map(|site| site.id).collect();
    site_ids.sort_unstable();

    // Act & Assert
    for faction in chronicle.factions().factions() {
        assert!(
            site_ids.binary_search(&faction.id()).is_err(),
            "势力 {} 用了一个据点的号",
            faction.id().get()
        );
    }
    assert!(
        chronicle.next_world_id() > site_ids.len() as u32,
        "势力号必须从编年史计数器继续分配"
    );
}

#[test]
fn 势力的文化与建立者种族由首邑现算而不是另存一份副本() {
    // 裁定 2：身份不存副本。这一条同时是「势力真的能说出自己是谁」的
    // 端到端证据。
    // Arrange
    let (chronicle, cultures) = seed_with_conquest();
    let sites = chronicle.sites();
    let alive: Vec<&Faction> = chronicle
        .factions()
        .factions()
        .iter()
        .filter(|faction| faction.is_active())
        .collect();
    assert!(!alive.is_empty(), "应当还有活着的势力");

    // Act & Assert
    for faction in alive {
        let seat = sites
            .iter()
            .find(|site| site.id == faction.seat)
            .expect("活着的势力，首邑必在据点快照里");
        assert_eq!(
            seat_culture(faction, sites),
            seat.culture,
            "势力的文化必须逐字等于首邑的文化"
        );
        assert!(
            founder_race_of(faction, sites, &cultures, chronicle_seed()).is_some(),
            "有文化的首邑必然抽得出建立者种族"
        );
        assert!(
            display_name_key(faction, sites, &cultures).is_some(),
            "展示名走文化的 display_name_key，不落字面字符串"
        );
    }
}

#[test]
fn 覆灭的势力留下记录而不是消失() {
    // 裁定 4：玩家加入的势力被灭了，那条归属仍然解析得到。
    // Arrange
    let (chronicle, _cultures) = seed_with_conquest();
    let table = chronicle.factions();

    // Act
    let fallen: Vec<&Faction> = table
        .factions()
        .iter()
        .filter(|faction| !faction.is_active())
        .collect();

    // Assert：这部推演里必然有覆灭（有占领就意味着至少有一座城易主，
    // 而只有一座城的势力失去它就覆灭；即便不然，废墟也会产生覆灭）。
    assert!(
        !fallen.is_empty(),
        "一部有战争的历史里应当留下已覆灭的势力记录"
    );
    for faction in fallen {
        assert!(faction.members.is_empty());
        assert!(matches!(faction.status, FactionStatus::Fallen { .. }));
        assert!(
            table.get(faction.id()).is_some(),
            "已覆灭的势力号仍然解析得到——OrgInstance::id 永不复用"
        );
    }
}

#[test]
fn 同一颗种子两次推演产出逐字段相同的势力表() {
    // 约束 C3/C5 的端到端判据：势力播种是种子的纯函数。
    // Arrange & Act
    let (first, _) = chronicle(7);
    let (second, _) = chronicle(7);

    // Assert
    assert_eq!(first.factions().factions(), second.factions().factions());
    assert_eq!(first.next_world_id(), second.next_world_id());
}

/// [`seed_with_conquest`] 用的那颗种子——它扫的是 `1..=40`，这里只是把
/// 「拿哪颗种子跑的」这件事说清楚给 `founder_race_of` 用。
///
/// 建立者种族的随机流由 `(世界种子, FOUNDER_RACE_STREAM_ID, 据点号)` 完全
/// 决定；本测试只断言「抽得出来」，不断言抽到哪一族，因此传哪颗种子都
/// 成立——但仍然显式写出来，而不是随手塞一个 0。
fn chronicle_seed() -> u64 {
    for seed in 1u64..=40 {
        let (chronicle, _cultures) = chronicle(seed);
        if conquests(&chronicle) > 0 {
            return seed;
        }
    }
    panic!("四十颗种子里一次占领都没有——战争判据多半被改坏了");
}
