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
#   - class-*-display_name                ll-mod ClassAttrs::display_name_key（本体十三条职业）
#   - subclass-*-display_name             ll-mod SubclassAttrs::display_name_key（本体两个转职）
#   - trait-*-display_name                ll-mod TraitAttrs::display_name_key（本体四条制作精通）
#   - recipe_category-*-display_name       ll-mod RecipeCategoryDef::display_name_key（本体五个配方类别）
#   - item-*-display_name                 ll-mod ItemAttrs::display_name_key（本体三十六件物品）
#   - recipe-*-display_name               ll-mod RecipeAttrs::display_name_key（本体十二条配方）
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
keybind-action-inventory = 背包
keybind-action-craft = 制作
keybind-action-pick_up = 拾取
keybind-action-drop = 丢弃
keybind-action-equip = 装备
keybind-action-use = 使用
keybind-action-place = 放置
keybind-action-interact = 交互

# 角色性别（ll_world::entity::Gender::display_name_key）——不是内容，
# 因此键长在 Rust 侧那个枚举上，不在任何内容表里。
gender-male-display_name = 男性
gender-female-display_name = 女性

race-human-display_name = 人类
race-dwarf-display_name = 矮人
race-elf-display_name = 精灵
race-goblin-display_name = 哥布林

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
class-fisher-display_name = 渔夫
class-shepherd-display_name = 牧羊人
class-mason-display_name = 石匠

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

# 尸体（ll-mod corpse_item：每个种族自动获得一件尸体物品）。
# 物种那一半靠 $species 参数插进来，取的是该种族自己的 display_name_key
# ——因此第三方 mod 加一个种族，它的尸体名自动就有，不需要再补一条键。
# 这是本条消息必须带参数、而不是每族一条键的全部理由，见
# ll_mod::corpse_item 模块文档。
item-corpse-display_name = { $species }的尸体

# 本体物品（mods/lostland/items.json5，三十六条）——
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
# 家具第二批——填满住宅／作坊／仓库／酒馆四类建筑的六件。哪一件
# 归哪类建筑，见 mods/lostland/items.json5 那一节的表格。
item-oak_chair-display_name = 橡木椅
item-oak_table-display_name = 橡木长桌
item-fur_bed-display_name = 毛皮卧铺
item-oak_bookshelf-display_name = 橡木书柜
item-oak_barrel-display_name = 橡木酒桶
item-iron_bound_chest-display_name = 铁箍箱

# 本体配方（mods/lostland/crafting.json5，十二条）——
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
recipe-fur_bed-display_name = 铺毛皮卧铺
recipe-iron_bound_chest-display_name = 打铁箍箱

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
#   - damage_category-*-display_name        ll-mod DamageCategoryDef::display_name_key（本体两个伤害类别）
#   - check_context-*-display_name          判定种类展示名（引擎侧开放标识符，无内容表，见下）
#   - attribute-*-display_name              AttributeKind 六项主属性名
#   - hud-inventory-*                       背包面板
#   - hud-equipment-*                       装备面板标题与空槽位占位
#   - hud-inventory-menu-* / hud-craft-*    背包/制作菜单（ll-game player_action）
#   - hud-feedback-*                        操作反馈行（ll-game player_action::Feedback）
#   - equip_slot-*-display_name             EquipSlot 22 个引擎具名槽位
#   - season-*-display_name                 Season 四季展示名
#   - weather-*-display_name                本体六种天气展示名
#   - resource-*-display_name               本体七种资源展示名

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

# 资源展示名——本体七种资源（mods/lostland/resources.json5）。
# 资源点由 (世界种子, 瓦片坐标) 纯派生，不进世界状态；这里只做展示名
# 映射，键本身来自 ResourceAttrs::display_name_key。一座死于资源枯竭的
# 据点，编年史要说出的正是这里的名字
# （ll_world::history::SettlementDemise::ResourceExhausted）。
resource-farmland-display_name = 良田
resource-pasture-display_name = 牧场
resource-timber-display_name = 木材
resource-iron_vein-display_name = 铁矿
resource-granite-display_name = 花岗岩
resource-fresh_water-display_name = 水源
resource-fishery-display_name = 渔场

