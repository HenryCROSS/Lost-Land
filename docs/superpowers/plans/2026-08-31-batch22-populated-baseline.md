# 批次 22：给确定性黄金基准补上覆盖空洞——新增一条「有人有城有物有势力」的基准

- **日期**：2026-08-31
- **工作树**：`wt-baseline`（分支 `wt-baseline`，基线 `origin/main`）
- **性质**：**不是新功能**。补一条已被实测证明存在的保护空洞。
- **并行批次**：`wt-dialogue2`（改 `ll-sim/dialogue`、`ll-mod/dialogue`、
  `ll-game/player_action.rs`、`ll-ui/screen`）。本批次**不碰**这四处。

---

## 一、问题：两条既有黄金基准各自守着一段，中间有一大块没人守

### 1.1 `EXPECTED_WORLD_DIGEST` 的世界里什么都没有

`crates/ll-world/tests/determinism.rs` 的
`固定种子的四十八乘四十八世界摘要跨平台稳定` 产出 `EXPECTED_WORLD_DIGEST`
（值自己 grep，本文档不抄——见
[2026-08-28 会话交接](../../../knowledge/handoff/2026-08-28-session-handoff.md)
第〇节「三个关键常量：**不要在文档里找它们的值**」，那一节记着同一张表已经
害过三个互不相干的代理）。

它的世界由
`WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn)`
**直接构造**，之后一行都不再动：

- **零 `actors.spawn`** ⇒ `self.actors` 恒是空 `Arena` ⇒ `WorldState::hash`
  里 `for agent in self.actors.iter()` 的循环体**一次都不执行** ⇒ `Agent`
  的每一个字段都在保护之外；
- **零 `stamp_settlement`**（它走 `WorldState::new`，从不建档、不跑编年史，
  `SurfaceStore` 的 `chronicle` 恒为 `None`）⇒ 据点地形、建筑、街道全在保护
  之外；
- **零 `ItemStack`** ⇒ 地面物品、`Owner`、家具放置状态全在保护之外；
- **零势力**（`factions` 表恒空，只有一个长度 0 标记进哈希）。

### 1.2 真正值得看的是那段文档注释本身

`EXPECTED_WORLD_DIGEST` 的文档注释里，已经累积了**四次**「本批次没有重冻，
同一条理由」的记录：

| 批次 | 记录的理由 | 位置 |
|---|---|---|
| 等级与经验系统 | `Agent::level`/`experience`/`xp_to_next_level` 只在 `for agent` 循环体内被读，而这个世界零 `actors.spawn` | `determinism.rs` 常量下方 |
| 装备栏位（P6 第三批） | `Agent::equipment` 同上，「同一条理由」 | 同上 |
| 角色创建（`Agent::gender`） | 同上，且这一次是**实测**不是推断 | 常量上方 |
| 归属（`ItemStack::owner`） | `write_item_stack` 三个调用点一次都不执行；用反例证的 | 常量上方 |
| 势力播种 | 本条重冻了，但**只覆盖了空表的长度标记**——如实写明「不假装它覆盖了势力的内容」 | 常量上方 |
| 据点建筑类型（街道与家具） | 两条反例都做了，都仍绿 | 常量上方 |

**每一条单独看都是诚实的核实结论**——写它们的人都老老实实做了反例验证、
都没有把「跑一遍没红」当成「覆盖到了」。这正是本仓库最值得保留的那条纪律
（ADR 0022、交接文档纪律第 2/6 条）在起作用。

**但把六条叠在一起看，它们是同一个空洞被反复撞到却没人去补。** 六个互不
相干的批次，每一批都在同一个位置停下来写一句「本条够不到」，然后各自走开。
这份文档注释本身已经变成了一份「谁都知道这里有个洞」的公开记录。

**本批次要做的就是把这个洞补上**，并且不动那两条既有常量——它们各自守着
自己那一段（地形生成 / 事件重放），是有价值的。

### 1.3 `EXPECTED_REPLAY_DIGEST` 覆盖面更宽，但仍有大块空白

`crates/ll-sim/tests/replay.rs` 的世界跑真实事件，且 `setup` 里有**两个**
真实 `actors.spawn`，因此 `Agent` 的基础字段（位置/生命/钱包/属性/种族/
职业/性别/等级……）确实在它的保护内。但它的文档注释自己写着：两个实体的
`inventory`/`equipment`/`affiliations` 均为空、`ground_items` 从未被写过、
一次 `stamp_settlement` 都不发生。

