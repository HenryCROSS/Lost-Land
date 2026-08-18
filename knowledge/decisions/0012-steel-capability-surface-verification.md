# ADR 0012 — Steel 标准库能力面实测（补 ADR 0001 未测的部分）

- 日期：2026-08-18
- 状态：已采纳
- 版本：`steel-core` 0.8.2，Rust 1.97.1，Windows 11
- 探针代码：`crates/ll-script/examples/probe.rs`（一次性验证工具，不是产品代码）

## 背景

ADR 0001 回答了「脚本能否搞死游戏进程」，测的是中断机制本身。但它没有回答
「`Engine::new()` 默认给脚本多大的能力面」——这是「能力不存在 > 规则禁止」
整套 mod 架构的地基。若默认标准库本身就带着文件/网络/进程/线程能力，
「脚本拿不到随机种子」这种针对单一 API 的收窄毫无意义，因为脚本能绕过 API
直接跟操作系统对话。

本 ADR 逐项实测「Mod API 设计」草案里标了「未核实」的能力，**如实记录**，
包括排除不了的项。

## 实测方法

`Engine::new()` 与 `Engine::new_sandboxed()` 各构造一次，分别尝试：直接调用
（不 `require`）、显式 `(require-builtin ...)` 后调用、以及事后用公开 API
覆盖模块/绑定后再调用。凡涉及「是否真的执行了系统调用」的项（进程、文件），
用可观察的副作用验证（子进程真实打印一行文本到 stdout），不满足于「函数调用
不报错」这种弱证据。

## 逐项结论

### 1. 进程执行——**默认暴露，`new_sandboxed()` 不排除**

`(command "cmd" (list "/c" "echo" "hi"))` 在 `Engine::new()` 与
`Engine::new_sandboxed()` 下**都不需要任何 `require`**，直接可调用——`steel/process`
模块在 `ALL_MODULES`（两种构造路径共用同一份引导程序）里以**不带 `as` 前缀**
的 `require-builtin` 被导入，函数名直接落进全局作用域。

`(spawn-process (command "cmd" (list "/c" "echo" "probe-ok")))` 在两种引擎下
都**真实拉起了子进程**——`probe-ok` / `sandbox-probe-ok` 实际打印到了探针进程
的 stdout（不是脚本内部的字符串，是操作系统真的执行了 `cmd.exe`）。

**这是本轮最大的发现：`new_sandboxed()` 这个名字具有误导性，它不隔离进程执行。**

对策见「必须落地的缓解措施」一节。

### 2. 文件系统——**`new_sandboxed()` 真排除，`new()` 不排除**

`Engine::new()`：`(path-exists? "Cargo.toml")` 直接返回 `#true`，能读真实文件系统。

`Engine::new_sandboxed()`：同样的调用报 `FreeIdentifier`——sandboxed 版的
`steel/filesystem` 模块（`fs_module_sandbox()`，见 steel-core 源码
`src/primitives/fs.rs:127`）**注册了零个函数**，`require-builtin` 能成功但
导入不到任何绑定。**文件系统在 sandboxed 引擎下是真排除，不是靠脚本自律。**

### 3. 网络——**`new_sandboxed()` 真排除（模块压根没注册），`new()` 不排除**

`Engine::new_sandboxed()` 下 `(require-builtin steel/tcp)` 直接报
`module not found: steel/tcp`——TCP/HTTP/轮询三个模块在 `register_builtin_modules`
里被 `if !sandbox { ... }` 整体跳过（steel-core 源码
`src/steel_vm/primitives.rs:643-653`），**连模块本身都不存在**，不是「存在但
未导出」，脚本没有任何路径能够得着。

`Engine::new()` 下同样的 `require-builtin` 成功——网络模块已注册，只是没有被
`ALL_MODULES` 自动导入到全局作用域，脚本主动 `require` 即可拿到。

**结论：网络必须以 `new_sandboxed()` 为地基，`new()` 不可用。**

