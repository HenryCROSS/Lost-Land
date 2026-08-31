# 资源池与休息系统：法力池、法术位、血魔法与可中断的休息事件

> **【2026-08-30 复核：下面这条「落地状态」已过期，正文原样保留。】** 这四样**全部已经存在**：`ResourcePoolDef`（`crates/ll-mod/src/resource_pool.rs:27`）、`Intent::Rest`（`crates/ll-sim/src/intent.rs:194`）、`RestState`（`crates/ll-world/src/entity/agent.rs:25`，另有 `:247` `resource_pools`、`:281` `resting`）、`Effect::BeginRest`（`crates/ll-sim/src/effect.rs:483`）。**连本文档说「需要 `trait-system.md` 补一个待办字段」的那个字段也有了**：`TraitAttrs::granted_resource_pools`，`crates/ll-mod/src/content_audit.rs:521`。逐条见 [2026-08-29 文档—代码一致性审计](../audit/2026-08-29-doc-code-audit.md) 一节第 1 条。

**落地状态**：纯设计，`crates/**` 全代码库检索无 `ResourcePoolDef`/`register-resource-pool`/`Intent::Rest`/`RestState` 等任何匹配。

**冻结于** 2026-08-20，基线提交 `234e041`（`main` 分支）。工作区此刻还有另一路并行工作（配置格式统一为 JSON5，改 `crates/ll-mod/`、`crates/ll-platform/src/config.rs`、`mods/`、`assets/sprites/`、`tools/ll-artgen`、`knowledge/design/mod-package-structure.md`）的未提交改动——本文档不触碰上述任何路径，只新增这一个文件 + 更新 `README.md` 索引里属于本文档的那一行。

**并发声明**：`knowledge/design/trait-system.md`（提交 `cfabb93`）与 `knowledge/design/level-and-experience-system.md`（提交 `bd6c3eb`）均已合入 `main`，可以正常读取、引用。**本文档需要 `trait-system.md` 追加第四类天赋效果（三节），但本文档没有该文件的写入权限——补丁需求已在三节精确写明，标注为待办，交协调者处理，本文档不代为修改它。**

---

## 零、项目所有者的要求（五轮）

第一轮：

> 「1.需要有个休息事件。2.需要有一个法术池，血池。这样法师应该就有更好的功能了」

第二轮澄清（纠正"池 vs 位二选一"这个错误前提）：

> 「1.加入法术位。2.我说的法术池是指 mana 之类的东西」

即法力池、法术位、血池三者共存，不是三选一。

第三轮澄清（血魔法致死）：

> 「血池把自己扣死听着很有趣，允许了」

第四轮（资源池的归属，直接改变了本文档的骨架）：

> 「或者法术位法术池的解锁是通过职业或者解锁职业被动技能获得？」

即资源池不是每个角色都有的固定字段，而是**天赋授予的能力**——法师的奥术施法天赋授予一族法术位，术士的天赋授予一个法力池。**本文档采纳这个方向，见三节的完整论证。**

第五轮（目的说明）：

> 「这样可以做出两种不同的流派」

**这是本设计的一条明确目标，不是"两种形态都能表达"的副产品**——法术位与法力池要玩起来真的不一样，见四节。

---

## 一、现状核实

**`ResourceKind`/`ResourceCost`/`SkillEffect`**（真相源在 `crates/ll-sim/src/skill.rs`，`crates/ll-mod/src/skill.rs` 直接 `pub use` 复用，是 P5-B 接线批次刚做完的收敛）：

```rust
pub enum ResourceKind { Mana, Stamina }               // 封闭枚举，只有两个变体
pub enum ResourceCost { None, Amount(ResourceKind, u32) }
pub enum SkillEffect {
    DealDamage { base: i32 },
    RestoreResource { resource: ResourceKind, base: i32 },
    TemporaryStatModifier { attribute: AttributeKind, amount: i32, duration_ticks: u32 },
}
```

`ResourceKind` 是封闭 Rust 枚举，不是走 `ContentIndex` 的开放注册表——mod 想注册"气""怒气"这类新资源族，现有形状完全做不到。

**`Agent.mana`/`Agent.stamina`**（`crates/ll-world/src/entity/agent.rs`）：两个专用 `i32` 字段，只存当前值不存上限，已进 `WorldState::hash()`（`state.rs:918-919`）——**这正是本文档要拆掉的"每个角色都隐式拥有"这条前提**，见三节。

**`Agent.health`**（同文件）：

```rust
/// 当前生命值。可以为零或负——是否算「死亡」是规则判断（`resolve`
/// 的职责），`apply` 只管照 `Effect::Damage` 的数字做加减，不在这
/// 个字段本身设下限。没有独立的「上限」字段。
pub health: i32,
```

`health` 本来就允许降到零以下，死亡判定从来不是字段自身的约束——用血施法把自己打死不需要新东西去"允许"它，真正要设计的是代价怎么进管线（五节）。

**死亡判定的既有形状**（`crates/ll-sim/src/resolve.rs`）：`resolve_attack`/`resolve_use_skill` 各自在产出伤害效果之后，用结算前读到的 `defender.health - damage <= 0` 判断是否追加 `Effect::Kill { target, killer, cause }`——本文档血代价致死判定直接复用这条既有纪律。

**`Effect` 现有相关变体**（`crates/ll-sim/src/effect.rs`）：`Effect::Damage { target, amount }`（固定"造成伤害"语义）、`Effect::AdjustResource { target, resource: ResourceKind, delta: i32 }`（增减皆可，`RestoreResource`/施放消耗共用同一个变体）。**没有 `Effect::Heal`**——生命值的正向调整目前无法表达。

**`trait-system.md` 已定的天赋形状**（四、六节）：

```rust
pub struct TraitDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId,
    pub granted_skills: Vec<ContentIndex>,
    pub stat_modifiers: Vec<(AttributeKind, i32)>,
    pub rule_modifiers: Vec<RuleModifier>,
}
pub struct TraitGrant {
    pub trait_id: ContentIndex,
    pub unlock_level: u32,   // 种族/副职/装备/buff 恒填 1；职业天赋按等级曲线填
}
pub fn effective_traits(agent, world) -> Vec<ContentIndex>   // 现算，不缓存进存档
```

