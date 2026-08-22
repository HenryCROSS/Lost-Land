;; 本体三个基础种族——逐字迁移自原先硬编码在 Rust 里的
;; `ll_mod::race::materialize_base_races`（该函数已随本次迁移删除）。
;;
;; 签名见 crates/ll-mod/src/script_race_api.rs：
;;   (register-race id display-name-key
;;                  strength dexterity constitution intelligence willpower charisma
;;                  darkvision-floor footprint-width footprint-height lifespan-years)
;; 六个属性参数是**固定增减量**（可为负），不是千分比。
;;
;; 三个种族演示三种不同的修正取向：人类（无修正，种族设计里惯常的
;; 「基准种族」）、矮人（体质向 + 暗视）、精灵（敏捷/智力向 + 长寿）。
;; 具体数值不是本次迁移引入的平衡设计，是原 Rust 常量的原样搬运——
;; `crates/ll-mod/tests/base_mod_races.rs` 逐字段钉住它们，
;; `ll_mod::content_hash` 的值哈希覆盖同一批字段。
;;
;; 显示名键（lostland:race.*.display_name）对应
;; assets/locales/{en,zh-CN}.ftl 里已有的条目，迁移不改动任何一条——
;; id 与本地化键的对应关系与迁移前逐字相同。
;;
;; 出生装备/击杀经验/种族天赋三项本体三族都不声明：它们分别走
;; register-race-starting-item / register-race-xp-reward /
;; register-race-trait 三个追加指令，本体不调用即为空，与迁移前
;; `RaceAttrs { xp_reward: 0, traits: [], starting_items: [] }` 一致。

;; 人类：无任何属性修正，无暗视，寿命 80 年。
(register-race "lostland:human" "lostland:race.human.display_name"
               0 0 0 0 0 0
               0 1 1 80)

;; 矮人：力量 +1、体质 +2，暗视下限 4（明显高于全黑的 0、明显低于满
;; 光照），寿命 250 年。
(register-race "lostland:dwarf" "lostland:race.dwarf.display_name"
               1 0 2 0 0 0
               4 1 1 250)

;; 精灵：敏捷 +2、智力 +1，无暗视，寿命 400 年。
(register-race "lostland:elf" "lostland:race.elf.display_name"
               0 2 0 1 0 0
               0 1 1 400)
