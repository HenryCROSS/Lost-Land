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
keybind-action-inventory = Inventory
keybind-action-craft = Craft
keybind-action-pick_up = Pick Up
keybind-action-drop = Drop
keybind-action-equip = Equip
keybind-action-use = Use
keybind-action-place = Place
keybind-action-interact = Interact

# Character gender (ll_world::entity::Gender::display_name_key). Not
# content: the key lives on the Rust enum, not in any content table.
gender-male-display_name = Male
gender-female-display_name = Female

race-human-display_name = Human
race-dwarf-display_name = Dwarf
race-elf-display_name = Elf
race-goblin-display_name = Goblin

class-warrior-display_name = Warrior
class-mage-display_name = Mage
class-ranger-display_name = Ranger
class-guard-display_name = Guard
class-steward-display_name = Steward
class-militia-display_name = Militia
class-farmer-display_name = Farmer
class-hunter-display_name = Hunter
class-butcher-display_name = Butcher
class-blacksmith-display_name = Blacksmith
class-fisher-display_name = Fisher
class-shepherd-display_name = Shepherd
class-mason-display_name = Mason

subclass-duelist-display_name = Duelist
subclass-apprentice-display_name = Apprentice

subclass-artisan-display_name = Artisan
subclass-tailor-display_name = Tailor
subclass-alchemist-display_name = Alchemist
subclass-cook-display_name = Cook
recipe_category-forging-display_name = Forging
recipe_category-advanced_forging-display_name = Advanced Forging
recipe_category-tailoring-display_name = Tailoring
recipe_category-alchemy-display_name = Alchemy
recipe_category-cooking-display_name = Cooking

# Base traits (mods/lostland/traits.json5, four crafting masteries) --
# ll-mod TraitAttrs::display_name_key.
trait-forging_mastery-display_name = Forging Mastery
trait-tailoring_mastery-display_name = Tailoring Mastery
trait-alchemy_mastery-display_name = Alchemy Mastery
trait-cooking_mastery-display_name = Cooking Mastery

# Corpses (ll-mod corpse_item: every race automatically gets a corpse item).
# The species half is interpolated via $species, taken from that race's own
# display_name_key — so a third-party mod that adds a race gets a working
# corpse name for free, with no extra key. See the ll_mod::corpse_item module
# docs for why this is one parameterised message rather than one key per race.
item-corpse-display_name = { $species } Corpse

# Base-game items (mods/lostland/items.json5, thirty-six entries) —
# ll-mod ItemAttrs::display_name_key.
item-iron_ingot-display_name = Iron Ingot
item-iron_rivet-display_name = Iron Rivet
item-linen_cloth-display_name = Linen Cloth
item-leather_strip-display_name = Leather Strip
item-fur_pelt-display_name = Fur Pelt
item-herb_bundle-display_name = Herb Bundle
item-raw_meat-display_name = Raw Meat
item-roast_meat-display_name = Roast Meat
item-herbal_draught-display_name = Herbal Draught
item-iron_shortsword-display_name = Iron Shortsword
item-iron_warpick-display_name = Iron Warpick
item-oak_buckler-display_name = Oak Buckler
item-forge_brand-display_name = Forge Brand
item-smith_hammer-display_name = Smith's Hammer
item-bone_needle-display_name = Bone Needle
item-iron_helm-display_name = Iron Helm
item-leather_jerkin-display_name = Leather Jerkin
item-iron_greaves-display_name = Iron Greaves
item-leather_boots-display_name = Leather Boots
item-linen_shirt-display_name = Linen Shirt
item-fur_mantle-display_name = Fur Mantle
item-forge_apron-display_name = Forge Apron
item-wool_gloves-display_name = Wool Gloves
item-amber_pendant-display_name = Amber Pendant
item-traveler_ring-display_name = Traveler's Ring
item-field_cookbook-display_name = Field Cookbook
item-tarnished_signet-display_name = Tarnished Signet
item-unmarked_phial-display_name = Unmarked Phial
item-sealed_relic_box-display_name = Sealed Relic Box
item-forge-display_name = Forge
# Furniture batch two — the six pieces that fill dwellings, workshops,
# warehouses and taverns. See mods/lostland/items.json5 for the table of
# which piece belongs in which building type.
item-oak_chair-display_name = Oak Chair
item-oak_table-display_name = Oak Long Table
item-fur_bed-display_name = Fur Bedding
item-oak_bookshelf-display_name = Oak Bookshelf
item-oak_barrel-display_name = Oak Barrel
item-iron_bound_chest-display_name = Iron-Bound Chest

