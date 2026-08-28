# 批次 5：世界地图的缩放、细节与玩家位置标记

**基线**：`c0706e1`　**分支**：`wt-worldmap`　**日期**：2026-08-27

来源：所有者原话两条——

> 「我希望地图不再是这么大的方块，而且存在更多的细节，例如直接对地图做一定的
> 缩放，这样打开地图就能更清晰地看清楚是什么东西。关于地图里，应该有个标识，
> 标记玩家正在哪个位置。」

> 「重生点就是随机点一个格子，然后在那区块内随机出生在陆地上。那个地图应该显示
> 多点细节，好让玩家决定选哪里。」

规格 §15 的 P7（UI 层）行。本批同时是后续「开局在地图上选重生点」那批的硬前置。

**基线测试数**：见第九节（本分支实跑）。

---

## 一、动手前逐条 grep 核实过的既有事实

| 事实 | 证据 |
|---|---|
| 世界默认 64×48 个区块、区块边长 48 → 3072×2304 格 | `crates/ll-game/src/world.rs:41`（`ZONE_SPAN`）、`:45`（`ZONE_COUNT`） |
| 世界地图 HUD 现在按 `WORLD_MAP_DOWNSAMPLE = 2` 出图，即 32×24 = 768 格 | `crates/ll-game/src/app.rs:127` |
| 一格因此覆盖 96×96 格，且**只采一个点**（区块左上角），不是归并 | `crates/ll-world/src/generate.rs:357`（`zone_representative_terrain`）、`crates/ll-world/src/overview.rs` 的 `continent_map` |
| `OverviewCell` 只有 `terrain`/`explored` 两个字段，**没有任何「玩家在哪」的概念** | `crates/ll-world/src/overview.rs:34-41` |
| `minimap`/`continent_map` 两个函数签名里也没有玩家 | 同上，两者的参数表 |
| `ContinentField` 是世界创建时算一次的粗粒度场，**区块分辨率**，不触碰 `SurfaceStore` | `overview.rs` 的 `ContinentField` 文档与 `continent_map不触发任何区块的按需生成` 测试 |
| `terrain_at_tile(noise, params, pos, ids)` 是**公开的纯函数**，任意瓦片坐标都能问，不需要区块常驻 | `crates/ll-world/src/generate.rs:339` |
| `zone_span` 恒是 `CELL_SIZE`（16）的整数倍 | `crates/ll-world/src/zone.rs:101-104`（`ZoneLayout::new` 的对齐校验） |
| `TerrainKind` 已派生 `Ord`，其文档明说「派生它是 C5 的正向选择」 | `crates/ll-world/src/terrain.rs:87-90` |
| 世界地图颜色映射**已经**有未知地形回退（洋红），不 panic、不漏画 | `crates/ll-ui/src/hud/world_map.rs:48`（`UNKNOWN_TERRAIN_COLOR`）与 `terrain_color` 的 `else` 分支 |
| `GameKey` 里**已经**有 `ZoomIn`/`ZoomOut`（参与自动重复，也接滚轮）与四个方向键 | `crates/ll-platform/src/input.rs:59-64`、`:36-43` |
| 据点表可以从 `world.terrain.chronicle_handle()` 拿到，`sites()` 按**区块光栅序**排好 | `crates/ll-world/src/surface_store.rs:320`、`crates/ll-world/src/chronicle.rs:427` |
| `SettlementSite` 有 `zone`/`anchor`/`status`/`population` | `crates/ll-world/src/settlement.rs:203-223` |

## 二、与并行批次的边界

- **`wt-climate` 正在给 `crates/ll-world/` 加新地形种类与贴图。** 本批**只读**
  `TerrainKind`/`BaseTerrainIds`，一个字段都不加不改。呈现层的地形着色**不写死
  八项 match**：见第六节。
- **`wt-menusettings` 正在改 `crates/ll-game/src/app.rs`、`crates/ll-platform/`、
  `crates/ll-ui/src/widget/`。** 本批**不新增任何 `GameKey`**（复用既有
  `ZoomIn`/`ZoomOut` 与四个方向键，见 4.1），因此 `crates/ll-platform/` 一行
  不动、`crates/ll-ui/src/widget/` 一行不动；`app.rs` 的改动压到「一个新字段 +
  一段输入分流 + 装配这一帧的地图数据」三处。

