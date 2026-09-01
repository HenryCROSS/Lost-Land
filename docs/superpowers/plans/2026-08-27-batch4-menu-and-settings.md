# 批次 4：游戏内菜单、设置界面与输入上下文接线

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，全文 0 次出现「反例」。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**基线**：`03555cd`　**分支**：`wt-menusettings`　**日期**：2026-08-27

来源：`knowledge/audit/2026-08-26-phase-reckoning-p6-p8.md` 三节认定 P7 未完成的
两项交付物（游戏内菜单、设置界面），加上交接文档 `knowledge/handoff/2026-08-27-session-handoff.md`
第四节第 16/17/18 三条待裁定（所有者已确认按推荐执行）。

**基线测试数（本会话自己跑的 `bash scripts/ci/run_tests.sh`）**：106 个测试二进制、
**2405 passed / 0 failed / 0 ignored**，exit 0。

---

## 一、为什么三件事必须一批做

阶段清算把「菜单」与「设置界面」列成两项，交接文档第四节第 17 条又单独记了
「`InputContext::Menu` 运行期是死路径」。这三者是**同一个决定面**：

- 没有上下文切换，`InputContext::Menu` 那张 11 条的默认表运行期永远查不到，
  菜单里按方向键走的仍然是 `Gameplay` 那份映射；
- 没有菜单，设置界面没有入口（阶段清算原话：「设置界面必须挂在菜单下，菜单是
  它的前置」）；
- 设置界面又是第 18 条（空格被 `Interact` 从 `Confirm` 手里拿走）唯一的出口。

拆开做必然返工——第一批要造一个假的上下文切换，第二批再推翻它。

---

## 二、动手前核实过的既有事实（基线 `03555cd`，实现者仍要自己复核）

| 事实 | 证据 |
|---|---|
| `GameKey::Menu` 变体已定义 | `crates/ll-platform/src/input.rs:47` |
| 已进 `ALL_KEYS` 定长数组 | `crates/ll-platform/src/input.rs:126` |
| 已有 i18n 显示名键 | `crates/ll-platform/src/input.rs:203` |
| 已进穷尽排序断言 | `crates/ll-platform/src/input.rs:697` |
| 默认键位是 **Tab**，且只在 `Gameplay` 上下文 | `crates/ll-platform/src/keybind.rs:393` |
| `ll-game` 里唯一提到它的是一句注释 | `crates/ll-game/src/app.rs:550` |
| `InputContext::Menu` 默认表（11 条）已存在 | `crates/ll-platform/src/keybind.rs:432`（`DEFAULT_MENU_BINDINGS`） |
| 该表已被 `load_or_default` 的合并覆盖并有测试 | `crates/ll-platform/src/config.rs:941`/`:947` |
| `UiModeStack` 已落地，`current_context()` **全仓库零调用者** | `crates/ll-ui/src/widget/ui_mode.rs`；`grep -rn current_context crates/` 只命中该文件自身 |
| `window.rs` 两处生产 resolve 硬编码 `Gameplay`（第三处在 `#[cfg(test)]`） | `crates/ll-platform/src/window.rs:260`（键盘）、`:280`（滚轮）、`:407`（测试） |
| `AppHandler::on_frame` 当前签名是 `&InputState` | `crates/ll-platform/src/window.rs:173`，7 处实现（1 生产 + 6 验收 demo） |
| `GameConfig` = bindings + display + language + unbound_actions + new_game | `crates/ll-platform/src/config.rs:99` |
| `config::save` 已存在，写出的是**普通 JSON**（`serde_json::to_string_pretty`） | `crates/ll-platform/src/config.rs:404` |
| `KeyBindings::try_bind` 冲突即拒绝，不静默覆盖 | `crates/ll-platform/src/keybind.rs:562`；模块文档 `:26` 写明这条约束 |
| `KeyBindings::bindings()` 已为设置界面预留只读列举 | `crates/ll-platform/src/keybind.rs:640` |
| `ALL_KEYS` 是私有常量，外部拿不到「全部动作」列表 | `crates/ll-platform/src/input.rs:118` |
| `GameKey::Screenshot` 在 `ll-game` 零消费点，只有 5 个验收 demo 有 | `grep -rn "GameKey::Screenshot" crates/` |
| `ll-render` 已经把 `image` 列为**正式依赖**（不是 dev） | `crates/ll-render/Cargo.toml:18` |
| `RenderTarget::read_pixels` 已存在 | `crates/ll-render/src/target.rs:446` |
| `ll-platform` **没有**重新导出 `winit::keyboard::KeyCode` | `crates/ll-platform/src/window.rs:44`-`45` 只导出了 `PhysicalSize`/`Window` |
| `Catalog` 内部是 `HashMap<String, FluentBundle>`，无「列出已装载语言」的入口 | `crates/ll-i18n/src/lib.rs:67`/`:124` |
| `Demo::advance` 每帧无条件跑 `advance_ai` + `player_command` | `crates/ll-game/src/app.rs:548` 起 |
| `run_game` 把 `config.bindings` **移动**进 `WindowConfig` | `crates/ll-game/src/lib.rs:436` |

