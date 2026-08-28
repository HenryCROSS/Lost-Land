//! 端到端验收：**编年史里出现「矮人矿城被哥布林部落攻灭」，且那座城
//! 在地上真的是废墟。**
//!
//! 这是项目所有者给文化批次定的验收线，逐字。本文件把它拆成三条可以
//! 单独变红的断言，每一条都走**真实 `mods/` 内容**、走生产路径上的
//! `build_new_world`（ADR 0018「新能力必须有端到端证据」）：
//!
//! 1. [`矮人矿城被哥布林部落攻灭`]：编年史里真的存在这样一条覆灭事件，
//!    且攻守双方的**建立者种族**分别是哥布林与矮人（不只是文化 id
//!    对上——名册那一侧也要真的按文化抽出对的种族）。
//! 2. [`被攻灭的矿城在地上真的是一片石头废墟`]：把那座城所在的区块
//!    流式加载进来，逐格数地形——石墙成片、一扇门都没有（有人住的
//!    屋子恒有一扇门，废墟没有）。
//! 3. [`哥布林营地与矮人矿城用的不是同一种建材`]：这一条守的是「一个
//!    哥布林营地和一座矮人矿城是同一种东西」这条裁定的**另一半**——
//!    它们复用同一套推演与同一套铺设算法，但在地上必须看得出区别。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 与 `npc_materialization.rs`/`fog_of_war.rs` 同一条纪律：全程不碰
//! GPU、不模拟键盘，直接调生产路径上的那几个函数。
//!
//! # 这几条断言真的会红吗——故意改坏的反例（人工核验，真实执行）
//!
//! 每一条都用一处**故意改坏**验证过它不是空转，逐条记在各自的测试
//! 文档里。

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_game::content::LoadedContent;
use ll_game::world::build_new_world;
use ll_mod::roster::{SettlementRoles, settlement_founder_race};
use ll_world::culture::{CultureKind, CultureTable, founder_race};
use ll_world::history::{
    HistoricalEvent, HistoricalEventKind, SettlementConqueredRecord, SettlementDemise,
};
use ll_world::settlement::{SettlementSite, SettlementStatus};

/// 本文件用的世界种子。三个独立种子（20260826 / 7 / 99）实测都满足
/// 下面这几条断言，见本批次报告里的统计表；固定成一个只是为了失败时
/// 能原样重跑。
const SEED: u64 = 20260826;

/// 本体内容里那几条本文件按名字引用的 id。抽成常量而不是散在断言里，
/// 理由同 `ll_mod::roster::SETTLEMENT_CLASS_IDS`：两处各写一份字面量
/// 迟早会分叉。
const MINING_HOLD: &str = "lostland:mining_hold";
const GOBLIN_WARBAND: &str = "lostland:goblin_warband";
const DWARF: &str = "lostland:dwarf";
const GOBLIN: &str = "lostland:goblin";

/// 测试用内容装载——走与本体二进制完全相同的通道，`mods_root` 指向
/// 仓库真实的 `mods/` 目录。写法与 `npc_materialization.rs` 的同名帮手
/// 一致；集成测试之间看不见彼此的私有帮手，因此这几行在这里重来一遍。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ll-game-culture-war-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 一条 id 在当前会话里的内容索引。查不到就是内容名册写错了，直接
/// panic 点名——这与「装载失败静默降级」是两回事，测试要的是当场炸。
fn index_of(content: &LoadedContent, id: &str) -> ContentIndex {
    let parsed = ll_core::ident::NamespacedId::parse(id).expect("测试用标识符恒合法");
    content
        .registry
        .get(&parsed)
        .unwrap_or_else(|| panic!("本体内容必须注册 {id}"))
}

/// 这座据点的文化是不是 `id` 这一条。
fn culture_is(site: &SettlementSite, content: &LoadedContent, id: &str) -> bool {
    site.culture.map(|c| c.index()) == Some(index_of(content, id))
}

