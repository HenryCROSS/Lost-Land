//! 端到端验证：盗贼被动两分批次——项目所有者裁定原话「被动可以分为
//! **2 种**，**不觉得可疑**，还有**查不出东西**」。两种被动落在卫兵
//! 盘查链路的**两个不同环节**，本文件各给一条经由
//! [`ll_sim::turn::TurnEngine`] 的证据，外加各自的反例。
//!
//! ```text
//! 守卫的行为树
//!   ├─ self-has-profession? "lostland:guard"
//!   ├─ nearby-actor-in-view          找到目标
//!   ├─ rng-chance (guard-inspect-chance target)  ← 被动①「不觉得可疑」
//!   └─ 产出 Intent::Inspect
//!        ↓
//!      resolve_inspect → Effect::Inspect { items_seen }  ← 被动②「查不出东西」
//! ```
//!
//! # 内容全部是现成的，本文件一条都不新造
//!
//! 两条被动都挂在 `mods/example_mod/traits.json5` 里**已经存在**的
//! `examplemod:cutpurse_training`（`examplemod:rogue` 职业 3 级解锁的
//! 职业天赋）上，本文件从磁盘装载真实 `mods/`，并跑真实的
//! `NativeBehaviorTree::guard`，不用任何内联副本——ADR 0018「玩法层
//! 内容必须能从 mod 注册，且要有真实内容为证」，与
//! `example_mod_stealth.rs`/`example_mod_guard_inspection.rs` 同一条
//! 既有纪律。
//!
//! # 反例是什么：**等级**
//!
//! `classes.json5` 里盗贼那条 `unlock_level: 3` ——同一个盗贼，2 级
//! 没有这两条被动、3 级才有。本文件全部四条测试都是「同一段代码、
//! 同一份内容、同一个几何布局，只把 `Agent.level` 从 3 改成 2」的
//! 对照，因此：
//!
//! - 把 `traits.json5` 那两条 `inspection-*` 规则修正删掉，
//! - 或者把 `native_behavior::guard_inspect_chance` 里那一乘摘掉，
//! - 或者把 `resolve_inspect` 里的藏匿判定摘掉，
//!
//! 三者任一都会让对应的对照塌成同一个数，测试立刻变红。

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
use ll_mod::modifier_type::ModifierTypeTable;
use ll_mod::native_behavior::{BehaviorRuleCatalogs, NativeBehaviorSource, NativeBehaviorTree};
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
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
use ll_sim::item::{EquipSlot, ItemStack};
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

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_stealth.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// `mods/example_mod/traits.json5` 里
/// `(register-class-trait "examplemod:rogue" "examplemod:cutpurse_training" 3)`
/// 的第三个参数。写成常量并在两个对照场景里精确使用，而不是随手写
/// 一个「够大的等级」：这个数字来自真实脚本，脚本改了本文件就该变红。
const CUTPURSE_UNLOCK_LEVEL: i32 = 3;

/// 解锁前的等级——恰好差一级，本文件全部反例的构造方式。
const BELOW_UNLOCK_LEVEL: i32 = CUTPURSE_UNLOCK_LEVEL - 1;

/// `(register-trait-inspection-concealment "examplemod:cutpurse_training" 800)`
/// 的第二个参数，同上：来自真实脚本的数字。
const CUTPURSE_CONCEAL_PERMILLE: i32 = 800;

/// 装载真实 `mods/` 一次，返回本文件断言需要的表与索引——形状照抄
/// `example_mod_stealth.rs::RealModsHandle`，只是多解析两个索引
/// （盗贼职业与扒手训练天赋）。
struct RealModsHandle {
    race: RaceTable,
    class: ClassTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    skill: SkillTable,
    resource_pool: ResourcePoolTable,
    registry: Registry,
    rogue_id: ContentIndex,
    guard_id: ContentIndex,
    sword_id: ContentIndex,
}