## 二·五、新门禁：验收 demo 会被真的跑起来

`scripts/ci/run_acceptance_demos.sh`（已接进 `run_all.sh` 第 60 步）带完整性
检查：工作区里每一个 example target 都必须显式出现在 `RUN_LIST` 或 `SKIP_LIST`
里。**本批不新建任何 example**——地图的可测部分（档位换算、屏幕像素 → zone、
环面平移绕回、归并规则与平局破法、玩家标记在接缝附近的位置）全都能用普通
`#[test]` 与属性测试覆盖，见第五节。

---

## 三、关键设计问题的裁定

### 3.1 缩放档位：离散，且以「整数」为单位

**离散。** 理由是 ADR 0002 的整数纪律：连续缩放意味着「一个格子覆盖多少瓦片」
是个浮点数，格边界会落在瓦片中间，同一片地形在相邻两帧被归并进不同的格子里，
画面会抖；而且「屏幕像素 → zone」的反算会带上舍入漂移，而那正是本批要交付给
「选重生点」那批的东西（第四节 C）。

档位表 `ZOOM_LADDER = [8, 4, 2, 1]`，单位是「**一个地图格覆盖多少个场采样**」：

| 档位 | 场采样/格 | 瓦片/格（默认区块边长 48） | 视野覆盖 |
|---|---|---|---|
| 0（最远） | 8 | 96 | 整个世界（32×24 格），与今天完全同一个尺寸 |
| 1 | 4 | 48（正好一个区块） | 世界的 1/2 × 1/2 |
| 2 | 2 | 24 | 世界的 1/4 × 1/4 |
| 3（最近） | 1 | 12 | 世界的 1/8 × 1/8（8×6 个区块） |

**视野格数恒定**（`view_cols = 场宽 / ZOOM_LADDER[0]` 向上取整，行同理），
不随档位变——面板布局与格子像素尺寸因此不抖，缩放只改「一格代表多大一片地」。
全程整数除法，无浮点参与。

### 3.2 「更多细节」从哪来：`ContinentField` 加密到子区块分辨率

今天的 `ContinentField` 一个区块只存一个采样点，**最细也只能到「一格 = 一个
区块 = 48×48 瓦片」**——再怎么缩放也变不出细节。因此把它加密：

- 新常量 `SAMPLES_PER_ZONE_AXIS = 4`，即每个区块存 4×4 = 16 个采样点，
  一个采样点覆盖 `zone_span / 4 = 12` 个瓦片（默认布局）。
- `zone_span` 恒是 16 的倍数（第一节已核实），因此 `% 4 == 0` 恒成立。
- 默认世界的场从 64×48 = 3072 个采样点涨到 256×192 = 49152 个
  （`TerrainKind` 是 `ContentIndex(u32)`，约 192 KB），生成成本从 3072 次噪声
  采样涨到 49152 次——**建局/读档各一次**，不在每帧路径上（`ContinentField`
  既有的「调用方应在世界创建时调用一次并长期持有结果」惯例不变）。
- **既有 `continent_map` 的行为逐位不变**：`ContinentField::terrain_at_zone`
  改为读区块左上角那一个子采样点，而子采样 `(zx*4, zy*4)` 对应的瓦片坐标恰是
  `(zx*48, zy*48)`——就是 `zone_representative_terrain` 采的那一点。因此
  p2/p5 验收 demo 与视觉基准不受影响。这条由一条新测试锁住。

### 3.3 归并规则：占比最高者，平局取 `TerrainKind` 最小者

一个地图格覆盖 `SPC × SPC` 个场采样（最多 8×8 = 64 个）。显示哪一种？

**取占比最高的那一种**（众数）——所有者要的是「最能代表这一片是什么」。

实现**不碰任何哈希容器**（C5）：把这 ≤64 个采样收进一个栈上的小 `Vec`，
按 `TerrainKind` 排序（它已派生 `Ord`，其文档写明「派生它是 C5 的正向选择」），
再线性扫最长的等值游程。

**平局破法：取 `TerrainKind` 排序序最小的那一个。** 扫描从已排序序列的头部
开始、只在**严格大于**当前最长游程时才替换，因此先出现（`Ord` 更小）的那一种
自然胜出。这条规则是纯函数、与容器迭代顺序无关、跨进程跨平台恒同。

