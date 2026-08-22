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
;;   register-class-trait      crates/ll-mod/src/script_class_api.rs
;;   register-resource-pool                crates/ll-mod/src/script_resource_pool_api.rs
;;   register-trait-resource-pool          crates/ll-mod/src/script_trait_api.rs
;;   register-trait-resource-pool-by-level crates/ll-mod/src/script_trait_api.rs
;;   register-item                         crates/ll-mod/src/script_item_api.rs
;;   register-item-equip-mask              crates/ll-mod/src/script_item_api.rs
;;   register-item-stat-bonus              crates/ll-mod/src/script_item_api.rs
;;   register-item-use-effect              crates/ll-mod/src/script_item_api.rs
;;   register-item-penetration             crates/ll-mod/src/script_item_api.rs
;;   register-race-starting-item           crates/ll-mod/src/script_race_api.rs
;;   register-damage-formula               crates/ll-mod/src/script_damage_formula_api.rs
;;   register-item-damage-formula          crates/ll-mod/src/script_item_api.rs
;;   register-weapon-category               crates/ll-mod/src/script_weapon_category_api.rs
;;   register-damage-category               crates/ll-mod/src/script_damage_category_api.rs
;;   register-trait-resistance              crates/ll-mod/src/script_trait_api.rs
;;   register-item-damage-category          crates/ll-mod/src/script_item_api.rs
;;   register-item-resistance              crates/ll-mod/src/script_item_api.rs
;;   register-trait-sneak-attack             crates/ll-mod/src/script_trait_api.rs
;;   register-recipe-category                crates/ll-mod/src/script_recipe_category_api.rs
;;   recipe-category-requires-subclass!      crates/ll-mod/src/script_recipe_category_api.rs
;;   register-recipe                         crates/ll-mod/src/script_recipe_api.rs
;;   recipe-requires-station!                crates/ll-mod/src/script_recipe_api.rs
;;   recipe-requires-tool!                   crates/ll-mod/src/script_recipe_api.rs
;;   register-subclass-unlock                crates/ll-mod/src/script_subclass_api.rs

;; 一个新职业：亡灵法师，意志向。
(register-class "examplemod:necromancer" "examplemod:necromancer_display_name" "willpower")

;; 一个新副职：暗影舞者。它的获得条件（register-subclass-unlock）写在
;; 下面配方类别那一段里，不写在这里——那个函数要求 trigger-target
;; 指向的配方类别**已经注册**，而 examplemod:cooking 在本文件靠后的
;; 位置才登记。同一个文件里的顺序依赖，与 recipe-requires-station!
;; 必须排在对应 register-recipe 之后是同一回事。
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
  "scalar" 0 "on-turn-start" 2)
(register-trait "examplemod:innate_sorcery" "examplemod:innate_sorcery_display_name" (list))
(register-trait-resource-pool "examplemod:innate_sorcery" "examplemod:sorcery_points" "fixed" 20)
(register-race-trait "examplemod:half_elf" "examplemod:innate_sorcery" 1)
(register-skill "examplemod:sorcerer_firebolt" "" (list) 10
  "examplemod:sorcery_points" 5 "deal-damage" "" 12 0)

;; 血法师附加示例（`resource-pools-and-rest.md` 十一节「血法师」）：
;; 直接扣 15 点生命值,绕开减伤/抗性,不需要任何天赋授予"使用许可"——
;; granted_skills（既有第一类天赋效果）已经完整覆盖了这层，见该节原文。
(register-skill "examplemod:blood_bolt" "" (list) 10 "blood" 15 "deal-damage" "" 30 0)

