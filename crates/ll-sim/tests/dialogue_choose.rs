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

use ll_core::ident::WorldId;
use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::dialogue::JOIN_SETTLEMENT_STANDING;
use ll_sim::dialogue::{
    DialogueCatalog, DialogueCondition, DialogueOptionView, DialogueOutcome, has_dialogue_flag,
};
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve_with_catalogs;
use ll_sim::turn::{PlayerTurnOutcome, TurnEngine};
use ll_world::entity::{
    Affiliation, AffiliationKind, Agent, BaseStats, EntityId, OrgInstance, OrgRef,
};
use ll_world::faction::{Faction, FactionStatus, FactionTable};
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
            // 本组用例的后果只有 `set-flag`，不读说话人；取 `actor` 自己
            // 是最短的合法值，`join-settlement` 那一支另有专门的用例。
            speaker: actor,
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
                // 本组用例的后果只有 `set-flag`，不读说话人；取 `actor` 自己
                // 是最短的合法值，`join-settlement` 那一支另有专门的用例。
                speaker: actor,
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
        // 本组用例的后果只有 `set-flag`，不读说话人；取 `actor` 自己
        // 是最短的合法值，`join-settlement` 那一支另有专门的用例。
        speaker: actor,
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
                // 本组用例的后果只有 `set-flag`，不读说话人；取 `actor` 自己
                // 是最短的合法值，`join-settlement` 那一支另有专门的用例。
                speaker: actor,
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
                // 本组用例的后果只有 `set-flag`，不读说话人；取 `actor` 自己
                // 是最短的合法值，`join-settlement` 那一支另有专门的用例。
                speaker: actor,
                node,
                option: 99,
            },
            &catalogs(&dialogues),
        )
        .is_empty()
    );
}

// ── join-settlement（对话系统的批次 3）─────────────────────────────

/// 造一张只有一个势力的势力表：`faction` 号的势力统治 `site` 号的据点。
///
/// 走 `FactionTable::rebuild`（**唯一**的有内容构造路径，全部不变式都
/// 在它里面），不手搓内部字段。
fn 一个势力统治一座据点(faction: WorldId, site: WorldId) -> FactionTable {
    FactionTable::rebuild(vec![Faction {
        org: OrgInstance {
            id: faction,
            def: None,
            authored: None,
        },
        seat: site,
        founded_epoch: 0,
        status: FactionStatus::Active,
        members: vec![site],
    }])
    .expect("一个势力一座据点满足全部不变式")
}

/// 一个节点、一条选项：无条件带 `join-settlement`。
fn 加入据点的对话(node: ContentIndex) -> FakeDialogues {
    FakeDialogues {
        node,
        options: vec![(Vec::new(), vec![DialogueOutcome::JoinSettlement])],
    }
}

/// 把「玩家 + 管理者 + 势力表」这一套摆好，返回 `(世界, 玩家, 管理者,
/// 势力号)`。
fn 有管理者的世界() -> (WorldState, EntityId, EntityId, WorldId) {
    let mut world = test_world();
    let actor = spawn_agent(&mut world);
    let speaker = spawn_agent(&mut world);
    let mut counter = 0u32;
    let site = WorldId::next(&mut counter);
    let faction = WorldId::next(&mut counter);
    world
        .actors
        .get_mut(speaker)
        .expect("刚生成的实体必然存在")
        .home = Some(site);
    world.factions = 一个势力统治一座据点(faction, site);
    (world, actor, speaker, faction)
}

/// **加入据点的主线**：产出一条 `Effect::AddAffiliation`，指向说话人那座
/// 据点所属的势力，`standing` 恰好是所有者裁定的 +250。
///
/// 故意改坏的反例（本批实测）：把 `join_settlement` 里的
/// `world.factions.faction_of(home)?` 换成 `Some(home)`（拿据点号冒充
/// 势力号，也就是规格 5.1 那条已作废的变通），本条当场红——`org` 指的
/// 是据点而不是势力。
#[test]
fn 选中join_settlement的选项产出一条指向势力的归属() {
    // Arrange
    let (world, actor, speaker, faction) = 有管理者的世界();
    let node = ContentIndex::default();
    let dialogues = 加入据点的对话(node);
    // 对照组前提：改之前玩家身上一条归属都没有。
    assert!(
        world
            .actors
            .get(actor)
            .expect("玩家在")
            .affiliations
            .is_empty(),
        "结算之前玩家必须一条归属都没有，否则下面的断言可能验的是别人挂的"
    );

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::DialogueChoose {
            actor,
            speaker,
            node,
            option: 0,
        },
        &catalogs(&dialogues),
    );

    // Assert
    assert_eq!(effects.len(), 1, "一条 join-settlement 恰好产出一条效果");
    let Effect::AddAffiliation {
        entity,
        affiliation,
    } = &effects[0]
    else {
        panic!("join-settlement 必须走 Effect::AddAffiliation，实际是 {effects:?}");
    };
    assert_eq!(*entity, actor, "归属挂在**发起者**身上，不是说话人");
    assert_eq!(affiliation.kind, AffiliationKind::Faction);
    assert_eq!(
        affiliation.org,
        OrgRef::Instance(faction),
        "指的必须是**势力**号，不是据点号"
    );
    assert_eq!(
        affiliation.standing, JOIN_SETTLEMENT_STANDING,
        "所有者裁定：加入据点给 +250"
    );
    assert_eq!(JOIN_SETTLEMENT_STANDING, 250, "常量本身就是那个裁定值");
}

