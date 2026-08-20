# 扩充给 mod 的 Steel 脚本 API：装载期一次性 API、事件监听 API 与其余候选

**冻结于** 2026-08-20。**落地状态**：纯设计，`crates/` 中无任何对应类型。核对提交 `2226469`（`main` 分支）。已核实的现状：`crates/ll-mod/src/pipeline.rs`（装载管线真实阶段划分）、`crates/ll-mod/src/topo.rs`（拓扑排序确定性总序）、`crates/ll-mod/src/registry.rs`（`Registry::intern` 允许悬空引用，无任何后续校验）、`crates/ll-script/src/host.rs`/`whitelist.rs`（能力边界、`map`/`filter`/`foldl`/`foldr` 白名单实测放行）、`crates/ll-sim/src/effect.rs`（现有 `Effect` 变体清单）、`knowledge/design/buffs-and-triggers.md` §三（既有的内容级触发器机制，本文档的事件监听 API 与它是两层不同粒度的机制，非重复设计）、`knowledge/design/script-entity-handles-and-batch-queries.md` 五节（既有的批量查询设计，纯设计未落地，本文档直接复用其形状而非重新发明）。

---

## 零、项目所有者的要求

> 「steel 给多几个 api，就是例如游戏刚刚加载的时候就运行的，这种只会运行一次就存入 rust 端的，然后再加点专门监听某类操作的 api，然后你再考虑有什么 api 是可以给的。」

以及后续追加：

> 「这类监听事件或许可以做成异步或者派发某个线程池之类的操作作为优化。」

架构划分（所有者已澄清）：**机制在 Rust 里实现好，真正可游玩的内容由 mod 填充**——与 [伤害公式的 mod API](damage-formula-mod-api.md) 同一句开工语。本文档拆成三件性质不同的事：**装载期一次性 API**（一节）、**事件监听 API**（二节，含异步/线程池的分区论证）、**其余值得给的 API**（三节）。

**判据总纲**：每一项候选都先问 [ADR 0018](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) 三步法（有没有设计自由度？自由度落在算法还是数据？高频是分档问题不是分层问题），能过三步再问 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md) 三档（能不能降到一/二档？只有真正本质动态的东西才配第三档）。**能用静态声明表达的，不做成回调**——这是所有者原话「机制在 Rust 里实现好」的直接推论。

---

## 一、装载期一次性 API

### 1.1 现有管线阶段（核实自 `crates/ll-mod/src/pipeline.rs`）

```
discover_mods(root)                    候选 mod.toml 路径（不解析）
  → parse_manifest(path)（逐个）       解析清单，互不影响
  → topo_sort(全部已解析清单)          一次性拿到确定性加载顺序，或在此中止整批
  → 按 topo 顺序逐个 mod：
       entry_points 为空 → Loaded（纯数据 mod）
       否则逐个 .scm 入口：
         为该 mod 新建一个 ScriptEngine
         set_active_registry/set_active_*_target（六张表 + Registry 整体移入线程局部）
         engine.load_source(源码)      register-terrain/class/skill/subclass/quest/race
                                        六个函数在求值期间直接写六张共享表
         take_active_registry/take_active_*_target（移回，供下一个 mod 复用同一份 Registry/表）
  → 返回 LoadReport
```

**关键事实**：六张内容表与 `Registry` 是**同一份实例**贯穿整个 topo 顺序循环——mod B 若在清单里声明依赖 mod A，B 加载时能看到 A 已经写进表里的内容（`Registry::get`/`*Table::get` 都能查到）。这已经是「mod 之间能互相看见」的真实机制，不需要额外设计。

**核实结论：「所有 mod 装载完毕之后」这个阶段不存在。** `load_all` 的 `for idx in order` 循环跑完就直接 `return report`，没有任何一步在全部 mod 都处理完之后再跑一遍。

### 1.2 这是真实缺口，且缺口有两种不同性质

#### 缺口 A：跨 mod 内容校验——已验证的悬空引用漏洞

`crates/ll-mod/src/registry.rs` 模块文档自己写明「`intern` 是注册，`get` 是解析，两者不能混」，但**没有第三个动作：校验**。核实 `script_quest_api.rs::do_register_quest`：

```rust
let target_id = NamespacedId::parse(condition_arg)...;
QuestCondition::KillCount { target_kind: registry.intern(target_id), ... }
```

`registry.intern` 对一个从未被任何 `register-*` 定义过的 id（比如任务引用一个拼错名字的怪物类型）**照样成功返回一个 `ContentIndex`**——`intern` 只负责字符串到索引的映射，不检查这个索引将来会不会真的被某张表 `define`。整个管线里没有任何一步回头检查「这个被引用的 id 最终有没有人定义它」。一个 mod 作者拼错怪物 id，装载**不会报错**，只会在游戏里表现成「这个任务永远数不到击杀数」——规格 §10.4「缺失内容不得崩溃」的精神被满足了（不 panic），但「明确报错」这半句没有被满足。

**这不需要新的脚本 API，需要新的管线阶段**：mod 作者今天已经在用 `register-quest`/`register-skill` 等函数间接声明引用，缺的是终局校验，不是声明手段。设计：

