# 据点、结构物与 NPC 生成

**冻结于** 2026-08-22。**基线提交** `6fa7eb8`（`main`）。

> **【2026-08-25 更新：本文档冻结于脚本时代，多处已过时】**
>
> 1. **脚本系统整个拆除了。** 全仓库零 `*.scm` 文件，内容改走 JSON5（`mods/lostland/*.json5`）。本文档里全部 `(register-... )` Scheme 片段、`ClassBehaviorBindings` 里那套「行为树入口函数名」、六节 6.5 关于「跨脚本边界 326ns」的性能论证，**都要按 JSON5 + Rust 重新表述**。结论（不新建 `StructureKind`、职业走 `ClassDef`、据点选址是纯派生、构成用 `DetRng::for_entity`）**全部仍然成立**，只有载体变了。
> 2. **`CONTENT_HASH_ALGORITHM_VERSION` 已经是 17**，不是八节写的 11。
> 3. **世界历史生成不再是「不存在」。** `crates/ll-world/src/chronicle.rs` 已落地一个最小可用的历史生成器（12 纪元 × 300 年，据点建立/遗弃两类事件），并且**真的把据点与废墟写进了地形**。七节「历史在后、派生在先」那个两步顺序因此已经被**跳过第一步直接做成了第二步的雏形**：据点不是「先纯派生、等历史落地再套偏差」，而是一开始就由历史推演产出。7.2 的接口形状（派生函数不变、外面套偏差层）对**将来的存档偏差**仍然成立。
> 4. **六节 6.4「选址必须与 `find_spawn_site` 共用连通域分析」已经照做**：那段算法提到了 `crates/ll-world/src/land.rs`（`largest_walkable_component`），`ll-game` 侧的私有副本已删除，出生点选址与据点选址各调一次、阈值不同。
> 5. **十一节⑥「`Effect::SpawnActor`」仍然不存在**（全仓库只有 `ll-sim/src/subclass.rs` 一句注释提到这个名字）。
> 6. 六节 6.3 的 `SettlementTemplateTable` **仍未落地**。**6.1 的职业→行为绑定与 NPC 生成本身已经落地**：NPC 生成走 `crates/ll-mod/src/roster.rs`（名册纯派生 + 按据点物化）+ `ll_game::world::materialize_nearby_settlements`；职业→行为绑定走 `crates/ll-mod/src/behavior_binding.rs`（`ClassBehaviorBindings`，形状确如 6.1 所述照抄 `XpCurveBindings`），内容侧入口是 `mods/lostland/classes.json5` 每条职业上的一个 `behavior` 字段（`townsfolk` / `sentry` / `beast` 三个**行为原型**，不是 6.1 写的「行为树入口函数名」——脚本没了，树是 Rust 写的封闭枚举）。
>
> 7. **资源已经从扁平四条改成「五大类 + 具体种类」两层**（项目所有者裁定：食物 / 木材 / 金属 / 石材 / 水）。本体七条具体种类：良田、牧场、木材、铁矿、花岗岩、水源、渔场。职业随之从十条增至十三条（新增渔夫 / 牧羊人 / 石匠）。据点名册的职业与种族亲和**按大类**挂规则（唯一的例外是牧场按具体种类，因为它与良田同属食物却要抬不同职业）。
>
> 8. **据点有了「建立者种族」**（`ll_mod::roster::settlement_founder_race`）：种族不再逐个居民独立抽，而是先按资源抽出这座据点是谁开的，其余居民以他为主 + 20% 外来者。
>
> 以下原文保留。

**落地状态**：纯设计。本文档描述的三个新接线点（`RecipeDef.station_becomes`、`ClassBehaviorBindings`、`SettlementTemplateTable`）与「据点生成器」本身**均未落地**，全部为本文档新提出。但本文档所依赖的**底座绝大部分已经落地**——这正是本设计能收得这么窄的原因，逐条核实见二节。

**并发声明**：核实时工作区存在未提交的杂项（`config.json5`、若干 `*.png`、`save.llsave`、`scripts/ci/__pycache__/`），与本文档无关；另有其他并行工作可能未提交。本文档的一切 grep 结论均以 `6fa7eb8` 的已提交树为准，读者若在更晚的提交上复核，请重跑而不是相信本文档的行号。

**范围**：项目所有者一条裁决同时点了四件事——基础结构物与可交互物、树的非方块表达、NPC 职业与生成、据点按历史生成。本文档给出四者互相咬合的**一条主线**，主线之外的分支在十一节逐条标为将来扩展并写明各自前置。

---

## 一、这份设计要回答的那个缺口

项目所有者原话：

> 「NPC生成你先补上设定，这东西需要根据历史生成，放在文明据点或者营地或者某种合理的地点，也就是说你还需要补入一些建筑，门，窗，墙壁，田地，床，抽屉，等等最基础的东西，包括树，树都变得可以交互，所以树的贴图不再是一个方块，而是带有图案的东西来表达。NPC会存在职业之类的东西，例如猎户，屠夫，农夫，据点管理者，民兵，守卫等等。。。。」

对应的已核实缺口，两条：

1. **真实游戏里一个 NPC 都不生成。** `crates/ll-game/src/world.rs:515` 的 `world.actors.spawn` 是整个 `ll-game` 唯一一处调用，生成的是玩家。其余 `actors.spawn` 调用点全部落在 `ll-content` 的验收 demo、`ll-content::remap`/`save_file` 的单元测试、`ll-mod` 的任务测试里——没有一处在生产装载路径上。
2. **没有「哪种生物用哪棵行为树」的内容绑定。** `ll_mod::script_behavior_source::ScriptBehaviorSource` 持有**一个** `tree_entry_fn: String`，一个实例只能跑一棵树；`mods/example_mod/behavior.scm` 里已经躺着两棵互不相干的树（`goblin-ai-tree`、`guard-ai-tree`），但没有任何数据能说出「卫兵用后者」。

`crates/ll-game/src/app.rs:71-92` 的 `no_npc_ai` 文档已经把这两条写成了显式的阻塞项，本文档就是来解这两条的。**第 1 条是本文档的终点，第 2 条是通往终点的必经之路**——先有内容能说出「生成谁、他用哪棵树」，再有一段代码去生成。

---

## 二、先核实：底座已经有多少

这一节是本设计的成本论证。**下表每一行都 grep 过**，不是从设计文档抄的——`world-history.md` 说「纯设计，尚无代码」，但 `crates/ll-world/src/history.rs` 有 251 行；`crates/ll-world/src/interior.rs` 有 577 行而 `coordinate-system-and-layers.md` 在索引里仍被列为「接口设想」。设计文档在这个仓库里已经过期过至少五次，一律以代码为准。

