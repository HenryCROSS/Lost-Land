//! 端到端验证：本体二十七件物品、九条配方、五个配方类别真的由
//! `mods/lostland/items.json5` 与 `mods/lostland/crafting.json5` 注册，
//! 且**逐字段**与那两个文件的声明一致。
//!
//! # 这份测试为什么必须存在
//!
//! 理由逐字同 `base_mod_races.rs` 模块文档：内容搬进数据文件之后，
//! 「铁短剑的耐久是多少」这件事在 Rust 侧一行代码都不剩，`ll_mod::item`
//! 的单元测试只验得了 `ItemTable` 这套机制，验不了内容本身。没有本
//! 文件，把铁匠锤的耐久从 200 改成 2 不会让任何一条测试变红——内容值
//! 哈希会变，但那是一个「变了」的信号，不是一个「应该是多少」的断言。
//!
//! # 本文件多做一件 `base_mod_races.rs` 不做的事：清单不多不少
//!
//! 两条计数断言（[`本体物品的id清单不多不少`]、[`本体配方与类别的id清单不多不少`]）
//! 逐字列出全部 id 并要求**集合完全相等**。逐字段断言只能钉住「已经
//! 写下来的那几条是对的」，钉不住「有人偷偷加了第二十五件物品」——
//! 而多一件物品会静默改掉内容值哈希、改掉存档兼容性判定，是一个必须
//! 有人看见的变化。
//!
//! # 为什么物品与配方合住一个文件
//!
//! 装载帮手 `load_real_mods` 是六十行样板，而配方的食材与成品全部指向
//! 物品表——两组断言共享同一次装载、同一张注册表，拆成两个文件要把那
//! 六十行逐字复制一遍（既有的 `base_mod_*.rs` 已经各复制了一份，本
//! 文件不再多添一份）。

use std::collections::BTreeSet;
use std::path::Path;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::scaled::Milli;
use ll_mod::item::{ItemTable, ItemView};
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::recipe::{RecipeTable, RecipeView};
use ll_mod::recipe_category::RecipeCategoryTable;
use ll_mod::registry::Registry;
use ll_sim::item::{EquipSlot, SlotMask, StatBonus, StatTarget};
use ll_sim::skill::{ResourceKind, SkillEffect};