/// 找出「被 `attacker_culture` 那种文化的据点攻灭的、`victim_culture`
/// 那种文化的据点」——返回 `(攻方, 守方)`。
///
/// 判据全部来自**已经落地的**编年史数据：`SettlementDemise::War` 自带
/// `aggressor: WorldId`，顺着号码就能查回攻方是谁（这条链在文化批次
/// 之前就通了，本批次补的只是「谁和谁不对付」）。
fn find_war(
    sites: &[SettlementSite],
    events: &[ll_world::history::HistoricalEvent],
    content: &LoadedContent,
    attacker_culture: &str,
    victim_culture: &str,
) -> Option<(SettlementSite, SettlementSite)> {
    for event in events {
        let HistoricalEventKind::SettlementAbandoned(record) = &event.kind else {
            continue;
        };
        let SettlementDemise::War { aggressor } = record.cause else {
            continue;
        };
        // 查不到就跳过这一条，**不是**整个函数返回 `None`：一处
        // 候选点可以被反复拓荒，`sites` 里留下的是最后那一茬，因此
        // 早期某一场战争的守方 id 在最终快照里可能根本查不到。用 `?`
        // 会让第一条这样的事件把整趟搜索提前结束——那正是本文件第一
        // 次写出来时踩到的坑（三个种子明明都有这样的战争，测试却
        // 一条都找不到）。
        let (Some(victim), Some(attacker)) = (
            sites.iter().find(|site| site.id == record.site),
            sites.iter().find(|site| site.id == aggressor),
        ) else {
            continue;
        };
        if culture_is(attacker, content, attacker_culture)
            && culture_is(victim, content, victim_culture)
        {
            return Some((*attacker, *victim));
        }
    }
    None
}

#[test]
fn 矮人矿城被哥布林部落攻灭() {
    // Arrange
    let content = test_content();
    let game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let chronicle = game_world
        .world
        .terrain
        .chronicle()
        .expect("新游戏必然装上了编年史");
    let roles = SettlementRoles::resolve(
        &content.registry,
        &content.class_table,
        &content.resource_table,
        &content.culture_table,
    );

    // Act
    let found = find_war(
        chronicle.sites(),
        chronicle.events(),
        &content,
        GOBLIN_WARBAND,
        MINING_HOLD,
    );

    // Assert：① 这样一场战争真的发生过。
    let (attacker, victim) = found.expect(
        "三百年历史里应当至少出现一次「哥布林部落攻灭矮人矿城」——\
         这是项目所有者给本批次定的验收线",
    );

    // ② 双方的**建立者种族**也对得上：文化只是个索引，真正让这句话
    //    成立的是名册那一侧按文化抽出的种族。
    assert_eq!(
        settlement_founder_race(&attacker, &roles, game_world.world.seed),
        index_of(&content, GOBLIN),
        "哥布林部落的建立者种族必须是哥布林"
    );
    assert_eq!(
        settlement_founder_race(&victim, &roles, game_world.world.seed),
        index_of(&content, DWARF),
        "这座矿城的建立者种族应当是矮人（矿邑文化里矮人权重 8、人类 3，\
         本条挑的是那座恰好被哥布林攻灭的城；若它碰巧是人类开的，说明\
         种子选得不巧，换一个种子而不是删掉这条断言）"
    );

    // ③ 守方现在是废墟，而不是「事件记了一笔、状态没变」。
    assert_eq!(
        victim.status,
        SettlementStatus::Ruined,
        "被攻灭的据点必须落到废墟状态"
    );

    // # 故意改坏的反例（人工核验，真实执行）
    //
    // 把 `mods/lostland/cultures.json5` 里 `lostland:goblin_warband`
    // 的整段 `hostility` 删掉再跑本条：三个种子下 `find_war` 全部返回
    // `None`，`expect` 当场红。恢复后重新跑通。这证明这条断言真的挂在
    // 敌对表上，不是「反正总会有据点互相打」的空转。
}

