# 批次 6：游戏主菜单（首页）与「设置屏每帧替换键位表」缺陷

**基线**：`c17f4a9`　**分支**：`wt-titlescreen`　**日期**：2026-08-28

两件事一批做，但**分两个提交**：

- **任务 A**（先提交）：修掉设置界面每帧无条件替换平台层键位表。所有者实机撞到，
  现象是设置屏一开，终端每帧刷一行 `键位绑定表已由上层替换`。
- **任务 B**：游戏主菜单（首页）——开始游戏 / 读取存档 / 设置 / 离开。所有者原话：
  「既然是 P7，那么就需要进入游戏的首页……我需要一个游戏的主菜单，而不是开始直接
  进入存档。」

本批大量复用上一批（`2026-08-27-batch4-menu-and-settings.md`）的成果：设置屏原样复用，
不写第二份；`UiModeStack`、`ScreenData`、`ScreenOutcome` 全部沿用。

---

## 〇、开工前自己 grep 复核过的数字（不信任何口头转述）

| 事实 | 复核结果（基线 `c17f4a9`） |
|---|---|
| `EXPECTED_WORLD_DIGEST` | `10_180_278_885_427_934_050`，`crates/ll-world/tests/determinism.rs:286`——**交接文档里写的 `17_228_492_522_544_021_674` 已经过期**，它自己也写了「以代码为准」 |
| `EXPECTED_REPLAY_DIGEST` | `6_885_882_507_408_978_859`，`crates/ll-sim/tests/replay.rs:910`——同上，交接文档里的 `14_731_332_643_995_045_404` 已过期 |
| 每帧塞值的那一行 | `crates/ll-game/src/app.rs:758` |
| `take_rebound_keys` 生产实现 | `crates/ll-game/src/app.rs:1544`（`self.pending_bindings.take()`） |
| 那句 INFO 日志 | `crates/ll-platform/src/window.rs:432` |
| 真正改键位的两处入口 | `crates/ll-game/src/menu_screen.rs:214`（`try_rebind`）、`:238`（`clear_bindings`） |
| 模态屏开着时 `advance` 整段早退 | `crates/ll-game/src/app.rs:608` |
| `on_exit` 无条件存档 | `crates/ll-game/src/app.rs:1547`-`1549`（`save_on_exit` 本体在 `:883`） |
| 存档只有一份 | `GamePaths::save`（`crates/ll-game/src/lib.rs:74`）是单个文件路径，`SAVE_FILE_NAME` 全仓库只有一个取值 |
| `self.game_world` 在 `app.rs` 的出现次数 | 19 处 |
| `Demo` 的 `game_world`/`camera`/`engine`/`continent_field`/`npc_ai` 五个字段 | 全部**非可空**，且后四个都在 `Demo::new` 里由 `game_world` 推出 |

基线测试数由本会话自己跑 `bash scripts/ci/run_tests.sh` 取得，写在第九节。

---

## 一、任务 A：根因复核与修法

### 1.1 根因（复核结论：所有者的定位成立）

`app.rs:746`-`769` 是 `update_screen` 里 `ScreenState::Settings` 那一支。它每帧**无条件**
执行：

```rust
self.pending_bindings = Some(self.config.bindings.clone());
```

而紧挨着的注释写的是：

> 整表克隆只发生在玩家真的在这块屏里操作的那些帧，不是每帧。

**注释与代码直接矛盾**——`update_screen` 在设置屏开着时每帧都被 `on_frame` 调一次，
这一行因此每帧都跑。规格 §13：文档与代码不一致即视为缺陷，所以这里有两个缺陷（浪费
与说谎的注释），不是一个。

`take_rebound_keys`（`app.rs:1544`）与平台层那一侧（`window.rs:429`-`433`）的语义都是
**对的**：取走而不是借出，`Option` 为空就什么都不做。问题**纯在生产端**。

后果分三层：

1. 每帧克隆一整张 `KeyBindings`（默认表 20 个动作、31 条绑定）；
2. 平台层每帧整表替换；
3. 每帧一行 INFO 日志——**这一层最贵**：那条日志本来是为「键位真的被替换了」这件
   稀有事件准备的，每帧刷之后它的信号价值归零，等于把一条诊断线索烧掉了。

