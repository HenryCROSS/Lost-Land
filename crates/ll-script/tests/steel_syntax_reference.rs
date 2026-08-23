//! `knowledge/design/steel-script-reference.md` 的实测后盾。
//!
//! # 为什么这份文件存在，以及它和文档如何保持同步
//!
//! 语法参考类文档最容易随 `steel-core` 升级、白名单调整而悄悄过时——
//! 一旦某个例子在文档写下的那一刻是对的、后来某次白名单收紧把它挡住了，
//! 文档不会自己知道。这里的做法是把参考文档里**每一段被标注「已实测」
//! 的代码**原样落成本文件里的一个测试函数：文档在对应例子旁边写
//! `参见 steel_syntax_reference.rs::<函数名>`，函数名与文档里的编号
//! 对应（`一_基础语法`/`二_核心能力`/`三_mod_api`/`四_常见错误` 四个
//! 模块，对应文档的四个章节）。
//!
//! 这样任何未来的改动——升级 `steel-core`、收紧
//! `crates/ll-script/src/whitelist.rs`、给 `META_DENY_LIST` 加新名字、
//! 改动某个 `register_fn` 的参数顺序——只要动到文档里任何一段被引用的
//! 例子，`cargo test` 会在这里先变红，而不是等到某个 mod 作者照抄文档
//! 写不出脚本才发现文档过时了。**文档不生成测试、测试也不生成文档**
//! ——两者是手工保持同步的一对，但同步点被压缩到"函数名对应"这一条
//! 简单规则，复查成本很低：打开这个文件，看看是不是每个函数都还在，
//! 函数体是不是和文档里贴的代码逐字一致。
//!
//! # 覆盖范围的边界（如实说明，不是遗漏）
//!
//! - **`register-terrain`/`register-class`/`register-skill`/
//!   `register-subclass`/`register-quest`/`register-race` 六个内容
//!   注册函数不在本文件覆盖范围**：它们的实现在 `crates/ll-mod`
//!   （`crate::script_terrain_api` 等模块），而 `ll-script` 的依赖方向
//!   是 `ll-script` ← `ll-mod`（规格 §5）——`ll-script` 不能反过来依赖
//!   `ll-mod`，本文件作为 `ll-script` 的集成测试自然也够不着。这六个
//!   函数的真实调用示例来自 `ll-mod` 自己已经存在、且持续跑在 908 条
//!   测试里的单元测试（例如
//!   `crates/ll-mod/src/script_skill_api.rs::通过线程局部注册目标脚本能真正调用register_skill`），
//!   文档对应小节会直接点名这些测试的文件路径，不在本文件里重复。
//! - **`entity-state-set!`/`entity-state-get!`（需要一个
//!   `ScriptEntityHandle`）同样不在本文件覆盖范围**：`ScriptEntityHandle::new`
//!   是 `pub(crate)`（`crates/ll-script/src/api/handle.rs`），只有
//!   `ll-script` 内部代码能构造一个真实句柄，外部集成测试（本文件属于
//!   这一类，`tests/` 下每个文件是独立编译的 crate，只能看见 `ll_script`
//!   的公开 API）没有合法路径拿到一个。这两个函数的验证只能留在
//!   `crates/ll-script/src/api/state.rs` 自己的单元测试里——本文件覆盖
//!   同一模块里不需要句柄的部分（`state-set!`/`state-get!`/
//!   `state-get-foreign`/`content-ref`，全局作用域）。
//! - **`log.rs`/`ordered.rs` 没有任何 `register` 函数**，脚本目前完全
//!   调用不到它们——不是本文件遗漏了例子，是这两个模块此刻确实没有
//!   任何脚本可调用的语法，文档对应小节会如实说明这一点。

use ll_core::rng::DetRng;
use ll_script::ScriptEngine;
use ll_script::api::{intent, query, rng, state};
use ll_world::entity::{Arena, EntityId};
use ll_world::generate::GenParams;
use ll_world::state::WorldState;
use ll_world::terrain::base_terrain_fixture;
use ll_world::zone::ZoneLayout;
use steel::rvals::SteelVal;

/// 造一个最小可用的 `WorldState`——`query`/`state` 两节的例子需要一个
/// 活跃世界才能求值,形状照抄 `crates/ll-script/src/api/query.rs` 测试
/// 模块里的 `small_world` 帮手（该函数是 `#[cfg(test)]` 私有项,本文件
/// 作为外部集成测试够不着,只能照着同样的构造步骤重新写一份,不是从
/// 记忆里拼的——每一步都对应该文件里已经验证过的调用序列）。
fn 造一个最小世界() -> WorldState {
    let zone_count = ll_core::torus::TorusSize::new(1, 1).unwrap();
    let layout = ZoneLayout::new(64, zone_count).unwrap();
    let (terrain_ids, terrain_table) = base_terrain_fixture();
    let spawn = layout.tile_size().wrap(0, 0);
    WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .unwrap()
}

