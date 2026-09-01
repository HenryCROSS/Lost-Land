# 批次 21：把对话跑通（对话系统的批次 2）

> 前置：批次 1（对话内容表）已落地在 main，计划文档
> `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md`，
> **本批的挂载点是它第七节专门留的**。
>
> 规格：`knowledge/design/dialogue-system.md`（尤其 **6 节**与 **7.2 节**）。
> 所有者裁定：`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节
> **第 1 条**（预选第一项 + 鼠标可点）与**第 2 条**（对话不消耗回合）。
>
> 工作树 `wt-dialogue2`，分支 `wt-dialogue2`。**不碰** `LostLand`（main）
> 与并行的 `wt-buildings`。

## 〇、开工基线（自己跑的，不抄任何文档）

```
crates/ll-world/tests/determinism.rs:351  EXPECTED_WORLD_DIGEST  = 11_270_479_921_196_970_914
crates/ll-sim/tests/replay.rs:984         EXPECTED_REPLAY_DIGEST = 11_222_878_776_777_704_235
crates/ll-mod/src/content_hash.rs:838     CONTENT_HASH_ALGORITHM_VERSION = 28
crates/ll-content/src/save_file.rs:139    CURRENT_SCHEMA_VERSION = 4
```

`bash scripts/ci/run_tests.sh` 改前：**EXIT=0，120 个测试二进制，2886 条通过，
0 条失败**。

## 一、范围

### 做（四件）

- **A** `outcomes` 字段（`RawDialogueOption` / `DialogueOption` / 内容哈希 /
  内容审计），**只实现 `set-flag` 一种后果**。
- **B** 对话进交互列表：`InteractTarget::Talk`，匹配复用批次 1 的
  `ll_mod::dialogue::DialogueTable::match_speaker`。
- **C** 会话 UI：模态屏一块，选项过滤调 `ll_sim::dialogue::all_conditions_hold`，
  预选第一项 + 鼠标点击（复用既有 `ll_game::pointer`）。
- **D** `Intent::DialogueChoose` + `resolve` 侧重新校验 + `set-flag` 写
  `Effect::SetModState`；**不产 `Effect::ScheduleNext`**。

### 不做

`join-settlement`（批次 3）、`complete-quest` / `give-item`（批次 4）、
`open-trade`（批次 5）、NPC 姓名（批次 6）、群体对话 / 语音 / 对话编辑器
（规格八节「明确不做」）。玩家自定义文本（裁定第 8 条）。

## 二、A：`outcomes` 的落地形状

### 2.1 类型住哪

`DialogueOutcome` 放 **`ll-sim/src/dialogue.rs`**，与 `DialogueCondition`
并排——理由与批次 1 把条件放在这里逐字相同（那份计划 4.2）：把声明翻译成
`Effect` 的是 `resolve`，而 `resolve` 在 `ll-sim`；`ll-mod` 只 `use` 它。

```rust
pub enum DialogueOutcome {
    /// 在发起者身上设一条对话标志。
    SetFlag(NamespacedId),
}
```

**单变体枚举不是过度设计**：批次 3–5 各加一支，而枚举形状让
`write_dialogue_outcome` 与 `resolve` 两处的穷尽 `match` 在那一刻逼每一处表态。

写入侧新增 `ll_sim::dialogue::set_dialogue_flag(actor, flag) -> ModStateWrite`，
与 `ll_sim::quest::mark_quest_completed` 逐条同办（值固定 `Bool(true)`，
命名空间取标志自己的）。

### 2.2 schema

`RawDialogueOption` 追加 `#[serde(default)] outcomes: Vec<RawDialogueOutcome>`，
`RawDialogueOutcome { kind: String, flag: Option<String> }`，
`deny_unknown_fields`，`kind` 只认 `"set-flag"`，其余四种报
「批次 3/4/5 才实现」的明确错误——**不静默接受**。

