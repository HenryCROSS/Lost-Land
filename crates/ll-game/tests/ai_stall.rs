//! 端到端回归：**流式加载边界上的 NPC 不再冻结整条回合推进路径**。
//!
//! # 复现的是哪一条实机缺陷
//!
//! 项目所有者起游戏后控制台每帧刷一条
//! `advance_ai 单次调用内达到 10000 步仍未轮到受控实体，提前放弃`，
//! 而角色完全无法移动。
//!
//! 根因（用探针在所有者那份存档上实测出来的，不是推测）：一个平民 NPC
//! 站在常驻区块集合的边界上，行为树掷出「往东南游走一步」，而东南那一
//! 格所属区块尚未常驻——`ll_sim::resolve` 的 `resolve_move` 对这种情形
//! 返回空 `Vec`（它有一段明确注释论证「查不到地形，无法判断这一步本该
//! 耗时多久，静默作废更安全」），空 `Vec` 里没有 `Effect::ScheduleNext`，
//! 于是这个 NPC 的 `next_action_at` 一个 tick 都不动，被原样排回时间轴、
//! 在同一刻被再次弹出。行为树的三个输入（世界状态、按
//! `(种子, 实体号, 世界时钟)` 派生的确定性随机流、树本身）一个都没变，
//! 它必然重复同一个决定——**确定性死循环**。玩家的时间轴条目永远排在
//! 这个 NPC 后面，因此一次行动都轮不到。
//!
//! 修法是 `ll_sim::turn::TurnEngine::perform` 的进展保证，不是把
//! `MAX_STEPS_PER_ADVANCE` 调高，见该方法文档。
//!
//! # 为什么这条测试此前抓不到
//!
//! 既有的 `npc_materialization.rs`「物化出的 npc 真的被既有回合引擎与
//! 行为树驱动」把流式邻域**恰好挪到据点锚点上**，物化出来的人全部落在
//! 常驻区块正中央，八个方向的目的地都查得到地形——那份夹具里
//! `resolve_move` 的「地形非常驻」分支一次都走不到。缺陷需要的是「NPC
//! 在常驻边界之外或之上」，而那只有在玩家走远之后才会自然出现。本文件
//! 把那个条件直接构造出来。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 与 `npc_materialization.rs` 同一条纪律：直接调生产路径上的那几个
//! 函数（`build_new_world`、`SurfaceStore::stream_neighborhood`、
//! `TurnEngine::advance_ai`/`try_player_intent`），只跳过窗口/输入外壳。

use ll_game::content::{LoadedContent, RuntimeCatalogs};
use ll_game::world::{STREAM_RADIUS_ZONES, build_new_world};
use ll_sim::behavior::behavior_ai_intent;
use ll_sim::intent::Intent;
use ll_sim::timeline::Timeline;
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

/// 固定种子，理由同 `npc_materialization.rs` 的同名常量：失败时能原样
/// 重跑。本文件的断言不依赖这颗种子长出什么地形。
const SEED: u64 = 20260826;

/// 一场观察跑多少轮（一轮 = 一次 `advance_ai` + 一次玩家「等待」）。
///
/// 取 64：平民那棵树每回合按
/// `ll_mod::native_behavior::TOWNSFOLK_WANDER_PERMILLE` 掷一次骰决定
/// 游走还是原地等待，几十轮足以让「游走」那一支反复出现很多次——修复
/// 前只要命中一次就会永久卡死。
const ROUNDS: usize = 64;

/// 单次 `advance_ai` 允许结算的步数上限。
///
/// 世界里只有玩家与一个 NPC，正常情况下每轮至多结算个位数次。取 32 是
/// 一个宽松到不会误报、又远小于 `MAX_STEPS_PER_ADVANCE`（10000）的
/// 阈值：一旦真的空转，实测是**一次就顶满 10000**，两者之间不存在
/// 灰色地带。
const MAX_STEPS_PER_ROUND: usize = 32;

/// 测试用内容装载——写法与 `npc_materialization.rs` 的同名帮手一致
/// （集成测试之间看不见彼此的私有帮手）。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ll-game-ai-stall-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 从玩家所在格一路向东找出**第一个目的地不再常驻**的格子，把它交回。
///
/// 找的是「站得住、但往东一步就出界」的那一格——所有者存档里卡死的那个
/// 平民正是这个形状（它自己脚下查得到地形，东边那一格查不到）。
fn tile_at_resident_edge(
    world: &WorldState,
    from: ll_core::torus::TorusPos,
) -> ll_core::torus::TorusPos {
    let mut probe = from;
    for _ in 0..4096 {
        let next = world.size.wrap(probe.x() + 1, probe.y());
        if world.terrain_at(next).is_none() {
            return probe;
        }
        probe = next;
    }
    panic!("向东扫描 4096 格都没走出常驻区块集合，夹具前提不成立");
}

