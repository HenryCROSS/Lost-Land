# P7 → P8 交接清单

**补写于** 2026-08-26，基线 `main` HEAD `ed1584f`。
**读者**：P7 的收尾者，与 P8（随从与行为树、指令系统、据点派工）的规划者。

---

## ⚠ P7 还没有收尾，这份清单是预写的

前六份交接清单都是在对应阶段冻结时写的。这一份不是：**P7 至今还差三项交付物**（一节），而 P8 的第一件事（行为树）反而已经被一次架构撤退提前做掉了一半（三节）。

因此这份文档同时承担两件事：

1. **告诉 P7 的收尾者还差什么**——一节，这部分不是「留给下一阶段」，是「本阶段自己没做完」；
2. **告诉 P8 的规划者会继承什么**——三节起，其中最要紧的是「P8 的三件事全部踩在一套没有生产者的归属体系上」（四节）。

逐条证据见 [P6/P7/P8 阶段清算](../audit/2026-08-26-phase-reckoning-p6-p8.md)，本文档不重复。

---

## 一、P7 尚未交付的四项

规格 §15 P7 行逐字点名了「像素 UI 控件库（九宫格边框、焦点导航）、游戏内菜单、设置界面、i18n」，加上 2026-08-18 修订追加的世界生成器三项。核实结果：

| 交付物 | 状态 | 证据 |
|---|---|---|
| 九宫格边框 | ✅ | `crates/ll-ui/src/widget/skin.rs`，生产消费点 `crates/ll-game/src/app.rs:198`/`:236` |
| 焦点导航 | ✅ | `crates/ll-ui/src/widget/focus.rs`（提交 `f46c363`） |
| i18n | ✅ | `ll-i18n` crate + `assets/locales/{en,zh-CN}.ftl` + `check_i18n_strings.py` 门禁（提交 `3331b23`） |
| 世界生成器（`Space`/`SpaceProfile` 注册、聚落播种、区块粒度） | ✅ | `chronicle.rs` 2657 行、`settlement.rs` 1239 行、`resource.rs` 1038 行、`culture.rs`、`space_profile.rs` 1034 行 |
| **游戏内菜单** | ❌ **零实现** | 见下 |
| **设置界面** | ❌ **零实现** | 见下 |
| **生成期 mod 集合的真实绑定时机** | ✅ **已修**（`wt-genmodset` 批次） | 见二节 |
| **势力播种** | ❌ **零实现** | `OrgInstance`（`crates/ll-world/src/entity/org.rs:21`）全仓库零生产构造点 |

### 游戏内菜单：一处标准的「声明了但没接线」

`GameKey::Menu` 四样齐全：

- 变体定义 `crates/ll-platform/src/input.rs:48`
- 进默认键位表 `:126`
- i18n 显示名键 `:203`（`"lostland:keybind.action.menu"`）
- 进设置界面展示排序 `:697`

**唯独没有第五样：消费点。** `crates/ll-game/**` 里对它的唯一一次提及是一句注释（`app.rs:515`「与 `GameKey::Screenshot`/`GameKey::Menu` 同一类键」），**没有任何 `was_just_pressed(GameKey::Menu)` 分支**。按下它，什么都不会发生。

**这处缺口恰好落在字段接线门禁扫不到的地方**：`check_field_consumers.py` 扫的是「内容表结构体字段与枚举变体在决策层有没有读取」，`GameKey` 不是内容表。**门禁的覆盖边界就是下一处缺口最可能出现的地方**——这是 ADR 0022 那条判据的又一次应用。

### 设置界面：数据侧全就绪，缺的纯粹是那块屏幕

- `GameConfig`（`crates/ll-platform/src/config.rs:54`）三个字段全部已被真实消费：`language` → `ll_i18n::Catalog`、`vsync`/`scale_filter` → 渲染管线（提交 `5c59a39`）；
- `KeyBindings::all`（`crates/ll-platform/src/keybind.rs:640`）的文档原文就写着「列出全部绑定，**供设置界面展示**、导出配置等只读场景使用」——为它准备的 API 早就写好了；
- 键位抽象层是专门为重绑定打的地基（`c6f7aa0`），模块文档 `:10-11` 写明「列进设置界面的必备项（P7），若不趁早把这张映射表抽成数据，等到做设置界面时就要去 `match` 分支里现场重构」。

