//! 端到端验收：三层可见性（战争迷雾）真的接进了 `ll-game` 本体二进制，
//! 不是只在 `ll-world`/`ll-sim` 里声明了接口却没人调用——本仓库已经
//! 反复点名过「声明了但从没接线」这类失败模式（见
//! `crates/ll-game/src/app.rs` `render_surface` 文档「三层可见性」
//! 一节），这里补一条脱离窗口/GPU 也能跑的程序化验收。
//!
//! # ADR 0025：不靠 SendKeys 盲注输入
//!
//! 本文件全程不启动窗口、不碰任何 GPU 资源、不模拟任何键盘/鼠标
//! 事件——直接调用 `ll_sim::resolve::resolve`/`ll_sim::apply::apply`，
//! 与 `ll_game::app::Demo::advance` 驱动玩家移动的是完全同一条管线
//! （`Intent → resolve → Effect → apply`），只是跳过了它外面那层
//! `InputState`/GPU 外壳。

use ll_core::torus::TorusPos;
use ll_game::content::LoadedContent;
use ll_game::world::{STREAM_RADIUS_ZONES, build_new_world};
use ll_sim::apply::apply;
use ll_sim::intent::{Direction, Intent};
use ll_sim::resolve::resolve;
use ll_world::entity::EntityId;
use ll_world::fov::{VisibleSet, compute_fov};
use ll_world::generate::GenParams;
use ll_world::noise::TileableNoise;
use ll_world::state::WorldState;
use ll_world::surface_store::SurfaceWindow;
use ll_world::zone::ZoneLayout;

/// 与 `ll_game::app::render_surface` 使用同一个基准视野半径的常量
/// 副本——本文件不依赖 `ll_game::layout`（那张模块的 `effective_sight_radius`
/// 还要接一份 `SpaceProfile`，本测试只关心可见性判定本身，不关心光照
/// 换算），直接复用其文档里写明的默认值 12。
const SIGHT_RADIUS: u32 = 12;

/// 向东走的步数——远大于 [`SIGHT_RADIUS`]，保证移动结束后出生点必然
/// 脱出当前 FOV，不依赖两个半径常量是否恰好相等。
const STEPS: u32 = 30;

