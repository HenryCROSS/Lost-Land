//! 回合引擎：把 [`Timeline`] 的弹出顺序、玩家输入路由（撞人即攻击）、
//! 固定策略 AI（朝玩家移动、相邻则攻击）粘合起来，是本 demo 里唯一
//! 「知道游戏规则」的地方——按键到 `Intent`、`resolve` 到 `Effect`、
//! `apply` 落地这条链路本身完全走 `ll_sim` 现成的公开 API，这个模块
//! 只负责「什么时候该问谁要一个 `Intent`」。
//!
//! # 为什么这里要接住 `resolve` 明确不做的两件事
//!
//! `ll_sim::resolve` 的模块文档写明它刻意不做「移动目的地站着别的实体
//! 就派生成攻击」（需要「同一格多个实体时打谁」这类新规则，规格没有
//! 交代）；`ll_sim` 也没有内置任何 AI。这两件事都留给了调用方——本
//! demo 正是那个调用方，[`route_player_intent`] 与 [`ai_intent`] 各自
//! 实现一版最朴素的策略，且都建立在已有的 `Intent`/`resolve` 之上，
//! 不绕过它们直接改世界。

use ll_core::torus::TorusPos;
use ll_platform::input::InputState;
use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_sim::intent::{Direction, Intent, intent_from_input};
use ll_sim::resolve::resolve;
use ll_sim::timeline::{Timeline, TimelineEntry};
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

use crate::spawn::Combatant;

/// 一条伤害飘字在屏幕上停留的帧数（60fps 下约 0.75 秒）。
///
/// 纯呈现层的时间基准，故意用帧数而非墙钟时长——与
/// `ll_platform::window::FrameId` 同一纪律（见其文档）：整数帧号可以
/// 安全地驱动动画计时，不引入浮点或不确定的墙钟耗时。
pub(crate) const DAMAGE_POPUP_LIFETIME_FRAMES: u32 = 45;

/// [`TurnEngine::advance_ai`] 单次调用最多结算的 AI 回合数，超过就放弃
/// 本帧剩余的推进——防止任何未预见的「行动不产生 `ScheduleNext`」缺陷
/// 冻结整个事件循环，见该方法文档「必须保证进展」一节。取值远大于
/// 本 demo 实际会用到的敌人数量（3 个），正常运行中不会触发。
const MAX_STEPS_PER_ADVANCE: u32 = 10_000;

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

/// 回合引擎：包一层 [`Timeline`]，额外持有「已经弹出、但还没配上
/// `Intent`」的那一条待行动记录。
///
/// # 为什么需要 `pending`
///
/// `Timeline::pop_next` 一旦调用就从队列里永久移除那条记录。轮到玩家
/// 行动时，本帧的玩家输入可能还没到（没按任何方向/等待键）——若这时
/// 已经 `pop_next` 却又无法立即结算，这条记录就会丢失，玩家会凭空
/// 跳过一次行动。`pending` 就是「已弹出、等待被消费」的缓冲区：非玩家
/// 回合立刻消费掉，玩家回合則等到 [`TurnEngine::try_player_turn`]
/// 拿到一个非空 `Intent` 才消费。
pub(crate) struct TurnEngine {
    timeline: Timeline,
    pending: Option<TimelineEntry>,
}

impl TurnEngine {
    /// 用一个已经排好初始行动的时间轴建立回合引擎。
    pub(crate) fn new(timeline: Timeline) -> TurnEngine {
        TurnEngine {
            timeline,
            pending: None,
        }
    }

