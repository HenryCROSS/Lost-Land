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
use steel::steel_vm::ThreadStateController;
use steel::steel_vm::builtin::BuiltInModule;
use steel::steel_vm::engine::Engine;
use steel::steel_vm::interrupt::InterruptHandler;
use steel::steel_vm::register_fn::RegisterFn;

use crate::alloc_guard;
use crate::whitelist::{check_whitelist, top_level_defined_names};

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
            for spelling in spellings_of(&name) {
                engine.register_value(&spelling, SteelVal::Void);
            }
        }
    }
    engine.register_module(BuiltInModule::new(module_name));
}

/// 全局环境里，同一个模块导出名字的**别名前缀**。
///
/// # 为什么每个危险名字都有第二个拼写
///
/// `steel-core` 0.8.2 的引导脚本 `ALL_MODULES`
/// （`src/steel_vm/primitives.rs`，用
/// `git show v0.8.2:crates/steel-core/src/steel_vm/primitives.rs` 读，
/// 工作树是 0.8.3+ 不作数）把同一批模块**导入两遍**：先无前缀
/// `(require-builtin steel/process)`，再带前缀
/// `(require-builtin steel/process as #%prim.)`。第二遍给每个导出名字
/// 在全局环境里额外造一个 `#%prim.<name>` 绑定——`command` 与
/// `#%prim.command` 是**两个不同的全局名字，指向同一个原生函数**。
///
/// 只覆盖无前缀那一份是本项目此前的真实漏洞（实测：`(command ...)`
/// 被白名单挡住，但
/// `(#%prim.spawn-process (#%prim.command "cmd" ...))` 一路放行、真的
/// 拉起了 `cmd.exe`；`#%prim.eval!` 更是能把任意源码字符串喂回 VM，
/// 等于整套白名单不存在）。
///
/// 带 `as #%prim.` 的模块清单里含 `steel/process`、`steel/threads`、
/// `steel/meta`、`steel/filesystem`——正好覆盖 [`FULLY_POISONED_MODULES`]
/// 与 [`META_DENY_LIST`] 系列关心的全部模块。`steel/time`/`steel/random`
/// 不在带前缀那一份里，多毒化一次也只是无害的空操作。
///
/// # 同步来源与防漂移
///
/// 这份前缀清单是全文件唯一手抄自上游的常量，同步来源就是上面点名的
/// `ALL_MODULES` 常量里 `as ` 后面出现过的全部前缀（0.8.2 实测只有
/// `#%prim.` 一个）。**它会不会过时由测试回答，不由人记性回答**：
/// `alias_prefix_list_covers_every_live_alias` 那条测试直接扫描活引擎
/// 的全局名字表，只要上游哪天多加一个别名前缀（或换掉现有的），被禁
/// 名字就会出现第三种拼写而没被毒化，测试当场变红。
const MODULE_ALIAS_PREFIXES: [&str; 1] = ["#%prim."];

/// 把一个基础名字展开成它在全局环境里的**全部拼写**：名字本身，加上
/// [`MODULE_ALIAS_PREFIXES`] 里每个前缀拼出来的别名。
///
/// 毒化与白名单排除都必须按这份完整拼写清单来做——只处理其中一种
/// 拼写等于没做，见 [`MODULE_ALIAS_PREFIXES`] 文档。
fn spellings_of(base: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(1 + MODULE_ALIAS_PREFIXES.len());
    out.push(base.to_string());
    for prefix in MODULE_ALIAS_PREFIXES {
        out.push(format!("{prefix}{base}"));
    }
    out
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
    for name in meta_deny_list() {
        for spelling in spellings_of(name) {
            engine.register_value(&spelling, SteelVal::Void);
        }
    }
}

/// [`META_DENY_LIST`] 三批常量拼成一条迭代器——三个常量只是因为 Rust
/// 数组字面量不便按理由分组书写才拆开的，语义上是同一份清单，凡是
/// 要遍历它的地方都该走这里，不该再手抄一遍三个循环。
fn meta_deny_list() -> impl Iterator<Item = &'static str> {
    META_DENY_LIST
        .into_iter()
        .chain(META_DENY_LIST_2)
        .chain(META_DENY_LIST_3)
}

/// mod 脚本一律不支持的加载形式，写给 mod 作者看的统一说法。
///
/// 三处共用同一句话（源码文本前置检查、`require-builtin` 前置检查、
/// [`DenyAllSourceModules`] 兜底后的错误翻译），是为了让 mod 作者不管
/// 从哪条路撞上来，看到的都是同一个结论，而不是三种听起来像不同问题
/// 的内部错误。
///
/// 文案刻意短（不到 20 字）——实测（P4 验收 demo 截图，见
/// `.superpowers/sdd/2026-08-18-p4-script-and-mod/task-11-12-report.md`）
/// 早期版本的完整解释性文案在加载管理界面的错误详情面板里会自动换行，
/// 压住下一行文字。面板本身已经加了截断兜底
/// （`ll_ui::load_report_view::truncate_for_panel`），但从源头把消息
/// 写短既不依赖那道兜底，也让面板不需要截断就能完整展示。
const REQUIRE_UNSUPPORTED_MESSAGE: &str = "mod 脚本不支持 require：能力由宿主注入";

/// 源码文本层面识别 `require` 家族的**词法**检查（不是子串匹配）。
///
/// # 为什么不能再用 `source.contains("(require ")`
///
/// 曾经的实现就是两条字面子串 `"require-builtin"` 与 `"(require "`。
/// 后者**实测可绕过**：`(require<换行>"C:/…/x.scm")`（左括号与 `require`
/// 之间是换行而不是空格）不含那条子串，直接穿过这一层；随后
/// `Engine::emit_fully_expanded_ast` 在展开阶段就会去读那个文件——
/// 白名单是在展开**之后**才检查的，读盘早已发生。实测用
/// `(require<换行>"C:/Windows/win.ini")` 触发，报错里出现了 `win.ini` 里
/// `[fonts]` 段落的名字，说明磁盘内容确实被读进来并解析了。制表符、
/// 两个空格、`( require ` 同理。
///
/// 现在的做法是扫描每一个 `(`，跳过其后的空白，取出紧接着的那个记号，
/// 命中 `require` 或任何 `require-*`（`require-builtin`、
/// `require-for-syntax`）就拒绝——空白怎么写都逃不掉。
///
/// # 这一层的定位仍然是「快速失败 + 给位置」，不是权威防线
///
/// 权威防线是 [`DenyAllSourceModules`]：宏展开产生的 `require`、或者
/// 任何本函数的词法近似没覆盖到的写法，都会在真正解析模块时被那道
/// 解析器无条件拒绝。这一层的价值是**在编译之前**就失败，并且能给出
/// 命中位置的字节偏移量（供 Task 11 加载管理界面换算行号）——权威防线
/// 那条路给不出这个位置。
fn reject_dangerous_syntax(source: &str) -> Result<(), ScriptError> {
    let bytes = source.as_bytes();
    for (open_paren, _) in source.match_indices(OPEN_PAREN_CHARS) {
        let mut cursor = open_paren + 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let token_start = cursor;
        while cursor < bytes.len() && !is_token_terminator(bytes[cursor]) {
            cursor += 1;
        }
        let token = &source[token_start..cursor];
        if token == "require" || token.starts_with("require-") {
            return Err(ScriptError::ParseError(
                format!("禁止的语法「{token}」：{REQUIRE_UNSUPPORTED_MESSAGE}"),
                Some(open_paren as u32),
            ));
        }
    }
    Ok(())
}

