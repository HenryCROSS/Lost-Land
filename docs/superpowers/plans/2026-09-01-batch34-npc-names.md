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

## 七、落地实测

### 7.1 落点一览

| 落点 | 做了什么 |
|---|---|
| `crates/ll-world/src/naming.rs` | 新增 `PhonemeTables` / `CultureNaming`（`syllables` + `phonemes: BTreeMap<语言, PhonemeTables>`）/ `NamingProblem` / `agent_given_name` / `bare_naming_fixture`；`CultureNaming::rules_for`、`longest_name`、`problem` |
| `crates/ll-world/src/culture.rs` | `CultureAttrs::naming`、`CultureTable::naming()`、`CultureError::Naming`、`define` 里的注册期校验。`CultureError` **不再是 `Copy`**（载荷带语言标签字符串） |
| `crates/ll-mod/src/content_schema_world.rs` | `RawNaming` / `RawPhonemeTables`（**必填**，无 `serde(default)`）；`apply_cultures` 里一处 `intern` 都没加 |
| `crates/ll-mod/src/content_hash.rs` | `write_culture_fields` 末尾多混一段；`CONTENT_HASH_ALGORITHM_VERSION` 34 → **35**；单元测试 `命名规则不同的两条文化摘要不同`（六份声明两两比对） |
| `mods/lostland/cultures.json5` | 七条文化各自一份 `naming`，zh-CN / en 两张按下标对齐的音素表 |
| `crates/ll-game/src/dialogue_screen.rs` | `NPC_NAME_ARG` 常量、`speaker_name`（文化派生 → 职业显示名 → 占位键三档降级） |
| `crates/ll-ui/src/screen/mod.rs` | `ScreenData::title_args`；`screen_text_lines` 改调 `resolve_with_args` |
| `crates/ll-game/src/app/screen_flow.rs` | `ScreenRows::speaker_name`（与行文字、标题键**同一个产出点**）、`title_args()` 帮手 |
| `crates/ll-game/src/menu_screen.rs` | `screen_data` 多收一个 `title_args`，只有 `Dialogue` 那一支读 |
| `assets/locales/{zh-CN,en}.ftl` | 两条开场白的职业名 → `{ $npc_name }`；文件末尾各加一条 `screen-dialogue-unknown-speaker` |
| `crates/ll-ui/tests/i18n_text_width.rs` | 两条 `参数化` 分类规则 + **新测试 `带变量的键必须归参数化`** |
| `crates/ll-game/src/world.rs` | `PLAYER_STARTING_WALLET = 1000`（B） |
| `crates/ll-game/tests/populated_determinism.rs` | `EXPECTED_POPULATED_WORLD_DIGEST` 重冻 + 四步证据（B） |
| 新测试文件 | `crates/ll-mod/tests/culture_naming.rs`（5 条）、`crates/ll-game/tests/dialogue_npc_name.rs`（7 条）、`crates/ll-game/tests/player_wallet.rs`（3 条） |

**`mods/lostland/dialogues.json5` 逐字未动**（`git diff` 里它不出现）——这是本批的交付承诺之一。

### 7.2 「渲染期现算、不进世界状态」的实测证据

1. **签名**：`agent_given_name(&Agent, EntityId, &CultureTable, &str, u64) -> Option<String>`，
   没有任何 `&mut`，不写任何东西。
2. **反例 ①**（把 `given_name` 里的 `entity.as_u64()` 换成常量 `0`，改的是**生产代码**）：
   `一座据点里的名字互不相同` 当场红（28 个 NPC 只剩 2 个名字），
   而**同一次注入下三条黄金基准全绿**。也就是说：本批的派生结果整个变了，
   世界摘要一个 bit 都没动。
3. **对照证伪**（「没红」不等于「咬不住」）：
   - `EXPECTED_POPULATED_WORLD_DIGEST` 在**同一个工作树、同一轮**里被
     `build_player_agent` 那一行改红了（见 7.4 第 ① 步），可见它对
     「`Agent` 的字段值变了」并不免疫；
   - `EXPECTED_WORLD_DIGEST` / `EXPECTED_REPLAY_DIGEST` 在
     `TerrainShape::default()` 的 `sea_level` 400→401（生产代码）下**双双变红**，
     可见它们也没有睡着。
4. **存档**：`Agent` 一个字段都没加，`CURRENT_SCHEMA_VERSION` 仍是 **7**。

