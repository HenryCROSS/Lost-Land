# P6 / P7 / P8 阶段清算：做了什么、跳过了什么、现在在哪一个 P

**清算日期**：2026-08-26
**基线**：`wt-phase` 分支，起点 `main` HEAD `ed1584f`（`Merge branch 'wt-input'`）。
**方法**：只读 `crates/**`、`mods/**`、`scripts/**` 与提交历史。每一条断言都给文件与行号或提交号，行号随后续提交漂移，读者在更晚的提交上复核请重跑 grep。
**并发声明**：清算期间另有三个代理在改 `crates/**`（`wt-render`、`wt-worldgen`、`wt-conquest`）。**本文档的一切结论以已提交树 `ed1584f` 为准**，未读任何未提交的半成品；本文档本身只改 `knowledge/**` 与 `docs/superpowers/**`，不动一行代码。

---

## 零、这份文档为什么存在

本仓库有一条被反复记录的失败模式，叫「**声明了但从没接线**」——类型建好、测试全绿、没有任何生产调用点。它已经被记录了六次（`p5-to-p6.md` 六节），并且最终换来一道机器门禁（`scripts/ci/check_field_consumers.py`，见下文四节）。

**阶段记录这件事现在是它的反面：做了但从没记录。**

核实结果：

| 载体 | 最后一份 | 之后发生的事 |
|---|---|---|
| `docs/superpowers/plans/` 实施计划 | `2026-08-19-p5-gameplay-systems.md`、`2026-08-19-p5-save-format-and-identity.md` | **零份新计划** |
| `knowledge/handoff/` 交接清单 | `p5-to-p6.md` | **零份新交接** |
| `.superpowers/sdd/` 批次账本 | 未纳入版本控制（`git ls-files` 下无任何 `.superpowers/sdd/` 条目） | 全部丢失 |
| 提交 | `b343a60`（P5 收尾） | **228 个提交**（`git log --oneline b343a60..ed1584f | wc -l`） |

228 个提交里，只有 4 个在标题或正文里带阶段编号（`P6 第四批`/`第五批`/`第六批`、`P7 只读观测 HUD`）。其余 224 个提交没有任何阶段归属线索——**P6 全部完成、P7 大半完成、P8 完成了三分之一，全程没有一份阶段记录。**

这份文档补的是记录，不是工作。

---

## 一、P5 冻结之后，工作区实际发生了什么

`p5-to-p6.md` 冻结时记录的基线是「10 个 crate、802 个测试、六道 CI 门禁、26 份 ADR」。今天：

| 指标 | P5 冻结时 | 现在 | 差 |
|---|---|---|---|
| 工作区 crate | 10 | **11** | `ll-script` 删除、`ll-i18n` 与 `ll-game` 新建（净 +1） |
| `#[test]` 计数 | 802（实际执行数） | **2257**（源码内 `#[test]` 属性计数，口径不同，不可直接相减） | — |
| CI 门禁 | 6 | **9** | 新增：字段接线检查、硬编码 i18n 字符串扫描、Markdown 死链检查 |
| ADR | 26 | **29** | 新增 0027/0028/0029 |

**其中一件事的量级远超阶段划分本身**：`ll-script` crate 与整个 Steel 脚本层被**整体拆除**（提交 `52975fc` `refactor: 删除 ll-script crate 与 steel-core 依赖`，起因见 ADR 0028「Steel 引擎构造期内存破坏」）。玩法层内容改走 `mods/<id>/*.json5`，行为树搬进引擎 Rust（`3793e4b`）。这不属于 P6/P7/P8 任何一行，是一次跨阶段的架构撤退——**规格 §10 整章、以及十余份设计文档里所有以「通过 Steel 脚本定义」「`register-*`」为载体的段落，都是在这次拆除中失效的**，只是载体失效、结论多数不失效（各文档的复核更正段已陆续记录）。

---

## 二、P6「物品与装备」逐条核实

规格 §15 P6 行点名七件事。逐条：

