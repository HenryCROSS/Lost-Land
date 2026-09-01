# 批次 26：对话系统的批次 3——加入据点

**工作树** `wt-dialogue3`（从 `origin/main` 的 `b85330c` 分出）。
**规格** `knowledge/design/dialogue-system.md` 五节 5.1、八节分批表第 3 行。
**上游交接** `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md`
第七节那张挂载点表的「批次 3」一行；
`docs/superpowers/plans/2026-08-31-batch21-dialogue-ui.md` 第十节的九条临时裁定。

---

## 〇、开工基线（自己跑的，不抄任何文档）

```
bash scripts/ci/run_tests.sh   → EXIT=0，2951 通过 / 127 个二进制 / 0 忽略
```

三个关键常量 grep 自取（**本文档不留副本，只记「改前是多少」这个事实**）：

```bash
grep -n "pub const CURRENT_SCHEMA_VERSION" crates/ll-content/src/save_file.rs
grep -n "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs
grep -rn "const EXPECTED_" crates/ll-world/tests/determinism.rs \
  crates/ll-sim/tests/replay.rs crates/ll-game/tests/populated_determinism.rs
```

---

## 一、范围

三件事，对应三个提交：

| | 内容 |
|---|---|
| **A** | `Agent.home: Option<WorldId>`——把 `NpcProfile.home` 这个既有真相源搬进实体。**存档主体形状变更**，`CURRENT_SCHEMA_VERSION` 必须跟着升 |
| **B** | `join-settlement` 后果：从「schema 报尚未实现」挪到「真的实现」，写入走 `apply` 唯一入口（C1），`standing` 常量化 |
| **C** | `affiliated` / `standing-at-least` 两条谓词第一次有非假的读数，端到端验证 |

**不做**：`complete-quest` / `give-item`（批次 4）、`open-trade`（批次 5）、
`AffiliationKind` 的第六个变体（规格九节第 2 条未裁定）、`standing` 的衰减/
上涨机制（本批只有「加入」这一个生产者）。

---

## 二、A：`Agent.home` 的落地形状

### 2.1 字段本身

```rust
/// 这个实体属于哪座据点（`SettlementSite::id`）；玩家与不属于任何
/// 据点的实体是 `None`。
pub home: Option<WorldId>,
```

**位置：结构体末尾**（`stealthed` 之后）。postcard 按声明顺序吃字节，
末尾追加是唯一不会让既有字段错位的位置——但**这不是兼容性论证**，
见 2.3。

**生产者恰好一个**：`ll_mod::roster::build_npc_agent` 写
`home: Some(profile.home)`。`NpcProfile.home: WorldId` 已经是真相源
（`roster.rs`），本字段只是把它搬运过来，不是第二份真相源。
`ll_game::world::build_player_agent` 写 `None`——玩家不属于任何据点，
这正是所有者裁定「玩家可以没有势力归属」在实体上的表达。

**消费者恰好一个，且从第一天起就在决策层**：
`ll_sim::resolve::dialogue::resolve_dialogue_choose` 读说话人的 `home`
（`crates/ll-sim/src/**` 是 `check_field_consumers.py` 的决策层 glob）。

**被否决的替代方案**（规格 5.1 已经论证过，这里只记结论）：按位置反查
`sites_touching_zone`。NPC 会走动，离开自己那片 zone 之后反查要么给出错的
据点、要么什么都给不出，**而且不会报错**。

### 2.2 进 `WorldState::hash`（ADR 0022）

`home` 决定玩家跟这个 NPC 说话能加入哪个势力——它真的分岔未来，因此必须
进哈希。走既有的 `write_optional_world_id`（`remembered_id` 用的同一个），
位置紧跟 `stealthed` 之后。

**这会动黄金基准**，预期如下（第五节实测）：

| 基准 | 预期 | 结构性理由 |
|---|---|---|
| `EXPECTED_WORLD_DIGEST` | **不变** | 那个世界零 `actor`，`for agent in self.actors.iter()` 循环体一次都不执行（该常量自己的文档与 `populated_determinism.rs` 的 A 表都已实测记录这条局限） |
| `EXPECTED_REPLAY_DIGEST` | **变** | 它的 `setup` 手写两个 `Agent`，每个都会多写一个 `0`（`None` 的判别值） |
| `EXPECTED_POPULATED_WORLD_DIGEST` | **变** | 那个世界里 29 个 `Agent`，其中物化出来的 NPC 全部是 `Some(据点号)` |

