# 批次 13：去掉 `examples/`，把有用的东西搬迁走

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，全文 0 次出现「反例」。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**基线**：`2a865c4`（main，`Merge branch 'wt-uxdesign'`）
**工作树/分支**：`wt-exampleclean`
**所有者裁定原文**：「我觉得应该要去掉 example。然后有用的东西搬迁了。剩下的后面考虑。」
**改前测试基线（本工作树实跑 `bash scripts/ci/run_tests.sh`）**：**2768 passed, 0 failed, 0 ignored**

---

## 〇、这条裁定推翻的是什么

规格 §15 的一句硬性要求：

> 每阶段在本规格批准后各自产出独立实施计划。**每阶段必须交付可独立运行的 `examples/` 验收 demo。**

§17 风险登记里它的原始理由：

| 风险 | 等级 | 缓解措施 |
|---|---|---|
| 逐层铺地基导致长期无可见产出 | 中 | 每阶段强制交付 `examples/` demo |

**那个理由今天已经由别的东西达成**：本体二进制可运行、所有者在实机试玩
并逐条报缺陷（本会话与前几会话的缺陷清单绝大多数来自实机试玩，不是来自
任何一个 demo）。而 2026-08-28 新加的门禁 `scripts/ci/run_acceptance_demos.sh`
把 demo 分成两类，其中一类对 CI 的价值是**零**：

- 无头、纯断言、自己会退出 → RUN_LIST，2 个；
- 开窗 / 要 GPU / 等人输入 → SKIP_LIST，12 个。

`knowledge/handoff/2026-08-28-session-handoff.md` 第四节第 7 条已把
「§15 这条要不要改」登记为待裁定项；本批次是那条的落地。

**同一份交接还记着一条教训**：「P6/P7 都没有 demo 而规格 §15 是硬性要求」
曾被登记为**一次未经记录的规格变更**。本批次不重蹈：规格正文改动同批
写 ADR 0030，把「什么时候、因为什么、由谁裁定」钉在一处。

---

## 一、盘点：14 个 example target，逐个四问

四个问题：① 它断言了什么？② 那些断言在别处有没有等价覆盖？
③ 它是不是某个冻结基准的生产者或消费者？④ 有没有独一无二的能力？

`cargo metadata` 认定的 example target 共 **14** 个（不是 `ls crates/*/examples`
的 16 个文件——p1/p2/p3/p4/p5_coordinate 是目录形式的单 target）。
`cargo test --workspace` 实跑的 example 内 `#[test]` 共 **80** 条
（`test = true` 的 6 个 target：p5_gameplay 1、p1 11、p3 18、p5_coordinate 25、
p4 11、p2 14）。

### 1.1 `ll-content:p5_gameplay_acceptance`（661 行，RUN_LIST）

| 问 | 答 |
|---|---|
| ① 断言了什么 | 职业/分支技能树/副职叠加/网状任务/存档往返，29 处 `assert`，全在 `run_walkthrough()` 里，由 1 条 `#[test]` 调用 |
| ② 别处有无等价覆盖 | **没有**。它把五个玩法子系统串成一条链跑，`ll-mod`/`ll-sim` 的单测各管一段 |
| ③ 冻结基准 | 否 |
| ④ 独一无二 | 是：唯一一条贯穿这五个子系统的端到端链路 |

**处置：搬迁**（无头、有断言、跑得动）。

### 1.2 `ll-content:p5_save_acceptance`（870 行，RUN_LIST）

| 问 | 答 |
|---|---|
| ① 断言了什么 | 存档往返逐位一致（`WorldState::hash`）、缺 mod 按内容类型降级（物品丢弃/NPC 种族占位/玩家种族拒绝降级/只读）、生成期 mod 硬门禁、模式 2→3 单向降级。35 处 `assert`，**全部写在 `main()` 里，`cargo test` 一条都跑不到** |
| ② 别处有无等价覆盖 | `ll-content/tests/e2e_save_cycle.rs`、`fuzz_save_load.rs` 覆盖往返与模糊，但**按内容类型的降级矩阵（b1–b5）没有等价物** |
| ③ 冻结基准 | 否 |
| ④ 独一无二 | 是。且它正是当初促成 `run_acceptance_demos.sh` 诞生的那个 demo（曾在主干上恒定 panic 几个批次无人发现） |

