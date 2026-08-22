# 天赋/特性系统（Trait）

**落地状态**：纯设计，无实现代码，`crates/**` 全代码库检索无 `TraitDef`/`register-trait`/`TraitTable` 等任何匹配。

**冻结于** 2026-08-20，基线提交 `383246d`（工作区除本文件外有另外三路并行工作的未提交改动，见下方并发声明；本文档只新增这一个文件）。

**并发声明**：本次任务与另外三路并行工作共享同一工作树——存档迁移链清理（`ll-content`）、本体贴图加命名空间前缀（`ll-mod/asset_vfs`+`ll-render`+`ll-game`+demo+`assets/`）、P6 属性接线（`ll-sim/resolve.rs`+`ll-world/entity/stats.rs`）。**任务过程中又追加了第四路**：等级与经验系统设计（同在 `knowledge/design/` 下，另一份文档，写作本文档时尚未存在，具体文件名未知）——两者分工已经协调过（见九节），本文档只新增这一个文件 + 更新 `README.md` 索引，不触碰 `crates/**`/`mods/**`/`assets/**`/`docs/**`/`.github/**`，也不代等级系统那份文档发言。

---

## 零、项目所有者的要求（两轮）

第一轮，起因是核实「mod 里能不能做出经典 D&D 种族设定」：

> 「能做一半——`register-race` 能表达六项属性修正、暗视下限、体型、寿命，做不到的是种族的灵魂：授予能力（龙裔吐息、精灵天生法术）、抗性（矮人抗毒、龙裔元素抗性）、改判定（半身人幸运、对某类豁免有优势）、亚种（丘陵矮人/山地矮人）。」「那还需要加上天赋特性的系统设计了。」

第二轮追加，验收范围从种族扩大到职业：

> 「对，你再加上 dnd 相关职业。」

并给出三个职业验收示例（野蛮人狂暴、盗贼偷袭、法师法术位）与两个必须正面核实的项目现状问题（有没有等级、资源池现状如何）。

---

## 一、现状核实

去代码与既有设计文档核实的结论，本节只列事实，判断留给后续章节。

**种族**（`crates/ll-mod/src/race.rs`，`RaceDef`/`RaceTable` **已落地**，与 `race-system.md`「落地状态」一节的旧记录不一致——该文档写作时 `RaceDef` 尚未落地，本文档写作时已经落地，如实更正）：

```rust
pub struct RaceDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId,
    pub stat_modifiers: BaseStats,   // 创建时一次性烘焙进 BaseStats，见 race-system.md 二节
    pub darkvision_cells: u32,
    pub footprint: (u8, u8),
    pub lifespan_years: u32,
}
```

没有任何字段能表达授予能力、抗性、改判定、亚种——与项目所有者的初判完全吻合。

**职业/副职**（`crates/ll-mod/src/class.rs`/`subclass.rs`，均已落地）：

```rust
pub struct ClassDef { pub id: NamespacedId, pub display_name_key: NamespacedId, pub primary_attribute: AttributeKind }
pub struct SubclassDef { pub id: NamespacedId, pub display_name_key: NamespacedId }
```

`ClassDef` 只有一个「主属性倾向」字段，`SubclassDef` 只有 `id`/`display_name_key` 两个字段——**都不携带任何数值/能力载荷**，只是可供技能学习闸门（`skill-learn-requirements.md`）与展示引用的标签。`Agent.profession: ContentIndex`、`Agent.subclasses: Vec<ContentIndex>` 均已落地并已进 `WorldState::hash()`。

**等级**：`Agent`（`crates/ll-world/src/entity/agent.rs`）、`ClassDef`、`SkillDef`（`crates/ll-mod/src/skill.rs`）全字段检索，**没有任何字段名含「level」或「等级」**。**核实结论：项目现在没有等级这个概念，一处都没有。** 与协调者的判断一致。

**资源与恢复**（`crates/ll-sim/src/skill.rs`）：

```rust
pub enum ResourceKind { Mana, Stamina }               // 只有两种，扁平，不分级
pub enum ResourceCost { None, Amount(ResourceKind, u32) }
pub enum SkillEffect {
    DealDamage { base: i32 },
    RestoreResource { resource: ResourceKind, base: i32 },
    TemporaryStatModifier { attribute: AttributeKind, amount: i32, duration_ticks: u32 },
}
```

`Agent.mana`/`Agent.stamina` 都是「只存当前值，不存上限」的裸 `i32`（`STARTING_MANA`/`STARTING_STAMINA` 是占位常量，上限公式尚未落地）。**全代码库检索 `rest`/`长休`/`短休`/`Rest` 无匹配——没有任何「休息恢复」机制**，资源恢复目前只能通过 `SkillEffect::RestoreResource`（等价于「喝一瓶法力药水」这种主动技能效果）触发。**`SkillRule` 一条技能只有一个 `effect: SkillEffect`（单值,不是列表），三个变体里没有任何一个能表达「进入一段带抗性的临时状态」或「有条件地追加伤害」。**

**判定/检定**：`damage-formula-mod-api.md` 七节已核实——本项目战斗结算目前没有「命中判定」这一步，`resolve_attack` 恒命中，只有伤害数值会变。这意味着 D&D 里「优势/劣势掷骰」「豁免检定」赖以存在的 d20 判定本身，在战斗场景下当前没有挂载点（`attribute-system.md` §八描述的 d20 判定机制存在但未落地，且从未接入战斗）。

**载具的授予技能模式**（`vehicle-and-mounting.md` 六节，纯设计）：「有效技能 = 已学会的 ∪ 当前载具授予的」，走 `skill_source` 函数派生，`granted_skills` 从不写进 `unlocked_skills`。**这是本文档第三节直接复用的既有模式**，但它本身也是纯设计，`resolve_use_skill` 门一目前只读 `unlocked_skills.contains`，尚未接这条并集。

**增益系统**（`buffs-and-triggers.md`，纯设计）：`ActiveEffect { def: ContentIndex, expires_at: Tick, stacks, applied_at, source }`——**`def: ContentIndex` 指向"注册表里的增益定义"，但该文档全篇从未定义这份注册表条目具体是什么类型**（只定义了触发它的 `TriggerDef`/`TriggerResponse::ApplyBuff { def, stacks, duration_ticks }`，没有定义 `def` 解析出来是什么）。**这是一处此前没人指出的真实缺口，本文档第二节要补的正是这个洞。**

---

## 二、天赋与 buff 是不是同一个东西

**结论：payload（一份效果载荷）共用同一个类型；实例化方式不共用——天赋是"引用",buff 是"实例"。**

### 先问 ADR 0021 的问题：有没有算法要共享

**有，而且是一节已经找出的那个具体缺口**：`buffs-and-triggers.md` 的 `ActiveEffect.def: ContentIndex` 需要解析出一份"这个增益具体授予什么"的数据——这份数据要回答的问题（授予哪些技能、修正哪些属性、修正哪些判定规则），与"种族/职业授予角色什么"是**完全同一个问题**，被两份此前独立写作的文档各自摸到了同一半，谁都没有把它写完整。这正是 ADR 0021 要求的判据：不是"看起来都是效果"这种表面对称，是**两处确实需要同一份可复用的载荷结构**。

### 但存储方式不共享——理由是两者的生命周期是两种性质不同的量

- **种族/职业/副职授予的天赋**：只要「你是这个种族/职业」这件事本身没变，天赋就一直生效——这不是一个"到期时刻"，是一个**引用关系是否仍然成立**的问题。`Agent.race`/`Agent.profession`/`Agent.subclasses` 已经是存档字段（已进 `hash()`），天赋不需要为自己另开一份实例状态，只需要"查 `RaceDef`/`ClassDef`/`SubclassDef` 挂了哪些天赋 ID"——**零存储，纯派生**（见六节）。
- **buff（含限时状态，如三级野蛮人狂暴）**：`expires_at` 是一个**真实的偏差**，不派生自任何既有字段，必须存进 `WorldState`（`buffs-and-triggers.md` 一节已经论证过，本文档不重复）。

