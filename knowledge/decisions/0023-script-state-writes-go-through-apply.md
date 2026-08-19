# 0023 — 脚本状态写入必须经 `apply`，不得直接写穿（细化 0006 在脚本层的具体应用）

**日期**：2026-08-19（裁定 P5-1，存档格式与身份批次，开工前阻塞级裁定）
**状态**：已生效，已落地（`ac27217`）
**关键提交**：`ac27217`（脚本状态存储落地——值类型、配额、孤儿保留、VM 强制重建）
**影响范围**：`crates/ll-script/src/api/state.rs`（`PENDING_WRITES` 线程局部缓冲，第 61 行；`take_pending_writes`，第 76 行）、`crates/ll-sim/src/effect.rs:127`（`Effect::SetScriptState`）、`crates/ll-sim/src/apply.rs:105`（对应处理分支）、[0006](0006-intent-resolve-effect-apply.md)（本 ADR 是它在脚本层的一次具体应用与再确认）、[knowledge/design/script-state-storage.md](../design/script-state-storage.md) §8.2（原文已改写，标注「本节原文错误」）

## 背景

[0006](0006-intent-resolve-effect-apply.md) 已经确立「`apply` 是全局唯一能修改世界的地方」这条不变式，理由是确定性重放、脚本沙箱、自由读档三者都要求写入路径单一。P4 引入脚本层之后，需要给 mod 脚本提供一种「记住状态」的能力（技能进度、任务标记、mod 自定义的持久数据）——这类状态的存在意义就是要进存档，因此必然要落进 `WorldState`。

设计文档 `script-state-storage.md` §8.2 原文写的是「直接写穿，没有中间层」——即脚本调用 `state-set!` 时直接修改 `WorldState` 里的脚本状态字段，不经过 `Intent → resolve → Effect → apply` 这条链路。这与 [0006](0006-intent-resolve-effect-apply.md) 的 C1 约束字面冲突。

## 决定

**C1 赢，设计文档的字面表述错误，脚本状态写入必须经过既有的 `Effect → apply` 管线。**

理由不是照搬教条，是重新推一遍第一性原理：脚本状态**存在 `WorldState` 里**——那是它存在的全部意义（要进存档，要能被读档恢复）。既然它是 `WorldState` 的一部分，它**就是**世界状态,不存在「脚本自己的数据」这样一个可以豁免于 [0006](0006-intent-resolve-effect-apply.md) 约束的特殊类别。绕开 `Effect` 流直接写穿的后果是确定性重放会失真：同一串 `Intent` 重跑一遍,如果脚本状态是写穿产生的、不经过 `Effect`，重放过程就不会重新产生这次写入——存档能存下当时的值，但从 `Intent` 流重放却复现不出它,这正是 [0006](0006-intent-resolve-effect-apply.md) 想要保证、也是本项目模式 3 自由读档依赖的性质。

**具体实现（性能顾虑的解法）**：脚本一次决策期间可能多次调用 `state-set!`/`entity-state-set!`，若每次调用都发一条独立 `Effect`，开销会显著高于原生代码路径。解法不是放弃「经 `apply`」这条原则,而是**批量化**：`ll-script::api::state` 用线程局部缓冲 `PENDING_WRITES`（`crates/ll-script/src/api/state.rs:61`）收集一次脚本调用期间的全部写入（去重、last-write-wins），调用结束由宿主 `take_pending_writes()`（同文件第 76 行）取走，包成**一条**携带 `Vec<ScriptStateWrite>` 的 `Effect::SetScriptState`（`crates/ll-sim/src/effect.rs:127`）发出，走既有 `resolve → apply`（`crates/ll-sim/src/apply.rs:105` 对应分支）。`Effect` 因此不再是 `Copy`（携带 `Vec`）,已核查全部既有调用点都不依赖隐式按位拷贝。**`Effect` 流保持诚实，又不必为每次写入单独付一条 `Effect` 的开销。**

读操作（`state-get!`/`state-get-foreign`/`entity-state-get!`）先查缓冲区（保证同一决策内先写后读可见），再查已提交 `WorldState`；查不到时用 `SteelVal::Void` 当哨兵，不用 `#f`——避免与合法存储的 `ScriptValue::Bool(false)` 混淆。