**这是 P7 剩余工作里代价最低、可验收性最强的一项。** 它同时也是三、四、五、六共六个 `Intent` 变体拿到入口的地方（见 `p6-to-p7.md` 三节）。

### 建议的 P7 收尾批次

**一批做完三件：生成期 mod 集合修正 + 游戏内菜单 + 设置界面。** 三件都小、可独立验收，且菜单是设置界面的前置。做完这一批，P7 除「势力播种」外全部交付。

**势力播种建议显式推迟到 P9**（它牵连组织关系矩阵进 `WorldState`、`hash()`、`remap`、存档版本，与 P9「智能体经济与人口」高度重叠），**但必须写进规格 §15 P7 行**，不能沉默地留着——那正是气候条带那条债务连续失效五轮的做法（六节）。

---

## 二、A. 生成期 mod 集合会被每一次存档静默覆盖（**已修**）

**这是本次清算发现的、此前无任何文档记录的真实缺陷，也是 P7 收尾里唯一一条会污染玩家数据的。**

> **状态更新**：已在 `wt-genmodset` 批次修复。本节以下的现象描述与论证**如实保留**（它仍然是理解这条缺陷形状的最完整记录），修法与验证见本节末尾「已落地的修法」。

### 现象

`crates/ll-game/src/save.rs:69`：

```rust
let generation = GenerationModSet::capture(&content.registry, &content.manifests);
```

`save_game` **每一次存档都从「当前会话已装载的内容」重新算一遍 `generation_mods`**，而不是把读档时从存档头里读到的那一份原样带回去写出。`load_game`（同文件 `:100`）也不把存档头的 `generation_mods` 交给任何人保管——它只是把内容表转手给 `load_full`。

### 为什么它走得通，不是理论风险

1. 存档头的两道 mod 集合检查——`check_mod_set`（`crates/ll-content/src/load_error.rs:396`，看「在不在、版本号是否相等」）与 `check_mod_content`（`:322`，看「内容哈希是否一致」）——**都只遍历 `generation_mods` 这一份名单**；
2. 玩家中途**新装**一个 mod：它不在 `generation_mods` 里，两道检查都不会看它一眼，读档放行；
3. 再存一次档：`GenerationModSet::capture` 把新装的 mod 一并算进 `generation_mods`——**这个世界的「生成期 mod 集合」从此永久多了一条它生成时并不存在的记录，而原始记录已经被覆盖掉了，追不回来。**

### 为什么这条正是当初专门要防的

`p4-to-p5.md` 二节原话：

> 只存当前的话，玩家中途装个 mod，那个世界就再也复现不出来——**种子分享、缺陷复现、回归测试全部失效**。

类型层的防护做得很到位：`GenerationModSet` 与 `CurrentModSet` 编译期不可互换（裁定 P4-3，`mod_set` 模块文档带 `compile_fail` 示例），`save.rs:33` 甚至为此单独写了一份三字段搬运而不强行复用类型受限的函数。**编译期挡住了「拿错类型」，挡不住「拿对类型、装错内容」**——这是一处类型系统天然覆盖不到的缺口，只能靠数据流本身正确。

### 修法与代价

**现在修接近零成本**：仓库里没有任何第三方 mod，也没有玩家存档。改法是让 `GameWorld`（或它的持有者）保管读档时拿到的 `SaveHeader::generation_mods`，`save_game` 优先写出那一份，只有「本次会话是新建世界」时才调 `GenerationModSet::capture`。

**拖着的代价是不可逆的**：一旦有玩家装了第三方 mod 并存过档，被污染的 `generation_mods` 就没有任何地方能追回原始值。

**P8 为什么会撞上它**：P8 的随从/指令/派工都会往存档里加数据，存档格式的每一次改动都会让「这个存档到底是用哪套 mod 生成的」这个问题更要紧；而且 P8 是第一个大概率会引入第三方 mod 内容（自定义随从行为、自定义据点工种）的阶段。

