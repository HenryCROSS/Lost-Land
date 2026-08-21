//! P5 存档格式与身份——验收 Demo（任务 13）。
//!
//! 运行：`cargo run --example p5_save_acceptance -p ll-content`。
//!
//! 必须展示的三件事（用户提出的最低要求）：
//!
//! 1. 存档 → 读档后世界逐位一致——用 [`ll_world::state::WorldState::hash`]
//!    比对，不是「肉眼看起来一样」（[`section_a_full_roundtrip`]）。
//! 2. 缺失 mod 时按内容类型正确降级且不崩溃——物品丢弃、NPC 种族降级
//!    占位、玩家角色种族拒绝降级、只读模式（[`section_b_degrade_by_kind`]）。
//! 3. 模式2 → 模式3 单向降级生效且不可逆（[`section_c_mode_downgrade`]）。
//!
//! # 裁定 CS-7：为什么本 demo 不涉及任何窗口/键盘
//!
//! 上一批（坐标系重写）验收 demo 实测发现：这台机器上合成键盘事件
//! 不能可靠地只送达目标窗口——`GetForegroundWindow()` 诊断确认前台
//! 窗口始终是宿主应用本身，继续尝试会把按键泄漏进宿主聊天窗口（已经
//! 真实发生过）。那次的补救是 `walkthrough_test.rs`：不模拟按键，直接
//! 驱动 `Intent → resolve → Effect → apply` 这条与真实按键完全相同的
//! 链路本身。
//!
//! 本 demo 验收的是存档格式，本来就不涉及任何渲染或输入系统——
//! `save_to_file`/`load_full` 是纯数据函数，不存在「前台窗口归属」
//! 这个问题，因此不需要额外规避 SendKeys。但游玩阶段（下方「步骤
//! 二」）仍然沿用与真实按键完全相同的
//! `Intent → resolve → Effect → apply` 路径（复用
//! `crates/ll-sim/examples/p5_coordinate_acceptance/walkthrough_test.rs`
//! 确立的同一套方法论），不是靠直接改字段伪造「玩家玩过」这件事。
//!
//! **本 demo 全程是程序化验证，不是肉眼验收**——不启动任何窗口，没有
//! 任何「目视确认」的步骤；每一条结论都来自 `assert!`/`assert_eq!`，
//! 断言失败会让本进程以非零状态退出，不会打印一个虚假的「通过」。
//!
//! # 曾经记录的两处真实限制，P5-A 任务 14 已修复
//!
//! 本 demo 早期版本在这里记录过两处生产代码限制,本次改动已经在
//! `ll-content`/`ll-mod` 里修掉,如实更新说明（不是删掉历史,是标注
//! 现状）：
//!
//! 1. **`load_full` 的 NPC 占位分支曾经不可达**——`save_file.rs` 里
//!    `load_full_from_bytes` 曾把 `placeholder` 参数硬编码为 `None`。
//!    修复：新增 [`ll_mod::base_placeholder`] 模块,把本体占位内容
//!    注册进 `Registry`（与 `base_terrain` 完全相同的注册通道）,
//!    `load_full` 现在从当前会话的 registry 里真的查询这个索引。
//!    [`section_b_degrade_by_kind`] 的 `b4` 小节验证这条分支在完整
//!    `load_full` 管线里确实可达,不再需要绕过它直接调用
//!    [`ll_content::remap::remap_world`]。
//! 2. **`check_mod_content` 曾经比 `remap_world` 的细粒度降级更严格**
//!    ——若某个 mod 被记入存档头 `generation_mods` 且带着真实内容
//!    哈希,而当前会话完全没有装载它,`check_mod_content` 会在
//!    `remap_world` 有机会展示任何「按内容类型降级」之前就直接判定
//!    `ModContentMismatch`（`LoadOutcome::Rejected`）——「玩家卸载了
//!    一整个 mod」这个最直观的场景因此被硬拒绝,而不是规格 §10.4 描述
//!    的「按内容类型优雅降级」。修复：`check_mod_content` 现在借助
//!    `current_manifests` 分清「mod 仍在场但内容变了」（真不兼容,硬
//!    拒绝）与「mod 完全不在场」（放行给 `remap_world` 降级）。
//!    [`b3_player_missing_full_pipeline_readonly`] 现在记一条真实的
//!    `generation_mods` 条目,不再需要靠留空规避这个检查点。