| # | 规格原文 | 落地？ | 代码证据 |
|---|---|---|---|
| 1 | `ItemDef`/`ItemStack` 分离、堆叠与合并规则 | **已落地** | `ItemStack` 在 `crates/ll-world/src/item.rs:84`；`can_merge`/`merge_stacks`/`split_stack` 同文件 `:754`/`:786`/`:821`；`ItemDef` 与装载器在 `crates/ll-mod/src/item.rs`（1347 行）。提交 `d734168`（P6 第一批） |
| 2 | 22 槽位与占位掩码 | **已落地，22 个一个不少** | `crates/ll-world/src/item.rs:305-347` 逐个 `EquipSlot` 常量，`:352` `ENGINE_SLOT_COUNT: u8 = 22`；`SlotMask` 在 `:464`，`MOD_RESERVED_BITS = 32 - 22` 给 mod 留了 10 位（`:472`）。提交 `b92f587`（第三批） |
| 3 | 背包、地面物品与 `dropped_at` 老化 | **已落地** | `GroundItemStack` 在 `crates/ll-world/src/item.rs:200`（含 `dropped_at: Tick`）；老化回收 `cleanup_aged_ground_items`；背包是 `Agent::inventory`。提交 `c76f8f1`（第二批）、`e5f94f6`（生命周期接线） |
| 4a | 归属（`Owner`） | **未落地，但属于显式拒绝，不是沉默跳过** | `crates/ll-world/src/item.rs:31-57` 整段模块文档「`Owner` 本批次仍然不落地——没有真正的消费者」，逐条核实了偷窃/商店/NPC 私产三个消费场景一个都不存在，并把最小形状写进文档供未来批次照抄。后续另补了完整设计（`knowledge/design/ownership-and-crime-detection.md`，提交 `4eca643`），**仍无实现** |
| 4b | 耐久 | **已落地，且超出规格范围** | 提交 `6bb0cb3`（第五批，`Intent::Use` 与耐久消耗）、`600f458`（第六批，耐久收窄到武器）、`fbff50d`（扩到护甲/衣物/工具，「挨打」与「使用」两条通道）、`3037d5e`（判据从槽位改成物品标签 + 标签注册表）。`WearChannels` 在 `crates/ll-world/src/item.rs:551` |
| 5 | 接进战斗结算（装备属性 → `DerivedStats`） | **已落地** | `derive_stats`/`derive_stats_at` 在 `crates/ll-sim/src/resolve.rs:399`/`:442`；耐久归零的装备不贡献加成（`:483`，`if stack.durability == Some(0)`）。提交 `754b04c`（第四批） |
| 6 | `Intent::PickUp`/`Drop`/`Use` 及对应 `Effect` | **已落地，且全部有真实按键生产者** | `crates/ll-sim/src/intent.rs:196`/`:259`/`:315`；键位生产者在 `crates/ll-platform/src/keybind.rs` 与 `crates/ll-game/src/player_action.rs`。**同批还多出四个规格没要求的**：`Place`（`:240`）、`Equip`（`:278`）、`Unequip`（`:296`）、`Loot`（`:341`）。提交 `8f8d1a9`（把六个物品意图接到真实键位） |
| 7 | 走注册表——每种内容重做一次「本体即 Mod」检验 | **已落地** | 本体二十四件物品与九条配方在 `mods/lostland/items.json5`/`crafting.json5`（提交 `2a36fcf`）；装载后跨表引用校验 pass 在提交 `f7a4203`——这正是 `p5-to-p6.md` 三节点名的「跨表引用校验缺一个统一阶段」，**P6 认领并还清了** |

### P6 结论

**P6 已完成。** 七项里六项完整落地，第七项（`Owner`）是**按项目纪律显式拒绝**的——模块文档写明了拒绝理由（三个消费场景全不存在）、写明了最小形状、写明了落地时必须同批改 `can_merge`。这与 P5 对待探索记忆那种「沉默跳过」是两回事，是本仓库文档纪律正确工作的一次范例。

**P6 还顺手还清了 `p5-to-p6.md` 列出的全部三条真实缺口**：