### 4. 系统时间/墙钟——**不能排除，需要主动清空**

`Engine::new_sandboxed()` 下，`(require-builtin steel/time) (instant/now)`
**成功返回真实 `std::time::Instant`**。`steel/time` 模块在
`register_builtin_modules` 里是**无条件注册**的（不在 `if !sandbox` 分支内），
`sandbox` 标志对它完全不生效。

### 5. 非 `DetRng` 随机源——**不能排除，需要主动清空**

同样：`Engine::new_sandboxed()` 下 `(require-builtin steel/random)
(rng->gen-usize)` 成功返回真实 OS 随机数（两次运行分别取到
`4772403194304568961`、`302239452657557478`——确认是真随机，不是常量）。
`steel/random` 模块同样无条件注册，`sandbox` 标志不生效。

### 6. 跨帧隐式可变状态（原生线程）——**当前依赖配置下不可达，但是巧合不是设计**

`(spawn-native-thread ...)` 报 `Generic: the feature needed for
spawn-native-thread is not enabled.`——这依赖 Cargo feature 门控（`sync` 之类），
`ll-script` 的 `Cargo.toml` 用的是 `steel-core` **默认 feature 集**
（`["std", "modules"]`），恰好没有打开使这个函数生效的 feature。

**这条排除是特性组合的副作用，不是我们主动设计的边界**——任何人往
`ll-script` 的 `Cargo.toml` 加一行 feature（哪怕是为了别的目的引入 `sync`），
都可能让这条路重新打开。**必须在 Cargo.toml 里用注释钉住「不可加 sync
feature」这条约束**，并在 CI 层面（`tools/ll-datacheck` 或类似）加一条对
`Cargo.lock` / feature 集合的检查，防止未来被静默打开。

`steel/threads` 模块本身仍然被 `ALL_MODULES` 无前缀导入到了全局作用域
（`spawn-native-thread` 这个名字是绑定着的，只是绑定的函数值在被调用时
自己报错，不是「未绑定」），这点也需要在设计里记住：**名字存在 ≠ 能力存在**，
两者是两回事，报告能力面时必须分开说。

### 7. 无序容器迭代顺序——**Steel 内置哈希表遍历顺序不稳定，不能暴露**

同一进程内，用完全相同的构造语句 `(hash 'a 1 'b 2 'c 3 'd 4 'e 5)` 分别造出
`h1`、`h2` 两个哈希表，`(hash-keys->list h1)` 与 `(hash-keys->list h2)`
**不相等**（两次实际运行分别得到 `(a c e b d)` 对比另一个不同排列、
`(b c a d e)` 对比另一个不同排列）。这说明 Steel 的哈希表用了带随机种子的
哈希器（类似 Rust `std::collections::HashMap` 的 `RandomState`，每次构造
哈希表实例都重新播种），**不仅跨进程不稳定，同一次运行内、内容完全相同的两个
哈希表实例之间都不稳定**。

**结论：不存在「固定种子就能稳定」这种退路，脚本侧原生哈希表遍历原语
必须完全不暴露。** 这正是简报里 `ordered.rs` 存在的理由——任何需要顺序参与
逻辑的场景，必须走宿主提供的确定性排序原语，不能给脚本任何直接接触
`hash-keys->list` 之类原语的路径。

## 必须落地的缓解措施（真实验证过，不是设想）

`Engine::register_module(&mut self, module: BuiltInModule)` 是公开 API，
用同名的空模块覆盖注册表条目，**在脚本第一次运行之前**执行，可以真实堵死
「脚本显式 `require-builtin` 拿到危险能力」这条路：

```rust
engine.register_module(BuiltInModule::new("steel/random"));
engine.register_module(BuiltInModule::new("steel/time"));
engine.register_module(BuiltInModule::new("steel/threads"));
engine.register_module(BuiltInModule::new("steel/process"));
```

