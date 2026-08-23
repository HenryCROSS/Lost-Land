# 0028 — Steel 引擎构造期的偶发内存破坏：定位、逐条否决的五条假说、暂不改产品代码

**日期**：2026-08-22
**状态**：已生效（结论为「不改产品代码」，缓解方案待项目所有者裁定）
**关键提交**：本 ADR 自身
**探针代码**：`crates/ll-script/examples/probe_engine_churn.rs`（复现工具，不是产品代码——与 ADR 0012 的 `probe.rs` 同一个定位）
**影响范围**：`crates/ll-script/src/host.rs`（`ScriptEngine::new`/`load_source`）、`crates/ll-mod/src/pipeline.rs`（`load_one_script`）、`scripts/ci/run_tests.sh` 这道门禁的可信度、`ll-game` 的启动路径
**版本**：`steel-core` 0.8.2（现用）与 0.8.3（实测更差，已否决）

## 背景

`cargo test -p ll-mod --tests` 长期有约三分之一的概率**不是测试失败，而是进程被打断**——`STATUS_ACCESS_VIOLATION`、野指针解引用、或 `steel-core` 编译分析 pass 里的 `unwrap()` panic。这不是某一批改动引入的：基线提交 `ff6bbfa` 本身就是这个概率。

这直接损害门禁本身的可信度：「跑一次绿了」不再是任何证据，而「跑红了」也可能与本次改动毫无关系。

## 怎么复现

**门禁口径**（本 ADR 全部「故障率」都指这个）：

```bash
for i in $(seq 1 24); do cargo test -p ll-mod --tests > run_$i.log 2>&1; echo "run $i rc=$?"; done
```

判定标准是**进程退出码**，不是测试断言——野指针崩溃不产生 `test result: FAILED`，它直接杀掉整个测试二进制。

**基线实测**（`steel-core` 0.8.2，两批各 12 次，中间隔了约两小时）：

| 批次 | 次数 | 失败 |
|---|---|---|
| 第一批 | 12 | 4 |
| 第二批 | 12 | 4 |
| **合计** | **24** | **8（33%）** |

两批相隔约两小时、数字完全一致，所以 33% 是这台机器上门禁口径的典型值。

**但这个数字本身也会漂移，别把它当常数**：后来那次交错 A/B 里，同样是默认配置、同样的门禁口径，16 次只崩了 2 次（12.5%）。也就是说**故障率在 12% 到 33% 之间随机器状态游走**。

**由此得到一条必须遵守的方法论纪律**：

- 口径缩小到**单个测试二进制**时漂移更凶——`example_mod_traits` 单独跑 300 次崩 7 次，几十分钟后同一个二进制（文件时间戳确认未重新链接）再跑 200 次一次都不崩。
- **任何「改前 vs 改后」的对照都必须交错跑**（一轮 A、紧接一轮 B，重复 N 轮），不能 A 跑一批、B 跑一批。本次调查里，非交错的对照曾经伪造出一个 `p≈0.03`、看起来很扎实、但完全不存在的效应，见「假说一」。

## 观测到的崩溃签名

| 签名 | 出现位置 |
|---|---|
| `STATUS_ACCESS_VIOLATION (0xc0000005)` | 最常见，无任何输出 |
| `misaligned pointer dereference: address must be a multiple of 0x8 but is 0x4` | `steel-core/src/compiler/passes/mod.rs:355`/`:358`/`:366` |
| `misaligned pointer dereference: ... but is 0x23afa689d9f` | `thin-vec-0.2.19/src/lib.rs:681` |
| `called Option::unwrap() on a None value` | `steel-core/src/compiler/passes/analysis.rs:947` |
| `called Option::unwrap() on a None value` | `steel-core/src/compiler/passes/analysis.rs:1493` |
| `memory allocation of 85899345936 bytes failed` | 分配失败中止，无栈 |

