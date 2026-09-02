//! 端到端验收：**文化敌意决定「走进对方那一格」是攻击还是互换。**
//!
//! 项目所有者的裁定原话（四条，本批次全部落在这里）：
//!
//! > 「玩家可以没有势力归属，这个可以通过后面和据点的管理者对话加入。」
//! > 「或者利用现有机制，哥布林种族默认对其他种族仇恨度非常高。仇恨度
//! >   高过某个值就会视为敌对。这样应该就可以了。」
//! > 「那文化设定一个独特的东西叫无文化，这东西颗粒度小到具体某个 NPC。」
//! > 敌对阈值：「我觉得 5 也没问题。」
//!
//! **2026-08-27 追加的第五条裁定（推翻了本文件此前的一条断言）**：
//!
//! > 「关于矮人那个，整体机制应该是**只要有一方处于敌对状态，另一方
//! >   也会发起攻击**」
//!
//! 落点是 `ll_sim::ai_query::culture_declares_hostile` 改取两个方向的
//! 最大值，`矮人矿工撞哥布林不是攻击` 因此反转为
//! `矮人矿工撞哥布林也是攻击`。**编年史层的战争推演仍然有向，本次
//! 一个字都没改。**
//!
//! 全部用例走**真实 `mods/` 内容**（敌意分 6/4/3 是
//! `mods/lostland/cultures.json5` 里真的写着的那几个数，不是夹具编的）
//! 与生产路径上的 `build_new_world` + `TurnEngine`，与本体二进制
//! `ll_game::app::Demo::advance` 走同一串调用（ADR 0018）。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 同 `bump_into_occupant.rs`：直接提交 `Intent::Move`，不模拟键盘。
//!
//! # 这几条断言真的会红吗——故意改坏的反例（人工核验，真实执行）
//!
//! 逐条记在各自的测试文档里，见本批次提交信息。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_game::content::{LoadedContent, RuntimeCatalogs};
use ll_game::world::{build_new_world, materialize_nearby_settlements};
use ll_mod::roster::SettlementRoles;
use ll_sim::intent::{Direction, Intent};
use ll_sim::timeline::Timeline;
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_world::culture::CultureKind;
use ll_world::entity::{Affiliation, AffiliationKind, EntityId, OrgRef};
use ll_world::settlement::SettlementStatus;
use ll_world::state::WorldState;

/// 固定种子，理由同 `bump_into_occupant.rs` 的同名常量。
const SEED: u64 = 20260826;

/// 本文件按名字引用的那几条本体内容 id。抽成常量而不是散在断言里，
/// 理由同 `culture_and_war.rs` 的同名一组：两处各写一份字面量迟早会
/// 分叉。
const GOBLIN_WARBAND: &str = "lostland:goblin_warband";
const MINING_HOLD: &str = "lostland:mining_hold";
const FARMSTEAD: &str = "lostland:farmstead";
const CULTURELESS: &str = ll_mod::base_cultureless::CULTURELESS_CULTURE_ID;

/// 测试用内容装载——写法与 `bump_into_occupant.rs`/`npc_materialization.rs`
/// 的同名帮手一致（集成测试之间看不见彼此的私有帮手）。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ll-game-culture-hostility-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 按 id 查一条内容索引。
fn index_of(content: &LoadedContent, raw: &str) -> ContentIndex {
    content
        .registry
        .get(&NamespacedId::parse(raw).expect("测试用标识符恒合法"))
        .unwrap_or_else(|| panic!("本体内容必须注册过 {raw}"))
}

/// 给一个实体挂一条文化归属——与 `ll_mod::roster::build_npc_agent`
/// 挂的**形状完全相同**（`Culture` + `OrgRef::Def` + 满声望）。
fn join_culture(world: &mut WorldState, actor: EntityId, culture: ContentIndex) {
    world
        .actors
        .get_mut(actor)
        .expect("实体存在")
        .affiliations
        .push(Affiliation {
            kind: AffiliationKind::Culture,
            org: OrgRef::Def(culture),
            standing: ll_mod::roster::NATIVE_CULTURE_STANDING,
        });
}

/// 建一局世界，并在玩家**东侧相邻格**放一个实体（克隆玩家的 `Agent`
/// 改坐标，理由同 `bump_into_occupant.rs` 的同名帮手）。
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

