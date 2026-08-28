# 批次 3：气候条带

**基线**：`03555cd`（main，`Merge branch 'wt-genmodset'`）
**工作树/分支**：`wt-climate`
**规格出处**：§7.1「气候为周期性条带」、决策表第 23 条「真环面……气候改用周期性条带」

这一项被历轮交接**四次指定归属 P7、五轮提醒失效、至今零实现**
（`knowledge/audit/2026-08-26-phase-reckoning-p6-p8.md`、
`knowledge/handoff/2026-08-27-session-handoff.md` 五节第 3 条）。本批次
认领它并落地。

---

## 一、规格原文（不是发挥）

§7.1：

> **气候为周期性条带**：两条赤道 + 两条极圈。玩家持续向北将穿越极地后
> 重新进入热带。此设定纳入世界观——「迷途大陆」即一片没有边缘、首尾
> 相接的土地。

决策表第 23 条：

> **真环面**：东西南北四面全连通；**气候改用周期性条带**。仅适用于
> 大陆世界地图层。

两条合起来钉死了形状：**纬度是 `y` 的周期函数，一个完整的 `y` 周期里
恰有两条赤道与两条极圈**。环面上没有极点，因此不许套用「一条赤道 +
两个极点」的球面模型。

---

## 二、范围（含一次已记录的范围变更）

### 2.1 原定范围：零新地形

本计划最初的约束是「让纬度调制『高度 → 地形』这个映射，**不新增任何
地形种类**」——干热带低海拔复用海岸的 `sand`、极地带低海拔复用雪线的
`snow`。理由是把本批次的影响面压到最小。

### 2.2 【范围变更 · 2026-08-27】所有者批准新增地形与贴图

**上面那条「零新地形」约束由派工方提出，已被项目所有者推翻，作废。**
所有者原话：

> 「我觉得你可以添加新的地形以及贴图。」

本仓库惯例是**追加更正段、不改正文**（见 §7.1 本身那一串「本节取代
说明」），因此 2.1 原样保留供追溯。

改成什么：不再把沙漠塞进海岸带那个 `sand`、把冻原塞进雪线那个 `snow`，
而是**新增两种独立地形**，让语义各归各位：

| 新地形 | 与既有哪一种区分 | 为什么不能合并 |
| --- | --- | --- |
| `lostland:desert`（沙漠） | 海岸的 `lostland:sand` | 一个是海滩，一个是沙漠；`move_cost`、日后的资源亲和与文化 `home_terrain` 都会不同 |
| `lostland:tundra`（冻原） | 高山的 `lostland:snow` | 一个是雪线以上的峰顶，一个是低地冻土；高度带完全不同 |

**只加两种，不加更多**（无「热带雨林」「盐碱地」「针叶林」等）：够表达
「气候条带真的存在」即可，多的留给后续内容批次（五族 + 骆驼人文化那
一批）。

### 2.3 明确不做（必须显式记账，不许沉默推迟）

本仓库对沉默推迟有专门的记账纪律——**气候条带自己就是被沉默推迟五轮
的反面教材**。因此以下四项在此显式登记：

1. **温度与天气不接纬度。** 让 `ll_world` 的温度读纬度是自然的下一步，
   但温度有自己的消费者（`ll-sim` 的暴露/体温那一路），一起做会让本批
   影响面翻倍、且要重冻更多基准。**推迟到：温度/体温那一批**，不是
   「以后再说」。
2. **森林/丘陵/山地三带不随纬度变化。** 本批次只调制「海岸带以上的
   第一段陆地」这一段（今天的 `grass` 带）。极地的森林（针叶林/泰加）
   与干热带的高地森林是否该另有地形，**推迟到后续内容批次**。
3. **`mods/lostland/cultures.json5` 五份文化的 `home_terrain` 一个都
   不改。** 改它会平移据点分布、把本批的下游影响数字搅浑。沙漠文化是
   后续那批的事，本批只负责让沙漠这种地形**存在**。
4. **干热带与极地带宽度不可分别配置**（一个旋钮对称控制两侧）。若日后
   需要非对称，改法是把 `climate_band_width` 拆成两个字段，不是推翻
   本批结构。

