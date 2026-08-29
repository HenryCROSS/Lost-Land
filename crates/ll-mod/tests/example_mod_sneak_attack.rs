//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! 盗贼偷袭接线批次新增的脚本 API——`register-trait-sneak-attack`——
//! 真的能被 `mods/example_mod/traits.json5` 调用，且真实注册的偷袭
//! 声明真的能走真实 `resolve_attack` + `apply` 追加伤害，不能靠
//! `crates/ll-sim/src/traits.rs`/`crates/ll-sim/src/resolve.rs`/
//! `crates/ll-mod/src/script_trait_api.rs` 里的单元测试自证——ADR 0018
//! 「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，本
//! 文件是盗贼偷袭接线批次的那份证据。
//!
//! 与 `crates/ll-mod/tests/example_mod_resistance.rs` 同一套「装载整个
//! `mods/` 目录，不是只挑 `example_mod`」手法，见该文件模块文档。
//!
//! # 为什么断言的是频率，不像抗性那样断言精确数值
//!
//! 抗性测试（`example_mod_resistance.rs`）能断言精确倍率，因为两组
//! 防御方的攻击者幸运恒为零，而抗性本身不掷骰。偷袭掷骰：判定系统
//! 迁移批次之后它是一次 `3d20` 对抗判定
//! （`ll_sim::combat::sneak_attacker_modifier`），幸运越高修正越大、
//! 触发越频繁，但**两端都不封顶**——顶格幸运也打得出不触发的那一下，
//! 零幸运也打得出触发的那一下。单次采样因此测不出「幸运更高更容易
//! 偷袭」这条效果，只有频率能，理由与 `crates/ll-sim/src/resolve.rs`
//! 的 `幸运更高的角色暴击命中频率更高` 同源。
//!
//! 「这一下有没有触发偷袭」不靠猜：基准伤害是
//! `damage_after_defense(10, 0, NONE) = 10`，这一下即便暴击也只到
//! `apply_crit_multiplier(10) = 15`，而偷袭追加的是 `15` 点——因此
//! 「伤害超过没偷袭时的最大可能值」这个阈值只可能被偷袭跨过，暴击
//! 单独跨不过去。两个常量都由本文件从 `ll_sim::combat` 现算，不写死。

use std::collections::BTreeMap;
use std::path::Path;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_mod::formula::{FormulaTable, RegistryFormulas};
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::race::RaceTable;
use ll_mod::trait_def::TraitTable;
use ll_sim::combat::{Penetration, apply_crit_multiplier, damage_after_defense};
use ll_sim::damage_category::NoDamageCategories;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::item::EquipSlot;
use ll_sim::resolve::resolve_with_skills_traits_pools_items_formulas_and_damage_categories;
use ll_sim::skill::NoSkills;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_resistance.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 「幸运充足」那一侧的幸运值。
///
/// 代进偷袭判定：`50（幸运） + 9（examplemod:predatory_instinct 声明的
/// sneak_modifier，半颗骰子）= 59`，越过修正上限 `28` 被钳回上限，对
/// 一个基准目标是 `97.51%` 的触发率。对照的零幸运一侧只有那半颗骰子，
/// `72.18%`。两个数都不是 0 也不是 1——这正是本文件改用频率断言的
/// 原因，见模块文档。
const LUCKY_LUCK: i32 = 50;

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引，理由同 `example_mod_resistance.rs::RealModsHandle`。
struct RealModsHandle {
    race: RaceTable,
    trait_def: TraitTable,
    item: ItemTable,
    formula: FormulaTable,
    footpad_id: ContentIndex,
}

