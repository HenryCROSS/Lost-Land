# 增益/减益与通用触发器

**冻结于** 2026-08-18，**本次修订于** 2026-08-20。**落地状态**：不再是「纯设计,尚无代码」——`SkillEffect::TemporaryStatModifier` → `Effect::ApplyStatModifier` → `Agent.active_stat_modifiers` → `ll-sim::resolve::effective_attribute` 这一条链路已经是真代码,且读端已在 `98621f5`（「战斗结算的攻击力接上 active_stat_modifiers 的临时修正」）接上——**在此之前**,这条链路写得进去、进得了 `WorldState::hash()`、存得进档,却被 `resolve_attack` 完全无视,技能加成因此永远不影响伤害;`98621f5` 修的正是这最后一环。但这只是本文档设计的「改属性」这一类增益里最窄的一个特例,而且**现在被核实出一个真实的存储限制**（见新增六节）——`active_stat_modifiers` 按属性做键,同一属性上只能挂一条修正,不同来源会互相覆盖而不是叠加,与项目所有者刚拍板的「不同效果能叠加,同效果只刷新时间」直接冲突,六节给出具体改法。完整的 `ActiveEffect`/`TriggerDef`/`TriggerResponse` 通用增益与触发器框架,以及持续伤害、行动限制、抗性变化三类效果,仍然是纯设计（新增七节逐一核实现状与可行性）。`crates/ll-sim/src/effect.rs` 现有十四个变体（`MoveTo`/`Damage`/`Kill`/`RecordHistoricalEvent`/`IncrementKillCount`/`ScheduleNext`/`SetTerrain`/`AdjustWallet`/`ChangeSpace`/`SetScriptState`/`AdjustResource`/`SetSkillCooldown`/`ApplyStatModifier`/`MarkExplored`）里,只有 `ApplyStatModifier` 属于本文档设计的范畴且已落地（`crates/ll-sim/src/effect.rs:281`）——冻结时核实的"六个变体、没有任何增益/触发器相关类型"这句话已经因为 P5-B 等多个批次的落地而过时,如实更正;`Agent` 结构仍然没有 `active_buffs` 字段。**实现阶段**：新 P6「物品与装备」需要装备的属性加成先落地,本文档描述的增益系统在此基础上属于战斗结算的后续批次,与[三轴战斗设计](combat-three-axis.md)同批考虑;通用触发器框架本身不依赖装备系统,可以独立实现;**六节的存储改法不依赖 P6 或任何其他未落地系统,是本文档目前唯一有真实代码需要同步改动的一节,也是新增八节给出的落地顺序里排在最前的一项**。**冻结时对应 git 提交**：`1abb1d3`（本文档写作时的仓库 HEAD）；**本次修订对应 git 提交**：`234e041`（修订时的仓库 HEAD，`main` 分支）。

---

## 一、失效惰性判定：不排进时间轴

### 与 ADR 0014 同一个形状

[ADR 0014](../decisions/0014-season-pure-function-derivation.md) 裁定季节维持纯函数派生而不是时间轴事件，理由是"事件驱动的季节状态是又一个可能与时钟脱节的真相源"，且约束 C4 要求后台推进能到达任意确定 tick——跳跃式推进要补发全部历史事件，漏发一次就永久错位。**增益/减益的失效判定是同一个问题的另一个实例**：

```
排事件到期触发 Effect   → 后台跳进 100 年要补发几千次「到期移除」事件，漏一次永久错位，
                          且后台推进本身要遍历全部挂着增益的实体逐一检查是否到期，
                          与「按 Cohort 批量推进」（agent-goals-and-economy.md §八）
                          的惰性追赶思路直接冲突
惰性判定：存 expires_at   → 读取（战斗结算、UI 面板、任何需要知道"这个增益还在不在"
                          的地方）时比对当前世界时钟，`tick < expires_at` 即视为有效，
                          跳多远都对，不需要主动清理
```

约束 C4「后台推进离屏世界时必须推进到一个确定的世界时刻 T」在惰性判定下自动满足：无论后台跳过多长时间，重新进入前景层时逐个读取 `active_buffs` 现比对一次时钟即可得到正确结果，不需要在跳跃期间做任何事。事件驱动方案则必须在跳跃区间内逐个补发到期事件才能保持正确——这正是 ADR 0014 否决的那条路。

### 也是「默认派生，只存偏差」的又一次复用

[0009](../decisions/0009-derive-by-default-store-only-deviation.md) 的模式在这里再一次成立：**「这个增益现在是否生效」不存，只存「这个增益是什么时候到期的」**，前者永远是后者与当前时钟比较的现算结果，不是需要维护同步的独立状态。这与 NPC 钱包（存偏移量+重定基准时刻，现算当前值）、关系记忆（存记忆偏移，现算当前好感度）是同一条思路的第五个独立实例。

### 具体形状

```rust
/// 一条生效中的增益/减益实例——真正的偏差，进存档。
///
/// 与 `WorldState` 其余惰性派生字段同一模式：只存"何时到期"，不存
/// "现在是否生效"（现算，见二、`DerivedStats` 不入存档）。
pub struct ActiveEffect {
    /// 指向注册表里的增益定义（本体/mod 均可注册，见三）。**具体解析到
    /// 什么类型，本文档写作时长期缺失——[天赋/特性系统](trait-system.md)
    /// 二节已经补上：`def` 指向 `TraitTable` 的条目（`TraitDef`），本文档
    /// 与该文档共享同一张表，「一个 buff 就是有到期时间的天赋，一个天赋
    /// 就是永不过期的 buff」（项目所有者原话，该文档二节引用）——两者
    /// 效果载荷相同，区别只在于要不要实例化（buff 要，天赋不要，理由
    /// 见该文档二节「但存储方式不共享」一节，本文档不重复）。九节详述。
    pub def: ContentIndex,
    /// 到期的世界时钟刻度。`tick >= expires_at` 即视为已过期，
    /// 读取侧自然忽略，不需要主动移除——但见下方"为什么仍需要一次
    /// 显式清理"说明这不等于永远不清理。
    pub expires_at: Tick,
    /// 当前叠加层数/强度——具体语义由该增益的堆叠规则决定，见五。
    pub stacks: u32,
    /// 施加时刻，供多个增益同时修改同一属性时的确定性排序使用，见二。
    pub applied_at: Tick,
    /// 施加者，供「吸血反弹给谁」「谁的破甲穿透生效」这类需要溯源的
    /// 效果使用。
    pub source: EntityId,
}
```

### 为什么仍需要一次显式清理，惰性判定不等于永不清理

惰性判定解决的是"到期这一刻要不要主动做点什么"，不解决"`Vec<ActiveEffect>` 会不会无限增长"。若一个实体几百年里累积了几千条早已过期的增益记录却从不清空，`active_buffs` 会变成一个只增不减的列表，每次现算 `DerivedStats` 都要线性扫过大量已经无意义的过期条目。**解法是在读取路径顺带清理**：任何一次读取 `active_buffs`（例如 `derive_stats` 调用）时，先过滤掉 `tick >= expires_at` 的条目再使用，且这次过滤的结果**可以**顺手写回（不是必须——写不写回只影响下次读取前是否要重新过滤一遍已经知道会被丢弃的条目，属于性能优化，不影响正确性）。这与「惰性判定」并不矛盾：清理动作本身仍然是惰性触发的（读的时候顺带做），不是排一个「N 天后清理」的独立事件。

---

## 二、增益改属性，但衍生属性绝不入存档

```
存    active_buffs: Vec<ActiveEffect>              真正的偏差（一、已定义）
不存  DerivedStats = f(基础属性, 装备, 生效增益)     每次现算，见 attribute-system.md §七
```

