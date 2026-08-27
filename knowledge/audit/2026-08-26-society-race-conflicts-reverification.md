# 社会 / 种族 / 冲突三份设计文档的落地状态复核

**复核日期**：2026-08-26
**复核范围**：[`design/society-and-affiliation.md`](../design/society-and-affiliation.md)、[`design/race-system.md`](../design/race-system.md)、[`design/conflicts.md`](../design/conflicts.md)——三份均冻结于 2026-08-17。
**基线**：`main`，HEAD `a9f6691`（`Merge branch 'wt-npc'`）。
**并发声明**：复核时主工作树、`../wt-furniture`、`../wt-cleanup` 各有代理正在改 `crates/**`，工作区存在未提交改动（`ll-mod/src/content_*.rs`、`ll-world/src/resource.rs`、`mods/lostland/{classes,resources}.json5` 等）。**本文档的一切结论以已提交树 `a9f6691` 为准**，未读任何未提交的半成品；读者在更晚的提交上复核请重跑 grep，不要相信本文档的行号。
**方法**：逐节读原文，每一条「落地状态」类断言都去 `crates/**`、`mods/**` grep 或读码核实。只读，不改代码。

**这份文档要干两件事**：① 说清三份文档今天哪些节仍然成立、哪些前提已经消失；② 直接充当下一批「`CultureDef` 完整落地 + 关系派生基线」的任务书（见六、七两节）。

---

## 零、三条把大半个「落地状态」推翻的事实

1. **脚本系统整个拆除了**（ADR [0028](../decisions/0028-steel-engine-construction-memory-corruption.md)）。核实：全仓库无 `crates/ll-script/`、无 `*.scm` 文件、`Cargo.toml` 无 `steel` 依赖。内容改走 `mods/<id>/*.json5`，玩法行为是引擎里的 Rust（`ll_mod::native_behavior`）。凡「通过 Steel 脚本定义」「`register-*`」「跨脚本边界 326ns」的段落**一律失效，但只是载体失效，结论不失效**。
2. **据点 / 资源点 / NPC 名册这三层已经真的落地并互相咬合**，且都是**纯派生**：`ll-world/src/chronicle.rs`（12 纪元历史推演）→ `ll-world/src/settlement.rs`（`SettlementSite` + 写进地形）→ `ll-world/src/resource.rs`（资源点与领地勘察）→ `ll-mod/src/roster.rs`（名册派生 + 建立者种族）。三份设计文档写作时这一层一处都不存在，所以凡是谈「聚落 / 资源 / 种族分布」的段落**都是在一片空地上写的，现在地上有东西了**。
3. **`Affiliation` 在真实游戏里恒为空。** 生产路径上唯二构造 `Agent` 的地方——`ll_game::world::build_player_agent`（`world.rs:541`）与 `ll_mod::roster::build_npc_agent`（`roster.rs:747`）——都写死 `affiliations: Vec::new()`、`wallet: 0`。`ThinPopulation::spawn` 在生产代码里一次都没被调用（只有测试）。**整套归属体系今天是一具没有生产者的空壳**，详见五节。

---

## 一、`society-and-affiliation.md` 逐节复核

