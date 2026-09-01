# 批次 28 实施计划：地形美术变化——同一种地形按位置取多张贴图

**日期**：2026-08-31
**工作树**：`wt-terrainart`（分支 `wt-terrainart`，基于 `origin/main` 的 `a9aa79a`）
**所有者报的现象**：地表「看起来太单调」——同一种地形整片铺同一张 16×16 贴图。
**前置**：[批次 27](2026-08-31-batch27-visual-baselines.md) 把三张纯 CPU 视觉基准
搬成带比对的测试，两张受影响的基准从此可以重新生成。**那份计划第八节
「地形美术变化那一批解锁了吗」给本批列了四条必做项**，逐条落在本文第六节。

**改前基线（本工作树自己跑的，纪律第 4 条）**：`bash scripts/ci/run_tests.sh`
→ **2954 通过 / 128 个测试二进制 / 0 失败**。

---

## 一、现状核实（开工前逐条 grep 过）

| 事实 | 判据 |
|---|---|
| `terrain_atlas_key` 今天的签名是 `(kind, ids, registry)`，不认识位置 | `crates/ll-game/src/layout.rs:151` |
| 生产渲染调用点只有一处 | `crates/ll-game/src/app/surface.rs:80` |
| 测试调用点两处 | `crates/ll-game/tests/atlas_coverage.rs:152`、`crates/ll-game/tests/visual_baselines.rs:200`/`:404` |
| `atlas_coverage.rs` 的地形清单是**手写的 19 条**，且模块文档自己记着「手写易漏」的历史 | `all_base_terrains`，`crates/ll-game/tests/atlas_coverage.rs` |
| 每种地形今天恰好一张图，图集条目名由一张硬编码静态表给出 | `layout::terrain_entry_name`，19 条 `if` |
| 松散贴图由 `tools/ll-artgen` 烧，新增内容走 `loose_only_entries()` 平行清单 | `tools/ll-artgen/src/main.rs:231`，`LooseOnlyEntry` 文档 |
| 自然地形的画法是「主色 + 稀疏点缀」，点缀由 `hash_pixel(tile_seed, x, y)` 决定 | `tools/ll-artgen/src/terrain.rs` |
| `TorusPos` 的坐标恒已规范化，只能经 `TorusSize::wrap` 构造 | `crates/ll-core/src/torus.rs`，字段私有 |
| 仓库有现成的、跨平台逐位确定的整数哈希 | `ll_core::hashing::StateHasher`（FNV-1a，模块文档写明「跨平台跨版本恒定」） |
| 三条黄金基准的落点 | `crates/ll-world/tests/determinism.rs`、`crates/ll-sim/tests/replay.rs`、`crates/ll-game/tests/populated_determinism.rs` |
| 待改的四个文件都**不在**行数棘轮快照里，代码行 434/283/577/172，离 800 都远 | `python3 scripts/ci/check_file_size_budget.py` 的同一套「非空非注释行」口径 |

---

## 二、A：变体怎么选

### 判据：变体号 = Lemire 折算(FNV-1a(条目名 ‖ x ‖ y), 变体数)

```text
terrain_variant_at(kind, ids, pos) =
    let n = terrain_variant_count(kind, ids);            // 每种地形自己声明
    if n <= 1 { 0 } else {
        let h = FNV1a( len_prefixed(条目名) ‖ le64(pos.x) ‖ le64(pos.y) );
        ((h as u128 * n as u128) >> 64) as u32           // 取高位折算
    }
```

三个选择各自的理由：

1. **哈希算法取 `ll_core::hashing::StateHasher`（FNV-1a）**，不新写一个。
   它的模块文档明写「完全由整数运算构成、由规范唯一确定，因而跨平台跨版本
   恒定」——正是这里需要的性质。`ll-core` 零依赖，`ll-game` 已经依赖它
   （`layout.rs` 顶上就 `use ll_core::time::Tick`）。
2. **折算走 Lemire 乘法取高位，不用取余。** FNV-1a 的低位雪崩弱，
   对 3 取余直接吃低位，会在规则网格上留下肉眼可见的条纹。乘法取高位是
   仓库已有的写法（`ll_core::rng::DetRng::gen_range` 逐字同一条），没有理由
   在这里另发明一个。
