;; 本体职业内容。目前只有一条：卫兵。
;;
;; 签名见 crates/ll-mod/src/script_class_api.rs：
;;   (register-class id display-name-key primary-attribute)
;; primary-attribute 六选一：strength/dexterity/constitution/
;; intelligence/willpower/charisma。
;;
;; # 为什么本文件里只有卫兵一条，另外三个基础职业还在 Rust 里
;;
;; 战士/法师/游侠三条至今仍写在 `ll_mod::class::materialize_base_classes`
;; 里——那个函数**不在生产装载路径上**（`ll_game::content::load_content`
;; 给出的是一张空 `ClassTable::new()`，只有 mod 脚本能往里写），它的
;; 唯一调用方是 `crates/ll-content/examples/p5_gameplay_acceptance.rs`
;; 这个验收 demo 与 `ll-mod` 自己的单元测试。换句话说：**真实游戏里
;; 至今一条职业内容都没有**。
;;
;; 把那三条一并迁过来、删掉 `materialize_base_classes`/`base_class_fixture`、
;; 照 `ll_mod::race::resolve_base_races` 的样子补一套 `BaseClassIds`
;; 契约解析——那是与种族迁移（`e8af2a8`）等量的一整批工作（11 处
;; `base_class_fixture` 调用点、p5 验收 demo、契约解析、句柄结构体
;; 穿进 `LoadedContent`），本批次刻意不夹带。
;;
;; 卫兵这一条之所以不能一起等：它与那三条有一个性质上的差别——
;; **已经有一份真实 mod 脚本按字符串引用它了**。
;; `mods/example_mod/behavior.scm` 的 `guard-try-inspect` 第一句就是
;; `(self-has-profession? "lostland:guard")`，而 `self-has-profession?`
;; 认的是注册表快照里的字符串（见
;; `ll_mod::script_behavior_api::register_profession_check_api`）。这条
;; 内容不存在于生产注册表，那个 `if` 就恒为假，**卫兵永远不会盘查**。
;; 也就是说卫兵职业的缺席不是「内容还没迁」，是一处悬空引用。
;;
;; 显示名键 lostland:class.guard.display_name 对应
;; assets/locales/{en,zh-CN}.ftl 里的 class-guard-display_name
;; （本批次一并补上——`materialize_base_classes` 当初声明了这个键，
;; 但两份 .ftl 里从来没有过对应条目，同样是「声明了没落地」）。
;;
;; 卫兵不声明任何职业天赋：天赋走注册后追加的 register-class-trait，
;; 本体不调用即为空，与其余本体内容同一条纪律（不为了让字段覆盖检查
;; 变绿硬塞一条天赋，见 `ll_mod::content_audit` 里对应的豁免条目）。

;; 卫兵：体质倾向——项目所有者裁定「卫兵算作一种职业」。
(register-class "lostland:guard" "lostland:class.guard.display_name" "constitution")
