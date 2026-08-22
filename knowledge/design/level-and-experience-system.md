# 等级与经验系统：单一角色等级，递推式经验曲线，机器复用不复用类型

**冻结于** 2026-08-20。**落地状态**：骨架已全部落地并接进生产路径——`Agent.level`/`experience`/`xp_to_next_level`、`XpCurveDef` 求值机器、`Effect::GrantExperience` 与 `apply` 侧升级循环、`register-xp-curve`/`register-class-xp-curve`/`register-race-xp-curve` 三个注册函数、击杀经验结算，全部经 `ll_sim::turn::TurnEngine` 可达（端到端证据见 `crates/ll-mod/tests/example_mod_kill_experience.rs`）。五节的两条结论已被项目所有者部分推翻，见该节「更新」一小节。原冻结时的落地状态记录（纯设计、`crates/` 中无任何对应类型、核对提交 `98621f5`）保留在此备查。

**并发声明**：本次任务与另外两路并行工作共享同一工作树——存档迁移链清理（`ll-content`）、本体贴图命名空间前缀化（`ll-mod/asset_vfs`+`ll-render`+`ll-game`+demo+`assets/`）、天赋/特性系统设计（正在写 `knowledge/design/` 下的新文件并会改 `README.md`）。本文档只新增这一个文件，**不改 `README.md`**（索引由协调者统一更新），不触碰 `crates/**`/`mods/**`/`assets/**`/`docs/**`/`.github/**`。天赋系统那份文档在写作本文档时尚未提交（`git log -- knowledge/design/` 与 `git status` 均未见其踪迹），本文档第七节只能对照**项目所有者本人转述的倾向**对齐，不能对照该文档的已提交正文——这一点如实标注，不假装核对过一份还不存在的文件。

---

## 零、项目所有者的要求

> 「对诶，我忘了设计等级系统，还有不同种类的经验需求公式」

---

## 一、现状核实：确实从零设计

去代码核实的结果，逐项列出：

- **`Agent`**（`crates/ll-world/src/entity/agent.rs`）：`pos`/`stats`/`next_action_at`/`health`/`affiliations`/`wallet`/`profession`/`goals`/`race`/`current_space`/`luck`/`mana`/`stamina`/`unlocked_skills`/`skill_cooldowns`/`subclasses`/`active_stat_modifiers`/`script_state`/`creature_kind`/`spawned_at`/`remembered_id`——**逐字段看过一遍，没有一个是等级或经验值**。
- **`BaseStats`**（`crates/ll-world/src/entity/stats.rs`）：六项主属性，没有等级。
- **`ClassDef`/`ClassAttrs`**（`crates/ll-mod/src/class.rs`，已落地）：`id`/`display_name_key`/`primary_attribute`，没有等级或经验曲线字段。
- **`SubclassDef`/`SubclassAttrs`**（`crates/ll-mod/src/subclass.rs`，已落地）：`id`/`display_name_key`，同样没有。
- **`SkillDef`**（`crates/ll-mod/src/skill.rs`）：`owning_class`/`prerequisites`/`cooldown_ticks`/`resource_cost`/`effect`，技能解锁走的是前置技能 DAG（`unlocked_skills.contains`），不是等级门槛。
- **`RaceDef`**（`knowledge/design/race-system.md`，未落地）：`stat_modifiers`/`darkvision_cells`/`footprint`/`lifespan_years`，没有等级。
- **`skill-learn-requirements.md`**（2026-08-20 冻结，纯设计）第三节已经预留了一个信号，值得转述：设计"盗贼且等级 5"这类技能可学条件时，作者写道「"等级 5"属于属性阈值扩展位——本次最小集合不含它，但结构上已经留了「加一个新字段」的口子」——**这是全代码库/全设计文档里唯一一处承认"等级"这个概念将来会被用到的地方，但它本身没有定义等级是什么、怎么增长**。本文档正是补上这个缺口。
- **全代码库搜索 `升级`/`经验值`/`leveling`/`character level`**：命中的全部是"版本升级""地形阻挡等级""难度等级""商队报价升级"这类无关用词，没有一处指向角色成长。

**结论：项目所有者的判断准确——这是从零设计，没有任何既有字段或行为需要兼容/迁移。**

---

## 二、什么东西有等级：角色总等级，一个字段，不拆分

### 结论

```rust
// Agent（新增两个字段，位置建议紧邻 unlocked_skills/subclasses）
pub level: i32,
pub experience: i64,
pub xp_to_next_level: i64,   // 见三节「为什么必须缓存」
```

**角色总等级**——单一整数，挂在 `Agent` 上，与 `profession`/`subclasses`/`unlocked_skills` 平级，不拆成"职业等级"或"技能等级"。

### 为什么不是职业等级（D&D 多职业式）

D&D 的"战士 3 级 / 法师 2 级"要求**每个职业各自累计经验、各自独立成长**——这个模型的前提是"同一个角色可以在多个职业之间分配经验投入"。本项目的职业模型不是这样：`Agent.profession: ContentIndex` 是**单值**（恰好一个主职），`Agent.subclasses: Vec<ContentIndex>` 是"持有哪些副职类型"的**集合**，不是"每个副职各自的等级/经验"——`class-skill-quest-system.md` 第三节定案的"主职与副职共享同一份技能命名空间"，其潜台词就是副职不是一条独立的成长轴，只是"解锁了另一批可学技能的资格"。给一个当前不存在"分配经验"这个操作的模型强行叠加"每个职业各自计经验"，是在没有真实需求的地方新增一整套账本（YAGNI）。

### 为什么不是技能各自等级（上古卷轴式）