3. **条目名进哈希。** 不进的话，`grass` 与 `forest` 在同一格会算出同一个
   变体号，相邻地形之间出现「变体图案对齐」的规则感——正好是本批要消除的
   那种单调。长度前缀是 `StateHasher::write_len_prefixed_bytes` 自己那条
   碰撞纪律。

### 确定性怎么保证

- **纯函数**：输入只有 `(地形种类, 位置)`，无状态、无时钟、无 RNG。同一格
  跑一万次同一个结果，跨帧、跨进程、跨平台都是。
- **不碰 `DetRng`**：这是渲染层，每帧要算满屏上千格。`DetRng` 是给世界状态
  用的事件流（C3），在渲染层取数等于让「这一帧画了几格」影响随机流——
  那会直接毁掉确定性重放。本批一次 `DetRng` 调用都不加。
- **不经任何哈希容器**（C5）：变体表是一串 `if`，与 `terrain_entry_name`
  同一个形状。

### 「不进世界状态」怎么保证，怎么实测

**结构上**：变体号是 `layout.rs` 里一个自由函数的返回值，**不写回任何结构体**。
`WorldState` 没有新字段，`WorldState::hash()` 没有新混入，存档 schema 不动
（`scripts/ci/check_save_schema_version.py` 会替我看着这条）。

**实测证据**（不是推理，落地报告里给数字）：

1. 三条黄金基准（世界摘要 / 回放摘要 / 有人世界摘要）**跑绿且常量一个字未改**；
2. `check_save_schema_version.py` 绿（存档主体形状没动）；
3. `git diff --stat` 里 `crates/ll-world/` 与 `crates/ll-content/` 零改动。

**如果任何一条黄金基准红了：停下来报告，不重冻。** 红就意味着渲染期的东西
漏进了世界状态，那是缺陷。

### 环面怎么处理

**不写一行取模。** 入参类型取 `ll_core::torus::TorusPos`——它的字段私有、
只能经 `TorusSize::wrap` 构造，不变式是「坐标恒被规范化到 `[0,width)` × `[0,height)`」。
也就是说**绕回这件事在类型边界上就已经做完了**：环面上同一个物理格子无论从
哪个方向走到，`TorusPos` 都是同一个值，因此变体号必然相同。

生产调用点（`app/surface.rs`）手上本来就是 `TorusPos`。据点平面图那条测试
手上是 `(col, row)`，用 `TorusSize::new(cols, rows).wrap(col, row)` 换算——
同样不手写取模（仓库有「禁止手写欧氏距离」的同类门禁
`scripts/ci/check_no_manual_euclidean_distance.sh`，同一条精神）。

### 签名怎么改

```rust
pub fn terrain_variant_count(kind, ids) -> u32;                       // 新增
pub fn terrain_variant_at(kind, ids, pos: TorusPos) -> u32;           // 新增
pub fn terrain_atlas_key_for_variant(kind, ids, registry, variant) -> Option<String>;  // 新增
pub fn terrain_atlas_key(kind, ids, registry, pos: TorusPos) -> Option<String>;        // 加一个入参
```

`terrain_atlas_key` 仍然**每格只分配一个 `String`**（与今天逐字相同的开销），
不返回 `Vec`——地形瓦片是全仓库最热的一处循环，每帧铺满整屏。
`terrain_atlas_key_for_variant` 是给门禁枚举用的，不在渲染路径上。

变体号 0 的条目名**恒等于今天那个名字**（`terrain_grass`），变体 `i>0` 是
`terrain_grass_alt{i}`。好处有二：既有 PNG 一个字节不用重画；把变体数全改回
1 就精确回到今天的行为（可反转性的具体形式）。

---

## 三、变体数声明在哪：三条路的代价

