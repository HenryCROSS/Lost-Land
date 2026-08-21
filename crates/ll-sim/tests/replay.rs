//! 确定性回归：Intent 流重放。
//!
//! 本文件里的期望值是**黄金基准**：它由算法定义唯一确定，在 Windows
//! 与 Linux 上必须逐位相同。
//!
//! # 测试失败意味着什么
//!
//! 若某次改动让这里的摘要变了，只有两种可能：
//!
//! 1. 有意修改了算法或常量——那么更新期望值，并在提交信息里说明为什么。
//! 2. **无意引入了平台相关行为**（最常见的是浮点运算，或依赖了哈希表
//!    的遍历顺序）。这是必须立刻修复的缺陷。
//!
//! **绝不允许「测试挂了就把期望值改成实际值」**——那等于删掉这道防线。
//! 与 `crates/ll-core/tests/determinism.rs` 保持同一条规矩。
//!
//! # 这是 P3 最有价值的交付物
//!
//! 记录「世界种子 + Intent 流」即可完整复现一局：玩家报告缺陷时发来
//! 存档与操作记录，本地一按就复现，是排查 Roguelike 缺陷最强的武器，
//! 也是模式 3 自由读档正确性的最终验证。

use ll_core::ident::{Interner, NamespacedId};
use ll_core::time::Tick;
use ll_core::torus::TorusSize;
use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_sim::intent::{Direction, Intent};
use ll_sim::resolve::resolve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
use ll_world::zone::ZoneLayout;

/// 测试用区块布局：边长 64，单个区块——是噪声格点周期的整数倍，满足
/// [`WorldState::new`] 的前置条件（与本仓库其余测试同一常量），整个
/// 测试世界（玩家/敌人/门都落在这个范围内）落在这一个区块内。
fn test_layout() -> ZoneLayout {
    let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
    ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
}

/// 搭一个带两个固定实体、固定地形的世界：玩家站在 `(10, 10)`，敌人站
/// 在远离玩家移动路径的 `(20, 20)`（避免移动测试意外撞上敌人所在格——
/// `resolve` 目前不处理「移动目的地站着别的实体」，见 `resolve.rs`
/// 模块文档「已知的范围边界」一节）。
///
/// 玩家移动路径上的几格地形被显式改写（草地/浅水/关着的门），使这条
/// Intent 流的结果不依赖噪声地形生成算法的具体输出——即便地形生成算法
/// 未来调整，这里的黄金基准也不会跟着漂移。
///
/// 两个实体总是**先玩家后敌人**依次 spawn，且都在同一个新建的
/// `Interner` 上登记同一个种族标识符——两次独立调用本函数（用于比较
/// 「同一流程跑两遍」）在这些方面完全没有分支或非确定输入，因此产出
/// 逐位相同的初始世界。
fn setup(seed: u64) -> (WorldState, EntityId, EntityId) {
    let params = GenParams {
        seed,
        ..GenParams::default()
    };
    let (terrain_ids, terrain_table): (BaseTerrainIds, _) = base_terrain_fixture();
    let layout = test_layout();
    let spawn = layout.tile_size().wrap(0, 0);
    let mut world = WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn)
        .expect("测试布局满足全部构造前置条件");

    world
        .terrain
        .set_terrain(world.size.wrap(11, 10), terrain_ids.grass);
    world
        .terrain
        .set_terrain(world.size.wrap(11, 9), terrain_ids.shallow_water);
    world
        .terrain
        .set_terrain(world.size.wrap(12, 9), terrain_ids.grass);
    world
        .terrain
        .set_terrain(world.size.wrap(13, 9), terrain_ids.door_closed);

    let mut interner = Interner::new();
    let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
    let player_profession =
        interner.intern(NamespacedId::parse("lostland:warrior").expect("合法标识符"));
    let enemy_profession =
        interner.intern(NamespacedId::parse("lostland:bandit").expect("合法标识符"));

    // 两个实体的 current_space 都取地表——本文件的黄金基准只演练
    // Move/Attack/OpenDoor/Wait 这批既有 Intent，不涉及进出 Interior
    // （那部分留给 ll-sim/src/resolve.rs 的任务 12 单元测试覆盖），层
    // 属性索引因此可以是占位值，不影响这条 Intent 流的结算逻辑。
    let player_pos = world.size.wrap(10, 10);
    let (player_zone, _) = world.terrain.layout().tile_to_zone(player_pos);
    let player = world.actors.spawn(Agent {
        pos: player_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: player_profession,
        goals: Vec::new(),
        race,
        luck: 0,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(player_zone, ll_core::ident::ContentIndex::default()),
        script_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
    });
    // 探索记忆写入路径（`resolve_move` 追加的 `Effect::MarkExplored`）
    // 按 `player_entity` 区分「谁在动」，只给玩家标记探索——见其文档
    // 「为什么只有玩家移动才追加」一节。不赋值这里，`intent_stream`
    // 里全部的 `Intent::Move { actor: player, .. }` 都不会产出任何
    // `MarkExplored` 效果，这条黄金基准就测不出写入路径本身，见本文件
    // `EXPECTED_REPLAY_DIGEST` 文档「本次重冻的原因（探索记忆写入路径
    // 批次）」一节。
    world.player_entity = Some(player);
    let enemy_pos = world.size.wrap(20, 20);
    let (enemy_zone, _) = world.terrain.layout().tile_to_zone(enemy_pos);
    let enemy = world.actors.spawn(Agent {
        pos: enemy_pos,
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: enemy_profession,
        goals: Vec::new(),
        race,
        luck: 0,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: std::collections::BTreeMap::new(),
        spent_slots: std::collections::BTreeMap::new(),
        inventory: Vec::new(),
        resting: None,
        unlocked_skills: Vec::new(),
        skill_cooldowns: std::collections::BTreeMap::new(),
        subclasses: Vec::new(),
        active_stat_modifiers: std::collections::BTreeMap::new(),
        current_space: Space::surface(enemy_zone, ll_core::ident::ContentIndex::default()),
        script_state: std::collections::BTreeMap::new(),
        creature_kind: None,
        spawned_at: ll_core::time::Tick(0),
        remembered_id: None,
        level: ll_world::entity::Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
    });

    (world, player, enemy)
}