后两条各走四步重冻（交接文档纪律第 2 条），证据写进常量文档与提交信息。

### 2.3 老存档兼容性：**不兼容，而且不能靠 `#[serde(default)]` 声称**

存档主体走 postcard（non-self-describing）。`#[serde(default)]` 在那条路径上
是**空操作**——`Agent::gender` 与 `GroundItemStack::placed` 已经各犯过一次，
`scripts/ci/check_save_schema_version.py` 就是为此补的门禁。

因此本批的做法是：**`CURRENT_SCHEMA_VERSION` 递增，老存档被明确拒绝**，
不写任何迁移，也不写任何「老存档读得回来」的断言。
**实测证据**（第五节回填）：

1. 门禁 `python scripts/ci/check_save_schema_version.py` 在未升版本时报红；
2. 一条新单元测试：把「旧形状」的 `Agent`（不带 `home`）用 postcard 编码，
   再用新形状解码——必须 `Err`。这条测试证的是「不兼容」这个事实本身，
   不是一句声明。

### 2.4 `remap_agent`：`home: _`，逐条写明理由

`WorldId` 不依赖 mod 装载顺序（`ll_core::ident::WorldId` 模块文档），
与 `remembered_id` / `OrgRef::Instance` 同一条既有处理。**但这条「不需要
重映射」不等于「没有风险」**——规格 5.1 第 2 条那条风险（编年史推演变了，
老存档里的据点号静默指向另一座据点）**原样成立、本批不解决**，它由
`CURRENT_SCHEMA_VERSION` 的「版本不对就明确拒绝」兜底，规格九节第 3 条
仍未裁定。这一句要写进字段文档，不能含糊过去。

---

## 三、B：`join-settlement` 的落地形状

### 3.1 数据侧：一条新的 `DialogueOutcome` 变体

```rust
pub enum DialogueOutcome {
    SetFlag(NamespacedId),
    /// 加入说话人所属据点的**势力**。
    JoinSettlement,
}
```

**不带参数**：加入哪座据点由**说话人**回答（他的 `home`），不由内容作者
写死——`WorldId` 是世界生成期分配的号，内容文件里根本写不出来（这与
`AffiliationQuery.org` 只能指向内容空间那条既有缺口是同一件事）。

schema 侧把 `"join-settlement"` 从「尚未实现」那一支挪到实现里，并沿用
`RawDialogueOutcome` 已有的「不该有的参数必须没有」纪律：`join-settlement`
不认 `flag`。

内容哈希：`write_dialogue_outcome` 加判别值 `1`（**往后接，不挪 `SetFlag`
的 `0`**），`CONTENT_HASH_ALGORITHM_VERSION` 递增一档。

### 3.2 结算侧需要「说话人是谁」——批次 2 第 1 条裁定就此反转

批次 2 的第 1 条临时裁定写着：

> `InteractTarget::Talk` 与 `ScreenState::Dialogue` 都不带说话人的
> `EntityId`……**反转成本一行**：批次 4/5 的 `give-item`/`open-trade`
> 真的需要「给谁」时把它加回来，那时它从第一天起就有消费者。

**本批就是那一刻**（比预告的批次 4 早一批）：`join-settlement` 要问说话人
的 `home`。因此这一条按它自己写好的反转条件反转，四处各加一个字段：

```
InteractTarget::Talk { speaker: EntityId, profession, dialogue }
PlayerCommand::OpenDialogue { speaker, dialogue }
ScreenState::Dialogue { speaker, node, cursor }
Intent::DialogueChoose { actor, speaker, node, option }
```

**为什么不重新查一次「这一格上站着谁」**：会话屏是模态屏，但玩家可以
在一次会话里走到另一个节点、而节点切换不重新扫地面。更硬的一条是
`resolve` 侧：它拿到的是一条 `Intent`，手上根本没有「玩家按空格时朝的
哪一格」这份信息。把说话人带进 `Intent` 是唯一不需要在结算层重建输入层
上下文的做法。

### 3.3 `resolve` 侧：五道闸门，写入走 `apply`

```
1. 发起者还在世界里（既有）
2. 这个节点的第 option 条选项查得到（既有）
3. 条件此刻仍然全部满足（既有，规格 7.2）
4. 说话人还在世界里，且他的 home 是 Some
5. world.factions.faction_of(home) 查得到一个势力
```