```
新增管线阶段（topo 循环结束后，仅当全部 mod 都 Loaded 时才跑）：
  遍历 Registry 里全部已 intern 的 ContentIndex
  对每一个，检查它是否出现在"应当定义它的那张表"的已定义集合里
  未定义的收集成新的 LoadStage::Validate 失败，指明是哪个 mod 的哪次引用造成的悬空
```

`LoadStage` 需要新增一个变体（`Parse`/`Topo`/`LoadScript`/`Register` 之外的 `Validate`），语义与既有四个阶段平级——不是「哪个 mod 加载失败」，是「全部 mod 都加载成功了，但它们拼出来的整体不自洽」。

**但一个引用属于哪张表，仅凭 `ContentIndex` 本身看不出来**——`registry.intern` 不携带类型信息。两个选项：

- 给每个 `register-*` 函数在 intern 引用字段时顺带打一个「期望种类」标签（例如 `Registry::intern_reference(id, ContentKind::Race)`），终局校验按标签分别去查对应表。**代价**：要改六个既有 `register-*` 函数的内部实现，动 `crates/ll-mod` 核心代码，超出本次「纯设计」范围能负责的边界，且这不是脚本面 API 的变化,是 Rust 内部记账粒度的变化。
- **本文档采纳的更小方案**：新增一个显式的、mod 作者**主动调用**的一次性 API，不强制、不retrofir 六个既有函数：

```scheme
(require-content! "gameplay:goblin" 'race)
(require-content! "gameplay:iron_sword" 'item)
```

`require-content!` 只做一件事：把 `(id, kind)` 追加进一个新的「终局校验清单」。终局校验阶段遍历这份清单（不是遍历全部 `ContentIndex`），逐条查对应表——`kind` 参数决定查哪张表，不需要 `Registry::intern` 本身携带类型。**这比「自动校验全部引用」范围更小、侵入性更低**：mod 作者对自己写的引用有把握就不用调用，觉得容易出错（例如任务链条较长、引用了别的 mod 的内容）就显式声明一次，加载期立刻拿到报错而不是运行期的沉默失败。

**为什么必须是脚本函数,不是静态声明**：校验清单本身在装载期就该跑完（不是运行时钩子），但「要不要校验哪些 id」是 mod 作者的判断，不是能提前物化成表的静态数据——`require-content!` 调用本身零开销（只是往 `Vec` 里 push 一条记录），不落入 ADR 0016 的三档讨论（那三档回答的是"运行期查询的开销"，`require-content!` 从不在运行期被调用）。

#### 缺口 B：跨 mod 内容修改（「打补丁」）——真实需求，需要新的第二遍

「一个 mod 要修改另一个 mod 的内容」——这不是「引用」（缺口 A 解决的问题），是「改写已经被别人定义过的字段」。现有 `*Table::define` **拒绝重复定义**（`pipeline.rs` 模块文档「为什么不写回正在运行的会话 `Registry`」一节已经论证过这一点：`reload_mod` 撞见的正是同一个限制）。这意味着即使把这个需求塞进现有的单遍 topo 循环，mod B 想调整 mod A 已经注册的地形移动代价，唯一能用的入口 `register-terrain` 会直接因为「重复定义」报错——**不是缺个 API，是现有 API 的语义（define，不是 patch）从根上就不支持这件事**。

**设计：一次独立的第二遍，与「定义」阶段彻底分开**

```
ModManifest 新增可选字段：late_entry_points: Vec<PathBuf>（与既有 entry_points 平级，默认空）

管线流程：
  阶段一（既有）：按 topo 顺序跑完全部 entry_points，register-* 六件套可用，patch-*/content-exists? 不可用
  阶段二（新增，仅当阶段一全部 mod Loaded 时才跑）：
    按同一个 topo 顺序（复用同一份排序结果，不重新计算）
    对每个声明了 late_entry_points 的 mod，跑一个新的 ScriptEngine：
      register-* 六件套不注册给这个引擎（防止在补丁阶段"定义"新内容，
      混淆"定义"与"补丁"两个阶段的边界）
      content-exists?/patch-* 系列注册给这个引擎，作用域是全部 mod（不
      只是自己声明依赖的那些）已经写进的最终 Registry/六张表
  阶段三（新增，缺口 A 的终局校验）：遍历 require-content! 清单
```

```scheme
;; late_entry_points 指向的脚本，典型写法
(if (content-exists? "othermod:iron_sword" 'item)
    (patch-item-damage "othermod:iron_sword" 15)
    #f)  ; 对方 mod 没装，静默跳过——不需要在清单里声明"软依赖"
```

`patch-*` 函数族（`patch-item-damage`/`patch-terrain-move-cost`/……，逐张表各自需要哪些字段就开哪个函数，具体清单是实现细节，不在本设计穷举）内部逻辑是「按 id 查现有条目，存在则改写指定字段，不存在返回 `Err`」——与 `register-*` 的「按 id 查，已存在则拒绝」正好互补，两者共用同一批 `*Table`，但绝不共用同一个校验分支。

