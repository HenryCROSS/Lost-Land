;; 故意的语法错误：最后一行少一个右括号——用来验证加载管理界面能把
;; 错误定位到具体行号（第 4 行），不是笼统的「加载失败」。
(define lava-cost 350)
(register-terrain "brokensyntax:oops" #f #f lava-cost ""
