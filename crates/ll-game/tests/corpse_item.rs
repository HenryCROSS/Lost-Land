//! 端到端验收：**尸体是一件真正的、可堆叠的、有物种名字的物品。**
//!
//! 所有者实机试玩后的裁定原话：
//!
//! > 「尸体也是一件可堆叠的物品才对」
//!
//! 他在交互列表里看到的是 `#103 x1（搜刮）`——`append_corpse_drop` 把
//! **种族**索引塞进了 [`ll_world::item::ItemStack::def`]（那是**物品**
//! 索引的位置），`ItemTable` 因此查不到任何字段，
//! `ll_ui::hud::item_display_name` 退化成 `#<索引>`。完整论证见
//! `ll_mod::corpse_item` 模块文档。
//!
//! 本文件全程走**真实 `mods/` 内容**与真实 `assets/locales/*.ftl`，
//! 不用任何夹具编的表（ADR 0018）。
//!
//! # 这几条断言真的会红吗
//!
//! 逐条记在各自的测试文档里，见本批次提交信息。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_game::content::LoadedContent;
use ll_sim::item::ItemCatalog;
use ll_ui::hud::item_display_name;

/// 本文件按名字引用的那几条内容 id。
const GOBLIN: &str = "lostland:goblin";
const HUMAN: &str = "lostland:human";
/// 示例 mod 的种族——**`crates/` 下零提及**，是「第三方 mod 加一个种族
/// 就自动有尸体」这条能力的活证据（规格 §10.3、ADR 0018）。
const EXAMPLE_MOD_RACE: &str = "examplemod:half_elf";

/// 测试用内容装载——写法与 `culture_hostility.rs` 的同名帮手一致。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ll-game-corpse-item-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

fn index_of(content: &LoadedContent, raw: &str) -> ContentIndex {
    content
        .registry
        .get(&NamespacedId::parse(raw).expect("测试用标识符恒合法"))
        .unwrap_or_else(|| panic!("内容必须注册过 {raw}"))
}

/// 真实的 i18n 目录——从仓库的 `assets/locales/` 装，不是夹具。
fn locales() -> ll_i18n::Catalog {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/locales");
    ll_i18n::Catalog::load_one(ll_game::content::BASE_NAMESPACE, &root)
}

#[test]
fn 尸体在物品表里查得到而不再是裸种族索引() {
    // **所有者实机看到的那一幕的直接反例。** 此前 `ItemStack.def` 是
    // 种族索引，`ItemTable::is_defined(种族索引)` 恒假。
    //
    // 故意改坏的反例（人工核验）：把
    // `ll_mod::load_session::LoadSession::load_all` 末尾那行
    // `register_corpse_items(..)` 删掉，本条当场变红。
    // Arrange
    let content = test_content();
    let goblin = index_of(&content, GOBLIN);

    // Act
    let corpse = ItemCatalog::corpse_of(&content.item_table, goblin);

    // Assert
    let corpse = corpse.expect("哥布林必须有一件尸体物品");
    assert!(
        content.item_table.is_defined(corpse),
        "尸体必须是一条真正的物品定义"
    );
    assert!(
        !content.item_table.is_defined(goblin),
        "种族索引本身仍然不是物品——这正是此前那次类型混淆的形状"
    );
}

#[test]
fn 尸体的显示名带物种而不是井号加数字() {
    // 所有者看到的 `#103 x1（搜刮）` 走的正是 `item_display_name` 的
    // `None => format!("#{}", def.get())` 那一支。
    //
    // 故意改坏的反例（人工核验）：把 `ll_ui::hud::item_display_name` 里
    // 那两行 `if let Some(species_key) = items.corpse_species_name_key(def)`
    // 删掉，名字退化成通用的「{ $species }的尸体」（物种插不进去），
    // 本条当场变红。
    // Arrange
    let content = test_content();
    let catalog = locales();
    let goblin_corpse = ItemCatalog::corpse_of(&content.item_table, index_of(&content, GOBLIN))
        .expect("哥布林尸体已注册");
    let human_corpse = ItemCatalog::corpse_of(&content.item_table, index_of(&content, HUMAN))
        .expect("人类尸体已注册");

    // Act
    let goblin_name = item_display_name(goblin_corpse, &content.item_table, &catalog, "zh-CN", &[]);
    let human_name = item_display_name(human_corpse, &content.item_table, &catalog, "zh-CN", &[]);

    // Assert
    assert!(
        !goblin_name.starts_with('#'),
        "尸体不该再退化成 #<索引>，实测「{goblin_name}」"
    );
    assert_eq!(goblin_name, "哥布林的尸体");
    assert_eq!(human_name, "人类的尸体");
    assert_ne!(
        goblin_name, human_name,
        "所有者要求名字能区分物种，不是笼统的「尸体」"
    );
}

