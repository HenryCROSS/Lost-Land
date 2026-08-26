//! 生产用回合引擎：把 [`Timeline`] 的弹出顺序、`resolve`/`apply` 的调用、
//! 死亡实体的时间轴清理粘合成「谁在什么世界时刻做了什么」这条唯一的
//! 世界时钟推进路径。
//!
//! # 为什么这段逻辑必须挪进 `ll-sim`，不能只留在 p3 验收 demo 里
//!
//! `crates/ll-sim/examples/p3_acceptance/turn.rs` 最早实现了这套引擎，
//! 但那是 `[[example]]`——`ll-game`（本体二进制）与它是同级的两个下游
//! crate，互不可见彼此的 `examples/` 代码。结果是本体二进制的
//! `ll_game::app::Demo::advance` 从未接上时间轴：每帧直接
//! `intent_from_input` → `resolve` → `apply`，`world.clock` 只在
//! `ll_game::world::build_new_world` 建局那一刻被赋值一次，此后永不
//! 推进——真实游玩时,昼夜循环、buff 到期、技能冷却、地面物品老化、
//! NPC 调度全部靠这个会走的时钟,而它从未走过。
//!
//! 本模块把「弹出 → 设时钟 → resolve → apply → 清理死者 → 重新排期」
//! 这条与具体游戏是 p3 demo 还是本体二进制完全无关的核心机制搬到
//! `ll-sim`（两者共同的上游），`p3_acceptance` 与 `ll-game` 各自只保留
//! 自己独有的策略：p3 的固定敌人 AI（`p3_acceptance::turn::ai_intent`，
//! 仍留在该 demo 里——行为树属 P7，这条策略本身不是「通用逻辑」，是
//! demo 自己验证时间轴用的占位策略）与伤害飘字（同样是纯呈现层状态，
//! 不属于「回合与模拟层」，见 crate 顶层文档），本体二进制则暂时没有
//! 敌人可言，传一个恒 `Wait` 的占位策略即可。
//!
//! # 与本仓库「同一份逻辑存在两份，迟早会漂」的教训一致
//!
//! `sprite_draw_position` 被手抄进多个验收 demo、P5 忘了写，导致精灵
//! 下凸；`base_hero_clips` 的六帧数据也险些在一次合并里丢失——两次
//! 教训都是「共用逻辑没有一个共同的家」。这次不再重演：`TurnEngine`
//! 只有这一份实现，`p3_acceptance` 与 `ll-game` 都调用它,不是各自抄
//! 一份。

use ll_core::time::Tick;
use ll_platform::input::InputState;
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

use crate::apply::apply_with_xp_curves;
use crate::catalogs::ResolveCatalogs;
use crate::effect::Effect;
use crate::intent::{Intent, intent_from_input};
use crate::resolve::resolve_with_catalogs;
use crate::timeline::{Timeline, TimelineEntry};

/// [`TurnEngine::advance_ai`] 单次调用最多结算的非受控实体回合数，超过
/// 就放弃本次推进——防止「某次行动不产生 `Effect::ScheduleNext`」这类
/// 未预见的缺陷冻结整条推进路径。
///
/// 取值远大于当前任何真实场景会用到的实体数：`p3_acceptance` 固定 3
/// 个敌人,本体二进制目前甚至没有 NPC——正常运行中不会触发,只有真的
/// 出现死循环级缺陷时才会被这道防线截住。
const MAX_STEPS_PER_ADVANCE: u32 = 10_000;

/// 回合引擎：包一层 [`Timeline`]，额外持有「已经弹出、但还没配上
/// `Intent`」的那一条待行动记录。
///
/// # 为什么需要 `pending`
///
/// `Timeline::pop_next` 一旦调用就从队列里永久移除那条记录。轮到受控
/// 实体（通常是玩家）行动时，本帧的输入可能还没到（没按任何方向/等待
/// 键）——若这时已经 `pop_next` 却又无法立即结算，这条记录就会丢失，
/// 玩家会凭空跳过一次行动。`pending` 就是「已弹出、等待被消费」的缓冲
/// 区：非受控实体的回合立刻消费掉，受控实体则等到
/// [`TurnEngine::try_player_turn`] 拿到一个非空 `Intent` 才消费。
pub struct TurnEngine {
    timeline: Timeline,
    pending: Option<TimelineEntry>,
}

