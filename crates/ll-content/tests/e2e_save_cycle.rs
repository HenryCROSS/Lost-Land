//! L6 端到端测试脚手架起步（任务 12）。
//!
//! 落地 `p4-to-p5.md` 五、1 的建议——三轮交接清单反复提议但一直未真正
//! 起步的端到端层。本文件不追求覆盖全部玩法，只交付「存档 → 修改 mod
//! 列表 → 读档 → 断言降级正确」这一条完整链路的自动化测试，作为 L6
//! 层第一个真正的用例，供后续阶段（含 P5-B）复用这套脚手架。
//!
//! # 为什么是外部集成测试，不是 crate 内 `#[cfg(test)]` 模块
//!
//! `crates/ll-content/src/save_file.rs`/`remap.rs` 各自的 `#[cfg(test)]`
//! 模块已经覆盖了同一批组件——但那些测试可以访问模块私有细节（例如
//! `Remapper` 内部字段），验证的是「单个组件接线是否正确」。本文件放在
//! `tests/`，只能看到 `ll-content` 对外公开的函数，验证的是「作为
//! 外部调用方（未来的存档管理 UI/其他 crate），单靠公开 API 能不能
//! 走完一整条存档 → 读档链路」——这正是 L6 端到端层要回答的问题，与
//! 单元测试的关注点不同，不是重复覆盖。
//!
//! # 程序化驱动（裁定 CS-7）
//!
//! 存档格式本身不涉及任何窗口/键盘——`save_to_file`/`load_full` 是纯
//! 数据函数，不经过渲染或输入系统，因此本文件自动满足 CS-7「不得使用
//! `SendKeys` 或任何合成键盘事件」的纪律，不需要额外小心：全程走的是
//! 与真实按键完全相同的下游路径（构造数据 → 调用公开函数 → 断言结果），
//! 只是驱动这条路径的输入来自测试代码直接构造，不来自键盘。

use ll_content::content_index_map::{rebuild_from_header, snapshot_for_header};
use ll_content::degrade::LoadOutcome;
use ll_content::header::SaveHeader;
use ll_content::mode::SaveMode;
use ll_content::save_file::{
    CURRENT_SCHEMA_VERSION, load_from_header_only, load_full, save_to_file,
};
use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};
use ll_mod::registry::Registry;
use ll_world::entity::{Agent, BaseStats};
use ll_world::generate::GenParams;
use ll_world::space::{Space, ZoneCoord};
use ll_world::state::WorldState;
use ll_world::terrain::{TerrainTable, base_terrain_fixture, materialize_base_terrain};
use ll_world::zone::ZoneLayout;

/// 测试用的最小区块布局：1×1 区块、边长 64——只要能覆盖出生点邻域即可，
/// 不需要真实游玩场景的尺寸。
fn small_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
}

/// 建一个带真实本体地形内容的存档时 `Registry` + 配套 `WorldState`——
/// `WorldState::new` 恒预热出生点邻域，地形从不为空，`content_index_map`
/// 必须覆盖它，这里统一走本体地形声明表而不是空表，避免各测试各自
/// 重新踩一遍这条前提。附带返回 `BaseTerrainIds`，供测试挑选具体地形
/// 种类（例如把某一格从当前值改成草地）。
fn world_with_registry() -> (WorldState, Registry, ll_world::terrain::BaseTerrainIds) {
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
    .expect("测试布局满足全部构造前置条件");
    (world, registry, terrain_ids)
}

/// 与 [`world_with_registry`] 地形内容逐字符串一致的「当前会话」
/// registry + `TerrainTable`，供读档一侧使用。
fn current_session_registry_with_terrain() -> (Registry, TerrainTable) {
    let mut registry = Registry::new();
    let (_ids, terrain_table) = materialize_base_terrain(&mut |id| registry.intern(id))
        .expect("本体地形声明表内部一致，注册恒不失败");
    (registry, terrain_table)
}

fn id(raw: &str) -> NamespacedId {
    NamespacedId::parse(raw).expect("测试用标识符恒合法")
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
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        equipment: std::collections::BTreeMap::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(zone, ContentIndex::default()),
        script_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        stealthed: false,
    }
}

