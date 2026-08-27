# 批次 1：移动与占位

**基线**：`b3ab694`　**分支**：`wt-moveoccupancy`　**日期**：2026-08-27

来源：`knowledge/handoff/2026-08-27-session-handoff.md` 第三节「批次 1：移动与
占位」。所有者已裁定四条，合成一批做——它们是同一个决定面，落在同两个文件
（`ll-sim/src/turn.rs`、`ll-sim/src/resolve.rs`）的同几十行上，拆开必然返工。

**基线测试数（本分支实跑，2026-08-27）**：`bash scripts/ci/run_tests.sh` exit 0，
105 个测试目标、2364 项通过、0 失败。

> ## ⚠ 范围变更（2026-08-27，正文写就之后）
>
> **裁定 1（种族默认敌对）已从本批次拆出，本文档第三节的 D1/D2/D3、第四节的
> 4.1 与 4.2、第六节的第 3/4/8/9 条测试**全部作废，正文原样保留不删改。
> 本批次实际交付的是**裁定 2/3/4**，落在提交 `c02ffe4`。
>
> **拆出的原因**（按发现顺序）：
>
> 1. 正文 D1 说得对——`declared_hostile` 的短路闸门让裁定 1 按字面写是空操作。
>    但 D2「同族豁免」的处理方式是错的：`race-system.md:267`-`269`
>    **明确否决**过「给关系派生基线加一条同种族 +X／异种族 −Y 的常量项」，
>    理由是常量对丢掉了不对称性；替代方案是把种族态度挂在**文化**上
>    （`race-system.md:271` 的 `CultureDef.race_affinity`）。正文 4.1 提出的
>    `RaceDef.hostile_to_all` 正是被否决的那个形状。
> 2. 而这条通路**已经落地了**：`crates/ll-world/src/culture.rs:189` 的
>    `CultureAttrs.hostility` 是文化 → 文化的有向敌意分（`0..=MAX_HOSTILITY`，
>    `culture.rs:94` 值为 7），`CultureTable::hostility`（`:369`）刻意不对称；
>    `mods/lostland/cultures.json5:150`-`153` **已经写好了哥布林的敌意内容**
>    （goblin_warband → mining_hold 6、farmstead 4、stonecutters 4；反向
>    mining_hold → goblin_warband 只有 3）。新开 `RaceDef` 字段会造出第二个
>    「谁跟谁不对付」的真相源。
>
> **拆出的代价是零**：裁定 1 撤走后，正文 4.2 整节（新 trait、`ResolveCatalogs`
> 加字段、15 处字面量构造点、生产装配）与 4.3 的「两处调用点转发 catalogs」
> **全部不再需要**——`declared_hostile` 签名一个字未改，路由也就不需要目录。
> 裁定 2/3/4 之间不存在对裁定 1 的代码依赖。
>
> **裁定 1 的新形状**见后续批次计划：NPC 物化时从 `SettlementSite::culture` 挂
> 一条 `Affiliation{Culture, OrgRef::Def}`（`Agent::affiliations` 的第一个生产
> 者，因此**会动世界摘要**），玩家与其余无归属实体走一条新的本体文化
> `lostland:cultureless`，`declared_hostile` 改读 `CultureTable::hostility`
> 超阈值。该批次顺带解掉交接文档第四节第 1 条与规格 §15 P8 行的第一条硬前置。

---

## 一、四条裁定的字面内容

1. **玩家没有默认势力归属，但有的种族默认敌对所有生物。** 给 `RaceDef` 加一个
   内容字段（`hostile_to_all: bool`），让 `declared_hostile` 读它。会动内容哈希
   版本，要递增并补说明段落。
2. **玩家优先度高于 NPC，只有玩家可以互换位置。** 推翻已落地的双向互换。
3. **`resolve_move` 在目的地区块非常驻时静默作废——不改。**
4. **「每格至多站一人」升级成真正被强制的不变式。**

---

## 二、动手前必须知道的既有事实（已逐条 grep 核实，2026-08-27）