#[test]
fn 尸体的英文名同样带物种() {
    // 首发中英双语（规格 §11.3）。两种语言各查一次，钉住的是
    // 「`item-corpse-display_name` 两份 `.ftl` 里都补齐了」——只补一份
    // 的话另一份会退化成键名本身。
    //
    // 故意改坏的反例（人工核验）：把 `assets/locales/en.ftl` 里
    // `item-corpse-display_name` 那一行删掉，本条当场变红。
    // Arrange
    let content = test_content();
    let catalog = locales();
    let corpse = ItemCatalog::corpse_of(&content.item_table, index_of(&content, GOBLIN))
        .expect("哥布林尸体已注册");

    // Act
    let name = item_display_name(corpse, &content.item_table, &catalog, "en", &[]);

    // Assert
    assert_eq!(name, "Goblin Corpse");
}

#[test]
fn 第三方mod加的种族自动获得尸体物品() {
    // **「本体即 Mod」检验**（规格 §10.3、ADR 0018）：`crates/` 下零
    // 提及 `examplemod:half_elf`，它照样有自己的尸体，且尸体名照样带
    // 物种——尸体名走「一条通用消息 + 物种名参数」而不是每族一条 Fluent
    // 键，全部理由就在这一条断言上，见 `ll_mod::corpse_item` 模块文档。
    //
    // 故意改坏的反例（人工核验）：把
    // `ll_mod::corpse_item::register_corpse_items` 里的遍历过滤改成
    // 「只处理 `lostland` 命名空间的种族」，本条当场变红而本体那几条
    // 照常绿——这正是这条检验存在的理由。
    // Arrange
    let content = test_content();
    let race = index_of(&content, EXAMPLE_MOD_RACE);

    // Act
    let corpse = ItemCatalog::corpse_of(&content.item_table, race);

    // Assert
    let corpse = corpse.expect("第三方 mod 的种族必须自动获得尸体物品");
    assert!(content.item_table.is_defined(corpse));
    let id = content
        .registry
        .resolve(corpse)
        .expect("注册过的索引必反查得到");
    assert_eq!(
        id.namespace(),
        "examplemod",
        "第三方种族的尸体必须归它自己的命名空间，否则会被算进本体的内容哈希"
    );
    assert!(
        content.item_table.corpse_species_name_key(corpse).is_some(),
        "第三方种族的尸体同样带物种名键，名字不退化成笼统的「尸体」"
    );
}

#[test]
fn 本体每个种族都有尸体且各不相同() {
    // 与 `npc_appearance.rs` 的种族清单同一个手法：清单从**注册表现查**，
    // 因此 `races.json5` 加第五个种族的那一刻，本条自动开始管它。
    //
    // 故意改坏的反例（人工核验）：把 `register_corpse_items` 的遍历
    // 改成只处理第一个种族，本条当场变红。
    // Arrange
    let content = test_content();
    let races: Vec<ContentIndex> = content
        .registry
        .snapshot()
        .into_iter()
        .filter_map(|id| {
            let index = content.registry.get(&id)?;
            content.race_table.is_defined(index).then_some(index)
        })
        .collect();
    assert!(races.len() >= 9, "本体至少九个种族");

    // Act
    let corpses: Vec<ContentIndex> = races
        .iter()
        .map(|race| {
            ItemCatalog::corpse_of(&content.item_table, *race)
                .expect("每个已定义属性的种族都必须有尸体物品")
        })
        .collect();

    // Assert：两两不同——一具矮人尸体不该与一具哥布林尸体是同一件物品。
    let unique: std::collections::BTreeSet<_> = corpses.iter().collect();
    assert_eq!(unique.len(), corpses.len(), "每个物种一件独立的尸体物品");
}

#[test]
fn 尸体可堆叠且有真实重量() {
    // 所有者原话：「尸体也是一件**可堆叠**的物品才对」。
    //
    // 同时钉住「不再是查不到任何字段」：`base_weight` 非零证明这件东西
    // 真的有一条 `ItemDef`，而不是又一个静默落空的查询。
    //
    // 故意改坏的反例（人工核验）：把
    // `ll_mod::corpse_item::CORPSE_STACK_LIMIT` 改成 1，本条当场变红。
    // Arrange
    let content = test_content();
    let corpse = ItemCatalog::corpse_of(&content.item_table, index_of(&content, GOBLIN))
        .expect("哥布林尸体已注册");

    // Act
    let view = content.item_table.get(corpse).expect("尸体已定义");

    // Assert
    assert!(
        view.stack_limit > 1,
        "尸体必须可堆叠，实测 stack_limit = {}",
        view.stack_limit
    );
    assert!(
        view.base_weight.0 > 0,
        "尸体必须有真实重量，实测 {}",
        view.base_weight.0
    );
    assert_eq!(view.max_durability, None, "尸体没有耐久概念");
}
