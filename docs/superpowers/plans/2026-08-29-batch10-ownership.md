# 批次 10：物品归属（`Owner`）落地 + 尸体与遗物平铺

**基线**：`93bf907`（main，`docs: 交接文档不再列常量数值……`）
**工作树/分支**：`wt-ownership`
**规格出处**：`knowledge/design/ownership-and-crime-detection.md` 一节、四节、八节
**并行批次**：`wt-textinput`（改 `crates/ll-platform/`、`ll-game/src/save_name.rs`、
`app.rs` 输入部分）——本批次不碰这三处。

**改前基线**（本工作树自己跑的，不抄任何人的数字）：

```
bash scripts/ci/run_tests.sh  →  EXIT=0，2714 passed，0 FAILED
grep -rn "const EXPECTED_WORLD_DIGEST"  crates/ll-world/tests/determinism.rs:303
grep -rn "const EXPECTED_REPLAY_DIGEST" crates/ll-sim/tests/replay.rs:948
grep -n  "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs:805
```

（三个常量的**当前值**按上面三条命令现取，本文不留副本——
`2026-08-28-session-handoff.md` 第〇节记录了「文档里存一份会漂移的副本」
三次事故的形状。）

---

## 〇、这一批做什么、不做什么

设计文档八节把「现在能做的」列了六条。本批次认领其中的 **A、C 两条半**，
外加一件设计文档没写、由所有者本次口头裁定的事（B：尸体平铺）：

| 设计文档八节的条目 | 本批次 |
|---|---|
| `ItemStack` 加 `owner`，`can_merge` 追加比较 | ✅ 做 |
| `ItemStack` 加 `stolen_marker` | ❌ **不做**——它只服务销赃计时，而销赃计时属于犯罪批次；提前加等于又一个死字段 |
| `GroundItemStack` 构造点补 `Owner::Unowned` 默认值 | ✅ 做（落在 `ItemStack` 上，见下 1.2） |
| `resolve_pick_up` 插入归属判定分支（盗窃/`stolen_marker`） | ❌ **不做**，只留**挂载点**并写进文档 |
| `witnessed_by` 两段式目击算法 | ❌ 不做（犯罪批次） |
| `HistoricalEventKind::Crime` / `record_crime` | ❌ 不做（犯罪批次） |
| `launder_stolen_items` / `NPC_BASE_SIGHT_RADIUS` | ❌ 不做（犯罪批次） |
| `register-item-theft-exempt` | ❌ 不做（犯罪批次） |
| `Effect::TransferOwnership` + `apply` 侧方法 | ✅ 做（设计文档四节的接口形状，调用方今天不存在） |
| **拾取即归属**（所有者原话「谁拿了就变成谁的」） | ✅ 做——设计文档没有这一条，是本次所有者裁定新增 |

不做的那几条不是遗漏，是**边界**：它们全部依赖「目击判定 + 犯罪记录」
这一整套，而那套东西一旦开工就是一整批的量，塞进本批只会让两件事都做
不干净。设计文档二、三、五节整体归下一批。

---

## 一、任务 A：`Owner` 落地

### 1.1 类型本体

落在 `crates/ll-world/src/item.rs`（`ItemStack` 隔壁），五变体，**采纳设计
文档 1.2/1.3 两条修正**：

```rust
pub enum Owner {
    Unowned,
    Player,
    Npc(WorldId),        // 修正一：EntityId → WorldId（1.2）
    Faction(WorldId),    // 修正二：ContentIndex → WorldId（1.3）
    Shop(EntityId),      // 1.4：保留原样，标注待定
}
```

- 采纳 1.2 的理由（照抄设计文档，不重新论证）：`EntityId` 的世代号在
  `despawn` 后立即失效，而「这原本是谁的」必须在主人死后仍能读出；
  `KillRecord::killer`/`victim` 已经用 `WorldId` 解决过同一个问题。
- 采纳 1.3 的理由：`OrgRef::Instance(WorldId)` 已经把「势力是世界生成期
  产出的实例」这条钉死了，`Owner::Faction` 若继续用 `ContentIndex`，
  「物品的势力归属」与「角色的势力归属」会指向同一个东西却用两套不相容
  的引用类型。
- 1.4 `Shop(EntityId)` **原样保留**：商店到底是「具名 NPC 摆摊」还是
  「独立设施」属经济/社会文档的地盘，本批次不代其发言。
- 全部由 `Copy` 类型组成 ⇒ `ItemStack` 保持 `Copy`（设计文档 1.6 已核实）。

