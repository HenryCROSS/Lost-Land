# 批次 9：存档系统（多槽位 / 命名 / 手动 / 自动 / 肉鸽）+ 暂停菜单补两项 + 贴图日志刷屏

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，全文 0 次出现「反例」。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**基线**：`40359a9`　**分支**：`wt-savesystem`　**工作树**：`../wt-savesystem`　**日期**：2026-08-29

三件事一批做，**分三个提交**（A → B → C）。

- **任务 A**（先做）：贴图回退链每帧每实体刷 `ERROR`，把日志文件淹掉。所有者实机撞到。
- **任务 B**：暂停菜单补「保存」与「返回主菜单」两项。
- **任务 C**：存档系统——多槽位、命名、列表、手动存档、自动存档、肉鸽模式、死亡接线。

---

## 〇、开工前自己 grep 复核过的数字（不信任何口头转述）

| 事实 | 复核结果（基线 `40359a9`） |
|---|---|
| `EXPECTED_WORLD_DIGEST` | `10_180_278_885_427_934_050`，`crates/ll-world/tests/determinism.rs:303` |
| `EXPECTED_REPLAY_DIGEST` | **`4_180_595_409_733_934_027`**，`crates/ll-sim/tests/replay.rs:948` |
| `CONTENT_HASH_ALGORITHM_VERSION` | `27`，`crates/ll-mod/src/content_hash.rs:805` |
| `CURRENT_SCHEMA_VERSION` | `2`，`crates/ll-content/src/save_file.rs:94` |
| 刷屏的那一行 | `crates/ll-game/src/app.rs:305`（`GpuResources::lookup` 里的 `tracing::error!`） |
| `superseded_by` 探测点 | `crates/ll-game/src/app.rs:1927`（`push_surface_draw` 里 `.any(|key| resources.lookup(key).is_some())`） |
| `lookup_first` | `crates/ll-game/src/app.rs:322` |
| 存档只有一份 | `SAVE_FILE_NAME`（`crates/ll-game/src/lib.rs:57`）+ `GamePaths::save`（`:96`）是单个文件路径 |
| `save_game` 的 `mode` 参数 | `crates/ll-game/src/save.rs:101`，**全部 7 处调用点硬编码 `SaveMode::Permadeath`** |
| `SaveMode` 已存在 | `crates/ll-content/src/mode.rs`，含 `downgrade()` 单向不可逆 + 私有标记字段 |
| `SaveHeader.mode` | `crates/ll-content/src/header.rs:174`，**是 `pub` 字段**（本批收紧） |
| `app.rs` 行数 | 3773（既有违规，本批新代码全部进新模块） |
| 文本输入 UI | **全仓库没有**。`InputState::last_physical_key()`（`ll-platform/src/input.rs:379`）是设置屏捕获模式用的裸键通道，本批复用它 |
| 死亡判定 | 今天**没有**。实体死亡 = `world.actors.despawn(target)`（`ll-sim/src/apply.rs:128`），因此「玩家死了」= `actors.get(player).is_none()` |
| `TICKS_PER_HOUR` | `36_000`（`ll-core/src/time.rs:49`）；`TICKS_PER_DAY` = `864_000` |
| 批次 8 的三处接缝 | `NewGameDraft::world_already_exists`（`chargen.rs:398`）、`world::apply_character_choice`、`world::move_player_to` |

**交接文档 `2026-08-28-session-handoff.md` 第〇节那张表的 `EXPECTED_REPLAY_DIGEST`
（`6_885_882_507_408_978_859`）已经过期**——批次 8 的性别字段动过它。这是那条「以代码
为准」纪律第二次真的派上用场。

基线测试数（本会话自己跑 `bash scripts/ci/run_tests.sh`）：**113 个测试二进制、
2647 passed / 0 failed**，exit 0。

---

## 一、任务 A：回退链把日志刷爆了

### 1.1 根因复核（所有者的定位成立，但落点比他说的多一处）

现象是每帧、每个 NPC、每个候选键各一行 `ERROR`，键名形如
`lostland_human_lostland_farmer`（`<种族>_<职业>`）与
`lostland_human_lostland_fisher_female`（`<种族>_<职业>_<性别>`）。

`GpuResources::lookup_first`（`app.rs:322`）**本身已经是对的**——它的文档明确写着
「前面的候选走不打日志的探测，只有最后一个候选（兜底记号）走 `lookup`」。所以身子层
（候选 = 两个合成键 → 种族键 → `NPC_SPRITE` 兜底）不会为合成键打日志。

真正的刷屏来自**另外两处**：