技能解锁走的是**离散的 DAG**（`unlocked_skills: Vec<ContentIndex>`，学会/未学会二值，前置关系校验成环），不是"用得越多越熟练"的连续量。上古卷轴式设计要求每个技能有自己的经验累积与等级——这要求给每个 `(Agent, SkillDef)` 组合开一条独立的成长记录，是比本文档大得多的一套系统，且当前技能系统的"可学"闸门（`skill-learn-requirements.md`）设计的是种族/职业/副职的静态资格,不是"用了多少次"的动态积累。这是一个完全不同的成长哲学，与项目所有者这次的原话（"不同种类的经验需求公式"，暗示的是"升级"这件事本身，不是"练技能"）不吻合，标记为将来可能的独立系统，本次不做。

### 与 `subclass` 的关系：解锁机制不归等级系统

项目所有者问："D&D 的『N 级选择子职』是不是就靠等级触发？"——**答案是：等级系统本身不管这件事**。`Agent.level` 只回答"现在几级"，谁来读这个值、在什么条件下允许持有某个副职，属于**副职系统自己的资格判定**，与技能可学条件（`skill-learn-requirements.md` 的 `SkillRequirement`）应该走**同一套机制**：给 `SubclassDef`（或某个专门的 `SubclassRequirement`）加一个"所需等级"字段，消费 `Agent.level` 这个已经存在的公开整数，不需要等级系统专门为副职开一个接口。这与第六节"等级解锁归天赋系统"是同一条设计哲学的两次应用：**等级系统只产出一个可被随时读取的整数，"达到这个整数能做什么"永远是消费方各自的判断，不塞进等级系统本身**——这也是为什么 `Agent.level` 是一个普通公开字段而不是包一层"能否解锁 X"的查询 API：消费方需要的输入形状（种族+职业+副职+等级的组合判断）差异很大，没有一个通用查询能覆盖全部消费场景，硬造一个只会变成"一半消费方绕开它自己比较"的样子。

### 被否决的方案

- **职业等级（D&D 多职业）**——否决，理由见上文：当前项目没有"经验可以在多个职业间分配"这个前提,是在不存在的需求上新增系统。
- **技能各自等级（上古卷轴式）**——否决，理由见上文：与当前离散 DAG 解锁模型的设计哲学不符，规模远超本次需求。
- **职业等级 + 角色总等级并存（例如角色总等级是职业等级之和）**——否决：当前只有一个主职，"职业等级之和"在只有一项时退化成角色总等级本身，多引入一层从不产生区分度的抽象，纯粹的复杂度税。

---

## 三、经验需求公式：复用伤害公式的机器（模式），不复用它的类型

### 三步判据（[ADR 0018](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)）——与伤害公式同一套判据，结论也相同

**第一步：有没有设计自由度？** 有——不同职业/种族的升级曲线形状不同（本节四示例会证明"不同"到什么程度）。**第二步：自由度落在算法还是参数上？** 落在算法本身（"经验需求随等级怎么增长"），不是某个可调系数。**第三步：调用频率是档位问题不是层问题**——升级事件比伤害结算稀疏得多（一局游戏几十到几百次量级，远不及伤害公式"几十万次"的量级），但判据不因为频率低就退回一档：经验需求依然消费**运行期才存在的输入**（当前等级、上一级门槛），不是可以提前物化成一张表的静态值（下一小节展开）。**结论：二档，与伤害公式同一档。**

### 为什么不是一档（查表）

若经验需求只是"某个等级对应固定一个数"，理论上可以用 `Vec<i64>` 直接按等级下标查表，是一档。但项目所有者要的是"公式"，且第四节两条示例会证明**递推式**曲线（后一级门槛依赖前一级门槛的结果）无法被"提前烘焙进一张数组"这件事本身消解——除非游戏在装载期就把 1 到某个上限的全部等级门槛都算好存成表，这与"公式"这个词的本意相悖（作者写的是生成规则，不是手抄一份数值表），也会在等级上限调整时需要重新枚举，不是本设计要走的路。

### 可行性结论：机器可以复用，类型不能

**复用什么**：s-表达式作为**声明载体**（`quote` 包起来的数据，白名单跳过其内部）、**装载期编译成扁平指令数组**、**运行期零脚本参与**、**全整数、除法向零截断**（[ADR 0002](../decisions/0002-integer-only-world-state.md)/[ADR 0020](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)）——这四条是`damage-formula-mod-api.md`定档的"第二档"整套机制,与领域无关，经验需求公式原样适用。

**不复用什么：`FormulaOp`/`FormulaOperand`/`FormulaDef` 这三个 Rust 类型本身**。理由是项目所有者自己点名的风险——「如果复用会污染伤害公式的设计」：`FormulaOperand` 现有的 `AttackPower`/`Defense`/`PenetrationFlat`/`PenetrationPermille`/`AttributeModifier`/`Crit` 六个操作数，没有一个在"经验需求"这个领域里有意义；反过来，经验需求需要的"当前等级""上一级门槛"两个操作数，塞进 `FormulaOperand` 会让伤害表达式的作者在写武器公式时,IDE 自动补全里混进两个跟战斗毫不相关的符号——两个领域共用一个操作数枚举,枚举本身就会变成两份互不相关的需求各自过时的注脚,不是任何一方乐见的。

**替代方案：定义一套结构同构、类型独立的姊妹类型**，与 `formula.rs` 平级：

