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
/// 就放弃本次推进。
///
/// 取值远大于当前任何真实场景会用到的实体数（本体一座据点物化十几个
/// NPC，全世界同时常驻的也远不到这个量级），正常运行中不会触发。
///
/// # 这道防线**不是**死循环的解药——那是一条被实机证伪的旧说法
///
/// 本常量的文档原本写着它能「防止某次行动不产生 `Effect::ScheduleNext`
/// 这类未预见的缺陷冻结整条推进路径」。**它防不住。** 放弃本次推进时，
/// 卡住的那个实体仍然以原来的 `next_action_at` 留在时间轴上，下一帧
/// `advance_ai` 会从同一个状态重跑同一段空转、再放弃一次——每帧刷一条
/// ERROR，而受控实体永远轮不到，玩家完全无法行动。所有者实机撞上的
/// 正是这个结果（见 [`TurnEngine::perform`] 文档「进展保证」一节）。
///
/// 真正的解药是那条进展保证：非受控实体的每一次行动，无论结算出什么，
/// 都必须让它的时钟往前走。本常量退回它唯一还成立的用途——**限制单次
/// 调用的时长**，让一个含有大量实体、或者未来某条真的能无限产出有效
/// 行动的规则，不至于把一帧拖到无限长。
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
    /// # 返回值：这次结算产出了几条效果
    ///
    /// `resolve` 判定「这一步什么都不发生」时返回空 `Vec`（撞墙、脚下
    /// 没东西可捡、食材不齐……见 `crate::resolve` 模块文档），本方法把
    /// 这个长度原样交出去。**这是调用方唯一能分辨「提交了但白提交」的
    /// 途径**，而分不分得出来对 AI 与对玩家的意义完全不同：AI 那条路
    /// 不在乎（下一步重新决策即可），玩家那条路在乎——按了键屏幕上一点
    /// 反应都没有，玩家会以为游戏卡死。反馈本身怎么呈现是调用方的事
    /// （`ll-sim` 不知道调用方在不在渲染，同 `on_effect` 文档），本层
    /// 只负责把这个事实交出去，见 [`PlayerTurnOutcome::Nothing`]。
    ///
    /// # 进展保证（`guarantee_progress`）
    ///
    /// `resolve` 判定「这一步什么都不发生」时返回空 `Vec`，其中**不含**
    /// `Effect::ScheduleNext`——于是这个实体的 `Agent::next_action_at`
    /// 一个 tick 都不动，而本方法末尾又照 `next_action_at` 把它重新排回
    /// 时间轴，下一次 `pop_next` 弹出的还是它、还是同一个 `entry.at`。
    /// AI 的决策来源只依赖世界状态（[`crate::behavior::BehaviorTreeSource`]
    /// 的签名就是 `&WorldState`）与一条按 `(种子, 实体号, 世界时钟)`
    /// 派生的确定性随机流（约束 C3），三个输入这一轮全都没变，因此它
    /// **必然**产出与上一轮逐字相同的意图、逐字相同的空结算——这不是
    /// 「大概率会重复」，是确定性系统里的**死循环**。
    ///
    /// 这不是假设的风险，是所有者实机撞上的缺陷：一个站在流式加载边界
    /// 上的平民朝东南方向游走，目的地那一格所属区块尚未常驻，
    /// `crate::resolve` 的 `resolve_move` 因此返回空 `Vec`（那里有一段
    /// 明确的注释论证「查不到地形，无法判断这一步本该耗时多久，静默作废
    /// 更安全」）——于是 [`Self::advance_ai`] 每帧空转满
    /// [`MAX_STEPS_PER_ADVANCE`] 步都轮不到玩家，玩家完全无法移动。
    ///
    /// 修法是这条保证，不是把上限调高：**一次失败的行动也必须推进那个
    /// 实体的时钟**。任何一条会返回空 `Vec` 的结算路径（当前至少还有
    /// 「实体在 `Interior` 内」「食材不齐」「脚下没东西可捡」若干条），
    /// 只要落到非受控实体头上，都是同一个死循环的另一个入口；逐条去修
    /// 那些结算函数治不了根——每加一条新意图就多一个入口。
    ///
    /// # 补的为什么是「等待」的结算，不是一个凭空造的 `ScheduleNext`
    ///
    /// 「这一回合什么都没做」在本 crate 里已经有一个名字，就是
    /// [`Intent::Wait`]，而「它该耗多久」这条规则已经写在
    /// `crate::resolve` 的 `resolve_wait` 里（基础行动开销按敏捷折算）。
    /// 这里重新走一次那条结算，等于复用同一条规则，不新增常量、不新增
    /// 公开接口、也不让本模块知道任何具体的时间数字——本引擎是调度者，
    /// 不是结算规则的实现者（见本方法文档开头）。这也与
    /// [`crate::behavior::behavior_ai_intent`] 早就定下的那条降级完全
    /// 一致：AI 算不出这一回合该干什么就补一个 `Wait`。本保证补的是它
    /// 覆盖不到的另一半——**算得出来、但算出来的那个意图结算为空**。
    ///
    /// `resolve_wait` 在实体不存在时同样返回空 `Vec`；那种情形下
    /// 时间轴末尾的 `world.actors.get(entry.actor)` 也查不到人，本来
    /// 就不会重新排期，不构成死循环。
    ///
    /// # 为什么受控实体不走这条保证
    ///
    /// 玩家按了一个什么都没发生的键（背包里没有那件东西、这格不能放
    /// 家具……）**不该被判成消耗了一回合**——那正是
    /// [`PlayerTurnOutcome::Nothing`] 存在的理由：这次输入没有改变世界
    /// 一个字节，下一帧仍然轮到玩家，他重新按一个有意义的键即可。玩家
    /// 那条路不存在死循环，因为下一轮的输入不是世界状态的函数，是人。
    fn perform(
        &mut self,
        world: &mut WorldState,
        entry: TimelineEntry,
        intent: Intent,
        guarantee_progress: bool,
        catalogs: &ResolveCatalogs<'_>,
        on_effect: &mut dyn FnMut(&WorldState, &Effect),
    ) -> usize {
        // 世界时钟推进——曾经在这里加过一条「只在变化时打一行」的
        // `tracing::info!` 调试日志（提交 `0ff2c9e`），因为项目所有者
        // 当时无法确认时钟到底走没走（ADR 0025 禁止合成按键，测试证明
        // 过推进但玩家本人无法实机验证）。P7 第一批（只读观测 HUD）
        // 已经把 `world.clock` 接进状态栏常驻显示（见
        // `ll_ui::hud::status_bar`），时间对所有者可见现在靠界面，不再
        // 需要靠日志刷屏确认时钟活着，该日志已按代码注释原文「P7 时间
        // UI 落地后应摘除」移除，不留任何调试期专用分支。
        world.clock = entry.at;
        let mut effects = resolve_with_catalogs(world, &intent, catalogs);
        // 进展保证（`guarantee_progress`）：这一步什么都没发生时，补一次
        // 「等待」的结算，让这个实体的时钟无论如何都往前走。判据与代价
        // 的完整论证见本方法文档「进展保证」一节。
        if effects.is_empty() && guarantee_progress {
            effects = resolve_with_catalogs(world, &Intent::Wait { actor: entry.actor }, catalogs);
        }
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
        effects.len()
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
    /// `ai_intent` 完全可能产出一个 `resolve` 判定为空效果的 `Intent`
    /// （撞进未加载区块的 `Move`、`Interior` 里的 `Move`、食材不齐的
    /// `Craft`……）。空效果里没有 `Effect::ScheduleNext`，该实体的
    /// `next_action_at` 原地不动，重新排入时间轴后会在**同一个 tick**
    /// 被立刻弹出，而决策的三个输入（世界状态、`(种子, 实体号, 世界
    /// 时钟)` 派生的随机流、行为树本身）一个都没变，于是它必然重复
    /// 同一个决定——死循环。
    ///
    /// 这不是假设的风险，出现过两次：`p3_acceptance` 的固定策略 AI 曾
    /// 因为快速敌人朝玩家方向的下一格恰好是深水而卡死（该 demo 已在
    /// `ai_intent` 自己那一层修了自己那一处）；本体二进制则因为流式
    /// 加载边界上的平民朝未常驻区块游走而卡死，玩家完全无法移动。
    ///
    /// 两次都说明「在决策来源那一层逐个避开会返回空效果的意图」治不了
    /// 根——决策来源不可能穷举结算侧全部的静默拒绝条件。根因修在
    /// [`Self::perform`] 的进展保证里：本函数结算的每一个非受控实体，
    /// 无论 `resolve` 判定了什么，时钟都必然往前走，见该方法文档
    /// 「进展保证」一节。[`MAX_STEPS_PER_ADVANCE`] 不是这条保证的替代
    /// 品，见其自身文档。
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
            let raw = ai_intent(world, entry.actor, controlled);
            // 撞格路由对 NPC 同样生效，但**互换那一支只对玩家开**
            // ——见 [`route_move_into_occupant`] 文档「玩家优先度高于
            // NPC」一节。
            let intent = route_move_into_occupant(world, raw);
            // `true`：非受控实体这条路必须保证进展，见 [`Self::perform`]
            // 文档「进展保证」一节与本方法文档「必须保证进展」一节。
            self.perform(world, entry, intent, true, catalogs, on_effect);
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
    /// 撞进另一个存活实体所在格的 `Move` 会被就地路由成 `Attack`（敌对）
    /// 或 [`Intent::Swap`]（非敌对；**只有受控实体走得到互换这一支**），
    /// 见 [`route_move_into_occupant`]；
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
        let Some(raw) = intent_from_input(player, input) else {
            return false;
        };
        !matches!(
            self.try_player_intent(world, player, raw, catalogs, on_effect),
            PlayerTurnOutcome::NotYet
        )
    }

    /// 用一个**已经选好的**意图结算受控实体的一次行动。
    ///
    /// # 为什么需要这条入口，[`Self::try_player_turn`] 不够
    ///
    /// [`crate::intent::intent_from_input`] 按设计不读 `WorldState`
    /// （见其文档与 `crate::intent` 模块文档「本层只管『按了什么键』」
    /// 一节），因此它产得出 `Move`/`Wait`/`PickUp`/`ToggleStealth` 这类
    /// **不需要参数**的意图，产不出 `Craft { recipe }`/`Drop { def }`
    /// 这类**要先选一条**的意图——「选哪一条」是一次真实的玩法选择，
    /// 得由一块看得见背包/配方列表的菜单来做，而那块菜单住在
    /// `ll-game`/`ll-ui`，不在本 crate。
    ///
    /// 于是分工是：本 crate 提供「提交一个意图当作玩家这一回合」的
    /// 通道，选择本身由持有菜单的那一层完成。[`Self::try_player_turn`]
    /// 现在就是本方法加上一次 `intent_from_input`，两条路径共用同一段
    /// 「查 `pending` → 撞格路由 → `perform`」逻辑，不是各抄
    /// 一遍（ADR 0021）。
    ///
    /// 返回值见 [`PlayerTurnOutcome`]：三态而不是 `bool`,因为玩家那条
    /// 路要分得出「还没轮到我」与「轮到了但这一步白按了」——后者需要
    /// 给玩家一句反馈，见 [`Self::perform`] 文档「返回值」一节。
    pub fn try_player_intent(
        &mut self,
        world: &mut WorldState,
        player: EntityId,
        intent: Intent,
        catalogs: &ResolveCatalogs<'_>,
        on_effect: &mut dyn FnMut(&WorldState, &Effect),
    ) -> PlayerTurnOutcome {
        let Some(entry) = self.pending.filter(|entry| entry.actor == player) else {
            return PlayerTurnOutcome::NotYet;
        };
        let routed = route_move_into_occupant(world, intent);
        self.pending = None;
        // `false`：受控实体这条路**刻意不保证进展**，见 [`Self::perform`]
        // 文档「为什么受控实体不走这条保证」一节——玩家白按一次不该被
        // 判成消耗了一回合。
        if self.perform(world, entry, routed, false, catalogs, on_effect) == 0 {
            PlayerTurnOutcome::Nothing
        } else {
            PlayerTurnOutcome::Acted
        }
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

/// [`TurnEngine::try_player_intent`] 的三种结果。
///
/// # 为什么不是 `bool`
///
/// 「没轮到玩家」与「轮到了、也提交了，但结算判定这一步什么都不发生」
/// 对调用方是两件完全不同的事：前者根本没有消费这次输入（下一帧原样
/// 重试即可），后者已经消费掉了，且**玩家看不出任何变化**。把两者压
/// 成同一个 `false` 正是「静默作废」对玩家不成立的根源——放置家具的
/// 三道前置（层可建造 / 地形不挡路 / 这格还没家具，见
/// `crate::resolve` 的 `resolve_drop` 文档）任一不成立时整条静默作废，
/// 玩家按了键屏幕纹丝不动，只会以为游戏卡了。
///
/// **本枚举不解释「为什么什么都没发生」**，只报告「确实什么都没发生」。
/// 把理由也带出来需要让 `crate::resolve` 的每一条静默返回点各自携带
/// 一个原因码——那是一次独立的、波及二十多个结算函数的改造，且当前
/// 唯一的消费者只需要「有没有反应」这一位信息。真要做，加法是给
/// `resolve` 系列换一个 `Result<Vec<Effect>, Blocked>` 形状的返回值,
/// 与本枚举无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTurnOutcome {
    /// 还没轮到这个实体（`pending` 是别人，或时间轴已空）——这次输入
    /// **没有**被消费。
    NotYet,
    /// 轮到了、意图也提交了，但 `resolve` 一条效果都没产出：这一步
    /// 白按了，且世界状态一个字节都没变（连 `Effect::ScheduleNext`
    /// 都没有，因此下一帧仍然轮到同一个实体）。
    Nothing,
    /// 真的发生了：至少一条效果被 `apply`。
    Acted,
}