> **`#[serde(default)]` 在 postcard 上是空操作。** 内容表走 JSON5 所以没问题，
> **但这不构成任何存档兼容性声明**（交接文档纪律第 9 条点名的 batch8 先例）。
> 本批不改存档主体形状，见第六节。

### 2.3 内容哈希

`write_dialogue_node_fields` 里**每个 option 的字段流末尾**追加
`outcomes.len()` + 逐条 `write_dialogue_outcome`（判别值从 `0` 起，
新增后果必须往后接）。**追加在末尾**，不插在 `conditions` 之前——并行的
`wt-buildings` 也在改这个文件。

`CONTENT_HASH_ALGORITHM_VERSION` **28 → 29**，归「已有表加字段」那一档，
说明段写在该常量的文档注释里（标题「# 版本 29（对话后果批次）」）。

### 2.4 内容审计

`inspect_dialogue_node` **末尾**追加一行
`auditor.field("DialogueNodeAttrs::options::outcomes", has_outcomes)`。
覆盖由本批新增的本体内容满足（2.5）。

### 2.5 内容

本体 `mods/lostland/dialogues.json5`：给管理者「我想在这里落脚」那条选项加
`outcomes: [{ kind: "set-flag", flag: "lostland:steward_asked_join" }]`，
并在开场白加一条**依赖该标志**的选项（`flag-set`）。这是端到端验收的标的：
**设了旗标之后，依赖它的选项真的出现**。

新键一律加在两份 `.ftl` **文件末尾**（并行冲突预警）。

## 三、B：进交互列表

### 3.1 变体

```rust
InteractTarget::Talk {
    /// 跟谁说：说话人的职业（排版取它的显示名，规格 3.4 的乙案）。
    profession: ContentIndex,
    /// 说哪一段：`match_speaker` 已经裁决完的那一段。
    dialogue: ContentIndex,
}
```

`item_def()` 返回 `None`（门那一支的现成先例）。`interact_row_text` 因此
不写数量；名字走职业显示名，动作键 `hud-interact-action-talk`。

### 3.2 签名

`interact_entries` 今天是 `(&WorldState, TorusPos)`，它够不到对话表、文化表，
也不知道**是谁**在看（敌对判定要发起者）。改成：

```rust
pub struct TalkLookup<'a> { pub dialogues: &'a DialogueTable, pub cultures: Option<&'a CultureTable> }
pub fn interact_entries(world, pos, actor: EntityId, talk: TalkLookup<'_>) -> Vec<InteractTarget>
```

**不做成 `Option<TalkLookup>`**：那会让「同一格、两个调用方、两份行列表」
在类型上成为可能，而玩家按的是「第几行」。既有测试各自建一张空
`DialogueTable`，行为与今天逐字相同。

### 3.3 三条钉死的细节（规格六节）

1. **顺序**：站在这一格上的人由 `ll_sim::resolve::occupant_at` 查
   （它今天是 `pub(crate)`，本批开成 `pub`——**不另写一份**，理由见它自己的
   文档「为什么必须只有这一份实现」）。`Arena::iter_with_id` 由 `Vec` 支撑，
   不碰哈希容器（C5）。
2. **`Talk` 排最前**：一格上同时有人和东西时「跟他说话」几乎总是玩家的意图
   （规格六节第 2 条）。
3. **敌对不列**：`ll_sim::ai_query::declared_hostile(viewer, other, cultures)`，
   不在输入层抄第二份判据（ADR 0021）。

匹配用 `DialogueTable::match_speaker(profession, culture)`——**批次 1 已实现
且有测试的裁决顺序**（culture 优先、平局取最小 `ContentIndex`），本批一个字
都不重写。NPC 的文化取自 `Agent.affiliations` 里那条 `Culture` 归属。

### 3.4 从列表到会话屏

`PlayerCommand` 新增一支 `OpenDialogue { npc, dialogue, node }`——开一块屏
**不是**一个 `Intent`（7.1：会话内的位置是 UI 状态）。`app.rs` 收到它就
`modal.set_screen(Some(ScreenState::Dialogue { .. }))`。