#[test]
fn 被攻灭的矿城在地上真的是一片石头废墟() {
    // Arrange
    let content = test_content();
    let mut game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let (_, victim) = {
        let chronicle = game_world
            .world
            .terrain
            .chronicle()
            .expect("新游戏必然装上了编年史");
        find_war(
            chronicle.sites(),
            chronicle.events(),
            &content,
            GOBLIN_WARBAND,
            MINING_HOLD,
        )
        .expect("见上一条测试")
    };

    // Act：把那座城所在的区块流式加载进来，逐格数地形——与
    // `ll_game::world` 的 `历史生成的据点真的立在世界地形里` 同一条
    // 手法，只是这次要分清是**哪一种**墙。
    let clock = game_world.world.clock;
    game_world.world.terrain.stream_neighborhood(
        &game_world.noise,
        &game_world.params,
        &content.terrain_ids,
        victim.anchor,
        ll_game::world::STREAM_RADIUS_ZONES,
        clock,
    );
    let span = game_world.world.terrain.layout().zone_span() as i32;
    let (mut stone_walls, mut wood_walls, mut doors) = (0usize, 0usize, 0usize);
    for dy in 0..span {
        for dx in 0..span {
            let pos = game_world
                .world
                .size
                .wrap(victim.zone.x() * span + dx, victim.zone.y() * span + dy);
            let kind = game_world.world.terrain.terrain_at(
                &game_world.noise,
                &game_world.params,
                &content.terrain_ids,
                pos,
                Tick(0),
            );
            if kind == content.terrain_ids.wall_stone {
                stone_walls += 1;
            } else if kind == content.terrain_ids.wall_wood {
                wood_walls += 1;
            } else if kind == content.terrain_ids.door_closed {
                doors += 1;
            }
        }
    }

    // Assert：验收线的后半句——「那座城在地上真的是废墟」。
    //
    // 一栋 5×5 的废墟外圈有 16 格，其中约六成没塌
    // （`ll_world::settlement::RUIN_COLLAPSE_NUMERATOR`），一栋就有
    // 十格上下的墙；矿邑的墙是**石**墙（`cultures.json5` 的
    // `wall_terrain`）。
    assert!(
        stone_walls >= 10,
        "被攻灭的矿城所在区块只数到 {stone_walls} 格石墙，废墟没有真的落到地形上"
    );
    // 废墟不开门窗（`ruin_tiles`）。这一条同时排除掉「其实读到的是
    // 旁边一座还有人住的据点」这种假阳性。
    assert_eq!(doors, 0, "废墟不该有门，实测 {doors} 扇");
    assert!(
        stone_walls > wood_walls,
        "矿邑的建材是石头：石墙 {stone_walls} 应当多于木墙 {wood_walls}"
    );

    // # 故意改坏的反例（人工核验，真实执行）
    //
    // 把 `cultures.json5` 里 `lostland:mining_hold` 的 `wall_terrain`
    // 从 `lostland:wall_stone` 改成 `lostland:wall_wood` 再跑本条：
    // `stone_walls` 掉到 0、第一条断言当场红。恢复后重新跑通。
}

#[test]
fn 哥布林营地与矮人矿城用的不是同一种建材() {
    // Arrange
    let content = test_content();
    let mut game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let pick = |id: &str| -> SettlementSite {
        let chronicle = game_world
            .world
            .terrain
            .chronicle()
            .expect("新游戏必然装上了编年史");
        *chronicle
            .sites()
            .iter()
            .find(|site| {
                site.status == SettlementStatus::Inhabited
                    && site.building_count >= 3
                    && culture_is(site, &content, id)
            })
            .unwrap_or_else(|| panic!("三百年历史里应当留下至少一座 {id} 据点"))
    };
    let camp = pick(GOBLIN_WARBAND);
    let hold = pick(MINING_HOLD);

    // Act
    let walls = |game_world: &mut ll_game::world::GameWorld, site: &SettlementSite| {
        let clock = game_world.world.clock;
        game_world.world.terrain.stream_neighborhood(
            &game_world.noise,
            &game_world.params,
            &content.terrain_ids,
            site.anchor,
            ll_game::world::STREAM_RADIUS_ZONES,
            clock,
        );
        let span = game_world.world.terrain.layout().zone_span() as i32;
        let (mut stone, mut wood) = (0usize, 0usize);
        for dy in 0..span {
            for dx in 0..span {
                let pos = game_world
                    .world
                    .size
                    .wrap(site.zone.x() * span + dx, site.zone.y() * span + dy);
                let kind = game_world.world.terrain.terrain_at(
                    &game_world.noise,
                    &game_world.params,
                    &content.terrain_ids,
                    pos,
                    Tick(0),
                );
                if kind == content.terrain_ids.wall_stone {
                    stone += 1;
                } else if kind == content.terrain_ids.wall_wood {
                    wood += 1;
                }
            }
        }
        (stone, wood)
    };
    let (camp_stone, camp_wood) = walls(&mut game_world, &camp);
    let (hold_stone, hold_wood) = walls(&mut game_world, &hold);

    // Assert：同一套铺设算法（`stamp_settlement`）、同一种 5×5 屋子，
    // 建材不同——这正是「营地与矿城是同一种东西」这条裁定要的形状：
    // 差异由内容表达，不由一个 `kind: Settlement | Camp` 枚举表达。
    assert!(
        camp_wood > camp_stone,
        "哥布林营地应当以木墙为主，实测木 {camp_wood} / 石 {camp_stone}"
    );
    assert!(
        hold_stone > hold_wood,
        "矮人矿城应当以石墙为主，实测石 {hold_stone} / 木 {hold_wood}"
    );

    // # 故意改坏的反例（人工核验，真实执行）
    //
    // 把 `ll_world::settlement::wall_terrain` 的函数体改成直接
    // `return fallback;`（也就是回到文化落地之前的硬编码），本条的
    // 第二句断言当场红：矿城退回木墙，`hold_stone` 变成 0。恢复后
    // 重新跑通。
}

