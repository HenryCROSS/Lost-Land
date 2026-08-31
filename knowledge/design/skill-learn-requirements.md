# 技能可学条件设计

**落地状态**：纯设计，无实现代码，无 `SkillRequirement`/`skill-requires!` 等任何新类型或函数已经写入 `crates/**`。

**冻结于** 2026-08-20，基线提交 `75b5e62`（工作区除本文件外干净）。

**并发声明**：本次任务与另外四路并行工作共享同一工作树——击杀计数（`ll-world`+`ll-sim`+`ll-content`）、错误变体拆分（`ll-script`+`ll-mod`）、本地化（新 i18n crate + `ll-game`）、载具设计（同期撰写 `knowledge/design/vehicle-and-mounting.md`）。本文档只新增这一个文件，不改 `README.md`（索引由协调者统一更新），不触碰 `crates/**`/`mods/**`/`assets/**`/`docs/**`/`.github/**`。

---

## 零、项目所有者的要求

> 「关于技能方面，我觉得有的技能可以是只能某个种族学习或者某个职业学习，也就是有一定自由度的调整可以被配置」

补充：

> 「其次我觉得还有文化类相关的技能」

---

## 一、现状核实

`SkillAttrs`/`SkillDef`（`crates/ll-mod/src/skill.rs:82-118`）目前只有一个相关字段：

```rust
pub owning_class: Option<ContentIndex>,
```

模块文档自己写明白了这个字段的边界（`skill.rs:85-88`）：「主职与副职共享同一份技能命名空间……这里因此只是一个分类/展示字段，不是命名空间隔离的边界」。**它从未被当成可学性闸门读取过**——核实了唯一的技能可用性判定点 `resolve_use_skill`（`crates/ll-sim/src/resolve.rs:684`），四道门分别是：

1. `agent.unlocked_skills.contains(&skill)`
2. 冷却比对 `world.clock`
3. `skills.skill(skill)` 能否在目录里查到
4. 资源是否充足

**没有一道门读 `owning_class`，也没有一道门比对 `agent.race`/`agent.profession`/`agent.subclasses`。** 载具设计文档（`vehicle-and-mounting.md` 一节）独立核实过同一件事：「全代码库没有第二处真实的技能可用性判定」。

更进一步：`unlocked_skills` 本身目前**只在测试夹具与验收 example 里被直接 `push`**（`crates/ll-sim/tests/skill_resolve.rs:222`/`521`、`crates/ll-sim/src/skill_overview.rs` 多处、`crates/ll-content/examples/p5_gameplay_acceptance.rs:266-267`）——全代码库不存在 `Intent::LearnSkill` 或任何名为 `resolve_learn_skill` 的函数。**「学习技能」这件事本身，在真实玩法路径里还不存在**，`SkillRequirement`（本文档要设计的东西）现在没有真实的接线点，是为未来这个缺失的 resolve 函数准备输入形状，这一点必须在文档里说清楚，不能让读者以为写完这份设计技能就会立刻被限制住。

已落地、可供本设计直接使用的数据源：

| 维度 | 位置 | 形状 |
|---|---|---|
| 种族 | `Agent.race` | `ContentIndex`，单值，`RaceTable::get` 可查 |
| 主职 | `Agent.profession` | `ContentIndex`，单值，`ClassTable::get` 可查 |
| 副职 | `Agent.subclasses` | `Vec<ContentIndex>`，`SubclassTable::get` 可查 |

三张表（`RaceTable`/`ClassTable`/`SubclassTable`）均已落地，均遵循「未注册返回 `None`」的 ADR 0015 纪律。

---

## 二、判断：声明式，不是脚本谓词——理由改写为「可展示」，不是性能

**采纳**原判断（可学条件必须是声明式数据），但按项目所有者的要求，把理由收窄到只剩一条，并把「性能也扛得住」这件事单独写清楚，防止将来被人拿性能反过来论证脚本谓词也行。

### 调用频率核实

「可学」判定目前连唯一真实触发点都不存在（一节已核实）。可以确定的是：它未来的触发点只会是「玩家打开技能界面尝试学习/升级时」这一类操作——**不在** `resolve_use_skill` 的四道门里（那四道门每次施法都跑，是真正的热路径；但那是「可用」判定，不是「可学」判定，见五节的边界）。按现实游戏节奏估算，这是每个角色一次游玩会话里几十次量级的调用，不是每帧、也不是每格移动。

### 性能不是论据，即便按最悲观口径也扛得住

