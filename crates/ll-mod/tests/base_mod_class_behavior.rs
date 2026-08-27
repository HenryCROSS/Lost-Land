//! 端到端验证「**按职业选行为树**」这条内容绑定：本体真实内容
//! （`mods/lostland/classes.json5` 的 `behavior` 字段）+ 经由
//! [`ll_sim::turn::TurnEngine`] 的真实回合推进 + 一条反例（ADR 0018）。
//!
//! # 本文件守的那个缺陷
//!
//! NPC 生成批次（`bc2fc81`）落地之后，`ll_game::app::npc_behavior_source`
//! 把**卫兵那棵树**发给了全部物化出来的 NPC。卫兵那棵树的兜底分支是
//! 「看得见人就走近一步」，于是**整座村子的居民（农夫、屠夫、猎户……）
//! 都会朝玩家走过来**。那批的文档如实标注了这一条，并写明正解是给
//! `ClassDef` 加一条行为绑定。
//!
//! 本文件是那条绑定的验收：
//!
//! 1. [`本体内容真的把农夫绑到了平民原型`]——内容侧断言。
//! 2. [`农夫经由回合引擎跑一百二十轮也不会朝玩家走过来`]——**本批次的
//!    验收核心**，端到端。
//! 3. [`摘掉职业绑定这条接线农夫立刻开始朝玩家走`]——**反例**：把
//!    `with_class_bindings` 那一句摘掉（回到本批次之前的接法），同一
//!    段场景里农夫会一路贴到玩家脸上，本条因此变红。
//! 4. [`卫兵仍然走守卫那棵树`]——守住「按职业选树」不是「一律平民」。
//!
//! # 「真实」的口径
//!
//! 内容来自真实 `mods/`（`LoadSession::with_engine_registrations` +
//! `load_all`，与本体二进制同一条装载路径），绑定表来自那次装载产出的
//! `LoadSession::class_behavior_bindings`，**不是**测试自己拼的一张表。
//! 结算经由 `TurnEngine::advance_ai` + `behavior_ai_intent`，与
//! `ll_game::app::Demo::advance` 是同一条接法。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::behavior_binding::{BehaviorArchetype, ClassBehaviorBindings};
use ll_mod::class::ClassTable;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
use ll_mod::item::ItemTable;
use ll_mod::load_session::LoadSession;
use ll_mod::native_behavior::{BehaviorRuleCatalogs, NativeBehaviorSource, NativeBehaviorTree};
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_platform::input::{GameKey, InputState};
use ll_sim::behavior::behavior_ai_intent;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::craft::NoRecipes;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::quest::NoQuests;
use ll_sim::skill::NoSkills;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `turn_engine_catalogs.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 一场观察跑多少轮（一轮 = 一次 `advance_ai` + 一次玩家「等待」）。
///
/// 120 轮足够让守卫那棵树把 [`INITIAL_GAP`] 格的距离走完并贴上去，也
/// 足够让平民那棵树的随机游走（
/// [`ll_mod::native_behavior::TOWNSFOLK_WANDER_PERMILLE`] 是 250‰）
/// 动上几十步——两条分支在这个尺度上分得很开。
const OBSERVED_TURNS: u32 = 120;

/// 农夫与玩家的初始切比雪夫距离。
///
/// 取 9：小于 `NEARBY_ACTOR_VIEW_RADIUS`（12），因此守卫那棵树**看得
/// 见**玩家、会走过来；同时留出足够的余量，让「平民没有走过来」不至于
/// 被随机游走的几步噪声淹没。
const INITIAL_GAP: i32 = 9;

/// 一场跑完之后，平民允许与玩家靠得多近。
///
/// **实测的余量在这里，不是估出来的**（本文件当前这颗种子、这段场景）：
///
/// | 决策来源 | 全程最近距离 | 末尾距离 |
/// |---|---|---|
/// | 平民（接上绑定） | **8** 格 | 14 格（一路漂远） |
/// | 守卫（摘掉绑定，本批次之前的接法） | **0** 格（走到同一格） | 0 格 |
///
/// 取 5 作为下界：比实测的 8 还宽三格，而与另一侧的 0 之间隔着整整
/// 八格。随机游走是无偏的（40 步左右、位移标准差约 3 格），这条断言
/// 因此不会因为换一颗种子就翻脸，也不会把「其实走过来了」放过去。
const TOWNSFOLK_MIN_GAP: u32 = 5;

/// 真实装载出来的那批表与索引。
struct RealModsHandle {
    registry: Registry,
    race: RaceTable,
    class: ClassTable,
    subclass: SubclassTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    skill: SkillTable,
    resource_pool: ResourcePoolTable,
    class_behavior_bindings: ClassBehaviorBindings,
    farmer_id: ContentIndex,
    guard_id: ContentIndex,
    human_id: ContentIndex,
}

impl RealModsHandle {
    /// 把真实装载出来的表借成结算目录束——与
    /// `turn_engine_catalogs.rs::RealModsHandle::catalogs` 同一形状、
    /// 同一批表（本体二进制交给 `TurnEngine` 的也是这个形状）。
    fn catalogs<'a>(&'a self, formulas: &'a RegistryFormulas<'a>) -> ResolveCatalogs<'a> {
        ResolveCatalogs {
            skills: &self.skill,
            quests: &NoQuests,
            race_traits: &self.race,
            class_traits: &self.class,
            subclass_traits: &self.subclass,
            trait_defs: &self.trait_def,
            pools: &self.resource_pool,
            items: &self.item,
            formulas,
            damage_categories: &NoDamageCategories,
            recipes: &NoRecipes,
            // 本文件的场景都在一个占位层属性的地表上，温度这一路没有
            // 可查的表，理由同 `turn_engine_catalogs.rs` 同一位置。
            ambient: AmbientSource::NONE,
            experience: &NoExperience,
            skill_tree: &NO_SKILLS,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
        }
    }

    /// 生产接线：平民兜底 + 真实内容绑定，与
    /// `ll_game::app::npc_behavior_source` 逐字同形。
    fn production_source(&self) -> NativeBehaviorSource {
        NativeBehaviorSource::new(
            NativeBehaviorTree::townsfolk(),
            self.rule_catalogs(),
            WORLD_SEED,
        )
        .with_class_bindings(self.class_behavior_bindings.clone(), &self.registry)
    }

    /// **本批次之前**的接线：卫兵那棵树发给所有人，没有任何职业绑定。
    /// 只有反例那条测试用它。
    fn pre_batch_source(&self) -> NativeBehaviorSource {
        NativeBehaviorSource::new(
            NativeBehaviorTree::guard(&self.registry),
            self.rule_catalogs(),
            WORLD_SEED,
        )
    }

    fn rule_catalogs(&self) -> BehaviorRuleCatalogs {
        BehaviorRuleCatalogs::snapshot(
            &self.race,
            &self.class,
            &self.subclass,
            &self.trait_def,
            &self.item,
        )
    }
}

/// 空技能树目录——`ResolveCatalogs::skill_tree` 要一个 `'static` 借用。
static NO_SKILLS: NoSkills = NoSkills;