最后那一条尤其能说明性质：`85899345936 == 0x14_0000_0010`，也就是 **20 × 2³² + 16**——一个本该是小数字的长度字段，高 32 位被塞进了垃圾。这是「读到坏内存」最直白的形态，任何栈深度假说都解释不了它。

**崩溃点不是固定的两处**：上表这几行是实测碰到过的位置，随批次还会冒出新的（`analysis.rs:1493` 就是后来才第一次撞见的）。「哪一行炸」取决于**坏数据先被谁碰到**，本身没有信息量——不要把某一行当成缺陷所在。

**`0xc0000409 STATUS_STACK_BUFFER_OVERRUN` 不是上表之外的又一种签名，更不是栈溢出的证据**——它是 Rust 在 Windows 上 `abort()` 的固定退出码（`__fastfail(FAST_FAIL_FATAL_APP_EXIT)` 复用了这个 NTSTATUS 值）。实测每一次 `0xc0000409` 都是上表某一行的收尾动作：野指针那几行前面跟着 `thread caused non-unwinding panic. aborting.`，分配失败那行前面跟着 `memory allocation of ... failed`。两者都走 `abort()`，于是共用同一个退出码。

**这个名字具有极强的误导性，本次调查里它把两批人各带偏过一次**：`STATUS_STACK_BUFFER_OVERRUN` 字面意思是「栈缓冲区溢出」，看到它很自然会去查栈。但真正的栈溢出 Rust 会先打印 `has overflowed its stack`——0.8.2 的全部失败日志里**一次都没有出现过**。

## 崩溃点：不在我们的脚本里，在 steel-core 自己的引导编译期

`RUST_BACKTRACE=full` 抓到两份完整调用栈，路径一致（只是最后落点差一两行、递归层数一份 2 层一份 5 层）。下面是摘录，略去了 panic 机制本身的十几帧与几个转发帧：

```
core::panicking::panic_misaligned_pointer_dereference
steel::compiler::passes::VisitorMutUnitRef::visit<...AnalysisPass>   passes/mod.rs:358
steel::compiler::passes::analysis::impl$8::visit_list                analysis.rs:1167
steel::compiler::passes::analysis::Analysis::run_with_scope          analysis.rs:579
steel::compiler::compiler::Compiler::lower_expressions_impl          compiler.rs:1298
steel::compiler::compiler::Compiler::compile_executable              compiler.rs:757
steel::steel_vm::engine::Engine::run                                 engine.rs:1367
steel::steel_vm::engine::Engine::new_sandboxed                       engine.rs:1293   <=== 这里
ll_script::host::ScriptEngine::new                                   host.rs:547
ll_mod::pipeline::load_one_script                                    pipeline.rs:544
```

崩溃发生在 **`Engine::new_sandboxed()` 内部**——steel-core 在编译它自己的内置 prelude，此时本仓库的 mod 脚本一行都还没被读到。这一条就推翻了「脚本越大越容易崩」这个直觉：出问题的 AST 不是我们的。

## 逐条假说与实测结论

用 `crates/ll-script/examples/probe_engine_churn.rs` 把这条路径上的每个因素拆开加压。判定标准同样是进程退出码；「样本」列是**周期总数**，「失败」列是**崩溃的进程数**。

| 模式 | 每周期做什么 | 经本仓库代码 | 样本 | 失败进程 |
|---|---|---|---|---|
| `pure` | `Engine::new_sandboxed()` | 否 | 16 进程 × 1000 = 16000 | **0** |
| `pure`（2 MiB 栈线程） | 同上 | 否 | 16 × 300 = 4800 | **0** |
| `pure`（8 MiB 栈线程） | 同上 | 否 | 16 × 300 = 4800 | **0** |
| `pureast` | 构造 + `emit_fully_expanded_ast` | 否 | 80 × 60 = 4800 | **0** |
| `purerun` | 构造 + `run` | 否 | 80 × 60 = 4800 | **0** |
| `pureload` | 构造 + `emit_fully_expanded_ast` + `run` | **否** | 80 × 60 = 4800 | **1** |
| `hardened` | `ScriptEngine::new()` | 是 | 15600 | **0** |
| `load` | `ScriptEngine::new()` + `load_source()` | 是 | 100 进程 × 60 = 6000 | **3** |
| `load`（12 线程并发） | 同上 | 是 | 20 × 12 × 60 = 14400 | **0** |
| `reuse` | **只构造一次**引擎，反复 `load_source()` | 是 | 80 × 60 = 4800 | **0** |

