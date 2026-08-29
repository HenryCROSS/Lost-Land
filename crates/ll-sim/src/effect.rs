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
use ll_world::item::EquipSlot;
use ll_world::mod_state::ModStateWrite;
use ll_world::ownership::Owner;
use ll_world::space::Space;
use ll_world::terrain::TerrainKind;

use crate::item::ItemStack;
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
/// [`Effect::SetModState`] 携带一个 `Vec<ModStateWrite>`——`Vec`
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
    /// 把两个实体的位置对调。
    ///
    /// # 为什么是一条效果，不是两条 `MoveTo`
    ///
    /// 两条 `MoveTo` 会让世界在两次 `apply` 之间经过一个**两个实体站在
    /// 同一格**的中间状态，而那个中间状态是真的能被看见的：
    /// `crate::turn::TurnEngine::perform` 逐条 `apply`，每条之前都先调
    /// 一次 `on_effect`（呈现层观察点），这一批效果里若还夹着别的效果，
    /// 它们读到的也是那个中间状态。互换位置在规则上是**一次**世界变化，
    /// 不是「先把 A 挪过去，再把 B 挪回来」两次；ADR 0023 说的「对世界
    /// 状态的写入必须经 `apply` 产出 `Effect`」里，`Effect` 就是这个
    /// 「一次变化」的粒度，所以它是一条。
    ///
    /// 顺带的好处是它不可能被写成一半：一条效果要么整条应用，要么没有
    /// 应用，不存在「A 挪走了、B 没挪回来」这种把两个实体摞在一格上的
    /// 残留状态。
    ///
    /// `a`/`b` 谁在前不影响结果（对调是对称的），但 `resolve` 恒把
    /// **发起者**放在 `a`——这样这条效果自己就带着「谁主动走过去」的
    /// 信息，呈现层想为发起者播一个走路动画时不需要另外查。
    SwapPositions {
        /// 发起者（主动走过去的那个）。
        a: EntityId,
        /// 被换位的那个。
        b: EntityId,
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
    /// 批量写入 mod 状态（裁定 P5-1）。
    ///
    /// # 为什么 mod 状态的写入也要走 `Effect`
    ///
    /// mod 状态存在 `WorldState` 里（[`ll_world::entity::Agent::mod_state`]），
    /// 那就是世界状态的一部分：写它就是改世界，绕开 `Effect` 流意味着
    /// 「同一串 Intent 重放」复现不出这些数据，「mod 自己的数据」这个
    /// 类别在存档/重放的意义上并不真实存在。裁定 P5-1 选择约束 C1
    /// 「`apply` 是唯一写入口」赢：写入必须经这条唯一入口。
    ///
    /// # 为什么是一条 `Effect` 携带一批，而不是每次写入一条
    ///
    /// 一次决策期间可能连续产生多条写入（例如
    /// [`crate::quest::kill_progress_effects`] 同时写击杀计数与可能
    /// 因此达标的任务完成标记）——若每条都发一条独立 `Effect`，会为
    /// 每次写入多付一条 `Effect` 的开销。产出方把一次决策内的写入攒
    /// 成一批、包成一条本变体发出，交给既有的 `resolve → apply` 管线
    /// ——`Effect` 流因此保持「每一次状态变化都经过它」的诚实，又不必
    /// 为每次写入单独付一条 `Effect` 的开销。
    SetModState {
        /// 这条 `Effect` 携带的全部写入，保留产出时的原始顺序——同一个
        /// 键在批内被覆写多次时，`apply` 按顺序逐条写入，最终生效的是
        /// 最后一条。
        writes: Vec<ModStateWrite>,
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
    /// ——模块私有，不对外可链接——经 `resolve::derive_stats`）在读取那一刻现比对世界时钟，`apply`
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
    /// 从地面移除一堆物品（P6 第二批：背包与地面物品）——
    /// `crate::resolve::resolve_pick_up` 唯一的产出者，与
    /// [`Effect::MergeIntoInventory`] 成对出现（先移除地面上的，再写进
    /// 背包）。
    ///
    /// # 为什么按 `(pos, def)` 定位，不是索引
    ///
    /// `resolve` 只有 `&WorldState`（C1），拿不到
    /// [`ll_world::state::WorldState::ground_items`] 的 `&mut`，无法把
    /// "移除哪一条"这件事本身预先做掉再产出效果——但 `apply` 也不能
    /// 反过来自己判断"该移除哪一堆"（那是规则判断，`apply` 不含任何
    /// 游戏逻辑）。折中是让 `resolve` 把它已经读到的坐标与物品定义
    /// 原样写进本效果，`apply` 只做一次按键查找+移除（机械执行，不是
    /// 判断该不该移除），这与 `Effect::SetSkillCooldown`
    /// 按 `(actor, skill)` 这一对键写入 `BTreeMap` 是同一类"用内容
    /// 索引定位，不用容器内部下标"的既有做法——`Vec` 下标会在同一批
    /// 效果里其它条目改动 `ground_items` 后失效，内容索引不会。
    RemoveGroundItem {
        /// 地面物品所在的位置。
        pos: TorusPos,
        /// 要移除的物品定义——本批次每次移除脚下匹配到的第一条整堆
        /// （不支持部分数量,见 [`crate::intent::Intent::Drop`] 文档
        /// 「为什么是整堆」一节同一条范围裁定）。
        def: ContentIndex,
    },
    /// 在地面上新增一堆物品——`crate::resolve::resolve_drop`（普通丢弃，
    /// `contents` 恒空）与 `crate::resolve::append_corpse_drop`（NPC
    /// 死亡掉落批次：死者变成一具装着战利品的尸体，`contents` 是死者
    /// 结算前的背包+装备）共用的产出者。
    ///
    /// # 为什么复用同一个变体，不给尸体单开一个 `Effect`
    ///
    /// 尸体在数据形状上就是"一件带 `contents` 的地面物品"（见
    /// [`ll_world::item::GroundItemStack::contents`] 文档「为什么用
    /// `contents` 是否非空作判据」一节）——`apply` 侧要做的机械操作
    /// （往 `world.ground_items` 追加一条）与普通丢弃完全相同，只是
    /// 多带一份内容物,不需要为同一个机械操作开两条 `Effect` 变体。
    AddGroundItem {
        /// 放置位置——通常是丢弃者/死者当前所在坐标。
        pos: TorusPos,
        /// 具体是哪一堆物品（普通丢弃）或容器本身这件"物品"的壳
        /// （尸体），数量/耐久均已由 `resolve` 决定。
        stack: ItemStack,
        /// 丢弃/生成时刻——`WorldState::cleanup_aged_ground_items` 的
        /// 老化判定依据，见其文档；尸体与内容物共用同一个时刻，作为
        /// 一个整体老化。
        dropped_at: Tick,
        /// 容器内容物——普通丢弃恒为空 `Vec`，尸体是死者结算前的
        /// `inventory` + `equipment` 全部物品，见
        /// [`ll_world::item::GroundItemStack::contents`] 文档。
        contents: Vec<ItemStack>,
        /// 这一堆是**立起来**的还是**躺着**的，见
        /// [`ll_world::item::GroundItemStack::placed`] 文档。
        ///
        /// 只有 `crate::resolve` 的 `resolve_place`（`Intent::Place`）
        /// 产出 `true`；普通丢弃、尸体、盲盒溢出等其余产出点全是
        /// `false`。**做成本变体的一个字段，不是新开一个
        /// `Effect::PlaceGroundItem` 变体**：`apply` 侧要做的机械操作
        /// 逐字相同（往 `world.ground_items` 追加一条），差别只在追加
        /// 的那条数据上的一个位——与尸体复用同一个变体是同一条理由，
        /// 见本变体文档「为什么复用同一个变体」一节。
        placed: bool,
    },
    /// 把物品写进某实体的背包，可能同时替换掉背包里已有的同种可堆叠
    /// 堆（`crate::resolve::resolve_pick_up` 的产出者）。
    ///
    /// # 为什么合并结果由 `resolve` 算好，`apply` 只做替换
    ///
    /// 是否要合并（`can_merge`）、合并后主堆/溢出堆各自多少
    /// （`merge_stacks`，需要查 [`crate::item::ItemCatalog`] 拿堆叠
    /// 上限）——这些全部是规则判断，必须在 `resolve` 做完；`apply` 拿
    /// 到的 `resulting` 已经是最终要写进背包的完整数据，不再做任何
    /// 算术,只做"找到旧堆位置并替换成新的这一批"这个纯粹的容器操作，
    /// 与 [`Effect::ApplyStatModifier`] 「apply 只管照单据执行」是同一
    /// 条纪律。
    MergeIntoInventory {
        /// 背包的持有者。
        actor: EntityId,
        /// 若与背包已有的一堆合并了，这里给出旧堆的 `(def, durability)`
        /// 用于原地定位并移除——`can_merge` 的判据正是这两个字段（见
        /// 其文档），`resolve` 已经用它确认过这堆确实存在。`None`
        /// 表示没有可合并的旧堆，本效果单纯往背包里追加。
        replaced: Option<(ContentIndex, Option<i32>)>,
        /// 合并/追加后要写进背包的结果——没有溢出时一条,有溢出（触及
        /// 堆叠上限）时两条。
        resulting: Vec<ItemStack>,
    },
    /// 从某实体背包移除一整堆匹配的物品——`crate::resolve::resolve_drop`
    /// 的产出者，与 [`Effect::AddGroundItem`] 成对出现。
    ///
    /// # 为什么按 `(def, durability)` 定位，不是索引
    ///
    /// 与 [`Effect::RemoveGroundItem`] 同一条理由：`resolve` 只有共享
    /// 引用，无法预先从背包里摘掉这一堆再产出效果，只能把已经读到的
    /// 定位信息原样写进本效果，交给 `apply` 做一次按键查找+移除。
    RemoveFromInventory {
        /// 背包的持有者。
        actor: EntityId,
        /// 要移除的物品定义。
        def: ContentIndex,
        /// 要移除的那一堆的耐久——与 `def` 一起构成 `can_merge` 判据，
        /// 保证移除的是 `resolve` 实际读到的那一堆，不是"随便一堆同
        /// `def` 的"。
        durability: Option<i32>,
    },
    /// 把某人背包里一堆物品的归属改成别人的（归属批次）——设计文档
    /// 四节「合法转移」的接口形状。
    ///
    /// # 一个变体就够，不是三个
    ///
    /// 设计文档四节原文：赠送、购买、任务发放物品在「改变 `Owner`」
    /// 这个动作本身上**完全同构**（都是把一堆物品的 `owner` 从 A 改成
    /// B），区别只在"谁触发的、有没有对价"——那部分逻辑属于各自系统
    /// （交易的价格结算、任务的完成判定），不属于归属转移本身。
    ///
    /// # 调用方今天一个都不存在，如实标注
    ///
    /// 没有交易系统（无价格结算、无货币扣减接线）、没有对话系统
    /// （赠送需要一个"NPC 决定要不要给你"的交互载体）、没有任务奖励
    /// 发放的 `resolve`（`QuestNodeDef` 已落地类型定义，发放机制未
    /// 落地）。设计文档把三者如实标注为「空中楼阁，接口形状可以先
    /// 给」——本变体就是那个形状，**没有对应的 `Intent`**，产出者要等
    /// 那三个系统各自落地。
    ///
    /// # 给未来三个调用方的一条硬前置
    ///
    /// 设计文档四节末尾（并在三节 3.3 末尾预告过）：三种合法转移的
    /// `resolve` 都**必须**校验「发起转移的一方确实是这堆物品当前的
    /// `owner`」（[`Owner::Unowned`](ll_world::ownership::Owner::Unowned)
    /// 的物品谁都能转移，因为没有人的权益受损）。不满足则这次转移本身
    /// 不合法，不该产出本效果。
    ///
    /// **`apply` 侧不做这条校验**——它是决策，属 `resolve`（约束 C1）。
    /// 少了它，销赃计时会被一条作弊路径绕开：小偷把赃物"卖"给自己控制
    /// 的另一个角色，标记瞬间清空。这条写在这里，是为了让那三个系统
    /// 落地时读得到。
    ///
    /// # 为什么用 `(holder, def, durability)` 三元组定位
    ///
    /// 设计文档四节给的形状是 `{ stack_def, new_owner }` 两个字段——
    /// **本变体比它多两个**，因为只给 `def` 定位不到"具体是谁背包里的
    /// 哪一堆"，而 `apply` 是全局唯一写入口（约束 C1），它必须能唯一
    /// 落到一堆上。这个三元组正是
    /// [`Effect::RemoveFromInventory`]/[`Effect::ConsumeInventoryItem`]
    /// 已经在用的定位方式，照抄既有惯例，不新发明一套。
    TransferOwnership {
        /// 这堆物品现在在谁的背包里。
        holder: EntityId,
        /// 哪一种物品。
        def: ContentIndex,
        /// 那一堆的耐久——与 `def` 一起唯一定位，见本变体文档。
        durability: Option<i32>,
        /// 转移之后归谁。
        new_owner: Owner,
    },
    /// 把物品堆装进某个槽位（装备栏位批次，P6 第三批）——
    /// `crate::resolve::resolve_equip` 唯一的产出者。`slot` 是这件
    /// 物品的**锚点槽位**（`crate::item::SlotMask::anchor_slot`，掩码
    /// 最低位），不是玩家发起 `Intent::Equip` 时提供的任何字段——
    /// `Intent::Equip` 根本不携带槽位（见其文档「为什么携带 `def`，不
    /// 携带目标槽位」一节），锚点完全由 `resolve_equip` 查表算出。
    /// 横跨多槽的物品（双手武器占 `MAIN_HAND`+`OFF_HAND`）只存一份，
    /// 见 [`ll_world::entity::Agent::equipment`] 文档「为什么以锚点
    /// 槽位为键」一节。
    ///
    /// # 为什么 `apply` 不检查槽位是否已被占用
    ///
    /// `resolve_equip` 保证在产出本效果之前，同一批效果里已经先产出了
    /// 覆盖全部冲突槽位的 [`Effect::Unequip`]（`crate::resolve` 模块
    /// 「占位冲突」一节），`apply` 按顺序依次执行这批效果，执行到本
    /// 效果时冲突槽位理应已经清空——这是"决策在 resolve，apply 只管
    /// 照单据执行"（约束 C1）的又一处体现，`apply` 无条件覆盖写入，不
    /// 重新判断"这个槽位现在是不是空的"。
    Equip {
        /// 装备的持有者。
        actor: EntityId,
        /// 锚点槽位。
        slot: EquipSlot,
        /// 具体是哪一堆物品。
        stack: ItemStack,
    },
    /// 从某个槽位卸下当前装备的物品（装备栏位批次，P6 第三批）——
    /// `crate::resolve::resolve_equip`（因占位冲突而卸下）与
    /// `crate::resolve::resolve_unequip`（玩家主动卸下）共用的产出者。
    ///
    /// `slot` 是**查找到的真实存储键**（锚点槽位），不是玩家在
    /// `Intent::Unequip` 里提供的原始请求槽位——`resolve_unequip` 会把
    /// 请求槽位翻译成真实存储键再产出本效果，见其文档「为什么要把
    /// 请求槽位翻译成锚点槽位」一节。`apply` 响应本效果时只做
    /// `agent.equipment.remove(&slot)` 这一步机械操作，不负责把卸下的
    /// 物品放回背包——那是同一批效果里紧随其后的
    /// [`Effect::MergeIntoInventory`] 的职责（`resolve_equip`/
    /// `resolve_unequip` 都遵循"先卸下、再合并回背包"的产出顺序）。
    Unequip {
        /// 装备的持有者。
        actor: EntityId,
        /// 要清空的槽位（真实存储键）。
        slot: EquipSlot,
    },
    /// 从某实体背包消耗一件物品——数量减一，减到零时整条堆从背包移除
    /// （耐久与 `Intent::Use` 落地批次，P6 第五批）——
    /// `crate::resolve::resolve_use_item` 唯一的产出者。
    ///
    /// # 为什么按 `(def, durability)` 定位，不是索引
    ///
    /// 与 [`Effect::RemoveFromInventory`]/[`Effect::RemoveGroundItem`]
    /// 同一条既有理由：`resolve` 只有共享引用，无法预先从背包里摘掉
    /// 这一堆再产出效果，只能把已经读到的定位信息原样写进本效果，交给
    /// `apply` 做一次按键查找+扣减。
    ///
    /// # 为什么恒扣一，不带 `amount` 字段
    ///
    /// `Intent::Use` 本身只表达「用掉一件」（见其文档），与
    /// `Intent::Drop`「不支持部分数量」是同一条范围裁定——一次性用掉
    /// 一整堆药水不是本批次要支持的手感，真要支持「一次用 N 个」，应
    /// 该是调用方连续提交 N 次 `Intent::Use`，不是给这条效果加一个
    /// 目前没有任何调用点会填非一值的字段。
    ConsumeInventoryItem {
        /// 背包的持有者。
        actor: EntityId,
        /// 被消耗的物品定义。
        def: ContentIndex,
        /// 被消耗的那一堆的耐久——与 `def` 一起构成定位判据，理由同
        /// [`Effect::RemoveFromInventory::durability`]。
        durability: Option<i32>,
    },
    /// 调整某个已装备物品的当前耐久（耐久与 `Intent::Use` 落地批次，
    /// P6 第五批；耐久扩面批次改写了产出规则并新增第二个产出者）。
    ///
    /// 两个产出者，三条产出规则：
    ///
    /// - `crate::resolve::resolve_attack`「使用」通道——攻击方主手
    ///   已装备的武器（若带耐久）每打出一下损失一点；
    /// - `crate::resolve::resolve_attack`「挨打」通道——防御方每一件
    ///   落在**非武器槽位**、且带耐久的已装备物品（护甲/衣物）各损失
    ///   一点；
    /// - `crate::resolve::resolve_craft`——制作真的发生时，配方点名的
    ///   那件工具（若带耐久）损失一点。
    ///
    /// 三条的完整论证分别见 `resolve_attack` 文档「耐久消耗：两条通道」
    /// 与 `resolve_craft` 文档「工具磨损」两节。
    ///
    /// # 为什么钳位到非负在 `apply` 做，不在 `resolve` 做
    ///
    /// 与 [`Effect::AdjustResourceSlot`] 「已消耗数不能是负的，钳位在
    /// apply 侧做」同一条既有先例——`resolve` 只需要知道"这一下要扣多少"
    /// 这个恒定的常量，不需要读取当前耐久才能决定扣多少（不像
    /// `ResourceCost::PoolAmount` 那样需要先查容量才能判断扣多少），
    /// 钳位是一个纯粹由当前值与固定增量机械算出结果的操作，符合
    /// 「apply 不含任何游戏逻辑」纪律里"机械执行"这一类允许的操作
    /// （与 `ApplyStatModifier` 的同源合并、`AdjustResourceSlot` 的非负
    /// 钳位是同一类"两个数就能算完，不需要额外读取世界状态"的机械
    /// 操作）。
    AdjustEquipmentDurability {
        /// 装备的持有者。
        actor: EntityId,
        /// 要调整的槽位（真实存储键——多槽物品的耐久只存一份，与
        /// `Effect::Equip`/`Effect::Unequip` 同一条既有约束）。
        slot: EquipSlot,
        /// 调整量，恒为负（本批次唯一的产出者只会扣减，见本变体文档）
        /// ——形状上仍允许正值，供未来修理系统复用同一条效果,不必再
        /// 新开一个变体。
        delta: i32,
    },
    /// 卫兵盘查（卫兵职业接线批次）：`inspector` 检查了 `target` 此刻
    /// 背包与已装备的全部物品，`items_seen` 是那一刻的完整快照——
    /// `crate::resolve::resolve_inspect` 唯一的产出者。
    ///
    /// # 为什么 `apply` 不把它写进 `WorldState::history`
    ///
    /// `HistoricalEventKind`（`ll_world::history`）目前只有 `Kill` 一个
    /// 变体，是"值得永久记住的例外"（击杀是稀有、重大的事件）——盘查
    /// 不是：卫兵按行为树的既定概率随时可能发起盘查，若每次都落一条
    /// 永久历史事件，`history` 会随卫兵数量/游戏时长线性无界增长，却
    /// 没有任何下游系统会去读"第 10000 次盘查、什么都没查到"这种记录
    /// （见 `knowledge/design/ownership-and-crime-detection.md`
    /// 「五节 5.1」`BattleLog` 否决先例同一类顾虑）。因此本效果**刻意**
    /// 不在 `apply` 里追加任何 `WorldState` 写入——它的可观察点就是
    /// `resolve` 产出的这一刻本身：调用方（测试、未来的日志/UI 系统）
    /// 直接消费 `resolve`/`resolve_ai_turn` 返回的 `Vec<Effect>` 里的
    /// 这一条，不需要等它落进任何持久存储才能确认"盘查真的发生过、
    /// 看到了什么"。
    ///
    /// # 为什么没有任何"是否违法"的判断
    ///
    /// `Owner`/`stolen_marker` 尚未落地（同上文档，纯设计）——"这堆
    /// 东西是不是 `target` 自己的"这个问题本批次回答不了，本效果因此
    /// 只如实记录"看到了什么"，不产出任何裁定。等 `Owner` 落地后，
    /// 消费本效果的下游逻辑才谈得上比对 `items_seen` 与各堆的
    /// `owner`、决定要不要转成一条 `HistoricalEventKind::Crime`——那是
    /// 本效果预留的挂载点，不是本批次要交付的部分。
    ///
    /// 「逐堆比对」这句话原先在本效果的形状上**落不了地**：
    /// `items_seen` 曾经是 `Vec<ContentIndex>`，只记种类，两堆同种物品
    /// 完全无法区分。槽位句柄批次把元素换成了 [`InspectedItem`]（种类 +
    /// 位置），挂载点这才真的挂得上——但本效果仍然**只记位置，不记
    /// 归属**，理由见 [`InspectedItem`] 文档「为什么不直接带 `Owner`」
    /// 一节。
    Inspect {
        /// 发起盘查的一方（卫兵）。
        inspector: EntityId,
        /// 被盘查的一方。
        target: EntityId,
        /// 盘查那一刻 `target` 背包（原始顺序）与已装备物品（按
        /// `EquipSlot` 顺序，`BTreeMap` 天然有序，不违反 C5）的**逐堆**
        /// 快照，先背包后装备。每条记录带着「是什么」与「在哪」两半，
        /// 见 [`InspectedItem`] 文档「为什么不是裸 `ContentIndex`」一节。
        items_seen: Vec<InspectedItem>,
    },
    /// 把一个实体的潜行状态设成一个**明确的值**（潜行与盗贼被动
    /// 批次）——[`ll_world::entity::Agent::stealthed`] 唯一的写入口
    /// （约束 C1）。
    ///
    /// # 为什么携带目标值，而不是「取反」
    ///
    /// 与 [`crate::intent::Intent::ToggleStealth`] 恰好相反的取舍，
    /// 两者不矛盾：`Intent` 是「玩家想干什么」的裸请求，那一层不该
    /// 读世界（不知道当前是开是关，因此只能说「切换」）；`Effect` 是
    /// 已经结算完的**确定结果**，`apply` 必须是可以脱离上下文重放的
    /// 纯赋值（ADR 0023/约束 C1：`apply` 不做规则判断）。一条「取反」
    /// 效果的结果依赖它被应用时的世界状态，同一条效果重放两次会得到
    /// 相反的结果——那正是效果日志/回放要防的东西。
    ///
    /// 两个真实生产者：
    /// - `crate::resolve::resolve_toggle_stealth`——读一次当前状态，
    ///   产出取反后的确定值。
    /// - `crate::resolve::resolve_attack`——攻击者正在潜行时产出
    ///   `stealthed: false`（攻击破除潜行，见该函数文档「潜行破除」
    ///   一节）。
    SetStealth {
        /// 被改写状态的实体。
        actor: EntityId,
        /// 这一刻之后它的潜行状态。
        stealthed: bool,
    },
    /// 花掉一点未分配属性点，把指定的那一项主属性加一（升级加点
    /// 批次）——[`crate::intent::Intent::AllocateAttributePoint`] 的
    /// 结算产物。
    ///
    /// # 为什么扣点与加属性是同一条效果，不是两条
    ///
    /// 两者必须原子成对：只应用其中一条的世界是「扣了点没加属性」或
    /// 者「凭空加了属性」，两种都是数据损坏。拆成两条效果就得靠调用
    /// 方永远记得同时产出、`apply` 永远按顺序应用来维持这条不变式，
    /// 而效果列表本身不提供任何这类保证。同一条效果里做两个字段的
    /// 写入，与 [`Effect::Kill`] 一条效果同时销毁实体并清理其残留是
    /// 同一个范式：原子性由「它就是一条效果」这件事本身保证。
    ///
    /// # `apply` 不做任何判断
    ///
    /// 余额够不够、加完会不会越过
    /// [`ll_world::entity::BaseStats::HARD_CAP`]，全部在 `resolve`
    /// 侧判完（不满足就一条效果都不产出）——`apply` 收到这条效果时
    /// 就是无条件的「减一、加一」，与约束 C1/ADR 0023 一致。
    AllocateAttributePoint {
        /// 加点的实体。
        actor: EntityId,
        /// 加到哪一项。
        attribute: AttributeKind,
    },
    /// 花掉一点未分配技能点，把一个技能加进已解锁集合（升级加点
    /// 批次）——[`crate::intent::Intent::LearnSkill`] 的结算产物，也是
    /// [`ll_world::entity::Agent::unlocked_skills`] 在本仓库里的**第一
    /// 个**写入口（此前它只有读取者：`resolve_use_skill` 的解锁闸门
    /// 与 [`crate::skill_overview`] 的技能树视图）。
    ///
    /// 扣点与解锁同为一条效果，理由同
    /// [`Effect::AllocateAttributePoint`]。余额、重复学习、前置未满足
    /// 三道闸门全部在 `resolve` 侧判完。
    LearnSkill {
        /// 学会技能的实体。
        actor: EntityId,
        /// 学会了哪个技能。
        skill: ContentIndex,
    },
    /// 把一个副职加进 [`ll_world::entity::Agent::subclasses`]（副职获得
    /// 机制批次）——这是该字段在本仓库里的**第一个**写入口。此前它只有
    /// 读取者（`crate::resolve::resolve_craft` 的副职闸门）与存档重映射
    /// （`ll_content::remap::remap_subclasses`），没有任何写入路径，于是
    /// `RecipeCategoryDef::required_subclasses` 声明的闸门等价于「谁都
    /// 过不去」。
    ///
    /// # 为什么不能塞进 [`Effect::SetModState`]（ADR 0023）
    ///
    /// `Agent::subclasses` 属 `WorldState`，不属 `Agent::mod_state`
    /// ——[`Effect::SetModState`] 只写后者那张表。ADR 0023 要求脚本
    /// 状态写入必须经 `apply`，同一条纪律在这里的推论是：世界状态的这个
    /// 字段需要**自己的**效果变体，见 `crate::subclass` 模块文档「为什么
    /// 授予必须是独立的 Effect 变体」一节。
    ///
    /// # 两道闸门在产出侧，发点那一步在 `apply` 侧
    ///
    /// 去重（已持有就不再加一份）与上限（[`crate::subclass::MAX_SUBCLASSES`]）
    /// 两道闸门全部在产出侧判完（[`crate::subclass::grant_subclass_effects`]
    /// 与 [`crate::subclass::craft_progress_effects`]，不满足就一条效果都
    /// 不产出）——`apply` 收到这条效果时对 `Agent::subclasses` 就是无条件
    /// 的 `push`，与约束 C1 / ADR 0023 一致。**这与设计文档四节「两件事
    /// 都放在 `apply` 里」那句相反**：那句写在
    /// `resolve_allocate_attribute_point` 落地之前，本仓库此后已经用
    /// 「闸门在 `resolve`、`apply` 无条件执行」的形状把同一类问题解决了
    /// 两次（加点、学技能），副职没有理由成为第三种写法。
    ///
    /// **副职发点批次给这条效果追加了第二个后果，它有一个判断，而且那个
    /// 判断刻意留在 `apply`**：第一次获得某个副职时还要发一批属性点/技能
    /// 点（[`crate::subclass::SUBCLASS_ATTRIBUTE_POINTS`]/
    /// [`crate::subclass::SUBCLASS_SKILL_POINTS`]），「是不是第一次」查的
    /// 是 [`ll_world::entity::Agent::subclasses_ever_granted`] 这份只增不
    /// 减的账本。它不放在产出侧的完整理由见 `crate::apply` 的
    /// `grant_first_time_subclass_points` 文档，一句话是：这条效果有三条
    /// 预定的产出路径，把「该不该发点」做成效果字段等于要求每一条都记得
    /// 算对它，而放在 `apply` 里则「账本里有它 ⟺ 已经发过点」这条不变式
    /// 由一段原子代码独自维护。同文件的 [`Effect::GrantExperience`] 早就
    /// 是同一种形状（升了几级、发几点全由 `apply` 自己算）。
    GrantSubclass {
        /// 获得副职的实体。
        actor: EntityId,
        /// 获得了哪个副职，指向副职表。
        subclass: ContentIndex,
    },
    /// 把一个副职从 [`ll_world::entity::Agent::subclasses`] 里移除——
    /// 与 [`Effect::GrantSubclass`] 成对（`subclass-system.md` 五节）。
    ///
    /// # 为什么上限存在就必须有它
    ///
    /// 没有放弃路径的话，玩家攒满 [`crate::subclass::MAX_SUBCLASSES`]
    /// 之后系统就锁死了，此后再也体验不到任何新副职带来的搭配变化——
    /// 上限想要的是「取舍」，不是「先到先得然后结束」。
    ///
    /// # 放弃**不**追溯已学会的技能，但制作闸门立刻失效
    ///
    /// 两种闸门的语义本来就不同，这不是缺陷：
    ///
    /// - `SkillRequirement` 闸的是「获得一个永久能力」，只在写进
    ///   `Agent::unlocked_skills` **之前**判一次；已经学会的技能此后与
    ///   是否仍满足条件无关（`skill-learn-requirements.md` 五节）。
    /// - `RecipeCategoryDef::required_subclasses` 闸的是「执行一次动
    ///   作」，`crate::resolve::resolve_craft` 第③步**每次制作都重新
    ///   判一遍**。因此放弃工匠之后，工匠类别的配方**立刻**做不了了。
    ///
    /// 这同时给了放弃机制一个真实代价，不需要额外设计任何惩罚数值。
    RemoveSubclass {
        /// 放弃副职的实体。
        actor: EntityId,
        /// 放弃了哪个副职。
        subclass: ContentIndex,
    },
    /// 把一条配方加进 [`ll_world::entity::Agent::known_recipes`]（配方
    /// 发现批次）——项目所有者裁定「菜谱就是通过随机丢入东西煮获取或者
    /// 阅读书籍的时候获取」在效果层的唯一落点。
    ///
    /// # 两条产出路径，一个效果变体
    ///
    /// `crate::resolve::resolve_read`（读一本书）与
    /// `crate::resolve::resolve_experiment`（拿手上的材料试做）产出的
    /// 是同一条效果——ADR 0021 的判据在这里成立且方向明确：两条路径**共
    /// 享的是「把这条索引写进这个角色的已知配方」这段完整算法**，差别
    /// 全在「怎么选出这条索引」，而那一步留在各自的 `resolve` 里。给两
    /// 条路径各造一个只有名字不同的效果变体，会逼 `apply` 为逐字相同的
    /// 一段 `push` 写两遍。
    ///
    /// # 为什么不能塞进 [`Effect::SetModState`]（ADR 0023）
    ///
    /// 与 [`Effect::GrantSubclass`] 逐字同理：`Agent::known_recipes` 属
    /// `WorldState`，不属 `Agent::mod_state`，因此需要自己的效果变体。
    ///
    /// # 为什么不复用 [`Effect::LearnSkill`]
    ///
    /// 目标字段不同（`known_recipes` vs `unlocked_skills`），且
    /// `LearnSkill` 在 `apply` 里**还要扣一点技能点**
    /// （`unspent_skill_points`），而学会一条配方不花费任何点数——复用
    /// 会让 `apply` 那一条分支多一个「这次要不要扣点」的判断，把两件
    /// 语义无关的事绑在一起。见
    /// `knowledge/design/food-and-cooking-system.md` 五节「复用
    /// `unlocked_skills` 是一次静默的概念污染」的同一条论证。
    ///
    /// # `apply` 不做任何判断
    ///
    /// 去重（已经知道就不再加一份）在产出侧判完——两个 `resolve` 都先
    /// 过滤掉 `agent.known_recipes.contains(...)` 的条目才产出效果，不
    /// 满足就一条效果都不产出。`apply` 收到这条效果时是无条件的
    /// `push`，与 [`Effect::GrantSubclass`]/[`Effect::LearnSkill`] 同一
    /// 条「闸门在 `resolve`、`apply` 无条件执行」的既有形状。
    LearnRecipe {
        /// 学会配方的实体。
        actor: EntityId,
        /// 学会了哪条配方，指向配方表。
        recipe: ContentIndex,
    },
    /// 把一个物品**种类**加进 [`ll_world::entity::Agent::identified_items`]
    /// （未鉴定物品批次）——项目所有者裁定「通过鉴定获取属性和说明」
    /// 在效果层唯一的落点。
    ///
    /// # 为什么不复用 [`Effect::LearnRecipe`]
    ///
    /// 目标字段不同（`identified_items` vs `known_recipes`），且两个
    /// `ContentIndex` 指向的是**不同的表**（物品表 vs 配方表）。复用会
    /// 让存档重映射那一侧彻底失去判断依据：`ll_content::remap` 只有一个
    /// 索引、没有类型标签，无从知道该按 `ContentKind::Item` 还是
    /// `ContentKind::Recipe` 去解析——这正是
    /// [`ll_world::entity::Agent::known_recipes`] 文档「复用
    /// `unlocked_skills` 是一次静默的概念污染」逐字相同的那条论证，
    /// 换个方向再次成立。
    ///
    /// # `apply` 不做任何判断
    ///
    /// 与 [`Effect::LearnRecipe`] 同一条形状：去重（已经认识就不再加
    /// 一份）在产出侧判完——`crate::resolve::resolve_identify` 先过滤掉
    /// `agent.identified_items.contains(...)` 才产出效果，不满足就一条
    /// 效果都不产出。`apply` 收到这条效果时是无条件的 `push`。
    ///
    /// # 盲盒不走这条效果
    ///
    /// 开盲盒**不**把盒子写进已鉴定集合（盒子被消耗了，「认识一种已经
    /// 不在世上的东西」没有意义），它产出的是
    /// [`Effect::ConsumeInventoryItem`] + [`Effect::MergeIntoInventory`] +
    /// [`Effect::GrantExperience`] 三条，见 `resolve_identify` 文档。
    IdentifyItem {
        /// 完成鉴定的实体。
        actor: EntityId,
        /// 认出了哪一**种**物品，指向物品表。
        def: ContentIndex,
    },
}

/// 「这一堆物品此刻放在这个实体身上的哪里」——[`InspectedItem`] 用来
/// 逐堆定位的槽位句柄，`item-system.md` 四节 `ItemLocation` 里两个
/// **随身**变体（`Inventory { holder, slot }` / `Equipped { holder, slot }`）
/// 在引擎侧的落地。
///
/// # 为什么不带 `holder`
///
/// 设计文档的两个变体都携带 `holder: EntityId`，本类型刻意不带——
/// 它只出现在已经点名了持有者的上下文里（[`Effect::Inspect::target`]），
/// 再记一份就是一个必须手动维持一致的冗余字段。与
/// [`ll_world::entity::Agent::inventory`] 文档「`holder` 就是这个
/// `Agent` 自身，因此不需要在 `ItemStack` 上重复记一份」是同一条既有
/// 判断。
///
/// # 为什么不是完整的 `ItemLocation`
///
/// 设计文档那个枚举还有 `Ground`/`Container` 两个变体。`Ground` 在
/// 本仓库已经有自己的存储形状（[`ll_world::item::GroundItemStack`]，
/// 见其文档「为什么不是完整的 `ItemLocation` 枚举」一节），`Container`
/// 尚未落地——把它们塞进来只会造出两个当前没有任何消费者的死变体，
/// 与那一节同一条 YAGNI 判断。本类型只回答「随身携带的东西在哪」这
/// 一个问题，名字也照这个范围取。
///
/// # 装备那一半沿用「真实存储键」，不发明新东西
///
/// [`CarriedItemSlot::Equipped`] 携带的是
/// [`ll_world::entity::Agent::equipment`] 的**存储键**（锚点槽位），
/// 与 [`Effect::Equip`]/[`Effect::Unequip`]/[`Effect::AdjustEquipmentDurability`]
/// 三条既有效果定位「哪一件装备」用的完全是同一个概念（见后者文档
/// 「要调整的槽位（真实存储键）」一句）：横跨多槽的双手武器只存一份，
/// 键取 `equip_mask` 的最低位。这一半是纯粹的沿用。
///
/// # 背包那一半为什么是下标，不是既有效果用的 `(def, durability)`
///
/// [`Effect::RemoveFromInventory`]/[`Effect::ConsumeInventoryItem`] 定位
/// 背包里的一堆用的是 `(def, durability)`——那对**它们**是够用的（它们
/// 要的是「移除一堆匹配的」，`resolve` 已经确认过存在），但对本类型
/// 要回答的问题**不够**：它必须能把两堆分开。
///
/// `(def, durability)` 不是背包的唯一键，这一点有代码为证：
/// [`ll_world::item::merge_stacks`] 在触及堆叠上限时产出的溢出堆是
/// `ItemStack { count: total - stack_limit, ..b }`——`def` 与
/// `durability` 与主堆**逐字相同**，只是数量不同。一支满堆的箭 + 一堆
/// 溢出的箭因此是两条 `(def, durability)` 完全一致的记录。归属比对
/// 拿到这样一对，判不了「哪一堆是偷的」。下标天然唯一，不依赖任何
/// 合并不变式。
///
/// # 这是快照期的位置，不是跨回合的持久句柄
///
/// 下标只在**产出这条效果的那一刻**有效——背包是
/// [`Vec`]，任何一次移除都会让它后面的下标整体前移。这不构成问题：
/// [`Effect::Inspect`] 是「盘查那一刻看到了什么」的快照，消费者
/// （当前是测试与未来的 `Owner` 比对）在同一批效果里就把它读完，
/// 不存在把这个句柄存起来隔几个回合再用的路径。真要有那种需求，需要
/// 的是一个稳定实例 id，不是位置——那是另一个批次的问题。
///
/// # 为什么下标是 `u32` 而不是设计文档写的 `u16`
///
/// `u16` 的上限（65535）不是物理不可达的：带耐久的物品按耐久值分堆
/// （[`ll_world::item::can_merge`] 的判据含 `durability`），一件耐久
/// 上限四位数的装备就能贡献上千堆。一个静默截断的下标会把两堆指成
/// 同一堆，正是本类型要消灭的那种歧义。`u32` 的上限在
/// `Vec<ItemStack>` 的内存占用面前物理不可达，因此这条转换不需要一条
/// 「截断了怎么办」的错误路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarriedItemSlot {
    /// 背包（[`ll_world::entity::Agent::inventory`]）里的第几堆，
    /// 0 起。
    Inventory {
        /// 下标，见 [`CarriedItemSlot`] 文档「背包那一半」一节。
        index: u32,
    },
    /// 装备栏（[`ll_world::entity::Agent::equipment`]）的某个槽位，
    /// 键是真实存储键（锚点槽位）。
    Equipped {
        /// 锚点槽位。
        slot: EquipSlot,
    },
}

