//! `resolve::portal`：门与空间的通行：开门、关门、进出室内空间。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::torus::TorusPos;
use ll_world::entity::EntityId;
use ll_world::space::{Space, SpaceId};
use ll_world::state::WorldState;

use crate::effect::Effect;
use crate::timeline::action_cost;

use super::movement::occupant_at;
use super::stats::effective_speed_from_dexterity;
use super::{BASE_ACTION_COST, schedule_after};

/// 开启某处的门：目的地不是一格「撞入即开」的地形时，位置与地形都不
/// 变，但仍消耗一次行动的时间——与 [`resolve_move`](super::movement::resolve_move) 撞墙时的处理是
/// 同一类判断（都是「查得到目标、确认这个动作在此处不成立」的确定
/// 结果，值得消耗一次行动，而不是像目标区块未常驻那样彻底放弃判断）,
/// 见 [`resolve_move`](super::movement::resolve_move) 文档「目的地完全不可通行」一节；这里同样查表，
/// 不再恒等比较某个硬编码地形 ID，见其「`opens_into`」一节。
///
/// 目的地所属区块尚未常驻（`world.terrain_at` 落空）是另一种情形，
/// 与 [`resolve_move`](super::movement::resolve_move) 对应分支同一条纪律：无法判断这一步「本该」耗时
/// 多久，静默作废、不消耗时间。
pub(super) fn resolve_open_door(
    world: &WorldState,
    actor: EntityId,
    pos: (i32, i32),
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let door_pos = world.size.wrap(pos.0, pos.1);
    // 同 resolve_move：只读查询，未常驻时无法判断耗时，静默作废、不
    // panic、不触发生成、不消耗时间——见本函数文档。
    let Some(terrain) = world.terrain_at(door_pos) else {
        return Vec::new();
    };
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    let Some(open_kind) = terrain.opens_into(&world.terrain_table) else {
        // 目标不是（或已经不是）一扇能开的门——仍消耗时间,见本函数
        // 文档。位置与地形都不变,只产出排期效果。
        return vec![Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        }];
    };

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

