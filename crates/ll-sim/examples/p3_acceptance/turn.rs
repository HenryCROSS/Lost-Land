//! P3 demo 独有的部分：固定策略 AI（朝玩家移动、相邻则攻击）与伤害
//! 飘字这两样纯呈现/demo 专属的东西。
//!
//! # 回合引擎本身已经搬进了 `ll_sim::turn`
//!
//! 「弹出时间轴 → 设世界时钟 → resolve → apply → 清理死者 → 重新排期」
//! 这条核心机制与具体是哪个游戏无关——`ll-game`（本体二进制）需要
//! 一模一样的一份，此前它只存在于本文件里，`ll-game` 因为看不见
//! `examples/` 代码而从未接线，导致真实游玩时 `world.clock` 永不
//! 推进。这条缺陷修复时把 `TurnEngine` 搬到了 `ll_sim::turn`（两者
//! 共同的上游 crate），见该模块文档「为什么这段逻辑必须挪进
//! `ll-sim`」一节的完整论证。本文件现在只留下 demo 自己独有、
//! `ll-game` 不需要也不该被强迫携带的两样东西：
//!
//! - 固定策略 AI（[`ai_intent`]）——行为树属 P7（见其文档），这是
//!   demo 验证时间轴用的占位策略，不是通用逻辑。
//! - 伤害飘字（[`DamagePopup`]/[`tick_popups`]）——纯呈现层状态，不
//!   进 `WorldState`、不属于「回合与模拟层」（见 `ll_sim` crate 顶层
//!   文档），`ll_sim::turn::TurnEngine` 通过 `on_effect` 回调把控制权
//!   交还给这里，自己不知道、也不需要知道呈现层长什么样子。

use ll_core::torus::TorusPos;
use ll_sim::effect::Effect;
use ll_sim::intent::{Direction, Intent};
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

/// 一条伤害飘字在屏幕上停留的帧数（60fps 下约 0.75 秒）。
///
/// 纯呈现层的时间基准，故意用帧数而非墙钟时长——与
/// `ll_platform::window::FrameId` 同一纪律（见其文档）：整数帧号可以
/// 安全地驱动动画计时，不引入浮点或不确定的墙钟耗时。
pub(crate) const DAMAGE_POPUP_LIFETIME_FRAMES: u32 = 45;

/// 一次伤害的飘字：位置、数值、剩余存活帧数。
///
/// 只在渲染层持有，不进 [`WorldState`]、不参与任何结算判断——纯粹是
/// 「伤害与受击，伤害数字可见」这条验收点的呈现效果。
#[derive(Debug, Clone, Copy)]
pub(crate) struct DamagePopup {
    pub(crate) pos: TorusPos,
    pub(crate) amount: i32,
    pub(crate) remaining_frames: u32,
}

/// 把一批伤害飘字向前推进一帧，丢弃已过期的。
pub(crate) fn tick_popups(popups: &mut Vec<DamagePopup>) {
    for popup in popups.iter_mut() {
        popup.remaining_frames = popup.remaining_frames.saturating_sub(1);
    }
    popups.retain(|popup| popup.remaining_frames > 0);
}

/// 供 [`ll_sim::turn::TurnEngine::perform`] 的 `on_effect` 回调使用：
/// 把一次 `Effect::Damage` 记成一条伤害飘字，推进 `popups`。
///
/// 必须在 `apply` 之前读位置——`TurnEngine::perform` 保证了这一点
/// （见其文档）：若同一批效果里紧接着一个 `Effect::Kill`，`apply` 后
/// 该实体已从世界里销毁，位置就再也取不到了。
pub(crate) fn record_damage_popup(
    world: &WorldState,
    effect: &Effect,
    popups: &mut Vec<DamagePopup>,
) {
    if let Effect::Damage { target, amount } = effect
        && let Some(agent) = world.actors.get(*target)
    {
        popups.push(DamagePopup {
            pos: agent.pos,
            amount: *amount,
            remaining_frames: DAMAGE_POPUP_LIFETIME_FRAMES,
        });
    }
}

