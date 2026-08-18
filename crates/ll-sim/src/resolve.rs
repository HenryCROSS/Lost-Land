//! `resolve`：把一个 [`Intent`] 结合当前世界状态，翻译成一串 [`Effect`]。
//!
//! # C1：`resolve` 必须是纯函数
//!
//! 签名 `resolve(world: &WorldState, intent: &Intent) -> Vec<Effect>`
//! 只接受 `&WorldState`（共享引用）——这不只是约定，是编译期保证：本
//! 文件里没有一处使用 `unsafe`、`Cell`、`RefCell` 或任何其他内部可变性
//! 手段，因此借用检查器直接禁止任何分支写世界，写世界唯一可能的入口
//! （`&mut WorldState`）根本不会出现在这个函数的调用树里。真正的写入
//! 全部延后到调用方对返回的 `Vec<Effect>` 逐个调用
//! [`crate::apply::apply`]（见其文档「三条纪律」）。
//!
//! 这个分离是并行结算的前提：未来成千上万个 AI 的 `resolve` 可以同时
//! 跑（各自只读世界、互不冲突），产出的 `Effect` 收集起来后再单线程
//! 依次 `apply`，读写从不交织。
//!
//! # 已知的范围边界：`Intent::Move` 不做「撞向实体即改判为攻击」的派生
//!
//! [`crate::intent`] 模块文档提到，`Intent::Move` 结合世界状态可以被
//! `resolve` 派生成攻击或开门——本文件确实把「移动目的地是关着的门」
//! 派生成开门效果（见本文件内部的 `resolve_move`），但**没有**把「移动目的地站着
//! 别的实体」派生成攻击。这不是遗漏：该派生一旦引入就要决定「同一格
//! 多个实体时打谁」这类新规则，而本批次的验收测试不需要它，贸然实现
//! 只会引入一段没有测试覆盖的行为。需要「撞人即攻击」的手感时，请把
//! 这条判定和它的打靶规则一起补上，而不是只加派生这一半。

use ll_core::time::Tick;
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

use crate::combat::{Penetration, damage_after_defense};
use crate::effect::Effect;
use crate::intent::{Direction, Intent};
use crate::timeline::action_cost;

/// 非位移动作（等待、攻击、开门）的基础代价，与平地移动同一基准
/// （草地的 `move_cost` 恰为这个值）——本批次没有武器速度、技能读条
/// 之类会让这些动作耗时不同于「一次基准行动」的系统，统一按这个基准
/// 计费，接入那些系统时按动作类型分别替换即可。
const BASE_ACTION_COST: u32 = 100;

/// 基准有效敏捷，对应 `BaseStats::BASELINE` 的敏捷值（10，调整值为零）。
///
/// 真正的「有效敏捷」需要 `derive_stats`（装备、状态效果、负重的综合
/// 结果）驱动，但那是衍生属性，规则上必须是纯函数且不进存档（见
/// `knowledge/design/attribute-system.md` 「七、衍生属性绝不进存档」），
/// 而 `derive_stats` 本身属于后续批次才落地的东西。`derive_stats` 落地
/// 后，[`effective_speed_from_dexterity`] 的函数体应替换成
/// `derive_stats(agent.stats, ..).effective_speed`，调用点不变。
const BASELINE_EFFECTIVE_SPEED: u32 = 1000;

/// `BaseStats::BASELINE` 的敏捷值——[`effective_speed_from_dexterity`]
/// 的线性映射以它为基准点：敏捷恰为这个值时，有效速度恰为
/// [`BASELINE_EFFECTIVE_SPEED`]。
const BASELINE_DEXTERITY: i64 = 10;