### 1.2 存哪：`ItemStack.owner`，不单独开表

照设计文档 1.6，不重新论证。`GroundItemStack` **不**单独加 `owner`——
它已经整体持有一个 `ItemStack`，归属跟着物品实例走（这也是三节 3.3
「转手不丢失标记」那条论证的地基），在 `GroundItemStack` 上再开一个
`owner` 就是第二真相源。设计文档 1.5 说的「`GroundItemStack` 新增该
字段」在 1.6 定稿时已经被 `ItemStack.owner` 取代——1.6 是后写的，且
给出了明确的结构体形状，以 1.6 为准。

`#[serde(default)]`：存档主体新增字段，走 `Agent.gender` 那条既有先例
（`state.rs` 的「缺性别键的老存档读得回来且取默认值」测试），
`CURRENT_SCHEMA_VERSION` 不动。`Owner` 自身 `#[derive(Default)]` +
`#[default] Unowned`。

进 `WorldState::hash()`：`write_item_stack` 追加一段（ADR 0022）。
**这会动世界摘要与回放摘要，走四步重冻。**

进 `ll_content::remap`：**不需要**——`Owner` 的三个带载荷变体里，两个是
`WorldId`（世界实例 ID，不随内容集变化），一个是 `EntityId`。没有
`ContentIndex`，没有可重映射的东西。这一条要在 `remap` 那一侧写清楚，
否则下一个人会以为是漏了。

### 1.3 `can_merge` 同批改

```rust
pub fn can_merge(a: &ItemStack, b: &ItemStack) -> bool {
    a.def == b.def && a.durability == b.durability && a.owner == b.owner
}
```

`item.rs` 模块文档的「落地时机」一节明写了这是同一个改动的两半。
`merge_stacks`/`split_stack` 因为用 `..a`/`..stack` 结构更新语法，一行
不改（设计文档开头「落地状态」已核实）。

**行为后果（真实存在，要有测试钉住）**：两堆同 `def` 同耐久但归属不同的
东西不再合并。今天唯一能造出归属不同的两堆的路径是「拾取即归属」
（1.5）——玩家捡起的东西是 `Player`，地上没被碰过的是 `Unowned`，两者
在背包里不会合并……**不会发生**，因为拾取那一刻归属就已经改成拾取者的
了，进背包时比较的是改写之后的值。真正会撞上的是「玩家丢下一堆
（`Player`）→ 另一个 NPC 捡起来（改写成 `Npc(..)`）」这类跨主人流转，
以及未来的赃物。

### 1.4 `Owner::Unowned` 是默认值，不改变任何现有行为

设计文档 1.5：「加这个字段只是把现有隐含语义显式化」。落地表现：

- `ItemStack::new`/`with_durability`/新造物品那条出口 → `Unowned`。
- `resolve_drop`/`resolve_place`/尸体掉落/盲盒溢出 → 原样搬移已有的
  `ItemStack`，`owner` 跟着走，不重置。
- 世界生成/出生装备/NPC 名册 → `Unowned`。

因此**除了「拾取即归属」这一条**，本批次的归属字段在默认参数下不改变
任何一条既有判定——这正是四步重冻第 ② 步要证明的东西。

### 1.5 拾取即归属

所有者原话：「也可以默认不归属于谁然后谁拿了就变成谁的」。

落在 `resolve_pick_up`：**只在 `ground.stack.owner == Owner::Unowned`
时改写**，改写成拾取者的归属：

| 拾取者 | 新归属 |
|---|---|
| 玩家（`world.player_entity == Some(actor)`） | `Owner::Player` |
| 有 `remembered_id` 的 NPC | `Owner::Npc(id)` |
| 没有 `remembered_id` 的 NPC | **维持 `Unowned`** |

第三行是 C1（`resolve` 只有 `&WorldState`）逼出来的：给一个无名 NPC
懒分配 `remembered_id` 需要 `&mut self`，只有 `apply` 能做。设计文档
1.2 已经点名「物品被赋予具体 `Owner::Npc` 时」应当成为
`remembered_id_of_or_assign` 触发清单的新增一项——**但那要新开一个
`Effect`，且它的唯一价值是给犯罪批次服务**。本批次取最保守的降级：
无名 NPC 捡到的东西继续无主，如实写进文档，留给犯罪批次接线。

**归属已经不是 `Unowned` 时，本批次原样保留、不比较、不拒绝**——那是
盗窃判定，属下一批。

