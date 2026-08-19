# P4 脚本层与 Mod 框架 实施计划

> **给执行者：** 必须配合 `superpowers:subagent-driven-development` 逐任务实施。步骤使用 `- [ ]` 复选框追踪。
> **并行提醒**：本计划的 `ll-script`/`ll-mod` 线（任务 3–9）与 `ll-text` 线（任务 10）互不依赖，可并行开工，符合规格「并行切分」一节的 crate 边界原则。**但并行代理必须各用独立 `git worktree`**——`knowledge/handoff/p3-to-p4.md` 第六节记录了本项目在共享工作树下三次提交撞车的真实教训，不是假设风险。

**目标：** 建立 `ll-script`（Steel VM 宿主、内存守卫、mod API 表面）与 `ll-mod`（发现、清单解析、依赖拓扑排序、内容注册表）两个新 crate；把 `TerrainKind` 的硬编码属性表迁入注册表，使本体地形与 mod 地形走同一条注册路径；把文本渲染地基（`cosmic-text` + `glyphon`）提前接入 `ll-render` 的 wgpu 管线；交付加载管理界面；修复 P3 交接遗留的 `action_cost` 下限缺陷；落地类型/实例分离（`WorldId`/`OrgInstance`）。

**架构：** 沿用 C1–C5（`apply` 唯一写入口、时间轴只装朴素数据、随机必经 `DetRng::for_entity`、后台推进必到确定 tick、禁止 `HashMap`/`HashSet` 迭代顺序参与逻辑判断）。C5 是 2026-08-18 才编号进规格 §4 的约束，本计划「有意排除的能力」表格里"无序容器的迭代顺序"一行、Task 5 的 `ordered.rs` 都是它在脚本层的具体落点，写计划时这条规则已经在设计里生效，只是当时还没有编号。本阶段新增的架构判断——也是贯穿全部任务的主线——是：**mod 沙箱的安全性靠「脚本 API 表面上不存在破坏确定性的能力」，不是靠「禁止脚本这样做」的规则**。见下方专门一节。

**技术栈：** `ll-core` + `ll-world` + `ll-sim`（已有）+ `steel-core` 0.8.2 + `cosmic-text` 0.19 + `glyphon` 0.12。

**规格：** [`docs/superpowers/specs/2026-08-16-lostland-design.md`](../specs/2026-08-16-lostland-design.md) §10、§15 P4 行
**上阶段交接：** [`knowledge/handoff/p3-to-p4.md`](../../../knowledge/handoff/p3-to-p4.md)
**相关设计：** [身份与 ID 空间](../../../knowledge/design/identity-and-ids.md)、[文本与字体渲染管线](../../../knowledge/pipelines/text-and-font-rendering.md)、[种族系统](../../../knowledge/design/race-system.md)、[世界历史生成](../../../knowledge/design/world-history.md)
**架构骨架：** [`docs/architecture/`](../../architecture/README.md) 九份，尤其 [`03-invariants.md`](../../architecture/03-invariants.md)（C1–C5）、[`07-determinism.md`](../../architecture/07-determinism.md)（哪些写法会破坏确定性）
**实测依据：** [ADR 0001 — Steel 沙箱能力实测](../../../knowledge/decisions/0001-steel-sandbox-verification.md)（P0 阶段，`steel-core` 0.8.2）
**serde 规矩：** [ADR 0011 — serde try_from 中转](../../../knowledge/decisions/0011-serde-try-from-bypasses-validating-constructors.md)

---

## 全局约束

- **世界状态禁止浮点。** `ll-text` 的排版结果（浮点、依赖字体库版本）**绝不回流入 `WorldState`**——只用于当帧绘制，理由见 [文本与字体渲染管线](../../../knowledge/pipelines/text-and-font-rendering.md) 1.3 节。
- **`apply` 是唯一写入口。** 脚本产出的只能是 `Intent` 或 `Effect`（数据），物理上摸不到 `&mut WorldState`——本阶段起这条边界要靠 Steel 宿主的 API 表面来保证，见下方专节。
- **`resolve` 是纯函数**，可并行。若 `resolve` 内部调脚本，脚本调用本身也必须满足这条纯函数约束（不持有跨调用可变状态）。
- 所有随机性来自 `DetRng::for_entity`，**禁止全局 RNG**（C3）。**脚本侧同样禁止**——不是靠约定，是靠脚本 API 表面根本不提供任何构造 RNG 的手段，见下方专节。
- **时间轴队列只能存纯数据**（C2）。
- **禁止让 `HashMap`/`HashSet` 的迭代顺序参与任何逻辑判断**（C5）。**脚本侧同样禁止**——`ordered.rs`（任务 5）只提供按稳定键排序的遍历原语，不给脚本任何裸遍历无序容器的手段，见下方专节「有序遍历」。
- 环面坐标只走 `TorusSize` 的方法。
- **新增的每一个「私有字段 + 校验构造函数」类型，加 serde 派生时都必须用 `try_from` 中转**——本阶段的 `TerrainKind` 迁移会正面撞上这条规矩与「反序列化需要注册表上下文」之间的张力，见任务 8。
- 所有公开项必须有文档注释；注释解释**为什么**。
- 测试 AAA 结构、测试名描述行为、一个测试只断言一件事。
- 文件 200–400 行为宜，800 行上限。
- 提交信息 `<type>: <描述>`，正文讲**为什么**，**不得含任何 AI 署名**。中文。
- 新增第三方依赖前先跑 `cargo deny check`——`steel-core`/`cosmic-text`/`glyphon` 均已在 `deny.toml` 白名单核实过（见规格 §3、[文本与字体渲染管线](../../../knowledge/pipelines/text-and-font-rendering.md) 2.1 节），不需要新增例外，但仍要跑一次确认。

---

## Mod API 设计：能力而非规则

这一节是本阶段最重要的架构决定，贯穿任务 3–9。**方向**：不设计成「约束脚本不许破坏确定性」（靠 mod 作者自律或 `tools/ll-datacheck` 事后检查），而设计成「破坏确定性这件事在 API 层面根本做不到」（脚本拿不到做这件事所需的能力）。理由很直接：项目无法审查未来出现的几千个 mod，但可以完全控制 Steel VM 里注册了什么函数——**没被注册的函数，脚本永远调用不到**。

### 有意排除的能力