use std::collections::BTreeMap;

use ll_content::content_index_map::{rebuild_from_header, snapshot_for_header};
use ll_content::degrade::{
    ContentKind, DegradeAction, LoadOutcome, OwnerContext, decide_degrade_action,
};
use ll_content::header::{ModHeaderEntry, SaveHeader};
use ll_content::mode::SaveMode;
use ll_content::remap::remap_world;
use ll_content::save_file::{
    CURRENT_SCHEMA_VERSION, load_from_header_only, load_full, save_to_file,
};
use ll_content::world_identity::{
    WorldIdentity, generation_mods_to_header_entries, validate_size_choice,
};
use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};
use ll_mod::manifest::ModManifest;
use ll_mod::mod_set::GenerationModSet;
use ll_mod::registry::Registry;
use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_sim::intent::Intent;
use ll_sim::resolve::resolve;
use ll_world::entity::{Agent, BaseStats};
use ll_world::generate::GenParams;
use ll_world::script_state::{ScriptStateTarget, ScriptStateWrite, ScriptValue};
use ll_world::space::{Space, ZoneCoord};
use ll_world::state::WorldState;
use ll_world::terrain::{
    BaseTerrainIds, TerrainTable, base_terrain_fixture, materialize_base_terrain,
};
use ll_world::zone::ZoneLayout;

fn main() {
    println!("=== P5 存档格式与身份 —— 验收 demo ===\n");

    step0_world_identity_chain_link();
    section_a_full_roundtrip();
    section_b_degrade_by_kind();
    section_c_mode_downgrade();

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

/// 建一个带真实本体地形的世界 + 对应的「存档时」`Registry`。
fn world_with_registry() -> (WorldState, Registry, BaseTerrainIds) {
    let mut registry = Registry::new();
    let (terrain_ids, terrain_table) = materialize_base_terrain(&mut |id| registry.intern(id))
        .expect("本体地形声明表内部一致，注册恒不失败");
    let layout = small_layout();
    let spawn = layout.tile_size().wrap(0, 0);
    let world = WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("demo 用布局满足全部构造前置条件");
    (world, registry, terrain_ids)
}

/// 与 [`world_with_registry`] 地形内容逐字符串一致的「当前会话」
/// registry + `TerrainTable`。
fn current_session_registry_with_terrain() -> (Registry, TerrainTable) {
    let mut registry = Registry::new();
    let (_ids, terrain_table) = materialize_base_terrain(&mut |id| registry.intern(id))
        .expect("本体地形声明表内部一致，注册恒不失败");
    (registry, terrain_table)
}

fn bare_agent(pos: TorusPos, zone: ZoneCoord) -> Agent {
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
        luck: 0,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        unlocked_skills: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        script_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
    }
}

fn header_with(
    content_index_map: Vec<String>,
    generation_mods: Vec<ModHeaderEntry>,
    mode: SaveMode,
) -> SaveHeader {
    SaveHeader {
        schema_version: CURRENT_SCHEMA_VERSION,
        saved_at: 1_755_200_000,
        character_name: "验收旅人".to_string(),
        current_region: "验收村落".to_string(),
        playtime_ticks: 0,
        generation_mods,
        current_mods: Vec::new(),
        content_hash_algorithm_version: ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION,
        content_index_map,
        world_size: (1, 1),
        world_seed: 0,
        mode,
    }
}

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "ll-content-p5-acceptance-{name}-{}.llsave",
        std::process::id()
    ));
    path
}