**`level-and-experience-system.md` 已定的等级形状**（二节）：`Agent.level: i32` 是**角色总等级，单一字段，不按职业拆分**——多重职业不会产生"这个数该用哪个职业的等级"这种歧义,全部资源池容量公式统一读同一个 `agent.level`。

**约束 C4**：管的是离屏世界的跳跃式推进,不是本文档要处理的场景（休息发生在前景,世界不跳过任何 tick,见七节）。

**`DetRng::for_entity(world_seed, entity_id, event_counter)`**：随机数由三元组纯函数计算得出——第八节休息防刷的关键论据。

---

## 二、三种资源形态怎么统一表达

**结论：注册身份层统一（`ResourcePoolDef`），消费算法不统一，血池完全独立表达。**

把三者的存储/消耗/恢复算法摆开看：

| | 存储 | 消耗算法 | 恢复算法 |
|---|---|---|---|
| **法力池**（标量） | 单个当前值 | 减去固定数量 N | 见四节 |
| **法术位**（分级槽位） | 每档一个已消耗数 | 在 ≥ 请求档位的最低一档里找空位占用 | 见四节 |
| **血池** | 就是 `Agent.health` | 直接减固定数量,**绕开减伤/抗性** | 治疗效果（不是本文档设计对象） |

**法力池的消耗算法与法术位的消耗算法不是同一个函数**——若强行套进"一族有序池,从最低满足阶开始取"这一个框架,会让法力池被迫携带一个恒定长度为一的 `Vec` 去配合一套它永远用不到的查找逻辑,与 `trait-system.md` 五节否决"亚种照抄副职"是同一把尺子（ADR 0021：表面对称,算法不同,不该套同一个形状）。**这里没有可共享的消耗算法,不统一消耗层。**

血池更极端：连存储都不新开（复用 `health`），消耗还要求绕过另外两者都要走的减伤管线——参与"有序池族"框架对它没有好处，反而会诱使人漏掉"必须绕开减伤"这条硬性要求。**血池完全独立表达，不进任何池的框架**（详见五节）。

真正共享的是**注册身份层**——「这是一种可被 mod 注册、可查询、可在技能表里被引用的资源身份」，与消耗算法无关，是纯粹的内容注册表问题，与 `TraitDef`/`RaceDef`/`SkillDef` 面对的是同一类需求：

```rust
/// 一种可注册的资源池——法力、耐力、气、法术位……的共同身份。走一档
/// （纯数据，注册期物化，运行期查表，ADR 0016/0017）。
/// 注意：本类型只描述"这种资源长什么样、怎么恢复"，不描述"谁能有
/// 多少"——后者是三节 ResourcePoolGrant 的职责，两者刻意分层。
pub struct ResourcePoolDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId,
    pub shape: ResourcePoolShape,
    pub regen_rule: RegenRule,      // 四节，与 shape 正交
}

pub enum ResourcePoolShape {
    Scalar,                                    // 标量池：法力、耐力、气……
    TieredSlots { tier_count: u8 },             // 分级槽位族：法术位
}
```

`ResourceCost` 三个变体（`Amount`/`SlotTier` 共用这张注册表，`Blood` 完全独立）：

```rust
pub enum ResourceCost {
    None,
    Amount(ContentIndex, u32),       // 标量池：付固定数量
    SlotTier(ContentIndex, u8),      // 法术位：付某族第 tier 档（或更高档）的一个槽位
    Blood(u32),                       // 血代价：直接扣 health，见五节
}
```

**「从最低阶开始取」是引擎规则，不做成可配置**：`SlotTier(pool, min_tier)` 消耗时，`resolve` 从 `min_tier` 起往上找第一个"上限（三节现算）> 已消耗数"的档位占用；找不到则不产出效果（与 `SkillCatalog` 文档"技能不存在"与"条件不满足"同等对待、都不产出效果的既有纪律一致）。不支持玩家手选具体档位——`ResourceCost` 挂在 `SkillDef` 上是静态定义，"这一次想扣哪档"是每次施放各自的选择，硬塞进静态定义不合适；真要支持，需要改 `Intent::UseSkill` 的字段形状，波及既有测试，无真实需求驱动，YAGNI。

**升阶施法的效果加成——不做**：项目所有者倾向不做，且结构上 `SkillEffect` 的三个变体全是注册期定死的静态值，不读"这次实际扣的是第几档"这个运行期信息，与 `trait-system.md` 的 `ConditionKind` 占位处理同构：先不加，需要时再加。

---

## 三、资源池由天赋授予：第四类天赋效果

**结论：采纳。资源池不是 `Agent` 上人人都有的固定字段，而是天赋（种族/职业/副职/装备/buff）授予的能力——`TraitDef` 需要新增第四类效果 `granted_resource_pools`。这需要 `trait-system.md` 追加一节，本文档没有写权限，在本节末尾精确列出补丁需求，标注待办。**

### 为什么采纳：这是对既有三类效果的自然扩展，不是新发明

`trait-system.md` 三节已经定了天赋能授予三类东西：**技能**（`granted_skills`）、**属性修正**（`stat_modifiers`）、**规则修正**（`rule_modifiers`）。资源池的"容量"在性质上与这三类完全同构——都是"拥有某个天赋就获得的一份能力/数值"，唯一的区别是这份数值需要按等级查表（见下）。**没有授予对应天赋的角色就是没有这个池，`effective_traits(agent)` 找不到匹配的授予关系时，容量恒为零——这是"没有则为零"的自然结果，不需要任何特判去区分"这个角色有没有法术位"，与四节①"有效技能=并集，查不到就是没有"是同一条纪律。**

### 形状：`ResourcePoolGrant` + `CapacityFormula`