impl RealModsHandle {
    /// 借成结算目录束——与 `example_mod_stealth.rs` 同一形状、同一批表
    /// （本体二进制交给 `TurnEngine` 的也是这个形状）。
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
            ambient: AmbientSource::NONE,
            experience: &NoExperience,
            skill_tree: &NoSkills,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
        }
    }

    /// 四张内容表的快照，喂给行为树引擎——被动①要在**决策**那一步
    /// 生效，行为树因此必须查得到它，见
    /// `ll_mod::native_behavior::BehaviorRuleCatalogs` 文档。
    fn behavior_catalogs(&self) -> BehaviorRuleCatalogs {
        BehaviorRuleCatalogs::snapshot(&self.race, &self.class, &self.trait_def, &self.item)
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
    let mut tag_table = ll_mod::tag::TagTable::new();
    let mut damage_category = DamageCategoryTable::new();
    let mut modifier_type_table = ModifierTypeTable::new();

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
            modifier_type: &mut modifier_type_table,
            tag: &mut tag_table,
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

    // 三个 `get`（不是 `intern`）各自都是一条断言：这三条内容必须真的
    // 被真实脚本注册过，删掉任何一条本文件立刻在这里失败并点名原因,
    // 理由同 `example_mod_stealth.rs::load_real_mods` 里 `lostland:guard`
    // 那一段。
    let lookup = |raw: &str| -> ContentIndex {
        registry
            .get(&NamespacedId::parse(raw).expect("合法标识符"))
            .unwrap_or_else(|| panic!("{raw} 应当已被真实 mod 脚本注册"))
    };
    let rogue_id = lookup("examplemod:rogue");
    let guard_id = lookup("lostland:guard");
    let sword_id = lookup("examplemod:iron_sword");

    RealModsHandle {
        rogue_id,
        guard_id,
        sword_id,
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

fn placeholder_race() -> ContentIndex {
    let mut interner = Interner::new();
    interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"))
}

/// 造一个实体。`level`/`inventory`/`equipment` 由调用方给定——本文件
/// 的全部对照都靠 `level` 这一个变量拉开（见模块文档「反例是什么」）。
fn spawn_agent(
    world: &mut WorldState,
    profession: ContentIndex,
    pos: (i32, i32),
    level: i32,
    inventory: Vec<ItemStack>,
    equipment: BTreeMap<EquipSlot, ItemStack>,
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
        race: placeholder_race(),
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory,
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
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
    })
}

/// 被动②的观测场景：一个卫兵**每回合都盘查**一个身上带四件物品的
/// 盗贼，连续 `turns` 个回合，全程经由 [`TurnEngine`]，返回每一次
/// `Effect::Inspect` 各看到了几件东西。
///
/// # 为什么这一条用固定 AI 策略，不走行为树
///
/// 刻意把被动①从这条测试里摘掉：走行为树的话，盘查**发起**的次数
/// 本身就会被被动①压低，两个被动的效果会混在同一个计数里，谁也证明
/// 不了谁。固定策略「每回合必查」让盘查次数在两个对照里恒等于
/// `turns`，剩下的唯一变量就是每次查到了几件——那正是被动②。
///
/// 这仍然是完整的生产链路：`TurnEngine::advance_ai` → `perform` →
/// `resolve`（真实目录）→ `apply`，与 `example_mod_stealth.rs` 用
/// `attack_controlled` 观测偷袭是同一条既有手法。
fn items_seen_per_inspection(
    handle: &RealModsHandle,
    catalogs: &ResolveCatalogs<'_>,
    rogue_level: i32,
    turns: usize,
) -> Vec<usize> {
    let (mut world, _terrain_ids) = test_world();
    // 四件东西：两件在背包、两件穿在身上（两个不同槽位）——覆盖
    // `resolve_inspect` 拼快照的两段（先背包、后装备）。
    let mut equipment = BTreeMap::new();
    equipment.insert(EquipSlot::MAIN_HAND, ItemStack::new(handle.sword_id, 1));
    equipment.insert(EquipSlot::OFF_HAND, ItemStack::new(handle.sword_id, 1));
    let rogue = spawn_agent(
        &mut world,
        handle.rogue_id,
        (8, 5),
        rogue_level,
        vec![
            ItemStack::new(handle.sword_id, 1),
            ItemStack::new(handle.sword_id, 1),
        ],
        equipment,
    );
    let guard = spawn_agent(
        &mut world,
        handle.guard_id,
        (5, 5),
        1,
        Vec::new(),
        BTreeMap::new(),
    );

    let mut timeline = Timeline::new();
    timeline.schedule(guard, Tick(0));
    timeline.schedule(rogue, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let mut wait_input = InputState::new();
    wait_input.press(GameKey::Wait);

    let mut seen = Vec::new();
    for _ in 0..turns {
        engine.advance_ai(
            &mut world,
            rogue,
            &mut |_world, actor, controlled| Intent::Inspect {
                actor,
                target: controlled,
            },
            catalogs,
            &mut |_, effect| {
                if let Effect::Inspect { items_seen, .. } = effect {
                    seen.push(items_seen.len());
                }
            },
        );
        engine.try_player_turn(&mut world, rogue, &wait_input, catalogs, &mut |_, _| {});
    }
    seen
}

/// 硬要求一（被动②「查不出东西」）：3 级盗贼身上的东西大部分查不
/// 出来；2 级的**同一个**盗贼每一件都被看得一清二楚。
#[test]
fn 三级盗贼的扒手训练让盘查查不出东西且经由turnengine生效() {
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);
    let turns = 200;
    let carried = 4;

    // Act
    let unlocked = items_seen_per_inspection(&handle, &catalogs, CUTPURSE_UNLOCK_LEVEL, turns);
    let locked = items_seen_per_inspection(&handle, &catalogs, BELOW_UNLOCK_LEVEL, turns);

    // Assert 一：两个对照都真的各发起了 `turns` 次盘查——链路本身通了，
    // 被动②**不减少盘查次数**（那是被动①的活）。
    assert_eq!(unlocked.len(), turns, "3 级对照应当每回合都真的发起盘查");
    assert_eq!(locked.len(), turns, "2 级对照应当每回合都真的发起盘查");

    // Assert 二：反例——没有这条被动时，四件东西每次都被看全。
    assert!(
        locked.iter().all(|&count| count == carried),
        "2 级盗贼没有扒手训练，每次盘查都应当看到全部 {carried} 件：{locked:?}"
    );

    // Assert 三：有这条被动时，看到的总件数显著更少。真实脚本声明
    // 800‰，期望值因此是 20%——下面只要求「不到一半」，留了极大的
    // 安全边际（概率断言，不是单次结果断言，与 `example_mod_stealth.rs`
    // 的盘查率断言同一条既有纪律）。
    let unlocked_total: usize = unlocked.iter().sum();
    let locked_total: usize = locked.iter().sum();
    assert_eq!(locked_total, turns * carried);
    assert!(
        unlocked_total * 2 < locked_total,
        "3 级盗贼身上应当大部分东西查不出来：{unlocked_total} vs {locked_total}"
    );

    // Assert 四：**逐件**掷骰，不是「一次判定决定整份快照」——若形状
    // 是后者，每次的件数只可能是 0 或 4；这里要求真的出现过「查到了
    // 一部分」的中间结果。这条钉的是
    // `RuleModifier::InspectionConcealment` 文档「为什么是逐件掷骰」
    // 一节选定的那个形状本身。200 次盘查里一次中间结果都不出现的概率
    // 可忽略不计（单次出现中间结果的概率约 41%）。
    assert!(
        unlocked.iter().any(|&count| count > 0 && count < carried),
        "逐件掷骰应当出现过「查到了一部分」的结果：{unlocked:?}"
    );
    // 上一条断言的前提：真实脚本声明的藏匿率若是 1000‰，逐件与整份
    // 两种形状就无法区分了。写成 `const` 块而不是运行期 `assert!`
    // ——两边都是常量，clippy::assertions_on_constants 要求它在编译期
    // 判定，语义上也确实该在编译期判定（真实脚本改成 1000 时，这条
    // 测试文件应当直接编译不过，而不是等到跑起来才说话）。
    const _: () = assert!(CUTPURSE_CONCEAL_PERMILLE < 1000);
}