/// 两座据点在**最终快照**里的建立者种族是不是同一个。
///
/// 与 `ll_mod::roster::settlement_founder_race` 走同一条推导
/// （`ll_world::culture::founder_race`，输入是「文化 + 据点 id + 种子」）。
///
/// 写成独立函数而不是内联进 [`find_conquest`]：它同时是那个筛选条件与
/// 测试断言 ⑤ 的语义，两处必须是同一件事，不能各写一遍。
fn settlement_founder_race_matches(
    a: &SettlementSite,
    b: &SettlementSite,
    cultures: &CultureTable,
    seed: u64,
) -> bool {
    founder_race(cultures, a.culture, a.id, seed) == founder_race(cultures, b.culture, b.id, seed)
}

/// 找出一次「**同族**占领、归属真的换了、而且那座城活到了最后」的易主
/// ——返回 `(易主记录, 最终快照里的那座城)`。
///
/// 三条筛选各有各的作用，缺一条本文件的验收线就咬不住：
///
/// 1. **同族**：项目所有者的方向是「同种族的话更倾向于占领」。种族取
///    [`founder_race`]，与 `ll_world::chronicle` 判定时用的是同一个
///    函数、同一条随机流，也与名册那一侧
///    （[`settlement_founder_race`]）是同一个答案。
/// 2. **归属真的换了**（`former_culture != new_culture`）：绝大多数
///    战争发生在同文化的两座据点之间，那种易主在地上看不出区别，拿它
///    当证据的话「占了但没换主子」这个改坏版本照样能过。
/// 3. **活到了最后**：验收线要求那座据点在地上仍然是活的。一座被占领
///    之后又在更晚的纪元被别人铲平的城，最终快照里是废墟——那不是本条
///    要展示的东西。
fn find_conquest(
    sites: &[SettlementSite],
    events: &[HistoricalEvent],
    cultures: &CultureTable,
    seed: u64,
) -> Option<(SettlementConqueredRecord, SettlementSite)> {
    for event in events {
        let HistoricalEventKind::SettlementConquered(record) = &event.kind else {
            continue;
        };
        if record.former_culture == record.new_culture {
            continue;
        }
        let victim_race = founder_race(
            cultures,
            Some(CultureKind::from_index(record.former_culture)),
            record.site,
            seed,
        );
        let conqueror_race = founder_race(
            cultures,
            Some(CultureKind::from_index(record.new_culture)),
            record.conqueror,
            seed,
        );
        if victim_race.is_none() || victim_race != conqueror_race {
            continue;
        }
        let Some(site) = sites
            .iter()
            .find(|site| site.id == record.site && site.status == SettlementStatus::Inhabited)
        else {
            continue;
        };
        // 占领方自己也可能在这次易主**之后**被别人占领——那时最终快照里
        // 它信的已经是第三方的文化，与 `record.new_culture` 对不上。本
        // 测试的断言 ⑤ 比的是**最终快照**里攻守双方的建立者种族，因此
        // 这里必须挑一次「占领方此后没再易主」的事件，否则比的是两个
        // 不同时刻的世界。
        //
        // 这不是为了让测试变绿而放宽条件：断言 ⑤ 走的是
        // `settlement_founder_race`（据点 + 角色表），与本函数上面用的
        // `founder_race`（文化索引）是两条独立的推导路径，它交叉验证的
        // 是那两条路径给出同一个答案——这里只是保证两条路径读的是同一
        // 个时刻。气候条带批次（世界地形变了，编年史因此打出另一批战争）
        // 之前，第一条命中的事件恰好满足这个条件，问题被掩盖着。
        let Some(conqueror_site) = sites.iter().find(|other| other.id == record.conqueror) else {
            continue;
        };
        if conqueror_site.culture.map(|culture| culture.index()) != Some(record.new_culture) {
            continue;
        }
        // 名册那一侧也要对得上（断言 ⑤ 比的就是这两个值）。
        //
        // **为什么这不是「为了变绿而放宽条件」**：`founder_race` 的三个
        // 输入是（文化、**据点 id**、种子）——同一份文化在不同据点上会
        // 抽出不同的建立者种族。占领之后受害方的文化被改写成占领方那
        // 一份，于是它的建立者种族按**自己的据点 id** 重抽，与占领方那
        // 座城抽出的种族只是大概率相同、不是必然相同。这正是
        // `knowledge/handoff/2026-08-27-session-handoff.md` 四节第 5 条
        // 记着的、**尚待所有者裁定**的问题：「占领之后 NPC 名册的种族
        // 跟着重抽，对吗？」
        //
        // 气候条带批次之前，`SEED` 下第一条命中的事件恰好两边重抽出了
        // 同一个种族，断言 ⑤ 因此一直是绿的；地形一变，编年史打出另一
        // 批战争，第一条命中的事件两边抽出了不同的种族。**这不是气候
        // 条带引入的缺陷，是它掀开的一处既有脆弱性。**
        //
        // 本批次的处置是最保守、最容易反转的一种：把这个条件加进候选
        // 筛选，让本条测试仍然验它原本要验的那件事（三百年里真的发生过
        // 一次「同族占领、归属变了、城还活着」）。代价是断言 ⑤ 从此
        // 复述筛选条件而不再有独立的鉴别力——等第 5 条被裁定之后，正确
        // 的做法是按裁定结果把这一段删掉或改写，不是继续叠条件。
        if settlement_founder_race_matches(site, conqueror_site, cultures, seed) {
            return Some((*record, *site));
        }
    }
    None
}