这是[属性系统](attribute-system.md) §七"衍生属性绝不进存档"这条纪律的直接延伸——`derive_stats` 的入参在[三轴战斗设计](combat-three-axis.md) §四已经补上了"装备"这一项的接线点，本文档补上"生效增益"这一项：`derive_stats(基础属性, 装备, 生效增益, 负重)`，增益的贡献与装备的贡献（`StatBonus`，接线点同样待补）用同一个函数一次性算出最终 `DerivedStats`，不分两次叠加——理由与[种族系统](race-system.md) §二"为什么不能走每次派生时叠加"一节完全相同：若增益的贡献单独在别处再叠一层，"这个属性到底该不该穿这层查询"就要在每个读取点重复决定，漏一处就是隐蔽的数值缺陷。

### 必须钉死：多个增益改同一属性时结算顺序必须确定

`active_buffs` 现算 `DerivedStats` 时，若两个增益都修改「护甲」，谁先谁后可能影响最终结果（例如一个是加法、一个是乘法百分比，顺序不同结果不同）。**排序规则**：按 `def`（`ContentIndex`）升序，同一 `def`（同一增益的多个独立实例，见五「独立计时」堆叠策略）再按 `applied_at` 升序。

这与[战斗结算设计](combat-three-axis.md) §六「按 `EntityId` 升序」、[脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) §5.5「排序必须是全序，平局按 `EntityId` 升序打破」是同一类问题的第三个实例——**任何"多个东西要合并成一个结果"的场景，合并顺序都必须由排序键+一个固定的平局规则决定，不能依赖容器的插入顺序或迭代顺序**。这条纪律与本仓库另一处正在整理的、编号混乱的"禁止 HashMap/HashSet 迭代顺序参与逻辑判断"约束（见 `knowledge/audit/worklist.md` W-05）是完全同一个精神：任何隐藏的非确定性输入源都会导致同一份存档在不同运行间产出不同结果，`ContentIndex` 升序+`applied_at` 升序对增益合并顺序而言，扮演的正是 `EntityId` 升序对实体排序而言的角色。

**这条排序纪律现在有了一个真实的、已落地代码需要遵守的具体实例**——六节 `active_stat_modifiers` 的存储改法（多个来源同时修正同一属性）正是本节这条通则第一次真正落地的场景；六节的具体容器选择为什么不需要再额外引入 `applied_at` 这个平局分支（键本身已经排除了平局的可能），见六节「与本节排序规则的关系」一段。

---

## 三、命中效果只是触发器的一种——做成一套通用触发器

「命中时中毒」「几率击晕」「吸血」目前看起来是 `on_hit` 专属的效果，但接下来必然还要 `on_kill`（击杀奖励）、`on_damaged`（受伤反击/荆棘甲）、`on_turn_start`（回合开始的持续伤害/回复）、`on_death`（死亡时的爆炸/召唤）。**如果现在只为 `on_hit` 写专用钩子，下一个需求来的时候就要再加一个专用钩子，五个需求就是五套各自独立的接线代码**——这正是本节要避免的模式。

### 分档设计（复用 ADR 0016）

```
一档（零开销）：常见响应声明式表达——"命中时 60% 概率施加中毒（持续 3 回合）"
              这类效果只是几个数值+一个增益 ContentIndex 引用，注册期物化成表，
              触发时查表直接产出 Effect，不经过任何脚本调用。
二档（低开销）：受限公式——"造成本次伤害 20% 的吸血"这类可以编译成一小段
              固定运算的效果，Rust 侧求值，不跨 VM。
三档（有开销）：任意脚本回调——只有真正需要"任意逻辑"的触发器（例如某个
              需要读取历史事件日志才能判断的稀有效果）才落到这一档，
              且必须像 ADR 0016 要求的那样，让这一档的存在对本体和 mod
              公平——本体也可以用三档，但用了就要承受同样的跨 VM 开销，
              不能借口"这是引擎自带的所以走捷径"。
```

「常见响应要能声明式表达（第一/二档），只有特殊的才落脚本回调（第三档）——否则每次命中跨一次 VM，热路径受不了」直接引用 [ADR 0012](../decisions/0012-steel-capability-surface-verification.md) 已实测的数字：`engine.run` 单次调用 327~400µs，一场战斗几十次命中若每次都触发脚本回调，仅触发器本身的开销就可能吃掉一帧的大半预算。

### 触发器的形状

```rust
/// 一个触发器定义：什么时候触发、触发时做什么。走注册表，mod 可注册。
pub struct TriggerDef {
    pub id: NamespacedId,
    pub on: TriggerKind,           // Hit / Kill / Damaged / TurnStart / Death / ……
    pub chance_permille: i32,      // 触发概率，千分比
    pub response: TriggerResponse, // 见下——一/二/三档三选一
}

pub enum TriggerResponse {
    /// 一档：施加一个已注册的增益。
    ApplyBuff { def: ContentIndex, stacks: u32, duration_ticks: u32 },
    /// 一档：直接造成一笔固定/比例伤害（吸血、反伤都是这个变体的两种读法：
    /// 吸血 = 对施加者的负伤害；反伤 = 对施加者的正伤害）。
    DealDamage { base: i32, permille_of_trigger_damage: i32 },
    /// 二档：受限公式，具体形状留给实现时按 ADR 0017 的编译产物设计。
    Formula(/* 占位，本文档不展开 */),
    /// 三档：脚本回调，逃生舱，代价见 ADR 0012 引用的实测数字。
    Script(NamespacedId),
}
```

`TriggerKind` 覆盖 `on_hit`/`on_kill`/`on_damaged`/`on_turn_start`/`on_death` 五种起步场景——**这份清单本身也走注册表式的开放集合，不是写死的 Rust 枚举硬编码**（若未来需要 `on_heal`/`on_block` 之类新触发点，新增一个变体即可，不影响已有触发器的声明方式），但触发点本身（"什么时候检查触发器"）仍然是引擎层职责（ADR 0018 三步法：触发时机由 `resolve`/`apply` 的既定管线决定，不能由脚本自行决定"我现在要不要触发"，那会破坏约束 C1）。

一件装备/一个物品的 `use_effect`、一个技能的效果，都可以只是"注册一个或多个 `TriggerDef`，挂在 `on_hit`/`on_use` 之类的触发点上"，不需要为每种内容各自发明一套效果表达方式——这与[物品系统](item-system.md)"物品的拿起、丢下、交易、装备、存入箱子全部走同一个 `Effect::MoveItem`"是同一层意义上的"少数几个通用原语覆盖大量具体内容"。

**`TriggerResponse::ApplyBuff { def, stacks, duration_ticks }` 的 `def` 解析进 `TraitTable`**——与一节 `ActiveEffect.def` 同一个类型、同一张表（九节详述，`ActiveEffect.def` 那处遗留的"解析到什么类型"缺口一并在这里补上）。`TriggerResponse` 的另外三个变体不受影响：`DealDamage`/`Formula` 只携带裸数值，不引用任何注册表内容，**不依赖 `TraitTable` 落地即可独立工作**（八节落地顺序据此把「触发器框架本体 + `DealDamage` 档」与「`ApplyBuff` 档」拆成两个不同批次）；`Script` 走脚本回调，同样不涉及这个类型问题。

---

## 四、必须结构性禁止递归——这是真陷阱

### 问题

```
命中 → 施加中毒 → 中毒伤害（on_turn_start 触发）→ 触发「受伤反击」（on_damaged）
  → 命中 → 施加中毒 → 中毒伤害 → 触发「受伤反击」→ ……
```

两个各自设计合理的触发器（中毒的持续伤害、荆棘甲式的受伤反击）组合在一起就能产生无限循环——**这不是任何一个触发器自身的缺陷，是触发器可以互相触发这件事的必然后果**，没有办法靠"小心设计每一个触发器"来杜绝，因为组合爆炸的空间不是设计单个触发器时能看见的。

### 解法：不是「小心点写」，而是结构性兜底

**触发产出的效果进队列，由 `apply` 逐条消费，并设深度上限。**