impl TurnEngine {
    /// 用一个已经排好初始行动的时间轴建立回合引擎。
    ///
    /// 时间轴通常由 [`crate::timeline::Timeline::schedule`] 逐个实体
    /// 排入，或者（存档读回场景）按每个存活实体已持久化的
    /// `Agent::next_action_at` 重建——后者不需要时间轴本身参与
    /// 序列化，因为这个字段已经是权威来源，见调用方（`ll-game`）
    /// 文档「为什么时间轴不进存档」一节。
    pub fn new(timeline: Timeline) -> TurnEngine {
        TurnEngine {
            timeline,
            pending: None,
        }
    }

    /// 把一个**新出现的**实体排进时间轴。
    ///
    /// # 为什么需要它：`rebuild_timeline` 在这里用不了
    ///
    /// `ll-game` 建局与读档时都是「按全部存活实体的
    /// `Agent::next_action_at` 整条重建时间轴」（见其
    /// `rebuild_timeline` 文档）。但 NPC 生成批次之后，实体会在**游玩
    /// 途中**出现（玩家走近一座据点，那座据点的 NPC 被物化）——那一刻
    /// 整条重建会丢掉 [`Self::pending`]：一条「已弹出、等待被消费」的
    /// 记录若被重建覆盖，玩家会凭空跳过一次行动，正是 `pending` 这个
    /// 字段本身要防的那个缺陷（见本类型文档「为什么需要 `pending`」）。
    ///
    /// 因此新实体走这条增量入口，不重建。调用方负责传一个不早于当前
    /// 世界时钟的 `at`（物化路径传的就是 `world.clock`），否则这个实体
    /// 会在下一次 `advance_ai` 里把时钟往回拨。
    pub fn schedule(&mut self, actor: EntityId, at: Tick) {
        self.timeline.schedule(actor, at);
    }

