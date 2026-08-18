//! Steel VM 宿主封装。
//!
//! # 为什么以 `Engine::new_sandboxed()` 为唯一起点，且仍需主动清空
//!
//! ADR 0012 实测：`Engine::new()` 默认把文件系统与网络都暴露给脚本；
//! `Engine::new_sandboxed()` 排除了这两项（sandboxed 版 `steel/filesystem`
//! 模块零导出、`steel/tcp`/`steel/http` 干脆没注册），但**不**排除进程执行、
//! 原生线程、系统时间、非 `DetRng` 随机——这四项在两种引擎下都默认可达，
//! 必须在构造期主动清空，见 [`ScriptEngine::new`]。

use std::time::Duration;

use steel::SteelErr;
use steel::rvals::SteelVal;
use steel::steel_vm::builtin::BuiltInModule;
use steel::steel_vm::engine::Engine;
use steel::steel_vm::interrupt::InterruptHandler;
use steel::steel_vm::register_fn::RegisterFn;

/// 脚本失控时的中断预算：超过这个墙钟时长仍未返回就强制掐断。
///
/// ADR 0001 实测死循环在 500ms 超时下能被稳定掐断；这里取 300ms——够长到
/// 不会误伤正常的重度计算（技能结算不该跑到几百毫秒），也够短到不会让
/// 单个失控脚本卡住一整帧的观感。具体数值属于可调参数，不是精确科学，
/// 后续如有真实卡顿投诉可以再调。
const INTERRUPT_TIMEOUT: Duration = Duration::from_millis(300);

/// 引导阶段无条件注册、且整个模块都不该留给脚本的 Steel 内置模块。
///
/// 见 ADR 0012：这五个模块与 `sandbox` 标志无关，`new_sandboxed()` 挡不住
/// 它们。`steel/meta` 尤其庞杂（实测 102 个导出名字，混着 `eval!`、`run!`、
/// `Engine::new`、`set-env-var!`、`load`、`env-var` 这类逃逸入口和
/// `value->string`、`arity?` 这类无害的内省函数）——**不逐个甄别，整体清空**：
/// 甄别是「规则禁止」思路，会随 steel-core 版本升级悄悄漏项；本项目现阶段
/// mod 脚本不需要任何 `steel/meta` 能力（task 5 的 API 走我们自己注册的
/// 函数，不需要脚本自省），全部清空没有已知代价。若后续任务发现某个具体
/// 名字（比如 `struct`/`make-struct-type` 相关的自定义数据类型）确实需要，
/// 应该单独、显式地把那一个名字重新放行，并写清楚理由——不是恢复整个模块。
const FULLY_POISONED_MODULES: [&str; 5] = [
    "steel/random",
    "steel/time",
    "steel/threads",
    "steel/process",
    "steel/meta",
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

/// 出现在脚本源码里就直接拒绝加载的字面子串。
///
/// 这不是「审查脚本写了什么」的黑名单式思路——它是
/// [`FULLY_POISONED_MODULES`] 那套「清空已绑定名字」机制对 `steel/time`
/// 失效之后，唯一验证有效的兜底：`require-builtin`/`require` 都是**编译期**
/// 处理的特殊语法，不是普通函数值，无法用 `register_value` 覆盖成毒值；
/// 唯一能确定挡住它们的办法是不让含有这个词的源码进入编译。脚本合法的
/// mod 逻辑本来就不需要写 `require`——task 5 的全部能力都通过宿主预先
/// `register_fn` 好的函数直接调用,不需要脚本自己 `require` 任何 Steel
/// 内置模块。
const BANNED_SOURCE_SUBSTRINGS: [&str; 2] = ["require-builtin", "(require "];

/// 检查源码文本是否触碰了 [`BANNED_SOURCE_SUBSTRINGS`]。
///
/// 命中哪一个子串，就把它写进错误信息里,方便 mod 作者定位——不能只说
/// 「被拒绝」,不给理由。
fn reject_dangerous_syntax(source: &str) -> Result<(), ScriptError> {
    for banned in BANNED_SOURCE_SUBSTRINGS {
        if source.contains(banned) {
            return Err(ScriptError::ParseError(format!(
                "脚本源码包含禁止的语法「{banned}」——mod 脚本不允许 require 任何 Steel 内置模块，\
                 所有能力必须通过宿主注册的函数访问"
            )));
        }
    }
    Ok(())
}

/// 脚本调用失败的分类。
///
/// 四道防线①②在此落地：[`ScriptEngine::call`] 与
/// [`ScriptEngine::load_source`] 的签名本身就是 `Result`，出错必定拿到
/// `Err` 而不是 panic；具体要不要降级、降级成什么默认值，是**调用方**的
/// 决定，本类型只保证「出错一定可观测」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// 因超时（脚本失控）被 `InterruptHandler` 强制掐断。
    Interrupted,
    /// 调用 Rust 侧注册函数时缺参或多参。
    ArityMismatch(String),
    /// 源码语法错误，编译阶段就失败，从未开始求值。
    ParseError(String),
    /// 求值期间的其余运行时错误（未定义标识符、类型不匹配等）。
    Runtime(String),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::Interrupted => write!(f, "脚本执行超时被中断"),
            ScriptError::ArityMismatch(msg) => write!(f, "参数个数不匹配：{msg}"),
            ScriptError::ParseError(msg) => write!(f, "脚本语法错误：{msg}"),
            ScriptError::Runtime(msg) => write!(f, "脚本运行时错误：{msg}"),
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

    let message = err.to_string();
    match err.kind() {
        ErrorKind::ArityMismatch => ScriptError::ArityMismatch(message),
        ErrorKind::Parse | ErrorKind::UnexpectedToken => ScriptError::ParseError(message),
        _ => ScriptError::Runtime(message),
    }
}