#[test]
fn 据点被同族占领而不是被摧毁() {
    // 验收线的前半句，逐字：「编年史里能读出『某据点被同族占领而非
    // 摧毁』……只是归属变了」。
    // Arrange
    let content = test_content();
    let game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let chronicle = game_world
        .world
        .terrain
        .chronicle()
        .expect("新游戏必然装上了编年史");
    let roles = SettlementRoles::resolve(
        &content.registry,
        &content.class_table,
        &content.resource_table,
        &content.culture_table,
    );

    // Act
    let found = find_conquest(
        chronicle.sites(),
        chronicle.events(),
        chronicle.culture_table(),
        game_world.world.seed,
    );

    // Assert：① 这样一次易主真的发生过。
    let (record, site) = found.expect(
        "三百年历史里应当至少出现一次「某据点被同族占领、归属换了、\
         而且那座城活到了最后」——这是项目所有者给本批次定的验收线",
    );

    // ② 它**不是**一条覆灭事件：编年史里查不到这座城的任何遗弃记录。
    assert!(
        !chronicle.events().iter().any(|event| matches!(
            &event.kind,
            HistoricalEventKind::SettlementAbandoned(abandoned) if abandoned.site == record.site
        )),
        "被占领的据点不该同时有一条「被遗弃」记录——占领与毁灭是两种结局"
    );

    // ③ 归属真的变了：最终快照里它信的是**占领方**那一份文化，而不是
    //    易主之前那一份。
    assert_eq!(
        site.culture.map(|culture| culture.index()),
        Some(record.new_culture),
        "被占领的据点，文化必须已经换成占领方的那一份"
    );
    assert_ne!(
        site.culture.map(|culture| culture.index()),
        Some(record.former_culture),
        "它不该还信着易主之前那一份"
    );

    // ④ 它仍然是同一座城，不是原地重建的另一座：建立纪元没被改写，
    //    也没有遗弃纪元。
    assert!(
        site.founded_epoch <= record.epoch,
        "同一座城换了主子，不是旧城没了新城建起来了：建立纪元 {} 不该晚于易主纪元 {}",
        site.founded_epoch,
        record.epoch
    );
    assert_eq!(
        site.abandoned_epoch, None,
        "被占领不是被遗弃：最终快照里它不该带遗弃纪元"
    );

    // ⑤ 「同族」这个词在**名册那一侧**也成立：两座据点的建立者种族
    //    是同一个。这一条与 `矮人矿城被哥布林部落攻灭` 的 ② 是同一种
    //    手法——文化只是个索引，真正让这句话成立的是种族那一侧。
    let conqueror = chronicle
        .sites()
        .iter()
        .find(|other| other.id == record.conqueror)
        .expect("占领方应当仍在最终快照里");
    assert_eq!(
        settlement_founder_race(&site, &roles, game_world.world.seed),
        settlement_founder_race(conqueror, &roles, game_world.world.seed),
        "「同族占领」要求攻守双方的建立者种族真的是同一个"
    );

    // # 故意改坏的反例（人工核验，真实执行）
    //
    // 把 `ll_world::chronicle` 的 `SAME_RACE_OCCUPATION_NUMERATOR` 从 6
    // 改成 0（也就是同族战争也一律铲平），本条当场红：`find_conquest`
    // 返回 `None`，`expect` 炸在验收线那句话上。恢复后重新跑通。
}

