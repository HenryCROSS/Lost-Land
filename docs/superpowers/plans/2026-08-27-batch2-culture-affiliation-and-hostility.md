# 批次 2：文化归属与敌对判定

**基线**：`46a1965`　**分支**：`wt-culturehostility`　**日期**：2026-08-27

来源：交接文档第三节「批次 1」的**裁定 1**（种族默认敌对）。该裁定在批次 1
（`c02ffe4`）中被拆出，原因见
`2026-08-27-batch1-movement-and-occupancy.md` 的「⚠ 范围变更」段。所有者随后
四次追加裁定，本文档是其最终形状。

**基线测试数**：由实现者自己跑 `bash scripts/ci/run_tests.sh` 记录，**不要照抄
批次 1 的数字**。

---

## 一、所有者裁定原文

1. 「玩家可以没有势力归属，这个可以通过后面和据点的管理者对话加入。哥布林有
   自己的据点，那也就存在自己的势力，势力可能通过历史模拟扩张，有多个据点。」
2. 「或者利用现有机制，哥布林种族默认对其他种族仇恨度非常高。仇恨度高过某个值
   就会视为敌对。这样应该就可以了。」
3. 「那文化设定一个独特的东西叫无文化，这东西颗粒度小到具体某个 NPC。」
4. 敌对阈值：「我觉得 5 也没问题。」

**所有者选定的是第 2 条**（利用现有机制）。第 1 条是更长期的势力播种方向，
**不在本批次范围**——不要碰 `OrgInstance`、不要做势力播种。

---

## 二、为什么不是给 `RaceDef` 加字段（必读，否则会重犯）

`knowledge/design/race-system.md:267`-`269` **明确否决**过「给关系派生基线加一条
『同种族 +X／异种族 −Y』的常量项」，理由是常量对丢掉了不对称性；替代方案是把
种族态度挂在**文化**上（`:271`）。

而这条通路**已经落地**：

| 事实 | 证据 |
|---|---|
| `CultureAttrs.hostility`：文化 → 文化的有向敌意分 | `crates/ll-world/src/culture.rs:189` |
| `MAX_HOSTILITY = 7` | `culture.rs:94` |
| `CultureTable::hostility(攻方, 守方)`，**刻意不对称**，查不到即 0 | `culture.rs:369` |
| 哥布林敌意内容**已经写好** | `mods/lostland/cultures.json5:150`-`153`：goblin_warband → mining_hold 6、farmstead 4、stonecutters 4 |
| 反向刻意更低 | 同文件 `:86`-`87`：mining_hold → goblin_warband 只有 3 |

新开 `RaceDef` 字段会造出第二个「谁跟谁不对付」的真相源。**不要这么做。**

---

## 三、动手前必须知道的既有事实（基线 `46a1965` 已 grep 核实，你仍要自己复核）

