# 制作类副职该给玩家什么：一个 `RuleModifier` 变体，四个副职共用

> **落地状态**：纯设计，零实现。本文档提出的 `RuleModifier::CraftYield` 变体、
> `craft_yield_bonus` 选择器、`resolve_craft` 第⑨步的接线，**一行都还没写**。
> 本文档不改 `crates/**`、不改 `mods/**`。
>
> **冻结日期**：2026-08-25
>
> **基线提交**：`7db7a63`（`Merge branch 'wt-gear'`）。下文所有「已核实」的
> 行号与形状都对着这个提交，读到本文档时若已漂移，以代码为准、以本文档的
> **判据**为参考。
>
> **并发声明**：写作期间工作区有**另一个代理正在改 `crates/**`** 的未提交改动。
> 本文档全程只读代码、不改代码，也不 `git add` 任何 `crates/` 下的文件。
> 一节的核实结果因此是「基线提交 `7db7a63` 的状态」，不是「工作区当前状态」——
> 若那位代理恰好动了 `rule_modifier.rs`/`resolve.rs`，落地时需要重新核对一遍。

---

## 零、要回答的问题

项目所有者收到的阻塞报告，原文记在 `mods/lostland/subclasses.json5` 文件头：

> **没有语义对的技能可给**：本体六条里四条是制作类（工匠/裁缝/炼金术士/厨师），
> 而 `SkillEffect` 当前只有 `deal-damage` / `temporary-stat-modifier` /
> `restore-resource` 三种形状，**没有任何一种能表达「会打铁」**。给工匠塞一个
> 伤害技能只是为了让字段覆盖检查变绿。

项目所有者的裁定：

> 「你先找几个人研究好，然后决定一份可行的方案。」

**本文档要回答的问题只有一个**：制作类副职（工匠/裁缝/炼金术士/厨师）
应该给玩家什么，以及那个东西怎么落地。

**本文档明确不回答**：其余两条本体副职（剑舞者/学徒）该给什么；副职奖励的
通用框架该长什么样；「给点数」那一半裁定（`subclass-system.md` 二节【第三次
订正】第 2 条）怎么落地；四个副职各自的具体数值。**YAGNI**：这四条副职今天
一条奖励都没有，本文档只把这一格填上，不顺手设计第二格。

---

## 一、现状核实：逐项 grep 确认，不采信任何转述

### ① `SkillEffect` —— 确切清单是三个变体

`crates/ll-sim/src/skill.rs:147`：

```rust
pub enum SkillEffect {
    DealDamage { base: i32 },
    RestoreResource { resource: ResourceKind, base: i32 },
    TemporaryStatModifier { attribute: AttributeKind, amount: i32, duration_ticks: u32 },
}
```

阻塞报告的描述**准确**。三个变体全部是「对某个目标当场做一件战斗/资源相关的事」，
没有一个能表达一条**持续生效、改变某类行动结算方式**的被动。

同一文件 `skill.rs:175` 的 `SkillRule` 还带 `cooldown_ticks`/`resource_cost`——
这两个字段本身就说明了 `SkillEffect` 的定位：它是**主动技能**的效果载荷，
消费者是 `resolve_use_skill`，入口是 `Intent::UseSkill`。

### ② `Effect` —— 三十六个变体

`crates/ll-sim/src/effect.rs:48`，按声明顺序：

`MoveTo`、`Damage`、`Kill`、`RecordHistoricalEvent`、`IncrementKillCount`、
`ScheduleNext`、`SetTerrain`、`AdjustWallet`、`ChangeSpace`、`SetModState`、
`AdjustResource`、`SetSkillCooldown`、`ApplyStatModifier`、`MarkExplored`、
`GrantExperience`、`AdjustResourcePool`、`SpendBloodCost`、`AdjustResourceSlot`、
`BeginRest`、`ClearResting`、`RemoveGroundItem`、`AddGroundItem`、
`MergeIntoInventory`、`RemoveFromInventory`、`Equip`、`Unequip`、
`ConsumeInventoryItem`、`AdjustEquipmentDurability`、`Inspect`、`SetStealth`、
`AllocateAttributePoint`、`LearnSkill`、`GrantSubclass`、`RemoveSubclass`、
`LearnRecipe`、`IdentifyItem`。

**本文档最重要的一条核实结果就在这里**：`MergeIntoInventory` 携带一整个
`ItemStack`（含 `count`），`ConsumeInventoryItem` 恒扣一件、要扣 N 件就产出
N 条。也就是说——**「产出更多」与「消耗更少」这两类奖励，都不需要新增任何
`Effect` 变体**（ADR 0023 因此自动满足：写入仍然只经 `apply` 这一个入口）。

### ③ `RuleModifier` —— 七个变体，其中三个零消费者

`crates/ll-sim/src/rule_modifier.rs:272`：

