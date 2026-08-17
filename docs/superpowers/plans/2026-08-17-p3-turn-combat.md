# P3 回合与战斗层 实施计划

> **给执行者：** 必须配合 `superpowers:subagent-driven-development` 逐任务实施。步骤使用 `- [ ]` 复选框追踪。

**目标：** 建立 `ll-sim` crate：实体存储、时间轴调度器、`Intent → resolve → Effect → apply` 单向管线、基础战斗结算，以及 Intent 流重放的确定性保障。

**架构：** `apply` 是全局唯一能修改世界的地方。`resolve` 是纯函数、只读世界、可并行。时间轴是一个按「下次行动时刻」排序的优先队列，敏捷高者自然行动更频繁。

**技术栈：** 仅 `ll-core` + `ll-world` + `serde`。**不引入 ECS 库**（理由见 Task 2）。

**规格：** [`docs/superpowers/specs/2026-08-16-lostland-design.md`](../specs/2026-08-16-lostland-design.md)
**上阶段交接：** [`knowledge/handoff/p2-to-p3.md`](../../../knowledge/handoff/p2-to-p3.md)
**相关设计：** [属性系统](../../../knowledge/design/attribute-system.md)、[社会系统](../../../knowledge/design/society-and-affiliation.md)、[Agent 目标与经济](../../../knowledge/design/agent-goals-and-economy.md)

## 全局约束

- **世界状态禁止浮点。** 含 `f32`/`f64` 的类型**不得派生 `Serialize`/`Deserialize`**。
- **`apply` 是唯一写入口**，单线程、无游戏逻辑，只做赋值。
- **`resolve` 是纯函数**，只读 `&WorldState`，可 `rayon` 并行。
- 所有随机性来自 `DetRng::for_entity(世界种子, 实体ID, 事件计数)`，**禁止全局 RNG**（约束 C3）。
- **时间轴队列只能存纯数据**（actor id + 行动类型 id + 参数），禁止闭包与裸指针（约束 C2）。
- 环面坐标只走 `TorusSize` 的方法。
- **新增的每一个「私有字段 + 校验构造函数」类型，加 serde 派生时都必须用 `try_from` 中转强制走校验**——P2 里这类缺陷出现了三次（见交接清单第一节第 3 条）。
- 所有公开项必须有文档注释；注释解释**为什么**；反直觉的选择必须解释。
- 测试 AAA 结构、测试名描述行为、**一个测试只断言一件事**、**测试名不得含混合大小写 ASCII 子串**。
- 文件 200–400 行为宜，800 行上限。
- 提交信息 `<type>: <描述>`，正文讲**为什么**，**不得含任何 AI 署名**。中文。

## 可依赖的既有 API（已照当前代码核实）

- `ll_core::torus`：`TorusSize::{new, width, height, wrap, delta, chebyshev, manhattan, squared_euclidean, MAX_EXTENT}`、`TorusPos::{x, y}`
- `ll_core::rng`：`DetRng::{for_entity, next_u64, gen_range, chance}`
- `ll_core::time`：`Tick`、`Season`、`TICKS_PER_MINUTE/HOUR/DAY`、`Tick::{hour_of_day, day_of_year, season, is_daylight}`
- `ll_core::light`：`day_curve(Tick) -> i32`
- `ll_core::hashing`：`StateHasher::{new, write_u64, write_i64, finish}`
- `ll_core::scaled`：`Milli`、`SCALE`
- `ll_core::ident`：`NamespacedId`、`ContentIndex`、`Interner`
- `ll_world::state::WorldState`：现有字段 `{ seed, clock, size, terrain }`，方法 `new`/`advance`/`hash`
- `ll_world::chunk::ChunkGrid::{new, world, terrain_at, set_terrain}`
- `ll_world::terrain::TerrainKind`：17 个常量，`blocks_sight`/`blocks_move`/`move_cost`/`is_known`
- `ll_world::fov::{VisibleSet, compute_fov}`
- `ll_world::light::{LightLevel, ambient_light, season_light_scale, sight_radius_at}`

---

### Task 1：修复 `DrawOrder.foot_y` 的环面缺陷（P2 交接项，必须最先做）

**Files:** `crates/ll-render/src/sprite.rs`、两个 demo 的调用点

**问题**：`DrawOrder.foot_y` 的文档定义为「**世界**纵坐标」，而环面世界里 `y = 世界高度 − 1` 与 `y = 0` 在屏幕上相邻却相差整个世界高度。**跨南北接缝时 Y 排序会反转**，接缝北侧的单位被南侧的错误遮挡。