```rust
/// TraitDef 新增第四个字段（补丁见本节末尾）。
pub granted_resource_pools: Vec<ResourcePoolGrant>,

/// 一条"这个天赋授予多少这种资源池容量"的声明。
pub struct ResourcePoolGrant {
    pub pool: ContentIndex,          // 指向 ResourcePoolDef（二节）
    pub capacity: CapacityFormula,
}

pub enum CapacityFormula {
    /// 容量恒定，不随等级变化——血魔法许可、多数标量池的简单情形。
    Fixed(u32),
    /// 容量随 `Agent.level`（level-and-experience-system.md，单一总
    /// 等级，不按职业拆分）查表——法术位"5 级 4 个一环位、9 级追加
    /// 五环位"这类阶梯式增长的直接落点。键是等级，值的形状与
    /// `ResourcePoolShape` 匹配；表未覆盖的等级取小于等于它的最大
    /// 已声明等级对应的值（阶梯式，不需要每级都填，对应 D&D 官方表
    /// "有些等级没有新增"）。
    ByLevel(BTreeMap<u32, CapacityValue>),
}

pub enum CapacityValue {
    Scalar(u32),
    Tiered(Vec<u32>),   // 每档一个数，索引 0 = 第 1 档
}
```

**为什么不复用 `TraitGrant.unlock_level` 来表达"5 级获得三环位、9 级获得五环位"这条曲线**：`TraitGrant.unlock_level`（`trait-system.md` 六节）回答的是"这条**授予关系**几时开始生效"——对法师而言，"拥有法术位这件事本身"从 1 级就成立（`unlock_level = 1`），不需要每级都开一条新的 `TraitGrant`；**真正逐级增长的是"容量数值"，不是"有没有这个池"，两者是不同的轴**：前者用一条 `TraitGrant` 表达（成立与否），后者用 `CapacityFormula::ByLevel` 表达（成立之后具体是多少）。若强行只用 `unlock_level` 阶梯式表达容量增长，需要给同一个池注册几十条几乎重复的 `TraitDef`（"法师 1 环位+1""法师 1 环位+2"……），既冗余又难维护——这是本节对项目所有者原始提法的一处精确化，不是推翻。

**血法师的"血池使用许可"不需要这套机制**：血池没有独立容量（就是 `health`，见五节），"能不能用血代价"完全由"会不会某个 `resource_cost` 是 `Blood(N)` 的技能"决定——而"会不会某个技能"已经是天赋效果第一类 `granted_skills` 在管的事，不需要在第四类里再造一层重复覆盖它。**`granted_resource_pools` 只服务需要一个数字（容量）的资源——标量池与法术位，不服务血池。**

### 上限派生，当前值/已耗值存储——「默认派生，只存偏差」的又一次复用

```rust
// Agent 新增字段（存储形状不因"容量从哪来"而改变）
pub resource_pools: BTreeMap<ContentIndex, i32>,        // 标量池：当前值（绝对量，非偏差）
pub spent_slots: BTreeMap<(ContentIndex, u8), u32>,     // 法术位：已消耗数（偏差量）
```

`effective_capacity(agent, world, pool) -> CapacityValue`：遍历 `effective_traits(agent, world)`（`trait-system.md` 既有函数，直接复用，不重新实现聚合逻辑），对每一条匹配 `pool` 的 `ResourcePoolGrant`，按 `agent.level` 求值出一个数量，**全部命中项求和**——与 `trait-system.md` 三节② `stat_modifiers` 的叠加语义一致，不是 `Resistance` 那种"取第一条命中"的语义（`Resistance` 取第一条是为了避免"免疫乘免疫"的荒谬结果，容量则是"两个来源各自贡献一部分"的自然叠加，两者语义不同，不能照搬同一条规则——这里刻意区分，不是疏漏）。**容量本身不存储，每次查询现算**——`Agent` 上只有 `resource_pools`/`spent_slots` 这两个"当前偏离了多少"的字段，与 `health`/`mana` 只存当前值不存上限、`active_stat_modifiers` 只存增量不存最终属性值是同一条纪律的又一次复用。

**上限变化时怎么办——读时钳位，不主动改写存储值：**

- **上限变大**（升级、新装备）：`stored_current`/`stored_spent` 不动，不自动补满/不自动清空——升级本身不是一次隐式治疗/�feedback，只是让"可用上限"变大，`stored_current` 与新上限之间的差距要靠既有恢复规则（四节）自然填上。
- **上限变小**（掉装备、天赋失效）：**不回写存储值**，改为在每次真正读取"当前可用量"时现场钳位：标量池 `usable = min(stored_current, effective_cap)`；法术位 `remaining(tier) = effective_cap(tier).saturating_sub(stored_spent(tier))`（`saturating_sub` 天然把"已耗超过新上限"钳成零剩余，不需要额外分支）。**不主动改写的理由**：若把"上限变小"这件事做成"立刻遍历全部池、把 `stored_current` 砍掉重写"，需要一套"天赋/装备变化时通知资源池"的观察者机制，这正是 `buffs-and-triggers.md`"惰性判定优于事件驱动"（约束 C4 同一条精神）已经反复论证过的错误方向——查询时现比较一次，比维护一套变化通知便宜得多，也不会因为漏发一次通知而产生"钳位没生效"的隐患。**副作用是良性的**：卸下装备再穿回，`stored_current`/`stored_spent` 原样还在，不会因为中途被临时钳位过就永久损失——这与"任何天赋的效果都该是查询时现算，不该在天赋消失的瞬间销毁与它无关的持久状态"是一致的。

**重算频率——与 `effective_traits`/`resistance_multiplier` 同一档，不是热路径：**`effective_capacity` 只在两处被调用：①技能结算时检查/扣减资源（`resolve_use_skill`，每次 `Intent::UseSkill` 一次）②回合开始的自动恢复检查（四节 `OnTurnStart`，每个实体自己的回合一次）——**与 `trait-system.md` 已经接受的 `effective_traits`（"每次要用时现算，不缓存进存档"）、`resistance_multiplier`（"减伤链路里逐次攻击查一次"）完全同一个调用频率量级**，不是逐格/逐帧的热路径，不需要额外优化或缓存。

### 血池的容量不破坏这条统一——它反而是最早的先例

