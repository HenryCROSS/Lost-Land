//! 端到端验收：**走近一座据点，屋里真的立着家具，而且家具有主人。**
//!
//! 所有者原话：
//!
//! > 「建筑需要根据他的类型填入不同的家具，例如箱子，椅子，床，书柜等。」
//! > 「每个物品都会有个主人，一个建筑内的物品通常都是属于某个人的。」
//!
//! 本文件走的是**生产路径**：真实的 `mods/`、真实的世界生成、真实的
//! `materialize_nearby_settlements`。它验四件此前无从验起的事：
//!
//! 1. 据点物化之后，`WorldState::ground_items` 里真的多出了立着的家具；
//! 2. 每一件家具**在生成的那一刻就带着主人**，而且主人就是这座据点；
//! 3. 家具真的落在**屋子里面**（外廓的内壁那一圈），不是撒在野地上；
//! 4. 家具**没有和 NPC 挤在同一格**，也没有两件叠在同一格。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 与 `npc_materialization.rs` 同一条纪律：全程不碰 GPU、不模拟键盘，
//! 直接调用生产路径上的那几个函数，只是跳过了外面那层窗口/输入外壳。

use ll_game::content::LoadedContent;
use ll_game::world::{build_new_world, materialize_nearby_settlements};
use ll_mod::roster::SettlementRoles;
use ll_world::ownership::Owner;
use ll_world::settlement::{BUILDING_SPAN, MAX_BUILDINGS, SettlementStatus, building_origin};

/// 与 `npc_materialization.rs` 同一颗种子：两份文件说的是同一个世界，
/// 一份验人、一份验家具。
const SEED: u64 = 20260826;

/// 测试用内容装载——写法与 `npc_materialization.rs` 的同名帮手一致
/// （集成测试之间看不见彼此的私有帮手，因此这几行在这里重来一遍）。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ll-game-settlement-furniture-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 建一局世界，把流式邻域挪到第一座**还有人住**的据点上，物化它。
///
/// 返回 `(世界, 内容, 那座据点)`。
fn materialised_settlement() -> (
    ll_game::world::GameWorld,
    LoadedContent,
    ll_world::settlement::SettlementSite,
) {
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
    materialize_nearby_settlements(&mut game_world.world, &content, &roles);
    (game_world, content, site)
}

#[test]
fn 据点物化之后屋里真的立着家具() {
    // Arrange & Act
    let (game_world, content, site) = materialised_settlement();

    // Assert ①：真的多出了立着的东西。
    let placed: Vec<_> = game_world
        .world
        .ground_items
        .iter()
        .filter(|item| item.placed)
        .collect();
    assert!(
        !placed.is_empty(),
        "据点（人口 {}、{} 栋建筑）物化之后应当至少立着一件家具",
        site.population,
        site.building_count
    );

    // Assert ②：每一件都是真的家具（`ItemDef.furniture`），不是随手
    // 丢在地上的铁锭。
    for item in &placed {
        let view = content
            .item_table
            .get(item.stack.def)
            .expect("摆下去的东西必须是一件已定义的物品");
        assert!(
            view.furniture,
            "立着的东西必须是家具，实际是 {:?}",
            content.registry.resolve(item.stack.def)
        );
        assert_eq!(item.stack.count, 1, "家具一格一件");
        assert!(item.contents.is_empty(), "本批次的家具肚子是空的");
    }
}

