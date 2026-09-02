//! 端到端验收：**跟 NPC 对话、开交易屏、真的买下一件东西。**
//!
//! 全程走真实 `mods/` 内容与生产路径（`build_new_world` +
//! `ll_game::dialogue_screen` + `ll_game::trade_screen` + `TurnEngine`），
//! 与本体二进制走同一串调用（ADR 0018）。**不启动窗口、不合成任何按键**
//! （ADR 0025）：`InputState::press` 是既有的公开构造 API。
//!
//! # 本文件咬住的五条
//!
//! | 能力 | 断言 |
//! |---|---|
//! | `open-trade` **不提交任何意图**，只推 UI | `选中open_trade那一行不提交意图只换屏` |
//! | 交易屏行来自双方背包，价钱是真的 | `交易屏把双方的货各列一行并带上真实价钱` |
//! | 文案真的被本地化（不是回退到键名） | `交易屏的两种语言文案真的不同` |
//! | **端到端成交**：货和钱各自换手 | `在交易屏上买下一件东西货和钱各自换手` |
//! | **`open-trade` 这一行不消耗回合** | `开交易屏那一行不消耗回合` |

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_game::content::{LoadedContent, RuntimeCatalogs};
use ll_game::dialogue_screen::{DialogueParticipants, dialogue_rows, update_dialogue};
use ll_game::menu_screen::ScreenState;
use ll_game::pointer::RowPointer;
use ll_game::trade_screen::{trade_rows, update_trade};
use ll_game::world::{GameWorld, build_new_world};
use ll_i18n::Catalog;
use ll_platform::input::{GameKey, InputState};
use ll_sim::intent::Intent;
use ll_sim::timeline::Timeline;
use ll_sim::trade::TradeDirection;
use ll_sim::turn::TurnEngine;
use ll_world::entity::EntityId;
use ll_world::item::ItemStack;
use ll_world::ownership::Owner;
use ll_world::state::WorldState;

/// 固定种子，理由同 `dialogue_session.rs` 的同名常量。
const SEED: u64 = 20260826;

