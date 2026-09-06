# 批次 35：UI 规格 P1 最后三条（N9 / F3 / D6）

做完这三条，`knowledge/design/ui-and-navigation.md` 的 P0 / P1 / P2 三张
清单**全部清空**。

- P0 十二条：批次 15 / 19。
- P1 十六条：批次 19 / 23（九条）→ 批次 30（L0/L1/L2）→ 批次 33（N11）
  → 本批（N9 / F3 / D6）。
- P2 八条：批次 30（L3/L4/L5/N12/N13）→ 批次 33（W6/W7/F7）。

上一批逐行扫规格勾出「还剩这三条」的记录在
`docs/superpowers/plans/2026-09-01-batch33-ui-final.md` 第十节。

---

## 〇、改前基线（本工作树实跑）

见第八节「落地后的偏离与实测」，收工时回填。

---

## 一、N9：模态屏进 `UiLayer`

### 1.1 规格原文（§7.5）与判据

> 新增 `UiLayer::Modal` 排在 `Notice` 之后，让 `draw_batches` 重新成为
> 遮挡关系的唯一真相源。
>
> **判据**：`draw_batches` 的输出里 `Modal` 批次永远排在 `Overlay` 之后；
> 删掉 `app.rs` 里「先 draw_hud 再 draw_screen」这条隐式依赖的注释性保证。

判据的后半句是本条真正的分量所在：**光加一个枚举变体不够**。今天
`ScreenFrame` 是一条与 `LayeredFrame` 平行的独立通道，两者的先后**只由
`app.rs` 里两次调用的书写顺序决定**——那正是 `widget/layer.rs` 模块文档
开头那条实机缺陷的形状（「跨批次的先后不由推入顺序决定」）。要让
`draw_batches` 成为唯一真相源，模态屏必须**进同一个 `LayeredFrame`**。

### 1.2 落点

| 文件 | 改什么 |
|---|---|
| `crates/ll-ui/src/widget/layer.rs` | `UiLayer` 加第五个变体 `Modal`；`ALL` 变成 5 元数组；`index()` 加一支 |
| `crates/ll-ui/src/widget/zone.rs` | `ScreenZone::of` 加 `UiLayer::Modal => ScreenZone::Modal`（**不加 `_` 兜底**）；模块文档「模态区今天没有成员」整节改写；两条测试跟着改 |
| `crates/ll-ui/src/widget/submit.rs`（新） | 把 `render_hud` 里那段 `for batch in frame.draw_batches()` 提交循环搬出来，成为**全 crate 唯一**的提交出口 |
| `crates/ll-ui/src/hud/render.rs` | `render_hud` 删掉（它做的两件事已经拆成 `build_hud_frame` + `submit_frame`）；文件因此**变短**（它在行数棘轮快照里，只许短不许长） |
| `crates/ll-ui/src/screen/render.rs` | `ScreenFrame` 类型删掉，`build_screen_frame` 改成 `push_screen_layer(&mut LayeredFrame, …)`，内容推进 `UiLayer::Modal`；`render_screen` 删掉 |
| `crates/ll-game/src/app/hud_draw.rs` | `draw_hud` 改成只建不提交，产出一个 `LayeredFrame` |
| `crates/ll-game/src/app/screen_flow.rs` | `draw_screen` 改成往那个 `LayeredFrame` 里推，不提交 |
| `crates/ll-game/src/app.rs` | 渲染段改成「建一帧 → 两处往里推 → 提交一次」；删掉「先 draw_hud 再 draw_screen」那条注释性保证 |

### 1.3 `ScreenZone` 那一处编译期红

批次 30 落地 `ScreenZone::of` 时**刻意**没有写 `_ =>` 兜底，并在模块文档
里写明「规格 N9 才会把它收进一个新的 `UiLayer::Modal`。那一天这里加一条
分支」。加第五个变体的那一刻它当场编译不过——**这是设计好的**。

本批**补那一支**，不加兜底。同时 `zone.rs` 的测试
`每一层都能说出自己属于哪个区` 今天断言的是「没有任何层落在模态区」，
那条断言的前提被本批推翻，改成：模态区**恰好**有一个成员，且它就是
`UiLayer::Modal`；其余四层一个都不许落进去。改写之后它仍然是「新加一层
必须回答自己属于哪个区」那条保护的运行期陪衬（真正的保护是那个没有兜底
的 `match`）。

