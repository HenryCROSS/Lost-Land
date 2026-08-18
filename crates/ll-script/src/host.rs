//! Steel VM 宿主封装。
//!
//! # 为什么以 `Engine::new_sandboxed()` 为唯一起点，且仍需主动清空
//!
//! ADR 0012 实测：`Engine::new()` 默认把文件系统与网络都暴露给脚本；
//! `Engine::new_sandboxed()` 排除了这两项（sandboxed 版 `steel/filesystem`
//! 模块零导出、`steel/tcp`/`steel/http` 干脆没注册），但**不**排除进程执行、
//! 原生线程、系统时间、非 `DetRng` 随机——这四项在两种引擎下都默认可达，
//! 必须在构造期主动清空，见 [`ScriptEngine::new`]。

use std::collections::HashSet;
use std::time::Duration;

use steel::SteelErr;
use steel::rvals::SteelVal;
use steel::steel_vm::builtin::BuiltInModule;
use steel::steel_vm::engine::Engine;
use steel::steel_vm::interrupt::InterruptHandler;
use steel::steel_vm::register_fn::RegisterFn;

use crate::whitelist::check_whitelist;

/// 脚本失控时的中断预算：超过这个墙钟时长仍未返回就强制掐断。
///
/// ADR 0001 实测死循环在 500ms 超时下能被稳定掐断；这里取 300ms——够长到
/// 不会误伤正常的重度计算（技能结算不该跑到几百毫秒），也够短到不会让
/// 单个失控脚本卡住一整帧的观感。具体数值属于可调参数，不是精确科学，
/// 后续如有真实卡顿投诉可以再调。
const INTERRUPT_TIMEOUT: Duration = Duration::from_millis(300);

/// 引导阶段无条件注册、且整个模块都不该留给脚本的 Steel 内置模块。
///
/// 见 ADR 0012：这四个模块的注册与 `sandbox` 标志无关，`new_sandboxed()`
/// 挡不住它们，而且它们没有任何"纯计算"用途——时间、随机、原生线程、
/// 进程执行本身就是要挡住的能力，不存在"只清空危险部分、保留安全部分"
/// 这回事，整体清空没有已知代价。
///
/// `steel/meta` **不在这份清单里**：它实测 102 个导出名字，混着
/// `eval!`/`run!`/`Engine::new`/`set-env-var!` 这类逃逸入口，也混着
/// `make-struct-type`（用户自定义 `struct` 的底层依赖）、`call/cc`、
/// `box`/`unbox`、错误处理这类纯语言能力——曾经"整体清空"过，但那样会
/// 把 `struct` 也挡死，而白名单的定位是能力边界、不是语言子集（项目
/// 所有者裁定，见 [`crate::whitelist`] 模块文档）。`steel/meta` 改用
/// [`META_DENY_LIST`] 逐项甄别，见该常量文档。
const FULLY_POISONED_MODULES: [&str; 4] = [
    "steel/random",
    "steel/time",
    "steel/threads",
    "steel/process",
];

/// 清空 [`FULLY_POISONED_MODULES`] 里每一个模块的**全部**导出名字。
///
/// 分两步，缺一不可：
/// 1. 用 `engine.builtin_modules().get(name)` 枚举出模块当前真实导出的每一个
///    名字，逐个 `register_value(name, SteelVal::Void)`——这一步处理的是
///    「引导阶段已经被无前缀 `require-builtin` 导入进全局作用域」的名字
///    （`steel/process`、`steel/threads`、`steel/meta` 都是这种情况），
///    只覆盖模块注册表本身**追溯不到**这些已完成的绑定。
/// 2. 再用同名空模块覆盖模块注册表（`register_module`），阻断脚本之后
///    再显式 `require-builtin` 这几个模块。
///
/// **已知局限（ADR 0012 如实记录）**：第 2 步对 `steel/time` 实测无效——
/// 覆盖模块注册表后，脚本仍能 `(require-builtin steel/time)` 拿到真实
/// `instant/now`，根因未在预算内查清（怀疑是编译期的模块解析走了另一条
/// 缓存路径，没有实际读 `self.modules`）。这正是本文件同时存在
/// [`reject_dangerous_syntax`] 这道独立防线的原因——它不依赖「模块注册表
/// 覆盖是否生效」这个前提,直接从源码文本层面禁止 `require-builtin`/
/// `require`,是这四个模块（尤其是 `steel/time`）真正被排除的原因,
/// 模块覆盖只是锦上添花的第二道防线。
fn poison_module(engine: &mut Engine, module_name: &'static str) {
    if let Some(module) = engine.builtin_modules().get(module_name) {
        for name in module.names() {
            let leaked_name: &'static str = Box::leak(name.into_boxed_str());
            engine.register_value(leaked_name, SteelVal::Void);
        }
    }
    engine.register_module(BuiltInModule::new(module_name));
}