实测验证：覆盖 `steel/random` 后，脚本再执行
`(require-builtin steel/random) (rng->gen-usize)`，得到
`FreeIdentifier: rng->gen-usize`——彻底拿不到。

**但这个覆盖必须在 `ALL_MODULES` 引导程序跑完之后、脚本代码跑之前执行**，
因为 `steel/process`（以及 `steel/threads`）已经在引导阶段被**无前缀** require
过一次，`command`、`spawn-process!`、`spawn-native-thread` 这些名字**已经**
绑定进全局作用域——覆盖模块注册表**不会追溯撤销已经完成的绑定**。对这批
「引导期已泄漏」的名字，必须额外用 `engine.register_value(name,
SteelVal::Void)` 之类的方式**逐个覆盖已绑定的全局名字**，把它们变成调用即
报错的哨兵值。实测：把 `spawn-native-thread` 覆盖成 `SteelVal::Void` 后再调用，
得到 `BadSyntax: Function application not a procedure`——**是 `Err`，不是
panic**，符合防线要求。

`ll-script::host` 的 `ScriptEngine::new()` 因此必须做两件事，顺序不能反：

1. `Engine::new_sandboxed()` 打底（真排除文件系统与网络，进程与线程需要下一步补）。
2. 立即执行上述模块覆盖 + 已绑定名字覆盖，再往下才注册游戏自己的 API 与运行脚本。

**这一版手工点名 `command`/`spawn-process!` 的做法后来被放弃**：`ll-script`
实现阶段（见下方「追加实测」一节）发现 `steel/process` 暴露的名字比预想
多、`steel/meta` 更是有 102 个导出、且模块覆盖对 `steel/time` 确认无效——
手工点名清单必然漏项。最终方案改成「枚举模块全部导出名字后整体清空」+
「源码文本层面禁止 `require-builtin`/`require`」两道防线，不再手工维护
危险名字清单。

## 性能实测（跨 VM 调用延迟，不是原子读探针）

| 调用方式 | 均摊耗时 | 说明 |
|---|---|---|
| `engine.run(新源码字符串)` | 327~400µs | 每次都重新词法分析+编译，不适合热路径 |
| `run_raw_program`（预编译字节码重放，固定字面量） | 560~580ns | 跳过解析，但没有从 Rust 侧传新参数 |
| `call_function_by_name_with_args`（预注册函数 + 每次传新参数，裸调用） | 74ns | 未包中断防线的下限值 |
| **`call_function_by_name_with_args` 包一层 `InterruptHandler::run_with_timeout`** | **326ns** | **`ScriptEngine::call` 实际要用的路径——见下方说明** |
| `Engine::new()` 整引擎重建 | 55~58ms | 200 次均摊，量级稳定 |
| `environment_offset()` + 脚本新增绑定 + `rollback_to_checkpoint()` | 21.4~21.9µs | 比整引擎重建快约 2500 倍 |

**关于 326ns 这一行**：任务简报要求「每次脚本调用都经过 `call`，是四道防线的
落点」，意味着每次调用都要包一层中断保护，不能只在裸调用上报数字——76ns 这个
数字若被误当成「防线包装后」的成本会造成误导。实测：套上
`InterruptHandler::run_with_timeout`（含一次原子 store、一次线程 `unpark`、
调用体、一次 channel `send`、`controller.resume()`、原子 store）后，均摊耗时
从 74ns 涨到 326ns，约 4.4 倍，但**仍是纳秒级**，与 ADR 0001「看门狗开销
2000 轮实测 0.96x，零开销」的结论方向一致——**每帧每实体级别的高频调用
（技能结算、行为树 tick）在主线程直接调用是可接受的，不需要另开线程**。

`engine.run(字符串)` 这条路径慢了 4000 多倍，**只能用于加载期一次性执行 mod
源码，绝不能用于运行期热路径**——这也印证了裁定 P4-4 里「注册期可以进 VM，
运行期不再进 VM」的分界线：即使运行期真要进 VM（第三档开销），也必须是
"函数已注册好、直接传参调用"，不能是每次现编译源码。