### 已落地的修法（`wt-genmodset` 批次）

**形状**：世界身份收拢成单一真相源，绑定一次、此后只被搬运。

- `ll_content::world_identity::WorldIdentity` 从三要素扩成**四要素**（种子 + 尺寸 + **地形形态** + 生成期 mod 集合），字段全部私有，只有两个构造器：`bind`（建新世界那一刻）与 `restore_from_header`（读档时把存档头那一份原样接回来）。
- `ll_game::world::GameWorld` 新增 `identity` 字段保管它；`build_new_world` 是 `GenerationModSet::capture` 在生产代码里的**唯一**调用点。
- `ll_game::save::load_game` 改为返回 `LoadedGame`（`Playable` 那一支额外带回身份）——`LoadOutcome::Playable` 只装 `WorldState`，而生成期集合不在主体里，不接出来就在读档那一刻丢了。
- `save_game` 直接写 `game_world.identity`，不再 `capture`。

**类型层修补（这条缺陷的教训是「编译期挡住了拿错类型，挡不住装错内容」）**：`SaveHeader` 的四个身份字段改成 `pub(crate)`，crate 外唯一的写出入口是 `SaveHeader::new(&WorldIdentity, SaveHeaderMeta)`。`ll-game` 因此**写不出**「现场 `capture` 一份塞进头部」这行代码——不是不该写，是编译不过。剩余的口子如实说明：`WorldIdentity::bind` 仍然公开（建档要用），所以「伪造一整份身份」在编译期仍可写，只是它再也不可能是顺手写错的样子。

**顺带收拢的第四个要素**：地形形态参数一并进了 `WorldIdentity` 与存档头（`SaveHeader.terrain_shape`）。头部新增的是一个 `Option` 键、存档主体字节布局未动，**老存档照常读得开、没有 schema 升级**。

**两道校验的语义未改**：「玩家多装了生成期名单之外的 mod」仍然放行（决策二只覆盖「缺 mod」与「版本对不上」两档）。本批次只保证放行之后名单不再被污染，语义由 `crates/ll-content/tests/e2e_save_cycle.rs` 的 `玩家中途多装一个生成期名单之外的mod时读档照常放行` 钉死。

---

## 三、B. P8 的第一件事已经被一次架构撤退白送了一半——但送的是另一半

规格 §15 P8 行：「随从与行为树、指令系统、据点派工」。

### 已落地：行为树引擎

Steel 脚本系统整体拆除时（ADR 0028），行为树从 `mods/example_mod/behavior.scm` 搬进了引擎 Rust（提交 `3793e4b`），随后一路接线到位：

- `crates/ll-sim/src/behavior.rs`：`BehaviorTreeSource` trait 与 `behavior_ai_intent` → `TurnEngine::advance_ai` 这条接线；
- `crates/ll-mod/src/native_behavior.rs`：**三棵硬编码的 Rust 树**——哥布林、卫兵、平民；
- `crates/ll-mod/src/behavior_binding.rs`：`ClassBehaviorBindings`，按职业选树（提交 `e12c39e`「按职业选行为树，农夫不再朝玩家走」）；
- 约束由签名保证而非纪律：`decide(&WorldState)` 物理上拿不到 `&mut`（C1/ADR 0023）；随机唯一来源是 `DetRng::for_entity` 派生的流（C3）；找目标走 `ll_sim::ai_query` 的固定顺序（C5）。`ll-mod` 的依赖里根本没有 `rand`。

**这半件事做得很扎实。**

### 送的是「敌人 AI」那一半，不是「随从」那一半

三棵树全部是**敌人/中立 NPC** 的树。全仓库 `Companion`/`Follower` 零命中；「随从」二字只出现在 9 处前瞻性注释里（`crates/ll-world/src/entity/stats.rs:30`「魅力：招募随从、交易议价、随从士气」等）。**招募、队伍、士气、跟随距离、随从装备归属——一样都不存在。**

### 一条已经被裁定、P8 必须知道的事：行为树不做数据驱动

`crates/ll-mod/src/native_behavior.rs:9-15` 记录了项目所有者的裁定：

