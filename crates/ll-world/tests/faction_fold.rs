//! 势力播种的**折叠规则**判据：拿手工拼的事件流逐条钉住
//! [`ll_world::faction::seed_factions`] 的三条规则与
//! [`ll_world::faction::FactionTable`] 的构造校验。
//!
//! # 为什么这些测试住在 `tests/` 而不是模块内的 `#[cfg(test)] mod tests`
//!
//! 本仓库的默认惯例是把单元测试放在模块内，但那会让 `faction.rs` 越过
//! 800 行的文件上限——而 `chronicle.rs`/`state.rs` 两处既有违规正是这样
//! 长起来的。被测的全部条目（`seed_factions`、`FactionTable::rebuild`、
//! `FactionTableError`、`Faction`、`FactionStatus`）都是 `pub`，从
//! 集成测试这一侧一个都够得着，因此这次搬出去**没有损失任何覆盖面**。
//!
//! 与同目录的 `faction_seeding.rs` 分工明确：那一份问「规则接到**真**
//! 编年史上之后还成立吗」，本文件问「规则本身对不对」。

use ll_core::ident::{ContentIndex, WorldId};
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};
use ll_world::entity::OrgInstance;
use ll_world::faction::{Faction, FactionStatus, FactionTable, FactionTableError, seed_factions};
use ll_world::history::{
    HistoricalEvent, HistoricalEventKind, SettlementAbandonedRecord, SettlementConqueredRecord,
    SettlementDemise, SettlementFoundedRecord,
};

/// 造一个指定号码的 [`WorldId`]——类型只开放「从计数器分配下一个」
/// 这一条构造路径（`WorldId::next`），测试要精确指定号码就得自己
/// 起一个一次性计数器。
fn wid(raw: u32) -> WorldId {
    let mut counter = raw;
    WorldId::next(&mut counter)
}

fn anywhere() -> TorusPos {
    TorusSize::new(16, 16).expect("合法尺寸").wrap(0, 0)
}

fn envelope(id: u32, kind: HistoricalEventKind) -> HistoricalEvent {
    HistoricalEvent {
        id: wid(id),
        at: Tick(0),
        location: anywhere(),
        kind,
    }
}

fn founded(site: u32, epoch: u32) -> HistoricalEvent {
    envelope(
        9000 + site,
        HistoricalEventKind::SettlementFounded(SettlementFoundedRecord {
            site: wid(site),
            epoch,
            initial_population: 10,
            land_area: 400,
        }),
    )
}

fn conquered(site: u32, conqueror: u32, epoch: u32) -> HistoricalEvent {
    envelope(
        9500 + site,
        HistoricalEventKind::SettlementConquered(SettlementConqueredRecord {
            site: wid(site),
            epoch,
            conqueror: wid(conqueror),
            former_culture: ContentIndex::default(),
            new_culture: ContentIndex::default(),
            survivors: 7,
        }),
    )
}

fn abandoned(site: u32, epoch: u32) -> HistoricalEvent {
    envelope(
        9800 + site,
        HistoricalEventKind::SettlementAbandoned(SettlementAbandonedRecord {
            site: wid(site),
            epoch,
            peak_population: 30,
            epochs_inhabited: 3,
            cause: SettlementDemise::Plague { dead: 30 },
        }),
    )
}

#[test]
fn 每座建立的据点当场自立为一个势力() {
    // 裁定 1：一座从未打过仗的孤立据点也有势力（只有它自己的城邦），
    // 「无势力的活据点」不合法。
    // Arrange
    let events = vec![founded(1, 0), founded(2, 1), founded(3, 2)];
    let mut counter = 100u32;

    // Act
    let table = seed_factions(&events, &mut counter);

    // Assert
    assert_eq!(table.len(), 3, "三次建立应当立起三个势力");
    assert!(table.factions().iter().all(|f| f.members.len() == 1));
    assert!(table.faction_of(wid(2)).is_some(), "孤立据点也有归属");
}

