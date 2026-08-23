//! P5-B 玩法系统——验收 Demo（任务 9）。
//!
//! 运行：`cargo run --example p5_gameplay_acceptance -p ll-content`。
//!
//! 必须展示的四件事（P5-B 计划任务 9 原文）：
//!
//! 1. **职业与技能树可用**——demo 角色选择职业（战士），解锁一条有
//!    分支的技能树路径（`strike` 同时解锁 `power_strike`/`brace`/
//!    `focus` 三条分支，`combo` 要求 `power_strike` 与 `brace` 两个
//!    前置同时满足才汇聚出现），施放 `power_strike` 产出正确的纯
//!    数值伤害效果。
//! 2. **副职可叠加**——demo 角色额外持有剑舞者副职，其索引与主职
//!    技能不冲突（裁定 P5-4：共享同一份 `ContentIndex` 命名空间，
//!    "不冲突"体现为注册表本身保证的索引唯一性，demo 显式断言）。
//! 3. **网状任务可推进**——`main_quest_1` 完成后 `branch_a`/`branch_b`
//!    两个后续任务同时可见（网状而非线性，一个前置解锁多个后续）。
//! 4. **技能冷却与任务进度经存档往返保持一致**——真实调用 P5-A 交付
//!    的 `ll_content::save_file::{save_to_file, load_full}` 存档读写
//!    管线，不是裸 `WorldState` 序列化替代（P5-A 已经完成到那一步，
//!    计划文档允许的替代方案本 demo 不需要）。
//!
//! # 裁定 CS-7 / ADR 0025：为什么本 demo 不涉及任何窗口/键盘
//!
//! 与 `crates/ll-content/examples/p5_save_acceptance.rs`（P5-A 验收
//! demo）同一条纪律：本次实现环境（后台代理会话）本身不具备任何
//! 桌面窗口自动化能力——没有可以确认"前台窗口归属"的工具，也没有
//! 任何合成键盘事件的手段，因此**不是**"图形环境可用但选择不用"，
//! 是从一开始就没有这个能力可用。如实记录，不假装已经用真实按键
//! 验证过；也不新起一个窗口再假装没测——干脆不碰窗口系统，全程走
//! 与真实按键完全相同的 `Intent → resolve → Effect → apply` 链路
//! （`ll_sim::resolve::resolve_with_skills_and_quests` 是本 demo 与
//! 未来任何真实输入层共用的同一个函数，不是 demo 自己另开的捷径）。
//!
//! **本 demo 全程是程序化验证，不是肉眼验收**——不启动任何窗口，没有
//! 任何"目视确认"的步骤；每一条结论都来自 `assert!`/`assert_eq!`，
//! 断言失败会让本进程以非零状态退出（`cargo run`）或让测试失败
//! （`cargo test`，见文件末尾 `#[cfg(test)]`）。技能解锁/任务完成前后
//! 的 `SkillTreeView`/`QuestLogView` 会被打印出来（`{:?}`），供人工
//! 复核数据确实反映了状态变化，但复核不是断言本身依赖的东西。
//!
//! # 完整调用链（对照 P5-B 计划文档「自查」一节逐环打勾）
//!
//! `mods/lostland/{classes,skills,subclasses,quests}.scm` 经
//! `ll_mod::pipeline::load_all` + `resolve_base_*` 契约解析（`ll-mod`）→
//! 玩家 `Agent` 直接持有 `profession`/`subclasses`/`unlocked_skills`
//! 字段（本计划不设计"解锁"这个 Intent 的具体触发方式，见计划文档
//! 「完整调用链」一节，本 demo 用直接赋字段模拟"升级/任务奖励解锁"）
//! → `Intent::UseSkill`/`Intent::Attack` →
//! `resolve_with_skills_and_quests`（真正查询 `SkillTable`/
//! `RegisteredQuests`）→ `Effect`（伤害/冷却/脚本状态）→ `apply`
//! （唯一写入口）→ `build_skill_tree_view`/`build_quest_log_view`
//! （任务 8 的 UI 数据层）→ `save_to_file`/`load_full`（P5-A 存档
//! 管线）→ 读档后世界与存档前逐位一致。

use std::collections::BTreeMap;