ADR 0016 的实测数据（`0012-steel-capability-surface-verification.md` 的原始测量，0016 引用）：`call_function_by_name_with_args` 越过 VM 边界均摊 **326ns**，`engine.run(源码字符串)` 整段求值 **327~400µs**。即便技能学习判定选择走三档脚本回调、且每次都是最贵的 `engine.run` 级别开销，几十次/小时量级的调用也远远够不上任何需要优化的量级——**三档脚本回调在这里确实负担得起**，这一点必须写明白。

### 因此选声明式的唯一理由

**是「可被展示」，不是性能。** 界面要能把「为什么学不了」渲染成「需要：精灵族、盗贼」这样的句子——脚本谓词只产出一个 `bool`，没有任何可供 UI 遍历、翻译、拼句子的结构。这条理由与调用频率无关：即使频率再高一个数量级，脚本谓词依然无法被展示；反过来，即使调用频率高到需要优化，只要理由仍然是「可被展示」，性能都不构成反驳它的论据，因为性能从来不是这里的决定因素。**这条区分必须写下来**——否则将来有人核实出「其实三档回调很便宜」之后，会误以为这条结论因此站不住，实际上它站得住的原因从头到尾都不是性能。

---

## 三、条件的种类

### 现在就做的最小集合：种族、主职、副职

三种，对应项目所有者原话里明确提到的两/三个词。不多加。

### 数据形状：一个结构体，字段内取「或」

```rust
/// 技能的可学条件。每个字段内部取"或"、字段之间取"且"（见四节）。
/// 空列表表示该维度不设限制——与 `SkillDef.prerequisites` 空列表表示
/// 无前置同一个既有约定，不是这里另起的新规则。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillRequirement {
    /// 允许学习的种族；空表示不限种族。
    pub races: Vec<ContentIndex>,
    /// 允许学习的主职；空表示不限主职。取代 `owning_class` 一直想表达
    /// 却从未真正执行的"谁能学"语义——见一节，`owning_class` 本身
    /// 保留，继续只做分类/展示（见六节的兼容处理）。
    pub classes: Vec<ContentIndex>,
    /// 允许学习的副职；空表示不限副职。
    pub subclasses: Vec<ContentIndex>,
}
```

判定（未来 `resolve_learn_skill` 内部逻辑，本文档只给形状，不写实现）：

```rust
fn requirement_satisfied(
    req: &SkillRequirement,
    race: ContentIndex,
    profession: ContentIndex,
    subclasses: &[ContentIndex],
) -> bool {
    (req.races.is_empty() || req.races.contains(&race))
        && (req.classes.is_empty() || req.classes.contains(&profession))
        && (req.subclasses.is_empty()
            || req.subclasses.iter().any(|s| subclasses.contains(s)))
}
```

### 将来扩展的口子——以及要不要改存档格式

属性阈值（如「力量 ≥ 15」）、已完成任务（`QuestTable` 已有 DAG，`completed_quests: Vec<ContentIndex>` 同一种「或列表」形状即可接）、势力归属（走 `Affiliation` 通道）——都可以用**新增一个字段**的方式挂到 `SkillRequirement` 上，不需要改现有三个字段，不需要引入 tagged enum 变体匹配。

**不改存档格式。** `SkillRequirement` 是内容注册表数据（`SkillTable` 的一部分），不是 `Agent` 运行期状态——核实过 `crates/ll-content/src/save_file.rs` 不序列化任何内容注册表（`SkillTable`/`ClassTable`/`RaceTable` 均未出现），只序列化世界/实体状态。它在每次装载时由 mod 脚本重新声明，增删字段是纯 Rust 结构体变更，不触发 P5 已冻结的存档 schema/迁移链。真正可能牵动存档的是「新增条件所依赖的 `Agent` 侧运行期字段」本身是否已经存在（例如属性阈值依赖的六维属性已经落地；任务完成记录是否已经落地需要单独核实）——但那是「这个字段何时落地」的既有问题，不是「多加一种技能条件」本身引入的新存档负担。

---

## 四、条件之间的组合

**结论：字段内取「或」、字段之间取「且」，不是任意布尔组合。**

- 「精灵或矮人」→ `races: [elf, dwarf]`（同一字段内的列表就是「或」）。
- 「盗贼且等级 5」→ `classes: [rogue]`，「等级 5」属于三节说的属性阈值扩展位——本次最小集合不含它，但结构上已经留了「加一个新字段」的口子，不需要动 `races`/`classes`/`subclasses` 三者已有的语义。

