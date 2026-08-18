;; 故意试图绕过白名单：拿到非确定性墙钟。
;; mod 脚本设计上不允许 require 任何 Steel 内置模块（见
;; crates/ll-script/src/host.rs 的 reject_dangerous_syntax 文档），
;; 下面这一行应当在编译前就被文本层前置优化直接拒绝——注意：本文件
;; 的注释故意不写出被禁子串本身的完整拼写，否则 reject_dangerous_syntax
;; 用 source.find 定位到的会是注释里的字面出现，而不是下面真正的代码，
;; 报出来的行号会变成误导性的第一行。
(define probe-line 1)
(require-builtin steel/time)
(instant/now)
