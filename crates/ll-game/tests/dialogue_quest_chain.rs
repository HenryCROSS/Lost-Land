//! 端到端验收：**本体第一条能从头走到尾的任务链，全程只靠对话。**
//!
//! ```text
//! steward_root ──「有什么活干吗？」（quest-not-completed）──► steward_work
//!                            │
//!            「山道我已经走过一趟了。」（complete-quest）
//!                            ▼
//! steward_root ──「我把事办完了。」（quest-completed）──► steward_reward
//!                            │
//!                「那我就收下了。」（give-item）
//!                            ▼
//!                    背包里多一份口粮，归玩家所有
//! ```
//!
//! 全程走真实 `mods/` 内容、真实 `assets/locales` 与生产路径
//! （`build_new_world`、`ll_game::player_action`、
//! `ll_game::dialogue_screen`、`TurnEngine`），与本体二进制走同一串调用
//! （ADR 0018）。**不启动窗口、不合成任何按键**（ADR 0025）。
//!
//! # 为什么另开一个文件
//!
//! `dialogue_session.rs` 已经 846 行（规格 §13 上限 800）——批次 26 立的
//! 「先拆再 bless」在这里的落法就是：新的一族验收自带一个文件，不往那个
//! 已经贴着上限的文件里塞。
//!
//! # 防假绿（本会话已出现 15 次「全绿但保护不存在」）
//!
//! - **不用空 `Catalog`**：走生产路径装真实 `assets/locales`，断言的是
//!   **具体中文文案**（空 `Catalog` 下 `resolve` 会退回键名，「文案 ≠ 键名」
//!   那种写法恒绿）；
//! - **每条「那一行在／不在」之前先断言被断言的对象存在**：这一格真的
//!   匹配出了一行对话、说话人就是我们放的那个、管理者身上真的带着那份
//!   口粮；
//! - **收到的那一堆按 `owner` 认**，不按「背包里有没有这种东西」——玩家
//!   的出生装备里本来就可能有同一种物品（人类带三份烤肉）。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_game::content::{LoadedContent, RuntimeCatalogs};
use ll_game::dialogue_screen::{DialogueParticipants, DialogueRow, dialogue_rows, update_dialogue};
use ll_game::menu_screen::ScreenState;
use ll_game::player_action::{InteractTarget, TalkLookup, interact_entries};
use ll_game::pointer::RowPointer;
use ll_game::world::{GameWorld, build_new_world};
use ll_i18n::Catalog;
use ll_platform::input::InputState;
use ll_sim::quest::is_quest_completed;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_world::entity::EntityId;
use ll_world::item::ItemStack;
use ll_world::ownership::Owner;
use ll_world::state::WorldState;

/// 固定种子，理由同 `dialogue_session.rs` 的同名常量。
const SEED: u64 = 20260826;

/// 那条本体任务——链条两端的条件（`quest-not-completed` /
/// `quest-completed`）读的都是它。
///
/// **刻意只用 `main_quest_1`（击杀 3 只哥布林）**：本体任务图里
/// `lostland:branch_b` 那条「击杀 1 个人类」是此前某批次自己定的口径并
/// 主动标记为**待所有者裁定**（见 `mods/lostland/quests.json5` 文件头
/// 「branch_b 的内容口径」一节）。本批的任务链一步都不碰它，也不绕开它
/// 另造一条——那是设定问题，不由实现者决定。
const QUEST: &str = "lostland:main_quest_1";

/// 奖励物品。选它的理由是 `give-item` 的硬前置：那件东西必须真的在说话人
/// 背包里，而 NPC 的背包今天唯一的生产者是种族出生装备。
const REWARD: &str = "lostland:roast_meat";

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
fn 真实文案() -> Catalog {
    let paths = ll_game::GamePaths::under(&repo_root());
    Catalog::load(
        ll_game::content::BASE_NAMESPACE,
        &ll_game::locale_sources(&paths),
    )
}

fn id(raw: &str) -> NamespacedId {
    NamespacedId::parse(raw).expect("固定字面量标识符恒合法")
}