// ---------------------------------------------------------------------
// 步骤零：建档——世界身份三要素的绑定链路（任务 4 的产出）
// ---------------------------------------------------------------------

/// 完整调用链的第一环：`validate_size_choice` → `WorldIdentity::bind` →
/// `GenerationModSet::capture` → `generation_mods_to_header_entries`——
/// 这是「玩家在开局界面选择世界尺寸」到「世界创建」再到「写进存档头」
/// 这一整段，本 demo 之前的批次已经交付并测试过前半段，这里做一次真实
/// 调用，确认这一环仍然接得通，不是假设它还成立。
///
/// # 断链三已修复（P5-A 任务 14）
///
/// 本 demo 早期版本在这里发现过一处生产代码缺口：`GenerationModSet`
/// （`ll_mod::mod_set`）到 `SaveHeader::generation_mods`
/// （`ll_content::header`）之间没有任何生产代码里的转换函数，demo 当时
/// 只能在自己代码里临时补一份等价的胶水代码绕过去。现在改为直接调用
/// [`ll_content::world_identity::generation_mods_to_header_entries`]——
/// 这是补上的生产函数，本 demo 不再需要自己的胶水实现。
fn step0_world_identity_chain_link() {
    println!("[步骤零] 建档：世界身份三要素绑定链路");

    let mut registry = Registry::new();
    materialize_base_terrain(&mut |id| registry.intern(id))
        .expect("本体地形声明表内部一致，注册恒不失败");
    let manifests = vec![ModManifest {
        id: id("lostland:self"),
        version: "0.1.0".to_string(),
        dependencies: Vec::new(),
        entry_points: Vec::<std::path::PathBuf>::new(),
    }];

    let layout = validate_size_choice(64, (1, 1)).expect("1x1/64 是合法尺寸选择");
    let generation = GenerationModSet::capture(&registry, &manifests);
    let identity = WorldIdentity::bind(20_260_819, layout, generation.clone());

    assert_eq!(identity.seed, 20_260_819);
    assert_eq!(identity.generation_mods, generation);

    let header_entries = generation_mods_to_header_entries(&generation);
    assert_eq!(header_entries.len(), 1);
    assert_eq!(header_entries[0].namespace, "lostland");
    assert_eq!(header_entries[0].version, "0.1.0");
    assert!(
        header_entries[0].content_hash.is_some(),
        "本体地形已经贡献了内容，生成期哈希不应为 None"
    );

    println!(
        "  world_identity 链路通：validate_size_choice -> WorldIdentity::bind -> GenerationModSet::capture -> generation_mods_to_header_entries"
    );
    println!(
        "  （断链三已修复：调用的是 ll-content 生产代码里的转换函数，不是 demo 自己的胶水代码）\n"
    );
}

// ---------------------------------------------------------------------
// 第一件事：存档 → 读档后世界逐位一致
// ---------------------------------------------------------------------