---

## 三、设计裁定

### D1：`UiModeStack` 由 `ll_game::app::Demo` 持有，平台层每帧向它询问上下文

`ui_mode.rs` 模块文档已经把原始设计意图写死了，本批次照做、不另起一套：

> 栈是 UI 层（`ll-ui`）自己维护的一个 `Vec<UiMode>`，不是给 `InputContext` 本身
> 加状态……`InputContext` 是 `KeyBindings` 冲突检测的判重维度，一个**无状态的
> 分类标签**，`KeyBindings::resolve` 是纯函数，不关心「之前发生过什么」。

于是「谁持有」这个问题只剩一个答案：**唯一同时认识 `ll-ui`（栈的类型）与
`ll-platform`（窗口事件循环）的那一层**——`ll_game::app::Demo`。`ll-platform`
物理上依赖不到 `ll-ui`（依赖方向 `ll-platform ← … ← ll-ui`），栈不可能住在
平台层；`ll-ui` 拿不到窗口事件循环，也驱动不了它。

**真相源方向：栈是真相源，平台层是询问方。** 平台层新增一个带默认实现的
trait 方法：

```rust
fn input_context(&self) -> InputContext { InputContext::Gameplay }
```

`window.rs` 的两处生产 resolve 改成调用它。默认实现让六个验收 demo 一行不用改
（它们全都没有模态 UI，恒为 `Gameplay`，与改动前逐位等价）。

**被否决的方案**：把 `UiModeStack` 下沉到 `ll-platform`。否决理由就是上面引用的
设计文档原文——那会让 `KeyBindings` 开始关心导航历史。

### D2：`AppHandler::on_frame` 的 `input` 改成 `&mut InputState`

`UiModeStack::push`/`pop` 要求 `&mut InputState`（每次上下文切换必须
`InputState::clear()`，设计文档 2.3 节的硬结论：**这是第三种「隐式全键松开」
边界**，与失焦同一个函数）。而 `on_frame` 当前只给 `&InputState`。

`ui_mode.rs` 模块文档自己写明了改这个签名的正确时机：

> 那需要 `ll_platform::window::AppHandler::on_frame` 能把 `&mut InputState`
> 交给上层（当前签名是 `&InputState`）……改动它的正确时机是下一批真正有内容要
> push 到菜单里的时候。

本批次就是那一刻。改动是机械的：7 处 `impl` 的参数类型改一个字，函数体全部只读，
不需要任何其他调整。

### D3：绑定表改完怎么回到平台层——`take_rebound_keys`

`run_game` 把 `config.bindings` **移动**进 `WindowConfig`，事件循环内部
（`App::config.bindings`）才是真正被 `resolve` 查的那一份。设置界面改的是
`Demo` 自己那份草稿，改完必须能换掉平台层那一份，否则「改了键位不生效」。