/// `steel/meta`（实测 102 个导出名字）里逐项审过、确认会触达六类被禁
/// 能力之一的名字。**其余名字一律放行**——见 [`crate::whitelist`] 模块
/// 文档「白名单的定位」一节：判断标准是"能不能到达被禁能力"，不是
/// "我们对它有没有把握"；本清单只收录能证明会触达被禁能力、或行为
/// 在不同机器/不同次运行间不一致的名字。逐条分类：
///
/// - **动态代码执行/宏反射**（等价于绕过整套白名单）：`eval!`、`eval`、
///   `eval-string`、`load`、`load-expanded`、`emit-expanded`、`expand!`、
///   `#%expand`、`#%expand-syntax-case`、`#%match-syntax-case`、
///   `#%macro-case-bindings`、`read!`（读字符串为代码/数据，是喂给
///   `eval!` 的前置步骤）。
/// - **构造/操纵独立的、完全不受本文件任何限制的 Engine**：`Engine::new`、
///   `Engine::clone`、`Engine::add-module`、`Engine::modules->list`、
///   `Engine::raise_error`、`run!`。
/// - **读写宿主进程环境**：`env-var`、`maybe-get-env-var`、`set-env-var!`。
/// - **泄漏宿主机信息，跨机器不确定**：`current-os!`、
///   `platform-dll-extension!`、`platform-dll-prefix!`、
///   `steel-home-location`、`path-separator`、`command-line`、
///   `target-arch!`、`feature-dylib-build?`。
/// - **原生代码加载**：`#%build-dylib`、`#%get-dylib`。
/// - **暴露随进程/运行而变的内部值，破坏确定性**（`memory-address` 是
///   真实指针值，`dump-profiler`/`#%snapshot-stacks`/
///   `%#interner-memory-usage`/`active-object-count`/
///   `callstack-hydrate-names`/`make-callstack-profiler`/
///   `current-function-span` 暴露的是运行期 VM 内部状态，两次运行不
///   保证一致）：以上七个。
/// - **调试/测试基础设施，无合法 mod 用途**：`set-test-mode!`、
///   `get-test-mode`、`breakpoint!`。
/// - **异步/future 执行模型**：`poll!`、`block-on`、
///   `local-executor/block-on`、`futures-join-all`、`join!`——没有已知
///   的异步 I/O 源（网络/文件已排除），但引入一整套独立的调度/重入
///   语义，与"跨帧隐式可变状态"是同一类风险，没有已证明的合法需求，
///   保守拒绝。
/// - **模块内省**：`module->exports`——探测模块导出了什么，与"能力
///   不存在"的精神有摩擦（脚本不该有办法探测自己够不着什么），保守
///   拒绝；`inspect` 的具体行为未验证清楚是否会暴露内部结构，同样保守
///   拒绝。
///
/// **放行的例子**（不需要在这里列出，因为白名单默认放行 `steel/meta`
/// 里不在这份清单上的名字）：`make-struct-type`/`struct->list`/
/// `#%struct-property-ref`/`#%struct-update`（用户自定义 `struct` 的
/// 底层依赖）、`call/cc`/`call-with-current-continuation`（纯控制流）、
/// `box`/`unbox`/`set-box!` 及其 weak/strong 变体（纯 VM 内可变单元，
/// 无 I/O）、`error-with-span`/`raise-error`/`call-with-exception-handler`
/// （错误处理）、`value->iterator`/`iter-next!`（通用迭代协议）、
/// `arity?`/`function-name`/`function-arity`（对函数值的内省，不是对
/// 宿主的内省）。
const META_DENY_LIST: [&str; 33] = [
    "eval!",
    "eval",
    "eval-string",
    "load",
    "load-expanded",
    "emit-expanded",
    "expand!",
    "#%expand",
    "#%expand-syntax-case",
    "#%match-syntax-case",
    "#%macro-case-bindings",
    "read!",
    "Engine::new",
    "Engine::clone",
    "Engine::add-module",
    "Engine::modules->list",
    "Engine::raise_error",
    "run!",
    "env-var",
    "maybe-get-env-var",
    "set-env-var!",
    "current-os!",
    "platform-dll-extension!",
    "platform-dll-prefix!",
    "steel-home-location",
    "path-separator",
    "command-line",
    "target-arch!",
    "feature-dylib-build?",
    "#%build-dylib",
    "#%get-dylib",
    "memory-address",
    "dump-profiler",
];