    /// 结算并应用一个实体的一次行动：设定世界时钟为该实体计划行动的
    /// 时刻，跑 `resolve` → 逐个 `apply`，产生的伤害记进 `popups`，
    /// 实体死亡则从时间轴移除其残留记录，存活则重新排入下一次行动。
    ///
    /// 这正是简报「完整调用链」里 `resolve(&world, &intent) -> Vec<Effect>`
    /// 与 `for effect in effects: apply(&mut world, &effect)` 两步的
    /// 实际调用点——`apply` 全程只经这一处写世界，符合约束 C1。
    fn perform(
        &mut self,
        world: &mut WorldState,
        entry: TimelineEntry,
        intent: Intent,
        popups: &mut Vec<DamagePopup>,
    ) {
        world.clock = entry.at;
        let effects = resolve(world, &intent);
        for effect in &effects {
            if let Effect::Damage { target, amount } = effect
                && let Some(agent) = world.actors.get(*target)
            {
                // 必须在 apply 之前读位置：若同一批效果里紧接着一个
                // Kill（见 ll_sim::resolve::resolve_attack），apply 后
                // 该实体已从 Arena 里销毁，位置就再也取不到了。
                popups.push(DamagePopup {
                    pos: agent.pos,
                    amount: *amount,
                    remaining_frames: DAMAGE_POPUP_LIFETIME_FRAMES,
                });
            }
            apply(world, effect);
            if let Effect::Kill { target } = effect {
                // Timeline 与 WorldState 是两个独立的存储（见
                // `ll_sim::timeline` 模块文档），apply 只知道
                // WorldState，清理时间轴里残留的死者行动记录是调用方
                // （这里）的职责。
                self.timeline.remove(*target);
            }
        }
        if let Some(agent) = world.actors.get(entry.actor) {
            self.timeline.schedule(entry.actor, agent.next_action_at);
        }
    }

    /// 反复弹出并结算非玩家实体的行动，直到轮到玩家或队列耗尽——这正是
    /// 「快角色在慢角色一次行动窗口内行动多次」肉眼可见的落地点：敏捷
    /// 越高的敌人，`next_action_at` 增量越小（见 `ll_sim::resolve` 的
    /// `effective_speed_from_dexterity`），本函数会在玩家的一次输入
    /// 之间把它反复弹出结算好几次。
    ///
    /// 返回按结算顺序排列的行动者列表——调用方（渲染层或测试）据此就能
    /// 数出「这段窗口里谁被结算了几次」，不必自己重新实现一遍时间轴
    /// 推进逻辑。
    ///
    /// # 必须保证进展（曾经的真实死循环）
    ///
    /// [`ai_intent`] 已经会避开明显不可通行的下一格（见其文档），但
    /// 这条防线本身仍建立在「AI 选的方向恰好可行」这个假设上——万一
    /// 未来某次改动让 `ai_intent` 又产出一个 `resolve` 会判定为空效果
    /// 的 `Intent`（例如撞墙的 `Move`），[`Self::perform`] 就不会产出
    /// `Effect::ScheduleNext`，该实体的 `next_action_at` 原地不动，
    /// 重新排入时间轴后会在**同一个 tick** 被立刻弹出，陷入死循环——
    /// 这不是假设的风险：在给 [`ai_intent`] 补上地形判断之前，这里
    /// 曾经因为快速敌人朝玩家方向的下一格恰好是深水而真实卡死过，
    /// 单元测试跑了一分钟没结束才被发现。[`MAX_STEPS_PER_ADVANCE`]
    /// 是修好根因之外的第二道防线：即使某次改动又引入了同一类缺陷，
    /// 单帧最多空转这么多步就会放弃，把已经死循环的那个实体的
    /// `pending` 状态原样交还给下一帧，而不是冻结整个事件循环。
    ///
    /// # 玩家死亡必须在循环内部逐次核查，不能只在入口查一次
    ///
    /// 玩家不是本函数唯一处理的实体——本函数每一步都可能结算一个
    /// 存活敌人对玩家的攻击，玩家因此完全可能在循环**进行到一半**时
    /// 死亡（[`Self::perform`] 处理 `Effect::Kill` 时会把死者从
    /// [`Timeline`] 里移除，见其实现）。玩家一旦被移出时间轴，
    /// 「弹出的条目属于玩家」这条唯一的提前返回条件就再也不会成立：
    /// 若这里只在循环开始前查一次玩家是否存活，玩家死后循环会继续
    /// 反复结算其余存活敌人，直到耗尽 [`MAX_STEPS_PER_ADVANCE`] 才
    /// 放弃——这正是真实发生过的缺陷：玩家死亡的那一帧要空转满
    /// 10000 步才能画出下一帧，肉眼表现为长达数秒的卡顿。修法是把
    /// 「玩家是否还在 `world.actors` 里」的核查放进循环体本身、
    /// 每一步都问一次，玩家一死就能在下一步立即察觉并返回，不必等到
    /// 弹出玩家自己的条目（那条条目已经不存在了）。
    pub(crate) fn advance_ai(
        &mut self,
        world: &mut WorldState,
        player: EntityId,
        popups: &mut Vec<DamagePopup>,
    ) -> Vec<EntityId> {
        let mut acted = Vec::new();
        for _ in 0..MAX_STEPS_PER_ADVANCE {
            if world.actors.get(player).is_none() {
                // 玩家可能在上一步里被某个敌人杀死——玩家的时间轴条目
                // 已经被 `perform` 清理掉，不会再有机会命中下面
                // `entry.actor == player` 那条返回路径，必须在这里
                // 单独察觉并立刻收工，否则会陷入本方法文档「玩家死亡
                // 必须在循环内部逐次核查」一节描述的那种空转。
                return acted;
            }
            if self.pending.is_none() {
                self.pending = self.timeline.pop_next();
            }
            let Some(entry) = self.pending else {
                return acted;
            };
            if world.actors.get(entry.actor).is_none() {
                // 时间轴可能残留已死实体的条目（见 Timeline::remove
                // 文档）；正常情况下 Kill 已经清理过，这里仍防御一次。
                self.pending = None;
                continue;
            }
            if entry.actor == player {
                return acted;
            }
            let intent = ai_intent(world, entry.actor, player);
            self.perform(world, entry, intent, popups);
            acted.push(entry.actor);
            self.pending = None;
        }
        tracing::error!(
            "advance_ai 单帧内达到 {} 步仍未轮到玩家，提前放弃——多半是某个 AI 卡在原地反复无效行动，见本方法文档「必须保证进展」",
            MAX_STEPS_PER_ADVANCE
        );
        acted
    }

