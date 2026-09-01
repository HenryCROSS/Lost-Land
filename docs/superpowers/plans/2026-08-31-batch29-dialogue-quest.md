# 批次 29：对话系统的批次 4——任务（`complete-quest` 与 `give-item`）

**工作树** `wt-dialogue4`（从 `origin/main` 的 `11eeea5` 分出）。
**规格** `knowledge/design/dialogue-system.md` 五节 5.2、八节分批表第 4 行。
**上游交接**
`docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md` 第七节挂载点表「批次 4」一行；
`docs/superpowers/plans/2026-08-31-batch26-dialogue-join.md` 第十一节的十条临时裁定
（本批在同一套约定下继续）。

---

## 〇、开工基线（自己跑的，不抄任何文档）

```
bash scripts/ci/run_tests.sh   → EXIT=0，2970 通过 / 129 个二进制 / 0 忽略
```

三个关键常量 grep 自取（**本文档不留副本**，只记「改前是多少」这个事实：
存档 schema 与内容哈希版本各是一个数，三条黄金基准各是一个数）：

```bash
grep -n "pub const CURRENT_SCHEMA_VERSION" crates/ll-content/src/save_file.rs
grep -n "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs
grep -rn "const EXPECTED_" crates/ll-world/tests/determinism.rs \
  crates/ll-sim/tests/replay.rs crates/ll-game/tests/populated_determinism.rs
```

---

## 一、范围

| | 内容 | 提交 |
|---|---|---|
| **A** | `complete-quest` 后果：一条新的 `DialogueOutcome` 变体，`resolve` 侧**调既有的 `ll_sim::quest::mark_quest_completed`** | 1 |
| **B** | `give-item` 后果：NPC 把自己背包里的一件东西交给发起者，**含 owner 校验硬前置** | 2 |
| **C** | 内容与端到端：本体对话上挂出一条走得通的任务链 | 3 |

**不做**：`open-trade`（批次 5）、NPC 姓名（批次 6）、任务日志 UI、
任务的「已接取／进行中」这一档状态（规格 5.2 明写今天不存在，本批不造）、
新的任务节点（理由见 4.3）。

---

## 二、A：`complete-quest` 的落地形状

### 2.1 数据侧

```rust
pub enum DialogueOutcome {
    SetFlag(NamespacedId),
    JoinSettlement,
    CompleteQuest(ContentIndex),   // 判别值 2
    GiveItem(ContentIndex),        // 判别值 3
}
```

`CompleteQuest` 携带的是 **`ContentIndex`**，不是 `NamespacedId`：任务有内容表
（`ll_mod::quest::QuestTable`），跨表引用走 `required_id`（只 get 不 intern），
拼错的任务 id 在**装载期**当场报错并点名文件——这与
`DialogueCondition::QuestCompleted` 逐字同办。`SetFlag` 之所以是
`NamespacedId`，是因为对话标志**没有内容表**，两者不是同一档。

因此 `RawDialogueOutcome::resolve` 从「不收 `Registry`」变成 **收 `&Registry`**
（批次 26 的注释预告过这一步：「批次 4/5 的 `complete-quest`/`give-item`
要查表时再把它加进来」）。

### 2.2 `resolve` 侧：调既有函数，不重写一份完成逻辑

```rust
DialogueOutcome::CompleteQuest(quest) => {
    if let Some(id) = content_ids.id_of(*quest) {
        writes.push(crate::quest::mark_quest_completed(actor, id));
    }
}
```

**ADR 0021**：任务「已完成」这件事的存储形状（命名空间 + `quest_progress:<id>`
键 + `Int(1)`）只有一处真相源，就是 `ll_sim::quest`。在这里重抄一遍
`ModStateWrite { key: format!("quest_progress:{…}") }` 正是那条 ADR 要拦的形状——
两处会各自漂移，而漂移时没有任何东西会报错。

`ContentIndex → NamespacedId` 的反查走**既有**的 `ContentIdLookup`
（批次 1 为 `quest-completed` 谓词建的那条窄接口），不新加一条依赖。
反查不到 ⇒ 这一条后果零效果（与既有闸门同一纪律）。

产出的 `ModStateWrite` 与 `SetFlag` 那一支**攒进同一条 `Effect::SetModState`**
——`mark_quest_completed` 返回的就是一条 `ModStateWrite`，形状天生对得上。

**不产 `Effect::ScheduleNext`**（所有者裁定第 2 条）。批次 3 的教训：批次 2 那条
「不消耗回合」的测试走的是 `set-flag` 一支，加了新变体之后它不再覆盖新的这一支。
**本批两个新变体各自带一条自己的「不消耗回合」测试。**