| 变体 | 字段 | 消费者 |
|---|---|---|
| `Resistance` | `damage_category: ContentIndex`、`damage_reduction: i32` | `resolve_attack`（经 `resistance_damage_reduction`，`rule_modifier.rs:620`） |
| `RerollOnce` | `value: i32` | **零** |
| `Advantage` | `check_context: NamespacedId` | **零** |
| `Disadvantage` | `check_context: NamespacedId` | **零** |
| `SneakAttack` | `luck_chance_permille_per_point: i32`、`extra_damage: i32` | `resolve_attack`（经 `sneak_attack_rule`） |
| `InspectionSuspicion` | `suspicion_reduction_permille: i32` | `ll_mod::native_behavior::guard_inspect_chance` |
| `InspectionConcealment` | `conceal_permille: i32` | `resolve_inspect` |

**那三个零消费者的变体当初是为什么加的**（这是任务书点名要查的一条，
答案在 `trait-system.md` 三节③与 `rule_modifier.rs:253-257`，两处互相印证）：

- **`RerollOnce`**：为**半身人幸运**（D&D「重掷 1」）加的。它刻意**不**做成
  伤害公式的一个算子（那会要求每个武器作者替所有可能拿到这把武器的种族预先
  写好重骰包装，本末倒置），而是下沉到「骰子取数原语」这一层——需要给
  `damage-formula-mod-api.md` 六节的求值器补一个 `roll_one_die` 钩子
  （多带一个「这次掷骰的主体是谁」的上下文）。**那个钩子至今没写**，
  所以变体是死的。它**不是**投机性预留：它有明确的目标场景、明确的接线点、
  明确的阻塞原因。
- **`Advantage` / `Disadvantage`**：为「对某类豁免有优势」这类种族天赋加的，
  依赖 **d20 判定/检定系统**。`attribute-system.md` §八描述过这套机制，但
  **从未接入战斗**——`resolve_attack` 恒命中，没有命中判定，也就没有「优势」
  这个中间态可查（`trait-system.md` 一节已核实）。两个变体因此被显式标注为
  **占位变体**。

三个变体在 `strength_key`（`rule_modifier.rs:763`）与 `cross_type_merge`
（`rule_modifier.rs:823`）里都取「不裁定」档（`StrengthKey::INDISTINGUISHABLE` /
`CrossTypeMerge::Undecided`），并显式登记在 `scripts/ci/check_field_consumers.py`
的 `EXEMPTIONS` 里——**仓库对它们是死变体这件事是知情的、有记录的**。

### ④ 副职授予天赋这条通道已经通了

`a6adab5` 落地：`SubclassDef.traits: Vec<TraitGrant>`（`crates/ll-mod/src/subclass.rs:105`），
`SubclassTable` 实现 `TraitGrantSource`，`agent_trait_sources`
（`crates/ll-sim/src/traits.rs:253`）返回类型从 `[TraitSource; 2]` 变成 `Vec`，
持有 N 个副职就展开 N 个来源。**聚合算法一行都没有为副职分流**（ADR 0021 复核
结论记在该函数文档里）。

`TraitDef`（`crates/ll-mod/src/trait_def.rs:82`）有且只有四类效果载荷：

1. `granted_skills: Vec<ContentIndex>` —— 授予主动技能（受 `SkillEffect` 三个形状所限）
2. `stat_modifiers: Vec<(AttributeKind, i32)>` —— 主属性修正（一个数字）
3. `rule_modifiers: Vec<TypedRuleModifier>` —— **改变规则本身**
4. `granted_resource_pools: Vec<ResourcePoolGrant>` —— 资源池

真实用例已存在：`mods/example_mod/traits.json5` 的「影舞」正是一条挂在
`SubclassDef.traits` 上的天赋（端到端测试 `crates/ll-mod/tests/example_mod_subclass_traits.rs`）。

### ⑤ 制作管线现状

- `resolve_craft`（`crates/ll-sim/src/resolve.rs:2907`）：十步——①行动者 ②配方
  ③副职闸门（any-of）④已知闸门 ⑤场地 ⑥工具 ⑦食材校验（`has_all_ingredients`，
  `resolve.rs:3526`）⑧逐条产出 `ConsumeInventoryItem` ⑨
  `ItemStack::freshly_made(product, product_count, max_durability)` 并进背包
  ⑩工具磨损（`TOOL_DURABILITY_LOSS_PER_CRAFT = 1`，`resolve.rs:160`）。
  **全程不掷一次骰**：`resolve_craft` 里没有任何 `DetRng` 调用。
- 现签名：`(world, actor, recipe, recipes: &dyn RecipeCatalog, items: &dyn ItemCatalog)`。
- `craft_progress_effects` / `craft_count_key`（`crates/ll-sim/src/subclass.rs:179`）：
  按类别累计制作次数进 `Agent::mod_state`，达阈值发 `Effect::GrantSubclass`。
  **「成长挂钩玩家本来就在做的动作」这条原则今天已经是真代码。**
- `agent_rule_modifiers`（`rule_modifier.rs:580`）：把天赋三路（种族/职业/副职）
  与**装备**（`equipment_rule_modifiers`）汇成一份 `Vec<RuleModifierEntry>`。
- `merged_across_types`（`rule_modifier.rs:901`）：按 `modifier_type` 分桶，
  **同类型取最强**（平局按 `origin` 升序），**跨类型相加**。全程 `BTreeMap` + `Vec`，
  不碰任何哈希表遍历顺序（约束 C5）。

---

