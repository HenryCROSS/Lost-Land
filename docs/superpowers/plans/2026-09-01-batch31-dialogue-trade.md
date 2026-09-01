# 批次 31：对话系统的批次 5——交易（`open-trade`、`Intent::Trade`、NPC 初始钱包、占位价格公式）

**工作树** `wt-dialogue5`，分支 `wt-dialogue5`，起点 `origin/main` = `8fd4ac5`。

**规格** `knowledge/design/dialogue-system.md` **五节 5.3**（交易）与 **八节**分批表第 5 行。
**上游交接** `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md` 第七节「批次 5」那一行：
「`open-trade` 后果（不产 `Effect`，只推 UI）、`Intent::Trade`、NPC 初始钱包（所有者裁定第 4 条）、
占位价格公式」。
**前一批的落地记录** `docs/superpowers/plans/2026-08-31-batch29-dialogue-quest.md`——本批沿用它的
全部约定（判别值往后接、反例验证要确认「红的原因是想验的那一条」、两组对照证伪、临时裁定单列）。

---

## 〇、开工基线（自己跑的，不抄任何文档）

```
bash scripts/ci/run_tests.sh   → 3001 通过 / 132 个二进制 / 0 忽略 / 0 失败
```

**第一次跑时红过一条，实测是偶发**：`ll-game --test npc_materialization` 的
`物化出的npc真的被既有回合引擎与行为树驱动` 在 `surface_store.rs:656` panic
（`SurfaceWindow 假定视野范围内的区块都已经常驻`）。单独重跑该二进制 7/7 绿，
再整跑一遍工作区 3001 全绿。**如实登记为一条偶发失败**——它不是纪律第 8 条那种链接器
OOM 假失败（日志里没有 `LNK1102`，测试真的跑起来了），根因未查，本批不动那条路径。

三个关键常量 grep 自取，**本文档不留副本**：

```bash
grep -n "pub const CURRENT_SCHEMA_VERSION" crates/ll-content/src/save_file.rs
grep -n "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs
grep -rn "const EXPECTED_" crates/ll-world/tests/determinism.rs \
  crates/ll-sim/tests/replay.rs crates/ll-game/tests/populated_determinism.rs
```

---

## 一、范围

| | 内容 | 提交 |
|---|---|---|
| **A** | `open-trade` 后果：一条新的 `DialogueOutcome` 变体，**不产 `Effect`**，只让 UI 推进交易屏 | 1 |
| **B** | `Intent::Trade` + 占位价格公式 + **复用**批次 4 那条 owner 校验（抽到 `ll_sim::ownership`，不另写一份） | 2 |
| **C** | NPC 初始钱包（所有者裁定第 4 条：不能是 0、按职业量级、从据点人口派生） | 3 |
| **D** | 交易屏 + 本体内容那一行 + 两份 `.ftl` + i18n 分类表 | 4 |

**不做**：真正的定价（库存/需求/政策/商路四因子、行会中介、商队——整体属 P9，规格 5.3 原话
「不要在这里实现它的任何一部分」）、商人库存补货、玩家初始钱包、多件一次成交、
`Effect::TransferOwnership` 的第一个调用方（批次 4 已论证它组合不出一条既正确又可观察的排法，
本批同样不用它）、NPC 姓名（批次 6）。

---

## 二、A：`open-trade` 的落地形状

### 2.1 「不产 `Effect` 只推 UI」与「C1 唯一写入口」不矛盾

规格 5.3 原话：这条后果「**不产出任何 `Effect`**，只把 UI 推进交易界面——与
`InteractTarget::Facility` 落到『打开制作菜单』是同一形状（那一支不产 `Intent`、不消耗回合）」。

**真正的交易（钱货两清）仍然走 `apply` 唯一入口**——那是 B 的 `Intent::Trade`。
两件事的分界与规格七节 7.1 那条分界逐字同一条：**「玩家现在停在哪块屏上」是 UI 状态，
「谁的钱和货变了」是世界状态**。

