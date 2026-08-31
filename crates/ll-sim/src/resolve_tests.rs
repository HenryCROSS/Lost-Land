//! `crate::resolve` 的断言。
//!
//! 用 `#[path]` 挂成 `crate::resolve` 的子模块而不是 `tests/` 下的集成测试：
//! 这些断言要看得见 `resolve` 的私有项（`use super::*`），走集成测试就得把一
//! 大批只为测试而存在的东西改成 `pub`。同一手法在 `app_save_tests.rs` 与
//! `app_navigation_tests.rs` 已有两处先例。
//!
//! 批次 16 把它整体搬出 `resolve.rs`，**一个字都没改**。刻意不按意图族拆开：
//! 45 条断言与约十个共享夹具（`test_world`/`spawn_agent`/`spawn_agent_with_luck`
//! …）交错在一起，按族切开要么重复夹具、要么造一个被九个模块回头调用的
//! `test_support`，那正是「一堆互相调来调去的小文件」。而且它不影响下一批的
//! 难度：新意图族在自己的模块里写自己的断言。

// 断言自己的导入。批次 16 把断言从 `resolve.rs` 搬出来时，`resolve.rs` 里那
// 些「只有断言用得到」的导入跟着搬到了这里——`use super::*` 只带得走父模块
// 里**仍在被父模块自己使用**的名字。断言本身一个字都没改。

use crate::check::CHECK_DICE;
use crate::combat::{Penetration, damage_after_defense};
use crate::damage_category::NoDamageCategories;
use crate::effect::Effect;
use crate::formula::{DamageFormulaCatalog, NoFormulas};
use crate::intent::{Direction, Intent};
use crate::item::NoItems;
use crate::quest::NoQuests;
use crate::resource_pool::NoResourcePools;
use crate::skill::NoSkills;
use crate::traits::{NO_TRAIT_GRANTS, NoTraitGrants, NoTraits, TraitCatalog, TraitGrantSource};
use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_world::entity::{ActiveStatModifier, Agent, AttributeKind, BaseStats, EntityId};
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::temperature::Temperature;

use ll_core::torus::TorusSize;
use ll_world::generate::GenParams;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

use super::*;

/// 测试用区块布局：边长 64，单个区块——是噪声格点周期的整数倍，
/// 满足 `WorldState::new` 的前置条件（与 `ll-sim`/`ll-world` 既有
/// 测试同一常量），整个测试世界落在这一个区块内。
fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
}

/// 返回值附带 [`BaseTerrainIds`]：`terrain_ids` 与
/// `world.terrain_table` 必须来自同一次 [`base_terrain_fixture`]
/// 调用——`ContentIndex` 只在产出它的那个 `Interner` 里有意义
/// （`ll_core::ident` 模块文档），两次独立调用各自的索引分配虽然
/// 因为固定顺序而恰好数值相同，但把它们当成「必须配对」处理更不
/// 容易在将来注册顺序调整时踩坑。
fn test_world() -> (WorldState, BaseTerrainIds) {
    let layout = test_layout();
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

/// 造一个占位实体，站在 `(5, 5)`，六项主属性取基准值，`current_space`
/// 取地表（占位层属性索引——本文件的移动/攻击/开门测试不消费空间
/// 层属性，见 `Space::surface` 文档）。
fn spawn_agent(world: &mut WorldState) -> EntityId {
    let mut interner = ll_core::ident::Interner::new();
    let profession = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
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
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: surface_space_at(world, pos),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

/// 在 `pos` 上再造一个占位实体——占位不变式的用例需要「目的地那
/// 一格站着别人」这个场景，而 [`spawn_agent`] 恒生成在 `(5, 5)`。
/// 除位置外与 [`spawn_agent`] 逐字段相同。
fn spawn_agent_at(world: &mut WorldState, pos: ll_core::torus::TorusPos) -> EntityId {
    let existing = spawn_agent(world);
    world.actors.get_mut(existing).expect("刚生成必然存在").pos = pos;
    existing
}

/// 把 `actor` 的潜行状态置为 `stealthed`——潜行相关测试的公共
/// Arrange 步骤。直接写字段而不是先跑一次 `Intent::ToggleStealth`：
/// 那会让「移动开销」这类测试的断言同时依赖切换本身是否正确，两件
/// 事应当各自独立验证（切换本身由
/// `切换潜行产出取反后的确定状态并消耗一个回合` 单独覆盖）。
fn set_stealthed(world: &mut WorldState, actor: EntityId, stealthed: bool) {
    world
        .actors
        .get_mut(actor)
        .expect("调用方刚生成的实体必然存在")
        .stealthed = stealthed;
}

#[test]
fn 切换潜行产出取反后的确定状态并消耗一个回合() {
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);

    // Act：从「未潜行」切一次。
    let effects = resolve(&world, &Intent::ToggleStealth { actor });

    // Assert：产出确定值 true（不是「取反」这个指令本身），且排了
    // 下一次行动（消耗了一个回合）。
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::SetStealth {
            actor: a,
            stealthed: true
        } if *a == actor
    )));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ScheduleNext { actor: a, .. } if *a == actor))
    );
}

#[test]
fn 已在潜行中再次切换产出退出潜行() {
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    set_stealthed(&mut world, actor, true);

    // Act
    let effects = resolve(&world, &Intent::ToggleStealth { actor });

    // Assert
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::SetStealth {
            actor: a,
            stealthed: false
        } if *a == actor
    )));
}

#[test]
fn 切换潜行的耗时与原地等待相同() {
    // 「消耗一个回合」这句话的准确含义：与 Intent::Wait 逐刻相同，
    // 不是另起一个数字，见 resolve_toggle_stealth 文档。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);

    // Act
    let toggle_at = next_action_tick(&resolve(&world, &Intent::ToggleStealth { actor }));
    let wait_at = next_action_tick(&resolve(&world, &Intent::Wait { actor }));

    // Assert
    assert_eq!(toggle_at, wait_at);
}

#[test]
fn 潜行时移动一格比不潜行时更慢() {
    // Arrange：两个完全相同的世界，只差潜行状态。目的地显式铺成
    // 草地——`test_world` 用 `GenParams::default()` 生成，出生点
    // 东侧未必可通行，不铺的话两条断言都会落进撞墙分支而恒相等
    // （与本文件其余移动测试同一条既有做法）。
    let (mut visible_world, ids_a) = test_world();
    let visible = spawn_agent(&mut visible_world);
    visible_world
        .terrain
        .set_terrain(east_of_spawn(&visible_world), ids_a.grass);
    let (mut stealth_world, ids_b) = test_world();
    let sneaker = spawn_agent(&mut stealth_world);
    stealth_world
        .terrain
        .set_terrain(east_of_spawn(&stealth_world), ids_b.grass);
    set_stealthed(&mut stealth_world, sneaker, true);

    // Act
    let open_at = next_action_tick(&resolve(
        &visible_world,
        &Intent::Move {
            actor: visible,
            dir: Direction::East,
        },
    ));
    let sneak_at = next_action_tick(&resolve(
        &stealth_world,
        &Intent::Move {
            actor: sneaker,
            dir: Direction::East,
        },
    ));

    // Assert：STEALTH_MOVE_COST_PERMILLE 是 2000（两倍），两次都从
    // Tick(0) 起算，因此潜行那一步的下一次行动时刻应当恰好是两倍。
    assert!(sneak_at > open_at);
    assert_eq!(sneak_at, open_at * 2);
}

#[test]
fn 潜行不改变撞墙的耗时() {
    // 反面覆盖 resolve_move 里「只挂在真的挪动了位置的那一条分支」
    // 这句话：撞墙走的是 BASE_ACTION_COST，不是地形开销，潜行不该
    // 让撞墙也变慢。
    // Arrange：东侧摆一堵石墙。
    let (mut visible_world, ids_a) = test_world();
    let visible = spawn_agent(&mut visible_world);
    let wall_a = east_of_spawn(&visible_world);
    visible_world.terrain.set_terrain(wall_a, ids_a.wall_stone);

    let (mut stealth_world, ids_b) = test_world();
    let sneaker = spawn_agent(&mut stealth_world);
    let wall_b = east_of_spawn(&stealth_world);
    stealth_world.terrain.set_terrain(wall_b, ids_b.wall_stone);
    set_stealthed(&mut stealth_world, sneaker, true);

    // Act
    let open_at = next_action_tick(&resolve(
        &visible_world,
        &Intent::Move {
            actor: visible,
            dir: Direction::East,
        },
    ));
    let sneak_at = next_action_tick(&resolve(
        &stealth_world,
        &Intent::Move {
            actor: sneaker,
            dir: Direction::East,
        },
    ));

    // Assert
    assert_eq!(open_at, sneak_at);
}

#[test]
fn 盘查消耗一个回合() {
    // 回归：`resolve_inspect` 曾经只产出一条 `Effect::Inspect`，
    // 不产出 `Effect::ScheduleNext`——被盘查者的下一次行动时刻
    // 原地不动，`TurnEngine::perform` 会把它重新排回**同一个
    // tick**，`advance_ai` 因此对同一个卫兵反复空转直到耗尽
    // `MAX_STEPS_PER_ADVANCE`。这个缺陷一直没暴露，只是因为在
    // 行为树接进回合引擎之前，`Intent::Inspect` 从来没有经由
    // `TurnEngine` 产出过；接上之后立刻表现为整条测试挂死。
    // Arrange
    let (mut world, _ids) = test_world();
    let guard = spawn_agent(&mut world);
    let target = spawn_agent(&mut world);

    // Act
    let effects = resolve(
        &world,
        &Intent::Inspect {
            actor: guard,
            target,
        },
    );

    // Assert：盘查照旧产出，且下一次行动时刻严格晚于当前世界时钟。
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Inspect { .. })),
        "盘查本身仍然要产出"
    );
    assert!(
        next_action_tick(&effects) > world.clock.0,
        "盘查必须推进发起者的下一次行动时刻，否则时间轴会在同一 tick 空转"
    );
}

