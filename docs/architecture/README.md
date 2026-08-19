# 迷途大陆 / LostLand 架构总览

**冻结时间**：2026-08-17
**核对依据提交**：`7a126f5`（写作期间仓库有其他代理并行提交，本组文档在写作过程中已根据该次核对做过一轮追加校对——例如
`WorldState::health` 旁挂表被替换为 `Agent::health` 字段这次改动，已同步反映进
[`02-core-data-flow.md`](02-core-data-flow.md)、[`06-entity-storage.md`](06-entity-storage.md)、
[`07-determinism.md`](07-determinism.md)。此后若仓库继续变化，请以代码为准，本文档组不会自动跟随）。
**2026-08-18 补充**：规格 §4 新增约束 C5（提交 `2f4cc26`），[`03-invariants.md`](03-invariants.md)
与 [`07-determinism.md`](07-determinism.md) 已同步补上，本表第 3、7 行的描述已更新；这一次补充
不改变本组文档其余部分对 `7a126f5` 的核对结论。

这组文档面向**新加入项目、需要在半天内理解系统骨架**的工程师。它不是规格的复述——规格在
[`docs/superpowers/specs/2026-08-16-lostland-design.md`](../superpowers/specs/2026-08-16-lostland-design.md)。
这组文档的价值在于：**它核对过真实代码**，并且明确标出规格与代码分歧的地方（见
[`discrepancies.md`](discrepancies.md)）。

## 核对方法与局限

写作方式是「先读代码，再读规格，冲突时以代码为准」。具体做法：

- 用 `cargo tree --workspace -e no-dev` 与各 crate 的 `Cargo.toml` 实测依赖图，不采信规格 §5 的文字描述。
- 每一条设计判断尽量给出 `路径:行号` 或 `模块::函数`，方便跳转到真代码验证。
- 规格、`knowledge/handoff/*`、`knowledge/decisions/*` 只作为「为什么这样设计」的背景资料，不作为「代码长什么样」的依据。

**局限**：`crates/ll-sim` 在本文档写作期间正被另一个代理实现（`resolve` 尚未落地，见
[`02-core-data-flow.md`](02-core-data-flow.md) 的说明），仓库同时还有另外两个代理在其他方向并行工作。
本组文档只保证在冻结时间点核对属实；此后若 `ll-sim::resolve` 等尚未实现的部分已经落地，请以代码为准，
并将本文档标记为过时段落更新掉——**不要因为文档说"尚未实现"就假设代码里也没有**。

## 阅读顺序

按依赖关系从下往上读，符合代码本身的分层顺序：

| # | 文档 | 内容 | 对应规格章节 |
|---|---|---|---|
| 1 | [`01-crate-layering.md`](01-crate-layering.md) | Crate 分层、真实依赖图、`ll-world` 与 `ll-sim` 的边界 | §5 |
| 2 | [`02-core-data-flow.md`](02-core-data-flow.md) | `Intent → resolve → Effect → apply` 单向数据流 | §4 |
| 3 | [`03-invariants.md`](03-invariants.md) | 五条不可让步的约束 C1–C5，违反后果 | §4 |
| 4 | [`04-torus-topology.md`](04-torus-topology.md) | 环面世界拓扑、`TorusSize::delta`、`DrawOrder` 排序键教训 | §7.1 |
| 5 | [`05-integer-discipline.md`](05-integer-discipline.md) | 整数纪律、`Milli`、浮点边界 | §13、ADR 0002 |
| 6 | [`06-entity-storage.md`](06-entity-storage.md) | 两层实体存储：薄层 SoA / 厚层 AoS | 技术栈表 |
| 7 | [`07-determinism.md`](07-determinism.md) | 确定性与可重放：机制清单 + 常见破坏方式 | §14.4 |
| — | [`discrepancies.md`](discrepancies.md) | 规格/交接清单与真实代码的不一致清单 | — |

## 项目当前所处阶段（写作时）

仓库正处于 **P3（回合与战斗层）** 施工中。已交付并有验收 demo 的阶段：

- P0 平台地基（`ll-platform`）
- P1 渲染与动画（`ll-render`）
- P2 世界与地形（`ll-world`）

P3（`ll-sim`）当前已实现：时间轴调度器（`timeline`）、`Intent`（`intent`）、`Effect`（`effect`）、
唯一写入口 `apply`（`apply`）。**`resolve`（从 `Intent` 结合 `WorldState` 产出 `Effect` 的纯函数）尚未实现**，
详见 `crates/ll-sim/src/lib.rs` 顶部模块文档与本组文档的 [`02-core-data-flow.md`](02-core-data-flow.md)。

## 一条贯穿全部文档的方法论提醒

`knowledge/handoff/p2-to-p3.md` 记录了一条本项目吃过亏换来的纪律：

> 每个阶段收尾必须反向核对一次规格——不是查实现有没有满足规格，而是查规格有没有被实现淘汰。

来由是 P1 阶段规格里「精灵批处理需支持最多 4 份偏移副本」这条错了整整一个阶段、五轮评审无一发现，
因为评审问法始终是「实现是否满足规格」。本组文档延续同一纪律：**代码是唯一真相，规格是历史意图的记录**，
两者冲突时以代码为准，并把冲突记录进 [`discrepancies.md`](discrepancies.md)。