/// 脚本宿主：包装一个经过能力收窄的 Steel VM 实例。
///
/// 每次调用都套一层 [`InterruptHandler`]：ADR 0012 实测这层包装把单次
/// 调用从 74ns 拉到 326ns，仍是纳秒级，可以放在每帧每实体的热路径上，
/// 不需要为脚本调用另开线程。
pub struct ScriptEngine {
    engine: Engine,
    interrupt: InterruptHandler,
}

impl ScriptEngine {
    /// 构造一个已经完成能力收窄的脚本引擎。
    ///
    /// 顺序不能变：`Engine::new_sandboxed()` 打底之后必须**立即**清空
    /// 危险模块与危险全局名字，任何脚本源码都不能在这之前被求值——
    /// 否则清空动作本身就晚了。
    pub fn new() -> Self {
        let mut engine = Engine::new_sandboxed();

        for module_name in FULLY_POISONED_MODULES {
            poison_module(&mut engine, module_name);
        }

        let interrupt = InterruptHandler::new(&mut engine, INTERRUPT_TIMEOUT);

        Self { engine, interrupt }
    }

    /// 注册一个 Rust 函数，供脚本以 `name` 调用。
    ///
    /// 转发到 Steel 的 `RegisterFn` trait；缺参/多参的 arity 检查是 Steel
    /// 原生行为（ADR 0001 已确认），这里不重新实现。
    pub fn register_fn<Args, Ret, F>(&mut self, name: &'static str, func: F) -> &mut Self
    where
        Engine: RegisterFn<F, Args, Ret>,
    {
        self.engine.register_fn(name, func);
        self
    }

    /// 加载并执行一段脚本源码（通常是 mod 的顶层定义）。
    ///
    /// 先过 [`reject_dangerous_syntax`] 这道源码文本关，再套中断防线执行：
    /// 源码本身死循环也会在预算耗尽后返回 `Err`。
    pub fn load_source(&mut self, source: String) -> Result<(), ScriptError> {
        reject_dangerous_syntax(&source)?;

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
        assert!(matches!(result, Err(ScriptError::ArityMismatch(_))));
    }

    #[test]
    fn 语法错误的源码返回解析错误而非崩溃进程() {
        // Arrange
        let mut engine = ScriptEngine::new();

        // Act：故意少一个右括号。
        let result = engine.load_source("(+ 1 2".to_string());

        // Assert
        assert!(matches!(result, Err(ScriptError::ParseError(_))));
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
        assert!(matches!(result, Err(ScriptError::ParseError(_))));
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