**判据是能不能表达真实需求，不是形式完备**：目前提出的全部例子都是「同一维度内多选一」或「跨维度同时满足」，没有出现过需要跨字段嵌套或（例如「某职业 或 (某种族 且 另一职业)」）的真实案例。真做成通用布尔表达式树，UI 展示也要跟着写一个表达式渲染器，而不是「资格清单」这种最直观的形式——字段间且、字段内或已经能表达全部已知需求，且渲染成「需要：X 或 Y、Z」这样的文案是平凡的字符串拼接。若将来真的出现需要跨字段或的例子，再扩展，不预先设计。

---

## 五、「可学」与「可用」是两件事

一节已核实：全代码库唯一的技能可用性判定是 `resolve_use_skill` 的四道门，门一只读 `agent.unlocked_skills.contains(&skill)`——不读 `SkillRequirement`，不读 `agent.race`/`profession`/`subclasses`。也核实了「学习」这个动作本身目前没有真实玩法路径，`unlocked_skills` 只被测试/验收代码直接赋值。

**明确边界**：

- **「可学」** = `SkillRequirement` 只在「把技能索引写进 `unlocked_skills` 之前」被判定一次——未来的 `resolve_learn_skill`（名字待定，本文档不裁定）读 `SkillRequirement`，不满足则不产出任何效果（与 `resolve_use_skill` 四道门「静默作废」同一条纪律），满足则产出一个把技能索引写进 `unlocked_skills` 的效果。
- **「可用」** = `resolve_use_skill` 门一，只做集合成员判断，**不重新验证** `SkillRequirement`。一旦技能索引进了 `unlocked_skills`，它是怎么进去的（正常学习路径、还是别的什么写入）与「能不能用」无关。
- **载具授予的技能天然绕开这一整套闸门**：`vehicle-and-mounting.md` 六节已经裁定「有效技能 = 已学会的 ∪ 当前载具授予的」，用 `skill_source` 函数派生——`granted_skills`（载具定义里的字段）**从未写进 `unlocked_skills`**，因此从未经过「可学」这一关。操作弩车靠的是车本身的能力，不是操作者的种族天赋；这条边界与本设计完全兼容，`resolve_use_skill` 门一未来若按载具文档改成 `skill_source(...) != SkillSource::None`，`SkillRequirement` 不需要跟着改一个字符——它只管「进 `unlocked_skills` 之前的那一次判定」，载具那条支路从设计上就没有经过这道门。

一句话供后续读者引用：**「可学」是写入 `unlocked_skills` 前的一次性闸门；「可用」是每次施法都做的集合成员判断；载具（以及未来其它临时效果）授予的技能直接进入「可用」判断的输入集合，不经过闸门。**

---

## 六、mod 可配置

`register-skill` 现有签名（10 个位置参数，见 `crates/ll-mod/src/script_skill_api.rs:81-92`）**保持不变**——`mods/example_mod/gameplay.scm:20` 是真实存在的调用：

```scheme
(register-skill "examplemod:frostbolt" "" (list) 25 "mana" 12 "deal-damage" "" 15 0)
```

若改 `register-skill` 的参数个数，这行脚本直接编译失败。`owning-class` 位置参数现有的「分类/展示」语义也不需要改变。

### 新签名：新增一个独立函数，不与 `register-skill` 合并

```
(skill-requires! id races classes subclasses)
```

- `id`：技能完整命名空间标识符字符串，**必须是已经调用过 `register-skill` 注册过的技能**（否则报错——与其余 `register-*` 函数「不能对不存在的内容声明属性」的既有纪律一致，不是新规则）。
- `races`/`classes`/`subclasses`：字符串列表，元素是完整命名空间标识符；空列表 `(list)` 表示该维度不限制。
- 返回 `Result<bool, String>`，理由同其余 `register-*`/`skill-requires!` 类函数——错误在装载期报出来，不静默吞掉。

### 与 `owning_class` 的兼容处理

`owning_class` 保留，继续只做分类/展示，**不作为闸门读取**。若 mod 作者只调用 `register-skill`、不调用 `skill-requires!`，该技能的 `SkillRequirement` 三个列表全空，等价于「不限种族/职业/副职」——与今天的行为（完全不限制，因为压根没有判定）完全一致，**老脚本零改动、行为不变**。

若 mod 作者两者都想表达（「这个技能主要属于战士，而且只有战士能学」），需要显式调两个函数：`register-skill` 传 `owning-class="lostland:warrior"`（展示），再调 `skill-requires!` 传 `classes=(list "lostland:warrior")`（强制）。**两者不会自动同步，这是有意的**：分类展示与强制闸门是两件独立的事（一节已引用 `SkillDef` 自己的文档说清楚 `owning_class` 不是边界），自动同步只会在某天有人想「分类但不强制」或「强制但不专属展示」时变成一个隐藏的意外耦合。