P2 只有一个实体所以看不出来。**P3 引入第二个实体的那一刻它就是可见缺陷**，故排在最前。

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-render/src/sprite.rs 的 tests 模块
    #[test]
    fn 跨接缝的两个实体按屏幕纵坐标排序() {
        // 环面世界里 y=世界高度-1 与 y=0 在屏幕上相邻。若排序键用世界
        // 纵坐标，接缝北侧的单位会被南侧的错误遮挡。
        // Arrange：屏幕坐标下北侧单位 y 更小（更靠上，应先绘制）。
        let north_on_screen = DrawOrder::new(Layer::ENTITY, 100, 1);
        let south_on_screen = DrawOrder::new(Layer::ENTITY, 116, 2);

        // Act & Assert
        assert!(north_on_screen < south_on_screen);
    }
```

- [ ] **Step 2：改文档与调用方**

`DrawOrder::new` 的 `foot_y` 参数文档改为：

```rust
    /// 脚底的**屏幕**纵坐标（相机相对），不是世界纵坐标。
    ///
    /// 必须用屏幕坐标：环面世界里 `y = 世界高度 − 1` 与 `y = 0` 在屏幕上
    /// 相邻却相差整个世界高度，用世界坐标会让跨南北接缝的排序反转，
    /// 接缝北侧的单位被南侧的错误遮挡。
    ///
    /// 屏幕坐标由 `Camera::world_to_screen` 得出，它已处理环面最短位移，
    /// 因此接缝两侧的相邻格在屏幕上也相邻。
```

两个 demo 的调用点改为传 `camera.world_to_screen(pos).1 + 脚底偏移`。

- [ ] **Step 3：验证并提交**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

```bash
git commit -F - <<'EOF'
fix: DrawOrder 的排序键改用屏幕纵坐标

环面世界里 y=世界高度-1 与 y=0 在屏幕上相邻却相差整个世界高度。排序键
用世界纵坐标会让跨南北接缝的 Y 排序反转，接缝北侧的单位被南侧的错误
遮挡。

P1 用 TorusSize::delta 解决了跨接缝的位置，但排序键没跟上——这是同一个
问题的两半，当时只修了一半。P2 只有一个实体所以看不出来，P3 引入第二个
实体就会暴露，故排在 P3 最前。

文档写明为什么不能用世界坐标，否则后来者会顺手改回去。
EOF
```

---

### Task 2：实体存储与 `WorldState` 扩展

**Files:** 创建 `crates/ll-sim/{Cargo.toml, src/lib.rs, src/entity.rs}`；修改 `crates/ll-world/src/state.rs`

> ## 裁定：不使用 ECS 库，用世代索引竞技场
>
> 规格 §3 技术栈表列了 `hecs` 作为实体存储。**本阶段不采用**：
>
> 1. **`WorldState` 必须完整序列化且迭代顺序确定。** ECS 的原型存储让序列化与确定性迭代都变复杂，而这两条是模式3 自由读档与确定性重放的地基。
> 2. **ECS 的优势用不上。** 它擅长「多种组件组合的稀疏查询」，而本项目的 agent 相当同质；背景 NPC 走批量公式（见 Agent 目标与经济 §7.2），不做逐实体组件查询。
> 3. **零依赖更可控。** 世代索引竞技场约 100 行，完全可测。
>
> **实施第一步须更新规格 §3 技术栈表**，把 `hecs` 一行改为「自研世代索引竞技场」并注明理由——否则就是文档与代码不一致（§13 视为缺陷）。

**Interfaces Produces:**
- `pub struct EntityId { index: u32, generation: u32 }`
- `pub struct Arena<T>`：`new`、`spawn(T) -> EntityId`、`get(EntityId) -> Option<&T>`、`get_mut`、`despawn(EntityId) -> bool`、`iter()`、`len()`
- `pub struct Agent`（字段见下）
- `WorldState` 新增 `pub agents: Arena<Agent>`、`pub timeline: Timeline`

> **世代索引解决悬垂 ID**：实体死亡后槽位被复用，旧 ID 因世代号不匹配而查询失败，而不是静默指向新实体。回合制里尤其重要——时间轴队列可能残留已死实体的条目。

```rust
pub struct Agent {
    pub pos: TorusPos,
    pub stats: BaseStats,
    /// 下次行动的世界时刻，时间轴排序依据。
    pub next_action_at: Tick,

