# 脚本层数据句柄与批量查询原语

> **【2026-08-23 状态订正】本文档的前提已经不成立：Steel 脚本系统整体拆除。**
> `crates/ll-script/`、`steel-core` 依赖与全部 `.scm` 文件均已删除；mod 内容改用
> `mods/<id>/*.json5` 数据文件声明，玩法层**逻辑**（AI 行为树、技能结算、物品使用
> 效果）住在引擎里的 Rust——第三方 Rust 扩展能力（注册表 / C ABI）明确推迟，不做。
> 起因与决定见 [ADR 0028](../decisions/0028-steel-engine-construction-memory-corruption.md)
> 与 [ADR 0018](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 各自的 2026-08-23 订正段，以及规格 §4 的 `[2026-08-23 规格修订]`。
>
> **正文一字未改**，仍是冻结时的原样：它记录的是脚本时代的设计，与本订正块叠加读
> 才是完整的演变过程。读的时候请自行把「脚本」「`register-*`」「VM」替换成
> 「内容数据文件」「JSON5 字段」「无」。


- **冻结时间**：2026-08-18
- **基线提交**：`270783e`（写作本文档时的 HEAD，工作区干净，496 测试全绿）
- **状态**（2026-08-19 复核更正——原文「纯设计，不要求本次实现」已部分过期）：**部分落地**。三、2/3 节的句柄形状与防伪造机制已随脚本状态存储批次（提交 `ac27217`）落地为 `crates/ll-script/src/api/handle.rs::ScriptEntityHandle`，模块文档明确写着「该文档整体标注『纯设计,不要求本次实现』,但本批次的脚本状态存储需要一个可以安全跨越 Steel FFI 边界的实体引用表示……因此这里按该设计文档已经给出的形状把它落地」——即句柄机制是作为脚本状态存储的前置依赖顺带落地的，不是本文档本身被认领实现。四节 `Intent::Attack` 的解禁也已落地（`crates/ll-sim/src/intent.rs` 已有 `Attack` 变体）。**五节「批量查询原语」（`filter-within-distance`/`average` 等）仍是纯设计，未落地**——已核实 `crates/ll-script/src/api/query.rs` 不含这些函数名。读者应分节看待落地状态，不能整份文档一概而论。
- **落地依赖**：`crates/ll-script/src/api/`（新增模块，路径见正文）、`crates/ll-world/src/entity/`（复用既有机制，无需改动）、`crates/ll-sim/src/intent.rs`（`Intent::Attack` 解禁，需要新增变体）
- **对应任务**：填补 `.superpowers/sdd/2026-08-18-p4-script-and-mod/task-3-5-report.md` 记录的缺口——`intent.rs` 只支持 `Move`/`Wait`，`Attack` 因「脚本如何安全持有一个不可伪造的 `EntityId`」这个未设计的机制而被搁置

## 一、目标与约束

**项目所有者要求**：减少数据传递次数；脚本应当操作 Rust 侧的数据，而不是把数据拷贝进脚本。

ADR 0012 实测：`call_function_by_name_with_args` 包一层 `InterruptHandler::run_with_timeout`（`ScriptEngine::call` 实际路径）均摊 **326ns**——纳秒级，可以放在每帧每实体的热路径上。这个数字决定了本设计的优化目标：

- **326ns 不是慢**，不需要为了让单次调用更快而做任何事（下限本身已经是纳秒级，没有明显可榨的空间）。
- **326ns 乘以调用次数会累积**。逐实体逐属性问答的模式——"对 100 个实体各问一次血量"——是 100 次跨界，约 32.6µs；对视野内上千实体做一次筛选就是上千次跨界，接近 0.33ms，在 16.6ms 的单帧预算里已经不是可以忽略的比例。
- 因此优化目标是**减少跨界次数**，不是**让每次跨界更快**。这决定了本设计的两个支柱：句柄语义（避免把整个实体拷进脚本）、批量原语（把"筛选 N 个"从 N 次跨界压成 1 次）。

### 两种语义的差别

```
拷贝语义   Rust 把世界数据构造成 Steel 值 → 脚本读 → 返回新值 → Rust 读回
           每次决策产生大量分配与转换，且天然是 N 次跨界（N=需要读的字段数）

句柄语义   脚本拿到不透明句柄，调 Rust 函数问答      ← 本文档设计这个
           数据留在 Rust 侧，只有基本类型（整数/符号/布尔）跨界
```

`crates/ll-script/src/api/query.rs` 现状已经是句柄语义的雏形——`world-move-cost-at`/`world-tick` 这类函数只把查询*结果*（一个整数）跨界，不把 `WorldState` 本身搬进脚本。本文档把这个模式从「世界的全局只读状态」推广到「单个实体」与「实体集合」。

## 二、`steel-core` 0.8.2 实际能力核实（读源码，非假设）

**核实方式**：直接阅读 `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/steel-core-0.8.2/src/rvals.rs` 与 `src/steel_vm/register_fn.rs`（本地 Cargo 依赖缓存的真实源码，版本号与 `Cargo.lock` 里锁定的 `steel-core 0.8.2` 一致），不是查文档或凭经验判断。

### 核实到的机制：`Custom` trait + `SteelVal::Custom`，可以直接用

`steel-core` 提供一个**密封**（`private::Sealed`）的标记 trait：

```rust
// src/rvals.rs:166
pub trait Custom: private::Sealed {
    fn fmt(&self) -> Option<Result<String, fmt::Error>> { None }
    // ……其余方法均有默认实现，最简单情形只需要空 impl
}
```

任何 Rust 类型只要 `impl Custom for T {}`（空实现即可），就自动获得 `CustomType` 的 blanket 实现（`impl<T: Custom + MaybeSendSyncStatic> CustomType for T`，`src/rvals.rs:315`），并因此自动获得跨界转换：

```rust
// src/rvals.rs:369-371
impl<T: CustomType + 'static> IntoSteelVal for T {
    fn into_steelval(self) -> Result<SteelVal> {
        Ok(SteelVal::Custom(Gc::new_mut(Box::new(self))))
    }
}

// src/rvals.rs:408-421（要求 T: Clone，取值时按 TypeId 精确匹配）
impl<T: CustomType + Clone + 'static> FromSteelVal for T {
    fn from_steelval(val: &SteelVal) -> Result<Self> {
        if let SteelVal::Custom(v) = val {
            let left = v.read().as_any_ref().downcast_ref::<T>().cloned();
            left.ok_or_else(|| /* ConversionError，不是 panic */)
        } else {
            Err(/* Type Mismatch: 期望 opaque struct */)
        }
    }
}
```

`ScriptEngine::register_fn`（`crates/ll-script/src/host.rs:444`）转发到 Steel 的 `RegisterFn` trait，其内部机制正是走 `IntoSteelVal`/`FromSteelVal`——因此**不需要新增任何跨界机制**，只需要给句柄类型 `impl Custom for T {}`，把它当普通返回值/参数类型用在已有的 `register_fn` 调用里即可，与 `query.rs` 现有的 `i64`/`bool` 返回值写法完全同构，只是类型换成了句柄。

### 为什么这就是防伪造的地基（结论先给，论证见第三节）

`SteelVal::Custom` 这个变体**没有对应的 Scheme 字面语法**——脚本代码里写不出一个「构造 `SteelVal::Custom`」的表达式。它只能通过 Rust 侧的 `IntoSteelVal::into_steelval` 产生，即：**只有宿主主动交给脚本的句柄，脚本才能拥有**。`FromSteelVal` 取值时按 `downcast_ref::<T>()`（`std::any::Any`，运行期用 `TypeId` 精确匹配）——脚本传回一个类型不对的值（哪怕凑巧是另一个 `Custom` 类型的实例）会得到 `ConversionError`（走 `Result`，不会 panic），不会被误判成匹配。

### 已核实但本设计不依赖的机制：`register_fn_with_ctx`

`src/steel_vm/register_fn.rs` 的 `RegisterFn` trait 有一个 `register_fn_with_ctx(ctx: &'static str, name: &'static str, func: FN)` 方法。**核实结论：这个 `ctx` 参数是一个静态字符串标签，不是运行期 VM 上下文的引用**——读了签名之后确认它不提供"Rust 函数在执行期间回调进 VM、反过来调用一个作为参数传入的脚本闭包"这种能力。本设计的「批量筛选逃生舱」（第五节 5.4）因此**不依赖**这个假设中的能力，改用一种已经验证过确实可行的模式（把集合物化成 Steel 列表，用已验证通过白名单的 `map`/`filter` 在脚本侧完成，见 5.4 节）。这一条**明确标注为「已排查、机制不是我最初设想的那样」**，不是「未核实」。

### 未核实项

- `Custom` 值参与 Steel 相等性判断（`equal?`）、哈希、`display`/`write` 时的具体输出格式——本设计不依赖脚本对句柄做这些操作，默认实现（`display` 落到 `format!("#<{}>", type_name)`，不含字段值）已经足够安全（不泄漏内部字段），但没有逐项测试验证。
- `Custom` 值能否被存进 Steel 原生的 `hash`/`vector` 等容器后正常取回——本设计的容器（实体集合）本身也设计成 `Custom` 句柄（见第五节），不依赖脚本原生容器装句柄，因此这一点不影响本设计，但如实标注未测试。
- `SteelVal::Custom` 在 Steel `equal?`/`eq?` 语义下，两个 `Clone` 出来的、内部数据相同的句柄是否判等——未测试。本设计不依赖句柄可比较（脚本不需要自己比较两个句柄是否相同，需要比较时应该问宿主注册的函数）。

## 三、句柄设计：`ScriptEntityHandle`

### 3.1 为什么不需要额外发一层「世代号/会话号」——`Arena` 的世代机制可以直接复用

任务要求核实 `ll-world` 的 `Arena` 世代号机制能否直接复用于「访问失效句柄必须返回错误而非 panic 或读到脏数据」。核实结论：**可以直接复用，不需要另造一层**。

`crates/ll-world/src/entity/id.rs` 里 `EntityId { index: u32, generation: u32 }` 本身就是「世代校验的标识符」；`crates/ll-world/src/entity/arena.rs` 的 `Arena::get`/`get_mut`：

```rust
pub fn get(&self, id: EntityId) -> Option<&T> {
    match self.slots.get(id.index() as usize)? {
        Slot::Occupied { generation, value } if *generation == id.generation() => Some(value),
        _ => None,
    }
}
```

——下标越界、世代不符两种情况都返回 `None`，不 panic。实体在脚本持有句柄期间死亡（`despawn`）会让该下标的世代号递增，旧 `EntityId` 因世代不符自动失效，`get` 立即返回 `None`。**句柄的"过期检测"因此不需要任何新代码**：句柄本身就是 `EntityId`，每次用句柄查询/操作时都经过 `Arena::get`/`get_mut`，失效句柄天然查不到值。

唯一需要新增的是：把「查不到值」这个 `Option::None` 翻译成脚本侧能理解的失败——见 3.4 节。

### 3.2 句柄的具体形状

```rust
// 新文件：crates/ll-script/src/api/handle.rs

use ll_world::entity::EntityId;

/// 脚本持有的不透明实体句柄。
///
/// 字段私有——脚本没有任何路径读到内部的 `index`/`generation`，只能
/// 把句柄整个传回宿主注册的函数。这是防伪造的核心：脚本连"看到"这两个
/// 数字的能力都没有，遑论凭空拼出一个新句柄。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptEntityHandle(EntityId);

impl ScriptEntityHandle {
    /// 仅供本 crate 内部（其余 api/*.rs 模块）构造——句柄只应该由「宿主
    /// 已经从 Arena 里查到的一个真实 EntityId」换取，不暴露给外部
    /// 任意构造。
    pub(crate) fn new(id: EntityId) -> Self {
        ScriptEntityHandle(id)
    }

    pub(crate) fn entity_id(&self) -> EntityId {
        self.0
    }
}

// 空实现即可获得 CustomType/IntoSteelVal/FromSteelVal——见第二节核实。
// 不覆盖 `display`：默认输出 `#<ScriptEntityHandle>`，不含 index/
// generation 数值，避免脚本靠打印句柄的字符串表示旁敲侧击猜测编码。
impl steel::rvals::Custom for ScriptEntityHandle {}
```

`ScriptEntityHandle` 本身是 `Copy`——`EntityId` 是 8 字节（`u32` + `u32`），包一层不增加任何有意义的开销，`Clone`（`FromSteelVal` 要求）等价于按位拷贝。

### 3.3 防伪造论证

三层理由，缺一都不够，合在一起构成完整论证：

1. **语法层面拿不到**：`SteelVal::Custom` 没有 Scheme 字面语法能直接构造（第二节已核实）。脚本写不出"我要一个 `ScriptEntityHandle`"这样的表达式，只能等宿主给。
2. **字段层面读不到**：`ScriptEntityHandle` 的字段是 Rust 私有字段，即使脚本通过某种方式拿到了一个 `SteelVal::Custom`，也只能调用宿主注册的、接受这个类型作为参数的函数——没有任何注册函数会把 `index`/`generation` 这两个数字单独吐给脚本（这是 API 表面设计上的自我约束，见 3.5 节"绝不注册的函数"）。
3. **类型层面伪造不了**：即使脚本嵌套构造了别的 `Custom` 类型（例如未来某个 mod 内容的句柄）试图冒充 `ScriptEntityHandle`，`FromSteelVal::from_steelval` 的 `downcast_ref::<ScriptEntityHandle>()` 按 Rust `TypeId` 精确匹配，类型不符返回 `ConversionError`，不会张冠李戴。

与 `EntityId::new` 是 `pub(crate)`（仅 `ll-world` 内部可见）这条既有的 crate 边界防线相比，本设计**不依赖**这条边界——即使未来某天 `EntityId::new` 意外被放宽可见性，`ScriptEntityHandle` 的防伪造性质也不会因此削弱，因为它防的是"脚本→Rust"这条边界，与"Rust crate→Rust crate"是两条独立的防线。这是刻意的设计选择：**防伪造应该建立在脚本语言本身的能力边界上，不应该依赖 Rust 内部可见性这个随时可能因为无关重构而松动的东西。**

### 3.4 失效句柄的处理

任何接受 `ScriptEntityHandle` 的注册函数，内部第一步都是 `Arena::get`/`get_mut`：

```rust
fn entity_handle_health(handle: ScriptEntityHandle) -> SteelVal {
    with_active_world(SteelVal::BoolV(false), |world| {
        match world.actors.get(handle.entity_id()) {
            Some(agent) => SteelVal::IntV(agent.health as isize),
            None => SteelVal::BoolV(false), // 句柄已失效
        }
    })
}
```

**返回值形状的选择**：不是每个查询函数都返回 `Option`/`Result`（Steel 没有原生 `Option` 类型，`FromSteelVal`/`IntoSteelVal` 对 `Result<T, E>` 有专门约定，会转成脚本侧的错误——这会让"实体已死亡"这种正常游戏状态被当成"脚本出错"处理，级别不对）。约定：**读取型查询在句柄失效时返回一个明确的哨兵值**（数值属性返回 `#f`，与"未设置活跃世界"这个既有约定——见 `query.rs` 的 `with_active_world` 默认值模式——保持同一套风格），**行为型操作（如攻击）在目标句柄失效时视为"这一步无意义，静默作废"**，与 `resolve_move`/`resolve_open_door` 撞墙时"不产生任何效果"的既有降级哲学完全一致，不是新发明一套错误处理方式。

这与四道防线的第二道「降级而非崩溃」（规格 §10.2）同源：句柄失效是运行时会发生的正常状况（实体死亡是游戏里天天发生的事），不是需要中断脚本执行的异常。

### 3.5 生命周期规则：跨帧持有句柄允不允许，与约束 C1 的张力

**结论：句柄本身允许跨帧持有，但持有句柄不构成约束 C1 意义上的"隐式状态"。**

约束 C1 原文：「Steel 脚本不得持有任何跨帧的隐式状态。任务进度、技能冷却、AI 记忆、行为树当前节点，全部存放于 `WorldState`。脚本是纯函数，通过明确 API 读写世界黑板。VM 必须可随时从零重建。」

这条约束防的是：脚本自己的执行环境里累积了一份"世界的私有副本"，导致重建 VM（一键重载 mod、存档读取后重新加载）时丢失只存在于脚本内存里的信息，或者两次运行的脚本状态不同步导致重放分叉。

句柄不落入这个范畴，理由：

1. **句柄不携带数据，只携带"去哪查"这一条索引**。`ScriptEntityHandle` 本身就是 `EntityId`——`EntityId` 早已经是规格与 ADR 0004 承认的、可以合法跨帧存在的东西（`Agent`/`ThinPopulation` 里到处都是靠 `EntityId` 互相引用，时间轴队列里也存着 `EntityId`）。句柄不是"脚本记住了某个实体某一刻的血量"，是"脚本记住了一个查询地址"——查询地址本身不是状态，是状态的指针，指针失效（世代号不符）会被立即检测到（3.4 节），不会读到脏数据。
2. **VM 从零重建不受影响**：约束 C1 要求「VM 必须可随时从零重建」——重建 VM 意味着脚本里所有 `define` 出来的全局绑定（包括脚本自己保存的句柄变量）都会丢失，这是符合预期的：句柄本来就只在"当前决策所需的临时引用"这个意义上有效，重建 VM 后脚本会在下一次决策时重新调用查询函数换到新的句柄，不依赖旧句柄延续下来。**句柄不是脚本状态的持久化载体，`WorldState` 才是**——这正是约束 C1 要求的分工，句柄机制没有破坏它，只是给了脚本一种"在当前这次调用范围内引用世界数据"的更便宜方式。
3. **真正需要正面处理的张力**：脚本理论上*可以*把句柄存进一个跨调用持续存在的全局变量（例如 `(define remembered-target #f) (define (probe target) (set! remembered-target target) ...)`），下次调用时读出来用——这确实是"跨帧隐式状态"，但**约束的是这种用法，不是句柄类型本身**。这与脚本用普通 `define`/`set!` 存一个整数当"记忆"是同一类违规，不是句柄机制独有的新问题。约束 C1 现有的执行方式（代码审查/`tools/ll-datacheck` 静态检查）同样适用于"脚本存了一个句柄"和"脚本存了一个整数"——**本设计不需要为句柄单独发明一种检测机制**，只需要在 `tools/ll-datacheck`（未来任务）的检查范围里明确写上"顶层 `define`/`set!` 存放的值不得跨调用持续存在，不论是普通值还是句柄"，这是既有检查规则的自然延伸,不是新规则。

**因此句柄可以跨帧持有（技术上不会读到脏数据），但脚本"应该"把句柄当一次性凭证用（每次决策重新查询）——前者是安全性保证，后者是使用规范，两者不是同一个层面的问题，不应该混为一谈。**

### 3.6 只读约束（约束 C1「apply 是唯一写入口」）如何在类型层面保证

`ScriptEntityHandle` 本身**没有任何注册函数接受它作为参数来直接改世界**——本设计只注册两类函数：

- **读**：`entity-handle-health`、`entity-handle-affiliation`……接受 `ScriptEntityHandle`，返回基本类型（整数/符号/布尔）。
- **产出意图**：`intent.rs` 的 `parse_intent` 解析脚本返回值，新增识别 `(list 'attack target-handle)` 形状，把 `ScriptEntityHandle` 解包成 `EntityId` 装进 `Intent::Attack { actor, target }`——这仍然是"脚本返回一个数据值，宿主解析成 `Intent`"的既有模式（见 `intent.rs` 模块文档），`Intent` 走既有的 `resolve → Effect → apply` 管线，脚本自己完全不接触 `Effect`/`apply`。

**类型层面的保证**：`ll-script` 这个 crate 里，任何函数签名只要接受 `&mut WorldState`（或任何等价的可变引用），都不会被注册给脚本调用——这本来就是 `register_fn` 的天然限制：Steel 的 `RegisterFn` 要求参数类型能双向转换（`FromSteelVal`/`IntoSteelVal`），`&mut WorldState` 不满足这个约束（`WorldState` 本身也不是 `'static`，`query.rs` 模块文档已经说明了这一点），物理上无法被注册成可调用的脚本函数。**"脚本只能读、不能写"不是靠人工审查维持的约定，是 Steel FFI 类型系统本身排除了"脚本函数签名里出现可变世界引用"这种可能性**——`ScriptEntityHandle` 的引入没有改变这条既有保证，只是新增了一种"读"的手段（读单个实体属性，而不只是读全局世界状态）。

## 四、`Intent::Attack` 的解禁

有了 `ScriptEntityHandle`，`intent.rs` 里当年搁置的注释可以正面回答：

```rust
// crates/ll-sim/src/intent.rs（需要新增变体，本文档只给形状，不改代码）
pub enum Intent {
    Wait { actor: EntityId },
    Move { actor: EntityId, dir: Direction },
    Attack { actor: EntityId, target: EntityId },  // 新增
}
```

```rust
// crates/ll-script/src/api/intent.rs::parse_intent 新增分支
SteelVal::ListV(list) => {
    let mut iter = list.iter();
    match symbol_str(iter.next()?)? {
        "move" => { /* 既有逻辑 */ }
        "attack" => {
            let target: ScriptEntityHandle = ScriptEntityHandle::from_steelval(iter.next()?).ok()?;
            if iter.next().is_some() { return None; }
            Some(Intent::Attack { actor, target: target.entity_id() })
        }
        _ => None,
    }
}
```

脚本侧写法：`(list 'attack target)`，其中 `target` 是某次查询函数（例如「视野内最近的敌对实体」）返回的 `ScriptEntityHandle`，而不是脚本自己拼出来的数字——这正是本设计要解决的问题。`target` 若在 `resolve` 真正结算之前已经死亡（`Arena::get` 查不到），`resolve` 阶段按既有降级哲学处理（作废这一步，不产生效果），与 `resolve_open_door` 目的地不是门时的处理方式同构。

## 五、批量查询原语

### 5.1 为什么批量是同一个模式（ADR 0017 第二档）在集合上的推广

ADR 0017 定的第二档：「声明受限公式 → 编译成平铺指令数组 → Rust 侧小循环求值，无分配、无 VM」。批量原语是同一个模式套在集合上：

```scheme
;; 逐个问答：每次 entity-handle-health 都是一次跨界，100 个实体 = 100 次
(filter (lambda (e) (> (entity-handle-health e) 50)) entities)
;; 100 × 326ns ≈ 32.6µs

;; 批量：一次跨界，Rust 侧扫完整列
(entities-query entities '((filter health > 50)))
;; 1 × 326ns + Rust 侧线性扫描（对几百到几千实体量级可忽略）
```

**额外收益，不只是"跨界次数少"**：薄层人口本来就是列式存储（ADR 0004/0017），厚层 `Arena<Agent>` 虽然是行式（AoS，理由见 `agent.rs` 模块文档——数量少、随机访问、一次读全部字段），但批量筛选/聚合关心的通常只是某一个属性——若把批量原语实现成"遍历 `Arena` 时只抽取需要的那个字段"，对连续内存的顺序扫描本身就比逐次 `Arena::get`（每次都是一次随机访问加一次跨界）快，即使不重新组织存储也有缓存局部性收益；若未来某个属性的批量查询证明是真正的热路径，可以再单独为那个属性建一份列式缓存（同 `TerrainTable` 的模式），但这是可选的后续优化，不是本设计的前提。**批量 API 因此不只是"跨界次数少"，Rust 侧的线性扫描本身也比来回跨界的逐实体访问模式更快，这与既有的列式设计哲学是同一个方向，不是新增负担。**

### 5.2 典型批量原语清单

设计为一个小型声明式 DSL——脚本用符号+数据描述一串操作，宿主一次性接收并在 Rust 侧执行整条链，不是每步都跨界一次。属性引用与算子引用都是**固定符号词表**，不是任意脚本代码——这本身就是「声明式受限公式」的字面含义。

#### 起点：如何拿到一个初始集合

```scheme
(world-entities-in-view actor-handle)      ; 该实体 FOV 内的全部实体
(world-entities-hostile-to actor-handle)   ; 敌对于该实体的全部实体（在 FOV 内）
```

两者都返回一个**集合句柄**（见 5.3 节），不是 Steel 原生列表。

#### 筛选（filter）

| 算子 | 形状 | 说明 |
|---|---|---|
| 按属性比较 | `(filter <attr> <cmp> <value>)` | `<attr>` ∈ 固定词表（`health`/`mana`/`level`/`wallet`……），`<cmp>` ∈ `> < >= <= = !=`，`<value>` 是整数字面量 |
| 按归属 | `(filter-affiliation <kind> <content-id>)` | `<kind>` ∈ `faction`/`guild`/`profession`……，`<content-id>` 是命名空间字符串，宿主解析成 `ContentIndex` 做整数比较 |
| 按距离 | `(filter-within-distance <origin-handle> <max>)` | 距离用 `TorusSize::delta` 算出的带符号位移换算成**平方欧氏距离**做比较（避免 `sqrt`——`ll-world::noise` 模块文档已经论证过 IEEE 754 超越函数跨平台不确定性的问题，同样的顾虑适用于这里：任何距离比较只用整数平方值，不引入浮点开方），`<max>` 也按平方值传入，脚本侧若要传"实际距离 5"，需要自己写 `(filter-within-distance origin 25)`，文档需要把这条换算规则写清楚 |
| 按可见性 | 用 5.1 节的 `world-entities-in-view` 作为起点即是"按可见性筛选"，不需要单独的 `filter-visible` 算子——可见性判断本身依赖 FOV（引擎层，见 ADR 0018），只能整体查询，不能作为一个可以在既有集合上追加的筛选条件 |

#### 投影（project）

```scheme
(project <set> <attr>)   ; 返回该集合每个成员某一列属性值，Steel 整数列表
```

这是唯一"必须把 N 个值真正搬进脚本"的原语——用于脚本确实需要逐个数值做脚本侧计算的场景（例如显示、或者聚合原语覆盖不到的自定义统计）。一次跨界内部产生 N 次小分配（构造 Steel 列表本身的成本），但仍然只是**一次**函数调用，不是 N 次跨界——与"逐个问 `entity-handle-health`"有本质区别。

#### 聚合（aggregate）

```scheme
(aggregate <set> <attr> 'count)     ; 集合大小，不需要 attr，attr 传 #f 或省略
(aggregate <set> <attr> 'sum)       ; 整数和
(aggregate <set> <attr> 'min)
(aggregate <set> <attr> 'max)
(aggregate <set> <attr> 'average)   ; 见下方 Milli 说明
```

**整数语义，用 `Milli` 表示分数**：`average` 内部用 `ll_core::scaled::Milli`（`pub struct Milli(pub i64)`，千分制定点数）算出结果，但**跨界时不把 `Milli` 包成 `Custom` 句柄**——`Milli` 没有不变式（任意 `i64` 都合法，`crates/ll-core/src/scaled.rs` 已有先例），它的底层就是一个基本类型，按本文档「拷贝语义 vs 句柄语义」的划分原则（第一节表格：句柄只用于有意义结构/防伪造需求的类型，基本类型直接跨界），`average` 直接返回 `Milli.0`（缩放后的原始整数），并额外注册一个零参函数 `(milli-scale)` 返回缩放系数（当前是 1000）——脚本用 `(quotient result (milli-scale))` 取整数部分、`(remainder result (milli-scale))` 取小数部分，不需要在每个 mod 里手写魔法数字 1000。

#### 排序与截取

```scheme
(sort-by <set> <attr> <order>)      ; <order> ∈ 'asc / 'desc，稳定排序 + 见 5.4 节确定性规则
(take <set> <n>)                    ; 取前 n 个
(nearest <set> <origin-handle> <n>) ; sort-by 距离升序 + take 的合并快捷方式，
                                     ; 允许 Rust 侧用部分选择算法而不必对整
                                     ; 个集合排序（大集合取小 n 时更快，属于
                                     ; 实现细节，不改变对外语义）
```

#### 分组

```scheme
(group-by <set> <attr>)   ; 返回一个"分组结果"句柄，见 5.3 节
```

### 5.3 集合本身是不透明句柄——这是设计的关键部分

**结论：是。** 批量原语返回的集合（以及分组结果）本身也是 `Custom` 包装的不透明句柄，不是 Steel 原生列表。

```rust
// crates/ll-script/src/api/handle.rs（续）

/// 一批实体的不透明引用。内部用 `Rc<[EntityId]>` 而非 `Vec<EntityId>`
/// ——`FromSteelVal` 取值时会 `.cloned()` 整个值（第二节已核实的机制），
/// `Rc` 让这次 clone 是引用计数自增（O(1)），不论集合里有多少个实体，
/// 都不会在"这条查询链的每一步"重新分配/拷贝底层数据。这是「链式操作
/// 不应该被返回路径吃掉收益」这条要求在类型层面的落地：`filter` →
/// `sort-by` → `take` 三步各自产出一个新的 `EntitySetHandle`，但底层
/// `Rc<[EntityId]>` 只在真正"数据变了"（筛选、排序、截取产生了不同的
/// 元素集合/顺序）时才重新分配一次，句柄本身的跨界传递不额外付出
/// O(n) 代价。
#[derive(Debug, Clone)]
pub struct EntitySetHandle(std::rc::Rc<[EntityId]>);

impl steel::rvals::Custom for EntitySetHandle {}

/// 一次 group-by 的结果：按分组键的 `ContentIndex` 升序排列的
/// (键, 子集句柄) 对——用 `Vec` 而不是 `HashMap`，升序排列本身就是
/// 确定性保证的一部分（见 5.4 节）。同样包一层不透明句柄，避免把
/// "有多少个分组、每组多大"这类信息强行摊平成 Steel 原生结构。
#[derive(Debug, Clone)]
pub struct GroupedEntitySetsHandle(std::rc::Rc<Vec<(ll_core::ident::ContentIndex, EntitySetHandle)>>);

impl steel::rvals::Custom for GroupedEntitySetsHandle {}
```

配套的小工具函数（脚本需要"看见"分组结果的结构时用，仍然不整体materializing）：

```scheme
(grouped-keys grouped)               ; 返回全部分组键（ContentIndex 打包成整数）的 Steel 列表——
                                      ; 分组数量通常是"势力数"/"职业数"这个量级（几十个），
                                      ; 不是实体数量，materializing 这一层是安全的
(grouped-subset grouped content-id)  ; 取某个分组键对应的 EntitySetHandle
(entity-set-count set)               ; 集合大小（等价于 (aggregate set #f 'count)，
                                      ; 提供是因为这是最常用的操作，值得有单独的名字）
(entity-set->list set)               ; 把集合"打开"成 Steel 列表（Vec<ScriptEntityHandle>）——
                                      ; 唯一真正把 N 个句柄逐个搬进脚本的原语，用于脚本确实
                                      ; 需要逐个处理时（见 5.4 逃生舱），本身仍是一次跨界，
                                      ; 但内部是 O(n) 次小分配，且不再需要后续的"用句柄查
                                      ; Arena"这类额外跨界——每个元素已经是可以直接用的句柄
```

**为什么不是"批量原语返回 Steel 原生列表，每个元素是一个 `ScriptEntityHandle`"**：如果每次 `filter`/`sort-by`/`take` 都把结果重新摊平成 Steel 列表（哪怕列表元素本身是句柄），链式调用的每一步都要付出 O(n) 次 `IntoSteelVal` 转换（构造 Steel 列表结构本身的分配），三步链就是 3×O(n)。用 `EntitySetHandle` 包一层 `Rc<[EntityId]>`，中间步骤不需要"打开"成列表，只有最终 `entity-set->list`（或脚本真正要逐个处理时）才付这个代价一次——这正是协调者要求里「如果批量筛选的结果要逐个拷进 Steel 才能用，那批量的收益就被返回路径吃掉了」要避免的情况。

### 5.4 逃生舱及其可见代价

有些谓词确实无法用 5.2 节的固定词表声明式表达（例如某个 mod 定义的复杂条件，涉及跨多个属性的自定义公式）。**不禁止逃生舱**——那会逼着 mod 作者做不成事。逃生舱的形状：

```scheme
(define candidates (world-entities-in-view actor))
(define chosen
  (filter (lambda (e) (some-complex-mod-defined-predicate e))
          (entity-set->list candidates)))
```

`entity-set->list` 把集合打开成 Steel 原生列表（一次跨界，O(n) 次句柄构造，但每个句柄构造本身不是"再跨一次界"——它们是同一次 `entity_set_to_list` 调用内部产生的），随后脚本用已经验证通过白名单的 `map`/`filter` 高阶函数（`host.rs` 的验收样本测试已经证明这类模式能通过白名单并正确执行）在 Steel 侧逐个处理。**代价可见的地方在于**：若 `some-complex-mod-defined-predicate` 内部还要调用其他句柄查询函数（例如 `entity-handle-affiliation`）来做判断，那每次调用都是一次独立的 326ns 跨界——对 100 个候选实体、每个谓词内部查 1 个属性，就是 100 × 326ns ≈ 32.6µs；对协调者示例里的一万个实体，就是 1 万 × 326ns ≈ 3.26ms——在 16.6ms 的单帧预算里已经是**显著**的比例（约 20%），若这类调用发生在每帧热路径而不是 E1 描述的"偶发决策事件"上，会直接影响帧率观感。

**加载管理界面应当如何显示这类开销（新增机制，不同于 ADR 0016 已有的静态分档展示）**：ADR 0016「配套：把开销做成可见的」一节描述的是**注册期静态声明**的档位（内容作者在声明某项内容时选了一/二/三档，加载时就能统计），本节讨论的逃生舱不是"声明"，是脚本运行时的临时行为——同一段脚本代码，在候选集合是 3 个实体时完全无害，在候选集合是 1 万个实体时就是显著开销，**无法在加载期静态判断**。因此需要一种运行期的、与 ADR 0016 静态分档互补的机制：

- `ScriptEngine`（`crates/ll-script/src/host.rs`）在实例上新增一个原子调用计数器，每次 `call_raw` 自增。
- 每个游戏 tick 结束时，把"本 tick 内该 mod 的脚本引擎产生了多少次跨界调用"这个数字喂给诊断面板（复用 `ll_mod::load_report` 已有的诊断类型形状，或新增一个平行的运行期诊断类型——具体接线属于实现任务，本设计只给出"需要一个运行期计数器 + 面板展示"这个形状）。
- 面板展示阈值可以参考本节算出的具体数字（1 万次跨界 ≈ 3.26ms ≈ 单帧预算的 20%）设定"高开销"的显示门槛，而不是拍脑袋定一个数字——**这正是任务要求"应当在界面上明确显示为高开销"的具体落地方式**。

这是本设计**新提出**的机制，不是 ADR 0016 已有内容的简单复用——两者互补：静态分档管"内容声明时选了哪一档"，运行期计数器管"脚本运行时实际产生了多少次跨界"，前者防不住"声明是一档但脚本里另外写了个逃生舱循环"这种情况，后者补上这个盲区。

### 5.5 确定性（约束 C3）

三条具体规则，任何一条不满足都会导致存档重放在跑到某个巧合触发平局的 tick 时悄悄分叉——这类缺陷极难定位（等到玩家反馈"读档后世界不一样了"时，触发平局的那次排序早已经过去几百个 tick）：

1. **排序必须是全序，平局按 `EntityId` 升序打破**：`sort-by` 的实现内部先按声明的 `<attr>` 排序（稳定排序，`ll_script::api::ordered::sorted_by_key` 已有），键相等的元素之间**再**按 `EntityId` 的既有 `Ord`（`(index, generation)` 字典序，`crates/ll-world/src/entity/id.rs` 已经派生）排序。这不是本文档发明的新规则——`crates/ll-sim/src/timeline.rs`「同刻打破平局」一节已经用 `EntityId` 升序解决了完全同构的问题（时间轴上同一 `Tick` 多个实体行动时的弹出顺序），本设计直接复用同一条约定，理由相同：保证"排序结果只由排序键 + 一个固定的、与输入到达顺序无关的平局规则决定"。
2. **`group-by` 的遍历顺序按分组键（`ContentIndex`）升序**，不使用 `HashMap`——5.3 节 `GroupedEntitySetsHandle` 已经是 `Vec<(ContentIndex, _)>` 而不是哈希表,天然满足。`ContentIndex` 的分配顺序本身由 mod 加载顺序决定（`ll-mod::registry` 已有机制），同一份存档 + 同一组 mod + 同一个加载顺序，`ContentIndex` 的值恒定，因此按它排序天然可重放。
3. **`take`/`nearest` 在存在并列时必须有确定结果**：`take` 建立在已经全序排序的集合之上（规则 1），天然确定。`nearest` 若用部分选择算法（而非"先排序再截取"）实现，选择算法本身在"距离相等的多个候选中该选哪个"这一点上必须同样按 `EntityId` 升序打破平局——**实现时必须显式测试这一点**（例如构造两个到 `origin` 距离相等、`EntityId` 不同的实体，断言 `nearest` 结果与 `EntityId` 顺序一致，且与两个实体在 `Arena` 里的注册顺序无关），不能默许"部分选择算法碰巧選中誰就是誰"这种不确定行为。

**已知的一处需要脚本作者自己注意的边界**：`filter-within-distance` 的平方距离比较、`aggregate` 的 `Milli` 定点数运算，都是纯整数运算，不引入浮点——但若未来任何批量原语的实现不慎引入了浮点比较（例如误用 `f64` 距离而不是整数平方距离），会重新踩中 `ll_world::noise` 模块文档已经论证过的 IEEE 754 跨平台不确定性问题。本设计的清单（5.2 节）刻意把所有数值运算限定在整数域，这条边界需要在实现时用测试钉住（例如在不同优化级别下跑同一批查询，断言结果字节级相同）。

### 5.6 只读约束不变——批量的「写」仍然走 `Intent`/`Effect`

批量原语（5.2/5.3 节）全部是只读——与单实体句柄（第三节）同样的类型层面保证：这些函数签名里没有一个接受 `&mut WorldState`，物理上无法被注册成可写函数。

**批量产出 Intent 的设计**：与批量*读*不同，批量*写*的收益来源不一样，需要分开论证。约束 E1（规格 §9.2）已经把决策频率限定在"事件驱动，非 tick 驱动"——单个商人年均约 50 次决策事件，这个量级本身就远小于批量读可能面对的"视野内上千实体"，批量读的 326ns×N 问题在写路径上本来就不严重。批量 Intent 产出真正有用的场景是**同一次脚本调用要为多个受同一逻辑控制的实体产出决策**（例如一个法术同时对 5 个召唤物下达指令，或者行为树的某个节点同时驱动一小队随从），而不是"一万个实体各自决策"这种量级。

形状设计：

```scheme
;; 脚本返回值从"单个 Intent 形状"扩展为"Intent 形状的列表也合法"
(list
  (cons summon-1 (list 'move 'north))
  (cons summon-2 'wait)
  (cons summon-3 (list 'attack nearest-enemy)))
```

宿主侧 `parse_intent` 需要扩展成 `parse_intents`（复数），识别顶层是不是一个"`(cons handle intent-shape)` 的列表"，是则对每个 `cons` 分别解析出 `(actor, Intent)`，返回 `Vec<Intent>`；不是则退回既有的单 `Intent` 解析路径（`actor` 由宿主提供，行为不变）——**这是对现有 `parse_intent` 的扩展，不是替换**，单实体场景的既有调用方式与测试完全不受影响。

这份返回值本身是"这次脚本调用最终交回给宿主的结果"，不是查询链的中间产物——因此即使它是把多个 `(handle . intent)` pair 摊平成 Steel 列表，也不违反"链式操作不应该被返回路径吃掉收益"这条原则：终点本来就需要把结果吐给 Rust 一次，与"查询链中间步骤不要来回搬"是两个不同阶段的问题。

## 六、与 C1（唯一写入口）的关系小结

| 机制 | 是否可能违反 C1 | 保证方式 |
|---|---|---|
| `ScriptEntityHandle` 单读 | 否 | 类型层面：注册函数签名不接受可变世界引用 |
| `EntitySetHandle`/`GroupedEntitySetsHandle` 批量读 | 否 | 同上 |
| 句柄跨帧持有 | 不直接违反，但可能被滥用 | 见 3.5 节：句柄本身不是状态，脚本若把句柄存进跨调用变量，与"脚本存了个整数当记忆"是同一类既有违规，用既有检查手段（`tools/ll-datacheck`，未来任务）覆盖，不需要新机制 |
| `Intent::Attack`/批量 `Intent` 产出 | 否 | 沿用既有的"脚本只产出 `Intent`，`resolve`→`Effect`→`apply` 走既有管线"模式，脚本从未接触 `Effect`/`apply` |

## 七、开放问题与未决事项（如实标注，不假装已解决）

1. **`filter`/`sort-by` 等原语的属性词表目前是固定枚举**（`health`/`mana`/`level`……），mod 若想让自己新增的自定义属性也能参与批量查询/排序，当前设计没有覆盖——这需要属性词表本身可通过注册表扩展（类似 `TerrainTable` 的列式扩展模式），是一个真实的后续设计缺口，本文档不在此展开解决方案，只如实标注存在。
2. **`Arena<Agent>` 的行式（AoS）存储下，批量筛选单一属性的缓存局部性收益有多大，没有实测**——5.1 节的论证是理论推导（顺序扫描优于随机访问），不是基准测试数据；若未来证明厚层实体的批量查询确实是热路径，可能需要为高频查询的属性单独建一份列式缓存（如 `TerrainTable` 模式），本设计只指出这个可能性，不预先决定要不要做。
3. **运行期跨界调用计数器（5.4 节）的具体接线方式**（存在 `ScriptEngine` 实例上、还是存在某个全局诊断汇总结构上、tick 边界如何触发上报）未设计到实现级别的细节，只给出了"需要一个计数器 + 面板展示"这个形状，留给后续任务。
4. **`SteelVal::Custom` 在 `equal?`/哈希/原生容器中的具体行为未测试**（第二节已列出），本设计的接口形状不依赖这些行为，但如果未来某个功能需要用到，需要先补测试。
