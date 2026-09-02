# 批次 33：UI 规格收尾——W7 + F7、W6、N11

**分支**：`wt-uifinal`（基线 `6e44591`，`origin/main`）
**规格**：`knowledge/design/ui-and-navigation.md` §8.5 W6 / W7、§9.3 F7、§7.5 N11
**前三批**：`2026-08-31-batch19-ui-text.md`（P0）、`2026-08-31-batch23-ui-p1.md`（P1
九条）、`2026-09-01-batch30-ui-p2.md`（P2 的 L0–L5 与 N12/N13）。三份最后一节的
临时约定在本批继续沿用，本文第七节接着往下记。

**这一批做完，规格十节的优先级表应当一条不剩。** 收工前逐条核对，写进第九节。

---

## 〇、改前基线（本工作树实跑）

`CARGO_BUILD_JOBS=4 bash scripts/ci/run_tests.sh` **第一次 `EXIT=101`、
一条测试都没跑到**——`rustc` 编 `naga` 时 `STATUS_ACCESS_VIOLATION`
（`exit code: 0xc0000005`）。这是交接文档纪律第 8 条那四种假失败之外的
**第五种症状**，判据完全一致（`EXIT=101` + `test result` 行为零 + 崩在
第三方 crate 上）。按纪律降并行重跑（`CARGO_BUILD_JOBS=2`），不 debug 代码。
**收工时把这条症状回写进交接文档第 8 条。**

改前基线数：见第九节（实跑后填）。

---

## 一、A：W7 + F7——光标记号从文本内容里拿出来（本批主体）

### 1.1 先把「几十条」核实成真实数字

批次 30 的记录写的是「全仓库几十条 `starts_with(CURSOR_PREFIX)`」。**实测不是
几十条**：全仓库共 **22 处，全部落在两个文件里**
（`ll-ui/src/hud/action_menu.rs`、`ll-ui/src/screen/mod.rs`），
`ll-game` 与集成测试**一处都没有**。批次 30 的那句估计偏大，本文原地更正
（纪律第 9 条：更正写回被更正方——同时改那两处标记）。

分类与处置：

| 类 | 处数 | 干什么 | 怎么处置 |
|---|---|---|---|
| 常量声明 + 文档 | 4 | `CURSOR_PREFIX` / `IDLE_PREFIX` 各两份 | **删掉**；文档改写成「光标是高亮矩形」 |
| 生产排版 | 4 | `write_action_menu_lines` 与 `screen_text_lines` 各拼一次前缀（含空列表占位行） | 去掉前缀，行文本 = 原文 |
| 测试断言「哪一行被选中」 | 6 | `lines[2] == "> 乙"`、`starts_with(CURSOR_PREFIX)` 等 | **换成断言高亮矩形**——这一类正是本批要拔掉的依赖 |
| 交叉引用文档 / 批次 30 的标记 | 6 | 模块文档、`IDLE_PREFIX` 那段「等宽是假的」 | 改写成落地后的真相，指回本文 |
| 反面教材断言 | 1 | `两种前缀等宽以免整列文字随光标抖动`（比字符数） | **删掉**——规格 §12 点名「落地 W7 时要把它换掉而不是留着」 |
| 长行夹具 | 1 | `screen/mod.rs` 用 `format!("{CURSOR_PREFIX}{长行}")` 造超长行 | 改成不带前缀的长行 |

### 1.2 高亮矩形落在哪

**模态屏（`screen/`）今天已经有高亮矩形**——`push_row_highlights` 给
`data.cursor` 画 `FOCUS_HIGHLIGHT_COLOR`、给 `data.hovered` 画
`HOVER_HIGHLIGHT_COLOR`（批次 31 的鼠标批落的）。也就是说模态屏这一侧
**F7 只需要把文本前缀拔掉**，视觉标记本来就在。

**HUD 动作菜单（`hud/action_menu.rs`）没有**——它今天唯一的选中态就是那个
文本前缀。这一侧要新增：

1. `action_menu.rs` 产出 `ActionMenuContent { panel: PanelContent, row_rects: Vec<Rect> }`
   ——行矩形与行文字**出自同一次游标推进**，与 `screen::layout_screen`
   逐字同一条纪律（那里的文档已经写死了理由：按公式反算会在长行换行后
   静悄悄错位）。