**若把两者硬塞进同一套实例化机制**（例如给天赋也造一个 `ActiveEffect { expires_at: 永远 }`），会制造一个不必要的"永远"哨兵值——`Tick` 是一个真实世界时钟刻度类型，"永远"没有自然的表示（`Tick::MAX` 是可以工作但没有意义的技巧值，且每次比较 `tick < expires_at` 都要为这个不会真的发生的比较买单，是给"零存储、无需比较"的东西强行加了一次比较）。**天赋不实例化，直接避开了这个坑。**

### 具体形状：一份共享的 `TraitDef`（四节详述），两种消费路径

```
天赋（种族/职业/副职/装备授予）：owner.traits: Vec<TraitGrant>（六节新形状）
                                   —— 纯引用，查 TraitTable 即得完整效果，零实例存储
buff（技能/触发器施加的限时状态）：Agent.active_traits: Vec<ActiveTraitInstance>（本节新增）
                                   —— 每条引用同一个 TraitTable 的 ContentIndex，
                                      外加一份真实偏差（expires_at/stacks/applied_at/source）

pub struct ActiveTraitInstance {
    pub def: ContentIndex,      // 指向 TraitTable——buffs-and-triggers.md 的
                                 // ActiveEffect.def 此前一直缺失的具体类型，
                                 // 本文档在这里补上
    pub expires_at: Tick,
    pub stacks: u32,
    pub applied_at: Tick,
    pub source: EntityId,
}
```

`ActiveTraitInstance` 就是 `buffs-and-triggers.md` 的 `ActiveEffect`，本文档只是给它的 `def` 字段第一次指定了具体类型（`TraitTable` 的 `ContentIndex`）。**项目所有者原话「一个 buff 就是有到期时间的天赋，一个天赋就是永不过期的 buff」在效果载荷这一层完全成立**——两者读的是同一张 `TraitTable`；在**是否需要实例化**这一层不成立——天赋从不实例化（引用即生效，零存储），buff 必须实例化（到期是真实偏差，必须存储、必须进 `hash()`）。

### `buffs-and-triggers.md` 要不要改

**要改，但改动很小，且不在本次任务的写权限内，如实标注为待办**：该文档需要补一句——`ActiveEffect.def: ContentIndex` 指向的正是本文档新定义的 `TraitTable` 条目（`TraitDef`）；`TriggerResponse::ApplyBuff { def, stacks, duration_ticks }` 的 `def` 同理。这不需要重写任何一节的论证（惰性到期判定、确定性合并顺序、`StackPolicy`、触发器深度上限全部不变），只需要在"三、命中效果只是触发器的一种"补一行"`ApplyBuff.def` 解析进 `TraitTable`"。**本次任务权限只允许写本文件 + `README.md`，这处补丁留给下一次触碰该文档的批次。**

---

## 三、天赋效果的四类表达

### ① 授予能力——直接复用载具已确立的模式，职业技能同样适用

`vehicle-and-mounting.md` 六节的「有效技能 = 已学会的 ∪ 当前载具授予的」直接推广：

```
有效技能(agent) = agent.unlocked_skills
                 ∪ granted_skills(agent.race)       —— 龙裔吐息
                 ∪ granted_skills(agent.profession)  —— 职业主动技能（若职业本身该授予）
                 ∪ granted_skills(agent.subclasses)
                 ∪ granted_skills(agent.mount_profile)   —— 载具，既有设计
                 ∪ granted_skills(agent.active_traits)   —— 限时状态（狂暴授予的"狂暴攻击"）
```

`granted_skills(X)` 统一定义为「遍历 X 引用的每一条 `TraitGrant`，取其 `TraitDef.granted_skills` 的并集」——五个来源用同一个函数、同一条并集规则，不是五套并行逻辑。**确认可行，无需新机制**，`resolve_use_skill` 门一未来需要从"只读 `unlocked_skills`"扩展成读这条并集（与载具那处遗留的接线缺口是同一处，不是天赋系统新增的缺口）。

### ② 属性修正——两条既有通道，按"能不能被剥夺"分流，不是一条通道通吃

- **种族固有修正**（一生不变）：`race-system.md` 二节的既有纪律——创建时一次性烘焙进 `BaseStats`，此后与种族脱钩。
- **职业/副职/装备/buff 授予的修正**（来源可以改变——转职、卸装、buff 到期）：走 `Agent.active_stat_modifiers`（已落地、已进 `hash()`），**不能烘焙**——烘焙意味着"哪怕来源消失了，效果还留着"，这与"可剥夺"的语义矛盾。

**这不是两套机制，是同一份 `TraitDef.stat_modifiers` 数据被两种不同的消费方式使用**，选哪种消费方式取决于「这个天赋的来源在游戏中会不会变化」，不取决于天赋本身的数据形状——与 `vehicle-and-mounting.md` 六节"移动速度替换 vs 其余属性叠加，判据是场景不是字段"是同一层次的判断。

### ③ 改变规则本身——新增 `RuleModifier`，声明式，减伤链路读它而不是事件回调

**这是本任务最难的部分，也是本节篇幅最长的部分。**

#### 为什么不能是监听器

`mod-lifecycle-and-event-api.md` 已经定死：监听器只读通知、不能改结果（改结果要求 `apply` 支持回滚，违反 C1）。「受到毒伤时改成一半」必须是**伤害管线主动去读的一份数据**，不是"伤害发生后有人举手要求打折"。

#### 形状：封闭枚举，走一档（声明式，ADR 0016/0017）

```rust
/// 一条规则修正——改变判定/减伤本身怎么算，不是加减一个数值。
/// 走注册表第一档：声明式数据，注册期物化，运行期查表/直接消费，
/// 不经过任何脚本回调（三步判据见 ADR 0018，本节末尾核对）。
pub enum RuleModifier {
    /// 抗性：该伤害类别的伤害，在既有减伤链路算完之后再打一个千分比
    /// 折扣——挂载点直接复用 damage-formula-mod-api.md 二十节已经定好
    /// 的位置（减伤之后、乘数形式），伤害类别走该文档十七节的开放
    /// Registry::intern 集合，本节不重新设计，只补上"谁能声明这个乘数"
    /// 这一环。0=免疫，500=半伤，2000=双倍。
    Resistance { damage_category: ContentIndex, multiplier_permille: i32 },
    /// 重骰：该实体在damage-formula-mod-api.md六节消耗的每一次单点骰子
    /// 抽取（Dice/AdvantageRoll/DisadvantageRoll/MultiRoundHit 内部的
    /// 每一次 gen_range 调用），若抽出的点数等于 value，立即重抽一次，
    /// 取新值（不再检查新值是否又是 value）——见七节实现细节。
    RerollOnce { value: i32 },
    /// 优势/劣势：该实体在某类判定上默认套用 adv/disadv（伤害公式
    /// 六节的同名算子）。占位变体，当前无消费者——本项目没有"判定/
    /// 检定"系统（一节已核实），check_context 的具体值域留给该系统
    /// 落地时一并定案，本节只声明这个变体存在，不假装它已经能工作。
    Advantage { check_context: NamespacedId },
    Disadvantage { check_context: NamespacedId },
}
```

#### 抗性：挂载点已经现成，本节只补"谁能声明"

`damage-formula-mod-api.md` 二十节已经把抗性的挂载点定死在"减伤之后、乘数形式"，但**该文档从未回答"这个乘数从哪来"**——本节补上：`resistance_multiplier(defender, damage_category)` 遍历 `defender` 的有效天赋（六节的并集算法），收集全部 `RuleModifier::Resistance` 里 `damage_category` 匹配的条目，取乘数（多条命中时，按 `TraitGrant` 的 `ContentIndex` 升序取第一条，与 `buffs-and-triggers.md` 二节"多个增益改同一属性时结算顺序必须确定"同一条纪律——不取乘积，理由是"免疫 500‰ 又免疫一次"不应该变成 25% 而不是 0%，取第一条命中即可，不是数值设计范畴，本文档不深入）。