新增第二个带默认实现的 trait 方法：

```rust
fn take_rebound_keys(&mut self) -> Option<KeyBindings> { None }
```

平台层在 `on_frame` 之后调用一次；返回 `Some` 就整表替换。**取走语义（take）
而不是借出**：借出会让平台层每帧都拷一份表；取走则只在真的改过的那一帧发生
一次，其余帧是一次 `Option` 判空。

**被否决的方案**：把 `&mut KeyBindings` 塞进 `on_frame` 参数。否决理由：会让
每个验收 demo 的签名都多一个用不到的参数，且把「谁能改绑定表」这件事从一个
显式的、可搜索的方法名，变成一个到处都能碰到的可变引用。

### D4：重绑定需要原始物理键——`InputState::last_physical_key`

这是本批次遇到的、设计文档 2.2 节**显式留给 P7 的开放问题**：

> 设置界面可能需要文本输入（改名字/改端口号），那才是一种真正需要不同物理键
> 映射的场景……但那也是 P7 才需要面对的问题，现在没有实现，分不出正确的边界。

现在面对了。**重绑定的形状是「按下你想绑的那个键」，而这个键按定义不在任何
绑定表里**——`resolve` 返回 `None`，事件循环当场 `return`，上层永远看不到它。
不解决这一点，重绑定物理上不可能。

裁定：**不新开 `InputContext` 变体，改为在 `InputState` 上多存一个「本帧按下的
物理键」**：

```rust
/// 本帧按下的物理键原样一份——重绑定唯一的输入来源。
last_physical_key: Option<KeyCode>,
```

- `window.rs` 在 `ElementState::Pressed` 时**无条件**记一次，与 `resolve` 成功
  与否无关（`resolve` 失败的那些键正是重绑定最需要的）；
- `end_frame()` 清空（与 `just_pressed` 同一帧生命周期）；
- `clear()` 一并清空（失焦/上下文切换语义一致）。

**为什么不做成新的 `InputContext::TextInput`**：那要求 `KeyBindings` 为「全部
物理键都要能穿透」这件事长出一个特例，而这条需求与「查表」根本不是同一件事
——它要的恰恰是**不查表**。多加一个上下文变体会让 `resolve` 在那个上下文下
永远返回 `None`，是一个纯粹的死分支。

配套：`ll-platform` 的 `keybind` 模块 `pub use winit::keyboard::KeyCode`——
设置界面要构造 `KeyBinding { key: KeyCode, .. }`，就必须能命名这个类型；让
`ll-game` 自己再声明一份 `winit` 依赖会引入版本漂移风险（与 `ll-render` 重新
导出 `wgpu`、`ll-ui` 复用它同一个理由）。

### D5：重绑=**追加**，解绑=**清空该动作在该上下文下的全部键**

冲突处理是硬要求（`keybind.rs:26` 模块文档已经写死「注册时拒绝」）。本批次
**直接复用 `KeyBindings::try_bind`**，不在 UI 层再抄一遍判重逻辑：

1. 克隆一份草稿表；
2. `draft.try_bind(KeyBinding { key: 刚按下的物理键, modifiers: NONE, context, action })`；
3. `Err(conflict)` → **不提交**，屏幕上显示一行「这个键已经绑给了 X」（i18n，
   带 `$action` 参数），玩家的表一个字节都没变；
4. `Ok(())` → 提交草稿，同时 `take_rebound_keys` 下一帧把它交给平台层。

**为什么追加而不是替换**：默认表里 `Up` 同时绑 `ArrowUp` 与 `KeyW`（方向键与
WASD 双绑是刻意的，见 `DEFAULT_BINDINGS` 文档）。「替换」语义会让玩家给 `Up`
加一个键的同时**静默丢掉另外两个**——那正是本批次要防的「不要静默覆盖」的
另一种形态。追加不丢任何东西；要丢就必须显式解绑。

