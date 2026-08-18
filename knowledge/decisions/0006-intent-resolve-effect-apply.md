# 0006 — Intent → resolve → Effect → apply 单向数据流

**日期**：2026-08-16（规格定稿）/ 2026-08-17（P3 陆续落地）
**状态**：部分生效——`Intent`、`Timeline`、`Effect`、`apply` 已实现；`resolve` 尚未实现
**关键提交**：596ef14（规格首次写下该架构）、05459cd（确定性 RNG，支撑 C3）、485ea4b（Intent 输入层）、f65579a（时间轴调度器）、4bdd87d（Effect 与 apply）
**影响范围**：`ll-sim`（intent/timeline/effect/apply）、`ll-world::WorldState`、规格 §4

## 背景

游戏要同时满足三件互相牵制的事：模式 3 自由读档（存档必须是完整世界状态，无隐藏状态）、mod 脚本沙箱（脚本不能直接改世界）、确定性重放（同一种子 + 同一操作流必须在任何机器上产出逐位相同的结果，参见 [0002](0002-integer-only-world-state.md)）。三者对「谁能改世界、怎么改」这件事提出的要求高度重合，规格 §4 把它们收敛成一条单向数据流。

## 决定

**`apply` 是全局唯一能修改世界的地方。**

```
玩家输入 ─────────────────→ Intent ─┐
                                     │
随从/敌人 行为树 (.scm) ───→ Intent ─┼─→ resolve(&WorldState, Intent) -> Vec<Effect>
                                     │      纯函数 · 只读世界 · 可并行 · 出错降级
Mod 注册的技能 / AI ───────→ Intent ─┘                    │
                                                          ↓
                                            apply(&mut WorldState, Effect)
                                              唯一写入口 · 单线程 · 无逻辑 · 极短
```

四个概念：

- **`WorldState`** — 完全可序列化、不含指针/引用/闭包/trait 对象的纯数据。
- **`Intent`** — 「我要向东走」「我要对 (x,y) 释放 3 号技能」，纯数据，可序列化，玩家与 AI 都只产出它。`crates/ll-sim/src/intent.rs`。
- **`resolve`** — `fn(&WorldState, Intent) -> Vec<Effect>`，纯函数，只读世界，命中判定/伤害公式/技能效果都在此。**尚未实现**，留给后续批次。
- **`Effect`** — 「实体 7 生命 −12」「实体 3 移动到 (5,9)」这类纯数据描述。`crates/ll-sim/src/effect.rs`。
- **`apply`** — `fn(&mut WorldState, effect: &Effect)`，唯一写入口，单线程，不含任何游戏逻辑。`crates/ll-sim/src/apply.rs:37`。

## 为什么是这个形状

**硬性约束驱动了具体写法，不是审美选择：**

- **C3（确定性）**：所有随机性必须来自按实体 ID 派生的确定性流，`rng = hash(世界种子, 实体ID, 该实体事件计数)`，禁止全局 RNG 流。05459cd 的提交说明写得很直接：全局随机流的取值取决于「谁先取」，一旦并行结算或读档后处理顺序有细微变化，整条序列就会错位，世界走向另一个平行宇宙——这会同时摧毁自由读档与确定性重放。三个输入用 splitmix64 逐级混合而非异或，因为直接异或会让 `(种子=1,实体=2)` 与 `(种子=2,实体=1)` 得到同一条流。
- **C4（离屏推进有界）**：后台推进世界必须推进到确定时刻 T，不得「能跑多少跑多少」，否则玩家思考时长会污染世界状态。这条约束直接影响了 `apply` 的实现边界（见下节 `ScheduleNext` 的例子）。
- **脚本沙箱**：脚本只能返回 `Effect`，物理上无法触及世界——呼应 [0001 — Steel 沙箱能力实测](0001-steel-sandbox-verification.md)，中断可以掐断脚本，但「脚本能不能碰到世界」这件事要靠架构而不是靠沙箱本身。
- **多线程不阻塞**：`resolve` 只读，可直接 `rayon` 并行；`apply` 极短不成瓶颈。

## 落地过程中的两处真实取舍

**`Effect::ScheduleNext` 只写 `Agent::next_action_at`，不碰时间轴队列。** `Timeline` 是 `ll-sim` 侧的运行期调度缓存，不放进 `WorldState`（避免 `ll-world` 反向依赖 `ll-sim`）。真正把实体重新排入队列是调用方在 `apply` 返回之后另行要做的事。代价：调用方多一步责任，但保住了「`apply` 是唯一写入口」这条不变式不被时间轴队列的更新绕过。

**`health` 字段选 `BTreeMap<EntityId, i32>` 而非 `HashMap`。** `apply.rs` 新增该字段时（4bdd87d）直接引用了 C4：键序必须确定，不能让哈希桶序参与任何逻辑。

**`Intent::OpenDoor` 的 `pos` 字段用裸 `(i32, i32)` 而非 `TorusPos`。** `TorusPos` 唯一构造路径需要世界尺寸做取模归一化，而 `Intent` 产生的那一刻未必已经拿到世界（例如输入层只知道按键，不知道世界尺寸）——交给 `resolve` 读到 `Intent` 时再归一化，是刻意把类型收紧的时机推迟到有能力做的地方。

## 被否决的选项

1. **在 `resolve`/AI/脚本里就地修改 `WorldState`（传统命令式写法）**——否决：无法保证多线程安全（`resolve` 并行时谁写谁读会冲突），脚本沙箱形同虚设（脚本能直接碰到世界），重放不可靠（写入时机与顺序不再由单一入口保证）。
2. **让 `resolve` 也写一部分「顺手」的状态（例如自己更新时间轴队列）**——否决：写入口一旦分散，就无法再保证「`apply` 是唯一写入口」这条不变式。`ScheduleNext` 的例子正是这个否决的落地：即便多写一步很方便，也宁可让调用方多做一步，不让 `apply` 之外的代码碰 `WorldState`。
3. **`Effect` 累积状态用 `HashMap`**——否决，见上节 C4 与 `BTreeMap` 的选择。

## 后果

- **截至本 ADR 写下时，管线还缺 `resolve` 这一段**：`Intent`、`Timeline`、`Effect`、`apply` 均已实现并有测试覆盖，但还不能从真实 `Intent` 推导出 `Effect`——目前只能手工构造 `Effect` 测试 `apply`。整条管线要等 `resolve` 落地才闭环。
- **`ll-sim` 对 `ll-world` 的依赖方向曾经走反过一次**（见 [0004](0004-two-layer-entity-storage.md) 「一次架构方向错误及其修正」），根源正是这条数据流涉及的模块（实体存储、Intent、Effect）在早期没有想清楚该归哪个 crate。这不是这条架构决定本身的缺陷，但提示了「单向数据流」在纸面上清晰，落到 crate 边界时仍需要额外一轮校验。
- **`Effect` 目前只覆盖 `MoveTo`/`Damage`/`Kill`/`ScheduleNext` 四种**（`effect.rs:23`），技能、任务、经济等后续系统要接入这条管线时，每新增一种 `Effect` 变体都要重新问一遍：这个变化是否只能通过 `apply` 落地，有没有绕过的捷径。