/// 从一批效果里取出 `Effect::ScheduleNext` 的时刻——潜行相关的
/// 耗时断言反复需要这一步。
fn next_action_tick(effects: &[Effect]) -> i64 {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::ScheduleNext { at, .. } => Some(at.0),
            _ => None,
        })
        .expect("这些意图都会排下一次行动")
}

/// 造一份「站在 `pos` 上」的地表空间——`current_space` 的
/// `profile` 用一个占位 `ContentIndex`（本文件测试不消费空间层
/// 属性），`zone` 由测试世界自身的区块布局推出。
fn surface_space_at(world: &WorldState, pos: ll_core::torus::TorusPos) -> Space {
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    Space::surface(zone, ll_core::ident::ContentIndex::default())
}

/// 从 `(5, 5)` 向东（`dx = 1`）走一步的目的地，与 [`spawn_agent`]
/// 的出生点配套——测试只需要一个已知、可控的目的地格。
fn east_of_spawn(world: &WorldState) -> ll_core::torus::TorusPos {
    world.size.wrap(6, 5)
}

/// 造一个占位实体，站在 `(5, 5)`，除敏捷外六项主属性取基准值——
/// 供敏捷相关测试指定一个非基准的敏捷值。
fn spawn_agent_with_dexterity(world: &mut WorldState, dexterity: i32) -> EntityId {
    let mut interner = ll_core::ident::Interner::new();
    let profession = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats {
            dexterity,
            ..BaseStats::BASELINE
        },
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
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: surface_space_at(world, pos),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

/// 造一个占位实体，站在 `pos`，除幸运外六项主属性取基准值——供
/// 暴击率频率测试指定一个非零的幸运值，与 [`spawn_agent_with_dexterity`]
/// 同一个模式。
fn spawn_agent_with_luck(
    world: &mut WorldState,
    pos: ll_core::torus::TorusPos,
    luck: i32,
) -> EntityId {
    let mut interner = ll_core::ident::Interner::new();
    let profession = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats {
            luck,
            ..BaseStats::BASELINE
        },
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
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: surface_space_at(world, pos),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

#[test]
fn 结算不修改世界() {
    // resolve 的签名只接受 &WorldState，编译期已经不允许它写世界；
    // 这条测试是这个保证的行为级回归——即使产出了效果，调用 resolve
    // 本身也绝不应改变世界的哈希（哈希已覆盖地形与实体状态，见
    // WorldState::hash 文档）。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    world
        .terrain
        .set_terrain(east_of_spawn(&world), terrain_ids.grass);
    let intent = Intent::Move {
        actor,
        dir: Direction::East,
    };
    let hash_before = world.hash();

    // Act
    let effects = resolve(&world, &intent);

    // Assert
    assert!(!effects.is_empty(), "本用例应产生效果，否则测不出意义");
    assert_eq!(world.hash(), hash_before);
}

#[test]
fn 移动到不可通行地形不产生移动效果() {
    // 项目所有者决策：撞墙仍要消耗时间（见 resolve_move 文档「目的地
    // 完全不可通行」一节），本用例只锁定「不产生 MoveTo」这一件事
    // ——时间是否推进、位置是否不变分别由下面两条测试独立断言。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    world
        .terrain
        .set_terrain(east_of_spawn(&world), terrain_ids.wall_stone);
    let intent = Intent::Move {
        actor,
        dir: Direction::East,
    };

    // Act
    let effects = resolve(&world, &intent);

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::MoveTo { .. }))
    );
}

#[test]
fn 撞墙仍产生排期效果推进行动时间() {
    // 撞墙本身是一次真实的行动尝试（伸手推了一下、发现推不开），
    // 应当消耗时间——这是本次缺陷交接记录明确记录的项目所有者决策。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    world
        .terrain
        .set_terrain(east_of_spawn(&world), terrain_ids.wall_stone);
    let intent = Intent::Move {
        actor,
        dir: Direction::East,
    };

    // Act
    let effects = resolve(&world, &intent);

    // Assert
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ScheduleNext { actor: a, .. } if *a == actor))
    );
}

#[test]
fn 撞墙结算后应用效果位置不变() {
    // 与上一条互补：确认「消耗时间」没有连带着悄悄移动位置——两件
    // 事分别断言,不合并进同一个测试。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    world
        .terrain
        .set_terrain(east_of_spawn(&world), terrain_ids.wall_stone);
    let pos_before = world
        .actors
        .get(actor)
        .expect("刚 spawn 的实体必然存在")
        .pos;
    let intent = Intent::Move {
        actor,
        dir: Direction::East,
    };

    // Act
    let effects = resolve(&world, &intent);
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert
    let pos_after = world.actors.get(actor).expect("apply 不会移除实体").pos;
    assert_eq!(pos_after, pos_before);
}

#[test]
fn 移动到浅水的行动耗时高于草地() {
    // Arrange
    let (mut grass_world, grass_ids) = test_world();
    let grass_actor = spawn_agent(&mut grass_world);
    grass_world
        .terrain
        .set_terrain(east_of_spawn(&grass_world), grass_ids.grass);

    let (mut water_world, water_ids) = test_world();
    let water_actor = spawn_agent(&mut water_world);
    water_world
        .terrain
        .set_terrain(east_of_spawn(&water_world), water_ids.shallow_water);

    // Act
    let grass_effects = resolve(
        &grass_world,
        &Intent::Move {
            actor: grass_actor,
            dir: Direction::East,
        },
    );
    let water_effects = resolve(
        &water_world,
        &Intent::Move {
            actor: water_actor,
            dir: Direction::East,
        },
    );

    // Assert
    let grass_cost = schedule_next_at(&grass_effects).0 - grass_world.clock.0;
    let water_cost = schedule_next_at(&water_effects).0 - water_world.clock.0;
    assert!(water_cost > grass_cost);
}

#[test]
fn 目的地站着别的实体时移动不产出moveto但仍推进时钟() {
    // 「每格至多站一人」这条不变式本身（项目所有者裁定：从一句只
    // 写在注释里的说法升级成真正被强制的规则）。本批次之前
    // `resolve_move` 里**一行占位检查都没有**，这条断言的左半在
    // 那时候是假的：两个实体可以直接摞在同一格上。
    //
    // 右半（仍然产出 `ScheduleNext`）同样是断言的一部分，不是顺带
    // ——与撞墙同一个口径。若这里退化成空 `Vec`，非受控实体那条路
    // 会被 `TurnEngine` 的进展保证兜住（不至于死循环），但受控实体
    // 那条路会把「前面站着人」这个确定结果报成「这一步白按了」。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    let dest = east_of_spawn(&world);
    // **这一行是断言的一部分**：`test_world` 的地形来自世界生成，
    // 目的地那一格可能天生就不可通行，那样「不产出 MoveTo」靠的会是
    // 撞墙分支而不是占位检查，摘掉占位检查这条照样绿（ADR 0018 反例
    // 验证抓出来的一处假绿）。显式铺成草地，把「地形挡不挡路」这个
    // 变量从本用例里摘掉。
    world.terrain.set_terrain(dest, terrain_ids.grass);
    let blocker = spawn_agent_at(&mut world, dest);
    let intent = Intent::Move {
        actor,
        dir: Direction::East,
    };

    // Act
    let effects = resolve(&world, &intent);

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::MoveTo { .. })),
        "目的地站着 {blocker:?}，这一步不该产出任何 MoveTo：{effects:?}"
    );
    assert!(schedule_next_at(&effects).0 > world.clock.0);
}

#[test]
fn 目的地是关着的门且门上站着人时判成撞人而不是开门() {
    // 占位检查排在开门分支**之前**这条顺序的证据。反过来排的话，
    // 「门那一格站着人」会先把门推开、消耗一回合，下一回合才发现
    // 人挡着——一个要两回合才识破的怪异结果，而不变式的字面意思
    // 根本不区分目的地是门还是平地。
    //
    // 这条同时守住一件更要紧的事：`Effect::SetTerrain` 是一次**真
    // 实的世界写入**。顺序排错的话，一个被人堵死的门口会在每一次
    // 徒劳的撞击里被反复改写成「开着」。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    let door = east_of_spawn(&world);
    world.terrain.set_terrain(door, terrain_ids.door_closed);
    let blocker = spawn_agent_at(&mut world, door);
    let intent = Intent::Move {
        actor,
        dir: Direction::East,
    };

    // Act
    let effects = resolve(&world, &intent);

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::SetTerrain { .. })),
        "门上站着 {blocker:?}，这一步不该开门：{effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::MoveTo { .. }))
    );
    assert!(schedule_next_at(&effects).0 > world.clock.0);
}