```
效果队列: VecDeque<(Effect, depth)>

初始：resolve(world, intent) 产出的 Vec<Effect>，每条 depth = 0，全部入队

循环，直到队列空：
    (effect, depth) = 队首出队
    apply(world, &effect)                 // 唯一写入口，不含逻辑，见下方"不破坏架构"
    若 depth < MAX_TRIGGER_DEPTH:
        triggered = resolve_triggers(world, &effect)   // 纯函数，读 apply 之后的世界，
                                                          // 决定这个 Effect 触发了哪些
                                                          // TriggerDef，产出新的 Effect
        队列尾部追加 (e, depth + 1) for e in triggered
    否则：
        记日志「触发深度超限，截断」，不再展开这条 effect 触发的后续效果
```

**这不破坏「意图—结算—效果」架构**：决定"这个 `Effect` 触发了什么"仍然是一个像 `resolve` 一样的纯函数（`resolve_triggers`：只读世界、不改世界、产出 `Effect`），只是它读取的是"某个 `Effect` 已经被 `apply` 之后"的世界状态（例如"受伤反击"必须知道反击时的最新生命值，这个值只有 `apply` 写入之后才存在）。`apply` 本身仍然是"唯一写入口，单线程，不含任何游戏逻辑"——真正决定触发什么的逻辑在 `resolve_triggers` 里，`apply` 只管把一条 `Effect` 写进世界，驱动这个队列循环的是比 `resolve`/`apply` 更外层的一个调度器（现有代码里 `TurnEngine::advance_ai` 已经是这一层"驱动循环"的先例，见下）。

### 深度上限取多少：8

**建议 `MAX_TRIGGER_DEPTH = 8`。** 依据：设计上能想到的最深合理链条大约是 3~4 层（命中 → 施加中毒 → 中毒跳字触发受伤反击 → 反击本身命中触发一次新的中毒）——8 是这个量级的两倍余量，既给真正需要稍深链条的内容（例如三段式连锁技能）留出空间，又足够紧，一旦真的进入无限循环，最多在 8 步内截断，不会像[规格 §14.4](../../docs/superpowers/specs/2026-08-16-lostland-design.md)描述的"逐级加压排查 O(n²)"那样需要真正跑起来才发现问题——深度上限是一个编译期常量，问题在设计阶段就能被看见（"这条链条最深会到几层"是可以静态推演的），不需要等到实际触发才暴露。

**与 P3 那个 `advance_ai` 的 `MAX_STEPS_PER_ADVANCE` 兜底是同一类，而那次兜底真的救了场**：`crates/ll-sim/examples/p3_acceptance/turn.rs:39` 的 `MAX_STEPS_PER_ADVANCE = 10_000` 原本只是"未预见的死循环"的保底防线，P3 收尾时真实撞上过——玩家死亡后 `advance_ai` 的循环条件没有跟着更新，继续对其余存活敌人反复空转，实测单帧卡顿达数秒，直到耗尽步数上限才放弃（见 `knowledge/handoff/p3-to-p4.md` 「一、2. 玩家死亡后主循环空转」）。**这次兜底不是理论上的保险——它真的在一个完全不同的缺陷场景里生效并把"冻结整个事件循环"降级成"本帧卡顿后自动放弃"**。触发器队列的深度上限是同一个思路在战斗结算里的应用：即便某次内容组合在设计阶段没被想到会互相触发出无限链条，深度上限也能保证最坏情况下只是"这一批效果的展开在第 8 层被截断并记日志"，而不是冻结整个回合结算。

两处兜底的量级不同（10000 vs 8）是因为职责不同：`MAX_STEPS_PER_ADVANCE` 保护的是"一整帧内可以处理多少个独立实体的行动"（步数级），本文档的 `MAX_TRIGGER_DEPTH` 保护的是"一次攻击触发的连锁反应最多能有多深"（层级），两者不是同一个维度上的数字，不能类比大小，只能类比"设一个硬上限+超限记日志截断"这个模式本身。

---

## 五、堆叠规则必须是数据

同一增益施加两次，行为不是唯一的："刷新持续时间"（重新点燃一次已经在燃烧的目标，燃烧时间从头算）、"叠加强度"（连续中两次弱效果毒，叠成更强的中毒）、"各自独立计时"（两支箭各自的流血效果互不影响，各自到期）——三种都是合理的游戏设计选择，**取决于具体是哪个增益，不取决于系统本身**。

```rust
/// 堆叠策略：走注册表第一档（声明式），mod 加新增益可以自由选择行为，
/// 不需要碰 Rust 代码。
pub enum StackPolicy {
    /// 再次施加时刷新 `expires_at`，`stacks` 恒为 1。
    RefreshDuration,
    /// 再次施加时 `stacks` 累加（有上限），`expires_at` 取较晚者
    /// （或按具体增益定义决定是否也刷新，留给数值设计）。
    AddIntensity { cap: u32 },
    /// 每次施加都是独立的 `ActiveEffect` 实例，各自计时、互不覆盖，
    /// 直到各自到期各自消失。数量若也需要上限，同样由数据声明。
    Independent { max_count: u32 },
}
```

**写死在代码里的话，mod 加新增益就没法选行为**——若堆叠逻辑是一段写在 Rust 里的固定 `if/else`（例如"所有增益一律刷新持续时间"），mod 作者想做一个"叠加强度"的中毒效果就完全没有办法，只能等引擎开发者专门为这一种增益加一个特例分支，而特例分支这个模式正是 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md) 明确否决的——"若本体需要一个 mod 够不着的性能档位（或行为分支），那是 API 缺陷，不是特性"，堆叠策略同理：走声明式（一档）means 本体的燃烧、中毒、流血都只是这个通用 `StackPolicy` 枚举 + 具体数值的注册表条目，与 mod 定义的堆叠行为走完全相同的代码路径。

---

## 六、`active_stat_modifiers` 不能叠加：项目所有者已经拍板的存储改法

### 现状：按属性做键，不同来源互相覆盖

`Agent::active_stat_modifiers: BTreeMap<AttributeKind, ActiveStatModifier>`（`crates/ll-world/src/entity/stats.rs:82`、`agent.rs:207`）按属性做键——同一项属性同一时刻只能挂一条修正。喝一瓶 +2 力量药水，再中一个 +3 力量祝福，第二次写入直接覆盖第一次，结算时只看得到 +3，不是 +5。[载具与骑乘系统](vehicle-and-mounting.md) 一节已经把这个行为当成既有事实写了下来（"按 `AttributeKind` 键控，`RefreshDuration`（后写覆盖先写），不是叠加多条"）——**本节的改法会让那句话过时**，见下方「对既有文档的影响」。

### 项目所有者的裁定

> 「我希望能叠加 buff」
>
> 「不同效果能叠加，同效果只刷新时间」

这不再是本文档单方面论证的设计选项，是既定需求。以下四条规则把这句话落到具体的类型与算法上。

### ① 身份是「效果来源」，键是 `(属性, 来源)`

**同一个效果可能同时修正多个属性**——「英雄气概」六维全加一点，「熊之忍耐」只加体质——所以「来源」本身不能单独做键：若键只是来源（不含属性），"英雄气概"这一次施加就要往同一个键里塞六个不同属性的 `delta`，值的形状会从一个标量修正膨胀成一个字典，等于把外层结构又搬回值里。**键必须是 `(属性, 来源)` 这一对**，`effective_attribute` 要问的问题本来就是"这一项属性现在有哪些修正在生效"，键的第一段就该是属性——这确定了容器的外层结构。

「来源」的类型是 `ContentIndex`，但**不是"技能的 `ContentIndex`"这么窄**——`vehicle-and-mounting.md` 已经证明了第二个真实生产者：一件载具的攻防加成（`MountDef` 的 `stat-modifiers` 列表）走的是"完全相同的底层机制"（该文档 `vehicle-and-mounting.md:342` 附近原话），意味着载具落地后，"来源"既可能是 `SkillTable` 的条目，也可能是 `MountTable` 的条目，未来还会是 `TraitTable` 的条目（九节）。**「来源」的准确定义是"施加这条修正的那份内容定义自己的 `ContentIndex`"，不绑定任何具体的注册表**——`ContentIndex` 本身就是一个跨注册表通用的紧凑索引类型（`crates/ll-core/src/ident.rs:98`），这条设计不需要额外的"这是哪一类来源"标签，两个不同注册表分配出的 `ContentIndex` 数值当然可能相同，但它们从来不会出现在同一次 `(attribute, source)` 判定里——同一次判定的两个来源永远来自"这一次施加时实际调用方传入的那个 `ContentIndex`"，调用方（`resolve_use_skill`、未来的载具结算）各自知道自己传的是哪张表的索引，本类型不需要、也不应该替调用方多存一份"这是哪张表"的元数据。