1. **`push_surface_draw` 的压制判定**（`app.rs:1927`）：

   ```rust
   if draw.superseded_by.iter().any(|key| resources.lookup(key).is_some())
   ```

   `superseded_by` 装的正是那两个合成键，而 `lookup` 是**会打 `ERROR` 的那个访问器**。
   今天零张合成图 ⇒ 两次必然未命中 ⇒ **每个 NPC 每帧两行 `ERROR`**。日志里那两个键名
   逐字对得上。

   这是一次**探测**（「这个键在不在？」），却用了「我需要它，缺了就是缺陷」的访问器。

2. **职业挂件层**（`surface_draw.rs:381`）：`preferred_keys` 只有一个职业键、
   `fallback_key: None`。于是 `keys()` 只产出**一个**名字，`lookup_first` 的
   「最后一个候选走 `lookup`」规则直接命中它 ⇒ 又是一行 `ERROR`。而
   `SurfaceDraw::fallback_key` 的字段文档明写「没有为某个职业准备挂件贴图是**正常
   状态**」——正常状态不该是 `ERROR`。

**结论**：`lookup_first` 的「最后一个候选一定该打 error」这条规则，在
`fallback_key` 为 `None`（整条指令本来就是可选的）时是错的；而 `superseded_by`
根本就不该走 `lookup`。两处是同一个根因的两种表现：**把「探测」和「取用」当成了
同一件事**。

### 1.2 所有者的追加裁定

> 「这一类改成 Warning」

**这是两条独立要求，都要满足**（协调者原话）：级别从 `ERROR` 降到 `WARN`；**刷屏
照旧要治**——`WARN` 刷屏一样把日志文件淹掉。级别降级不能替代去重。

### 1.3 选定的日志策略与理由

**判据：回退链还有候选可试 ⇒ 一个字都不打；全部候选落空 ⇒ 打一条 `WARN`，同一组
候选在整个进程生命期内只打一次。**

- **中间未命中：完全不打。** 连 `trace!` 都不打——回退链的中间步骤未命中是它的**正常
  工作方式**，不是任何意义上的事件。这一条由**类型**保证而不是由纪律保证：探测走
  新增的 `GpuResources::contains`（`-> bool`，函数体里没有任何 `tracing` 调用），
  能打日志的路径根本不经过它。
- **全部落空：`WARN`，按候选链去重。** 去重容器是 `BTreeSet<String>`（不是 `HashSet`
  ——约束 C5 禁止逻辑依赖哈希容器迭代顺序；这里虽然只做存在性判定，但保持仓库既有
  习惯，且它同时给了确定的诊断输出顺序）。键是**整条候选链拼成的字符串**，不是单个
  键：玩家真正关心的是「这条指令一个候选都没命中」，而不是「第 2 个候选没命中」。

**为什么去重（第一次出现时打一条）而不是限流（每 N 秒一条）**：

1. **成本上界是内容规模，不是时间。** 去重之后日志总行数 ≤ 不同候选链的条数
   （13 个职业 + 9 个种族这个量级），与帧率、NPC 数量、游玩时长全部无关。限流的
   上界是「时长 ÷ 周期」，玩一小时仍然会有几十上百行。
2. **信息量在第一次就已经全部给出。** 第 4000 次「同一个键还是查不到」不携带任何
   新信息。
3. **它保住了这条诊断线索的真正价值。** 本仓库踩过「五张 UI 贴图全部查不到、每帧
   静默退回纯色、**不打任何日志**」那个缺陷（`2026-08-27` 交接第二节）。在本方案下
   那个缺陷会在启动后立刻产出 5 行 `WARN` 然后安静——**响亮、有限、看得见**，正是
   当时缺的东西。**没有矫枉过正改成完全静默。**

**为什么是 `WARN` 而不是 `ERROR`（除所有者裁定外的独立理由）**：一条落空的候选链
可能是一层**本来就可选**的内容（`fallback_key: None`，例如没有挂件贴图的职业）。
把正常状态记成 `ERROR`，`ERROR` 这个级别就再也不能表示「真出事了」。

**为什么不做成「只在集合变化时打一次」**：那需要每帧维护一份「本帧缺了哪些」的集合
并与上一帧比较，等于每帧仍然要构造几十个字符串——把日志的开销换成了集合的开销，
而本方案在命中路径上**一个字符串都不构造**。

### 1.4 落地形状

新模块 `crates/ll-game/src/atlas_miss.rs`（`app.rs` 已 3773 行，新代码不进它）：