# 文化（文化批次）——每座据点恰好一份，见 crates/ll-world/src/culture.rs
# 与 mods/lostland/cultures.json5。
culture-farmstead-display_name = 农庄
culture-mining_hold-display_name = 矿邑
culture-forest_kin-display_name = 林居
culture-harbour-display_name = 渔港
culture-stonecutters-display_name = 石砦
culture-goblin_warband-display_name = 哥布林部落

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
# 下面这两条键**不是**约定拼出来的：DamageCategoryDef 有一个必填的
# display_name_key 字段，那两处内容各自逐字声明了这里的键，呈现层读的
# 就是它（见 crates/ll-mod/src/damage_category.rs 模块文档「显示名字段」
# 一节）。mod 想叫什么名字，写自己的键、补自己那份 .ftl 即可。
damage_category-physical-display_name = 物理
damage_category-fire-display_name = 火焰

# 判定种类展示名——引擎当前认得三种（ll_sim::check 的 INSPECTION_CHECK
# / CONCEALMENT_CHECK / CRITICAL_CHECK）。这三条与上面两条不同：判定
# 种类是引擎侧的开放标识符，**没有内容表**可以声明显示名，键按
# `命名空间:check_context.路径.display_name` 由
# ll_sim::rule_modifier::subject_key 现拼——那是本仓库仅剩的一处拼键，
# 理由与代价见该函数文档。
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

# 背包菜单（I 键）与制作菜单（C 键）——ll_game::player_action 的
# menu_data，经 ll_ui::hud::action_menu 画出。
hud-inventory-menu-title = 背包（上下选择）
hud-inventory-menu-empty = 空空如也
hud-inventory-menu-hint = 装备/卸下=E　使用=U　丢弃=X　放置=P　关闭=Esc
hud-inventory-menu-equipped-label = 已装备
hud-craft-menu-title = 制作（上下选择）
hud-craft-menu-empty = 没有任何配方
hud-craft-menu-hint = 制作=Enter　关闭=Esc
hud-craft-station-label = 场地
hud-craft-tool-label = 工具

# 交互菜单（空格键）——ll_game::player_action 的 InteractTarget。
hud-interact-menu-title = 脚下（上下选择）
hud-interact-menu-empty = 什么都没有
hud-interact-menu-hint = 确认=Enter　捡起=G　关闭=Esc
hud-interact-action-work = 在此开工
hud-interact-action-loot = 搜刮
hud-interact-action-take = 捡起
# 门（交互列表批次）——门是**地形**不是物品，因此名字不走 ItemTable，
# 用这两条 HUD 通用文案，理由见 ll_game::player_action 的
# interact_target_name 文档。
hud-interact-action-open_door = 开门
hud-interact-action-close_door = 关门
hud-interact-door-closed = 一扇关着的门
hud-interact-door-open = 一扇开着的门
hud-interact-direction-title = 和哪边的东西交互（上下选择）
hud-interact-direction-prompt = 附近什么都没有
hud-interact-direction-hint = 确认=Enter　关闭=Esc
hud-interact-direction-more = 等

# 方向名——ll_game::player_action::direction_key，方向列表每一行的前缀。
hud-direction-here = 脚下
hud-direction-north = 北
hud-direction-north_east = 东北
hud-direction-east = 东
hud-direction-south_east = 东南
hud-direction-south = 南
hud-direction-south_west = 西南
hud-direction-west = 西
hud-direction-north_west = 西北

# 操作反馈行——ll_game::player_action::Feedback，见
# ll_sim::turn::PlayerTurnOutcome 文档「静默作废对玩家不成立」。
hud-feedback-no-selection = 没有可操作的条目
hud-feedback-nothing-happened = 这一下没有起作用
hud-feedback-nothing-nearby = 附近没有可交互的东西
hud-feedback-door-blocked-occupant = 门口有人挡着，关不上
hud-feedback-door-blocked-object = 门口立着东西，关不上

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

# 地形形态预设（世界生成参数落地批次）：
#   - worldgen-preset-*-display_name / -description
#     ll-content world_identity::TERRAIN_PRESETS（本体四档地形预设）
# 说明文字里的百分比是「标准」尺寸（96×64 区块）下十个种子的实测均值，
# 数据来源见 docs/worldgen-tuning.md。
worldgen-preset-continent-display_name = 大陆
worldgen-preset-continent-description = 一整块连绵的大陆，海洋只在边缘。水域约占三成七，山地稀少。
worldgen-preset-archipelago-display_name = 群岛
worldgen-preset-archipelago-description = 汪洋之中散落着数百座岛屿，没有哪一座称得上大陆。水域约占七成三。
worldgen-preset-highland-display_name = 山地
worldgen-preset-highland-description = 陆地为主的高原世界，群山连绵。山地约占两成四，水域仅两成五。
worldgen-preset-inland-display_name = 内陆
worldgen-preset-inland-description = 几乎不见海洋的内陆世界，水域仅约一成六，渔猎让位给农牧。

