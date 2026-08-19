# ADR 0012 — Steel 标准库能力面实测（补 ADR 0001 未测的部分）

- 日期：2026-08-18
- 状态：已采纳
- 版本：`steel-core` 0.8.2，Rust 1.97.1，Windows 11
- 探针代码：`crates/ll-script/examples/probe.rs`（一次性验证工具，不是产品代码）

> **「本体即 Mod」用法已被 [ADR 0018](0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) 限定为仅玩法层成立，本文其余实测结论不受影响。**

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

## 追加实测二——从黑名单换成 AST 白名单（可行，已落地）

背景：项目所有者指出 `reject_dangerous_syntax` 是黑名单，只能拦住写进
清单里的写法，拦不住未来 `steel-core` 新增的、没人预料到的口子。要求
评估并尽量实现「语法树层面的形式白名单」——不在允许列表里的标识符，
不论以什么方式出现，一律拒绝，包括我们没想到的。

### 三个可行性问题的实测答案

**1. `steel-core` 0.8.2 是否暴露「先解析后求值」的 API？——是。**

`Engine::emit_fully_expanded_ast(&mut self, expr: &str, path: Option<PathBuf>)
-> Result<Vec<ExprKind>>` 是公开 API，返回完整的、可编程遍历的 AST
（`steel::parser::ast::ExprKind`），且**不会**顺带执行脚本——用它拿到树、
校验完再决定要不要真的 `run`，是这条防线成立的前提。

**2. 宏展开会不会绕过白名单？校验能否在展开之后？——会绕过，但可以在
展开之后校验，实测验证过。**

`emit_fully_expanded_ast` 给出的**就是**完整展开之后的树。用
`crates/ll-script/examples/probe_whitelist.rs` 第 1 节实测：对
`(require-builtin steel/time) (instant/now)` 调用它，`require-builtin`
已经展开成一串

```scheme
(define instant/now
  (%module-get% %-builtin-module-steel/time (quote instant/now)))
```

这样的 `define`——真正被引用的名字（`instant/now`、`%module-get%`、
`%-builtin-module-steel/time`）全部摆在明面上，即使白名单从没听说过
`steel/time` 这个模块名，只要这三个名字不在白名单里，遍历到它们就会
被拒绝。**校验对象必须是这份展开后的树，不能是展开前的**（展开前只有
一个 `Require` 节点，看不出脚本最终引用了什么）。

**3. 运行时能否构造出调用（字符串拼符号再求值）绕过白名单？——实测：
符号构造本身不构成绕过，但依赖 `eval!`/`run!` 类反射入口本身也在白名单
之外。**

用 `probe_whitelist.rs` 第 4 节实测：脚本执行

```scheme
(define parts (list "require" "-" "builtin"))
(define word (string->symbol (apply string-append parts)))
```

真的拼出了一个字面等于 `require-builtin` 的**符号值**，`displayln`
确认打印出来。但这只是数据构造——`require-builtin` 是编译期宏，只认
字面出现在源码文本里的 `(require-builtin ...)` 形式，运行时拼出来的
符号值不会触发任何宏展开或模块加载。真正能让「拼出来的代码」产生效果的
唯一途径是 `eval!`/`run!`/`eval-string`/`load` 这类反射入口——这些名字
本身就是普通的被引用标识符，会在脚本引用它们的那一刻被白名单挡下（已
实测：`crates/ll-script/src/host.rs` 的
`白名单挡住脚本自己拼出来的require_builtin符号` 测试完整跑了上面这段
拼接脚本，`string->symbol` 调用成功、返回值正确，但没有任何
`require-builtin` 副作用发生）。

### 结论：白名单挡住了黑名单挡不住的东西

黑名单（`reject_dangerous_syntax`）只在源码文本里出现字面
`require-builtin`/`(require ` 时生效；白名单不依赖这个前提。用
`host.rs` 的 `白名单挡住没有出现require字样的裸引用` 测试验证：脚本源码
`(some-future-unknown-builtin-capability)`——不含任何黑名单关键词，
模拟"未来 steel-core 新增的、我们从没见过的内置能力"——被白名单直接
拒绝（`ScriptError::ParseError`）。这正是黑名单结构性做不到的事：黑名单
拦的是"我们想到的坏词"，白名单拦的是"任何不在好词单里的词"。

### 实现要点（`crates/ll-script/src/whitelist.rs` + `host.rs`）

