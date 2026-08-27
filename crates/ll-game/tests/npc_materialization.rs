//! 端到端验收：据点里真的有 NPC，而且不会被重复生成一批。
//!
//! 这条链路此前**整条不存在**：`ll-game` 全 crate 只有 `spawn_player`
//! 一处 `world.actors.spawn`（见 `ll_game::app` 模块里那段已被本批次
//! 取代的 `no_npc_ai` 文档）。本文件验的是三件事，每一件此前都无从验起：
//!
//! 1. 走近一座还有人住的据点，那座据点的名册真的被物化成 `Agent`；
//! 2. 同一座据点**不会**因为再跑一遍物化而多出一批人（本批次要解决的
//!    那个缺陷：区块淘汰再加载不能复活玩家杀掉的人）；
//! 3. 物化出来的 `Agent` 真的被既有回合引擎与既有行为树驱动
//!    （`TurnEngine::advance_ai` + `ll_mod::native_behavior`）。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 与 `fog_of_war.rs` 同一条纪律：全程不碰 GPU、不模拟键盘，直接调用
//! 生产路径上的那几个函数（`build_new_world`、
//! `materialize_nearby_settlements`、`TurnEngine::advance_ai`），只是
//! 跳过了它们外面那层窗口/输入外壳。

use ll_game::content::{LoadedContent, RuntimeCatalogs};
use ll_game::world::{build_new_world, materialize_nearby_settlements};
use ll_mod::roster::{MAX_ROSTER, SettlementRoles, settlement_roster};
use ll_sim::behavior::behavior_ai_intent;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_world::settlement::SettlementStatus;

/// 本文件用的世界种子——固定值，让下面几条断言里的「这个世界有还有人
/// 住的据点」这条前提可复现。任何种子都该满足它（编年史在全世界铺出
/// 两百多座据点，见 `ll_world::chronicle::ChronicleParams::default`
/// 文档的实测表），选一个固定值只是为了失败时能原样重跑。
const SEED: u64 = 20260826;

/// 测试用内容装载——走与本体二进制完全相同的通道，mods_root 指向仓库
/// 真实的 `mods/` 目录（本体内容住在那里，临时空目录下契约解析会正确
/// 地失败，见 `ll_mod::base_contract` 模块文档）。写法与
/// `fog_of_war.rs` 的同名帮手一致；集成测试之间看不见彼此的私有帮手，
/// 因此这几行在这里重来一遍。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ll-game-npc-materialization-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 建一局世界，把流式邻域挪到**第一座还有人住的据点**的锚点上，返回
/// `(世界, 内容, 角色表, 那座据点)`。
///
/// 「把邻域挪过去」而不是「让玩家走过去」：走过去要几百步真实结算，
/// 而本文件验的不是移动，是物化。`stream_neighborhood` 是生产路径上
/// 每帧都在调的那一个函数（`Demo::maintain_streaming`），直接对着据点
/// 锚点调一次，与玩家真的走到那里之后的常驻集合是同一个东西。
fn world_at_a_living_settlement() -> (
    ll_game::world::GameWorld,
    LoadedContent,
    SettlementRoles,
    ll_world::settlement::SettlementSite,
) {
    let content = test_content();
    let mut game_world = build_new_world(&content, SEED).expect("建世界应当成功");
    let roles = SettlementRoles::resolve(
        &content.registry,
        &content.class_table,
        &content.resource_table,
        &content.culture_table,
    );

    let site = {
        let chronicle = game_world
            .world
            .terrain
            .chronicle_handle()
            .expect("新游戏必然装了编年史");
        *chronicle
            .sites()
            .iter()
            .find(|site| site.status == SettlementStatus::Inhabited && site.population > 0)
            .expect("三百年历史必然留下至少一座还有人住的据点")
    };

    let clock = game_world.world.clock;
    game_world.world.terrain.stream_neighborhood(
        &game_world.noise,
        &game_world.params,
        &content.terrain_ids,
        site.anchor,
        ll_game::world::STREAM_RADIUS_ZONES,
        clock,
    );
    (game_world, content, roles, site)
}