#[test]
fn 占领把据点搬进征服者的势力从而一个势力下属多个据点() {
    // 本批的整个由来：一条占领链天然就是「一个势力下属多个据点」。
    // 反例验证：把 SettlementConquered 那一支改成 `continue`（无操作），
    // 本条立刻红——每个势力都只剩自己一座城。
    // Arrange
    let events = vec![
        founded(1, 0),
        founded(2, 0),
        founded(3, 0),
        conquered(2, 1, 3),
        conquered(3, 1, 5),
    ];
    let mut counter = 100u32;

    // Act
    let table = seed_factions(&events, &mut counter);

    // Assert
    let conqueror = table
        .get(table.faction_of(wid(1)).expect("征服者有势力"))
        .expect("势力表里查得到");
    assert_eq!(
        conqueror.members,
        vec![wid(1), wid(2), wid(3)],
        "占领链应当让一个势力下属三座据点"
    );
    assert_eq!(
        table.faction_of(wid(2)),
        Some(conqueror.id()),
        "被占的据点归属改指征服者的势力"
    );
}

#[test]
fn 一座据点只属于一个势力() {
    // 裁定 3：严格一对一。反例验证：让占领时**不**从旧势力移除
    // （去掉 remove_member 那一行），FactionTable::rebuild 立刻返回
    // SiteRuledTwice，本条与上一条同时红。
    // Arrange
    let events = vec![founded(1, 0), founded(2, 0), conquered(2, 1, 3)];
    let mut counter = 100u32;

    // Act
    let table = seed_factions(&events, &mut counter);

    // Assert
    let claiming: Vec<WorldId> = table
        .factions()
        .iter()
        .filter(|f| f.members.contains(&wid(2)))
        .map(|f| f.id())
        .collect();
    assert_eq!(claiming.len(), 1, "同一座据点被两个势力声称统治");
}

#[test]
fn 最后一座据点没了之后势力转为已覆灭而不是消失() {
    // 裁定 4：玩家加入的势力被灭了，归属仍然解析得到。反例验证：
    // 把 remove_member 里那一支改成保持 Active，本条红。
    // Arrange
    let events = vec![founded(1, 0), founded(2, 0), conquered(2, 1, 4)];
    let mut counter = 100u32;

    // Act
    let table = seed_factions(&events, &mut counter);

    // Assert
    let fallen = table
        .factions()
        .iter()
        .find(|f| !f.is_active())
        .expect("失去唯一一座据点的势力应当留下已覆灭的记录");
    assert_eq!(fallen.status, FactionStatus::Fallen { epoch: 4 });
    assert!(fallen.members.is_empty());
    assert_eq!(fallen.seat, wid(2), "覆灭不抹掉「起于何处」");
    assert!(
        table.get(fallen.id()).is_some(),
        "已覆灭的势力号仍然解析得到——OrgInstance::id 永不复用"
    );
}

#[test]
fn 据点被铲平同样让势力覆灭() {
    // 覆灭的另一条成因：不是被占走，是被铲平/瘟疫/资源枯竭。
    // Arrange
    let events = vec![founded(1, 0), abandoned(1, 6)];
    let mut counter = 100u32;

    // Act
    let table = seed_factions(&events, &mut counter);

    // Assert
    assert_eq!(
        table.factions()[0].status,
        FactionStatus::Fallen { epoch: 6 }
    );
    assert_eq!(table.faction_of(wid(1)), None, "废墟不归任何势力");
}

#[test]
fn 首邑丢了而还有别的据点时改指现存成员里号最小的那座() {
    // 确定性规则，不是「随便挑一个」。
    // Arrange：势力 A 建于据点 1，先占下据点 5 与据点 3，随后首邑被 9 占走。
    let events = vec![
        founded(1, 0),
        founded(3, 0),
        founded(5, 0),
        founded(9, 0),
        conquered(5, 1, 1),
        conquered(3, 1, 2),
        conquered(1, 9, 3),
    ];
    let mut counter = 100u32;

    // Act
    let table = seed_factions(&events, &mut counter);

    // Assert
    let a = table
        .factions()
        .iter()
        .find(|f| f.members.contains(&wid(3)))
        .expect("势力 A 还在");
    assert_eq!(a.seat, wid(3), "首邑应当改指现存成员里号最小的那座");
    assert!(a.is_active());
}

#[test]
fn 势力号与据点号不相交() {
    // 所有者否掉的那条变通「拿据点 WorldId 冒充势力」在号段层面就不
    // 可能发生。反例验证：把 org.id 改成直接复用 record.site，本条红。
    // Arrange：编年史的计数器已经发过 0..=50 号给据点与事件。
    let events = vec![founded(1, 0), founded(2, 0)];
    let mut counter = 51u32;

    // Act
    let table = seed_factions(&events, &mut counter);

    // Assert
    for faction in table.factions() {
        assert!(
            faction.id().get() >= 51,
            "势力号必须从编年史计数器继续分配，不能复用据点号"
        );
        assert!(!faction.members.contains(&faction.id()));
    }
    assert_eq!(counter, 53, "两个势力应当各吃掉一个号");
}

