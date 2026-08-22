;; 本体制作类别内容。
;;
;; 签名见 crates/ll-mod/src/script_recipe_category_api.rs：
;;   (register-recipe-category id display-name-key)
;;   (recipe-category-requires-subclass! category-id subclass-id)
;;
;; # 为什么本文件里只有类别，没有一条 register-recipe
;;
;; 配方要引用**物品**（食材与成品都指向 ItemDef），而本体的物品内容
;; 至今一条都没迁进 mods/lostland/——`ll_game::content::load_content`
;; 交给装载管线的是一张空 ItemTable，真实游戏里的全部物品内容目前只
;; 存在于 mods/example_mod/gameplay.scm。没有物品就写不出配方，这不是
;; 本批次刻意留的坑，是内容迁移的既有进度（同一句话对职业也成立，见
;; classes.scm 文件头）。
;;
;; 类别本身**不需要**物品，因此可以先落地——而它必须先落地，理由见
;; subclasses.scm：副职的获得条件要指向一个已注册的配方类别。
;;
;; # 四个类别都**不设**副职闸门，这是一条刻意的决定
;;
;; register-recipe-category 登记出来的类别 required_subclasses 恒以空
;; 列表开始（人人可做），要设闸门必须额外调用
;; recipe-category-requires-subclass!。本文件一次都不调用它，两条理由：
;;
;; 1. **会造出一个死锁。** subclasses.scm 让四个副职各自从「在对应类别
;;    里做满 N 次」获得。若同一个类别又要求那个副职才能做，就成了
;;    「要当工匠才能锻造，要锻造才能当工匠」——两边互相等，谁都拿不到。
;;    resolve_craft 的副职闸门是每次制作都判的（crates/ll-sim/src/resolve.rs
;;    第③步），所以这个死锁是真的，不是理论上的。
;; 2. **烹饪本来就不该有闸门。** knowledge/design/food-and-cooking-system.md
;;    五节已裁定「任何角色只要凑齐食材就能做出对应菜谱，不需要学会
;;    这一步」。
;;
;; 闸门这条能力本身**没有闲置**：mods/example_mod/gameplay.scm 的
;; examplemod:forging 就设了闸门（要求 examplemod:shadowdancer），并且
;; 有一份端到端测试盯着它（crates/ll-mod/tests/example_mod_subclass_unlock.rs）。
;; 本体不用它，是内容决定，不是能力缺失。
;;
;; # 将来想要「进阶类别设闸门」怎么写
;;
;; 正确形状是**两个类别**：一个不设闸门的基础类别负责让玩家把副职练
;; 出来，一个设闸门的进阶类别把守真正的高级配方。等本体有了真实配方
;; 内容再拆，现在拆等于凭空造四个空类别。

;; 锻造：金属装备与工具。对应副职 lostland:artisan（工匠）。
(register-recipe-category "lostland:forging" "lostland:recipe_category.forging.display_name")

;; 裁缝：衣物与织物。对应副职 lostland:tailor（裁缝）。
;; 温度系统（c12c04f）落地之后这一条有了真实的玩法压力——低温需要保暖
;; 衣物，见 knowledge/design/subclass-system.md 六节「补给循环」。
(register-recipe-category "lostland:tailoring" "lostland:recipe_category.tailoring.display_name")

;; 炼金：药剂与试剂。对应副职 lostland:alchemist（炼金术士）。
(register-recipe-category "lostland:alchemy" "lostland:recipe_category.alchemy.display_name")

;; 烹饪：食物。对应副职 lostland:cook（厨师）。
;;
;; # 炼金与烹饪是两个独立的类别、两个独立的副职
;;
;; 项目所有者裁定原话：「药水调剂和厨艺是两个不同的方向，所以需要
;; 拆分。」这**推翻了** 6fa7eb8 记录的「调剂 = 炼金 + 厨艺合并成一个
;; 副职」那条复核结论。拆分的代价是不对称的（合并不可逆——存档重映射
;; 对解析不到的副职索引直接丢弃；拆开只是多一行 register-subclass），
;; 所以按裁定拆开是安全的方向。
(register-recipe-category "lostland:cooking" "lostland:recipe_category.cooking.display_name")