### 1.2 修法：让改键位的那两处显式报告「我改了」

**明确否决**「每帧克隆一份再和旧表比较」：那只是把浪费从「克隆 + 替换 + 日志」减成
「克隆 + 比较」，克隆本身一次没少。

真正改键位的入口全仓库只有两处，都在 `menu_screen.rs`：

- `apply_capture`（`:395`-`:407`）——`try_rebind` 成功后 `ctx.config.bindings = bindings`；
- `update_capture` 的 `KeyCode::Backspace` 分支（`:370`-`:374`）——调 `clear_bindings`。

两者都在 `update_settings` 内部。于是把 `update_settings` 的返回值从裸元组改成一个
具名结构体：

```rust
/// 一次设置界面输入处理的全部产出。
pub struct SettingsUpdate {
    pub outcome: ScreenOutcome,
    pub notice: Option<ScreenNotice>,
    /// 这一帧**真的**改动了键位表——只有为真时调用方才需要把整表送回
    /// 平台层。
    pub rebound: bool,
}
```

`rebound` 只在上述两条路径上置真。`app.rs` 那一行改成：

```rust
if update.rebound {
    self.pending_bindings = Some(self.config.bindings.clone());
}
```

**为什么是具名结构体而不是三元组**：`(ScreenOutcome, Option<ScreenNotice>, bool)` 的
第三位在调用点上读不出含义，而这个布尔的全部价值就在于它的名字。

**为什么不从 `ScreenNotice` 推导**：`ScreenNotice::Bound`/`Cleared` 恰好就是那两条
路径的提示，`matches!` 一下确实能省掉这个字段。否决理由：那会把「屏幕上对玩家说什么」
和「要不要把表送回平台层」绑成同一件事——将来有一条改了键位但不说话（或说别的话）的
路径，缺陷会以「改了键位不生效」的形态回来，而那是本项目已经付过一次代价的形状
（上一批 D3 的存在理由）。

### 1.3 顺带修的那句注释

改成实话：说明它只在 `rebound` 为真的那些帧发生，并点名是哪两条路径置的位。

`window.rs:432` 那行 `tracing::info!` **保留**——修好之后它才第一次真正有信号价值。

### 1.4 两条测试（都要反例验证）

| # | 断言 | 落点 |
|---|---|---|
| A1 | 设置屏开着、玩家什么都不按 → `take_rebound_keys()` 返回 `None` | `crates/ll-game/tests/menu_and_settings.rs` + `app.rs` 单元测试 |
| A2 | 玩家真的重绑之后 → 返回 `Some`，且内容里那个动作确实绑上了新键 | 同上 |

A1 的反例：把 `if update.rebound` 那道判断去掉（恢复成无条件塞值），A1 必须变红。
A2 的反例：把 `rebound` 永远置假，A2 必须变红。

`app.rs` 的既有测试 `demo.take_rebound_keys()`（`:2657`/`:2663`）覆盖的是「菜单开着
按键」这条路径，与本批新增两条不重叠，保留。

---

## 二、任务 B 的最大结构风险：「世界尚未存在」

### 2.1 勘查结论

启动流程现状（`crates/ll-game/src/lib.rs:418`）：`run_game` 在建窗口之前就
`load_or_new_game`，然后把 `GameWorld` **按值**交给 `Demo::new`。`Demo` 的这五个字段
全部非可空，且后四个都是从 `game_world` 现推的：

| 字段 | 来源 | 依赖世界吗 |
|---|---|---|
| `game_world` | 参数 | —— |
| `camera` | 玩家坐标 + `world.size` | **是** |
| `engine` | `mem::take(&mut game_world.timeline)` | **是** |
| `continent_field` | `game_world.noise` + `params` | **是** |
| `npc_ai` | `content` + `game_world.world.seed` | **是**（种子） |
| `zoom`/`anim`/`walk_clip`/`idle_clip`/`settlement_roles`/`menu`/`hud_anim`/… | 只依赖 `content`/常量 | 否 |

所以「世界尚未存在」这个状态**同时**让五个字段没有意义。把它们各自改成 `Option` 是
五个可空点、五处解包，且它们的可空性永远同生同死——那是一个典型的「本该是一个
`Option` 却写成五个」的形状。

### 2.2 处理方式：新增 `Session`，`Demo::session: Option<Session>`