/// 再往东加一个实体：`(世界, 内容, 东邻, 东邻的东邻)`——两个 NPC
/// 互撞的用例需要两个**都不是玩家**的实体。
fn world_with_two_npcs() -> (ll_game::world::GameWorld, LoadedContent, EntityId, EntityId) {
    let (mut game_world, content, mover) = world_with_neighbour();
    let mut agent = game_world.world.actors.get(mover).expect("东邻在").clone();
    agent.pos = game_world.world.size.wrap(agent.pos.x() + 1, agent.pos.y());
    let target = game_world.world.actors.spawn(agent);
    (game_world, content, mover, target)
}

/// 让玩家朝东提交一次移动——`Demo::advance` 里那一串调用的最小等价物。
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

/// 让一个**非受控实体**朝东走一步：把它排在玩家前面，跑一次
/// `advance_ai`——与真实游戏里 NPC 轮到自己时走的是同一条路径（撞格
/// 路由对 NPC 同样生效，见 `ll_sim::turn` 的撞格路由文档）。
///
/// 玩家排在**下一个 tick**，因此 `advance_ai` 只会结算 `mover` 一次
/// 就轮到玩家、当场返回。把玩家排得更远会让 `mover` 连着行动很多次
/// ——攻击那条用例里目标会被打死，`health_of` 随即在「实体还在」上
/// 恐慌，那是夹具的锅不是判据的锅。
fn npc_steps_east(
    game_world: &mut ll_game::world::GameWorld,
    content: &LoadedContent,
    mover: EntityId,
) {
    let player = game_world.player;
    let clock = game_world.world.clock;
    let mut timeline = Timeline::new();
    timeline.schedule(mover, clock);
    timeline.schedule(player, ll_core::time::Tick(clock.0 + 1));
    let mut engine = TurnEngine::new(timeline);
    let runtime = RuntimeCatalogs::new(content);
    let catalogs = runtime.as_resolve_catalogs();
    let mut on_effect = |_world: &WorldState, _effect: &ll_sim::effect::Effect| {};
    engine.advance_ai(
        &mut game_world.world,
        player,
        &mut move |_world: &WorldState, actor: EntityId, _controlled: EntityId| {
            if actor == mover {
                Intent::Move {
                    actor,
                    dir: Direction::East,
                }
            } else {
                Intent::Wait { actor }
            }
        },
        &catalogs,
        &mut on_effect,
    );
}

fn pos_of(world: &WorldState, actor: EntityId) -> ll_core::torus::TorusPos {
    world.actors.get(actor).expect("实体还在").pos
}

fn health_of(world: &WorldState, actor: EntityId) -> i32 {
    world.actors.get(actor).expect("实体还在").health
}

// ── 一、玩家能直接观察到的那两条 ─────────────────────────────────

#[test]
fn 没有任何归属的玩家走向哥布林是攻击而不是互换() {
    // **本批次唯一能被玩家直接观察到的后果**，也是所有者裁定第 2 条
    // 「哥布林仇恨度高过某个值就视为敌对」的落点。
    //
    // 玩家身上一条归属都没有（`build_player_agent` 写死 `Vec::new()`，
    // 所有者裁定「玩家可以没有势力归属」），因此判定时回退到「无文化」
    // 哨兵；`goblin_warband` 对无文化声明了 6，6 ≥ 5 → 敌对。
    //
    // 故意改坏的反例（人工核验）：把
    // `mods/lostland/cultures.json5` 里那条 `lostland:cultureless`
    // 敌意从 6 改成 4，本条当场变红（变成互换）。
    // Arrange
    let (mut game_world, content, neighbour) = world_with_neighbour();
    let goblin = index_of(&content, GOBLIN_WARBAND);
    join_culture(&mut game_world.world, neighbour, goblin);
    let player = game_world.player;
    let player_before = pos_of(&game_world.world, player);
    let neighbour_health_before = health_of(&game_world.world, neighbour);

    // Act
    let outcome = player_steps_east(&mut game_world, &content);

    // Assert
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(
        pos_of(&game_world.world, player),
        player_before,
        "攻击不移动：玩家应当留在原地"
    );
    assert!(
        health_of(&game_world.world, neighbour) < neighbour_health_before,
        "哥布林应当挨了一刀，实测血量 {} 未低于 {}",
        health_of(&game_world.world, neighbour),
        neighbour_health_before
    );
}