2. `placement::placed_action_menu` 的整体平移**连行矩形一起平移**。
3. `hud/render.rs` 在推面板背景之后、推标签之前，把 `row_rects[cursor]`
   推成一块高亮。

**不新造第五份布局实现**：高亮矩形的**颜色与皮肤分支**今天只有一份，住在
`screen/`。本批把它搬进 `widget/highlight.rs`，`screen::FOCUS_HIGHLIGHT_COLOR`
等一律改成 `pub use` 重导出（公开路径一个都不断），两边调同一个
`push_row_highlight`。**这是「不要第二份」，不是新增一份**——搬家前 1 份，
搬家后仍是 1 份，只是住在两边都看得见的地方（与批次 30 把间距常量搬进
`widget/metrics.rs` 是同一条理由）。

行矩形本身走的是 `RowCursor` 的前后差，不碰 `Rect::anchored` /
`ScreenZone` / `snap_to_pixels` 那一套的任何一条算术，
`check_single_anchor_impl.sh` 的四种形状一条都不命中。

### 1.3 「拔掉文本前缀后仍验得出选中行」的证据

这是本批**点名要的第 2 条反例**。三条断言，两条走生产渲染路径：

1. `hud/render.rs`（挂在 `render_layout_tests.rs`）：`build_hud_frame` 带一块
   `cursor: 1` 的动作菜单，在 `UiLayer::Popup` 的 `quads` 里找颜色等于
   `FOCUS_HIGHLIGHT_COLOR` 的那**一块**（先断言恰好一块，否则「找不到」会
   退化成恒绿），断言它的 `position[1]` 等于 `action_menu_content` 给出的
   `row_rects[1]` 取整后的 `y`；再把 `cursor` 换成 2，断言高亮**跟着动了
   一整行高**。
2. `screen/render.rs`：同形，走 `build_screen_frame`。
3. 反例（点名）：把高亮的取行下标写死成 `0`。上面两条必须**都红**，
   且红在「cursor 不为 0 时高亮仍在第 0 行」这条消息上。

### 1.4 视觉基准

批次 30 已 grep 确认 `crates/ll-game/tests/visual_baselines.rs` **不引用
`ll_ui` 的任何符号**，三张图走 `ll_render` 精灵路径。本批预期三张图逐字节
不动。收工实跑确认；**变了就逐张说明每处差异对应哪条规则再决定，不无说明
地 bless。**

---

## 二、B：W6——状态栏拆成字段

### 2.1 拆成哪些字段

规格原文：「`status_bar.rs` 把 6 段翻译拼成**一个** `Label`……拆成 6 个独立
标签横向排列」。今天那 6 段翻译是 `time` / `health` / `mana` / `fps` 四个
标签键 + `season` + `weather`。

落地取 **6 个字段**：

| # | 字段 | 内容 | 备注 |
|---|---|---|---|
| 1 | 时间 | `时间 3 07:20` | 标签 + 值同一格（值没有独立的翻译键） |
| 2 | 季节 | `春` | |
| 3 | 天气 | `雨` | **没有天气时这一格整个不出现**（今天的 `Option` 语义原样保留） |
| 4 | 生命 | `生命 30` | |
| 5 | 法力 | `法力 12` | |
| 6 | 帧率 | `帧率 60` | |

**括号没有了**：今天季节与天气共用一对括号，理由原文是「分开成两组括号
只会让这行更拥挤」——那条理由的前提是「这些段挤在同一条连续文本里」。
拆成各自定位的字段之后，格与格之间靠间隔区分，括号不再承担分隔职责。
这一条记进第七节。

**判据**：`status_bar_panel` 产出的 `Label` 数——有天气时 **== 6**（规格
要的 ≥6），无天气时 == 5，且**逐字段**断言第 i 个 Label 的文本就是第 i 格
的内容。再加一条结构断言：全部字段的 `y` 相同（真的是横排，不是又堆成了
六行）。

### 2.2 横排算法住哪

`widget/list.rs` 的 `RowCursor` 新增一个方法：

```rust
pub fn push_fields(&mut self, labels: &mut Vec<Label>, fields: &[String], gap: f32)
```

一行里把若干格从左往右摆开，游标**只推进一行**。放在 `RowCursor` 而不是
`status_bar.rs` 里，是因为它要的正是 `RowCursor` 已经持有的那四样
（`x` / `cursor_y` / `font_size` / `measure`），写在外面就要把测量器再传一遍
——而 `push` 与 `push_fields` 对「这一行推进多少」必须给出同一个答案，
两处实现迟早分叉。