/// 本文件全部场景共用的世界种子。
const WORLD_SEED: u64 = 20_260_826;

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let _report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        class,
        skill,
        race,
        subclass,
        trait_def,
        resource_pool,
        item,
        formula,
        class_behavior_bindings,
        ..
    } = session;

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).expect("合法标识符"))
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/lostland/ 的内容文件注册"))
    };
    let farmer_id = resolve("lostland:farmer");
    let guard_id = resolve("lostland:guard");
    let human_id = resolve("lostland:human");

    RealModsHandle {
        registry,
        race,
        class,
        subclass,
        trait_def,
        item,
        formula,
        skill,
        resource_pool,
        class_behavior_bindings,
        farmer_id,
        guard_id,
        human_id,
    }
}

/// 造一间 1×1 分区的空白世界。
fn test_world() -> WorldState {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let spawn = layout.tile_size().wrap(0, 0);
    let mut world = WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件");
    // 决策来源的随机流由 `world.seed` 派生（C3）——两场景要可比就得
    // 用同一颗种子。
    world.seed = WORLD_SEED;
    world
}

fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    profession: ContentIndex,
    pos: (i32, i32),
) -> EntityId {
    let agent_pos = world.size.wrap(pos.0, pos.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(agent_pos);
    world.actors.spawn(Agent {
        pos: agent_pos,
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
    })
}

/// 一场观察的结果。
struct Observation {
    /// 每回合结束时 NPC 与玩家的切比雪夫距离，长度 [`OBSERVED_TURNS`]。
    gaps: Vec<u32>,
    /// 每回合结束时 NPC 的位置——两场景比对轨迹用。
    trail: Vec<(i32, i32)>,
}

impl Observation {
    fn closest(&self) -> u32 {
        self.gaps.iter().copied().min().expect("至少跑了一回合")
    }
}

/// 让一个 `profession` 的 NPC 在玩家旁边跑 [`OBSERVED_TURNS`] 回合，
/// 全程经由 [`TurnEngine::advance_ai`]（本体二进制驱动 AI 的唯一入口）。
///
/// `player_offset` 是玩家相对 NPC 的位移——两条测试用**不同的方向**
/// 摆玩家，用来证明平民的轨迹与玩家在哪逐位无关。
///
/// # 每回合两步，与本体二进制逐字相同
///
/// 先 `advance_ai` 结算排在玩家之前的非受控实体，再 `try_player_turn`
/// 用一次「等待」输入消费掉玩家自己的回合——这与
/// `ll_game::app::Demo::advance` 每帧跑的两步一模一样，也与
/// `example_mod_stealth.rs` 那条既有测试同一个写法。玩家全程按兵不动，
/// 那恰恰是「农夫会不会朝一个不动的人走过去」这个问题最干净的形式。
fn observe(
    handle: &RealModsHandle,
    source: &mut NativeBehaviorSource,
    profession: ContentIndex,
    player_offset: (i32, i32),
) -> Observation {
    let mut world = test_world();
    let npc = spawn_agent(&mut world, handle.human_id, profession, (20, 20));
    let player = spawn_agent(
        &mut world,
        handle.human_id,
        handle.guard_id,
        (20 + player_offset.0, 20 + player_offset.1),
    );

    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        // 本文件的实体全都徒手，不会走到显式公式引用那条路。
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);

    let mut timeline = Timeline::new();
    timeline.schedule(npc, Tick(0));
    timeline.schedule(player, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let mut wait_input = InputState::new();
    wait_input.press(GameKey::Wait);

    let mut gaps = Vec::with_capacity(OBSERVED_TURNS as usize);
    let mut trail = Vec::with_capacity(OBSERVED_TURNS as usize);
    for _ in 0..OBSERVED_TURNS {
        {
            // 一轮里 NPC 可能行动一次、也可能一次都不动（两者的行动
            // 耗时不同，时间轴上谁排在前面每轮都可能换），本文件不对
            // 「这一轮谁动了」做断言——要观察的是**位置**随回合的演化。
            let mut ai_intent = behavior_ai_intent(source);
            engine.advance_ai(
                &mut world,
                player,
                &mut ai_intent,
                &catalogs,
                &mut |_, _| {},
            );
        }
        engine.try_player_turn(&mut world, player, &wait_input, &catalogs, &mut |_, _| {});
        let npc_pos = world.actors.get(npc).expect("NPC 不会死").pos;
        let player_pos = world.actors.get(player).expect("玩家不会死").pos;
        gaps.push(world.size.chebyshev(npc_pos, player_pos));
        trail.push((npc_pos.x(), npc_pos.y()));
    }
    Observation { gaps, trail }
}

#[test]
fn 本体内容真的把农夫绑到了平民原型() {
    // Arrange / Act
    let handle = load_real_mods();

    // Assert：绑定表来自真实装载，不是测试拼出来的。
    assert_eq!(
        handle.class_behavior_bindings.archetype(handle.farmer_id),
        Some(BehaviorArchetype::Townsfolk),
        "mods/lostland/classes.json5 的农夫应当声明 behavior: \"townsfolk\""
    );
    assert_eq!(
        handle.class_behavior_bindings.archetype(handle.guard_id),
        Some(BehaviorArchetype::Sentry),
        "卫兵应当声明 behavior: \"sentry\""
    );
    // 十条：卫兵 + 民兵（守卫型）+ 八条平民型据点职业。三条冒险者职业
    // 刻意不绑（玩家不由行为树驱动）。
    assert_eq!(
        handle.class_behavior_bindings.len(),
        10,
        "本体十三条职业里应当恰好十条声明了 behavior"
    );
}

#[test]
fn 农夫经由回合引擎跑一百二十轮也不会朝玩家走过来() {
    // Arrange
    let handle = load_real_mods();
    let mut source = handle.production_source();

    // Act
    let observed = observe(&handle, &mut source, handle.farmer_id, (INITIAL_GAP, 0));

    // Assert：这是本批次的验收核心。
    assert!(
        observed.closest() >= TOWNSFOLK_MIN_GAP,
        "农夫全程最近距离 {} 格，不应当靠到 {TOWNSFOLK_MIN_GAP} 格以内；\
         逐回合距离：{:?}",
        observed.closest(),
        observed.gaps
    );
}

#[test]
fn 平民的轨迹与玩家站在哪里逐位无关() {
    // Arrange：同一颗种子、同一个 NPC，唯一的差别是玩家摆在东边还是
    // 南边。平民那棵树拿不到 `&WorldState`（见
    // `ll_mod::native_behavior::townsfolk_tick` 文档），因此这两条轨迹
    // 必须逐位相同——这比「没有靠近」更强：它证明的是**玩家的位置根本
    // 没有进入这棵树的输入**。
    let handle = load_real_mods();

    // Act
    let east = observe(
        &handle,
        &mut handle.production_source(),
        handle.farmer_id,
        (INITIAL_GAP, 0),
    );
    let south = observe(
        &handle,
        &mut handle.production_source(),
        handle.farmer_id,
        (0, INITIAL_GAP),
    );

    // Assert
    assert_eq!(
        east.trail, south.trail,
        "农夫的逐回合位置不应当随玩家摆在哪里而变化"
    );
}

#[test]
fn 摘掉职业绑定这条接线农夫立刻开始朝玩家走() {
    // Arrange：**反例**。`pre_batch_source` 就是本批次之前的那条接法
    // ——卫兵那棵树发给所有人、没有任何职业绑定。若有人把
    // `with_class_bindings` 从生产路径上摘掉，生产行为就退回这里，
    // 而下面这条断言正是那种行为的画像。
    let handle = load_real_mods();
    let mut source = handle.pre_batch_source();

    // Act：同一个农夫、同一段场景，只换决策来源。
    let observed = observe(&handle, &mut source, handle.farmer_id, (INITIAL_GAP, 0));

    // Assert：卫兵那棵树的兜底分支是「看得见人就走近一步」，四十回合
    // 足够把 9 格走完并贴到相邻格。
    assert!(
        observed.closest() <= 1,
        "没有职业绑定时农夫会一路走到玩家身边（贴脸或同格）；逐轮距离：{:?}",
        observed.gaps
    );
    // 与生产接线的差别必须是**可观察的一整个数量级**，否则上面那条
    // 验收断言就只是在赌随机游走的运气。
    let production = observe(
        &handle,
        &mut handle.production_source(),
        handle.farmer_id,
        (INITIAL_GAP, 0),
    );
    assert!(
        production.closest() > observed.closest(),
        "接上绑定之后农夫应当明显离得更远：接上 {} 格 vs 摘掉 {} 格",
        production.closest(),
        observed.closest()
    );
}

#[test]
fn 卫兵仍然走守卫那棵树() {
    // Arrange：按职业选树不等于「一律平民」——守卫型那一支必须还在，
    // 否则本批次就是用一个新缺陷换掉了旧缺陷。
    let handle = load_real_mods();
    let mut source = handle.production_source();

    // Act
    let observed = observe(&handle, &mut source, handle.guard_id, (INITIAL_GAP, 0));

    // Assert
    assert!(
        observed.closest() <= 1,
        "卫兵应当走向视野内的人；逐轮距离：{:?}",
        observed.gaps
    );
}