/// Steel 认的**三种**左括号。
///
/// 同步来源：`steel-parser` 0.8.2 `src/lexer.rs` 的
/// `Some(&paren @ ('(' | '[' | '{'))` 那一支——圆括号、方括号、花括号
/// 在 Steel 里完全等价。
///
/// 只扫圆括号是不够的，这不是理论担心：实测 `[require "C:/…/x.scm"]`
/// 穿过了只认 `(` 的那版扫描，最后是靠 [`DenyAllSourceModules`] 兜住
/// 的（错误信息里没有「禁止的语法」前缀，正是兜底那条路的特征）。
/// 兜住了不等于该放着不管——文本层多认两种括号，mod 作者才能拿到命中
/// 位置，而不是一条没有位置的编译错误。
const OPEN_PAREN_CHARS: [char; 3] = ['(', '[', '{'];

/// Steel 记号的结束字符：空白、三种括号的两侧、引号与注释起始——只
/// 用来在 [`reject_dangerous_syntax`] 里切出「左括号后面的第一个
/// 记号」，不追求与 Steel 词法器完全等价（权威判据在
/// [`DenyAllSourceModules`]，见其文档）。
fn is_token_terminator(byte: u8) -> bool {
    byte.is_ascii_whitespace()
        || matches!(
            byte,
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b'\'' | b';'
        )
}

/// `steel-core` 找不到源码模块时抛出的错误消息固定含有这个子串
/// （`compiler/modules.rs`：
/// `crate::throw!(Generic => "Unable to find builtin module: {:?}", module)`）。
///
/// [`DenyAllSourceModules`] 让**每一次** `require` 都走到这条错误上，
/// 因此命中这个子串就唯一对应「脚本试图 require 某个东西」，可以安全
/// 地翻译成 [`REQUIRE_UNSUPPORTED_MESSAGE`]。判断依据是消息文本、天生
/// 比 `ErrorKind` 脆弱（未来 `steel-core` 改了措辞会静默失效），但退化
/// 后果只是错误信息重新变得晦涩，**能力边界不受影响**——拒绝本身由
/// [`DenyAllSourceModules`] 保证，与这条翻译无关。
const UNRESOLVED_MODULE_MARKER: &str = "Unable to find builtin module:";

/// 把 `steel-core` 的「找不到模块」内部错误翻译成 mod 作者读得懂的说法。
///
/// 只在装载路径（[`ScriptEngine::load_source`]）上做这次翻译：`require`
/// 只可能在编译/展开阶段出现，运行期调用（[`ScriptEngine::call_raw`]）
/// 不存在这条路，没必要也不该在那里多一次字符串匹配。
fn translate_require_error(err: ScriptError) -> ScriptError {
    let ScriptError::Runtime(ref message, offset) = err else {
        return err;
    };
    if !message.contains(UNRESOLVED_MODULE_MARKER) {
        return err;
    }
    ScriptError::ParseError(REQUIRE_UNSUPPORTED_MESSAGE.to_string(), offset)
}

/// 「一律拒绝」的源码模块解析器——[`ScriptEngine`] 关掉 `require`
/// 文件系统逃逸的权威防线。
///
/// # 机制：`exists()` 恒真，把 FS 分支彻底挤掉
///
/// `steel-core` 0.8.2 解析 `(require "…")` 时（`compiler/modules.rs`
/// 的 `parse_require`）按固定顺序问：内置模块表 → `custom_builtins`
/// （`Engine::register_steel_module` 注册的那些）→ **本 trait 的
/// `exists()`** → 都不认就当成文件路径，交给 `parse_from_path` 直接
/// `std::fs::File::open`。最后那一步不看沙箱标志、不看搜索目录白名单，
/// 绝对路径、`..` 相对路径一律照读——这正是实测能用
/// `(require "C:/…/私密文件.scm")` 把盘上任意文件读进编译过程的原因。
///
/// 让 `exists()` **恒返回 `true`**，第三步就永远命中，控制权在到达文件
/// 系统之前就被截住；随后 `resolve()` 返回 `None`，`steel-core` 抛出
/// [`UNRESOLVED_MODULE_MARKER`] 那条错误，被 [`translate_require_error`]
/// 翻译成 [`REQUIRE_UNSUPPORTED_MESSAGE`]。
///
/// # 为什么不是 `register_steel_module`，也不是 `add_search_directory`
///
/// `Engine::register_steel_module` 只把某个名字放进 `custom_builtins`，
/// 让它**优先于**文件系统命中——没命中的名字照样掉进 FS 分支，关不死
/// 这个洞。`Engine::add_search_directory` 方向恰好相反：它只会**扩大**
/// FS 分支的搜索面，对收窄能力是负资产，本项目不使用。
///
/// # 本批次刻意只做「全部拒绝」这一档
///
/// `resolve()` 无条件返回 `None`，不做任何按名字放行的分档。mod 之间
/// 的模块系统（`provide`/`import` 语法与跨 mod 权限）设计还没定完，是
/// 独立的一批工作；在那之前放行任何一个名字都是在给一套还没设计完的
/// 语义开口子。真要开的时候，`exists()` 拿到的是**原样的 key 字符串**
/// （含绝对路径），路径校验与名字解析就写在这两个方法里，控制权是
/// 完整的。
struct DenyAllSourceModules;

impl steel::compiler::modules::SourceModuleResolver for DenyAllSourceModules {
    /// 恒 `None`：任何名字都不解析成源码，见类型文档「本批次刻意只做
    /// 『全部拒绝』这一档」。
    fn resolve(&self, _key: &str) -> Option<String> {
        None
    }