新文件 `crates/ll-game/src/session.rs`：

```rust
/// 一局游戏运行期的全部状态——**只有玩家真的进了世界之后才存在**。
pub struct Session {
    pub game_world: GameWorld,
    pub camera: Camera,
    pub engine: TurnEngine,
    pub continent_field: ContinentField,
    pub npc_ai: NativeBehaviorSource,
}

impl Session {
    /// 从一局刚建好或刚读回来的世界推出运行期状态——`Demo::new` 与
    /// 首页的「开始游戏 / 读取存档」走同一条路，不写两份。
    pub fn begin(game_world: GameWorld, content: &LoadedContent) -> Session;
}
```

`Demo` 的那五个字段合并成一个 `session: Option<Session>`。`None` 就是「还在首页，
世界尚未存在」这个状态的**唯一**表示。

**为什么不把 `Demo` 拆成 `enum { Title, InGame }`**：`Demo` 还持有 `content`/`config`/
`catalog`/`resources`/`save_path` 等十几个两种状态下都要用的字段（首页也要画字、也要
进设置屏改配置、也要 GPU），拆枚举会让它们全部重复一份或再套一层。`Option<Session>`
把可空性收敛到恰好那一处真正会空的东西上。

### 2.3 三处早退的纪律

复用上一批 D9 的先例，`session` 为 `None` 时：

1. `Demo::advance` 整段早退（世界一个字节都不动）——与 `self.screen.is_some()` 并列
   一道闸门。**两道都要**：首页是一块屏，第一道就挡住了；但第二道守的是不同的东西
   ——「没有世界可推进」这件事本身，将来任何一条绕过屏的路径也会被它挡住。
2. `Demo::maintain_streaming` 早退。
3. `on_frame` 的渲染段：不画世界层、不画 HUD，只画屏。世界层的离屏目标仍然
   `batch.flush` 一次（空批次，`wgpu::LoadOp::Clear(BLACK)`），首页背后因此是干净的
   黑，不是上一帧的残影或未初始化内存。

### 2.4 `on_exit`：首页直接离开时**不存档**

现状 `on_exit` 无条件 `save_on_exit()`，而 `save_on_exit` 读 `&self.game_world`——
`session` 为 `None` 时那个字段根本不存在，编译期就过不去。修法是把它变成显式的判断：

```rust
fn on_exit(&mut self) {
    let Some(session) = self.session.as_ref() else {
        // 从首页直接离开：这一局从来没有开始过，没有任何世界状态可存。
        // 无条件存档会写出一份「玩家从未玩过」的垃圾档，反而可能覆盖掉
        // 磁盘上真正有价值的那一份存档（存档只有一份）。
        tracing::info!("从首页退出，没有进行中的世界，跳过退出存档");
        return;
    };
    ...
}
```

**这一条不只是防 panic，是防数据丢失**：存档只有一份，从首页按「离开」若照旧存档，
会把一个空世界写到玩家真正的存档上。

---

## 三、`UiModeStack` 怎么表达首页

### 3.1 结论：语义上原样容纳，只缺一个构造器

`ui_mode.rs` 的设计意图是「栈非空 ⇔ 有一块模态 UI 盖着 ⇔ 按 `InputContext::Menu`
查表」。它**从来没有**把「栈底是 Gameplay」编码进任何地方——`current_context()` 只
问一句「空不空」。首页恰好是「栈里压着一层 Menu，只是它底下没有世界」，
`current_context()` 返回 `Menu`，正是我们要的。

所以**不新增 `UiMode` 变体，不改 `current_context()`**。真正变化的是那句不变式的
措辞，要在 `Demo::ui_modes` 字段文档里改写（不是删掉）：

- 改前：栈非空 ⇔ 有一块模态屏盖在世界上 ⇔ …… ⇔ `advance` 整段早退。
- 改后：栈非空 ⇔ 有一块模态屏 ⇔ …… ⇔ `advance` 整段早退。**栈空则必然有世界在跑**
  （反过来不成立：首页时栈非空而世界不存在）。

### 3.2 唯一的扩展：`UiModeStack::opened`

`push`/`pop` 都要 `&mut InputState`，因为上下文切换必须 `clear()`（设计文档 2.3 节的
硬结论：第三种「隐式全键松开」边界）。但首页在**第一帧之前**就已经开着——`Demo::new`
那一刻整个平台层事件循环还没起来，根本没有 `InputState` 可传。

