# P6 → P7 交接清单

**补写于** 2026-08-26，基线 `main` HEAD `ed1584f`。
**读者**：P7（UI 层与世界生成器）的收尾者，以及任何想知道「P6 到底交付了什么」的人。

---

## ⚠ 这是一份补写的交接清单，不是当时写的

**P6 结束时没有人写交接清单，P7 也已经做掉了大半。** 这份文档在 P7 进行到一半时补写，因此与前六份的性质不同：前六份是「上一阶段留给下一阶段的坑」，这一份是「**P6 留下的坑，以及 P7 至今有没有碰它们**」——每一条都附一列「P7 至今的处理」，如实标注哪些被解决了、哪些被绕过了、哪些没人碰。

补写的依据与完整逐条核实见 [P6/P7/P8 阶段清算](../audit/2026-08-26-phase-reckoning-p6-p8.md)。这份文档不重复那里的证据，只列缺口。

---

## 一、P6 收尾评审：反向核对规格

按 `p2-to-p3.md` 第六节立下、历轮交接延续执行的纪律——**每个阶段收尾必须反向核对一次规格，不是查实现是否满足规格，而是查规格有没有被实现淘汰**——补做一次：

### 1. 规格 P6 行的裁定理由已被完整兑现

P6 行的裁定理由原文：「职业与技能树要读装备属性——没有物品的话技能树只能加纯数值；双模式存档也要序列化背包。」两条都兑现了：`derive_stats`（`crates/ll-sim/src/resolve.rs:399`）真的把装备属性算进战斗；`Agent::inventory` 与 `WorldState::ground_items` 都进了序列化与 `WorldState::hash()`。**这一行没有被实现淘汰，反向核对无发现。**

### 2. 规格 P6 行「物品的接口面比地形大得多」这句预警兑现了，但兑现方式与预期不同

原文预期物品的「本体即 Mod」检验会像地形那样「当场抓出洞」。实际抓出的不是特权路径，而是**跨表引用校验缺一个统一阶段**（`p5-to-p6.md` 三节点名的那件事）——物品引入的槽位表/标签表/配方表/伤害类别表互相指涉，逼出了提交 `f7a4203`「装载后内容校验 pass——跨表引用完整性与本体字段覆盖」。**`p5-to-p6.md` 三节的那条建议被 P6 认领并还清了**，这是历轮交接里第一次有建议真的被下一阶段照做。

### 3. 一项被规格指名交给 P5、P5 沉默跳过的债务，在 P7 期间被悄悄还清了

`OverviewCell::explored` 恒为 `true`——`p5-to-p6.md` 一、2 节把它作为「被规格点名指派、却既未认领也未拒绝」的典型记录下来，并明确写着「P6 不是这项债务的天然归属」。

**核实结果：它在 P7 期间被还清了**（`ExplorationMemory` 在 `crates/ll-world/src/exploration.rs:131`，`overview.rs:86`/`:205` 已读真实探索记忆，战争迷雾接进本体渲染并有集成测试 `crates/ll-game/tests/fog_of_war.rs`）。

**但还清它的批次同样没有留下任何阶段记录**——没有计划、没有交接、提交信息里没有阶段编号。一项「做了但没记录」的债务，被另一次「做了但没记录」的工作还清。这不是好消息，是同一个问题的两次发作。

---

## 二、A. P6 遗留的三处真实缺口

### 1. `Owner`（物品归属）：P6 显式拒绝，但拒绝的理由到 P7 已经只剩一半

`crates/ll-world/src/item.rs:31-57` 整段模块文档写明了不落地的理由，逐条核实过「偷窃判定、随从装备归属、商店库存」三个消费场景一个都不存在，并把最小形状（`Unowned`/`Player`/`Npc`/`Faction`/`Shop`）留在文档里。**这是本仓库文档纪律的正面范例，不是缺陷**——与 P5 沉默跳过探索记忆完全不同。

**但 P7 期间前置变了**：

- **卫兵盘查已经落地**（提交 `e81e03c`：视野内概率发起物品盘查，`crates/ll-mod/tests/example_mod_guard_inspection.rs`）；
- **「目击」已经用 FOV 实现**（提交 `68c5d8f`「推翻目击不可表达的结论，接入 FOV 实现目击判定」）；
- **完整设计已经写完**（`../design/ownership-and-crime-detection.md`，提交 `4eca643`：五变体形状核实、两处类型修正对齐 `OrgRef`、盗窃判定挂载在 `resolve_pick_up`、销赃计时 `StolenMarker`、`Effect::TransferOwnership` 接口、犯罪记录走 `HistoricalEventKind::Theft`）。