#[test]
fn 空事件流产出空表() {
    // 空文化表、零纪元推演、绝大多数单元测试的世界都走这条。
    // Arrange
    let mut counter = 7u32;

    // Act
    let table = seed_factions(&[], &mut counter);

    // Assert
    assert!(table.is_empty());
    assert_eq!(counter, 7, "没有势力就不该动计数器");
}

#[test]
fn 自家人占自家城不改变任何东西() {
    // from == to 的那一支：不 panic、不重复计数。
    // Arrange
    let events = vec![founded(1, 0), founded(2, 0), conquered(2, 1, 1)];
    let mut counter = 100u32;
    let before = seed_factions(&events, &mut counter);

    let mut with_self = events.clone();
    with_self.push(conquered(2, 1, 2));
    let mut counter = 100u32;

    // Act
    let after = seed_factions(&with_self, &mut counter);

    // Assert
    assert_eq!(
        before.factions(),
        after.factions(),
        "同一个势力「占领」自己的城不应当改变任何东西"
    );
}

#[test]
fn 构造校验拒绝一座据点归两个势力() {
    // rebuild 是唯一的有内容构造路径，读档也走它——手工拼出来的
    // 矛盾表必须当场被拒。反例验证：把 SiteRuledTwice 那一段删掉，
    // 本条红。
    // Arrange
    let make = |id: u32, members: Vec<WorldId>| Faction {
        org: OrgInstance {
            id: wid(id),
            def: None,
            authored: None,
        },
        seat: members[0],
        founded_epoch: 0,
        status: FactionStatus::Active,
        members,
    };

    // Act
    let built = FactionTable::rebuild(vec![make(1, vec![wid(10)]), make(2, vec![wid(10)])]);

    // Assert
    assert_eq!(
        built,
        Err(FactionTableError::SiteRuledTwice { site: wid(10) })
    );
}

#[test]
fn 构造校验拒绝活着却没有成员的势力() {
    // 存续状态与成员数是同一件事的两面，对不上就是一份坏数据。
    // Arrange
    let orphan = Faction {
        org: OrgInstance {
            id: wid(1),
            def: None,
            authored: None,
        },
        seat: wid(10),
        founded_epoch: 0,
        status: FactionStatus::Active,
        members: Vec::new(),
    };

    // Act
    let built = FactionTable::rebuild(vec![orphan]);

    // Assert
    assert_eq!(
        built,
        Err(FactionTableError::StatusMembersMismatch { faction: wid(1) })
    );
}

#[test]
fn 折叠是事件流的纯函数() {
    // 约束 C3/C5：同一份事件流跑两次逐字段相同，不依赖任何哈希容器
    // 迭代顺序，也不掷任何骰子。
    // Arrange
    let events = vec![
        founded(1, 0),
        founded(2, 0),
        founded(3, 1),
        conquered(2, 1, 2),
        abandoned(3, 3),
        conquered(1, 2, 4),
    ];

    // Act
    let mut first_counter = 100u32;
    let first = seed_factions(&events, &mut first_counter);
    let mut second_counter = 100u32;
    let second = seed_factions(&events, &mut second_counter);

    // Assert
    assert_eq!(first, second);
    assert_eq!(first_counter, second_counter);
}

#[test]
fn 势力表往返序列化后倒排索引重算得回来() {
    // 倒排索引不进存档（类型文档），读档时重算并跑完整校验。
    // Arrange
    let events = vec![founded(1, 0), founded(2, 0), conquered(2, 1, 3)];
    let mut counter = 100u32;
    let table = seed_factions(&events, &mut counter);

    // Act
    let bytes = postcard::to_allocvec(&table).expect("势力表可序列化");
    let restored: FactionTable = postcard::from_bytes(&bytes).expect("势力表可反序列化");

    // Assert
    assert_eq!(restored, table);
    assert_eq!(
        restored.faction_of(wid(2)),
        table.faction_of(wid(2)),
        "倒排索引应当在读档时被重算出来"
    );
}
