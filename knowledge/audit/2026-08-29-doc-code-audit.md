# 文档—代码一致性审计与未接线声明盘点

**复核日期**：2026-08-30（**文件名沿用 2026-08-29**，那是派工时给定的路径；正文日期
以本行为准，两者相差一天，不是另一次审计）。
**基线**：`main`，HEAD `19efd8b`（`Merge branch 'wt-navigation'`）。工作分支 `wt-docaudit`。
**并发声明**：复核时 `../wt-filesize` 有代理正在**搬移** `crates/ll-sim/src/resolve.rs`、
`crates/ll-game/src/app.rs` 等文件的代码。**本次因此一行 `.rs` 都没有改**（含文档注释），
只改 `knowledge/**` 与 `docs/**` 的 Markdown。凡是落点在 `.rs` 里的更正，本文档只**登记**，
留给后续批次动手——那些文件正在被搬，改了必冲突。
**方法**：所有行号都是本次 `grep -n` 实测，不转述任何既有文档里的行号。凡判断不了的，
标「存疑」保留原样，不猜着改（见五节）。

**这份文档要干三件事**：① 逐条列出**会误导下一批工作**的文档—代码不一致（一节）；
② 把「声明了但没接线」逐条判成**缺陷**还是**刻意搁置**并给出证据（二节）；
③ 回答「下一个接手的人从哪读起」（三节）。

**判据只有一条**（一、二节共用）：**如果下一个人照这句话去做，会不会做错事、或者白做
一件已经做完的事。** 不精确但不改变行动的措辞一律不进正文，只在四节汇总。

---

## 零、三条最要紧的

1. **`knowledge/design/README.md` 五节「落地状态速览」那张表，27 行里 20 行是错的，
   而且全部错在同一个方向：把已经落地的东西写成「纯设计，代码中无任何对应类型」。**
   这是全仓库**最会让人白做工**的一处——它是新人读的第一份索引，而 12 份被它写成
   「纯设计」的文档，**它们自己开头的「落地状态」行早就更正过了**。索引与它索引的
   文档互相矛盾，索引是错的那一方。见一节第 1 条。

2. **「更正写在更正方、不写在被更正方」是这个仓库反复出现的形状，本次抓到三例。**
   `dialogue-system.md` 更正了 `narrative-system.md`、`ui-and-navigation.md` 更正了
   `action-capability-and-input-context.md`、`batch10-ownership.md` 更正了
   `batch8-character-creation.md`——三次都只改了更正方，**被更正的那三份一个标记都没有**。
   打开被更正文档的人拿不到这条信息，而**他恰恰是最需要它的人**。见一节第 4、5、6 条。

3. **「未接线」不是一类东西，本次盘点的 12 项里，缺陷 5 条、刻意搁置 7 条**，其中
   **3 条刻意搁置的「理由文字」指向已经不存在的东西**（已删除的 `examples/`、已删除的
   `ll-script` crate、已经合并的并行批次）。理由失效不等于结论失效——但**把理由留在那儿
   不动，下一个人就无法判断它是不是仍然该搁置**。见二节。

---

## 一、会误导下一批工作的对不上之处

共 **9 条**。每条给出：文档怎么说 / 代码实际怎样 / 改了没有。
**改的方式一律照仓库惯例**：正文原样保留，追加更正段或原文划掉，写明什么时候因为什么
被推翻——不静默改写（教训见 [2026-08-28 会话交接](../handoff/2026-08-28-session-handoff.md)
第〇之二节第 10 条「一次未经记录的规格变更」）。

### 1. 设计文档总索引的「落地状态速览」整张表 —— **已改（改成不再充当判据）**

**文档怎么说**：`knowledge/design/README.md` 五节那张 27 行的表，逐份文档给「状态」。

**代码实际怎样**：20 行与代码对不上，且**全部错在同一方向**。挑最会致命的：

