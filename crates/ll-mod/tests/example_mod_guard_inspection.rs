//! 端到端验证：卫兵职业接线批次——证明卫兵职业的行为树脚本
//! （真实的 `mods/example_mod/behavior.scm`，不是内联字符串副本）能够
//! 真正驱动 AI 决策发起一次盘查（`Intent::Inspect` → `Effect::Inspect`
//! → `resolve`），且视野判定走的是真正的 FOV（隔墙看不见），概率判定
//! 与其余随机性一样确定性可重放。
//!
//! ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本为
//! 证」——本文件直接从磁盘读取仓库里真实的 `mods/example_mod/behavior.scm`
//! （见 [`load_guard_behavior_source`]），不能靠
//! `crates/ll-mod/src/script_behavior_api.rs`/
//! `crates/ll-script/src/api/actor.rs` 的单元测试自证，与
//! `example_mod_sneak_attack.rs` 同一条既有纪律。
//!
//! # 本文件不做什么
//!
//! 只验证「盘查真的发生、看到了什么」这一条链路——`Owner`/
//! `stolen_marker` 尚未落地（`knowledge/design/ownership-and-crime-detection.md`
//! 仍是纯设计），本文件因此不断言、也不能断言"这次盘查算不算违法"，
//! `Effect::Inspect` 本身也不产出任何这方面的判断，见其文档。

