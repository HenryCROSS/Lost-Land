# 批次 14：势力播种（把编年史的占领链物化成 `OrgInstance`）

- **基线提交**：`d5e8bf1`（`main` 当前 HEAD），工作树 `wt-factions`，分支 `wt-factions`
- **改前测试基线**：`bash scripts/ci/run_tests.sh` → **2731 passed / 0 failed / `EXIT=0`**
- **裁定来源**：`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节第 **3** 条
  与紧随其后的「第 3 条的后果：对话的『加入』依赖势力播种」一小节。
  **本文档不重新论证那些裁定**，只落地。

## 〇、三个会漂移的常量：这里不列值

交接文档第〇节的纪律照办——本计划**不抄任何一个数值**。要看当前值，跑：

```bash
grep -rn "const EXPECTED_WORLD_DIGEST\|const EXPECTED_REPLAY_DIGEST" \
  crates/ll-world/tests/determinism.rs crates/ll-sim/tests/replay.rs
grep -n "pub const CURRENT_SCHEMA_VERSION" crates/ll-content/src/save_file.rs
```

---

## 一、这一批要做成什么

**不发明新机制**，只把编年史**已经在推演**的结果物化。原料现成（自己核实过的落点）：

| 原料 | 落点 |
|---|---|
| 据点建立 | `HistoricalEventKind::SettlementFounded(SettlementFoundedRecord { site, epoch, .. })` |
| 据点覆灭 | `HistoricalEventKind::SettlementAbandoned(SettlementAbandonedRecord { site, epoch, cause })` |
| **据点易主** | `HistoricalEventKind::SettlementConquered(SettlementConqueredRecord { site, epoch, conqueror, .. })` |
| 同族/异族占领概率 | `EpochRun::occupation_numerator`（`SAME_RACE_OCCUPATION_NUMERATOR` / `CROSS_RACE_OCCUPATION_NUMERATOR`，分母 `OCCUPATION_DENOMINATOR`，值自己 grep） |

**一条占领事件就是一句「谁统治谁」**：`conqueror` 这座据点所属的势力，从此多统治一座
`site`。把整部事件日志按时间折一遍，落下来的就是「一个势力下属多个据点」。

---

## 二、四个必须回答的设计问题

### Q1：势力从哪些据点长出来？「无势力」合法吗？

**答：每一次 `SettlementFounded` 都当场立一个势力；一座活着的据点恒属于且只属于一个势力。
「无势力的活据点」不合法。**

- 一座从未打过仗的孤立据点 → 它自己一个成员的势力（城邦）。它**不是**「拿据点 `WorldId`
  冒充势力」——势力有自己独立分配的 `WorldId`（≠ 据点的），有自己的成员表，会随占领长大
  或覆灭。所有者否掉的是「用据点的号当势力的号」，不是「小势力」。
- 否掉的另一条：**只让发生过占领的据点成势力**。那样绝大多数据点没有势力可加入，而对话
  「加入」这条后果的动机（所有者提出对话系统的原话「和据点的管理者对话加入」）就在多数
  据点上落空——把一个刚建立的机制立刻变成「大部分时候不可用」。
- 废墟（`SettlementStatus::Ruined`）不属于任何势力：它没有人。

### Q2：势力的身份怎么定？名字 / 文化 / 建立者种族从哪来？

| 身份项 | 来源 | 是否落成字段 |
|---|---|---|
| `id` | `WorldId::next(&mut chronicle.next_world_id)`——与据点、历史事件**同一个号段**（`identity-and-ids.md`，`OrgInstance::id` 文档「永不复用」） | 是（`OrgInstance.id`） |
| 首邑 `seat` | 立势力那一刻的那座据点；首邑覆灭而势力还有别的成员时，改指**现存成员里 `WorldId` 最小**的那座（确定性、与遍历顺序无关） | 是 |
| 文化 | 首邑的 `SettlementSite::culture`（`CultureKind`），空文化表时为 `None` | 是 |
| 建立者种族 | **不落字段**。`ll_world::culture::founder_race(cultures, culture, seat_id, seed)` 现算——与 `ll_mod::roster` 排名册、与 `occupation_numerator` 判同族用的是**同一个函数、同一条随机流**（`FOUNDER_RACE_STREAM_ID`）。落成字段就是本仓库最恨的「真相源之外的副本」 | 否（`FactionTable::founder_race_of` 现算） |
| **名字** | **不落字符串字段**。展示名由**文化的 `display_name_key`**（`CultureAttrs::display_name_key: NamespacedId`，已是 i18n 键，不是字面量）+ 首邑组合而成，走 `FactionTable::display_name_key(&CultureTable, faction)` 现算 | 否 |

**为什么名字不落字段**：`CultureAttrs::display_name_key` 已经是唯一真相源；把它拷进
`Faction` 就是第二份副本，而文化会随占领改变。同时这条让「不许硬编码用户可见字符串」
（`check_i18n_strings.py`）**不战而胜**：本批一个字面字符串都不新增。
真正的「卡拉克第三王朝」这类专名要等 `NamingRules` 接进文化表（今天 `NamingRules`
**全仓库零生产消费点**，接线是另一批），本批不预支。

`OrgInstance.def` / `OrgInstance.authored` 对播种出来的势力恒为 `None`（纯生成，
没有 mod 模板、没有 mod 具名定义）——保留这两个字段而不是绕过 `OrgInstance`，
是因为 mod 直接定义具体势力那条路（`identity-and-ids.md` 四）以后要从这里进来。

### Q3：据点 → 势力是不是一对一？

**是，严格一对一，且在类型上表示出来。**

- `FactionTable` 内部持有一份 `by_site: Vec<(WorldId /*据点*/, WorldId /*势力*/)>`，
  **按据点号升序、且据点号唯一**；`FactionTable::validate` 把「一座据点出现两次」判成
  构造错误（`FactionTableError::SiteRuledTwice`），读档路径也走这条校验。
- 查询走二分（`faction_of(site) -> Option<WorldId>`），不是 `HashMap`（约束 C5）。
- `Faction.members` 是升序去重的 `Vec<WorldId>`，与 `by_site` 互为倒排，
  `validate` 同时校验两者一致。

### Q4：势力被灭了怎么办？

**留下「已覆灭」记录，`OrgInstance` 不消失。**

```rust
pub enum FactionStatus {
    /// 还统治着至少一座活着的据点。
    Active,
    /// 最后一座据点在这个纪元被打没了/被别人占走了。
    Fallen { epoch: u32 },
}
```

三条理由，缺一条都不成立：

1. `OrgInstance::id` 的既有文档写死了「**永不复用**——王朝覆灭后历史事件仍要能解析回它」。
2. 玩家的归属是 `Affiliation { kind: Faction, org: OrgRef::Instance(WorldId) }`，
   而 `ll_content::remap::remap_affiliations` 对 `OrgRef::Instance` **不做重映射**。
   势力条目一旦消失，那条归属就成了指向空气的号码，而**没有任何东西会报错**。
3. 编年史本来就在记覆灭（`SettlementAbandoned`）——势力这一层跟着记，语义一致。

**这一条直接回答「玩家加入的势力被灭了会怎样」**：归属仍然解析得到，解析到的是一个
`Fallen` 势力。玩家是一个亡国之人，不是一个指针悬空的人。后续批次（对话/UI）读
`FactionStatus` 就能说出这件事。

**「势力被灭」在本批只有一种成因**：它统治的据点数归零（被铲平，或最后一座被别人占走）。
势力**不会**因为首邑被占就整体易主——占领事件只搬**一座**据点，这是事件本身的字面
语义（`SettlementConqueredRecord` 只记 `site` 一座）。首邑被占的势力若还有别的据点，
首邑改指现存成员里号最小的那座，继续存在。

---

## 三、播种算法（`WorldChronicle` 内，一次前向折叠）

编年史事件已经**按发生顺序**排好（纪元升序，同纪元内按候选点光栅序）。播种是对这个
`Vec` 的一次纯折叠，没有第二次排序、没有哈希容器、没有新的随机流：

```
for event in events （原序）:
    SettlementFounded { site, epoch } →
        新建 Faction { id: WorldId::next(counter), seat: site, culture: 该据点当时的文化,
                       founded_epoch: epoch, status: Active, members: [site] }
        by_site.insert(site → 新势力)

    SettlementConquered { site, epoch, conqueror } →
        let from = faction_of(site);  let to = faction_of(conqueror);
        if from == to: 无操作（自家城被自家人「占」——不可能，但不 panic）
        else:
            from.members 移除 site；to.members 插入 site（保持升序）
            by_site[site] = to
            若 site 曾是 from.seat 且 from 还有成员 → from.seat = from.members[0]
            若 from.members 空 → from.status = Fallen { epoch }

    SettlementAbandoned { site, epoch } →
        let f = faction_of(site)（覆灭的据点可能从没进过任何势力？不可能——
            每座据点必先有 Founded 事件）
        f.members 移除 site；by_site 移除 site
        若 site 曾是 seat 且还有成员 → seat = members[0]
        若 members 空 → status = Fallen { epoch }

    Kill → 与势力无关，跳过
