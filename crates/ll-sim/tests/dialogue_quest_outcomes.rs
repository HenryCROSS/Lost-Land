//! `Intent::DialogueChoose` 的**任务族后果**：`complete-quest`
//! （`give-item` 在下一个提交里加进本文件；对话系统的批次 4，计划文档
//! `docs/superpowers/plans/2026-08-31-batch29-dialogue-quest.md`）。
//!
//! # 为什么另开一个文件，不塞进 `dialogue_choose.rs`
//!
//! 那个文件已经近 600 行，而规格 §13 的上限是 800——批次 26 立的规矩是
//! 「先拆再 bless」，本批照做：`set-flag`/`join-settlement` 留在原处，
//! 任务族两条后果连同它们各自的夹具住在这里。两个文件都只用公开入口
//! （`resolve_with_catalogs` / `TurnEngine`），不碰任何私有函数。
//!
//! # 本文件咬住的几条
//!
//! | 能力 | 断言 |
//! |---|---|
//! | `complete-quest` 走的是**既有**的 `mark_quest_completed` | `complete_quest产出的写入就是mark_quest_completed的返回值` |
//! | `complete-quest` 反查不到任务 id ⇒ 零效果 | `反查不到任务标识符时complete_quest零效果` |
//! | `complete-quest` 不消耗回合 | `完成任务不消耗回合` |

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::dialogue::{
    ContentIdLookup, DialogueCatalog, DialogueCondition, DialogueOptionView, DialogueOutcome,
};
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::quest::{is_quest_completed, mark_quest_completed};
use ll_sim::resolve::resolve_with_catalogs;
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 一个只认识**一个节点**的测试目录，形状同 `dialogue_choose.rs` 里的
/// 同名夹具（两处各留一份：跨测试二进制的夹具没法共用，而把它提升成
/// 生产代码里的公开类型只为测试服务，是更糟的一档）。
struct FakeDialogues {
    node: ContentIndex,
    options: Vec<(Vec<DialogueCondition>, Vec<DialogueOutcome>)>,
}

impl DialogueCatalog for FakeDialogues {
    fn option(&self, node: ContentIndex, option: usize) -> Option<DialogueOptionView<'_>> {
        if node != self.node {
            return None;
        }
        let (conditions, outcomes) = self.options.get(option)?;
        Some(DialogueOptionView {
            conditions,
            outcomes,
        })
    }
}

/// 一份**真的能反查**的 `ContentIdLookup`——`complete-quest` 需要它把
/// `ContentIndex` 换回标识符。
///
/// 不用 `NoContentIds`：那一份任何索引都查不到，`complete-quest` 在它
/// 下面恒零效果，于是「主线断言」会因为**根本没走到**而恒绿。
struct FakeContentIds(Vec<(ContentIndex, NamespacedId)>);

impl ContentIdLookup for FakeContentIds {
    fn id_of(&self, index: ContentIndex) -> Option<&NamespacedId> {
        self.0
            .iter()
            .find(|(known, _)| *known == index)
            .map(|(_, id)| id)
    }
}

fn test_world() -> WorldState {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let spawn = layout.tile_size().wrap(0, 0);
    WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件")
}

/// 造一个占位实体，站在 `(5, 5)`。
fn spawn_agent(world: &mut WorldState) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory: Vec::new(),
        equipment: BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        mod_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
        home: None,
    })
}

fn quest_id() -> NamespacedId {
    NamespacedId::parse("lostland:main_quest_1").expect("固定字面量恒合法")
}

/// 一个索引，与 `ContentIndex::default()` **不同**——夹具里的物品用
/// 默认索引，任务用这一个，两者撞在一起会让「送错了东西」这类错误
/// 看不出来。
fn 任务索引() -> ContentIndex {
    let mut interner = Interner::new();
    interner.intern(NamespacedId::parse("lostland:filler").expect("合法标识符"));
    interner.intern(quest_id())
}

fn 目录<'a>(
    dialogues: &'a FakeDialogues,
    content_ids: &'a FakeContentIds,
) -> ResolveCatalogs<'a> {
    ResolveCatalogs {
        dialogues,
        content_ids,
        ..ResolveCatalogs::empty()
    }
}

fn 一个节点一条选项(node: ContentIndex, outcome: DialogueOutcome) -> FakeDialogues {
    FakeDialogues {
        node,
        options: vec![(Vec::new(), vec![outcome])],
    }
}

fn 选中(actor: EntityId, speaker: EntityId, node: ContentIndex) -> Intent {
    Intent::DialogueChoose {
        actor,
        speaker,
        node,
        option: 0,
    }
}

// ── complete-quest ────────────────────────────────────────────────

