//! 第三条确定性黄金基准：**有人、有城、有物、有势力**的世界。
//!
//! # 为什么需要第三条（既有两条各自守着哪一段）
//!
//! 本仓库此前有两条黄金基准，各自守着自己那一段，两条都有价值：
//!
//! - `crates/ll-world/tests/determinism.rs` 的 `EXPECTED_WORLD_DIGEST`
//!   守**地形生成**：固定种子的 48×48 世界逐格地形必须跨平台逐位相同。
//! - `crates/ll-sim/tests/replay.rs` 的 `EXPECTED_REPLAY_DIGEST`
//!   守**事件重放**：固定意图流跑完之后的世界必须跨平台逐位相同。
//!
//! 但两条的世界都是**测试里手搓出来的**：前者由
//! `WorldState::new(..)` 直接构造之后一行都不再动（零 `actors.spawn`、
//! 零 `stamp_settlement`、零 `ItemStack`、零势力）；后者由 `setup` 手写
//! 两个 `Agent`，`inventory`/`equipment`/`affiliations` 全空、
//! `ground_items` 从未被写过、一次据点铺设都不发生。
//!
//! 于是「据点真的铺进地形」「名册真的物化成 NPC」「家具真的带着归属躺在
//! 地上」「势力表真的装着编年史折叠出来的那份内容」这一整块——也就是
//! **玩家实际会走进去的那个世界**——不在任何一条黄金基准的判据里。
//!
//! # 这不是一次推断，是一份被反复撞到的公开记录
//!
//! `EXPECTED_WORLD_DIGEST` 的文档注释里累积了**六条**「本批次没有重冻／
//! 只重冻了一个空表的长度标记，同一条理由」的记录：等级与经验、装备栏位、
//! 角色创建（`Agent::gender`）、归属（`ItemStack::owner`）、势力播种、
//! 据点建筑类型（街道与家具）。
//!
//! **每一条单独看都是诚实的核实结论**——写它们的人都做了反例验证，都没
//! 有把「跑一遍没红」当成「覆盖到了」。但六条叠在一起，它们是**同一个
//! 空洞被六个互不相干的批次反复撞到，每次都在同一个位置停下来写一句
//! 「本条够不到」，然后各自走开**。本文件就是去把那个洞补上。
//!
//! 空洞的实测量化（哪些类别在既有两条的保护内、哪些在外）见
//! `docs/superpowers/plans/2026-08-31-batch22-populated-baseline.md` 第二节。
//!
//! # 与既有两条的关系：**新增，不替代**
//!
//! 既有两条常量一个字都没动。它们各自守的那一段仍然由它们守着，而且它们
//! 比本条**便宜得多**（不装载 mods、不跑编年史），日常改动先撞到的应该是
//! 它们。本条是最外面那一层网，代价也最高——见
//! [`EXPECTED_POPULATED_WORLD_DIGEST`] 文档「这条基准红了怎么办」一节。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 与 `npc_materialization.rs`/`fog_of_war.rs` 同一条纪律：全程不碰 GPU、
//! 不模拟键盘，直接调用生产路径上的那几个函数，只是跳过了它们外面那层
//! 窗口/输入外壳。

use ll_game::content::LoadedContent;
use ll_game::world::{GameWorld, STREAM_RADIUS_ZONES, build_new_world};
use ll_mod::roster::SettlementRoles;
use ll_world::generate::GenParams;
use ll_world::ownership::Owner;
use ll_world::settlement::SettlementStatus;

/// 本条基准的世界种子。固定值，让「这个世界里有一座还有人住的据点」这条
/// 前提可复现；换种子等于换一个世界，必须重冻常量。
const SEED: u64 = 20260831;