| 路 | 怎么做 | 代价 | 会不会动内容哈希 |
|---|---|---|---|
| **甲：内容侧声明** | `mods/lostland/terrain.json5` 每条地形加一个 `variants: 3` | 需要地形定义加字段，`ll-mod` 的内容 schema 跟着改，**`CONTENT_HASH_ALGORITHM_VERSION` 要升**；`check_field_consumers.py` 要求新字段有决策层消费者；地形表还要把这个字段一路带到 `ll-game`。**mod 作者能自己声明变体数**是唯一真收益，而今天没有任何 mod 需要它（YAGNI） | **会**。内容哈希一动，`populated_determinism.rs` 那条黄金基准跟着红——而并行批次 `wt-dialogue3` 正要重冻它，撞车 |
| **乙：资产侧按文件名发现** | 扫 `assets/sprites/` 里有几张 `terrain_grass_alt*.png` | 不动内容哈希。但 `terrain_atlas_key` 是纯函数、**手上没有图集也没有资产 VFS**；要发现就得把图集/清单塞进这个每帧上千次的热函数，或者另起一份缓存。更要命的是**它把「漏了一张图」从错误变成了正常**——少一张 alt 就自动少一个变体，恰好是 ADR 0022 点名的「覆盖退化」形状 | **不会** |
| **丙（选它）：引擎侧静态表** | `layout.rs` 里一个 `terrain_variant_count` 函数，与 `terrain_entry_name` 那 19 条 `if` 并排 | 与既有惯例逐字同构（`terrain_entry_name` 本身就是「硬编码字面量、不经 Registry」）。mod 地形恒 1 张（回退路径不变）。声明侧与资产侧是两份清单，**会漂**——但漂了当场红：门禁两个方向都咬（见第五节） | **不会** |

**选丙。** 判据是任务书那句「选最保守可反转的那条」：

- **最保守**：不动内容 schema、不动内容哈希、不动存档、不动 `ll-world`/`ll-content`
  一个字节，因此三条黄金基准结构上不可能受影响。
- **最可反转**：`terrain_variant_count` 全改回 `1`，行为精确回到今天（变体 0
  的条目名就是今天的名字），五张 PNG 变成孤儿——而孤儿会被门禁抓到，不会
  静默留着。
- **乙那条的致命伤**是它与本批必做项 ③ 直接冲突：按文件名发现意味着「删掉
  一张变体图，变体数自动减一，全绿」，正是任务书点名要验的那条反例必须
  变红的场景。

甲那条留给「mod 要自带多变体地形」真的出现的那天；那时的落点写在
`terrain_variant_count` 的函数文档里。

---

## 四、B：画哪些变体、怎么画

### 范围：三种地形，合计 8 张（其中 3 张是既有图），新增 5 个 PNG

| 地形 | 变体数 | 新增文件 | 为什么是它 |
|---|---|---|---|
| `grass` | 3 | `terrain_grass_alt1/2.png` | 温带陆地的绝对主色，玩家眼前铺得最满 |
| `forest` | 3 | `terrain_forest_alt1/2.png` | 第二多，且成片连块，单调感最强 |
| `sand` | 2 | `terrain_sand_alt1.png` | 海岸线细长条带，两张就够打散规则感；**刻意与草/林不同数**，把「每种地形变体数可以不同」这条真的走一遍，而不是留成纸面能力 |

其余 16 种恒 1 张，即今天的行为。**宁可少而扎实**：三种地形做对做透，好过
十七种各两张但没人验过。石墙/木墙这类建筑地形刻意不做——它们靠**结构图案**
（门板、窗棂、砖缝）表达自己是什么，变体很容易跌到「看起来是另一种地形」，
留给后续批次单独论证。

### 画法：扩展 `tools/ll-artgen`，不手工画

`terrain.rs` 新增 `decorate_terrain_variant(image, rect, spec, variant)`：

- **变体 0** 逐字调用今天的 `decorate_terrain_tile`。既有三张 PNG 因此
  **一个字节不变**，git 里不会出现无意义的二进制改动。
- **变体大于 0** 在同一份 `TerrainSpec`（同一个主色、同一套邻近色/互补色换算）
  上做两件事：
  1. **点缀重新播种**——`tile_seed` 混入变体号，稀疏点缀的位置整片换掉；
  2. **叠一个几何图案**——变体 1 是「丛簇」（两团实心块），变体 2 是
     「碎屑」（一条斜向细纹加三粒散点）。图案用字符画常量声明，与
     `visual_baselines.rs` 的 `FLOOR_PLAN` 同一种可读写法。