### 1.6 `resolve_pick_up` 的挂载点

在「读出 `ground.stack`」与「产出两个 `Effect`」之间抽一个函数：

```rust
fn owner_after_pick_up(world: &WorldState, agent: &Agent, actor: EntityId, picked: ItemStack) -> Owner
```

它就是挂载点：犯罪批次要插的「这次拾取算不算盗窃」判定与
`stolen_marker` 写入，进的是这个函数（它已经拿到了判定需要的全部输入：
世界、拾取者、被拾取的堆）。函数文档里写明这件事，别让下一个人再去
`resolve.rs` 八千行里找位置。

### 1.7 「据点归属」用哪个变体

**结论：`Owner::Faction(SettlementSite::id)`。**

理由三条：

1. **五个变体里只有 `Faction` 指向「一个集体」**——`Player`/`Npc` 是
   自然人，`Shop` 是商业设施，`Unowned` 是没有主张。「这是这座据点的
   东西」是一次集体主张。
2. **载荷类型天然对齐**。`SettlementSite::id` 的字段文档原文：
   「永久标识，与历史事件、势力、家族共用 `WorldId` 空间」——采纳 1.3
   之后 `Faction` 的载荷正是 `WorldId`，据点 id 是这个空间里一个合法
   的值，不需要任何换算。
3. **它是一次可收窄的加宽，不是推翻**。建筑↔居民关系落地后，家具的
   归属从「这座据点的」细化成「住这儿的那个 NPC 的」，即
   `Faction(据点 id)` → `Npc(居民 id)`，是同一个字段换一个变体，不动
   任何结构。

**代价，如实记录**：`Faction` 这个变体名此后承载两类 `WorldId`——势力
实例（P9 落地后）与据点。二者在 `WorldId` 空间里全局唯一，不会互相
误认，但**变体的名字不再逐字对应它装的东西**。备选是加第六个变体
`Settlement(WorldId)`；没选它是因为设计文档 1.1 明确「五变体已经是
`item-system.md` 定型过的形状，未来系统都会对齐它」，而本批次同样没有
权限单方面给别的系统将来要用的枚举加变体。**这条列进报告第 10 节，
所有者一句话就能反转**（加变体 + 改一处赋值点，因为本批次不摆家具，
今天没有任何一处真的构造 `Faction`）。

本批次**不摆家具**（那属于据点建筑批次），只把这条表示法写进 `Owner`
的类型文档，让那批直接用。

### 1.8 `Effect::TransferOwnership`

设计文档四节：赠送/购买/任务发放在「改变 `Owner`」这个动作上完全同构，
**只需要一个 `Effect`，不是三个**。

```rust
Effect::TransferOwnership {
    holder: EntityId,       // 这堆物品现在在谁的背包里
    def: ContentIndex,      // 哪一种（第一条匹配，与 RemoveFromInventory 同一条既有定位纪律）
    durability: Option<i32>,
    new_owner: Owner,
}
```

`apply` 侧对应 `WorldState::transfer_item_ownership(..)`。设计文档给的
形状是 `{ stack_def, new_owner }`，**本计划把它补成上面四个字段**：
只给 `def` 定位不到具体是谁背包里的哪一堆，而 `apply` 是全局唯一写入口
（C1），它必须能唯一定位。`(holder, def, durability)` 这个三元组是
`Effect::RemoveFromInventory`/`MergeIntoInventory` 已经在用的定位方式，
照抄，不新发明。

**调用方今天一个都不存在**（无交易、无对话、无任务奖励发放）。因此：

- 不给它编造一个 `Intent`。
- 单元测试直接构造这个 `Effect` 喂给 `apply`——这是既有先例
  （`Effect` 的测试本来就这么写）。
- 设计文档四节末尾那条「转移方必须是当前 `owner`」的前置约束写进
  `Effect` 文档，作为将来三个系统各自落地时的要求；**本批次不实现它**
  （`apply` 不做判断，C1）。

---

## 二、任务 B：尸体与遗物平铺

### 2.1 死结

尸体现在是**容器**（`GroundItemStack::contents` 装遗物），而
`resolve_pick_up` 把 `contents` 非空的地面物品整体排除在拾取之外。于是
`CORPSE_STACK_LIMIT = 8` 至今只是一条诚实的声明，生产路径上尸体根本
捡不起来。

### 2.2 所有者的解法

> 「尸体会变成物品，然后原本的物品和尸体都会放在一格子内的掉落物列表里。」