- 白名单基础集合来自两处：`SAFE_MODULES`（18 个确认纯计算、无 I/O、无
  反射能力的 Steel 内置模块，运行期枚举其真实导出名）与
  `ScriptEngine::register_fn` 注册的每一个函数名（自动加入，调用方不需要
  手工同步）。刻意不收录 `steel/io`/`steel/filesystem`/`steel/ports`/
  `steel/json`/`steel/syntax`/`steel/git`，即便沙箱版某些模块本身已经
  零导出——这份列表只收录"确认过安全"的模块，不收录"恰好现在是空的"
  模块，两者的安全性来源不同。
- **必须区分"自由引用"与"局部绑定"，否则会把合法脚本也拒了**：实测
  `(define (add a b) (+ a b))` 完整展开后，函数体引用的是卫生宏重写过的
  `##a2`/`##b2`，不是原始形参名 `a`/`b`。若不做作用域跟踪，任何带参数的
  函数定义都会被误判成引用了白名单外的标识符。做法是标准的自由变量
  分析：遍历时维护"当前作用域内绑定了哪些局部名"（来自 `let` 绑定、
  `lambda` 形参、`define` 的函数名+形参），只有不在这个集合里的引用才
  对照白名单，实现在 `whitelist.rs` 的 `walk`/`collect_bound_names`。
- **跳过 `quote` 包住的部分**：脚本用符号表达数据是正常用法（本项目
  `api/intent.rs` 约定脚本用 `(list 'move 'north)` 表达意图），`'north`
  不是"引用"，是字面量，不检查。
- **不允许脚本自定义宏**（`ExprKind::Macro`/`SyntaxRules` 直接拒绝）：
  宏能在展开期生成任意新代码，是比 `require-builtin` 更难静态审查的
  攻击面，本阶段整体拒绝，没有已知的合法 mod 需求需要它。
- 校验开销：`load_source` 里对同一份源码多解析一次
  （`emit_fully_expanded_ast` 之外，`run` 内部还会再解析一次），刻意
  接受——mod 加载是一次性的加载期操作，不是每帧热路径，ADR 0012 前面
  「性能实测」一节已经量出单次 `run(源码字符串)` 是百微秒量级，多付
  一次完全在可接受范围。用 `probe_whitelist.rs` 第 5c 节实测过：先
  `emit_fully_expanded_ast` 再对同一份源码 `run`、甚至重复 `run` 两次，
  都不产生重复定义报错或其他副作用。

### 现在黑名单（`reject_dangerous_syntax`）扮演什么角色

**降级为快速失败的前置优化，不再是权威防线。** 源码里出现字面
`require-builtin` 几乎没有合法用途，在真正付出「解析 + 完整宏展开」的
成本之前用一次字符串 `contains` 拦掉，省一次编译；删掉这一层不会有
任何能力重新泄漏——白名单会独立、完整地挡住同样的东西（已用
`whitelist::tests::require_builtin展开后引用的模块内部名字被拒绝` 验证：
这个测试直接调用 `check_whitelist`，完全绕开 `reject_dangerous_syntax`，
依然正确拒绝）。

### 已知边界（诚实记录，不留在会话里）

- `SAFE_MODULES` 的 18 个模块是否真的"确认过安全"依赖人工审阅每个
  模块导出的函数列表——本次审阅了名字与模块用途，**没有逐个函数体验证
  实现细节**（例如 `steel/strings`/`steel/lists` 里是否存在某个函数
  间接读了进程环境）。这是比黑名单更小、但不是零的残余面。
- 若未来某个安全模块（如 `steel/hash`）新增了一个不安全的导出函数
  （steel-core 版本升级），本机制会**自动**把新函数纳入白名单（因为
  是运行期枚举 `module.names()`，不是写死的名字列表）——这是白名单
  相对黑名单的优势在别处失效的一个例外：对于"已经在安全名单里的模块
  新增了不安全函数"这一种情况，白名单不比黑名单更安全，需要升级
  `steel-core` 版本时人工复核这 18 个模块有没有新增导出。
- `steel/meta` 完全清空（[`FULLY_POISONED_MODULES`]）与白名单是两套
  独立机制，`steel/meta` 里 `value->string`/`arity?` 这类无害的内省
  函数目前**没有**被单独放行进白名单——若后续任务发现 mod 作者确实
  需要，应该走"给白名单显式加一个名字并写清楚理由"的路径，不是恢复
  整个 `steel/meta` 模块。

## 待查项（未解决，明确记录，不随会话消失）

**`steel/time` 模块注册表覆盖为什么失效，根因未查清。**

已排除的可能性：
- **不是**因为模块本身没被真正清空——Rust 侧内省确认
  `engine.builtin_modules().get("steel/time").unwrap().names().len()`
  在覆盖后确实是 0，与 `steel/random`（覆盖后确认有效）用的是完全相同
  的覆盖代码路径。