| 行 | 表里怎么说 | 代码实际怎样（本次 grep 实测） |
|---|---|---|
| 物品系统 | 纯设计，代码中无任何对应类型 | `crates/ll-mod/src/item.rs:70` `ItemDef`；`crates/ll-world/src/item.rs:79` `ItemStack`、`:283` `GroundItemStack`；`crates/ll-world/src/entity/agent.rs:629` `inventory` |
| 装备栏位与占位掩码 | 纯设计，代码中无任何对应类型 | `crates/ll-world/src/item.rs:426` `pub struct EquipSlot(u8)`、`:589` `pub struct SlotMask(u32)`；`crates/ll-world/src/entity/agent.rs:661` `equipment` |
| 坐标系与空间模型 | 纯设计（`Space`/`SpaceProfile`/区块索引均未落地） | `crates/ll-world/src/space.rs:56`、`crates/ll-world/src/space_profile.rs:58`、`crates/ll-world/src/zone.rs`。**该文档自己开头写的是「已完整落地」**（`coordinate-system-and-layers.md:5`） |
| 身份与 ID 空间 | 纯设计，代码中无任何对应类型 | `crates/ll-core/src/ident.rs:249` `WorldId`；`crates/ll-world/src/entity/org.rs:36` `OrgInstance` |
| 天赋/特性系统 | 纯设计；且「全项目检索确认『等级』这个概念当前不存在于任何字段」 | `crates/ll-mod/src/trait_def.rs:85` `TraitDef`、`:168` `TraitTable`；`crates/ll-sim/src/traits.rs:92` `TraitGrant`、`:304` `effective_traits`；等级字段在 `crates/ll-world/src/entity/agent.rs:558`/`:563`/`:572` |
| 资源池与休息系统 | 纯设计；「需要 `trait-system.md` 补一个待办字段 `granted_resource_pools` 才能真正落地」 | 连它说的那个待办字段都已经有了：`crates/ll-mod/src/content_audit.rs:521` `TraitAttrs::granted_resource_pools`；另有 `crates/ll-mod/src/resource_pool.rs:27` `ResourcePoolDef`、`crates/ll-world/src/entity/agent.rs:25` `RestState`、`crates/ll-sim/src/intent.rs:194` `Intent::Rest` |
| 击杀与死亡记录 | 纯设计；`Effect::Kill` 目前只有 `target` 一个字段 | `crates/ll-sim/src/effect.rs:91` 有三个字段（`target`/`killer`/`cause`）；`crates/ll-world/src/history.rs:283` `KillRecord`、`:312` `KillCause` |
| 伤害公式 mod API | 纯设计；「武器类别/伤害类别/`DamageComponent` 均为纯设计」 | `crates/ll-sim/src/formula.rs:78` `FormulaDef`；`crates/ll-mod/src/weapon_category.rs:36`；`crates/ll-mod/src/damage_category.rs:78`；内容侧 `mods/example_mod/damage_formulas.json5` |
| 增益与通用触发器 | 「`crates/ll-sim/src/effect.rs` 目前**只有六个既有变体**，均不含增益相关内容」 | 约四十个变体，且其中一个**就是**增益通道：`crates/ll-sim/src/effect.rs:321` `ApplyStatModifier`；`crates/ll-world/src/entity/agent.rs:495` `active_stat_modifiers` |
| 行动能力与输入上下文 | 「`InputContext` 已落地但**仅 `Gameplay` 一个变体**；`UiMode` 栈尚未落地」 | `crates/ll-platform/src/keybind.rs:132`-`141` 三个变体；`crates/ll-ui/src/widget/ui_mode.rs:105` `UiMode`、`:138` `UiModeStack` |
| 界面布局与导航模型 | 「六至十节是规格……**尚未落地任何一条**」 | N8/N2/N7/N1/N10 已落地，提交 `1d74e75`「三套模态系统收敛进一套栈（规格 N8/N2/N7/N1/N10）」；指针约定 `b6cd2e8` |
| mod 包结构与资产 VFS | 「`crates/ll-mod/src/` 无 `vfs.rs` 或等价实现」「已在分支 `wt-walkart`（**未合入 `main`**）落地」 | `crates/ll-mod/src/asset_vfs.rs` 就在 `main` 上，并已接进应用：`crates/ll-game/src/app.rs:18`、`:154`、`:246` |
| 三轴战斗结算 / 载具与骑乘 | 两行都说 `resolve_attack`「已核实是占位实现（攻击力恒读力量、防御恒为零）」 | 已走 `DerivedStats`（吸收 `active_stat_modifiers` 与 `equipment`）：`crates/ll-sim/src/resolve.rs:263`、`:2647`。**同一句占位断言被复制到四处**（本表两行 + `damage-formula-mod-api.md:511` + `combat-three-axis.md:3`） |
| 脚本层数据句柄 / 脚本状态存储 / 资产管理系统 | 三行都把 `steel-core`、`ll-script` 的 `register_fn`、`ScriptEntityHandle`、`crates/ll-world/src/script_state.rs` 当作**已落地的依赖** | **四者全部不存在**：`crates/` 下无 `ll-script`（`Cargo.toml:6` members = `crates/*`，实际 11 个 crate）；`ScriptEntityHandle`/`script_state` 全仓库零命中；`script_state.rs` 已改名 `crates/ll-world/src/mod_state.rs` |
| 种族系统 | 「`ThinPopulation.race` 是需要在 P9 解决的实现债务」；`RaceDef` 未落地 | 债务已还：`crates/ll-world/src/entity/thin.rs:42`-`52` 明写「`race` 列已经还掉了（文化批次）」；`crates/ll-mod/src/race.rs:114` `RaceDef`、`:302` `RaceTable` |

**另有 6 份文档在索引三节里列着、在这张表里根本没有行**：等级与经验系统、食物与烹饪
系统、物品归属与犯罪判定、制作系统、制作类副职的奖励、据点/结构物/NPC 生成——**其中
四份已经落地**。

**改了没有：改了，但没有逐行重写。** 逐行重写等于再造一份会漂移的副本——这正是
[2026-08-28 会话交接](../handoff/2026-08-28-session-handoff.md) 第〇节「凡是把真相源
之外的副本当判据，迟早分叉」点名的失败模式，那一节的结论是**不留副本**。处置是：
在五节表头加一段更正，说明这张表已经不能当判据、真相源是**代码**与**每份文档自己
开头的「落地状态」行**，并把上面这张「会致人白做工」的清单直接嵌进去。表本身原样保留。

### 2. 会话交接里的黄金基准常量表 —— **已改（第四次过期）**

**文档怎么说**：`knowledge/handoff/2026-08-27-session-handoff.md` 第一节纪律 1 那张
「三个关键常量**当前值**」表。

**代码实际怎样**：两条黄金基准的**数值与行号全部对不上**。本次实测：

```
crates/ll-world/tests/determinism.rs:351   （表里写 :231）
crates/ll-sim/tests/replay.rs:984          （表里写 :862）
```

数值本文档**刻意不抄**——理由见下。`CONTENT_HASH_ALGORITHM_VERSION` 本次仍是表里那个值
（`crates/ll-mod/src/content_hash.rs:805`），但同样请自己 grep。

**这是同一张表的第四次事故**。前三次记在 `2026-08-28-session-handoff.md` 第〇节：
气候条带批次重冻 → 表过期 → **两个互不相干的代理各自撞上**；08-28 那份于是补了一张
「当前值」新表 → **角色创建批次当场又把它冻掉** → **第三个代理撞上新表**。08-28 第〇节
的结论是「这里没有值可抄」。**第四次是本次**：08-27 那张原表从来没有被处理过，它至今
还在那儿，还写着「当前值」。