```rust
// crates/ll-sim/src/xp_curve.rs（设计，未落地）
pub struct XpCurveDef {
    pub id: NamespacedId,
    /// 从 1 级升到 2 级所需经验的种子值——递推公式需要一个起点，
    /// 见「为什么需要种子值」一节。
    pub base_requirement: i64,
    pub instructions: Vec<XpCurveOp>,
}

pub enum XpCurveOp {
    Const(i64),
    Ref(XpCurveOperand),
    Add(XpCurveOperand, XpCurveOperand),
    Sub(XpCurveOperand, XpCurveOperand),
    Mul(XpCurveOperand, XpCurveOperand),
    Div(XpCurveOperand, XpCurveOperand),
    MulPermille(XpCurveOperand, XpCurveOperand),
    Min(XpCurveOperand, XpCurveOperand),
    Max(XpCurveOperand, XpCurveOperand),
    Select { cond: XpCurveCond, if_true: XpCurveOperand, if_false: XpCurveOperand },
}

pub enum XpCurveCond {
    Lt(XpCurveOperand, XpCurveOperand), Le(XpCurveOperand, XpCurveOperand),
    Gt(XpCurveOperand, XpCurveOperand), Ge(XpCurveOperand, XpCurveOperand),
    Eq(XpCurveOperand, XpCurveOperand), Ne(XpCurveOperand, XpCurveOperand),
}

pub enum XpCurveOperand {
    Const(i64),
    Local(u8),
    /// 即将离开的那一级（求"从 N 级升到 N+1 级需要多少经验"时，
    /// 这个操作数就是 N）。
    Level,
    /// 上一次同一条曲线求值算出的门槛——递推的输入，见下一小节。
    PrevRequirement,
}
```

**求值语义**：不是"给定等级 N，纯函数算出总经验"，是"给定即将离开的等级 N 与上一级门槛，算出这一级需要多少经验"——`XpCurveDef` 求的是**相邻两级之间的差值（delta）**，不是从零累积的总量。

**编译器落在哪个 crate**：与伤害公式同构——编译器（`SteelVal → Vec<XpCurveOp>`）落在 `ll-mod`（新增 `script_xp_curve_api.rs`，依赖 `steel-core`），`XpCurveDef`/`XpCurveOp`/`XpCurveOperand` 类型定义与求值器落在 `ll-sim`（新增 `xp_curve.rs`，纯 Rust 整数运算，不依赖 `steel-core`）。`ll-mod` 的编译器 `use ll_sim::xp_curve::{XpCurveOp, ...}`，与 `ll-mod` 复用 `ll-sim::formula` 的方式完全同构，见 `damage-formula-mod-api.md`「编译产物住在哪个 crate」一节。

### 需要哪些伤害公式没有的算子：**零个**

`+`/`-`/`*`/`/`/`mul-permille`/`min`/`max`/`if` 这套算术子集已经完整——四节两条示例会证明单靠这几个算子就能表达"线性"与"指数式"两种截然不同的成长节奏。**唯一新增的是操作数,不是算子**：`level`（当前离开的等级）与 `prev-requirement`（上一级门槛，供递推）。

### 为什么不需要 `pow`：用递推代替指数，零新增算子换来一个真正指数增长的曲线

真正的"指数增长"（门槛 = 基数 × 增长率^等级）若写成"等级的纯函数"，需要计算"增长率的 N 次方"，`N` 是运行期才知道的等级——这与伤害公式指令集一条贯穿全篇的纪律正面冲突：**`Dice`/`AdvantageRoll`/`multi-hit` 的内部重复次数永远是编译期已知的字面常量**（`(d N S)` 的 `N`/`S`、`multi-hit` 的 `rounds`,均在注册期校验为字面整数，`damage-formula-mod-api.md` 三节表 8），从未出现过"一条指令内部的重复次数由运行期输入决定"这种形状。给经验公式发明一个"以等级为指数的 `Pow`"，会是伤害公式指令集里从未允许过的一种新风险——运行期这一步要执行多少次乘法，取决于这一刻角色恰好几级，理论上随游玩时长无上限增长，与"一条指令的执行时间是一个编译期就能确定的上限"这条贯穿全文的保证背道而驰。

**递推法完全绕开这个问题**：不要求公式一次性算出"任意等级的门槛"，只要求它算出"下一级比这一级多多少"，把"逐级复利"这件事交给**升级事件本身的循环**（见五节），不是交给公式内部的循环。这样"指数式增长"的效果由多次调用同一个只做加减乘除的公式自然叠加出来——**递推是"运行期的循环"，公式内部依然是零循环的扁平指令数组，两者不矛盾**：`XpCurveOp` 的求值器本身与伤害公式的求值器一样"每条指令恰好执行一次"，会重复的是"外层调用这个求值器很多次"，不是"求值器内部某条指令重复很多次"——这条区分正是伤害公式`multi-hit`一节反复强调的"有界重复必须是指令内部的、编译期已知的"这条纪律的延伸，不是破例。

### 为什么需要种子值 `base_requirement`

递推需要一个起点——"从 1 级升到 2 级需要多少经验"这一步，没有"上一级"可以引用。`register-xp-curve` 因此比 `register-damage-formula` 多一个参数：

```scheme
(register-xp-curve "mymod:example" base-requirement (quote 表达式))
```

第一次求值（1 级→2 级）时，`prev-requirement` 操作数取值 `base_requirement` 本身（一个普通的注册期整数常量，不经过表达式）；此后每一级，`prev-requirement` 取上一次表达式求值的结果——完整机制见五节。

### 不需要哪些伤害公式有的算子：骰子、优势/劣势、多轮判定——全部拒绝

`(d N S)`/`(adv ...)`/`(disadv ...)`/`(multi-hit ...)`/`crit` 操作数——**一个都不进 `XpCurveOp`/`XpCurveOperand` 的封闭表**。理由不是"用不上"这么简单,是两条更硬的理由：

1. **经验需求是确定性数据，不是结算结果**。伤害需要随机性是因为"同一下攻击每次感觉不同"是战斗的设计目标；但"升到 11 级需要多少经验"若掷骰决定，会让玩家在查看"还差多少经验"这类 UI 时看到一个会无理由跳动的数字——**没有任何游戏设计动机支持经验门槛本身是随机的**，随机性该出现在"这一次击杀掉落多少经验"（属于五节"经验来源"给分数量,不是本节"需要多少经验才能升级"）,两者是完全不同的量,不能混为一谈。
2. **封闭表本身就是安全边界**——与伤害公式"未知符号即拒绝"同一条纪律（`damage-formula-mod-api.md`三节）：`XpCurveOp` 的编译器压根不认识 `d`/`adv`/`disadv`/`multi-hit`/`crit` 这几个符号,mod 作者若在经验曲线里写了它们,直接触发"未知符号"编译期错误,与写了 `lambda` 走同一条拒绝路径——不需要专门为"骰子类算子出现在错误的领域"写一条特判,普适的"封闭表之外一律拒绝"规则本身已经覆盖。