;; 法术位落地批次（第二批）：knowledge/design/resource-pools-and-rest.md
;; 十一节「法师验收示例」版本一（法术位，D&D 式法师）的真实版本——一个
;; 四档法术位池 + 一个按等级授予档位分布的天赋 + 一个消耗三档法术位的
;; 技能，四行连起来证明 register-resource-pool 的 "tiered-slots" 形状/
;; register-trait-resource-pool-by-level 与
;; effective_slot_tier_capacity/resolve_use_skill 门四在完整装载管线里
;; 真的接通，不是只在单元测试里自证（ADR 0018）。休息完成时回满——
;; 同时验收 RegenRule::OnRest(Full)，与上面术士的 sorcery_points
;; （标量池 + RegenRule::OnTurnStart）合在一起，是「两种流派」这条设计
;; 目标（`resource-pools-and-rest.md` 零节第五轮）的真实落地证据：一个
;; 靠休息配给、一个靠回合缓慢回复,玩起来真的不一样。
(register-resource-pool "examplemod:wizard_spell_slots" "examplemod:wizard_spell_slots_display_name"
  "tiered-slots" 4 "on-rest-full" 0)
(register-trait "examplemod:arcane_casting" "examplemod:arcane_casting_display_name" (list))
;; 1 级：两个一环位；5 级追加：四个一环位、三个二环位；9 级追加：两个
;; 三环位——阶梯式增长，未覆盖的等级取小于等于它的最大已声明档位
;; （`CapacityFormula::ByLevel` 文档）。
(register-trait-resource-pool-by-level "examplemod:arcane_casting" "examplemod:wizard_spell_slots" 1
  (list 2 0 0 0))
(register-trait-resource-pool-by-level "examplemod:arcane_casting" "examplemod:wizard_spell_slots" 5
  (list 4 3 0 0))
(register-trait-resource-pool-by-level "examplemod:arcane_casting" "examplemod:wizard_spell_slots" 9
  (list 4 3 2 0))
(register-race "examplemod:elf" "examplemod:elf_display_name" 0 1 0 1 0 0 0 1 1 700)
(register-race-trait "examplemod:elf" "examplemod:arcane_casting" 1)
;; 火球术：消耗三环位（或更高档，单向可兑换）——三环位直到 9 级断点
;; 才出现（见上面），因此 1~8 级的精灵法师放不出这个技能（门四会因为
;; 找不到任何 ≥3 档且有空位的档位而拒绝，见 `resolve_use_skill`
;; 文档），这正是"三环法术不能用一环位放"这条单向兑换规则在真实内容
;; 上的体现；9 级起真的能放出来,不需要任何额外校验。
(register-skill "examplemod:fireball" "" (list) 30
  "slot-tier:examplemod:wizard_spell_slots" 3 "deal-damage" "" 28 0)

;; 反常组合（验证 RegenRule 与 ResourcePoolShape 正交，
;; `resource-pools-and-rest.md` 四节）：法术位配「每回合缓慢恢复」而
;; 不是「休息回满」——同样的 TieredSlots 形状，恢复节奏反过来，证明
;; 引擎不在任何地方假设"位就该长休回满、池就该缓回"。设计文档四节
;; 「反过来的组合一」的真实版本。
(register-resource-pool "examplemod:druid_slots" "examplemod:druid_slots_display_name"
  "tiered-slots" 3 "on-turn-start" 1)
(register-trait "examplemod:druidic_casting" "examplemod:druidic_casting_display_name" (list))
(register-trait-resource-pool-by-level "examplemod:druidic_casting" "examplemod:druid_slots" 1
  (list 3 0 0))
(register-race "examplemod:gnome" "examplemod:gnome_display_name" 0 1 0 1 0 0 0 1 1 400)
(register-race-trait "examplemod:gnome" "examplemod:druidic_casting" 1)

