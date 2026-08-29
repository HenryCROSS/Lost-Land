# 批次 11：通用文本输入通道（中文 / IME / 任意非 ASCII）

**基线**：`93bf907`　**分支**：`wt-textinput`　**工作树**：`../wt-textinput`　**日期**：2026-08-29

所有者原话：

> 「有没有办法让存档有中文？或者其他输入的东西都支持中文。」

**注意「其余输入的东西」**——本批做的是**平台层的通用文本输入通道**，不是给存档命名
打一个补丁。将来的角色命名、聊天、mod 搜索框都走同一条。存档命名只是它的**第一个
消费者**，顺带被改造。

分两个提交：**A = 平台通道**，**B = 存档命名改造**。

---

## 〇、开工前自己 grep 复核过的数字（不信任何口头转述）

| 事实 | 复核结果（基线 `93bf907`） |
|---|---|
| `EXPECTED_WORLD_DIGEST` | `10_180_278_885_427_934_050`，`crates/ll-world/tests/determinism.rs:303` |
| `EXPECTED_REPLAY_DIGEST` | `4_180_595_409_733_934_027`，`crates/ll-sim/tests/replay.rs:948` |
| `CONTENT_HASH_ALGORITHM_VERSION` | `27`，`crates/ll-mod/src/content_hash.rs:805` |
| winit 版本 | **0.30.13**（`Cargo.lock:3702-3703`），`crates/ll-platform/Cargo.toml:27` 锁 `"0.30"` |
| `InputState::last_physical_key` | `crates/ll-platform/src/input.rs:379`，生产者 `record_physical_key`（`:386`），`end_frame` 每帧清空（`:519`） |
| `InputContext` | 两个变体 `Gameplay`/`Menu`，`crates/ll-platform/src/keybind.rs:117` |
| `DEFAULT_MENU_BINDINGS` | 11 条，`keybind.rs:446`；**`Space` 在菜单里是 `Confirm`**（`:496`） |
| `UiMode` | 只有 `Menu` 一个变体，`crates/ll-ui/src/widget/ui_mode.rs` |
| `SlotId::from_name` 白名单 | `crates/ll-game/src/save_slot.rs:84`，只留 `is_ascii_alphanumeric()` 或 `-` `_`，**兼作路径穿越闸门** |
| `MAX_SAVE_NAME_CHARS` | `24`，`save_slot.rs:59` |
| `SaveHeader.save_name` | `String`，`crates/ll-content/src/header.rs:93`（**无长度上限，非 ASCII 无障碍**） |
| 命名屏路由 | `Demo::update_save_naming`，`crates/ll-game/src/app.rs:1509` |
| 屏切换唯一漏斗 | `crates/ll-game/src/app.rs:1219`（`self.screen = Some(next)`） |
| `app.rs` 行数 | **4331**（既有违规，交接第四节第 8 条记着这笔账）——本批新代码全部进新模块 |

**基线测试数（本会话自己跑 `bash scripts/ci/run_tests.sh`）**：**113 个测试二进制、
2714 passed / 0 failed**，`EXIT=0`。

---

## 一、现状复核：渲染早就行了，断的是输入

### 1.1 渲染不是问题

游戏已经在显示中文（首页 / 继续游戏 / 设置），文本栈是 `cosmic-text` + `glyphon`
运行期排版栅格化（`knowledge/pipelines/text-and-font-rendering.md` 第 1.2 节），
**任意字形都画得出来**，不是预烘焙图集。所以「把玩家打的中文显示出来」这一半
一行代码都不用改——只要那串 `String` 能走到 `ScreenData` 的某一行。

### 1.2 输入是问题，而且断在最底层

平台层**从来没有把「文本」接出来**。`window.rs` 的 `WindowEvent::KeyboardInput`
分支只做两件事：

```rust
if event.state == ElementState::Pressed {
    self.input.record_physical_key(code);      // ← 裸物理键，重绑用
}
let Some(action) = resolve_key_for(...) else { return };   // ← 抽象动作
```

`event.text`（winit 自带的文本字段）**被整个丢掉**，`WindowEvent::Ime(..)` 分支
**根本不存在**（落进末尾的 `_ => {}`）。