| `p5-to-p6.md` 缺口 | 现状 | 证据 |
|---|---|---|
| 二、A、1 `Interior.origin` 未落地 | **已还清** | `crates/ll-world/src/interior.rs:169` `pub origin: Option<GeneratorParams>`；提交 `1b68266`（schema 升到 v2 + 真实迁移函数） |
| 二、A、2 技能击杀不产出 `Effect::Kill` | **已还清** | 提交 `135a4c0`（`resolve_use_skill` 补致死判定，技能击杀能推进任务进度） |
| 二、A、3 缺敌人类型注册表 | **半还清** | `Agent::creature_kind: Option<ContentIndex>` 已存在（`crates/ll-world/src/entity/agent.rs:475`），构造时回退到 `race`（`:838`）。但 `QuestKillRule::target_kind` 仍是裸 `ContentIndex`（`crates/ll-sim/src/quest.rs:83`），模块文档 `:30` 的「如实记录简化」一节原样保留 |
| 三、B 跨表引用校验缺统一阶段 | **已还清** | 提交 `f7a4203` |

---

## 三、P7「UI 层 + 世界生成器」逐条核实

规格 §15 P7 行点名八件事（四件 UI、四件世界生成）。逐条：

### UI 侧

| # | 规格原文 | 落地？ | 代码证据 |
|---|---|---|---|
| 1 | 像素 UI 控件库（九宫格边框） | **已落地** | `NineSliceSkin` 在 `crates/ll-ui/src/widget/skin.rs`，生产消费点 `crates/ll-game/src/app.rs:198`/`:236` |
| 2 | 焦点导航 | **已落地** | `crates/ll-ui/src/widget/focus.rs`，配套 `button.rs`/`hit_test.rs`/`state.rs`。提交 `f46c363`（UI 交互层：光标/鼠标输入、命中测试、按钮控件、焦点导航） |
| 3 | **游戏内菜单** | **未落地，且是一处「声明了但没接线」** | `GameKey::Menu` 已定义（`crates/ll-platform/src/input.rs:48`）、已进键位表（`:126`）、已有 i18n 显示名键（`:203`）、已进排序（`:697`）；但 `crates/ll-game/**` 里**唯一一次提到它是一句注释**（`app.rs:515`「与 `GameKey::Screenshot`/`GameKey::Menu` 同一类键」），**没有任何 `was_just_pressed(GameKey::Menu)` 分支**——按下这个键什么都不会发生 |
| 4 | **设置界面** | **未落地** | 全仓库对「设置界面」的引用全部是「未来会有」式的前瞻注释（`crates/ll-platform/src/keybind.rs:10-11`、`crates/ll-game/src/app.rs:181`、`crates/ll-ui/src/lib.rs:17`）。数据侧已经就绪（`GameConfig`/`DisplayConfig`/`KeyBindings` 在 `crates/ll-platform/src/config.rs:54`，`vsync`/`scale_filter`/`language` 三项可配置且已被消费），**缺的只是那块屏幕** |
| 5 | i18n | **已落地并接线** | `ll-i18n` crate（604 行）、`assets/locales/{en,zh-CN}.ftl`、消费者在 `ll-game`/`ll-platform`/`ll-mod`；配套门禁 `scripts/ci/check_i18n_strings.py`（warn 模式）。提交 `3331b23` |

**已落地但超出规格 P7 行文字的**：整套只读观测 HUD（状态栏/角色面板/背包/装备栏/世界地图/动作菜单，`crates/ll-ui/src/hud/`，提交 `b763738`）。规格 P7 行没有点名 HUD，它是「完整控件库」这个笼统说法下自然长出来的。

### 世界生成侧

