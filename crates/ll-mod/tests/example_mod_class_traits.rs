//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-class-trait` 这个新脚本 API 真的能被
//! `mods/example_mod/classes.json5` 声明，且注册出来的**职业**天赋真的
//! 能走 `ll_sim::resolve::resolve_with_all_catalogs` 端到端放出对应
//! 技能——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod
//! 脚本为证」，本文件是那份证据，不能靠单元测试自证。
//!
//! 与 `crates/ll-mod/tests/example_mod_traits.rs`（种族那一路的同名
//! 证据）独立成两个文件：两者装载的是同一份 `mods/` 目录，但断言的
//! 对象是两张不同的表、两条不同的所有权路径，混在一个文件里会让
//! 「哪条断言在守哪一路」变得需要逐条读注释才能分辨。
//!
//! # 本文件专门守住职业那一路与种族那一路唯一的实质差异
//!
//! `unlock_level`：种族/副职/装备/buff 恒填 `1`（"拥有即生效"），职业
//! 天赋按等级曲线填（`trait-system.md` 六节）。仓库里此前全部
//! `register-race-trait` 调用都填 `1`，因此「等级不够就不生效」这条
//! 判定虽然写在 `ll_sim::traits::effective_traits` 里、也有单元测试，
//! 却从未被任何真实 mod 内容走过。本文件的两条端到端断言（2 级放不出、
//! 3 级放得出同一个技能）正是补上这一段。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::class::ClassTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::race::RaceTable;
use ll_mod::skill::SkillTable;
use ll_mod::trait_def::TraitTable;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::effect::Effect;
use ll_sim::formula::NoFormulas;
use ll_sim::intent::Intent;
use ll_sim::item::NoItems;
use ll_sim::quest::NoQuests;
use ll_sim::resolve::resolve_with_all_catalogs;
use ll_sim::resource_pool::NoResourcePools;
use ll_sim::traits::{NO_TRAIT_GRANTS, TraitGrant, TraitSource, effective_traits};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_traits.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// `mods/example_mod/classes.json5` 给盗贼天赋填的解锁等级——测试里
/// 用它算出「差一级」与「刚好够」两个等级，避免把 `2`/`3` 两个裸数字
/// 散在断言里，改脚本时只需要改这一处。
const ROGUE_TRAIT_UNLOCK_LEVEL: i32 = 3;

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引——理由同 `example_mod_traits.rs::RealModsHandle`。
struct RealModsHandle {
    class: ClassTable,
    race: RaceTable,
    skill: SkillTable,
    trait_def: TraitTable,
    rogue_id: ContentIndex,
    cutpurse_training_id: ContentIndex,
    backstab_id: ContentIndex,
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
        ..
    } = session;
    let examplemod_id = NamespacedId::parse("examplemod:self").unwrap();
    let examplemod_status = report
        .entries
        .iter()
        .find(|(id, _)| *id == examplemod_id)
        .map(|(_, status)| status);
    assert_eq!(
        examplemod_status,
        Some(&LoadStatus::Loaded),
        "examplemod 必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/ 的内容文件注册"))
    };

    RealModsHandle {
        rogue_id: resolve("examplemod:rogue"),
        cutpurse_training_id: resolve("examplemod:cutpurse_training"),
        backstab_id: resolve("examplemod:backstab"),
        class,
        race,
        skill,
        trait_def,
    }
}

fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
}

fn test_world() -> WorldState {
    let layout = test_layout();
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

/// 造一个占位实体，站在 `(5, 5)`，不携带任何已解锁技能——`profession`/
/// `level` 由调用方给出。种族一律填 [`ContentIndex::default()`]（一个
/// 从未在 `RaceTable` 里 `define` 过的索引）：本文件要证明的是技能来自
/// **职业**这一路，种族那一路必须确实是空的，否则「技能放得出来」这条
/// 断言证明不了它来自哪一路。
fn spawn_rogue(world: &mut WorldState, profession: ContentIndex, level: i32) -> EntityId {
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
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
        race: ContentIndex::default(),
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
        level,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
        home: None,
    })
}

/// 用真实装载出来的三张表跑一次「使用背刺」结算——两条端到端断言只在
/// `level` 上不同，其余输入逐字相同。
fn use_backstab_effects(handle: &RealModsHandle, level: i32) -> Vec<Effect> {
    let mut world = test_world();
    let actor = spawn_rogue(&mut world, handle.rogue_id, level);
    resolve_with_all_catalogs(
        &world,
        &Intent::UseSkill {
            actor,
            skill: handle.backstab_id,
            target: None,
        },
        &handle.skill,
        &NoQuests,
        &handle.race,
        &handle.class,
        &NO_TRAIT_GRANTS,
        &handle.trait_def,
        &NoResourcePools,
        &NoItems,
        &NoFormulas,
        &NoDamageCategories,
    )
}

