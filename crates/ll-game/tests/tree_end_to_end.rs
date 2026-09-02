//! 树木系统的**端到端**证据（[ADR 0031]）：真实 `mods/` + 生产路径 +
//! 一路验到可观察产出。
//!
//! # 三条各自落在哪
//!
//! - **真实内容**：`ll_game::content::load_content` 读仓库真实
//!   `mods/`，两件产出物（`lostland:timber_log` / `lostland:tree_seed`）
//!   与森林地形都从那里出来，不内联任何夹具。
//! - **生产路径**：`interact_entries` 产出行 → `player_command` 产出
//!   `Intent::TendTree` → `TurnEngine::try_player_intent` 结算并写世界。
//!   **中间一步都不跳过**，不在测试里另推一遍等价逻辑。
//! - **端到端**：从「玩家站在树前」一路验到世界状态里的偏差记录、背包里
//!   的木料与种子。
//!
//! # 与 `crates/ll-sim/tests/tend_tree.rs` 的分工
//!
//! 那一份验的是**结算规则**（闸门、产量、回合），用最小夹具，跑得快。
//! 本文件验的是**接线**：那些规则在真实游戏里真的被接上了吗。两者都要
//! ——仓库记过三次「只在测试里成立的接线」（击杀经验、副职、对话），
//! 症状全都是「集成测试全绿，跑起来那条玩法根本不存在」。
//!
//! [ADR 0031]: ../../../knowledge/decisions/0031-end-to-end-evidence-through-real-content.md

use std::path::Path;

use ll_core::ident::NamespacedId;
use ll_core::torus::TorusPos;
use ll_game::content::RuntimeCatalogs;
use ll_game::content::{LoadedContent, load_content};
use ll_game::interact_list::{InteractLookup, InteractTarget, interact_entries};
use ll_game::player_action::{PlayerCommand, PlayerMenu, player_command};
use ll_game::world::{GameWorld, build_new_world};
use ll_mod::tree::RegisteredTrees;
use ll_platform::input::{GameKey, InputState};
use ll_sim::intent::Intent;
use ll_sim::timeline::Timeline;
use ll_sim::tree::{TreeAction, TreeCatalog};
use ll_sim::turn::TurnEngine;
use ll_world::entity::EntityId;
use ll_world::item::ItemStack;
use ll_world::state::WorldState;
use ll_world::tree::{TreeDeviation, tree_at};

/// 仓库根。与 `dialogue_session.rs` 同一条走法。
fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// **生产装载路径**：真实 `mods/` + 真实 `assets/`（ADR 0031 第 1 条）。
fn real_content() -> LoadedContent {
    let root = repo_root();
    load_content(&root.join("mods"), &root.join("assets")).expect("仓库真实 mods/ 必须装得起来")
}

fn lookup<'a>(content: &'a LoadedContent) -> InteractLookup<'a> {
    // **与生产代码同一份构造**（`ll_game::app` 与 `app::hud_draw` 那两处
    // 逐字相同）：森林取本体地形索引缓存，种子走 `RegisteredTrees`。
    InteractLookup {
        dialogues: &content.dialogue_table,
        cultures: Some(&content.culture_table),
        forest: Some(content.terrain_ids.forest),
        tree_seed: RegisteredTrees {
            registry: &content.registry,
        }
        .tree_seed_index(),
    }
}

fn index(content: &LoadedContent, id: &str) -> ll_core::ident::ContentIndex {
    content
        .registry
        .get(&NamespacedId::parse(id).expect("字面量合法"))
        .unwrap_or_else(|| panic!("{id} 必须已注册"))
}

