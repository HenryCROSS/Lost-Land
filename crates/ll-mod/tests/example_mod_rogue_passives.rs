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
use ll_sim::check::{CHECK_DICE, CONCEALMENT_CHECK, INSPECTION_CHECK, RollBias};
use ll_sim::craft::NoRecipes;
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::effect::Effect;
use ll_sim::experience::NoExperience;
use ll_sim::exposure::AmbientSource;
use ll_sim::intent::Intent;
use ll_sim::item::{EquipSlot, ItemStack};
use ll_sim::quest::NoQuests;
use ll_sim::rule_modifier::{
    RuleModifierEntry, check_roll_bias, concealment_check_modifier, inconspicuous_check_modifier,
};
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

/// `mods/example_mod/traits.json5` 里 `examplemod:cutpurse_training` 那条
/// `kind: "inspection-concealment"` 的 `concealment_modifier`，同上：
/// 来自真实内容的数字。
///
/// 判定系统落地批次把它从千分比 `800` 换成了**判定修正点数** `9`
/// （半颗骰子，见 `ll_sim::check::CheckDice::half_die`）。
const CUTPURSE_CONCEALMENT_MODIFIER: i64 = 9;

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
    cutpurse_id: ContentIndex,
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
            ambient: AmbientSource::NONE,
            experience: &NoExperience,
            skill_tree: &NoSkills,
            xp_curves: &FlatXpCurve::DEFAULT,
            subclass_unlocks: &ll_sim::subclass::NoSubclassUnlocks,
            // 对话这两路（对话批次 2 新增）：本条测试与对话无关，接空实现。
            dialogues: &ll_sim::dialogue::NoDialogues,
            content_ids: &ll_sim::dialogue::NoContentIds,
        }
    }

    /// 四张内容表的快照，喂给行为树引擎——被动①要在**决策**那一步
    /// 生效，行为树因此必须查得到它，见
    /// `ll_mod::native_behavior::BehaviorRuleCatalogs` 文档。
    fn behavior_catalogs(&self) -> BehaviorRuleCatalogs {
        // 副职那一路：本文件的实体 `subclasses` 恒为空，一张空表与真实
        // 副职表在这里逐位等价。
        BehaviorRuleCatalogs::snapshot(
            &self.race,
            &self.class,
            &SubclassTable::new(),
            &self.trait_def,
            &self.item,
        )
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
    let cutpurse_id = lookup("examplemod:cutpurse_training");

    RealModsHandle {
        rogue_id,
        guard_id,
        sword_id,
        cutpurse_id,
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
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
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

    // Assert 三：有这条被动时，看到的总件数显著更少。真实内容声明
    // 9 点判定修正，双方属性都是基准 10（两侧调整值 0），因此单件被
    // 看到的概率是 `3d20` 净差 −9 时的精确值 255‰——期望值约 25%。
    // 下面只要求「不到一半」，留了很大的安全边际（概率断言，不是单次
    // 结果断言，与 `example_mod_stealth.rs` 的盘查率断言同一条既有
    // 纪律）。
    //
    // 旧模型下这个数是 200‰（藏匿率 800‰ 的补）。同一档，但换成对抗
    // 判定之后它不再是一个与人无关的常数：搜身的人的意志调整值现在
    // 进了式子。
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
    // 一节选定的那个形状本身。这么多次盘查里一次中间结果都不出现的
    // 概率可忽略不计（单件被看到 255‰，四件里「既非全见也非全藏」
    // 的概率约 72%）。
    assert!(
        unlocked.iter().any(|&count| count > 0 && count < carried),
        "逐件掷骰应当出现过「查到了一部分」的结果：{unlocked:?}"
    );
    // 上一条断言的前提：真实内容声明的藏匿修正若顶到上限，逐件与整份
    // 两种形状就几乎无法区分了（顶格时单件被看到只有 21‰，四件全藏的
    // 概率高达 92%，中间结果稀少）。写成 `const` 块而不是运行期
    // `assert!`——两边都是常量，clippy::assertions_on_constants 要求它
    // 在编译期判定，语义上也确实该在编译期判定（真实内容改成顶格时，
    // 这条测试文件应当直接编译不过，而不是等到跑起来才说话）。
    //
    // 注意这条前提**换了依据**：旧模型里 `1000‰` 是「绝对藏住」，
    // 因此非它不可；新模型里根本没有绝对，顶格也只是「很难查到」，
    // 所以这里挡的是「统计上区分不开」，不是「逻辑上区分不开」。
    const _: () = assert!(CUTPURSE_CONCEALMENT_MODIFIER < CHECK_DICE.max_modifier());
}

/// 被动①的观测场景：真实的卫兵行为树经由 [`TurnEngine`] 连续
/// 推进 `turns` 个卫兵回合，返回 (盘查次数, 移动次数)——形状与理由
/// 完全照抄 `example_mod_stealth.rs::guard_turns_with_profession`，
/// 唯一的变量换成了目标的**等级**（而不是它的潜行状态）。
///
/// 两个计数都要：只数盘查次数无法区分「被动①降低了判定成功率」与
/// 「被动①让卫兵干脆看不见你」——后者会让移动次数也一起归零。
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
    /// 单独数它、而不是让它悄悄并进 [`Self::blocked`]，是 ADR 0022 反例
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

fn guard_turns_against_rogue(
    handle: &RealModsHandle,
    catalogs: &ResolveCatalogs<'_>,
    rogue_level: i32,
    turns: usize,
) -> GuardTally {
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
    let mut swaps = 0usize;
    let mut move_intents = 0usize;
    let mut other_intents = 0usize;
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
                rogue,
                &mut ai,
                catalogs,
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
        engine.try_player_turn(&mut world, rogue, &wait_input, catalogs, &mut |_, _| {});
    }
    GuardTally {
        inspects,
        moves,
        blocked: move_intents - moves,
        swaps,
        other_intents,
    }
}

