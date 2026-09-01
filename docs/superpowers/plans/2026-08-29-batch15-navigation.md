# 批次 15：导航收敛 + 鼠标与按钮

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，全文 0 次出现「反例」。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**基线**：`main` HEAD `d5e8bf1`（工作树 `wt-navigation`，分支 `wt-navigation`）。
**改前测试数**：2731 通过（`bash scripts/ci/run_tests.sh`，114 个 `test result: ok`）。
**两条黄金基准**（本批**预期不变**，值自己 grep，不在本文抄）：
`crates/ll-world/tests/determinism.rs` 的 `EXPECTED_WORLD_DIGEST`、
`crates/ll-sim/tests/replay.rs` 的 `EXPECTED_REPLAY_DIGEST`。理由见
`knowledge/handoff/2026-08-28-session-handoff.md` 第〇节：文档里存一份会漂移的
副本，三次事故同一个形状。

**规格**：`knowledge/design/ui-and-navigation.md` §7 / §10 的「导航收敛批」——
**N8 + N7 + N2 + N1 + N10 五条一次做完**（§10 原话：N2 依赖 N8 的统一栈，拆开做
等于把同一处代码改两遍）。
**所有者裁定**：`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节第 1 条
（标题屏预选第一项 + 要有按钮选项 + 鼠标点击有反应），以及同节「其余记在案的债」
里那条「接鼠标要一并定焦点由谁驱动、hover 改不改焦点」。

---

## 一、根因与收敛目标

规格 §0 已经诊断清楚：**三套互不知情的模态系统**——`Demo::screen`（进
`UiModeStack`、切输入上下文）、`Demo::menu`（不进）、`Demo::world_map_open`
（不进）。后果是 Esc 被实现了两遍、地图那套一遍都没有，方向键同时驱动菜单光标与
地图平移。

**收敛目标**：让「现在开着哪一层模态」只有一个真相源，并且让「两套模态各自记
各自的」**在编译期就写不出来**。

### 做法：把四个字段封进一个私有字段的 `Modal`

新增 `crates/ll-game/src/modal.rs`，`pub struct Modal` **私有**持有四样东西：

| 字段 | 回答什么 |
|---|---|
| `stack: UiModeStack` | 现在盖着几层、最上面那层是哪一类、输入上下文查哪张表 |
| `screen: Option<ScreenState>` | 模态屏那一层里具体在显示什么 |
| `menu: PlayerMenu` | 玩家菜单那一层里具体开着哪块、光标在第几行 |
| `world_map_open: bool` | 地图那一层开没开 |

`Demo` 只持有 `modal: Modal`，四个字段**在 `modal.rs` 之外不可写**。于是
`self.world_map_open = true` 这种「只改自己那一份、栈不知道」的写法**编译不过**
——这与本会话另外三处先例（存档时重算生成期 mod 集合、肉鸽反向升级、新世界配
老槽位）同一种解法：**写不出来**比**提醒一下**可靠。

`modal.rs` 内部每个改动方法末尾调一次一致性断言：栈里的层与三个状态字段逐条
配对（地图开 ⇔ 栈里恰有一层 `Overlay`，屏开 ⇔ 恰有一层 `Menu`，玩家菜单开 ⇔
恰有一层 `PlayerMenu`，命名屏 ⇔ 顶上再加一层 `TextEntry`）。

**`PlayerMenu` 的可变借用是唯一的缺口**：`player_command` 收 `&mut PlayerMenu`
并在内部开关它。因此不提供 `menu_mut()`，只提供
`Modal::with_player_menu(&mut self, input, f)`——`f` 拿到 `&mut PlayerMenu`，
返回之后由 `with_player_menu` 负责把栈重新对齐。拿不到裸引用 ⇒ 绕不过对齐。

### `UiMode` 新增两个变体

`UiMode::PlayerMenu` 与 `UiMode::Overlay`，两者的 `current_context()` **仍然
映射到 `InputContext::Gameplay`**（规格 N8 明写：不推翻
`player_action.rs` 那条「键位表不该换」的裁定，只进栈）。

**顺带把 `push`/`pop` 的清键改成有条件**：今天两者无条件
`InputState::clear()`；清键的**理由**是「上下文切换那一刻按住的键会带过边界」
（`ui_mode.rs` 模块文档），那么没有发生上下文切换就不该清。改成「
`current_context()` 真的变了才清」——对今天已有的四种转移
（空→Menu、Menu→TextEntry、TextEntry→Menu、Menu→空）**逐条等价**，且新变体
（Gameplay→Gameplay）不再误清玩家正按着的方向键。

---

## 二、五条各自的落点

### N8 统一栈
- `crates/ll-ui/src/widget/ui_mode.rs`：加 `PlayerMenu`/`Overlay` 两个变体；
  `push`/`pop` 改成上下文真的变了才 `clear()`。
- 新增 `crates/ll-game/src/modal.rs`（见上）。
- `crates/ll-game/src/app.rs`：`Demo` 的四个字段换成一个 `modal`；
  `advance` 里地图的早退判据挪到**模态屏判据之后**（规格 N8 落点第 5 条）。

**判据（规格给了四条）**：
1. 地图开着按 Esc → 地图关掉，**不开菜单**（今天会红）。
2. 地图开着按方向键 → 只有地图动；菜单开着按方向键 → 只有光标动；两者不同时。
3. 背包开着时 `modal.depth() == 1`（今天是 0）。
4. 任意一块 UI 关掉后 `modal.is_empty()`（防 push/pop 不配对）。

### N2 取消键只退一层
`app.rs` 顶层那条判据从
`screen.is_none() && !menu.is_open() && Cancel` 改成 **`modal.is_empty() && Cancel`**
——「没有层就开菜单，有层就交给栈顶那一层自己退一层」。地图那一层的取消处理
新增在 `advance` 的地图段（今天没有）；玩家菜单那一层的在 `player_action.rs`
（见 N7）；模态屏那一层的各屏本来就有。

### N7 交互列表退回方向列表
`PlayerMenu::Interact` 加 `from_direction: bool`（**不存上一级状态**——方向列表
的内容由 `interact_tiles(world, pos)` 现算，重进一次自然重算，ADR 0009
「默认派生、只存偏差」）。取消时的方向列表光标同样**派生**：在
`interact_tiles` 的结果里找 `pos` 的下标。`player_action.rs` 那段无条件
`Closed` 改成走 `cancelled_menu(menu, world, pos)`。

### N1 设置屏按确认不改数值
`menu_screen.rs` `update_navigation` 末尾那个
`other => { adjust_value(other, ctx, true); ... }` 改成 idle。
数值一律左右键改。

### N10 列表进入时预选第 0 行
`app.rs` 四处建/重建 `screen_focus` 的地方（构造在首页、`open_menu`、
`back_to_title`、死亡回角色创建）改成预置第 0 项——**复用
`ll_ui::widget::focus::move_focus(.., Next)` 在空表上的既有行为**（冷启动
`Next`→0），不新造「预置焦点」的第二套逻辑。

**那段过期文档要重写不要删**（规格 §13：文档与代码不一致即视为缺陷）：
`app.rs` `open_menu` 上面那段「焦点刻意不预置」的论证，改写成「所有者
2026-08-29 裁定预选第一项 + 原论证的成立条件（当时只有键盘）已经不成立」。

---

## 三、任务 B：鼠标点击与按钮

### 已有的地基（不重造）
- `crates/ll-ui/src/widget/hit_test.rs`：命中测试，**后来居上**与绘制序对齐。
- `crates/ll-platform/src/input.rs`：`cursor_position` / `was_mouse_just_pressed`
  / `was_mouse_just_released`（P7 落地）。
- `crates/ll-ui/src/widget/button.rs`：`update_button` 的
  **按下武装 / 松开且仍悬停才触发**语义。
- `app.rs` 的 `clicked_spawn_zone`：本仓库已有的一处生产鼠标点击路径，
  「没有 `resources` 就没有窗口尺寸、鼠标一律不生效」这条降级由它先立过。

### 四条新约定（所有者点名要一并定）

| # | 问题 | 裁定 | 理由 |
|---|---|---|---|
| 1 | 鼠标移过一项，键盘焦点跟不跟着走 | **不跟**。hover 只改**外观**，不改焦点 | 跟着走会出现「键盘走到第 3 项、手碰了下鼠标、焦点跳回第 1 项」。而「高亮在 A、点击生效在 B」这个反面代价由第 3 条堵住：**按下**那一刻焦点就跳到指针所在行，玩家在触发之前一定先看到高亮跟过去了。仓库既有形状也支持这条：`WidgetState` 的 `hovered` 与 `focused` 本来就是两个字段 |
| 2 | 点击落在没有条目的空白上 | **什么都不做**：不改焦点、不触发、不关屏 | 最保守、最容易反转。「点空白关掉模态」会与 N2「取消键只退一层」多出第二条退层路径，而在角色创建/世界配置这类多步流程上误关一层的代价是玩家白填一遍 |
| 3 | 点中的正好是已经用键盘选中的那一项 | **确认**（与点别的项完全一样） | 「按下=聚焦，松开=确认」是同一个手势的两半。若已聚焦的那项要点两下，玩家的行为就取决于一条他看不见的历史状态 |
| 4 | 拖动/按住 | **按下与松开必须落在同一行**才算一次点击；按下在 A、松开在 B 或空白 → 不触发（焦点留在 A） | 直接沿用 `widget/button.rs` 已经写死的桌面手感，不发明第二套 |

**「按钮选项」怎么落**：模态屏的**每一行本身就是按钮**——可命中、可点击、
聚焦行与悬停行各画一块高亮矩形。**不另起一列 `Button` 控件**：那会得到「一份
行文字 + 一份按钮清单」两份迟早分叉的清单，正是本仓库反复付过代价的形状。
光标前缀 `"> "` 原样保留（换掉它是 W7，属下一批）。

### 落点
- `crates/ll-ui/src/widget/hit_test.rs`：`hit_test` 泛型化（`WidgetId` → `T`），
  这样「第几行」也能走**同一个**命中测试，不写第二份。
- `crates/ll-ui/src/screen/mod.rs`：`ScreenContent` 加 `row_rects` 与两块高亮；
  `build_screen_panel` 与新增的 `screen_row_rects` 共用同一段布局算法
  （一个内部 `layout_screen`，两个入口）。
- 新增 `crates/ll-game/src/pointer.rs`：`RowPointer`（`Idle`/`Focus`/`Activate`）
  与 `PointerState`（跨帧记住「按下是从哪一行开始的」+ 当前悬停行）、
  纯函数 `resolve_row_pointer`。
- `app.rs`：新增 `viewport: Option<(f32, f32)>`，由 `on_resume`/`on_resize`
  维护（**先记窗口尺寸，再更新 GPU 资源**——尺寸是窗口的事实，不是 GPU 的）。
  `update_screen` 之前算一次 `RowPointer`，传给各屏的 `update_*`。
- 六块行列表屏各加一个 `pointer: RowPointer` 参数：首页、暂停菜单、存档列表、
  设置、角色创建、世界配置。

**不接的两块，写明理由**：`SpawnPick` 的「屏」是整张世界地图，它**已经有**
鼠标点击（`clicked_spawn_zone`）；`SaveNaming` 是文本输入屏，没有可触发的行
（它的行是「当前名字」与提示），点它没有可对应的动作。

### ADR 0025 与可测边界
**不合成任何操作系统级按键、也不合成任何操作系统级鼠标事件。** 可测的四段全部
走普通 `#[test]`：
1. **命中测试**：`hit_test` 泛型化后对 `row_rects` 的逐行断言（`ll-ui`）。
2. **行矩形布局**：`screen_row_rects` 与 `build_screen_panel` 同源，断言
   第 i 行的矩形正对第 i 行文字（`ll-ui`）。