**处置：搬迁**，并把 `main()` 拆成 8 条独立 `#[test]`（step0/section_a/b1–b5/section_c
的顶层函数本来就零参数、各自建夹具，拆开只增加失败隔离度，不改任何断言）。

### 1.3 `ll-ui:p4_acceptance`（4 文件 598 行，SKIP_LIST：开窗渲染 mod 加载界面）

| 问 | 答 |
|---|---|
| ① 断言了什么 | `layout.rs` 3 条：熔岩地形映射到沙地条目并染橙红、自然地形不被误染、建筑地形无展示条目——**全部是 demo 自己那张展示用映射表**。`world.rs` 8 条：真实 `load_all` 管线跑三个 mod 根目录，验熔岩地板注册成功、`brokendependency` 归失败分组、亡灵法师职业经完整管线注册、正常 mod 不被错误 mod 连累、重复命名空间两个都失败、跨引用校验通过、出生点可站立、出生点正东紧邻一格是熔岩 |
| ② 别处有无等价覆盖 | `layout.rs` 3 条：**代码随 demo 一起删，无需覆盖**。`world.rs` 8 条：`ll-mod/src/pipeline.rs` 的单测用**临时目录**造失败样本，**没有任何一处跑仓库根目录下那两个真实夹具目录** |
| ③ 冻结基准 | **是——`crates/ll-ui/tests/visual/baseline/p4_acceptance.png` 的唯一生产者**（按 F2 存图，要 GPU） |
| ④ 独一无二 | 是：`mods_missing_dependency/`、`mods_duplicate_namespace/` 两个仓库根目录夹具**全仓库只有它一个消费者**，删了它两个目录立刻变孤儿 |

**处置：`world.rs` 8 条搬迁到 `crates/ll-ui/tests/mod_load_pipeline.rs`；
`layout.rs` 3 条随代码删除；p4 基准图见第三节。**

### 1.4 `ll-sim:p5_coordinate_acceptance`（5 文件 1992 行，SKIP_LIST：开窗并等 WASD）

| 问 | 答 |
|---|---|
| ① 断言了什么 | `walkthrough_test.rs` 6 条：沿东向走廊连续跨区块无阻挡、出生邻域预热覆盖不到第 3 列、走到未预热区块查地形不 panic（证明真流式加载）、世界地图标记随移动更新、进出 Interior 完整调用链且层属性生效、走过的区块标已探索。`world.rs` 3 条：出生点可站立、Interior 入口在出生点正南 `ENTRANCE_OFFSET_Y` 格且可反查、Interior 四周是挡视线石墙。`layout.rs` 7 条：3 条 demo 自己的地形→条目映射表、3 条 `effective_sight_radius`、1 条小地图版式。`main.rs` 9 条：动画兜底 4 条 + 玩家动画状态机 5 条 |
| ② 别处有无等价覆盖 | `walkthrough_test.rs`/`world.rs`：**没有**，它们直接驱动 `Intent → resolve → Effect → apply` 与 `stream_neighborhood`。`layout.rs` 的 3 条视野半径：`本体注册的地下城profile在正午视野半径小于地表` 用的是真实注册表，**没有等价物**。`main.rs` 9 条：**`crates/ll-render/src/anim.rs` 自己的 28 条测试逐条覆盖了同样的性质**（`帧名在图集里存在时按原样使用`／`缺失时退回兜底帧`／`剪辑数据损坏时退回兜底帧而不是崩溃`／`剪辑数据完好时按剪辑当前帧显示`／`四个方向键任意按住都判定为正在移动`／`电平驱动连续多帧调用同一状态全程不回弹到默认状态`／`松开后立即切回默认状态不拖延`），demo 那 9 条测的是 demo 自己的 `Demo::update_player_animation` 这层接线 |
| ③ 冻结基准 | **否**。`main.rs` 里写着 `"/tests/visual/baseline/p5_coordinate_acceptance.png"`，但**那个文件不存在**（`find` 实证），从来没有被存出来过 |
| ④ 独一无二 | 是：唯一一处程序化走通「跨区块流式加载 + 进出 Interior + 世界地图探索记忆」的链路 |