| 排除项 | 现状 | 排除方式 |
|---|---|---|
| 系统时间、墙钟 | **未核实**：ADR 0001 只测试了中断/内存/参数检查/深递归，没有测过 `Engine::new()` 默认注册的标准库里是否含时间相关函数（例如 `steel/time` 模块，若存在）。这是本计划新增的、任务 3 第一步必须做的实测项，不是可以假设的结论 | 若默认标准库包含此类函数：`Engine` 构造时不加载对应模块，或显式 `shadow`/覆盖同名绑定为报错桩。若发现无法关闭：如实记入风险节，不假装解决了 |
| 文件系统、网络 | 同上，**未核实** | 同上 |
| 无序容器的迭代顺序 | 对应规格 §4 约束 C5。**未核实**：需确认 Steel 是否内置哈希表类型（如 `hash-map`）且是否对脚本暴露遍历原语 | 若存在且暴露：只注册 get/set 类函数，不注册遍历函数；任何需要脚本侧顺序遍历的场景改由 Rust 侧提供一个显式有序的关联列表/`for-each-sorted` 函数，脚本不持有可遍历的无序结构 |
| 非 `DetRng::for_entity` 的随机源 | Steel 本身大概率内置某种伪随机函数（多数 Lisp 系语言标准库都有），**未核实具体名称** | 不注册任何允许脚本自建随机流的函数。脚本能拿到的唯一随机通道见下方「确定性随机」——且这条通道**不给脚本传入种子的能力**，种子始终由宿主侧持有 |
| 跨帧隐式可变状态 | 规格 §4 约束 C1 已经写明这条要求 | Steel `Engine` 内部允许 `define` 全局可变绑定，若脚本在两次调用之间用全局变量攒状态（例如一个技能脚本自己维护一个计数器），这条状态不会被序列化进存档，读档后会丢失或错位。**排除方式不是禁止 `define`（做不到，那是 Steel 语言本身的能力），而是每次调用一个 mod 函数前，用一份全新的 `Engine` 或显式重置的求值环境**——ADR 0001 已确认「中断后同一 `Engine` 可复用，一键重载成本低」，这条实测结果同时说明重建/重置引擎的开销可控，是这条排除得以落地的前提 |

**没有一项排除是"已验证可行"的既定事实**——ADR 0001 的实测范围没有覆盖这些问题。任务 3 的第一步必须是一次新的探针（方法论与 ADR 0001 相同：先测，再定架构），把上表的"未核实"逐项核实清楚，核实结果写回本文档或新开一份 ADR。**若某一项排除在 Steel 0.8.2 上做不到，必须如实记入风险节，不能假装解决了转而依赖约定。**

### 提供的能力分类

「不给能力」要成立，「给的那部分必须够用」——否则 mod 作者做不成事，架构名存实亡。至少覆盖八类：

| 类别 | 内容 | 设计要点 |
|---|---|---|
| **确定性随机** | 包装 `DetRng::for_entity` | 脚本**看不到种子**，只能调用宿主在调用脚本前用当前上下文（实体 ID、事件计数）构造好的一个「已经派生完毕」的 RNG，通过类似 `(rng-next-u64)`/`(rng-gen-range lo hi)`/`(rng-chance permille)` 的函数消耗它。这个 RNG 句柄只在本次脚本调用期间有效，调用结束即丢弃——脚本物理上拿不到能跨调用持久化的随机流 |
| **世界只读查询** | 地形、实体属性、时间、光照、归属关系 | 全部是 `fn(&WorldState, ...) -> T` 形态的只读函数，与 `resolve` 本身的纯函数约束一致；**不注册任何返回 `&mut` 或允许原地修改的函数** |
| **产出 Intent / Effect** | 脚本的唯一「写」手段 | 脚本函数的返回值被 Rust 侧解析成 `Intent`/`Effect` 数据，脚本本身不直接调用 `apply`——这是 C1 在脚本层的具体体现，规格原文（§4）已把「脚本沙箱」列为这个架构一次性解决的问题之一 |
| **有序遍历** | 需要顺序参与逻辑判断的场景 | 只提供按稳定键排序的遍历原语（类似 `BTreeMap`/`Vec` 在 Rust 侧的角色），不提供无序容器的裸遍历，呼应上表「无序容器迭代顺序」的排除项 |
| **内容注册** | 职业、技能、物品、地形、种族等 | 本阶段只落地地形（`TerrainDef`，见任务 8）；其余类型的注册函数留给后续阶段按同一模式添加，本阶段负责把**注册管线本身**（发现→解析→加载→注册）搭好 |
| **i18n 文本查找** | 配合 Fluent | 脚本按翻译键取字符串，不允许脚本内嵌自然语言硬编码——否则会绕开规格 §11.3「无硬编码用户可见字符串」这条 CI 门禁背后的设计意图 |
| **日志与错误上报** | 面向加载管理界面 | 脚本内部的 `error`/断言失败需要能带着文件名、调用点信息冒泡到 Rust 侧，由加载管理界面分组展示（任务 11）。**Steel 的错误对象是否携带源码位置信息，本计划未核实**，见风险节 |
| **注册期元信息** | mod 自身的 `NamespacedId`、版本号 | 用于任务 7 的内容哈希与任务 9 的 mod 集合记录，不算「游戏玩法 API」，但同属「脚本能读到什么」的范畴 |

### 注册期 / 运行期分界线

这是整个 mod 架构里对性能影响最大的一条判断，必须在计划阶段正面回答，不能留到实现时才发现踩坑。

- **注册期**（mod 装载/热重载时，一次性）：Steel VM 被调用来"生产数据"——执行 `.scm` 文件里的注册调用（如 `(register-terrain "lostland:mountain" #:blocks-sight #t ...)`），Rust 侧的注册函数把返回值**物化成纯 Rust 结构**（`TerrainDef`）存进 `Registry`。这一阶段每次 VM 调用的开销完全可接受——只发生在装载或热重载时，不在任何逐帧/逐格的热路径上。
- **运行期**（每帧/每格高频路径）：地形属性查询（`blocks_sight`/`move_cost`）、FOV、寻路，**全部只读已经物化的 Rust 结构，不再进入 Steel VM**。这是这条分界线存在的理由——FOV/寻路是逐格调用的路径，哪怕一次跨 VM 调用的开销很小，乘以每帧成千上万次查询也会迅速不可接受。**注意**：ADR 0001 测的「看门狗零开销」（0.96x）是一个原子读探针的开销，与「一次完整的跨 VM 函数调用（含参数编解码、返回值解析）」是完全不同量级的操作，不能把前者的实测结论直接套用到后者头上——这是本计划的推断，不是实测结论。
- **唯一允许运行期真正调用进 VM 的场景**：脚本产出 `Intent`/`Effect` 的场景（P5 技能效果、P8 AI 决策）——这类判断本来就无法预先物化（"这一刻这个实体该做什么"），调用开销靠"时间轴回合制天然一次只处理一个行动者"来摊销，不是每帧对全体实体调用。**P4 本身没有这类需求**——本阶段唯一的注册内容（`TerrainDef`）完全落在注册期——但 `ScriptEngine`（任务 3）的 API 需要同时支持这两种调用模式，因为 P5 马上就要用到运行期模式。

**这条判断本身未经性能实测**，是基于 ADR 0001 的间接推断，不是量化结论。**建议**：任务 3 的探针阶段顺带测一次"单次跨 VM 函数调用往返延迟"这个具体数字（哪怕是一个粗略数量级），把"注册期可接受、运行期必须物化"这个设计判断落到可验证的数字上，而不是停留在直觉推断。若实测发现运行期偶发调用其实开销可控，不代表这条分界线设计错了——"物化到 Rust 结构"仍是更简单、更不容易踩确定性坑的默认路径，只是意味着未来某些场景可以豁免，这类豁免应作为后续阶段的具体决定，不在本计划预先开口子。

