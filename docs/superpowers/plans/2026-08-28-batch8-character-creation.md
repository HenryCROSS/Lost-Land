# 批次 8：角色创建（种族/性别/职业）+ 世界配置 + 在地图上选出生地

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，正文与反例验证无关（`grep -c 反例 knowledge/decisions/0018-*.md` 在 0018 末尾追加订正节之前为 0）。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

本文是开工前写的计划，落地后的偏差逐条记在第十一节，**不回头改前面几节
假装计划一开始就是这样**（沿用批次 5/6 的做法）。

基线是 `main` 的 `15346df`，工作树 `wt-chargen`（分支同名）。

---

## 〇、开工前自己 grep 复核过的数字（不信任何口头转述）

纪律第 1 条。交接文档自己写了「读到这份文档时它们可能又变了，以代码为准」，
本节是那句话的执行结果。

| 常量 / 事实 | 复核值 | 位置 |
|---|---|---|
| `EXPECTED_WORLD_DIGEST` | `10_180_278_885_427_934_050` | `crates/ll-world/tests/determinism.rs:286` |
| `EXPECTED_REPLAY_DIGEST` | `6_885_882_507_408_978_859` | `crates/ll-sim/tests/replay.rs:910` |
| `CONTENT_HASH_ALGORITHM_VERSION` | `27` | `crates/ll-mod/src/content_hash.rs:805` |
| 本体职业条数 | **13** | `mods/lostland/classes.json5`（`id:` 行计数） |
| 本体种族条数 | **4** | `mods/lostland/races.json5`（human/dwarf/elf/goblin） |
| 玩家职业硬编码处 | `crates/ll-game/src/world.rs:601` | `profession: ContentIndex::default()` |
| `ZOOM_LADDER` | **`[4, 2, 1]`**（三档，不是交接文档写的四档） | `crates/ll-world/src/world_map.rs:88` |
| 改动前测试基线 | **112 个二进制、2598 passed / 0 failed / 0 ignored**，`run_tests.sh` exit 0 | 本会话自己跑的 |

**`ZOOM_LADDER` 那一行值得单列**：交接文档写的是 `[8,4,2,1]`，而 `main` 最后
一个提交（`15346df`「世界地图屏上格数翻两番」）把它改成了三档，并把「一格恰好
一个区块」那一档从第 1 档挪到了**第 0 档**（打开地图默认就是选点需要的粒度）。
本批选点屏因此**默认停在最远档**就已经是「一格 = 一个区块」。

---

## 一、所有者裁定与它对结构的决定性影响

> 「开始游戏的时候需要玩家设置种族，性别，职业。然后设置历史生成的配置。
> 接着就是选择地图上在哪重生。」
>
> 「重生点就是随机点一个格子，然后在那区块内随机出生在陆地上。」
>
> 「以后可能会加入不同性别的贴图，不过目前先留着个位置默认用其中一个好了。」
>
> **（肉鸽模式死亡）**「死亡后变成一般模式，可以再创建角色然后选择在某个
> 地方出生。」

**最后一条决定了本批的结构**：角色创建不是「开局流程」的一段，它是**「一个
角色进入一个已经存在的世界」这件事的入口**，而死亡重生会第二次走它。
**世界比角色活得长。**

落到形状上就是一条硬约束：**「造角色」与「造世界」必须是两个可以独立调用的
步骤，中间由一个可复用的数据结构衔接**，而不是一条从头贯到尾的
`start_new_game()`。接缝的具体位置见第七节。

---

## 二、任务 A：玩家拿不到任何职业

### 2.1 复核结论（缺陷成立，注释双重过期）

`crates/ll-game/src/world.rs:598-601`：

```rust
// 本体目前没有注册任何职业内容（职业只经 mod 脚本
// `register-class` 注册，见 `ll_mod::class` 模块文档）——占位索引
// 是诚实的「尚无职业」表达，不是缺陷。
profession: ll_core::ident::ContentIndex::default(),
```

两句都过期：

1. **脚本系统已整体拆除**（ADR 0028），`register-class` 这条通道今天不存在；
2. `mods/lostland/classes.json5` **有 13 个职业**，`resolve_base_classes` 已经
   把其中三个解析进 `content.class_ids`（`warrior`/`mage`/`ranger`）。