### 假说一：栈溢出（Rust 测试线程默认栈太小） — **否决，五条独立证据**

1. **崩溃那一刻的递归只有两层。** 抓到的完整栈总共 **53 个栈帧**，其中递归的 `VisitorMutUnitRef::visit` 只出现 **2 次**（另一份是 57 帧、5 次）。爆栈需要几万层。这一条是直接测量，不是推断——**它单独就足以判死这个假说**。「栈顶落在递归分发 `match` 上」看起来像爆栈，但只要数一下栈帧就知道栈几乎是空的。
2. **没有爆栈提示。** Rust 在 Windows 上遇到真正的栈溢出会先打印 `has overflowed its stack`。0.8.2 的全部失败日志里一次都没有（唯二两次出现在 0.8.3 批次，见下）。
3. **`0xc0000409` 不是栈的证据**，见上一节——它是 Rust `abort()` 的固定表现。
4. **小栈不加剧、大栈不缓解。** `Engine::new_sandboxed()` 在只有 2 MiB 栈的线程上连跑 4800 次一次不崩；最小复现 `pureload` 跑在主线程 8 MiB 栈上照崩。
5. **门禁口径交错 A/B**（每一轮先跑一次默认栈、紧接着跑一次 `RUST_MIN_STACK=33554432`，共 16 轮，用交错抵消机器状态漂移）：

   | 条件 | 次数 | 失败 |
   |---|---|---|
   | 默认栈 | 16 | 2 |
   | `RUST_MIN_STACK=33554432`（32 MiB） | 16 | 2 |

   **完全没有差别。**

**这里有一个差点把人带偏的坑，值得单独记下**：**非**交错地先跑 24 次默认栈（8 次失败）、再跑 24 次 32 MiB 栈（2 次失败），会得到「32 MiB 把故障率从 33% 压到 8%」这个看起来很有说服力、`p≈0.03` 的结论——**它是假的**，交错重测直接归零。机器状态漂移足以伪造出这个量级的差异。**任何 A/B 都必须交错跑。**

退一步说，即便 `RUST_MIN_STACK` 真的让故障率下降，也**不构成修复**：幸存的失败签名与基线一模一样（`passes/mod.rs:366` 野指针、访问违例），不是栈溢出被消除，而是更大的线程栈改变了进程的地址空间布局，让同一个野指针有更大概率落在已映射的内存上。**把一个内存破坏缺陷从「崩溃」改成「静默读到垃圾」是更坏的结果，不是修复。**

顺带排除的相邻选项：`steel-core` 的 `stacker` 特性（`stacker::maybe_grow` 按需扩栈）在 0.8.2 里**只出现在 `primitives/transducers.rs` 一处**，`compiler/passes/` 下一次都没有——就算爆栈假说成立，开这个特性也够不着崩溃点。

### 假说二：Steel VM 的线程安全 / 我们的 `thread_local!` 用法 — **否决**