/// 造一个真实世界，把玩家够得着的那一圈铺成森林，并挑出其中**派生层
/// 真的长出了树**的那一格。
///
/// # 两处刻意的做法
///
/// 1. **铺地形**而不是「在世界里找一格森林」：出生点附近有没有森林取决
///    于噪声，依赖那种巧合的测试会在换一次生成参数之后**静默**失去覆盖面。
/// 2. **挑一格派生出树的**，而不是写一条 `TreeDeviation` 把树摆上去。
///    写偏差记录会让本文件退化成「只验偏差层」——而砍伐这条链要验的第一
///    件事恰恰是「派生出来的树也砍得动」。密度是 620‰，一圈八格里必有
///    几格有树；一格都没有时下面那条 `expect` 会点名说清楚。
fn 世界与一棵树(content: &LoadedContent) -> (GameWorld, EntityId, TorusPos) {
    let mut world = build_new_world(content, ll_world::generate::GenParams::default())
        .expect("默认预设建得起来");
    let player = world.player;
    let here = world.world.actors.get(player).expect("玩家存在").pos;
    // 玩家够得着的那一圈（不含脚下——脚下那一格留给
    // `不是森林的格子上一行树都没有` 当对照）。
    let 一圈: Vec<TorusPos> = [(1, 0), (0, 1), (1, 1), (-1, 0), (0, -1), (-1, -1)]
        .into_iter()
        .map(|(dx, dy)| world.world.size.wrap(here.x() + dx, here.y() + dy))
        .collect();
    for pos in &一圈 {
        world
            .world
            .terrain
            .set_terrain(*pos, content.terrain_ids.forest);
    }
    let 树位 = 一圈
        .into_iter()
        .find(|pos| tree_at(&world.world, *pos, content.terrain_ids.forest).is_some())
        .expect(
            "铺成森林的那一圈里一棵派生出来的树都没有——密度是 620‰，六格全空的             概率约 0.3%。真撞上了就换一个种子，并把新种子与这段理由写在这里",
        );
    (world, player, 树位)
}

/// 走**完整生产链**提交一次交互：`interact_entries` 挑行 →
/// `player_command` 产意图 → `TurnEngine` 结算并写世界。
///
/// 返回那次真正被提交的意图，供断言核对「列表里那一行按下去提交的确实
/// 是它看起来该提交的那条」。
fn 按下这一行(
    world: &mut GameWorld,
    content: &LoadedContent,
    engine: &mut TurnEngine,
    actor: EntityId,
    pos: TorusPos,
    want: TreeAction,
) -> Intent {
    let rows = interact_entries(&world.world, pos, actor, lookup(content));
    let row_index = rows
        .iter()
        .position(|row| matches!(row, InteractTarget::Tree { action, .. } if *action == want))
        .unwrap_or_else(|| panic!("交互列表里没有 {want:?} 那一行，实际 {rows:?}"));

    let mut menu = PlayerMenu::Interact {
        pos,
        cursor: row_index,
        from_direction: false,
    };
    let mut input = InputState::new();
    input.press(GameKey::Confirm);
    let command = player_command(
        &mut menu,
        &input,
        &world.world,
        actor,
        &content.recipe_table,
        lookup(content),
    );
    let PlayerCommand::Submit(intent) = command else {
        panic!("按确认应当提交一条意图，实际 {command:?}");
    };
    // **走生产那一束目录**（`RuntimeCatalogs::as_resolve_catalogs`），
    // 不在这里手搭一束——树木那一路的接线点正在那里面，手搭一束就等于
    // 把要验的东西替换掉了。
    let runtime = RuntimeCatalogs::new(content);
    let catalogs = runtime.as_resolve_catalogs();
    let mut on_effect = |_w: &WorldState, _e: &ll_sim::effect::Effect| {};
    let mut ai = |_w: &WorldState, a: EntityId, _c: EntityId| Intent::Wait { actor: a };
    engine.advance_ai(&mut world.world, actor, &mut ai, &catalogs, &mut on_effect);
    let outcome =
        engine.try_player_intent(&mut world.world, actor, intent, &catalogs, &mut on_effect);
    assert!(
        !matches!(outcome, ll_sim::turn::PlayerTurnOutcome::Nothing),
        "{want:?} 走完生产链之后什么都没发生——接线断了"
    );
    intent
}

/// 一个「轮到玩家」的回合引擎——与 `dialogue_session.rs` 同一条走法。
fn 引擎(world: &GameWorld) -> TurnEngine {
    let mut timeline = Timeline::new();
    timeline.schedule(world.player, world.world.clock);
    TurnEngine::new(timeline)
}