### 7.3 那 8.9px：**没撞上**

`dialogue-steward-reward` **本批一个字都没改**——它里面出现的是「他」，不是称呼。
改的是两条 `-root`（`dialogue-steward-root` / `dialogue-guard-root`）。

- 这两条键**改归了 `宽度判据::参数化`**（模式串里 `{ $npc_name }` 的宽度与真实渲染宽无关）。
- 因此**另补了一条对「拼好的整行」的实测断言**：
  `crates/ll-game/tests/dialogue_npc_name.rs` 的
  `会话屏标题在最长的名字下也排得进两行`。它比取样更强——名字取的是**上界**
  （`CultureNaming::longest_name`：音节数取上限、每个位置取最长音素），
  七份文化 × 两种语言 × 两条台词全量排一次，预算与 `dialogue-` 那一类同为 2 行。
  **实测全部通过，没有需要改文案的地方。**
- 这条断言**确实是承重的**：把 `dialogue-steward-root` 的 en 文案加长一句之后，
  `i18n_text_width` 那一侧**仍然全绿**（因为它已归参数化），
  而这一条红在「排成了 4 行，预算 2 行……最宽一行 505.1，可用 508」。

### 7.4 三条黄金基准

| 基准 | 本批 | 说明 |
|---|---|---|
| `EXPECTED_WORLD_DIGEST` | **未动** | 世界里零 `actors`，`build_player_agent` 够不着；姓名不进世界状态 |
| `EXPECTED_REPLAY_DIGEST` | **未动** | 手搓两个 `Agent`，不经 `build_player_agent` |
| `EXPECTED_POPULATED_WORLD_DIGEST` | **重冻**（B 半） | `10943522416722902806` → `5461649738848521059` |

**四步证据**（全部实跑，记录同时写在那条常量的文档注释里）：

1. **基线红**：`left: 5461649738848521059` / `right: 10943522416722902806`。
   **七条存在性断言全部通过**，panic 落在最后那行 `assert_eq!(world.hash(), ..)`——世界没有变空。
2. **把改动关掉，精确回到旧值**：只把 `build_player_agent` 那一行
   `wallet: PLAYER_STARTING_WALLET` 改回 `wallet: 0`（常量本身、NPC 姓名那一整半、
   内容里的 `naming` 声明全部保留），本条**绿**，摘要**精确等于** `10943522416722902806`。
3. **恢复**那一行。
4. **两个独立进程复现**新值 `5461649738848521059`（两次分别启动的 `cargo test` 进程）。

### 7.5 七条存在性断言：**仍然全绿**

一条都没改。第 ① 步的红是落在摘要那一行上的（panic 位置
`populated_determinism.rs:514`，即 `assert_eq!(world.hash(), ..)`），
说明七条存在性断言在改动前后都通过——世界里仍然有据点、NPC、地面物品、
放置的家具、带归属的物品、势力、带归属的 `Agent`。

### 7.6 B：与 `thin.rs` 的 `wallet_rebase` / `wallet_delta`

**结论：不相互作用。** 逐条查证：

- 玩家是**厚层** `Agent`（`world.actors`），`Agent::wallet` 直接存值
  （字段文档：「厚层直接存值，不像薄层那样走公式」）。
- `wallet_rebase` / `wallet_delta` 是 `ThinPopulation` 的两列，只被
  `wallet_of` 读：`公式(seed, id, elapsed) + rebase + delta`。
  `wallet_rebase` 的唯一写入点是 `ThinPopulation::spawn` 的 `wallet_baseline`
  参数与 `rebase`；`wallet_delta` 的唯一写入点是 `batch_update_wallets` 与 `rebase`。
- **玩家从来不进薄层**：`ThinPopulation::spawn` 的全部调用点都在 `#[cfg(test)]`
  里（`thin.rs` 类型文档自己写着「薄层在生产路径上从来没有被写入过一次」，
  `grep` 复核属实），`population` 也不参与 `WorldState::hash`。
- 反方向也没有：`promote`（薄层升格成厚层）把 `wallet_of` 的结果写进
  `Agent.wallet`，那条路径产出的是 NPC 不是玩家，本批一个字未动它。

因此「初始值不再是 0」**不会**让薄层的棘轮性质（重定基准前后 `wallet_of` 不变）
发生任何变化——薄层的钱包基准来自 `spawn` 的入参，与 `PLAYER_STARTING_WALLET`
没有任何调用关系。`thin.rs` 那几条钱包测试改前改后都绿。