/// [`META_DENY_LIST`] 剩余部分（第二批常量，Rust 数组字面量不便拆成两个
/// 常量合并，用第二个常量 + 合并逻辑更清楚哪几组是同一类理由）。
const META_DENY_LIST_2: [&str; 12] = [
    "#%snapshot-stacks",
    "%#interner-memory-usage",
    "active-object-count",
    "callstack-hydrate-names",
    "make-callstack-profiler",
    "current-function-span",
    "set-test-mode!",
    "get-test-mode",
    "breakpoint!",
    "module->exports",
    "inspect",
    "poll!",
];

/// [`META_DENY_LIST`]/[`META_DENY_LIST_2`] 之外，异步执行模型这一组
/// （第三批，理由见 [`META_DENY_LIST`] 文档「异步/future 执行模型」）。
const META_DENY_LIST_3: [&str; 4] = [
    "block-on",
    "local-executor/block-on",
    "futures-join-all",
    "join!",
];

/// 用 [`META_DENY_LIST`] 系列逐项覆盖 `steel/meta` 里确认危险的名字，
/// **不清空整个模块**——其余名字（`make-struct-type`/`call/cc`/`box`
/// 等）保持宿主 `ALL_MODULES` 引导阶段绑定的原样，会被
/// `compute_allowed_identifiers` 枚举进白名单。
fn poison_meta_deny_list(engine: &mut Engine) {
    for name in META_DENY_LIST {
        engine.register_value(name, SteelVal::Void);
    }
    for name in META_DENY_LIST_2 {
        engine.register_value(name, SteelVal::Void);
    }
    for name in META_DENY_LIST_3 {
        engine.register_value(name, SteelVal::Void);
    }
}

/// 出现在脚本源码里就直接拒绝加载的字面子串。
///
/// **这一层已经降级成快速失败的前置优化，不再是权威防线**——权威防线是
/// 下面的 [`crate::whitelist`] AST 白名单（见 `ScriptEngine::load_source`）。
/// 保留这一层的原因很朴素：源码里出现 `require-builtin` 几乎总是没有
/// 合法用途（本项目的 mod 脚本设计上不需要 `require` 任何 Steel 内置
/// 模块），在真正调用较重的「解析 + 完整宏展开」之前就用一次字符串
/// `contains` 拦掉，省一次编译。删掉这一层不会有任何能力重新泄漏——
/// 白名单会独立、完整地挡住同样的东西。
const BANNED_SOURCE_SUBSTRINGS: [&str; 2] = ["require-builtin", "(require "];

/// 检查源码文本是否触碰了 [`BANNED_SOURCE_SUBSTRINGS`]。
///
/// 命中哪一个子串，就把它写进错误信息里,方便 mod 作者定位——不能只说
/// 「被拒绝」,不给理由。命中位置的字节偏移量一并带出（`source.find`
/// 恰好能给出），供调用方（Task 11 加载管理界面）换算成行号——这是
/// 文本层前置优化仍然值得携带位置信息的原因：它和 AST 白名单一样，
/// 拒绝时不该让 mod 作者自己去脚本里逐行找是哪一处触发的。
fn reject_dangerous_syntax(source: &str) -> Result<(), ScriptError> {
    for banned in BANNED_SOURCE_SUBSTRINGS {
        if let Some(byte_offset) = source.find(banned) {
            return Err(ScriptError::ParseError(
                format!(
                    "脚本源码包含禁止的语法「{banned}」——mod 脚本不允许 require 任何 Steel 内置模块，\
                     所有能力必须通过宿主注册的函数访问"
                ),
                Some(byte_offset as u32),
            ));
        }
    }
    Ok(())
}