/// 被动①的观测场景：真实的卫兵行为树经由 [`TurnEngine`] 连续
/// 推进 `turns` 个卫兵回合，返回 (盘查次数, 移动次数)——形状与理由
/// 完全照抄 `example_mod_stealth.rs::guard_turns_with_profession`，
/// 唯一的变量换成了目标的**等级**（而不是它的潜行状态）。
///
/// 两个计数都要：只数盘查次数无法区分「被动①降低了判定成功率」与
/// 「被动①让卫兵干脆看不见你」——后者会让移动次数也一起归零。
fn guard_turns_against_rogue(
    handle: &RealModsHandle,
    catalogs: &ResolveCatalogs<'_>,
    rogue_level: i32,
    turns: usize,
) -> (usize, usize) {
    let (mut world, terrain_ids) = test_world();
    for x in 0..16 {
        for y in 0..12 {
            world
                .terrain
                .set_terrain(world.size.wrap(x, y), terrain_ids.grass);
        }
    }
    let guard = spawn_agent(
        &mut world,
        handle.guard_id,
        (5, 5),
        1,
        Vec::new(),
        BTreeMap::new(),
    );
    let rogue = spawn_agent(
        &mut world,
        handle.rogue_id,
        (8, 5),
        rogue_level,
        Vec::new(),
        BTreeMap::new(),
    );

    let mut source = NativeBehaviorSource::new(
        NativeBehaviorTree::guard(&handle.registry),
        handle.behavior_catalogs(),
        1,
    );

    let mut timeline = Timeline::new();
    timeline.schedule(guard, Tick(0));
    timeline.schedule(rogue, Tick(1));
    let mut engine = TurnEngine::new(timeline);

    let mut wait_input = InputState::new();
    wait_input.press(GameKey::Wait);

    let mut inspects = 0usize;
    let mut moves = 0usize;
    for _ in 0..turns {
        {
            let mut ai = behavior_ai_intent(&mut source);
            engine.advance_ai(
                &mut world,
                rogue,
                &mut ai,
                catalogs,
                &mut |_, effect| match effect {
                    Effect::Inspect { .. } => inspects += 1,
                    Effect::MoveTo { .. } => moves += 1,
                    _ => {}
                },
            );
        }
        engine.try_player_turn(&mut world, rogue, &wait_input, catalogs, &mut |_, _| {});
    }
    (inspects, moves)
}

