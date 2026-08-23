//! mod 脚本的模块系统：`(require "模块名")` 解析到哪份源码、允许不允许。
//!
//! # 项目所有者要的语义
//!
//! > 「每个文件能够导出导入，文件之间是隔离的，同时能导入本 Mod 的
//! > 东西，而导入其他 mod 的东西是需要有那个 Mod 的 id 作为域名前缀
//! > 的。」
//!
//! 落成三条规则：
//!
//! | 写法 | 含义 |
//! |------|------|
//! | `(require "helpers")` | 本 mod 根目录下的 `helpers.scm` |
//! | `(require "content/races")` | 本 mod 根目录下的 `content/races.scm` |
//! | `(require "lostland:helpers")` | `lostland` 这个 mod 根目录下的 `helpers.scm`，且本 mod 必须在 `mod.json5` 的 `dependencies` 里声明过 `lostland` |
//!
//! 导出用 `provide`（**不是** `export`）：没写进 `provide` 的名字在
//! 要求方**编译期**就不可见（实测报 `FreeIdentifier`），完全没写
//! `provide` 的模块什么都不导出——不是「默认全导出」。
//!
//! # Steel 运行期永不碰盘
//!
//! `steel-core` 0.8.2 解析 `(require "…")` 时按固定顺序问：内置模块表
//! → `custom_builtins` → [`steel::compiler::modules::SourceModuleResolver`]
//! 的 `exists()` → **都不认就当成文件路径直接 `std::fs::File::open`**。
//! 最后那一步不看沙箱标志、不看搜索目录白名单，绝对路径与 `..` 一律
//! 照读。[`ModuleResolver`] 的 `exists()` 因此**恒返回 `true`**，把文件系统
//! 那一支彻底挤掉；能不能拿到源码完全由它的 `resolve()`
//! 说了算，而它只在一张**我们自己读盘、自己校验过**的内存表
//! （[`ModuleTable`]）里查。
//!
//! 这也是不能用 `Engine::add_search_directory` 的原因：那个 API 的方向
//! 恰好相反，只会**扩大**文件系统搜索面。
//!
//! # 键从磁盘反推，不是拿脚本给的字符串去拼路径
//!
//! 表由调用方（`ll_mod::pipeline`）遍历 mod 目录构造：**每一个键都是
//! 从一个真实存在的文件名反推出来的**。脚本写的字符串只用来在这张表
//! 里查，从来不参与拼接文件路径——目录上跳与绝对路径因此不是「被识别
//! 出来挡住的」，是根本没有一条从键到文件系统的通路。[`parse_key`]
//! 里那些语法检查是为了给 mod 作者一句准确的话，不是这道隔离的依据。
//!
//! # 同一个 mod 内，模块状态是共享的（约束，不是 Steel 的保证）
//!
//! 「每个脚本一份私有副本」这条性质依赖的是**一个脚本一个 VM** 这条
//! 纪律，不是 Steel 的语义。当前的装载管线是**一个 mod 一个 VM**
//! （`ll_mod::pipeline::load_all`），因此：
//!
//! - 同一个 mod 内，一个模块只会被求值**一次**（实测：
//!   `crates/ll-script/examples/probe_modules.rs` 第 11 节——两份脚本先后
//!   `require` 同一个模块，模块体的副作用只发生一次），它顶层 `define`
//!   出来的状态被本 mod 的全部脚本共享。
//! - 跨 mod 是真副本：每个 mod 自己的 VM 各自把依赖方的模块源码重新
//!   编译一遍，两边的模块级状态互不可见。
//!
//! 写 mod 时不能指望「我 require 一次就拿到一份新的」。要独立状态，就
//! 别把状态放在模块顶层。
//!
//! # 跨 mod require 的模块，在**要求方**的 VM 里求值
//!
//! `(require "lostland:helpers")` 是把 `lostland` 的那份源码搬进本 mod
//! 的引擎重新编译一次，不是去 `lostland` 的引擎里取值。因此被跨 mod
//! require 的模块**不该带副作用**——它里面的 `register-*` 调用会以
//! **要求方**的身份注册内容（事件订阅表记的 mod id 来自装载管线当前
//! 处理的那个 mod）。辅助函数、常量表这类纯定义才是跨 mod 模块的正确
//! 用法。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// 模块源码文件的扩展名——[`ModuleTable`] 的键**不写**它。
///
/// 不写扩展名不是风格偏好：键就是 Steel 眼里的模块身份，同一份源码有
/// 两种拼写（`helpers` 与 `helpers.scm`）就会在同一个引擎里编译出两个
/// 互不相干的模块实例，模块级状态跟着分裂成两份。[`parse_key`] 因此
/// 把带扩展名的写法当成错误点名拒绝，而不是「顺手也认」。
pub const MODULE_FILE_EXTENSION: &str = "scm";