- **最小复现是单线程的**（`pureload`、`load` 都在 `threads=1` 下复现）。
- 反向证据更强：**提高并行度反而压低故障率**——`load` 模式 12 线程跑了 14400 个周期一次不崩，而单线程 6000 个周期崩 3 次；16 路进程并行跑同一个测试二进制 192 次也是 0 失败。这与「并发竞态」的方向完全相反。
- `steel-core` 的实际编译特性是 `["default", "modules", "std"]`（读 `target/debug/.fingerprint/steel-core-*/lib-steel.json` 确认），**`sync` 特性没有开**。这意味着 steel-core 的 `Shared<T>` 就是 `Rc<T>`，而它全部的引擎级全局状态（`KERNEL_IMAGE`/`KERNEL_IMAGE_SB`、`VTABLE`、`ROOTS`、`FUNCTION_TABLE`、`DEFAULT_PRELUDE_MACROS`）在 `not(feature = "sync")` 下都是 `thread_local!`，跨线程共享的只剩 `ThreadedRodeo` 字符串驻留表、几个 `Mutex`/`RwLock` 包住的映射、和若干 `AtomicUsize` 计数器——都是线程安全的。
- 本仓库全部 crate 里只有一处 `unsafe impl`（`ScriptAllocGuard` 的 `GlobalAlloc`），没有任何 `unsafe impl Send/Sync`。
- `ScriptAllocGuard` **根本没有装进 `ll-mod` 的测试二进制**（`#[global_allocator]` 只在 `crates/ll-game/src/main.rs` 与 `crates/ll-script/tests/memory_budget_enforced.rs` 两处声明），因此与本次崩溃无关。

### 假说二之二：开 `steel-core` 的 `sync` 特性（换掉 thread_local 内核） — **否决，实测毫无差别**

「每条线程各自 bootstrap 一整套 `thread_local!` 内核、线程退出时各自析构，析构期 use-after-free」是个听起来很能自洽的故事。开 `sync` 之后 steel-core 会把 `KERNEL_IMAGE`/`VTABLE`/`ROOTS` 从 `thread_local!` 换成进程级 `Lazy` + 锁，`Shared<T>` 从 `Rc` 换成 `Arc`——如果故事成立，这个开关应当有明显效果。

编译零错误，门禁口径 24 次：

| 条件 | 次数 | 失败 |
|---|---|---|
| 默认特性（基线） | 24 | 8 |
| **加 `features = ["sync"]`** | 24 | **8** |

**一模一样**，签名也一样（访问违例 + `analysis.rs` 系列 panic，另冒出一处新的 `analysis.rs:1604`）。这个开关既不缓解也不加剧。已还原，`Cargo.toml` 不带 `sync`。

### 假说三：升级 `steel-core` 能修 — **否决，实测更差**

`steel-core` 0.8.3 于 2026-08-20 发布（0.8.2 是 2026-02-22）。升级后编译零错误，但门禁口径：

| 版本 | 次数 | 失败 |
|---|---|---|
| 0.8.2（基线） | 24 | 8（33%） |
| **0.8.3** | 24 | **16（67%）** |

签名完全相同（`analysis.rs:955`、`passes/mod.rs:355`、访问违例），只是位置随行号偏移。**并且 0.8.3 多出一种 0.8.2 从未出现过的失败模式**：两次真正的 `has overflowed its stack`。

上游 2026-02 至 2026-08 之间唯一与内存安全沾边的提交是 PR #663「Attempt to fix bug in gc visitor double counting」——但它改的是 `values/closed.rs` 里 `ParallelMarker` 的并行标记路径，整段在 `#[cfg(feature = "sync")]` 下，**我们的构建根本不编译它**。这解释了为什么升级没有帮助。

结论：**不要升级到 0.8.3**。`Cargo.toml` 与 `Cargo.lock` 已还原到 0.8.2。

### 假说四：`analysis.rs` 那个 `.unwrap()` 是个独立的逻辑缺陷，由某个畸形 `.scm` 稳定触发 — **否决**

`analysis.rs:947` 是 `arg.atom_identifier().unwrap()`——「lambda 的某个形参不是标识符原子」。这确实是逻辑 panic 而不是内存破坏**的表现形式**，所以值得单独验一次：如果真有某个 `.scm` 是畸形输入，单独反复编译它应当 **100% 失败**（编译器对固定输入是确定性的）。

