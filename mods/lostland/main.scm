;; 本体内容 mod 的唯一入口脚本——**已经几乎空了**。
;;
;; # 内容去哪了
;;
;; 本体的七类内容声明（种族/职业/技能/任务/副职/配方类别/标签）已经
;; 全部搬进同目录下的 `*.json5` 数据文件。项目所有者的裁定原话是
;; 「内容用数据文件（JSON5），行为用 Rust」「这样也能有数据驱动的方式
;; 编写」——起因是 steel-core 0.8.2 那个查不出根因的内存破坏缺陷
;; （ADR 0028），脚本系统整体在拆，但「玩家下载即用的 mod」不该跟着
;; 一起丢掉。装载入口见 `ll_mod::content_data`。
;;
;; 此前这里有七行 `(require ...)`，一行对应一个 `.scm` 内容文件。那七个
;; 文件连同它们表达的装载顺序约束一起没了：顺序现在是
;; `ll_mod::content_data` 里 CONTENT_FILES 这一个常量，由引擎保证，
;; mod 作者改不动也改不坏。
;;
;; # 那本文件为什么还在
;;
;; `ids.scm` 还在——它是**跨 mod require 的示范**，
;; `mods/example_mod/gameplay.scm` 至今 `(require "lostland:ids")` 在用
;; 它。本文件保留一行 require 把它带进本 mod 自己的 VM，是为了让
;; 「lostland 这个 mod 的脚本部分还剩什么」有一个不需要翻目录就能看见
;; 的答案。
;;
;; 本文件与 `ids.scm` 都是**过渡状态**：脚本引擎删除是收尾批次的事，
;; 那一批之后 lostland 会是一个纯数据 mod（清单里 entry_points 为空是
;; 合法的，见 `ll_mod::manifest`）。

(require "ids")