```rust
/// 一条绘制指令走完整条回退链之后的结局——**唯一**决定要不要留日志的地方。
pub enum ChainOutcome<'a> {
    /// 命中了第 `index` 个候选（0 最优先）。中间候选未命中是回退链的正常
    /// 工作方式，本变体**不携带任何要打的话**。
    Hit { index: usize, key: &'a str },
    /// 全部候选落空，且这组候选**第一次**落空——调用方应当打一条 WARN。
    MissedFirstTime { candidates: String },
    /// 全部候选落空，但这组候选之前已经报过——静默。
    MissedAgain,
}

/// 已经报告过的候选链，去重器。
pub struct MissLedger { seen: BTreeSet<String> }

impl MissLedger {
    /// 按优先级挨个**静默**探测，返回结局并在必要时登记。
    pub fn resolve<'a>(&mut self, keys: impl Iterator<Item = &'a str>,
                       exists: impl FnMut(&str) -> bool) -> ChainOutcome<'a>;
    /// 至今登记过多少条不同的候选链（测试与诊断用）。
    pub fn reported(&self) -> usize;
}
```

**「不打日志」这件事因此是结构性的，不是靠调用方自觉**：`tracing::warn!` 只能挂在
`MissedFirstTime` 这一个分支上，而 `Hit` 与 `MissedAgain` 在类型上根本没有可打的内容。
测试断言返回的这个枚举，等价于断言日志行为，**不需要装 `tracing-subscriber` 去抓
日志**（那是进程级全局状态，在并行测试下本身就是一处不确定性）。

`app.rs` 这一侧只改四处：

| 位置 | 改什么 |
|---|---|
| `GpuResources` | 新增字段 `miss_ledger: MissLedger`；新增方法 `contains(&self, name) -> bool`（静默探测） |
| `lookup`（`:299`） | `error!` → `warn!`。**保留**：它仍然是「就这一个名字、缺了就是缺陷」那些调用方（`push_player_marker` 的当前动画帧、HUD 单图）的正确访问器 |
| `lookup_first`（`:322`） | 整体改写：走 `miss_ledger.resolve(keys, |k| self.contains(k))`，按结局分支 |
| `push_surface_draw`（`:1927`） | 压制判定 `resources.lookup(key).is_some()` → `resources.contains(key)` |

五个 `examples/*/main.rs` 里同样那行 `tracing::error!(name, "图集条目缺失…")`
**一并降成 `warn!`**（所有者说的是「这一类」）。它们是各自独立的验收 demo，只有单个
名字、没有回退链，不需要去重器。

### 1.5 两条测试（按 ADR 0018 做反例验证）

| # | 断言 | 落点 |
|---|---|---|
| A1 | 回退链命中较后候选时，结局是 `Hit`，且账本一条都没登记（⇒ 不可能产生任何日志） | `atlas_miss.rs` 单元测试 |
| A2 | 全部候选落空时留下**恰好一条**记录：第一次 `MissedFirstTime`，同一组候选再来一次是 `MissedAgain`，账本恒为 1 | 同上 |

补两条守住 `app.rs` 那一侧的接线（否则 A1/A2 只证明了纯函数对、没证明它被用上）：

| # | 断言 | 落点 |
|---|---|---|
| A3 | `SurfaceDraw` 的压制判定与查图走的是同一套「静默探测」——给一条 `superseded_by` 全部查不到的指令，账本不因压制判定而增长 | `app.rs` 单元测试 |
| A4 | 职业挂件层（`fallback_key: None`、单候选）落空时只登记一次，连画 100 帧也只有一条 | 同上 |

反例：
- A1 → 把 `resolve` 改成「每个未命中的候选各登记一次」，A1 必须红。
- A2 → 把 `resolve` 的登记去掉（永远返回 `MissedAgain`），A2 必须红。
- A3 → 把压制判定改回 `lookup`（打日志的那个），A3 必须红。
- A4 → 把 `lookup_first` 改回「最后一个候选走 `lookup`」，A4 必须红。

---

## 二、任务 B：暂停菜单补「保存」与「返回主菜单」

### 2.1 菜单行改成现算，不再是静态数组

现状 `MENU_ITEM_IDS: [WidgetId; 3]` 是编译期数组。本批「保存」这一项**在肉鸽模式下
不出现**（所有者裁定：肉鸽只有自动保存），因此行数不再固定。

照 `settings_rows()` 的既有形状做（不发明第二种）：

```rust
pub enum MenuRow { Continue, Save, Settings, BackToTitle, Quit }
pub fn menu_rows(can_save_manually: bool) -> Vec<MenuRow>;
```

`can_save_manually` 由 `SaveMode` 推出（`FreeSave` 为真、`Permadeath` 为假）——
**判据只有一处**，UI 不自己 `match` 模式。

