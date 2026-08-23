//! 端到端验证：潜行与盗贼被动批次——三条互补的证据链。
//!
//! 1. **潜行状态真的经 [`ll_sim::turn::TurnEngine`] 翻转**（本体二进制
//!    `ll-game` 驱动世界的唯一路径），不是靠测试直接调 `resolve` +
//!    `apply` 自证——与 `turn_engine_catalogs.rs` 立下的那条更高的验收
//!    标准一致，见其模块文档。
//! 2. **潜行真的让偷袭直通**，且用的是真实 `mods/example_mod/gameplay.scm`
//!    注册的天赋（`examplemod:footpad` 种族授予的
//!    `examplemod:predatory_instinct`），同样经由 `TurnEngine`。反例是
//!    同一场景下不潜行的同一个角色：它的幸运是 `0`，掷骰那条路径的
//!    触发率恒为 `0‰`（见 `ll_sim::combat::sneak_attack_chance_permille`），
//!    因此**不可能**靠随机命中——两场景的伤害差只可能来自潜行直通。
//! 3. **潜行真的让卫兵的盘查率下降，且不是靠让卫兵看不见你**——用的是
//!    仓库里真实的 `mods/example_mod/behavior.scm`（不是内联字符串
//!    副本），与 `example_mod_guard_inspection.rs` 同一条 ADR 0018
//!    纪律。这一条同时钉死本批次的核心设计选择：`compute_fov`/
//!    `VisibleSet` 一个字节都没改，潜行中的目标照样被
//!    `nearby-actor-in-view` 找到（卫兵照常朝它走），变的只是
//!    `rng-chance` 那一次判定。
//! 4. **行为树真的经 [`ll_sim::turn::TurnEngine`] 驱动结算**——第 3 条
//!    与第 4 条走的是同一段代码（[`guard_turns`]），因此不是两条独立
//!    证据，而是同一条证据在两个维度上的断言。
//!
//! # 第 3/4 条此前为什么不经 `TurnEngine`，现在为什么可以
//!
//! 此前不行：[`ll_sim::turn::TurnEngine::advance_ai`] 的 `ai_intent`
//! 曾经是一个**函数指针**，而 `ScriptBehaviorSource::decide` 需要
//! `&mut self`，捕获不进函数指针——「行为树经由 `TurnEngine` 生效」
//! 这条路径在类型层面就不存在，本文件当时只能沿用
//! `example_mod_guard_inspection.rs` 的「真实脚本 → `decide` → 真实
//! `Intent`」这条较短的路径，并如实记录了降级理由。
//!
//! 现在可以：`ai_intent` 已经放宽成 `&mut dyn FnMut`，
//! `ll_sim::behavior::behavior_ai_intent` 把任意
//! `BehaviorTreeSource`（这里就是真实的 `ScriptBehaviorSource`）包成
//! 它要的那个闭包。本文件因此升级成走完整链路：
//!
//! ```text
//! 真实 behavior.scm → ScriptBehaviorSource::decide → behavior_ai_intent
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
//! 把 `mods/lostland/classes.scm` 的那条 `register-class` 删掉，或者把
//! `behavior.scm` 里 `self-has-profession?` 那个分支摘掉，正面那条测试
//! 立刻变红。
use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::script_behavior_source::{PreparedBehaviorEngine, ScriptBehaviorSource};
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
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

