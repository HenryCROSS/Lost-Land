# 击杀与死亡记录：历史事件系统的特化

**冻结于** 2026-08-19。核对提交 `1b4094e`（`main` 分支，876 测试全绿）。

> **【2026-08-30 复核：下面这条「落地状态」已过期，正文原样保留。】** `Effect::Kill` 今天有**三个**字段（`target`/`killer`/`cause`），`crates/ll-sim/src/effect.rs:91`；`KillRecord`/`KillCause` 已落地在 `crates/ll-world/src/history.rs:283`/`:312`；`Agent.creature_kind` 在 `crates/ll-world/src/entity/agent.rs:530`；事件写入走 `Effect::RecordHistoricalEvent`（`crates/ll-sim/src/effect.rs:132`）。逐条见 [2026-08-29 文档—代码一致性审计](../audit/2026-08-29-doc-code-audit.md) 一节第 1 条。

**落地状态**：纯设计，`crates/` 中无任何对应类型。已核实：`Effect::Kill`（`crates/ll-sim/src/effect.rs`）只有 `target: EntityId` 一个字段；`resolve_attack`/`resolve_use_skill`（`crates/ll-sim/src/resolve.rs`）在目标生命值降到零或以下时产出它；`KillCount` 任务条件（`crates/ll-mod/src/quest.rs`）已借用 `Agent::race` 当"敌人类型"，其模块文档「跨表引用」一节已如实记录这是简化，不是精确解；`append_quest_kill_progress`（`crates/ll-sim/src/resolve.rs`）已经示范了本文档要复用的模式——**在 `apply` 销毁目标之前，`resolve` 阶段读取它的信息、追加更多 `Effect`**。`HistoricalEvent` 作为一个概念已被[命名、改名与本地化](naming-and-localization.md)使用（改名产生一条 `HistoricalEvent`），但至今没有任何文档给出它的正式字段——本文档顺带把这个信封定型，供 `Kill` 这个变体使用。

---

## 一、为什么是历史事件的特化，不是独立日志

玩家杀了一只哥布林、一个 NPC 杀了另一个 NPC、王朝更替时的一场战死——**这是同一件事在不同规模下的三个实例**：谁杀了谁、用什么杀的、在哪、什么时候。区别只在于「值不值得被记住」，不在于「记录的形状该是什么」。

### 被否决的方案：独立战斗日志（`BattleLog`）

一个直觉方案是给战斗系统单开一张日志表，专门记录攻击/命中/击杀这类高频事件，与[世界历史生成](world-history.md)的事件日志分开维护。**否决**，理由：

- **本项目已经在"同一个概念被独立定义了两次"上栽过两次跟头**：[0010](../decisions/0010-single-source-of-truth-for-daylight.md) 记录 `is_daylight()` 与光照曲线曾是两套互相矛盾的白昼判定，四小时窗口内结论相反；[0014](../decisions/0014-season-pure-function-derivation.md) 把季节收敛为纯函数派生正是为了不重蹈这个覆辙。战斗日志与历史事件日志若分开存，「玩家杀了某个具名 NPC」这件事会同时出现在两处——一处更新漏了同步，就会出现"传说浏览说他还活着，战斗日志说他已经死了"这类静默不一致，与白昼判定的教训是同一个根因。
- **下游消费者会被迫适配两套数据源**：血仇（[社会系统](society-and-affiliation.md)）、任务 `KillCount`（[职业/技能树/副职/任务系统](class-skill-quest-system.md)）、传说浏览（世界历史生成）都需要查询"谁杀了谁"。若这份信息分裂成两张表，每个消费者都要先判断"这次要查哪一张"，或者两张都查再去重——白白多出一层永远可能出错的协调逻辑。
- **矮人要塞式"每个事件各开一张专表"正是[世界历史生成](world-history.md)「六、事件日志」一节已经否决过的模式**（"矮人要塞的传说文本能到 GB 级，是因为它记录每一次出生、死亡、婚姻"）——本文档只是把这条否决延伸到"战斗"这一个具体事件种类上，不是重新论证一遍。

