# 示例 mod 的中文文案。
#
# # 这份文件证明了什么
#
# 「mod 能带自己的本地化」此前是**零实现**：全仓库唯一的装载点只读本体
# 一个目录，`mods/*/locales/` 连目录都不存在（见
# knowledge/design/dialogue-system.md 三节 3.2）。本目录是那条缺口被补上
# 之后的第一份真实证据——本 mod 声明的每一个 display_name_key，在真实
# 装载管线下解析出的都是**这里**的文字，不是键名、也不是本体的文字。
#
# # 键怎么写：不要把命名空间再写进键名
#
# 内容文件里写的是 `examplemod:iron_sword_display_name`；这里的条目 id
# 只写 `iron_sword_display_name`——「这个键属于哪个命名空间」由**文件所在
# 的目录**回答（mod-package-structure.md「本地化文件：为什么键不需要再
# 编码命名空间」一节）。冒号本来也不是 Fluent 的合法 id 字符。
# 键路径里的点号在查表前会被换成连字符（`weather.ashfall.display_name`
# → `weather-ashfall-display_name`），见 ll_i18n::split_key。

## 职业
necromancer_display_name = 死灵法师
rogue_display_name = 游侠

## 配方类别与配方
recipe_category_cooking_display_name = 烹饪（示例）
recipe_category_forging_display_name = 锻造（示例）
roast_meat_recipe_display_name = 烤肉
iron_sword_recipe_display_name = 铁剑锻造
iron_sword_from_scrap_display_name = 废铁重铸铁剑
arrow_batch_recipe_display_name = 箭矢（一打）
herb_stew_recipe_display_name = 草药炖汤

## 伤害类别
damage_category_acid_display_name = 酸蚀

## 物品
arrow_display_name = 箭矢
iron_sword_display_name = 铁剑
war_hammer_display_name = 战锤
wooden_shield_display_name = 木盾
healing_potion_display_name = 治疗药水
crude_dagger_display_name = 粗制匕首
flame_longbow_display_name = 烈焰长弓
acid_dagger_display_name = 蚀骨匕首
acid_ward_amulet_display_name = 抗酸护符
wool_liner_display_name = 羊毛内衬
fur_cloak_display_name = 毛皮斗篷
raw_meat_display_name = 生肉
roast_meat_display_name = 烤肉
iron_ingot_display_name = 铁锭
wild_herb_display_name = 野生草药
herb_stew_display_name = 草药炖汤
cookbook_display_name = 食谱手札
portable_anvil_display_name = 便携铁砧

## 种族
half_elf_display_name = 半精灵
goblin_display_name = 哥布林（示例）
dragonborn_display_name = 龙裔
elf_display_name = 精灵（示例）
gnome_display_name = 侏儒
ooze_display_name = 软泥怪
footpad_display_name = 拦路贼

## 资源池
sorcery_points_display_name = 术法点
wizard_spell_slots_display_name = 法师法术位
druid_slots_display_name = 德鲁伊法术位

## 副职
shadowdancer_display_name = 影舞者

## 天赋
draconic_breath_display_name = 龙息
innate_sorcery_display_name = 天生术法
arcane_casting_display_name = 奥术施法
druidic_casting_display_name = 德鲁伊施法
acid_hide_display_name = 酸蚀皮膜
predatory_instinct_display_name = 掠食本能
shadow_dance_display_name = 暗影之舞
cutpurse_training_display_name = 扒窃训练

## 天气
weather-ashfall-display_name = 落灰

## 撞键回归夹具：**故意**与本体同 id
#
# 下面两条的 id 与本体 assets/locales/zh-CN.ftl 里的条目**逐字相同**。
# 它们是活的回归夹具，不是笔误——先例是本 mod 的
# assets/overrides/lostland/sprites/terrain_dirt.png，那张图同样只为了
# 让资产覆盖机制有一份真实证据而存在。
#
# 命名空间维度落地之前，两个 mod 写同一个消息 id 会**静默**互相覆盖
# （或者后一份整个文件装不进来），而 mod 恒在本体之后装载，也就是说
# 一个第三方 mod 可以不声不响地改掉游戏本体的文案。落地之后：
#
# - `examplemod:race.elf.display_name` → 这里的「示例模组的精灵」
# - `lostland:race.elf.display_name`   → 本体的「精灵」
# - `hud-inventory-empty`（**裸键，无命名空间前缀**）→ 恒定落到本体，
#   本 mod 这一条永远不会被任何人查到。
#
# 三条断言在 crates/ll-game/tests/mod_locales.rs 里。
race-elf-display_name = 示例模组的精灵
hud-inventory-empty = 示例模组不该劫持本体的这条文案