`rollback_to_checkpoint` 比整引擎重建快约 2500 倍，是「一键重载 mod」场景下
更便宜的选项，但语义不同——它只回滚全局绑定表，不清空已经产生的副作用
（比如已经 spawn 的原生线程，虽然本项目已确认线程能力当前不可达）。是否
用它替代整引擎重建留给任务 6/9（mod 生命周期）决策，本 ADR 只提供数字。

## 追加实测——`steel/meta` 是比预想大得多的口子，且模块覆盖对 `steel/time` 确认无效

「必须落地的缓解措施」一节写完初稿后，在 `ll-script` 实现阶段（`host.rs`）
继续按图施工时发现两个问题，都已改用更稳的方案，如实记录发现过程：

**发现一：`steel/meta` 实测导出 102 个名字**，混着 `eval!`、`run!`
（`super::meta::EngineWrapper::call`，能在脚本内部构造一个**全新、完全不
受本文件任何限制**的 `Engine::new()`）、`Engine::new`/`Engine::clone`/
`Engine::add-module`、`env-var`/`maybe-get-env-var`（读宿主环境变量）、
`set-env-var!`（改宿主环境变量，steel-core 源码里这个函数体本身标了
`unsafe`）、`load`/`eval-string`/`eval`（把字符串当代码执行，能绕过任何
「脚本源码里没写 XXX」式的静态检查）——同时也混着 `value->string`、
`arity?`、`function-name` 这类完全无害的内省函数。这个模块同样在
`ALL_MODULES` 里被无前缀 `require-builtin`，`command`/`spawn-native-thread`
那套「引导期已经绑进全局作用域」的问题在这里更严重，因为口子多了几十个。

**对策：不逐个甄别，整个模块清空。** 逐个挑「这个danger那个不danger」
在 102 个名字的规模下必然会漏项（而且会随 steel-core 版本升级持续漏），
本项目现阶段的 mod 脚本也不需要这个模块的任何能力（task 5 的 API 完全
基于宿主自己 `register_fn` 的函数，不需要脚本自省）。实测：枚举
`engine.builtin_modules().get("steel/meta").unwrap().names()` 后逐个
`register_value(name, SteelVal::Void)`，`(eval! "(+ 1 2)")` 与
`(run! (Engine::new) "(+ 1 2)")` 都变成 `Err`，验证有效。

**发现二：「覆盖模块注册表」这条缓解手段对 `steel/time` 确认无效，且
排查未能在预算内查明根因。** 用 Rust 侧内省确认过：覆盖后
`engine.builtin_modules().get("steel/time")` 返回的模块 `names()` 长度
确实是 0；但脚本 `(require-builtin steel/time) (instant/now)` 在**全新、
从未被任何其他引擎接触过的独立进程**里依然成功返回真实
`std::time::Instant`。用同样手法处理 `steel/random` 则每次都能可靠拦截。
排查过「进程级缓存」「跨引擎共享 Arc」等几个假设，均被对照实验排除
（`steel/random` 用的是完全相同的代码路径、完全相同的共享结构，行为却不同）；
怀疑与 `require-builtin` 的宏展开（`src/parser/expand_visitor.rs`，
`self.builtin_modules.get(s.resolve())`）在编译期读取的 `ModuleContainer`
实例与运行期被我们覆盖的实例存在不为人知的分歧路径有关（可能是 Kernel
内部持有独立的模块快照），但未能在本任务预算内继续深挖到底。

**如实记录：这条缓解手段目前只能定性为「对部分模块有效、对至少一个模块
（`steel/time`）确认无效」，不能整体宣称「模块覆盖排除了随机与时间」。**