### 2.2 「返回主菜单」时怎么处理未保存进度：**先存一次，存成功才回**

所有者只说「需要一个返回主菜单选项」，没有裁定未保存进度怎么办。批次 6 报告第 10 节
第 3 条把这件事记为「独立的决定面」，本批解开它。

**选定：回主菜单前自动存一次当前槽位；写盘成功才真的回去，失败就留在菜单并报错。**

理由，按重要性排：

1. **绝不静默丢弃玩家进度**（任务书的硬要求）。
2. **它与「退出游戏」是同一条规则，不是第二条。** `on_exit` 今天就是无条件
   `save_on_exit()`。让「回主菜单」也存一次，玩家只需要记住一件事：**离开世界就会
   存**。两条路径给出两种结果才是真正会咬人的形状。
3. **肉鸽模式下这是唯一正确的做法。** 肉鸽没有手动存档入口，如果回主菜单不存，
   玩家上一次自动存档之后的进度就凭空消失了——而那正是肉鸽模式最不能接受的事
   （它的全部约束是「后果不可撤销」，不是「进度可以蒸发」）。
4. **「弹一个确认框」是一块本仓库今天没有的 UI。** `ScreenData` 是一块「标题 + 若干
   行 + 一句提示」的居中面板，没有「是/否」模态的概念。造一个是独立的一批。
5. **最容易反转。** 将来所有者要「问一句再回」，改动是在这条路径前面插一块屏，
   存档那一句原样保留。

写盘失败时**留在暂停菜单**并显示 `screen-menu-save-failed`——这一条比「回去了但没
存上」重要得多：玩家至少还站在世界里，可以再按一次。

### 2.3 「保存」这一项

- 当前槽位**已经有名字**（进世界那一刻就定了，见第三节）⇒ 直接存，显示已保存提示。
- 肉鸽模式下这一行**根本不在列表里**（不是置灰）——`ScreenData` 没有逐行禁用样式的
  概念（批次 6 第 4.2 节已经论证过），而这一项的缺席本身就是模式的可见后果。

---

## 三、任务 C：存档系统

### 3.1 现状勘查

| 事实 | 现状 |
|---|---|
| 路径 | `GamePaths::save = <数据目录>/save.llsave`，单个文件；`SAVE_FILE_NAME` 全仓库一个取值 |
| 「只有一份」的假定散在哪 | `Demo::save_path: PathBuf`、`Demo::save_exists: bool`、`update_title(has_save: bool)`（一个布尔而不是一张列表）、`load_or_new_game`、`save_on_exit` |
| 物理布局 | `4 字节长度前缀 + 头部 JSON + 压缩主体`；`load_from_header_only` 只读前两段，**不解压主体**——存档列表正是它的设计用途（`SaveHeader::world_seed` 字段文档原话） |
| 格式版本 | `CURRENT_SCHEMA_VERSION = 2`，迁移链在 `ll_content::migrations` |
| 模式 | `SaveMode` 已存在且已在头部，但 `save_game` 的 7 处调用点**全部硬编码 `Permadeath`** |

### 3.2 多槽位的形状

```
<数据目录>/
  save.llsave          ← 老的那一份，只读，用于一次性收编（见 3.6）
  saves/
    <槽位标识>.llsave
    <槽位标识>.llsave
```

新模块 `crates/ll-game/src/save_slot.rs`：

- `SlotId(String)`——**文件名主干**，由玩家输入的名字过滤而来（只保留
  `[A-Za-z0-9_-]`，其余换 `_`；空则退回 `save`）。碰撞时追加 `-2`、`-3`……
  在**创建那一刻**定死，此后这个槽位永远写同一个文件（再存一次就是覆盖，不是新建）。
- `SaveSlot { id, path, save_name, character_name, saved_at, mode }`——列表项，
  **只读头部**得来。
- `list_slots(dir) -> Vec<SaveSlot>`——扫目录、逐个 `load_from_header_only`，
  读不动的条目**跳过并记一条 `warn`**（一份坏档不该让整个列表打不开），
  按 `saved_at` 倒序（最近的在最上面），同刻按 `id` 定序（不依赖目录遍历顺序）。

**`GamePaths` 的改动**：`save: PathBuf` → 保留但改名 `legacy_save`（它现在只有收编
一个用途），新增 `saves_dir: PathBuf`。

### 3.3 存档名存哪：头部新增 `save_name`

`SaveHeader` 已有 `character_name`（角色名）与 `current_region`，**没有世界/存档名**。
新增：

```rust
#[serde(default)]
pub save_name: String,
```

`#[serde(default)]` ⇒ 老存档缺这个键时读回 `""`，展示时退回文件名主干。