/// 固定的 Intent 流：先往东、往北各走一步（分别踩上草地与浅水，验证
/// 分级 move_cost 接线正确），等待、攻击敌人一次，再往东走进一扇关着
/// 的门（先由 `Move` 派生出开门效果，再显式 `OpenDoor` 确认幂等地
/// 停在原地，最后借开出的门再往东走一步），最后原地等待收尾。
///
/// 混合了 `Move`/`Wait`/`Attack`/`OpenDoor` 四种 Intent，且后续几步
/// 依赖前面几步的效果已经落地（走进门之前门必须已经被撞开）——足以
/// 暴露「结算顺序被打乱」「某个效果没有真的落地」这类缺陷。
///
/// # 曾经的覆盖缺口（本次修复）
///
/// 这份 Intent 流一度只在玩家走到 `(12,9)`（门前一格）后直接下发显式
/// `OpenDoor`——门是被这条显式意图第一次打开的，`resolve_move` 里
/// 「撞门派生开门效果」那条分支（`opens_into` 分支，见 `resolve.rs`）
/// 从未在这个文件里被真正走到过，与本文档「先由 `Move` 派生出开门
/// 效果」的描述不符：显式 `OpenDoor` 在那种排布下做的是「第一次真正
/// 开门」的工作，而不是文档所说的「确认幂等地停在原地」。现在补上了
/// 一步专门撞向关着的门的 `Move`（下方第 6 个 Intent）：这一步走
/// `opens_into` 分支，产出 `SetTerrain` 而非 `MoveTo`，玩家停在
/// `(12,9)` 不动；紧随其后的显式 `OpenDoor` 这才是真正的「门已经开了,
/// 再开一次确认没有副作用」——与文档描述完全对齐，且顺带让这个分支
/// 第一次被这份黄金基准真正覆盖到。
fn intent_stream(player: EntityId, enemy: EntityId) -> Vec<Intent> {
    vec![
        Intent::Move {
            actor: player,
            dir: Direction::East,
        },
        Intent::Move {
            actor: player,
            dir: Direction::North,
        },
        Intent::Wait { actor: enemy },
        Intent::Attack {
            actor: player,
            target: enemy,
        },
        Intent::Move {
            actor: player,
            dir: Direction::East,
        },
        // 撞向关着的门：resolve_move 的 opens_into 分支派生出开门效果，
        // 玩家原地不动（见上方模块文档「曾经的覆盖缺口」一节）。
        Intent::Move {
            actor: player,
            dir: Direction::East,
        },
        Intent::OpenDoor {
            actor: player,
            pos: (13, 9),
        },
        Intent::Move {
            actor: player,
            dir: Direction::East,
        },
        Intent::Wait { actor: player },
    ]
}

/// 依次把一串 Intent 结算、应用到世界上——`resolve` 产出 `Effect`，
/// `apply` 落地，前一个 Intent 的效果必须在下一个 Intent 结算前就已
/// 可见（`resolve` 读的是 `&WorldState`，是当时那一刻的真实状态）。
fn play(world: &mut WorldState, intents: &[Intent]) {
    for intent in intents {
        let effects = resolve(world, intent);
        for effect in &effects {
            apply(world, effect);
        }
    }
}