**解绑**（清空某个动作在当前上下文下的全部绑定）是必需的，不是附赠：第 18 条
要求「玩家能把空格改回 `Confirm`」，而空格此刻绑着 `Interact`——不先解绑
`Interact`，`try_bind` 必然（且应该）拒绝。解绑同时把该动作写进
`GameConfig::unbound_actions`，否则下次加载 `fill_missing_defaults` 会把默认键
又塞回来（该字段文档写明了这正是它存在的理由）。

**捕获模式下两个特例键**（不经 `GameKey`，直接看原始物理键）：

| 原始键 | 含义 | 代价 |
|---|---|---|
| `Escape` | 取消这次捕获 | 无法把 Esc 绑给任何动作 |
| `Backspace` | 解绑当前这一行的动作 | 无法把 Backspace 绑给任何动作 |

这是绝大多数游戏的既有约定，代价（两个键不可绑）明确、可逆、且写进本文档。

### D6：显式保存——**并且如实承认它会抹掉玩家手写的 JSON5 注释**

所有者已裁定「不回写，等设置界面落地后由界面显式保存」。本批次落地那个显式
动作：设置界面的「保存」行按确认 → 调 `ll_platform::config::save`。

**做不到的事，如实说**：`config::save` 走的是 `serde_json::to_string_pretty`
（`config.rs:404`，模块文档「格式：JSON5，读写不对称」一节写明了原因：`json5`
crate 只提供解析、不提供序列化）。**保存会把玩家手写的全部注释与尾逗号抹掉。**

本批次**不假装**解决它。三条为什么不在本批次解决：

1. 保留注释需要一个**保序、保注释**的 JSON5 编辑器（读进 CST、只改叶子值、
   原样写回）。工作区里没有这样的依赖，引入一个是独立的一批。
2. 手写注释与「界面改了值」本来就存在无法自动调和的冲突（注释写的是旧值的
   理由）。
3. 现状（不保存）比「保存但可能损坏」更糟：玩家在界面里改的键位**一次都存不
   下来**。

**降低代价的两件事，本批次做**：
- 保存只在玩家显式按下「保存」时发生，绝不自动触发；
- 保存前后各记一条 `tracing::warn`，明说注释会丢。

**留给所有者的裁定**：要不要引入保注释的 JSON5 写出（例如 `jsonc`/CST 方案）。
写进本文档第七节。

### D7：三项显示/语言设置各自何时生效

| 项 | 生效时机 | 为什么 |
|---|---|---|
| 语言 | **当场** | `Catalog::resolve(language, key)` 每帧现查，改 `Demo::language` 下一帧就是新语言 |
| 缩放滤波 | **当场** | `GpuResources::blit_filter` 是一个普通字段，赋值即生效（`acquire_and_blit` 每帧读它） |
| 垂直同步 | **重启后** | `vsync` 只在 `GpuContext::new` 时决定 `PresentMode`（`ll-render/src/gpu.rs:96`），`ll-render` 没有运行期改呈现模式的入口 |

vsync 那一行在界面上带一句「重启后生效」的提示（i18n）。**不为它新开一条
「重建 surface」的路径**：那要动 `ll-render` 的 `GpuContext` 公开面，是一次独立
的、有真实风险（重建 surface 期间的帧丢失处理）的改动，不该夹带在 UI 批次里。

语言列表需要 `Catalog` 能列出已装载的语言，新增：

```rust
pub fn languages(&self) -> Vec<String>   // 必须排序！
```

内部是 `HashMap`，**直接遍历违反 C5**（列表顺序决定玩家按左右键切到哪一个，
是不折不扣的「迭代顺序参与逻辑判断」）。实现里必须 `sort()`。

### D8：新屏放在 `crates/ll-ui/src/screen/`，不动 `hud/`

`wt-playfeel` 正在改 `hud/`。新开 `screen` 模块，只**复用** `hud::build_panel`
与 `hud::PanelContent`（`pub(crate)`，同 crate 内可见），不复制一套面板布局
算法，也不改 `hud/` 下任何既有文件。