**这不需要 schema 版本升级**，理由与 `terrain_shape` 那次逐字相同（`header.rs:168`
的字段文档已经论证过）：**存档主体的字节布局一个字节都没动**，动的只是头部 JSON 多了
一个可缺席的键。真正需要迁移的是**文件位置**，见 3.6。

### 3.4 命名输入 UI：做到多简单

**先确认：仓库今天没有任何文本输入控件。** 唯一的裸键通道是
`InputState::last_physical_key() -> Option<KeyCode>`（设置屏捕获模式在用）。

选定的最小形状——新模块 `crates/ll-game/src/save_name.rs`：

- 一行输入，**只接受 `A-Z`/`a-z`/`0-9`/空格/`-`/`_`**（从 `KeyCode::KeyA..KeyZ`、
  `Digit0..9`、`Space`、`Minus` 直接映射）；
- `Backspace` 删一个字符；
- `Confirm`/`Enter` 确认，`Cancel`/`Escape` 取消；
- 长度上限 24 字符（够识别、不撑破那块面板）；
- 光标用一个尾随的 `_` 表示，不做插入点移动、不做选区、不做剪贴板。

**做不到、且如实说明的**：没有 IME，**打不了中文**。做 IME 要接 winit 的
`Ime` 事件、维护预编辑串、还要一套能画任意字形的文本栈——那是独立的一整批。存档名
只用来在列表里认出「哪一份是哪一份」，拉丁字母加数字够用；玩家什么都不打就直接确认
时，退回一个默认名（`screen-savename-default` 的译文 + 一个序号）。

**大写怎么办**：不读修饰键，全部按小写记（`Shift` 组合在 `last_physical_key` 这一层
拿不到）。这是本批为了不改平台层而做的最保守取舍，单列在第八节。

### 3.5 五块接线

| 事项 | 落点 |
|---|---|
| **列表** | 新 `ScreenState::SaveList { cursor }` + 新模块 `save_list.rs`。首页「读取存档」不再直接读那一份，而是进这块屏；每行 = `名字 · 可读时间 · 模式`。时间戳走本模块自己的 `civil_from_days`（约 20 行纯算术，**不引入 `chrono`**——`SaveHeader::saved_at` 的字段文档已经明确否决过为此新增重依赖） |
| **命名** | 新 `ScreenState::SaveNaming { .. }`。插在**选出生地确认之后、进世界之前**：那一刻世界已经建好、角色已经选好，取名是这条链的最后一步，之后每次存档都写同一个槽位，不再问 |
| **手动存档** | 暂停菜单那一项（任务 B）。肉鸽模式下不出现 |
| **自动存档** | 见 3.7 |
| **死亡** | 见 3.8 |

### 3.6 旧存档迁移：**路径迁移，不是格式迁移**

所有者手上有一份 `save.llsave`。本批**不动存档主体的字节布局、不升 schema 版本**
（头部只多了一个 `#[serde(default)]` 的键），所以那份档本身照常读得开。真正变的是
「去哪儿找它」。

**收编（adopt）**：启动时若 `<数据目录>/save.llsave` 存在，且
`<数据目录>/saves/<收编槽位>.llsave` **不存在**，就把它**复制**过去。

- **复制而不是移动**：移动会让老文件消失，万一收编本身有缺陷，玩家的原始档就没了。
  复制永远不删除任何东西，是这两个选择里唯一不可能造成数据丢失的那个。
- **代价（如实记录）**：玩家若把收编出来的槽位删掉，下次启动会再收编一次。这个方向
  是安全的（多一份档 ≫ 少一份档），单列在第八节。
- 收编出来的槽位名取 `screen-savelist-legacy-name` 的译文，模式取存档头里那一份
  （老档写的是 `Permadeath`，因此它会以肉鸽档的身份出现——**这是老档里真实记录的
  值，不猜、不改**）。

端到端测试：造一份**用旧路径、旧头部（没有 `save_name` 键）**写出的存档 → 跑一次
启动流程 → 断言它出现在槽位列表里、读得回来、玩家实体位置与存档一致。

### 3.7 自动存档：按**世界时间**

`AUTOSAVE_INTERVAL_TICKS = TICKS_PER_HOUR`（36 000 tick = 游戏内一小时）。

判据：`Session` 上记一个 `last_autosave_tick: Tick`；每次 `run_turn` 之后比较
`world.clock.0 - last_autosave_tick >= AUTOSAVE_INTERVAL_TICKS`。

**为什么必须是世界时间而不是墙钟**：墙钟会让存档时机取决于玩家盯着屏幕想了多久——
同一串输入在两台机器上（或同一台机器的两次运行）会在**不同的世界状态**上触发存档。
那正是约束 C4 禁止的那类隐藏输入。世界时钟只由回合推进驱动，是玩家输入的纯函数。