/// 测试用内容装载——走与本体二进制完全相同的通道，`mods_root` 指向仓库
/// 真实的 `mods/` 目录（本体内容住在那里，临时空目录下契约解析会正确地
/// 失败，见 `ll_mod::base_contract` 模块文档）。写法与
/// `npc_materialization.rs`/`fog_of_war.rs` 的同名帮手一致；集成测试之间
/// 看不见彼此的私有帮手，因此这几行在这里重来一遍。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ll-game-populated-determinism-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 造出本条基准的世界：**每一步都是生产路径上的那一个函数**，测试自己
/// 一次都不直接写 `WorldState` 的任何字段。
///
/// ```text
/// ll_game::content::load_content(仓库真实 mods/)     ← 与本体二进制同一条通道
///   ↓
/// ll_game::world::build_new_world(content, GenParams { seed, ..default })
///   ├─ 地形噪声 + WorldState::new
///   ├─ WorldChronicle::generate           ← 三百年兴衰：据点、战争、占领
///   ├─ world.factions = chronicle.factions()  ← 势力播种（真实折叠结果）
///   ├─ SurfaceStore::install_chronicle     ← 据点真的被 stamp_settlement 铺进地形
///   └─ spawn_player                       ← 第一个真实 Agent
///   ↓
/// SurfaceStore::stream_neighborhood(据点锚点)  ← 生产路径每帧都在调的那一个
///   ↓
/// ll_game::world::materialize_nearby_settlements
///   ├─ ll_mod::roster::settlement_roster   ← 走 DetRng::for_entity（约束 C3）
///   ├─ ll_mod::roster::build_npc_agent     ← 若干真实 Agent，带文化 affiliations
///   └─ furnish_settlement                  ← 若干 GroundItemStack，
///                                            owner = Owner::Faction(site.id)、placed
/// ```
///
/// # 为什么「把邻域挪到据点上」而不是「让玩家走过去」
///
/// 走过去要几百步真实结算，而本文件验的不是移动。`stream_neighborhood`
/// 是生产路径上每帧都在调的那一个函数（`Demo::maintain_streaming`），
/// 直接对着据点锚点调一次，与玩家真的走到那里之后的常驻集合是同一个
/// 东西。写法与 `npc_materialization.rs` 的同名帮手一致。
///
/// # 为什么只物化**一座**据点
///
/// 编年史在全世界铺出两百多座据点，全部物化要把两百多个区块拉进内存，
/// 耗时与内存都不可接受。一座据点已经足以让四类对象全部非空——这一点
/// 不靠信任，靠 [`有人有城有物有势力的世界摘要跨平台稳定`] 里那组**存在性
/// 断言**钉死。取几座是可调的，取一座是最保守、最容易反转的那一档。
fn populated_world() -> GameWorld {
    let content = test_content();
    let mut game_world = build_new_world(
        &content,
        GenParams {
            seed: SEED,
            ..GenParams::default()
        },
    )
    .expect("建世界应当成功");
    let roles = SettlementRoles::resolve(
        &content.registry,
        &content.class_table,
        &content.resource_table,
        &content.culture_table,
    );

    let anchor = {
        let chronicle = game_world
            .world
            .terrain
            .chronicle_handle()
            .expect("新游戏必然装了编年史");
        chronicle
            .sites()
            .iter()
            .find(|site| site.status == SettlementStatus::Inhabited && site.population > 0)
            .expect("三百年历史必然留下至少一座还有人住的据点")
            .anchor
    };

    let clock = game_world.world.clock;
    game_world.world.terrain.stream_neighborhood(
        &game_world.noise,
        &game_world.params,
        &content.terrain_ids,
        anchor,
        STREAM_RADIUS_ZONES,
        clock,
    );
    ll_game::world::materialize_nearby_settlements(&mut game_world.world, &content, &roles);
    game_world
}

/// 由本批次首次运行记录、并在**两个独立进程**里复现的黄金基准。
///
/// # 它守的是什么（与另外两条常量的分工）
///
/// 一个**玩家真的会走进去**的世界：据点已经铺进地形、名册已经物化成
/// `Agent`、家具已经带着 `Owner::Faction` 躺在地上、势力表装着编年史
/// 折叠出来的真实内容。既有两条的世界里这四类对象**一个都不存在**
/// （见本文件模块文档）。
///
/// # 这条基准红了怎么办
///
/// **先分清是哪一种红。**
///
/// - **摘要值变了**（`assert_eq` 的 left/right 不同）：这条基准的覆盖面
///   比另外两条宽得多，因此它**本来就会更频繁地红**——动内容表（`mods/`
///   下任何影响名册、家具、职业、种族的数据）、动据点布局
///   （`ll_world::settlement`）、动编年史推演、动 `Agent` 的任何字段、
///   动 `WorldState::hash` 的任何一行，它都会红。**这是它该有的行为，
///   不是缺陷**——正因为它对这些东西敏感，那些改动才第一次有了自动化
///   兜底。按下面的四步重冻。
/// - **存在性断言红了**（「据点数 > 0」那几条之一）：**不要重冻**。
///   它说的是这个世界里某一类对象消失了——要么是种子/世界生成/编年史
///   参数变了导致这个种子不再有还有人住的据点，要么是物化路径本身坏了。
///   先查清对象为什么不见了；如果是前者，换种子并在本文档记下换的理由，
///   然后按四步重冻。**绝不允许把断言删掉让它变绿**——那会让这条基准
///   退化成又一条空基准，也就是它当初被造出来要补的那个洞。
///
/// ## 四步重冻流程（交接文档纪律第 2 条，一步都不能少）
///
/// 1. **确认基线红**：改动落地、常量还没动，跑
///    `cargo test -p ll-game --test populated_determinism`，
///    记下 `left`（新值）与 `right`（旧值）两个数。
/// 2. **把改动关掉，确认精确回到旧值**：把这次改动（只有这一处，其余
///    改动全部保留）临时注掉/还原，再跑一次，摘要必须**精确等于**旧
///    常量。**这一步才是真正的证据**——它证明摘要的变化只来自这一处
///    改动，不是别的什么顺手平移了索引。跳过这一步，第 1 步拿到的
///    新值就只是「实际输出」，把它抄进常量等于删掉这道防线。
/// 3. **恢复**改动。
/// 4. **新常数在两个独立进程里复现**：跑两次彼此独立的
///    `cargo test -p ll-game --test populated_determinism` 进程，
///    两次拿到同一个新值，才把它写进常量。
///
/// 四步的证据写进提交信息或本常量的文档注释，不要只写在别处的报告里。
///
/// ## 顺带：另外两条常量在哪
///
/// **不要在任何文档里找它们的值**（同一张表已经害过三个互不相干的代理，
/// 见 `knowledge/handoff/2026-08-28-session-handoff.md` 第〇节）。跑：
///
/// ```bash
/// grep -rn "const EXPECTED_" crates/ll-world/tests/determinism.rs \
///   crates/ll-sim/tests/replay.rs crates/ll-game/tests/populated_determinism.rs
/// ```
const EXPECTED_POPULATED_WORLD_DIGEST: u64 = 960_808_593_865_190_740;