| # | 规格原文 | 落地？ | 代码证据 |
|---|---|---|---|
| 6 | 世界生成器落地于本阶段，建在两级坐标系之上 | **已落地** | `crates/ll-world/src/chronicle.rs`（2657 行，12 纪元历史推演）、`settlement.rs`（1239 行）、`resource.rs`（1038 行）、`history.rs`、`culture.rs`、`generate.rs`。提交 `393177c`（世界历史生成器，据点与废墟真的写进地形）、`a953094`（据点横跨区块）、`b6bb04d`（资源点系统）、`bc2fc81`（据点名册派生 + 物化）、`4aec07e`（文化定义 + 关系派生基线 + 敌对战争） |
| 7 | `Space`/`SpaceProfile` 注册走与 `TerrainDef` 相同的内容注册表模式 | **已落地** | `crates/ll-world/src/space_profile.rs`（1034 行，`SpaceProfileTable`/`materialize_base_space_profiles`）+ `crates/ll-mod/src/base_space_profile.rs`（本体走同一条 `Registry::intern`）。提交 `b4e2820`（空间层属性补上注册通道，第十六类玩法层内容接进 mod API） |
| 8 | 聚落/势力播种工作在区块粒度 | **一半落地** | **聚落**：是的，`SettlementSite` 选址与铺设按区块粒度惰性铺设（`a953094`）。**势力**：不是——`OrgInstance`（`crates/ll-world/src/entity/org.rs:21`）**全仓库零生产构造点**，只有 `:53`/`:73` 两处测试夹具和 `crates/ll-world/src/entity.rs:43` 一行 `pub use`。世界生成期不造任何组织实例 |
| 9 | 「生成期 mod 集合」的真实绑定时机也要在本阶段真正落地 | **接线了，但接错了一半——见下** | 见下一小节 |

### ⚠ 第 9 项：接上了，但生成期 mod 集合会被每一次存档静默覆盖

`crates/ll-game/src/save.rs:69`：

```rust
let generation = GenerationModSet::capture(&content.registry, &content.manifests);
```

`save_game` **每一次存档都从「当前会话已装载的内容」重新算一遍 `generation_mods`**，而不是把读档时从存档头里读到的那一份原样带回去写出。`load_game`（同文件 `:100`）也**不把存档头的 `generation_mods` 交给 `GameWorld` 保管**，它只把内容表转手给 `load_full`。

这条路径是真的走得通的，不是理论风险：

1. 存档头两个 mod 集合检查（`crates/ll-content/src/load_error.rs` 的 `check_mod_set:396` 与 `check_mod_content:322`）**都只遍历 `generation_mods` 这一份名单**；
2. 玩家中途**新装**一个 mod，这个 mod 不在 `generation_mods` 里，两条检查都不会看它一眼，读档放行；
3. 再存一次档，`GenerationModSet::capture` 把新装的 mod 一并算进 `generation_mods`——**这个世界的「生成期 mod 集合」从此永久多了一条它生成时并不存在的记录。**

这正是 `p4-to-p5.md` 二节点名要防的那件事的原话：「只存当前的话，玩家中途装个 mod，那个世界就再也复现不出来——**种子分享、缺陷复现、回归测试全部失效**」。类型层的区分（`GenerationModSet` 与 `CurrentModSet` 编译期不可互换，裁定 P4-3）做得很到位，**但值的来源接错了**：编译期挡住了「拿错类型」，挡不住「拿对类型、装错内容」。

**这是本次清算发现的、此前无任何文档记录的真实缺陷。** 已写进 `p7-to-p8.md`，不在本次修（本次只改文档）。

### P7 结论

**P7 未完成。** 世界生成侧基本完整（势力播种除外），UI 侧五项里三项落地、**两项规格逐字点名的交付物（游戏内菜单、设置界面）零实现**，且其中一项（菜单）已经是一处标准的「声明了但没接线」。加上第 9 项接错的生成期 mod 集合，P7 有三项待办。

---

## 四、P8「随从与行为树、指令系统、据点派工」逐条核实

| # | 规格原文 | 落地？ | 代码证据 |
|---|---|---|---|
| 1a | 行为树 | **已落地** | `crates/ll-sim/src/behavior.rs`、`crates/ll-mod/src/native_behavior.rs`、`crates/ll-mod/src/behavior_binding.rs`（按职业选树）。提交 `3793e4b`（从 Steel 搬进 Rust）、`5862dbe`（真的经回合引擎驱动 AI）、`b146b0c`（AI 真的能做出决策并用技能打人）、`e12c39e`（按职业选行为树，农夫不再朝玩家走） |
| 1b | **随从** | **零实现** | 全仓库 `Companion`/`Follower` 零命中，「随从」二字只出现在 9 处前瞻性注释里（`crates/ll-world/src/entity/stats.rs:30`「魅力：招募随从」等）。行为树是给**敌人与 NPC** 做的，不是给随从 |
| 2 | **指令系统** | **零实现** | 无任何命令/指令类型。玩家对 NPC 的唯一交互是 `Intent::Inspect`/`Attack` |
| 3 | **据点派工** | **零实现** | 「派工」`WorkAssignment`/`JobAssign` 全仓库零命中。`ll_mod::roster` 派生的是「这座据点住着哪些职业的人」，不是「谁去干哪件活」——那是名册派生，不是派工 |