---

## 四、两条真实曲线示例：线性 vs 递推式指数，证明是不同的规则而不是调系数

### `register-xp-curve` 签名

```scheme
(register-xp-curve id base-requirement (quote 表达式))
```

- `id`：完整命名空间标识符字符串。
- `base-requirement`：1 级→2 级所需经验的种子值,整数字面量。
- 第三参数：`quote` 包起来的表达式,语法与伤害公式共享同一套书写风格（中缀前缀混合的 s-表达式,`min`/`max`/`if`按二元列表形式书写）,但作者能引用的操作数只有 `level`/`prev-requirement`,不含伤害公式那六个战斗专属操作数。

### 示例一：战士，线性——纯等级函数,不碰 `prev-requirement`

```scheme
(register-xp-curve "lostland:warrior_xp_curve" 140
  (quote (+ 100 (* level 40))))
```

**语义**：从等级 N 升到 N+1，需要 `100 + 40×N` 点经验。第 1→2 级用种子值 `140`（等价于代入 N=1：`100+40=140`，种子值与公式在这里恰好一致,是设计者刻意让两者对齐，不是必须——种子值允许与代入 N=1 的结果不同,用来表达"第一级刻意更容易/更难"这类特殊调校，本例不需要,直接对齐）。

| 从…升到… | 所需经验 |
|---|---|
| 1→2 | 140 |
| 2→3 | 180 |
| 5→6 | 300 |
| 9→10 | 460 |
| 15→16 | 700 |
| 30→31 | 1300 |

**这条曲线完全不读 `prev-requirement`**——只读 `level`,是一个纯函数,不依赖递推链,展示的是"线性"这条曲线不需要用到本节新增的第二个操作数,`level` 一个就够。

### 示例二：法师，递推式指数——早期近似线性保底，后期真正复利

```scheme
(register-xp-curve "lostland:mage_xp_curve" 80
  (quote (max (+ prev-requirement 20)
              (mul-permille prev-requirement 1180))))
```

**语义**：下一级门槛 = `max(上一级门槛+20, 上一级门槛×1.18)`——两条分支哪个大用哪个。`mul-permille` 就是伤害公式里已经存在的千分比乘法算子，`1180` 即 118.0%（增长 18%）。

逐级手算（`mul-permille` 除法向零截断，与既有约定一致）：

| 从…升到… | 上一级门槛 | `+20` 分支 | `×1.18` 分支 | 取较大值 |
|---|---|---|---|---|
| 1→2 | 80（种子值） | 100 | 94 | **100** |
| 2→3 | 100 | 120 | 118 | **120** |
| 3→4 | 120 | 140 | 141 | **141** |
| 4→5 | 141 | 161 | 166 | **166** |
| 5→6 | 166 | 186 | 195 | **195** |
| 8→9 | 271 | 291 | 319 | **319** |
| 9→10 | 319 | 339 | 376 | **376** |
| 14→15 | 725 | 745 | 855 | **855** |
| 15→16 | 855 | 875 | 1008 | **1008** |

**这为什么是「不同的一套规则」而不是调系数**：

1. **早期由加法分支主导（1→2 到 3→4 级，`+20` 更大或与乘法分支持平）,后期由乘法分支主导（4→5 级起，`×1.18` 反超并从此不再让位）**——两条分支的交叉点在门槛≈111 附近（`20 / 0.18 ≈ 111.1`）,这不是刻意调出来的数字，是两个算子（加法/千分比乘法）自身的数学性质决定的自然交叉点,战士曲线不存在这种"两段式"结构,它自始至终只有一条直线。
2. **到 15→16 级,法师门槛（1008）已经超过战士同一级门槛（700）——即便法师的起点（种子值 80）远低于战士（140）**。这正是"成长节奏不同"而非"调系数"的直接证据：若只是调系数，起点更低的曲线要在某一级反超起点更高的曲线,需要的是系数恰好选对；但这里反超不是巧合调出来的,是"线性 vs 复利"这两种数学结构本身的必然结果——任何固定增长率大于零的复利曲线,只要级数足够多,终将超过任何线性曲线,不需要为了"证明不同"刻意选数字。
3. **`prev-requirement` 操作数本身是战士曲线完全用不到的**——法师曲线的形状**依赖历史**（这一级门槛由上一级门槛决定，层层累加），战士曲线**不依赖历史**（每级门槛只看等级数字本身）。这是两种不同的"公式对时间的态度"，不是同一个公式换了参数。

---

## 五、经验来源：`Effect::Kill` 是正确的挂载点，`KillRecord` 不是

### 核实结论：`KillRecord`/`HistoricalEvent::Kill` 的信息够，但它是错误的挂载点

`kill-and-death-events.md` 三节把击杀分三档：「玩家相关」「具名 NPC 相关」全记 `HistoricalEvent::Kill`，「无名小卒之间」**完全不产出事件**，只累加进死因统计聚合。这意味着——**如果经验产出挂在 `HistoricalEvent::Kill`/`KillRecord` 上,游戏里绝大多数的战斗击杀（无名小卒互相击杀,以及很可能占多数的"玩家杀无名小卒但对方够格进"具名相关"档"这条线之外的情况）都不会触发经验产出**，这与"打怪能升级"这个几乎所有 RPG 的基本预期直接冲突。**`KillRecord` 这个信息载体本身的字段是够用的（`killer`/`victim`/`cause`/`killing_blow` 足以判断"谁杀的、杀了什么、怎么杀的、这一下多重"），但它被产出的**时机**只覆盖"值得被记住"这个远比"值得给经验"更窄的子集**——这是本文档核实出的一处真正的接线点错配，不是信息量不够。