/// 从 `engine.globals()` 拍下当前**全部**全局绑定名字的快照，减去
/// `poisoned` 里的名字，构成白名单的基础集合。
///
/// # 为什么不再手工维护一份"安全模块"清单
///
/// 早期实现手工列了 18 个"确认纯计算"的模块名，逐个枚举其导出——这个
/// 做法有个致命漏洞：`map`/`filter`/`foldl`/`foldr` 这些高阶函数根本
/// **不是**任何模块的导出，它们是 `steel-core` 标准库脚本
/// （`src/scheme/stdlib.scm`）里用 Scheme 自己写的 `define`，跟随
/// prelude 直接注册进全局作用域，不挂在任何 `BuiltInModule` 下——手工
/// 枚举模块永远看不到它们，实测（本文件对应的 host.rs 测试）在验收样本
/// 里用 `map` 直接被白名单拒绝，才发现这个洞。
///
/// `Engine::globals()` 是公开 API，返回编译器符号表里当前**全部**已知
/// 全局名字——不管这个名字是来自某个 `BuiltInModule`（包括
/// `#%prim.`-前缀的稳定引用，`struct` 宏展开依赖的 `#%prim.hash` 就在
/// 这里）、还是像 `map` 这样纯 Scheme 写的 prelude 函数，都会出现在这
/// 份快照里，一次性收全，不需要再猜"还漏了哪个模块"。
///
/// # 顺序要求：必须在 [`poison_module`]/[`poison_meta_deny_list`] **之前**拍快照
///
/// 毒化操作是把名字的**值**改成 `SteelVal::Void`，不是把名字从符号表
/// 里删掉——`engine.globals()` 在毒化之后调用，`command`/`eval!` 这些
/// 名字依然会出现在快照里（符号表不知道"这个值被换成毒药了"）。必须
/// 在毒化前拍快照、再用 `poisoned` 参数显式减去要挡的名字，两步顺序
/// 不能颠倒。
fn compute_allowed_identifiers(engine: &Engine, poisoned: &HashSet<&str>) -> HashSet<&'static str> {
    let mut names = HashSet::new();
    for interned in engine.globals().iter() {
        let name = interned.resolve();
        if poisoned.contains(name) {
            continue;
        }
        let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
        names.insert(leaked);
    }
    names
}

/// 枚举 [`FULLY_POISONED_MODULES`] 里每个模块的全部导出名字，加上
/// [`META_DENY_LIST`] 系列，构成 [`compute_allowed_identifiers`] 要
/// 排除的完整名单。
fn poisoned_identifiers(engine: &Engine) -> HashSet<&'static str> {
    let mut poisoned = HashSet::new();

    for module_name in FULLY_POISONED_MODULES {
        if let Some(module) = engine.builtin_modules().get(module_name) {
            for name in module.names() {
                poisoned.insert(Box::leak(name.into_boxed_str()) as &'static str);
            }
        }
    }
    for name in META_DENY_LIST {
        poisoned.insert(name);
    }
    for name in META_DENY_LIST_2 {
        poisoned.insert(name);
    }
    for name in META_DENY_LIST_3 {
        poisoned.insert(name);
    }

    poisoned
}

/// 脚本调用失败的分类。
///
/// 四道防线①②在此落地：[`ScriptEngine::call_raw`] 与
/// [`ScriptEngine::load_source`] 的签名本身就是 `Result`，出错必定拿到
/// `Err` 而不是 panic；具体要不要降级、降级成什么默认值，是**调用方**的
/// 决定，本类型只保证「出错一定可观测」。
/// 每个携带消息的变体都附带一个可选的**源码字节偏移量**（不是行号
/// 本身）——`ScriptError`/`ll-script` 本身不知道调用方是用什么路径把
/// 源码传进来的，无法自己把偏移量换算成行号（换算需要重新扫描一遍
/// 源码文本数换行符，调用方已经持有那份源码字符串，没理由让本类型
/// 再拷贝一份）。Task 11 加载管理界面据此在自己一侧换算出行号，见
/// `ll-mod` 的换算帮手。
///
/// 实测（`crates/ll-script/examples/probe_span.rs`）：`SteelErr::span()`
/// 对语法错误、`FreeIdentifier`、`ArityMismatch` 都能给出非空
/// `Span`，白名单拒绝时 AST 节点自身的 `SyntaxObject::span` 同样可用
/// （见 `crate::whitelist`）——因此加载管理界面能做到**行号级别**的错误
/// 定位，不是简报草稿担心的「只能显示到哪个文件」那种退化情形。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// 因超时（脚本失控）被 `InterruptHandler` 强制掐断。
    ///
    /// 没有携带偏移量：超时是「整份脚本跑太久」，不是某一行的问题，
    /// 不存在一个能归咎的具体位置。
    Interrupted,
    /// 调用 Rust 侧注册函数时缺参或多参。
    ArityMismatch(String, Option<u32>),
    /// 源码语法错误，编译阶段就失败，从未开始求值。
    ParseError(String, Option<u32>),
    /// 求值期间的其余运行时错误（未定义标识符、类型不匹配等）。
    Runtime(String, Option<u32>),
}

impl ScriptError {
    /// 取出携带的源码字节偏移量，`Interrupted` 恒为 `None`。
    pub fn byte_offset(&self) -> Option<u32> {
        match self {
            ScriptError::Interrupted => None,
            ScriptError::ArityMismatch(_, offset)
            | ScriptError::ParseError(_, offset)
            | ScriptError::Runtime(_, offset) => *offset,
        }
    }
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::Interrupted => write!(f, "脚本执行超时被中断"),
            ScriptError::ArityMismatch(msg, _) => write!(f, "参数个数不匹配：{msg}"),
            ScriptError::ParseError(msg, _) => write!(f, "脚本语法错误：{msg}"),
            ScriptError::Runtime(msg, _) => write!(f, "脚本运行时错误：{msg}"),
        }
    }
}