**具体谁在里谁在外，本文档不推断，用第二节那张实测表说话。**

---

## 二、A 表：哪些类别在保护内、哪些在外（实测）

方法：**往每一类东西的哈希入口灌一个显眼常量，看两条既有基准动不动。**
灌进去还不红 ⇒ 那一类在保护之外。这是 ADR 0022「覆盖不全的守护等于没有
守护」的直接检验形式，也是交接文档纪律第 6 条（反例验证是硬要求）要求的
「故意改坏」。

每一格的判据是**那一条黄金基准断言本身**是否 FAILED，不是整个文件是否绿
（文件里另有对同一类东西专门写的红/绿测试，它们红了不说明黄金基准咬住了
——那正是「假绿」的反面镜像）。

> **本表由本批次自己重跑，不引用任何既有文档里的结论**（包括
> `EXPECTED_WORLD_DIGEST` 文档注释里那六条记录）。

### 2.1 受测类别、注入点与实测结果

判据：那一条黄金基准断言本身是否 FAILED。**基线提交 `3db8e23`，
Windows，2026-08-31。**

| # | 类别 | 注入点与注入内容 | `EXPECTED_WORLD_DIGEST` | `EXPECTED_REPLAY_DIGEST` | 结论 |
|---|---|---|---|---|---|
| **A0** | **对照**：地形生成阈值 | `TerrainShape::default()` 的 `sea_level` 400→401 | **红** | **红** | 在保护内（对照组，证明实验手法本身能让两条变红） |
| A1 | `Agent` 全部字段 | `WorldState::hash` 的 `for agent in self.actors.iter()` 循环体开头插 `hasher.write_u64(0xDEAD_BEEF)` | 绿 | **红** | **世界摘要之外**，回放摘要之内 |
| A2 | 据点地形（建筑/街道/墙） | `ll_world::settlement::stamp_settlement` 函数体开头插 `grid.set_terrain(grid.world().wrap(0, 0), ctx.ids.wall_stone)` | 绿 | 绿 | **两条都在外** |
| A3 | `ItemStack`（物品 id / 数量 / `Owner` / 鉴定状态……） | `ll_world::state::write_item_stack` 函数体开头插 `hasher.write_u64(0xDEAD_BEEF)` | 绿 | 绿 | **两条都在外** |
| A4 | 地面物品条目（位置 / `dropped_at` / 容器内容 / `placed`） | `WorldState::hash` 的 `for item in &self.ground_items` 循环体开头插 `hasher.write_u64(0xDEAD_BEEF)` | 绿 | 绿 | **两条都在外** |
| A5 | 势力**内容**（非长度） | `FactionTable::write_hash` 的 `for faction in &self.factions` 循环体开头插 `hasher.write_u64(0xDEAD_BEEF)` | 绿 | 绿 | **两条都在外** |
| A6 | 编年史推演 | `ChronicleParams::default()` 的 `epochs` 12→11 | 绿 | 绿 | **两条都在外** |

复现命令（每一行做完都 `git checkout` 还原被改的文件）：

```bash
cargo test -p ll-world --test determinism 固定种子的四十八乘四十八世界摘要跨平台稳定
cargo test -p ll-sim   --test replay      固定种子与固定意图流的世界哈希跨平台稳定
```

### 2.2 A6 与前五条形状不同

编年史**本身不进 `hash()`**（它不进存档，靠种子重跑派生，见交接文档
第〇之二第 9 条）。它进哈希是**间接**的——通过它铺出来的地形、它折叠出来的
势力表、它派生出来的名册。因此 A6 测的是「编年史的推演结果有没有落到任何
一条基准的判据里」，而不是「编年史字段有没有混进哈希」。答案是没有：两条
既有基准都不跑编年史。

### 2.3 一次真实的实验误差，如实记下来

A0 的第一次注入改的是 `crates/ll-world/src/generate.rs:423` 的
`sea_level: 400`——**那一行在 `#[cfg(test)]` 模块里**，是一条单元测试自己
写的字面量，不是生产默认值。注入之后两条基准都绿，若就此收工，结论会变成
「地形阈值也在保护之外」，与事实完全相反。

第二次改的是 `crates/ll-world/src/terrain_shape.rs:106` 的
`TerrainShape::default()`，两条当场都红。

**教训与本批次要补的洞是同一个形状**：「改了一处、测试没红」有两种可能，
一种是判据漏了，另一种是**改动根本没被执行到**。区分它们的唯一办法是**先
有一个已知会红的对照组**。这就是 A0 这一行存在的理由——没有它，A1–A6
的六个「绿」全都可以用「注入点没被执行」解释掉，整张表一文不值。

