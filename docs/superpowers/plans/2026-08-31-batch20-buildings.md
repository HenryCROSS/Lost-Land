# 批次 20：据点建筑类型 + 街道间距 + 按类型填家具

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，全文 0 次出现「反例」。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**分支** `wt-buildings`（工作树 `C:/Users/henry/Desktop/迷途大陆/wt-buildings`）。
**基线** `main` 的 `7da38ff`。改前门禁基线：`bash scripts/ci/run_tests.sh`
**EXIT=0，2863 条通过**（`test result: FAILED` 0 次，`LNK1102` 0 次）。

**并行声明**：`wt-uitext` 正在并行跑，它改 `crates/ll-ui/`、
`crates/ll-game/src/hud_draw.rs` 与两份 `.ftl` 的**既有条目**。本批次不碰这三处；
若确有新文案，只**追加到 `.ftl` 文件末尾**。

---

## 〇、关键常量：本文档不列值

按 `knowledge/handoff/2026-08-28-session-handoff.md` 第〇节，三个会漂移的常量
**不在本文档里留副本**。开工与收工各跑一次：

```bash
grep -rn "const EXPECTED_WORLD_DIGEST\|const EXPECTED_REPLAY_DIGEST" \
  crates/ll-world/tests/determinism.rs crates/ll-sim/tests/replay.rs
grep -n "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs
grep -n "pub const CURRENT_SCHEMA_VERSION" crates/ll-content/src/save_file.rs
```

---

## 一、所有者要什么

> 「聚居地的建筑靠这么近，而且只有款式一样的房子，这不像是一个能正常运作的聚居地。」
> 「建筑需要根据他的类型填入不同的家具，例如箱子，椅子，床，书柜等。」
> 「每个物品都会有个主人，一个建筑内的物品通常都是属于某个人的。」

三句话对应三件事：**建筑有类型**（A）、**建筑之间有街道**（B）、
**按类型填家具且家具有主人**（C）。

### 现状核实（grep 复核过，行号自查）

| 事实 | 出处 |
|---|---|
| `BUILDING_SPAN = 5`、`BUILDING_SPACING = BUILDING_SPAN + 1` ⇒ 两栋屋子恒隔 1 格 | `crates/ll-world/src/settlement.rs` |
| `SettlementStatus::Inhabited => house_tiles(..)` 是有人住据点**唯一**的建筑生成函数 | 同上，`stamp_settlement` |
| 本体七件家具（锻炉 + 椅/桌/卧铺/书柜/酒桶/铁箍箱），全部 `furniture: true` | `mods/lostland/items.json5` |
| `Owner::Faction(WorldId)` 已落地，**今天零构造点** | `crates/ll-world/src/ownership.rs` |
| 势力播种已落地，`FactionTable::faction_of(据点 id)` 查得到 | `crates/ll-world/src/faction.rs` |
| 家具在世界生成期**刻意没做**，是一条待裁定 | `knowledge/design/settlements-structures-and-npc-spawning.md` 十二节第 7 条 |
| NPC 物化住在 `materialize_nearby_settlements`（按据点触发，一次到位） | `crates/ll-game/src/world.rs` |
| `crates/ll-game/src/world.rs` 在行数棘轮快照里（960 代码行），**不许涨** | `scripts/ci/file_size_budget.json` |
| `crates/ll-world/src/settlement.rs` 701 代码行，上限 800，余量 99 行 | 本地实测 |

**本批次因此同时回答设计文档十二节第 7 条**：走（a）——世界生成期就摆家具。
（b）「NPC 自己造」仍然是将来的路，两条并存不冲突（引擎侧产出的是同一个
`GroundItemStack`）。落地后**必须回写那份设计文档**（纪律第 9 条）。

---

## 二、A：建筑类型由文化在内容里声明

### 2.1 形状

照 `CultureAttrs.wall_terrain` 那条已经验证过的路：**类型是内容，不是 Rust 枚举**。

`crates/ll-world/src/building.rs`（**新文件**）：

```rust
pub struct BuildingTemplate {
    /// 抽取权重（0 不参与抽取）。
    pub weight: u32,
    /// 屋里摆什么：物品内容索引 + 件数，按声明顺序摆。
    pub furniture: Vec<(ContentIndex, u32)>,
}
```

`CultureAttrs` 新增第七个字段 `pub buildings: Vec<BuildingTemplate>`。