### 1.4 字号/行高/文字色三者必须相同，否则合不进一个帧

合帧之后文本批次是按**层**切开的，而提交循环拿不到「这一批来自哪一层」。
因此合帧的前置条件是 HUD 与模态屏的文本参数一致。实测：

- `hud::DEFAULT_FONT_SIZE` = 14.0 ＝ `screen::SCREEN_FONT_SIZE` = 14.0
- `hud::DEFAULT_LINE_HEIGHT` = 18.0 ＝ `screen::SCREEN_LINE_HEIGHT` = 18.0
- 两处 `TEXT_COLOR` 都是 `Color::rgba(235, 235, 235, 255)`

三者全同，合帧不改任何一个像素。**这一条写进新模块的文档**，将来谁想把
模态屏字号调大，编译不会红，但会撞上那段文档与守着它的那条断言。

### 1.5 取整（规格 L0）在哪一步发生

今天 `build_hud_frame` 结尾调 `LayeredFrame::snap_to_pixels`，
`build_screen_frame` 结尾调 `ScreenFrame::snap_to_pixels`（同一份三个
`snap_*` 助手）。合帧之后**只在提交前取一次**：提交函数收
`&mut LayeredFrame`，进循环之前调一次 `snap_to_pixels`。
取整幂等，因此这不改任何坐标；但它把「提交那一刻取整」这句话从两处变成
一处，正是 L0 那条判据本来的形状。

### 1.6 视觉基准会不会变

`crates/ll-game/tests/visual_baselines.rs` 三张图是**纯 CPU 的世界层
预览**（地表内容 / 据点建筑地形 / NPC 点名册），一行 HUD、一块模态屏都
不画。本条与它零交集。**预期不动，动了就是出事了。**

### 1.7 反例（点名的第 ① 条）

`Modal` 真的排在 `Notice` 之后：把 `UiLayer` 声明里 `Modal` 与 `Notice`
两行**对调**，断言必须红。要红在「次序」上，不是红在编译错误上——因此
断言走 `draw_batches()` 的产出序列，不比枚举本身。

---

## 二、F3：自动存档要有一个不打扰人的痕迹

### 2.1 规格原文（§9.2）与判据

> `Notice` 层加一个**两秒后自渐隐**的小字「已自动保存」。
> `widget/anim.rs` 的 `DEFAULT_ANIM_DURATION_FRAMES` 与 `render.rs` 的
> `AFTERGLOW_DURATION_FRAMES` 是现成的渐隐机制，复用它。
>
> **判据**：`maybe_autosave` 真的存了之后，notice 非空；
> `AFTERGLOW_DURATION_FRAMES` 帧之后归空。

**裁定句与判据句自相矛盾**：`AFTERGLOW_DURATION_FRAMES`
= `DEFAULT_ANIM_DURATION_FRAMES * 3` = 60 帧 = 一秒，不是两秒。本批取
**判据**那一句（它是可执行的那一句），常量写成同一条派生式而不是抄一个
60，理由记在常量的文档里。这一条进最终报告第九节。

### 2.2 落点

| 文件 | 改什么 |
|---|---|
| `crates/ll-game/src/autosave_notice.rs`（新） | 生命周期本体：一个「第几帧存的」+ 一个纯函数「这一帧还该不该显示」。**唯一产出点** |
| `crates/ll-game/src/app/save_flow.rs` | `maybe_autosave` 收一个帧号，成功那一支打上痕迹 |
| `crates/ll-game/src/app.rs` | `Demo` 加一个字段；`advance` 把帧号传下去；渲染段把「这一帧该不该显示」算出来传给 HUD |
| `crates/ll-ui/src/hud/bottom_rows.rs` | 加第三行 `push_autosave_row`，走**同一个** `bottom_row_panel` |
| `crates/ll-ui/src/hud/render.rs` | `build_hud_frame` 多收一个 `autosave: Option<&str>`，推进 `UiLayer::Notice` |
| `assets/locales/{en,zh-CN}/*.ftl` | 新增 `hud-autosave-saved`，**加在文件末尾**（避开并行批次 wt-npcnames） |
| `crates/ll-ui/tests/i18n_text_width.rs` | 分类表加 `hud-autosave-` 一条（该门禁两个方向都会红） |

### 2.3 「由时钟驱动」的形状