> 「先做 json5 就好了，其他搬迁回系统内」——**第三方 mod 的行为扩展能力是一个明确推迟的决定**，不是一个还没想清楚的问题。因此本模块**没有**节点注册表、没有按名字查表的原语、没有「树结构写成数据文件」那一层：为一个已经决定不做的东西预留扩展点，正是这个代码库反复踩过的「声明了但从没接线」。

**P8 会正面撞上这条裁定**：指令系统（玩家命令随从做事）与据点派工（把工种指派给 NPC）**天然需要「行为随数据变化」**——一个随从今天砍树、明天守门，如果每种行为都要新增一棵硬编码 Rust 树，树的数量会随「工种 × 状态」组合爆炸。

**这不是说裁定错了**，而是说 P8 开工前必须重新问一次所有者：指令与派工要不要行为树数据化？三个可能的答案各有代价：

- **(a) 仍不数据化**：指令/派工做成树内部的一个「当前任务」状态字段，树的数量不变但每棵树变复杂。最省，表达力最弱。
- **(b) 只对本体数据化**：树结构进 `mods/lostland/*.json5`，但不开放给第三方 mod。中等成本，与所有者原裁定（推迟的是「第三方扩展能力」，不是「数据化」本身）并不冲突。
- **(c) 完整数据化 + 第三方开放**：推翻原裁定。

**别在不裁定的情况下自己选 (a)**——那是最容易在实现中途悄悄发生的选择，也是最难回退的。

---

## 四、C. P8 的三件事全部踩在一套没有生产者的归属体系上

这是 P8 规划者最该先读的一节。

### 现状：归属体系是一具空壳

| 类型 | 存在？ | 生产者 |
|---|---|---|
| `Affiliation` / `AffiliationKind` | ✅（`crates/ll-world/src/entity/affiliation.rs`，五变体） | **无**——两条唯一生产路径 `crates/ll-game/src/world.rs:544` 与 `crates/ll-mod/src/roster.rs:802` 都写死 `affiliations: Vec::new()` |
| `Agent::wallet` | ✅（`agent.rs:122`） | **无**——同两处写死 `wallet: 0` |
| `OrgInstance` | ✅（`crates/ll-world/src/entity/org.rs:21`） | **无**——只有 `:53`/`:73` 两处测试夹具 |
| `ThinPopulation` 薄层 | ✅ | **无**——`spawn` 的全部调用点都在测试里，`WorldState.population` 生产中恒为空 |
| `ll_sim::ai_query::is_hostile` | ✅ | 有链路（→ `nearest_hostile` → 哥布林行为树），但因 `affiliations` 恒空，**恒走「a 无势力 → 对谁都敌对」那条退化分支** |

### 三件事各自怎么踩上去

- **随从**：「这个 NPC 是不是我的人」正是一条归属关系。没有 `Affiliation` 生产者，随从身份只能另开一个平行字段——**那会是第二套「谁属于谁」的表达**，与本仓库反复裁定的「不建第三处真相源」直接冲突（`AffiliationKind::Profession` 与 `Agent.profession` 那次已经付过一次代价，见五节）。
- **指令系统**：命令的合法性判据是「我有没有资格命令他」——同样是归属关系。
- **据点派工**：派工的对象是据点里的 NPC，而据点居民今天是**纯派生**的（`ll_mod::roster` 按资源画像现算职业与种族，只有被物化过的据点才进 `WorldState::actors`）。**「派工」是一次真实偏差写入**，它要求被派工的那个 NPC 已经被物化并持久化——这条边界（`WorldState::materialized_settlements`）现在是存在的，但派工数据本身要不要进存档、怎么进，没有任何设计。

### 已有的现成路线图，不要重新发明

`2026-08-26 社会/种族/冲突三份文档落地状态复核` 六节已经给出「`CultureDef` 完整落地 + 关系派生基线」的三批拆分与前置清单，其中**第三批就是「世界生成期造 `OrgInstance`，给 `Agent`/名册真的填 `Affiliation`」**——那正是让整套归属体系第一次有生产者的那一步。第一批（文化定义 + 敌对表）**已经在提交 `4aec07e` 落地**，第二批（怪物种族与部落）与第三批未动。