**行为树能产出的意图只有五种**：`Move`/`Wait`/`Attack`/`UseSkill`/`Inspect`（`crates/ll-mod/src/native_behavior.rs` 全文只出现这五个 `Intent::` 变体）。规格 P8 的另外两件事（指令、派工）都需要行为树能表达「去某处做某事」这类带目标的复合行为，当前的五个变体离那一步还很远。

### P8 结论

**P8 只完成了第一项的一半。** 行为树引擎完整落地并接进回合引擎，但它服务的是敌人/NPC，不是随从；随从、指令系统、据点派工三项**零实现**。

**这一半是被提前拉进来的**——行为树是被「Steel 拆除」这次架构撤退顺带搬进 Rust 的（`3793e4b`），不是 P8 主动开工的结果。

---

## 五、当前阶段判定

### 结论：**当前处于 P7，P7 未完成。**

依据三条，逐条给证据：

1. **P6 已完成**（本文档二节）。七项交付物六项落地，第七项显式拒绝并留下形状。`p5-to-p6.md` 的三条真实缺口全部还清或半还清。
2. **P7 未完成**（本文档三节）。规格 P7 行逐字点名的「游戏内菜单」与「设置界面」**零实现**，且「生成期 mod 集合的真实绑定时机」虽已接线但值的来源接错。世界生成侧的「势力播种」也未落地（`OrgInstance` 零生产构造点）。
3. **P8 不能算「开始了」**（本文档四节）。落地的那一半（行为树）是架构撤退的副产品，随从/指令/派工三项零实现——**已落地的部分不构成「P8 已开工」，构成的是「P8 的一件前置已经白拿了」。**

### P7 还差什么才算完成

按「先补上会越拖越贵的、再补交付物」排序：

| 优先级 | 待办 | 为什么是这个顺序 |
|---|---|---|
| **1** | 修生成期 mod 集合被存档覆盖（三节第 9 项） | **唯一一条会污染玩家数据的**。今天仓库里没有第三方 mod，现在修零成本；一旦有玩家装了第三方 mod 并存过档，被污染的 `generation_mods` **追不回来**——存档头里已经没有原始记录了 |
| **2** | 游戏内菜单（`GameKey::Menu` 接上） | 规格逐字点名的交付物，且已经是一处「声明了但没接线」实例。设置界面必须挂在菜单下，菜单是它的前置 |
| **3** | 设置界面 | 规格逐字点名的交付物。数据侧已完全就绪（`GameConfig` 三项可配置且已被消费 + `KeyBindings::all` 已为「列出全部绑定供设置界面展示」预留，`crates/ll-platform/src/keybind.rs:640`），**缺的纯粹是那块屏幕**——这是 P7 剩余工作里代价最低、可验收性最强的一项 |
| **4** | 世界生成期造 `OrgInstance`（势力播种） | 规格 P7 行「聚落/势力播种」的势力那一半。但它牵连组织关系矩阵进存档（见七节裁定项），**不是一批能做完的**，可以显式推迟到 P9（智能体经济与人口）而不必强塞进 P7——**只要显式推迟，别再沉默跳过** |

### 下一批该做什么

**建议下一批 = P7 收尾第一批：「生成期 mod 集合修正 + 游戏内菜单 + 设置界面」。**

理由：三件事都小、都可独立验收、且互为前置（菜单 → 设置界面）；生成期 mod 集合那条与前两件正交，可并行；做完这一批，P7 除「势力播种」外全部交付，可以正式收 P7、开 P8。

**并且：这一批开工前先写一份 `docs/superpowers/plans/` 的实施计划。** 这是本次清算最该带走的一条——228 个提交里没有一份计划，不是因为计划没用，是因为没人记得要写。

---

## 六、悬空债务逐条复核（含对既有说法的纠正）

以下逐条核实，**其中三条已经不成立**，一条与转述有出入。