| 所有者点名的东西 | 现状 | 证据 |
|---|---|---|
| **墙壁** | **已存在**，两种：`wall_wood`/`wall_stone`，`blocks_move` + `blocks_sight` | `crates/ll-world/src/terrain.rs:418-421` |
| **门** | **已存在**，`door_closed`/`door_open`，走 `TerrainDef.opens_into` 声明式转换；`Intent::OpenDoor` → `resolve_open_door`（`resolve.rs:3005`）与撞门分支（`resolve.rs:2443`）两条产出路径，`Effect::SetTerrain` 由 `apply.rs:138` 消费 | `terrain.rs:422-425`、`resolve.rs:909/2443/3005`、`apply.rs:138` |
| **窗** | **已存在**，`window`：`blocks_move` 为真、`blocks_sight` 为假（刻意——可隔窗放箭/被看见） | `terrain.rs:426-427` |
| **建筑（内部）** | **已存在**：`Interior` + `InteriorTable`，`anchor: TorusPos` 为单一真相源，反向索引现算不缓存；`Intent::EnterSpace`/`ExitSpace` 已在 `Intent` 里 | `crates/ll-world/src/interior.rs`（577 行）、`crates/ll-sim/src/intent.rs:116/125` |
| **建筑（地板/楼梯）** | **已存在**：`floor_wood`/`floor_stone`/`stairs_up`/`stairs_down` | `terrain.rs:414-431` |
| **抽屉（容器）** | **机制已存在**：`GroundItemStack.contents` 非空即容器，`Intent::Loot` → `resolve_loot`（`resolve.rs:1795`）搜刮脚下第一个 `contents` 非空的地面物品。尸体就是走这条路 | `crates/ll-world/src/item.rs:153-186`、`resolve.rs:1795-1818` |
| **田地** | 不存在 | 无 `field`/`farmland` 地形 |
| **床** | 不存在。但 `Agent.resting: Option<RestState>` 与 `Intent::Rest`/`Effect::BeginRest`/`ClearResting` 已落地 | `agent.rs:226`、`effect.rs:461/479` |
| **树** | **半存在**：`forest` 地形（可通行、慢、树冠阻挡视线）已注册，`assets/sprites/terrain_forest.png` 已有贴图。但它就是所有者说的那个「方块」，且不可交互 | `terrain.rs:406-407`、`assets/sprites/manifest.json5:172` |
| **NPC 职业** | **路已通**：`Agent.profession: ContentIndex`，`ClassDef` + `register-class`，`lostland:guard` 已进 `mods/lostland/classes.json5`，行为树 `self-has-profession?` 认它 | `agent.rs:124`、`mods/lostland/classes.json5:44` |
| **NPC 物品与掉落** | **已存在**：`Agent.inventory`/`equipment`，死亡走 `append_corpse_drop` → `Effect::AddGroundItem { contents }` | `effect.rs:520-534` |
| **世界历史生成** | **不存在**。`crates/ll-world/src/history.rs` 只有 `HistoricalEventKind::Kill` 一个变体，模块文档自己写明另外四个变体（含 `SettlementFounded`）「没有任何字段定案」 | `history.rs:13-27` |
| **`StructureKind`** | **代码里根本不存在**。只出现在四份设计文档的行文里（`society-and-affiliation.md:115` 给了一份枚举草图，该文档自己标注「未落地」） | grep 全仓库 `crates/`/`mods/`/`scripts/` 零命中 |
| **`SpaceProfile.buildable` / `diggable`** | **已声明、零玩法消费者**：只被 `content_audit`/`content_hash`/`space_profile_of` 三处**机械地**读取（哈希、审计、拼结构体），没有任何 `resolve`/`apply` 读它们 | `app.rs:829-830`、`content_audit.rs:1165`、`content_hash.rs:922` |
| **区域气候** | **不存在**。`SpaceProfile.base_temperature` 是**按空间类型**的一个常量（整个地表共用一个值），`Weather` 由 `weather_kind_at(world_seed, tick, table)` 全局派生，与坐标无关 | `weather.rs:483`、`temperature.rs:209` |
| **`Agent.subclasses` 的写入路径** | **完全不存在**：`grep GrantSubclass` 零命中，`ll-sim/src/apply.rs` 里 `subclasses` 只在一处测试夹具的 `Vec::new()` 出现。`subclass-system.md` 说「获得机制整套隐式只对玩家」，实际比这更糟——**对谁都不存在** | `apply.rs:552` 是唯一命中 |

**结论**：所有者列的「最基础的那批东西」里，墙、门、窗、地板、楼梯、建筑内部、容器机制**已经全部躺在代码里**。真正缺的只有田地、床、树的交互，以及 NPC 这条主线。这是本设计能收窄到「三个接线点 + 一段生成器」的全部理由。

---

## 三、结构物与可交互物：不新建结构物类型

### 3.1 判据

ADR 0021 的判据是「**有没有一份算法要被共用**」，而且它是双向的：既拦「为对称而抽象」，也拦「把同一份算法复制四遍」。逐个候选表达过一遍：

| 候选 | 它自带什么算法 | 谁该走 |
|---|---|---|
| **地形**（`TerrainDef` + `ChunkGrid` 每格一个 `TerrainKind`） | FOV 遮挡（`blocks_sight`）、寻路代价（`move_cost`）、通行判定（`blocks_move`）、声明式转换（`opens_into`）、按格存储与流式加载、`Effect::SetTerrain` 写入 | 墙、门、窗、地板、楼梯、**田地**、**床**、**树** |
| **地面物品**（`GroundItemStack`） | 老化清理、堆叠合并、`contents` 容器搜刮（`resolve_loot`）、掉落/拾取 | **抽屉/箱子** |
| **实体**（`Agent`） | 时间轴调度、行为树决策、属性派生、装备 | 谁都不该（床和树显然不是） |
| **新的 `StructureKind` 类型** | —— | **不建** |

**为什么不建 `StructureKind`。** 它没有任何一份自己的算法。把床做成一个新类型，就要为它重写：怎么按格存储、怎么参与 FOV、怎么参与寻路、怎么进 `WorldState::hash()`、怎么进存档 remap——**这四样地形层每一样都已经写好了**。`society-and-affiliation.md:115` 那份 `StructureKind` 枚举草图（聚落/道路/遗迹/地标/资源点）描述的是**世界生成期的地图结构层**，粒度是「这个区块有一座城」，不是「这一格有一张床」；`crafting-system.md:390` 与 `food-and-cooking-system.md:211` 两处已经各自核实并写明了这一点。两件事同名不同物，不要混。本文档四节要用的正是那个粗粒度的东西，但它在本设计里不需要成为一个 `enum`——见 6.3。

**反向检查（ADR 0021 的另一半）。** 有没有把同一份算法复制多遍的地方？有一处，见 6.4：`ll-game::world::find_spawn_site` 的连通陆地判定，据点选址需要**同一份**算法，绝不能复制。

### 3.2 逐个落点

- **墙 / 门 / 窗 / 地板 / 楼梯**：已经是地形。**本设计对它们零改动。**
- **田地**：三种地形 `field_tilled`（已耕）/ `field_sown`（已播）/ `field_ripe`（可收）。生长推进 = `Effect::SetTerrain`，与门的开合是同一个机械操作。三种独立地形而不是「一种地形 + 一个生长阶段字段」：`ChunkGrid` 每格只存一个 `TerrainKind`，加一个平行的「每格生长阶段」网格等于凭空多一份需要与地形同步的存储（本仓库已经为「同一概念被独立定义两次」付过三次代价：ADR 0010、`Affiliation.org`、`Interior` 反向索引）。谁来推进生长，见十一节「将来扩展 ①」。
- **床**：地形 `bed`，`blocks_move: false`（要能躺上去）、`blocks_sight: false`。本批次它**只是一张能站上去的地板加一张贴图**——「睡在床上恢复更快」需要 `resolve_rest` 读一个地形属性，那是给 `TerrainDef` 加字段，见十一节「将来扩展 ②」。**如实标注：本批次的床没有玩法后果。** 这是刻意的 YAGNI 边界，不是遗漏。
- **抽屉 / 箱子**：地面物品，`contents` 非空。`resolve_loot` 今天就能搜刮它。**但有一处真实缺陷必须在实现前解决**，见 3.3。
- **树**：地形 `forest` 原样保留（属性已经对：可通行、慢、遮挡视线），交互见 3.4，渲染见四节。

### 3.3 抽屉的两处缺陷（实现前必须处理）

核实 `resolve_loot`（`resolve.rs:1795`）：

```
Effect::RemoveGroundItem { pos, def }   // 容器本身被移除
+ 每件 contents → MergeIntoInventory
```

- **缺陷 A：搜刮完抽屉会消失。** 对尸体这是对的（尸体搜刮干净就该没了），对抽屉是错的。
  **最小解，零新增 `Effect`**：`resolve_loot` 在容器为「常驻容器」时，产出 `RemoveGroundItem` + `AddGroundItem { stack, dropped_at, contents: vec![] }`——把它原样放回去，只是空了。两个 `Effect` 都已落地（`effect.rs:500/520`）。「是不是常驻容器」是一条一档静态声明：`register-item-persistent-container "lostland:drawer"`，写法照抄已落地的 `register-item-theft-exempt` 先例（见 `ownership-and-crime-detection.md`）。
  *`dropped_at` 要不要刷新？* **要刷成当前 tick**，否则一个空抽屉会按它最初被放下的时刻老化掉。这与「尸体和内容物作为一个整体老化」（`item.rs:158-161`）不冲突——那条讲的是尸体，常驻容器本就不该被老化清理掉，刷新 `dropped_at` 是把它永远推到清理窗口之外的最省事写法。**如果所有者希望抽屉严格不可清理，那需要给 `cleanup_aged_ground_items` 加豁免，那是另一条路，见十一节「将来扩展 ③」。**