`cultures.json5` 新增**必填**字段（不给 `serde(default)`，与 `wall_terrain`
同一条纪律——漏写的症状是「房子又空了」而没有任何报错）：

```json5
buildings: [
  { weight: 6, furniture: [ { item: "lostland:fur_bed", count: 1 },
                            { item: "lostland:oak_chair", count: 1 },
                            { item: "lostland:iron_bound_chest", count: 1 } ] },
  ...
]
```

### 2.2 分几类、为什么

**四类**：住宅、作坊、仓库、酒馆。理由不是凭空定的——
`mods/lostland/items.json5` 里那七件家具的注释**自己已经写出了这四个去处**
（「住宅与酒馆各摆几把」「仓库与酒馆」「仓库与住宅」「住宅（识字人家）与作坊」）。
本批次做的是把那些注释兑现成内容声明，而不是发明一套新分类。

**每份文化各自决定它有哪几类、各占多少权重**：矿邑作坊多、农庄住宅多、
部落只有住宅与仓库两类（哥布林没有酒馆）。「加一份 `cultures.json5`
就有自己的城镇形态」这条判据由此成立。

### 2.3 类型**不给** id、不给展示名——刻意的

`BuildingTemplate` 里没有 `id` / `display_name_key`。今天没有任何消费者需要
「按名字找一栋酒馆」——加了就是又一个 `buildable`/`diggable`。类型的身份就是
文化声明里的那一条模板，人读的名字写在 json5 的注释里。
**这是一次可反转的收窄**：真需要按名字找建筑时，加一个字段即可，不动结构。
（列进第九节「规格没裁定、临时选的做法」。）

### 2.4 注册期校验（ADR 0017）

`CultureTable::define` 新增两条：

- `CultureError::NoBuildingTemplate`——`buildings` 空或全部权重为 0。
  症状否则是「这份文化的城镇一件家具都没有」，静默。
- `CultureError::TooMuchFurniture`——单条模板的家具件数合计超过
  `MAX_FURNITURE_PER_BUILDING`（= 8，见 4.2）。否则超出的部分被静默丢弃。

跨表引用（家具索引必须是**已定义的、且 `furniture: true` 的**物品）走
`content_audit.rs` 的 `slice_reference`——那是仓库既有的跨表检查落点。

---

## 三、B：街道与密度

### 3.1 规则

两条，都是**纯整数算术，不取随机数**：

1. **巷宽按人口分档**（大城密、村落疏）：
   `alley = 1 / 2 / 3`，由 `site.peak_population.max(site.population)` 分档。
   用峰值人口而不是当前人口：一座城的**建成形态**是它鼎盛时留下的，
   废墟因此保留原来的疏密，不会一被遗弃就散开。
2. **每 `BLOCK_SPAN`（3）个格位插一条街**：格位 → 瓦片的换算从
   `cell * BUILDING_SPACING` 改成
   `cell * (BUILDING_SPAN + alley) + cell.div_euclid(BLOCK_SPAN) * STREET_EXTRA`。
   于是每三栋屋子之后多出 `STREET_EXTRA`（2）格，街道净宽 = `alley + 2`（3~5 格）。

`div_euclid` 而不是 `/`：格位可以是负数（方环由内向外排，锚点在原点），
截断除法会让负半轴的街道错位一格。

**没有引入寻路、最小生成树、道路地形**——街道就是没被建筑占住的那几列格子，
与设计文档把「道路」单列一节的安排不冲突。

### 3.2 确定性（C3 / C5）

- 本节**一次随机数都不取**：巷宽是人口的函数，街道偏移是格位号的函数。
- 建筑类型抽取取随机数，走 `DetRng::for_entity(world_seed,
  SETTLEMENT_FURNISH_STREAM_ID, site.id * MAX_BUILDINGS + building)`——
  **新的流编号**，与 `SETTLEMENT_LAYOUT_STREAM_ID`（门窗）分开，
  改家具不会连带改门窗位置。
- 权重抽取遍历 `Vec<BuildingTemplate>`（声明顺序），不碰任何哈希容器（C5）。
- 环面换算全部走 `TorusSize::wrap`，不手写取模。

### 3.3 `MAX_FOOTPRINT_RADIUS` 跟着变

它由 `MAX_BUILDINGS` / 间距推出，不是可独立调的数值。新公式取**最疏**那一档
（`alley = 3`）算最坏情况。约束仍是「小于据点最小间距的一半」
（`ChronicleParams::min_settlement_spacing` 默认 144 ⇒ 上界 72）。
预估新值 36（旧值 26），**留一条测试钉死这条不等式**。