/// **说话人没有 `home` → 零效果。** 玩家、以及任何不隶属某座据点的实体
/// 都是这一档。
///
/// 故意改坏的反例（本批实测）：把 `join_settlement` 里的
/// `world.actors.get(speaker)?.home?` 换成 `.home.unwrap_or(<任意号>)`，
/// 本条当场红。
#[test]
fn 说话人没有所属据点时join_settlement零效果() {
    // Arrange
    let (mut world, actor, speaker, _faction) = 有管理者的世界();
    world.actors.get_mut(speaker).expect("说话人在").home = None;
    let node = ContentIndex::default();
    let dialogues = 加入据点的对话(node);

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::DialogueChoose {
            actor,
            speaker,
            node,
            option: 0,
        },
        &catalogs(&dialogues),
    );

    // Assert
    assert!(effects.is_empty(), "实际产出了 {effects:?}");
}

/// **那座据点查不到势力（废墟、或从不存在的号）→ 零效果。**
///
/// 故意改坏的反例（本批实测）：把 `faction_of(home)?` 换成
/// `faction_of(home).unwrap_or(home)`，本条当场红。
#[test]
fn 据点查不到势力时join_settlement零效果() {
    // Arrange：说话人的 home 指向一座**不在任何势力成员表里**的据点。
    let (mut world, actor, speaker, _faction) = 有管理者的世界();
    let mut counter = 900u32;
    world.actors.get_mut(speaker).expect("说话人在").home = Some(WorldId::next(&mut counter));
    let node = ContentIndex::default();
    let dialogues = 加入据点的对话(node);

    // Act
    let effects = resolve_with_catalogs(
        &world,
        &Intent::DialogueChoose {
            actor,
            speaker,
            node,
            option: 0,
        },
        &catalogs(&dialogues),
    );

    // Assert
    assert!(effects.is_empty(), "实际产出了 {effects:?}");
}

/// **加入据点同样不消耗回合**（所有者裁定第 2 条）。
///
/// 批次 21 已经有一条 `说完一整轮话世界时钟一格没动`，但那条走的是
/// `set-flag` 那一支——`match` 加了新变体之后它**不再覆盖**这一支，
/// 因此本批必须补这一条。
///
/// 故意改坏的反例（本批实测）：给 `join_settlement` 的返回值再补一条
/// `Effect::ScheduleNext`，本条当场红。
#[test]
fn 加入据点不消耗回合() {
    // Arrange
    let (mut world, actor, speaker, _faction) = 有管理者的世界();
    let node = ContentIndex::default();
    let dialogues = 加入据点的对话(node);
    let clock_before = world.clock;
    let next_before = world.actors.get(actor).expect("玩家在").next_action_at;
    let mut timeline = ll_sim::timeline::Timeline::new();
    timeline.schedule(actor, clock_before);
    let mut engine = TurnEngine::new(timeline);
    let mut on_effect = |_world: &WorldState, _effect: &Effect| {};

    // Act：先让引擎弹出玩家（受控实体早退路径），再提交——这正是真实
    // 游戏里 `run_turn` 走的那条链，与上面那条 `set-flag` 用例同办。
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
            speaker,
            node,
            option: 0,
        },
        &catalogs(&dialogues),
        &mut on_effect,
    );

    // Assert：效果真的落地了（否则「时钟没动」可能只是因为什么都没发生）。
    assert_eq!(outcome, PlayerTurnOutcome::Acted);
    assert_eq!(
        world.actors.get(actor).expect("玩家在").affiliations.len(),
        1,
        "对照组：归属真的挂上了"
    );
    assert_eq!(world.clock, clock_before, "对话不消耗回合，世界时钟不动");
    assert_eq!(
        world.actors.get(actor).expect("玩家在").next_action_at,
        next_before,
        "对话不消耗回合，下次行动时刻不动"
    );
}