#[test]
fn 退出建筑时锚点被占会造出两人同格这条缺口仍然存在() {
    // **这条钉的是当前行为，不是当前的正确行为。**
    //
    // `resolve_move` 现在强制「每格至多站一人」，但
    // `resolve_exit_space` 自行构造 `Effect::MoveTo { pos: anchor }`、
    // 不走那条检查，因此仍然造得出两人同格——这是本批次刻意不堵的
    // 一处剩余缺口，理由见 `resolve_exit_space` 文档「已知缺口」
    // 一节（堵它必须先裁定「退出时锚点有人，人去哪」，而那条与
    // 作弊传送时如何安置挡路 NPC 是同一个尚未落地的机制）。
    //
    // 这条断言的用处是：将来真正堵这个缺口的人会立刻看到自己改动
    // 了什么，而不是在一片全绿里悄悄换掉一条语义。**它变红不代表
    // 出了缺陷，代表缺口被堵上了**——届时请把它改写成新行为的
    // 断言，不要删掉。
    // Arrange：actor 进了建筑，另一个人随后站到锚点上。
    let (mut world, _ids) = test_world();
    let actor = spawn_agent(&mut world);
    let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
    let interior_id = insert_interior_at(&mut world, anchor);
    for effect in &resolve(
        &world,
        &Intent::EnterSpace {
            actor,
            target: interior_id,
        },
    ) {
        crate::apply::apply(&mut world, effect);
    }
    let squatter = spawn_agent_at(&mut world, anchor);

    // Act
    for effect in &resolve(&world, &Intent::ExitSpace { actor }) {
        crate::apply::apply(&mut world, effect);
    }

    // Assert 一：退出**真的发生了**——`current_space` 回到了地表。
    //
    // 这一条不是布景。少了它，本用例在「`resolve_exit_space` 学会
    // 查占位、锚点有人就静默作废」的那个实现下**照样全绿**：那时
    // 谁都没动，两人的 `pos` 仍然都等于 `anchor`（进 Interior 的
    // 一方 `pos` 从进去起就没变过）。ADR 0018 反例验证抓出来的一处
    // 假绿——这条钉子若钉不住「缺口被堵上」这件事，它就没有存在的
    // 意义。
    assert!(
        matches!(
            world.actors.get(actor).expect("还在").current_space,
            Space::Surface { .. }
        ),
        "退出应当真的把 current_space 换回地表"
    );

    // Assert 二：于是两个人此刻站在同一格上。
    assert_eq!(world.actors.get(actor).expect("还在").pos, anchor);
    assert_eq!(world.actors.get(squatter).expect("还在").pos, anchor);
}

#[test]
fn 攻击关着的门产生开门效果而非伤害效果() {
    // 「攻击关着的门」在这套设计里就是朝它的方向移动一步——门不是
    // 实体，Intent::Attack 的 target 必须是 EntityId，指向不了一格
    // 地形；玩家的「攻击」输入落到 resolve 这里，撞见关着的门时
    // 被派生成开门而不是造成伤害。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    world
        .terrain
        .set_terrain(east_of_spawn(&world), terrain_ids.door_closed);
    let intent = Intent::Move {
        actor,
        dir: Direction::East,
    };

    // Act
    let effects = resolve(&world, &intent);

    // Assert
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::SetTerrain { kind, .. } if *kind == terrain_ids.door_open
    )));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. }))
    );
}

#[test]
fn 撞入即开不是只对关着的门生效的特判() {
    // 这是本次迁移撞见并修掉的 API 洞的直接验收：opens_into 是
    // 任意地形都能声明的属性，不是只有 lostland:door_closed 才有
    // 的硬编码特权——一个假想 mod 注册的「活板门」同样应该走这条
    // 通用路径，而不需要去改 ll-sim 的源码。
    //
    // 用同一个 Interner 先注册本体 17 个地形、再追加两个自定义地形
    // ——不能各自新起一个 Interner：ContentIndex 只在产出它的那个
    // Interner 里有意义，另起一个会与本体的 0..17 撞号。
    // Arrange
    let mut interner = ll_core::ident::Interner::new();
    let (terrain_ids, mut table) =
        ll_world::terrain::materialize_base_terrain(&mut |id| interner.intern(id))
            .expect("本体地形声明表内部一致");
    let hatch_open = ll_world::terrain::TerrainKind::from_index(
        interner.intern(ll_core::ident::NamespacedId::parse("yourmod:hatch_open").expect("合法")),
    );
    let hatch_closed = ll_world::terrain::TerrainKind::from_index(
        interner.intern(ll_core::ident::NamespacedId::parse("yourmod:hatch_closed").expect("合法")),
    );
    table
        .define(
            hatch_open.index(),
            ll_world::terrain::TerrainAttrs {
                blocks_sight: false,
                blocks_move: false,
                move_cost: 100,
                opens_into: None,
            },
        )
        .expect("测试声明内部自洽");
    table
        .define(
            hatch_closed.index(),
            ll_world::terrain::TerrainAttrs {
                blocks_sight: false,
                blocks_move: true,
                move_cost: u32::MAX,
                opens_into: Some(hatch_open),
            },
        )
        .expect("测试声明内部自洽");

    let layout = test_layout();
    let spawn = layout.tile_size().wrap(0, 0);
    let mut world = WorldState::new(layout, &GenParams::default(), &terrain_ids, table, spawn)
        .expect("测试布局满足全部构造前置条件");
    world
        .terrain
        .set_terrain(east_of_spawn(&world), hatch_closed);
    let actor = spawn_agent(&mut world);

    // Act
    let effects = resolve(
        &world,
        &Intent::Move {
            actor,
            dir: Direction::East,
        },
    );

    // Assert
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::SetTerrain { kind, .. } if *kind == hatch_open
    )));
}

#[test]
fn 对着不能开的地形使用开门意图仍消耗行动时间() {
    // 与 resolve_move 撞墙同一条决策：`Intent::OpenDoor` 对着一格
    // 并非「撞入即开」的地形（这里直接用普通草地）时，仍是一次
    // 「查得到目标、确认这个动作在此处不成立」的确定结果，应当
    // 消耗时间——见 resolve_open_door 文档。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    let target = east_of_spawn(&world);
    world.terrain.set_terrain(target, terrain_ids.grass);
    let intent = Intent::OpenDoor {
        actor,
        pos: (target.x(), target.y()),
    };

    // Act
    let effects = resolve(&world, &intent);

    // Assert
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ScheduleNext { actor: a, .. } if *a == actor))
    );
}

#[test]
fn 对着不能开的地形使用开门意图不改写地形() {
    // 与上一条互补：确认「消耗时间」没有连带着悄悄把目标地形改写成
    // 别的东西——两件事分别断言，不合并进同一个测试。
    // Arrange
    let (mut world, terrain_ids) = test_world();
    let actor = spawn_agent(&mut world);
    let target = east_of_spawn(&world);
    world.terrain.set_terrain(target, terrain_ids.grass);
    let intent = Intent::OpenDoor {
        actor,
        pos: (target.x(), target.y()),
    };

    // Act
    let effects = resolve(&world, &intent);

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::SetTerrain { .. }))
    );
}

#[test]
fn 敏捷更高的角色等待耗时更短() {
    // 这是 P3 验收 demo（Task 9）排查出的阻断性缺陷的回归测试：
    // 修复前 resolve 的四个分支全部直接传常量 BASELINE_EFFECTIVE_SPEED，
    // 不读 agent.stats.dexterity，敏捷高低对行动耗时毫无影响——时间轴
    // 调度器「敏捷高者能在同一窗口内多行动几次」这条核心手感因此在
    // 结算层根本不成立。
    // Arrange
    let (mut slow_world, _slow_ids) = test_world();
    let slow_actor = spawn_agent_with_dexterity(&mut slow_world, 5);
    let (mut fast_world, _fast_ids) = test_world();
    let fast_actor = spawn_agent_with_dexterity(&mut fast_world, 40);

    // Act
    let slow_effects = resolve(&slow_world, &Intent::Wait { actor: slow_actor });
    let fast_effects = resolve(&fast_world, &Intent::Wait { actor: fast_actor });

    // Assert
    let slow_cost = schedule_next_at(&slow_effects).0 - slow_world.clock.0;
    let fast_cost = schedule_next_at(&fast_effects).0 - fast_world.clock.0;
    assert!(fast_cost < slow_cost);
}

/// 在 `world` 里插入一个锚定在 `anchor` 的 `Interior`，带一层 0 层
/// 楼层（4x4 石地板）——task 12 进出空间测试的公共夹具。
fn insert_interior_at(
    world: &mut WorldState,
    anchor: ll_core::torus::TorusPos,
) -> ll_core::ident::WorldId {
    let mut counter = 0u32;
    let mut interner = ll_core::ident::Interner::new();
    let profile = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("字面量恒合法"));
    let id = ll_core::ident::WorldId::next(&mut counter);
    let mut interior = ll_world::interior::Interior::new(id, anchor, profile);
    let (ids, _table) = base_terrain_fixture();
    let size = ll_core::bounded::BoundedSize::new(4, 4).expect("4x4 是合法尺寸");
    interior.set_floor(
        0,
        ll_world::bounded_grid::BoundedGrid::new(size, ids.floor_stone),
    );
    world.insert_interior(interior);
    id
}

#[test]
fn 站在有interior入口的格子上触发进入意图产出changespace效果() {
    // Arrange
    let (mut world, _ids) = test_world();
    let actor = spawn_agent(&mut world);
    let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
    let interior_id = insert_interior_at(&mut world, anchor);

    // Act
    let effects = resolve(
        &world,
        &Intent::EnterSpace {
            actor,
            target: interior_id,
        },
    );

    // Assert
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::ChangeSpace { space: Space::Interior { id, .. }, .. } if *id == interior_id
    )));
}