### 示例（设计阶段示例代码块，不修改 `mods/**`）

```scheme
(register-skill "yourmod:elven_blessing" "" (list) 15 "mana" 8
                 "temporary-stat-modifier" "willpower" 3 20)
(skill-requires! "yourmod:elven_blessing"
                  (list "lostland:elf")   ; races：只有精灵能学
                  (list)                  ; classes：不限主职
                  (list))                 ; subclasses：不限副职
```

---

## 七、缺失引用怎么办

核实早先审计结论成立：`Registry::intern` 对任意 `NamespacedId` 都会分配一个索引，不检查这个索引背后有没有对应的 `RaceTable`/`ClassTable`/`SubclassTable` 定义。`skill-requires!` 若直接对 `races`/`classes`/`subclasses` 字符串调 `registry.intern`，一个指向未装载 mod 的种族 id 会被无声接受，产出一个永远查不到 `RaceTable::get` 的索引，该技能因此永远学不到，且没有任何报错。

**结论：不在 `skill-requires!` 内部另起一套校验机制**，复用 `mod-lifecycle-and-event-api.md`「缺口 A」一节已经设计好的 `require-content!`（该文档给出的既有方案，本身也尚未实现，但已经是「装载期显式校验引用是否真实存在」的标准答案，不需要为 `skill-requires!` 重新发明一个）。`skill-requires!` 的文档应当明确**建议**（不强制——理由与 `require-content!` 本身「是否值得为每个引用做校验是 mod 作者的判断，不能强制」一致，见该文档「为什么必须是脚本函数」一节）mod 作者在引用外部种族/职业/副职时配一条：

```scheme
(require-content! "elfmod:elf" 'race)
```

**为什么不强制**：`require-content!` 的设计取舍已经论证过这条不是强制项，`skill-requires!` 没有理由对这一条引用另立一套比其余全部 `register-*` 函数都更严格的规则——那会打破 mod API 内部一致性。真要收紧，应该回去改 `require-content!` 本身的定位（例如让某些「打了标签」的注册函数自动隐式声明），那是另一个更大的决定，不属于本文档范围。

与「文化」追加要求的关系见八节——那里出现的是一个 `require-content!` **解决不了**的更深问题。

---

## 八、追加：文化条件

项目所有者补充：某些技能该限定文化背景。

> **【2026-08-30 复核：下面这条核实结论已过期，正文原样保留。】** **文化表存在，只是不在这个 crate、也不叫这个名字**：`crates/ll-world/src/culture.rs:73`/`:99`/`:250` （三件套 `CultureKind`/`CultureAttrs`/`CultureTable`，走 `TerrainKind`/`TerrainAttrs`/`TerrainTable` 的既有形状），内容侧 `mods/lostland/cultures.json5`。落地时间是 2026-08-27 文化批次（提交 `4aec07e`）。「`crates/ll-mod/src/` 下没有 `culture.rs`」这句话字面上仍然为真，**但由它推出的结论是错的**。逐条见 [2026-08-29 文档—代码一致性审计](../audit/2026-08-29-doc-code-audit.md) 一节第 1 条。

**核实协调者的判断：成立。** `CultureDef`/`CultureTable` 完全不存在于代码——`crates/ll-mod/src/` 目录下没有 `culture.rs`，没有 `register-culture`，没有任何文化注册表。`society-and-affiliation.md`「落地状态」一节明确写「完整 `CultureDef` 未落地」，实现阶段 P9。

但这里有一个比「缺失引用」更深的问题，必须正面处理：**即使某个具体文化 id 被正确注册（`require-content!` 能保证的那一层），当前代码库里没有任何一行代码会把 `AffiliationKind::Culture` 的 `Affiliation` 写到任何 `Agent` 身上**——世界生成、角色创建流程都不产出文化归属（P9 之前它就是一个从未被赋值过的字段）。这意味着即便 `skill-requires!` 现在就支持文化条件、且引用的文化 id 完全合法，该技能依然会对**当前存在的每一个 `Agent`**永远不满足——不是「某个特定 mod 没装」这种可修复的配置错误，而是「支撑这个判断的运行期数据源，系统性地、对全部实体都不存在」。`require-content!` 校验的是「这个 id 被谁注册过」，不校验、也不可能校验「未来会不会有 `Agent` 真的拿到这个归属」——两者是不同层面的问题，**乙选项（现在加、靠 `require-content!` 强制校验）不能覆盖后者**。