> **规格没裁定，本批临时选定**：`TerrainKind` 的 `Ord` 来自 `ContentIndex`，
> 而索引由**加载顺序**决定——平局时显示哪种地形因此取决于 mod 加载顺序。
> 这不影响任何世界状态（纯呈现），也不影响确定性重放（同一份内容集下恒同），
> 但它确实意味着「换一组 mod，某几格的颜色可能变」。备选是按地形属性排（例如
> 优先显示不可通行的），那要给这条纯查询再加一个 `TerrainTable` 参数，且
> 「哪种属性更该被看见」本身要所有者裁定。选了最保守、最容易反转的一条。

### 3.4 平移：在场采样坐标上走 `TorusSize`，不手写取模

视野**中心**是一个场采样坐标 `TorusPos`，环面尺寸是
`TorusSize::new(场宽, 场高)`。平移一格 = 中心移动 `SPC` 个采样，越界由
`TorusSize::wrap` 处理（`crates/ll-core/src/torus.rs:148`）——**全程零手写
取模**，符合 `docs/architecture/04-torus-topology.md` 与仓库既有门禁的同一条
纪律。视野左上角同样由 `wrap(center - 半个视野)` 得出。

由此「场是环面的」这件事被完整继承：视野跨接缝时右半屏画的是世界另一头，
中间不会有断裂，也不需要画多份偏移拷贝（同 `Camera::world_to_screen` 的既有
取舍）。

### 3.5 据点/资源点显不显示

**显示据点，不显示资源点。**

- **据点拿得到，代价极低**：`world.terrain.chronicle_handle()` 给出
  `Option<Arc<WorldChronicle>>`，`sites()` 是一个**已按区块光栅序排好的切片**
  （`chronicle.rs:427`），默认世界二百多座。每帧遍历一次两百多项做坐标换算，
  与已经在跑的 768 格归并相比可以忽略；顺序来自切片本身，不碰哈希容器（C5）。
- **资源点拿不到（如实说，不硬接）**：`SettlementSite` 只存了这座据点**靠什么
  吃饭**的两种资源（`primary_resources`），那是据点的属性，不是「地图上哪里有
  一处资源」。世界里真正的资源点分布没有一份可供概览查询的索引，要现算就得
  逐区块跑资源采样——那正是 `ContinentField` 存在的理由所要避免的开销。本批
  **不做**资源点显示。
- **战争迷雾一致性**：据点标记与地形格走**同一条过滤规则**——所在格未探索就
  不画标记。否则一开局就能看见全世界所有村庄，与本模块「没去过的地方就黑着」
  这条既有硬规矩直接冲突。

  > **给「选重生点」那批的接口说明**：`continent_map` 与新的切片函数都要求
  > 调用方**显式传入** `&ExplorationMemory`（`exploration.rs` 模块文档
  > 「为什么读取接口要求显式传入」）。选重生点的界面因此只要传一份「全部
  > 已探索」的记忆进去，同一份代码就变成「全图可见」，**不需要给呈现层加
  > 任何 `reveal_all` 旗标**。这是本批刻意不加那个旗标的理由（YAGNI）。

---

## 四、落地形状

### A. 玩家位置标记（第一个提交）

**不进 `ll-world` 的世界状态**，也不进 `OverviewCell`——玩家在哪是呈现，
不是世界事实，而 `overview.rs` 的两个查询函数保持纯。

落点：`WorldMapPanelData` 新增 `player: Option<(u32, u32)>`（视野内的
列/行下标），由 `ll-game::app` 在装配这一帧数据时算出；`world_map.rs` 在
所有地形格之后追加一个**内缩的标记方块**（`PLAYER_MARKER_COLOR`）。

- **环面正确**：从玩家瓦片坐标算到视野列行，走的是「瓦片 → 场采样 →
  与视野左上角求 `TorusSize::delta`」这条链，接缝由 `delta` 处理。
- **任何档位都对得上**：列行是 `delta / SPC` 的整数除法，与地形格用的是
  **同一个**换算（同一个函数），不可能各算各的而错位。
- 玩家不在视野内时是 `None`，不画。

### B. 缩放与更多细节（第二个提交）

- `ll-world/src/overview.rs`：`ContinentField` 加密（3.2）、新增
  `WorldMapView`（档位 + 中心）与 `world_map_slice`（归并出一屏格子，3.3）。