    /// 恒 `true`：这是整道防线的机制本身，见类型文档「机制」一节——
    /// 返回 `false` 会让 `steel-core` 继续走到文件系统分支，洞就还在。
    fn exists(&self, _key: &str) -> bool {
        true
    }
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
                for spelling in spellings_of(&name) {
                    poisoned.insert(Box::leak(spelling.into_boxed_str()) as &'static str);
                }
            }
        }
    }
    for name in meta_deny_list() {
        for spelling in spellings_of(name) {
            poisoned.insert(Box::leak(spelling.into_boxed_str()) as &'static str);
        }
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
    /// 因执行墙钟时长超过 [`INTERRUPT_TIMEOUT`]，被 `InterruptHandler`
    /// 的看门狗线程强制掐断——脚本失控（死循环）或单纯计算量太大。
    ///
    /// **曾经与内存预算共用同一个 `Interrupted` 变体**（见
    /// [`classify_error`] 文档「两条路曾经共用一个变体，为什么必须拆」
    /// 一节）——拆分之后，`Timeout` 专指墙钟超时这一条路。
    ///
    /// 没有携带任何诊断数据（既没有字节偏移，也没有耗时数字）：中断
    /// 发生在任意一条字节码上，不是某一行源码的错，不存在一个能归咎
    /// 的具体位置；至于"跑了多久"——`InterruptHandler` 的看门狗线程在
    /// `steel-core` 内部（本 crate 不能修改，见 [`classify_error`]
    /// 文档），它只知道"等了固定的 [`INTERRUPT_TIMEOUT`] 还没等到完成
    /// 信号"，不追踪单次调用的真实耗时——能报告的"耗时"永远是那个
    /// 固定常量本身，不是任何有信息量的每次调用数据，因此干脆不带。
    Timeout,
    /// 因单次调用的净分配字节数超过
    /// [`crate::alloc_guard::set_memory_budget`] 设定的预算，被本 crate
    /// 自己的内存守卫（[`crate::alloc_guard`]）强制掐断。
    ///
    /// 携带触发那一刻的诊断数据——与 [`Timeout`](Self::Timeout) 不同，
    /// 这份数据本来就是内存记账过程的副产品（[`crate::alloc_guard`]
    /// 本来就在每次分配时精确维护累计字节数），不是额外花代价采集的，
    /// 没有理由不带。
    MemoryBudgetExceeded {
        /// 触发中断那一刻，当前线程累计的净分配字节数（已经越界之后
        /// 的值，不是预算本身）。
        allocated_bytes: usize,
        /// 触发中断那一刻生效的预算，字节。
        budget_bytes: usize,
    },
    /// 调用 Rust 侧注册函数时缺参或多参。
    ArityMismatch(String, Option<u32>),
    /// 源码语法错误，编译阶段就失败，从未开始求值。
    ParseError(String, Option<u32>),
    /// 求值期间的其余运行时错误（未定义标识符、类型不匹配等）。
    Runtime(String, Option<u32>),
}

impl ScriptError {
    /// 取出携带的源码字节偏移量，两个中断变体
    /// （[`Timeout`](Self::Timeout)/[`MemoryBudgetExceeded`](Self::MemoryBudgetExceeded)）
    /// 恒为 `None`——理由分别见各自的变体文档。
    pub fn byte_offset(&self) -> Option<u32> {
        match self {
            ScriptError::Timeout => None,
            ScriptError::MemoryBudgetExceeded { .. } => None,
            ScriptError::ArityMismatch(_, offset)
            | ScriptError::ParseError(_, offset)
            | ScriptError::Runtime(_, offset) => *offset,
        }
    }
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::Timeout => write!(f, "脚本执行超时被中断"),
            ScriptError::MemoryBudgetExceeded {
                allocated_bytes,
                budget_bytes,
            } => write!(
                f,
                "脚本内存预算超限被中断：已分配 {allocated_bytes} 字节，预算 {budget_bytes} 字节"
            ),
            ScriptError::ArityMismatch(msg, _) => write!(f, "参数个数不匹配：{msg}"),
            ScriptError::ParseError(msg, _) => write!(f, "脚本语法错误：{msg}"),
            ScriptError::Runtime(msg, _) => write!(f, "脚本运行时错误：{msg}"),
        }
    }
}

impl std::error::Error for ScriptError {}

/// `InterruptHandler` 掐断超时脚本时，`steel-core` 内部构造的错误消息
/// 固定含有这个子串（`steel_vm/vm.rs`：
/// `format!("Thread: {:?} - Interrupted by user", ...)`）——这是**唯一**
/// 可靠区分「这次 `Err` 是不是某种中断（超时或超预算）」的信号，见
/// [`classify_error`] 文档「为什么按消息文本而不是 `ErrorKind` 判断」
/// 一节。命中这个标记只回答"是不是中断"，回答不了"是哪一种"——那是
/// [`classify_error`] 文档下一节的问题。
const INTERRUPTED_MESSAGE_MARKER: &str = "Interrupted by user";

/// 把 Steel 的 [`SteelErr`] 归类成 [`ScriptError`]。
///
/// 分类依据 `SteelErr::kind()`：`ArityMismatch` 与 `Parse` 各自独立成一类
/// 是因为调用方对它们的应对策略不同——语法错误说明整份 mod 源码就没编译
/// 通过（该拒绝加载这个 mod），arity 不匹配可能只是某一次调用传错了参数
/// （可以只丢弃这一次结果）。其余错误种类（`FreeIdentifier`、
/// `TypeMismatch`、`ContractViolation` 等）现阶段没有差异化处理需求，
/// 统一归入 `Runtime`，需要时再拆细。
///
/// # 为什么按消息文本而不是 `ErrorKind` 判断中断
///
/// 曾经的实现留了一个 `ScriptError::Interrupted` 变体但从未在这里
/// 构造过它——`InterruptHandler` 掐断死循环时，`steel-core`
/// （`steel_vm/vm.rs:1796`）用 `stop!(Generic => ...)` 抛出错误，
/// `ErrorKind::Generic` 是整个 `steel-core` 里最常见的兜底错误种类
/// （核实：`grep -rc "stop!(Generic" steel-core-0.8.2/src` 命中三十多个
/// 文件，从数字类型错误到端口 I/O 错误全都用它），落进上面 `match` 的
/// `_` 分支得到 `Runtime`，从未真正产出过 `Interrupted`——这是
/// `Interrupted` 曾经是一个死变体的根因：文档说它是超时变体，实现却
/// 从未构造它，真实超时永远走 `Runtime`（消息含 `"Interrupted by
/// user"` 且带一个语义上没有意义的字节偏移）。`ErrorKind` 给不出任何
/// 区分度，能用来识别中断的唯一信号是这条错误消息本身固定包含的
/// [`INTERRUPTED_MESSAGE_MARKER`] 子串——判断依据虽然是消息文本匹配、
/// 天生比 `ErrorKind` 判断脆弱（未来 `steel-core` 版本若改了这句话的
/// 英文措辞，识别会静默失效、退化成把中断错误落进 `Runtime` 这个笼统
/// 分类），但这是当前版本唯一可用的信号，且退化后果不是错误分类或
/// 安全问题，可接受。
///
/// # `interrupt()` 通道的两个调用点，与两者共用一个变体曾经造成的真实
/// # 误诊（本节是拆分 `Timeout`/`MemoryBudgetExceeded` 的直接起因）
///
/// `ThreadStateController::interrupt()`（触发上面这条消息的根源）在
/// 当前代码库里穷尽核实过只有两个真实可达的调用点：
///
/// 1. `InterruptHandler` 的看门狗线程（`steel-core`
///    `steel_vm/interrupt.rs`，本 crate 不能修改）——`run_with_timeout`
///    的调用没能在 [`INTERRUPT_TIMEOUT`] 内送回完成信号时触发，运行在
///    **另一根**看门狗线程上。
/// 2. `alloc_guard::record_alloc`（模块私有，无法作为 rustdoc 链接
///    目标）——单次调用的净分配字节数超出
///    [`crate::alloc_guard::set_memory_budget`] 设定的预算时触发，运行
///    在脚本自己执行的那根线程上（分配本来就发生在那根线程）。
///
/// （`steel/threads` 模块的 `thread-interrupt` 是第三个技术上存在的
/// 调用点，但该模块在 [`FULLY_POISONED_MODULES`] 里被整体清空，脚本
/// 没有能力触达，不构成真实来源。）
///
/// 两者最终都只是把同一个原子状态置成 `Interrupted`，VM 在下一次安全
/// 点检查时观察到就抛出同一条含 [`INTERRUPTED_MESSAGE_MARKER`] 的
/// 消息——**从这条消息本身完全无法反推是哪一个调用点触发的**。这不是
/// 假设性的缺口：本 crate 的
/// `crates/ll-script/tests/memory_budget_enforced.rs` 曾经因为这个
/// 耦合真实误诊过两次——"预算不足"那条用例一直因为撞上 300ms 超时（而
/// 不是真的验证了内存记账）而"碰巧"通过；"预算充足"那条则曾经因为
/// 递归深度选得太大、在高并行负载下真的被超时打断而失败，两次事故都
/// 与"内存预算是否生效"这件事本身无关，纯粹是分类信息不够精细掩盖了
/// 真相。
///
/// 解法是显式记号，不是启发式：[`crate::alloc_guard`] 在调用点 2
/// **触发 `interrupt()` 之前**，在当前线程的线程局部变量里显式立一个
/// 记号（[`crate::alloc_guard::MemoryInterruptInfo`]，见其
/// `INTERRUPT_REASON` 文档）。调用点 1（`steel-core` 内部代码）不知道
/// 也不会去设这个记号——因此下面 `take_interrupt_reason()` 读到
/// `Some(_)` 就唯一对应调用点 2，读到 `None` 就唯一对应调用点 1（穷尽
/// 覆盖了上面核实过的两个真实来源）。这不是「跑了大约 300ms 就当作
/// 超时」那种按耗时猜测的启发式——记号的设置严格发生在决定"要不要
/// 中断"的同一次函数调用里，读取严格发生在"确认这次错误确实是中断"
/// 之后，两者之间没有时间窗口或调度延迟能影响这个记号本身的值,高
/// 负载只会让两条路各自变慢，不会让记号出现在错误的地方。
fn classify_error(err: SteelErr) -> ScriptError {
    use steel::rerrs::ErrorKind;

    // 必须先取 span 再取 message：`err.to_string()` 不消费 err，取值
    // 顺序其实无关紧要，这里先取 span 只是让「位置信息从哪来」在阅读
    // 顺序上排在消息之前，与下面 match 里两者一起打包的顺序一致。
    let offset = err.span().map(|span| span.start());
    let message = err.to_string();
    if message.contains(INTERRUPTED_MESSAGE_MARKER) {
        return match alloc_guard::take_interrupt_reason() {
            Some(reason) => ScriptError::MemoryBudgetExceeded {
                allocated_bytes: reason.allocated_bytes,
                budget_bytes: reason.budget_bytes,
            },
            None => ScriptError::Timeout,
        };
    }
    match err.kind() {
        ErrorKind::ArityMismatch => ScriptError::ArityMismatch(message, offset),
        ErrorKind::Parse | ErrorKind::UnexpectedToken => ScriptError::ParseError(message, offset),
        _ => ScriptError::Runtime(message, offset),
    }
}

