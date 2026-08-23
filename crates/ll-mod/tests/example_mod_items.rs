//! 端到端验证：真实装载仓库里的 `mods/` 目录（不是临时夹具），证明
//! `register-item` 这个新脚本 API 真的能被 `mods/example_mod/gameplay.scm`
//! 调用，且注册出来的两种物品（可堆叠的箭矢、不可堆叠的铁剑）真的能
//! 走 `ll_sim::item::merge_stacks`/`split_stack` 端到端算出正确的堆叠
//! 结果——ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod
//! 脚本为证」，本文件是 P6 第一批（物品基础）的那份证据，不能靠
//! `crates/ll-mod/src/item.rs`/`crates/ll-mod/src/script_item_api.rs`/
//! `crates/ll-sim/src/item.rs` 里的单元测试自证。
//!
//! 与 `crates/ll-mod/tests/example_mod_resource_pools.rs` 同一个理由
//! 独立成文件、同一套「装载整个 `mods/` 目录，不是只挑 `example_mod`」
//! 手法，见该文件模块文档。物品本批次不接线 `resolve`（背包/装备留给
//! 后续批次，见 `ll_mod::item` 模块文档「本批次范围」一节），因此本
//! 文件不像 `example_mod_resource_pools.rs` 那样需要构造 `WorldState`/
//! `Agent`——断言直接落在「真实注册出来的 `ItemTable` 属性是否正确」
//! 与「拿这些真实属性喂给 `merge_stacks`/`split_stack` 是否算出设计
//! 文档要求的结果」两层。

use std::path::Path;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_mod::class::ClassTable;
use ll_mod::clip::ClipTable;
use ll_mod::item::ItemTable;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::{GameplayTables, load_all};
use ll_mod::quest::QuestTable;
use ll_mod::race::RaceTable;
use ll_mod::registry::Registry;
use ll_mod::resource_pool::ResourcePoolTable;
use ll_mod::skill::SkillTable;
use ll_mod::subclass::SubclassTable;
use ll_mod::trait_def::TraitTable;
use ll_mod::xp_curve::{XpCurveBindings, XpCurveTable};
use ll_sim::item::{ItemStack, merge_stacks, split_stack};

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_resource_pools.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回全部断言需要的表与已经解析好的
/// 索引——理由同 `example_mod_resource_pools.rs::RealModsHandle`。
struct RealModsHandle {
    item: ItemTable,
    arrow_id: ContentIndex,
    iron_sword_id: ContentIndex,
}

fn load_real_mods() -> RealModsHandle {
    let mut registry = Registry::new();
    let mut terrain = ll_world::terrain::TerrainTable::new();
    let mut class = ClassTable::new();
    let mut skill = SkillTable::new();
    let mut subclass = SubclassTable::new();
    let mut quest = QuestTable::new();
    let mut race = RaceTable::new();
    let mut clip = ClipTable::new();
    let mut xp_curve = XpCurveTable::new();
    let mut xp_curve_bindings = XpCurveBindings::new();
    let mut trait_def = TraitTable::new();
    let mut resource_pool = ResourcePoolTable::new();
    let mut item = ItemTable::new();
    let mut formula = ll_mod::formula::FormulaTable::new();
    let mut weapon_category = ll_mod::weapon_category::WeaponCategoryTable::new();
    let mut space_profile = ll_world::space_profile::SpaceProfileTable::new();
    let mut weather_table = ll_world::weather::WeatherTable::new();
    let mut recipe_table = ll_mod::recipe::RecipeTable::new();
    let mut recipe_category_table = ll_mod::recipe_category::RecipeCategoryTable::new();
    let mut tag_table = ll_mod::tag::TagTable::new();
    let mut damage_category = ll_mod::damage_category::DamageCategoryTable::new();
    let report = load_all(
        Path::new(REAL_MODS_ROOT),
        &mut registry,
        &mut GameplayTables {
            terrain: &mut terrain,
            class: &mut class,
            skill: &mut skill,
            subclass: &mut subclass,
            quest: &mut quest,
            race: &mut race,
            clip: &mut clip,
            xp_curve: &mut xp_curve,
            xp_curve_bindings: &mut xp_curve_bindings,
            trait_def: &mut trait_def,
            resource_pool: &mut resource_pool,
            item: &mut item,
            formula: &mut formula,
            weapon_category: &mut weapon_category,
            damage_category: &mut damage_category,
            space_profile: &mut space_profile,
            weather: &mut weather_table,
            recipe: &mut recipe_table,
            recipe_category: &mut recipe_category_table,
            tag: &mut tag_table,
            events: &mut ll_mod::event::EventSubscriptionTable::new(),
        },
    );
    let examplemod_id = NamespacedId::parse("examplemod:self").unwrap();
    let examplemod_status = report
        .entries
        .iter()
        .find(|(id, _)| *id == examplemod_id)
        .map(|(_, status)| status);
    assert_eq!(
        examplemod_status,
        Some(&LoadStatus::Loaded),
        "examplemod 必须成功加载，否则下面的索引解析毫无意义"
    );

    let resolve = |id: &str| {
        registry
            .get(&NamespacedId::parse(id).unwrap())
            .unwrap_or_else(|| panic!("{id} 应当已经被 mods/example_mod/gameplay.scm 注册"))
    };

    RealModsHandle {
        arrow_id: resolve("examplemod:arrow"),
        iron_sword_id: resolve("examplemod:iron_sword"),
        item,
    }
}