**正确的挂载点是 `Effect::Kill`**（`crates/ll-sim/src/effect.rs`）——它由 `resolve_attack`/`resolve_use_skill` 在**每一次**击杀（无论死者是否具名）产出，是 `HistoricalEvent::Kill` 的严格超集。经验产出应该在 `resolve` 判断"这次击杀该不该记进历史"的**同一个位置**，独立地判断"这次击杀该给多少经验"——两件事目前共享同一个触发信号（一次死亡），但服务不同的下游，不应该让"值得记录"这个更严格的判据顺带决定"该不该给经验"。

### 核实出的真正缺口：经验值本身没有出处

`KillRecord`/`Agent` 现有字段能回答"谁杀的""怎么杀的""这一下多重",但回答不了**"杀了这个东西该给多少经验"**——这才是项目所有者括号里点名的缺口（"`KillRecord` 不记录死者种类"）的准确定位,但缺口比"记录死者种类"更深一层：

- `Agent.creature_kind: Option<ContentIndex>` **已经存在**（`agent.rs` 字段，`kill-and-death-events.md` 六节要消费的落点），`None` 时退回 `Agent.race`——**死者的"种类"信息本身是够的**，`resolve` 完全能从 `victim: EntityId` 反查到它死前的 `creature_kind`/`race`。
- **真正缺的是"这个种类的生物死亡时该给多少经验"这份数值**——全代码库、全设计文档都没有任何地方声明过这个数字。既没有 `CreatureDef.xp_reward` 这样的字段（`CreatureDef` 本身也不存在，怪物内容目前完全借用 `race`/`creature_kind` 这两个通用字段），也没有按 `KillCause` 或武器/技能反推经验值的机制。
- **是否可以按"死者自己的等级"算经验（如"经验 = 死者等级 × 系数"）**：理论上可行的前提是死者本身也是一个有 `level` 字段的 `Agent`——本文档二节把等级放在 `Agent` 上、不区分玩家和 NPC，因此**如果死者是一个已经"升格"成厚层的 `Agent`（无论玩家、随从还是被具名的怪物首领），它确实带着自己的 `level`,`resolve` 可以直接读取**。但薄层 `ThinPopulation`（背景 NPC/杂兵，`crates/ll-world/src/entity/thin.rs`）目前的列式存储里没有 per-instance 的等级列（与它不追踪逐项属性、只在升格时用 `BaseStats::BASELINE` 填充是同一条设计纪律），意味着**游戏里数量最多的"薄层杂兵"死亡时，没有一个天然可读的"它的等级"**——按死者等级算经验这条路对占多数的薄层击杀行不通。

### 更新（项目所有者裁定，升级加点批次）：基准值按 `creature_kind` 注册，最终值按等级差算

**项目所有者推翻了本节两条结论中的各一半**，原话：

> 有个最低经验 1xp，然后等级差越多给越多，有个经验公式。

落地后的形状是两条结论的合成，不是任何一条的整体作废：

- **「按 `creature_kind` 注册」保留**——`RaceDef.xp_reward` 仍然是内容作者声明的那一个数，仍然对薄层/厚层一视同仁。但它的语义收窄成**公式的基准输入**，不再是玩家最终拿到的数字。
- **「不按死者等级算」被推翻**——最终经验 = `max(1, 基准值 × clamp(100 + 25 × (死者等级 − 击杀者等级), 10, 400) / 100)`，落在 `crates/ll-sim/src/experience.rs` 的 `kill_experience`。

**本节当初否决「按死者等级算」的那条理由，在真实代码里不成立**（这是核实出来的，不是被绕开的）：否决的依据是「薄层杂兵没有 per-instance 等级列」。但 `Effect::Kill` 的 `target` 是一个 `EntityId`，指向的是 `world.actors` 这个**厚层竞技场**——薄层背景 NPC 根本不在其中。一个薄层实体要被攻击，必须先经 `ThinPopulation::promote` 升格成厚层 `Agent`，升格那一刻它就有了 `level` 字段。换句话说：**能被 `Effect::Kill` 点名的死者，恒定是有等级的**。本节的否决对「薄层不需要升格就能被杀」这个假设是对的，但那个假设在当前代码里从不成立——`append_kill_experience` 自接线之初就在做 `world.actors.get(target)` 这次查询。

**等级差的方向取「杀比自己高的给得多」**，理由不是惯例而是裁定的前半句本身：「最低 1xp」这个保底之所以需要存在，只可能是因为存在一档给得极少的击杀；若按「差的绝对值越大给越多」读，碾压弱小目标反而给得多，保底就没有任何会触发的场合，那半句话会变成空文。两半句话只有在这个读法下才同时有意义。

**保底夹在公式的最后一步**，与 `attribute-system.md` 四节「为什么下限必须夹在最后一步」记的那次教训完全同构：夹在倍率之前时，`1 × 10 / 100 = 0`，「最低 1xp」在它唯一本该生效的场合失效。

**本体三族因此不再豁免**：`mods/lostland/races.scm` 给人类/矮人/精灵各自声明了基准值（10/12/12），`ll_mod::content_audit` 里那条「本体三族是可玩种族不是猎物」的字段豁免随之摘除。

### 结论：按 `creature_kind` 注册固定经验值，不按死者等级算