| # | 债务 | 核实结论 | 证据 |
|---|---|---|---|
| 1 | `RaceDef.footprint` / `lifespan_years` 零消费者 | **仍然成立** | 两者都在 `scripts/ci/check_field_consumers.py:216`/`:217` 的豁免清单里，各带一条写明理由的豁免。门禁扫描 142 个目标字段，58 条未接线、58 条已豁免 |
| 2 | `OverviewCell::explored` 恒为 `true` | **❌ 已不成立——债务已还清** | `crates/ll-world/src/overview.rs:11`「`explored` 现在接的是真实的探索记忆」，`:86` 取 `exploration.is_explored(&layout, pos)`、`:205` 取 `zone_has_any_explored(zone)`；`ExplorationMemory` 在 `crates/ll-world/src/exploration.rs:131`（含 serde、`mark_explored`、`is_explored`）。提交 `c3a4236`（落地探索记忆）与 `0921b7b`（补上写入路径）。战争迷雾已接进本体渲染（`620b66c`），且有集成测试 `crates/ll-game/tests/fog_of_war.rs`。**这项被规格指名交给 P5 又被 P5 沉默跳过的债务，在 P7 期间被还清了，同样没有任何阶段记录。** |
| 3 | `WorldState::terrain_table` 仍标 `#[serde(skip)]` | **仍然成立** | `crates/ll-world/src/state.rs:302-303`。校验点 `assert_terrain_table_loaded` 存在，字段本身仍需调用方读档后显式重新灌入 |
| 4 | `AddGroundItem` 产出点不判「这格立没立着东西」 | **成立，且已被显式记录为已知边界** | `crates/ll-sim/src/resolve.rs:2674-2681` 整段「已知边界：只有 `Drop`/`Place` 两条路径认这道闸门」，点名尸体掉落（`append_corpse_drop`）与盲盒溢出，并写明补齐它需要先裁定「放不下时尸体去哪」 |
| 5 | NPC 行为树只产 Move/Wait/Attack/UseSkill/Inspect | **成立** | `crates/ll-mod/src/native_behavior.rs` 全文只出现这五个 `Intent::` 变体 |
| 6 | `OrgInstance` 未构造、`Affiliation` 恒为空 | **成立** | `OrgInstance` 只有 `crates/ll-world/src/entity/org.rs:53`/`:73` 两处测试构造；`affiliations: Vec::new()` 硬编码在两条唯一生产路径 `crates/ll-game/src/world.rs:544` 与 `crates/ll-mod/src/roster.rs:802`（`wallet: 0` 在紧邻的下一行） |
| 7 | 十余个 `Intent` 变体无输入产出者 | **⚠ 数目已过期：13 → 11** | `UseSkill` 与 `Inspect` **已有生产者**（NPC 行为树，`native_behavior.rs`），只是仍无**玩家输入**生产者。仍然零生产者的是 11 个：`OpenDoor`、`EnterSpace`、`ExitSpace`、`Rest`、`ToggleStealth`、`AllocateAttributePoint`、`LearnSkill`、`AbandonSubclass`、`Read`、`Experiment`、`Identify`。**其中 `AllocateAttributePoint`/`LearnSkill`/`AbandonSubclass` 三个是「升级加点/学技能/弃副职」，它们缺的正是 P7 未落地的那块屏幕**——不是玩法缺口，是 UI 缺口 |
| 8 | `assets/atlas/` 下只有 `placeholder.*` | **字面成立，但会误导** | `assets/atlas/` 确实只有 `placeholder.png`/`placeholder.json`/`README.md`。**但真实美术已经不住在那里了**：`assets/sprites/` 下有 22 张真实贴图（地形 9 张、英雄行走/待机 8 张、UI 5 张），mod 侧另有 2 张；图集改为**运行期打包**（`crates/ll-render/src/atlas_pack.rs`，提交 `be616f4` 资产 VFS），`placeholder.*` 是编译期烧死那个旧路径的残留。这属于 `wt-render` 批次的地盘，此处只做事实更正 |

### 新发现的、此前无记录的悬空项