| 节 | 结论 | 证据 |
|---|---|---|
| **落地状态**（开头段） | **三处过期**：① `Affiliation.org` 的类型**已经变了**，不再是 `ContentIndex`；② `StructureKind` 不是「未落地」而是**已被明确否决**；③ 说 `Kinship`/`Traits`/关系基线/宗教戒律/LOD 聚合「未落地」——**这四条仍然准确**。 | `ll-world/src/entity/affiliation.rs`；`ll-world/src/settlement.rs:17`；全仓库 `Kinship`/`struct Traits`/`Tenet` 零命中 |
| **一、四类是同一个结构** | **结论仍然成立，形状有一处必须记录的改动。** 六个 `AffiliationKind` 变体一字未改。但 `org` 字段从 `ContentIndex` 换成了 `OrgRef`（两变体：`Def(ContentIndex)` / `Instance(WorldId)`）——`Culture`/`Profession` 恒走 `Def`，`Faction`/`Religion`/`Guild`/`Family` 恒走 `Instance`，判据是 [`identity-and-ids.md`](../design/identity-and-ids.md) 的「mod 定义种类，世界生成造个体」。**文档第一节那段 `Affiliation` 代码已经不是代码里的样子。** | `ll-world/src/entity/affiliation.rs:36-77` |
| 一、「直接回报」五条 | **五条全部仍是设计，一条都没接线。** 偷窃判定、交易价格、任务接取权、行为树条件、战争结盟——`Owner::Faction` 无消费者、无定价公式（全仓库无 `Caravan`/`Task`/「本地价」）、行为树里没有任何归属查询。唯一沾边的是 `ll_sim::ai_query::is_hostile`，见五节。 | `Caravan`/`struct Task` 零命中 |
| **二、文化是一组权重** | **设计仍然成立，且现在有了它要驱动的东西——但 `CultureDef` 依然完全不存在。** 全仓库 `CultureDef` 只出现在 `naming.rs:35` 的一句注释里。六个字段中只有 `naming` 落地成独立类型 `NamingRules`。三种文化选址对照表所描述的效果，今天由**资源画像**（不是文化）承担，见四之二。 | `CultureDef`/`EconomyKind`/`SocialStructure`：仅注释命中 |
| **三、地图结构层 `StructureKind`** | **整节前提消失。** `StructureKind` 不是「还没做」，是**被 ADR 0021 判据明确否决**：墙/门/窗/地板已经全部是地形（`BaseTerrainIds`），地形层已经有按格存储、FOV 遮挡、寻路代价、存档 remap、内容哈希五样，另起一个类型要把这五样重写一遍换零新增能力。[`settlements-structures-and-npc-spawning.md`](../design/settlements-structures-and-npc-spawning.md) 十节已把本文档三节那份草图点名为「唯一存在过的地方」。 | `ll-world/src/settlement.rs:17-25`、`ll-world/src/resource.rs:24` |
| 三、「道路是经济的骨架」 | **仍是纯设计，且现在有了一个替代它的已落地机制。** 无 `Road`、无最小生成树、无商队。「城市自然分化出大小」这件事**已经由别的路径实现了**：承载力 = 领地里资源点数 × `residents_supported`，人口按比例增长撞承载力封顶——实测三个种子 788 座据点，平均建筑数从 3.3 涨到 20.2~21.6，人口中位数 31~33、最大 159~175。**正反馈的来源是资源不是道路。** | `ll-world/src/settlement.rs` 模块文档「实测」一节；`chronicle.rs` 的 `GROWTH_RATE_DIVISOR` |
| 三、「资源点决定聚落的经济性格」 | **这一句已经兑现了，但兑现方式与原文写的不同。** 原文是「文化偏好 × 周边资源点」；落地的是**纯资源点，没有文化这一项**：`SettlementSite::resource_profile` 取领地里 `资源点数 × settlement_draw` 最高的两种，`ll_mod::roster` 按它调职业与建立者种族的权重。**「性格是涌现的」成立；「文化偏好」那一半是空的**——这正是下一批要补的那一半。 | `ll-world/src/settlement.rs:222-252`、`ll-mod/src/roster.rs` 的 `commoner_weights`/`race_weights` |
| **四、宗教如何参与世界运转** | **整节仍是纯设计，一处未动**（无 `Tenet`、无神殿、无信仰值、无先知）。「戒律通过 Steel 脚本定义，与技能效果走同一套注册通道」这句**载体失效**：今天的对应写法是「戒律是 `mods/<id>/*.json5` 的一份数据表，判定在引擎里用 Rust 写」。**其余论证（戒律是可查询条件、神殿是经济节点、宗教关系独立于政治边界、信仰强度是廉价标量、传播机制全部复用已有系统、新宗教要能在游玩中诞生）全部原样成立**，一条都不需要改写。 | `Tenet`/神殿/`piety` 零命中 |
| **四之二、家族与代际** | **仍是纯设计。** `Kinship` 不存在。`FamilyId` 存在（`ll-world/src/entity/id.rs`），但只作为 `ThinPopulation.family` 的一列，而**薄层在生产里从未被写入过**——`ThinPopulation::spawn` 全部调用点都在测试。「名字是纯函数、家族姓氏同理」这条**已经落地**：`ll_world::naming::{given_name, surname}` 就是按 `(rules, seed, entity/family)` 现算，实现与原文逐字一致。 | `ll-world/src/naming.rs`；`population.spawn` 仅测试命中 |
| **四之三、关系派生基线** | **一行都没有落地**，且**缺的不只是这条公式**，见四之一节的完整前置清单。今天唯一站在这个位置上的是 `ll_sim::ai_query::is_hostile`——「A 与 B 有没有共同 `Faction`；A 一个势力都没有就视为对谁都敌对」。它自己的模块文档已经写明「真正的声望/关系矩阵是 `society-and-affiliation.md` 描述的范围」。**因为生产里所有 `affiliations` 恒空，它今天恒返回 `true`。** | `ll-sim/src/ai_query.rs:32-35,134-146` |
| 四之三、职业参与关系派生 | **仍是纯设计。** `AffiliationKind::Profession` 从未被构造过；落地的是**另一条路**——`Agent.profession: ContentIndex` 单列 + `ThinPopulation.profession` 单列。[`settlements-structures-and-npc-spawning.md`](../design/settlements-structures-and-npc-spawning.md) 十二节 1 已把「职业到底有几个真相源」列为**待所有者裁决**，至今未决。熟练度（`standing`）没有任何存储位置。 | `ll-world/src/entity/agent.rs:124` |
| 四之三、职业声望是局部的 | **仍是纯设计**，无「职业 × 聚落」表。但它的**上游已经具备**：`SettlementSite::resource_profile` + `ResourceTable::category` 恰好就是「本地条件修正」需要的输入，`roster.rs` 的 `apply_affinity` 已经是这条公式的一个退化版（只有本地条件，没有文化基准、没有经济状态）。 | `ll-mod/src/roster.rs` 的 `apply_affinity` |
| 四之三、性格 `Traits` | **仍是纯设计**，`struct Traits` 全仓库零命中。「每个特质都接着一个已有机制」那张表里，今天真的存在的接口只有一个（`ll_sim::check` 的对抗判定，可接勇敢/贪婪），其余六项接口（商队、议价、转行、记忆槽、衰减、什一税）自身都不存在。**「加特质前必须先回答它接在哪个已有机制上」这条纪律现在比冻结时更重要，不是更不重要。** | — |
| 四之三、势力/宗教/文化关系的三种变化方式 | **仍然成立，原样引用，不需要改写。** 尤其「把文化关系做成动态的是个陷阱」这条判断没有任何证据反对它。 | — |
| **五、必须在 P3 就预留的字段** | **已兑现，且预留是对的。** `Agent` 上 `affiliations`/`wallet`/`profession` 三个字段确实在 `agent.rs:120-124`，并已进 `WorldState::hash`（`state.rs:1084`）与存档 remap（`ll-content/src/remap.rs` 的 `remap_affiliations`）。**这一节是三份文档里唯一被完整验证为「当时那么做省下了迁移链」的判断。** | `ll-world/src/entity/agent.rs:120-124` |
| **六、LOD 兼容性（被记住/被模拟两轴）** | **仍然成立，且已被一次真实落地间接验证。** `ll_mod::roster` 的做法正是这条轴：一座玩家没走近过的村子，人口、职业、种族随时可现算（**被记住**，零存储），只有真的被物化过的据点才进 `WorldState::actors`（**被模拟**，`materialized_settlements` 记的就是这条边界）。距离确实只是「谁当前该被模拟」的一个近似信号。 | `ll-mod/src/roster.rs` 模块文档一、二节；`ll-world/src/state.rs:420-450` |

---

## 二、`race-system.md` 逐节复核