- `ll-ui/src/hud/world_map.rs`：`WorldMapPanelData` 接住新的切片元数据；
  据点标记；比例尺文案。
- `ll-game/src/app.rs`：`Demo` 新增 `world_map_view` 字段；地图打开时
  `ZoomIn`/`ZoomOut` 改缩放档位、四个方向键改平移（见 4.1）；装配据点与
  玩家标记。
- 新文案（`en.ftl` 与 `zh-CN.ftl` **两个都加**）：比例尺与操作提示。

#### 4.1 地图打开时的输入分流（规格没裁定，本批临时选定）

地图打开时，四个方向键与 `ZoomIn`/`ZoomOut` **只作用于地图**，不再驱动玩家
移动与画面缩放。理由：地图是全屏浮层，玩家看着地图按方向键，期待的是移动地图
而不是让角色在看不见的地方走两步（后者还会推进世界时钟，不可撤销）。这是最
保守的一条——反转它只需要删掉分流的那个 `if`。

### C. 与「选重生点」的接口（第三个提交）

公开函数（`crates/ll-ui/src/hud/world_map.rs`）：

```rust
pub fn world_map_zone_at_pixel(
    data: &WorldMapPanelData<'_>,
    rect: Rect,
    pixel: (f32, f32),
) -> Option<ZoneCoord>
```

`rect` 就是 `world_map_frame` 收到的那个矩形；`pixel` 是屏幕像素。返回
`None` 表示这个像素落在网格之外（网格在 `rect` 内居中，四周有留白）。

**它是给谁用的**：下一批「开局在地图上选重生点」。那批要做的是「玩家在地图
上点一个 **zone**，然后在那个区块内随机挑一格陆地出生」，本函数就是「点了
哪里」到「哪个 zone」的唯一换算。本批**不做**选点交互本身。

**整数纪律**：像素 → 列行用浮点（呈现层允许），列行 → 场采样 → zone
**全程整数**，环绕走 `TorusSize::wrap`。

---

## 五、测试计划

1. `ContinentField` 加密后，`continent_map` 的输出与
   `zone_representative_terrain` 逐格相同（锁住 3.2 的「行为逐位不变」）。
2. 归并取众数，不是取左上角（构造一格里 3 个 A + 1 个 B，断言出 A）。
3. 归并平局取 `TerrainKind` 较小者（2 个 A + 2 个 B）。
4. 档位越深，同一块世界区域被切成越多格（「细节真的变多了」）。
5. 平移跨接缝时视野连续（左移一格后，新的第 0 列等于旧的第 −1 列）。
6. 玩家标记：在视野内时列行正确；跨接缝时不错位；不在视野内时为 `None`。
7. 据点标记在未探索格上不出现（战争迷雾不泄漏）。
8. **属性测试**（规格 §14.2 点名环面坐标要有属性测试）：任意档位 × 任意
   平移 × 任意像素，`world_map_zone_at_pixel` 的返回值要么是 `None`，要么
   落在 `[0, zone_count)` 的合法区间内；且横向相邻的两个采样像素给出的 zone
   在环面上 Chebyshev 距离 ≤ 1（接缝两侧连续）。
9. 每条新断言按 ADR 0018 做**反例验证**：故意改坏实现，确认它真的会红。

---

## 六、地形颜色映射如何容纳 `wt-climate` 即将加入的新地形

**这一条是本批与那批不撞车的关键**，单独成节。

`terrain_color`（`crates/ll-ui/src/hud/world_map.rs`）今天已经不是「八项
`match`」：`TerrainKind` 是注册期物化的 `ContentIndex`，数值由加载顺序决定，
编译期 `match` 字面量在这个仓库里**根本写不出来**（`ll_world::terrain` 模块
文档「从硬编码 match 到注册表」）。它是一串对 `BaseTerrainIds` 具名字段的
逐一比较，**末尾有 `else` 兜底**到 `UNKNOWN_TERRAIN_COLOR`。

本批**保持这个形状不变，一个分支都不加**：

- `wt-climate` 给 `BaseTerrainIds` 加沙漠/冻原等字段时，本批的代码**不需要
  改一行也不会编译失败**——`terrain_color` 从不穷尽匹配，新字段只是没被比较到。
- 新地形因此走 `else` 分支，落到 `UNKNOWN_TERRAIN_COLOR`：**不 panic、不漏画**，
  且那个洋红是刻意选来「让没配色这件事直接可见」的（该常量既有文档）。
