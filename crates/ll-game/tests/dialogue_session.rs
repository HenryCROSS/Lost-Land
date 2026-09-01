//! 端到端验收：**跟 NPC 说话，能开口、能分支、能记住选择。**
//!
//! 所有者原话：
//!
//! > 「我希望交互也能包括和 NPC 对话」
//! > 「我觉得应该要预选一项，同时我希望能有按钮选项，鼠标点击也有反应。」
//!
//! 全程走真实 `mods/` 内容与生产路径（`build_new_world` + `TurnEngine` +
//! `ll_game::player_action` + `ll_game::dialogue_screen`），与本体二进制
//! 走同一串调用（ADR 0018）。**不启动窗口、不合成任何按键**（ADR 0025）：
//! `InputState::press` 是既有的公开构造 API，`ll_platform::input` 自己的
//! 单元测试就在用它。
//!
//! # 本文件咬住的四条
//!
//! | 能力 | 断言 |
//! |---|---|
//! | 对话进交互列表、排在最前、敌对不列 | `站着的npc让交互列表多出一行对话`、`敌对目标不进对话行` |
//! | 选项过滤调的是 `resolve` 那一侧的同一个函数 | `条件不满足的选项在会话屏上一行都不显示` |
//! | 预选第一项 + 鼠标点击 | `打开会话屏时预选第一项`、`鼠标点在第几行就选中第几行` |
//! | **`set-flag` 端到端**：设了旗标之后选项真的变 | `听完风声之后那一行消失换成另一行` |
//! | 对话不消耗回合 | `说完一整轮话世界时钟一格没动`（`ll-sim` 那一侧另有一条） |

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_game::content::{LoadedContent, RuntimeCatalogs};
use ll_game::dialogue_screen::{DialogueRow, dialogue_rows, dialogue_title_key, update_dialogue};
use ll_game::menu_screen::{ScreenOutcome, ScreenState};
use ll_game::player_action::{InteractTarget, TalkLookup, interact_entries};
use ll_game::pointer::RowPointer;
use ll_game::world::{GameWorld, build_new_world};
use ll_i18n::Catalog;
use ll_platform::input::{GameKey, InputState};
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_world::entity::{Affiliation, AffiliationKind, EntityId, OrgRef};
use ll_world::state::WorldState;

/// 固定种子，理由同 `door_interaction.rs` 的同名常量。
const SEED: u64 = 20260826;

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
/// 「文案不等于键名」这一类断言会恒绿——批次 19 刚在这个形状上抓到过
/// 一条。本文件因此一律断言**具体文案**或**具体行数/顺序**。
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

/// 建一局世界，并在玩家东侧相邻格放一个指定职业与文化的 NPC。
///
/// 复制玩家的 `Agent` 再改字段，写法同 `door_interaction.rs` 的
/// 「门口站着人」那一段。
fn world_with_neighbour(
    content: &LoadedContent,
    profession: ContentIndex,
    culture: Option<ContentIndex>,
) -> (GameWorld, EntityId) {
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
    let mut agent = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家在")
        .clone();
    agent.pos = east;
    agent.profession = profession;
    agent.affiliations = culture
        .map(|org| {
            vec![Affiliation {
                kind: AffiliationKind::Culture,
                org: OrgRef::Def(org),
                standing: 0,
            }]
        })
        .unwrap_or_default();
    let npc = game_world.world.actors.spawn(agent);
    (game_world, npc)
}

/// 玩家东侧那一格。
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

/// 会话屏这一帧的行，走的是生产路径那个函数。
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

