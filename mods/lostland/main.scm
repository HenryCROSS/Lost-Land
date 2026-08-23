;; 本体内容 mod 的**唯一**入口脚本。
;;
;; # 为什么 entry_points 只剩一条
;;
;; `entry_points` 是一个数组，装载管线**按数组顺序**逐个编译——于是
;; 「谁必须排在谁前面」这条真实存在的约束就被写进了清单，而清单里
;; 看不出**为什么**。本 mod 曾经有两条这样的隐形约束：
;;
;; - crafting.scm 必须排在 subclasses.scm 前面（`register-subclass-unlock`
;;   的 trigger-target 只 get 不 intern）
;; - classes.scm 必须排在 skills.scm 前面（技能的 owning-class 同理）
;;
;; 两条都只在 mod.json5 的注释里写着。注释不参与装载，改错顺序的人
;; 得等到装载期报错才知道；更糟的是，第三方 mod 作者照抄这份清单时
;; 根本不知道有这回事。
;;
;; 有了模块系统，依赖就写在**需要它的那个文件里**：skills.scm 自己
;; `(require "classes")`，subclasses.scm 自己 `(require "crafting")`。
;; 于是本文件里这七行的**顺序不再有意义**——刻意按字母序排（与此前
;; entry_points 的工作顺序不同），就是为了让「顺序不再有意义」这件事
;; 一眼可验：真依赖顺序还在起作用的话，这份排法会当场装载失败。
;;
;; 实测依据：`crates/ll-script/examples/probe_modules.rs` 第 11 节——
;; 主脚本故意把依赖方写在被依赖方前面，模块体仍然按 require 图求值，
;; 且每个模块只求值一次。
;;
;; # 这些文件为什么不写 provide
;;
;; 它们是「跑一遍、把内容注册进表」的脚本，不对外提供任何名字。没写
;; `provide` 的模块什么都不导出（实测：不是"默认全导出"），这正是想
;; 要的效果——本文件 require 它们只为触发那次注册。

(require "classes")
(require "crafting")
(require "quests")
(require "races")
(require "skills")
(require "subclasses")
(require "tags")
