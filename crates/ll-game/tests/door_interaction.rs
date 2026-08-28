//! 端到端验收：**门进交互列表，能开也能关。**
//!
//! 所有者原话：
//!
//! > 「我希望交互也能包括和 NPC 对话，开关门，和放置在地上的家具互动等」
//!
//! **本批次只做「开关门」这一条**（NPC 对话是一个全仓库零实现的系统，
//! 且牵扯到「通过和据点管理者对话加入势力」这条更大的设计；家具交互
//! 早已存在，是 `InteractTarget::Facility`）。
//!
//! 全程走真实 `mods/` 内容与生产路径上的 `build_new_world` +
//! `TurnEngine`，与本体二进制 `ll_game::app::Demo::advance` 走同一串
//! 调用（ADR 0018）。不启动窗口、不模拟键盘（ADR 0025）。
//!
//! # 这几条断言真的会红吗
//!
//! 逐条记在各自的测试文档里，见本批次提交信息。

use ll_core::torus::TorusPos;
use ll_game::content::{LoadedContent, RuntimeCatalogs};
use ll_game::player_action::{DoorAction, InteractTarget, interact_entries, interact_tiles};
use ll_game::world::{GameWorld, build_new_world};
use ll_sim::intent::Intent;
use ll_sim::timeline::Timeline;
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_world::entity::EntityId;
use ll_world::item::{GroundItemStack, ItemStack};
use ll_world::state::WorldState;

/// 固定种子，理由同 `culture_hostility.rs` 的同名常量。
const SEED: u64 = 20260826;

fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("ll-game-door-interact-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 建一局世界，并把玩家**东侧相邻格**改成指定地形。
fn world_with_east_terrain(
    content: &LoadedContent,
    kind: ll_world::terrain::TerrainKind,
) -> (GameWorld, TorusPos) {
    let mut game_world = build_new_world(
        content,
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
    let east = game_world
        .world
        .size
        .wrap(player_pos.x() + 1, player_pos.y());
    game_world.world.terrain.set_terrain(east, kind);
    (game_world, east)
}

/// 让玩家提交一次意图——`Demo::advance` 里那一串调用的最小等价物。
fn player_submits(
    game_world: &mut GameWorld,
    content: &LoadedContent,
    intent: Intent,
) -> PlayerTurnOutcome {
    let player = game_world.player;
    let clock = game_world.world.clock;
    let mut timeline = Timeline::new();
    timeline.schedule(player, clock);
    let mut engine = TurnEngine::new(timeline);
    let runtime = RuntimeCatalogs::new(content);
    let catalogs = runtime.as_resolve_catalogs();
    let mut on_effect = |_world: &WorldState, _effect: &ll_sim::effect::Effect| {};
    // 先把排在玩家前面的实体推完——与 `culture_hostility.rs` 的
    // `player_steps_east` 同一个写法，否则 `try_player_intent` 返回
    // `NotYet`（还没轮到玩家）。
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
        intent,
        &catalogs,
        &mut on_effect,
    )
}

fn terrain_at(world: &WorldState, pos: TorusPos) -> ll_world::terrain::TerrainKind {
    world.terrain_at(pos).expect("测试用的这一格必然常驻")
}

// ── 一、门进得了交互列表 ─────────────────────────────────────────

#[test]
fn 关着的门出现在交互列表里且主交互是开门() {
    // 在此之前门只能靠**撞上去**开（`resolve_move` 的 `opens_into`
    // 分支），交互列表里一行都没有——`interact_entries` 只扫
    // `ground_items`，地形一眼都不看。
    //
    // 故意改坏的反例（人工核验）：把 `interact_entries` 末尾那三行
    // `if let Some(action) = door_action_at(..)` 删掉，本条当场变红。
    // Arrange
    let content = test_content();
    let (game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_closed);

    // Act
    let entries = interact_entries(&game_world.world, east);

    // Assert
    assert_eq!(
        entries,
        vec![InteractTarget::Door {
            action: DoorAction::Open
        }],
        "关着的门应当出现在交互列表里，主交互是开门"
    );
}

#[test]
fn 开着的门出现在交互列表里且主交互是关门() {
    // **关门此前完全不存在**：撞不出一扇关上的门，没有任何 `Intent`
    // 能把门关回去。这一行是关门唯一的入口。
    //
    // 判据是 `TerrainTable::closes_into` 反查 `opens_into`，不是硬编码
    // `lostland:door_open`。
    //
    // 故意改坏的反例（人工核验）：把 `door_action_at` 里
    // `closes_into(..).map(..)` 那一段改成恒 `None`，本条当场变红而
    // 上一条照常绿。
    // Arrange
    let content = test_content();
    let (game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_open);

    // Act
    let entries = interact_entries(&game_world.world, east);

    // Assert
    assert_eq!(
        entries,
        vec![InteractTarget::Door {
            action: DoorAction::Close
        }]
    );
}

#[test]
fn 有门的那一格进得了方向列表() {
    // `interact_tiles` 按 `interact_entries` 非空筛格。门若不进
    // `interact_entries`，玩家按空格时那一格根本不会被列出来——列表里
    // 有行却选不到那一格，等于没做。
    //
    // 故意改坏的反例（人工核验）：同第一条（删掉 `door_action_at` 的
    // 调用），本条当场变红。
    // Arrange
    let content = test_content();
    let (game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_closed);
    let player_pos = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家在")
        .pos;

    // Act
    let tiles = interact_tiles(&game_world.world, player_pos);

    // Assert
    assert!(
        tiles.iter().any(|tile| tile.pos == east),
        "东边那一格有门，应当出现在方向列表里"
    );
}

#[test]
fn 平地不会被当成门() {
    // 反向断言：没有 `opens_into`、也不是任何地形 `opens_into` 目标的
    // 普通地形，一行都不该多出来。这条守的是「判据不是恒真」。
    //
    // 故意改坏的反例（人工核验）：把 `door_action_at` 改成无条件返回
    // `Some(DoorAction::Open)`，本条当场变红。
    // Arrange
    let content = test_content();
    let (game_world, east) = world_with_east_terrain(&content, content.terrain_ids.floor_stone);

    // Act
    let entries = interact_entries(&game_world.world, east);

    // Assert
    assert!(entries.is_empty(), "石地板不是门，实测 {entries:?}");
}

// ── 二、真的开得了、关得上 ───────────────────────────────────────

#[test]
fn 从交互列表开门真的把地形改成开着的门() {
    // 走生产路径的 `TurnEngine::try_player_intent`，不是直接调 `resolve`。
    //
    // 故意改坏的反例（人工核验）：把 `interact_command` 里
    // `DoorAction::Open => Intent::OpenDoor { .. }` 换成
    // `Intent::Wait { actor }`，本条当场变红。
    // Arrange
    let content = test_content();
    let (mut game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_closed);
    let player = game_world.player;

    // Act
    let outcome = player_submits(
        &mut game_world,
        &content,
        Intent::OpenDoor {
            actor: player,
            pos: (east.x(), east.y()),
        },
    );

    // Assert
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(
        terrain_at(&game_world.world, east),
        content.terrain_ids.door_open
    );
}

#[test]
fn 从交互列表关门真的把地形改回关着的门() {
    // **本批次新增的能力**，此前没有任何代码路径做得到这件事。
    //
    // 故意改坏的反例（人工核验）：把 `resolve_close_door` 里那句
    // `let Some(closed_kind) = world.terrain_table.closes_into(terrain)`
    // 改成恒 `None`（早退成只消耗时间），本条当场变红。
    // Arrange
    let content = test_content();
    let (mut game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_open);
    let player = game_world.player;

    // Act
    let outcome = player_submits(
        &mut game_world,
        &content,
        Intent::CloseDoor {
            actor: player,
            pos: (east.x(), east.y()),
        },
    );

    // Assert
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(
        terrain_at(&game_world.world, east),
        content.terrain_ids.door_closed
    );
}

#[test]
fn 撞门开门那条既有路径原样保留() {
    // 两条路并存：撞上去是「顺手推开」，从列表里选是「我就是要开这一
    // 扇」。本次改动不许把既有那条拆掉。
    //
    // 故意改坏的反例（人工核验）：把 `resolve_move` 里
    // `if let Some(open_kind) = terrain.opens_into(..)` 整个分支删掉，
    // 本条当场变红而上面两条照常绿。
    // Arrange
    let content = test_content();
    let (mut game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_closed);
    let player = game_world.player;

    // Act
    let outcome = player_submits(
        &mut game_world,
        &content,
        Intent::Move {
            actor: player,
            dir: ll_sim::intent::Direction::East,
        },
    );

    // Assert
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(
        terrain_at(&game_world.world, east),
        content.terrain_ids.door_open,
        "撞门开门必须照旧有效"
    );
}

// ── 三、门不会关在人身上 ─────────────────────────────────────────

#[test]
fn 门口站着人时关不上() {
    // 所有者要求的前置。占位查找复用批次 1 的
    // `ll_sim::resolve::occupant_at`，不另写一份。
    //
    // 故意改坏的反例（人工核验）：把 `resolve_close_door` 里
    // `if occupant_at(world, door_pos, actor).is_some()` 那一段删掉，
    // 本条当场变红。
    // Arrange
    let content = test_content();
    let (mut game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_open);
    let player = game_world.player;
    // 在门那一格上放一个实体（克隆玩家的 `Agent` 改坐标，写法同
    // `bump_into_occupant.rs` 的同名帮手）。
    let mut agent = game_world.world.actors.get(player).expect("玩家在").clone();
    agent.pos = east;
    let _standing: EntityId = game_world.world.actors.spawn(agent);

    // Act
    player_submits(
        &mut game_world,
        &content,
        Intent::CloseDoor {
            actor: player,
            pos: (east.x(), east.y()),
        },
    );

    // Assert
    assert_eq!(
        terrain_at(&game_world.world, east),
        content.terrain_ids.door_open,
        "门口站着人，门不该关上"
    );
}

#[test]
fn 门口立着家具时关不上() {
    // 另一半前置：立着的家具同样占着那一格（`GroundItemStack::placed`，
    // 与 `resolve_place` 的「一格至多立一件」用的是同一个字段）。
    //
    // 故意改坏的反例（人工核验）：把 `resolve_close_door` 里那段
    // `world.ground_items.iter().any(|ground| .. && ground.placed)`
    // 删掉，本条当场变红。
    // Arrange
    let content = test_content();
    let (mut game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_open);
    let player = game_world.player;
    let forge = content
        .registry
        .get(&ll_core::ident::NamespacedId::parse("lostland:forge").expect("合法标识符"))
        .expect("本体注册过锻炉");
    game_world.world.ground_items.push(GroundItemStack {
        pos: east,
        stack: ItemStack::new(forge, 1),
        dropped_at: game_world.world.clock,
        contents: Vec::new(),
        placed: true,
    });

    // Act
    player_submits(
        &mut game_world,
        &content,
        Intent::CloseDoor {
            actor: player,
            pos: (east.x(), east.y()),
        },
    );

    // Assert
    assert_eq!(
        terrain_at(&game_world.world, east),
        content.terrain_ids.door_open,
        "门口立着一座锻炉，门不该关上"
    );
}

#[test]
fn 门口只是散落着东西时照样关得上() {
    // 与上一条成对：散落在地上的东西**不挡门**——一把掉在门槛上的匕首
    // 不该让门关不上。这条守的是上一条的判据用的是 `placed` 而不是
    // 「这一格有没有东西」。
    //
    // 故意改坏的反例（人工核验）：把 `resolve_close_door` 里那段
    // `&& ground.placed` 去掉（变成「这一格有任何东西就不许关」），
    // 本条当场变红。
    // Arrange
    let content = test_content();
    let (mut game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_open);
    let player = game_world.player;
    let ingot = content
        .registry
        .get(&ll_core::ident::NamespacedId::parse("lostland:iron_ingot").expect("合法标识符"))
        .expect("本体注册过铁锭");
    game_world.world.ground_items.push(GroundItemStack {
        pos: east,
        stack: ItemStack::new(ingot, 1),
        dropped_at: game_world.world.clock,
        contents: Vec::new(),
        placed: false,
    });

    // Act
    player_submits(
        &mut game_world,
        &content,
        Intent::CloseDoor {
            actor: player,
            pos: (east.x(), east.y()),
        },
    );

    // Assert
    assert_eq!(
        terrain_at(&game_world.world, east),
        content.terrain_ids.door_closed,
        "散落的铁锭不该挡住关门"
    );
}

// ── 四、门与地面物品同格时两行都在 ───────────────────────────────

#[test]
fn 门那一行排在地面物品之后() {
    // 顺序确定（约束 C5）：玩家按的是「第几行」。门排在 `ground_items`
    // 扫描结果**之后**——这样这一格上原有的物品保持它们原来的行号，
    // 加门这件事不会让老存档里同一串按键落到不同的东西上。
    //
    // 故意改坏的反例（人工核验）：把 `interact_entries` 里那三行
    // `rows.push(InteractTarget::Door { .. })` 挪到扫描 `ground_items`
    // 的循环之前，本条当场变红。
    // Arrange
    let content = test_content();
    let (mut game_world, east) = world_with_east_terrain(&content, content.terrain_ids.door_closed);
    let ingot = content
        .registry
        .get(&ll_core::ident::NamespacedId::parse("lostland:iron_ingot").expect("合法标识符"))
        .expect("本体注册过铁锭");
    game_world.world.ground_items.push(GroundItemStack {
        pos: east,
        stack: ItemStack::new(ingot, 1),
        dropped_at: game_world.world.clock,
        contents: Vec::new(),
        placed: false,
    });

    // Act
    let entries = interact_entries(&game_world.world, east);

    // Assert
    assert_eq!(
        entries,
        vec![
            InteractTarget::Loose { def: ingot },
            InteractTarget::Door {
                action: DoorAction::Open
            },
        ]
    );
}

// ── 五、家具全部进得了交互列表（核实，本批次不改） ───────────────

#[test]
fn 每一件立着的家具都进得了交互列表() {
    // 所有者那句话里的第三条（「和放置在地上的家具互动」）**早已存在**
    // ——`InteractTarget::Facility` 就是那一支。本条是一次**核实**：
    // 判据是 `GroundItemStack::placed` 这一个布尔，不查配方表、不查
    // `ItemDef::furniture`，因此**凡是立起来的东西无一例外都进得了列表**。
    //
    // 这里用一件**不是家具**的物品（铁锭）立在地上：`resolve_place` 的
    // 前置会拦住玩家这么做，但一旦它以任何方式变成 `placed`，交互列表
    // 照样列出它——这正是「无一例外」的强形式。
    //
    // 故意改坏的反例（人工核验）：把 `interact_entries` 里
    // `if ground.placed` 那一支改成
    // `if ground.placed && items.furniture(def)`（即改成查 `ItemDef`），
    // 本条当场变红。
    // Arrange
    let content = test_content();
    let (mut game_world, east) = world_with_east_terrain(&content, content.terrain_ids.floor_stone);
    let ingot = content
        .registry
        .get(&ll_core::ident::NamespacedId::parse("lostland:iron_ingot").expect("合法标识符"))
        .expect("本体注册过铁锭");
    game_world.world.ground_items.push(GroundItemStack {
        pos: east,
        stack: ItemStack::new(ingot, 1),
        dropped_at: game_world.world.clock,
        contents: Vec::new(),
        placed: true,
    });

    // Act
    let entries = interact_entries(&game_world.world, east);

    // Assert
    assert_eq!(entries, vec![InteractTarget::Facility { def: ingot }]);
}