/// 把一次会话屏的输入跑完，并把它要提交的意图真的交给回合引擎——
/// `Demo::update_dialogue_screen` 那一串调用的最小等价物。
fn 会话一帧(
    game_world: &mut GameWorld,
    content: &LoadedContent,
    catalog: &Catalog,
    node: ContentIndex,
    cursor: &mut usize,
    input: &InputState,
    pointer: RowPointer,
) -> (ScreenOutcome, Option<ScreenState>) {
    let rows = 会话行(game_world, content, catalog, node);
    let player = game_world.player;
    let update = update_dialogue(
        node,
        cursor,
        &rows,
        &content.dialogue_node_table,
        player,
        input,
        pointer,
    );
    if let Some(intent) = update.submit {
        let clock = game_world.world.clock;
        let mut timeline = Timeline::new();
        timeline.schedule(player, clock);
        let mut engine = TurnEngine::new(timeline);
        let runtime = RuntimeCatalogs::new(content);
        let catalogs = runtime.as_resolve_catalogs();
        let mut on_effect = |_world: &WorldState, _effect: &ll_sim::effect::Effect| {};
        // 先把 pending 顶上来（引擎要求「轮到了」才受理），再提交。
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
    (update.outcome, update.next)
}

// ── 一、进交互列表 ───────────────────────────────────────────────

/// 故意改坏的反例（本批实测）：把 `interact_entries` 开头那三行
/// `talk_target` 去掉，本条当场变红。
#[test]
fn 站着的npc让交互列表多出一行对话() {
    // Arrange：东侧站一个管理者（本体给这个职业写了一段对话）。
    let content = test_content();
    let steward = 索引(&content, "lostland:steward");
    let (game_world, _npc) = world_with_neighbour(&content, steward, None);

    // Act
    let rows = interact_entries(
        &game_world.world,
        east_of_player(&game_world),
        game_world.player,
        talk(&content),
    );

    // Assert：**排在最前**（规格六节第 2 条）。
    let expected = 索引(&content, "lostland:steward_greeting");
    assert_eq!(
        rows.first(),
        Some(&InteractTarget::Talk {
            profession: steward,
            dialogue: expected,
        }),
        "对话那一行必须在最前，且匹配到管理者那一段"
    );
    assert_eq!(
        rows[0].item_def(),
        None,
        "对话这一行指的不是一件物品——与门那一支同一档"
    );
}

/// 敌对目标不列对话行（规格六节第 3 条）。
///
/// 故意改坏的反例（本批实测）：把 `talk_target` 里那句
/// `if declared_hostile(..) { return None; }` 删掉，本条当场变红。
#[test]
fn 敌对目标不进对话行() {
    // Arrange：同一个职业、但挂上哥布林部族文化——本体内容声明
    // goblin_warband → mining_hold 的敌意是 6，过了阈值 5
    // （`mods/lostland/cultures.json5`；农庄那条只有 4，**不够**，
    // 挑错文化这条测试会假绿）。
    let content = test_content();
    let steward = 索引(&content, "lostland:steward");
    let goblin = 索引(&content, "lostland:goblin_warband");
    let (mut game_world, _npc) = world_with_neighbour(&content, steward, Some(goblin));
    // 玩家也得有个文化，否则两边都是「无文化」，敌意查不出东西来。
    let farmstead = 索引(&content, "lostland:mining_hold");
    game_world
        .world
        .actors
        .get_mut(game_world.player)
        .expect("玩家在")
        .affiliations = vec![Affiliation {
        kind: AffiliationKind::Culture,
        org: OrgRef::Def(farmstead),
        standing: 0,
    }];

    // Act
    let rows = interact_entries(
        &game_world.world,
        east_of_player(&game_world),
        game_world.player,
        talk(&content),
    );

    // Assert
    assert!(
        !rows
            .iter()
            .any(|row| matches!(row, InteractTarget::Talk { .. })),
        "敌对目标不该出现对话行，实际列出了 {rows:?}"
    );
}

// ── 二、会话屏 ───────────────────────────────────────────────────

/// 标题就是 NPC 那一句，而且真的查得到中文文案（不是键名）。
#[test]
fn 会话屏的标题是npc说的那一句() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let root = 索引(&content, "lostland:steward_root");

    // Act
    let key = dialogue_title_key(root, &content.dialogue_node_table);
    let text = catalog.resolve("zh-CN", &key);

    // Assert：断言**具体文案**而不是「不等于键名」——后者在空 Catalog
    // 下恒绿（批次 19 抓到过）。
    assert_eq!(key, "lostland:dialogue.steward.root");
    assert!(
        text.contains("外乡人"),
        "标题应当是管理者那一句台词，实际是 {text}"
    );
}

