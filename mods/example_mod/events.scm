;; 示例 mod 的**结算期**事件处理函数。
;;
;; # 这个文件不在 entry_points 里，这是刻意的
;;
;; 它面向的是与装载期完全不同的一套 ScriptEngine（见
;; ll_mod::script_event_source::ScriptEventSource），注册的是
;; event-kind/event-target/event-amount 这类运行期查询函数，不是
;; register-* 那套装载期注册函数。把它塞进 entry_points，装载管线的
;; 引擎会因为认不出这些名字而白名单拒绝——理由与 behavior.scm 逐字
;; 相同，见 mod.json5 里 entry_points 上方那段注释。
;;
;; 它由清单的 event_scripts 字段列出（见
;; ll_mod::manifest::ModManifest::event_scripts 文档），
;; **订阅声明**则在 gameplay.scm 里（`(on-event ...)` 是装载期动作）。
;; 声明与实现因此分居两个文件——不是设计得不好，是那道引擎隔离墙本来
;; 的样子。
;;
;; # 处理函数的调用约定
;;
;; - 零参。事件数据走 (event-kind)/(event-actor)/(event-target)/
;;   (event-amount) 四个零参查询，理由见
;;   ll_script::api::event 模块文档「为什么 payload 走零参查询函数」。
;; - 返回一个**写入列表**（可以是空表 '()，或 #f）。每条写入是：
;;     (list 'global "键" 值)          ;; 写本 mod 的全局脚本状态
;;     (list 'entity 句柄 "键" 值)     ;; 写某个实体上本 mod 的状态
;;   值支持整数/布尔/字符串三种。
;; - 返回值**不是**"已经写好的世界状态"，是一批还没落地的 Effect：
;;   宿主把它们包成一条 Effect::SetScriptState 交给 apply（ADR 0023），
;;   处理函数自己一行世界都写不了。
;;
;; # 已知边界（如实记录，不是遗漏）
;;
;; 结算期引擎**没有**注册 state-get! 一族——本 mod 因此读不到自己上
;; 一次写下的值，只能做"无条件写入"类的事（打标记、记下最后一次事件
;; 的数值）。理由见 ll_mod::script_event_source 模块文档
;; 「为什么不注册 ll_script::api::state」一节：那套 API 的写入路径挂在
;; 一个当前有已知缺陷的 thread_local 上，本批次刻意不建立在它上面。

;; 有人被杀时：在**击杀者**身上记一笔，并在全局记下"最后一次事件"。
;;
;; # 为什么写击杀者而不是死者
;;
;; 反应效果在这一批效果**全部落地之后**才 apply（见
;; ll_sim::turn::TurnEngine::perform 文档），而这一批里就有那条
;; Effect::Kill——等轮到反应效果时，死者已经从世界里销毁了，一条指向
;; 它的 entity 写入会被 apply 静默丢弃。这不是本 API 的缺陷，是"效果
;; 按顺序落地"的直接后果；如实写在这里，免得下一个人踩同一个坑。
;;
;; (event-actor) 在环境致死/坠落致死时是 #f（没有击杀者），那是常态
;; 不是错误，因此这里先判一次。
(define (examplemod-on-kill)
  (let ([killer (event-actor)])
    (if killer
        (list (list 'entity killer "last-kill-seen" #t)
              (list 'global "last-event" (event-kind)))
        (list (list 'global "last-event" (event-kind))))))

;; 有人受伤时：把这一次的伤害量同时记进全局状态与受伤者身上。
;;
;; 受伤者此刻**还活着**（Damage 只做减法，致死是紧随其后的另一条
;; Kill 效果），因此这条 entity 写入落得下去。
;;
;; 只记最后一次而不是累加，正是上面「已知边界」那一节的直接后果——
;; 累加要求先读回上一次的值，而结算期引擎读不到。
(define (examplemod-on-damage)
  (let ([victim (event-target)])
    (if victim
        (list (list 'global "last-damage" (event-amount))
              (list 'entity victim "last-damage-taken" (event-amount)))
        (list (list 'global "last-damage" (event-amount))))))
