;; 迷途大陆示例 mod：注册一段自定义动画剪辑——史莱姆的挤压动画。
;;
;; register-animation-clip 由宿主（crates/ll-mod/src/script_clip_api.rs）
;; 注册进脚本引擎，签名固定：
;;   (register-animation-clip id frames frames-per-step looping? exit-grace-frames)
;;
;; 这是第七类可注册的玩法层内容——此前只有本体能把动画剪辑写死在
;; Rust 里（ll_mod::base_clip::register_base_clips），mod 完全够不着；
;; 本文件证明同一条 register-animation-clip 现在对 mod 与本体一视
;; 同仁，走完全相同的 Registry::intern 通道。
;;
;; 两帧循环挤压，节奏比本体的行走剪辑略慢（每帧停留 6 个游戏帧）；
;; exit-grace-frames 填 0——这段剪辑打算配合电平驱动使用（史莱姆是否
;; 正在移动/警戒本身是电平判据），不经过 AnimStateMachine 的
;; 触发+余韵机制。
(register-animation-clip "examplemod:slime_squish" (list "slime_0" "slime_1") 6 #t 0)