| 事实 | 证据 |
|---|---|
| `declared_hostile(a,b) = (has_faction(a) \|\| has_faction(b)) && is_hostile(a,b)` | `crates/ll-sim/src/ai_query.rs:182` |
| `Agent::affiliations` 至今零生产者，两条构造路径都写死空列表 | `ai_query.rs:32`、交接文档第二节末 |
| `route_move_into_occupant` 被 `advance_ai`、`try_player_intent` 共用 | `turn.rs:370`、`turn.rs:445`、定义在 `turn.rs:543` |
| 两个调用点**都已经收着** `catalogs: &ResolveCatalogs<'_>`，只是没往下转发 | `turn.rs:344`、`turn.rs:439` |
| `resolve_move` 里一行占位检查都没有 | `crates/ll-sim/src/resolve.rs:3976` 起 |
| `resolve_move` 现有分支序：Surface 检查 → `terrain_at` 非常驻静默作废 → `opens_into` 开门 → `blocks_move` 撞墙 → 真正移动 | `resolve.rs:3980` / `3997` / `4002` / `4016` / `4038` |
| `Arena::iter_with_id` 由 `Vec` 支撑，遍历序与哈希无关（约束 C5） | `crates/ll-world/src/entity/arena.rs:144` |
| `place_roster` **已经**保证不摞人（`occupied` 集合 + `continue`） | `crates/ll-game/src/world.rs:728`、`:747` |
| `spawn_player` 无显式检查，但建局时是第一个 `actors.spawn`，天然安全 | `crates/ll-game/src/world.rs:305`、`:468` |
| `resolve_exit_space` 直接产 `Effect::MoveTo{pos: anchor}`，**不查锚点是否有人** | `resolve.rs:4952`-`4967` |
| `CONTENT_HASH_ALGORITHM_VERSION` 现 27；**没有任何测试钉住这个数字**，纯人工纪律 | `crates/ll-mod/src/content_hash.rs:805`、`:803` |
| 新字段进哈希必须递增；`ItemDef.furniture`（版本 24）是同构先例 | `content_hash.rs:760`-`782` |
| 字段门禁决策层 = `crates/ll-sim/src/*.rs`（非递归）+ `ll-world/src/{fov,light}.rs`，判据是全文正则 `\.字段名` | `scripts/ci/check_field_consumers.py:185`-`192`、`:432` |
| `RaceDarkvisionSource` 先例：trait 定义在 `ll-sim`、`RaceTable` 实现、方法名**故意**与字段同名以让门禁天然命中 | `crates/ll-sim/src/vision.rs:56`-`68` |
| `ResolveCatalogs` 字面量构造 **15 处**（生产 1 + 测试/示例 14），另有 1 处用 `..ResolveCatalogs::empty()` | `grep "ResolveCatalogs {"` |
| `advance_ai` 调用点 40 处 / `try_player_turn`+`try_player_intent` 21 处 | `grep` |
| 本体种族现有 **4 个**：human/dwarf/elf/goblin；`BaseRaceIds` 只有前三个字段 | `mods/lostland/races.json5:138/165/186/232`、`crates/ll-mod/src/race.rs:554` |
| 三对一惯例：击杀经验/击杀计数/尸体 def 读 `creature_kind.unwrap_or(race)`，只有任务进度读裸 `race` | `resolve.rs:1581` / `:1714` / `:1823` vs `:1613` |

---

## 三、设计裁定（技术负责人裁定，均需所有者复核，见第八节）

### D1：`declared_hostile` 的短路闸门必须开口，否则裁定 1 是空操作

裁定 1 的字面落点「让 `declared_hostile` 读 `hostile_to_all`」**按现有实现落不了地**：

```rust
// 现状
(has_faction(a) || has_faction(b)) && is_hostile(a, b)
```

`affiliations` 零生产者 ⇒ 玩家与哥布林双方 `has_faction` 均假 ⇒ **整个表达式短路
成 `false`**，新字段读了也没用。唯一能让裁定真正生效的形状是把种族敌对**提到闸门
之前**：

```rust
pub fn declared_hostile(a: &Agent, b: &Agent, races: &dyn RaceHostilitySource) -> bool {
    if race_declares_hostile_to_all(a, b, races) { return true; }
    (has_faction(a) || has_faction(b)) && is_hostile(a, b)
}
```

对称：玩家撞哥布林是攻击，哥布林撞玩家也是攻击。

### D2：`hostile_to_all` 不对**同族**生效

字面读「敌对所有生物」会让哥布林营地当场内战——哥布林 NPC 之间随机游走撞上
就互相攻击，而文化批次刚落地的「哥布林部落攻灭矮人矿城」编年史依赖哥布林能
聚居。判据因此是：

> 声明了 `hostile_to_all` 的一方，对**除自己同类之外**的所有生物敌对。

这与 `is_hostile`「同势力即友」是同一条结构（种族在这里充当隐式势力）。**这是
本批次风险最高的一条自裁定**，一行可反转，见第八节第 1 条。

### D3：查询键用 `creature_kind.unwrap_or(race)`，不是裸 `race`