于是上一批的存档命名只能拿 `last_physical_key` 硬拼——`save_name.rs` 里那张
`typed_char` 白名单把 `KeyCode::KeyA` 映射成 `'a'`。它的模块文档已经如实写明了
后果：**打不了中文、打不了大写、打不了标点**（拿不到修饰键状态）。

**根因不是那张表写得不好，是它在回答错误的问题**：`KeyCode` 是「玩家按下了键盘上
哪个位置」，文本输入要的是「玩家实际输入了什么文字」。这两件事在 AZERTY 键盘上
就已经分叉了，到了中文输入法那里差得更远——一次上屏的「你好」根本没有对应的
物理键。

---

## 二、winit 0.30.13 实际提供什么（勘查所得，不是转述）

在 `~/.cargo/registry/src/*/winit-0.30.13/src/` 里读到的：

### 2.1 `WindowEvent::Ime(Ime)`（`event.rs:231`）

```rust
pub enum Ime {          // event.rs:774
    Enabled,
    Preedit(String, Option<(usize, usize)>),   // 串 + 光标(字节下标)区间
    Commit(String),
    Disabled,
}
```

文档原文：`Ime` 事件**必须先 `Window::set_ime_allowed(true)` 才会送来**
（`event.rs:226`）。`Preedit` 是「正在拼、尚未上屏」；`Commit` 是上屏；
「`Commit` 之前 winit 会先送一条空 `Preedit`」（`event.rs:793`）——**这一条很重要，
它意味着 preedit 的清空不需要我们自己猜时机**。

### 2.2 `KeyEvent::text: Option<SmolStr>`（`event.rs:594`）

文档原文要点：

- 多数情况等于 `logical_key` 的 `Character` 变体；
- **Windows 上死键无法合成时会一次产出两个字符**（所以它是串不是单字符）；
- 「任何有文本表示的键」都会填这个字段——**`Enter` 是 `Some("\r")`**（原文举的例）；
- 无法解释成文本时是 `None`。

### 2.3 `Window::set_ime_allowed(bool)`（`window.rs:1283`）与 `set_ime_cursor_area`（`window.rs:1248`）

`set_ime_allowed` 的平台备注：macOS 上「IME 必须开启才能收到死键组合出来的文本」；
X11 上「开启 IME 会关掉 compose 期间的死键上报」。**这两条合起来正是「IME 不能
一直开着、也不能一直关着」的技术理由**，与游戏侧「WASD 会被输入法吃掉」的理由
互相独立、结论一致。

`set_ime_cursor_area` 决定候选窗画在哪。**本批不调用它**，理由见第八节。

### 2.4 结论：两条都要接

- 只接 `Ime` → 漏掉直接键入（英文/拉丁布局下 IME 根本不参与）；
- 只接 `KeyEvent::text` → 漏掉中文（IME 上屏的串不走键盘事件）。

---

## 三、通道的形状

### 3.1 新模块 `crates/ll-platform/src/text_input.rs`

```rust
/// 一次文本编辑动作。**按发生顺序排队**，不是一个累积字符串。
pub enum TextEdit {
    /// 插入一段**已经上屏**的文本。是 `String` 不是 `char`：IME 一次上屏
    /// 一个词，Windows 的死键一次可能吐两个字符（见 winit `KeyEvent::text`
    /// 文档）。
    Insert(String),
    /// 删掉插入点前**一个字符**（不是一个字节）。
    Backspace,
}

pub struct TextInput {
    active: bool,
    edits: Vec<TextEdit>,
    preedit: String,
}
```

**为什么是有序的编辑序列，而不是「本帧文本 + 退格次数」**：一帧里玩家完全可能
「打 abc → 退格 → 打 d」。两个标量字段表达不了顺序，合成回去只能猜；`Vec<TextEdit>`
把顺序**存进类型里**，消费方照着重放就一定对。这是本仓库反复吃过亏的那类问题
（「把真相拆成两份互相依赖的副本」）的预防。

**为什么 `preedit` 跨帧保持而 `edits` 每帧清空**：`edits` 是**事件**（发生过一次
就消费掉），`preedit` 是**状态**（玩家还在拼，屏上必须一直显示着）。`end_frame()`
只清前者。这与 `last_physical_key` 每帧清空是同一条纪律的两半。

关键方法：