fn 索引(content: &LoadedContent, raw: &str) -> ContentIndex {
    content
        .registry
        .get(&id(raw))
        .unwrap_or_else(|| panic!("{raw} 必须已注册"))
}

/// 建一局世界，在玩家东侧放一个**背着 `count` 份口粮**的管理者。
///
/// 口粮的归属取 [`Owner::Unowned`]——那正是 `ll_mod::roster` 物化 NPC 时
/// 出生装备的实际归属（`ItemStack::freshly_made` 走 `Owner` 的默认值），
/// 因此这个夹具摆出来的不是一个特制场景，是生产路径上真会出现的那一档。
fn 有管理者的世界(content: &LoadedContent, count: u32) -> (GameWorld, EntityId) {
    let mut game_world = build_new_world(
        content,
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
        .expect("建局之后玩家必然存在")
        .clone();
    let east = game_world
        .world
        .size
        .wrap(player.pos.x() + 1, player.pos.y());
    let mut agent = player;
    agent.pos = east;
    agent.profession = 索引(content, "lostland:steward");
    agent.affiliations = Vec::new();
    agent.inventory = vec![ItemStack::new(索引(content, REWARD), count)];
    let npc = game_world.world.actors.spawn(agent);
    (game_world, npc)
}

fn east_of_player(game_world: &GameWorld) -> ll_core::torus::TorusPos {
    let pos = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家在")
        .pos;
    game_world.world.size.wrap(pos.x() + 1, pos.y())
}

fn talk<'a>(content: &'a LoadedContent) -> TalkLookup<'a> {
    TalkLookup {
        dialogues: &content.dialogue_table,
        cultures: Some(&content.culture_table),
    }
}

fn 会话行(
    game_world: &GameWorld,
    content: &LoadedContent,
    catalog: &Catalog,
    node: ContentIndex,
) -> Vec<DialogueRow> {
    dialogue_rows(
        node,
        &content.dialogue_node_table,
        game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家在"),
        &content.registry,
        catalog,
        "zh-CN",
    )
}

fn 文案(rows: &[DialogueRow]) -> Vec<String> {
    rows.iter().map(|row| row.text.clone()).collect()
}

/// 在这一帧的行里找出写着 `text` 的那一行的下标。
fn 第几行(rows: &[DialogueRow], text: &str) -> usize {
    文案(rows)
        .iter()
        .position(|row| row == text)
        .unwrap_or_else(|| panic!("这一帧里没有「{text}」：{:?}", 文案(rows)))
}

struct 会话夹具<'a> {
    content: &'a LoadedContent,
    catalog: &'a Catalog,
}

