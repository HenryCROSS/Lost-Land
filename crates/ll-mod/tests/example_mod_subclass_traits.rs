//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `subclasses.json5` 新增的 `traits` 字段真的能被内容作者声明，且注册
//! 出来的**副职**天赋真的能走 `ll_sim::resolve::resolve_with_all_catalogs`
//! 端到端放出对应技能——ADR 0018「玩法层内容必须能从 mod 注册，且要有
//! 真实 mod 内容为证」，本文件是那份证据，不能靠单元测试自证。
//!
//! 与 `example_mod_traits.rs`（种族那一路）/`example_mod_class_traits.rs`
//! （职业那一路）独立成第三个文件，理由同后者：装载的是同一份 `mods/`
//! 目录，但断言的对象是第三张表、第三条所有权路径。
//!
//! # 本文件专门守住副职那一路与前两路唯一的结构差异
//!
//! `Agent::race`/`Agent::profession` 是**单值**，`Agent::subclasses` 是
//! `Vec`——一个角色持有 N 个副职就要展开 N 路 `TraitSource`。
//! `ll_sim::traits::agent_trait_sources` 的返回类型因此从
//! `[TraitSource; 2]` 变成了 `Vec<TraitSource>`。本文件的
//! `副职来源的展开条数等于二加持有的副职数` 直接钉住这条形状，其余
//! 断言钉住「展开出来的那几路真的被聚合读到了」。
//!
//! # 三条反例
//!
//! 1. 同一个角色**不持有**这个副职时放不出技能（证明技能确实来自副职
//!    这一路，不是碰巧被别的来源也授予了一份）；
//! 2. 副职那一路来源换成 `NO_TRAIT_GRANTS` 时放不出（证明
//!    `resolve_dispatch` 里那一路接线是活的，若哪天有人把它悄悄改回空
//!    实现，本条会变红）；
//! 3. 种族/职业两路各自为空时查不到这条天赋（证明它不是从前两路漏进
//!    来的）。

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
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::effect::Effect;
use ll_sim::formula::NoFormulas;
use ll_sim::intent::Intent;
use ll_sim::item::NoItems;
use ll_sim::quest::NoQuests;
use ll_sim::resolve::resolve_with_all_catalogs;
use ll_sim::resource_pool::NoResourcePools;
use ll_sim::traits::{
    NO_TRAIT_GRANTS, TraitGrant, TraitGrantSource, TraitSource, agent_trait_sources,
    effective_traits,
};
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_class_traits.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// `mods/example_mod/subclasses.json5` 给影舞天赋填的解锁等级——1，
/// 即「拥有即生效」，副职/种族/装备/buff 的既有惯例
/// （`ll_sim::traits::TraitGrant` 文档）。写成常量而不是把 `1` 散在
/// 断言里，理由同 `example_mod_class_traits.rs` 的同名常量。
const SHADOW_DANCE_UNLOCK_LEVEL: i32 = 1;

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的索引。
struct RealModsHandle {
    class: ClassTable,
    race: RaceTable,
    skill: SkillTable,
    subclass: SubclassTable,
    trait_def: TraitTable,
    shadowdancer_id: ContentIndex,
    shadow_dance_id: ContentIndex,
    backstab_id: ContentIndex,
}

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        class,
        skill,
        subclass,
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
        shadowdancer_id: resolve("examplemod:shadowdancer"),
        shadow_dance_id: resolve("examplemod:shadow_dance"),
        backstab_id: resolve("examplemod:backstab"),
        class,
        race,
        skill,
        subclass,
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

/// 造一个占位实体，站在 `(5, 5)`，不携带任何已解锁技能。**种族与职业
/// 一律填 [`ContentIndex::default()`]**（一个既没在 `RaceTable`、也没在
/// `ClassTable` 里 `define` 过的索引）：本文件要证明的是技能来自**副职**
/// 这一路，前两路必须确实是空的，否则「技能放得出来」这条断言证明不了
/// 它来自哪一路。
fn spawn_with_subclasses(world: &mut WorldState, subclasses: Vec<ContentIndex>) -> EntityId {
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: ContentIndex::default(),
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
        subclasses,
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        mod_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: Tick(0),
        remembered_id: None,
        level: SHADOW_DANCE_UNLOCK_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

/// 用真实装载出来的几张表跑一次「使用背刺」结算——`subclass_source`
/// 与 `subclasses` 是仅有的两个变量，其余输入逐字相同。
fn use_backstab_effects(
    handle: &RealModsHandle,
    subclasses: Vec<ContentIndex>,
    subclass_source: &dyn TraitGrantSource,
) -> Vec<Effect> {
    let mut world = test_world();
    let actor = spawn_with_subclasses(&mut world, subclasses);
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
        subclass_source,
        &handle.trait_def,
        &NoResourcePools,
        &NoItems,
        &NoFormulas,
        &NoDamageCategories,
    )
}

#[test]
fn 真实注册的影舞者副职在副职表里携带影舞天赋() {
    // 这条守的是 schema 与注册表两层：`subclasses.json5` 的 `traits`
    // 数组真的被 `apply_subclasses` 解析进了 `SubclassTable`。
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .subclass
        .get(handle.shadowdancer_id)
        .expect("影舞者副职应当已被真实注册");

    // Assert
    assert_eq!(
        view.traits,
        &[TraitGrant {
            trait_id: handle.shadow_dance_id,
            unlock_level: SHADOW_DANCE_UNLOCK_LEVEL,
        }]
    );
}