/// 关上某处的门（交互列表批次：所有者裁定「我希望交互也能包括……开关
/// 门」）。
///
/// # 四道前置，任一不成立都只消耗时间、不改地形
///
/// 1. **发起者存在**（否则连耗时都算不出来，静默作废）。
/// 2. **目标区块已常驻**——`world.terrain_at` 落空时静默作废、不消耗
///    时间，与 [`resolve_open_door`]/[`resolve_move`](super::movement::resolve_move) 同一条纪律：查不
///    到地形就无法判断这一步「本该」耗时多久。
/// 3. **目标是一格「已打开形态」**——
///    [`ll_world::terrain::TerrainTable::closes_into`] 有值。反查
///    `opens_into` 而不是新加一条内容字段，理由见那个方法的文档；
///    副作用是**mod 自己声明的门自动可以被关上**，不需要内容作者多写
///    一个字。
/// 4. **那一格上没有实体、也没有立着的家具**——否则门会关在人身上，
///    或者把一座炉子封进墙里。占位查找复用批次 1 落地的
///    [`occupant_at`]（不另写一份），家具判据是
///    [`ll_world::item::GroundItemStack::placed`]，与
///    `resolve_place` 的「一格至多立一件」用的是同一个字段。
///
/// **散落在地上的东西不挡门**：它们本来就躺在地上、和门在同一格并不
/// 矛盾（一把掉在门槛上的匕首不该让门关不上）。挡门的只有「站着的人」
/// 与「立着的东西」这两类真正占据了这一格的存在。
///
/// 前置 3/4 不成立时**仍然消耗一次行动**，与 [`resolve_open_door`] 对
/// 着一格不是门的地方按下去、与 [`resolve_move`](super::movement::resolve_move) 撞墙，是同一个口径：
/// 「查得到目标、确认这个动作在此处不成立」是一个确定结果，值得消耗
/// 一次行动。
///
/// # 为什么不在交互列表那一层先把关不上的门筛掉
///
/// 那会让同一条判据存在两份，迟早分叉（分叉的表现是「列表里能选，按
/// 下去没反应」或者更糟的「明明关得上，列表里却没有」）。这条纪律与
/// `ll_game::player_action::craft_entries` 文档里写明的完全一致：
/// 玩法前置只住在 `resolve`，呈现层不复制。
pub(super) fn resolve_close_door(
    world: &WorldState,
    actor: EntityId,
    pos: (i32, i32),
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let door_pos = world.size.wrap(pos.0, pos.1);
    let Some(terrain) = world.terrain_at(door_pos) else {
        return Vec::new();
    };
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    let idle = vec![Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    }];

    let Some(closed_kind) = world.terrain_table.closes_into(terrain) else {
        return idle; // 前置 3：这一格不是一扇开着的门。
    };
    // 前置 4：门口被占着。两种占法由 [`door_close_blocker`] 判定——
    // **输入层用的是同一个函数**，见它的文档。
    if door_close_blocker(world, door_pos, actor).is_some() {
        return idle;
    }

    vec![
        Effect::SetTerrain {
            pos: door_pos,
            kind: closed_kind,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// 一扇开着的门关不上的两种原因——**结算层与输入层共用的那份判据的
/// 值域**。
///
/// # 为什么要分成两条，而不是一句「有东西挡着」
///
/// 项目所有者 2026-08-29 的裁定给的是**两句**文案：「门口有人挡着」
/// 与「门口立着东西」（`knowledge/handoff/2026-08-28-session-handoff.md`
/// 第〇之二节第 6 条）。结算层这两条本来就是两道独立前置
/// （[`resolve_close_door`] 的前置 4a / 4b），合成一条等于把**已经
/// 分开的信息**在呈现层重新丢掉。
///
/// `knowledge/design/ui-and-navigation.md` 九节 F1 曾把它收敛成一个
/// `DoorBlocked` 变体；那一条已按所有者原话更正，更正记在该节原地。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorCloseBlocker {
    /// 门口站着另一个活着的实体。
    Occupant,
    /// 门口立着一件家具（`GroundItem::placed`）。
    PlacedObject,
}

/// 这一格上的门此刻**关不上**的原因，`None` 表示关得上。
///
/// # 一份判据，两个调用点
///
/// [`resolve_close_door`] 的前置 4 用它，`ll_game::player_action` 的
/// 关门分派**也**用它——后者要在提交意图**之前**就能告诉玩家「为什么
/// 关不上」（规格 F1：此前只有一句笼统的「这一下没有起作用」）。
///
/// 规格原文写的是「这条判据输入层自己就能答」，但**照着在输入层再写
/// 一遍**正是 ADR 0021 点名要拦的形状：同一条判据两份实现，改了一份
/// 另一份不会有任何东西报错。提成一个公开函数之后，两个调用点分叉在
/// 结构上不可能发生。
///
/// **本函数不判「这一格是不是一扇开着的门」**——那是
/// [`ll_world::terrain::TerrainTable::closes_into`] 的事，输入层的候选
/// 列表已经按它分过类（`InteractTarget::Door` 的 `DoorAction::Close`）。
/// 本函数只回答「挡没挡着」。
pub fn door_close_blocker(
    world: &WorldState,
    door_pos: TorusPos,
    actor: EntityId,
) -> Option<DoorCloseBlocker> {
    if occupant_at(world, door_pos, actor).is_some() {
        return Some(DoorCloseBlocker::Occupant);
    }
    if world
        .ground_items
        .iter()
        .any(|ground| ground.pos == door_pos && ground.placed)
    {
        return Some(DoorCloseBlocker::PlacedObject);
    }
    None
}

/// 尝试进入 `target` 这个具体的 `Interior` 空间实例。
///
/// 三重校验，任一失败都静默作废（不产生效果，与撞墙同一种处理）：
/// 1. `actor` 当前必须在地表——已经在某个 `Interior` 里时不允许直接
///    「传送」进另一个（不支持 `Interior` 嵌套 `Interior`，本批次范围
///    之外）。
/// 2. `target` 必须真实存在于 `world.interiors`。
/// 3. `target` 的入口锚点必须等于 `actor` 当前所在的世界格——玩家必须
///    真的站在入口上，不能隔空进入。
///
/// 通过校验后，进入哪一层由 [`entry_floor`] 决定。
pub(super) fn resolve_enter_space(
    world: &WorldState,
    actor: EntityId,
    target: SpaceId,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    if !matches!(agent.current_space, Space::Surface { .. }) {
        return Vec::new();
    }
    let Some(interior) = world.interiors.get(target) else {
        return Vec::new();
    };
    if interior.anchor != agent.pos {
        return Vec::new();
    }
    let Some(floor) = entry_floor(interior) else {
        return Vec::new();
    };
    vec![Effect::ChangeSpace {
        actor,
        space: Space::Interior {
            id: target,
            floor,
            anchor: interior.anchor,
            profile: interior.profile,
        },
    }]
}

/// 从入口进入 `Interior` 时应该落在哪一层：优先取 0 层（约定俗成的
/// 「地面层」），若这个 `Interior` 恰好没有 0 层（稀疏楼层，见
/// [`ll_world::interior`] 模块文档「稀疏性」一节），退而取已生成楼层里
/// 编号最小的一个。若一层都还没生成，返回 `None`——这不是编程错误
/// （`Interior` 允许先插入实例、楼层由生成器按需补齐，见其模块文档
/// 「与共享常驻预算的关系」），只是这一步无法进入，与撞墙同一种
/// 「静默作废」处理。
pub(super) fn entry_floor(interior: &ll_world::interior::Interior) -> Option<i16> {
    let floors = interior.floor_numbers();
    if floors.contains(&0) {
        Some(0)
    } else {
        floors.first().copied()
    }
}

/// 退出当前所在的 `Interior`，返回地表。
///
/// 在地表触发（`agent.current_space` 不是 `Interior`）时静默作废——见
/// 模块文档「已知的范围边界」一节的同一套处理方式。
///
/// 产出两个效果：把 `current_space` 换回地表（`profile` 取自
/// [`WorldState::surface_profile`]，见模块文档「`Interior` 退出如何
/// 拿到地表 profile」一节），以及把 `pos` 显式写回 `Interior` 的锚点
/// ——`Interior` 内部漫游本批次不接线（见模块文档），`pos` 理论上从
/// 进入起就没变过，这里仍然显式写一遍而不是依赖「反正没人动过它」：
/// 显式写入让这条不变式不依赖调用方是否恰好遵守了另一条完全不同的
/// 规则（`resolve_move` 对 `Interior` 静默无效），两条防线互相独立更
/// 安全。
///
/// # 已知缺口：本函数**不查锚点上有没有人**（记录在案，等一条裁定）
///
/// [`resolve_move`](super::movement::resolve_move) 现在强制「每格至多站一人」（见其文档同名一节），
/// 但本函数自行构造 `Effect::MoveTo { pos: anchor }`，**不经过那条
/// 检查**：锚点那一格上站着别人时，退出建筑会造出两人同格。这条
/// 不变式因此只在**移动路径**上强制，不是全局强制——`resolve_move`
/// 的文档也是这么写的，任何人不得把它读成一句全称断言。
///
/// 本批次刻意不堵它，因为堵它必须先回答一个还没有答案的设计问题：
/// **退出时锚点站着人，人去哪？** 三条候选（挤不出去、把对方挪到旁边
/// 一格、把自己挪到旁边一格）各自蕴含不同的玩法后果，而其中「把 NPC
/// 随机挪到旁边一格」与项目所有者给出的另一条附带设计输入（作弊传送
/// 时如何安置挡路的 NPC）是同一个尚未落地的机制。在那条机制落地之前
/// 就地拍一个，等于把一条本该统一的规则先分叉成两份。
///
/// 本文件的 `退出建筑时锚点被占会造出两人同格这条缺口仍然存在` 钉住了**当前
/// 行为**（不是当前的正确行为，是当前的实际行为）——将来真正堵这个
/// 缺口的人会立刻看到自己改动了什么,而不是在一片全绿里悄悄换掉一条
/// 语义。
pub(super) fn resolve_exit_space(world: &WorldState, actor: EntityId) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Space::Interior { anchor, .. } = agent.current_space else {
        return Vec::new();
    };
    let (zone, _) = world.terrain.layout().tile_to_zone(anchor);
    vec![
        Effect::ChangeSpace {
            actor,
            space: Space::surface(zone, world.surface_profile),
        },
        Effect::MoveTo { actor, pos: anchor },
    ]
}
