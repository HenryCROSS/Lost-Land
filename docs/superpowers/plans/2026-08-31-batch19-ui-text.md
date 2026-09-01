# 批次 19：布局与文本（UI 规格 P0 第三组 W1 / W2 / F1 / F5）

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，正文与反例验证无关（`grep -c 反例 knowledge/decisions/0018-*.md` 在 0018 末尾追加订正节之前为 0）。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**基线**：`main` 的 `7d7a5e7`（`git worktree` 分支 `wt-uitext`）。
**规格**：`knowledge/design/ui-and-navigation.md` §6（L0–L5）、§8（W1–W7）、§9（F1–F7）、§10。
**上一批**：`docs/superpowers/plans/2026-08-29-batch15-navigation.md`（导航收敛 + 鼠标，
建了 `Modal`、`pointer.rs`、行矩形与行文字的单一产出点，本批建在它之上）。
**并行批次**：`wt-dlgcontent` 正在改 `crates/ll-mod/` 的内容表、`mods/*/dialogues.json5`
与两份 `.ftl` 里**新增**的对话条目。本批只改 `.ftl` 里**既有**条目并追加三条反馈键，
尽量不与它落在同一段。

**改前基线（本地实跑，`bash scripts/ci/run_tests.sh`）**：`EXIT=0`，118 个测试二进制，
**2819 条通过、0 条失败**。

---

## 一、本批要解决的核心事实（不重复规格，只记落地判断）

规格 §8 已用内嵌思源黑体的**真实字形宽度**量过：英文比中文宽 1.44 倍（254 个 UI 键
实测比值 0.694），十处溢出里八处只在英文下出现。根因两条：

1. `crates/ll-ui/src/hud/render.rs` 的断行宽度写死 `400.0`，**与六块面板的内容宽
   （608/248/208/208/348/408）没有一块对得上**；
2. `crates/ll-ui/src/widget/list.rs` 的 `RowCursor::push` 按**标签条数**推进纵坐标，
   不是按**渲染后的行数**——于是任何换行同时造成「第二行压住下一行」与「面板背景矮
   一行」。

**行号自己 grep 复核过**（2026-08-31，`7d7a5e7`）：

| 事实 | 位置 |
|---|---|
| 写死的断行宽 `400.0` | `crates/ll-ui/src/hud/render.rs:709` |
| 六块面板宽常量 | `hud/render.rs:88`(620) `:90`(260) `:92`(220) `:94`(220) `:109`(360) `:112`(420) |
| `RowCursor::push` 按条数推进 | `crates/ll-ui/src/widget/list.rs:42-51` |
| HUD 面板高度按 `cursor_y()` 现算 | `crates/ll-ui/src/hud/mod.rs:114-131` |
| 模态屏高度按 `probe.len()` 现算 | `crates/ll-ui/src/screen/mod.rs:196` |
| 模态屏断行宽用的是 `SCREEN_WIDTH`(520) 而不是内容宽(500) | `crates/ll-ui/src/screen/render.rs:183-192` |
| 关门的两条前置（人 / 立着的东西） | `crates/ll-sim/src/resolve/portal.rs:127`、`:130-136` |
| `Feedback` 三变体 | `crates/ll-game/src/player_action.rs:245-261` |
| 关门意图的派发点 | `crates/ll-game/src/player_action.rs:812-820`（`InteractTarget::Door`） |
| 已经存在但零调用点的测量 API | `crates/ll-text/src/layout.rs:64`、`crates/ll-text/src/render.rs:111` |

---

## 二、W1：面板宽度成为唯一真相源

### 2.1 「测量」这件事要有一个能在无 GPU 环境里跑的入口

`ll_text::layout::layout_text` 已经是纯 CPU 的（只要 `FontSystem` + `FontCatalog`），
但今天唯一的持有者是 `TextRenderer`，而它要 `wgpu::Device`。测试与门禁都不能建 GPU。

**落点**：`crates/ll-text/src/measure.rs` 新增

```rust
pub struct TextMetrics { pub line_count: usize, pub max_line_width: f32 }
pub trait MeasureText { fn measure_text(&mut self, text: &str, font_size: f32,
                                        line_height: f32, max_width: f32) -> TextMetrics; }
pub struct TextMeasurer { /* FontSystem + FontCatalog */ }
```

`TextMeasurer`（纯 CPU）与 `TextRenderer`（复用它自己那份 `FontSystem`，**不再建
第二份**）各实现一次 `MeasureText`。产品路径走 `TextRenderer`，测试与门禁走
`TextMeasurer`——**同一个 `layout_text`，不存在两套度量**。

### 2.2 断行宽度从面板宽度派生，一路带到每一个 `Label`

今天 `Label` 只有 `text/x/y`，断行宽度是提交给 GPU 时**才**补上的一个与面板无关的
参数（`hud/render.rs:709` 的 `400.0`、`screen/render.rs:188` 的 `SCREEN_WIDTH`）。