---

## 四、C：按类型填家具，生成时就带主人

### 4.1 落点：`materialize_nearby_settlements` 的同一趟

家具是 `GroundItemStack`，住在 `WorldState`；`stamp_settlement` 只写
`ChunkGrid` 的地形，拿不到 `WorldState`。因此家具挂在 **NPC 物化那一趟**——
同一个「按据点触发、一座据点只跑一次」的路径，`materialized_settlements`
已经记着谁物化过了，不需要第二套触发机制。

**行数棘轮**：`crates/ll-game/src/world.rs` 在快照里不许涨，因此
`materialize_nearby_settlements` / `place_roster` / `NPC_PLACEMENT_RADIUS`
**整体搬到新文件** `crates/ll-game/src/settlement_spawn.rs`，`world.rs`
只留 `pub use` 转发（既有调用点 `ll_game::world::materialize_nearby_settlements`
一个字都不用改）。world.rs 因此**净变短**。

### 4.2 摆在哪：内壁一圈八格，正中恒留空

5×5 外廓的内部是 3×3 = 9 格。**正中那一格永远不摆家具**，家具按行主序填
外面那一圈 8 格（`MAX_FURNITURE_PER_BUILDING = 8`）。

这一条同时回答「与『每格至多站一人』的相互作用」：

- 放置的家具**独占该格**（`GroundItemStack::placed`，`resolve_drop`/
  `resolve_place` 的前置），但它**不阻挡实体站上去**——两条不变式作用在
  不同的东西上。真正的风险是「把 NPC 生成的位置堵死」，而
  1. **NPC 先摆、家具后摆**：`furnish_settlement` 在 `place_roster` 之后跑，
     并跳过任何已被实体占住的格；
  2. **屋子正中恒空**：每栋屋子至少有一格干净地板；
  3. `place_roster` 追加一条「跳过已有放置物的格」——于是**先来的据点摆下的
     家具不会被后来的 NPC 站上去**。

三条合起来的净效果：家具永不挤掉 NPC，NPC 也不会站进家具里。

### 4.3 归属：`Owner::Faction(SettlementSite::id)`，生成那一刻就带

`crates/ll-world/src/ownership.rs` 的 `Owner::Faction` 文档里**已经写好了这条
裁定**（「『据点归属』用的就是本变体……表示法是
`Owner::Faction(SettlementSite::id)`」，三条理由在那里）。本批次是那条裁定的
第一个构造点，**不改它的语义**。

**为什么不用 `faction_of(site.id)` 换成势力号**：势力下属多座据点，
「这是某某势力的东西」比「这是这座据点的东西」**更宽**；而
`ownership.rs` 已经写明将来的收窄方向是 `Faction(据点) → Npc(住这儿的人)`，
用势力号会让那条收窄多绕一层。（列进第九节。）

**「生成时就带主人」的判据**：`furnish_settlement` 构造 `ItemStack` 的那一处
直接写 `owner`，全流程**没有任何事后回填**——反例验证见 7.2。

### 4.4 废墟不摆家具

`SettlementStatus::Ruined` 直接跳过：没人住的地方没有人的东西。
（名册对废墟本来就是空的，这一条只是把同一条语义补齐到家具这一侧。）

---

## 五、改动清单