### 存储结构：`BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>`

```rust
/// `Agent.active_stat_modifiers` 新形状——外层按属性做键（匹配
/// `effective_attribute` 的真实访问模式：一次查询要问的始终是"这一项
/// 属性现在有哪些修正在生效"），内层按「来源」做键（见上）。
pub active_stat_modifiers: BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>,

/// 一条生效中的临时属性修正——形状不变，仍然只有两个字段。`source`
/// 不再是它的字段，改由外层容器的内层键携带（不把同一份信息在键和值
/// 里各存一份——那样会制造一种类型层面本不该存在的可能性："这条记录
/// 的键与它自己声称的来源不一致"，哪怕调用方永远不会真的写出这种
/// 不一致，也不该让类型允许它存在）。
pub struct ActiveStatModifier {
    pub delta: i32,
    pub expires_at: Tick,
}
```

**为什么不是另外三种候选形状**（均已评估）：

- **扁平 `Vec<ActiveStatModifier>`，每条自带 `attribute` 字段**——`effective_attribute` 每次查询都要线性扫过这个实体全部属性上的全部修正,再筛出目标属性那一部分,访问模式与容器形状完全不匹配,且"同源覆盖、异源共存"的判定要靠手写的"先找有没有相同 `(attribute, source)`" 线性查找实现,是本该由容器结构本身保证的不变式,退化成了调用方每次都要正确维护的手写逻辑。
- **单层 `BTreeMap<(AttributeKind, ContentIndex), ActiveStatModifier>`**——键本身是对的,但"查这一项属性当前所有修正"需要一次 `range` 查询,而不是一次直接的 `get`（元组的字典序是先比较 `AttributeKind` 再比较 `ContentIndex`,同一属性的全部条目确实连续排列,`range` 能做,但比嵌套 `BTreeMap` 多一层"构造正确的 range 边界"的样板代码,且没有换来任何嵌套方案给不了的好处）。
- **`BTreeMap<AttributeKind, Vec<ActiveStatModifier>>`**——外层对了,但内层退化成 `Vec` 后,"同源覆盖"需要在插入时手写"先线性扫描找相同来源再决定覆盖还是追加",与扁平方案在这一点上是同一个问题,只是扫描范围从"这个实体全部修正"缩小成了"这一项属性上的全部修正"——范围缩小了,但手写查找的责任还在,不如让内层容器本身（`BTreeMap<ContentIndex, _>`）的 `entry` API 免费提供这份保证。

**嵌套 `BTreeMap` 两层都满足约束 C5**：外层按 `AttributeKind`（已实现 `Ord`）排序,内层按 `ContentIndex`（已实现 `Ord`）排序,不涉及任何 `HashMap`/`HashSet` 迭代顺序,不需要额外的排序步骤。

### ② 同一来源再次施加：刷新是取较晚的到期时刻，不是时长相加

```rust
/// 同一 `(attribute, source)` 再次被施加时的合并规则——项目所有者
/// 「不同效果能叠加，同效果只刷新时间」这句话里「同效果」半句的具体
/// 落地。
fn merge_same_source(
    existing: ActiveStatModifier,
    incoming: ActiveStatModifier,
) -> ActiveStatModifier {
    ActiveStatModifier {
        // ③：强度取绝对值更大者，见下一小节。
        delta: if incoming.delta.abs() >= existing.delta.abs() {
            incoming.delta
        } else {
            existing.delta
        },
        // ②：到期时刻取较晚者，不是把两段时长加起来。
        expires_at: existing.expires_at.max(incoming.expires_at),
    }
}
```

**为什么是「取较晚」不是「时长相加」**：时长相加会把"无限刷同一瓶药水"的漏洞从"数值无限叠加"原样平移成"持续时间无限叠加"——连续喝十次 10 tick 的药水会变成 100 tick 的效果，这与"同源只刷新，不叠加"这条规则本身要防的漏洞是同一个漏洞，只是换了个维度发作。**取较晚**允许玩家靠"到期前重新施放"维持永续在线（这是绝大多数游戏里"续 buff"这个操作本身的合理玩法），但任何一次重复施放本身，最多只能把到期时刻推到"这一次施放本该到期的时刻"，不会让连续快速重复施放的总效果超过单次施放——这与`Tick` 已实现 `Ord`（`crates/ll-core/src/time.rs:44`）这一点直接对应，`.max()` 不需要任何额外实现。

### ③ 同一来源不同强度：取较强的数值，且独立取较晚的到期时刻

**同一个技能的强化版本**，或**由更高战力的施法者放出的版本**，再次施加时 `delta` 可能与已经生效的那条不同。两个维度分别处理，互不牵连：

- **强度**：取 `delta.abs()` 更大的那个 `delta`——防止一次弱化的重复施放（例如低等级角色对同一目标补了一次同名但较弱的技能）悄悄把已经生效的强化版本冲淡。若两次 `delta` 绝对值相等，退化成取新值（两者本就等价，谁赢都不改变结果）。
- **到期时刻**：仍然独立取较晚者（②）——**这与强度谁赢无关**：哪怕弱化版本没能刷新强度，它依然应该把到期时刻续到自己本该持续到的那一刻，不应该因为强度上"打了败仗"就连续航时间也一并作废——两个维度各自回答各自的问题，不应该因为其中一个维度"输了"就连带影响另一个维度的结果，这与一般直觉一致（弱版本至少续了个时间）。

**为什么不是「取最新」或「取较强+不管到期」两种更简单的方案**：「取最新」会让一次弱化的重复施放（哪怕只是同一个技能的不同等级版本，或不同施法者）直接冲掉已经生效的强化效果，这是明显违反直觉的行为，且没有任何理由认为"更晚发生"天然意味着"应该赢"——发生的先后顺序在这里不该是决定强度的依据。「取较强但到期时刻恒定不变」则会让弱化版本的重复施放变得完全没有意义（哪怕玩家确实想用它续一下时间），与"同源刷新"这个操作本身应该有的效果（至少续时间）矛盾。**结论：强度取较强、到期时刻取较晚，两个维度独立比较**——这条规则需要在实现批次里配一条测试断言：`较弱版本刷新较强版本 -> 强度不变，到期时刻更新`与`较强版本覆盖较弱版本 -> 强度更新为较强值，到期时刻取两者较晚者`两条用例都要被显式覆盖，不能只测其中一条方向。

### 叠加上限：现在不做

**结论：不设「同一属性最多同时挂 N 个不同来源」这类上限，YAGNI**——当前没有任何已知内容会让同一属性同时被数十个不同来源修正，异源数量天然受"游戏内容里到底设计了多少种会修正同一属性的技能/天赋/装备"这个内容规模约束，不是一个会失控增长的向量（会失控增长的是"同源无限重复施放"，②已经堵死）。若未来真的出现需要限制的场景，加一个上限不需要改变本节的容器形状——只需要在 `merge_same_source` 之外的"这是一个全新来源，要不要接受插入"这一步（`per_source.get(&source).is_none()` 分支）加一条"若 `per_source.len() >= cap` 则拒绝插入或淘汰最弱/最旧一条"的判断，是纯增量式的补丁，不需要推翻已经定下的键与容器形状——这正是先把接缝焊对、再等真实需求出现的低成本先手棋，与[行动能力与输入上下文](action-capability-and-input-context.md)一节"先把接缝焊好，再等内容"是同一个模式。

### 每条修正各自到期，聚合从「查一次」变成「遍历一小圈」

