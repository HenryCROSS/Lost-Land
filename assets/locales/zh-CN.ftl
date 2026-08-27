# 迷途大陆本体（命名空间 `lostland`）的中文本地化文件。
#
# 消息 id 用连字符分隔，不是原始 `_key` 字面量里的点号——Fluent 的消息
# 标识符语法不允许点号（点号是消息「属性」的分隔符），装载器
# （crates/ll-i18n）在查表前会把 `_key` 取值里的点号统一换成连字符，
# 详见 `ll_i18n::to_fluent_id` 的文档。下划线不受影响，原样保留。
#
# 键的来源（按代码里的字段分组，方便核对覆盖是否完整）：
#   - window-title                        ll-platform WindowConfig::title_key
#   - keybind-action-*                    ll-platform GameKey::display_name_key
#   - race-*-display_name                 ll-mod RaceAttrs::display_name_key（本体三个种族）
#   - class-*-display_name                ll-mod ClassAttrs::display_name_key（本体十条职业）
#   - subclass-*-display_name             ll-mod SubclassAttrs::display_name_key（本体两个转职）
#   - trait-*-display_name                ll-mod TraitAttrs::display_name_key（本体四条制作精通）
#   - recipe_category-*-display_name       ll-mod RecipeCategoryDef::display_name_key（本体五个配方类别）
#   - item-*-display_name                 ll-mod ItemAttrs::display_name_key（本体二十四件物品）
#   - recipe-*-display_name               ll-mod RecipeAttrs::display_name_key（本体九条配方）
#   - save-mod-missing / save-mod-version-mismatch
#                                          ll-content ModSetMismatch::message_key
#   - mod-dependency-version-mismatch     ll-mod DependencyVersionMismatch::message_key

window-title = 迷途大陆

keybind-action-up = 上
keybind-action-down = 下
keybind-action-left = 左
keybind-action-right = 右
keybind-action-confirm = 确认
keybind-action-cancel = 取消
keybind-action-menu = 菜单
keybind-action-map = 地图
keybind-action-wait = 等待
keybind-action-screenshot = 截图
keybind-action-zoom_in = 放大
keybind-action-zoom_out = 缩小

race-human-display_name = 人类
race-dwarf-display_name = 矮人
race-elf-display_name = 精灵

class-warrior-display_name = 战士
class-mage-display_name = 法师
class-ranger-display_name = 游侠
class-guard-display_name = 卫兵
class-steward-display_name = 据点管理者
class-militia-display_name = 民兵
class-farmer-display_name = 农夫
class-hunter-display_name = 猎户
class-butcher-display_name = 屠夫
class-blacksmith-display_name = 铁匠

subclass-duelist-display_name = 剑术家
subclass-apprentice-display_name = 学徒

subclass-artisan-display_name = 工匠
subclass-tailor-display_name = 裁缝
subclass-alchemist-display_name = 炼金术士
subclass-cook-display_name = 厨师
recipe_category-forging-display_name = 锻造
recipe_category-advanced_forging-display_name = 进阶锻造
recipe_category-tailoring-display_name = 缝纫
recipe_category-alchemy-display_name = 炼金
recipe_category-cooking-display_name = 烹饪

# 本体天赋（mods/lostland/traits.json5，四条制作精通）——
# ll-mod TraitAttrs::display_name_key。
trait-forging_mastery-display_name = 锻造精通
trait-tailoring_mastery-display_name = 缝纫精通
trait-alchemy_mastery-display_name = 炼金精通
trait-cooking_mastery-display_name = 烹饪精通

# 本体物品（mods/lostland/items.json5，三十条）——
# ll-mod ItemAttrs::display_name_key。
item-iron_ingot-display_name = 铁锭
item-iron_rivet-display_name = 铁铆钉
item-linen_cloth-display_name = 亚麻布
item-leather_strip-display_name = 皮革条
item-fur_pelt-display_name = 毛皮
item-herb_bundle-display_name = 草药束
item-raw_meat-display_name = 生肉
item-roast_meat-display_name = 烤肉
item-herbal_draught-display_name = 草药汤剂
item-iron_shortsword-display_name = 铁短剑
item-iron_warpick-display_name = 战镐
item-oak_buckler-display_name = 橡木圆盾
item-forge_brand-display_name = 锻炉烙铁
item-smith_hammer-display_name = 铁匠锤
item-bone_needle-display_name = 骨针
item-iron_helm-display_name = 铁盔
item-leather_jerkin-display_name = 皮甲衣
item-iron_greaves-display_name = 铁胫甲
item-leather_boots-display_name = 皮靴
item-linen_shirt-display_name = 亚麻衬衣
item-fur_mantle-display_name = 毛皮披风
item-forge_apron-display_name = 锻炉围裙
item-wool_gloves-display_name = 羊毛手套
item-amber_pendant-display_name = 琥珀坠
item-traveler_ring-display_name = 旅人指环
item-field_cookbook-display_name = 野外食谱
item-tarnished_signet-display_name = 蒙尘印戒
item-unmarked_phial-display_name = 无标小瓶
item-sealed_relic_box-display_name = 封蜡遗物匣
item-forge-display_name = 锻炉