# Base-game recipes (mods/lostland/crafting.json5, twelve entries) —
# ll-mod RecipeAttrs::display_name_key.
recipe-roast_meat-display_name = Roast Meat
recipe-herb_roast-display_name = Herb-Crusted Roast
recipe-herbal_draught-display_name = Herbal Draught
recipe-iron_rivet_batch-display_name = Batch of Iron Rivets
recipe-iron_shortsword-display_name = Forge Iron Shortsword
recipe-iron_helm-display_name = Forge Iron Helm
recipe-iron_greaves-display_name = Forge Iron Greaves
recipe-linen_shirt-display_name = Sew Linen Shirt
recipe-fur_mantle-display_name = Sew Fur Mantle
recipe-forge-display_name = Build a Forge
recipe-fur_bed-display_name = Lay Out Fur Bedding
recipe-iron_bound_chest-display_name = Forge Iron-Bound Chest

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

# Weather display names — the base game's six weathers
# (ll_world::weather::materialize_base_weathers). Weather is derived from
# (world seed, world clock) and never stored; these keys are only the
# display-name mapping, resolved via WeatherDef::display_name_key.
weather-clear-display_name = Clear
weather-overcast-display_name = Overcast
weather-rain-display_name = Rain
weather-wind-display_name = Wind
weather-fog-display_name = Fog
weather-snow-display_name = Snow

# Resource display names — the base game's seven resources
# (mods/lostland/resources.json5). Resource nodes are derived from
# (world seed, tile) and never stored; these keys are only the
# display-name mapping, resolved via ResourceAttrs::display_name_key.
# A settlement that dies of resource exhaustion names the resource here
# (ll_world::history::SettlementDemise::ResourceExhausted).
resource-farmland-display_name = Farmland
resource-pasture-display_name = Pasture
resource-timber-display_name = Timber
resource-iron_vein-display_name = Iron Vein
resource-granite-display_name = Granite
resource-fresh_water-display_name = Fresh Water
resource-fishery-display_name = Fishery

# Cultures (culture batch) — exactly one per settlement; see
# crates/ll-world/src/culture.rs and mods/lostland/cultures.json5.
culture-farmstead-display_name = Farmstead
culture-mining_hold-display_name = Mining Hold
culture-forest_kin-display_name = Forest Kin
culture-harbour-display_name = Harbour
culture-stonecutters-display_name = Stonecutters
culture-goblin_warband-display_name = Goblin Warband

hud-character-panel-title = Character
hud-character-level-label = Level
hud-character-experience-label = XP
hud-character-attribute-points-label = Attribute Points
hud-character-skill-points-label = Skill Points
hud-character-primary-attribute-label = Primary Attribute
hud-character-modifiers-title = Active Modifiers
hud-character-modifiers-empty = None

# Rule modifiers (ll_sim::rule_modifier) — the "Active Rule Modifiers"
# section of the character panel. See assets/locales/zh-CN.ftl for the
# source-field mapping and the argument list of every message below.
hud-character-rule-modifiers-title = Active Rule Modifiers
hud-character-rule-modifiers-empty = None
hud-character-rule-modifier-sources = { $sources ->
    [1] { "" }
   *[other] { " " }({ $sources } sources)
}