- 补一条新地形的颜色只是**在那串 `else if` 里加一行**，那一行属于 `wt-climate`
  合并之后的收尾工作，不属于本批——本批如果现在就去猜它会叫什么名字，反而会
  和它撞车。
- 本批新增的东西（归并、缩放、标记）**全部只用 `TerrainKind` 的相等与 `Ord`**，
  从不问「这是什么地形」，因此地形种类变多对它们完全透明。

已有测试 `未注册的地形回退到显眼的未知色而不是复用某种自然地形色`
（`world_map.rs`）就是这条的守卫，本批不动它。

---

## 七、不做什么

- 不做选重生点的交互（下一批）。
- 不做资源点显示（3.5，拿不到，如实说）。
- 不新建任何 example（二·五节）。
- 不新增 `GameKey`、不动 `crates/ll-platform/`、不动 `crates/ll-ui/src/widget/`
  （避让 `wt-menusettings`）。
- 不动 `TerrainKind`/`BaseTerrainIds`（避让 `wt-climate`）。
- 不动黄金基准，除非实测变红（本批是呈现层，预期不变）。

## 八、提交切分

- 提交 A：玩家位置标记。
- 提交 B：缩放、归并、平移、据点标记、文案。
- 提交 C：`world_map_zone_at_pixel` 与它的属性测试。

## 九、实测数字与计划的偏差（落地后回填）

**测试数（本分支实跑，2026-08-27）**：

| | `bash scripts/ci/run_all.sh` | 测试目标 | 通过 |
|---|---|---|---|
| 改前基线（`c0706e1`） | exit 0 | 108 | 2435 |
| 改后（`c0e21fd`） | exit 0 | 108 | 2473 |

**黄金基准**：两条都实跑通过，常量一个字未改，且各在两个独立进程里复现
——`EXPECTED_WORLD_DIGEST` = `17_228_492_522_544_021_674`
（`crates/ll-world/tests/determinism.rs:252`）、`EXPECTED_REPLAY_DIGEST`
= `14_731_332_643_995_045_404`（`crates/ll-sim/tests/replay.rs:881`）。
视觉回归基准一张都没红，因此一张都没重冻。

### 与计划不同的四处

1. **玩家标记的换算函数换了落点。** 计划里提交 A 落的是
   `ll_ui::hud::world_map::zone_grid_cell_of_tile`（瓦片 → 区块网格）。
   提交 B 引入 `WorldMapSlice::cell_of_tile` 之后，两者回答同一个问题，
   留着就是第二份真相源，**提交 B 已把前者删掉**。
2. **`cell_of_tile` 第一版写错了环面换算，被测试抓住。** 用了
   `TorusSize::delta`（最短带符号偏移），但视野是从原点朝正方向铺开的
   窗口，要的是**正向**偏移；最远档位下视野右半边距原点超过半周，
   `delta` 一律返回负数，整块会被判成不在视野内而消失。改用
   `TorusSize::wrap(目标 − 原点)`。
3. **第五节第 5 条（平移跨接缝）的第一版测试咬不住钳制实现。** ADR 0018
   反例验证时把 `wrap` 换成 `max(0)`，`平移绕世界一整圈后逐位回到起点`
   照样全绿——它从原点出发，钳制下中心恒为 0，绕一圈「碰巧」也回到起点。
   补了 `从原点往西平移一格视野中心绕到世界东侧` 与
   `往西平移一格之后新的第一列等于旧的第零列` 两条堵洞。
4. **第五节第 8 条（属性测试）比计划多两条普通测试。** 反例验证发现
   「删掉按边框内缩」与「`cell_at_pixel` 改成钳制」两处破坏都不会让原有
   断言变红，补了 `网格之外的像素返回空而不是钳到最近的一格` 才堵上。

### 已知未达标：文件行数

`crates/ll-ui/src/hud/world_map.rs` 落地后约 1300 行，超过「800 行上限」
这条约定（其中约三分之二是 `#[cfg(test)] mod tests`，生产代码约 450 行）。
**仓库没有对应的门禁**，且既有文件里 `ll-sim/src/resolve.rs` 近 8000 行、
`ll-world/src/state.rs` 3386 行、`ll-game/src/app.rs` 2427 行，为本文件
单独拆分与仓库现状不一致。如实记录，留给后续统一处置。