# 本体配方（mods/lostland/crafting.json5，十条）——
# ll-mod RecipeAttrs::display_name_key。
recipe-roast_meat-display_name = 烤肉
recipe-herb_roast-display_name = 香草烤肉
recipe-herbal_draught-display_name = 草药汤剂
recipe-iron_rivet_batch-display_name = 打一批铁铆钉
recipe-iron_shortsword-display_name = 打铁短剑
recipe-iron_helm-display_name = 打铁盔
recipe-iron_greaves-display_name = 打铁胫甲
recipe-linen_shirt-display_name = 缝亚麻衬衣
recipe-fur_mantle-display_name = 缝毛皮披风
recipe-forge-display_name = 砌锻炉

# 下面两条与下方 mod-dependency-version-mismatch 携带 Fluent 变量
# （`{ $名字 }`），对应结构体字段：ModSetMismatch 的 namespace/
# required_version/current_version，DependencyVersionMismatch 的
# dependent/dependency/required/actual——变量名与字段名故意保持一致，
# 方便对照代码核实没有漏传参数。
save-mod-missing = 存档需要模组 { $namespace }（版本 { $required }），但当前会话未装载该模组。
save-mod-version-mismatch = 存档需要模组 { $namespace } 版本 { $required }，但当前装载的是版本 { $current }。
mod-dependency-version-mismatch = 模组 { $dependent } 依赖 { $dependency } 版本 { $required }，但当前装载的是版本 { $actual }。

# 下面这批键服务只读观测 HUD（ll-ui::hud）——P7 第一批「状态栏/角色
# 面板/背包/装备栏」。来源分组：
#   - hud-status-*                          状态栏（时间/生命/法力，常驻）
#   - hud-character-*                       角色面板（等级/经验/生效中的修正）
#   - rule-modifier-*                       ll-sim RuleModifier 九个变体的展示文案
#   - damage_category-*-display_name        伤害类别展示名（表本身不声明键，见下）
#   - check_context-*-display_name          判定种类展示名（同上）
#   - attribute-*-display_name              AttributeKind 六项主属性名
#   - hud-inventory-*                       背包面板
#   - hud-equipment-*                       装备面板标题与空槽位占位
#   - equip_slot-*-display_name             EquipSlot 22 个引擎具名槽位
#   - season-*-display_name                 Season 四季展示名
#   - weather-*-display_name                本体六种天气展示名
#   - resource-*-display_name               本体四种资源展示名

hud-status-time-label = 时间
hud-status-health-label = 生命
hud-status-mana-label = 法力
hud-status-fps-label = 帧率

# 四季展示名——Tick::season() 现成算出，这里只做展示名映射，不重算
# 季节本身（见 crates/ll-ui/src/hud/status_bar.rs 的 season_key）。
season-spring-display_name = 春
season-summer-display_name = 夏
season-autumn-display_name = 秋
season-winter-display_name = 冬

# 天气展示名——本体六种天气（ll_world::weather::materialize_base_weathers）。
# 天气由 (世界种子, 世界时钟) 纯派生，不进世界状态；这里只做展示名映射，
# 键本身来自 WeatherDef::display_name_key，不是像四季那样写死在 UI 里。
weather-clear-display_name = 晴
weather-overcast-display_name = 阴
weather-rain-display_name = 雨
weather-wind-display_name = 大风
weather-fog-display_name = 雾
weather-snow-display_name = 雪

# 资源展示名——本体四种资源（mods/lostland/resources.json5）。
# 资源点由 (世界种子, 瓦片坐标) 纯派生，不进世界状态；这里只做展示名
# 映射，键本身来自 ResourceAttrs::display_name_key。一座死于资源枯竭的
# 据点，编年史要说出的正是这里的名字
# （ll_world::history::SettlementDemise::ResourceExhausted）。
resource-farmland-display_name = 良田
resource-timber-display_name = 木材
resource-iron_vein-display_name = 铁矿
resource-fresh_water-display_name = 水源

