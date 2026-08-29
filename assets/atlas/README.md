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

### 气候条带新增的两块（2026-08-27）

规格 §7.1 的气候条带落地后，本体自然地形从八种变成十种。两块新色块
**只进松散贴图树**（`assets/sprites/terrain_desert.png` /
`terrain_tundra.png` + `assets/sprites/manifest.json5`），**不进**本目录
这张遗留共享画布，也不进 `placeholder.json`——那张画布是五个更早批次
验收 demo 的冻结像素基准，往里塞新内容只会把它们卷进来（理由见
`tools/ll-artgen/src/main.rs` 的 `LooseOnlyEntry` 文档）。

| 条目名 | 对应地形 | 颜色（RGBA） |
|---|---|---|
| `terrain_desert` | `lostland:desert` | `(198, 154, 86, 255)` 深橙沙 |
| `terrain_tundra` | `lostland:tundra` | `(196, 206, 208, 255)` 灰青白 |

两块的颜色都刻意与它们最容易被混淆的那一块拉开距离：沙漠比海滩
`terrain_sand`(214,196,140) 更深更橙，冻原比高山 `terrain_snow`(238,240,244)
更暗更青。`crates/ll-game/tests/atlas_coverage.rs` 的
`十九种本体地形的贴图两两之间至少四分之一像素不同` 把这条钉死。

### 为什么不复用/不整体重排

新色块全部追加在扩宽出的画布右侧（`x∈[64,96)`），而不是复用图像里
其余的空白区域或整体重排版面：既有条目（`hero_*`/`boss_idle_0`/
`terrain_grass`/`terrain_dirt`）的矩形坐标已经写死在
`crates/ll-render/examples/p1_acceptance/` 与本 crate若干测试里，
任何挪动都会让那些坐标失效；只在画布上"新开一块地"追加，能保证
这次扩图对既有引用零影响。

## 像素点缀与角色标志（占位美术改进）

占位色块「测试时一片同色、人物分不清谁是谁」，因此在不改动
`placeholder.json` 布局（全部 `rect`/`pivot`/`footprint` 保持不变）的
前提下，给 `placeholder.png` 的像素内容加了两类改动，由新工具
`tools/ll-artgen`（见其 crate 文档）生成，**不是手工画的**：

1. **地形点缀**：每块 16×16 地形贴图上，约 5%（13/256）像素被替换成
   邻近色（色相 ±18°，制造深浅层次）或互补色（色相 +180°，稀疏几个
   点，作强对比标记）；主色像素占比保持在 91%~97%（各地形实测值不同，
   详见 `tools/ll-artgen/src/terrain.rs` 单测），上表列出的主色数值
   **全部未变**。点缀像素的位置由地块在图集里的像素坐标当种子算出，
   确定性生成、可重新生成，不是随机噪点。
2. **角色标志**：`boss_idle_0` 在原有面甲标记内新增两个暗色眼部像素，
   并在胸口新增青色菱形警示标志——青色是红色主体的互补色，刻意不与
   玩家同款的暖金色标志混淆，让玩家与 boss 除了主体色红/蓝之别外，
   标志的形状与颜色也不同。boss 的主体色（暗红 `(180,40,40)`）与既有
   面甲位置**未变**。
3. **玩家贴图重画**（所有者：「目前的贴图有点丑了」）：八张 `hero_*`
   此前是**一整块 16×24 的实心钢蓝矩形**，上面压一个金色头部方块与一个
   金十字——没有轮廓、没有明暗、四角全部不透明，摆在四周透明的 NPC
   人形旁边就是一块砖。重画之后是一个有轮廓的人形：金盔 + 三档明暗的
   钢蓝上衣 + 金腰带 + 胸口纹章 + 双腿双靴，外加一圈近黑描边；脚底行
   号取 21，**与 `tools/ll-artgen/src/npc.rs` 的 `FEET_TOP` 一致**，
   玩家与 NPC 站在相邻两格里踩在同一条地平线上。
   主体钢蓝 `(70,130,180)` 与暖金 `(255,220,120)` **一个字没改**：
   「玩家=蓝、boss=红」这条视觉约定跑通过全部验收 demo。
   重画的是同八张，**没有新增第二套**（文件名与动画语义未动）。
   四条可核实的改进各有一条断言盯着，见
   `tools/ll-artgen/src/sprite.rs` 模块文档那张表。

## 世界内容记号（地面物品堆 / 家具 / NPC）

此前 `render_surface` 只画地形与玩家；地面物品、放置家具、NPC 三类世界
内容在引擎里都存在，屏幕上一个都看不见。补上的四张图由
`tools/ll-artgen/src/world_marks.rs` 生成：

| 条目名 | 尺寸 | 用途 | 主色 |
| --- | --- | --- | --- |
| `ground_pile` | 16×16 | 地面物品堆的「团」，**恒定这一张** | 琥珀橙 `(224,160,60)` |
| `furniture_placed` | 16×16 | 放置家具查不到自带贴图时的通用记号 | 紫罗兰 `(130,120,180)` |
| `npc_idle_0` | 16×24 | NPC 查不到种族自带贴图时的通用记号 | 紫红 `(150,110,160)` |
| `forge` | 16×16 | **内容自带**贴图的样例（`lostland:forge`） | 石灰 `(96,90,86)` + 火色 |

