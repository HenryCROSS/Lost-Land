//! `Intent::DialogueChoose` 的结算：条件重新校验、`set-flag` 写入、
//! **以及「对话不消耗回合」**。
//!
//! 与 `quest_resolve.rs` 同一条理由独立成文件：只用公开入口
//! （[`resolve_with_catalogs`]/[`TurnEngine`]），不碰任何私有函数。
//! 这里的 [`FakeDialogues`] 是纯测试用的 [`DialogueCatalog`] 实现——
//! 生产代码里真正的目录是 `ll_mod::dialogue::DialogueNodeTable`（依赖
//! 方向不允许本 crate 依赖它），端到端那一半在
//! `crates/ll-game/tests/dialogue_session.rs`。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::dialogue::{
    DialogueCatalog, DialogueCondition, DialogueOptionView, DialogueOutcome, has_dialogue_flag,
};
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve_with_catalogs;
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 一个只认识**一个节点**的测试目录，那个节点有两条选项。
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

fn flag() -> NamespacedId {
    NamespacedId::parse("lostland:dialogue_flag.heard").expect("固定字面量恒合法")
}

/// 一个节点、两条选项：第 0 条无条件带 `set-flag`，第 1 条要求那条标志
/// **没设过**、且不带任何后果（纯导航）。
fn fake_dialogues(node: ContentIndex) -> FakeDialogues {
    FakeDialogues {
        node,
        options: vec![
            (Vec::new(), vec![DialogueOutcome::SetFlag(flag())]),
            (vec![DialogueCondition::FlagNotSet(flag())], Vec::new()),
        ],
    }
}

fn catalogs<'a>(dialogues: &'a FakeDialogues) -> ResolveCatalogs<'a> {
    ResolveCatalogs {
        dialogues,
        ..ResolveCatalogs::empty()
    }
}

#[test]
fn 选中带set_flag的选项产出一条setmodstate() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let node = ContentIndex::default();
    let dialogues = fake_dialogues(node);

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::DialogueChoose {
            actor,
            node,
            option: 0,
        },
        &catalogs(&dialogues),
    );

    // Assert
    assert_eq!(effects.len(), 1, "一条决策的全部写入攒成一条 Effect");
    let Effect::SetModState { writes } = &effects[0] else {
        panic!(
            "set-flag 必须走 Effect::SetModState，实际是 {:?}",
            effects[0]
        );
    };
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].entity, actor);
    assert_eq!(writes[0].mod_namespace, "lostland");
}

/// **对话不消耗回合**（所有者裁定，交接文档第〇之二节第 2 条）。
///
/// 故意改坏的反例（本批实测）：给 `resolve_dialogue_choose` 的返回值
/// 补一条 `Effect::ScheduleNext { actor, at: ... }`，本条当场变红——
/// `world.clock` 与 `next_action_at` 会一起往前跳。
#[test]
fn 说完一整轮话世界时钟一格没动() {
    // Arrange：把玩家排进时间轴，记下开局的时钟与下次行动时刻。
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let node = ContentIndex::default();
    let dialogues = fake_dialogues(node);
    let mut timeline = ll_sim::timeline::Timeline::new();
    timeline.schedule(actor, Tick(0));
    let mut engine = TurnEngine::new(timeline);
    let clock_before = world.clock;
    let next_before = world
        .actors
        .get(actor)
        .expect("刚生成必然存在")
        .next_action_at;

    // Act：连说五轮，每一轮都是一条带后果的选项。
    let mut on_effect = |_world: &WorldState, _effect: &Effect| {};
    for _ in 0..5 {
        // 每一轮先让引擎弹出玩家（`advance_ai` 的受控实体早退路径），
        // 再提交一次选择——这正是真实游戏里 `run_turn` 走的那条链。
        let mut ai =
            |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Wait { actor };
        engine.advance_ai(
            &mut world,
            actor,
            &mut ai,
            &catalogs(&dialogues),
            &mut on_effect,
        );
        let outcome = engine.try_player_intent(
            &mut world,
            actor,
            Intent::DialogueChoose {
                actor,
                node,
                option: 0,
            },
            &catalogs(&dialogues),
            &mut on_effect,
        );
        assert_eq!(
            outcome,
            PlayerTurnOutcome::Acted,
            "写了标志就是真的发生了事，不能是 Nothing"
        );
    }

    // Assert
    assert_eq!(world.clock, clock_before, "对话不消耗回合：世界时钟不动");
    assert_eq!(
        world.actors.get(actor).expect("还在").next_action_at,
        next_before,
        "对话不消耗回合：下次行动时刻不动"
    );
    assert!(
        has_dialogue_flag(world.actors.get(actor).expect("还在"), &flag()),
        "五轮说下来标志确实设上了——不是因为什么都没发生才时钟不动"
    );
}