**本文档建议的落点**：新增一张小注册表（`CreatureXpTable` 或直接扩展进未来的 `CreatureDef`，本次只给形状不落地）,把 `ContentIndex`（`creature_kind`，`race` 退化兜底）映射到一个固定的 `xp_reward: i64`——与武器/技能各自声明伤害公式是同一个模式：**内容作者在注册这个种类的生物时,顺手声明"杀死它给多少经验",与它是厚层还是薄层实体无关**（薄层杀死照样能查表拿到经验值,不需要它先有 `level` 字段）。这条路径对齐"本体即 Mod"的既有惯例,且不需要触碰 `ThinPopulation` 的列式布局——**是本节核实后给出的唯一现实可行的方案**，按死者等级算经验的方案在薄层普遍存在的前提下不成立，予以否决。

### 被否决的方案

- **挂在 `HistoricalEvent::Kill`/`KillRecord` 上**——否决：覆盖面只有"值得记录"这一档,绝大多数战斗击杀不触发,与"打怪升级"的基本预期冲突,见上文核实。
- **按死者自身的 `level` 计算经验**——~~否决（当前批次）~~**已被项目所有者推翻，见本节「更新」一小节**：原文理由是多数击杀的目标是薄层杂兵,没有 per-instance 等级字段可读,这条路只能覆盖厚层实体（玩家、随从、具名怪物），不是通用方案；若未来薄层也需要某种"等级"概念（例如给薄层加一列近似等级用于经验计算),是一次独立的、需要单独评估存储代价的设计,本文档不代为决定。
- **按 `KillingBlow.damage`（这一下打了多少伤害）反推经验**——否决：伤害量是"用什么打的""打得多准"的函数,与"这个生物本身值多少经验"没有必然关系(一拳打死一只弱怪与蓄力半天打死同一只弱怪,给的经验没有理由不同),把两个不相关的量绑在一起会让内容作者没有独立调节"这只怪物值多少经验"的手段。

### 现在能做的 vs 等什么

**现在能做**：`Effect::GrantExperience { target: EntityId, amount: i64 }`（新变体,与 `Effect::Damage`/`Effect::Kill` 平级）的形状可以现在定,`resolve` 在产出 `Effect::Kill` 的同一处,若能查到 `victim` 的 `creature_kind`/`race` 对应的 `xp_reward`,一并产出这个新效果——**这部分不依赖任何尚未落地的系统**。

**等什么**：`CreatureXpTable`（或等价的 `xp_reward` 注册位）本身没有被设计过,需要单独一次内容注册表设计（形状与 `ClassTable`/`SkillTable` 同构,不复杂,但本文档的范围是"等级与经验系统骨架",不下场设计第七张内容注册表）；任务完成/探索/制作三类经验来源——项目所有者的要求里没有点名,本文档按要求只列出不设计（任务系统本身`class-skill-quest-system.md`第四节的 `QuestDef` 仍未落地,探索/制作目前连玩法机制本身都不存在）。

---

## 六、等级是世界状态：走 `Effect` 管线，升级判定整段丢给 `apply`

### 进 `WorldState` 与进 `hash()`

`level`/`experience`/`xp_to_next_level` 全部是 `Agent` 的新字段,随 `Agent` 一起存进 `WorldState.actors`（[0002](../decisions/0002-integer-only-world-state.md) 全整数、无浮点已满足——三者均是 `i32`/`i64`)。但**光是字段挂在 `Agent` 上不代表自动进 `hash()`**——核实过 `WorldState::hash()`（`crates/ll-world/src/state.rs:876` 起）,它对 `self.actors` 的遍历是**逐字段手写**（`agent.pos`/`agent.health`/`agent.wallet`……一行一个 `hasher.write_*`），**不是"整个 `Agent` 结构体自动折叠进哈希"**——这正是 [ADR 0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) 点名警告的失效模式（"判据字段不全 → 输入里该有的东西没被采样 → 守护形同虚设"，该 ADR 记录的三个历史实例里两个正是"新增 `Agent`/`WorldState` 字段但忘了同步进 `hash()`"）。**本设计落地时必须在 `WorldState::hash()` 的 `for agent in self.actors.iter()` 循环体内，紧邻 `agent.mana`/`agent.stamina` 那两行 `write_i64` 之后，手动补三行**：

```rust
hasher.write_i32(agent.level);         // 假设保留现有 write_i64 家族，i32 需先转换
hasher.write_i64(agent.experience);
hasher.write_i64(agent.xp_to_next_level);
```

这不是本文档的推测性提醒，是对着 `state.rs` 现有代码结构核实后的具体施工指引——不像大多数字段那样"提醒读者记得做",而是精确到"在哪一行之后加"。

### 为什么 `xp_to_next_level` 必须缓存，而不是像 `health`/`mana` 那样现算

`Agent` 现有的字段文档反复强调一条纪律（`health`/`mana`/`stamina` 共享的注释）：「只存当前值，不存上限——上限由 `stats` 现算，不需要同步维护」。**`xp_to_next_level` 看起来违反了这条纪律,但理由是站得住的**：`health`/`mana` 的上限是**纯函数**（体质/智力现算,`derive_stats` 一次调用就出结果),而三节定义的 `XpCurveDef` 是**递推**（下一级门槛依赖上一级门槛的求值结果）——要"现算"某个等级的门槛,必须从 1 级开始重放整条递推链,是 `O(当前等级)` 的工作量,不是 `O(1)`。对一个可能玩到几十甚至上百级的角色,每次查询"还差多少经验"都重放一遍历史,是不必要的重复计算。**缓存 `xp_to_next_level`，只在每次真正升级那一刻重新求值一次（`O(1)` 增量更新）**，与 `Agent.skill_cooldowns` 存"到期时刻"而非"剩余时长"是同一类"存一个能被增量维护的量,而不是每次都从头推导"的设计选择,不是破例,是同一条纪律在不同数学结构下的正确应用。

### 升级判定：走 `Effect` 管线，判定本身整段交给 `apply`