;; P6 第一批（物品基础）：knowledge/design/item-system.md 二节「堆叠
;; 规则」的真实版本——一种可堆叠物品（箭矢，堆叠上限 99，没有耐久
;; 概念）与一种不可堆叠物品（铁剑，堆叠上限 1，带 100 点耐久上限），
;; 证明 register-item 这个新脚本 API 真的能被 mod 脚本调用，且两种
;; 堆叠形状都被正确注册——ADR 0018「玩法层内容必须能从 mod 脚本注册，
;; 且要有真实 mod 脚本为证」，crates/ll-mod/tests/example_mod_items.rs
;; 是那份证据，不能靠单元测试自证。基础重量/价格是 Milli（千分之一为
;; 单位）的原始整数，见 register-item 文档。
(register-item "examplemod:arrow" "examplemod:arrow_display_name" 99 50 2000 -1)
(register-item "examplemod:iron_sword" "examplemod:iron_sword_display_name" 1 3000 50000 100)

;; P6 第三批（装备槽位）：knowledge/design/equipment-slots.md「一条
;; 规则覆盖所有特例」一节的真实版本——一件占用两个槽位的双手武器
;; （战锤，同时占主手与副手）与一件只占一个槽位的单手装备（木盾，
;; 只占副手），证明 register-item-equip-mask 这个新脚本 API 真的能被
;; mod 脚本调用，且占位冲突判定（装备战锤会连带卸下已装备的木盾，
;; 反之亦然）在真实注册的内容上成立——ADR 0018「玩法层内容必须能从
;; mod 脚本注册，且要有真实 mod 脚本为证」，
;; crates/ll-mod/tests/example_mod_equipment.rs 是那份证据。
(register-item "examplemod:war_hammer" "examplemod:war_hammer_display_name" 1 6000 90000 150)
(register-item-equip-mask "examplemod:war_hammer" (list "main-hand" "off-hand"))
(register-item "examplemod:wooden_shield" "examplemod:wooden_shield_display_name" 1 4000 12000 80)
(register-item-equip-mask "examplemod:wooden_shield" (list "off-hand"))

;; P6 第四批（derive_stats 与装备属性接进战斗）：knowledge/design/
;; attribute-system.md 七节 derive_stats「装备」输入的真实版本——战锤
;; 加力量（攻击端），木盾加护甲（防御端，第一次真的生效），证明
;; register-item-stat-bonus 这个新脚本 API 真的能被 mod 脚本调用，且
;; 装备后的加成真的能走真实 resolve_attack + apply 改变结算出的伤害
;; ——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本
;; 为证」，crates/ll-mod/tests/example_mod_combat.rs 是那份证据。
(register-item-stat-bonus "examplemod:war_hammer" "strength" 6)
(register-item-stat-bonus "examplemod:wooden_shield" "armor" 8)

;; P6 第五批（耐久与 Intent::Use）：knowledge/design/item-system.md 八节
;; 「物品作用」的真实版本——一瓶可堆叠的治疗药水（堆叠上限 10，没有
;; 耐久概念，见 register-item「可堆叠物品不能携带耐久上限」一节），
;; 使用后恢复 40 点法力（复用 SkillEffect，与技能效果同一套编码，见
;; register-item-use-effect 文档），证明 register-item-use-effect 这个
;; 新脚本 API 真的能被 mod 脚本调用，且真实注册的消耗品能走真实
;; Intent::Use + resolve + apply 让效果发生——ADR 0018「玩法层内容必须
;; 能从 mod 脚本注册，且要有真实 mod 脚本为证」，
;; crates/ll-mod/tests/example_mod_use.rs 是那份证据。
(register-item "examplemod:healing_potion" "examplemod:healing_potion_display_name" 10 200 500 -1)
(register-item-use-effect "examplemod:healing_potion" "restore-resource" "mana" 40 0)

;; P6 第六批（武器引用与穿透接线）：knowledge/design/attribute-system.md
;; 三节「穿透属性」的真实版本——给战锤一份固定穿透 3、千分比穿透 100
;; （10%），证明 register-item-penetration 这个新脚本 API 真的能被 mod
;; 脚本调用，且真实注册的穿透值真的能走真实 resolve_attack + apply
;; 改变结算出的伤害（攻击者主手装备战锤时，`damage_after_defense` 第三
;; 个参数从 Penetration::NONE 变成这里注册的值）——ADR 0018「玩法层
;; 内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，
;; crates/ll-mod/tests/example_mod_weapon_reference.rs 是那份证据。
(register-item-penetration "examplemod:war_hammer" 3 100)