#[test]
fn 站在没有interior入口的格子上触发进入意图不产生任何空间切换() {
    // Arrange：Interior 锚定在离玩家很远的一格,玩家当前所在格没有
    // 任何入口。
    let (mut world, _ids) = test_world();
    let actor = spawn_agent(&mut world);
    let far_anchor = world.size.wrap(40, 40);
    let interior_id = insert_interior_at(&mut world, far_anchor);

    // Act
    let effects = resolve(
        &world,
        &Intent::EnterSpace {
            actor,
            target: interior_id,
        },
    );

    // Assert
    assert!(effects.is_empty());
}

#[test]
fn 进入interior后agent的pos不变只有当前空间变化() {
    // Arrange
    let (mut world, _ids) = test_world();
    let actor = spawn_agent(&mut world);
    let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
    let interior_id = insert_interior_at(&mut world, anchor);
    let effects = resolve(
        &world,
        &Intent::EnterSpace {
            actor,
            target: interior_id,
        },
    );

    // Act
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert
    let agent = world.actors.get(actor).expect("刚生成必然存在");
    assert_eq!(agent.pos, anchor);
    assert!(matches!(agent.current_space, Space::Interior { id, .. } if id == interior_id));
}

#[test]
fn 退出interior后agent的pos恢复为interior的锚点() {
    // Arrange：先进入,把玩家「弄脏」成一个非锚点位置不需要——本批次
    // Interior 内部移动本就静默无效（见模块文档），这里直接验证
    // 退出后 pos 仍精确等于锚点,而不是随便一个值。
    let (mut world, _ids) = test_world();
    let actor = spawn_agent(&mut world);
    let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
    let interior_id = insert_interior_at(&mut world, anchor);
    for effect in &resolve(
        &world,
        &Intent::EnterSpace {
            actor,
            target: interior_id,
        },
    ) {
        crate::apply::apply(&mut world, effect);
    }

    // Act
    let exit_effects = resolve(&world, &Intent::ExitSpace { actor });
    for effect in &exit_effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert
    let agent = world.actors.get(actor).expect("刚生成必然存在");
    assert_eq!(agent.pos, anchor);
    assert!(matches!(agent.current_space, Space::Surface { .. }));
}

#[test]
fn worldstate的hash纳入current_space的变化() {
    // Arrange
    let (mut world, _ids) = test_world();
    let actor = spawn_agent(&mut world);
    let anchor = world.actors.get(actor).expect("刚生成必然存在").pos;
    let interior_id = insert_interior_at(&mut world, anchor);
    let hash_before = world.hash();
    let effects = resolve(
        &world,
        &Intent::EnterSpace {
            actor,
            target: interior_id,
        },
    );

    // Act
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert：只有 current_space 变了（pos/health/wallet/
    // next_action_at 均未受这条 Intent 影响),哈希仍必须不同——否则
    // 说明 hash() 没有真正混入 current_space。
    assert_ne!(world.hash(), hash_before);
}

/// 从一批效果里取出 [`Effect::ScheduleNext`] 的排期时刻——上面几条
/// 移动耗时测试都要读这个字段，抽成小工具避免重复的
/// `iter().find_map(...)`。
fn schedule_next_at(effects: &[Effect]) -> Tick {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::ScheduleNext { at, .. } => Some(*at),
            _ => None,
        })
        .expect("本文件的移动类测试用例都应产生 ScheduleNext 效果")
}

/// 造一个已具名（`remembered_id` 已赋值）的占位实体，站在 `pos`,
/// 生命值可由调用方指定——供击杀历史记录的端到端测试构造"低血量
/// 但已经被记住"的目标。
fn spawn_named_agent(
    world: &mut WorldState,
    pos: ll_core::torus::TorusPos,
    health: i32,
) -> EntityId {
    let mut interner = ll_core::ident::Interner::new();
    let profession = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let race = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));
    let mut world_id_counter = 0u32;
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
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
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: surface_space_at(world, pos),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: Some(ll_core::ident::WorldId::next(&mut world_id_counter)),
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

#[test]
fn 近战攻击致死已具名目标后历史事件记录着近战死因() {
    // 端到端验证（不是结构往返）：从 Intent::Attack 造成致死伤害
    // 开始，一路断言到 apply 真的把这条击杀写进
    // world.history——KillCause 必须精确到「近战」这一级，而不是
    // 只有一句"A 杀了 B"。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    // 生命值 1：BASELINE 力量算出的攻击力必然大于 1（见
    // combat::damage_after_defense 的单元测试），一击必死。
    let victim = spawn_named_agent(&mut world, victim_pos, 1);

    // Act
    let effects = resolve(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
    );
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert：目标真的被销毁（不是只造出了记录、目标却还活着）。
    assert!(world.actors.get(victim).is_none());
    // 历史事件真的被写入了,不是只在效果列表里飘过。
    assert_eq!(world.history.len(), 1);
    let ll_world::history::HistoricalEventKind::Kill(record) = &world.history[0].kind else {
        panic!("战斗结算写进 WorldState::history 的必须是一条击杀记录");
    };
    // 致死手段精确到「近战」——不是笼统的"被杀"。
    assert!(matches!(
        record.cause,
        ll_world::history::KillCause::Melee { weapon: None }
    ));
    // 攻击者没有被记住（remembered_id 为 None），记录里的
    // killer 因此如实为 None——不是伪造出一个不存在的具名击杀者。
    assert_eq!(record.killer, None);
    // 致命一击确实造成了伤害、结算后生命值不高于零。
    assert!(record.killing_blow.damage > 0);
    assert!(record.killing_blow.remaining_health <= 0);
}

/// 恒对任意生物种类返回同一个固定经验值的测试用经验目录——真实
/// 实现（`ll-mod` 的 `RaceTable::xp_reward`）会按种类区分，这里的
/// 测试只关心「经验真的被授予了」这条链路本身是否接通，不关心具体
/// 种族与经验值的对应关系，用固定值足够、也更不脆弱（不依赖攻击者
/// /受害者各自 `Interner` 分配出的具体 `ContentIndex` 数值）。
struct FixedReward(i64);

impl crate::experience::ExperienceCatalog for FixedReward {
    fn xp_reward_for(&self, _kind: ll_core::ident::ContentIndex) -> i64 {
        self.0
    }
}

#[test]
fn 完整管线结算一次致死击杀后击杀者的经验真的增加() {
    // 端到端验证：从 Intent::Attack 造成致死伤害开始，走
    // resolve_with_skills_quests_and_experience（真实的四层入口，
    // 不是直接构造 Effect::GrantExperience 抄近路）+
    // apply_with_xp_curves，断言击杀者身上的 experience 字段确实
    // 变化了——这是设计文档五节「Effect::Kill 是正确的挂载点」
    // 落地后必须成立的最基本一条链路。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    // 生命值 1：一击必死，见「近战攻击致死……」测试同一注释。
    let victim = spawn_named_agent(&mut world, victim_pos, 1);
    let reward_amount = 30; // 小于 Agent::STARTING_XP_TO_NEXT_LEVEL（100），这条测试不涉及升级。

    // Act
    let effects = resolve_with_skills_quests_and_experience(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &NoSkills,
        &NoQuests,
        &FixedReward(reward_amount),
    );
    for effect in &effects {
        crate::apply::apply_with_xp_curves(
            &mut world,
            effect,
            &crate::xp_curve::FlatXpCurve::DEFAULT,
        );
    }

    // Assert：击杀者的经验值真的从零涨到了这次击杀应得的数额。
    assert_eq!(
        world
            .actors
            .get(attacker)
            .expect("攻击者仍然存活")
            .experience,
        reward_amount
    );
}

#[test]
fn 经验积累超过门槛时击杀者的等级真的提升且门槛真的重新求值() {
    // 端到端验证：这次击杀产出的经验足以跨过默认门槛
    // （Agent::STARTING_XP_TO_NEXT_LEVEL = 100），断言 apply 侧的
    // 升级循环真的把 level 加了一、真的用曲线目录重新算出了新的
    // xp_to_next_level（而不是原样保留旧值 100）——升级判定整段
    // 放进 apply 一次算完，见 apply::apply_with_xp_curves 文档。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    let victim = spawn_named_agent(&mut world, victim_pos, 1);
    let reward_amount = 150; // 150 > 100（默认门槛），恰好触发一次升级，剩余 50 点经验。
    // 升级后重算门槛用的曲线与 apply() 默认的保底曲线（100）取不同
    // 的固定值（250），这样"门槛真的被重新求值"这件事才能通过
    // "新值既不等于升级前的旧门槛，也不等于任何巧合相同的默认值"
    // 来验证，而不是巧合蒙对。
    let level_up_curve = crate::xp_curve::FlatXpCurve { amount: 250 };

    // Act
    let effects = resolve_with_skills_quests_and_experience(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &NoSkills,
        &NoQuests,
        &FixedReward(reward_amount),
    );
    for effect in &effects {
        crate::apply::apply_with_xp_curves(&mut world, effect, &level_up_curve);
    }

    // Assert：等级真的从 1 涨到了 2，新门槛真的等于曲线目录重新
    // 求值的结果（250），不是升级前的旧值（100）原样保留。
    let attacker_agent = world.actors.get(attacker).expect("攻击者仍然存活");
    assert_eq!(attacker_agent.level, Agent::STARTING_LEVEL + 1);
    assert_eq!(attacker_agent.xp_to_next_level, 250);
}

