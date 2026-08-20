//! 行为树 tick 求值器（规格 §10.5，[ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)）。
//!
//! # 落地规格 §10.5 原文
//!
//! > 行为树定义直接书写为 Steel `.scm`——**S 表达式本身即树结构**，
//! > 无需发明第三种格式，也无需编写树解析器。Rust 侧仅实现 tick
//! > 求值器，节点判断与动作以 Steel 函数实现。
//!
//! 示例（规格原文）：
//!
//! ```scheme
//! (selector
//!   (sequence (hp-below? 0.3) (flee-from-nearest-enemy))
//!   (sequence (has-order? 'guard) (attack-anyone-near (order-target)))
//!   (follow-leader 3))
//! ```
//!
//! 本模块实现的正是「仅 tick 求值器」这一半：[`tick`] 拿到脚本某个零参
//! 函数返回的一份**未被求值**的列表（`selector`/`sequence` 这两个符号
//! 从未注册给 Steel，若把它们当普通代码求值会在装载期就被白名单拒绝
//! ——mod 作者必须用 `quote`/`'` 把整棵树包成数据，见
//! `mods/example_mod/behavior.scm`），按 `selector`/`sequence` 语义
//! 遍历这份数据，遇到其余任意符号就当作一次真正的 Steel 函数调用
//! （节点判断/动作），转发给 [`crate::host::ScriptEngine::call_raw`]。
//!
//! # 为什么遍历算法在这里、不在 `ll-sim`
//!
//! [ADR 0018] 判定「行为树 tick 求值器（Rust 侧遍历 selector/sequence
//! 节点）」是引擎层——遍历算法本身没有设计自由度（自由度落在节点
//! 判断/动作这些 Steel 函数里，不在怎么遍历上）。这个判据回答的是
//! 「这段逻辑该不该暴露给 mod 重新定义」（不该），不是「这段代码该编译
//! 进哪个 Rust crate」。后一个问题纯粹是依赖方向的物理约束：本函数要
//! 读写 `steel::rvals::SteelVal`（`steel-core` 的类型），`ll-sim` 不
//! 依赖 `steel-core`（依赖方向 `ll-sim` ← `ll-script`，规格 §5），因此
//! 求值器只能落在本 crate，由本 crate 产出的
//! [`crate::api::intent::parse_intent`] 再把结果翻译成 `ll-sim` 认识的
//! [`ll_sim::intent::Intent`]。真正体现「依赖倒置」的是
//! `ll_sim::behavior::BehaviorTreeSource`——见其模块文档。
//!
//! # 节点结果的表示：`#f` 即失败，其余一律成功并携带自己的值
//!
//! 不单独区分「条件节点」与「动作节点」两种语法——两者在数据形状上
//! 完全相同（都是 `(名字 参数...)` 的列表），区别只在语义上：`selector`
//! 用返回值是否为 `#f` 判断要不要试下一条分支，`sequence` 用同一个
//! 判断决定要不要继续；一条 `sequence` 链最终产出的值就是它最后一个
//! 子节点的返回值——这正是规格示例里
//! `(sequence (hp-below? 0.3) (flee-from-nearest-enemy))` 的读法：第一
//! 个子节点是条件（成功时返回非 `#f`，本模块不关心具体是什么），第二
//! 个子节点是真正的动作，它的返回值（预期是
//! `api::intent::parse_intent` 认识的某种 Intent 形状）才是整条分支
//! 冒泡上去的结果。
//!
//! # 降级而非崩溃（规格 §10.2 第二道防线）
//!
//! 任一叶子调用失败（[`crate::host::ScriptError`]）都按「这个节点
//! 失败」处理（等价于该节点返回 `#f`），不会让 [`tick`] 的调用方看到
//! `Err`——AI 算不出这一步该干什么，原地不动，是既有纪律（`四道防线②`）
//! 在行为树上的直接延伸。
//!
//! # 与约束 C1（VM 不持有隐式跨帧状态）的关系
//!
//! 本求值器是**无状态的**：每次 [`tick`] 调用都从 `tree_entry_fn` 重新
//! 取一遍树、从根节点重新遍历一遍——不存在「上次跑到哪个节点」这种
//! 需要跨调用记住的东西，因此不需要为「当前节点」发明任何持久化存储
//! （规格 §10.5「行为树运行时状态（当前节点、黑板内容）依约束 C1 存放
//! 于 `WorldState`」描述的是可能需要多个 tick 才跑完一个「运行中」
//! 动作节点的更复杂行为树；本次落地的行为树在一次 `tick` 内就能跑到
//! 一个终止的叶子，不产生这种「运行中」状态，因此暂不需要这条存储
//! 路径——若未来的行为树需要跨 tick 记住「上次跑到哪」，应经
//! `api::state` 的 `entity-state-set!` 显式落地，不能留在 VM 内存里，
//! 与约束 C1 的既有纪律完全一致，只是本批次没有用到）。真正需要跨
//! tick 记忆的是「黑板」（例如玩家下达的战术指令），这条路径已经由
//! `api::state` 的 `entity-state-get!`/`entity-state-set!` 提供，脚本
//! 自己的叶子函数可以按需读写，与本求值器是否有状态无关。