```rust
/// 六节改法下的聚合逻辑——每条 `(attribute, source)` 都有自己的
/// `expires_at`，互不牵连，聚合时逐条过滤已过期的条目再求和。
fn effective_attribute(
    base: i32,
    kind: AttributeKind,
    modifiers: &BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>,
    now: Tick,
) -> i32 {
    let Some(per_source) = modifiers.get(&kind) else {
        return base;
    };
    per_source
        .values()
        .filter(|modifier| modifier.expires_at.0 > now.0)
        .fold(base, |acc, modifier| acc + modifier.delta)
}
```

**性能影响评估**（`resolve_attack` 是热路径）：原实现是一次 `Option` 查表 + 一次比较，`O(1)`。新实现是一次外层查表（`O(log 6)`，`AttributeKind` 只有六个变体，实际上是 `O(1)`）+ 一次对内层 `m` 条记录的遍历，`m` 是这一项属性**当前生效的不同来源数**——现实内容规模下是个位数（一个角色同一时刻身上叠着两三个同时修正力量的效果已经算是极端场景），且这次遍历只在真正结算一次攻击时付一次，不是每 tick 都要跑一遍——**可接受，不需要任何额外优化**。若未来某个离谱的内容组合（例如数十个来源同时叠在同一属性上）真的让这个数字变得可观，问题出在内容设计本身允许了这种组合，不是本节引入的算法复杂度问题，叠加上限（上一小节）到那时候才是该动的口子，不是本节这段代码。

**为什么仍然需要一次显式清理**：一节已经论证过"惰性判定不等于永不清理"——同一个来源的修正若不再被重新施加（例如某个技能被卸载、角色不再使用它），它的 `ActiveEffect` 会在过期后一直留在 `per_source` 里，直到某个未来的读取顺带把它筛掉。这条纪律原样适用于本节新形状，不需要重新论证。

### 与二节排序规则的关系

二节要求"多个增益改同一属性时，合并顺序必须由 `def` 升序 + `applied_at` 升序这个全序决定"——本节的具体容器选择让这条规则在 `active_stat_modifiers` 这个具体场景下**自动满足，且不需要 `applied_at` 这个平局分支**：内层 `BTreeMap<ContentIndex, _>` 本身就按 `ContentIndex`（即"来源"，等价于二节说的 `def`）升序遍历，`.fold` 求和用的正是这个天然顺序；`applied_at` 平局分支之所以不需要，是因为**本节的键设计从根本上排除了平局**——同一个 `(attribute, source)` 永远只能有一条记录（这正是"同源合并"这条规则本身的定义），不会存在两条键相同、只能靠 `applied_at` 分高下的记录。二节的 `applied_at` 分支仍然对**别处**有意义——五节 `StackPolicy::Independent`（同一个 `def` 允许多个独立实例并存，例如"两支箭各自的流血效果互不影响"）会真的产生"同一 `def`、不同 `applied_at`"的多条记录，那时候二节的平局规则才真正派上用场；`active_stat_modifiers` 从落地第一天起就固定选用"同源合并"而非 `Independent`（`crates/ll-world/src/entity/stats.rs` 既有文档注释已经点明这是"本计划固定选用的唯一堆叠策略"），本节的改法没有推翻这条固定选择，只是把它的判定范围从"整个属性"缩小成了"同一个来源"——这正是本次修订要修的漏洞本身。

**加法是可交换的，但迭代顺序仍然要保持确定**：当前 `.fold` 做的是纯加法求和，不同来源的迭代顺序不影响结果数值，但这不代表顺序可以随意——**若未来出现「替换型」修正**（[载具与骑乘系统](vehicle-and-mounting.md) 已经点名速度是替换语义，不是叠加语义），聚合就不再是可交换的加法，届时顺序会直接决定"谁的替换值最终生效"，`BTreeMap<ContentIndex, _>` 按键天然有序这条性质到那时候才真正体现出价值——现在虽然用不上（加法不在乎顺序），但选一个天然有序的容器不需要为这个"现在用不上、将来会用上"的性质多付任何代价，不是过度设计,是免费的。

### hash()：多一层遍历，仍然全序，满足约束 C5

```rust
hasher.write_u64(agent.active_stat_modifiers.len() as u64);       // 有修正的属性种类数
for (attribute, per_source) in &agent.active_stat_modifiers {     // 按 AttributeKind 升序
    hasher.write_u64(*attribute as u64);
    hasher.write_u64(per_source.len() as u64);                    // 这一属性上的来源数
    for (source, modifier) in per_source {                       // 按 ContentIndex 升序
        hasher.write_u64(u64::from(source.get()));
        hasher.write_i64(i64::from(modifier.delta));
        hasher.write_i64(modifier.expires_at.0);
    }
}
```

两层遍历都由 `BTreeMap` 自身的键序保证确定性，不需要额外排序调用——`crates/ll-world/src/state.rs:927`~`932` 现有的单层遍历需要在实现批次改成这个双层形状，改动范围局限在 `hash()` 这一段，不影响它前后的其余字段。

### 存档：schema v1，不需要迁移，这次改动是免费的

**已核实**：`383246d`（"清空发布前的存档迁移链，老存档明确拒绝"）已经把 `CURRENT_SCHEMA_VERSION` 重置为 `1`（`crates/ll-content/src/save_file.rs:90`），删掉了此前三步迁移函数与配套镜像类型——项目尚未发布，此前累计的存档没有保留价值，老存档遇到字段形状变化会被直接拒绝，不需要写一条 `Migration` 把旧的单层 `BTreeMap<AttributeKind, ActiveStatModifier>` 转换成新的双层形状。**这也是本节改法值得排在最前面做的一条理由**（八节）：越晚做，越可能有更多依赖 `active_stat_modifiers` 当前形状的代码需要跟着改（例如载具落地后 `MountDef.stat-modifiers` 若已经按旧形状写好接线代码），现在做，改动面最小，且完全不用为存档兼容性买单。

### 对既有文档的影响：`vehicle-and-mounting.md` 一句话会过时

**如实标注，不代为修改**（本次任务写权限只覆盖本文件与 `README.md` 自己那一行）：`vehicle-and-mounting.md` 一节原话"按 `AttributeKind` 键控，`RefreshDuration`（后写覆盖先写），不是叠加多条"——本节改法落地后这句话不再成立：`active_stat_modifiers` 会变成"按 `(属性, 来源)` 键控，同源刷新、异源叠加"。**这不影响该文档已经做出的核心判断**（载具的攻防加成复用 `active_stat_modifiers` 这条通道本身完全正确，四节已经论证"来源"不限定于技能，载具正是第二个真实生产者）——需要更正的只是这一句对当前存储行为的事实性描述，留给下一次真正触碰该文档的批次顺手补上，与九节"`ActiveEffect.def` 待补"是完全同一类"已发现、无权限、如实标注"的处理方式。

---

## 七、四类效果缺口：现状核实、DoT 架构分岔的结论、能不能做

项目所有者点名的四类效果——持续伤害、行动限制、抗性变化、触发式效果——本节逐一核实现状，并回答"现在能不能开始做"。

### 7.1 持续伤害（中毒、灼烧）：架构必须先回答"谁来推进它"

**现机制纯惰性，读的时候比一次时钟，没有任何东西在"跑"**——这套机制回答"这个 buff 现在还生不生效"完全够用（一节已经用 ADR 0014 的同构论证证明了这一点），但回答不了"这段时间里掉了多少血"，因为没有任何代码路径会在"中毒的第 N 个 tick"这个时刻主动做点什么。持续伤害要求的是一个**真实发生过的离散事件**（这一下扣了多少血、扣血这件事本身可能触发 `on_damaged`——三节已经点名"受伤反击/荆棘甲"），不是一个可以随时现算的布尔值——这正是本节要解决的架构分岔。

**两条路**：

- **甲：buff 进时间轴**——给周期性 buff 排时间轴条目，到期时刻由调度器主动触发扣血。
- **乙：惰性补算**——受害者下次行动时，一次性结算"上次到现在应该掉多少血"。