/// 完整调用链的核心一段：世界生成 → 游玩（真实
/// `Intent → resolve → Effect → apply`，含脚本状态写入）→ 存档 →
/// 退出 → 读档 → 世界与存档前逐位一致。
fn section_a_full_roundtrip() {
    println!("[验收 1/3] 存档 → 读档后世界逐位一致");

    let (mut world, mut registry, terrain_ids) = world_with_registry();
    let farmer = registry.intern(id("lostland:farmer"));
    let human = registry.intern(id("lostland:human"));
    let miner = registry.intern(id("lostland:miner"));

    let player_pos = world.size.wrap(1, 1);
    let player_zone = world.terrain.layout().tile_to_zone(player_pos).0;
    let mut player_agent = bare_agent(player_pos, player_zone);
    player_agent.profession = farmer;
    player_agent.race = human;
    player_agent.wallet = 100;
    let player = world.actors.spawn(player_agent);
    world.player_entity = Some(player);

    let npc_pos = world.size.wrap(2, 1);
    let npc_zone = world.terrain.layout().tile_to_zone(npc_pos).0;
    let mut npc_agent = bare_agent(npc_pos, npc_zone);
    npc_agent.profession = miner;
    npc_agent.race = human;
    world.actors.spawn(npc_agent);

    // 游玩：真实的 Intent → resolve → Effect → apply（与
    // walkthrough_test.rs 同一条路径,不是直接改字段伪造）。
    let wait_intent = Intent::Wait { actor: player };
    let effects = resolve(&world, &wait_intent);
    assert!(
        !effects.is_empty(),
        "Wait 意图恒产出至少一条 ScheduleNext 效果"
    );
    for effect in &effects {
        apply(&mut world, effect);
    }

    // 玩家改动的地形（不是噪声生成的原始结果）。
    let edited_pos = world.size.wrap(3, 1);
    world.terrain.set_terrain(edited_pos, terrain_ids.grass);

    // 脚本状态：证明写入真的经过 apply 这条唯一入口,并且会被
    // WorldState::hash() 捕捉——这是本次验收纪律点 (a) 的直接验证。
    let hash_before_script = world.hash();
    let script_effect = Effect::SetScriptState {
        writes: vec![
            ScriptStateWrite {
                target: ScriptStateTarget::Global,
                mod_namespace: "lostland".to_string(),
                key: "world_flag".to_string(),
                value: ScriptValue::Bool(true),
            },
            ScriptStateWrite {
                target: ScriptStateTarget::Entity(player),
                mod_namespace: "lostland".to_string(),
                key: "quest_stage".to_string(),
                value: ScriptValue::Int(3),
            },
        ],
    };
    apply(&mut world, &script_effect);
    let hash_after_script = world.hash();
    assert_ne!(
        hash_before_script, hash_after_script,
        "脚本状态写入经 apply 之后，WorldState::hash() 必须变化——\
         否则脚本状态就游离在确定性回归测试之外（P3 hash() 早期版本\
         同一类缺口的重演）"
    );
    assert_eq!(
        world
            .global_script_state
            .get(&("lostland".to_string(), "world_flag".to_string())),
        Some(&ScriptValue::Bool(true)),
        "全局脚本状态必须真的落到 WorldState 上，不是只改了哈希"
    );
    assert_eq!(
        world
            .actors
            .get(player)
            .expect("玩家实体应当仍存在")
            .script_state
            .get(&("lostland".to_string(), "quest_stage".to_string())),
        Some(&ScriptValue::Int(3)),
        "每实体脚本状态必须落到对应 Agent 上"
    );

    let content_index_map = snapshot_for_header(&registry);
    // generation_mods 这里留空是刻意的（与断链二无关）：让本节聚焦
    // 「往返逐位一致」这一件事,不与 mod 版本兼容性检查纠缠——那件事由
    // 下方 section_b 分别验证（含真实的 generation_mods 条目）。
    let header = header_with(content_index_map.clone(), Vec::new(), SaveMode::Permadeath);
    let hash_before_save = world.hash();
    let path = temp_path("full-roundtrip");
    save_to_file(&path, &header, &world).expect("写出应当成功");

    let current_registry = rebuild_from_header(&content_index_map)
        .expect("content_index_map 本 demo 自己产出，恒合法");
    let (_terrain_ids, current_terrain_table) = base_terrain_fixture();
    let outcome = load_full(&path, &current_registry, &[], current_terrain_table, &[]);

    match outcome {
        LoadOutcome::Playable(loaded_world) => {
            assert_eq!(
                loaded_world.hash(),
                hash_before_save,
                "存档 → 读档后世界必须逐位一致（含实体、地形改动、脚本状态）"
            );
            assert_eq!(
                loaded_world
                    .global_script_state
                    .get(&("lostland".to_string(), "world_flag".to_string())),
                Some(&ScriptValue::Bool(true)),
                "脚本状态本身也必须在读档后原样还在,不能只是哈希碰巧相等"
            );
        }
        other => panic!("期望 Playable，实际 {other:?}"),
    }
    let _ = std::fs::remove_file(&path);

    println!(
        "  世界生成 -> 游玩(Intent/resolve/apply) -> 脚本状态写入(经 apply) -> 存档 -> 读档：哈希逐位一致"
    );
    println!("  脚本状态写入确认被 WorldState::hash() 捕捉，且读档后原样还在\n");
}