/// **主线**：产出的写入**逐字节等于** `mark_quest_completed` 的返回值。
///
/// 这条断言的形状是刻意的：它钉住的不是「任务变成已完成了」（那样一份
/// 在 `resolve` 里另抄一遍键名的实现照样全绿），而是「产出的东西**就是
/// 那个既有函数返回的东西**」——ADR 0021 要求的「同一个算法只有一处
/// 实现」在测试里唯一能表达出来的形式。
///
/// 故意改坏的反例（本批实测，见计划文档六节 ④）：把
/// `ll_sim::quest::mark_quest_completed` 的函数体改坏（写到另一个键上），
/// 本条与下面的 `任务真的变成已完成` 一起红——若 `resolve` 里另抄了一份
/// 完成逻辑，改坏它不会有任何反应。
#[test]
fn complete_quest产出的写入就是mark_quest_completed的返回值() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let speaker = spawn_agent(&mut world);
    let node = ContentIndex::default();
    let quest = 任务索引();
    let dialogues = 一个节点一条选项(node, DialogueOutcome::CompleteQuest(quest));
    let ids = FakeContentIds(vec![(quest, quest_id())]);
    // 对照组前提：结算之前这条任务确实**没有**完成。
    assert!(
        !is_quest_completed(world.actors.get(actor).expect("玩家在"), &quest_id()),
        "结算之前这条任务必须是未完成，否则下面验的不是这一次结算"
    );

    // Act
    let effects =
        resolve_with_catalogs(&world, &选中(actor, speaker, node), &目录(&dialogues, &ids));

    // Assert
    assert_eq!(effects.len(), 1, "一条 complete-quest 恰好产出一条效果");
    let Effect::SetModState { writes } = &effects[0] else {
        panic!("complete-quest 必须走 Effect::SetModState，实际是 {effects:?}");
    };
    assert_eq!(
        writes,
        &vec![mark_quest_completed(actor, &quest_id())],
        "产出的必须就是 mark_quest_completed 的返回值本身"
    );
}

/// 走一遍 `apply`：任务真的变成「已完成」，读的是既有的
/// `is_quest_completed`。
#[test]
fn 结算之后任务真的变成已完成() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let speaker = spawn_agent(&mut world);
    let node = ContentIndex::default();
    let quest = 任务索引();
    let dialogues = 一个节点一条选项(node, DialogueOutcome::CompleteQuest(quest));
    let ids = FakeContentIds(vec![(quest, quest_id())]);

    // Act
    let effects =
        resolve_with_catalogs(&world, &选中(actor, speaker, node), &目录(&dialogues, &ids));
    for effect in &effects {
        apply(&mut world, effect);
    }

    // Assert
    assert!(
        is_quest_completed(world.actors.get(actor).expect("玩家在"), &quest_id()),
        "complete-quest 之后这条任务必须是已完成"
    );
}

/// 反查不到那个索引 ⇒ **零效果**，不 panic、也不写一条键名是索引号的
/// 假记录。与本模块其余闸门同一条纪律。
#[test]
fn 反查不到任务标识符时complete_quest零效果() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let speaker = spawn_agent(&mut world);
    let node = ContentIndex::default();
    let quest = 任务索引();
    let dialogues = 一个节点一条选项(node, DialogueOutcome::CompleteQuest(quest));
    // 对照组：同一份对话，反查得到时**产得出**效果（见上一条），这里
    // 只把反查换成空的。
    let ids = FakeContentIds(Vec::new());

    // Act
    let effects =
        resolve_with_catalogs(&world, &选中(actor, speaker, node), &目录(&dialogues, &ids));

    // Assert
    assert!(effects.is_empty(), "实际产出了 {effects:?}");
}

/// **`complete-quest` 也不消耗回合**（所有者裁定第 2 条）。
///
/// 批次 21 那条 `说完一整轮话世界时钟一格没动` 走的是 `set-flag` 一支、
/// 批次 26 那条走的是 `join-settlement` 一支——`match` 每加一个变体，
/// 旧的那几条就**不再覆盖**新的这一支。每一支都得有自己的这一条。
///
/// 故意改坏的反例（本批实测）：给 `complete-quest` 那一支再补一条
/// `Effect::ScheduleNext`，本条当场红。
#[test]
fn 完成任务不消耗回合() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let speaker = spawn_agent(&mut world);
    let node = ContentIndex::default();
    let quest = 任务索引();
    let dialogues = 一个节点一条选项(node, DialogueOutcome::CompleteQuest(quest));
    let ids = FakeContentIds(vec![(quest, quest_id())]);
    let clock_before = world.clock;
    let next_before = world.actors.get(actor).expect("玩家在").next_action_at;
    let mut timeline = ll_sim::timeline::Timeline::new();
    timeline.schedule(actor, clock_before);
    let mut engine = TurnEngine::new(timeline);
    let mut on_effect = |_world: &WorldState, _effect: &Effect| {};
    let mut ai =
        |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Wait { actor };
    engine.advance_ai(
        &mut world,
        actor,
        &mut ai,
        &目录(&dialogues, &ids),
        &mut on_effect,
    );

    // Act
    let outcome = engine.try_player_intent(
        &mut world,
        actor,
        选中(actor, speaker, node),
        &目录(&dialogues, &ids),
        &mut on_effect,
    );

    // Assert：先确认效果真的落地了，否则「时钟没动」可能只是因为什么都
    // 没发生。
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert!(
        is_quest_completed(world.actors.get(actor).expect("玩家在"), &quest_id()),
        "对照组：任务真的完成了"
    );
    assert_eq!(world.clock, clock_before, "对话不消耗回合，世界时钟不动");
    assert_eq!(
        world.actors.get(actor).expect("玩家在").next_action_at,
        next_before,
        "对话不消耗回合，下次行动时刻不动"
    );
}
