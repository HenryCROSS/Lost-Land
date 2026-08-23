# Steel 语法参考——在迷途大陆里能写什么

- **冻结时间**：2026-08-19（初版）；2026-08-19 补充官方 book 对照（同日追加批次）
- **对应提交**：`aeac32af32dde3616e4259f3c00f060bebd7589a`（初版冻结时的提交，`main` 分支，908 条既有测试全绿、六道门禁全过时的状态）；本次补充批次对应提交 `d77f997`（958 条测试全绿）
- **`steel-core` 版本**：`0.8.2`（见 `crates/ll-script/Cargo.toml`）
- **官方文档**：<https://mattwparas.github.io/steel/book/>（mdbook，源码在 <https://github.com/mattwparas/steel> 的 `docs/src/`）——**页面上没有任何地方标注对应的 `steel-core` 版本号**，`docs/src/SUMMARY.md`/各章节页面都没有版本字段，无法确认查阅时的 book 是否与本项目锁定的 `0.8.2` 完全同步；本次核对是拿 book 的说法逐条在 `0.8.2` 上实跑，跑不通的都在下面标注，没发现跑不通的地方按"未见不一致"处理，不代表两者百分之百同版本。
- **验证方式**：`crates/ll-script/tests/steel_syntax_reference.rs`——本文档每一段标注「已实测」的代码，都能在该文件里找到同名（或本文档指名）的测试函数，运行的是真实的 `ll_script::ScriptEngine`，不是纸面推演。少数几段因为跨 crate 边界（见下）无法从 `ll-script` 自己的测试里验证，改为指向 `ll-mod`/`ll-script` 源码里已经存在、且持续跑在 CI 里的其他测试。

## 零、这份文档回答的是什么问题

**不是**「Steel 语言能做什么」——那是 Steel 官方文档/R5RS 规范的事，而且 Steel 本身是「类 R5RS」，不是 R5RS 本身，两者不能划等号。

**是**「在迷途大陆的 mod 脚本沙箱里，穿过 `crates/ll-script/src/whitelist.rs` 的 AST 白名单、`host.rs` 的 `META_DENY_LIST` 系列拒绝清单、`Engine::new_sandboxed()` 的能力收窄之后，真正能写出什么、调用什么」。这三层机制共同定义的边界见 [ADR 0012](../decisions/0012-steel-capability-surface-verification.md)（标准库能力面实测）与 [ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md)（拒绝能力必须给替代品的通则）——本文档不重复这两份 ADR 的论证过程，只把结论整理成"能写什么/怎么写/写错了长什么样"这个使用者视角。

**白名单的定位是能力边界，不是语言子集**（项目所有者裁定，`whitelist.rs` 模块文档写死）：被挡住的是文件系统、网络、进程、线程、墙钟、非确定性随机，以及能触达以上任意一项的反射入口；宏、闭包、递归、自定义结构体、高阶函数**全部可用**。一份通用 Steel/Scheme 教程会教出一些在这里跑不通的写法（`display` 打到 stdout、`(require-builtin steel/time)`、`eval!`……），本文档只收录在本项目沙箱里真正能跑通的部分。

## 一、基础语法

以下全部例子对应 `crates/ll-script/tests/steel_syntax_reference.rs` 的 `一_基础语法` 模块，逐条已实测通过。

### 1. 定义与字面量（已实测：`define变量与整数字符串布尔字面量`）

```scheme
(define x 42)
(define name "lostland")
(define ok? #t)
(define (probe) (list x name ok?))
```

`(probe)` 返回 `(42 "lostland" #t)`。整数、字符串、布尔字面量与普通 Scheme 写法一致；标识符可以带 `?`/`!`/`-`（`ok?`、`state-set!` 这类命名在本项目 mod API 里大量出现，见第三节）。

### 2. 列表与向量（已实测：`列表与向量字面量及其存取函数`）

```scheme
(define xs (list 1 2 3))
(define v (vector 10 20 30))
(define (probe)
  (list (car xs) (cdr xs) (length xs) (vector-ref v 1)))
```

`(probe)` 返回 `(1 (2 3) 3 20)`。`list`/`vector`/`car`/`cdr`/`length`/`vector-ref` 均在白名单内——它们不是任何 `BuiltInModule` 的导出，是 `steel-core` 标准库脚本（`src/scheme/stdlib.scm`）里用 Scheme 自己写的 `define`，跟随 prelude 直接落进全局作用域（`host.rs` 的 `compute_allowed_identifiers` 文档「为什么不再手工维护一份安全模块清单」一节已核实这一点）。

**哈希表字面量存在但不要拿来做需要确定顺序的事**：`(hash 'a 1 'b 2)` 能构造、能用 `hash-ref` 按键查，但 [ADR 0012](../decisions/0012-steel-capability-surface-verification.md) 实测 Steel 内置哈希表的**遍历顺序不稳定**——同一进程内、构造语句完全相同的两个哈希表实例，`hash-keys->list` 的结果都不相等。任何需要顺序参与逻辑的场景（"按某个键排序后依次处理"），不能依赖脚本侧对哈希表做裸遍历，必须让宿主先排好序再喂给脚本（见第三节 `ordered.rs`，目前是纯 Rust 侧工具，脚本还拿不到）。

### 3. 条件：`if`/`cond`（已实测：`if与cond条件分支`）

```scheme
(define (classify n)
  (cond
    [(< n 0) "negative"]
    [(= n 0) "zero"]
    [else "positive"]))
(define (probe) (list (if (> 3 2) 'yes 'no) (classify -5) (classify 0) (classify 7)))
```

`cond` 的分支可以用方括号 `[...]` 或圆括号 `(...)`，两者等价（本文档统一用方括号，读起来更容易分清"分支"和"分支里的表达式"）。

### 4. `let` 家族（已实测：`let家族四种绑定形式`）

```scheme
(let ([a 1] [b 2]) (+ a b))                     ; 普通 let：绑定值在外层作用域求值
(let* ([a 1] [b (+ a 1)]) (+ a b))              ; let*：后面的绑定能看见前面的
(letrec ([even? (lambda (n) ...)]
         [odd?  (lambda (n) ...)])
  (even? 10))                                    ; letrec：互相递归的绑定
(let loop ([i 0] [acc 0])                        ; 命名 let：写循环最常用的形式
  (if (= i 5) acc (loop (+ i 1) (+ acc i))))
```

四种形式全部可用，命名 `let`（这里的 `loop` 不是关键字，是给这次 `let` 起的名字，可以换成任意标识符）是本项目脚本里写循环的推荐写法。

### 5. 递归与尾调用（已实测：`尾递归循环一万次不超时不爆栈`）

```scheme
(define (probe)
  (let loop ([i 0] [acc 0])
    (if (= i 10000) acc (loop (+ i 1) (+ acc 1)))))
```

用命名 `let` 写的尾递归循环一万次能在中断预算（300ms，见 `host.rs` 的 `INTERRUPT_TIMEOUT`）内正常返回，说明尾调用确实被优化，不是每次递归都在增长调用栈。**非尾递归也完全可用**（见第二节「四、非尾递归」），只是深度过大时会撞上中断预算或原生栈限制——`crates/ll-script/src/host.rs` 的中断机制是墙钟超时（300ms），不是按调用深度计数,具体多深会撞线取决于每次调用的实际耗时,本文档不给一个虚构的"最大递归深度"数字。