// ---------------------------------------------------------------------
// 第二件事：缺失 mod 时按内容类型正确降级且不崩溃
// ---------------------------------------------------------------------

fn section_b_degrade_by_kind() {
    println!("[验收 2/3] 缺失 mod 时按内容类型正确降级且不崩溃");
    b1_item_missing_policy_only();
    b2_player_vs_npc_race_missing_mixed_outcome();
    b3_player_missing_full_pipeline_readonly();
    b4_npc_race_missing_full_pipeline_placeholder();
    println!();
}

/// 物品类型缺失 → 丢弃并提示。
///
/// # 为什么不走完整读档管线（如实记录）
///
/// `WorldState` 目前没有背包/物品字段——`ll-world` 还没有落地物品系统
/// （规格已经把物品与装备排到了 P6），[`ContentKind::Item`] 因此在
/// 生产代码里也确实没有任何调用点在用它（`crates/ll-content/src/remap.rs`
/// 穷尽处理了角色属性/目标/归属/结构性内容四类，唯独不包含物品——
/// 因为压根没有物品字段可以遍历）。本节只能验证策略层本身
/// （[`decide_degrade_action`]）行为正确，不能证明它已经接入某条真实
/// 读档路径——那条路径要等 P6 物品系统落地之后才存在。
fn b1_item_missing_policy_only() {
    let action = decide_degrade_action(ContentKind::Item, OwnerContext::None, None);
    assert_eq!(
        action,
        DegradeAction::DropWithWarning,
        "物品类型缺失应当丢弃并警告"
    );
    println!(
        "  [2a] 物品类型缺失 -> DropWithWarning（策略层验证；P6 物品系统落地前，WorldState 无物品字段可供端到端验证，如实记录）"
    );
}