use ll_content::content_index_map::snapshot_for_header;
use ll_content::degrade::LoadOutcome;
use ll_content::header::{ModHeaderEntry, SaveHeader};
use ll_content::mode::SaveMode;
use ll_content::save_file::{CURRENT_SCHEMA_VERSION, load_full, save_to_file};
use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};
use ll_mod::class::{BaseClassIds, ClassTable, resolve_base_classes};
use ll_mod::clip::ClipTable;
use ll_mod::damage_category::DamageCategoryTable;
use ll_mod::formula::FormulaTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::{BaseQuestIds, QuestTable, RegisteredQuests, resolve_base_quests};
use ll_mod::quest_overview::build_quest_log_view;
use ll_mod::race::RaceTable;
use ll_mod::recipe::RecipeTable;
use ll_mod::recipe_category::RecipeCategoryTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::{BaseSkillIds, SkillTable, resolve_base_skills};
use ll_mod::subclass::{BaseSubclassIds, SubclassTable, resolve_base_subclasses};
use ll_mod::tag::TagTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::weapon_category::WeaponCategoryTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::quest::is_quest_completed;
use ll_sim::resolve::resolve_with_skills_and_quests;
use ll_sim::skill_overview::build_skill_tree_view;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, TerrainTable, materialize_base_terrain};
use ll_world::zone::ZoneLayout;

fn main() {
    println!("=== P5-B 玩法系统 —— 验收 demo ===\n");
    run_walkthrough();
    println!("\n=== 全部验收断言通过（程序化验证，未启动任何窗口） ===");
}

// ---------------------------------------------------------------------
// 共享测试夹具
// ---------------------------------------------------------------------

fn id(raw: &str) -> NamespacedId {
    NamespacedId::parse(raw).expect("demo 用标识符恒合法")
}

fn small_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
}

/// demo 用的全部内容注册表——本体地形（仍由 Rust 注册）+ 真实
/// `mods/` 目录装载出来的职业/技能/副职/任务，共用同一个 `Registry`。
///
/// # 为什么读真实 `mods/` 目录，而不是在这里现造一份内容
///
/// 职业/技能/副职/任务四类内容此前由 `materialize_base_*` 四个 Rust
/// 函数产出，本 demo 是它们**唯一**的非测试调用方。那四个函数在本体
/// 内容迁进脚本的批次里已经删除（见 `ll_mod::class` 模块文档同名
/// 一节），本 demo 因此改读真实内容目录——这不是权宜之计，是把这条
/// 验收链路的起点从"Rust 字面量"换成"玩家真正会装载的那份内容"，
/// ADR 0018 的判据（玩法层内容必须能从 mod 脚本注册，且要有真实 mod
/// 脚本为证）在本 demo 上因此也成立了。
struct Content {
    registry: Registry,
    terrain_ids: BaseTerrainIds,
    terrain_table: TerrainTable,
    class_ids: BaseClassIds,
    skill_ids: BaseSkillIds,
    skill_table: SkillTable,
    subclass_ids: BaseSubclassIds,
    quest_ids: BaseQuestIds,
    quest_table: QuestTable,
    goblin_kind: ContentIndex,
    human_race: ContentIndex,
}

/// 仓库根目录下的真实 `mods/` 路径。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