;; NPC 生命周期批次（NPC 带物品 → 死亡掉落 → 尸体 → 老化回收）：给上面
;; 已经注册过的哥布林种族（"examplemod:goblin"，第 50 行）声明出生
;; 携带的物品——一把粗制匕首、两支箭，证明 register-race-starting-item
;; 这个新脚本 API 真的能被 mod 脚本调用,且真实注册的出生物品能通过
;; crate::race::starting_inventory 转换成背包物品——ADR 0018「玩法层
;; 内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」，
;; crates/ll-mod/tests/example_mod_starting_items.rs 是那份证据。
(register-item "examplemod:crude_dagger" "examplemod:crude_dagger_display_name" 1 500 8000 20)
(register-race-starting-item "examplemod:goblin" "examplemod:crude_dagger" 1)
(register-race-starting-item "examplemod:goblin" "examplemod:arrow" 2)

;; 伤害公式引擎批次：knowledge/design/damage-formula-mod-api.md 四节
;; 「两个示例，证明这是『不同的一套规则』而不是调系数」的真实版本——
;; 本批次公式只算「送进 damage_after_defense 的攻击力数值」（任务
;; 硬要求一，见 crates/ll-sim/src/formula.rs 模块文档「公式只算
;; 『攻击力』」一节），不像设计文档四节原文那样在公式内部重新实现
;; 整条减伤链路,两条公式因此比原文示例更短,但仍然在四个维度上截然
;; 不同,不是同一套算法调了两个系数：
;;
;;   examplemod:iron_sword_formula——纯物理，恒定确定：攻击力永远是
;;     "有效力量 + 力量调整值"，同样的输入永远算出同样的结果，不含
;;     任何随机性、不理会暴击。
;;   examplemod:flame_longbow_formula——骰子驱动，风格截然不同：
;;     (1) 随机性——攻击力由 1d10（暴击时 2d10，D&D 5e 真实暴击规则：
;;         骰子数翻倍而不是最终结果乘二）决定，不是代数式；
;;     (2) 依赖的属性不同——用敏捷调整值（远程武器），不是力量；
;;     (3) 下限的语义不同——iron_sword_formula 没有下限（纯加法，可能
;;         算出很小的值）,flame_longbow_formula 有一个"至少 1"的绝对
;;         下限（"这一箭无论如何总能划出一点伤害"）；
;;     (4) 暴击的处理点不同——iron_sword_formula 完全没有暴击概念,
;;         flame_longbow_formula 把暴击接进了骰子数量。
;; 与设计文档四节「示例二」同一条论证:随机性来源、依赖的输入、下限
;; 语义、暴击处理点四个维度都不同,不是可以靠调一个系数从一条公式
;; 得到另一条的关系。
(register-damage-formula "examplemod:iron_sword_formula"
  (quote (+ attack-power str-mod)))
(register-damage-formula "examplemod:flame_longbow_formula"
  (quote (max 1 (if (= crit 1) (+ (d 2 10) dex-mod) (+ (d 1 10) dex-mod)))))

;; 铁剑（已在上面第一批注册）现在显式声明使用确定性公式——证明
;; register-item-damage-formula 这个新脚本 API 真的能被 mod 脚本调用，
;; 且注册出来的引用真的能走真实 resolve_attack + apply 改变结算出的
;; 伤害——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod
;; 脚本为证」，crates/ll-mod/tests/example_mod_damage_formula.rs 是
;; 那份证据。
(register-item-damage-formula "examplemod:iron_sword" "examplemod:iron_sword_formula")