跟随四条路径里三条的多数惯例（`resolve.rs:1581/1714/1823`）。裸 `race` 那一条
（`:1613`）是交接文档已记录的**缺陷**，不是可效仿的范式。本批次不顺手修那条
缺陷——那是独立的一处改动，不在四条裁定范围内。

### D4：只动 `declared_hostile`，不动 `is_hostile` / `nearest_hostile` / 哥布林行为树

裁定原文的落点就是 `declared_hostile`（「我走进你这一格意味着什么」）。
`is_hostile`（「野怪该扑向谁」）在 `affiliations` 零生产者的现状下**已经**对每一
对实体返回真，哥布林本来就会主动扑向所有人——不需要也不应该在本批次改它。
`native_behavior.rs` 一个字不动。

### D5：NPC 撞非敌对 = 移动失败，**消耗一次行动**

与撞墙同口径（`resolve.rs:4016` 分支）：不产 `Effect::MoveTo`，仍产
`Effect::ScheduleNext`。理由：

- 效果非空 ⇒ 不触发 `perform` 的 `guarantee_progress` 补跑 `Wait`，不存在重复计费。
- 与「撞墙也消耗时间」这条既有裁定一致，NPC 不会因为前面站了人就白赚一回合。

### D6：占位检查落在 `resolve_move` 的开门分支**之前**

即紧跟 `terrain_at` 查询成功之后。若放在开门分支之后，会出现「门那一格站着人 →
先把门推开、消耗一回合 → 下一回合才发现人挡着」这种两回合才识破的怪异结果。
不变式的字面意思不区分目的地是门还是平地。

裁定 3 明确不改的 `terrain_at` 非常驻静默作废分支**排在占位检查之前，顺序不动**。

### D7：占位检查**不**过滤 `current_space`

进了 Interior 的 Agent 其 `pos` 仍指向地表锚点格（`resolve.rs:3980` 的不变式），
因此会「幽灵占用」那一格。这是**既有行为**——`place_roster` 的 `occupied` 同样
不过滤（`world.rs:728`），当前的撞格路由也不过滤。本批次保持一致，不在此处引入
第二套判据。见第八节第 3 条。

### D8：不变式的作用域 = `resolve_move`；`resolve_exit_space` 是记录在案的剩余缺口

`resolve_exit_space`（`resolve.rs:4952`）自行构造 `Effect::MoveTo` 回锚点，不查
占位。堵它需要先裁定「退出时锚点有人，人去哪」，而那条与所有者给的附带设计输入
（作弊传送时把 NPC 随机挪到旁边一格）是同一个未落地的机制。本批次**不堵**，但要：

- 在 `resolve_exit_space` 文档里写明这条缺口与它等待的那个裁定；
- 加一条**钉住当前行为**的测试，让将来堵它的人立刻看到自己改了什么。

**不允许**在文档里声称不变式「全局强制」——它只在移动路径上强制。

---

## 四、实现形状

### 4.1 内容侧（`ll-mod` / `mods`）

1. `crates/ll-mod/src/race.rs`：`RaceDef`（:114）、`RaceAttrs`（:194）、
   `RaceView`（:260）各加 `hostile_to_all: bool`；`RaceTable` 列式存储（:301）加
   一列 `Vec<bool>`；`define`（:325）resize 默认 `false` + 写入；`get`（:369）
   组装时带上。
2. `crates/ll-mod/src/content_schema.rs`：`RawRace`（:296）加
   `#[serde(default)] hostile_to_all: bool`；`apply_races`（:332）塞进 `RaceAttrs`。
3. `mods/lostland/races.json5`：**只有 `lostland:goblin` 写 `hostile_to_all: true`**，
   human/dwarf/elf 三族不写（`serde(default)` 即 `false`）。哥布林条目自己的注释
   已写着「本体名册里第一条『打了不太划算』的可遭遇种族」「不给武器，因为本体
   武器全属于文明那一侧」——它就是这个字段的目标。补一段注释说明这条裁定。
4. `mods/example_mod/races.json5`：按「示例 mod 两张都写」的既有惯例，给一个
   种族写显式值，作为「mod 作者能声明这个字段」的活证据。
5. `crates/ll-mod/src/content_hash.rs`：`write_race_fields`（:1329 起）末尾追加
   `hostile_to_all` 的混入（形状照 `ItemDef.furniture` 那条布尔的既有写法，
   **先去读它当前怎么写的，逐字对齐**）；`CONTENT_HASH_ALGORITHM_VERSION`
   **27 → 28**，并按 `content_hash.rs:760`-`782` 的既有段落格式补一段「版本 28」
   说明，写清「既有表多了一个字段，不是新增内容表」。

