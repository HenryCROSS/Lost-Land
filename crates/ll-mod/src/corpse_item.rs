//! **每个种族自动获得一件真正的尸体物品**——引擎侧注册，内容作者不写
//! 一个字。
//!
//! # 这个模块修的是什么缺陷
//!
//! `ll_sim::resolve` 的 `append_corpse_drop` 此前把**种族索引**直接塞进
//! [`ll_world::item::ItemStack::def`]，而那个字段本该是**物品**索引：
//!
//! ```text
//! let corpse_def = victim.creature_kind.unwrap_or(victim.race);
//! stack: ItemStack::new(corpse_def, 1)   // ← 种族索引冒充物品索引
//! ```
//!
//! 后果不只是名字难看。所有者实机在交互列表里看到的 `#103 x1（搜刮）`
//! 只是最表层的症状（`ll_ui::hud::item_display_name` 查不到定义就退化成
//! `#<索引>`）；真正的问题是**凡是下游要查 `ItemDef` 的地方对尸体全部
//! 静默落空**：没有重量、没有堆叠上限、没有耐久、没有标签、查不到任何
//! 字段。当初那段代码的文档如实写了理由（`ll-sim` 不能依赖 `ll-mod`，
//! 拿不到 `Registry` 去 `intern`，而「区分哥布林尸体与人类尸体」当时
//! 没有真实消费场景），但所有者试玩后的裁定是——
//!
//! > 「尸体也是一件可堆叠的物品才对」
//!
//! ——于是那个场景现在有了。
//!
//! # 形状：注册一件物品，外加一条 `种族 → 尸体物品` 的映射
//!
//! 全部 mod 装载完之后（[`crate::load_session::LoadSession::load_all`]
//! 的末尾，与「无文化」哨兵同一处），遍历注册表里**每一个已定义属性的
//! 种族**，给它注册：
//!
//! | 东西 | 取值 | 例 |
//! |---|---|---|
//! | 物品 id | 种族 id 后缀 [`CORPSE_ID_SUFFIX`] | `lostland:goblin.corpse` |
//! | 显示名键 | [`CORPSE_DISPLAY_NAME_KEY`]（**全species共用一条**） | `lostland:item.corpse.display_name` |
//! | 物种名键 | 该种族自己的 `display_name_key` | `lostland:race.goblin.display_name` |
//!
//! 「物种名键」是本模块与 `ll-ui` 之间的接线：呈现层拿它把一条通用的
//! 「{ $species }的尸体」Fluent 消息插值成「哥布林的尸体」，见
//! [`ItemTable::corpse_species_name_key`](crate::item::ItemTable::corpse_species_name_key)
//! 与 `ll_ui::hud::item_display_name`。
//!
//! # 为什么名字走「一条通用消息 + 物种名参数」，不是每族一条 Fluent 键
//!
//! 因为**第三方 mod 加一个种族必须自动获得尸体物品**（规格 §10.3、
//! ADR 0018 的「本体即 Mod」检验）。若尸体名走 `item-goblin_corpse-
//! display_name` 这类**每族一条**的键，那么：
//!
//! - 本体的九个种族没问题（往 `assets/locales/*.ftl` 里补九条）；
//! - **第三方 mod 的种族拿不到键**——mod 的 `.ftl` 装载至今没有落地
//!   （`ll_i18n` 模块文档「五、mod 的 `.ftl`」一节留的是接口，不是实现），
//!   它的尸体名会退化成键名文本。等于「加个种族就自动有尸体」只对本体
//!   成立，而这恰恰是 ADR 0018 点名要防的那种「只在本体里成立的能力」。
//!
//! 走参数插值之后，尸体名的**唯一**新增翻译负担是一条通用消息
//! （`item-corpse-display_name`，en 与 zh-CN 各一条），物种那一半直接
//! 复用种族自己早就有的 `display_name_key`。第三方 mod 的种族只要它
//! 的种族名能显示，它的尸体名就能显示——不多欠任何一条。
//!
//! # 归并键**没有变**
//!
//! `victim.creature_kind.unwrap_or(victim.race)` 这条回退规则一个字没
//! 改（它是四条路径里三条的既有惯例，见
//! [`ll_sim::effect::Effect::IncrementKillCount`] 文档）。改的是「拿这
//! 个键去查什么」：此前直接当物品索引用，现在拿它查
//! [`ll_sim::item::ItemCatalog::corpse_of`]。
//!
//! `creature_kind` 指向的**不一定是种族**（那个字段是裸
//! [`ContentIndex`]，至今没有「生物种类表」）。查不到就查不到——
//! `append_corpse_drop` 那一侧按 ADR 0015 退回旧行为，见其文档。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::scaled::Milli;

