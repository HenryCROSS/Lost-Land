;; 本体职业内容：四条——战士 / 法师 / 游侠 / 卫兵。
;;
;; 签名见 crates/ll-mod/src/script_class_api.rs：
;;   (register-class id display-name-key primary-attribute)
;;   (register-class-trait class-id trait-id unlock-level)
;; primary-attribute 六选一：strength/dexterity/constitution/
;; intelligence/willpower/charisma。
;;
;; # 战士/法师/游侠三条是本批次迁进来的
;;
;; 这三条此前写在 `ll_mod::class::materialize_base_classes` 里，而那个
;; 函数**从来不在生产装载路径上**——`ll_game::content::load_content`
;; 交给装载管线的是一张空 `ClassTable::new()`，它的唯一调用方是
;; `crates/ll-content/examples/p5_gameplay_acceptance.rs` 这个验收 demo
;; 与 `ll-mod` 自己的单元测试。也就是说**在此之前真实游戏里一条职业
;; 内容都没有**（卫兵那一条除外，它上一批已经迁进来了）。
;;
;; 项目所有者裁定「迁移吧，工作要做好」之后，那个函数与它的测试夹具
;; `base_class_fixture` 一并删除，三条职业改由本文件注册，走的是与
;; 任何第三方 mod 完全相同的 `register-class` 通道。Rust 侧只留下
;; `ll_mod::class::BaseClassIds`（句柄，保住使用点的编译期安全）与
;; `resolve_base_classes`（装载后按 id 逐字段解析，缺一条整批失败）。
;;
;; 值得写下来的一点：这三条内容**第一次真正进到游戏里**，因此本批次
;; 的内容值哈希会变——那不是「迁移不忠实」，恰恰相反，它是「此前它们
;; 根本没被装载过」的直接证据。卫兵/种族那几批是纯搬家，哈希逐位不变。
;;
;; # 显示名键
;;
;; 四条各自对应 assets/locales/{en,zh-CN}.ftl 里的
;; class-{warrior,mage,ranger,guard}-display_name——四条都早就在两份
;; .ftl 里，也早就挂在 `ll_i18n` 的 PRODUCTION_KEYS 覆盖检查上。
;;
;; # 本体职业不声明任何职业天赋
;;
;; 天赋走注册后追加的 register-class-trait，本体不调用即为空，与其余
;; 本体内容同一条纪律（不为了让字段覆盖检查变绿硬塞一条天赋，见
;; `ll_mod::content_audit` 里 `ClassAttrs::traits` 那条豁免）。字段
;; 本身不是死的：mods/example_mod/gameplay.scm 的 examplemod:rogue 用
;; register-class-trait 在 3 级授予 examplemod:cutpurse_training。
;;
;; # 顺序：本文件必须排在 skills.scm 之前
;;
;; skills.scm 里四条战士技能的 owning-class 指向 lostland:warrior。
;; register-skill 对 owning-class 只 intern 不要求已定义，所以顺序反了
;; 不会当场报错——但 `ll_mod::content_audit` 的引用完整性校验会在装载
;; 末尾把它判成一条 `SkillAttrs::owning_class` 违规（那一步看的是最终
;; 合并结果，与文件顺序无关）。把顺序写对是让读者一眼看出依赖方向，
;; 不是让检查通过的手段。

;; 战士：力量倾向。
(register-class "lostland:warrior" "lostland:class.warrior.display_name" "strength")

;; 法师：智力倾向。
(register-class "lostland:mage" "lostland:class.mage.display_name" "intelligence")

;; 游侠：敏捷倾向。
(register-class "lostland:ranger" "lostland:class.ranger.display_name" "dexterity")

;; 卫兵：体质倾向——项目所有者裁定「卫兵算作一种职业」。
;;
;; 这一条上一批（5862dbe）就迁进来了，理由与上面三条不同：它有一处
;; **悬空引用**在等着它——mods/example_mod/behavior.scm 的
;; guard-try-inspect 第一句是 `(self-has-profession? "lostland:guard")`，
;; 而 self-has-profession? 认的是注册表快照里的字符串。这条内容不存在
;; 于生产注册表，那个 if 就恒为假，卫兵永远不会盘查。
;;
;; 它刻意**不**进 `BaseClassIds`：Rust 侧一行代码都没按名字引用过它，
;; 给一条没有 Rust 使用点的内容加一个句柄字段只会造出一条「声明了但
;; 从没接线」，见 `ll_mod::class::BaseClassIds` 文档「哪些内容进」一节。
(register-class "lostland:guard" "lostland:class.guard.display_name" "constitution")