#[test]
fn 没有任何归属的玩家走向农夫仍然是互换位置() {
    // 阈值 5 把这一半挡住了：`farmstead` 一条敌意都没声明，对无文化
    // 自然是 0。这条变红说明阈值被调低、或者判据写反了（例如错把
    // 「没有声明」当成「敌对」）。
    //
    // 故意改坏的反例（人工核验）：把
    // `ll_sim::ai_query::culture_declares_hostile` 的比较从
    // `score >= HOSTILE_CULTURE_THRESHOLD` 改成 `true`，本条当场变红。
    // Arrange
    let (mut game_world, content, neighbour) = world_with_neighbour();
    let farmstead = index_of(&content, FARMSTEAD);
    join_culture(&mut game_world.world, neighbour, farmstead);
    let player = game_world.player;
    let player_before = pos_of(&game_world.world, player);
    let neighbour_before = pos_of(&game_world.world, neighbour);
    let neighbour_health_before = health_of(&game_world.world, neighbour);

    // Act
    let outcome = player_steps_east(&mut game_world, &content);

    // Assert
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(
        pos_of(&game_world.world, player),
        neighbour_before,
        "玩家应当站到农夫原来那一格"
    );
    assert_eq!(
        pos_of(&game_world.world, neighbour),
        player_before,
        "农夫应当被换到玩家原来那一格"
    );
    assert_eq!(
        health_of(&game_world.world, neighbour),
        neighbour_health_before,
        "农夫一点血都不该掉"
    );
}

// ── 二、NPC 之间：内容声明有向，撞格判定对称 ─────────────────────
//
// **本节在 2026-08-27 被所有者裁定改写过。** 原标题是「那份刻意的
// 不对称」，原意是撞格路由只看「发起者朝目标」那一个方向。所有者实机
// 试玩后裁定「只要有一方处于敌对状态，另一方也会发起攻击」，判据改为
// 两个方向取最大值。`mining_hold → goblin_warband` 那个刻意写低的 3
// 一个字没改，仍然在**编年史层**决定出兵与否，见
// `ll_sim::ai_query::declared_hostile` 文档「内容声明有向、战斗判定
// 对称」一节。

#[test]
fn 哥布林撞哥布林不敌对() {
    // 同文化：`goblin_warband` 的敌意表里没有指向自己的条目，
    // `CultureTable::hostility` 查不到即 0。这条守的是「哥布林不会
    // 在自己营地里内讧」。
    //
    // **对称化之后这条更值钱了**：判据现在取两个方向的最大值，但两侧
    // 各自的文化都是 `goblin_warband`（哨兵只在某一方**没有**文化时
    // 才顶上），两个方向查的都是 `goblin_warband → goblin_warband`，
    // 表里没这一条即 0。这条守的就是「对称化不等于让哨兵到处乱入」。
    //
    // 故意改坏的反例（人工核验）：把 `culture_declares_hostile` 里的
    // `culture_of(a).unwrap_or(cultureless)` 改成无条件 `cultureless`
    // （让哨兵替掉发起者真实的文化），本条当场变红——因为哥布林对
    // 无文化是 6。
    // Arrange
    let (mut game_world, content, mover, target) = world_with_two_npcs();
    let goblin = index_of(&content, GOBLIN_WARBAND);
    join_culture(&mut game_world.world, mover, goblin);
    join_culture(&mut game_world.world, target, goblin);
    let target_health_before = health_of(&game_world.world, target);

    // Act
    npc_steps_east(&mut game_world, &content, mover);

    // Assert
    assert_eq!(
        health_of(&game_world.world, target),
        target_health_before,
        "同族哥布林之间不该动手"
    );
}

#[test]
fn 哥布林撞矮人矿工是攻击() {
    // `goblin_warband → mining_hold` 敌意 6，6 ≥ 5 → 敌对。这条与
    // 下一条合起来才是 D5 的核心证据：同一对文化，换个方向答案不同。
    //
    // 故意改坏的反例（人工核验）：把
    // `ll_sim::ai_query::HOSTILE_CULTURE_THRESHOLD` 从 5 改成 7，
    // 本条当场变红。
    // Arrange
    let (mut game_world, content, mover, target) = world_with_two_npcs();
    join_culture(
        &mut game_world.world,
        mover,
        index_of(&content, GOBLIN_WARBAND),
    );
    join_culture(
        &mut game_world.world,
        target,
        index_of(&content, MINING_HOLD),
    );
    let target_health_before = health_of(&game_world.world, target);

    // Act
    npc_steps_east(&mut game_world, &content, mover);

    // Assert
    assert!(
        health_of(&game_world.world, target) < target_health_before,
        "哥布林走向矮人矿工应当动手，实测血量 {} 未低于 {}",
        health_of(&game_world.world, target),
        target_health_before
    );
}