`gap` 取 `widget::metrics::PANEL_GAP`（批次 30 落的间距刻度），不新造常量。

**每一格的 `max_width`**：一格不换行（它们是短标签），`max_width` 取
「这一格从它的 x 到面板内容区右边界」剩下的宽——这样最后一格若真的被
挤到边上，`cosmic-text` 会在这一格自己身上换行，而不是画到面板外面去。

### 2.3 溢出门禁那条规则怎么改

`crates/ll-ui/tests/i18n_text_width.rs` 今天给 `hud-status-` / `season-` /
`weather-` 三条前缀的判据是 `一行(content_width(STATUS_WIDTH))`——把
**一格**拿去和**整条状态栏的内容宽**比。拆成字段之后这条判据更没有牙了
（`FPS` 在 608px 里当然只占一行，永远绿）。

改法：这三条规则改成新的 `宽度判据::横排一格(&'static str)`（理由字符串
空着就红，与既有的 `参数化` 同一形状），**真正的牙挪进同一文件的一条新
测试**：

> `状态栏整行在两种语言下都排得下`：用**真实 `assets/locales`**，遍历
> **四个季节 × 六种天气 + 无天气**、两种语言，各调一次生产代码
> `status_bar_panel`，断言最后一格的右边界 ≤ 面板内容区右边界。

这比旧规则严格得多（旧的量单格，新的量最坏情况下的整行之和），且
「两个方向都会红」两半不受影响：三条规则仍然各自匹配到键（不是死规则），
每个键仍然命中恰好一条规则（不缺）。

**注意 `英文确实是更宽的那一侧` 那条测试**按 `matches!(判据, 行数上限)`
过滤，新变体会让这三类键退出那条求和——那条断言的结论（en 总宽 > zh ×1.2）
由两百多条键支撑，退掉十几条短标签不影响，但**收工前实跑确认它仍绿且
仍然真的量到了东西**。

---

## 三、C：N11——上下键一律循环，长按一律连发

### 3.1 今天的九块屏实况（已逐个 grep）

| 落点 | 循环 | 连发 |
|---|---|---|
| `ll_ui::widget::focus::move_focus` / `navigate_focus`（首页、游戏内菜单） | 是 | 是（`was_activated`） |
| `menu_screen::moved_cursor`（设置屏） | 是 | 是 |
| `player_action::moved_cursor`（HUD 三块动作菜单） | 是 | 是 |
| `save_list::update_save_list`（存档列表） | 是 | **否**（`was_just_pressed`） |
| `dialogue_screen`（会话屏） | 是 | **否** |
| `trade_screen`（交易屏） | 是 | **否** |
| `chargen::move_cursor`（**角色创建 + 世界配置共用**） | **否**（到边即停） | **否** |
| 选点屏地图光标（`app.rs` 的 `dx/dy`） | 否（到边即停） | 是 |

**规格点名的例外**是最后一行（空间坐标不是列表）——它今天就是「到边即停 +
连发」，本批**一行不动**。

规格原文只点了「角色创建/世界配置/选点屏」，但 N11 的标题是「**一律**」，
而存档列表 / 会话屏 / 交易屏三块的连发是缺的（这三块是批次 27/31 之后
才有的，写规格时还不存在）。本批一并改，理由记进第七节。

### 3.2 收敛成一份

`ll-game` 侧今天有**五份**「这一帧光标移到第几行」的算术。本批把它们收进
`nav_row.rs` 已有的那一份：

- `stepped_cursor(cursor, forward, len)` **改成循环**（这就是批次 30 那条
  裁定的回写，见 3.3）。
- 新增 `moved_cursor(input, cursor, len) -> Option<usize>`：`was_activated`
  的 Up/Down，同时按住视为无输入（沿用 `player_action::moved_cursor` 那条
  已有理由），方向确定后**调用 `stepped_cursor`**——循环算法只有一份。
- `menu_screen::moved_cursor` / `player_action::moved_cursor` 直接换掉；
  `save_list` / `dialogue_screen` / `trade_screen` / `chargen`
  的内联算术全部换成调用它。