**结论**：击杀/死亡走同一条 `HistoricalEvent` 管线，作为它的一个 `kind` 变体，与建城、战争、王朝更替、改名共用同一个信封、同一份存储、同一套查询接口。

---

## 二、事件信封与 `Kill` 载荷

### 信封（首次定型，供其余变体对齐）

```rust
/// 一条历史事件的通用信封。城市建立、战争、王朝更替、改名、击杀……
/// 共用这一个结构，`kind` 携带各自的载荷。
pub struct HistoricalEvent {
    /// 永久标识，供跨引用与传说浏览查询——与势力/家族/聚落共用
    /// `WorldId` 空间（`identity-and-ids.md`「类型/实例分离」定案表
    /// 已把「历史事件」列入 `WorldId` 一侧）。
    pub id: WorldId,
    /// 发生时刻。历史生成阶段「年」粒度的事件按世界历史生成文档
    /// 「三层时间粒度」一节换算落到具体 tick，不是两套时钟。
    pub at: Tick,
    /// 发生地点。部分事件种类（例如改名）地理意义弱，仍然保留这个
    /// 字段——统一形状比"部分变体缺一个字段"更值得。
    pub location: TorusPos,
    pub kind: HistoricalEventKind,
}

pub enum HistoricalEventKind {
    SettlementFounded { .. },   // 见 world-history.md，字段留给该文档后续补
    War { .. },
    DynastyChange { .. },
    Rename { old_name: String, new_name: String }, // naming-and-localization.md
    Kill(KillRecord),           // 本文档新增
}
```

### `KillRecord`：项目所有者要求的"怎么杀的"

```rust
pub struct KillRecord {
    /// 谁杀的。环境/坠落/饥饿致死时没有"谁"，为 `None`。
    pub killer: Option<WorldId>,
    /// 被杀的是谁。只有进入「全记」档位的死亡才会走到这里——见三。
    pub victim: WorldId,
    /// 用什么杀的。
    pub cause: KillCause,
    /// 致命一击的数值。
    pub killing_blow: KillingBlow,
    /// 死亡那一刻的状态。
    pub victim_state: VictimState,
}

/// "怎么杀的"——武器/技能/地形/坠落/饥饿，项目所有者点名要求的字段。
pub enum KillCause {
    /// 近战。`weapon` 为 `None` 表示徒手。
    Melee { weapon: Option<ContentIndex> },
    /// 技能击杀，指向 `SkillDef` 注册表。
    Skill { skill: ContentIndex },
    /// 地形致死（熔岩、深渊……）。
    Terrain { kind: TerrainKind },
    Fall,
    Starvation,
    /// 持续伤害类致死（`buffs-and-triggers.md` 的 `on_turn_start` 持续
    /// 伤害触发到生命值归零）。
    Poison,
    /// mod 扩展死因，走注册表而不是给 Rust 枚举反复加变体——与
    /// `KillCause` 其余变体已经封闭的核心死因并存，是"本体即 Mod"在
    /// 死因这个小角落的应用。
    Environmental(ContentIndex),
}

pub struct KillingBlow {
    /// 这一下造成的伤害量。
    pub damage: i32,
    /// 致命一击结算后的剩余生命值（通常 ≤ 0，允许记录过量伤害）。
    pub remaining_health: i32,
}

/// 死亡时的状态标记，定宽位标记，成本可以忽略不计。
pub struct VictimState {
    pub poisoned: bool,
    /// 结算时同一 tick 内是否有 2 个以上攻击者对其造成过伤害——
    /// "被围攻"的一个可判定近似，不追求叙事上的精确围攻定义。
    pub surrounded: bool,
}
```