fn 背包里的(world: &WorldState, actor: EntityId, def: ll_core::ident::ContentIndex) -> u32 {
    world
        .actors
        .get(actor)
        .expect("实体存在")
        .inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

#[test]
fn 真实内容装载之后三个树木索引都解析得到() {
    // **这一条挡的是「静默失效」**：`RegisteredTrees` 任何一条查询返回
    // `None`，树的整条玩法就恒产出空效果——玩家按下砍伐什么都不发生，
    // **不报错、不打日志**。
    //
    // 本仓库刻意没有把这三条升成 `base_contract` 的硬性装载失败（范围
    // 理由见 `ll_mod::tree` 模块文档），代价就换成了这一条会红的断言。
    // Arrange
    let content = real_content();
    let trees = RegisteredTrees {
        registry: &content.registry,
    };

    // Act & Assert
    assert!(
        trees.forest_terrain().is_some(),
        "lostland:forest 解析不到——树永远长不出来"
    );
    assert!(
        trees.timber().is_some(),
        "lostland:timber_log 解析不到——砍伐永远零效果"
    );
    assert!(
        trees.tree_seed().is_some(),
        "lostland:tree_seed 解析不到——采果与培植永远零效果"
    );
    // 森林那一条必须与本体地形索引缓存是**同一个**索引，否则
    // `resolve_tend_tree` 判「这一格是不是森林」会永远判假。
    assert_eq!(
        trees.forest_terrain(),
        Some(content.terrain_ids.forest.index()),
        "TreeCatalog 给的森林索引与 BaseTerrainIds 对不上"
    );
}

#[test]
fn 玩家从交互列表砍倒一棵树木料真的进了背包() {
    // Arrange
    let content = real_content();
    let (mut world, player, 树位) = 世界与一棵树(&content);
    let timber = index(&content, "lostland:timber_log");
    let mut engine = 引擎(&world);
    let tree = tree_at(&world.world, 树位, content.terrain_ids.forest)
        .unwrap_or_else(|| panic!("前提：({},{}) 该有一棵派生出来的树", 树位.x(), 树位.y()));
    assert_eq!(
        背包里的(&world.world, player, timber),
        0,
        "前提：一开始没有木料"
    );

    // Act：走完整生产链。
    let intent = 按下这一行(
        &mut world,
        &content,
        &mut engine,
        player,
        树位,
        TreeAction::Fell,
    );

    // Assert：提交的确实是砍伐那条意图。
    assert!(
        matches!(
            intent,
            Intent::TendTree {
                action: TreeAction::Fell,
                ..
            }
        ),
        "提交的意图不是砍伐，实际 {intent:?}"
    );
    // 树没了——而且**是偏差层写进世界状态**，不是派生层变了。
    assert!(
        tree_at(&world.world, 树位, content.terrain_ids.forest).is_none(),
        "砍完树还在"
    );
    assert_eq!(
        world.world.trees.get(树位),
        Some(TreeDeviation::felled()),
        "偏差记录没有写进世界状态"
    );
    // 木料按树种产量进背包。
    assert_eq!(
        背包里的(&world.world, player, timber),
        tree.species.timber_yield(),
        "{:?} 该出 {} 份木料",
        tree.species,
        tree.species.timber_yield()
    );
}

#[test]
fn 玩家从交互列表采果之后种子真的进了背包() {
    // Arrange
    let content = real_content();
    let (mut world, player, 树位) = 世界与一棵树(&content);
    let seed = index(&content, "lostland:tree_seed");
    let mut engine = 引擎(&world);
    assert!(
        tree_at(&world.world, 树位, content.terrain_ids.forest).is_some(),
        "前提：那一格该有树"
    );

    // Act
    按下这一行(
        &mut world,
        &content,
        &mut engine,
        player,
        树位,
        TreeAction::Harvest,
    );

    // Assert
    assert_eq!(背包里的(&world.world, player, seed), 1, "该拿到一颗种子");
    assert!(
        tree_at(&world.world, 树位, content.terrain_ids.forest).is_some(),
        "采果不该把树采没了"
    );
    // **采完之后交互列表上的「采果」那一行必须消失**——列表与结算必须
    // 一致，否则玩家会看到一行按下去什么都不发生的假选项。
    let rows = interact_entries(&world.world, 树位, player, lookup(&content));
    assert!(
        !rows.iter().any(|row| matches!(
            row,
            InteractTarget::Tree {
                action: TreeAction::Harvest,
                ..
            }
        )),
        "果子刚采过，采果那一行不该还在，实际 {rows:?}"
    );
}

#[test]
fn 玩家从交互列表种下一颗种子长出一棵树() {
    // Arrange：一格空森林 + 背包里一颗真实内容的种子。
    let content = real_content();
    let (mut world, player, 树位) = 世界与一棵树(&content);
    let seed = index(&content, "lostland:tree_seed");
    world.world.trees.set(树位, TreeDeviation::felled());
    world
        .world
        .actors
        .get_mut(player)
        .expect("玩家存在")
        .inventory
        .push(ItemStack::new(seed, 1));
    let mut engine = 引擎(&world);
    assert!(
        tree_at(&world.world, 树位, content.terrain_ids.forest).is_none(),
        "前提：那一格该是空地"
    );

    // Act
    按下这一行(
        &mut world,
        &content,
        &mut engine,
        player,
        树位,
        TreeAction::Plant,
    );

    // Assert
    let tree = tree_at(&world.world, 树位, content.terrain_ids.forest).expect("种下之后该有树");
    assert_eq!(背包里的(&world.world, player, seed), 0, "种子该被消耗掉");
    // 长出来的树种由**那块地的气候**决定，与派生层同一个函数。
    assert_eq!(
        tree.species,
        ll_world::tree::derived_species_at(
            world.world.seed,
            树位,
            world.world.size.height(),
            world.world.terrain_shape.climate_band_width
        )
    );
}

#[test]
fn 没有种子时培植那一行根本不出现() {
    // 列表与结算必须一致：列出一行按下去什么都不发生的选项就是在骗玩家。
    // Arrange
    let content = real_content();
    let (mut world, player, 树位) = 世界与一棵树(&content);
    world.world.trees.set(树位, TreeDeviation::felled());

    // Act
    let rows = interact_entries(&world.world, 树位, player, lookup(&content));

    // Assert
    assert!(
        !rows.iter().any(|row| matches!(
            row,
            InteractTarget::Tree {
                action: TreeAction::Plant,
                ..
            }
        )),
        "背包里没有种子，培植那一行不该出现，实际 {rows:?}"
    );

    // 对照组：塞一颗种子进去，那一行就该出现——否则上面那条可以被一个
    // 「培植那一行永远不显示」的实现满足（ADR 0022 的判据退化）。
    let seed = index(&content, "lostland:tree_seed");
    world
        .world
        .actors
        .get_mut(player)
        .expect("玩家存在")
        .inventory
        .push(ItemStack::new(seed, 1));
    let rows = interact_entries(&world.world, 树位, player, lookup(&content));
    assert!(
        rows.iter().any(|row| matches!(
            row,
            InteractTarget::Tree {
                action: TreeAction::Plant,
                ..
            }
        )),
        "手上有种子了，培植那一行该出现，实际 {rows:?}"
    );
}

#[test]
fn 不是森林的格子上一行树都没有() {
    // 「`forest` 地形保留当底图」是项目所有者的要求原话。
    let content = real_content();
    let (world, player, _) = 世界与一棵树(&content);
    let here = world.world.actors.get(player).expect("玩家存在").pos;
    // 玩家脚下那一格没被铺成森林（`世界与一棵树` 只铺了右边那一格）。
    assert_ne!(
        world.world.terrain_at(here),
        Some(content.terrain_ids.forest),
        "前提：玩家脚下不是森林，否则这条断言在空跑"
    );

    let rows = interact_entries(&world.world, here, player, lookup(&content));

    assert!(
        !rows
            .iter()
            .any(|row| matches!(row, InteractTarget::Tree { .. })),
        "非森林格子上出现了树的交互行，实际 {rows:?}"
    );
}
