;; 迷途大陆示例 mod：注册一种新地形——熔岩地板。
;;
;; register-terrain 由宿主（crates/ll-mod/src/script_terrain_api.rs）
;; 注册进脚本引擎，签名固定：
;;   (register-terrain id blocks-sight blocks-move move-cost opens-into)
;;
;; 熔岩地板可以走上去（不阻挡移动、不阻挡视线），但移动代价远高于
;; 平地（100）——这正是「属性生效」这条验收点要看到的效果：玩家能
;; 走上去，但会明显变慢，与本体的浅水/沙地同一类「可通行但更慢」的
;; 地形没有任何结构性差异，唯一不同的只是命名空间是 examplemod 而
;; 不是 lostland。
(register-terrain "examplemod:lava_floor" #f #f 350 "")

;; 第二种地形：铺石地面。可通行、代价与本体平地相同——它存在的意义
;; 是「不是工作台的那种地」：制作系统批次的配方②声明必须站在
;; examplemod:lava_floor 上才能锻造，
;; crates/ll-mod/tests/example_mod_crafting.rs 的场地反例需要一块
;; **确定不是**工作台的地面站上去，才能证明那道判定真的在生效。
(register-terrain "examplemod:paved_floor" #f #f 100 "")