**结论：甲，但不是"给每个 buff 单开一条时间轴条目"**——是把 DoT 的结算点挂在**已经存在**的、每个能行动的实体本就参与的调度节点上，不新增时间轴条目总量。

**依据**：`TurnEngine`/`advance_ai`（`crates/ll-sim/examples/p3_acceptance/turn.rs:124`）已经把每个实体的 `next_action_at: Tick` 排进 `timeline`——**任何一个受 `TurnEngine` 驱动的实体本来就是时间轴参与者，不是因为挂了 DoT 才变成时间轴参与者**。三节已经把 `on_turn_start`（"回合开始的持续伤害/回复"）列为触发器起步场景之一——这不是巧合，是本文档写作时就已经预见到 DoT 该挂在"回合开始"这个既有节点上，本节只是把这句原本抽象的话落到具体的调度决策上：**一个实体的时间轴条目到期、真正轮到它这一回合时，先检查它身上有没有到期该结算的 DoT，有就先产出对应的 `Effect::Damage`，再继续解析这一回合的 `Intent`**——不是"每个 buff 一条独立条目"，是"每个实体本来就有的那一条时间轴条目，顺带多做一件事"。这与本文档四节"效果队列，由 `apply` 逐条消费"是同一个"复用既有调度点，不新开一层"的精神。

**为什么不是乙——两个具体的失效场景，不是抽象顾虑**：

1. **中途被治疗，补算的语义会变得很怪**——受害者在中毒期间被治疗了一次，下次行动时"补算上次到现在该扣多少血"要不要把治疗那一刻之前已经被抵消的伤害也算进去？乙没有天然的答案：要么在每次治疗时也顺手做一次"结算到此刻为止的欠账"（等于自己也变成了一种事件驱动，只是换了个触发点，没有真的避开"事件驱动"这条路），要么干脆不管，把治疗前后的欠账混在一起一次性算（会让"这次治疗到底顶了多少中毒伤害"这个问题在数值上失去意义）。
2. **陷阱：一直不行动，补算永远不触发**——这是本节最关键的否决理由，且有两个具体的、现在就能指认的失效实例，不是一个笼统的"未来可能有问题"：
   - **被眩晕/定身的实体**——七节 7.2 会确认 `ActionCapability` 不会禁止 `Intent::Wait`（"什么都不做"永远可以选），但乙的"下次行动"若被狭义地理解成"下一次产生实际效果的行动"（移动/攻击/使用技能），一个被完全控制到只能 `Wait` 的实体就永远不会触发补算——而"控制到动不了 + 身上挂着中毒"恰恰是这类效果最常被组合使用的场景（放倒后确保对方无法自救），乙在这个最该生效的场景里恰恰失效。
   - **背景层实体**——`ThinPopulation` 这类背景/统计层实体根本不逐个走 `Intent` 解析（`agent-goals-and-economy.md` §八"按 Cohort 批量推进"），乙的"下次行动"这个触发点对它们而言可能永远不存在，中毒的背景 NPC 会在数据上"生病但永不发作"，直到（如果）它被提升回前景层。

**甲（挂在既有 `on_turn_start` 调度节点上）为什么不踩第一个坑**：`Intent::Wait` 仍然会让实体正常轮到自己的回合（`action-capability-and-input-context.md` 3.2 节已经论证"完全跳过回合"不是本项目 `ActionCapability` 的设计——被限制的是"能不能做某类具体动作"，不是"要不要轮到这个实体"），`on_turn_start` 挂在"轮到这个实体"这一刻，不挂在"这个实体真的做了什么"，眩晕、定身都不影响它触发。

**如实保留的残余缺口（不在本文档解决范围）**：背景层实体不逐个触发 `on_turn_start` 这件事，甲同样没有天然解法——这不是 DoT 专属的新问题，是"周期性效果在背景/统计层如何被批量近似"这个更大的既有课题（`agent-goals-and-economy.md` §八本就要处理"背景层如何低成本追赶"），DoT 只是第一个真正需要这个答案的具体效果。本文档如实标注：**甲解决了"实体在前景层但不产生 `Intent`"这一半的陷阱（眩晕），没有解决"实体根本不在前景层"这一半（背景层）——后者留给背景推进机制自己的批次回答，不是本节能顺带解决的**。

### 7.2 行动限制（眩晕、定身）：`ActionCapability` 已经设计好，分两段看"现在能不能做"

`ActionCapability`（`action-capability-and-input-context.md` 三节）是一组位标志（`MOVE`/`ATTACK`/`CAST`/`ITEM`），`current_capability(agent, tick)` 通过折叠 `agent.active_buffs` 里每条效果的 `restricts: ActionCapability` 字段算出当前还能做什么——多重限制天然满足位运算的交集语义（`ALL.difference(A).difference(B)` 恒等于 `ALL.difference(A ∪ B)`），**不需要本文档二节那套"结算顺序必须确定"的排序规则**（该文档已经指出这一点：布尔交集满足交换律，不存在"先禁后禁结果不同"的问题）。

**分两段看**：

- **接口占位，现在就能做，零风险**：`ActionCapability` 类型定义 + `resolve_move`/`resolve_attack` 顶部插入 `current_capability` 检查点——`current_capability` 在 `active_buffs` 字段落地之前恒返回 `ALL`，接上检查点不改变任何现有测试结果，是"先把接缝焊好，再等内容"的低成本先手棋（该文档五节结论）。
- **真正生效**：需要某个技能效果真的能产出一条"限制了某类行动"的记录。**这一步不依赖 `TraitTable` 落地**——与六节的教训一致：`restricts: ActionCapability` 这个字段的数值可以像 `TemporaryStatModifier` 现在这样直接内联在技能/触发器定义里（"眩晕术"这个技能自己声明"施加时 `restricts = ALL`，持续 3 回合"），不需要先有一份"可被多处复用、有名字的具体天赋"才能工作——走 `TraitTable` 引用复用是更长远、更 DRY 的形态（多个不同的眩晕来源共享同一份"眩晕"定义），但不是"能不能做"的前提条件。

### 7.3 抗性变化：**本节已过期，抗性早已落地**（更正于抗性多来源聚合批次）

> **过期声明（抗性多来源聚合批次核实并更正）**：本节此前写着「抗性仍然做不了，卡在 `TraitTable`/`TraitDef`/`effective_traits` 全体零实现」——**这条结论已经不成立**。天赋抗性早在「伤害类别/抗性接线批次」就已落地并跑通端到端：`ll_sim::traits::RuleModifier::Resistance` 是真实类型，`ll_sim::rule_modifier::resistance_multiplier_permille` 是真实函数，`ll_sim::resolve::resolve_attack` 在减伤链路之后真的乘上它，`mods/example_mod/gameplay.scm` 的 `examplemod:acid_hide`（酸抗 500‰）+ `examplemod:ooze` 种族是真实内容，`crates/ll-mod/tests/example_mod_resistance.rs` 是端到端证据。`TraitTable`/`TraitDef`/`effective_traits` 三者也早已全部存在。
>
> 抗性多来源聚合批次进一步接上了**第二路来源（装备）**：`ll_mod::item::ItemDef.rule_modifiers` + 脚本 API `register-item-resistance`，聚合点是 `ll_sim::rule_modifier::agent_rule_modifiers`。项目所有者对抗性来源的完整裁定是「抗性肯定会来自天赋，以及装备，还有各种药品，或者技能」四路，**目前接了前两路**；技能（缺 `SkillDef.rule_modifiers`）与药品（缺一个按 `damage_category` 分类的限时容器）两路仍未接，理由见 `agent_rule_modifiers` 文档「接第三、第四路来源时改哪里」一节。
>
> 下面两条是当时的原文，保留作为「阻塞点判断随基础设施推进而变化」的记录，**不再是现状**：

