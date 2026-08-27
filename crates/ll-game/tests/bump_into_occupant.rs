//! 端到端验收：**走向一个非敌对的 NPC 是换位置，不是砍他**。
//!
//! 项目所有者的裁定原话：
//!
//! > 当角色与NPC非敌对的时候，移动向NPC的位置时，是和NPC互换位置，
//! > 而敌对NPC则是对敌对NPC攻击。
//!
//! 敌对那一半此前就存在（`ll_sim::turn` 的撞格路由，传统 roguelike 的
//! 「撞人即攻击」手感），但它当时**无条件**成立：目的地站着任何一个
//! 活人就改判成攻击。于是在本体内容里——全世界的 NPC 都是农夫、铁匠、
//! 渔夫这类平民——朝一个村民走过去就是把他砍了。
//!
//! 本文件验的是这条裁定真的落在**能跑起来的那条路径**上：真实
//! `mods/` 内容 + `build_new_world` + `TurnEngine::try_player_intent`，
//! 与本体二进制 `ll_game::app::Demo::advance` 走的是同一串调用
//! （ADR 0018）。
//!
//! # 「敌对」现在怎么判定
//!
//! [`ll_sim::ai_query::declared_hostile`]：只有**已声明**的对立关系才
//! 算敌对，双方都没有任何势力归属时不敌对。本文件两条用例分别钉住这条
//! 判据的两端——而第一条用例里那两个实体的 `affiliations` 是空的，
//! 这**不是夹具偷懒**：`Agent::affiliations` 至今没有任何生产者，本体
//! 里每一个 NPC 与玩家都是这个形态，见
//! `ll_world::entity::affiliation` 模块文档。
//!
//! # 互换只对玩家开——NPC 撞上非敌对目标是一次失败的移动
//!
//! 项目所有者随后补了一条裁定：**玩家优先度高于 NPC，只有玩家可以
//! 互换位置**，推翻了上一批「角色与NPC 双向都成立」的读法。互换是一次
//! 让路，被换的一方没有做出任何决定却被挪走了，而「谁有资格要求别人
//! 给自己让路」在本作里不是对称的。
//!
//! 本文件因此有三条用例，不是两条：玩家撞非敌对（互换）、玩家撞敌对
//! （攻击）、**NPC 撞非敌对（挪不动，但仍消耗一次行动）**。第三条同时
//! 是「每格至多站一人」这条不变式在生产路径上的验收——那条规则此前只
//! 写在注释里，`ll_sim::resolve::resolve_move` 里一行占位检查都没有。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 同 `ai_stall.rs`：直接提交 `Intent::Move`，不模拟键盘。

use ll_game::content::{LoadedContent, RuntimeCatalogs};
use ll_game::world::build_new_world;
use ll_sim::intent::{Direction, Intent};
use ll_sim::timeline::Timeline;
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_world::entity::{Affiliation, AffiliationKind, EntityId, OrgRef};
use ll_world::state::WorldState;

/// 固定种子，理由同 `ai_stall.rs` 的同名常量。
const SEED: u64 = 20260826;

/// 测试用内容装载——写法与 `ai_stall.rs`/`npc_materialization.rs` 的
/// 同名帮手一致（集成测试之间看不见彼此的私有帮手）。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ll-game-bump-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 建一局世界，并在玩家**东侧相邻格**放一个实体（克隆玩家的 `Agent`
/// 改坐标，理由同 `ai_stall.rs` 的 `spawn_edge_dweller`）。
fn world_with_neighbour() -> (ll_game::world::GameWorld, LoadedContent, EntityId) {
    let content = test_content();
    let mut game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("建世界应当成功");
    let mut agent = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("建局之后玩家必然存在")
        .clone();
    agent.pos = game_world.world.size.wrap(agent.pos.x() + 1, agent.pos.y());
    let neighbour = game_world.world.actors.spawn(agent);
    (game_world, content, neighbour)
}

