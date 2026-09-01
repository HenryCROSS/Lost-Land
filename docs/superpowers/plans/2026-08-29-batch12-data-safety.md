# 批次 12：数据安全（D1 / N6 / N5 / D5）

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，全文 0 次出现「反例」。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**基线**：`main` 的 `2a865c4`（`Merge branch 'wt-uxdesign'`）。工作树
`wt-datasafety`，分支同名。改前基线测试数 **2768 passed / 0 failed**
（自己跑 `bash scripts/ci/run_tests.sh` 取，不抄别人的数字）。

**规格**：`knowledge/design/ui-and-navigation.md` 第十节「优先级总表」P0 的
**第一组：数据安全批（N6 + N5 + D5）**，外加它 2.2 节记的 D1。规格自己写明这
三条「面积最小、风险最高，先做」，且**不能**顺手把第二组（N8/N7/N2/N1/N10）
一起做——那五条必须同批落地。

**本批不做**：导航收敛（P0 第二组）、布局与文本（P0 第三组）、`examples/`
（并行批次正在删）。

---

## 〇、开工前自己 grep 复核过的数字（不信任何口头转述）

交接文档 `2026-08-28` 第〇节的纪律：**这里也不留副本**，跑这两条：

```bash
grep -rn "const EXPECTED_WORLD_DIGEST\|const EXPECTED_REPLAY_DIGEST" \
  crates/ll-world/tests/determinism.rs crates/ll-sim/tests/replay.rs
grep -n "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs
```

**本批预期这三个数一个都不变**：不碰世界生成、不碰模拟、不碰内容哈希，只动
`ll-game` 的屏路由与草稿类型。两条黄金基准仍要实跑验证（`2026-08-27` 交接
纪律第 4 条）。

---

## 一、D1 的完整链条（每一环都已 grep 复核，行号按基线 `2a865c4`）

规格 2.2 节 D1 描述的那条链，逐环核对结果：

| 环 | 位置 | 核对结论 |
|---|---|---|
| ① 死亡 → 角色创建，草稿是 `for_reincarnation` | `app.rs:967-971` | 成立 |
| ② 草稿上 `world_already_exists=true`、`existing_target=Some` | `chargen.rs:485,488` | 成立 |
| ③ 角色创建「下一步」跳过世界配置，直接去 `SpawnPick` | `chargen.rs:279` | 成立（注释 `:272-278` 论证了为什么必须跳过） |
| ④ **选点屏取消目标写死 `WorldSetup`** | `spawn_pick.rs:318-323` | **成立——这是入口** |
| ⑤ 世界配置屏「生成世界」→ `generate_draft_world` | `app.rs:1349-1352` → `:1359` | 成立 |
| ⑥ `draft.world = Some(全新世界)`，**不看 `existing_target`** | `app.rs:1381` | **成立——世界在这里被换掉** |
| ⑦ `existing_target` 全仓库无任何清空点 | `grep existing_target` 只有 `chargen.rs:452/488` 两处写入 | **成立** |
| ⑧ `finish_entering_world` 仍选中老槽位 | `app.rs:1613-1615` | **成立** |
| ⑨ 此后自动/手动/退出存档全写那个槽位 | `app.rs` 的 `enter_world_in_slot` | 成立 |

**两次按键**（选点屏 Esc、世界配置屏确认）**，无确认框，玩家那一局永久消失。**

### 顺带核实出的一处死代码

`app.rs:1534-1541` `enter_world_at` 里那个 `if draft.existing_target.is_some()`
的两个分支**返回值逐字相同**（都是 `going(SaveNaming)`）。真正的短路在
`update_save_naming`（`app.rs:1551`）。本批把那个空分支删掉——它读起来像
「转生这条路走了另一条道」，实际没有。

---

## 二、修法：把「世界从哪来」与「存哪个槽位」合成一个类型

### 2.1 为什么不是「加一道 if」

规格十一节明确否决过「加一个确认对话框」，理由是「治标」，并要求
**「写不出来」比「提醒一下」可靠**，点名与「存档时重算生成期 mod 集合现在
编译不过」（批次 9 第 3.8 节）是同一种解法。

D1 的结构性根因是**同一件事被两个字段表达**：

```rust
// chargen.rs:408 与 :425（基线）
pub world_already_exists: bool,
pub existing_target: Option<crate::save_slot::SaveTarget>,
```

它们必然同真同假（`new()` 里 `false`/`None`，`for_reincarnation` 里
`true`/`Some`），但类型上互不相干；而第三个字段 `world: Option<GameWorld>`
可以被任何人单独换掉。**D1 就是这三者漂移出的那个非法组合：新世界 + 老槽位。**