| # | 文件 | 改什么 |
|---|---|---|
| A1 | `crates/ll-world/src/building.rs` | **新文件**：`BuildingTemplate`、`MAX_FURNITURE_PER_BUILDING`、内壁八格偏移、按权重抽模板 |
| A2 | `crates/ll-world/src/lib.rs` | `pub mod building;` |
| A3 | `crates/ll-world/src/culture.rs` | `CultureAttrs.buildings` + 两条 `CultureError` + `CultureTable` 一列 + 查询 |
| A4 | `crates/ll-mod/src/content_schema_world.rs` | `RawCulture.buildings`（必填）、`RawBuilding`、`RawBuildingFurniture`，`apply_cultures` 解析 |
| A5 | `mods/lostland/cultures.json5` | 六条文化各自声明建筑类型 |
| A6 | `mods/example_mod/*.json5` | 若它声明了文化，同步（**新条目一律追加在末尾**） |
| A7 | `crates/ll-mod/src/content_hash.rs` | `write_culture_fields` 混入 `buildings`；`CONTENT_HASH_ALGORITHM_VERSION` +1 |
| A8 | `crates/ll-mod/src/content_audit.rs` | `inspect_culture` 记 `CultureAttrs::buildings` + 家具索引的 `slice_reference` |
| A9 | `scripts/ci/check_field_consumers.py` | `CultureAttrs.buildings` 一条豁免（消费者在 `ll-game` 物化层，不在决策层 glob 内，与既有六条同一种处境） |
| B1 | `crates/ll-world/src/settlement.rs` | 巷宽分档 + 街道偏移 + `building_origin` 改写 + `MAX_FOOTPRINT_RADIUS` 重推；`building_origin` 转 `pub` |
| B2 | `crates/ll-world/tests/settlement_layout.rs` | **新文件**：街道、密度分档、占地半径三组断言 |
| C1 | `crates/ll-game/src/settlement_spawn.rs` | **新文件**：搬来 NPC 物化 + `place_roster`，新增 `furnish_settlement` |
| C2 | `crates/ll-game/src/world.rs` | 删掉搬走的三段，`pub use` 转发（净变短） |
| C3 | `crates/ll-game/src/lib.rs` | `pub mod settlement_spawn;` |
| C4 | `crates/ll-game/tests/settlement_furniture.rs` | **新文件**：端到端——真实 mod、真实世界、家具真的在屋里、真的带主人 |
| D1 | `knowledge/design/settlements-structures-and-npc-spawning.md` | 十二节第 7 条**原文保留**，原地追加「已裁定并落地」的更正段，指回本文档（纪律第 9 条） |
| D2 | `crates/ll-world/src/ownership.rs` | `Owner::Faction` 文档里「本批次因此今天没有任何一处真的构造本变体」**原文保留 + 原地更正**，指向本批次的构造点 |
| D3 | `knowledge/design/README.md` / 交接文档 | 落地状态回填 |

---

## 六、黄金基准四步重冻

**预判（必须用证据推翻或坐实，不许当结论）**：两条黄金基准的世界里
**都没有据点**——`determinism.rs` 走 `WorldState::new`（不建档、不跑编年史），
`replay.rs` 的 `setup` 只 spawn 两个实体。因此本批次**很可能一条都不用重冻**。

**这正是交接文档点名的第二类陷阱**（「基线没红是因为那个测试的世界里根本
不存在这类对象」）。因此**必须用反例证伪，不能用「跑一遍没红」当结论**：

1. **① 确认基线红**：改完实现、常量不动，跑两条。
2. **② 把改动关掉，确认精确回到旧值**：把街道偏移那一行改回
   `cell * BUILDING_SPACING`、把 `furnish_settlement` 的调用注掉，其余全部保留。
3. **③ 恢复**。
4. **④ 新常数在两个独立进程里复现**。

**若 ① 没红**（预期如此），补做**灌常量反例**：

- 往 `settlement.rs` 的 `write_footprint` 开头插一行显眼常量写入 /
  把 `stamp_settlement` 的建筑数临时改成 9999 → 两条仍绿 ⇒ 证明这两个世界里
  **一栋据点建筑都没有**。
- 往 `write_item_stack` 开头插 `hasher.write_u64(0xDEAD_BEEF)` → 两条仍绿 ⇒
  证明这两个世界里**一个 `ItemStack` 都没有**（既有记录已有此结论，本批次
  重新实测一次而不是引用）。

两条反例都做完、都仍绿，才允许写「本批次没有重冻」，并把反例过程记进两个
常量的文档注释里。

### 存档 schema

`GroundItemStack` / `WorldState` 的**字节布局一个字段都不动**（地面物品只是
变多）。`scripts/ci/check_save_schema_version.py` 预期保持绿；**若它报了，
照它的提示做**（升 `CURRENT_SCHEMA_VERSION` + `--bless`），不绕过。

### 内容哈希

`CultureAttrs` 加字段 ⇒ `CONTENT_HASH_ALGORITHM_VERSION` **必须 +1**
（现值自己 grep）。新内容条目（家具引用、`buildings` 块）**追加在既有条目
末尾**，不插在中间——气候批次那次 `ContentIndex` 整体平移就是这么来的。

---

## 七、ADR 0018 反例验证（每条新断言都要故意改坏一次）

本批次**必验的三条**（任务书点名）：

