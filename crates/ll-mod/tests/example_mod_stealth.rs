//! 端到端验证：潜行与盗贼被动批次——三条互补的证据链。
//!
//! 1. **潜行状态真的经 [`ll_sim::turn::TurnEngine`] 翻转**（本体二进制
//!    `ll-game` 驱动世界的唯一路径），不是靠测试直接调 `resolve` +
//!    `apply` 自证——与 `turn_engine_catalogs.rs` 立下的那条更高的验收
//!    标准一致，见其模块文档。
//! 2. **潜行真的让偷袭更容易得手，但不是必定得手**，且用的是真实
//!    `mods/example_mod/traits.json5` 注册的天赋（`examplemod:footpad`
//!    种族授予的 `examplemod:predatory_instinct`），同样经由
//!    `TurnEngine`。潜行此前是偷袭判定的一条**直通**（必定触发），
//!    项目所有者裁定去掉那条「必定」，潜行改成判定里的一个修正
//!    （见 `ll_sim::combat::STEALTH_SNEAK_MODIFIER`），因此本条从
//!    「单次精确等式」改成「频率 + 两端不封顶」，论证见那条测试的
//!    函数文档。
//! 3. **潜行真的让卫兵的盘查率下降，且不是靠让卫兵看不见你**——用的是
//!    真实的 `ll_mod::native_behavior::NativeBehaviorTree::guard`，不是
//!    内联一份逻辑副本，与 `example_mod_guard_inspection.rs` 同一条
//!    纪律。这一条同时钉死本批次的核心设计选择：`compute_fov`/
//!    `VisibleSet` 一个字节都没改，潜行中的目标照样被
//!    `ll_sim::ai_query::nearest_visible_actor` 找到（卫兵照常朝它走），
//!    变的只是掷骰那一次判定。
//! 4. **行为树真的经 [`ll_sim::turn::TurnEngine`] 驱动结算**——第 3 条
//!    与第 4 条走的是同一段代码（[`guard_turns`]），因此不是两条独立
//!    证据，而是同一条证据在两个维度上的断言。
//!
//! # 第 3/4 条此前为什么不经 `TurnEngine`，现在为什么可以
//!
//! 此前不行：[`ll_sim::turn::TurnEngine::advance_ai`] 的 `ai_intent`
//! 曾经是一个**函数指针**，而决策来源的 `decide` 需要 `&mut self`，
//! 捕获不进函数指针——「行为树经由 `TurnEngine` 生效」这条路径在类型
//! 层面就不存在。
//!
//! 现在可以：`ai_intent` 已经放宽成 `&mut dyn FnMut`，
//! `ll_sim::behavior::behavior_ai_intent` 把任意 `BehaviorTreeSource`
//! （这里就是真实的 `NativeBehaviorSource`）包成它要的那个闭包。本文件
//! 因此走完整链路：
//!
//! ```text
//! NativeBehaviorSource::decide → behavior_ai_intent
//!   → TurnEngine::advance_ai → perform → resolve → apply → WorldState
//! ```
//!
//! 这条链路与 `ll_game::app::Demo::advance` 每帧跑的那一条逐字相同
//! （`advance_ai` + `try_player_turn` 两步，本文件的
//! [`guard_turns`] 同样两步都跑）。
//!
//! # 反例是什么
//!
//! [`非卫兵职业的实体经由turnengine一次盘查都不会发起`]：同一段代码、
//! 同一棵真实行为树、同一个几何布局，只把卫兵的 `profession` 换成一个
//! 不是 `lostland:guard` 的索引，`Effect::Inspect` 必须一条都不产出。
//! 把 `mods/lostland/classes.json5` 的那条卫兵职业删掉，或者把
//! `native_behavior::guard_try_inspect` 里的职业判定摘掉，正面那条测试
//! 立刻变红。
use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
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
use ll_sim::effect::Effect;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::quest::NoQuests;
use ll_sim::skill::NoSkills;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_sim::xp_curve::FlatXpCurve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `turn_engine_catalogs.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 防御方的起始生命——远高于一次攻击可能造成的伤害，本文件的场景都
/// 不触发击杀，理由同 `turn_engine_catalogs.rs::DEFENDER_HEALTH`。
const DEFENDER_HEALTH: i32 = 1_000;

/// `mods/example_mod/traits.json5` 里
/// `(register-trait-sneak-attack "examplemod:predatory_instinct" 20 15)`
/// 的第二个参数——偷袭触发后追加的固定伤害。写成常量并在断言里精确
/// 比对，而不是只断言「更高」：这个数字来自真实脚本，脚本改了这条
/// 测试就该变红。
const PREDATORY_INSTINCT_EXTRA_DAMAGE: i32 = 15;

/// 装载真实 `mods/` 一次，返回本文件断言需要的表与索引。
struct RealModsHandle {
    race: RaceTable,
    class: ClassTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    skill: SkillTable,
    resource_pool: ResourcePoolTable,
    registry: Registry,
    footpad_id: ContentIndex,
    guard_id: ContentIndex,
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
            // 副职天赋那一路接空实现：本文件的实体 `subclasses` 恒为空,
            // 接真实副职表与接空实现逐位等价。那一路真的接进生产路径的
            // 证据在 `example_mod_subclass_traits.rs`。
            subclass_traits: &ll_sim::traits::NoTraitGrants,
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
}

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        class,
        skill,
        race,
        trait_def,
        resource_pool,
        item,
        formula,
        ..
    } = session;
    let examplemod_id = NamespacedId::parse("examplemod:self").expect("合法标识符");
    assert_eq!(
        report
            .entries
            .iter()
            .find(|(id, _)| *id == examplemod_id)
            .map(|(_, status)| status),
        Some(&LoadStatus::Loaded),
        "examplemod 必须成功加载，否则下面的索引解析毫无意义"
    );

    let footpad_id = registry
        .get(&NamespacedId::parse("examplemod:footpad").expect("合法标识符"))
        .expect("examplemod:footpad 应当已被 gameplay.scm 注册");
    // `lostland:guard`：`self-has-profession?` 靠这份注册表的快照才认
    // 得出 `native_behavior` 里写的这个字符串，因此它必须在表里。
    //
    // **`get` 而不是 `intern`**——这一行本身就是一条断言：卫兵职业现在
    // 是 `mods/lostland/classes.json5` 里一条真实注册的本体内容，装载
    // 管线跑完之后它必须已经在注册表里。此前这里写的是 `intern`
    // （「查不到就现造一个」），因为整个仓库里根本没有任何脚本注册过
    // 这个职业——`native_behavior` 引用它，`guard_try_inspect` 的第一个
    // `if` 却恒为假，卫兵在真实游戏里永远不会盘查。换成 `get` 之后，
    // 那条注册一旦被删掉，本文件的全部卫兵用例会立刻在这里失败并点名
    // 原因，而不是静默退化成「测试自己造了一个，生产里没有」。
    let guard_id = registry
        .get(&NamespacedId::parse("lostland:guard").expect("合法标识符"))
        .expect("lostland:guard 应当已被 mods/lostland/classes.json5 注册");

    RealModsHandle {
        footpad_id,
        guard_id,
        registry,
        race,
        class,
        trait_def,
        item,
        formula,
        skill,
        resource_pool,
    }
}

fn test_world() -> (WorldState, BaseTerrainIds) {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let spawn = layout.tile_size().wrap(0, 0);
    let world = WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件");
    (world, terrain_ids)
}

/// 一个不属于任何真实注册职业的占位职业索引，理由同
/// `turn_engine_catalogs.rs::placeholder_profession`。
fn placeholder_profession() -> ContentIndex {
    let mut interner = Interner::new();
    interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"))
}

fn placeholder_race() -> ContentIndex {
    let mut interner = Interner::new();
    interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"))
}

/// 造一个占位实体。`stats` 恒取 [`BaseStats::BASELINE`]——**幸运因此
/// 恒为 `0`**，这正是第 2 条证据链的前提（见本文件模块文档）。
fn spawn_agent(
    world: &mut WorldState,
    race: ContentIndex,
    profession: ContentIndex,
    pos: (i32, i32),
    health: i32,
) -> EntityId {
    let agent_pos = world.size.wrap(pos.0, pos.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(agent_pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos: agent_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
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

/// 喂给 [`TurnEngine::advance_ai`] 的 AI 策略：非受控实体一律切换潜行。
/// 函数指针（不是闭包），与 `advance_ai` 的既有签名一致。
fn toggle_stealth(_world: &WorldState, actor: EntityId, _controlled: EntityId) -> Intent {
    Intent::ToggleStealth { actor }
}

/// 喂给 [`TurnEngine::advance_ai`] 的 AI 策略：非受控实体一律攻击受控
/// 实体，理由同 `turn_engine_catalogs.rs::attack_controlled`。
fn attack_controlled(_world: &WorldState, actor: EntityId, controlled: EntityId) -> Intent {
    Intent::Attack {
        actor,
        target: controlled,
    }
}

/// 硬要求一：潜行状态真的经 `TurnEngine` 翻转。
#[test]
fn 切换潜行经由turnengine真的改写世界状态() {
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let sneaker = spawn_agent(
        &mut world,
        placeholder_race(),
        placeholder_profession(),
        (5, 5),
        Agent::STARTING_HEALTH,
    );
    let bystander = spawn_agent(
        &mut world,
        placeholder_race(),
        placeholder_profession(),
        (20, 20),
        Agent::STARTING_HEALTH,
    );
    assert!(
        !world.actors.get(sneaker).expect("刚生成").stealthed,
        "起始状态应当是未潜行，否则下面的断言证明不了任何事"
    );

    let mut timeline = Timeline::new();
    timeline.schedule(sneaker, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    // Act：`bystander` 当受控实体，`advance_ai` 因此恰好结算 `sneaker`
    // 一次行动就返回（理由同 `turn_engine_catalogs.rs` 同一手法）。
    let acted = engine.advance_ai(
        &mut world,
        bystander,
        &mut toggle_stealth,
        &ResolveCatalogs::empty(),
        &mut |_, _| {},
    );

    // Assert：真的翻转了，且真的消耗了一个回合（下一次行动被排到了
    // 世界起始时刻之后）。
    assert_eq!(acted, vec![sneaker]);
    let after = world.actors.get(sneaker).expect("没有击杀，实体仍在");
    assert!(after.stealthed, "经 TurnEngine 切换后潜行状态应当为真");
    assert!(
        after.next_action_at.0 > 0,
        "切换潜行应当消耗一个回合，下一次行动时刻必须被推后"
    );
}

/// 硬要求二：潜行让真实注册的盗贼系天赋**更容易**偷袭得手，但**不是
/// 必定**——经由 `TurnEngine`，内容来自真实 `mods/`。
///
/// # 这条测试为什么从「精确等式」变成「频率 + 两端不封顶」
///
/// 潜行此前是偷袭判定的一条**直通**（`resolve_attack` 里那句
/// `Some(rule) if attacker.stealthed =>` 跳过掷骰），因此一次采样就能
/// 断出精确等式。那是一条「必定成功」，与项目所有者「不允许绝对」直接
/// 冲突；所有者裁定改成一次判定（原话「就算是概率最小都可以」），潜行
/// 从一条分支变成一个修正（一整颗骰子，见
/// `ll_sim::combat::STEALTH_SNEAK_MODIFIER`）。单次采样从此测不出这条
/// 效果，只有频率能。
///
/// # 「这一下到底有没有触发偷袭」怎么读出来
///
/// 不靠猜伤害数值，也不靠减去暴击：每一轮试验并排跑**三场**，第三场
/// 用的是一个**同样布局、同样生成顺序、只把攻击者换成不带偷袭天赋的
/// 占位种族**的世界。生成顺序相同 → 两个世界里攻击者/防御方拿到的
/// `EntityId` 逐位相同 → 暴击流与伤害公式骰子流
/// （三元组都只含 `(种子, 实体, 时钟)`）也逐位相同。于是第三场的伤害
/// 就是**这一下攻击去掉偷袭之后的基准**，「有没有触发」= 「有没有比
/// 基准高」，与暴击是否命中完全解耦。
///
/// 偷袭流的三元组同样不含 `stealthed`，因此潜行那一场与不潜行那一场
/// 掷出的点数也逐位相同，唯一的差别是加在上面的修正——这让下面
/// 「潜行不该让偷袭更难触发」那条单调性断言成为一条真正的不变式，
/// 而不是一句统计上的期望。
///
/// # 三条断言各钉死什么
///
/// 1. `潜行那一侧严格更多` —— 潜行确实有用（把
///    `STEALTH_SNEAK_MODIFIER` 改成 `0`，本条立刻红）。
/// 2. `潜行那一侧没有全中` —— **本批次的核心**：潜行不再是「必定
///    成功」（把那条守卫分支加回去，本条立刻红）。
/// 3. `不潜行那一侧两端都不封顶` —— 反面：偷袭本身也没有变成必定不
///    触发或必定触发。
#[test]
fn 潜行让真实盗贼天赋更容易偷袭得手但不是必定得手() {
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        // 本场景的攻击者徒手（没有装备任何武器），不会走到显式公式
        // 引用那条路，默认值不会被真的用到——理由同
        // `turn_engine_catalogs.rs` 同一处注释。
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);

    // 两套世界各只造一次，逐轮试验只重置那几个会被这一下攻击改到的
    // 字段——每轮重造 64×64 的世界会把本条测试拖慢两个数量级，而这条
    // 测试要的只是「换一条随机流再打一次」。
    //
    // 两套世界的**生成顺序必须一致**（先攻击者、后防御方），
    // `EntityId` 才会逐位对上，见本函数文档。
    let scenario = |attacker_race: ContentIndex| -> (WorldState, EntityId, EntityId) {
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(
            &mut world,
            attacker_race,
            placeholder_profession(),
            (5, 5),
            Agent::STARTING_HEALTH,
        );
        let defender = spawn_agent(
            &mut world,
            placeholder_race(),
            placeholder_profession(),
            (6, 5),
            DEFENDER_HEALTH,
        );
        (world, attacker, defender)
    };
    let (mut rogue_world, rogue_attacker, rogue_defender) = scenario(handle.footpad_id);
    let (mut plain_world, plain_attacker, plain_defender) = scenario(placeholder_race());
    assert_eq!(
        (rogue_attacker, rogue_defender),
        (plain_attacker, plain_defender),
        "两套世界的生成顺序相同，实体标识必须逐位对上"
    );

    // 一轮试验里打一下，返回防御方掉了多少血。
    let strike = |world: &mut WorldState, seed: u64, stealthed: bool| -> i32 {
        world.seed = seed;
        world.clock = Tick(0);
        {
            let agent = world.actors.get_mut(rogue_attacker).expect("刚生成");
            agent.stealthed = stealthed;
            agent.health = Agent::STARTING_HEALTH;
            agent.stamina = Agent::STARTING_STAMINA;
            agent.next_action_at = Tick(0);
        }
        {
            let agent = world.actors.get_mut(rogue_defender).expect("刚生成");
            agent.health = DEFENDER_HEALTH;
            agent.stamina = Agent::STARTING_STAMINA;
            agent.next_action_at = Tick(1);
        }
        let mut timeline = Timeline::new();
        timeline.schedule(rogue_attacker, Tick(0));
        timeline.schedule(rogue_defender, Tick(1));
        let mut engine = TurnEngine::new(timeline);
        let acted = engine.advance_ai(
            world,
            rogue_defender,
            &mut attack_controlled,
            &catalogs,
            &mut |_, _| {},
        );
        assert_eq!(
            acted,
            vec![rogue_attacker],
            "本场景应当恰好结算攻击方一次行动"
        );

        DEFENDER_HEALTH
            - world
                .actors
                .get(rogue_defender)
                .expect("防御方生命远高于单次伤害，不应死亡")
                .health
    };

    // 试验轮数：潜行那一侧每轮约 2.49% 打不出偷袭（见
    // `ll_sim::combat::STEALTH_SNEAK_MODIFIER` 文档那张表），400 轮上
    // 期望约 10 次落空——足够让断言 2 有话可说，又不至于慢。种子逐轮
    // 递增，因此本条测试**是确定性的**：同一份代码永远给出同一批结果，
    // 不是一条会偶发变红的统计测试。
    let trials = 400u64;
    let mut stealth_hits = 0i32;
    let mut visible_hits = 0i32;

    // Act
    for trial in 0..trials {
        let seed = 20_260_825 + trial;
        // 基准场：同一条暴击流与骰子流，只是攻击者没有偷袭天赋。
        // 潜行与否对它毫无影响（没有天赋就没有偷袭判定），取不潜行。
        let baseline = strike(&mut plain_world, seed, false);
        let visible = strike(&mut rogue_world, seed, false);
        let stealthed = strike(&mut rogue_world, seed, true);

        assert!(
            stealthed >= visible,
            "潜行只加修正、不减修正，不该让偷袭更难触发"
        );
        for (damage, label) in [(visible, "不潜行"), (stealthed, "潜行")] {
            let gap = damage - baseline;
            assert!(
                gap == 0 || gap == PREDATORY_INSTINCT_EXTRA_DAMAGE,
                "{label}那一场与基准场共用同一条暴击流，伤害差只可能是 0 或                  {PREDATORY_INSTINCT_EXTRA_DAMAGE}，实得 {gap}"
            );
        }
        if visible > baseline {
            visible_hits += 1;
        }
        if stealthed > baseline {
            stealth_hits += 1;
        }
    }

    // Assert
    assert!(
        stealth_hits > visible_hits,
        "潜行那一侧触发次数应当严格更多（潜行 {stealth_hits} / 不潜行 {visible_hits}）"
    );
    assert!(
        stealth_hits < trials as i32,
        "潜行也不该必定触发——这正是本批次去掉的那条「必定成功」         （潜行 {stealth_hits} / 共 {trials} 轮）"
    );
    assert!(
        visible_hits > 0 && visible_hits < trials as i32,
        "不潜行那一侧两端都不该封顶（{visible_hits} / 共 {trials} 轮）"
    );
}

/// 硬要求三：潜行中发起的这一下攻击之后，潜行被破除——同样经由
/// `TurnEngine`（`Effect::SetStealth` 真的被 `apply` 了）。
#[test]
fn 潜行中攻击一次之后经由turnengine破除潜行() {
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(
        &mut world,
        placeholder_race(),
        placeholder_profession(),
        (5, 5),
        Agent::STARTING_HEALTH,
    );
    let defender = spawn_agent(
        &mut world,
        placeholder_race(),
        placeholder_profession(),
        (6, 5),
        DEFENDER_HEALTH,
    );
    world.actors.get_mut(attacker).expect("刚生成").stealthed = true;

    let mut timeline = Timeline::new();
    timeline.schedule(attacker, Tick(0));
    timeline.schedule(defender, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    // Act
    engine.advance_ai(
        &mut world,
        defender,
        &mut attack_controlled,
        &ResolveCatalogs::empty(),
        &mut |_, _| {},
    );

    // Assert
    assert!(
        !world.actors.get(attacker).expect("没有击杀").stealthed,
        "攻击应当破除攻击者自己的潜行"
    );
}

/// 摆一个「卫兵盯着一个目标」的场景，用真实的卫兵行为树经由
/// [`TurnEngine`] 连续推进 `turns` 个卫兵回合，返回这段窗口里卫兵各
/// 产出了多少条 `Effect::Inspect`（盘查）、多少条 `Effect::MoveTo`
/// （看见了但没盘查，朝目标走）。
///
/// 两个计数都要：只数盘查次数无法区分「潜行降低了判定成功率」与
/// 「潜行让卫兵干脆看不见了」——后者会让移动次数也一起归零，那正是
/// 本批次刻意**没有**采用的那条设计。
///
/// # 为什么两个计数加起来恒等于 `turns`
///
/// 卫兵每个回合恰好产出一条这两种效果之一：卫兵那棵树只有两个分支
/// （盘查 / 走近），兜底的「原地等待」只在视野内找不到目标时才会
/// 命中。调用方据此断言「卫兵每一回合都有动作」——
/// 若哪天潜行被改成「改视野」，潜行那一侧会整体退化成 `Intent::Wait`
/// （既不 `Inspect` 也不 `MoveTo`），这条恒等式立刻不成立。
///
/// # 地形：显式铺一片草地
///
/// 卫兵这次会**真的移动**（不再只是「决策一次、不落地」），落脚点的
/// 地形因此参与结算：撞墙的 `Intent::Move` 只产出 `ScheduleNext`、
/// 不产出 `MoveTo`，会让上面那条恒等式因为一个与潜行毫无关系的原因
/// 变红。把两人周围这一小片显式铺成草地，把「地形长什么样」这个变量
/// 从本用例里摘掉。
/// 一段卫兵回合的**三态**计数：这一回合卫兵要么盘查了、要么真的挪了
/// 一格、要么撞在目标身上没挪成。
///
/// # 为什么是三态而不是两个数字
///
/// 本文件此前只数两个数（盘查次数、移动次数），断言是
/// `inspects + moves == turns`——「卫兵每一回合都有动作，没有落进
/// `Intent::Wait` 兜底分支」。项目所有者随后裁定「玩家优先度高于NPC，
/// 只有玩家可以互换位置」，卫兵（NPC）贴身之后朝目标迈的那一步不再
/// 换位、也挪不动（目的地站着人，`resolve_move` 的占位检查判成一次
/// 失败的移动），于是那条恒等式的左边开始小于 `turns`。
///
/// **修法不是把恒等式放松成不等式**——那等于把这批改动唯一可观测的
/// 后果从测试里抹掉。修法是把新出现的那一态显式数出来：三个数之和
/// 仍然恒等于 `turns`，而「卫兵这一回合到底干了什么」的分辨率比改动
/// 之前更高，不是更低。
#[derive(Debug, Clone, Copy)]
struct GuardTally {
    /// 产出了 `Effect::Inspect` 的回合数。
    inspects: usize,
    /// 产出了 `Effect::MoveTo` 的回合数——卫兵真的挪了一格。
    moves: usize,
    /// 行为树要求走一步、但那一步没挪成的回合数（目的地站着目标本
    /// 人）。本批次之前这一态**不存在**：那时候同一步会被路由成互换
    /// 位置，稳稳产出一条 `SwapPositions`。
    blocked: usize,
    /// 产出了 `Effect::SwapPositions` 的回合数。**恒应为 0**——卫兵是
    /// NPC，而项目所有者裁定「只有玩家可以互换位置」。
    ///
    /// 单独数它、而不是让它悄悄并进 [`Self::blocked`]，是 ADR 0018 反例
    /// 验证抓出来的一处不足：`blocked` 是 `走一步的意图数 − MoveTo 数`
    /// 算出来的，一次互换既不产 `MoveTo` 也不消耗那个意图，会被**误记
    /// 成一次「撞住了」**。于是「把互换那一支重新对 NPC 打开」这个改坏
    /// 方式不会让任何断言变红。这一个字段就是那条反例的钉子。
    swaps: usize,
    /// 既不是盘查也不是移动的意图数（`Intent::Wait` 兜底分支等）。
    /// 恒应为 0——它一旦非零，说明卫兵已经看不见目标了，而本文件全部
    /// 用例的前提都是「卫兵照样看得见」。
    other_intents: usize,
}

impl GuardTally {
    /// 三态之和——应当恒等于跑过的回合数。
    fn accounted(&self) -> usize {
        self.inspects + self.moves + self.blocked
    }
}

fn guard_turns(handle: &RealModsHandle, target_stealthed: bool, turns: usize) -> GuardTally {
    guard_turns_with_profession(handle, handle.guard_id, target_stealthed, turns)
}

/// [`guard_turns`] 的一般形式：把卫兵的职业索引也开放成参数，供反例
/// 用例传一个**不是** `lostland:guard` 的职业进来。
fn guard_turns_with_profession(
    handle: &RealModsHandle,
    guard_profession: ContentIndex,
    target_stealthed: bool,
    turns: usize,
) -> GuardTally {
    let (mut world, terrain_ids) = test_world();
    // 卫兵起点 (5,5)、目标 (8,5)，两人可能走到的范围全部铺草地。
    for x in 0..16 {
        for y in 0..12 {
            world
                .terrain
                .set_terrain(world.size.wrap(x, y), terrain_ids.grass);
        }
    }
    let guard = spawn_agent(
        &mut world,
        placeholder_race(),
        guard_profession,
        (5, 5),
        Agent::STARTING_HEALTH,
    );
    let target = spawn_agent(
        &mut world,
        placeholder_race(),
        placeholder_profession(),
        (8, 5),
        Agent::STARTING_HEALTH,
    );
    world.actors.get_mut(target).expect("刚生成").stealthed = target_stealthed;

    // 树认的职业**恒是** `lostland:guard`（从真实注册表里查出来），
    // 与 `guard_profession` 参数无关——后者只决定「这个实体的职业是
    // 什么」。反例用例正是靠这个差别成立：给实体一个别的职业，树的
    // 职业判定就不成立。
    let mut source = NativeBehaviorSource::new(
        NativeBehaviorTree::guard(&handle.registry),
        // 真实装载出来的四张表的快照——盘查概率会向它们查询目标的
        // 「盘查意愿」。本文件的目标都是占位种族/占位职业，一条
        // `InspectionSuspicion` 都没有，因此本文件的既有断言逐条不受
        // 影响（那正是「没有这条被动的人行为完全不变」这句话的可执行
        // 形式）。
        BehaviorRuleCatalogs::snapshot(
            &handle.race,
            &handle.class,
            // 副职那一路：本文件的实体 `subclasses` 恒为空，一张空表与
            // 真实副职表在这里逐位等价。
            &SubclassTable::new(),
            &handle.trait_def,
            &handle.item,
        ),
        1,
    );

    let mut timeline = Timeline::new();
    timeline.schedule(guard, Tick(0));
    timeline.schedule(target, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    // 目标当受控实体，每一轮用一次「等待」输入消费掉它自己的回合——
    // 这与 `ll_game::app::Demo::advance` 每帧跑的两步（先
    // `advance_ai` 结算排在玩家之前的非受控实体，再
    // `try_player_turn` 用本帧输入结算玩家）逐字相同。
    let mut wait_input = InputState::new();
    wait_input.press(GameKey::Wait);

    let mut inspects = 0usize;
    let mut moves = 0usize;
    let mut swaps = 0usize;
    let mut move_intents = 0usize;
    let mut other_intents = 0usize;
    let catalogs = ResolveCatalogs::empty();
    for _ in 0..turns {
        {
            let mut decide = behavior_ai_intent(&mut source);
            let mut ai = |world: &WorldState, actor: EntityId, controlled: EntityId| {
                let intent = decide(world, actor, controlled);
                match intent {
                    Intent::Move { .. } => move_intents += 1,
                    Intent::Inspect { .. } => {}
                    _ => other_intents += 1,
                }
                intent
            };
            engine.advance_ai(
                &mut world,
                target,
                &mut ai,
                &catalogs,
                &mut |_, effect| match effect {
                    Effect::Inspect { .. } => inspects += 1,
                    // 只数 `MoveTo`：卫兵**真的挪了一格**。
                    //
                    // 这里此前还数 `SwapPositions`——那时候非敌对撞格
                    // 对 NPC 也路由成互换。项目所有者随后裁定「玩家优先
                    // 度高于NPC，只有玩家可以互换位置」，卫兵（NPC）撞上
                    // 贴身的非敌对目标不再产出任何 `SwapPositions`，那条
                    // 分支在本文件里已经**不可能**被走到；留着它会让读者
                    // 以为还有一条活着的路径。第三态（撞上了、没挪成）由
                    // 下面 `move_intents - moves` 数出来。
                    Effect::MoveTo { .. } => moves += 1,
                    // 见 `GuardTally::swaps` 文档：这一支在裁定「只有
                    // 玩家可以互换位置」之后**应当永不触发**，数它就是
                    // 为了断言它是 0。
                    Effect::SwapPositions { .. } => swaps += 1,
                    _ => {}
                },
            );
        }
        engine.try_player_turn(&mut world, target, &wait_input, &catalogs, &mut |_, _| {});
    }
    GuardTally {
        inspects,
        moves,
        blocked: move_intents - moves,
        swaps,
        other_intents,
    }
}

/// 硬要求四：潜行显著降低卫兵的盘查率——而且**不是**靠让卫兵看不见你，
/// 而且整条链路经由 [`TurnEngine`]（本体二进制驱动世界的唯一路径）。
///
/// 判定系统落地批次把这条链路换成了**对抗判定**：潜行不再把基础概率
/// 从 500‰ 换成 50‰（一个 10× 的乘法档），而是给隐蔽方加
/// `GUARD_INSPECT_STEALTH_MODIFIER = 19` 点（一整颗骰子的跨度）。
/// `3d20` 下两侧属性均为基准时，卫兵赢面从 486‰ 降到 86‰。
/// 400 个回合的期望值因此约 194 次 vs 约 34 次；下面只要求「潜行一侧
/// 严格少于不潜行一侧的一半」，留了极大的安全边际（这是概率断言，不是
/// 单次结果断言，与 `resolve.rs` 里偷袭/暴击频率测试同一条既有纪律）。
///
/// **反例**：见本文件模块文档「反例是什么」一节与
/// [`非卫兵职业的实体经由turnengine一次盘查都不会发起`]。另外，若有人
/// 把 `native_behavior::guard_inspect_chance` 里的潜行分支摘掉（或把
/// `ll_sim::ai_query::is_stealthed` 接线摘掉让它恒返回 `false`），两侧
/// 的盘查次数会落回同一个分布，第一条断言立刻变红。
#[test]
fn 潜行显著降低卫兵盘查率但不让卫兵看不见你() {
    // Arrange
    let handle = load_real_mods();
    let turns = 400;

    // Act
    let visible = guard_turns(&handle, false, turns);
    let stealth = guard_turns(&handle, true, turns);

    // Assert 一：盘查率真的降下来了。
    assert!(
        stealth.inspects * 2 < visible.inspects,
        "潜行应当显著降低盘查率：不潜行 {} 次，潜行 {} 次",
        visible.inspects,
        stealth.inspects
    );

    // Assert 二：盘查真的发生过——这一条钉的是「链路通了」本身。行为树
    // 经由 TurnEngine 驱动之前，`Effect::Inspect` 在整条生产链路上是
    // 零产出的。
    assert!(
        visible.inspects > 0,
        "不潜行时卫兵应当真的发起过盘查，一次都没有说明链路断了"
    );

    // Assert 三：卫兵**照样看得见**潜行中的目标——两侧的三态之和都是
    // 满的（每一回合要么盘查、要么真的挪了一格、要么撞在目标身上没挪
    // 成，没有落进 'wait 兜底分支）。若潜行是靠改 FOV 实现的，潜行那
    // 一侧会全部退化成 `Intent::Wait`，`other_intents` 立刻非零。这正
    // 是本批次核心设计选择的可执行形式，见 `ll_script::api::actor`
    // 模块文档「潜行：为什么是一次判定的减值，不是一次可见性的改写」
    // 一节。
    assert_eq!(
        visible.other_intents, 0,
        "不潜行时卫兵每一回合都应当有动作（盘查或走近）：{visible:?}"
    );
    assert_eq!(
        visible.accounted(),
        turns,
        "三态之和应当恰好是回合数：{visible:?}"
    );
    assert_eq!(
        stealth.other_intents, 0,
        "潜行中的目标照样应当被视野查询找到——潜行不是隐身：{stealth:?}"
    );
    assert_eq!(
        stealth.accounted(),
        turns,
        "三态之和应当恰好是回合数：{stealth:?}"
    );

    // Assert 四：第三态真的出现过——项目所有者裁定「玩家优先度高于
    // NPC，只有玩家可以互换位置」之后，卫兵贴身之后每一次「走近」都
    // 撞在目标身上挪不动。本批次之前同一步被路由成互换位置，
    // `blocked` 恒为 0。见 `GuardTally` 文档。
    assert!(
        visible.blocked > 0 && stealth.blocked > 0,
        "卫兵贴身后每一次走近都该撞在目标身上挪不动：不潜行 {visible:?}，潜行 {stealth:?}"
    );

    // Assert 五：卫兵一次都没有和目标换过位置——见 `GuardTally::swaps`
    // 文档。这条与上一条是一对：`blocked` 说「有一态出现了」，`swaps`
    // 说「出现的不是互换」。
    assert_eq!(visible.swaps, 0, "卫兵是 NPC，不该互换位置：{visible:?}");
    assert_eq!(stealth.swaps, 0, "同上：{stealth:?}");
}

/// 反例：同一段代码、同一棵真实行为树、同一个几何布局，只把卫兵的
/// 职业换成一个不是 `lostland:guard` 的索引——`Effect::Inspect` 必须
/// 一条都不产出，而「看见了、走近了」照旧。
///
/// 这条同时是「`lostland:guard` 真的被注册了」这件事的可执行证明：
/// 职业判定比的是注册表里查出来的索引，正面用例传的
/// `handle.guard_id` 来自 `registry.get("lostland:guard")`（见
/// `load_real_mods`），本例传的是另一个索引，两者唯一的差别就是这一
/// 条内容在不在。
#[test]
fn 非卫兵职业的实体经由turnengine一次盘查都不会发起() {
    // Arrange
    let handle = load_real_mods();
    let turns = 400;

    // Act
    let tally = guard_turns_with_profession(&handle, placeholder_profession(), false, turns);

    // Assert
    assert_eq!(
        tally.inspects, 0,
        "不是卫兵职业的实体不该发起任何盘查：{tally:?}"
    );
    // 它照样看得见目标、照样每一回合都要求走近——盘查分支不成立只是让
    // selector 落到下一条。**但「走近」不等于「挪动了」**：贴身之后
    // 每一次走近都撞在目标身上挪不动（所有者裁定「只有玩家可以互换
    // 位置」）。此前这里写的是 `moves == turns`，那在互换还对 NPC 开着
    // 的年代才成立；现在恒等式落在三态之和上，而 `blocked > 0` 把新
    // 出现的那一态钉住。
    assert_eq!(tally.other_intents, 0, "每一回合都该有动作：{tally:?}");
    assert_eq!(
        tally.accounted(),
        turns,
        "三态之和应当恰好是回合数：{tally:?}"
    );
    assert!(tally.blocked > 0, "贴身之后应当撞得动不了：{tally:?}");
    assert_eq!(tally.swaps, 0, "它是 NPC，不该互换位置：{tally:?}");
}

/// 空技能目录常量——同时充当空技能树目录（`NoSkills` 实现了
/// `SkillTreeCatalog`，见 `ll_sim::skill_overview` 里那条 impl 的
/// 文档：不为对称再造第二个语义相同的空对象）。
const NO_SKILLS: NoSkills = NoSkills;