## 四、C：会话屏

### 4.1 形状：**就是既有的模态屏**，不新造一套

`ScreenState::Dialogue { npc, dialogue, node, cursor }`（全 `Copy`）。
复用 `ll_ui::screen::ScreenData`：

| 槽 | 装什么 |
|---|---|
| `title_key` | **节点的 `text_key`**（NPC 这一句）。它本来就是一条 Fluent 键，`Catalog::resolve` 认带命名空间的完整键 |
| `rows` | 过滤后的选项文案，按书写顺序（C5） |
| `cursor` | 预选第一项（裁定第 1 条） |
| `hint_key` | `screen-dialogue-hint` |
| `empty_key` | `screen-dialogue-empty`（一条选项都不显示时，只能退出） |

**不给 `ScreenData` 加字段**：加一个 `lead` 要改十余处构造点，而
「屏的标题就是 NPC 这一句」在这块屏上是准确的语义，不是凑合。

**溢出门禁**（`crates/ll-ui/tests/i18n_text_width.rs` 的 `dialogue-` 一条）
按模态屏内容宽 500px 计量、预算 2 行；本屏就是模态屏、就是那个宽度，
**判据不用改**。选项行会多一个 `"> "` 前缀，但 `panel_width` 是
`max(520, 最长行 + 2×内边距)`，面板跟着变宽而不是溢出。

### 4.2 选项过滤：与 `resolve` 同一个函数

`ll_sim::dialogue::all_conditions_hold(&option.conditions, agent, ids)`，
`ids` 是 `&content.registry`（批次 1 已 `impl ContentIdLookup for Registry`）。
**UI 这一侧不写任何条件判定**，规格 7.2。

行号与选项序号的映射：可见选项的**原始下标**随行一起产出，提交 `Intent`
时传原始下标——过滤后的行号在 `resolve` 那一侧毫无意义，且世界可能已经变了。

### 4.3 预选与鼠标

- 预选：进屏时 `cursor: 0`，与 `Demo::open_menu` 的 `preselected_focus`
  同一条裁定。
- 鼠标：`Demo::resolve_screen_pointer` 已经是**全部模态屏共用**的那条路径
  （行矩形由 `ll_ui::screen::screen_row_rects` 与渲染侧同一个
  `screen_geometry` 产出）。本屏只要落进 `screen_row_texts` 的 `match`，
  hover 高亮、按下移焦点、松开触发**一行代码都不用新写**。
  **不合成任何按键**（ADR 0025）。

## 五、D：`Intent::DialogueChoose` 与 `set-flag`

### 5.1 形状

```rust
Intent::DialogueChoose { actor: EntityId, node: ContentIndex, option: usize }
```

不带 `dialogue`：`resolve` 要的全部信息（这个节点的第几条选项）都在
`(node, option)` 里，`dialogue` 只在 UI 那侧用来记「这段会话是哪一段」。
带一个 `resolve` 不读的字段，`check_field_consumers.py` 会如实报出来。

**只有带 `outcomes` 的选项提交 `Intent`**（规格 7.2）：纯导航选项在 UI 层
换节点，提交一个恒产出空效果的 `Intent` 只会污染 `Intent` 日志。

### 5.2 `resolve` 侧

新增 `resolve_dialogue_choose`：

1. 查节点的第 `option` 条选项（新 trait `DialogueCatalog`，真实实现是
   `ll_mod::dialogue::DialogueNodeTable`——依赖倒置，与 `SkillCatalog` /
   `QuestCatalog` 同形）；查不到 → 空效果。
2. **重新校验** `all_conditions_hold`（同一个函数）→ 不满足返回空效果。
3. 逐条把 `outcomes` 翻成 `Effect`：`SetFlag` → 一条
   `Effect::SetModState { writes }`。
4. **不产 `Effect::ScheduleNext`**（裁定第 2 条）。