/// 模块路径里**不允许**出现的字符。
///
/// `.` 在列表里，`..` 就不可能拼得出来——目录上跳不是靠「识别 `..` 这个
/// 特例」挡住的，是靠字符集让它压根写不出来。`:` 也在列表里：跨 mod
/// 前缀在 [`parse_key`] 里已经先被切走，剩下的路径部分再出现 `:` 只
/// 可能是 `C:/…` 这类盘符写法。反斜杠、通配符与重定向符号一并禁掉，
/// 是为了让这份字符集在 Windows 与 POSIX 上给出同一个答案。
const FORBIDDEN_PATH_CHARS: [char; 10] = ['.', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'];

/// 跨 mod 前缀与路径之间的分隔符：`lostland:helpers`。
const NAMESPACE_SEPARATOR: char = ':';

/// 一个已经通过语法校验的模块键。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleKey {
    /// 跨 mod 前缀（`lostland:helpers` 的 `lostland`）；无前缀写法为
    /// `None`，表示「本 mod」。
    pub namespace: Option<String>,
    /// 相对 mod 根目录的路径，不含扩展名，分隔符恒为 `/`。
    pub path: String,
}

impl ModuleKey {
    /// 这个键原样写出来是什么样——[`ModuleTable`] 用它做查找键，保证
    /// 「脚本里怎么写的」与「表里怎么存的」是同一个字符串。
    pub fn canonical(&self) -> String {
        match &self.namespace {
            Some(ns) => format!("{ns}{NAMESPACE_SEPARATOR}{}", self.path),
            None => self.path.clone(),
        }
    }
}

/// 解析并校验一个模块键的**语法**（不涉及「这个 mod 声明没声明依赖」
/// 与「文件在不在」，那两件事是 [`ModuleTable::check`] 的判断）。
///
/// 拒绝的写法与理由：
/// - 带扩展名（`helpers.scm`）——同一份源码两种拼写会分裂成两个模块
///   实例，见 [`MODULE_FILE_EXTENSION`]。
/// - 绝对路径（`/etc/passwd`、`C:/Windows/win.ini`）与目录上跳
///   （`../secret`）——前者靠「路径不得以 `/` 开头」与盘符里的 `:`
///   拒绝，后者靠 [`FORBIDDEN_PATH_CHARS`] 含 `.` 而不可能拼出。
/// - 空段（`a//b`、`a/`）、含空白或控制字符的段。
/// - 一个以上的 `:`。
///
/// `self_namespace` 用来拒绝**显式写出本 mod 前缀**的写法
/// （`lostland:helpers` 出现在 `lostland` 自己的脚本里）：允许它等于
/// 允许同一个模块有两种拼写，与扩展名那条是同一个理由。
pub fn parse_key(raw: &str, self_namespace: &str) -> Result<ModuleKey, String> {
    let colon_count = raw.matches(NAMESPACE_SEPARATOR).count();
    if colon_count > 1 {
        return Err(format!("模块名最多一个「{NAMESPACE_SEPARATOR}」：{raw}"));
    }
    let (namespace, path) = match raw.split_once(NAMESPACE_SEPARATOR) {
        Some((ns, rest)) => (Some(ns), rest),
        None => (None, raw),
    };

    if let Some(ns) = namespace {
        validate_segment(ns).map_err(|why| format!("mod id「{ns}」{why}"))?;
        if ns == self_namespace {
            return Err(format!("本 mod 的模块不写前缀：{raw}"));
        }
    }

    // 上跳目录单独点名，排在扩展名与字符集之前——`../secret.scm` 真正
    // 的问题是它想跳出 mod 目录，不是它带了扩展名。字符集那一关也挡得
    // 住（`.` 是禁用字符），但报出来的话是「含非法字符『.』」，对着一个
    // 明显在尝试目录穿越的写法说这句，帮不到任何人。
    if path.split('/').any(|segment| segment == "..") {
        return Err(format!("模块名不能上跳目录：{raw}"));
    }
    if path.starts_with('/') {
        return Err(format!("模块名不能是绝对路径：{raw}"));
    }
    if path.ends_with(&format!(".{MODULE_FILE_EXTENSION}")) {
        return Err(format!("模块名不写扩展名：{raw}"));
    }
    if path.is_empty() {
        return Err("模块名为空".to_string());
    }
    for segment in path.split('/') {
        validate_segment(segment).map_err(|why| format!("模块名「{raw}」{why}"))?;
    }

    Ok(ModuleKey {
        namespace: namespace.map(str::to_string),
        path: path.to_string(),
    })
}