---

## 三、动手前的既有事实（基线 `03555cd` 已 grep 核实，你仍要自己复核）

| 事实 | 位置 |
| --- | --- |
| 地形完全由高度决定，八段阈值链 | `crates/ll-world/src/generate.rs` 的 `height_to_terrain` |
| 阈值取自 `TerrainShape`（`sea_level` 400 / `mountain_level` 750） | 同上，`TerrainShape::default` |
| 全仓库 `latitude`/`纬度` 零命中 | 确认零实现 |
| `TerrainShape` 已进存档、已进世界身份四要素 | `ll_content::world_identity::WorldIdentity` |
| 本体 17 种地形在 Rust 里声明，不在 `mods/lostland/*.json5` | `crates/ll-world/src/terrain.rs` 的 `materialize_base_terrain` |
| 地形 → 图集键的生产路径 | `ll_game::layout::terrain_entry_name` / `terrain_atlas_key` |
| 贴图由 `tools/ll-artgen` 生成，**不手画** | 该 crate 模块文档 |
| `LooseOnlyEntry` 是「只进松散贴图树、不进遗留共享画布」的既有通道 | `tools/ll-artgen/src/main.rs` |
| 五个更早批次的验收 demo `include_bytes!` 遗留共享画布，其像素是冻结基准 | 同上 |

---

## 四、设计裁定

### D1：纬度是 `y` 的**整数三角波**，周期取世界高度的一半

规格要求「一个完整 `y` 周期里两条赤道 + 两条极圈」。三角波在一个周期
内恰有一个极大与一个极小，因此**取周期 `P = H / 2`**（`H` 是世界瓦片
高度）就得到整张世界两个极大（赤道）与两个极小（极圈）。

落点：

```
y = 0      → 赤道
y = H/4    → 极圈
y = H/2    → 赤道
y = 3H/4   → 极圈
y = H      → 赤道（≡ y = 0，接缝天然闭合）
```

**值域用千分比**（与 `TileableNoise` 输出、`Milli`、`TerrainShape` 三处
既有惯例同一套）：`warmth(y) ∈ 0..=1000`，1000 = 赤道，0 = 极圈。

公式（**全整数，无 `f32`/`f64`，无 `sin`/`cos`**）：

```
p = H / 2
t = y.rem_euclid(p)            // 0..p
warmth = if 2*t <= p { 1000 - 2000*t/p } else { 2000*t/p - 1000 }
```

**为什么不许用三角函数**：ADR 0002 与 `docs/architecture/05-integer-discipline.md`
——IEEE 754 不规定超越函数精度，不同 libm 结果不同，跨平台逐位相同当场
失效。这正是 §7.1 当年把「4D 噪声投影」改成「模格点」的同一条理由，
不要在气候这里把它重新引进来。

**`H` 从哪来**：`TileableNoise` 构造时已经算出 `period_y = H / CELL_SIZE`，
本批次给它加一个 `tile_height()` 读取器（`period_y * CELL_SIZE`）。
**不新增第二个真相源**：世界高度只在噪声构造那一处被确定。

### D2：接缝连续性是**构造上**的，并补一条与噪声同型的属性测试

`H` 是 `CELL_SIZE`（16）的整数倍 ⇒ `p = H/2` 是 8 的整数倍 ⇒
`H.rem_euclid(p) == 0 == 0.rem_euclid(p)` ⇒ `warmth(0) == warmth(H)`
**恒成立，与 `H` 的具体取值无关**。

规格 §14.2 的属性测试表里已有「可平铺整数噪声接缝处连续：
`terrain(0, y) == terrain(W, y)`」这一条。本批次补一条**同型**的：
`climate_band(0) == climate_band(H)`，以及 `warmth(0) == warmth(H)`，
用 `proptest` 对任意合法世界高度成立。

### D3：气候只调制「海岸带以上第一段陆地」

阈值链其余七段**一个字不改**。第四段（今天的 `grass`）按气候带三选一：

