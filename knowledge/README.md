# 迷途大陆 知识库

供长期开发复用的资料库。**不放临时笔记**——每一份文件都应该在半年后仍然有用。

## 目录

| 目录 | 内容 | 写入时机 |
|---|---|---|
| `decisions/` | 架构决策记录（ADR）：背景 / 选项 / 结论 / 后果 | 做出任何影响多个 crate 的决定时 |
| `licenses/` | `cargo-deny` 扫描报告与逐依赖许可证结论 | 每次新增依赖后 |
| `design/` | 游戏设计：职业表、技能树结构、任务图规范、数值曲线 | 设计定稿时 |
| `audit/` | 文档—代码一致性审计报告，以及审计产出的可直接照做工单清单 | 每次审计执行后 |
| `pipelines/` | 资产管线：图集规格、动画格式、地图格式 | 格式冻结时 |
| `workflow/` | 协作约定：分支策略、提交规范、并行任务切分 | 约定变更时 |
| `handoff/` | 阶段交接：上一阶段留给下一阶段的已知问题与前瞻判断 | 每个阶段收尾时 |

## 索引

> **【2026-08-23】Steel 脚本系统整体拆除。** `crates/ll-script/`、`steel-core`
> 依赖与全部 `.scm` 文件均已删除；mod 内容改用 `mods/<id>/*.json5` 数据文件，
> 玩法层逻辑住在引擎里的 Rust，第三方 Rust 扩展能力明确推迟不做。
> 下面凡是标着「〔脚本时代，历史记录〕」的条目，正文都保持冻结原样，各自带一段
> 2026-08-23 的订正说明——**它们不描述当前代码**。起因见
> [ADR 0028](decisions/0028-steel-engine-construction-memory-corruption.md)，
> 取代关系见 [ADR 0018](decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 的订正段与规格 §4 的 `[2026-08-23 规格修订]`。

### 决策记录

- [0001 — Steel 沙箱能力实测](decisions/0001-steel-sandbox-verification.md) — Steel 能否保证 mod 脚本不搞死游戏进程 〔脚本时代，历史记录〕
- [0002 — 世界状态一律用整数](decisions/0002-integer-only-world-state.md) — 为什么禁止浮点，以及最容易破防的那一处（脚本内部计算的例外见 0020）
- [0003 — winit 版本与 feature 策略](decisions/0003-winit-dependency-policy.md) — 为什么锁 0.30、为什么裁 feature 而非豁免公告
- [0004 — 两层实体存储替代 ECS](decisions/0004-two-layer-entity-storage.md) — 薄层列式 `ThinPopulation` + 厚层行式 `Arena<T>`，ECS 的思想留下、框架本身不引入
- [0005 — 环面世界拓扑与整数可平铺噪声](decisions/0005-torus-topology-and-integer-noise.md) — 为什么坐标距离与地形噪声都必须是整数运算
- [0006 — Intent → resolve → Effect → apply 单向数据流](decisions/0006-intent-resolve-effect-apply.md) — `apply` 是全局唯一能改写世界状态的地方，为什么要收敛成这一条
- [0007 — 对称阴影投射视野及其墙可见性取舍](decisions/0007-symmetric-shadowcasting-fov.md) — FOV 对称性与「能看见墙」之间刻意选择的取舍
- [0008 — 行会中介贸易](decisions/0008-guild-mediated-trade.md) — 把 O(n²) 的居民互市约束做成世界观本身，而不是纯粹为省算力的妥协
- [0009 — 默认派生，只存偏差](decisions/0009-derive-by-default-store-only-deviation.md) — 钱包、姓名、关系、性格四处复用的同一个项目级原则
- [0010 — 白昼判定与光照曲线收敛为同一份真相源](decisions/0010-single-source-of-truth-for-daylight.md) — 为什么两套本该一致的计算不能各算各的
- [0011 — serde 的 `try_from` 中转规矩](decisions/0011-serde-try-from-bypasses-validating-constructors.md) — `#[derive(Deserialize)]` 会绕过私有字段的校验构造函数，三次踩坑后定的规矩
- [0012 — Steel 标准库能力面实测](decisions/0012-steel-capability-surface-verification.md) — 补 0001 未测的部分：白名单机制、已知边界、`steel/time` 待查项 〔脚本时代，历史记录〕
- [0013 — 阶段账本记过程，ADR 记决策](decisions/0013-ledger-vs-adr-discipline.md) — 为什么阶段账本从不进版本库、重要裁定必须及时提升为 ADR
- [0014 — 季节维持纯函数派生](decisions/0014-season-pure-function-derivation.md) — 不做时间轴事件，理由与白昼真相源收敛（0010）同构
- [0015 — 内容 ID 的注册校验是解析，不是 serde 不变式](decisions/0015-content-id-registration-is-parsing-not-invariant.md) — 校验属于哪个阶段
- [0016 — mod 性能分档按声明方式，不按作者身份](decisions/0016-mod-performance-tiers-by-declaration.md) — 「本体即 Mod」守门规则的完整论证
- [0017 — 声明式分档物化为列式数据](decisions/0017-tiered-declarations-materialize-columnar.md) — 注册期完整校验
- [0018 — 脚本层边界按系统类型划分（引擎层/玩法层）](decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) — 取代「本体不享有特权通道」的字面读法
- [0019 — 每禁一项脚本能力，必须提供确定性替代品或写明理由](decisions/0019-denied-capability-needs-substitute-or-justification.md) — 通则 + 现有拒绝清单逐项核实 〔脚本时代，历史记录〕
- [0020 — 脚本内部允许浮点](decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md) — 安全边界收拢到 `register_fn` 类型签名，部分取代 0002 〔脚本时代，历史记录〕
- [0021 — 抽象的理由是有算法要共用，不是看起来该对称](decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) — `Camera`/`BoundedCamera` 与 `compute_fov` 泛化不对称的裁定依据
- [0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 三个独立实例
- [0023 — 脚本状态写入必须经 `apply`](decisions/0023-script-state-writes-go-through-apply.md) — 不得直接写穿，细化 0006 在脚本层的具体应用 〔脚本时代，历史记录〕
- [0024 — 存档按可重算部分 + 偏差表达空间](decisions/0024-save-format-recomputable-vs-deviation-not-surface-vs-interior.md) — 分界是有无生成器，不是地表/室内
- [0025 — demo 交互验收禁止合成键盘事件（SendKeys）盲注](decisions/0025-demo-interaction-verification-forbids-sendkeys.md) — 改用程序化驱动同一调用路径
- [0026 — 账本纪律的第一次复发](decisions/0026-recurring-ledger-to-adr-discipline-lapse.md) — ADR 0013 立下三天后，又攒了 21 条未提升的裁定
- [0028 — Steel 引擎构造期的偶发内存破坏](decisions/0028-steel-engine-construction-memory-corruption.md) — 定位到上游缺陷，逐条否决五条假说（爆栈/并发/sync/升级/畸形脚本） 〔脚本时代，历史记录；本 ADR 是「为什么拆掉脚本系统」的证据链〕
- [0029 — 构造阶段先于编译阶段（约束 C6）](decisions/0029-engine-construction-phase-precedes-compilation.md) — 机器强制的引擎构造时序，作用域单位从脚本文件升格为 mod 〔**约束 C6 已废止**，见该 ADR 订正段〕

### 许可证

- [2026-08-16 P0 收尾扫描](licenses/2026-08-16-p0-scan.md) — 四项全过；含「没有远程就必须手工跑」的提醒
- [2026-08-17 文本渲染与字体许可证扫描](licenses/2026-08-17-text-font-license-scan.md) — 字体方案调研期的许可证核实
- [2026-08-18 `ll-mod` 引入 toml/serde 扫描](licenses/2026-08-18-ll-mod-toml-scan.md)
- [2026-08-18 `ll-text` 字体与图标字体资产入库](licenses/2026-08-18-ll-text-asset-import.md)
- [2026-08-18 P4 引入 `steel-core` 扫描](licenses/2026-08-18-steel-core-scan.md) 〔历史记录：`steel-core` 已移除，`deny.toml` 里由它带来的 MPL-2.0 白名单与四条 RUSTSEC 豁免一并删除〕
- [2026-08-19 `ll-content` 引入 postcard/lz4_flex 扫描](licenses/2026-08-19-ll-content-postcard-lz4-scan.md)

### 设计

- [设计文档总索引](design/README.md) — 十五份文档谁管什么、核心概念对照表、贯穿全局的设计原则、建议阅读顺序
- [物品系统](design/item-system.md) — 定义与实例分离、堆叠规则、归属、耐久、地面老化
- [装备栏位与占位掩码](design/equipment-slots.md) — 22 槽位；一条掩码规则同时覆盖双手武器与全身甲
- [角色属性系统](design/attribute-system.md) — DnD 六维骨架、三系攻防、四种穿透、衍生属性绝不入存档；伤害公式的 10% 下限为什么必须夹在最后一步
- [社会系统：归属、文化、聚落与地图结构](design/society-and-affiliation.md) — 势力/宗教/行会/文化/家族/职业共用一个 `Affiliation` 结构；LOD 按「是否被模拟」分层，不按距离分
- [Agent 目标与经济](design/agent-goals-and-economy.md) — 目标—需求—任务—悬赏循环、行会枢纽贸易把 O(n²) 变成 O(n)、背景/前景/具名三档精度与升格棘轮问题
- [种族系统](design/race-system.md) — 属性修正的烘焙时机、暗视接口与 FOV 对称性的关系、寿命的三条平衡手段、种族偏见的乘法分解
- [世界历史生成](design/world-history.md) — 历史生成期只模拟「被记住」的家族而非百万平民，世界生成时间的三条规避手段
- [身份与 ID 空间](design/identity-and-ids.md) — `ContentIndex`（类型）与 `WorldId`（实例）的分野、`WorldId` 永不复用的理由、存档必须分开记录生成期与当前 mod 集合
- [命名、改名与本地化](design/naming-and-localization.md) — 命名跟出生地文化走而非种族、改名走 `Effect`/`HistoricalEvent`、派生名与覆盖名两条不同规则
- [坐标系与空间模型](design/coordinate-system-and-layers.md) — 区块与两种坐标分辨率、`Space`（`Surface`/`Interior`）统一接口、地表连续流式加载、离散空间稀疏存储；**已完整落地**（P5 坐标系重写批次）
- [脚本层数据句柄与批量查询](design/script-entity-handles-and-batch-queries.md) — 脚本操作 Rust 侧数据的句柄语义、防伪造论证、批量查询原语清单〔脚本时代，历史记录〕
- [脚本状态存储](design/script-state-storage.md) — 脚本跨帧/跨存档持久化状态的受认可通道，`ScriptValue` 值类型系统、命名空间隔离、有界配额〔脚本时代，历史记录：`WorldState::global_script_state`/`Agent::script_state` 两个字段仍在存档格式里，但已无任何写入方〕
- [Steel 语法参考](design/steel-script-reference.md) — 在迷途大陆的 mod 沙箱里能写什么〔脚本时代，历史记录：语言本身已不在项目里〕
- [职业 / 技能树 / 副职 / 任务系统](design/class-skill-quest-system.md) — `ClassDef`/`SkillDef`/`SubclassDef`/`QuestDef` 四个内容注册表；`ClassDef`/`SkillDef` 已落地（P5-B）
- [三轴战斗结算](design/combat-three-axis.md) — 瞄准形状 × 伤害系别 × 投送方式三条正交轴，取代按武器类型分类实现的直觉方案
- [增益与通用触发器](design/buffs-and-triggers.md) — `ActiveEffect` 惰性到期判定、`TriggerDef`/`TriggerResponse` 通用触发器框架、堆叠策略三选一
- [设计文档交叉核对：冲突与缺口清单](design/conflicts.md) — 前五份文档间 4 条矛盾均已裁定、3 条设计缺口待补、2 条记录备查

### 审计

- [2026-08-17 文档—代码一致性审计报告](audit/2026-08-17-doc-code-audit.md) — 不只问「实现是否满足规格」，还问「规格是否已被实现淘汰」；哪些 CI 门禁是纪律保证、哪些是机器强制，逐条拆开
- [审计工单清单](audit/worklist.md) — 落在审计角色自身改不了的禁区（`.github/**`、`crates/**` 等）里的可直接照做工单
- [2026-08-19 过时标注整理清单](audit/2026-08-19-stale-annotation-sweep.md) — 上一轮整理：标注而非删改被取代的内容，含落地状态复核结果
- [2026-08-19 文档清理清单](audit/2026-08-19-doc-cleanup.md) — 本轮整理：方针改为删除过时内容，不再加注释；删了什么、改了什么、依据是什么
- [2026-08-26 社会/种族/冲突三份文档落地状态复核](audit/2026-08-26-society-race-conflicts-reverification.md) — 三份 2026-08-17 冻结的文档逐节对今天的代码复核：哪些仍成立、哪些前提已消失；`Affiliation` 形状变更与「有没有真实消费者」的逐项核对；同时充当「`CultureDef` 完整落地 + 关系派生基线」那一批的任务书（前置清单、建议顺序、待所有者裁决项）

### 阶段交接

- [P0 → P1 交接清单](handoff/p0-to-p1.md) — P1 第一天就会撞上的三处接口改动，以及最容易无声破坏确定性的那一处
- [P1 → P2 交接清单](handoff/p1-to-p2.md) — 渲染层已就绪的能力、visible_tiles 的世界尺寸下限，以及三次计划缺陷换来的方法论
- [P2 → P3 交接清单](handoff/p2-to-p3.md) — 三处 P3 一加实体就会暴露的潜伏缺陷、FOV 的对称性取舍，以及「反向核对规格」这条新纪律
- [P3 → P4 交接清单](handoff/p3-to-p4.md) — 三个真实缺陷带来的教训（`resolve` 从不读敏捷等）、一处靠数值约定挡着的结构性缺陷、P4 直接相关的设计决定
- [P4 → P5 交接清单](handoff/p4-to-p5.md) — 存档格式的一串前置债务、坐标系变更对存档格式的影响、两条「本体特权」缺陷
- [P5 → P6 交接清单](handoff/p5-to-p6.md) — P5 遗留给 P6 的真实缺口、跨表引用校验缺一个统一阶段、「没接线」失败模式的第六次实例

### 管线

- [文本与字体渲染管线](pipelines/text-and-font-rendering.md) — 字体方案从像素点阵改为思源黑体的调研过程、规格自身的排期矛盾

### 协作

- [分支与提交约定](workflow/branching.md)

## 维护规约

依据总纲规格 §13「代码卫生」：**过时的知识库文件必须删除或更新，不得留存**。一份写错的备忘比没有备忘更危险。

发现文档与代码不一致时，按缺陷处理。**例外**：`decisions/` 下的 ADR 记录的是历史决策本身，被取代的内容不删除，只在标题下方压缩成一行状态指针指向取代者，正文保持原样——ADR 存在的意义就是保留「当初为什么这么定、后来为什么改」。

## 相关文档

- [总纲设计规格](../docs/superpowers/specs/2026-08-16-lostland-design.md) — 全部架构决策的唯一真相源