    /// 尝试用本帧输入结算玩家的一次行动。没有等到玩家回合、或本帧没有
    /// 任何方向/等待键激活时，不消费这次回合，返回假。
    pub(crate) fn try_player_turn(
        &mut self,
        world: &mut WorldState,
        player: EntityId,
        all: &[Combatant],
        input: &InputState,
        popups: &mut Vec<DamagePopup>,
    ) -> bool {
        let Some(entry) = self.pending.filter(|entry| entry.actor == player) else {
            return false;
        };
        let Some(raw) = intent_from_input(player, input) else {
            return false;
        };
        let intent = route_player_intent(world, player, raw, all);
        self.pending = None;
        self.perform(world, entry, intent, popups);
        true
    }

    /// 预览接下来 `count` 条待行动记录（含当前 `pending`），供时间轴
    /// 侧栏显示——只读不弹出：克隆一份时间轴在克隆体上弹出，不触碰
    /// 真正驱动结算的那一份。[`Timeline`] 派生了 `Clone`
    /// （见其类型定义），这里不需要 `ll-sim` 额外开放新接口。
    pub(crate) fn upcoming(&self, count: usize) -> Vec<TimelineEntry> {
        let mut preview = Vec::with_capacity(count);
        if let Some(entry) = self.pending {
            preview.push(entry);
        }
        let mut probe = self.timeline.clone();
        while preview.len() < count {
            match probe.pop_next() {
                Some(entry) => preview.push(entry),
                None => break,
            }
        }
        preview
    }
}