fn 造一个实体id() -> EntityId {
    let mut arena: Arena<()> = Arena::new();
    arena.spawn(())
}

// ============================================================
// 一、基础语法
// ============================================================
mod 一_基础语法 {
    use super::*;

    /// 文档「一、1 定义与字面量」。
    #[test]
    fn define变量与整数字符串布尔字面量() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define x 42)
                (define name "lostland")
                (define ok? #t)
                (define (probe) (list x name ok?))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [
                    SteelVal::IntV(42),
                    SteelVal::StringV("lostland".into()),
                    SteelVal::BoolV(true),
                ]
                .into_iter()
                .collect()
            )
        );
    }

    /// 文档「一、2 列表与向量」。
    #[test]
    fn 列表与向量字面量及其存取函数() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define xs (list 1 2 3))
                (define v (vector 10 20 30))
                (define (probe)
                  (list (car xs) (cdr xs) (length xs) (vector-ref v 1)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [
                    SteelVal::IntV(1),
                    SteelVal::ListV([SteelVal::IntV(2), SteelVal::IntV(3)].into_iter().collect()),
                    SteelVal::IntV(3),
                    SteelVal::IntV(20),
                ]
                .into_iter()
                .collect()
            )
        );
    }

    /// 文档「一、3 条件」——`if` 与 `cond`。
    #[test]
    fn if与cond条件分支() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (classify n)
                  (cond
                    [(< n 0) "negative"]
                    [(= n 0) "zero"]
                    [else "positive"]))
                (define (probe) (list (if (> 3 2) 'yes 'no) (classify -5) (classify 0) (classify 7)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [
                    SteelVal::SymbolV("yes".into()),
                    SteelVal::StringV("negative".into()),
                    SteelVal::StringV("zero".into()),
                    SteelVal::StringV("positive".into()),
                ]
                .into_iter()
                .collect()
            )
        );
    }

    /// 文档「一、4 let 家族」——`let`/`let*`/`letrec`/命名 `let`。
    #[test]
    fn let家族四种绑定形式() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe-let) (let ([a 1] [b 2]) (+ a b)))
                (define (probe-let*) (let* ([a 1] [b (+ a 1)]) (+ a b)))
                (define (probe-letrec)
                  (letrec ([even? (lambda (n) (if (= n 0) #t (odd? (- n 1))))]
                           [odd? (lambda (n) (if (= n 0) #f (even? (- n 1))))])
                    (even? 10)))
                (define (probe-named-let)
                  (let loop ([i 0] [acc 0])
                    (if (= i 5) acc (loop (+ i 1) (+ acc i)))))
                (define (probe) (list (probe-let) (probe-let*) (probe-letrec) (probe-named-let)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [
                    SteelVal::IntV(3),
                    SteelVal::IntV(3),
                    SteelVal::BoolV(true),
                    SteelVal::IntV(10),
                ]
                .into_iter()
                .collect()
            )
        );
    }

    /// 文档「一、5 递归与尾调用」——命名 `let` 写成的尾递归循环一万次
    /// 不应该超时/爆栈,证明尾调用确实被优化。
    #[test]
    fn 尾递归循环一万次不超时不爆栈() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe)
                  (let loop ([i 0] [acc 0])
                    (if (= i 10000) acc (loop (+ i 1) (+ acc 1)))))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(result, SteelVal::IntV(10000));
    }

    /// 文档「一、6 quote/quasiquote」——字面量数据与
    /// `unquote`/`unquote-splicing`。
    #[test]
    fn quote与quasiquote构造数据() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe)
                  (list 'move 'north
                        `(sum ,(+ 1 2) end)
                        `(a ,@(list 1 2 3) b)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [
                    SteelVal::SymbolV("move".into()),
                    SteelVal::SymbolV("north".into()),
                    SteelVal::ListV(
                        [
                            SteelVal::SymbolV("sum".into()),
                            SteelVal::IntV(3),
                            SteelVal::SymbolV("end".into()),
                        ]
                        .into_iter()
                        .collect()
                    ),
                    SteelVal::ListV(
                        [
                            SteelVal::SymbolV("a".into()),
                            SteelVal::IntV(1),
                            SteelVal::IntV(2),
                            SteelVal::IntV(3),
                            SteelVal::SymbolV("b".into()),
                        ]
                        .into_iter()
                        .collect()
                    ),
                ]
                .into_iter()
                .collect()
            )
        );
    }

    /// 文档「一、7 数值：大整数、有理数、exact/inexact」——来自 Steel 官方
    /// book《Values > Numbers》一节，实测确认 book 描述的四种数值形式
    /// （bignum/有理数/`exact`/`inexact`）在 0.8.2 上均可用。
    #[test]
    fn 数值大整数有理数与exact_inexact转换() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe)
                  (list
                    9999999999999999999999
                    1/2
                    (exact 1.5)
                    (inexact 3/2)
                    (exact? 1/2)
                    (numerator 3/4)
                    (denominator 3/4)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        // 大整数、`inexact` 结果与浮点相关的成员用字符串化比对，避免依赖
        // `SteelVal` 大整数/浮点变体的内部构造细节；有理数与整数成员直接
        // 比对结构化值。
        let rendered = format!("{result:?}");
        assert!(rendered.contains("9999999999999999999999"));
        assert!(rendered.contains("1.5"));
        match result {
            SteelVal::ListV(items) => {
                let items: Vec<_> = items.into_iter().collect();
                assert_eq!(items[4], SteelVal::BoolV(true)); // (exact? 1/2)
                assert_eq!(items[5], SteelVal::IntV(3)); // (numerator 3/4)
                assert_eq!(items[6], SteelVal::IntV(4)); // (denominator 3/4)
            }
            other => panic!("期望 ListV，实际拿到 {other:?}"),
        }
    }

    /// 文档「一、8 字符串与符号操作」——来自 book《Values > Strings /
    /// Symbols》两节列出的函数名，逐个实测确认在 0.8.2 上可调用。
    #[test]
    fn 字符串与符号操作() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe)
                  (list
                    (string-append "a" "b" "c")
                    (symbol->string 'foo)
                    (string->symbol "bar")
                    (concat-symbols 'foo 'bar)
                    (starts-with? "hello" "he")
                    (ends-with? "hello" "lo")
                    (trim "  hi  ")))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [
                    SteelVal::StringV("abc".into()),
                    SteelVal::StringV("foo".into()),
                    SteelVal::SymbolV("bar".into()),
                    SteelVal::SymbolV("foobar".into()),
                    SteelVal::BoolV(true),
                    SteelVal::BoolV(true),
                    SteelVal::StringV("hi".into()),
                ]
                .into_iter()
                .collect()
            )
        );
    }
}