### 2.2 因此这条后果**不提交 `Intent::DialogueChoose`**

规格 7.2 原文：「纯导航的选项（`outcomes` 为空）不提交 `Intent`……提交一个恒产出空效果的
`Intent` 只会污染 `Intent` 日志。」`open-trade` 是同一档：它恒产出空效果。

落法是给 `DialogueOutcome` 加一个 `is_ui_only()`：

```rust
pub fn is_ui_only(&self) -> bool { matches!(self, DialogueOutcome::OpenTrade) }
```

`ll_game::dialogue_screen::update_dialogue` 的 `submit` 判据从
`!option.outcomes.is_empty()` 收窄成 `option.outcomes.iter().any(|o| !o.is_ui_only())`
——一条**只有** `open-trade` 的选项因此一个 `Intent` 都不提交；一条
`[set-flag, open-trade]` 混着的选项照样提交（`set-flag` 那一半要落地）。

`resolve` 那一侧仍然要有 `OpenTrade => {}` 这一支（穷尽 `match`，编译器逼它表态），
带一段注释写明「这里空着就是规格 5.3 那句话的落点」。

### 2.3 换屏：`open-trade` 压过 `next`

一条带 `open-trade` 的选项同时还有 `next`。**裁定：交易屏赢**，`next` 不生效。
理由：`ll_game::modal::Modal` 只有一个 `Option<ScreenState>`，不是栈；把会话屏留在底下需要
一层屏栈，那是 UI 那一批的事（本批不碰 `ll-ui`）。代价：**从交易屏退出回到的是世界，不是
刚才那段对话**。如实登记在第十一节，反转成本是屏栈落地之后改一行。

### 2.4 内容哈希

`write_dialogue_outcome` 加一条判别值 `4` 的分支，**往后接、不挪既有四条**。
这一支不带任何参数（「跟谁交易」由说话人回答，与 `JoinSettlement` 同理）。

**这让判别值守卫多出第二对**：`OpenTrade` 与 `JoinSettlement` 的载荷形状完全相同
（只有一个判别值），撞号之后字节流一模一样 ⇒ 把 `4` 改成 `1` 必须当场红。
批次 26 那条记账（「`JoinSettlement` 的判别值改坏了不红，等批次 5 的 `open-trade`」）
**本批才是它原本预告的那一刻**，批次 29 已经用另一对提前兑现过；本批把它预告的那一对
也验一遍，两处都写。

`CONTENT_HASH_ALGORITHM_VERSION` 递增（「已有表的枚举加变体」+「本体内容真的用上了新变体」，
ADR 0027 两件事都要求递增）。

---

## 三、B：`Intent::Trade` 与价格

### 3.1 形状

```rust
Intent::Trade { actor, partner, item: ContentIndex, direction: TradeDirection }
pub enum TradeDirection { Buy, Sell }   // 站在 actor 的角度：买进 / 卖出
```

**不带 `count`**——与批次 4 的 `give-item` 逐条同一收窄（规格 5.3 草图写的是
`{ partner, item, count, direction }`）。好处相同：`apply` 侧直接复用既有的
`Effect::ConsumeInventoryItem`（「数量减一，减到零整条移除」正是这个语义），
N > 1 需要一套拆堆机械而今天没有一条内容需要它（YAGNI）。反转成本相同：加一个默认 1 的字段。

### 3.2 五道闸门（任何一道不过 = 零效果）

1. `actor` 与 `partner` 都还在世界里；
2. 按 `direction` 定出 `(卖方, 买方)`；
3. **卖方背包里真的有一堆 `item`**（第一条匹配，与 `give-item` 的既有定位纪律相同）；
4. **owner 校验**——见 3.3；
5. **买方的 `wallet` 够付这个价**（`wallet >= price`）。