逐个脚本单独加压（`tolerant` 模式，容忍 `FreeIdentifier` 这类预期编译错误，只统计 steel-core 自己崩掉/panic）：

| 脚本 | 行数 | 样本 | 失败进程 |
|---|---|---|---|
| `lostland/races.scm` | 61 | 40 进程 × 30 次 | 2 |
| `lostland/classes.scm` | 42 | 40 × 30 | 0 |
| `lostland/crafting.scm` | 65 | 40 × 30 | 0 |
| `lostland/subclasses.scm` | 78 | 40 × 30 | 0 |
| `example_mod/gameplay.scm` | **477** | 40 × 30 | **0** |
| `example_mod/behavior.scm` | 102 | 40 × 30 | 2 |
| `example_mod/weather.scm` | 66 | 40 × 30 | 0 |
| `example_mod/terrain.scm` | 19 | 40 × 30 | 0 |
| `example_mod/animation.scm` | **16** | 40 × 30 | **2** |

**没有任何一个脚本是稳定触发的**：命中的三个各约 5%，其余四个 0%，而且**最大的 477 行零失败、最小的 16 行照样命中**——脚本规模与命中率完全不相关。

还有一条结构性证据更强：崩溃栈显示 panic 发生在 `Engine::new_sandboxed()` 里，编译的是 **steel-core 自己的 prelude**，一份**固定不变**的输入；而 `pure` 模式把这同一件事做了 16000 次，零失败。一个确定性的畸形 AST 不可能在固定输入上时而炸时而不炸。

**结论：`947`（以及 `1493`、`1604`）不是独立缺陷，它和野指针崩溃同源**——都是「先碰到坏数据的那一行」。把它们当成两个问题分别修，会修错地方。

## 根因结论

**确定的部分**（都有实测支撑）：

1. 崩溃点在 `Engine::new_sandboxed()` 内部，steel-core 引导编译自己的 prelude 时读到野指针（`0x4`，一个小整数被当成指针）。这是**上游的内存安全缺陷**，不是我们的脚本、不是我们的沙箱加固、不是我们的 `thread_local!`。
2. 触发条件是**同一根线程上反复「构造引擎 + 编译脚本」这个组合**：
   - 只反复构造引擎（16000 次）——不崩；
   - 只构造一次引擎、反复编译（4800 次）——不崩；
   - 两者交替——崩。
3. 复现**不需要多线程、不需要本仓库任何代码、不需要真实 mod 脚本**：`pureload` 模式只用 `steel-core` 的三个公开 API，可以原样作为上游缺陷报告的复现用例。
4. 0.8.3 没修，且更差。

**量级自洽性核对**：`pureload` 的崩溃率约 1/4800 个周期。一次全量 `cargo test -p ll-mod --tests` 里，24 个测试二进制、121 个 `#[test]`、其中多数会调一次 `load_all`，每次 `load_all` 要为 9 个脚本文件各构造一个引擎——周期总数在几百到一千的量级，据此推出的整体失败率与实测 33% 属同一量级。这不是精确预测，但没有出现数量级矛盾。

**还不确定的部分——不要把下面的内容当成结论**：

- **steel-core 内部究竟哪一行制造了这个野指针，没有查到。** `compiler/` 目录下一个 `unsafe` 都没有，说明破坏发生在别处（VM、`values/`、`gc.rs` 的 `unsafe_erased_pointers`；全 crate 约 250 处 `unsafe`），分析 pass 只是第一个踩到它的地方。
- 「反复编译污染了线程局部的 kernel 镜像（`KERNEL_IMAGE_SB`），下一次 `new_sandboxed()` 的 `deep_clone` 因此复制出一棵坏 AST」是**最符合现有证据的假说**，但没有直接证据。
- `pureast`(0/4800) 与 `purerun`(0/4800) 各自不崩、只有 `pureload`(1/4800) 崩——样本量太小，**不足以断定是 `emit_fully_expanded_ast` 与 `run` 的组合才触发**。要判定需要把这三档各加到 ≥1000 个进程。
- 「12 线程反而不崩」这个反直觉现象没有解释。

