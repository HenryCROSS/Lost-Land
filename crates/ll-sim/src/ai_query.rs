//! AI 决策用的**只读世界查询原语**：找目标、判方向。
//!
//! # 为什么这些函数住在 `ll-sim`
//!
//! 它们此前是 `ll_script::api::actor` 里的私有函数，只给 Steel 查询
//! 函数（`nearby-enemy`/`nearby-actor-in-view`/`direction-toward`）当
//! 实现。行为树本身搬进 Rust（见 [`crate::behavior`] 与
//! `ll_mod::native_behavior`）之后，同一批函数出现了第二个调用方，而
//! 那个调用方在 `ll-mod`——`ll-script` 是它的依赖，不能反过来。
//!
//! 依赖链是 `ll-world ← ll-sim ← ll-script ← ll-mod`（规格 §5），
//! `ll-sim` 是两个调用方共同的上游，也是这些函数语义上该在的地方：
//! 它们的输入是 `&WorldState`、输出是 `EntityId`/[`Direction`]，一个
//! Steel 类型都不碰。`ll_script::api::actor` 现在只是一层把它们包成
//! `SteelVal` 的适配。
//!
//! # 约束
//!
//! - **ADR 0023 / C1**：本模块全部函数只接 `&WorldState`（共享引用），
//!   物理上写不了世界。它们回答「我看到了什么」，不回答「我做了什么」。
//! - **C5**：候选者遍历走 `Arena::iter_with_id`（`Vec` 支撑的固定
//!   顺序），距离相等时按 `EntityId` 升序打破平局——没有一处依赖
//!   `HashMap` 迭代顺序。
//!
//! # 已知简化（原样承继，本批次不动）
//!
//! [`nearest_hostile`] 的「附近」是固定半径的**平方距离**筛选，不是
//! 真正的 FOV 可见性；[`nearest_visible_actor`] 才走真正的 FOV。两者
//! 的差别是刻意的：前者服务「野怪扑向最近的敌人」，后者服务「卫兵
//! 看见了谁」，隔着墙的目标只有后者会漏掉。
//!
//! [`is_hostile`] 同样是粗略近似：没有任何势力归属的实体（野怪）视为
//! 对谁都敌对。真正的声望/关系矩阵是
//! `knowledge/design/society-and-affiliation.md` 描述的 P8 范围。

use ll_world::entity::{AffiliationKind, Agent, EntityId};
use ll_world::fov::compute_fov;
use ll_world::state::WorldState;
use ll_world::surface_store::SurfaceWindow;

use crate::intent::Direction;

/// [`nearest_hostile`] 的「附近」平方距离阈值——半径约 10 格。
pub const NEARBY_ENEMY_RANGE_SQ: i64 = 100;

/// [`nearest_visible_actor`] 默认的视野半径。
///
/// 与 `crate::resolve::EXPLORATION_SIGHT_RADIUS`（玩家探索标记）及设计
/// 文档 `DEFAULT_NPC_BASE_SIGHT_RADIUS` 建议值同一个量级（12），不是
/// 巧合：这是本代码库目前对「一个前景实体大致能看多远」的既有拍板值。
pub const NEARBY_ACTOR_VIEW_RADIUS: u32 = 12;

/// 找出离 `self_id` 最近、且对它敌对、且落在 [`NEARBY_ENEMY_RANGE_SQ`]
/// 内的实体；没有就是 `None`。
pub fn nearest_hostile(world: &WorldState, self_id: EntityId) -> Option<EntityId> {
    let me = world.actors.get(self_id)?;
    world
        .actors
        .iter_with_id()
        .filter(|(id, _)| *id != self_id)
        .filter(|(_, other)| is_hostile(me, other))
        .filter_map(|(id, other)| {
            let (dx, dy) = world.size.delta(me.pos, other.pos);
            let dist_sq = i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy);
            (dist_sq <= NEARBY_ENEMY_RANGE_SQ).then_some((dist_sq, id))
        })
        // 距离相等时按 EntityId 升序打破平局（C5）。
        .min_by_key(|&(dist_sq, id)| (dist_sq, id))
        .map(|(_, id)| id)
}

