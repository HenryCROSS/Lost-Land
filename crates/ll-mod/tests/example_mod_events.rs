//! 端到端验证：mod 脚本订阅的运行期事件，真的经
//! [`ll_sim::turn::TurnEngine`] 这条生产路径被回调，且它产出的写入
//! 真的经 `apply` 落进 `WorldState`。
//!
//! # 这份测试是 ADR 0018 要求的那一半证据
//!
//! ADR 0018 的判据是「玩法层能力必须有真实 mod 脚本为证 + 经真实结算
//! 路径的端到端测试 + 反例」。本文件三样都给：
//!
//! - **真实脚本**：`mods/example_mod/gameplay.scm` 里两条
//!   `(on-event ...)` 声明 + `mods/example_mod/events.scm` 里两个处理
//!   函数，都是已发货内容，不是测试内联的字符串。
//! - **真实路径**：`TurnEngine::advance_ai` → `perform` → `resolve`
//!   → `on_effect`（= 事件分发）→ `apply`。一次都不直接调用
//!   `ScriptEventSource::dispatch` 之外的捷径。
//! - **反例**：`摘掉事件分发接线后脚本写入不再落地` 一条——把
//!   `on_effect` 换回一个恒返回空 `Vec` 的闭包（那正是接线之前的样子），
//!   断言立刻不成立。
//!
//! # 约束 C6：为什么引擎构造全都排在装载之前
//!
//! `PreparedEventEngine::new()` 是一次 `ScriptEngine` 构造，而
//! `load_all` 会在本线程上编译一堆脚本。「同一根线程上全部构造先于
//! 全部编译」（ADR 0028）因此要求本文件的每条测试都先把事件引擎造好、
//! 再装载 mod——顺序反了会当场撞上 `ScriptEngine::new` 的断言。

use std::path::Path;

use ll_core::ident::NamespacedId;
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::event::{EventSubscriptionTable, GameEventKind};
use ll_mod::formula::FormulaTable;
use ll_mod::item::ItemTable;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::recipe::RecipeTable;
use ll_mod::recipe_category::RecipeCategoryTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::script_event_source::{PreparedEventEngine, ScriptEventSource};
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::tag::TagTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::intent::Intent;
use ll_sim::turn::TurnEngine;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::script_state::ScriptValue;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");
const EVENT_SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../mods/example_mod/events.scm"
);

/// 装载真实 `mods/` 目录，返回订阅表。
fn load_subscriptions() -> EventSubscriptionTable {
    let mut registry = Registry::new();
    let mut terrain = base_terrain_fixture().1;
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
    let mut damage_category = DamageCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather = ll_world::weather::WeatherTable::new();
    let mut recipe = RecipeTable::new();
    let mut recipe_category = RecipeCategoryTable::new();
    let mut tag = TagTable::new();
    let mut events = EventSubscriptionTable::new();

    load_all(
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
            weather: &mut weather,
            recipe: &mut recipe,
            recipe_category: &mut recipe_category,
            tag: &mut tag,
            events: &mut events,
        },
    );
    events
}

/// 读真实的 `mods/example_mod/events.scm`——不内联一份拷贝：本文件要
/// 证明的正是**已发货的那份脚本**能跑（ADR 0018），内联一份等于换成
/// 证明「测试自己写的脚本能跑」。
fn event_sources() -> Vec<(String, String)> {
    let source = std::fs::read_to_string(EVENT_SCRIPT).expect("真实 events.scm 必须可读");
    vec![("examplemod".to_string(), source)]
}