fn sample_header(content_index_map: Vec<String>, mode: SaveMode) -> SaveHeader {
    SaveHeader {
        schema_version: CURRENT_SCHEMA_VERSION,
        saved_at: 1_755_100_000,
        character_name: "端到端测试角色".to_string(),
        current_region: "端到端测试区域".to_string(),
        playtime_ticks: 0,
        generation_mods: Vec::new(),
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
        "ll-content-e2e-{name}-{}.llsave",
        std::process::id()
    ));
    path
}

#[test]
fn 含实体与地形改动的世界存档读档后哈希一致() {
    // Arrange：一个玩家 + 一个 NPC，外加一处手动改动过的地形（不是
    // WorldState::new 预热时的原始噪声结果），逼真于真实游玩场景。
    let (mut world, mut registry, terrain_ids) = world_with_registry();
    let farmer = registry.intern(id("lostland:farmer"));
    let human = registry.intern(id("lostland:human"));
    let miner = registry.intern(id("lostland:miner"));
    let brace = registry.intern(id("lostland:brace"));

    let player_pos = world.size.wrap(1, 1);
    let player_zone = world.terrain.layout().tile_to_zone(player_pos).0;
    let mut player_agent = bare_agent(player_pos, player_zone);
    player_agent.profession = farmer;
    player_agent.race = human;
    player_agent.wallet = 42;
    // buffs-and-triggers.md 六节：一条生效中的临时属性修正也要能完整
    // 往返——这不是「结构能序列化」的空跑,是「存了能读回同一个世界」
    // 这条要求本身,`ActiveStatModifier` 与它的来源（brace）都必须经过
    // save_to_file -> load_full 这条真实链路（含 remap_active_stat_modifiers）
    // 后原样保留,hash() 逐位相等的断言（见下）才有意义。
    player_agent.active_stat_modifiers.insert(
        ll_world::entity::AttributeKind::Constitution,
        std::collections::BTreeMap::from([(
            brace,
            ll_world::entity::ActiveStatModifier {
                delta: 3,
                expires_at: Tick(150),
            },
        )]),
    );
    let player = world.actors.spawn(player_agent);
    world.player_entity = Some(player);

    let npc_pos = world.size.wrap(2, 1);
    let npc_zone = world.terrain.layout().tile_to_zone(npc_pos).0;
    let mut npc_agent = bare_agent(npc_pos, npc_zone);
    npc_agent.profession = miner;
    npc_agent.race = human;
    world.actors.spawn(npc_agent);

    // 手动改动一格地形——把它设为草地，与出生点邻域预热出的原始噪声
    // 结果不同（预热范围内多为深水/浅水，见 warm_spawn_neighborhood），
    // 验证「玩家改过的地形」这类偏差本身也会往返保留，不只是噪声生成
    // 的原始结果。
    let edited_pos = world.size.wrap(3, 1);
    world.terrain.set_terrain(edited_pos, terrain_ids.grass);

    let content_index_map = snapshot_for_header(&registry);
    let header = sample_header(content_index_map.clone(), SaveMode::Permadeath);
    let hash_before = world.hash();
    let path = temp_path("full-cycle");
    save_to_file(&path, &header, &world).expect("写出应当成功");

    // Act：mod 集合原样未变——当前会话 registry 按存档头
    // content_index_map 同样顺序重建，索引分配与写出时一致。
    let current_registry =
        rebuild_from_header(&content_index_map).expect("content_index_map 本测试自己产出，恒合法");
    let (_terrain_ids, current_terrain_table) = base_terrain_fixture();
    let outcome = load_full(&path, &current_registry, &[], current_terrain_table, &[]);

    // Assert
    match outcome {
        LoadOutcome::Playable(loaded_world) => {
            assert_eq!(loaded_world.hash(), hash_before);
            // 逐字段核实——不只依赖哈希相等（哈希本身也是本批次改动的
            // 一部分，单靠它会形成「用同一份可能有缺陷的代码验证自己」
            // 的循环论证）：读回的玩家身上，力量属性上 brace 这个来源的
            // 修正必须原样还在，delta/expires_at 都不变。
            let reloaded_player = loaded_world
                .actors
                .get(player)
                .expect("玩家实体读档后必然仍存在");
            let reloaded_modifier = reloaded_player
                .active_stat_modifiers
                .get(&ll_world::entity::AttributeKind::Constitution)
                .and_then(|per_source| per_source.get(&brace));
            assert_eq!(
                reloaded_modifier,
                Some(&ll_world::entity::ActiveStatModifier {
                    delta: 3,
                    expires_at: Tick(150),
                })
            );
        }
        other => panic!("期望 Playable，实际 {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn 存档后卸载一个曾贡献内容的mod读档后被硬门禁拒绝而不崩溃() {
    // 决策二（项目所有者拍板：「存档的 mod 如果不存在或者版本对不上
    // 就不能进入这个存档」）推翻了这条测试曾经验证的行为——P5-A 任务
    // 14 断链二修复时，「完整卸载一个曾贡献内容的 mod」曾经被特意放行
    // 给 `remap_world` 按内容类型细粒度降级为只读；决策二明确要求这个
    // 场景一律拒绝进入存档，不再给细粒度降级机会。本测试因此改为验证
    // 「不崩溃」的新形态：拒绝是显式的、可诊断的
    // （`LoadOutcome::Rejected(LoadError::ModSetMismatch)`，错误信息
    // 指明了具体是哪个 mod、要什么版本、当前是什么版本），不是 panic
    // 或静默产出一个不自洽的世界。
    // Arrange
    let (mut world, mut registry, _terrain_ids) = world_with_registry();
    let vanished_race = registry.intern(id("vanishedmod:ghost_race"));
    let vanished_content_hash = registry
        .content_hash_of("vanishedmod")
        .expect("刚刚 intern 过，必有内容哈希");
    let pos = world.size.wrap(1, 1);
    let zone = world.terrain.layout().tile_to_zone(pos).0;
    let mut npc = bare_agent(pos, zone);
    npc.race = vanished_race;
    world.actors.spawn(npc);

    let content_index_map = snapshot_for_header(&registry);
    let mut header = sample_header(content_index_map, SaveMode::Permadeath);
    header
        .generation_mods
        .push(ll_content::header::ModHeaderEntry {
            namespace: "vanishedmod".to_string(),
            version: "0.1.0".to_string(),
            content_hash: Some(vanished_content_hash),
        });
    let path = temp_path("mod-unload");
    save_to_file(&path, &header, &world).expect("写出应当成功");

    // Act：当前会话完全没有装载 vanishedmod（manifests 里也找不到它——
    // 玩家把它整个卸载了，只装载了本体地形）。
    let (current_registry, terrain_table) = current_session_registry_with_terrain();
    let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);

    // Assert：硬门禁拒绝，且错误信息指明了缺的是哪个 mod、要什么版本。
    match outcome {
        LoadOutcome::Rejected(ll_content::load_error::LoadError::ModSetMismatch(detail)) => {
            assert_eq!(detail.namespace, "vanishedmod");
            assert_eq!(detail.required_version, "0.1.0");
            assert_eq!(detail.current_version, None);
        }
        other => panic!("期望 Rejected(ModSetMismatch)，实际 {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn 模式2存档降级为模式3后存档读档模式仍是自由读档且标记为真() {
    // Arrange：一份模式2（纯永久死亡）存档，先写一次。
    let (world, registry, _terrain_ids) = world_with_registry();
    let content_index_map = snapshot_for_header(&registry);
    let permadeath_header = sample_header(content_index_map.clone(), SaveMode::Permadeath);
    let path = temp_path("mode-downgrade");
    save_to_file(&path, &permadeath_header, &world).expect("写出应当成功");

    // Act：玩家选择降级为模式3，重新存档（覆盖同一份存档，与模式2
    // 「仅保留断点续玩存档」的既有语义一致）。
    let downgraded_mode = SaveMode::Permadeath
        .downgrade()
        .expect("Permadeath 必然可以降级");
    let downgraded_header = sample_header(content_index_map, downgraded_mode);
    save_to_file(&path, &downgraded_header, &world).expect("重新写出应当成功");

    // Assert：只读头部（不触发主体解压）就能看到模式已经是自由读档，
    // 且降级标记为真。
    let loaded_header = load_from_header_only(&path).expect("读头部应当成功");
    assert!(matches!(loaded_header.mode, SaveMode::FreeSave { .. }));
    assert!(loaded_header.mode.was_downgraded_from_permadeath());

    // 不可逆——即便存档往返之后，也没有任何路径能把这个标记「升级」
    // 回模式2。
    assert_eq!(loaded_header.mode.downgrade(), None);
    let _ = std::fs::remove_file(&path);
}
