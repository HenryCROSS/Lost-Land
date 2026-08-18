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