/// 由首次运行记录的黄金基准。修改前请阅读本文件顶部说明。
///
/// # 本次重冻的原因（两级坐标系重写，任务 11）
///
/// 与 `crates/ll-world/tests/determinism.rs` 同一个理由：`WorldState.terrain`
/// 从 `ChunkGrid` 换成按区块流式生成与常驻的 `SurfaceStore`，
/// `WorldState::hash` 相应地从「按 `size` 遍历世界每一格」改为「按
/// `resident_zones()` 排序后的区块坐标集合遍历，且额外把区块坐标本身
/// 混入哈希」（见 `WorldState::hash` 文档「不再遍历整个世界的每一格」）。
/// `setup` 里的测试世界仍然只有一个区块（64×64 单区块布局），地形/
/// 实体内容与迁移前完全相同，但哈希算法本身的输入构造方式变了，产出
/// 的摘要数值随之改变——这是「基准值变了」，不是「断言结构变了」：
/// 仍然是「同一份意图流产出同一个摘要」这条断言，只是摘要的计算方式
/// 换了输入顺序。人工核对：迁移前后分别跑通本文件另外三条测试（相同
/// 种子相同哈希、不同意图流不同哈希、序列化往返一致）全部保持通过，
/// 证明哈希仍然对种子/意图流/序列化敏感，不是退化成常量。
///
/// # 第二次重冻的原因（`TileableNoise` 大陆尺度层退化修复）
///
/// `TileableNoise::new` 改用 `safe_coarse_scale` 而非直接取 `gcd` 的
/// 最大二次幂因数（见 `ll_world::noise` 模块文档「一个更隐蔽的退化」）
/// ——本文件的测试世界是 64×64 单区块，换算出的格点周期恰好是
/// `period_x = period_y = 4`，正好命中那个退化条件，修复前后大陆
/// 尺度层的取值不同，世界背景地形（`setup` 里没有显式 `set_terrain`
/// 覆盖的格子）随之改变，`world.hash()` 自然跟着变。
///
/// 同一次提交里 `intent_stream` 也补上了一步撞向关着的门的 `Move`
/// （见其文档「曾经的覆盖缺口」一节）——但这**没有**让摘要再变一次：
/// 补的那一步与它取代的显式 `OpenDoor` 做的是同一件事（把门从
/// `door_closed` 改写成 `door_open`），而 `ScheduleNext` 覆盖式写入
/// `next_action_at`（`schedule_after` 只读 `world.clock`，本文件的
/// 意图流从不推进它），补一步进去只是把「谁先把门打开」的归属换了个
/// 位置，最终世界状态（地形/位置/时钟）逐位不变——这也是为什么后面
/// 那个「真正打开门的显式 OpenDoor 现在变成了空操作」的重构本身是
/// 安全的：世界状态不因为顺序调整而漂移。
///
/// # 任务 12 的第二次变更：`hash()` 新增混入 `current_space`
///
/// `WorldState::hash` 现在额外遍历每个存活实体的 `current_space`（见
/// 其文档「厚层实体也参与摘要」一节新增的段落）。**这条 Intent 流
/// 全程没有任何 `EnterSpace`/`ExitSpace`——两个实体的 `current_space`
/// 从 `setup` 到结束恒为地表，数值本身没有变化**，摘要仍然改变，
/// 是因为 `hash()` 现在多混入了这几个此前完全不参与摘要的字段（哪怕
/// 它们的值本身没变，混入顺序本身就会让最终摘要不同），这与「地形/
/// 位置/时钟没有变化」不矛盾。人工核验：把 `write_space` 的调用临时
/// 注释掉重新跑这条测试，摘要回到迁移前的旧常量
/// `6_078_574_347_230_641_570`——确认这次变化完全、只来自
/// `current_space` 被纳入摘要这一处改动，没有引入其他意外的行为漂移。
///
/// # 第三次重冻的原因（P5 批次 B：`population`/`actors` 摘掉 `#[serde(skip)]`）
///
/// `WorldState::hash` 现在额外把每个存活实体的 `stats`/`profession`/
/// `race`/`luck`/`affiliations`/`goals` 混入摘要（见其文档新增段落）
/// ——`setup` 里两个实体的 `profession`/`race` 是从真实 `Interner`
/// 登记出来的非零 `ContentIndex`，`stats` 取 `BaseStats::BASELINE`
/// （六项均为 10，非零），因此这六项加入摘要必然改变数值。人工核验：
/// 把这六行新增的混入调用临时注释掉重新跑这条测试，摘要回到本次重冻
/// 之前的旧常量 `16_465_209_158_075_336_802`——确认这次变化完全、只
/// 来自这一处改动，没有引入其他意外的行为漂移（与上面两次重冻同一套
/// 核验方法）。`affiliations`/`goals` 在这份测试夹具里恒为空
/// `Vec`（`setup` 从不填充），只贡献一次长度为零的写入，真正让摘要
/// 改变的是 `stats`/`profession`/`race`/`luck` 四项标量。
///
/// # 第四次重冻的原因（脚本状态存储批次，裁定 P5-9）
///
/// `WorldState::hash` 新混入 `player_entity`（`Option<EntityId>`）与
/// `global_script_state`——本文件的 `setup` 从不设置 `player_entity`
/// （恒 `None`）、也从不写脚本状态（恒空），但混入本身仍然改变字节流
/// （`write_optional_entity` 恒写一个判别字节，`write_script_state`
/// 恒写一个长度字节），摘要数值随之改变，与前三次重冻同一个模式：
/// 断言结构不变，只是 `hash()` 的输入构造方式变了。人工核验：把
/// `write_optional_entity`/`write_script_state` 两处新增调用临时注释
/// 掉重新跑这条测试，摘要回到本次重冻之前的旧常量
/// `10_420_841_280_615_735_009`。
///
/// # 第五次重冻的原因（P5-B 任务 5：`Agent` 新增职业/技能相关字段）
///
/// `WorldState::hash` 新混入 `Agent` 的 `mana`/`stamina`/
/// `unlocked_skills`/`skill_cooldowns`/`subclasses`/
/// `active_stat_modifiers` 六个字段（见其文档「职业/技能相关字段也已
/// 混入」一节）——本文件的 `setup` 从不设置这六项（两个实体的
/// `mana`/`stamina` 恒为 `Agent::STARTING_MANA`/`STARTING_STAMINA`，
/// 其余四项恒为空），但混入本身仍然改变字节流（每一项都至少贡献一次
/// 「长度为零」或「固定数值」的写入），摘要数值随之改变，与前四次
/// 重冻同一个模式：断言结构不变，只是 `hash()` 的输入构造方式变了。
///
/// 人工核验（真实执行，不是假设）：把 `crates/ll-world/src/state.rs`
/// 里 `hash()` 新增的那一段（`agent.mana`/`agent.stamina`/
/// `write_content_index_vec` 两次调用/`skill_cooldowns` 循环/
/// `active_stat_modifiers` 循环，共八行）临时删掉重新跑这条测试，摘要
/// 精确回到本次重冻之前的旧常量 `54_795_308_315_924_513`——确认这次
/// 变化完全、只来自这一处改动，恢复后重新跑通再确认新常量
/// `3_120_509_028_390_886_945` 稳定复现。
///
/// # 第六次重冻的原因（落地探索记忆批次）
///
/// `WorldState::hash` 新混入 `self.exploration.write_hash(&mut hasher)`
/// （见 [`ll_world::exploration::ExplorationMemory::write_hash`]）——
/// 本文件的 `setup`/`play` 从不调用
/// [`ll_world::exploration::ExplorationMemory::mark_explored`]，
/// `world.exploration` 全程是一份空记忆，但混入本身仍然改变字节流
/// （`write_hash` 恒先写一个「已探索区块数」的长度字节，即便该数字是
/// 零），摘要数值随之改变，与前五次重冻同一个模式：断言结构不变，只是
/// `hash()` 的输入构造方式变了。
///
/// 人工核验（真实执行，不是假设）：把 `crates/ll-world/src/state.rs`
/// 的 `hash()` 里 `self.exploration.write_hash(&mut hasher);` 这一行
/// 临时删掉重新跑这条测试，摘要精确回到本次重冻之前的旧常量
/// `3_120_509_028_390_886_945`——确认这次变化完全、只来自这一处改动，
/// 恢复后重新跑通再确认新常量 `9_151_147_838_687_915_073` 稳定复现。
///
/// # 第七次重冻的原因（探索记忆写入路径批次）
///
/// 上一批（第六次重冻）只交付了 [`ll_world::exploration::ExplorationMemory`]
/// 的存储与读取，`mark_explored` 没有任何调用方——本文件的 `setup`
/// 因此从不设置 `world.player_entity`（恒 `None`），这条黄金基准测不出
/// 任何写入路径。这一批补上了两处改动，两处都真实改变了摘要：
///
/// 1. `setup` 现在显式赋值 `world.player_entity = Some(player);`——
///    [`ll_sim::resolve::resolve_move`] 新增的 `Effect::MarkExplored`
///    只在移动者是玩家时才追加（见其文档「为什么只有玩家移动才追加」
///    一节），不赋值这个字段，`intent_stream` 里的全部 `Intent::Move`
///    都测不到新写入路径，等于没测。
/// 2. `resolve_move` 在产生 `Effect::MoveTo` 的分支里追加一条
///    `Effect::MarkExplored`，`apply` 落地时据此调用
///    `compute_fov` + `ExplorationMemory::mark_explored`，把玩家沿途
///    看到的格子真正记进 `world.exploration`——`intent_stream` 里三步
///    `Intent::Move` 都会触发，`world.exploration` 不再是一份空记忆，
///    `write_hash` 混入的字节流因此改变。
///
/// 人工核验（真实执行，不是假设，两步改动分开验证）：
///
/// - 只加第 1 项（`player_entity` 赋值），把 `resolve_move` 里
///   `effects.push(Effect::MarkExplored { .. })` 这一行临时注释掉重新
///   跑这条测试，摘要是 `3_069_981_719_783_750_112`——证明「玩家身份
///   被记下来」本身也会改摘要（`player_entity` 从 `None` 变成
///   `Some`，这条字节流本就参与哈希，见第四次重冻）。
/// - 恢复 `MarkExplored` 的追加（两项改动都在），摘要变成
///   `17_575_307_657_617_953_743`——证明真正的探索记忆写入（而不只是
///   `player_entity` 本身）也确实改变了摘要，写入路径不是摆设。
///
/// 两步核验都在本次提交前实际跑过，不是假设的推演。
///
/// # 第八次重冻的原因（击杀与死亡记录批次）
///
/// `WorldState::hash` 新增混入了 `Agent::creature_kind`/`spawned_at`/
/// `remembered_id` 三个字段（每个存活实体一份）与 `WorldState::history`/
/// `next_world_id` 两个字段（见 `ll_world::state::WorldState::hash`
/// 文档同名字段的「参与 hash()」一节）——本文件的 `intent_stream` 没有
/// 让任何一次攻击致死（见 `setup`/`intent_stream` 文档，本用例只覆盖
/// 移动/开门/攻击的普通结算路径，不含 `Effect::Kill`），因此
/// `history`/`next_world_id` 的取值本身没变（仍是空/零）；但新增字段
/// 意味着喂进哈希器的字节流本身变长了，摘要因此改变，这与前几次重冻
/// （新增字段即便取值恒定也会移动摘要，因为哈希覆盖的是"这个字段有没有
/// 被纳入"而不是"这个字段现在的值是不是默认值"）是同一条先例。
///
/// 人工核验（真实执行）：把
/// `crates/ll-world/src/state.rs` 里新增的
/// `write_optional_content_index`/`hasher.write_i64(agent.spawned_at.0)`/
/// `write_optional_world_id(&mut hasher, agent.remembered_id)` 三行与
/// `history`/`next_world_id` 混入的三行临时删掉重新跑这条测试，摘要
/// 精确回到本次重冻之前的旧常量 `17_575_307_657_617_953_743`，恢复
/// 后重新跑通再确认新常量 `6_199_102_875_138_192_911` 稳定复现。
///
/// # 第九次重冻的原因（无名单位击杀计数批次，决策一）
///
/// `WorldState::hash` 新增混入了 `WorldState::kill_counts`（见其文档
/// 「参与 hash()」一节）——本文件的 `intent_stream` 不含任何
/// `Effect::Kill`（同第八次重冻的说明），`kill_counts` 因此恒为空表，
/// 但新增字段意味着喂进哈希器的字节流本身变长了（至少多出一个长度为
/// 零的 `u64`），摘要因此改变，与前几次重冻同一条先例。
///
/// 人工核验（真实执行）：把 `crates/ll-world/src/state.rs` 里 `hash()`
/// 新增混入 `kill_counts` 的那几行临时删掉重新跑这条测试，摘要精确
/// 回到本次重冻之前的旧常量 `6_199_102_875_138_192_911`，恢复后重新
/// 跑通再确认新常量 `11_328_278_044_222_098_927` 稳定复现。
///
/// # 第十次重冻的原因（等级与经验系统落地批次）
///
/// `WorldState::hash` 新增混入了每个 `Agent` 的
/// `level`/`experience`/`xp_to_next_level` 三个字段（ADR 0022「判据
/// 字段不全」的直接施工，见 `crates/ll-world/src/state.rs` `hash()`
/// 对 `self.actors` 遍历新增的三行 `hasher.write_i64`）——本文件
/// `setup` 生成的 `player`/`enemy` 都是真实 `Agent`（不是空表），三个
/// 新字段各自取新增的占位默认值（`level = Agent::STARTING_LEVEL`
/// （1）、`experience = 0`、`xp_to_next_level =
/// Agent::STARTING_XP_TO_NEXT_LEVEL`（100）），喂进哈希器的字节流因此
/// 真的变长、变了内容，摘要随之改变，与前九次重冻同一条先例。
///
/// 人工核验（真实执行，非由脚本自动回填）：
/// 1. 先在改动后的代码上把这条测试单独跑了两次，确认新摘要
///    `13_338_753_139_158_337_327` 在两次独立进程里稳定复现（不是
///    一次性偶然值）。
/// 2. 再把 `state.rs` `hash()` 里新增的三行 `hasher.write_i64(agent.level
///    /.experience/.xp_to_next_level)` 临时注释掉重新跑这条测试，摘要
///    精确回到本次重冻之前的旧常量 `11_328_278_044_222_098_927`（与
///    上面记录的第九次重冻结果一致），证明这三行确实是本次摘要变化
///    的唯一成因。
/// 3. 恢复这三行后重新跑通，再次确认新常量
///    `13_338_753_139_158_337_327` 稳定复现，才把它写进下面的常量。
///
/// # 第十一次重冻的原因（资源池落地批次，第一批：法力池/血池）
///
/// `WorldState::hash` 新增混入了每个 `Agent` 的 `resource_pools`
/// （`BTreeMap<ContentIndex, i32>`，紧邻 `mana`/`stamina` 插入，见
/// `crates/ll-world/src/state.rs` `hash()` 对 `self.actors` 遍历新增的
/// 一段 `hasher.write_u64(agent.resource_pools.len() as u64)` +
/// 逐条遍历）——本文件 `setup` 生成的 `player`/`enemy` 都不持有任何
/// 天赋授予的资源池（`intent_stream` 也没有任何 `Intent::UseSkill`），
/// `resource_pools` 因此恒为空表，但新增字段意味着喂进哈希器的字节流
/// 本身变长了（至少多出一个长度为零的 `u64`），摘要因此改变，与前十次
/// 重冻同一条先例。
///
/// 人工核验（真实执行，非由脚本自动回填）：
/// 1. 先在改动后的代码上把这条测试单独跑了两次，确认新摘要
///    `5_035_886_638_381_990_543` 在两次独立进程里稳定复现（不是一次性
///    偶然值）。
/// 2. 再把 `state.rs` `hash()` 里新增的
///    `hasher.write_u64(agent.resource_pools.len() as u64)` 与紧随其后
///    的 `for (pool, current) in &agent.resource_pools { .. }` 循环临时
///    注释掉重新跑这条测试，摘要精确回到本次重冻之前的旧常量
///    `13_338_753_139_158_337_327`（与上面记录的第十次重冻结果一致），
///    证明这几行确实是本次摘要变化的唯一成因。
/// 3. 恢复这几行后重新跑通，再次确认新常量
///    `5_035_886_638_381_990_543` 稳定复现，才把它写进下面的常量。
///
/// # 第十二次重冻的原因（资源池落地批次，第二批：法术位/休息事件）
///
/// `WorldState::hash` 新增混入了每个 `Agent` 的 `spent_slots`
/// （`BTreeMap<(ContentIndex, u8), u32>`）与 `resting`
/// （`Option<RestState>`），紧邻上一批插入的 `resource_pools` 之后，见
/// `crates/ll-world/src/state.rs` `hash()` 对 `self.actors` 遍历新增的
/// 一段 `hasher.write_u64(agent.spent_slots.len() as u64)` + 逐条遍历，
/// 以及紧随其后对 `agent.resting` 的 `match`——本文件 `setup` 生成的
/// `player`/`enemy` 都不持有任何法术位（`intent_stream` 也没有任何
/// `Intent::Rest`），`spent_slots` 因此恒为空表、`resting` 恒为
/// `None`，但新增字段意味着喂进哈希器的字节流本身变长了（至少多出
/// 一个长度为零的 `u64` 与一个判别用的 `u64`），摘要因此改变，与前
/// 十一次重冻同一条先例。
///
/// 人工核验（真实执行，非由脚本自动回填）：
/// 1. 先在改动后的代码上把这条测试单独跑了两次，确认新摘要
///    `2_505_820_810_245_065_935` 在两次独立进程里稳定复现（不是一次性
///    偶然值）。
/// 2. 再把 `state.rs` `hash()` 里新增的
///    `hasher.write_u64(agent.spent_slots.len() as u64)`/紧随其后的
///    `for ((pool, tier), spent) in &agent.spent_slots { .. }` 循环/
///    `match agent.resting { .. }` 三段临时注释掉重新跑这条测试，摘要
///    精确回到本次重冻之前的旧常量 `5_035_886_638_381_990_543`（与上面
///    记录的第十一次重冻结果一致），证明这三段确实是本次摘要变化的
///    唯一成因。
/// 3. 恢复这三段后重新跑通，再次确认新常量
///    `2_505_820_810_245_065_935` 稳定复现，才把它写进下面的常量。
///
/// # 第十三次重冻的原因（P6 第二批：背包与地面物品）
///
/// `WorldState::hash` 新增混入了每个 `Agent` 的 `inventory`
/// （`Vec<ItemStack>`，紧邻上一批插入的 `remembered_id` 之后）与
/// `WorldState::ground_items`（`Vec<GroundItemStack>`，紧邻
/// `kill_counts` 之后）——见 `crates/ll-world/src/state.rs` `hash()`
/// 对应位置新增的 `hasher.write_u64(agent.inventory.len() as u64)`/
/// 逐条 `write_item_stack` 循环，以及末尾对
/// `self.ground_items` 同一形状的一段。本文件 `setup` 生成的
/// `player`/`enemy` 都不持有任何背包物品，`intent_stream` 也没有任何
/// `Intent::PickUp`/`Intent::Drop`，两个新字段因此恒为空，但新增字段
/// 意味着喂进哈希器的字节流本身变长了（各多出一个长度为零的
/// `u64`），摘要因此改变，与前十二次重冻同一条先例。
///
/// 人工核验（真实执行，非由脚本自动回填）：
/// 1. 先在改动后的代码上把这条测试单独跑了两次（两次独立的
///    `cargo test` 进程），确认新摘要 `5_311_272_733_871_972_559`
///    在两次独立进程里稳定复现（不是一次性偶然值）。
/// 2. 再把 `state.rs` `hash()` 里新增的 `agent.inventory`/
///    `self.ground_items` 两段混入代码临时注释掉重新跑这条测试，摘要
///    精确回到本次重冻之前的旧常量 `2_505_820_810_245_065_935`（与上面
///    记录的第十二次重冻结果一致），证明这两段确实是本次摘要变化的
///    唯一成因。
/// 3. 恢复这两段后重新跑通，再次确认新常量
///    `5_311_272_733_871_972_559` 稳定复现，才把它写进下面的常量。
const EXPECTED_REPLAY_DIGEST: u64 = 5_311_272_733_871_972_559;