**克制**：没有加"击杀者当时的血量""目标的完整战斗历史"这类字段——项目所有者要"精确"指的是"这一下怎么打死的"，不是把整场战斗的每一次交手都塞进一条记录。需要完整战斗过程时，查询这段时间窗口内该实体参与的全部 `HistoricalEvent`（若都被记录）即可重建，不需要在单条 `Kill` 记录里预先塞进整场战斗。

---

## 三、分级记录与量级估算

`world-history.md`「事件日志」一节已经定死预算：**只记可被引用的事件，约一万条**，且这个数字对应的是**历史生成阶段**（500 年模拟）的静态产出。玩家落地后的实时游玩会持续产生新的击杀，这是一个不同时间窗口的独立增量，不能直接套用同一个"一万条"上限，也不能放任不管——下面给出分级规则与对应的量级。

### 分级规则

| 档位 | 规则 | 产出 |
|---|---|---|
| **玩家相关** | 玩家杀的、杀玩家的、玩家随从的击杀/死亡 | 全记 `HistoricalEvent::Kill`，不论对方是否具名 |
| **具名 NPC 相关** | 击杀双方至少一方是"具名"（已被赋予 `WorldId` 的历史人物，见五「敌人类型」一节前的身份问题） | 全记 |
| **无名小卒之间** | 双方都不具名的前景层战斗 | 不产事件，只累加进死因统计聚合（见四） |

前景层规模本身天然有界（[社会系统](society-and-affiliation.md)「LOD 兼容性」一节：被模拟集合受 LRU 上限约束），背景层不跑个体决策、势力/野怪的批量冲突结算走闭式公式（[世界历史生成](world-history.md)「核心架构判断」）——**背景层从不产出 `Effect::Kill`**，这一点直接把"无名小卒对无名小卒"的爆量风险挡在了源头，不需要靠事后抽样再补救。

### 量级估算

以一次典型的百小时游玩为参照：

- **单场遭遇战**：前景层参战单位通常 3–10 个（受 LRU 上限约束），产出 1–6 条 `Effect::Kill`。
- **一局游戏（约 100 小时游玩，遭遇战按每 15–20 分钟一次估算，约 300–400 场）**：
  - 玩家相关（全记）：玩家平均每场遭遇战杀死约 1.5 个目标，加上玩家自身死亡与随从战损，估计 **500–800 条**。
  - 具名 NPC 相关（全记）：具名人口本身是稀疏且有界的一批（历史生成阶段约一万五千名历史人物是"被记住"的上限量级，实际存活并可能死亡的具名 NPC 只是其中一个子集），估计 **50–300 条**。
  - 无名小卒之间：不产事件，计入统计聚合，**不占用事件日志的存储预算**。
  - 合计单局新增约 **600–1200 条**，与历史生成阶段的一万条相加，总量级停在 **一万到一万二千条**左右——仍然是"一万条"这个数量级，不需要重新和既有预算谈判。

### 存储量

`HistoricalEvent` + `KillRecord` 的字段按整数打包估算约 50–90 字节/条（`WorldId` 4 字节 × 2、`KillCause` 判别式 + 载荷约 5–8 字节、`TorusPos`/`Tick` 各 8 字节、`KillingBlow` 8 字节、`VictimState` 1 字节）。一万二千条 × 80 字节 ≈ **960 KB**；若走 `serde_json` 文本序列化（本项目存档格式的既有选择），膨胀 2–4 倍，估计 **2–4 MB**——与 `world-history.md` 对一万条事件"微不足道"的结论同一数量级。

### 被否决的方案：朴素全记（不分级）

若"NPC 之间"这一档也全记（不区分是否具名），前景层里所有战斗都会产出永久事件——势力冲突、野怪清剿这类玩家未参与的战斗若也持续跑在前景层，百小时游玩里可能达到数万场交手、**数万到十万条事件**，直接击穿既有预算，且绝大多数是"某个从未被命名的哥布林杀死了另一个从未被命名的哥布林"这类没有人会去传说浏览里查阅的记录，稀释了"可被引用"这个筛选标准本身的意义。分级不是性能妥协，是延续 `world-history.md` 已经立住的判断：**"值得记录"本身就是筛选依据，不是懒得记。**