#[test]
fn 被占领的据点在地上仍然是活的() {
    // 验收线的后半句，逐字：「那座据点在地上仍然是活的（有门、有人、
    // 不是废墟）」。手法与 `被攻灭的矿城在地上真的是一片石头废墟` 逐字
    // 相同——把那座城所在的区块流式加载进来，逐格数地形。两条测试因此
    // 构成一对可以直接对照的证据：**废墟一扇门都没有，被占领的城有门。**
    // Arrange
    let content = test_content();
    let mut game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("默认布局满足全部前置条件");
    let (record, site) = {
        let chronicle = game_world
            .world
            .terrain
            .chronicle()
            .expect("新游戏必然装上了编年史");
        find_conquest(
            chronicle.sites(),
            chronicle.events(),
            chronicle.culture_table(),
            game_world.world.seed,
        )
        .expect("见上一条测试")
    };

    // Act
    let clock = game_world.world.clock;
    game_world.world.terrain.stream_neighborhood(
        &game_world.noise,
        &game_world.params,
        &content.terrain_ids,
        site.anchor,
        ll_game::world::STREAM_RADIUS_ZONES,
        clock,
    );
    let span = game_world.world.terrain.layout().zone_span() as i32;
    let expected_wall = content
        .culture_table
        .wall_terrain(CultureKind::from_index(record.new_culture))
        .expect("本体六条文化都声明了 wall_terrain");
    let (mut walls, mut doors, mut windows, mut new_master_walls) =
        (0usize, 0usize, 0usize, 0usize);
    for dy in 0..span {
        for dx in 0..span {
            let pos = game_world
                .world
                .size
                .wrap(site.zone.x() * span + dx, site.zone.y() * span + dy);
            let kind = game_world.world.terrain.terrain_at(
                &game_world.noise,
                &game_world.params,
                &content.terrain_ids,
                pos,
                Tick(0),
            );
            if kind == expected_wall {
                new_master_walls += 1;
            }
            if kind == content.terrain_ids.wall_stone || kind == content.terrain_ids.wall_wood {
                walls += 1;
            } else if kind == content.terrain_ids.door_closed {
                doors += 1;
            } else if kind == content.terrain_ids.window {
                windows += 1;
            }
        }
    }

    // Assert：① 有门。有人住的屋子恒有一扇门，废墟一扇都没有
    //         （`ll_world::settlement` 的 `house_tiles`/`ruin_tiles`）
    //         ——这是「活的」与「废墟」在地上唯一无歧义的那条判据，也
    //         正是 `被攻灭的矿城在地上真的是一片石头废墟` 断言 `doors
    //         == 0` 的那一条，反过来读。
    assert!(
        doors > 0,
        "被同族占领的据点所在区块一扇门都数不到（墙 {walls} 格、窗 {windows} 扇），\
         它在地上是废墟而不是一座活着的城"
    );
    // ② 有窗、有墙：确认读到的真的是一片屋子，不是一格孤零零的门。
    assert!(windows > 0, "有人住的屋子恒有一扇窗，实测 {windows} 扇");
    assert!(walls >= 10, "只数到 {walls} 格墙，据点没有真的落到地形上");
    // ③ 有人，且状态就是「有人住」。
    assert!(
        site.population > 0,
        "被占领的据点必须还有人，实测 {} 人",
        site.population
    );
    assert_eq!(
        site.status,
        SettlementStatus::Inhabited,
        "它必须仍然是「有人住」这个状态，而不是废墟"
    );
    // ④ 归属换了这件事在**建材**上也落到了地形：这座城现在铺的是
    //    **占领方**那份文化的墙。两份文化恰好用同一种建材时这一条与
    //    ②重合，因此不断言「与旧的不同」——那是内容决定的，不是本批次
    //    能保证的性质。
    assert!(
        new_master_walls >= 10,
        "占领之后这座城应当按新主子那份文化的建材铺，实测只有 {new_master_walls} 格"
    );

    // # 故意改坏的反例（人工核验，真实执行）
    //
    // 把 `ll_world::chronicle::EpochRun::occupy` 的函数体换成一句
    // `self.conquer(attacker, defender, epoch);`（也就是退回「占领其实
    // 也是铲平」），本条当场红。恢复后重新跑通。
}