**`content-exists?` 顺带解决了「软依赖」**：不需要在 `ModManifest` 里新增一种「可选依赖」的清单字段（这会是一处更大的改动，牵涉 `topo_sort` 的依赖图语义），mod 作者用运行时（严格说是装载期）判断代替声明式的可选依赖——**这是刻意选择的更小方案**，用一个查询函数换掉一次清单格式扩展。

**为什么这必须是脚本，不能是静态声明**：「该不该打这个补丁」依赖「对方 mod 装没装」这个只有在装载期、集齐全部候选之后才知道的事实，且不同的补丁组合（mod C 同时兼容 A 和 B，需要针对两种情况分别调整）是条件逻辑，不是一张能提前列举穷尽的表——这正是 ADR 0018 第一步判据「有没有自由度」在「补丁该怎么打」这件事上成立的地方。

#### 派生表计算——核实结论：不需要新 API

任务简报建议考虑「mod 在装载期算出一张查找表交给 Rust」。**核实结论：这个能力已经存在，不需要任何新 API。** `ScriptEngine::load_source` 内部是 `engine.run(source)`——完整求值一整份脚本，Steel 本身图灵完备（递归、高阶函数、`let`/`define` 全部可用，见 `whitelist.rs` 模块文档「白名单的定位」）。mod 作者今天就能在调用 `register-terrain` 之前，用普通 Steel 代码算出一批数值（循环/递归生成一组地形变体），再依次调用 `register-*`——这些调用发生在装载期（一次性，不是每帧），[ADR 0012](../decisions/0012-steel-capability-surface-verification.md) 实测的 327~400µs 级别 `engine.run` 开销本来就是为装载期设计的代价，装载期的多次 `register-*` 调用（哪怕上千次）在毫秒级预算内完全可以接受——这不是热路径，不受 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md) 的三档约束（那三档管的是运行期）。**不设计新 API，因为现有能力已经完整覆盖这个需求。**

#### 世界生成参数声明——接受，形状最小

`ll_world::generate::GenParams`（`seed`/`sea_level`/`mountain_level`/`octaves`）目前完全不对 mod 开放——核实 `crates/ll-mod/src/*.rs` 里 `GenParams` 只出现在测试代码，从未接过任何 `register-*` 式的脚本入口。「不同的海平面/山地阈值」代表设计选择（ADR 0018 第一步：存在自由度），且这是一组标量而非可批量的内容表，直接落一档（静态声明，装载期物化）：

```scheme
(register-world-gen-params 400 750 4)  ; sea-level mountain-level octaves
```

**冲突处理**：与「重复命名空间」同一条纪律——若两个 mod 都调用这个函数，第二次调用直接报错（不静默覆盖，不取后者为准，理由与 `topo.rs` 「重复命名空间：曾经的已知缺口」一节完全相同：静默覆盖会让其中一个 mod 的意图在游戏里凭空消失且没有任何报错）。`seed` **不放进这个函数**——种子是存档/会话级别的身份，不是 mod 内容，混进内容声明会让「同一个 mod 组合、不同存档」产生不该产生的耦合。

### 1.3 §15 阶段归属

上述三项新增机制全部落在**装载阶段本身**（发生在进入任何游戏循环之前），对应规格既有的「装载管线」范围，不新增 P 阶段——它们是 P4「script-and-mod」既定范围（`ll-mod`/`ll-script`）的自然延伸，不是新系统。

---

## 二、事件监听 API

### 2.1 与既有机制的边界：这不是重新发明 `TriggerDef`

[增益与通用触发器](buffs-and-triggers.md) §三已经设计了一套**内容级**响应机制——`TriggerDef` 挂在某个具体的物品/技能/增益上，只在「持有这件装备的实体命中时」触发。本节要设计的是**mod 级**的全局监听——「不管是谁的命中，我这个统计/成就类 mod 都想知道」。两者粒度不同，不是同一套机制的重复：

- `TriggerDef`：内容作者视角，「我这件装备命中时做什么」，天然按持有者数量摊薄频率。
- 本节的事件监听：mod 作者视角，「这类事件在整个世界任何地方发生，我都想被通知」，频率是**全世界**该类事件的总频率，不摊薄。

这个区别直接决定了下面的频率分级——同一个事件种类（例如「命中」），作为 `TriggerDef` 是可负担的（已有设计），作为**全局**监听则未必。

### 2.2 问题一：哪些事件给得起

326ns 是下限，不是全部代价——见 2.5 节的口径说明。频率核实：

