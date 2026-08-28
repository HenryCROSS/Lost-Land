//! AI 空转探针：复刻 `ll_game::app::Demo::advance` 里「维护流式邻域 →
//! `TurnEngine::advance_ai` → 玩家提交一次行动」这一串，把每一帧结算了
//! 多少步、卡住的是谁、它提交的是什么意图、产出了几条效果原样打出来。
//!
//! **这是测量工具，不是产品代码**——与 `probe_conquest.rs` 同一个定位。
//! 存在的理由是：「advance_ai 卡住了」这句话在没有这个探针之前只能靠
//! 猜（那条 ERROR 日志自己的措辞「多半是某个 AI 卡在原地反复无效行动」
//! 就是一句猜测）。本探针把「哪个实体、哪条意图、为什么不推进时钟」
//! 变成可以直接读出来的数据。
//!
//! 它当初定位到的结论：一个平民站在常驻区块集合的边界上，往非常驻的
//! 那一侧游走，`resolve_move` 因此返回空 `Vec`——没有
//! `Effect::ScheduleNext`，时钟原地不动，同一刻被反复弹出。根因与修法
//! 见 `ll_sim::turn::TurnEngine::perform` 文档「进展保证」一节，回归
//! 测试见 `crates/ll-game/tests/ai_stall.rs`。
//!
//! ```text
//! cargo run --release --example probe_aistall -p ll-game
//! cargo run --release --example probe_aistall -p ll-game -- path/to/save.llsave
//! ```
//!
//! 不给参数时按 `config.json5` 的默认新游戏配置现建一局；给一个存档
//! 路径时读那份存档——所有者那份卡死的存档就是这样第一次被复现的。

use std::cell::RefCell;
use std::collections::BTreeMap;

use ll_mod::native_behavior::{BehaviorRuleCatalogs, NativeBehaviorSource, NativeBehaviorTree};
use ll_mod::roster::SettlementRoles;
use ll_sim::effect::Effect;
use ll_sim::intent::{Direction, Intent};
use ll_sim::turn::TurnEngine;
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

/// 跑多少「帧」。每帧 = 一次流式维护 + 一次 `advance_ai` + 玩家走一步。
const FRAMES: usize = 400;

/// 单帧结算超过这么多步就当作空转，打详情并停下。
const STALL_THRESHOLD: usize = 100;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let content = ll_game::content::load_content(&root.join("mods"), &root.join("assets"))
        .expect("装载本体内容");
    let mut game_world = match std::env::args().nth(1) {
        Some(path) => load_save(std::path::Path::new(&path), &content),
        None => {
            let params = ll_game::worldgen::resolve_gen_params(
                &ll_platform::config::NewGameConfig::default(),
            );
            println!("新局：种子={} 形态={:?}", params.seed, params.shape);
            ll_game::world::build_new_world(&content, params).expect("建局")
        }
    };
    let player = game_world.player;
    let mut engine = TurnEngine::new(std::mem::take(&mut game_world.timeline));
    let mut npc_ai = NativeBehaviorSource::new(
        NativeBehaviorTree::townsfolk(),
        BehaviorRuleCatalogs::snapshot(
            &content.race_table,
            &content.class_table,
            &content.subclass_table,
            &content.trait_table,
            &content.item_table,
        ),
        game_world.world.seed,
    )
    .with_class_bindings(content.class_behavior_bindings.clone(), &content.registry);
    let roles = SettlementRoles::resolve(
        &content.registry,
        &content.class_table,
        &content.resource_table,
        &content.culture_table,
    );

    for frame in 0..FRAMES {
        // ── `Demo::maintain_streaming` 的等价物 ──
        let pos = game_world.world.actors.get(player).expect("玩家存在").pos;
        let clock = game_world.world.clock;
        game_world.world.terrain.stream_neighborhood(
            &game_world.noise,
            &game_world.params,
            &content.terrain_ids,
            pos,
            ll_game::world::STREAM_RADIUS_ZONES,
            clock,
        );
        let spawned =
            ll_game::world::materialize_nearby_settlements(&mut game_world.world, &content, &roles);
        for actor in &spawned {
            engine.schedule(*actor, clock);
        }

        // ── `advance_ai` 的等价物（带记录） ──
        let runtime = ll_game::content::RuntimeCatalogs::new(&content);
        let catalogs = runtime.as_resolve_catalogs();
        let log: RefCell<Vec<(EntityId, Intent)>> = RefCell::new(Vec::new());
        let effect_counts: RefCell<Vec<usize>> = RefCell::new(Vec::new());
        {
            let mut inner = ll_sim::behavior::behavior_ai_intent(&mut npc_ai);
            let mut ai_intent = |world: &WorldState, actor: EntityId, controlled: EntityId| {
                let intent = inner(world, actor, controlled);
                log.borrow_mut().push((actor, intent));
                effect_counts.borrow_mut().push(0);
                intent
            };
            let mut on_effect = |_w: &WorldState, _e: &Effect| {
                if let Some(last) = effect_counts.borrow_mut().last_mut() {
                    *last += 1;
                }
            };
            engine.advance_ai(
                &mut game_world.world,
                player,
                &mut ai_intent,
                &catalogs,
                &mut on_effect,
            );
        }
        let steps = log.borrow().len();
        if !spawned.is_empty() || steps > STALL_THRESHOLD {
            println!(
                "帧 {frame}：物化 {} 人，实体 {} 个，时钟 {}，advance_ai 结算 {steps} 步",
                spawned.len(),
                game_world.world.actors.iter().count(),
                game_world.world.clock.0,
            );
        }
        if steps > STALL_THRESHOLD {
            report(&game_world.world, &log.borrow(), &effect_counts.borrow());
            return;
        }
        // 玩家这一帧朝东走一步——真实游玩里玩家一行动，自己的
        // `next_action_at` 就被推到将来，时间轴上剩下的全是 NPC。
        engine.try_player_intent(
            &mut game_world.world,
            player,
            Intent::Move {
                actor: player,
                dir: Direction::East,
            },
            &catalogs,
            &mut |_: &WorldState, _: &Effect| {},
        );
    }
    println!("跑完 {FRAMES} 帧，没有出现单帧超过 {STALL_THRESHOLD} 步的空转。");
}