### 「本体即 Mod」升格为验收手段

规格 §10.3 的原文是公平性原则；本计划把它升格为一条**强制检验**：

> 如果 mod API 缺了什么能力，本体自己也做不出来——我们在开发期就会撞上这道墙，而不是等 mod 作者事后抱怨。

具体落点：

1. **任务 8（`TerrainKind` 迁入注册表）是这条检验第一次真正生效**。它当前是硬编码 `match`（P2 遗留），迁移后必须走与 mod 地形**完全相同**的注册路径（发现同一目录结构、解析同一清单格式、走同一个 `Registry::intern` 调用）。**若某个本体地形属性无法通过公开 API 表达，这是 API 缺陷信号，要修 API，不是给本体开后门。**
2. **任务 12（验收 demo）要求本体内容与 mod 内容在注册表内部结构上不可区分**——除了命名空间字符串本身（`lostland:*` vs `examplemod:*`），`Registry` 不应该有任何字段或逻辑分支用来识别"这是不是本体注册的"。这是可以直接写成测试断言的一条不变式。
3. 计划明确写下：**任何本体内容若发现无法只用公开 API 实现，必须视为 API 缺陷去修 API，不允许开一条特权通道绕过去。** 这条纪律比"公平"更实际的价值在于：它把"mod API 是否够用"这个原本要等 mod 生态成熟后才会暴露的问题，提前到本体自己开发期就会撞见。

---

## 可依赖的既有 API（已照当前代码核实）

- `ll_core::ident`：`NamespacedId::{parse, namespace, path}`、`ContentIndex::get`、`Interner::{new, intern, resolve, len, is_empty}`——**`Interner` 内部哈希表不可遍历**，只能走 `to_id`（已有此不变式，本阶段新代码必须遵守同一纪律）
- `ll_core::rng`：`DetRng::{for_entity, next_u64, gen_range, chance}`
- `ll_core::error`：`CoreError { InvalidIdentifier, DivisionByZero, Overflow }`——本阶段新增的错误类型（`ScriptError`/`ModError`）应参照这个模式，面向开发者，不走 i18n
- `ll_world::terrain::TerrainKind`：17 个常量，`blocks_sight`/`blocks_move`/`move_cost`/`is_known`——任务 8 的迁移对象，**已确认 13 个文件引用 `TerrainKind::` 常量**（`ll-world`：`chunk.rs`、`fov.rs`、`generate.rs`、`terrain.rs`、`tests/fov_blackbox.rs`、`examples/p2_acceptance/{layout,spawn}.rs`；`ll-sim`：`apply.rs`、`resolve.rs`、`tests/replay.rs`、`examples/p3_acceptance/{layout,turn}.rs`），迁移前必须重新 `grep` 一次确认现状未变
- `ll_world::entity::affiliation::{Affiliation, AffiliationKind}`：`org: ContentIndex` 字段，任务 1 的修正对象
- `ll_sim::intent::Intent`、`ll_sim::effect::Effect`、`ll_sim::apply::apply`、`ll_sim::timeline::{Timeline, action_cost}`：P3 已交付，任务 2 修改 `action_cost`
- `ll_render`：`GpuContext`/`RenderTarget`（`crates/ll-render/src/target.rs`）——任务 10 要接入的既有 wgpu 管线，**接入方式未经验证**，见任务 10 风险说明

---

### Task 1：类型/实例分离——`WorldId`、`OrgInstance` 落地，修正 `Affiliation.org`

**必须最先做。** 理由：本阶段第一件事就是建内容注册表，注册表要处理的对象里既有"类型"（文化、职业、种族，走 `ContentIndex`）也有"实例"（势力、家族、聚落，走 `WorldId`）——若不先把这条分界线钉死，注册表接口的形状从一开始就是错的，后续任务会在错误的假设上继续搭建，返工面积随时间只会更大。

**Files:** `crates/ll-core/src/ident.rs`（新增 `WorldId`）、`crates/ll-world/src/entity/affiliation.rs`（修正 `org` 字段）、新增 `crates/ll-world/src/entity/org.rs`（`OrgInstance`）

**Interfaces Produces:**
```rust
// ll-core/src/ident.rs
/// 世界生成实例的持久标识。永不复用，不需要代际号——
/// 历史事件要求即便指向的对象已消亡，引用依然能正确解析。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldId(u32);
impl WorldId {
    pub fn next(counter: &mut u32) -> Self; // 单调递增，不回收
}
```

```rust
// ll-world/src/entity/org.rs
/// 一个组织实例——势力/宗教/行会/家族在世界生成期间被创造出来的具体
/// 个体，与它所属的 Def（若有）分开。
pub struct OrgInstance {
    pub id: WorldId,
    pub def: Option<ContentIndex>,   // 源自哪个 mod 模板，纯生成则 None
    pub authored: Option<NamespacedId>, // mod 直接命名的具体实例才有
}
```

```rust
// affiliation.rs：org 字段按 AffiliationKind 分叉
pub enum OrgRef {
    /// 类型：文化、职业——mod 装载时确定，集合封闭。
    Def(ContentIndex),
    /// 实例：势力、宗教、行会、家族——世界生成期间创造，数量随世界增长。
    Instance(WorldId),
}
pub struct Affiliation {
    pub kind: AffiliationKind,
    pub org: OrgRef,   // 原为 ContentIndex
    pub standing: i32,
}
```

> **设计判断说明**：`identity-and-ids.md` 原文只裁定了"势力/家族/聚落"这几种 `AffiliationKind` 应该用 `WorldId`，未裁定`Culture`/`Profession` 是否保留 `ContentIndex`，也未给出 `Affiliation.org` 具体怎么改的实现形状（原文："具体迁移方式属于实现批次的工作，本文档只定形状"）。本任务的实现判断是：用一个 `OrgRef` 枚举包住两种可能，而不是给 `Affiliation` 拆成两个并列字段——理由是 `AffiliationKind` 本身已经决定了 `org` 该是哪一种，枚举把这条"由 kind 决定 org 的具体类型"的约束显式表达出来，调用方 `match` 一次就能拿到正确类型，不需要"看 kind 再决定该读哪个字段"这种隐式约定。**这是本任务自己的设计判断，不是原文档裁定的形状，若与项目所有者的预期不符，可在评审时调整。**

- [ ] **TDD 循环**，测试至少覆盖：
  - `WorldId 单调递增不重复`
  - `WorldId 用尽已分配区间后继续递增而非回绕`（正常范围内不触发，构造边界情形验证逻辑）
  - `Culture 归属的 org 是 Def 变体`
  - `Faction 归属的 org 是 Instance 变体`
  - `OrgInstance 的 authored 字段区分 mod 命名实例与纯生成实例`