#[test]
fn 走近一座还有人住的据点会物化出npc() {
    // Arrange
    let (mut game_world, content, roles, site) = world_at_a_living_settlement();
    let actors_before = game_world.world.actors.iter().count();

    // Act
    let spawned = materialize_nearby_settlements(&mut game_world.world, &content, &roles);

    // Assert
    assert!(
        !spawned.is_empty(),
        "第一座还有人住的据点（人口 {}）应当至少物化出一个 NPC",
        site.population
    );
    assert_eq!(
        game_world.world.actors.iter().count(),
        actors_before + spawned.len(),
        "新增实体数必须与返回的 id 数一致"
    );
    assert!(
        spawned.len() <= (MAX_ROSTER as usize) * 4,
        "常驻邻域里最多几座据点，物化总量应当有界，实测 {}",
        spawned.len()
    );
    assert!(
        game_world.world.settlement_is_materialized(site.id),
        "物化过的据点必须被记进偏差表，否则下次会再生成一批"
    );
}

#[test]
fn 物化出来的npc全部带着据点职业而不是占位索引() {
    // Arrange
    let (mut game_world, content, roles, _site) = world_at_a_living_settlement();

    // Act
    let spawned = materialize_nearby_settlements(&mut game_world.world, &content, &roles);

    // Assert：本体十条职业内容全部注册成功时，名册抽出来的每一个职业
    // 都应当是一条**真实存在**的职业，而不是 `ContentIndex::default()`
    // 那个「尚无职业」的占位——占位大量出现意味着
    // `SettlementRoles::resolve` 没查到内容，那是一条静默失效。
    for id in &spawned {
        let agent = game_world.world.actors.get(*id).expect("刚生成必然存在");
        assert!(
            content.class_table.is_defined(agent.profession),
            "物化出的 NPC 必须带一条真实注册过的职业"
        );
        assert!(
            content.race_table.get(agent.race).is_some(),
            "物化出的 NPC 必须带一条真实注册过的种族"
        );
    }
    // 守卫职业（`lostland:guard`）此前是一条悬空引用：没有任何路径生成
    // 过带这个职业的实体，`native_behavior` 的卫兵那棵树第一句因此恒为
    // 假。这条断言是那条引用第一次真的被接上的证据。
    let guards = spawned
        .iter()
        .filter(|id| {
            let agent = game_world.world.actors.get(**id).expect("刚生成必然存在");
            Some(agent.profession) == roles.guard
        })
        .count();
    assert!(guards >= 1, "每座还有人住的据点至少配一名守卫");
}

#[test]
fn 同一座据点再跑一遍物化不会多出任何人() {
    // 这正是本批次要解决的那个缺陷：区块被淘汰再加载时，不能照着同一份
    // 名册再生成一批，把玩家杀掉的人原样复活。
    // Arrange
    let (mut game_world, content, roles, _site) = world_at_a_living_settlement();
    let first = materialize_nearby_settlements(&mut game_world.world, &content, &roles);
    assert!(!first.is_empty(), "第一次必须真的生成了人");
    let after_first = game_world.world.actors.iter().count();
    let hash_after_first = game_world.world.hash();

    // Act：原样再跑一遍——模拟「区块被淘汰又加载回来」之后的那一帧。
    let second = materialize_nearby_settlements(&mut game_world.world, &content, &roles);

    // Assert
    assert!(second.is_empty(), "第二次不该再生成任何人，实测 {second:?}");
    assert_eq!(game_world.world.actors.iter().count(), after_first);
    assert_eq!(
        game_world.world.hash(),
        hash_after_first,
        "第二次物化不该改变世界的任何一位"
    );
}

#[test]
fn 同一颗种子两局世界物化出逐位相同的名册() {
    // Arrange & Act
    let rosters: Vec<Vec<(u32, u32)>> = (0..2)
        .map(|_| {
            let (mut game_world, content, roles, _site) = world_at_a_living_settlement();
            let spawned = materialize_nearby_settlements(&mut game_world.world, &content, &roles);
            spawned
                .iter()
                .map(|id| {
                    let agent = game_world.world.actors.get(*id).expect("刚生成必然存在");
                    (agent.profession.get(), agent.race.get())
                })
                .collect()
        })
        .collect();

    // Assert
    assert!(!rosters[0].is_empty());
    assert_eq!(rosters[0], rosters[1]);
}