于是新增一个构造器：

```rust
/// 建一个**开局就压着一层**的栈——首页（游戏主菜单）在第一帧之前就
/// 已经盖在屏幕上了，那一刻平台层事件循环还没启动，不存在任何
/// `InputState` 可以清空，也没有任何键可能正被按住。
///
/// 这不是 `push` 的旁路：`push`/`pop` 要求 `&mut InputState` 是因为它们
/// 表达的是**运行期的一次上下文切换**（切换那一刻可能正按着键）；本
/// 构造器表达的是**初始状态**，没有「切换前」可言。
pub fn opened(mode: UiMode) -> UiModeStack
```

不用它的替代方案（在第一帧里补一次 `push`）被否决：那让首页的第一帧之前
`input_context()` 返回 `Gameplay`，而平台层的按键 resolve 发生在帧与帧之间——第一
批按键会按错表解析。

---

## 四、首页的形状

### 4.1 状态机（`menu_screen.rs` 扩展）

```rust
pub enum ScreenState {
    Title,                       // 新增
    Menu,
    Settings { cursor, capturing, origin: SettingsOrigin },  // origin 新增
}

/// 设置屏是从哪儿进来的——按取消/返回要回到那里。
pub enum SettingsOrigin { Title, Menu }

pub enum ScreenOutcome {
    Idle,
    Close,
    Quit,
    StartNewGame,   // 新增
    LoadSave,       // 新增
}
```

`SettingsOrigin` 是必需的，不是装饰：设置屏现在有两个入口，按「返回」必须回到进来
的那一个。写死回 `Menu` 会让从首页进设置的玩家被扔进一个**底下没有世界**的暂停菜单。

首页四项，`TITLE_ITEM_IDS` 四个静态 id + `TITLE_ITEM_KEYS` 四条 Fluent 键，与菜单屏
`MENU_ITEM_IDS` 完全同构，走同一个 `navigate_focus`。

### 4.2 「读取存档」不可用时

存档是否存在在 `Demo` 构造时算一次（`save_path.exists()`），存成一个布尔字段——
首页停留期间没有任何路径能改变它，每帧 stat 一次文件系统是白付的开销。

**不可用时的表现（保守取舍，见第八节）**：该行仍然显示（位置固定，玩家知道这个功能
存在），但文案换成 `screen-title-load-empty`（「读取存档（没有存档）」），确认键落在
它上面**什么都不发生**，只给一句 `screen-title-no-save` 的提示。

**为什么不做成真正的置灰或跳过**：`ll_ui::screen::ScreenData` 目前没有「逐行禁用样式」
这个概念，加它要动 `screen/mod.rs` 的数据形状与 `render.rs` 的配色；而导航跳过会让
光标在按一下方向键之后跳两格，是一种玩家会当成 bug 的手感。两者都超出本批范围。

### 4.3 「开始游戏」= 直接建新档进世界（本批范围）

```rust
ScreenOutcome::StartNewGame => {
    let world = crate::new_game(&self.content, &self.config.new_game);
    self.enter_world(world, input);
}
```

角色创建（种族/性别/职业）、世界配置、选重生点**不在本批**。衔接点写在第七节。

### 4.4 「读取存档」

把 `lib.rs` 的 `load_or_new_game` 拆成两个 `pub(crate)` 函数，**行为逐字不变**：

```rust
pub(crate) fn load_saved_game(save: &Path, content: &LoadedContent) -> Option<GameWorld>
pub(crate) fn new_game(content: &LoadedContent, cfg: &NewGameConfig) -> GameWorld
fn load_or_new_game(...) -> GameWorld {          // 保留，供既有测试与文档引用
    load_saved_game(&paths.save, content).unwrap_or_else(|| new_game(content, &cfg))
}
```

首页的「读取存档」调 `load_saved_game`。读失败（损坏 / 降级只读）时**留在首页**并给
一句 `screen-title-load-failed`，**不**静默回退到新游戏——启动期那条回退是「玩家已经
决定要玩了，给他一个能玩的世界」；首页这条不同，玩家明确点的是「读取存档」，给他一个
新世界是答非所问，而且下次退出就把那份坏档彻底覆盖了。