impl std::error::Error for ScriptError {}

/// 把 Steel 的 [`SteelErr`] 归类成 [`ScriptError`]。
///
/// 分类依据 `SteelErr::kind()`：`ArityMismatch` 与 `Parse` 各自独立成一类
/// 是因为调用方对它们的应对策略不同——语法错误说明整份 mod 源码就没编译
/// 通过（该拒绝加载这个 mod），arity 不匹配可能只是某一次调用传错了参数
/// （可以只丢弃这一次结果）。其余错误种类（`FreeIdentifier`、
/// `TypeMismatch`、`ContractViolation` 等）现阶段没有差异化处理需求，
/// 统一归入 `Runtime`，需要时再拆细。
fn classify_error(err: SteelErr) -> ScriptError {
    use steel::rerrs::ErrorKind;

    // 必须先取 span 再取 message：`err.to_string()` 不消费 err，取值
    // 顺序其实无关紧要，这里先取 span 只是让「位置信息从哪来」在阅读
    // 顺序上排在消息之前，与下面 match 里两者一起打包的顺序一致。
    let offset = err.span().map(|span| span.start());
    let message = err.to_string();
    match err.kind() {
        ErrorKind::ArityMismatch => ScriptError::ArityMismatch(message, offset),
        ErrorKind::Parse | ErrorKind::UnexpectedToken => ScriptError::ParseError(message, offset),
        _ => ScriptError::Runtime(message, offset),
    }
}

/// 脚本宿主：包装一个经过能力收窄的 Steel VM 实例。
///
/// 每次调用都套一层 [`InterruptHandler`]：ADR 0012 实测这层包装把单次
/// 调用从 74ns 拉到 326ns，仍是纳秒级，可以放在每帧每实体的热路径上，
/// 不需要为脚本调用另开线程。
///
/// # 能力边界的权威定义是白名单，不是黑名单
///
/// [`allowed_identifiers`](Self::allowed_identifiers) 才是"脚本能引用
/// 什么"的权威判据——[`FULLY_POISONED_MODULES`]/[`reject_dangerous_syntax`]
/// 是在此之上的额外防线（分别处理"名字已经绑定，poison 不到"与"省一次
/// 完整解析+展开"两种场景），三者共同生效，但唯独白名单具备"未来
/// steel-core 新增我们没见过的内置能力也自动排除在外"这个性质。
pub struct ScriptEngine {
    engine: Engine,
    interrupt: InterruptHandler,
    allowed_identifiers: HashSet<&'static str>,
}

impl ScriptEngine {
    /// 构造一个已经完成能力收窄的脚本引擎。
    ///
    /// 顺序不能变：`Engine::new_sandboxed()` 打底之后必须**立即**清空
    /// 危险模块与危险全局名字，任何脚本源码都不能在这之前被求值——
    /// 否则清空动作本身就晚了。白名单基础集合（`poisoned_identifiers`+`compute_allowed_identifiers`）
    /// 在同一时刻构建，之后每次 [`Self::register_fn`] 都会追加新名字。
    pub fn new() -> Self {
        let mut engine = Engine::new_sandboxed();

        // 顺序不能变：先拍下"毒化之前"的全局名字快照并减去要挡的名字
        // （见 compute_allowed_identifiers 文档），再真正执行毒化——
        // 毒化只改值不删符号表条目，顺序反了快照会把毒名字也收进去。
        let poisoned = poisoned_identifiers(&engine);
        let allowed_identifiers = compute_allowed_identifiers(&engine, &poisoned);

        for module_name in FULLY_POISONED_MODULES {
            poison_module(&mut engine, module_name);
        }
        poison_meta_deny_list(&mut engine);

        let interrupt = InterruptHandler::new(&mut engine, INTERRUPT_TIMEOUT);

        Self {
            engine,
            interrupt,
            allowed_identifiers,
        }
    }