**改法**：`Label` 增加 `max_width: f32` 字段，`to_text_run` 不再收这个参数、改用
`self.max_width`。这条字段的**唯一**写入点是 `RowCursor`，而 `RowCursor` 的断行宽度
由 `build_panel` / `build_screen_panel` 从**面板宽度**算出来：

```
content_width = panel_width - 2 * padding
```

于是「面板宽度」是唯一真相源，断行宽度是它的派生值，`400.0` 那个常量删掉。
**任何人再想写死一个断行宽度，都得先绕过 `RowCursor`**——这就是把纪律变成结构。

### 2.3 模态屏面板宽度按内容伸缩（W3 的模态屏那一半，F5 的判据）

F5（P0）要求「每块屏底部那一行提示必须完整可见」，它的判据规格明写「见 W3」。
W3 对模态屏采纳**面板伸缩**：

```
panel_width = max(SCREEN_WIDTH, 本屏全部文本的最长渲染宽 + 2 * SCREEN_PADDING)
```

**每屏算一次、进屏时固定**——规格 §8.5 W3 已经论证过「现算会随光标跳变」那条反对
理由针对的是**行**，而行从来不是问题；这里算一次的输入是「这一屏的全部行」，光标
移动不改变行的集合，因此不跳变。`row_rects` 与 `screen_row_rects` 共用同一个宽度
算法（今天已经是同一个 `centered_origin`），点击与绘制因此不可能对不上。

---

## 三、W2：纵向推进按实际渲染行数

`RowCursor::push` 改成：先用 `MeasureText` 量这一行在 `content_width` 下断成几行，
`Label.max_width` 记下断行宽，`cursor_y += line_count * row_height`。

`RowCursor` 因此持有 `&mut dyn MeasureText`（生命期参数 `RowCursor<'m>`），
`build_panel` / `screen_lines` / `build_screen_panel` / `build_hud_frame` /
`render_hud` / `build_screen_frame` / `render_screen` 逐层加一个 `measure` 参数。
`ll-game` 侧传自己已经持有的 `TextRenderer`。

面板高度的两处现算点（`hud/mod.rs` 的 `cursor_y()`、`screen/mod.rs` 的 `probe.len()`）
自动跟着对——前者本来就读游标，后者改成读游标（不再数标签条数）。

---

## 四、F1：关不上的门要说清楚是被什么挡住的

### 4.1 文案取所有者原话的**两条**，不是规格 §9.2 收敛后的一条

- 所有者裁定（`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节第 6 条）：
  「门口**有人**挡着」「门口**立着东西**」，中英各一——**两条**。
- 规格 §9.2 F1 把它收敛成一个 `Feedback::DoorBlocked`、一句「门口有东西挡着」。

两者冲突时取所有者原话：结算层本来就是**两条独立前置**（`portal.rs:127` 站着人 /
`:130-136` 立着家具），合成一条反而把已经分开的信息丢掉。因此落两个变体、两对文案。
**按纪律第 9 条，这条更正要写回规格 §9.2 F1 原地，并互相指向。**

### 4.2 判据不抄，改成两边共用同一个函数

规格说「输入层自己就能答」，但**照抄一遍**就是 ADR 0021 点名的形状。改法：

`crates/ll-sim/src/resolve/portal.rs` 提取

```rust
pub enum DoorCloseBlocker { Occupant, PlacedObject }
pub fn door_close_blocker(world: &WorldState, door_pos: TorusPos, actor: EntityId)
    -> Option<DoorCloseBlocker>;