/// 一个路径段（或跨 mod 前缀）自身的字符集校验。
fn validate_segment(segment: &str) -> Result<(), String> {
    if segment.is_empty() {
        return Err("有空的路径段".to_string());
    }
    for ch in segment.chars() {
        if FORBIDDEN_PATH_CHARS.contains(&ch) || ch.is_whitespace() || ch.is_control() {
            return Err(format!("含非法字符「{ch}」"));
        }
    }
    Ok(())
}

/// 表里一条模块记录：要么有源码可供，要么已经在灌表时被判定不可用
/// （附一句理由）。
///
/// 「读得到文件但内容违规」不在灌表时直接把整个 mod 判失败，是刻意
/// 的：一个从来没被 `require` 过的文件不该拖垮整个 mod 的装载。理由
/// 存下来，等真的有人 require 它时再原样报出去。
#[derive(Debug, Clone)]
enum ModuleEntry {
    Available(String),
    Rejected(String),
}

/// 一个 mod 专属的「模块名 → 源码文本」表，外加它的跨 mod 权限。
///
/// 见模块文档「键从磁盘反推」一节——本类型只做查表与判权限，不碰文件
/// 系统。
#[derive(Debug, Clone, Default)]
pub struct ModuleTable {
    self_namespace: String,
    dependencies: HashSet<String>,
    entries: HashMap<String, ModuleEntry>,
}

impl ModuleTable {
    /// 一张什么都 require 不到的空表——[`crate::host::ScriptEngine::new`]
    /// 用的就是它，行为与「整个模块系统关闭」等价。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 造一张属于 `self_namespace` 这个 mod 的表，`dependencies` 是它在
    /// `mod.json5` 里声明过的依赖命名空间。
    pub fn new(self_namespace: impl Into<String>, dependencies: HashSet<String>) -> Self {
        Self {
            self_namespace: self_namespace.into(),
            dependencies,
            entries: HashMap::new(),
        }
    }

