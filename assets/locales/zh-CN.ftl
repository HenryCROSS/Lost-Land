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

subclass-duelist-display_name = 剑术家
subclass-apprentice-display_name = 学徒

# 下面两条与下方 mod-dependency-version-mismatch 携带 Fluent 变量
# （`{ $名字 }`），对应结构体字段：ModSetMismatch 的 namespace/
# required_version/current_version，DependencyVersionMismatch 的
# dependent/dependency/required/actual——变量名与字段名故意保持一致，
# 方便对照代码核实没有漏传参数。
save-mod-missing = 存档需要模组 { $namespace }（版本 { $required }），但当前会话未装载该模组。
save-mod-version-mismatch = 存档需要模组 { $namespace } 版本 { $required }，但当前装载的是版本 { $current }。
mod-dependency-version-mismatch = 模组 { $dependent } 依赖 { $dependency } 版本 { $required }，但当前装载的是版本 { $actual }。