> **落地后的更正（抗性多来源聚合批次）**：这条规则本身原样保留，但它的**作用范围已经不止天赋**。项目所有者对抗性来源的裁定是「抗性肯定会来自天赋，以及装备，还有各种药品，或者技能」四路，实现上因此把「收集候选」与「多条命中怎么取」拆成了两层：各路来源各有一个收集器（天赋走 `ll_sim::rule_modifier::trait_rule_modifiers`，装备走 `equipment_rule_modifiers`），tie-break 则由唯一的消费者 `ll_sim::rule_modifier::resistance_multiplier_permille` 执行，判据从「天赋的 `ContentIndex`」推广成「**声明这条修正的内容条目**的 `ContentIndex`」——天赋一路的行为逐位不变。跨来源使用同一把尺子时留下一个内容设计问题（同类别上天赋抗性与装备抗性谁生效，取决于谁先被 intern，而不是谁更强），已如实记录在该模块文档「跨来源 tie-break」一节，等待所有者裁定。

#### 重骰：不是新的 `FormulaOp`，是骰子取数原语本身的一个可选钩子

**这是本节最需要讲清楚"为什么不那样设计"的一点。** 一个自然的错误方向是：把"重掷 1"编译成伤害公式里的一个新算子，例如 `(reroll-once 1 (d 1 20))`，要求 mod 作者在**每一处**引用骰子的地方手写这个包装。**否决**：半身人幸运是种族天赋，不管半身人用哪把武器、哪个技能，都应该生效——若做成公式算子，半身人用一把没写 `reroll-once` 包装的武器就不生效，等于要求武器作者替每个可能拿到这把武器的种族预先想好所有重骰规则，本末倒置。

**正确的挂载点是骰子取数本身**：`damage-formula-mod-api.md` 六节定义的求值器在遇到 `Dice`/`AdvantageRoll`/`DisadvantageRoll`/`MultiRoundHit` 时，从共享随机流里连续取值——本节把这个"取一个骰子点数"的动作本身包一层：

```
roll_one_die(stream, sides, roller_rule_modifiers) -> i32:
    v = stream.gen_range(sides) + 1
    若 roller_rule_modifiers 里存在 RerollOnce { value } 且 v == value:
        v = stream.gen_range(sides) + 1   // 重抽一次，只重抽这一次
    返回 v
```

`Dice { count, sides }` 内部原本"连续取 `count` 次 `gen_range(sides)+1`"的每一次都换成调这个函数——**求值器本身完全不需要知道"重骰"这个概念是从种族天赋来的**，它只是多带了一个"这次掷骰的主体是谁"的上下文，用来查一次那个主体的有效天赋。这与 `race-system.md` 五节"暗视只改变喂给半径计算的输入，不碰 FOV 算法本身"是同一个设计动作——**重骰只改变喂给骰子结果的输入来源判断，不碰求值器遍历指令数组这件事本身**。多消耗的随机抽取次数（重骰这一次）需要计入 `六节"取数顺序"`的确定性纪律：仍然是"同一条共享流，按需要连续取值"，只是"需要几次"从"骰子面数固定"变成"最坏情况下多一次"——这仍然是一个**编译期可判定的有界值**（每个骰子最多重骰一次），与 `multi-hit` 当初"有界重复"的论证同构，不引入无界循环。

**代价诚实标注**：这要求 `roll_one_die` 这个新的骰子取数函数知道"谁在掷这颗骰子"（用来查它的有效天赋），而 `damage-formula-mod-api.md` 现有的求值器签名（六节）只知道"这是哪个 `FormulaDef`"，不显式携带"attacker 是谁"这个身份——**这处接线需要该文档在实现阶段补一个参数，本文档不代为改写它的求值器签名，只指出这个缺口的位置。**

#### 条件式规则修正（盗贼偷袭）——如实标注：结构上可以留口子，数值上目前算不出来

**核实结论：当前架构下，"满足某个战术条件才生效"这类规则修正，能表达的深度非常有限，真实的偷袭规则表达不出来。**

`RuleModifier` 理论上可以加一层：

```rust
pub struct ConditionalRuleModifier {
    pub condition: ConditionKind,   // 开放注册表，同 TriggerKind 一样可扩展
    pub modifier: RuleModifier,
}
```

问题不在这层包装本身（包装本身是平凡的），**问题在于 `ConditionKind` 要怎么被求值**。伤害公式的求值器（`damage-formula-mod-api.md` 六节）已经证明了一种可行模式——"外部算好一个 0/1，喂给公式当操作数"（`crit` 正是这么做的：`resolve` 在求值前用幸运值算好暴击与否，公式内部只读 `Crit` 操作数，不自己判断）。**偷袭的条件（"你对目标有优势，或者目标 5 尺内有你的盟友"）需要的输入，`resolve` 现在完全拿不到**：

- 「是否有优势」——依赖判定系统（一节已核实：不存在，`resolve_attack` 恒命中，没有 d20 判定，因此也没有"优势"这个中间态可查）。
- 「目标附近是否有你的盟友」——依赖一次以目标为中心的空间查询（"半径 N 格内是否存在与攻击者同阵营的实体"）。`script-entity-handles-and-batch-queries.md` 定义了批量查询原语，但 `resolve_attack`/伤害公式求值器目前都不接受"再做一次额外的实体查询"这类输入——它们的输入是攻防双方各自的属性与穿透，不是"战场态势"。

**结论：`ConditionKind` 这个口子本身可以现在就声明（结构成本几乎为零），但除了"永远为真"这个退化条件之外，没有任何真实条件目前可求值。** 偷袭在当前架构下只能退化成两种近似之一，两种都明确不等价于原版规则：

1. **拿掉"条件"，做成恒定加骰**——一个天赋，`rule_modifiers` 留空，改用②节的效果层：一个专门的主动技能"偷袭"，`SkillEffect::DealDamage { base }` 里把骰子加成算进 `base`，玩家自己选择使用，不做成被动自动判定。**这是唯一今天就能落地的近似**，代价是把"要不要触发"的决策权从"规则自动判断条件"移交给"玩家手动点技能"，与 D&D 原版"符合条件就自动生效"的手感不同。
2. **等判定系统 + 空间查询接入 `resolve_attack` 之后再做真正的条件版本**——`ConditionKind::AllyAdjacentToTarget` 这类变体现在可以在文档里预留名字，但不实现、不假装能用（与 `RuleModifier::Advantage` 同样的"声明占位、如实标注无消费者"处理方式）。

**这不是天赋系统本身的缺口，是 `resolve` 阶段"掌握多少战场上下文"这个更底层的缺口——天赋系统只能声明它想读的条件，读不到的条件不会凭空被生出来。**

#### 三步判据核对（ADR 0018）与档位

第一步：有没有自由度——有，"这个天赋改哪种规则"是内容设计的自由。第二步：自由度落在数据上还是算法上——落在**数据**上：`RuleModifier` 的具体取值（哪个伤害类别、多少千分比、重骰哪个点数）全部是注册期已知的静态量，运行期不需要任何"这一刻才存在"的输入去决定"用哪条规则"（`ConditionKind` 那半句"是否满足条件"确实需要运行期输入，但正如上一节指出，当前没有可用的输入源，因此当前唯一能落地的部分——`Resistance`/`RerollOnce`——完全是纯数据）。第三步：调用频率——`Resistance` 在减伤链路里逐次攻击查一次（与穿透同频率），`RerollOnce` 在每次骰子取值时查一次（与骰子取值同频率）——都是热路径，**必须一档，不能落到脚本回调**，与伤害公式本身的分档理由完全同构。

#### 能不能被 mod 扩展