### 2.2 新增模块 `crates/ll-game/src/draft_world.rs`

（`app.rs` 已 4400+ 行，交接第四节第 8 条记着这笔账。新代码放新模块。）

```rust
/// 草稿手里那个世界的**来处**——它同时回答「世界从哪来」与「将来存进哪个
/// 槽位」，因为这两件事是同一件事的两面。
pub enum DraftWorld {
    /// 新游戏：世界由本流程现场生成，槽位要等命名屏之后才开。
    Fresh(FreshWorld),
    /// 转生：世界与槽位都来自磁盘上已经存在的那一局。
    Reborn(RebornWorld),
}

/// 新游戏那条路手里的世界。**这个类型里根本没有槽位字段**。
pub struct FreshWorld { world: Option<GameWorld> }   // 字段私有

/// 转生那条路手里的世界与它的槽位，**绑在一起**。
pub struct RebornWorld { world: GameWorld, target: SaveTarget }  // 字段私有
```

关键在于**写入路径**：

```rust
impl DraftWorld {
    /// 拿到「可以往里放一个新生成的世界」的那个槽——**只有新游戏那条路
    /// 有**。转生草稿返回 `None`。
    pub fn generatable(&mut self) -> Option<&mut FreshWorld>;
}
impl FreshWorld {
    /// 把刚生成出来的世界放进去。**本类型没有槽位字段**，所以「生成了新
    /// 世界却还指着老槽位」在这里写不出来。
    pub fn put(&mut self, world: GameWorld);
}
```

`RebornWorld` **不提供任何替换 `world` 的方法**，字段私有 ⇒ 模块外拿到
`&mut RebornWorld` 也换不掉里面的世界。于是：

- **「新生成的世界 + 老槽位」这个状态在类型层面表示不出来**：能接收新世界的
  只有 `FreshWorld`，它没有槽位字段；持有槽位的只有 `RebornWorld`，它没有写
  世界的方法。
- `generate_draft_world` 必须先过 `generatable()` 这道 `match`——**转生草稿上
  那条路径根本编译不出调用**（拿不到 `&mut FreshWorld`）。

**残余口子如实记录**：`RebornWorld::new(world, target)` 本身仍然可以被人拿一
个新生成的世界和一个老槽位去调。它是转生的唯一构造入口（`for_reincarnation`
一处调用），且调用点必须**显式写出那个老槽位**——不是漂移，是明写。文档里点
出这一点，并由 D1 那条端到端断言兜住。

### 2.3 `NewGameDraft` 上的改动

三个字段 → 一个：

```rust
- pub world: Option<crate::world::GameWorld>,
- pub world_already_exists: bool,
- pub existing_target: Option<crate::save_slot::SaveTarget>,
+ pub world: crate::draft_world::DraftWorld,
```

`world_already_exists` 的全部读点改成 `draft.world.is_reborn()`；
`existing_target` 的全部读点改成 `draft.world.existing_target()`。

---

## 三、N6：`generate_draft_world` 的闸门

规格原文：「`existing_target.is_some()` 时直接返回错误并留在原地，**不生成
世界**」。

```rust
let Some(fresh) = draft.world.generatable() else {
    tracing::error!("转生草稿上不存在「生成世界」这条路径，拒绝生成，留在原地");
    return ChargenUpdate::idle();
};
```

**留在原地、不加提示语**：按规格字面。N5 之后这块屏在转生流程里已经结构性
不可达，这道闸门是纵深防御，不该为一条不可达路径新造用户可见文案（那要动
`ScreenNotice` + 两份 `.ftl`）。

同时按规格把 `app.rs:1439-1441`（选点屏缺世界时回退到 `WorldSetup`）改成回退
到 `CharacterCreation { cursor: 0 }`——**任何降级路径都不许把玩家扔到世界配置
屏上**。

---

## 四、N5：选点屏记住来处

照抄 `SettingsOrigin`（`menu_screen.rs:144-176`）的形状，不新造机制。

```rust
// menu_screen.rs
pub enum SpawnOrigin { WorldSetup, CharacterCreation }
impl SpawnOrigin { pub fn screen(self) -> ScreenState { .. } }

- SpawnPick,
+ SpawnPick { origin: SpawnOrigin },
```

三个入口各自产出正确的来处：

| 入口 | 位置 | origin |
|---|---|---|
| 世界配置「生成世界」 | `app.rs:1384` | `WorldSetup` |
| 转生（角色创建「下一步」） | `chargen.rs:279` | `CharacterCreation` |
| 命名屏按 Esc 退回 | `save_name.rs:190` | **进选点屏时的那一个** |

