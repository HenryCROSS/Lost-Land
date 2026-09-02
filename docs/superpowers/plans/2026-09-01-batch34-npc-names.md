# 批次 34：NPC 姓名（对话批次 6）+ 玩家初始钱包

**工作树** `wt-npcnames`（分支 `wt-npcnames`，基于 `origin/main` = `5f0761b`）。
**日期** 2026-09-02。

两件事合成一批，理由只有一条：**它们都会动「有人有城」那条黄金基准，合批只重冻一次**。
仓库先例是五族与性别字段合批。除此之外两者互不相干，因此下面的论证也分开写，
提交也分开。

**这是对话链的最后一批**（`knowledge/design/dialogue-system.md` 八节分批表第 6 行）。

---

## 〇、开工基线（自己跑的，不抄任何文档）

```bash
grep -rn "const EXPECTED_WORLD_DIGEST" crates/ll-world/tests/determinism.rs
grep -rn "const EXPECTED_REPLAY_DIGEST" crates/ll-sim/tests/replay.rs
grep -rn "const EXPECTED_POPULATED_WORLD_DIGEST" crates/ll-game/tests/populated_determinism.rs
grep -n "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs
grep -n "pub const CURRENT_SCHEMA_VERSION" crates/ll-content/src/save_file.rs
```

| 量 | 开工值 |
|---|---|
| `EXPECTED_WORLD_DIGEST` | `9_641_812_801_195_310_050` |
| `EXPECTED_REPLAY_DIGEST` | `8_626_273_720_694_163_787` |
| `EXPECTED_POPULATED_WORLD_DIGEST` | `10_943_522_416_722_902_806` |
| `CONTENT_HASH_ALGORITHM_VERSION` | `34` |
| `CURRENT_SCHEMA_VERSION` | `7` |

改前测试数：见第七节（`CARGO_BUILD_JOBS=2 bash scripts/ci/run_tests.sh`，本工作树自己跑的）。

---

## 一、范围

**A（本批主体）NPC 姓名**：`CultureAttrs.naming` + 渲染期现算 + 对话文案换 `{ $npc_name }`。
**B 玩家初始钱包 = 1000**（所有者裁定）。

**不做**：改名 `Effect`、覆盖名、姓氏与家族、mod 命名钩子、NPC 名字出现在
HUD/交互列表/编年史里。理由逐条见第二节与第八节。

---

## 二、A：`CultureAttrs.naming` 的形状

### 2.1 复用既有的 `NamingRules`，不新造一套

`crates/ll-world/src/naming.rs` **已经落地**：`NamingRules { onsets, nuclei, codas,
syllables, surname_first }` 与 `given_name`/`surname`/`full_name` 三个纯函数，
八条单元测试，走 `DetRng::for_entity`。它今天**生产路径零调用**。本批要做的就是
给它接上唯一缺的那一环：「这个 NPC 该用哪一份 `NamingRules`」。

`naming-and-localization.md` 二节已经冻结了答案：**跟出生地的文化走，不跟种族。**
今天 `Agent` 身上「出生地文化」的唯一表示就是 `AffiliationKind::Culture` 那条归属
（`ll_mod::roster::build_npc_agent` 是它唯一的生产者）。因此不需要任何新字段。

### 2.2 多语言音素表：按下标对齐，注册期校验

`naming-and-localization.md` 三节把这条定死了，而且给了理由（**切语言不该改名**，
否则种子分享名存实亡）：各语言版本表长必须相同、同一下标必须是同一个音。

所以 `naming` 的形状是「一份音节数区间 + 每种语言一份音素表」：

```json5
naming: {
  syllables: [2, 3],
  phonemes: {
    "zh-CN": { onsets: [...], nuclei: [...], codas: [...] },
    "en":    { onsets: [...], nuclei: [...], codas: [...] },
  },
}
```

Rust 侧：

```rust
pub struct CultureNaming {
    pub syllables: (u8, u8),
    /// 语言标签 -> 那种语言的音素表。`BTreeMap` 而不是 `HashMap`：遍历
    /// 顺序进内容哈希、也决定语言查不到时的回落对象（约束 C5）。
    pub phonemes: BTreeMap<String, PhonemeTables>,
}
pub struct PhonemeTables { pub onsets: Vec<String>, pub nuclei: Vec<String>, pub codas: Vec<String> }
```

**校验做在注册期（ADR 0017），不做成 shell 门禁**——设计文档建议的是「CI 门禁」，
但 shell 门禁只看得见本仓库的 `mods/`，第三方 mod 装载时照样能塞一张对不齐的表。
注册期校验两者都管得住。三条：

1. 至少一种语言；
2. 每种语言的 `onsets`/`nuclei` 非空（`codas` 允许为空，`naming.rs` 本来就允许某文化
   没有韵尾）；
3. 各语言之间 `onsets`/`nuclei`/`codas` 三张表**长度两两相等**。

### 2.3 不做姓氏（本节最要紧的一条）