#[test]
fn 矮人矿工撞哥布林也是攻击() {
    // **本条在 2026-08-27 被所有者裁定反转过。** 原断言是「矮人矿工撞
    // 哥布林**不是**攻击」，理由是撞格路由取「发起者朝目标」那一个方向
    // 而 `mining_hold → goblin_warband` 只有 3。所有者实机试玩后裁定：
    // 「整体机制应该是**只要有一方处于敌对状态，另一方也会发起攻击**」。
    //
    // 判据因此改成两个方向取最大值：`max(3, 6) = 6 >= 5` → 敌对。
    // 哥布林砍得动矮人，矮人就还得了手——不再是「哥布林砍矮人，矮人
    // 撞回去却只是一次失败的移动」。
    //
    // **那个刻意写低的 3 没有作废，只是换了一层**：它照常决定编年史层
    // 「矮人矿邑会不会主动出兵讨伐哥布林」（`ll_world::chronicle`
    // 的 `hostility_between`/`wage_wars`/`pick_target` 仍然按有向读表，
    // 本次一个字都没改）。见 `ll_sim::ai_query::declared_hostile` 文档
    // 「内容声明有向、战斗判定对称」一节。
    //
    // 与上一条 `哥布林撞矮人矿工是攻击` 合起来，两条钉住的是**对称性**
    // 本身：同一对文化，换个方向答案相同。
    //
    // 故意改坏的反例（人工核验）：把 `culture_declares_hostile` 里的
    // `.max(cultures.hostility(Some(target), Some(mover)))` 删掉、退回
    // 有向判据，本条当场变红。
    // Arrange
    let (mut game_world, content, mover, target) = world_with_two_npcs();
    join_culture(
        &mut game_world.world,
        mover,
        index_of(&content, MINING_HOLD),
    );
    join_culture(
        &mut game_world.world,
        target,
        index_of(&content, GOBLIN_WARBAND),
    );
    let target_health_before = health_of(&game_world.world, target);

    // Act
    npc_steps_east(&mut game_world, &content, mover);

    // Assert
    assert!(
        health_of(&game_world.world, target) < target_health_before,
        "矮人矿工走向哥布林应当动手（哥布林那边的 6 把这一对拖进了敌对），         实测血量 {} 未低于 {}",
        health_of(&game_world.world, target),
        target_health_before
    );
}

// ── 三、物化：`Agent::affiliations` 的第一个生产者 ───────────────

#[test]
fn 物化出来的npc身上带着所属据点的文化归属() {
    // D3 的活证据。在此之前 `Agent::affiliations` 零生产者，两条构造
    // 路径都写死 `Vec::new()`。
    //
    // 故意改坏的反例（人工核验）：把
    // `ll_game::world::materialize_nearby_settlements` 里那行
    // `culture: site.culture` 改成 `culture: None`，本条当场变红。
    // Arrange
    let content = test_content();
    let mut game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("建世界应当成功");
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
            .find(|site| {
                site.status == SettlementStatus::Inhabited
                    && site.population > 0
                    && site.culture.is_some()
            })
            .expect("三百年历史必然留下至少一座还有人住、且有文化的据点")
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

    // Act
    let spawned = materialize_nearby_settlements(&mut game_world.world, &content, &roles);

    // Assert
    assert!(!spawned.is_empty(), "还有人住的据点应当物化出 NPC");
    let mut checked = 0usize;
    for id in &spawned {
        let agent = game_world.world.actors.get(*id).expect("刚物化的实体在");
        let culture: Vec<ContentIndex> = agent
            .affiliations
            .iter()
            .filter(|aff| aff.kind == AffiliationKind::Culture)
            .map(|aff| match aff.org {
                OrgRef::Def(index) => index,
                OrgRef::Instance(_) => panic!("Culture 归属恒走 OrgRef::Def"),
            })
            .collect();
        assert_eq!(
            culture.len(),
            1,
            "每个物化出来的 NPC 恰好带一条文化归属，实测 {}",
            culture.len()
        );
        // 常驻邻域里可能不止一座据点，只核对确实属于某座**有文化**的
        // 据点：这里断言的是「不是占位、不是空」，逐个对回具体哪一座
        // 需要把物化返回值按据点分组，那是 `npc_materialization.rs`
        // 的职责。
        assert!(
            game_world
                .world
                .terrain
                .chronicle_handle()
                .expect("编年史在")
                .sites()
                .iter()
                .any(|s| s.culture.map(CultureKind::index) == Some(culture[0])),
            "NPC 身上那条文化必须真的是某座据点的文化"
        );
        checked += 1;
    }
    assert!(checked > 0, "至少核对过一个 NPC，否则本条是空转");
}