- [ ] **提交**（改动波及 `ll-world` 内所有构造 `Affiliation` 的调用点——先 `grep -rn "Affiliation {" crates/` 摸清改动面，再动手）

---

### Task 2：`action_cost` 下限钳位

**独立、低风险、可与 Task 1 并行**，处理 P3 交接清单记录的结构性缺陷：`action_cost` 在有效速度超过 100000（敏捷超过 1000）时整数除法截断为 0，导致该实体在同一 tick 无限重排、耗尽 `MAX_STEPS_PER_ADVANCE`。当前只靠数值约定（种族修正 ±2~4）挡住，P4 引入 mod 后任意 mod 可以给敏捷叠加到任意值，约定会失效。

**Files:** `crates/ll-sim/src/timeline.rs`

**改动：**
```rust
pub fn action_cost(base_cost: u32, effective_speed: u32) -> u32 {
    let speed = u64::from(effective_speed.max(1));
    let cost = u64::from(base_cost) * 1000 / speed;
    (cost as u32).max(1)   // 新增：下限钳位，防止极端敏捷值下的无限重排
}
```

- [ ] **TDD 循环**：
  - `极端有效速度下行动代价不为零`（构造 `effective_speed = 200_000` 这类会让原公式截断为 0 的输入，断言结果 `>= 1`）
  - 既有测试（`行动代价公式在基准敏捷下等于基础代价`等）保持通过，确认这条钳位不影响正常数值区间
- [ ] **提交**（`fix:`，说明这条缺陷此前只靠数值约定挡住，mod 生态会让约定失效）

---

### Task 3：`ll-script` crate 骨架——标准库范围实测、错误边界

**Files:** 创建 `crates/ll-script/{Cargo.toml, src/lib.rs, src/host.rs, src/probe.rs}`

**第一步（先测再建，方法论同 ADR 0001）**：`probe.rs` 写一组一次性探针，核实「Mod API 设计」一节表格里标注"未核实"的每一项：

- `Engine::new()` 默认注册的标准库是否包含时间/文件系统/网络相关函数？（若 Steel 有模块化 prelude 机制，是否可以选择性不加载）
- 是否内置哈希表/无序容器类型？是否对脚本暴露遍历原语？
- 是否内置某种伪随机函数？名称与语义？
- 单次跨 VM 函数调用（含参数编解码、返回值解析）的往返延迟量级（哪怕只是粗略数量级，不追求精确基准）？
- `Engine` 求值环境重置/重建的实测开销（ADR 0001 已确认"中断后同一 Engine 可复用"，本次要验证的是"显式重置全局绑定"这个更强的操作）

**探针结果必须写回本文档（追加一节）或新开一份 ADR**，不得凭空假设。

**Interfaces Produces（探针结果确认后再定稿，以下是概念形状）：**
```rust
pub struct ScriptEngine { /* 包装 steel::steel_vm::engine::Engine */ }
pub enum ScriptError {
    Interrupted,          // 超时或超内存中断
    ArityMismatch(String),
    Runtime(String),
    ParseError(String),
}
impl ScriptEngine {
    pub fn new() -> Self;
    pub fn load_source(&mut self, source: String) -> Result<(), ScriptError>;
    /// 每次脚本调用都经过这里，是四道防线①②的落点：
    /// 出错返回 Err，不 panic；调用方决定降级默认值。
    pub fn call<T>(&mut self, name: &str, args: &[ScriptValue]) -> Result<T, ScriptError>;
}
```

> **四道防线①②在这里落地**：每次脚本调用包裹错误边界（`call` 签名本身就是 `Result`）；降级而非崩溃——这是**调用方**的责任（`call` 返回 `Err` 后，调用方决定用什么默认值），本任务不替调用方做这个决定，只保证"出错一定拿到 `Err`，不会 panic"。

- [ ] **TDD 循环**（覆盖 ADR 0001 已实测的能力，确认包装层没有引入回归）：
  - `死循环脚本返回 Err 而非 panic`
  - `Engine 中断后可以继续处理下一次调用`
  - `注册函数缺参返回 ArityMismatch`
  - `语法错误的源码返回 ParseError 而非 panic`
- [ ] **提交**（探针结果单独一次 `docs:` 提交，代码实现另一次）

---

### Task 4：`ll-script` 内存守卫

**Files:** `crates/ll-script/src/alloc_guard.rs`

依赖 ADR 0001 的结论：内存是唯一原生缺口（时间维度已由 `InterruptHandler` 覆盖，"零开销"已实测）。

**Interfaces Produces:**
```rust
/// 包装系统分配器，用原子计数器统计脚本执行期间的分配量。
pub struct ScriptAllocGuard<A>(pub A);
unsafe impl<A: GlobalAlloc> GlobalAlloc for ScriptAllocGuard<A> { /* alloc/dealloc 各一次原子加减 */ }

pub fn set_memory_budget(bytes: usize);
pub fn reset_alloc_counter();
/// 超阈值时调用 ThreadStateController::interrupt()——复用 ADR 0001
/// 确认的同一条中断通道，不是另开一条机制。
```

- [ ] **TDD 循环**：
  - `分配量超过预算后触发中断`
  - `预算内的正常脚本不受影响`
  - `计数器在两次调用之间正确重置`（否则前一次调用的分配量会误伤下一次）
- [ ] **提交**

---

### Task 5：`ll-script` mod API 表面

**Files:** `crates/ll-script/src/api/{rng.rs, query.rs, intent.rs, ordered.rs, log.rs}`

落地"Mod API 设计"一节列出的能力分类中，本阶段实际要用到的几类（内容注册用的函数留给任务 7 按需添加，不在本任务预先造好）：

- `rng.rs`：`(rng-next-u64)`/`(rng-gen-range lo hi)`/`(rng-chance permille)`——**脚本拿不到种子**，这些函数背后的 `DetRng` 实例由宿主在调用前用 `DetRng::for_entity(种子, 实体ID, 事件计数)` 构造好，脚本只能消耗，不能重新构造
- `query.rs`：地形/时间/光照等只读查询函数（`fn(&WorldState, ...) -> T`）
- `intent.rs`：脚本返回值 → `Intent`/`Effect` 的解析层
- `ordered.rs`：有序遍历原语，脚本不接触任何无序容器的裸遍历
- `log.rs`：脚本内部错误/断言失败上报，携带来源信息（**具体能带多少信息取决于任务 3 探针对"错误对象是否含源码位置"的核实结果**）

- [ ] **TDD 循环**：
  - `脚本连续两次调用 rng-next-u64 得到不同值`
  - `相同实体相同事件计数的两次独立调用得到相同的随机序列`（确定性核心断言）
  - `脚本无法访问未注册的函数名`（确认排除清单里没注册的能力确实不可达——用一个刻意尝试调用不存在函数的脚本，断言返回 Err 而非某种意外成功）
  - `有序遍历结果与输入顺序无关，只由排序键决定`