- **缺陷 B：抽屉能被 `Intent::PickUp` 整个揣走。** 同一条 `register-item-persistent-container` 声明可以同时被 `resolve_pick_up` 读来拒绝——**同一份声明，两个消费者**，正好对上 `subclass-system.md` 对「死字段」的那条批评（`buildable`/`diggable` 至今零消费者）。

### 3.4 树的交互：不新建机制，扩一个已落地的字段

所有者要求「树都变得可以交互」。砍树的完整语义是：

> 站在这一格 → 手里有斧子 → 花一个行动 → 这格从「森林」变成「草地」 → 背包里多出木材

逐条对照已落地的 `resolve_craft`（`resolve.rs:2271` 起，`08cdeb0` 落地）：

| 砍树需要 | `resolve_craft` 已有 |
|---|---|
| 站在特定地形上 | `recipe-requires-station!` → `terrain_at` 判定（`crafting-system.md` 六节「场地 = 地形」） |
| 手里有特定工具 | `recipe-requires-tool!` → 装备着且耐久未归零 |
| 消耗材料 | `ingredients`（砍树填空列表） |
| 产出物品进背包 | `product` / `product_count` → `Effect::MergeIntoInventory` |
| 花一个行动 | `BASE_ACTION_COST` → `Effect::ScheduleNext` |
| 副职闸门 | `category` 上的 `recipe-category-requires-subclass!` |
| **这格地形变成另一种** | **缺这一条** |

**九分之八已经写好了。** 为砍树另起一套 `HarvestRuleDef` + `Intent::Harvest` + `resolve_harvest`，就是把验证/扣减/产出/计费这四段算法**再抄一遍**——这正是 ADR 0021 反向拦截的那件事，`crafting-system.md` 也已经用完全相同的论证否决过「烹饪/锻造/裁缝/炼金各建一套」。

**结论：给 `RecipeDef` 加一个可选字段 `station_becomes: Option<ContentIndex>`（地形索引）。**

```scheme
;; mods/lostland/recipes.scm
(register-recipe "lostland:chop_tree" "lostland:recipe.chop_tree.display_name"
                 "lostland:gathering" "lostland:log" 3)
(recipe-requires-station! "lostland:chop_tree" "lostland:forest")
(recipe-requires-tool!    "lostland:chop_tree" "lostland:axe")
(recipe-becomes-terrain!  "lostland:chop_tree" "lostland:grass")   ;; ← 本设计新增
```

`resolve_craft` 的产出序列末尾追加一条 `Effect::SetTerrain { pos: agent.pos, kind: station_becomes }`。**零新增 `Intent`、零新增 `Effect`、零新增内容表。**

顺带白拿的两件事：

- **收获田地** = `lostland:field_ripe` → `lostland:field_tilled`，产出谷物。同一条路径，一行内容。
- **挖矿** = `lostland:mountain` → 某种矿坑地形，产出矿石。同一条路径。`SpaceProfile.diggable` 那个死字段在这里终于有了第一个可能的消费者（`resolve_craft` 在产出 `SetTerrain` 之前查一次「这个空间允许挖掘吗」），**但本批次不接**——见十一节「将来扩展 ④」。

**命名**：用 `recipe-becomes-terrain!`（`!` 后缀）而不是 `register-recipe-becomes-terrain`。仓库里两套惯例并存：`register-*`（新增一条内容）与 `*!`（往已注册的一条上追加属性）。`recipe-requires-station!`/`recipe-requires-tool!` 是同一张表的直接邻居，跟它们走。（`subclass-system.md` 七节说「两条已落地惯例都是 `register-*`」——**那条判断在配方表上不成立**，`script_recipe_api.rs` 里的两个函数名都是 `!` 后缀。这是一处本文档核实出的、与既有设计文档不符的地方。）

---

## 四、树：从方块变成图案

所有者要求「树的贴图不再是一个方块，而是带有图案的东西来表达」。这一节给可执行结论，不设计整套美术管线。

### 4.1 核实渲染现状

- 地形每格一张贴图：`app.rs:781-807` 遍历 `camera.visible_tiles_zoomed(zoom)`，每格 `terrain_atlas_key` → `resources.lookup(&name)` → `resources.batch.push(order, sprite_instance(...))`，`order = DrawOrder::new(Layer::TERRAIN, sy, ...)`。
- 图集条目**已经带 `pivot` 与 `footprint`**（`assets/atlas/placeholder.json`、`assets/sprites/manifest.json5`、`ll_render::sprite::{Pivot, Footprint}`）。`sprite.rs` 模块文档明确写了这套设计的目的：「重点目标的精灵是 32×48 像素，却只占 2×2 格——它画得比自己占的地方高」。
- **但地形绘制路径完全不用它们**：直接 `sprite_instance(zx, zy, entry.sprite_size(), ...)`，在原始屏幕坐标画一个与图像等大的四边形。`sprite_draw_position`/`footprint_bottom_screen_y` 这两个已经写好并有文档的函数在 `ll-game` 里**零调用点**。
- `Layer::DECOR`（`Layer(1)`，介于 `TERRAIN(0)` 与 `ENTITY(2)` 之间，文档写明用途是「草丛、碎石等不参与遮挡逻辑但需要按 Y 排的物件」）——**全仓库零使用者**。
- 图集是 RGBA，`pack_atlas` 现场打包松散贴图。透明通道可用。

### 4.2 「带图案的东西」在格子化 roguelike 里是什么

两条路：

- **(a) 一棵树占多格**：`footprint` 2×2 或 1×2，逻辑上要么占地多格（那要改 `ChunkGrid` 的一格一 `TerrainKind` 模型，推翻寻路/FOV），要么逻辑仍占一格而只是画得大（那 `footprint` 就名不副实）。
- **(b) 一格，但贴图是带透明轮廓的树，画在地面之上**：地面照常画 `terrain_grass`，树的树干+树冠画在它上面，四周透明处露出草地。

**选 (b)。** 理由不是省事：所有者的原话是「树的贴图不再是一个方块」——他要的是**视觉上不是方块**，不是「一棵树占两格」。(b) 完全满足，且**逻辑模型一格不动**：`forest` 仍是一格地形，`blocks_sight`/`move_cost` 原样，寻路、FOV、存档、内容哈希**全部零改动**。(a) 会把渲染层的一个愿望变成世界模型的一次重写，这是典型的过度设计。

### 4.3 具体做法

**在地形绘制循环里，每格再查一次「装饰键」，命中则在 `Layer::DECOR` 补一次绘制。**

```
装饰键 = format!("{terrain_atlas_key}__decor")
```

- `lostland:forest` → 地面画 `lostland:terrain_forest`（**这张图要重画成林地地面，不再是绿方块**），装饰画 `lostland:terrain_forest__decor`（树干+树冠，带透明通道）。
- 查不到 `__decor` 条目就跳过——**绝大多数地形没有装饰层，这是零成本的常见路径**，与 `terrain_atlas_key` 查不到时 `continue` 是同一条既有降级纪律。
- 装饰层的绘制位置**必须**走 `sprite_draw_position(tile_origin, entry.footprint(), entry.pivot())`，排序键走 `DrawOrder::new(Layer::DECOR, footprint_bottom_screen_y(sy, footprint.height), ...)`。这两个函数今天就在 `ll-render` 里，有完整文档，只是没人调用。**这是本节唯一需要写的新代码，约二十行。**

**为什么这解决了「方块」问题**：装饰精灵的像素高度可以远大于 16（例如 16×40 的一棵树），`pivot.y = 40` 让它的脚底落在格子上、树冠越过格子上沿——`sprite.rs` 模块文档描述的正是这个能力，`footprint_bottom_screen_y` 的文档更是直接写明「高精灵若用图像顶部排序，会在视觉上错误地挡住本该在它前面的矮单位」。**能力已经建好并有测试，只是从没被接上。**

### 4.4 与已知渲染限制的关系：不撞上

已知限制是「渲染 pass 顺序固定，贴图 pass 恒在纯色之后，两者无法按 z 序交错」。**这条限制在本节不生效**：地形、装饰、玩家标记**全部**经由同一个 `resources.batch.push` 进同一个 `SpriteBatch`，`DrawOrder` 在批内完整排序（`batch.rs:364-373` 的测试直接验收了 `TERRAIN`/`ENTITY` 跨层排序）。纯色 pass 与贴图 pass 的交错问题是 UI 层（`ll-ui` 的九宫格/纯色回退）的问题，世界精灵这条路不经过它。

