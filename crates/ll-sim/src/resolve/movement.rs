//! `resolve::movement`：智能体挪动自己：走一步、与人换位、开关潜行。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::torus::TorusPos;
use ll_world::entity::{Agent, EntityId};
use ll_world::space::Space;
use ll_world::state::WorldState;

use crate::effect::Effect;
use crate::intent::Direction;
use crate::timeline::action_cost;

use super::stats::effective_speed_from_dexterity;
use super::{
    BASE_ACTION_COST, EXPLORATION_SIGHT_RADIUS, STEALTH_MOVE_COST_PERMILLE, schedule_after,
};

/// [`Intent::ToggleStealth`](crate::intent::Intent::ToggleStealth) 的结算（潜行与盗贼被动批次）：读一次
/// 发起者当前的 [`ll_world::entity::Agent::stealthed`]，产出取反后的
/// 确定值，并按 [`BASE_ACTION_COST`] 消耗一个回合。
///
/// # 为什么切换本身要计费
///
/// 见 [`Intent::ToggleStealth`](crate::intent::Intent::ToggleStealth) 文档「为什么消耗一个回合」：不计费的话
/// 「每走一格之前开、走完立刻关」可以白嫖潜行的全部收益而完全绕开
/// 它唯一的代价（[`STEALTH_MOVE_COST_PERMILLE`] 的移动开销上升）。
/// 计费口径与 [`resolve_wait`](super::upkeep::resolve_wait) 完全相同（基础代价 × 敏捷速度），不是
/// 另起一个数字：切换姿态在时间轴上就是「这一回合我没干别的」。
///
/// # 为什么不检查任何前置条件
///
/// 没有可检查的东西：潜行不消耗资源、不要求地形、不要求技能解锁。
/// 与 [`resolve_pick_up`](super::inventory::resolve_pick_up)「脚下没东西就静默作废」那类需要读世界才能
/// 判断的意图不同，本意图恒合法——唯一的失败路径是发起者根本不存在
/// （已被同一批效果里更早的 `Effect::Kill` 收走），那一条走本文件
/// 统一的「查不到实体就返回空效果、不消耗时间」既有降级。
pub(super) fn resolve_toggle_stealth(world: &WorldState, actor: EntityId) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![
        Effect::SetStealth {
            actor,
            stealthed: !agent.stealthed,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// 从 `from` 朝 `dir` 迈一格落在哪——环面绕回由
/// [`ll_core::torus::TorusSize::wrap`] 负责（跨接缝时裸加减会算出界外
/// 坐标）。
///
/// 抽成函数与 [`occupant_at`] 是同一条理由，见那里。
pub(crate) fn step_destination(world: &WorldState, from: TorusPos, dir: Direction) -> TorusPos {
    let (dx, dy) = dir.delta();
    world.size.wrap(from.x() + dx, from.y() + dy)
}

/// `pos` 这一格上站着谁（`mover` 自己不算），没有就是 `None`。
///
/// # 为什么必须只有这一份实现
///
/// 两处消费它：[`resolve_move`] 的占位检查与
/// `crate::turn::route_move_into_occupant` 的撞格路由。这两处问的是
/// **同一个问题**——「我要迈进的那一格上站着谁」——而它们的答案必须
/// 逐字一致：路由据此决定这一步是攻击、互换还是原样放行，占位检查据此
/// 决定这一步能不能落地。两边各写一遍的真正代价不是多几行，是那条
/// **平局打破规则**（同一格站着多于一个单位时取谁）会各自漂移，症状是
/// 「路由认为 A 挡路、占位检查认为 B 挡路」这种玩家可见却极难归因的
/// 不一致。
///
/// 直接在 `world.actors` 上查找，不要求调用方另外维护一份「全部实体」
/// 的列表（`p3_acceptance` 曾经为此单独传一个 `&[Combatant]` 参数，纯属
/// 多余）。同一格站着多于一个单位时取遍历序第一个——
/// [`ll_world::entity::Arena::iter_with_id`] 的顺序由 `Vec` 支撑，固定
/// 且与任何哈希容器的迭代顺序无关（约束 C5）。
///
/// 返回 `(id, &Agent)` 而不是只返回 id：撞格路由紧接着就要拿那个
/// `Agent` 去问 `crate::ai_query::declared_hostile`，返回 id 会逼它再查
/// 一次同一张表，而「两次查找之间表没变」这件事需要读者自己去确认。
pub(crate) fn occupant_at(
    world: &WorldState,
    pos: TorusPos,
    mover: EntityId,
) -> Option<(EntityId, &Agent)> {
    world
        .actors
        .iter_with_id()
        .find(|(id, other)| *id != mover && other.pos == pos)
}

/// 朝某方向移动一格：按目的地的地形与是否有人站着分四种情形处理。
///
/// - 目的地是一格「撞入即开」的地形（[`ll_world::terrain::TerrainTable::opens_into`]
///   有值，例如关着的门）：产生把该格改写成 `opens_into` 目标地形的
///   效果，而不是移动效果——门挡住了这一步，但「撞门」本身是有意义的
///   动作，不该像撞墙一样什么都不发生。**这条规则是任何地形都能声明的
///   属性，不是只对某个硬编码地形 ID 生效的特判**——见
///   `ll_world::terrain` 模块文档「`opens_into`」一节：这正是本次迁移
///   撞见并修掉的一处 API 洞，mod 现在可以给自己的地形也声明同样的
///   行为。
/// - 目的地完全不可通行（墙、窗等）：**不产生 `Effect::MoveTo`，但仍
///   产生 `Effect::ScheduleNext`**——项目所有者决策：撞墙本身也是一次
///   真实的行动尝试（伸手推了一下、发现推不开），应当消耗时间，只是
///   位置不变；耗时按 [`BASE_ACTION_COST`] 计费，不查地形的 `move_cost`
///   （那是「走完整段距离」的代价，撞墙这一步根本没有走完，用它定价
///   不成立，见 [`resolve_wait`](super::upkeep::resolve_wait) 同样按基准代价计费的理由）。
/// - **目的地站着另一个存活实体**：与撞墙同一个口径——不产生
///   `Effect::MoveTo`，仍产生 `Effect::ScheduleNext`。见下面「每格至多
///   站一人」一节。
/// - 目的地可通行且空着：产生移动效果，行动耗时按该地形的分级
///   `move_cost` 计算——浅水、山地这类「过得去但更慢」的地形因此耗时
///   更长；若移动的是玩家自己，额外追加一条
///   [`Effect::MarkExplored`]（见其文档），把探索记忆的写入接到这唯一
///   的移动落点。
///
/// # 「每格至多站一人」：本函数是这条不变式**唯一**被强制的地方
///
/// 这条规则此前只写在注释里（`ll_game::world::place_roster` 与
/// `crate::turn` 的撞格路由各有一句），**在本批次之前一行代码都没有
/// 强制它**：本函数一行占位检查都没有，两个非敌对 NPC 因此可以直接
/// 摞在同一格上。项目所有者裁定把它升级成真正被强制的不变式，落点就
/// 是这里——移动是实体改变位置的主要路径，堵住它就堵住了绝大多数
/// 违反。
///
/// **作用域必须说清楚：它只在移动路径上强制，不是全局强制。**
/// [`resolve_exit_space`](super::portal::resolve_exit_space) 自行构造 `Effect::MoveTo` 回锚点、不查占位，
/// 仍然造得出两人同格——那处缺口是记录在案的、等待一条独立裁定的
/// 剩余缺口，见该函数文档。任何人在别处读到「每格至多站一人」时，读到
/// 的都应该是这条带作用域的说法，不是一句会被 `resolve_exit_space`
/// 当场证伪的全称断言。
///
/// # 为什么排在开门分支**之前**
///
/// 不变式的字面意思不区分目的地是门还是平地。若占位检查排在开门分支
/// 之后，「门那一格站着人」会走成：先把门推开、消耗一回合 → 下一回合
/// 才发现人挡着——一个要两回合才识破的怪异结果。
///
/// 排在 `terrain_at` 那条「目的地区块尚未常驻 → 静默作废」分支**之后**：
/// 那条是项目所有者明确裁定不改的既有行为，顺序不动。
///
/// # 为什么不过滤 `current_space`
///
/// 进了 `Interior` 的 Agent 其 `pos` 仍指向地表锚点格（见下面那条
/// `Space::Surface` 前置守的不变式），因此会「幽灵占用」那一格。这是
/// **既有行为**——`ll_game::world::place_roster` 的 `occupied` 集合同样
/// 不过滤，`crate::turn` 的撞格路由也不过滤。本函数保持一致，不在这里
/// 引入第二套「谁算占着这一格」的判据：两套判据必然漂移，而漂移的表现
/// 是「路由说 A 挡路、占位检查说没人挡路」这种极难归因的不一致。
///
/// # 撞到人为什么仍然消耗一次行动
///
/// 与撞墙同一条既有裁定：这是一次真实的行动尝试。更要紧的是效果非空
/// ⇒ 不触发 [`crate::turn::TurnEngine::perform`] 的进展保证去补跑一次
/// `Intent::Wait`，因此不存在「撞人这一步被计费两次」的可能；反过来，
/// 若这里返回空 `Vec`，非受控实体那条路会被进展保证兜住（不至于死
/// 循环），但受控实体那条路会白按一次——而「前面站着人」是一个玩家
/// 完全看得见的、确定的结果，不是白按。
///
/// # 为什么只有玩家移动才追加 `MarkExplored`
///
/// 本函数同时服务玩家与 NPC——`actor` 是任意实体。[`WorldState::exploration`]
/// 却只代表玩家一个人的视角（见其字段文档「为什么按角色只存一份」）。
/// 若不加区分地让每个 NPC 的移动都追加一条 `MarkExplored`，游荡的怪物
/// 会替玩家「看见」它们自己路过的地方——那是把探索记忆的语义换成了
/// 「世界上任意实体去过哪」，与「玩家亲眼见过哪」是两个不同的东西，
/// 后者才是战争迷雾要回答的问题。这里用 `world.player_entity ==
/// Some(actor)` 这一个比较收住范围，不需要改 `Intent`/`Effect` 的
/// 形状去区分「谁在动」。
pub(super) fn resolve_move(world: &WorldState, actor: EntityId, dir: Direction) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // Interior 内部漫游不在本批次范围内——见模块文档「Interior 内部
    // 移动的范围边界」一节。静默无效，不改 agent.pos，保住「进入
    // Interior 后 Agent.pos 不变」这条不变式。
    if !matches!(agent.current_space, Space::Surface { .. }) {
        return Vec::new();
    }
    let dest = step_destination(world, agent.pos, dir);
    // resolve 必须是纯函数（C1），不能触发 SurfaceStore 的按需生成——
    // 见 WorldState::terrain_at 文档「resolve 只读、加载收窄到……」。
    // 目的地所属区块尚未常驻时（真正的邻域缓冲维护接线是设计文档
    // 任务 14 的范围，本次迁移之后正常游玩路径下应恒已常驻），这是防御
    // 性兜底而非玩家能在正常游玩中触发的情形，保守地不产生任何效果、
    // 也不消耗时间——不是让整个结算 panic。**与下方撞墙分支不同**：
    // 撞墙是「查得到地形、确认过不去」的确定结果，值得消耗一次行动；
    // 这里根本查不到地形，无法判断这一步「本该」耗时多久，静默作废
    // 更安全。
    let Some(terrain) = world.terrain_at(dest) else {
        return Vec::new();
    };
    let speed = effective_speed_from_dexterity(agent.stats.dexterity);

    // 「每格至多站一人」——见本函数文档同名一节。排在开门分支之前：
    // 不变式不区分目的地是门还是平地，排在后面会让「门上站着人」变成
    // 两回合才识破的怪事。
    if occupant_at(world, dest, actor).is_some() {
        // 与撞墙同一个口径：位置不变（不产生 `Effect::MoveTo`），但仍
        // 推进时间轴，按 `BASE_ACTION_COST` 计费而不是目的地地形的
        // `move_cost`——这一步根本没有走完那一格。
        let cost = action_cost(BASE_ACTION_COST, speed);
        return vec![Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        }];
    }

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
        // 撞墙仍消耗时间——见本函数文档「目的地完全不可通行」一节。
        // 位置不变（不产生 `Effect::MoveTo`），只推进时间轴。
        let cost = action_cost(BASE_ACTION_COST, speed);
        return vec![Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        }];
    }

    // 潜行时移动开销上升（潜行与盗贼被动批次）——倍率与完整论证见
    // `STEALTH_MOVE_COST_PERMILLE`。乘在**地形开销**上、`action_cost`
    // 换算敏捷速度之前：潜行放慢的是「挪这一格本身有多费事」，敏捷高
    // 的人潜行同样比自己不潜行时慢，两者是可以叠乘的两层，不是互相
    // 替代。饱和乘法防止一个极端 `move_cost` 在这一步环绕
    // （`u32::saturating_mul`，与本文件其余「内容作者填的数值一律饱和
    // 运算」同一条既有纪律）。
    //
    // **只挂在这一条真的挪动了位置的分支上**：上面撞墙/开门两条分支
    // 各自提前返回，它们按 `BASE_ACTION_COST` 计费而不是地形开销——
    // 潜行不该让「推开一扇门」或「撞上一堵墙」也变慢，那两件事与
    // 「悄悄挪一格」不是同一个动作。
    let terrain_cost = terrain.move_cost(&world.terrain_table);
    let terrain_cost = if agent.stealthed {
        terrain_cost
            .saturating_mul(STEALTH_MOVE_COST_PERMILLE)
            .saturating_div(1000)
    } else {
        terrain_cost
    };
    let cost = action_cost(terrain_cost, speed);
    let mut effects = vec![
        Effect::MoveTo { actor, pos: dest },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ];
    // 只在移动者是玩家、且这一步真的挪动了位置（本分支恒如此）时追加
    // 探索标记——见本函数文档「为什么只有玩家移动才追加」一节。没有
    // `MoveTo` 就不该有 `MarkExplored`：站着不动（`Intent::Wait`）或
    // 撞墙（上面 `blocks_move` 分支提前返回空 `Vec`）都不会走到这里，
    // 天然不会为「原地不动」重复标记同一批格子，这正是避免每帧全量
    // 重写探索位图的做法（见 `Effect::MarkExplored` 文档「何时才触发」
    // 一节）。
    if world.player_entity == Some(actor) {
        effects.push(Effect::MarkExplored {
            origin: dest,
            radius: EXPLORATION_SIGHT_RADIUS,
        });
    }
    effects
}