#[test]
fn 真实注册的箭矢携带九十九的堆叠上限且没有耐久概念() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .item
        .get(handle.arrow_id)
        .expect("箭矢应当已被真实注册");

    // Assert
    assert_eq!(view.stack_limit, 99);
    assert_eq!(view.max_durability, None);
}

#[test]
fn 真实注册的铁剑堆叠上限为一且携带一百点耐久上限() {
    // Arrange
    let handle = load_real_mods();

    // Act
    let view = handle
        .item
        .get(handle.iron_sword_id)
        .expect("铁剑应当已被真实注册");

    // Assert
    assert_eq!(view.stack_limit, 1);
    assert_eq!(view.max_durability, Some(100));
}

#[test]
fn 真实箭矢定义的两堆按注册的堆叠上限合并() {
    // 端到端验收：拿真实注册出来的 stack_limit（不是测试里手写的假
    // 数值）喂给 merge_stacks——证明 mod 脚本声明的堆叠上限真的能
    // 驱动堆叠算法，不是只停留在 ItemTable 能查到这一步。
    // Arrange
    let handle = load_real_mods();
    let stack_limit = handle.item.get(handle.arrow_id).unwrap().stack_limit;
    let a = ItemStack::new(handle.arrow_id, 60);
    let b = ItemStack::new(handle.arrow_id, 60);

    // Act
    let result = merge_stacks(a, b, stack_limit);

    // Assert：120 支箭超过 99 的上限，应当是一满一余。
    assert_eq!(
        result,
        Ok((
            ItemStack::new(handle.arrow_id, 99),
            Some(ItemStack::new(handle.arrow_id, 21))
        ))
    );
}

#[test]
fn 真实铁剑定义的两把合并后各自数量原样不变() {
    // 端到端验收：铁剑的真实 stack_limit（1）喂给 merge_stacks 后,
    // 两把剑应当"什么都没发生"——这正是"不可堆叠"这句设计要求在真实
    // 注册内容上的体现，不是测试里假造一个 stack_limit=1 的场景。
    // Arrange
    let handle = load_real_mods();
    let stack_limit = handle.item.get(handle.iron_sword_id).unwrap().stack_limit;
    let a = ItemStack::new(handle.iron_sword_id, 1);
    let b = ItemStack::new(handle.iron_sword_id, 1);

    // Act
    let result = merge_stacks(a, b, stack_limit);

    // Assert
    assert_eq!(
        result,
        Ok((
            ItemStack::new(handle.iron_sword_id, 1),
            Some(ItemStack::new(handle.iron_sword_id, 1))
        ))
    );
}

#[test]
fn 真实箭矢定义的一堆可以拆分出请求的数量() {
    // Arrange
    let handle = load_real_mods();
    let stack = ItemStack::new(handle.arrow_id, 40);

    // Act
    let result = split_stack(stack, 15);

    // Assert
    assert_eq!(
        result,
        Ok((
            ItemStack::new(handle.arrow_id, 15),
            ItemStack::new(handle.arrow_id, 25)
        ))
    );
}