`ll_ui::widget::focus` **不动**：它已经循环、已经连发，且它操作的是
`WidgetStateTable` 而不是一个 `usize` 光标，塞进同一个函数等于给 `usize`
和状态表做一个带分支的四不像（ADR 0021）。这一条写在 `moved_cursor` 文档里。

### 3.3 批次 30 那条 `stepped_cursor` 不循环的裁定，回去处理

批次 30 的 `stepped_cursor` 文档原文：

> 不循环是**保守取舍**：规格 N11（上下键一律循环）是 P1、还没落地，今天
> 上下键到边即停。左右键此刻循环就会比上下键更「新」……**N11 落地时这一个
> 函数跟着改，三块屏不用动。**

本批就是它说的那一天。改成循环，并且：

- `nav_row.rs` 原地把那段理由改写成「N11 已落地（批次 33），上下与左右
  在这三块屏上走同一个函数、同一套循环语义」，**指向本文**；
- `docs/superpowers/plans/2026-09-01-batch30-ui-p2.md` 第八节那一条与规格
  §7.6 的批次 30 回写段落各加一行**指回本批**。

纪律第 9 条要的「两边互相指向」就是这两处。

### 3.4 「连发不是合成按键」的证据（点名的第 3 条）

ADR 0025 禁止合成按键。本批的连发**不是**「假装按了很多次」——它走的是
`InputState::begin_frame(now, RepeatConfig)` 的**时间**：`press` 一次之后，
每一帧用一个前进的 `Instant` 调 `begin_frame`，超过 `initial_delay` 才第一次
`repeated`，之后每 `interval` 一次。

判据（新测试，落在 `ll-game` 侧）：

- **只调一次 `press(GameKey::Down)`**，此后一次 `press` 都不再调；
- 逐帧 `begin_frame(t0 + k×16ms, RepeatConfig::default())`；
- 断言光标在 `initial_delay` 之前**只动了那一格**（`just_pressed` 的那一次），
  之后按 `interval` 继续动；
- **反例**：把 `RepeatConfig.interval` 调成 10 秒（不碰任何 `press` 调用），
  这条必须红——红在「时间到了却没动」，而不是「按键次数不够」。这就是
  「连发由计时驱动」的证据：改的是时钟，红的是断言。

---

## 四、硬约束核对

不动世界状态、内容表、存档主体形状。三条黄金基准 /
`CONTENT_HASH_ALGORITHM_VERSION` / `CURRENT_SCHEMA_VERSION` 收工前 grep 比对
（**不在本文写数值**，理由见交接文档第〇节）。**本批预期不新增任何 `.ftl`
条目**（W7/F7 是把文字拿掉、W6 是把同一批既有键分开摆、N11 是输入语义）；
真要加一律加在两份 `.ftl` **末尾**。不新增 example target。
**不碰任何 `mods/` 下的 JSON5 与内容侧代码**（另一个会话正在跑内容 id 撞车审计）。

## 五、行数棘轮

`hud/render.rs` 快照 1118 行，本批要往里加高亮那几行与新断言。**先拆再
bless**：新断言用 `#[path]` 挂进已有的 `render_layout_tests.rs`（批次 30
的先例）。`screen/mod.rs` 不在快照里但代码行必须留在 800 以下，加断言前
先看余量；超了就把 `mod tests` 整块搬成 `screen/mod_tests.rs`。
**`--bless` 的理由一律写满，空着即红。**

## 六、反例验证计划（ADR 0022）

**写断言之前先跑基线**，每条都确认红的**确实是想验的那一条**（逐个二进制
单独跑——批次 31 实测「一次 `cargo test` 跑多个二进制时第一个失败会盖住
后面的结果」）。

| 断言 | 反例 | 预期红在 |
|---|---|---|
| **① 高亮跟着选中行走** | 把高亮取行下标写死成 `0` | HUD 与模态屏两条各自红在「cursor 不为 0 时高亮仍在第 0 行」 |
| **② 拔掉前缀后仍验得出选中行** | 同上；外加把 `row_rects` 改成恒空 | 「恰好一块高亮」那条前置断言先红（**证明后面不是空集恒绿**） |
| **③ 连发不是合成按键** | `RepeatConfig.interval` 改成 10 秒 | 「时间到了就该再动一格」红；**不碰任何 `press` 调用** |
| N11 循环 | `stepped_cursor` 改回 `.min(len-1)` | 「末行按下移动到首行」红（角色创建 / 世界配置各一条） |
| W6 拆成字段 | `push_fields` 改成逐格换行 | 「六格 y 相同」红 |
| W6 整行不溢出 | `STATUS_WIDTH` 调窄 | 新的整行断言红，且报出是哪一门语言哪一格越界 |
| 溢出门禁不留死规则 | 删掉 `weather-` 那条规则 | `每一个键都声明了自己画在哪块面板里` 红 |