| 气候带 | 该段地形 |
| --- | --- |
| 干热带（Hot） | `lostland:desert`（新） |
| 温带（Temperate） | `lostland:grass`（不变） |
| 极地带（Polar） | `lostland:tundra`（新） |

**温带逐位维持今天的行为**——这是 D4 那个「关掉即恒等」的性质能够成立
的前提。

### D4：做成可配置旋钮 `TerrainShape::climate_band_width`（千分比）

**做，理由三条**：

1. **它给出黄金基准重冻第 ② 步的永久证据。** 纪律要求「把改动关掉，
   确认精确回到旧值」。有旋钮就不必临时改坏代码再改回——`0` 就是恒等，
   而且可以钉成一条**长期活着**的回归测试，不是一次性的手工验证。

   > **【修订 · 落地时】** 本条原计划把那条长期测试写成「带宽为零时
   > **世界摘要**等于旧的黄金基准值」。**做不到，且不该做**：
   > `WorldState::hash` 必须混入 `climate_band_width`（地形是流式生成
   > 的，带宽不进摘要，「带宽没有正确随存档往返」这条缺陷就没有任何
   > 确定性回归测得出来，见 `write_terrain_shape` 的文档），于是即便
   > 带宽为零，摘要也会因为多写了一个 0 而与旧值不同。
   >
   > 改成钉**地形本身**：`气候带宽为零时整张地形与气候条带落地之前逐格
   > 相同`（`ll_world::generate`，用测试里独立重抄的旧阈值链当黄金参照）
   > 与 `带宽为零时任何纬度都是温带`（`tests/climate_blackbox.rs`，
   > proptest）。这两条比摘要那条更强——摘要只说「有没有变」，它们说的是
   > 「哪一格变了」。
   >
   > 「摘要精确回到旧值」那一步仍然真实做过，只是作为**一次性**的四步
   > 重冻证据写进提交信息与两条基准常量的文档，见六节。
2. **它让五个更早批次的验收 demo 的冻结像素基准零漂移。**
   `p2_acceptance`（512×320 真实生成世界）与 `p5_coordinate_acceptance`
   是唯二从噪声生成地形的 demo，把它们的 `GenParams` 钉到
   `climate_band_width: 0`，那两张图的地形分布与今天**逐格相同**，
   遗留共享画布也因此不需要多两个条目（见 D6）。
3. **它跟着 `TerrainShape` 那套既有形态参数走**，不新开第二套机制：
   已进存档、已进世界身份、已有 `validate`、已有 `config.json5` 入口。

语义：

```
干热带 ⇔ warmth >  1000 - climate_band_width
极地带 ⇔ warmth <         climate_band_width
其余    ⇔ 温带
```

- `climate_band_width == 0` ⇒ 两条判据分别是 `warmth > 1000` 与
  `warmth < 0`，**恒假**，整图温带 ⇒ **逐位恒等**。用严格不等号正是
  为了让 0 是真正的恒等，而不是「只剩赤道那一行还是热的」。
- 合法区间 `0..=500`（`validate` 拒绝越界）。上界取 500：再大两条带
  就会互相穿插，温带消失，与 `MIN_LEVEL_GAP` 拒绝阈值链穿插是同一条
  纪律。
- 默认 `250`：干热带与极地带各占纬度的 25%，温带 50%。

**serde 兼容**：新字段带 `#[serde(default = ...)]` 回落到默认值，老存档
照常读得开、无 schema 升级——与 `SaveHeader.terrain_shape` 那次同一条
处理方式。

### D5：两种新地形的属性取值

`TerrainDef` 只有四个字段（`blocks_sight`/`blocks_move`/`move_cost`/
`opens_into`，见 `ll_mod::content_hash::write_terrain_fields`），没有
可建造性字段，因此只需定三项：

| 地形 | `blocks_sight` | `blocks_move` | `move_cost` | 理由 |
| --- | --- | --- | --- | --- |
| `lostland:desert` | false | false | 140 | 松软深沙比海滩 `sand`（120）更费力，但远不到丘陵/森林（150）那一档 |
| `lostland:tundra` | false | false | 130 | 冻土比草地（100）难走，比高山积雪 `snow`（150）好走——冻土是硬的，雪线上的是松雪 |