一小时这个值的理由：`TICKS_PER_HOUR` 是既有常量，不新造魔数；游戏内一小时对应几十
到上百个回合，既不会频繁到每几步就卡一次盘，也不会久到死一次退回很远。

### 3.8 肉鸽 → 普通：单向不可逆怎么在类型层面保证

**不新造类型。** `ll_content::mode::SaveMode` 已经就是这件事：

- `Permadeath`（= 肉鸽）与 `FreeSave { downgraded_from_permadeath }`（= 普通），
  后者的标记字段**是模块私有的**；
- `downgrade()` 是唯一的转换入口，`match` 里**没有任何一个分支返回 `Permadeath`**；
- 因此「普通 → 肉鸽」不是「不该写」，是**写不出来**。已有 4 条单元测试钉着。

本批要补的是**把它接上**（今天 7 处调用点全硬编码 `Permadeath`），并**堵掉一个洞**：

> `SaveHeader::mode` 今天是 `pub` 字段。crate 外可以 `header.mode = SaveMode::Permadeath;`
> 把降级抹掉。

修法照 `4394668`（世界身份收拢）那个先例逐字复制：

1. `SaveMode` 移进 `WorldIdentity`（私有字段），`bind` 多一个参数、
   `restore_from_header` 从 `header.mode` 接回来；
2. `SaveHeader::mode` 收成 `pub(crate)`，加只读访问器 `mode()`；
3. `SaveHeaderMeta` **删掉** `mode` 字段——`SaveHeader::new` 从 `identity.mode()` 取；
4. `save_game` 的 `mode: SaveMode` 参数**删掉**（它现在只能来自世界身份）；
5. `WorldIdentity` 上唯一的模式变更入口：

   ```rust
   /// 唯一允许的模式变化：肉鸽 → 普通。反向不存在，见
   /// `ll_content::mode::SaveMode::downgrade`。
   pub fn downgrade_mode(&mut self) -> bool;
   ```

   它内部就是 `SaveMode::downgrade()`，没有第二份判据。

**于是「存档时把模式改回肉鸽」和「存档时重算生成期 mod 集合」变成同一种编译错误**：
`SaveHeaderMeta` 里没有那个字段可以填，`WorldIdentity` 上没有那个方法可以调。

加一条 `compile_fail` 文档测试钉住（先例：`ll-content/src/degrade.rs` 的
`ReadOnlySave`）。

**模式在哪儿选**：世界配置屏（`world_setup`）新增一行「存档模式」，左右键在两者之间
切换。它是世界的属性，与地形形态旋钮同屏，`NewGameDraft` 多一个 `mode` 字段。

### 3.9 死亡接线：**复用批次 8 留的三处接缝，不抄第三份**

`run_turn` 之后检查 `session.game_world.world.actors.get(player).is_none()`：

1. `identity.downgrade_mode()`——肉鸽变普通（已经是普通就是无操作）；
2. **存一次**（把模式变化落盘；不存的话玩家一关游戏，降级就白降了）；
3. 把 `GameWorld` 从 `Session` 里取出来（`self.session.take()`），
   `timeline` 用 `crate::world::rebuild_timeline` 重建（`Session::begin` 当初把它
   `mem::take` 走了）；
4. 进 `ScreenState::CharacterCreation`，草稿是
   `NewGameDraft { world: Some(那个世界), world_already_exists: true, .. }`
   ——**批次 8 第七节留的接缝 1**，状态机因此跳过世界配置屏直接去选出生地；
5. 玩家选完 → `apply_character_choice`（**接缝 2**）+ `move_player_to`（**接缝 3**）
   → `Session::begin` → 继续玩同一个世界。

**没有删档。** 所有者的修正原话：「死亡后变成一般模式，可以再创建角色然后选择在
某个地方出生。」

---

## 四、落地形状

### 新增文件

| 文件 | 内容 |
|---|---|
| `crates/ll-game/src/atlas_miss.rs` | 任务 A 的去重器与结局枚举 |
| `crates/ll-game/src/save_slot.rs` | 槽位标识、列表、收编、时间戳格式化 |
| `crates/ll-game/src/save_name.rs` | 一行文本输入 + 命名屏状态机 |
| `crates/ll-game/src/save_list.rs` | 存档列表屏状态机 |
| `crates/ll-game/tests/save_slots.rs` | 多槽位 / 命名 / 列表 / 迁移端到端 |
| `crates/ll-game/tests/atlas_miss_wiring.rs` | A3/A4 接线断言 |