    /// 灌入一条模块源码。`namespace` 为 `None` 表示「本 mod 的文件」，
    /// `Some(ns)` 表示「依赖方 `ns` 的文件」；`path` 是相对那个 mod 根
    /// 目录、**不含扩展名**、分隔符为 `/` 的路径。
    ///
    /// 源码在这里就过一遍 [`crate::host::reject_dangerous_syntax`]
    /// ——模块体不经过 [`crate::host::ScriptEngine::load_source`]，那道
    /// 文本层检查够不着它，而白名单**拦不住模块体里的
    /// `require-builtin`**（实测：`(require-builtin steel/time)` 展开成
    /// `(define ##mm…instant/now (%module-get% %-builtin-module-steel/time
    /// 'instant/now))`，其中 `%module-get%` 与 `%-builtin-module-steel/time`
    /// 都在 `Engine::globals()` 里、因而都在白名单内，被禁的名字
    /// `instant/now` 只出现在 `quote` 里不受检查）。所以这道文本层检查
    /// 不是锦上添花，是模块体上唯一挡得住 `require-builtin` 的防线。
    ///
    /// 返回键本身，方便调用方记日志。
    pub fn insert(&mut self, namespace: Option<&str>, path: &str, source: String) -> String {
        let key = match namespace {
            Some(ns) => format!("{ns}{NAMESPACE_SEPARATOR}{path}"),
            None => path.to_string(),
        };
        let entry = match crate::host::reject_dangerous_syntax(&source) {
            Ok(()) => ModuleEntry::Available(source),
            Err(err) => ModuleEntry::Rejected(format!("模块「{key}」里有{err}")),
        };
        self.entries.insert(key.clone(), entry);
        key
    }

    /// 这个 mod 自己的命名空间。
    pub fn self_namespace(&self) -> &str {
        &self.self_namespace
    }

    /// 表里一条模块都没有——[`crate::host::ScriptEngine::with_modules`]
    /// 据此决定要不要多造一台展开器引擎（见那个字段的文档）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 判断脚本写的这个模块名能不能用，能用返回 `Ok`，不能用返回一句
    /// 写给 mod 作者看的理由。
    ///
    /// 四档判断依次是：语法（[`parse_key`]）→ 跨 mod 权限（目标是不是
    /// 声明过的依赖）→ 表里有没有这条 → 这条是不是灌表时就被判过不
    /// 可用。**权限在「文件存不存在」之前判**：否则「没声明依赖」与
    /// 「依赖方没这个文件」会给出同一句话，mod 作者分不清该改清单还是
    /// 改路径。
    pub fn check(&self, raw_key: &str) -> Result<(), String> {
        let key = parse_key(raw_key, &self.self_namespace)?;
        if let Some(ns) = &key.namespace
            && !self.dependencies.contains(ns)
        {
            return Err(format!(
                "未在 mod.json5 的 dependencies 里声明「{ns}」，不能 require 它的模块"
            ));
        }
        match self.entries.get(&key.canonical()) {
            Some(ModuleEntry::Available(_)) => Ok(()),
            Some(ModuleEntry::Rejected(why)) => Err(why.clone()),
            None => Err(format!("找不到模块：{raw_key}")),
        }
    }

    /// 取一条模块的源码——只有 [`Self::check`] 通过的键才拿得到。
    fn source(&self, raw_key: &str) -> Option<String> {
        self.check(raw_key).ok()?;
        let key = parse_key(raw_key, &self.self_namespace).ok()?;
        match self.entries.get(&key.canonical()) {
            Some(ModuleEntry::Available(source)) => Some(source.clone()),
            _ => None,
        }
    }
}

/// 装在引擎上的源码模块解析器：`exists()` 恒真挤掉文件系统那一支，
/// `resolve()` 只在 [`ModuleTable`] 里查。完整机制见模块文档。
pub(crate) struct ModuleResolver {
    table: Arc<ModuleTable>,
}

impl ModuleResolver {
    pub(crate) fn new(table: Arc<ModuleTable>) -> Self {
        Self { table }
    }
}

impl steel::compiler::modules::SourceModuleResolver for ModuleResolver {
    fn resolve(&self, key: &str) -> Option<String> {
        self.table.source(key)
    }