**六个假绿形状主动防**：

1. 不用空 `Catalog`——W6 的整行断言用真实 `assets/locales`，且断言
   `en 文案 != zh 文案`。
2. 先断言对象存在——「恰好一块高亮 quad」在前，「它在第几行」在后。
3. 判据适用面不被新代码绕过——高亮断言走 `build_hud_frame` /
   `build_screen_frame` **整帧**，不走单个函数。
4. 前置断言不挡后置——逐条确认红的是表里写的那一条。
5. 生产数据别让判据退化成恒真——W6 的整行断言遍历**全部**季节 × 天气，
   不只测一个恰好很短的组合。
6. 逐个二进制单独跑反例。

**若某条反例改坏了它不红**：不粉饰、不改宽断言——查清原因、如实登记、
补一条真咬得住的。

## 七、规格没裁定、本批临时选的做法（滚动记录，收工搬进最终报告）

1. **模态屏与动作菜单一并拔掉 `IDLE_PREFIX`**（不只是 `CURSOR_PREFIX`）。
   规格只说「光标从文字前缀改成高亮矩形」，没说另一个前缀怎么办。留着
   `IDLE_PREFIX` 等于让每一行都缩进两格却没有任何一行不缩进——那个缩进
   当初存在的唯一理由就是与光标前缀对齐。
2. **高亮的颜色与皮肤分支搬进 `widget/highlight.rs`**，`screen::` 的两个
   常量改 `pub use` 重导出。规格没说 HUD 的高亮该长什么样；取「与模态屏
   同一份色」是因为两处对玩家是同一个意思（「这一行现在会响应确认键」）。
3. **状态栏的括号取消**（季节/天气不再共用一对括号），理由见 2.1。
4. **状态栏无天气时是 5 格不是 6 格**。规格判据写「≥6」，但天气本来就是
   `Option`；编一个空格子出来只会让那一格的间隔无缘无故存在。
5. **溢出门禁的 `hud-status-`/`season-`/`weather-` 改成 `横排一格`，牙挪进
   整行断言**，见 2.3。规格 W6 的判据原文是「en 下整行宽 ≤ 内容宽」，
   那正是整行断言在做的事。
6. **N11 一并改存档列表 / 会话屏 / 交易屏的连发**，规格只点了三块屏，
   理由见 3.1。
7. **`ll_ui::widget::focus` 不并进 `nav_row::moved_cursor`**，理由见 3.2。
8. **选点屏地图光标一行不动**（规格明写的例外）。

## 八、提交划分

1. `refactor(ll-ui)`：把高亮矩形的颜色与皮肤分支搬进 `widget/highlight.rs`，
   `screen` 改重导出。**逐像素零变更**，自身应全绿。
2. `feat(ll-ui)`：F7 上半——动作菜单产出行矩形 + HUD 画高亮。
3. `feat(ll-ui)`：W7 + F7 下半——两处拔掉文本前缀、删掉那条比字符数的
   反面教材断言、六条测试换成断言高亮矩形。
4. `feat(ll-ui)`：W6——`RowCursor::push_fields` + 状态栏拆六格 + 断言。
5. `test(ll-ui)`：溢出门禁那条规则改写 + 整行断言。
6. `feat(ll-game)`：N11——`nav_row::moved_cursor` + `stepped_cursor` 改循环
   + 五处调用点收敛 + 连发的计时证据。
7. `docs`：规格十节回写、批次 30 计划的互相指向、交接文档第 8 条补第五种
   假失败症状。

尽量让每个提交自身是绿的——**特别盯
`cargo doc -D rustdoc::broken-intra-doc-links`**：本批要删两对公开常量并搬走
两个公开常量，断链风险高，每个提交单独跑一次 `bash scripts/ci/check_doc_links.sh`。
做不到就在提交信息里写明。**不 push、不合并 main。**

## 九、落地后的偏离与实测

（收工回填：门禁 exit code、改前/改后测试数、视觉基准动没动、三条黄金
基准核对、UI 规格还剩什么。）