**`RuleModifier` 的变体集合是封闭的 Rust 枚举，不能被 mod 添加新变体**——与 `buffs-and-triggers.md` 的 `TriggerResponse`（`ApplyBuff`/`DealDamage`/`Formula`/`Script` 四个变体同样封闭）是同一层判断：ADR 0017 要求一档/二档内容压平成 Rust 侧可以直接匹配的固定操作类型，"操作的种类"必须封闭，"操作作用在哪个具体值上"（`damage_category`/`value`/`check_context`）必须开放。**mod 能做的**：用已有变体表达新内容（"精灵抗魅惑"是新的 `Resistance { damage_category: charm_id, ... }`，`damage_category` 走 `damage-formula-mod-api.md` 十七节已经开放的 `register-damage-category`，mod 可以先注册一个新伤害类别再声明抗它）——**mod 不能做的**：发明一种全新的规则修改方式（例如"每次未命中都能立刻再攻击一次"这种全新的战斗节奏改写），那需要 Rust 侧新增一个 `RuleModifier` 变体，是引擎层改动，不是天赋声明能表达的自由度。**这不是遗漏——`buffs-and-triggers.md` 的 `TriggerResponse::Script` 是这条边界的既有逃生舱**，本设计同构地留一个未来口子（不现在设计）：若某天真的需要"规则修改本身要跑一段任意逻辑"，走三档脚本回调（`TraitDef` 可以有一个 `Script(NamespacedId)` 变体，同构于 `TriggerResponse::Script`），**本次三个示例都不需要它，不现在加**（YAGNI）。

### ④ 资源池容量——`resource-pools-and-rest.md` 三节提出的补丁，按其精确要求落地

**来源：`resource-pools-and-rest.md`（提交 `2e7dc02`）三节采纳"资源池由天赋授予"这个方向后，指出 `TraitDef` 需要第四类效果，并在该文档三节末尾精确列出了补丁形状，标注为待办——该文档没有本文件的写权限。本小节按其原文落地，不另发明一套。**

法力池、法术位、血法力许可不是 `Agent` 上人人都有的固定字段，而是天赋（种族/职业/副职/装备/buff）授予的能力——这是①②③已经确立的"没有对应授予关系就是没有，`effective_traits` 找不到匹配即为零"这条纪律的第四次应用，不是新发明。

```rust
pub granted_resource_pools: Vec<ResourcePoolGrant>,   // TraitDef 新增第四个字段，见下方四节

/// 一条"这个天赋授予多少这种资源池容量"的声明。
pub struct ResourcePoolGrant {
    pub pool: ContentIndex,          // 指向 ResourcePoolDef（resource-pools-and-rest.md 二节）
    pub capacity: CapacityFormula,
}

pub enum CapacityFormula {
    Fixed(u32),                        // 容量恒定，不随等级变化——血魔法许可、多数标量池
    ByLevel(BTreeMap<u32, CapacityValue>), // 随 Agent.level 查表，阶梯式增长；未覆盖的等级
                                            // 取小于等于它的最大已声明等级对应的值
}

pub enum CapacityValue {
    Scalar(u32),
    Tiered(Vec<u32>),   // 每档一个数，索引 0 = 第 1 档（法术位环位）
}
```

**存储**：`Agent` 新增 `resource_pools: BTreeMap<ContentIndex, i32>`（标量池当前值，绝对量）与 `spent_slots: BTreeMap<(ContentIndex, u8), u32>`（法术位已消耗数，偏差量）——容量本身不存储，`effective_capacity(agent, world, pool)` 每次现算，与 `resource_pools`/`spent_slots` 只存"当前偏离了多少"是同一条"默认派生、只存偏差"的纪律（八节已编号到十二个实例的既有清单，本条是第十三个）。

**聚合规则**：`effective_capacity` 复用 `effective_traits(agent, world)`（四节既有函数，不重新实现聚合逻辑），对全部命中 `pool` 的 `ResourcePoolGrant` **求和**——与三节②`stat_modifiers` 的叠加语义一致，**不是** `Resistance`"取第一条命中"的语义：`Resistance` 取第一条是为了避免"免疫乘免疫"的荒谬结果，容量是"两个来源各自贡献一部分"的自然叠加，两者性质不同，不能套同一条规则。

**容量变化时读时钳位，不主动改写存储值**：容量变大（升级、新装备）时 `resource_pools`/`spent_slots` 不自动补满/清空，靠既有恢复规则自然填上差距；容量变小（掉装备、天赋失效）时不回写存储值，改为每次读取"当前可用量"时现场钳位——标量池 `usable = min(stored_current, effective_cap)`，法术位 `remaining(tier) = effective_cap(tier).saturating_sub(stored_spent(tier))`。**不主动改写的理由**：若做成"变化时立刻遍历改写"，需要一套"天赋/装备变化时通知资源池"的观察者机制，与 `buffs-and-triggers.md`"惰性判定优于事件驱动"（约束 C4）同一条精神相悖——查询时现比较一次，比维护一套变化通知便宜。

**血池不走这条通道**：血池的容量是最大生命值，来自体质衍生（`Agent.health` 既有纪律，公式尚未落地），不是天赋容量表；`granted_resource_pools` 只服务需要一个数字（容量）的资源——标量池与法术位，不服务血池。血法师"能不能用血代价"完全由"会不会某个 `resource_cost` 是 `Blood(N)` 的技能"决定，已经是①`granted_skills` 在管的事，不在此重复。

**调用频率**：`effective_capacity` 只在技能结算（每次 `Intent::UseSkill` 一次）与回合开始的自动恢复检查（每个实体自己的回合一次）两处被调用——与 `effective_traits`/`resistance_multiplier` 同一档，不是热路径。

---

## 四、天赋归谁所有

**结论：独立内容类型 `TraitDef` + 被引用，不是让种族/职业/副职/装备各自长一份效果字段。**

### 为什么——真正共享的算法是什么

- **聚合算法共享**：三节①已经证明"有效技能 = 并集"这条算法要同时喂给种族、职业、副职、载具、buff 五个来源；②"哪条通道消费属性修正"、③"抗性/重骰去哪张表查"、④"`effective_capacity` 去哪张表求和"，全部要遍历"这个实体当前持有的全部天赋"，`effective_capacity(agent, world, pool)` 与 `effective_traits` 共用同一份聚合遍历，不是第六套并行逻辑。**若每个所有者类型各自长一份 `granted_skills`/`stat_modifiers`/`rule_modifiers`/`granted_resource_pools` 字段（而不是引用同一张 `TraitDef` 表），聚合函数就要对五种不同的宿主类型各写一份取字段的代码，字段名/类型哪怕有一处漂移（例如某天有人给 `ClassDef` 的字段改了名字）都会在聚合函数里产生不对称的特例分支**——这正是二节"为什么天赋要和种族脱钩"一节论证过的同一类风险的重演。
- **DRY**：龙裔吐息这个天赋，若龙裔亚种、某个"龙裔血统"副职、某件传说装备都想授予它，独立注册一次、三处引用同一个 `ContentIndex`，比在三处分别声明三份等价但独立维护的效果数据更不容易漂移——与 `class-skill-quest-system.md`「主职与副职共享同一份技能命名空间」的理由（避免复制导致的内容漂移）完全同构。

### `register-trait` 签名与档位

```scheme
(register-trait "lostland:dwarven_resilience"
  "lostland:trait.dwarven_resilience.display_name"
  (list)                                    ; granted-skills：授予的技能 ID 列表
  (list)                                    ; stat-modifiers：(属性 增量) 对列表
  (list (resistance "lostland:poison" 500)))  ; rule-modifiers：见三节 RuleModifier
```

```rust
pub struct TraitDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId,
    pub granted_skills: Vec<ContentIndex>,      // 三节①
    pub stat_modifiers: Vec<(AttributeKind, i32)>, // 三节②，格式复用 vehicle-and-mounting.md 六节
    pub rule_modifiers: Vec<RuleModifier>,      // 三节③
    pub granted_resource_pools: Vec<ResourcePoolGrant>, // 三节④，resource-pools-and-rest.md 三节要求
}
```