`ContentIndex::default()` 是 `lostland:placeholder_race` 占的那个 0 号索引，
`class_table.is_defined(0)` 为假 ⇒ `class_table.get(profession)` 恒 `None` ⇒
角色面板的「主属性倾向」那一行永远不出现、`npc_draws` 的职业挂件对玩家永远
查不到。这正是交接文档待裁定第 13 条那个现象的根因。

### 2.2 修法

`build_player_agent` 新增 `profession: ContentIndex` 参数（与既有的
`race: ContentIndex` 参数**同一条理由**：该函数文档已经写明「按任意 race 工作，
不假设调用方只会传人类」，职业是同一维度的东西，不该反而写死）。

默认值取 `content.class_ids.warrior`——**具名句柄，不是字符串字面量、也不是
「第一个已注册的职业」**：

- 字符串字面量在 `crates/` 里出现就违反「引擎不按 id 分支」那条既有纪律；
- 「第一个已注册的职业」依赖注册顺序，而注册顺序是 mod 装载顺序的函数——
  第三方 mod 装在前面就会让本体玩家的默认职业变成别人的职业，那是交接文档
  第五节第 13 条（`ContentIndex` 裸数值不可作判据）的同型问题。

注释**重写不删**（规格 §13）。重写后要说清三件事：脚本通道已拆除、职业内容
在 `classes.json5`、以及**为什么默认是战士而不是别的**。

### 2.3 代价：这会动两条黄金基准

`WorldState::hash` 混入 `agent.profession`（`state.rs:1177`）。玩家的职业索引
从 0 变成 warrior 的索引 ⇒ 世界摘要变 ⇒ 重放摘要也变。与任务 B 合并成**一次**
重冻（见第三节第 3.4 小节）。

---

## 三、任务 B：`Agent` 新增性别字段

### 3.1 类型与取值集合

新模块 `crates/ll-world/src/entity/gender.rs`（`Agent` 所在的那个模块目录）：

```rust
pub enum Gender { Male, Female }
```

**为什么只有两项**：今天唯一的消费者是贴图查找回退链，而已批准的美术批次是
**种族 × 职业 = 117 张**，性别维度一张都没有；多加变体等于凭空要求美术多产
一档，且没有任何裁定支持。P9 婚配系统真正需要更多变体时**追加一个变体是纯
加法**（枚举加变体 + `serde(default)` 已就位），代价低、可逆。

**不是内容注册项**（不进 `Registry`、不占 `ContentIndex`），因此**不存在
「插在中间导致其后条目索引整体平移」那个陷阱**——气候批次踩的是内容表插入，
本批加的是一个 Rust 枚举，两者不同。这一条要在第 ②' 步实测确认，不靠推理。

`Default` 取 `Male`：老存档里的 `Agent` 早于性别这个概念，**没有任何真相可以
恢复**，`serde(default)` 只能给一个占位值。文档要写明它是占位、不是断言。

### 3.2 谁产出它

| 生产者 | 取值 |
|---|---|
| 玩家（`build_player_agent`） | 角色创建界面选的那一个；未经界面时（既有 `build_new_world` 路径）取 `Gender::default()` |
| NPC（`ll_mod::roster::build_npc_agent`） | 按 `DetRng` 从据点/名册确定性抽（约束 C3） |

NPC 必须有性别，否则「渲染层今天就在读它」这条豁免理由只在玩家一个实体上成立，
太单薄。

### 3.3 字段消费者门禁豁免（原文）

`scripts/ci/check_field_consumers.py` 的 `EXEMPTIONS` 新增一条：

```
"Agent.gender": (
    "渲染层今天就在读它：ll_game::surface_draw::npc_draws 把它拼进精灵键的"
    "候选串（<种族>_<职业>_<性别> → <种族>_<职业> → <种族>），"
    "crates/ll-game/tests/npc_appearance.rs 有断言钉住这条链的次序。"
    "决策层消费者随 P9 婚配系统（Kinship）落地——那是本字段被加进来的"
    "原始理由，本批只交付它的存在与渲染消费。"
    "登记日期 2026-08-28；安排：P9「智能体经济与人口模拟」批次开工时，"
    "婚配/血缘接线的同一批必须回来删掉这一条豁免，"
    "或改写成一条说明它为什么仍然只在渲染层被读。"
),
```

**日期与安排必须写进条目本身**，免得变成第二个「`RaceDef.footprint` 零消费者、
问了三次没答复」。

### 3.4 黄金基准重冻：四步，一步都不能少

任务 A 与任务 B 都改 `world.hash()`，**合并成一次重冻**。

