;; 本体任务内容：四条，构成一张**网状**（不是树状）的任务图。
;;
;; 签名见 crates/ll-mod/src/script_quest_api.rs：
;;   (register-quest id prerequisites condition-kind condition-arg
;;                   condition-count)
;;
;; - prerequisites：前置任务节点标识符字符串的列表。
;; - condition-kind："kill-count"/"script"。
;; - condition-arg："kill-count" 时是目标敌人类型的标识符；"script"
;;   时是脚本回调标识符。
;; - condition-count："kill-count" 时是击杀数量；"script" 时忽略。
;;
;; # 这四条是本批次从 Rust 迁进来的
;;
;; 它们此前写在 `ll_mod::quest::materialize_base_quests` 里，与职业/
;; 技能处境完全相同：那个函数**从来不在生产装载路径上**。详细记录见
;; classes.scm 文件头同名一节。
;;
;; # 图的形状是刻意的（验收「网而不是树」）
;;
;;   main_quest_1 ──┬── branch_a ──┐
;;                  └── branch_b ──┴── finale
;;
;; - 「一个前置解锁多个后续」：main_quest_1 同时解锁两条分支。
;; - 「一个任务有多个前置」：finale 要求两条分支都完成——finale 有
;;   两个父节点，因此这张图不是树。
;;
;; 同时演示两档完成条件：三条用 kill-count（一档），branch_b 用
;; script（三档）。
;;
;; # lostland:goblin 与 lostland:branch_b_condition 是开放标识符
;;
;; 前者指向「敌人类型」，代码库至今没有敌人类型注册表——
;; `ll_mod::content_audit::ReferenceExpectation::UntypedIdSpace` 正是
;; 为这种情形留的豁免出口，把它按「必须在某张内容表里已定义」检查会
;; 把一条**正确的设计**判成错误，见 `ll_mod::quest` 模块文档「跨表
;; 引用」一节。
;;
;; 后者是 `QuestCondition::Script` 携带的脚本回调标识符。**必须如实
;; 说清楚**：求值它指向的回调是**尚未落地**的能力——`ll_sim` 的任务
;; 完成判定当前只处理 KillCount 变体，Script 变体目前只是一个携带
;; 命名空间 id 的数据标签。这条内容存在的意义是把「三档条件能被注册、
;; 能被存档、能被查询」这半条链路钉住，不是宣称脚本回调已经能跑。
;;
;; # 环检查现在真的会跑
;;
;; 同 skills.scm 文件头同名一节：`ll_mod::quest::validate_no_cycles`
;; 此前唯一的调用点在 materialize_base_quests 内部，mod 注册的任务从
;; 来没有被环检查覆盖过；本批次把它接到了 `load_content` 上。

;; 起点任务：击杀 3 个哥布林。无前置。
(register-quest "lostland:main_quest_1" '() "kill-count" "lostland:goblin" 3)

;; 分支之一（一档条件）：击杀 5 个哥布林。
(register-quest "lostland:branch_a" '("lostland:main_quest_1") "kill-count" "lostland:goblin" 5)

;; 分支之二（三档条件）：脚本回调判定，见文件头对 Script 档现状的说明。
(register-quest "lostland:branch_b" '("lostland:main_quest_1") "script" "lostland:branch_b_condition" 0)

;; 汇聚任务：两条分支都完成后才解锁。
(register-quest "lostland:finale" '("lostland:branch_a" "lostland:branch_b") "kill-count" "lostland:goblin" 1)