;; 新武器：火焰长弓——骰子驱动公式的真实挂载对象，占用主手槽位。
(register-item "examplemod:flame_longbow" "examplemod:flame_longbow_display_name" 1 4000 60000 90)
(register-item-equip-mask "examplemod:flame_longbow" (list "main-hand"))
(register-item-damage-formula "examplemod:flame_longbow" "examplemod:flame_longbow_formula")

;; 伤害类别/抗性接线批次：knowledge/design/damage-formula-mod-api.md
;; 十七、二十一节与 knowledge/design/trait-system.md 三节③的真实版本
;; ——一个新的伤害类别（酸，证明这条开放轴真的能被 mod 无限扩展，不是
;; 只有本体注册的 lostland:physical 一个封闭值）、一件用这个类别攻击的
;; 匕首、一个对酸有 500‰（半伤）抗性的天赋，与一个被授予这个天赋的
;; 种族（软泥怪），四行连起来证明 register-damage-category/
;; register-item-damage-category/register-trait-resistance 三个新脚本
;; API 真的能被 mod 脚本调用，且真实注册的抗性声明真的能走真实
;; resolve_attack + apply 降低伤害——ADR 0018「玩法层内容必须能从 mod
;; 脚本注册，且要有真实 mod 脚本为证」，
;; crates/ll-mod/tests/example_mod_resistance.rs 是那份证据。
;;
;; 顺带用 register-weapon-category 声明一个武器类别（匕首）——证明这个
;; 新脚本 API 同样真的可达；本批次没有给任何武器接上武器类别字段（见
;; crates/ll-mod/src/weapon_category.rs 模块文档「本批次没有给 ItemDef
;; 加对应字段」一节），这里只验证注册本身成功，不代表它已经影响结算。
(register-weapon-category "examplemod:dagger" "")
(register-damage-category "examplemod:acid" "")
(register-trait "examplemod:acid_hide" "examplemod:acid_hide_display_name" (list))
(register-trait-resistance "examplemod:acid_hide" "examplemod:acid" 500)
(register-race "examplemod:ooze" "examplemod:ooze_display_name" 0 0 0 0 0 0 0 1 1 30)
(register-race-trait "examplemod:ooze" "examplemod:acid_hide" 1)
(register-item "examplemod:acid_dagger" "examplemod:acid_dagger_display_name" 1 500 6000 40)
(register-item-equip-mask "examplemod:acid_dagger" (list "main-hand"))
(register-item-damage-category "examplemod:acid_dagger" "examplemod:acid")
;; 复用上面已经注册过的确定性铁剑公式（+ attack-power str-mod）——本
;; 批次不涉及公式本身的设计,只需要一条不掷骰、期望值可手算复现的公式
;; 挂在这件新武器上,不必为它另写一条等价的公式。
(register-item-damage-formula "examplemod:acid_dagger" "examplemod:iron_sword_formula")

;; 抗性多来源聚合批次：项目所有者对抗性来源的裁定原话——「抗性肯定会
;; 来自天赋，以及装备，还有各种药品，或者技能」。上面的 acid_hide 天赋
;; 是第一路（天赋）；这里接上**第二路（装备）**——一件挂在脖子上的酸抗
;; 护符，同样声明 500‰（半伤）对酸的抗性。两路来源写进的是同一种载荷
;; （RuleModifier::Resistance），被同一个聚合点
;; （ll_sim::rule_modifier::resistance_multiplier_permille）按同一条
;; tie-break 规则消费,差别只在"这条声明存在哪张表里"。
;;
;; 这三行证明 register-item-resistance 这个新脚本 API 真的能被 mod 脚本
;; 调用，且真实注册的装备抗性真的能走真实 resolve_attack + apply 降低
;; 伤害——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod
;; 脚本为证」，crates/ll-mod/tests/example_mod_resistance.rs 里的
;; `真实注册的酸抗护符装备在身上时真实降低了酸匕首造成的伤害` 是那份
;; 证据。刻意用一个**没有 acid_hide 天赋的种族**（半精灵）来戴它,这样
;; 降下来的那部分伤害只可能来自装备这一路,不会与天赋那一路混淆。
;; 最后一个参数是耐久上限，-1 表示"没有耐久概念"——护符占的是脖子
;; 槽位，不是武器槽位，register-item 的既有校验只允许武器携带耐久。
(register-item "examplemod:acid_ward_amulet" "examplemod:acid_ward_amulet_display_name" 1 300 15000 -1)
(register-item-equip-mask "examplemod:acid_ward_amulet" (list "neck"))
(register-item-resistance "examplemod:acid_ward_amulet" "examplemod:acid" 500)