/// 固定策略 AI：相邻（切比雪夫距离 ≤ 1）则攻击玩家，否则朝玩家移动
/// 一格——简报「有意留给后续阶段的缺口」明确写着行为树属 P7，P3 的
/// 敌人只需要这一条策略就足以验证时间轴。
///
/// 用 [`ll_core::torus::TorusSize::chebyshev`]/`delta`
/// 判定距离与朝向，全程不手写欧氏距离（硬性约束：环面坐标只走
/// `TorusSize` 的方法）。
///
/// # 移动前必须先看一眼地形，否则会真的卡死
///
/// 朝玩家方向的下一格若不可通行（水域、山体……），`ll_sim::resolve`
/// 的 `resolve_move` 会判定「撞墙」，产出空的 `Vec<Effect>`——不含
/// `Effect::ScheduleNext`（见其文档）。固定策略 AI 没有寻路能力（行为
/// 树属 P7），若不管三七二十一都往玩家方向走，一旦那个方向恰好是水，
/// 这个实体的 `next_action_at` 就永远不会前进，会在同一个 tick 被
/// [`ll_sim::turn::TurnEngine::advance_ai`] 反复弹出——这不是假设：
/// 接了真实生成的地形后，这里就是那次死循环的根因。相邻格不可通行时
/// 改为原地等待（[`ll_sim::resolve::resolve`] 对 `Intent::Wait` 恒产出
/// `ScheduleNext`，见其实现），保证时间只会前进不会卡住；等待一回合
/// 也不算错误的 AI 手感，比「贴着水面反复无效地往同一个方向撞」更
/// 合理。
pub(crate) fn ai_intent(world: &WorldState, actor: EntityId, player: EntityId) -> Intent {
    let (Some(agent), Some(target)) = (world.actors.get(actor), world.actors.get(player)) else {
        return Intent::Wait { actor };
    };
    if world.size.chebyshev(agent.pos, target.pos) <= 1 {
        return Intent::Attack {
            actor,
            target: player,
        };
    }
    let (dx, dy) = world.size.delta(agent.pos, target.pos);
    let dir = direction_toward(dx, dy);
    let (step_x, step_y) = dir.delta();
    let dest = world
        .size
        .wrap(agent.pos.x() + step_x, agent.pos.y() + step_y);
    // 只读查询：demo 世界是单区块布局，WorldState::new 的出生点邻域
    // 预热已让它整体常驻，见 `spawn::is_walkable` 文档同一节。未常驻
    // 时保守地原地等待，与「相邻格不可通行」走同一条分支。
    let blocked = world
        .terrain_at(dest)
        .is_none_or(|kind| kind.blocks_move(&world.terrain_table));
    if blocked {
        return Intent::Wait { actor };
    }
    Intent::Move { actor, dir }
}

/// 把一个（可能很大的）带符号位移换算成八向里最接近的一个：只看符号，
/// 不看大小——移动恒为一格（见 `ll_sim::intent::Direction` 文档）。
///
/// 用 [`std::cmp::Ordering`] 而非直接匹配 `i32::signum()` 的结果：
/// `signum()` 的返回类型仍是 `i32`，编译器无法从字面量模式推断出它
/// 只会取 `-1`/`0`/`1` 三个值，穷尽匹配会被拒绝（除非另加通配符兜底，
/// 而通配符正是本仓库编码风格想避免的——见 `ll_sim::intent` 的
/// `Axis` 类型文档「用具名的两变体枚举而非 -1/1 整数」一节）。
/// `i32::cmp(&0)` 返回的 `Ordering` 只有三个变体，`(Ordering, Ordering)`
/// 的九种组合可以被真正穷尽，不需要通配符。
fn direction_toward(dx: i32, dy: i32) -> Direction {
    use std::cmp::Ordering;
    match (dx.cmp(&0), dy.cmp(&0)) {
        (Ordering::Equal, Ordering::Less) => Direction::North,
        (Ordering::Equal, Ordering::Greater) => Direction::South,
        (Ordering::Less, Ordering::Equal) => Direction::West,
        (Ordering::Greater, Ordering::Equal) => Direction::East,
        (Ordering::Greater, Ordering::Less) => Direction::NorthEast,
        (Ordering::Greater, Ordering::Greater) => Direction::SouthEast,
        (Ordering::Less, Ordering::Greater) => Direction::SouthWest,
        (Ordering::Less, Ordering::Less) => Direction::NorthWest,
        (Ordering::Equal, Ordering::Equal) => Direction::North,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::terrain::{BaseTerrainIds, base_terrain_fixture};
    use ll_world::zone::ZoneLayout;

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

    fn spawn_at(world: &mut WorldState, pos: (i32, i32), dexterity: i32) -> EntityId {
        let mut interner = Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        let agent_pos = world.size.wrap(pos.0, pos.1);
        let (zone, _) = world.terrain.layout().tile_to_zone(agent_pos);
        world.actors.spawn(Agent {
            pos: agent_pos,
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
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(
                zone,
                ll_core::ident::ContentIndex::default(),
            ),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        })
    }

    #[test]
    fn 相邻时ai选择攻击而非移动() {
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let enemy = spawn_at(&mut world, (6, 5), 10);

        // Act
        let intent = ai_intent(&world, enemy, player);

        // Assert
        assert!(matches!(intent, Intent::Attack { .. }));
    }

    #[test]
    fn 不相邻时ai选择朝玩家方向移动() {
        // Arrange：显式把敌人西边一格设成草地——ai_intent 现在会先看一眼
        // 下一格是否可通行（见其文档「移动前必须先看一眼地形」），若
        // 依赖生成地形恰好可通行，这条测试会随生成参数/世界尺寸的调整
        // 变得脆弱。
        let (mut world, terrain_ids) = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let enemy = spawn_at(&mut world, (10, 5), 10);
        world
            .terrain
            .set_terrain(world.size.wrap(9, 5), terrain_ids.grass);

        // Act
        let intent = ai_intent(&world, enemy, player);

        // Assert：目标在正东方，应产生向西移动——朝玩家所在方向。
        assert!(matches!(
            intent,
            Intent::Move {
                dir: Direction::West,
                ..
            }
        ));
    }
}