#[test]
fn 每一件家具生成时就带着这座据点的归属() {
    // 这是所有者第三句话的落点：「每个物品都会有个主人，一个建筑内的
    // 物品通常都是属于某个人的」。
    //
    // 反例（ADR 0022，人工验证过）：把 `furnish_settlement` 里
    // `let owner = Owner::Faction(site.id);` 改成 `Owner::Unowned`，
    // 本条当场红。
    // Arrange & Act
    let (game_world, _content, _site) = materialised_settlement();

    // 这一局里被物化过的据点可能不止一座（常驻邻域覆盖得到的都算），
    // 因此判据不是「等于某一个 id」，而是「每一件都指向**某座真实存在
    // 的据点**，且没有一件是无主的」。
    let sites: Vec<u32> = game_world
        .world
        .terrain
        .chronicle_handle()
        .expect("装了编年史")
        .sites()
        .iter()
        .map(|s| s.id.get())
        .collect();

    // Assert
    let placed: Vec<_> = game_world
        .world
        .ground_items
        .iter()
        .filter(|item| item.placed)
        .collect();
    assert!(!placed.is_empty(), "先得有家具才谈得上归属");
    for item in placed {
        match item.stack.owner {
            Owner::Faction(id) => assert!(
                sites.contains(&id.get()),
                "家具的归属 {} 不指向任何一座真实据点",
                id.get()
            ),
            other => panic!("据点家具必须带 Owner::Faction(据点 id)，实际是 {other:?}"),
        }
        assert!(item.stack.owner.is_claimed(), "家具不该是无主的");
    }
}

#[test]
fn 家具落在屋子里面而不是野地上() {
    // Arrange & Act
    let (game_world, _content, _site) = materialised_settlement();
    let chronicle = game_world
        .world
        .terrain
        .chronicle_handle()
        .expect("装了编年史");
    let tile_size = game_world.world.terrain.layout().tile_size();

    // 把全世界每一栋建筑的**内壁**格子收成一份清单。内壁 = 外廓去掉
    // 最外那一圈墙，也就是局部坐标两轴都落在 `1..BUILDING_SPAN-1`。
    let mut interiors: Vec<ll_core::torus::TorusPos> = Vec::new();
    for site in chronicle.sites() {
        for building in 0..site.building_count.min(MAX_BUILDINGS) {
            let (left, top) = building_origin(site, building);
            for dy in 1..BUILDING_SPAN - 1 {
                for dx in 1..BUILDING_SPAN - 1 {
                    interiors.push(tile_size.wrap(left + dx, top + dy));
                }
            }
        }
    }
    interiors.sort_by_key(|p| (p.y(), p.x()));
    interiors.dedup();

    // Assert：每一件家具都站在某栋屋子的内壁上。
    let placed: Vec<_> = game_world
        .world
        .ground_items
        .iter()
        .filter(|item| item.placed)
        .collect();
    assert!(!placed.is_empty());
    for item in placed {
        assert!(
            interiors
                .binary_search_by_key(&(item.pos.y(), item.pos.x()), |p| (p.y(), p.x()))
                .is_ok(),
            "有一件家具落在 {:?}，那不是任何一栋屋子的内壁",
            item.pos
        );
    }
}

#[test]
fn 家具不和npc同格也不互相叠放() {
    // 与「每格至多站一人」那条不变式的相互作用，见
    // `ll_game::settlement_spawn` 模块文档最后一节。
    //
    // 反例（ADR 0022，人工验证过）：把 `furnish_settlement` 里那两行
    // 「跳过被实体占住的格」「跳过已有放置物的格」任意删掉一行，本条
    // 都会红。
    // Arrange & Act
    let (game_world, _content, _site) = materialised_settlement();

    // Assert ①：没有任何一件家具和一个实体同格。
    let actor_positions: Vec<_> = game_world.world.actors.iter().map(|a| a.pos).collect();
    for item in game_world.world.ground_items.iter().filter(|i| i.placed) {
        assert!(
            !actor_positions.contains(&item.pos),
            "家具与实体挤在同一格 {:?}",
            item.pos
        );
    }

    // Assert ②：没有两件家具叠在同一格（放置物独占一格）。
    let mut positions: Vec<(i32, i32)> = game_world
        .world
        .ground_items
        .iter()
        .filter(|i| i.placed)
        .map(|i| (i.pos.y(), i.pos.x()))
        .collect();
    let before = positions.len();
    positions.sort_unstable();
    positions.dedup();
    assert_eq!(before, positions.len(), "同一格上出现了两件放置物");
}