血池的"容量"是最大生命值，不是天赋授予的——但这**不破坏**"上限派生、当前值存储"这条原则，恰恰相反：`Agent.health` 从最初设计起就是"只存当前值，上限由体质经 `derive_stats` 现算，不存字段"（一节引用的字段文档原文）。**血池的容量公式输入是体质衍生（既有机制，尚未落地）而不是天赋容量表（本节新机制），两条路径不同，但都遵守同一条"不存上限"的纪律**——血池不是这条统一之外的特例，是这条统一最古老的既有实践，本节的 `ResourcePoolGrant` 反而是照着 `health` 已经确立的模式重新做了一遍，不是反过来。

### 需要 `trait-system.md` 追加的补丁（待办，本文档无写权限）

- `TraitDef` 新增字段 `granted_resource_pools: Vec<ResourcePoolGrant>`（本节定义），与既有 `granted_skills`/`stat_modifiers`/`rule_modifiers` 并列，四节"天赋效果的三类表达"标题需要改成"四类表达"。
- 四节聚合小节需要补一句：`effective_capacity(agent, world, pool)` 复用 `effective_traits` 做聚合，求和而非取第一条命中（与 `Resistance` 的取第一条区分开）。
- 六节 `TraitGrant.unlock_level` 文档需要补一句区分："容量随等级增长"不通过多条 `TraitGrant`，通过单条 `ResourcePoolGrant.CapacityFormula::ByLevel`，`unlock_level` 只回答"这个池存在与否从几级开始"，不回答"存在之后有多少"。
- 十节"现在能做的 vs 等什么"需要把本文档列为新的依赖方。

与该文档二节当初处理 `buffs-and-triggers.md` 的 `ActiveEffect.def` 缺口同一种姿态：指出精确缺口、给出精确补丁形状，不越权代为修改。

### 依赖链变成三层：资源池 → 天赋 → 等级

本文档定义的容量公式读 `agent.level`（`level-and-experience-system.md`，纯设计，无代码），容量归属又依赖 `TraitDef.granted_resource_pools`（`trait-system.md`，纯设计+待办补丁）——**三层全部是纯设计，最底层（等级）不落地，上面两层在实现意义上都动不了**。十一节"现在能做的 vs 等什么"给出精确清单。

**不做"先在 `Agent` 上放固定字段、等天赋落地再改成派生"这个过渡版本**：项目所有者倾向不要，理由也站得住——固定字段版本的消费/校验代码路径（"读一个字段判断够不够"）与派生版本（"遍历 `effective_traits` 找匹配的 `ResourcePoolGrant`，按等级求值再求和"）不是同一段代码的两个版本，是两套完全不同的查询逻辑，过渡版不会节省后续工作量，只会让"资源够不够"这个判断被写两遍、第一遍很快作废。**纯设计阶段没有"跑不起来"的成本，只有实现阶段才有，而实现本来就要等等级系统先落地——现在按最终、正确的形状把设计定下来，不为了一个不存在的"先能跑"的好处牺牲长期正确性。**

---

## 四、恢复节奏：两种流派的真正差异，可配置，不写死

**设计目标（项目所有者原话："这样可以做出两种不同的流派"）：法术位与法力池玩起来必须真的不一样，差异点不在"分不分格子"这个存储细节，在恢复节奏本身。**

| | 恢复节奏 | 玩法后果 |
|---|---|---|
| 法术位（典型配置） | 休息时一次性回满 | 两次休息之间是定量配给，逼玩家规划："这个三环位现在用还是留着" |
| 法力池（典型配置） | 每隔固定 tick 数回复固定量 | 节奏问题：打一阵等一阵，不需要规划总量，但单位时间内的爆发受限 |

`RegenRule`（二节 `ResourcePoolDef` 的字段，与 `ResourcePoolShape` **正交**——刻意分成两个独立字段，不是"形状决定恢复方式"这种写死的对应关系）：

```rust
pub enum RegenRule {
    None,                                    // 不自动恢复，只能靠主动效果/休息
    OnTurnStart { amount: u32 },              // 每次该实体自己的回合开始时恢复固定量
    OnRest { amount: RestRecoveryAmount },     // 休息完成时恢复
    ResetOnLeaveCombat,                        // 脱战清零（占位，见六节）
}
pub enum RestRecoveryAmount { Full, Amount(u32) }
```

**正交性是刻意的设计目标，不是副产品**：`ResourcePoolShape::Scalar` 配 `RegenRule::OnRest` 得到"回满的法力池"，`ResourcePoolShape::TieredSlots` 配 `RegenRule::OnTurnStart` 得到"缓慢回复的法术位"——**两种反过来的组合结构上同样合法，mod 可以自由声明**，引擎不在任何地方假设"位就该长休回满、池就该缓回"。举例证明这不是空话：

```scheme
;; 反过来的组合一：缓慢回复的法术位（某个 mod 的"自然法师"想要更宽松的节奏）
(register-resource-pool "yourmod:druid_slots"
  "yourmod:pool.druid_slots.display_name"
  (tiered-slots 6)
  (regen-on-turn-start 1))          ; 每回合自动回一个（回哪一档由消耗逻辑同一套"最低阶优先"规则处理）

;; 反过来的组合二：休息回满的法力池（某个 mod 想要术士也有"配给感"）
(register-resource-pool "yourmod:disciplined_sorcery"
  "yourmod:pool.disciplined_sorcery.display_name"
  'scalar
  (regen-on-rest full))
```

两者都直接落在既有的 `ResourcePoolDef`/`RegenRule` 形状上，不需要引擎新增任何代码路径——**这正是"注册身份层统一"（二节）留出的自由度**：形状（怎么存、怎么消耗）与恢复节奏（什么时候、恢复多少）是两个独立的轴，mod 在两个轴上各自自由组合，四种组合引擎一视同仁。

**恢复触发点复用既有机制,不新开一条通知链路**：`OnTurnStart` 与 `buffs-and-triggers.md` 的 `on_turn_start` 触发点是同一个挂载时机（持续伤害用它,资源回复也用它）；`OnRest` 挂在七节"休息完成"这一刻产出的效果批次里。两者都由 `resolve` 判断触发时机（游戏逻辑,C1),产出的效果是 `Effect::AdjustResourcePool`/`Effect::AdjustResourceSlot`（五节以下),不绕开既有 `Effect`→`apply` 管线（ADR 0023 同一条精神）。

