# 核心数据流：Intent → resolve → Effect → apply

**冻结时间**：2026-08-17，核对提交 `7a126f5`。

## 现状先说清楚：这条管线只有一半已经落地

写作本文档时，`crates/ll-sim/src/` 下的文件是：

```
apply.rs   effect.rs   intent.rs   lib.rs   timeline.rs
```

**没有 `resolve.rs`**。`crates/ll-sim/src/lib.rs` 顶部的模块文档写得很明确：

> 时间轴调度器（`timeline`）、`Intent`（`intent`）、`Effect`（`effect`）与唯一写入口
> `apply::apply` 均已实现；`resolve`（从 `Intent` 结合世界状态产出 `Effect`）是批次 C 的内容。

也就是说，这条管线目前是：

```
Intent  ✅ 已实现（ll-sim::intent::Intent）
  │
  ▼
resolve ❌ 尚未实现（规划中，见 docs/superpowers/plans/2026-08-17-p3-turn-combat.md Task 6）
  │
  ▼
Effect  ✅ 已实现（ll-sim::effect::Effect）
  │
  ▼
apply   ✅ 已实现（ll-sim::apply::apply）
```

这不代表设计有问题——`Effect` 与 `apply` 本来就可以先于 `resolve` 交付：`apply` 只需要知道
"如果收到一个 `Effect`该怎么落地"，不需要知道"谁会产出这些 `Effect`"。当前 `apply.rs` 的测试里
是直接手写 `Effect` 值来驱动测试的（见 `crates/ll-sim/src/apply.rs:85-219` 的测试模块），这正是这种分层
带来的好处：下游可以在上游就绪前独立开发和测试。

**读者提醒**：本文档写作之后，`resolve` 随时可能已经由并行工作的代理实现完毕。若发现
`crates/ll-sim/src/resolve.rs` 已存在，请以代码为准，本节描述的"尚未实现"状态已经过时。

## 四个概念，逐个对照真实代码

### `Intent`——玩家或 AI「想做什么」

定义：`crates/ll-sim/src/intent.rs:73-101`

```rust
pub enum Intent {
    Move { actor: EntityId, dir: Direction },
    Attack { actor: EntityId, target: EntityId },
    Wait { actor: EntityId },
    OpenDoor { actor: EntityId, pos: (i32, i32) },
}
```

关键设计点（模块文档已写明，见 `intent.rs:1-20`）：

- **纯数据，不做任何校验或世界查询**。`Intent::OpenDoor` 的 `pos` 字段是裸 `(i32, i32)`，
  不是 `ll_core::torus::TorusPos`——因为 `TorusPos` 的唯一构造路径 `TorusSize::wrap` 需要世界
  尺寸做归一化，而 `Intent` 产生的那一刻未必已经拿到 `WorldState`。等 `resolve` 读取 `Intent`
  时，它自然持有 `WorldState`，届时用 `world.size.wrap(x, y)` 归一化一次即可。
- **必须可序列化**（`#[derive(Serialize, Deserialize)]`）——这是确定性重放的基石，见下文。

已实现的输入映射：`intent_from_input(actor, &InputState) -> Option<Intent>`
（`crates/ll-sim/src/intent.rs:118-126`）把玩家一帧的按键状态映射成 `Intent::Move`/`Intent::Wait`。
`Attack`/`OpenDoor` 目前**没有**从输入产出的路径——模块文档解释了原因：这两者需要知道"那个方向上
到底有什么"，这是读世界之后才能判断的事，属于 `resolve` 的职责，不是输入层能单独决定的。

### `resolve`——纯函数，只读世界，可并行（规划中）

规划签名（`docs/superpowers/plans/2026-08-17-p3-turn-combat.md` Task 6）：

```rust
pub fn resolve(world: &WorldState, intent: &Intent) -> Vec<Effect>
```

### `Effect`——「发生了什么」的纯数据

定义：`crates/ll-sim/src/effect.rs:23-72`

```rust
pub enum Effect {
    MoveTo { actor: EntityId, pos: TorusPos },
    Damage { target: EntityId, amount: i32 },
    Kill { target: EntityId },
    ScheduleNext { actor: EntityId, at: Tick },
    SetTerrain { pos: TorusPos, kind: TerrainKind },
    AdjustWallet { actor: EntityId, delta: i64 },
}
```

值得注意的一条设计取舍（`effect.rs:16-21`）：`Effect` **不要求可序列化**，这与 `Intent` 刻意不同。
理由是 `Effect` 是 `resolve` 到 `apply` 之间同一进程内的瞬时产物，算完立刻被消费掉，不需要跨进程
或跨存档留存——真正要长期保留、用于重放的是产生它的 `Intent`。记录一整局的 `Intent` 流 + 世界种子
就足以完整重放，不需要额外记录中间产出的 `Effect`。

### `apply`——唯一写入口

签名与实现：`crates/ll-sim/src/apply.rs:54-83`

