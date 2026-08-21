;; 迷途大陆示例 mod：一个真的会用技能的敌人（哥布林法师）——规格
;; §10.5 行为树接线批次。落地「S 表达式本身即树结构」：`goblin-ai-tree`
;; 返回一份被 `quote` 包起来、从未被 Steel 自己求值的列表，Rust 侧的
;; tick 求值器（crates/ll-script/src/behavior.rs）按 selector 语义
;; 遍历它，命中的分支才真正调用对应的 Steel 函数。
;;
;; 决策优先级（selector 从上到下试，第一个成功的分支胜出）：
;;   1. 附近有敌人且冰霜箭技能可用 → 施放冰霜箭
;;   2. 附近有敌人（技能不可用）   → 普通近战攻击
;;   3. 都不满足                  → 有敌人则走近一步，否则原地等待
;;
;; 注意：**本文件不在 mod.json5 的 entry_points 里**——`nearby-enemy`/
;; `skill-ready?`/`direction-toward` 是运行期查询 API（由
;; `ll_mod::script_behavior_source::ScriptBehaviorSource` 在构造它
;; 自己的 `ScriptEngine` 时注册），不是 mod 装载管线
;; （`ll_mod::pipeline::load_all`）为六类内容注册函数（register-terrain
;; 等）准备的那个 `ScriptEngine`——两者是职责不同的两套引擎，装载管线
;; 的引擎没有注册这些运行期查询函数，把本文件塞进 entry_points 只会
;; 让白名单拒绝一个它认不出的函数名。真正加载/执行本文件的路径是
;; `ScriptBehaviorSource::new`，见其模块文档。

(define (goblin-try-skill)
  (let ([enemy (nearby-enemy)])
    (if (and enemy (skill-ready? "examplemod:frostbolt"))
        (list 'use-skill "examplemod:frostbolt" enemy)
        #f)))

(define (goblin-try-attack)
  (let ([enemy (nearby-enemy)])
    (if enemy (list 'attack enemy) #f)))

(define (goblin-try-approach)
  (let ([enemy (nearby-enemy)])
    (if enemy (list 'move (direction-toward enemy)) 'wait)))

(define (goblin-ai-tree)
  (quote (selector
           (goblin-try-skill)
           (goblin-try-attack)
           (goblin-try-approach))))