| 节 | 结论 | 证据 |
|---|---|---|
| **落地状态**（开头段） | **已经严重过期：`RaceDef` 落地了。** 原文列的「均未落地」清单里，`RaceDef` 注册表、暗视接口 `sight_radius_at`、体型 `footprint`、寿命字段**四项都已落地**；只有混血注册、`race_affinity`、`birth_settlement` 列仍未落地。[`trait-system.md`](../design/trait-system.md) 一节已经如实更正过这一点（「与 `race-system.md`「落地状态」一节的旧记录不一致」），但没有回头改这份文档。 | `ll-mod/src/race.rs:114`；`ll-world/src/light.rs` |
| **一、种族不是 `AffiliationKind`** | **完全成立，且已被落地实现证实**：`Agent.race: ContentIndex`、`ThinPopulation.race: Vec<ContentIndex>` 都是分类标签，没有 `standing` 这个维度。`RaceDef` 的实际字段是 `id` / `display_name_key` / `stat_modifiers` / `darkvision_cells` / `footprint` / `lifespan_years` / `xp_reward` / `traits` / `starting_items`——草图里的六项全在，另多出三项（后三项是后续批次加的）。**唯一偏离**：`name_key: String` 落成 `display_name_key: NamespacedId`，理由是本地化惯例，已在 `race.rs` 模块文档里说明。 | `ll-mod/src/race.rs:114-190` |
| **二、属性修正烘焙进 `BaseStats`** | **完全成立且已落地**：`ll_sim::character::bake_race_stat_modifiers` 在两条 `Agent` 构造路径上各调一次，创建时一次性叠加。`Effect::ChangeRace` 仍不存在（本节只把它作为「将来怎么做」写下，不是断言它存在）。 | `ll-game/src/world.rs:535`、`ll-mod/src/roster.rs` 的 `build_npc_agent` |
| **三、固定加减不用千分比** | **完全成立且已落地**（`stat_modifiers: BaseStats`，存的是增量）。「调整值死区可能吃掉种族修正」那条数值警告仍然有效，且**数值定稿仍未做过**。 | `ll-mod/src/race.rs` 模块文档「属性修正」一节 |
| **四、时间轴只改数值预算** | **仍然成立**，是一条纯数值预算判断，代码侧无对应物可核实（也不需要）。 | — |
| **五、暗视** | **本节已经是更正后的版本，与代码一致**：`sight_radius_at(base_radius, light, darkvision_cells)` 逐字符合。**唯一失效的是一处证据引用**——「已发货证据：`mods/example_mod/gameplay.scm` 的 `examplemod:ooze`」，那个文件不存在了；等价证据现在是 `mods/example_mod/races.json5` 里的 `examplemod:ooze`。对称性论证、「暗视不可做成无视黑暗」两段**全部原样成立**。 | `ll-world/src/light.rs`；`mods/example_mod/races.json5` |
| **六、体型只做三样** | **字段落地了（`footprint: (u8,u8)`），但三样能力一样都没接线**：无多格碰撞、无体型移动代价、`footprint` 全仓库无消费者（只在 `race.rs`/`content_hash.rs`/`content_audit.rs` 命中，也就是「定义它、哈希它、审计它」）。十二节把「2×2 碰撞与寻路是否已支持」列为待验证——**核实结论：不支持，且没有任何代码读这个字段。** 这是一条真实的「声明了但没接线」。 | `footprint` 三处命中全部非消费 |
| **七、寿命三条平衡手段** | **仍是纯设计**：`lifespan_years` 字段在，但无年龄、无衰老死亡、无熟练度曲线、无继承。三条论证本身没有过期。 | — |
| **八、存储：薄层零列 + `birth_settlement`** | **实现债务框仍然准确，但它的成本比框里写的低得多。** `ThinPopulation.race` 确实还是显式存储列、`promote()` 原样复制、没有 `birth_settlement`。**但薄层在生产里从来没有被写入过一次**——`ThinPopulation::spawn` 的全部调用点都在测试里。所以「需要一次迁移」在今天不是迁移，是**改一个还没有任何真实数据的类型**。**这条债务现在还的代价接近零，往后每拖一批只会更贵。** | `ll-world/src/entity/thin.rs:42-59,112-126` |
| **九、混血只在厚层、深度 1** | **仍是纯设计**，无 `Kinship` 就谈不上混血。论证本身没有过期。内容侧已有一条混血条目（`examplemod:half_elf`），但它只是一条普通 `RaceDef`，与本节的「出生时现算一次」路径无关。 | `mods/example_mod/races.json5` |
| **十、种族偏见挂在 `CultureDef` 上** | **前提整个悬空**：`race_affinity` 要挂的那个 `CultureDef` 不存在，全仓库 `race_affinity` 零命中。**但本节最重要的那条硬约束（种族绝不可成为职业声望表的第三个维度、必须乘法分解）今天变得更要紧了**——因为职业分布已经真的在按资源算了（`roster.rs` 的 `commoner_weights`），下一批往里加种族维度时正好会撞上这条。 | — |
| **十、交叉歧视需要显式挂钩** | **仍然成立**，且指向的那个挂钩点（职业审计）本身也不存在。 | — |
| **十一、美术成本** | **仍然成立**，且渲染层至今没有按种族分裂过资源，约束事实上被遵守着。 | — |
| **十二、待验证项** | ① **已可结案：不支持**（见六节行）。② 大型精灵跨接缝贴图位移**仍未验证**，且因为 `footprint` 无消费者，暂时不可能触发。 | — |
| **本文档有没有覆盖「怪物种族」** | **没有。见四之三节，这是一个整节级别的空白，不是一句话的遗漏。** | — |

---

## 三、`conflicts.md` 逐条复核