#[test]
fn 固定种子与固定意图流的世界哈希跨平台稳定() {
    // Arrange
    let (mut world, player, enemy) = setup(20260817);
    let intents = intent_stream(player, enemy);

    // Act
    play(&mut world, &intents);

    // Assert
    assert_eq!(world.hash(), EXPECTED_REPLAY_DIGEST);
}

#[test]
fn 同一意图流在同一种子下产出相同的世界哈希() {
    // 两个世界各自独立从零构造，只共享种子与 Intent 流——这是「记录
    // 种子加 Intent 流即可完整复现一局」这条承诺的最基本验证。
    // Arrange
    let (mut first_world, first_player, first_enemy) = setup(7);
    let (mut second_world, second_player, second_enemy) = setup(7);

    // Act
    play(&mut first_world, &intent_stream(first_player, first_enemy));
    play(
        &mut second_world,
        &intent_stream(second_player, second_enemy),
    );

    // Assert
    assert_eq!(first_world.hash(), second_world.hash());
}

#[test]
fn 不同意图流产出不同哈希() {
    // 反面验证：若哈希对任何输入都返回同一个值，上一条测试会毫无
    // 意义地恒真。用「只走到攻击那步之前」的变体流确认哈希确实随
    // 输入变化——两个世界的实体标识由确定性 setup 保证完全相同，
    // 故变体流可以直接复用 full_intents 的前缀，不必另起一份。
    //
    // 这里特意不是简单地砍掉流末尾的最后一条 `Wait`：本文件的意图流
    // 从不推进 `world.clock`（`resolve`/`apply` 都不含推进世界时钟的
    // 效果——见 `Effect::ScheduleNext` 文档，它只写 `next_action_at`
    // 这一个字段），连续两次对同一实体排期若算出同一个目标时刻，
    // 后一次只是把前一次的值原样覆盖，不会在哈希里留下痕迹。最初这样
    // 写确实测出了这一点：砍掉末尾 `Wait{player}` 的变体流与完整流
    // 产出了相同的哈希，因为它前一步的 `Move` 已经把玩家的
    // `next_action_at` 算到了与这次 `Wait` 相同的目标时刻。改成砍掉
    // 「攻击」及其后所有步骤，确保变体流里敌人的生命值必然不同，
    // 不依赖任何排期巧合。
    // Arrange
    let (mut full_world, player, enemy) = setup(99);
    let (mut short_world, _, _) = setup(99);
    let full_intents = intent_stream(player, enemy);
    let short_intents = &full_intents[..3];

    // Act
    play(&mut full_world, &full_intents);
    play(&mut short_world, short_intents);

    // Assert
    assert_ne!(full_world.hash(), short_world.hash());
}