### 4.2 依赖倒置接口（`ll-sim`）

6. 新 trait，照 `crates/ll-sim/src/vision.rs:68` 的 `RaceDarkvisionSource` 逐条对齐：

   ```rust
   pub trait RaceHostilitySource {
       /// 给定种族索引，返回它是否默认敌对所有非同类生物；未注册的索引返回
       /// `false`——查不到就是查不到（ADR 0015）。
       fn hostile_to_all(&self, race: ContentIndex) -> bool;
   }
   ```

   **方法名必须字面叫 `hostile_to_all`**：字段门禁按正则 `\.hostile_to_all` 在
   `ll-sim/src/*.rs` 里搜，同名即天然命中，不必写豁免。先 grep 确认这个名字在
   `TARGET_TYPES` 覆盖的其余结构体里不存在（避免 `stat_modifiers` 那种撞名假阳性）。
   放在 `ai_query.rs` 还是新开一个小模块，按 `vision.rs`/`character.rs` 的粒度自行
   判断，但要在模块文档里写明依赖方向的理由。

7. `crates/ll-mod/src/race.rs`（或照 `xp_curve.rs` 的先例另起落点）：
   `impl RaceHostilitySource for RaceTable`。

8. `crates/ll-sim/src/catalogs.rs`：`ResolveCatalogs` 加
   `pub race_hostility: &'a dyn RaceHostilitySource`；补空实现
   （`NoRaceHostility`，恒 `false`）与 `empty()` 里的常量，照 `NoExperience`
   （`experience.rs:141`）逐条对齐。**15 处字面量构造点**逐一补一行。

9. `crates/ll-game/src/content.rs:329` `as_resolve_catalogs`：接
   `&self.content.race_table`。

### 4.3 判定与路由（`ll-sim`）

10. `ai_query.rs:182` `declared_hostile` 按 D1 加参数与新分支，D2 的同族豁免、
    D3 的 `creature_kind.unwrap_or(race)` 查询键都在这里落地。函数文档要整段
    重写：现有文档大段论证「双方都没有归属就不敌对」，D1 之后那句话不再无条件
    成立。
11. `turn.rs:543` `route_move_into_occupant` 加两个入参：种族敌对源，以及**发起者
    是不是受控实体**。分支变成：
    - 目的地空 → 原样 `Move`
    - 已声明敌对 → `Attack`（玩家与 NPC 都是，不变）
    - 非敌对 **且** 发起者是玩家 → `Swap`
    - 非敌对 **且** 发起者是 NPC → 原样 `Move`，交给 `resolve_move` 的占位检查
      判成失败（D5）

    「是不是玩家」的判据用 `world.player_entity == Some(actor)`——`resolve_move`
    与 `resolve_swap` 里判 `MarkExplored` 用的就是这个（`resolve.rs:4060`、`:4141`），
    不新开第二套「谁是受控实体」的表示法。
12. `turn.rs:370`/`:445` 两个调用点转发已有的 `catalogs`。
13. `turn.rs:527`-`531` 那段「本实现读作双向都成立……两个 NPC 因此……而是换位」
    的文档**整段过期，必须重写**，不是删掉了事——要写清所有者推翻它的裁定。

### 4.4 占位不变式（`ll-sim`）

14. `resolve.rs:3976` `resolve_move`：按 D6 在 `terrain_at` 成功之后、`opens_into`
    之前插入占位检查。查找逻辑与 `route_move_into_occupant` **必须共用同一段代码**
    （抽一个私有帮手），否则两处判据会各自漂移，出现「路由认为 A 挡路、占位检查
    认为 B 挡路」的不一致。命中时按 D5 返回只含 `Effect::ScheduleNext` 的效果。
15. `resolve.rs:4952` `resolve_exit_space`：按 D8 补文档 + 钉住当前行为的测试。

---

## 五、会变红的清单（侦察已预判，实现者必须逐条处理）

1. **`crates/ll-mod/tests/example_mod_rogue_passives.rs:569`-`578`**——两条
   `assert_eq!(inspects + moves, turns)` 恒等式。卫兵（NPC）贴身非敌对目标后不再
   产 `MoveTo`/`SwapPositions`，恒等式左边会小于 `turns`。