| 项 | 说明 |
|---|---|
| **生成期 mod 集合被每次存档覆盖** | 三节第 9 项。已写进 `p7-to-p8.md` |
| **`GameKey::Menu` 无处理分支** | 三节第 3 项。定义、键位、i18n 键、排序四样齐全，唯独没有消费点 |
| **`crates/` 内三处模块文档仍在描述已被拆除的 Steel 时代** | `crates/ll-sim/src/behavior.rs:5`（「行为树写成 Steel `.scm`」）、`crates/ll-mod/src/race.rs`（多处 `register-race`）、`crates/ll-world/src/chronicle.rs`（「没有战争」而同文件里 `wage_wars` 在跑）。后两处 `2026-08-26-society-race-conflicts-reverification.md` 八节已记，`behavior.rs` 是本次新发现。**属于 `crates/**`，本次只读未改** |
| **`crates/ll-ui/src/lib.rs:17` 的范围声明已过期** | 原文「**仍然排除**：焦点导航、按钮、输入处理、设置界面/主菜单」——前三项已由提交 `f46c363` 落地，只剩后两项仍准确。**属于 `crates/**`，本次只读未改** |

---

## 七、需要项目所有者裁定的

按「拖着代价最高」排序。前两条是**必须在下一批开工前定**的。

1. **`Owner`（物品归属）：认领还是正式放弃？** 设计已完整（`../design/ownership-and-crime-detection.md`，五变体形状、盗窃判定挂载点、销赃计时、`Effect::TransferOwnership` 接口、犯罪记录走历史事件），实现零行。P6 拒绝它的理由（三个消费场景全不存在）**今天只剩两个成立**——卫兵盘查已经落地（`e81e03c`，视野内概率发起物品盘查），「目击」也已用 FOV 实现（`68c5d8f`），**盗窃判定的两个前置现在都在了**。请裁定：接进哪个阶段，还是明确标注「无限期推迟」。**不能继续挂着**——`Owner` 一旦加进 `ItemStack` 就必须同批改 `can_merge`，而背包/地面物品每多一批就更贵。

2. **`RaceDef.footprint` / `lifespan_years`：接线还是摘掉？** 两者已在「声明了、存了、哈希了、审计了、没有玩法后果」这个状态上停留了若干批次（`race-system.md` 复核更正三节已提过一次）。`footprint` 要接需要多格碰撞与寻路支持（是一整个批次），`lifespan_years` 要接需要老化/死亡判定（同理）。**这是第二次提出同一个问题**。

3. **「势力播种」归 P7 还是推到 P9？** 规格 P7 行写着「聚落/势力播种工作在区块粒度」，聚落那一半已落地，势力那一半（`OrgInstance` 构造 + 组织关系矩阵进 `WorldState`）没有。后者牵连存档格式改动（`WorldState`/`hash()`/`remap`/版本号），代价与 P9「智能体经济与人口」高度重叠。**建议明确推到 P9 并在规格 P7 行标注**，但这需要裁定，不能由清算文档代为决定。

4. **「职业」有几个真相源？** `Agent.profession`（已落地、有生产者与消费者）与曾经的 `AffiliationKind::Profession` 在描述同一件事。`Profession` 变体已在文化批次删除（见八节），**这个问题实际上已经被那次删除单方面解决了**——但解决方式是删掉未使用的那个，不是裁定。请确认这个结果是所要的：从属关系里不再有「职业」这一类，职业只由 `Agent.profession` 表达。**这是这个问题第四次出现，前三次都没等到裁定。**

5. **`birth_settlement` 与 `settlement` 是不是同一列？** 文化批次删掉薄层 `race` 列时，理由里写着「`settlement` 那一列本来就是『出生聚落』——`birth_settlement` 不需要新增，它一直在」（`crates/ll-world/src/entity/thin.rs:54`）。但 `race-system.md` 八节把两者设计成**不同**的东西：`birth_settlement` 终身不变、`settlement` 随迁徙变化。今天没有迁徙系统，两者确实是同一件事；**迁徙一旦落地，这个等号就断了，而那时薄层里已经有真实数据**。请裁定：现在就拆成两列（零成本），还是接受等号并在迁徙落地时付迁移代价。