/// 给一个实体挂一条势力归属。
fn join_faction(world: &mut WorldState, actor: EntityId, faction: u32) {
    world
        .actors
        .get_mut(actor)
        .expect("实体存在")
        .affiliations
        .push(Affiliation {
            kind: AffiliationKind::Faction,
            org: OrgRef::Instance(ll_core::ident::WorldId::next(&mut { faction })),
            standing: 0,
        });
}

/// 让玩家朝东提交一次移动，返回结果——`Demo::advance` 里那一串调用的
/// 最小等价物。
fn player_steps_east(
    game_world: &mut ll_game::world::GameWorld,
    content: &LoadedContent,
) -> PlayerTurnOutcome {
    let player = game_world.player;
    let clock = game_world.world.clock;
    let mut timeline = Timeline::new();
    timeline.schedule(player, clock);
    let mut engine = TurnEngine::new(timeline);
    let runtime = RuntimeCatalogs::new(content);
    let catalogs = runtime.as_resolve_catalogs();
    let mut on_effect = |_world: &WorldState, _effect: &ll_sim::effect::Effect| {};
    // 先让引擎把玩家那一条弹进 `pending`——世界里另一个实体排在更晚，
    // 因此这一步不会结算任何人。
    engine.advance_ai(
        &mut game_world.world,
        player,
        &mut |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Wait { actor },
        &catalogs,
        &mut on_effect,
    );
    engine.try_player_intent(
        &mut game_world.world,
        player,
        Intent::Move {
            actor: player,
            dir: Direction::East,
        },
        &catalogs,
        &mut on_effect,
    )
}

#[test]
fn 走向非敌对的邻居是互换位置而不是攻击() {
    // Arrange：双方都没有任何势力归属——本体内容里每一个实体的真实形态。
    let (mut game_world, content, neighbour) = world_with_neighbour();
    let player = game_world.player;
    let player_before = game_world.world.actors.get(player).expect("玩家在").pos;
    let neighbour_before = game_world.world.actors.get(neighbour).expect("邻居在").pos;
    let neighbour_health_before = game_world
        .world
        .actors
        .get(neighbour)
        .expect("邻居在")
        .health;

    // Act
    let outcome = player_steps_east(&mut game_world, &content);

    // Assert
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(
        game_world.world.actors.get(player).expect("玩家还在").pos,
        neighbour_before,
        "玩家应当站到邻居原来那一格"
    );
    assert_eq!(
        game_world
            .world
            .actors
            .get(neighbour)
            .expect("邻居还在")
            .pos,
        player_before,
        "邻居应当被换到玩家原来那一格"
    );
    assert_eq!(
        game_world
            .world
            .actors
            .get(neighbour)
            .expect("邻居还在")
            .health,
        neighbour_health_before,
        "非敌对的邻居一点血都不该掉——这正是所有者实机看到的那个缺陷"
    );
}

#[test]
fn 走向已声明敌对的邻居仍然是攻击() {
    // 反例：证明上一条不是「无论如何都换位置」，撞人即攻击那半条手感
    // 还在。
    // Arrange
    let (mut game_world, content, neighbour) = world_with_neighbour();
    let player = game_world.player;
    join_faction(&mut game_world.world, player, 1);
    join_faction(&mut game_world.world, neighbour, 2);
    let player_before = game_world.world.actors.get(player).expect("玩家在").pos;
    let neighbour_health_before = game_world
        .world
        .actors
        .get(neighbour)
        .expect("邻居在")
        .health;

    // Act
    let outcome = player_steps_east(&mut game_world, &content);

    // Assert
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(
        game_world.world.actors.get(player).expect("玩家还在").pos,
        player_before,
        "攻击不挪位置"
    );
    // 目标已经不在世界里 = 一击致死，同样是「真的挨了一下」的合法结果，
    // 因此只在它还活着时比血量。
    if let Some(agent) = game_world.world.actors.get(neighbour) {
        assert!(
            agent.health < neighbour_health_before,
            "已声明敌对的邻居应当真的挨了一下：{} → {}",
            neighbour_health_before,
            agent.health
        );
    }
}