- **`AttributeKind` 缺抗性变体这条路径已经被完全绕开，不再是阻碍**——`trait-system.md` 三节③新增的 `RuleModifier::Resistance { damage_category: ContentIndex, multiplier_permille: i32 }` 走的是声明式规则修正（一档，ADR 0016/0017），挂在 `TraitTable`（`resistance_multiplier(defender, damage_category)` 遍历 `defender` 的有效天赋，收集匹配的 `Resistance` 条目取乘数），从头到尾没有用到、也不需要 `AttributeKind` 这个类型——抗性从来就不该是"体质"这类主属性的一个变体（`damage-formula-mod-api.md` 九节早就论证过"体质"是描述性文字，不是按伤害类别分列的抗性表），`RuleModifier::Resistance` 直接按 `damage_category`（`ContentIndex`，`damage-formula-mod-api.md` 十七节已开放的注册表）分类，粒度天然匹配"火抗""冰抗"这类具体类别，不需要经过 `AttributeKind` 这一层。
> **【2026-08-30 复核：下面这条阻塞判断已过期，正文原样保留。】** **天赋系统已经落地，这条阻塞已经解除**：`TraitDef`/`TraitTable` 在 `crates/ll-mod/src/trait_def.rs:85`/`:168`，`effective_traits` 在 `crates/ll-sim/src/traits.rs:304`。另：本文档「落地状态」说 `crates/ll-sim/src/effect.rs`「只有六个既有变体」也已过期——今天约四十个，且增益通道就是其中之一（`crates/ll-sim/src/effect.rs:321` `ApplyStatModifier`）。**仍然成立的是本文档自己的主题**：`ActiveEffect`/`TriggerDef`/`StackPolicy` 零命中。逐条见 [2026-08-29 文档—代码一致性审计](../audit/2026-08-29-doc-code-audit.md) 一节第 1 条。

- **真正卡住的是 `TraitTable`/`TraitDef`/`effective_traits` 这一整套天赋系统本身零实现**——`trait-system.md` 自己的「落地状态」已经写明"纯设计，无实现代码，全代码库检索无 `TraitDef`/`register-trait`/`TraitTable` 任何匹配"。`RuleModifier::Resistance` 给出的是"乘数从哪来"这个此前缺失的规则**形状**，不是可以立刻运行的代码——它依赖的 `effective_traits(agent, world)`（天赋系统六节的并集算法）同样是纯设计。`damage-formula-mod-api.md` 二十节已经把挂载点定死在"减伤之后、乘数形式"，现在**规则来源**也有了形状（`RuleModifier::Resistance`），但**这条链路从"伤害类别"到"实际乘数"中间的每一环——`TraitTable` 本身、`effective_traits`、`resistance_multiplier`——都还没有一行代码**，抗性因此仍然是四类里唯一"做不了"的一类，只是阻塞点从"没有规则形状"变成了"规则形状有了，但它依赖的整套基础设施还不存在"。

### 7.4 触发式效果（命中时中毒）：框架本体能做，`ApplyBuff` 档卡在与抗性同一处

三节/四节已经设计好触发器的分档、形状与"队列 + 深度上限"的防递归调度——这套**框架本体**（`TriggerDef`/`TriggerKind` 分发、效果队列循环、深度上限截断、`TriggerResponse::DealDamage`——吸血/反伤只携带裸数值，不引用任何注册表内容）**不依赖 `TraitTable`，现在就能做**。

但项目所有者点名的具体例子——"命中时附加中毒"——是 `TriggerResponse::ApplyBuff { def, stacks, duration_ticks }` 这一档，`def` 解析进 `TraitTable`（本节新补，见九节）。**这意味着"命中时附加中毒"这个具体例子，若要求"中毒"是一份可以被多处引用、有名字的具体效果内容（今天这个技能引用它，明天那把武器也引用它，改一次数值处处生效——三节 DRY 论证的原始理由），就和抗性卡在同一个依赖上（`TraitTable`）**。

**能不能绕开**：能，走六节/7.2 已经用过的同一条"内联"套路——一个具体的"命中时中毒"触发器可以不引用 `TraitTable`，把中毒的数值（每 tick 掉多少血、持续几回合）直接内联在这个 `TriggerDef` 自己身上，代价是"中毒"从此不是一份可复用的具体内容，而是散落在每个引用它的触发器定义里各自一份——若未来有五个不同的技能都想有"命中时中毒"，就要各自内联五份等价但独立维护的数值，容易漂移（五节论证 `StackPolicy` 走声明式而非硬编码时用的正是同一个"不复用会导致漂移"的理由）。**结论：框架本体 + 内联版本的具体触发器现在就能做；要它们真正做到"同一份中毒定义被多处复用"，需要等 `TraitTable`**。

### 小结：对项目所有者初判的核实结论

**方向基本正确，两处需要精确化**：

1. ~~**抗性**：确实做不了，但原因已更正——不再是 `AttributeKind` 缺变体（这条路已被 `RuleModifier::Resistance` 绕开），是 `TraitTable`/`TraitDef`/`effective_traits` 全体零实现。~~ **已过期**：抗性（天赋一路）早已落地，装备一路也已接上，见 7.3 顶部的过期声明。
2. **触发式效果不是铁板一块的"能做"**——触发器框架本体、`DealDamage` 档、以及任何愿意先接受"内联、不可复用"这个简化版本的具体触发器（含"命中时中毒"的内联版本），现在都能做；但"命中时中毒"若要求它是一份可复用的具体内容（`ApplyBuff.def` 真正指向一条 `TraitTable` 记录），就和抗性卡在完全同一个依赖上。
3. **持续伤害、行动限制**：确认可以现在开始做，但同样是"内联版本现在能做，走注册表引用复用的版本要等 `TraitTable`"这同一个模式——六节的存储改法已经证明了这个模式本身可行（`TemporaryStatModifier` 从一开始就是这套"内联，不查注册表"路线的既有实现）。

---

## 八、落地顺序

**依赖关系归纳成一句话**：`TraitTable`（`trait-system.md`）是本节所有条目里唯一一个"落地后能同时解锁两类原本各自独立卡住的东西"的投资——抗性（7.3）与"可复用版本的触发式效果"（7.4）都卡在它上面。除此之外的每一项，都可以走"内联，不查注册表"的路线立刻开始，与 `TraitTable` 是否落地无关。