/// 与相邻格上的另一个实体互换位置。
///
/// # 谁产出这个意图
///
/// 只有 `crate::turn` 的撞格路由：一次 [`Intent::Move`](crate::intent::Intent::Move) 的目的地站着
/// 一个**非敌对**实体时，「走过去」的含义是「和他换位置」（项目所有者
/// 裁定）。敌对时那条路由产出的是 [`Intent::Attack`](crate::intent::Intent::Attack)，走
/// [`resolve_attack`](super::combat::resolve_attack)。
///
/// # 三道前置
///
/// 1. 两个实体都还活着（`world.actors` 里查得到）。
/// 2. 不是自己和自己换（路由那一层已经排除了，这里仍然防御一次——
///    自己和自己换位置是一次零变化的世界写入，没有任何规则依据）。
/// 3. 两者相邻（切比雪夫距离恰好 1）。互换位置是**一步移动**的另一种
///    结果，不是一次瞬移；路由那一层只会从一次单格 `Move` 产出它，
///    这道前置守的是将来别处直接构造 `Intent::Swap` 的调用方。
///
/// 三道任一不成立就静默作废（空 `Vec`），与本模块其余结算函数同一条
/// 既有纪律。非受控实体走到这里时空 `Vec` 不会造成死循环——
/// `crate::turn::TurnEngine::perform` 的进展保证兜住了，见该方法文档。
///
/// # 为什么不查目的地地形，也不按地形开销计费
///
/// 目的地那一格**站着一个人**，这件事本身就是「这格站得住」的证明——
/// 再去查一次地形既多余，又会凭空多出一条「地形所属区块尚未常驻 →
/// 返回空 `Vec`」的路径，而那正是本批次修掉的那个死循环的来源
/// （见 `crate::turn::TurnEngine::perform` 文档「进展保证」一节）。
///
/// 计费取 [`BASE_ACTION_COST`]，与撞墙、开门两条分支同一个口径：那两条
/// 同样是「这一步没按地形走完一格」的动作。互换位置是两个人贴着身子
/// 侧过去，不是踩过那一格的地表，按目的地地形开销计费反而说不通。
///
/// # 探索标记
///
/// 玩家真的换到了新的一格，因此与 [`resolve_move`] 那条真的挪了位置的
/// 分支一样追加 [`Effect::MarkExplored`]——少了它，玩家会发现「换位置
/// 走过去」的那一格周围没被点亮，而正常走过去就会。
///
/// # 只重排发起者
///
/// 被换位的那一方**不消耗自己的回合**：他没有做出任何决定，是被挪过去
/// 的。这与传统 roguelike 的换位手感一致，也是本函数只产出一条针对
/// `actor` 的 [`Effect::ScheduleNext`] 的原因。
pub(super) fn resolve_swap(world: &WorldState, actor: EntityId, with: EntityId) -> Vec<Effect> {
    if actor == with {
        return Vec::new();
    }
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(other) = world.actors.get(with) else {
        return Vec::new();
    };
    if world.size.chebyshev(agent.pos, other.pos) != 1 {
        return Vec::new();
    }
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    let dest = other.pos;
    let mut effects = vec![
        Effect::SwapPositions { a: actor, b: with },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ];
    if world.player_entity == Some(actor) {
        effects.push(Effect::MarkExplored {
            origin: dest,
            radius: EXPLORATION_SIGHT_RADIUS,
        });
    }
    effects
}
