# 批次 18：对话内容表（对话系统的批次 1）

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，正文与反例验证无关（`grep -c 反例 knowledge/decisions/0018-*.md` 在 0018 末尾追加订正节之前为 0）。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**工作树** `wt-dlgcontent`，分支 `wt-dlgcontent`，基线 `7d7a5e7`（`main` HEAD，
`Merge branch 'wt-modftl'`）。

**规格来源**：`knowledge/design/dialogue-system.md`，八节分批表的**批次 1**。
本批的直接依据是**二节**（数据放哪长什么样）、**三节**（i18n）、**四节**（选项条件的
能力边界）。批次 0（mod 的 `.ftl` 装载）已于 2026-08-30 落地，计划文档
`docs/superpowers/plans/2026-08-30-batch17-mod-localization.md`，本批全部文案走它铺的路。

**所有者裁定引用**（`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节，
不重新论证）：第 2 条「对话不消耗回合」、第 7 条「第一批对话不支持加入行会/宗教」、
第 8 条「对话里不出现玩家自定义文本」。

**改前基线**（自己跑，不抄）：

- `bash scripts/ci/run_tests.sh` → `EXIT=0`，118 条 `test result: ok`，合计 **2819 passed**，
  0 failed。
- 两条黄金基准与内容哈希版本：**本文档刻意不留副本**，跑
  ```bash
  grep -rn "const EXPECTED_WORLD_DIGEST\|const EXPECTED_REPLAY_DIGEST" \
    crates/ll-world/tests/determinism.rs crates/ll-sim/tests/replay.rs
  grep -n "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs
  ```
  （交接文档第〇节：这里没有值可抄。开工时实测三条都在，值记在提交与最终报告里。）
- **两条黄金基准预期不变**：本批只加内容表，不进 `WorldState`、不参与世界生成、不参与
  任何 NPC 决策。若变了，**先查清楚为什么**（已知教训：新增内容插在注册表中间会让其后条目
  的 `ContentIndex` 整体平移）——因此新内容一律**追加在末尾**。
- **存档形状不动**：本批不给 `Agent` 加任何字段（规格 1.3 明文否决），
  `check_save_schema_version.py` 若拦下来就说明改到了不该改的地方。

---

## 一、范围

### 做

| # | 事项 | 落点 |
|---|---|---|
| 1 | `DialogueDef` / 节点 / 选项的类型与两张注册表 | 新文件 `crates/ll-mod/src/dialogue.rs` |
| 2 | 条件谓词（封闭清单）与它的求值 | 新文件 `crates/ll-sim/src/dialogue.rs` |
| 3 | `mods/<id>/dialogues.json5` 的 schema 与装载 | 新文件 `crates/ll-mod/src/content_schema_dialogue.rs`；`content_data.rs` 加两项 |
| 4 | 跨表引用校验（`root`/`next`/`speaker`/条件里的引用） | 装载期 `required_id` + 注册末尾的整表校验 + `content_audit` |
| 5 | 内容哈希 | `content_hash.rs`：两个新判别值、两个新字段写入函数、版本递增 |
| 6 | 本体内容 | 新文件 `mods/lostland/dialogues.json5` |
| 7 | 示例 mod 内容（走**自己**的 `.ftl` 命名空间） | 新文件 `mods/example_mod/dialogues.json5` + 追加进它自己的 `locales/*.ftl` |
| 8 | i18n 文案 | `assets/locales/{zh-CN,en}.ftl` **追加**一节；`mods/example_mod/locales/{zh-CN,en}.ftl` 追加一节 |

### 不做（批次 2 及之后），但挂载点写在下面第七节

交互列表接线、会话 UI、`Intent::DialogueChoose`、`outcomes` 三条后果
（加入据点 / 任务 / 交易）、`Agent.home` 字段、`standing` 常量、NPC 初始钱包。

**特别说明：本批的 JSON5 schema 里没有 `outcomes` 字段。** 不是遗漏。批次 1 一条后果都
不做，一个只允许空数组的字段就是一个「声明了但没接线」的死字段——本仓库长期记账的正是
这一类。`#[serde(deny_unknown_fields)]` 会让今天写 `outcomes:` 的内容当场报错，这比让它
静默无效诚实。批次 2 加这个字段时，加的是一个从第一天起就有真实消费者的字段。

---

## 二、数据形状

### 2.1 落点与装载顺序

`mods/<id>/dialogues.json5`，`ContentFileKind::Dialogues` **追加在 `CONTENT_FILES` 数组
末尾**（今天 22 项 → 23 项）。判据是那张表自己的注释里那一条：**只 `get` 不 `intern` 的
那一方必须排在被引用者之后**。对话引用职业（`speaker.profession`）、文化
（`speaker.culture`、条件里的 `org`）、任务（`quest-completed`）、物品（`has-item`）、
种族（`is-race`），这五张表全部排在它前面，因此排末尾同时满足全部五条。

排末尾还有第二个理由，与内容无关：**新内容追加在注册表末尾，既有条目的 `ContentIndex`
一个都不平移**。

### 2.2 一个文件两张表

先例是 `crafting.json5`（配方类别 + 配方）。

```json5
{
  dialogues: [
    { id: "lostland:steward_greeting",
      speaker: { profession: "lostland:steward" },
      root: "lostland:steward_root" },
  ],
  nodes: [
    { id: "lostland:steward_root",
      text_key: "lostland:dialogue.steward.root",
      options: [
        { text_key: "lostland:dialogue.steward.ask_join",
          conditions: [ { kind: "not-affiliated", affiliation: "faction" } ],
          next: "lostland:steward_join" },
        { text_key: "lostland:dialogue.common.farewell",
          conditions: [],
          next: "end" },     // 保留字，结束会话
      ] },
  ],
}
```

Rust 侧（`crates/ll-mod/src/dialogue.rs`）：

```rust
pub struct DialogueSpeaker { pub profession: ContentIndex, pub culture: Option<ContentIndex> }
pub enum DialogueNext { End, Node(ContentIndex) }
pub struct DialogueOption { pub text_key: NamespacedId,
                            pub conditions: Vec<DialogueCondition>,
                            pub next: DialogueNext }
pub struct DialogueDef      { pub id: NamespacedId, pub speaker: DialogueSpeaker, pub root: ContentIndex }
pub struct DialogueNodeDef  { pub id: NamespacedId, pub text_key: NamespacedId,
                              pub options: Vec<DialogueOption> }
pub struct DialogueAttrs     { pub speaker: DialogueSpeaker, pub root: ContentIndex }
pub struct DialogueNodeAttrs { pub text_key: NamespacedId, pub options: Vec<DialogueOption> }
pub struct DialogueTable { .. }      // 列式存储，按 ContentIndex 下标
pub struct DialogueNodeTable { .. }  // 同上
```

`*Def` / `*Attrs` 两套是照 `QuestNodeDef`/`QuestAttrs` 的既有分工：`*Def` 是「一条声明
长什么样」（含 `id`），`*Attrs` 是 `define` 实际存进列式存储的子集（不含 `id`，`id` 由
`ContentIndex` 表达）。

schema 侧（`content_schema_dialogue.rs`）照抄既有形状：全部 `Raw*` 带
`#[serde(deny_unknown_fields)]`，跨表引用在 `Raw*` 里一律是 `String`，
`conditions` 的元素是 `{ kind: "…", … }` 带标签的对象（与 `RawQuestCondition`、
`RawSkillEffect` 同一写法，**不用 `serde(untagged)`**）。

### 2.3 跨表引用校验怎么做

分成三层，**每层各管一件今天真的会发生的错**：

| 层 | 位置 | 管什么 |
|---|---|---|
| 装载期 `required_id` | `apply_dialogues` | `speaker.profession` / `speaker.culture` / 条件里的 quest / item / race / org——**只 get 不 intern**，拼错当场报错并点名文件与 id |
| 装载期 `intern_id` | `root` / `next` | 允许前向引用（节点可以定义在引用它的节点之后，同一个文件里回环必然出现前向引用） |
| 全部 mod 装完之后 | `ll_mod::dialogue::validate_references`，调用点在 `ll_game::content::load_content` | 每个 `root` 与每个 `next` 指向的索引**必须真的被节点表 `define` 过**——与 `QuestError::UnregisteredPrerequisite` 同一形状 |

第三层排在全部 mod 装载完毕之后，理由与技能/任务的环校验逐字相同：一个 mod 完全可以把
自己的节点接到本体的对话上，只看单个 mod 的装载结果会误判。

**不校验「每个节点都从某个根可达」**（规格 1.2）：一份 mod 完全可能只提供一批被别人引用的
通用节点，判它孤儿会把一条正确的设计判成错误。

---

## 三、对话图允许有环——怎么保证，终止性靠什么

### 3.1 不复用 `prereq_graph::validate_no_cycles`

`SkillTable`/`QuestTable` 在注册期跑无环校验，因为那两张图表达「前置解锁」，环意味着
谁都学不到。**对话图的环是合法的、且是设计意图**：「加入 → 好的 → 还有别的事吗 → 回到
开场白」在丙档里必然出现。

因此 `DialogueNodeTable` **刻意不实现** `crate::prereq_graph::PrerequisiteGraph`。
`validate_references` 只做「指向的节点已定义」这一件事，**结构上没有 DFS、没有颜色标记、
没有环路径收集**——不是「实现了但没调用」，是根本没有那段代码。

### 3.2 终止性靠结构，不靠静态分析

**每一次跳转都必须由玩家按一次键。** 引擎侧不提供「条件满足就自动推进」这种转移；一个
节点显示出来之后唯一的推进方式是玩家从选项里选一条。这条规则把终止性从一个需要静态分析
的性质降级成一个**结构上不可能违反**的性质。代价是不能写自动播放的过场——今天没有这个
需求，按 YAGNI 不做。

**本批的落点**：`DialogueNext` 只有 `End` 与 `Node` 两个变体，没有 `Auto`/`Immediate`
之类的第三种；节点表不存任何「无需输入的转移」。批次 2 的 UI 接线必须保持这一条。

### 3.3 ADR 0018 反例验证（本批必须实测的第 ①）

把 `validate_references` 换成一份真的无环校验（对节点图跑 `prereq_graph::validate_no_cycles`，
把 `next` 当成边），**本体那段回环内容必须让装载失败**。验证完改回。
——如果加了无环校验装载照样绿，说明我的回环内容没有真的构成环，那条内容就是假的。

---

## 四、条件谓词：七行、封闭、数组即合取

### 4.1 清单（规格四节 4.1 逐条照搬，不增不减）

| # | `kind` | 参数 | 读什么 |
|---|---|---|---|
| 1 | `affiliated` / `not-affiliated` | `affiliation`（五类之一）、可选 `org` | `Agent.affiliations` |
| 2 | `standing-at-least` | `affiliation`、可选 `org`、`value` | `Affiliation.standing` |
| 3 | `quest-completed` / `quest-not-completed` | `quest` | `ll_sim::quest::is_quest_completed` |
| 4 | `flag-set` / `flag-not-set` | `flag` | `Agent.mod_state`（与任务进度同一套存储） |
| 5 | `has-item` | `item`、`count` | `Agent.inventory` |
| 6 | `wallet-at-least` | `value` | `Agent.wallet` |
| 7 | `is-race` | `race` | `Agent.race` |

七行、十条 `kind` 字符串。**没有 `or`、没有可嵌套的 `not`（否定由成对的 `kind` 表达）、
没有嵌套、没有算术、没有变量、没有动态值比较。** 不复用 `content_expr::RawExpr`，判据是
ADR 0021：与伤害公式共享的只有**语法**不是**算法**，而 `RawExpr` 天然支持任意嵌套，
一旦有布尔嵌套就会滑向解释器（规格四节 4.2）。

### 4.2 求值放在 `ll-sim`，不是 `ll-mod`

`DialogueCondition` 这个枚举与它的求值函数一起住在 `crates/ll-sim/src/dialogue.rs`，
`ll_mod::dialogue` 只 `pub use` 它——与 `ll_mod::quest` `pub use ll_sim::quest::
{is_quest_completed, mark_quest_completed, quest_progress_key}` 是同一条既有分工。

三条理由：

1. 条件是**对世界状态的查询**（`Agent` 的归属/背包/钱包/种族/`mod_state`），那些类型住在
   `ll-world`/`ll-sim`，不住在 `ll-mod`。
2. 规格 7.2：**条件判定的代码只写一份，UI 与 `resolve` 共用同一个函数。** 批次 2 的
   `resolve` 侧在 `ll-sim`，UI 在 `ll-ui`/`ll-game`；唯一能被两边共用的位置是 `ll-sim`。
3. `scripts/ci/check_field_consumers.py` 的「决策层」定义就是 `ll-sim/src/**`——放在那里，
   这批谓词不是死字段。

**本批不产出任何 `Effect`**：求值是一个纯函数，输入是 `&Agent` + 一张 `ContentIndex →
NamespacedId` 的反查（`quest-completed` 需要它，因为 `is_quest_completed` 按
`NamespacedId` 查 `mod_state`）。反查用一条新的窄接口 `ContentIdLookup`，`ll-mod` 的
`Registry` 实现它——与 `ll_sim::skill::SkillCatalog` / `ll_sim::quest::QuestCatalog` 的
依赖倒置手法逐条同构。

### 4.3 硬规则：新增谓词必须同批带一条真实内容用例

规格四节 4.3 立的。本批**十条 `kind` 全部有真实内容在用**，逐条对照写在第五节的表里。

### 4.4 `org` 参数今天只能指向内容空间的组织（本批裁定，规格未覆盖）

`OrgRef` 有两支：`Def(ContentIndex)`（文化）与 `Instance(WorldId)`（势力/宗教/行会/家族）。
**`WorldId` 是世界生成期分配的，内容文件里根本写不出来。** 因此：

- `org` 字段是**可选**的，写了就必须是一条**已定义的内容 id**（今天只有文化能满足），
  匹配 `OrgRef::Def`；
- 不写 `org` = 「这一类归属里任意一条」——`standing-at-least` 因此读的是「该类归属中
  `standing` 的最大值」。

这是最保守、最容易反转的做法：将来势力有了内容空间的表示（或者加一条按势力 id 匹配的
参数），加的是一个新字段，既有内容一个字不用改。**如实登记的缺口**：今天写不出
「加入了**某一个具体势力**」这条条件。

---

## 五、内容：本体与示例 mod 各写什么

### 5.1 本体 `mods/lostland/dialogues.json5`

两段会话、**十三个节点**。

- **`lostland:steward_greeting`**（`speaker: { profession: "lostland:steward" }`）——
  据点管理者。所有者提出对话系统的动机原话「玩家可以没有势力归属，这个可以通过后面和
  据点的管理者对话加入」就落在这一段上。开场节点有**五条分支选项**，各带自己的条件。
- **`lostland:mining_guard_greeting`**（`speaker: { profession: "lostland:guard",
  culture: "lostland:mining_hold" }`）——矿堡的卫兵。它存在的理由有两条：证明
  `speaker.culture` 这一档收窄真的能用；以及给规格 1.3 的**裁决顺序**（声明了 `culture`
  的胜过只声明 `profession` 的）一条真实的平局用例。

**回环**（证明「不能用无环校验」不是空话）：每个子节点都有一条「还有别的事吗」回到开场
节点，`lostland:steward_tax` 再回到 `lostland:steward_duties`——两级回环。

**十条 `kind` 的真实用例分布**：

| `kind` | 用在哪 |
|---|---|
| `not-affiliated` | steward 开场「我想加入这里」（没有势力归属才显示） |
| `affiliated` | steward 开场「我该做些什么」（`affiliation: "faction"`）／「同乡」（带 `org: "lostland:farmstead"`，覆盖可选 `org`） |
| `standing-at-least` | steward「税金能不能宽限」（`value: 250`，与所有者裁定的「加入据点给 +250」同一个数） |
| `quest-not-completed` | steward「有活干吗」 |
| `quest-completed` | steward「我把事办完了」 |
| `flag-not-set` | 卫兵「听说过什么传闻吗」 |
| `flag-set` | 卫兵「你刚才说的那件事」 |
| `has-item` | 卫兵「（出示徽记）」（`lostland:tarnished_signet`） |
| `wallet-at-least` | 卫兵「我付得起过路费」 |
| `is-race` | 卫兵「矮人对矮人」（`lostland:dwarf`） |

**注意**：`affiliated`/`standing-at-least` 这几条今天**求值恒为假**（玩家的
`affiliations` 仍写死空列表，批次 3 才写第一条）。它们**能被求值**、只是结果是假——
这与「求值不出来」是两回事，如实记在这里。

### 5.2 示例 mod `mods/example_mod/dialogues.json5`

一段会话（`examplemod:necromancer_greeting`，`speaker: { profession:
"examplemod:necromancer" }`），四个节点，含一条带条件的选项
（`has-item examplemod:healing_potion`）与一处回环。

**文案全部走它自己的命名空间**：键一律 `examplemod:dialogue.…`，写在
`mods/example_mod/locales/{zh-CN,en}.ftl` 里。这是「本体即 Mod」在对话上的检验，也是
批次 0 的验收标的第二次兑现。

**撞键回归夹具**：示例 mod 定义 `examplemod:dialogue.common.farewell`，与本体的
`lostland:dialogue.common.farewell` 折出**同一个 Fluent id**（`dialogue-common-farewell`）
但文案不同。两条必须各自解析到自己那一份。这条与 `mods/example_mod/locales/*.ftl` 末尾
已有的两条撞键夹具（`race-elf-display_name`、裸键 `hud-inventory-empty`）是同一形状，
沿用它们的写法与注释。

### 5.3 ADR 0018 反例验证（本批必须实测的第 ②）

把 `ll_i18n::split_key` 的命名空间分流改回「剥掉前缀」，**示例 mod 的告别台词必须顶掉
本体那一条**（mod 恒在本体之后装载）。验证完改回。

### 5.4 i18n 边界（规格三节 3.1）

**`dialogues.json5` 里不允许出现任何用户可见的字面文本，只允许 `*_key`。**
理由不是「一致性」：`scripts/ci/check_i18n_strings.py` **只扫
`crates/*/src/**/*.rs` 的 CJK 字面量**，`mods/**/*.json5` 完全在它视野之外——允许内联
文案等于让规格 §11.3 在项目文本量最大的系统上彻底失效。

`text_key` 走 `parse_id`（**不 intern**）：本地化键只解析成 `NamespacedId`，不进注册表、
不占内容索引号、不参与 `ContentIndex` 的分配顺序，与
`RaceAttrs`/`ClassAttrs`/`ItemDef` 的 `display_name_key` 逐字同办。

**本批新增一条测试（不是门禁）**：`crates/ll-game/tests/dialogue_content.rs` 走生产装载
路径，断言仓库里每一条对话 `text_key` 在 **zh-CN 与 en 两种语言下都有精确译文**
（`Catalog::try_resolve`，不走语言回退链，理由同批次 0 的 2.4）。规格三节 3.5 建议的
那条**门禁**（覆盖全部内容类型的 `text_key`）仍不属于本批。

---

## 六、内容哈希、门禁与代价（规格二节 2.3 逐条）

- `ContentTableKind` 新增两个变体，**判别值往后接**：`Dialogue = 23`、`DialogueNode = 24`。
  **不挪既有值。**
- `ContentValueTables` 新增 `dialogue` / `dialogue_node` 两个字段；`classify_index` 的穷尽
  解构与 `entry_value_digest` 的 `match` 各补两条（两处都不带通配分支，忘了补会编译失败）。
- 新增 `write_dialogue_fields` / `write_dialogue_node_fields`。
- `content_audit.rs`：`ALL_CONTENT_TABLE_KINDS` 23 → 25、`roster_slot`、`table_display_name`、
  `inspect_dialogue` / `inspect_dialogue_node`，两张表都进 `covered`。
- **`CONTENT_HASH_ALGORITHM_VERSION` 递增**（27 → 28），并按既有段落格式补一段说明：
  这是「**新增内容表**」那一类（版本 4/5/16/22/27 同类），不是「既有表多字段」——
  这批 id 此前落在 `ContentTableKind::Opaque` 一侧（只混 id、不混字段值），现在混的是
  完整字段流。
- `scripts/ci/check_field_consumers.py`：`CONTENT_HASH_KIND_TO_TARGET_TYPE` 与
  `TARGET_TYPES` 各补两条（`Dialogue → DialogueDef`、`DialogueNode → DialogueNodeDef`），
  否则 `check_content_hash_gate_cross_coverage` 那条互校会当场把门禁变红。新表的字段今天
  没有决策层 `.field` 读取点（对话的消费者是 UI 与批次 2 的 `resolve`），逐条补
  `EXEMPTIONS` 并写明预期接线批次。
- **`check_file_size_budget.py` 的棘轮**：`content_hash.rs` 与 `content_audit.rs` 已在快照里
  且必然会涨——这是「新增一类内容」不可避免的代价（那两个文件的结构就是「每张表一段」）。
  改动与 `--bless` 一起提交，并在提交信息里说清楚。**新文件三个全部远低于 800 代码行上限。**
- **不新增 example target**（`check_no_examples.sh`，ADR 0030）。
- **不动世界摘要与回放摘要**：对话内容不参与世界生成，也不参与任何 NPC 的既有决策。

---

## 七、批次 2 及之后的挂载点（本节是给下一批的交接）

> **【2026-08-31 回填：批次 2 已落地】** 计划文档
> `docs/superpowers/plans/2026-08-31-batch21-dialogue-ui.md`，工作树
> `wt-dialogue2`。下表**批次 2 那五行全部照本节写的挂载点做完了**，
> 一处偏离：`Intent::DialogueChoose` 的形状是
> `{ actor, node, option }`——**不带 `dialogue`**，因为结算要的全部
> 信息都在 `(node, option)` 里，带一个 `resolve` 从不读的字段会被
> `check_field_consumers.py` 如实报成未接线。
> `CONTENT_HASH_ALGORITHM_VERSION` 28 → 29（本节预告的「会再次递增
> 内容哈希版本」兑现了）。批次 3–6 那几行原样有效。

| 批 | 要加什么 | 挂在哪（本批已经留好的位置） |
|---|---|---|
| **2** | `outcomes` 字段 | `RawDialogueOption` 加一个 `#[serde(default)] outcomes: Vec<RawDialogueOutcome>`；`DialogueOption` 加同名字段；`write_dialogue_node_fields` 与 `inspect_dialogue_node` 各补一段（**会再次递增内容哈希版本**） |
| **2** | 进交互列表 | `ll_game::player_action::interact_entries`（`player_action.rs`，今天只扫 `world.ground_items` + 地形门）新增一支 `InteractTarget::Talk`；匹配用本批的 `ll_mod::dialogue::DialogueTable::match_speaker(profession, culture)`，**裁决顺序已在那里实现并有测试**（culture 优先、平局取最小 `ContentIndex`） |
| **2** | 会话 UI | 显示用 `DialogueNodeTable::get(node)`；选项过滤用本批的 `ll_sim::dialogue::all_conditions_hold`——**UI 与 `resolve` 必须调同一个函数**（规格 7.2），不各写一份 |
| **2** | `Intent::DialogueChoose` | `resolve` 侧**重新校验条件**，调的还是 `all_conditions_hold`；**不产 `Effect::ScheduleNext`**（所有者裁定第 2 条：对话不消耗回合） |
| **2** | `set-flag` 后果 | 键的构造已经在 `ll_sim::dialogue::dialogue_flag_key`，写入走 `Effect::SetModState`；条件侧的 `flag-set`/`flag-not-set` 本批已经读它 |
| **3** ✅ | 加入据点（**2026-08-31 已落地**，计划文档 `docs/superpowers/plans/2026-08-31-batch26-dialogue-join.md`） | 本行预告的每一项都照做了：`join-settlement` 后果、`Agent.home: Option<WorldId>`（`CURRENT_SCHEMA_VERSION` 5 → 6）、`standing` 常量（`JOIN_SETTLEMENT_STANDING` = 250 在 `ll-sim`，`Affiliation::STANDING_FULL` = 1000 在 `ll-world`）。`affiliated`/`standing-at-least` 两条谓词确实第一次有了非假读数，端到端证据在 `crates/ll-game/tests/dialogue_session.rs` 的 `加入据点之前那条选项不出现加入之后出现` 与 `standing不够时那一行仍然不出现`。**一处本行没预告到的代价**：批次 2 的「不带说话人 `EntityId`」那条裁定必须反转（`join-settlement` 要问说话人的 `home`） |
| **4** ✅ | 任务（**2026-08-31 已落地**，计划文档 `docs/superpowers/plans/2026-08-31-batch29-dialogue-quest.md`） | `complete-quest` 照本行预告**调的就是既有 `mark_quest_completed`**；`give-item` 的 **owner 校验硬前置照原话落地了**，但**本行「`Effect::TransferOwnership` 的第一个调用方」这半句被反转**：那个效果只改 `owner` 不搬运，两种排法一种会波及给方剩下的几件、另一种定位不到刚收下的那一堆，归属因此改由 `resolve` 算好写进搬运效果（`resolve_pick_up` 的既有手法），`TransferOwnership` **至今仍无调用方**。完整论证见批次 29 计划文档三节 3.5 与 `Effect::TransferOwnership` 的文档末节。另一处收窄：`give-item` 不带 `count`（一次一件） |
| **5** | 交易 | `open-trade` 后果（不产 `Effect`，只推 UI）、`Intent::Trade`、NPC 初始钱包（所有者裁定第 4 条）、占位价格公式 |
| **6** | NPC 姓名 | `CultureAttrs.naming` + 渲染期现算；对话文案从职业名换成 `{ $npc_name }`——**只改 `.ftl`，一个 JSON5 字都不改**（这正是本批 5.4 那条边界白送的好处） |
| 门禁 | `mods/**/*.json5` 的 CJK 字面量扫描、`text_key` 多语言覆盖率门禁 | 规格三节 3.1/3.5 建议的两条，本批只交付了后者的**测试版**（`crates/ll-game/tests/dialogue_content.rs`），门禁版仍未做 |