## 二、问题重述：缺的不是「制作技能」，是「制作被动」

阻塞报告把问题定位在 `SkillEffect` 上。**这个定位是错的，而且正是它让这条阻塞
看起来比实际更贵。**

「会打铁」不是一个玩家按下去会发生什么的**动作**——玩家已经有制作这个动作了
（`Intent::Craft`）。「会打铁」是「**当我制作时，结算方式不一样**」。
这在本仓库的既有词汇里有一个精确的名字，而且它已经存在：

> `RuleModifier` —— 天赋效果③「**改变规则本身**」。

`Resistance` 的语义是「当我挨打时，减伤链路多减几点」；本文档要的语义是
「当我制作时，产出数量多几件」。**同一种句式，同一条通道，同一套聚合算法。**

**把「会打铁」做成 `SkillEffect` 会具体错在哪**（这不是风格问题）：

- `SkillEffect` 的消费者是 `resolve_use_skill`，入口是 `Intent::UseSkill`——
  玩家得**主动施放**「打铁精通」，然后才去制作。这不是被动。
- `SkillRule` 强制带 `cooldown_ticks` 与 `resource_cost`。一条「我会打铁」
  要冷却时间和法力消耗，是把一个身份属性硬塞进一个技能框子。
- 真要做成「施放后 N tick 内制作有加成」，那就是 `TemporaryStatModifier` 的
  形状——而它作用在 `AttributeKind`（六维主属性）上，制作产出不是主属性。

**结论：`SkillEffect` 不需要新增任何变体。** 这是本文档最省事的一条发现——
阻塞报告问的那个问题（「`SkillEffect` 该加什么」）不需要答案，因为它问错了地方。

---

## 三、方案：一个 `RuleModifier` 新变体，四个副职共用

**一句话**：给 `RuleModifier` 增开第八个变体
`CraftYield { category, bonus_product_count }`——「在某一类配方上，每次制作
多产出若干件」；四条制作副职各自授予一条只带这一条修正、指向自己那个配方类别的
天赋。**不新增任何 `Effect`、不新增任何注册表、不新增任何逐实体状态、
不新增任何随机数调用。**

### 3.1 类型

```rust
// crates/ll-sim/src/rule_modifier.rs，RuleModifier 的第八个变体
/// 制作产出加成：在 `category` 这一类配方上，每次成功制作的产出数量
/// 额外增加 `bonus_product_count` 件。
CraftYield {
    /// 配方类别，指向 `RecipeCategoryTable`——与 `Resistance.damage_category`
    /// 指向伤害类别表是同一种「开放集合的一个成员」，不是新概念。
    category: ContentIndex,
    /// 额外产出件数。可为负（见 3.4）。
    bonus_product_count: i32,
},
```

**为什么必须按类别键控**：一个铁匠不该因为会打铁就烧得一手好菜。
`Resistance` 按 `damage_category` 键控是同一个理由的既有先例。

### 3.2 选择器（与 `resistance_damage_reduction` 逐字同构）

```rust
// crates/ll-sim/src/rule_modifier.rs
pub fn craft_yield_bonus(
    modifiers: &[RuleModifierEntry],
    category: ContentIndex,
) -> i32 {
    merged_across_types(modifiers, |modifier| match modifier {
        RuleModifier::CraftYield { category: candidate, bonus_product_count }
            if *candidate == category => Some(*bonus_product_count),
        _ => None,
    })
    .unwrap_or(0)
}
```

**这八行是本方案新增的全部算法。** `merged_across_types`、
`AddAcrossTypes for i32`、分桶、平局规则、跨类型求和——一行都不用改，
它们已经在为 `Resistance` 服务。

### 3.3 消费点（`resolve_craft` 第⑨步）

```rust
// 第⑨步，把写死的 rule.product_count 换成算出来的件数
let bonus = craft_yield_bonus(&modifiers, rule.category);
let count = i32::try_from(rule.product_count)
    .unwrap_or(i32::MAX)
    .saturating_add(bonus)
    .max(MINIMUM_CRAFT_PRODUCT_COUNT);      // = 1
```

`modifiers` 由 `agent_rule_modifiers(agent, race_grants, class_grants,
subclass_grants, traits, items)` 现算。**其余九步一个字都不改。**

### 3.4 保底常量：`MINIMUM_CRAFT_PRODUCT_COUNT: i32 = 1`

照抄 `MINIMUM_DAMAGE_AFTER_RESISTANCE` 的形状与理由。它做两件事：

1. **兜住负值**。`bonus_product_count` 允许为负（「手艺生疏」「诅咒的铁砧」），
   与 `Resistance.damage_reduction` 允许负值表示「脆弱」是同一条先例。
2. **顺带守住一条既有裁定**：产出恒 ≥ 1 意味着「消耗了材料却什么都没拿到」
   在机制层面**不可能**发生——而这正是 `crafting-system.md` 九节⑤在玩法上
   否决过的「制作失败」。保底不是防御性编程，它是那条裁定的机制化。

### 3.5 逐变体声明（两处穷尽 `match`，不补就编译不过）