`append_corpse_drop` 从产出**一条**带 `contents` 的 `AddGroundItem`，
改为产出 **1 + N 条**：尸体自己一条（`contents: Vec::new()`），死者的
每一堆遗物各一条，**全在同一个 `victim.pos`**。

死结当场解开：尸体的 `contents` 恒空 ⇒ 不再被 `resolve_pick_up` 的容器
排除挡住 ⇒ 可拾取、可堆叠，`CORPSE_STACK_LIMIT` 第一次真的生效。

### 2.3 `contents` 字段**不删**，改写它的分工

方向与家具那批相反：那批删掉了「丢家具即放置」这条合并，本批**保留**
`contents` 这个字段。理由是箱子——家具那批的箱子已经在 JSON5 注释里
写明了它将来是 `contents` 的正经消费者。

**要改的是字段文档**（`GroundItemStack::contents` 与类型文档那一段
「为什么用 `contents` 是否非空作判据」）：把「典型是尸体」全部改掉，
写清新分工——

- `contents` 非空 = 这是一个**真容器**（箱子、袋子……），里面的东西
  要开一次容器才拿得到；
- **尸体不再是容器**。它是一件普通的、可堆叠的地面物品；死者的遗物是
  同一格上另外若干条独立的地面物品。

`Intent::Loot`/`resolve_loot`/`InteractTarget::Container` 三处**保留
但暂时没有生产者**——它们是箱子那批的地基，删掉再写一遍是净损失。三处
文档都要写明「今天没有任何生产路径会造出容器」。

### 2.4 空手死者

现状：`loot.is_empty()` 时**不产出尸体**。那条 guard 的原文理由是
「`contents` 非空是『这是一具容器』的唯一判据……一具打不出任何东西的
尸体没有玩法意义（`resolve_loot`/`resolve_pick_up` 都不会把它当作合法
目标）」。

**这个理由被本批次自己作废了**：平铺之后 `resolve_pick_up` 就是尸体的
合法目标，一具空手死者的尸体是一件正常的、捡得起来的物品。因此
**去掉 guard，每一次死亡都产出尸体**。这也让所有者那句「尸体会变成
物品」在所有死亡路径上成立，而不只是「死者身上恰好有东西」的那一半。

代价：`ground_items` 会比现在多——每个空手死者多一条。老化清理
（30 天）照常收，与普通丢弃物同一条通道。这条**写进报告第 10 节**：
它改变行为、会动两条黄金基准，是本批次里最像「顺手扩大范围」的一步。

### 2.5 归属

| 东西 | 归属 | 理由 |
|---|---|---|
| 尸体本身 | `Unowned` | 设计文档 1.5 点名「怪物尸体」 |
| 死者的遗物 | **`Unowned`** | 见下 |

遗物判 `Unowned` 的三条理由：

1. **不改变任何现有行为**。今天从尸体上搜刮遗物零权限检查；判成
   `Npc(死者)` 会让「战场搜刮」在犯罪判定落地的那一刻**一次性**变成
   盗窃——那是一条所有者没有裁定过的玩法规则（战利品权利、继承），
   不该由本批次夹带。
2. 设计文档 1.5 把「野外掉落」与「怪物尸体」并列为 `Unowned` 的两个
   典型场景，遗物落在同一格、同一次结算里产出，与尸体同判是唯一自洽
   的选择。
3. **最容易反转**：真要改成「死者的遗物仍属死者」，是
   `append_corpse_drop` 里一行——死亡路径本来就是
   `remembered_id_of_or_assign` 今天唯一的真实调用点，死者恒有
   `remembered_id`，接线成本近乎为零。

### 2.6 同格堆数上限——先勘查

勘查结论（本计划写作时已核实）：

- `WorldState::ground_items` 是一个扁平的 `Vec<GroundItemStack>`，**没有
  任何按格计数的上限**。
- 唯一的数量约束是 `resolve_place`/`resolve_drop` 的「一格至多一件
  **放置物**」（`placed == true`），它不约束躺着的堆。
- 唯一的回收机制是 `cleanup_aged_ground_items`（30 天老化）。

**结论：本批次不引入上限。** 平铺把「1 条容器」变成「1 + N 条」，N 是
死者背包 + 装备的堆数——那些堆本来就已经存在于世界状态里（在
`contents` 这个 `Vec` 里），平铺**不增加物品总数**，只是把它们从嵌套
一层挪到顶层。真正的增量只有 2.4 那条（空手死者多一具尸体）。

### 2.7 交互列表会不会变长——勘查，不改规则

