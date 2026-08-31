# 批次 16：文件行数棘轮门禁 + 按「下一批要加什么」重构两处

**工作树** `wt-filesize`（分支 `wt-filesize`），基线 `19efd8b`。
**改前基线**：`bash scripts/ci/run_tests.sh` → **exit 0，2798 条通过、0 条失败**（本会话自己跑的）。

**两条黄金基准（本批开工时自己 grep 的，本批结束必须逐位不变）**：

| 常量 | 位置 | 开工时的值 |
|---|---|---|
| `EXPECTED_WORLD_DIGEST` | `crates/ll-world/tests/determinism.rs:351` | `11_270_479_921_196_970_914` |
| `EXPECTED_REPLAY_DIGEST` | `crates/ll-sim/tests/replay.rs:984` | `11_222_878_776_777_704_235` |
| `CONTENT_HASH_ALGORITHM_VERSION` | `crates/ll-mod/src/content_hash.rs:805` | `27` |

（照 `2026-08-28-session-handoff.md` 第〇节的纪律：这张表**只在本计划文档内当「开工快照」用**，
任何人读到这里要复核，请重新跑那三条 grep，不要抄。）

---

## 〇、这批要解决的到底是什么问题

任务书最初写的是「把最严重的两个文件拆到 800 行以内」。**所有者纠正了这个提法**：

> 「要合理规划，不是说超了就得砍。所以说是重构，让代码为之后的开发做准备。」

于是判据换了：**行数超标是症状，不是病**。真正要回答的是「接下来那批工作要往这些
文件里加东西时，会不会因为现在的结构而变得又慢又危险」。一个职责单一、内聚良好的
文件一千五百行也可以不动；一个把三种不相干职责搅在一起的文件八百行也该拆。

所以本批分两件事，**门禁是配角**：

- **任务 A（防回潮）**：加一道棘轮门禁，拦住「悄悄累积」——新文件一上来就超限、
  老文件继续膨胀。它**不是**「文件不许长」的硬约束。
- **任务 B（真正的重构）**：按下面第一节摸出来的「谁会碰哪里」表，选真正会打架的
  地方按职责拆开。

---

## 一、已排期的七批各自会落在哪里（勘查结论）

三个只读勘查代理并行摸出来的，行号均为勘查时实测。**这张表是任务 B 的唯一依据。**

| # | 批次 | 主要落点（文件 → 函数/行号） | 会不会与别的批次在同一个文件里打架 |
|---|---|---|---|
| 1 | **NPC 对话**（四批） | `ll-sim/src/intent.rs` 加 `Intent::DialogueChoose`/`Trade` 变体 + `actor()` 穷尽 match(729–757)；**`ll-sim/src/resolve.rs` 的 `resolve_dispatch` match(987–1085) 加 arm + 新 `resolve_*`**；`ll-game/src/player_action.rs` 加 `InteractTarget::Talk`（枚举 313–359、`item_def` 370–385、`interact_entries` 482–508、`interact_command` 732–822、`interact_row_text` ~1164、`interact_target_name` 1206–1227 共 6 处）；**`ll-game/src/app.rs` 加一块 `ScreenState::Dialogue`**（`update_screen` 1211–1361、`screen_row_texts` 2411–2488、`resolve_screen_pointer` 1174–1209、`draw_screen` 2503–2541）；`Effect::TransferOwnership`（`effect.rs` 650–659）**今天零调用方**，第 4/5 批是第一批 | **会。** 与批次 6 在 `app.rs` 撞（对话加屏 vs 布局改 HUD），与批次 5 在 `resolve.rs` 撞 |
| 2 | **据点建筑类型 + 街道 + 按类型填家具** | `ll-world/src/settlement.rs`：`stamp_settlement`(343–376) 主循环、`house_tiles`(520–555) 按类型分派、`spiral_offset`(608–637)/`building_origin`(410–422) 换街道感知布局、`SettlementSite`(203–280) 加字段；家具若走 `ground_items` 则连带 `ll-world/src/state.rs`(440/813/833/1435) 与存档版本 | 与批次 7 在 `chronicle.rs`/`settlement.rs` 轻度重叠 |
| 3 | **五个新种族 + 沙漠文化** | **几乎全是内容**：`mods/lostland/races.json5`、`cultures.json5`、`*.ftl`、`assets/sprites/`。Rust 侧零改动（`RaceTable`/`CultureTable` 完全数据驱动）。风险点在 `chronicle.rs` 的 `survey_habitable_zones`(650) 是否把沙漠判为可住 | 否 |
| 4 | **地形美术变化**（同地形多贴图按坐标哈希） | `ll-game/src/layout.rs`：`terrain_atlas_key`(151–162) 加坐标参数、`terrain_entry_name`(78–120) 的 19 条静态表；调用点只有 3 处（`app.rs:2617` + 两处测试）；`assets/sprites/manifest.json5` | 否（改动面最小的一批） |
| 5 | **树木系统** | `ll-game/src/player_action.rs` 加 `InteractTarget::Tree` + `TreeAction`（同批次 1 的 6 处）；**`resolve.rs` 加砍伐/培植/采果三族意图**；派生查询照 `ll-world/src/resource.rs::resource_node_at`(497) 的先例 | **会。** 与批次 1 在 `resolve.rs` 与 `player_action.rs` 撞 |
| 6 | **UI P0 第三组 W1/W2/F1/F5** | `ll-ui/src/widget/list.rs::push`(42–51)、`ll-ui/src/hud/render.rs`(`to_text_run` 703–712、`build_hud_frame` 277+)、`ll-ui/src/screen/mod.rs`(`SCREEN_WIDTH` 62、`build_screen_panel` 207)；**`ll-game/src/app.rs` 的 `draw_hud`(2165–2397)、`screen_row_texts`(2411–2488)、`draw_screen`(2503–2541)、`run_turn`(1012–1105)**；`player_action.rs` 加 `Feedback::DoorBlocked` | **会。** 与批次 1 在 `app.rs` 撞 |
| 7 | **P9**（势力外交/据点扩张/经济人口/Cohort LOD） | `ll-world/src/faction.rs`（关系矩阵）、`ll-world/src/state.rs`（进 `WorldState` + `hash` + **存档版本**）、`ll-world/src/chronicle.rs`（运行期扩张）、`ll-world/src/entity/thin.rs`（`promote` 229/`rebase` 303）、新 crate `ll-econsim`（不存在） | 与批次 2 轻度重叠 |