---

## 五、血池：`health` 本身，不是新开的第三个池

**结论：血池就是 `Agent.health`，不新开存储**——理由见一节，`health` 本来就没有下限、死亡判定本来就是规则层的事。

### 血代价必须绕开减伤/抗性——独立效果，不复用 `Effect::Damage`

**核实成立**：`Effect::Damage` 携带的 `amount` 是 `resolve_attack`/`resolve_use_skill` 已经跑完 `damage_after_defense`（固定减+百分比减+10%下限，`crates/ll-sim/src/combat.rs`）算出来的**最终**数字——若血代价复用这条路径，防御高的角色施法就会变得更便宜，这是规则错误。**新增独立效果，直接扣 `health`，不查任何防御/抗性表**：

```rust
Effect::SpendBloodCost { target: EntityId, amount: i32 }
```

`apply` 侧：`agent.health -= amount`，无条件——不调用 `damage_after_defense`，不查 `resistance_multiplier`（`trait-system.md` 三节③），不读 `active_stat_modifiers`。**刻意不接入，不是漏写**：血代价链路必须从一开始就不产出 `Effect::Damage`。

### 血代价不算"受到伤害"，不触发 `on_damaged`

**结论：不算。** `on_damaged`（"受伤反击/荆棘甲"）建模的是"被外部/敌意来源命中"的连锁反应；血代价是施法者自己选择付出的资源，语义上是"消耗一种资源，恰好这种资源是生命值"，不是"被击中"。结构上这个区分免费获得：`on_damaged` 挂在 `Effect::Damage` 这个具体变体上，血代价走独立的 `Effect::SpendBloodCost`，天然不会触发任何键在 `Effect::Damage` 上的触发器，不需要额外特判。

### 用血施法致死：复用既有 Kill 判定纪律，不留兜底

`resolve` 计算血代价时，与 `resolve_attack`/`resolve_use_skill` 完全同构地，在结算前读 `caster.health - cost.amount <= 0`：

```
若 caster.health - blood_cost <= 0:
    额外产出 Effect::Kill { target: caster, killer: Some(caster), cause: KillCause::Skill { skill } }
```

不设 1 点血兜底，不在施法前拒绝——执行项目所有者的明确裁定。

**`KillCause` 不需要新变体**：`KillCause::Skill { skill }` 已经精确记录用哪个技能杀的，比笼统的"死于血魔法"信息量更大；"自杀还是被别人杀"完全由 `killer == target`（`Effect::Kill`）/`killer == victim`（`KillRecord`）是否相等表达，不需要再造一个信息冗余的判别式。

**`killer` 填施法者自己，不填 `None`**：`None` 目前用于坠落/饥饿这类没有责任方的死因；血魔法自尽的责任方明确是施法者本人，填 `None` 会把它错误归类成"环境致死"，丢失"这是一次主动的资源选择"这条信息。

**具名角色自杀要不要产出完整 `HistoricalEvent`——要，且不需要新分支**：`append_kill_history` 现有判据只看 `victim_agent.remembered_id.is_some()`，不看 `killer` 是谁，`killer == victim` 天然落在既有判据内，直接产出完整记录——规则统一，不为特殊情形开分支的既有纪律的自然结果。

**代价可预先查询**：血代价来自 `ResourceCost::Blood(N)`，`N` 是注册期定死的静态数值（或未来若接入骰子表达式，也是纯函数），施法前读一遍 `resource_cost` 与当前 `health` 就能算出结果，不存在算不出来的情况。UI 要不要施法前警告是 P7 的事，不在本设计范围，形状上未被挡住。

---

## 六、mod 能不能定义新族

**结论：现在有了自然答案——mod 只需要注册一个新的 `ResourcePoolDef` + 一个（或多个）授予它的 `TraitDef`，不需要单独的"资源注册入口"之外的任何东西。「怒气脱战清零」结构上可以声明恢复规则，但现在没有消费者。**

### 「气」（标量池，可现在设计到位）

```scheme
(register-resource-pool "yourmod:ki" "yourmod:pool.ki.display_name"
  'scalar (regen-on-turn-start 1))
(register-trait "yourmod:monk_ki_pool" "yourmod:trait.monk_ki_pool.display_name"
  (list) (list) (list)
  (list (resource-pool-grant "yourmod:ki" (fixed 4))))   ; granted_resource_pools，三节新字段
(register-class "yourmod:monk" ...
  (list (trait-grant "yourmod:monk_ki_pool" 1)))
```

三节 `ResourcePoolGrant`/`effective_capacity` 直接支持，不需要任何新代码路径。

### 「怒气」（脱战清零，结构可声明，运行期无消费者）

`RegenRule::ResetOnLeaveCombat`（四节已定义）现在就能声明，但全代码库/全设计文档检索确认**没有任何"进入/离开战斗"这个状态概念**——不是本设计遗漏，是这个概念本身在整个项目里都还不存在。**这需要先有一个战斗状态系统才有地方接**，与 `trait-system.md` 处理 `RuleModifier::Advantage`（声明占位、如实标注无消费者）同一种处理方式。「战斗中获得怒气」这半件事已有落点：`Effect::AdjustResourcePool` 的 `delta` 可正可负，一次命中触发 `on_hit`、`TriggerResponse` 追加"这个池 +N"的响应，是对既有开放触发器框架的自然扩展，可以现在做；「清零」那一半确实卡在战斗状态系统缺失上。

---

## 七、休息事件：长等待 + 恢复效果，成立，但需要三样小东西

**核实结论：思路基本成立，`Intent::Wait`/`Timeline` 不需要被替换。需要新增：一个 `Intent::Rest` 用来开始会话、`Agent` 上一个 `resting` 状态字段、`resolve_wait` 里追加的完成/中断检查。**

`Timeline` 已经保证：休息中的玩家不断提交 `Intent::Wait`，其余全部实体照常按各自敏捷被正常 `resolve`/`apply`——世界照常推进，不需要任何跳过式快进,这正是"昼夜在变、怪物在动"这条要求天然满足的方式。