#[test]
fn 攻击者力量的生效中临时修正会改变结算出的伤害() {
    // 端到端验证（不是结构往返）：给攻击者的 active_stat_modifiers
    // 塞一条真实的力量修正 → 走真实的 resolve(Intent::Attack) +
    // apply → 断言目标掉血量确实随之变化。这条链路此前断在
    // resolve_attack 只读裸 attacker.stats.strength，从不看
    // active_stat_modifiers——两端各自都有测试覆盖（ActiveStatModifier
    // 的序列化往返、Effect::ApplyStatModifier 的 apply 单测），却没
    // 有一条测试穿过中间那根线，见 resolve_attack 与
    // derive_stats 的文档。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    // 生命值给够大的余量,这条测试只关心「伤害数值变了多少」，不
    // 关心目标是否被打死——致死路径已由上一条测试单独覆盖。
    let victim = spawn_named_agent(&mut world, victim_pos, 1_000);
    let mut interner = ll_core::ident::Interner::new();
    let source =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
    world
        .actors
        .get_mut(attacker)
        .expect("刚生成必然存在")
        .active_stat_modifiers
        .insert(
            AttributeKind::Strength,
            std::collections::BTreeMap::from([(
                source,
                ActiveStatModifier {
                    delta: 20,
                    expires_at: Tick(100),
                },
            )]),
        );
    // 期望伤害直接复用 combat::damage_after_defense（该公式本身已
    // 有独立单测覆盖，这里只用它算出「修正后的力量」应得的伤害，
    // 不是重新验证公式本身）——BASELINE 力量为 10，加上本测试
    // 施加的 +20 修正，应得力量 30。
    let expected_damage =
        damage_after_defense(BaseStats::BASELINE.strength + 20, 0, Penetration::NONE);

    // Act
    let effects = resolve(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
    );
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert：目标生命值精确反映了「叠加修正后的力量」算出的伤害，
    // 不是裸力量值算出的那个（更低的）数字。
    let victim_after = world.actors.get(victim).expect("生命值远高于伤害,不会死亡");
    assert_eq!(victim_after.health, 1_000 - expected_damage);
}

/// 任务硬要求二「全局默认公式必须逐行复现现在的行为」的验收——不
/// 走 [`NoFormulas`] 这条「没接目录」的短路便利类型，而是构造一个
/// 真正实现 [`DamageFormulaCatalog`] 的公式目录（其 `formula_for`
/// 恒返回 [`crate::formula::default_attack_power_instructions`]
/// 这条全局默认公式——与 `ll_mod::base_damage_formula::register_base_damage_formula`
/// 生产环境真正注册出来的那条公式逐字同构），证明"即便真的经过公式
/// 求值这条代码路径，没有任何 mod 指定公式时算出的伤害仍然与接入
/// 公式引擎之前完全一致"，不是因为走了某条特殊的空实现快捷路径才
/// 凑巧相等。
struct DefaultOnlyFormulas;

impl DamageFormulaCatalog for DefaultOnlyFormulas {
    fn formula_for(
        &self,
        _explicit: Option<ll_core::ident::ContentIndex>,
    ) -> crate::formula::FormulaDef {
        crate::formula::FormulaDef {
            id: ll_core::ident::ContentIndex::default(),
            instructions: crate::formula::default_attack_power_instructions(),
            needs_rng: false,
        }
    }
}

#[test]
fn 全局默认公式接入公式引擎后伤害数值与接入前逐位相同() {
    // Arrange：真实经过 DamageFormulaCatalog 这条代码路径（不是
    // NoFormulas 的短路），且没有任何武器显式声明公式（NoItems 恒
    // 让 explicit_formula 为 None）。
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    let victim = spawn_named_agent(&mut world, victim_pos, 1_000);
    // 期望伤害：接入公式引擎之前的既有实现——攻击力恒等于
    // BaseStats::BASELINE.strength，无穿透，防御为零。
    let expected_damage = damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);

    // Act
    let effects = resolve_with_skills_traits_pools_items_and_formulas(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
        &NoSkills,
        &NoTraitGrants,
        &NoTraits,
        &NoResourcePools,
        &NoItems,
        &DefaultOnlyFormulas,
    );
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert
    let victim_after = world.actors.get(victim).expect("生命值远高于伤害,不会死亡");
    assert_eq!(victim_after.health, 1_000 - expected_damage);
}

#[test]
fn 幸运更高的角色暴击命中频率更高() {
    // 频率断言，不是单次结果（见任务纪律：幸运只改变判定的概率
    // 形状，不保证任意一次攻击必然暴击/不暴击，单次断言测不出这
    // 条效果，只有在足够多次独立试验上比较命中频率才能）。用固定
    // 世界种子、固定的两个幸运值，让 `world.clock` 在一段范围内
    // 变化以取得一串不同的 `DetRng` 事件计数（见 `resolve_attack`
    // 文档「暴击」一节：三元组是 `(世界种子, 实体 ID, 世界时钟)`），
    // 统计两侧「伤害超过零暴击基准值」的次数。
    // Arrange
    let trials = 3_000i64;
    // 两个幸运值代进对抗判定（被攻击者幸运取基准 0，因此净差就是
    // 攻击者一侧的修正）：见 `crate::combat::CRIT_BASE_CHECK_MODIFIER`
    // 文档「幸运怎么进式子」那张表。
    let low_luck = 5; // −23 + 5 = −18 → 9.77% 暴击率。
    let high_luck = 100; // −23 + 100 = 77，钳到上限 28 → 97.51%。
    let baseline_damage = damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);

    let (mut low_world, _low_terrain_ids) = test_world();
    let low_attacker_pos = low_world.size.wrap(5, 5);
    let low_attacker = spawn_agent_with_luck(&mut low_world, low_attacker_pos, low_luck);
    let low_victim_pos = east_of_spawn(&low_world);
    let low_victim = spawn_named_agent(&mut low_world, low_victim_pos, 1_000_000);

    let (mut high_world, _high_terrain_ids) = test_world();
    let high_attacker_pos = high_world.size.wrap(5, 5);
    let high_attacker = spawn_agent_with_luck(&mut high_world, high_attacker_pos, high_luck);
    let high_victim_pos = east_of_spawn(&high_world);
    let high_victim = spawn_named_agent(&mut high_world, high_victim_pos, 1_000_000);

    // Act：只挪动世界时钟取得不同的随机流，不真正推进回合/不
    // `apply` 任何效果——每次试验都在同一份「满血目标」上独立重
    // 打一次，伤害是否超过基准值只取决于这一次判定是否暴击。
    let mut low_crits = 0i64;
    let mut high_crits = 0i64;
    for tick in 0..trials {
        low_world.clock = Tick(tick);
        let low_effects = resolve(
            &low_world,
            &Intent::Attack {
                actor: low_attacker,
                target: low_victim,
            },
        );
        if low_effects.iter().any(
            |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > baseline_damage),
        ) {
            low_crits += 1;
        }

        high_world.clock = Tick(tick);
        let high_effects = resolve(
            &high_world,
            &Intent::Attack {
                actor: high_attacker,
                target: high_victim,
            },
        );
        if high_effects.iter().any(
            |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > baseline_damage),
        ) {
            high_crits += 1;
        }
    }

    // Assert：97.51% 暴击率的一侧命中次数应远多于 9.77% 的一侧——
    // 差距留了很大的安全边际（3000 次试验上期望值相差约 2630 次，
    // 这里只要求多过 100 次），避免二项分布的正常波动把测试变成
    // 偶发性失败。
    assert!(high_crits > low_crits + 100);
    // 两端都不是绝对：高幸运那一侧仍然打得出非暴击，低幸运那一侧
    // 仍然打得出暴击。这是「不允许绝对」在暴击这条链路上的可观察
    // 证据——旧的概率模型里幸运 200 以上是**必定**暴击。
    assert!(high_crits < trials, "顶格幸运也不该次次暴击");
    assert!(low_crits > 0, "低幸运也不该一次都暴不出来");
}

