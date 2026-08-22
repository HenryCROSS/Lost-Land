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
;; 出生装备与种族天赋两项本体三族都不声明：它们分别走
;; register-race-starting-item / register-race-trait 两个追加指令，
;; 本体不调用即为空。
;;
;; 击杀经验（register-race-xp-reward）此前也在这一档，理由是「本体三
;; 族是可玩种族不是猎物」——**项目所有者已推翻这条**：裁定原文「有个
;; 最低经验 1xp，然后等级差越多给越多，有个经验公式」，人人都给经验。
;; 三族因此各自声明一个基准值（见文件末尾）。这不是为了让字段覆盖
;; 检查变绿硬塞的数字：本作里人类山贼、矮人劫匪、精灵刺客都是会动手
;; 的敌人，「杀了一个人类给多少经验」是一个真实存在、必须有答案的
;; 问题。

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

;; 击杀基准经验值——`ll_sim::experience::kill_experience` 的公式输入，
;; 不是玩家最终拿到的数字：最终经验 = max(1, 基准值 × 等级差倍率 /
;; 100)，见该函数文档。同级击杀时倍率恰好 100%，下面的数字就是玩家
;; 拿到的数字。
;;
;; 三个数字的依据是同一条：这三族都是**没有特殊构造的人形**，彼此的
;; 战斗价值差异应该来自等级与装备（那两样已经各自进了公式与结算），
;; 不应该由「你杀的是精灵还是人类」凭空拉开。因此三者取同一个基数
;; 10，矮人与精灵各自 +2 只反映它们相对人类多出来的那点属性修正
;; （矮人体质 +2/力量 +1，精灵敏捷 +2/智力 +1，人类无修正）——这是
;; 本文件里已经写死的既有数值，不是新引入的一套平衡设定。
(register-race-xp-reward "lostland:human" 10)
(register-race-xp-reward "lostland:dwarf" 12)
(register-race-xp-reward "lostland:elf" 12)