rule-modifier-resistance = { $subject } Resistance { $amount } dmg reduction
rule-modifier-vulnerability = { $subject } Vulnerability +{ $amount } dmg taken
rule-modifier-reroll_once = Reroll a roll of { $amount } once
rule-modifier-advantage = Advantage on { $subject }
rule-modifier-disadvantage = Disadvantage on { $subject }
rule-modifier-sneak_attack = Sneak Attack check +{ $amount }, damage +{ $extra }
rule-modifier-inspection_suspicion = Inconspicuous +{ $amount }
rule-modifier-inspection_concealment = Concealment +{ $amount }
rule-modifier-craft_yield = { $subject } yield +{ $amount }

damage_category-physical-display_name = Physical
damage_category-fire-display_name = Fire

check_context-inspection-display_name = Inspection
check_context-concealment-display_name = Concealment
check_context-critical-display_name = Critical

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
hud-item-unidentified = an unidentified item

hud-equipment-panel-title = Equipment
hud-equipment-empty-slot = (empty)

hud-inventory-menu-title = Inventory (up/down to select)
hud-inventory-menu-empty = Nothing here
hud-inventory-menu-hint = E equip/unequip  U use  X drop  P place  Esc close
hud-inventory-menu-equipped-label = worn
hud-craft-menu-title = Crafting (up/down to select)
hud-craft-menu-empty = No recipes
hud-craft-menu-hint = Enter craft  Esc close
hud-craft-station-label = Station
hud-craft-tool-label = Tool

hud-interact-menu-title = Underfoot (up/down to select)
hud-interact-menu-empty = Nothing here
hud-interact-menu-hint = Enter confirm  G take  Esc close
hud-interact-action-work = work here
hud-interact-action-loot = loot
hud-interact-action-take = take
# Doors (interact-list batch) — a door is TERRAIN, not an item, so its name
# does not come from the ItemTable. See ll_game::player_action's
# interact_target_name docs for why these are generic HUD strings.
hud-interact-action-open_door = open
hud-interact-action-close_door = close
hud-interact-door-closed = a closed door
hud-interact-door-open = an open door
hud-interact-direction-title = Interact with (up/down to select)
hud-interact-direction-prompt = Nothing nearby
hud-interact-direction-hint = Enter confirm  Esc close
hud-interact-direction-more = and more

hud-direction-here = Underfoot
hud-direction-north = North
hud-direction-north_east = Northeast
hud-direction-east = East
hud-direction-south_east = Southeast
hud-direction-south = South
hud-direction-south_west = Southwest
hud-direction-west = West
hud-direction-north_west = Northwest

hud-feedback-no-selection = Nothing to act on
hud-feedback-nothing-happened = That had no effect
hud-feedback-nothing-nearby = Nothing nearby to interact with

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

# Terrain shape presets (world generation parameters batch):
#   - worldgen-preset-*-display_name / -description
#     ll-content world_identity::TERRAIN_PRESETS (the four base presets)
# Percentages below are measured means over ten seeds at the "standard"
# world size (96x64 zones); see docs/worldgen-tuning.md for the raw data.
worldgen-preset-continent-display_name = Continent
worldgen-preset-continent-description = One unbroken landmass with ocean only at its edges. Roughly 37% water, very little mountain.
worldgen-preset-archipelago-display_name = Archipelago
worldgen-preset-archipelago-description = Hundreds of islands scattered across open ocean, none of them a continent. Roughly 73% water.
worldgen-preset-highland-display_name = Highland
worldgen-preset-highland-description = A land-dominated world of unbroken ranges. Roughly 24% mountain and only 25% water.
worldgen-preset-inland-display_name = Inland
worldgen-preset-inland-description = An inland world that barely sees the sea: roughly 16% water, farming and herding in place of fishing.

# In-game menu and settings screens (P7 wrap-up batch):
#   - screen-menu-* / screen-settings-*
#     ll-ui screen module; the rows themselves are laid out by
#     ll-game menu_screen, which passes already-formatted strings.
screen-menu-title = Menu
screen-menu-empty = (no entries)
screen-menu-hint = Up/Down to move, Confirm to choose, Esc to close
screen-menu-continue = Continue
screen-menu-settings = Settings
screen-menu-quit = Quit Game