---

## 八、提交划分

1. `feat(ll-sim): 对话条件谓词与求值` —— `ll-sim/src/dialogue.rs` + 单元测试
2. `feat(ll-mod): 对话内容表与 dialogues.json5 装载` —— `dialogue.rs`、
   `content_schema_dialogue.rs`、`content_data.rs`、`pipeline.rs`、`load_session.rs`
3. `feat(ll-mod): 对话两张表进内容哈希与装载后校验` —— `content_hash.rs`、
   `content_audit.rs`、`check_field_consumers.py`、内容哈希版本递增、行数快照 `--bless`
4. `feat(content): 本体与示例 mod 的第一批对话内容与文案`
5. `test(ll-game): 对话内容的端到端验收` + 计划文档与设计文档回填

## 九、纪律核对（交接文档第一节九条）

- 第 1 条：三个关键常量全部 grep 自取，本文档不留副本 ✅
- 第 2 条：黄金基准四步重冻——**本批预期不重冻**，实测确认不变即可 ✅
- 第 3 条：独立工作树 `wt-dlgcontent` ✅，**不碰** `LostLand` 与 `wt-uitext`
  （后者改 `ll-ui/src/hud/`、`ll-ui/src/widget/`、`ll-game/src/hud_draw.rs` 与两份
  `.ftl` 的**既有条目**——本批只在两份 `.ftl` **末尾追加**新的一节，零重叠）