| 顺序 | 项目 | 依赖 | 现在能不能做 | 改动面 |
|---|---|---|---|---|
| 0（最优先） | `active_stat_modifiers` 存储改法（六节，多来源叠加） | 无——只需要已落地的 `ActiveStatModifier`/`Effect::ApplyStatModifier`/`effective_attribute` 本身 | **已落地**（抗性多来源聚合批次核实：`ll_world::entity::Agent::active_stat_modifiers` 现在就是 `BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>`，`derive_stats` 对内层逐条过滤未过期项再求和，六节裁定的「同源刷新、异源叠加」已经是运行中的代码，不再是待办） | `ll-world`（`Agent.active_stat_modifiers` 字段形状换成双层 `BTreeMap`）；`ll-sim`（`Effect::ApplyStatModifier` 新增 `source: ContentIndex` 字段、`apply()` 里的写入逻辑改用 `merge_same_source`、`effective_attribute` 改成对内层 map 的过滤求和、`resolve_use_skill` 把已经持有的 `skill` 参数原样传入新字段，不需要新查表）；要动 `WorldState::hash()`（`state.rs:927`~`932`，多一层遍历）；**不需要存档迁移**（schema v1，迁移链已清空，六节已确认） |
| 1 | 持续伤害（7.1，DoT，走内联路线） | 架构分岔的结论（本文档 7.1 已给出：挂在既有 `on_turn_start`/`next_action_at` 调度节点上，不新开时间轴） | **能**，不卡任何未落地系统 | `ll-sim`（新 `SkillEffect`/`Effect` 变体承载"每 tick 掉多少血、持续几回合"这两个内联数值；`TurnEngine`/回合推进入口顺带检查到期该结算的 DoT，产出 `Effect::Damage`）；`ll-world`（`Agent` 新增一个专用容器，形状可以直接照抄六节"按来源做键，同源刷新"的容器纪律）；要动 `hash()`；**不需要**改 `TurnEngine` 的调度结构本身（复用既有 `next_action_at` 节点，不新增时间轴条目） |
| 2 | 行动限制（7.2，走内联路线） | `ActionCapability` 类型（`action-capability-and-input-context.md` 已给出完整形状）——占位版本零依赖；真正生效同样走内联，不依赖 `TraitTable` | **能**，分两段：类型定义 + `resolve()` 检查点可以立刻接（零风险，恒返回 `ALL`）；某个具体技能（如"眩晕术"）真的产出限制效果，同样现在能做 | `ll-platform` 不变；`ll-sim`（`ActionCapability` 类型、`resolve_move`/`resolve_attack` 顶部检查点、新 `SkillEffect` 变体）；`ll-world`（新容器，同样照抄六节纪律）；要动 `hash()` |
| 3 | 触发器框架本体（7.4，`TriggerDef`/队列+深度上限/`TriggerResponse::DealDamage`） | 无——`DealDamage` 档不引用任何注册表内容 | **能** | `ll-sim` 新增比 `resolve`/`apply` 更外层的一个调度层（四节已给出队列伪代码：效果进队列、`apply` 逐条消费、`resolve_triggers` 判定后续触发、深度上限截断） |
| 4 | 触发器 `ApplyBuff` 档（7.4，"命中时附加中毒"的可复用版本） | `TraitTable`/`TraitDef`/`effective_traits`（`trait-system.md`，零实现） | **不能**，卡在这里——愿意接受内联退化版本的话可以并入第 1 项现在做 | 待 `TraitTable` 落地后另行评估，本文档不预先设计 |
| 5 | 抗性（7.3） | ~~`TraitTable`/`TraitDef`/`effective_traits` + `RuleModifier::Resistance`/`resistance_multiplier`~~ 依赖全部已满足 | **已落地**（天赋一路：伤害类别/抗性接线批次；装备一路：抗性多来源聚合批次）——所有者裁定的四路来源里还剩技能、药品两路 | 已实现：`ll_sim::rule_modifier`（聚合点 + 两个消费者）、`ll_sim::resolve::resolve_attack`（挂载点）、`ll_mod::script_trait_api::register_trait_resistance` / `ll_mod::script_item_api::register_item_resistance`（两路来源各自的注册入口）。剩余两路的具体缺口见 `agent_rule_modifiers` 文档 |

**为什么 0 排在最前**：不是因为它比其余四类更"重要"，是因为它改的是**已经在跑的代码**——`active_stat_modifiers` 从六节改法落地那一刻起，就会成为第 1、2 两项"内联容器"要照抄的存储纪律范本（"按来源做键，同源刷新"）。若先做第 1、2 项、再回头改第 0 项的存储形状，等于让新写的两个容器先按旧纪律实现一遍，再跟着第 0 项返工一次——**先定好"多个修正怎么共存"这一条通用规则，后面每一类新增的效果容器都直接照抄，不需要各自重新发明一遍，也不需要事后回补**。

---

## 九、遗留缝合：`ActiveEffect.def` 指向 `TraitTable`

`knowledge/design/trait-system.md`（刚定稿）指出：本文档从未定义过 `ActiveEffect.def: ContentIndex` 解析到什么类型——一节给出了 `ActiveEffect` 的完整形状，却从头到尾没有回答"这个索引指向哪张表"。天赋设计给出了答案：**`def` 指向 `TraitTable` 的条目（`TraitDef`）**，且已在该文档二节完整论证：

- `ActiveTraitInstance`（该文档二节新增）就是本文档的 `ActiveEffect`，字段完全对应（`def`/`expires_at`/`stacks`/`applied_at`/`source`）——该文档只是第一次给 `def` 指定了具体类型，没有改动本文档任何一节的论证（惰性到期判定、确定性合并顺序、`StackPolicy`、触发器深度上限全部原样成立）。
- **天赋与 buff 共享同一张 `TraitTable`，但不共享实例化机制**——种族/职业/副职/装备授予的天赋是纯引用（`owner.traits: Vec<TraitGrant>`，查表即得完整效果，零实例存储，"你还是这个种族"这件事本身就是天赋仍然生效的全部凭据）；buff（技能/触发器施加的限时状态）必须实例化（`Agent.active_traits: Vec<ActiveTraitInstance>`，`expires_at` 是真实偏差，必须存储、必须进 `hash()`）。**项目所有者原话「一个 buff 就是有到期时间的天赋，一个天赋就是永不过期的 buff」在效果载荷这一层完全成立——两者读的是同一张表；在"要不要实例化"这一层不成立——天赋从不实例化，buff 必须实例化**，理由是给天赋也造一个 `expires_at: 永远` 会制造一个没有自然表示的哨兵值（`Tick::MAX` 能工作但没有意义，且每次比较都要为一个不会真的发生的情形买单）。

**本节同时确认与二节的一致性**：`ActiveTraitInstance.def` 与本文档 `ActiveEffect.def` 是同一个字段、同一个类型、同一张表，`trait-system.md` 没有引入任何与本文档冲突的形状——三节已经补上 `TriggerResponse::ApplyBuff.def` 同理解析进 `TraitTable`，本节与三节那处补丁合起来，`ActiveEffect.def` 此前唯一的类型缺口现在完整补上。

---

## 相关文档

- [三轴战斗设计](combat-three-axis.md) — `on_hit` 触发点在伤害结算管线里的确切位置、伤害系别/穿透的复用
- [角色属性系统](attribute-system.md) §七 — `derive_stats` 纯函数签名、衍生属性绝不入存档的原始纪律
- [物品系统](item-system.md) — `use_effect` 脚本接口、`Effect::MoveItem` 少数通用原语覆盖大量内容的先例
- [ADR 0009 — 默认派生，只存偏差](../decisions/0009-derive-by-default-store-only-deviation.md)
- [ADR 0014 — 季节维持纯函数派生](../decisions/0014-season-pure-function-derivation.md) — 惰性判定 vs 事件驱动的同构论证
- [ADR 0012 — Steel 标准库能力面实测](../decisions/0012-steel-capability-surface-verification.md) — 脚本调用开销实测数字
- [ADR 0016 — mod 性能分档按声明方式，不按作者身份](../decisions/0016-mod-performance-tiers-by-declaration.md)
- [ADR 0017 — 声明式分档物化为列式数据，注册期完整校验](../decisions/0017-tiered-declarations-materialize-columnar.md)
- [ADR 0018 — 引擎层与玩法层脚本边界](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) — 触发点时机为何仍是引擎层职责
- [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) §5.5 — 确定性排序平局规则，本文档二、的增益合并顺序复用同一条纪律
- `knowledge/audit/worklist.md` W-05 — HashMap/HashSet 迭代顺序约束编号混乱问题，与本文档"合并顺序必须确定"是同一类非确定性来源
- `knowledge/handoff/p3-to-p4.md`「一、2. 玩家死亡后主循环空转」— `MAX_STEPS_PER_ADVANCE` 兜底真实生效的先例
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) §4（约束 C2/C4）
- [天赋/特性系统](trait-system.md) — 九节 `ActiveEffect.def`/`ActiveTraitInstance` 的缝合来源；三节③ `RuleModifier::Resistance` 是七节 3 抗性依赖链的具体形状
- [行动能力与输入上下文](action-capability-and-input-context.md) — `ActionCapability` 完整形状与 `resolve()` 检查点位置，七节 2 直接复用
- [伤害公式 mod API](damage-formula-mod-api.md) — 二十节抗性挂载点（减伤之后、乘数形式）与十七节 `damage_category` 开放注册表，七节 3 引用
- [载具与骑乘系统](vehicle-and-mounting.md) — 攻防加成复用 `active_stat_modifiers` 通道的第二个真实生产者，六节存储改法对该文档一处既有事实描述的影响已如实标注
- [ADR 0022 — 覆盖不全的确定性哈希等于没有确定性哈希](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 六节 `active_stat_modifiers` 新形状进 `hash()` 的纪律依据