**必须走 `Effect` 管线,没有例外**——[ADR 0006](../decisions/0006-intent-resolve-effect-apply.md) 的 C1 约束（`apply` 是全局唯一能修改世界的地方）与 [ADR 0023](../decisions/0023-script-state-writes-go-through-apply.md)（脚本状态写入必须经 `apply`，"任何东西一旦被放进 `WorldState`，就自动被约束覆盖，不存在因为存储位置特殊所以可以绕开管线的例外"）对经验值/等级同样成立——它们和脚本状态一样,唯一存在的意义就是要进存档,没有理由是这条纪律的例外。

**具体切法**：`resolve` 判断出"这一击/这次任务该给多少经验"后，产出 `Effect::GrantExperience { target, amount }`——**只携带"给多少"这一个决定,不携带"该不该升级""升几级"这类判断**。真正的升级判定（"加上这些经验后有没有超过 `xp_to_next_level`，超过了就扣掉门槛、等级加一、重新求值下一级门槛，可能连续触发好几次"）**整段放进 `apply` 一次算完**，理由是这段逻辑**没有下游需要 `resolve` 提前知道结果的次生判断**——对照 `Effect::Damage`：`apply` 直接做减法，但"是否致死"必须在 `resolve` 先判断出来才能同时产出 `Effect::Kill`（下游确实需要这个判断结果）。升级不存在这种"下游需要提前知道"的情况——没有第二个效果依赖"这一下是否升级"这件事本身，`apply` 内部循环比较 `experience >= xp_to_next_level` 并调用求值器重算下一级门槛，是纯粹的整数算术，不涉及脚本/VM，与 `resolve` 内部调用 `damage_after_defense` 同一层级的"纯 Rust 函数调用",没有理由为了"看起来更像 resolve 该干的事"而人为拆成两步。**不需要额外的 `Effect::LevelUp` 变体**——升级是 `Effect::GrantExperience` 在 `apply` 侧的一个自然后果，不是一个需要单独被"决定"的独立效果。

### 被否决的选项

- **升级判定放进 `resolve`（`resolve` 算出新等级、新经验值，`apply` 只是"整体替换"）**——否决：`resolve` 只有只读访问权限，理论上能读到旧值算出新值再整体塞进 `Effect`，但这样 `Effect::GrantExperience` 的载荷会从"给多少经验"膨胀成"整个新的 level/experience/xp_to_next_level 三元组",`apply` 变成纯粹的赋值——这与 `Effect::Damage` 只携带 `amount`（一个决定,不是最终状态)的既有范式不一致,且如果同一个 tick 内一个实体连续吃到两条 `GrantExperience`（理论上可能,例如一次群体技能同时击杀多个目标各自触发经验),两条效果各自携带"resolve 时刻算出的新状态"会互相覆盖而不是叠加——是一个真实的正确性风险,不是理论洁癖。
- **引入独立的 `Effect::LevelUp`，由 `resolve` 显式产出**——否决：见上文,升级没有下游判断依赖它,不需要在 `resolve` 阶段就把"是否升级"这个信息暴露成一个独立效果供其他逻辑读取,现在这么做是在没有消费者的情况下预先设计一个接口(YAGNI)。

---

## 七、与天赋系统的接缝：等级解锁归天赋系统，等级系统只产出一个整数

### 核实限制：对方文档尚未提交，本节只能对齐项目所有者转述的立场

写作本文档时检查过 `knowledge/design/` 目录与 `git log`/`git status`，**天赋/特性系统的设计文档尚未提交，本文档看不到它已落定的正文**。以下结论因此不是"核对过对方文档后的一致性确认"，而是**对齐项目所有者本人在任务简报里转述的倾向**（"我倾向归天赋系统——天赋条目自己带一个『需要等级』字段，种族天赋填 1"）——如实标注这一点，避免让读者误以为两份文档已经互相核对过。

### 结论：同意项目所有者的倾向，且与本文档二节"副职解锁"的判断完全同构

**"等级解锁"这件事归天赋系统，不归等级系统**——理由与二节"副职解锁不归等级系统"是同一条：`Agent.level` 是一个任何系统都能读的公开整数，"达到某个等级能做什么"永远是**读它的那一方**的判断，不是**产出它的这一方**的职责。若把"解锁判定"塞进等级系统本身,等级系统就要反过来认识"天赋""技能""副职"这些完全不属于它的概念,职责边界会立刻模糊——这正是 `skill-learn-requirements.md` 已经确立、本文档二节复用、这里第三次复用的同一条设计哲学：**产出一个可读的原始值，消费方各自决定怎么用**。

天赋条目自己带"需要等级"字段（狂暴 1 级、额外攻击 5 级）、种族天赋固定填 1——这与三节 `SkillRequirement`（`skill-learn-requirements.md`）的现有形状完全对得上：`SkillRequirement` 已经是"种族/职业/副职"的声明式静态判据，三节原文已经预留了"等级 5"这类阈值扩展位。天赋系统若照同样的思路给天赋条目加一个 `required_level: i32` 字段，消费的正是本文档新增的 `Agent.level`——**两份文档在这一点上不需要互相调用对方的函数,只需要都读同一个公开字段**，是最低耦合的对接方式。

### 没有发现冲突

核实范围内（`Agent` 现有字段、`class-skill-quest-system.md`、`skill-learn-requirements.md`）没有发现与本文档相冲突的既有设计——`Agent.level` 是全新字段，不与任何已落地或已冻结的设计文档争用命名或语义。**唯一的风险点**：若天赋系统文档最终提交后,选择把"当前等级"存成天赋系统自己的一份拷贝（而不是直接读 `Agent.level`），会重演 `attribute-system.md`/`0010`/`0014` 已经出现过两次的"同一个概念被独立定义了两次"的教训——但这是一个**假设性风险**，不是本文档核实出的真实冲突,如实标注为需要协调者在两份文档合并进 README 索引时留意的一个点，不是本文档能单方面裁定的事。

---

## 八、mod 可配置：注册函数签名与档位

### 新增函数，不改既有函数