### 长期游玩的老化

若玩家实际游玩时长远超 100 小时（数百到上千小时也并不罕见），"玩家相关全记"这一档会持续线性增长。**建议**给事件日志加一个软上限（例如默认保留最近若干千条完整 `Kill` 记录），超出窗口的低优先级记录（无名小卒相关、非转折性的玩家击杀）降级为统计聚合，具名相关与剧情钉住的事件永久保留。这与 [0009](../decisions/0009-derive-by-default-store-only-deviation.md) 记忆槽"定容 + LRU 淘汰"是同一个思路的又一次复用——一条记录从"完整事件"降级为"一个数字"，而不是被直接丢弃。本文档只给出这个方向，具体窗口大小属于后续实现的数值调参，不在设计范围内。

---

## 四、死亡统计：存还是现算

**事件是"发生了什么"，统计是"累计了多少"**——两者形状不同，存储方式也不该相同。

### 结论：按对象拆开，不是整体二选一

| 统计对象 | 存法 | 理由 |
|---|---|---|
| **具名个体的击杀数/连杀/存活时长** | 在该实体身上存几个小整数字段（`kills_total`、`current_kill_streak`、`longest_kill_streak`、`spawned_at`） | 具名个体数量有界（几百到几千），字段定宽，成本可忽略；且这些量需要"连续状态"（连杀在无事件记录的击杀之间也要累加），无法单纯从事件流现算——见下 |
| **死因/种族/职业分布这类全局或聚落级聚合** | 在聚落 × 死因（或种族 × 死因）维度存计数器，每次结算批量刷新 | 与「职业声望局部化」（`society-and-affiliation.md`）同一量级（几百聚落 × 几十死因类别 ≈ 几千条），走 `Effect::SetScriptState` 的全局命名空间存储即可，不需要新的持久化机制 |
| **某个具体人物的死亡细节（供传说浏览展示）** | 直接读它的 `HistoricalEvent::Kill` 记录 | 不是统计，是查询——见七 |

### 为什么不能整体走"现算"

若把死亡统计设计成"完全从事件流现算，零存储"（对齐 [0009](../decisions/0009-derive-by-default-store-only-deviation.md) 的一贯做法），会撞上一个具体障碍：**无名小卒之间的死亡根本没有对应的事件条目**（三节的分级规则本就不记）。现算需要遍历的输入本身就不完整，算出来的"总死亡数""死因分布"会系统性漏掉这部分——不是遍历成本的问题（一万多条事件遍历一次是毫秒级），是**数据源本身有缺口**。

因此死亡统计不能整体套用"默认派生，只存偏差"（这是该原则第十二次被考虑复用，但第一次因为"数据源不完整"而不适用，不是因为规模）——`0009` 文档「适用条件」一节列的前提是"默认值可以从种子 + 已知上下文确定性重算"，而无名小卒的死亡是**真实发生但选择不留痕迹**的事件，不是"可以从公式反推"的量，两者性质不同。聚落级聚合计数器因此是必要的存储，不是懒得设计现算的妥协。

### 具名个体统计为什么也不能纯靠"从事件流现算"

即便某个具名 NPC 只杀无名小卒（对方不具名，不产生事件），他自己的 `kills_total`/连杀计数仍然要涨——因为分级规则的第二档要求"具名 NPC 相关全记"，这个 NPC 作为击杀者一方本身已经让这条记录进入了"全记"档位（他是具名一方）。真正会漏计的场景是"具名个体杀了另一个也不具名，且这条判定被简化为按对象而非按事件双方计算"这种边界——**本文档的分级规则按"参与双方中至少一方具名"判定"全记"（见三），因此只要击杀者具名，这条记录必然被记，具名个体的 `kills_total` 从其自身作为 `killer` 的历史事件条数现算是准确的**。这里保留独立字段的理由是**读取效率**（连杀这类连续状态如果每次查询都要回放整段事件历史,成本会随游玩时长增长，而独立字段是 O(1) 读取），不是数据完整性问题——与聚落级聚合的"必须存"不是同一个理由。

