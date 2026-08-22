//! 「当前正在决策的实体」这一活跃指针，外加建立在它之上的几个行为树
//! 查询原语：`self-handle`/`nearby-enemy`/`direction-toward`。
//!
//! # 为什么需要一个独立于「活跃世界」的「活跃实体」
//!
//! [`crate::api::query`] 的活跃世界指针回答「脚本现在能读哪个
//! `WorldState`」；行为树还需要回答另一个问题——「这次调用是在为哪个
//! 实体决策」。两者是正交的两条信息（同一个世界可以先后为不同实体
//! 调用同一棵树），因此各自独立成一套 `thread_local!`，与
//! [`crate::api::rng`] 的活跃随机流是完全相同的模式：宿主在调用脚本前
//! 用 [`set_active_actor`] 设置好，调用窗口结束后 [`clear_active_actor`]，
//! 不清空会让下一次忘记设置的调用悄悄复用上一个实体的身份。
//!
//! # 「找到附近的目标」——已落地部分，与仍未落地的部分
//!
//! `knowledge/design/script-entity-handles-and-batch-queries.md` 五节
//! 设计了一整套批量查询原语（`world-entities-hostile-to`/`filter`/
//! `sort-by`/`nearest`……，返回不透明的 `EntitySetHandle`）——那套设计
//! 本次**不实现**（如该文档状态行所记：截至本次仍是纯设计）。本模块
//! 只落地「找到附近的目标」这一条最小可用路径：[`nearby_enemy`] 直接
//! 返回**单个**最近的敌对实体句柄（或 `#f`），不经过集合/筛选/排序
//! 这一整层不透明句柄机制。
//!
//! 这个简化在两处进一步收窄，都如实记录在这里而不是留给读者自己
//! 发现：
//!
//! 1. **「附近」是固定半径的平方距离筛选，不是真正的 FOV 可见性**。
//!    完整设计要求用 `world-entities-in-view` 起步（该文档 5.2 节：
//!    「可见性判断本身依赖 FOV（引擎层），只能整体查询」）——但
//!    `WorldState` 目前不为每个 NPC 缓存一份持久 FOV 网格（FOV 是
//!    按需现算的渲染期查询，见 `ll_world::fov` 模块文档），本模块不
//!    在这里现算一次完整的阴影投射（那是每次查询一次 O(视野格数)
//!    的开销，且与 FOV 网格该不该被 AI 复用是一个更大的、本次不解决
//!    的问题）。改用 [`NEARBY_ENEMY_RANGE_SQ`] 平方距离阈值（环面最短
//!    位移，`TorusSize::delta`，不开方——硬性约束「环面坐标只走
//!    `TorusSize` 方法」「禁止 `sqrt`/`powi(2)`/`hypot`」）近似替代，
//!    这是一个诚实的简化，不是「视野」的完整实现。
//! 2. **「敌对」是粗略的势力比对，不是完整的关系矩阵**。见
//!    [`is_hostile`] 文档——`knowledge/design/society-and-affiliation.md`
//!    描述的完整声望/关系系统是 P8 才落地的范围，本模块只用
//!    `Affiliation::kind == Faction` 是否有交集做一个能跑起来的近似。
//!
//! # 确定性（约束 C3/C5）
//!
//! [`nearby_enemy`] 遍历 `Arena<Agent>`（`iter_with_id`，槽位数组的
//! 固定顺序，不是 `HashMap`，不受 C5 约束）挑最近的一个；候选距离
//! 相等时按 `EntityId` 的既有 `Ord`（`(index, generation)` 字典序）
//! 打破平局——与 `crates/ll-sim/src/timeline.rs`「同刻打破平局」、
//! `script-entity-handles-and-batch-queries.md` 5.5 节同一条既有纪律，
//! 见 [`nearest_hostile`] 的 `min_by_key` 调用。本模块的查询不消费任何
//! 随机性，谈不上 C3；行为树若要用随机性（本批次的示例 mod 不需要），
//! 必须走 [`crate::api::rng`] 的 `DetRng` 通道，不能自行构造。

use std::cell::Cell;

use steel::rvals::{IntoSteelVal, SteelVal};

use ll_world::entity::{AffiliationKind, Agent, EntityId};
use ll_world::state::WorldState;

use crate::api::handle::ScriptEntityHandle;
use crate::api::query::with_active_world;
use crate::host::ScriptEngine;

thread_local! {
    /// 当前调用窗口内，行为树正在为哪个实体决策。
    static ACTIVE_ACTOR: Cell<Option<EntityId>> = const { Cell::new(None) };
}

/// 设置本次调用窗口的活跃实体。
pub fn set_active_actor(actor: EntityId) {
    ACTIVE_ACTOR.with(|cell| cell.set(Some(actor)));
}

