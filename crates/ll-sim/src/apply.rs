//! `apply`：把一个 [`Effect`] 落到 [`WorldState`] 上的唯一入口。

use ll_world::state::WorldState;

use crate::effect::Effect;

/// 把一个 [`Effect`] 应用到世界状态，这是全局唯一允许改动
/// [`WorldState`] 的函数。
///
/// # 三条纪律（见规格 Task 5 简报，逐条落实为下面的实现）
///
/// 1. **它是全局唯一能改世界的函数。** 别处出现 `&mut WorldState` 都是
///    设计错误——`resolve`（批次 C）只读世界、产出 `Effect`，不持有
///    `&mut WorldState`；时间轴、Intent 层同样不改世界。仓库里理应
///    只有这一处代码同时具备「拿到 `&mut WorldState`」与「据此给字段
///    赋值」两件事。
/// 2. **它不含任何游戏逻辑。** 下面每个分支要么是直接赋值，要么是
///    `if let Some(..)` 这类边界防御（实体是否还存在），没有一处在
///    判断「这算不算命中」「伤害该扣多少」之类的规则——那些判断已经
///    在 `resolve` 里做完，产出这个 `Effect` 时就已经带着最终数字。
/// 3. **它必须极短。** 六个分支各自不超过两行，任何看起来需要更多
///    行数才能表达的分支，多半是规则判断偷偷混了进来，应该退回
///    `resolve`。
///
/// # 签名如何拦住「不经 `Effect` 就改世界」
///
/// [`WorldState`] 的字段全部公开（存档格式即结构体本身，见其文档），
/// Rust 的可见性系统无法把「写 `WorldState`」这件事在编译期锁死到
/// 只有本函数能做——那需要把字段私有化、只留访问器，是比本批次大得
/// 多的封装改造。本函数的签名 `apply(world: &mut WorldState, effect:
/// &Effect)` 做到的是把「要改世界，必须先有一个 `Effect` 值」焊进
/// 类型签名：拿不出一个 `Effect`，就没有把状态改动传给这个函数的
/// 办法。守住「别处不会真的绕过去」靠的是约定而非编译器——评审时若
/// 在本文件之外看到 `&mut WorldState` 紧跟着字段赋值，就是这条纪律
/// 被打破的信号，这正是简报原文给出的兜底标准（「在类型上做不到，
/// 或至少显然错误」的后半句）。
pub fn apply(world: &mut WorldState, effect: &Effect) {
    match *effect {
        Effect::MoveTo { actor, pos } => {
            if let Some(agent) = world.actors.get_mut(actor) {
                agent.pos = pos;
            }
        }
        Effect::Damage { target, amount } => {
            *world.health.entry(target).or_insert(0) -= amount;
        }
        Effect::Kill { target } => {
            world.actors.despawn(target);
            world.health.remove(&target);
        }
        Effect::ScheduleNext { actor, at } => {
            if let Some(agent) = world.actors.get_mut(actor) {
                agent.next_action_at = at;
            }
        }
        Effect::SetTerrain { pos, kind } => {
            world.terrain.set_terrain(pos, kind);
        }
        Effect::AdjustWallet { actor, delta } => {
            if let Some(agent) = world.actors.get_mut(actor) {
                agent.wallet += delta;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::terrain::TerrainKind;

    use super::*;

    /// 测试世界尺寸：64 是噪声格点周期的整数倍，满足
    /// `WorldState::new` 的前置条件（与 `ll-world` 既有测试同一常量）。
    fn test_world() -> WorldState {
        let size = TorusSize::new(64, 64).expect("64x64 满足整除约束");
        WorldState::new(size, &GenParams::default()).expect("测试尺寸满足全部构造前置条件")
    }

    fn blank_agent(world: &WorldState) -> Agent {
        // `ContentIndex` 只能经 `Interner::intern` 取得（见其文档：
        // 索引依赖登记顺序，没有可以凭空构造的公开常量）——测试只是
        // 需要一个占位职业，登记哪个标识符不重要。
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        Agent {
            pos: world.size.wrap(0, 0),
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
        }
    }

    #[test]
    fn 移动效果改变实体位置() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        let target_pos = world.size.wrap(5, 7);

        // Act
        apply(
            &mut world,
            &Effect::MoveTo {
                actor,
                pos: target_pos,
            },
        );

        // Assert
        assert_eq!(
            world.actors.get(actor).expect("刚生成的实体必然存在").pos,
            target_pos
        );
    }

    #[test]
    fn 伤害效果扣减生命() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let target = world.actors.spawn(agent);
        world.health.insert(target, 100);

        // Act
        apply(&mut world, &Effect::Damage { target, amount: 30 });

        // Assert
        assert_eq!(world.health[&target], 70);
    }

    #[test]
    fn 对已销毁实体施加效果不会崩溃() {
        // 时间轴队列可能残留死者条目，effect 仍可能对着一个已销毁的
        // 实体到来——apply 必须安全地忽略，而不是 panic。
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        world.actors.despawn(actor);
        let pos = world.size.wrap(1, 1);

        // Act & Assert：不应崩溃。
        apply(&mut world, &Effect::MoveTo { actor, pos });
        apply(
            &mut world,
            &Effect::Damage {
                target: actor,
                amount: 10,
            },
        );
        apply(&mut world, &Effect::Kill { target: actor });
        apply(&mut world, &Effect::ScheduleNext { actor, at: Tick(5) });
        apply(&mut world, &Effect::AdjustWallet { actor, delta: 100 });
    }

    #[test]
    fn 效果的应用顺序不影响最终世界哈希() {
        // 用两个互不重叠位置的地形效果验证 apply 本身不引入隐藏的顺序
        // 依赖——SetTerrain 直接落在 WorldState::hash 已经覆盖的地形
        // 网格上，不需要为这条测试额外扩大 hash 的覆盖范围。
        // Arrange
        let pos_a = TorusSize::new(64, 64).expect("64x64 合法").wrap(2, 2);
        let pos_b = TorusSize::new(64, 64).expect("64x64 合法").wrap(40, 12);
        let effect_a = Effect::SetTerrain {
            pos: pos_a,
            kind: TerrainKind::FLOOR_STONE,
        };
        let effect_b = Effect::SetTerrain {
            pos: pos_b,
            kind: TerrainKind::WALL_STONE,
        };

        let mut forward = test_world();
        apply(&mut forward, &effect_a);
        apply(&mut forward, &effect_b);

        let mut backward = test_world();
        apply(&mut backward, &effect_b);
        apply(&mut backward, &effect_a);

        // Act & Assert
        assert_eq!(forward.hash(), backward.hash());
    }
}