### 4.5 z 序的一个刻意取舍

`DECOR(1) < ENTITY(2)`，所以**树画在玩家下面**。站在森林格上的玩家会盖住树。

这是对的，不是缺陷：`footprint` 是 1×1，玩家和树站在同一格，玩家必须可见，否则玩家会「消失在树里」。若要树冠遮住玩家，就得让树进 `ENTITY` 层并按 `foot_y` 与玩家比较——那要求树有一个比玩家更靠前的脚点，而同格意味着脚点相同，只能靠稳定的 tie-break 决定，且会立刻引出「玩家走进森林就看不见自己」的可玩性问题。**明确否决，不列为将来扩展。**

### 4.6 这一节不新增任何内容类型

图集条目名的约定（`<命名空间 ID>` 与 `<命名空间 ID>__decor`）**就是全部机制**。`ll_mod::asset_vfs::ResolvedSprite::atlas_name` 已经规定「任意命名空间的精灵，图集条目名恒定就是完整 ID 字符串」，本节只是在这个既有约定上加一个后缀。

ADR 0018（每种玩法层内容类型都能从 mod 脚本注册）**天然满足**：mod 作者在自己的 `manifest.json5` 里声明一个名为 `mymod:huge_oak__decor`、`footprint {width:1,height:1}`、`pivot {x:8,y:48}` 的精灵，就得到一棵三格高的橡树，**不需要任何新的注册函数**。`footprint`/`pivot` 本来就是 mod 作者能填的清单字段。

**要改的资产**：`tools/ll-artgen` 需要生成 `terrain_forest.png`（改成林地地面）与 `terrain_forest__decor.png`（树）。属于资产管线，不在本文档设计范围，只记为实现前置。

---

## 五、NPC 职业：与 `ClassDef` 是同一个东西

### 5.1 结论

**猎户、屠夫、农夫、据点管理者、民兵、守卫全部走 `register-class`，与已落地的 `lostland:guard` 完全同一条路。不建第二个「NPC 身份」类型。**

### 5.2 论证

「玩家可选主职业」与「NPC 身份」是不是两个类型？逐条检验：

1. **有没有一份算法只属于其中一边？** 没有。两边都只做同一件事：在一张表里占一行，被 `Agent.profession` 这个 `ContentIndex` 指向，被行为树的 `self-has-profession?` 按字符串匹配，被 `register-class-trait` 挂天赋。ADR 0021 的判据在这里给出的是**否**。
2. **建第二个类型的实际代价。** 一个 `NpcRoleDef` 意味着：`Agent` 上多一个字段（存档格式变更 + `ll-content::remap` 新增一条重映射 + `WorldState::hash()` 新增覆盖 + 两道门禁互校）、`classify_index` 多一条分支、`content_audit` 多一条花名册、`GameplayTables` 多一个字段与 `pipeline.rs` 三处调用点、27 个 `GameplayTables` 构造点全部要改。换来的是**零新增能力**。
3. **第三处定义的风险。** `society-and-affiliation.md` 已经把「职业」列为 `AffiliationKind` 六类之一。**现在有两处在描述同一个概念**（`Agent.profession: ContentIndex` 与 `Affiliation { kind: Profession, .. }`），再加一个就是三处。本仓库为「同一概念被独立定义两次」付过三次代价（ADR 0010 白昼判定、`identity-and-ids.md` 的 `Affiliation.org`、`Interior` 反向索引）。**这一处不该有第四次。**

### 5.3 但「玩家能不能选」是个真问题——它是一列，不是一个类型

所有者不会希望玩家在创建角色时能选「屠夫」。这个区分是真的，但它是 `ClassDef` 上的一个布尔属性，不是一个平行类型：

```scheme
(register-class "lostland:butcher" "lostland:class.butcher.display_name" "strength")
(class-not-playable! "lostland:butcher")     ;; 默认可选，显式关闭
```

**本批次不做。** 理由：唯一的消费者是角色创建界面，而角色创建界面不存在（规格 §15，P7）。加一个此刻零消费者的字段，就是再造一个 `buildable`/`diggable`。列为将来扩展，前置写明。**如实标注：本批次里 `lostland:butcher` 与 `lostland:guard` 在数据上没有任何区别。**

### 5.4 与本文档第 2 个新接线点的关系

`ClassDef` 现有的字段（`primary_attribute`、经 `register-class-trait` 挂的天赋、经 `register-class-xp-curve` 挂的经验曲线）对 NPC 全部适用且有意义——一个猎户力量/敏捷倾向、有「追踪」天赋，都是自然的表达。**唯一缺的是「这个职业的 NPC 用哪棵行为树」**，见六节 6.1。

### 5.5 NPC 的副职从哪来：本文档回答不了，且缺口比记录的更大

`subclass-system.md` 记录的漏洞是「整套副职获得机制隐式只对玩家（`MarkExplored` 玩家专属、NPC 从不提交 `Intent::Craft`）」。

**本文档核实出这个记录本身是乐观的**：`Effect::GrantSubclass` 在代码里**不存在**（`grep GrantSubclass` 全仓库零命中），`ll-sim/src/apply.rs` 里 `subclasses` 唯一命中是 `apply.rs:552` 一处测试夹具的 `Vec::new()`。也就是说**副职对玩家也同样不存在**——它不是「只对玩家」，是「对谁都没有」。`Agent.subclasses` 今天是一个只读的空列表，唯一的消费者是 `resolve_craft` 的闸门（`resolve.rs:2340-2344`），而那道闸门在没有任何授予路径的前提下等价于「凡是声明了副职要求的配方类别，谁都做不了」。

**这对本设计的直接后果**：`lostland:gathering`（砍树/收获所属的类别）**绝不能**声明副职要求，否则砍树在落地当天就是死的。本文档因此规定：`recipe-category-requires-subclass!` 在采集类别上留空——**这与 `crafting-system.md`「副职闸门挂在类别上」不冲突**（空列表 = 人人可做，是该文档明写的合法取值）。

> **【2026-08-22 更新】本节记录的缺口已被「副职获得机制」批次修掉**：`Effect::GrantSubclass`/
> `Effect::RemoveSubclass` 已落地（`crates/ll-sim/src/effect.rs`），`Agent.subclasses` 的写入路径是
> `ll_sim::subclass::grant_subclass_effects`（三条授予路径的唯一出口），使用计数触发器
> `ItemsCrafted` 已接进 `resolve_dispatch`。**⑩「NPC 获得副职」的前置因此已经解除**，剩下的
> 是 NPC 自己那一半（NPC 从不提交 `Intent::Craft`，使用计数对它们结构上无效，需要走
> 「世界生成/职业注册时写死初始副职」那条路——它调同一个 `grant_subclass_effects`，不需要新出口）。
> 5.5 里「采集类别绝不能声明副职要求」那条约束的**成因**也随之改变：不再是「授予根本不存在」，
> 而是新核实出的死锁（获得条件与闸门指向同一类别时互相等死），见 `subclass-system.md` 八节⑤。

NPC 副职本身**不在本文档范围**，前置是 `Effect::GrantSubclass` 落地，那是副职系统自己的批次。

---

## 六、NPC 生成：三个新接线点里的两个

### 6.1 新接线点 ②：`ClassBehaviorBindings`——职业 → 行为树

**缺口**：`ScriptBehaviorSource` 一个实例一棵树（`tree_entry_fn: String`）。

**形状**：一张**只存绑定关系**的表，照抄已落地的 `XpCurveBindings`（`GameplayTables.xp_curve_bindings`，与 `xp_curve` 表分成两个字段，走同一条 `std::mem::take` 搬运手法——`pipeline.rs:190-198` 已经把这个先例的理由写清楚了）。

```scheme
;; mods/lostland/behavior_bindings.scm
(register-class-behavior "lostland:guard"   "guard-ai-tree")
(register-class-behavior "lostland:farmer"  "farmer-ai-tree")
(register-class-behavior "lostland:hunter"  "hunter-ai-tree")
```