**档位：一档。** 三步判据：有自由度（mod 能声明任意新天赋）；自由度落在纯数据上（`granted_skills`/`stat_modifiers`/`rule_modifiers`/`granted_resource_pools` 全部是注册期一次性交出的值，运行期只查表/遍历小列表，不消费运行期才存在的输入——三节已经论证过 `Resistance`/`RerollOnce` 各自的消费点都不需要脚本回调，`ResourcePoolGrant.capacity` 同理是注册期定死的公式，不消费运行期输入除了 `agent.level` 本身）；调用频率——天赋聚合发生在"这个实体当前持有哪些天赋"每次被查询时（技能可用性判断、减伤链路、骰子取值、资源池容量查询），是战斗结算的热路径，必须一档。

**注册期校验**：`granted_skills` 每项必须已经 `register-skill` 注册过（与 `vehicle-and-mounting.md` 校验 `granted-skills` 同一条纪律）；`stat_modifiers` 的属性名必须是六个既有 `AttributeKind` 变体之一；`Resistance.damage_category` 必须已经 `register-damage-category` 注册过（复用该文档 21 节 `MountTable::define` 校验 `grants-passage` 表面 ID 的同构纪律）；`granted_resource_pools` 里的 `pool` 必须已经注册过对应的 `ResourcePoolDef`（校验时机与形式留给 `resource-pools-and-rest.md` 自己的注册 API 裁定，本文档只声明字段与聚合规则）；重复定义同一个 `TraitDef` 索引——报错，不静默覆盖（与全部既有 `*Table::define` 同一条纪律）。

---

## 五、亚种

**结论：不照抄副职，扩展 `RaceDef` 本身——`parent_race: Option<ContentIndex>` + `traits: Vec<TraitGrant>`，不新开 `SubraceDef` 类型。**

### 核实副职的既有形状——不是"基础+一层修正"的形状

一节已核实：`SubclassDef` 只有 `id`/`display_name_key` 两个字段，**没有任何数值/效果载荷**，`Agent.subclasses` 只被 `skill-learn-requirements.md` 的 `SkillRequirement.subclasses` 当成"是否在这个列表里"的成员判断消费。**副职今天的唯一职责是"技能学习的一道闸"，不是"给基础职业叠一层修正"**——D&D 的亚种恰恰需要后者（丘陵矮人在矮人基础修正之上再叠 +1 感知，山地矮人叠 +2 力量 +2 体质），这不是副职现有代码路径能表达的东西，套用它的形状只会得到一个"能挂但不生效"的空壳。

### 按 ADR 0021 判据核对：副职与亚种有没有共享算法——没有

副职的消费算法是"成员判断"（`req.subclasses.iter().any(|s| subclasses.contains(s))`，见 `skill-learn-requirements.md` 三节）；亚种需要的算法是"创建时数值叠加"（`race-system.md` 二节的既有烘焙步骤，只是要叠两层而不是一层）。**两者不是同一个问题，套同一个形状不会让代码变少，只会制造一个字段存在但语义不对的假抽象**——与 `Camera`/`BoundedCamera` 那次否决（ADR 0021 原始案例）是同一类误判：表面上都叫"变体"，但驱动它们的分别是"引用查表"与"数值合成"两种完全不同的操作。

### 最终形状：`RaceDef` 自己长出层级

```rust
pub struct RaceDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId,
    pub stat_modifiers: BaseStats,
    pub darkvision_cells: u32,
    pub footprint: (u8, u8),
    pub lifespan_years: u32,
    /// 新增：父种族——非 None 时表示这是一个亚种，创建时先烘焙父种族
    /// 的 stat_modifiers，再叠加自己的 stat_modifiers（数值直接相加，
    /// race-system.md 二节的烘焙步骤只需要多做一次"先取父再加己"）。
    pub parent_race: Option<ContentIndex>,
    /// 新增：这个种族/亚种自己授予的天赋——见六节 TraitGrant。
    pub traits: Vec<TraitGrant>,
}
```

**有效天赋**（六节聚合函数的直接应用）：`effective_traits(race) = traits(parent_race).unwrap_or(&[]) ∪ race.traits`——与三节①"有效技能=并集"是同一个模式的第三次复用（种族天赋并集是第一次的直接对象，这里是"父种族天赋 ∪ 子种族天赋"这个更具体的并集实例）。**不递归到祖父种族**——与 `race-system.md` 九节"混血不递归到祖父母"同一条纪律（`parent_race` 本身若也声明了 `parent_race`，注册期直接拒绝——不支持多级嵌套，亚种只有一层，与 D&D 原版"亚种不再有亚种的亚种"一致）。

**为什么不新开一个 `SubraceDef` 类型**：亚种要读的字段（`stat_modifiers`/`darkvision_cells`/`footprint`/`lifespan_years`/`traits`）与基础种族**完全相同**——丘陵矮人自己也有暗视、有体型、有寿命，不存在"亚种独有、基础种族没有"的字段。新开一个类型只会复制 `RaceDef` 全部字段再加一个 `parent_race`，不如直接给 `RaceDef` 加这一个可选字段。

---

## 六、天赋条目的形状：`TraitGrant`，等级字段放在"授予关系"上而不是 `TraitDef` 本身

### 与协调者的分工对齐，并给出一处精确的形状修正

协调者转达的分工——**"等级解锁"归天赋系统（天赋条目自带"需要等级"字段，种族天赋填 1），"现在几级"归另一份正在并行设计的等级/经验系统文档**——本文档采纳这条分工，理由（见下）成立，但对"字段放在哪"给出一处更准确的落点：

**不放在 `TraitDef` 自己身上，放在"谁在什么时候获得这个天赋"这条引用关系上：**

```rust
/// 一条"某个所有者在什么等级授予某个天赋"的引用——种族/职业/副职/
/// 装备/buff 的 traits 字段统一用这个类型的列表，不是裸 Vec<ContentIndex>。
pub struct TraitGrant {
    pub trait_id: ContentIndex,   // 指向 TraitTable
    /// 解锁所需等级。种族/副职/装备/buff 恒填 1（"拥有即生效"，
    /// 这些来源本身不随等级变化）；职业天赋按实际设计填对应等级。
    pub unlock_level: u32,
}
```

**为什么不直接放在 `TraitDef` 上**：同一个天赋内容，理论上可能被两个不同职业在不同等级授予（例如"额外攻击"这类效果，若某天设计出两个都有近战爆发定位的职业，各自在自己的等级曲线上解锁它，复用同一份 `TraitDef` 效果载荷）——"什么时候解锁"是**授予关系**的属性，不是**效果载荷**的属性,把它放在 `TraitDef` 上会让同一份效果载荷被迫为"谁在什么时候拿到它"这件事负责,而这件事因所有者而异。这与四节"为什么种族/职业/副职/装备/buff 该共享同一张 `TraitTable`,而不是各自长字段"是完全同一层理由的另一次应用——**载荷共享，关系各自declare**。

**种族/亚种/副职/装备/buff 恒填 `unlock_level = 1`，不是因为"没有等级"这件事需要特判**，是因为这些来源本身的存在与否不随等级变化（你拥有这个种族的那一刻起就该有它的天赋,不存在"三级矮人才有抗毒"这种设计），`unlock_level = 1` 与"任何等级都满足"在数值上恒等——**不需要一个独立的"不限等级"哨兵值，用最小合法等级表达"总是满足"是最省心的写法**，与 `skill-learn-requirements.md` 三节"空列表表示不限"是同一种"用默认值表达'无限制'"的思路。

**`unlock_level` 只回答"存在与否"，不回答"存在之后有多少"——与三节④ `ResourcePoolGrant.CapacityFormula::ByLevel` 是两个不同的轴，不要混用**：法师"5 级获得三环位、9 级获得五环位"这条曲线，不通过声明多条 `TraitGrant`（"法师 1 环位+1""法师 1 环位+2"……那样既冗余又难维护）表达，而是单条 `TraitGrant { unlock_level: 1 }` 声明"法术位这件事从 1 级就成立"，容量随等级怎么涨完全交给 `ResourcePoolGrant.capacity: CapacityFormula::ByLevel` 去查表——前者回答"有没有这个池"，后者回答"有了之后具体是多少"（`resource-pools-and-rest.md` 三节原始论证）。