/// 让 `mover` 朝西提交一次移动并由 `TurnEngine` 结算——`Demo::advance`
/// 每帧那两步里的**前一步**（`advance_ai` 结算排在玩家之前的非受控
/// 实体）的最小等价物。返回这一步产出的效果条数。
///
/// 与 [`player_steps_east`] 的差别只有一处：这一次动的是**非受控
/// 实体**，走的是 `advance_ai` 那条路，不是 `try_player_intent`。
fn npc_steps_west(
    game_world: &mut ll_game::world::GameWorld,
    content: &LoadedContent,
    mover: EntityId,
) -> usize {
    let player = game_world.player;
    let clock = game_world.world.clock;
    let mut timeline = Timeline::new();
    timeline.schedule(mover, clock);
    // 玩家排在更晚：`advance_ai` 会先结算 `mover`，轮到玩家时立即返回。
    timeline.schedule(player, ll_core::time::Tick(clock.0 + 10_000));
    let mut engine = TurnEngine::new(timeline);
    let runtime = RuntimeCatalogs::new(content);
    let catalogs = runtime.as_resolve_catalogs();
    let mut effects_seen = 0usize;
    engine.advance_ai(
        &mut game_world.world,
        player,
        &mut |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Move {
            actor,
            dir: Direction::West,
        },
        &catalogs,
        &mut |_world: &WorldState, _effect: &ll_sim::effect::Effect| effects_seen += 1,
    );
    effects_seen
}

#[test]
fn npc走向非敌对的玩家挪不动但仍然消耗一次行动() {
    // 所有者裁定「玩家优先度高于NPC，只有玩家可以互换位置」在生产路径
    // 上的验收，同时也是「每格至多站一人」这条不变式的验收：邻居朝
    // 玩家走一步，**两个人的坐标都不该变**（既没互换，也没摞在一起）。
    //
    // 上一批的实现在这里会把两人换位；再上一批（占位检查落地之前）
    // 会让邻居直接站到玩家那一格上——两种旧行为下这条都会红。
    // Arrange
    let (mut game_world, content, neighbour) = world_with_neighbour();
    let player = game_world.player;
    let player_before = game_world.world.actors.get(player).expect("玩家在").pos;
    let neighbour_before = game_world.world.actors.get(neighbour).expect("邻居在").pos;
    let neighbour_clock_before = game_world
        .world
        .actors
        .get(neighbour)
        .expect("邻居在")
        .next_action_at;

    // Act
    let effects_seen = npc_steps_west(&mut game_world, &content, neighbour);

    // Assert 一：谁都没动。
    assert_eq!(
        game_world.world.actors.get(player).expect("玩家还在").pos,
        player_before,
        "玩家没有做出任何决定，不该被 NPC 推开——互换只对玩家开"
    );
    assert_eq!(
        game_world
            .world
            .actors
            .get(neighbour)
            .expect("邻居还在")
            .pos,
        neighbour_before,
        "目的地站着人，这一步不该落地——每格至多站一人"
    );

    // Assert 二：这一步仍然消耗了一次行动（与撞墙同一个口径），且是
    // 它**自己**产出的效果，不是被 `perform` 的进展保证补跑一次
    // `Intent::Wait` 兜出来的。
    assert!(
        game_world
            .world
            .actors
            .get(neighbour)
            .expect("邻居还在")
            .next_action_at
            .0
            > neighbour_clock_before.0,
        "撞人仍然消耗一次行动，NPC 不该因为前面站了人就白赚一回合"
    );
    assert!(
        effects_seen > 0,
        "这一步必须自己产出 ScheduleNext，不能靠进展保证兜底"
    );

    // Assert 三：谁都没掉血——挪不动不是打了一架。
    assert_eq!(
        game_world
            .world
            .actors
            .get(player)
            .expect("玩家还在")
            .health,
        ll_world::entity::Agent::STARTING_HEALTH
    );
}