/// 同一个世界里，玩家角色种族缺失（拒绝降级）与 NPC 种族缺失（占位
/// 可用时降级为占位）——两者必须走向不同结果，证明「谁是玩家」这个
/// 判断没有被弄反（验收纪律点 b）。
///
/// # 为什么直接调用 `remap_world`，不经过 `load_full`
///
/// 这不再是绕过某处硬编码限制（`load_full` 的占位分支现在确实可达，
/// 见 [`b4_npc_race_missing_full_pipeline_placeholder`]）——这里继续
/// 直接调用 [`remap_world`]（`ll-content` 对外公开的真实生产函数）单纯
/// 是因为本节要在**同一次**重映射里同时观察玩家与 NPC 两种归属的结果，
/// `load_full` 每次调用只处理一份存档、产出一个整体 `LoadOutcome`，
/// 拿不到「这一条具体是 Reject 还是 FallbackToPlaceholder」的逐条明细。
fn b2_player_vs_npc_race_missing_mixed_outcome() {
    let (mut world, mut save_registry, _terrain_ids) = world_with_registry();
    let vanished_player_race = save_registry.intern(id("uninstalledmod:player_race"));
    let vanished_npc_race = save_registry.intern(id("uninstalledmod:npc_race"));

    let player_pos = world.size.wrap(1, 1);
    let player_zone = world.terrain.layout().tile_to_zone(player_pos).0;
    let mut player_agent = bare_agent(player_pos, player_zone);
    player_agent.race = vanished_player_race;
    let player = world.actors.spawn(player_agent);
    world.player_entity = Some(player);

    let npc_pos = world.size.wrap(2, 1);
    let npc_zone = world.terrain.layout().tile_to_zone(npc_pos).0;
    let mut npc_agent = bare_agent(npc_pos, npc_zone);
    npc_agent.race = vanished_npc_race;
    let npc = world.actors.spawn(npc_agent);

    let content_index_map = snapshot_for_header(&save_registry);

    // 当前会话：装载了本体地形 + 本体占位内容（走
    // ll_mod::base_placeholder 的生产注册路径，不是 demo 自己拼一个
    // 字符串），但完全没有装载 uninstalledmod。
    let (mut current_registry, _terrain_table) = current_session_registry_with_terrain();
    let placeholder =
        ll_mod::base_placeholder::register_base_placeholder_content(&mut current_registry);

    let actions = remap_world(
        &mut world,
        &content_index_map,
        &current_registry,
        Some(placeholder),
    )
    .expect("地形/结构性内容都能对上号，本次调用不应返回 Err");

    assert!(
        actions.contains(&DegradeAction::Reject),
        "玩家角色种族缺失必须产生 Reject 决策，即便占位索引确实可用"
    );
    assert!(
        actions.contains(&DegradeAction::FallbackToPlaceholder(placeholder)),
        "NPC 种族缺失且占位索引可用时必须降级为占位，不是也被拒绝"
    );

    let player_race_after = world
        .actors
        .get(player)
        .expect("玩家实体应当仍存在（不崩溃）")
        .race;
    let npc_race_after = world
        .actors
        .get(npc)
        .expect("NPC 实体应当仍存在（不崩溃）")
        .race;
    // 注意：不能拿 player_race_after 与 placeholder 的裸整数比较来判断
    // 「有没有被换成占位」——Reject 分支保留的是存档写出时的原始索引
    // （remap.rs 原文「原样保留旧索引」），它与当前会话 registry 的
    // 占位索引活在两个完全独立的编号空间里，两者的裸数值恰好相等只是
    // 两个 Registry 各自从相同起点顺序分配索引的巧合（两边都紧跟在
    // 本体地形之后第一个插入），不代表内容真的变成了占位——第一版
    // demo 代码在这里写反了断言方式，被这条巧合当场揭穿，如实记录在
    // 任务报告里。正确的判据是：字段必须与写出时的原始值逐位相等，
    // 这才是「没有被替换成任何新内容」的确凿证明。
    assert_eq!(
        player_race_after, vanished_player_race,
        "玩家角色种族字段必须保持存档写出时的原始索引不变（Reject 决策的\
         定义，见 remap.rs），不会被替换成占位内容或任何其他值——那正是\
         「玩家会失去自己的角色」"
    );
    assert_eq!(npc_race_after, placeholder, "NPC 应当真的换成了占位内容");

    // 汇总结果：即便 NPC 优雅降级了，只要出现过一次 Reject（玩家），
    // 整体读档结果仍然是只读——不会因为「大部分内容都能降级」就放行
    // 继续游玩。
    let outcome = ll_content::degrade::summarize_load_outcome(world, &actions);
    assert!(matches!(outcome, LoadOutcome::ReadOnly(_)));

    println!(
        "  [2b] 同一世界内玩家种族缺失(Reject) 与 NPC 种族缺失(FallbackToPlaceholder({placeholder:?})) 结果不同，未误判"
    );
}