| 条目 | 今天的状态 |
|---|---|
| 1. LOD 术语（已裁定） | **裁定仍然成立，且已被 `roster` 的落地间接验证**（见一节末行）。无需改动。 |
| 2. 行会定价的买家归属系数（已裁定） | **裁定仍然成立，但两侧都不存在**：无定价公式、无 `Affiliation` 生产者。这是一条「等经济系统落地才谈得上」的裁定。 |
| 3. 受限转行把熟练度算进阈值（已裁定） | 同上，**裁定成立、两侧都不存在**。熟练度今天没有存储位置（`AffiliationKind::Profession` 从未被构造）。 |
| 4. 「名望」vs「职业声望」改名区分（已裁定） | **裁定成立**，两个量今天都不存在，但命名冲突确实已经解除。 |
| **5. `StatBonus` 从未被定义（设计缺口）** | **已闭合。** `ll_sim::item::{StatBonus, StatTarget}` 已落地并进内容哈希。可以标记为已解决。 |
| **6. `EquipSlot` 从未被正式定义（设计缺口）** | **已闭合，但落地形状与清单的猜测不同**：`ll_world::item::EquipSlot(u8)` 是**位下标新类型**，不是 22 变体的 `enum`——理由是 `SlotMask` 给 mod 预留的 10 个高位无法用 Rust `enum` 表达。清单里「读者只能自行脑补它就是 22 个变体」那句猜测**是错的**，实际选择比它更好。 |
| **7. 品质价格倍率如何进入基础价（设计缺口）** | **仍然开着，且比记录时更空**：`ItemAttrs::base_price: Milli` 存在，但没有任何定价公式读它，品质倍率表也没有落地。 |
| 8. 「默认派生」复用编号不连续（记录备查） | 仍然只是编号问题，且 `README.md` 已改为直接罗列实例。**但现在可以补记：`chronicle`/`settlement`/`roster` 三层是这条原则迄今最大的一次复用**（整个世界史、据点、名册都不进存档）。 |
| 9. 千分比用 `Milli` 还是裸 `i32`（记录备查） | **仍然如此，且分裂扩大了**：`ResourceAttrs::abundance`、`roster::OUTSIDER_PERMILLE` 等新落地的千分比字段一律是裸 `u32`，`Milli` 只用在价格/重量上。不影响确定性，仍不要求现在改。 |
| **清单本身的范围声明** | 仍然准确（固定在前五份文档），但**清单没有覆盖后来出现的最大一处不符**——`society-and-affiliation.md` 三节的 `StructureKind` 被 `settlements-structures-and-npc-spawning.md` 十节点名否决。那份文档的十节事实上是 `conflicts.md` 的续篇。 |

---

## 四、四个特别问题

### 四之一、「关系派生基线」设计成什么样？今天缺什么才能实现它

**设计（原样引用，不改写）**：三层结构——组织↔组织用**稠密矩阵**（几十到几百个组织，一万条随便存）；个体→组织就是 `Affiliation`，无新结构；个体↔个体**默认不存，由上两层派生**。派生基线是各类归属贡献之和（同家族 +80、同势力 +20 / 敌对势力 −60、同宗教 +25 / 异端 −40、同行会 +15、同文化 +10 / 异文化 −5，加职业项：同职业 −10、互补 +15、对立 −50），实际关系 = 派生基线 + **定容记忆槽**（建议 8 格 LRU）里的个体记忆偏移。种族偏见由 `CultureDef.race_affinity` 以**查表项**形式加进基线（[`race-system.md`](../design/race-system.md) 十节），保持不对称。

**今天缺的，按「缺的是地基还是缺的是公式」分成两类：**

**A. 缺地基（这些不做，公式无处可写）**

| 缺什么 | 现状 | 为什么是硬前置 |
|---|---|---|
| **组织实例本身** | `OrgInstance` 类型已落地（`ll-world/src/entity/org.rs`，82 行），但**全仓库零构造点**——只有一行 `pub use` 和 `state.rs:332` 一句「未来 `OrgInstance` 等」的注释。世界生成不造任何势力/宗教/行会/家族。 | 三层结构的第一层是空的。没有组织就没有「组织↔组织矩阵」，也没有 `OrgRef::Instance` 可指。 |
| **`Affiliation` 的生产者** | 两条 `Agent` 生产路径都写死 `Vec::new()`。 | 第二层是空的。 |
| **`CultureDef`** | 完全不存在。 | 「同文化 +10 / 异文化 −5」与 `race_affinity` 两项无处取值。 |
| **组织↔组织矩阵的存储位置** | 不存在。`WorldState` 没有任何组织级容器。 | 这是唯一一处**必须进存档**的关系数据（势力关系事件驱动、会变），需要决定它挂在哪、进不进 `WorldState::hash`、怎么 remap。 |
| **记忆槽** | 不存在。 | 「实际关系 = 基线 + 偏移」的偏移那一半。 |
| **`Traits`** | 不存在。 | 相容度贡献与衰减速率两条路径都要它。**但它不是基线本身的前置**——基线可以先只做归属项。 |

**B. 缺公式（地基有了就是几十行整数运算）**

派生基线求和函数本身、职业关系表（同职业/互补/对立）、`is_hostile` 从「有没有共同 `Faction`」换成「基线是否低于阈值」。

**一句话**：**关系派生基线缺的不是算法，是「有组织可归属」这件事本身。** 下一批的真正工作量在世界生成期造出势力/文化实例，不在写那个求和函数。

### 四之二、完整 `CultureDef` 设计成什么样？现在只有 `naming` 落地，其余是什么

**设计（原样引用）**：

| 字段 | 语义 | 今天 |
|---|---|---|
| `id` / `name_key` | 注册表标识 + 本地化键 | 无（惯例上应落成 `display_name_key: NamespacedId`，与 `RaceDef` 一致） |
| `building_materials: Vec<(ContentIndex, i32)>` | 建筑材料偏好，千分比权重 | **无。今天是硬编码**：`stamp_settlement` 有人住的一律 `ids.wall_wood`、废墟一律 `ids.wall_stone`。这是 `building_materials` 唯一一个已经存在的、等着被替换的落点。 |
| `site_terrain: Vec<(TerrainKind, i32)>` | 选址地形偏好 | **无。今天选址完全不看文化**：`try_found` 的四条加分是「基础分 + 土地肥沃 + 世界人口压力 + 资源吸引力」，没有文化项。 |
| `economy_weights: Vec<(EconomyKind, i32)>` | 农耕/游牧/商贸/采矿/渔猎 | **无，`EconomyKind` 类型也不存在。** 今天的等价物是 `SettlementSite::resource_profile`——**它是「周边资源」那一半，`economy_weights` 是「文化偏好」那一半**，两者相乘才是原文说的「经济结构 = 文化偏好 × 周边资源点」。 |
| `naming: NamingRules` | 音素表 + 拼接规则 | **已落地**（`ll-world/src/naming.rs`），但**唯一消费者是 `ll-sim/examples/p3_acceptance` 这个 demo**，用的是写死的 `demo_naming_rules()`。生产路径（`roster`/`settlement`）**一次都没调用过它**——NPC 今天没有名字。 |
| `social_structure: SocialStructure` | 家族/氏族/城邦/部落 | **无，类型也不存在。** ← **这一项与部落复用 `SettlementSite` 直接相关，见四之四。** |
| `religion_affinity: Vec<(ContentIndex, i32)>` | 主流宗教倾向 | 无（宗教实例本身也不存在）。 |
| `race_affinity: Vec<(ContentIndex, i32)>` | 对各种族的态度，可正可负、**刻意不对称** | 无。由 [`race-system.md`](../design/race-system.md) 十节追加，不在原六字段里。 |
| （职业声望基准表） | 每个 `CultureDef` 带一张职业声望表 | 无。由 `society-and-affiliation.md` 四之三「文化基准」隐含要求，**而原结构体定义里没有这个字段**——这是一处设计文档自身的不完整，落地时要么补字段、要么明确它是另一张表。 |