### 7.7 B：老存档行为（实测）

`crates/ll-game/tests/player_wallet.rs` 的 `老存档里的钱包按存档里的值读回来`：
造一份玩家 `wallet = 0` 的存档，经 `ll_game::save::save_game` 落盘、
`load_game` 读回，断言**读回来仍然是 0**。**通过。**

也就是说：钱包是存档里的数据，不是读档重算的派生量，本批**不写迁移**，
老存档里的玩家仍然是他存盘时的数额。这与批次 31 第十一节第 7 条对 NPC 钱包
做的判断逐字相同，区别是这一次有实测。

**如实登记这条测试的局限**：它无法造出「真正的旧版本存档」——本批一个字节的
存档形状都没改，`CURRENT_SCHEMA_VERSION` 前后都是 7，所以「本批之前写出的
存档」与「本批之后写出的、钱包恰好是 0 的存档」在字节上无从区分。
它验的是**读档路径不会用新初值覆盖存档里的值**，那正是唯一可能出问题的地方。

### 7.8 内容哈希、存档 schema、跨表撞名

| 量 | 改前 | 改后 |
|---|---|---|
| `CONTENT_HASH_ALGORITHM_VERSION` | 34 | **35** |
| `CURRENT_SCHEMA_VERSION` | 7 | **8** |
| 新增内容 id | — | **零**（音素是字面字符串，不进注册表） |

**存档 schema 那一格是收工时被门禁纠正的**，本文档第二节 2.5 与第四节
原先写的都是「不动」——**那是错的，此处更正**（原文留在上面各节，追溯用）。
`scripts/ci/check_save_schema_version.py` 实跑报出：
`CultureNaming`/`PhonemeTables` 新进入存档主体闭包、`CultureTable` 字段序
10 → 11，因此按它的判据必须升版本并 `--bless`。

**但存档主体的字节布局其实没有变**：`CultureTable` 跟着编年史走，而
`SurfaceStore` 的手写 `SurfaceStoreData` repr 里根本没有 `chronicle` 字段。
这是那道门禁**同一处过度近似的第二次命中**——第一次是建筑类型批次
（`e40cd6a`，存档 4 → 5），两处已在 `CURRENT_SCHEMA_VERSION` 的文档里
互相指向。与那一次不同的是，**本批没有第二个「真事」理由**（那一次派生的
据点布局真的变了）：本批什么派生形状都没变，证据是
`EXPECTED_POPULATED_WORLD_DIGEST` 在 NPC 姓名那一半下逐位不变。
诚实说法是：**升版本是为了过那道阻断门禁，不是因为老存档会被误解析。**

**老存档的实际处境没有因此变差**：内容哈希算法版本 34 → 35 之后，
`check_content_hash_algorithm` 对版本不等一律 `Rejected`，且它排在读档
流程更靠前的位置。那是每一次内容改动都要付的既有代价（ADR 0027），
7 → 8 这一步**没有增加任何额外损失**。
第 7.7 节那条实测验的是**读档路径不会用新初值覆盖存档里的值**，
不是版本兼容性，两件事分开看。

**跨表撞名门禁（第 15 道）绿**：本批不新增任何 `ContentIndex`，
`naming` 里没有一处 `intern`，因此没有任何 id 会被第二张表 `define`。

### 7.9 ADR 0022 反例验证（每条都先跑基线、逐个二进制单独跑）