fn build_content() -> Content {
    let mut registry = Registry::new();
    // 地形仍由 Rust 注册（`ll_world::terrain` 尚未迁进脚本），且必须
    // 排在 `load_all` 之前——与 `ll_game::content::load_content` 的
    // 真实顺序一致。
    let (terrain_ids, mut terrain_table) =
        materialize_base_terrain(&mut |raw| registry.intern(raw))
            .expect("本体地形声明表内部一致，注册恒不失败");

    let mut class_table = ClassTable::new();
    let mut skill_table = SkillTable::new();
    let mut subclass_table = SubclassTable::new();
    let mut quest_table = QuestTable::new();
    let mut race_table = RaceTable::new();
    let mut clip_table = ClipTable::new();
    let mut xp_curve_table = XpCurveTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut trait_table = TraitTable::new();
    let mut resource_pool_table = ResourcePoolTable::new();
    let mut item_table = ItemTable::new();
    let mut formula_table = FormulaTable::new();
    let mut weapon_category_table = WeaponCategoryTable::new();
    let mut damage_category_table = DamageCategoryTable::new();
    let mut space_profile_table = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = RecipeTable::new();
    let mut recipe_category_table = RecipeCategoryTable::new();
    let mut tag_table = TagTable::new();

    let report = load_all(
        std::path::Path::new(REAL_MODS_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut terrain_table,
            class: &mut class_table,
            skill: &mut skill_table,
            subclass: &mut subclass_table,
            quest: &mut quest_table,
            race: &mut race_table,
            clip: &mut clip_table,
            xp_curve: &mut xp_curve_table,
            xp_curve_bindings: &mut xp_curve_bindings,
            trait_def: &mut trait_table,
            resource_pool: &mut resource_pool_table,
            item: &mut item_table,
            formula: &mut formula_table,
            weapon_category: &mut weapon_category_table,
            damage_category: &mut damage_category_table,
            space_profile: &mut space_profile_table,
            weather: &mut weather_table,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            tag: &mut tag_table,
        },
    );

    let lostland_id = id("lostland:self");
    let status = report
        .entries
        .iter()
        .find(|(id, _)| *id == lostland_id)
        .map(|(_, status)| status);
    assert_eq!(
        status,
        Some(&LoadStatus::Loaded),
        "本体内容 mod（mods/lostland/）必须成功加载，否则下面的断言毫无意义"
    );

    let class_ids =
        resolve_base_classes(&registry, &class_table).expect("本体职业契约必须解析成功");
    let skill_ids = resolve_base_skills(&registry, &skill_table).expect("本体技能契约必须解析成功");
    let subclass_ids =
        resolve_base_subclasses(&registry, &subclass_table).expect("本体副职契约必须解析成功");
    let quest_ids = resolve_base_quests(&registry, &quest_table).expect("本体任务契约必须解析成功");

    // quests.json5 里 main_quest_1/branch_a/finale 的 target_kind 已经
    // intern 过 "lostland:goblin"——再次 intern 同一个字符串按 Interner
    // 的既有语义返回相同索引，不会重复注册。种族同理（races.json5）。
    let goblin_kind = registry.intern(id("lostland:goblin"));
    let human_race = registry.intern(id("lostland:human"));

    Content {
        registry,
        terrain_ids,
        terrain_table,
        class_ids,
        skill_ids,
        skill_table,
        subclass_ids,
        quest_ids,
        quest_table,
        goblin_kind,
        human_race,
    }
}

fn build_world(content: &Content) -> WorldState {
    let layout = small_layout();
    let spawn = layout.tile_size().wrap(0, 0);
    WorldState::new(
        layout,
        &GenParams::default(),
        &content.terrain_ids,
        content.terrain_table.clone(),
        spawn,
    )
    .expect("demo 用布局满足全部构造前置条件")
}

fn pos_at(world: &WorldState, x: i32, y: i32) -> TorusPos {
    world.size.wrap(x, y)
}

/// 造一个空白 `Agent`——各调用点按需覆盖 `race`/`health`/`profession`/
/// `unlocked_skills`/`subclasses` 等字段。
fn bare_agent(world: &WorldState, pos: TorusPos) -> Agent {
    let (zone, _) = world.terrain.layout().tile_to_zone(pos);
    Agent {
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
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
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
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    }
}

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "ll-content-p5b-gameplay-acceptance-{name}-{}.llsave",
        std::process::id()
    ));
    path
}

// ---------------------------------------------------------------------
// 驱动全部验收断言的主流程——单一共享世界，四件事在同一次游玩里依次
// 发生（不是四个互相独立的场景），更贴近"这是一局真实游玩"而不是
// 四段互不相干的单元测试拼接。
// ---------------------------------------------------------------------

fn run_walkthrough() {
    let content = build_content();
    let mut world = build_world(&content);

    // 玩家：战士主职,携带剑舞者副职,初始只学会 strike。
    let mut player_agent = bare_agent(&world, pos_at(&world, 0, 0));
    player_agent.profession = content.class_ids.warrior;
    player_agent.race = content.human_race;
    player_agent.subclasses = vec![content.subclass_ids.duelist];
    player_agent.unlocked_skills = vec![content.skill_ids.strike];
    let player = world.actors.spawn(player_agent);

    section1_class_and_branching_skill_tree(&content, &mut world, player);
    section2_subclass_stacking(&content, &world, player);
    section3_networked_quest_progress(&content, &mut world, player);
    // 此前这里必须换一根线程：`load_full` 内部会强制重建全部 Steel
    // 引擎，而 `build_content` 已经在当前线程上编译过 mod 脚本，
    // 「先编译、后构造」正是 steel-core 0.8.2 偶发内存破坏的唯一已知
    // 触发条件（旧约束 C6 / ADR 0028）。脚本系统整体拆除后既没有引擎
    // 也没有那条相邻关系，直接在本线程上跑即可。
    section4_save_load_roundtrip(&content, &world);
}