---

## 五、`Effect::Kill` 如何携带来源信息

### 扩展字段

```rust
pub enum Effect {
    // ...
    Kill {
        target: EntityId,
        /// 谁杀的。环境/坠落/饥饿致死时为 `None`。
        killer: Option<EntityId>,
        /// 怎么杀的。
        cause: KillCause,
    },
    /// 落盘一条历史事件——本文档新增，`apply` 从
    /// `WorldState.next_world_id` 分配一个 `WorldId`，把 `event` 追加进
    /// `WorldState.history`。
    RecordHistoricalEvent {
        event: HistoricalEvent,
    },
    // ...
}
```

`killer`/`cause` 都是 `EntityId`/枚举/`ContentIndex` 这类朴素值类型，不含引用、闭包或裸指针——与 [`Effect::SetScriptState`] 携带的 `Vec<ScriptStateWrite>` 同一个纪律：`Effect` 本身不要求像时间轴队列（约束 C2）那样满足"只装 actor id + 行动类型 id + 参数"的字面形状（C2 字面上约束的是 `TimelineEntry`，不是 `Effect`），但精神是一致的——**新增字段必须是朴素数据，不能是需要在 `apply` 之外解引用的东西**，`killer: Option<EntityId>` 满足这一点。

### 决策在 `resolve`，不在 `apply`（约束 C1）

是否要产出 `RecordHistoricalEvent`（对应三节的分级规则）这个判断**不能**放进 `apply`——`apply` 的三条纪律里有一条是"不含任何游戏逻辑"，分级本身就是游戏逻辑。正确的位置是 `resolve` 阶段，与 `append_quest_kill_progress` 完全同构：

```rust
// 镜像 append_quest_kill_progress 的既有模式（resolve.rs）——
// 在 apply 销毁 target 之前，读取它与 killer 双方是否具名，决定产出
// 哪些追加 Effect。
fn append_kill_history(
    world: &WorldState,
    effects: &mut Vec<Effect>,
) {
    let kills: Vec<(EntityId, Option<EntityId>, KillCause)> = /* 从 effects 里已有的 Effect::Kill 提取 */;
    for (target, killer, cause) in kills {
        let victim_named = world.actors.get(target).and_then(|a| a.remembered_id);
        let killer_named = killer.and_then(|k| world.actors.get(k)).and_then(|a| a.remembered_id);
        if victim_named.is_some() || killer_named.is_some() {
            // 全记：产出 RecordHistoricalEvent（需要 victim/killer 都已
            // 具名才能拿到 WorldId 填充 KillRecord；一方不具名时，
            // KillRecord.killer 或本条记录本身如何处理不具名的一侧，
            // 属于实现期需要拍板的细节，本文档只定分级判据）
        }
        // 无论是否全记，死因统计聚合（四节）总是累加。
    }
}
```

**必须在 `apply` 之前读取**：目标一旦被 `apply` 里的 `world.actors.despawn(target)` 销毁，`race`/`remembered_id`/所属家族这些字段就再也读不到——这与 `append_quest_kill_progress` 文档「必须在 apply 之前读取被击杀者的 race」一节是同一条纪律，本文档不是新发明，是复用。

### 一个尚未存在的字段：`remembered_id`

上面的伪代码用到 `Agent.remembered_id: Option<WorldId>`——这个字段目前**不存在**。`identity-and-ids.md`「类型/实例分离」定案表只把 `WorldId` 分配给"势力、家族、聚落、宗教团体、历史事件"，没有覆盖"历史人物/具名 NPC"这一类个体。但 `world-history.md`「族谱必须有界」一节已经描述了同一个概念的雏形（"只有当某个旁支成员做了值得记的事，他才被追认为历史人物，补进族谱"）——本文档的判断是：**这个缺口需要 `identity-and-ids.md` 未来修订时正式纳入"历史人物"这一类到 `WorldId` 空间**，本文档不越界改写那份文档，只指出消费端（"判断一个实体是否具名"）需要这个字段存在，具体归属留给该文档的后续修订。