**下一步该查什么**（留给后续批次）：

1. 把 `pureast`/`purerun`/`pureload` 三档各跑 ≥1000 进程，判定 `emit_fully_expanded_ast` 是不是必要条件。**这是现有证据链里最薄的一环**：目前 1/4800 vs 0/4800 vs 0/4800，三档在统计上分不开。
2. 在 steel-core 的本地 fork 里给 `Engine::deep_clone` 与 kernel 镜像加不变式断言，把「什么时候变坏的」从「踩到它的时候」前移到「制造它的时候」。
3. 用 `-Zsanitizer=address`（需要 nightly）跑 `pureload` 最小复现，直接拿到 use-after-free 的分配/释放栈。
4. 带着 `pureload` 复现用例去上游开 issue——本条的成本最低、期望收益最高。

## 决定：暂不改产品代码

三条候选缓解，逐条说明为什么现在都不动：

- **候选 A：`load_all` 复用同一个 `ScriptEngine`，不再每个脚本文件构造一个。** 实测 `reuse` 模式 0/4800，方向是对的。**但它改变隔离语义**：现在每个脚本文件拿到一个全新 VM，`races.scm` 里的 `define` 对 `classes.scm` 不可见；复用之后 mod 之间能互相看见对方的定义，这与 ADR 0012 的沙箱论证、以及 mod 冲突模型直接相关。**这是产品语义决定，必须由项目所有者裁定，子代理不擅自改。**
- **候选 B：每个脚本文件的加载各占一根新线程。** steel-core 的 kernel 镜像是 `thread_local!` 的，换线程等于换一份全新 kernel，隔离语义完全不变。**但没有实测**——探针里留了 `threadper` 模式，还没跑过数据，也没有量过「每个脚本都重付一次 kernel 引导」的启动开销。**注意这条的理论依据已经被削弱**：`sync` 特性实验（把 thread_local 内核整个换成进程级共享）故障率纹丝不动，说明「内核放在哪」大概率不是变量。仍值得实测，但先验期望应当调低。
- **候选 C：调栈 / 限制测试并行度 / 开 `sync` / 升级依赖。** 四条都已被上面的实测逐条否决。
- **候选 D：把「装载真实 mods」做成进程内共享一次，别让每个测试各装一遍。** 能把编译周期数砍一个数量级，因此能按比例压低命中率。**治标不治本**，而且会削弱「每个测试各自从真实 `mods/` 装一遍」这条现行做法的证据强度——现在每个用例都是端到端独立验证，共享之后就只剩一次。（**订正**：调查过程中有人把这条做法归给 ADR 0018，实际不是——0018 讲的是「脚本层边界按引擎层/玩法层划分」，通篇没有这条；这条做法只体现在 `crates/ll-mod/tests/*.rs` 的写法里，没有任何 ADR 为它背书。）同样属于产品/纪律取舍，交项目所有者裁定。

因此本次交付**只有调查记录与复现探针，不含任何产品代码改动**。

## 后果

- **`bash scripts/ci/run_all.sh` 跑一次绿了，不构成任何证据**——这条必须写进后续任何批次的验收纪律里。基线故障率 33%，一次绿了的先验概率是 67%。判断一次改动有没有引入回归，需要门禁口径至少 12–24 次的统计，而且要和对照组交错跑。
- **这不只是测试的问题，也是产品的问题。** `ll-game` 启动时同样在主线程上跑 `load_all`，同样是「构造引擎 + 编译脚本」交替。按 `pureload` 的量级，装 9 个脚本文件的一次启动大约有千分之几的崩溃概率，且**随玩家装的 mod 数量线性增长**。这不是「测试环境才有」的问题。
- **`steel-core` 版本锁在 0.8.2，升级前必须先用门禁口径跑 24 次对照。** 0.8.3 这次就是靠这条纪律才没有被当成「修复」合进去。