// ---------------------------------------------------------------------
// 第一件事：职业与有分支的技能树 + 施放技能产出正确效果
// ---------------------------------------------------------------------

fn section1_class_and_branching_skill_tree(
    content: &Content,
    world: &mut WorldState,
    player: EntityId,
) {
    println!("[验收 1/4] 职业与技能树可用——分支解锁 + 施放技能产出效果");

    let before = build_skill_tree_view(
        world.actors.get(player).expect("玩家应存在"),
        &content.skill_table,
        world.clock,
    );
    println!("  升级前 SkillTreeView = {before:?}");
    assert_eq!(before.unlocked, vec![content.skill_ids.strike]);
    // strike 同时解锁 power_strike/brace/focus 三条分支——"树"而不是
    // "线性序列"的直接验收：一个已解锁技能有两个以上可选后续。
    //
    // 断言的是「三条都在、combo 还不在」而不是逐位相等：本 demo 现在
    // 装载的是**真实 `mods/` 目录**（见 `build_content` 文档），
    // `SkillTable` 里还有 `mods/example_mod/` 注册的技能，其中无前置的
    // 那几条同样会出现在 `available` 里。那不是缺陷，是真实内容集合的
    // 样子；逐位相等会让这条验收在任何人给 example_mod 加一条技能时
    // 无端变红，而它要验的性质与那件事无关。
    for branch in [
        content.skill_ids.power_strike,
        content.skill_ids.brace,
        content.skill_ids.focus,
    ] {
        assert!(
            before.available.contains(&branch),
            "strike 已解锁时，它的三条分支都应当可选"
        );
    }
    assert!(
        !before.available.contains(&content.skill_ids.combo),
        "combo 的两个前置都未解锁，此刻不该可选"
    );

    // "升级"：同时解锁 power_strike 与 brace（模拟任务奖励/升级点数，
    // 计划文档明确本批次不设计"解锁"这个 Intent 的具体触发方式,只
    // 保证字段与判定就绪，见本文件顶部模块文档「完整调用链」一节）。
    {
        let agent = world.actors.get_mut(player).expect("玩家应存在");
        agent.unlocked_skills.push(content.skill_ids.power_strike);
        agent.unlocked_skills.push(content.skill_ids.brace);
    }

    let after_unlock = build_skill_tree_view(
        world.actors.get(player).expect("玩家应存在"),
        &content.skill_table,
        world.clock,
    );
    println!("  升级后 SkillTreeView = {after_unlock:?}");
    // power_strike 与 brace 两个前置都满足后,combo（汇聚技能）出现在
    // available——验收"一个技能可以有多个前置"这条网状/分支性质。
    assert!(after_unlock.available.contains(&content.skill_ids.combo));
    assert!(
        !after_unlock
            .available
            .contains(&content.skill_ids.power_strike)
    );

    // 施放 power_strike：造一个"陪练"哥布林（血量足够高,这一下打不死,
    // 单纯验证伤害数值,不与下面的击杀计数场景混在一起）。
    let dummy_pos = pos_at(world, 1, 0);
    let dummy_agent = {
        let mut agent = bare_agent(world, dummy_pos);
        agent.race = content.goblin_kind;
        agent.health = 30;
        agent
    };
    let dummy = world.actors.spawn(dummy_agent);

    let effects = resolve_with_skills_and_quests(
        world,
        &Intent::UseSkill {
            actor: player,
            skill: content.skill_ids.power_strike,
            target: Some(dummy),
        },
        &content.skill_table,
        &RegisteredQuests {
            table: &content.quest_table,
            registry: &content.registry,
        },
    );
    assert!(
        !effects.is_empty(),
        "已解锁、无冷却的 power_strike 理应产出效果"
    );
    for effect in &effects {
        apply(world, effect);
    }

    let dummy_after = world.actors.get(dummy).expect("陪练目标应仍存在");
    assert_eq!(
        dummy_after.health, 18,
        "power_strike 基础伤害 12,陪练目标应从 30 掉到 18"
    );
    let player_after = world.actors.get(player).expect("玩家应存在");
    assert!(
        player_after
            .skill_cooldowns
            .contains_key(&content.skill_ids.power_strike),
        "施放后应当写入冷却"
    );

    println!(
        "  power_strike 命中：目标生命 30 -> {}，冷却已写入",
        dummy_after.health
    );
    println!("  职业/技能树验收通过\n");
}