### 2.3 不校验前置、不校验「是不是已经完成过」

`ll_sim::quest::kill_progress_effects` 在标记完成前会检查前置任务全部完成、
且这条任务尚未完成。**对话这一侧本批两条都不做**，理由与「最保守、最容易反转」
同一档：

- 「前置没完成能不能靠对话跳过」是一次**玩法**裁定，规格没写。加一道今天没有
  任何内容需要的闸门，就是替所有者做决定；
- 「已经完成了再写一次」是幂等的（同一个键写同一个 `Int(1)`），不会产生第二份
  状态，也不会叠加任何数值。

两条都如实登记在第九节，反转成本各是 `resolve` 里的一个 `if`。

---

## 三、B：`give-item` 的落地形状

### 3.1 语义：NPC 把**自己背包里真的有的**一件东西交给发起者

```rust
GiveItem(ContentIndex)   // 只带「哪一种物品」
```

**不带 `count`**（规格 5.2 的草图里写的是 `{ item, count: N }`，本批**刻意收窄**，
第九节登记）：一次交一件，`apply` 侧因此能直接复用**既有**的
`Effect::ConsumeInventoryItem`（「数量减一，减到零时整条堆移除」——正是这个语义，
一个字都不用改）。支持 N > 1 需要拆堆机械，而今天一条内容都不需要它（YAGNI）。
**反转成本**：schema 加一个 `#[serde(default = "one")] count`，既有内容一个字不改。

顺带的好处：`CompleteQuest` 与 `GiveItem` 两支的载荷形状**完全相同**
（判别值 + 一个 `ContentIndex`），字节流长度相等——**判别值撞号这一次才第一次
成为真守卫**，见第六节 ③。

### 3.2 owner 校验硬前置落在 `resolve`（C1）

`Effect::TransferOwnership` 的文档给未来三个调用方立的那条硬前置：

> 三种合法转移的 `resolve` 都**必须**校验「发起转移的一方确实是这堆物品当前的
> `owner`」（`Owner::Unowned` 的物品谁都能转移，因为没有人的权益受损）。

落到对话上就是 **NPC 不能把不属于自己的东西送人**。判据：

| 那一堆的 `owner` | 能不能送 | 理由 |
|---|---|---|
| `Owner::Unowned` | ✅ | 没有人的权益受损（效果文档原文） |
| `Owner::Npc(id)`，且 `id == 说话人的 remembered_id` | ✅ | 是他自己的 |
| `Owner::Npc(别人)` / `Owner::Player` / `Owner::Faction` / `Owner::Shop` | ❌ | 不是他的 |

`Owner::Faction`（据点／势力公产）**这一批一律拒**：「管理者能不能把据点的公产
发给你」是一次玩法裁定，不做是最保守的一档，反转成本是这张表加一行。

校验落在 `resolve` 侧、在产出任何 `Effect` **之前**——C1：`apply` 是唯一写入口，
它只机械执行，一句判断都不做。

### 3.3 校验失败时的行为：**零效果**

选项照常显示（显示与否是内容作者用 `conditions` 表达的事，条件清单里今天没有
「这个 NPC 有没有这件东西」这条谓词，加一条需要它自己的真实内容用例）；
选中之后产出**空效果**，经 `TurnEngine` 变成
`PlayerTurnOutcome::Nothing`——「按了但什么都没发生」，玩家收到一句通用反馈。

这与 `resolve_dialogue_choose` 既有的四道闸门、以及 `join_settlement` 的第 4/5
道闸门**逐条同一纪律**：不 panic、不产出一条什么都不做的效果、不新增一条反馈键。
它同时是最容易反转的一档：将来要加一句专门的反馈，加的是一条 `Feedback`，
这里的零效果分支原样还在。

### 3.4 五道闸门

1. 发起者还在世界里（既有）；
2. 选项查得到、条件此刻仍然满足（既有）；
3. **说话人还在世界里**；
4. **说话人背包里真的有一堆 `def == item`**（第一条匹配，与
   `RemoveFromInventory`/`ConsumeInventoryItem` 的既有定位纪律相同）；
5. **owner 校验通过**（3.2 那张表）。

### 3.5 产出的效果与「为什么不是 `Effect::TransferOwnership`」

```rust
vec![
    Effect::ConsumeInventoryItem { actor: speaker, def, durability },   // 离开给方
    merge_into_inventory_effect(收方, actor, 交出的那一件, items),      // 进入收方
]
```