/// 玩家角色种族缺失，走完整的「文件 → load_full」管线（不是直接调用
/// `remap_world`），确认端到端也是 `ReadOnly` 而不是崩溃/静默丢数据；
/// 并展示只读模式本身：允许查看/导出，不提供任何能推进世界的方法。
fn b3_player_missing_full_pipeline_readonly() {
    let (mut world, mut save_registry, _terrain_ids) = world_with_registry();
    let vanished_race = save_registry.intern(id("uninstalledmod:player_race"));
    let vanished_content_hash = save_registry
        .content_hash_of("uninstalledmod")
        .expect("刚刚 intern 过，必有内容哈希");
    let player_pos = world.size.wrap(1, 1);
    let player_zone = world.terrain.layout().tile_to_zone(player_pos).0;
    let mut player_agent = bare_agent(player_pos, player_zone);
    player_agent.race = vanished_race;
    player_agent.wallet = 55;
    let player = world.actors.spawn(player_agent);
    world.player_entity = Some(player);

    let content_index_map = snapshot_for_header(&save_registry);
    // 断链二已修复：generation_mods 现在记一条真实的 uninstalledmod
    // 条目（带真实 content_hash），不再需要靠留空规避 check_mod_content
    // ——当前会话的 current_manifests（load_full 调用处传 &[]）里完全
    // 找不到这个命名空间，check_mod_content 会把判断放行给
    // remap_world，本节验证的正是这条放行之后的细粒度降级路径。
    let generation_mods = vec![ModHeaderEntry {
        namespace: "uninstalledmod".to_string(),
        version: "0.1.0".to_string(),
        content_hash: Some(vanished_content_hash),
    }];
    let header = header_with(content_index_map, generation_mods, SaveMode::Permadeath);
    let path = temp_path("player-race-missing");
    save_to_file(&path, &header, &world).expect("写出应当成功");

    // current_manifests 传 &[]——uninstalledmod 确实被卸载了，manifests
    // 里找不到它，这正是要验证的场景。
    let (current_registry, terrain_table) = current_session_registry_with_terrain();
    let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);

    match outcome {
        LoadOutcome::ReadOnly(read_only) => {
            // 只读：仍然可以查看——玩家的钱包/位置这些数据没有丢失。
            let viewed = read_only
                .world()
                .actors
                .get(player)
                .expect("只读模式下仍能查看玩家实体");
            assert_eq!(viewed.wallet, 55, "只读模式下数据必须完整，不能丢失");

            // 只读边界是编译期保证：ReadOnlySave 不提供任何 &mut
            // WorldState 的方法（degrade.rs 模块文档带了一条
            // compile_fail 示例锁住这一点），这里只做运行期层面的
            // 补充验证——导出后拿到的是一个可以被完整使用的普通
            // WorldState。
            let exported = read_only.export();
            assert_eq!(
                exported.actors.get(player).expect("导出后实体仍在").wallet,
                55
            );
        }
        other => panic!("期望 ReadOnly，实际 {other:?}"),
    }
    let _ = std::fs::remove_file(&path);

    println!(
        "  [2c] 玩家角色种族缺失（完整 load_full 管线）-> ReadOnly，未崩溃；只读模式下数据可查看/导出"
    );
}

/// NPC 种族缺失，走完整的「文件 → load_full」管线（不是直接调用
/// `remap_world`），确认占位降级分支在生产读档管线里真的可达
/// （断链一修复，P5-A 任务 14）——此前这条分支只能通过直接调用
/// `remap_world` 观察，见 [`b2_player_vs_npc_race_missing_mixed_outcome`]
/// 文档。
fn b4_npc_race_missing_full_pipeline_placeholder() {
    let (mut world, mut save_registry, _terrain_ids) = world_with_registry();
    let vanished_race = save_registry.intern(id("uninstalledmod:npc_race"));
    let npc_pos = world.size.wrap(2, 1);
    let npc_zone = world.terrain.layout().tile_to_zone(npc_pos).0;
    let mut npc_agent = bare_agent(npc_pos, npc_zone);
    npc_agent.race = vanished_race;
    let npc = world.actors.spawn(npc_agent);

    let content_index_map = snapshot_for_header(&save_registry);
    let header = header_with(content_index_map, Vec::new(), SaveMode::Permadeath);
    let path = temp_path("npc-race-missing-full-pipeline");
    save_to_file(&path, &header, &world).expect("写出应当成功");

    // 当前会话：地形 + 本体占位内容都走生产注册路径。
    let (mut current_registry, terrain_table) = current_session_registry_with_terrain();
    let expected_placeholder =
        ll_mod::base_placeholder::register_base_placeholder_content(&mut current_registry);
    let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);

    match outcome {
        LoadOutcome::Playable(loaded_world) => {
            let race_after = loaded_world
                .actors
                .get(npc)
                .expect("NPC 实体应当仍存在（不崩溃）")
                .race;
            assert_eq!(
                race_after, expected_placeholder,
                "NPC 种族应当真的被换成本体占位内容"
            );
        }
        other => panic!("期望 Playable，实际 {other:?}"),
    }
    let _ = std::fs::remove_file(&path);

    println!(
        "  [2d] NPC 种族缺失（完整 load_full 管线，此前不可达）-> Playable，种族已换成占位内容"
    );
}