第三条是关键：命名屏不能自己编一个来处。`ScreenState::SaveNaming` 因此也带上
`origin: SpawnOrigin`（`ScreenState` 是 `Copy`，装得下），一路透传。

**为什么不干脆从草稿推**（`is_reborn()` 与 origin 今天一一对应）：那等于把
「哪块屏能回哪块屏」这件导航的事塞进存档草稿，而规格 N5 的判据是按屏写的；
且 P0 第二组会把这套来处统一进模态栈，届时改的是同一处。照规格走。

**取消目标的读点**：`spawn_pick::update_spawn_pick` 新增一个 `origin` 参数，
`:319-322` 改成 `SpawnPickUpdate::going(origin.screen())`。

---

## 五、D5：存档列表的光标接上

```rust
// app.rs:1757（基线）
fn selected_slot(&self) -> Option<SaveSlot> { self.save_slots.first().cloned() }
```

`ScreenOutcome::LoadSave` 的**唯一生产者**是 `save_list.rs:81`（已 grep 复核，
首页那一行改成进列表屏之后就只剩这一条路）。而 `update_screen` 的漏斗在
`match outcome` **之前**就把新光标写回了 `self.screen`，所以 `load_saved_game`
跑的那一刻 `self.screen == Some(SaveList { cursor })` 已经是本帧的光标。

修法：

```rust
fn selected_slot(&self) -> Option<SaveSlot> {
    let Some(ScreenState::SaveList { cursor }) = self.screen else { return None; };
    let cursor = crate::save_list::clamp_cursor(cursor, &self.save_slots);
    self.save_slots.get(cursor).cloned()
}
```

夹一次而不是直接索引：列表可能在玩家离开这块屏期间变短，`clamp_cursor` 已经
是这件事的既有判据（`save_list.rs`），不写第二份。

**不在列表屏时返回 `None`**：读档只有列表屏这一条路，别处调到它是状态错乱，
返回 `None` 会走既有的「留在原地 + `NoSave` 提示」降级路径，不 panic。

---

## 六、必须新增的测试（每条按 ADR 0018 做反例验证）

| # | 断言 | 落点 | 基线颜色 |
|---|---|---|---|
| T1 | **端到端**：转生 → 角色创建 → 选点屏 → **按一次取消** → 屏不是 `WorldSetup`；接着走完全程进世界并存档 → 磁盘上仍是**同一份**槽位、且世界种子逐位未变 | `app_save_tests.rs` | **红**（必须先跑一遍确认） |
| T2 | 转生草稿调 `generate_draft_world` 被拒绝：世界 `seed` 与 `WorldState::hash()` 都不变（N6 判据原文） | `app_save_tests.rs` | 红 |
| T3 | 从世界配置进选点屏、按取消 → 回世界配置（N5 判据 1，守既有行为） | `spawn_pick.rs` | 绿（要靠改坏证明咬得住） |
| T4 | 从转生进选点屏、按取消 → 回角色创建（N5 判据 2） | `spawn_pick.rs` | 红 |
| T5 | 命名屏按 Esc 回选点屏、再按 Esc → 回**进选点屏时的那个来处** | `app_save_tests.rs` | 红 |
| T6 | 「新生成的世界 + 老槽位」写不出来——`compile_fail` 文档测试（先例 `ll-content/src/degrade.rs` 的 `ReadOnlySave`） | `draft_world.rs` | 新增即绿，靠删掉私有性证伪 |
| T7 | 存档列表光标停在第二份、按确认 → 读的是第二份（D5） | `app_save_tests.rs` | 红 |
| T8 | 选点屏缺世界时的降级目标不是 `WorldSetup` | 改既有 `app_save_tests.rs` 那一条 | 红 |

**ADR 0025**：全部程序化驱动（`走一帧`/`frame`/直接调 `update_*`），不合成任何
键盘事件。

---

## 七、落地形状

**新增**：`crates/ll-game/src/draft_world.rs`（目标 < 300 行）。

**改动**：

- `crates/ll-game/src/chargen.rs`——三字段合一、`world_already_exists` 参数换
  形状、「下一步」带上 origin。
- `crates/ll-game/src/menu_screen.rs`——`SpawnOrigin` + 两个变体带字段。
- `crates/ll-game/src/spawn_pick.rs`——取消目标读 origin。
- `crates/ll-game/src/save_name.rs`——退回选点屏时透传 origin。
- `crates/ll-game/src/app.rs`——闸门、降级目标、`selected_slot`、删死分支、
  路由跟着新字段走。**净增行数控制在最小**。
- `crates/ll-game/src/app_save_tests.rs`——新断言。