`interact_entries` 现状（已核实）：

- 立着的 → `Facility`；
- `contents` 非空 → `Container`，**只留第一具**；
- 其余 → `Loose`，**同一个 `def` 只留第一次出现**。

也就是说**列表长度 = 这一格上不同 `def` 的个数（+ 门那一行）**，不是
地面堆数。平铺之后一具尸体从「1 行 Container」变成「1 行尸体 +
每种遗物各 1 行」。

**本批次不改交互列表的任何规则**（所有者已裁定：脚下 + 相邻八格、无
条件弹）。要做的是**实测**：起一个死者身上有 k 种不同物品的场景，数
实际行数，如实报告。

---

## 三、四步重冻

`ItemStack` 加字段 ⇒ `write_item_stack` 多写一段 ⇒ `world.hash()` 变。
平铺 + 空手死者出尸体 ⇒ `ground_items` 内容变 ⇒ 摘要再变一次。
**两条黄金基准极可能都要重冻。**

四步，一步不少（`2026-08-27-session-handoff.md` 纪律第 2 条）：

1. **确认基线红**——改完之后跑 `determinism` 与 `replay`，两条都必须红。
   若某一条没红，说明这条基准根本没覆盖到本批改动的路径，要先查清楚
   为什么，不能直接抄新值。
2. **把改动关掉，确认精确回到旧值**。本批次的「关掉」怎么做：
   - `write_item_stack` 里新增的那一段用一个 `const` 开关注释掉；
   - `append_corpse_drop` 换回旧实现（一条带 `contents` 的
     `AddGroundItem`，含空手 guard）；
   - `owner_after_pick_up` 恒返回原 `owner`。

   三处一起关掉，两条摘要必须**逐位**回到旧值。
   **重点提防枚举/字段导致的索引平移**：气候批次就是在这一步抓到「新
   地形插在注册表中间导致其后条目 `ContentIndex` 整体平移」。本批次的
   风险点：`Owner` 是新枚举、不进注册表，理论上不平移；但**尸体物品
   的注册**（`ll_mod::corpse_item`）若被本批次碰到顺序就会平移。计划是
   一个字都不动那处注册——若第 ② 步回不到旧值，第一个要查的就是它。
3. **恢复**。
4. **新常数在两个独立进程里复现**（两次分开的 `cargo test` 调用，不是
   同一次跑两遍）。

四步证据写进提交信息。

---

## 四、提交切分

A 与 B 分开，中文提交信息，**不 push、不合并 main**：

1. `feat: 物品归属 Owner 落地——类型、字段、can_merge、拾取即归属`
   （含 `Effect::TransferOwnership` 与 `apply` 侧方法、存档兼容测试）
2. `feat: 尸体不再是容器——尸体与遗物平铺进同格掉落列表`
3. `chore: 重冻两条黄金基准`（四步证据在这条的信息里；若 1、2 各自
   都动了摘要，则各自重冻各自提交，不留一个中间红的提交）

—— 第 3 条的形式取决于第 ① 步的实测：**不允许存在一个测试是红的提交**，
所以重冻要跟着造成它的那次改动走，不是攒到最后。

---

## 五、验证清单

- [ ] `bash scripts/ci/run_all.sh` exit 0
- [ ] 改前/改后测试数都报告（改前：2714 passed / EXIT=0）
- [ ] 每条新断言做 ADR 0018 反例验证（故意改坏 → 确认红 → 改回），过程
      写进报告
- [ ] 老存档（缺 `owner` 键、尸体带 `contents`）读得回来、不读崩——端到端
      测试
- [ ] `check_field_consumers.py` 绿（`ItemStack.owner` 在
      `crates/ll-sim/src/resolve.rs` 有 `.owner` 点号读取，天然满足；
      但 `ItemStack` 当前**不在** `TARGET_TYPES` 里，要核实门禁到底管
      不管它，如实报告）
- [ ] `check_i18n_strings.py` 绿——本批次**不新增用户可见字符串**
      （归属不上屏）；平铺后交互列表里多出来的行用的是各物品自己已有的
      i18n 名字
- [ ] 新代码放新模块，不让 `resolve.rs`（近 8000 行）更糟
- [ ] 不新增 example target

## 五之二、落地后的实测修订（本节写于两批都做完之后）

计划第三节假设「两条黄金基准极可能都要重冻」。**实测结论相反：两条
都不需要重冻，而且这是用反例证的，不是「跑一遍没红就当没事」。**