**落地时必须先回答的一个架构问题**：`CultureDef` 是内容（`ll-mod`），而选址/建材/承载力全部发生在 `ll-world`。**已有先例可照抄**：`TerrainTable`、`ResourceTable` 都是**类型定义在 `ll-world`、由 `ll-mod` 的装载器填**，然后注入进世界生成。`CultureDef` 应当走同一条路，而不是把内容注册表倒灌进 `ll-world`。

### 四之三、`race-system.md` 有没有覆盖「怪物种族」？

**没有。这是一个整节级别的空白。** 逐条核实：

- 文档八节「存储」、十节「种族偏见」、十二节「待验证项」全部预设**种族出生在聚落里**（`birth_settlement` + 聚落种族权重表）。**没有任何一节谈过「不住在聚落里的种族」或「天生敌对的种族」。**
- `RaceDef` 的九个字段里**没有一个能表达「这个种族住哪儿、跟谁不对付」**：`stat_modifiers`/`darkvision_cells`/`footprint`/`lifespan_years`/`xp_reward`/`traits`/`starting_items` 全是个体数值，没有选址亲和、没有资源偏好、没有敌对关系。
- 内容侧确实已经有怪物种族：`mods/example_mod/races.json5` 里有 `examplemod:goblin`、`examplemod:ooze`、`examplemod:footpad`。**它们是能被造出来的 `Agent`，但世界上没有任何地方生成它们**——`Effect::SpawnActor` 不存在（全仓库只有两句注释提到这个名字），世界生成期只经 `roster` 造据点居民。
- **建立者种族的资源亲和是 Rust 硬编码的三元数组**：`roster.rs` 的 `race_weights` 返回 `[WeightedSlot; 3]`，按 `SETTLEMENT_RACE_IDS = ["lostland:human", "lostland:dwarf", "lostland:elf"]` 解析，规则写死为「食物→人类、金属→矮人、木材→精灵」（水与石材刻意无种族亲和）。**第三方 mod 加一个种族，它拿不到任何选址亲和，一座据点都不会属于它。** 这与同一个模块里职业亲和已经做到的「按大类挂规则、加一条 JSON5 就有对口职业」形成鲜明对照——**种族这一侧还停在职业那一侧改造之前的形态。**

**结论**：怪物种族要能参与选址，需要的是**把种族的资源/地形亲和从 Rust 硬编码搬到内容声明上**（`RaceDef` 新增字段，或 `CultureDef` 承担）。`race-system.md` 今天对这一层一个字都没写，**这是需要新写的设计，不是核实既有设计**。

### 四之四、部落复用 `SettlementSite`——所有者已裁定「是同一种东西」，以下是它的推论

**先给核实结论：ADR [0021](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) 的判据支持这个裁定，且支持得很强。** 判据是「有没有一份算法要被两种类型真正共用」。答案是**有，而且不止一份**：纪元推演（`advance_settled`）、承载力（`capacity`）、拓荒判定（`try_found`）、覆灭四原因的统一出口（`abandon`——该函数的文档注释已经点名「这正是 ADR 0021 的另一半：有一份算法要被四处共用」）、领地资源勘察（`survey_resources`）、按区块惰性铺设（`stamp_settlement`）、名册派生（`settlement_roster`）。另建一个 `TribeSite` 会把这七处全部复制一遍。**没有发现机制上过不去的障碍**——下面四条是需要一起想清楚的**推论**，不是反对意见。

#### 推论 1：编年史推演套用到部落上，哪些白拿、哪些语义要重新说

| 机制 | 判断 | 说明 |
|---|---|---|
| **人口增长 / 承载力** | 白拿 | 哥布林营地也吃饭。唯一要注意的是承载力全部来自**已注册资源点**——若哥布林不靠良田/矿脉吃饭（靠狩猎？靠劫掠？），承载力公式会给它一个「文明口径」的规模。**这需要内容侧回答「哥布林靠什么吃饭」，不需要改机制**（加一条 `resources.json5` 就是一种新资源）。 |
| **拓荒 / 重新拓荒** | 白拿，且效果很好 | `try_found` 接手上一茬的 `extracted`/`depleted`——「哥布林在人类矿城的废墟上重新扎营，但矿早被采光了」是这条**现有逻辑的直接产物，一行不用改**。 |
| **遗弃（凋零）** | 白拿 | — |
| **资源枯竭** | 白拿 | — |
| **瘟疫** | 白拿 | `PLAGUE_*` 的风险模型是「人越密越容易爆发」，对哥布林同样成立。唯一可能说不通的是**跨种族的疫病同一性**，但今天瘟疫是每座据点独立掷骰、不传播，**所以这个问题现在不存在**；等瘟疫做成会传播的那天再谈。 |
| **战争** | **白拿，但判据现在是错的** | 见推论 2。 |
| **编年史因果** | 白拿，且这是最大的收获 | `SettlementDemise::War { aggressor: WorldId }` 已经能顺着号码查回「谁灭的」。部落复用之后，「第 7 纪元，某矮人矿城被某哥布林部落攻灭」是**零新增代码**就能出现在编年史里的句子。 |

**净结论：七条里六条白拿，一条（战争）的判据必须改。**

#### 推论 2：战争配对判据要不要改成「敌对才打」——**要，而且这一改会把关系派生基线从锦上添花变成硬前置**

**今天的判据**（`chronicle.rs` 的 `wage_wars`/`nearest_rival`，逐条核实）：