任何一道不过 → 这一条后果**静默产出零效果**（不是 panic，也不是一条什么
都不做的效果），与既有四道闸门同一条纪律。

产出：

```rust
Effect::AddAffiliation {
    entity: actor,
    affiliation: Affiliation {
        kind: AffiliationKind::Faction,
        org: OrgRef::Instance(faction),
        standing: JOIN_SETTLEMENT_STANDING,
    },
}
```

**不产 `Effect::ScheduleNext`**（所有者裁定第 2 条：对话不消耗回合）。
本批要有测试钉死这一条，与批次 2 那条并存——批次 2 那条测的是
`SetFlag` 一支，`match` 加了新变体之后它不再覆盖新的这一支。

### 3.4 `apply` 侧：`Effect::AddAffiliation`

- 已经有一条 `(kind, org)` 完全相同的归属 → **整条静默不做**（不叠加、
  不刷新）。理由：本批只有「加入」这一个生产者，「再加入一次该怎样」
  是一次数值设计决定，没有裁定；不做是最保守、最容易反转的那一档。
- `standing` 在写入时 clamp 到 `[-STANDING_FULL, STANDING_FULL]`。

### 3.5 `standing` 常量落在哪

所有者裁定第 5 条：**加入据点 +250，满值 1000**。

| 常量 | 落点 | 理由 |
|---|---|---|
| `Affiliation::STANDING_FULL: i32 = 1000` | `ll-world/src/entity/affiliation.rs` | 「满值」是 `standing` 这个字段本身的量纲，不是对话独有的；clamp 的唯一执行点在 `apply` |
| `JOIN_SETTLEMENT_STANDING: i32 = 250` | `ll-sim/src/dialogue.rs` | 「加入据点给多少」是这条**后果**的数值，与后果同住 |

`mods/lostland/dialogues.json5` 里那条 `standing-at-least: 250` 是内容作者
写的阈值，**刻意不指向常量**——内容里的数字与引擎常量是两回事，让内容
去引用引擎常量就是把数值决定权从内容作者手里拿走。两者相等是这一批内容
的设计选择，本批用一条测试把「加入之后那条 `>= 250` 的选项真的出现」钉住，
而不是靠两处数字长得一样。

---

## 四、C：两条谓词的端到端验证

`affiliated` / `standing-at-least` 在批次 1 就写好了，但**至今没有任何东西
能让它们为真**（`build_player_agent` 写死 `affiliations: Vec::new()`）。

要验的三条（都在 `crates/ll-game/tests/dialogue_session.rs`，走真实
`mods/` 内容与真实 `Catalog`）：

1. **加入前**：管理者开场白里「我想在这里落脚」（`not-affiliated`）**在**、
   「我该做些什么？」（`affiliated`）**不在**。
2. **加入**：选中第一条，`Intent::DialogueChoose` 经 `TurnEngine` 落地。
3. **加入后**：「我想在这里落脚」**消失**，「我该做些什么？」**出现**；
   并且玩家身上真的多了一条 `AffiliationKind::Faction` 归属，`standing`
   恰好是 250。
4. **反向**：`standing-at-least` 在阈值高于 250 时**仍然不出现**——
   用一条 `standing-at-least { faction, 251 }` 直接对求值函数断言，证明
   那条谓词真的在比大小，而不是退化成了「有没有这类归属」。
   另外，`steward_duties` 里那条 `>= 250` 的选项在**加入前**不出现
   （没有任何 faction 归属 ⇒ `best_standing` 是 `None`）。

**防假绿**：断言的是**具体中文文案**（真实 `Catalog`，不是空 `Catalog`），
且每条断言之前先断言被断言的对象存在（那位管理者真的匹配到了对话、
玩家真的没有 faction 归属）。

---

## 五、黄金基准、存档 schema、内容哈希（收尾回填）

见第十节。

---

## 六、ADR 0022 反例验证（每条实测，先跑基线）

计划要验的：