/// 选中写着 `text` 的那一行，把它要提交的意图真的交给回合引擎，并返回
/// **下一屏**（`None` = 会话结束）。
///
/// 与 `dialogue_session.rs` 的同名帮手同一串调用，只是按**文案**定位而
/// 不是按下标——本文件的链条要跨好几个节点，写死下标既脆又读不出意思。
fn 选中一行(
    game_world: &mut GameWorld,
    夹具: &会话夹具<'_>,
    speaker: EntityId,
    node: ContentIndex,
    text: &str,
) -> Option<ScreenState> {
    let 会话夹具 { content, catalog } = 夹具;
    let rows = 会话行(game_world, content, catalog, node);
    let 目标行 = 第几行(&rows, text);
    let mut cursor = 目标行;
    let player = game_world.player;
    let update = update_dialogue(
        node,
        &mut cursor,
        &rows,
        &content.dialogue_node_table,
        DialogueParticipants {
            actor: player,
            speaker,
        },
        &InputState::new(),
        RowPointer::Activate(目标行),
    );
    if let Some(intent) = update.submit {
        let clock = game_world.world.clock;
        let mut timeline = Timeline::new();
        timeline.schedule(player, clock);
        let mut engine = TurnEngine::new(timeline);
        let runtime = RuntimeCatalogs::new(content);
        let catalogs = runtime.as_resolve_catalogs();
        let mut on_effect = |_world: &WorldState, _effect: &ll_sim::effect::Effect| {};
        let mut ai = |_world: &WorldState, actor: EntityId, _controlled: EntityId| {
            ll_sim::intent::Intent::Wait { actor }
        };
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
    update.next
}

/// 下一屏必须还是会话屏，把它的节点取出来。
fn 下一节点(next: Option<ScreenState>) -> ContentIndex {
    match next {
        Some(ScreenState::Dialogue { node, .. }) => node,
        other => panic!("这一行应当跳到另一个节点，实际是 {other:?}"),
    }
}

/// 玩家名下（`Owner::Player`）那一种物品的总数——**按归属认**，不按
/// 「背包里有没有这种东西」：玩家的出生装备里本来就可能有同一种物品
/// （人类带三份烤肉），只数「有没有」会让本文件的核心断言恒真。
fn 玩家名下的数量(game_world: &GameWorld, def: ContentIndex) -> u32 {
    game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家在")
        .inventory
        .iter()
        .filter(|stack| stack.def == def && stack.owner == Owner::Player)
        .map(|stack| stack.count)
        .sum()
}

fn 说话人手里的数量(game_world: &GameWorld, who: EntityId, def: ContentIndex) -> u32 {
    game_world
        .world
        .actors
        .get(who)
        .expect("说话人在")
        .inventory
        .iter()
        .filter(|stack| stack.def == def)
        .map(|stack| stack.count)
        .sum()
}

/// **整条链一次走通**：接活 → 交差（`complete-quest`）→ 领赏
/// （`give-item`），全程只按对话选项。
///
/// 故意改坏的反例（本批实测）：
/// - 把 `resolve` 的 `complete-quest` 一支去掉 ⇒ 「我把事办完了」永远不
///   出现，本条红在第二段；
/// - 把 `give-item` 一支去掉 ⇒ 本条红在最后一段（玩家名下的口粮没多）。
#[test]
fn 管理者的任务链从接活走到领赏() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (mut game_world, npc) = 有管理者的世界(&content, 2);
    let 夹具 = 会话夹具 {
        content: &content,
        catalog: &catalog,
    };
    let root = 索引(&content, "lostland:steward_root");
    let reward_item = 索引(&content, REWARD);
    let 有活干吗 = "有什么活干吗？";
    let 办完了 = "我把事办完了。";
    let 交差 = "山道我已经走过一趟了。";
    let 收下 = "那我就收下了。";

    // 对照组前提一：这一格上的人真的匹配出了管理者那段对话——否则下面
    // 「那一行在／不在」两类断言会因为整块列表为空而恒真恒假。
    let rows = interact_entries(
        &game_world.world,
        east_of_player(&game_world),
        game_world.player,
        talk(&content),
    );
    assert!(
        matches!(rows.first(), Some(InteractTarget::Talk { speaker, .. }) if *speaker == npc),
        "东侧那一格必须匹配出一行对话，实际 {rows:?}"
    );
    // 对照组前提二：任务还没完成，管理者手里真的有两份口粮，玩家名下
    // 一份都没有。
    assert!(
        !is_quest_completed(
            game_world
                .world
                .actors
                .get(game_world.player)
                .expect("玩家在"),
            &id(QUEST)
        ),
        "开局这条任务必须是未完成"
    );
    assert_eq!(说话人手里的数量(&game_world, npc, reward_item), 2);
    assert_eq!(
        玩家名下的数量(&game_world, reward_item),
        0,
        "开局玩家名下不该有这种东西——有的话最后那条断言验不出是这次给的"
    );

    // Act & Assert 第一段：接活那一行在，领赏那一行不在。
    let 开场 = 文案(&会话行(&game_world, &content, &catalog, root));
    assert!(开场.iter().any(|t| t == 有活干吗), "{开场:?}");
    assert!(!开场.iter().any(|t| t == 办完了), "{开场:?}");

    // Act 第二段：进 steward_work，选「山道我已经走过一趟了」。
    let work = 下一节点(选中一行(&mut game_world, &夹具, npc, root, 有活干吗));
    assert_eq!(work, 索引(&content, "lostland:steward_work"));
    let 回到开场 = 下一节点(选中一行(&mut game_world, &夹具, npc, work, 交差));
    assert_eq!(回到开场, root, "交差那一行的 next 指回开场白");

    // Assert 第二段：任务真的完成了，两条谓词的读数一起翻过来。
    assert!(
        is_quest_completed(
            game_world
                .world
                .actors
                .get(game_world.player)
                .expect("玩家在"),
            &id(QUEST)
        ),
        "complete-quest 之后这条任务必须是已完成"
    );
    let 交差后 = 文案(&会话行(&game_world, &content, &catalog, root));
    assert!(
        !交差后.iter().any(|t| t == 有活干吗),
        "交差之后「有什么活干吗」必须消失：{交差后:?}"
    );
    assert!(
        交差后.iter().any(|t| t == 办完了),
        "交差之后「我把事办完了」必须出现：{交差后:?}"
    );

    // Act 第三段：进 steward_reward，领赏。
    let reward = 下一节点(选中一行(&mut game_world, &夹具, npc, root, 办完了));
    assert_eq!(reward, 索引(&content, "lostland:steward_reward"));
    let _ = 选中一行(&mut game_world, &夹具, npc, reward, 收下);

    // Assert 第三段：东西真的换了手，且归玩家所有。
    assert_eq!(
        说话人手里的数量(&game_world, npc, reward_item),
        1,
        "管理者手里必须真的少一份"
    );
    assert_eq!(
        玩家名下的数量(&game_world, reward_item),
        1,
        "玩家名下必须多出恰好一份"
    );
}