**P8 的规划者应当把「第三批」当成自己的前置**，而不是在 P8 内部另起炉灶。

---

## 五、D. 行为树只能产出五种意图——指令与派工需要的表达力还不存在

`crates/ll-mod/src/native_behavior.rs` 全文只出现五个 `Intent::` 变体：

```
Move    Wait    Attack    UseSkill    Inspect
```

指令系统与据点派工需要的是「去 X 处做 Y 事」这类**带目标的复合行为**：去伐木场砍树（`Move` + `Craft`）、去仓库存放（`Move` + `Drop`/`Place`）、守住这个路口（`Move` + 持续 `Wait` + 条件 `Attack`）。

**结算侧的原料都在**——`Intent::Craft`（`08cdeb0`，含 `station_becomes` 那条「砍树复用制作管线」的设计）、`Place`、`Drop`、`Loot` 全部落地且有玩家按键生产者。**缺的是行为树能不能产出它们**，而这直接回到三节末尾那个待裁定问题：行为树要不要数据化。

**另一条独立的前置**：全仓库没有寻路到远处目标的路径规划——`ai_query` 提供的是「最近的敌对目标」，`Move` 是单步方向。「去伐木场」需要多步路径。**这是 P8 一个没有被规格 P8 行提到、但绕不过去的前置**，如实记在这里。

---

## 六、E. 两条转发链：其中一条这一轮必须停止转发

### 气候条带：第五轮提醒失效，已登记为流程问题

规格 §7.1/决策 23「气候为周期性条带：两条赤道 + 两条极圈」，历轮交接（`p2-to-p3.md` → `p3-to-p4.md` → `p4-to-p5.md` → `p5-to-p6.md`）四次指定归属 **P7**。

**P7 的世界生成器已经完整落地，气候条带仍是零实现**（`grep -rn "climate\|气候\|zonal_band" crates/` 只有三处无关命中；`SpaceProfileTable::base_temperature` 按空间层取温度，与坐标无关）。P7 期间甚至落地了天气系统（`d5215f1`）与温度系统（`c12c04f`），**两者都是纯派生且都不看纬度**——离气候条带只差「按 j 坐标分带」这一步，却没走。

`p5-to-p6.md` 一、3 节立下的规矩是：「若 P7 计划作者仍然不认领，第五轮提醒失效的记录本身就该单独成为一次流程问题登记」。照办，登记见 `p6-to-p7.md` 四节「流程问题登记 #1」。

**给 P7 收尾者的要求（不是建议）**：P7 收尾时**必须显式二选一**——排进 P7 剩余批次，或在规格 §15 P7 行写明「本阶段不认领，推迟到 P<N>，理由是 X」。**第三种选择（继续沉默）已经用掉五次了。**

**给 P8 的要求**：如果 P7 收尾时仍然没有做这个二选一，**P8 不要接手转发**——第六轮转发只会重复前五轮的结果。直接把它作为一次流程问题上报给项目所有者。

### 光照透过率：第六轮转发，认领点在 P9

规格 §7.3「瓦片地形携带光照透过率等属性」，历轮建议归属 **P9**（与 `RaceDef.darkvision_floor`/`sight_radius_at` 同批更省一次返工）。**仍是零实现**（`grep -rn "透光率\|透过率\|light_transmit\|transmittance" crates/` 零命中）。

P8 只是转发方，**这条转发链继续往下传**，P9 是真正的认领点。

---

## 七、F. 已知边界（照实写，不要粉饰）