```

### 确定性（C3 / C5）

- **C5**：全程只有 `Vec` + 二分 + 升序插入/删除，`Faction` 表按 `id` 升序（= 分配顺序），
  一个 `HashMap`/`HashSet` 都不碰。输入 `events` 本身的顺序是既有的确定性顺序。
- **C3**：**这一折叠里没有任何随机决策**——它是事件日志的纯函数。本批唯一的随机来源是
  **建立者种族**，它走既有的 `DetRng::for_entity(seed, FOUNDER_RACE_STREAM_ID, site_id)`
  （`ll_world::culture::founder_race`），本批**一个字都没改**那条流。
  「势力的生成必须走 `DetRng`」这条要求在本批的正确落地方式就是**不新开随机流**：
  硬塞一次掷骰进来只会让世界摘要多一个没有语义的自由度。这一条列进「临时选的做法」。

### `WorldId` 怎么分配

**继续用编年史自己的计数器 `EpochRun::next_world_id`**，与据点 id、历史事件 id 同一个
号段、同一个 `WorldId::next(&mut u32)` 惯例（`identity-and-ids.md` 三）。因此：

- 势力号与据点号**永不相等**——「拿据点号冒充势力」在号段层面就不可能发生；
- `WorldChronicle::next_world_id()` 自然把势力用掉的号算进去，
  `ll_game::world::build_world` 那句 `world.next_world_id = max(...)` 一个字不用改，
  游戏内的击杀记录不会撞号；
- **代价如实登记**：`next_world_id` 因此变大，`WorldChronicle` 的确定性测试与两条黄金
  基准都会动（见第五节）。

---

## 四、代码落点（文件 800 行 / 函数 50 行上限；新代码进新模块）

`chronicle.rs` 已 3104 行、`state.rs` 已 3745 行，**都是既有违规**。本批**不往这两个
文件里加算法**：

| 新增/改动 | 文件 | 说明 |
|---|---|---|
| **新** `Faction` / `FactionStatus` / `FactionTable` / 折叠算法 / 单元测试 | `crates/ll-world/src/faction.rs` | 新模块，本批主体 |
| `OrgInstance` 派生 `serde` | `crates/ll-world/src/entity/org.rs` | 见下 |
| `WorldChronicle` 多一个 `factions: FactionTable` 字段 + `factions()` 取值器 + 在 `generate` 里调一次折叠 | `crates/ll-world/src/chronicle.rs` | **只加十来行接线**，算法在 `faction.rs` |
| `WorldState.factions: FactionTable` + `WorldStateRepr` 对应字段 + `hash()` 里一段 | `crates/ll-world/src/state.rs` | 同上，写哈希的那段抽成 `faction.rs` 里的 `write_hash` |
| 建档时把编年史的势力表搬进世界 | `crates/ll-game/src/world.rs` | 紧挨 `world.next_world_id = max(...)` 那一行 |
| `CURRENT_SCHEMA_VERSION` 递增 + `--bless` 刷新形状快照 | `crates/ll-content/src/save_file.rs`、`scripts/ci/save_body_shape.json` | 见第五节 |

### `OrgInstance` 要派生 `serde`——这推翻它自己的模块文档

`org.rs` 今天写着「不派生 `serde`：`def` 里的 `ContentIndex` 依赖 mod 加载顺序、
不可持久化」。**那条理由在今天只剩一半**：`ContentIndex` 早已补齐了无上下文的直接
`Serialize`/`Deserialize`（`affiliation.rs` 的 `OrgRef::Def(ContentIndex)` 就在派生），
「是否已注册」的解析留给拿到注册表之后的调用方。所有者已裁定「`OrgInstance` 进入存档，
因为被占领后肯定会有变化的」。

**照办的方式是改文档、不是删文档**（交接纪律）：原段落重写为「为什么现在可以派生了」，
并如实写明播种出来的势力 `def`/`authored` 恒为 `None`，因此 `ContentIndex` 的跨会话
稳定性问题在**当前**没有实例——真正 mod 定义势力那天要补 remap，账记在字段文档里。

---

## 五、两条硬闸门的走法

### 5.1 黄金基准重冻——四步，一步都不能少

`WorldState.factions` 进 `hash()` ⇒ `EXPECTED_WORLD_DIGEST` 与 `EXPECTED_REPLAY_DIGEST`
**极可能都要重冻**。四步证据全部写进提交信息：

1. **确认基线红**：改完实现、还没改常量，跑这两个测试，确认它们**真的失败**，并记下
   实际算出来的新值。
   *若某条没红*，**不许直接放行**——本会话的先例是「基线没红是因为那个测试的世界里
   根本不存在这类对象」，要先补一个真的有据点/占领的固件再判。
2. **把改动关掉，确认精确回到旧值**：把 `hash()` 里新增的那一段用
   `if !world.factions.is_empty()` 之外的手法整段注掉（同时保留其余全部改动），
   重跑，确认摘要**精确等于旧常量**。这一步才是真正的证据——它排除掉「其实是别的
   改动顺手平移了索引」。
3. **恢复**那一段。
4. **两个独立进程复现新值**：连跑两次（分别独立启动 `cargo test`），两次算出同一个数
   才写进常量。

### 5.2 存档形状——照提示做，不绕过

`WorldState` 多一个字段 ⇒ `python scripts/ci/check_save_schema_version.py` 会红。照它
给的两步做：`CURRENT_SCHEMA_VERSION` **3 → 4**，再跑 `--bless` 刷新
`scripts/ci/save_body_shape.json`。

**老存档不写迁移**（交接第〇之二第 9 条已裁定）：走既有的「版本不对就明确拒绝」路径
（`check_schema_version`）。端到端证据：一条测试写一个 `schema_version = 3` 的存档头，
断言读档返回明确的版本不符错误、而不是静默乱读。

新字段放在 `WorldStateRepr` **末尾**（postcard 按声明顺序定位，插在中间会静默错位——
门禁头注释点名的那条）。

---

## 六、本批**不做**的事（写清楚挂载点，不预支）

- **不做对话的「加入」那一支**。挂载点已经勘明：
  `knowledge/design/dialogue-system.md` **5.1 节**——它今天写的是「变通：拿
  `SettlementSite::id` 当 org」。本批落地后，那一节的变通**作废**，正确写法变成
  `Affiliation { kind: Faction, org: OrgRef::Instance(那座据点所属**势力**的 WorldId), standing: … }`，
  查法是 `world.factions.faction_of(npc.home_site)`。
  同节记着的另外两笔账（`Agent.home: Option<WorldId>` 要从 `NpcProfile.home` 搬运、
  `standing` 加入据点给 +250 满值 1000）**都不在本批**。
  本批交付后要回去改 5.1 节，把「今天没有任何东西可以加入」改成「有了」。
- **不碰 `Owner::Faction` 的现有语义**。物品归属那批刚落地、今天零构造点。
  **衔接说明写在 `ll_world::ownership` 的 `Owner::Faction` 字段文档里**：本批之后它
  第一次有真实的势力号可指（`FactionTable` 里的 `Faction::id`），但**本批不构造任何
  `Owner::Faction`**——谁来构造是「据点财产归属」那一批的事。
- 不做智能体经济、人口模拟、Cohort LOD（规格 §15 明确留 P9）。
- 不新增 example target（ADR 0030 / `check_no_examples.sh`）。

---

## 七、ADR 0018 反例验证清单

每条新断言都要用**故意改坏实现**的方式验证真的会红，然后改回。至少这几条：

| 断言 | 故意改坏的方式 |
|---|---|
| 占领会把据点搬进征服者的势力 | 把 `SettlementConquered` 那一支改成无操作 |
| 一座据点只属于一个势力 | 让搬运时**不**从旧势力移除 |
| 最后一座据点没了 → `Fallen` | 把 `Fallen` 那一支改成保持 `Active` |
| 势力号与据点号不相交 | 把势力 id 改成直接复用 `seat` 的号（正是所有者否掉的那条） |
| 势力表进世界哈希 | 见 5.1 第 ② 步（整段注掉必须精确回到旧常量） |
| 老存档被明确拒绝 | 把 `check_schema_version` 的比较改成恒 `Ok` |

---

## 八、提交切分

1. `feat: OrgInstance 派生 serde，势力表与占领链折叠落新模块 faction.rs`（含单元测试）
2. `feat: 编年史产出势力表，世界状态持有并进哈希与存档`（含 schema 升版 + `--bless` + 黄金基准重冻，四步证据写在提交信息里）
3. `docs: 对话 5.1 变通作废、Owner::Faction 衔接、交接账目更新`