/// **owner 校验硬前置的端到端那一半**：管理者手里那份口粮若是**别人的**
/// （这里取 `Owner::Player`——玩家的东西寄放在他那里），「那我就收下了」
/// 这一行照常显示，但选中之后**什么都不会发生**。
///
/// 与上一条共用同一份夹具，只改一个字段——这样两条的差别就只有归属这一
/// 项，不会是别的什么造成的。
///
/// 故意改坏的反例（本批实测）：把 `resolve` 里那段 owner 校验去掉，本条
/// 当场红（玩家名下凭空多出一份）。
#[test]
fn 管理者拿别人的东西当奖励时领赏那一行什么都不做() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (mut game_world, npc) = 有管理者的世界(&content, 2);
    let reward_item = 索引(&content, REWARD);
    // 只改这一个字段：那两份口粮是**玩家**的，寄放在管理者那里。
    for stack in &mut game_world
        .world
        .actors
        .get_mut(npc)
        .expect("说话人在")
        .inventory
    {
        stack.owner = Owner::Player;
    }
    let 夹具 = 会话夹具 {
        content: &content,
        catalog: &catalog,
    };
    let root = 索引(&content, "lostland:steward_root");
    let 交差 = "山道我已经走过一趟了。";
    let 收下 = "那我就收下了。";

    // 走到领赏那一步（与上一条同一串操作）。
    let work = 下一节点(选中一行(
        &mut game_world,
        &夹具,
        npc,
        root,
        "有什么活干吗？",
    ));
    let _ = 选中一行(&mut game_world, &夹具, npc, work, 交差);
    let reward = 下一节点(选中一行(
        &mut game_world,
        &夹具,
        npc,
        root,
        "我把事办完了。",
    ));

    // 对照组前提：那一行**确实显示得出来**（否则下面的「什么都没发生」
    // 会因为压根点不到而恒真）。
    let rows = 文案(&会话行(&game_world, &content, &catalog, reward));
    assert!(
        rows.iter().any(|t| t == 收下),
        "领赏那一行必须照常显示——显示与否是内容作者用 conditions 表达的\
         事，owner 校验只管结算：{rows:?}"
    );
    let 之前 = 玩家名下的数量(&game_world, reward_item);

    // Act
    let _ = 选中一行(&mut game_world, &夹具, npc, reward, 收下);

    // Assert：一件都没换手。
    assert_eq!(
        说话人手里的数量(&game_world, npc, reward_item),
        2,
        "不是他的东西，一件都送不出去"
    );
    assert_eq!(
        玩家名下的数量(&game_world, reward_item),
        之前,
        "玩家名下一件都不该多"
    );
}