零效果经 `TurnEngine` 变成 `PlayerTurnOutcome::Nothing`，与批次 4 同一条纪律：
不 panic、不产出一条什么都不做的效果、不新增反馈键。

### 3.3 owner 校验**复用**批次 4 那一条，不另写一份（ADR 0021）

批次 4 把它落在 `crates/ll-sim/src/resolve/dialogue.rs` 的私有 `may_give_away`。
**本批把它整体搬到 `ll_sim::ownership`（`holder_owner` 的旁边）并公开**，
`give_item` 与交易两处**调同一个函数**。搬家的同时把入参从
`Option<WorldId>` 泛化成 `Owner`：

```rust
pub fn may_give_away(giver: Owner, owner: Owner) -> bool {
    match owner {
        Owner::Unowned => true,                       // 没有人的权益受损（效果文档原话）
        Owner::Player => giver == Owner::Player,
        Owner::Npc(id) => giver == Owner::Npc(id),
        Owner::Faction(_) | Owner::Shop(_) => false,  // 公产：本批一律拒（批次 4 第 4 条裁定）
    }
}
```

调用方一律用**既有的** `holder_owner(world, 卖方, 卖方 id)` 算出「卖方名下的东西长什么样」
再传进来。对赠送那条路径**行为逐条不变**：说话人是 NPC，`holder_owner` 给
`Owner::Npc(id)`（有 `remembered_id`）或 `Owner::Unowned`（无名 NPC ⇒ 任何 `Owner::Npc(_)`
都拒），与批次 4 那张表一模一样。唯一放宽的是「说话人恰好是玩家」这一格
（旧实现恒拒），而那条路径今天不可达（`interact_entries` 不列玩家自己）——泛化之后它是
**正确**的那一档，如实写在函数文档里。

**这是本批「复用而不是另写一份」的证据**：改坏 `may_give_away`，赠送侧与交易侧**同时红**
（反例验证 ①）。

### 3.4 占位价格公式

规格 5.3：「**价格 = 物品基础价 × 买家归属系数**，两个因子今天都有……
这不是简化版的经济系统，是**一个显式的占位公式**，要在代码里写明它将来会被
`agent-goals-and-economy.md` 三节的公式替换。」

```rust
// crates/ll-sim/src/trade.rs
pub const TRADE_PRICE_NEUTRAL_PERMILLE: i64 = 1000;   // standing = 0 时按基础价原价
pub const TRADE_STANDING_SWING_PERMILLE: i64 = 200;   // 声望满值最多便宜两成 / 敌对满值最多贵两成

pub fn trade_price(base: Milli, standing: i32) -> i64
```

- `base` 就是 `ItemAttrs::base_price`（`Milli`，**本批是它的第一个非哈希消费者**）；
  `Agent::wallet` 的单位是「最小货币单位」，`Milli` 的最小单位就是它 ⇒ **取 `base.0`，
  不取 `whole()`**：本体一份烤肉 `base_price: 900` 用 `whole()` 会变成 0，白拿。
- 系数 = `1000 - standing * 200 / Affiliation::STANDING_FULL`，
  `standing` 先夹到 `[-STANDING_FULL, STANDING_FULL]`；中间乘积用 `i128` 承接。
- 结果对 `1` 取下界：**基础价非零的东西不可能白拿**。基础价本身为 0 的东西价格就是 0
  （那是内容作者说它不值钱）。
- **两个方向同价，不设买卖差价**。见第十一节：差价是又一次玩法数值裁定，
  而同价恰好让「反复买卖」的净收益恒为 0——不设差价是**最不容易被套利的**那一档。

`standing` 从哪来：`ll_sim::trade::partner_standing(world, actor, partner)`——
说话人的 `Agent::home` → `FactionTable::faction_of` → `actor` 身上那条
`(Faction, OrgRef::Instance(势力))` 归属的 `standing`；任何一步查不到就取 `0`（中立原价）。
这条查询链与 `resolve::dialogue::join_settlement` 是同一条，**批次 3 让玩家第一次真的有了
一条 `standing`，本批是它的第一个读者**。