1. 攻方人口 ≥ `WAR_MIN_POPULATION`（12）；
2. 守方是**射程内最近的、人口也 ≥ 12 的**据点（`war_range` = `WAR_RANGE_IN_SPACINGS`(3) × 最小据点间距）；
3. 攻方人口 > 守方人口 × `WAR_DOMINANCE_RATIO`（2）；
4. 一条 `WAR_NUMERATOR`/`WAR_DENOMINATOR` = 1/8 的确定性掷骰。

**四条里没有一条涉及种族、势力或关系。** 部落一旦复用，**今天就会出现「哥布林营地因为比隔壁哥布林营地大两倍所以把它灭了」，而旁边的人类村庄相安无事**——纯粹因为人口比。这在文明内部本来就已经在发生（现有行为），但加入部落之后它会变得刺眼。

**改成「敌对才打」是对的，但它有一个真实的架构障碍，必须现在说清楚：**

> **`wage_wars` 住在 `ll-world/src/chronicle.rs`，而种族与文化是内容、住在 `ll-mod`。`ll-world` 拿不到注册表。** 这正是 `roster.rs` 的 `settlement_founder_race` 文档记录的同一堵墙——建立者种族之所以是 `ll-mod` 的一个 `pub fn` 而不是 `SettlementSite` 的一个字段，就是因为「硬挂就要把种族内容倒灌进 `ll-world`，那是一次比那批大得多的依赖方向改动」。

**破法已经有现成先例**：`TerrainTable`、`ResourceTable` 都是**类型在 `ll-world`、数据由 `ll-mod` 装载器填、注入进世界生成**（`ChronicleParams` 已经在接 `ResourceTable`）。所以要让战争认敌对，需要的是**把「谁和谁敌对」做成一张 `ll-world` 侧的表**（形状与 `ResourceTable` 同构），由 `ll-mod` 从 `CultureDef.race_affinity` / 势力关系矩阵填好后注入。

**这就是优先级的改变**：原本「关系派生基线」是给 NPC 之间的好感度用的，可以排在 `CultureDef` 之后慢慢做；**一旦战争要认敌对，关系基线里的「文化↔种族 / 组织↔组织」这一层就成了世界生成本身的输入，必须与 `CultureDef` 同批落地**，否则部落一接上就会产出语义错误的战争史。已反映到六节的顺序里。

**一条务必守住的边界**：改判据时**不要把「敌对」做成开战的唯一条件**。现有四条里的人口阈值与优势比是**防止世界打空**的闸门（1/8 掷骰 + 2 倍优势比让实测四十八个种子里战争覆灭是少数派，测试 `四十八个种子里一次战争导致的覆灭都没有` 守着它至少发生过）。正确改法是**加一条乘法项/加分项**（敌对抬高开战概率、同族压低），而不是替换 2、3 两条——这与 `try_found` 已经在用的「四条加分互相独立地推高同一个概率分子」是同一个手法。

#### 推论 3：怪物种族参与选址，撞的是同一堵墙

见四之三。补一句与本节直接相关的：**建立者种族的抽取发生在 `ll-mod`（`settlement_founder_race`），而选址发生在 `ll-world`（`try_found`）。今天这两件事是单向的——先选址、后定种族。** 要让「哥布林偏好某种地形/资源」真的影响**选在哪**，必须把种族亲和送进 `try_found`，也就是同一堵 `ll-world`/`ll-mod` 依赖墙。**推论 2 与推论 3 撞的是同一堵墙，一次破墙两个都通。**

#### 推论 4：`SettlementSite` 上哪些字段是「文明专属」的——**核实结论：一个都不是**

`SettlementSite` 的全部十个字段：`id` / `zone` / `anchor` / `status` / `founded_epoch` / `abandoned_epoch` / `population` / `peak_population` / `building_count` / `resource_profile`。**逐个看，没有一个字段的语义只对文明成立**——一个哥布林营地同样有位置、有建立纪元、有人口、有峰值人口、有几间窝棚、靠什么吃饭。**这正面支持了所有者的裁定。**

**文明专属的东西不在 `SettlementSite` 上，而在读它的那两个消费者里，全部是硬编码：**

| 硬编码 | 在哪 | 今天写死成什么 |
|---|---|---|
| **建材** | `settlement.rs` 的 `house_tiles`/`ruin_tiles` | 有人住 → `ids.wall_wood` + 木地板 + 门窗；废墟 → `ids.wall_stone`、无门窗。**与文化、种族、资源全部无关。** |
| **布局** | `settlement.rs` 的 `spiral_offset` | 以锚点为中心的方环螺旋，`BUILDING_SPACING` 6、`BUILDING_SPAN` 5×5。**一座哥布林营地会长得和矮人矿城一模一样。** |
| **职业名册** | `roster.rs` 的 `SETTLEMENT_CLASS_IDS` / `commoner_weights` | 十条 `lostland:*` 职业写死，资源亲和按大类挂（这一侧已经是可扩展的）。 |
| **种族权重** | `roster.rs` 的 `SETTLEMENT_RACE_IDS` / `race_weights` | `[人类, 矮人, 精灵]` 三元数组写死（这一侧**不可扩展**，见四之三）。 |
| **行会** | — | 不存在，谈不上专属。 |

**所以差异该怎么表达？** 三个选项，按 ADR 0021 判据评估：

- **(a) 靠建立者种族这一个维度**——已经落地一半：`settlement_founder_race` 是 `pub`，签名只要 `(&SettlementSite, &SettlementRoles, u64)`，其文档明确写着「将来铺房子那边要知道这座据点是谁建的，调它一次即可」。**但它不够**：种族决定不了建材偏好（矮人用石头是文化，不是生理），也决定不了社会结构。
- **(b) 靠 `CultureDef`**——原设计给的答案，`building_materials`/`site_terrain`/`economy_weights`/`social_structure` 四个字段正好就是上表四处硬编码要的输入。
- **(c) 给 `SettlementSite` 加一个 `kind: Settlement | Camp` 枚举**——**建议否决，理由正是 ADR 0021**：这不会消除任何重复逻辑（七处算法照旧共用），只会在每一处消费点插一个 `match`，而那些 `match` 分支要读的数据（用什么建材、住着什么职业）**本来就该来自一张表而不是一个枚举变体**。「营地 vs 城市」的差别应该是 `CultureDef` 里的权重差别（社会结构 = 部落、建材 = 木/皮、经济 = 狩猎），不是类型里的一个标签。