/// [`Effect::Inspect::items_seen`] 的元素——盘查那一刻看到的**一堆**
/// 物品：是什么（`def`）+ 在哪（`slot`）。
///
/// # 为什么不是裸 `ContentIndex`
///
/// 卫兵职业接线批次落地时这里是 `Vec<ContentIndex>`，只说「看到了哪
/// 几**种**物品」，不说「看到了哪几**堆**」。背包里两堆各一把铁剑
/// （一把自己买的、一把偷来的）在那个形状里**完全无法区分**——而
/// [`Effect::Inspect`] 文档「为什么没有任何是否违法的判断」一节写明
/// 的那个未来消费者要做的事，恰恰是等 `Owner` 落地后**逐堆**比对
/// `items_seen` 与各堆的 `owner`。拿到的只有种类就判不了罪：这个形状
/// 表达不了它自己文档里预告的那件事。带上槽位句柄之后能。
///
/// [`crate::rule_modifier::RuleModifier::InspectionConcealment`] 的「逐件掷骰」
/// （`ll_sim::rule_modifier`，该变体文档「为什么是逐件掷骰」一节）本来
/// 就是照着「逐堆比对」这个粒度选的——那一节原文「那条比对的粒度是
/// 『单件物品』，因此本被动的粒度也必须是单件」。本类型把那条论证依赖
/// 的粒度真正落进了类型里。
///
/// # 为什么不直接带 `Owner`
///
/// 项目所有者裁定：**不带**。守卫看到的是「你身上某个位置有一把剑」，
/// 那把剑归谁是**后果判定**该去查的事，不该在「看见」这个效果里就
/// 泄漏——与 [`Effect::Inspect`] 文档「为什么没有任何是否违法的判断」
/// 一节「本效果因此只如实记录『看到了什么』，不产出任何裁定」是同一
/// 条边界。何况 `Owner` 当前还不存在。槽位句柄恰好是这条边界上正确的
/// 那一侧：它是「位置」，查归属要拿它去问世界，不是从这条效果里读。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectedItem {
    /// 这一堆是什么物品，指向物品表。
    pub def: ContentIndex,
    /// 这一堆此刻在被盘查者身上的哪个位置。
    pub slot: CarriedItemSlot,
}