/// 清空活跃实体。
pub fn clear_active_actor() {
    ACTIVE_ACTOR.with(|cell| cell.set(None));
}

/// 在活跃实体上执行 `f`；没有设置活跃实体时返回 `default`——与
/// `query::with_active_world`「宿主接线可能有 bug」同一条降级思路，
/// 不 panic。
fn with_active_actor<T>(default: T, f: impl FnOnce(EntityId) -> T) -> T {
    ACTIVE_ACTOR.with(|cell| match cell.get() {
        Some(actor) => f(actor),
        None => default,
    })
}

/// 供下游 crate（`ll-mod` 的运行期查询注册函数，例如
/// `skill-ready?`——它需要读当前决策实体的 `unlocked_skills`/
/// `skill_cooldowns`，但拿不到 `query::with_active_world`，那是本 crate
/// 私有的）在「活跃实体 + 活跃世界」上只读查询该实体的完整视图。
///
/// `T: Copy`：活跃实体缺失与活跃世界缺失是两次独立的降级检查，`default`
/// 可能被用到两次，要求 `Copy` 避免为了这个内部实现细节强行给调用方
/// 增加复杂度（本模块两处调用点都只需要 `bool`，天然满足）。
pub fn with_active_self<T: Copy>(default: T, f: impl FnOnce(&WorldState, &Agent) -> T) -> T {
    with_active_actor(default, |actor| {
        with_active_world(default, |world| match world.actors.get(actor) {
            Some(agent) => f(world, agent),
            None => default,
        })
    })
}

/// 「附近」的平方距离阈值——半径约 10 格（见模块文档「已知简化」）。
const NEARBY_ENEMY_RANGE_SQ: i64 = 100;

/// 注册 `self-handle`/`nearby-enemy`/`direction-toward` 三个行为树查询
/// 原语。
pub fn register(engine: &mut ScriptEngine) {
    engine.register_fn("self-handle", self_handle);
    engine.register_fn("nearby-enemy", nearby_enemy);
    engine.register_fn("direction-toward", direction_toward);
}

/// `(self-handle)`：当前决策实体自己的句柄；没有活跃实体时返回 `#f`
/// （宿主接线遗漏，不应在正常调用路径下发生，见模块文档同一条降级
/// 思路）。
fn self_handle() -> SteelVal {
    with_active_actor(SteelVal::BoolV(false), |actor| {
        ScriptEntityHandle::new(actor)
            .into_steelval()
            .unwrap_or(SteelVal::BoolV(false))
    })
}

/// `(nearby-enemy)`：活跃实体附近最近的一个敌对实体句柄；没有则 `#f`。
/// 见模块文档「找到附近的目标」一节的完整范围说明。
fn nearby_enemy() -> SteelVal {
    with_active_actor(SteelVal::BoolV(false), |actor| {
        with_active_world(SteelVal::BoolV(false), |world| {
            match nearest_hostile(world, actor) {
                Some(target) => ScriptEntityHandle::new(target)
                    .into_steelval()
                    .unwrap_or(SteelVal::BoolV(false)),
                None => SteelVal::BoolV(false),
            }
        })
    })
}

/// `(direction-toward target)`：从活跃实体指向 `target` 的八向之一
/// （符号，与 `ll_sim::intent::Direction`/`api::intent::direction_from_symbol`
/// 的命名一致），句柄失效或没有活跃实体/世界时返回 `#f`。
fn direction_toward(target: ScriptEntityHandle) -> SteelVal {
    with_active_actor(SteelVal::BoolV(false), |actor| {
        with_active_world(SteelVal::BoolV(false), |world| {
            let (Some(me), Some(them)) = (
                world.actors.get(actor),
                world.actors.get(target.entity_id()),
            ) else {
                return SteelVal::BoolV(false);
            };
            let (dx, dy) = world.size.delta(me.pos, them.pos);
            SteelVal::SymbolV(direction_symbol(dx, dy).into())
        })
    })
}

/// `(dx, dy)`（环面最短带符号位移）→ 八向符号名。零位移（同格）没有
/// 明确方向，任意但稳定地退化为 `"north"`——与本文件其余查询「宿主
/// 接线可能出现意料之外的输入时选一个确定值而不是 panic」同一条纪律。
fn direction_symbol(dx: i32, dy: i32) -> &'static str {
    match (dx.signum(), dy.signum()) {
        (0, -1) | (0, 0) => "north",
        (0, 1) => "south",
        (-1, 0) => "west",
        (1, 0) => "east",
        (1, -1) => "north-east",
        (1, 1) => "south-east",
        (-1, 1) => "south-west",
        (-1, -1) => "north-west",
        // `i32::signum` 的值域恰为 {-1, 0, 1}，上面九种组合已穷尽；
        // 保留这一分支只是让编译器确认穷尽性，不代表存在第十种输入。
        _ => "north",
    }
}