6. **要不要给阶段划分补一条纪律？** 本次清算暴露的根因不是某个阶段做错了，是**没有任何机制强制「开一个 P 要写计划、收一个 P 要写交接」**。建议裁定一条最低限度的纪律（例如：`docs/superpowers/plans/` 下没有对应阶段的计划文档时，该阶段的第一个 feat 提交拒绝合并），或者明确「不设这条纪律，接受阶段记录靠自觉」。**两者都行，但要选一个**——现在的状态是纪律写在纸面上、机器不管、人也忘了。

---

## 八、顺带核实的设计文档过期情况

本次逐份扫了 `knowledge/design/` 下 36 份文档的「落地状态」行。**六份的开头断言与今天的代码直接矛盾**，其中三份本次已改（见文末「本次改了什么」），三份规模过大留给后续：

| 文档 | 开头写着 | 实际 |
|---|---|---|
| `item-system.md` | 「纯设计。`crates/` 中尚未找到 `ItemDef`、`ItemStack`、`Owner`、`ItemLocation`、`Quality`、`StatBonus` 等类型」 | **`ItemDef`/`ItemStack`/`StatBonus` 全部落地**（`Owner`/`Quality` 确未落地）。本次已改 |
| `equipment-slots.md` | 「纯设计。`crates/` 中尚未找到 `SlotMask` 或本文档定义的任何槽位类型」 | **`SlotMask`/`EquipSlot`/22 槽位全部落地**。本次已改 |
| `crafting-system.md` | 「纯设计，零实现。全代码库检索确认 `RecipeDef`/`RecipeTable`…」 | **制作系统已落地**（`08cdeb0`，两张内容表 + `Intent::Craft` + 副职闸门接进回合引擎）。本次已改 |
| `combat-three-axis.md` | 「纯设计，尚无代码——`resolve_attack` 仍是占位实现」 | 部分落地：伤害公式引擎（`b08ad7c`）、抗性/减伤链路（`fe2bbad`）、暴击（`3570649`）、对抗判定 MdN（`edc487a`）均已落地；三轴本身仍未完整。**规模大，留给后续复核** |
| `kill-and-death-events.md` | 「纯设计，`crates/` 中无任何对应类型」 | `HistoricalEvent`/击杀死亡记录已落地（`2226469`），`Agent::creature_kind` 已存在。**规模大，留给后续复核** |
| `identity-and-ids.md` | 「`Affiliation.org: ContentIndex`（已落地）依然是本文档要处理的问题的现状」 | 已改成 `OrgRef` 枚举。**留给后续复核** |

另有 `society-and-affiliation.md` 与 `race-system.md` 两份，它们 2026-08-26 的复核更正段本身已被同一天晚些的文化批次（`4aec07e`）追平——本次已各补一段跟进更正。

---

## 本次改了什么

只改文档，`crates/**` 一行未动。

- 新增本文档
- 新增 `../handoff/p6-to-p7.md`、`../handoff/p7-to-p8.md`
- 规格 `../../docs/superpowers/specs/2026-08-16-lostland-design.md` §15 的 P6/P7/P8 三行各补一段落地状态标注
- `../design/society-and-affiliation.md`、`../design/race-system.md` 各补一段「文化批次之后的跟进更正」
- `../design/README.md` 修正五处过期记录
- `../design/item-system.md`、`../design/equipment-slots.md`、`../design/crafting-system.md` 各修正开头「落地状态」一行
- `README.md`（`knowledge/` 索引）补三条新文档条目

---

## 相关文档

- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) — §15 阶段划分，本次修订 P6/P7/P8 三行
- [P5 → P6 交接清单](../handoff/p5-to-p6.md) — 本文档二节逐条核对了它列出的全部缺口
- [P6 → P7 交接清单](../handoff/p6-to-p7.md) — 本文档二、三节的结论落成交接
- [P7 → P8 交接清单](../handoff/p7-to-p8.md) — 本文档三、四节的结论落成交接
- [2026-08-26 社会/种族/冲突三份文档落地状态复核](2026-08-26-society-race-conflicts-reverification.md) — 同日更早的一次复核，本文档六、八节延续了它的若干条目
- [ADR 0028](../decisions/0028-steel-engine-construction-memory-corruption.md) — Steel 引擎构造期内存破坏，本文档一节「架构撤退」的直接依据
- [审计工单清单](worklist.md) — 落在本次禁区（`crates/**`）里的可直接照做工单