use ll_world::item::WearChannels;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `base_mod_races.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 一次真实装载留下的、本文件用得到的那几张表。
struct Loaded {
    registry: Registry,
    item: ItemTable,
    recipe: RecipeTable,
    recipe_category: RecipeCategoryTable,
}

/// 装载真实 `mods/` 目录一次，理由（为什么不只装 `mods/lostland/`）同
/// `base_mod_races.rs`：真实装载路径就是整目录装载。
fn load_real_mods() -> Loaded {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        item,
        recipe,
        recipe_category,
        ..
    } = session;

    let lostland_id = NamespacedId::parse("lostland:self").expect("合法标识符");
    let status = report
        .entries
        .iter()
        .find(|(id, _)| *id == lostland_id)
        .map(|(_, status)| status);
    assert_eq!(
        status,
        Some(&LoadStatus::Loaded),
        "本体内容 mod（mods/lostland/）必须成功加载，否则下面的断言毫无意义"
    );

    Loaded {
        registry,
        item,
        recipe,
        recipe_category,
    }
}

impl Loaded {
    /// 按 id 字符串取一个已注册的内容索引。
    fn index(&self, id: &str) -> ContentIndex {
        let parsed = NamespacedId::parse(id).unwrap_or_else(|_| panic!("{id} 是合法标识符"));
        self.registry
            .get(&parsed)
            .unwrap_or_else(|| panic!("{id} 应当已注册"))
    }

    fn item_view(&self, id: &str) -> ItemView<'_> {
        self.item
            .get(self.index(id))
            .unwrap_or_else(|| panic!("{id} 应当登记在物品表里"))
    }

    fn recipe_view(&self, id: &str) -> RecipeView<'_> {
        self.recipe
            .get(self.index(id))
            .unwrap_or_else(|| panic!("{id} 应当登记在配方表里"))
    }

    /// 本命名空间下、真的登记在某张表里的全部 id 字符串。
    fn ids_in<F: Fn(ContentIndex) -> bool>(
        &self,
        namespace: &str,
        is_defined: F,
    ) -> BTreeSet<String> {
        self.registry
            .snapshot()
            .into_iter()
            .filter(|id| id.namespace() == namespace)
            .filter_map(|id| self.registry.get(&id).map(|index| (id, index)))
            .filter(|(_, index)| is_defined(*index))
            .map(|(id, _)| id.to_string())
            .collect()
    }

    /// 把一组 id 字符串解析成索引集合，用来比对「标签/食材」这类
    /// `Vec<ContentIndex>` 字段。
    fn indices(&self, ids: &[&str]) -> Vec<ContentIndex> {
        ids.iter().map(|id| self.index(id)).collect()
    }
}

/// 把若干槽位名并成一个掩码，让下面的断言写得像内容文件里那一行。
fn slots(names: &[&str]) -> SlotMask {
    names.iter().fold(SlotMask::EMPTY, |mask, name| {
        mask.union(
            EquipSlot::from_name(name)
                .unwrap_or_else(|| panic!("{name} 是合法槽位名"))
                .mask(),
        )
    })
}

// ─────────────────────────── 清单不多不少 ───────────────────────────

#[test]
fn 本体物品的id清单不多不少() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let actual = loaded.ids_in("lostland", |index| loaded.item.is_defined(index));

    // Assert：逐字列全，集合相等。多一条少一条都在这里变红。
    let expected: BTreeSet<String> = [
        "lostland:amber_pendant",
        "lostland:bone_needle",
        "lostland:field_cookbook",
        "lostland:forge_apron",
        "lostland:forge_brand",
        "lostland:fur_mantle",
        "lostland:fur_pelt",
        "lostland:herb_bundle",
        "lostland:herbal_draught",
        "lostland:iron_greaves",
        "lostland:iron_helm",
        "lostland:iron_ingot",
        "lostland:iron_rivet",
        "lostland:iron_shortsword",
        "lostland:iron_warpick",
        "lostland:leather_boots",
        "lostland:leather_jerkin",
        "lostland:leather_strip",
        "lostland:linen_cloth",
        "lostland:linen_shirt",
        "lostland:oak_buckler",
        "lostland:raw_meat",
        "lostland:roast_meat",
        "lostland:sealed_relic_box",
        "lostland:smith_hammer",
        "lostland:tarnished_signet",
        "lostland:traveler_ring",
        "lostland:unmarked_phial",
        "lostland:wool_gloves",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 29);
}

#[test]
fn 本体配方与类别的id清单不多不少() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let recipes = loaded.ids_in("lostland", |index| loaded.recipe.is_defined(index));
    let categories = loaded.ids_in("lostland", |index| loaded.recipe_category.is_defined(index));

    // Assert
    let expected_recipes: BTreeSet<String> = [
        "lostland:fur_mantle_recipe",
        "lostland:herb_roast_recipe",
        "lostland:herbal_draught_recipe",
        "lostland:iron_greaves_recipe",
        "lostland:iron_helm_recipe",
        "lostland:iron_rivet_batch",
        "lostland:iron_shortsword_recipe",
        "lostland:linen_shirt_recipe",
        "lostland:roast_meat_recipe",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(recipes, expected_recipes);
    assert_eq!(recipes.len(), 9);

    let expected_categories: BTreeSet<String> = [
        "lostland:advanced_forging",
        "lostland:alchemy",
        "lostland:cooking",
        "lostland:forging",
        "lostland:tailoring",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(categories, expected_categories);
    assert_eq!(categories.len(), 5);
}

// ─────────────────────────── 物品逐字段 ───────────────────────────

#[test]
fn 铁短剑逐字段与内容文件的声明一致() {
    // 本体唯一一件引用了伤害公式的物品，钉住的正是「物品级公式覆盖」
    // 这条能力在本体侧的第一个用例。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.item_view("lostland:iron_shortsword");

    // Assert
    assert_eq!(
        view.display_name_key.to_string(),
        "lostland:item.iron_shortsword.display_name"
    );
    assert_eq!(view.stack_limit, 1);
    assert_eq!(view.base_weight, Milli(2400));
    assert_eq!(view.base_price, Milli(40000));
    assert_eq!(view.max_durability, Some(120));
    assert_eq!(view.equip_mask, slots(&["main-hand", "off-hand"]));
    assert_eq!(
        view.damage_formula,
        Some(loaded.index("lostland:blade_damage_formula")),
        "引用的必须是内容文件里那条真正不同于全局默认的公式，\
         不是引擎注册的 lostland:default_damage_formula（那条与不写等价）"
    );
    assert_eq!(view.damage_category, None);
    assert_eq!(view.tags, loaded.indices(&["lostland:weapon"]).as_slice());
    assert_eq!(view.wear_channels, WearChannels::ON_USE);
    assert!(view.stat_bonuses.is_empty());
    assert!(view.rule_modifiers.is_empty());
}

#[test]
fn 战镐同时带武器与工具两条标签且是本体唯一一处穿透声明() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.item_view("lostland:iron_warpick");

    // Assert
    assert_eq!(view.max_durability, Some(110));
    assert_eq!(view.equip_mask, slots(&["main-hand"]));
    assert_eq!(view.penetration.flat, 2);
    assert_eq!(view.penetration.permille, 150);
    assert_eq!(
        view.tags,
        loaded
            .indices(&["lostland:weapon", "lostland:tool"])
            .as_slice()
    );
    // 两条标签今天都是 on-use，因此并起来仍是单通道——这条断言正是
    // tags.json5 里「位掩码上并起来一模一样」那句话的可执行版本。
    assert_eq!(view.wear_channels, WearChannels::ON_USE);
}

#[test]
fn 橡木圆盾同时走挨打与使用两条磨损通道() {
    // 本体唯一一件双通道物品：armor → on-hit，weapon → on-use。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.item_view("lostland:oak_buckler");

    // Assert
    assert_eq!(view.max_durability, Some(90));
    assert_eq!(view.equip_mask, slots(&["off-hand"]));
    assert_eq!(
        view.stat_bonuses,
        [StatBonus {
            target: StatTarget::Armor,
            amount: 7,
        }]
    );
    assert_eq!(
        view.tags,
        loaded
            .indices(&["lostland:armor", "lostland:weapon"])
            .as_slice()
    );
    assert_eq!(
        view.wear_channels,
        WearChannels::ON_HIT.union(WearChannels::ON_USE)
    );
    // 通用减伤走 armor 不走 resistances，理由见 items.json5 与
    // content_audit 里 ItemAttrs::rule_modifiers 那条豁免。
    assert!(view.rule_modifiers.is_empty());
}

#[test]
fn 铁匠锤是纯工具且耐久最高() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.item_view("lostland:smith_hammer");

    // Assert
    assert_eq!(view.max_durability, Some(200));
    assert_eq!(view.equip_mask, slots(&["main-hand"]));
    assert_eq!(view.tags, loaded.indices(&["lostland:tool"]).as_slice());
    assert_eq!(view.wear_channels, WearChannels::ON_USE);
}

#[test]
fn 骨针占副手而不是主手() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.item_view("lostland:bone_needle");

    // Assert
    assert_eq!(view.max_durability, Some(60));
    assert_eq!(view.equip_mask, slots(&["hand-r"]));
    assert_eq!(view.tags, loaded.indices(&["lostland:tool"]).as_slice());
}

#[test]
fn 皮甲衣同时给护甲与保温两项加成() {
    // 「同一件东西同时是护甲又是保暖衣物」的证据——两个加成目标共用
    // 同一段求和算法。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.item_view("lostland:leather_jerkin");

    // Assert
    assert_eq!(view.max_durability, Some(110));
    assert_eq!(view.equip_mask, slots(&["body"]));
    assert_eq!(
        view.stat_bonuses,
        [
            StatBonus {
                target: StatTarget::Armor,
                amount: 6,
            },
            StatBonus {
                target: StatTarget::Insulation,
                amount: 30,
            },
        ]
    );
    assert_eq!(view.tags, loaded.indices(&["lostland:armor"]).as_slice());
    assert_eq!(view.wear_channels, WearChannels::ON_HIT);
}

#[test]
fn 毛皮披风是本体保温值最高的衣物且占外层槽() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let mantle = loaded.item_view("lostland:fur_mantle");
    let shirt = loaded.item_view("lostland:linen_shirt");

    // Assert：披风占 outer、衬衣占 body，因此两件可以同时穿，保温相加。
    assert_eq!(mantle.equip_mask, slots(&["outer"]));
    assert_eq!(
        mantle.stat_bonuses,
        [StatBonus {
            target: StatTarget::Insulation,
            amount: 80,
        }]
    );
    assert_eq!(mantle.max_durability, Some(90));
    assert_eq!(shirt.equip_mask, slots(&["body"]));
    assert_eq!(
        shirt.stat_bonuses,
        [StatBonus {
            target: StatTarget::Insulation,
            amount: 20,
        }]
    );
    assert!(
        !mantle.equip_mask.intersects(shirt.equip_mask),
        "两件保暖衣物必须占不同槽位，否则「两层比一层暖」这条设计不成立"
    );
}

#[test]
fn 皮靴与羊毛手套各自占左右一对槽位() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let boots = loaded.item_view("lostland:leather_boots");
    let gloves = loaded.item_view("lostland:wool_gloves");

    // Assert
    assert_eq!(boots.equip_mask, slots(&["boot-l", "boot-r"]));
    assert_eq!(gloves.equip_mask, slots(&["hand-l", "hand-r"]));
    assert_eq!(boots.max_durability, Some(80));
    assert_eq!(gloves.max_durability, Some(40));
}

#[test]
fn 饰品可装备但刻意不带耐久也不带标签() {
    // 「可装备 ≠ 会磨损」——首饰没有任何磨损通道，给它耐久等于声明
    // 一个永远不变的数字。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let pendant = loaded.item_view("lostland:amber_pendant");
    let ring = loaded.item_view("lostland:traveler_ring");

    // Assert
    assert_eq!(pendant.equip_mask, slots(&["neck"]));
    assert_eq!(pendant.max_durability, None);
    assert!(pendant.tags.is_empty());
    assert_eq!(pendant.wear_channels, WearChannels::NONE);
    assert_eq!(
        pendant.stat_bonuses,
        [StatBonus {
            target: StatTarget::Attribute(ll_world::entity::AttributeKind::Willpower),
            amount: 2,
        }]
    );

    assert_eq!(ring.equip_mask, slots(&["ring-l", "ring-r"]));
    assert_eq!(ring.max_durability, None);
    assert!(ring.tags.is_empty());
    assert_eq!(
        ring.stat_bonuses,
        [StatBonus {
            target: StatTarget::Attribute(ll_world::entity::AttributeKind::Luck),
            amount: 1,
        }]
    );
}

#[test]
fn 材料与消耗品可堆叠且一律不带耐久() {
    // 注册期硬校验（可堆叠 + 有耐久 = 拒绝）在内容侧的对应事实。
    // Arrange
    let loaded = load_real_mods();

    // Act & Assert
    for (id, stack_limit) in [
        ("lostland:iron_ingot", 50),
        ("lostland:iron_rivet", 99),
        ("lostland:linen_cloth", 50),
        ("lostland:leather_strip", 50),
        ("lostland:fur_pelt", 20),
        ("lostland:herb_bundle", 20),
        ("lostland:raw_meat", 20),
        ("lostland:roast_meat", 20),
        ("lostland:herbal_draught", 10),
    ] {
        let view = loaded.item_view(id);
        assert_eq!(view.stack_limit, stack_limit, "{id} 的堆叠上限");
        assert_eq!(view.max_durability, None, "{id} 不该带耐久");
        assert_eq!(view.equip_mask, SlotMask::EMPTY, "{id} 不该可装备");
    }
}

#[test]
fn 两件消耗品各自恢复一种资源() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let roast = loaded.item_view("lostland:roast_meat");
    let draught = loaded.item_view("lostland:herbal_draught");

    // Assert
    assert_eq!(
        roast.use_effect,
        Some(SkillEffect::RestoreResource {
            resource: ResourceKind::Stamina,
            base: 25,
        })
    );
    assert_eq!(
        draught.use_effect,
        Some(SkillEffect::RestoreResource {
            resource: ResourceKind::Mana,
            base: 30,
        })
    );
}

#[test]
fn 野外食谱教的是真的存在的那条烤肉配方() {
    // 「这本书静默什么都不教」是配方发现批次点名最难查的一类内容缺陷。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let book = loaded.item_view("lostland:field_cookbook");

    // Assert
    assert_eq!(book.max_durability, None);
    assert_eq!(
        book.taught_recipes,
        loaded.indices(&["lostland:roast_meat_recipe"]).as_slice()
    );
    assert!(
        loaded
            .recipe
            .is_defined(loaded.index("lostland:roast_meat_recipe")),
        "taught_recipes 指向的必须真的登记在配方表里，不只是「这个 id 被 intern 过」"
    );
}

// ─────────────────────────── 配方逐字段 ───────────────────────────

#[test]
fn 本体全部九条配方都必须先被发现() {
    // 项目所有者裁定「一开始什么都不会，只能乱煮」在本体内容侧唯一的
    // 落点。**这条断言比逐条写九次更值钱**：它对将来新增的配方同样
    // 生效——有人加一条不写 requires_discovery 的本体配方，本条立刻红。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let ids = loaded.ids_in("lostland", |index| loaded.recipe.is_defined(index));

    // Assert
    assert_eq!(ids.len(), 9);
    for id in &ids {
        assert!(
            loaded.recipe_view(id).requires_discovery,
            "{id} 必须声明 requires_discovery——本体不存在天生就会的配方"
        );
    }
}

#[test]
fn 烤肉是无场地无工具无闸门的基础配方() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.recipe_view("lostland:roast_meat_recipe");
    let category = loaded
        .recipe_category
        .get(view.category)
        .expect("烹饪类别已注册");

    // Assert
    assert_eq!(
        view.display_name_key.to_string(),
        "lostland:recipe.roast_meat.display_name"
    );
    assert_eq!(view.category, loaded.index("lostland:cooking"));
    assert_eq!(view.ingredients.len(), 1);
    assert_eq!(view.ingredients[0].item, loaded.index("lostland:raw_meat"));
    assert_eq!(view.ingredients[0].count, 1);
    assert_eq!(view.product, loaded.index("lostland:roast_meat"));
    assert_eq!(view.product_count, 1);
    assert_eq!(view.required_station, None);
    assert_eq!(view.required_tool, None);
    assert!(
        category.required_subclasses.is_empty(),
        "烹饪类别不设副职闸门——乱煮这条路径只能发生在没有闸门的类别里"
    );
}

#[test]
fn 香草烤肉是同一件成品的第二条配方() {
    // RecipeTable 刻意不对「成品」设唯一性约束，本条是本体侧的证据。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let plain = loaded.recipe_view("lostland:roast_meat_recipe");
    let herbed = loaded.recipe_view("lostland:herb_roast_recipe");

    // Assert
    assert_eq!(plain.product, herbed.product);
    assert_eq!(herbed.product, loaded.index("lostland:roast_meat"));
    assert_eq!(herbed.ingredients.len(), 2);
    assert_eq!(
        herbed.ingredients[0].item,
        loaded.index("lostland:raw_meat")
    );
    assert_eq!(herbed.ingredients[0].count, 1);
    assert_eq!(
        herbed.ingredients[1].item,
        loaded.index("lostland:herb_bundle")
    );
    assert_eq!(herbed.ingredients[1].count, 1);
}

#[test]
fn 打铆钉一次出八颗且要工具不要场地() {
    // product_count > 1 与 required_tool 两条覆盖的落点。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.recipe_view("lostland:iron_rivet_batch");

    // Assert
    assert_eq!(view.category, loaded.index("lostland:forging"));
    assert_eq!(view.product, loaded.index("lostland:iron_rivet"));
    assert_eq!(view.product_count, 8);
    assert_eq!(view.required_station, None);
    assert_eq!(
        view.required_tool,
        Some(loaded.index("lostland:smith_hammer"))
    );
}

#[test]
fn 打铁短剑既要场地也要工具() {
    // required_station 覆盖的落点。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.recipe_view("lostland:iron_shortsword_recipe");

    // Assert
    assert_eq!(view.category, loaded.index("lostland:forging"));
    assert_eq!(
        view.required_station,
        Some(loaded.index("lostland:floor_stone")),
        "场地指向的是本体十七个地形之一，不是为当场地凭空造出来的地形"
    );
    assert_eq!(
        view.required_tool,
        Some(loaded.index("lostland:smith_hammer"))
    );
    assert_eq!(view.ingredients.len(), 2);
    assert_eq!(view.product, loaded.index("lostland:iron_shortsword"));
    assert_eq!(view.product_count, 1);
}

#[test]
fn 裁缝两条配方都要骨针() {
    // Arrange
    let loaded = load_real_mods();

    // Act & Assert
    for id in ["lostland:linen_shirt_recipe", "lostland:fur_mantle_recipe"] {
        let view = loaded.recipe_view(id);
        assert_eq!(view.category, loaded.index("lostland:tailoring"), "{id}");
        assert_eq!(
            view.required_tool,
            Some(loaded.index("lostland:bone_needle")),
            "{id}"
        );
        assert_eq!(view.required_station, None, "{id}");
    }
}

#[test]
fn 炼金唯一那条配方两束草药出一瓶汤剂() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let view = loaded.recipe_view("lostland:herbal_draught_recipe");

    // Assert
    assert_eq!(view.category, loaded.index("lostland:alchemy"));
    assert_eq!(view.ingredients.len(), 1);
    assert_eq!(
        view.ingredients[0].item,
        loaded.index("lostland:herb_bundle")
    );
    assert_eq!(view.ingredients[0].count, 2);
    assert_eq!(view.product, loaded.index("lostland:herbal_draught"));
    assert_eq!(view.product_count, 1);
}

// ────────────────────── 类别闸门与「不成环」 ──────────────────────

#[test]
fn 只有进阶锻造设了副职闸门而它依赖的副职从不设闸的类别练出来() {
    // 「设闸门」唯一正确的形状：
    //   forging（人人可做）→ 做满 20 件 → artisan → advanced_forging
    // 反过来把闸门设在 forging 自己身上就是死锁，装载期
    // `content_audit::detect_unlock_deadlocks` 会硬失败。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let gate = |id: &str| -> Vec<ContentIndex> {
        loaded
            .recipe_category
            .get(loaded.index(id))
            .unwrap_or_else(|| panic!("{id} 应当登记在配方类别表里"))
            .required_subclasses
            .clone()
    };

    // Assert
    assert_eq!(
        gate("lostland:advanced_forging"),
        loaded.indices(&["lostland:artisan"])
    );
    for id in [
        "lostland:forging",
        "lostland:tailoring",
        "lostland:alchemy",
        "lostland:cooking",
    ] {
        assert!(
            gate(id).is_empty(),
            "{id} 必须不设闸门：四个制作副职正是分别从这四个类别里练出来的，\
             设了闸门就成了「要当工匠才能锻造，要锻造才能当工匠」"
        );
    }
}

#[test]
fn 进阶锻造那两条配方都落在设了闸门的类别里() {
    // Arrange
    let loaded = load_real_mods();
    let advanced = loaded.index("lostland:advanced_forging");

    // Act & Assert
    for id in ["lostland:iron_helm_recipe", "lostland:iron_greaves_recipe"] {
        let view = loaded.recipe_view(id);
        assert_eq!(view.category, advanced, "{id}");
        assert_eq!(
            view.required_station,
            Some(loaded.index("lostland:floor_stone")),
            "{id}"
        );
        assert_eq!(
            view.required_tool,
            Some(loaded.index("lostland:smith_hammer")),
            "{id}"
        );
    }
}