| # | 改坏什么 | 预期红的是 |
|---|---|---|
| ① | `build_npc_agent` 的 `home` 改成 `None` | 「物化出来的 NPC 带着自己那座据点的号」+ 加入那条 e2e |
| ② | `write_optional_world_id(&mut hasher, agent.home)` 那一行去掉 | `Agent.home` 的哈希敏感性测试 |
| ③ | `resolve_dialogue_choose` 的 `JoinSettlement` 一支补一条 `Effect::ScheduleNext` | 「加入据点不消耗回合」 |
| ④ | `apply` 的 `AddAffiliation` 一支去掉写入 | 加入那条 e2e + `standing` 断言 |
| ⑤ | `apply` 的 clamp 去掉 | 「standing 写入被夹到满值」 |
| ⑥ | `resolve` 里的第 5 道闸门（`faction_of`）改成 `unwrap_or(说话人的 home)` | 「查不到势力时零效果」 |
| ⑦ | schema 的 `join-settlement` 一支改回报错 | 内容装载测试（本体 `dialogues.json5` 装不进来） |

每一条都要确认**红的原因确实是预期的那个**，不是别的什么顺手红了。

---

## 七、提交划分（三个，中文提交信息）

1. `feat(ll-world): Agent.home 字段与存档 schema 升版`
2. `feat(ll-sim): join-settlement 后果与归属写入效果`
3. `test(ll-game): 加入据点的端到端验收与文档回填`

---

## 八、纪律核对（交接文档第一节九条）

- 第 1 条：三个关键常量全部 grep 自取 ✅
- 第 2 条：两条基准要重冻，四步一步不少 ✅
- 第 3 条：独立工作树 `wt-dialogue3`，不碰 `LostLand` 与 `wt-adrfix` ✅
- 第 4 条：`run_all.sh` 必须 exit 0，报告自己跑的改前/改后测试数 ✅
- 第 6 条 / ADR 0022：六节那七条 ✅
- 第 9 条：回填 `knowledge/design/dialogue-system.md` 八节批次 3 那一行 ✅

---

## 九、规格没裁定、本批临时选的做法

收尾回填，见第十一节。

---

## 十、落地实测（收尾回填）

### 10.1 三条黄金基准：**两条重冻、一条不动**，全部有证据

| 基准 | 结果 |
|---|---|
| `crates/ll-world/tests/determinism.rs` `EXPECTED_WORLD_DIGEST` | **没动** |
| `crates/ll-sim/tests/replay.rs` `EXPECTED_REPLAY_DIGEST` | **重冻** |
| `crates/ll-game/tests/populated_determinism.rs` `EXPECTED_POPULATED_WORLD_DIGEST` | **重冻** |

两条重冻各走完四步（值与逐步证据写在两个常量各自的文档注释里，不在这里
留副本——那正是交接文档第〇节点名过三次的那种漂移副本）：

1. 基线红，记下 left/right；populated 那条的**七条存在性断言全部在摘要
   断言之前通过**（panic 落在 `assert_eq!(world.hash(), ..)` 那一行）。
2. **只**把 `WorldState::hash` 里那一行
   `write_optional_world_id(&mut hasher, agent.home)` 临时注掉，本批其余
   改动（新字段本身、八十余处构造点、`home: Some(profile.home)`、
   schema 5 → 6）**全部保留** —— 两条**都绿**，摘要**精确等于**各自的
   旧常量。
3. 恢复那一行。
4. 两个彼此独立的 `cargo test` 进程都给出同一对新值。

**`EXPECTED_WORLD_DIGEST` 没动的证伪**（不是「跑了一遍没红就算」）：

- **对照组**：第 ② 步注掉那一行时，另外两条**从红转绿**——说明这一行
  确实是本批唯一改变哈希的地方，而这一行对 `EXPECTED_WORLD_DIGEST`
  从头到尾没有影响。
- **结构性理由**：那一行住在 `for agent in self.actors.iter()` 循环体内，
  而 `determinism.rs` 的世界零 `actor`（`WorldState::new` 之后一行不动、
  零 `actors.spawn`）。这条局限**不是本批推断出来的**，是该常量自己的
  文档与 `populated_determinism.rs` 模块文档 A 表第二行早就实测记录过的
  同一件事。

### 10.2 七条存在性断言：**仍然全绿**

`populated_determinism.rs` 的七条（已物化据点非空、实体数 > 1、地面物品
非空、存在 `placed` 家具、存在 `Owner::Faction` 的地面物品、势力表非空、
存在 `affiliations` 非空的 `Agent`）在重冻前后都通过——重冻那一次的
panic 落在它们**之后**的摘要断言上，世界没有变空。**一条都没有重冻、
一条都没有删。**