    /// 注册一个 Rust 函数，供脚本以 `name` 调用，并把 `name` 加入白名单
    /// ——脚本引擎自己注册的函数天然是"我们想让脚本用的能力"，不需要
    /// 调用方再手工同步一份白名单。
    ///
    /// 转发到 Steel 的 `RegisterFn` trait；缺参/多参的 arity 检查是 Steel
    /// 原生行为（ADR 0001 已确认），这里不重新实现。
    pub fn register_fn<Args, Ret, F>(&mut self, name: &'static str, func: F) -> &mut Self
    where
        Engine: RegisterFn<F, Args, Ret>,
    {
        self.engine.register_fn(name, func);
        self.allowed_identifiers.insert(name);
        self
    }

    /// 加载并执行一段脚本源码（通常是 mod 的顶层定义）。
    ///
    /// 三道关卡依次生效：
    /// 1. [`reject_dangerous_syntax`]——源码文本快速失败，省一次解析。
    /// 2. **AST 白名单**（[`crate::whitelist::check_whitelist`]）——解析并
    ///    完整展开源码（`Engine::emit_fully_expanded_ast`，宏与
    ///    `require-builtin` 均已展开），确认树上出现的每一个被引用的
    ///    标识符都在 [`Self::allowed_identifiers`] 或脚本自己的局部作用域
    ///    里，这是权威防线。
    /// 3. 通过前两关才真正 `run`，套着中断防线执行——脚本本身死循环
    ///    也会在预算耗尽后返回 `Err`。
    ///
    /// 步骤 2 会对同一份源码重新解析一次（`run` 内部还会再解析一次）——
    /// 这是刻意的简化：mod 加载是一次性的加载期操作，不是每帧热路径，
    /// 多付一次解析的代价（ADR 0012 实测量级 ~百微秒）换来"校验的是
    /// 真正要执行的那份展开结果"这个正确性保证，用真实脚本验证过重复
    /// 解析/重复 `define` 不会产生副作用或报错。
    pub fn load_source(&mut self, source: String) -> Result<(), ScriptError> {
        reject_dangerous_syntax(&source)?;

        let exprs = self
            .engine
            .emit_fully_expanded_ast(&source, None)
            .map_err(classify_error)?;
        check_whitelist(&exprs, &self.allowed_identifiers)?;

        let engine = &mut self.engine;
        self.interrupt
            .run_with_timeout(|| engine.run(source))
            .map(|_| ())
            .map_err(classify_error)
    }

    /// 以 `name` 调用一个已经在脚本中定义好的函数。
    ///
    /// 每次调用都包中断防线（见结构体文档的开销说明），出错返回 `Err`，
    /// 绝不 panic。降级策略由调用方决定。
    pub fn call_raw(&mut self, name: &str, args: Vec<SteelVal>) -> Result<SteelVal, ScriptError> {
        let engine = &mut self.engine;
        self.interrupt
            .run_with_timeout(|| engine.call_function_by_name_with_args(name, args))
            .map_err(classify_error)
    }
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 死循环脚本返回错误而非崩溃进程() {
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source("(define (loop) (loop)) (loop)".to_string());

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 中断后同一引擎可以继续处理下一次调用() {
        // Arrange
        let mut engine = ScriptEngine::new();
        engine
            .load_source("(define (loop) (loop)) (loop)".to_string())
            .expect_err("死循环理应先被中断");

        // Act
        let result = engine.load_source("(define answer 42) answer".to_string());

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn 注册函数缺参返回参数个数不匹配错误() {
        // Arrange
        let mut engine = ScriptEngine::new();
        engine.register_fn("needs-two-args", |a: i64, b: i64| a + b);

        // Act
        let result = engine.load_source("(needs-two-args 1)".to_string());

        // Assert
        assert!(matches!(result, Err(ScriptError::ArityMismatch(_, _))));
    }

    #[test]
    fn 语法错误的源码返回解析错误而非崩溃进程() {
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act：故意少一个右括号。
        let result = engine.load_source("(+ 1 2".to_string());

        // Assert
        assert!(matches!(result, Err(ScriptError::ParseError(_, _))));
    }

    #[test]
    fn 语法错误携带的字节偏移量落在触发错误的那一行() {
        // 加载管理界面（Task 11）要靠这个偏移量换算行号，这里钉住
        // 「确实拿到了偏移量，且偏移量落在第二行」——不只是断言
        // Some(_)，避免将来偷懒把偏移量恒定填成 0 也能让测试通过。
        // Arrange：第一行是合法定义，第二行才是语法错误。
        let mut engine = ScriptEngine::new();
        let source = "(define x 1)\n(+ 1 2".to_string();

        // Act
        let result = engine.load_source(source.clone());

        // Assert
        match result {
            Err(ScriptError::ParseError(_, Some(offset))) => {
                let line = source[..offset as usize].matches('\n').count() + 1;
                assert_eq!(line, 2);
            }
            other => panic!("期望带偏移量的 ParseError，实际拿到 {other:?}"),
        }
    }

    #[test]
    fn 脚本无法在中断预算内跑完时结果是中断而非挂起() {
        // 与「死循环」用例的区别：这里断言的是「进程仍然响应」这个更强的
        // 事实——若中断机制失效，这个测试本身会挂起到测试框架超时,而不是
        // 断言失败,能更快暴露回归。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source(
            "(define (spin n) (if (= n 0) 0 (spin (- n 1)))) (define (loop) (loop)) (loop)"
                .to_string(),
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 脚本无法拉起系统进程() {
        // 直接验证 ADR 0012 的缓解措施在包装层里确实生效：即使脚本
        // 显式尝试 command/spawn-process,也拿到 Err 而不是真的执行。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine
            .load_source(r#"(command "cmd" (list "/c" "echo" "should-not-run"))"#.to_string());

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 脚本无法读取文件系统() {
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source(r#"(path-exists? "Cargo.toml")"#.to_string());

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 脚本无法读取系统墙钟() {
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source("(require-builtin steel/time) (instant/now)".to_string());

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 脚本无法取到非确定性随机数() {
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result =
            engine.load_source("(require-builtin steel/random) (rng->gen-usize)".to_string());

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 脚本无法通过eval动态构造代码逃逸() {
        // steel/meta 的 eval! 能在运行期把字符串当代码执行,若不poison,
        // 源码文本层面的黑名单（reject_dangerous_syntax）可以被绕过。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source(r#"(eval! "(+ 1 2)")"#.to_string());

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 脚本无法通过run构造全新未收窄的引擎() {
        // steel/meta 的 Engine::new / run! 会在脚本内部构造一个全新、完全
        // 不受本文件任何限制的 Steel 引擎——这条路必须堵死。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source(r#"(run! (Engine::new) "(+ 1 2)")"#.to_string());

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 含require字面量的脚本在编译前就被拒绝() {
        // 这是 steel/time 场景下唯一验证有效的防线：模块覆盖对它不生效
        // （见 ADR 0012），必须在源码文本层面挡住 require-builtin。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source("(require-builtin steel/time)".to_string());

        // Assert
        assert!(matches!(result, Err(ScriptError::ParseError(_, _))));
    }

    #[test]
    fn 脚本能定义并使用自己的结构体() {
        // 白名单的定位是能力边界不是语言子集：struct 是纯语言特性
        // （不触达文件/网络/进程/线程/墙钟/随机中的任何一个），必须放行。
        // Arrange
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (struct Point (x y))
                (define p (Point 3 4))
                (define (probe) (list (Point-x p) (Point-y p) (Point? p)))
                "#
                .to_string(),
            )
            .unwrap();

        // Act
        let result = engine.call_raw("probe", Vec::new());

        // Assert
        assert_eq!(
            result,
            Ok(steel::rvals::SteelVal::ListV(
                [
                    steel::rvals::SteelVal::IntV(3),
                    steel::rvals::SteelVal::IntV(4),
                    steel::rvals::SteelVal::BoolV(true),
                ]
                .into_iter()
                .collect()
            ))
        );
    }

    #[test]
    fn 脚本能定义并使用自己的宏() {
        // define-syntax/syntax-rules 是 Lisp 的核心能力，必须放行——
        // 安全性来自"校验的是宏展开之后的树"，不靠"不让脚本写宏"。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source(
            r#"
            (define-syntax my-unless
              (syntax-rules ()
                [(my-unless test body) (if test #f body)]))
            (define (probe) (my-unless #f 42))
            "#
            .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        assert_eq!(
            engine.call_raw("probe", Vec::new()),
            Ok(steel::rvals::SteelVal::IntV(42))
        );
    }

    #[test]
    fn 验收样本一份有分量的内容定义脚本能通过白名单并算出正确结果() {
        // 白名单的验收标准补另一半：不能只测"它挡住了什么"，还要测
        // "它放行了什么"。这份样本模拟 mod 作者真实会写的内容定义
        // 表——同时用到宏、自定义结构体、闭包/递归、高阶函数（map/
        // foldl），做一件真实的事：定义三件物品，用宏语法糖构造它们，
        // 用递归找出"价值密度"（base-value / weight）最高的一件，用
        // map/foldl 求全部密度与总价值。
        //
        // 这个测试要留在这里：将来任何人收紧白名单（不管是收紧
        // compute_allowed_identifiers 的实现，还是往 META_DENY_LIST
        // 加错名字），只要动到这份样本用到的任何一个特性，这个测试就
        // 会变红——这是防止白名单慢慢收窄成"什么都做不了的玩具"的
        // 机制,比任何文档都有效。
        //
        // Arrange
        let mut engine = ScriptEngine::new();
        let source = r#"
            ;; 自定义结构体：物品的名字、基础价值、重量。
            (struct Item (name base-value weight))

            ;; 宏：给"用 Item 构造函数"包一层更好读的语法糖——
            ;; define-syntax/syntax-rules 是 Lisp 的核心能力，白名单
            ;; 必须放行,不能因为"宏能生成任意代码"就整体拒绝
            ;; （安全性来自校验展开之后的树，见 whitelist.rs 模块文档）。
            (define-syntax defitem
              (syntax-rules ()
                [(defitem name base weight) (Item name base weight)]))

            ;; 派生规则：价值密度 = base-value / weight（整数除法）。
            (define (value-density item)
              (quotient (Item-base-value item) (Item-weight item)))

            (define items
              (list (defitem "sword" 100 10)
                    (defitem "gem" 500 2)
                    (defitem "shield" 80 20)))

            ;; 高阶函数：map 求每件物品的密度，foldl 求总价值。
            (define densities (map value-density items))
            (define total-value (foldl + 0 (map Item-base-value items)))

            ;; 递归（不用内置 max，确保真的走递归路径）：找出密度最高
            ;; 的物品。
            (define (best-by-density xs best)
              (if (null? xs)
                  best
                  (let ([candidate (car xs)])
                    (if (> (value-density candidate) (value-density best))
                        (best-by-density (cdr xs) candidate)
                        (best-by-density (cdr xs) best)))))

            (define best (best-by-density (cdr items) (car items)))

            (define (probe) (list total-value (Item-name best) densities))
            "#;

        // Act
        engine
            .load_source(source.to_string())
            .expect("这份纯语言特性组合的脚本必须能通过白名单编译");
        let result = engine.call_raw("probe", Vec::new());

        // Assert：sword 密度 10、gem 密度 250、shield 密度 4，总价值
        // 100+500+80=680，密度最高的是 gem。
        assert_eq!(
            result,
            Ok(steel::rvals::SteelVal::ListV(
                [
                    steel::rvals::SteelVal::IntV(680),
                    steel::rvals::SteelVal::StringV("gem".into()),
                    steel::rvals::SteelVal::ListV(
                        [
                            steel::rvals::SteelVal::IntV(10),
                            steel::rvals::SteelVal::IntV(250),
                            steel::rvals::SteelVal::IntV(4),
                        ]
                        .into_iter()
                        .collect()
                    ),
                ]
                .into_iter()
                .collect()
            ))
        );
    }

    #[test]
    fn 白名单挡住没有出现require字样的裸引用() {
        // 证明白名单不依赖 reject_dangerous_syntax 的文本黑名单：这段
        // 源码里根本没有 "require-builtin"/"(require " 字样，若真的存在
        // 某个像 command/spawn-native-thread 一样"引导期已绑定"的名字，
        // 黑名单对它无能为力，必须靠白名单的默认拒绝兜底。用一个真实
        // 世界里不存在、但语法上完全合法的裸引用模拟"未来 steel-core
        // 新增的、我们从没见过的内置能力"。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source("(some-future-unknown-builtin-capability)".to_string());

        // Assert
        assert!(matches!(result, Err(ScriptError::ParseError(_, _))));
    }

    #[test]
    fn 白名单挡住脚本自己拼出来的require_builtin符号() {
        // 实测（见 examples/probe_whitelist.rs 第 4 节）：脚本能用
        // string->symbol 拼出一个字面等于 "require-builtin" 的符号，但
        // 这只是数据构造，不会触发宏展开——因此这里断言的不是"拼接本身
        // 被挡住"，而是"拼接之后没有任何路径能把这个符号变成真正的
        // require-builtin 效果"：整段脚本本身用到的 string->symbol/
        // string-append/apply/list 都在白名单内，应当正常跑完，但不会
        // 产生任何 require-builtin 的副作用。
        // Arrange
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe)
                  (string->symbol (apply string-append (list "require" "-" "builtin"))))
                "#
                .to_string(),
            )
            .unwrap();

        // Act
        let result = engine.call_raw("probe", Vec::new());

        // Assert：拼出来的只是一个惰性符号值，调用成功但不产生任何
        // require-builtin 效果——因为脚本从未真正拥有触发它的能力
        // （eval!/run!/eval-string 等反射入口不在白名单内）。
        assert_eq!(
            result,
            Ok(steel::rvals::SteelVal::SymbolV("require-builtin".into()))
        );
    }

    #[test]
    fn 脚本无法访问网络() {
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source("(require-builtin steel/tcp)".to_string());

        // Assert
        assert!(result.is_err());
    }
}