/// 找出离 `self_id` 最近、且真的落在它 FOV 内的实体（**不看敌对
/// 关系**）；范围外或不存在时返回 `None`。
///
/// 两段式过滤：`world.size.chebyshev` 粗筛（`O(1)`/候选者）+
/// `VisibleSet::contains` 成员测试，只对观察者自己的位置算一次
/// [`compute_fov`]。隔着墙的目标因此找不到。
pub fn nearest_visible_actor(
    world: &WorldState,
    self_id: EntityId,
    radius: u32,
) -> Option<EntityId> {
    let me = world.actors.get(self_id)?;
    let visible = compute_fov(
        &SurfaceWindow::new(&world.terrain),
        &world.terrain_table,
        me.pos,
        radius,
    );
    world
        .actors
        .iter_with_id()
        .filter(|(id, _)| *id != self_id)
        .filter_map(|(id, other)| {
            let dist = world.size.chebyshev(me.pos, other.pos);
            if dist > radius {
                return None; // 粗筛：距离已经超出半径，FOV 不可能命中。
            }
            visible.contains(other.pos).then_some((dist, id))
        })
        .min_by_key(|&(dist, id)| (dist, id))
        .map(|(_, id)| id)
}

/// 从 `from` 指向 `to` 的八向之一；任一实体不存在时返回 `None`。
pub fn direction_toward(world: &WorldState, from: EntityId, to: EntityId) -> Option<Direction> {
    let (me, them) = (world.actors.get(from)?, world.actors.get(to)?);
    let (dx, dy) = world.size.delta(me.pos, them.pos);
    Some(direction_from_delta(dx, dy))
}

/// `(dx, dy)`（环面最短带符号位移）→ 八向。
///
/// 零位移（同格）没有明确方向，任意但稳定地退化为
/// [`Direction::North`]——与本模块其余查询「意料之外的输入选一个确定
/// 值而不是 panic」同一条纪律。
pub fn direction_from_delta(dx: i32, dy: i32) -> Direction {
    match (dx.signum(), dy.signum()) {
        (0, -1) | (0, 0) => Direction::North,
        (0, 1) => Direction::South,
        (-1, 0) => Direction::West,
        (1, 0) => Direction::East,
        (1, -1) => Direction::NorthEast,
        (1, 1) => Direction::SouthEast,
        (-1, 1) => Direction::SouthWest,
        (-1, -1) => Direction::NorthWest,
        // `i32::signum` 的值域恰为 {-1, 0, 1}，上面九种组合已穷尽；
        // 保留这一分支只是让编译器确认穷尽性。
        _ => Direction::North,
    }
}

/// `b` 是否对 `a` 敌对——粗略近似，见模块文档「已知简化」。
pub fn is_hostile(a: &Agent, b: &Agent) -> bool {
    let a_factions: Vec<_> = a
        .affiliations
        .iter()
        .filter(|aff| aff.kind == AffiliationKind::Faction)
        .map(|aff| aff.org)
        .collect();
    if a_factions.is_empty() {
        return true;
    }
    !b.affiliations
        .iter()
        .any(|aff| aff.kind == AffiliationKind::Faction && a_factions.contains(&aff.org))
}

/// 一个实体此刻在不在潜行。
///
/// 潜行**不改可见性**：卫兵照常看得见潜行中的目标
/// （[`nearest_visible_actor`] 一个字都没改），只是「要不要把这个人
/// 当回事」那次判定的成功率降下来，见 `ll_mod::native_behavior` 的
/// 卫兵盘查概率。
pub fn is_stealthed(world: &WorldState, target: EntityId) -> bool {
    world
        .actors
        .get(target)
        .is_some_and(|agent| agent.stealthed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 同格退化为北而不是恐慌() {
        // Arrange & Act & Assert
        assert_eq!(direction_from_delta(0, 0), Direction::North);
    }

    #[test]
    fn 八个方向各自映射到对应的对角或正向() {
        // Arrange & Act & Assert
        assert_eq!(direction_from_delta(0, -3), Direction::North);
        assert_eq!(direction_from_delta(0, 2), Direction::South);
        assert_eq!(direction_from_delta(-4, 0), Direction::West);
        assert_eq!(direction_from_delta(1, 0), Direction::East);
        assert_eq!(direction_from_delta(2, -2), Direction::NorthEast);
        assert_eq!(direction_from_delta(3, 5), Direction::SouthEast);
        assert_eq!(direction_from_delta(-1, 7), Direction::SouthWest);
        assert_eq!(direction_from_delta(-9, -1), Direction::NorthWest);
    }
}