### 消费者需要 `Agent.level`——本文档不引入它，明确标注为待另一份文档补齐的假设

**`unlock_level` 字段现在就可以声明（结构成本为零），但它完全无法被消费，直到 `Agent` 上出现某种"当前等级"字段。** 本文档核实过（一节）：这个字段今天不存在。**本文档对它的假设**：

- 假设它是一个 `u32`，随职业/角色成长单调不减（不假设具体的增长来源——经验值、里程碑、还是别的机制，那是等级系统文档的职责）。
- 假设"某职业在某等级授予某天赋"这条判定的读取路径类似 `skill-learn-requirements.md` 已经定的"可学"闸门形状——`resolve_learn_trait`（名字待定，本文档不裁定）在某个触发点（升级时？打开天赋面板时？）遍历 `ClassDef.traits`/`SubclassDef.traits`，对 `agent.level >= grant.unlock_level` 且尚未拥有的天赋，产出一个"写入天赋"的效果——**这是否需要专门存储"已解锁的天赋集合"，还是直接靠"等级 ≥ 解锁等级"这条比较现算（零存储，与种族天赋一样纯派生），本文档倾向后者**（见七节"派生还是存储"），但等级系统文档若引入"回退等级"（降级、诅咒减益）之类的玩法，"现算"与"存储"哪个更合适需要重新核对——**如实标注为一处依赖对方设计细节、本文档现在无法完全裁定的接缝**。

**若等级系统文档对"等级"给出了与上述假设不同的形状**（例如不是单一 `u32`，而是每个职业各自独立计数——D&D 5e 多重职业正是这样：战士 3 级/法师 2 级，天赋解锁按"这个职业自己的等级"而不是"角色总等级"判断），**`unlock_level` 这个字段本身不需要改**——它比较的对象从"`agent.level`"换成"`agent.class_levels.get(&class_id)`"，字段的语义（"这条授予关系需要多少级"）不变，只是等级系统提供的读取路径变了。**这一点写清楚，是为了让对方文档知道：天赋系统这边不预设"角色只有一个全局等级"，无论对方选单一等级还是逐职业等级，`TraitGrant.unlock_level` 都能对接，不需要两边协调改字段形状。**

---

## 七、资源池与休息恢复——不属于天赋系统，如实标注边界所在

**核实结论（一节）：`ResourceKind` 只有 `Mana`/`Stamina` 两种，扁平，不分级；没有"休息"这个事件/`Effect`；资源恢复只能靠 `SkillEffect::RestoreResource` 这种主动技能效果。**

**后续更新（`resource-pools-and-rest.md` 提交 `2e7dc02`）：法师的分级法术位这半个问题——"归属哪个天赋、按等级给多少容量"——现在能在天赋系统这层声明了（三节④ `TraitDef.granted_resource_pools` + `CapacityFormula::ByLevel`），但下面三点缺口原样成立，不因三节④而改变**——三节④只回答"容量从哪个天赋来、多少"，不回答"这种资源池长什么样、怎么恢复、怎么消耗"，那三个问题依然完全在天赋系统之外：

1. **分级资源池的形状**——法术位不是"一个当前值"，是"每个环位各自一个计数"（1 环 4 个、2 环 3 个……），`ResourceKind` 需要变成能表达"某个种类下有 N 个独立子池"，或者干脆是另一套完全不同的资源模型，这是资源系统本身的重新设计，不是加一个新 `ResourceKind` 变体能解决的。**`resource-pools-and-rest.md` 已经给出这个形状（`ResourcePoolDef`/`ResourcePoolShape`，该文档二节），但那是该文档的产物，不是本文档的。**
2. **休息事件本身**——"长休恢复全部法术位、短休恢复部分"需要"长休"/"短休"是可以被触发的游戏动作（大概率是一个新 `Intent`/`Effect`），当前完全不存在。`resource-pools-and-rest.md` 四节已设计（复用 `Intent::Wait`/`Timeline` 机制扩展），仍未落地任何代码。
3. **技能消耗"某个环位"而不是"某种资源的 N 点"**——`ResourceCost::Amount(ResourceKind, u32)` 表达的是"扣一种资源的固定数量"，法术位消耗的语义是"占用一个环位的一个格子"，格子被占用后要等恢复事件才能重新使用，这是"计数池"而不是"扣血条"的形状,现有 `ResourceCost` 天生表达不了。

**本文档的边界**：`TraitDef.granted_skills` 引用的技能，其 `ResourceCost` 只能引用**已经存在**的 `ResourceKind` 变体（`Mana`/`Stamina`）——天赋系统不能、也不该在自己的设计里顺手发明新的资源池形状,那会让"资源系统该长什么样"这个更大的决定被一个天赋系统文档的附带产物意外定型。**三节④的 `granted_resource_pools` 没有违反这条边界**：它只引用一个已注册的 `ResourcePoolDef`（`ContentIndex`）声明"这个天赋给多少容量"，不定义池子本身长什么样、怎么恢复、怎么消耗——那三件事仍然是 `resource-pools-and-rest.md` 的份内工作,本文档不代为设计。

**野蛮人狂暴的资源消耗则可以今天就表达**——狂暴不需要"每日次数"这种分级池,复用已存在的 `Stamina`（`ResourceCost::Amount(Stamina, N)`）在数值预算上是合理的替代（D&D 原版"每长休 N 次"这条限制本身依赖"长休"事件,同样缺失,但"消耗耐力"这个替代形状不依赖它,见九节示例四）。

---

## 八、派生还是存储

**结论：天赋本身（`TraitDef`/`TraitTable`/`TraitGrant` 列表）派生，不存；`ActiveTraitInstance`（buff 化的天赋）存储，且必须进 `hash()`。**

- **`TraitTable`**：内容注册表，与 `RaceTable`/`ClassTable`/`SkillTable`/`SubclassTable` 同类，**不进 `WorldState::hash()`**——`crates/ll-world/src/state.rs` 已核实（一节引用）只哈希 `Agent` 的实例字段（`race`/`profession`/`subclasses`/`luck`/`active_stat_modifiers`/`unlocked_skills`……），从不哈希任何 `*Table` 注册表本身。
- **`RaceDef.traits`/`ClassDef.traits`/`SubclassDef.traits`（`Vec<TraitGrant>`）**：注册表数据的一部分，同样不进 `hash()`。
- **"这个实体当前有效的天赋"**：纯函数 `effective_traits(agent, world) -> Vec<ContentIndex>`，输入是 `agent.race`/`agent.profession`/`agent.subclasses`（已存储、已 `hash()`）+ 各自的 `RaceTable`/`ClassTable`/`SubclassTable` 查表结果 + `agent.level`（等级系统落地后）+ `agent.active_traits`（下条）——**每次要用时现算，不缓存进存档**，与 `attribute-system.md` 七节"衍生属性绝不进存档"、`buffs-and-triggers.md` 一节"现在是否生效不存，只存到期时刻"是同一条纪律的第十二个实例（`README.md`「默认派生，只存偏差」一节已经编号到十一，本设计是第十二个独立实例）。
- **`Agent.active_traits: Vec<ActiveTraitInstance>`（二节新增，buff 化天赋的实例）**：**真实偏差，必须存储，必须进 `hash()`**——`expires_at`/`stacks`/`applied_at`/`source` 都是无法从别处推出来的值，与 `active_stat_modifiers`/`unlocked_skills` 已经进 `hash()` 的既有先例同一条纪律（ADR 0022「覆盖不全的确定性哈希等于没有确定性哈希」——这个字段一旦落地就必须从第一个提交起就进 `hash()`，不能像 `player_entity` 那次一样先漏后补）。

