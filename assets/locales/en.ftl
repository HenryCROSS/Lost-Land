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

# Base-game items (mods/lostland/items.json5, thirty entries) —
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

# Base-game recipes (mods/lostland/crafting.json5, ten entries) —
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