P6 拒绝时列的三个缺席场景，今天缺的只剩「商店系统」一个。**这项债务需要一次新的裁定，不能靠 P6 那次拒绝继续挂着**——见 [阶段清算](../audit/2026-08-26-phase-reckoning-p6-p8.md) 七节第 1 条。

**成本会随时间涨**：`Owner` 一旦加进 `ItemStack`，`can_merge`（`crates/ll-world/src/item.rs:754`）必须同批追加这一条比较（`item-system.md` 二节：「新增任何实例字段都自动被覆盖……只要补进这个比较，堆叠逻辑就自动正确」），而背包/地面物品/尸体容器每多一条产出路径，回头补的面就更大。

**P7 至今的处理**：没碰。设计写了，实现零行。

### 2. 击杀类型：四条路径里三条换了新字段，任务系统那条被落下了

`p5-to-p6.md` 二、A、3 记录的缺口是「缺敌人类型注册表——`KillCount.target_kind` 借用 `Agent::race` 表达」。P6/P7 期间新增了 `Agent::creature_kind: Option<ContentIndex>`（`crates/ll-world/src/entity/agent.rs:475`），配套的既有回退规则是 `creature_kind.unwrap_or(race)`。

**核实四条读「敌人类型」的路径**：

| 路径 | 位置 | 读的是 |
|---|---|---|
| `Effect::IncrementKillCount` 聚合计数 | `crates/ll-sim/src/resolve.rs:1571` | `victim.creature_kind.unwrap_or(victim.race)` |
| 死亡统计 | `crates/ll-sim/src/resolve.rs:1704` | `creature_kind.unwrap_or(race)` |
| 尸体的 `def` | `crates/ll-sim/src/resolve.rs:1813` | `creature_kind.unwrap_or(race)` |
| **任务击杀进度** | **`crates/ll-sim/src/resolve.rs:1603`** | **`agent.race`（裸读，没有回退）** |

```rust
Effect::Kill { target, .. } => world.actors.get(*target).map(|agent| agent.race),
```

**这是一处此前没有任何文档记录的分叉。** 一个 mod 只要给某个生物同时设了 `race` 与不同的 `creature_kind`，它的击杀在聚合计数/死亡统计/尸体里算一种东西、在任务进度里算另一种东西，**两边静默不一致**。本体内容今天不会撞见（`creature_kind` 构造时恒等于 `race`，`agent.rs:838`），纯粹是运气。

`crates/ll-sim/src/quest.rs:30` 的模块文档「为什么用 `Agent::race` 作为 `target_kind`，如实记录简化」仍原样存在，**但它记录的是「借用 race」这个简化，不是「其余三条路径已经改用 creature_kind、只有我没改」这个新事实**——文档没跟上代码。

**P7 至今的处理**：没碰。改法是一行（把 `:1603` 换成同一条回退表达式），但**需要先确认「任务击杀计数应该按 `creature_kind` 还是按 `race`」是同一个语义**——如果是，那就是一行；如果任务系统有意要按种族统计，那 `quest.rs:30` 的模块文档要改写成「有意与其余三条不同」，而不是继续写成「简化」。**不要不裁定就改。**

### 3. `AddGroundItem` 的占位闸门只覆盖两条路径

`crates/ll-sim/src/resolve.rs:2674-2681` 已经显式记录为已知边界：`Intent::Drop`/`Intent::Place` 会检查「这一格已经立着一件放置物」，而**尸体掉落（`append_corpse_drop`）与盲盒溢出等其余 `Effect::AddGroundItem` 产出点不检查**——一个 NPC 恰好死在锻炉那一格上，尸体照样摞上去。

文档写明了补齐它的前置：那些路径各自要能拿到 `WorldState`，并且**要先裁定「放不下时尸体去哪」**（挤到旁边一格？直接蒸发？）。

**P7 至今的处理**：没碰。这是一处**记录完整、拒绝理由充分**的已知边界，不是遗漏。

---

## 三、B. 三个 `Intent` 变体在等 P7 那块屏幕，不是在等玩法

`Intent` 现有 24 个变体，其中 11 个**至今没有任何生产者**（既无玩家输入、也无 NPC 行为树）：