**上游交接表预告的是「`give-item` 是 `Effect::TransferOwnership` 的第一个调用方」，
本批实测之后反转这一条，理由写在这里，并写回被更正方**（纪律第 9 条）。

`Effect::TransferOwnership` 只改一堆物品的 `owner` 字段，**不搬运它**。一次赠送
必须同时做两件事：搬运 + 改主。用现有效果集组合，只有两种排法，两种都不成立：

| 排法 | 为什么不成立 |
|---|---|
| `TransferOwnership`(说话人那一堆) → 搬运 | ① 若只交一件而他还剩几件，那几件会被一起改成收方的（**可观察的错**）；② 若整堆交出，那一次写入紧接着被搬运覆盖，**在同一批效果里不可观察** ⇒ 它是一条测不出来的死效果，改坏了不会红 |
| 搬运（保留原主）→ `TransferOwnership`(收方那一堆) | `(holder, def, durability)` **定位不到刚收下的那一堆**：收方若已经有一堆同 `def` 同耐久、但归属不同的东西，`position` 命中的是**前一堆**（`transfer_item_ownership` 取第一条匹配），新收的那一堆仍然挂着原主。而这恰恰是常见情形（玩家捡来的同种物品是 `Owner::Player`，NPC 的出生装备是 `Owner::Unowned`） |

因此本批采纳 `resolve_pick_up` 已经在用的那条既有手法：**归属由 `resolve` 算好，
写进搬运效果携带的那一堆里**（`pick_up_owner` 就是那个算子）。赠送与拾取共用的
那一半——「谁拿到手就归谁」——抽成 `ll_sim::ownership::holder_owner`，
`pick_up_owner` 在「原本就有主」的早退之后调它，赠送路径直接调它。
**不复用 `pick_up_owner` 本身**：它的第一句是「原本有主就保持原主」，那是拾取
（也是将来盗窃判定的挂载点）的语义；赠送是一次**合法转移**，转移之后归收方，
两者在这一点上判据不同，共用会让将来的盗窃判定把赠送也标成赃物。

`Effect::TransferOwnership` 因此**仍然没有调用方**，它的文档里那句
「给未来三个调用方的一条硬前置」原样成立（本批把 owner 校验按它的原话落地了），
只把「对话赠送会是第一个调用方」这半句更正掉，并指向本文档。

---

## 四、C：内容——一条走得通的任务链

### 4.1 链条

批次 1 写下的管理者那段对话已经把形状留好了：`ask_work`（`quest-not-completed`）
与 `ask_reward`（`quest-completed`）本来就是一对。本批只补两条**带后果**的选项：

```
steward_root ──ask_work（quest-not-completed main_quest_1）──► steward_work
                                                                  │
                       report（outcomes: complete-quest main_quest_1）
                                                                  ▼
steward_root ──ask_reward（quest-completed main_quest_1）──► steward_reward
                                                                  │
                       take（outcomes: give-item roast_meat）
                                                                  ▼
                                                            背包里多一份口粮
```

**零新增节点、零新增内容 id**：两条选项挂在既有节点上，`next` 指回
`steward_root`。这一条同时保证**注册表里一个索引都不平移**（第五节的结构性理由）。

### 4.2 奖励物品为什么是 `lostland:roast_meat`

`give-item` 的硬前置要求那件东西**真的在 NPC 背包里**。NPC 的背包今天唯一的
生产者是 `ll_mod::roster::build_npc_agent` → `starting_inventory`（种族的
`starting_items`）→ `outfit_decision` 把能穿的挪进装备栏，**剩在背包里的才是
可赠送的**。`mods/lostland/races.json5` 里人类带 `roast_meat ×3`、矮人带 ×1，
而 `cultures.json5` 的 farmstead/harbour 两支人类权重最高——所以这是本体今天
**唯一一件「管理者大概率真的拿得出来」**的东西。

**如实登记的缺口**：管理者若恰好是精灵（出生装备里没有烤肉），这条后果零效果，
玩家会得到「按了但什么都没发生」。这不是本批引入的缺陷，是规格 5.2 已经写明的
那条边界——「一个 NPC 凭空变出奖励物品是**另一件事**，不能靠转移假装」。
把它变成「稳定拿得出来」需要 `NpcProfile` 有一份内容声明的背包，那是另一批。

### 4.3 不新增任务节点

`mods/lostland/quests.json5` 一个字不动，`complete-quest` 指向既有的
`lostland:main_quest_1`。三条理由：