use crate::item::{ItemAttrs, ItemTable};
use crate::race::RaceTable;
use crate::registry::Registry;
use ll_sim::combat::Penetration;
use ll_sim::item::SlotMask;

/// 尸体物品 id 在种族 id 后面追加的后缀——`lostland:goblin` →
/// `lostland:goblin.corpse`。
///
/// 留在种族**自己的命名空间**里（不是一律塞进 `lostland:`）：第三方
/// mod 的种族尸体应当归它自己，与该 mod 的其余内容一起被
/// `content_hash_of(命名空间)` 算进去。`.` 是
/// [`NamespacedId`] 路径段的合法字符（`ll_core::ident::is_valid_segment`）。
pub const CORPSE_ID_SUFFIX: &str = ".corpse";

/// 全部尸体**共用**的显示名键——物种那一半由
/// [`ItemTable::corpse_species_name_key`](crate::item::ItemTable::corpse_species_name_key)
/// 单独带，理由见模块文档「为什么名字走一条通用消息」。
///
/// 它落在 `lostland:` 命名空间：这是**引擎侧**注册的一条通用文案，与
/// `ll_i18n` 里其余 `hud-*` 键同一档，不属于任何一个种族的作者。
pub const CORPSE_DISPLAY_NAME_KEY: &str = "lostland:item.corpse.display_name";

/// 尸体的堆叠上限。
///
/// # 为什么是 8，不是 1、也不是 99
///
/// 所有者明确要求尸体「可堆叠」，因此下界是 `> 1`（`stack_limit == 1`
/// 在 [`ll_world::item::merge_stacks`] 里的行为含义正是「不可堆叠」，
/// 见该函数文档「为什么不用三条特判分支」一节）。
///
/// 上界取小，量级对齐本体 `items.json5` 里**又大又沉**的那一档：毛皮
/// 20、木盾之类的装备 1，而铁铆钉那种「小、多、论把数」的东西才 99。
/// 一具尸体比一整张毛皮还大，8 是这条量级线往下的自然落点——够表达
/// 「清完一窝哥布林，尸体归成一堆」，又不至于让一个人扛着九十九具尸体
/// 满地图跑。
///
/// **这是一个内容参数，没有数据支撑**，与 `items.json5` 文件头写明的
/// 那批取值同一性质。要调就改这一个常量。
pub const CORPSE_STACK_LIMIT: u32 = 8;

/// 尸体的基础重量（`Milli` 千分之一单位）。
///
/// 取 40000——本体最重的成品装备（铁匠锤一档）也就几千，一具完整的
/// 人形尸体应当明显重于任何单件装备。负重系统尚未接线
/// （[`crate::item::ItemDef::base_weight`] 文档），这个数今天没有任何
/// 消费者，但它必须是一个**诚实的量级**而不是 0：写 0 等于声明「尸体
/// 没有重量」，那是一句假话，且负重接线的那一天没人会想起来回头改它。
pub const CORPSE_BASE_WEIGHT: Milli = Milli(40_000);