- `strength_key`：`larger_is_stronger(*bonus_product_count)`——多产出的更强。
- `cross_type_merge`：`Add`——附魔 +1、天赋 +1，合起来 +2。全程整数加法，
  没有整数除法、没有截断、没有顺序依赖（约束 C5）。

**注意**：这两处必须写出 `RuleModifier::CraftYield` 字面量（不能用 `R` 别名），
因为本变体**确实有消费者**——`check_field_consumers.py` 该判它绿。那三个死变体
用别名 `R` 的纪律与本变体无关，别照抄。

### 3.6 四条本体副职的内容形状（示意，数值非最终）

```json5
// mods/lostland/traits.json5（本文件今天不存在，本方案要求开这个头）
{
  traits: [
    {
      id: "lostland:forging_mastery",
      display_name_key: "lostland:trait.forging_mastery.display_name",
      rule_modifiers: [
        { kind: "craft-yield", category: "lostland:forging", bonus_product_count: 1 },
      ],
    },
    // tailoring_mastery / alchemy_mastery / cooking_mastery 三条同形
  ],
}
```

```json5
// mods/lostland/subclasses.json5，工匠那一条加一行
{
  id: "lostland:artisan",
  display_name_key: "lostland:subclass.artisan.display_name",
  unlock: { kind: "items-crafted", target: "lostland:forging", threshold: 20 },
  traits: [{ trait_id: "lostland:forging_mastery", unlock_level: 1 }],
},
```

闭环因此成立：**做够 20 件锻造品 → 得到工匠 → 此后每件锻造品多出 1 件。**
挂钩的动作、奖励的动作，是同一个动作。

---

## 四、四个视角的自检

这是本任务的核心产出。每个视角的反对意见**照原样写出来，包括我最后没有采纳的**。

### 4.1 玩法设计视角

**反对 G1（最强的一条，我只部分反驳得了）**：
「打一次铁出两把剑」在**题材上**是别扭的。厨师多出一份汤、炼金多出一瓶药，
读起来天经地义；铁匠一锤子下去蹦出两把一模一样的剑，读起来像 bug。
四个副职里有两个（工匠、裁缝）的产出是不可堆叠语义的装备，这条奖励对它们
**味道不对**。

**回应**：这条我**接受一半，不接受另一半**。

- 不接受的那一半：它与被否决的「给工匠塞个伤害技能」**不是同一类错误**。
  那条错在**跨轴**——用战斗力奖励制作行为，奖励和挂钩动作分属两个轴，
  循环是断的。本条错在**味道**，轴是对的：做东西 → 更会做东西。
  在同一个轴上味道不够好，和在错误的轴上，是两个量级的问题。
- 接受的那一半：味道成本是真的。缓解有两条，都在**内容侧**、不改机制：
  ①「多出来的一件」在物品经济里不是废物——`agent-goals-and-economy.md` 的
  行会中介贸易让它是可卖的，工匠因此拿到一条比裁缝更强的换钱通道，而这
  正是 `crafting-system.md` 四节希望工匠拥有的重复需求循环的另一半；
  ②内容作者可以给工匠的锻造类配方设计成「一炉出两把匕首」这类本来就多产出
  的形状（`product_count > 1` 今天就合法），此时 +1 读起来完全自然。
- **我没有采纳「那就给工匠换一条别的奖励」**：四个副职各配一条不同的奖励
  意味着四个新变体，那是 YAGNI 明令禁止的「顺手设计整套框架」。先同构，
  等真实的差异化需求出现再拆——那时候的理由是「发现了不同」，不是「现在」。

**反对 G2**：这个奖励让玩家**想去**当工匠吗？还是只是一个数字？

**回应**：它是数字，但不是「+1 力量」那种数字。
一条 `product_count = 1` 的配方拿到 +1 就是**产能翻倍**——同样的材料、
同样的时间，出货量 ×2。这不是面板上多一行字，是玩家在背包里、在钱包里
**立刻能数出来**的差别。所有者对旧方案的定价原话是「一个纯资格的副职在玩家
那一侧是没有获得感的」（`subclass-system.md` 二节【第三次订正】），
产能翻倍不属于「没有获得感」那一档。

**反对 G3**：与「副职成长挂钩玩家本来就在做的动作」这条既有原则一致吗？

**回应**：**一致，而且是这条原则目前最完整的一次闭合**。
今天的闭环只有前半段：制作 N 次 → 获得副职 → 然后什么都没有。本方案补的正是
后半段：获得副职 → 制作变强 → 更愿意继续制作。挂钩动作与奖励作用点是**同一个
动作**，这一点上它比「授予一个主动技能」严格更好——技能是玩家要额外学会去按的
新按钮，被动是玩家原本的动作直接变好。

**反对 G4**：每次制作 +1 会不会变成印钞机？便宜配方刷一万次。

**回应**：这条我**接受为一条内容纪律，不接受为一条机制缺陷**。
风险的形状是「产物售价 > 材料成本」，而这个风险**今天就已经存在**——
`product_count` 今天就能填 5。+1 只是把已有的调参空间平移一格，没有引入新的
失控通道：材料照常消耗、行动照常耗时、工具照常磨损、上限恒为「每次制作一次
加成」。真正该拦它的地方是配方定价与行会售价（`agent-goals-and-economy.md`），
不是这条修正。