| 方法 | 语义 |
|---|---|
| `set_active(bool)` | 由平台层设。**关闭时同时清空 `edits` 与 `preedit`**——文本输入态一结束，没上屏的拼写串就作废了 |
| `push_committed(&str)` | 过滤控制字符后入队 `Insert`；**未激活时直接丢弃** |
| `push_backspace()` | 入队 `Backspace`；未激活时丢弃 |
| `set_preedit(String)` | 覆盖式，不是追加 |
| `edits()` / `preedit()` / `is_active()` | 只读 |
| `end_frame()` | 只清 `edits` |
| `clear()` | 清 `edits` + `preedit`，**不动 `active`** |

**`clear()` 为什么不动 `active`**：`InputState::clear()` 的语义是「这一刻按住的键
全部视为松开」（失焦、`UiModeStack` 切换）。`active` 不是按键状态，是平台层设的
**模式**；把它一起清掉会让「进文本输入态那一帧顺带 `clear()`」把刚打开的通道当场
关上。这个坑不是假想的——`UiModeStack::push` 的定义里就带着 `input.clear()`。

**控制字符过滤在哪**：`push_committed` 里，判据是「保留非控制字符」。`Enter` 的
`"\r"`、`Esc` 的 `"\u{1b}"`、`Tab` 的 `"\t"`、`Backspace` 的 `"\u{8}"` 全部在这里
被挡掉。**「只留可打印」而不是「排除这几个」**——与 `SlotId::from_name` 同一条
纪律（`asset_vfs` 那次「黑名单漏了一种写法」的代价已经付过一次）。

**注意这里刻意不做「只留 ASCII」的过滤**：那正是本批要消灭的东西。中文、标点、
大写字母全部原样通过。

### 3.2 挂进 `InputState`

`InputState` 新增一个 `text: TextInput` 字段，转发只读访问器
（`text_edits()`、`preedit()`、`text_input_active()`）与写入口。

**为什么挂在 `InputState` 上而不是另开一条回调**：`AppHandler::on_frame` 的签名
已经是 `&mut InputState`，上层每一块屏拿到的就是它。另开一条通道等于让消费方
认识两个对象，且要各自记住谁在哪一帧清空——那是第二处真相源。

### 3.3 与 `last_physical_key` 的分工（写进两边的文档）

| 通道 | 回答的问题 | 唯一正当用途 |
|---|---|---|
| `InputState::last_physical_key()` | **玩家按下了键盘上哪个位置** | **键位重绑**。玩家要绑的是「那个键」，不是「那个键打出的字」——AZERTY 上 `KeyW` 印着 Z，重绑必须记 `KeyW` |
| `InputState::text_edits()` / `preedit()` | **玩家实际输入了什么文字** | 任何文本输入框：存档名、角色名、将来的聊天与搜索 |

**`last_physical_key` 一行都不删。** 两条通道并存不是冗余：它们回答的是不同问题，
在非 QWERTY 布局与 IME 下答案会不同。

---

## 四、IME 何时开、游戏按键怎么屏蔽——同一件事，一个真相源

### 4.1 新增第三个 `InputContext::TextEntry`

```rust
pub enum InputContext { Gameplay, Menu, TextEntry }
```

配一张**只有两条**的默认表：

```rust
const DEFAULT_TEXT_ENTRY_BINDINGS: &[KeyBinding] = &[
    { Enter,  NONE, TextEntry, Confirm },
    { Escape, NONE, TextEntry, Cancel  },
];
```

**这一步就是「文本输入态下游戏按键不触发游戏动作」的全部实现，而且它是结构性的**：
在 `TextEntry` 上下文下，`KeyBindings::resolve` 对 `W`/`A`/`S`/`D`/`I`/`C`/`G`/
`Space` **查不到任何绑定**，`window.rs` 那句 `let Some(action) = ... else { return }`
直接返回。角色不可能因为玩家打了个「W」而往上走——**不是靠某处 `if` 记得跳过，
是那条路径上根本没有动作可以产出**。

**`Space` 刻意不给 `Confirm`**（菜单表里它是 `Confirm`）：在文本框里空格是一个
字符。这正是「文本输入态需要自己一张表」而不是「复用菜单表再打补丁」的理由。