3. **焦点状态机**：`resolve_row_pointer` 对「按下/松开/拖出/空白」四种序列的
   断言，输入是**直接构造的 `InputState`**（`set_cursor_position` /
   `mouse_press` / `mouse_release` 是既有的公开构造 API，`input.rs` 自己的
   单元测试就在用）——这与 ADR 0025 禁止的「操作系统级事件盲注」是两件事：
   这里驱动的是**真实鼠标最终也要走的同一条 `InputState` 路径**，正是
   ADR 0025 要求的那种程序化驱动。
4. **屏级联动**：`update_title`/`update_menu` 等收到 `RowPointer::Activate(i)`
   之后走的分支，与键盘确认走同一条。

**测不到的一段（如实记录）**：从 winit 的 `CursorMoved`/`MouseInput` 事件到
`InputState::set_cursor_position`/`mouse_press` 这一段**平台事件回调**，以及
「窗口坐标与 `Rect` 坐标系是否真的同一套」——前者要真实事件循环，后者要真实
窗口。本批不开窗、不截图（ADR 0025 + 本仓库无 GPU 测试的既有纪律）。这一段
今天由 `clicked_spawn_zone` 那条已在生产路径上跑着的同构代码间接背书。

---

## 四、不做什么

- **P0 第三组（W1/W2/F1/F5 布局与文本）不做**——下一批。F1（关不上的门的提示）
  的落点是 `interact_command` 的关门分派，**不在本批改动的取消/导航路径上**，
  因此不顺手做，报告里单列。