hud-character-panel-title = 角色
hud-character-level-label = 等级
hud-character-experience-label = 经验
hud-character-attribute-points-label = 属性点
hud-character-skill-points-label = 技能点
hud-character-primary-attribute-label = 主属性
hud-character-modifiers-title = 生效中的属性修正
hud-character-modifiers-empty = 无

# 规则修正（ll_sim::rule_modifier）——角色面板「生效中的规则修正」一段。
# 每条消息对应枚举的一个变体，键名由 `RuleModifier` 那边的 *_NAME_KEY
# 常量声明（rule-modifier-*）；实参 $subject（主语显示名，没有主语的
# 变体传空串）、$amount 与 $extra（数值）由
# `ll_sim::rule_modifier::display_shape` 逐变体给出。新增第十个变体时，
# 这里与 en.ftl 各补一条，Rust 侧只改 display_shape 一处。
#
# 数值一律是**合并之后**的（同加值类型取最强、跨类型相加），行尾的
# 来源计数说的是合并前有几条声明——见 RuleModifierDisplay 文档。
hud-character-rule-modifiers-title = 生效中的规则修正
hud-character-rule-modifiers-empty = 无
# 只有一条来源时不出现——满屏「（1 项来源）」只是噪声。这个分支写在
# 这里而不是 Rust 里：哪些数量该怎么念是语言的事。
hud-character-rule-modifier-sources = { $sources ->
    [1] { "" }
   *[other] （{ $sources } 项来源）
}

rule-modifier-resistance = { $subject }抗性 减伤 { $amount }
rule-modifier-vulnerability = { $subject }易伤 增伤 { $amount }
rule-modifier-reroll_once = 重掷 掷出 { $amount } 时重掷一次
rule-modifier-advantage = 优势 { $subject }
rule-modifier-disadvantage = 劣势 { $subject }
rule-modifier-sneak_attack = 偷袭 判定 +{ $amount } 伤害 +{ $extra }
rule-modifier-inspection_suspicion = 盘查减免 +{ $amount }
rule-modifier-inspection_concealment = 藏匿 +{ $amount }
rule-modifier-craft_yield = { $subject }产出 +{ $amount }

# 伤害类别展示名——本体两种（mods/lostland/damage_categories.json5 的
# lostland:fire 与 ll_mod::base_damage_category 的 lostland:physical）。
# 伤害类别表本身**不声明** display_name_key（DamageCategoryDef 只有
# default_formula 一个字段），键按
# `命名空间:damage_category.路径.display_name` 的约定由
# `ll_sim::rule_modifier::subject_key` 现拼——与配方类别自己声明出来的
# 键形状逐字相同，mod 补自己那份 .ftl 即可。
damage_category-physical-display_name = 物理
damage_category-fire-display_name = 火焰

# 判定种类展示名——引擎当前认得三种（ll_sim::check 的 INSPECTION_CHECK
# / CONCEALMENT_CHECK / CRITICAL_CHECK）。同样没有对应的内容表，键走
# 与伤害类别同一条约定。
check_context-inspection-display_name = 盘查
check_context-concealment-display_name = 藏匿
check_context-critical-display_name = 暴击

attribute-strength-display_name = 力量
attribute-dexterity-display_name = 敏捷
attribute-constitution-display_name = 体质
attribute-intelligence-display_name = 智力
attribute-willpower-display_name = 意志
attribute-charisma-display_name = 魅力
attribute-luck-display_name = 幸运

hud-inventory-panel-title = 背包
hud-inventory-empty = （空）
hud-inventory-durability-label = 耐久
hud-item-unidentified = 未鉴定的物品

hud-equipment-panel-title = 装备
hud-equipment-empty-slot = （空）

equip_slot-main_hand-display_name = 主手
equip_slot-off_hand-display_name = 副手
equip_slot-head-display_name = 头部
equip_slot-face-display_name = 面部
equip_slot-eyes-display_name = 眼部
equip_slot-neck-display_name = 颈部
equip_slot-body-display_name = 躯干
equip_slot-outer-display_name = 外袍
equip_slot-back-display_name = 背部
equip_slot-shoulder_l-display_name = 左肩
equip_slot-shoulder_r-display_name = 右肩
equip_slot-arm_l-display_name = 左臂
equip_slot-arm_r-display_name = 右臂
equip_slot-hand_l-display_name = 左手
equip_slot-hand_r-display_name = 右手
equip_slot-belt-display_name = 腰带
equip_slot-tasset-display_name = 腿甲
equip_slot-legs-display_name = 双腿
equip_slot-boot_l-display_name = 左靴
equip_slot-boot_r-display_name = 右靴
equip_slot-ring_l-display_name = 左戒指
equip_slot-ring_r-display_name = 右戒指
equip_slot-unknown-display_name = 未知槽位