**改了没有：改了。** 原表三行**原文划掉保留**（追溯用），表头加一段更正指向 08-28 第〇节
的 grep 命令。**本次刻意不填新值**——填新值就是第五次。

### 3. 交接文档第四节「会越拖越贵」三条里，两条已经关闭 —— **已改**

**文档怎么说**：`knowledge/handoff/2026-08-28-session-handoff.md:393` 起，第四节
「会越拖越贵」列三条待所有者裁定项。

**代码实际怎样**：

- **第 1 条「`Owner`（物品归属）认领还是正式放弃」——已认领并落地。**
  `crates/ll-world/src/ownership.rs:63` `pub enum Owner`（`#[default] Unowned`）；
  `crates/ll-sim/src/effect.rs:650` `Effect::TransferOwnership`。落地批次
  `docs/superpowers/plans/2026-08-29-batch10-ownership.md`。
- **第 2 条「尸体被捡起来之后，肚子里的遗物去哪」——所有者已答复并落地。**
  形状是 `append_corpse_drop` 产出 1 + N 条（尸体自己 `contents` 恒空，遗物各一条，
  全部落在同一 `victim.pos`），见 `crates/ll-sim/src/resolve.rs:1775`-`1798` 与
  `crates/ll-game/tests/corpse_flattening_interact_list.rs`。顺带**改变了行为**：
  空手死者现在也产出尸体。
- 第 3 条（树木派生 vs 独立实体）**仍然悬着**，是三条里唯一还开着的。

**为什么这条会误导**：下一批读第四节，会把两条已经关闭的问题**再去问一次所有者**，
或者更糟——照它排期。而所有者对第 1 条的答复本身就藏在另一份计划文档里。

**改了没有：改了。** 两条原文划掉保留，各追加一段「什么时候因为什么关闭 + 落点」。

### 4. `narrative-system.md`：四条依赖前提 + 三处「必须等 `format-text`」 —— **已改**

**文档怎么说**（`knowledge/design/narrative-system.md`）：

- 开头「落地状态」把 `OrgInstance`/`WorldId` 与世界历史生成列为「**纯设计，无代码**」，
  据此二节把「角色绑定解析」定成本系统**唯一有意义的世界生成依赖**，七节把整个剧本
  系统推迟到世界生成之后。
- 185/201/214 三处：对话变量插值「**必须等 `format-text` 落地**才能支持」，在此之前
  「只能使用不含变量的静态文案」。

**代码实际怎样**：

- **`OrgInstance` 已落地**：`crates/ll-world/src/entity/org.rs:36`；
  `WorldState::factions: FactionTable` 进存档主体（`crates/ll-world/src/state.rs:502`），
  `CURRENT_SCHEMA_VERSION` 因此到 4（`crates/ll-content/src/save_file.rs:139`）。
  **那条阻塞已经解除。**
- 世界历史生成已落地并在跑：`crates/ll-world/src/chronicle.rs`、`history.rs`、`settlement.rs`。
- `crates/ll-script/src/api/state.rs` 整个 crate 已删除；`ScriptValue` 已改名 `ModStateValue`。
- **对话变量插值今天就能做**：`Catalog::resolve_with_args`（`crates/ll-i18n/src/lib.rs:167`）
  与 `FluentArgs`（`:49`）都是 `pub`，**从一开始就是**，今天有六个真实生产调用方：
  `crates/ll-game/src/menu_screen.rs:360`、`save_list.rs:119`、`settings_view.rs:78`、
  `crates/ll-ui/src/hud/character_panel.rs:321` 与 `:331`、`crates/ll-ui/src/hud/mod.rs:199`。

**这一条不是新发现**——`knowledge/design/dialogue-system.md:309`（3.3 节）早已写下这条
更正，标题就叫「`narrative-system.md` 那句『必须等 `format-text`』已经过期」。**但上一次
只改了更正方**：`narrative-system.md` 里一个标记都没有，而 `dialogue-system.md`
**根本不在设计文档总索引里**（见三节第 2 条）——于是这条更正在实践上不可达。

**改了没有：改了。** 顶部加复核横幅，文末追加「⚠ 落地状态复核更正（2026-08-30）」，
逐条给出证据与被推翻的时间/原因，正文一个字未改。

### 5. `action-capability-and-input-context.md`：整节前提已落地 —— **已改**

**文档怎么说**：开头「落地状态」= 「纯设计，`crates/` 中无任何对应类型——`InputContext`
目前只有 `Gameplay` 一个变体」；2.2 节的**结论**是「**新增 `InputContext::Menu` 一个变体**」。

**代码实际怎样**：`crates/ll-platform/src/keybind.rs:132`-`141` 三个变体
（`Gameplay`/`Menu`/`TextEntry`）。**2.2 节提议要做的那件事早已做完**——照着做就是重做一遍。
另外 2.2 节把「背包首页」列进 `Menu` 覆盖范围也是错的：背包**刻意**跑在 `Gameplay` 上，
理由完整写在 `crates/ll-game/src/player_action.rs:124`-`133`。

**同样是「更正写在更正方」**：`knowledge/design/ui-and-navigation.md`（冻结于 2026-08-30）
的「相关文档」一节已经逐条列出本文档的**五处**过期——但那条记录留在 `ui-and-navigation.md`，
被更正的这一份一个标记都没有。