`naming::surname` 要一个 `FamilyId`，而 `FamilyId` 在**厚层零来源**：
`Agent` 没有家族字段，`ThinPopulation.family` 那一列的全部写入点都在
`#[cfg(test)]` 里。给 `Agent` 加 `family` 是一次存档 schema 改动 + 一个今天没有
第二个消费者的字段（`check_field_consumers.py` 是阻断模式）。

**因此本批只派生给定名**，`surname_first` 也**不进内容 schema**——它只被
`full_name` 读，而本批不调 `full_name`。`rules_for()` 造 `NamingRules` 时给它
`false`，并在文档里写明「`given_name` 一个字节都不读它，取任何值结果相同」。
家族系统落地那天，`surname`/`full_name`/`surname_first` 三样一起接线，
`naming.rs` 一行都不用改。

### 2.4 派生入口

```rust
// crates/ll-world/src/naming.rs
pub fn agent_given_name(
    agent: &Agent, entity: EntityId, cultures: &CultureTable, language: &str, seed: u64,
) -> Option<String>
```

- 从 `agent.affiliations` 线性扫出 `AffiliationKind::Culture` 那一条（C5：`Vec`，不碰哈希容器）；
- `CultureTable::naming(kind)` -> `rules_for(language)`（该语言没声明时回落到
  `BTreeMap` 的**第一条**——不是「另换一个名字」，是同一个名字换一套字形，
  与设计文档三节的语义一致）；
- `naming::given_name(&rules, seed, entity)`。

**没有文化归属 / 该文化没声明 `naming` => `None`**，调用方回落到**职业显示名**
（`ClassAttrs.display_name_key`，也就是批次 1-5 的旧行为）。ADR 0015：诚实表达
「这个人没有名字」，不伪造一个。

### 2.5 「渲染期现算、不进世界状态」怎么证

1. `agent_given_name` 的输入只有 `(agent, entity, cultures, language, seed)`，
   **不写任何东西**——签名里没有 `&mut`；
2. 三条黄金基准**都不该动**（本批 A 部分）。这是反例 3 的判据：
   往派生里灌常量，三条基准全绿 => 它确实没进世界状态；
3. **没红的必须给证伪**：对照组是**生产代码里已知会红的注入点**——
   B 部分的 `build_player_agent` 钱包那一行自己就是这个对照组；
4. 存档：`Agent` 一个字段都没加，`CURRENT_SCHEMA_VERSION` 不动。

### 2.6 内容哈希

`CultureAttrs` 加字段 => `write_culture_fields` 末尾多混一段 =>
`CONTENT_HASH_ALGORITHM_VERSION` **34 -> 35**。
混入顺序：`syllables.0`/`syllables.1`，语言条数，然后按 `BTreeMap` 顺序逐条混
「语言标签 + 三张表各自的长度 + 逐项字节」。音素是**字面字符串**、不是
`ContentIndex`，因此不需要 `registry.resolve`。

**不新增任何内容 id** => `ContentIndex` 不平移，跨表撞名门禁不受影响。

---

## 三、A：i18n 与那 8.9px

### 3.1 只改 `.ftl`，不改任何 JSON5

批次 18 第 5.4 节那条边界白送的：把 `dialogue-steward-root` / `dialogue-guard-root`
里的职业名换成 `{ $npc_name }`，`mods/**/dialogues.json5` 一个字不动。
**只有这两条**：其余台词里出现的是「他/她」，不是称呼。

### 3.2 `{ $npc_name }` 是 Fluent 运行期变量 => 会话屏标题要能带参数

今天会话屏的标题走的是「键」这条路：`ScreenData.title_key` 由 `ll-ui` 用
`catalog.resolve` 解析。带参数就解析不出来。

**做法**：给 `ScreenData` 加一个 `title_args`，`screen_text_lines` 改调
`resolve_with_args`。理由是**照抄同一个结构体里 `notice` 那一条既有分工**：
「参数注入是持有 `Catalog` 与领域数据的调用方的事」。`ll-ui` 已经在两处用
`FluentArgs`（`hud/mod.rs`、`hud/character_panel.rs`），不引入新依赖。

**否决的替代**：把 `title_key` 整个改成「已解析好的标题文本」。那要给
`menu_screen::screen_data` 塞进 `Catalog` 与 `language` 两个参数，并把十二个
写死字面量键的分支各改一遍——为一块屏的一个参数，动十二块屏的分工。

### 3.3 溢出门禁：这两条键改归 `宽度判据::参数化`，另补一条量「拼好的整行」

`crates/ll-ui/tests/i18n_text_width.rs` 的分类表是**最长前缀优先**，
因此加两条更细的规则 `dialogue-steward-root` / `dialogue-guard-root`，
判据 `参数化`。其余 `dialogue-` 照旧走 `散文`。那道门禁两个方向都会红
（缺一条红、死规则也红），两条新规则各自都能匹配到一个键。