照批次 33 的 N11（`nav_row::长按方向键连发是由时钟驱动的不是由按键次数
驱动的`）。这里的时钟是 `ll_platform::input::FrameId`——与
`widget::anim` 同一个时钟源（那个模块的文档「时钟源：帧计数，不是墙钟
时间」一节写明帧计数属 ADR 0020 甲区，不污染世界状态）。

判定抽成**纯函数**：给「自动存档成功那一帧的帧号」与「现在第几帧」，
回答这一帧还该不该显示。这样反例只要**改时钟**（把时长常量改掉、把判据
改成恒真）就能红，全程一次按键都不合成（ADR 0025）。

**注意不要与 `maybe_autosave` 自己那条「世界时钟」混淆**：*要不要存*
由世界时钟决定（约束 C4，见 `maybe_autosave` 文档，本批一个字不动）；
*存过的痕迹还要显示多久*是纯表现层，由帧计数决定。两条时钟各管各的，
这一句写进新模块的文档。

### 2.4 「不打扰人」体现在哪（四条，逐条可查）

1. **不抢焦点**：不碰 `modal`、不碰 `screen_focus`、不改 `InputContext`。
   打痕迹的那一句只写 `Demo` 上一个新字段。
2. **不消耗回合**：`maybe_autosave` 排在 `run_turn` 之后、与世界推进无关，
   本批不动它的位置；新字段不进 `GameWorld`，不进存档主体。
3. **不挡住玩家正在看的东西**：落在屏幕最下沿那条**底栏**里，叠在反馈行
   上面一格，走既有的 `bottom_row_panel`（规格 L2 的 `Rect::anchored`，
   不新造第五份落位算术）。中段（玩家看世界的地方）一个像素都不占。
4. **有模态屏盖着时看不见**：它在 `UiLayer::Notice`，而 N9 之后模态屏在
   `UiLayer::Modal`——压暗背板与面板都排在它之后提交。这是本批两条一起
   做的好处，不需要再写一句 `if`。

### 2.5 反例（点名的第 ② 条）

- 把时长常量改成 `u32::MAX`（**不碰任何按键、不碰 `maybe_autosave`**）
  →「时长走完之后归空」那条必须红。
- 把判定纯函数里那句比较改成恒 `true` → 同上。
- 端到端那条：`maybe_autosave` 真的存了之后痕迹非空 —— 把成功分支里那
  一句打痕迹删掉必须红。

三条分别咬「时钟」「判据」「生产路径上真的被调用」。

---

## 三、D6：角色创建退出不清草稿

### 3.1 规格原文（§2.2 D6）

> `chargen.rs`（Esc）与「返回」行都直接去 `Title`，**不动
> `new_game_draft`**。对比 `back_to_title` 是显式清的。后果两条：
> 玩家选好的种族/性别/职业在下次按「开始游戏」时被无条件覆盖；转生场景
> 下那份草稿**持有一整个 `GameWorld`**，回到首页之后再没有任何路径能回到
> 它，只能从磁盘重新读档。

§10 只把 D6 列进 P1 清单，**没有给裁定**（对比 F2/F3/F4 都有「裁定」
一句）。因此修法由本批选定，记在最终报告第九节。

### 3.2 两条后果指向同一个修法：**让留着的那份草稿真的能回去**

「清掉」只治得了第二条（世界不再悬着），却把第一条治反了——玩家选好的
三项照样没了。而「留着 + 回得去」两条一起治：

- 种族/性别/职业留着 ⇒ 按 Esc 出去再进来，选择还在；
- 转生那份世界留着**且有路回去** ⇒ 不必从磁盘重读。

所以本批不去 `chargen.rs` 加清理，而是把**真正错的那一处**改掉：
`start_new_game` 里那句**无条件覆盖**。

### 3.3 落点

| 文件 | 改什么 |
|---|---|
| `crates/ll-game/src/app/screen_flow.rs` | `start_new_game`：已经有草稿就**沿用**，没有才新建 |
| `crates/ll-game/src/app/save_flow.rs` | `enter_world_in_slot`：世界已经开局，**清掉**草稿 |

第二处是本条**必须一起做**的配套：草稿一旦变得「回得去」，
「读档进世界之后首页那份旧草稿仍然回得去」就成了新的数据丢失路径
（玩家死亡 → 留下一份转生草稿 → 回首页 → 读档玩了很久 → 再回首页 →
「开始游戏」竟然回到死亡那一刻的世界，并且写回同一个槽位）。
不变式因此写成一句话：**草稿与 `session` 不共存**。今天
`finish_entering_world` 用 `take()` 已经守住了新游戏那条路，
`enter_world_in_slot`（读档那条路）是唯一的漏口。