use steel::rvals::SteelVal;

use crate::host::ScriptEngine;

/// 分支择一：试各子节点，返回第一个非 `#f` 的结果；全部失败则 `#f`。
const SELECTOR: &str = "selector";
/// 顺序执行：任一子节点失败（`#f`）整体失败；全部成功时返回最后一个
/// 子节点的结果。
const SEQUENCE: &str = "sequence";
/// 标准 Scheme `quote`——树是一份被 `'`/`(quote ...)` 包起来的字面数据，
/// reader 会把内部出现的 `'sym` 展开成 `(quote sym)`，遍历到这个形状
/// 时必须原样交回内部数据，不能当成对函数 `quote` 的调用转发出去
/// （从未注册这样一个函数，转发只会得到 `ArityMismatch`/自由标识符
/// 错误）。
const QUOTE: &str = "quote";

/// 对 `tree_entry_fn`（脚本里一个零参函数，调用它取到这次 tick 要遍历
/// 的树——这个函数每次调用都可以现算返回不同的树，例如按黑板内容选
/// 分支，也可以像示例 mod 那样恒定返回同一棵树，本函数不关心树是怎么
/// 来的，只管遍历它）跑一次 tick。
///
/// 返回值是整棵树最终生效的叶子节点的返回值（预期是
/// [`crate::api::intent::parse_intent`] 认识的某种 Intent 形状）；
/// `tree_entry_fn` 调用失败，或整棵树的每一条分支都失败（`#f`），都
/// 返回 `None`——两种情形对调用方而言是同一件事：这次没有算出任何
/// 决策。
pub fn tick(engine: &mut ScriptEngine, tree_entry_fn: &str) -> Option<SteelVal> {
    let tree = engine.call_raw(tree_entry_fn, Vec::new()).ok()?;
    let result = tick_control(engine, &tree);
    if is_false(&result) {
        None
    } else {
        Some(result)
    }
}

fn is_false(value: &SteelVal) -> bool {
    matches!(value, SteelVal::BoolV(false))
}

fn symbol_str(value: &SteelVal) -> Option<&str> {
    match value {
        SteelVal::SymbolV(s) => Some(s.as_str()),
        _ => None,
    }
}

/// 按 `selector`/`sequence` 语义遍历一个控制节点；非列表值（不该正常
/// 出现在控制层，但防御性地）原样返回，不 panic。
fn tick_control(engine: &mut ScriptEngine, node: &SteelVal) -> SteelVal {
    let SteelVal::ListV(list) = node else {
        return node.clone();
    };
    let mut iter = list.iter();
    let Some(head) = iter.next().and_then(symbol_str) else {
        return SteelVal::BoolV(false);
    };
    match head {
        SELECTOR => {
            for child in iter {
                let result = tick_control(engine, child);
                if !is_false(&result) {
                    return result;
                }
            }
            SteelVal::BoolV(false)
        }
        SEQUENCE => {
            let mut last = SteelVal::BoolV(false);
            let mut ran_any = false;
            for child in iter {
                ran_any = true;
                let result = tick_control(engine, child);
                if is_false(&result) {
                    return SteelVal::BoolV(false);
                }
                last = result;
            }
            if ran_any {
                last
            } else {
                // 空 sequence——没有子节点可执行，视为失败而不是凭空
                // 成功：与「没有产出任何决策」同一个哨兵值，调用方不
                // 需要额外区分「空节点」与「真的没有决策」两种情形。
                SteelVal::BoolV(false)
            }
        }
        _ => eval_call(engine, node),
    }
}

/// 把一个 `(名字 参数...)` 形状的列表当成一次真正的 Steel 函数调用：
/// 参数先各自求值（[`eval_arg`]），再转发给 `engine.call_raw`。
fn eval_call(engine: &mut ScriptEngine, node: &SteelVal) -> SteelVal {
    let SteelVal::ListV(list) = node else {
        return node.clone();
    };
    let mut iter = list.iter();
    let Some(head) = iter.next().and_then(symbol_str) else {
        return SteelVal::BoolV(false);
    };
    if head == QUOTE {
        return iter.next().cloned().unwrap_or(SteelVal::BoolV(false));
    }
    let args: Vec<SteelVal> = iter.map(|arg| eval_arg(engine, arg)).collect();
    // 降级而非崩溃（模块文档「四道防线②」）：叶子调用出错（未注册的
    // 名字、arity 不匹配、脚本内部运行时错误）一律当作这个节点失败，
    // 不把 `ScriptError` 冒泡给调用方。
    engine
        .call_raw(head, args)
        .unwrap_or(SteelVal::BoolV(false))
}