**改了没有：改了。** 顶部横幅 + 文末更正表（把 `ui-and-navigation.md` 那五条搬回来，
另补两条：`Intent` 七变体、`mod-lifecycle-and-event-api.md` 的脚本时代框架）。

### 6. `batch8-character-creation.md` 3.5 节：一条会毁存档的「先例」 —— **已改**

**文档怎么说**：`docs/superpowers/plans/2026-08-28-batch8-character-creation.md:175`-`183`
（3.5 节「存档兼容」）：给 `Agent.gender` 加字段「改的是**主体**……⇒ 走 `serde(default)`，
`CURRENT_SCHEMA_VERSION` **不动**（与气候批次给 `TerrainShape::climate_band_width` 加
`serde(default)` 是同一条既有先例）」。

**代码实际怎样**：**存档主体走 `postcard`（non-self-describing），`#[serde(default)]` 在
那条路径上是空操作。** 已实测（老结构体三字段编码 → 新结构体四字段带 `serde(default)`
解码 → `Err("Hit the end of buffer")`），新字段若不在末尾更糟：后续字段字节错位读成合法值。
代码侧现在自带完整论证：`crates/ll-world/src/entity/gender.rs:56`、
`crates/ll-world/src/item.rs:372`-`384`、`crates/ll-content/src/save_file.rs:99`-`115`。
今天 `CURRENT_SCHEMA_VERSION = 4`（`crates/ll-content/src/save_file.rs:139`），
门禁 `scripts/ci/check_save_schema_version.py` 盯着这件事。

**这是第三例「更正写在更正方」**：`docs/superpowers/plans/2026-08-29-batch10-ownership.md`
五之三节如实记录了这次翻案（连「`Agent::gender` 与 `GroundItemStack::placed` 两条既有
先例因此都是错的」都写了），**但 batch8 那份一个标记都没有**。而 batch8 才是那条错误
先例的**出处**——下一批引先例时引的是它。

**改了没有：改了。** 在 `2026-08-28-session-handoff.md` 第四节补了「第 5 条之二」把这条
翻案登记进交接主线（交接文档第二节曾为气候批次那条先例背书），并在 batch8 3.5 节原地
加更正段。

### 7. `terrain_dirt` 的消费者口径：文档把单元测试算成了生产消费者 —— **未改（落点在 `.rs`）**

**文档怎么说**：`assets/atlas/README.md` 有一段 2026-08-29 更正，说 `terrain_dirt`
「消费者从两处降到一处：`crates/ll-game/src/content.rs` 的 mod 资产覆盖验收拿它当被
覆盖目标……**因此它仍然不是孤儿图**」。`crates/ll-game/src/layout.rs:73`-`77` 同口径。

**代码实际怎样**：那唯一一处**在测试里**——`crates/ll-game/src/content.rs:1091`，而该文件的
`#[cfg(test)] mod tests` 从 `:793` 开始。生产路径上 `crates/ll-game/src/layout.rs` 的
`terrain_entry_name`（`:79`-`:121`）十九支里**没有 `terrain_dirt`**。按本次全仓统一判据
（排除测试与 `content_hash`/`content_audit` 这类逐字段扫描），它是**零生产消费者**，
与 `boss_idle_0` 同级。

**改了没有：未改。** 两处落点一处在 `assets/atlas/README.md`（可改），一处在
`crates/ll-game/src/layout.rs:73`-`77`（`.rs`，本批不碰）。**只改一半会造出第三种口径**，
因此本次两处都不动，整条登记在二节第 4 项，留给同一批一起改。

### 8. `outfit_from_inventory` 的「等谁来用」：消费者早就来了 —— **未改（落点全在 `.rs`/`.json5`）**

**文档怎么说**：三处仍写着它在等消费者——
`crates/ll-sim/src/item.rs:405`-`411`（「它今天的调用点只有下面那四条单元测试，这是
**有意的等待状态**」）、`crates/ll-game/src/world.rs:645`（「所有者裁定的另一半……
还没有落地」）、`mods/lostland/races.json5`（「它在等 NPC 自行决策那一批」）。

**代码实际怎样**：**那一批已经落地。** `crates/ll-mod/src/roster.rs:977` 与 `:991`
（函数 `outfit_decision`，入口 `:881`）是真实生产调用点，`#[cfg(test)]` 在其后；
`roster.rs:939` 自己就写着「架在 `outfit_from_inventory` 之上」。

**改了没有：未改**，三处落点全在 `.rs` 与 `.json5`。登记在二节第 3 项。

### 9. `assets/atlas/README.md` 残留的两处已删路径 —— **未改（低危，存疑）**

正文两处仍引 `crates/ll-sim/examples/p5_coordinate_acceptance` 与「供……`ll-world` 的
验收 demo 引用」，而 `examples/` 已整体删除（[ADR 0030](../decisions/0030-remove-examples-acceptance-demos.md)）。
**该文件顶部已有 2026-08-29 的整段更正**，`boss_idle_0`/`terrain_dirt` 两节也已重写，
残留的只是叙述历史时的路径提及——**不改变任何行动**，按本文档判据不进正文清单，
登记在四节。

---

## 二、未接线声明盘点（**只出清单，不删**）

**删除是后续独立批次，且要所有者过目。本次一行代码都没删。**

判据（两类，逐条给证据）：

- **缺陷**：本该接线却漏了，或者「等接线」的条件**已经满足而没人回来接**，
  或者文档把它的接线状态**说错了**。
- **刻意搁置**：写明了**理由**、写明了**最小形状**、写明了**落地条件**。
  先例：`Owner` 当初被 P6 显式拒绝；`RaceDef.footprint`/`lifespan_years` 是所有者
  2026-08-29 刚裁定「留着不接线」。