/// 由角色敏捷推出有效行动速度：基准敏捷（10）对应
/// [`BASELINE_EFFECTIVE_SPEED`]，此后与敏捷成正比。
///
/// # 为什么不能继续让全体角色共用同一个常量
///
/// 本函数落地前，四个 `resolve_*` 分支全部直接传入
/// [`BASELINE_EFFECTIVE_SPEED`] 这个常量本身，不读 `agent.stats.dexterity`
/// ——这是 P3 验收 demo（Task 9）排查时发现的阻断性缺陷：无论给敌人
/// 分配多高或多低的敏捷，`resolve` 算出的行动耗时都完全相同，时间轴
/// 调度器（[`crate::timeline`]）本身「敏捷高者能在同一窗口内多行动
/// 几次」这条核心手感（见其模块文档开篇）在结算层根本没有输入通道
/// 可以体现出来——`Timeline` 的排序逻辑是对的，喂给它的排期时刻却
/// 从未因敏捷不同而不同。
///
/// 这不是要提前实现完整的 `derive_stats`（装备/状态效果/负重那套还
/// 没有任何字段落地，见 [`BASELINE_EFFECTIVE_SPEED`] 文档），只是把
/// 「敏捷」这个已经存在于 [`ll_world::entity::BaseStats`] 的字段接上
/// 最朴素的线性比例，让 Intent → resolve → Effect → 时间轴这条链路
/// 真正对「敏捷不同」敏感，而不是看起来接好了、实际上分支从不读取
/// 敏捷字段。`derive_stats` 落地后应替换本函数体，调用点不必改动。
fn effective_speed_from_dexterity(dexterity: i32) -> u32 {
    let dexterity = i64::from(dexterity).max(1);
    let speed = i64::from(BASELINE_EFFECTIVE_SPEED) * dexterity / BASELINE_DEXTERITY;
    speed.clamp(1, i64::from(u32::MAX)) as u32
}

/// 把一个 [`Intent`] 结合当前世界状态，翻译成一串 [`Effect`]。
///
/// 目标实体（`actor`/`target`）若已不在 `world.actors` 中（可能已在
/// 同一批结算里被更早的 `Effect` 销毁），一律返回空 `Vec`——这与
/// [`crate::apply::apply`] 对不存在实体的处理方式一致（静默忽略而非
/// panic 或报错），理由同样是「目标不存在不是异常状况，是结算并发/
/// 时序下的正常可能性」。
pub fn resolve(world: &WorldState, intent: &Intent) -> Vec<Effect> {
    match *intent {
        Intent::Wait { actor } => resolve_wait(world, actor),
        Intent::Move { actor, dir } => resolve_move(world, actor, dir),
        Intent::Attack { actor, target } => resolve_attack(world, actor, target),
        Intent::OpenDoor { actor, pos } => resolve_open_door(world, actor, pos),
    }
}

/// 算出「从现在起 `cost` 个 tick 之后」的世界时刻。
fn schedule_after(world: &WorldState, cost: u32) -> Tick {
    Tick(world.clock.0 + i64::from(cost))
}

/// 原地等待一回合：只消耗基础代价，不产生除排期外的任何效果。
fn resolve_wait(world: &WorldState, actor: EntityId) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    }]
}