`ResolveCatalogs` 追加两个字段：`dialogues: &dyn DialogueCatalog` 与
`content_ids: &dyn ContentIdLookup`（后者是条件里 `quest-completed` 那一支
反查标识符要的）。**追加在结构体末尾**。

### 5.3 不消耗回合怎么成立

`TurnEngine::perform` 末尾无条件
`self.timeline.schedule(entry.actor, agent.next_action_at)`：不产
`ScheduleNext` ⇒ `next_action_at` 不变 ⇒ 玩家在**同一刻**重新入列，
`world.clock = entry.at` 也不变。测试钉死：说完一整轮话，`world.clock`
与 `agent.next_action_at` 一格没动。

## 六、黄金基准、存档 schema、门禁

- **存档**：`Agent.mod_state` 早已在存档主体里（任务进度就存在那儿），
  本批**不改任何存档主体形状** ⇒ `CURRENT_SCHEMA_VERSION` 保持 4。
  开工前 grep 复核，收尾再复核一次。
- **两条黄金基准**：预期**不变**——本批不新增任何被 `intern` 的内容 id
  （`set-flag` 的 `flag` 走 `parse_id`，不进注册表），既有内容的
  `ContentIndex` 一个都不平移；两条摘要的世界里也没有任何对话被触发。
  **「预期不变」必须被证伪一次**（交接文档记的那类假绿）：临时把一段
  对话插到 `dialogues.json5` 的**最前面**，确认世界摘要当场变红，再撤掉。
  真变了就走四步重冻，四步证据进提交信息。
- `bash scripts/ci/run_all.sh` 必须 exit 0。
- **文件行数**：`player_action.rs` 今天 717 代码行（上限 800），
  `screen/mod.rs` 615。会话屏的排版与状态机因此落在**新文件**
  `crates/ll-game/src/dialogue_screen.rs`，不往那两个里塞。
- **不新增 example target**（ADR 0030）。
- **i18n**：新键中英各一，**加在两份 `.ftl` 末尾**。

## 七、ADR 0018 反例验证（每条都要实测，不是声明）

| # | 改坏什么 | 期望 |
|---|---|---|
| ① | 把会话屏的选项过滤换成恒 `true`（不调 `all_conditions_hold`） | 「条件不满足的选项不显示」当场红 |
| ② | 给 `resolve_dialogue_choose` 加一条 `Effect::ScheduleNext` | 「对话不消耗回合」当场红 |
| ③ | 把 `SetFlag` 那一支的 `ModStateWrite` 去掉 | 「设了旗标之后依赖它的选项出现」当场红（选项**还在**原来那个状态） |
| ④ | 把 `resolve` 侧的重新校验删掉 | 「UI 过时时 `resolve` 拒掉这次选择」当场红 |
| ⑤ | 把 `Talk` 那一行的敌对过滤删掉 | 「敌对目标不列对话行」当场红 |
| ⑥ | 把新内容插到 `dialogues.json5` 最前 | 世界摘要变红（**证明基准对内容敏感**，排除「基线没红是因为世界里没有这类对象」） |

**主动防本会话反复出现的假绿形状**：测试里的 `Catalog` 是空的 ⇒ 查不到会
回落到另一门语言 ⇒ 「两种语言都有文案」用「文案 != 键名」判会恒绿。
本批的 UI 断言一律断言**具体文案内容**或**具体行数/顺序**，不断言
「不等于键名」。

## 八、提交划分（四个，中文提交信息）

1. `feat(ll-sim): 对话后果 outcomes 与 set-flag 的写入` —— `ll-sim/src/dialogue.rs`
   （`DialogueOutcome`/`set_dialogue_flag`）、`ll-mod` 两处、schema、内容哈希
   递增、内容审计、本体内容与两份 `.ftl`
2. `feat(ll-game): 对话进交互列表` —— `InteractTarget::Talk`、`TalkLookup`、
   `occupant_at` 开放、既有测试跟签名
3. `feat(ll-game): 会话屏` —— `dialogue_screen.rs`、`ScreenState::Dialogue`、
   `screen_row_texts`/`screen_data`/`update_screen` 接线