- [ ] **提交**

---

### Task 6：`ll-mod` crate 骨架——发现、清单解析、依赖拓扑排序

**Files:** 创建 `crates/ll-mod/{Cargo.toml, src/lib.rs, src/discover.rs, src/manifest.rs, src/topo.rs}`

四道防线第④条（加载分阶段隔离）从这里开始：发现与解析阶段互相独立，一个 mod 的清单解析失败不影响其他 mod 被发现和尝试解析。

**清单格式：本任务采用 TOML**——规格没有明文规定 mod 清单格式，这是本任务的假设，理由是与规格 §11.1"用户设置 TOML"同一类"给人手改的元数据"场景，且不需要先起 Steel VM 就能解析出依赖关系用于拓扑排序（清单本身不该依赖脚本求值）。**这是实现假设，不是规格裁定**，若与预期不符可在评审时调整。

**Interfaces Produces:**
```rust
pub struct ModManifest {
    pub id: NamespacedId,       // mod 自己的命名空间
    pub version: String,
    pub dependencies: Vec<String>,
    pub entry_points: Vec<PathBuf>,  // .scm 文件列表
}
pub enum ModError { Io(...), ParseError(...), CyclicDependency(Vec<NamespacedId>), MissingDependency(NamespacedId) }

pub fn discover_mods(root: &Path) -> Vec<PathBuf>;
pub fn parse_manifest(path: &Path) -> Result<ModManifest, ModError>;
/// 拓扑排序失败（成环/缺依赖）时返回具体是哪些 mod，供加载管理
/// 界面（任务 11）逐条展示，而不是一个笼统的"加载失败"。
pub fn topo_sort(manifests: &[ModManifest]) -> Result<Vec<usize>, ModError>;
```

- [ ] **TDD 循环**：
  - `发现目录下的所有子目录候选`
  - `缺少必填字段的清单解析失败`
  - `依赖成环时拓扑排序报告具体环路`
  - `依赖缺失时拓扑排序报告缺失的具体 mod`
  - `无依赖关系的多个 mod 按发现顺序排序`（稳定性要求，不能依赖文件系统遍历的不确定顺序——**这里要小心**：目录遍历顺序本身在不同操作系统上可能不同，若拓扑排序结果依赖这个顺序，就是新的确定性风险，测试要覆盖"发现顺序打乱后排序结果仍然一致"这条不变式，必要时按 mod ID 字典序做稳定化）
- [ ] **提交**

---

### Task 7：内容注册表核心

**Files:** `crates/ll-mod/src/registry.rs`

依赖 Task 3–6。这是"能力而非规则"落地的枢纽：注册表本身不区分"这是本体注册的还是 mod 注册的"，只认命名空间字符串。

**Interfaces Produces:**
```rust
pub struct Registry {
    interner: Interner,          // 复用 ll-core，不重新发明
    content_hash: HashMap<String, u64>,  // 按 mod 命名空间统计贡献内容的哈希
}
impl Registry {
    pub fn new() -> Self;
    /// 注册期调用——由 ll-script 的注册函数在 Steel 求值时触发。
    pub fn intern(&mut self, id: NamespacedId) -> ContentIndex;
    pub fn resolve(&self, index: ContentIndex) -> Option<&NamespacedId>;
    /// 供加载管理界面与存档头使用：本次装载会话里，某个 mod 命名空间
    /// 贡献的全部内容的哈希——版本号相同但内容变了时用于警告。
    pub fn content_hash_of(&self, namespace: &str) -> Option<u64>;
}
```

**为 P5 预留的接口（本阶段只留形状，不实现完整存档集成）：**
```rust
/// ContentIndex ↔ 字符串 ID 映射快照。P5 存档格式落地时，存档头
/// 需要写出这份快照，读档时用它把索引换回字符串，再按当前 mod
/// 加载顺序重新 intern。P4 只需要保证 Registry 能产出/消费这份
/// 快照，不需要真正接入存档读写（存档格式 P5 才冻结）。
pub fn snapshot(&self) -> Vec<NamespacedId>;   // 按 ContentIndex 顺序
pub fn rebuild_from(snapshot: &[NamespacedId]) -> Self;
```

- [ ] **TDD 循环**：
  - `同一命名空间字符串重复注册返回相同索引`（复用 `Interner` 已有的这条不变式）
  - `不同 mod 的相同路径名不冲突`（命名空间前缀天然隔离）
  - `content_hash 随注册内容变化而变化`
  - `snapshot 与 rebuild_from 往返后索引对应关系不变`（在同一份 snapshot 顺序下）
- [ ] **提交**

---

### Task 8：`TerrainKind` 迁入注册表——「本体即 Mod」的第一次验收

**这是本计划改动面最大、风险最高的任务。** 依赖 Task 3–7。

**范围确认（先做，不要假设）**：`grep -rn "TerrainKind::" crates/` 摸清当前真实改动面——写这份计划时确认是 13 个文件（见"可依赖的既有 API"一节列表），实现前必须重新跑一次这个 grep，以当时结果为准。

**核心设计问题：`TerrainKind::MOUNTAIN` 这类编译期常量如何在"数值由注册期加载顺序决定"的世界里继续存在？**

`ContentIndex` 是运行时 `Interner::intern` 调用产生的（`to_id.len()`），根本不可能是 `const`。若把 `TerrainKind` 完全换成裸 `ContentIndex`，全部 13 个文件里的 `TerrainKind::MOUNTAIN` 字面用法都会编译失败。

**建议做法**：保留本体注册顺序固定这条约束——**本体内容永远先于任何 mod 注册，且总是按 `scripts/terrain.scm` 里固定的文件顺序注册**（这本身就是"发现→解析→拓扑排序→加载"这条管线里"本体是一个没有依赖的特殊 mod，永远排第一个"的自然结果，不需要额外特殊逻辑）。在此前提下，本体地形的 `ContentIndex` 值在同一次进程运行内是稳定的，可以在注册完成后一次性物化成一个运行时结构：

```rust
/// 本体地形的 ContentIndex，注册完成后一次性物化，不再是编译期常量。
/// 调用方从 WorldState（或专门的上下文）持有的 Registry 里取，
/// 不能再写 TerrainKind::MOUNTAIN 这种字面量。
pub struct BaseTerrainIds {
    pub deep_water: ContentIndex,
    pub shallow_water: ContentIndex,
    // …… 对应现有 17 个常量
}
pub struct TerrainDef {
    pub id: NamespacedId,
    pub blocks_sight: bool,
    pub blocks_move: bool,
    pub move_cost: u32,
}
```

`TerrainKind` 本身收缩成 `ContentIndex` 的一个 newtype（或直接消失，各处改用 `ContentIndex` + 一个 `terrain_defs: &HashMap<ContentIndex, TerrainDef>` 查表）。**具体是保留 `TerrainKind` 包一层还是直接换成 `ContentIndex`，本计划不预先拍板**——这属于实现批次要做的接口设计判断，取决于 13 个调用点里哪种改法侵入性更小，留给执行者在动手前先写一份两种方案的对照，评估工作量再定。