### 4.5 「设置」与「离开」

- 设置：`ScreenState::Settings { cursor: 0, capturing: false, origin: SettingsOrigin::Title }`
  ——**同一块屏**，一行都不复制。
- 离开：`ScreenOutcome::Quit` → `FrameOutcome::Exit`，与暂停菜单那一项同一条路。

### 4.6 进入世界那一刻

```rust
fn enter_world(&mut self, world: GameWorld, input: &mut InputState) {
    self.session = Some(Session::begin(world, &self.content));
    self.close_screen(input);   // 把首页那一层弹掉，上下文回 Gameplay
}
```

`close_screen` 现有实现是 `while self.ui_modes.pop(input).is_some() {}` + `screen = None`
——原样可用。

---

## 五、落地形状

### 新增文件

| 文件 | 内容 | 预估行数 |
|---|---|---|
| `crates/ll-game/src/session.rs` | `Session` + `Session::begin` + 模块文档 | ~110 |
| `crates/ll-game/tests/title_screen.rs` | 首页行为断言 | ~260 |

### 改动的既有文件

| 文件 | 改什么 |
|---|---|
| `crates/ll-ui/src/widget/ui_mode.rs` | 新增 `UiModeStack::opened` |
| `crates/ll-game/src/menu_screen.rs` | A：`SettingsUpdate`；B：`ScreenState::Title`、`SettingsOrigin`、两个新 `ScreenOutcome`、`update_title` |
| `crates/ll-game/src/settings_view.rs` | `title_row_texts`；`menu_focus_index` 泛化 |
| `crates/ll-game/src/app.rs` | A：一行判断 + 注释；B：`session: Option<Session>`、`save_exists`、渲染/推进/退出三处闸门、`enter_world` |
| `crates/ll-game/src/lib.rs` | 拆 `load_or_new_game`；`run_game` 改走首页 |
| `crates/ll-game/tests/menu_and_settings.rs` | 跟随 `SettingsUpdate` 与 `SettingsOrigin` 的机械改写 + A1/A2 两条新断言 |
| `assets/locales/{en,zh-CN}.ftl` | 首页全部新文案，**两个语言都加** |

**不碰**：`crates/ll-world/src/overview.rs`、`crates/ll-ui/src/hud/`（`wt-worldmap` 在改）。

### 关于「收拢 `screen/` 里那十行复制的 `hud::build_panel`」

上一批为躲开当时并行的批次，在 `crates/ll-ui/src/screen/` 里刻意复制了
`hud::build_panel` 的十行算法。**本批不收拢**：`wt-worldmap` 正在改 `crates/ll-ui/src/hud/`，
收拢必然要动 `hud/` 下的文件（至少要把 `build_panel` 的可见性/位置改一次），那正好落在
本批被明令禁止触碰的目录里。判断：有风险，别动。留给 `wt-worldmap` 合并之后的批次。

---

## 六、必须新增的测试（每条按 ADR 0018 用「故意改坏」验证会红）

| # | 断言 | 落点 |
|---|---|---|
| A1 | 设置屏开着、玩家什么都不按 → 不产生待送回的键位表 | `tests/menu_and_settings.rs` |
| A2 | 玩家真的重绑之后 → 产生待送回的键位表，且内容正确 | 同上 |
| B1 | 新建的 `Demo` 停在首页：`session` 为空、屏是 `Title`、输入上下文是 `Menu` | `tests/title_screen.rs` |
| B2 | 首页四项的行文字全部解析成功（没有一条回落成 Fluent 键名） | 同上 |
| B3 | 有存档时「读取存档」那一行用的是正常文案；无存档时用的是 `-empty` 文案 | 同上 |
| B4 | 无存档时确认「读取存档」→ `Idle` + `NoSave` 提示，**不**进世界 | 同上 |
| B5 | 确认「开始游戏」→ `ScreenOutcome::StartNewGame` | 同上 |
| B6 | 确认「离开」→ `ScreenOutcome::Quit` | 同上 |
| B7 | 从首页进设置屏，按取消**回首页**而不是回暂停菜单 | 同上 |
| B8 | 从暂停菜单进设置屏，按取消回暂停菜单（守住既有行为不被 `origin` 改坏） | `tests/menu_and_settings.rs` |
| B9 | 首页开着时 `Demo::advance` 不推进世界（`session` 为空，什么都不发生） | `app.rs` 单元测试 |
| B10 | `UiModeStack::opened(Menu)` 的深度为 1、上下文为 `Menu` | `ll-ui` `ui_mode.rs` |