## 被否决的选项

**按设计文档原文，直接写穿，不经中间层**——否决理由见上文「决定」一节：这不是本 ADR 新发现的问题,是 [0006](0006-intent-resolve-effect-apply.md) 已经论证过的「就地修改 `WorldState`」这条被否决路径的一个具体化实例（无法保证多线程安全、脚本沙箱形同虚设、重放不可靠）。设计文档写出这条路径的原因不明,原始记录未说明是疏漏还是刻意的性能取舍,本 ADR 不代为编造具体动机,只裁定它与既有架构约束冲突、不应执行。

**每次 `state-set!` 调用都单独发一条 `Effect`，不做批量收集**——否决：性能上会让脚本状态写入的开销显著高于必要值（一次决策期间可能有多次状态写入,例如任务进度里几个变量在同一次判定里先后更新)，且大多数场景下没有理由分成多条 `Effect`——批量化不改变「写入必须经 `Effect`」这条不变式,只是把多次调用打包成一次跨越管线的开销,是纯粹的性能优化,不牺牲正确性。

## 与 [0006](0006-intent-resolve-effect-apply.md) 的关系：确认而非新裁定

本 ADR 记录的不是一条独立的新架构原则,是 [0006](0006-intent-resolve-effect-apply.md) 既有原则在脚本层被质疑（设计文档试图开一个口子）之后的**再确认**——之所以仍值得单独立卷，是因为：

1. 这次质疑差点被写死进正式设计文档（`script-state-storage.md` §8.2），若不纠正，会成为脚本层长期存在的、与核心架构不一致的实现依据。
2. 批量化收集这个具体解法（`PENDING_WRITES` + 一条携带 `Vec` 的 `Effect`）是本项目在「保持不变式」与「不为每次写入付管线开销」之间给出的一个可复用范式，值得独立记录供未来类似场景（例如任何需要脚本频繁写状态的新系统）参考,不应该被淹没在 P5 账本的批次记录里。

## 后果

- 设计文档 `script-state-storage.md` §8.2 已按裁定改写，标注「本节原文错误」而非直接删除原文——保留错误曾经存在过的痕迹，与 [0013](0013-ledger-vs-adr-discipline.md)「已发布的文档应当被取代而非篡改」的惯例一致。
- **配额判定**（`PER_MOD_QUOTA_BYTES = 256KB`/`PER_MOD_ENTITY_QUOTA_BYTES = 4KB`，`crates/ll-world/src/script_state.rs:33`/`:38`）只扫描目标 mod 自己的已提交 + 待提交记录,不触碰其他 mod 的数据,天然不受加载顺序影响——这条性质由专门测试锁定（一个 mod 写满配额,另一个 mod 不受影响）,是「经 `apply`」这条管线之外的一个独立正确性保证,附带记录以说明批量化没有引入跨 mod 的配额泄漏。
- **孤儿保留**：`WorldState`/`Agent` 反序列化不对脚本状态做任何「当前 mod 集合」过滤,卸载 mod 后其脚本状态原样随存档往返,直到有明确的清理机制——这是刻意的,与「诚实退化,不静默丢数据」的项目一贯态度一致(见 P5-save 账本「拒绝制造虚假保证」一节),但意味着脚本状态在理论上可以无界增长(每个已卸载 mod 的历史数据永久占用存档空间),当前用 256KB/mod 的配额封顶单个 mod 的贡献,未对「已卸载 mod 的孤儿数据」设置额外的清理或压缩机制,留作已知的未决事项。
- 这条纪律对未来任何「看起来应该有特殊待遇」的新状态类别都适用:任何东西一旦被放进 `WorldState`，就自动被 [0006](0006-intent-resolve-effect-apply.md) 的约束覆盖,不存在「因为存储位置特殊所以可以绕开管线」的例外——这正是本 ADR 标题里「细化」二字的含义,细化的是判断方法(问「它是不是被放进了 WorldState」，而不是问「它是不是脚本产生的」),不是放松原则本身。