use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::registry::Registry;
use ll_mod::script_behavior_source::{PreparedBehaviorEngine, ScriptBehaviorSource};
use ll_sim::behavior::BehaviorTreeSource;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::resolve::resolve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/example_mod/behavior.scm`——理由见模块
/// 文档，与 `example_mod_sneak_attack.rs::REAL_MODS_ROOT` 同一条既有
/// 纪律，只是这里只需要单独这一个文件，不需要走完整的 `load_all`
/// 装载管线（行为树脚本本就不经过那条管线，见 `behavior.scm` 文件头
/// 注释「本文件不在 mod.json5 的 entry_points 里」一节）。
fn load_guard_behavior_source() -> String {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mods/example_mod/behavior.scm"
    ));
    std::fs::read_to_string(path).expect("仓库里应当存在真实的 behavior.scm")
}

/// 循环跑到 `max_ticks` 次决策为止（每次把 `world.clock` 推进一格,
/// 让 `DetRng::for_entity` 的事件计数跟着变化,从而尝试到不同的随机数）
/// ，返回沿途每一次 `decide` 的结果——供多条测试各自扫描。
fn decide_over_ticks(
    source: &mut ScriptBehaviorSource,
    world: &mut WorldState,
    actor: EntityId,
    max_ticks: i64,
) -> Vec<Option<Intent>> {
    let mut results = Vec::new();
    for _ in 0..max_ticks {
        results.push(source.decide(world, actor));
        world.advance(1);
    }
    results
}

/// 造一间 1x1 分区的空白世界，返回世界与本体地形索引——供三条测试
/// 各自摆放墙体/生成实体。
fn test_world() -> (WorldState, BaseTerrainIds) {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
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

/// 生成一个实体，`inventory`/`equipment` 由调用方给定——盘查测试需要
/// 目标身上真的带着物品，才能断言 `Effect::Inspect::items_seen` 确实
/// "看到了什么"。
#[allow(clippy::too_many_arguments)]
fn spawn_agent(
    world: &mut WorldState,
    x: i32,
    y: i32,
    profession: ContentIndex,
    race: ContentIndex,
    inventory: Vec<ItemStack>,
    equipment: std::collections::BTreeMap<EquipSlot, ItemStack>,
) -> EntityId {
    let pos = world.size.wrap(x, y);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
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
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory,
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
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

/// 装载 `guard-ai-tree` 需要的注册表——`registry` 必须先 intern
/// `"lostland:guard"`（`self-has-profession?` 靠这份快照才认得出
/// 这个字符串），返回可以直接赋给 `Agent.profession` 的索引。
///
/// # 为什么这里仍然是 `intern` 而不是 `get`
///
/// 卫兵职业本身现在是一条真实注册的本体内容
/// （`mods/lostland/classes.json5`），但**本文件不装载 `mods/`**：它自建
/// 一个空 `Registry`，只从磁盘读真实的 `behavior.scm`（ADR 0018 要的是
/// 「行为树脚本是真的」，不是「整条装载管线跑过」）。在一个没跑过装载
/// 管线的注册表里 `get` 必然落空，`intern` 才是这里正确的写法。
///
/// 「卫兵职业真的被本体内容注册过」这条断言有自己的落点：
/// `example_mod_stealth.rs` 的 `load_real_mods` 跑完整条 `load_all`
/// 之后用 `registry.get("lostland:guard")`，注册一旦被删就立刻变红。
fn intern_guard_profession(registry: &mut Registry) -> ContentIndex {
    registry.intern(NamespacedId::parse("lostland:guard").expect("合法标识符"))
}

fn human_race() -> ContentIndex {
    let mut interner = Interner::new();
    interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"))
}

/// 硬要求一：卫兵会真的发起盘查——一个卫兵和一个玩家在视野内，推进
/// 若干回合，断言盘查真的发生过（产出了 `Intent::Inspect`，经
/// `resolve` 产出对应的 `Effect::Inspect`，且如实带回了目标身上的
/// 物品）。
#[test]
fn 视野内有目标时卫兵最终会发起盘查() {
    // Arrange
    let mut registry = Registry::new();
    let guard_profession = intern_guard_profession(&mut registry);
    let race = human_race();
    let (mut world, _terrain_ids) = test_world();

    let mut item_interner = Interner::new();
    let sword = item_interner.intern(NamespacedId::parse("lostland:iron_sword").expect("合法"));
    let mut equipment = std::collections::BTreeMap::new();
    equipment.insert(EquipSlot::MAIN_HAND, ItemStack::new(sword, 1));

    let guard = spawn_agent(
        &mut world,
        5,
        5,
        guard_profession,
        race,
        Vec::new(),
        std::collections::BTreeMap::new(),
    );
    let target = spawn_agent(
        &mut world,
        6,
        5,
        race,
        race,
        vec![ItemStack::new(sword, 1)],
        equipment,
    );

    let source_code = load_guard_behavior_source();
    let mut source = ScriptBehaviorSource::new(
        &source_code,
        "guard-ai-tree",
        "examplemod",
        &registry,
        // 本文件不装载 `mods/`（见 `intern_guard_profession` 文档），
        // 因此也没有任何内容表可传——空快照让
        // `actor-inspection-suspicion` 恒返回「与常人无异」，
        // `guard-inspect-chance` 因此退回本批次之前的那个基础概率，
        // 本文件的三条既有断言不受盗贼被动接线影响。
        ll_mod::script_behavior_api::BehaviorRuleCatalogs::default(),
        1,
    )
    .expect("真实 behavior.scm 应当能通过白名单并装载成功");

    // Act：推进到第一次盘查发生为止（50% 触发率，60 回合内几乎必然
    // 命中至少一次，(1/2)^60 可忽略不计）。
    let intents = decide_over_ticks(&mut source, &mut world, guard, 60);
    let inspect_intent = intents
        .into_iter()
        .flatten()
        .find(|intent| matches!(intent, Intent::Inspect { .. }));

    // Assert：真的产出了盘查意图。
    let intent = inspect_intent.expect("视野内一直有目标，60 回合内应当至少触发一次盘查");
    assert_eq!(
        intent,
        Intent::Inspect {
            actor: guard,
            target
        }
    );

    // Act：结算——证明这不是一个只存在于脚本层的假意图，真的能走
    // resolve 产出效果。
    let effects = resolve(&world, &intent);

    // Assert：产出了带正确"看到了什么"快照的 Effect::Inspect。
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::Inspect { inspector, target: t, items_seen }
            if *inspector == guard
                && *t == target
                && items_seen.len() == 2
                && items_seen.iter().all(|&def| def == sword)
    )));
}

/// 硬要求二：隔着墙的卫兵看不见玩家——即使切比雪夫距离很近，一整排
/// 石墙挡在中间时，`nearby-actor-in-view` 应当恒为假，卫兵永远既不
/// 会盘查、也不会走近（两个分支用的是同一个视野查询），只会原地
/// 等待。证明视野判定走的是真正的 FOV，不是距离近似。
#[test]
fn 隔墙的卫兵看不见玩家因而从不发起盘查() {
    // Arrange
    let mut registry = Registry::new();
    let guard_profession = intern_guard_profession(&mut registry);
    let race = human_race();
    let (mut world, terrain_ids) = test_world();

    let guard = spawn_agent(
        &mut world,
        5,
        5,
        guard_profession,
        race,
        Vec::new(),
        std::collections::BTreeMap::new(),
    );
    let _target = spawn_agent(
        &mut world,
        7,
        5,
        race,
        race,
        Vec::new(),
        std::collections::BTreeMap::new(),
    );
    // 竖直一整排石墙挡在 x=6，把卫兵与目标完全隔开——理由同
    // crates/ll-script/src/api/actor.rs 的
    // `隔着石墙的目标即使距离很近也看不见` 测试。
    for y in 0..12 {
        world
            .terrain
            .set_terrain(world.size.wrap(6, y), terrain_ids.wall_stone);
    }

    let source_code = load_guard_behavior_source();
    let mut source = ScriptBehaviorSource::new(
        &source_code,
        "guard-ai-tree",
        "examplemod",
        &registry,
        // 本文件不装载 `mods/`（见 `intern_guard_profession` 文档），
        // 因此也没有任何内容表可传——空快照让
        // `actor-inspection-suspicion` 恒返回「与常人无异」，
        // `guard-inspect-chance` 因此退回本批次之前的那个基础概率，
        // 本文件的三条既有断言不受盗贼被动接线影响。
        ll_mod::script_behavior_api::BehaviorRuleCatalogs::default(),
        1,
    )
    .expect("真实 behavior.scm 应当能通过白名单并装载成功");

    // Act
    let intents = decide_over_ticks(&mut source, &mut world, guard, 60);

    // Assert：既没有盘查，也没有朝目标移动——每一次决策都退化成
    // guard-try-approach 的兜底分支 'wait（见 behavior.scm）。
    for intent in intents.into_iter().flatten() {
        assert_eq!(intent, Intent::Wait { actor: guard });
    }
}

/// 硬要求五：确定性重放——同一个世界种子跑两遍，盘查发生的时机
/// （在哪个 tick）与对象完全相同。
#[test]
fn 相同种子的两次决策序列完全相同() {
    // Arrange：两份完全独立的世界/引擎，只有 world_seed（这里固定传 7）
    // /actor id/tick 序列相同。
    let mut registry = Registry::new();
    let guard_profession = intern_guard_profession(&mut registry);
    let race = human_race();
    let source_code = load_guard_behavior_source();

    let build = || {
        let (mut world, _terrain_ids) = test_world();
        let guard = spawn_agent(
            &mut world,
            5,
            5,
            guard_profession,
            race,
            Vec::new(),
            std::collections::BTreeMap::new(),
        );
        let _target = spawn_agent(
            &mut world,
            6,
            5,
            race,
            race,
            Vec::new(),
            std::collections::BTreeMap::new(),
        );
        (world, guard)
    };

    let (mut world_a, guard_a) = build();
    let (mut world_b, guard_b) = build();
    // 两个引擎都在编译之前造齐——见 `ll_script::host` 里
    // `COMPILED_ON_THIS_THREAD` 上方注释与 ADR 0028：同一根线程上全部
    // 引擎构造必须先于全部脚本编译，写成「造一个编一个」第二次构造会
    // 直接 panic。
    let prepared_a = PreparedBehaviorEngine::new();
    let prepared_b = PreparedBehaviorEngine::new();
    let mut source_a = ScriptBehaviorSource::from_prepared(
        prepared_a,
        &source_code,
        "guard-ai-tree",
        "examplemod",
        &registry,
        ll_mod::script_behavior_api::BehaviorRuleCatalogs::default(),
        7,
    )
    .expect("真实 behavior.scm 应当能通过白名单并装载成功");
    let mut source_b = ScriptBehaviorSource::from_prepared(
        prepared_b,
        &source_code,
        "guard-ai-tree",
        "examplemod",
        &registry,
        ll_mod::script_behavior_api::BehaviorRuleCatalogs::default(),
        7,
    )
    .expect("真实 behavior.scm 应当能通过白名单并装载成功");

    // Act
    let sequence_a = decide_over_ticks(&mut source_a, &mut world_a, guard_a, 40);
    let sequence_b = decide_over_ticks(&mut source_b, &mut world_b, guard_b, 40);

    // Assert：两个实体在两个世界里的编号相同（都是各自世界里第一个
    // 生成的实体），意图序列因此必须逐个相等——盘查发生的 tick 与
    // 目标完全一致。两个序列里都应当真的出现过至少一次盘查，否则本
    // 测试无法证明"盘查的时机也重放一致"这句话（只重放了一堆 Wait
    // 算不上验证）。
    assert_eq!(sequence_a, sequence_b);
    assert!(
        sequence_a
            .iter()
            .flatten()
            .any(|intent| matches!(intent, Intent::Inspect { .. })),
        "40 回合内应当至少触发一次盘查，否则本测试没有真正覆盖盘查分支"
    );
}