- **P7 至今零份验收 demo。** 规格 §15 开头的硬性要求「每阶段必须交付可独立运行的 `examples/` 验收 demo」在 P6 与 P7 连续两个阶段没有兑现。详见 `p6-to-p7.md` 七节，那里把它标为一处需要裁定的规格—实现分叉。
- **P7 的 UI 至今没有一帧被人眼在真实窗口里确认过的自动化证据。** 这是 ADR 0025（demo 交互验收禁止合成键盘事件）之后的既有代价，`p5-to-p6.md` 八节已记录。主工作树里躺着几张手工截图（`hud_p7_screenshot.png` 等，未纳入版本控制），是仅有的视觉证据。**L7 视觉回归仍然只有「生成基准图、肉眼比对」，没有自动比对**——`p4-to-p5.md` 就指出「`ll-ui` 落地后这一层成本很低」，P5/P6/P7 三个阶段都没有动它。**完整控件库现在真的落地了，这条建议第一次完全适用，P8 之前是它最后一个便宜的时间点。**
- **`AddGroundItem` 的占位闸门只覆盖 `Drop`/`Place` 两条路径**（`crates/ll-sim/src/resolve.rs:2674-2681`，已显式记录为已知边界）。**P8 会放大它**：据点派工一旦落地，NPC 会在据点里大量产出地面物品（伐木、采集、搬运），尸体与产出物摞在炉子/工作台那一格上的概率会显著上升。补齐它需要先裁定「放不下时东西去哪」。
- **任务击杀进度仍读裸 `Agent::race`，其余三条击杀路径已改读 `creature_kind`**（`crates/ll-sim/src/resolve.rs:1603` vs `:1571`/`:1704`/`:1813`）。详见 `p6-to-p7.md` 二、2 节。**P8 会放大它**：随从击杀、派工产生的击杀都会走这条链路。
- **`Owner`（物品归属）仍是零实现，但拒绝它的三个理由今天只剩一个。** 详见 `p6-to-p7.md` 二、1 节。**P8 会正面撞上它**：随从装备归属是 `Owner` 设计文档列出的三个消费场景之一。
- **`crates/ll-sim/src/behavior.rs:5` 的模块文档仍写着「行为树写成 Steel `.scm`」**，而 Steel 已整体拆除、树已搬进 Rust。P8 的规划者读这份模块文档时会读到一个不存在的世界。**属于 `crates/**`，本次只读未改。**
- **本文档补写时，仍有三个代理在并发修改 `crates/**`**（`ll-render`/`ll-ui`/`assets` 一路、`ll-world/src/generate.rs` 与存档一路、`chronicle.rs`/`settlement.rs` 一路）。本文档全部结论以 `ed1584f` 为准，读者在更晚的提交上复核请重跑 grep，不要相信本文档的行号。

---

## 相关文档

- [P6/P7/P8 阶段清算](../audit/2026-08-26-phase-reckoning-p6-p8.md) — 本文档全部结论的证据来源，含当前阶段判定与下一批入口
- [P6 → P7 交接清单](p6-to-p7.md) — P6 遗留的三处缺口与「P7 至今有没有碰」的逐条标注
- [P4 → P5 交接清单](p4-to-p5.md) — 二节「生成期与当前两组 mod 集合必须分开记录」，本文档二节的缺陷正是它要防的那件事
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) — §15 P7/P8 两行本次已补落地状态标注
- [2026-08-26 社会/种族/冲突三份文档落地状态复核](../audit/2026-08-26-society-race-conflicts-reverification.md) — 六节的三批拆分，本文档四节直接引用为 P8 的前置路线图
- [社会系统：归属、文化、聚落与地图结构](../design/society-and-affiliation.md) — 四节归属体系的设计源头，文末两段复核更正记录了它与今天代码的差距
- [身份与 ID 空间](../design/identity-and-ids.md) — `OrgInstance` 的形状设计，至今零生产构造点
- [据点、结构物与 NPC 生成](../design/settlements-structures-and-npc-spawning.md) — 据点派工的直接上游；十二节六条待所有者裁决项与本文档三、五节重叠
- [ADR 0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 覆盖不全的门禁等于没有门禁，一节末尾 `GameKey::Menu` 落在门禁边界外是同一条判据
- [ADR 0025](../decisions/0025-demo-interaction-verification-forbids-sendkeys.md) — 七节第二条已知边界的由来
- [ADR 0028](../decisions/0028-steel-engine-construction-memory-corruption.md) — Steel 拆除，三节行为树搬家的直接原因
- [覆盖率与缺失测试层](../../docs/qa/04-覆盖率与缺失测试层.md) — 七节 L7 视觉回归空白的定位依据