    // ↓ 以下四个 P3 可留空，但字段必须现在就有。
    // 往 WorldState 加字段意味着存档迁移，而存档格式在 P5 冻结——
    // P3 加是零成本，P8 加要写迁移链。
    /// 归属列表（势力/宗教/行会/文化）。见 society-and-affiliation.md
    pub affiliations: Vec<Affiliation>,
    /// 钱包，最小货币单位。见 agent-goals-and-economy.md
    pub wallet: i64,
    /// 当前职业，指向注册表。
    pub profession: ContentIndex,
    /// 目标栈。
    pub goals: Vec<Goal>,
}
```

- [ ] **TDD 循环**，测试至少覆盖：
- `新生成的实体可以按标识取回`
- `销毁后原标识无法再取到实体`
- `槽位被复用后旧标识因世代不符而失效`（世代索引存在的理由）
- `销毁不存在的实体返回假而非崩溃`
- `序列化往返后实体数量不变`
- `世代号溢出时槽位被弃用而非回绕`（回绕会让旧标识意外复活）

- [ ] **提交**（规格更新单独一次 `docs:` 提交）

---

### Task 3：时间轴调度器

**Files:** `crates/ll-sim/src/timeline.rs`、`crates/ll-sim/tests/timeline_blackbox.rs`

**Interfaces Produces:**
- `pub struct TimelineEntry { pub at: Tick, pub actor: EntityId }`
- `pub struct Timeline`：`new`、`schedule(EntityId, Tick)`、`pop_next() -> Option<TimelineEntry>`、`remove(EntityId)`、`peek_next_tick() -> Option<Tick>`、`len()`
- `pub fn action_cost(base_cost: u32, effective_speed: u32) -> u32`

**核心公式**（来自属性系统）：

```
行动耗时 = 基础代价 × 1000 / max(1, 有效敏捷)
```

整数除法，确定。`max(1, ...)` 防敏捷被减到 0 时除零。

> ## 确定性的两条硬要求
>
> **① 同刻事件必须有稳定的打破平局规则。** 两个实体在同一 `Tick` 行动时按 `EntityId` 排序决定先后——否则 `BinaryHeap` 的弹出顺序依赖插入历史，读档后会分叉。
>
> **② 队列必须可完整序列化。** 约束 C2 要求只存纯数据。序列化时存**排序后的 `Vec<TimelineEntry>`**，反序列化时重新入堆——保证重建后的弹出顺序与原来一致。

- [ ] **TDD 循环**，测试至少覆盖：
- `最早的条目先弹出`
- `同刻条目按实体号升序弹出`
- `敏捷翻倍则行动耗时减半`
- `敏捷为零时不会除零`
- `移除某实体后其条目不再弹出`
- `空队列弹出返回空值`
- `序列化往返后弹出顺序不变`

- [ ] **属性测试**：
- `弹出顺序恒按时刻单调不减`
- `任意调度序列下弹出总数等于调度总数`
- `序列化往返前后弹出顺序完全一致`

- [ ] **提交**

---

### Task 4：`Intent` 与玩家输入映射

**Files:** `crates/ll-sim/src/intent.rs`

**Interfaces Produces:**
- `pub enum Intent { Move { actor, dir }, Attack { actor, target }, Wait { actor }, OpenDoor { actor, pos } }`
- `pub struct Direction`（八向），`Direction::delta() -> (i32, i32)`
- `pub fn intent_from_input(actor: EntityId, input: &InputState) -> Option<Intent>`

> `Intent` 是**纯数据且可序列化**——记录 Intent 流 + 世界种子即可完整重放一局（规格 §4）。这是 Task 7 的基础，也是排查玩家报告缺陷最强的手段。

- [ ] **TDD 循环**：各方向映射、无输入返回空值、Intent 序列化往返。
- [ ] **提交**

---

### Task 5：`Effect` 与 `apply`

**Files:** `crates/ll-sim/src/effect.rs`、`crates/ll-sim/src/apply.rs`

**Interfaces Produces:**
- `pub enum Effect { MoveTo { actor, pos }, Damage { target, amount }, Kill { target }, ScheduleNext { actor, at }, SetTerrain { pos, kind }, AdjustWallet { actor, delta } }`
- `pub fn apply(world: &mut WorldState, effect: &Effect)`

> ## `apply` 的三条纪律
>
> 1. **它是全局唯一能改世界的函数。** 别处出现 `&mut WorldState` 都是设计错误。
> 2. **它不含任何游戏逻辑**——只做赋值。判定、公式、随机全在 `resolve`。
> 3. **它必须极短**，因为它单线程执行，是并行结算之后的串行瓶颈。
>
> **判断标准**：`apply` 里若出现判断游戏规则的 `if`（而非边界防御），说明逻辑漏进来了。

- [ ] **TDD 循环**，测试至少覆盖：
- `移动效果改变实体位置`
- `伤害效果扣减生命`
- `对已销毁实体施加效果不会崩溃`（时间轴可能残留死者条目）
- `效果的应用顺序不影响最终世界哈希`（不成立则说明存在顺序依赖，必须显式排序）

- [ ] **提交**

---

### Task 6：`resolve` 与战斗结算

**Files:** `crates/ll-sim/src/resolve.rs`、`crates/ll-sim/src/combat.rs`

**Interfaces Produces:**
- `pub fn resolve(world: &WorldState, intent: &Intent) -> Vec<Effect>`
- `pub fn damage_after_defense(attack: i32, defense: i32, pen: Penetration) -> i32`

**伤害公式**（规格决策 30）：

```
有效防御 = max(0, (防御 − 穿透.flat) × (1000 − 穿透.permille) / 1000)
减后伤害 = max(基础伤害 × 100 / 1000, 基础伤害 − 有效防御)   // 下限 10%
最终伤害 = 减后伤害 × 1000 / (1000 + 有效防御)
```

**穿透必须先减固定再乘千分比**，顺序定死。

- [ ] **TDD 循环**，测试至少覆盖：
- `结算不修改世界`（纯函数的核心保证——前后世界哈希不变）
- `移动到不可通行地形不产生移动效果`
- `移动到浅水的行动耗时高于草地`（分级 move_cost 的落地）
- `防御极高时伤害仍不低于攻击力的一成`（10% 下限）
- `固定穿透对低防御目标收益更高`
- `千分比穿透对高防御目标收益更高`
- `攻击关着的门产生开门效果而非伤害效果`（门是两个地形种类）

- [ ] **提交**

---

### Task 7：确定性回归——Intent 流重放

**Files:** `crates/ll-sim/tests/replay.rs`

> **这是 P3 最重要的交付物**，价值超过任何单个战斗功能。
>
> 记录「世界种子 + Intent 流」即可完整复现一局。玩家报告缺陷时发来存档与操作记录，本地一按就复现——这是排查 Roguelike 缺陷最强的武器，也是模式3 自由读档正确性的最终验证。

- [ ] **TDD 循环**：
- `同一意图流在同一种子下产出相同的世界哈希`
- `序列化世界并读回后继续执行同一意图流，结果与不中断执行一致`（**最关键**——同时验证存档完整性与重放确定性）
- `不同意图流产出不同哈希`

黄金基准：固定种子 + 固定 Intent 流，冻结最终世界哈希。**文件顶部必须写明「测试挂了不许把期望值改成实际值」的规矩**——去读 `crates/ll-core/tests/determinism.rs` 顶部，照同样的精神写，不要另编措辞。

- [ ] **提交**

---

### Task 8：季节归属的裁定（规格反向核对项）

**Files:** 规格文档

交接清单第四节指出：规格 §7.2 写「季节更替是**时间轴上的一个定时事件**，其 `Effect` 修改各城镇生产速率、地形通行性与野怪分布表」，而实现是 `Tick::season()` **纯函数派生**，无事件、无 Effect。

**P3 建时间轴，正是必须决定的时候。两者不能都留着。**

- [ ] **Step 1：裁定并说明理由**

建议方向（执行者可提出反对并说明）：**保留纯函数派生作为「当前是什么季节」的查询，同时在时间轴上放一个季节切换事件用于触发 `Effect`。**

理由：查询必须是纯函数（否则光照、视野每帧都要翻事件历史）；而「季节切换那一刻要改各城镇生产速率」是一次性状态变更，天然是 Effect。**两者不矛盾——一个是「现在是什么」，一个是「切换时做什么」。**

- [ ] **Step 2：更新规格 §7.2 措辞**，准确描述这个两层结构。
- [ ] **提交**（`docs:`）

---

### Task 9：P3 验收 Demo

**Files:** `crates/ll-sim/examples/p3_acceptance/`

必须展示：

1. 玩家在真实生成的地形上移动，**FOV 随移动更新**
2. **至少三个敌人，各有不同敏捷——出手频率肉眼可见地不同**（时间轴的核心验收点）
3. 时间轴侧栏显示**接下来若干次出手顺序**
4. 攻击与受击，伤害数字可见
5. **跨南北接缝时遮挡关系正确**（Task 1 的验收）
6. 按 F2 存图作为视觉回归基准
7. Esc 退出

**必须实测**：跑起来、无 wgpu validation error、拿到非全黑渲染结果，**如实报告哪些验证了、哪些没有**。

> **顺带记录一个体验问题**（非本阶段解决）：若一场战斗有几百个单位，队列挨个转完一整轮，玩家眼看几百次行动逐个播放会很啰嗦。**这不是性能瓶颈，是节奏问题**——时间轴回合制天然没有「几百单位挤在一帧」的压力，因为永远只处理队首一个。背景/非交战单位仍应批量简化，但理由是「别让玩家等」，不是「怕卡」。P7 做随从与大规模战斗时须正视。

- [ ] **提交**

---

## 自查

### 完整调用链（P1 的教训要求的一节）

从玩家按键到世界改变再到出图，逐个 API 点名：

```
InputState::was_activated(GameKey::Right)                       ← ll-platform
  ↓