/// 造一个占位实体，站在 `pos`，除幸运外六项主属性取基准值，且
/// `race` 由调用方直接给出（不像 [`spawn_agent_with_luck`] 那样在
/// 函数体内部临时 intern 一份「反正只看数值,不看具体是哪个种族」
/// 的占位种族）——偷袭判定测试需要种族索引与授予偷袭天赋的
/// [`TraitGrantSource`] 测试替身用的是**同一个** `ContentIndex`,
/// 若各自在互不相干的 `Interner` 里各 intern 一次,两边算出的数值
/// 不保证相等（`ll_core::ident` 模块文档「不可持久化——索引依赖 mod
/// 加载顺序」），因此本函数把「种族索引哪来的」这个决定权交还给
/// 调用方,调用方在测试里只 intern 一次,两处引用同一个值。
fn spawn_agent_with_luck_and_race(
    world: &mut WorldState,
    pos: ll_core::torus::TorusPos,
    luck: i32,
    race: ContentIndex,
) -> EntityId {
    let mut interner = ll_core::ident::Interner::new();
    let profession = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats {
            luck,
            ..BaseStats::BASELINE
        },
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
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: surface_space_at(world, pos),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

/// 一个只认识固定种族索引的测试用天赋授予来源，专供偷袭判定测试
/// 使用——形状与 [`FixedRacePoolGrant`] 相同（只回答"这个种族授予
/// 哪条天赋引用"），但刻意不复用它：两者服务的测试意图不同（资源池
/// 容量钳位 vs 偷袭判定），共享同一个类型名会让两组测试的失败信息
/// 混在一起,不利于定位。
struct FixedSneakRaceGrant {
    race: ContentIndex,
    trait_id: ContentIndex,
}

impl TraitGrantSource for FixedSneakRaceGrant {
    fn granted_traits(&self, owner: ContentIndex) -> Vec<crate::traits::TraitGrant> {
        if owner == self.race {
            vec![crate::traits::TraitGrant {
                trait_id: self.trait_id,
                unlock_level: 1,
            }]
        } else {
            Vec::new()
        }
    }
}

/// 固定把 `trait_id` 映射到一条声明 [`crate::traits::RuleModifier::SneakAttack`]
/// 的 `TraitRule`——供偷袭判定测试使用。
struct FixedSneakAttackTrait {
    trait_id: ContentIndex,
    sneak_modifier: i32,
    extra_damage: i32,
}

impl TraitCatalog for FixedSneakAttackTrait {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<crate::traits::TraitRule> {
        if trait_id != self.trait_id {
            return None;
        }
        Some(crate::traits::TraitRule {
            granted_skills: Vec::new(),
            granted_resource_pools: Vec::new(),
            rule_modifiers: vec![crate::traits::TypedRuleModifier {
                modifier_type: None,
                modifier: crate::traits::RuleModifier::SneakAttack {
                    sneak_modifier: self.sneak_modifier,
                    extra_damage: self.extra_damage,
                },
            }],
        })
    }
}

#[test]
fn 有效幸运更高的攻击者偷袭触发频率更高() {
    // 频率断言，不是单次结果——理由同「幸运更高的角色暴击命中频率
    // 更高」：偷袭同样只改变判定的概率形状,不保证任意一次攻击必然
    // 触发/不触发。`extra_damage` 故意取得远大于暴击单独能放大的
    // 上限（基准伤害 10，暴击最多放大到 15，见
    // `CRIT_DAMAGE_MULTIPLIER_PERMILLE` 文档）——`sneak_threshold`
    // 因此只可能被「偷袭真的触发」跨过，不会被暴击单独触发,统计
    // 频率时不需要额外剔除暴击的贡献,即使高幸运一侧的暴击也更频繁
    // （同一个 `effective_luck` 两条判定都读）。
    // Arrange：天赋自己那一路的修正取 0（显式声明成 0 是合法的，
    // 与「一条也没声明」不是一回事，见 `concealment_check_modifier`
    // 文档同名一节），好让这条测试**只**观察幸运那一路的贡献。
    // 被攻击者的意志取基准（察觉修正 0），因此净差就等于攻击者的
    // 幸运点数。
    let trials = 3_000i64;
    let low_luck = 5; // 净差 +5 → 62.20% 触发率。
    let high_luck = 40; // 净差 +40，钳到上限 28 → 97.51% 触发率。
    let per_point = 0;
    let extra_damage = 1_000;
    let baseline_damage = damage_after_defense(BaseStats::BASELINE.strength, 0, Penetration::NONE);
    let sneak_threshold = baseline_damage + 100;

    let mut interner = ll_core::ident::Interner::new();
    let race =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:rogue").expect("合法标识符"));
    let trait_id = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:sneak_attack").expect("合法标识符"));
    let race_traits = FixedSneakRaceGrant { race, trait_id };
    let traits = FixedSneakAttackTrait {
        trait_id,
        sneak_modifier: per_point,
        extra_damage,
    };

    let (mut low_world, _low_terrain_ids) = test_world();
    let low_attacker_pos = low_world.size.wrap(5, 5);
    let low_attacker =
        spawn_agent_with_luck_and_race(&mut low_world, low_attacker_pos, low_luck, race);
    let low_victim_pos = east_of_spawn(&low_world);
    let low_victim = spawn_named_agent(&mut low_world, low_victim_pos, 1_000_000);

    let (mut high_world, _high_terrain_ids) = test_world();
    let high_attacker_pos = high_world.size.wrap(5, 5);
    let high_attacker =
        spawn_agent_with_luck_and_race(&mut high_world, high_attacker_pos, high_luck, race);
    let high_victim_pos = east_of_spawn(&high_world);
    let high_victim = spawn_named_agent(&mut high_world, high_victim_pos, 1_000_000);

    // Act：只挪动世界时钟取得不同的随机流,理由同「幸运更高的角色
    // 暴击命中频率更高」。
    let mut low_sneaks = 0i64;
    let mut high_sneaks = 0i64;
    for tick in 0..trials {
        low_world.clock = Tick(tick);
        let low_effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
            &low_world,
            &Intent::Attack {
                actor: low_attacker,
                target: low_victim,
            },
            &NoSkills,
            &race_traits,
            &traits,
            &NoResourcePools,
            &NoItems,
            &NoFormulas,
            &NoDamageCategories,
        );
        if low_effects.iter().any(
            |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > sneak_threshold),
        ) {
            low_sneaks += 1;
        }

        high_world.clock = Tick(tick);
        let high_effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
            &high_world,
            &Intent::Attack {
                actor: high_attacker,
                target: high_victim,
            },
            &NoSkills,
            &race_traits,
            &traits,
            &NoResourcePools,
            &NoItems,
            &NoFormulas,
            &NoDamageCategories,
        );
        if high_effects.iter().any(
            |effect| matches!(effect, Effect::Damage { amount, .. } if *amount > sneak_threshold),
        ) {
            high_sneaks += 1;
        }
    }

    // Assert：97.51% 触发率的一侧命中次数应远多于 62.20% 的一侧
    // ——差距留了很大的安全边际（3000 次上期望值相差约 1059 次，
    // 这里只要求多过 100 次），理由同「幸运更高的角色暴击命中频率
    // 更高」。
    assert!(high_sneaks > low_sneaks + 100);
    // 两端都不封顶：高的那一侧不是必定触发，低的那一侧也打得出。
    assert!(high_sneaks < trials, "顶格修正也不该次次触发");
    assert!(low_sneaks > 0, "低幸运也不该一次都触发不了");
}

#[test]
fn 偷袭触发时伤害恰好高出一份追加伤害且两端都不封顶() {
    // 精确数值断言，不是频率断言——利用暴击判定/伤害公式骰子的
    // `DetRng` 三元组 `(世界种子, 实体 ID, 世界时钟)` 完全不依赖
    // 调用方传入的 `race_traits`/`traits` 目录这一点：同一个世界、
    // 同一个攻击者、同一个目标、同一个 `world.clock`,两次调用
    // 之间暴击是否命中、伤害公式的骰子抽出什么值逐位相同,唯一的
    // 差异是这次传入的天赋目录有没有声明偷袭——两次的伤害差因此
    // 必须精确等于 `extra_damage`,不多不少（若偷袭判定读到了不该
    // 读的东西,或者额外消费了一次随机数导致后续判定错位,这条精确
    // 断言会立刻暴露）。
    //
    // 「这一轮到底触发没触发」不靠猜:同一个时钟下带天赋与不带天赋
    // 各打一次,差值只可能是 0 或恰好一份 `extra_damage`。本条逐轮
    // 断言这条不变式,并统计触发次数——判定系统迁移之后触发不再
    // 钳得住 100%（幸运 50 + 天赋 20 = 70 越过上限被钳回 28，对
    // 基准目标是 97.51%），因此断言从「精确等式」改成「不变式 +
    // 两端不封顶」。
    // Arrange
    let trials = 400i64;
    let luck = 50;
    let per_point = 20;
    let extra_damage = 37;
    let (mut world, _terrain_ids) = test_world();
    let attacker_pos = world.size.wrap(5, 5);
    let mut interner = ll_core::ident::Interner::new();
    let race =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:rogue").expect("合法标识符"));
    let trait_id = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:sneak_attack").expect("合法标识符"));
    let attacker = spawn_agent_with_luck_and_race(&mut world, attacker_pos, luck, race);
    let victim_pos = east_of_spawn(&world);
    let victim = spawn_named_agent(&mut world, victim_pos, 1_000_000);
    let race_traits = FixedSneakRaceGrant { race, trait_id };
    let traits = FixedSneakAttackTrait {
        trait_id,
        sneak_modifier: per_point,
        extra_damage,
    };

    let attack = |world: &WorldState,
                  race_traits: &dyn TraitGrantSource,
                  traits: &dyn TraitCatalog|
     -> i32 {
        let effects = resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
            world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
            &NoSkills,
            race_traits,
            traits,
            &NoResourcePools,
            &NoItems,
            &NoFormulas,
            &NoDamageCategories,
        );
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Damage { amount, .. } => Some(*amount),
                _ => None,
            })
            .expect("攻击必然产出一条伤害效果")
    };

    // Act：只挪动世界时钟取得不同的随机流，不 `apply` 任何效果。
    let mut sneaks = 0i64;
    for tick in 0..trials {
        world.clock = Tick(tick);
        let damage_without_sneak = attack(&world, &NoTraitGrants, &NoTraits);
        let damage_with_sneak = attack(&world, &race_traits, &traits);

        // 不变式：带天赋那一场只可能与不带天赋的那一场相等，或者
        // 恰好高出一份 extra_damage——不多不少。
        let gap = damage_with_sneak - damage_without_sneak;
        assert!(
            gap == 0 || gap == extra_damage,
            "偷袭要么不触发、要么恰好追加 {extra_damage} 点，实得 {gap}"
        );
        if gap == extra_damage {
            sneaks += 1;
        }
    }

    // Assert：两端都不封顶。97.51% 的触发率在 400 轮上期望约 390 次
    // 触发、约 10 次落空，两条断言各留了足够的余量。
    assert!(
        sneaks > trials / 2,
        "顶格修正的偷袭应当频繁触发（{sneaks} / {trials}）"
    );
    assert!(
        sneaks < trials,
        "顶格修正也不该必定触发（{sneaks} / {trials}）"
    );
}