/// 朝某方向移动一格：按目的地的地形分三种情形处理。
///
/// - 目的地是一格「撞入即开」的地形（[`ll_world::terrain::TerrainTable::opens_into`]
///   有值，例如关着的门）：产生把该格改写成 `opens_into` 目标地形的
///   效果，而不是移动效果——门挡住了这一步，但「撞门」本身是有意义的
///   动作，不该像撞墙一样什么都不发生。**这条规则是任何地形都能声明的
///   属性，不是只对某个硬编码地形 ID 生效的特判**——见
///   `ll_world::terrain` 模块文档「`opens_into`」一节：这正是本次迁移
///   撞见并修掉的一处 API 洞，mod 现在可以给自己的地形也声明同样的
///   行为。
/// - 目的地完全不可通行（墙、窗等）：不产生任何效果，这一步作废。
/// - 目的地可通行：产生移动效果，行动耗时按该地形的分级 `move_cost`
///   计算——浅水、山地这类「过得去但更慢」的地形因此耗时更长。
fn resolve_move(world: &WorldState, actor: EntityId, dir: Direction) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let (dx, dy) = dir.delta();
    let dest = world.size.wrap(agent.pos.x() + dx, agent.pos.y() + dy);
    let terrain = world.terrain.terrain_at(dest);
    let speed = effective_speed_from_dexterity(agent.stats.dexterity);

    if let Some(open_kind) = terrain.opens_into(&world.terrain_table) {
        let cost = action_cost(BASE_ACTION_COST, speed);
        return vec![
            Effect::SetTerrain {
                pos: dest,
                kind: open_kind,
            },
            Effect::ScheduleNext {
                actor,
                at: schedule_after(world, cost),
            },
        ];
    }

    if terrain.blocks_move(&world.terrain_table) {
        return Vec::new();
    }

    let cost = action_cost(terrain.move_cost(&world.terrain_table), speed);
    vec![
        Effect::MoveTo { actor, pos: dest },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// 直接攻击一个已知目标（与 [`resolve_move`] 的隐式派生分开的显式路径，
/// 供已经知道目标的调用方——例如已锁定目标的 AI ——直接使用）。
///
/// 防御与穿透：本批次 `Agent` 还没有护甲字段（护甲属于装备系统，
/// P5 才落地），故这里固定传 `defense = 0`、`pen = Penetration::NONE`。
/// [`damage_after_defense`] 本身的穿透/下限行为已经由 `combat.rs` 的
/// 单元测试独立验证正确，这里只是先把接线接上；装备落地后只需把
/// 这两个占位值换成从目标身上算出的真实护甲与穿透。
///
/// 若这一下会让目标生命值降到零或以下，额外产出一个 [`Effect::Kill`]
/// ——是否致死是规则判断，必须在这里（`resolve`）做出，`apply` 只管
/// 照数字做加减（见 [`crate::effect::Effect::Damage`] 文档）。
fn resolve_attack(world: &WorldState, actor: EntityId, target: EntityId) -> Vec<Effect> {
    let Some(attacker) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(defender) = world.actors.get(target) else {
        return Vec::new();
    };

    let attack_power = attacker.stats.strength;
    let damage = damage_after_defense(attack_power, 0, Penetration::NONE);

    let mut effects = vec![Effect::Damage {
        target,
        amount: damage,
    }];
    if defender.health - damage <= 0 {
        effects.push(Effect::Kill { target });
    }

    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(attacker.stats.dexterity),
    );
    effects.push(Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    });
    effects
}