**统计：12 项，缺陷 5、刻意搁置 7**（其中 3 条刻意搁置的**理由文字已失效**，需要重写理由，
但**结论不一定要翻**）。

### 缺陷（5 条）

| # | 什么 | 在哪 | 谁在用 | 证据（为什么判成缺陷） | 建议处置 | 处置代价 |
|---|---|---|---|---|---|---|
| 1 | 本体任务 `lostland:branch_b` 用 `kind: "script"` 条件，而 `lostland:finale` 以它为前置 | `mods/lostland/quests.json5:60`（`finale` 在 `:66`-`:69`） | 解析器造得出 `QuestCondition::Script`（`crates/ll-mod/src/content_schema.rs:770`），但**没有任何求值器**：`crates/ll-sim/src/quest.rs:102` 明写「只交付 `KillCount`」 | **`branch_b` 永不可完成 ⇒ `finale` 永不可解锁，本体任务图的终点是死的。** `quests.json5` 文件头（`:26`-`:30`）诚实地写了「Script 变体目前只是一个数据标签」，**但没有写下这个下游后果**——一条诚实的声明加一条没人算过的连锁 | 二选一：把 `branch_b` 改成 `kill-count`（保住任务图连通），或在装载期加一条「Script 条件 ⇒ 该节点不可达」的 warn，让它至少**说出来** | **小**：改一行 `.json5`，或加一条装载期检查 |
| 2 | 精灵 `terrain_dirt` | 声明在 `assets/sprites/manifest.json5:124`、`assets/atlas/placeholder.json:65` | **零生产消费者**（唯一命中 `crates/ll-game/src/content.rs:1091` 在 `#[cfg(test)]` 之内，测试模块从 `:793` 起） | 判成缺陷的**不是图本身**，是**文档把测试算成了生产消费者**——`crates/ll-game/src/layout.rs:73`-`77` 与 `assets/atlas/README.md` 都据此断言「它仍然不是孤儿图」。这个口径与同一份 README 给 `boss_idle_0` 用的口径**不一致** | 先统一口径（两处一起改，见一节第 7 条），再由所有者裁「留图还是删」——**不要先删图** | 文档小；删图要重冻 mod 覆盖测试夹具 |
| 3 | `ll_sim::item::outfit_from_inventory` | `crates/ll-sim/src/item.rs:447` | **已有真实生产消费者**：`crates/ll-mod/src/roster.rs:977`、`:991` | 三处文档仍说它「在等消费者」（见一节第 8 条）。这不是没接线，是**接线了但没人回来改文档**——下一批读了会以为自己要去写那一层，而它已经在 `roster.rs:938` 之上写好了 | 三处「等谁来用」段落改写成「消费者已是 `ll_mod::roster::outfit_decision`」 | **纯文档**，三处各几行 |
| 4 | `Intent::AllocateAttributePoint` / `Intent::LearnSkill` | `crates/ll-sim/src/intent.rs:510` / `:533` | 只有 `crates/ll-sim/src/resolve.rs:1073` / `:1076` 的 dispatch 臂 + 测试 ⇒ **零生产生产者** | **仓库自称是缺陷**：`crates/ll-sim/src/intent.rs:506`-`508` 原文「**这仍是一处真实的缺口**，如实标注」。它写下的落地条件是「加点要的是一块角色面板上的交互」——**那块面板已经落地**：`crates/ll-ui/src/hud/character_panel.rs:235`/`:240` 已在显示两种余额，模态栈与鼠标点击已随 `1d74e75`/`b6cd2e8` 合并。**条件已满足，没人回来接** | 排一批把两个键接上角色面板 | **中**：一块已存在面板的键位/点击接线 |
| 5 | `Intent::EnterSpace` / `Intent::ExitSpace`（连带 `crates/ll-world/src/interior.rs` 整个模块） | `crates/ll-sim/src/intent.rs:156` / `:165` | **零**。`crates/ll-game/src` 下 `SpaceId`/`interior` 零命中 | `crates/ll-sim/src/intent.rs:154`-`155` 写它「面向已经知道要进哪个具体实例的调用方（**demo**/未来的交互层）」——**它写下的唯一现存消费者是 `examples/`，而 `examples/` 已整体删除**（ADR 0030）。理由段现在指向一个不存在的东西，于是**没有任何判据能说明它今天该不该接** | 先改写 `intent.rs:145`-`155` 的理由段（说清楚「未来的交互层」具体指什么、什么时候）；`interior.rs` 整个模块进「未接线存量」清单**由所有者裁** | 文档小；真接线是一整批 |

### 刻意搁置（7 条）