| # | 改坏什么（全部在生产代码或真实内容里） | 结果 | 红的原因是不是想验的那一条 |
|---|---|---|---|
| 1 | `given_name` 的 `entity.as_u64()` → 常量 `0` | `一座据点里的名字互不相同` **红** | 是：「28 个 NPC 只有 2 个不同的名字——实体号多半没有参与派生」 |
| 1 附 | 同一注入下跑三条黄金基准 | **全绿** | 这正是要的结果，见 7.2 第 2 条 |
| 2 | `agent_given_name` 里给 `seed` 加一个自增原子计数 | `同一个人两次派生出同一个名字` **红** | 是：「在 zh-CN 下两次算出了不同的名字——姓名必须是纯函数」 |
| 3 | `build_player_agent` 的钱包常量 1000 → 0 | `玩家开局就买得起一件中等价位的货` **红** | 是：「玩家手上那件货应当多出一件」`left: 3 / right: 4`——货**没有**换手 |
| 4 | 真实 `cultures.json5` 里矿邑的 en 声母表少一项 | 装载期**当场拒绝**（`culture_naming` 与整条生产装载路径都红） | 是：「文化索引 12 的命名规则在语言 en 与 zh-CN 下的 onsets 表长度不同」 |
| 5 | `dialogue-steward-root` 的 en 文案加一长句 | `会话屏标题在最长的名字下也排得进两行` **红** | 是：「排成了 4 行，预算 2 行……最宽一行 505.1，可用 508」 |
| 5 附 | 同一注入下跑 `i18n_text_width` | **全绿** | 证明参数化那一侧确实量不到，新断言是唯一的牙 |
| 6 | `screen_text_lines` 改回 `catalog.resolve`（丢掉 `title_args`） | `会话屏标题里真的是说话人的名字` **红** | 是：「标题里没有说话人的名字『尼阿恩费阿』：`{$npc_name}`从账册上抬起头……」 |
| 7 | 删掉分类表里 `dialogue-steward-root` 那条规则 | **第一次：不红**（见下） | — |
| 8 | 对照组：`TerrainShape::default()` 的 `sea_level` 400→401 | `EXPECTED_WORLD_DIGEST` 与 `EXPECTED_REPLAY_DIGEST` **双双红** | 是：两条摘要断言 |

#### 一处「改坏了它不红」，以及怎么处理的

**第 7 条第一次跑出来是绿的。** 原因不是测试写错了，是**判据的适用面有洞**：
`i18n_text_width` 的「缺一条就红」只在**一条规则都匹配不到**时生效，而
`dialogue-steward-root` 上面还压着更粗的 `dialogue-`——删掉细规则之后它
静悄悄地退回按**模式串**量行数，门禁照常绿。也就是说：把一条键改归参数化
之后，**没有任何东西阻止下一个人把它改回去**，也没有任何东西阻止下一个人
给别的键加变量而忘了改分类。

**没有粉饰、没有把断言改宽**，补了一条**判据通用、不点名任何一条键**的新测试
`带变量的键必须归参数化`：扫两份真实 `.ftl` 的原文，凡是文案里含 Fluent 变量
（`{ $`）的键，命中的分类规则一律不许是 `行数上限`。它还带一条「扫到的带变量
的键 ≥ 2」的钉子，防「循环一次都没进」那种恒绿。

**补完之后重跑第 7 条：红。** 消息逐条点名
「zh-CN: dialogue-steward-root（命中规则前缀 dialogue-，面板 会话屏）」
「en: dialogue-steward-root（同上）」——正是想验的那一条。
教训写进了 `i18n_text_width.rs` 的模块文档（原文一字未改，追加一段）。

### 7.10 按纪律第 9 条写回的更正（每一处都两边互相指向）

| 更正方 | 被更正方 |
|---|---|
| 本文档 | `docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md` 十一节第 9 条——原文划掉保留，原地追加裁定与落点 |
| 本文档 | `knowledge/design/dialogue-system.md`：3.4 节落地回填（含三处偏离）、八节分批表第 6 行打勾、顶部事实表「`naming.rs` 生产路径零调用」那一行原地更正 |
| 本文档 | `knowledge/design/naming-and-localization.md` 顶部复核横幅：「与出生地文化挂钩」「i18n 音素表对齐」两项从「未落地」变成已落地，其余各项原样 |
| 本文档 | `knowledge/design/society-and-affiliation.md` 五节落地对照表 `naming` 那一行，并在顶部「落地状态」与那张早期字段表各加一处原地更正 |
| 本文档 | `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md` 第七节「批次 6」那一行改写成落地记录 |
| `crates/ll-ui/tests/i18n_text_width.rs` 模块文档 | 它自己那句「不声明就是缺一条，照样红」——原地追加实测出的那个洞与补丁 |

---

## 八、规格没裁定、本批临时选的做法

逐条列出，都取了「最保守、最容易反转」的那一种。