    /// 结算并应用一个实体的一次行动：设定世界时钟为该实体计划行动的
    /// 时刻，跑 `resolve` → 逐个 `apply`，实体死亡则从时间轴移除其
    /// 残留记录，存活则重新排入下一次行动。
    ///
    /// `catalogs` 原样转交给 [`crate::resolve::resolve_with_catalogs`]，
    /// 本引擎自己一份都不读——它是「谁在什么时刻行动」的调度者，不是
    /// 结算规则的实现者。这个参数**必须存在**：本方法此前调的是不带
    /// 任何目录的 `resolve`，于是种族/职业天赋、抗性、偷袭规则、资源池
    /// 容量——全部走 `effective_traits` 的东西——在真正能跑的游戏里
    /// 从未生效过（`ll-game` 全程只经本引擎驱动世界，从不直接调
    /// `resolve_with_*` 系列），天赋系统当时全部的「真实证据」都止步于
    /// 集成测试直接调 `resolve_with_*`。与本模块文档开头记的
    /// 「`TurnEngine` 本身当初只在 demo 里接了线」是同一类缺陷的第二次
    /// 复发，修法同样是「让生产路径走上那条真实链路」，不是再补一份
    /// 只在测试里成立的证据。
    ///
    /// 没有任何内容表的调用方（`examples/` 里自己合成世界的验收 demo）
    /// 传 [`ResolveCatalogs::empty`]，与接线之前逐字等价。
    ///
    /// `on_effect` 在每条效果被 `apply` 之前调用一次——这是本引擎与
    /// 「这条效果在呈现层意味着什么」（例如要不要弹一条伤害飘字）
    /// 之间唯一的接缝：`ll-sim` 是「回合与模拟层」（见 crate 顶层
    /// 文档），不知道、也不需要知道调用方是否在渲染、渲染成什么样子。
    /// 必须在 `apply` **之前**调用：若这批效果里紧接着一个
    /// `Effect::Kill`（见 [`crate::resolve::resolve`] 的
    /// `resolve_attack`），`apply` 之后该实体已从世界里销毁，`on_effect`
    /// 若想读它此刻的位置就再也读不到了。
    ///
    /// # `on_effect` 是纯观察者，返回 `()`
    ///
    /// 它曾经返回 `Vec<Effect>`（**反应效果**），那是给 mod 脚本事件
    /// 监听器用的：一个只能看不能动的监听器用处有限。脚本事件监听
    /// 拆除之后那条通道的唯一生产者消失了（呈现层与测试一直返回空
    /// `Vec`），签名因此收回成 `()`——留着一个恒为空的返回值，就是这个
    /// 代码库反复踩过的「声明了但从没接线」。
    ///
    /// 收回**不改变任何可观察行为**：反应列表恒为空，那两个循环（攒
    /// 反应、事后 apply 反应）跑的都是零次。
    ///
    /// 引擎自身真要「一条效果触发另一条」时，正确的落点是
    /// [`crate::resolve`]（结算期就把整批效果算全，例如
    /// `rest_completion_effects`），不是让宿主回调往里塞——那样效果
    /// 序列会取决于宿主是谁。
    fn perform(
        &mut self,
        world: &mut WorldState,
        entry: TimelineEntry,
        intent: Intent,
        catalogs: &ResolveCatalogs<'_>,
        on_effect: &mut dyn FnMut(&WorldState, &Effect),
    ) {
        // 世界时钟推进——曾经在这里加过一条「只在变化时打一行」的
        // `tracing::info!` 调试日志（提交 `0ff2c9e`），因为项目所有者
        // 当时无法确认时钟到底走没走（ADR 0025 禁止合成按键，测试证明
        // 过推进但玩家本人无法实机验证）。P7 第一批（只读观测 HUD）
        // 已经把 `world.clock` 接进状态栏常驻显示（见
        // `ll_ui::hud::status_bar`），时间对所有者可见现在靠界面，不再
        // 需要靠日志刷屏确认时钟活着，该日志已按代码注释原文「P7 时间
        // UI 落地后应摘除」移除，不留任何调试期专用分支。
        world.clock = entry.at;
        let effects = resolve_with_catalogs(world, &intent, catalogs);
        for effect in &effects {
            on_effect(world, effect);
            // `apply_with_xp_curves`，不是薄封装 `apply`：后者恒用
            // `FlatXpCurve::DEFAULT` 那条保底曲线，于是
            // `register-xp-curve`/`register-class-xp-curve`/
            // `register-race-xp-curve` 三个已经落地的注册函数在真正
            // 能跑的游戏里从来不会被读到（`ll-game` 全程只经本引擎
            // 驱动世界）——与本批次同时修掉的「击杀经验只在测试里
            // 成立」是同一类缺陷。曲线目录随
            // `crate::catalogs::ResolveCatalogs` 一起搬进来，见该字段
            // 文档「为什么一个 `apply` 侧的目录也在这一束里」。
            apply_with_xp_curves(world, effect, catalogs.xp_curves);
            if let Effect::Kill { target, .. } = effect {
                // Timeline 与 WorldState 是两个独立的存储（见
                // `crate::timeline` 模块文档），apply 只知道
                // WorldState，清理时间轴里残留的死者行动记录是调用方
                // （这里）的职责。
                self.timeline.remove(*target);
            }
        }
        if let Some(agent) = world.actors.get(entry.actor) {
            self.timeline.schedule(entry.actor, agent.next_action_at);
        }
    }