```
OpenDoor  EnterSpace  ExitSpace  Rest  ToggleStealth
AllocateAttributePoint  LearnSkill  AbandonSubclass  Read  Experiment  Identify
```

**这 11 个不是同一类东西，混在一起看会误判优先级**：

| 分组 | 变体 | 缺的是什么 |
|---|---|---|
| **缺 UI，玩法已完整** | `AllocateAttributePoint`、`LearnSkill`、`AbandonSubclass` | 结算侧全通（升级发点 `b6d8757`、技能树 P5-B、副职 `83ef14b`/`fc92f24`），**缺的纯粹是「一块能点的屏幕」——这正是 P7 的活** |
| **缺 UI，玩法已完整** | `Read`、`Experiment`、`Identify` | 鉴定与配方发现已落地（`c291857`、`e3f00c1`），同样缺入口屏幕 |
| **缺按键，实现已完整** | `OpenDoor`、`EnterSpace`、`ExitSpace` | 门的开合、进出 `Interior` 的结算链路 P5 坐标系批次就全通了；`GameKey` 里没有对应键，`Intent::Interact` 也没有分流到它们 |
| **缺按键 + 缺反馈** | `Rest`、`ToggleStealth` | 休息事件（`224f06c`）与潜行状态（`bb6cda8`）结算都在，同样没有入口 |

**给 P7 收尾者的具体建议**：这 11 个变体是 P7「游戏内菜单 + 设置界面」这两项交付物的**天然验收线**。菜单落地时，前六个应该同时拿到入口；`OpenDoor`/`EnterSpace`/`ExitSpace` 三个更简单，`Intent::Interact` 的分流表里补三条即可（`crates/ll-game/src/player_action.rs` 的 `InteractTarget` 已经有 `Facility`/`Container`/`Loose` 三种目标，加门与空间入口是同一个模式）。

**不要把它们当成「玩法没做完」记进待办**——玩法做完了，是入口没接。这个区分很重要：混在一起会让 P8 的计划作者以为要重做结算。

---

## 四、C. 气候条带：第五轮提醒，而 P7 就是它的指定归属阶段

`p5-to-p6.md` 一、3 节把这条写得很重，原话是「**这是第五轮/第六轮提醒了，光提醒不管用**」，并且明确要求：

> P7 是气候条带的指定归属阶段，若 P7 计划作者仍然不认领，第五轮提醒失效的记录本身就该单独成为一次流程问题登记，而不是继续被动等第六轮。

**核实结果：P7 的世界生成器完整落地了，气候条带仍然是零实现。**

- `grep -rn "climate\|气候\|zonal_band" crates/` 只有三处无关命中（`content_schema_gear.rs:557`「一条带加值类型的规则修正」、`load_session.rs:48`「串成一条带」是断词误命中，`temperature.rs:155` 是一句解释昼夜温差的注释）。
- `SpaceProfileTable::base_temperature`（`crates/ll-world/src/space_profile.rs:282`）按**空间层**取温度，与坐标无关——`settlements-structures-and-npc-spawning.md` 已经点名过这件事：「区域气候（`base_temperature` 与 `Weather` **都与坐标无关**），『寒冷区域的据点』当前不可表达」。
- P7 期间落地了天气系统（`d5215f1`）与温度系统（`c12c04f`），**两者都是纯派生且都不看纬度**——离气候条带只差「按 j 坐标分带」这一步，却没走。

**因此，按 `p5-to-p6.md` 立下的规矩，这里登记一次流程问题**：

> **流程问题登记 #1**：规格 §7.1/决策 23 的「气候为周期性条带：两条赤道 + 两条极圈」被历轮交接指定归属 P7。P7 的世界生成器（编年史、据点、资源点、文化、天气、温度六个子系统）已完整落地，**气候条带既未实现，也未被任何文档显式声明推迟**。这是第五轮提醒失效。根因与 `OverviewCell::explored` 那次完全相同：**没有阶段计划文档，就没有地方承载「本阶段认领/不认领哪些债务」这个动作。**

**给 P7 收尾者的要求（不是建议）**：在 P7 收尾时**必须显式做一次二选一**——要么把气候条带排进 P7 剩余批次，要么在规格 §15 P7 行明确写「本阶段不认领气候条带，推迟到 P<N>，理由是 X」。**第三种选择（继续沉默）已经用掉五次了。**

### 同一条转发链上的另一项：光照透过率