/// 硬要求二（被动①「不觉得可疑」）：3 级盗贼显著更少被卫兵盘查，
/// 而且**不是**靠让卫兵看不见他，整条链路经由 [`TurnEngine`]。
///
/// 判定系统落地批次把这条链路换成了**对抗判定**（`3d20`，卫兵的意志
/// 调整值 vs 目标的敏捷调整值 + 各路修正）。两侧属性都是基准 10，因此：
///
/// - **2 级**（没有这条被动）：净差 0 → 卫兵赢面 486‰。旧模型是写死的
///   500‰，几乎逐字对上——两个势均力敌的人各掷一轮同样的骰子，赢面本来
///   就该接近一半。
/// - **3 级**（有扒手训练）：目标那一侧拿到 `inconspicuous_modifier: 9`
///   **外加**一条 `kind: "advantage"`（`check_context:
///   "lostland:inspection"`，本仓库第一条真实的优势声明），赢面降到
///   一成出头。
///
/// 旧模型下 3 级那一侧是 `500 − 400 = 100‰`。同一档，但这一次它是
/// 两个修正在骰子上真的算出来的，不是从一个写死的基数上减出来的。
/// 下面只要求「3 级一侧严格少于 2 级一侧的一半」，留了很大的安全边际。
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
    let unlocked = guard_turns_against_rogue(&handle, &catalogs, CUTPURSE_UNLOCK_LEVEL, turns);
    let locked = guard_turns_against_rogue(&handle, &catalogs, BELOW_UNLOCK_LEVEL, turns);

    // Assert 一：盘查率真的降下来了。
    assert!(
        unlocked.inspects * 2 < locked.inspects,
        "3 级盗贼应当显著更少被盘查：3 级 {} 次，2 级 {} 次",
        unlocked.inspects,
        locked.inspects
    );

    // Assert 二：反例侧真的被盘查过——这一条钉的是「链路通了」本身。
    assert!(
        locked.inspects > 0,
        "2 级盗贼没有这条被动，400 回合内应当真的被盘查过，一次都没有说明链路断了"
    );

    // Assert 三：卫兵**照样看得见**他——两侧的三态之和都是满的（每一
    // 回合要么盘查、要么真的挪了一格、要么撞在目标身上没挪成，没有落
    // 进 'wait 兜底分支）。若被动①被误接成「改视野」，3 级那一侧会
    // 整体退化成 `Intent::Wait`，`other_intents` 立刻非零、`accounted`
    // 立刻小于 `turns`，这两条断言一起变红。与
    // `example_mod_stealth.rs` 对潜行的同一条断言同一个用意。
    assert_eq!(
        unlocked.other_intents, 0,
        "3 级盗贼照样应当被 nearby-actor-in-view 找到——「不觉得可疑」不是隐身：{unlocked:?}"
    );
    assert_eq!(
        unlocked.accounted(),
        turns,
        "三态之和应当恰好是回合数：{unlocked:?}"
    );
    assert_eq!(
        locked.other_intents, 0,
        "2 级盗贼每一回合都应当被卫兵盘查或走近：{locked:?}"
    );
    assert_eq!(
        locked.accounted(),
        turns,
        "三态之和应当恰好是回合数：{locked:?}"
    );

    // Assert 四：第三态**真的出现过**——这是项目所有者裁定「只有玩家
    // 可以互换位置」在本文件里唯一可观测的后果。卫兵三步之内就贴到
    // 目标身边，此后每一次「走近」都撞在目标身上挪不动；本批次之前
    // 同一步会被路由成互换位置，`blocked` 恒为 0。这条断言一旦变红，
    // 说明 NPC 又能推开别人了。
    assert!(
        unlocked.blocked > 0 && locked.blocked > 0,
        "卫兵贴身后每一次走近都该撞在目标身上挪不动：3 级 {unlocked:?}，2 级 {locked:?}"
    );

    // Assert 五：卫兵一次都没有和目标换过位置。互换是一次让路，而
    // 被让路的那一方（这里是受控实体）没有做出任何决定——所有者裁定
    // 只有玩家有资格要求别人让路。这条一旦变红，说明 NPC 又能推开
    // 别人了。
    assert_eq!(
        unlocked.swaps, 0,
        "卫兵是 NPC，不该产出任何 SwapPositions：{unlocked:?}"
    );
    assert_eq!(locked.swaps, 0, "同上：{locked:?}");
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