**`ScriptBehaviorSource` 的改动**：
- 构造时不再接收单个 `tree_entry_fn`，改为接收一份 `BTreeMap<ContentIndex, String>`（职业索引 → 入口函数名）的一次性快照，与它已有的 `skill_index: BTreeMap<String, ContentIndex>` 快照是同一个手法、同一条理由（`script_behavior_api` 模块文档「为什么用一次性快照」）。
- `decide(world, actor)` 先读 `world.actors.get(actor)?.profession`，查快照拿入口函数名，没有绑定则返回 `None`——**`BehaviorTreeSource` 的降级契约已经规定 `None` 是合法返回**（「AI 算不出这一回合该干什么，不是异常」，`behavior.rs:70-79`），调用方 `resolve_ai_turn` 产出空效果，不 panic、不 `Err`。**零新增失败模式。**
- 所有行为树源码装进**同一个** `ScriptEngine`（`engine.load_source` 可以调多次），按函数名分派。

**为什么绑定在职业上，不建一个「生物模板」类型。** 再走一遍 ADR 0021：
- 一个 `NpcTemplateDef {race, class, tree}` 的三个字段里，`race` 与 `class` 各自已经是内容表的一行，`tree` 是本节要加的这一条——模板层的全部价值，是允许「同职业不同树」或「同树不同职业」。**现在没有任何一份内容设计需要这种错位。** YAGNI。
- 模板层还要在 `Agent` 上多存一个 `template: ContentIndex`（存档变更 + remap + hash 覆盖）。绑在职业上则**`Agent` 一个字段都不加**：`profession` 已经在那儿了。
- 若将来真出现「两个哥布林同职业但一个是斥候一个是萨满」的需求，正确的表达是**两个职业**，不是两个模板——这与 `subclass-system.md` 一之二节的判据（「存不存在一份想对其中一个开门、对另一个关门的内容设计」）同构。

**代价不对称，方向也对**：绑在职业上，将来要拆出模板层，是新增一张表（可加）；先建模板层再想合并，是丢弃已存在的 `Agent` 字段与存档数据（不可逆）。

**顺带解掉的一处已落地缺陷**：`mods/example_mod/behavior.scm` 头注释写明它「刻意不在 `entry_points` 里」，因为装载管线的引擎没注册那些运行期查询 API。本设计不改这一点——`ClassBehaviorBindings` 只记录**函数名字符串**，装载管线从不求值这些函数，行为树源码仍由 `ScriptBehaviorSource` 自己的引擎加载。装载期与运行期两套引擎的职责分离原样保持。

### 6.2 谁在真实游戏里调用它

`app.rs` 的 `no_npc_ai` 占位换成 `ll_sim::behavior::behavior_ai_intent(&mut source)`（该适配器 `behavior.rs:99` 已落地，文档称其为「行为树真的驱动回合推进这条链路的唯一标准接法」）。这是**本设计终点处的那一行代码**，两条阻塞（无 NPC、无绑定）在此处同时解除。

### 6.3 新接线点 ③：`SettlementTemplateTable`——据点里有谁

**形状**：一张表 + 追加式名册，照抄 `ItemTable` + `register-item-stat-bonus` 的「先注册一行、再往这行上追加列表项」模式。

```scheme
;; mods/lostland/settlements.scm
(register-settlement-template "lostland:hamlet" "lostland:settlement.hamlet.display_name"
                              9      ;; 建筑数（决定占地）
                              600)   ;; 选址要求：最小连通可行走陆地格数
(settlement-requires-fresh-water! "lostland:hamlet")
(settlement-role! "lostland:hamlet" "lostland:steward"  1)   ;; 据点管理者
(settlement-role! "lostland:hamlet" "lostland:farmer"   4)
(settlement-role! "lostland:hamlet" "lostland:hunter"   2)
(settlement-role! "lostland:hamlet" "lostland:butcher"  1)
(settlement-role! "lostland:hamlet" "lostland:guard"    2)

(register-settlement-template "lostland:camp" "lostland:settlement.camp.display_name" 3 200)
(settlement-role! "lostland:camp" "lostland:hunter"  2)
(settlement-role! "lostland:camp" "lostland:militia" 1)
```

**ADR 0016/0017 一档**：全部是静态字面量声明，装载期物化成扁平列（`building_count: Vec<u32>`、`min_land_area: Vec<u32>`、`needs_fresh_water: Vec<bool>`、`roles: Vec<Vec<(ContentIndex, u32)>>`）。**运行期零脚本调用**——这是硬要求，见 6.5。

**这就是所有者说的「据点或者营地」**：`hamlet` 与 `camp` 是同一张表的两行，只是数值不同。不为「营地」单开一个类型（ADR 0021）。

### 6.4 选址：与 `find_spawn_site` 共用同一份算法

**「合理的地点」的判据**（全部只读地形，无额外数据源）：

1. **连通可行走陆地面积 ≥ `min_land_area`**——`ll-game::world::find_spawn_site` **今天就在做这件事**（`MIN_SPAWN_LAND_AREA = 500`，区块窗口内连通域分析，`MAX_SPAWN_SEARCH_ZONES = 128` 有界）。
2. **邻接淡水**（`needs_fresh_water` 为真时）：候选区块窗口内存在 `shallow_water`。
3. **不落在不可通行地形上**：建筑锚点格 `blocks_move` 为假。

**ADR 0021 的反向拦截在这里生效**：条件 1 与 `find_spawn_site` 是**同一份连通域分析算法**，据点生成器**绝不能**复制它。实现前置：把 `find_spawn_site` 里那段连通域分析提成一个参数化函数（阈值、区块坐标、地形表作参数），玩家出生点搜索与据点选址各调一次。这不是重构癖——复制它就是把「什么叫一块能住人的地」这条判据变成两个真相源，两边一旦漂移，就会出现「据点建在玩家出生点判定为不合格的碎地上」这类说不清的表现。

**建筑与地形的具体铺法**（哪一格是墙、哪一格是门、田地铺在哪）：本文档**不设计**。它是一个纯函数 `f(种子, 区块, 模板) → Vec<(TorusPos, TerrainKind)>`，输入输出边界在此定死，算法本身留给实现批次（一间 3×3 的小屋 = 八格墙 + 一格门 + 中间地板，足以验收）。

### 6.5 确定性（约束 C3）与性能（ADR 0016/0017）

**确定性**：
- **选址不用随机。** 它是 `f(world_seed, zone) → bool` 的纯地形判定——地形本身已经由 `TileableNoise(seed)` 完全确定（决策 0005），连通域分析是确定性算法。**没有 RNG 就没有 C3 问题。**
- **构成需要随机**（同一个模板下的 NPC 具体属性、名字、建筑朝向）。走 `DetRng::for_entity(world_seed, SETTLEMENT_STREAM_ID, zone_linear_index)`——**这不是滥用 `for_entity`**：已落地的 `weather_kind_at`（`weather.rs:494`）用的正是这个形状（固定的 `WEATHER_STREAM_ID` 作 `entity_id`，时间周期号作 `event_counter`），本设计只是把「时间周期号」换成「区块线性号」。**同一条已落地先例，同一个函数。**
- **区块线性号必须由 `ZoneLayout` 按规范顺序算出**（`zone.y() * zone_count.x() + zone.x()`），不得来自任何 `HashMap`/`HashSet` 迭代（约束 C5）。
- **推论**：同一个种子的同一个区块，无论玩家什么时候走到、以什么顺序流式加载，据点都完全相同。据点因此**不需要进存档**——它是 ADR 0009「默认派生，只存偏差」的又一次应用，与地形本身同一条纪律。**但生成出来的 `Agent` 必须进存档**（NPC 会移动、会死、会掉东西，那些是偏差），这与「据点布局不进存档」不矛盾：`ChunkGrid` 本来就随存档走（`WorldState` 的字段），生成器只在区块**首次**物化时跑一次。

**性能**：据点生成挂在区块流入这条路上，是热路径。
- 跨脚本边界每次 326ns（ADR 0016）。一个区块 48×48 = 2304 格，每格一次脚本调用 = 750µs，**流式加载会肉眼卡顿**。因此：**据点生成器整段是纯 Rust，读扁平列，一次脚本调用都不发生。**
- 这与 ADR 0018（玩法层内容可从 mod 注册）不冲突：mod 注册的是**数据**（模板、名册、阈值），执行的是框架。这正是所有者「Rust = 框架能力，Steel 脚本 = 具体内容」分工原则的标准形状。
- 一档判据的检验：`register-settlement-template` 的每一个参数都是字面量，没有任何一处需要在运行期回调脚本。**通过。**