# 游戏内菜单与设置界面（P7 收尾批次）：
#   - screen-menu-* / screen-settings-*
#     ll-ui 的 screen 模块；每一行的文字本身由 ll-game 的 menu_screen
#     排好版之后作为字符串传进去，见该模块文档。
screen-menu-title = 菜单
screen-menu-empty = （没有条目）
screen-menu-hint = 上下移动，确认键选择，Esc 关闭
screen-menu-continue = 继续游戏
screen-menu-save = 保存游戏
screen-menu-settings = 设置
screen-menu-back-to-title = 返回主菜单
screen-menu-quit = 退出游戏
screen-menu-game-saved = 进度已保存
screen-menu-game-save-failed = 存档写入失败，进度还在，但没有存下来

screen-settings-title = 设置
screen-settings-empty = （没有条目）
screen-settings-hint = 左右切换取值，确认键改键位，Esc 返回
screen-settings-capture-hint = 按下要绑定的键；退格键解绑，Esc 取消
screen-settings-language = 语言
screen-settings-vsync = 垂直同步
screen-settings-scale-filter = 缩放滤波
screen-settings-save = 保存到配置文件
screen-settings-back = 返回
screen-settings-keybinds-header = --- 键位（游戏内） ---
screen-settings-on = 开
screen-settings-off = 关
screen-settings-filter-nearest = 最近邻
screen-settings-filter-sharp-bilinear = 锐利双线性
screen-settings-restart-required = （重启后生效）
screen-settings-unbound = （未绑定）
screen-settings-capturing = ……请按键……
screen-settings-row = { $label }：{ $value }
screen-settings-conflict = 这个键已经绑给了{ $action }
screen-settings-bound = 已绑定{ $action }
screen-settings-cleared = 已清除{ $action }的键位
screen-settings-saved = 设置已保存（config.json5 里手写的注释会丢失）
screen-settings-save-failed = 配置文件写入失败，本次会话内改动仍然有效

# 语言自称（endonym）——设置界面的语言选单每一项用自己的文字写，
# 见 ll_game::menu_screen::language_display_name。
language-name = 简体中文
# 世界地图（M 键切换的大陆概览浮层，ll-ui::hud::world_map）——缩放批次。
# hud-world-map-scale-label 后面紧跟 "1:<每格覆盖的瓦片数>"，见
# crates/ll-ui/src/hud/world_map.rs 的 scale_caption。
hud-world-map-scale-label = 比例尺
hud-world-map-hint = 方向键平移，缩放键放大缩小

# 游戏主菜单（首页，批次 6）：
#   - screen-title-*
#     与游戏内菜单共用 ll-ui 的同一个 screen 模块，唯一的区别是它底下
#     还没有世界。
screen-title-title = 迷途大陆
screen-title-empty = （没有条目）
screen-title-hint = 上下移动，确认键选择
screen-title-new-game = 开始游戏
screen-title-load = 读取存档
screen-title-load-empty = 读取存档（没有存档）
screen-title-settings = 设置
screen-title-quit = 离开
screen-title-no-save = 还没有可以读取的存档
screen-title-load-failed = 存档读不回来，什么都没有改变

# 角色创建 / 世界配置 / 选出生地三块屏（ll_game::chargen / world_setup /
# spawn_pick）——所有者裁定的开局三步：「设置种族，性别，职业。然后设置
# 历史生成的配置。接着就是选择地图上在哪重生。」
screen-chargen-title = 创建角色
screen-chargen-empty = （没有条目）
screen-chargen-hint = 上下移动，左右改选项，确认键选择，Esc 返回
screen-chargen-race = 种族
screen-chargen-gender = 性别
screen-chargen-profession = 职业
screen-chargen-next = 下一步：世界配置
screen-chargen-back = 返回首页

screen-worldsetup-title = 世界配置
screen-worldsetup-hint = 上下移动，左右调整，确认键生成世界，Esc 返回
screen-worldsetup-preset = 地形预设
screen-worldsetup-sea-level = 海平面（千分比）
screen-worldsetup-mountain-level = 山地阈值（千分比）
screen-worldsetup-octaves = 噪声倍频层数
screen-worldsetup-continent-shrink = 大陆缩减档位
screen-worldsetup-climate-band-width = 气候带宽（千分比）
screen-worldsetup-generate = 生成世界
screen-worldsetup-back = 返回角色创建
screen-worldsetup-invalid = 这个取值超出合法范围，本次调整已撤销

