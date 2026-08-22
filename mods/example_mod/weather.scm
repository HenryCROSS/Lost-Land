;; 迷途大陆示例 mod：注册一种新天气（灰烬雨），以及一种新空间层属性
;; （火山洞窟）。
;;
;; 本文件是两条 register-* 通道的 ADR 0018 真实脚本证据。
;;
;; ---------------------------------------------------------------- 天气
;;
;; register-weather 由宿主（crates/ll-mod/src/script_weather_api.rs）
;; 注册进脚本引擎，签名固定：
;;   (register-weather id display-name-key light-scale sight-scale
;;                     temperature-offset
;;                     spring-weight summer-weight autumn-weight winter-weight)
;;
;; 灰烬雨：天空被灰烬遮蔽，明显变暗（光照乘数 550‰），同时呛人的
;; 悬浮灰烬极大缩短视距（视野乘数 400‰）——这两个乘数是**独立**的
;; 旋钮，正是本体的「阴天」（暗但看得远）与「雾」（不太暗但看不远）
;; 之间那条对比在 mod 侧的验收：只有一个乘数的话，灰烬雨就只能是
;; 「更暗一点的阴天」，做不出「又暗又看不见」这种独立的第三种效果。
;;
;; 季节权重刻意不均匀：夏秋两季火山活跃（8 / 6），春季偶发（2），
;; 冬季完全不出现（0）。冬季那个 0 是「取 0 表示这一季绝不出现」这条
;; 语义在 mod 侧的用例，与本体的雪（春夏为 0）互为镜像。
;;
;; temperature-offset 取 +150（比无天气时**暖** 15℃，单位是十分之一
;; 摄氏度）——这是温度系统批次给这条 register-* 通道补的第九个参数，
;; 也是本体六种天气都没有覆盖到的那一半：本体的偏移全部是 0 或负数
;; （天气只会让人更冷），灰烬雨是仓库里唯一一条**正**偏移的天气内容。
;; 它验收的是 WEATHER_TEMPERATURE_OFFSET_LIMIT 那条「上下界对称」的
;; 设计声明——「变暖」与「变冷」在语义上完全对等，不是只有一个方向
;; 说得通。火山灰云锁住地表热量，在设定上也自洽。
;;
;; 与本体六种天气（lostland:clear/overcast/rain/wind/fog/snow）走完全
;; 相同的 Registry::intern 通道，唯一的差异只是命名空间是 examplemod
;; 而不是 lostland——本体天气注册在
;; crates/ll-mod/src/base_weather.rs，那里同样只是把 registry.intern
;; 包成回调传下去，没有任何本体专属的特权路径。
(register-weather "examplemod:ashfall"
                  "examplemod:weather.ashfall.display_name"
                  550 400
                  150
                  2 8 6 0)

;; ---------------------------------------------------- 空间层属性（补证）
;;
;; register-space-profile 由宿主
;; （crates/ll-mod/src/script_space_profile_api.rs）注册进脚本引擎，
;; 签名固定：
;;   (register-space-profile id ambient-light-floor exposed-to-sky
;;                           base-temperature diggable buildable reverb-tag)
;;
;; 这条调用补的是一处如实记录的欠账：register-space-profile 落地时
;; （空间层属性脚本注册批次）**没有留下任何已发货脚本的调用**，是当时
;; 十六个注册函数里唯一一个只有单元测试、没有真实 mod 脚本证据的。
;; ADR 0018 要的是「mod 真的能注册这类内容」，而不是「宿主侧有一个
;; 能被单测调用的函数」——这两件事之间隔着装载管线的接线，正是本项目
;; 反复吃亏的那一节。
;;
;; 火山洞窟：不露天（exposed-to-sky = #f），因此环境光恒等于
;; ambient-light-floor——这里取 90‰ 而不是 0，表现「岩浆自带的暗红
;; 幽光」，与本体洞窟（0，伸手不见五指）明确区分开。温度基准 480
;; （本体建筑内部是 220），可挖掘（#t）、不可建造（#f）。
;;
;; 最后一个参数 reverb-tag 传空串——空串是「没有」的哨兵约定（合法的
;; 命名空间字符串恒非空，不会与真实标识符混淆）；代码库至今没有音频
;; 层，填一个没有消费者的标签只会制造又一处「声明了没人读」。
(register-space-profile "examplemod:volcanic_cave" 90 #f 480 #t #f "")