### 6. `quote`/`quasiquote`（已实测：`quote与quasiquote构造数据`）

```scheme
(list 'move 'north
      `(sum ,(+ 1 2) end)
      `(a ,@(list 1 2 3) b))
```

返回 `(move north (sum 3 end) (a 1 2 3 b))`。`'north` 是符号字面量（数据，不是"引用了一个叫 north 的函数"——白名单专门跳过 `quote` 包住的部分，不要求 `north` 出现在白名单里，见 `whitelist.rs` 模块文档「为什么跳过 quote 包住的部分」）；`` `(...) `` 里 `,` 展开一个表达式的值、`,@` 展开并拼接一个列表——本项目 `api/intent.rs` 就是用 `(list 'move 'north)` 这种写法让脚本表达"这一回合想干什么"，见下面第三节。

### 7. 数值：大整数、有理数、`exact`/`inexact`（已实测：`数值大整数有理数与exact_inexact转换`）

来源：官方 book《Values > Numbers》一节，列出了 Steel 数值塔的几种字面量形式：`1`（`i64`）、`3.14`（`f64`）、`1/2`（有理数）、`6.02e+23`（`f64`）、`1+2i`（复数）、`9999999999999999999999`（大整数，堆分配）。

```scheme
(define (probe)
  (list
    9999999999999999999999      ; 大整数字面量，超出 i64 范围
    1/2                          ; 有理数字面量
    (exact 1.5)                  ; 浮点 → 有理数：3/2
    (inexact 3/2)                ; 有理数 → 浮点：1.5
    (exact? 1/2)                 ; #t
    (numerator 3/4)              ; 3
    (denominator 3/4)))          ; 4
