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
use ll_sim::intent::{Direction, Intent};
use ll_sim::resolve::resolve;
use ll_world::entity::{Agent, BaseStats, EntityId};
use ll_world::generate::GenParams;
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};

/// 测试世界尺寸：64 是噪声格点周期的整数倍，满足 [`WorldState::new`]
/// 的前置条件（与本仓库其余测试同一常量）。
fn test_size() -> TorusSize {
    TorusSize::new(64, 64).expect("64x64 满足整除约束")
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
    let mut world = WorldState::new(test_size(), &params, &terrain_ids, terrain_table)
        .expect("测试尺寸满足全部构造前置条件");

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

    let player = world.actors.spawn(Agent {
        pos: world.size.wrap(10, 10),
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: player_profession,
        goals: Vec::new(),
        race,
        luck: 0,
    });
    let enemy = world.actors.spawn(Agent {
        pos: world.size.wrap(20, 20),
        stats: BaseStats::BASELINE,
        next_action_at: Tick(0),
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: enemy_profession,
        goals: Vec::new(),
        race,
        luck: 0,
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
/// # 本次重冻的原因（P4 Task 8：`TerrainKind` 迁入内容注册表）
///
/// 与 `crates/ll-world/tests/determinism.rs` 同一个理由：地形不再是
/// 硬编码的 `u16` 值，而是注册期按顺序分配的稠密 `ContentIndex`。
/// `WorldState::hash` 混入的地形原始数值编码方式变了，期望值随之
/// 更新为新编码在同一种子、同一意图流下的真实产出。
const EXPECTED_REPLAY_DIGEST: u64 = 16_052_812_501_842_541_409;

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
    // `WorldState` 当前只有 `seed`/`clock`/`size`/`terrain` 四个字段
    // 真正参与序列化——`population`/`actors` 两个字段标着
    // `#[serde(skip)]`，这是 P3 阶段的已知限制，见 `WorldState` 自己
    // 的文档「population/actors 暂不参与序列化」一节：厚层 `Agent` 的
    // `profession`/`race` 是 `ContentIndex`，`ll_core::ident` 模块文档
    // 明确写着这个类型不可持久化（依赖 mod 加载顺序，真正持久化需要
    // 先把它解析回字符串 ID），这是存档格式在 P5 冻结前才会补齐的
    // 内容注册表工作，不在本批次范围内。
    //
    // 因此这里对「读档后继续」的模拟是：让 `seed`/`clock`/`size`/
    // `terrain` 真正走一遍 `serde_json` 序列化再反序列化（验证这四个
    // 字段当前就有的存档能力确实完整、无损），而 `actors`/
    // `population` 两个尚不支持序列化的字段直接搬运过去（对应它们
    // 「暂留在内存里、还不真正落盘」的当前实现现实）。等 P5 补上
    // 内容注册表、这两个字段也能真正序列化后，这里应当改为对整个
    // `WorldState` 做序列化往返，不再手动搬运。
    // Arrange
    let (mut uninterrupted, player, enemy) = setup(2026);
    let full_intents = intent_stream(player, enemy);
    play(&mut uninterrupted, &full_intents);
    let uninterrupted_hash = uninterrupted.hash();

    let (mut live, resumed_player, resumed_enemy) = setup(2026);
    let resumed_intents = intent_stream(resumed_player, resumed_enemy);
    let checkpoint = resumed_intents.len() / 2;
    play(&mut live, &resumed_intents[..checkpoint]);

    // Act：序列化往返只覆盖当前真正参与序列化的字段，actors/population
    // 原样搬运——见上方本测试的说明。
    let encoded = serde_json::to_vec(&live).expect("WorldState 当前参与序列化的字段必可序列化");
    let mut reloaded: WorldState =
        serde_json::from_slice(&encoded).expect("刚序列化的数据必然合法");
    reloaded.actors = live.actors.clone();
    reloaded.population = live.population.clone();
    // terrain_table 同样不参与序列化（见 WorldState 文档），本任务新增
    // 的已知限制，与 actors/population 同一处理方式：直接搬运当前会话
    // 已经注册好的表，而不是假装读档本身就能重建它。
    reloaded.terrain_table = live.terrain_table.clone();
    play(&mut reloaded, &resumed_intents[checkpoint..]);

    // Assert
    assert_eq!(reloaded.hash(), uninterrupted_hash);
}