**为什么不给每个 `Agent` 都发一个 `WorldId`**：那等于让"被记住"这个本该几乎零成本的轴（`society-and-affiliation.md`"被记住……成本便宜……可以有几百万个"）背上一个"必须分配全局唯一递增 ID"的负担，与"背景 NPC 零存储现算"的设计前提冲突。`remembered_id` 应该只在实体首次"值得被记住"的那一刻才被赋值（出生进历史家族族谱、被玩家收为随从、成为任务发布者……），懒分配，不是每个 `Agent` 出生时的必填项。

---

## 六、"敌人类型"歧义怎么解

`crates/ll-mod/src/quest.rs` 模块文档已经如实记录：`QuestCondition::KillCount.target_kind` 目前借用 `Agent::race` 匹配"敌人类型"，导致"击杀 3 个哥布林"与"击杀 3 个哥布林种族的玩家角色"共用同一个索引。击杀记录要做到"精确"，这条歧义必须一并解决。

### 判断：需要一张小注册表，但不需要一整套新体系

**不需要**：一整张覆盖所有生物/敌人的复杂分类体系（子种、变种、稀有度……）——当前没有任何用例需要这种深度，YAGNI。

**需要**：一张独立于 `RaceDef` 的 `CreatureKindDef` 小注册表（几十条：哥布林、狼、亡灵……），加一个新字段：

```rust
pub struct Agent {
    // ... 既有字段
    /// 生物类型，用于击杀匹配与死因统计分类。`None` 时退回 `race`
    /// （见下）。绝大多数"有种族意义"的智慧类人型（玩家、NPC）不需要
    /// 设置这个字段——只有专门的"怪物"内容需要。
    pub creature_kind: Option<ContentIndex>,
}
```

**匹配规则**：`creature_kind` 有值时用它，否则退回 `race`——不破坏现有测试（本体夹具里的"哥布林"已经是把 `race` 设成 `lostland:goblin` 的怪物，退回规则保证它们继续按原样匹配）。真正需要精确区分的场景（"哥布林种族的玩家角色"不应被"击杀哥布林"任务算作命中）此时会因为玩家角色没有设置 `creature_kind`、退回 `race` 后仍然会被匹配——**这个残余歧义需要额外一条规则**：只有 `Agent` 不是玩家角色、不是任何具名历史人物（即 `remembered_id.is_none()`）时，才允许"退回 `race`"这条路径生效；一旦一个实体是具名的人形角色，必须显式设置 `creature_kind` 或干脆不参与"敌人类型"匹配。这条附加规则把"哪些实体算作可被 `KillCount` 计数的敌人"这件事，与"是否具名"这个本文档已经在用的轴对齐，不是另开一条判据。

### 为什么不是"更省的办法"（否决项）

考虑过的更省方案：**不新增字段，只是在任务系统查询侧加一条"若目标是玩家角色则跳过"的特判**。否决：这只堵住了"哥布林种族的玩家角色"这一个具体案例，任何其余用 `race` 表达"这是个怪物"的场景（例如未来的死因统计想按"哥布林/狼/亡灵"分类，而不是按"种族"分类）仍然没有一个干净的字段可用，迟早要为下一个类似需求重新讨论——与 `buffs-and-triggers.md`"如果现在只为一个需求写专用特判，下一个需求来了就要再加一个"是同一个模式，应当避免。

---

## 七、与既有系统的接口

### 血仇——不新增写入通道，等关系记忆偏移落地后接上同一个触发点