**为什么这不是约束 C4 要处理的场景**：C4 管的是离屏世界的跳跃式推进；休息发生在前景，怪物 AI 仍逐 tick 正常 `resolve`，世界没有"跳过"任何一个 tick，只是玩家自己连续提交了很多次 `Wait`——不需要"确定跳到哪个 tick"的推导,因为压根没有跳。

**新增：`Intent::Rest`，只用来开始会话**：

```rust
Intent::Rest { actor: EntityId, target_ticks: u32 },
```

`resolve` 收到它时：若 `agent.resting` 为 `None`，产出 `Effect::BeginRest { actor, target_ticks }` + 一条与 `resolve_wait` 相同的 `Effect::ScheduleNext`。**后续每回合不需要再提交 `Intent::Rest`**——玩家/AI 照常提交普通 `Intent::Wait`，`resolve_wait` 被扩展成：若 `agent.resting.is_some()`，额外检查是否到达 `target_ticks`、是否被打断,据此追加恢复效果批次或什么都不追加。这是对既有 `resolve_wait` 的一次扩展，不是新造一条平行路径。

**休息完成时恢复什么**：遍历该实体 `effective_traits` 命中的每一个 `ResourcePoolGrant`（三节），对其 `regen_rule` 含 `OnRest` 的池产出对应的 `Effect::AdjustResourcePool`/`Effect::AdjustResourceSlot`；生命值是否连带恢复、恢复多少，不属于 `resource_pools` 机制（`health` 不是 `ResourcePoolDef`），需要一个独立的配置口，本文档只声明 `Effect::Heal` 这个执行原语存在（新增最小变体，与 `Effect::Damage` 对称但不做击杀判定、不触发 `on_damaged`），不裁定"休息时该不该用它、用多少"，见十一节"等什么"。

---

## 八、中断与刷恢复漏洞；短休/长休

### 中断怎么表达

```rust
pub struct RestState { pub started_at: Tick, pub target_ticks: u32 }
// Agent 新增
pub resting: Option<RestState>,
```

```rust
Effect::BeginRest { actor: EntityId, target_ticks: u32 },   // apply: 写入 resting
Effect::ClearResting { actor: EntityId },                    // apply: resting = None
```

两种"结束"路径都落到同一个 `Effect::ClearResting`，区别完全在 `resolve` 是否在它**前面**插入了恢复效果批次：正常完成（`world.clock + 本次行动耗时 >= started_at + target_ticks` 且未被打断）先产出恢复批次再 `ClearResting`；被打断（玩家自己提交非 `Wait`/`Rest` 意图；或依赖尚未落地的"战场感知"系统，见十一节）只产出 `ClearResting`，不带恢复。玩家主动取消：`resolve` 顶层分发加一条前置检查——发起者 `resting.is_some()` 且意图不是 `Wait`/`Rest` 时，额外插入不带恢复批次的 `Effect::ClearResting`，与 D&D 长休/短休规则"做别的事就要重新计时"一致。

### 刷恢复漏洞——两条独立防线

**防线一（主防线）**：恢复只在"正常完成"这一刻整批产出，从不按已过时间比例给。「休息一回合、取消」这个序列从头到尾没有让 `world.clock` 到达 `started_at + target_ticks`，因此从不触发正常完成分支，每次都只产出不带恢复的 `Effect::ClearResting`——重复一百次累计恢复量恒为零,没有"按比例"这回事可刷,因为压根不存在比例发放的代码路径。

**防线二（若未来有人想加按比例发放，这条仍然堵得住）**：休息期间的任何随机遭遇判定必须用 `DetRng::for_entity(world.seed, actor.raw_id(), world.clock.0 as u64)`——键是绝对世界时刻，不是"这次休息会话经过了几个 tick"。把一段 10-tick 休息拆成十段 1-tick 休息，每个绝对 tick 上抽到的结果与它属于哪次会话无关，总期望暴露风险不变，拆段不产生"降低风险、保留恢复"的套利空间。

### 短休 vs 长休——不做区分

**结论：不设两档，只有一种"休息"动作**，`target_ticks` 由发起时的 `Intent::Rest` 自带，恢复力度由每个池自己的 `RegenRule::OnRest.amount`（`Full`/`Amount(N)`）决定。项目所有者的要求原文只提了"休息事件"，单数，D&D 的短休/长休分野服务于"频繁小额 vs 稀有完整"这套节奏设计，本项目现在没有内容需要这套精细区分（法术位只需要"消耗多少档"这一件事需要恢复,不需要"短休恢复一部分槽位"这种 D&D 少数子职业才有的特殊能力）。差异化的自由度已经在数据层给出来了（`RestRecoveryAmount`），不需要在动作本身再叠一层分类；未来真有需求，加一个可选标签是纯粹的新增,不影响现在这个更简单的形状。

---

## 九、多重职业：两条资源轨道互不干扰

**能不能同时拥有两种——能，天然支持，不需要任何新机制。** `Agent.subclasses: Vec<ContentIndex>` 本就支持同时持有多个（`class-skill-quest-system.md` 已落地的既有形状），`trait-system.md` 十节已给 `ClassDef`/`SubclassDef` 都加了 `traits: Vec<TraitGrant>` 字段——一个角色的主职业授予法术位、副职授予法力池，`effective_traits(agent)` 的并集聚合（`trait-system.md` 三节①"有效技能=并集"同一个模式）天然把两条授予关系都纳入,不需要为"多重来源"写任何特判。

**施法时怎么决定用哪个付账——不存在这个决定，因为付账对象从来不是临场选的**：`ResourceCost` 挂在 `SkillDef` 上，是每个技能注册时就写死的静态属性，不是"施法"这个动作本身的属性。法师的技能（由法师职业的天赋授予）永远在自己的 `resource_cost` 里写死引用法术位池，术士的技能（由术士副职的天赋授予）永远写死引用法力池——**两条轨道各自独立，从不互相借用，引擎不需要做任何"选哪个"的裁决**，因为要付账的技能是哪一个、这个技能扣哪个池，从注册的那一刻起就已经确定。