;; 盗贼偷袭接线批次：所有者对「盗贼偷袭」的裁定原话——「盗贼偷袭做成
;; 技能判定吧，通过幸运值之类的属性以及一定的随机值组合一下」。
;; trait-system.md 此前判定盗贼偷袭表达不了（真实条件「目标旁边有我的
;; 盟友」需要一次本项目不存在的空间查询），所有者的裁定绕开了这条
;; 依赖——改成只依赖攻击者自身有效幸运的判定，落地成天赋效果
;; RuleModifier::SneakAttack，不是技能效果（见
;; crate::resolve::resolve_attack 文档「偷袭接线」一节的完整论证）。
;; 一个天赋（潜行本能，每点有效幸运 20‰ 触发率加成，触发后追加 15 点
;; 固定伤害）+ 一个新种族（迅足者，1 级即被授予这个天赋）——两行连起来
;; 证明 register-trait-sneak-attack 这个新脚本 API 真的能被 mod 脚本
;; 调用，且真实注册的偷袭声明真的能走真实 resolve_attack + apply 追加
;; 伤害，不只是在单元测试里自证（ADR 0018），
;; crates/ll-mod/tests/example_mod_sneak_attack.rs 是那份证据。
(register-trait "examplemod:predatory_instinct" "examplemod:predatory_instinct_display_name" (list))
(register-trait-sneak-attack "examplemod:predatory_instinct" 20 15)
(register-race "examplemod:footpad" "examplemod:footpad_display_name" 0 0 0 0 0 0 0 1 1 60)
(register-race-trait "examplemod:footpad" "examplemod:predatory_instinct" 1)

;; 职业天赋接线批次：`trait-system.md` 三节①五路来源公式里「职业天赋」
;; 那一路的真实内容证据。载荷与聚合算法与上面的种族天赋完全共用
;; （同一个 TraitGrant、同一段 effective_traits）——**唯一的实质差异
;; 在 unlock-level**：种族天赋恒填 1（拥有即生效），职业天赋按等级
;; 曲线填。这里填 3，于是同一个盗贼角色 2 级放不出下面这个技能、3 级
;; 能放，证明「按等级解锁」这条只有职业才真正用得上的语义确实走通了，
;; 而不是又一条恒填 1 的声明。
;;
;; 证明 register-class-trait 这个新脚本 API 真的能被 mod 脚本调用，且
;; 真实注册的职业天赋真的能走真实 resolve_use_skill 门一（有效技能
;; 并集）——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod
;; 脚本为证」，crates/ll-mod/tests/example_mod_class_traits.rs 是那份
;; 证据，不能靠单元测试自证。
(register-class "examplemod:rogue" "examplemod:rogue_display_name" "dexterity")
(register-skill "examplemod:backstab" "" (list) 20 "mana" 5 "deal-damage" "" 18 0)
(register-trait "examplemod:cutpurse_training" "examplemod:cutpurse_training_display_name"
  (list "examplemod:backstab"))
(register-class-trait "examplemod:rogue" "examplemod:cutpurse_training" 3)