---

## 七、下一批的衔接点（角色创建 / 世界配置 / 选重生点）

**唯一的衔接点是 `ScreenOutcome::StartNewGame` 在 `app.rs` 里的那一个 `match` 臂**
（第 4.3 节那三行）。本批它直接：

```
StartNewGame → crate::new_game(content, &config.new_game) → Session::begin → 进世界
```

下一批要把这三行换成「先进一串新的 `ScreenState`」：

```
StartNewGame → ScreenState::CharacterCreation { … }   （种族 / 性别 / 职业）
             → ScreenState::WorldSetup { … }          （四档预设 + 四个旋钮，
                                                        数据侧已就绪：`NewGameConfig`
                                                        的 sea_level / mountain_level /
                                                        octaves / continent_shrink）
             → ScreenState::SpawnPick { … }           （选重生点）
             → crate::new_game_with(…) → Session::begin → 进世界
```

三件事在数据侧的现状（下一批不用从零开始）：

- **世界配置**：`ll_platform::config::NewGameConfig` 四个旋钮已经进了新游戏流程并进
  存档（交接文档二节「世界生成参数」），下一批只需要给它一块屏，`crate::new_game` 的
  签名一个字都不用改。
- **角色创建**：`Demo::character_name` 现在写死 `"旅人"`（`lib.rs:459`），种族/职业
  由 `crate::world::build_new_world` 内部选定——这两处是种族/性别/职业要接进去的地方。
- **选重生点**：需要 `ll_world::overview::ContinentField`（首页阶段还没有世界，得先
  按选定参数生成一次地形场再让玩家点）。**注意 `wt-worldmap` 正在改 `overview.rs`**，
  下一批开工前先确认它合并后的形状。

`Session::begin` 是这四条路径共同的终点，本批把它做成公开的单一入口正是为此。

---

## 八、规格没裁定、本批临时选的做法（留给所有者）

1. **「读取存档」不可用时是换文案 + 确认无效，不是真正的置灰或跳过**（4.2）。真正
   的置灰要给 `ll_ui::screen::ScreenData` 加「逐行禁用」的概念。
2. **首页读档失败时留在首页并报错，不回退到新游戏**（4.4）。与启动期
   `load_or_new_game` 的既有回退语义**不同**，理由写在 4.4。
3. **本批不做「回到主菜单」这一项**（暂停菜单里的「退出游戏」仍然是退出进程）。
   回到首页要求把 `session` 置空并决定「要不要先存档」，是一个独立的决定面。
4. **首页背后画的是纯黑**，不是任何标题画面/背景图。资产侧没有这样一张图，凭空造一张
   不在本批范围。
5. **`SettingsOrigin` 只有两个变体**，将来若有第三个入口需要再加一个变体（而不是存
   一个「上一块屏」的通用返回栈）——通用返回栈是 `UiModeStack` 的职责范围，但那个栈
   目前只表达上下文、不表达具体屏，扩展它是独立的一批。
6. **`Demo::new` 保留**（构造一个已经在世界里的 `Demo`），新增 `Demo::at_title`。
   保留的理由是 `app.rs` 里十几条既有测试全部依赖 `Demo::new` 直接进世界；把它们
   全部改成「先过一遍首页」会让那些测试的主题（时钟推进、buff 到期、背包）被无关的
   UI 状态污染。

---

## 九、黄金基准与基线测试数

本批**不碰任何世界状态语义**：改的是输入层、UI 层、启动流程与 `Demo` 的字段分组，
`WorldState` 的结构、`resolve`/`apply`、`build_new_world` 一个字节都不动。

预期两条黄金基准逐位不变：

| 基准 | 期望值 |
|---|---|
| `crates/ll-world/tests/determinism.rs` | `10_180_278_885_427_934_050` |
| `crates/ll-sim/tests/replay.rs` | `6_885_882_507_408_978_859` |

真变了就说明改动漏进了世界状态——**不要改常数**，走四步重冻并解释。