**为什么必须动几何**：五族那批的教训是「只换配色会跌破可分辨门槛」。这里
方向相反但同源——只重播种点缀，两张图差的全是 5% 的散点，缩到 16×16
读起来就是同一张。图案是那 5% 之外的、**成块**的差异。

**风格一致是构造上保证的**：变体与基准共用同一个 `TerrainSpec`（主色不改）、
同一套 `Hsl::rotated/lighten/saturate` 换算、同一个 `hash_pixel`。没有任何
一处新配色是手挑的。

### 可分辨性判据（四条，落地报告给实测数字）

| # | 方向 | 判据 | 门槛 |
|---|---|---|---|
| ① | **变体之间真的看得出不同** | 同一种地形任意两张变体逐像素比，不同像素数 | 至少 1/8（256 像素里 32 个） |
| ② | **一眼还是那种地形（近邻仍是自己）** | 每张变体的平均 RGB 与全部 19 种地形基准图的平均 RGB 比最近邻 | 最近的必须是它自己那种地形 |
| ③ | **一眼还是那种地形（不许慢性漂移）** | 每张变体的平均 RGB 与它自己基准图的平均 RGB，每通道差 | 不超过 24 |
| ④ | **跨地形仍然两两分得开** | 既有那条「两两至少四分之一像素不同」扩展到**全部变体乘全部变体**（同种地形的变体之间除外） | 至少 1/4，与今天同一个门槛 |

①③ 是一对上下界：下界防「改了跟没改一样」，上界防「改到读成另一种地形」。
② 是把「一眼还是那种地形」写成可执行的形式，比单纯的通道阈值更贴近那句话。

另外 `十九种本体地形的贴图都铺满整格` 那条自动扩展到每一张变体（地形是
底层，留透明会露出清屏背景）。

---

## 五、C：门禁怎么按变体逐张咬住（本批最容易出事的地方）

必做项 ③ 的原话是「否则退化成『每种地形至少一个变体有图』」。
`atlas_coverage.rs` 的模块文档记着它自己的历史：**它当初就是因为用手写的
地形清单而漏掉新地形才被重写的**。不要让它第二次退化。

### 三道锁，缺一条就会退化

1. **枚举来自生产代码，按变体展开。**
   清单是 `(0..terrain_variant_count(kind, ids))` 逐个调
   `terrain_atlas_key_for_variant`——**不是测试里另抄一份变体名**。每一张
   变体各得一条断言，删掉 `terrain_grass_alt1.png` 会红，而**不会**因为
   `terrain_grass` 还在就绿。

2. **反向锁：图集里不许有声明侧数不出来的变体图。**
   扫真实图集里全部名字带变体后缀的条目，逐个要求它出现在第 1 条那份清单里。
   这是第三节「丙那条会漂」的解药：

   - 声明减了、PNG 留着，反向锁红；
   - PNG 少了、声明留着，正向锁红。

   **两个方向都红，才叫咬住。**

3. **地形清单本身不再手抄。**
   `all_base_terrains` 那张手写的 19 行表是模块文档自己点名的债
   （「日后若嫌手写易漏，正确的改法是让本函数从 `BaseTerrainIds` 的字段
   穷尽解构里推导，不是继续手抄」）。本批**顺手还掉**：改成对
   `BaseTerrainIds` 做**穷尽解构**（不许写 `..`），加一个字段编译不过。
   这不是顺手扩范围——变体覆盖的分母就是这张表，分母漏一行，逐张覆盖照样
   退化。

### 两张基准同批重冻

`surface_preview.png`（地表内容，含地形场）与 `settlement_preview.png`
（据点平面图，草地那一圈会取到变体）会变。按 README 的处置规矩：
**先逐条判断每一处差异对应什么**，确认全部是本批有意的视觉调整之后，
才 `LL_BLESS_VISUAL=1`。`npc_roster_preview.png` 不画地形，**应当零差异**——
如果它也变了，说明改动漏到了不该漏的地方，停下来查。

### README 原地更正（必做项 ②，纪律第 9 条）