#[test]
fn 有人有城有物有势力的世界摘要跨平台稳定() {
    // Arrange
    let game_world = populated_world();
    let world = &game_world.world;

    // Assert（存在性）：**必须排在摘要断言之前。**
    //
    // 本会话反复出现的假绿形状是「断言恒绿，因为被断言的对象根本不存在」
    // ——`EXPECTED_WORLD_DIGEST` 走的正是这条路：它对 `Agent`/据点/物品/
    // 势力的任何改动都天然免疫，因为它的世界里这四类对象一个都没有，而
    // 它仍然年复一年地绿着，看起来像是有一条基准在守着。
    //
    // 这条基准的**全部意义**就是「对象真的存在」。没有下面这几行，某次
    // 重构（改了种子、改了编年史参数、改坏了物化路径）会让这个世界悄悄
    // 变空，而摘要照样能被重冻成一个新值、照样绿——这条基准就退化成了
    // 又一条空基准，而且没有任何人会发现。ADR 0022：覆盖不全的守护等于
    // 没有守护。
    assert!(
        !world.materialized_settlements.is_empty(),
        "这条基准的世界里必须真的有已物化的据点——没有就说明据点铺设/物化路径断了，\
         此时摘要断言即使绿也毫无意义（它守着一个空世界）"
    );
    let actors = world.actors.iter().count();
    assert!(
        actors > 1,
        "这条基准的世界里必须有玩家之外的 NPC（实际 {actors} 个实体）——\
         `Agent` 的每一个字段都只在 `for agent in self.actors.iter()` 循环体内\
         参与哈希，实体数为 1（只有玩家）就意味着覆盖面退化"
    );
    assert!(
        !world.ground_items.is_empty(),
        "这条基准的世界里必须有地面物品——`write_item_stack` 的三个调用点\
         全部在循环体内，一个 `ItemStack` 都没有时它对物品的任何改动都免疫"
    );
    assert!(
        world.ground_items.iter().any(|item| item.placed),
        "地面物品里必须有**放置**的家具——`GroundItemStack::placed` 只有在\
         真的摆过家具时才可能为真"
    );
    assert!(
        world
            .ground_items
            .iter()
            .any(|item| matches!(item.stack.owner, Owner::Faction(_))),
        "地面物品里必须有带归属的——`ItemStack::owner` 的非 `Unowned` 变体\
         此前在两条既有基准里一次都没被构造过（见归属批次在\
         `EXPECTED_WORLD_DIGEST` 文档里留下的那条记录）"
    );
    assert!(
        !world.factions.is_empty(),
        "这条基准的世界里必须有势力——空势力表只往哈希里写一个长度 0 标记，\
         势力播种批次已经如实记录过『不假装它覆盖了势力的内容』"
    );
    assert!(
        world
            .actors
            .iter()
            .any(|agent| !agent.affiliations.is_empty()),
        "必须有至少一个带归属的 `Agent`——`write_affiliation` 只在\
         `agent.affiliations` 非空时被调用"
    );

    // Assert（摘要）
    assert_eq!(world.hash(), EXPECTED_POPULATED_WORLD_DIGEST);
}