/// 把一个原始意图路由成最终意图：一次 [`Intent::Move`] 的目的地站着
/// 别的存活实体时，按双方是否**已声明敌对**
/// （[`ll_sim_declared_hostile`](crate::ai_query::declared_hostile)）
/// 以及**发起者是不是受控实体**，改判为 [`Intent::Attack`] 或
/// [`Intent::Swap`]；目的地空着就原样放行。
///
/// # 三条分支
///
/// | 目的地 | 发起者 | 结果 |
/// |---|---|---|
/// | 空着 | 任意 | 原样 [`Intent::Move`] |
/// | 站着已声明敌对的实体 | 任意 | [`Intent::Attack`] |
/// | 站着非敌对实体 | **受控实体** | [`Intent::Swap`] |
/// | 站着非敌对实体 | 非受控实体 | 原样 [`Intent::Move`]（随后由
///   [`crate::resolve`] 的占位检查判成一次失败的移动） |
///
/// # 敌对那一支的裁定来源
///
/// 项目所有者的原话：「当角色与NPC非敌对的时候，移动向NPC的位置时，
/// 是和NPC互换位置，而敌对NPC则是对敌对NPC攻击。」敌对那一半此前就
/// 存在（撞人即攻击，传统 roguelike 手感），只是**无条件**成立——于是
/// 走向一个农夫就是砍他。
///
/// 判据用 [`crate::ai_query::declared_hostile`] 而不是
/// [`crate::ai_query::is_hostile`]，理由见前者文档：后者在当前内容下
/// 对每一对实体都返回真，拿它当判据等于这条裁定一个字都没落地。
///
/// # 玩家优先度高于 NPC：**只有玩家可以互换位置**
///
/// **这一段推翻了本函数此前的实现。** 上一批把所有者那句「角色与NPC」
/// 读作双向都成立，于是两个非敌对 NPC 撞上也互换位置。项目所有者随后
/// 明确裁定：**玩家优先度高于 NPC，只有玩家可以互换位置**。互换是一次
/// 让路——被换的一方没有做出任何决定却被挪走了（见
/// [`crate::resolve`] 的 `resolve_swap` 文档「只重排发起者」一节），
/// 而「谁有资格要求别人给自己让路」这件事在本作里不是对称的。
///
/// NPC 撞上非敌对目标因此**不再互换**，而是原样放行成
/// [`Intent::Move`]，交给 `resolve_move` 的占位检查判成一次失败的移动：
/// 不产 `Effect::MoveTo`，但仍产 `Effect::ScheduleNext`——与撞墙完全
/// 同一个口径。这一条不会造成死循环（效果非空 ⇒ 不触发
/// [`TurnEngine::perform`] 的进展保证），但它确实带来一个**已知的、
/// 尚未裁定的副作用**：`crate::ai_query::direction_toward` 任何距离都
/// 返回方向、从不因「已相邻」停手，因此贴身跟着非敌对目标的 NPC 会
/// 每回合撞一次、失败一次、消耗一次行动。要不要给「靠近」加一条「已
/// 相邻就不再挪」是一条独立的、尚未裁定的问题，本批次不做。
///
/// 「是不是受控实体」的判据用 `world.player_entity == Some(actor)`
/// ——`crate::resolve` 里判 `Effect::MarkExplored` 该不该追加用的就是
/// 这一个比较（`resolve_move`/`resolve_swap` 各一处），不新开第二套
/// 「谁是受控实体」的表示法。
///
/// # 查找目标格的那段代码在 `crate::resolve` 里
///
/// [`crate::resolve::step_destination`]/[`crate::resolve::occupant_at`]
/// ——与 `resolve_move` 的占位检查**共用同一份实现**，理由见后者文档
/// 「为什么必须只有这一份实现」一节：两处问的是同一个问题，答案必须
/// 逐字一致，各写一遍会让平局打破规则各自漂移。
fn route_move_into_occupant(world: &WorldState, raw: Intent) -> Intent {
    let Intent::Move { actor, dir } = raw else {
        return raw;
    };
    let Some(agent) = world.actors.get(actor) else {
        return raw;
    };
    let dest = crate::resolve::step_destination(world, agent.pos, dir);
    let Some((target, other)) = crate::resolve::occupant_at(world, dest, actor) else {
        return raw;
    };
    if crate::ai_query::declared_hostile(agent, other) {
        return Intent::Attack { actor, target };
    }
    if world.player_entity == Some(actor) {
        return Intent::Swap {
            actor,
            with: target,
        };
    }
    // 非受控实体撞上非敌对实体：原样放行，由 `resolve_move` 的占位
    // 检查判成一次失败的移动——见本函数文档「玩家优先度高于 NPC」一节。
    raw
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
        // 撞格路由只把**已声明敌对**的一对判成攻击（所有者裁定，见
        // `route_move_into_occupant`）——本用例要的正是那一支，因此
        // 两人分属不同势力。
        join_faction(&mut world, player, 1);
        join_faction(&mut world, victim, 2);
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

    /// 给一个实体挂一条势力归属——[`crate::ai_query::declared_hostile`]
    /// 只在至少一方声明过势力时才可能判敌对，见其文档。
    fn join_faction(world: &mut WorldState, actor: EntityId, faction: u32) {
        let agent = world.actors.get_mut(actor).expect("实体刚生成");
        agent.affiliations.push(ll_world::entity::Affiliation {
            kind: ll_world::entity::AffiliationKind::Faction,
            org: ll_world::entity::OrgRef::Instance(ll_core::ident::WorldId::next(&mut {
                faction
            })),
            standing: 0,
        });
    }

    #[test]
    fn 移动到已声明敌对的实体所在格被路由成攻击() {
        // Arrange：两人分属不同势力，因此是已声明的敌对关系。
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let enemy = spawn_at(&mut world, (6, 5), 10);
        join_faction(&mut world, player, 1);
        join_faction(&mut world, enemy, 2);
        let raw = Intent::Move {
            actor: player,
            dir: crate::intent::Direction::East,
        };

        // Act
        let routed = route_move_into_occupant(&world, raw);

        // Assert
        assert!(matches!(
            routed,
            Intent::Attack { target, .. } if target == enemy
        ));
    }

    #[test]
    fn 受控实体移动到非敌对实体所在格被路由成互换位置() {
        // 所有者裁定：「当角色与NPC非敌对的时候，移动向NPC的位置时，
        // 是和NPC互换位置」。两人都没有任何势力归属——这正是当前内容
        // 下每一个实体的真实形态，见 `declared_hostile` 文档。
        //
        // **`world.player_entity` 这一行是断言的一部分，不是布景**：
        // 互换那一支现在只对受控实体开（所有者裁定「玩家优先度高于
        // NPC」），见 `route_move_into_occupant` 文档。删掉它这条会
        // 立刻变红——下面那条 NPC 用例正是它的对照组。
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let neighbour = spawn_at(&mut world, (6, 5), 10);
        world.player_entity = Some(player);
        let raw = Intent::Move {
            actor: player,
            dir: crate::intent::Direction::East,
        };

        // Act
        let routed = route_move_into_occupant(&world, raw);

        // Assert
        assert_eq!(
            routed,
            Intent::Swap {
                actor: player,
                with: neighbour
            }
        );
    }

    #[test]
    fn 非受控实体移动到非敌对实体所在格不互换而是原样放行成移动() {
        // 所有者裁定「玩家优先度高于NPC，只有玩家可以互换位置」，
        // 推翻了上一批「双向都成立」的读法。**互换是一次让路**，被换的
        // 一方没有做出任何决定却被挪走了（见 `resolve_swap` 文档「只
        // 重排发起者」一节），而「谁有资格要求别人给自己让路」在本作里
        // 不是对称的。
        //
        // 与上面那条受控实体用例逐字段相同,唯一的差别是 `player_entity`
        // 指向另一个人——这一条与那一条构成同一个场景的正反两例。
        // Arrange
        let mut world = test_world();
        let npc = spawn_at(&mut world, (5, 5), 10);
        let neighbour = spawn_at(&mut world, (6, 5), 10);
        world.player_entity = Some(neighbour);
        let raw = Intent::Move {
            actor: npc,
            dir: crate::intent::Direction::East,
        };

        // Act
        let routed = route_move_into_occupant(&world, raw);

        // Assert：原样放行，交给 `resolve_move` 的占位检查判成失败。
        assert_eq!(routed, raw);
    }

    #[test]
    fn 同一势力的两人撞格不互相攻击而是由发起者身份决定后果() {
        // 反例，守的是「已声明势力」这一半不会反过来把队友判成敌人。
        //
        // 本条在裁定「只有玩家可以互换位置」之后拆成了两半：同一对
        // 队友，受控实体撞过去是互换、非受控实体撞过去是原样放行——
        // **两半都不是 `Attack`**，那才是本条真正守的东西。只断言
        // 「不是 Attack」会把这条放松成一句几乎恒真的话，因此这里逐
        // 分支钉住它到底路由成了什么。
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let ally = spawn_at(&mut world, (6, 5), 10);
        join_faction(&mut world, player, 7);
        join_faction(&mut world, ally, 7);
        let raw = Intent::Move {
            actor: player,
            dir: crate::intent::Direction::East,
        };

        // Act：先以受控实体的身份走一次，再以非受控实体的身份走一次。
        world.player_entity = Some(player);
        let routed_as_player = route_move_into_occupant(&world, raw);
        world.player_entity = Some(ally);
        let routed_as_npc = route_move_into_occupant(&world, raw);

        // Assert
        assert_eq!(
            routed_as_player,
            Intent::Swap {
                actor: player,
                with: ally
            }
        );
        assert_eq!(routed_as_npc, raw);
    }

    #[test]
    fn 非受控实体撞上非敌对实体经turnengine结算后不移动但时钟仍前进() {
        // 裁定「只有玩家可以互换位置」的**可观测后果**，走的是完整
        // 链路（route → resolve → apply），不只是路由返回了什么：
        //
        // - 两人的坐标都没变（既没互换，也没摞在一起）；
        // - 撞人的那一个 `next_action_at` 真的前进了——与撞墙同一个
        //   口径，NPC 不会因为前面站了人就白赚一回合。
        //
        // 时钟这一半不是顺带：若占位检查退化成返回空 `Vec`，
        // `TurnEngine::perform` 的进展保证会补跑一次 `Intent::Wait`,
        // 时钟照样前进、坐标照样不变——本条的**前两个**断言在那种实现
        // 下仍然全绿。真正把两者分开的是第三个断言：这一步必须自己
        // 产出效果,不能靠进展保证兜底。
        // Arrange
        let mut world = test_world();
        let npc = spawn_at(&mut world, (5, 5), 10);
        let neighbour = spawn_at(&mut world, (6, 5), 10);
        // 受控实体是第三个人，站在远处、永远轮不到它挪动——`advance_ai`
        // 需要一个「不是 npc」的受控实体才肯结算 npc 那一条。
        let controlled = spawn_at(&mut world, (20, 20), 10);
        world.player_entity = Some(controlled);
        let npc_before = world.actors.get(npc).expect("刚生成").pos;
        let neighbour_before = world.actors.get(neighbour).expect("刚生成").pos;

        let mut timeline = Timeline::new();
        timeline.schedule(npc, Tick(0));
        timeline.schedule(controlled, Tick(TICKS_PER_MINUTE));
        let mut engine = TurnEngine::new(timeline);
        let mut effects_seen = 0usize;

        // Act：npc 朝东撞上 neighbour。
        engine.advance_ai(
            &mut world,
            controlled,
            &mut |_, actor, _| Intent::Move {
                actor,
                dir: crate::intent::Direction::East,
            },
            &EMPTY,
            &mut |_, _| effects_seen += 1,
        );

        // Assert
        assert_eq!(
            world.actors.get(npc).expect("还在").pos,
            npc_before,
            "非受控实体撞上非敌对实体不该挪动"
        );
        assert_eq!(
            world.actors.get(neighbour).expect("还在").pos,
            neighbour_before,
            "被撞的一方更不该被挪走——互换只对受控实体开"
        );
        assert!(
            world.actors.get(npc).expect("还在").next_action_at.0 > 0,
            "撞人仍然消耗一次行动，与撞墙同一个口径"
        );
        assert!(
            effects_seen > 0,
            "这一步必须自己产出 ScheduleNext,不能靠 perform 的进展保证兜底"
        );
    }

    #[test]
    fn 移动到空地不被路由() {
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let raw = Intent::Move {
            actor: player,
            dir: crate::intent::Direction::East,
        };

        // Act
        let routed = route_move_into_occupant(&world, raw);

        // Assert
        assert!(matches!(routed, Intent::Move { .. }));
    }

    #[test]
    fn 玩家与非敌对实体互换位置后两人的坐标真的对调了() {
        // 这一条测的是路由之后的整条链路（route → resolve → apply），
        // 不只是路由本身返回了什么。裁定「只有玩家可以互换位置」只
        // 收紧了 NPC 那一侧，玩家这一侧一个字没改——本条就是那句话的
        // 可执行形式，它一旦变红说明收紧收过头了。
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let neighbour = spawn_at(&mut world, (6, 5), 10);
        world.player_entity = Some(player);
        let player_before = world.actors.get(player).expect("刚生成").pos;
        let neighbour_before = world.actors.get(neighbour).expect("刚生成").pos;
        let mut timeline = Timeline::new();
        timeline.schedule(player, Tick(0));
        let mut engine = TurnEngine::new(timeline);
        engine.advance_ai(&mut world, player, &mut no_op_ai, &EMPTY, &mut |_, _| {});

        // Act
        let outcome = engine.try_player_intent(
            &mut world,
            player,
            Intent::Move {
                actor: player,
                dir: crate::intent::Direction::East,
            },
            &EMPTY,
            &mut |_, _| {},
        );

        // Assert
        assert_eq!(outcome, PlayerTurnOutcome::Acted);
        assert_eq!(
            world.actors.get(player).expect("玩家还在").pos,
            neighbour_before
        );
        assert_eq!(
            world.actors.get(neighbour).expect("邻居还在").pos,
            player_before
        );
    }

    /// 一个恒产出「结算为空」意图的 AI：实体被挪进 `Interior` 之后，
    /// `crate::resolve` 的 `resolve_move` 会直接返回空 `Vec`（Interior
    /// 内部漫游不在范围内），因此这个意图**确定地**一条效果都不产出。
    ///
    /// 这是所有者实机撞上的那类意图的一个可复现替身：真实触发点是
    /// 「目的地所属区块尚未常驻」，同样返回空 `Vec`，同样不带
    /// `Effect::ScheduleNext`。真实触发点本身的端到端复现在
    /// `crates/ll-game/tests/ai_stall.rs`。
    fn always_futile_move(world: &WorldState, actor: EntityId, _controlled: EntityId) -> Intent {
        let _ = world;
        Intent::Move {
            actor,
            dir: crate::intent::Direction::East,
        }
    }

    /// 把一个实体挪进 `Interior`——它的 `Intent::Move` 从此恒结算为空。
    fn move_into_interior(world: &mut WorldState, actor: EntityId) {
        let anchor = world.size.wrap(0, 0);
        world
            .actors
            .get_mut(actor)
            .expect("实体刚生成")
            .current_space = ll_world::space::Space::Interior {
            id: ll_core::ident::WorldId::next(&mut 1),
            floor: 0,
            anchor,
            profile: ll_core::ident::ContentIndex::default(),
        };
    }

    #[test]
    fn 结算为空的ai行动仍然推进该实体的时钟并让出回合给玩家() {
        // 回归测试：修复前，这个 NPC 的 next_action_at 原地不动，
        // advance_ai 会在同一 tick 上反复弹出它直到耗尽
        // MAX_STEPS_PER_ADVANCE（10000）都轮不到玩家——每帧刷一条
        // ERROR，玩家完全无法行动。
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        let stuck = spawn_at(&mut world, (20, 20), 10);
        move_into_interior(&mut world, stuck);
        let stuck_at_before = world.actors.get(stuck).expect("刚生成").next_action_at;
        let mut timeline = Timeline::new();
        timeline.schedule(stuck, Tick(0));
        timeline.schedule(player, Tick(1));
        let mut engine = TurnEngine::new(timeline);

        // Act
        let acted = engine.advance_ai(
            &mut world,
            player,
            &mut always_futile_move,
            &EMPTY,
            &mut |_, _| {},
        );

        // Assert：卡住的那个只被结算了一次，且它的时钟真的往前走了。
        assert_eq!(acted, vec![stuck], "空转的 NPC 应当只被结算一次");
        let stuck_at_after = world.actors.get(stuck).expect("还在").next_action_at;
        assert!(
            stuck_at_after > stuck_at_before,
            "结算为空的行动仍必须推进该实体的时钟：{} → {}",
            stuck_at_before.0,
            stuck_at_after.0
        );
        // 且轮次真的让给了玩家——这是所有者那条验收线的机器版本。
        let mut input = InputState::new();
        input.press(GameKey::Wait);
        assert!(
            engine.try_player_turn(&mut world, player, &input, &EMPTY, &mut |_, _| {}),
            "玩家必须能在下一步拿到自己的回合"
        );
    }

    #[test]
    fn 玩家提交一个结算为空的意图时不消耗回合() {
        // 反例，守的是上一条那条进展保证**没有**被顺手套到玩家身上：
        // 玩家白按一次仍应是 Nothing（世界一个字节没变，下一帧还轮到
        // 他），而不是被补一次「等待」消耗掉一回合。
        // Arrange
        let mut world = test_world();
        let player = spawn_at(&mut world, (5, 5), 10);
        move_into_interior(&mut world, player);
        let before = world.actors.get(player).expect("刚生成").next_action_at;
        let mut timeline = Timeline::new();
        timeline.schedule(player, Tick(0));
        let mut engine = TurnEngine::new(timeline);
        engine.advance_ai(&mut world, player, &mut no_op_ai, &EMPTY, &mut |_, _| {});

        // Act
        let outcome = engine.try_player_intent(
            &mut world,
            player,
            Intent::Move {
                actor: player,
                dir: crate::intent::Direction::East,
            },
            &EMPTY,
            &mut |_, _| {},
        );

        // Assert
        assert_eq!(outcome, PlayerTurnOutcome::Nothing);
        assert_eq!(
            world.actors.get(player).expect("玩家还在").next_action_at,
            before,
            "玩家白按一次不该被判成消耗了一回合"
        );
    }
}