1. **注册表索引不平移**：`quests.json5` 排在 `CONTENT_FILES` 中段，往它里面加
   一条会让其后**每一张表**的 `ContentIndex` 整体后移——那是批次 18 文件头写死
   的那条纪律，代价是三条黄金基准全部要重冻，而这批一条内容都不值那个价；
2. `crates/ll-mod/tests/base_quest_graph_completable.rs` 这道门要求**本体每一条
   任务都能被今天真实在跑的判定器完成**。一条「只能靠对话完成」的新任务会让它
   假红，除非同批把对话这条完成路径并进那道门的判据——那是一次独立的门禁扩面，
   与本批的两条后果没有依赖关系；
3. 更要紧的一条：**本批刻意不碰「打通本体任务链是否必须杀一个人类平民」那个
   悬而未决的问题**。`lostland:branch_b` 的 `target: "lostland:human"` 是此前
   某批次自己定的口径并**主动标记为待所有者裁定**（`quests.json5` 文件头
   「branch_b 的内容口径（本批次自己定的，不是所有者裁定）」一节）。本批的任务链
   **只碰 `main_quest_1`（杀 3 只哥布林）**，既不动那个选择，也不绕开它另造一条链。

### 4.4 「任务今天只有两态」这件事要写进内容注释

规格 5.2 原文：**今天不存在「接取」这个概念**——任务从「不存在」直接跳到
「已完成」。因此管理者那一行「山道我已经走过一趟了」既是接活也是交差，
`kill-count` 是同一条任务的另一条完成路径，两条写的是**同一个 `mod_state` 键**、
互相幂等。**没有任何东西会校验玩家真的去过山道**——这是任务系统缺「进行中」
那一档的直接后果，不是本批引入的，如实写在 `dialogues.json5` 里。

### 4.5 i18n

`assets/locales/{en,zh-CN}.ftl` **末尾各追加一节**，两条新键：

- `dialogue-steward-report`（「山道我已经走过一趟了。」）
- `dialogue-steward-take_reward`（「那我就收下了。」）

外加把 `dialogue-steward-reward` 那一句改写成「东西当场交到你手上」——批次 3
第 7 条裁定同一手法（改写既有台词，而不是新增一个没人指向的节点）。

两条新键都进 `crates/ll-ui/tests/i18n_text_width.rs` 的分类表（`dialogue-` 规则：
模态屏内容宽、散文 2 行）。**预算很紧**，新文案更长就改文案，不放宽预算。

---

## 五、三条黄金基准：**预期一条都不动**，要给证伪不是「跑了没红」

| 基准 | 预期 | 结构性理由 |
|---|---|---|
| `EXPECTED_WORLD_DIGEST` | 不变 | 那个世界零 `actor`、零据点、零物品（该常量自己的文档与 `populated_determinism.rs` 的 A 表已实测记录） |
| `EXPECTED_REPLAY_DIGEST` | 不变 | 回放脚本里没有一条 `Intent::DialogueChoose`；本批不加任何字段、不改任何既有 `Effect` 的 `apply` |
| `EXPECTED_POPULATED_WORLD_DIGEST` | 不变 | 世界生成不跑对话；且本批**零新增内容 id** ⇒ 一个 `ContentIndex` 都不平移 |

**证伪怎么做**（不许当作「改动无害」）：

- **对照组必须是一个已知会红的注入点**，且注入点必须在**生产代码**里，不能写在
  `#[cfg(test)]` 里（本会话有过一次注入点写在 `#[cfg(test)]`、结论与事实完全相反）。
  本批取的对照组：在 `mods/lostland/dialogues.json5` 里**插入一个新节点到文件中段**
  （不是末尾）——它会真的分配一个新 `ContentIndex` 并让其后的索引平移。若这样都不红，
  说明这三条基准根本读不到内容索引，那时候「本批不红」这件事才需要另找理由。
- 加上上表的结构性理由（每一条都指向一处已经实测记录过的既有事实）。

`populated_determinism.rs` 的 **7 条存在性断言**改前改后都必须绿，一条都不重冻、
一条都不删。

---

## 六、ADR 0022 反例验证（每条先跑基线，再改坏，并确认红的原因是预期的那个）