#[test]
fn 序列化世界并读回后继续执行同一意图流结果与不中断执行一致() {
    // 这是本文件最关键的一条：同时验证存档完整性与重放确定性。
    //
    // `population`/`actors` 现在真正参与序列化（P5 批次 B，摘掉了
    // `WorldState` 上这两个字段的 `#[serde(skip)]`）——厚层 `Agent` 的
    // `profession`/`race` 是 `ContentIndex`，该类型已经补齐无上下文的
    // 直接 `Serialize`/`Deserialize`（见 `WorldState` 模块文档
    // 「population/actors 现在参与序列化」一节）。因此这里不再需要像
    // 此前那样手动搬运 `actors`/`population`：`serde_json` 序列化再
    // 反序列化整个 `WorldState` 就足以复现「读档后继续」这个场景，
    // 包括两个实体各自的职业/种族/属性都要经过真实的编解码往返。
    //
    // `terrain_table` 仍然不参与序列化（该字段的 `#[serde(skip)]` 不在
    // 本批次范围内，见 `WorldState` 文档），因此仍需手动搬运——这与
    // 「读档后必须由调用方重新灌入当前会话地形表」这条既有设计一致。
    // Arrange
    let (mut uninterrupted, player, enemy) = setup(2026);
    let full_intents = intent_stream(player, enemy);
    play(&mut uninterrupted, &full_intents);
    let uninterrupted_hash = uninterrupted.hash();

    let (mut live, resumed_player, resumed_enemy) = setup(2026);
    let resumed_intents = intent_stream(resumed_player, resumed_enemy);
    let checkpoint = resumed_intents.len() / 2;
    play(&mut live, &resumed_intents[..checkpoint]);

    // Act：population/actors 现在随整个 WorldState 一起真正序列化，
    // 只有 terrain_table 仍需手动搬运——见上方本测试的说明。
    let encoded = serde_json::to_vec(&live).expect("WorldState 全部字段可序列化");
    let mut reloaded: WorldState =
        serde_json::from_slice(&encoded).expect("刚序列化的数据必然合法");
    reloaded.terrain_table = live.terrain_table.clone();
    play(&mut reloaded, &resumed_intents[checkpoint..]);

    // Assert
    assert_eq!(reloaded.hash(), uninterrupted_hash);
}