**不动**：存档主体形状（`check_save_schema_version.py` 因此不该拦；若拦了，
照它的提示升版本 + `--bless`）、世界生成、模拟、内容哈希、`examples/`。

**i18n**：本批**不新增任何用户可见字符串**。

---

## 八、提交拆分

1. `fix: 转生流程按一次取消不再落到会抹掉世界的那块屏`（D1 + N6 的类型层修法）
2. `fix: 选点屏记住自己是从哪块屏进来的`（N5 的三个入口与判据）
3. `fix: 存档列表的光标不再是装饰品`（D5）

---

## 九、规格没裁定、本批临时选的做法

落地时逐条填进最终报告。开工时已知的三条：

1. **N6 被拒绝时不给用户可见提示**（规格只说「留在原地」）——理由见第三节。
2. **`ScreenState::SaveNaming` 也带 `origin`**——规格 N5 只点了 `SpawnPick`，
   但它的判据 3（命名屏退回后再退，要回到进选点屏时的来处）逼出这一条。
3. **`selected_slot` 在非列表屏返回 `None`**——规格没说别处调到它该怎么办。

---

## 十、落地之后：与本计划的偏差

### 偏差 1：D1 端到端断言比的是**世界时钟**，不是世界哈希

计划第六节 T1 写的是「世界种子逐位未变」。**实跑推翻了它**：转生草稿的
种子取自世界身份（`chargen.rs` 的 `for_reincarnation`），按它重新生成出来
的世界与原来那个**逐位相同**，覆盖了也看不出来——第一版断言在真正被覆盖的
情况下照样绿。

改成先把世界时钟往前拨一段（「玩过一阵」），再断言盘上那份的时钟没掉回
生成时的初值。世界哈希也不行：转生**本来就会**改动世界（造一个新玩家实体、
把他放到选中的那一格），逐位相同是错的判据。

**这一条是 ADR 0018 那条纪律的又一个实例**：断言写出来不算数，要看它在
「实现被改坏」时是不是真的红。

### 偏差 2：D1 / N6 / N5 落在**同一个提交**里

计划第八节想拆成两个（D1+N6、N5）。做不到：N6 的闸门挂在
`DraftWorld::screen_after_character_creation` 上，而那个方法的返回值是
`ScreenState::SpawnPick { origin }`——N5 的类型改动是它的编译前置；反过来
先落 N5 的话，D1 那条端到端断言在中间那个提交里是红的。

**拆开会产出一个编译不过或测试不过的中间提交**，那比提交粒度更硬。合成
一个，提交信息里把三条分别写清楚。

### 偏差 3：`test_content` 从 `app::tests` 搬到了 `test_support`

`crate::draft_world` 的断言也要同一份内容集，而 `#[cfg(test)] mod tests`
是各模块私有的、兄弟模块看不见。第二份拷贝正是 `test_support` 当初被抽出来
要防的那件事，所以是搬不是抄——`app::tests::test_content` 现在只剩一行委托。

### 偏差 4：`RebornWorld` 上没有 `spare` 那类字段，`generatable` 因此是纯 `match`

计划 2.2 节写的 API 形状原样落地。反例验证里给 `RebornWorld` 加一个
`FreshWorld` 字段并从 `generatable` 返回它，`转生草稿拿不到可以写新世界的
那个槽` 当场变红——那条断言盯的正是这个形状本身。

### 偏差 5：`SpawnOrigin` 住在 `spawn_pick.rs`，不是 `menu_screen.rs`

计划第四节写的是「照抄 `SettingsOrigin`」，那自然读成「摆在它旁边」。落地时
量了一下：`menu_screen.rs` 基线 **815 行，本来就已经越过 800 行的文件上限**
（既有违规），把 `SpawnOrigin` 摆进去会变成 861。

搬到 `spawn_pick.rs`（它描述的就是那块屏，而那个模块也是唯一读它的地方）之后
是 826——**仍然比基线多 11 行**，那 11 行是两个枚举变体各多一个字段加各自的
文档，结构上省不掉。`SettingsOrigin` 原地不动。

### 偏差 6：T6 那条 `compile_fail` 文档测试真的写了，而且验证过它咬得住

计划第六节把它标成「新增即绿」。落地后按 ADR 0018 补了反例验证：把
`RebornWorld::world` 改成 `pub`，那条 `compile_fail` 当场变红（示例代码编译
过了）。旁边还留了一条 `no_run` 对照组，钉住「转生的构造入口本身是合法的，
不合法的只有事后替换世界」。

### 偏差 7：计划第九节那三条「临时选的做法」全部按原样落地

一条都没变，见第九节。