| 事实 | 证据 |
|---|---|
| `Agent::affiliations` 至今零生产者 | `crates/ll-mod/src/roster.rs:806`、`crates/ll-game/src/world.rs:553` 都写死 `Vec::new()` |
| `SettlementSite.culture: Option<CultureKind>` | `crates/ll-world/src/settlement.rs:276` |
| `build_npc_agent(profile, pos, zone, roles, ctx)` | `crates/ll-mod/src/roster.rs:783` |
| `MaterializeContext` 有 `races`/`items`/`surface_profile`/`now` | `roster.rs:755` |
| 物化循环里 `site` 现成可用 | `crates/ll-game/src/world.rs` 的 `materialize_nearby_settlements` |
| `AffiliationKind::Culture` 恒走 `OrgRef::Def(ContentIndex)` | `crates/ll-world/src/entity/affiliation.rs:49`-`57`、`:74`-`82` |
| `Affiliation.standing: i32`，千分比，**负值表示敌对** | `affiliation.rs:99` |
| **敌意目标只 `intern`、不要求已 `define`** | `crates/ll-mod/src/content_schema_world.rs:246` 原文：「`hostility[].culture` 全部走 `intern` 而不是『只 get』」 |
| `CultureTable::registered()` 返回 `self.order`，**只含 `define()` 过的** | `culture.rs:312` |
| `pick_culture` 只遍历 `registered()` | `crates/ll-world/src/chronicle.rs:1312` |
| `culture_weight` 给每份已注册文化保底 `CULTURE_BASE_WEIGHT = 4` | `chronicle.rs:1347` |
| `CultureTable::define` 校验「至少要有一个权重为正的建立者种族」 | `culture.rs:270` 附近，`CultureError::NoFounderRace` |
| `CultureKind::from_index` 是**无校验**的 `const fn` | `culture.rs:77` |
| 「只 intern 不 define 的占位索引」有完整先例 | `crates/ll-mod/src/base_placeholder.rs:40`/`:51`/`:68`（`lostland:placeholder_race`） |
| `Affiliation.standing` 进世界哈希 | `crates/ll-world/src/state.rs:1633` |
| `ll-sim` **可以直接借** `ll-world` 的具体类型 | 依赖方向 `ll-world ← ll-sim`；先例见 `crates/ll-sim/src/catalogs.rs` 的 `AmbientSource` 文档「为什么它不是一个 `&dyn 某某Catalog`」 |

---

## 四、设计裁定

### D1：`lostland:cultureless` 是「只 intern、不 define」的文化索引

**这是本批次最省的一步，也是最容易做错的一步。**

因为敌意目标只 `intern`（第三节），一个从未 `define()` 过的文化索引**照样可以
被别的文化声明敌意**。而 `pick_culture` 只遍历 `registered()`（= 已 `define()`
的），因此这个索引：

- **永远不会被选为建城文化**——不必改 `pick_culture` 一个字；
- **不改变任何权重与掷骰序列**——世界生成路径逐位不变；
- **不必改 `CultureTable::define` 的三条校验**（尤其「至少要有一个权重为正的
  建立者种族」那条，正是「无文化」过不去的那一关）；
- **不必递增 `CONTENT_HASH_ALGORITHM_VERSION`**（内容表一个字段都没加，改的只是
  内容取值——见 `content_hash.rs:1345` 那段区分「量尺」与「取值」的注释）。

**注册落点**：照 `crates/ll-mod/src/base_placeholder.rs` 的三件套写法
（`PLACEHOLDER_RACE_ID` 常量 + 注册函数 + 查询函数）加一份文化版。

**这不是在重犯 `SETTLEMENT_RACE_IDS` 那个错误**，文档里要写清区别：
`race-system.md:433`-`437` 批评硬编码 `SETTLEMENT_RACE_IDS`，是因为它**把 mod
内容排除在机制之外**（第三方种族拿不到任何选址亲和）；而一个「缺席」哨兵索引
不排除任何人——mod 照样可以声明对它的敌意，也照样可以让自己的 NPC 无文化。
形状上它与 `lostland:placeholder_race` 完全同类。

### D2：无文化是**查询期回退**，不是写进每个实体的归属

所有者裁定「玩家不挂归属」。因此**不给玩家（也不给任何实体）写一条指向
`lostland:cultureless` 的 `Affiliation`**。判定时，实体身上找不到
`AffiliationKind::Culture` 归属，就回退到这个索引。

好处：`world.rs:553` 的玩家构造路径 `affiliations` 保持 `Vec::new()` 不变，
玩家不产生任何存档/哈希增量。

所有者「颗粒度小到具体某个 NPC」这条仍然成立：将来要让某个具体 NPC 显式无文化，
给它写一条指向该索引的归属即可，与回退结果一致，不产生歧义。

### D3：NPC 物化时挂据点文化——`Agent::affiliations` 的第一个生产者

`build_npc_agent` 给 NPC 挂一条
`Affiliation { kind: Culture, org: OrgRef::Def(文化索引), standing: <具名常量> }`，
文化取自 `SettlementSite::culture`。据点没有文化（`None`，空文化表的世界）时
**不挂**——回退到 D2 的无文化，与「诚实表达尚无内容」一致。