必须同时改的两处：`KeyBindings::default_bindings()`（`keybind.rs:539`）与
`merged_key_bindings()`（`:785`，老配置文件的默认值补齐路径）——两处都要接上新表，
否则「有配置文件的老玩家永远拿不到这两条绑定」会成为 `crate::config` 模块文档里
那条缺陷的第三个实例。

### 4.2 `UiMode::TextEntry`，栈还是那个栈

`ll-ui` 的 `UiModeStack` 加一个变体，`current_context()` 从「空/非空」改成按栈顶
分派：

```rust
match self.stack.last() {
    None                    => InputContext::Gameplay,
    Some(UiMode::Menu)      => InputContext::Menu,
    Some(UiMode::TextEntry) => InputContext::TextEntry,
}
```

**复用既有的栈，不另起一套**：`push`/`pop` 自带的 `input.clear()` 正好是进出文本
输入态时想要的（进去时把按住的方向键视为松开、出来时同理），一行都不用新写。

`Demo` 侧在**屏切换那个唯一漏斗**（`app.rs:1219`）后面调一次
`sync_text_entry_mode(input)`：目标屏想要文本输入而栈顶不是 `TextEntry` 就压一层，
反之弹一层。判据来自 `ScreenState::wants_text_entry()`（今天只有 `SaveNaming`
返回真）。关整块屏走的 `close_screen` 本来就把栈弹空，不需要额外处理。

### 4.3 IME 开关：绑死在上下文上，不是第二个布尔

`window.rs` 每帧末尾（`end_frame()` 之后、下一批事件到达之前）同步一次：

```rust
let want = self.handler.input_context() == InputContext::TextEntry;
if want != self.ime_allowed {
    self.ime_allowed = want;
    window.set_ime_allowed(want);
    self.input.set_text_input_active(want);   // 同一个判据，同一处代码
}
```

**「IME 开着」与「文本通道收数据」与「游戏按键查不到绑定」三件事共用同一个判据**
（`input_context() == TextEntry`），因此不可能出现「IME 开了但通道没收」或「通道
收了但 WASD 还在动角色」这种半接线状态。要让它们分叉，得先把这一处拆成三处。

放在帧末而不是帧首：屏切换发生在 `on_frame` 里，帧末同步意味着**这一帧就生效**，
下一批事件已经按新上下文走。放在帧首会白白多一帧延迟。

### 4.4 `Ime` 事件的四个变体怎么处理

| 变体 | 处理 |
|---|---|
| `Enabled` | 清空 preedit（新的一轮开始） |
| `Preedit(s, _)` | `set_preedit(s)`。**光标区间参数丢弃**——见第八节 |
| `Commit(s)` | `push_committed(&s)`。winit 保证紧邻的前一条是空 `Preedit`，所以不需要在这里再清一次 |
| `Disabled` | 清空 preedit（没上屏的拼写作废） |

### 4.5 直接键入与 IME 的**双重插入**防护

`KeyboardInput` 分支在文本输入态下额外做：

```rust
if code == KeyCode::Backspace {
    self.input.push_backspace();
} else if self.input.preedit().is_empty()
    && let Some(text) = &event.text {
    self.input.push_committed(text);
}
```

**`preedit` 非空时不吃 `event.text`**：正在拼中文的时候，那些字母键是给输入法的，
不是给文本框的。这一条是保守取舍——万一某平台在 compose 期间仍然送
`KeyboardInput` 且带文本，不加这条就会把拼音重复插进去一遍。加了这条最坏情况是
「某平台下 compose 期间的直接键入丢了一个字符」，而那个场景本身就自相矛盾。

---

## 五、Preedit 怎么显示（中文能不能真的用起来的分界线）

候选窗（那个列着「你/尼/泥/逆」的小窗）由**操作系统画**，我们既画不了也不该画。
但**正在拼的那串拼音必须出现在输入框里**——玩家看不见自己打了什么，这个功能就
等于没做。

`NameField::display()` 因此改成三段拼接：

```
<已上屏文本> + <preedit> + <光标 '_'>
```

举例：玩家已上屏「我的」，正在拼 `shijie`，屏上是 `我的shijie_`；输入法一上屏，
下一帧变成 `我的世界_`。

