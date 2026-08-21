;; 迷途大陆示例 mod：职业、技能、副职、任务、种族——证明这五类玩法层
;; 内容现在与地形一样，脚本真的能调用对应的 register-* 函数注册它们
;; （P5-C 缺口修补批次新增，补上 ADR 0018 判定为玩法层、但此前只有
;; 纯 Rust 函数调用能触达的注册 API）。
;;
;; 每个 register-* 的签名见对应宿主模块的文档：
;;   register-class            crates/ll-mod/src/script_class_api.rs
;;   register-subclass         crates/ll-mod/src/script_subclass_api.rs
;;   register-skill            crates/ll-mod/src/script_skill_api.rs
;;   register-quest            crates/ll-mod/src/script_quest_api.rs
;;   register-race             crates/ll-mod/src/script_race_api.rs
;;   register-race-xp-reward   crates/ll-mod/src/script_race_api.rs
;;   register-xp-curve         crates/ll-mod/src/script_xp_curve_api.rs
;;   register-class-xp-curve   crates/ll-mod/src/script_xp_curve_api.rs
;;   register-race-xp-curve    crates/ll-mod/src/script_xp_curve_api.rs
;;   register-trait            crates/ll-mod/src/script_trait_api.rs
;;   register-race-trait       crates/ll-mod/src/script_race_api.rs
;;   register-resource-pool         crates/ll-mod/src/script_resource_pool_api.rs
;;   register-trait-resource-pool   crates/ll-mod/src/script_trait_api.rs

;; 一个新职业：亡灵法师，意志向。
(register-class "examplemod:necromancer" "examplemod:necromancer_display_name" "willpower")

;; 一个新副职：暗影舞者。
(register-subclass "examplemod:shadowdancer" "examplemod:shadowdancer_display_name")

;; 一个新技能：冰霜箭——消耗 12 点法力，冷却 25 tick，造成 15 点伤害。
(register-skill "examplemod:frostbolt" "" (list) 25 "mana" 12 "deal-damage" "" 15 0)

;; 一个新任务：击杀 3 只哥布林。
(register-quest "examplemod:kill_goblins" (list) "kill-count" "examplemod:goblin" 3)

;; 一个新种族：半精灵——敏捷 +1、魅力 +1，寿命 150 年。
(register-race "examplemod:half_elf" "examplemod:half_elf_display_name" 0 1 0 0 0 1 0 1 1 150)

;; 另一个新种族：哥布林——上面 "examplemod:kill_goblins" 任务点名的
;; 击杀目标，此前只是一个被 kill-count 匹配规则引用的裸字符串，从未
;; 真正注册过（种族本身是否存在不影响击杀计数匹配，见
;; crate::quest 模块文档「跨表引用」一节）。这里补上真实注册，并用
;; register-race-xp-reward 声明"杀死一只哥布林给 15 点经验"——见
;; crate::race 模块文档 RaceDef::xp_reward 一节：等级与经验系统落地
;; 批次判断"生物值多少经验"落在种族表上，这两行就是那个判断的真实
;; 落地证据，不是只在单元测试里自证。
(register-race "examplemod:goblin" "examplemod:goblin_display_name" 0 0 0 0 0 0 0 1 1 5)
(register-race-xp-reward "examplemod:goblin" 15)

;; 两条形状截然不同的经验曲线（等级与经验系统落地批次新增,
;; knowledge/design/level-and-experience-system.md 四节两条示例的真实
;; 版本）——线性曲线完全不读 prev-requirement，递推曲线完全依赖它，
;; 证明"不同公式"不是同一套算法调了两个系数。
;;
;; 线性：从 N 级升到 N+1 级需要 100 + 40*N 点经验，只读 level。
(register-xp-curve "examplemod:linear_xp_curve" 140
  (quote (+ 100 (* level 40))))

;; 递推指数：下一级门槛 = max(上一级门槛+20, 上一级门槛×1.18)——早期
;; 由加法分支主导、后期由千分比乘法分支主导，只读 prev-requirement。
(register-xp-curve "examplemod:recursive_xp_curve" 80
  (quote (max (+ prev-requirement 20) (mul-permille prev-requirement 1180))))

;; 把两条曲线分别绑定给一个职业与一个种族——证明
;; register-class-xp-curve/register-race-xp-curve 两个"配置与定义
;; 分离"的绑定函数在完整装载管线里真的生效，不只是孤立的单元测试。
(register-class-xp-curve "examplemod:necromancer" "examplemod:recursive_xp_curve")
(register-race-xp-curve "examplemod:half_elf" "examplemod:linear_xp_curve")

;; 天赋系统落地批次：龙裔吐息——knowledge/design/trait-system.md 九节
;; 示例二「种族授予技能」的真实版本。吐息武器是一个不消耗任何资源、
;; 造成 20 点伤害的技能；"龙裔吐息"这个天赋授予它；龙裔种族在 1 级
;; （unlock_level=1，"拥有即生效"，见 trait-system.md 六节）被授予这个
;; 天赋——三行连起来证明 resolve_use_skill 门一真的会去读种族天赋的
;; 授予技能并集，不是只在单元测试里自证（ADR 0018）。
(register-skill "examplemod:breath_weapon" "" (list) 30 "none" 0 "deal-damage" "" 20 0)
(register-trait "examplemod:draconic_breath" "examplemod:draconic_breath_display_name"
  (list "examplemod:breath_weapon"))
(register-race "examplemod:dragonborn" "examplemod:dragonborn_display_name" 0 0 0 0 0 0 0 1 1 80)
(register-race-trait "examplemod:dragonborn" "examplemod:draconic_breath" 1)

;; 资源池落地批次（第一批：法力池/血池）：knowledge/design/resource-pools-and-rest.md
;; 十一节「法师验收示例」版本二（法力池，术士式施法者）的真实版本——
;; 一个标量法力池 + 一个授予它固定 20 点容量的天赋 + 一个消耗法力的
;; 技能，三行连起来证明 register-resource-pool/register-trait-resource-pool
;; 与 effective_scalar_capacity/resolve_use_skill 门四在完整装载管线里
;; 真的接通，不是只在单元测试里自证（ADR 0018）。每回合自动回复 2
;; 点——同时验收 RegenRule::OnTurnStart。
(register-resource-pool "examplemod:sorcery_points" "examplemod:sorcery_points_display_name"
  "scalar" "on-turn-start" 2)
(register-trait "examplemod:innate_sorcery" "examplemod:innate_sorcery_display_name" (list))
(register-trait-resource-pool "examplemod:innate_sorcery" "examplemod:sorcery_points" "fixed" 20)
(register-race-trait "examplemod:half_elf" "examplemod:innate_sorcery" 1)
(register-skill "examplemod:sorcerer_firebolt" "" (list) 10
  "examplemod:sorcery_points" 5 "deal-damage" "" 12 0)

;; 血法师附加示例（`resource-pools-and-rest.md` 十一节「血法师」）：
;; 直接扣 15 点生命值,绕开减伤/抗性,不需要任何天赋授予"使用许可"——
;; granted_skills（既有第一类天赋效果）已经完整覆盖了这层，见该节原文。
(register-skill "examplemod:blood_bolt" "" (list) 10 "blood" 15 "deal-damage" "" 30 0)