/// 建一局世界、把流式邻域维护到玩家身上，返回 `(世界, 内容)`。
fn world_with_streamed_neighborhood() -> (ll_game::world::GameWorld, LoadedContent) {
    let content = test_content();
    let mut game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("建世界应当成功");
    let player_pos = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("建局之后玩家必然存在")
        .pos;
    let clock = game_world.world.clock;
    // 生产路径上每帧都在调的那一个函数（`Demo::maintain_streaming`）。
    game_world.world.terrain.stream_neighborhood(
        &game_world.noise,
        &game_world.params,
        &content.terrain_ids,
        player_pos,
        STREAM_RADIUS_ZONES,
        clock,
    );
    (game_world, content)
}

/// 在常驻边界上放一个平民 NPC，返回它的实体号。
///
/// 用玩家的 `Agent` 克隆出来改坐标——本 crate 的 NPC 构造走
/// `ll_mod::roster::build_npc_agent`（要一份完整名册），而本文件验的
/// 不是 NPC 长什么样，是「一个站在边界上的实体会不会冻结回合推进」。
/// 克隆保证这个实体在**其余每一个字段上**都是一个完全正常的地表实体。
fn spawn_edge_dweller(game_world: &mut ll_game::world::GameWorld) -> EntityId {
    let player_pos = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家存在")
        .pos;
    let edge = tile_at_resident_edge(&game_world.world, player_pos);
    let mut agent = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家存在")
        .clone();
    agent.pos = edge;
    agent.next_action_at = game_world.world.clock;
    game_world.world.actors.spawn(agent)
}

#[test]
fn 常驻边界上的npc不会冻结回合推进并且玩家每轮都拿得到自己的回合() {
    // Arrange
    let (mut game_world, content) = world_with_streamed_neighborhood();
    let player = game_world.player;
    let dweller = spawn_edge_dweller(&mut game_world);
    let clock = game_world.world.clock;
    let mut timeline = Timeline::new();
    // NPC 排在玩家前面——这正是实机里的顺序（NPC 物化时按
    // `world.clock` 排入，玩家此刻的 `next_action_at` 不早于它）。
    timeline.schedule(dweller, clock);
    timeline.schedule(player, ll_core::time::Tick(clock.0 + 1));
    let mut engine = TurnEngine::new(timeline);
    // 真实的引擎自带行为树（平民那棵），与 `ll_game::app` 生产接线同一
    // 个构造，只是这里不需要职业绑定——夹具里的实体没有据点职业。
    let mut source = ll_mod::native_behavior::NativeBehaviorSource::new(
        ll_mod::native_behavior::NativeBehaviorTree::townsfolk(),
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
    let mut on_effect = |_world: &WorldState, _effect: &ll_sim::effect::Effect| {};

    // Act & Assert：逐轮跑，任何一轮空转或玩家拿不到回合都当场失败。
    let mut player_turns = 0usize;
    for round in 0..ROUNDS {
        let acted = {
            let mut ai_intent = behavior_ai_intent(&mut source);
            engine.advance_ai(
                &mut game_world.world,
                player,
                &mut ai_intent,
                &catalogs,
                &mut on_effect,
            )
        };
        assert!(
            acted.len() <= MAX_STEPS_PER_ROUND,
            "第 {round} 轮 advance_ai 结算了 {} 步——边界上的 NPC 又在空转了",
            acted.len()
        );
        let outcome = engine.try_player_intent(
            &mut game_world.world,
            player,
            Intent::Wait { actor: player },
            &catalogs,
            &mut on_effect,
        );
        assert_eq!(
            outcome,
            PlayerTurnOutcome::Acted,
            "第 {round} 轮玩家没能拿到自己的回合（这正是实机里角色无法移动的那一刻）"
        );
        player_turns += 1;
    }

    // 世界时钟真的走了这么多轮——不是「每轮都返回 Acted 但时间原地不动」。
    assert_eq!(player_turns, ROUNDS);
    assert!(
        game_world.world.clock.0 > clock.0,
        "跑完 {ROUNDS} 轮之后世界时钟必须真的前进了"
    );
    // 那个 NPC 自己的时钟也必须一直在往前走——它是死循环的当事人。
    let dweller_at = game_world
        .world
        .actors
        .get(dweller)
        .expect("边界上的 NPC 仍然活着")
        .next_action_at;
    assert!(
        dweller_at.0 > clock.0,
        "边界上的 NPC 的 next_action_at 必须真的推进过：{} → {}",
        clock.0,
        dweller_at.0
    );
}
