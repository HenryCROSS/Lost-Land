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
use ll_world::history::{HistoricalEventKind, SettlementDemise};
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
    let game_world = build_new_world(&content, SEED).expect("默认布局满足全部前置条件");
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
    let mut game_world = build_new_world(&content, SEED).expect("默认布局满足全部前置条件");
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
    let mut game_world = build_new_world(&content, SEED).expect("默认布局满足全部前置条件");
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