/// 找出 `world` 中离 `self_id` 最近、且对它敌对的实体；范围外或不存在
/// 时返回 `None`。
fn nearest_hostile(world: &WorldState, self_id: EntityId) -> Option<EntityId> {
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
        // 距离相等时按 EntityId 升序打破平局——见模块文档「确定性」。
        .min_by_key(|&(dist_sq, id)| (dist_sq, id))
        .map(|(_, id)| id)
}

/// `b` 是否对 `a` 敌对：粗略近似，见模块文档「找到附近的目标」一节
/// 「已知简化」第 2 条——`a` 没有任何势力归属（例如野怪）时视为对谁都
/// 敌对；否则要求 `a`/`b` 没有任何共同的势力归属。真正的声望/关系矩阵
/// 是 `knowledge/design/society-and-affiliation.md` 描述的 P8 范围，
/// 本函数不是那个系统的实现。
fn is_hostile(a: &Agent, b: &Agent) -> bool {
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

#[cfg(test)]
mod tests {
    use ll_core::time::Tick;
    use ll_world::entity::BaseStats;
    use ll_world::generate::GenParams;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    use steel::rvals::FromSteelVal;

    use crate::api::query::{clear_active_world, set_active_world};

    use super::*;

    fn test_world() -> WorldState {
        let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    fn spawn_agent_at(world: &mut WorldState, x: i32, y: i32) -> EntityId {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(x, y);
        world.actors.spawn(Agent {
            pos,
            stats: BaseStats::BASELINE,
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
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(
                world.terrain.layout().tile_to_zone(pos).0,
                ll_core::ident::ContentIndex::default(),
            ),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        })
    }

    #[test]
    fn 没有活跃实体时self_handle返回假而不崩溃() {
        // Arrange
        clear_active_actor();

        // Act & Assert
        assert_eq!(self_handle(), SteelVal::BoolV(false));
    }

    #[test]
    fn nearby_enemy找到范围内最近的敌对实体() {
        // Arrange：两个候选，一个更近；两者都没有势力归属，按「没有
        // 势力归属者对谁都敌对」的简化规则，二者互相敌对。
        let mut world = test_world();
        let me = spawn_agent_at(&mut world, 0, 0);
        let near = spawn_agent_at(&mut world, 2, 0);
        let _far = spawn_agent_at(&mut world, 9, 0);
        set_active_actor(me);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = nearby_enemy();
            clear_active_world();
            result
        };
        clear_active_actor();

        // Assert
        let handle = ScriptEntityHandle::from_steelval(&result).expect("范围内应有一个敌对目标");
        assert_eq!(handle.entity_id(), near);
    }

    #[test]
    fn nearby_enemy范围外没有目标时返回假() {
        // Arrange
        let mut world = test_world();
        let me = spawn_agent_at(&mut world, 0, 0);
        let _far = spawn_agent_at(&mut world, 50, 0);
        set_active_actor(me);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = nearby_enemy();
            clear_active_world();
            result
        };
        clear_active_actor();

        // Assert
        assert_eq!(result, SteelVal::BoolV(false));
    }

    #[test]
    fn direction_toward指向正东的目标返回east() {
        // Arrange
        let mut world = test_world();
        let me = spawn_agent_at(&mut world, 0, 0);
        let target = spawn_agent_at(&mut world, 5, 0);
        set_active_actor(me);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = direction_toward(ScriptEntityHandle::new(target));
            clear_active_world();
            result
        };
        clear_active_actor();

        // Assert
        assert_eq!(result, SteelVal::SymbolV("east".into()));
    }

    #[test]
    fn 共享势力归属的实体不被视为敌对() {
        // Arrange
        let mut world = test_world();
        let faction = ll_core::ident::WorldId::next(&mut 0u32);
        let me = spawn_agent_at(&mut world, 0, 0);
        let ally = spawn_agent_at(&mut world, 2, 0);
        for id in [me, ally] {
            let agent = world.actors.get_mut(id).expect("刚生成必然存在");
            agent.affiliations.push(ll_world::entity::Affiliation {
                kind: AffiliationKind::Faction,
                org: ll_world::entity::OrgRef::Instance(faction),
                standing: 1000,
            });
        }
        set_active_actor(me);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = nearby_enemy();
            clear_active_world();
            result
        };
        clear_active_actor();

        // Assert：同势力，不敌对，附近没有其他候选。
        assert_eq!(result, SteelVal::BoolV(false));
    }
}
