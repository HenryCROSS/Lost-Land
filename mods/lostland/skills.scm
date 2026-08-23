;; 本体技能内容：五条，构成一棵有分支、再汇聚的技能树。
;;
;; 签名见 crates/ll-mod/src/script_skill_api.rs：
;;   (register-skill id owning-class prerequisites cooldown-ticks
;;                   resource-kind resource-amount
;;                   effect-kind effect-tag effect-amount effect-amount2)
;;
;; - owning-class：完整标识符字符串；空串 "" 表示通用技能
;;   （owning_class: None）。
;; - prerequisites：前置技能标识符字符串的列表。
;; - resource-kind："none"/"mana"/"stamina"/"blood"，或一个已注册的
;;   标量资源池 id，或 "slot-tier:<pool-id>"。
;; - effect-kind："deal-damage"/"restore-resource"/
;;   "temporary-stat-modifier"，effect-tag/amount/amount2 按 kind 解释。
;;
;; # 这五条是本批次从 Rust 迁进来的
;;
;; 它们此前写在 `ll_mod::skill::materialize_base_skills` 里，与职业
;; 那三条处境完全相同：那个函数**从来不在生产装载路径上**，真实游戏
;; 里此前一条本体技能都没有。详细记录见 classes.scm 文件头同名一节。
;;
;; # 树的形状是刻意的（验收「树而不是线性序列」）
;;
;; knowledge/design/class-skill-quest-system.md 第二节要求技能树能表达
;; 两件事，这五条各自是其中一件的证据：
;;
;;   strike ──┬── power_strike ──┐
;;            ├── brace ─────────┴── combo
;;            └── focus
;;
;; - 「一个前置解锁多个后续」：strike 同时解锁三条分支。
;; - 「一个技能有多个前置」：combo 要求 power_strike 与 brace 都满足
;;   ——这一点单靠"树"表达不了（树里每个节点只有一个父节点）。
;;
;; focus 刻意声明为通用技能（owning-class 传空串），演示"不专属任何
;; 职业"这一类；其余四条属于 lostland:warrior。
;;
;; # 环检查现在真的会跑
;;
;; `ll_mod::skill::validate_no_cycles` 此前唯一的调用点在
;; materialize_base_skills 内部，也就是说**mod 注册的技能从来没有被
;; 环检查覆盖过**。本批次把它接到了 `ll_game::content::load_content`
;; 上（全部 mod 装载完毕之后跑一次，覆盖本体 + 全部 mod 的合并结果），
;; 前置成环或指向一条谁都没注册过的技能，现在都是一条会让装载整批
;; 失败的、点名具体 id 的错误。
;;
;; # 顺序：本文件必须排在 classes.scm 之后
;;
;; 四条战士技能的 owning-class 指向 lostland:warrior，理由见
;; classes.scm 文件头最后一节。
;;
;; # 数值
;;
;; 冷却与消耗（0/20/15/10/30 与 stamina 10/5/15）是迁移前那份数值的
;; 逐字复制，不是本批次新定的平衡——真正的职业/技能数值平衡不在本
;; 任务范围（设计文档开篇已声明只交付系统骨架）。

;; 起点技能：基础打击。无前置、无冷却、无消耗。
(register-skill "lostland:strike" "lostland:warrior" '()
                0 "none" 0
                "deal-damage" "" 5 0)

;; 分支之一：强力打击，更高伤害，耗体力。
(register-skill "lostland:power_strike" "lostland:warrior" '("lostland:strike")
                20 "stamina" 10
                "deal-damage" "" 12 0)

;; 分支之二：格挡姿态，临时提升体质 10 tick。
(register-skill "lostland:brace" "lostland:warrior" '("lostland:strike")
                15 "stamina" 5
                "temporary-stat-modifier" "constitution" 3 10)

;; 分支之三：凝神，恢复法力。**通用技能**——owning-class 传空串。
(register-skill "lostland:focus" "" '("lostland:strike")
                10 "none" 0
                "restore-resource" "mana" 8 0)

;; 汇聚技能：连击，要求 power_strike 与 brace 两个前置同时满足。
(register-skill "lostland:combo" "lostland:warrior" '("lostland:power_strike" "lostland:brace")
                30 "stamina" 15
                "deal-damage" "" 20 0)