**结论：选甲。** 现在不加文化条件种类，只在本文档留下形状（见下），等 P9 `CultureTable`/文化归属产出路径真正落地后再加。

**明确写：文化系统尚不存在**——`CultureDef` 未落地，没有任何代码路径会给 `Agent` 写入 `Culture` 归属。现在加这个条件种类，写出来也是一句谁都学不到的装饰，是本项目已点名过的「声明了但从没接线」问题的又一次重演，不该在核实过后还明知故犯。

留的口子（不实现，只记形状，供 P9 之后参考）：

```rust
pub struct SkillRequirement {
    pub races: Vec<ContentIndex>,
    pub classes: Vec<ContentIndex>,
    pub subclasses: Vec<ContentIndex>,
    // P9 之后，文化系统落地（CultureTable 存在、且有代码路径真正往
    // Agent.affiliations 写入 AffiliationKind::Culture）才加这一个字段：
    // pub cultures: Vec<ContentIndex>,
    // 判定读 agent.affiliations（过滤 kind == Culture），而不是像
    // races 那样读一个专属标量字段——理由见九节「文化与种族不同层」。
}
```

这不影响本设计其余部分：`races`/`classes`/`subclasses` 三个字段现在就能实现、现在就有真实的判定数据源（`RaceTable`/`ClassTable`/`SubclassTable` + `Agent.race`/`profession`/`subclasses`，全部已落地）。文化只是暂缓，不拖累其余三种条件今天就能用。

---

## 九、文化与种族是不是同一层

**核实：不是同一层。**

- **种族**：`Agent.race: ContentIndex` 是专属字段，单值，先天，不变——判定是一次字段相等比较（`agent.race == 候选之一`）。
- **文化**（若 P9 后落地）：不会是专属字段，而是 `Agent.affiliations: Vec<Affiliation>` 里 `kind == AffiliationKind::Culture` 的一条——`AffiliationKind::Culture` 与 `OrgRef::Def(ContentIndex)` 已经落地（`crates/ll-world/src/entity/affiliation.rs:25-27`/`:47-50`），但走的是通用归属列表，不是专属字段。判定是「在一个通用列表里按 `kind`+`org` 组合查找是否存在」，不是字段相等；存储形状（`Vec`）结构上不禁止一个实体同时有多条文化归属，虽然 `society-and-affiliation.md` 的文字描述「几乎不可改」暗示实践上通常唯一——但这是内容设计约定，不是类型系统保证的不变式。

按 **ADR 0021** 的判据（「抽象的理由是有算法要共享，不是看起来该对称」）：这两种判定**没有共享算法**——一个是对专属标量字段的相等比较，另一个要先按 `kind` 过滤通用 `Vec<Affiliation>` 再比对 `org`，读的是完全不同的底层存储。**不应该为了「都是身份标签」就把 races/cultures 塞进同一个「identity 匹配」抽象里**——那会重复 0021 否决过的错误（`Camera`/`BoundedCamera` 那次：为了表面对称插入一层没有实际内容的接口）。未来的 `cultures` 字段应当独立存在、独立实现判定逻辑，不与 `races` 共用泛型代码。

这一条也直接回应了协调者的疑虑：两者在条件系统里的表达确实不应该一样，理由不是「看起来不一样」这种审美直觉，是两者背后的数据形状本来就不共享任何一段可复用算法——这正是 0021 要求的判据本身，不是套用「都是身份标签所以应该对称」这种更省事但错误的推理。

---

## 相关文档

- `crates/ll-mod/src/skill.rs`、`class.rs`、`race.rs`、`subclass.rs`、`script_skill_api.rs`
- `crates/ll-sim/src/resolve.rs`（`resolve_use_skill`）
- `crates/ll-world/src/entity/affiliation.rs`（`AffiliationKind`/`OrgRef`）
- `knowledge/design/class-skill-quest-system.md`、`race-system.md`、`society-and-affiliation.md`、`mod-lifecycle-and-event-api.md`（`require-content!`）、`vehicle-and-mounting.md`（载具技能授予，另一位代理同期撰写，本文档未修改该文件）
- `knowledge/decisions/0016-mod-performance-tiers-by-declaration.md`、`0017-tiered-declarations-materialize-columnar.md`、`0018-engine-layer-vs-gameplay-layer-scripting-boundary.md`、`0021-abstraction-requires-shared-algorithm-not-symmetry.md`