### 6.6 生成出来的 NPC 长什么样

按名册，每个 `(class, count)` 生成 `count` 个 `Agent`：

- `profession` = 该职业索引。
- `race` = 该据点的种族（本批次：固定 `lostland:human`；种族分布场是 `world-history.md` 流程第二步，不存在——见十一节「将来扩展 ⑤」）。
- 属性/血量/背包 = **复用 `build_player_agent` 那条已落地的路径**（`world.rs:524`），把种族/职业换掉即可。`register-race-starting-item` 已落地，NPC 的初始物品白拿。
- `next_action_at` = `world.clock`，然后 `rebuild_timeline`——**已落地**（`world.rs:277`），NPC 直接进时间轴。
- 死亡掉落 = `append_corpse_drop`，**已落地**，与所有者早先裁定「其他 NPC 也存在物品列表，死亡后爆出身上所有物品」完全一致。

**不新增 `Effect`。** 世界生成期的 `actors.spawn` 是直接构造，与 `spawn_player` 同一条路，不经 `Intent`/`resolve`/`apply`——`Effect::Kill` 等等管的是**游戏内**的状态变更，世界生成是在世界存在之前。（若将来要「游戏中动态刷怪」，那需要一个 `Effect::SpawnActor`，属于另一件事，见十一节「将来扩展 ⑥」。）

---

## 七、据点与「历史」：两步，历史在后

所有者说「这东西需要根据历史生成」。本节给出一个**不推翻这句话、但现在就能落地**的顺序。

### 7.1 冲突：历史生成不存在，而且它是 P7 规模

> **【2026-08-30 复核：下面这条核实结论已过期，正文原样保留。】** **世界历史生成已经落地并在跑**：`crates/ll-world/src/history.rs:122`-`131` 的 `HistoricalEventKind` 有四个变体（含 `SettlementFounded`/`Abandoned`/`Conquered`），编年史推演在 `crates/ll-world/src/chronicle.rs:140`，据点写进地形在 `crates/ll-world/src/settlement.rs`，势力在 `crates/ll-world/src/faction.rs:200`。「`history.rs` 只有 `Kill` 一个变体」这句核实**已经不成立**。逐条见 [2026-08-29 文档—代码一致性审计](../audit/2026-08-29-doc-code-audit.md) 一节第 1 条。

`world-history.md` 冻结于 2026-08-17，实现阶段列为 P7，落地状态「纯设计，尚无代码」——**本文档核实：仍然如此**（`history.rs` 只有 `Kill` 一个变体，模块文档自己写明 `SettlementFounded` 等四个变体「连字段都不存在」）。该文档描述的是 500 年 × 500 聚落的聚合量模拟、王朝继承、联姻、政变、一万五千名历史人物。

**这不是可以顺手夹带的东西。** 而 NPC 需要现在就能生成。

### 7.2 解法：派生是默认，历史是偏差

ADR 0009「默认派生，只存偏差」——`world-history.md` 自己一节就在用这条原则（「百万 NPC 是玩家落地后、从聚落属性现算派生出来的」）。把同一条原则往上再推一层：

- **第一步（本设计）**：据点位置与构成是 `f(种子, 区块, 地形)` 的纯派生。没有历史，没有存储。
- **第二步（`world-history.md` 落地后）**：历史生成产出一份**偏差表**——「第 214 年，`zone(31,17)` 的聚落被烧毁」「第 380 年，`zone(9,42)` 由某王朝建城，人口翻倍」。派生函数照跑，偏差表覆盖它。

**这个顺序不是妥协，是这个项目自己的原则。** 而且它有一个具体好处：第二步落地时，第一步的派生函数**不需要改**，只需要在它外面套一层「查偏差表」——与 `Agent` 的属性派生/偏差存储是同一个形状。

### 7.3 与温度/天气的关系：**没有关系，而且不能有**

所有者的裁决里没有点到温度，但任务书问了。**核实结论：目前不可能有关系。**

- `SpaceProfile.base_temperature` 是**按空间层类型**的一个常量（`space_profile.rs`），整个地表共用一个值。它不随坐标变化。
- `Weather` 由 `weather_kind_at(world_seed, tick, table)` 派生，**参数里没有坐标**——全世界同一时刻同一种天气。
- `temperature_under(base, tick, weather)` = 空间基准 + 季节偏移 + 昼夜偏移 + 天气偏移。**四项没有一项与位置有关。**

所以「寒冷区域的据点」**在当前模型里不可表达**。前置是「区域气候」，而区域气候不存在。

**但有一条不需要新机制的近似**：`snow`/`sand` 地形本身就是高度阈值的产物，是有坐标的。据点模板可以按**锚点地形**分流（雪地上的据点用另一份职业名册：多猎户、少农夫）。这与「气候」不是一回事——它是地形驱动的，不是温度驱动的——但它足够表达所有者要的那种差异，且**零新增机制**（`SettlementTemplateTable` 上加一个「适用地形」列即可）。**本批次不做**，见十一节「将来扩展 ⑦」，因为最小可用形状只需要一个 `hamlet` 和一个 `camp`。

---

## 八、三个新接线点的完整接线清单

按仓库的既有清单逐项列出。**注意：`CONTENT_HASH_ALGORITHM_VERSION` 当前是 11**（`content_hash.rs:345`），三个接线点若同批落地只需**递增一次到 12**；若分批落地则每批各递增一次。

### 8.1 `RecipeDef.station_becomes`（新**字段**，不是新表）

| 项 | 内容 |
|---|---|
| 注册函数 | `recipe-becomes-terrain!`，写在 `crates/ll-mod/src/script_recipe_api.rs`（与 `recipe-requires-station!` 同一文件） |
| `GameplayTables` | **不变**——写进已有的 `tables.recipe` |
| `pipeline.rs` 三处调用点 | **不变**——`set_active_recipe_target` / `register_recipe_api` / `take_active_recipe_target` 三处已存在，新函数在 `register_recipe_api` 内部一并注册 |
| 27 个 `GameplayTables` 构造点 | **不变** |
| `classify_index` 穷尽 match | **不变**（不是新表） |
| `ContentValueTables` 穷尽解构 | **不变** |
| `entry_value_digest` 分支 | **不变** |
| `write_recipe_fields` | **必改**：新增一行 `hasher.write_u64(...)` 覆盖 `station_becomes`（ADR 0027：内容哈希覆盖字段值） |
| `CONTENT_HASH_ALGORITHM_VERSION` | **11 → 12** |
| `content_audit.rs` 花名册 | **必改**：`RecipeDef` 已在 `covered` 里，需补一条 `auditor.field("RecipeAttrs::station_becomes", ...)` |
| 两道门禁互校 | `check_field_consumers.py::check_content_hash_gate_cross_coverage` 会自动比对，无需手改 |
| `ll-game/content.rs` `Opaque` 回归测试 | **不变**（不是新表，不产生新索引类别） |
| i18n 两份 `.ftl` | **不变**（无新增显示名键；砍树配方本身的 `display_name_key` 是内容，随 `mods/lostland/recipes.scm` 一起补） |
| 依赖倒置 | `ll_sim::recipe::RecipeCatalog`（`ll-sim` 定 trait、`ll-mod` 实现）**必改**：新增一个 `station_becomes(index) -> Option<ContentIndex>` 方法 |
| 存档 remap | **不变**（`RecipeDef` 不进 `WorldState`；被 `SetTerrain` 改写的地形本来就随 `ChunkGrid` 走既有 remap） |
| 真实 mod 脚本佐证（ADR 0018） | `mods/example_mod/gameplay.scm` 补一条用 `examplemod:lava_floor` 做场地并把它变成 `examplemod:paved_floor` 的配方——两种地形**都已经在 `mods/example_mod/terrain.scm` 里注册好了** |

### 8.2 `ClassBehaviorBindings`（新**表**）