### 10.3 内容哈希与存档 schema

- `CONTENT_HASH_ALGORITHM_VERSION`：**30 → 31**，说明段写在常量文档
  「版本 31（加入据点，对话系统的批次 3）」一节。
- `CURRENT_SCHEMA_VERSION`：**5 → 6**，说明段写在常量文档
  「5 → 6」一节；`scripts/ci/save_body_shape.json` 已 `--bless`
  （门禁自己报的是 `Agent：字段序列变了（35 → 36 个字段）`）。

### 10.4 老存档兼容性：**不兼容**，两条实测证据

**没有**给 `Agent::home` 加 `#[serde(default)]`——加了也是空操作，加了
反而会让下一个人以为老存档还读得回来。

1. `crates/ll-world/src/entity/agent.rs`
   `少一个末尾字段的旧形状用postcard解不回新形状`：`home` 是声明顺序上
   最后一个字段且 `None` 恰好一字节，「砍掉末尾一字节」就是旧形状的
   **精确**字节流；解码必须报 `postcard::Error::DeserializeUnexpectedEnd`
   （断言的是**具体错误**，不是「反正 Err」）。对照组：同一份完整字节流
   往返必须无损。
2. `crates/ll-content/src/save_file.rs`
   `加入据点批次之前的老存档被明确拒绝而不是静默按新布局误解析`：写一份
   `schema_version = 5` 的存档，读档必须是
   `Rejected(SchemaMigrationGap { from: 5 })`。

### 10.5 ADR 0022 反例验证：十条实测，含**一条「改坏了它不红」**

每一条都确认了**红的原因确实是预期的那个**（下表「红在哪」一列）。

| # | 改坏什么 | 结果 | 红在哪 |
|---|---|---|---|
| ① | `build_npc_agent` 的 `home: Some(profile.home)` → `None` | **红** | `物化出来的npc带着自己那座据点的号`：「物化出的 NPC 必须知道自己是哪座据点的人」 |
| ② | `WorldState::hash` 里 `write_optional_world_id(.., agent.home)` 那一行注掉 | **红** | `所属据点变化会改变世界哈希`：两个只差 `home` 的世界算出同一个哈希 |
| ③ | `join-settlement` 那一支补一条 `Effect::ScheduleNext` | **红** | `加入据点不消耗回合`：`next_action_at` 从 `Tick(0)` 跳到 `Tick(100)` |
| ④ | `apply` 的 `AddAffiliation` 一支不 `push` | **红** | `加入据点之前那条选项不出现加入之后出现`：「加入之后必须有一条势力归属」 |
| ⑤ | `apply` 的 `clamp_standing` 去掉 | **红** | `addaffiliation挂上归属夹紧声望且不重复叠加`（1001 ≠ 1000）与 `addaffiliation的负声望夹到负满值`（−1007 ≠ −1000） |
| ⑥ | `faction_of(home)?` → `faction_of(home).unwrap_or(home)` | **红** | `据点查不到势力时join_settlement零效果`：产出了一条指向 `WorldId(900)` 的归属 |
| ⑥b | `faction_of(home)?` 整个换成 `home`（拿据点号冒充势力号，规格 5.1 那条已作废的变通） | **红** | `选中join_settlement的选项产出一条指向势力的归属`：「指的必须是**势力**号，不是据点号」 |
| ⑦ | schema 的 `join-settlement` 挪回「尚未实现」那一支 | **红** | 本体内容整份装不进来 ⇒ `站着的npc让交互列表多出一行对话` 报 `lostland:steward_greeting 必须已注册` |
| ⑧ | `condition_holds` 的 `StandingAtLeast` 退化成 `best_standing(..).is_some()`（丢掉比大小） | **红** | `standing不够时那一行仍然不出现` 的**第 2 半**：「阈值高于 250 时必须不满足」 |
| ⑨ | `join_settlement` 的 `speaker.home?` → `unwrap_or(某个号)` | **红** | `说话人没有所属据点时join_settlement零效果` |
| ⑩ | `apply` 的 `already` 那道闸门去掉 | **红** | `addaffiliation挂上归属夹紧声望且不重复叠加`：归属变成两条 |

**如实登记一条「改坏了它不红」**（本节最要紧的一行）：