/// 选项过滤真的在起作用——**它调的是 `resolve` 那一侧的同一个函数**
/// （`ll_sim::dialogue::all_conditions_hold`，规格 7.2）。
///
/// 故意改坏的反例（本批实测）：把 `dialogue_rows` 里那句
/// `.filter(|(_, option)| all_conditions_hold(..))` 换成恒 `true`，
/// 本条当场变红（六条全显示）。
#[test]
fn 条件不满足的选项在会话屏上一行都不显示() {
    // Arrange：新玩家没有任何归属、没完成任何任务。管理者开场白六条
    // 选项里，`affiliated`/`standing`/`同乡` 三条都不该显示。
    let content = test_content();
    let catalog = 真实文案();
    let (game_world, _npc) =
        world_with_neighbour(&content, 索引(&content, "lostland:steward"), None);
    let root = 索引(&content, "lostland:steward_root");

    // Act
    let rows = 会话行(&game_world, &content, &catalog, root);
    let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();

    // Assert：能显示的恰好是「我想在这里落脚」（not-affiliated）、
    // 「有什么活干吗」（quest-not-completed）与无条件的「告辞」。
    assert_eq!(
        texts,
        vec!["我想在这里落脚。", "有什么活干吗？", "（告辞）"],
        "过滤后的行不对：{texts:?}"
    );
    // 原始下标必须带着走（0/3/5），不是过滤后的 0/1/2。
    assert_eq!(
        rows.iter().map(|row| row.option).collect::<Vec<_>>(),
        vec![0, 3, 5],
        "提交给 resolve 的必须是**原始**下标"
    );
}

/// 预选第一项（所有者裁定第 1 条）。
#[test]
fn 打开会话屏时预选第一项() {
    // Arrange & Act：`app` 那一侧开屏时构造的就是这个状态。
    let state = ScreenState::Dialogue {
        node: ContentIndex::default(),
        cursor: 0,
    };

    // Assert
    assert!(matches!(state, ScreenState::Dialogue { cursor: 0, .. }));
}

/// 鼠标点在第几行就选中并触发第几行（所有者裁定第 1 条的后半）。
///
/// **不合成任何操作系统级事件**（ADR 0025）：`RowPointer` 是
/// `crate::pointer::resolve_row_pointer` 的纯数据产出，这里直接构造。
#[test]
fn 鼠标点在第几行就选中第几行() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (mut game_world, _npc) =
        world_with_neighbour(&content, 索引(&content, "lostland:steward"), None);
    let root = 索引(&content, "lostland:steward_root");
    let mut cursor = 0;

    // Act：指针触发第 1 行（「有什么活干吗？」→ steward_work 节点）。
    let (outcome, next) = 会话一帧(
        &mut game_world,
        &content,
        &catalog,
        root,
        &mut cursor,
        &InputState::new(),
        RowPointer::Activate(1),
    );

    // Assert
    assert_eq!(outcome, ScreenOutcome::Idle, "换节点不关屏");
    assert_eq!(
        next,
        Some(ScreenState::Dialogue {
            node: 索引(&content, "lostland:steward_work"),
            cursor: 0,
        }),
        "点第 1 行就走第 1 行那条边，且新节点重新预选第一项"
    );
}

/// 取消键结束会话，不提交任何东西。
#[test]
fn 取消键关掉整块会话屏() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let (mut game_world, _npc) =
        world_with_neighbour(&content, 索引(&content, "lostland:steward"), None);
    let root = 索引(&content, "lostland:steward_root");
    let mut cursor = 0;
    let mut input = InputState::new();
    input.press(GameKey::Cancel);

    // Act
    let (outcome, next) = 会话一帧(
        &mut game_world,
        &content,
        &catalog,
        root,
        &mut cursor,
        &input,
        RowPointer::Idle,
    );

    // Assert
    assert_eq!(outcome, ScreenOutcome::Close);
    assert_eq!(next, None);
}

// ── 三、`set-flag` 端到端 ────────────────────────────────────────

