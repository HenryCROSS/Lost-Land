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
//!
//! # `nearby-actor-in-view`：真正的 FOV 可见性（卫兵职业接线批次新增）
//!
//! [`nearby_enemy`] 的「已知简化」第 1 条明确写着它用平方距离近似
//! 代替真正的 FOV——那条简化对「敌人该不该追上来打」这类场景够用，
//! 但卫兵盘查（`knowledge/design/ownership-and-crime-detection.md`
//! 二节 2.3 论证过的同一套两段式算法）要求的是「卫兵真的看得见」，
//! 隔着一面墙的目标不该被当成候选——这正是那条简化本身要回避的
//! 差异。[`nearby_actor_in_view`] 因此不复用 [`nearest_hostile`]，
//! 单独实现同一套「`TorusSize::chebyshev` 粗筛 → `compute_fov` 成员
//! 测试」两段式过滤：粗筛用候选者到活跃实体的距离与查询半径比较
//! （`O(1)`/候选者），只有通过粗筛的候选者才去查一次已经算好的
//! `ll_world::fov::VisibleSet::contains`（`O(1)` 哈希查找）——FOV 本身只在活跃实体
//! 的位置上算一次（不是每个候选者各算一次，那是二节 2.3
//! `witnessed_by` 面对「多个观察者各自的视角」时才需要的形状；这里
//! 只有活跃实体一个观察者，天然只需要一次），粗筛因此省下的是「明显
//! 越界的候选者也要做一次 `HashSet` 查找」这一步,量级虽然本就便宜，
//! 但与二节 2.3 描述的算法结构保持一致，也符合 C5（只用 `contains`
//! 成员测试，不遍历 `VisibleSet` 做决策）。
//!
//! 不看敌对关系——[`is_hostile`] 的势力近似在这里不适用：卫兵要能
//! 盘查任何看得见的单位（含友方/中立），不是只盘查敌人，见
//! `mods/example_mod/behavior.scm` 的 `guard-try-inspect`。
//!
//! # 潜行：为什么是一次判定的减值，不是一次可见性的改写
//!
//! [`actor_stealthed`]（`actor-stealthed?`，潜行与盗贼被动批次新增）
//! 是本模块唯一与潜行有关的东西，它**不参与** [`nearby_actor_in_view`]
//! 的任何一步：潜行中的实体照样会被 `nearby-actor-in-view` 找到，
//! `compute_fov`/`VisibleSet` 一个字节都没有因为潜行而改变。
//!
//! 被否决的替代方案是「潜行让敌人看不见你」——那要动 `ll_world::fov`
//! 的 `compute_fov`/`VisibleSet`，代价有两层：（一）FOV 是本项目性能
//! 与确定性最敏感的代码，它同时服务渲染（每帧）与 AI 查询；（二）
//! 语义会变得很怪——同一格对不同观察者的可见性不同，`VisibleSet`
//! 这个「以观察者为中心算一次」的类型就必须再多带一维「谁在看」，
//! 而它当前的全部调用点都不需要这一维。
//!
//! 选中的方案把潜行放在**下一步**：守卫照常看得见你，`guard-ai-tree`
//! 拿到目标之后要掷一次骰子决定「要不要把这个人当回事」
//! （`rng-chance`），潜行在那次掷骰上减值。零 FOV 改动，且语义更贴近
//! 项目所有者对卫兵的原始裁定（「有概率会来核查其他单位身上的物品」
//! ——本来就是一次概率判定，潜行只是改了那个概率）。

use std::cell::Cell;

use steel::rvals::{IntoSteelVal, SteelVal};

use ll_sim::intent::Direction;
use ll_world::entity::{Agent, EntityId};
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

/// 「附近」的平方距离阈值——真实定义在
/// [`ll_sim::ai_query::NEARBY_ENEMY_RANGE_SQ`]，本别名只是让本模块
/// 文档里的既有引用继续解析得到，本文件自己不再读它。
#[allow(dead_code)]
const NEARBY_ENEMY_RANGE_SQ: i64 = ll_sim::ai_query::NEARBY_ENEMY_RANGE_SQ;

/// [`nearby_actor_in_view`] 的 FOV 查询半径——与
/// `crate::resolve::EXPLORATION_SIGHT_RADIUS`（`ll-sim`,玩家探索标记）
/// 及设计文档 `DEFAULT_NPC_BASE_SIGHT_RADIUS` 建议值同一个量级（12），
/// 不是巧合：这是本代码库目前对「一个前景实体大致能看多远」的既有
/// 拍板值,本模块沿用而不是另起一个数字。
const NEARBY_ACTOR_VIEW_RADIUS: u32 = ll_sim::ai_query::NEARBY_ACTOR_VIEW_RADIUS;

/// 注册 `self-handle`/`nearby-enemy`/`nearby-actor-in-view`/
/// `direction-toward`/`actor-stealthed?` 五个行为树查询原语。
pub fn register(engine: &mut ScriptEngine) {
    engine.register_fn("self-handle", self_handle);
    engine.register_fn("nearby-enemy", nearby_enemy);
    engine.register_fn("nearby-actor-in-view", nearby_actor_in_view);
    engine.register_fn("direction-toward", direction_toward);
    engine.register_fn("actor-stealthed?", actor_stealthed);
}