| ⑪ | `write_dialogue_outcome` 里 `JoinSettlement` 的 `write_u64(1)` 改成 `write_u64(0)`（**与 `SetFlag` 撞判别值**） | **不红** |
|---|---|---|

原因不是测试写坏了，是**今天观察不到**：`SetFlag` 写「判别值 + 标识符」，
`JoinSettlement` 只写判别值，两条字节流长度本来就不同，撞判别值也分得开。
`JoinSettlement` 是今天**唯一一个不带参数的变体**，它的判别值要等批次 5
的 `open-trade`（同样不带参数）落地那天才成为真的守卫。

> **【2026-08-31 更正（批次 29，对话系统的批次 4）】上面这段话的**结论**
> 已经兑现，但比预告的早一批、而且是**另一对**变体。**
> `complete-quest` / `give-item` 的载荷形状完全相同（判别值 + 一条反查出来
> 的标识符），撞号之后两条字节流一模一样。实测：把 `GiveItem` 的
> `write_u64(3)` 改成 `write_u64(2)`（与 `CompleteQuest` 撞号），
> `后果种类不同的两个对话节点摘要不同` **当场红**
> （left == right == `2181073775456014323`）。
> **判别值这条纪律从此有了真守卫，不必再等批次 5。**
> 「`JoinSettlement` 自己的判别值仍然观察不到」这一半原样成立——它至今是
> 唯一一个不带参数的变体，那条「必须往哈希里写点什么」的兜底断言因此保留。
> 本节按纪律第 9 条**原文一字不改**，只在此加标记；更正方与完整证据见
> [批次 29 计划](2026-08-31-batch29-dialogue-quest.md) 与
> `crates/ll-mod/src/content_hash.rs` 里
> `后果种类不同的两个对话节点摘要不同` 的文档注释。

**发现之后做了两件事**：① 把这条「不红」连同原因写进
`后果种类不同的两个对话节点摘要不同` 的文档注释（不是悄悄改成一条会绿的
断言）；② 补一句**真的咬得住**的直接断言——`write_dialogue_outcome` 对
`JoinSettlement` 必须往哈希里写点什么，整支空过时它当场红（实测：
`14695981039346656037 == 14695981039346656037`）。判别值纪律照旧不松。

### 10.6 主动防治的假绿形状

- **不用空 `Catalog`**：`dialogue_session.rs` 的 `真实文案()` 走生产路径
  装载真实 `assets/locales`，断言的是**具体中文文案**（空 `Catalog` 下
  `resolve` 会退回键名，「文案 ≠ 键名」那种写法恒绿）。
- **先断言被断言的对象存在**：加入那条 e2e 有两条对照组前提——
  「加入之前玩家一条势力归属都没有」、「东侧那一格真的匹配出了一行对话
  且说话人就是我们放的那个」。少了后者，「那一行不在」会因为整块列表
  为空而恒真。
- **`所属据点变化会改变世界哈希` 带前提断言**：改之前两个世界必须哈希
  相同，否则 `assert_ne!` 可能是因为别的什么不同才绿的。
- **`物化出来的npc带着自己那座据点的号` 不止断言「非 `None`」**：还要求
  至少一个 NPC 的 `home` 正是我们挑的那一座——只验非 `None` 的话，一个
  恒返回固定野号的实现照样全绿。
- **`加入据点不消耗回合` 先断言效果真的落地了**（归属数为 1），否则
  「时钟没动」可能只是因为什么都没发生。

### 10.7 门禁与测试数

- `bash scripts/ci/run_all.sh` **EXIT=0**。
- `bash scripts/ci/run_tests.sh`：改前 **2951 通过 / 127 个二进制 / 0 忽略**，
  改后 **2967 通过 / 128 个二进制 / 0 忽略**（+16 条，+1 个二进制：
  新的 `crates/ll-sim/tests/affiliation_apply.rs`）。