/// 尸体的基础价格（`Milli` 千分之一单位）——0。
///
/// 与重量相反，这里 0 是**诚实**的：尸体不是商品，本作至今没有任何
/// 「卖尸体」的内容设计。真要做（猎人交货、赏金凭证）该给具体种族配
/// 具体价，不是在这里拍一个通用值。
pub const CORPSE_BASE_PRICE: Milli = Milli(0);

/// 给注册表里**每一个已定义属性的种族**注册一件尸体物品，返回
/// `(种族索引, 尸体物品索引)` 的清单（按种族的注册顺序）。
///
/// # 调用时机：全部 mod 装载完之后
///
/// 与 `crate::base_cultureless::register_base_cultureless_culture` 同一
/// 个理由、同一处调用点（[`crate::load_session::LoadSession::load_all`]
/// 末尾）：[`Registry::intern`] 按调用顺序分配索引，放在装载**之前**
/// 会把全部 mod 内容的索引整体后移。放在这里，新 `intern` 出来的尸体
/// 索引一律排在既有号段**后面**，不挤占任何已有内容。
///
/// 同时这也是唯一正确的时机：只有全部 mod 都装完了，「一共有哪些种族」
/// 才是确定的。
///
/// # 幂等
///
/// 对同一个会话调用两次，第二次每个种族的 `intern` 都拿到同一个索引，
/// 而 [`ItemTable::define`] 会返回
/// [`ItemError::DuplicateDefinition`](crate::item::ItemError::DuplicateDefinition)
/// ——本函数**跳过**已经定义过的索引，因此第二次调用是无操作，返回的
/// 清单与第一次逐位相同。理由同
/// `register_base_cultureless_culture`：`load_all` 允许被调用多次。
///
/// # 确定性（约束 C5）
///
/// 遍历走 [`Registry::snapshot`]（`Vec`，注册顺序），不经任何哈希容器
/// ——与 `ll_game::tests::npc_appearance` 的 `registered_races` 同一个
/// 写法。
pub fn register_corpse_items(
    registry: &mut Registry,
    races: &RaceTable,
    items: &mut ItemTable,
) -> Vec<(ContentIndex, ContentIndex)> {
    let display_name_key =
        NamespacedId::parse(CORPSE_DISPLAY_NAME_KEY).expect("固定字面量标识符恒合法");
    let race_ids: Vec<NamespacedId> = registry
        .snapshot()
        .into_iter()
        .filter(|id| {
            registry
                .get(id)
                .is_some_and(|index| races.is_defined(index))
        })
        .collect();

    let mut registered = Vec::with_capacity(race_ids.len());
    for race_id in race_ids {
        let Some(race_index) = registry.get(&race_id) else {
            continue;
        };
        let Some(corpse_id) = corpse_id_of(&race_id) else {
            continue;
        };
        let corpse_index = registry.intern(corpse_id);
        define_corpse_item(items, corpse_index, &display_name_key);
        // 物种名键取种族自己的显示名键——呈现层靠它把通用消息插值成
        // 「哥布林的尸体」，见模块文档。种族必然已定义（上面按
        // `is_defined` 过滤过），`get` 必返回 `Some`。
        let species_name_key = races
            .get(race_index)
            .expect("已按 is_defined 过滤，get 必返回 Some")
            .display_name_key
            .clone();
        items.set_corpse_link(race_index, corpse_index, species_name_key);
        registered.push((race_index, corpse_index));
    }
    registered
}

/// 一个种族 id 对应的尸体物品 id；种族 id 拼上后缀之后不再合法时返回
/// `None`（理论上不会发生——[`CORPSE_ID_SUFFIX`] 只含合法字符，而原
/// 路径已经合法——但这里不 `expect`：解析失败静默跳过一个种族，好过
/// 让整局游戏在装载期恐慌）。
fn corpse_id_of(race_id: &NamespacedId) -> Option<NamespacedId> {
    NamespacedId::parse(&format!(
        "{}:{}{CORPSE_ID_SUFFIX}",
        race_id.namespace(),
        race_id.path()
    ))
    .ok()
}