| # | 要验的事 | 怎么改坏 | 该红的断言 |
|---|---|---|---|
| ① | 建筑类型真的由文化决定 | 把某文化的 `buildings` 换成另一份（如把矿邑的作坊换成住宅） | 「不同文化的据点家具构成不同」 |
| ② | 家具真的带着主人生成 | 把 `owner` 那一行改成 `Owner::Unowned` | 「据点家具的归属恒指向该据点」 |
| ③ | 街道真的留出来了 | 把街道偏移改回 `cell * BUILDING_SPACING`（1 格间距） | 「相邻建筑块之间存在宽度 ≥3 的连续空地」 |

外加：把 `MAX_FURNITURE_PER_BUILDING` 校验删掉 → 「家具件数超限当场拒绝」该红；
把「跳过被实体占住的格」删掉 → 「家具不与 NPC 同格」该红。

---

## 八、下游影响要量化

据点变大变疏会改变什么？**去测，不是去猜**。改前/改后各跑一遍，报实测数字：

- `crates/ll-game/tests/npc_materialization.rs`（物化出的 NPC 数、据点数）
- `crates/ll-game/tests/culture_and_war.rs`（战争场次、存活据点、活人口）
- `crates/ll-world/tests/faction_seeding.rs` / `faction_fold.rs`（势力数）
- 跨区块据点比例（`settlement.rs` 模块文档「实测」一节那张表要跟着更新）

预期方向：占地半径 26 → 36 ⇒ **跨区块据点比例上升**、每座据点覆盖的区块数上升；
战争与人口**不应该变**（编年史一个字都不动）。若战争数变了，说明有隐藏耦合，
必须查明。

---

## 九、规格没裁定、本批临时选的做法（收工时逐条搬进最终报告）

1. **建筑类型不给 id / 展示名**（2.3）——最保守、最容易反转。
2. **归属取据点号而不是势力号**（4.3）——跟随 `ownership.rs` 已写下的裁定。
3. **巷宽用峰值人口分档**（3.1）——让废墟保留原有疏密。
4. **街区 3×3、街宽 = 巷宽 + 2**——数值凭观感定，一处常量可调。
5. **废墟不摆家具**（4.4）。
6. **屋子正中恒留空**（4.2）——把「不堵死 NPC」变成结构性保证而不是概率。

---

## 十、提交切分

- 提交 1（A）：建筑类型的内容层 —— `building.rs` + `culture.rs` + schema +
  `cultures.json5` + 内容哈希 + 花名册 + 字段门禁豁免。
- 提交 2（B）：街道与密度 —— `settlement.rs` + 新测试 + 占地半径。
- 提交 3（C）：按类型填家具 + 归属 —— `settlement_spawn.rs` + 端到端测试。
- 提交 4（D）：文档回写（设计文档、`ownership.rs`、交接、README）+ 四步证据。

中文提交信息。**不 push、不合 main。**

---

## 十一、落地结果（收工回填）

### 11.1 与计划不同的地方

| 计划 | 实际 | 为什么 |
|---|---|---|
| 建筑类型「四类」写死在本体 | 四类**由本体六份文化各自挑用**（部落只用两类），引擎不认类型的名字 | 见 2.3；判据是「加一份 `cultures.json5` 就有自己的城镇形态」 |
| 家具计划与写入都在 `ll-game` | **计划下沉到 `ll_world::building::settlement_furnishing`**（纯函数），只有写入留在 `ll-game` | 这样一份 `cultures.json5` 的文本经真实解析路径就能直接答出「这份文化的城镇怎么摆家具」，不必先造一个世界——`crates/ll-mod/tests/culture_town_shape.rs` 正是靠这条 |
| 街道偏移用 `div_euclid` | 改成**先取绝对值再除、再贴回符号** | `div_euclid` 数学上对但**不对称**，负半轴的第一个街区只有两个格位，占地半径在负方向超界。`settlement.rs` 的既有单测 `外廓半径上界真的是上界` 当场抓住了它 |
| 存档 schema 预期不动 | **4 → 5** | 见 6.2 |
| —— | **多修了一个先于本批次就存在的缺陷**：出生点可能落在某栋屋子的 3×3 内壁里 | 见 11.4 |

### 11.2 四步重冻：两条黄金基准**都没有重冻**，用反例证的

改完实现之后两条**当场就是绿的**（第 ① 步没红）。按第六节的预案补做反例：

- **反例 A（据点）**：往 `ll_world::settlement::stamp_settlement` 开头插一行
  `grid.set_terrain(grid.world().wrap(0, 0), ctx.ids.wall_stone);` ⇒
  `determinism.rs` 十一条、`replay.rs` 七条**仍然全绿**。证明这两个世界里
  **一次据点铺设都没发生过**。