**反对 G5**：四个副职拿到一模一样的奖励，玩家选哪个都一样，选择没有意义。

**回应**：**它们本来就不该靠奖励区分。** 四条副职的差异全部在
**能做什么**（`required_subclasses` 闸门 + 各自的配方类别）——工匠做武器护甲、
厨师做食物。奖励同构不会让「当工匠」和「当厨师」变成同一件事，因为你能做的
东西完全不同。把差异塞进奖励，反而会掩盖一件更重要的事：**副职的价值主要来自
它开的门，不是它给的数**。

### 4.2 引擎架构视角

**反对 A1**：为什么不老老实实给 `SkillEffect` 加变体？阻塞报告说的就是它。

**回应**：见二节。一句话——被动不是主动技能，`SkillEffect` 的消费者是
`Intent::UseSkill`，硬塞进去要连带解释「打铁精通的冷却时间是多少」。
阻塞报告问错了地方，这是本文档的核心发现。

**反对 A2（ADR 0021 正面复核）**：新增的东西有没有第二个消费者？还是为一个用例
发明的抽象？

**回应**：**这里没有发明任何抽象，因此 ADR 0021 的判据不适用于「要不要加」，
只适用于「要不要为它抽新东西」——答案是不抽。** 逐项核对：

| 本方案要新增的 | 是不是新抽象 | 复用的既有算法 / 既有用户数 |
|---|---|---|
| `RuleModifier` 第 8 个变体 | 否，是既有封闭枚举的一个分支 | `TypedRuleModifier` 载体、`RuleModifierEntry`、`agent_rule_modifiers`（天赋 + 装备两路）—— 7 个既有变体在用 |
| `craft_yield_bonus`（8 行） | 否，是一个 `select` 闭包 | `merged_across_types` + `AddAcrossTypes for i32` + 分桶 + 平局规则 —— 4 个已接线变体在用 |
| `strength_key` / `cross_type_merge` 各一条分支 | 否 | 穷尽 `match`，不补就编译不过 |
| `MINIMUM_CRAFT_PRODUCT_COUNT` 常量 | 否 | `MINIMUM_DAMAGE_AFTER_RESISTANCE` 的同款 |

**没有新 trait、没有新注册表、没有新 `Effect`、没有新表结构。**
ADR 0021 要拦的是「为了对称而抽出一层没有内容的接口」——本方案一层都没抽。

**反对 A3**：那第二个消费者到底有没有？一个变体只服务制作，就是一个用例。

**回应**：**有两个来源、四个内容消费者，且都是白拿的**：

- **来源侧**：`agent_rule_modifiers` 同时汇聚天赋路与**装备路**
  （`equipment_rule_modifiers`）。也就是说「大师级铁砧锤」这件装备可以携带
  同一条修正，一行代码都不用加。这不是设想——`Resistance` 已经同时走这两路
  （`mods/example_mod/traits.json5` 的酸蚀护甲 + 护符）。
- **内容侧**：四条制作副职各用一次，各自指向不同的 `category`。

**我不接受「只服务一个用例所以别加」这条推论**，理由是它算错了代价基线：
不加的代价不是零，是「四条副职永远空着」——而那正是本任务被派下来要解决的
那件事。`InspectionConcealment` 也只有一个消费者（`resolve_inspect`），
它是既有先例，不是反例。

**反对 A4（我接受的一条真实成本）**：`resolve_craft` 的签名要从 5 个参数长到
9 个（多 `race_grants`/`class_grants`/`subclass_grants`/`traits`）。

**回应**：成本是真的，但它是**转发成本不是新增成本**——这四个 `&dyn` 今天
**已经在 `resolve_dispatch` 的参数表里**（`resolve.rs:550` 起那一串
`resolve_with_*` 便利函数全在传它们），`resolve_attack` 也已经接受同一组。
本方案只是把它们再往下传一层，不引入任何新的目录、新的构造、新的生命周期。
如实标注：这是本方案唯一一处「代码看起来更丑」的地方。

**反对 A5**：为什么不做成全局的「制作精通」，省掉 `category` 字段？

**回应**：否决。会打铁不等于会做饭。而且没有 `category` 就没法让四条副职
各拿各的——一条全局修正会让任意一个制作副职给全部四类制作提速，闸门设计
（`required_subclasses` 按类别）与它直接打架。

**反对 A6**：这会不会又变成一个「声明了但从没接线」的第 31 处？

**回应**：判据是「落地时有没有真实消费者一起进去」。本方案的落地清单
（五节）把 `resolve_craft` 的消费点与**本体四条内容**放在同一个批次里——
变体、选择器、消费点、内容、i18n 一次做完，`check_field_consumers.py`
当场判绿，不进 `EXEMPTIONS`。**若这个批次做不完整（只加变体不接消费点），
那就不该开始做**——这条写在这里当作落地纪律。

### 4.3 确定性与存档视角

**反对 D1**：它会不会引入新的逐实体状态？

**回应**：**不会，一个字段都不加。** `RuleModifier` 住在 `TraitDef`/`ItemDef`
里，而**内容表是装载期产物、不进存档**（`crafting-system.md` 十一节 D 组已
确立）。每次制作现算一次 `agent_rule_modifiers`，算完即弃。
`Agent` 不加字段，`WorldState` 不加字段。