**为什么不给 preedit 加下划线/反色来区分**：`ll_ui::screen::ScreenData` 的一行就是
一个 `String`，没有富文本。做区分要给整条渲染链加「一行里的一段」这个概念，那是
独立的一批。**先让玩家看得见，再谈好不好看**——看不见是功能缺失，没有下划线只是
不够漂亮。单列在第八节。

**长度上限只管已上屏的部分**，preedit 不计入：拼到一半的拼音不是名字的一部分，
把它算进上限会让玩家在第 23 个字时突然打不出拼音。上屏那一刻按字符截断。

---

## 六、存档命名改造：显示名与文件名彻底分开

### 6.1 两个名字，两套规则，**闸门一个字不改**

| | 显示名 | 文件名（`SlotId`） |
|---|---|---|
| 内容 | 玩家原样输入的串，**中文/大写/标点全留** | ASCII 白名单过滤后的主干 |
| 存在哪 | `SaveHeader.save_name`（`String`，上一批已加好，无长度/字符集限制） | 磁盘上的 `<主干>.llsave` |
| 谁看 | 存档列表屏、命名屏 | 只有文件系统 |
| 规则 | 长度不超过 `MAX_SAVE_NAME_CHARS` **字符** | `save_slot.rs:84` 那张白名单 + 重名追加 `-2`/`-3` |

**`SlotId::from_name` 的白名单一个字都不动。** 先读懂它为什么存在：它兼作**路径
穿越闸门**——`/`、`\`、`:`、`.` 全都不在名单里，所以 `../../etc/passwd` 过滤出来是
`_________etc_passwd`，落不出目标目录。为了支持中文去放宽它，等于用一个显示问题
换一个安全漏洞，而显示问题**根本不需要动它就能解决**：显示名走存档头，从来不经过
文件系统。

### 6.2 一个如实记录的副作用

纯中文名（例如「我的世界」）过滤后全是 `_`、trim 之后为空，退回兜底主干 `save`；
第二份纯中文名的存档因此落到 `save-2`、第三份 `save-3`。**这不影响正确性**
（重名追加机制本来就在，玩家在列表里看到的是「我的世界」这个显示名，区分靠时间
戳），只是文件名不再自解释。规格没裁定这一条，取最保守做法，单列在第八节。

### 6.3 `save_name.rs` 的改造

- **删掉** `typed_char` 那张 26 字母 + 数字的白名单，以及整条 `last_physical_key`
  路径。它存在的全部理由就是「没有文本通道」，理由已经消失。
- `NameField` 新增 `apply(&TextEdit)`（按顺序重放）、`set_preedit(&str)`；
  `display()` 改成三段拼接。
- `Backspace` 改成 `self.text.pop()`——Rust 的 `String::pop` **弹的就是一个
  `char`**，天然按字符退。24 字节只能存 8 个汉字那种错法，在这里从一开始就不成立。
- 长度上限判据保持按字符数（上一批已经这么写了，它的注释写着「将来真接上 IME 时
  这一行不用改」——现在兑现了）。
- `update_save_name` 的 `Confirm` 分支加一条守卫：**preedit 非空时不提交**。玩家
  正在拼字的时候按回车是给输入法的。

### 6.4 i18n

`screen-savename-hint` 两个语言都要改（现在写的是「字母数字可输入」，那句话本批
之后就是错的）。**不新增硬编码字符串**，`check_i18n_strings.py` 扫描通过。

---

## 七、测试：可测的全部测，测不到的如实说明

### 7.1 测不到什么，为什么

**`WindowEvent::Ime(..)` 与 `WindowEvent::KeyboardInput` 那一层测不到。** 两条独立
理由：

1. **构造不出来。** `ApplicationHandler::window_event` 要一个只有 winit 事件循环
   才造得出的 `ActiveEventLoop`；`WindowEvent` 本身虽然是公开枚举，但要把它送进
   `App::window_event` 就得先有那个参数。这与 `window.rs` 既有测试把
   `resolve_key_for` 抽成自由函数才测得到，是同一个约束。
2. **ADR 0025 禁止合成按键盲注。** 那条纪律的由来是一串合成按键泄漏进了协调者与
   所有者的对话。IME 事件比按键更不可合成——它要一个真实的输入法在真实的前台
   窗口里工作。

**因此本批不写任何「模拟一次 IME 上屏」的端到端测试，也不用 SendKeys。**
补偿手段是把事件分支里**除了「从 winit 事件取值」之外的每一点判断**都下沉到
`TextInput` 这个纯类型里，让事件分支退化成三行转发。

### 7.2 测得到的（全部用普通 `#[test]`）