| 事件种类 | 频率量级 | 依据 | 全局逐条监听是否给得起 |
|---|---|---|---|
| 每 tick / 每格移动 | 每秒数十到数百次（渲染/移动帧率量级） | `WorldState::advance`/`Timeline` 逐 tick 推进 | **给不起**——ADR 0016/0018 已经把「什么时候该不该触发」判为引擎层调度权，脚本不该接管 |
| 命中/攻击结算（`resolve_attack`/`resolve_use_skill`） | 现状：每次 `Intent::Attack` 一次；三轴战斗落地后按命中目标数展开，累计约几十万次/局量级（[伤害公式设计](damage-formula-mod-api.md)「真实调用频率」一节已实测论证） | `damage-formula-mod-api.md`、`buffs-and-triggers.md` §三 | **逐条给不起**（两份既有文档已经各自独立论证过一次，本文档不重新论证），**批量可以**，见 2.3 节 |
| 交易/经济决策 | E1（规格 §9.2）：单商人年均约 50 次决策事件，`docs/architecture/02-core-data-flow.md` 按万级人口估算约 50 万事件/年 | `docs/architecture/02-core-data-flow.md` 142 行 | **逐条给不起**（同一数量级），**批量可以** |
| 击杀/死亡（`Effect::Kill`） | 击杀是命中的**下游稀疏事件**——按典型 HP/伤害比例，需要多次命中才产生一次击杀，量级比命中低 1～2 个数量级 | 命中量级已知，击杀量级按构造关系推算，**未做独立实测，如实标注为推算而非精确数字** | **逐条给得起**——即使按population-wide 估算，仍比命中稀疏 1～2 个数量级；若未来实测证明背景模拟里的击杀率异常高，2.6 节的运行期跨界调用计数器会先暴露出来，届时可退回批量模式，不是本设计现在就要解决的问题 |
| 任务完成/任务节点推进 | 每个 agent 的任务链条是有限步骤，远低于决策事件频率 | `class-skill-quest-system.md`（任务链条设计） | **给得起** |
| 历史事件（建城/战争/王朝更替/改名） | 按世界历史生成的「年」粒度产出，一局游戏几十到上百条量级 | `kill-and-death-events.md`/`world-history.md`「否决独立战斗日志」一节论证的量级 | **给得起**——本来就是刻意做成稀疏的重大事件 |
| 季节切换 | 每年 4 次 × 游戏年数，几十次/局量级 | ADR 0014（季节纯函数派生） | **给得起** |
| mod 装载完成 | 每次启动一次 | 一节已覆盖，属装载期 API 不是运行期事件 | 不适用（不是这一节的范畴） |

**结论**：`Kill`/`Death`/任务推进/历史事件/季节切换五类，**逐条**投递给全局监听器是可负担的（量级在几十到几百/局，326ns × 几百 ≈ 百微秒量级，远低于单帧预算，也远低于装载期本身的开销）。命中/攻击结算与交易/经济决策**逐条给不起**——这与两份既有文档的结论完全一致，本设计不推翻它们，只是把结论从「内容级触发器不能默认走脚本」扩展到「全局监听器同样不能逐条投递」。

### 2.3 给不起的怎么办：批量投递，与批量查询共享同一套形状

**这是协调者要求的「批量事件分发」与「批量查询」放在一起设计**——两者确实是同一个模式在「读集合」与「读事件流」上的两个应用。[脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) 5.3 节已经设计了 `EntitySetHandle(Rc<[EntityId]>)`：不透明句柄包一份 `Rc` 切片，`filter`/`sort-by`/`aggregate`/`group-by` 等算子在句柄上链式操作，中间步骤不重新摊平成 Steel 列表，只有最终 `entity-set->list` 才真正把数据搬进脚本。

对高频事件（命中、交易）复用**同一套句柄形状**：

```rust
// crates/ll-script/src/api/handle.rs（续，与 EntitySetHandle 同一模式）
#[derive(Debug, Clone)]
pub struct EventBatchHandle(std::rc::Rc<[EventRecord]>);
impl steel::rvals::Custom for EventBatchHandle {}
```

投递时机不是「每 tick」，是**每个回合/每次 `advance` 调用结束**（与既有的回合制时间模型对齐，不是新发明一个"帧"概念）——这个批次边界天然把数量级摁在「一次战斗/一次决策批次」的规模（几十到几百条），不是整局游戏的累计量：

```scheme
(register-batched-world-event-listener 'hit
  (lambda (batch)
    ;; batch 是 EventBatchHandle，复用 5.2/5.3 节已有算子
    (aggregate batch 'damage-amount 'sum)
    (filter batch 'attacker-affiliation 'faction "somemod:bandits")))
```

**跨界次数**：一次批量调用（326ns）+ Rust 侧线性扫描，与逐条投递（N × 326ns）的差距与 5.1 节「批量查询」论证的量级完全同构——不重复推导。**这直接回应协调者的第三点**：优化对象从「让单次跨界更快」换成「减少跨界次数」，批量事件与批量查询是同一个答案。

### 2.4 问题二：多 mod 监听同一事件，回调顺序怎么定

**复用 `topo_sort`，不新造排序机制。** 核实结论：装载管线本身已经用 `topo_sort` 算出一个完全确定的 mod 加载顺序（`crates/ll-mod/src/topo.rs`：多个候选同时满足条件时按命名空间字典序打破平局，全程不依赖 `HashMap` 迭代顺序，符合 C5）。事件监听器的注册调用（`register-world-event-listener`/`register-batched-world-event-listener`）发生在装载期的 `load_source` 求值过程中，**这次求值本身就发生在 topo 顺序循环的某个确定位置**——宿主只需要在注册时把「这是第几个被加载的 mod、这是该 mod 脚本里第几次调用注册函数」这两个数字（后者是脚本内代码书写顺序，`load_source` 单线程顺序求值，天然确定）附加在监听器记录上，**不需要任何额外排序步骤**——只要按注册发生的时间顺序把监听器追加进一个 `Vec`（不用 `HashMap`），这个 `Vec` 的顺序就自动等价于「按 mod 的 topo 顺序、mod 内按声明顺序」的确定性总序。分发事件时按这个 `Vec` 的顺序依次调用即可。