#[test]
fn 真实注册的盗贼职业在职业表里携带扒手训练天赋且解锁等级不是一() {
    // 「不是 1」本身就是断言的一部分：仓库里全部 register-race-trait
    // 调用都填 1，本条守住的是「职业这一路真的能填别的值」，而不是
    // 又一条与种族无异的声明。
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .class
        .get(handle.rogue_id)
        .expect("盗贼职业应当已被真实注册");

    // Assert
    assert_eq!(
        view.traits,
        &[TraitGrant {
            trait_id: handle.cutpurse_training_id,
            unlock_level: ROGUE_TRAIT_UNLOCK_LEVEL,
        }]
    );
    assert_ne!(
        ROGUE_TRAIT_UNLOCK_LEVEL, 1,
        "本测试的意义就在于解锁等级不是种族天赋恒用的 1"
    );
}

#[test]
fn 职业表作为天赋授予来源在等级达标时才产出有效天赋() {
    // `ClassTable` 与 `RaceTable` 走的是同一个 `TraitGrantSource`
    // trait、同一段 `effective_traits` 聚合——本条直接以职业索引为
    // 所有者调用那段聚合，证明复用是真的，不是各自抄了一份。
    // Arrange
    let handle = load_real_mods();
    let sources = [TraitSource::new(handle.rogue_id, &handle.class)];

    // Act
    let below = effective_traits(&sources, ROGUE_TRAIT_UNLOCK_LEVEL - 1);
    let at = effective_traits(&sources, ROGUE_TRAIT_UNLOCK_LEVEL);

    // Assert
    assert!(below.is_empty(), "差一级时职业天赋不该生效，实际 {below:?}");
    assert_eq!(at, vec![handle.cutpurse_training_id]);
}

#[test]
fn 只接种族一路来源时盗贼的职业天赋查不到() {
    // 反例：同一个 `effective_traits`，来源换成「种族表 + 一个从未
    // 注册过的种族索引」（`spawn_rogue` 给实体填的正是这个），结果
    // 必须是空的——这证明上一条测试拿到的天赋确实来自职业那一路，
    // 不是碰巧被别的来源也授予了一份。
    // Arrange
    let handle = load_real_mods();
    let sources = [TraitSource::new(ContentIndex::default(), &handle.race)];

    // Act
    let result = effective_traits(&sources, ROGUE_TRAIT_UNLOCK_LEVEL);

    // Assert
    assert!(
        result.is_empty(),
        "种族那一路不该有任何天赋，实际 {result:?}"
    );
}

#[test]
fn 真实盗贼角色等级达标时能通过职业天赋放出从未解锁过的背刺() {
    // 端到端验收：agent.unlocked_skills 从头到尾是空的、种族那一路
    // 也是空的，`resolve` 走的是 mod 脚本真实注册出来的 ClassTable/
    // TraitTable/SkillTable 三张表——ADR 0018 要求的「真实 mod 脚本
    // 为证」在职业这一路上的完整闭环。
    // Arrange
    let handle = load_real_mods();

    // Act
    let effects = use_backstab_effects(&handle, ROGUE_TRAIT_UNLOCK_LEVEL);

    // Assert
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "盗贼职业天赋授予的技能应当真实产出伤害效果,实际 effects={effects:?}"
    );
}

#[test]
fn 真实盗贼角色差一级时放不出背刺() {
    // 与上一条只差一个等级——`TraitGrant::unlock_level` 在真实内容上
    // 真的挡住了技能，不是一个写了但永远为真的比较。
    // Arrange
    let handle = load_real_mods();

    // Act
    let effects = use_backstab_effects(&handle, ROGUE_TRAIT_UNLOCK_LEVEL - 1);

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "等级不够时职业天赋不该放行技能,实际 effects={effects:?}"
    );
}

#[test]
fn 职业那一路传空来源时盗贼放不出背刺() {
    // 第三条反例，守的是接线本身而不是内容：把 `resolve` 的职业来源
    // 换成 `NO_TRAIT_GRANTS`（其余八个既有 `resolve_with_*` 入口传的
    // 正是它），同一个 3 级盗贼就放不出背刺——证明上面那条端到端断言
    // 依赖的确实是新接的这一路来源，若哪天有人把
    // `resolve_dispatch` 里的职业来源悄悄改回空实现，本条会变红。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let actor = spawn_rogue(&mut world, handle.rogue_id, ROGUE_TRAIT_UNLOCK_LEVEL);

    // Act
    let effects = resolve_with_all_catalogs(
        &world,
        &Intent::UseSkill {
            actor,
            skill: handle.backstab_id,
            target: None,
        },
        &handle.skill,
        &NoQuests,
        &handle.race,
        &NO_TRAIT_GRANTS,
        &NO_TRAIT_GRANTS,
        &handle.trait_def,
        &NoResourcePools,
        &NoItems,
        &NoFormulas,
        &NoDamageCategories,
    );

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "职业来源为空时不该放行技能,实际 effects={effects:?}"
    );
}