#[test]
fn 没有文化的据点物化出来的npc不挂归属() {
    // D2：无文化是**查询期回退**，不是写进每个实体的归属。据点没有
    // 文化时不挂——不伪造一条指向哨兵的归属。
    //
    // 用一个空文化表的世界来造这个情形：`CultureTable::new()` 是合法
    // 的空表（「这个世界没有文化这一层」），此时每座据点的 `culture`
    // 都是 `None`。
    //
    // 故意改坏的反例（人工核验）：把 `build_npc_agent` 里那段
    // `ctx.culture.map(..)` 改成 `unwrap_or(哨兵)` 无条件挂一条，
    // 本条当场变红。
    // Arrange
    let races = ll_mod::race::RaceTable::new();
    let items = ll_sim::item::NoItems;
    let size = ll_core::torus::TorusSize::new(64, 64).expect("64x64 是合法尺寸");
    let profile = ll_mod::roster::NpcProfile {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        home: ll_core::ident::WorldId::next(&mut 1u32),
        roster_index: 0,
        race: ContentIndex::default(),
        profession: ContentIndex::default(),
    };
    let roles = SettlementRoles::resolve(
        &ll_mod::registry::Registry::new(),
        &ll_mod::class::ClassTable::new(),
        &ll_world::resource::ResourceTable::new(),
        &ll_world::culture::CultureTable::new(),
    );
    let make = |culture: Option<CultureKind>| {
        let ctx = ll_mod::roster::MaterializeContext {
            races: &races,
            items: &items,
            surface_profile: ContentIndex::default(),
            now: ll_core::time::Tick(0),
            culture,
            // 本用例只看文化归属，人口取一个非零值即可（钱包按它派生，
            // 但这里不断言钱包）。
            population: 1,
        };
        ll_mod::roster::build_npc_agent(&profile, size.wrap(0, 0), size.wrap(0, 0), &roles, &ctx)
    };

    // Act
    let without_culture = make(None);
    let with_culture = make(Some(CultureKind::from_index(ContentIndex::default())));

    // Assert
    assert!(
        without_culture.affiliations.is_empty(),
        "没有文化的据点物化出来的 NPC 身上不该有任何归属，实测 {:?}",
        without_culture.affiliations
    );
    // 对照组：同一条路径、只把 `culture` 换成 `Some`，归属就挂上了
    // ——证明上面那条不是因为整段挂归属的代码根本没跑而空转成真。
    assert_eq!(
        with_culture.affiliations.len(),
        1,
        "据点有文化时应当恰好挂一条归属"
    );
    assert_eq!(with_culture.affiliations[0].kind, AffiliationKind::Culture);
}

// ── 四、哨兵永远不会被选为建城文化 ──────────────────────────────

#[test]
fn 无文化哨兵永远不会被选为建城文化() {
    // D1 的核心证据：`lostland:cultureless` 只 `intern`、不 `define`，
    // 因此不在 `CultureTable::registered()` 里，而 `pick_culture` 只
    // 遍历那个列表——世界生成的权重与掷骰序列一个字节都不受影响。
    //
    // 故意改坏的反例（人工核验）：给
    // `mods/lostland/cultures.json5` 真加一条 id 为
    // `lostland:cultureless` 的文化定义（配一个建立者种族让
    // `define` 过校验），本条当场变红。
    // Arrange
    let content = test_content();
    let cultureless = index_of(&content, CULTURELESS);
    let game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("建世界应当成功");
    let chronicle = game_world
        .world
        .terrain
        .chronicle_handle()
        .expect("新游戏必然装了编年史");

    // Act & Assert
    let sites = chronicle.sites();
    assert!(!sites.is_empty(), "三百年历史必然留下据点，否则本条是空转");
    let with_culture = sites.iter().filter(|s| s.culture.is_some()).count();
    assert!(
        with_culture > 0,
        "至少要有一座据点抽到了文化，否则本条是空转"
    );
    for site in sites {
        assert_ne!(
            site.culture.map(CultureKind::index),
            Some(cultureless),
            "据点 {} 抽到了「无文化」哨兵——它本该压根不在候选集里",
            site.id.get()
        );
    }
    // 同一条性质在表这一侧的直接表述：哨兵不在注册顺序列表里。
    assert!(
        !content
            .culture_table
            .registered()
            .iter()
            .any(|kind| kind.index() == cultureless),
        "哨兵不该出现在 CultureTable::registered() 里"
    );
    // 但它确实**被注册进了 registry**、也确实被记进了文化表——否则
    // 上面那两条会因为「压根没这个东西」而空转成真。
    assert_eq!(
        content.culture_table.cultureless(),
        Some(cultureless),
        "文化表必须记着这次会话的哨兵索引，否则敌意判据整个不生效"
    );
}
