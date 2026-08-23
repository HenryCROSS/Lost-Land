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

;; 潜行（潜行与盗贼被动批次）——项目所有者裁定「潜行需要时可切换状态
;; 的」。落在**这一行掷骰**上，不落在视野上：卫兵照常看得见潜行中的
;; 目标（`nearby-actor-in-view` 一个字都没改，`compute_fov`/`VisibleSet`
;; 也没有），只是「要不要把这个人当回事」这次判定的成功率降下来。
;; 完整论证见 crates/ll-script/src/api/actor.rs 模块文档「潜行：为什么
;; 是一次判定的减值，不是一次可见性的改写」一节。
;;
;; `actor-stealthed?` 是本批次新增的运行期查询 API（同一文件），接一个
;; 目标句柄——问的是「我看到的这个人在不在潜行」，不是「我自己」。
;;
;; 两个概率都是这份脚本里的普通字面量，与 GUARD_INSPECT_CHANCE_PERMILLE
;; 当初同一条可编辑性（ADR 0018：这段逻辑该不该暴露给 mod 重新定义
;; ——盘查触发率与潜行的减免幅度显然都该）。mod 作者想让潜行完全免疫
;; 盘查就把下面这个数改成 0，想让潜行毫无用处就改成 500，不需要动
;; 任何 Rust 代码。
(define GUARD_INSPECT_CHANCE_PERMILLE 500)
(define GUARD_INSPECT_CHANCE_PERMILLE_STEALTHED 50)

;; 盗贼被动两分批次——项目所有者裁定「被动可以分为 2 种，不觉得可疑，
;; 还有查不出东西」。**前一种落在这一行**：`actor-inspection-suspicion`
;; 是本批次新增的运行期查询 API（crates/ll-mod/src/script_behavior_api.rs），
;; 返回目标此刻的「盘查意愿」千分比乘数——1000 表示与常人无异，更小表示
;; 卫兵更不容易起疑。它的值来自目标身上聚合出的
;; RuleModifier::InspectionSuspicion（天赋/装备两路来源都算，走
;; ll_sim::rule_modifier::agent_rule_modifiers 这个唯一聚合点）。
;;
;; **后一种（查不出东西）不在这份脚本里**：那一路是「盘查照常发起，
;; 只是搜不出东西」，判定在 ll_sim::resolve::resolve_inspect（见
;; RuleModifier::InspectionConcealment 文档）。两个被动分别落在链路的
;; 两环，正是所有者「分为 2 种」这句裁定的形状。
;;
;; 潜行与被动①是**相乘**，不是二选一：两者回答的是不同的问题（这一刻
;; 我藏没藏起来 vs 我这个人天生多不起眼），一个盗贼在潜行时理应两者都
;; 生效。整数乘除，先乘后除，与本项目「百分比一律千分比、全程整数」的
;; 既有纪律一致（ADR 0020 乙区）。
(define (guard-inspect-chance target)
  (quotient (* (if (actor-stealthed? target)
                   GUARD_INSPECT_CHANCE_PERMILLE_STEALTHED
                   GUARD_INSPECT_CHANCE_PERMILLE)
               (actor-inspection-suspicion target))
            1000))

(define (guard-try-inspect)
  (if (self-has-profession? "lostland:guard")
      (let ([target (nearby-actor-in-view)])
        (if (and target (rng-chance (guard-inspect-chance target)))
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