| 项 | 内容 |
|---|---|
| 注册函数 | `register-class-behavior`，新文件 `crates/ll-mod/src/script_class_behavior_api.rs` |
| `GameplayTables` 字段 | **新增** `pub class_behavior: &'a mut ClassBehaviorBindings`（照 `xp_curve_bindings` 的先例：与 `class` 表分成两个字段，走 `std::mem::take` 搬运） |
| `pipeline.rs` 三处调用点 | **新增三处**：`set_active_class_behavior_target(std::mem::take(tables.class_behavior))`（~540 行区）、`register_class_behavior_api(&mut engine)`（~560 行区）、`*tables.class_behavior = take_active_class_behavior_target()`（~583 行区） |
| 27 个 `GameplayTables` 构造点 | **全部要改**（`ll-game/src/content.rs`、`ll-mod/src/pipeline.rs`、`ll-mod/tests/*.rs` 20 个、`ll-ui/examples/p4_acceptance/world.rs`） |
| 内容值哈希 | **`ClassBehaviorBindings` 不产生新的 `ContentIndex`**——它的键是已注册的职业索引，值是一个字符串。因此 `classify_index` **不加分支**（与 `XpCurveBindings` 同一处境：`classify_index` 里只有 `xp_curve` 表，没有 `xp_curve_bindings`）。**但绑定值必须进内容哈希**，落点在 `write_class_fields`（职业条目多写一列「它绑的树名」），与 `XpCurveBindings` 的既有做法对齐——**实现前必须先读 `content_hash.rs` 里 `xp_curve_bindings` 究竟落在哪一列，照它抄，不要另发明** |
| `CONTENT_HASH_ALGORITHM_VERSION` | **递增** |
| `content_audit.rs` 花名册 | **必改**：绑定值作为 `ClassDef` 的一个字段登记（`auditor.field("ClassAttrs::behavior_tree", ...)`）。若判定它该独立成一张受审表，则须进 `covered` 或带理由进 `deferred`，二选一，漏了会报 `UnknownExemption` |
| `ll-game/content.rs` `Opaque` 回归测试 | **不变**（不产生新索引类别） |
| i18n | **不变**（函数名不是显示文本） |
| 依赖倒置 | **不需要新 trait**：`ScriptBehaviorSource` 本身就在 `ll-mod`，它直接读 `ll-mod` 的表。`ll-sim` 侧的 `BehaviorTreeSource` trait 已落地且签名不变 |
| 存档 remap | **不变**（绑定不进 `WorldState`；`Agent.profession` 的 remap 已落地） |
| 真实 mod 脚本佐证 | `mods/example_mod/gameplay.scm` 补 `(register-class-behavior "examplemod:goblin_mage" "goblin-ai-tree")`——`goblin-ai-tree` **已经写好了**，缺的只是一个能引用它的职业 |

### 8.3 `SettlementTemplateTable`（新**表**）

| 项 | 内容 |
|---|---|
| 注册函数 | `register-settlement-template` + `settlement-role!` + `settlement-requires-fresh-water!`，新文件 `crates/ll-mod/src/script_settlement_api.rs` |
| `GameplayTables` 字段 | **新增** `pub settlement: &'a mut SettlementTemplateTable`（名册作为该表上的一个 `Vec` 列，追加式，照 `register-item-stat-bonus` 的先例，不另开第二张表） |
| `pipeline.rs` 三处调用点 | **新增三处**，同 8.2 |
| 27 个 `GameplayTables` 构造点 | **全部要改** |
| 内容值哈希 | `classify_index` **新增一条分支** `ContentTableKind::Settlement`；`ContentValueTables` 穷尽解构**新增一个字段**；`entry_value_digest` **新增一条分支**；新增 `write_settlement_fields`（覆盖 `building_count`/`min_land_area`/`needs_fresh_water`/`roles` 四项，ADR 0027） |
| `CONTENT_HASH_ALGORITHM_VERSION` | **递增**（新表必须递增） |
| `ll-game/content.rs` `Opaque` 回归测试 | **必改**：新增一条断言，确认据点模板索引被分类为 `Settlement` 而非 `Opaque` |
| 两道门禁互校 | `check_content_hash_gate_cross_coverage` 会要求 `content_audit` 与 `content_hash` 两边覆盖同一组字段 |
| `content_audit.rs` 花名册 | **必改**：`ContentTableKind::Settlement` 进 `covered`，四个字段逐一 `auditor.field(...)`，外加一条跨表引用检查（名册里的职业索引必须都在 `ClassTable` 里） |
| i18n 两份 `.ftl` | **必改**：`settlement-hamlet-display_name` / `settlement-camp-display_name` 两条 × 两份文件（`assets/locales/en.ftl`、`assets/locales/zh-CN.ftl`），`scripts/ci/check_i18n_strings.py` 会查 |
| 依赖倒置 | **需要新 trait**：据点生成器住在 `ll-game`（它要调 `world.actors.spawn` 与 `ChunkGrid` 写入），`ll-game` 依赖 `ll-mod`，可以直接读表。**但若生成器要下沉到 `ll-world`**（更合理——它是世界生成的一部分，而 `generate.rs` 就在那儿），则 `ll-world` 不能认识 `ll-mod`，必须照 `materialize_base_terrain` 的先例接受注入的查询回调，或照 `SkillCatalog`/`BehaviorTreeSource` 的先例在 `ll-sim` 定 trait。**这个落点选择留给实现批次，本文档只标出它是个必须先答的问题** |
| 存档 remap | **模板本身不进 `WorldState`**（据点是派生的，7.2）。**但生成出来的 `Agent.profession` 进**，而它的 remap 已落地 |
| 真实 mod 脚本佐证 | `mods/example_mod/gameplay.scm` 补一个只有哥布林的营地模板 |

### 8.4 `register-item-persistent-container`（3.3 的抽屉修复）

若与上面同批落地：它是 `ItemTable` 上的一个布尔列，接线形状与 8.1 完全相同（无新表、`write_item_fields` 加一行、`content_audit` 加一条字段、版本号已在同批递增）。照 `register-item-theft-exempt` 抄。

---

## 九、最小可用形状：一次能验收的东西

把上面全部收窄成一个可验收的目标：

> **新游戏开局，玩家出生点附近某个区块里有一座九间屋的村子。村子里有一个据点管理者、四个农夫、两个猎户、一个屠夫、两个民兵。他们在时间轴上真的行动。村外有树，玩家拿着斧子站上去可以砍掉它、背包里多三根木头、那一格变成草地。树在画面上是一棵带轮廓的树，不是一个绿方块。**

验收路径全部经过已落地的机制：时间轴（`rebuild_timeline`）、行为树（`ScriptBehaviorSource` + `behavior_ai_intent`）、制作（`resolve_craft`）、地形改写（`Effect::SetTerrain`）、精灵批（`SpriteBatch` + `DrawOrder`）。

**新写的代码只有四段**：
1. `resolve_craft` 末尾追加 `SetTerrain`（约 5 行）
2. `ScriptBehaviorSource` 由单树改为按职业查表（约 30 行）
3. 地形绘制循环补一次 `DECOR` 层查找（约 20 行）
4. 据点生成器（约 200 行，含从 `find_spawn_site` 提出来的共用连通域分析）

**新写的内容（Steel 脚本 + 资产）**：`mods/lostland/` 补 `terrain.scm`（田地/床）、`recipes.scm`（砍树/收获）、`settlements.scm`、`behavior_bindings.scm`，`classes.scm` 补六条职业；`mods/example_mod/behavior.scm` 补农夫/猎户两棵树；`tools/ll-artgen` 补两三张贴图。

---

## 十、与既有设计文档不符的地方（核实结果）