/// `(actor-stealthed? target)`：`target` 此刻是否正在潜行
/// （[`ll_world::entity::Agent::stealthed`]）；句柄失效或没有活跃世界
/// 时返回 `#f`（与本文件其余查询同一条降级纪律：宿主接线可能有 bug，
/// 选一个确定值而不是 panic）。见模块文档「潜行：为什么是一次判定的
/// 减值，不是一次可见性的改写」一节。
///
/// # 为什么落在 `ll-script` 而不是 `ll-mod`
///
/// 与 `direction-toward` 同一条判据（对比
/// `ll_mod::script_behavior_api` 模块文档「为什么这一个函数单独落在
/// `ll-mod`」）：本查询只需要读 `WorldState` 的一个 `bool` 字段，
/// 不需要把任何命名空间字符串翻译成 `ContentIndex`，因此不需要
/// `ll_mod::registry::Registry`，可以落在依赖方向更上游的这里。
///
/// # 为什么取一个目标句柄，不是零参读活跃实体
///
/// 唯一的调用场景（`mods/example_mod/behavior.scm` 的
/// `guard-try-inspect`）问的是「**我看到的这个人**在不在潜行」，
/// 不是「我自己在不在潜行」——观察者与被观察者是两个不同的实体。
/// 与 `direction-toward` 同样接一个 [`ScriptEntityHandle`] 参数。
fn actor_stealthed(target: ScriptEntityHandle) -> SteelVal {
    with_active_world(SteelVal::BoolV(false), |world| {
        SteelVal::BoolV(
            world
                .actors
                .get(target.entity_id())
                .is_some_and(|agent| agent.stealthed),
        )
    })
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

/// `(nearby-actor-in-view)`：活跃实体视野内最近的一个实体句柄（不看
/// 敌对关系，真正的 FOV 可见性，不是平方距离近似）；没有则 `#f`。见
/// 模块文档「`nearby-actor-in-view`：真正的 FOV 可见性」一节。
fn nearby_actor_in_view() -> SteelVal {
    with_active_actor(SteelVal::BoolV(false), |actor| {
        with_active_world(SteelVal::BoolV(false), |world| match nearest_visible_actor(
            world,
            actor,
            NEARBY_ACTOR_VIEW_RADIUS,
        ) {
            Some(target) => ScriptEntityHandle::new(target)
                .into_steelval()
                .unwrap_or(SteelVal::BoolV(false)),
            None => SteelVal::BoolV(false),
        })
    })
}

/// 找出 `world` 中离 `self_id` 最近、且真的落在它 FOV 内的实体——
/// 委托给 [`ll_sim::ai_query::nearest_visible_actor`]，本文件不再持有
/// 第二份实现，理由见该模块文档「为什么这些函数住在 `ll-sim`」。
fn nearest_visible_actor(world: &WorldState, self_id: EntityId, radius: u32) -> Option<EntityId> {
    ll_sim::ai_query::nearest_visible_actor(world, self_id, radius)
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

/// `(dx, dy)`（环面最短带符号位移）→ 八向符号名。
///
/// 方向判定本身委托给 [`ll_sim::ai_query::direction_from_delta`]（唯一
/// 一份），本函数只负责把它翻成脚本侧的符号名——那个名字要与
/// [`crate::api::intent::direction_from_symbol`] 认的那一份逐字对应。
fn direction_symbol(dx: i32, dy: i32) -> &'static str {
    match ll_sim::ai_query::direction_from_delta(dx, dy) {
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
        Direction::NorthEast => "north-east",
        Direction::SouthEast => "south-east",
        Direction::SouthWest => "south-west",
        Direction::NorthWest => "north-west",
    }
}

/// 找出 `world` 中离 `self_id` 最近、且对它敌对的实体——委托给
/// [`ll_sim::ai_query::nearest_hostile`]，理由同
/// [`nearest_visible_actor`]。
fn nearest_hostile(world: &WorldState, self_id: EntityId) -> Option<EntityId> {
    ll_sim::ai_query::nearest_hostile(world, self_id)
}

#[cfg(test)]
mod tests {
    use ll_core::time::Tick;
    use ll_world::entity::AffiliationKind;
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
            known_recipes: Vec::new(),
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
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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

    /// 与 `test_world` 同一套构造,但把 `BaseTerrainIds` 也交回调用方——
    /// 本模块其余测试不需要 `wall_stone` 这类具体地形索引,`test_world`
    /// 因此丢弃了它;下面两条 `nearby_actor_in_view` 测试需要摆放一面
    /// 墙,必须拿到这份索引。
    fn test_world_with_terrain_ids() -> (WorldState, ll_world::terrain::BaseTerrainIds) {
        let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
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

    #[test]
    fn nearby_actor_in_view找到视野内最近的可见实体() {
        // 与 nearby_enemy 的区别:两个候选者都没有势力归属,对
        // nearby_enemy 而言二者互相敌对;本函数不看敌对关系,只看
        // FOV——这里只是先验证「视野内有目标时能找到最近的那个」这条
        // 基本路径,敌对与否不影响结果。
        // Arrange
        let mut world = test_world();
        let me = spawn_agent_at(&mut world, 5, 5);
        let near = spawn_agent_at(&mut world, 7, 5);
        let _far = spawn_agent_at(&mut world, 40, 5);
        set_active_actor(me);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = nearby_actor_in_view();
            clear_active_world();
            result
        };
        clear_active_actor();

        // Assert
        let handle = ScriptEntityHandle::from_steelval(&result).expect("视野内应有一个可见目标");
        assert_eq!(handle.entity_id(), near);
    }

    #[test]
    fn 隔着石墙的目标即使距离很近也看不见() {
        // 证明 nearby_actor_in_view 用的是真正的 FOV,不是距离近似——
        // 目标与观察者的切比雪夫距离只有 2(远小于查询半径),若这里用
        // 距离判定会误判为「看得见」;摆一整排石墙(阻挡视线)隔在两者
        // 之间,FOV 应当判定看不见。
        // Arrange
        let (mut world, terrain_ids) = test_world_with_terrain_ids();
        let me = spawn_agent_at(&mut world, 5, 5);
        let _target = spawn_agent_at(&mut world, 7, 5);
        // 竖直一整排石墙挡在 x=6 这一列,把 me 与 target 完全隔开
        // (只挡这一条水平线上的直接视线还不够——阴影投射可能绕过单点
        // 墙从斜向看到目标,一整列才能确保挡住全部路径)。
        for y in 0..12 {
            world
                .terrain
                .set_terrain(world.size.wrap(6, y), terrain_ids.wall_stone);
        }
        set_active_actor(me);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = nearby_actor_in_view();
            clear_active_world();
            result
        };
        clear_active_actor();

        // Assert
        assert_eq!(result, SteelVal::BoolV(false));
    }

    #[test]
    fn actor_stealthed如实回答目标的潜行状态() {
        // Arrange：两个实体，只有其中一个在潜行。
        let mut world = test_world();
        let visible = spawn_agent_at(&mut world, 0, 0);
        let sneaker = spawn_agent_at(&mut world, 2, 0);
        world
            .actors
            .get_mut(sneaker)
            .expect("刚生成必然存在")
            .stealthed = true;

        // Act
        let (visible_result, sneaker_result) = unsafe {
            set_active_world(&world);
            let a = actor_stealthed(ScriptEntityHandle::new(visible));
            let b = actor_stealthed(ScriptEntityHandle::new(sneaker));
            clear_active_world();
            (a, b)
        };

        // Assert
        assert_eq!(visible_result, SteelVal::BoolV(false));
        assert_eq!(sneaker_result, SteelVal::BoolV(true));
    }

    #[test]
    fn 潜行中的实体照样会被nearby_actor_in_view找到() {
        // 本批次核心设计选择的可执行断言：潜行**不是**隐身，FOV 一个
        // 字节都没改——见模块文档「潜行：为什么是一次判定的减值，不是
        // 一次可见性的改写」一节。若哪天有人"顺手"把潜行接进
        // nearest_visible_actor，这条测试立刻变红。
        // Arrange
        let mut world = test_world();
        let me = spawn_agent_at(&mut world, 5, 5);
        let sneaker = spawn_agent_at(&mut world, 7, 5);
        world
            .actors
            .get_mut(sneaker)
            .expect("刚生成必然存在")
            .stealthed = true;
        set_active_actor(me);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = nearby_actor_in_view();
            clear_active_world();
            result
        };
        clear_active_actor();

        // Assert：照样找得到。
        let handle = ScriptEntityHandle::from_steelval(&result).expect("潜行不影响可见性");
        assert_eq!(handle.entity_id(), sneaker);
    }

    #[test]
    fn 注册后脚本能调用actor_stealthed判断目标是否潜行() {
        // 端到端：真实脚本源码经 ScriptEngine::load_source 调用
        // actor-stealthed?，不是直接在 Rust 里调 actor_stealthed——与
        // `ll_mod::script_behavior_api` 的
        // `注册后脚本能调用self_has_profession判断当前职业` 同一条既有
        // 纪律。目标句柄由脚本自己从 nearby-actor-in-view 取，因此这条
        // 测试同时覆盖了 behavior.scm 里真实的调用形状。
        // Arrange
        let mut world = test_world();
        let me = spawn_agent_at(&mut world, 5, 5);
        let sneaker = spawn_agent_at(&mut world, 7, 5);
        world
            .actors
            .get_mut(sneaker)
            .expect("刚生成必然存在")
            .stealthed = true;
        let mut engine = ScriptEngine::new();
        register(&mut engine);
        engine
            .load_source("(define (probe) (actor-stealthed? (nearby-actor-in-view)))".to_string())
            .expect("这段源码只用到本模块注册的两个原语");

        // Act
        let result = unsafe {
            set_active_world(&world);
            set_active_actor(me);
            let result = engine.call_raw("probe", Vec::new());
            clear_active_actor();
            clear_active_world();
            result
        };

        // Assert
        assert_eq!(result, Ok(SteelVal::BoolV(true)));
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
