//! 把 `register-recipe`/`recipe-requires-station!`/`recipe-requires-tool!`
//! 注册进脚本引擎：落地 `knowledge/design/crafting-system.md` 十节③⑤⑥。
//!
//! # 食材为什么是两条平行列表，不是设计文档写的 `(recipe-ingredient …)`
//!
//! 设计文档十节④给出的形状是一个构造子
//! `(recipe-ingredient item-id count)`，配方声明里写成
//! `(list (recipe-ingredient "a" 2) (recipe-ingredient "b" 1))`。
//! **本模块没有照做，改成两条平行列表**（`ingredient-item-ids` 与
//! `ingredient-counts`），理由是那个形状需要一套本代码库**明确记录为
//! 尚未约定**的 FFI 编码：[`crate::script_trait_api`] 模块文档原文写着
//! 「`stat_modifiers`/`rule_modifiers`/`granted_resource_pools` 三类效果
//! ……脚本层尚未为『列表套元组』/『打标签的构造子』这两种更复杂的 FFI
//! 编码约定过怎么做」，[`crate::trait_def`] 也因同一条理由把
//! `capacity-kind` 的 `"by-level"` 档推迟。**在一个新批次里顺手发明那
//! 套约定，代价远大于本批次的收益**——两条 `Vec<String>`/`Vec<i64>`
//! 是 `steel-core` 的 `Vec<T>: FromSteelVal` 已经逐元素支持、且
//! `register-trait`（`granted-skills: Vec<String>`）与
//! `register-trait-resource-pool-by-level`（`tier-amounts: Vec<i64>`）
//! 两处**已发货**的既有手法。
//!
//! 代价是一条注册期校验：两条列表长度必须相等，不等即拒绝
//! （ADR 0017）。这条校验是真实的、当场报错的，不是靠约定。
//!
//! # ADR 0020 核对
//!
//! 三个函数的参数只有字符串（标识符）与 `i64`（数量），**没有任何
//! 浮点**。乙区（流进世界状态）的量只有食材数量与产出数量，全是整数，
//! 不需要 `Milli` 量化。