`standing` 取一个具名常量（个体对自己文化的认同度），**文档要写明取值理由**；
不要取 0（0 在千分比语义下是「毫无认同」，与「生在这个文化里」矛盾）。

**会动世界摘要吗**：`Affiliation.standing` 进 `hash()`（`state.rs:1633`），
所以凡是经物化路径造 NPC 的世界哈希都会变。两条黄金基准大概率不受影响
（见第六节），**必须实跑确认**。

### D4：`declared_hostile` 直接借 `&CultureTable`，不新开依赖倒置 trait

`CultureTable` 住在 **`ll-world`**，而 `ll-sim` 依赖 `ll-world`——可以直接借具体
类型。先例是 `catalogs.rs` 的 `AmbientSource`，其文档专门论证了「为了对称而多造
一对没有第二个实现的 trait，正是 ADR 0021 点名要避免的那种抽象」。

**第一件要查的事**：`WorldState` 能不能直接拿到 `CultureTable`。

- **拿得到** → `route_move_into_occupant(world, raw)` 签名一个字都不用改，
  `declared_hostile` 从 `world` 取表。**优先走这条。**
- **拿不到** → `declared_hostile` 与 `route_move_into_occupant` 各加一个
  `&CultureTable` 参数，两处调用点从各自已有上下文转发。
  **不要**为此新增 `ResolveCatalogs` 字段（那要改 15 处字面量构造点）。

判定形状：

```
declared_hostile(a, b, cultures) =
    culture_hostility_declares_hostile(a, b, cultures)
    || ((has_faction(a) || has_faction(b)) && is_hostile(a, b))
```

第一项**对称**取两个方向的最大值（撞格路由必须给出对称答案——一次换位不可能
「我换你、你砍我」），任一方向的敌意分 **≥ `HOSTILE_CULTURE_THRESHOLD`** 即敌对。

**短路闸门必须开口**：现状 `(has_faction(a) || has_faction(b)) && is_hostile(a,b)`
在 `affiliations` 零生产者时恒假。文化判据必须排在它**之前**，否则本批次是空操作。

**注意**：D3 落地后 NPC 有了 `Culture` 归属，但 `has_faction` 查的是
`AffiliationKind::Faction`，**仍然恒假**。这是对的——势力播种是另一批。

### D5：阈值 `HOSTILE_CULTURE_THRESHOLD = 5`（所有者裁定）

具名常量，文档要写清它与 `MAX_HOSTILITY = 7` 的关系，以及**它在现有内容上的
可观测后果**（这一段是本裁定的价值所在，必须写）：

| 攻方 → 守方 | 敌意 | ≥5？ |
|---|---|---|
| goblin_warband → mining_hold | 6 | **敌对** |
| goblin_warband → farmstead | 4 | 不敌对 |
| goblin_warband → stonecutters | 4 | 不敌对 |
| mining_hold → goblin_warband | 3 | 不敌对 |

即阈值 5 之下，**哥布林只对矮人矿业据点敌对，对农庄与石匠不敌对；矮人不主动
敌对哥布林**。那份刻意的不对称被完整保住了。

### D6：哥布林对「无文化」的敌意分

`mods/lostland/cultures.json5` 的 `goblin_warband` 追加一条
`{ culture: "lostland:cultureless", hostility: 6 }`——取 6 与它对 mining_hold
一致（「无依无靠的外来者」与「世仇矮人」同档），**必须 ≥5**，否则「玩家走向
哥布林 = 攻击」这条所有者最想要的效果不成立。JSON5 里写注释说明。

其余五份文化**不追加**对无文化的敌意——农夫不该因为你没有文化就砍你。

### D7：`lostland:cultureless` 必须**追加在末尾**

`Registry::intern` 按装载顺序分配 `ContentIndex`。插在中间会平移既有文化的索引，
同时改内容哈希与世界摘要。若按 D1 走 `base_placeholder` 那条注册路径，它不进
`cultures.json5`，本条自动满足——但仍要确认**注册时机不会挤占既有号段**。