**反对 D2**：进不进 `WorldState::hash()`？

**回应**：**不进，因为没有东西可进。** `hash()` 覆盖的是世界状态；本方案不产生
任何世界状态。**黄金基准不需要重冻**（前提是本体内容不改；改了 `mods/lostland`
则按既有的内容变更流程走，与本机制无关）。

**反对 D3**：存档 remap 怎么办？

**回应**：**没有 remap**。remap 处理的是「存档里记着的索引在新内容集合里换了位置
或消失了」。本方案不往存档里写任何索引。既有的 `remap_subclasses`
（副职索引）与 `known_recipes` 那两路一个字都不用改。

**反对 D4（我接受为一条必须做的事，不接受为一条反对）**：ADR 0022/0027 呢？
内容值哈希漏一个变体，门禁就形同虚设。

**回应**：接受，落地清单里有这一条。`crates/ll-mod/src/content_hash.rs:1325`
附近那个逐变体 `match` 必须补一条 `CraftYield` 分支（摘 `category` 的完整
`NamespacedId` 字符串 + `bonus_product_count`）。ADR 0027「内容哈希覆盖字段值」
的直接义务。**这是本方案唯一一处「漏了会静默出错」的接线点**，因此在五节里
单独标星。

**反对 D5**：老存档读进来，同一串 `Intent` 重放会不会分叉？

**回应**：会——但这是**任何内容变更都有的性质**，且已经被既有机制兜住：
存档记录了生成期与当前两组 mod 集合与内容哈希（`identity-and-ids.md`），
内容一变，哈希就变，读档时按既有策略报错或提示。**本机制不需要为此新增任何
东西。** 反过来说一句更重要的：本方案**不引入任何 `DetRng` 调用**——
`resolve_craft` 今天一次骰都不掷，之后仍然一次都不掷。随机流的取数顺序
（约束 C3/C5 最难守的那一处）**完全不受影响**，重放测试面一点没变宽。
这是本方案相对「制作掷骰」类方案的一条硬优势。

**反对 D6**：聚合顺序确定吗？

**回应**：确定，且**是继承来的、不是新论证的**。`merged_across_types`
用 `BTreeMap<Option<ContentIndex>, _>` 分桶、同桶平局按 `origin` 升序、
跨桶按 `BTreeMap` 有序遍历求和；`agent_trait_sources`/`trait_rule_modifiers`
全程 `Vec`，不碰哈希表。这些结论 `Resistance` 已经在用，本变体走同一条路径，
不新增任何一处需要重新论证顺序的地方。

**反对 D7（ADR 0020）**：乙区整数？

**回应**：`bonus_product_count: i32`，`product_count: u32`，
`MINIMUM_CRAFT_PRODUCT_COUNT: i32`——**全整数，无浮点，无千分比，无除法**。
唯一要小心的是 `u32 → i32` 的转换与加法溢出，形状写在 3.3
（`try_from(...).unwrap_or(i32::MAX).saturating_add(...).max(1)`）。

### 4.4 内容作者视角

**反对 C1**：会不会又变成一堆位置参数？

**回应**：不会。天赋的规则修正在 JSON5 里是**带 `kind` 标签的具名字段对象**
（`mods/example_mod/traits.json5` 实证）：

```json5
{ kind: "craft-yield", category: "lostland:forging", bonus_product_count: 1 }
```

三个具名字段、零位置参数，与既有的 `resistance` / `sneak-attack` /
`inspection-suspicion` 完全同形。mod 作者要学的新概念**只有一个** `kind` 取值。

**反对 C2**：作者得先知道配方类别的 id，这是一层新耦合。

**回应**：**不是新耦合**。同一个作者在 `subclasses.json5` 里写 `unlock.target`
时**已经**要写这个 id（`{ kind: "items-crafted", target: "lostland:forging" }`），
而且那里是「只 get 不 intern，拼错当场报错」。同一份知识、同一套报错，
不是第二样要学的东西。

**反对 C3（我接受的一条真实工作量）**：`lostland` 命名空间下今天**零条天赋内容**
（本目录没有 `traits.json5`，`content_audit.rs` 的 Trait deferred 条目记的就是
这件事）。本方案要求为它开这个头。

**回应**：接受。工作量是一个新文件 + 四条天赋 + 四条 `traits` 引用 +
四个 i18n 键，量级约四十行内容数据。这不是本方案的额外负担——**这正是阻塞
本身**：所有者要的就是「四条制作副职终于给得出东西了」。

**反对 C4**：我想要的是「材料更省」，这套东西给不了我。

**回应**：**说得对，给不了。** 这是刻意的范围裁剪（六节④有完整否决理由），
不是疏漏。若将来真有 mod 作者提出这个需求，正确的应对是那时候加第二条分支
（`CraftIngredientDiscount`），复用同一个 `merged_across_types`——加宽成本
与今天预先加它几乎相同，而今天加它就是在为一个不存在的需求付复杂度。
**记为需所有者裁决的第 4 条**。