规格 §7.3「瓦片地形携带光照透过率等属性」，历轮建议归属 **P9**（与 `RaceDef.darkvision_floor`/`sight_radius_at` 同批更省一次返工）。**仍是零实现**（`grep -rn "透光率\|透过率\|light_transmit\|transmittance" crates/` 零命中）。

P7 只是转发方，真正的认领点在 P9。**P7 → P8 的交接需要把这条转发链继续往下传**，这是第六轮转发。

---

## 五、D. 「没接线」这条纪律终于有了机器门禁——这是 P6/P7 期间最大的方法论收获

`p5-to-p6.md` 六节把「没接线」记为「本项目最顽固的失败模式——六次」，并给出一条可操作建议（每新增接口必须同批新增「至少有一个真实调用点」的断言）。

**P6/P7 期间做的比那条建议更彻底：把它变成了一道 CI 门禁。**

`scripts/ci/check_field_consumers.py`（700 行，提交 `ae19134`「新增字段接线检查门禁，堵住『声明但无决策层消费者』的静默失败」）：

- 扫描内容表结构体的全部字段与枚举变体（当前 **142 个目标**），检查它们在「决策层」（`crates/ll-sim/src` 等 3 组文件路径）有没有任何读取；
- **阻断模式**，不是 warn；
- 存量 **58 条**未接线字段全部收进 `EXEMPTIONS` 豁免清单，**每条必须写明理由与预期接线阶段**（例如 `RaceDef.footprint`：「占位格数——`race-system.md` 十二节明确标注碰撞/寻路是否支持 >1x1 占位尚未核实，声明先行、消费后补」）；
- **反向也检查**：豁免清单里的条目如果已经被接线了却没摘掉豁免，门禁会报错（`:667`）；条目对应的字段被删了却留着豁免，也会报错（`:676`）——防止豁免清单本身腐烂成一堆假绿；
- 与内容值哈希门禁**互校**（`:592`）：`ContentTableKind` 覆盖的表必须 ⊆ 本门禁覆盖的表，防止新增内容表同时绕过两道门禁。

CI 门禁总数从 P5 收尾时的 **6 道**涨到 **9 道**（新增：字段接线检查、硬编码 i18n 字符串扫描 warn 模式、Markdown 死链检查），本地入口统一到 `scripts/ci/run_all.sh`（提交 `2880c94`：把 CI 内联命令抽成脚本，本地与 CI 共用单一来源）。

**给 P7/P8 的启示**：「没接线」这个问题在**字段**这一层已经被机器接管了，但**在阶段这一层没有**——本次清算发现的正是它的镜像：「做了但没记录」。`GameKey::Menu` 定义齐全却无消费点（三、四节），恰好落在字段门禁扫不到的地方（它是 `enum GameKey` 的变体，不是内容表字段）。**门禁的覆盖边界本身就是下一处缺口最可能出现的地方**——这是 ADR 0022「覆盖不全的确定性哈希等于没有确定性哈希」同一条判据的又一次应用。

---

## 六、E. 端到端测试层：第一次有了真正的集成测试目录

`p5-to-p6.md` 五节连续第五轮建议「L6 端到端测试仍然没有真正建起来」。P6/P7 期间：

- `crates/ll-game/` 新建（提交 `aa95024`「新建游戏本体二进制 ll-game，接通启动到存档的最小可玩闭环」）——**这是端到端测试第一次有地方可写**；
- `crates/ll-game/tests/` 现有四条集成测试：`fog_of_war.rs`、`culture_and_war.rs`、`npc_materialization.rs`、`prerequisite_graph_gate.rs`，每一条都走「真实内容装载 → 建世界 → 推进 → 断言可观测结果」的完整链路；
- `crates/ll-content/tests/` 的 `e2e_save_cycle.rs` 与 `fuzz_save_load.rs` 保留。

**但 `p5-to-p6.md` 五、1 建议的那一条具体链路没被覆盖**：「扩展 `e2e_save_cycle.rs`，覆盖至少一条完整的物品拾取→装备→战斗结算链路」。物品链路的测试确实有（`crates/ll-mod/tests/example_mod_equipment.rs`、`example_mod_use.rs`、`example_mod_starting_items.rs` 等十余份），但它们是**按 mod 分的功能测试**，不是「存档往返 + 玩法结算」合成的那一条。

**P7 至今的处理**：绕过了——用「更多更细的功能测试」代替了「一条更长的端到端」。这是一个合理的替代，**但不是原建议**，如实标注。