| 步 | 做什么 | 期望 |
|---|---|---|
| ① | 改完 A+B 直接跑两条黄金基准 | **红**（否则说明改动没进哈希，那本身是缺陷） |
| ② | 把改动「关掉」——玩家职业改回 `ContentIndex::default()`、`Agent.gender` 不参与哈希且 NPC 不抽性别 | **精确**回到 `10_180_278_885_427_934_050` / `6_885_882_507_408_978_859` |
| ③ | 恢复 | —— |
| ④ | 新常数在**两个独立进程**里各跑一次 | 两次一致 |

**第 ② 步是真正的证据**，气候批次正是在这一步抓到索引平移。本批要单独确认的
是：**`Gender` 不是内容注册项，因此不该有任何索引平移**——第 ② 步若无法精确
回到旧值，说明这条推理错了，必须查明再继续，**不许直接抄新值**。

第 ② 步的验证方式做成**长期活着的东西**：`Gender::default()` + 默认职业维持
现状时的世界摘要不是一条可长期保留的常量（职业默认值改了它就变），因此这一步
只留在提交信息与本文档里，不额外造一条会误导后人的测试。

### 3.5 存档兼容

> # ⚠ 【2026-08-30 更正：本节整段是错的，不要拿它当先例。】
>
> **本节原文保留在下面只为追溯。它被 2026-08-29 的归属批次实测推翻了**，翻案记录在
> `docs/superpowers/plans/2026-08-29-batch10-ownership.md` 五之三节
> 「存档：`serde(default)` 在真正的存档主体上是**空操作**」。
>
> **错在哪**：存档主体走 **`postcard`**（non-self-describing），字节流里没有字段名，
> 反序列化按声明顺序逐字段吃字节。`#[serde(default)]` 需要格式能报告「这个字段缺席」，
> `postcard` **报告不了**。实测（独立最小探针）：老结构体三字段编码 → 新结构体四字段带
> `#[serde(default)]` 解码 → `Err("Hit the end of buffer, expected more data")`。
> **新字段若不在末尾会更糟**：后续字段的字节被错位读成合法值。
>
> **为什么当时没被抓住**：本节那条端到端测试（以及气候批次那条「既有先例」）走的都是
> `serde_json::Value`——**自描述格式，`serde(default)` 在那里确实生效**，
> **测不到真正的 `postcard` 主体那条路**。`Agent::gender` 与 `GroundItemStack::placed`
> 两条「老存档兼容」的论证因此**因果讲反了**。
>
> **今天的正确做法**：**给存档主体加字段一律要动 `CURRENT_SCHEMA_VERSION`**
> （今天是 4，`crates/ll-content/src/save_file.rs:139`），老存档走
> `LoadError::SchemaMigrationGap` 这条**明确拒绝**的路径，而不是被当前字段布局静默误解析。
> 门禁 `scripts/ci/check_save_schema_version.py` 现在盯着这件事。代码侧已修好并自带
> 完整论证：`crates/ll-world/src/entity/gender.rs:56`、
> `crates/ll-world/src/item.rs:372`-`384`、`crates/ll-content/src/save_file.rs:99`-`115`。
>
> 逐条见 [2026-08-29 文档—代码一致性审计](../../../knowledge/audit/2026-08-29-doc-code-audit.md) 一节第 6 条。

~~改的是**主体**（`WorldState` → `Agent`），不是头部 ⇒ 走 `serde(default)`，
`CURRENT_SCHEMA_VERSION` 不动（与气候批次给 `TerrainShape::climate_band_width`
加 `serde(default)` 是同一条既有先例）。~~

~~端到端测试：造一份**缺 `gender` 键**的存档主体 JSON，读回来不崩、性别是
`Gender::default()`。~~ ——**这条测试正是上面说的那个盲区**：它走 JSON，
测不到 postcard 主体。

---

## 四、任务 C：角色创建界面

### 4.1 三项清单全部从注册表现查

生产代码里新增（`crates/ll-game/src/chargen.rs`）：

```rust
pub fn registered_races(content: &LoadedContent) -> Vec<ContentIndex>
pub fn registered_professions(content: &LoadedContent) -> Vec<ContentIndex>
```

实现照抄 `crates/ll-game/tests/npc_appearance.rs:91` 已经在用的那一套
（`registry.snapshot()` 过滤 `*_table.is_defined`）——`snapshot()` 返回 `Vec`，
不经任何哈希容器（约束 C5），顺序即注册顺序。

