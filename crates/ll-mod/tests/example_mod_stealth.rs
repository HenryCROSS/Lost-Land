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
//!
//! # 第 3 条为什么不经 `TurnEngine`
//!
//! 如实记录，不是遗漏：**`ScriptBehaviorSource` 目前根本没有接进
//! `ll-game`**——[`ll_sim::turn::TurnEngine::advance_ai`] 的 `ai_intent`
//! 是一个**函数指针**（见其文档：「当前两个调用方都不需要按调用方状态
//! 捕获环境」），而 `ScriptBehaviorSource::decide` 需要 `&mut self`，
//! 捕获不进函数指针；`ll-game` 那侧传的也确实是一个恒 `Wait` 的占位
//! 策略（`crates/ll-game/src/app.rs`）。因此「行为树经由 `TurnEngine`
//! 生效」这条路径在本批次开始之前就不存在，本批次也不打算顺手接它
//! （那是一次独立的、会牵动 `advance_ai` 签名的改动）。第 3 条因此沿用
//! `example_mod_guard_inspection.rs` 既有的最高保真路径：真实脚本
//! → `decide` → 真实 `Intent`。

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
use ll_mod::script_behavior_source::ScriptBehaviorSource;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::behavior::BehaviorTreeSource;
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::quest::NoQuests;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
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
            // 本文件的场景都在一个占位层属性的地表上，温度这一路没有
            // 可查的表，理由同 `turn_engine_catalogs.rs` 同一位置。
            ambient: AmbientSource::NONE,
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
    // **如实记录一个既有缺口**：`mods/` 下**没有任何脚本注册过这个
    // 职业**——`behavior.scm` 引用它，但 `register-class "lostland:guard"`
    // 在整个仓库里不存在（已 grep 核实）。既有的
    // `example_mod_guard_inspection.rs` 用的也是「测试自己 intern 一个」
    // 这条同样的绕法，本文件沿用它而不是顺手补一条注册：补注册属于
    // 本体内容批次，会牵动内容哈希与 i18n 显示名，不该夹带进本批次。
    // `Registry::intern` 对已存在的 id 返回既有索引，因此这一行在那条
    // 注册补上之后也依然正确。
    let guard_id = registry.intern(NamespacedId::parse("lostland:guard").expect("合法标识符"));

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
        toggle_stealth,
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
            attack_controlled,
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
        attack_controlled,
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

/// 摆一个「卫兵盯着一个目标」的场景，让真实的 `guard-ai-tree` 连续
/// 决策 `ticks` 次，返回这段窗口里卫兵各产出了多少次盘查、多少次
/// 「看见了但没盘查」（朝目标移动）。
///
/// 两个计数都要：只数盘查次数无法区分「潜行降低了判定成功率」与
/// 「潜行让卫兵干脆看不见了」——后者会让移动次数也一起归零，那正是
/// 本批次刻意**没有**采用的那条设计。
fn guard_decisions(handle: &RealModsHandle, target_stealthed: bool, ticks: i64) -> (usize, usize) {
    let (mut world, _terrain_ids) = test_world();
    let guard = spawn_agent(
        &mut world,
        placeholder_race(),
        handle.guard_id,
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
    let mut source = ScriptBehaviorSource::new(
        &source_code,
        "guard-ai-tree",
        "examplemod",
        &handle.registry,
        1,
    )
    .expect("真实 behavior.scm 应当能通过白名单并装载成功");

    let mut inspects = 0usize;
    let mut approaches = 0usize;
    for _ in 0..ticks {
        match source.decide(&world, guard) {
            Some(Intent::Inspect { .. }) => inspects += 1,
            Some(Intent::Move { .. }) => approaches += 1,
            _ => {}
        }
        // 只推进世界时钟、不真的移动卫兵——`DetRng::for_entity` 的事件
        // 计数取世界时钟（见 `ScriptBehaviorSource::decide` 文档），推进
        // 时钟才能让每一次决策抽到不同的随机数；不移动是为了让两次
        // 场景的几何完全相同（理由同 `example_mod_guard_inspection.rs`
        // 的 `decide_over_ticks`）。
        world.advance(1);
    }
    (inspects, approaches)
}

/// 硬要求四：潜行显著降低卫兵的盘查率——而且**不是**靠让卫兵看不见你。
///
/// `behavior.scm` 里两个千分比是 500（不潜行）与 50（潜行），相差十倍。
/// 400 次决策的期望值因此约 200 次 vs 约 20 次；下面只要求「潜行一侧
/// 严格少于不潜行一侧的一半」，留了极大的安全边际（这是概率断言，不是
/// 单次结果断言，与 `resolve.rs` 里偷袭/暴击频率测试同一条既有纪律）。
///
/// **反例在同一条测试里**：若有人把 `guard-inspect-chance` 那个分支从
/// `behavior.scm` 摘掉（或把 `actor-stealthed?` 接线摘掉让它恒返回
/// `#f`），两侧的盘查次数会落回同一个分布，第一条断言立刻变红。
#[test]
fn 潜行显著降低卫兵盘查率但不让卫兵看不见你() {
    // Arrange
    let handle = load_real_mods();
    let ticks = 400;

    // Act
    let (visible_inspects, visible_approaches) = guard_decisions(&handle, false, ticks);
    let (stealth_inspects, stealth_approaches) = guard_decisions(&handle, true, ticks);

    // Assert 一：盘查率真的降下来了。
    assert!(
        stealth_inspects * 2 < visible_inspects,
        "潜行应当显著降低盘查率：不潜行 {visible_inspects} 次，潜行 {stealth_inspects} 次"
    );

    // Assert 二：卫兵**照样看得见**潜行中的目标——两侧的「决策总数」
    // 都是满的（每一次决策要么盘查要么走近，没有落进 'wait 兜底分支）。
    // 若潜行是靠改 FOV 实现的，潜行那一侧会全部退化成 `Intent::Wait`，
    // 这条断言立刻变红。这正是本批次核心设计选择的可执行形式，见
    // `ll_script::api::actor` 模块文档「潜行：为什么是一次判定的减值，
    // 不是一次可见性的改写」一节。
    assert_eq!(
        visible_inspects + visible_approaches,
        ticks as usize,
        "不潜行时卫兵每一回合都应当有动作（盘查或走近）"
    );
    assert_eq!(
        stealth_inspects + stealth_approaches,
        ticks as usize,
        "潜行中的目标照样应当被 nearby-actor-in-view 找到——潜行不是隐身"
    );
}