#[test]
fn 潜行把偷袭触发率抬到很高但仍然不是必定触发() {
    // 本条钉死的是本批次去掉的那条「必定成功」。潜行此前是偷袭
    // 判定的一条**直通**（`Some(rule) if attacker.stealthed`），
    // 与项目所有者「不允许绝对」直接冲突；现在它是判定里的一个
    // 修正（一整颗骰子，见 `crate::combat::STEALTH_SNEAK_MODIFIER`）。
    //
    // 天赋自己那一路的修正取**半颗骰子** 9——`CHECK_DICE` 文档
    // 「为什么是 3 颗」算的就是这道题：`19 + 9 = 28` 恰好等于修正
    // 上限，不触发钳制，对一个基准目标是 `97.51%`。不潜行时只剩
    // 那半颗骰子，`72.18%`。两个数都不是 0 也不是 1。
    //
    // 「有没有触发」不靠猜伤害数值：同一个世界、同一个时钟各打两
    // 次，一次带天赋目录、一次喂 `NoTraits`——后者就是这一下去掉
    // 偷袭之后的基准（暴击流与骰子流的三元组都只含
    // `(种子, 实体, 时钟)`，两次逐位相同），差值只可能是 0 或恰好
    // 一份 `extra_damage`。
    // Arrange
    let trials = 400i64;
    let luck = 0;
    let half_die = CHECK_DICE.half_die() as i32;
    let extra_damage = 37;
    let (mut world, _terrain_ids) = test_world();
    let attacker_pos = world.size.wrap(5, 5);
    let mut interner = ll_core::ident::Interner::new();
    let race =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:rogue").expect("合法标识符"));
    let trait_id = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:sneak_attack").expect("合法标识符"));
    let attacker = spawn_agent_with_luck_and_race(&mut world, attacker_pos, luck, race);
    let victim_pos = east_of_spawn(&world);
    let victim = spawn_named_agent(&mut world, victim_pos, 1_000_000);
    let race_traits = FixedSneakRaceGrant { race, trait_id };
    let traits = FixedSneakAttackTrait {
        trait_id,
        sneak_modifier: half_die,
        extra_damage,
    };

    let attack = |world: &WorldState,
                  race_traits: &dyn TraitGrantSource,
                  traits: &dyn TraitCatalog|
     -> i32 {
        resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
            world,
            &Intent::Attack {
                actor: attacker,
                target: victim,
            },
            &NoSkills,
            race_traits,
            traits,
            &NoResourcePools,
            &NoItems,
            &NoFormulas,
            &NoDamageCategories,
        )
        .iter()
        .find_map(|effect| match effect {
            Effect::Damage { amount, .. } => Some(*amount),
            _ => None,
        })
        .expect("攻击必然产出一条伤害效果")
    };

    // Act：只挪动世界时钟取得不同的随机流，理由同「幸运更高的角色
    // 暴击命中频率更高」。本条因此仍然是**确定性**测试：同一份代码
    // 永远给出同一批结果。
    let mut visible_hits = 0i64;
    let mut stealth_hits = 0i64;
    for tick in 0..trials {
        world.clock = Tick(tick);
        let baseline = attack(&world, &NoTraitGrants, &NoTraits);

        set_stealthed(&mut world, attacker, false);
        let visible = attack(&world, &race_traits, &traits);
        set_stealthed(&mut world, attacker, true);
        let stealthed = attack(&world, &race_traits, &traits);

        for (damage, label) in [(visible, "不潜行"), (stealthed, "潜行")] {
            let gap = damage - baseline;
            assert!(
                gap == 0 || gap == extra_damage,
                "{label}那一场与基准场共用同一条暴击流，伤害差只可能是 0 或                      {extra_damage}，实得 {gap}"
            );
        }
        assert!(stealthed >= visible, "潜行只加修正，不该让偷袭更难触发");
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
        stealth_hits < trials,
        "潜行也不该必定触发——这正是本批次去掉的那条「必定成功」             （潜行 {stealth_hits} / 共 {trials} 轮）"
    );
    assert!(
        visible_hits > 0 && visible_hits < trials,
        "不潜行那一侧两端都不该封顶（{visible_hits} / 共 {trials} 轮）"
    );
}

#[test]
fn 潜行中发起攻击会破除潜行() {
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    let victim = spawn_named_agent(&mut world, victim_pos, 1_000_000);
    set_stealthed(&mut world, attacker, true);

    // Act
    let effects = resolve(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
    );

    // Assert：产出一条把攻击者潜行置假的效果，且它排在伤害之后
    // （这一下仍然算潜行中的攻击，见 resolve_attack 文档
    // 「潜行破除」一节）。
    let damage_at = effects
        .iter()
        .position(|effect| matches!(effect, Effect::Damage { .. }))
        .expect("攻击必然产出一条伤害效果");
    let break_at = effects
        .iter()
        .position(|effect| {
            matches!(
                effect,
                Effect::SetStealth {
                    actor: a,
                    stealthed: false
                } if *a == attacker
            )
        })
        .expect("潜行中的攻击应当产出一条破除潜行的效果");
    assert!(break_at > damage_at);
}

#[test]
fn 不在潜行中的攻击不产出破除潜行的效果() {
    // 反面：没有相关状态时不多产一条效果，见 resolve_attack
    // 「潜行破除」一节末尾。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    let victim = spawn_named_agent(&mut world, victim_pos, 1_000_000);

    // Act
    let effects = resolve(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
    );

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::SetStealth { .. }))
    );
}

#[test]
fn 已过期的属性修正不再叠加到有效值() {
    // Arrange：到期时刻早于当前世界时钟——惰性到期判定要求这类
    // 条目在读取时被当作已失效处理,即使它仍然留在
    // active_stat_modifiers 里没被清理（见 ActiveStatModifier 文档
    // 「惰性到期判定」一节）。
    let mut interner = ll_core::ident::Interner::new();
    let source =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
    let modifiers = std::collections::BTreeMap::from([(
        AttributeKind::Strength,
        std::collections::BTreeMap::from([(
            source,
            ActiveStatModifier {
                delta: 20,
                expires_at: Tick(5),
            },
        )]),
    )]);

    // Act
    let derived = derive_stats(
        BaseStats::BASELINE,
        &modifiers,
        &std::collections::BTreeMap::new(),
        &NoItems,
        Tick(5),
    );

    // Assert：世界时钟已达到 expires_at,回落到裸值（BASELINE 力量
    // 为 10）,不叠加 delta。
    assert_eq!(derived.attribute(AttributeKind::Strength), 10);
}

#[test]
fn 温度这一路没接时与旧入口逐位等价() {
    // `derive_stats` 是 `derive_stats_at(..., TEMPERATE_BASELINE)`
    // 的薄封装，这条测试是那句话的机器检查——黄金基准回放走的正是
    // 不带任何目录的 `resolve`，等价一旦破了，摘要就会变。
    // Arrange
    let mut interner = ll_core::ident::Interner::new();
    let source =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
    let modifiers = std::collections::BTreeMap::from([(
        AttributeKind::Strength,
        std::collections::BTreeMap::from([(
            source,
            ActiveStatModifier {
                delta: 4,
                expires_at: Tick(100),
            },
        )]),
    )]);

    // Act
    let legacy = derive_stats(
        BaseStats::BASELINE,
        &modifiers,
        &std::collections::BTreeMap::new(),
        &NoItems,
        Tick(5),
    );
    let explicit = derive_stats_at(
        BaseStats::BASELINE,
        &modifiers,
        &std::collections::BTreeMap::new(),
        &NoItems,
        Tick(5),
        Temperature::TEMPERATE_BASELINE,
    );

    // Assert
    assert_eq!(legacy, explicit);
    assert_eq!(legacy.attribute(AttributeKind::Strength), 14);
}

#[test]
fn 极寒环境削弱力量且只削弱力量() {
    // 惩罚必须落在一个保证被 resolve_attack 读到的量上（力量），
    // 且不该顺手污染别的属性或护甲。
    // Arrange
    let empty = std::collections::BTreeMap::new();

    // Act
    let warm = derive_stats_at(
        BaseStats::BASELINE,
        &empty,
        &std::collections::BTreeMap::new(),
        &NoItems,
        Tick(0),
        Temperature::TEMPERATE_BASELINE,
    );
    let frozen = derive_stats_at(
        BaseStats::BASELINE,
        &empty,
        &std::collections::BTreeMap::new(),
        &NoItems,
        Tick(0),
        Temperature(-120),
    );

    // Assert
    assert!(frozen.attribute(AttributeKind::Strength) < warm.attribute(AttributeKind::Strength));
    for kind in [
        AttributeKind::Dexterity,
        AttributeKind::Constitution,
        AttributeKind::Intelligence,
        AttributeKind::Willpower,
        AttributeKind::Charisma,
        AttributeKind::Luck,
    ] {
        assert_eq!(
            frozen.attribute(kind),
            warm.attribute(kind),
            "{kind:?} 不该被暴露惩罚牵连"
        );
    }
    assert_eq!(frozen.armor(), warm.armor());
}

#[test]
fn 不同来源的属性修正在生效值上求和而非互相覆盖() {
    // 规则①「不同效果能叠加」在 derive_stats 这一层的直接验证：
    // 两个不同来源（source_a、source_b）各自给同一属性 +5、+7，
    // 有效值必须是 base + 5 + 7，不是只看到其中一条。
    // Arrange
    let mut interner = ll_core::ident::Interner::new();
    let source_a =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
    let source_b = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:blessing").expect("合法标识符"));
    let modifiers = std::collections::BTreeMap::from([(
        AttributeKind::Strength,
        std::collections::BTreeMap::from([
            (
                source_a,
                ActiveStatModifier {
                    delta: 5,
                    expires_at: Tick(100),
                },
            ),
            (
                source_b,
                ActiveStatModifier {
                    delta: 7,
                    expires_at: Tick(100),
                },
            ),
        ]),
    )]);

    // Act
    let derived = derive_stats(
        BaseStats::BASELINE,
        &modifiers,
        &std::collections::BTreeMap::new(),
        &NoItems,
        Tick(0),
    );

    // Assert：10（base） + 5 + 7 = 22，两条修正都参与了求和。
    assert_eq!(derived.attribute(AttributeKind::Strength), 22);
}