**测试的反例形式**：不是「断言有 4 个种族」（那会在加种族时变红、变成噪音），
而是「注册一个额外种族之后，界面行数比之前多 1，且新种族的展示名出现在清单
里」——加种族的那一刻界面自动多一项，这条断言恒成立。

性别清单来自 `Gender::ALL`（一个 `[Gender; 2]` 常量），同理由：**不在 UI 层
手抄一张平行清单**。

### 4.2 界面形状

复用 `ll_ui::screen` 的 `ScreenData`（一列已排好版的字符串 + 一个光标），
**不新造控件**。新增 `ScreenState::CharacterCreation { cursor }`，五行：

| 行 | 内容 | 左右键 |
|---|---|---|
| 0 | 种族：`<展示名>` | 在 `registered_races` 里循环 |
| 1 | 性别：`<展示名>` | 在 `Gender::ALL` 里循环 |
| 2 | 职业：`<展示名>` | 在 `registered_professions` 里循环 |
| 3 | 下一步（世界配置） | —— |
| 4 | 返回首页 | —— |

展示名走各自内容表声明的 `display_name_key` → `Catalog::resolve`
（**不按约定拼键**，与 `ll_mod::damage_category` 那条既有纪律一致）。加种族/
职业时它的名字自动出现，不需要碰 `crates/`。

新增的用户可见字符串全部进 `assets/locales/{en,zh-CN}.ftl`
（`check_i18n_strings.py` 门禁）。

### 4.3 贴图查找回退链

所有者裁定：「留个位置，不要复制粘贴」。

```
<种族>_<职业>_<性别>   ← 今天零张
      ↓
<种族>_<职业>          ← 后续美术批次的 117 张
      ↓
<种族> + <职业>         ← 现有分层合成
```

落点是 `crates/ll-game/src/surface_draw.rs`：

- `SurfaceDraw::preferred_key: Option<String>` 改成 `preferred_keys: Vec<String>`
  （`keys()` 的次序语义一个字没变，只是候选从一个变成一串）。
- 身子层候选：`[race_class_gender, race_class, race]`，兜底 `NPC_SPRITE`。
- 职业挂件层新增 `superseded_by: Vec<String>` = `[race_class_gender, race_class]`
  ——**其中任一在图集里查得到就整层不画**。

**为什么必须有 `superseded_by`**：合成图里职业已经画在身子上了，挂件再叠一层
等于同一件事画两遍。今天零张合成图 ⇒ 该字段恒不命中 ⇒ **行为与改动前逐像素
相同**，但 117 张落地那天不会突然人人挂两个职业记号。

判定「查不查得到」是图集的事，因此 `superseded_by` 只声明候选键（纯数据、可测），
真正的查表在 `push_surface_draw` 里一行 `lookup_first(...).is_some()`。

**这条链就是 3.3 节豁免理由里说的「渲染层今天就在读性别」**——`npc_draws` 真的
读 `agent.gender`，否则豁免不成立。

**绝不复制文件**（两份同样的图迟早会漂）。

---

## 五、任务 D：世界配置 + 选出生地

### 5.1 世界配置屏

新增 `ScreenState::WorldSetup { cursor }`，行来自已有数据侧：

| 行 | 数据源 | 左右键 |
|---|---|---|
| 预设 | `ll_content::world_identity::TERRAIN_PRESETS`（4 档） | 循环，切换即整组覆写 |
| 海平面 | `TerrainShape::sea_level` | ±25 |
| 山地阈值 | `TerrainShape::mountain_level` | ±25 |
| 倍频层数 | `TerrainShape::octaves` | ±1 |
| 大陆缩减 | `TerrainShape::continent_shrink` | ±1 |
| 气候带宽 | `TerrainShape::climate_band_width` | ±25 |
| 生成世界 | —— | —— |
| 返回 | —— | —— |

**非法值怎么挡**：每次调整先算出候选 `TerrainShape`，调用**已有的**
`TerrainShape::validate()`；`Err` 就**整体丢弃这次调整**并把返回的中文原因当
提示显示。**绝不在 UI 层抄第二份判据**（与设置屏用 `KeyBindings::try_bind`
判重、不重抄一份是同一条既有纪律）。

预设清单同样是现查（`TERRAIN_PRESETS` 是 `const` 切片，加一档预设界面自动多
一项）。