// 本线程是否已经编译过脚本源码（[`ScriptEngine::load_source`] 成功
// 与否都算——编译动作本身已经发生了）。
//
// # 这是「构造阶段/编译阶段」这条项目级约束的机器强制
//
// [ADR 0028](../../../knowledge/decisions/0028-steel-engine-construction-memory-corruption.md)
// 实测定位：`steel-core` 0.8.2 的内存破坏**只在「先编译、后构造」这个
// 相邻关系上出现**——只构造（16000 次）不崩、只编译（4800 次）不崩、
// 两者交替才崩。约束因此是：
//
// > **同一根线程上，全部引擎构造必须发生在全部脚本编译之前。**
//
// 违反会 panic，不是 `debug_assert!`：这条规则的失效方式极其隐蔽——
// 将来任何人新增一处引擎构造点，代码照常编译、测试照常绿，偶发的内存
// 破坏会悄悄回来，而且崩在与新增点毫无关系的地方（见 ADR 0028「哪一行
// 炸取决于坏数据先被谁碰到」）。一条点名的 panic 远好于三分之一概率的
// 野指针。
//
// # 为什么是「每线程」而不是「每进程」
//
// `steel-core` 在**不开** `sync` 特性时（本仓库的构建，见 ADR 0028）
// 全部引擎级状态是 `thread_local!` 的，`Engine` 内部是 `Rc`——
// [`ScriptEngine`] 因此**不是 `Send`**，根本不可能在 A 线程构造、拿到
// B 线程使用。「进程级构造阶段」在类型系统层面就不成立；每线程各自
// 「先构造完再编译」才是这条约束唯一可实现、也是与实测机制对齐的
// 形态（ADR 0028 实测：提高并行度反而压低故障率，与「跨线程相邻也
// 危险」的方向相反）。
//
// 新线程天生是干净的：`thread_local!` 初值为 `false`，等于每根线程
// 各自重新开一次构造阶段。这也是「真的需要在编译之后再造引擎」时唯一
// 被认可的做法——换一根线程，而不是想办法把这个标记清掉。
thread_local! {
    static COMPILED_ON_THIS_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 本线程是否已经进入编译阶段（即已经调用过一次
/// [`ScriptEngine::load_source`]）。
///
/// 供调用方在自己那一层做同样的编排检查——例如装载管线要先把全部引擎
/// 造齐再逐个编译，可以用它断言「我确实还在构造阶段」。
pub fn has_compiled_on_this_thread() -> bool {
    COMPILED_ON_THIS_THREAD.with(std::cell::Cell::get)
}

fn mark_compiled_on_this_thread() {
    COMPILED_ON_THIS_THREAD.with(|flag| flag.set(true));
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
///
/// # 第四道防线：内存执行预算
///
/// `alloc_controller` 是本引擎的 [`ThreadStateController`]——与
/// `interrupt` 内部持有的是同一份底层状态的另一份 `Clone`（两者都来自
/// `engine.get_thread_state_controller()`，见 [`Self::new`]），单独存一份
/// 的原因是 [`InterruptHandler`] 不对外暴露它持有的那一份。[`Self::call_raw`]/
/// [`Self::load_source`] 在真正求值前把这份 `Clone` 交给
/// [`crate::alloc_guard::set_active_controller`]，超预算时
/// [`crate::alloc_guard`] 会调用它的 `interrupt()`——脚本侧看到的效果与
/// 死循环被 `InterruptHandler` 掐断完全一样。装没装 `#[global_allocator]`
/// 决定这道防线是否真的在拦截：见 `alloc_guard` 模块文档，本 crate 自己
/// 的测试二进制没有安装它，真正验证过「装上之后」的测试在
/// `crates/ll-script/tests/memory_budget_enforced.rs`。
pub struct ScriptEngine {
    engine: Engine,
    interrupt: InterruptHandler,
    allowed_identifiers: HashSet<&'static str>,
    alloc_controller: ThreadStateController,
    /// 本引擎上、此前已经装载成功的脚本在顶层 `define` 出来的名字。
    ///
    /// 存在的理由是「作用域单位是 mod，不是脚本文件」这条语义（见
    /// `ll_mod::pipeline::load_all` 文档）：同一个 mod 的多份脚本共用
    /// 同一个引擎，前一份的辅助函数在 Steel 的全局环境里对后一份真实
    /// 可见，白名单必须承认这批名字，否则会把它们误判成自由引用。
    /// 跨 mod 的隔离**不靠这个集合**，靠「换一个引擎」——新引擎的这个
    /// 集合天然是空的。
    script_defined: HashSet<String>,
}

impl ScriptEngine {
    /// 构造一个已经完成能力收窄的脚本引擎。
    ///
    /// 顺序不能变：`Engine::new_sandboxed()` 打底之后必须**立即**清空
    /// 危险模块与危险全局名字，任何脚本源码都不能在这之前被求值——
    /// 否则清空动作本身就晚了。白名单基础集合（`poisoned_identifiers`+`compute_allowed_identifiers`）
    /// 在同一时刻构建，之后每次 [`Self::register_fn`] 都会追加新名字。
    pub fn new() -> Self {
        assert!(
            !has_compiled_on_this_thread(),
            "本线程已经编译过脚本，不得再构造引擎——见              ll_script::host::COMPILED_ON_THIS_THREAD 文档与 ADR 0028：             「先编译、后构造」这个相邻关系是 steel-core 0.8.2 偶发内存             破坏的唯一已知触发条件。请把这次构造提前到本线程的构造阶段，             或者换一根新线程。"
        );
        let mut engine = Engine::new_sandboxed();

        // 关掉 `require` 的文件系统逃逸，见 DenyAllSourceModules 文档。
        // 必须在任何脚本源码被编译之前注册——解析器只对注册之后的编译
        // 生效，晚一步就等于这一份脚本没有这道防线。
        engine.register_source_module_resolver(DenyAllSourceModules);

        // 顺序不能变：先拍下"毒化之前"的全局名字快照并减去要挡的名字
        // （见 compute_allowed_identifiers 文档），再真正执行毒化——
        // 毒化只改值不删符号表条目，顺序反了快照会把毒名字也收进去。
        let poisoned = poisoned_identifiers(&engine);
        let allowed_identifiers = compute_allowed_identifiers(&engine, &poisoned);

        for module_name in FULLY_POISONED_MODULES {
            poison_module(&mut engine, module_name);
        }
        poison_meta_deny_list(&mut engine);

        // 必须在构造 InterruptHandler 之前或之后都行——两者各自拿到的是
        // 同一份底层状态的独立 Clone（见 ScriptEngine 结构体文档），谁先
        // 谁后不影响观测到的行为。放在这里是为了让 alloc_controller 的
        // 初始化紧挨着字段列表，阅读顺序上更直接。
        let alloc_controller = engine.get_thread_state_controller();
        let interrupt = InterruptHandler::new(&mut engine, INTERRUPT_TIMEOUT);

        Self {
            engine,
            interrupt,
            allowed_identifiers,
            alloc_controller,
            script_defined: HashSet::new(),
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
    /// 五道关卡依次生效：
    /// 1. [`reject_dangerous_syntax`]——源码词法层快速失败（`require`
    ///    家族），省一次解析，并给出命中位置。
    /// 2. [`DenyAllSourceModules`]——`require` 的权威防线，装在引擎
    ///    构造期，文本层的词法近似漏掉的写法（实测 `[require "…"]`
    ///    这种方括号写法曾经漏过）也逃不掉；错误经
    ///    [`translate_require_error`] 翻译成 mod 作者读得懂的说法。
    /// 3. **AST 白名单**（[`crate::whitelist::check_whitelist`]）——解析并
    ///    完整展开源码（`Engine::emit_fully_expanded_ast`，宏与
    ///    `require-builtin` 均已展开），确认树上出现的每一个被引用的
    ///    标识符都在 [`Self::allowed_identifiers`] 或脚本自己的局部作用域
    ///    里，这是权威防线。
    /// 4. **内存执行预算**（[`crate::alloc_guard`]）——真正求值前重置计数
    ///    器、重置「超预算中断」诊断记号、把本引擎的中断通道登记为「当前
    ///    线程活跃通道」，求值结束后立刻解除登记，见
    ///    [`Self::alloc_controller`](Self) 字段文档与
    ///    [`classify_error`] 文档「`interrupt()` 通道的两个调用点」。
    /// 5. 通过前面几关才真正 `run`，套着中断防线执行——脚本本身死循环
    ///    也会在预算耗尽后返回 `Err`。
    ///
    /// 步骤 3 会对同一份源码重新解析一次（`run` 内部还会再解析一次）——
    /// 这是刻意的简化：mod 加载是一次性的加载期操作，不是每帧热路径，
    /// 多付一次解析的代价（ADR 0012 实测量级 ~百微秒）换来"校验的是
    /// 真正要执行的那份展开结果"这个正确性保证，用真实脚本验证过重复
    /// 解析/重复 `define` 不会产生副作用或报错。
    pub fn load_source(&mut self, source: String) -> Result<(), ScriptError> {
        reject_dangerous_syntax(&source)?;

        mark_compiled_on_this_thread();

        let exprs = self
            .engine
            .emit_fully_expanded_ast(&source, None)
            .map_err(classify_error)
            .map_err(translate_require_error)?;
        check_whitelist(&exprs, &self.allowed_identifiers, &self.script_defined)?;
        // 顶层定义的名字要在**这一次装载成功之后**才对下一份脚本可见
        // ——先算好（`exprs` 马上就要被丢弃），装载失败时不并入。
        let newly_defined = top_level_defined_names(&exprs);

        // 步骤 4：登记内存守卫。窗口只覆盖下面这一次 `run`——前面两步
        // 的解析/展开分配不该算进脚本自己的预算（见 alloc_guard 模块
        // 文档「MEMORY_BUDGET 默认排除引擎构造本身」同一条精神）。
        // `reset_interrupt_reason` 必须与 `reset_alloc_counter` 一起在
        // 每次调用前清空：理由相同——上一次调用若曾经因超预算触发过
        // 中断，遗留的记号不清空会污染这一次的 `classify_error` 判断
        // （见 `alloc_guard::reset_interrupt_reason` 文档）。
        alloc_guard::reset_alloc_counter();
        alloc_guard::reset_interrupt_reason();
        alloc_guard::set_active_controller(self.alloc_controller.clone());

        let engine = &mut self.engine;
        let result = self
            .interrupt
            .run_with_timeout(|| engine.run(source))
            .map(|_| ())
            .map_err(classify_error);

        alloc_guard::clear_active_controller();
        if result.is_ok() {
            self.script_defined.extend(newly_defined);
        }
        result
    }

    /// 这个引擎上已经装载的脚本有没有在顶层 `define` 过 `name`。
    ///
    /// # 为什么需要它
    ///
    /// 宿主按**名字**调用脚本函数的地方（行为树入口、事件处理函数）
    /// 全都面临同一个问题：名字拼错的表现是 [`Self::call_raw`] 在
    /// **运行期**返回 `Err`，而那时候能做的只有静默降级——mod 作者
    /// 只会看到"我的函数没被调用"，没有任何线索。有了这个查询，宿主
    /// 可以在**接线建立的那一刻**就把「这个名字查不到」变成一条点名
    /// 的、可行动的错误（ADR 0017「注册期完整校验」）。
    ///
    /// 首个消费者是 `ll_mod::script_event_source::ScriptEventSource::new`
    /// ——它拿订阅表里记下的处理函数名逐条核对。
    ///
    /// 查的是 `script_defined`（脚本自己 `define` 出来的名字），不是
    /// 宿主注册的 Rust 函数：宿主函数的存在与否由 Rust 代码本身保证，
    /// 不需要运行期查询。
    pub fn has_definition(&self, name: &str) -> bool {
        self.script_defined.contains(name)
    }

    /// 以 `name` 调用一个已经在脚本中定义好的函数。
    ///
    /// 每次调用都包中断防线（见结构体文档的开销说明）与内存执行预算窗口
    /// （见 [`Self::load_source`] 步骤 4 的同一套接线），出错返回 `Err`，
    /// 绝不 panic。降级策略由调用方决定。
    pub fn call_raw(&mut self, name: &str, args: Vec<SteelVal>) -> Result<SteelVal, ScriptError> {
        alloc_guard::reset_alloc_counter();
        alloc_guard::reset_interrupt_reason();
        alloc_guard::set_active_controller(self.alloc_controller.clone());

        let engine = &mut self.engine;
        let result = self
            .interrupt
            .run_with_timeout(|| engine.call_function_by_name_with_args(name, args))
            .map_err(classify_error);

        alloc_guard::clear_active_controller();
        result
    }
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 存档/读档边界的强制重建计数——每调用一次
/// [`rebuild_all_engines_after_load`] 递增一次，供测试与调用方断言
/// 「重建确实发生」，而不是只能断言"行为看起来正常"这种弱验证（设计
/// 文档九、1 节 TDD 要求）。用 `AtomicU64` 而非 `Cell`：本类型的计数
/// 是跨调用观测的全局状态，不依赖某个 `ScriptEngine` 实例的生命周期，
/// `AtomicU64` 是这个场景标准的、无需额外同步原语包装的选择。
static REBUILD_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 存档/读档边界：强制重建全部脚本引擎（设计文档九、1 节）。
///
/// # 为什么是强制，不是「先检测再决定」
///
/// 若重建是可选的，需要一种「检测」机制判断某个 VM 实例是不是干净的
/// ——而 `tools/ll-datacheck` 这类静态检查工具本身还不存在（设计文档
/// 一、2 节已核实）。强制重建绕开了这条本来就没有可靠检测手段的路：
/// 不管 VM 里有没有脏状态，统一清空重来，天然安全，不依赖任何检测的
/// 可靠性——约束 C1 的修订表述正是把这条策略钉成了断言：「VM 必须可
/// 随时从零重建，且重建不需要任何迁移步骤」（规格 §4 C1，见其修订
/// 说明）。
///
/// # 参数与返回值
///
/// `sources` 是「（mod 命名空间，该 mod 已经装载成功的脚本源码）」的
/// 列表——调用方（`ll-mod` 装载管线，或未来存档读取流程里持有已装载
/// mod 清单的一方）负责提供，本函数不知道、也不需要知道这些源码原本
/// 来自哪个文件。对每一对丢弃旧引擎、从零 `ScriptEngine::new()`、重新
/// `load_source` 一遍，返回同样数量的 `(命名空间, 结果)`——各类 API
/// 表面（`api::query`/`api::state` 等）的 `register` 调用仍需调用方
/// 自行完成，本函数只负责「丢弃旧实例、从零构造、重新跑一遍脚本源码」
/// 这一步本身，不知道每个 mod 具体需要挂载哪些 API 表面，那是装载
/// 管线的职责（与 `ll_mod::pipeline::compile_one_script` 同一层分工）。
///
/// # 存档（写盘）不需要调用本函数
///
/// 见 [`Effect`](ll_sim::effect::Effect) 模块与本设计的既有论证：VM
/// 本身不该持有任何值得保留的状态（脚本状态经 `state-set!` 系列显式
/// 写入 `WorldState`，不是 VM 内部的 `define`/`set!`），存盘只是把
/// `WorldState` 序列化，不触碰 VM，因此只有**读档**（世界状态被替换
/// 成另一个时间点的快照）才需要重建 VM。
pub fn rebuild_all_engines_after_load(
    sources: &[(String, String)],
) -> Vec<(String, Result<ScriptEngine, ScriptError>)> {
    REBUILD_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    // 先把全部引擎造齐，再逐个编译——顺序不能改成「造一个编一个」：
    // 那正是 ADR 0028 定位到的「先编译、后构造」相邻关系，也是本线程
    // 构造阶段/编译阶段这条约束（见 `COMPILED_ON_THIS_THREAD` 上方
    // 注释）在本函数内部的具体落法。第二个 `ScriptEngine::new()` 会
    // 直接 panic，不是「概率性地出问题」。
    let mut engines: Vec<ScriptEngine> = sources.iter().map(|_| ScriptEngine::new()).collect();
    sources
        .iter()
        .zip(engines.drain(..))
        .map(|((namespace, source), mut engine)| {
            let result = engine.load_source(source.clone());
            (namespace.clone(), result.map(|()| engine))
        })
        .collect()
}

/// 当前进程内 [`rebuild_all_engines_after_load`] 被调用的次数——供测试
/// 断言"读档完成后确实触发了一次重建"，见其文档。
pub fn rebuild_count() -> u64 {
    REBUILD_COUNT.load(std::sync::atomic::Ordering::SeqCst)
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
    fn 死循环脚本被中断时归类为timeout而不是runtime() {
        // 曾经的死变体：`ScriptError::Interrupted` 从未被 `classify_error`
        // 构造过，真实超时一律落进 `Runtime`（见 `classify_error` 文档
        // 「为什么按消息文本而不是 ErrorKind 判断中断」一节）。这条测试
        // 钉住修复后的行为——不只是断言 `is_err()`（那条既有测试
        // 「死循环脚本返回错误而非崩溃」测的是更粗的粒度），而是断言
        // 具体拿到的是哪一个变体。本测试二进制没装
        // `#[global_allocator]`（见 `alloc_guard` 模块文档），
        // `alloc_guard` 永远不会因为超预算触发中断，这里的死循环唯一
        // 可能的中断来源就是 300ms 看门狗超时，因此 `Timeout` 是这条
        // 测试环境下唯一能观察到的分类。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source("(define (loop) (loop)) (loop)".to_string());

        // Assert
        assert_eq!(result, Err(ScriptError::Timeout));
    }

    #[test]
    fn timeout变体不携带字节偏移量() {
        // 中断发生在任意一条字节码上，不是某一行源码的错——即使
        // `steel-core` 底层的 `SteelErr` 本身带了一个 span（`classify_error`
        // 因为命中消息标记而提前 return，从未把这个 span 装进
        // `ScriptError::Timeout`），`byte_offset()` 恒为 `None`，见
        // 该变体文档。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source("(define (loop) (loop)) (loop)".to_string());

        // Assert
        match result {
            Err(err @ ScriptError::Timeout) => assert_eq!(err.byte_offset(), None),
            other => panic!("期望 Timeout，实际 {other:?}"),
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

    /// 造一个真实存在于磁盘上的 Steel 源码文件，返回它的绝对路径。
    ///
    /// `require` 的文件系统逃逸只有对着**真实存在**的文件才有说服力
    /// ——对不存在的路径报「找不到文件」，证明不了任何防线在起作用。
    fn 写一个真实的磁盘模块(标记: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "ll_script_require_probe_{标记}_{}.scm",
            std::process::id()
        ));
        std::fs::write(&path, "(provide 磁盘上的答案)\n(define 磁盘上的答案 42)\n")
            .expect("测试自己的临时文件应当写得进临时目录");
        path
    }

    /// 把一个路径写成能安全嵌进 Steel 字符串字面量的形式。
    ///
    /// Windows 的临时目录是 `C:\Users\…`，反斜杠在 Steel 字符串里是
    /// 转义符（实测报 `Syntax Error: invalid escape 'U'`），会让测试
    /// 在还没走到被测防线之前就先死在语法上。Steel 两种分隔符都认，
    /// 统一换成正斜杠。
    fn 写成steel字符串(path: &std::path::Path) -> String {
        path.display().to_string().replace('\\', "/")
    }

    /// 一个**没有经过任何毒化**的沙箱引擎，专供防漂移测试枚举「上游
    /// 当前真实提供了什么」。
    ///
    /// 不能拿 [`ScriptEngine`] 内部那个引擎来枚举：[`poison_module`]
    /// 的第二步会用同名空模块覆盖模块注册表，之后
    /// `builtin_modules().get("steel/process")` 拿到的是那个**空**替身
    /// （实测导出集合为 `{}`）。用它做判据，等于拿「我们已经清空过了」
    /// 当成「上游本来就没有」，三条防漂移测试会全部退化成恒真。
    fn 未毒化的参照引擎() -> Engine {
        Engine::new_sandboxed()
    }

    #[test]
    fn 脚本无法用prim前缀别名拉起系统进程() {
        // 这是真实发生过的逃逸：`(command ...)` 被白名单挡住，但
        // `ALL_MODULES` 还额外做过一遍 `(require-builtin steel/process
        // as #%prim.)`，于是同一个原生函数在全局环境里有第二个拼写
        // `#%prim.command`/`#%prim.spawn-process`——修复前这条路一路
        // 放行、真的拉起了 cmd.exe（见 MODULE_ALIAS_PREFIXES 文档）。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source(
            r#"(#%prim.spawn-process (#%prim.command "cmd" (list "/c" "echo" "should-not-run")))"#
                .to_string(),
        );

        // Assert
        assert!(
            matches!(result, Err(ScriptError::ParseError(_, _))),
            "实际拿到 {result:?}"
        );
    }

    #[test]
    fn 脚本无法用prim前缀别名绕过meta黑名单() {
        // `#%prim.eval!` 是同一个洞里最狠的一种：它能把任意源码字符串
        // 喂回 VM，等于整套白名单不存在（修复前实测能用它拉起进程）。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source(r#"(#%prim.eval! "(+ 1 2)")"#.to_string());

        // Assert
        assert!(
            matches!(result, Err(ScriptError::ParseError(_, _))),
            "实际拿到 {result:?}"
        );
    }

    #[test]
    fn 进程模块在活引擎里确实枚举得到() {
        // 防漂移第一层，挡的是「毒化悄悄变成空操作」这种失效方式：
        // `poison_module`/`poisoned_identifiers` 都写成
        // `if let Some(module) = engine.builtin_modules().get(name)`，
        // 上游哪天改了模块名、或不再把 steel/process 放进模块注册表，
        // `get` 返回 None，两处循环一次都不跑、测试却仍然全绿——整个
        // 进程能力会静默地重新暴露。这里直接钉住「模块枚举得到，且
        // 里面确实有进程原语」。
        // Arrange
        let 参照 = 未毒化的参照引擎();

        // Act
        let 导出名字: HashSet<String> = 参照
            .builtin_modules()
            .get("steel/process")
            .expect("steel/process 必须仍在沙箱引擎的模块注册表里，否则全部毒化都成了空操作")
            .names()
            .into_iter()
            .collect();

        // Assert：两个哨兵名字，同步来源是 steel-core 0.8.2 的
        // `src/primitives/process.rs` 里的 `process_module()`。
        assert!(导出名字.contains("command"), "实际导出 {导出名字:?}");
        assert!(导出名字.contains("spawn-process"), "实际导出 {导出名字:?}");
    }

    #[test]
    fn 进程模块的每个导出名字连同别名拼写都不在白名单内() {
        // 防漂移第二层，挡的是「上游新增了一个我们没见过的进程原语」：
        // 判据不是任何手抄的名单，而是**活引擎里 steel/process 当前
        // 真实导出的每一个名字**，逐个连同 MODULE_ALIAS_PREFIXES 的
        // 别名拼写一起检查。升级 steel-core 之后多出来的新原语会自动
        // 进入这条断言的覆盖范围，不需要谁记得改名单。
        // Arrange：两个引擎都在编译任何脚本之前造齐（C6，ADR 0028）。
        let 参照 = 未毒化的参照引擎();
        let engine = ScriptEngine::new();
        let 导出名字 = 参照
            .builtin_modules()
            .get("steel/process")
            .expect("见「进程模块在活引擎里确实枚举得到」那条测试")
            .names();

        // Act & Assert：拼写清单在这里就地拼出来，**不复用**
        // `spellings_of`——测试与被测实现共用同一个助手的话，助手本身
        // 出错时两边会一起错、断言恒真（实测：把 `spellings_of` 改成
        // 只返回裸名字，这条测试若复用它就仍然全绿）。
        for name in 导出名字 {
            let mut 拼写清单 = vec![name.clone()];
            拼写清单.extend(
                MODULE_ALIAS_PREFIXES
                    .iter()
                    .map(|前缀| format!("{前缀}{name}")),
            );
            for 拼写 in 拼写清单 {
                assert!(
                    !engine.allowed_identifiers.contains(拼写.as_str()),
                    "进程原语「{拼写}」仍在白名单内——毒化漏了这个拼写"
                );
            }
        }
    }

    #[test]
    fn 别名前缀清单覆盖上游当前真实存在的每一种别名拼写() {
        // 防漂移第三层，挡的是「上游换了或新增了别名前缀」：
        // MODULE_ALIAS_PREFIXES 是本文件唯一手抄自上游的常量，这条测试
        // 不信任那份手抄，而是从未毒化的参照引擎上**反推**出上游当前
        // 真实在用的前缀集合，再断言它被那份手抄覆盖住。
        //
        // 反推的判据是别名机制本身：`(require-builtin M as P)` 会给
        // 模块 M 的**每一个**导出名字都造一个 `P<name>` 绑定。所以先
        // 用某一个导出名字捞出所有「以它结尾」的全局名字得到候选前缀，
        // 再只保留那些对模块**全部**导出名字都成立的候选——`with-`
        // 这种恰好撞上 `with-env-var`/`env-var` 的巧合会被这一步筛掉
        // （不存在 `with-spawn-process`），真正的别名前缀留得下来。
        // Arrange
        let 参照 = 未毒化的参照引擎();
        let 全局名字: HashSet<String> = 参照
            .globals()
            .iter()
            .map(|interned| interned.resolve().to_string())
            .collect();

        // Act & Assert
        for module_name in FULLY_POISONED_MODULES {
            let Some(module) = 参照.builtin_modules().get(module_name) else {
                continue;
            };
            let 导出名字: Vec<String> = module.names();
            let Some(探针) = 导出名字.first() else {
                continue;
            };

            let mut 候选前缀: HashSet<String> = 全局名字
                .iter()
                .filter(|name| name.ends_with(探针.as_str()))
                .map(|name| name[..name.len() - 探针.len()].to_string())
                .collect();
            候选前缀.retain(|前缀| {
                导出名字
                    .iter()
                    .all(|名字| 全局名字.contains(&format!("{前缀}{名字}")))
            });

            for 前缀 in 候选前缀 {
                assert!(
                    前缀.is_empty() || MODULE_ALIAS_PREFIXES.contains(&前缀.as_str()),
                    "模块「{module_name}」在全局环境里有一种 MODULE_ALIAS_PREFIXES \
                     没收录的别名前缀「{前缀}」——被禁名字因此还有一种没被毒化的拼写"
                );
            }
        }
    }

    #[test]
    fn 换行分隔的require不再绕过文本层检查() {
        // 修复前 reject_dangerous_syntax 用的是字面子串 `"(require "`，
        // 左括号与 require 之间换行就绕过去了，随后展开阶段真的会去
        // 读盘（实测 win.ini 的 `[fonts]` 段名出现在了报错里）。
        // Arrange
        let mut engine = ScriptEngine::new();
        let 真实文件 = 写一个真实的磁盘模块("newline");

        // Act
        let result = engine.load_source(format!("(require\n\"{}\")", 写成steel字符串(&真实文件)));

        // Assert：断言到「是**文本层**拒绝的」这个粒度，不能只断言
        // `ParseError`——兜底解析器最终也产出 `ParseError`，只断言类型
        // 的话，文本层整个退回旧的字面子串匹配都测不出来（实测：那样
        // 的变异体能让本条测试保持全绿）。「禁止的语法」这个前缀是
        // 文本层独有的标记，兜底那条路不带；命中位置同理，只有文本层
        // 给得出来。
        let _ = std::fs::remove_file(&真实文件);
        match result {
            Err(ScriptError::ParseError(message, offset)) => {
                assert!(
                    message.starts_with("禁止的语法"),
                    "期望文本层在编译前就拒绝，实际是「{message}」"
                );
                assert!(
                    offset.is_some(),
                    "文本层必须带上命中位置，供加载管理界面换算行号"
                );
            }
            other => panic!("期望 ParseError，实际拿到 {other:?}"),
        }
    }

    #[test]
    fn require磁盘上真实存在的绝对路径被拒绝且错误信息说得清楚() {
        // Arrange
        let mut engine = ScriptEngine::new();
        let 真实文件 = 写一个真实的磁盘模块("abs");

        // Act
        let result = engine.load_source(format!("(require \"{}\")", 写成steel字符串(&真实文件)));

        // Assert：既要拒绝，也要给 mod 作者一句读得懂的结论，而不是
        // 「Unable to find builtin module」这种内部说法。
        let _ = std::fs::remove_file(&真实文件);
        match result {
            Err(ScriptError::ParseError(message, _)) => {
                assert!(
                    message.contains(REQUIRE_UNSUPPORTED_MESSAGE),
                    "错误信息没告诉 mod 作者不支持 require，实际是「{message}」"
                );
            }
            other => panic!("期望 ParseError，实际拿到 {other:?}"),
        }
    }

    #[test]
    fn require上跳的相对路径被拒绝() {
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source("(require \"../../../../secret.scm\")".to_string());

        // Assert
        match result {
            Err(ScriptError::ParseError(message, _)) => {
                assert!(
                    message.contains(REQUIRE_UNSUPPORTED_MESSAGE),
                    "实际是「{message}」"
                );
            }
            other => panic!("期望 ParseError，实际拿到 {other:?}"),
        }
    }

    #[test]
    fn 解析器兜底独立于文本检查也能拒绝磁盘上真实存在的模块() {
        // 其余几条 require 测试走的是 `ScriptEngine::load_source`，
        // 文本层 `reject_dangerous_syntax` 会抢先拒绝——那证明不了
        // 权威防线 [`DenyAllSourceModules`] 有没有被真的装到引擎上。
        // 这条测试**绕开文本层**，直接对着 `ScriptEngine` 内部那个
        // 引擎跑 `require`，钉住「解析器确实装上了」这件事本身。
        //
        // 为什么值得单独钉：文本层是词法近似，实测已经漏过一次
        // （`[require "…"]` 用方括号写就绕过了只认 `(` 的那版扫描，
        // 全靠这道兜底接住）。兜底一旦在构造期被摘掉，那类漏网写法
        // 会直接变成真实的任意文件读取。
        //
        // 对照组用**没装解析器**的裸引擎跑同一份源码：它必须成功读到
        // 磁盘上的文件——读不到就说明这条测试根本没走到被测的那条路
        // （比如路径拼错），断言会退化成恒真。
        //
        // C6（ADR 0028）：同一线程上全部引擎构造必须先于全部脚本编译
        // ——所以两个引擎都在这里先造齐，再开始跑。
        // Arrange
        let mut 对照组 = Engine::new_sandboxed();
        let mut 被测 = ScriptEngine::new();
        let 真实文件 = 写一个真实的磁盘模块("backstop");
        let source = format!("(require \"{}\") 磁盘上的答案", 写成steel字符串(&真实文件));

        // Act
        let 洞还在吗 = 对照组.run(source.clone());
        let 兜底结果 = 被测.engine.run(source);

        // Assert
        let _ = std::fs::remove_file(&真实文件);
        assert!(
            洞还在吗.is_ok(),
            "对照组理应读到磁盘上的模块——读不到说明这条测试没在测真正的那条路：{洞还在吗:?}"
        );
        let err = 兜底结果.expect_err("ScriptEngine 的引擎上必须装着 DenyAllSourceModules");
        assert!(
            err.to_string().contains(UNRESOLVED_MODULE_MARKER),
            "拒绝理由应当是「解析不出这个模块」而不是别的失败，实际是「{err}」"
        );
    }

    #[test]
    fn 方括号写法的require也被拒绝() {
        // Steel 三种括号等价，`[require "…"]` 与 `(require "…")` 是同一
        // 件事——实测这种写法曾经穿过只认圆括号的文本层扫描。
        // Arrange
        let mut engine = ScriptEngine::new();
        let 真实文件 = 写一个真实的磁盘模块("bracket");

        // Act
        let result = engine.load_source(format!("[require \"{}\"]", 写成steel字符串(&真实文件)));

        // Assert
        let _ = std::fs::remove_file(&真实文件);
        match result {
            Err(ScriptError::ParseError(message, _)) => {
                assert!(
                    message.contains(REQUIRE_UNSUPPORTED_MESSAGE),
                    "实际是「{message}」"
                );
            }
            other => panic!("期望 ParseError，实际拿到 {other:?}"),
        }
    }

    #[test]
    fn 毒化别名拼写没有误伤正常脚本() {
        // 与上面几条拒绝测试配对的「没打错人」证据：`#%prim.` 前缀下
        // 绝大多数名字（`#%prim.hash` 等）是 struct 宏展开的底层依赖，
        // 毒化只能命中被禁模块那一份，不能波及它们。
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act
        let result = engine.load_source(
            "(struct 点 (x y))\n(define 双倍 (map (lambda (n) (* 2 n)) (list 1 2 3)))".to_string(),
        );

        // Assert
        assert!(result.is_ok(), "正常脚本被误伤：{result:?}");
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

    #[test]
    fn 读档完成后强制重建全部脚本引擎() {
        // 用重建计数器断言「重建确实发生」——不是断言「行为看起来
        // 正常」这种弱验证，见 rebuild_all_engines_after_load 文档。
        // `REBUILD_COUNT` 是进程级全局状态，测试默认并行执行，其他
        // 测试用例（本文件另外两条重建相关测试）可能同时递增它——因此
        // 这里只断言"调用后计数严格增加"，不断言恰好 +1，避免对测试
        // 执行顺序/并发性做出不成立的假设。
        // Arrange
        let sources = vec![
            ("moda".to_string(), "(define answer 1) answer".to_string()),
            ("modb".to_string(), "(define answer 2) answer".to_string()),
        ];
        let count_before = rebuild_count();

        // Act
        let rebuilt = rebuild_all_engines_after_load(&sources);

        // Assert
        assert!(rebuild_count() > count_before);
        assert_eq!(rebuilt.len(), 2);
        assert!(rebuilt.iter().all(|(_, result)| result.is_ok()));
    }

    #[test]
    fn 重建产出的引擎是全新实例而非复用旧状态() {
        // 每个 mod 各自拿到一个从零构造的 ScriptEngine——重建出的引擎
        // 本身是功能完好的全新实例，可以正常继续装载/调用，不是一个
        // 半残的占位对象。
        // Arrange
        let sources = vec![(
            "lostland".to_string(),
            "(define answer 1) answer".to_string(),
        )];

        // Act
        let mut rebuilt = rebuild_all_engines_after_load(&sources);
        let (namespace, engine_result) = rebuilt.remove(0);
        let mut engine = engine_result.expect("合法脚本源码理应重建成功");
        engine
            .load_source("(define (probe) 42)".to_string())
            .expect("重建出的引擎应当是一个功能完好的全新实例");

        // Assert
        assert_eq!(namespace, "lostland");
        assert_eq!(
            engine.call_raw("probe", Vec::new()),
            Ok(steel::rvals::SteelVal::IntV(42))
        );
    }

    #[test]
    fn 重建时语法错误的mod返回err不影响同批其他mod() {
        // Arrange
        let sources = vec![
            ("broken".to_string(), "(+ 1 2".to_string()),
            ("good".to_string(), "(define answer 1) answer".to_string()),
        ];

        // Act
        let rebuilt = rebuild_all_engines_after_load(&sources);

        // Assert
        let broken = rebuilt.iter().find(|(ns, _)| ns == "broken").unwrap();
        let good = rebuilt.iter().find(|(ns, _)| ns == "good").unwrap();
        assert!(broken.1.is_err());
        assert!(good.1.is_ok());
    }
}