### 从这张表读出来的三个真实痛点

1. **`resolve.rs` 是意图分派的总汇，批次 1 与批次 5 都要往里加意图族。**
   今天加一族新意图，是往一个 8115 行的文件**中间**插——而且这个文件里同时住着
   属性派生（`derive_stats`）、10 个公开入口重载、9 族意图结算、以及 2452 行断言。
2. **`app.rs` 同时是「屏装配」与「HUD 装配」，批次 1 与批次 6 会在同一个文件里打架。**
   对话要加屏（改 `update_screen`/`screen_row_texts`/`draw_screen`），
   布局那批要改 HUD（改 `draw_hud`/`screen_row_texts`/`draw_screen`）——
   `screen_row_texts` 与 `draw_screen` **两批都要改，且在同一个文件里**。
3. `state.rs` 的确定性哈希那 680 行（`hash` 1181–1460 + 17 个 `write_*` 1461–1859）
   是最干净的可拆出物，批次 2 与批次 7 都会把这个文件推过 4000 行。
   **但本批不动它**——理由见第四节。

### 明确否掉的：`settlement.rs`

勘查结论：`settlement.rs`（1239 行，701 代码行）**是单一职责的**——只做「往 `ChunkGrid`
写地形」这一件纯函数的事，不含运行时查询、不含序列化，48% 是测试。批次 2 会让它长，
但**长的方向与现有结构一致**（`house_tiles` 变成按类型分派、`spiral_offset` 换布局器），
不是「新职责挤进来」。按新判据它不该动，本批不碰。

---

## 二、任务 A：棘轮门禁

### 判据：非空非注释行

实测：超限文件里代码行只占总行数的 **38%–75%**（中位数约 55%），接近一半的行是中文
文档注释。而规格 §13 自己下一条就要求「所有公开项必须有文档注释」「注释解释**为什么**」。
用总行数当判据等于让这两条规约互相打架，把「写清楚为什么」直接惩罚成行数负担。

因此判据是**非空、非注释行**。完整计数规则与三条已知偏差写在
`scripts/ci/check_file_size_budget.py` 头注释里。

### 快照形状

`scripts/ci/file_size_budget.json`，照 `save_body_shape.json` 的架子：

```json
{ "_note": "...", "limit": 800,
  "files": { "<路径>": { "code_lines": <脚本算的>, "reason": "<人写的>" } } }
```

**`code_lines` 全部由 `--bless` 从源码算出，一个数字都不是手写的**；`reason` 是人写的
「为什么这个文件可以这么长」，`--bless` 原样保留、不覆盖，**空着门禁就红**。
这是本门禁真正的判据：一个超限文件如果没人能用一句话说清它为什么可以这么长，
那它多半就是该拆的那个。

### 五条规则