screen-spawnpick-title = 选择出生地
screen-spawnpick-hint = 方向键移动光标或用鼠标点击，确认键在此出生，Esc 返回
screen-spawnpick-no-land = 这个区块里没有能落脚的陆地，换一个地方

# 存档列表 / 命名 / 存档模式（批次 9）
screen-savelist-title = 读取存档
screen-savelist-empty = （没有条目）
screen-savelist-empty-row = 还没有任何存档
screen-savelist-hint = 上下移动，确认键读取，Esc 返回
screen-savelist-row = { $name } · { $time } · { $mode }
screen-savelist-mode-roguelike = 肉鸽
screen-savelist-mode-normal = 普通
screen-savelist-legacy-name = 旧存档
screen-savename-title = 给这份存档起个名字
screen-savename-hint = 直接输入即可（输入法可用，中文/大写/标点都行），退格删除，确认键开始游戏，Esc 返回
screen-savename-prompt = 名称
screen-savename-default = 无名之地
screen-worldsetup-mode = 存档模式
screen-chargen-player-died = 你死了。这个世界保留下来，模式转为普通——再造一个角色，另选一处出生。

## 对话（对话内容表批次）
#
# 结构在 mods/lostland/dialogues.json5，文案在这里。两边靠 text_key 相连，
# 一句台词的措辞怎么改都不触碰 JSON5，反过来加一条选项也不需要动这里已有的
# 任何一行——这正是「结构在 JSON5、文案在 .ftl」这条边界买到的东西。
#
# 台词里不出现 NPC 的名字：`Agent` 今天没有 name 字段（设计文档三节 3.4
# 采纳的是「第一批用职业显示名代替」那一条）。等 NPC 姓名那一批落地，把这里
# 的称呼换成 { $npc_name } 即可，mods/**/dialogues.json5 一个字都不用改。

dialogue-common-farewell = （告辞）
dialogue-common-back = （还有别的事）

dialogue-steward-root = 管理者从账册上抬起头。「又一个外乡人。说吧，你要什么。」
dialogue-steward-ask_join = 我想在这里落脚。
dialogue-steward-join = 他把笔搁下，在名册末尾添了一行。「记上了。这一带的事，从今天起也有你一份。」
dialogue-steward-ask_duties = 我该做些什么？
dialogue-steward-duties = 「守好你那一段墙，按时缴税，别在集市上动刀子。」他数着，「就这三条。」
dialogue-steward-ask_tax = 今年的税，能不能宽限几日？
dialogue-steward-tax = 他盯了你一会儿。「看在你这些年的份上——十日。多一日都没有。」
dialogue-steward-ask_kin = 我也是从麦垄里出来的。
dialogue-steward-kin = 「哦？」他的语气松了半分，「那你该知道谷子什么时候该收。坐吧。」
dialogue-steward-ask_work = 有什么活干吗？
dialogue-steward-work = 「山道上不干净。」他朝北边扬了扬下巴，「你要是有那个本事，去清一清。」
dialogue-steward-ask_reward = 我把事办完了。
dialogue-steward-reward = 「我听说了。」他推过来一份干粮，「先拿着，库房清点完还有。」

dialogue-guard-root = 卫兵横过长戟。「矿堡不接外客。想进去，先说个理由。」
dialogue-guard-ask_toll = 过路的规矩我懂。
dialogue-guard-toll = 他掂了掂钱袋，侧身让开半步。「规矩懂就好。别往深处走。」
dialogue-guard-show_signet = （出示那枚发乌的印记）
dialogue-guard-signet = 他的眼神变了。「……这东西你从哪儿来的？进去吧，别声张。」
dialogue-guard-kinsman = 石头认得石头。
dialogue-guard-kin = 「哈！」他咧开嘴，「山下头难得见着自家人。进去吧。」
dialogue-guard-ask_rumour = 最近听到什么风声没有？
dialogue-guard-rumour = 他压低声音。「三坑那边，夜里有声音。管事的说是塌方，可塌方不会一夜一夜地响。」
dialogue-guard-ask_rumour_again = 你刚才说的三坑，再讲讲。
dialogue-guard-rumour_again = 「我什么都没说过。」他重新横起长戟，「你也一样。」