**因此改为不依赖它作为唯一防线**：新增 `reject_dangerous_syntax`，在源码
**编译之前**，纯文本层面拒绝任何包含 `require-builtin` 或 `(require ` 字面
子串的脚本源码。这条防线不依赖 steel-core 内部任何缓存/解析路径的行为，
只要求「这两个词不出现在源码文本里」，因此对 `steel/time` 同样有效——
实测 `(require-builtin steel/time)` 在文本关卡直接被拒绝
（`ScriptError::ParseError`），根本不会进入编译。**mod 脚本设计上本来就
不需要写 `require`**：所有能力都通过宿主预先注册好的函数直接调用，这条
限制不损失任何设计内的合法用法。

**最终防线结构（三层，缺一不可）：**

1. **源码文本关**（`reject_dangerous_syntax`，在 `load_source` 里、编译之前）
   ——拒绝任何出现 `require-builtin`/`(require ` 字面文本的源码。这是
   `steel/time`/`steel/random`/`steel/tcp` 这类「需要脚本显式 require 才能
   拿到」的能力被排除的**真正原因**，模块覆盖对它们而言只是锦上添花。
2. **枚举清空**（`poison_module`，构造期）——对 `steel/process`、
   `steel/threads`、`steel/meta` 这类**引导期已经无前缀绑进全局作用域**
   的模块，逐个枚举导出名字并用 `SteelVal::Void` 覆盖。这是它们被排除的
   真正原因，因为源码文本关挡不住「名字已经绑定、脚本不需要写 require
   就能直接调用」这种情况。
3. **模块注册表覆盖**（`poison_module` 的第二步）——尽力而为，对
   `steel/random` 确认有效，对 `steel/time` 确认无效，保留是因为它不产生
   任何已知副作用、且未来 steel-core 版本升级后也许能生效。

## 错误对象的源码位置信息

`SteelErr::span() -> Option<Span>`，`Span { start: u32, end: u32, source_id }`
是字节偏移区间，不是行列号。实测对 `(+ 1 undefined-identifier)` 求值，
拿到 `Some(Span { start: 5, end: 25, .. })`，精确框住了
`undefined-identifier` 这段文本在源码字符串里的字节范围。

**结论**：`log.rs` 能够上报「源码中哪一段文本触发的错误」，但只有字节偏移，
换算成行号/列号需要宿主侧自己用换行符位置表做一次映射（一次性、按源码文本
长度线性，不是运行时热路径成本）。

## 后果

1. `ScriptEngine::new()`（任务 3）必须以 `Engine::new_sandboxed()` 为唯一
   合法起点，`Engine::new()` 不得出现在产品代码里。
2. 构造后必须立即执行模块覆盖 + 已绑定危险名字覆盖两步，顺序在任何脚本
   运行之前。任务 3 的 TDD 循环需要为此新增测试，覆盖「覆盖后仍尝试逃逸的
   脚本得到 Err 而非成功」。
3. `Cargo.toml` 不得添加 `sync`（或任何已知会打开 `spawn-native-thread` 的）
   feature，需要注释钉住，长期应补一条自动化检查。
4. mod API 表面（任务 5）的跨 VM 调用必须走 `call_function_by_name_with_args`
   一类的预注册路径，不能现场拼源码字符串再 `run`。
5. 任何暴露给脚本的「集合」类查询，只能是宿主侧提供的有序结构（`ordered.rs`），
   不能是 Steel 原生哈希表的直接遍历——原生遍历顺序连"同一次运行内可复现"
   都做不到。

## 踩坑记录

- `Engine::new_sandboxed()` 这个名字本身会让人误以为它排除了进程执行，
  实测证明并不排除。写文档/写代码注释时必须明确写「排除文件系统与网络，
  不排除进程/线程/时间/随机」，不能笼统写「sandboxed」。
- `steel/threads` 当前不可达是 Cargo feature 未打开的副作用，不是主动设计，
  必须当成脆弱假设持续盯防，而不是当成已解决问题。