| 情形 | 结论 |
|---|---|
| 快照外的文件超限 | **红**（新债） |
| 快照内文件涨了 | **红**（棘轮只许缩） |
| 快照内文件降了（仍超限） | **红**，提示跑 `--bless` 收紧预算 |
| 快照内文件降到 800 以下 | **红**，要求从快照里摘掉，此后按新文件规则管 |
| 快照内条目没写 `reason` / 文件已不存在 | **红** |

### 反例验证（ADR 0018 硬要求）

四条，逐条实跑并记录：① 给快照内文件加行 → 红；② 新建一个 801 代码行的文件 → 红；
③ 把某文件减到快照值以下 → 提示 bless；④ `--bless` 之后 → 绿。

### 接线

`scripts/ci/run_all.sh` 加一步；`.github/workflows/ci.yml` 照
`save-schema-version-check` 的形状加一个 `ubuntu-latest` job。

---

## 三、任务 B：拆哪两个、怎么拆

### B1 `crates/ll-sim/src/resolve.rs`（8115 总行 / 4246 代码行）

**它为什么该拆**：批次 1 与批次 5 都要往里加意图族，而今天这一个文件里住着四种
不相干的东西——属性派生、公开入口重载族、九族意图结算、2452 行断言。
**按意图族切开之后，加一族新意图 = 加一个模块**，而不是往八千行中间插。

拆成 `resolve.rs`（门面：模块文档 + 公开入口族 + `resolve_dispatch` + 共享常量/helper）
加一个 `resolve/` 目录，每个模块一句话职责：

| 模块 | 一句话职责 | 搬进去的函数 | 预估代码行 |
|---|---|---|---|
| `resolve/stats.rs` | 把智能体的基础属性、装备加成与生效中的临时修正算成一份 `DerivedStats` | `DerivedStats`、`derive_stats`、`derive_stats_at`、`attribute_slot`、`effective_speed_from_dexterity` | ~107 |
| `resolve/movement.rs` | 智能体挪动自己：走一步、与人换位、开关潜行 | `step_destination`、`occupant_at`、`resolve_move`、`resolve_swap`、`resolve_toggle_stealth` | ~131 |
| `resolve/portal.rs` | 门与空间的通行：开门、关门、进出室内 | `resolve_open_door`、`resolve_close_door`、`resolve_enter_space`、`entry_floor`、`resolve_exit_space` | ~120 |
| `resolve/combat.rs` | 攻击结算，以及一次击杀之后的全部善后（经验、任务进度、史册、尸体掉落） | `resolve_attack`、`append_kill_experience`、`append_quest_kill_progress`、`append_kill_history`、`append_corpse_drop` | ~327 |
| `resolve/inventory.rs` | 地面与背包之间搬运物品：拾取、搜刮、查看、丢弃、放置家具 | `resolve_pick_up`、`resolve_loot`、`resolve_inspect`、`merge_into_inventory_effect`、`resolve_drop`、`resolve_place`、`can_place_at` | ~284 |
| `resolve/equipment.rs` | 装备槽与消耗品：穿上、脱下、用掉 | `resolve_equip`、`resolve_unequip`、`resolve_use_item` | ~137 |
| `resolve/crafting.rs` | 制作与知识获取：合成、阅读、试验、鉴定 | `resolve_craft`、`resolve_read`、`resolve_experiment`、`resolve_identify`、`has_all_ingredients`、两个事件 tag 常量 | ~307 |
| `resolve/progression.rs` | 角色成长与技能施放：属性点、学技能、弃子职业、放技能、资源池读写 | `resolve_allocate_attribute_point`、`resolve_learn_skill`、`resolve_abandon_subclass`、`append_craft_progress`、`resolve_use_skill`、`resolve_resource_pool_regen`、`restore_slots_from_lowest_tier`、`current_resource`、`resource_pool_usable`、`find_available_slot_tier` | ~367 |
| `resolve/upkeep.rs` | 不做事的那两族意图：等待与休息，以及休息完成时的恢复结算 | `resolve_wait`、`resolve_rest`、`rest_completion_effects`、`scalar_rest_effect`、`tiered_slot_rest_effects` | ~199 |

耦合形状是**星形**，不是网状：`resolve_dispatch` → 各族模块 → 少数共享 helper
（`schedule_after`、`within_reach`、`derive_stats`、`occupant_at`）。各族模块之间不互相调用。