### 3.4 那道「写错路径不编译」的防线：不许削弱

`crates/ll-game/src/draft_world.rs` 的 `FreshWorld`/`RebornWorld` 私有字段
是修 D1（真实数据丢失）时立的防线，它有一条 `compile_fail` 文档测试盯着
（「把一个新生成的世界塞进转生草稿」）。

**本条一行都不碰 `draft_world.rs`**：改的是 `Option<NewGameDraft>` 这个
字段的**生命周期**，不是草稿内部的形状。因此不存在冲突。
反例第 ③ 条要同时验两件事：草稿真的留着（退出再进必须还在），
**以及**那条 `compile_fail` 仍然编译不过。

### 3.5 与三条黄金基准 / 存档 schema 的关系

D6 只动 `Demo` 上一个 `Option` 字段的生命周期，**不进 `GameWorld`、不进
存档主体、不改任何内容表**。预期 `CONTENT_HASH_ALGORITHM_VERSION`、
`CURRENT_SCHEMA_VERSION`、`EXPECTED_POPULATED_WORLD_DIGEST` 三者一动不动。
`scripts/ci/check_save_schema_version.py` 那道联动门禁会替我证。

---

## 四、硬约束核对

| 约束 | 本批怎么满足 |
|---|---|
| 不动内容表 / `CONTENT_HASH_ALGORITHM_VERSION` | 三条都不碰 `mods/`、不碰 `ll-mod` |
| 三条黄金基准 | 预期全不动（见 3.5） |
| 视觉基准 | 预期全不动（见 1.6） |
| 溢出门禁 | F3 的新键进分类表（见 2.2） |
| 防第五份布局实现 | F3 走既有 `bottom_row_panel` → `Rect::anchored` |
| 不硬编码用户可见字符串 | `hud-autosave-saved` 两份 `.ftl`，**加在文件末尾** |
| 不新增 example target | 不新增 |
| 文件行数棘轮 | 快照里被本批碰到的只有 `crates/ll-ui/src/hud/render.rs`（1042），N9 把提交循环搬走 + F3 只加几行 ⇒ **净减**。`app_tests.rs`（1058）本批不加测试进去 |
| ADR 0025 禁止合成按键 | F3 的三条反例全走时钟，一次 `press` 都没有 |
| 每个提交自身绿 | 含 `cargo doc -D rustdoc::broken-intra-doc-links`：删 `render_hud`/`ScreenFrame` 时要同批改掉全部文档引用 |

---

## 五、反例验证计划（ADR 0022）

**写断言之前先跑基线**；每条改坏之后**单独跑那一个二进制**（一次
`cargo test` 跑多个二进制时第一个失败会盖住后面的）。

| # | 改什么 | 该红的那一条 | 咬的是 |
|---|---|---|---|
| ① | `UiLayer` 声明里 `Modal` 与 `Notice` 对调 | 模态层的批次永远排在浮层之后 | N9 的次序 |
| ①b | `ScreenZone::of` 加 `_ => ScreenZone::Floating` 兜底并删掉 `Modal` 那一支 | 模态层落在模态区 | N9 的分区 |
| ①c | 模态屏那一侧改推 `UiLayer::Popup` | 同 ① | 模态屏真的进了 Modal 层 |
| ② | 痕迹时长常量改成 `u32::MAX`（不碰按键） | 痕迹在时长走完之后自己消失 | F3 由时钟驱动 |
| ②b | `maybe_autosave` 成功支里打痕迹那一句删掉 | 自动存档成功之后痕迹非空 | 注入点真的在生产路径上 |
| ③ | `start_new_game` 改回无条件覆盖 | 退出角色创建再进来草稿还在 | D6 |
| ③b | `draft_world.rs` 的 `compile_fail` 文档测试 | 它本来就该编译不过 | 防线仍在 |
| ③c | `enter_world_in_slot` 里清草稿那一句删掉 | 读档进世界之后草稿不该还在 | D6 的配套不变式 |