`screen` 与 `hud` 的职责分界，写进模块文档：**`hud` 是画在世界之上的观测层，
`screen` 是盖住世界的模态屏**（世界不在它底下继续跑，见 D9）。

### D9：菜单打开时**整条游戏推进跳过**

先核实现状：`Demo::advance` 每帧无条件调 `advance_ai` + `player_command`。
`advance_ai` 只结算「排在玩家之前」的实体，排到玩家就停；玩家不提交意图，
`try_player_intent` 返回 `NotYet`/不推进时钟。所以**回合制「玩家不动世界不走」
这条在现状下确实成立**（本批次会补一条测试钉死它）。

但光靠这一条不够：菜单开着时方向键会被 `player_command` 读成移动。因此
`advance` 在 `ui_modes` 非空时**整段早退**——不跑流式维护、不跑 AI、不跑玩家
指令，只跑菜单屏自己的输入处理。这是最保守也最容易解释的语义：**这块屏盖住的
时候，世界一个字节都不动。**

配套：`on_frame` 里「Esc 退出游戏」那条要加上 `ui_modes.is_empty()` 判断，
否则玩家想关菜单会直接退出整局（与 `player_command` 第 ④ 步同一个已知陷阱）。

另：`GameKey::Menu` 只在 `PlayerMenu::Closed`（背包/制作/交互列表都没开）时才
开菜单——两块模态 UI 叠在一起没有任何人要求过，且会引出「Esc 关哪一层」的
新裁定。

### D10：`GameKey::Screenshot` 接进 `ll-render` 的新公开入口

五个验收 demo 各自抄了一份 `save_baseline_png`。本批次**不重构那五份**（不在
范围内），但生产入口不再抄第六份：在 `ll-render` 新增

```rust
pub mod screenshot;
pub fn save_png(gpu: &GpuContext, target: &RenderTarget, path: &Path) -> Result<…>
```

`ll-render` 已经把 `image` 列为正式依赖（`Cargo.toml:18`），`ll-game` 因此**不
需要**把 `image` 从 dev-dependencies 提升为正式依赖。

存到 `<数据目录>/screenshots/screenshot-<帧号>.png`，**不覆盖**任何既有文件。
注意：`crates/ll-game/tests/visual/` 下那三张基准是 `examples/*_preview.rs`
产出的，与 F2 无关——本体的 F2 是玩家功能，不是冻结基准的入口（`GameKey::Screenshot`
的文档写的是验收 demo 那一侧的语义，本批次在 `ll-game` 侧的接线要在文档里
点明这条区别）。

---

## 四、落地形状

### 新增文件

| 文件 | 内容 | 预估行数 |
|---|---|---|
| `crates/ll-ui/src/screen/mod.rs` | `ScreenFrame`、模块文档、与 `hud` 的分界 | ~120 |
| `crates/ll-ui/src/screen/menu.rs` | 游戏内菜单的行产出（纯函数） | ~200 |
| `crates/ll-ui/src/screen/settings.rs` | 设置界面的行产出（纯函数） | ~280 |
| `crates/ll-ui/src/screen/render.rs` | 两块屏的 GPU 提交（镜像 `hud::render`） | ~180 |
| `crates/ll-game/src/menu_screen.rs` | 状态机：焦点、捕获模式、应用/保存 | ~500 |
| `crates/ll-render/src/screenshot.rs` | F2 存图 | ~80 |

### 改动的既有文件

| 文件 | 改什么 |
|---|---|
| `crates/ll-platform/src/window.rs` | 两处 resolve 走 `handler.input_context()`；`on_frame` 收 `&mut InputState`；帧末 `take_rebound_keys` |
| `crates/ll-platform/src/input.rs` | `last_physical_key`；`GameKey::all()` |
| `crates/ll-platform/src/keybind.rs` | `pub use KeyCode`；`unbind_action` |
| `crates/ll-i18n/src/lib.rs` | `Catalog::languages()`（排序，C5） |
| `crates/ll-game/src/app.rs` | `Demo` 三个新字段、菜单接线、渲染接线、F2 |
| `crates/ll-game/src/lib.rs` | 把 `GameConfig` 与配置路径交给 `Demo` |
| 六个验收 demo 的 `on_frame` | 参数类型改一个字 |
| `assets/locales/{en,zh-CN}.ftl` | 全部新文案，**两个语言都加** |

