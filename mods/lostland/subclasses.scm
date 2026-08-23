;; 本体副职内容：六条——四条制作类 + 剑舞者/学徒。
;;
;; 签名见 crates/ll-mod/src/script_subclass_api.rs：
;;   (register-subclass id display-name-key)
;;   (register-subclass-unlock subclass-id trigger-kind trigger-target threshold)
;;
;; trigger-kind 目前只接受 "items-crafted"，trigger-target 是一个已经
;; 通过 register-recipe-category 注册过的配方类别 id。传别的 kind 会
;; 当场报错并列出支持的取值，不会被静默当成制作。
;;
;; **本文件依赖 crafting.scm**：register-subclass-unlock 要求
;; trigger-target 指向的类别已经注册，只 get 不 intern。这条依赖由本
;; 文件下方那句 `(require "crafting")` 表达，不再是 mod.json5 里
;; entry_points 的排序约定。
;;
;; # 为什么本体副职只有制作类这四个
;;
;; knowledge/design/subclass-system.md 六节的名册有九条，本文件只落
;; 其中四条。判据是**挂钩动作在代码里存不存在**，不是名册长度：
;;
;; - 制作类四个（本文件）：闸门（resolve_craft 第③步）与成长挂钩
;;   （craft_progress_effects）**都已经是真代码**。
;; - 采集：resolve_pick_up/resolve_loot 两个挂载点确实已落地，但它的
;;   触发器 ItemsGathered 要指向「物品类别」——那个内容表在代码里
;;   **根本不存在**（ItemDef 没有任何「类别」字段）。造一张物品类别表
;;   是一整个独立批次。
;; - 求生：resolve_rest 已落地，触发器不需要任何新内容表，但本批次
;;   没有任何副职消费它——为一个没有消费者的触发器造变体是 ADR 0021
;;   点名要避免的抽象。
;; - 营造/驭兽/行商/学者：分别阻塞在 Intent::Build、同伴系统、
;;   Intent::Trade、「阅读/研究」这四个尚未落地的东西上。
;;
;; **注册一个拿不到的副职比不注册它更糟**：它会出现在将来的角色面板
;; 与存档里，而玩家永远达不到它的条件。因此上面五条一条都不注册。
;;
;; # 剑舞者/学徒两条是本批次迁进来的，与上面那条纪律不矛盾
;;
;; 这两条此前写在 `ll_mod::subclass::materialize_base_subclasses` 里，
;; 与职业/技能/任务处境完全相同：那个函数**从来不在生产装载路径上**
;; （详见 classes.scm 文件头同名一节），本批次按项目所有者「迁移吧」
;; 的裁定一并搬进来。
;;
;; 上面那条纪律说的是「**不要往名册里新增**没有挂钩动作的副职」——
;; 判据是「要不要造一条新内容」。这两条不是新造的，是**已经存在、
;; 只是从没被装载过**的存量内容，把它们删掉才是内容损失。
;;
;; **但必须如实说清楚一件事**：这两条目前**玩家拿不到**。
;; `Effect::GrantSubclass` 在整个 `ll-sim` 里只有一个产出点
;; （`ll_sim::subclass` 的制作计数达标那一路），而 `register-subclass-unlock`
;; 的 trigger-kind 至今只接受 "items-crafted"。给一个近战副职配一条
;; 「做满 N 件东西」的获得条件是荒唐的，因此本文件**不**给它们编造
;; 获得条件——宁可让「拿不到」是一条写下来的、可查的缺口，也不要一条
;; 语义不对的内容。
;;
;; 补这个缺口的正确形状是给 `SubclassUnlockTrigger` 增开新的触发器
;; 种类（例如「用某类武器命中 N 次」），而那要求 `ll_sim` 侧先有对应
;; 的挂钩动作与计数——是一个独立批次，不夹带在内容迁移里。同一个
;; 判据下，本文件仍然不给设计文档名册里那五条没有挂钩动作的副职开门。
;;
;; # 上限与放弃
;;
;; 一个角色最多同时持有 ll_sim::subclass::MAX_SUBCLASSES（当前 3）个
;; 副职。满员时使用计数达标**只拒绝授予、不吞掉计数**：进度照常累加，
;; 玩家经 Intent::AbandonSubclass 放弃一个腾出槽位之后，下一次在那个
;; 类别里制作就会当场补发（判据是「累计 >= 阈值」，不是「恰好等于」）。
;;
;; **放弃有一个立刻生效的代价**：设了副职闸门的配方类别，放弃的那一刻
;; 起就做不了了（resolve_craft 每次制作都重判）。这与技能那一路的
;; 「学会了就永远能用」相反——两种闸门的语义本来就不同，见
;; crates/ll-sim/src/effect.rs 里 Effect::RemoveSubclass 的文档。
;;
;; # 阈值为什么是这几个数
;;
;; 20 / 20 / 15 / 15：没有任何数据支撑，是四个占位数值，量级取「玩家
;; 认真做一段时间某类东西」而不是「顺手做两下」。它们是纯内容参数，改
;; 它们一行 Rust 都不用动。真正需要平衡的时候，判据应当是实际游玩节奏
;; 下拿到第一个副职需要多久。
;;
;; # 副职目前不给任何东西
;;
;; SubclassDef 只有 id 与 display_name_key 两个字段。设计文档二节说的
;; 「唯一的『给东西』字段 traits: Vec<TraitGrant>」**在代码里还不存在**
;; ——register-subclass-trait 这条路还没通。因此这四个副职今天的全部
;; 作用是「资格」：它们能被 recipe-category-requires-subclass! 引用当
;; 闸门（example_mod 已经这么用了）。天赋授予是下一批的事。

;; 依赖写在代码里，不写在清单顺序里：register-subclass-unlock 的
;; trigger-target 指向的配方类别必须**已经注册**（只 get 不 intern），
;; 这一句 require 就是那个保证。此前它是 mod.json5 里
;; entry_points 的一条排序约定——写在清单里的顺序看不出「为什么」，
;; 改错了也只有装载期才发现；写成 require 之后，依赖就在需要它的那个
;; 文件里，谁都改不掉。
(require "crafting")

;; 工匠：锻造金属装备与工具。
(register-subclass "lostland:artisan" "lostland:subclass.artisan.display_name")
(register-subclass-unlock "lostland:artisan" "items-crafted" "lostland:forging" 20)

;; 裁缝：缝制衣物与织物。
(register-subclass "lostland:tailor" "lostland:subclass.tailor.display_name")
(register-subclass-unlock "lostland:tailor" "items-crafted" "lostland:tailoring" 20)

;; 炼金术士：调配药剂与试剂。
;; 与厨师是**两个独立副职**，见 crafting.scm 里那条裁定的完整记录。
(register-subclass "lostland:alchemist" "lostland:subclass.alchemist.display_name")
(register-subclass-unlock "lostland:alchemist" "items-crafted" "lostland:alchemy" 15)

;; 厨师：烹饪食物。
(register-subclass "lostland:cook" "lostland:subclass.cook.display_name")
(register-subclass-unlock "lostland:cook" "items-crafted" "lostland:cooking" 15)

;; # 以下两条来自 `materialize_base_subclasses`，见文件头同名一节
;;
;; 两条都**不**声明获得条件（register-subclass-unlock）——理由见文件头：
;; 现有的唯一触发器种类 "items-crafted" 对它们语义不对。

;; 剑舞者：轻装近战副职。
(register-subclass "lostland:duelist" "lostland:subclass.duelist.display_name")

;; 学徒：可搭配任意主职的通用魔法副职。
(register-subclass "lostland:apprentice" "lostland:subclass.apprentice.display_name")