**七个恒绿形状主动防**：
- 用真实 `assets/locales`，断言 `en 文案 != zh 文案`（不是「文案 != 键名」）。
- 先断言被断言的对象存在（例如先断言底栏里确实有三行，再断言第三行的位置）。
- 判据跑在**生产产出**上（`build_hud_frame` / `draw_batches`），不自己拼数据。
- 每条断言前面不放更容易红的断言。
- 生产数据不让判据退化：行高全项目都是 18.0，因此「差一整行高」这类断言
  要配一条「行数真的多了一行」的存在性断言。
- 反例逐个二进制单独跑。
- 注入点确认在生产路径上（不是 `cfg(test)`、不是文档注释）。

---

## 六、提交划分

1. `feat: N9 模态屏进 UiLayer::Modal，提交次序收敛成一个 LayeredFrame`
2. `feat: F3 自动存档留下一条自渐隐的痕迹`
3. `fix: D6 角色创建退出保住草稿，读档进世界时清掉它`

三个提交各自要绿（含 `cargo doc`）。

---

## 七、规格没裁定、本批临时选的做法（滚动记录，收工搬进最终报告）

1. **F3 的时长取一秒不是两秒**：规格裁定句写「两秒」，判据句写
   「`AFTERGLOW_DURATION_FRAMES`（=60 帧=一秒）之后归空」，两句矛盾，
   取判据句。
2. **F3 的「渐隐」落成「到点消失」，没有 alpha 渐变**：`widget::label::Label`
   没有逐标签颜色/透明度字段，文字色是提交循环按批次统一给的常量；要做真
   alpha 渐变得改 `Label` 与三处提交路径。取最保守、最容易反转的做法。
3. **F3 的痕迹放在底栏第三行**（叠在反馈行上面一格），不是屏幕角落。
4. **D6 取「留着 + 回得去」而不是「清掉」**，见 3.2。
5. **D6 顺带补上 `enter_world_in_slot` 清草稿**，见 3.3。

---

## 八、落地后的偏离与实测

收工时回填。

---

## 九、过程坑：12 个文件名是中文散文片段的空文件被误提交

### 现象

`745e3b6`（N9）与 `34826ef`（F3）两个提交里各混进了几个**零字节**的文件，
落在仓库根目录，文件名是仓库 Markdown 文档里的散文片段——一共 9 个进了
历史，另有 3 个只留在工作区。全部 0 字节，全部在根目录，一个都不在
`crates/` / `assets/` / `scripts/` 之下。

### 成因（已核实到行）

本仓库的设计文档大量使用 Markdown **引用块**，行首是 `>`。这些行如果整行
落回 shell 的**命令解析**上下文（heredoc 里的引号提前闭合、命令行里嵌了
未加引号的文档正文，都会造成这个后果），`>` 就被当成**输出重定向**，
它后面那个词成了文件名，于是产出一个零字节的空文件。

逐条对得上：

| 空文件名 | 出处 |
|---|---|
| `让这款游戏更像回事。」` | `knowledge/design/ui-and-navigation.md:11`（行首是引用块标记） |
| `现状` | `knowledge/design/ui-and-navigation.md:107` |
| `玩家只能买不能卖，交易落地即残废。触及货币守恒，落地时要在` | `docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md:230` |
| `讲反例验证／「覆盖不全的守护等于没有守护」的是` | `knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md` 那一族 |

它们被 `git add -A` 一并扫进了提交。**没有任何一份真实文件被覆盖**——
重定向创建的是新文件，且全部落在根目录、全部 0 字节；`git diff` 显示的
真实改动一行都没受影响。

### 处置

不改写历史（`--amend`/`rebase`/`filter-branch` 在本环境被安全门拦，而且
为几个空文件冒险重写四个提交不划算）。改为**追加一个删除提交**，让分支的
最终树是干净的；那 9 个空文件因此只存在于 `745e3b6`..`443f29a` 这一段
历史里，合并后的树看不到它们。

### 教训（下一批照做）

1. 把文档正文送进 shell 之前**一律加引号**，尤其是含 `>` 「」 反引号 的中文正文。
2. 长文本一律走 `Write` 工具或 `python3 - <<'PY'` 这种**带引号的** heredoc，
   不要用 `cat > 文件 <<'EOF'` 塞含引用块的 Markdown。
3. 提交前先看一眼 `git status --short` 里**有没有根目录的新文件**——
   本仓库根目录本来就有 7 个历史遗留物（`hud_p7_screenshot.png`、
   `save.llsave`、`world_map_screenshot.png` 等，**先于本批存在，不动它们**），
   除此之外根目录不该再冒出任何新文件。