`society-and-affiliation.md`"杀了土匪，其兄弟所属家族对你的声望归零，不需要写复仇系统"——这套机制的运转依赖"个体记忆偏移"（关系系统"默认派生，只存偏差"三层结构里的第三层），而**关系系统本身仍是纯设计，未落地**（`README.md`"落地状态速览"已经如实标注）。本文档不越界替它设计存储结构，只指出触发点：`append_kill_history`（五节）已经是"resolve 检测到击杀、批量追加效果"的统一位置，未来关系记忆偏移的写入 `Effect`（尚不存在）落地时，应该在同一个函数里、同一次 `resolve` 批次内一并产出，而不是另开一条独立的"击杀触发血仇"通路——这与 `RecordHistoricalEvent`、死因统计聚合三者共享同一个触发点、各自写各自的存储，是同一个模式的第四个消费者。

### 任务 `KillCount`——继续用既有的计数机制，只换匹配依据

`kill_progress_effects`（`crates/ll-sim/src/quest.rs`）已经用 `Agent.script_state` 的全局命名空间存了一份独立的击杀计数器，**这条计数路径本来就没有从事件流现算**（它是 resolve 阶段直接对 `Effect::Kill` 计数），本文档不改变这一点。唯一的改动是 `target_kind` 的匹配依据从"总是 `race`"换成"`creature_kind` 优先，否则 `race`"（六节）。任务计数、死因统计聚合、`HistoricalEvent` 记录三者因此是**同一个 `resolve` 触发点驱动的三个平行消费者**，各自独立存储，不是一套计数机制服务三个用途,也不是三套计数机制各自扫描一遍——这个形状本身也回答了项目所有者"任务如何从事件流读，而不是另起一套计数"的问题：现状（也是本文档维持的现状）是**任务计数从来不是从事件流读的，是与事件流并列产出的**，两者共享触发点、不共享存储，硬要让任务计数改成"从 `HistoricalEvent` 里现算"反而是新增一条依赖——任务计数需要覆盖无名小卒（分级规则里明确不产出事件的那一档），若改成依赖事件流，"击杀 3 个哥布林小怪"这类最常见的任务反而会因为哥布林小怪不产生事件而永远算不出进度,这是本文档拒绝"任务从事件流现算"的直接理由。

### 传说浏览——复用查询式 API，不新增查询范式

`identity-and-ids.md`「脚本 API 必须是查询式」一节已经定了范式："查询：玩家所属家族参与过的所有战争"这类按 `WorldId` 反查关联事件的接口。传说浏览要展示"某历史人物死于何时何地、被谁所杀、用什么杀的"，直接是同一种查询的另一个实例——按 `victim: WorldId` 或 `killer: WorldId` 过滤 `WorldState.history` 里 `kind` 为 `Kill` 的条目即可，不需要新的查询范式，也不需要给 `Kill` 事件单独开一条展示逻辑之外的检索通道。

---

## 八、阶段归属

[世界历史生成](world-history.md)本身在 **P7** 位置（规格 §15 阶段表，[2026-08-18 规格修订] 后的编号）。但击杀/死亡记录的消费端——`resolve`/`apply` 战斗结算管线——是 **P3 已经落地并测试覆盖**的代码，每一次 `Intent::Attack`/`Intent::UseSkill` 命中致死都已经在产出 `Effect::Kill`。这意味着：

- **历史生成阶段产出的战死事件**（王朝战争战死、显赫人物死亡）确实要等 P7 世界历史生成器落地才能真正出现。
- **玩家游玩期间的击杀历史事件**在技术上不需要等 P7——只要 `WorldState.history`/`WorldId` 分配器存在，`resolve` 侧的接线（五节）今天就可以做。`world-history.md`「阶段归属」一节已经把"事件日志 ID 空间预留"列为三件"必须提前做"的事之一——本文档的分析进一步说明了**为什么这件事的紧迫性比原文档描述的更高**：P3 的战斗系统已经在日常产出 `Effect::Kill`，若 `history`/`WorldId` 容器不提前留出，P3 只能继续维持"击杀只销毁实体、什么都不记"的阉割状态，直到 P7 才补上——这段窗口期里死亡统计、血仇触发点、传说浏览全部无法运作，不是"暂缓一个可选功能"，是一条主线玩法（战斗）与另一条主线玩法（历史/传说）之间出现了功能断档。