/// 管理者背包里那件用来买卖的东西，以及玩家开局带的那点钱。
const 货: &str = "lostland:roast_meat";
const 玩家开局的钱: i64 = 10_000;

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn test_content() -> LoadedContent {
    let root = repo_root();
    ll_game::content::load_content(&root.join("mods"), &root.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功")
}

/// 按生产路径装出 `Catalog`（本体 + 全部带 `locales/` 的 mod）。
///
/// **不用空 `Catalog`**：空目录下 `resolve` 会退回键名本身，于是
/// 「文案不等于键名」这一类断言会恒绿。本文件因此断言**具体文案**、
/// 以及**两种语言互不相同**。
fn 真实文案() -> Catalog {
    let paths = ll_game::GamePaths::under(&repo_root());
    Catalog::load(
        ll_game::content::BASE_NAMESPACE,
        &ll_game::locale_sources(&paths),
    )
}

fn 索引(content: &LoadedContent, raw: &str) -> ContentIndex {
    content
        .registry
        .get(&NamespacedId::parse(raw).expect("固定字面量标识符恒合法"))
        .unwrap_or_else(|| panic!("{raw} 必须已注册"))
}

/// 建一局世界，东侧相邻格放一位**背着三份烤肉、口袋里有钱**的管理者，
/// 并给玩家一点开局资金。
fn 有管理者可交易的世界(content: &LoadedContent) -> (GameWorld, EntityId) {
    let mut game_world = build_new_world(
        content,
        ll_world::generate::GenParams {
            seed: SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("建世界应当成功");
    let player = game_world.player;
    let player_pos = game_world
        .world
        .actors
        .get(player)
        .expect("建局之后玩家必然存在")
        .pos;
    let east = game_world
        .world
        .size
        .wrap(player_pos.x() + 1, player_pos.y());
    let mut agent = game_world.world.actors.get(player).expect("玩家在").clone();
    agent.pos = east;
    agent.profession = 索引(content, "lostland:steward");
    agent.affiliations = Vec::new();
    agent.wallet = 玩家开局的钱;
    agent.inventory = vec![ItemStack {
        owner: Owner::Unowned,
        ..ItemStack::new(索引(content, 货), 3)
    }];
    let npc = game_world.world.actors.spawn(agent);
    game_world
        .world
        .actors
        .get_mut(player)
        .expect("玩家在")
        .wallet = 玩家开局的钱;
    game_world
        .world
        .actors
        .get_mut(player)
        .expect("玩家在")
        .inventory
        .clear();
    (game_world, npc)
}

/// 管理者开场白里那条 `open-trade` 选项在过滤后是第几行。
fn 交易那一行(
    game_world: &GameWorld,
    content: &LoadedContent,
    catalog: &Catalog,
) -> (usize, usize) {
    let root = 索引(content, "lostland:steward_root");
    let rows = dialogue_rows(
        root,
        &content.dialogue_node_table,
        game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家在"),
        &content.registry,
        catalog,
        "zh-CN",
    );
    let 行 = rows
        .iter()
        .position(|row| row.text.contains("可换的"))
        .expect("管理者开场白上必须有那条交易选项——它是本文件全部断言的前提");
    (行, rows.len())
}

/// 把玩家的一次意图真的交给回合引擎（`Demo` 那一串调用的最小等价物）。
fn 提交(game_world: &mut GameWorld, content: &LoadedContent, intent: Intent) {
    let player = game_world.player;
    let clock = game_world.world.clock;
    let mut timeline = Timeline::new();
    timeline.schedule(player, clock);
    let mut engine = TurnEngine::new(timeline);
    let runtime = RuntimeCatalogs::new(content);
    let catalogs = runtime.as_resolve_catalogs();
    let mut on_effect = |_world: &WorldState, _effect: &ll_sim::effect::Effect| {};
    let mut ai =
        |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Wait { actor };
    engine.advance_ai(
        &mut game_world.world,
        player,
        &mut ai,
        &catalogs,
        &mut on_effect,
    );
    engine.try_player_intent(
        &mut game_world.world,
        player,
        intent,
        &catalogs,
        &mut on_effect,
    );
}

fn 钱包(game_world: &GameWorld, who: EntityId) -> i64 {
    game_world.world.actors.get(who).expect("实体在").wallet
}

fn 背包(game_world: &GameWorld, who: EntityId) -> Vec<ItemStack> {
    game_world
        .world
        .actors
        .get(who)
        .expect("实体在")
        .inventory
        .clone()
}

/// **`open-trade` 不提交任何意图**（规格 7.2：提交一条恒产出空效果的
/// 意图只会污染意图日志），只把 UI 推进交易屏。
///
/// 故意改坏的反例（本批实测）：把 `update_dialogue` 里那句
/// `.any(|outcome| !outcome.is_ui_only())` 换回 `!outcomes.is_empty()`，
/// 本条当场红（`submit` 变成 `Some`）。
#[test]
fn 选中open_trade那一行不提交意图只换屏() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (mut game_world, npc) = 有管理者可交易的世界(&content);
    let root = 索引(&content, "lostland:steward_root");
    let (行, _) = 交易那一行(&game_world, &content, &catalog);
    let rows = dialogue_rows(
        root,
        &content.dialogue_node_table,
        game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家在"),
        &content.registry,
        &catalog,
        "zh-CN",
    );
    let mut cursor = 行;

    // Act
    let update = update_dialogue(
        root,
        &mut cursor,
        &rows,
        &content.dialogue_node_table,
        DialogueParticipants {
            actor: game_world.player,
            speaker: npc,
        },
        &InputState::new(),
        RowPointer::Activate(行),
    );

    // Assert
    assert!(
        update.submit.is_none(),
        "open-trade 恒产出空效果，不该提交意图：{:?}",
        update.submit
    );
    assert_eq!(
        update.next,
        Some(ScreenState::Trade {
            partner: npc,
            cursor: 0
        }),
        "它该把 UI 推进交易屏（并且 open-trade 压过 next: end）"
    );
    // 世界一个字节都不该变（这一行不产 Effect）。
    assert_eq!(钱包(&game_world, game_world.player), 玩家开局的钱);
    assert!(背包(&game_world, game_world.player).is_empty());
    // 顺带确认引擎没被叫起来——`game_world` 声明成 `mut` 只是为了与
    // 下面几条用同一个夹具函数。
    let _ = &mut game_world;
}

/// 交易屏把双方的货各列一行，价钱是 `ll_sim::trade::trade_price` 算出来
/// 的真值（不是占位、不是 0）。
///
/// 故意改坏的反例（本批实测）：把 `trade_rows` 里那句 `continue`
/// （查不到定价规则就不显示）改成显示一行价格 0，本条当场红。
#[test]
fn 交易屏把双方的货各列一行并带上真实价钱() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (mut game_world, npc) = 有管理者可交易的世界(&content);
    let player = game_world.player;
    // 给玩家也塞一件，好让「卖」那一半也有行。
    game_world
        .world
        .actors
        .get_mut(player)
        .expect("玩家在")
        .inventory = vec![ItemStack {
        owner: Owner::Player,
        ..ItemStack::new(索引(&content, 货), 1)
    }];

    // Act
    let rows = trade_rows(
        &game_world.world,
        player,
        npc,
        &content.item_table,
        &catalog,
        "zh-CN",
    );

    // Assert
    assert_eq!(rows.len(), 2, "对方一行、自己一行：{rows:?}");
    assert_eq!(rows[0].direction, TradeDirection::Buy, "对方的货排在前面");
    assert_eq!(rows[1].direction, TradeDirection::Sell);
    // 烤肉的 base_price 是 900（`mods/lostland/items.json5`），玩家与
    // 管理者之间没有归属 ⇒ 中立原价。**断言具体数字**：它同时钉住
    // 「读的是 Milli 的原始值而不是 whole()」。
    assert_eq!(rows[0].price, 900, "中立原价就是基础价：{rows:?}");
    assert_eq!(rows[1].price, 900);
    assert!(
        rows[0].text.contains("烤肉") && rows[0].text.contains("900"),
        "行文案要带物品名与价钱：{}",
        rows[0].text
    );
}

/// 交易屏的文案真的走了本地化——**两种语言互不相同**。
///
/// 这一条防的是「空 `Catalog` 回落到键名/另一门语言」那个恒绿形状：
/// 若两边都回退到键名，它们会相等，本条当场红。
#[test]
fn 交易屏的两种语言文案真的不同() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (game_world, npc) = 有管理者可交易的世界(&content);

    // Act
    let 中文 = trade_rows(
        &game_world.world,
        game_world.player,
        npc,
        &content.item_table,
        &catalog,
        "zh-CN",
    );
    let 英文 = trade_rows(
        &game_world.world,
        game_world.player,
        npc,
        &content.item_table,
        &catalog,
        "en",
    );

    // Assert（先断言对象存在，否则下面的比较是两个空列表）
    assert_eq!(中文.len(), 1);
    assert_eq!(英文.len(), 1);
    assert_ne!(
        中文[0].text, 英文[0].text,
        "两种语言的行文案不该相同（相同 = 两边都回退到了键名）"
    );
    assert!(英文[0].text.contains("Roast") || 英文[0].text.contains("roast"));
}