对齐 `register-skill` 刚定下的先例（"不要改既有函数的参数个数——会破坏真实 mod 脚本"）：本设计**只新增函数，不touch `register-class`/`register-skill`/`register-damage-formula`等任何已落地或已冻结的既有函数签名**。

| 函数 | 签名 | 档位 | 说明 |
|---|---|---|---|
| `register-xp-curve` | `(id base-requirement (quote 表达式))` | 二档 | 定义一条命名的经验曲线，见三/四节。 |
| `register-class-xp-curve` | `(class-id curve-id)` | 一档（纯绑定，不含表达式） | 把已注册的职业与已注册的曲线绑定,不存在的 `curve-id` 在注册期报错（对齐 ADR 0017"注册期完整校验"）。**新函数**，不改 `register-class` 本身——职业注册与"这个职业用哪条曲线"是两次独立的声明,与伤害公式"武器声明用哪条公式"走独立配置、不改武器注册函数本身是同一个模式（`damage-formula-mod-api.md` 十八/十九节，本文档未展开引用其具体形状，仅借用其"配置与定义分离"的思路）。 |
| `register-race-xp-curve` | `(race-id curve-id)` | 一档 | 与上一条同构，服务种族。 |

**未绑定的职业/种族退回 `lostland:default_xp_curve`**——与伤害公式"总有一条默认公式"（`damage-formula-mod-api.md` 十一节起）同一个兜底思路：本体注册一条默认曲线（形状留给具体数值设计批次,本文档不代为选定,与"不设计过头"的要求一致），未显式绑定的职业/种族没有查询失败的可能。

### 真实 mod 脚本：本文档暂不提供

`ADR 0018` 要求"玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证"——**本文档目前只给出签名与档位，没有落地任何 Rust 类型，因此也没有可以真实调用的引擎**，暂时无法像 `class.rs`/`skill.rs` 那样在 `mods/example_mod/gameplay.scm` 里补一段真实调用并跑通测试。这是本文档"纯设计"状态的直接后果，不是遗漏——落地实现批次开工时,这一条是补真实 mod 脚本证据的检查点，本文档如实标注，不假装已经满足。

---

## 九、现在能做的 vs 等什么

**现在能做（不依赖任何尚未落地的系统）**：

- `Agent` 新增 `level: i32`/`experience: i64`/`xp_to_next_level: i64` 三个字段，随 `WorldState` 存档，且手动补进 `WorldState::hash()`（六节已给出精确施工位置）。
- `ll-sim::xp_curve` 的 `XpCurveDef`/`XpCurveOp`/`XpCurveOperand`/求值器——结构完全类比已落地的伤害公式,不依赖任何新机制。
- `Effect::GrantExperience { target, amount }` 新变体,及 `apply` 侧"加经验、循环判定升级、增量重算 `xp_to_next_level`"的处理分支——不依赖任何尚未落地的系统。
- `register-xp-curve`/`register-class-xp-curve`/`register-race-xp-curve` 三个新脚本 API——依赖的编译器模式（`SteelVal → Vec<XpCurveOp>`）已有伤害公式作为已验证先例。

**等什么**：

- **经验来源的数值本体**——`CreatureXpTable`（`creature_kind → xp_reward`）没有被设计过，是产出经验这一步唯一缺失的环节（五节已定位）。
- **具体职业/种族的曲线数值**——本文档给的两条示例是"证明形状不同"用的示范,不是本体应该采用的最终平衡数值,与 `class.rs`/`subclass.rs` 现有"本文档只交付系统骨架,不交付具体数值"的既有边界一致。
- **副职/技能/天赋各自的"所需等级"字段**——`skill-learn-requirements.md` 已预留扩展位,天赋系统按七节的对接方式各自消费 `Agent.level`,均不在本文档范围内落地。
- **任务/探索/制作三类经验来源**——项目所有者的要求未点名，本文档不设计,只在五节末尾如实列出未展开。
- **薄层实体的等级**——五节已核实薄层没有 per-instance 等级列，若未来需要薄层也能"按等级给经验"或"薄层怪物也能升级"，是一次独立的存储代价评估，本文档不代为决定。

---

## 十、被否决的方案汇总（跨节索引，方便复核）

| 方案 | 否决位置 | 一句话理由 |
|---|---|---|
| 职业等级（D&D 多职业式） | 二节 | 当前项目没有"经验分配到多个职业"的前提。 |
| 技能各自等级（上古卷轴式） | 二节 | 与当前离散 DAG 解锁模型的设计哲学不符,规模远超需求。 |
| 职业等级+角色总等级并存 | 二节 | 只有一个主职时退化成角色总等级,纯复杂度税。 |
| 引入运行期指数 `Pow` 算子 | 三节 | 与"指令内部重复次数必须编译期已知"这条贯穿伤害公式指令集的纪律冲突。 |
| 骰子/优势劣势/多轮判定/`crit` 进经验曲线封闭表 | 三节 | 经验需求是确定性数据,不是结算结果,没有设计动机支持它随机波动。 |
| 经验挂在 `HistoricalEvent::Kill`/`KillRecord` | 五节 | 覆盖面只有"值得记录"这一档,漏掉绝大多数战斗击杀。 |
| 按死者自身等级算经验 | 五节 | 多数击杀目标是薄层杂兵,没有 per-instance 等级可读。 |
| 按 `KillingBlow.damage` 反推经验 | 五节 | 伤害量与"这个生物值多少经验"没有必然关系,混淆两个独立的量。 |
| 升级判定放 `resolve`，`apply` 只整体赋值 | 六节 | `Effect` 载荷会膨胀,且同 tick 多条效果时有互相覆盖而非叠加的正确性风险。 |
| 独立 `Effect::LevelUp` 由 `resolve` 显式产出 | 六节 | 升级没有下游判断依赖它,提前暴露成独立效果是没有消费者的预先设计。 |