// ---------------------------------------------------------------------
// 第二件事：副职可叠加，索引不与主职技能冲突
// ---------------------------------------------------------------------

fn section2_subclass_stacking(content: &Content, world: &WorldState, player: EntityId) {
    println!("[验收 2/4] 副职可叠加——剑舞者副职与主职技能索引不冲突");

    let agent = world.actors.get(player).expect("玩家应存在");
    assert!(agent.subclasses.contains(&content.subclass_ids.duelist));
    assert_eq!(
        agent.profession, content.class_ids.warrior,
        "主职不受副职影响"
    );

    // 索引不冲突：剑舞者的 ContentIndex 与战士全部技能的 ContentIndex
    // 两两不同——共享同一份 Registry 天然保证唯一性（裁定 P5-4：共享
    // 命名空间，不是"看起来不冲突"，是注册表结构本身不允许冲突）。
    let warrior_skills = [
        content.skill_ids.strike,
        content.skill_ids.power_strike,
        content.skill_ids.brace,
        content.skill_ids.focus,
        content.skill_ids.combo,
    ];
    for skill in warrior_skills {
        assert_ne!(content.subclass_ids.duelist, skill);
    }

    println!(
        "  玩家同时持有主职 warrior({:?}) 与副职 duelist({:?})，索引互不相同",
        content.class_ids.warrior, content.subclass_ids.duelist
    );
    println!("  副职叠加验收通过\n");
}

// ---------------------------------------------------------------------
// 第三件事：网状任务——一个前置解锁多个后续，真实击杀推进进度
// ---------------------------------------------------------------------

fn section3_networked_quest_progress(content: &Content, world: &mut WorldState, player: EntityId) {
    println!("[验收 3/4] 网状任务可推进——真实击杀经 resolve 结算完成 main_quest_1");

    let before = build_quest_log_view(
        world.actors.get(player).expect("玩家应存在"),
        &content.quest_table,
        &content.registry,
    );
    println!("  击杀前 QuestLogView = {before:?}");
    assert!(before.completed.is_empty());
    // 断言「起点任务可见、两条分支还不可见」而不是逐位相等：理由同
    // section2 里对 SkillTreeView::available 的处理——本 demo 装载的是
    // 真实 `mods/` 目录，example_mod 也注册着无前置的任务节点。
    assert!(
        before
            .unlocked_not_completed
            .contains(&content.quest_ids.main_quest_1)
    );
    for branch in [content.quest_ids.branch_a, content.quest_ids.branch_b] {
        assert!(
            !before.unlocked_not_completed.contains(&branch),
            "起点任务尚未完成，两条分支此刻都不该解锁"
        );
    }

    // main_quest_1 要求击杀 3 个哥布林（mods/lostland/quests.json5 的
    // 本体声明）——真实走
    // Intent::Attack -> resolve_with_skills_and_quests -> Effect ->
    // apply,不是直接调用 mark_quest_completed 伪造。
    for i in 0..3 {
        let goblin_pos = pos_at(world, 2 + i, 0);
        let goblin_agent = {
            let mut agent = bare_agent(world, goblin_pos);
            agent.race = content.goblin_kind;
            agent.health = 1; // 一击必杀,聚焦验证击杀->任务这条接线。
            agent
        };
        let goblin = world.actors.spawn(goblin_agent);

        let effects = resolve_with_skills_and_quests(
            world,
            &Intent::Attack {
                actor: player,
                target: goblin,
            },
            &content.skill_table,
            &RegisteredQuests {
                table: &content.quest_table,
                registry: &content.registry,
            },
        );
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Kill { .. })),
            "第 {} 次攻击应当致死",
            i + 1
        );
        for effect in &effects {
            apply(world, effect);
        }
    }

    let after = build_quest_log_view(
        world.actors.get(player).expect("玩家应存在"),
        &content.quest_table,
        &content.registry,
    );
    println!("  击杀 3 个哥布林后 QuestLogView = {after:?}");
    assert_eq!(after.completed, vec![content.quest_ids.main_quest_1]);
    // 网状结构的直接验收：一个前置任务完成后,两个后续分支同时可见,
    // 而汇聚任务 finale 仍需两者都完成,此刻不该出现。
    for branch in [content.quest_ids.branch_a, content.quest_ids.branch_b] {
        assert!(
            after.unlocked_not_completed.contains(&branch),
            "起点任务完成后,两条分支应当同时可见"
        );
    }
    assert!(
        !after
            .unlocked_not_completed
            .contains(&content.quest_ids.finale),
        "两条分支都还没完成,finale 此刻不该解锁"
    );

    let main_quest_id = content
        .registry
        .resolve(content.quest_ids.main_quest_1)
        .expect("main_quest_1 已注册")
        .clone();
    assert!(is_quest_completed(
        world.actors.get(player).expect("玩家应存在"),
        &main_quest_id,
    ));

    println!("  网状任务验收通过（branch_a 与 branch_b 同时解锁，finale 仍需两者都完成）\n");
}

