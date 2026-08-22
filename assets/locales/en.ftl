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

# The keys below serve the read-only observation HUD (ll-ui::hud) — P7
# batch one ("status bar / character panel / inventory / equipment").
# See assets/locales/zh-CN.ftl for the source-field grouping.

hud-status-time-label = Time
hud-status-health-label = HP
hud-status-mana-label = MP
hud-status-fps-label = FPS

# Season display names — Tick::season() already computes the season;
# these are only the display-name mapping (see season_key in
# crates/ll-ui/src/hud/status_bar.rs).
season-spring-display_name = Spring
season-summer-display_name = Summer
season-autumn-display_name = Autumn
season-winter-display_name = Winter

hud-character-panel-title = Character
hud-character-level-label = Level
hud-character-experience-label = XP
hud-character-modifiers-title = Active Modifiers
hud-character-modifiers-empty = None

attribute-strength-display_name = Strength
attribute-dexterity-display_name = Dexterity
attribute-constitution-display_name = Constitution
attribute-intelligence-display_name = Intelligence
attribute-willpower-display_name = Willpower
attribute-charisma-display_name = Charisma
attribute-luck-display_name = Luck

hud-inventory-panel-title = Inventory
hud-inventory-empty = (empty)
hud-inventory-durability-label = Durability

hud-equipment-panel-title = Equipment
hud-equipment-empty-slot = (empty)

equip_slot-main_hand-display_name = Main Hand
equip_slot-off_hand-display_name = Off Hand
equip_slot-head-display_name = Head
equip_slot-face-display_name = Face
equip_slot-eyes-display_name = Eyes
equip_slot-neck-display_name = Neck
equip_slot-body-display_name = Body
equip_slot-outer-display_name = Outer
equip_slot-back-display_name = Back
equip_slot-shoulder_l-display_name = Left Shoulder
equip_slot-shoulder_r-display_name = Right Shoulder
equip_slot-arm_l-display_name = Left Arm
equip_slot-arm_r-display_name = Right Arm
equip_slot-hand_l-display_name = Left Hand
equip_slot-hand_r-display_name = Right Hand
equip_slot-belt-display_name = Belt
equip_slot-tasset-display_name = Tasset
equip_slot-legs-display_name = Legs
equip_slot-boot_l-display_name = Left Boot
equip_slot-boot_r-display_name = Right Boot
equip_slot-ring_l-display_name = Left Ring
equip_slot-ring_r-display_name = Right Ring
equip_slot-unknown-display_name = Unknown Slot
