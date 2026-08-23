;; 示例 mod 装载期脚本的**残余**：只剩两条事件订阅声明。
;;
;; 此前这个文件有 148 行有效代码，是仓库里最大的一份内容声明——
;; 十八类内容（物品/天赋/种族/技能/配方/资源池/经验曲线/伤害公式/
;; 伤害类别/武器类别/职业/副职/任务……）全在里面。它们已经整体搬进
;; 同目录下的 *.json5：玩家下载即用、零虚拟机、零沙箱问题，且未知
;; 字段与缺字段两类错误都带文件名与行列位置。见
;; ll_mod::content_schema / content_schema_gear / content_schema_world。
;;
;; 剩下这两行是**订阅声明**，不是内容——它们要写的
;; EventSubscriptionTable 里一个 ContentIndex 都没有，不进内容值哈希、
;; 不进存档 remap（见 ll_mod::event 模块文档「这不是一张内容表」）。
;; 事件处理整体搬进引擎是下一个批次的事，本批次只清空内容。
(on-event "killed" "examplemod-on-kill")
(on-event "experience-gained" "examplemod-on-experience")