---

## 五、必须新增的测试（每条都要按 ADR 0018 用「故意改坏」验证真的会红）

1. 玩家（无任何归属）撞哥布林 NPC → **攻击**，不是互换。本批次唯一能被玩家直接
   观察到的后果。
2. 玩家撞农夫 NPC（farmstead 文化）→ **仍然互换**。阈值 5 把这一半挡住了；这条
   变红说明阈值被调低或判据写反了。
3. 哥布林 NPC 撞哥布林 NPC → **不敌对**（同文化，`hostility` 表里无自指条目）。
4. 哥布林 NPC 撞矮人矿业 NPC → **攻击**（6 ≥ 5）。
5. 矮人矿业 NPC 撞哥布林 NPC → **不攻击**（3 < 5）。**这条守的是那份刻意的
   不对称**，是 D5 的核心证据。
6. 敌意**恰好等于阈值**时算敌对，低一分不算（钉住 `>=` 不被写成 `>`）。
7. 物化出来的 NPC 身上真的有一条 `AffiliationKind::Culture` 归属，且指向所属据点
   的文化（D3 的活证据，`Agent::affiliations` 第一个生产者的证明）。
8. 据点没有文化时物化出来的 NPC **不挂**归属，判定回退到无文化（D2）。
9. `lostland:cultureless` **不会被选为建城文化**——跑一遍世界生成，断言没有任何
   据点的 `culture` 等于该索引（D1 的核心证据）。

---

## 六、黄金基准

**两条都要实跑，不许照抄本节预判。**

- **`EXPECTED_WORLD_DIGEST`**（`crates/ll-world/tests/determinism.rs:231`，
  `17_228_492_522_544_021_674`）：该测试全程零 `actors.spawn`（文件内 `:233`-`:249`
  已由前人 grep 验证并写明），不经物化路径，**预期逐位不变**。
- **`EXPECTED_REPLAY_DIGEST`**（`crates/ll-sim/tests/replay.rs:862`，
  `14_731_332_643_995_045_404`）：`setup` 直接 `spawn` 两个 `affiliations:
  Vec::new()` 的实体，不经 `build_npc_agent`，**预期逐位不变**。

**真正会动的是经物化路径的测试**（`crates/ll-game/tests/npc_materialization.rs`、
`culture_and_war.rs` 等）。它们不是黄金基准，但若其中钉死了哈希/计数，**同样要走
四步重冻**：① 确认基线红 → ② 把改动关掉确认**精确**回到旧值 → ③ 恢复 →
④ 新常数在**两个独立进程**里复现。四步证据写进提交信息。

`CONTENT_HASH_ALGORITHM_VERSION` **保持 27**——本批内容表一个字段都没加，改的只是
内容取值。`content_hash_of("lostland")` 会变，那是它该变的东西。

---

## 七、范围边界（不要越界）

- **不做势力播种**，不碰 `OrgInstance`。所有者裁定第 1 条是长期方向。
- **不改 `is_hostile` / `nearest_hostile` / `native_behavior.rs`**。哥布林行为树的
  主动索敌在 `affiliations` 零生产者时本来就对所有人返回真。
- **不做卫兵巡逻**。所有者已裁定卫兵走「每天在据点附近巡逻」的任务，归 P8 据点
  派工（见交接第四节第 12 条的裁定段）。
- **不修** `resolve.rs` 那处读裸 `agent.race` 的已知缺陷（交接第五节第 2 条）。
- **不碰** `resolve_exit_space` 的占位缺口（批次 1 的 D8，已登记）。
- **不碰** `nearest_visible_actor` 与 `SurfaceWindow` 的常驻问题——那是并行进行的
  另一批（`wt-fovhotfix`）在修的崩溃缺陷，两批都会改 `crates/ll-sim/src/ai_query.rs`
  但**是不同函数**。你只动 `declared_hostile`，别碰 `nearest_visible_actor`。
