;; 本体对外提供的标识符小工具——**本文件是跨 mod require 的示范**。
;;
;; # 它凭什么可以被别的 mod require
;;
;; 只有 `provide` 出去的名字对要求方可见（`provide` 才是导出形式，不是
;; `export`；没写进去的名字在要求方那边编译期就报 FreeIdentifier）。
;; 本文件只导出 `lostland-id` 一个纯函数，没有任何 `register-*` 副作用
;; ——这不是风格选择：跨 mod require 是把这份源码搬进**要求方**的 VM
;; 重新编译一次，模块体里的注册动作会以要求方的身份发生（见
;; `ll_script::modules` 模块文档「跨 mod require 的模块，在要求方的 VM
;; 里求值」）。辅助函数、常量表这类纯定义才是跨 mod 模块的正确用法。
;;
;; # 别的 mod 怎么用
;;
;;   ;; mod.json5 里先声明依赖：dependencies: ["lostland"]
;;   (require "lostland:ids")
;;   (register-item-tag "examplemod:iron_sword" (lostland-id "weapon"))
;;
;; 没在 `dependencies` 里声明过 lostland 就 require 它的模块，会在装载
;; 期拿到一条点名的错误（「未在 mod.json5 的 dependencies 里声明」），
;; 不是「找不到模块」——两件事分得清，才知道该改清单还是改路径。

(provide lostland-id)

;; 把一个裸名字拼成本体命名空间下的完整标识符：
;;   (lostland-id "weapon") → "lostland:weapon"
;;
;; 存在的理由不是省几个字符，是让「本体的命名空间叫什么」在别的 mod
;; 里只出现一次。命名空间真要改名时，改这一个文件，而不是去所有引用
;; 方的字符串字面量里逐个找。
(define (lostland-id 裸名字)
  (string-append "lostland:" 裸名字))