**参数化必须另有一条对「拼好的整行」的实测断言**（批次 23 `hud-key-hint-` 的
做法，那条断言在 `ll_game::key_hint` 的 `两行提示在两种语言下都排得进一行`）。
本批那条落在 `ll-game`，且**比取样更强**：它不随便挑一个名字，而是从真实
`mods/lostland/cultures.json5` 的音素表里算出**每份文化在每种语言下最长的
可能名字**（音节数取上界、每个位置取最长音素），把它代进这两条台词，
按模态屏内容宽真实排版，断言不超过 2 行（`dialogue-` 那一类的散文预算）。

**撞上 8.9px 就改文案，不放宽预算。**

---

## 四、B：玩家初始钱包 = 1000

- 落点 `crates/ll-game/src/world.rs` 的 `build_player_agent`，`wallet: 0` -> 一个具名常量。
- **常量而不是字面量**：与 `Agent::STARTING_HEALTH`/`STARTING_MANA` 同一形状。
- 数值是**所有者裁定**（1000），不是推导；常量文档写明这一点，并给量纲对照
  （交易占位价格公式在 `ll_sim::trade`），免得后人以为它是算出来的。
- 批次 31 第十一节第 9 条那条临时裁定（「玩家初始钱包仍然是 0，等所有者裁定」）
  按纪律第 9 条**写回被更正方**，两边互相指向。

### 4.1 与 `thin.rs` 的 `wallet_rebase` / `wallet_delta`

**结论（要实测确认，不是转述）**：不相互作用。

- 玩家是**厚层** `Agent`（`world.actors`），`Agent::wallet` 直接存值；
- `wallet_rebase`/`wallet_delta` 是 `ThinPopulation` 的两列，公式是
  `wallet_of = 公式(seed, id, elapsed) + rebase + delta`；
- 玩家从来不进薄层：`ThinPopulation::spawn` 的全部调用点都在 `#[cfg(test)]`；
- `promote`（薄层升格成厚层）把 `wallet_of` 的结果写进 `Agent.wallet`——
  那条路径产出的是 NPC，不是玩家，且本批一个字都不动它。

**要跑的验证**：grep `ThinPopulation::spawn` / `.promote(` 的生产调用点；
确认改动之后薄层的钱包测试逐条仍绿（它们钉的是「重定基准前后 `wallet_of`
不变」这条棘轮性质）。

### 4.2 老存档

钱包是**存档里的数据**，不是读档重算的派生量（`Agent::wallet` 字段文档：
「厚层直接存值，不像薄层那样走公式」）。因此老存档里的玩家仍然是他存盘时的数额。
**不写迁移**（交接文档第〇之二第 9 条；批次 31 第十一节第 7 条对 NPC 钱包做过
同一判断）。**这条要实测**：造一份 `wallet` 为 0 的存档，读回来断言仍是 0。

### 4.3 三条黄金基准

`populated_world()` 里有 `spawn_player`，`Agent::wallet` 进 `WorldState::hash`
=> **`EXPECTED_POPULATED_WORLD_DIGEST` 必然动**，走四步。
另外两条基准的世界都不经 `build_player_agent`（前者零 `actors`，后者手搓
两个 `Agent`）=> 不该动；不动的那两条由对照组一并证伪。

---

## 五、ADR 0022 反例验证计划（每条先跑基线，再改坏，逐个二进制单独跑）

点名四条 + 其余：

| # | 改坏什么（都在生产代码或真实内容里，不在 `#[cfg(test)]`） | 该红的是谁 |
|---|---|---|
| 1 | `given_name` 里把 `entity.as_u64()` 换成常量 `0` | 「不同 NPC 名字不同」 |
| 2 | `agent_given_name` 里把 `seed` 换成实体号（打破跨调用确定性） | 「同一个 NPC 两次同名」 |
| 3 | `agent_given_name` 里把整份 `NamingRules` 换成常量表 | **三条基准都不该红**（要的是「不红」，见 2.5） |
| 4 | `build_player_agent` 的钱包常量改回 0 | 「玩家买得起」端到端 |
| 5 | 某份文化的 `en` 音素表少一项（真实 `cultures.json5`） | 表长一致性校验 |
| 6 | 把 `dialogue-steward-root` 的 en 文案加长到溢出 | 「拼好的整行」宽度断言 |
| 7 | `screen_text_lines` 改回 `resolve`（丢掉 `title_args`） | 「标题里真的插进了名字」 |
| 8 | 分类表里删掉 `dialogue-steward-root` 那条规则 | 溢出门禁「缺一条」方向 |

每条都要确认**红的原因确实是想验的那一条**（读 panic 消息，不看颜色）。

---

## 六、提交划分（中文提交信息；尽量让每个提交自身是绿的）

1. `feat(world): CultureAttrs.naming——按文化派生 NPC 姓名（渲染期现算）`
2. `feat(ui): 会话屏标题插入 { $npc_name }`
3. `feat(game): 玩家初始钱包 1000（所有者裁定）` — 含 `EXPECTED_POPULATED_WORLD_DIGEST` 重冻
4. `docs(plan): 批次 34 收工回填`

---

## 七、（落地实测回填位）

---

## 八、（规格没裁定、本批临时选的做法，回填位）