平台层 `text_input.rs`：

| # | 断言 |
|---|---|
| P1 | `Insert` 的中文原样进队列、`push_committed` 不做 ASCII 过滤 |
| P2 | 控制字符被挡（`"\r"`、`"\u{8}"`、`"\t"` 打不进来），且**过滤后为空的一次插入不入队** |
| P3 | 未激活时 `push_committed`/`push_backspace` 全部丢弃 |
| P4 | `end_frame()` 清 `edits` 但**保住 `preedit`** |
| P5 | `set_active(false)` 同时清 `edits` 与 `preedit` |
| P6 | `clear()` 清 `edits`+`preedit` 但**不动 `active`** |

绑定表 `keybind.rs`：

| # | 断言 |
|---|---|
| K1 | `TextEntry` 上下文下 `KeyW`/`Space` 解析成 `None`（游戏按键在文本输入态下产不出动作） |
| K2 | `TextEntry` 上下文下 `Enter` 得 `Confirm`、`Escape` 得 `Cancel` |
| K3 | 老配置文件走 `fill_missing_defaults` 之后拿得到这两条（第 4.1 节那条「老玩家永远拿不到」的预防） |

`ll-ui` `ui_mode.rs`：

| # | 断言 |
|---|---|
| U1 | 栈顶是 `TextEntry` 时 `current_context()` 是 `InputContext::TextEntry`；弹掉之后回到 `Menu` |

`ll-game` `save_name.rs`：

| # | 断言 |
|---|---|
| S1 | 中文能输入：一串 `Insert("你")`/`Insert("好")` 之后 `text()` 是 `"你好"` |
| S2 | 大写与标点能输入（`Insert("Hello, 世界!")`） |
| S3 | **退格按字符退**：`"你好"` 退一次剩 `"你"`（不是半个 UTF-8 字节） |
| S4 | **长度上限按字符数**：打 `MAX+5` 个汉字，字符数恰为 `MAX`，且内容是合法 UTF-8 |
| S5 | **preedit 与已上屏文本拼接显示**：上屏「我的」+ preedit `shijie` 得到 `我的shijie_` |
| S6 | preedit 不计入长度上限：已上屏 `MAX` 个字符时 preedit 仍然显示得出来 |
| S7 | 编辑序列**按顺序重放**：`Insert("abc")` → `Backspace` → `Insert("d")` 得 `"abd"` |
| S8 | preedit 非空时按确认不提交 |

`ll-game` 接线（`app.rs` 单元测试）：

| # | 断言 |
|---|---|
| W1 | 屏切到 `SaveNaming` 之后 `Demo::input_context()` 是 `TextEntry`；切走之后回到 `Menu` |

### 7.3 ADR 0018 反例验证（每条都要真的改坏一次）

| 断言 | 故意改坏成 | 预期 |
|---|---|---|
| P2 | 过滤条件改成恒真（不挡控制字符） | 红 |
| P3 | `push_committed` 去掉 `active` 判定 | 红 |
| P4 | `end_frame()` 顺手清 `preedit` | 红 |
| P6 | `clear()` 顺手把 `active` 设 false | 红 |
| K1 | 把 `TextEntry` 的解析退回查 `Menu` 表 | 红 |
| K3 | `merged_key_bindings` 不接上新表 | 红 |
| U1 | `current_context` 退回「空/非空」二分 | 红 |
| S3 | 退格改成按字节截断 | 红 |
| S4 | 上限改成按字节数 | 红 |
| S5 | `display()` 不拼 preedit | 红 |
| S7 | 把 `edits` 换成「累积串 + 退格计数」再合成 | 红 |
| W1 | `sync_text_entry_mode` 不接线 | 红 |

---

## 八、规格没裁定、本批临时选的做法（**这一节最重要**）