**可行性核实结论：可行，且不需要新设计——是既有机制的自然复用，不是新的排序算法。**

### 2.5 问题三：监听器能不能改变事件结果

**结论：不能修改/否决已经发生的事件，只能追加新效果——世界状态类监听器与 `TriggerDef` 走同一条既定路径,不新造一种"事后否决"机制。**

事件监听器观察到的事件，定义上是 `apply` 已经把对应 `Effect` **写入世界之后**才广播的通知（否则"事件已发生"这个说法本身就不成立）。若允许监听器"修改"或"取消"这个已经写入的效果，等价于给 `apply` 引入回滚——直接违反 C1「`apply` 不含逻辑、是唯一写入口」与「已发生的写入不可撤销」这条既有不变式,也是 `buffs-and-triggers.md` §四已经论证过的同一条边界（`resolve_triggers` 读的是"某个 `Effect` 已经被 `apply` 之后"的世界，只产出**新的** `Effect` 追加进队列,从不回头改已经 apply 过的那条）。

**世界状态类监听器的形状**：与 `TriggerDef` 完全同构——回调返回值被解析成新的 `Intent`/`Effect`，喂进 `buffs-and-triggers.md` §四已有的深度上限触发队列（`MAX_TRIGGER_DEPTH = 8`），不新造一条独立的效果管线。区别只在于"谁能挂这个监听"：`TriggerDef` 挂在某个内容 `ContentIndex` 上，本节的监听器挂在"事件种类"上，两者产出效果之后走的是同一条既有队列——**这也回答了"是不是要重新设计一套递归防护"：不需要，深度上限已经覆盖新来源的效果。**

**「只读通知 vs 可修改可取消」的结论**：只读通知 + 可追加新效果，不提供"可修改"或"可取消"这两种更侵入性的变体——理由已在上一段给出（会破坏 `apply` 的不变式），这不是本文档新裁定的边界，是既有架构约束的直接推论。

### 2.6 追加：异步/线程池评估（协调者要求）

#### 326ns 的测量口径

核实 [ADR 0012](../decisions/0012-steel-capability-surface-verification.md) 「性能实测」表：326ns 是 `call_function_by_name_with_args`（**预注册函数 + 每次传新参数**）包一层 `InterruptHandler::run_with_timeout`（一次原子 store、一次线程 `unpark`、调用体、一次 channel `send`、`controller.resume()`、一次原子 store）的均摊耗时，未包中断防线的下限值是 74ns。**这个数字已经包含"传新参数"这一步的开销**，但探针脚本用的是简单参数（核实 `crates/ll-script/examples/probe.rs` 定位为验证探针，未见对"多字段事件记录"这类复杂载荷的专门测量）——真实的事件记录（击杀者/受害者/伤害量/位置等多个字段）若逐个跨界传递，`SteelVal` 构造成本会在 326ns 之上再叠加，**326ns 应视为下界，不是上界**。这个事实进一步支持批量投递（2.3 节）：批量把"构造多字段事件值"的成本从"每条一次"摊到"一批一次"，比逐条投递的边际收益更大。

#### 线程池派发开销：数量级判断——协调者的怀疑成立,且原因比"数量级更大"更根本

未在仓库里找到线程池派发开销的实测数据（`rayon` 仅见于 `crates/ll-platform/Cargo.toml`，未见用于脚本调用路径）。给出可核实的技术事实而非空对空的数量级估算：

**核实结论：`steel-core` 的核心值类型物理上不能安全跨线程共享。** 直接读 `steel-core-0.8.2` 源码：

- `src/rvals.rs`（`SteelVal` 的定义文件）大量使用 `Rc<...>` 而非 `Arc<...>` 包装内部数据——`Rc` 的引用计数不是原子操作，**不满足 `Send`/`Sync`**。
- `src/steel_vm/engine.rs` 第 94 行、第 512 行各有一处 `thread_local!`——引擎自身的实现依赖"当前调用发生在同一根线程上"这个假设。

这意味着：一个 `ScriptEngine`（包着 `steel::steel_vm::engine::Engine`）以及它求值产出的任何 `SteelVal`，**不能被移动到或共享给另一个 OS 线程**——不是"性能不划算"，是 Rust 的 `Send`/`Sync` 自动 trait 在编译期就会拒绝这样的代码（除非引入 `unsafe`，那是另一个必须独立评估的安全问题，本设计不考虑）。这比协调者原始判断（线程池派发开销比 326ns 大一个数量级）更根本：**即使派发开销真的可以忽略,「把一次 Steel 调用扔进线程池」这件事本身在当前架构下不能直接实现**，需要的不是"调优派发方式"，是"给每根工作线程各自常驻一份独立 `ScriptEngine`，且任何跨线程传递的数据必须先退化成不含 `Rc`/`SteelVal` 的纯 Rust 值"——这是一次架构级改造，不是给现有机制加一个 `async` 关键字那么简单。