/// 把玩家的原始输入意图路由成最终意图：若一次 [`Intent::Move`] 的目的地
/// 站着别的存活单位，改判为 [`Intent::Attack`]（撞人即攻击，传统
/// roguelike 手感）；否则原样放行。
///
/// `ll_sim::resolve` 刻意不做这个派生（见其模块文档），因为「同一格
/// 多个实体时打谁」这类规则需要调用方按自己的场景决定——本 demo 里
/// 每格至多站一个单位，规则退化成「有就是它」，没有歧义可言。
fn route_player_intent(
    world: &WorldState,
    player: EntityId,
    raw: Intent,
    all: &[Combatant],
) -> Intent {
    let Intent::Move { actor, dir } = raw else {
        return raw;
    };
    let Some(agent) = world.actors.get(actor) else {
        return raw;
    };
    let (dx, dy) = dir.delta();
    let dest = world.size.wrap(agent.pos.x() + dx, agent.pos.y() + dy);
    match find_actor_at(world, all, dest, player) {
        Some(target) => Intent::Attack {
            actor: player,
            target,
        },
        None => raw,
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
/// [`TurnEngine::advance_ai`] 反复弹出——这不是假设：接了真实生成的
/// 地形后，这里就是那次死循环的根因。相邻格不可通行时改为原地等待
/// （[`ll_sim::resolve::resolve`] 对 `Intent::Wait` 恒产出
/// `ScheduleNext`，见其实现），保证时间只会前进不会卡住；等待一回合
/// 也不算错误的 AI 手感，比「贴着水面反复无效地往同一个方向撞」更
/// 合理。
fn ai_intent(world: &WorldState, actor: EntityId, player: EntityId) -> Intent {
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

/// 在 `all` 里找出（若有）站在 `pos` 的那个存活单位，`exclude` 自身
/// 除外。`all` 是 [`Vec`]，遍历顺序恒为出生顺序——不经过任何
/// `HashMap`/`HashSet`，满足约束 C3。
fn find_actor_at(
    world: &WorldState,
    all: &[Combatant],
    pos: TorusPos,
    exclude: EntityId,
) -> Option<EntityId> {
    all.iter()
        .map(|combatant| combatant.id)
        .filter(|&id| id != exclude)
        .find(|&id| world.actors.get(id).is_some_and(|agent| agent.pos == pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_platform::input::GameKey;
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
            luck: 0,
            current_space: ll_world::space::Space::surface(
                zone,
                ll_core::ident::ContentIndex::default(),
            ),
            script_state: std::collections::BTreeMap::new(),
        })
    }

    fn combatant(id: EntityId) -> Combatant {
        Combatant {
            id,
            sprite: "hero_idle_0",
            tint: [1.0; 4],
        }
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

    #[test]
    fn 移动到敌人所在格被路由成攻击() {
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let enemy = spawn_at(&mut world, (6, 5), 10);
        let all = vec![combatant(player), combatant(enemy)];
        let raw = Intent::Move {
            actor: player,
            dir: Direction::East,
        };

        // Act
        let routed = route_player_intent(&world, player, raw, &all);

        // Assert
        assert!(matches!(
            routed,
            Intent::Attack { target, .. } if target == enemy
        ));
    }

    #[test]
    fn 移动到空地不被路由成攻击() {
        // Arrange
        let (mut world, terrain_ids) = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        world
            .terrain
            .set_terrain(world.size.wrap(6, 5), terrain_ids.grass);
        let all = vec![combatant(player)];
        let raw = Intent::Move {
            actor: player,
            dir: Direction::East,
        };

        // Act
        let routed = route_player_intent(&world, player, raw, &all);

        // Assert
        assert!(matches!(routed, Intent::Move { .. }));
    }

    #[test]
    fn 敏捷更高的敌人在同一段时间窗口内被结算得更多次() {
        // 这是「快角色在慢角色一次行动窗口内行动多次」这条核心验收点
        // 的自动化回归——图形环境不可用时，这条测试就是替代证据。
        // Arrange：一快一慢两个敌人都离玩家很远，纯移动不触发攻击。
        let (mut world, _terrain_ids) = test_world();
        let player = spawn_at(&mut world, (0, 0), 10);
        let fast = spawn_at(&mut world, (40, 0), 30);
        let slow = spawn_at(&mut world, (0, 40), 5);
        let mut timeline = Timeline::new();
        timeline.schedule(player, Tick(0));
        timeline.schedule(fast, Tick(0));
        timeline.schedule(slow, Tick(0));
        let all = vec![combatant(player), combatant(fast), combatant(slow)];
        let mut engine = TurnEngine::new(timeline);
        let mut popups = Vec::new();

        // Act：让玩家连续等待三次，驱动整条时间轴向前推进，累计
        // advance_ai 每次返回的行动者列表。
        let mut acted_log = Vec::new();
        for _ in 0..3 {
            acted_log.extend(engine.advance_ai(&mut world, player, &mut popups));
            assert_eq!(
                engine.upcoming(1).first().map(|entry| entry.actor),
                Some(player)
            );
            let mut input = InputState::new();
            input.press(GameKey::Wait);
            let acted = engine.try_player_turn(&mut world, player, &all, &input, &mut popups);
            assert!(acted, "本用例中每一轮都应能成功结算一次玩家等待");
        }

        // Assert：同一段窗口内，敏捷 30 的一方被结算的次数应严格多于
        // 敏捷 5 的一方——这正是时间轴调度器要验证的核心手感。
        let fast_count = acted_log.iter().filter(|&&id| id == fast).count();
        let slow_count = acted_log.iter().filter(|&&id| id == slow).count();
        assert!(fast_count > slow_count);
    }

    #[test]
    fn 死亡实体不再出现在后续的时间轴预览中() {
        // Arrange：玩家攻击力设得极高，一击必杀相邻敌人。
        let (mut world, _terrain_ids) = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        world
            .actors
            .get_mut(player)
            .expect("刚生成的实体必然存在")
            .stats
            .strength = 9999;
        let victim = spawn_at(&mut world, (6, 5), 10);
        let mut timeline = Timeline::new();
        timeline.schedule(player, Tick(0));
        timeline.schedule(victim, Tick(100));
        let all = vec![combatant(player), combatant(victim)];
        let mut engine = TurnEngine::new(timeline);
        let mut popups = Vec::new();
        engine.advance_ai(&mut world, player, &mut popups);
        assert_eq!(
            engine.upcoming(1).first().map(|entry| entry.actor),
            Some(player)
        );

        let mut input = InputState::new();
        input.press(GameKey::Right);

        // Act
        let acted = engine.try_player_turn(&mut world, player, &all, &input, &mut popups);

        // Assert
        assert!(acted);
        assert!(world.actors.get(victim).is_none(), "受击方应已死亡");
        assert!(
            !engine
                .upcoming(16)
                .iter()
                .any(|entry| entry.actor == victim),
            "死者不应残留在时间轴预览里"
        );
        assert!(!popups.is_empty(), "本用例应产生至少一条伤害飘字");
    }

    #[test]
    fn 玩家在advance_ai内被杀死后立即返回不耗尽单帧步数上限() {
        // 这是「玩家死亡后主循环空转到 MAX_STEPS_PER_ADVANCE」这一真实
        // 缺陷的直接回归：敌人甲与玩家相邻且攻击力极高，甲的回合排在
        // 最前，会在 advance_ai 循环进行到一半时把玩家杀死——此时玩家
        // 已被移出时间轴，若函数只在弹出玩家自己的条目时才返回，就再
        // 也等不到那个条件成立，会转而对旁观者乙反复空转直到耗尽上限。
        // Arrange
        let (mut world, _terrain_ids) = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let killer = spawn_at(&mut world, (6, 5), 10);
        world
            .actors
            .get_mut(killer)
            .expect("刚生成的实体必然存在")
            .stats
            .strength = 9999;
        let bystander = spawn_at(&mut world, (40, 40), 10);
        let mut timeline = Timeline::new();
        timeline.schedule(killer, Tick(0));
        timeline.schedule(player, Tick(50));
        timeline.schedule(bystander, Tick(100));
        let mut engine = TurnEngine::new(timeline);
        let mut popups = Vec::new();

        // Act
        let acted = engine.advance_ai(&mut world, player, &mut popups);

        // Assert：玩家应已被击杀，且结算过的实体数应远小于
        // MAX_STEPS_PER_ADVANCE（10000）——只有杀死玩家的那一击算数，
        // 不应该在旁观者乙身上反复空转到耗尽上限才放弃。
        assert!(world.actors.get(player).is_none(), "玩家应已被击杀");
        assert!(
            acted.len() < 10,
            "玩家死亡后 advance_ai 应立即返回，实际结算了 {} 次",
            acted.len()
        );
    }
}