/// **本批最要紧的一条**：设了旗标之后，依赖那条旗标的选项真的变了。
///
/// 故意改坏的反例（本批实测）：把 `resolve_dialogue_choose` 里
/// `DialogueOutcome::SetFlag` 那一支产出的 `ModStateWrite` 去掉（返回空
/// 效果），本条当场变红——「最近听到什么风声没有？」那一行**还在**，
/// 「你刚才说的三坑」那一行永远出不来。
#[test]
fn 听完风声之后那一行消失换成另一行() {
    // Arrange：矿堡卫兵。他那段开场白里有一对靠标志互斥的选项。
    let content = test_content();
    let catalog = 真实文案();
    let guard = 索引(&content, "lostland:guard");
    let mining = 索引(&content, "lostland:mining_hold");
    let (mut game_world, _npc) = world_with_neighbour(&content, guard, Some(mining));
    let root = 索引(&content, "lostland:mining_guard_root");
    let 问风声 = "最近听到什么风声没有？";
    let 再问三坑 = "你刚才说的三坑，再讲讲。";

    let 之前: Vec<String> = 会话行(&game_world, &content, &catalog, root)
        .into_iter()
        .map(|row| row.text)
        .collect();
    assert!(
        之前.iter().any(|text| text == 问风声),
        "开局这一行必须在：{之前:?}"
    );
    assert!(
        !之前.iter().any(|text| text == 再问三坑),
        "开局那一行不该在：{之前:?}"
    );

    // Act：选中「最近听到什么风声没有？」——它带 `set-flag`。
    let row = 会话行(&game_world, &content, &catalog, root)
        .into_iter()
        .position(|r| r.text == 问风声)
        .expect("刚断言过它在");
    let mut cursor = row;
    let (_, next) = 会话一帧(
        &mut game_world,
        &content,
        &catalog,
        root,
        &mut cursor,
        &InputState::new(),
        RowPointer::Activate(row),
    );
    assert!(next.is_some(), "这一条是导航到 rumour 节点，不是结束会话");

    // Assert：标志真的写进了世界状态，且那一对选项当场互换。
    let player = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家在");
    assert!(
        ll_sim::dialogue::has_dialogue_flag(
            player,
            &id("lostland:dialogue_flag.guard_rumour_heard")
        ),
        "set-flag 必须真的写进 Agent::mod_state"
    );
    let 之后: Vec<String> = 会话行(&game_world, &content, &catalog, root)
        .into_iter()
        .map(|row| row.text)
        .collect();
    assert!(
        !之后.iter().any(|text| text == 问风声),
        "听过之后这一行必须消失：{之后:?}"
    );
    assert!(
        之后.iter().any(|text| text == 再问三坑),
        "听过之后那一行必须出现：{之后:?}"
    );
}

/// 对话不消耗回合（所有者裁定第 2 条）——走**真实内容**再钉一遍。
/// `ll-sim` 那一侧另有一条走假目录的同名断言。
#[test]
fn 说完一整轮话世界时钟一格没动() {
    // Arrange
    let content = test_content();
    let catalog = 真实文案();
    let guard = 索引(&content, "lostland:guard");
    let mining = 索引(&content, "lostland:mining_hold");
    let (mut game_world, _npc) = world_with_neighbour(&content, guard, Some(mining));
    let root = 索引(&content, "lostland:mining_guard_root");
    let clock_before = game_world.world.clock;
    let next_before = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("玩家在")
        .next_action_at;

    // Act：把整段对话走一圈（选第一行、回来、再选一次）。
    let mut node = root;
    let mut cursor = 0;
    for _ in 0..6 {
        let (_, next) = 会话一帧(
            &mut game_world,
            &content,
            &catalog,
            node,
            &mut cursor,
            &InputState::new(),
            RowPointer::Activate(0),
        );
        match next {
            Some(ScreenState::Dialogue { node: target, .. }) => {
                node = target;
                cursor = 0;
            }
            _ => break,
        }
    }

    // Assert
    assert_eq!(game_world.world.clock, clock_before, "对话不消耗回合");
    assert_eq!(
        game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家在")
            .next_action_at,
        next_before,
        "下次行动时刻也不动"
    );
}