fn load_real_mods() -> RealModsHandle {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        race,
        trait_def,
        item,
        formula,
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
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/traits.json5 注册"))
    };

    RealModsHandle {
        footpad_id: resolve("examplemod:footpad"),
        race,
        trait_def,
        item,
        formula,
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

/// 造一个占位实体，站在 `(5, 5)`，理由同
/// `example_mod_resistance.rs::spawn_agent`——本文件额外暴露 `luck`
/// 参数（其余六项主属性/装备/种族固定），供两个攻击者各自指定不同的
/// 有效幸运。
fn spawn_agent_with_luck(
    world: &mut WorldState,
    race: ContentIndex,
    health: i32,
    luck: i32,
) -> EntityId {
    let mut interner = Interner::new();
    let profession = interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
    let pos = world.size.wrap(5, 5);
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    world.actors.spawn(Agent {
        // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
        gender: ll_world::entity::Gender::default(),
        pos,
        stats: BaseStats {
            luck,
            ..BaseStats::BASELINE
        },
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
        equipment: BTreeMap::<EquipSlot, ll_sim::item::ItemStack>::new(),
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
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    })
}

#[test]
fn 真实注册的迅足者种族幸运越高偷袭触发越频繁且两端都不封顶() {
    // 手工验证过这条会红：把 `resolve_attack` 里偷袭判定那一段整段
    // 去掉（等价于攻击者没有声明这条天赋），两个攻击者的触发次数都变成
    // 0，第一条断言立即失败——`crates/ll-sim/src/resolve.rs` 的统计
    // 频率测试与本文件的目标一致：前者验证判定本身受幸运影响、走
    // DetRng；本文件验证真实注册的 mod 天赋走的是同一条真实链路，不是
    // 只在单元测试里自证。
    // Arrange
    let handle = load_real_mods();
    let mut world = test_world();
    // 两个攻击者只差幸运一项：一个零幸运、一个 LUCKY_LUCK。两个都是
    // 迅足者（种族授予了偷袭天赋），因此「有没有声明」这一路完全相同。
    let baseline_attacker =
        spawn_agent_with_luck(&mut world, handle.footpad_id, Agent::STARTING_HEALTH, 0);
    let baseline_defender = spawn_agent_with_luck(&mut world, handle.footpad_id, 1_000_000, 0);
    let sneak_attacker = spawn_agent_with_luck(
        &mut world,
        handle.footpad_id,
        Agent::STARTING_HEALTH,
        LUCKY_LUCK,
    );
    let sneak_defender = spawn_agent_with_luck(&mut world, handle.footpad_id, 1_000_000, 0);

    let formulas = RegistryFormulas {
        formulas: &handle.formula,
        // 两个攻击者都是徒手（没有装备任何武器），恒退回全局默认公式
        // ——`formula_for` 只有在显式引用查不到时才会退回它,这里的
        // 默认值因此不会被真的用到。
        default_formula: ContentIndex::default(),
    };

    // 没偷袭时这一下最多打出多少：基准伤害走完整条减伤链路，再让暴击
    // 放大到顶。两个数都现算，不写死，见模块文档。
    let no_sneak_ceiling = apply_crit_multiplier(damage_after_defense(
        BaseStats::BASELINE.strength,
        0,
        Penetration::NONE,
    ));

    let damage = |world: &WorldState, attacker: EntityId, defender: EntityId| -> i32 {
        resolve_with_skills_traits_pools_items_formulas_and_damage_categories(
            world,
            &Intent::Attack {
                actor: attacker,
                target: defender,
            },
            &NoSkills,
            &handle.race,
            &handle.trait_def,
            &ll_sim::resource_pool::NoResourcePools,
            &handle.item,
            &formulas,
            &NoDamageCategories,
        )
        .iter()
        .find_map(|effect| match effect {
            Effect::Damage { amount, .. } => Some(*amount),
            _ => None,
        })
        .expect("攻击必然产出一条伤害效果")
    };

    // Act：只挪动世界时钟取得不同的随机流，不 `apply` 任何效果——每轮
    // 都在同一份满血目标上独立重打一次。本条因此仍是**确定性**测试。
    let trials = 400i64;
    let mut baseline_sneaks = 0i64;
    let mut lucky_sneaks = 0i64;
    for tick in 0..trials {
        world.clock = Tick(tick);
        if damage(&world, baseline_attacker, baseline_defender) > no_sneak_ceiling {
            baseline_sneaks += 1;
        }
        if damage(&world, sneak_attacker, sneak_defender) > no_sneak_ceiling {
            lucky_sneaks += 1;
        }
    }

    // Assert
    assert!(
        lucky_sneaks > baseline_sneaks,
        "幸运更高的一侧触发次数应当严格更多（{lucky_sneaks} 应大于 {baseline_sneaks}）"
    );
    assert!(
        lucky_sneaks < trials,
        "顶格修正也不该必定触发（{lucky_sneaks} / 共 {trials} 轮）"
    );
    assert!(
        baseline_sneaks > 0,
        "零幸运也不该一次都触发不了（共 {trials} 轮）"
    );
}