**协调者数量级判断的结论成立（线程池比被优化的操作更贵），且成立的原因更强——不只是"更贵"，是"不能直接做"。**

#### 世界状态类 vs 表现类：分界如何在 API 上表达，以及怎么防止误用

**分界复用 [ADR 0020](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md) 已经确立的判据**——「这个计算的结果最终会不会变成世界状态的一部分」，本节把它原样套在监听器上：**结果只变成像素/声音（甲区）→ 表现类；结果要写回 `WorldState`（乙区）→ 世界状态类**。

**API 表达：两个不同的注册函数，不是一个标注参数**——理由是"标注"这种做法把强制力交给了 mod 作者自己填的一个字段，宿主如果不做额外检查，标注错了（写着表现类实际却调用了 `state-set!`）不会被拦下；分成两个函数名，宿主才能在**注册那一刻**就决定"这个回调运行在哪种上下文里"，不依赖脚本作者如实填写：

```scheme
(register-world-event-listener 'kill on-kill-handler)          ; 结果被解析成 Intent/Effect
(register-presentation-event-listener 'kill on-kill-fx-handler) ; 返回值被宿主整体丢弃
```

**如何在 API 层面强制,而不是只靠文档约定**——这是协调者点名"最容易出错的地方"。方案：新增一个线程局部守卫，与既有的 `ACTIVE_WORLD`/`ACTIVE_ACTOR`（`crates/ll-script/src/api/query.rs`/`crates/ll-script/src/api/actor.rs`）同一个模式：

```rust
thread_local! {
    static IN_PRESENTATION_CONTEXT: Cell<bool> = const { Cell::new(false) };
}
```

宿主在调用一个「表现类监听器」回调之前 `IN_PRESENTATION_CONTEXT.set(true)`，回调结束后 `set(false)`——与 `set_active_actor`/`clear_active_actor` 完全同构。**关键**：`state-set!`/`entity-state-set!`（`crates/ll-script/src/api/state.rs`）与任何解析回调返回值成 `Intent`/`Effect` 的路径，第一步都先检查这个标志——若为真，**不执行写入，返回一个明确的"拒绝"哨兵值**（与 3.4 节既有的"句柄失效返回 `#f`"同一套降级哲学），不是静默忽略也不是 panic。这样"表现类监听器不能改世界"不再是文档里的一句话,是**同一个 `ScriptEngine` 实例上、任何注册函数都躲不掉的一道运行期检查**——因为同一个 mod 的全部脚本代码共享同一个 `ScriptEngine`（同一份 `allowed_identifiers` 白名单),`state-set!` 在世界状态类监听器里能用、在表现类监听器里被挡下，靠的不是"这个引擎有没有注册这个函数"（两边都注册了,函数名字全局可见）,而是"调用发生的那一刻,这个线程局部标志是什么"——这与 `alloc_guard`/`ACTIVE_CONTROLLER` 已经在用的"按线程记账、调用窗口内外分明"是同一个既有模式,不是新发明的机制类别。

#### 表现类监听器真正能异步的地方——消费端，不是脚本执行端

协调者第四点判断正确（甲区可以异步），但**异步的正确位置需要重新定位**：鉴于上一小节"Steel 值不能跨线程"的硬限制，"表现类监听器的 Steel 回调本身"不能被扔进线程池。真正能异步的是**回调产出之后的东西**：表现类监听器的返回值本身就是纯数据（一个符号 + 若干基本类型参数,例如 `'play-sound "death-cry" pos`)，这份数据**不含 `SteelVal`/`Rc`**，可以安全地跨线程传给渲染/音频管线（`ll-render`/`ll-audio`,若已有独立的异步消费队列,直接复用；若没有,也不是本设计的范围）——**异步边界应该画在"Steel 回调结束、拿到一份纯数据之后"，而不是"把 Steel 调用本身丢给另一个线程"**。回调本身仍然同步、单线程执行，但由于表现类监听器覆盖的事件种类与世界状态类共享同一张"给得起"清单（2.2 节：Kill/Death/任务/历史事件/季节，几十到几百次/局量级），同步执行这部分本身开销可忽略,不需要为了"让它异步"而承担架构改造的代价。

**结论**：这个方向"有对的部分也有致命的部分"（协调者原话）——对的部分是"表现效果的产出可以脱离主逻辑的同步顺序"，致命的部分是"把 Steel 脚本调用本身派发到线程池"在当前 `steel-core` 选型下不能直接实现，且即使能实现，收益也早已被 2.3 节的批量方案吃掉（表现类监听器覆盖的事件种类本来就在"给得起"清单里,不存在需要靠异步抢救的性能问题）。**不采纳"脚本调用异步化"，采纳"表现类回调的纯数据产出交给既有/未来的异步渲染消费端"。**

#### C4 的落地状态与是否复用

核实 `docs/architecture/03-invariants.md` C4 一节：**尚未实现**——"当前仓库里没有找到离屏世界后台推进相关的代码……这条约束目前是给未来实现者的红线，不是检查现有代码是否遵守的问题"。