// ---------------------------------------------------------------------
// 第三件事：模式2 → 模式3 单向降级，生效且不可逆
// ---------------------------------------------------------------------

fn section_c_mode_downgrade() {
    println!("[验收 3/3] 模式2 → 模式3 单向降级，生效且不可逆");

    let (world, registry, _terrain_ids) = world_with_registry();
    let content_index_map = snapshot_for_header(&registry);

    let permadeath_header =
        header_with(content_index_map.clone(), Vec::new(), SaveMode::Permadeath);
    let path = temp_path("mode-downgrade");
    save_to_file(&path, &permadeath_header, &world).expect("写出应当成功");

    // 「尝试升级回模式2」——SaveMode 没有任何公开 API 能做到这件事：
    // 唯一的转换函数是 downgrade()，FreeSave 上调用它恒返回 None。这里
    // 先确认降级本身生效。
    let downgraded_mode = SaveMode::Permadeath
        .downgrade()
        .expect("Permadeath 必然可以降级");
    assert!(downgraded_mode.was_downgraded_from_permadeath());

    let downgraded_header = header_with(content_index_map, Vec::new(), downgraded_mode);
    save_to_file(&path, &downgraded_header, &world).expect("重新写出应当成功");

    // 只读头部即可看到模式与降级标记——不需要触发主体解压。
    let loaded_header = load_from_header_only(&path).expect("读头部应当成功");
    assert!(matches!(loaded_header.mode, SaveMode::FreeSave { .. }));
    assert!(loaded_header.mode.was_downgraded_from_permadeath());

    // 「升级回模式2」应被拒绝——尝试对已降级的 FreeSave 再调用
    // downgrade()，唯一存在的返回路径仍然不产出 Permadeath。
    assert_eq!(
        loaded_header.mode.downgrade(),
        None,
        "FreeSave 不存在任何返回 Permadeath 的代码路径"
    );

    // 完整读档管线：确认降级标记不只在头部持久化，走完整
    // load_full（含主体反序列化）之后仍然是同一个存档、同一个模式。
    let current_registry =
        rebuild_from_header(&content_index_map_from(&path)).expect("重建应当成功");
    let (_terrain_ids, terrain_table) = base_terrain_fixture();
    let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);
    assert!(
        matches!(outcome, LoadOutcome::Playable(_)),
        "模式降级本身不应影响存档是否可玩"
    );

    let _ = std::fs::remove_file(&path);

    println!("  模式2 存档降级为模式3 -> 重新写出 -> 读档：模式仍是 FreeSave，标记为真");
    println!("  再次调用 downgrade() 恒返回 None——没有任何路径能把标记撤销或\"升级\"回模式2");
}

/// 辅助：从已经写出的存档文件重新读一次头部,取出它的
/// `content_index_map`——供 [`section_c_mode_downgrade`] 复用同一份
/// 索引映射构造当前会话 registry,不需要重新构造一次存档时的世界。
fn content_index_map_from(path: &std::path::Path) -> Vec<String> {
    load_from_header_only(path)
        .expect("此函数只在已知存在合法存档文件时调用")
        .content_index_map
}