```

`resolve_close_door` 自己改用它（前置 4a/4b 原地替换，行为逐字不变），
`ll_game::player_action::interact_command` 在派发 `Intent::CloseDoor` **之前**调它。
**一份实现两个调用点，分叉不可能发生。**

`interact_command` 今天拿不到 `&WorldState`——加一个 `world: &WorldState` 参数
（调用点 `player_action.rs:712` 已经有 `world`）。

**判据**：门口站一个 NPC，按关门 ⇒ `PlayerCommand::Rejected(DoorBlockedByOccupant)`
且世界时钟不前进（输入层拒绝 ⇒ 不产 `Intent` ⇒ 不消耗回合）。立着家具那条同形。

### 4.3 i18n

`assets/locales/{en,zh-CN}.ftl` 各加两条：

```
hud-feedback-door-blocked-occupant / hud-feedback-door-blocked-object
```

**追加在既有 `hud-feedback-*` 段末尾**，与 `wt-dlgcontent` 新增的对话条目不在同一段。

---

## 五、溢出门禁

### 5.1 判据

> **对 `assets/locales/{en,zh-CN}.ftl` 里的每一个 UI 键 × 两种语言，按它所属面板的
> 内容宽渲染，断言渲染出的行数 ≤ 该面板为这一类内容声明的行数预算。**

选它而不是「宽度 ≤ 内容宽」的理由：W1/W2 落地之后**断行宽度恒等于内容宽**，
「渲染宽 ≤ 内容宽」变成 cosmic-text 自己保证的恒真命题（除非出现一个宽过整块面板
的不可断长单词），当门禁没有牙。真正会退化的是**行数**：一条一行的按钮标签变成
两行、一句提示变成三行，是玩家看得见的糙。行数预算因此是有牙且稳定的判据。

同时保留一条**结构不变式**测试（更强、零映射表）：把真实 `assets/locales` 喂给
`build_hud_frame` / `build_screen_frame`，断言**每一个产出的 `Label` 的
`max_width` 恰等于它所在面板的内容宽**，且量出来的包围盒落在面板内边距之内。
这条一旦有人再写死一个断行宽，立刻红。

### 5.2 「键 ↔ 面板」映射怎么解决

**能现取的现取**：面板内容宽全部从 `ll-ui` 的公开常量现算（`STATUS_WIDTH` 等），
门禁里**不出现任何像素字面量**。

**取不到的那一半**：一个键会被画进哪块面板，今天在代码里根本不是静态可达的——
`item-*-display_name` / `equip_slot-*-display_name` 是经数据表的 `display_name_key`
字段在运行期解析出来的，没有任何静态调用图能给出这层映射。因此**按前缀分类**，
并做成**「缺一条就红」**：`en.ftl` 里出现的每一个键都必须命中恰好一条分类规则，
**未命中即 fail**。新加一个前缀的人会被门禁挡下、被迫声明它画在哪儿；多写一条规则
则只会让规则表出现一条没有键匹配的死规则——这一条也断言（无键匹配的规则同样红），
两个方向都不留「多写一条没人管」的洞。

### 5.3 形态：Rust 测试，不是 Python 脚本

`scripts/ci/` 下的既有门禁都是 Python/Bash。这一条**不能**照做：判据是**字形宽度**，
Python 侧要么自己解析 TTF 的 `hmtx`/`cmap`（那就是在真相源之外造一份度量副本——
本仓库反复付过代价的那个形状），要么装第三方字体库（许可证门禁 + 无引擎纪律）。
用 `ll_text::TextMeasurer` 的 Rust 集成测试**走的是渲染器自己那条 `layout_text`**，
零副本。落点 `crates/ll-ui/tests/i18n_text_width.rs`，由 `scripts/ci/run_tests.sh`
（`run_all.sh` 已含）在 **windows 与 ubuntu 两个 target 上各跑一次**——比只在
ubuntu 跑一个脚本 job 的覆盖面更大。

---

## 六、不做什么

- **L0（提交前取整）/ L1 / L2 / L3 / L4 / L5**：全是 P1/P2，不在本批。
- **W6（状态栏拆成 6 个标签）/ W7（光标前缀等宽）**：P2。W1 落地后 O-1 不再溢出
  （断行宽从 400 变成 608，446.5 一行放得下），W6 的紧迫性随之消失。
- **W4（`MAX_SAVE_NAME_CHARS` 按宽度判）**：P1，本批只把 O-8 从「画出面板」变成
  「在面板内换行」，字符上限不动。
- **F2/F3/F4/F6/F7**：P1/P2。
- **不新增 example target**（ADR 0030）。
- **不 push、不合并 main。**

---

## 七、ADR 0018 反例验证计划

每条新断言都要用**故意改坏实现**的方式验证真的会红。**特别提防「用空串测所以永远
绿」**——上一批在「语言」行上抓到过一条这种假绿。本批的宽度断言全部用
**`assets/locales` 里的真实文案**，不用占位符、不用空串，并在反例验证里额外确认
「把被测文案换成空串，断言仍然能红」是不成立的（即断言真的依赖文案内容）。

| 断言 | 反例 |
|---|---|
| `RowCursor` 按渲染行数推进 | 把 `line_count` 改回恒 1 |
| `Label.max_width` 恒等于面板内容宽 | 把某一处改回写死 `400.0` |
| 模态屏面板按内容伸缩 | 把 `max(SCREEN_WIDTH, …)` 改回恒 `SCREEN_WIDTH` |
| 每个键的行数预算 | 把 `screen-worldsetup-hint` 的 en 文案加三个词（规格 W5 指定的反例） |
| F1 两条反馈 | 把 `door_close_blocker` 改成恒 `None` |
| F1 不消耗回合 | 把拒绝改成照常 `Submit(Intent::CloseDoor)` |

## 八、提交划分

1. `feat(ll-text)`：`MeasureText`/`TextMeasurer`/`TextMetrics`。
2. `fix(ll-ui)`：W1 + W2（`Label.max_width`、`RowCursor` 按渲染行数、面板宽度成为
   唯一真相源、删掉 `400.0`、模态屏伸缩）+ `ll-game` 调用点接线。
3. `feat(ll-sim, ll-game)`：F1（`door_close_blocker` + 两条反馈 + i18n）。
4. `test(ll-ui)`：溢出门禁。
5. `docs`：规格与交接文档的更正回写（纪律第 9 条）。