/// 测试用内容装载——走与本体二进制完全相同的通道
/// （[`ll_game::content::load_content`]），只用一个空 mods 目录，装载
/// 本体自带的自然内容，与 `ll-game` 其余测试同一惯例（见
/// `crates/ll-game/src/world.rs` 测试 `test_content`）。
fn test_content() -> LoadedContent {
    let dir = std::env::temp_dir().join(format!(
        "ll-game-fog-of-war-test-content-{}-{}",
        std::process::id(),
        line!()
    ));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let content = ll_game::content::load_content(&dir, &dir.join("assets"));
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 把出生点正东 `steps` 格铺成草地——保证下面 [`move_player_east`] 的
/// 每一步都真的能走通，不受地形生成随机性影响（默认出生点只强制铺了
/// `ll_game::world` 里 `SPAWN_CLEARING_RADIUS`＝3 格半径的草地，测试
/// 要走的距离远不止 3 格，若不这样铺路，`resolve_move` 撞上生成出的
/// 水域/山地会静默拒绝这一步，`Effect::MoveTo` 根本不会产生，探索
/// 记忆也就测不出「玩家真的走远了」这一前提）。做法与
/// `ll_game::world::carve_spawn_clearing` 同一手法：先 `terrain_at`
/// 触发按需流式生成，再 `set_terrain` 覆写。
fn carve_east_path(
    world: &mut WorldState,
    noise: &TileableNoise,
    params: &GenParams,
    content: &LoadedContent,
    spawn: TorusPos,
    steps: i32,
) {
    for dx in 0..=steps {
        let pos = world.size.wrap(spawn.x() + dx, spawn.y());
        world
            .terrain
            .terrain_at(noise, params, &content.terrain_ids, pos, world.clock);
        world.terrain.set_terrain(pos, content.terrain_ids.grass);
    }
}

/// 沿东方向移动玩家 `steps` 格——每步先维护流式邻域（与
/// `ll_game::app::Demo::advance`/`maintain_streaming` 同一条纪律：目的
/// 地所属区块必须先常驻，`resolve_move` 才不会把它保守当成「不可
/// 通行」拒绝），再走 `resolve → apply`，与本体二进制每帧驱动移动的
/// 是同一条调用序列。
fn move_player_east(
    world: &mut WorldState,
    noise: &TileableNoise,
    params: &GenParams,
    content: &LoadedContent,
    player: EntityId,
    steps: u32,
) {
    for _ in 0..steps {
        let pos = world
            .actors
            .get(player)
            .expect("玩家在整个移动过程中恒存在")
            .pos;
        world.terrain.stream_neighborhood(
            noise,
            params,
            &content.terrain_ids,
            pos,
            STREAM_RADIUS_ZONES,
            world.clock,
        );
        let intent = Intent::Move {
            actor: player,
            dir: Direction::East,
        };
        let effects = resolve(world, &intent);
        for effect in &effects {
            apply(world, effect);
        }
    }
}

/// 一次完整的「新游戏 → 铺路 → 向东走 [`STEPS`] 格」过程的产出——四个
/// 测试（本文件与「离开出生点」相关的用例）共用同一套前置状态，各自
/// 只断言其中一件事，避免在单个 `#[test]` 里堆叠多个不相关的判据。
struct WalkedAway {
    world: WorldState,
    spawn: TorusPos,
    current_pos: TorusPos,
    layout: ZoneLayout,
}

fn walk_away_from_spawn() -> WalkedAway {
    let content = test_content();
    let mut game_world = build_new_world(&content, 7).expect("测试用默认布局满足全部前置条件");
    let spawn = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家刚生成，必然存在")
        .pos;
    let layout = *game_world.world.terrain.layout();

    carve_east_path(
        &mut game_world.world,
        &game_world.noise,
        &game_world.params,
        &content,
        spawn,
        STEPS as i32,
    );
    move_player_east(
        &mut game_world.world,
        &game_world.noise,
        &game_world.params,
        &content,
        game_world.player,
        STEPS,
    );
    let current_pos = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("移动后玩家仍存在")
        .pos;

    WalkedAway {
        world: game_world.world,
        spawn,
        current_pos,
        layout,
    }
}

fn visible_set_at(world: &WorldState, origin: TorusPos) -> VisibleSet<TorusPos> {
    compute_fov(
        &SurfaceWindow::new(&world.terrain),
        &world.terrain_table,
        origin,
        SIGHT_RADIUS,
    )
}

#[test]
fn 铺路后玩家沿路径每一步都真的移动成功() {
    // 前提性验收：后面几条测试的结论都建立在「玩家确实走出去了」这个
    // 事实上，这里单独锁定这件事本身，与「离开后战争迷雾三层判定」是
    // 两个不同的断言,不合并进同一个测试。
    // Arrange & Act
    let walked = walk_away_from_spawn();

    // Assert
    assert_eq!(
        walked.current_pos,
        walked
            .world
            .size
            .wrap(walked.spawn.x() + STEPS as i32, walked.spawn.y())
    );
}

#[test]
fn 玩家离开出生点后出生点仍标记为已探索() {
    // Arrange & Act：移动沿途第一步的目的地距出生点只有 1 格，远小于
    // 视野半径 12，出生点必然被那一步的 FOV 覆盖并标记探索。
    let walked = walk_away_from_spawn();

    // Assert
    assert!(
        walked
            .world
            .exploration
            .is_explored(&walked.layout, walked.spawn)
    );
}

#[test]
fn 玩家离开出生点后出生点不再落在当前视野内() {
    // Arrange & Act：这正是「探索过、当前无视野」那一层——渲染时改用
    // 记忆色调压暗，而不是完全不画。
    let walked = walk_away_from_spawn();
    let visible_now = visible_set_at(&walked.world, walked.current_pos);

    // Assert
    assert!(!visible_now.contains(walked.spawn));
}

#[test]
fn 从未走近的坐标不在当前视野内() {
    // Arrange
    let content = test_content();
    let game_world = build_new_world(&content, 7).expect("测试用默认布局满足全部前置条件");
    let far_away: TorusPos = game_world.world.size.wrap(2000, 1500);
    let player_pos = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家刚生成，必然存在")
        .pos;

    // Act
    let visible = visible_set_at(&game_world.world, player_pos);

    // Assert
    assert!(!visible.contains(far_away));
}

#[test]
fn 从未走近的坐标也未被标记探索() {
    // 对应「从未探索」那一层——渲染时完全不画，交给清屏黑背景表现。
    // Arrange
    let content = test_content();
    let game_world = build_new_world(&content, 7).expect("测试用默认布局满足全部前置条件");
    let layout = *game_world.world.terrain.layout();
    let far_away: TorusPos = game_world.world.size.wrap(2000, 1500);

    // Act & Assert
    assert!(!game_world.world.exploration.is_explored(&layout, far_away));
}