**处置：`walkthrough_test.rs` 6 条 + `world.rs` 3 条搬迁到
`crates/ll-sim/tests/coordinate_layers_e2e.rs`；`layout.rs` 3 条视野半径搬迁到
`crates/ll-world/tests/space_profile_light.rs`；其余随代码删除。**

### 1.5 `ll-render:p1_acceptance`（3 文件 741 行，SKIP_LIST：开窗渲染）

| 问 | 答 |
|---|---|
| ① 断言了什么 | 11 条：棋盘格相邻格交替 2 条、巡逻三角波 3 条、精灵锚点/绘制原点 3 条、绘制顺序 3 条 |
| ② 别处有无等价覆盖 | 棋盘格与巡逻是 **demo 自己的 `terrain_entry_name`/`hero_patrol_y`**，随代码删。锚点/绘制原点/绘制顺序 6 条**在 `crates/ll-render/src/sprite.rs` 的 11 条测试里被更强地覆盖**——那边锁的是**精确数值**（`assert_eq!(draw_position, [100.0, 192.0])`），demo 这边只锁方向性（`assert!(draw_y < 200.0)`）；平局打破与全序另有 `crates/ll-render/tests/sprite_blackbox.rs` 的三条属性测试 |
| ③ 冻结基准 | **是——`crates/ll-render/tests/visual/baseline/p1_acceptance.png` 的唯一生产者**（按 F2，要 GPU）。该基准 README 已写明「已知过期（玩家贴图重画）」，并留了重拍说明 |
| ④ 独一无二 | 它与 p3 是 `boss_idle_0` 仅存的两个消费者 |

**处置：全删，零搬迁**（11 条无一提供独有的生产代码覆盖）。基准见第三节。

### 1.6 `ll-world:p2_acceptance`（4 文件 1129 行，SKIP_LIST：开窗渲染）

| 问 | 答 |
|---|---|
| ① 断言了什么 | 14 条：地形→条目映射 2、昼夜/四季色调 3、小地图版式 2、精灵换算 3、出生点搜索 2、山脊雕刻 2 |
| ② 别处有无等价覆盖 | 映射/色调/版式/出生点搜索/山脊**全部是 demo 自己的函数**（`crate::layout::ambient_tint`、`crate::spawn::find_spawn`/`carve_wall_ridge`），随代码删。3 条精灵换算同 1.5，被 `sprite.rs` 更强覆盖 |
| ③ 冻结基准 | **是——`crates/ll-world/tests/visual/baseline/p2_acceptance.png` 的唯一生产者**（要 GPU）。该 demo 的 `demo_gen_params()` 把 `climate_band_width` 钉在 0，正是为了让这张基准在气候条带落地后逐格不变 |
| ④ 独一无二 | 否 |

**处置：全删，零搬迁。** 基准见第三节。

### 1.7 `ll-sim:p3_acceptance`（6 文件 2060 行，SKIP_LIST：开窗渲染战斗）

| 问 | 答 |
|---|---|
| ① 断言了什么 | 18 条：demo 内置点阵字体 6、地形/色调/精灵 5、出生与单位生成 5、AI 选择 2 |
| ② 别处有无等价覆盖 | 点阵字体是 demo 自建的（真正的文本渲染在 `ll-text`），随代码删。`ai_intent` 是 **demo 自己的两行 AI**（真正的行为树在 `ll_sim::behavior` + `ll_mod::native_behavior`，各有自己的测试）。出生/生成走 demo 自己的 `build_world`/`spawn_actors`。5 条地形/精灵同 1.5 |
| ③ 冻结基准 | **是——`crates/ll-sim/tests/visual/baseline/p3_acceptance.png` 的唯一生产者**（要 GPU） |
| ④ 独一无二 | **`坦克敌人的占地并非一比一` 这条只锁「敌人用的条目名是 `boss_idle_0`」，footprint 数值本身它明说不在断言范围内。**「footprint 从图集条目读取」这条性质真正的守门人是 `ll-render/src/sprite.rs` 的 `重点目标占四格` 与 `三十二乘四十八双格精灵绘制原点锁定实际坐标`，两条都在生产 crate 里、都锁精确数值 |

