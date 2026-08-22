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

;; 卫兵盘查（卫兵职业接线批次）——项目所有者原话「卫兵职业的单位有
;; 概率会来核查其他单位身上的物品」。落地成一棵独立的行为树
;; （`guard-ai-tree`），不是塞进上面哥布林那棵：卫兵与哥布林是两种
;; 完全不同的行为模式,共用一棵树只会让 selector 分支互相污染。
;;
;; 概率本身——GUARD_INSPECT_CHANCE_PERMILLE——就写在这份脚本里，走
;; 已有的 `rng-chance` 原语（crates/ll-script/src/api/rng.rs）；mod
;; 作者要调这个概率，直接改这一个数字即可，不需要任何新的注册函数/
;; 内容表：整棵行为树本身就是玩法内容,概率只是其中一个字面量,与
;; 其余分支同一份可编辑性（ADR 0018 判定「这段逻辑该不该暴露给 mod
;; 重新定义」——盘查触发率显然该）。
;;
;; `self-has-profession?`/`nearby-actor-in-view` 是本批次新增的运行期
;; 查询 API：
;;   - self-has-profession?  crates/ll-mod/src/script_behavior_api.rs
;;     ——把 Agent.profession 与命名空间字符串的比对暴露给脚本,让这棵
;;     树只对卫兵职业的实体生效（与 skill-ready? 同一接线手法）。
;;   - nearby-actor-in-view  crates/ll-script/src/api/actor.rs
;;     ——两段式过滤（chebyshev 粗筛 + compute_fov 成员测试）算出「视野
;;     内离自己最近的任意实体」,不看敌对关系（区别于 nearby-enemy）,
;;     隔着墙的目标不会被找到。

(define GUARD_INSPECT_CHANCE_PERMILLE 500)

(define (guard-try-inspect)
  (if (self-has-profession? "lostland:guard")
      (let ([target (nearby-actor-in-view)])
        (if (and target (rng-chance GUARD_INSPECT_CHANCE_PERMILLE))
            (list 'inspect target)
            #f))
      #f))

(define (guard-try-approach)
  (let ([target (nearby-actor-in-view)])
    (if target (list 'move (direction-toward target)) 'wait)))

(define (guard-ai-tree)
  (quote (selector
           (guard-try-inspect)
           (guard-try-approach))))