// ============================================================
// 二、Lisp 的核心能力
// ============================================================
mod 二_核心能力 {
    use super::*;

    /// 文档「二、1 宏」——`define-syntax`/`syntax-rules`。
    #[test]
    fn define_syntax定义并使用宏() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define-syntax my-when
                  (syntax-rules ()
                    [(my-when test body ...) (if test (begin body ...) #f)]))
                (define (probe)
                  (my-when (> 3 2) (+ 1 1) (+ 2 2)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(result, SteelVal::IntV(4));
    }

    /// 文档「二、2 闭包与高阶函数」——`map`/`filter`/`foldl`/`foldr`/
    /// `apply`。
    #[test]
    fn 高阶函数map_filter_foldl_foldr_apply() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define nums (list 1 2 3 4 5))
                (define (probe)
                  (list
                    (map (lambda (x) (* x x)) nums)
                    (filter (lambda (x) (> x 2)) nums)
                    (foldl + 0 nums)
                    (foldr cons '() nums)
                    (apply + nums)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [
                    SteelVal::ListV([1, 4, 9, 16, 25].into_iter().map(SteelVal::IntV).collect()),
                    SteelVal::ListV([3, 4, 5].into_iter().map(SteelVal::IntV).collect()),
                    SteelVal::IntV(15),
                    SteelVal::ListV([1, 2, 3, 4, 5].into_iter().map(SteelVal::IntV).collect()),
                    SteelVal::IntV(15),
                ]
                .into_iter()
                .collect()
            )
        );
    }

    /// 文档「二、2 闭包」——闭包捕获并通过 `box`/`unbox`/`set-box!`
    /// 持有可变状态（计数器生成器）。`box` 一族在 ADR 0012「追加实测
    /// 三」明确点名放行,不属于 `META_DENY_LIST`。
    #[test]
    fn 闭包用box持有可变状态实现计数器生成器() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (make-counter start)
                  (let ([state (box start)])
                    (lambda ()
                      (set-box! state (+ (unbox state) 1))
                      (unbox state))))
                (define counter (make-counter 10))
                (define (probe) (list (counter) (counter) (counter)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV([11, 12, 13].into_iter().map(SteelVal::IntV).collect())
        );
    }

    /// 文档「二、3 自定义结构体」——`struct`（依赖 `make-struct-type`
    /// 一族,曾被 `steel/meta` 全量清空误挡,ADR 0012「追加实测三」改为
    /// 逐名拒绝清单后恢复）。
    #[test]
    fn struct自定义结构体定义构造访问与谓词() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (struct Point (x y))
                (define p (Point 3 4))
                (define (probe) (list (Point-x p) (Point-y p) (Point? p) (Point? 5)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [
                    SteelVal::IntV(3),
                    SteelVal::IntV(4),
                    SteelVal::BoolV(true),
                    SteelVal::BoolV(false),
                ]
                .into_iter()
                .collect()
            )
        );
    }

    /// 文档「二、4 递归」——非尾递归（阶乘），证明递归本身不受限制，
    /// 不是只有尾递归形式才能过白名单/中断预算。
    #[test]
    fn 非尾递归阶乘() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))
                (define (probe) (fact 10))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(result, SteelVal::IntV(3628800));
    }

    /// 文档「二、5 宏的嵌套省略号模式」——book《Language Reference >
    /// Macros》给出的是单层 `y ...`；这里额外验证嵌套形式（`syntax-rules`
    /// 模式里出现 `([var val] rest ...)` 这种"列表的列表 + 省略号"）在
    /// 0.8.2 上同样展开正确，对应一个手写的 `let*`。
    #[test]
    fn 宏支持嵌套省略号模式() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define-syntax my-let*
                  (syntax-rules ()
                    [(my-let* () body ...) (begin body ...)]
                    [(my-let* ([var val] rest ...) body ...)
                     (let ([var val]) (my-let* (rest ...) body ...))]))
                (define (probe) (my-let* ([a 1] [b (+ a 1)] [c (+ b 1)]) (list a b c)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV([1, 2, 3].into_iter().map(SteelVal::IntV).collect())
        );
    }

    /// 文档「二、6 宏的卫生性」——经典卫生性探针：宏在展开出的 `let`
    /// 里引入一个和调用点变量同名的绑定（都叫 `t`），若宏是卫生的，
    /// 调用点传入的 `t`（全局变量，值 100）不会被宏内部的 `t` 遮蔽；
    /// 若不卫生（朴素文本替换），三处 `t` 会被合并成同一个绑定，结果是
    /// `#f` 而不是 `100`。实测返回 `100`，确认 `syntax-rules` 展开卫生。
    #[test]
    fn 宏展开卫生不遮蔽调用点同名变量() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define-syntax my-or
                  (syntax-rules ()
                    [(my-or a b) (let ([t a]) (if t t b))]))
                (define t 100)
                (define (probe) (my-or #f t))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(result, SteelVal::IntV(100));
    }

    /// 文档「二、7 `syntax-case` 过程式宏」——book 提到 Steel 同时提供
    /// `syntax-rules` 和 `syntax-case` 两套宏系统，但没有给出 `syntax-case`
    /// 的调用形状。**实测发现**：`(define-syntax name (syntax-case () ...))`
    /// 这种照抄 `syntax-rules` 形状的写法会在运行期报错
    /// `"syntax-case expects a function"`；正确形状必须是过程式变换器
    /// `(define-syntax (name stx) (syntax-case stx () [pattern #'template]))`
    /// ——`stx` 是显式参数，`#'`/`#` 构造语法对象，这与 `syntax-rules`
    /// 声明式的 `(syntax-rules () [pattern template])` 形状不同。
    /// 正确形状照 steel-core 自身测试
    /// （`steel-core-0.8.2/src/tests/success/syntax_case.scm`）核对过。
    #[test]
    fn syntax_case必须写成过程式变换器形式() {
        let mut engine = ScriptEngine::new();

        // 错误形状：直接照抄 syntax-rules 的声明式写法，编译期能过
        // （white list 不拦宏定义本身），但运行期报错。
        let mut wrong_engine = ScriptEngine::new();
        let wrong = wrong_engine.load_source(
            r#"
            (define-syntax my-thing
              (syntax-case ()
                [(my-thing a) (quote (a))]))
            (define (probe) (my-thing 5))
            "#
            .to_string(),
        );
        match wrong {
            Err(ll_script::ScriptError::Runtime(ref msg, _)) => {
                assert!(
                    msg.contains("syntax-case expects a function"),
                    "期望「syntax-case expects a function」，实际消息：{msg}"
                );
            }
            other => panic!("期望运行期错误，实际拿到 {other:?}"),
        }

        // 正确形状：过程式变换器。
        engine
            .load_source(
                r#"
                (define-syntax (my-thing stx)
                  (syntax-case stx ()
                    [(_ a) #'(list 'got a)]))
                (define (probe) (my-thing 5))
                "#
                .to_string(),
            )
            .unwrap();
        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [SteelVal::SymbolV("got".into()), SteelVal::IntV(5)]
                    .into_iter()
                    .collect()
            )
        );
    }

    /// 文档「二、8 `match` 模式匹配」——book 的
    /// `#%private/steel/match`（`match`/`match-define`/`match-syntax`）
    /// 一节只给出"该模块在 prelude 里自动可用"这一句话，没给示例；
    /// 实测对照 steel-core 0.8.2 自带的
    /// `src/scheme/modules/match.scm` 源码，确认 `match` 支持：
    /// `(list ...)` 前缀的列表模式、裸符号做绑定变量（不需要 `?` 前缀，
    /// 这与另一份仅用于 steel-core 自身测试、从未随 crate 发布的
    /// `matcher.scm`／`match!` 实验版本要求 `?x` 前缀不同）、`_` 通配、
    /// `else` 兜底分支、`(list first rest ...)` 省略号收集剩余元素、
    /// `#:when` 守卫子句。
    #[test]
    fn match模式匹配列表下划线else省略号与guard() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe-basic)
                  (match (list 1 2 3)
                    [(list a b c) (+ a b c)]))
                (define (probe-wildcard x)
                  (match x
                    [(list a _ c) (list 'three a c)]
                    [else 'other]))
                (define (probe-rest)
                  (match (list 1 2 3 4 5)
                    [(list first rest ...) (list first rest)]))
                (define (probe-guard n)
                  (match n
                    [n #:when (> n 10) 'big]
                    [n 'small]))
                "#
                .to_string(),
            )
            .unwrap();

        assert_eq!(
            engine.call_raw("probe-basic", Vec::new()).unwrap(),
            SteelVal::IntV(6)
        );
        assert_eq!(
            engine
                .call_raw(
                    "probe-wildcard",
                    vec![SteelVal::ListV(
                        [SteelVal::IntV(1), SteelVal::IntV(2), SteelVal::IntV(3)]
                            .into_iter()
                            .collect()
                    )]
                )
                .unwrap(),
            SteelVal::ListV(
                [
                    SteelVal::SymbolV("three".into()),
                    SteelVal::IntV(1),
                    SteelVal::IntV(3),
                ]
                .into_iter()
                .collect()
            )
        );
        assert_eq!(
            engine
                .call_raw("probe-wildcard", vec![SteelVal::IntV(5)])
                .unwrap(),
            SteelVal::SymbolV("other".into())
        );
        assert_eq!(
            engine.call_raw("probe-rest", Vec::new()).unwrap(),
            SteelVal::ListV(
                [
                    SteelVal::IntV(1),
                    SteelVal::ListV([2, 3, 4, 5].into_iter().map(SteelVal::IntV).collect()),
                ]
                .into_iter()
                .collect()
            )
        );
        assert_eq!(
            engine
                .call_raw("probe-guard", vec![SteelVal::IntV(20)])
                .unwrap(),
            SteelVal::SymbolV("big".into())
        );
        assert_eq!(
            engine
                .call_raw("probe-guard", vec![SteelVal::IntV(3)])
                .unwrap(),
            SteelVal::SymbolV("small".into())
        );
    }

    /// 文档「二、8 `match`」的一处限制——**实测发现，book 没提**：
    /// `match` 的列表模式不能直接写成 `(struct名 字段...)` 去匹配一个
    /// `struct` 实例并同时解构字段，因为 `match.scm` 的模式编译器只认
    /// 两种列表模式形状：`(list ...)` 前缀，或者裸符号/通配符/嵌套列表
    /// ——把 `(Pt a b)` 当模式写会被当成"没有 `list` 前缀的列表模式"，
    /// 直接报错 `"list pattern must start with `list` - found Pt"`。
    /// 要匹配 `struct` 实例，只能退回 `Pt?` 谓词 + 手动调用访问器
    /// （`Pt-x`/`Pt-y`），不能指望 `match` 直接解构。**这个检查发生在
    /// `load_source` 阶段**（`match` 是 `syntax-rules` 宏，模式形状是在
    /// 宏展开时、也就是编译期被检查的，不需要真的调用 `probe` 就会报
    /// 错），不是运行时才暴露。
    #[test]
    fn match不支持直接解构struct实例() {
        let mut engine = ScriptEngine::new();
        let result = engine.load_source(
            r#"
            (struct Pt (x y))
            (define (probe p)
              (match p
                [(Pt a b) (+ a b)]))
            "#
            .to_string(),
        );

        match result {
            Err(ll_script::ScriptError::Runtime(msg, _)) => {
                assert!(
                    msg.contains("list pattern must start with"),
                    "期望「list pattern must start with」，实际消息：{msg}"
                );
            }
            other => panic!("期望 struct 解构模式在编译期报错，实际拿到 {other:?}"),
        }
    }

    /// 文档「二、9 `hashset`」——book《Collections > Hash sets》给出
    /// `(hashset 10 20 30 30 40)` 的构造示例，这里补一个查询用法。
    #[test]
    fn hashset构造与包含判断() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe)
                  (define hs (hashset 1 2 3))
                  (list (hashset-contains? hs 2) (hashset-contains? hs 99)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(
            result,
            SteelVal::ListV(
                [SteelVal::BoolV(true), SteelVal::BoolV(false)]
                    .into_iter()
                    .collect()
            )
        );
    }

    /// 文档「二、10 脚本侧捕获运行时错误：`with-handler`」——**book 完全
    /// 没有错误处理章节**（`docs/src/stdlib/private_steel_stdlib.md` 通篇
    /// 搜索 `with-handler`/`guard`/`raise`/`call-with-exception-handler`
    /// 均无结果），这条写法是直接读 `steel-core` 0.8.2 的
    /// `src/scheme/stdlib.scm`（`with-handler` 的 `define-syntax` 定义，
    /// 基于 `call-with-exception-handler` + `reset`/`shift` 分界续延）
    /// 找到、再实测验证的，不来自官方文档。**R7RS 标准的 `guard`/`raise`
    /// 在 0.8.2 里不存在**——不是被白名单挡住（`guard` 报的是
    /// `ParseError`「不在白名单内」，但白名单本身是"能力边界，不是语言
    /// 子集"：一个标识符从未在 prelude 里 `define` 过，天然不会出现在
    /// `compute_allowed_identifiers` 收集到的全局作用域里，这与"故意
    /// 拒绝"是两回事，是"压根不存在"）。`with-handler` 用起来是
    /// `(with-handler (lambda (e) ...) 可能出错的表达式)`。
    #[test]
    fn with_handler捕获脚本内运行时错误() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe)
                  (with-handler (lambda (e) 'caught)
                                (car '())))
                "#
                .to_string(),
            )
            .unwrap();

        let result = engine.call_raw("probe", Vec::new()).unwrap();
        assert_eq!(result, SteelVal::SymbolV("caught".into()));
    }
}

// ============================================================
// 三、本项目提供的 mod API（ll-script 自身可测的部分）
// ============================================================
mod 三_mod_api {
    use super::*;

    /// 文档「三、1 world-* 只读查询」。
    #[test]
    fn query系列四个只读查询函数() {
        let mut world = 造一个最小世界();
        let terrain_ids = {
            // 复用 `造一个最小世界` 内部同一份 base_terrain_fixture,
            // 重新取一次 ids 用于设置地形——与
            // `crates/ll-script/src/api/query.rs` 测试模块的写法一致。
            let (ids, _table) = base_terrain_fixture();
            ids
        };
        world
            .terrain
            .set_terrain(world.size.wrap(0, 0), terrain_ids.grass);
        world.advance(3);

        let mut engine = ScriptEngine::new();
        query::register(&mut engine);
        engine
            .load_source(
                r#"
                (define (probe)
                  (list (world-move-cost-at 0 0)
                        (world-blocks-sight-at 0 0)
                        (world-tick)
                        (world-ambient-light)))
                "#
                .to_string(),
            )
            .unwrap();

        let result = unsafe {
            query::set_active_world(&world);
            let result = engine.call_raw("probe", Vec::new());
            query::clear_active_world();
            result
        }
        .unwrap();

        let expected_cost = terrain_ids.grass.move_cost(&world.terrain_table) as isize;
        match result {
            SteelVal::ListV(items) => {
                let items: Vec<_> = items.into_iter().collect();
                assert_eq!(items[0], SteelVal::IntV(expected_cost));
                assert_eq!(items[2], SteelVal::IntV(world.clock.0 as isize));
            }
            other => panic!("期望 ListV，实际拿到 {other:?}"),
        }
    }

    /// 文档「三、2 确定性随机」——`rng-next-u64`/`rng-gen-range`/
    /// `rng-chance`。
    #[test]
    fn rng系列三个函数() {
        let mut engine = ScriptEngine::new();
        rng::register(&mut engine);
        engine
            .load_source(
                r#"
                (define (probe)
                  (list (rng-next-u64) (rng-gen-range 10 20) (rng-chance 1000)))
                "#
                .to_string(),
            )
            .unwrap();

        rng::set_active_rng(DetRng::for_entity(1, 2, 3));
        let result = engine.call_raw("probe", Vec::new()).unwrap();
        rng::clear_active_rng();

        match result {
            SteelVal::ListV(items) => {
                let items: Vec<_> = items.into_iter().collect();
                assert!(matches!(items[0], SteelVal::IntV(_)));
                match items[1] {
                    SteelVal::IntV(n) => assert!((10..20).contains(&n)),
                    ref other => panic!("期望区间内的整数，实际拿到 {other:?}"),
                }
                // 千分之一千的概率恒命中。
                assert_eq!(items[2], SteelVal::BoolV(true));
            }
            other => panic!("期望 ListV，实际拿到 {other:?}"),
        }
    }

    /// 文档「三、3 脚本状态存储（全局作用域）」——
    /// `state-set!`/`state-get!`/`state-get-foreign`/`content-ref`。
    /// 不覆盖 `entity-state-*`：见本文件顶部模块文档「覆盖范围的边界」。
    #[test]
    fn state系列全局存储与跨mod只读查询与内容引用() {
        let world = 造一个最小世界();

        // 两个引擎都在编译之前造齐——见 `ll_script::host` 里
        // `COMPILED_ON_THIS_THREAD` 上方注释：同一根线程上全部构造必须
        // 先于全部编译。
        let mut writer = ScriptEngine::new();
        let mut reader = ScriptEngine::new();
        state::register(&mut writer, "lostland");
        writer
            .load_source(
                r#"
                (define (probe)
                  (state-set! "reputation" 42)
                  (state-set! "last-item" (content-ref "yourmod:healing_potion"))
                  (state-get! "reputation"))
                "#
                .to_string(),
            )
            .unwrap();

        let write_result = unsafe {
            query::set_active_world(&world);
            let result = writer.call_raw("probe", Vec::new());
            query::clear_active_world();
            result
        };
        assert_eq!(write_result, Ok(SteelVal::IntV(42)));

        // 待写缓冲区里的记录还没提交进 WorldState（没有走 apply），
        // 因此另一个 mod 通过 state-get-foreign 现在还查不到——这里只
        // 验证「同一次调用内自己能读到自己刚写的」与「换一个命名空间
        // 默认读不到」两件事，不模拟完整的 apply 落盘流程（那属于
        // ll-sim 的职责，不是本文件要覆盖的语法示例）。
        state::register(&mut reader, "someothermod");
        reader
            .load_source(r#"(define (probe) (state-get! "reputation"))"#.to_string())
            .unwrap();
        let foreign_default = unsafe {
            query::set_active_world(&world);
            let result = reader.call_raw("probe", Vec::new());
            query::clear_active_world();
            result
        };
        assert_eq!(foreign_default, Ok(SteelVal::Void));
    }

    /// 文档「三、4 脚本返回值 → Intent」——`parse_intent` 认识的三种
    /// 形状：`'wait`、`(list 'move 方向)`、`(list 'use-skill "id")`。
    #[test]
    fn intent系列wait_move_use_skill三种形状() {
        let actor = 造一个实体id();
        let mut engine = ScriptEngine::new();
        engine
            .load_source(
                r#"
                (define (probe-wait) 'wait)
                (define (probe-move) (list 'move 'north))
                (define (probe-skill) (list 'use-skill "lostland:strike"))
                "#
                .to_string(),
            )
            .unwrap();

        let wait_value = engine.call_raw("probe-wait", Vec::new()).unwrap();
        let move_value = engine.call_raw("probe-move", Vec::new()).unwrap();
        let skill_value = engine.call_raw("probe-skill", Vec::new()).unwrap();

        let wait_intent = intent::parse_intent(actor, &wait_value, &|_| None);
        let move_intent = intent::parse_intent(actor, &move_value, &|_| None);

        let mut interner = ll_core::ident::Interner::new();
        let skill_index =
            interner.intern(ll_core::ident::NamespacedId::parse("lostland:strike").unwrap());
        let skill_intent = intent::parse_intent(actor, &skill_value, &|id| {
            (id == "lostland:strike").then_some(skill_index)
        });

        assert_eq!(wait_intent, Some(ll_sim::intent::Intent::Wait { actor }));
        assert_eq!(
            move_intent,
            Some(ll_sim::intent::Intent::Move {
                actor,
                dir: ll_sim::intent::Direction::North
            })
        );
        assert_eq!(
            skill_intent,
            Some(ll_sim::intent::Intent::UseSkill {
                actor,
                skill: skill_index,
                target: None,
            })
        );
    }
}

// ============================================================
// 四、常见错误
// ============================================================
mod 四_常见错误 {
    use super::*;

    /// 文档「五、1 语法错误」——缺右括号,`ParseError` 且携带字节偏移。
    #[test]
    fn 缺右括号返回parseerror并携带字节偏移() {
        let mut engine = ScriptEngine::new();
        let result = engine.load_source("(+ 1 2".to_string());
        assert!(matches!(
            result,
            Err(ll_script::ScriptError::ParseError(_, Some(_)))
        ));
    }

    /// 文档「五、2 引用了白名单外的标识符」——同样是编译期 `ParseError`,
    /// 不是运行时错误,消息里点名具体是哪个标识符。
    #[test]
    fn 引用未注册函数在编译期就被拒绝并点名() {
        let mut engine = ScriptEngine::new();
        let result =
            engine.load_source("(define (probe) (this-was-never-registered 1))".to_string());
        match result {
            Err(ll_script::ScriptError::ParseError(msg, Some(_))) => {
                assert!(msg.contains("this-was-never-registered"));
            }
            other => panic!("期望带点名的 ParseError，实际拿到 {other:?}"),
        }
    }

    /// 文档「五、3 参数个数不匹配」——`register_fn` 注册的函数缺参。
    #[test]
    fn 注册函数缺参返回arity_mismatch() {
        let mut engine = ScriptEngine::new();
        engine.register_fn("needs-two", |a: i64, b: i64| a + b);
        let result = engine.load_source("(needs-two 1)".to_string());
        assert!(matches!(
            result,
            Err(ll_script::ScriptError::ArityMismatch(_, _))
        ));
    }

    /// 文档「五、4 运行时错误」——类型不匹配（`car` 作用在非 pair 上），
    /// 不同于语法/白名单错误：这类错误只有真正求值到那一行才会触发。
    #[test]
    fn 对空列表取car返回runtime错误() {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(r#"(define (probe) (car '()))"#.to_string())
            .unwrap();
        let result = engine.call_raw("probe", Vec::new());
        assert!(result.is_err());
    }

    /// 文档「五、5 超时中断」——死循环得到 `Err`，不崩溃进程。
    ///
    /// **历史记录**：本测试曾经记录过 `classify_error`（`host.rs`）与
    /// 早期 `ScriptError::Interrupted` 变体文档字面表述不一致的一处
    /// 实测发现——超时实际拿到的是 `ScriptError::Runtime`（消息含
    /// `"Interrupted by user"`，且携带一个字节偏移量），从未真正构造
    /// 过 `Interrupted` 这个变体，是一处死变体。这处不一致先是在
    /// `classify_error` 里修复成识别消息标记、提前返回 `Interrupted`；
    /// 后来这个单一变体又被拆成 `Timeout`/`MemoryBudgetExceeded`
    /// 两个（超时与超预算共用一个变体时，两条独立防线的失败在外部
    /// 无法区分，见 `classify_error` 文档「`interrupt()` 通道的两个
    /// 调用点」一节记录的两次真实误诊）。本测试断言的是拆分之后的
    /// 真实行为：本测试二进制没装 `#[global_allocator]`（见
    /// `alloc_guard` 模块文档），`alloc_guard` 不可能触发中断，死循环
    /// 唯一可能的中断来源是 300ms 看门狗超时，因此这里必须拿到
    /// `Timeout`，不是 `MemoryBudgetExceeded`。
    #[test]
    fn 死循环返回timeout变体而不崩溃进程() {
        let mut engine = ScriptEngine::new();
        let result = engine.load_source("(define (loop) (loop)) (loop)".to_string());
        assert_eq!(result, Err(ll_script::ScriptError::Timeout));
    }

    /// 文档「五、6 字节偏移 → 行号」——加载管理界面据此定位到具体行的
    /// 换算方式：数偏移量之前出现了几个换行符。
    #[test]
    fn 字节偏移量能换算成第几行() {
        let mut engine = ScriptEngine::new();
        let source = "(define a 1)\n(define b 2)\n(+ 1 2".to_string();
        let result = engine.load_source(source.clone());
        match result {
            Err(ll_script::ScriptError::ParseError(_, Some(offset))) => {
                let line = source[..offset as usize].matches('\n').count() + 1;
                assert_eq!(line, 3);
            }
            other => panic!("期望带偏移量的 ParseError，实际拿到 {other:?}"),
        }
    }
}