1. **纯中文存档名的文件名主干退化成 `save`/`save-2`。** 见 6.2。最保守：不动安全
   闸门、不引入音译表、不改文件名规则。**可反转**：将来要「文件名带可读线索」，
   加一个「取名字里的 ASCII 片段，没有就用时间戳」的规则即可，存档头里的显示名
   一直是完整的。
2. **preedit 在屏上与已上屏文本没有视觉区分**（不加下划线/反色）。见第五节。
   可反转：等 `ScreenData` 支持「一行里的一段」时再上。
3. **不调用 `set_ime_cursor_area`。** 候选窗因此画在窗口默认位置，不跟着文本框
   走。理由：平台层不知道文本框在屏幕上的哪里，要传进来就得给 `AppHandler` 加一条
   「当前文本框矩形」的回调，那是为一个纯观感问题引入的一条跨层通道。X11 上这个
   API 本来就只支持位置不支持区域（winit 文档原话）。可反转且改动局部。
4. **compose 期间不吃 `event.text`。** 见 4.5。取「宁可少一个字符也不要重复插入」。
5. **`Ime::Preedit` 的光标区间参数丢弃。** 我们只显示串本身、光标恒在末尾。要用
   它得先有「行内插入点」这个概念，而本批的输入框连插入点移动都没有。
6. **文本输入态下不做插入点移动 / 选区 / 剪贴板 / 撤销。** 与上一批同一条取舍，
   本批没有扩大范围——本批解决的是「打不了中文」，不是「做一个完整的文本编辑器」。
7. **`InputState::clear()` 不清 `active`。** 见 3.1。这是一条真实的取舍：另一种
   写法是让 `clear()` 也清、由平台层每帧无条件重设。选了前者，因为「模式不是按键
   状态」这条区分更容易讲清楚，且不依赖「每帧都记得重设」。

---

## 九、黄金基准

**本批不碰世界状态、不碰内容表、不碰任何进 `WorldState` 的东西**——改的是平台
输入层、UI 上下文栈、一块屏的状态机。预期两条摘要**逐位不变**：

- `EXPECTED_WORLD_DIGEST` = `10_180_278_885_427_934_050`
- `EXPECTED_REPLAY_DIGEST` = `4_180_595_409_733_934_027`

**实跑验证，不靠推理**：改后单独跑 `ll-world` 的 `determinism` 与 `ll-sim` 的
`replay` 两个测试二进制，结果写进报告。

`CONTENT_HASH_ALGORITHM_VERSION` 保持 `27`（没有新增/改动任何内容定义）。

---

## 十、落地清单

### 新增文件

| 文件 | 内容 |
|---|---|
| `crates/ll-platform/src/text_input.rs` | `TextEdit` / `TextInput` + P1–P6 |

### 改动的既有文件

| 文件 | 改什么 |
|---|---|
| `ll-platform/src/lib.rs` | `pub mod text_input;` |
| `ll-platform/src/input.rs` | `InputState` 加 `text` 字段 + 转发访问器；`end_frame`/`clear` 接线；`last_physical_key` 文档补「分工」一节 |
| `ll-platform/src/keybind.rs` | `InputContext::TextEntry` + `DEFAULT_TEXT_ENTRY_BINDINGS` + 两处接表 + K1–K3 |
| `ll-platform/src/window.rs` | `Ime` 分支、`KeyboardInput` 分支的文本抽取、帧末 IME 同步；两个分支抽成方法保住 50 行上限 |
| `ll-ui/src/widget/ui_mode.rs` | `UiMode::TextEntry` + `current_context` 按栈顶分派 + `top()` + U1 |
| `ll-game/src/menu_screen.rs` | `ScreenState::wants_text_entry()` |
| `ll-game/src/app.rs` | `sync_text_entry_mode`（新方法，短）+ 漏斗处一行调用 + W1 |
| `ll-game/src/save_name.rs` | 删 `typed_char`，改吃新通道，preedit 显示，S1–S8 |
| `assets/locales/en.ftl`、`assets/locales/zh-CN.ftl` | `screen-savename-hint` 改写 |

### 提交划分

- **提交 A**：平台通道（`ll-platform` + `ll-ui`）
- **提交 B**：存档命名改造（`ll-game` + 两份 `.ftl`）

`app.rs` 已 4331 行，新代码只在其中加一个不到 15 行的同步方法与一条调用；其余全部
进新模块或既有的小模块。
