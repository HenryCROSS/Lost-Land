//! `Effect`：描述「发生了什么」的纯数据，是 `resolve`（批次 C）与
//! [`crate::apply::apply`] 之间的唯一接口。
//!
//! `resolve` 读世界、算规则、决定判定结果，但**绝不直接改世界**——它
//! 的产出是一串 `Effect` 值，纯数据，不含任何执行逻辑。真正的写入
//! 全部交给 [`crate::apply::apply`] 一处完成（见该函数文档的「三条
//! 纪律」）。这个分离是并行结算的前提：成千上万个 AI 的 `resolve` 可以
//! 同时跑（各自只读世界，互不冲突），产出的 `Effect` 收集起来后再单
//! 线程依次 `apply`，读写从不交织。

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_world::entity::{AttributeKind, EntityId};
use ll_world::history::KillCause;
use ll_world::script_state::ScriptStateWrite;
use ll_world::space::Space;
use ll_world::terrain::TerrainKind;

use crate::skill::ResourceKind;

/// 「发生了什么」的纯数据描述。
///
/// 不要求可序列化（不像 [`crate::intent::Intent`]）：`Effect` 是
/// `resolve` 到 `apply` 之间同一进程内、同一次结算里的瞬时产物，算完
/// 立刻被 `apply` 消费掉，不需要跨进程/跨存档留存——真正要长期保留、
/// 用于重放的是产生它的 [`crate::intent::Intent`]。
///
/// # 为什么没有季节相关变体（W-03，P3 收尾裁定）
///
/// 规格 §7.2 原文把季节更替描述成时间轴上的定时 `Effect`，但 P3 收尾
/// 时裁定季节维持纯函数派生（见 [`ll_world::light::season_light_scale`]
/// 文档「裁定：季节是纯函数派生」一节的完整理由）：季节原本要驱动的
/// 城镇生产速率、地形通行性、野怪分布表三者本身都还不存在，为它们
/// 预留一个尚无内容可改的 `Effect` 变体没有意义。真正引入这些系统的
/// 阶段落地时，应由那个阶段的实现者决定各自是否需要接入 `Effect`，
/// 而不是现在为空气发一个变体。
/// # 为什么不再 `Copy`（脚本状态存储批次）
///
/// [`Effect::SetScriptState`] 携带一个 `Vec<ScriptStateWrite>`——`Vec`
/// 不是 `Copy`，`derive(Copy)` 因此不能再成立。检查过全部既有调用点
/// （`resolve.rs`/`apply.rs`/`ll-sim/tests/replay.rs` 等）：全部通过
/// `&Effect` 引用或值移动使用 `Effect`，没有任何地方依赖隐式按位拷贝，
/// 去掉 `Copy` 不需要改动这些调用点的用法。
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// 把某实体的位置设为 `pos`。
    MoveTo {
        /// 被移动的实体。
        actor: EntityId,
        /// 目标位置。
        pos: TorusPos,
    },
    /// 对某实体造成 `amount` 点伤害（从其生命值里减去）。
    Damage {
        /// 受创的实体。
        target: EntityId,
        /// 伤害量。是否致死等规则判断不在这里——`apply` 只做减法。
        amount: i32,
    },
    /// 销毁某实体。
    Kill {
        /// 被销毁的实体。
        target: EntityId,
        /// 谁杀的——环境/坠落/饥饿致死，或击杀者本身尚未"具名"（见
        /// `ll_world::entity::Agent::remembered_id` 文档）时为 `None`。
        ///
        /// # 为什么这是朴素数据（C2）
        ///
        /// `killer` 是 `EntityId`——一个不透明的整数句柄，不含引用、
        /// 闭包或裸指针，与 `Effect::Damage.target`/`Effect::MoveTo.actor`
        /// 同一类值，见 `Effect::RecordHistoricalEvent` 文档「决策在
        /// resolve」一节的完整论证。
        killer: Option<EntityId>,
        /// 怎么杀的——项目所有者点名要求的"精确到武器/技能/环境"这一
        /// 级，见 `knowledge/design/kill-and-death-events.md` 二节。
        cause: KillCause,
    },
    /// 落盘一条击杀历史事件（`knowledge/design/kill-and-death-events.md`
    /// 五节）——`resolve` 侧的 `append_kill_history`
    /// （`crate::resolve` 模块）判断这场击杀是否值得被记录（`victim`
    /// 已具名，见其文档），值得记录时产出本效果；`apply` 响应它时调用
    /// [`ll_world::state::WorldState::record_kill`]，由该方法分配
    /// `WorldId`、把事件追加进 `WorldState::history`。
    ///
    /// # 决策在 `resolve`，不在 `apply`（约束 C1）
    ///
    /// 是否要产出本效果（分级规则：`victim` 是否已具名）本身是游戏
    /// 逻辑判断，必须在 `resolve` 做出——`apply` 三条纪律之一是「不含
    /// 任何游戏逻辑」。本效果携带的全部字段因此都是 `resolve` 已经
    /// 读出的朴素数据（`EntityId`/`KillCause`/`Tick`/`TorusPos`/`i32`），
    /// `apply` 只是照单据执行写入,不再做任何判断。
    ///
    /// # 为什么必须排在对应的 `Effect::Kill` 之前
    ///
    /// `WorldState::record_kill` 需要读取（并可能懒分配）`victim` 的
    /// `remembered_id`——若 `Effect::Kill` 已经把 `victim` 从
    /// `world.actors` 里销毁，这个查询会直接失败、静默不产出任何记录。
    /// `crate::resolve` 模块内部的 `append_kill_history`（模块私有，
    /// 不对外公开，故此处不使用文档链接）因此把本效果插入在对应
    /// `Effect::Kill` **之前**，而不是像 `append_quest_kill_progress`
    /// 那样追加在整批效果的末尾。
    RecordHistoricalEvent {
        /// 事件发生的世界时刻。
        at: Tick,
        /// 事件发生地点——记录时 `victim` 仍然存活，取自其当前 `pos`。
        location: TorusPos,
        /// 被击杀的实体。
        victim: EntityId,
        /// 击杀者，若有。
        killer: Option<EntityId>,
        /// 怎么杀的。
        cause: KillCause,
        /// 致命一击造成的伤害量。
        damage: i32,
        /// 致命一击结算后的剩余生命值。
        remaining_health: i32,
    },
    /// 累加一次击杀计数（项目所有者决策二：「一起计算，就是杀了 10
    /// 只」，取代了决策一「无名单位击杀改计数」原有的互斥设计）——
    /// `resolve` 侧的 `append_kill_history`（`crate::resolve` 模块）对
    /// **每一场**击杀都产出本效果，不论受害者是否"具名"
    /// （`Agent::remembered_id`）；受害者已具名时，`append_kill_history`
    /// 还会**额外**再产出一条 `Effect::RecordHistoricalEvent`（完整
    /// 记录，见其文档）——两者是叠加关系，不是互斥的替代关系，本效果
    /// 不会因为已经产出了完整记录就被跳过。`apply` 响应它时调用
    /// [`ll_world::state::WorldState::record_kill_count`]，按
    /// `kind` 归并进 [`ll_world::state::WorldState::kill_counts`]。
    ///
    /// # 为什么按 `kind: ContentIndex`，不是新建一张生物类型注册表
    ///
    /// `Agent::creature_kind` 字段本就已经声明了这个归并键的取法——
    /// `Some` 时用它本身，`None`（绝大多数"有种族意义"的智慧类人型）
    /// 时回退到 `Agent::race`（见该字段文档「用于击杀匹配与死因统计
    /// 分类」一节，与 `crate::quest` 模块「击杀计数」既有的
    /// `target_kind: ContentIndex` 匹配规则同一套键空间）。核实过当前
    /// 仓库没有任何 `CreatureKindDef` 注册表——引入一张只服务本次计数
    /// 而不服务任何其它系统的新注册表是投机性设计（YAGNI），复用已经
    /// 存在的字段与既有回退规则就是"最小的 kind 标识"。
    ///
    /// # 为什么必须排在对应的 `Effect::Kill` 之前
    ///
    /// 与 `Effect::RecordHistoricalEvent` 同一条纪律：本效果携带的
    /// `kind` 由 `resolve` 阶段读取（结算前仍然存在的）受害者的
    /// `creature_kind`/`race` 算出，`apply` 只是照单据把这个已经算好
    /// 的值累加进去，不重新判断一遍"该按什么归并"。
    IncrementKillCount {
        /// 归并键——受害者的 `creature_kind`，若为 `None` 则回退到
        /// `race`，见本变体文档「为什么按 kind: ContentIndex」一节。
        kind: ContentIndex,
    },
    /// 把某实体下一次可行动的时刻设为 `at`。
    ///
    /// 只写 [`ll_world::entity::Agent::next_action_at`] 这个字段本身，
    /// 不触碰任何时间轴队列——真正把该实体重新排入时间轴（调用
    /// `ll_sim::timeline::Timeline::schedule`）是调用方在 `apply`
    /// 返回之后另行要做的事：`apply` 的签名只有 `&mut WorldState`，
    /// 拿不到调用方持有的 `Timeline`（`Timeline` 定义在本 crate，是
    /// 运行期的调度缓存，不是存档的一部分，因此不在 `WorldState`
    /// 内——见 `timeline` 模块文档）。
    ScheduleNext {
        /// 被重新安排的实体。
        actor: EntityId,
        /// 下一次可行动的世界时刻。
        at: Tick,
    },
    /// 把某位置的地形设为 `kind`。
    SetTerrain {
        /// 目标位置。
        pos: TorusPos,
        /// 目标地形。
        kind: TerrainKind,
    },
    /// 调整某实体的钱包，`delta` 可正可负。
    AdjustWallet {
        /// 被调整的实体。
        actor: EntityId,
        /// 调整量。
        delta: i64,
    },
    /// 把某实体的当前空间设为 `space`（任务 12：两级坐标系重写）。
    ///
    /// `apply` 响应这个效果时，除了写 `Agent::current_space` 本身，还
    /// 要同步 [`ll_world::state::WorldState::enter_interior`]/
    /// [`ll_world::state::WorldState::exit_interior`]——这两步共同维护
    /// 「当前所在空间的锚点区块钉住不淘汰」（裁定 CS-3），是 `apply`
    /// 里少数几个需要碰两处状态才能保持内部一致的效果之一，见
    /// [`crate::apply::apply`] 对应分支的文档。
    ChangeSpace {
        /// 被切换的实体。
        actor: EntityId,
        /// 目标空间。
        space: Space,
    },
    /// 批量写入脚本状态（裁定 P5-1，脚本状态存储）。
    ///
    /// # 为什么脚本状态的写入也要走 `Effect`
    ///
    /// `knowledge/design/script-state-storage.md` 8.2 节原文写「直接
    /// 写穿，没有中间层」，与本文档开篇、约束 C1「`apply` 是唯一写
    /// 入口」字面冲突——脚本状态存在 `WorldState` 里（
    /// `WorldState::global_script_state`/`Agent::script_state`），那就
    /// 是世界状态的一部分：写它就是改世界，绕开 `Effect` 流意味着
    /// 「同一串 Intent 重放」复现不出脚本存的东西，「脚本自己的数据」
    /// 这个类别在存档/重放的意义上并不真实存在。裁定 P5-1 选择 C1
    /// 赢：写入必须经这条唯一入口。
    ///
    /// # 为什么是一条 `Effect` 携带一批，而不是每次写入一条
    ///
    /// 一次决策期间脚本可能连续调用多次 `state-set!`/
    /// `entity-state-set!`——若每次调用都发一条独立 `Effect`，会为每
    /// 次写入多付一条 `Effect` 的开销。`ll_script::api::state` 模块在
    /// 脚本调用窗口内把写入攒进一个线程局部缓冲，调用结束后宿主取走
    /// 整批，包成一条本变体发出，交给既有的 `resolve → apply` 管线——
    /// `Effect` 流因此保持「每一次状态变化都经过它」的诚实，又不必为
    /// 每次写入单独付一条 `Effect` 的开销。
    SetScriptState {
        /// 这条 `Effect` 携带的全部写入，保留脚本调用它们时的原始
        /// 顺序——同一个键在批内被覆写多次时，`apply` 按顺序逐条写入，
        /// 最终生效的是最后一条（与 `ll-script` 侧缓冲区「同一决策内
        /// 重复写同一个键只保留最后一次」的既有语义一致，见
        /// [`ScriptStateWrite::matches`] 文档）。
        writes: Vec<ScriptStateWrite>,
    },
    /// 把某个实体的某项资源（法力/耐力）调整 `delta`（P5-B 任务 5）。
    ///
    /// # 为什么这是本任务在计划草案之外新增的第三个变体
    ///
    /// `docs/superpowers/plans/2026-08-19-p5-gameplay-systems.md` 任务 5
    /// 的接口草案只列了 `SetSkillCooldown`/`ApplyStatModifier` 两个新
    /// 变体，但技能的 `resource_cost`（施放消耗）与
    /// `SkillEffect::RestoreResource`（技能效果本身恢复资源）都需要
    /// 一个「按增量调整某资源当前值」的落点——现有变体里没有一个能
    /// 表达它（[`Effect::AdjustWallet`] 专用于货币，语义不通用）。留着
    /// 这个缺口不补,会让 `resource_cost` 永远只是一个摆设的门槛检查
    /// （能通过检查但从不真正扣减,技能可以被无限次免费施放）——这不是
    /// 「有意留给后续阶段」的缺口,是会让资源系统名不副实的缺陷,因此
    /// 这里补上,而不是假装计划草案已经穷尽了需要的变体（该草案本身
    /// 标注为「概念形状」，允许实现时按需微调，见其文档）。
    AdjustResource {
        /// 被调整的实体。
        actor: EntityId,
        /// 资源种类。
        resource: ResourceKind,
        /// 调整量，可正可负——施放消耗传负值，`RestoreResource` 效果
        /// 传正值，两个方向共用同一个变体。
        delta: i32,
    },
    /// 把某个技能的冷却设为 `until`（到期时刻，P5-B 任务 5）。
    ///
    /// 与 [`Effect::ScheduleNext`] 同一种「只写一个字段，不触碰任何
    /// 调度队列」的克制——这里只写
    /// [`ll_world::entity::Agent::skill_cooldowns`] 这一个条目。
    SetSkillCooldown {
        /// 被设置冷却的实体。
        actor: EntityId,
        /// 冷却的技能。
        skill: ContentIndex,
        /// 冷却到期的世界时刻。
        until: Tick,
    },
    /// 对某个实体施加一条临时属性修正（P5-B 任务 5；`source` 字段
    /// 为 `buffs-and-triggers.md` 六节「多来源叠加」新增）。
    ///
    /// # 只能是纯数值，不接装备（规格 §15 P6 边界）
    ///
    /// `attribute`/`delta` 是技能自身声明的静态数值，`apply` 落地时把
    /// 这一条 `(delta, expires_at)` 写进
    /// [`ll_world::entity::Agent::active_stat_modifiers`] 里
    /// `(attribute, source)` 对应的那个位置——不触发任何装备槽位读取，
    /// 也不触发完整的衍生属性重算（那属于 P6 装备落地之后的
    /// `derive_stats`，见 `crates/ll-mod/src/skill.rs` 模块文档「与规格
    /// §15 P6 边界的关系」一节，本变体延续同一条边界）。
    ///
    /// # `source`：施加者身份，不是「谁挨了这一下」
    ///
    /// 与 `target`（受影响的实体）是完全不同的两个字段——`source` 是
    /// 「这条修正来自哪份内容定义」（目前唯一的生产者是
    /// `resolve::resolve_use_skill`（模块私有，不对外可链接），传入被使用的技能自身的
    /// `ContentIndex`），供 `apply` 判断「这是不是同一个效果的重复
    /// 施加」：`(target 身上的 attribute, source)` 相同即视为同源，走
    /// `merge_same_source`；不同则各自独立存在、叠加生效。见
    /// `buffs-and-triggers.md` 六节①「身份是『效果来源』」。
    ///
    /// # 惰性到期，`apply` 不做「是否已过期」的判断
    ///
    /// 与 [`ll_world::entity::ActiveStatModifier`] 文档一致：是否已经
    /// 过期由未来读取「有效属性值」的调用方（`resolve::resolve_attack`
    /// ——模块私有，不对外可链接——经 `effective_attribute`）在读取那一刻现比对世界时钟，`apply`
    /// 本身不含这个判断（三条纪律「不含任何游戏逻辑」）——`apply` 唯一
    /// 要做的判断是「同源合并」，这是一个纯粹由两个既有值机械算出结果
    /// 的固定算法（[`ll_world::entity::ActiveStatModifier::merge_same_source`]），
    /// 不是需要读取更多世界状态才能决定的游戏规则判断，与「不含任何
    /// 游戏逻辑」这条纪律不冲突——`RefreshDuration` 曾经也是靠
    /// `BTreeMap::insert` 的覆盖语义在 `apply` 里免费实现，这里是同一
    /// 条先例的延续，只是覆盖语义换成了一个两行的合并函数。
    ApplyStatModifier {
        /// 受影响的实体。
        target: EntityId,
        /// 受影响的属性。
        attribute: AttributeKind,
        /// 增减量，可为负。
        delta: i32,
        /// 到期时刻。
        expires_at: Tick,
        /// 施加这条修正的来源——`(attribute, source)` 相同视为同一效果
        /// 的重复施加（同源刷新），不同则视为不同效果（异源叠加）。
        source: ContentIndex,
    },
    /// 把玩家探索记忆里、以 `origin` 为圆心、`radius` 为半径的可见格
    /// 标记为已探索（探索记忆写入路径批次）。
    ///
    /// # 为什么是「圆心 + 半径」，不是「一批坐标」
    ///
    /// 一次视野覆盖的格子有几百个（半径 12 时接近 `3*12*12≈432`
    /// 到 `4*12*12≈576` 格，见 `ll_world::fov` 模块「开阔地带的可见格数
    /// 接近圆面积」一节的同一个数量级估算）。若这里携带一个
    /// `Vec<TorusPos>`，等于每次移动都要在 `resolve`（本 crate 里成千
    /// 上万个 AI 未来要并行跑的那一层）里现跑一遍
    /// [`ll_world::fov::compute_fov`]、再把几百个坐标克隆进 `Effect`——
    /// 这份计算与这份内存对 `resolve` 的调用频率（每次移动一次）来说
    /// 是纯粹的浪费：`apply` 反正也要能读到世界地形（它本来就持有
    /// `&mut WorldState`），没有理由不让它自己算这几百格。
    ///
    /// 圆心+半径因此只是两个 `Copy` 字段（`TorusPos` + `u32`），完全
    /// 符合「`Effect` 只装朴素数据」（C2）——不比 `Effect::MoveTo`/
    /// `Effect::Damage` 复杂，`resolve_move`（本 crate `resolve.rs`）
    /// 发出这条 `Effect` 时甚至不需要跑一次 FOV，只需要知道玩家挪到了
    /// 哪、该用多大半径。
    ///
    /// # 为什么「apply 算出的集合与 resolve 看到的完全一致」在这个设计下
    /// 是自动成立的，不需要额外校验
    ///
    /// 这份「一致性」担忧的前提是 `resolve` 和 `apply` 各自独立算了一遍
    /// FOV、结果可能因为输入不同步而分岔。本设计从根上避免了这个分岔：
    /// **全过程只有 `apply` 一处真正调用
    /// [`ll_world::fov::compute_fov`]**——`resolve`（见 `resolve.rs` 的
    /// `resolve_move`）从不自己跑 FOV，只是把「谁挪到了哪、该用多大
    /// 半径」这两个朴素数字封进这条 `Effect`。`apply` 收到之后用与
    /// 渲染路径完全同一个函数
    /// （[`ll_world::surface_store::SurfaceWindow`] +
    /// `compute_fov`，与 demo `render_surface` 喂给渲染的是同一套
    /// 调用）在这一刻的地形上现算一次可见集合，即算即用，没有第二份
    /// 「记忆中的可见集合」需要对齐——单一计算点，天然不可能出现两份
    /// 计算结果不一致的问题。
    ///
    /// # 前置条件：`origin` 周围 `radius` 范围内的区块必须已经常驻
    ///
    /// `apply` 落地这条效果时用
    /// [`ll_world::surface_store::SurfaceWindow`] 包一层
    /// `WorldState::terrain` 喂给 `compute_fov`——`SurfaceWindow` 对未
    /// 常驻区块的查询会直接 panic（见其文档「前置条件」），这是既有的
    /// 既定纪律，不是本变体新引入的风险：`render_surface`
    /// （demo 渲染路径）已经承担着同一个前提，靠调用方在移动前调用
    /// [`ll_world::surface_store::SurfaceStore::stream_neighborhood`]
    /// 覆盖「视野半径 + 余量」的区块半径来满足它。这条效果复用同一套
    /// 既有前提，不需要为它单独发明一套新的容错。
    ///
    /// # 何时才触发：只在真的挪动了位置时
    ///
    /// 只有 `crate::resolve::resolve_move`（该函数是模块私有的,
    /// 不能在此处以可解析的文档链接引用）在产出 `Effect::MoveTo`
    /// 的同一分支才会追加一条本效果，且只对玩家自己（见该函数文档
    /// 「为什么只有玩家移动才追加」一节）。玩家原地等待、撞墙、被挡在
    /// 门前都不会走到那个分支，因此不会重复对同一批格子发起标记——这
    /// 就是避免「玩家站着不动时每帧仍把几百格重新标记一遍」的做法：
    /// 标记的触发时机绑定的是「位置真的变了」这个离散事件，不是渲染
    /// 帧率，两者频率天差地别（移动一次通常跨越好几帧）。
    MarkExplored {
        /// 视野圆心——即玩家刚刚挪到的位置。
        origin: TorusPos,
        /// 视野半径。
        radius: u32,
    },
    /// 给某实体授予 `amount` 点经验（等级与经验系统，
    /// `knowledge/design/level-and-experience-system.md` 六节）。
    ///
    /// # 只携带「给多少」，不携带「该不该升级」
    ///
    /// 与 [`Effect::Damage`] 只携带 `amount`（一个决定,不是最终状态）
    /// 同一个范式：本效果的产出者（`crate::resolve` 的
    /// `append_kill_experience`）只判断「这场击杀该给多少经验」，不
    /// 判断「加上这些经验后有没有升级、升几级」——那段判定没有任何
    /// 下游效果需要提前知道结果（不像「是否致死」需要提前算出来才能
    /// 同时产出 `Effect::Kill`），因此整段放进 `apply` 一次算完，见
    /// [`crate::apply::apply_with_xp_curves`] 文档。不需要独立的
    /// `Effect::LevelUp` 变体——升级是本效果在 `apply` 侧的一个自然
    /// 后果，不是一个需要单独被「决定」的独立效果，见设计文档六节
    /// 「被否决的选项」一节。
    GrantExperience {
        /// 获得经验的实体——通常是一场击杀的击杀者。
        target: EntityId,
        /// 经验量。设计上恒为非负（击杀不会倒扣经验），但本类型本身
        /// 不校验这条约束——校验是产出者（`resolve`）的职责，`apply`
        /// 侧的升级循环只要求 `xp_to_next_level > 0` 就不会因为一个
        /// 意外的负数而死循环，见 `apply` 侧实现注释。
        amount: i64,
    },
    /// 把某个实体的某个开放注册资源池当前值调整 `delta`（资源池落地
    /// 批次，第一批：法力池/血池，`knowledge/design/resource-pools-and-rest.md`
    /// 二节）——与 [`Effect::AdjustResource`] 是同一种「按增量调整某
    /// 资源当前值」的语义，区别只在于资源身份的来源：`AdjustResource`
    /// 走封闭的 [`crate::skill::ResourceKind`]，本变体走开放注册的
    /// `ContentIndex`（服务 `ResourceCost::PoolAmount`/技能自身的
    /// `RestoreResource`——本批次 `RestoreResource` 仍只支持
    /// `ResourceKind`，未来若需要对开放池也支持"技能效果里恢复"，是
    /// 该效果变体自身的扩展，不影响本变体）。
    AdjustResourcePool {
        /// 被调整的实体。
        actor: EntityId,
        /// 资源池索引（指向经 `register-resource-pool` 注册的
        /// `ResourcePoolDef`）。
        pool: ContentIndex,
        /// 调整量，可正可负——施放消耗传负值，`RegenRule::OnTurnStart`
        /// 传正值，两个方向共用同一个变体，与 `AdjustResource` 同一条
        /// 既有纪律。
        delta: i32,
    },
    /// 血代价：直接扣 `amount` 点 `Agent::health`，绕开减伤/抗性
    /// （`resource-pools-and-rest.md` 五节）。
    ///
    /// # 为什么不是 `Effect::Damage`（刻意，不是漏写）
    ///
    /// `Effect::Damage` 携带的 `amount` 是 `resolve_attack`/
    /// `resolve_use_skill` 已经跑完 `damage_after_defense`（固定减+
    /// 百分比减+10% 下限）算出来的**最终**数字——若血代价复用这条
    /// 路径，防御高的角色施法就会变得更便宜，这是规则错误。`apply`
    /// 响应本效果时无条件 `agent.health -= amount`，不查任何防御/抗性
    /// 表，也因此天然不会触发任何键在 `Effect::Damage` 上的触发器
    /// （例如"受伤反击"）——血代价是施法者自己选择付出的资源，语义上
    /// 是"消耗一种资源，恰好这种资源是生命值"，不是"被击中"。
    SpendBloodCost {
        /// 付出代价的实体——通常是施法者自己。
        target: EntityId,
        /// 代价量，非负。
        amount: i32,
    },
    /// 把某个实体的某个法术位池、某一档的已消耗数调整 `delta`
    /// （法术位落地批次，`resource-pools-and-rest.md` 二节）——与
    /// [`Effect::AdjustResourcePool`] 是同一种「按增量调整某个数」的
    /// 语义，区别只在于调整的是「已消耗数」不是「当前值」，方向也因此
    /// 相反：施放消耗传正值（多花了一个槽位），休息/回合开始的恢复
    /// 传负值（少了一些已消耗记录）。
    AdjustResourceSlot {
        /// 被调整的实体。
        actor: EntityId,
        /// 资源池索引（指向 `ResourcePoolShape::TieredSlots` 池）。
        pool: ContentIndex,
        /// 档位，1 起编号。
        tier: u8,
        /// 调整量，可正可负——`apply` 落地时钳位到非负（已消耗数不能
        /// 是负的），见其分支注释。
        delta: i32,
    },
    /// 开始一段休息会话（`resource-pools-and-rest.md` 七、八节）——
    /// `resolve` 收到 [`crate::intent::Intent::Rest`] 时，若发起者当前
    /// 未在休息，产出本效果 + 与 `Intent::Wait` 相同的
    /// [`Effect::ScheduleNext`]。
    BeginRest {
        /// 开始休息的实体。
        actor: EntityId,
        /// 目标持续的 tick 数。
        target_ticks: u32,
    },
    /// 结束一段休息会话——把 `resting` 清回 `None`。
    ///
    /// # 正常完成与中断共用同一个效果，区别在于它前面有没有恢复批次
    ///
    /// `resolve_wait` 判定「已到达 `target_ticks`」时，先追加恢复批次
    /// （`Effect::AdjustResourcePool`/`Effect::AdjustResourceSlot`），
    /// 再追加本效果；`resolve_dispatch` 判定「发起者正在休息、这次却
    /// 提交了非 `Wait`/`Rest` 意图」（中断）时，只追加本效果，不带任何
    /// 恢复——见 `resource-pools-and-rest.md` 八节「中断怎么表达」一节。
    /// 这正是防刷漏洞的主防线：恢复只在「正常完成」这一刻整批产出，从
    /// 不按已过时间比例给，反复「休息一回合、取消」不存在能刷出恢复的
    /// 代码路径。
    ClearResting {
        /// 结束休息的实体。
        actor: EntityId,
    },
}