/// `mods/example_mod/gameplay.scm` 里
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
    let mut registry = Registry::new();
    let mut terrain = ll_world::terrain::TerrainTable::new();
    let mut class = ClassTable::new();
    let mut skill = SkillTable::new();
    let mut subclass = SubclassTable::new();
    let mut quest = QuestTable::new();
    let mut race = RaceTable::new();
    let mut clip = ClipTable::new();
    let mut xp_curve = XpCurveTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut trait_def = TraitTable::new();
    let mut resource_pool = ResourcePoolTable::new();
    let mut item = ItemTable::new();
    let mut formula = FormulaTable::new();
    let mut weapon_category = WeaponCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut damage_category = DamageCategoryTable::new();

    let report = load_all(
        Path::new(REAL_MODS_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut terrain,
            class: &mut class,
            skill: &mut skill,
            subclass: &mut subclass,
            quest: &mut quest,
            race: &mut race,
            clip: &mut clip,
            xp_curve: &mut xp_curve,
            xp_curve_bindings: &mut xp_curve_bindings,
            trait_def: &mut trait_def,
            resource_pool: &mut resource_pool,
            item: &mut item,
            formula: &mut formula,
            weapon_category: &mut weapon_category,
            damage_category: &mut damage_category,
            space_profile: &mut space_profile,
            weather: &mut weather_table,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
        },
    );
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
    // 得出 `behavior.scm` 里写的这个字符串，因此它必须在表里。
    //
    // **`get` 而不是 `intern`**——这一行本身就是一条断言：卫兵职业现在
    // 是 `mods/lostland/classes.scm` 里一条真实注册的本体内容，装载
    // 管线跑完之后它必须已经在注册表里。此前这里写的是 `intern`
    // （「查不到就现造一个」），因为整个仓库里根本没有任何脚本注册过
    // 这个职业——`behavior.scm` 引用它，`guard-try-inspect` 的第一个
    // `if` 却恒为假，卫兵在真实游戏里永远不会盘查。换成 `get` 之后，
    // 那条注册一旦被删掉，本文件的全部卫兵用例会立刻在这里失败并点名
    // 原因，而不是静默退化成「测试自己造了一个，生产里没有」。
    let guard_id = registry
        .get(&NamespacedId::parse("lostland:guard").expect("合法标识符"))
        .expect("lostland:guard 应当已被 mods/lostland/classes.scm 注册");

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
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        script_state: BTreeMap::new(),
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

