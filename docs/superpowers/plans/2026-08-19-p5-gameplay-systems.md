# P5-B：玩法系统——职业、技能树、副职、网状任务 实施计划

> **【2026-08-31 编号更正（批次 25）】本文档「任务 6：网状任务图」一节的「ADR 0018 的三档分级」编号有误。**
> 三档分级（一档静态声明／二档受限公式／三档脚本回调）是
> [ADR 0016 — mod 性能分档按声明方式，不按作者身份](../../../knowledge/decisions/0016-mod-performance-tiers-by-declaration.md)
> 与 [ADR 0017 — 声明式分档物化为列式数据](../../../knowledge/decisions/0017-tiered-declarations-materialize-columnar.md)
> 的内容；[ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的边界，只在第三步判据里**引用**了 0016 的一/二档。
> **分档结论本身完全成立、一字不改，错的只是编号。** 本文档「关键设计判断」第 2 条那处
> 「按 ADR 0018 的引擎层/玩法层边界判断」是**正确引用**，未动。
> 本文档是历史档案，按纪律第 9 条原文一字不改，只在此加标记。更正方：
> [批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

> **给执行者：** 必须配合 `superpowers:subagent-driven-development` 逐任务实施。步骤使用 `- [ ]` 复选框追踪。
> **前置条件**：本计划依赖 [`2026-08-19-p5-save-format-and-identity.md`](2026-08-19-p5-save-format-and-identity.md)（下称 P5-A）任务 3（`Agent`/`Arena<Agent>` 补齐 serde）与任务 8（脚本状态存储）已经落地——技能冷却与任务进度的持久化直接建在这两者之上（见本文档关键设计判断 2）。**建议顺序**：P5-A 先收尾评审通过，本计划再开工；若项目所有者要求并行，本计划任务 1–4（设计基线与注册表）不依赖 P5-A，可以提前动手，但任务 6（技能冷却持久化）与任务 8（任务进度持久化）必须等 P5-A 任务 3/8 落地才能开工。
> **红灯窗口提醒**：本计划绝大多数任务是新增内容类型（职业/技能/副职/任务），理论上可以保持全绿；任务 5（`Agent` 新增职业/副职相关字段）若与 P5-A 任务 3 的换型窗口重叠，需要协调避免两次分别触碰 `Agent` 引发不必要的合并冲突——建议紧跟 P5-A 任务 3 之后立即开工，具体见测试迁移策略一节。

**目标：** 落地规格 P5 行「玩法系统：职业、技能树、副职、网状任务」。范围边界（必须在开工前想清楚，反复出现在下文各任务）：**规格 §15 把 P6「物品与装备」排在本计划之后**——技能树初期只能提供纯数值加成（直接修改 `BaseStats`/`DerivedStats` 的整数分量），不能读取装备属性（`ItemDef.stat_bonuses` → `DerivedStats` 的接线是 P6 的工作）。`buffs-and-triggers.md`（触发器/增益完整设计）已经自行标注实现阶段依赖 P6 装备落地——本计划只借用其中「不依赖装备、可以独立实现」的最小子集（惰性到期判定 + 深度限制的效果队列），不实现完整的 `TriggerDef` 一/二/三档分发与 `StackPolicy` 全部三种堆叠策略。

**架构：** 沿用 C1–C5。本计划新增内容对「本体即 Mod」（ADR 0016）的检验是核心工作之一——`p4-to-p5.md` 已经指出地形那一种内容类型就抓出两个特权路径洞（`opens_into`、`TerrainAttrs` 私有导致公开函数不可调用），**职业/技能/任务的接口面比地形大得多**（涉及前置关系图、冷却计时、多阶段任务状态机），本计划每个内容类型的任务都必须重做一次这项检验，不能假设"抄地形的模式就自动没问题"。

**技术栈：** `ll-world`（`Agent` 新增字段）、`ll-sim`（新增 `Effect` 变体）、`ll-mod`（新增注册表模块，模式与 `terrain.rs`/`space_profile.rs` 一致）、`ll-script`（技能/任务条件判定的脚本接口）。不新建 crate。

**设计依据：**
- 规格 §4（C1–C5）、§15 P5/P6/P7 三行（P6 物品接线边界、P7 世界生成器）
- [`knowledge/design/attribute-system.md`](../../../knowledge/design/attribute-system.md)（六项主属性、`DerivedStats` 纯函数派生纪律、d20 判定）
- [`knowledge/design/buffs-and-triggers.md`](../../../knowledge/design/buffs-and-triggers.md)（惰性到期判定、效果队列深度上限、`StackPolicy`——本计划部分借用，见范围边界）
- [`knowledge/design/society-and-affiliation.md`](../../../knowledge/design/society-and-affiliation.md)（`AffiliationKind::Profession` 既有形状，本计划职业系统在此基础上扩展，不重新发明）
- [`knowledge/design/identity-and-ids.md`](../../../knowledge/design/identity-and-ids.md)（类型/实例分离判据，职业/技能是"类型"走 `ContentIndex`）
- [`knowledge/design/script-state-storage.md`](../../../knowledge/design/script-state-storage.md)（技能冷却/任务进度持久化的落点，P5-A 任务 8 交付）
- ADR 0009（默认派生只存偏差）、0014（惰性判定优于事件驱动，同构论证）、0015（注册校验是解析不是不变式）、0016/0017（分档与列式物化）、0018（引擎层/玩法层边界）
- **真实代码基线**：`crates/ll-world/src/entity/{agent,affiliation,stats,goal}.rs`、`crates/ll-sim/src/effect.rs`、`crates/ll-world/src/terrain.rs`/`crates/ll-world/src/space_profile.rs`（注册表模式的两个已验证先例）、`crates/ll-sim/examples/p3_acceptance/turn.rs`（`MAX_STEPS_PER_ADVANCE` 兜底先例）

---

## 全局约束

- 沿用 P5-A 的全部全局约束（世界状态禁止浮点、`apply` 唯一写入口、C5 容器纪律、ADR 0011/0015 的 serde 分工、依赖方向）。
- **本体即 Mod（ADR 0016）**：本体注册的职业/技能/任务与 mod 注册的必须走完全相同的公开 API，不允许"本体内部特权路径"。每个任务的验收标准都包含一条"本体与假想 mod 内容走同一条注册路径"的测试。
- **纯数值边界（规格 §15 P6 行的直接推论）**：技能效果只能修改 `BaseStats`（六项主属性的当前值/临时修正）与既有 `Effect`（`Damage`/`AdjustWallet` 等）能表达的范围，不得引入任何读取装备槽位的逻辑——若某个技能效果"看起来需要装备信息才能表达"，正确做法是记录到「待裁定」或「有意留给后续阶段」，不是提前接一条装备读取的旁路。
- **技能/任务/职业的"进行到哪一步"状态持久化，一律经 P5-A 任务 8 的脚本状态存储 API（`entity-state-set!`/`entity-state-get!`）或 `Agent` 的直接字段——不新开第三套持久化机制**（关键设计判断 2 展开）。
- 文件 200–400 行为宜，800 行上限；提交信息 `<type>: <描述>`，正文讲**为什么**，中文，不得含 AI 署名。

---

## 关键设计判断（本计划在规格留白处做出的实现判断，非规格裁定——规格对这四个系统只有一句话「职业、技能树、副职、网状任务」，没有任何形状细节，本计划的设计判断因此比 P5-A 更多，评审时更需要逐条确认）

1. **职业（Class）复用既有 `AffiliationKind::Profession` 的字段位置，不新增第二套"职业"概念**——`Agent.profession: ContentIndex` 字段已经存在（P3 阶段建的字段布局），`society-and-affiliation.md` 已经把 `Profession` 定为"恒为 Def"（即职业本身是类型不是实例，与 `ContentIndex` 语义完全吻合）。本计划的"职业系统"是给这个既有占位字段配一张真正的注册表（`ClassDef`），不是重新设计一个平行字段。
2. **技能树的"这个实体解锁了哪些技能节点、每个技能的冷却还剩多久"，持久化落在 `Agent` 的直接字段（若是引擎需要频繁读取的核心状态,例如冷却，影响每回合结算）或脚本状态存储（若是玩法层/mod 可扩展的状态，例如某个 mod 自定义的技能进度标记）——按 ADR 0018 的引擎层/玩法层边界判断该用哪一个，不是任选其一。** 具体判断标准：**冷却计时**是每回合都要读、且是"技能"这个内容类型的通用属性（不因 mod 而异），归引擎层，做成 `Agent` 的字段（`BTreeMap<ContentIndex, Tick>`，"到期时刻"而非"剩余时长"，见 4 的惰性判定）；**技能树的"已解锁节点集合"**若技能树结构本身是数据驱动的树/图（mod 可以定义全新的树），则"解锁了哪些节点"更接近"任务进度"这类因内容而异的状态，归脚本状态存储更合适——**但本计划最终选择让它也走 `Agent` 字段**（`Vec<ContentIndex>` 或等价的已解锁集合），理由是"解锁与否"是几乎所有技能效果结算都要查询的高频状态，脚本状态存储的跨界调用开销（326ns/次，`script-entity-handles-and-batch-queries.md` 已实测）在"每次判断能不能放技能"这个热路径上不是不能接受，但没有必要——直接读 `Agent` 字段更简单、不需要经过脚本 API。**任务进度**（网状任务图节点完成状态）不是每回合都读的高频状态，且任务内容本身就是 mod 高度可扩展的（一个任务可能有 mod 自定义的复杂完成条件），归脚本状态存储更合适。
3. **技能效果统一走"产出既有 `Effect` 或本计划新增的极少数 `Effect` 变体"，不引入独立的"技能结算"旁路**——`resolve` 收到 `Intent::UseSkill { actor, skill, target }` 后查技能定义、判断冷却/资源是否足够、产出对应 `Effect`（可能是 `Damage`、`AdjustWallet`,或本计划新增的 `Effect::ApplyStatModifier`/`Effect::SetSkillCooldown`，见任务 5），这与地形/空间系统"效果只是纯数据,由 apply 统一写入"的既有纪律完全一致，不为技能另开一条特殊路径。
4. **临时数值增益（技能提供的"下 N 回合力量 +2"这类效果）采用 `buffs-and-triggers.md` 一、的惰性到期判定，但只借用这一条机制，不引入完整 `TriggerDef`/`StackPolicy`**——存"到期时刻"不存"当前是否生效"，读取时现比对世界时钟；堆叠策略本计划**固定**为"刷新持续时间"（`buffs-and-triggers.md` 五、`StackPolicy::RefreshDuration`）这一种,不做成可配置数据（该文档设计的完整三选一堆叠策略需要更完整的效果系统支撑，本计划范围内没有足够的真实用例驱动"为什么需要另外两种"，属于过度设计,按 YAGNI 推迟）。**这是比设计文档本身更窄的实现范围，评审时需要确认这条收窄是否可接受。**

---

### 任务 1：设计基线——`ClassDef`/`SkillDef`/`SubclassDef`/`QuestDef` 形状草案

**Files:** `knowledge/design/class-skill-quest-system.md`（新，纯设计文档，不是代码）
**依赖：** 无

**这是本计划唯一一个产出设计文档而非代码的任务，理由**：规格对这四个系统只给了一句话，不存在类似 `coordinate-system-and-layers.md`/`identity-and-ids.md` 那样"先经过项目所有者多轮往返定案"的冻结设计可以直接抄。若跳过这一步直接让后续任务各自决定形状，四个系统的接口会互相不一致（例如技能树用 `Vec<SkillDef>` 表示前置关系，任务图却用另一套图结构，读者需要学两套心智模型）。**本任务只产出形状，不产出任何具体游戏内容**（不设计"战士""法师"这类具体职业，那是内容设计而非系统设计，属于后续任务或专门的内容批次）。

**必须回答的形状问题**（本任务的核心交付物）：
1. `ClassDef` 与既有 `AffiliationKind::Profession`/`Agent.profession` 的关系（按关键设计判断 1，本任务把这条判断写成正式设计文档条目）。
2. `SkillDef` 的前置关系如何表达——推荐 DAG（有向无环图，前置技能列表 + 注册期校验无环），不是线性序列（"技能树"这个名字暗示分支，线性序列表达不了分支）。
3. `SubclassDef`（副职）与 `ClassDef`（主职）的关系——是否允许同时持有多个副职、副职是否也提供技能树、副职与主职的技能树是否共享同一个"已解锁节点"命名空间（若共享，两个不同职业若定义了相同 `ContentIndex` 的技能会冲突；若不共享，需要设计命名空间隔离方式）。**这是本任务最容易被想当然做错的一处，必须明确写下结论，不能留白让后续任务各自猜。**
4. `QuestDef` 网状结构的节点/边形状——前置任务列表 + 后置解锁列表（双向还是只存单向、另一侧现算，比照 `Interior.anchor`/反向索引"单一真相源"的既有模式，本任务应该复用同一条纪律：只存一个方向，另一个方向是派生视图）。
5. 技能效果的数值边界——哪些字段允许存在（纯数值：造成伤害基础值、消耗资源量、冷却时长、临时属性修正量），哪些明确排除（任何"读取装备槽位"的字段，按规格 §15 P6 行的边界）。

**Interfaces Produces（概念形状，供后续任务直接照抄，可在实现时微调字段名）：**
```rust
pub struct ClassDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId, // 指向 Fluent 本地化键，不存字面字符串
    pub primary_attribute: AttributeKind, // STR/DEX/... 之一，供职业倾向展示等场景使用
}

pub struct SkillDef {
    pub id: NamespacedId,
    pub owning_class: ContentIndex, // 指向 ClassDef，或 None 表示通用技能
    pub prerequisites: Vec<ContentIndex>, // 前置技能，DAG 边
    pub cooldown_ticks: u32,
    pub resource_cost: ResourceCost, // 具体资源类型（法力/耐力）留给任务 2 细化
    pub effect: SkillEffect, // 纯数值，见下
}

pub enum SkillEffect {
    DealDamage { base: i32 },
    RestoreResource { base: i32 },
    TemporaryStatModifier { attribute: AttributeKind, amount: i32, duration_ticks: u32 },
    // 不含任何装备/物品相关变体——规格 §15 P6 行边界
}

pub struct SubclassDef {
    pub id: NamespacedId,
    pub skill_namespace: NamespacedId, // 决定与主职技能树是否共享 ContentIndex 命名空间——
                                        // 具体结论见本任务问题 3，此处只是占位展示需要有这个字段
}

pub struct QuestNodeDef {
    pub id: NamespacedId,
    pub prerequisites: Vec<ContentIndex>, // 单一真相源；"解锁了哪些后续任务"是派生视图
    pub completion_condition: QuestCondition, // 见任务 6，脚本可扩展
}
```

- [ ] 与项目所有者过一遍问题 3（副职与主职技能树的命名空间关系）——**这是本任务里唯一一处真正需要外部输入而非纯技术判断的问题，若拿不到明确答复，本任务应该给出一个保守默认（不共享命名空间，各自独立）并在文档里标注"待裁定，见 P5-B 待裁定第 1 条"，不能自己悄悄拍板一个影响深远的设计后不声明这是临时选择**。
- [ ] **提交**（`docs:`，本任务不涉及代码，产出纯设计文档）

---

### 任务 2：`ClassDef` 注册表——本体即 Mod 检验

**Files:** `crates/ll-mod/src/class.rs`（新）
**依赖：** 任务 1

**照抄 `terrain.rs`/`space_profile.rs` 已验证的模式**：私有字段 + `ClassTable::define` 注册期校验 + `materialize_base_classes(intern: &mut dyn FnMut(...))` 本体注册入口 + `base_class_fixture()` 测试夹具。本体至少注册 2–3 种基础职业（数值/名称本身是内容设计，不是本任务重点，可以用占位名称，真正的职业数值平衡不在本计划范围）。

**必须重做的"本体即 Mod"检验**：`p4-to-p5.md` 已经点名两处地形踩过的坑（`opens_into` 特权路径、`TerrainAttrs` 私有导致公开函数不可调用）——本任务必须显式测试"本体注册函数与 mod 注册函数是同一个公开函数"，不能只靠"看起来像"。

- [ ] **TDD 循环**：
  - `本体注册的职业与假想 mod 注册的职业走同一个 ClassTable::define 函数`
  - `重复定义同一个索引返回错误而非静默覆盖`
  - `未注册的 ContentIndex 查询返回 None`（对齐 ADR 0015 的解析纪律）
- [ ] **提交**

---

### 任务 3：`SkillDef` 注册表 + 前置关系 DAG 校验

**Files:** `crates/ll-mod/src/skill.rs`（新）
**依赖：** 任务 1、任务 2（`SkillDef.owning_class` 指向 `ClassDef`）

**DAG 无环校验是本任务的核心正确性要求**——技能树若允许循环前置（技能 A 需要技能 B，技能 B 需要技能 A），任何"能否解锁"的判定都会死循环或产出错误结果。注册期用拓扑排序（`ll-mod::topo` 已有依赖拓扑排序的先例，`topo.rs`，核实是否可以直接复用其算法而不是重新写一遍）校验无环，检测到环时注册失败并报告具体是哪几个技能构成了环（可读的错误信息，不是"检测到环"这种无法定位的提示）。

**Interfaces Produces：**
```rust
pub struct SkillTable { /* 列式存储，同 TerrainTable 结构 */ }
pub fn materialize_base_skills(intern: &mut dyn FnMut(NamespacedId) -> ContentIndex)
    -> Result<(BaseSkillIds, SkillTable), SkillError>;

/// 注册期校验：给定全部已注册技能的前置关系，是否存在环。
/// 复用 ll_mod::topo 的拓扑排序算法（若其接口足够通用），否则独立实现
/// 但采用同一种"检测到环时报告具体环路"的错误粒度。
fn validate_no_cycles(skills: &SkillTable) -> Result<(), SkillError>;
```

- [ ] **TDD 循环**：
  - `技能前置关系形成环时注册失败`
  - `环形错误信息包含构成环的具体技能 ID 列表`（不是笼统的"存在环"）
  - `合法的分支型技能树（一个技能有多个前置，一个前置解锁多个后续）注册成功`
  - `本体与 mod 技能走同一条注册路径`
- [ ] **提交**

---

### 任务 4：`SubclassDef` 注册表

**Files:** `crates/ll-mod/src/subclass.rs`（新）
**依赖：** 任务 1（问题 3 的裁定结果）、任务 2、任务 3

具体形状取决于任务 1 问题 3 的裁定结果（共享还是不共享技能命名空间）。**若裁定为"待裁定，保守默认不共享"**（任务 1 的保守默认），本任务应按不共享实现，并在提交信息里明确标注"待项目所有者确认，若后续裁定为共享，需要返工"——不能假装这是已经拍板的最终形态。

- [ ] **TDD 循环**：
  - `副职技能与主职技能的 ContentIndex 不冲突`（按不共享命名空间的默认实现）
  - `同一个 Agent 可以持有一个主职与至少一个副职`（`Agent` 字段设计见任务 5）
  - `本体与 mod 副职走同一条注册路径`
- [ ] **提交**（提交信息标注待裁定依赖）

---

### 任务 5：`Agent` 新增职业/技能相关字段 + 技能效果 `Intent`/`Effect`

**Files:** `crates/ll-world/src/entity/agent.rs`（新增字段）、`crates/ll-sim/src/{intent,effect,resolve,apply}.rs`
**依赖：** 任务 3（`SkillDef`）、P5-A 任务 3（`Agent` 已可序列化，本任务新增字段需要一并保持可序列化，不能引入新的不可序列化字段）

**关键设计判断 2 的落点**：冷却计时归引擎层字段，"已解锁技能"归引擎层字段（本计划的选择），任务进度归脚本状态存储（任务 8）。

**Interfaces Produces（概念形状）：**
```rust
// Agent 新增字段
pub unlocked_skills: Vec<ContentIndex>,
/// 到期时刻，不是"剩余时长"——惰性判定（关键设计判断 4），
/// 不在这里存"是否在冷却中"这个可以现算的布尔值。
pub skill_cooldowns: BTreeMap<ContentIndex, Tick>, // C5：BTreeMap 不是 HashMap
pub subclasses: Vec<ContentIndex>,

// intent.rs 新增
pub enum Intent { /* 既有变体 */ UseSkill { actor: EntityId, skill: ContentIndex, target: Option<EntityId> } }

// effect.rs 新增（数量克制——只加纯数值效果需要的最小集合）
pub enum Effect {
    /* 既有变体 */
    SetSkillCooldown { actor: EntityId, skill: ContentIndex, until: Tick },
    ApplyStatModifier { target: EntityId, attribute: AttributeKind, delta: i32, expires_at: Tick },
}
```

**`resolve` 判定逻辑**（技能是否可用）：查 `unlocked_skills` 是否包含该技能、查 `skill_cooldowns.get(skill)` 是否 `< current_tick`（惰性判定，不需要主动清理这个字段本身——但与 `buffs-and-triggers.md` 一、的提醒一致，`skill_cooldowns` 这个 `BTreeMap` 若某个技能永远不再被使用，过期条目会一直占着，本任务不强制清理,记入「有意留给后续阶段的缺口」）、查资源是否足够（`resource_cost` vs `Agent` 当前资源值——若 `Agent` 尚无"当前法力/耐力"字段，本任务需要核实并可能需要一并补上,这是本任务开工前必须核实的前置条件）。

**本体即 Mod 检验（本任务必须做的部分）**：技能效果的产出必须完全走"查 `SkillDef.effect` → 匹配变体 → 产出对应 `Effect`"这条通用路径,**不允许为某个特定技能在 `resolve` 里写专门的 `if skill == 某个硬编码 ID` 分支**——这正是 `p4-to-p5.md` 点名的"撞门即开"同类陷阱在技能系统上的复现风险,必须在写 `resolve` 逻辑时主动避免,并用一条测试钉住（"新增一个假想 mod 技能,不修改 resolve 任何代码,该技能效果依然正确产出"）。

- [ ] **TDD 循环**：
  - `使用未解锁的技能不产出任何 Effect`
  - `使用冷却中的技能不产出任何 Effect`
  - `资源不足时不产出任何 Effect`
  - `成功使用技能后产出对应 SkillEffect 变体映射出的 Effect,且产出 SetSkillCooldown`
  - `新增一个假想 mod 技能（不修改 resolve 代码）,该技能效果依然被正确处理`（本体即 Mod 检验的直接体现）
  - `WorldState::hash() 纳入 unlocked_skills/skill_cooldowns 的变化`（否则技能解锁/冷却这类会被结算改动的字段游离在确定性回归测试之外，重演 hash() 文档"早期版本只混入地形"同一类缺口——这是坐标系重写计划任务 12 已经示范过的纪律，本任务必须延续）
- [ ] **提交前必须通过的检查**：`cargo check --workspace`、`cargo test --workspace`、`cargo clippy --workspace`
- [ ] **提交**

---

### 任务 6：网状任务图——`QuestDef` 注册表 + 完成条件

**Files:** `crates/ll-mod/src/quest.rs`（新）
**依赖：** 任务 1

**完成条件必须可脚本扩展**（ADR 0018 的三档分级同样适用于任务系统）：一档（声明式，例如"击杀 N 个某类型敌人"，纯数据）、三档（脚本回调，处理"拜访某个 NPC 并说出特定台词"这类无法穷举成数据的条件）。**本计划不做二档**（受限公式）——任务完成条件通常要么是简单计数（一档够用）要么是复杂逻辑（需要三档），中间的"公式"这一档在当前已知需求下没有明确用例,按 YAGNI 不做,若后续发现真实需要,是独立的扩展任务。

**Interfaces Produces：**
```rust
pub struct QuestNodeDef {
    pub id: NamespacedId,
    pub prerequisites: Vec<ContentIndex>, // 单一真相源
    pub condition: QuestCondition,
}
pub enum QuestCondition {
    KillCount { target_kind: ContentIndex, count: u32 }, // 一档
    Script(NamespacedId), // 三档，脚本回调判定是否完成
}
pub struct QuestTable { /* 列式存储 */ }

/// 派生视图：给定一个已完成的任务节点集合，返回因此解锁的后续节点。
/// 不是独立存储——见任务 1 问题 4 的"单一真相源"结论。
pub fn unlocked_by(table: &QuestTable, completed: &[ContentIndex]) -> Vec<ContentIndex>;
```

**注册期校验**：与技能树同理，`prerequisites` 也需要无环校验（复用任务 3 的 DAG 校验逻辑,不重新写一份——若任务 3 的 `validate_no_cycles` 设计得足够通用，本任务应该直接复用同一个函数，只是换一张表）。

- [ ] **TDD 循环**：
  - `任务前置关系形成环时注册失败`（复用任务 3 的校验逻辑或验证其可复用性）
  - `unlocked_by 对给定已完成集合返回正确的后续节点`
  - `unlocked_by 不是存储，两次调用不会因为内部状态变化而产出不同结果（纯函数性质）`
  - `脚本回调型完成条件可以被本体与 mod 同样注册`
- [ ] **提交**

---

### 任务 7：任务进度持久化——脚本状态存储接线

**Files:** `crates/ll-mod/src/quest.rs`（延续任务 6）、脚本侧任务判定 API（`crates/ll-script/src/api/quest.rs`，新，或并入既有 `api/query.rs`）
**依赖：** 任务 6、P5-A 任务 8（脚本状态存储必须已经落地）

按关键设计判断 2，任务进度（"这个实体完成了哪些任务节点"）走脚本状态存储的每实体存储,不是 `Agent` 的直接字段——理由：任务内容高度依赖 mod 扩展，不是每回合都要读的高频状态，脚本状态存储的命名空间隔离天然适合"任务是 mod 定义的内容,进度应该按 mod 命名空间隔离"这条需求。

**Interfaces Produces（概念形状）：**
```scheme
; 脚本侧使用示例，键约定使用任务 ID 字符串
(entity-state-set! actor-handle "quest_progress:lostland:main_quest_1" 1)
```

```rust
// Rust 侧：resolve 判定某个任务节点是否已完成,读取脚本状态存储
// （复用 P5-A 任务 8 的 entity-state-get! 等价的 Rust 侧接口，
// 具体是否有 Rust 直接读取的路径,还是必须经脚本调用,留给实现者核实
// P5-A 任务 8 交付的具体接口形状后决定）。
```

**必须验证**：任务进度经存档往返后保持一致（这条测试天然依赖 P5-A 任务 9 的存档读写管线，若 P5-A 尚未完成到那一步，本任务的这条测试可以先用"直接序列化 `WorldState` 再反序列化"的方式验证，不必等待完整的 `ll-content` 读写管线）。

- [ ] **TDD 循环**：
  - `任务进度写入后可以在同一会话内读回`
  - `任务进度经 WorldState 序列化往返后保持一致`
  - `不同 mod 命名空间的任务进度互不干扰`
- [ ] **提交**

---

### 任务 8：UI 数据层挂钩——技能树面板/任务日志的数据接口（不含渲染）

**Files:** `crates/ll-world/src/overview.rs` 或新增 `crates/ll-world/src/skill_overview.rs`（视实现时判断哪个更合适）
**依赖：** 任务 5、任务 7

**明确边界**：`ll-ui` 完整像素控件库排在 P7（规格 §15 已明确），本任务**不交付任何渲染代码**，只交付"给定一个 `Agent`，返回技能树当前状态的一份可展示数据结构"（哪些节点已解锁、哪些可解锁但未解锁、哪些冷却中及剩余时长）与"给定任务进度，返回当前任务日志的一份可展示数据结构"——这两份数据结构是未来 P7 UI 层的直接消费对象,本任务不涉及像素、不涉及 `ll-render`。

**Interfaces Produces（概念形状）：**
```rust
pub struct SkillTreeView {
    pub unlocked: Vec<ContentIndex>,
    pub available: Vec<ContentIndex>, // 前置已满足但尚未解锁
    pub on_cooldown: Vec<(ContentIndex, Tick)>, // 技能 + 剩余到期时刻
}
pub fn build_skill_tree_view(agent: &Agent, skills: &SkillTable, now: Tick) -> SkillTreeView;

pub struct QuestLogView {
    pub completed: Vec<ContentIndex>,
    pub unlocked_not_completed: Vec<ContentIndex>,
}
```

- [ ] **TDD 循环**：
  - `已解锁但前置未满足的技能不出现在 available 中`（这不应该发生,但作为防御性测试确认数据一致性）
  - `冷却中的技能剩余时刻计算正确`
  - `QuestLogView 与 quest::unlocked_by 的结果一致`（不能自己另算一遍，重复造成两处可能漂移的真相源）
- [ ] **提交**

---

### 任务 9：验收 Demo

**Files:** 建议 `crates/ll-sim/examples/p5_gameplay_acceptance/`（若实现中发现更合适的落点可调整）
**依赖：** 任务 1–8 全部

必须展示：

1. **职业与技能树可用**——demo 角色选择一个职业，解锁至少一条有分支的技能树路径（至少一个技能有两个可选后续，验证"树"而不是"线性序列"这条形状要求）,施放至少一个技能产出正确的纯数值效果（伤害/属性修正）。
2. **副职可叠加**——demo 角色额外持有一个副职，其技能与主职技能不冲突（对应任务 4 的命名空间设计）。
3. **网状任务可推进**——demo 世界至少有一个"一个前置任务解锁两个后续任务"的分支结构（网状而非线性），玩家完成前置后两个后续任务同时可见。
4. **技能冷却与任务进度经存档往返保持一致**——与 P5-A 的验收 demo 呼应，本 demo 应该实际调用 P5-A 交付的存档读写路径（若 P5-A 尚未完成到那一步，本任务可以先用裸 `WorldState` 序列化往返代替，并在提交信息里注明这是临时替代,待 P5-A 完成后应该改为走完整存档路径）。

**必须实测，如实报告哪些验证了、哪些没有**——延续既有纪律。**本次最可能重演"单元测试测不出连线"这个模式的地方**：(a) 任务 5「技能效果是否真的只用通用路径,没有为具体某个技能硬编码分支」——这条本体即 Mod 检验若只在单元测试层面做（"新增假想技能不改代码"），demo 应该再实测一次同样的性质,用真实的完整技能定义走一遍完整链路；(b) 任务 8 的数据视图与实际技能/任务状态是否真的同步——demo 应该在技能解锁/任务完成的前后分别截取 `SkillTreeView`/`QuestLogView`，肉眼确认数据确实反映了状态变化，而不是停留在"函数被调用了"这个弱验证。

**裁定 CS-7**：若本 demo 需要真实按键驱动技能释放/任务查看，必须先确认前台窗口归属，确认不了就改用程序化驱动（`Intent → resolve → apply`），与 P5-A 验收 demo 同一条纪律。

- [ ] **提交**

---

## 自查

### 完整调用链

```
玩家选择职业（建档流程一部分，UI 属 P7）                            ← ll-mod::class（任务 2）
  → Agent.profession 写入对应 ClassDef 的 ContentIndex               ← ll-world::entity::agent（既有字段）
玩家解锁一个技能（本计划不设计"解锁"这个 Intent 的具体触发方式——
  可能是升级奖励/任务奖励，属于内容设计，本计划只保证字段与判定就绪） ← ll-world::entity::agent（任务 5，unlocked_skills）
玩家释放技能                                                       ← Intent::UseSkill（任务 5）
  → resolve 查 unlocked_skills 是否包含                             ← ll-sim::resolve（任务 5）
  → resolve 查 skill_cooldowns 是否已过期（惰性判定）                ← ll-sim::resolve（任务 5）
  → resolve 查资源是否充足                                          ← ll-sim::resolve（任务 5，需核实资源字段存在）
  → resolve 按 SkillDef.effect 产出 Effect（Damage/ApplyStatModifier 等）← ll-sim::resolve（任务 5）
  → apply 写入 Agent 状态（生命/属性修正/冷却到期时刻）               ← ll-sim::apply（既有唯一写入口）
玩家完成一个任务节点的完成条件                                      ← ll-mod::quest（任务 6）判定
  → 完成状态写入脚本状态存储（每实体，任务命名空间隔离）              ← ll-script::api::quest（任务 7）
  → unlocked_by 现算出新解锁的后续任务节点（可能是网状分支）          ← ll-mod::quest（任务 6）
  → build_skill_tree_view/QuestLogView 供未来 UI 消费                ← ll-world::skill_overview（任务 8）
  → 存档：技能冷却（Agent 字段）随 Agent 序列化，任务进度（脚本状态）
    随脚本状态存储序列化，两者都经由 P5-A 任务 9 的存档读写管线落盘   ← P5-A::save_file（跨计划依赖）
  → 读档后世界与存档前逐位一致（含技能/任务状态）                    ← 验收 demo（任务 9）
```

**每一环都指出了负责的任务与接口。** 没有断链，但有一处依赖 P5-A 尚未完成部分的软连接（存档读写管线）——这是计划拆分的直接后果，不是本计划自身的缺陷,已在文档顶部与任务 9 明确标注处理方式（先用裸序列化代替，待 P5-A 完成后补齐）。

### 测试迁移策略

| 任务 | 是否可能变红 | 说明 |
|---|---|---|
| 1、2、3、4、6、7、8 | **否** | 纯新增类型/新增注册表/新增数据视图函数,不改动既有函数签名 |
| 5（`Agent` 新增字段 + `Intent`/`Effect` 新增变体） | **可能局部变红** | `Effect` 是 `match` 穷尽的枚举，新增变体会要求 `apply` 补分支（编译器强制，不补就编译不过，不存在"忘了补但还能跑"的风险）；`Agent` 新增字段若与 P5-A 任务 3 的换型窗口重叠，可能产生合并冲突（不是逻辑错误，是纯粹的并发编辑冲突）——建议协调开工时间避免同时触碰 `agent.rs` |
| 9（验收 demo） | 否 | 新增 example，不影响既有测试 |

**结论**：本计划红灯风险显著低于 P5-A（P5-A 有一处确定的换型红灯，本计划最多是编译器强制的穷尽匹配补充与人为的合并时序问题，不是设计上必然的红灯窗口）。

---

## 有意留给后续阶段的缺口

- **完整的 `TriggerDef` 一/二/三档分发与 `StackPolicy` 三种堆叠策略**——`buffs-and-triggers.md` 自行标注依赖 P6 装备，本计划只借用惰性到期判定这一条机制（关键设计判断 4），不实现完整触发器系统。
- **装备属性接入技能效果**（技能伤害读取武器攻击力等）——规格 §15 P6 行的硬边界，本计划技能效果止步于纯数值。
- **职业数值平衡、具体技能内容设计**——本计划只交付系统骨架（注册表、判定逻辑、DAG 校验），不交付"战士的技能树应该长什么样"这类内容设计，那是独立的内容批次。
- **技能树/任务日志的像素 UI 渲染**——止步于任务 8 的数据视图,渲染留给 P7。
- **`skill_cooldowns` 过期条目的主动清理**——惰性判定不要求主动清理（关键设计判断 4/`buffs-and-triggers.md` 一、已论证这条），本计划不实现清理逻辑，若未来发现 `Agent` 长期运行后这个字段线性增长导致性能问题，是独立的优化任务。
- **网状任务的失败/放弃分支**——本计划只处理"完成 → 解锁后续"这条主路径，任务失败、超时、玩家主动放弃这些分支不在规格给出的一句话范围内的明确要求，留给内容设计批次决定是否需要。

---

## 待裁定

### 1. 副职与主职技能树是否共享 `ContentIndex` 命名空间（任务 1 问题 3）

本计划任务 1 给出保守默认（不共享），任务 4 按此实现并在提交信息标注待确认。**这是需要项目所有者拍板的问题，不是技术判断能独立解决的**——共享与否直接影响副职系统未来能否支持"主职技能被副职复用/增强"这类设计（若不共享，这类玩法需要额外的桥接机制；若共享，需要设计冲突解决规则）。

### 2. `skill_cooldowns`/技能资源判定依赖的"当前资源值"字段是否已存在

任务 5 提到"`Agent` 尚无当前法力/耐力字段"的可能性，需要在任务 5 开工前核实 `crates/ll-world/src/entity/agent.rs` 与 `stats.rs` 的现状——本计划撰写时未能确认这一点（`attribute-system.md` 「六、次级属性」把"生命/法力/耐力，各自的上限与回复速率"列为设计层面的次级属性，但落地状态标注"尚未落地"）。**若确实缺失，任务 5 需要一并补上这个字段**，本计划不预先判断这算不算任务 5 范围内的工作,留给实现者核实后决定，若判断超出任务 5 合理范围，应该拆出一个前置子任务而不是在任务 5 里默默扩大范围。

### 3. 技能/任务是否需要接入 `WorldState::hash()`

任务 5 已经要求 `unlocked_skills`/`skill_cooldowns` 接入（比照坐标系重写计划任务 12 的纪律），但任务 7 的任务进度（脚本状态存储）是否也需要接入,取决于 P5-A 任务 8 自己是否接入 `hash()`（P5-A 待裁定/缺口一节已经标注这是可选项）——**两份计划在这一点上需要保持一致：若 P5-A 最终决定脚本状态不接入 `hash()`,本计划任务 7 的任务进度也不应该接入（否则会出现"部分脚本状态参与确定性校验、部分不参与"这种不一致），若 P5-A 决定接入,本计划应该跟进**。这不是本计划能独立裁定的,依赖 P5-A 的最终选择。