#[test]
fn 每栋有家具的屋子都至少留着一格空地() {
    // 「结构上就有一格永远不参与」那条保证的端到端验收，见
    // `ll_world::building::MAX_FURNITURE_PER_BUILDING` 文档。
    // Arrange & Act
    let (game_world, _content, _site) = materialised_settlement();
    let chronicle = game_world
        .world
        .terrain
        .chronicle_handle()
        .expect("装了编年史");
    let tile_size = game_world.world.terrain.layout().tile_size();
    let mid = BUILDING_SPAN / 2;

    // Assert：每栋屋子的正中格上都没有放置物。
    for site in chronicle.sites() {
        for building in 0..site.building_count.min(MAX_BUILDINGS) {
            let (left, top) = building_origin(site, building);
            let centre = tile_size.wrap(left + mid, top + mid);
            assert!(
                game_world.world.placed_at(centre).is_none(),
                "据点 {} 第 {building} 栋屋子的正中被占住了",
                site.id.get()
            );
        }
    }
}

#[test]
fn 同一座据点物化两次不会多出第二批家具() {
    // 与 NPC 那一条同一个缺陷面：区块淘汰再加载不能让家具翻倍。
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
    let anchor = {
        let chronicle = game_world
            .world
            .terrain
            .chronicle_handle()
            .expect("装了编年史");
        chronicle
            .sites()
            .iter()
            .find(|s| s.status == SettlementStatus::Inhabited && s.population > 0)
            .expect("至少一座")
            .anchor
    };
    let clock = game_world.world.clock;
    game_world.world.terrain.stream_neighborhood(
        &game_world.noise,
        &game_world.params,
        &content.terrain_ids,
        anchor,
        ll_game::world::STREAM_RADIUS_ZONES,
        clock,
    );

    // Act
    materialize_nearby_settlements(&mut game_world.world, &content, &roles);
    let first = game_world.world.ground_items.len();
    materialize_nearby_settlements(&mut game_world.world, &content, &roles);
    let second = game_world.world.ground_items.len();

    // Assert
    assert!(first > 0, "第一次物化应当摆下家具");
    assert_eq!(first, second, "第二次物化不该再摆一遍");
}

#[test]
fn 玩家不会在谁家屋里开局() {
    // 这修的是一个**先于本批次就存在**的缺陷：`find_spawn_site` 读的是
    // 基础地形（据点还没盖上去），因此光栅序最先那一格可能正好落在某栋
    // 屋子的 3×3 内壁里。街道把建筑摊开之后
    // `crates/ll-game/tests/worldgen_params_e2e.rs` 的
    // 「四档预设都能建出带玩家实体且出生点连得开的世界」当场红了
    // （「预设 continent 的出生点周围只有 9 格连通可行走地面」）。
    //
    // 反例（ADR 0022，人工验证过）：把 `build_new_world` 里那一行
    // `spawn_outside_buildings` 去掉，本条与那一条同时红。
    // Arrange & Act
    let content = test_content();
    let game_world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("建世界应当成功");
    let player = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("新游戏必有玩家");
    let tile_size = game_world.world.terrain.layout().tile_size();
    let chronicle = game_world
        .world
        .terrain
        .chronicle_handle()
        .expect("装了编年史");

    // Assert：玩家那一格不落在任何一栋建筑的外廓里。
    for site in chronicle.sites() {
        for building in 0..site.building_count.min(MAX_BUILDINGS) {
            let (left, top) = building_origin(site, building);
            for dy in 0..BUILDING_SPAN {
                for dx in 0..BUILDING_SPAN {
                    assert_ne!(
                        tile_size.wrap(left + dx, top + dy),
                        player.pos,
                        "玩家在据点 {} 第 {building} 栋屋子里开局了",
                        site.id.get()
                    );
                }
            }
        }
    }
}