### 必须提前留的接口

| 接口 | 归属 | 理由 |
|---|---|---|
| `Effect::Kill` 扩展 `killer`/`cause` 字段 | P3 代码，可直接改，不依赖 P7 | 是最小改动，`apply` 分支同步更新即可 |
| `WorldState.history: Vec<HistoricalEvent>` + `WorldId` 分配器 | 存档格式 P5 冻结之前 | 与 `society-and-affiliation.md`"必须在 P3 就预留的字段"一节同一个理由——P5 之后加字段需要写迁移链 |
| `Agent.creature_kind: Option<ContentIndex>`、`Agent.spawned_at: Tick` | 同上，P5 之前 | 同上，晚加要迁移 |
| `Agent.remembered_id: Option<WorldId>`（或等价标记） | 同上；正式归属留给 `identity-and-ids.md` 未来修订 | 五节已论证的消费端依赖 |

### schema 迁移问题

`WorldState.history` 若在 P5 就以空 `Vec` 的形态预留（字段存在，内容为空），后续新增 `HistoricalEventKind` 的枚举变体（例如本文档新加的 `Kill`）属于"内容层面的扩充"，不是"容器结构变化"——只要该枚举走可扩展的 serde 表示（未知变体不 panic，降级处理），不构成需要走存档迁移函数链的破坏性变更。**但若 `history` 字段本身拖到 P7 才加进 `WorldState`，就是 P5 之后新增字段，必然要走一次真正的存档迁移**——这不是本文档的新发现，是 `world-history.md`"事件日志 ID 空间预留"这条"必须提前做"的事在本文档的具体场景下又验证了一次紧迫性。

### 与确定性哈希的关系（[0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md)）

`WorldState.history`、新增的 `Agent` 字段（`creature_kind`/`spawned_at`/`remembered_id`）、死因统计聚合（走 `script_state`，已经在 `WorldState::hash()` 覆盖范围内，见 0022"实例二"）——**任何影响玩法的新字段落地时都必须同步确认它进了 `WorldState::hash()`**，否则确定性回归测试会在这部分状态上空跑。本文档不实现代码，这里只是提前记下这条检验，避免真正落地时被遗漏。

---

## 相关文档

- [世界历史生成](world-history.md) —— 历史事件系统的既有设计，本文档的特化对象，"只记可被引用的事件"预算的原始定义
- [身份与 ID 空间](identity-and-ids.md) —— `WorldId`/`ContentIndex` 分野，本文档指出"历史人物"这一类尚未被纳入 `WorldId` 定案表的缺口
- [社会系统：归属、文化、聚落与地图结构](society-and-affiliation.md) —— 血仇的既有派生机制、LOD 三档、"必须在 P3 就预留的字段"同类判断
- [职业/技能树/副职/任务系统](class-skill-quest-system.md) —— `QuestCondition::KillCount` 的注册与图校验
- [增益与通用触发器](buffs-and-triggers.md) —— `on_kill`/`on_death` 触发点（mod 可扩展效果），与本文档的历史事件记录是同一个 `Effect::Kill` 之下的两个平行消费者
- [三轴战斗结算](combat-three-axis.md) —— `resolve_attack` 现状与伤害结算管线
- [0009 — 默认派生，只存偏差](../decisions/0009-derive-by-default-store-only-deviation.md) —— 死亡统计存储方案的适用性判断依据
- [0010 — 白昼判定与光照曲线收敛为同一份真相源](../decisions/0010-single-source-of-truth-for-daylight.md)、[0014 — 季节纯函数派生](../decisions/0014-season-pure-function-derivation.md) —— "不做独立战斗日志"的先例依据
- [0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) —— 新字段必须计入 `hash()` 的检验
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) —— 规格 §15 阶段表