;; 温度系统批次（保暖）：knowledge 里「温度必须有真实消费者」这条要求
;; 的内容侧落点——两件保暖装备，各占一个**不同**的槽位（羊毛内衬占
;; body，毛皮斗篷占 outer），于是它们可以同时穿在身上。
;;
;; 这正是「绝缘值走求和、不走 tie-break」那条判断在真实内容上的验收：
;; 单穿内衬 +50（5℃）、单穿斗篷 +90（9℃）、两件都穿 +140（14℃）。本体
;; 冬季午夜的地表是 -4℃，单穿任一件都还差一点、两件穿齐就完全不冷——
;; 若绝缘值走的是 ll_sim::rule_modifier 那条 tie-break 语义（多个来源
;; 只取一条），穿上第二件将毫无作用，这三档就塌成两档。
;; crates/ll-mod/tests/example_mod_temperature.rs 是那份证据（ADR 0018
;; 「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本为证」）。
;;
;; register-item-stat-bonus 的第二个参数多认识了一个目标名
;; "insulation"（此前只有六个属性名 + "luck" + "armor"），单位是十分之
;; 一摄氏度，与 ll_world::temperature::Temperature 同一量纲。
;; 两件都传 -1（没有耐久概念）：register-item 的注册期校验只允许占用
;; 武器槽位（主手/副手）的物品携带耐久上限，而这两件占的是 body/outer
;; ——最初写成 60/90 时装载直接失败，是 ADR 0017「注册期完整校验」在
;; 本批次内容上的一次真实拦截。
(register-item "examplemod:wool_liner" "examplemod:wool_liner_display_name" 1 2000 8000 -1)
(register-item-equip-mask "examplemod:wool_liner" (list "body"))
(register-item-stat-bonus "examplemod:wool_liner" "insulation" 50)
(register-item "examplemod:fur_cloak" "examplemod:fur_cloak_display_name" 1 5000 30000 -1)
(register-item-equip-mask "examplemod:fur_cloak" (list "outer"))
(register-item-stat-bonus "examplemod:fur_cloak" "insulation" 90)

;; ── 制作系统（制作系统落地批次）─────────────────────────────────────
;;
;; 六个新注册函数的签名见宿主模块文档：
;;   register-recipe-category            crates/ll-mod/src/script_recipe_category_api.rs
;;   recipe-category-requires-subclass!  crates/ll-mod/src/script_recipe_category_api.rs
;;   register-recipe                     crates/ll-mod/src/script_recipe_api.rs
;;   recipe-requires-station!            crates/ll-mod/src/script_recipe_api.rs
;;   recipe-requires-tool!               crates/ll-mod/src/script_recipe_api.rs
;;
;; 烹饪/锻造/裁缝/炼金是**同一套机制**，四类的差别全部落在数据上
;; （类别/食材/场地/工具），见 knowledge/design/crafting-system.md 二节
;; 用 ADR 0021 做的统一论证。下面两个类别 + 四条配方覆盖了这套机制的
;; 全部字段组合，是 crates/ll-mod/tests/example_mod_crafting.rs 那份
;; 端到端证据（ADR 0018）的内容来源。

;; 三件新材料/食材。都不占装备槽位，因此耐久上限一律传 -1
;; （register-item 只允许占武器槽位的物品带耐久）。
(register-item "examplemod:raw_meat" "examplemod:raw_meat_display_name" 20 500 300 -1)
(register-item "examplemod:roast_meat" "examplemod:roast_meat_display_name" 20 400 900 -1)
(register-item "examplemod:iron_ingot" "examplemod:iron_ingot_display_name" 50 2000 4000 -1)

;; 类别一：烹饪。**刻意不调用 recipe-category-requires-subclass!**
;; ——空闸门就是「人人可做」，正是
;; knowledge/design/food-and-cooking-system.md 五节「菜谱不设解锁门槛」
;; 那条裁定的直接落点：有没有闸门是纯内容决定，系统不预设立场。
(register-recipe-category "examplemod:cooking" "examplemod:recipe_category_cooking_display_name")