| # | 改坏什么 | 预期红的是 |
|---|---|---|
| ① | `give-item` 的 owner 校验整段去掉 | 「NPC 送不属于自己的东西必须被拦」 |
| ②a | `complete-quest` 那一支补一条 `Effect::ScheduleNext` | 「`complete-quest` 不消耗回合」 |
| ②b | `give-item` 那一支补一条 `Effect::ScheduleNext` | 「`give-item` 不消耗回合」 |
| ③ | `write_dialogue_outcome` 里 `GiveItem` 的判别值 `3` 改成 `CompleteQuest` 的 `2` | 「后果种类不同的两个对话节点摘要不同」——**批次 3 登记过「改坏了它不红」，本批要验它现在红了** |
| ④ | `ll_sim::quest::mark_quest_completed` 改坏（键名换一个） | 本批的 `complete-quest` 端到端测试必须跟着红（证明真的调的是那个函数） |
| ⑤ | 第 4 道闸门（说话人背包里有没有这一堆）去掉 | 「说话人没有那件东西时零效果」 |
| ⑥ | 交出的那一件的 `owner` 不改成收方的 | 「收下之后那一堆归收方所有」 |
| ⑦ | schema 的 `complete-quest`/`give-item` 挪回「尚未实现」那一支 | 本体内容整份装不进来 |
| ⑧ | `ConsumeInventoryItem` 那条效果不产出 | 「给方背包里真的少了一件」 |

**如果某条改坏了它不红**：不粉饰、不把断言改宽，查清原因、如实登记、补一条真的
咬得住的（批次 3 第 10.5 节那一行就是这么处理的）。

**主动防治的假绿形状**（本会话已出现 15 次「全绿但保护不存在」）：

- 不用空 `Catalog`：端到端断言走真实 `assets/locales`，断言**具体中文文案**；
- 每条「那一行不在」之前先断言**被断言的对象存在**（这一格真的匹配出了对话、
  说话人就是我们放的那个、给方真的带着那件东西）；
- 「不消耗回合」先断言**效果真的落地了**，否则「时钟没动」可能只是因为什么都
  没发生。

---

## 七、内容哈希、存档 schema、行数棘轮

- `CONTENT_HASH_ALGORITHM_VERSION` **递增一档**，说明段按既有格式追加：这是
  「已有表的枚举加变体 + 本体内容真的用上了新变体」那一档（同批次 26 那一节）。
- `CURRENT_SCHEMA_VERSION` **不动**：本批不给任何进存档主体的类型加字段。
  `check_save_schema_version.py` 若拦下来，说明改到了不该改的地方——照提示做，
  **不绕过**。postcard 非自描述，`#[serde(default)]` 在那条路径上是空操作，
  本文档不写任何没实测过的兼容性声明。
- 行数棘轮：**先拆再 bless**。`content_hash.rs` 与 `apply.rs`（若涨）在快照里；
  `crates/ll-sim/src/resolve/dialogue.rs`、`crates/ll-sim/src/dialogue.rs`、
  `crates/ll-mod/src/content_schema_dialogue.rs` 不在快照里但受 800 行上限约束。
  新测试优先落在**独立测试文件**里，不往快照内文件塞。
- **不新增 example target**（ADR 0030）。

---

## 八、提交划分（三个，中文提交信息；尽量让每个提交自身是绿的）

1. `feat(ll-sim): complete-quest 后果，复用既有的 mark_quest_completed`
2. `feat(ll-sim): give-item 后果与 NPC 送礼的 owner 校验硬前置`
3. `test(ll-game): 对话任务链的端到端验收与文档回填`

批次 3 报过一处：它的第一个提交单独看过不了 `check_field_consumers.py`。
本批把 schema、内容哈希、`resolve` 与它们各自的测试**放在同一个提交里**，
让每个提交自身可编译可测；做不到的写进提交信息。

---

## 九、规格没裁定、本批临时选的做法（收尾复核，见第十一节）

1. `give-item` **不带 `count`**，一次一件。
2. `complete-quest` **不校验前置任务**。
3. `complete-quest` **不校验「是不是已经完成过」**（幂等）。
4. owner 校验对 `Owner::Faction` / `Owner::Shop` **一律拒**。
5. 校验失败 = **零效果**，不加新反馈键、选项照常显示。
6. **不产出 `Effect::TransferOwnership`**（三节 3.5，反转上游交接表的预告）。
7. 收方的归属由 `holder_owner` 算，**不复用 `pick_up_owner`**（3.5 末段）。
8. 奖励物品取 `lostland:roast_meat`（4.2）。
9. `complete-quest` 指向既有的 `main_quest_1`，**不新增任务节点**（4.3）。

---

## 十、落地实测（收尾回填）

## 十一、规格没裁定、本批临时选的做法（收尾回填）