**处置：全删，零搬迁。** 基准见第三节。`boss_idle_0` 的身份变化见第四节。

### 1.8 `ll-platform:p0_acceptance`（162 行，SKIP_LIST：开窗等方向键/Esc）

① 零 `assert`；② 无可搬；③ 非基准生产者；④ 无。**全删。**

### 1.9 `ll-text:mixed_text_demo`（260 行，SKIP_LIST：要 wgpu 适配器）

① 零 `assert`，只截图；② 无可搬；③ **是 `crates/ll-text/tests/visual/mixed_text_demo.png` 的唯一生产者**（要 GPU）；④ 唯一一处把中英混排 + Tabler 图标画到原生分辨率离屏纹理的样例。**删 target，基准见第三节。**

### 1.10–1.12 `ll-game:{surface,settlement,npc_roster}_preview`（327/170/244 行，SKIP_LIST）

| 问 | 答 |
|---|---|
| ① 断言了什么 | 各 1 条兜底 `assert`（「一个有图的种族都没有」之类），**产物是给人看的 PNG，不是断言** |
| ② 别处有无等价覆盖 | **有，而且是明写的分工**：`ll-game/tests/surface_render.rs`、`atlas_coverage.rs`、`npc_appearance.rs` 就是这三张图各自「自动化的那一半」（见 `crates/ll-game/tests/visual/README.md`） |
| ③ 冻结基准 | **是——`crates/ll-game/tests/visual/{surface,settlement,npc_roster}_preview.png` 三张的唯一生产者。但它们无需 GPU、无需窗口，纯 CPU 出图** |
| ④ 独一无二 | 是：唯一能把「合成图回退链的实际观感」摆给人看的东西 |

**处置：删 target，基准见第三节（这三张是唯一「三条路都便宜」的一组）。**

### 1.13–1.15 `ll-game:probe_{aistall,conquest,content_hash}`（256/152/56 行，SKIP_LIST）

① 零 `assert`，一次性排查探针；② 无可搬（`probe_aistall` 对应的回归已经是
`crates/ll-game/tests/ai_stall.rs`）；③ 否；④ 无。**全删。**

---

## 二、搬迁清单（断言强度必须不变）

| 从 | 到 | 条数 | 强度变化 |
|---|---|---|---|
| `ll-content/examples/p5_save_acceptance.rs` 的 `main()` | `crates/ll-content/tests/save_acceptance.rs` | 0 → **8** | **变强**：35 条断言此前 `cargo test` 一条都跑不到，只有 `run_acceptance_demos.sh` 跑；现在进 `cargo test --workspace`，且按 8 个独立用例报告失败 |
| `ll-content/examples/p5_gameplay_acceptance.rs` | `crates/ll-content/tests/gameplay_acceptance.rs` | 1 → 1 | 不变（`run_walkthrough` 四节共享同一个 `world`，**刻意不拆**——拆开要重排状态依赖，那才是弱化） |
| `ll-ui/examples/p4_acceptance/world.rs` | `crates/ll-ui/tests/mod_load_pipeline.rs` | 8 → 8 | 不变（`build_demo_world` 连同三次 `load_all` 原样搬过去） |
| `ll-sim/examples/p5_coordinate_acceptance/{walkthrough_test,world}.rs` | `crates/ll-sim/tests/coordinate_layers_e2e.rs` | 9 → 9 | 不变 |
| `ll-sim/examples/p5_coordinate_acceptance/layout.rs` 的 3 条视野半径 | `crates/ll-sim/tests/coordinate_layers_e2e.rs`（**落地时改了落点**，见下） | 3 → 3 | 不变（demo 的 `effective_sight_radius` 包装函数一并搬过去，不改换算） |

**落地时与本表的一处偏差（如实记录）**：三条视野半径断言原计划落
`crates/ll-world/tests/space_profile_light.rs`，实际落在
`crates/ll-sim/tests/coordinate_layers_e2e.rs` 的 `space_profile_tests`
子模块里。理由是另开一个文件就要**复制第二份 `effective_sight_radius`**
——本仓库反复吃亏的正是「真相源之外的副本当判据」。同一个包装函数一处
定义，Interior 验收与这三条共用。断言逐字未改。