#[test]
fn 序列化往返后厚层实体的职业与种族保持原样而非默认值() {
    // 直接对应 P5 批次 B 存在的理由：population/actors 摘掉
    // `#[serde(skip)]` 之前，这条断言根本无法成立——读档后 actors
    // 恒是空的 `Arena::default()`，任何关于职业/种族是否保留的断言都
    // 无从谈起。现在两者真正参与序列化，这里锁定「往返后不是默认值，
    // 而是往返前的真实内容」这条性质，防止将来有人不小心把 `Repr` 里
    // 的字段接回默认值（例如像 `surface_profile` 那样在 `TryFrom` 里
    // 手滑写成 `Arena::default()`）却没有任何测试能抓到。
    // Arrange
    let (world, player, _enemy) = setup(2026);
    let before = world
        .actors
        .get(player)
        .expect("setup 刚 spawn 的玩家标识必然有效")
        .clone();

    // Act
    let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
    let decoded: WorldState = serde_json::from_slice(&encoded).expect("刚序列化的数据必然合法");
    let after = decoded
        .actors
        .get(player)
        .expect("往返后同一个标识必须仍能取到实体");

    // Assert：整个 Agent 逐字段相等（Agent 派生了 PartialEq），职业与
    // 种族这两个此前被 skip 掉的 ContentIndex 字段自然也在其中。
    assert_eq!(after, &before);
}