**反对 C5（我接受，且这是一个真实缺口）**：这是一条**看不见的**被动。
角色面板不显示规则修正，玩家怎么知道自己每次多产出一件？

**回应**：接受，且如实上报——**这个缺口不是本方案造成的**：
`Resistance`/`SneakAttack`/`InspectionConcealment` 三条已接线的修正
**今天同样不显示**。本方案不修这个缺口（那是 UI 批次），但它让缺口**更值得修**：
战斗类被动至少能从伤害数字上感觉出来，制作类被动如果不显示，玩家可能压根
没注意到自己变强了。**记为需所有者裁决的第 5 条。**

---

## 五、落地需要新增什么（完整清单）

| # | 位置 | 改动 | 备注 |
|---|---|---|---|
| 1 | `crates/ll-sim/src/rule_modifier.rs` | `RuleModifier` 增 `CraftYield` 变体 | 封闭枚举，加了就得补 2/3 |
| 2 | 同上 | `strength_key` 增一条：`larger_is_stronger(bonus_product_count)` | 穷尽 `match`，不补编译不过。**写全名，不用 `R` 别名** |
| 3 | 同上 | `cross_type_merge` 增一条：`Add` | 同上 |
| 4 | 同上 | 新增 `craft_yield_bonus`（8 行）+ `MINIMUM_CRAFT_PRODUCT_COUNT` | 与 `resistance_damage_reduction` 同构 |
| 5 | `crates/ll-sim/src/resolve.rs` | `resolve_craft` 加 4 个 `&dyn` 参数并从 `resolve_dispatch` 转发 | 目录在调用点已具备，纯转发 |
| 6 | 同上 | 第⑨步件数改为算出来的 `count` | 其余九步不改 |
| 7 | **★** `crates/ll-mod/src/content_hash.rs` | 逐变体 `match` 补 `CraftYield` 分支 | **ADR 0027 义务，漏了门禁形同虚设** |
| 8 | `crates/ll-mod/src/content_schema.rs` | `rule_modifiers` 的 `kind` 增 `"craft-yield"` 解析分支 | 三个具名字段 |
| 9 | `mods/lostland/traits.json5`（**新文件**） | 四条制作精通天赋 | 内容 |
| 10 | `mods/lostland/subclasses.json5` | 四条制作副职各加一行 `traits` | 内容；文件头那段「没有语义对的技能可给」的说明随之改写 |
| 11 | i18n | 四个 `display_name_key` | `check_i18n_strings.py` |
| 12 | 测试 | 单元（选择器分桶/相加/保底）+ 端到端（持工匠副职制作产出 +1） | 照抄 `example_mod_subclass_traits.rs` 的形状 |

**不需要改的**（逐项点名，免得落地时有人多做）：`Effect` 枚举、
`WorldState`/`Agent` 字段、`WorldState::hash()`、黄金基准、存档 remap、
`DetRng` 取数顺序、`SkillEffect`、`RecipeDef`/`RecipeCategoryDef`、
`agent_trait_sources`、`merged_across_types`。

**第二个消费者**：来源侧两个（天赋路 + 装备路，后者白拿）；
内容侧四个（四条副职）；算法侧共用 `merged_across_types`（已有 4 个用户）。

---

## 六、考虑过但否决的方案

| # | 方案 | 否决理由 |
|---|---|---|
| ① | **给 `SkillEffect` 增开制作向变体** | 方向反了：被动不是主动技能。`SkillEffect` 的消费者是 `Intent::UseSkill`，还强制携带冷却与资源消耗。见二节 |
| ② | **产出品质分档** | `ItemStack` 没有品质字段。加逐实例品质要动 `can_merge` 判据、`ItemStack` 存档形状、`WorldState::hash()`、全部 `StatBonus` 派生路径——`crafting-system.md` 九节⑥已判为跨 `ll-world`/`ll-sim`/`ll-content` 的独立批次。**它可能是更好的最终答案，但它不是一个小方案** |
| ③ | **产出随「锻造技艺」数值浮动** | 技艺数值在当前设计里**没有合法存放位置**（九节⑦）。要么推翻 `subclass-system.md` 的红线，要么给 `Agent` 加字段——后者改 `WorldState::hash()`、牵动黄金基准重冻与存档 remap，与「给点数」那一半裁定卡在同一个问题上（防「放弃再获得」的点数复制机需要新的持久化状态） |
| ④ | **消耗更少（材料折扣）** | **味道比①好，但机制上打空**：食材数量恒 ≥ 1，折扣必须钳在 1，于是**所有单件配方（大多数）拿到零收益**——一条「大部分时候什么都不发生」的奖励比味道别扭更糟。且它要同时改第⑦步校验与第⑧步扣减两处，接线面比产出加成大。**作为备选记录，见需裁决第 4 条** |
| ⑤ | **工具磨损更慢** | 只对声明了 `required_tool`、且工具带耐久与 `on-use` 标签的配方生效——四类制作里覆盖不全（烹饪/炼金未必用工具），一条时灵时不灵的奖励。且它是「少一点损失」而非「多一点收获」，感知弱 |
| ⑥ | **维持现状：只靠 `required_subclasses` 开专属配方** | 这**就是**今天的样子，而所有者已经把它的代价定价为「不可接受」：「一个纯资格的副职在玩家那一侧是没有获得感的」（`subclass-system.md` 二节【第三次订正】） |
| ⑦ | **复用零消费者的 `RerollOnce` / `Advantage`** | **认真评估过，这是任务书点名要查的一条。** 要用它们，制作必须先有一次掷骰：`Advantage` 需要一套 d20 判定系统（`attribute-system.md` §八，从未接入战斗），`RerollOnce` 需要伤害公式求值器的 `roll_one_die` 钩子（没写）。**为了复用一个死变体去造两套更大的机制，是把成本算反了。** 更要命的是玩法方向：给制作加掷骰就是加成败判定，而 `crafting-system.md` 九节⑤**已经在玩法上否决过制作失败**（「一次吃掉材料、什么都不给、玩家无法通过任何决策规避的失败，是纯粹的挫败感」）。就算只做「大成功」不做失败，也要往 `resolve_craft` 里塞进第一次 `DetRng` 调用，把重放测试面无端拓宽。**结论：它们不适合这里，且不适合的理由与它们当初被加进来的理由完全一致——它们等的是判定系统，不是制作系统** |
| ⑧ | **给制作副职配一条 `stat_modifiers`（例如 +1 力量）** | 这正是所有者说的「还是只是一个数字」。而且跨轴：用主属性奖励制作行为，和用伤害技能奖励制作行为是同一类错误的轻量版 |
| ⑨ | **新开一张「制作精通」注册表 / 一套技艺数值** | 为一个用例发明一整张表，ADR 0021 点名要避免的那类抽象；且落进 `Agent` 就是③ |
| ⑩ | **四条副职各配一条不同的奖励变体** | 四个新变体服务四个用例，YAGNI 明令禁止的「顺手设计整套副职奖励框架」。先同构；差异化的理由要等真实需求出现 |