### 改动的既有文件

`ll-content`：`mode` 收进 `WorldIdentity`、`SaveHeader.mode` 收 `pub(crate)`、
`SaveHeaderMeta` 去掉 `mode`、`SaveHeader` 新增 `save_name`。
`ll-game`：`app.rs`（四处日志接线 + 屏路由 + 自动存档 + 死亡判定）、`lib.rs`
（`GamePaths`）、`save.rs`（签名）、`session.rs`（槽位 + 自动存档节拍）、
`menu_screen.rs`（`MenuRow`、两个新 `ScreenState`、新 `ScreenNotice`）、
`title_screen.rs`（「读取存档」进列表屏）、`settings_view.rs`（行文字）、
`chargen.rs`（草稿加 `mode`/`save_name`）、`world_setup.rs`（模式那一行）。
`assets/locales/{en,zh-CN}.ftl`：全部新文案，两个语言都加。

---

## 五、黄金基准

本批**不碰任何世界状态语义**：`WorldState` 的结构、`resolve`/`apply`、
`build_new_world` 的地形与实体产出一个字节都不动。模式标记住在
`WorldIdentity`（**不在 `WorldState` 里**，不参与 `world.hash()`）。

| 基准 | 期望值 |
|---|---|
| `EXPECTED_WORLD_DIGEST` | `10_180_278_885_427_934_050` |
| `EXPECTED_REPLAY_DIGEST` | `4_180_595_409_733_934_027` |

真变了就说明改动漏进了世界状态——**不要改常数**，走四步重冻并解释。

---

## 六、范围边界（不要越界）

- **不新增任何 `examples/` target**。
- **不用合成按键做验收**（ADR 0025）——全部普通 `#[test]`，程序化驱动公开路径。
- **不拆 `app.rs`**（3773 行，既有违规）——新代码全部进新模块。
- **不做**存档删除 UI、不做存档重命名、不做云同步。
- **不做** IME / 中文输入（3.4 已说明）。
- **不碰** `LostLand`（main 工作树）。

---

## 七、必须新增的测试（每条按 ADR 0018 做反例验证）

见第 1.5 节（A1–A4）与下表。

| # | 断言 |
|---|---|
| B1 | 普通模式的暂停菜单有「保存」这一行；肉鸽模式**没有** |
| B2 | 「返回主菜单」会先写盘：调用之后槽位文件的 `saved_at` 变新，且 `session` 变空、屏回到 `Title` |
| B3 | 写盘失败时**不**回主菜单（仍在 `Menu`），并给出 `SaveFailed` 提示 |
| C1 | 两个不同名字建出两个并存的槽位文件，列表列出两条，各自读回各自的世界种子 |
| C2 | 同一个槽位存两次是**覆盖**，不是新建第三个文件 |
| C3 | 名字过滤：非法字符换成 `_`；空名字退回默认；重名追加 `-2` |
| C4 | 列表按 `saved_at` 倒序；目录里混进一个坏档时**跳过它**、其余照常列出 |
| C5 | 老存档收编：旧路径 + 缺 `save_name` 键的头部 → 出现在列表里、读得回来、玩家位置一致（**端到端**） |
| C6 | 自动存档按世界时间：世界时钟走满一小时触发一次，**同一串输入触发时机逐次相同**；不足一小时不触发 |
| C7 | 肉鸽档死亡后模式变普通，**存档文件仍在**，且再存一次读回来仍是普通 |
| C8 | 普通 → 肉鸽写不出来：`compile_fail` 文档测试 |
| C9 | 死亡后走的是批次 8 那三处接缝：草稿的 `world` 是 `Some`、`world_already_exists` 为真、下一块屏是选出生地而不是世界配置 |

---

## 八、规格没裁定、本批临时选的做法（留给所有者）

落地过程中新增的补在最终报告，不在这里预写。开工时已知的：

1. **回主菜单先自动存一次**（2.2），不弹确认框。
2. **命名输入只接受 ASCII 字母/数字/空格/`-`/`_`，且不分大小写**（3.4）。
3. **老存档用复制收编，不移动**；被删掉会再收编一次（3.6）。
4. **自动存档间隔 = 游戏内一小时**（3.7）。
5. **存档模式在世界配置屏选**，不是单独一块屏。
6. **死亡后不删档、不清空世界**——只换一个角色（所有者修正原话）。

---

## 九、落地之后：与本计划的偏差

计划是开工前写的，落地过程中有以下几处与它不符。**逐条如实记录**，不回头
改前面几节假装计划一开始就是这样。

### 偏差 1：任务 A 的根因比所有者定位的多一处，且主因不在他指的那里