**serde 反序列化的 context 张力（必须正面处理，不能回避）**：ADR 0011 要求"任何私有字段+校验构造函数的类型，加 serde 必须用 `try_from` 中转"，且 `TerrainKind` 现在的校验标准（`is_known`）就是靠这个模式做的。但 `try_from` 是**无上下文的静态函数**——它拿不到"当前注册表里到底注册了哪些地形"这个运行时状态,而"迁入注册表后校验标准应改为『是否已注册』"（ADR 0011 原文已经预告了这个待办）恰恰需要这个运行时状态。

**这个张力没有在任何现有文档里被解决，本计划也不代为一次性拍板**，但给出一个建议方向、并标注为**待验证**：参考 ADR 0011 案例三（`WorldState` 的 `size`/`terrain` 交叉校验）的模式——**不试图让单个字段的 `try_from`拿到注册表上下文，而是把地形校验也做成"先落地未校验值，再在更外层做一次显式交叉校验"**。具体到地形，可能的形状是：反序列化阶段只还原成一个不做已知性校验的裸索引，`ChunkGrid`/`WorldState` 整体反序列化完成后，调用方显式传入当前 `Registry` 做一次批量校验（"这份存档里出现的每一个地形索引，是否都能在当前注册表里 resolve 成功"），不通过则整体判定存档不兼容。**这个方向未经代码验证，任务开始时应该先花小半天写一个最小可行的原型验证可行性，验证不通过就换方案，不要在没验证的前提下把大改动铺开。**

- [ ] **TDD 循环**（在确定具体接口形状后填充，至少覆盖）：
  - 既有 17 个地形常量的全部现有测试（`blocks_sight`/`blocks_move`/`move_cost` 各条断言）迁移后行为不变
  - `本体地形通过与 mod 地形完全相同的 Registry::intern 调用路径注册`
  - `未在当前注册表里出现的地形索引，反序列化/校验时被拒绝`
  - `本体地形与 mod 注册的自定义地形在 Registry 内部结构上不可区分`（除了命名空间字符串本身）——这是「本体即 Mod」验收手段的直接断言
- [ ] **提交**（这次改动面大，建议按"新增 `BaseTerrainIds`/`TerrainDef` 基础设施"与"迁移全部调用点"拆成两次提交，即便在同一个任务里完成）

---

### Task 9：mod 集合双记录接口

**Files:** `crates/ll-mod/src/mod_set.rs`

落地 [身份与 ID 空间](../../../knowledge/design/identity-and-ids.md) "存档与 mod 集合"一节最要紧的一条：**生成期 mod 集合**（世界由哪批 mod 生成，写入后永久不变）与**当前 mod 集合**（玩家现在开着哪批）必须分开记录，不能只存一份——否则种子分享、缺陷复现、回归测试三者失效，而这个区分一旦等到 P5 存档格式冻结后再补，就是一次追不回旧档的存档迁移。

**本阶段范围诚实说明**：P4 还没有世界生成器（P7 才落地），"生成期 mod 集合"目前没有真正的生成事件可以绑定。本任务只做**类型层面的区分**，不接入任何真实的存档读写（存档格式本身在 P5 冻结）：

```rust
/// 一次 mod 装载的快照：命名空间、版本、内容哈希（见 Task 7）。
pub struct ModSetEntry { pub id: NamespacedId, pub version: String, pub content_hash: u64 }

/// 类型层面强制区分两种集合，不允许把它们混进同一个 Vec——
/// 调用方必须显式说明"这是生成期的还是当前的"，编译期即可发现
/// 混用。真正的"世界生成时刻"绑定逻辑留给 P7。
pub struct GenerationModSet(pub Vec<ModSetEntry>);
pub struct CurrentModSet(pub Vec<ModSetEntry>);
```

- [ ] **TDD 循环**：
  - `当前装载的 mod 集合可以从 Registry 派生出 CurrentModSet`
  - `GenerationModSet 与 CurrentModSet 是不同类型，无法互相赋值`（编译期约束，用一个"故意尝试混用会编译失败"的说明性注释/doctest 记录意图，而不是运行时测试）
- [ ] **提交**（`docs:` 里说明这是接口占位，真正的"生成期"语义要等 P7 世界生成落地才有意义，避免读者误以为这是完整实现）

---

### Task 10：`ll-text` 地基——`cosmic-text` 排版 + `glyphon` 栅格化接入 wgpu

**可与 Task 3–9 并行**（不同 crate，无依赖关系），**风险最高的候选之一**：[文本与字体渲染管线](../../../knowledge/pipelines/text-and-font-rendering.md) 第 8 节明确标注"`cosmic-text`/`glyphon` 与本项目现有 `wgpu` 管线具体怎么接、要不要独立一个渲染目标——完全未实现、未验证"。

**Files:** 创建 `crates/ll-text/{Cargo.toml, src/lib.rs, src/layout.rs, src/render.rs}`

**范围边界（明确写死，避免范围蔓延）**：本任务只做"文字能不能画出来"这一层地基——排版整形、断行、字形回退、栅格化到 wgpu。**不做**九宫格切片边框、焦点导航、菜单/设置控件——那些是 P7 的"完整像素 UI 控件库"，规格 §15 已经把这条边界写清楚。

**技术方案**：
- 字体：思源黑体 Source Han Sans（Adobe 发行版，OFL-1.1），Regular + Bold 两档（约 16.2MB），打包进 `assets/`
- 排版：`cosmic-text::FontSystem` + `Buffer`
- 栅格化：`glyphon`（官方 `cosmic-text` 配套的 wgpu 渲染后端）
- **原生分辨率，不参与 640×360 世界层的整数缩放管线**——独立于 `ll-render` 现有的 `RenderTarget`，需要新建一个原生分辨率的渲染目标，与世界层画布分开合成到最终 surface

**Interfaces Produces（首次接入，接口可能需要在实现中调整）：**
```rust
pub struct TextRenderer { /* 持有 glyphon::TextRenderer + FontSystem */ }
impl TextRenderer {
    pub fn new(gpu: &GpuContext) -> Result<Self, TextError>;
    pub fn layout(&mut self, text: &str, max_width: f32) -> LayoutResult;
    pub fn render(&mut self, gpu: &GpuContext, target: &wgpu::TextureView, runs: &[TextRun]);
}
```

- [ ] **Step 1：最小可行验证**——不追求完整 API，先跑通"一段包含中英文的字符串被排版并画到屏幕上"这一条最短路径，实测截图作为证据，写进提交信息或知识库（呼应 [文本与字体渲染管线](../../../knowledge/pipelines/text-and-font-rendering.md) 4.1 节④"未核实小字号可读性"，本任务是第一次拿到真实渲染结果去验证的机会）
- [ ] **TDD 循环**（在最小可行验证跑通后补充）：
  - `中英文混排的断行位置符合字体度量`（用 `insta` 快照测试排版结果，而非渲染像素——像素级快照对字体/驱动版本太敏感）
  - `排版结果不写回 WorldState`（静态检查/代码审查项，不是运行时测试能覆盖的，需要在评审清单里显式列出）