---

## 七、需要项目所有者裁决的事

1. **这份方案本身要不要走**。若走，落地纪律见四节 A6：变体 + 选择器 + 消费点 +
   四条内容 + i18n **必须同一个批次做完**，否则就是仓库里的第 31 处「声明了但
   从没接线」。宁可不开工。
2. **`bonus_product_count` 允不允许为负**。我按 `Resistance` 允许「脆弱」的
   先例设计成允许（保底 1 件），但这是一条我替所有者做的推断，不是他说过的话。
   若判定不允许，把字段收成 `u32` 即可，保底常量仍然要留（跨类型相加不会
   变负，但它同时在守「制作不失败」那条裁定）。
3. **四条副职的具体数值**（示意用的 `+1`）。这是纯内容参数，改它一行 Rust
   都不用动。
4. **要不要同时做「材料折扣」**（六节④）。**我的建议是不要**：它对多数
   单件配方打空，而且今天加它与将来加它成本几乎相同。
5. **规则修正的可见性缺口**：角色面板今天不显示任何 `RuleModifier`，
   `Resistance`/`SneakAttack`/`InspectionConcealment` 三条已接线的都不显示。
   制作类被动受这个缺口的伤害比战斗类更重（战斗至少能从伤害数字看出来）。
   **这不属于本方案范围**，但所有者应当知道它的存在与优先级。
6. **`RerollOnce`/`Advantage`/`Disadvantage` 仍然是死变体**。本文档核实了它们
   当初为什么被加（半身人幸运的骰子钩子 / d20 判定系统的占位），
   也确认了**制作系统不是它们的归宿**。它们仍然等着两个未落地的系统。
   若所有者希望「零死变体」，那要么砍掉它们（会丢掉两条有记录的设计意图），
   要么排期做判定系统——**两条都不该由制作副职这个批次顺手决定**。

---

## 相关文档

- [制作系统](crafting-system.md) —— 九节⑤⑥⑦（失败/品质/技艺浮动三条推迟理由，
  本文档的否决②③⑦全部建在它们之上）、四节（工匠/裁缝的价值轴与耐久扩面订正）
- [副职系统](subclass-system.md) —— 二节【第三次订正】（「副职不给数值」被推翻的
  原话与两半裁定）、三节（`SubclassDef` 最小形状）、一之四节（制作类副职的地基）
- [天赋/特性系统](trait-system.md) —— 三节③（`RuleModifier` 的定形；
  重骰/优势两条为什么落不了地的原始论证）
- [职业 / 技能树 / 副职 / 任务系统](class-skill-quest-system.md) —— `SkillDef` 与
  技能效果的数值边界
- [物品系统](item-system.md) —— `ItemStack` 三个字段、六档品质（否决②的前提）
- [Agent 目标与经济](agent-goals-and-economy.md) —— 行会中介贸易（G1 回应里
  「多出来的一件是可卖的」那条依赖）
- [ADR 0021](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md)
  —— 四节 A2 的判据来源
- [ADR 0027](../decisions/0027-content-hash-covers-field-values.md) —— 落地清单
  第 7 条的义务来源
- [ADR 0023](../decisions/0023-script-state-writes-go-through-apply.md) —— 本方案
  零新增 `Effect`、写入仍只经 `apply` 的合规依据