### 2.4 结论

**两条既有基准合起来，覆盖的是「地形怎么生成」与「两个空手的实体互相打了
几下」。** 玩家真正会走进去的那个世界——铺进地形的据点、物化出来的名册、
带归属的家具、编年史折叠出来的势力表——**一条基准都碰不到**。

`EXPECTED_WORLD_DIGEST` 文档注释里那六条「本条够不到」的记录，因此不是六次
巧合，是同一个空洞的六次目击报告。

---

## 三、B：新增第三条基准——「有人有城有物有势力」

### 3.1 不动既有两条常量

`EXPECTED_WORLD_DIGEST` 守地形生成，`EXPECTED_REPLAY_DIGEST` 守事件重放。
两条都有价值，且并行批次可能正在动它们。**新增第三条，不改前两条。**

### 3.2 它住在哪里：`crates/ll-game/tests/populated_determinism.rs`

**不放进 `crates/ll-world/tests/determinism.rs`**，两条理由：

1. **够不到生产路径**。「有人有城有物有势力」的世界唯一的生产构造点是
   `ll_game::world::build_new_world` + `ll_game::world::materialize_nearby_settlements`
   ——两个都住在 `ll-game`，而 `ll-world` 是它的上游。放在 `ll-world` 里就
   只能在测试里手搓状态，而**手搓的状态跟真实世界不是一回事，这正是既有
   基准的毛病**。
2. **行数棘轮**。`determinism.rs` 已经 711 行（代码行数由
   `check_file_size_budget.py` 判定，注释不计），再往里堆是在给一个已经很
   长的文件加长。新开一个文件同时解决归属与长度两件事。

### 3.3 世界怎么构造（全部走生产路径）

```
ll_game::content::load_content(仓库真实 mods/)      ← 与本体二进制同一条通道
  ↓
ll_game::world::build_new_world(content, GenParams { seed: 固定值, ..default })
  ├─ 地形噪声 + WorldState::new
  ├─ WorldChronicle::generate            ← 三百年兴衰，据点/战争/占领
  ├─ world.factions = chronicle.factions()   ← 势力播种（真实折叠结果）
  ├─ SurfaceStore::install_chronicle      ← 据点真的被 stamp_settlement 铺进地形
  └─ spawn_player                        ← 第一个真实 Agent
  ↓
SurfaceStore::stream_neighborhood(据点锚点)   ← 生产路径每帧都在调的那一个
  ↓
ll_game::world::materialize_nearby_settlements(world, content, roles)
  ├─ ll_mod::roster::settlement_roster    ← 走 DetRng::for_entity（约束 C3）
  ├─ ll_mod::roster::build_npc_agent      ← 若干真实 Agent，带文化 affiliations
  └─ furnish_settlement                   ← 若干 GroundItemStack，
                                             owner = Owner::Faction(site.id)，
                                             placed = true
```

**零手搓**：测试里不出现任何 `world.actors.spawn(..)`、
`world.ground_items.push(..)`、`world.factions = ..` 的直接赋值。
这一点由测试文件自己的一条 grep 级断言之外，更由**代码评审**保证——
本文档在第五节列出「全走生产路径的证据」的取证方式。

### 3.4 规模与耗时的权衡

- 世界尺寸走 `ll_game::world::build_zone_layout()` 的生产常量，**不缩小**。
  缩小就是又一次「为了让基准好看而缩小它的覆盖」。
- 编年史走 `ChronicleParams::default()`，**不缩短纪元数**。缩短会让占领链
  变短、势力表变小，正好削掉本批次要覆盖的那部分。
- 物化范围取**一座**据点（第一座还有人住的），不是全世界两百多座。理由：
  物化全世界要把两百多个区块全部拉进内存，耗时与内存都不可接受，而**一座
  据点已经足以让四类对象全部非空**——存在性断言（第 3.5 节）会把这一点钉死。
  取几座、取哪几座是可调的，取一座是最保守、最容易反转的那一档。
**实测规模与耗时**（`cargo test -p ll-game --test populated_determinism`，
Windows，debug profile）：

| 量 | 值 |
|---|---|
| 已物化据点 | 2（第一座还有人住的据点 + 它邻域里的另一座） |
| 实体（`world.actors`） | 29（1 个玩家 + 28 个 NPC） |
| 其中 `affiliations` 非空 | 28 |
| 地面物品（`ground_items`） | 126 |
| 其中 `placed: true` | 126 |
| 其中 `Owner::Faction(..)` | 126 |
| 势力（`factions`） | 255 |
| 常驻区块 | 25 |
| **测试本体耗时** | **0.47 s** |

