# English localization for the Lost Land base game (namespace `lostland`).
#
# Message ids use hyphens, not the dots that appear in the raw `_key`
# literals in source — Fluent's message identifier grammar does not allow
# dots (a dot introduces a message *attribute*). The loader
# (crates/ll-i18n) rewrites dots to hyphens before lookup; see
# `ll_i18n::to_fluent_id` for the exact rule. Underscores are left as-is.
#
# See assets/locales/zh-CN.ftl for the source-field mapping of every key
# below — kept in one place so the two files don't drift out of sync.

window-title = Lost Land

keybind-action-up = Up
keybind-action-down = Down
keybind-action-left = Left
keybind-action-right = Right
keybind-action-confirm = Confirm
keybind-action-cancel = Cancel
keybind-action-menu = Menu
keybind-action-map = Map
keybind-action-wait = Wait
keybind-action-screenshot = Screenshot
keybind-action-zoom_in = Zoom In
keybind-action-zoom_out = Zoom Out

race-human-display_name = Human
race-dwarf-display_name = Dwarf
race-elf-display_name = Elf

class-warrior-display_name = Warrior
class-mage-display_name = Mage
class-ranger-display_name = Ranger

subclass-duelist-display_name = Duelist
subclass-apprentice-display_name = Apprentice

save-mod-missing = This save requires mod { $namespace } (version { $required }), which is not loaded in the current session.
save-mod-version-mismatch = This save requires mod { $namespace } version { $required }, but the loaded version is { $current }.
mod-dependency-version-mismatch = Mod { $dependent } depends on { $dependency } version { $required }, but the loaded version is { $actual }.
