# English strings for the example mod. See zh-CN.ftl for why this
# directory exists and how the ids are formed (namespace comes from the
# directory, dots in the key path become hyphens).

## Classes
necromancer_display_name = Necromancer
rogue_display_name = Rogue

## Recipe categories and recipes
recipe_category_cooking_display_name = Cooking (example)
recipe_category_forging_display_name = Forging (example)
roast_meat_recipe_display_name = Roast Meat
iron_sword_recipe_display_name = Forge Iron Sword
iron_sword_from_scrap_display_name = Reforge Iron Sword from Scrap
arrow_batch_recipe_display_name = Arrows (batch)
herb_stew_recipe_display_name = Herb Stew

## Damage categories
damage_category_acid_display_name = Acid

## Items
arrow_display_name = Arrow
iron_sword_display_name = Iron Sword
war_hammer_display_name = War Hammer
wooden_shield_display_name = Wooden Shield
healing_potion_display_name = Healing Potion
crude_dagger_display_name = Crude Dagger
flame_longbow_display_name = Flame Longbow
acid_dagger_display_name = Acid Dagger
acid_ward_amulet_display_name = Acid Ward Amulet
wool_liner_display_name = Wool Liner
fur_cloak_display_name = Fur Cloak
raw_meat_display_name = Raw Meat
roast_meat_display_name = Roast Meat
iron_ingot_display_name = Iron Ingot
wild_herb_display_name = Wild Herb
herb_stew_display_name = Herb Stew
cookbook_display_name = Cookbook
portable_anvil_display_name = Portable Anvil

## Races
half_elf_display_name = Half-Elf
goblin_display_name = Goblin (example)
dragonborn_display_name = Dragonborn
elf_display_name = Elf (example)
gnome_display_name = Gnome
ooze_display_name = Ooze
footpad_display_name = Footpad

## Resource pools
sorcery_points_display_name = Sorcery Points
wizard_spell_slots_display_name = Wizard Spell Slots
druid_slots_display_name = Druid Spell Slots

## Subclasses
shadowdancer_display_name = Shadowdancer

## Traits
draconic_breath_display_name = Draconic Breath
innate_sorcery_display_name = Innate Sorcery
arcane_casting_display_name = Arcane Casting
druidic_casting_display_name = Druidic Casting
acid_hide_display_name = Acid Hide
predatory_instinct_display_name = Predatory Instinct
shadow_dance_display_name = Shadow Dance
cutpurse_training_display_name = Cutpurse Training

## Weather
weather-ashfall-display_name = Ashfall

## Key-collision regression fixture: ids deliberately identical to the base game's.
# See the zh-CN.ftl comment for the full reasoning.
race-elf-display_name = Elf of the example mod
hud-inventory-empty = the example mod must not hijack this base-game string

## Dialogue (dialogue content-table batch)
#
# The structure lives in this mod's own dialogues.json5; the words live here -
# the base game's assets/locales/ needs no change at all. This is the living
# evidence that a third-party mod can write its own dialogue.
#
# Note `dialogue-common-farewell` and `dialogue-common-back`: they collapse to
# the SAME Fluent id as the base game's entries but carry different text. Another
# deliberate key-collision fixture, same shape as the two at the end of this
# file. See crates/ll-game/tests/dialogue_content.rs.

dialogue-common-farewell = (sweep out without another word)
dialogue-common-back = (there was more to say)

dialogue-necromancer-root = The necromancer does not look up. "The living come here for one of two reasons. Which are you?"
dialogue-necromancer-offer_potion = (hold out the healing potion)
dialogue-necromancer-potion = He snorts. "You mean to buy me with that? Keep it. You need it more than I do."
dialogue-necromancer-ask_craft = What are you making?
dialogue-necromancer-craft = "A draught." He finally raises his eyes. "The materials are hard to find, the heat harder. If you want to help, stand further back."
dialogue-necromancer-ask_more = What kind of draught?
dialogue-necromancer-more = "The kind that makes things talk when they should not." He lowers his head again. "Ask no more."