/// 读一份存档，重建编年史与时间轴——与 `ll_game::load_or_new_game` 的
/// 读档分支同一串动作（那个函数是私有的，探针不在它的调用链上）。
fn load_save(
    path: &std::path::Path,
    content: &ll_game::content::LoadedContent,
) -> ll_game::world::GameWorld {
    println!("读档：{}", path.display());
    match ll_game::save::load_game(path, content) {
        ll_game::save::LoadedGame::Playable {
            mut world,
            identity,
        } => {
            let player = world.player_entity.expect("可游玩的存档记录了玩家");
            let params = world.gen_params();
            let layout = ll_game::world::build_zone_layout().expect("默认布局");
            let noise = ll_world::generate::build_zone_noise(&layout, &params).expect("默认布局");
            world.terrain.attach_chronicle(std::sync::Arc::new(
                ll_world::chronicle::WorldChronicle::generate(
                    &ll_world::chronicle::ChronicleInput {
                        layout: &layout,
                        noise: &ll_world::generate::build_zone_noise(&layout, &params)
                            .expect("默认布局"),
                        params: &params,
                        terrain_ids: &content.terrain_ids,
                        terrain_table: &content.terrain_table,
                        resources: &content.resource_table,
                        cultures: &content.culture_table,
                    },
                    ll_world::chronicle::ChronicleParams::default(),
                ),
            ));
            let timeline = ll_game::world::rebuild_timeline(&world);
            ll_game::world::GameWorld {
                world,
                noise,
                params,
                player,
                timeline,
                identity,
            }
        }
        other => {
            eprintln!("存档不可游玩：{other:?}");
            std::process::exit(1);
        }
    }
}

/// 把这一帧的结算记录汇总成「谁被结算了几次、其中几次零效果」，再对
/// 头号嫌疑人打出它的意图序列与周围地形。
fn report(world: &WorldState, log: &[(EntityId, Intent)], effects: &[usize]) {
    let mut per_actor: BTreeMap<u64, (usize, usize)> = BTreeMap::new();
    for (i, (actor, _)) in log.iter().enumerate() {
        let slot = per_actor.entry(actor.as_u64()).or_insert((0, 0));
        slot.0 += 1;
        if effects.get(i).copied().unwrap_or(0) == 0 {
            slot.1 += 1;
        }
    }
    let mut ranked: Vec<_> = per_actor.iter().collect();
    ranked.sort_by_key(|(id, (n, _))| (std::cmp::Reverse(*n), **id));
    println!("  结算次数排名（次数 / 其中零效果次数）：");
    for (id, (n, zero)) in ranked.iter().take(8) {
        println!("    实体 {id}: {n} 次，其中 {zero} 次零效果");
    }
    let Some((worst_id, (n, zero))) = ranked.first().copied() else {
        return;
    };
    println!("  头号嫌疑人 实体 {worst_id}：{n} 次结算，{zero} 次零效果");
    for (i, (actor, intent)) in log.iter().enumerate().take(4096) {
        if actor.as_u64() != *worst_id {
            continue;
        }
        println!("    第 {i} 步 意图={intent:?} 效果数={}", effects[i]);
        if i > 4 {
            break;
        }
    }
    for (id, agent) in world.actors.iter_with_id() {
        if id.as_u64() != *worst_id {
            continue;
        }
        println!(
            "    当前状态：pos=({},{}) space={:?} next_action_at={}",
            agent.pos.x(),
            agent.pos.y(),
            agent.current_space,
            agent.next_action_at.0,
        );
        println!("    脚下地形常驻查询 = {:?}", world.terrain_at(agent.pos));
        for dir in [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            let (dx, dy) = dir.delta();
            let dest = world.size.wrap(agent.pos.x() + dx, agent.pos.y() + dy);
            println!(
                "      {dir:?} → ({},{}) 地形={:?}",
                dest.x(),
                dest.y(),
                world.terrain_at(dest)
            );
        }
    }
}