use std::cell::RefCell;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::recipe::{RecipeAttrs, RecipeError, RecipeIngredient, RecipeTable};
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-recipe` 应该写入的配方表。
    static ACTIVE_TABLE: RefCell<Option<RecipeTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内可写入的目标。
pub fn set_active_target(table: RecipeTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 [`RecipeTable`]。
pub fn take_active_target() -> RecipeTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把三个配方注册函数注册进 `engine`。
/// 借出当前活跃的配方表做**只读**查询——`crate::script_item_api` 的
/// `register-item-teaches-recipe` 需要校验「这个 id 真的是一条配方」，
/// 而配方表归本模块所有。手法与所有权论证同
/// `crate::script_tag_api::with_active_tag_table`（`register-item-tag`
/// 校验标签 id 时的同一处需要）：两张表各自的 `ACTIVE_TABLE` 是两个
/// 独立的 `RefCell`，同一线程上一个 `borrow_mut()`、一个 `borrow()`
/// 不冲突；两者都由 `crate::pipeline::compile_one_script` 在同一个窗口
/// 里成对 `set`/`take`，生命周期完全对齐。
pub(crate) fn with_active_recipe_table<R>(f: impl FnOnce(Option<&RecipeTable>) -> R) -> R {
    ACTIVE_TABLE.with(|cell| f(cell.borrow().as_ref()))
}

/// 把配方相关的四个注册函数注册进 `engine`。
pub fn register_recipe_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-recipe", register_recipe);
    engine.register_fn("recipe-requires-station!", recipe_requires_station);
    engine.register_fn("recipe-requires-tool!", recipe_requires_tool);
    engine.register_fn("recipe-requires-discovery!", recipe_requires_discovery);
}

/// `(recipe-requires-discovery! id)`——声明这条配方**必须先被发现**才
/// 做得出来（配方发现批次），落地项目所有者的裁定「菜谱就是通过随机
/// 丢入东西煮获取或者阅读书籍的时候获取」。
///
/// - `id`：已经通过 `register-recipe` 注册过的完整命名空间标识符字符串。
///
/// 形状照 [`recipe_requires_station`]/[`recipe_requires_tool`] 这条既有
/// 先例（`register-recipe` 的七参数签名不能改参数个数，会破坏仓库里已有
/// 的真实 mod 脚本）——差别只有一个：本函数**只有一个参数**，因为它声明
/// 的是一个布尔事实，没有「要求什么」这个宾语。
///
/// # 为什么没有配套的「取消发现要求」
///
/// 注册期声明是**加法**，不是一台可以来回拨的开关——与
/// `recipe-requires-station!` 同样没有 `recipe-clears-station!` 是同一条
/// 形状。「不需要发现」就是不调用本函数。
///
/// # 顺序要求：`register-recipe` 必须先跑
///
/// 与 `recipe-requires-station!`/`recipe-requires-tool!` 逐字相同
/// （ADR 0017「注册期完整校验」）：目标配方尚未注册时整次调用失败，
/// 不静默创建一条只有这一个字段的半成品。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn recipe_requires_discovery(id: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("recipe-requires-discovery! 在没有活跃配方表的窗口内被调用".to_string());
            };
            do_recipe_requires_discovery(registry, table, &id)
        })
    })
}

/// [`recipe_requires_discovery`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_recipe_requires_discovery(
    registry: &Registry,
    table: &mut RecipeTable,
    id: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("配方 {id:?} 尚未通过 register-recipe 注册"));
    };
    table
        .set_requires_discovery(index)
        .map(|()| true)
        .map_err(|err: RecipeError| err.to_string())
}

/// `(register-recipe id display-name-key category-id ingredient-item-ids
/// ingredient-counts product-id product-count)`。
///
/// - `id`/`display-name-key`：完整命名空间标识符字符串。
/// - `category-id`：**必须已经通过 `register-recipe-category` 注册过**
///   （本函数只 `get` 不 `intern`）——这正是配方类别值得一张独立内容表
///   的直接理由：它拦得住 `"lostlan:cooking"` 这类拼写错误，而拼写错误
///   若不拦，症状是「这条配方永远不出现在任何分类里」，是最难查的一类
///   内容 bug。
/// - `ingredient-item-ids`/`ingredient-counts`：两条**等长**的平行列表，
///   见模块文档「食材为什么是两条平行列表」一节。物品标识符只
///   `intern`、**不**跨表校验它是不是一件已注册的物品（理由同
///   `register-trait` 的 `granted-skills`：跨表强校验会让注册顺序产生
///   不必要的耦合；装载完成后的跨表引用完整性由
///   [`crate::content_audit`] 统一兜住）。
/// - `product-id`：产出物品标识符，同样只 `intern` 不跨表校验。
/// - `product-count`：产出数量，恒 ≥ 1。
///
/// 两条可选前置（场地/工具）**不在本函数的位置参数里**，走
/// [`recipe_requires_station`]/[`recipe_requires_tool`]——照
/// `register-item-damage-category` 这条已落地的先例：两个可选参数塞进
/// 位置列表会逼每一条普通配方都传两个空串哨兵。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_recipe(
    id: String,
    display_name_key: String,
    category_id: String,
    ingredient_item_ids: Vec<String>,
    ingredient_counts: Vec<i64>,
    product_id: String,
    product_count: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-recipe 在没有活跃配方表的窗口内被调用".to_string());
            };
            do_register_recipe(
                registry,
                table,
                &id,
                &display_name_key,
                &category_id,
                &ingredient_item_ids,
                &ingredient_counts,
                &product_id,
                product_count,
            )
        })
    })
}