```rust
pub fn apply(world: &mut WorldState, effect: &Effect)
```

`apply.rs` 顶部文档写了「三条纪律」，这是理解这个函数为什么长这样的关键：

1. **它是全局唯一能改世界的函数**。别处出现 `&mut WorldState` 都是设计错误。
2. **它不含任何游戏逻辑**——六个分支要么直接赋值，要么是 `if let Some(..)` 这类"实体是否还存在"
   的边界防御，没有一处判断"这算不算命中""伤害该扣多少"，那些判断在 `resolve` 里已经做完。
3. **它必须极短**——每个分支不超过两行。

例如 `Effect::Damage` 分支（`apply.rs:61-65`）：

```rust
Effect::Damage { target, amount } => {
    if let Some(agent) = world.actors.get_mut(target) {
        agent.health -= amount;
    }
}
```

只做减法，不判断"减到负数算不算死"——那是规则判断，属于 `resolve`（或未来的战斗结算模块）。

**这行代码本身有一段值得记住的历史**：`health` 最初不是 `Agent` 的字段，而是 `WorldState`
上一张独立的 `BTreeMap<EntityId, i32>` 旁挂表，`Effect::Damage` 当时写的是
`*world.health.entry(target).or_insert(0) -= amount`。后来发现这张旁挂表不受 `Arena` 的世代号
管辖——实体被 `despawn` 后，旁挂表里的条目不会跟着消失，一旦某次 `Effect` 忘了同步删除，就会
积累出指向不存在实体的孤儿记录，而这类记录还会被写进存档。修法是把 `health` 直接收作 `Agent`
的字段（`crates/ll-world/src/entity/agent.rs:69`），让它随 `Agent` 一起被 `Arena::despawn`
整体收走，物理上不可能再出现孤儿。这次修正在 [`06-entity-storage.md`](06-entity-storage.md#世代索引之外的教训旁挂表与孤儿记录)
与 [`07-determinism.md`](07-determinism.md) 里还有更完整的讨论。

**这个签名如何拦住"不经 `Effect` 就改世界"**（`apply.rs:25-36` 有详细说明）：Rust 的可见性系统
管不住"`WorldState` 的字段是公开的，任何持有 `&mut WorldState` 的代码都能直接赋值"这件事——把
字段私有化只留访问器是更大的封装改造，本批次没有做。`apply` 的签名 `apply(world: &mut WorldState,
effect: &Effect)` 做到的是把"要改世界，必须先有一个 `Effect` 值"焊进类型签名：拿不出一个 `Effect`，
就没有把状态改动传给这个函数的办法。但**这不是编译期保证**——评审时若在 `apply.rs` 之外看到
`&mut WorldState` 紧跟着字段赋值，就是这条纪律被打破的信号，靠的是约定与评审，不是编译器。

## 为什么 `resolve` 必须是纯函数

这不是风格偏好，是并行结算的硬性前提。规格 §9.2（E1/E2）设想的规模是：一万智能体，单个商人
年均约 50 次决策事件，约 50 万事件/年。若每次决策都要串行跑 `resolve`，帧预算会被大量 AI 决策
吃满。

`resolve` 的签名 `fn(&WorldState, Intent) -> Vec<Effect>` 只接收共享引用 `&WorldState`，不接收
`&mut`——这意味着成千上万个 `resolve` 调用可以在 `rayon` 任务池里同时跑，因为它们全部只读同一份
世界状态，互不冲突，不需要任何锁或同步原语。产出的 `Effect` 收集起来后，再单线程依次交给
`apply`（`apply.rs` 里明确写着"读写从不交织"）。

这个结构反过来也约束了 `resolve` 不能做的事：它不能修改任何跨调用共享的可变状态（哪怕只是一个
计数器），否则并行调用之间会产生数据竞争或不确定的执行顺序——而不确定的执行顺序正是确定性重放
最大的敌人（见 [`07-determinism.md`](07-determinism.md)）。

## 为什么只有 `apply` 能写世界（C1 的具体体现）

这是约束 C1 的直接后果，详细的"违反会怎样"分析见 [`03-invariants.md`](03-invariants.md#c1)。
这里只说这个设计在数据流层面解决了什么：

| 需求 | 这个结构如何满足 |
|---|---|
| 自由读档（模式 3） | 存档就是序列化 `WorldState`，不需要额外收集"谁在什么时候动过世界"这类隐藏状态 |
| 脚本沙箱（未来 P4） | Steel 脚本只能产出 `Effect`（数据），物理上摸不到 `&mut WorldState` |
| 确定性重放 | 记录 `Intent` 流 + 世界种子即可完整复现一局，因为 `resolve` 对同样的输入恒产出同样的 `Effect` |
| 大量实体优化 | `Effect` 可以先累积、排序、去重、合并，再批量交给 `apply`，`apply` 本身不关心这些 `Effect` 是怎么来的 |