/// 开启某处的门：目的地不是一格「撞入即开」的地形时，这一步作废、不
/// 产生任何效果——与 [`resolve_move`] 撞墙时的处理一致，都是「动作在
/// 这个世界里无意义，静默作废」而不是报错。见 [`resolve_move`] 文档
/// 「`opens_into`」一节：这里同样查表，不再恒等比较某个硬编码地形 ID。
fn resolve_open_door(world: &WorldState, actor: EntityId, pos: (i32, i32)) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let door_pos = world.size.wrap(pos.0, pos.1);
    let Some(open_kind) = world
        .terrain
        .terrain_at(door_pos)
        .opens_into(&world.terrain_table)
    else {
        return Vec::new();
    };

    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![
        Effect::SetTerrain {
            pos: door_pos,
            kind: open_kind,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

#[cfg(test)]
mod tests {
    use ll_core::torus::TorusSize;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};

    use super::*;

    /// 测试世界尺寸：64 是噪声格点周期的整数倍，满足
    /// `WorldState::new` 的前置条件（与 `ll-sim`/`ll-world` 既有测试
    /// 同一常量）。
    ///
    /// 返回值附带 [`BaseTerrainIds`]：`terrain_ids` 与
    /// `world.terrain_table` 必须来自同一次 [`base_terrain_fixture`]
    /// 调用——`ContentIndex` 只在产出它的那个 `Interner` 里有意义
    /// （`ll_core::ident` 模块文档），两次独立调用各自的索引分配虽然
    /// 因为固定顺序而恰好数值相同，但把它们当成「必须配对」处理更不
    /// 容易在将来注册顺序调整时踩坑。
    fn test_world() -> (WorldState, BaseTerrainIds) {
        let size = TorusSize::new(64, 64).expect("64x64 满足整除约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let world = WorldState::new(size, &GenParams::default(), &terrain_ids, terrain_table)
            .expect("测试尺寸满足全部构造前置条件");
        (world, terrain_ids)
    }

    /// 造一个占位实体，站在 `(5, 5)`，六项主属性取基准值。
    fn spawn_agent(world: &mut WorldState) -> EntityId {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        world.actors.spawn(Agent {
            pos: world.size.wrap(5, 5),
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
        })
    }

    /// 从 `(5, 5)` 向东（`dx = 1`）走一步的目的地，与 [`spawn_agent`]
    /// 的出生点配套——测试只需要一个已知、可控的目的地格。
    fn east_of_spawn(world: &WorldState) -> ll_core::torus::TorusPos {
        world.size.wrap(6, 5)
    }

    /// 造一个占位实体，站在 `(5, 5)`，除敏捷外六项主属性取基准值——
    /// 供敏捷相关测试指定一个非基准的敏捷值。
    fn spawn_agent_with_dexterity(world: &mut WorldState, dexterity: i32) -> EntityId {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        world.actors.spawn(Agent {
            pos: world.size.wrap(5, 5),
            stats: BaseStats {
                dexterity,
                ..BaseStats::BASELINE
            },
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
        })
    }

    #[test]
    fn 结算不修改世界() {
        // resolve 的签名只接受 &WorldState，编译期已经不允许它写世界；
        // 这条测试是这个保证的行为级回归——即使产出了效果，调用 resolve
        // 本身也绝不应改变世界的哈希（哈希已覆盖地形与实体状态，见
        // WorldState::hash 文档）。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        world
            .terrain
            .set_terrain(east_of_spawn(&world), terrain_ids.grass);
        let intent = Intent::Move {
            actor,
            dir: Direction::East,
        };
        let hash_before = world.hash();

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(!effects.is_empty(), "本用例应产生效果，否则测不出意义");
        assert_eq!(world.hash(), hash_before);
    }

    #[test]
    fn 移动到不可通行地形不产生移动效果() {
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        world
            .terrain
            .set_terrain(east_of_spawn(&world), terrain_ids.wall_stone);
        let intent = Intent::Move {
            actor,
            dir: Direction::East,
        };

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(effects.is_empty());
    }

    #[test]
    fn 移动到浅水的行动耗时高于草地() {
        // Arrange
        let (mut grass_world, grass_ids) = test_world();
        let grass_actor = spawn_agent(&mut grass_world);
        grass_world
            .terrain
            .set_terrain(east_of_spawn(&grass_world), grass_ids.grass);

        let (mut water_world, water_ids) = test_world();
        let water_actor = spawn_agent(&mut water_world);
        water_world
            .terrain
            .set_terrain(east_of_spawn(&water_world), water_ids.shallow_water);

        // Act
        let grass_effects = resolve(
            &grass_world,
            &Intent::Move {
                actor: grass_actor,
                dir: Direction::East,
            },
        );
        let water_effects = resolve(
            &water_world,
            &Intent::Move {
                actor: water_actor,
                dir: Direction::East,
            },
        );

        // Assert
        let grass_cost = schedule_next_at(&grass_effects).0 - grass_world.clock.0;
        let water_cost = schedule_next_at(&water_effects).0 - water_world.clock.0;
        assert!(water_cost > grass_cost);
    }

    #[test]
    fn 攻击关着的门产生开门效果而非伤害效果() {
        // 「攻击关着的门」在这套设计里就是朝它的方向移动一步——门不是
        // 实体，Intent::Attack 的 target 必须是 EntityId，指向不了一格
        // 地形；玩家的「攻击」输入落到 resolve 这里，撞见关着的门时
        // 被派生成开门而不是造成伤害。
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let actor = spawn_agent(&mut world);
        world
            .terrain
            .set_terrain(east_of_spawn(&world), terrain_ids.door_closed);
        let intent = Intent::Move {
            actor,
            dir: Direction::East,
        };

        // Act
        let effects = resolve(&world, &intent);

        // Assert
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetTerrain { kind, .. } if *kind == terrain_ids.door_open
        )));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Damage { .. }))
        );
    }

    #[test]
    fn 撞入即开不是只对关着的门生效的特判() {
        // 这是本次迁移撞见并修掉的 API 洞的直接验收：opens_into 是
        // 任意地形都能声明的属性，不是只有 lostland:door_closed 才有
        // 的硬编码特权——一个假想 mod 注册的「活板门」同样应该走这条
        // 通用路径，而不需要去改 ll-sim 的源码。
        //
        // 用同一个 Interner 先注册本体 17 个地形、再追加两个自定义地形
        // ——不能各自新起一个 Interner：ContentIndex 只在产出它的那个
        // Interner 里有意义，另起一个会与本体的 0..17 撞号。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let (terrain_ids, mut table) =
            ll_world::terrain::materialize_base_terrain(&mut |id| interner.intern(id))
                .expect("本体地形声明表内部一致");
        let hatch_open = ll_world::terrain::TerrainKind::from_index(
            interner
                .intern(ll_core::ident::NamespacedId::parse("yourmod:hatch_open").expect("合法")),
        );
        let hatch_closed = ll_world::terrain::TerrainKind::from_index(
            interner
                .intern(ll_core::ident::NamespacedId::parse("yourmod:hatch_closed").expect("合法")),
        );
        table
            .define(
                hatch_open.index(),
                ll_world::terrain::TerrainAttrs {
                    blocks_sight: false,
                    blocks_move: false,
                    move_cost: 100,
                    opens_into: None,
                },
            )
            .expect("测试声明内部自洽");
        table
            .define(
                hatch_closed.index(),
                ll_world::terrain::TerrainAttrs {
                    blocks_sight: false,
                    blocks_move: true,
                    move_cost: u32::MAX,
                    opens_into: Some(hatch_open),
                },
            )
            .expect("测试声明内部自洽");

        let size = TorusSize::new(64, 64).expect("64x64 满足整除约束");
        let mut world = WorldState::new(size, &GenParams::default(), &terrain_ids, table)
            .expect("测试尺寸满足全部构造前置条件");
        world
            .terrain
            .set_terrain(east_of_spawn(&world), hatch_closed);
        let actor = spawn_agent(&mut world);

        // Act
        let effects = resolve(
            &world,
            &Intent::Move {
                actor,
                dir: Direction::East,
            },
        );

        // Assert
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetTerrain { kind, .. } if *kind == hatch_open
        )));
    }

    #[test]
    fn 敏捷更高的角色等待耗时更短() {
        // 这是 P3 验收 demo（Task 9）排查出的阻断性缺陷的回归测试：
        // 修复前 resolve 的四个分支全部直接传常量 BASELINE_EFFECTIVE_SPEED，
        // 不读 agent.stats.dexterity，敏捷高低对行动耗时毫无影响——时间轴
        // 调度器「敏捷高者能在同一窗口内多行动几次」这条核心手感因此在
        // 结算层根本不成立。
        // Arrange
        let (mut slow_world, _slow_ids) = test_world();
        let slow_actor = spawn_agent_with_dexterity(&mut slow_world, 5);
        let (mut fast_world, _fast_ids) = test_world();
        let fast_actor = spawn_agent_with_dexterity(&mut fast_world, 40);

        // Act
        let slow_effects = resolve(&slow_world, &Intent::Wait { actor: slow_actor });
        let fast_effects = resolve(&fast_world, &Intent::Wait { actor: fast_actor });

        // Assert
        let slow_cost = schedule_next_at(&slow_effects).0 - slow_world.clock.0;
        let fast_cost = schedule_next_at(&fast_effects).0 - fast_world.clock.0;
        assert!(fast_cost < slow_cost);
    }

    /// 从一批效果里取出 [`Effect::ScheduleNext`] 的排期时刻——上面几条
    /// 移动耗时测试都要读这个字段，抽成小工具避免重复的
    /// `iter().find_map(...)`。
    fn schedule_next_at(effects: &[Effect]) -> Tick {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::ScheduleNext { at, .. } => Some(*at),
                _ => None,
            })
            .expect("本文件的移动类测试用例都应产生 ScheduleNext 效果")
    }
}
