//! 端到端验证：真实装载仓库里的 `mods/` 目录，证明「打怪升级、升级
//! 发点、玩家自己加点」这一整条链路**经由
//! [`ll_sim::turn::TurnEngine`]**（本体二进制 `ll-game` 驱动世界的唯一
//! 路径）真的发生——不是靠测试直接调 `ll_sim::resolve`/`ll_sim::apply`
//! 的某个专用入口自证。
//!
//! # 这一整条链路指的是
//!
//! 1. `mods/example_mod/races.json5` 的 `register-race-xp-reward`
//!    声明「杀死一只哥布林的基准经验是 15」；
//! 2. `mods/lostland/races.json5` 的同一个指令给本体三族各自声明了基准
//!    值（项目所有者裁定「最低经验 1xp、人人都给」之后新增）；
//! 3. 一次经 `TurnEngine::advance_ai` 提交的 `Intent::Attack` 打死目标，
//!    `resolve_dispatch` 的 `append_kill_experience` 按
//!    `ll_sim::experience::kill_experience` 算出最终经验；
//! 4. `apply` 侧的升级循环把经验换成等级，并按
//!    `Agent::ATTRIBUTE_POINTS_PER_LEVEL`/`SKILL_POINTS_PER_LEVEL`
//!    发点；
//! 5. 玩家经 `TurnEngine` 提交 `Intent::AllocateAttributePoint`/
//!    `Intent::LearnSkill` 把点花出去。
//!
//! # 为什么这份端到端测试非有不可
//!
//! 击杀经验的接线批次当初把 `append_kill_experience` 挂在
//! `resolve_with_skills_quests_and_experience` 这个第四层专用入口上，
//! 而生产路径走的是 `resolve_with_catalogs`——两条路从不相交，于是
//! **真正能跑起来的游戏里，击杀从来没有产出过任何经验**，全部证据都
//! 止步于集成测试直接调那个专用入口。这与 `TurnEngine` 文档记的天赋
//! 系统那次「只在测试里成立的接线」是同一类缺陷。本文件是那条缺陷被
//! 修好之后的守卫。
//!
//! # 反例守卫
//!
//! [`经验目录从回合引擎摘掉后哥布林只值保底的一点经验`] 是那份守卫：
//! 同一段场景、同一个 `TurnEngine`，只把经验目录换成
//! [`ll_sim::experience::NoExperience`]，`examplemod:goblin` 声明的
//! 15 点基准值立刻查不到，经验塌回
//! [`ll_sim::experience::MIN_KILL_XP`]。谁把经验目录从 `TurnEngine`
//! 那条链路上摘掉（比如把 `resolve_dispatch` 里那行
//! `append_kill_experience` 删掉，或把 `ResolveCatalogs::experience`
//! 改回不接），正向测试就会拿到与本条完全一样、甚至更少的结果而变红。
//!
//! # 本文件不验收什么
//!
//! **玩家怎么按键提交加点。** `ll_sim::intent::intent_from_input` 至今
//! 只映射 `Move`/`Wait` 两个意图——`PickUp`/`Drop`/`Equip`/`Unequip`/
//! `Rest`/`Loot`/`Use`/`Inspect`/`ToggleStealth`/`Craft` 与本批次新增的
//! 两个加点意图全都没有绑定按键，输入映射层整体尚未展开。本文件里
//! 「AI 策略直接返回那条意图」是最小占位提交路径，不假装加点在真实
//! 玩法里已经可以用键盘够到。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::race::RaceTable;
use ll_mod::skill::SkillTable;
use ll_mod::xp_curve::{RegistryXpCurves, XpCurveBindings, XpCurveTable};
use ll_sim::catalogs::ResolveCatalogs;
use ll_sim::craft::NoRecipes;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::experience::{ExperienceCatalog, MIN_KILL_XP, NoExperience, kill_experience};
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::quest::NoQuests;
use ll_sim::skill_overview::SkillTreeCatalog;
use ll_sim::timeline::Timeline;
use ll_sim::turn::TurnEngine;
use ll_world::entity::{Agent, AttributeKind, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `turn_engine_catalogs.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 被击杀者的起始生命——1 点，保证一次普通攻击必然打死它。本文件
/// 验收的是「击杀之后发生了什么」，不是「多少伤害算致死」（那是
/// `turn_engine_catalogs.rs` 的范围）。
const VICTIM_HEALTH: i32 = 1;

// 本文件只关心经验/技能树/曲线那三路，其余目录一律接空实现——与
// `ResolveCatalogs::empty()` 里各路空实现逐字同源。
const NO_RACE_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_CLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_SUBCLASS_TRAITS: ll_sim::traits::NoTraitGrants = ll_sim::traits::NoTraitGrants;
const NO_TRAITS: ll_sim::traits::NoTraits = ll_sim::traits::NoTraits;
const NO_POOLS: ll_sim::resource_pool::NoResourcePools = ll_sim::resource_pool::NoResourcePools;
const NO_FORMULAS: ll_sim::formula::NoFormulas = ll_sim::formula::NoFormulas;
const NO_ITEMS: ll_sim::item::NoItems = ll_sim::item::NoItems;
const NO_EXPERIENCE: NoExperience = NoExperience;

/// 一次真实装载的产物——只留下本文件断言需要的那几张表与索引。
struct RealModsHandle {
    race: RaceTable,
    skill: SkillTable,
    xp_curves: XpCurveTable,
    xp_curve_bindings: XpCurveBindings,
    default_xp_curve: ContentIndex,
    goblin: ContentIndex,
    human: ContentIndex,
    frostbolt: ContentIndex,
}

impl RealModsHandle {
    /// 把真实装载出来的表借成结算目录束——本体二进制
    /// （`ll_game::content::RuntimeCatalogs::as_resolve_catalogs`）交给
    /// `TurnEngine` 的是同一个形状、同一批表。
    ///
    /// `experience` 由调用方传入，正是为了让反例守卫能在**其余一切
    /// 都不变**的前提下只把这一路换成空实现。
    fn catalogs<'a>(
        &'a self,
        experience: &'a dyn ExperienceCatalog,
        curves: &'a RegistryXpCurves<'a>,
    ) -> ResolveCatalogs<'a> {
        ResolveCatalogs {
            skills: &self.skill,
            quests: &NoQuests,
            race_traits: &NO_RACE_TRAITS,
            class_traits: &NO_CLASS_TRAITS,
            subclass_traits: &NO_SUBCLASS_TRAITS,
            trait_defs: &NO_TRAITS,
            pools: &NO_POOLS,
            items: &NO_ITEMS,
            formulas: &NO_FORMULAS,
            damage_categories: &NoDamageCategories,
            recipes: &NoRecipes,
            ambient: AmbientSource::NONE,
            experience,
            skill_tree: &self.skill,
            xp_curves: curves,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
        }
    }

    /// 真实曲线目录——与本体二进制 `RuntimeCatalogs::new` 里那一份
    /// 逐字段同构。
    fn real_curves(&self) -> RegistryXpCurves<'_> {
        RegistryXpCurves {
            curves: &self.xp_curves,
            bindings: &self.xp_curve_bindings,
            default_curve: self.default_xp_curve,
        }
    }
}

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        default_xp_curve_id: default_xp_curve,
        skill,
        race,
        xp_curve,
        xp_curve_bindings,
        ..
    } = session;
    for id in ["examplemod:self", "lostland:self"] {
        let parsed = NamespacedId::parse(id).unwrap();
        let status = report
            .entries
            .iter()
            .find(|(entry, _)| *entry == parsed)
            .map(|(_, status)| status);
        assert_eq!(
            status,
            Some(&LoadStatus::Loaded),
            "{id} 必须成功加载，否则下面的索引解析毫无意义"
        );
    }

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被真实 mods/ 注册"))
    };

    RealModsHandle {
        goblin: resolve("examplemod:goblin"),
        human: resolve("lostland:human"),
        frostbolt: resolve("examplemod:frostbolt"),
        default_xp_curve,
        race,
        skill,
        xp_curves: xp_curve,
        xp_curve_bindings,
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

/// 造一个占位实体：本文件只关心位置、等级、生命、种族四项。
fn spawn_agent(
    world: &mut WorldState,
    pos: (i32, i32),
    level: i32,
    health: i32,
    race: ContentIndex,
) -> EntityId {
    let mut interner = Interner::new();
    let placeholder = interner.intern(NamespacedId::parse("lostland:tester").expect("合法"));
    let agent_pos = world.size.wrap(pos.0, pos.1);
    let (zone, _) = world.terrain.layout().tile_to_zone(agent_pos);
    world.actors.spawn(Agent {
        pos: agent_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health,
        affiliations: Vec::new(),
        wallet: 0,
        profession: placeholder,
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
        creature_kind: Some(race),
        spawned_at: Tick(0),
        remembered_id: None,
        level,
        experience: 0,
        // 门槛远高于本文件任何一次击杀能给的经验——正向测试要断言的
        // 是「经验真的加上了」，把升级这件事从那条断言里隔离出去。
        // 需要真的升级的那条测试（升级发点）自己把门槛调低。
        xp_to_next_level: 10_000,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

/// 一次击杀场景的产物——击杀者结算完之后的那份 `Agent` 快照。
struct KillOutcome {
    experience: i64,
    level: i32,
    unspent_attribute_points: u32,
    unspent_skill_points: u32,
}

/// 跑一场「击杀者经由 `TurnEngine` 提交恰好一次 `Intent::Attack` 打死
/// 目标」，返回击杀者结算后的状态。
///
/// # 为什么恰好只结算一次
///
/// 手法与 `example_mod_crafting.rs::craft_via_turn_engine` 完全相同：
/// 击杀者排在 `Tick(0)`、旁观者（`controlled`）排在 `Tick(1)`，
/// `advance_ai` 先弹出击杀者结算一次，下一次弹出的是 `controlled`，
/// 于是立即返回。
fn kill_via_turn_engine(
    handle: &RealModsHandle,
    experience: &dyn ExperienceCatalog,
    killer_level: i32,
    victim_level: i32,
    victim_race: ContentIndex,
    xp_to_next_level: i64,
) -> KillOutcome {
    let mut world = test_world();
    let killer = spawn_agent(&mut world, (5, 5), killer_level, 1_000, handle.human);
    let victim = spawn_agent(&mut world, (6, 5), victim_level, VICTIM_HEALTH, victim_race);
    let bystander = spawn_agent(&mut world, (20, 20), 1, 1_000, handle.human);
    world
        .actors
        .get_mut(killer)
        .expect("刚生成")
        .xp_to_next_level = xp_to_next_level;

    let mut timeline = Timeline::new();
    timeline.schedule(killer, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let curves = handle.real_curves();
    let catalogs = handle.catalogs(experience, &curves);
    let mut intent = |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Attack {
        actor,
        target: victim,
    };
    engine.advance_ai(
        &mut world,
        bystander,
        &mut intent,
        &catalogs,
        &mut |_, _| {},
    );

    assert!(
        world.actors.get(victim).is_none(),
        "目标只有 1 点生命，一次普通攻击必然把它打死——本文件全部断言都以此为前提"
    );
    let slayer = world.actors.get(killer).expect("击杀者不会因为击杀而死");
    KillOutcome {
        experience: slayer.experience,
        level: slayer.level,
        unspent_attribute_points: slayer.unspent_attribute_points,
        unspent_skill_points: slayer.unspent_skill_points,
    }
}

#[test]
fn 同级击杀哥布林经回合引擎产出脚本声明的基准经验() {
    // ADR 0018 的正向证据：内容来自真实 mods/example_mod/races.json5
    // 的 `(register-race-xp-reward "examplemod:goblin" 15)`，结算经由
    // TurnEngine::advance_ai 这条生产路径发生。
    // Arrange
    let handle = load_real_mods();

    // Act：双方同级，等级差倍率恰好 100%，最终经验就是基准值本身。
    let outcome = kill_via_turn_engine(&handle, &handle.race, 3, 3, handle.goblin, 10_000);

    // Assert
    assert_eq!(outcome.experience, 15);
    assert_eq!(outcome.level, 3, "门槛远高于 15，这一场不应该升级");
}

#[test]
fn 经验目录从回合引擎摘掉后哥布林只值保底的一点经验() {
    // 反例守卫：同一段场景、同一个 TurnEngine，只把经验目录换成空
    // 实现——脚本声明的 15 点基准值立刻查不到，经验塌回保底的 1 点。
    // 这条与上一条的差值（15 vs 1）就是「真实 mod 内容确实被读到了」
    // 这句话的全部证据。
    // Arrange
    let handle = load_real_mods();

    // Act
    let outcome = kill_via_turn_engine(&handle, &NO_EXPERIENCE, 3, 3, handle.goblin, 10_000);

    // Assert
    assert_eq!(outcome.experience, MIN_KILL_XP);
}

#[test]
fn 杀死本体人类同样产出经验而不是零() {
    // 项目所有者裁定「有个最低经验 1xp」推翻了「本体三族是可玩种族
    // 不是猎物、刻意不声明 xp_reward」这条旧判断——本条是那次推翻在
    // 真实本体内容（mods/lostland/races.json5）上的端到端证据。
    // Arrange
    let handle = load_real_mods();

    // Act：同级，人类基准值 10。
    let outcome = kill_via_turn_engine(&handle, &handle.race, 5, 5, handle.human, 10_000);

    // Assert
    assert_eq!(outcome.experience, 10);
}

#[test]
fn 越级击杀比同级击杀经回合引擎产出更多经验() {
    // 「等级差越多给越多」这条裁定在生产路径上的证据——不是只在
    // ll_sim::experience 的单元测试里成立。
    // Arrange
    let handle = load_real_mods();

    // Act
    let same_level = kill_via_turn_engine(&handle, &handle.race, 5, 5, handle.goblin, 10_000);
    let higher = kill_via_turn_engine(&handle, &handle.race, 5, 9, handle.goblin, 10_000);
    let lower = kill_via_turn_engine(&handle, &handle.race, 5, 1, handle.goblin, 10_000);

    // Assert
    assert!(higher.experience > same_level.experience);
    assert!(lower.experience < same_level.experience);
    // 与公式逐字对齐，防止「大小关系对了但数值算错」这一档漏过去。
    assert_eq!(higher.experience, kill_experience(15, 5, 9));
    assert_eq!(lower.experience, kill_experience(15, 5, 1));
}

#[test]
fn 击杀升级后经回合引擎真的发下属性点与技能点() {
    // Arrange：把门槛压到 1，这一场击杀（至少 1 点经验）必然升一级。
    let handle = load_real_mods();

    // Act
    let outcome = kill_via_turn_engine(&handle, &handle.race, 1, 1, handle.goblin, 1);

    // Assert
    assert!(outcome.level > 1, "经验超过门槛必然升级");
    let levels_gained = u32::try_from(outcome.level - 1).expect("升级数非负");
    assert_eq!(
        outcome.unspent_attribute_points,
        levels_gained * Agent::ATTRIBUTE_POINTS_PER_LEVEL
    );
    assert_eq!(
        outcome.unspent_skill_points,
        levels_gained * Agent::SKILL_POINTS_PER_LEVEL
    );
}

/// 跑一场「玩家经由 `TurnEngine` 提交恰好一次加点/学技能意图」，返回
/// 结算后的那份 `Agent`。手法同 [`kill_via_turn_engine`]。
fn submit_via_turn_engine(
    handle: &RealModsHandle,
    prepare: &dyn Fn(&mut Agent),
    make_intent: &dyn Fn(EntityId) -> Intent,
) -> Agent {
    let mut world = test_world();
    let actor = spawn_agent(&mut world, (5, 5), 1, 1_000, handle.human);
    let bystander = spawn_agent(&mut world, (20, 20), 1, 1_000, handle.human);
    prepare(world.actors.get_mut(actor).expect("刚生成"));

    let mut timeline = Timeline::new();
    timeline.schedule(actor, Tick(0));
    timeline.schedule(bystander, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let curves = handle.real_curves();
    let catalogs = handle.catalogs(&handle.race, &curves);
    // 只提交**一次**那条意图，之后一律 `Wait`——加点/学技能是自由动作，
    // 刻意不产出 `Effect::ScheduleNext`（见 `resolve_allocate_attribute_point`
    // 文档「为什么不产出 Effect::ScheduleNext」一节），行动者因此会立刻
    // 又轮到自己。`Wait` 会正常推进它的下一次行动时刻，把这一场收束到
    // 「恰好提交了一次」。
    let mut submitted = false;
    let mut intent = |_world: &WorldState, acting: EntityId, _controlled: EntityId| {
        if submitted {
            return Intent::Wait { actor: acting };
        }
        submitted = true;
        make_intent(acting)
    };
    engine.advance_ai(
        &mut world,
        bystander,
        &mut intent,
        &catalogs,
        &mut |_, _| {},
    );
    world.actors.get(actor).expect("加点不会杀死任何人").clone()
}

#[test]
fn 玩家经回合引擎加一点力量后基础力量加一余额减一() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let after = submit_via_turn_engine(
        &handle,
        &|agent| agent.unspent_attribute_points = 2,
        &|actor| Intent::AllocateAttributePoint {
            actor,
            attribute: AttributeKind::Strength,
        },
    );

    // Assert
    assert_eq!(after.stats.strength, BaseStats::BASELINE.strength + 1);
    assert_eq!(after.unspent_attribute_points, 1);
    // 其余六项一动不动——加点只动被点名的那一项。
    assert_eq!(after.stats.dexterity, BaseStats::BASELINE.dexterity);
    assert_eq!(after.stats.luck, BaseStats::BASELINE.luck);
}

#[test]
fn 余额为零时加点整条静默失败而不是凭空加属性() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let after = submit_via_turn_engine(
        &handle,
        &|agent| agent.unspent_attribute_points = 0,
        &|actor| Intent::AllocateAttributePoint {
            actor,
            attribute: AttributeKind::Strength,
        },
    );

    // Assert
    assert_eq!(after.stats.strength, BaseStats::BASELINE.strength);
    assert_eq!(after.unspent_attribute_points, 0);
}

#[test]
fn 属性已达硬上限时加点被拒绝且点数原样保留() {
    // 「超了要拒绝而不是静默钳位」——被拒绝时点数必须**原样还在**，
    // 玩家可以改加别的属性。若 apply 侧改成钳位，本条会因为
    // unspent 从 1 掉到 0 而变红。
    // Arrange
    let handle = load_real_mods();

    // Act
    let after = submit_via_turn_engine(
        &handle,
        &|agent| {
            agent.unspent_attribute_points = 1;
            agent.stats = agent.stats.with_added(
                AttributeKind::Strength,
                BaseStats::HARD_CAP - agent.stats.strength,
            );
        },
        &|actor| Intent::AllocateAttributePoint {
            actor,
            attribute: AttributeKind::Strength,
        },
    );

    // Assert
    assert_eq!(after.stats.strength, BaseStats::HARD_CAP);
    assert_eq!(after.unspent_attribute_points, 1);
}

#[test]
fn 玩家经回合引擎花一点技能点学会真实mod声明的技能() {
    // ADR 0018 的正向证据：`examplemod:frostbolt` 来自真实
    // mods/example_mod/races.json5 的 register-skill，前置为空。
    // Arrange
    let handle = load_real_mods();
    assert!(
        handle.skill.prerequisites(handle.frostbolt).is_empty(),
        "本条测试以 frostbolt 无前置为前提"
    );

    // Act
    let after =
        submit_via_turn_engine(&handle, &|agent| agent.unspent_skill_points = 1, &|actor| {
            Intent::LearnSkill {
                actor,
                skill: handle.frostbolt,
            }
        });

    // Assert
    assert_eq!(after.unlocked_skills, vec![handle.frostbolt]);
    assert_eq!(after.unspent_skill_points, 0);
}

#[test]
fn 技能点为零时学技能整条静默失败() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let after =
        submit_via_turn_engine(&handle, &|agent| agent.unspent_skill_points = 0, &|actor| {
            Intent::LearnSkill {
                actor,
                skill: handle.frostbolt,
            }
        });

    // Assert
    assert!(after.unlocked_skills.is_empty());
}

#[test]
fn 学一个未注册的技能整条静默失败且不扣点() {
    // resolve_learn_skill 第 4 道闸门的「已注册」那半句：
    // SkillTreeCatalog::prerequisites 对未注册索引返回空列表，单看
    // 前置判定它会「前置全部满足」而被学会——那会把一个查不到定义的
    // 索引写进 unlocked_skills。
    // Arrange
    let handle = load_real_mods();
    // 一个绝不会出现在 all_skills() 里的索引：本体人类种族的索引。
    let not_a_skill = handle.human;
    assert!(
        !handle.skill.all_skills().contains(&not_a_skill),
        "本条测试以这个索引不是技能为前提"
    );

    // Act
    let after =
        submit_via_turn_engine(&handle, &|agent| agent.unspent_skill_points = 1, &|actor| {
            Intent::LearnSkill {
                actor,
                skill: not_a_skill,
            }
        });

    // Assert
    assert!(after.unlocked_skills.is_empty());
    assert_eq!(after.unspent_skill_points, 1);
}

#[test]
fn 重复学同一个技能不会再扣一点技能点() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let after = submit_via_turn_engine(
        &handle,
        &|agent| {
            agent.unspent_skill_points = 1;
            agent.unlocked_skills.push(handle.frostbolt);
        },
        &|actor| Intent::LearnSkill {
            actor,
            skill: handle.frostbolt,
        },
    );

    // Assert
    assert_eq!(after.unlocked_skills, vec![handle.frostbolt]);
    assert_eq!(after.unspent_skill_points, 1);
}