4. `feat(ll-sim): Intent::DialogueChoose 与端到端验收` ——
   `Intent`/`resolve`/`ResolveCatalogs`、端到端测试、计划与设计文档回填

## 九、规格没裁定、本批临时选的做法

逐条列出，都取了「最保守、最容易反转」的那一种。

1. **`InteractTarget::Talk` 与 `ScreenState::Dialogue` 都不带说话人的
   `EntityId`。** 会话屏是模态屏，`Demo::advance` 在它开着时整个早退，
   世界一个字节不动 ⇒ 说话人不可能在会话中途走开或死掉，一个从头到尾
   没有消费者的字段就是又一个「声明了但没接线」。**反转成本一行**：
   批次 4/5 的 `give-item`/`open-trade` 真的需要「给谁」时把它加回来，
   那时它从第一天起就有消费者。
2. **会话屏的标题就是节点的 `text_key`**（NPC 那一句），不给
   `ll_ui::screen::ScreenData` 加第六个槽。规格七节只说了「会话位置是
   UI 状态」，没有规定这块屏的排版。反转成本：加一个 `lead` 槽 + 改
   十余处构造点。
3. **一条选项同时带 `outcomes` 与 `next: "end"` 时，后果照常提交、屏
   照常关。** 规格没说这两件事怎么组合。取「互不排斥」是唯一不丢信息
   的解释（另一种解释是「结束会话的选项不许带后果」，那要在装载期加
   一条校验，且没有任何设计理由）。
4. **换节点时光标复位到第一项。** 所有者裁定第 1 条说的是「预选第一
   项」，没说换节点之后。不复位的话光标会停在新节点里毫不相干的行上。
5. **一条选项都显示不出来时留在屏上、显示占位行**，而不是自动关屏。
   自动关屏会让玩家看不出「他没话跟你说」与「按键没生效」的区别。
6. **schema 对四种未实现的后果报「尚未实现」错误**，而不是解析成一条
   什么都不做的后果。静默无效比当场报错贵得多。
7. **`TalkLookup` 是必填参数而不是 `Option`。** 可选会让「同一格、
   两个调用方、两份行列表」在类型上成为可能。
8. **敌对判定用 `declared_hostile`（对称判据）而不是单向敌意。**
   规格六节第 3 条点名了这个函数，本批只是照做；如实登记它今天只有
   文化那一半在起作用。
9. **尸体不列对话行**（`health <= 0`）。规格没提，但「跟尸体说话」
   显然不是意图。

## 十、落地实测（收尾回填）

### 10.1 两条黄金基准：**实测未变**，并做了覆盖面证伪

```
crates/ll-world/tests/determinism.rs  EXPECTED_WORLD_DIGEST  = 11_270_479_921_196_970_914
crates/ll-sim/tests/replay.rs         EXPECTED_REPLAY_DIGEST = 11_222_878_776_777_704_235
```

两条常量一个字符都没改，两个测试全绿。**「没红」这件事本身被证伪过一次**
（交接文档点名的那个陷阱：基线没红可能只是因为那个测试的世界里根本
不存在这类对象）：往 `ll_world::state::write_mod_state` 里灌一个
`hasher.write_u64(0xDEAD_BEEF)`——

| 基准 | 灌常量之后 | 说明 |
|---|---|---|
| `EXPECTED_WORLD_DIGEST` | **不红** | 那个世界零 `actor`，`write_mod_state` 一次都不执行。该常量自己的文档早就写着这条局限 |
| `EXPECTED_REPLAY_DIGEST` | **红**（`14171074418694948339` ≠ 旧值） | 它真的覆盖 `Agent::mod_state` |

结论因此是有据的：回放基准**有能力**发现 `set-flag` 的写入，本批没让它
变，是因为回放的固定意图流里没有 `Intent::DialogueChoose`。