**与 D&D 5e 的一处刻意分歧，如实标注**：5e 的多重职业施法者实际上是把两个职业的施法者等级合并算出**一张统一**的法术位表（"合并槽位"规则），比本设计"两条独立轨道各自计数"更复杂。本设计不采用合并槽位——没有真实需求驱动现在就要支持"融合计算"这种精细规则，两条独立轨道已经能表达"一个角色同时是法师又是术士"这个诉求本身（两种资源都有、各自独立管理），YAGNI；若未来有人明确要 5e 那种合并公式，是在 `CapacityFormula` 上新增一个"跨多个授予来源合并计算"的变体,不影响现有两个变体的形状。

---

## 十、进 `WorldState`、进 `hash()`：精确插入位置

### 新增字段

```rust
// Agent 新增字段（建议紧邻 mana/stamina 原有位置，取代它们）
pub resource_pools: BTreeMap<ContentIndex, i32>,       // 标量池当前值
pub spent_slots: BTreeMap<(ContentIndex, u8), u32>,    // 法术位已消耗数
pub resting: Option<RestState>,                         // 七/八节
```

全部整数/`ContentIndex`/`BTreeMap`，满足 ADR 0020 无浮点、ADR 0022 全覆盖要求。**容量本身（`CapacityFormula` 求值结果）不进这里——它不存储，三节已论证。**

### `WorldState::hash()` 的精确插入点

已核实 `crates/ll-world/src/state.rs:876` 起的 `hash()`，`for agent in self.actors.iter()` 循环体内目前有：

```rust
hasher.write_i64(i64::from(agent.mana));
hasher.write_i64(i64::from(agent.stamina));
```

**这两行随 `mana`/`stamina` 两个字段一起被 `resource_pools` 取代，原位置替换为：**

```rust
hasher.write_u64(agent.resource_pools.len() as u64);
for (pool, current) in &agent.resource_pools {          // BTreeMap，键序确定，满足约束 C5
    hasher.write_u64(u64::from(pool.get()));
    hasher.write_i64(i64::from(*current));
}
hasher.write_u64(agent.spent_slots.len() as u64);
for ((pool, tier), spent) in &agent.spent_slots {
    hasher.write_u64(u64::from(pool.get()));
    hasher.write_u64(u64::from(*tier));
    hasher.write_u64(u64::from(*spent));
}
match &agent.resting {
    None => hasher.write_u64(0),
    Some(state) => {
        hasher.write_u64(1);
        hasher.write_i64(state.started_at.0);
        hasher.write_u64(u64::from(state.target_ticks));
    }
}
```

三段紧邻着插，不需要动 `hash()` 函数体的其余任何一行——与 `level-and-experience-system.md` 同一条纪律：`hash()` 是逐字段手写，新增字段不会被自动覆盖（ADR 0022），这里精确给出施工位置。

---

## 十一、法师验收示例

`trait-system.md` 九节示例六原文：「结论：完全无法表达……法师法术位需要一个独立的资源系统设计任务」——本节直接回应，按项目所有者澄清给两版，且体现"天赋授予"这条骨架（不是全局注册就人人可用）。

### 版本一：法术位（D&D 式法师）

```scheme
(register-resource-pool "lostland:wizard_spell_slots"
  "lostland:pool.wizard_spell_slots.display_name"
  (tiered-slots 9) (regen-on-rest full))

(register-trait "lostland:arcane_casting"
  "lostland:trait.arcane_casting.display_name"
  (list) (list)
  (list)                                     ; rule-modifiers 留空
  (list (resource-pool-grant "lostland:wizard_spell_slots"
          (by-level ((1 . #(2 0 0 0 0 0 0 0 0))
                     (3 . #(4 2 0 0 0 0 0 0 0))
                     (5 . #(4 3 2 0 0 0 0 0 0)))))))   ; 示意，非最终数值曲线

(register-class "lostland:wizard" "lostland:class.wizard.display_name" "intelligence"
  (list (trait-grant "lostland:arcane_casting" 1)))    ; 1 级即拥有施法能力，容量按等级现算

(register-skill "lostland:fireball" ... (slot-tier "lostland:wizard_spell_slots" 3) ...
  "deal-damage" 28 ...)
```

一个没有学过 `lostland:wizard` 职业、没有拿到 `arcane_casting` 天赋的角色，`effective_capacity` 对 `wizard_spell_slots` 恒为零——`fireball` 的 `SlotTier` 消耗检查天然找不到可用槽位，技能静默不产出效果，不需要任何"这个角色没资格施法"的特判。

### 版本二：法力池（术士式施法者，也是项目所有者第二轮"法术池"字面所指）

```scheme
(register-resource-pool "lostland:sorcery_points"
  "lostland:pool.sorcery_points.display_name"
  'scalar (regen-on-turn-start 1))            ; 每回合缓慢回复，不依赖休息

(register-trait "lostland:innate_sorcery"
  "lostland:trait.innate_sorcery.display_name"
  (list) (list) (list)
  (list (resource-pool-grant "lostland:sorcery_points" (fixed 20))))

(register-subclass "lostland:sorcerer_bloodline" "lostland:subclass.sorcerer_bloodline.display_name"
  (list (trait-grant "lostland:innate_sorcery" 1)))

(register-skill "lostland:sorcerer_firebolt" ... (resource-amount "lostland:sorcery_points" 5) ...
  "deal-damage" 12 ...)
```

**多重职业验证**：一个角色若同时持有 `lostland:wizard` 主职与 `lostland:sorcerer_bloodline` 副职（`Agent.subclasses` 既有形状天然支持复数），`effective_traits` 会同时命中 `arcane_casting` 与 `innate_sorcery`——**两条资源轨道同时存在，互不干扰**：`fireball` 永远扣法术位，`sorcerer_firebolt` 永远扣法力池，九节已论证不需要任何"选哪个付账"的裁决。

### 血法师（附加示例，验证五节）

```scheme
(register-skill "lostland:blood_bolt" ... (blood-cost 15) ...
  "deal-damage" 30 ...)
```