```

book 描述的四种形式（大整数、有理数、`exact`/`inexact` 互转、`numerator`/`denominator`）在 `0.8.2` 上全部实测通过。**但这只是脚本内部计算允许的形式**——[ADR 0020](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md) 划定的边界是：脚本内部可以自由用浮点/有理数/大整数做计算，但跨过 `register_fn` 这道墙传给宿主（比如第三节的 `register-skill`）时，宿主侧的 Rust 签名只收整数（`i64`）与 `Milli`（定点小数），不接受浮点或有理数——把一个 `1/2` 或 `1.5` 传给期望整数参数的注册函数，会在 FFI 转换层直接报错，不是脚本语法层面的限制。

### 8. 字符串与符号操作（已实测：`字符串与符号操作`）

来源：官方 book《Values > Strings》与《Values > Symbols》两节列出的函数名。

```scheme
(define (probe)
  (list
    (string-append "a" "b" "c")   ; "abc"
    (symbol->string 'foo)         ; "foo"
    (string->symbol "bar")        ; 'bar
    (concat-symbols 'foo 'bar)    ; 'foobar
    (starts-with? "hello" "he")   ; #t
    (ends-with? "hello" "lo")     ; #t
    (trim "  hi  ")))             ; "hi"
```

book 还列出了 `string->list`（转字符列表）、`split-whitespace`、`string->upper`/`string->lower`、`trim-start`/`trim-end`、`to-string`（把任意值拼接成字符串表示）——这些没有单独写测试逐个验证，但都属于同一批 `steel/strings`/`steel/symbols` 导出，与已验证的几个函数同源，按白名单"能力边界不是语言子集"的定位，没有理由单独被挡。真要确认某个具体函数名能不能用，仍按第四节末尾给出的方法：写一小段脚本调用它试跑。

## 二、Lisp 的核心能力（本项目明确保留的）

以下例子对应 `steel_syntax_reference.rs` 的 `二_核心能力` 模块。这一节的每一项都曾经在某个历史版本的白名单实现里被误挡（[ADR 0012](../decisions/0012-steel-capability-surface-verification.md)「追加实测三」记录了三个真实 bug 的修复过程），项目所有者明确裁定「白名单的定位是能力边界，不是语言子集」之后才恢复——这里列出来不是走过场，是因为它们曾经真的坏过。

### 1. 宏：`define-syntax`/`syntax-rules`（已实测：`define_syntax定义并使用宏`）

```scheme
(define-syntax my-when
  (syntax-rules ()
    [(my-when test body ...) (if test (begin body ...) #f)]))
(define (probe)
  (my-when (> 3 2) (+ 1 1) (+ 2 2)))
```

`(probe)` 返回 `4`。**宏是 Lisp 的核心能力，本项目刻意保留**——安全性不靠"不让脚本写宏"，靠"校验的是宏展开之后的树"：`emit_fully_expanded_ast` 给出的就是完整展开后的 AST，宏定义本身在这份树里完全消失，宏的每一次使用都被替换成它展开出的普通代码，那些代码照常被白名单检查（`whitelist.rs` `ExprKind::Macro`/`SyntaxRules` 分支的文档）。**脚本不能自己扩展白名单**——宏只能重新组合已经在白名单里的东西，写不出一个能召唤 `eval!` 的宏（`eval!` 本身不在白名单里，宏展开后照样会被挡）。

### 2. 闭包与高阶函数（已实测：`高阶函数map_filter_foldl_foldr_apply`、`闭包用box持有可变状态实现计数器生成器`）

```scheme
(define nums (list 1 2 3 4 5))
(list
  (map (lambda (x) (* x x)) nums)   ; (1 4 9 16 25)
  (filter (lambda (x) (> x 2)) nums) ; (3 4 5)
  (foldl + 0 nums)                   ; 15
  (foldr cons '() nums)              ; (1 2 3 4 5)
  (apply + nums))                    ; 15
```

`map`/`filter`/`foldl`/`foldr`/`apply` 全部可用，与 `let`/`car`/`cdr` 一样属于 prelude 自带的 Scheme 函数,不挂在任何模块下。

闭包可以捕获并持有可变状态——用 `box`/`unbox`/`set-box!`（ADR 0012「追加实测三」明确点名放行，纯 VM 内可变单元，无 I/O）：

```scheme
(define (make-counter start)
  (let ([state (box start)])
    (lambda ()
      (set-box! state (+ (unbox state) 1))
      (unbox state))))
(define counter (make-counter 10))
(list (counter) (counter) (counter))  ; (11 12 13)
```

**这类闭包状态只活在这次 `ScriptEngine` 实例的生命周期内，不会跨帧/跨存档保留**——存档读取会强制重建全部脚本引擎（`host.rs` 的 `rebuild_all_engines_after_load`），VM 内部的 `define`/`set!`/`box` 状态全部清零重来。需要跨帧持久化的状态必须走 `state-set!` 系列显式写入 `WorldState`，见第三节「三」。

### 3. 自定义结构体：`struct`（已实测：`struct自定义结构体定义构造访问与谓词`）

```scheme
(struct Point (x y))
(define p (Point 3 4))
(list (Point-x p) (Point-y p) (Point? p) (Point? 5))
```

返回 `(3 4 #t #f)`。`struct` 宏展开后依赖 `make-struct-type`/`#%struct-property-ref` 等一批 `steel/meta` 下的纯 VM 内机制——这批机制**曾经因为 `steel/meta` 被整体清空而连带被挡死**（旧版本把 `steel/meta` 当成一个整体"太复杂看不过来"直接清空,102 个导出名字不分青红皂白全部拒绝），现在改成逐名审查的 `META_DENY_LIST`（`host.rs`），只挡确认危险的名字，`struct` 依赖的机制不在其中，因此可用。

自动生成的访问器命名规律：`(struct 类型名 (字段...))` 会生成 `类型名` 本身（构造函数）、`类型名-字段名`（访问器）、`类型名?`（谓词）。

### 4. 非尾递归（已实测：`非尾递归阶乘`）

```scheme
(define (fact n) (if (= n 0) 1 (* n (fact (- n 1)))))
(fact 10)  ; 3628800
```

递归本身不受任何特殊限制，尾递归（第一节「五」）只是"更省栈"的一种写法，不是"唯一合法的写法"。

### 5. 宏的嵌套省略号模式（已实测：`宏支持嵌套省略号模式`）

官方 book《Language Reference > Macros》给出的示例只有单层省略号（`(or x y ...)`）。嵌套形式——模式里出现"列表的列表 + 省略号"，比如 `syntax-rules` 里常见的 `([var val] rest ...)`——book 没有专门举例，实测确认同样能正确展开：

```scheme
(define-syntax my-let*
  (syntax-rules ()
    [(my-let* () body ...) (begin body ...)]
    [(my-let* ([var val] rest ...) body ...)
     (let ([var val]) (my-let* (rest ...) body ...))]))
(define (probe) (my-let* ([a 1] [b (+ a 1)] [c (+ b 1)]) (list a b c)))
```

`(probe)` 返回 `(1 2 3)`——手写了一个 `let*`，验证嵌套省略号模式在递归展开中逐层匹配正确。

### 6. 宏的卫生性（已实测：`宏展开卫生不遮蔽调用点同名变量`）

book 只有一句话带过："These macros allow for a simple extension of Steel"，没有专门讲卫生性保证。用经典探针实测确认 `syntax-rules` 展开是卫生的：

```scheme
(define-syntax my-or
  (syntax-rules ()
    [(my-or a b) (let ([t a]) (if t t b))]))
(define t 100)
(define (probe) (my-or #f t))
```

`(probe)` 返回 `100`。如果宏展开不卫生（朴素文本替换），宏内部引入的 `t` 和调用点传入的 `t`（`b` 参数字面量就是符号 `t`）会被合并成同一个绑定，展开成 `(let ([t #f]) (if t t t))`，三处 `t` 全部指向同一个值 `#f`，结果会是 `#f`；实测拿到 `100`，说明宏内部的 `t` 与调用点的全局变量 `t` 被区分开了，没有发生变量捕获。

### 7. `syntax-case` 过程式宏（已实测：`syntax_case必须写成过程式变换器形式`）——**实测发现，book 举例不够，容易写错**

book 提到 Steel 同时提供 `syntax-rules` 和 `syntax-case` 两套宏系统，但页面上没有给出 `syntax-case` 的调用形状。**实测踩坑记录**：照抄 `syntax-rules` 的声明式形状去写 `syntax-case` 会在运行期报错，不是编译期：

```scheme
;; 错的写法——照抄 syntax-rules 的形状
(define-syntax my-thing
  (syntax-case ()
    [(my-thing a) (quote (a))]))
```

`load_source` 拿到 `Err(Runtime("Error: Generic:  syntax-case expects a function", ...))`——这份宏定义本身通过了白名单和编译，但求值到 `syntax-case` 那一步时，它发现自己的参数不是一个"函数"，运行期才报错。

正确形状是**过程式变换器**：`(define-syntax (名字 stx) (syntax-case stx () [模式 #'模板]))`——`stx` 是显式接收的语法对象参数，`#'`/`#\`` 构造语法而不是普通数据，这与 `syntax-rules` 声明式的 `(syntax-rules () [模式 模板])` 形状完全不同。写法照 `steel-core` 自身测试（`steel-core-0.8.2/src/tests/success/syntax_case.scm`）核对过：

```scheme
(define-syntax (my-thing stx)
  (syntax-case stx ()
    [(_ a) #'(list 'got a)]))
(define (probe) (my-thing 5))
```

`(probe)` 返回 `(got 5)`。`syntax-case` 能力更强（可以在展开体里嵌任意 Steel 代码做判断，`syntax-rules` 只能做纯模式替换），但调用形状是本文档实测才发现的，book 页面本身没写清楚。

### 8. `match` 模式匹配（已实测：`match模式匹配列表下划线else省略号与guard`、`match不支持直接解构struct实例`）

官方 book 的 `#%private/steel/match` 一节只说了一句"这个模块在 prelude 里，因此运行 Steel 时自动可用"，没有给出任何调用示例。**对照 `steel-core` 0.8.2 自带的 `src/scheme/modules/match.scm` 源码逐条实测**，确认 `match` 支持：

- `(list ...)` 前缀的列表模式
- 裸符号做绑定变量——**不需要 `?` 前缀**（这一点容易搞混：`steel-core` 仓库里另有一份只用于自身测试、从未随 crate 一起发布的 `matcher.scm`/`match!` 实验版本，那个版本要求变量写成 `?x`；随 `0.8.2` 真正发布并自动进入 prelude 的是 `match.scm`，变量就是普通符号）
- `_` 通配符（忽略该位置，不绑定）
- `else` 兜底分支
- `(list first rest ...)` 用省略号收集剩余元素
- `#:when` 守卫子句

```scheme
(define (probe-basic)
  (match (list 1 2 3)
    [(list a b c) (+ a b c)]))               ; 6

(define (probe-wildcard x)
  (match x
    [(list a _ c) (list 'three a c)]
    [else 'other]))                          ; (list 1 2 3) => (three 1 3)；5 => 'other

(define (probe-rest)
  (match (list 1 2 3 4 5)
    [(list first rest ...) (list first rest)])) ; (1 (2 3 4 5))

(define (probe-guard n)
  (match n
    [n #:when (> n 10) 'big]
    [n 'small]))                             ; 20 => 'big；3 => 'small
```

**实测发现的限制，book 完全没提**：`match` 不能直接用 `(结构体名 字段...)` 这种形状去匹配并解构一个 `struct` 实例：

```scheme
(struct Pt (x y))
(define (probe p)
  (match p
    [(Pt a b) (+ a b)]))
```

`load_source` 直接拿到 `Err(Runtime("Error: Generic:  list pattern must start with \`list - found  Pt", ...))`——这个检查发生在**编译期**（`match` 宏展开时），不需要真的调用 `probe` 就会报错，原因是 `match.scm` 的模式编译器只认"`list` 前缀的列表模式"或"裸符号/通配符/嵌套列表"两类形状，把 `(Pt a b)` 当模式写会被当成"没有 `list` 前缀"直接拒绝。要匹配一个 `struct` 实例，只能退回 `Pt?` 谓词判断 + 手动调用访问器（`Pt-x`/`Pt-y`，第二节「3」），不能指望 `match` 帮忙解构。

### 9. `hashset`（已实测：`hashset构造与包含判断`）

book《Collections > Hash sets》给出构造示例 `(hashset 10 20 30 30 40)`（重复元素自动去重）。实测补一个查询用法：

```scheme
(define (probe)
  (define hs (hashset 1 2 3))
  (list (hashset-contains? hs 2) (hashset-contains? hs 99)))
```

`(probe)` 返回 `(#t #f)`。`hashset` 与 `hash`（第一节「二」）同样基于哈希数组映射字典树（HAMT）实现，**同样不应该假定遍历顺序稳定**——book 没有为 `hashset` 单独写遍历顺序说明，但实现机制与 `hash` 共享（`crates` 依赖树里都落到 `im`/`im_rc`/`steel_imbl` 的同一套 `GenericHashMap`/`GenericHashSet`），第一节「二」对 `hash` 遍历顺序不稳定的警告同样适用于 `hashset`，脚本侧不要写依赖 `hashset` 遍历顺序的逻辑（C5）。

### 10. 脚本侧捕获运行时错误：`with-handler`（已实测：`with_handler捕获脚本内运行时错误`）——**book 完全没有错误处理章节，这条不来自官方文档**

对官方 book 通篇检索 `with-handler`/`guard`/`raise`/`call-with-exception-handler`，**没有任何一处提及**——book 没有错误处理相关的章节。这条写法是直接读 `steel-core` 0.8.2 的 `src/scheme/stdlib.scm` 源码找到的：`with-handler` 是一个 `define-syntax` 宏，基于 `call-with-exception-handler` 加上 `reset`/`shift` 分界续延实现，随 prelude 自动可用：

```scheme
(define (probe)
  (with-handler (lambda (e) 'caught)
                (car '())))
```

`(probe)` 返回 `'caught`——`(car '())` 本该产生一个第四节「4」描述的 `ScriptError::Runtime`，但因为被 `with-handler` 包住，错误在脚本内部被处理程序捕获，`call_raw` 从宿主视角看到的是正常返回值 `'caught`，不是 `Err`。**这意味着脚本可以自己决定哪些运行时错误要处理、哪些要继续向上抛给宿主**——`with-handler` 只包住它的表达式体，没被包住的错误照样按第四节「4」的方式冒出来变成 `Err`。

**R7RS 标准的 `guard`/`raise` 语法在 `0.8.2` 里不存在**：全仓库检索 `steel-core` 的 `.scm` 源码，从未找到 `guard` 的定义。脚本里写 `(guard (e (#t ...)) ...)` 会得到 `ParseError("脚本引用了不在白名单内的标识符「guard」", ...)`——**这不是白名单故意拒绝，是"压根不存在"**：白名单的机制是从 prelude 引导后的全局作用域里收集允许的标识符（`compute_allowed_identifiers`），一个从未在任何 `.scm` 源码里 `define`/`define-syntax` 过的名字，天然不会出现在这份收集结果里，报错文案和"故意拉黑的名字"（比如 `eval!`）用的是同一种 `ParseError`，但成因完全不同——如果 `steel-core` 未来版本加入了 `guard`，这个名字会自动出现在允许列表里，不需要本项目做任何改动。

## 三、本项目提供的 mod API

本节逐个记录每个函数的真实签名与调用示例，并**明确标注哪些目前还没有生产消费者**——`crates/ll-script/src/api/` 下的大多数模块是给尚未落地的行为树求值器预留的，接上会是空转，不要以为这些函数已经在真实游戏循环里被调用。

判断"有没有生产消费者"的方法：搜索仓库里除了各模块自己的单元测试之外，还有没有别的代码真正调用了 `register(engine)`/`set_active_*` 这套接线。实测结果：

| API 模块 | 脚本侧函数名 | 生产消费者 |
|---|---|---|
| `crates/ll-mod` 六个 `script_*_api.rs` | `register-terrain`/`register-class`/`register-skill`/`register-subclass`/`register-quest`/`register-race` | **有**——`crates/ll-mod/src/pipeline.rs` 的 `load_one_script` 函数（`329`~`334` 行）在真实 mod 装载管线里依次调用六个 `register_*_api`，是本节唯一已经接进真实装载流程的一批 |
| `crates/ll-script/src/api/query.rs` | `world-move-cost-at`/`world-blocks-sight-at`/`world-tick`/`world-ambient-light` | **无**——全仓库搜索，除自身单元测试与本文档的验证测试外，没有任何非测试代码调用 `query::register`/`query::set_active_world` |
| `crates/ll-script/src/api/rng.rs` | `rng-next-u64`/`rng-gen-range`/`rng-chance` | **无**——同上，没有任何非测试代码调用 `rng::register`/`rng::set_active_rng` |
| `crates/ll-script/src/api/state.rs` | `state-set!`/`state-get!`/`entity-state-set!`/`entity-state-get!`/`state-get-foreign`/`content-ref` | **无**——同上，没有任何非测试代码调用 `state::register` |
| `crates/ll-script/src/api/intent.rs` | （不是脚本调用的函数，是宿主侧解析脚本返回值的 Rust 函数）`parse_intent` | **无**——没有任何非测试代码调用它 |
| `crates/ll-script/src/api/handle.rs` | （`ScriptEntityHandle` 类型，供别的注册函数的参数/返回值使用） | 部分——`state.rs` 的 `entity-state-set!`/`entity-state-get!`/`content-ref` 往返用到这个类型，但没有任何查询函数会把一个新的实体句柄交给脚本（比如"最近的敌人是谁"），脚本目前无法凭空拿到一个句柄 |
| `crates/ll-script/src/api/log.rs` | 无——**这个模块没有任何 `register_fn` 调用，脚本完全调用不到它** | 不适用——`ScriptDiagnostic` 是纯 Rust 类型，只在脚本调用失败时由宿主自动构造，不是脚本主动调用的 API（[ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md) B-7 已核实记录为待办：脚本目前没有办法主动打印一条日志） |
| `crates/ll-script/src/api/ordered.rs` | 无——**这个模块同样没有任何 `register_fn` 调用** | 不适用——`sorted_by_key` 是纯 Rust 侧工具函数，脚本目前拿不到任何调用它的入口 |

**换句话说**：目前真正接入了「mod 装载 → 脚本调用 → 产生真实效果」这条完整链路的，只有六个 `register-*` 内容声明函数。`query`/`rng`/`state`/`intent`/`handle` 五个模块的功能本身已经实现、也有各自完整的单元测试保证行为正确，但把它们接进真实的游戏循环（世界 tick、AI 决策、脚本触发的状态写入）是另一项尚未完成的工作——本文档只负责如实标注这个状态，不代为判断这项工作的优先级。

### 1. 六个内容注册函数（已实测，见下方逐项引用）

**共同约定**：全部返回 `Result<bool, String>`（Steel 侧的 `Err` 会被 `steel-core` 自动转成一次真正的求值期错误，`load_source` 会拿到 `Err`，不会被脚本当成普通返回值悄悄吞掉）；完整命名空间标识符字符串形如 `"yourmod:foo"`；表示"无此项"用**空字符串 `""`** 做哨兵，不是 `#f`/`'()`（Steel FFI 转换层没有现成的 `Option<String>` 支持）。

#### `register-terrain`

```scheme
(register-terrain id blocks-sight blocks-move move-cost opens-into)
```

| 位置 | 参数 | 类型 | 说明 |
|---|---|---|---|
| 1 | `id` | 字符串 | 完整命名空间标识符，如 `"examplemod:lava_floor"` |
| 2 | `blocks-sight` | 布尔 | 是否阻挡视线 |
| 3 | `blocks-move` | 布尔 | 是否阻挡移动 |
| 4 | `move-cost` | 整数 | 移动代价；`blocks-move` 为真时被忽略 |
| 5 | `opens-into` | 字符串 | 撞入后变成的地形标识符，`""` 表示没有 |

已实测的真实调用（`crates/ll-mod/src/script_terrain_api.rs::通过线程局部注册目标脚本能真正调用register_terrain`）：

```scheme
(register-terrain "examplemod:lava_floor" #f #t 4294967295 "")
```

#### `register-class`

```scheme
(register-class id display-name-key primary-attribute)
```

| 位置 | 参数 | 类型 | 说明 |
|---|---|---|---|
| 1 | `id` | 字符串 | 完整命名空间标识符 |
| 2 | `display-name-key` | 字符串 | 指向 Fluent 本地化键的完整标识符 |
| 3 | `primary-attribute` | 字符串/符号 | 六选一：`"strength"`/`"dexterity"`/`"constitution"`/`"intelligence"`/`"willpower"`/`"charisma"` |

已实测（`crates/ll-mod/src/script_class_api.rs::通过线程局部注册目标脚本能真正调用register_class`）：

```scheme
(register-class "yourmod:necromancer" "yourmod:necromancer_display_name" "willpower")
```

#### `register-subclass`

```scheme
(register-subclass id display-name-key)
```

最简单的一个，只有两个参数。已实测（`crates/ll-mod/src/script_subclass_api.rs::通过线程局部注册目标脚本能真正调用register_subclass`）：

```scheme
(register-subclass "yourmod:shadowdancer" "yourmod:shadowdancer_display_name")
```

#### `register-skill`——**签名最长的一个，10 个参数，务必按顺序核对**

```scheme
(register-skill id owning-class prerequisites cooldown-ticks
                 resource-kind resource-amount
                 effect-kind effect-tag effect-amount effect-amount2)
```

| 位置 | 参数 | 类型 | 说明 |
|---|---|---|---|
| 1 | `id` | 字符串 | 完整命名空间标识符 |
| 2 | `owning-class` | 字符串 | 所属职业标识符，`""` 表示通用技能 |
| 3 | `prerequisites` | 字符串列表 | 前置技能标识符列表，`(list)` 表示无前置 |
| 4 | `cooldown-ticks` | 整数 | 冷却 tick 数 |
| 5 | `resource-kind` | 字符串 | `"none"`/`"mana"`/`"stamina"` |
| 6 | `resource-amount` | 整数 | `resource-kind` 为 `"none"` 时忽略 |
| 7 | `effect-kind` | 字符串 | `"deal-damage"`/`"restore-resource"`/`"temporary-stat-modifier"` |
| 8 | `effect-tag` | 字符串 | 按 `effect-kind` 解释（见下） |
| 9 | `effect-amount` | 整数 | 按 `effect-kind` 解释（基础伤害/恢复量/属性增减量） |
| 10 | `effect-amount2` | 整数 | 仅 `"temporary-stat-modifier"` 使用（持续 tick 数），其余传 `0` |

`effect-tag`（第 8 个参数）随 `effect-kind` 而变，是这个签名里最容易记混的一点：

- `effect-kind = "deal-damage"`：`effect-tag` 不使用，传 `""`
- `effect-kind = "restore-resource"`：`effect-tag` 是恢复的资源种类，`"mana"`/`"stamina"`
- `effect-kind = "temporary-stat-modifier"`：`effect-tag` 是受影响的主属性名（同 `register-class` 的 `primary-attribute` 六选一）

已实测的两个例子（分别来自 `crates/ll-mod/src/script_skill_api.rs` 的
`通过线程局部注册目标脚本能真正调用register_skill` 与
`临时属性修正效果解析出正确的属性与持续时间` 两条测试）：

```scheme
;; deal-damage：造成 15 点基础伤害，消耗 12 点法力，冷却 25 tick，无前置
(register-skill "yourmod:frostbolt" "" (list) 25 "mana" 12 "deal-damage" "" 15 0)

;; temporary-stat-modifier：体质 +3，持续 10 tick，消耗 5 点耐力，冷却 15 tick
(register-skill "yourmod:brace" "" (list) 15 "stamina" 5
                "temporary-stat-modifier" "constitution" 3 10)
```

#### `register-quest`

```scheme
(register-quest id prerequisites condition-kind condition-arg condition-count)
```

| 位置 | 参数 | 类型 | 说明 |
|---|---|---|---|
| 1 | `id` | 字符串 | 完整命名空间标识符 |
| 2 | `prerequisites` | 字符串列表 | 前置任务节点标识符列表 |
| 3 | `condition-kind` | 字符串 | `"kill-count"`/`"script"`（只有这两档） |
| 4 | `condition-arg` | 字符串 | `"kill-count"` 时是目标敌人类型标识符；`"script"` 时是脚本回调标识符 |
| 5 | `condition-count` | 整数 | `"kill-count"` 时是需要击杀的数量；`"script"` 时忽略，传 `0` |

已实测（`crates/ll-mod/src/script_quest_api.rs::通过线程局部注册目标脚本能真正调用register_quest`）：

```scheme
(register-quest "yourmod:kill_goblins" (list) "kill-count" "lostland:goblin" 3)
```

#### `register-race`——**13 个位置参数，七项属性修正 + 四项其余数值**

> **签名在「暗视语义改版 + 幸运 authoring」批次变过一次，是破坏性变更。**
> 相对旧版：第 9 位新插入了 `luck-mod`（此前 mod 作者写不出种族幸运
> 修正，是已记录的 API 缺口），其后的 `darkvision-*` 顺延到第 10 位
> **并且改名改语义**（`darkvision-floor` 光照千分比下限 →
> `darkvision-cells` 夜间视野格数下限）。旧脚本必须逐条更新，照旧值
> 不改会让参数整体错位一格。

```scheme
(register-race id display-name-key
                strength-mod dexterity-mod constitution-mod
                intelligence-mod willpower-mod charisma-mod luck-mod
                darkvision-cells footprint-width footprint-height
                lifespan-years)
```

| 位置 | 参数 | 类型 | 说明 |
|---|---|---|---|
| 1 | `id` | 字符串 | 完整命名空间标识符 |
| 2 | `display-name-key` | 字符串 | 本地化键标识符 |
| 3–9 | `strength-mod` … `luck-mod` | 整数 | 七项主属性的**固定增减量**（可为负）——**不是千分比**，顺序固定为力量/敏捷/体质/智力/意志/魅力/幸运 |
| 10 | `darkvision-cells` | 整数 | 夜间视野格数下限。`0` = **未声明**（按常人处理，落回 `DEFAULT_NIGHT_SIGHT_RADIUS`，当前为 4 格）；非 0 直接生效，**允许低于默认值**表示「夜里比常人更瞎」。负数钳到 `0` |
| 11 | `footprint-width` | 整数 | 占位格宽度，钳位到 `u8` |
| 12 | `footprint-height` | 整数 | 占位格高度，钳位到 `u8` |
| 13 | `lifespan-years` | 整数 | 寿命（年） |

（表格里的"位置"从 1 数到 13 是把 `id`/`display-name-key` 也计入；函数签名本身连同 `id` 共 13 个位置参数，比其余五个都长——写这个调用时强烈建议逐个数一遍位置,不要凭印象排列。）

已实测（`crates/ll-mod/src/script_race_api.rs::通过线程局部注册目标脚本能真正调用register_race`，该测试逐个钉住每个参数落在哪一格）：

```scheme
(register-race "yourmod:half_elf" "yourmod:half_elf_display_name" 0 1 0 0 0 1 3 5 1 1 150)
```

对照上表：力量 +0、敏捷 +1、体质 +0、智力 +0、意志 +0、魅力 +1、幸运 +3、暗视 5 格、占位 1×1、寿命 150 年。

#### 六个函数在同一份脚本里连续调用（已实测，`crates/ll-mod/src/pipeline.rs` 的装载管线测试）

六个 `register-*` 共享同一个 `Registry`（`crates/ll-mod/src/active_registry.rs`），因此一份 mod 脚本可以在同一个文件里连续调用全部六种：

```scheme
(register-terrain "gameplay:lava_floor" #f #f 350 "")
(register-class "gameplay:necromancer" "gameplay:necromancer_display_name" "willpower")
(register-subclass "gameplay:shadowdancer" "gameplay:shadowdancer_display_name")
(register-skill "gameplay:frostbolt" "" (list) 25 "mana" 12 "deal-damage" "" 15 0)
(register-quest "gameplay:kill_goblins" (list) "kill-count" "gameplay:goblin" 3)
(register-race "gameplay:half_elf" "gameplay:half_elf_display_name" 0 1 0 0 0 1 0 0 1 1 150)
```

### 2. 脚本状态存储：`state-set!`/`state-get!`/`entity-state-set!`/`entity-state-get!`

**目前没有生产消费者**（见本节开头的表），但函数本身已经实现并有完整测试覆盖。`register(engine, mod_namespace)` 需要一个命名空间参数——**不是脚本参数**，由宿主在构造这个 `ScriptEngine` 时固化，脚本没有任何语法能覆盖它，这是命名空间隔离的类型层面保证。

全局作用域（已实测，`steel_syntax_reference.rs::三_mod_api::state系列全局存储与跨mod只读查询与内容引用`）：

```scheme
(state-set! "reputation" 42)          ; 写；返回 #t/#f（是否成功）
(state-get! "reputation")             ; 读；查不到返回 Void，不是 #f
(state-get-foreign "lostland" "reputation")  ; 显式跨 mod 只读查询
(content-ref "yourmod:healing_potion")       ; 把一个字符串标记成"内容引用"而非普通字符串
```

**写入不是直接写穿 `WorldState`**：`state-set!`/`entity-state-set!` 只把写入攒进一个待写缓冲区，真正落盘要等宿主在脚本调用结束后取走整批、包成一条 `Effect::SetScriptState` 交给 `resolve → apply` 管线——这是为了满足"`apply` 是全局唯一能改世界的地方"这条约束（[脚本状态存储](script-state-storage.md) 有完整设计）。同一次调用窗口内"先写后读同一个键"能立即读到刚写的值（缓冲区优先查找），不需要等 `apply` 真正跑完。

**写入有配额**（`PER_MOD_QUOTA_BYTES`/`PER_MOD_ENTITY_QUOTA_BYTES`），超限时 `state-set!` 返回 `#f` 并产生一条 `Severity::Warning` 级别的诊断（见第五节「常见错误」）。

`entity-state-set!`/`entity-state-get!` 需要一个 `ScriptEntityHandle` 参数——**本文档没有给出可独立运行的调用示例**，因为构造一个真实句柄的入口（`ScriptEntityHandle::new`）是 `crate` 内部可见性（`pub(crate)`），脚本沙箱里也没有任何查询函数会把一个新句柄交给脚本（见本节开头表格「`handle.rs`」一行）。已实测的调用形态可以在 `crates/ll-script/src/api/state.rs` 自己的单元测试里找到，例如 `每实体存储随实体销毁而消失不产生孤儿`：

```scheme
(entity-state-set! target "cooldown" 5)
```

其中 `target` 是宿主传入的一个 `ScriptEntityHandle`，脚本自己写不出这个值。

### 3. `world-*` 只读查询（已实测：`steel_syntax_reference.rs::三_mod_api::query系列四个只读查询函数`）

```scheme
(world-move-cost-at x y)     ; 该格地形的移动代价，整数
(world-blocks-sight-at x y)  ; 该格地形是否阻挡视线，布尔
(world-tick)                 ; 当前世界时钟，整数
(world-ambient-light)        ; 当前环境光照等级，整数
```

坐标 `x`/`y` 是未经归一化的环面坐标，宿主内部会调用 `TorusSize::wrap` 处理。这四个函数只读，没有任何一个接收"要写什么"参数的版本。

### 4. 确定性随机（已实测：`steel_syntax_reference.rs::三_mod_api::rng系列三个函数`）

```scheme
(rng-next-u64)          ; 下一个随机数（Steel 整数是 isize，超过 i64::MAX 的位模式会显示成负数，但确定性不受影响）
(rng-gen-range lo hi)   ; [lo, hi) 内的随机整数；hi <= lo 时返回 lo
(rng-chance permille)   ; 以 permille/1000 的概率返回 #t
```

脚本**拿不到种子本身**，也没有任何函数能传入种子/实体 ID/事件计数去重新拼一个随机流——宿主在每次调用前用 `DetRng::for_entity(世界种子, 实体ID, 事件计数)` 构造好一个流，脚本只能"要下一个数"。**同样的三元组（种子、实体、事件计数）在任何时候重放都得到同样的随机序列**，这是本项目确定性重放的地基之一。

**已知但目前完全没有代码处理的确定性陷阱**（[ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md) B-3）：`event_counter` 目前没有按 mod 命名空间隔离的机制——若未来多个 mod 共用同一套"这是本实体第 N 次请求随机数"的计数方式，装一个新 mod 可能让其他 mod 的随机流跟着偏移。这个问题**当前还没有真实代码触发**（`set_active_rng` 目前只在测试里被调用），但设计脚本触发的随机消耗时应当留意。

### 5. 脚本返回值 → `Intent`：`parse_intent`（不是脚本可调用的函数）

这是宿主侧的 Rust 函数，不是注册进脚本引擎的 API——脚本只需要按约定的形状**返回一个值**，宿主调用完脚本后自己解析。已实测（`steel_syntax_reference.rs::三_mod_api::intent系列wait_move_use_skill三种形状`）认识三种形状：

```scheme
'wait                              ; 等待
(list 'move 'north)                ; 移动，八方向：north/south/east/west/north-east/south-east/south-west/north-west
(list 'use-skill "lostland:strike") ; 使用技能（技能施于自身，target 恒为 None——脚本目前没有安全路径指定攻击目标）
```

其余形状（包括脚本产出的任何不认识的符号/结构）被解析为"这一回合什么都不做"，不会让宿主 panic。

### 6. `handle.rs`/`log.rs`/`ordered.rs`——存在但脚本暂时用不到

- **`ScriptEntityHandle`**（`handle.rs`）：脚本能**持有**并**传回**一个句柄（例如从 `entity-state-get!` 读回一个之前存过的实体引用），但**没有任何函数会凭空发一个新句柄给脚本**——字段私有、无法从 Scheme 字面语法直接构造、`downcast` 按 `TypeId` 精确匹配，三层防伪造机制完整,但目前欠缺"发放"这一半。
- **`log.rs`**：`ScriptDiagnostic` 是纯 Rust 类型，脚本没有任何 `(log-xxx ...)` 可以调用——目前脚本运行时完全没有办法主动打印一条消息，只有脚本**调用失败**（语法错误/超时/参数不匹配）时宿主才会自动构造一条诊断。[ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md) B-7 已记录为待办。
- **`ordered.rs`**：`sorted_by_key` 是纯 Rust 侧排序工具，脚本没有任何入口调用它——它存在的意义是"未来任何需要把无序数据交给脚本的场景，必须先经过这个函数排序"，本身不是脚本 API。

## 四、被拒绝的能力，以及替代品

以下对照表核实自 [ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md)（`host.rs`/`whitelist.rs` 源码 + 该 ADR 全文交叉核对），只摘录**mod 作者写内容时最可能撞见**的条目；完整的十二类拒绝清单分组与八条待办的详细论证见 ADR 原文，本节不重复。

| 想用的能力 | 状态 | 该用什么代替 |
|---|---|---|
| 打印调试信息（`display`/`displayln`） | **不适用**——`display` 本身在白名单内可以调用，但打到的是宿主进程 stdout，玩家看不到、也不知道是哪个 mod 打的 | 无——结构化脚本日志（`(log-info ...)`）是已识别的正当需求但**尚未实现**（ADR 0019 B-7，待办），当前完全没有替代品，如实告知 |
| 读系统时间/墙钟 | 拒绝，且**无正当替代需求**——游戏内时间已经有 `(world-tick)` | `(world-tick)`（第三节「三」） |
| 非确定性随机（`(require-builtin steel/random)`） | 拒绝 | `(rng-next-u64)`/`(rng-gen-range lo hi)`/`(rng-chance permille)`（第三节「四」） |
| 执行 shell 命令/拉起进程 | 拒绝，**无正当替代需求** | 无——mod 不应该、也不需要拉起系统进程 |
| 读写文件系统 | 拒绝 | **待办**——mod 目录资源的 VFS（ADR 0019 J：规格 §5 已规划、`crates/ll-mod/src/` 目前没有 `vfs.rs`），如实告知目前没有替代品 |
| `require-builtin` / `require-for-syntax` | 拒绝——那是向 Steel 内置模块表**直接要能力**，不是模块引用 | 宿主注册的 `register-*`/`world-*`/`rng-*` 等函数；要引用**别的脚本文件**用 `(require "模块名")`，见第六节 |
| `eval!`/`eval-string`/`load`（动态执行字符串代码） | 拒绝，**且判断为不正当**——动态生成代码的正当需求走声明式内容定义（ADR 0016/0017 一/二档）；「把另一个文件的定义拿过来用」这个正当需求由 `(require "模块名")` 满足（第六节），不需要运行期 `load` | 用声明式注册函数（第三节「一」六个 `register-*`），或本项目已设计的"物化列表 + `map`/`filter`" 逃生舱模式（见 [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) 5.4 节） |
| `Engine::new`/`run!`（脚本内构造一个全新、不受限的引擎） | 拒绝，**判断为不正当**——VM 生命周期归宿主管，脚本不该有能力构造另一个不受限的引擎实例 | 无需替代品 |
| 读写进程环境变量 | 拒绝，**判断为不正当** | 若确实需要可配置项，应走 mod 清单声明的配置字段，不是运行期读环境变量 |
| 探测自己能调用哪些 API（`module->exports`） | 拒绝——**判断为"不该在这一层被满足"**（脚本不该在运行期自省自己够不着什么） | 静态 API 文档（本文档）+ 规划中的 `tools/ll-datacheck` |
| 给自定义效果设置延迟触发（"5 回合后触发某效果"） | 拒绝异步族（`poll!`/`block-on`/`join!` 等），**待办** | 已设计接口形状 `(schedule-after ticks 'handler-name payload)`（具名处理器 + 可序列化 payload，不能是闭包），**尚未实现** |
| 给技能范围/弹道计算距离、三角函数 | 不适用——不是"被拒绝"，是**这类工具尚未注册给脚本**（`TorusSize::{delta, chebyshev, squared_euclidean}` 在 `ll-core` 里已经实现，但没有任何 `register_fn` 把它们暴露出来） | **待办，且已核实是当前优先级最高的真实缺口**（ADR 0019 B-1）——目前完全没有替代品，需要脚本做距离/朝向判定的 mod 作者会在这里撞墙 |
| 本地化文本拼接（多语言语序） | 不适用——本项目还没有任何本地化 API 暴露给脚本 | **待办**——`(format-text "key" 'name name 'count n)` 具名参数格式化工具已设计，未实现（ADR 0019 B-2） |
| R7RS 风格的 `guard`/`raise` 异常处理 | **不适用——不是被拒绝，是"压根不存在"**（第二节「10」已详细核实）：`guard` 从未在 `steel-core` 0.8.2 的任何 `.scm` 源码里定义过，报的 `ParseError` 只是"标识符不在允许列表"的通用文案，不是白名单专门拉黑了它 | `with-handler`（第二节「10」）——语义不完全等价（`with-handler` 是单一处理函数包住单一表达式，不是 R7RS `guard` 那种带多分支条件判断的语法），但能满足"脚本内部捕获运行时错误、不让整份脚本调用直接失败"这个核心需求 |

**如何验证某个具体名字到底能不能用**：白名单是权威判据，不是这张表——若表里没列到的某个 Steel 内置函数不确定能不能用，最快的办法是直接写一小段脚本调用它，跑 `ScriptEngine::load_source`，看是拿到 `Ok` 还是 `ParseError`（白名单拒绝走的正是这条路径，见第五节）。

## 五、常见错误

四种 `ScriptError` 变体对应四类失败，`crates/ll-script/tests/steel_syntax_reference.rs::四_常见错误` 模块逐条实测。

### 1. 语法错误（`ScriptError::ParseError`，编译期，从未开始求值）

```scheme
(+ 1 2
```

缺右括号——`load_source` 直接返回 `Err(ParseError("...", Some(偏移量)))`，脚本源码整体没有编译通过，这个 mod 不该被加载。

### 2. 引用了白名单外的标识符（同样是 `ParseError`，编译期）

```scheme
(define (probe) (this-was-never-registered 1))
```

**这类错误也发生在编译期，不是运行时**——Steel 编译器在把这个自由标识符编译进字节码之前，白名单校验已经先审过一遍完整展开后的 AST，命中就直接拒绝整份源码。错误消息会点名具体是哪个标识符（`"脚本引用了不在白名单内的标识符「this-was-never-registered」"`），方便定位，不是一个笼统的"编译失败"。

**这与"函数存在但没被注册"是同一种失败表现**：无论是纯粹拼错了名字，还是引用了一个真实存在但没有被 `register_fn` 注册进这个 `ScriptEngine` 的能力（比如脚本以为 `state-set!` 已经注册,实际上宿主忘了调用 `state::register`），得到的都是同一种 `ParseError`，不会有更细的区分。

### 3. 参数个数不匹配（`ScriptError::ArityMismatch`）

```scheme
(needs-two 1)   ; needs-two 期望两个参数
```

这类错误发生在实际调用那一刻（不是编译期，因为 Steel 允许把函数当值传递,只有真的调用才知道传了几个参数），`load_source`/`call_raw` 都可能返回 `ArityMismatch`。

### 4. 运行时错误（`ScriptError::Runtime`）

```scheme
(car '())   ; 对空列表取 car
```

类型不匹配、除零、越界访问等，只有真正求值到那一行才会触发,不像前两类能在加载阶段就拦下来。

### 5. 超时中断——**实测发现与 `host.rs` 文档字面表述不一致**

死循环：

```scheme
(define (loop) (loop))
(loop)
```

`ScriptError` 类型定义了一个专门的 `Interrupted` 变体，其文档写着"没有携带偏移量：超时是整份脚本跑太久，不是某一行的问题"。**但实测（`死循环返回err而不崩溃进程_变体与文档字面表述不符`）发现 `host.rs` 的 `classify_error` 从未真正构造过这个变体**——全仓库搜索 `ScriptError::Interrupted` 的构造点，只在这个类型自己的 `Display`/`byte_offset` 实现里出现，没有任何一处真正 `match` 出中断并返回它。超时实际拿到的是 `ScriptError::Runtime`，消息形如：

```
Error: Generic: Thread: ThreadId(20) - Interrupted by user
```

**且携带一个字节偏移量**（本次实测拿到 `Some(16)`）——这与文档"超时没有偏移量"的说法相反,偏移量来自中断发生时恰好停在的那条字节码对应的源码位置，不是"没有位置"，是"位置恰好落在死循环内部的某一处"。

调用方目前只能用 `result.is_err()` 或检查消息文本里是否出现 `"Interrupted"` 来判断"是不是被超时打断的"，不能匹配 `ScriptError::Interrupted` 这个变体（会永远匹配不上）。**这是一处已知的、未修复的文档/实现不一致**，本文档如实记录，不代为"修正"成文档原本预期的样子——把 `classify_error` 改成真正识别中断场景、返回 `Interrupted` 变体属于代码修复，不在本次写参考文档的任务范围内，留给后续任务处理。

### 6. 字节偏移量 → 行号

`ScriptError` 携带的是源码里的**字节偏移量**，不是行号本身——`ll-script` 不知道调用方是怎么把源码传进来的，换算成行号需要调用方自己用换行符位置数一遍：

```rust
let line = source[..offset as usize].matches('\n').count() + 1;
```

已实测（`字节偏移量能换算成第几行`）：三行脚本，第三行故意留一个未闭合的括号，换算结果确实是 `3`。加载管理界面正是用这套换算逻辑做到"精确到行号"的错误定位——真正的生产实现在 `crates/ll-mod/src/pipeline.rs` 的 `line_number(source, byte_offset)`（第 390 行），按字节而非按字符扫描、越界时钳位，比这里的示例写法更谨慎，但计数原理相同（数偏移量之前出现了几个换行符）。

## 六、脚本之间：`provide` / `require`

**这一节的规则由 `ll_script::modules` 落地，实测依据见 `crates/ll-script/examples/probe_modules.rs`。**

```scheme
;; helpers.scm —— provide 才是导出形式（不是 export）
(provide 翻倍)
(define (翻倍 x) (* 2 x))
(define (内部细节) 42)          ; 没写进 provide = 私有

;; content/races.scm
(require "helpers")             ; 同 mod，相对本 mod 根目录，不写 .scm
(require "lostland:ids")        ; 跨 mod，必须带 mod id 前缀
```

| 规则 | 说明 |
|---|---|
| 路径基准 | 相对**本 mod 根目录**，不是相对当前文件 |
| 扩展名 | **不写** `.scm`（两种拼写会编译出两个互不相干的模块实例） |
| 导出 | `provide`。没写进去的名字在要求方**编译期**就不可见（报 `FreeIdentifier`）；**完全没写 `provide` 的模块什么都不导出** |
| 跨 mod | 必须写 `<mod id>:路径`，且本 mod 的 `mod.json5` 里得先声明 `dependencies: ["<mod id>"]`；本 mod 的模块**不要**写自己的前缀 |
| 传递性 | **没有**。A require B、B require C，A 看不见 C 的导出 |
| 求值次数 | 同一个 mod 内，一个模块只求值一次；模块按 require 图求值，与 `entry_points` 的排列无关 |
| 环 import | 干净报错（`circular dependency found during module resolution`），不挂死 |
| 写法 | 只支持 `(require "模块名")`。`(require (for-syntax "x"))`/`only-in`/`prefix-in` 一律拒绝 |
| 不许 | 绝对路径、`../` 上跳、`require-builtin` 家族 |

**状态是共享的**：同一个 mod 的全部脚本共用一个 VM，模块顶层 `define` 出来的状态被它们共享——不要指望「我 require 一次就拿到一份新的」。跨 mod 才是真副本（对方的源码在**你的** VM 里重新编译一次），也正因为如此，**被跨 mod require 的模块不该带副作用**：它里面的 `register-*` 会以**要求方**的身份注册内容。

**用它把 `entry_points` 收成一条**：依赖顺序写在需要它的那个文件里，比写在清单数组的排列里可靠得多。现货示范见 `mods/lostland/`（`entry_points: ["main.scm"]`）与 `mods/lostland/ids.scm` → `mods/example_mod/gameplay.scm`（跨 mod）。完整设计见 [mod 包结构与资产 VFS](mod-package-structure.md)「六、脚本模块系统」。

## 文档与测试如何保持同步

本文档每一段标「已实测」的代码，对应 `crates/ll-script/tests/steel_syntax_reference.rs` 里同名或本文档明确指名的一个测试函数，运行的是真实 `ScriptEngine`（不是记忆/推断）。选择"测试文件独立存在、靠函数名手工对应"而不是"从文档生成测试"或"从测试生成文档"，是因为：

- 本文档需要大量自然语言解释"为什么"（例如宏为什么安全、mod API 为什么没有生产消费者），生成式方案要么牺牲这些讲解、要么需要一套自定义的文档内嵌测试 DSL——本项目目前没有这类基础设施，引入它本身就是一项新的、超出本次任务范围的工程投入。
- "函数名对应"这条规则足够轻量：升级 `steel-core`、收紧白名单、改动某个 `register_fn` 的参数顺序，只要影响到本文档引用的任何一段代码，`cargo test -p ll-script --test steel_syntax_reference` 会先变红——这就是本文档要求的"文档会自己变红"，不需要额外的生成管线。
- 复查成本低：任何人想确认"这份文档是不是还跟得上代码"，只需要跑一次这一个测试文件，不需要理解一套生成器实现。

**已知的覆盖边界**（见测试文件顶部模块文档，这里不重复）：六个 `register-*` 函数因为跨 crate 依赖方向（`ll-script` 不能依赖 `ll-mod`）无法在本文件里直接验证，改为指向 `ll-mod` 自己已经存在、持续跑在 CI 里的单元测试；`entity-state-set!`/`entity-state-get!` 因为 `ScriptEntityHandle` 的构造函数是 `pub(crate)`，外部集成测试拿不到真实句柄，同样指向 `ll-script` 自己的单元测试。这两类边界不是"没测"，是"测试存在于另一个测试文件/crate 里，本文档如实指明它们的位置"。

## 相关文档

- [ADR 0012 — Steel 标准库能力面实测](../decisions/0012-steel-capability-surface-verification.md) —— 白名单机制、三层防线的完整实测过程
- [ADR 0019 — 每禁一项脚本能力，必须提供确定性替代品，或写明这个需求不正当](../decisions/0019-denied-capability-needs-substitute-or-justification.md) —— 第四节对照表的核实来源
- [ADR 0018 — 脚本层边界按系统类型划分](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) —— 「本体即 Mod」只在玩法层成立
- [ADR 0020 — 脚本内部允许浮点，边界类型把关](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md) —— 脚本内部浮点计算的边界
- [脚本状态存储](script-state-storage.md) —— `state-set!` 系列完整设计（配额、命名空间隔离、孤儿保留策略）
- [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) —— `ScriptEntityHandle` 防伪造论证、批量查询原语清单（本文档第三节「六」引用的逃生舱模式出自这里）