基线测试数（本会话自己跑的 `bash scripts/ci/run_tests.sh`，改动**前**）：
**111 个测试二进制、2512 passed / 0 failed / 0 ignored**，exit 0。

两批改完之后：**112 个二进制、2533 passed / 0 failed / 0 ignored**，
`bash scripts/ci/run_all.sh` exit 0。两条黄金基准常数一字未动，实跑通过。

---

## 十、范围边界（不要越界）

- **不碰** `crates/ll-world/src/overview.rs` 与 `crates/ll-ui/src/hud/`。
- **不新增任何 `examples/` target**（`scripts/ci/run_acceptance_demos.sh` 的登记表
  因此不受影响；理由同上一批第九节偏差 5）。
- **不用合成按键做验收**（ADR 0025）——全部普通 `#[test]`，程序化驱动公开路径。
- **不做**角色创建 / 世界配置 / 选重生点。
- **不做**「回到主菜单」。
- **不收拢** `screen/` 里复制的 `build_panel`（第五节末）。

---

## 十一、落地之后：与本计划的偏差

计划是开工前写的，落地过程中有五处与它不符。**逐条如实记录**，不回头改前面几节
假装计划一开始就是这样。

### 偏差 1：首页的状态机住在新文件 `title_screen.rs`，不是 `menu_screen.rs`

第五节写的是「`menu_screen.rs`：新增 `ScreenState::Title`、两个新 `ScreenOutcome`、
`update_title`」。实际把 `update_title` 那一套写进去之后，`menu_screen.rs` 变成
**828 行**，越过本仓库 800 行的上限。于是把首页那一套（`TITLE_ITEM_IDS`/
`TITLE_ITEM_KEYS`/`TITLE_LOAD_ROW`/`TitleUpdate`/`update_title`/`title_focus_index`）
搬进新文件 `crates/ll-game/src/title_screen.rs`（153 行），`menu_screen.rs` 回到
707 行。

共用的类型（`ScreenState`/`ScreenOutcome`/`ScreenNotice`/`SettingsOrigin`）仍然只有
一份，住在 `menu_screen`——拆的是文件，不是职责。

### 偏差 2：`Demo::advance` 被拆成两半

计划只说「加一道 `session` 闸门」。实际做不到只加一行：`advance` 后半段要持一个
`&mut Session` 全程活着，而 `maintain_streaming`/`update_zoom` 是 `&mut self` 的
方法，两者不能同时借。于是把后半段整体搬进新方法 `Demo::run_turn`——纯搬运，一行
逻辑都没改。副作用是那个早已越过 50 行上限的函数被切小了一截。

顺带：`cleanup_aged_ground_items` 因此从「`maintain_streaming` 紧后面」挪到了
「`update_zoom`/`update_player_animation` 之后」。同一帧内、且那两步都不碰地面物品，
行为不变。

### 偏差 3：`Demo::new` 与 `Demo::at_title` 共用一个私有 `assemble`

计划没写这一层。写的时候发现两个构造器如果各自摆一遍二十几个字段的结构体字面量，
就会出现「首页那份忘了改 `ui_modes`」这类只可能靠人眼发现的漂移。改成
`assemble(content, session: Option<Session>, …)`，**屏与模态栈的初值完全由
`session` 是不是 `None` 推出来**，结构上不可能拼出「停在首页但栈是空的」这种自相
矛盾的组合。

### 偏差 4：`app.rs` 的测试改用 `test_world()`/`test_world_mut()`

`session` 合并之后，`app.rs` 里四十多处 `demo.game_world` 全部失效。没有让每条断言
各写一遍 `session.as_ref().unwrap()`（那是四十多行与断言主题无关的解包噪音），而是
加了一对 `#[cfg(test)]` 的取用方法。

### 偏差 5：交接文档里那两个黄金基准常数是过期的

`knowledge/handoff/2026-08-27-session-handoff.md` 第一节列的是
`17_228_492_522_544_021_674` / `14_731_332_643_995_045_404`，代码里实际是
`10_180_278_885_427_934_050` / `6_885_882_507_408_978_859`。本计划第〇节最初照抄了
交接文档，开工后 grep 复核时发现并改正——**那份文档自己就写了「以代码为准」，这次
是那条纪律第一次真的派上用场**。