/// 求值一个叶子调用的参数：嵌套列表（例如规格示例里的
/// `(order-target)`）递归当作另一次调用求值；字面量（数字/字符串/
/// 符号/布尔……）原样返回——这是「S 表达式本身即树结构」的另一半含义：
/// 参数位置上的子表达式仍然是普通的、会被求值的 Steel 调用，只有
/// `selector`/`sequence` 包起来的控制结构才是不求值的数据。
fn eval_arg(engine: &mut ScriptEngine, node: &SteelVal) -> SteelVal {
    match node {
        SteelVal::ListV(_) => eval_call(engine, node),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_with(source: &str) -> ScriptEngine {
        let mut engine = ScriptEngine::new();
        engine
            .load_source(source.to_string())
            .expect("测试脚本源码应当能通过白名单并编译");
        engine
    }

    #[test]
    fn selector选出第一个非假分支的结果() {
        // Arrange：第一条分支条件为假,第二条为真——应当选中第二条。
        let mut engine = engine_with(
            r#"
            (define (cond-false) #f)
            (define (action-b) (list 'move 'east))
            (define (tree) (quote (selector (cond-false) (action-b))))
            "#,
        );

        // Act
        let result = tick(&mut engine, "tree");

        // Assert
        assert_eq!(
            result,
            Some(SteelVal::ListV(
                [
                    SteelVal::SymbolV("move".into()),
                    SteelVal::SymbolV("east".into()),
                ]
                .into_iter()
                .collect()
            ))
        );
    }

    #[test]
    fn selector全部分支失败时返回空() {
        // Arrange
        let mut engine = engine_with(
            r#"
            (define (always-false) #f)
            (define (tree) (quote (selector (always-false) (always-false))))
            "#,
        );

        // Act
        let result = tick(&mut engine, "tree");

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn sequence任一子节点失败则整体失败() {
        // Arrange
        let mut engine = engine_with(
            r#"
            (define (cond-true) #t)
            (define (cond-false) #f)
            (define (action) 'should-not-be-reached-as-final)
            (define (tree) (quote (sequence (cond-true) (cond-false) (action))))
            "#,
        );

        // Act
        let result = tick(&mut engine, "tree");

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn sequence全部成功时冒泡最后一个子节点的返回值() {
        // Arrange
        let mut engine = engine_with(
            r#"
            (define (cond-true) #t)
            (define (action) 'did-it)
            (define (tree) (quote (sequence (cond-true) (action))))
            "#,
        );

        // Act
        let result = tick(&mut engine, "tree");

        // Assert
        assert_eq!(result, Some(SteelVal::SymbolV("did-it".into())));
    }

    #[test]
    fn 嵌套调用参数在叶子求值前先被求值() {
        // Arrange：`(wrap (inner))` 形状——`inner` 必须先被调用求值，
        // `wrap` 才能拿到它的返回值作为参数。
        let mut engine = engine_with(
            r#"
            (define (inner) 41)
            (define (wrap x) (+ x 1))
            (define (tree) (quote (selector (wrap (inner)))))
            "#,
        );

        // Act
        let result = tick(&mut engine, "tree");

        // Assert
        assert_eq!(result, Some(SteelVal::IntV(42)));
    }

    #[test]
    fn 叶子调用出错时该节点降级为失败而非向上冒泡错误() {
        // Arrange：`boom` 缺参——`call_raw` 会返回 Err，本求值器必须
        // 把它降级成「这个节点失败」，不能让 tick 自己 panic 或者把
        // Result 类型泄漏出去。
        let mut engine = engine_with(
            r#"
            (define (boom a b) (+ a b))
            (define (fallback) 'ok)
            (define (tree) (quote (selector (boom 1) (fallback))))
            "#,
        );

        // Act
        let result = tick(&mut engine, "tree");

        // Assert：第一条分支的调用因缺参而报错,被降级为失败,selector
        // 应当继续试第二条分支。
        assert_eq!(result, Some(SteelVal::SymbolV("ok".into())));
    }

    #[test]
    fn 树入口函数不存在时返回空而非崩溃() {
        // Arrange
        let mut engine = engine_with("(define (unrelated) 1)");

        // Act
        let result = tick(&mut engine, "no-such-tree");

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 复现规格示例结构的嵌套selector加sequence() {
        // Arrange：结构对齐规格 §10.5 原文示例（函数名不同，语义相同）
        // ——第一条 sequence 条件为假，第二条 sequence 条件为真，命中
        // 第二条的动作。
        let mut engine = engine_with(
            r#"
            (define (hp-below?) #f)
            (define (flee) 'fleeing)
            (define (has-order?) #t)
            (define (attack) (list 'attack-order-target))
            (define (tree)
              (quote (selector
                       (sequence (hp-below?) (flee))
                       (sequence (has-order?) (attack))
                       (fallback-follow))))
            (define (fallback-follow) 'following)
            "#,
        );

        // Act
        let result = tick(&mut engine, "tree");

        // Assert
        assert_eq!(
            result,
            Some(SteelVal::ListV(
                [SteelVal::SymbolV("attack-order-target".into())]
                    .into_iter()
                    .collect()
            ))
        );
    }
}