**买家系数用谁的 `standing`**：两个方向都用**玩家（`actor`）与对方势力**的那一条。
规格写的是「买家归属系数」，而玩家卖东西时买家是 NPC、NPC 对玩家没有 `standing` 这个量。
如实登记在第十一节。

### 3.5 产出的效果（四条，货币守恒）

```rust
vec![
    Effect::ConsumeInventoryItem { actor: 卖方, def: item, durability },
    merge_into_inventory_effect(买方, 买方 id, 交出的那一件（owner 由 holder_owner 算好）, items),
    Effect::AdjustWallet { actor: 买方, delta: -price },
    Effect::AdjustWallet { actor: 卖方, delta: price },
]
```

与批次 4 的 `give-item` 逐条同形，只多了两条钱。**归属由 `resolve` 算好写进搬运效果**
（`holder_owner`），不产 `Effect::TransferOwnership`——理由是批次 4 三节 3.5 那两张排法表，
本批一个字都不改地继承它。

### 3.6 交易消不消耗回合：**不消耗**（本批自裁，见第十一节第 1 条）

规格没说。**取「不消耗」**，理由三条：

1. **反转成本不对称**。加一条 `Effect::ScheduleNext` 是一行；而先落地「消耗」再改成
   「不消耗」，中间任何一份走过交易的存档/回放脚本都会带上时间轴的痕迹——规格九节 1
   点名的正是这条不对称（「落地之后再改会动回放摘要」）。**取不动时间轴的那一档。**
2. **交易屏是模态屏**，`Demo::advance` 在它开着时整个早退，世界一个字节不动。让它消耗回合
   会得到「屏开着时时间不走、一关屏 NPC 连跳 N 回合」的怪相。
3. 所有者裁定第 2 条说的是**对话**不消耗回合。交易不是对话，因此这一条不是「照抄裁定」，
   是本批自己裁的；但它与那条裁定同向，不制造新的不一致。

守卫：`交易不消耗回合` 一条独立测试（**每个新变体都要有自己的那一条**——批次 3 踩过的那个陷阱：
批次 2 那条「不消耗回合」只走 `set-flag` 一支）。`open-trade` 与 `Intent::Trade` **各算一条**。

---

## 四、C：NPC 初始钱包（所有者裁定第 4 条）

裁定原文（`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节第 4 条）：

> **NPC 初始钱包不能是 0**，按职业量级给、从据点人口派生。
> 现状 `ll_mod::roster::build_npc_agent` 写死 `wallet: 0` ⇒ **玩家只能买不能卖，交易落地即残废**。
> 触及货币守恒，落地时要在文档里写明取值理由。

### 4.1 两个因子各自的真相源

- **据点人口**：`SettlementSite::population`。`build_npc_agent` 今天拿不到它 ⇒
  给 `MaterializeContext` 加 `population: u32`。它「与哪一位无关」（物化按据点成批进行），
  正是那个结构体自己文档里写的那一束，与 `culture` 同一格。
  唯一构造点 `ll_game::settlement_spawn` 那里 `site.population` 就在手边。
  消费者当天就有（钱包派生）⇒ 过得了 `check_field_consumers.py`。
- **职业量级**：**不新增任何内容字段**，判据是**既有的** `SettlementRoles`——
  「哪个 `ContentIndex` 是管理者/铁匠/……」的真相源就是它，而 `build_npc_agent` 本来就收着它。

### 4.2 公式与取值理由

```rust
pub const NPC_WALLET_PER_RESIDENT: i64 = 100;   // 每位居民 100（最小货币单位）
pub const NPC_WALLET_TIER_STEWARD: i64 = 5;     // 管理者掌着据点的账
pub const NPC_WALLET_TIER_TRADER: i64 = 3;      // 有产出可卖的职业
pub const NPC_WALLET_TIER_DEFAULT: i64 = 1;     // 其余（守卫、民兵、以及尚无职业）