1. **只做给定名，不做姓氏。**〔**本节最要紧的一条**〕
   `naming-and-localization.md` 把姓氏与「同族同姓」写得很清楚，而
   `naming::surname` 已经落地、有测试。本批**没有**接它，因为它要一个
   `FamilyId`，而厚层 `Agent` 没有家族字段、`ThinPopulation.family` 生产路径
   零写入。接它就要给 `Agent` 加一个字段：一次存档 schema 改动 + 一个今天
   没有第二个消费者的字段（`check_field_consumers.py` 是阻断模式）。
   **反转成本**：家族系统落地那天，`surname`/`full_name`/`surname_first`
   三样一起接线，`naming.rs` 一行都不用改（它们本来就在）。
2. **`surname_first` 不进内容 schema。** 它只被 `full_name` 读，而本批不调它。
   `rules_for()` 恒给 `false` 并在文档里写明「`given_name` 一个字节都不读它」。
   反转成本：schema 加一个 `bool`。
3. **音素表的语言键回落到 `BTreeMap` 的第一条**，而不是返回 `None`。
   各语言按下标对齐，回落拿到的是**同一个人的另一套转写**，不是另一个人；
   让它返回 `None` 会把「这个 mod 没写日文音素表」升级成「日文玩家看到的
   全是占位名」。反转成本：改 `rules_for` 一行。
4. **表长一致性做成注册期校验，不是 shell 门禁。** 设计文档建议的是 CI 门禁；
   shell 只看得见本仓库的 `mods/`，第三方 mod 装载时照样能塞一张对不齐的表。
   注册期校验两者都管得住。反转成本：加一个脚本（不冲突，可以叠加）。
5. **`naming` 在 schema 里是必填**，没有 `serde(default)`。与 `RawCulture`
   其余各字段同一条纪律（漏写的症状是「这份文化的每个 NPC 都叫无名氏」）。
   代价：任何已有的第三方 `cultures.json5` 会当场装不进来——今天没有这种 mod。
6. **派生不出来时回落到职业显示名**，而不是占位符。这样批次 1–5 的旧行为
   与本批在代码里是同一条路径的两档，`.ftl` 的措辞两档都读得通。
7. **只有两条 `-root` 台词带 `{ $npc_name }`。** 其余台词里出现的是「他/她」，
   不是称呼，硬塞名字会让文案变差。反转成本：改 `.ftl`，零代码。
8. **`ScreenData` 收的是 `title_args` 而不是「已解析好的标题文本」。**
   后者要给 `menu_screen::screen_data` 塞进 `Catalog` 与 `language`，
   并把十二个写死字面量键的分支各改一遍——为一块屏的一个参数动十二块屏的分工。
9. **本体七条文化的音素表是本批自己编的内容。** 没有任何设计文档规定
   「矿邑的名字该是什么音」。取值原则写在 `cultures.json5` 每条的注释里
   （矿邑塞音开头闭音节收尾、林居流音长元音、部落最短最喉音……），
   并由 `本体每一条文化的取名方式互不相同` 钉住「七条互不相同」这条性质。
10. **`CultureError` 不再是 `Copy`。** 因为 `Naming` 那一支的载荷里带语言
    标签字符串——「哪两种语言的表对不齐」是这条错误消息唯一有用的信息。
    三处既有构造点都是 `return Err(..)` 的一次性移动，不受影响。
11. **`player_wallet.rs` 里刻意不写「夹具前提：开局这笔钱买得起」那句断言。**
    实测过：加了那一句之后，把钱包改回 0 这条反例红在**那一句**上，
    而不是红在「货真的到手了」上——正是「一条断言前面有更容易红的断言」
    那种假绿。判据因此全部落在产出断言里，钱包数值只进错误消息。

---

## 九、对话链剩下什么

**空。** 怎么确认的：

- `knowledge/design/dialogue-system.md` 八节那张分批表共 7 行（批次 0–6），
  本批之后**每一行都带勾**（逐行核对过）。
- 该节「明确不做」的五条（对话编辑器、语音、群体对话、说服判定、
  NPC 主动搭话）**仍然明确不做**，不属于剩余项。
- 该节「两条门禁建议」——`mods/**/*.json5` 的 CJK 字面量扫描、
  `text_key` 的多语言覆盖率检查——**仍未做**，但它原文就写着
  「不属于任何一批的主线」。本批**顺带补上了同一族的第三条**
  （`带变量的键必须归参数化`），另两条留着。
- 九节「需要所有者裁定的问题」里，第 6 条（NPC 初始钱包）由批次 5 关闭，
  第 4 条（`standing` 的初始值、上限与折扣函数）**仍然开着**——它不属于
  对话链的实现，是一次数值裁定。