/// 把一条尸体物品定义写进表；已经定义过就跳过（幂等，见
/// [`register_corpse_items`] 文档）。
fn define_corpse_item(items: &mut ItemTable, index: ContentIndex, display_name_key: &NamespacedId) {
    if items.is_defined(index) {
        return;
    }
    // `define` 只会因「重复定义」失败，上面刚判过——这里忽略返回值会
    // 掩盖将来新增的失败原因，因此显式 `expect`。
    items
        .define(
            index,
            ItemAttrs {
                display_name_key: display_name_key.clone(),
                stack_limit: CORPSE_STACK_LIMIT,
                base_weight: CORPSE_BASE_WEIGHT,
                base_price: CORPSE_BASE_PRICE,
                // 尸体这件「容器」本身没有耐久概念——与
                // `ItemStack::new` 给材料/消耗品的既有语义一致，也与
                // `append_corpse_drop` 恒 `durability: None` 对得上。
                max_durability: None,
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
                // 不带任何标签：磨损通道由标签决定（`tags.json5`），
                // 而尸体没有耐久，给它标签只会造出一条没有含义的配对，
                // 见 `mods/lostland/items.json5` 文件头「耐久与标签怎么
                // 配对」一节。
                tags: Vec::new(),
                taught_recipes: Vec::new(),
                blind_box_pool: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                // 尸体是**躺**在地上的，不是被谁立起来的家具——与
                // `append_corpse_drop` 恒 `placed: false` 对得上。
                furniture: false,
            },
        )
        .expect("刚判过 is_defined，define 不会因重复定义失败");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::RaceAttrs;
    use ll_sim::item::ItemCatalog;
    use ll_world::entity::BaseStats;

    fn race_attrs(display_name_key: &str) -> RaceAttrs {
        RaceAttrs {
            display_name_key: NamespacedId::parse(display_name_key).expect("合法标识符"),
            stat_modifiers: BaseStats {
                strength: 0,
                dexterity: 0,
                constitution: 0,
                intelligence: 0,
                willpower: 0,
                charisma: 0,
                luck: 0,
            },
            darkvision_cells: 0,
            footprint: (1, 1),
            lifespan_years: 0,
            xp_reward: 0,
            traits: Vec::new(),
            starting_items: Vec::new(),
        }
    }

    /// 造一个「注册表 + 一张种族表」的最小夹具。
    fn fixture(races_ids: &[&str]) -> (Registry, RaceTable, ItemTable) {
        let mut registry = Registry::new();
        let mut races = RaceTable::new();
        for raw in races_ids {
            let index = registry.intern(NamespacedId::parse(raw).expect("合法标识符"));
            races
                .define(index, race_attrs(&format!("{raw}.display_name")))
                .expect("测试用种族声明合法");
        }
        (registry, races, ItemTable::new())
    }

    #[test]
    fn 每个种族都拿到一件已定义的尸体物品() {
        // Arrange
        let (mut registry, races, mut items) = fixture(&["test:goblin", "test:human"]);

        // Act
        let registered = register_corpse_items(&mut registry, &races, &mut items);

        // Assert
        assert_eq!(registered.len(), 2);
        for (_, corpse) in &registered {
            assert!(items.is_defined(*corpse), "尸体物品必须真的在物品表里");
        }
    }

    #[test]
    fn 第三方命名空间的种族尸体留在自己的命名空间里() {
        // 「本体即 Mod」检验（规格 §10.3、ADR 0018）：`crates/` 下没有
        // 一处提到 `examplemod:`，它的种族照样得到自己的尸体物品，且
        // 那件物品归它自己的命名空间——否则 `content_hash_of` 会把它
        // 算进本体。
        //
        // 故意改坏的反例（人工核验）：把 `corpse_id_of` 里的
        // `race_id.namespace()` 写死成 `"lostland"`，本条当场变红。
        // Arrange
        let (mut registry, races, mut items) = fixture(&["examplemod:half_elf"]);

        // Act
        let registered = register_corpse_items(&mut registry, &races, &mut items);

        // Assert
        let (_, corpse) = registered[0];
        let id = registry.resolve(corpse).expect("刚 intern 过，必反查得到");
        assert_eq!(id.namespace(), "examplemod");
        assert_eq!(id.path(), "half_elf.corpse");
    }

    #[test]
    fn 尸体可堆叠且堆叠上限大于一() {
        // 所有者原话：「尸体也是一件可堆叠的物品才对」。`stack_limit
        // == 1` 在 `ll_world::item::merge_stacks` 里的行为含义正是
        // 「不可堆叠」，因此这条钉的是 `> 1` 而不是某个具体数值。
        //
        // 故意改坏的反例（人工核验）：把 `CORPSE_STACK_LIMIT` 改成 1，
        // 本条当场变红。
        // Arrange
        let (mut registry, races, mut items) = fixture(&["test:goblin"]);

        // Act
        let registered = register_corpse_items(&mut registry, &races, &mut items);
        let rule = ItemCatalog::item(&items, registered[0].1).expect("尸体物品已定义");

        // Assert
        assert!(
            rule.stack_limit > 1,
            "尸体必须可堆叠，实测 stack_limit = {}",
            rule.stack_limit
        );
        assert_eq!(rule.stack_limit, CORPSE_STACK_LIMIT);
    }

    #[test]
    fn 尸体物品能从种族索引反查到() {
        // `append_corpse_drop` 就是靠这条查询把归并键翻译成物品索引。
        //
        // 故意改坏的反例（人工核验）：把 `register_corpse_items` 里那行
        // `items.set_corpse_link(..)` 删掉，本条当场变红。
        // Arrange
        let (mut registry, races, mut items) = fixture(&["test:goblin"]);

        // Act
        let registered = register_corpse_items(&mut registry, &races, &mut items);

        // Assert
        assert_eq!(
            ItemCatalog::corpse_of(&items, registered[0].0),
            Some(registered[0].1)
        );
    }

    #[test]
    fn 不是种族的索引查不到尸体() {
        // ADR 0015「查不到就是查不到」：`creature_kind` 是裸
        // `ContentIndex`，指向的不一定是种族。
        // Arrange
        let (mut registry, races, mut items) = fixture(&["test:goblin"]);
        let not_a_race = registry.intern(NamespacedId::parse("test:not_a_race").expect("合法"));

        // Act
        register_corpse_items(&mut registry, &races, &mut items);

        // Assert
        assert_eq!(ItemCatalog::corpse_of(&items, not_a_race), None);
    }

    #[test]
    fn 重复调用是无操作() {
        // `load_all` 允许被调用多次，理由同
        // `register_base_cultureless_culture`。
        // Arrange
        let (mut registry, races, mut items) = fixture(&["test:goblin", "test:human"]);

        // Act
        let first = register_corpse_items(&mut registry, &races, &mut items);
        let second = register_corpse_items(&mut registry, &races, &mut items);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 尸体带着自己物种的名字键() {
        // 呈现层靠它把「{ $species }的尸体」插值成「哥布林的尸体」。
        //
        // 故意改坏的反例（人工核验）：把 `set_corpse_link` 的第三个实参
        // 换成 `display_name_key.clone()`（通用键），本条当场变红。
        // Arrange
        let (mut registry, races, mut items) = fixture(&["test:goblin"]);

        // Act
        let registered = register_corpse_items(&mut registry, &races, &mut items);

        // Assert
        assert_eq!(
            items
                .corpse_species_name_key(registered[0].1)
                .map(NamespacedId::to_string),
            Some("test:goblin.display_name".to_string())
        );
    }
}
