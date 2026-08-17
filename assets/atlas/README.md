# 占位图集

`placeholder.png` 与 `placeholder.json` 是**程序生成的临时占位资产**，
不是美术成品。

- `placeholder.png`：96×72 RGBA PNG，由 `ll-render` Task 5 提交时的一段
  一次性脚本（未随本次提交保留）生成，纯色块拼出四块内容：16×24 的
  「普通单位」、32×48 的「重点目标」、两块 16×16 地形。每个单位顶部
  留了一小块浅色标记，仅用来在测试截图里辨认朝向，不代表任何美术设计。
  Task 9 验收 demo 需要展示循环播放的行走动画，又用另一段一次性脚本
  （同样未随本次提交保留）在原图未使用的列 0、行 24-72 区域追加了两帧
  行走姿态（`hero_walk_0`/`hero_walk_1`，脚部标记左右交替），画布高度
  因此从 64 增至 72；原有四块内容的像素**未被改动**，仍在原坐标。
  P2 Task 8 验收 demo 需要把 [`ll_world::terrain::TerrainKind`] 的八种
  自然地形画出可区分的颜色，又用第三段一次性脚本（同样未随本次提交
  保留：生成后即删除，规矩见下方「地形色块」一节）把画布向右扩宽到
  96（原 64→96），在新增的 `x∈[64,96)` 区域追加七块 16×16 纯色地形；
  原有全部像素（含 `terrain_grass`/`terrain_dirt`）**未被改动**，仍在
  原坐标——扩图只在新画布上追加内容，不改写既有资产的既有像素，理由
  见下方「为什么不复用/不整体重排」一节。
- `placeholder.json`：与上图配套的 [`ll_render::atlas::AtlasMetadata`]
  元数据，条目名 `hero_idle_0`、`hero_walk_0`、`hero_walk_1`、
  `boss_idle_0`、`terrain_grass`、`terrain_dirt`，以及 P2 新增的
  `terrain_deep_water`/`terrain_shallow_water`/`terrain_sand`/
  `terrain_forest`/`terrain_hill`/`terrain_mountain`/`terrain_snow`，
  都是内部标识符，供渲染层单测、集成测试与 `ll-world` 的验收 demo 引用。

## 地形色块（P2 新增）

[`ll_world::terrain::TerrainKind`] 定义了八种自然地形（深水、浅水、沙、
草、林、丘、山、雪），`crates/ll-world/examples/p2_acceptance/` 需要把
它们画成能用肉眼区分的颜色。既有的 `terrain_grass`（草绿色，见上一节）
本身就适合直接复用给 `TerrainKind::GRASS`，因此没有新建一块重复的绿色
——新增的只是另外七种地形各一块 16×16 纯色：

| 条目名 | 对应 `TerrainKind` | 颜色（RGBA） |
|---|---|---|
| `terrain_deep_water` | `DEEP_WATER` | `(24, 52, 128, 255)` 深蓝 |
| `terrain_shallow_water` | `SHALLOW_WATER` | `(86, 172, 214, 255)` 浅蓝 |
| `terrain_sand` | `SAND` | `(214, 196, 140, 255)` 沙黄 |
| `terrain_grass`（复用既有条目） | `GRASS` | `(86, 125, 70, 255)` 草绿 |
| `terrain_forest` | `FOREST` | `(32, 96, 40, 255)` 深绿 |
| `terrain_hill` | `HILL` | `(150, 138, 74, 255)` 橄榄 |
| `terrain_mountain` | `MOUNTAIN` | `(128, 128, 132, 255)` 灰 |
| `terrain_snow` | `SNOW` | `(238, 240, 244, 255)` 近白 |

### 为什么不复用/不整体重排

新色块全部追加在扩宽出的画布右侧（`x∈[64,96)`），而不是复用图像里
其余的空白区域或整体重排版面：既有条目（`hero_*`/`boss_idle_0`/
`terrain_grass`/`terrain_dirt`）的矩形坐标已经写死在
`crates/ll-render/examples/p1_acceptance/` 与本 crate若干测试里，
任何挪动都会让那些坐标失效；只在画布上"新开一块地"追加，能保证
这次扩图对既有引用零影响。

## 待办

美术资产到位后：
1. 用真实图集替换 `placeholder.png` 与 `placeholder.json`。
2. 检查所有引用这些条目名的测试/代码是否需要同步改名。
3. 删除本文件，或更新为正式图集的说明。

在此之前，请勿删除或依赖这两个文件的具体像素内容——它们只保证
「结构合法、可解析」，不保证任何视觉效果。