#[test]
fn 一条来源过期后另一条来源的修正仍然独立生效() {
    // 规则②③强调「各条修正各自到期」——这里验证的正是这一点：
    // source_a 已过期，source_b 未过期，聚合结果应只包含 source_b。
    // Arrange
    let mut interner = ll_core::ident::Interner::new();
    let source_a =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:brace").expect("合法标识符"));
    let source_b = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:blessing").expect("合法标识符"));
    let modifiers = std::collections::BTreeMap::from([(
        AttributeKind::Strength,
        std::collections::BTreeMap::from([
            (
                source_a,
                ActiveStatModifier {
                    delta: 5,
                    expires_at: Tick(10),
                },
            ),
            (
                source_b,
                ActiveStatModifier {
                    delta: 7,
                    expires_at: Tick(100),
                },
            ),
        ]),
    )]);

    // Act：世界时钟已经越过 source_a 的到期时刻，但仍早于 source_b。
    let derived = derive_stats(
        BaseStats::BASELINE,
        &modifiers,
        &std::collections::BTreeMap::new(),
        &NoItems,
        Tick(10),
    );

    // Assert：只有 source_b 的 +7 参与求和，source_a 已被过滤。
    assert_eq!(derived.attribute(AttributeKind::Strength), 17);
}

#[test]
fn 未具名目标被击杀时不产生历史事件记录() {
    // 与上一条对照：victim 从未被"记住"（remembered_id 恒
    // None）——分级判据要求 victim 已具名才产出完整记录（见
    // append_kill_history 文档「触发判据」一节），这里验证「不产出
    // 完整记录」也是真实生效的分支，不是恰好每次都触发。决策一
    // 落地后，这类击杀改为产出聚合计数而不是"什么都不产生"——那条
    // 断言由下面 未具名目标被击杀时按生物类型归并计数加一 单独
    // 覆盖，这里只关注"没有完整记录"这一件事。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    let victim = world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos: victim_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: 1,
        affiliations: Vec::new(),
        wallet: 0,
        profession: ContentIndex::default(),
        goals: Vec::new(),
        race: ContentIndex::default(),
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: surface_space_at(&world, victim_pos),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    });

    // Act
    let effects = resolve(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
    );
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert：目标依旧真的死了，但没有产生历史事件——分级判据把
    // 「击杀发生」与「值不值得记录」分开，两者不能混为一谈。
    assert!(world.actors.get(victim).is_none());
    assert!(world.history.is_empty());
}

#[test]
fn 未具名目标被击杀时按生物类型归并计数加一() {
    // 决策一端到端验证：杀死一个无名单位（remembered_id 恒
    // None）——从 Intent::Attack 一路到 apply,断言 world.kill_counts
    // 里对应 race 的计数恰好 +1,且没有产生完整历史事件（两件事
    // 同时成立,互不替代）。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    let mut interner = ll_core::ident::Interner::new();
    let goblin_race = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));
    let victim = world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos: victim_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: 1,
        affiliations: Vec::new(),
        wallet: 0,
        profession: ContentIndex::default(),
        goals: Vec::new(),
        race: goblin_race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: surface_space_at(&world, victim_pos),
        mod_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    });

    // Act
    let effects = resolve(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
    );
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert
    assert!(world.actors.get(victim).is_none());
    assert!(world.history.is_empty());
    assert_eq!(world.kill_counts.get(&goblin_race), Some(&1));
}

#[test]
fn 具名目标被击杀时按生物类型归并计数加一() {
    // 与「未具名目标被击杀时按生物类型归并计数加一」对照,同时与
    // 「近战攻击致死已具名目标后历史事件记录着近战死因」互补——
    // 后者已经单独证明了具名死者仍会产出完整历史记录,本测试只
    // 补上另一半：项目所有者裁定否决了决策一原有的互斥设计（「一
    // 起计算,就是杀了 10 只」,见 append_kill_history 文档「决策二」
    // 一节）之后,具名死者的击杀现在也照常累加聚合计数,不再因为
    // 已经产出完整记录就被排除在计数之外。
    // Arrange
    let (mut world, _terrain_ids) = test_world();
    let attacker = spawn_agent(&mut world);
    let victim_pos = east_of_spawn(&world);
    let victim = spawn_named_agent(&mut world, victim_pos, 1);
    let victim_race = world.actors.get(victim).expect("刚生成必然存在").race;

    // Act
    let effects = resolve(
        &world,
        &Intent::Attack {
            actor: attacker,
            target: victim,
        },
    );
    for effect in &effects {
        crate::apply::apply(&mut world, effect);
    }

    // Assert
    assert_eq!(world.kill_counts.get(&victim_race), Some(&1));
}

/// 一个只认识固定种族索引的测试用天赋授予来源，供
/// [`resource_pool_usable`] 的钳位测试使用——理由同本文件其余
/// `Fake*` 测试替身。
struct FixedRacePoolGrant {
    race: ContentIndex,
    trait_id: ContentIndex,
}

impl TraitGrantSource for FixedRacePoolGrant {
    fn granted_traits(&self, owner: ContentIndex) -> Vec<crate::traits::TraitGrant> {
        if owner == self.race {
            vec![crate::traits::TraitGrant {
                trait_id: self.trait_id,
                unlock_level: 1,
            }]
        } else {
            Vec::new()
        }
    }
}

/// 固定把 `trait_id` 映射到一条授予 `pool` 某个固定容量的
/// `TraitRule`——供 [`resource_pool_usable`] 的钳位测试使用。
struct FixedPoolCapacity {
    trait_id: ContentIndex,
    pool: ContentIndex,
    capacity: u32,
}

impl TraitCatalog for FixedPoolCapacity {
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<crate::traits::TraitRule> {
        if trait_id != self.trait_id {
            return None;
        }
        Some(crate::traits::TraitRule {
            granted_skills: Vec::new(),
            granted_resource_pools: vec![crate::resource_pool::ResourcePoolGrant {
                pool: self.pool,
                capacity: crate::resource_pool::CapacityFormula::Fixed(self.capacity),
            }],
            rule_modifiers: Vec::new(),
        })
    }
}

#[test]
fn 容量从十降到五时存储值八读出来被钳位为五而存储本身不改写() {
    // 直接验收「容量变化时读时钳位,不主动改写存储值」
    // （`resource-pools-and-rest.md` 三节）：先构造一个天赋只授予
    // 5 点容量（模拟"容量已经从 10 降到 5"这一刻），但
    // agent.resource_pools 里存储的当前值仍是掉容量之前留下的 8——
    // usable 必须被钳位为 5,而 agent.resource_pools 这份存储数据
    // 本身完全不受这次读取影响。
    // Arrange
    let (mut world, _ids) = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = ll_core::ident::Interner::new();
    let race = world.actors.get(actor).expect("刚生成必然存在").race;
    let trait_id = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:diminished_sorcery").unwrap());
    let pool =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:sorcery_points").unwrap());
    if let Some(agent) = world.actors.get_mut(actor) {
        agent.resource_pools.insert(pool, 8);
    }
    let race_traits = FixedRacePoolGrant { race, trait_id };
    let traits = FixedPoolCapacity {
        trait_id,
        pool,
        capacity: 5,
    };

    // Act
    let agent = world.actors.get(actor).expect("刚生成必然存在");
    let usable = resource_pool_usable(
        agent,
        pool,
        &race_traits,
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        &traits,
    );

    // Assert：读出来的可用量被钳位为容量（5），不是原始存储值（8）。
    assert_eq!(usable, 5);
}

#[test]
fn 容量钳位不改写存储值本身() {
    // 与上一条测试同一份构造,断言的对象换成「存储值」而不是
    // 「读出来的可用量」——钳位只发生在读取这一刻,agent.resource_pools
    // 里的原始 8 必须原封不动,不会被这次查询悄悄砍成 5。
    // Arrange
    let (mut world, _ids) = test_world();
    let actor = spawn_agent(&mut world);
    let mut interner = ll_core::ident::Interner::new();
    let race = world.actors.get(actor).expect("刚生成必然存在").race;
    let trait_id = interner
        .intern(ll_core::ident::NamespacedId::parse("lostland:diminished_sorcery").unwrap());
    let pool =
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:sorcery_points").unwrap());
    if let Some(agent) = world.actors.get_mut(actor) {
        agent.resource_pools.insert(pool, 8);
    }
    let race_traits = FixedRacePoolGrant { race, trait_id };
    let traits = FixedPoolCapacity {
        trait_id,
        pool,
        capacity: 5,
    };

    // Act：查询一次可用量（钳位只应该发生在这次读取的返回值上）。
    let agent = world.actors.get(actor).expect("刚生成必然存在");
    let _ = resource_pool_usable(
        agent,
        pool,
        &race_traits,
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        &traits,
    );

    // Assert：存储值本身仍然是 8，没有被这次读取改写。
    assert_eq!(
        world.actors.get(actor).unwrap().resource_pools.get(&pool),
        Some(&8)
    );
}