    /// 恒 `true`：这是整道防线的机制本身（见模块文档「Steel 运行期永不
    /// 碰盘」）——返回 `false` 会让 `steel-core` 继续走到文件系统分支，
    /// 洞就还在。
    fn exists(&self, _key: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 表() -> ModuleTable {
        let mut table = ModuleTable::new("examplemod", HashSet::from(["lostland".to_string()]));
        table.insert(None, "helpers", "(provide f) (define (f) 1)".to_string());
        table.insert(
            None,
            "content/races",
            "(provide g) (define (g) 2)".to_string(),
        );
        table.insert(
            Some("lostland"),
            "helpers",
            "(provide h) (define (h) 3)".to_string(),
        );
        table
    }

    #[test]
    fn 本mod的相对模块名通过校验() {
        assert!(表().check("helpers").is_ok());
        assert!(表().check("content/races").is_ok());
    }

    #[test]
    fn 声明过依赖的跨mod模块名通过校验() {
        assert!(表().check("lostland:helpers").is_ok());
    }

    #[test]
    fn 未声明依赖的跨mod模块名被拒绝且点名是依赖问题() {
        let err = 表().check("someone:helpers").expect_err("必须拒绝");
        assert!(err.contains("dependencies"), "实际是「{err}」");
    }

    #[test]
    fn 未声明依赖优先于文件不存在报出来() {
        // 「没声明依赖」与「依赖方没这个文件」必须给出不同的话，否则
        // mod 作者不知道该改清单还是改路径。
        let err = 表().check("someone:没有这个文件").expect_err("必须拒绝");
        assert!(err.contains("dependencies"), "实际是「{err}」");
    }

    #[test]
    fn 绝对路径被拒绝() {
        for raw in ["/etc/passwd", "C:/Windows/win.ini", "//server/share/x"] {
            assert!(表().check(raw).is_err(), "「{raw}」本该被拒绝");
        }
    }

    #[test]
    fn 目录上跳被拒绝() {
        for raw in ["../secret", "a/../../secret", ".."] {
            assert!(表().check(raw).is_err(), "「{raw}」本该被拒绝");
        }
    }

    #[test]
    fn 带扩展名的写法被拒绝且点名扩展名() {
        let err = 表().check("helpers.scm").expect_err("必须拒绝");
        assert!(err.contains("扩展名"), "实际是「{err}」");
    }

    #[test]
    fn 显式写本mod前缀被拒绝() {
        let mut table = ModuleTable::new("lostland", HashSet::new());
        table.insert(None, "helpers", "(provide f) (define (f) 1)".to_string());
        let err = table.check("lostland:helpers").expect_err("必须拒绝");
        assert!(err.contains("不写前缀"), "实际是「{err}」");
    }

    #[test]
    fn 空表什么都require不到() {
        let table = ModuleTable::empty();
        assert!(table.check("helpers").is_err());
    }

    #[test]
    fn 模块源码里的requirebuiltin在灌表时就被记成不可用() {
        let mut table = ModuleTable::new("m", HashSet::new());
        table.insert(
            None,
            "evil",
            "(require-builtin steel/time) (provide now) (define (now) (instant/now))".to_string(),
        );
        let err = table.check("evil").expect_err("必须拒绝");
        assert!(err.contains("require-builtin"), "实际是「{err}」");
    }

    #[test]
    fn 灌表时不可用的模块不影响同一张表里别的模块() {
        let mut table = ModuleTable::new("m", HashSet::new());
        table.insert(None, "evil", "(require-builtin steel/time)".to_string());
        table.insert(None, "good", "(provide f) (define (f) 1)".to_string());
        assert!(table.check("good").is_ok());
    }

    #[test]
    fn 多个冒号被拒绝() {
        assert!(表().check("a:b:c").is_err());
    }

    #[test]
    fn 空段被拒绝() {
        for raw in ["a//b", "a/", "/a", ""] {
            assert!(表().check(raw).is_err(), "「{raw}」本该被拒绝");
        }
    }

    #[test]
    fn canonical把前缀原样拼回去() {
        let key = parse_key("lostland:content/races", "examplemod").expect("合法");
        assert_eq!(key.canonical(), "lostland:content/races");
        let key = parse_key("content/races", "examplemod").expect("合法");
        assert_eq!(key.canonical(), "content/races");
    }
}