/// [`register_recipe`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_recipe(
    registry: &mut Registry,
    table: &mut RecipeTable,
    id: &str,
    display_name_key: &str,
    category_id: &str,
    ingredient_item_ids: &[String],
    ingredient_counts: &[i64],
    product_id: &str,
    product_count: i64,
) -> Result<bool, String> {
    if ingredient_item_ids.len() != ingredient_counts.len() {
        return Err(format!(
            "register-recipe 的食材标识符列表（{} 项）与数量列表（{} 项）长度不一致",
            ingredient_item_ids.len(),
            ingredient_counts.len()
        ));
    }
    let product_count = u32::try_from(product_count)
        .map_err(|_| format!("非法产出数量 {product_count}：必须是非负整数"))?;

    let parsed_category = NamespacedId::parse(category_id)
        .map_err(|err| format!("非法内容标识符 {category_id:?}：{err}"))?;
    let Some(category) = registry.get(&parsed_category) else {
        return Err(format!(
            "配方类别 {category_id:?} 尚未通过 register-recipe-category 注册"
        ));
    };

    let mut ingredients: Vec<RecipeIngredient> = Vec::with_capacity(ingredient_item_ids.len());
    for (raw, count) in ingredient_item_ids.iter().zip(ingredient_counts) {
        let parsed =
            NamespacedId::parse(raw).map_err(|err| format!("非法食材标识符 {raw:?}：{err}"))?;
        let count =
            u32::try_from(*count).map_err(|_| format!("非法食材数量 {count}：必须是非负整数"))?;
        ingredients.push(RecipeIngredient {
            item: registry.intern(parsed),
            count,
        });
    }

    let parsed_product = NamespacedId::parse(product_id)
        .map_err(|err| format!("非法成品标识符 {product_id:?}：{err}"))?;
    let product = registry.intern(parsed_product);

    let parsed_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    table
        .define(
            index,
            RecipeAttrs {
                display_name_key: parsed_key,
                category,
                ingredients,
                product,
                product_count,
            },
        )
        .map(|()| true)
        .map_err(|err: RecipeError| err.to_string())
}

/// `(recipe-requires-station! recipe-id station-id)`——声明这条配方必须
/// 站在某种地形上才能制作。
///
/// - `recipe-id`：已经通过 `register-recipe` 注册过的完整标识符字符串。
/// - `station-id`：地形标识符，只 `intern` 不跨表校验（理由同
///   [`register_recipe`] 的 `product-id`）。
///
/// **覆盖，不是追加**——一条配方只有一个场地。
fn recipe_requires_station(recipe_id: String, station_id: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("recipe-requires-station! 在没有活跃配方表的窗口内被调用".to_string());
            };
            let target = resolve_recipe_and_reference(registry, &recipe_id, &station_id, "场地")?;
            table
                .set_required_station(target.0, target.1)
                .map(|()| true)
                .map_err(|err: RecipeError| err.to_string())
        })
    })
}

/// `(recipe-requires-tool! recipe-id tool-id)`——声明这条配方必须装备着
/// 某件物品才能制作，语义与校验同 [`recipe_requires_station`]。
///
/// 「装备着**且耐久未归零**」这条判定在结算侧
/// （`ll_sim::resolve::resolve_craft`），不在注册期。
fn recipe_requires_tool(recipe_id: String, tool_id: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("recipe-requires-tool! 在没有活跃配方表的窗口内被调用".to_string());
            };
            let target = resolve_recipe_and_reference(registry, &recipe_id, &tool_id, "工具")?;
            table
                .set_required_tool(target.0, target.1)
                .map(|()| true)
                .map_err(|err: RecipeError| err.to_string())
        })
    })
}