---

## 七、F. 已知边界（照实写，不要粉饰）

- **P6 与 P7 都没有验收 demo。** 规格 §15 开头写着「**每阶段必须交付可独立运行的 `examples/` 验收 demo**」。P0–P5 每一阶段都有（`p0_acceptance`、`p1_acceptance`、`p2_acceptance`、`p3_acceptance`、`p4_acceptance`，以及 P5 的三份 `p5_coordinate_acceptance`/`p5_gameplay_acceptance`/`p5_save_acceptance`），**P6 与 P7 各自零份**——`crates/ll-game/examples/` 下唯一的一份是 `probe_content_hash.rs`，一个探针工具，不是阶段验收。部分原因是 `ll-game` 本体二进制本身可跑了（`aa95024`），验收从 demo 转移到了「真跑本体」——但那是一次**未经记录的规格变更**，规格 §15 这条硬性要求至今原文未改。**这是一处需要裁定的规格—实现分叉**：要么补 demo，要么改规格说明「本体二进制可运行后不再要求逐阶段 demo」。
- **主工作树根目录躺着五张验收截图与两个存档文件，均未纳入版本控制**（`hud_p7_screenshot.png`、`hud_p7_screenshot_textured.png`、`hud_p8_screenshot_final.png`、`ui_interaction_p7_button_screenshot.png`、`world_map_screenshot.png`、`save.llsave`、`save.llsave.stale-backup`；`git status` 里全是 `??`，克隆一份干净仓库看不到它们）。文件名里的 `p7`/`p8` 是**唯一残存的阶段标记**——它们比任何提交信息都更接近「当时的人认为自己在做哪个 P」。`hud_p8_screenshot_final.png` 这个名字说明当时有人认为 HUD 工作属于 P8，而规格把 UI 记在 P7。**这处命名分歧本身值得记录**：没有阶段文档时，阶段编号就退化成截图文件名里的私人记法。
- **`.superpowers/sdd/` 账本机制在 P6/P7 期间事实上废弃了。** `p5-to-p6.md` 七节要求「P6 若继续使用这套机制，开工第一批任务前应当先验证简报抽取是否生效」——核实结果是**整套机制没有再被使用**（`git ls-files` 下无任何 `.superpowers/sdd/` 条目）。这不算违反建议（建议的前提是「若继续使用」），但意味着 P5 那套「计划 + 账本 + 简报」三件套只剩计划一件，而计划也停在 P5。
- **`crates/` 内有四处模块文档仍在描述已被拆除或已被超越的现实**：`crates/ll-sim/src/behavior.rs:5`（「行为树写成 Steel `.scm`」）、`crates/ll-ui/src/lib.rs:17`（「仍然排除：焦点导航、按钮、输入处理」，前三项已落地）、`crates/ll-mod/src/race.rs`（多处 `register-race`）、`crates/ll-world/src/chronicle.rs`（「没有战争」而同文件 `wage_wars` 在跑）。**属于 `crates/**`，本次只读未改**，留给后续代码批次顺手修正。

---

## 相关文档

- [P6/P7/P8 阶段清算](../audit/2026-08-26-phase-reckoning-p6-p8.md) — 本文档全部结论的证据来源，含逐条代码行号
- [P5 → P6 交接清单](p5-to-p6.md) — 本文档一、二节逐条核对了它列出的缺口与建议
- [P7 → P8 交接清单](p7-to-p8.md) — 气候条带与光照透过率两条转发链在那里继续
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) — §15 P6 行本次已补落地状态标注
- [物品系统](../design/item-system.md) / [装备栏位与占位掩码](../design/equipment-slots.md) — 两份的开头「落地状态」本次已从「纯设计」更正为实测状态
- [物品归属与犯罪判定系统](../design/ownership-and-crime-detection.md) — 二、A、1 节 `Owner` 债务的完整设计，实现零行
- [制作系统](../design/crafting-system.md) — 开头「落地状态」本次已更正
- [ADR 0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 覆盖不全的门禁等于没有门禁，本文档五节末尾同一条判据
- [ADR 0028](../decisions/0028-steel-engine-construction-memory-corruption.md) — Steel 拆除，横跨 P6/P7 的架构撤退
- [覆盖率与缺失测试层](../../docs/qa/04-覆盖率与缺失测试层.md) — 六节测试层定位依据