/// **端到端**：在交易屏上按下确认，货和钱各自换手。
///
/// 故意改坏的反例（本批实测）：把 `update_trade` 产出的
/// `Intent::Trade` 的 `direction` 改成恒 `Sell`，本条当场红。
#[test]
fn 在交易屏上买下一件东西货和钱各自换手() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (mut game_world, npc) = 有管理者可交易的世界(&content);
    let player = game_world.player;
    let 管理者原有的钱 = 钱包(&game_world, npc);
    let rows = trade_rows(
        &game_world.world,
        player,
        npc,
        &content.item_table,
        &catalog,
        "zh-CN",
    );
    assert_eq!(rows.len(), 1, "夹具前提：管理者手里真的有一堆货");
    let mut cursor = 0;
    let mut input = InputState::new();
    input.press(GameKey::Confirm);

    // Act
    let update = update_trade(&mut cursor, &rows, player, npc, &input, RowPointer::Idle);
    let intent = update.submit.expect("按下确认必须提交一条成交意图");
    assert_eq!(
        intent,
        Intent::Trade {
            actor: player,
            partner: npc,
            item: 索引(&content, 货),
            direction: TradeDirection::Buy,
        }
    );
    提交(&mut game_world, &content, intent);

    // Assert
    assert_eq!(背包(&game_world, npc)[0].count, 2, "管理者少一件");
    let 到手 = 背包(&game_world, player);
    assert_eq!(到手.len(), 1, "玩家多出一堆");
    assert_eq!(到手[0].count, 1, "一次一件");
    assert_eq!(到手[0].owner, Owner::Player, "到手之后归玩家");
    assert_eq!(钱包(&game_world, player), 玩家开局的钱 - 900);
    assert_eq!(钱包(&game_world, npc), 管理者原有的钱 + 900);
}

/// **`open-trade` 这一行不消耗回合**——每一支新能力各有自己的这一条
/// （批次 3 记下的那个「新变体不再被旧测试覆盖」陷阱）。
///
/// 这一条走的是**端到端**那一路：它连一条意图都不提交，因此时钟不可能
/// 动；断言的价值在于**把这条性质钉在真实内容与真实屏上**，而不是只钉
/// 在 `ll-sim` 的夹具上（那一条在
/// `crates/ll-sim/tests/dialogue_choose.rs`）。
///
/// 故意改坏的反例（本批实测）：让 `update_dialogue` 对 `open-trade` 也
/// 提交意图、并给 `resolve` 的 `OpenTrade` 一支补一条
/// `Effect::ScheduleNext`，本条当场红。
#[test]
fn 开交易屏那一行不消耗回合() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (mut game_world, npc) = 有管理者可交易的世界(&content);
    let root = 索引(&content, "lostland:steward_root");
    let (行, _) = 交易那一行(&game_world, &content, &catalog);
    let clock_before = game_world.world.clock;
    let next_before = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家在")
        .next_action_at;
    let rows = dialogue_rows(
        root,
        &content.dialogue_node_table,
        game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家在"),
        &content.registry,
        &catalog,
        "zh-CN",
    );
    let mut cursor = 行;

    // Act
    let update = update_dialogue(
        root,
        &mut cursor,
        &rows,
        &content.dialogue_node_table,
        DialogueParticipants {
            actor: game_world.player,
            speaker: npc,
        },
        &InputState::new(),
        RowPointer::Activate(行),
    );
    if let Some(intent) = update.submit {
        提交(&mut game_world, &content, intent);
    }

    // Assert：**先确认这一行真的被选中了**（换屏了），否则「时钟没动」
    // 只是因为什么都没发生。
    assert!(matches!(update.next, Some(ScreenState::Trade { .. })));
    assert_eq!(game_world.world.clock, clock_before, "开交易屏不消耗回合");
    assert_eq!(
        game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家在")
            .next_action_at,
        next_before,
        "开交易屏不消耗回合：下次行动时刻不动"
    );
}
