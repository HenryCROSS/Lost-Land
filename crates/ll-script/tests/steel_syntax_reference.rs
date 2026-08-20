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

        let mut writer = ScriptEngine::new();
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
        let mut reader = ScriptEngine::new();
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
    /// **已修复，历史记录见下**：本测试曾经记录过 `classify_error`
    /// （`host.rs`）与 `ScriptError::Interrupted` 变体文档字面表述不一致
    /// 的一处实测发现——超时实际拿到的是 `ScriptError::Runtime`（消息
    /// 含 `"Interrupted by user"`，且携带一个字节偏移量），从未真正
    /// 构造过 `Interrupted` 这个变体，是一处死变体。这处不一致已经
    /// 在 `classify_error` 里修复（识别消息里的 `"Interrupted by user"`
    /// 标记，提前返回 `Interrupted`，见其文档「为什么按消息文本而不是
    /// `ErrorKind` 判断超时中断」一节的完整论证），本测试同步更新为
    /// 断言修复后的真实行为，不再断言"与文档不符"这个已经不成立的
    /// 事实。
    #[test]
    fn 死循环返回interrupted变体而不崩溃进程() {
        let mut engine = ScriptEngine::new();
        let result = engine.load_source("(define (loop) (loop)) (loop)".to_string());
        assert_eq!(result, Err(ll_script::ScriptError::Interrupted));
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