`ResourceCost::Blood(15)` → `Effect::SpendBloodCost { target: caster, amount: 15 }`（直接扣 `health`，不查防御/抗性）→ `resolve` 并行检查 `caster.health - 15 <= 0`，若是则追加 `Effect::Kill { target: caster, killer: Some(caster), cause: KillCause::Skill { skill: blood_bolt_id } }`——一个 20 点血的角色连续两次 `blood_bolt` 会在第二次直接死于自己的法术，`killer == victim` 如实记录，具名角色照既有规则产出完整历史事件。**血法师不需要任何天赋授予"使用许可"——`granted_skills`（既有第一类天赋效果）已经完整覆盖了这层，见三节。**

**三个职业验收示例（法术位、法力池、血魔法）现在全部可表达，且体现出"天赋授予、不是全局固定字段"这条骨架，`trait-system.md` 留下的"完全无法表达"缺口在本文档闭环。**

---

## 十二、现在能做的 vs 等什么

**依赖链是三层：资源池（本文档）→ 天赋（`trait-system.md`，需要待办补丁）→ 等级（`level-and-experience-system.md`）。三者全部是纯设计，最底层不落地，上面两层在实现意义上都动不了——三节已论证为什么不做过渡版本。**

**现在就能落地的设计形状（不代表可以立刻写代码，代表设计本身没有阻塞）：**

1. `ResourcePoolDef`/`ResourceCost` 三变体/`register-resource-pool`（二节）。
2. `TraitDef.granted_resource_pools`/`ResourcePoolGrant`/`CapacityFormula`（三节）——**但落地前必须先等 `trait-system.md` 接受本文档三节末尾列出的补丁**。
3. `Agent.resource_pools`/`spent_slots`/`resting` 三个新字段 + `hash()` 精确插入点（十节）。
4. `Effect::AdjustResourcePool`（`AdjustResource` 泛化）、`Effect::AdjustResourceSlot`、`Effect::SpendBloodCost`、`Effect::Heal`、`Effect::BeginRest`、`Effect::ClearResting` 六个新/改变体（四/五/八节）。
5. `Intent::Rest`（七节）+ `resolve_wait` 的扩展检查。
6. 「气」这类标量池 mod 内容（六节）——一旦 1-2 落地即可直接使用。

**等什么（明确阻塞）：**

1. **`Agent.level` 不存在**——`level-and-experience-system.md` 纯设计，`CapacityFormula::ByLevel` 无输入可用。
2. **`TraitDef.granted_resource_pools` 不存在**——需要 `trait-system.md` 接受三节末尾的补丁，本文档无写权限，只能标注待办。
3. **法术位的具体每档容量曲线是内容设计问题**——本文档只给出 `CapacityFormula` 这个机制,不定案"法师 5 级该有几个三环位"这类具体数值,D&D 有现成表,要不要照搬是另一次内容设计决策。
4. **休息完成时"生命值恢复多少"的配置口没有设计**——`Effect::Heal` 这个执行原语已声明，但 `health` 不是 `ResourcePoolDef`,休息批次连不连带恢复生命值、恢复多少,需要一个独立的配置位,本文档不裁定。
5. **"战场感知"（附近出现敌意实体主动打断休息）不存在**——`Effect::ClearResting` 已给出通用挂载点,但判断源需要一套完全没设计过的感知/AI 系统。
6. **"脱离战斗"状态不存在**——`RegenRule::ResetOnLeaveCombat` 现在只是声明占位。
7. **法术位/法力池升阶施法的效果加成不做**（二节，项目所有者本人倾向不做）。
8. **UI 侧"施法前提示会不会致死"不在本设计范围**（五节,P7 的事,形状上未被挡住)。
9. **P6 装备系统**——若未来某件装备想授予/修正资源池容量,需要装备系统落地之后才有地方挂,与几乎每一份已冻结设计文档点名的既有缺口相同。

---

## 相关文档

- [天赋/特性系统](trait-system.md) 一、三、四、六、七、九节 — `TraitDef`/`TraitGrant`/`effective_traits` 既有形状，本文档三节新增第四类效果的直接依附对象与待办补丁清单
- [等级与经验系统](level-and-experience-system.md) 二节 — `Agent.level` 单一总等级字段，`CapacityFormula::ByLevel` 与九节多重职业论证的直接依据
- [增益与通用触发器](buffs-and-triggers.md) 一、三节 — `on_turn_start`/`on_damaged` 触发点，四节 `RegenRule::OnTurnStart` 与五节"血代价不触发 `on_damaged`"两处判断的直接依据；约束 C4 与惰性到期判定
- [击杀与死亡记录](kill-and-death-events.md) — `KillRecord`/`append_kill_history` 既有形状与"具名才出完整记录"判据，五节"自杀记录"直接复用
- [伤害公式的 mod API](damage-formula-mod-api.md) — `damage_after_defense` 现有签名，五节"血代价必须绕开它"的直接依据
- `crates/ll-sim/src/skill.rs`/`crates/ll-mod/src/skill.rs` — `ResourceKind`/`ResourceCost`/`SkillEffect` 现状核实依据
- `crates/ll-world/src/entity/agent.rs` — `health`/`mana`/`stamina` 现有字段与文档
- `crates/ll-sim/src/{resolve,apply,effect,timeline,intent}.rs` — `resolve_wait`/`Intent::Wait`/`Effect::Damage`/`Effect::AdjustResource`/`Effect::Kill` 现状核实依据
- `crates/ll-world/src/state.rs` — `WorldState::hash()` 覆盖范围核实与十节精确插入点
- `crates/ll-core/src/rng.rs` — `DetRng::for_entity` 纯函数派生随机数，八节防刷第二条防线的直接依据
- [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md) / [0017](../decisions/0017-tiered-declarations-materialize-columnar.md) — 一档判据
- [ADR 0020](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md) — 无浮点边界
- [ADR 0021](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) — 二节"池与位统一到什么程度"核心判断的直接依据
- [ADR 0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 十节 `hash()` 必须手动同步覆盖新字段的纪律依据
- [ADR 0023](../decisions/0023-script-state-writes-go-through-apply.md) — 四节"恢复效果必须经 `Effect`/`apply`"的同一条精神