计划第 1.1 节写的时候已经发现了：`lookup_first` **本身是对的**（它的文档
明写「只有最后一个候选走 `lookup`」）。刷屏的主因是 `push_surface_draw` 的
**压制判定**用了会打日志的取用接口，次因是「可选层（`fallback_key: None`）
的最后一个候选也被当成必需品」。两处都是「把探测当成取用」。

### 偏差 2：所有者中途追加了「这一类改成 Warning」

级别降级与去重是**两条独立要求**，都做了。计划第 1.2/1.3 节是收到裁定之后
补写的。

### 偏差 3：A3/A4 的落点从 `app.rs` 移到了纯模块

计划第 1.5 节说 A3/A4 落在 `app.rs` 单元测试。实际做不到：`push_surface_draw`
要一台真实 GPU 设备，而 `app.rs` 的测试全部在 `resources: None` 下跑。于是把
**压制判定也收进** `MissLedger::resolve_draw`，A3/A4 变成不需要 GPU 的纯单元
测试。这不是妥协——收进去之后「探测是静默的」这件事只需要成立一次。

### 偏差 4：存档模式**不新造类型**，`ll_content::mode::SaveMode` 早就是它

计划 3.8 节预判对了。真正的工作是「把它接上」（7 处调用点全部硬编码
`Permadeath`）与「堵掉 `SaveHeader::mode` 是 `pub` 字段这个洞」。

### 偏差 5：`build_new_world` 的签名**没有**改，另开了一个 `_with_mode`

计划没写这一层。开工后 grep 出 57 个调用点，它们一个都不关心模式；加第三个
参数等于让 57 处各写一遍 `SaveMode::fresh_free_save()`。

### 偏差 6：`menu_screen.rs` 被拆出 `pause_menu.rs`

加两行之后它到了 910 行，越过 800 上限（基线 774）。照批次 6 拆
`title_screen.rs` 的先例拆分：共用类型仍只有一份住在 `menu_screen`。

### 偏差 7：`app.rs` 的新断言住在 `app_save_tests.rs`（`#[path]` 子模块）

计划第六节的边界写「不拆 `app.rs`，新代码放新模块」。产品代码确实都进了新
模块，但四百多行断言必须摸 `Demo` 的私有字段与私有方法，集成测试摸不到。
用 `#[path]` 挂成 `crate::app` 的子模块：断言仍走真实私有路径，而 `app.rs`
没有因此多四百行。

### 偏差 8：**批次 8 的接缝有一处是断的，本批修好了**

批次 8 第七节说「`chargen` 的状态机因此要按 `world.is_some()` 决定下一步去
哪块屏」。实际 `update_character_creation` 里 `CharacterRow::Next` **无条件**
去 `WorldSetup`——`world_already_exists` 这个字段有生产者、没有消费者。

死亡重生若照旧走那条路，玩家按下「生成世界」就会得到一个**全新的世界**，
这局玩过的一切当场被抹掉。本批把那条分支接上，并加了一对断言（重生走选出
生地 / 开局仍走世界配置屏）。

### 偏差 9：死亡重生要**造**一个新玩家，不是「挪」一个

计划 3.9 节说复用 `apply_character_choice` + `move_player_to`。两者都要求
玩家实体**还在**，而死亡在本仓库里就是实体从 arena 里消失。新增
`world::respawn_player`——它只是 `build_player_agent`（批次 8 接缝 2）+
`actors.spawn` + 三处 id 同步的装配，没有一行属于「玩家长什么样」的逻辑。

顺带把 `apply_character_choice` 从 `generate_draft_world` 挪到了
`finish_entering_world`：两条路共用同一处，新游戏那条路也不再应用两次。

### 偏差 10：`prepare_spawn_pick_view` 从 `generate_draft_world` 里拆出来

进选出生地屏有两条路（新游戏「先生成再准备」、重生「只准备」）。不拆的话
重生那条路会顺手把世界重新生成一遍。

### 偏差 11：交接文档第〇节的 `EXPECTED_REPLAY_DIGEST` 又过期了一次

文档写 `6_885_882_507_408_978_859`，代码里实际是
`4_180_595_409_733_934_027`（批次 8 的性别字段动过它）。本计划第〇节的表是
grep 复核过的。**这是那条纪律连续第二个会话真的派上用场。**

### 偏差 12：`lib.rs` 从 816 涨到 830

既有违规（基线就已越过 800）被本批加重了 14 行，主要是 5 行 `pub mod` 与
测试改写。已经把「取最近那一份存档」搬进 `save_slot::latest_slot` 抵掉一部分。
**未还清，如实记账。**