;; 暗影舞者的获得条件：在**烹饪**类别里做满 3 次。
;; 签名：(register-subclass-unlock subclass-id trigger-kind trigger-target threshold)
;; trigger-kind 目前只接受 "items-crafted"。
;;
;; # 为什么挂在烹饪而不是锻造——这一条不是随便挑的
;;
;; 锻造类别下面一行就要求 examplemod:shadowdancer 才能做。若把获得
;; 条件也挂在锻造上，就成了「要当暗影舞者才能锻造，要锻造才能当暗影
;; 舞者」——两边互相等，这个副职永远拿不到，而且**完全不会报错**。
;; resolve_craft 的副职闸门是每次制作都判的，所以这个死锁是真的。
;;
;; 正确的形状就是本文件现在这样：**从一个不设闸门的类别里练出副职，
;; 用它去开另一个设了闸门的类别的门。** 这两行合起来是一条完整的
;; 玩法链路，也是 crates/ll-mod/tests/example_mod_subclass_unlock.rs
;; 那份端到端证据的内容来源：烤三次肉 → 拿到暗影舞者 → 锻造解锁。
;;
;; 阈值取 3 是为了让那份端到端测试跑得快；本体那四个副职用的是
;; 20/20/15/15 这种真实量级，见 mods/lostland/subclasses.scm。
(register-subclass-unlock "examplemod:shadowdancer" "items-crafted" "examplemod:cooking" 3)

;; 类别二：锻造。闸在**类别**上而不是每条配方上——新增一条锻造配方
;; 自动继承这道闸，加一个新副职也只改这一行。
(register-recipe-category "examplemod:forging" "examplemod:recipe_category_forging_display_name")
(recipe-category-requires-subclass! "examplemod:forging" "examplemod:shadowdancer")

;; 配方①：三条前置全不设——完全等价于食物系统九节的烤肉示例，证明
;; 类别闸门/场地/工具三个新能力全部可选，一条都不填也能跑。
(register-recipe "examplemod:roast_meat_recipe" "examplemod:roast_meat_recipe_display_name"
                 "examplemod:cooking"
                 (list "examplemod:raw_meat") (list 1)
                 "examplemod:roast_meat" 1)

;; 配方②：类别闸 + 场地 + 工具三条全开，最复杂的一条路径。
;; 场地用 examplemod:lava_floor（mods/example_mod/terrain.scm 注册的
;; 可通行地形）——工作台地形必须可通行，否则玩家站不上去，见
;; crate::recipe::RecipeDef::required_station 文档「配套的内容纪律」。
;; 工具用战锤：判定是「装备着**且耐久未归零**」，坏掉的锤子打不了铁。
(register-recipe "examplemod:iron_sword_recipe" "examplemod:iron_sword_recipe_display_name"
                 "examplemod:forging"
                 (list "examplemod:iron_ingot") (list 2)
                 "examplemod:iron_sword" 1)
(recipe-requires-station! "examplemod:iron_sword_recipe" "examplemod:lava_floor")
(recipe-requires-tool!    "examplemod:iron_sword_recipe" "examplemod:war_hammer")

;; 配方③：**同一件成品的第二条配方**——register-recipe 刻意不校验
;; product 唯一性（设计文档九节④：零成本的变化度），这条因此合法。
;; 它只要场地不要工具，与②合起来证明两条前置真的互相独立。
(register-recipe "examplemod:iron_sword_from_scrap" "examplemod:iron_sword_from_scrap_display_name"
                 "examplemod:forging"
                 (list "examplemod:iron_ingot" "examplemod:crude_dagger") (list 1 1)
                 "examplemod:iron_sword" 1)
(recipe-requires-station! "examplemod:iron_sword_from_scrap" "examplemod:lava_floor")

;; 配方④：product-count 大于一——一次产出五支箭。
(register-recipe "examplemod:arrow_batch_recipe" "examplemod:arrow_batch_recipe_display_name"
                 "examplemod:forging"
                 (list "examplemod:iron_ingot") (list 1)
                 "examplemod:arrow" 5)