- 第 4 条：`run_all.sh` 必须 exit 0，报告自己跑的改前/改后测试数 ✅
- 第 6 条 / ADR 0018：每条新断言用故意改坏实现的方式验证真的会红，本批特别要验的两条见
  3.3 与 5.3 ✅
- 第 9 条：本批会更正 `knowledge/design/dialogue-system.md`（八节批次 1 那一行落地标注）
  与 `crates/ll-mod/src/quest.rs` 的 `QuestCondition::Script` 文档（它举的例子「拜访某个
  NPC 并说出特定台词」正是对话）——**两边互相指向**

---

## 十、落地实测（本节在收尾时回填，不是计划）

### 10.1 两条黄金基准：**实测未变**，符合预期

```
crates/ll-world/tests/determinism.rs:351  EXPECTED_WORLD_DIGEST  = 11_270_479_921_196_970_914
crates/ll-sim/tests/replay.rs:984         EXPECTED_REPLAY_DIGEST = 11_222_878_776_777_704_235
```

两条常量一个字符都没改，`cargo test -p ll-world --test determinism` 与
`cargo test -p ll-sim --test replay` 全绿。这正是「新内容追加在 `CONTENT_FILES` 末尾」
那条规避买到的东西——本体的对话排在 `lostland` 那一遍的最后，既有条目的 `ContentIndex`
一个都没有平移。