> **推荐答案：(a) + (b)。建立者种族回答「谁建的」，`CultureDef` 回答「他们怎么建」。** 两者的绑定关系（种族 → 默认文化）需要所有者裁定一次，见七节 2。

---

## 五、文档里声称「已落地」的东西，今天真的还在且有消费者吗

| 声称 | 还在吗 | 形状变了吗 | **有真实消费者吗** |
|---|---|---|---|
| `Affiliation` | 在（`ll-world/src/entity/affiliation.rs`） | **变了**：`org: ContentIndex` → `org: OrgRef` | **半个。** 唯一读它的是 `ll_sim::ai_query::is_hostile`（→ `nearest_hostile` → `ll_mod::native_behavior` 的哥布林行为树），链路是真的、跑得到。**但生产里所有 `affiliations` 恒空，`is_hostile` 恒走「a 无势力 → 对谁都敌对」那条退化分支。** 其余「消费者」全是 `Vec::new()` 夹具、`WorldState::hash` 混入、存档 remap。 |
| `AffiliationKind` | 在，六变体一字未改 | 否 | 只有 `Faction` 一个变体被读过。`Culture`/`Guild`/`Religion`/`Family`/`Profession` **五个变体从未被构造、也从未被匹配**。 |
| `OrgRef` | 在（新增） | 新类型 | 只有 `ll-content/src/remap.rs` 按变体分流（`Instance` 不重映射、`Def` 走可丢弃重映射）。**没有生产者。** |
| `OrgInstance` | 在（`ll-world/src/entity/org.rs`，82 行） | — | **零。** 只有一行 `pub use` 与一句「未来 `OrgInstance` 等」的注释。**教科书式的「声明了但从没接线」。** |
| `Agent.affiliations` | 在（`agent.rs:120`） | 否 | 进哈希、进 remap，**但两条生产路径都写死 `Vec::new()`**。 |
| `Agent.wallet` | 在（`agent.rs:122`） | 否 | **零消费者**，两条生产路径都写死 `0`。 |
| `Agent.profession` | 在（`agent.rs:124`） | 否 | **有真实生产者了**（`roster` 按资源画像抽出的职业），消费者是 `native_behavior` 的「这个实体是不是 `lostland:guard`」分支——`roster.rs` 明确写着「本模块是它第一次真的可能成立的地方」。 |
| `NamingRules` | 在（`ll-world/src/naming.rs`） | 否 | **只有 demo。** `ll-sim/examples/p3_acceptance` 用写死的 `demo_naming_rules()`。生产路径零调用，**NPC 至今没有名字**。 |
| `Agent.race` | 在（`agent.rs:139`） | 否 | **有真实生产者与消费者**（`roster` 抽种族 → `bake_race_stat_modifiers` 烘焙属性 → 开局装备）。 |
| `ThinPopulation.race` / `.family` / `.wallet_of` | 在 | 否 | **零。整个薄层在生产里从未被写入过一次。** |
| `RaceDef` / `RaceTable` | **在**（`ll-mod/src/race.rs`）——文档说未落地，**说错了** | 比草图多三个字段 | 有：属性烘焙、开局装备、天赋授予、暗视。 |
| `RaceDef.footprint` | 在 | 否 | **零消费者。** 进了哈希与审计，没有任何碰撞/寻路代码读它。 |
| `RaceDef.lifespan_years` | 在 | 否 | **零消费者。** |
| `CultureDef` / `Kinship` / `Traits` / `StructureKind` / `Tenet` / `EconomyKind` / `SocialStructure` / `race_affinity` / `birth_settlement` | **全部不存在** | — | — |

---

## 六、下一批「`CultureDef` 完整落地 + 关系派生基线」的前置清单与建议顺序

**范围判断**：这不是一批，**至少是三批**。里面夹着一次跨 crate 的依赖方向工作（内容表注入世界生成）和一次 `WorldState` 存档格式改动（组织关系矩阵），任何一次单独拿出来都是一个完整批次。

### 前置清单（动工前必须已经为真的事）

| # | 前置 | 现状 | 谁挡着谁 |
|---|---|---|---|
| P1 | **裁定「职业有几个真相源」** | 未决，[`settlements-structures-and-npc-spawning.md`](../design/settlements-structures-and-npc-spawning.md) 十二节 1 挂了很久 | `AffiliationKind::Profession` 与 `Agent.profession` 同时存在，关系基线的职业项不知道该读哪个。**不裁定就会造出第三处。** |
| P2 | **裁定「种族 → 默认文化」的绑定关系** | 未提出 | `CultureDef` 落地时必须回答「一座矮人建的据点用哪份文化」。见七节 2。 |
| P3 | **`CultureDef` 的宿主 crate** | 未决 | 必须走 `TerrainTable`/`ResourceTable` 那条路（类型在 `ll-world`、数据由 `ll-mod` 填、注入世界生成），否则选址与建材接不上。 |
| P4 | **`SettlementSite` 要不要带文化字段** | 未决 | 它是 `Copy` 且**不进存档**（ADR 0009，纯派生），加字段**不触发存档迁移**；但会触发依赖方向问题。`settlement_founder_race` 已经论证过一次「不加字段、改用 `pub fn` 现算」的做法，可照抄。 |
| P5 | **组织↔组织矩阵的存档归属** | 未决 | 这是整批里**唯一必须进存档**的东西。要动 `WorldState`、`hash()`、`remap`、存档版本——`roster` 那批为了避开这件事专门绕了一大圈（`materialized_settlements` 而不是逐 NPC 偏差表），可见成本。 |
| P6 | **薄层 `race` 列的实现债务** | 见二节「八」 | **现在还接近零成本**（薄层无真实数据）。`birth_settlement` 是聚落种族权重表的前置，而权重表是文化选址的前置。**建议在第一批顺手还掉，不要再拖。** |

### 建议顺序

> **顺序的核心判断：`CultureDef` 必须先落地，因为关系派生基线的两项（文化项、`race_affinity`）都要从它取值；而「战争认敌对」又要求「文化↔种族敌对表」与 `CultureDef` 同批——所以敌对关系这一小块被提前到了第一批，不能等第三批。**

**第一批：`CultureDef` 骨架 + 敌对表，全部注入世界生成**