/// 回归测试：撞向关着的门那一步真的产生了「派生开门」效果，而不是
/// 悄悄落到某个从未被走到的分支。
///
/// 见 `intent_stream` 文档「曾经的覆盖缺口」：修复前这份 Intent 流从
/// 玩家走到 `(12,9)` 后直接下发显式 `OpenDoor`，`resolve_move` 里
/// 「撞门派生开门」的 `opens_into` 分支从未在本文件里被走到过。这里
/// 单独把这一步（intent_stream 里第 6 个 Intent，紧接在显式 OpenDoor
/// 之前的那个 `Move`）结算出来，断言它产出的是 `SetTerrain` 而不是
/// `MoveTo`——门被撞开，人没挪窝。
#[test]
fn 撞向关着的门产生派生开门效果而不是移动效果() {
    // Arrange：把序列跑到「刚走到门前一格」为止（跳过撞门这一步本身）。
    let (mut world, player, enemy) = setup(3);
    let intents = intent_stream(player, enemy);
    let bump_door_index = 5; // 见 intent_stream：第 6 个（0 基下标 5）是撞门的 Move。
    assert!(
        matches!(intents[bump_door_index], Intent::Move { .. }),
        "这个下标本该是撞门的 Move，而不是显式 OpenDoor——\
         若这里断言失败，说明 intent_stream 又退回了「门只靠显式 \
         OpenDoor 打开、opens_into 分支从未被走到」的旧结构"
    );
    for intent in &intents[..bump_door_index] {
        for effect in resolve(&world, intent) {
            apply(&mut world, &effect);
        }
    }
    let before_bump = world.actors.get(player).map(|agent| agent.pos);

    // Act
    let effects = resolve(&world, &intents[bump_door_index]);
    for effect in &effects {
        apply(&mut world, effect);
    }
    let after_bump = world.actors.get(player).map(|agent| agent.pos);

    // Assert：产出的是「改地形」而不是「挪位置」，且玩家确实原地未动。
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::SetTerrain { .. })),
        "撞门这一步没有产生 SetTerrain 效果：{effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::MoveTo { .. })),
        "撞门这一步不该产生 MoveTo 效果：{effects:?}"
    );
    assert_eq!(before_bump, after_bump, "撞门这一步玩家不应该真的挪动位置");
}