// ---------------------------------------------------------------------
// 第四件事：技能冷却与任务进度经存档往返保持一致
// ---------------------------------------------------------------------

fn section4_save_load_roundtrip(content: &Content, world: &WorldState) {
    println!("[验收 4/4] 技能冷却与任务进度经存档往返保持一致（真实 P5-A 存档管线）");

    let content_index_map = snapshot_for_header(&content.registry);
    let header = SaveHeader {
        schema_version: CURRENT_SCHEMA_VERSION,
        saved_at: 1_755_200_000,
        character_name: "验收旅人".to_string(),
        current_region: "验收村落".to_string(),
        playtime_ticks: 0,
        generation_mods: Vec::<ModHeaderEntry>::new(),
        current_mods: Vec::new(),
        content_hash_algorithm_version: ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION,
        content_index_map: content_index_map.clone(),
        world_size: (1, 1),
        world_seed: 0,
        mode: SaveMode::Permadeath,
    };
    let hash_before = world.hash();
    let path = temp_path("full-walkthrough");
    save_to_file(&path, &header, world).expect("写出应当成功");

    // 当前会话：独立的一份 Content（独立 Registry 对象），重新走一遍
    // 与存档时结构相同的注册序列（地形 + 职业 + 技能 + 副职 + 任务）
    // ——remap_world 按字符串反查当前索引,不要求两个 Registry 对象
    // 数值上相同,只要求全部同一批字符串都已注册（见
    // ll_content::remap 模块文档）。
    let current = build_content();
    let outcome = load_full(&path, &current.registry, &[], current.terrain_table.clone());

    let loaded_world = match outcome {
        LoadOutcome::Playable(loaded) => loaded,
        other => panic!("期望 Playable，实际 {other:?}"),
    };
    assert_eq!(
        loaded_world.hash(),
        hash_before,
        "存档 -> 读档后世界必须逐位一致（含技能冷却、任务进度）"
    );

    let mut checked_player = false;
    for agent in loaded_world.actors.iter() {
        if agent.profession != content.class_ids.warrior {
            continue;
        }
        checked_player = true;
        assert!(
            agent
                .unlocked_skills
                .contains(&content.skill_ids.power_strike)
        );
        assert!(agent.unlocked_skills.contains(&content.skill_ids.brace));
        assert!(
            agent
                .skill_cooldowns
                .contains_key(&content.skill_ids.power_strike),
            "power_strike 的冷却应当经存档往返保留"
        );
        assert!(agent.subclasses.contains(&content.subclass_ids.duelist));
        let quest_id = content
            .registry
            .resolve(content.quest_ids.main_quest_1)
            .expect("main_quest_1 已注册")
            .clone();
        assert!(
            is_quest_completed(agent, &quest_id),
            "main_quest_1 的完成状态应当经存档往返保留"
        );
    }
    assert!(
        checked_player,
        "存档里应当能找到玩家（profession = warrior）"
    );

    let _ = std::fs::remove_file(&path);
    println!("  存档 -> 读档：世界哈希逐位一致，技能冷却/已解锁技能/副职/任务完成状态均原样保留");
    println!("  存档往返验收通过\n");
}

#[cfg(test)]
mod tests {
    #[test]
    fn 完整验收流程程序化验证() {
        super::run_walkthrough();
    }
}