**是否该复用：不该，两者回答的不是同一个问题。** C4 解决的是"如何让**世界模拟本身**在墙钟时间上提前跑,但游戏时间上仍然对齐到一个确定 tick"——它异步的是**离屏区块的整段模拟计算**（墙钟时间），确定的是**目标 tick**（游戏时间）。事件监听器分发不涉及"提前模拟",只涉及"一次已经发生的事件该不该、以什么顺序通知给谁"——即使做成上一小节描述的"回调产出的纯数据异步交给渲染消费端",这份数据的**产生时刻本身仍然是同步的、确定的**（哪个 tick 产生了这个事件，由既有的 `resolve`/`apply` 管线决定,不受本设计触碰),不存在"C4 式的、需要提前推进到某个确定 tick"这个问题——C4 的机制（提前算完一段模拟）与本设计的机制（把已经算完的一个通知的呈现步骤挪到消费端）解决的是完全不同层面的问题,复用会是概念上的误用,本设计不复用。

---

## 三、其余值得给的 API

### 3.1 判据

沿用零节判据总纲（ADR 0018 三步 + ADR 0016 三档）。额外补一条本节专属的检验：**是不是所有者早先明确要过、至今未落地的东西**——这类需求不需要重新论证"要不要给"，只需要论证"给成什么形状"。

### 3.2 批量/链式查询原语——所有者明确要过，至今未实现

**核实结论**：[脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) 五节已经完整设计了这套"批量工具库，一次处理整个列表"（`filter`/`sort-by`/`aggregate`/`group-by`/`nearest`，`EntitySetHandle`/`GroupedEntitySetsHandle` 不透明句柄），文档自身状态行已如实标注「五节仍是纯设计，未落地」（核实 `crates/ll-script/src/api/query.rs` 不含这些函数名，与文档标注一致）。**本文档不重新设计这套原语**——它已经存在、形状已经论证过（句柄语义 vs 拷贝语义、确定性排序打破平局规则、逃生舱代价），本文档只做两件事：

1. **确认它仍然是当前唯一悬而未决的"所有者明确要过"的 API**，落地优先级应当高于本文档新提出的任何东西。
2. **按二、3 节的设计，把事件监听的批量投递（`EventBatchHandle`）接到同一套算子上**——这是本文档对五节设计的唯一实质性扩展：不新发明 `filter`/`aggregate`/`group-by`，直接让 `EventBatchHandle` 复用它们,理由已在 2.3 节给出。

### 3.3 `map`/`filter`/`foldl`/`foldr` 白名单状态——已实际核实，非文档推断

任务要求"去实际核实，不要靠文档"。核实方式：读 `crates/ll-script/src/host.rs` 第 245-282 行 `compute_allowed_identifiers` 与其测试。结论：

- **机制层面**：白名单基础集合来自 `engine.globals()`——这是 Steel 编译器符号表的**全量快照**，不区分"来自某个 `BuiltInModule` 的导出"还是"`stdlib.scm` 里用 Scheme 自己写的 `define`"（`map`/`filter`/`foldl`/`foldr` 全部属于后一类，`host.rs` 模块文档「为什么不再手工维护一份安全模块清单」一节明确点名这四个函数举过例）。这个机制是**穷尽性**的（снимает全部全局名字,不是逐个点名放行),不是"点名放行了 map/foldl,没点名 filter/foldr"这种手工清单会有的疏漏。
- **实测层面**：`host.rs` 第 931-1016 行「验收样本」测试**实际调用了 `map` 与 `foldl`**（`(map value-density items)`/`(foldl + 0 ...)`）并断言正确结果,测试通过。`filter`/`foldr` 没有专门的同名断言测试,但由于放行机制是"快照全部 `engine.globals()`"而非逐名单点,`filter`/`foldr` 与 `map`/`foldl` 处于完全相同的地位（同样是 `stdlib.scm` 里的 `define`,同样会出现在快照里）——**核实 `script-entity-handles-and-batch-queries.md` 5.4 节「逃生舱」一节的表述"脚本用已经验证通过白名单的 `map`/`filter` 高阶函数",与本次核实结论一致**。

**结论：`map`/`filter`/`foldl`/`foldr` 四个全部可用,不存在协调者档案里提到的"历史上曾被误挡"问题在当前代码上复现**——那次误挡是**手工维护安全模块清单**这个已被放弃的旧实现的产物（`host.rs` 模块文档如实记录了这段历史),当前实现（`engine.globals()` 全量快照)结构性地不会重蹈覆辙。**唯一值得留意的后续工作**（非本设计范围,如实标注）：`filter`/`foldr` 没有专门的同名回归测试锁定,若未来有人误改白名单机制,`map`/`foldl` 的既有测试会先变红,但不能排除一种"只挡住 filter/foldr,不挡 map/foldl"的假想回归不会被现有测试覆盖——建议留一条 `spawn_task` 级别的后续工作,不在本设计内处理。

---

## 四、被否决的 API 与理由