`crates/ll-game/tests/visual/README.md` 据点那一节的
「**手工摆的只有『哪一格是哪种地形』这一件事**」从本批起失效——坐标
从此参与选图。原文按纪律第 9 条**一个字不改**，原地追加更正段并指回本文。
同一节「自动化的那一半」写的「全部 17 种本体地形」也一并原地更正
（实际早已是 19 种，且本批起是「按变体逐张」）。

---

## 六、上一批点名的四条必做项，本文哪一节回应

| # | 必做项 | 落在 |
|---|---|---|
| ① | `terrain_atlas_key` 加位置/变体号入参，两条测试各改一行调用点 | 二节末「签名怎么改」 |
| ② | `visual/README.md` 那句「手工摆的只有哪一格是哪种地形」原地更正 | 五节末 |
| ③ | `atlas_coverage.rs` 按变体逐张覆盖 | 五节「三道锁」 |
| ④ | 两张视觉基准同批重冻并逐条说明差异 | 五节「两张基准同批重冻」 |

---

## 七、反例验证（ADR 0022，硬要求）

**注意编号**：讲反例验证的是
[ADR 0022](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)，
**不是 ADR 0018**——仓库刚扫除过一批把它误写成 0018 的引用，本批不跟着写错。

任务点名必验的三条，外加各断言自己的机制反例：

| # | 改坏什么 | 期望变红的 | 红的理由必须是 |
|---|---|---|---|
| ① | 同一格连跑两次取变体 | —— | 恒相等（这条是**正向**实测，写成断言） |
| ② | 把 `assets/sprites/terrain_grass_alt1.png` 移走 | 按变体覆盖 | 「**这一张**变体查不到条目」，**不是**「grass 一张图都没有」 |
| ②b | 把 `terrain_grass_alt1.png` 换成全透明 | 铺满整格 | 「有 256 个像素不是完全不透明」 |
| ②c | 把 `terrain_variant_count` 里 grass 那一支改回 1（PNG 留着） | 反向锁 | 「图集里有声明侧数不出来的变体图 `terrain_grass_alt1`」 |
| ③ | 遍历一批不同的格子 | —— | 至少取到过两个不同的变体号（否则等于没做） |
| ④ | 变体图案改成照抄基准（只重播种点缀） | 变体可分辨 | 「不同像素数低于 1/8 门槛」 |
| ⑤ | 变体主色改成另一种地形的主色 | 近邻仍是自己 | 「最近的是别的地形而不是它自己」 |
| ⑥ | `terrain_variant_at` 里去掉条目名那一段混入 | ——（观察项） | 若无断言变红，如实记着「这条性质没被咬住」 |

**每一条都要确认红的原因是我以为的那个**，不能只看到红就算过。
**写断言之前先跑基线**——本会话有一条断言在主干未改动时就已经是红的。

---

## 八、门禁与提交

- 改前基线：**2954 通过 / 128 个二进制**（本工作树自己跑的，见文首）。
- 提交前 `bash scripts/ci/run_all.sh` 必须 exit 0。
- 行数棘轮：四个待改文件都不在快照里，代码行 434/283/577/172，改完仍应远
  低于 800。**不预期需要 `--bless`**；真需要就写实在理由（留空即红）。
- 不新增 example target（ADR 0030）。
- 不硬编码用户可见字符串；本批不新增 `.ftl` 键。
- PNG 体积：新增 5 张 16×16，落地报告给增量字节数。
- A / B / C 分三个提交，中文提交信息。**不 push、不合并 main。**

## 九、不碰什么

`LostLand`（main 工作树）与 `wt-dialogue3`（并行批次，在改
`crates/ll-sim/src/dialogue.rs`、`ll-world` 的 `Agent`、存档 schema、
`crates/ll-game/src/dialogue_screen.rs`，并会重冻
`crates/ll-game/tests/populated_determinism.rs`）。本批**只读**
`populated_determinism.rs`（跑它、不改它），落点与 `wt-dialogue3` 零交集。

---

## 十、落地记录

（落地后回填。）

## 十一、规格没裁定、本批临时选的做法

（落地后回填。）