/// 硬要求二（被动①「不觉得可疑」）：3 级盗贼显著更少被卫兵盘查，
/// 而且**不是**靠让卫兵看不见他，整条链路经由 [`TurnEngine`]。
///
/// 引擎里 `GUARD_INSPECT_CHANCE_PERMILLE` 是 500、扒手训练**减掉**
/// 400‰（加值类型批次把这条被动从乘数改成了概率减点数），3 级那一侧的
/// 实际触发率因此是 500 − 400 = 100‰，相差五倍。这个数与乘数模型那一版
/// （500 × 200 / 1000 = 100‰）**逐位相同**——`mods/example_mod/traits.json5`
/// 里那条声明的新值正是照着「不改变非潜行状态下的既有行为」挑的，见该
/// 文件里 cutpurse_training 的注释。下面只要求「3 级一侧严格少于 2 级
/// 一侧的一半」，留了很大的安全边际。
#[test]
fn 三级盗贼的扒手训练让卫兵不觉得可疑但仍然看得见他() {
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);
    let turns = 400;

    // Act
    let (unlocked_inspects, unlocked_moves) =
        guard_turns_against_rogue(&handle, &catalogs, CUTPURSE_UNLOCK_LEVEL, turns);
    let (locked_inspects, locked_moves) =
        guard_turns_against_rogue(&handle, &catalogs, BELOW_UNLOCK_LEVEL, turns);

    // Assert 一：盘查率真的降下来了。
    assert!(
        unlocked_inspects * 2 < locked_inspects,
        "3 级盗贼应当显著更少被盘查：3 级 {unlocked_inspects} 次，2 级 {locked_inspects} 次"
    );

    // Assert 二：反例侧真的被盘查过——这一条钉的是「链路通了」本身。
    assert!(
        locked_inspects > 0,
        "2 级盗贼没有这条被动，400 回合内应当真的被盘查过，一次都没有说明链路断了"
    );

    // Assert 三：卫兵**照样看得见**他——两侧的「行动总数」都是满的
    // （每一回合要么盘查要么走近，没有落进 'wait 兜底分支）。若被动①
    // 被误接成「改视野」，3 级那一侧会整体退化成 `Intent::Wait`，这条
    // 断言立刻变红。与 `example_mod_stealth.rs` 对潜行的同一条断言
    // 同一个用意。
    assert_eq!(
        unlocked_inspects + unlocked_moves,
        turns,
        "3 级盗贼照样应当被 nearby-actor-in-view 找到——「不觉得可疑」不是隐身"
    );
    assert_eq!(
        locked_inspects + locked_moves,
        turns,
        "2 级盗贼每一回合都应当被卫兵盘查或走近"
    );
}

/// 硬要求三：两个被动**互不干涉**——被动①只改盘查次数、不改每次
/// 查到几件；被动②只改每次查到几件、不改盘查次数。
///
/// 前半句由 [`三级盗贼的扒手训练让卫兵不觉得可疑但仍然看得见他`] 的
/// Assert 三（行动总数恒等于 `turns`）与本条一起钉住，后半句由
/// [`三级盗贼的扒手训练让盘查查不出东西且经由turnengine生效`] 的
/// Assert 一（盘查次数恒等于 `turns`）钉住。
///
/// 本条补的是「同一个 3 级盗贼身上两条被动同时生效时，
/// `resolve_inspect` 仍然照常产出效果、照常消耗回合」——两个被动挂在
/// 同一条天赋上，若哪天有人把它们合并成一个变体、或者让被动①的乘数
/// 意外流进 `items_seen`，本条会连同上面两条一起变红。
#[test]
fn 同一条天赋上的两个被动同时生效时盘查仍然照常结算() {
    // Arrange
    let handle = load_real_mods();
    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        default_formula: ContentIndex::default(),
    };
    let catalogs = handle.catalogs(&formulas);

    // Act：固定策略「每回合必查」，因此盘查次数与被动①无关。
    let seen = items_seen_per_inspection(&handle, &catalogs, CUTPURSE_UNLOCK_LEVEL, 50);

    // Assert：五十回合、五十次盘查，一次不少——被动①没有从
    // `resolve_inspect` 那一侧漏进来。
    assert_eq!(
        seen.len(),
        50,
        "被动①只作用于「要不要发起盘查」那一次掷骰，不该影响已经发起的盘查"
    );
    // 每次看到的件数恒在 0..=4，不会因为被动②而变成负数或超出携带量。
    assert!(seen.iter().all(|&count| count <= 4));
}