| 候选 | 否决理由 | 替代品（ADR 0019 通则） |
|---|---|---|
| 派生表计算专用 API | 装载期本来就是完整 `engine.run`,脚本本身图灵完备,已能在调用 `register-*` 前用普通 Steel 代码算出整张表——不是能力缺口,是对现有能力的误判 | 无需替代,既有装载流程已完整覆盖 |
| 全局逐 tick / 每帧事件监听（`on-tick`/`on-frame`） | 频率给不起（每秒数十到数百次）,且"什么时候该触发"是引擎层调度权（ADR 0018）,脚本接管调度会破坏 C1 | 已有的一/二档声明式内容（地形/技能/触发器）+ 引擎既有的时间轴调度,不需要脚本参与"什么时候" |
| 命中级别（`on-hit`）**无批量**的全局监听器 | 两份既有文档（`buffs-and-triggers.md`/`damage-formula-mod-api.md`）已各自独立论证过"命中不能默认走脚本回调",全局无批量版本比内容级 `TriggerDef` 更差（对每个实体每次命中都跑,不摊薄） | 内容级 `TriggerDef`（已有设计）+ 本文档 2.3 节的批量事件分发（`EventBatchHandle`） |
| 可修改/可取消已发生事件的监听器（事后否决式 hook） | 破坏 `apply`「唯一写入口、不含逻辑、已发生不可撤销」的不变式,等价于给 `apply` 引入回滚 | `resolve` 阶段的既有声明式判定（例如"是否格挡"这类判定应在产出 `Effect` 之前,用已有的一/二档表达,不是"事后否决"）+ 世界状态类监听器的"只读通知、可追加新效果"模式（与 `TriggerDef` 同构） |
| 脚本调用本身派发到线程池的"异步监听器" | `steel-core` 核心值类型基于 `Rc`（非 `Arc`）,引擎内部依赖 `thread_local!`,物理上不满足 `Send`/`Sync`,不能直接跨线程共享；即使强行实现（每线程常驻独立 VM + 只传纯数据跨线程）,收益也已被批量方案与"给得起清单"本身覆盖 | 世界状态类监听器保持同步有序（确定性刚性需求）；表现类监听器的**纯数据产出**（不含 `SteelVal`）交给既有/未来的异步渲染/音频消费端,脚本回调本身仍同步执行 |
| mod 间可选依赖的清单字段（新增 manifest 软依赖声明） | 会牵涉 `topo_sort` 依赖图语义的扩展,是比"一个查询函数"更大的改动 | `content-exists?`（一、缺口 B）——装载期查询代替清单声明,mod 作者自己写条件分支 |
| 自动遍历全部 `ContentIndex` 做终局校验（不要求 mod 显式声明） | 需要给六个既有 `register-*` 函数逐个补"期望种类"标签,改动 `ll-mod` 核心内部记账粒度,超出本次纯设计能负责的范围,且并非所有引用都值得校验（mod 作者自己有把握的引用不需要被强制检查） | `require-content!`（一、缺口 A）——mod 作者按需显式声明,范围更小,侵入性更低 |

---

## 五、与 §4 约束 C1–C5 的对照小结

| 机制 | 可能违反的约束 | 保证方式 |
|---|---|---|
| 装载期 `require-content!`/`content-exists?`/`patch-*` | 无（装载期,非运行期,不涉及 `WorldState` 写入路径） | 类型/阶段隔离：`patch-*` 只在阶段二可用,阶段二不注册 `register-*` |
| 世界状态类事件监听器 | C1（唯一写入口） | 回调返回值解析成 `Intent`/`Effect`,喂入既有深度限界触发队列,监听器本身不持有 `&mut WorldState` |
| 表现类事件监听器 | C1（若被滥用写状态） | `IN_PRESENTATION_CONTEXT` 线程局部守卫,写入类注册函数运行期检查该标志,拒绝而非静默放行 |
| 多 mod 监听顺序 | C5（禁止容器迭代顺序参与逻辑） | 复用 `topo_sort` 已有确定性总序,注册记录用 `Vec` 而非 `HashMap` |
| `EventBatchHandle` 批量投递 | C1/C3 | 与 `EntitySetHandle` 同构：只读句柄,不接受可变世界引用；不产生随机性 |
| 异步表现产出（消费端） | C1（若误把世界写入路径搬到消费端） | 消费端只接收不含 `SteelVal` 的纯数据,物理上没有调用 `state-set!` 之类函数的能力（那些函数只注册在脚本引擎里,消费端不是脚本环境） |

---

## 相关文档

- [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) — 本文档三、2 节直接复用其五节的句柄/算子设计
- [增益与通用触发器](buffs-and-triggers.md) §三/§四 — 本文档事件监听 API 的既有先例（内容级粒度）与深度限界触发队列（世界状态类监听器复用同一条队列）
- [伤害公式的 mod API](damage-formula-mod-api.md) — 命中级别调用频率的独立实测论证来源之一
- [脚本状态存储](script_state_storage.md) — `PENDING_WRITES` 批量提交模式，`ScriptEngine` 一次调用多次写入的既有先例
- [Steel 语法参考](steel-script-reference.md) — `map`/`filter`/`foldl`/`foldr` 等高阶函数的既有实测记录
- [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md) / [0017](../decisions/0017-tiered-declarations-materialize-columnar.md) / [0018](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) / [0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md) / [0020](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md) / [0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) / [0023](../decisions/0023-script-state-writes-go-through-apply.md)