/// 回归测试：门打开之后，最后那步「往东走过打开的门」真的产生了
/// `MoveTo` 效果，不是空 `Vec`。
///
/// 这条直接对应本任务要修的问题：曾经有种描述认为这一步的目的地格恒
/// 不可通行、`resolve_move` 因此恒返回空效果。实测并非如此——目的地
/// `(13,9)` 是 `setup` 显式 `set_terrain` 成 `door_closed` 的格子，
/// 不依赖噪声地形生成，本就不受种子影响；这里直接断言这一步的效果
/// 里确实含有 `MoveTo`，把这个结论钉成一条永久回归测试，而不是只靠
/// 黄金哈希间接覆盖。
#[test]
fn 门打开后向东走过打开的门产生真正的移动效果() {
    // Arrange：把序列跑到「门已经打开、人还在门前一格」为止。
    let (mut world, player, enemy) = setup(5);
    let intents = intent_stream(player, enemy);
    let walk_through_index = intents.len() - 2; // 倒数第二条：穿门的 Move，最后一条是收尾 Wait。
    for intent in &intents[..walk_through_index] {
        for effect in resolve(&world, intent) {
            apply(&mut world, &effect);
        }
    }

    // Act
    let effects = resolve(&world, &intents[walk_through_index]);

    // Assert
    let moved_to_door = effects.iter().any(
        |effect| matches!(effect, Effect::MoveTo { pos, .. } if *pos == world.size.wrap(13, 9)),
    );
    assert!(
        moved_to_door,
        "穿门那一步没有产生真正的 MoveTo 效果：{effects:?}"
    );
}