# ── 会话屏与交互列表的对话那一行（批次 21）───────────────────────
# 会话屏的标题不在这里：它是 NPC 那一句台词，键由内容表的 text_key
# 现给（见 crates/ll-game/src/dialogue_screen.rs 模块文档那张表）。

screen-dialogue-empty = （没有话可说）
screen-dialogue-hint = 上下选择 · 回车确认 · Esc 结束对话
screen-dialogue-missing = （他张了张嘴，没有出声）
hud-interact-action-talk = 交谈
hud-interact-someone = 一个人

# ── 「世界正在推进」（批次 23，规格 F4）─────────────────────────
# 连续多帧轮不到玩家时才出现，见 crates/ll-game/src/app.rs 的
# NOT_YET_FEEDBACK_FRAMES。

hud-feedback-world-advancing = 世界正在推进…

# ── 世界层底部那一行常驻按键提示（批次 23，规格 F6）─────────────
# 键名是运行期插值：走 crates/ll-game/src/key_hint.rs，从玩家当前的
# 键位表现查，重绑之后这一行跟着变。

hud-key-hint-world = { $inventory } 背包　{ $craft } 制作　{ $interact } 交互　{ $map } 地图　{ $menu } 菜单
hud-key-hint-map = 方向键 平移　{ $zoom_in } / { $zoom_out } 缩放　{ $close } 关闭
# ── 五个新种族（批次 24）───────────────────────────────────────────
# 五族的数值由项目所有者批准，见
# knowledge/handoff/2026-08-28-session-handoff.md 第六节。声明在
# mods/lostland/races.json5，追加在注册表末尾。
# 这几条只是展示名：贴图查找用的是 id 的本地名（camelfolk / catfolk /
# orc / lizardfolk / merfolk），永远不用译名。

race-camelfolk-display_name = 骆驼人
race-catfolk-display_name = 猫人
race-orc-display_name = 欧克
race-lizardfolk-display_name = 蜥蜴人
race-merfolk-display_name = 鱼人

# ── 沙漠文化（批次 24）─────────────────────────────────────────────
# 第七份文化：食物 × 沙漠。气候条带让沙漠真实存在之后才写得出来的
# 一条。声明在 mods/lostland/cultures.json5。

culture-sand_nomads-display_name = 沙民

# ── 任务链的两行（批次 29，对话系统的批次 4）─────────────────────
# 挂在管理者那段上的两条**带后果**的选项：交差走 complete-quest（调既有
# 的 mark_quest_completed），领赏走 give-item（含 owner 校验硬前置）。
# 上面 dialogue-steward-reward 那一句同批改写过：原文是「等库房清点完，
# 该给你的一样不少」——东西当场交到手上之后那句话不再成立。改写而不是
# 新增一个节点，同批次 26 第 7 条裁定。

dialogue-steward-report = 山道我已经走过一趟了。
dialogue-steward-take_reward = 那我就收下了。

# ── 交易（批次 31，对话系统的批次 5）─────────────────────────────
# 管理者开场白上那一行 open-trade 选项，以及交易屏自己的五条。
# 行文案走具名参数插值（Catalog::resolve_with_args），**不拼字符串**
# ——语序在不同语言里不一样，见 ADR 0019 B-2 那段论证。
# 价钱的单位是「最小货币单位」，与 ItemDef.base_price 那个 Milli 的
# 最小单位是同一个（见 ll_sim::item::ItemRule::base_price 文档）。

dialogue-steward-ask_trade = 你这儿有什么可换的？
screen-trade-title = 交易
screen-trade-empty = 两边都拿不出可换的东西。
screen-trade-hint = ↑↓ 选择　确认 成交　取消 离开
screen-trade-buy = 买　{ $item }（{ $count }）　{ $price }
screen-trade-sell = 卖　{ $item }（{ $count }）　{ $price }

# ── 树木（批次 32）───────────────────────────────────────────────
# 两件内容物品（砍伐出木料、采果出树种，树种又被培植消耗）、三条交互
# 动作、三个树名。树名走 HUD 文案键而不是物品表：树**不是**
# `ground_items` 里的一条——一百万棵以上，它们是派生出来的、不存储
# （ADR 0009）。与门那两条键同一档，见 ll-game 的 `interact_target_name`。

item-timber_log-display_name = 木料
item-tree_seed-display_name = 树种
hud-interact-action-fell = 砍伐
hud-interact-action-harvest = 采果
hud-interact-action-plant = 培植
hud-interact-tree-oak = 一棵橡树
hud-interact-tree-pine = 一棵松树
hud-interact-tree-palm = 一棵棕榈