## 顺带查实、但与本次根因无关的两件事

这两件都**没有在本次改动**，只是查根因过程中顺带核实的事实，记在这里免得下次再查一遍。

### `take_pending_writes` 零生产调用方，且这条路已经可达

`ll_script::api::state::take_pending_writes` 在整个 `crates/` 下只有 `state.rs` 自己的测试助手调用它，**没有任何生产调用方**。而 `api::state::register`（注册 `state-set!`/`state-get!`）**已经接在生产路径上**——`ll_mod::script_behavior_source::ScriptBehaviorSource::new` 会调它，而 `decide` 从不清空缓冲。

后果是：行为树脚本一旦调用 `state-set!`，写入会永远攒在线程局部的 `PENDING_WRITES` 里，既不会经 `apply` 落进 `WorldState`，又因为 `state-get!` **先查缓冲区**而在后续每一帧都读得到——看起来像写成功了，实际上存档里没有，且第一帧的写入会被错误归属到第一百帧的决策上。

**这是一处 C1 违反**：约束 C1 要求脚本不持有跨帧状态、VM 可从零重建，而 `PENDING_WRITES` 正是活在 `WorldState` 之外的跨帧隐式状态。`api::state` 自己的模块文档已经警告过这一点。

**当前是潜伏而非已发生**：`mods/` 下没有任何脚本调用 `state-set!`（全目录 grep 为空）。但自 `5862dbe` 行为树能经 `TurnEngine` 驱动 AI 之后，这条路已经可达，第一个写 `state-set!` 的 mod 作者就会踩到。修法（在 `decide` 里取走缓冲、包成 `Effect::SetScriptState`）与本次根因无关，应当单独立项。

### `ScriptEngine::new()` 每次都泄漏一批内存和一根线程

两处，都不是本次崩溃的原因，但都是真实的泄漏：

- `host.rs` 的 `compute_allowed_identifiers`/`poisoned_identifiers` 对**每一个全局名字**做 `Box::leak`，每构造一个引擎泄漏一整份（Steel prelude 的全局名字数量级在千位）。
- `steel-core` 的 `InterruptHandler::drop` 只设了一个 `dropped` 标志，**既不 `unpark` 也不 `join`** 那根看门狗线程——它停在 `park()` 上永远醒不过来。每构造一个 `ScriptEngine` 泄漏一根 OS 线程。

一次 `load_all` 构造 9 个引擎，量级上不致命，所以本次不动；记下来是因为「引擎构造是廉价的、可以随便多构造几个」这个隐含假设并不成立。


---

## 【2026-08-23 订正】缺陷已随脚本系统整体拆除而消失；本 ADR 转为历史记录

**本节是追加的订正，正文一字未改。**

本 ADR 记录的缺陷在 `steel-core` 0.8.2 内部，本项目侧没有任何规避手段
（六条假说全部被数据否决）。项目所有者据此裁定：**去掉整个脚本系统**，
内容改用 JSON5 数据文件、行为逻辑改用引擎内 Rust
（见 [0018](0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
的同日订正段）。

因此：

- `crates/ll-script/` 整个 crate、`steel-core` 依赖、`mods/**/*.scm`
  全部删除。本 ADR 「影响范围」一节点名的文件除
  `crates/ll-mod/src/pipeline.rs`（已重写为纯数据装载）外均不再存在，
  探针 `probe_engine_churn.rs` 亦然。
- 本 ADR 的**结论**（「不改产品代码」）与**缓解方案**（ADR 0029 的
  约束 C6）都不再适用——没有引擎可构造，也就没有那个相邻关系。
- 记录本身仍有价值：它是「为什么最终去掉脚本系统」这个决定的证据链，
  也是「六条假说逐条被数据否决」这套排查方法的范例。**不要据此认为
  当前代码里还有这个缺陷**。

拆除后的实测：完整测试套件连跑 12 次，0 次被打断（拆除前为
17–33%）。
