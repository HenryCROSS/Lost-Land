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
#   - class-*-display_name                ll-mod ClassAttrs::display_name_key（本体三个职业）
#   - subclass-*-display_name             ll-mod SubclassAttrs::display_name_key（本体两个转职）
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

subclass-duelist-display_name = 剑术家
subclass-apprentice-display_name = 学徒

subclass-artisan-display_name = 工匠
subclass-tailor-display_name = 裁缝
subclass-alchemist-display_name = 炼金术士
subclass-cook-display_name = 厨师
recipe_category-forging-display_name = 锻造
recipe_category-tailoring-display_name = 缝纫
recipe_category-alchemy-display_name = 炼金
recipe_category-cooking-display_name = 烹饪

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
#   - attribute-*-display_name              AttributeKind 六项主属性名
#   - hud-inventory-*                       背包面板
#   - hud-equipment-*                       装备面板标题与空槽位占位
#   - equip_slot-*-display_name             EquipSlot 22 个引擎具名槽位
#   - season-*-display_name                 Season 四季展示名
#   - weather-*-display_name                本体六种天气展示名

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

hud-character-panel-title = 角色
hud-character-level-label = 等级
hud-character-experience-label = 经验
hud-character-attribute-points-label = 属性点
hud-character-skill-points-label = 技能点
hud-character-primary-attribute-label = 主属性
hud-character-modifiers-title = 生效中的属性修正
hud-character-modifiers-empty = 无

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