两者都不阻挡视线：开阔地貌，与 `sand`/`snow` 同一档。

**追加在 17 种之后**，索引顺序不插队——与批次 2「`lostland:cultureless`
必须追加在末尾」同一条纪律（插队会平移其后每一种地形的 `ContentIndex`）。

### D6：贴图走 `LooseOnlyEntry`，**不进** `assets/atlas/placeholder.json`

`tools/ll-artgen` 已经有这条平行通道，它的模块文档写得很清楚：新增内容
不该塞进遗留共享画布，因为那张画布是五个更早批次验收 demo 的冻结像素
基准，塞进去只会把它们卷进来。

因此：

- 在 `tools/ll-artgen` 新增两条 `TerrainSpec` 配方（沙漠色、冻原色），
  并让 `loose_only_entries()` 除 NPC 之外也产出这两条地形尺寸的条目
  （16×16、`pivot (0,0)`、`footprint 1×1`，与既有地形条目同档）。
- `assets/atlas/placeholder.json` **一个字不动**，
  `assets/atlas/placeholder.png` 因此逐字节不变。
- 产物：`assets/sprites/terrain_desert.png`、`assets/sprites/terrain_tundra.png`
  + `assets/sprites/manifest.json5` 两条新条目，由运行期
  `ll_render::atlas_pack::pack_atlas` 打包（`crates/ll-game/tests/atlas_coverage.rs`
  会自动开始管它们——那条测试从注册表现查）。

配色（与 `assets/atlas/README.md`「地形色块」既有取值拉开距离）：

- 沙漠 `(198, 154, 86)`：比海滩 `sand (214, 196, 140)` 更深更橙，
  一眼分得出「这不是海滩」。
- 冻原 `(196, 206, 208)`：比高山 `snow (238, 240, 244)` 更暗更青，
  一眼分得出「这不是峰顶的雪」。

### D7：两个从噪声生成地形的 demo 钉到「无气候」

`crates/ll-world/examples/p2_acceptance` 与
`crates/ll-sim/examples/p5_coordinate_acceptance` 各自 `GenParams::default()`
生成真实地形。把它们改成显式 `climate_band_width: 0`：

- 它们的冻结像素基准（`crates/ll-world/tests/visual/baseline/p2_acceptance.png`）
  **零漂移**，不需要重冻，也不需要一台能跑 GPU 的机器去重截；
- 它们的地形映射表（各自的 `terrain_entry_name`）因此**不需要**多认两种
  地形，也就不需要在遗留共享画布里凭空多两个条目，D6 那条约束保持成立；
- demo 的定位本来就是「验它那一阶段的那件事」，不是「展示最新世界生成
  形态」——本体二进制 `ll-game` 走的是真默认值，气候条带在真游戏里是开
  着的。

**这条要在两个 demo 里各写一段注释说明**，否则下一个人会以为是漏改。

### D8：地形占比的改前/改后对比写成**测试**，不是新增一个 `probe_*` example

下游影响必须给出改前/改后的地形占比对比数字，而仓库里没有现成的地形
占比测量工具（`probe_conquest` 只测战争结局，`probe_content_hash` 只测
内容哈希）。

**【修订 · 2026-08-27】** 本节第一版写的是「新增
`crates/ll-game/examples/probe_climate.rs`，定位与另外三个 `probe_*`
相同」。那一版真的写出来了、也真的跑出了数字，随后**删掉并改成测试**
（`crates/ll-world/tests/climate_terrain_mix.rs`）。两条理由：

1. 主干新增了门禁 `scripts/ci/run_acceptance_demos.sh`，工作区里每个
   example target 都必须显式登记进 RUN_LIST/SKIP_LIST 并长期维护——为
   一次性测量养一条 demo 不划算；
2. **更要紧的**：example 只打印、不断言。下次有人把气候调制泄漏到别的
   高度段，一个只会打印的探针一声不吭。写成测试之后，同一批数字变成了
   三条断言：其余七类地形逐格不动、草地被切走的部分精确等于沙漠加冻原、
   三条气候带按带宽切分纬度。