- [ ] **提交**
- [ ] **必须如实报告**：接入过程中哪些假设成立、哪些不成立，与 P3 验收 demo 同样的纪律——"实测了什么、没测什么"要写清楚，不能因为是地基工作就放松这条要求

---

### Task 11：加载管理界面

**Files:** 待裁定 crate 归属，见「待裁定」一节第 1 条；本计划暂按 `crates/ll-mod/src/console.rs` + 调用 `ll-text` 起草，标注为可推翻的临时方案

依赖 Task 6（发现/解析结果）、Task 7（注册结果与内容哈希）、Task 3（脚本错误）、Task 10（文本渲染）。

规格 §10.6 要求：加载按阶段推进（发现→解析清单→依赖拓扑排序→加载脚本→注册内容→交叉引用校验）；结果分组显示（已加载/有警告/失败）；可展开查看含文件名与行号的详细错误；支持单个 mod 一键重载；开发期文件监听、`.scm` 存盘即生效。

**本任务的诚实范围**：
- **分组显示、可展开错误详情**：做，这是核心交付物
- **文件名与行号**：**取决于 Task 3 探针结果**——若 Steel 错误对象不携带源码位置，只能显示到"哪个文件加载失败"这个粒度，做不到行号级别。如实报告，不能因为规格写了"含行号"就假装实现了
- **单个 mod 一键重载**：做最小可行版本——重新对该 mod 跑一次"解析→加载→注册"，不要求做成绑定快捷键的完整交互，验收 demo 里用一个按键触发即可
- **开发期文件监听、存盘即生效**：**列为本阶段不做**，记入"有意留给后续阶段的缺口"一节——这是一个独立的、需要引入文件系统监听库的功能，与本阶段核心交付物（能不能把错误显示出来）不是同一优先级，勉强塞进本任务只会拖慢核心路径

**Interfaces Produces:**
```rust
pub enum LoadStatus { Loaded, Warning(String), Failed(LoadError) }
pub struct LoadError {
    pub mod_id: NamespacedId,
    pub stage: LoadStage,      // 发现/解析/加载/注册/交叉引用校验，哪个阶段失败
    pub message: String,
    pub location: Option<SourceLocation>,  // 有则显示，无则如实留空
}
pub struct LoadReport { pub entries: Vec<(NamespacedId, LoadStatus)> }
pub fn render_load_report(text: &mut TextRenderer, report: &LoadReport, expanded: &HashSet<NamespacedId>);
```

**mod 内容不兼容 vs schema 版本不兼容分开报错**：[身份与 ID 空间](../../../knowledge/design/identity-and-ids.md) "④"一节要求这两类失败用词分开——P4 阶段还没有真正的存档 schema 迁移（P5 才有），但 `LoadStage`/`LoadError` 的设计现在就要把"mod 内容问题"与"其他类型问题"分成不同的错误分支，为 P5 接入 schema 版本迁移错误时留出插入点，不要用一个笼统的 `String` 错误信息把所有失败原因混在一起。

- [ ] **TDD 循环**：
  - `失败的 mod 归入 Failed 分组而不影响其他 mod 的加载结果`
  - `展开状态与折叠状态渲染不同的详细程度`
  - `一键重载后该 mod 的状态被刷新，不影响其余 mod 状态`
  - `LoadError 的 stage 字段能区分至少发现/解析/注册三个阶段`
- [ ] **提交**

---

### Task 12：P4 验收 Demo

**Files:** `crates/ll-mod/examples/p4_acceptance/`（或按 Task 11 crate 归属裁定调整路径）

必须展示：

1. **一个真实的 mod 目录**（独立于 `scripts/` 本体内容，例如 `mods/example_mod/`）被发现、解析清单、依赖拓扑排序、加载
2. **该 mod 注册的内容出现在游戏里**——沿用 Task 8 的地形注册路径，mod 注册一个自定义地形（例如 `examplemod:lava_floor`），demo 世界里能查到它的正确属性（阻挡/不阻挡、移动代价）
3. **本体地形与 mod 地形走同一条路径**——demo 里用一个断言或界面提示证明这一点（例如打印"注册表共 N 项，其中本体 17 项、mod 1 项，二者除命名空间外结构相同"）
4. **一个故意写错的脚本**（语法错误或调用未注册函数）被加载时捕获为 `Err`，**进程存活**，错误出现在加载管理界面的"失败"分组
5. **加载管理界面**分组显示已加载/警告/失败，且失败项可展开
6. 若字体/文本管线已就绪，界面文字用思源黑体渲染（不再是 P3 demo 的 4×6 点阵占位）；**若 Task 10 未能在验收 demo 前完全接入 wgpu，如实标注这一点并用占位字体退化展示，不得假装接上了**
7. F2 存图作为视觉回归基准，Esc 退出（沿用 P3 demo 约定）

**必须实测**：跑起来、无 wgpu validation error、拿到非全黑渲染结果，**如实报告哪些验证了、哪些没有**——这是 P3 交接清单第一节反复强调的纪律：单元测试各自绿不代表连线通，只有真正跑起来才会暴露断链。

- [ ] **提交**

---

## 自查

### 完整调用链（P1/P3 教训要求的一节）

```
mods/example_mod/manifest.toml + lava.scm
  ↓
discover_mods(root) -> Vec<PathBuf>                              ← ll-mod::discover（Task 6）
  ↓
parse_manifest(path) -> Result<ModManifest, ModError>            ← ll-mod::manifest（Task 6）
  ↓ 与本体 scripts/terrain.scm 一起参与
topo_sort(manifests) -> Result<Vec<usize>, ModError>             ← ll-mod::topo（Task 6）
  ↓ 本体永远排第一（无依赖的特殊 mod）
按排序结果逐个：
  ScriptEngine::load_source(scm 文件内容) -> Result<(), ScriptError>  ← ll-script::host（Task 3）
    ↓ 每次调用包裹错误边界（四道防线①②）
    ↓ 内存守卫在后台监控分配量（Task 4）
  脚本内调用 (register-terrain "examplemod:lava_floor" #:blocks-move #f ...)
    ↓ 这个 Steel 函数由 Rust 侧注册，签名固定、参数强校验
  Registry::intern(NamespacedId) -> ContentIndex                 ← ll-mod::registry（Task 7）
    ↓ 与本体地形（Task 8 迁移后）走同一个 intern 调用，无特权分支
  TerrainDef { blocks_sight, blocks_move, move_cost } 物化存入 Registry ← Task 7/8
  失败分支：语法错误/未注册函数调用 -> ScriptError -> LoadError    ← Task 3 → Task 11
    ↓
LoadReport { entries: [...] }                                    ← ll-mod::console（Task 11）
  ↓
render_load_report(text_renderer, report, expanded)              ← ll-text::TextRenderer（Task 10）
  ↓ 排版（cosmic-text）→ 栅格化（glyphon）→ 提交到 wgpu
demo 主循环：世界里某个 tile 的地形是 examplemod:lava_floor 对应的 ContentIndex
  ↓
玩家/寻路查询该 tile：Registry::terrain_def(index) -> &TerrainDef  ← 运行期，纯 Rust 查表（不进 VM，见分界线一节）
  ↓
渲染该 tile（沿用 P3 已验证的世界层渲染管线，不变）
```