/// [`recipe_requires_station`]/[`recipe_requires_tool`] 共用的前半段：
/// 解析两个标识符，要求配方本身已注册（`get`），被引用的目标只
/// `intern`。
///
/// 两处共用同一段而不是各写一遍：这里真的有一份可共享的算法（两次
/// 解析 + 一次存在性校验 + 一次 intern），不是「形状相似」——ADR 0021
/// 的判据成立。`what` 只进错误信息，不改变任何一步的行为。
fn resolve_recipe_and_reference(
    registry: &mut Registry,
    recipe_id: &str,
    reference_id: &str,
    what: &str,
) -> Result<(ContentIndex, ContentIndex), String> {
    let parsed_recipe = NamespacedId::parse(recipe_id)
        .map_err(|err| format!("非法内容标识符 {recipe_id:?}：{err}"))?;
    let Some(recipe) = registry.get(&parsed_recipe) else {
        return Err(format!("配方 {recipe_id:?} 尚未通过 register-recipe 注册"));
    };
    let parsed_reference = NamespacedId::parse(reference_id)
        .map_err(|err| format!("非法{what}标识符 {reference_id:?}：{err}"))?;
    Ok((recipe, registry.intern(parsed_reference)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe_category::RecipeCategoryTable;
    use crate::script_recipe_category_api::{
        register_recipe_category_api, set_active_target as set_active_category_target,
        take_active_target as take_active_category_target,
    };

    /// 建一个已经注册好 `yourmod:forging` 类别的脚本引擎与两张活跃表。
    fn engine_with_category() -> ScriptEngine {
        let mut engine = ScriptEngine::new();
        register_recipe_category_api(&mut engine);
        register_recipe_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_category_target(RecipeCategoryTable::new());
        set_active_target(RecipeTable::new());
        engine
            .load_source(
                r#"(register-recipe-category "yourmod:forging" "yourmod:forging_display_name")"#
                    .to_string(),
            )
            .expect("类别注册应当成功");
        engine
    }

    /// 收回两张活跃表与注册表，供失败路径的测试收尾。
    fn cleanup() {
        take_active_target();
        take_active_category_target();
        crate::active_registry::take_active_registry();
    }

    #[test]
    fn 合法配方声明注册成功并写入配方表() {
        // Arrange
        let mut engine = engine_with_category();

        // Act
        let result = engine.load_source(
            r#"(register-recipe "yourmod:iron_sword_recipe" "yourmod:iron_sword_recipe_display_name"
                                "yourmod:forging"
                                (list "yourmod:iron_ingot" "yourmod:leather_strip")
                                (list 2 1)
                                "yourmod:iron_sword" 1)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok(), "{result:?}");
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let _categories = take_active_category_target();
        let recipe = registry
            .get(&NamespacedId::parse("yourmod:iron_sword_recipe").unwrap())
            .expect("刚注册的配方应能查到索引");
        let ingot = registry
            .get(&NamespacedId::parse("yourmod:iron_ingot").unwrap())
            .expect("食材应当被 intern");
        let view = table.get(recipe).expect("已注册");
        assert_eq!(view.ingredients.len(), 2);
        assert_eq!(
            view.ingredients[0],
            RecipeIngredient {
                item: ingot,
                count: 2
            }
        );
        assert_eq!(view.product_count, 1);
    }

    #[test]
    fn 声明配方需要先被发现之后配方表真的记下了这条要求() {
        // 配方发现批次：`recipe-requires-discovery!` 经真实脚本引擎
        // 调用，写进配方表的 requires_discovery——没有这条，
        // resolve_craft 的第 4 道闸门在内容侧就永远没有输入。
        // Arrange
        let mut engine = engine_with_category();
        engine
            .load_source(
                r#"(register-recipe "yourmod:stew" "yourmod:stew_display_name"
                                    "yourmod:forging" (list "yourmod:meat") (list 1)
                                    "yourmod:stew_bowl" 1)"#
                    .to_string(),
            )
            .expect("配方注册应当成功");

        // Act
        let result =
            engine.load_source(r#"(recipe-requires-discovery! "yourmod:stew")"#.to_string());

        // Assert
        assert!(result.is_ok(), "{result:?}");
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let _categories = take_active_category_target();
        let stew = registry
            .get(&NamespacedId::parse("yourmod:stew").unwrap())
            .expect("刚注册的配方应能查到索引");
        assert!(table.get(stew).expect("已注册").requires_discovery);
    }

    #[test]
    fn 没调用过发现声明的配方默认不需要发现() {
        // 本批次对既有内容零影响这条兼容性承诺的直接验收：仓库里已有
        // 的每一条配方都没调用过 recipe-requires-discovery!，它们必须
        // 全部保持「人人天生会做」。
        // Arrange
        let mut engine = engine_with_category();

        // Act
        engine
            .load_source(
                r#"(register-recipe "yourmod:plain" "yourmod:plain_display_name"
                                    "yourmod:forging" (list "yourmod:meat") (list 1)
                                    "yourmod:plain_dish" 1)"#
                    .to_string(),
            )
            .expect("配方注册应当成功");

        // Assert
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let _categories = take_active_category_target();
        let plain = registry
            .get(&NamespacedId::parse("yourmod:plain").unwrap())
            .expect("刚注册的配方应能查到索引");
        assert!(!table.get(plain).expect("已注册").requires_discovery);
    }

    #[test]
    fn 给未注册的配方声明需要发现时失败而不panic() {
        // ADR 0017「注册期完整校验」：不静默创建一条只有这一个字段的
        // 半成品，与 recipe-requires-station! 对未注册配方的既有处理
        // 同一条纪律。
        // Arrange
        let mut engine = engine_with_category();

        // Act
        let result =
            engine.load_source(r#"(recipe-requires-discovery! "yourmod:ghost")"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup。
        cleanup();
    }

    #[test]
    fn 类别未注册时配方声明失败而不panic() {
        // 这一条正是「配方类别值得一张独立内容表」的直接验收：
        // 拼错的类别名当场被拒，而不是产出一条永远不出现在任何分类里
        // 的配方。
        // Arrange
        let mut engine = engine_with_category();

        // Act
        let result = engine.load_source(
            r#"(register-recipe "yourmod:oops" "yourmod:oops_display_name"
                                "yourmod:forgign" (list "yourmod:iron_ingot") (list 1)
                                "yourmod:iron_sword" 1)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        cleanup();
    }

    #[test]
    fn 两条食材列表长度不一致时被拒绝() {
        // Arrange
        let mut engine = engine_with_category();

        // Act
        let result = engine.load_source(
            r#"(register-recipe "yourmod:oops" "yourmod:oops_display_name"
                                "yourmod:forging" (list "yourmod:a" "yourmod:b") (list 1)
                                "yourmod:iron_sword" 1)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        cleanup();
    }

    #[test]
    fn 空食材列表被拒绝() {
        // Arrange
        let mut engine = engine_with_category();

        // Act
        let result = engine.load_source(
            r#"(register-recipe "yourmod:free_lunch" "yourmod:free_lunch_display_name"
                                "yourmod:forging" (list) (list)
                                "yourmod:iron_sword" 1)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        cleanup();
    }

    #[test]
    fn 两条可选前置经脚本写进配方表() {
        // Arrange
        let mut engine = engine_with_category();
        engine
            .load_source(
                r#"(register-recipe "yourmod:iron_sword_recipe" "yourmod:iron_sword_recipe_display_name"
                                    "yourmod:forging" (list "yourmod:iron_ingot") (list 2)
                                    "yourmod:iron_sword" 1)"#
                    .to_string(),
            )
            .expect("配方注册应当成功");

        // Act
        let station = engine.load_source(
            r#"(recipe-requires-station! "yourmod:iron_sword_recipe" "yourmod:forge_floor")"#
                .to_string(),
        );
        let tool = engine.load_source(
            r#"(recipe-requires-tool! "yourmod:iron_sword_recipe" "yourmod:smithing_hammer")"#
                .to_string(),
        );

        // Assert
        assert!(station.is_ok(), "{station:?}");
        assert!(tool.is_ok(), "{tool:?}");
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let _categories = take_active_category_target();
        let recipe = registry
            .get(&NamespacedId::parse("yourmod:iron_sword_recipe").unwrap())
            .expect("配方已注册");
        let view = table.get(recipe).expect("已注册");
        assert_eq!(
            view.required_station,
            registry.get(&NamespacedId::parse("yourmod:forge_floor").unwrap())
        );
        assert_eq!(
            view.required_tool,
            registry.get(&NamespacedId::parse("yourmod:smithing_hammer").unwrap())
        );
    }

    #[test]
    fn 给未注册的配方设置前置失败而不panic() {
        // Arrange
        let mut engine = engine_with_category();

        // Act
        let result = engine.load_source(
            r#"(recipe-requires-tool! "yourmod:never_defined" "yourmod:smithing_hammer")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        cleanup();
    }
}