- **不是**进程级或跨引擎共享状态——`probe_isolate.rs` 在两个完全独立、
  互不干扰的进程里分别单独测试 `steel/time` 与 `steel/random`，结果
  依然一个失效一个生效。
- **不是**"先请求过一次就被永久缓存"——`steel/random` 在被覆盖测试之前
  已经在另一个引擎实例上被成功请求过一次（`probe.rs` 第 5 节），随后
  在新引擎上覆盖依然生效，说明不存在"第一次成功 require 就永久解锁"
  这种缓存机制（至少 `steel/random` 不是这样，无法解释为何 `steel/time`
  表现不同）。

尚未验证、留给后续有余力时查证的方向：`require-builtin` 的宏展开
（`src/parser/expand_visitor.rs`，`self.builtin_modules.get(s.resolve())`）
在编译期读取的 `ModuleContainer` 实例，与我们运行期覆盖的
`Engine.modules` 是否是**同一个** `Arc`——`Kernel::new()` 内部持有一个
独立的 `Box<Engine>`（用于宏展开），如果 `require-builtin` 的展开实际上
走的是 Kernel 内部那个引擎的模块注册表而不是外层引擎的，覆盖外层引擎
就不会生效；但这个假说需要解释为什么 `steel/random` 又不受影响，尚未
找到自洽的解释。

**这条待查项目前不影响产品代码的正确性**——`whitelist.rs` 从另一个
完全独立的角度（校验展开后的 AST 而不是覆盖模块注册表）同样能排除
`steel/time`，已经过测试验证。但根因不明意味着**不能假设这个模式在
未来遇到的新模块上会重演或不会重演**，任何新增的"清空模块"操作都应该
像本次一样，用 `probe_isolate.rs` 式的独立进程实测去验证，不能凭这次
的经验类推。

### 重新评估触发条件

- `steel-core` 升级版本时，逐一重跑 `probe.rs`/`probe_isolate.rs`/
  `probe_whitelist.rs` 三份探针，确认标准库范围、模块覆盖有效性、AST
  展开形状三件事都没有变化。
- 若发现 `require-builtin` 展开机制的官方文档或源码注释提到 Kernel 与
  外层引擎的模块解析分工，回来补上这条待查项的根因。

## 追加实测三——白名单的定位是能力边界，不是语言子集（项目所有者裁定）

背景：项目所有者指出「追加实测二」交付的白名单太窄——`steel/meta` 整体
清空挡死了 `make-struct-type`（用户自定义 `struct` 的底层依赖），
`ExprKind::Macro`/`SyntaxRules` 被直接拒绝挡死了 `define-syntax`。裁定：
**必须挡住的是能力**（文件系统、网络、进程、线程、墙钟、非确定性随机，
以及能触达以上任意一项的反射入口），**必须放行的是语言本身**（闭包、
递归、宏、`quote`/`quasiquote`、`let` 族、高阶函数、列表/向量/哈希表
作为数据结构、字符串与数学运算、用户自定义 `struct`）——「纯的东西被
挡住」是白名单的缺陷，不是安全特性。这条原则本身以及判断标准（"能不能
到达六类被禁能力之一"，不是"我们有没有把握"）写进了
`crates/ll-script/src/whitelist.rs` 模块文档「白名单的定位」一节，
不重复贴在这里。

### 实测发现的三个真实 bug（不是"设计选择"，是纠正错误）

1. **`define-syntax`/`syntax-rules` 被无条件拒绝**——实测
   `crates/ll-script/examples/probe_whitelist.rs` 第 7 节：宏定义本身在
   `emit_fully_expanded_ast` 的输出里完全消失（`(define-syntax my-when
   ...) (my-when #t (+ 1 2))` 完整展开后就是裸的 `(+ 1 2)`），宏的每一次
   使用都被替换成它展开出的普通代码，会照常被白名单检查——宏本身不提供
   任何绕过白名单的额外能力。改为放行 `ExprKind::Macro`/`SyntaxRules`
   两个节点。
2. **`steel/meta` 整体清空挡死了 `struct`**——`(struct Point (x y))`
   展开后依赖 `make-struct-type`、`#%vtable-update-entry!`、
   `#%struct-property-ref` 等一批纯 VM 内机制（无 I/O，不触达任何被禁
   能力），全部挂在 `steel/meta` 下。改为逐项审查 102 个导出名字，
   只拒绝确认危险的 44 个（见 `host.rs` 的 `META_DENY_LIST` 系列常量及
   其分类注释），其余（含 `struct` 依赖的、`call/cc`、`box`/`unbox`、
   错误处理、函数内省等）默认放行。