- `write_item_stack` 函数体开头临时插一行 `hasher.write_u64(0xDEAD_BEEF)`
  ——让每一个物品堆都往哈希里灌一个显眼常量——`determinism` 九条与
  `replay` 七条**仍然全绿、摘要逐位不变**。这证明的比「新字段没影响」
  更强：**这两条测试的世界里根本不存在任何一个 `ItemStack`**。
- `append_corpse_drop` 产出的尸体数量临时改成 9999，两条**同样全绿**
  ——证明这两条测试的意图流里没有任何一次死亡。

两处 `const` 的上方各补了一段注释记录这次核实，与该文件里已有的
gender/equipment 两次同类记录同一形式。

计划第三节那句「极可能都要重冻」因此是**错的预判**，如实保留在上面，
不改写——留着它才看得出「第 ① 步不是走过场」。

## 六、规格没覆盖、临时选的做法（滚动记录，最终进报告第 10 节）

1. 据点归属用 `Owner::Faction`（1.7）——备选是加第六个变体。
2. 无名 NPC 拾取后维持 `Unowned`（1.5）——备选是新开一个懒分配 `Effect`。
3. 死者遗物判 `Unowned`（2.5）。
4. 空手死者也产出尸体（2.4）——这条改变行为。
5. `Effect::TransferOwnership` 的字段比设计文档四节多三个（1.8）。
6. 不落地 `stolen_marker`（〇节）。
7. `Intent::Loot`/`resolve_loot`/`InteractTarget::Container` 三处保留但
   **今天零生产者**（2.3）——备选是一并删掉，等箱子那批重写。
8. **`CURRENT_SCHEMA_VERSION` 2 → 3，不配迁移函数**（见下「五之三」）
   ——所有者手上那份真实存档从此被明确拒绝。备选是写一份迁移函数
   （需要一份「形状变了」的 `ItemStackV2` 镜像类型），但那与所有者
   已经裁定过的「老存档去掉就好了」相反。

## 五之三、存档：`serde(default)` 在真正的存档主体上是**空操作**

计划 1.2 写的「走 `Agent.gender` 那条既有先例，`CURRENT_SCHEMA_VERSION`
不动」**是错的**，落地时才查出来，如实记录：

- 存档主体走 **`postcard`**（`ll_content::save_file::save_to_file`），
  那是 non-self-describing 的二进制格式——字节流里没有字段名，反序列化
  按声明顺序逐字段吃字节。`#[serde(default)]` 需要格式能报告「这个字段
  缺席」，`postcard` 报告不了。
- **实测**（独立最小探针）：老结构体三字段编码 → 新结构体四字段带
  `#[serde(default)]` 解码 → `Err("Hit the end of buffer, expected more
  data")`。新字段若不在末尾会更糟：后续字段的字节被错位读成合法值。
- **`Agent::gender`（2026-08-28）与 `GroundItemStack::placed` 两条既有
  先例因此都是错的**：它们的「老存档读得回来」测试走的是
  `serde_json::Value`（自描述格式，`serde(default)` 在那里确实生效），
  **测不到真正的 `postcard` 主体那条路**。上一次真的动过
  `CURRENT_SCHEMA_VERSION` 是 2026-08-23（`2661a27`）。

本批次的处理：**`CURRENT_SCHEMA_VERSION` 2 → 3，不配迁移函数**，与
所有者已经裁定过的「老存档去掉就好了」一致（项目尚未发布，全部存档都是
开发期产物，`crate::migrations` 模块文档记录了这次裁定）。效果是老存档
走 `LoadError::SchemaMigrationGap` 这条**明确拒绝**的路径，而不是被当前
的字段布局静默误解析。端到端证据：`crates/ll-game/tests/save_slots.rs`
的「上一版schema的老存档被明确拒绝而不是静默误解析」，反例（把常量改回
2）实跑当场变红。

## 七、实测数字（改后）

- `bash scripts/ci/run_all.sh` → **exit 0**
- 测试数：改前 **2714** → 改后 **2737**（+23）
- 一格的交互列表长度 = 这一格上**不同 `def` 的个数**（+ 门那一行），
  不是地面堆数——`interact_entries` 对 `Loose` 按 `def` 去重这条规则
  平铺之前就存在，本批次一个字没改。实测证据：
  `crates/ll-game/tests/corpse_flattening_interact_list.rs` 三条
  （一具哥布林尸体平铺后 3 行；两具同物种尸体只占 1 行；六堆三种仍是
  3 行）。