- **反例 B（家具）**：往 `ll_world::state::write_item_stack` 开头插
  `hasher.write_u64(0xDEAD_BEEF)` ⇒ 两条仍然全绿。证明这两个世界里
  **一个 `ItemStack` 都没有**。

因此**第 ② 步（把改动关掉，确认精确回到旧值）在本批次没有对象**——没有
「新值」需要回退验证。两个常量的字面值一位未动，证据写进了它们各自的
文档注释。

### 11.3 下游影响：实测数字（三个种子 20260826 / 7 / 99，真实 `mods/`）

| 指标 | 改前（恒 1 格间距） | 改后（街道 + 分档） |
|---|---|---|
| 据点总数 / 存活 / 活人口 | 765 / 703 / 28588 | 765 / 703 / 28588 |
| 建筑总数 / 占领事件 / 遗弃事件 / 势力数 | 15206 / 21 / 119 / 822 | 15206 / 21 / 119 / 822 |
| **跨区块据点** | 58 | **223** |
| 覆盖区块总数 | 1027 | **1798** |
| 单座最多覆盖区块 | 8 | 8 |
| `MAX_FOOTPRINT_RADIUS` | 26 | **36** |
| 全世界可摆家具数（计划值） | 0 | **55824** |

物化一座据点（把流式邻域挪到第一座有人住的据点上）：

| 种子 | 改前 NPC / 家具 | 改后 NPC / 家具 |
|---|---|---|
| 20260826 | 24 / 0 | 24 / **133** |
| 7 | 11 / 0 | 11 / **26** |
| 99 | 24 / 0 | **28** / **29** |

编年史那一整行逐个数字不变，是本批次最重要的一条下游结论：
`chronicle` 一个字没动、也不读任何建筑几何。种子 99 的 NPC 从 24 涨到 28
是**街道的副作用**：建筑摊开之后，`place_roster` 的方环扫描在半径 26 内
找得到更多可站立的格，那座据点名册里原本「找不到位置就这一局不出现」的
四个人现在出得来了。

### 11.4 顺带修掉的一个既有缺陷

`find_spawn_site` 读的是**基础地形**（据点还没盖上去），因此它选的那一格
可能正好落在某栋屋子的 3×3 内壁里——玩家在一间关着门的屋子里开局，
连通可行走面积从几千掉到 9。街道把建筑摊开之后，
`crates/ll-game/tests/worldgen_params_e2e.rs` 的
`四档预设都能建出带玩家实体且出生点连得开的世界` 当场红了。

修法：`ll_game::settlement_spawn::spawn_outside_buildings`——出生点落在
建筑外廓里就按方环由内向外挪到最近的一块屋外空地（半径上限
`2 × BUILDING_SPAN`），找不到就原样保留（§10.2 降级而非崩溃）。

**这不是本批次引入的回归**：那条路径在街道落地之前就走得到，只是本体
默认种子没踩上。

### 11.5 门禁

`bash scripts/ci/run_all.sh` **exit 0**。改前基线 2863 条通过，改后见提交
信息里记的数字。

`scripts/ci/check_file_size_budget.py --bless` 刷新了五个条目：
`world.rs` **960 → 871**（净减 89，据点物化整段搬进
`settlement_spawn.rs`），`content_hash.rs` +70（新的
`write_culture_fields` 尾段与它的单测）、`content_audit.rs` +11（新字段
与家具索引的跨表引用检查）、`roster.rs` +2 与 `chronicle.rs` +2（各两处
测试夹具补 `buildings` 字段）。四处上涨都是「这张表的字段检查必须住在
这张表的检查函数里」，拆出去只会让字段与检查两地分居。

### 11.6 顺手记下的两处**门禁自身**的缺口（本批次没修）

1. `scripts/ci/check_save_schema_version.py` 的 `EXEMPTIONS` 字典
   **声明了却从未被用来跳过任何比对**——全文只有一处引用它，那是
   「死豁免要清理」那条反向检查。那份豁免机制今天是空转的。
2. `crates/ll-mod/src/content_hash.rs` 的
   `CONTENT_HASH_ALGORITHM_VERSION` 文档「版本 27」一节写着守门的是
   「本模块单元测试 `建材不同的两条文化摘要不同`」——**那条测试从来
   没有存在过**（`grep` 只命中那句注释自己）。本批次新增的
   `建筑类型不同的两条文化摘要不同` 顺带补上了那个缺口。