1. `CultureDef` 类型定在 `ll-world`（照 `ResourceTable` 的形状），`ll-mod` 加 `mods/lostland/cultures.json5` 与 schema、内容哈希版本递增、`content_audit` 跟进、两份 `.ftl` 补键。
2. 先落**四个直接有落点的字段**，每个都能单独验收：
   - `building_materials` → 替换 `settlement.rs` 的 `wall_wood`/`wall_stone` 硬编码；
   - `site_terrain` → 加进 `try_found` 的加分项；
   - `economy_weights` → 乘进 `resource_profile` 的排序；
   - `naming` → 把 `NamingRules` 从 demo 接进 `roster`，**让 NPC 第一次有名字**。
3. `race_affinity` + 一张 `ll-world` 侧的**敌对表**（文化↔文化、文化↔种族），由 `ll-mod` 填、注入 `ChronicleParams`。
4. 改 `wage_wars`：敌对作为**加分项/乘法项**接进现有的 1/8 掷骰，**不动人口阈值与优势比**（理由见四之四推论 2 末尾）。
5. 还掉 P6：加 `birth_settlement` 列，`race` 改成按聚落权重表现算。

**第二批：怪物种族与部落**

6. 种族的选址/资源亲和从 `roster.rs` 的三元硬编码搬到内容声明（`RaceDef` 新增字段或 `CultureDef` 承担）——**这需要新写一节设计，`race-system.md` 今天没覆盖，见四之三。**
7. 给部落文化各配一份 `CultureDef`（社会结构 = 部落、建材 = 木/皮、经济 = 狩猎/劫掠），让哥布林走同一条 `try_found`/`advance_settled`/`wage_wars`。
8. **验收线建议**：编年史里能出现一句「第 N 纪元，某矮人矿城被某哥布林部落攻灭」，并且那座矿城**在地上真的是废墟**。这条链路今天除了「谁建的、跟谁敌对」之外全部已经通了。

**第三批：关系派生基线本体**

9. 世界生成期造 `OrgInstance`（势力/家族至少两类），给 `Agent`/名册真的填 `Affiliation`——**这是让整套归属体系第一次有生产者的那一步。**
10. 组织↔组织稠密矩阵进 `WorldState`（P5 的存档工作在这里付账）。
11. 派生基线求和函数 + `is_hostile` 换实现。
12. **记忆槽与 `Traits` 明确排到第四批**——它们是「基线 + 偏移」的偏移那一半，基线本身不依赖它们。

---

## 七、需要项目所有者裁决的

1. **「职业」的真相源合并成哪一个**（P1）。`Agent.profession: ContentIndex`（已落地、已有生产者与消费者）与 `AffiliationKind::Profession`（从未构造）在描述同一件事。**这是第四次要求裁决同一个问题**（[`settlements-structures-and-npc-spawning.md`](../design/settlements-structures-and-npc-spawning.md) 十二节 1 已提过）。不决就会在关系基线里造出第三处。
2. **种族与文化怎么绑定**（P2）。一座矮人建立的据点用哪份 `CultureDef`？三个候选：(a) `RaceDef` 带一个默认文化字段（最省）；(b) 文化独立抽取，种族与文化各抽各的（表现力最强，能长出「住在矮人地界的人类城」，也最贵）；(c) 文化是主、种族由文化的 `race_affinity` 反推。**这条决定了 `CultureDef` 的抽取路径长什么样，必须在第一批动工前定。**
3. **「营地」是玩家搭的还是世界生成的**。[`settlements-structures-and-npc-spawning.md`](../design/settlements-structures-and-npc-spawning.md) 十二节 3 提过、至今未决。所有者这次说的「哥布林营地」听起来是**世界生成的**（那就完全复用 `SettlementSite`，本文档全部结论适用）；但如果还想要**玩家自己搭的临时营地**，那是另一件事——它需要「可放置物件」这条至今不存在的路径，**不在本次复用范围内**。
4. **怪物据点要不要立刻有自己的外观**。今天 `stamp_settlement` 会把每座据点都铺成有门有窗的 5×5 木屋。哥布林营地复用同一份铺设算法，在 `building_materials` 落地之前会长得和人类村庄一模一样。**接受这个过渡态（先跑通历史、外观后补），还是要求建材与布局同批落地？** 前者能让第二批的验收线提前很多。
5. **战争会不会把世界打空**。改成认敌对之后，若敌对是加分项，敌对密集的区域战争频率会显著上升。建议第一批落地后**实测四十八个种子的战争覆灭占比**，若超过某个阈值就调 `WAR_DENOMINATOR`——请所有者给一个「多少算太多」的口径。

---

## 八、复核中发现的、不属于上面任何一节的问题

1. **`crates/ll-mod/src/race.rs` 的模块文档仍在描述脚本时代**：多处提到 `register-race`、`register-race-xp-reward`、「不改既有 `register-*` 函数的参数个数」，而这些函数与 `.scm` 文件都已不存在。同理 `crates/ll-world/src/chronicle.rs` 的「落地范围（如实标注）」一节仍写着「没有战争、没有王朝、没有具名历史人物」，而同一文件里 `wage_wars` 与 `CHRONICLE_WAR_STREAM_ID` 都在。**属于 `crates/**`，本次只读未改，记在此处供后续批次顺手修正。**
2. **[`design/README.md`](../design/README.md) 的一处过期**：「`Affiliation.org` 字段类型需要从 `ContentIndex` 改为 `WorldId`，尚未实现」——**已经实现了**，落成 `OrgRef` 枚举（`Def(ContentIndex)` / `Instance(WorldId)`）。同表关于薄层 `race` 实现债务的那一行仍然准确。本次已在三份设计文档的更正段里指出，索引本身留给下一次维护。
3. **`conflicts.md` 的两条设计缺口（5 `StatBonus`、6 `EquipSlot`）其实已经闭合**，但清单没人回来关。建议下次维护该清单时标注。
4. **`RaceDef` 有两个零消费者字段**（`footprint`、`lifespan_years`）。它们不是错误——设计文档为它们各写了完整的一节——但按本仓库自己的纪律（「加特质前必须先回答它接在哪个已有机制上」），它们已经在「声明了、存了、哈希了、审计了、没有玩法后果」这个状态上停留了若干批次，值得在某次批次里明确一次「什么时候接、还是先摘掉」。