**每一环都指出了负责的任务与接口。** 唯一的软连接是 Task 11 的 crate 归属未定（见待裁定），但接口形状（`LoadReport`/`render_load_report`）不受这个归属影响，先按占位路径写，裁定后搬迁即可，不阻塞其余环节。

### 规格覆盖

| 规格要求 | 对应任务 |
|---|---|
| §10.1 Steel 沙箱能力（复用 ADR 0001 实测） | Task 3 |
| §10.2 四道防线 | Task 3（①②）、Task 4（③）、Task 6（④） |
| §10.3 本体即 Mod | Task 8、Task 12（验收） |
| §10.4 命名空间 ID | Task 7 |
| §10.6 加载管理界面 | Task 11 |
| §15 P4 迁移债务：`TerrainKind` 迁入注册表 | Task 8 |
| §15 P4 排期调整：文本渲染地基提前 | Task 10 |
| P3 交接：`action_cost` 下限钳位 | Task 2 |
| `identity-and-ids.md`：类型/实例分离 | Task 1 |
| `identity-and-ids.md`：`ContentIndex` 不可持久化，需映射层接口 | Task 7 |
| `identity-and-ids.md`：存档需分开记录两组 mod 集合 | Task 9 |
| §15 每阶段交付验收 demo | Task 12 |

### 有意留给后续阶段的缺口

- **技能效果、AI 决策的运行期脚本调用** 属 P5/P8。P4 的 `ScriptEngine` API 设计要同时支持注册期与运行期两种模式（见分界线一节），但本阶段没有任何调用点真正用到运行期模式。
- **行为树 tick 求值器** 属 P8（规格 §10.5 虽然记在"脚本层与 Mod 框架"一节，但 §15 阶段表明确把"随从与行为树"排在 P8），本计划不提前实现。
- **加载管理界面的开发期文件监听、`.scm` 存盘即生效** 明确列为本阶段不做（见 Task 11），是一个独立的文件系统监听功能，不应挤占核心交付物的开发时间。
- **存档头真正接入 mod 集合双记录、内容哈希、schema 迁移链** 属 P5。Task 9/Task 7 只留接口形状。
- **`identity-and-ids.md` 第②条"缺失 mod 之后怎么处理"（含只读模式建议）** 需要真实的存档读写才有意义，本阶段不涉及，留给 P5。
- **九宫格切片边框、焦点导航、完整像素 UI 控件库** 属 P7，Task 10/11 明确不做这些。
- **`RaceDef` 注册表、种族薄层存储债务**（`ThinPopulation::race` 应改为按出生聚落现算而非显式列，见 [种族系统](../../../knowledge/design/race-system.md) "存储"一节的实现债务记录）——本阶段的内容注册表管线搭好后，`RaceDef` 走同一套注册路径在技术上可行，但主体工作（世界历史生成产出种族分布场）在 P9，本计划不提前实现，只确认注册表接口形状不会挡住它。
- **光照透过率、气候周期性条带** 仍是规格里两项无人认领的条目（`p2-to-p3.md`/`p3-to-p4.md` 反复提醒过），与 P4 无直接关系，不在本计划范围，留给 P7/P9 计划作者认领。

---

## 待裁定

以下事项在阅读交接材料与设计文档时发现是**未被现有文档解决的分叉**，本计划给出了一个可执行的默认方向以避免任务停摆，但不代为最终裁定，请项目所有者定夺。

### 1. 加载管理界面的 crate 归属

规格 §5 把"加载管理界面"记在 `ll-ui`（游戏内 UI 控件库），但 `ll-ui` 本身按 §15 阶段表排在 **P7**，P4 阶段还不存在这个 crate。`text-and-font-rendering.md` 已经指出并解决了"文本渲染能力该不该提前到 P4"这个矛盾（结论：提前地基，不提前控件库），但**没有回答"P4 交付的加载管理界面这个具体功能模块该放进哪个 crate"这个更具体的问题**。

两个可选方向：
- **(a) 放进 `ll-mod`，作为一个轻量模块**，P7 `ll-ui` 落地后再把它搬迁过去、套上正式的九宫格控件外观。本计划暂按这个方向起草（Task 11）。
- **(b) 提前创建 `ll-ui` crate**，但只放加载管理界面这一个模块，其余控件继续留空到 P7。

两个方向都合理，差别在于"是否提前开一个未来要长期存在的 crate 名字"这类边界决策，本计划不代为拍板。

### 2. `TerrainKind` 反序列化校验的具体技术方案

Task 8 已经给出一个建议方向（"先落地未校验值，整体反序列化后再做一次显式交叉校验"，参照 ADR 0011 案例三），**但这个方向没有代码验证过**，只是基于既有先例的合理推断。若实现时验证不通过，需要的可能是完全不同的技术路线（例如改用 `DeserializeSeed` 显式传入注册表引用）。这不算需要项目所有者业务判断的"裁定"，更接近一个需要实现时验证的技术不确定性，但因为它牵动 ADR 0011 这条项目级规矩的适用边界，列在这里供知悉，若验证结果需要修改 ADR 0011 本身的适用范围，应回来更新那份决策记录。

### 3. mod 集合"生成期"记录在 P4 阶段的必要投入程度

`identity-and-ids.md` 明确要求"P4 建注册表时就要把这个区分做出来"，但 P4 还没有世界生成器，"生成期"这个概念在本阶段没有真正的绑定事件。Task 9 按"只做类型层面区分，不接真实存档"这个最小方向执行，是否需要更多（例如提前设计"世界创建"这个事件本身该长什么样，即便 P7 才真正触发它），本计划判断不需要——过早设计一个没有消费方的机制存在"猜错形状、P7 还是要返工"的风险，与"定得晚要返工"的风险相互对冲，本计划倾向晚一点、等 P7 世界生成器的真实需求出现后再补全形状。但这是一个可以商榷的范围判断，列在这里供知悉。

---

## 收尾必做：反向核对规格

P2 立下的纪律：**阶段收尾必须反向核对一次规格**——不是查实现是否满足规格，而是查**规格是否已被实现淘汰**。P4 阶段涉及的规格章节多、新增设计判断也多（尤其是本计划新增的"能力而非规则""注册期/运行期分界线"这两条，规格原文完全没有提及，是本计划在实现前补的设计），收尾时尤其需要检查这些新判断是否与规格原文出现新的不一致，而不只是核对既有条目。