**若某天真的需要"已经拿到过哪些天赋"这种可能偏离"等级现算"的记录**（例如允许玩家在满足条件后延迟领取，或者某个天赋一旦解锁即使降级也保留）——那是一处真实的存储需求，**但本文档现在不预先加这个字段**（YAGNI，七节末尾已经标注这是留给等级系统文档核对的接缝，此处不重复展开）。

---

## 九、六个示例

### 种族天赋

**示例一：矮人抗毒（无条件抗性）**

```scheme
(register-damage-category "lostland:poison" "")   ; 若尚未注册
(register-trait "lostland:dwarven_resilience"
  "lostland:trait.dwarven_resilience.display_name"
  (list) (list)
  (list (resistance "lostland:poison" 500)))       ; 减半
(register-race "lostland:dwarf" "lostland:race.dwarf.display_name"
  ;; stat-modifiers/darkvision/footprint/lifespan（既有六项，省略）
  (list (trait-grant "lostland:dwarven_resilience" 1)))
```

`resistance_multiplier(defender, "lostland:poison")` 遍历 `effective_traits(defender)`，命中这条 `Resistance`，返回 `500`。**完全可表达，三节③已给出挂载点，四/五/六节给出所有权与等级形状。**

**示例二：龙裔吐息（授予能力）**

```scheme
(register-skill "lostland:breath_weapon" ...)      ; 既有 register-skill，不变
(register-trait "lostland:draconic_breath"
  "lostland:trait.draconic_breath.display_name"
  (list "lostland:breath_weapon") (list) (list))
(register-race "lostland:dragonborn" "lostland:race.dragonborn.display_name"
  (list (trait-grant "lostland:draconic_breath" 1)))
```

`granted_skills(agent.race)` 命中 `lostland:breath_weapon`，进"有效技能"并集（三节①）。**完全可表达，直接复用载具已确立的模式，`resolve_use_skill` 门一的接线缺口是既有缺口，非本设计新增。**

**示例三：半身人幸运（重掷 1，改判定）**

```scheme
(register-trait "lostland:halfling_luck"
  "lostland:trait.halfling_luck.display_name"
  (list) (list)
  (list (reroll-once 1)))
(register-race "lostland:halfling" "lostland:race.halfling.display_name"
  (list (trait-grant "lostland:halfling_luck" 1)))
```

**结构上可表达，但有一处诚实的范围收窄**：三节③已经论证——本项目没有"判定/检定"这一层（一节核实），5e 原版"重骰攻击/检定/豁免的 d20"因此没有挂载点；本设计把"重骰"机制下沉到伤害公式**任意骰子取值**这一层（`roll_one_die` 钩子），所以半身人今天能生效的场景是"造成伤害用到的骰子"（例如武器伤害骰 `(d 1 4)`），**不是**D&D 原版真正针对的攻击/检定/豁免骰——因为后者不存在。**机制完全就位，等判定系统落地后，同一个 `RuleModifier::RerollOnce` 不需要改一个字段就能覆盖到 d20 判定，只是"喂给谁用"的调用点从伤害公式求值器扩展到判定求值器。**

### 职业天赋

**示例四：野蛮人狂暴（资源 + 限时状态，同时考验天赋/buff/资源池接缝）**

```scheme
(register-trait "lostland:rage"
  "lostland:trait.rage.display_name"
  (list "lostland:reckless_attack")                 ; 狂暴期间授予的额外攻击方式（示意）
  (list ("strength" 2))                              ; 伤害加值——走②节的 active_stat_modifiers 通道
  (list (resistance "lostland:physical" 500)))        ; 抗物理——走③节 RuleModifier
```

`SkillDef` 的 `SkillEffect` **需要新增第四个变体**（本文档核实：当前只有 `DealDamage`/`RestoreResource`/`TemporaryStatModifier` 三个，均无法表达"授予一整份天赋载荷、限时"）：

```rust
// crates/ll-sim/src/skill.rs（设计，未落地）——SkillEffect 新增变体
ApplyTrait { trait_id: ContentIndex, duration_ticks: u32 },
```

`apply` 阶段消费这个变体：往 `agent.active_traits` 塞一条 `ActiveTraitInstance { def: trait_id, expires_at: clock + duration_ticks, stacks: 1, applied_at: clock, source: agent_id }`。**这正是二节论证的核心场景——`ApplyTrait` 与 `buffs-and-triggers.md` 已经设计好的 `TriggerResponse::ApplyBuff { def, stacks, duration_ticks }` 是同一个形状，只是触发点从"命中时"换成"技能主动使用时"**，`def` 现在有了具体类型（`TraitTable` 的 `ContentIndex`，二节已补）。

技能本身：`(register-skill "lostland:rage_activate" ... (resource-cost "stamina" 20) "apply-trait" "lostland:rage" 200 ...)`（示意，`register-skill` 真实签名的 `effect` 参数扩展需要配合 `SkillEffect::ApplyTrait` 落地，本文档不代为定案完整签名）。

**结论：结构上可表达，但需要 `SkillEffect` 新增一个变体（本节已给出建议形状），这是天赋系统对战斗/技能批次提出的一条新接线需求，不是本文档能自己落地的东西。** 资源消耗用现有 `Stamina`（七节已论证：不需要"每长休 N 次"这类分级池，只需要一种可以被 `RestoreResource` 或自然恢复补充的既有资源，D&D 原版狂暴次数与长休绑定，本设计退而求其次绑定一种通用消耗资源，非等价近似，如实标注）。

**示例五：盗贼偷袭（条件式规则修正）**

见三节③"条件式规则修正"一节的完整论证——**结论：条件本身（有优势，或目标附近有盟友）当前不可求值，只能退化成一个不带条件的主动技能**：

```scheme
(register-skill "lostland:sneak_attack" ... "deal-damage" 
  ;; base 里已经把 2d6 近似值算成固定数，或等 damage-formula-mod-api
  ;; 落地后写成 (d 2 6) 骰子表达式——两条路都不含任何条件判断
  7 ...)
```

**这不是"表达不了就放弃"，是明确标注"能表达的只是退化版本"**：真正的条件版本需要判定系统（提供"是否有优势"）与一次目标周边空间查询（提供"是否有盟友邻接"）两样都不存在的输入,天赋/规则修正机制本身已经准备好了接收这类条件（`ConditionKind` 占位,三节已给出),缺的是上游能不能算出这个条件,不是天赋系统这一层的责任。

### 法师天赋

**示例六：法师法术位（分级资源池）**

**结论：结构上现在可以声明，运行期仍完全不可用——`resource-pools-and-rest.md`（提交 `2e7dc02`）填补了这半个问题。**

```scheme
;; 示意：法师法术位归属声明，容量随等级阶梯式增长（1 环 4 个起，9 级追加五环位）
(register-trait "lostland:wizard_spell_slots"
  "lostland:trait.wizard_spell_slots.display_name"
  (list) (list) (list)
  ;; granted-resource-pools（三节④新增第四个参数,示意语法待 register-trait
  ;; 签名真正扩展时定案）：pool、capacity-formula（by-level 阶梯表）
  (list (grant-resource-pool "lostland:spell_slots" (by-level ...))))
```

`TraitDef.granted_resource_pools`（三节④）能表达"这个天赋归属哪个职业、按等级给多少容量"——但七节已经重申，这只解决了三个缺口里的零个：**分级资源池的形状本身**（`ResourcePoolDef`/`ResourcePoolShape`）、**休息事件**（长休/短休 `Intent`）、**"占用格子而非扣点数"的消耗语义**，全部仍在 `resource-pools-and-rest.md` 的范畴内，且该文档本身也是纯设计，无任何代码。**依赖链变成三层——资源池 → 天赋 → 等级（`resource-pools-and-rest.md` 三节末尾原话）**：本文档的 `granted_resource_pools` 归属 `agent.level`（六节已标注 `Agent.level` 不存在），最底层不落地，上面两层在实现意义上都动不了。**如实标注：这是三个职业示例里此前唯一连近似都给不出的，现在结构上补齐了，但仍是三层纯设计里最深的一层，离能跑起来最远。**