/// 硬要求二：潜行让真实注册的盗贼系天赋偷袭直通——经由 `TurnEngine`，
/// 内容来自真实 `mods/`。
///
/// 反例内建在同一条测试里：不潜行的那一场用的是**同一个种族、同一份
/// 天赋、同一个幸运值（0）**，掷骰路径的触发率恒为 `0‰`，因此绝不可能
/// 命中；两场的伤害差只可能来自潜行直通。若有人把
/// `resolve_attack` 里那条 `Some(rule) if attacker.stealthed` 守卫分支
/// 摘掉，两场伤害立刻相等，本测试变红。
#[test]
fn 潜行让真实盗贼天赋的偷袭直通并经由turnengine生效() {
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

    let damage_dealt = |stealthed: bool| -> i32 {
        let (mut world, _terrain_ids) = test_world();
        let attacker = spawn_agent(
            &mut world,
            handle.footpad_id,
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
        world.actors.get_mut(attacker).expect("刚生成").stealthed = stealthed;

        let mut timeline = Timeline::new();
        timeline.schedule(attacker, Tick(0));
        timeline.schedule(defender, Tick(1));
        let mut engine = TurnEngine::new(timeline);
        let acted = engine.advance_ai(
            &mut world,
            defender,
            &mut attack_controlled,
            &catalogs,
            &mut |_, _| {},
        );
        assert_eq!(acted, vec![attacker], "本场景应当恰好结算攻击方一次行动");

        DEFENDER_HEALTH
            - world
                .actors
                .get(defender)
                .expect("防御方生命远高于单次伤害，不应死亡")
                .health
    };

    // Act
    let visible_damage = damage_dealt(false);
    let stealth_damage = damage_dealt(true);

    // Assert：精确多出真实脚本声明的那个数，不多不少。
    assert_eq!(
        stealth_damage,
        visible_damage + PREDATORY_INSTINCT_EXTRA_DAMAGE,
        "潜行中的 footpad 应当精确多打出 gameplay.scm 声明的 {PREDATORY_INSTINCT_EXTRA_DAMAGE} 点偷袭伤害"
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

/// 仓库根目录下真实的 `mods/example_mod/behavior.scm`，理由同
/// `example_mod_guard_inspection.rs::load_guard_behavior_source`。
fn load_guard_behavior_source() -> String {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mods/example_mod/behavior.scm"
    ));
    std::fs::read_to_string(path).expect("仓库里应当存在真实的 behavior.scm")
}

/// 摆一个「卫兵盯着一个目标」的场景，用真实的 `guard-ai-tree` 经由
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
/// 卫兵每个回合恰好产出一条这两种效果之一：`guard-ai-tree` 只有两个
/// 分支（盘查 / 走近），兜底的 `'wait` 只在 `nearby-actor-in-view`
/// 找不到目标时才会命中。调用方据此断言「卫兵每一回合都有动作」——
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
fn guard_turns(
    prepared: PreparedBehaviorEngine,
    handle: &RealModsHandle,
    target_stealthed: bool,
    turns: usize,
) -> (usize, usize) {
    guard_turns_with_profession(prepared, handle, handle.guard_id, target_stealthed, turns)
}

/// [`guard_turns`] 的一般形式：把卫兵的职业索引也开放成参数，供反例
/// 用例传一个**不是** `lostland:guard` 的职业进来。
/// `prepared` 是调用方在**装载真实 mods 之前**就造好的空引擎——
/// 装载会编译一批脚本，之后这根线程上就不许再构造引擎了（见
/// `ll_script::host` 里 `COMPILED_ON_THIS_THREAD` 上方注释与 ADR 0028）。
fn guard_turns_with_profession(
    prepared: PreparedBehaviorEngine,
    handle: &RealModsHandle,
    guard_profession: ContentIndex,
    target_stealthed: bool,
    turns: usize,
) -> (usize, usize) {
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

    let source_code = load_guard_behavior_source();
    let mut source = ScriptBehaviorSource::from_prepared(
        prepared,
        &source_code,
        "guard-ai-tree",
        "examplemod",
        &handle.registry,
        1,
    )
    .expect("真实 behavior.scm 应当能通过白名单并装载成功");

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
    let catalogs = ResolveCatalogs::empty();
    for _ in 0..turns {
        {
            let mut ai = behavior_ai_intent(&mut source);
            engine.advance_ai(
                &mut world,
                target,
                &mut ai,
                &catalogs,
                &mut |_, effect| match effect {
                    Effect::Inspect { .. } => inspects += 1,
                    Effect::MoveTo { .. } => moves += 1,
                    _ => {}
                },
            );
        }
        engine.try_player_turn(&mut world, target, &wait_input, &catalogs, &mut |_, _| {});
    }
    (inspects, moves)
}

/// 硬要求四：潜行显著降低卫兵的盘查率——而且**不是**靠让卫兵看不见你，
/// 而且整条链路经由 [`TurnEngine`]（本体二进制驱动世界的唯一路径）。
///
/// `behavior.scm` 里两个千分比是 500（不潜行）与 50（潜行），相差十倍。
/// 400 个回合的期望值因此约 200 次 vs 约 20 次；下面只要求「潜行一侧
/// 严格少于不潜行一侧的一半」，留了极大的安全边际（这是概率断言，不是
/// 单次结果断言，与 `resolve.rs` 里偷袭/暴击频率测试同一条既有纪律）。
///
/// **反例**：见本文件模块文档「反例是什么」一节与
/// [`非卫兵职业的实体经由turnengine一次盘查都不会发起`]。另外，若有人
/// 把 `guard-inspect-chance` 那个分支从 `behavior.scm` 摘掉（或把
/// `actor-stealthed?` 接线摘掉让它恒返回 `#f`），两侧的盘查次数会落回
/// 同一个分布，第一条断言立刻变红。
#[test]
fn 潜行显著降低卫兵盘查率但不让卫兵看不见你() {
    // Arrange：两个行为树引擎都要在 load_real_mods（会编译一批脚本）
    // 之前造好——同一根线程上全部构造必须先于全部编译，见
    // `ll_script::host` 里 `COMPILED_ON_THIS_THREAD` 上方注释。
    let visible_engine = PreparedBehaviorEngine::new();
    let stealth_engine = PreparedBehaviorEngine::new();
    let handle = load_real_mods();
    let turns = 400;

    // Act
    let (visible_inspects, visible_moves) = guard_turns(visible_engine, &handle, false, turns);
    let (stealth_inspects, stealth_moves) = guard_turns(stealth_engine, &handle, true, turns);

    // Assert 一：盘查率真的降下来了。
    assert!(
        stealth_inspects * 2 < visible_inspects,
        "潜行应当显著降低盘查率：不潜行 {visible_inspects} 次，潜行 {stealth_inspects} 次"
    );

    // Assert 二：盘查真的发生过——这一条钉的是「链路通了」本身。行为树
    // 经由 TurnEngine 驱动之前，`Effect::Inspect` 在整条生产链路上是
    // 零产出的。
    assert!(
        visible_inspects > 0,
        "不潜行时卫兵应当真的发起过盘查，一次都没有说明链路断了"
    );

    // Assert 三：卫兵**照样看得见**潜行中的目标——两侧的「行动总数」
    // 都是满的（每一回合要么盘查要么走近，没有落进 'wait 兜底分支）。
    // 若潜行是靠改 FOV 实现的，潜行那一侧会全部退化成 `Intent::Wait`，
    // 这条断言立刻变红。这正是本批次核心设计选择的可执行形式，见
    // `ll_script::api::actor` 模块文档「潜行：为什么是一次判定的减值，
    // 不是一次可见性的改写」一节。
    assert_eq!(
        visible_inspects + visible_moves,
        turns,
        "不潜行时卫兵每一回合都应当有动作（盘查或走近）"
    );
    assert_eq!(
        stealth_inspects + stealth_moves,
        turns,
        "潜行中的目标照样应当被 nearby-actor-in-view 找到——潜行不是隐身"
    );
}

/// 反例：同一段代码、同一棵真实行为树、同一个几何布局，只把卫兵的
/// 职业换成一个不是 `lostland:guard` 的索引——`Effect::Inspect` 必须
/// 一条都不产出，而「看见了、走近了」照旧。
///
/// 这条同时是「`lostland:guard` 真的被注册了」这件事的可执行证明：
/// `self-has-profession?` 比的是注册表快照里的字符串，正面用例传的
/// `handle.guard_id` 来自 `registry.get("lostland:guard")`（见
/// `load_real_mods`），本例传的是另一个索引，两者唯一的差别就是这一
/// 条内容在不在。
#[test]
fn 非卫兵职业的实体经由turnengine一次盘查都不会发起() {
    // Arrange：行为树引擎在 load_real_mods 之前造好，理由同上一条用例。
    let prepared = PreparedBehaviorEngine::new();
    let handle = load_real_mods();
    let turns = 400;

    // Act
    let (inspects, moves) =
        guard_turns_with_profession(prepared, &handle, placeholder_profession(), false, turns);

    // Assert
    assert_eq!(
        inspects, 0,
        "不是卫兵职业的实体不该发起任何盘查，实际 {inspects} 次"
    );
    assert_eq!(
        moves, turns,
        "它照样看得见目标、照样走近——盘查分支不成立只是让 selector 落到下一条"
    );
}

/// 空技能目录常量——同时充当空技能树目录（`NoSkills` 实现了
/// `SkillTreeCatalog`，见 `ll_sim::skill_overview` 里那条 impl 的
/// 文档：不为对称再造第二个语义相同的空对象）。
const NO_SKILLS: NoSkills = NoSkills;