### 5.2 选出生地：顺序与粒度

顺序**必须**是：角色创建 → 世界配置 → **生成世界** → 选出生地 → 进入。
反过来（先选点再生成）做不到——不生成就没有地图可看。

- `WorldSetup` 的「生成世界」→ `build_new_world(content, params)` →
  存进 `Demo::new_game_draft`（**不是** `Session`，见 5.5）。
- 选点屏用 `ll_ui::hud::world_map::world_map_zone_at_pixel`（鼠标）与
  `WorldMapSlice::zone_at_cell`（键盘光标）——**同一条换算的两个入口**，
  前者内部就调后者。
- 默认停在 `ZOOM_LADDER` 第 0 档，**一格恰好一个区块**（见第〇节）。

### 5.3 全图可见怎么做

批次 5 的 `site_marker_quads` 文档写明了做法：`world_map_slice` 与
`continent_map` 都**显式要求传一份 `&ExplorationMemory`**，选点屏传一份
「全部已探索」的进去，`explored` 恒真，同一份呈现代码自然变成全图可见。

**没有 `reveal_all` 标志**。新增
`ExplorationMemory::fully_explored(layout) -> ExplorationMemory`
（每个区块标一格；`sample_explored` 走的是区块粒度的 `zone_has_any_explored`，
标一格就够）。这份记忆**只活在选点屏的草稿里，绝不写进 `WorldState`**——写进去
等于永久摧毁战争迷雾。

### 5.4 区块内挑陆地格：退化策略

新增 `ll_game::world::pick_spawn_in_zone(...) -> Option<TorusPos>`：

1. `generate_zone_window` 生成该区块窗口（与 `find_spawn_site` 同一条路径）；
2. `largest_walkable_component(window, .., min_area = 1)` 取窗口内**最大**的
   连通可行走分量——不是「随便一格陆地」：一格礁石被深水包围时玩家寸步难行，
   而同一个区块里可能就有一大块陆地。用**已有的**共用算法，不写第二份判据；
3. 在该分量内按 `DetRng::for_entity(seed, zone_key, 0)` 均匀挑一格（约束 C3，
   同一个种子 + 同一个区块恒得到同一格）；
4. **区块内没有任何可行走格时返回 `None`。**

**退化策略：提示玩家重选，不自动换邻近区块。** 理由：

- 玩家点了哪里就是哪里。悄悄换到隔壁等于答非所问——与批次 6「首页读档失败
  不悄悄回退到新游戏」是**同一条已被采纳的纪律**；
- 自动换需要定义「邻近」的搜索顺序、上限、以及「换出去多远算换错了」，那是
  一套新的确定性判据与一堆边界情形，而所有者没有裁定过；
- **最容易反转**：将来所有者要自动换，只需把 `None` 那条分支换成搜索，
  换算、界面、测试一个字都不用动；
- 玩家看得见地图（水是蓝的），点到全水区块是他自己一眼能纠正的操作。

第 3 步「在最大分量内均匀挑」需要分量的成员格列表，而 `LandComponent` 只给
`start`/`center`/`area`。**不改 `LandComponent`**（它进了据点选址的热路径）——
在 `pick_spawn_in_zone` 里按窗口光栅序重扫一遍、用 `LandComponent::start` 做
BFS 起点收集成员。多一次 48×48 的扫描，只在玩家按下确认那一帧跑一次。

### 5.5 草稿类型：接缝在这里

新增 `crates/ll-game/src/chargen.rs` 的：

```rust
/// 一个**尚未进入世界**的角色 + 它要进入的那个世界。
pub struct NewGameDraft {
    pub character: CharacterChoice,   // 种族 / 性别 / 职业
    pub shape: TerrainShape,          // 世界形态旋钮
    pub preset: usize,
    pub world: Option<GameWorld>,     // 「生成世界」按下之后才有
    pub exploration: ExplorationMemory, // 选点屏用的「全图已探索」
    pub cursor: (u32, u32),           // 选点光标
}
```

`Session::begin` 仍然是「世界准备好了，开始玩」唯一的入口——本批只是在它前面
接了三块屏。

---

## 六、必须新增的测试（每条按 ADR 0018 用「故意改坏」验证会红）