四张图的矩形全部落在既有布局的空白处（`(48,32)`、`(16,48)`、
`(32,48)`、`(48,48)`），画布尺寸仍是 96×112，既有条目的 `rect` 一个都
没动——理由同上一节「为什么不复用/不整体重排」。

三张通用记号与 `forge` 的地位不同：前三张是**引擎的兜底**，后一张只是
本体内容顺手带的一张图。把 `assets/sprites/forge.png` 删掉，锻炉会自动
退回 `furniture_placed` 那张通用记号，引擎一行都不用改——渲染层拿的是
内容的完整命名空间 ID 当图集键（见 `ll_mod::asset_vfs::ResolvedSprite::atlas_name`
与 `ll_game::surface_draw` 模块文档），**没有任何一处按物品 id 分支**。
mod 想给自己的家具/种族一张专属贴图，同样只要在自己的
`assets/sprites/` 里放一张与本地名同名的图。

与地形色块不同，这四张图**不铺满不透明底色**：它们画在地形之上，四周
留透明让下面那格地形透出来。

调参数、加新地形配方都在 `tools/ll-artgen/src/terrain.rs` 的一张表里
改，改完重新跑 `cargo run -p ll-artgen` 即可重新生成整张图集，不需要
再手工画。

## 据点建筑地形（九张）

`ll_world::terrain::define_base` 注册 17 种本体地形，此前只有 10 种
查得到图集条目（8 种自然地形各一张，`floor_stone`/`wall_stone` 借用
`terrain_dirt`/`terrain_mountain`）。剩下 7 种建筑地形整个没有图——
玩家走进据点，控制台每帧刷「图集条目缺失，跳过本次绘制」，那些格子
一格都画不出来。补上的九张由 `tools/ll-artgen/src/building.rs` 生成：

| 条目名 | 尺寸 | 图案 | 主色 |
| --- | --- | --- | --- |
| `terrain_floor_wood` | 16×16 | 横向板缝木地板 | 暖褐 `(152,110,66)` |
| `terrain_wall_wood` | 16×16 | 竖向板缝木墙 + 顶端横梁 | 暗暖褐 `(104,68,38)` |
| `terrain_floor_stone` | 16×16 | 错缝铺面石 | 亮中性灰 `(166,166,172)` |
| `terrain_wall_stone` | 16×16 | 错缝砌石墙 + 亮墙帽 | 暗中性灰 `(100,100,108)` |
| `terrain_door_closed` | 16×16 | 深框 + 整块门板 + 两枚把手 | 橙褐 `(176,122,56)` |
| `terrain_door_open` | 16×16 | 深框 + 收到两侧的门板 + 门洞 | 近黑 `(46,38,32)` |
| `terrain_window` | 16×16 | 深框 + 亮玻璃 + 十字窗棂 | 浅青 `(168,214,228)` |
| `terrain_stairs_up` | 16×16 | 越往上越亮的四级阶梯 + 上行箭头 | 暖黄箭头 `(240,208,96)` |
| `terrain_stairs_down` | 16×16 | 越往上越暗的四级阶梯 + 下行箭头 | 冷蓝箭头 `(96,148,220)` |

配色守两条规则：**材质分色相**（木质暖褐、石质中性灰）、**墙暗地板亮**
（同材质下墙的明度明显低于地板）。两条正交，四种组合各占一个象限，
木墙/石墙/木地板/石地板因此两两之间至少差一个维度。门/窗/楼梯靠形状
而非颜色区分。

九张的矩形全部落在**新增的两整行**（`y∈[112,144)`），画布因此从
96×112 长到 96×144，**既有条目的 `rect` 一个都没动**——理由同「为什么
不复用/不整体重排」一节。画布长高不影响五个遗留 demo 的冻结截图基准：
UV 换算是 `(像素坐标 ± 半纹素) / 图片尺寸`，采样器固定
`FilterMode::Nearest`，分子分母同步变化，既有条目命中的纹素中心逐个
不变（本批次实测：旧的 96×112 区域逐像素零差异）。

与地形色块相同、与四张世界记号相反：这九张**铺满整格不留透明**——
它们是那一格的底层地形，留透明会露出清屏背景。

`floor_stone`/`wall_stone` 借用 `terrain_dirt`/`terrain_mountain` 的
关系已在生产渲染路径（`ll_game::layout::terrain_entry_name`）与
`crates/ll-sim/examples/p5_coordinate_acceptance` 两处**全部**解除——
后者一度还留着旧借用，所有者裁定统一（「第三条的话先统一了吧，避免
以后有什么问题」）。`terrain_dirt` **没有**因此变成孤儿图：
`crates/ll-render/examples/p1_acceptance` 拿它铺棋盘格、
`crates/ll-game/src/content.rs` 的 mod 资产覆盖验收拿它当被覆盖目标，
两处都还在用——那两处的 `terrain_dirt` 就是泥土本身，不是借用。