### 10.2 `CONTENT_HASH_ALGORITHM_VERSION`：27 → 28

说明段落写在 `crates/ll-mod/src/content_hash.rs` 该常量的文档注释里，标题
「# 版本 28（对话内容表批次）」，归类为**新增内容表**那一档（同版本 4/5/16/22/27）。

### 10.3 ADR 0018 反例验证：四条，全部实测

| # | 改坏什么 | 结果 |
|---|---|---|
| ① | 给 `validate_references` 加一份真的无环校验（把 `next` 当边跑 DFS） | **本体内容当场装不进来**：`DialogueGraph { UnregisteredNext { node: ContentIndex(143) } }`，反查出来是 `lostland:steward_root`。`crates/ll-game/tests/dialogue_content.rs` 里 10 条断言红了 8 条。改回后全绿 |
| ② | 把 `ll_i18n::split_key` 的命名空间分流改回「剥掉前缀」 | 三条红：`对话里故意撞键的两条互不覆盖`、`示例模组的对话文案来自它自己的ftl`、`每一条对话文案键在中英文下都有精确译文`。**实测到的坍缩方向是本体赢**（示例模组那句永远查不到），与批次 0 的 `race-elf-display_name` 那次实测方向相反——方向取决于 `FluentBundle::add_resource` 撞上重复 id 时是整份跳过还是只跳过冲突条目，不重要；重要的是两条键坍缩成了一条。改回后全绿 |
| ③ | 删掉本体矿堡卫兵那一段的 `culture:` 收窄 | 两条红：`矿堡卫兵匹配到按文化收窄的那一段`，以及 `content_audit` 的字段覆盖（`会话入口表::DialogueAttrs::speaker::culture：没有任何一条内容把它设成非默认值`）。改回后全绿 |
| ④ | 删掉本体那一条 `is-race` 谓词的用例 | `十条谓词在真实内容里全部有用例` 红（9 ≠ 10）。这条把设计文档四节 4.3 那条硬规则从劝告变成了机器检查。改回后全绿 |

### 10.4 规格没裁定、本批临时选的做法

逐条列在最终报告里，并已回填进 `knowledge/design/dialogue-system.md` 八节前的复核横幅
（纪律第 9 条：更正必须写回被更正方，两边互相指向）。