---

## 五、必须新增的测试（每条都要按 ADR 0018 用「故意改坏」验证真的会红）

| # | 断言 | 落点 |
|---|---|---|
| T1 | 空栈时平台层解析用 `Gameplay`，压栈后用 `Menu` | `ll-platform` `window.rs` |
| T2 | `last_physical_key` 记下了没有任何绑定的物理键 | `ll-platform` `input.rs` |
| T3 | `end_frame` 后 `last_physical_key` 归空 | 同上 |
| T4 | 按菜单键后 `UiModeStack` 深度为 1、上下文变 `Menu` | `ll-game` `app.rs` |
| T5 | 菜单开着时按方向键，**世界时钟与玩家坐标都不变** | `ll-game` `app.rs` |
| T6 | 菜单开着按 Esc 关掉菜单，**不退出游戏** | `ll-game` `app.rs` |
| T7 | 菜单里选「设置」后进入设置屏 | `ll-game` `menu_screen.rs` |
| T8 | 把已被别的动作占着的键绑过来 → **被拒绝且原表不变** | `ll-game` `menu_screen.rs` |
| T9 | **解绑 `Interact` 后把空格绑给 `Confirm` 成功**（第 18 条的验收） | `ll-game` `menu_screen.rs` |
| T10 | 解绑会把动作写进 `unbound_actions` | `ll-game` `menu_screen.rs` |
| T11 | 切换语言后同一个键解析出另一种语言的文本 | `ll-game` `menu_screen.rs` |
| T12 | 保存写出的文件能被 `load_or_default` 读回且键位一致 | `ll-game` `menu_screen.rs` |
| T13 | `Catalog::languages()` 恒返回排序结果（C5） | `ll-i18n` |
| T14 | 菜单退出项返回 `Exit` | `ll-game` |
| T15 | 每个 `GameKey` 在设置界面都有一行（`GameKey::all()` 全覆盖） | `ll-ui` `screen/settings.rs` |

---

## 六、黄金基准

本批次**不碰任何世界状态语义**——改的全部是输入层、UI 层与配置层，三者都在
`WorldState` 之外（`config.rs` 模块文档「配置不是世界状态」一节的依赖方向就是
结构性保证）。

预期：`crates/ll-world/tests/determinism.rs` 与 `crates/ll-sim/tests/replay.rs`
两条**逐位不变**。真变了就说明改动漏进了世界状态，**走四步重冻并在报告里
解释为什么**——不要直接改常数。

`crates/ll-game/tests/visual/` 三张基准同样预期不变（它们由
`examples/*_preview.rs` 产出，不经 HUD/screen 路径）。

---

## 七、范围边界（不要越界）

- **不碰** `crates/ll-ui/src/hud/` 下任何既有文件（`wt-playfeel` 在改）。
- **不碰** `ll-sim`/`ll-world` 的任何结算语义。
- **不重构**五个验收 demo 里各自那份 `save_baseline_png`。
- **不做** 保注释的 JSON5 写出（D6，留给所有者裁定）。
- **不做** 运行期切换 vsync（D7）。
- **不给** `InputContext` 加第三个变体。
- **不做** 鼠标点击菜单项（焦点导航是硬要求，鼠标不是）。

## 八、留给所有者的裁定

1. 设置界面保存会抹掉 `config.json5` 里玩家手写的注释（D6）。要不要引入保注释
   的 JSON5 写出？
2. vsync 改动需要重启才生效（D7）。要不要给 `ll-render` 开一条运行期重配
   surface 的路径？
3. 重绑定采用「追加」语义、解绑是「清空该动作在该上下文下的全部键」（D5）。
   要不要改成更常见的「一个动作一个主键 + 一个副键」的固定槽位模型？