## NPC 的种族身子与职业挂件（十七张 + 示例 mod 两张）

此前 NPC 全部退回同一张 `npc_idle_0`——所有者报的现象是「所有 NPC 长得
一模一样」，裁定是「npc 根据职业种族做出区别，多画点」。

本体现有 4 个种族 × 13 个职业 = 52 种组合，所有者已说还要再加 5 个种族
（9 × 13 = 117）。**逐个组合备一张图不可接受**，因此这里画的是两套可以
叠起来的图，资产量从 `种族数 × 职业数` 降到 `种族数 + 职业数`：

| 这一层 | 张数 | 查的键 | 查不到时 | 画什么 |
| --- | --- | --- | --- | --- |
| 种族身子 | 4 | 种族的完整 ID（`lostland:dwarf`） | 退回 `npc_idle_0` | 体型/肤色/耳朵/胡子 |
| 职业挂件 | 13 | 职业的完整 ID（`lostland:blacksmith`） | **什么都不画** | 胸口一块 6×6 徽记 |

两张图都是 16×24、`pivot (8, 24)`、占地 1×1——与 `hero_*`/`npc_idle_0`
同一档，因此像素级对齐，渲染层不需要任何额外的偏移换算。叠的次序由
`ll_game::surface_draw` 的绘制序号号段保证（挂件的号恒大于身子的号）。

四个种族靠**三个正交维度**互相区分，不只靠颜色：身高（`head_top`）、
体型（`shoulder_w`）、耳朵形状。十三个职业靠底板色 + 徽记图形区分，
底板色两两不同，笔画色由底板沿明度轴现算（亮底板压暗笔画、暗底板提亮
笔画），因此**整块徽记 36 个像素两两全不同**是设计下界而不是运气。

`mods/example_mod/assets/sprites/` 另有 `half_elf.png` 与
`necromancer.png` 两张：示例 mod 自己声明的种族/职业，自己配的图。它们
存在的理由是**验收而不是美术**——`crates/` 下没有任何一处提到过这两个
id，它们照样画得出来，这就是「加第 10 个种族只加数据、不改 Rust」。

### 这十九张不进 `placeholder.png`

与前面每一节都不同：这批图**只**进松散贴图树（`assets/sprites/`），
不进遗留共享画布。`placeholder.png` 是五个更早批次验收 demo 的冻结像素
基准，而本体二进制早就不读它了；把只有运行期图集用得到的图塞进那张
画布，只会把画布撑大、把五个 demo 的基准卷进来。做法上的落点是
`tools/ll-artgen` 的 `LooseOnlyEntry`：一份与 `placeholder.json` 平行、
只喂给 `generate_loose_sprites` 的条目清单。**画布尺寸仍是 96×144，
`placeholder.json` 一个字没动，`placeholder.png` 逐字节未变。**

## `boss_idle_0` 的去留：留图，不再算「待接线」

上一批查出 `boss_idle_0` 在 `ll-game` 里零消费者——只有
`crates/ll-render/examples/p1_acceptance` 与
`crates/ll-sim/examples/p3_acceptance` 两个更早批次的验收 demo 在用，
本体二进制一处都没有。项目所有者的裁定是「现在应该不太需要 boss
这东西」。

**处置：图与 `placeholder.json` 条目原样保留，但它不再是一条「等着接
进游戏」的待办。** 理由是删它并不便宜而留它不花钱：

- p1/p3 两个 demo 各自 `include_bytes!` 这张共享画布，且 p3 的冻结截图
  基准 `crates/ll-sim/tests/visual/baseline/p3_acceptance.png` 里真的
  画着这只 boss（它还是唯一一个 2×2 占地的条目，p3 的
  `spawn.rs`/`ll_render::sprite` 靠它验「footprint 从图集条目读取」这条
  性质）。删条目要连带重冻 p3 的基准、改两个 demo 的资产装载，代价与
  收益完全不成比例。
- 留着它不占运行期成本：本体二进制根本不读这张共享画布（运行期图集由
  `pack_atlas` 从松散贴图现打），松散贴图那边的 `boss_idle_0.png` 也
  只是多一个没人查的图集条目。

换句话说：**它现在的身份是「两个验收 demo 的测试夹具」，不是「本体缺
的一块内容」。** 真到了做 boss 的那天，那时的美术不会是这张占位红块。
这条处置是本批次的判断，所有者只说了那一句「现在应该不太需要」。

## 待办

美术资产到位后：
1. 用真实图集替换 `placeholder.png` 与 `placeholder.json`。
2. 检查所有引用这些条目名的测试/代码是否需要同步改名。
3. 删除本文件与 `tools/ll-artgen`，或更新为正式图集的说明。

在此之前，请勿删除或依赖这两个文件的具体像素内容——它们只保证
「结构合法、可解析」，不保证任何视觉效果。