| 文档说 | 实际是 | 影响 |
|---|---|---|
| `world-history.md`：「纯设计，尚无代码」 | `crates/ll-world/src/history.rs` 有 251 行（`HistoricalEvent`/`KillReport`/`HistoricalEventKind::Kill`） | 该文档的说法对**历史生成**成立、对**历史事件类型**不成立。信封已定型（`kill-and-death-events.md` 才是它的真实归属） |
| `README.md` 索引把坐标系文档列为「只给接口设想」 | `crates/ll-world/src/interior.rs` 577 行、`space.rs`/`surface_store.rs` 均已落地 | 「建筑内部」这件事比索引给人的印象成熟得多，本设计因此没有为它设计任何东西 |
| `subclass-system.md` 七节：「按已落地的两条脚本命名惯例」把追加式函数改名为 `register-*` | `crates/ll-mod/src/script_recipe_api.rs` 里是 `recipe-requires-station!` / `recipe-requires-tool!`，**`!` 后缀** | 该判断在配方表上不成立。本文档的 `recipe-becomes-terrain!` 跟随其直接邻居 |
| `subclass-system.md`：副职获得机制「隐式只对玩家」 | `Effect::GrantSubclass` **全仓库零命中**，`Agent.subclasses` 没有任何写入路径 | 缺口比记录的更大：对谁都不存在。直接后果见 5.5——采集类别绝不能声明副职要求 |
| `crafting-system.md` 六节 / `food-and-cooking-system.md` 四节：把 `StructureKind` 说成「世界生成期的地图结构层」 | **代码里根本不存在这个类型** | 两份文档的表述会让读者以为它已经在某处。本文档三节明确：它只存在于 `society-and-affiliation.md:115` 的一份草图里 |
| `README.md` 缺口栏：`Settlement` 结构「三者尚未对齐」 | 三者**全都不存在** | 本设计的 `SettlementTemplateTable` 是「据点内容模板」，**不是** `world-history.md` 要的那个「历史生成的操作对象 `Settlement`」。两者将来必须对齐，见十一节 ⑧ |

---

## 十一、明确标为将来扩展的，各自前置

| # | 扩展 | 前置 |
|---|---|---|
| ① | **作物生长推进**（`field_sown` → `field_ripe`） | 需要一个「到某个 tick 就改这一格地形」的调度机制。时间轴（`Timeline`）只调度**实体**（`Agent.next_action_at`），没有「地格事件」这个概念。最省的路是让农夫 NPC 的行为树自己去推进（他走到田里、提交一个配方），那就**零新机制**——但需要农夫的行为树先存在 |
| ② | **床有玩法后果**（睡床上恢复更快） | `TerrainDef` 新增一个「休息品质」字段（内容哈希版本递增 + 字段覆盖），且 `resolve_rest` 要读它。`resolve_rest`/`RestState` 已落地，接线点明确，纯粹是本批次的 YAGNI 边界 |
| ③ | **常驻容器严格不被老化清理** | 3.3 给的「刷新 `dropped_at`」是近似。严格版需要给 `cleanup_aged_ground_items` 加豁免，参数化先例见 `ownership-and-crime-detection.md` 的销赃计时 |
| ④ | **挖矿**（`SpaceProfile.diggable` 的第一个真实消费者） | 机制上**今天就通**（8.1 的 `station_becomes` 加一次 `diggable` 查询）。缺的只是矿石物品与矿坑地形这两条内容，以及所有者对「要不要现在做采矿」的裁定 |
| ⑤ | **据点的种族分布** | `world-history.md` 流程第二步「种族分布场」，不存在。本批次全部人类 |
| ⑥ | **游戏中动态刷怪**（不是世界生成期） | 需要 `Effect::SpawnActor`（新变体：`WorldState` 写入必须经 `apply`，ADR 0023）。本设计的生成全部发生在区块首次物化时，走不到这条路 |
| ⑦ | **据点按锚点地形分流**（雪地据点 / 沙漠据点） | 零新机制：`SettlementTemplateTable` 加一个「适用地形」列。纯 YAGNI 推迟——最小可用形状只需要一个模板 |
| ⑧ | **历史覆盖派生的据点基线** | `world-history.md` 落地（P7）。7.2 已经把接口形状定死（派生函数不变，外面套一层偏差查询），但那份偏差表的字段要等历史生成自己定案。**同时必须解决**：本设计的 `SettlementTemplateTable`（内容模板）与 `world-history.md` 的 `Settlement`（历史操作对象）是两个东西，将来要对齐 |
| ⑨ | **职业的「玩家可选」标记** | 角色创建界面（规格 §15，P7）。5.3 已论证：在它之前加这个字段就是再造一个 `buildable`/`diggable` |
| ⑩ | **NPC 获得副职** | `Effect::GrantSubclass` 落地（副职系统自己的批次）。见 5.5——它对玩家也还不存在 |
| ⑪ | **可搬动的家具**（能捡起来放下的床/桌子） | ~~`crafting-system.md` 已核实：「物品变实体 / 可放置物件」这条路径**不存在**。本设计的床是地形，捡不起来~~ **家具层批次已落地这条路径**：`ItemDef.furniture` + `Intent::Drop` 放置 + `Intent::PickUp` 收回，见 [物品系统](item-system.md) 四节「家具」。本条的**阻碍消失了**，但本批次仍然没有把床/桌子做成家具（内容决策），也**没有**让 `stamp_settlement` 自动摆家具——那是「NPC 自己造建筑放家具」那条更大的路，见下面第 6 条待裁定 |
| ⑫ | **区域气候**（真正的「寒冷区域」） | `SpaceProfile.base_temperature` 与 `Weather` 都与坐标无关（7.3）。这是一次温度模型的重写，不是加个字段 |

---

## 十二、需要项目所有者裁决的（本文档不代为决定）

1. **「职业」到底有几个真相源。** `Agent.profession: ContentIndex`（已落地）与 `society-and-affiliation.md` 的 `AffiliationKind::Profession`（未落地）在描述同一个概念。本文档五节判定不该有第三处，但**没有裁决既有的这两处该合并成哪一个**。这是一处需要所有者拍板的架构问题，拖下去就是第四次「同一概念定义两次」。

2. **据点生成器住在哪个 crate。** `ll-game`（能直接读 `ll-mod` 的表，但世界生成散在两个 crate）还是 `ll-world`（与 `generate.rs` 同处，但要走注入回调/依赖倒置）。见 8.3。技术上两条路都通，取舍是「代码归属整洁」对「多一层间接」。

3. **「营地」是不是一定跟「据点」同一张表。** 本文档按 ADR 0021 判定是（同样的算法、不同的数据）。但所有者说的「营地」若指的是**临时的、会消失的、玩家自己搭的**东西，那它跟「据点」就不是同一件事——玩家搭的营地需要「可放置物件」这条不存在的路径（见扩展 ⑪）。**所有者的原话「文明据点或者营地或者某种合理的地点」不足以区分这两种理解。**（**家具层批次更新**：「可放置物件」这条路径现在存在了，见扩展 ⑪——这条待裁定的**技术阻碍**因此消失，剩下的纯粹是「所有者说的营地指哪一种」这个语义问题。）

4. **采集要不要有副职门槛。** 本文档 5.5 规定采集类别留空（否则砍树在落地当天就是死的），但这是被 `Effect::GrantSubclass` 不存在这个事实**逼出来的**，不是设计上的取舍。若所有者希望砍树需要「采集」副职，则必须先做副职授予机制，本设计的落地顺序要相应推后。

5. **树被砍掉之后会不会长回来。** 本设计只做「森林 → 草地」。反向（草地随时间变回森林）需要扩展 ①的同一套「地格随时间变化」机制，且会引出「玩家能不能把整片大陆砍光」这个纯玩法问题。**不代为决定。**

6. **`terrain_forest.png` 的语义变更。** 4.3 要求把它从「一片树林」改画成「林地地面」。这是既有资产的语义变更，影响世界地图（`continent_map` 用同一张贴图表示森林区块）——**远看的大陆地图上，森林会变成一片褐色地面而不是绿色树林。** 若所有者在意大陆地图的观感，那需要给世界地图另一套贴图键，属于另一件事。

7. **据点里该不该自动摆家具，谁来摆。** 家具层批次落地后这件事第一次成为可能，且**刻意没做**。两条路形状完全不同：（a）`stamp_settlement` 铺建筑时顺手往 `WorldState::ground_items` 里塞几件家具——纯派生、按 ADR 0009 不进存档，与铺墙铺门同一档；（b）NPC 自己造：那需要一个会提交 `Intent::Craft` + `Intent::Drop` 的 NPC 行为层，落在 `agent-goals-and-economy.md` 那一路，与本文档的「一次性铺地形」不是同一件事。**所有者说过「NPC 自己造建筑放家具」是后续**，因此（b）是已知方向；（a）是不是也要做、要不要与（b）并存（世界生成先摆一批、NPC 后续再造），需要裁定。**引擎侧不阻碍任何一条**：家具就是一堆 `GroundItemStack`，两条路产出的是同一个东西。