- 行数棘轮：**先拆、再 bless**。
  - **拆**：`Effect::AddAffiliation` 的三条 `apply` 侧测试从
    `crates/ll-sim/src/apply.rs` 的内联测试模块搬进新文件
    `crates/ll-sim/tests/affiliation_apply.rs`（只用公开入口）。
    `apply.rs` 因此从 +91 降到 +19，留在那里的只有实现——**这正是
    它自己那条预算理由说的东西**（文件长度就是 `Effect` 变体数，实现
    不能拆，测试可以）。搬家时顺手补了第三条
    `不同势力的归属各挂各的`：没有它，一个「已经有这一类归属就不再挂」
    的实现照样能让另外两条全绿，而那会让玩家永远只能加入第一个势力。
  - **bless 九个文件**，每个都在 `reason` 里追加了本批的具体理由。
    其中**六个是加一个结构体字段的机械代价**、不可拆分：`remap.rs`
    (+2，穷尽解构必须为新字段写一行——那正是它的设计)、`world.rs`
    (+1)、`roster.rs` (+1)、`resolve_tests.rs` (+7)、
    `derive_stats_resolve.rs` (+2) 全是 `home:` 那一行；`apply.rs`
    (+19) 是新 `Effect` 分支本身。另外三个是测试与被测对象同住：
    `state.rs` (+34)、`content_hash.rs` (+50)、`save_file.rs` (+20)
    ——三处的新测试都依赖各自文件里的**私有**夹具或私有函数
    （`test_world_with_one_agent`、`write_dialogue_outcome`、
    `temp_path`/`sample_header`），搬出去要把夹具复制一份，而「同一族
    测试两份夹具」正是本仓库反复付过代价的形状。

---

## 十一、规格没裁定、本批临时选的做法

逐条列出，都取了「最保守、最容易反转」的那一种。

1. **`Affiliation::STANDING_FULL` 的负方向取 `-1000`（对称）。**
   所有者只裁定了「满值 1000」这一半。字段文档自 P3 起写着「千分比，
   负值表示敌对」，千分比这个词本身给出的就是一条对称标尺；非对称需要
   一条独立的设计理由，今天没有。**反转成本：一处 clamp。**
2. **clamp 的唯一执行点在 `apply`，`Affiliation` 的构造函数不夹。**
   纯数据结构不夹，测试与内容表才造得出越界值来验证夹紧本身。规格没说
   夹在哪。
3. **同一条 `(kind, org)` 再加一次 → 整条静默不做**（不叠加、不刷新
   `standing`）。「再加入一次该怎样」是一次数值设计决定，没有裁定。
   **反转成本：`apply` 里那三行。**
4. **`join-settlement` 不带任何参数。** 规格 5.1 只说「加入说话人所属
   据点」，没说这条声明长什么样。不带参数是唯一写得出来的形状——
   `WorldId` 是世界生成期分配的号，内容文件里根本写不出来。
5. **说话人没有 `home` / 据点查不到势力 → 零效果，不报错、不反馈。**
   规格没说这两种情况怎么办。与既有四道闸门同一条纪律（空效果经
   `TurnEngine` 会变成「按了但什么都没发生」，玩家收到一句通用反馈）。
6. **批次 2 第 1 条裁定的反转方式**：`speaker` 加在**四处**
   （`InteractTarget::Talk`/`PlayerCommand::OpenDialogue`/
   `ScreenState::Dialogue`/`Intent::DialogueChoose`），而不是只加在
   `Intent` 上再从别处现查。规格没说。判据是「`resolve` 手上只有一条
   `Intent`，没有输入层上下文」。
7. **`dialogue-steward-join` 的文案改写，而不是新增一对键。**
   那个节点原来的台词是婉拒（「先做点像样的事，再来找我谈」），加入
   真的发生之后它不再成立。改写是零新增内容 id、零新增 `.ftl` 键的那一
   档；新增一个 `steward_joined` 节点会留下一个没人指向的孤儿节点。
8. **`mods/lostland/dialogues.json5` 里那条 `standing-at-least: 250`
   仍然写字面量**，不指向 `JOIN_SETTLEMENT_STANDING`。让内容去引用引擎
   常量等于把数值决定权从内容作者手里拿走。两者相等由端到端测试钉住。
9. **`Agent::home` 加在结构体末尾。** postcard 按声明顺序吃字节，末尾
   追加是唯一不会让既有字段错位的位置——但这**不是**兼容性论证（版本号
   已经把老存档明确拒掉了），只是让「旧形状 = 砍掉末尾一字节」这个
   实测手法成立。
10. **`remap_agent` 里 `home: _`。** `WorldId` 不依赖 mod 装载顺序，与
    `remembered_id` / `OrgRef::Instance` 同一条既有处理。规格九节第 3 条
    那道题（世界生成变了怎么办）**仍然开着**，本批只把它如实写进两处
    字段文档，没有替所有者做决定。