intent_from_input(玩家实体, &input) -> Option<Intent>            ← ll-sim::intent
  ↓ Intent::Move { actor, dir }
resolve(&world, &intent) -> Vec<Effect>                          ← ll-sim::resolve
  ↓ 需要 &WorldState ✓、&Intent ✓
  ↓ 内部查 world.terrain.terrain_at(目标格) → TerrainKind ✓      ← ll-world
  ↓ 查 TerrainKind::blocks_move / move_cost ✓
  ↓ 查 world.agents.get(actor) → Option<&Agent> ✓                ← ll-sim::entity
  ↓ 算 action_cost(基础代价, 有效敏捷) ✓                          ← ll-sim::timeline
  ↓ 产出 [Effect::MoveTo, Effect::ScheduleNext]
for effect in effects: apply(&mut world, &effect)                ← ll-sim::apply
  ↓ 唯一写入口，改 world.agents / world.timeline
world.timeline.pop_next() -> Option<TimelineEntry>               ← ll-sim::timeline
  ↓ 下一个行动者；AI 由固定策略产出 Intent（行为树属 P7）
  ↓ 循环直到轮到玩家
compute_fov(&world.terrain, 玩家位置,
            sight_radius_at(基础, ambient_light(world.clock)))   ← ll-world
Camera { center: 玩家位置, world: world.size }                    ← ll-render
camera.visible_tiles() → Vec<TorusPos> ✓
对每个 tile / 每个 agent：
  camera.world_to_screen(pos) → (i32, i32) ✓
  DrawOrder::new(Layer::ENTITY, 屏幕脚底Y, entity.index()) ✓      ← Task 1 修正后
  atlas.uv_rect(名字) → Option<[f32;4]> ✓
  batch.push(order, SpriteInstance { .. }) ✓