    /// 反复弹出并结算「非受控」实体（通常是 AI）的行动，直到轮到受控
    /// 实体（通常是玩家）或队列耗尽。
    ///
    /// `ai_intent` 是「这个实体这一回合该做什么」的决策来源——本引擎
    /// 刻意不内置任何具体策略：固定策略、行为树或干脆恒 `Wait`（本体
    /// 二进制目前没有 NPC 时的占位）都由调用方决定，见模块文档「p3 的
    /// 固定敌人 AI……仍留在该 demo 里」一节。
    ///
    /// # 为什么从 `fn` 指针放宽成 `&mut dyn FnMut`
    ///
    /// 这个参数原本是普通函数指针，理由记录得很明确：「当前两个调用方
    /// （`p3_acceptance`、`ll-game`）都不需要按调用方状态捕获环境，
    /// 函数指针已经足够，不需要为假设中的未来需求引入更重的
    /// `dyn FnMut`（YAGNI）」。那条理由在真实的行为树决策来源出现之后
    /// **失效了**，而且不是「假设中的未来需求」：
    /// [`crate::behavior::BehaviorTreeSource::decide`] 需要 `&mut self`
    /// （求值 Steel VM 本身要独占访问，见 [`crate::behavior`] 模块文档
    /// 「为什么 `decide` 接收 `&mut dyn BehaviorTreeSource`」一节），
    /// 一个 `fn` 指针**物理上捕获不进任何决策来源**——于是
    /// `ll_mod::script_behavior_source::ScriptBehaviorSource` 落地之后，
    /// 唯一能喂给本函数的东西仍然只有无状态的自由函数，行为树因此
    /// 一次都没有经由本函数跑过。这是「接线断在最后一环」在本仓库的
    /// 第三次同形复发（前两次：`TurnEngine` 本身只接进 demo；内容目录
    /// 没接进 `TurnEngine`，天赋在真实游戏里从未生效，`fff73d8` 修）。
    ///
    /// 放宽成 `&mut dyn FnMut` 而不是泛型 `impl FnMut`：与紧邻的
    /// `on_effect: &mut dyn FnMut(&WorldState, &Effect)` 同一种形状，
    /// 本函数不会因为多一个决策来源就多单态化一份；既有的函数指针
    /// 调用方只需要在实参前加一个 `&mut`（`fn` 项本身是实现了 `FnMut`
    /// 的零尺寸类型）。把决策来源包成这个闭包的标准写法见
    /// [`crate::behavior::behavior_ai_intent`]。
    ///
    /// `catalogs`/`on_effect` 见 [`Self::perform`] 文档。
    ///
    /// 返回按结算顺序排列的行动者列表——调用方据此就能数出「这段窗口
    /// 里谁被结算了几次」，不必自己重新实现一遍时间轴推进逻辑。
    ///
    /// # 必须保证进展（曾经的真实死循环）
    ///
    /// 若某次改动让 `ai_intent` 产出一个 `resolve` 会判定为空效果的
    /// `Intent`（例如撞墙的 `Move`），[`Self::perform`] 就不会产出
    /// `Effect::ScheduleNext`，该实体的 `next_action_at` 原地不动，
    /// 重新排入时间轴后会在**同一个 tick** 被立刻弹出，陷入死循环——
    /// 这不是假设的风险：`p3_acceptance` 的固定策略 AI 曾经因为快速
    /// 敌人朝玩家方向的下一格恰好是深水而真实卡死过，单元测试跑了一
    /// 分钟没结束才被发现（该 demo 已经在 `ai_intent` 自己那一层修好
    /// 根因）。[`MAX_STEPS_PER_ADVANCE`] 是修好根因之外的第二道防线：
    /// 即使某次改动又引入了同一类缺陷，单次最多空转这么多步就会放弃，
    /// 把已经死循环的那个实体的 `pending` 状态原样交还给下一次调用，
    /// 而不是冻结整条推进路径。
    ///
    /// # 受控实体死亡必须在循环内部逐次核查，不能只在入口查一次
    ///
    /// 受控实体不是本函数唯一处理的实体——本函数每一步都可能结算一个
    /// 存活敌人对受控实体的攻击，受控实体因此完全可能在循环**进行到
    /// 一半**时死亡（[`Self::perform`] 处理 `Effect::Kill` 时会把死者
    /// 从 [`Timeline`] 里移除，见其实现）。受控实体一旦被移出时间轴，
    /// 「弹出的条目属于受控实体」这条唯一的提前返回条件就再也不会
    /// 成立：若这里只在循环开始前查一次是否存活，死后循环会继续反复
    /// 结算其余存活实体，直到耗尽 [`MAX_STEPS_PER_ADVANCE`] 才放弃。
    /// 修法是把「受控实体是否还在 `world.actors` 里」的核查放进循环体
    /// 本身、每一步都问一次，一死就能在下一步立即察觉并返回。
    pub fn advance_ai(
        &mut self,
        world: &mut WorldState,
        controlled: EntityId,
        ai_intent: &mut dyn FnMut(&WorldState, EntityId, EntityId) -> Intent,
        catalogs: &ResolveCatalogs<'_>,
        on_effect: &mut dyn FnMut(&WorldState, &Effect),
    ) -> Vec<EntityId> {
        let mut acted = Vec::new();
        for _ in 0..MAX_STEPS_PER_ADVANCE {
            if world.actors.get(controlled).is_none() {
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
            if entry.actor == controlled {
                return acted;
            }
            let intent = ai_intent(world, entry.actor, controlled);
            self.perform(world, entry, intent, catalogs, on_effect);
            acted.push(entry.actor);
            self.pending = None;
        }
        tracing::error!(
            "advance_ai 单次调用内达到 {} 步仍未轮到受控实体，提前放弃——多半是某个 AI 卡在原地反复无效行动",
            MAX_STEPS_PER_ADVANCE
        );
        acted
    }

    /// 尝试用一次输入结算受控实体（通常是玩家）的一次行动。没有等到
    /// 它的回合、或本帧没有任何方向/等待键激活时，不消费这次回合，
    /// 返回假。
    ///
    /// 撞进另一个存活实体所在格的 `Move` 会被就地路由成 `Attack`（撞人
    /// 即攻击，传统 roguelike 手感,见 [`route_move_to_attack`]）；
    /// `resolve` 刻意不做这个派生（见其模块文档），因为「同一格多个
    /// 实体时打谁」这类规则需要调用方按自己的场景决定。
    ///
    /// `catalogs`/`on_effect` 见 [`Self::perform`] 文档——玩家这条路径
    /// 与 AI 那条走的是同一个 `perform`，天赋对玩家自己的攻击/技能/
    /// 休息同样生效，不存在「只有 NPC 吃规则」的不对称。
    pub fn try_player_turn(
        &mut self,
        world: &mut WorldState,
        player: EntityId,
        input: &InputState,
        catalogs: &ResolveCatalogs<'_>,
        on_effect: &mut dyn FnMut(&WorldState, &Effect),
    ) -> bool {
        let Some(entry) = self.pending.filter(|entry| entry.actor == player) else {
            return false;
        };
        let Some(raw) = intent_from_input(player, input) else {
            return false;
        };
        let intent = route_move_to_attack(world, raw);
        self.pending = None;
        self.perform(world, entry, intent, catalogs, on_effect);
        true
    }

    /// 预览接下来 `count` 条待行动记录（含当前 `pending`）——只读不
    /// 弹出：克隆一份时间轴在克隆体上弹出，不触碰真正驱动结算的那一
    /// 份。[`Timeline`] 派生了 `Clone`（见其类型定义），这里不需要
    /// `ll-sim` 额外开放新接口。
    pub fn upcoming(&self, count: usize) -> Vec<TimelineEntry> {
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

/// 把一个原始意图路由成最终意图：若一次 [`Intent::Move`] 的目的地站着
/// 别的存活实体，改判为 [`Intent::Attack`]；否则原样放行。
///
/// 直接在 `world.actors` 上查找目标格——不需要调用方另外维护一份
/// 「全部实体」的列表（`p3_acceptance` 曾经为此单独传一个
/// `&[Combatant]` 参数，纯属多余：[`ll_world::entity::Arena::iter_with_id`]
/// 已经能给出同样的信息，且同样不依赖任何哈希容器迭代顺序,满足约束
/// C5）。当前世界规则里每格至多站一个单位，「有就是它」不存在歧义。
fn route_move_to_attack(world: &WorldState, raw: Intent) -> Intent {
    let Intent::Move { actor, dir } = raw else {
        return raw;
    };
    let Some(agent) = world.actors.get(actor) else {
        return raw;
    };
    let (dx, dy) = dir.delta();
    let dest = world.size.wrap(agent.pos.x() + dx, agent.pos.y() + dy);
    let target = world
        .actors
        .iter_with_id()
        .find(|(id, other)| *id != actor && other.pos == dest)
        .map(|(id, _)| id);
    match target {
        Some(target) => Intent::Attack { actor, target },
        None => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_core::time::{TICKS_PER_MINUTE, Tick};
    use ll_core::torus::TorusSize;
    use ll_platform::input::GameKey;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    /// 本模块全部用例共用的空目录束——它们验收的是「谁在什么时刻被
    /// 结算」这条调度逻辑本身，与任何内容表无关（世界也是合成的，没有
    /// 装载过 `mods/`）。真实目录经由本引擎影响结算结果的端到端验收在
    /// `crates/ll-mod/tests/turn_engine_catalogs.rs`——那里才有真实
    /// mod 脚本注册的天赋可查。
    const EMPTY: ResolveCatalogs<'static> = ResolveCatalogs::empty();

    /// 测试用世界：与 `crate::resolve` 测试模块同一套构造（边长 64、
    /// 单区块，见其文档「测试用区块布局」一节），本模块只关心时间轴
    /// 推进本身,不关心地形种类,不需要另外持有 `BaseTerrainIds`。
    fn test_world() -> WorldState {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
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

    fn spawn_at(world: &mut WorldState, pos: (i32, i32), dexterity: i32) -> EntityId {
        let mut interner = Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        let agent_pos = world.size.wrap(pos.0, pos.1);
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
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(
                world.terrain.layout().tile_to_zone(agent_pos).0,
                ll_core::ident::ContentIndex::default(),
            ),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        })
    }

    fn no_op_ai(world: &WorldState, actor: EntityId, _controlled: EntityId) -> Intent {
        let _ = world;
        Intent::Wait { actor }
    }

    #[test]
    fn 单个受控实体等待一次后世界时钟前进到其排定的时刻() {
        // Arrange：只有一个受控实体，时间轴里只有它自己这一条记录。
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let mut timeline = Timeline::new();
        timeline.schedule(player, Tick(100));
        let mut engine = TurnEngine::new(timeline);
        let clock_before = world.clock;
        let mut input = InputState::new();
        input.press(GameKey::Wait);

        // Act
        engine.advance_ai(&mut world, player, &mut no_op_ai, &EMPTY, &mut |_, _| {});
        let acted = engine.try_player_turn(&mut world, player, &input, &EMPTY, &mut |_, _| {});

        // Assert
        assert!(acted, "本用例应能成功结算一次受控实体的等待");
        assert_eq!(world.clock, Tick(100));
        assert_ne!(world.clock, clock_before);
    }

    #[test]
    fn 连续三次等待后世界时钟严格递增() {
        // 直接回应本任务的核心缺陷：不是「时钟走了一次」，是每次受控
        // 实体行动都真的往前走，不是原地不动或倒退。
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let mut timeline = Timeline::new();
        timeline.schedule(player, Tick(0));
        let mut engine = TurnEngine::new(timeline);
        let mut input = InputState::new();
        input.press(GameKey::Wait);

        // Act：记录每次结算后的时钟读数。
        let mut clocks = Vec::new();
        for _ in 0..3 {
            engine.advance_ai(&mut world, player, &mut no_op_ai, &EMPTY, &mut |_, _| {});
            engine.try_player_turn(&mut world, player, &input, &EMPTY, &mut |_, _| {});
            clocks.push(world.clock);
        }

        // Assert：严格递增，不允许原地不动或倒退。
        assert!(clocks[0] < clocks[1]);
        assert!(clocks[1] < clocks[2]);
    }

    #[test]
    fn 六次基准敏捷等待后世界时钟前进恰好一分钟() {
        // 直接量化流速本身，不只是「时钟在走」：基准敏捷（10）下
        // 一次等待的耗时是 `action_cost(BASE_ACTION_COST=100,
        // effective_speed=1000) == 100` 刻度（见 `resolve::resolve_wait`
        // /`timeline::action_cost`，本测试不重复该公式，只依赖其恒定
        // 产出 100 这一事实，由旁边「连续三次等待后世界时钟严格递增」
        // 同一套 `spawn_at(.., 10)` 基准敏捷保证），与
        // `TICKS_PER_MINUTE` 完全无关——六次即固定 600 刻度，写成绝对
        // 期望值，不与被测常量共用同一个表达式，防止「常量改了、断言
        // 跟着自动改了因而测不出问题」这种自欺。
        //
        // 断言分两层：第一层锁定原始刻度数（验证流速常量的改动没有
        // 意外牵动行动耗时本身，见 `TICKS_PER_MINUTE` 文档「完全不
        // 出现在 action_cost 的公式里」一节）；第二层用
        // `TICKS_PER_MINUTE` 把刻度换算成分钟——这一层如果把本次改动
        // （60 → 600）还原回旧值，600 刻度就会变成 10 分钟而不是 1
        // 分钟，断言会变红，真正验证了「流速确实变了」而不只是「时钟
        // 在走」。
        //
        // 初始排期用 `Tick(100)` 而非 `Tick(0)`：`TurnEngine::perform`
        // 把 `world.clock` 设成本次弹出的条目自身的 `at`（见其实现），
        // 这个 `at` 是**上一次**行动结束时算出的排期，不是本次行动
        // 结束后的时刻——若从 `Tick(0)` 起排，六次循环弹出的 `at` 依次
        // 是 0/100/200/300/400/500，最后一次弹出仍是「上一次」的排期，
        // 时钟停在 500 而非 600（这是 `TurnEngine` 消费队列的既有
        // 行为，不是本测试要覆盖的缺陷，`单个受控实体等待一次后……`
        // 那条既有测试同样从非零的 `Tick(100)` 起排,原因一致）。从
        // `Tick(100)` 起排,六次循环弹出的 `at` 变成
        // 100/200/300/400/500/600,时钟最终落在六次行动应得的 600,
        // 测的是「六次行动共花掉多少刻度」这件事本身,不掺进队列消费
        // 时机的位移。
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let mut timeline = Timeline::new();
        timeline.schedule(player, Tick(100));
        let mut engine = TurnEngine::new(timeline);
        let mut input = InputState::new();
        input.press(GameKey::Wait);

        // Act：连续结算六次基准敏捷的等待。
        for _ in 0..6 {
            engine.advance_ai(&mut world, player, &mut no_op_ai, &EMPTY, &mut |_, _| {});
            engine.try_player_turn(&mut world, player, &input, &EMPTY, &mut |_, _| {});
        }

        // Assert
        assert_eq!(world.clock, Tick(600));
        assert_eq!(world.clock.0 / TICKS_PER_MINUTE, 1);
    }

    #[test]
    fn 敏捷更高的实体在同一段时间窗口内被结算得更多次() {
        // 与 p3_acceptance 迁移前同名测试等价——移动到 ll-sim 后仍需要
        // 覆盖「非受控实体在受控实体一次行动窗口内可以行动多次」这条
        // 时间轴调度器的核心手感,证明搬迁没有改变行为。
        // Arrange：一快一慢两个非受控实体都离受控实体很远，纯移动不
        // 触发攻击。
        let mut world = test_world();
        let player = spawn_at(&mut world, (0, 0), 10);
        let fast = spawn_at(&mut world, (40, 0), 30);
        let slow = spawn_at(&mut world, (0, 40), 5);
        let mut timeline = Timeline::new();
        timeline.schedule(player, Tick(0));
        timeline.schedule(fast, Tick(0));
        timeline.schedule(slow, Tick(0));
        let mut engine = TurnEngine::new(timeline);
        let mut input = InputState::new();
        input.press(GameKey::Wait);

        fn move_toward_origin(
            world: &WorldState,
            actor: EntityId,
            _controlled: EntityId,
        ) -> Intent {
            use crate::intent::Direction;
            let agent = world.actors.get(actor).expect("测试里始终存活");
            let dir = if agent.pos.x() > 0 {
                Direction::West
            } else {
                Direction::North
            };
            Intent::Move { actor, dir }
        }

        // Act：让受控实体连续等待三次，驱动整条时间轴向前推进。
        let mut acted_log = Vec::new();
        for _ in 0..3 {
            acted_log.extend(engine.advance_ai(
                &mut world,
                player,
                &mut move_toward_origin,
                &EMPTY,
                &mut |_, _| {},
            ));
            let acted = engine.try_player_turn(&mut world, player, &input, &EMPTY, &mut |_, _| {});
            assert!(acted, "本用例中每一轮都应能成功结算一次受控实体等待");
        }

        // Assert：同一段窗口内，敏捷 30 的一方被结算的次数应严格多于
        // 敏捷 5 的一方。
        let fast_count = acted_log.iter().filter(|&&id| id == fast).count();
        let slow_count = acted_log.iter().filter(|&&id| id == slow).count();
        assert!(fast_count > slow_count);
    }

    #[test]
    fn 死亡实体不再出现在后续的时间轴预览中() {
        // Arrange：受控实体攻击力设得极高，一击必杀相邻实体。
        let mut world = test_world();
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
        let mut engine = TurnEngine::new(timeline);
        engine.advance_ai(&mut world, player, &mut no_op_ai, &EMPTY, &mut |_, _| {});
        let mut input = InputState::new();
        input.press(GameKey::Right);

        // Act
        let acted = engine.try_player_turn(&mut world, player, &input, &EMPTY, &mut |_, _| {});

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
    }

    #[test]
    fn 受控实体在advance_ai内被杀死后立即返回不耗尽单次步数上限() {
        // 与 p3_acceptance 迁移前同名测试等价：敌人甲与受控实体相邻且
        // 攻击力极高，甲的回合排在最前，会在 advance_ai 循环进行到一半
        // 时把受控实体杀死——此时受控实体已被移出时间轴，若函数只在
        // 弹出它自己的条目时才返回，就再也等不到那个条件成立。
        // Arrange
        let mut world = test_world();
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

        fn attack_player(world: &WorldState, actor: EntityId, controlled: EntityId) -> Intent {
            let _ = world;
            Intent::Attack {
                actor,
                target: controlled,
            }
        }

        // Act
        let acted = engine.advance_ai(
            &mut world,
            player,
            &mut attack_player,
            &EMPTY,
            &mut |_, _| {},
        );

        // Assert：受控实体应已被击杀，且结算过的实体数应远小于
        // MAX_STEPS_PER_ADVANCE（10000）。
        assert!(world.actors.get(player).is_none(), "受控实体应已被击杀");
        assert!(
            acted.len() < 10,
            "受控实体死亡后 advance_ai 应立即返回，实际结算了 {} 次",
            acted.len()
        );
    }

    #[test]
    fn 移动到实体所在格被路由成攻击() {
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let enemy = spawn_at(&mut world, (6, 5), 10);
        let raw = Intent::Move {
            actor: player,
            dir: crate::intent::Direction::East,
        };

        // Act
        let routed = route_move_to_attack(&world, raw);

        // Assert
        assert!(matches!(
            routed,
            Intent::Attack { target, .. } if target == enemy
        ));
    }

    #[test]
    fn 移动到空地不被路由成攻击() {
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let raw = Intent::Move {
            actor: player,
            dir: crate::intent::Direction::East,
        };

        // Act
        let routed = route_move_to_attack(&world, raw);

        // Assert
        assert!(matches!(routed, Intent::Move { .. }));
    }
}