/// `resolve` **重新校验**条件，不相信 UI 传来的序号。
///
/// 故意改坏的反例（本批实测）：把 `resolve_dialogue_choose` 里那句
/// `if !all_conditions_hold(...) { return Vec::new(); }` 删掉，本条当场
/// 变红——一条 UI 早就不该显示的选项照样会被结算。
#[test]
fn 条件不再满足的选项在结算侧被拒掉() {
    // Arrange：先把标志设上，于是第 1 条选项（要求「没设过」）不该再
    // 能被选中——但 UI 可能是在设上之前算出来的那一帧。
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let node = ContentIndex::default();
    let dialogues = fake_dialogues(node);
    let choose = |option: usize| Intent::DialogueChoose {
        actor,
        node,
        option,
    };
    let before = resolve_with_catalogs(&world, &choose(1), &catalogs(&dialogues));
    assert!(
        before.is_empty(),
        "第 1 条选项不带后果，无论条件满不满足都产不出效果——这条前置\
         保证下面的断言不是被「本来就没有后果」蒙对的"
    );
    // 真正的对照：把第 1 条选项也挂上后果，条件仍然是「标志没设过」。
    let gated = FakeDialogues {
        node,
        options: vec![(
            vec![DialogueCondition::FlagNotSet(flag())],
            vec![DialogueOutcome::SetFlag(flag())],
        )],
    };

    // Act：标志未设时能结算；设上之后同一条选项被拒。
    let allowed = resolve_with_catalogs(&world, &choose(0), &catalogs(&gated));
    let agent = world.actors.get_mut(actor).expect("刚生成必然存在");
    agent.mod_state.insert(
        (
            "lostland".to_string(),
            ll_sim::dialogue::dialogue_flag_key(&flag()),
        ),
        ll_world::mod_state::ModStateValue::Bool(true),
    );
    let blocked = resolve_with_catalogs(&world, &choose(0), &catalogs(&gated));

    // Assert
    assert_eq!(allowed.len(), 1, "条件满足时照常结算");
    assert!(blocked.is_empty(), "条件不再满足时结算侧拒掉，不产任何效果");
}

#[test]
fn 查不到的节点或越界的选项产出空效果而不是panic() {
    // Arrange
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let node = ContentIndex::default();
    let dialogues = fake_dialogues(node);
    let 别的节点 = {
        // 先 intern 一条占位再取第二条：新建 `Interner` 的第一个索引就是
        // `ContentIndex::default()`，直接取会与 `node` 撞成同一个索引，
        // 这条测试就会在「查不到的节点」这一半上假绿。
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse("lostland:placeholder").expect("合法标识符"));
        interner.intern(NamespacedId::parse("lostland:other_node").expect("合法标识符"))
    };
    assert_ne!(别的节点, node, "对照组必须真的是另一个索引");

    // Act & Assert
    assert!(
        resolve_with_catalogs(
            &world,
            &Intent::DialogueChoose {
                actor,
                node: 别的节点,
                option: 0,
            },
            &catalogs(&dialogues),
        )
        .is_empty()
    );
    assert!(
        resolve_with_catalogs(
            &world,
            &Intent::DialogueChoose {
                actor,
                node,
                option: 99,
            },
            &catalogs(&dialogues),
        )
        .is_empty()
    );
}