2. **`crates/ll-mod/tests/example_mod_stealth.rs`**——同构的恒等式断言
   （计数逻辑在 `:679`-`687`）。

   **这两条必须改成表达新的三态现实（巡查 / 移动 / 被挡住），不许把断言放松成
   不等式了事。** 放松断言等于把这批改动的可观测后果从测试里抹掉。
3. **`crates/ll-sim/src/turn.rs` 内 `route_move_into_occupant` 的四条单元测试**
   （`:933`/`:957`/`:984`/`:1001`）——签名变化导致编译不过；其中「同一势力的两人
   撞格互换」这条的语义在 D2/D5 之后需要重新裁定它测的到底是玩家还是 NPC。
4. `crates/ll-game/tests/bump_into_occupant.rs` 两条用例走的都是**玩家**路径
   （`advance_ai` 只用来把玩家条目弹进 `pending`，喂的是恒 `Wait` 的空转闭包），
   **不会**因 D5 变红。但本批次要给它**补**用例，见第六节。

---

## 六、必须新增的测试（每条都要按 ADR 0018 用「故意改坏」验证真的会红）

1. NPC 撞非敌对 NPC：不互换、不移动、仍推进时钟（D5）。
2. 玩家撞非敌对 NPC：仍然互换（裁定 2 只收紧 NPC 那一侧，玩家这侧不许回归）。
3. 玩家撞哥布林：即便双方都没有 `affiliations`，判定为**攻击**（D1 的活证据，
   也是裁定 1 唯一能被外部观察到的后果）。
4. 哥布林撞哥布林：**不**互相攻击（D2 的反例；这条一旦变红说明同族豁免被拿掉了）。
5. `resolve_move` 目的地有人时不产 `MoveTo`（D6，占位不变式本身）。
6. 目的地是**关着的门且门上站着人**：判成撞人失败，**不**产 `SetTerrain`（D6 的
   分支顺序证据）。
7. `resolve_exit_space` 锚点被占时的当前行为（D8 的缺口钉子）。
8. `mods/lostland` 四条种族的 `hostile_to_all` 取值逐条钉住
   （照 `crates/ll-mod/tests/base_mod_races.rs` 的既有断言风格）。

---

## 七、黄金基准

侦察给出的证据链（实现者仍必须实跑验证，不许照抄）：

- **`EXPECTED_WORLD_DIGEST`（`crates/ll-world/tests/determinism.rs:231`，
  `17_228_492_522_544_021_674`）**：该测试全程零 `actors.spawn`，世界生成不碰
  移动结算。**预期逐位不变。**
- **`EXPECTED_REPLAY_DIGEST`（`crates/ll-sim/tests/replay.rs:862`，
  `14_731_332_643_995_045_404`）**：`setup`（`replay.rs:45`-`48`）刻意把敌人放在
  远离玩家路径的 `(20,20)`，`play`（`:243`）直接 `resolve`+`apply`、**根本不经过
  `TurnEngine`**，占位检查在这条回放里恒走「目的地无人」分支。**预期逐位不变。**

**若任一摘要真的动了，走四步重冻，一步都不能少**：① 确认基线红 → ② 把改动关掉、
确认**精确**回到旧值 → ③ 恢复 → ④ 新常数在**两个独立进程**里复现。四步证据写进
提交信息。

内容哈希不同：`CONTENT_HASH_ALGORITHM_VERSION` 27 → 28 是**本批次刻意为之**的
改动，不是重冻，按第 4.1 节第 5 条补说明段落即可。

---

## 八、待所有者裁定（做完汇报，不阻塞实现）

1. **D2 的同族豁免**是本批次风险最高的自裁定。字面裁定是「敌对所有生物」，实现
   取的是「除同类外的所有生物」，否则哥布林营地内战。一行可反转。
2. **裁定 2 的副作用：卫兵贴身后会每回合白撞。** `direction_toward`
   （`ai_query.rs:106`）任何距离都返回方向，从不因「已相邻」停手；D5 之后卫兵贴身
   非敌对目标会永远撞、永远失败、永远消耗回合。这正是交接文档第四节第 12 条那个
   悬而未决的问题（「要不要给『靠近』加『已相邻就不再挪』」）——本批次**不做**，
   因为它没有被裁定，但裁定 2 把它从「手感像跳舞」升级成了「NPC 原地空转」。
3. **D7 的幽灵占位**：进了建筑的 NPC 会永久占住它的地表门格。既有行为，但不变式
   强制后第一次变成硬阻挡。
4. **D8 的剩余缺口**：`resolve_exit_space` 仍可造出两人同格。堵它要先裁定「退出时
   锚点有人，人去哪」。