/// 判定系统落地批次的接线证据：`RuleModifier::Advantage` 那条声明真的
/// 从 `mods/example_mod/traits.json5` 走到了消费者。
///
/// 这条与本文件其余测试互补。上面那条「三级盗贼……不觉得可疑」是**概率
/// 断言**——它会因为修正点数、优势、或者两者任意一个生效而变绿，因此
/// 单独看它证明不了「优势这一路真的接上了」。本条直接在真实装载出来的
/// 天赋表上问消费者要答案，把那一路单独钉死。
///
/// 三个变体（`RerollOnce`/`Advantage`/`Disadvantage`）此前一直挂在
/// `scripts/ci/check_field_consumers.py` 的豁免清单里，理由是「本项目
/// 没有判定/检定系统」。本批次把那三条豁免删了，本测试是删除的凭据。
#[test]
fn 扒手训练在盘查判定上真的拿到优势而在藏匿判定上没有() {
    // Arrange：装载真实 `mods/`，取扒手训练那条天赋的规则修正列表。
    let handle = load_real_mods();
    let rule = ll_sim::traits::TraitCatalog::trait_rule(&handle.trait_def, handle.cutpurse_id)
        .expect("examplemod:cutpurse_training 必须真的被装载出来");
    let entries: Vec<RuleModifierEntry> = rule
        .rule_modifiers
        .iter()
        .map(|typed| RuleModifierEntry {
            modifier_type: typed.modifier_type,
            origin: handle.cutpurse_id,
            modifier: typed.modifier.clone(),
        })
        .collect();

    // Act & Assert 一：盘查判定拿到优势。
    assert_eq!(
        check_roll_bias(&entries, INSPECTION_CHECK),
        RollBias::Advantage,
        "traits.json5 里那条 kind: \"advantage\" 没有走到消费者：{entries:?}"
    );

    // Assert 二：藏匿判定**没有**优势——两环是两条独立的被动，这条
    // 反例同时证明上一条不是「`check_roll_bias` 恒返回优势」。
    assert_eq!(
        check_roll_bias(&entries, CONCEALMENT_CHECK),
        RollBias::Normal
    );

    // Assert 三：另外两条被动的点数也是内容里那两个真实的数——本文件
    // 上方那个 `const` 与 traits.json5 若漂移，这里立刻变红。
    assert_eq!(
        concealment_check_modifier(&entries),
        Some(CUTPURSE_CONCEALMENT_MODIFIER as i32)
    );
    assert_eq!(
        i64::from(inconspicuous_check_modifier(&entries)),
        CHECK_DICE.half_die()
    );
}