wallet = population as i64 * NPC_WALLET_PER_RESIDENT * 档位
```

量纲对照（本体 `mods/lostland/items.json5`，`base_price` 是 `Milli` = 最小货币单位）：
一份烤肉 900、一把武器 40000~55000。一座 40 人的据点：普通居民 4000（拿得出几份口粮的钱）、
铁匠 12000、管理者 20000（买得起一把武器，买不起两把）。**这是一个占位量级**，
与占位价格公式同批、同理由：写明它将来会被 `agent-goals-and-economy.md` 的经济系统替换。

「有产出可卖的职业」= `farmer`/`hunter`/`butcher`/`blacksmith`/`fisher`/`shepherd`/`mason`
（`SettlementRoles` 上那七个）。`guard`/`militia` 与未列出的落默认档。

### 4.3 通胀这笔账如实记在这里

规格 5.3 与所有者裁定都点了名：凭空发钱是通胀源，触及
`agent-goals-and-economy.md` 四节的「货币守恒与回收汇」。**本批的交易本身是守恒的**
（买方减多少、卖方就加多少，四条效果两两相消）；不守恒的只有**世界生成期这一次性发放**。
记在 P9 的账上，写进常量文档。

### 4.4 这会动 `EXPECTED_POPULATED_WORLD_DIGEST`

`Agent::wallet` 进 `WorldState::hash`（`crates/ll-world/src/state.rs` 的 `write_i64(agent.wallet)`）。
populated 那个基准世界里有 29 个 `Agent`，全部经 `build_npc_agent` 物化 ⇒ **必红，必重冻**，
走四步（① 确认基线红 → ② 把改动关掉、确认精确回到旧值 → ③ 恢复 → ④ 新常数在两个独立进程复现）。
另外两条基准预期不动（`EXPECTED_WORLD_DIGEST` 的世界零 `actor`；
replay 那条不经 `build_npc_agent`，见 `crates/ll-sim/tests/replay.rs` 该处注释），
**没红的给两组对照证伪**，照批次 29 十节 10.1 的写法。

---

## 五、D：交易屏与内容

### 5.1 屏

`ScreenState::Trade { partner: EntityId, cursor: usize }` + `crates/ll-game/src/trade_screen.rs`，
形状照抄会话屏（`ll_ui::screen::ScreenData` 的五个槽，不给它加第六个槽）：

| 槽 | 装什么 |
|---|---|
| `title_key` | `screen-trade-title`（字面量，与除会话屏外的每一块屏同办） |
| `rows` | 对方背包（买）在前、自己背包（卖）在后，各行带价 |
| `cursor` | 预选第一项 |
| `empty_key` | `screen-trade-empty` |
| `hint_key` | `screen-trade-hint` |

**顺序确定（约束 C5）**：两侧的 `inventory` 都是 `Vec`，全程线性扫描，不碰任何哈希容器
——玩家按的是「第几行」。

行文案走 `Catalog::resolve_with_args`（`{ $item }` / `{ $price }`），
物品名走既有的 `ll_ui::hud::item_display_name`。**不拼接字符串**。

### 5.2 内容

`mods/lostland/dialogues.json5` 的管理者开场白加一条选项
`text_key: "lostland:dialogue.steward.ask_trade"`、`outcomes: [{ kind: "open-trade" }]`、
`next: "end"`（交易屏压过 `next`，见 2.3；写 `end` 是让「不装交易屏的将来」也有一条自洽的退路）。

**零新增内容 id**：`text_key` 走 `parse_id` 不 intern，一个 `ContentIndex` 都不平移
（批次 29 十节 10.1 那条结构性理由原样成立）。

### 5.3 i18n

- `.ftl` 新键**一律加在两份文件末尾**（并行批次 `wt-uip2` 也在改这两份）：
  `dialogue-steward-ask_trade`、`screen-trade-title`、`screen-trade-empty`、
  `screen-trade-hint`、`screen-trade-buy`、`screen-trade-sell`。en 与 zh-CN 都要有。
- `crates/ll-ui/tests/i18n_text_width.rs` 分类表加规则
  （`screen-trade-buy` / `screen-trade-sell` → 参数化；其余三条落既有的 `screen-` 一行规则，
  `dialogue-` 那一条落既有的会话屏规则）。**那条 `dialogue-` 预算是全表最紧的几条之一**：
  新台词先量再写，红了改文案、不放宽规则。

---

## 六、ADR 0022 反例验证计划（每条先跑基线，再改坏，并确认红的原因是想验的那一条）

**本批点名必验的四条**：

| # | 改坏什么 | 预期红在哪 |
|---|---|---|
| ① | `ll_sim::ownership::may_give_away` 的 `Owner::Npc(id) => giver == Owner::Npc(id)` 改成恒真 | **赠送侧与交易侧同时红**——这就是「复用同一条校验」的证据 |
| ②a | `open-trade` 那条选项照样提交 `Intent` 并在 `resolve` 里补 `ScheduleNext` | `open-trade那一行不消耗回合` |
| ②b | `resolve_trade` 补一条 `Effect::ScheduleNext` | `交易不消耗回合` |
| ③ | `write_dialogue_outcome` 里 `OpenTrade` 的 `4` 改成 `JoinSettlement` 的 `1` | `后果种类不同的两个对话节点摘要不同` |
| ④ | 「买方钱够不够」那道闸门去掉 | `钱不够时买不成` |

另加：`open-trade` 那一支产出一条效果（验「不产 `Effect`」）、价格公式的系数方向搞反、
卖方那条 `ConsumeInventoryItem` 不产、两条 `AdjustWallet` 各去一条（验货币守恒）、
`build_npc_agent` 的钱包改回 `0`（验钱包测试与 populated 基准都真的咬着它）。

**四个形状主动防**：不用空 `Catalog`（端到端一律用真实 `assets/locales`）；
断言之前先断言对象存在；**每个新变体各有自己的「不消耗回合」那一条**；
验反例时确认红的**确实是**想验的那一条断言。

---

## 七、内容哈希、存档 schema、行数棘轮

- `CONTENT_HASH_ALGORITHM_VERSION`：递增（提交 1，`open-trade` 那一支 + 本体内容真的用上了它）。
- `CURRENT_SCHEMA_VERSION`：**预期不动**。`Agent::wallet` **本来就在存档主体里**
  （本批只改它的初值，不加字段），`MaterializeContext` 是物化期的临时结构、不进存档。
  由 `scripts/ci/check_save_schema_version.py` 判定，**照它的提示做，不绕过**。
  **不写任何未实测的兼容性声明**（postcard 非自描述，`#[serde(default)]` 在那条路上是空操作）。
- 行数棘轮：新代码尽量落在**新文件**（`crates/ll-sim/src/trade.rs`、
  `crates/ll-sim/src/resolve/trade.rs`、`crates/ll-game/src/trade_screen.rs`），
  快照内文件涨了就**先拆再 bless**，`--bless` 的 `reason` 必须写满。

---

## 八、提交划分（四个，中文提交信息；尽量让每个提交自身是绿的）

1. `feat: 对话后果 open-trade——不产效果，只推 UI`
2. `feat: Intent::Trade 与占位价格公式，owner 校验与赠送共用同一条`
3. `feat: NPC 初始钱包按职业量级与据点人口派生`（含 populated 基准重冻）
4. `feat: 交易屏与本体那一行交易选项`

---

## 十、落地实测

（收尾时填）

## 十一、规格没裁定、本批临时选的做法

（收尾时填）