4. 捕获模式占用了 `Escape`/`Backspace` 两个物理键，它们因此不可绑（D5）。
5. 设置界面只列 `Gameplay` 上下文的 20 个动作，`Menu` 上下文那 11 条不可改。

---

## 九、落地之后：与本计划的偏差（写于四段全部提交之后）

计划是开工前写的，落地过程中有五处与它不符。**逐条如实记录**，不回头
改前面几节假装计划一开始就是这样。

### 偏差 1：`ll-ui` 的新屏只有一个模块，不是「菜单屏 + 设置屏」两套

计划第四节列了 `screen/menu.rs` 与 `screen/settings.rs` 两个文件。实际
落地发现两块屏在画法上完全同构（居中面板 + 标题 + 一列行 + 光标标记 +
提示行），差别全在**行里写什么字**，而那是 `ll-game` 的排版职责。写成
两份等于把同一个算法抄两遍（ADR 0021 的反面）。最终形状：
`screen/mod.rs`（数据 + 排版）+ `screen/render.rs`（GPU 提交）。

### 偏差 2：`ll-game` 侧拆成了三个文件，不是一个

计划里 `menu_screen.rs` 预估 500 行，实际产品代码 + 断言合起来越过了
800 行的上限。最终拆成：

- `crates/ll-game/src/menu_screen.rs`（551 行）——状态机：读输入、改
  配置、切状态；
- `crates/ll-game/src/settings_view.rs`（225 行）——排版：只读，纯函数；
- `crates/ll-game/tests/menu_and_settings.rs`（456 行）——行为断言。

搬进 `tests/` 有一个计划里没想到的好处：那里只摸得到 `pub` 的东西，
断言因此**必须走玩家真正走的那条公开路径**（`update_settings`），而不
是抄近路去调私有的捕获处理函数。原先直接调 `update_capture` 的四条
断言随之改写。

### 偏差 3：C5 那条断言第一版**咬不住**，是反例验证抓出来的

计划 T13 只写了「`Catalog::languages()` 恒返回排序结果」。第一版铺两种
语言，去掉排序之后六次里只红两次——哈希桶序有约一半概率恰好就是字典
序。改成铺八种语言（碰巧有序的概率约 1/8!）后重测六次全红。**这正是
C5 一节警告的那种「测试照样全绿」的形状，只是这次发生在守护它的测试
自己身上**，已写进该测试的注释。

### 偏差 4：`self.screen.is_none()` 那道退出闸门的验证换了一条断言

计划 T6 是「菜单开着按 Esc 关掉菜单，不退出游戏」。反例验证时删掉闸门
**它没有变红**——追查发现关屏顺带 `InputState::clear()`
（`UiModeStack::pop` 的语义），取消键被吃掉，少一道闸门也看不出问题。
于是补了一条更紧的：**设置界面里按取消退回菜单屏而不是退出整局**——
设置界面按取消不关屏，取消键的「刚按下」原封不动留到退出判定，闸门在
那条路径上是真的在挡事。补完重测：删掉闸门当场变红。

### 偏差 5：本批次**没有**新建任何 `examples/` 验收 demo

主干新增了 `scripts/ci/run_acceptance_demos.sh`（带「每个 example
target 都必须显式登记」的完整性检查）。本批次新增的 example target 数
为 **0**，因此那道门禁不会被本批次触发。

理由不只是省事：菜单/设置是 UI，验收 demo 天然要开窗、要等输入，按
SKIP_LIST 的判据只能进 SKIP_LIST，对 CI 的价值近乎为零；而 UI 的可测
部分（布局换算、焦点导航顺序、键位冲突判定、上下文切换、配置保存往返）
全部可以用普通 `#[test]` 覆盖——这既绕开 ADR 0025，也不给门禁添一条要
长期维护的登记。规格 §15「每阶段必须交付 `examples/` 验收 demo」要不要
改，是所有者的裁定（交接文档第四节第 25 条），不是本批次的。