---

## 十、现在能做的 vs 等什么

**现在就能落地（本文档给出的形状，无阻塞前置依赖）：**

1. `TraitDef`/`TraitTable`（四节）——新类型，照抄 `RaceTable`/`ClassTable`/`SubclassTable` 的列式物化模式，无阻塞。
2. `register-trait` Steel API（四节）——新函数，同构于 `register-vehicle`/`register-skill`，无阻塞。
3. `RaceDef` 新增 `parent_race`/`traits` 两个字段（五节）——`RaceDef` 已落地，加字段是纯结构体扩展，无阻塞；但**若要真正生效，需要 P9 世界历史生成种族分布场那批工作一并核对**（`race-system.md` 已有的既有依赖，非本设计新增）。
4. `ClassDef`/`SubclassDef` 新增 `traits: Vec<TraitGrant>` 字段——同上，纯结构体扩展。
5. `RuleModifier::Resistance`/`RerollOnce` 两个变体的数据结构与注册期校验（三节）——可以现在声明，**但运行期没有任何消费者**，见下方"等什么"。
6. `TraitDef` 新增 `granted_resource_pools: Vec<ResourcePoolGrant>` 字段与 `CapacityFormula`/`CapacityValue` 数据结构（三节④，`resource-pools-and-rest.md` 三节要求的补丁）——纯结构体扩展，无阻塞；但**运行期没有任何消费者**，见下方"等什么"第 10 项（依赖链比其余五项更深，三层全是纯设计）。

**等什么（明确阻塞，本文档不解决）：**

1. **`AttributeKind` 没有抗性变体，抗性乘数机制本身不存在**——`damage-formula-mod-api.md` 二十/二十三节已经承认这一点，本文档的 `Resistance` 只是给这个尚不存在的机制补上"谁能声明它"这一环,`resistance_multiplier` 函数本身要等damage-formula-mod-api的抗性系统真正落地才有地方接。
2. **`resolve_attack` 不读 `active_stat_modifiers`**（damage-formula-mod-api.md 十六/二十三节已核实的既有缺口）——天赋②节的属性修正即使正确写入 `active_stat_modifiers`,伤害结算目前也读不到。
3. **`resolve_use_skill` 门一不读任何"授予技能"的并集**——载具的 `granted_skills` 与天赋的 `granted_skills` 是同一处待接线（三节①已指出）,现在只有设计,没有代码。
4. **骰子取数原语 `roll_one_die` 的重骰钩子（三节③）**——`damage-formula-mod-api.md` 的求值器现在不知道"是谁在掷这颗骰子",需要该文档在实现阶段补一个身份参数,本文档只指出接口缺口的位置。
5. **判定/检定系统不存在**——`RuleModifier::Advantage`/`Disadvantage`、半身人幸运针对 d20 的真正语义、盗贼偷袭"有优势"这半个条件，全部等待 `attribute-system.md` 八节的 d20 判定机制真正接入战斗（当前只是纯设计,从未接线）。
6. **`resolve` 没有战场空间上下文可查**——盗贼偷袭"目标附近有盟友"这半个条件,需要 `resolve_attack` 能额外发起一次以目标为中心的邻接查询,当前的输入只有攻防双方各自的属性,没有第三方实体的查询能力。
7. **`SkillEffect` 需要新增 `ApplyTrait` 变体**（示例四已给出建议形状）——当前 `SkillEffect` 三个变体都不能表达"授予一份天赋载荷、限时",这是本文档对战斗/技能批次提出的新接线需求。
8. **`Agent.active_traits: Vec<ActiveTraitInstance>` 字段本身不存在**——二节新定义的类型,连同它必须进 `hash()` 这条纪律,都要等实现批次真正加这个字段时才成立(现在只是设计)。
9. **`Agent.level`（或逐职业等级）不存在**——六节已详细标注为等待另一份并行设计的等级/经验系统文档,本文档的 `TraitGrant.unlock_level` 字段现在可以声明,但无法被消费。
10. **分级资源池 + 休息事件不存在**——七节,法师法术位此前完全无法表达的根因,需要独立的资源系统重新设计任务,不是天赋系统的份内工作。`resource-pools-and-rest.md`（提交 `2e7dc02`）已经给出该任务的设计,依赖链因此变成三层——资源池（该文档,`ResourcePoolDef`/`ResourcePoolTable`/`Agent.resource_pools`/`Agent.spent_slots`）→ 天赋（本文档三节④ `granted_resource_pools`,**现在能声明**）→ 等级（第 9 项,`Agent.level` 不存在）——三层全部纯设计,最底层不落地,上面两层在实现意义上都动不了。
11. **`buffs-and-triggers.md` 需要补一句"`ActiveEffect.def` 指向 `TraitTable`"**（二节）——本次任务写权限不包含该文件,标注为待下一次触碰该文档的批次顺手补上的小改动。
12. **P6 装备系统**——`TraitDef` 被装备引用（"这件武器天生带一个天赋"）这条所有权路径,要等 P6 `WeaponDef`/`ItemDef` 落地才有地方挂,与几乎每一份已冻结设计文档点名的既有缺口相同,非本设计新增。

---

## 相关文档

- [种族系统](race-system.md) 一至三节、九节 — `RaceDef` 已落地形状、属性修正烘焙时机、混血"不递归到祖父母"先例（五节亚种"只有一层"直接复用）
- [职业/技能树/副职/任务系统](class-skill-quest-system.md) — `ClassDef`/`SubclassDef` 已落地形状、`SkillDef`/`SkillEffect` 现有三个变体
- [技能可学条件设计](skill-learn-requirements.md) — `SkillRequirement` 声明式闸门的既有先例，六节 `TraitGrant.unlock_level` 的消费路径形状直接类比该文档"可学"闸门
- [增益与通用触发器](buffs-and-triggers.md) — `ActiveEffect`/`TriggerResponse::ApplyBuff` 的既有形状，二节补上其 `def` 字段此前缺失的具体类型
- [载具与骑乘系统](vehicle-and-mounting.md) 六节 — "有效技能=并集"、`stat-modifiers` 通用列表两条模式的原始出处，三节①②直接复用
- [资源池与休息系统](resource-pools-and-rest.md) 三、四节 — `TraitDef.granted_resource_pools` 第四类效果的原始需求方与精确补丁形状，三节④按其原文落地；分级资源池的形状、休息事件、消耗语义均在该文档，七节"不属于天赋系统"的边界依据
- [伤害公式的 mod API](damage-formula-mod-api.md) 六、十七、二十节 — 骰子取数求值器、开放伤害类别注册表、抗性挂载点，三节③直接对接
- [角色属性系统](attribute-system.md) §五、§八 — 幸运→优势骰/暴击率描述、d20 判定机制（尚未接入战斗，三节③"等什么"的直接依据）
- `crates/ll-mod/src/{race,class,subclass,skill}.rs`、`script_race_api.rs` — 一节现状核实的代码依据
- `crates/ll-world/src/entity/{stats,agent}.rs` — `AttributeKind`/`ActiveStatModifier`/`Agent` 字段现状
- `crates/ll-sim/src/skill.rs` — `ResourceKind`/`ResourceCost`/`SkillEffect` 现状，七节资源池缺口的直接依据
- `crates/ll-world/src/state.rs` — `WorldState::hash()` 覆盖范围核实
- [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md) / [0017](../decisions/0017-tiered-declarations-materialize-columnar.md) — 一档判据，四节/三节③档位核对
- [ADR 0018](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) — 三步判据
- [ADR 0021](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) — 二节"天赋与 buff 是不是同一个东西"、五节"亚种要不要照抄副职"两处核心判断的直接依据
- [ADR 0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 八节 `active_traits` 必须进 `hash()` 的纪律依据
- `mod-lifecycle-and-event-api.md` — 监听器不能改结果（C1），三节③"为什么不能是监听器"的依据