batch.flush(&gpu, render_target.view()) ✓
gpu.acquire_frame() → blit_to → queue().present() ✓
```

**每一步的参数都能从上一步或已有状态取到，无断裂。** 本阶段需新建的只有「`Agent` → 图集条目名」的映射表，列入 Task 9。

### 规格覆盖

| 规格要求 | 对应任务 |
|---|---|
| §4 意图—结算—效果单向流 | Task 4、5、6 |
| §4 约束 C2 队列只存纯数据 | Task 3 |
| §4 约束 C3 每实体确定性 RNG | Task 6、7 |
| §8 时间轴调度器 | Task 3 |
| 决策 11 时间轴事件队列 | Task 3 |
| 决策 30 伤害公式与 10% 下限 | Task 6 |
| §7.2 季节的归属 | Task 8（反向核对项） |
| §3 实体存储 | Task 2（裁定改为自研，须更新规格） |
| §15 每阶段交付验收 demo | Task 9 |
| 交接：`DrawOrder.foot_y` 环面缺陷 | Task 1 |
| 交接：预留 affiliations/wallet/profession/goals | Task 2 |
| 交接：serde 绕过校验的规律 | 全局约束 |

### 有意留给后续阶段的缺口

- **行为树驱动的 AI** 属 P7。P3 的敌人用固定策略（朝玩家移动、相邻则攻击），足以验证时间轴。
- **技能与职业** 属 P5。P3 只做基础攻击。
- **背景 NPC 的批量公式** 属 P8。P3 的实体全是前景层。
- **`cargo doc` 门禁**与规格 §14.7 缺失的四项 CI 门禁，见交接清单第五节——建议 P3 收尾一并处理，不阻塞本阶段。
- **规格中三项无人认领的条目**：季节由 Task 8 处理，光照透过率与气候条带仍待认领。

### 收尾必做：反向核对规格

P2 立下的纪律：**阶段收尾必须反向核对一次规格**——不是查实现是否满足规格，而是查**规格是否已被实现淘汰**。P1 有条规格错了整整一个阶段无人发现，因为从没人问过反过来的问题。