- **N4（栈深上限 3）不做**：新变体落地后地图 + 玩家菜单 + 屏 + 文本输入理论上
  能到 4 层，此刻加一条「超 3 就拒绝压入」会让 `Modal` 的配对不变式当场被自己
  破坏。留给规格 N4 那一批连同「哪些层能叠」一起裁。
- **N13（Tab 关菜单、F2 任意上下文截图）不做**：P2。
- 不新增 example target（ADR 0030），不动存档形状，不新增用户可见文案
  （本批全部改动都是行为与几何，零新 Fluent 键）。

---

## 五、ADR 0018 反例验证计划

规格 §12 硬要求。本批**特别容易出**的那个形状：断言测的是新方法本身，而漏斗里
那行调用删掉也不红（本会话的菜单批次在这里被抓过一次）。因此每条新断言的反例
**改的是漏斗里那行调用**，不是被调用的那个函数：

| 断言 | 故意改坏的地方 |
|---|---|
| 地图开着按 Esc 关地图 | 删掉 `advance` 地图段里那条取消分支 |
| 地图开着方向键不驱动菜单 | 把地图早退判据挪回模态屏判据之前 |
| 背包开着栈深为 1 | 把 `with_player_menu` 里的对齐调用删掉 |
| 关掉后栈必空 | 同上 |
| 交互列表退回方向列表 | 把 `cancelled_menu` 的调用换回无条件 `Closed` |
| 设置屏确认不改数值 | 把那条 idle 改回 `adjust_value` |
| 首页不按方向键直接确认能进角色创建 | 把 `Demo::new` 里的预置焦点那一行删掉 |
| 点击第 i 行触发第 i 行 | 把 `update_screen` 里传 `pointer` 的那一行改成恒传 `Idle` |
| 按下 A 松开 B 不触发 | 把 `resolve_row_pointer` 里「同一行」那条判据删掉 |

---

## 六、提交划分

1. `feat: 三套模态收敛进一套栈（N8/N2/N7/N1/N10）` —— 五条一次落地，理由见规格 §10。
2. `feat: 模态屏行接上鼠标点击与聚焦/悬停高亮` —— 任务 B，含四条新约定写进规格。