0.47 秒里已经包含了「装载仓库真实 `mods/`、生成整张地图的噪声、跑完
十二个纪元的编年史、铺出两百多座据点、物化两座据点的名册与家具」全过程。
**规模上没有任何为了跑得快而做的削减**，因此也没有为此付出可测的代价。

### 3.4.1 「全走生产路径」的证据

测试文件里**零**下列写法（可用 grep 复核）：

```bash
grep -nE "actors\.spawn|ground_items\.push|\.factions =|set_terrain|WorldState::new" \
  crates/ll-game/tests/populated_determinism.rs
```

零命中。测试只调用四个生产函数：`ll_game::content::load_content`、
`ll_game::world::build_new_world`、
`ll_world::surface_store::SurfaceStore::stream_neighborhood`、
`ll_game::world::materialize_nearby_settlements`——前两个是新游戏流程本身，
第三个是 `Demo::maintain_streaming` 每帧都在调的那一个，第四个是
`ll_game::app` 在流式加载之后调的那一个。

**更强的证据是第四节那张反例表**：手搓的状态不会让「往 `stamp_settlement`
里插一行」变红，只有真的走过那条路径的世界才会。

### 3.5 存在性断言：这条基准的全部意义

本会话反复出现的假绿形状是：**断言恒绿是因为被断言的对象根本不存在。**
这条基准的全部意义就是「对象真的存在」，因此**摘要断言之前必须先断言存在**：

```
据点数 > 0     （已物化据点集合非空）
Agent 数 > 1   （玩家之外还有 NPC）
物品数 > 0     （ground_items 非空）
势力数 > 0     （factions 非空）
```

**没有这几条，这条基准会在某次重构里悄悄退化成又一条空基准而没人发现**
——退化后它仍然绿，仍然「有一条基准在守着」，而实际守着的是一个空世界。
这正是 `EXPECTED_WORLD_DIGEST` 走过的那条路。

---

## 四、C：反例验证（ADR 0022 / 交接文档纪律第 6 条）

A 表里**每一类**「保护外」的东西，都要有一行对应的反例结果：改坏那一处，
新基准必须红。逐类实测，结果填在第五节。

某一类若实在进不了这条基准，**如实记为局限并说明原因**——本仓库反复付过
「假装覆盖到了」的代价（ADR 0022 三个实例、交接文档第〇节三次事故）。

---

## 五、D：这条基准红了怎么办

覆盖面变宽的代价是**重冻会变频繁**：今后动内容表、动据点布局、动 `Agent`
字段，这条都会红。**这是它该有的行为**，不是缺陷。操作指引与四步重冻流程
写在新测试文件里那个常量的文档注释上（就近，不另开一份会漂移的副本——
理由与交接文档第〇节同一条）。

四步（交接文档纪律第 2 条，一步都不能少）：

1. **确认基线红**；
2. **把改动关掉，确认精确回到旧值**（这一步才是真正的证据）；
3. **恢复**；
4. **新常数在两个独立进程里复现**。

---

## 六、硬约束核对

| 约束 | 本批次怎么满足 |
|---|---|
| C3（随机走 `DetRng::for_entity`） | 测试自己不取任何随机数；名册派生走 `ll_mod::roster` 内部的 `DetRng::for_entity` |
| C5（禁止哈希容器迭代顺序参与） | 测试不遍历任何 `HashMap`/`HashSet`；`materialize_nearby_settlements` 的据点顺序由 `sort_by_key` + `dedup_by_key` 固定（见其文档「确定性」一节） |
| ADR 0030（不新增 example target） | 新增的是 `tests/` 下的集成测试，不是 example |
| 文件行数棘轮 | 新文件独立，远低于 800 代码行；`determinism.rs` 一行不加 |
| 常量现值 | 全部 grep 取，本文档不抄任何值 |
| 两个独立进程复现 | 新常数写进代码前跑两次独立 `cargo test` 进程 |
| 门禁 | `bash scripts/ci/run_all.sh` exit 0，改前/改后测试数自己跑 |

---

## 七、提交拆分

- **A**：本计划文档 + A 表实测结果
- **B**：新基准测试文件
- **C**：反例验证结果（写进测试文件的文档注释）
- **D**：「这条基准红了怎么办」操作指引