1. 玩家的职业不是占位索引，且 `class_table.get(玩家职业)` 查得到。
2. `Agent.gender` 参与 `world.hash()`（改一个实体的性别，摘要必变）。
3. 缺 `gender` 键的老存档主体读得回来，性别是默认值。
4. **加一个种族，角色创建界面自动多一项**（第 4.1 节的反例形式）。
5. 贴图回退链的次序恰好是三段 + 兜底，且合成图存在时职业挂件层被压制。
6. 世界配置屏拒绝非法值，且拒绝后参数**逐字段回到调整前**。
7. `pick_spawn_in_zone` 在全水区块返回 `None`；在有陆地的区块返回的格子
   `!blocks_move`；同一 `(种子, 区块)` 两次调用返回同一格。
8. `world_map_zone_at_pixel` 与键盘光标路径对同一格给出同一个 `ZoneCoord`。
9. 三块屏的状态机：确认/取消的去向、光标不越界、左右键循环。

**全部普通 `#[test]`，零合成按键**（ADR 0025）。

---

## 七、角色创建作为「死亡之后重新入世」入口的接缝

**本批不接线**，但形状必须留对。接缝有且只有两个：

1. **`NewGameDraft::world` 是 `Option<GameWorld>`。** 开局那条路是
   `None → 生成 → Some`；死亡重生那条路是**一开始就是 `Some(现有世界)`**，
   跳过「世界配置」那块屏，直接进「选出生地」。这不是设想——它是同一个类型
   的另一种初始化，`chargen` 的状态机因此要按 `world.is_some()` 决定
   「下一步」去哪块屏，而不是写死一条固定的三屏顺序。

2. **`ll_game::world::build_player_agent(pos, zone, content, race, profession,
   gender, next_action_at)` 是公开的、不碰 `WorldState` 的纯构造器。**
   死亡重生要做的就是「拿一份 `CharacterChoice` + 一个选好的格子 → 造一个新
   `Agent` → `world.actors.spawn`」，与 `spawn_player` 今天做的完全一样。
   本批把它从「`build_new_world` 的私有细节」提升成公开入口，就是为了那一天
   不必把同一段逻辑抄第二份。

**没有做的那一半**（属存档批次）：死亡事件 → 清掉旧玩家实体 → 把
`world.player_entity` 指向新实体 → 存档里「这局玩过几个角色」的记录。

---

## 八、规格没裁定、本批临时选的做法

逐条列在最终报告第 10 节，落地过程中新增的补在那里，不在这里预写。

---

## 九、范围边界（不要越界）

- **不新增任何 `examples/` target**（`run_acceptance_demos.sh` 的登记表因此不动）。
- **不做**死亡重生的接线（第七节只留接缝）。
- **不做**五个新种族、117 张种族×职业贴图（那是后续批次）。
- **不改** `find_spawn_site` 的既有语义（默认路径逐位不变）。
- **不拆** `app.rs`（已 3372 行、既有违规）——**新代码全部放新模块**。

---

## 十、黄金基准与基线测试数

改动前（本会话自己跑的 `bash scripts/ci/run_tests.sh`）：
**112 个二进制、2598 passed / 0 failed / 0 ignored**，exit 0。

两条黄金基准**预期会变**（任务 A 与 B 各自都改 `world.hash()`），走四步重冻，
最终值记在第十一节与提交信息里。

---

## 十一、落地之后：与本计划的偏差

计划是开工前写的，落地过程中有八处与它不符。**逐条如实记录**，不回头改
前面几节假装计划一开始就是这样。

### 偏差 1：任务 A **没有**动黄金基准（计划预测它会动）

第 2.3 节预测「玩家职业从占位索引变成战士 ⇒ 世界摘要变」。**实测两条基准
逐位不变**：`ll-world`/`ll-sim` 都不依赖 `ll-game`，两条基准的世界由各自
的夹具直接构造 `Agent`，根本不经过 `build_player_agent`。

因此重冻从「A+B 合并一次」变成「只有 B 需要」，A 那个提交是干净的。

### 偏差 2：`EXPECTED_WORLD_DIGEST` 也没动，只有重放摘要动了

第 3.4 节预测两条都要重冻。实测 `determinism.rs` 那条的世界
**一个实体都没有 spawn**，`for agent in ..` 循环体一次都不进，新增的
`gender_hash_tag` 那一行对它毫无贡献。只有 `replay.rs` 那条（世界里有两个
真实实体）被重冻：`6_885_882_507_408_978_859` →
`4_180_595_409_733_934_027`。

第 ② 步在两条上都验过（关掉那一行，两条同时精确回到旧值）。