3. **手工维护的"安全模块"清单遗漏了整个 Scheme 标准库**——`map`/
   `filter`/`foldl`/`foldr` 等高阶函数根本不是任何 `BuiltInModule` 的
   导出，是 `steel-core` 自带的 `src/scheme/stdlib.scm` 用 Scheme 写的
   `define`，跟着 prelude 直接落进全局作用域，不挂在任何模块下——手工
   枚举模块列表（`SAFE_MODULES`，18 个）天生看不到它们。用验收样本一测
   就撞见：`map` 被白名单拒绝。改用 `Engine::globals()`（公开 API，
   返回编译器符号表里当前全部全局名字）在毒化前拍一次快照，减去
   `poisoned_identifiers()`（`FULLY_POISONED_MODULES` 的全部导出 +
   `META_DENY_LIST` 系列）即为白名单——一次性收全，不再需要手工枚举
   任何模块名。`host.rs` 的 `compute_allowed_identifiers` 文档记录了
   顺序要求：必须在真正执行毒化**之前**拍快照（毒化只改值，不删符号表
   条目，顺序反了快照会把毒名字也收进去）。

### 白名单的作用域跟踪也有一个真实 bug：顶层/`begin` 序列不能用只读快照

实测过程中还发现（不是设计裁定要求的，是修 `struct` 时连带撞见的）：
`struct` 宏展开成一个大 `begin` 块，块内先 `(define struct:Point
(quote uninitialized))` 占位，后面兄弟表达式再 `(set! struct:Point
...)` 引用它；顶层脚本同理，`(define p (Point 1 2))` 后面接
`(define (f) (list p))` 引用 `p` 是最常见的写法。早期实现给每个表达式
一份只读的 `locals` 快照，前一条 `define` 引入的名字对同层级后一条兄弟
表达式不可见，会被误判成自由引用而拒绝。改为 `locals` 全程可变
（`&mut HashSet`），`walk_sequence` 在顶层/`begin` 序列内按顺序遍历、
`define` 遇到就把名字塞进同一份可变集合供后续兄弟表达式使用；`let`/
`lambda`/单独出现的 `define` 仍然各自克隆一份**不泄漏**的临时作用域。
细节与理由见 `whitelist.rs` 里 `walk`/`walk_sequence` 的文档注释。

### 验收：一份有分量的 Lisp 程序，留在测试里防止白名单退化

`crates/ll-script/src/host.rs` 的
`验收样本一份有分量的内容定义脚本能通过白名单并算出正确结果` 测试：
定义三件带 `base-value`/`weight` 的 `Item`（自定义 `struct`），用一个
`define-syntax` 宏包一层构造语法糖，用 `map`/`foldl`（高阶函数）求
每件物品的"价值密度"与总价值，用一段真正的递归（不借用内置 `max`）
找出密度最高的一件。真实运行结果 `'(680 "gem" (10 250 4))`——
100+500+80=680 的总价值、gem 密度 500/2=250 全场最高、三件物品密度分别
是 10/250/4——全部对得上手算结果，不是只验证"通过了白名单"。这个测试
留在套件里：将来任何人收紧白名单，只要动到这份样本用到的任何一个特性，
测试立刻变红。

### 与「本体即 Mod」的联系（规格 §10.3，项目所有者指出）

本体内容与 mod 走完全相同的 API，没有特权通道。白名单太窄的后果不只是
"mod 作者受限"，而是本体自己也写不出内容来——与 ADR 0016 的守门规则
同源：若本体需要一个 mod 够不着的东西，那是 API 缺陷，不是特性。

### 保留在拒绝名单上的项目（如实说明理由，不是含糊地"不确定所以挡住"）

`META_DENY_LIST`/`META_DENY_LIST_2`/`META_DENY_LIST_3` 共 44 个名字，
逐条分类理由见 `host.rs` 常量文档；这里只点两个容易被质疑"是不是保守
过头"的：

- **`inspect`**：具体实现未逐行审查（102 个名字里体量最大的一批之一，
  预算内没有做到每个都读源码），无法证明它纯净，也没能证明它一定触达
  被禁能力——按"不确定就默认拒绝，且写清楚不确定在哪"处理，不是含糊
  地"整体太复杂所以挡住"。
- **异步/future 一族**（`poll!`/`block-on`/`local-executor/block-on`/
  `futures-join-all`/`join!`）：没有已知的异步 I/O 源可以喂给它们
  （网络/文件已经排除），但引入独立的调度/重入语义，与"跨帧隐式可变
  状态"是同一类风险，且没有任何已证明的 mod 合法需求要用到它们——两个
  条件同时满足（有风险、无需求）才保守拒绝，不是单纯"看着陌生就拒绝"。

若后续任务证明以上判断有误（比如某个 mod 场景确实需要异步），应该
针对那个具体名字单独复核并写清楚新的判断依据，而不是恢复整批。