内容侧不变的理由是结构性的：对话排在 `CONTENT_FILES` **最后**
（`crates/ll-mod/src/content_data.rs`），本批又**零新增 `intern` 的
内容 id**（`set-flag` 的 `flag` 走 `parse_id`），既有 `ContentIndex`
一个都没平移。

### 10.2 `CONTENT_HASH_ALGORITHM_VERSION`：28 → 29

说明段写在 `crates/ll-mod/src/content_hash.rs` 该常量的文档注释里，
标题「# 版本 29（对话后果批次，对话系统的批次 2）」，归**「已有表加
字段」**那一档。

> **并行冲突（协调方已确认）**：`wt-buildings` 那一批也递增到了 29，
> 且已先合入 main。**合并时后到的那一个要重新递增到 30**，由协调方处理。

### 10.3 存档 schema：**不动**

`CURRENT_SCHEMA_VERSION` 仍是分叉时的值。`Agent::mod_state` 早已在存档
主体里（任务进度就存那儿），本批不改任何存档主体形状。
`RawDialogueOption.outcomes` 的 `#[serde(default)]` **只在 JSON5 侧有
意义**，postcard 上是空操作——这一句写进了那个字段的文档，不是一句
兼容性声明。

### 10.4 ADR 0018 反例验证：六条，全部实测

| # | 改坏什么 | 结果 |
|---|---|---|
| ① | `dialogue_rows` 的条件过滤换成恒 `true` | **3 条红**：`条件不满足的选项在会话屏上一行都不显示`、`鼠标点在第几行就选中第几行`、`听完风声之后那一行消失换成另一行` |
| ② | `resolve_dialogue_choose` 补一条 `Effect::ScheduleNext` | **3 条红**：`说完一整轮话世界时钟一格没动` 等 |
| ③ | `SetFlag` 那一支的 `ModStateWrite` 去掉 | `听完风声之后那一行消失换成另一行` 红——那一行**还在** |
| ④ | 删掉 `resolve` 侧的重新校验 | `条件不再满足的选项在结算侧被拒掉` 红 |
| ⑤ | 删掉 `talk_target` 的敌对过滤 | `敌对目标不进对话行` 红 |
| ⑥ | 去掉 `interact_entries` 开头的 `talk_target` | `站着的npc让交互列表多出一行对话` 红 |

六条全部改回后复跑，全绿。

### 10.5 抓到的两处假绿（都在本批自己的测试里）

1. `查不到的节点或越界的选项产出空效果而不是panic`：对照组「另一个
   节点」用新建 `Interner` 的第一个索引取，而那就是
   `ContentIndex::default()`，与被测节点**撞成同一个**，那一半当时恒
   绿。已加 `assert_ne!` 钉死。
2. `敌对目标不进对话行`：一开始给玩家挑的是 `farmstead` 文化，而
   `goblin_warband → farmstead` 的敌意只有 4、不到阈值 5，测试当时是
   **假红**（写完就不过）。换成 `mining_hold`（6）才真的验到，理由写进
   了测试注释。

第三条是主动防治：本批 UI 断言一律断言**具体文案**或**具体行序**，
测试用**真实 `Catalog`**——空 `Catalog` 下 `resolve` 会退回键名，
「文案不等于键名」那种写法会恒绿（批次 19 刚抓到过一条）。

### 10.6 门禁与测试数

- `bash scripts/ci/run_all.sh` **EXIT=0**。
- `bash scripts/ci/run_tests.sh`：改前 **2886 通过 / 120 个二进制 / 0 失败**，
  改后 **2904 通过 / 122 个二进制 / 0 失败**（+18 条，+2 个二进制：
  `ll-sim/tests/dialogue_choose.rs` 与 `ll-game/tests/dialogue_session.rs`）。
- 行数棘轮 `--bless` 三处，理由都写进了快照文件：
  `content_hash.rs` +53、`content_audit.rs` +2、`app_tests.rs` +5。
  `player_action.rs` 加完对话那一行是 822 行（超 800 且不在快照里），
  **拆而不是 bless**：新模块 `interact_list.rs`。