| # | 什么 | 在哪 | 谁在用 | 理由 / 最小形状 / 落地条件（三样齐不齐） | 建议处置 | 处置代价 |
|---|---|---|---|---|---|---|
| 6 | `GroundItemStack.contents` / `Intent::Loot` / `resolve_loot` / `InteractTarget::Container` | `crates/ll-world/src/item.rs:306`；`crates/ll-sim/src/intent.rs:381`；`crates/ll-sim/src/resolve.rs:2418`；`crates/ll-game/src/player_action.rs:313`/`:495`/`:795` | 结构上**已经串通**（`player_action.rs:492` 判 `!ground.contents.is_empty()`），只是**无生产路径造出非空 `contents`** | **三样齐全。** 理由与落地条件写在 `crates/ll-world/src/item.rs:297`-`310`（「今天没有任何生产者，这是**故意**的……字段**不删**：箱子是它将来的正经消费者」）与 `crates/ll-sim/src/resolve.rs:1795`-`1796`、`:2307`-`:2308`（「这道排除因此暂时空转，等箱子那批把它用起来」）。**这是本仓库刻意搁置的样板写法** | **保留，不动。** 补一条观察给下一批：`lostland:iron_bound_chest` 物品**已经存在**（`mods/lostland/items.json5:764`），只是今天还只是家具、不带 `contents`——落地条件比写文档时更近了 | 零 |
| 7 | `RaceDef.footprint` / `RaceDef.lifespan_years` | `crates/ll-mod/src/race.rs:143` / `:147` | 零 | **结论是所有者刚裁定的**（`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节第 12 条：「**留着不接线**，在字段文档里写明『等体型/寿命系统落地』。**这条问了四次才有答复**。要写下来，别再沉默」）。**但那条指示没执行**：`race.rs:139`-`142` 与 `scripts/ci/check_field_consumers.py:242`-`243` 两处**仍是「等接线」口径**（「预期随占位系统落地一并接线」「是后续批次范围」），与「留着不接线」的裁定相反 | **执行所有者那条指示**：改写 `race.rs` 两处字段文档 + 两条豁免理由，照 `ClassDef.primary_attribute` 那条「永久判定」的写法标成永久豁免 | **纯文档**，四处 |
| 8 | `QuestCondition::Script` | `crates/ll-mod/src/quest.rs:121` | 生产上只有 `content_schema.rs:770` 造它，**无求值者** | 理由/形状/条件都写了，**但理由指向已删除的系统**：`crates/ll-mod/src/quest.rs:40`-`41` 仍写「需要串起 `ll-sim`/**`ll-script`** 的运行期管线」，而 `ll-script` 不存在了。**内容侧已经更新过口径**（`mods/lostland/quests.json5:31`-`34`：「将来指向的会是 Rust 侧注册的具名判定函数，不是一段 Steel 源码」），**`quest.rs` 没跟上** | 把 `quest.rs:25`-`42` 的理由重写成「等 Rust 侧具名判定函数注册表」并写明落地条件；**与缺陷第 1 项同批处理**（同一条链的两端） | **纯文档** |
| 9 | `ll-ui` 里 `build_panel` 的刻意重复 | 原件 `crates/ll-ui/src/hud/mod.rs:112`；副本 `crates/ll-ui/src/screen/mod.rs:207`（`build_screen_panel`） | 两者**各有真实消费者**（hud 五处；screen 走 `render.rs:57`） | 理由写得很清楚（`crates/ll-ui/src/screen/mod.rs:34`-`45`：「不复用的理由是**并行批次风险**……代价明确、**可逆**：两个批次都落地之后，把这一段收拢回一个共用 helper 是一次**纯机械的重构**」）。**那两个批次都已合并**（`1365b20`、`655c891`），**并行风险不存在了**——但**「纯机械」也已经不成立**：两份**已实质分叉**，`build_screen_panel` 多出 probe 两遍量高 + `centered_origin` 居中 + `backdrop` + `row_rects`（`screen/mod.rs:214`-`227`），`hud::build_panel` 仍是单遍闭包 + 调用方给 origin（`hud/mod.rs:112`-`130`） | **不要照那句「纯机械」去合并**——照做会发现成本远超承诺。二选一：真的收拢（现在是一次真实重构，不是机械替换），或**更正 `screen/mod.rs:34`-`45`** 改成「两份已实质分叉，不再计划合并」。**现状最糟**：承诺挂着、无人认领、代价已经变了 | 文档小；真合并**中等** |
| 10 | 精灵 `boss_idle_0` | `assets/sprites/manifest.json5:100`、`assets/atlas/placeholder.json:53` | **零** | **三样齐全且已两次更新身份**：`assets/atlas/README.md` 有一整节记录它「留图但不再算待接线项」的两次理由变更（第二次就是 `examples/` 删除之后的重写），并明说「**这条处置是本批次的判断，不是所有者的新裁定**」 | **保留，不动。** 这一条是「刻意搁置怎么维护」的正面样板 | 零 |
| 11 | `Intent::Rest` / `ToggleStealth` / `Read` / `Identify` / `Experiment` / `AbandonSubclass` | `crates/ll-sim/src/intent.rs:194`/`:440`/`:621`/`:713`/`:659`/`:564` | 各只有 `resolve.rs` 的 dispatch 臂 + 测试 ⇒ 零生产生产者 | **三样齐全**，且**接法写得很具体**：`intent.rs:432`-`439`（潜行「与背包菜单不是同一块界面」）、`:605`-`:620`（读书「接法是现成的：背包菜单上多一个键」，并给了端到端证据 `crates/ll-mod/tests/example_mod_recipe_discovery.rs`）、`:706`-`:711`（鉴定同上）。汇总清单在 `intent.rs:786`-`790` | **保留，不动。** 与缺陷第 4 项（加点/学技能）区别在于：那两条的落地条件**已经满足**，这六条还没有 | 零 |
| 12 | `Agent.gender` 的字段消费者门禁豁免 | `scripts/ci/check_field_consumers.py:261`-`275` | 渲染层在读（精灵键的一部分） | **本仓库写得最好的一条豁免**：写死了解除日期与承接批次（「P9 婚配/血缘落地的同一批**必须**回来删掉这一条」），并明说这么写就是「为了不让它变成第二个 `RaceDef.footprint`」 | **保留。** 只需在 P9 开工时挂钩——建议把它写进 P9 的开工清单，别只靠这条注释自己等着 | 零 |

### 全量对表的两条结论（供下一批省一次重扫）

- **图集/精灵键全量对表**：`assets/sprites/manifest.json5` 全部 115 条 +
  `assets/atlas/placeholder.json` 36 条逐条 grep `crates/*/src`，**零生产消费者的只有两条**
  （`boss_idle_0`、`terrain_dirt`）。家具六件与 52 条种族×职业合成图**字面 grep 为零但不是
  孤儿**——它们走「物品/种族/职业 id 即图集条目名」的动态查找，由
  `crates/ll-game/tests/atlas_coverage.rs` 与 `crates/ll-game/tests/npc_appearance.rs` 守住。
  **「52 张贴图全查不到而画面看不出区别」那个先例当前不复发。**
- **`#[allow(dead_code)]` 全仓库只有一处**：`crates/ll-game/src/lib.rs:233`，形式是
  `#[cfg_attr(not(test), allow(dead_code))]`，是「测试专用项在非测试构建下静音」的正当用法。
- **`check_field_consumers.py` 的 EXEMPTIONS 约 50 条逐条读过**，**只有两条理由过期**
  （即刻意搁置第 7 项的 `RaceDef.footprint`/`lifespan_years`）。已接线的旧条目
  （`Agent.luck`、`RaceDef.darkvision_cells`、四条 `RuleModifier`、`Agent.affiliations`、
  `Agent.subclasses`、`CultureAttrs.hostility`）都已被正确摘除并留下摘除说明——
  **这道门禁本身在正常工作**。

---

## 三、文档结构：下一个接手的人从哪读起

现状：设计文档 **40 份**（`ls knowledge/design/*.md`，不含 README）、交接 **10 份**、
审计 **5 份 + 1 份工单**、ADR **30 份**、计划 **20 份**。

**本次否掉的大搬家**（先说不做什么）：

- **不重排 `knowledge/design/` 的目录结构**（例如按「已落地 / 纯设计」分两个子目录）。
  40 份文档被全仓几十处交叉引用，重排会让 `check_markdown_links.py` 与
  `check_doc_links.sh` 两道门禁下面几十条链接同时失效，**收益是「看起来整齐」，
  代价是一整批修链接**。判据不满足。
- **不合并、不删除任何一份设计文档**，包括六份脚本时代的。它们开头都带 2026-08-23
  状态订正，作为「为什么当初这么设计」的背景仍然有用；删掉会让若干 ADR 的引用悬空。
- **不把两份会话交接改写成 `pN-to-pN+1` 的形式**。它们记的是**会话内**的进度与待裁定项，
  阶段边界切不出来——这正是 `2026-08-27-session-handoff.md` 开头自己给出的理由。

**本次做的四件事**：

### 1. 修掉索引里会漂移的计数 —— **已改**

`knowledge/design/README.md` 正文写「本目录下**二十份文档**」、一节标题写
「**十九份文档**各管什么」、正文里还有「第二十一份」「第二十三份」这类序数。
**实际 40 份。** 这些计数与序数是**另一份会漂移的副本**，与黄金基准常量表同一个形状。

**改法与常量表一致**：不填新数字（填了就等着第二次过期），改成
「跑 `ls knowledge/design/*.md`」，并保留原文说明它为什么被换掉。

### 2. 三份设计文档**根本不在索引里** —— **已改**

`dialogue-system.md`、`save-and-mod-version-policy.md`、`worldgen-parameters.md`
在 `knowledge/design/README.md` 里**零命中**。

**这三份里有一份是要命的**：`dialogue-system.md` 承载着所有者 2026-08-29 十三条裁定的
对话那一半，**并且它是 `narrative-system.md` 那条「必须等 `format-text`」更正的唯一
出处**（3.3 节）。它不在索引里 ⇒ 那条更正在实践上不可达 ⇒ 一节第 4 条那个坑就一直开着。

**改了没有：改了。** 三份补进索引一节的表，并在索引里点明 `dialogue-system.md` 与
`narrative-system.md` 的更正关系。

### 3. 两份文档讲同一件事而结论不一致：找到三对，全部是「更正方 vs 被更正方」

| 这一份说 | 那一份说 | 谁对 | 处置 |
|---|---|---|---|
| `narrative-system.md:185`/`:201`/`:214`：对话变量插值必须等 `format-text` | `dialogue-system.md:309`（3.3）：那句话已经过期 | `dialogue-system.md` | 已在被更正方加横幅与文末更正（一节第 4 条） |
| `action-capability-and-input-context.md:3`/2.2：`InputContext` 只有 `Gameplay`，结论是新增 `Menu` | `ui-and-navigation.md` 相关文档一节：该文档有五处过期 | `ui-and-navigation.md` | 已在被更正方加横幅与文末更正表（一节第 5 条） |
| `batch8-character-creation.md` 3.5：主体加字段走 `serde(default)` 即可 | `batch10-ownership.md` 五之三：那是空操作，必须动 schema 版本 | `batch10-ownership.md` | 已在被更正方加更正段，并登记进交接主线（一节第 6 条） |

**加上索引 vs 12 份设计文档自己「落地状态」行的矛盾（一节第 1 条），一共四类。
四类的形状完全相同：更正被写进了发现问题的那一份，没有写回被更正的那一份。**

**结构性建议（需要所有者点头，本次未擅自立规矩）**：把「更正必须写回被更正方」
补成一条纪律，进 `2026-08-27-session-handoff.md` 第一节那六条的行列。今天的做法是
「在自己文档里如实记录」——**那对写的人是够的，对读的人不够**，因为读的人打开的是
被更正的那一份。成本极低（一条横幅），收益是这四类问题不再复发。

### 4. 十份交接文档，读者不知道该读哪份 —— **已改**

`knowledge/README.md` 的「阶段交接」一节列了**八条**，全是 `pN-to-pN+1.md`——
**两份会话交接（`2026-08-27` / `2026-08-28`）一条都没有**。而这两份才是：
承载所有者十三条裁定的、记着当前待办与已知缺陷的、任何新人都被要求先读整份的那两份。
**从 `knowledge/README.md` 出发的人找不到它们。**

同时 `p7-to-p8.md` 是**预写**的（P7 未收尾），它记不了 P7 内部的进度——这正是会话交接
存在的理由，但索引里只有它。

**改了没有：改了。** 在 `knowledge/README.md` 的「阶段交接」一节：把两份会话交接补进去、
放在最前，并加一段说明两种切分方式各自管什么、**新人应当先读哪一份**。

### 5. 一条只登记不动手的观察：`docs/architecture/` 已经落后 13 天

`docs/architecture/`（8 份 + `discrepancies.md`）自称面向「**新加入项目、需要在半天内
理解系统骨架**的工程师」，冻结于 2026-08-17、核对提交 `7a126f5`，并自己写明
「此后若仓库继续变化，请以代码为准，**本文档组不会自动跟随**」。

**它是全仓库最指向新人、又最落后的一组。** 例如它写作时 `crates/ll-sim` 的 `resolve`
「尚未落地」，而今天 `resolve.rs` 是全仓最大的文件之一（正被 `wt-filesize` 拆分）。
**本次不动它**：它有明确的自我声明、且它讲的是 crate 分层/数据流/不变式这类**变化最慢**
的东西，主体判断多半仍然成立；逐份复核是一次独立审计的工作量，不该塞进本批。
**登记在此，供所有者决定要不要排。**

---

## 四、存疑未改（5 条）

按纪律：判断不了的**留着并标存疑**，不猜着改。

1. **`docs/architecture/` 八份的具体条目今天还剩多少成立。**（见三节第 5 条）
   不是「不知道有没有过期」，是「逐份核对是一次独立审计」，本批没有做，
   因此**不敢说它哪几条过期、哪几条没有**。
2. **`assets/atlas/README.md` 两处对已删 `examples/` 路径的叙述性提及**（一节第 9 条）。
   它们出现在讲历史的段落里，改成什么措辞才既不丢历史又不误导，**没有把握**；
   且该文件顶部已有整段更正，读者不至于被带偏。留着。
3. **`crates/ll-world/src/interior.rs` 整个模块（约 577 行）该不该留。**
   本次只能判定它的**理由文字失效**（指向已删的 `examples/`），判不了**结论**——
   离散空间/室内是规格里的正经内容，「今天没接线」与「该不该留」是两个问题。
   进二节缺陷第 5 项，但处置写的是「先改写理由、再由所有者裁」。
4. **`crates/ll-game/src/player_action.rs:131` 那句「那里目前恒传 `InputContext::Gameplay`」。**
   `2026-08-28-session-handoff.md` 第二节说菜单批次已经修掉「两处生产 resolve 硬编码
   `Gameplay`」，而 `ui-and-navigation.md` 说运行期唯一生产点在 `app.rs`。
   **这句注释可能已经过期，也可能说的是另一条路径**——`app.rs` 正在被 `wt-filesize` 搬，
   本次不去读一个正在移动的目标。登记，不下结论。
5. **`knowledge/audit/worklist.md` 里 W-01 起的工单今天还剩几条没做。**
   它源自 2026-08-17 的审计，其中 W-01（覆盖率门禁）从 `scripts/ci/check_coverage.sh`
   的存在看**多半已经做了**，但逐条核对不在本批范围。**没核对就不标「已完成」。**

---

## 五、需要所有者裁定

1. **`lostland:finale` 永远解不开，怎么处理？**（二节缺陷第 1 项）改 `branch_b` 的条件
   保住任务图连通，还是保留 `Script` 这条「半条链路」的示范价值、只加一条装载期警告？
   **前者立刻可玩，后者保住示范。** 这是内容取舍，不是技术判断。
2. **`crates/ll-world/src/interior.rs`（约 577 行）与 `Intent::EnterSpace`/`ExitSpace`：
   留着等，还是这一轮就清掉？**（存疑第 3 条）它写下的唯一消费者是已被裁定删除的
   `examples/`。留着要重写理由与落地条件，清掉将来重做。
3. **`terrain_dirt` 留图还是删？**（二节缺陷第 2 项）它与 `boss_idle_0` 今天是同一种
   东西（零生产消费者），但 `boss_idle_0` 有裁定、它没有。**建议一并按同一条口径处置。**
4. **`ll-ui` 那两份 `build_panel`：合还是不合？**（二节刻意搁置第 9 项）当初写的
   「纯机械的重构」承诺已经不成立，两份已实质分叉。**不裁定的话它会一直挂着。**
5. **「更正必须写回被更正方」要不要立成第九条纪律？**（三节第 3 条）本次四类问题
   同一个形状，成本一条横幅。
6. **`docs/architecture/` 那一组要不要排一次复核？**（三节第 5 条）它是最指向新人的
   一组，也是最落后的一组。

---

## 相关文档

- [2026-08-17 文档—代码一致性审计](2026-08-17-doc-code-audit.md) — 本系列第一份
- [2026-08-26 社会/种族/冲突三份设计文档落地状态复核](2026-08-26-society-race-conflicts-reverification.md) — 本文档的形式来源
- [2026-08-26 P6/P7/P8 阶段清算](2026-08-26-phase-reckoning-p6-p8.md)
- [2026-08-27 会话交接](../handoff/2026-08-27-session-handoff.md) — 第一节六条纪律
- [2026-08-28 会话交接](../handoff/2026-08-28-session-handoff.md) — 第〇节「不要在文档里找常量的值」、第〇之二节所有者十三条裁定
- [ADR 0030 — 去掉 examples/ 验收 demo](../decisions/0030-remove-examples-acceptance-demos.md)