#[test]
fn 未探索区域的据点不物化任何实体但名册随时算得出来() {
    // 「NPC 在未探索区域也能正常运作」这条裁定的落点：他们**根本不需要
    // 实体化**——世界状态里一个字节都不占，而「那座村子里有几个铁匠」
    // 这类问题随时可以由派生函数当场回答。
    // Arrange
    let content = test_content();
    let game_world = build_new_world(&content, SEED).expect("建世界应当成功");
    let roles = SettlementRoles::resolve(
        &content.registry,
        &content.class_table,
        &content.resource_table,
        &content.culture_table,
    );
    let chronicle = game_world
        .world
        .terrain
        .chronicle_handle()
        .expect("新游戏必然装了编年史");

    // Act：**完全不做任何物化**，直接对全世界每一座据点派生名册。
    let living: Vec<_> = chronicle
        .sites()
        .iter()
        .filter(|site| site.status == SettlementStatus::Inhabited && site.population > 0)
        .collect();
    let total: usize = living
        .iter()
        .map(|site| settlement_roster(site, &roles, game_world.world.seed).len())
        .sum();

    // Assert
    assert!(living.len() > 10, "一片大陆上应当有不止十座活着的据点");
    assert!(total > 0, "派生名册必须算得出人来");
    assert_eq!(
        game_world.world.actors.iter().count(),
        1,
        "建局之后世界里应当只有玩家一个实体——未探索区域的 NPC 不占任何世界状态"
    );
    assert!(
        game_world.world.materialized_settlements.is_empty(),
        "一座都没走近过，偏差表应当是空的"
    );
}

#[test]
fn 物化出的npc真的被既有回合引擎与行为树驱动() {
    // Arrange
    let (mut game_world, content, roles, _site) = world_at_a_living_settlement();
    let spawned = materialize_nearby_settlements(&mut game_world.world, &content, &roles);
    assert!(!spawned.is_empty());

    // 时间轴：NPC 全部排在当前时刻，玩家排在很远的将来——`advance_ai`
    // 一遇到受控实体就返回（见其文档），把玩家排远是让这一次调用真的
    // 走完全部 NPC 的最直接办法，不需要模拟任何输入。
    let clock = game_world.world.clock;
    let mut timeline = Timeline::new();
    for actor in &spawned {
        timeline.schedule(*actor, clock);
    }
    timeline.schedule(game_world.player, ll_core::time::Tick(clock.0 + 100_000));
    let mut engine = TurnEngine::new(timeline);

    let mut source = ll_mod::native_behavior::NativeBehaviorSource::new(
        ll_mod::native_behavior::NativeBehaviorTree::guard(&content.registry),
        ll_mod::native_behavior::BehaviorRuleCatalogs::snapshot(
            &content.race_table,
            &content.class_table,
            &content.subclass_table,
            &content.trait_table,
            &content.item_table,
        ),
        game_world.world.seed,
    );
    let runtime = RuntimeCatalogs::new(&content);
    let catalogs = runtime.as_resolve_catalogs();
    let mut on_effect = |_world: &ll_world::state::WorldState, _effect: &ll_sim::effect::Effect| {};

    // Act
    let mut ai_intent = behavior_ai_intent(&mut source);
    let acted = engine.advance_ai(
        &mut game_world.world,
        game_world.player,
        &mut ai_intent,
        &catalogs,
        &mut on_effect,
    );
    drop(ai_intent);

    // Assert
    assert!(
        !acted.is_empty(),
        "排在玩家之前的 NPC 必须真的被驱动了至少一次"
    );
    for actor in &acted {
        let agent = game_world
            .world
            .actors
            .get(*actor)
            .expect("行动过的实体仍然活着");
        assert!(
            agent.next_action_at.0 > clock.0,
            "每个行动过的实体必须真的被重新排进了将来，否则时间轴会在同一刻反复弹出它"
        );
    }
}