合计搬入 **29** 条。

**搬不了的，逐条如实登记**（见第一节 ③④ 与第三节）：

- p1/p2/p3/p4 四张 GPU 冻结基准的**截图动作**——要真实窗口 + 图形适配器，
  `RenderTarget::read_pixels` 拿不到就没有图。测试进程里没有窗口，硬搬只能
  搬成「构造一个 GPU 上下文，拿不到就跳过」，那等于把断言弱化成「跑起来
  不 panic」，**明令禁止**。
- `mixed_text_demo` 的截图同上（`request_adapter` 在无头 runner 上失败）。
- p0 的「按方向键窗口里有反应」——ADR 0025 已经明令禁止用 `SendKeys` 之类
  盲注按键做验收，这条从来就没有可自动化的版本。
- 三张 `ll-game` 预览图的**观感判断**——「矮人铁匠和人类渔夫一眼分得出不是
  同一个人」这句话没有机器版本；机器版本（「两两之间至少差四分之一像素」）
  早就在 `npc_appearance.rs` 里，且**继续存在**。丢的是人眼那一半。

---

## 三、冻结基准：**出现了「唯一生产者被删」，八张，分两类**

**硬约束要求停下来说清楚、给三条路的代价、不要自己选。本节即是。**

### 3.1 四张 GPU 冻结基准（+1 张文本截图）

| 基准 | 唯一生产者 | 现状 |
|---|---|---|
| `crates/ll-render/tests/visual/baseline/p1_acceptance.png` | `ll-render:p1_acceptance` 按 F2 | README 已标「已知过期」，等有 GPU 的机器重拍 |
| `crates/ll-world/tests/visual/baseline/p2_acceptance.png` | `ll-world:p2_acceptance` 按 F2 | 有效（`climate_band_width=0` 钉住） |
| `crates/ll-sim/tests/visual/baseline/p3_acceptance.png` | `ll-sim:p3_acceptance` 按 F2 | 有效，图里画着 `boss_idle_0` |
| `crates/ll-ui/tests/visual/baseline/p4_acceptance.png` | `ll-ui:p4_acceptance` 按 F2 | 有效（只覆盖世界层，文字面板读不回） |
| `crates/ll-text/tests/visual/mixed_text_demo.png` | `ll-text:mixed_text_demo` | 有效 |

**三条路与代价**（**不由本批次选**）：

1. **保留那一个 example**（例如只留 p1/p2/p3/p4/mixed_text 五个 target）。
   代价：所有者裁定「去掉 example」只执行了一半；`run_acceptance_demos.sh`
   的 SKIP_LIST 要继续维护；这五个 target 合计约 4700 行只为「有朝一日
   有 GPU 时能重拍一张图」而留在工作区，且它们编译进每一次 `cargo test`
   与 `cargo clippy --all-targets`（`ll-game` 那个测试二进制的链接器 OOM
   问题会更容易撞上）。
2. **把生成逻辑搬成测试**。代价：**做不到而且危险**。这五张图都要真实
   GPU（前四张还要真实窗口 surface）；无头 CI 上 `request_adapter` 会失败，
   测试只能写成「拿不到适配器就 skip」——那是一条**永远绿、永远不咬人**
   的假测试，正是 ADR 0018 反例验证要根除的东西。
3. **放弃那张基准**（连 PNG 一起删）。代价：这是唯一一次「渲染管线看起来
   应该是什么样」的留档，`docs/qa/04-覆盖率与缺失测试层.md` 把 L7 视觉回归
   列为「有基准生成机制、没有自动比对」的已知缺口并给了补法（lavapipe /
   WARP 无头 GPU）；删图等于把那条路的起点也删掉。

**本批次的做法（最保守、最容易反转，见第六节）**：
**PNG 一张不删**，删 target，并在每个 `tests/visual/README.md` 里写明
「唯一生产者已于某提交删除，恢复方式：`git show <hash>^:<路径>`」。
git 里逐字节留着，任何一条路都还走得通；本批次不替所有者做选择。