**测试**：`#[cfg(test)] mod tests`（2452 总行 / 1794 代码行）整体用 `#[path]` 挪进
`resolve_tests.rs`，走 `app_save_tests.rs`/`app_navigation_tests.rs` 的既有先例。
**刻意不按意图族拆断言**——45 条测试与约 10 个共享夹具（`test_world`、`spawn_agent`、
`spawn_agent_with_luck`…）交错在一起，按族切开要么重复夹具、要么造一个
`test_support` 模块被九个文件回头调用，那正是所有者警告的「硬拆出一堆互相调来调去
的小文件」。而且**它不影响下一批的难度**：新意图族在自己的模块里写自己的测试。
这一条会如实登记进快照，理由写明。

### B2 `crates/ll-game/src/app.rs`（4741 总行 / 2842 代码行）

**它为什么该拆**：批次 1（对话加屏）与批次 6（布局改 HUD）会在同一个文件里打架，
而且撞在同两个函数上（`screen_row_texts`、`draw_screen`）。今天 `app.rs` 同时是
GPU 资源管理 + 主循环 + 世界推进 + 输入路由 + 八块屏的流程 + HUD 装配 + 地表渲染 + 存档接线。

拆成 `app.rs`（门面：`Demo` 结构体 + 构造 + 主循环 `AppHandler` + 世界推进/回合）
加一个 `app/` 目录：

| 模块 | 一句话职责 | 搬进去的东西 | 预估代码行 |
|---|---|---|---|
| `app/gpu.rs` | 管住这一帧的 GPU 侧家当：图集贴图、表面获取、上屏 | `GpuResources` 及其 `impl`、`load_sprite_sources`、`atlas_contains` | ~190 |
| `app/screen_flow.rs` | 八块模态屏的流程：吃输入、算转屏、产每行文本、画出来 | `ScreenTransition`、`update_screen` 及八个 `update_*`/`start_new_game`/`generate_draft_world`/`prepare_spawn_pick_view`/`spawn_pick_slice`/`clicked_spawn_zone`/`enter_world_at`/`finish_entering_world`、`resolve_screen_pointer`、`close_screen`、`open_menu`、`screen_row_texts`、`draw_screen` | ~600 |
| `app/save_flow.rs` | 三条存档路（自动、手动、退出）与读档 | `write_save`、`maybe_autosave`、`save_now`、`can_save_manually`、`save_on_exit`、`load_saved_game`、`back_to_title`、`selected_slot`、`enter_world_in_slot` | ~180 |
| `app/hud_draw.rs` | 把常驻 HUD 那几块面板喂给 `ll-ui` 并提交 | `draw_hud`、`SpawnPickHud` | ~230 |
| `app/surface.rs` | 把世界的一屏格子连同三层战争迷雾画出来 | `render_surface`、`push_surface_draw`、`space_profile_of`、`push_player_marker`、`sprite_instance` | ~220 |

`impl Demo` 分散在多个模块是 Rust 允许的（同一 crate 内的固有 impl 可以分块），
调用方一个字都不用改。

**测试**：`app.rs` 已有的两处 `#[path]` 外挂测试（`app_save_tests.rs`、
`app_navigation_tests.rs`）保持不动；文件内 `mod tests`（1622 行）本批**不动**——
它与本批要解决的「两批工作撞在同一个文件」无关，动它只会把风险面白白扩大。
如实登记进快照。

---

## 四、明确不做的

| 不做 | 理由 |
|---|---|
| 拆 `state.rs` 的哈希那 680 行 | 确实是最干净的可拆出物，但它服务的是批次 2/7，**不是最先开工的那一批**；而且它紧挨着存档形状（`WorldStateRepr`/`ChunkGrid` 的 serde impl），搬移风险高于本批另两处。**登记进快照并写明「待重构：等批次 2 之前」**，不在本批做 |
| 拆 `settlement.rs` | 单一职责、内聚良好，见第一节末尾 |
| 按意图族拆 `resolve` 的断言 | 见 B1 |
| 删任何代码 / 改任何文档结论 | 并行批次（`wt-deadcode`）在盘点死代码，删除是它之后的独立批次 |
| 新增 example target | `check_no_examples.sh` 会拦（ADR 0030） |

---

## 五、执行顺序与提交切分

1. **提交 1**：门禁脚本 + 快照（含 17 条理由）+ `run_all.sh`/`ci.yml` 接线 + 反例验证
2. **提交 2**：`resolve.rs` 按意图族拆开 + 测试外挂 + 刷新快照
3. **提交 3**：`app.rs` 按职责拆开 + 刷新快照

每一步后跑 `bash scripts/ci/run_tests.sh`；全部做完跑 `bash scripts/ci/run_all.sh`
必须 exit 0，并复核两条黄金基准逐位不变。

**纯搬移的硬证据**：两条黄金基准逐位不变 + 既有断言一个字不改仍然全绿。
任何一条不成立就说明动到了行为，停下来查，不要改断言去迁就。