「改前」那一列由 `climate_band_width = 0` 产出——这一列同时被
`气候带宽为零时整张地形与气候条带落地之前逐格相同` 那条测试证明**确实
等于落地前的行为**，不是另跑一个近似基线。表本身用
`cargo test -p ll-world --test climate_terrain_mix -- --nocapture` 打出来。

---

## 五、必须新增的测试（每条都按 ADR 0018 用「故意改坏」验证真的会红）

| 测试 | 钉住什么 | 反例 |
| --- | --- | --- |
| `气候纬度在南北接缝处连续`（proptest） | `warmth(0) == warmth(H)`，任意合法 `H` | 把周期从 `H/2` 改成不整除 `H` 的常数 |
| `气候带在南北接缝处连续`（proptest） | `climate_band(0) == climate_band(H)` | 同上 |
| `一个世界高度里恰有两条赤道与两条极圈` | 规格钉死的形状本身 | 把周期改成 `H`（只剩一条赤道一条极圈） |
| `气候带宽为零时整张地形与气候条带落地之前逐格相同` | D4 的恒等性质 | 把 `>` 改成 `>=` |
| `带宽为零时任何纬度都是温带`（proptest） | 同上，任意世界高度与纬度 | 同上 |
| `气候条带只改草地那一段其余七类地形逐格不动` | D3「只调制一段高度带」 | 让森林那一段也按气候带三选一 |
| `草地那一段被原样切成草地加沙漠加冻原一格不多一格不少` | 调制是重新分配而不是增删 | 干热带那一支改回 `terrain_ids.sand` |
| `三条气候带按带宽切分纬度且温带占一半` | 默认带宽 250‰ 的语义 | 把三角波改成非线性形状 |
| `干热带低海拔是沙漠而不是草地` | 沙漠真的会出现 | 让调制函数恒返回 `grass` |
| `极地带低海拔是冻原而不是草地` | 冻原真的会出现 | 同上 |
| `沙漠与海岸沙地是两种不同地形` | D5 的语义分离 | 把 `desert` 指回 `ids.sand` |
| `冻原与高山雪地是两种不同地形` | 同上 | 把 `tundra` 指回 `ids.snow` |
| `气候带宽越界时校验拒绝` | `validate` 的新分支 | 删掉那个分支 |
| 既有 `atlas_coverage` 的全地形覆盖 | 两张新贴图真的存在 | 删掉 `assets/sprites/terrain_desert.png` |

---

## 六、黄金基准

`EXPECTED_WORLD_DIGEST`（`crates/ll-world/tests/determinism.rs`）
**必然改变**——地形判定的输入变了。走**四步重冻**，四步证据写进提交
信息：

1. 确认基线红（改动落地后跑，看它确实不等于旧值）；
2. **把 `climate_band_width` 关到 0，确认精确回到旧值**——这一步是真正
   的证据；本批次把它固化成一条长期存在的测试（见五节），不是一次性
   手工验证；
3. 恢复默认值；
4. 新常数在**两个独立进程**里复现。

`EXPECTED_REPLAY_DIGEST`（`crates/ll-sim/tests/replay.rs`）大概率不变
（那条回放用合成世界、不走地形生成），但**必须实跑确认**，不得推测。

`content_hash_of("lostland")` 会变——新增两条地形条目，那是它该变的。
**`CONTENT_HASH_ALGORITHM_VERSION` 不动**：`write_terrain_fields` 的
字段集合一个字未改，动版本号才是错的。

---

## 七、范围边界（不要越界）

- 不碰 `crates/ll-ui/src/hud/`、不碰 `crates/ll-sim/src/`（`wt-playfeel` 在改）；
- 不碰 `BaseRaceIds`（交接四节第 20 条那个「名不副实」是种族那一侧的
  问题，地形这一侧加字段时保持结构整洁即可）；
- 不改 `mods/lostland/cultures.json5`；
- 不 push、不合并 main。