### 3.2 三张 `ll-game` 预览图（**这一组三条路都便宜**）

`surface_preview.png` / `settlement_preview.png` / `npc_roster_preview.png` 的
生产者**不需要 GPU、不需要窗口，是纯 CPU 出图**。因此对这一组：

1. 保留三个 example target——代价最小的一路（741 行），但与裁定相悖；
2. 把出图逻辑搬成 `#[test]`——技术上可行，但要**新造一套「比对基准 PNG
   + 环境变量覆盖」的机制**，本批次范围之外，且这三张图的机器版判据
   （`surface_render.rs`/`atlas_coverage.rs`/`npc_appearance.rs`）**已经存在
   且不受影响**；
3. 放弃三张图——丢的是人眼那一半（「一眼分得出不是同一个人」）。

同样**不由本批次选**，做法同 3.1：图留着，target 删，README 写清恢复方式。

### 3.3 `assets/atlas/placeholder.png` / `placeholder.json`：**不受影响**

它是被点名的冻结基准之一，但它的生产者是 `tools/ll-artgen`
（`generate_legacy_shared_atlas`），**不是任何 example**。删 example 之后它的
**消费者**从「5 个 demo + ll-artgen 自己的测试」变成「只剩 ll-artgen 自己的
测试」。图与 JSON 逐字节不动，仍可随时重新生成。这一条要写进
`assets/atlas/README.md`（见第四节）。

---

## 四、善后

### 4.1 `scripts/ci/run_acceptance_demos.sh`：**保留成更弱的形式，并改名**

**判断：不删。** 理由是这个门禁**有两件事，只有一件失去了对象**：

- 「真的运行 RUN_LIST 里的 demo」——失去对象（RUN_LIST 变空）；
- 「工作区每一个 example target 都必须被显式分类，否则报错」——**这一半
  才是它自己文档里写的『本门禁真正的价值所在』**，而且现在它的价值更大了：
  所有者刚裁定去掉 example，如果将来有人不声不响加回一个，必须当场变红，
  而不是等下一次盘点。

**改成**：`scripts/ci/check_no_examples.sh` —— 用同一套 `cargo metadata`
枚举，断言 example target 集合为**空**，非空就列名报错并指向 ADR 0030。
`run_all.sh` 与 `.github/workflows/ci.yml` 两处调用点同步改。

**为什么改名**：一个叫 `run_acceptance_demos.sh` 却一个 demo 都不跑的脚本，
正是本仓库反复吃亏的那种「名字与真相分叉」（`skin.rs` 查裸贴图名、
`atlas_coverage.rs` 手写地形清单、`composite_keys` 少一个冒号）。改名是
两行调用点的事，代价远小于留一个骗人的名字。

### 4.2 规格 §15 与 §17

- §15 那句硬性要求改成：**验收由「本体二进制实机试玩 + 分层自动化测试」
  承担，不再要求 `examples/` demo**，并在同一行加 `[2026-08-29 所有者裁定]`
  标记指向 ADR 0030（仓库惯例：正文可改，但必须留裁定来由）。
- §17 风险登记「逐层铺地基导致长期无可见产出」那一行的缓解措施同步改，
  **风险等级不动**（裁定改的是缓解手段，不是风险本身）。
- 新写 `knowledge/decisions/0030-remove-examples-acceptance-demos.md`：日期、
  裁定人、原文、被推翻的是哪一句、代价（八张基准的唯一生产者随之消失）、
  三条路留给谁。

### 4.3 `assets/atlas/README.md`

- 「`boss_idle_0` 的去留」整节失效——它的身份写着「**两个验收 demo 的测试
  夹具**」，两个 demo 都没了。改写成：现在它在 `crates/` 下**零消费者**，
  只是遗留共享画布与松散贴图树里各一条没人查的条目；所有者「现在应该不太
  需要 boss」的裁定不变，处置仍是留图；但**它不再有任何「测试夹具」身份**。