screen-settings-title = Settings
screen-settings-empty = (no entries)
screen-settings-hint = Left/Right to change, Confirm to rebind, Esc to go back
screen-settings-capture-hint = Press the key to bind; Backspace clears it, Esc cancels
screen-settings-language = Language
screen-settings-vsync = Vertical Sync
screen-settings-scale-filter = Scale Filter
screen-settings-save = Save to config file
screen-settings-back = Back
screen-settings-keybinds-header = --- Key Bindings (gameplay) ---
screen-settings-on = On
screen-settings-off = Off
screen-settings-filter-nearest = Nearest
screen-settings-filter-sharp-bilinear = Sharp Bilinear
screen-settings-restart-required = (takes effect after restart)
screen-settings-unbound = (unbound)
screen-settings-capturing = ...press a key...
screen-settings-row = { $label }: { $value }
screen-settings-conflict = That key is already bound to { $action }
screen-settings-bound = Bound { $action }
screen-settings-cleared = Cleared the keys for { $action }
screen-settings-saved = Settings saved (hand-written comments in config.json5 are lost)
screen-settings-save-failed = Could not write the config file; the change is still active this session

# Endonym: every entry in the settings language picker is written in its
# own language. See ll_game::menu_screen::language_display_name.
language-name = English
# World map (the continent overview overlay toggled with M, ll-ui::hud::world_map)
# — zoom batch. hud-world-map-scale-label is immediately followed by
# "1:<tiles per cell>"; see scale_caption in crates/ll-ui/src/hud/world_map.rs.
hud-world-map-scale-label = Scale
hud-world-map-hint = Arrows pan, zoom keys zoom

# Title screen — the game's front page (batch 6):
#   - screen-title-*
#     Same ll-ui screen module as the in-game menu; the only difference is
#     that there is no world underneath it yet.
screen-title-title = Lost Land
screen-title-empty = (no entries)
screen-title-hint = Up/Down to move, Confirm to choose
screen-title-new-game = New Game
screen-title-load = Load Game
screen-title-load-empty = Load Game (no save file)
screen-title-settings = Settings
screen-title-quit = Quit
screen-title-no-save = There is no save file to load yet
screen-title-load-failed = The save file could not be read; nothing was changed

# Character creation / world setup / spawn picking (ll_game::chargen,
# world_setup, spawn_pick) - the three opening steps the project owner
# specified: pick race, gender and class; configure world history
# generation; then choose where on the map to be born.
screen-chargen-title = Create Character
screen-chargen-empty = (no entries)
screen-chargen-hint = Up/Down to move, Left/Right to change, Confirm to select, Esc to go back
screen-chargen-race = Race
screen-chargen-gender = Gender
screen-chargen-profession = Class
screen-chargen-next = Next: World Setup
screen-chargen-back = Back to Title

screen-worldsetup-title = World Setup
screen-worldsetup-hint = Up/Down to move, Left/Right to adjust, Confirm to generate, Esc to go back
screen-worldsetup-preset = Terrain Preset
screen-worldsetup-sea-level = Sea Level (per mille)
screen-worldsetup-mountain-level = Mountain Threshold (per mille)
screen-worldsetup-octaves = Noise Octaves
screen-worldsetup-continent-shrink = Continent Shrink
screen-worldsetup-climate-band-width = Climate Band Width (per mille)
screen-worldsetup-generate = Generate World
screen-worldsetup-back = Back to Character Creation
screen-worldsetup-invalid = That value is out of range; the change was discarded

screen-spawnpick-title = Choose Birthplace
screen-spawnpick-hint = Arrows or mouse to pick a zone, Confirm to be born there, Esc to go back
screen-spawnpick-no-land = No walkable land in that zone - pick somewhere else