### 偏差 3：NPC 的性别走**独立的一条流**，不是名册那条

第 3.2 节只写了「按 `DetRng` 从据点/名册确定性抽」。落地时发现
`settlement_roster` 的循环里那句注释写得很清楚：「抽取顺序（先种族后职业）
本身是这条流的一部分」。在那条流里再插一次抽取会把**全世界每一座据点的
种族与职业全部重抽**——战争结果、据点存亡、人口构成会跟着全变，那是远超
「加一个性别字段」的世界改动。

于是新开 `ROSTER_GENDER_STREAM_ID`，与名册那条流用同一个坐标
（`据点 id × MAX_ROSTER + 序号`）但不同的流标识。既有名册逐位不变。

### 偏差 4：性别混入哈希走的是**手写标签**，不是 `as u64`

第 3.4 节没写这一层。落地时意识到 `gender as u64` 会让摘要依赖**变体的
声明顺序**——将来在 `Male` 与 `Female` 之间插一个变体，全世界的性别标签
整体平移，摘要跟着变。那正是气候批次踩过的 `ContentIndex` 平移在枚举上的
翻版。改成 `match` 出手写常量（`Male => 1`、`Female => 2`），加变体只需补
一条新常量，已有变体一位不动。

### 偏差 5：`SurfaceDraw` 多了一个 `superseded_by` 字段

第 4.3 节的回退链只说了「身子层候选变成一串」。写的时候发现少了另一半：
身子层一旦落到合成图，**职业挂件层必须让位**，否则 117 张合成图落地那天
人人挂两个职业记号。因此 `preferred_key: Option<String>` →
`preferred_keys: Vec<String>` 之外，还新增了 `superseded_by: Vec<String>`。

今天零张合成图 ⇒ 该字段恒不命中 ⇒ 行为与本批之前逐像素相同。

### 偏差 6：三块屏拆成**三个新模块**，不是一个

第 5.5 节写的是「新增 `crates/ll-game/src/chargen.rs`」。三块屏的状态机 +
排版塞进一个文件会越过 800 行上限（与批次 6 把 `title_screen.rs` 拆出来
是同一次判断）。落地形态是三个文件：`chargen.rs`（角色创建 + 草稿类型 +
三块屏共用的导航帮手）、`world_setup.rs`、`spawn_pick.rs`。共用的类型
（`ScreenState`/`ScreenOutcome`/`ScreenNotice`）仍然只有一份，住在
`menu_screen`。

### 偏差 7：选出生地屏**不画那块居中面板**，它复用整张世界地图 HUD

计划没写这一块屏怎么画。落地形态：`draw_screen` 对 `SpawnPick` 整块早退，
画面全部由 `draw_hud` 那一侧产出——世界地图强制打开，只改**两样东西**：
探索记忆（换成全部已探索）与玩家标记落在哪一格（换成光标那一格），其余
一行不动（`app::SpawnPickHud`）。

理由：玩家在选点屏上看到的地图与他进游戏之后按 M 看到的**必须是同一张**。
另写一份呈现代码等于把「世界地图长什么样」变成两个真相源，两边一旦漂移，
玩家会在「我按着地图选的地方」和「我进去之后看到的地方」之间对不上号。

副作用：`ll_ui::hud::render::world_map_rect` 从私有提升为 `pub`——鼠标
反算（`world_map_zone_at_pixel`）要的正是画图时用的那一份矩形，抄第二份
会让点击系统性偏移一个边距。

### 偏差 8：任务 C 与任务 D 落在**同一个提交**里

任务书要求按 A/B/C/D 分成多个提交。A 与 B 各自独立成提交（且各自门禁全绿），
**C 与 D 没有拆开**，理由是它们在类型层面是一次改动：

- `ScreenState` 是一个枚举，三块屏的变体必须同批加——只加
  `CharacterCreation` 会让 `app.rs` 的两处 `match` 缺臂（非穷尽），编译不过；
- 角色创建屏的「下一步」直接指向 `ScreenState::WorldSetup`，世界配置屏的
  「生成世界」直接指向 `ScreenState::SpawnPick`，三者是一条链；
- 硬拆的唯一办法是先写一版「角色创建完直接进世界」的中间态、下一个提交
  再把它删掉——那会留下一个**没有任何人会去读、且从未打算保留**的中间
  提交，对评审是负价值。

如实登记为一处与任务书的偏差，而不是假装拆过。