- 「五个更早批次验收 demo 的冻结像素基准」这个说法在多处出现
  （本文件、`tools/ll-artgen/src/{npc,composite,furniture}.rs`）——那些
  demo 已删，但**「不要往那张画布里塞新内容」这条纪律的理由要重写而不是
  删除**：四张基准 PNG 还在，画布一动它们就再也对不上，而现在**连重截的
  生产者都没有了**，理由只增不减。
- 「地形色块（P2 新增）」等节里「`crates/ll-world/examples/p2_acceptance/`
  需要把它们画成能用肉眼区分的颜色」这类现在时表述，改成过去时并注明来由。

### 4.4 其余失效引用

`crates/ll-game/src/app.rs`、`crates/ll-game/tests/visual/README.md`、
`crates/ll-render/tests/visual/README.md`、`docs/qa/04-*.md`、
`tools/ll-artgen/src/*.rs`、`ll-render/src/sprite.rs` 等处提到 example 的
注释/文档，逐条 grep 后更正——`scripts/ci/check_doc_links.sh` 与
`check_markdown_links.py` 会替我们抓断链，但**Rust 注释里的裸路径它们抓不到**，
必须手工 grep `examples/`。

---

## 五、执行顺序与提交切分

1. **提交 1（盘点）**：本文件。
2. **提交 2（搬迁）**：新增 5 个测试文件，**example 一个都还不删**——
   这一步跑完门禁应当是 2768 + 29 = **2797**（搬入的与 example 里的并存），
   证明搬过去的确实跑得起来、且没有靠 example 里的任何东西。
3. **提交 3（删除）**：删 14 个 example target、6 个 `[[example]]` 段、
   `crates/*/examples/` 全部目录。
4. **提交 4（善后）**：门禁改形改名 + 规格 §15/§17 + ADR 0030 +
   `assets/atlas/README.md` + 各 `tests/visual/README.md` + 失效引用。

不 push、不合并 main。

### 预期测试数

| 项 | 数 |
|---|---|
| 改前基线 | 2768 |
| 删掉的 example 内 `#[test]` | −80 |
| 搬入 `tests/` | +29 |
| **预期改后** | **2717** |

**实跑结果与预期逐位相同**：搬迁提交后 **2797**（= 2768 + 29，两侧并存），
删除提交后 **2717**。

**净减 51，且必须能逐条解释**：这 51 条测的全是**随 demo 一起删掉的
demo 自有代码**（demo 的棋盘格、巡逻三角波、点阵字体、两行 AI、
自己那张地形→条目映射表、自己的出生点搜索与山脊雕刻、自己的动画接线层），
以及 **9 条被生产 crate 里更强的测试逐条覆盖**的精灵换算/绘制顺序断言。
**没有任何一条生产代码的覆盖被删掉**——这一点在第一节 ② 列里逐个 target
核实过。同时，`p5_save_acceptance` 的 **35 条断言从 `cargo test` 之外搬进了
`cargo test` 之内**，测试函数数下降但被 `cargo test` 执行的断言数上升。

---

## 六、规格没裁定、本批次临时选的做法

按纪律「选最保守、最容易反转的做法继续做完，报告里单列」：

1. **冻结基准 PNG 一张不删**（第三节）。所有者只说了「去掉 example」，
   没说图。删 target 是裁定的直接内容，删图不是。
2. **`run_acceptance_demos.sh` 保留成更弱的形式而不是删除**（4.1）。
   删掉最省事，但会把「不许悄悄加回 example」这条保护一起删掉。
3. **`p5_gameplay_acceptance` 不拆成多条测试**（第二节）。拆开更好看，
   但四节共享同一个 `world`，重排状态依赖有弱化断言的风险。
4. **`boss_idle_0` 留图**。所有者早前的裁定是「现在应该不太需要 boss」+
   处置「留图」，那条裁定的**理由**（删它要重冻 p3 基准）今天变了，
   但**结论**没有被重新裁定。留图是可反转的一侧。
5. **`mods_missing_dependency/`、`mods_duplicate_namespace/` 两个根目录夹具
   保留**——因为 p4 的 8 条断言被搬进了 `ll-ui/tests/`，夹具仍有消费者。
   若那 8 条将来也被认为不该留在 `ll-ui`，夹具的归宿要重议。