fn test_world() -> WorldState {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
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

fn spawn_at(world: &mut WorldState, x: i32, y: i32, health: i32) -> EntityId {
    let pos = world.size.wrap(x, y);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    let profession = ll_core::ident::ContentIndex::default();
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession,
        goals: Vec::new(),
        race: profession,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(zone, ll_core::ident::ContentIndex::default()),
        script_state: std::collections::BTreeMap::new(),
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

/// 读某个实体上 `examplemod` 命名空间下的一个键。
fn entity_state(world: &WorldState, entity: EntityId, key: &str) -> Option<ScriptValue> {
    world
        .actors
        .get(entity)?
        .script_state
        .get(&("examplemod".to_string(), key.to_string()))
        .cloned()
}

fn global_state(world: &WorldState, key: &str) -> Option<ScriptValue> {
    world
        .global_script_state
        .get(&("examplemod".to_string(), key.to_string()))
        .cloned()
}

#[test]
fn 真实mod脚本的事件订阅被装载管线收下() {
    // Arrange & Act
    let subscriptions = load_subscriptions();

    // Assert：两条订阅都来自 mods/example_mod/gameplay.scm。
    assert!(subscriptions.has_subscriber(GameEventKind::Killed));
    assert!(subscriptions.has_subscriber(GameEventKind::ExperienceGained));
    assert_eq!(
        subscriptions.all().len(),
        2,
        "订阅表不得凭空多出订阅，也不得漏掉 gameplay.scm 里的任何一条"
    );
    let namespaces: Vec<&str> = subscriptions
        .all()
        .iter()
        .map(|s| s.mod_namespace.as_str())
        .collect();
    assert!(
        namespaces.iter().all(|ns| *ns == "examplemod"),
        "订阅方命名空间由宿主固化，只能是声明它的那个 mod"
    );
}

#[test]
fn 击杀经turnengine结算时经验监听器产出的写入真的落进世界() {
    // 这是本文件的核心验收：真实脚本 + 真实结算路径 + apply 落地。
    // 走的是 experience-gained 那一路——击杀会同时产出 `Effect::Kill`
    // 与 `Effect::GrantExperience`，后者落在**击杀者**身上，击杀者在
    // 反应效果落地那一刻还活着，因此 entity 写入落得下去。
    // Arrange：**先造引擎再装载**（约束 C6，见模块文档）。
    let prepared = vec![PreparedEventEngine::new()];
    let subscriptions = load_subscriptions();
    let mut dispatcher = ScriptEventSource::new(prepared, &event_sources(), subscriptions)
        .expect("真实 events.scm 必须能建起事件分发");

    let mut world = test_world();
    let attacker = spawn_at(&mut world, 0, 0, Agent::STARTING_HEALTH);
    let victim = spawn_at(&mut world, 1, 0, 1);

    let mut timeline = ll_sim::timeline::Timeline::new();
    timeline.schedule(attacker, Tick(0));
    let mut engine = TurnEngine::new(timeline);
    let catalogs = ResolveCatalogs::empty();

    // Act：经生产路径结算一次致命攻击。
    let mut ai = |_: &WorldState, actor: EntityId, _: EntityId| Intent::Attack {
        actor,
        target: victim,
    };
    engine.advance_ai(
        &mut world,
        victim,
        &mut ai,
        &catalogs,
        &mut |world, effect| dispatcher.dispatch(world, effect),
    );

    // Assert：经验事件的两条写入都落地了。
    assert!(
        matches!(global_state(&world, "last-experience"), Some(ScriptValue::Int(n)) if n > 0),
        "全局 last-experience 必须被 examplemod-on-experience 写入一个正数，实际是 {:?}",
        global_state(&world, "last-experience")
    );
    assert!(
        matches!(
            entity_state(&world, attacker, "last-experience-gained"),
            Some(ScriptValue::Int(n)) if n > 0
        ),
        "击杀者身上必须留下这次获得的经验量"
    );
}

#[test]
fn 摘掉事件分发接线后脚本写入不再落地() {
    // ADR 0018 要求的反例：把 `on_effect` 换回接线之前那个恒返回空
    // `Vec` 的闭包，同一段结算不再产生任何脚本状态——证明上面那条
    // 测试的绿是真的来自这条接线，不是来自别的什么东西。
    // Arrange
    let mut world = test_world();
    let attacker = spawn_at(&mut world, 0, 0, Agent::STARTING_HEALTH);
    let victim = spawn_at(&mut world, 1, 0, 1);
    let mut timeline = ll_sim::timeline::Timeline::new();
    timeline.schedule(attacker, Tick(0));
    let mut engine = TurnEngine::new(timeline);
    let catalogs = ResolveCatalogs::empty();
    let mut ai = |_: &WorldState, actor: EntityId, _: EntityId| Intent::Attack {
        actor,
        target: victim,
    };

    // Act：注意这里**没有** dispatcher。
    engine.advance_ai(&mut world, victim, &mut ai, &catalogs, &mut |_, _| {
        Vec::new()
    });

    // Assert
    assert_eq!(global_state(&world, "last-experience"), None);
    assert_eq!(global_state(&world, "last-event"), None);
    assert_eq!(
        entity_state(&world, attacker, "last-experience-gained"),
        None
    );
}

#[test]
fn 订阅点名一个不存在的处理函数时建分发器当场失败() {
    // ADR 0017「注册期完整校验」在事件订阅上的落点：不留到结算期变成
    // 一条永远静默失败的回调。
    // Arrange
    let prepared = vec![PreparedEventEngine::new()];
    let mut subscriptions = EventSubscriptionTable::new();
    subscriptions
        .subscribe(ll_mod::event::EventSubscription {
            mod_namespace: "examplemod".to_string(),
            kind: GameEventKind::Killed,
            handler: "examplemod-this-name-does-not-exist".to_string(),
        })
        .expect("登记订阅本身应当成功");

    // Act
    let result = ScriptEventSource::new(prepared, &event_sources(), subscriptions);

    // Assert
    let Err(error) = result else {
        panic!("指向不存在函数的订阅必须当场失败");
    };
    let text = error.to_string();
    assert!(text.contains("examplemod-this-name-does-not-exist"));
    assert!(text.contains("examplemod"));
}

#[test]
fn 没有任何订阅时分发器对任何效果都不回调脚本() {
    // 「没人订阅就一分钱都不花」那条性能承诺的可执行守卫。
    // Arrange
    let prepared = vec![PreparedEventEngine::new()];
    let mut dispatcher =
        ScriptEventSource::new(prepared, &event_sources(), EventSubscriptionTable::new())
            .expect("空订阅表必须能建起分发器（它不会建任何引擎）");
    let mut world = test_world();
    let victim = spawn_at(&mut world, 0, 0, Agent::STARTING_HEALTH);

    // Act
    let reactions = dispatcher.dispatch(
        &world,
        &ll_sim::effect::Effect::GrantExperience {
            target: victim,
            amount: 5,
        },
    );

    // Assert
    assert!(reactions.is_empty());
}

#[test]
fn 一个mod的处理函数写不进别的mod的命名空间() {
    // 订阅方命名空间由宿主固化（脚本参数里没有它），处理函数返回的
    // 写入因此只能落在它自己的命名空间下。
    // Arrange
    let prepared = vec![PreparedEventEngine::new()];
    let subscriptions = load_subscriptions();
    let mut dispatcher = ScriptEventSource::new(prepared, &event_sources(), subscriptions)
        .expect("必须能建起事件分发");
    let mut world = test_world();
    let victim = spawn_at(&mut world, 0, 0, Agent::STARTING_HEALTH);

    // Act
    let reactions = dispatcher.dispatch(
        &world,
        &ll_sim::effect::Effect::GrantExperience {
            target: victim,
            amount: 5,
        },
    );

    // Assert
    let ll_sim::effect::Effect::SetScriptState { writes } =
        reactions.first().expect("必须产出一条 SetScriptState")
    else {
        panic!("反应效果必须是 SetScriptState");
    };
    assert!(!writes.is_empty());
    for write in writes {
        assert_eq!(
            write.mod_namespace, "examplemod",
            "写入的命名空间必须恒等于订阅方自己"
        );
    }
    // 落地之后也确认一次：别的命名空间下什么都没有。
    for effect in &reactions {
        ll_sim::apply::apply(&mut world, effect);
    }
    assert!(
        world
            .global_script_state
            .keys()
            .all(|(namespace, _)| namespace == "examplemod")
    );
    let _ = NamespacedId::parse("examplemod:self").expect("合法标识符");
}

#[test]
fn 击杀事件的写入落在击杀者身上并记下全局最后一次事件() {
    // 与上一条互补：这一条走的是 killed 那一路，且刻意让目标一击致命，
    // 验证 examplemod-on-kill 写的是**击杀者**（死者此刻已被销毁，见
    // events.scm 里那段说明）。
    // Arrange：**先造引擎再装载**（约束 C6）。
    let prepared = vec![PreparedEventEngine::new()];
    let subscriptions = load_subscriptions();
    let mut dispatcher = ScriptEventSource::new(prepared, &event_sources(), subscriptions)
        .expect("真实 events.scm 必须能建起事件分发");

    let mut world = test_world();
    let attacker = spawn_at(&mut world, 0, 0, Agent::STARTING_HEALTH);
    let victim = spawn_at(&mut world, 1, 0, 1);

    let mut timeline = ll_sim::timeline::Timeline::new();
    timeline.schedule(attacker, Tick(0));
    let mut engine = TurnEngine::new(timeline);
    let catalogs = ResolveCatalogs::empty();
    let mut ai = |_: &WorldState, actor: EntityId, _: EntityId| Intent::Attack {
        actor,
        target: victim,
    };

    // Act
    engine.advance_ai(
        &mut world,
        victim,
        &mut ai,
        &catalogs,
        &mut |world, effect| dispatcher.dispatch(world, effect),
    );

    // Assert
    assert!(world.actors.get(victim).is_none(), "目标应当已被击杀");
    assert_eq!(
        global_state(&world, "last-event"),
        Some(ScriptValue::Str("killed".to_string().into_boxed_str())),
        "全局 last-event 必须记下事件种类字符串"
    );
    assert_eq!(
        entity_state(&world, attacker, "last-kill-seen"),
        Some(ScriptValue::Bool(true)),
        "击杀者身上必须留下标记"
    );
}