#[test]
fn 影舞者副职同时还留着它的获得条件两个字段互不相干() {
    // `unlock` 与 `traits` 并存：前者回答「怎么拿到」，后者回答「拿到
    // 之后给什么」。本条顺带钉住「这个副职玩家真的拿得到」——否则
    // 上一条证明的只是一份拿不到的内容。
    // Arrange
    let handle = load_real_mods();

    // Act
    let unlock = handle.subclass.craft_unlock(handle.shadowdancer_id);

    // Assert
    assert!(
        unlock.is_some(),
        "影舞者仍应保留 mods/example_mod/subclasses.json5 里的制作计数获得条件"
    );
    assert!(
        !handle
            .subclass
            .get(handle.shadowdancer_id)
            .expect("已注册")
            .traits
            .is_empty(),
        "同一条内容上两个字段应当并存"
    );
}

#[test]
fn 副职表作为天赋授予来源能查到影舞天赋() {
    // `SubclassTable` 与 `RaceTable`/`ClassTable` 走的是同一个
    // `TraitGrantSource` trait、同一段 `effective_traits` 聚合——本条
    // 直接以副职索引为所有者调用那段聚合，证明复用是真的（ADR 0021），
    // 不是又抄了一份。
    // Arrange
    let handle = load_real_mods();
    let sources = [TraitSource::new(handle.shadowdancer_id, &handle.subclass)];

    // Act
    let result = effective_traits(&sources, SHADOW_DANCE_UNLOCK_LEVEL);

    // Assert
    assert_eq!(result, vec![handle.shadow_dance_id]);
}

#[test]
fn 只接种族与职业两路来源时影舞天赋查不到() {
    // 第三条反例：证明上一条拿到的天赋确实来自副职那一路。
    // Arrange
    let handle = load_real_mods();
    let sources = [
        TraitSource::new(ContentIndex::default(), &handle.race),
        TraitSource::new(ContentIndex::default(), &handle.class),
    ];

    // Act
    let result = effective_traits(&sources, SHADOW_DANCE_UNLOCK_LEVEL);

    // Assert
    assert!(
        result.is_empty(),
        "种族/职业两路都不该有任何天赋，实际 {result:?}"
    );
}

#[test]
fn 副职来源的展开条数等于二加持有的副职数() {
    // 副职那一路与前两路唯一的结构差异（见本文件模块文档）：
    // `agent_trait_sources` 的返回长度不再是编译期常数。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    let none = spawn_with_subclasses(&mut world, Vec::new());
    let one = spawn_with_subclasses(&mut world, vec![handle.shadowdancer_id]);

    // Act
    let without = agent_trait_sources(
        world.actors.get(none).expect("刚生成"),
        &handle.race,
        &handle.class,
        &handle.subclass,
    );
    let with = agent_trait_sources(
        world.actors.get(one).expect("刚生成"),
        &handle.race,
        &handle.class,
        &handle.subclass,
    );

    // Assert
    assert_eq!(without.len(), 2, "不持有副职时只有种族与职业两路");
    assert_eq!(with.len(), 3, "每持有一个副职就多一路");
    assert_eq!(
        with[2].owner, handle.shadowdancer_id,
        "第三路的所有者应当是副职索引本身"
    );
}

#[test]
fn 持有影舞者副职的角色能通过副职天赋放出从未解锁过的背刺() {
    // 端到端验收：`agent.unlocked_skills` 从头到尾是空的、种族与职业
    // 两路也都是空的，`resolve` 走的是真实内容文件注册出来的
    // SubclassTable/TraitTable/SkillTable 三张表——ADR 0018 要求的
    // 「真实 mod 内容为证」在副职这一路上的完整闭环。
    // Arrange
    let handle = load_real_mods();

    // Act
    let effects = use_backstab_effects(&handle, vec![handle.shadowdancer_id], &handle.subclass);

    // Assert
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "副职天赋授予的技能应当真实产出伤害效果，实际 effects={effects:?}"
    );
}

#[test]
fn 不持有影舞者副职的同一个角色放不出背刺() {
    // 第一条反例——与上一条的唯一差别是 `subclasses` 为空。
    // Arrange
    let handle = load_real_mods();

    // Act
    let effects = use_backstab_effects(&handle, Vec::new(), &handle.subclass);

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "不持有副职时不该放行技能，实际 effects={effects:?}"
    );
}

#[test]
fn 副职那一路传空来源时影舞者放不出背刺() {
    // 第二条反例，守的是接线本身而不是内容：把 `resolve` 的副职来源
    // 换成 `NO_TRAIT_GRANTS`（其余八个 `resolve_with_*` 入口传的正是
    // 它），同一个持有副职的角色就放不出背刺——若哪天有人把
    // `resolve_dispatch` 里的副职来源悄悄改回空实现，本条会变红。
    // Arrange
    let handle = load_real_mods();

    // Act
    let effects = use_backstab_effects(&handle, vec![handle.shadowdancer_id], &NO_TRAIT_GRANTS);

    // Assert
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Damage { .. })),
        "副职来源为空时不该放行技能，实际 effects={effects:?}"
    );
}
