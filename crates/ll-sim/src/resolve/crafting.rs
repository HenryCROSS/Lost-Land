//! `resolve::crafting`：制作与知识获取：合成、阅读、试验、鉴定。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::ident::ContentIndex;
use ll_world::entity::{Agent, EntityId};
use ll_world::state::WorldState;

use crate::craft::{RecipeCatalog, RecipeRule};
use crate::effect::Effect;
use crate::item::{EquipSlot, ItemCatalog, ItemStack, WearChannels};
use crate::rule_modifier::{agent_rule_modifiers, craft_product_count, craft_yield_bonus};
use crate::timeline::action_cost;
use crate::traits::{TraitCatalog, TraitGrantSource};

use super::inventory::merge_into_inventory_effect;
use super::stats::effective_speed_from_dexterity;
use super::{BASE_ACTION_COST, TOOL_DURABILITY_LOSS_PER_CRAFT, schedule_after};

/// [`Intent::Craft`](crate::intent::Intent::Craft) 结算（制作系统批次，`knowledge/design/crafting-system.md`
/// 五节）：校验三道前置与食材是否齐全，齐全就逐条产出消耗效果、把成品
/// 并进背包，并按一次普通行动计费。
///
/// # 判定顺序本身是设计决定
///
/// ```text
/// 1. 查 agent，查不到 → 空
/// 2. 查配方，查不到 → 空                          （ADR 0015：未注册当作没有）
/// 3. 副职闸门：类别要求非空且与 agent.subclasses 无交集 → 空
/// 4. 已知闸门：配方声明 requires_discovery 且不在 known_recipes 里 → 空
/// 5. 场地前置：required_station 与脚下**摆着的那件家具**不符 → 空
/// 6. 工具前置：没有「def 匹配且耐久未归零」的已装备物品 → 空
/// 7. 食材校验：任意一条数量不够 → 空（不消耗任何食材）
/// 8. 逐条食材产出 Effect::ConsumeInventoryItem，重复 count 次
/// 9. 成品并进背包（Effect::MergeIntoInventory）
/// 10. 工具磨损（Effect::AdjustEquipmentDurability，工具带耐久时才产出）
/// ```
///
/// 四道前置（3/4/5/6）排在食材校验（7）之前，是因为**前四道回答的是
/// 「你能不能做这件事」，第五道回答的是「你现在够不够料」**。虽然本
/// 批次不设计任何 UI，判定顺序决定了将来制作界面能拿到的失败原因的
/// 优先级——玩家更需要先知道「我不会锻造」而不是「你缺两块铁锭」。
///
/// # 第 4 道闸门：配方发现（配方发现批次新增）
///
/// 项目所有者裁定「菜谱就是通过随机丢入东西煮获取或者阅读书籍的时候
/// 获取」，本步是那句话在制作侧的执行者。它**推翻了**
/// `food-and-cooking-system.md` 五节「菜谱全部已知、不设解锁门槛」那条
/// 裁定（更正记录写在该文档五节末尾，原文未删）。
///
/// 排在副职闸门**之后**、场地/工具**之前**，与那句「能不能 vs 够不够」
/// 的分界一致：「我不会这张图纸」和「我不是工匠」同属「你能不能做这件
/// 事」，而它比「我不是工匠」更具体，因此排后一位——玩家已经是工匠却
/// 做不出某条配方时，「你还没学会这张图纸」才是他真正需要的那条信息。
///
/// 声明 `requires_discovery == false` 的配方（既有全部内容的默认值）
/// 完全跳过本步，本函数对它们与本批次之前逐字节等价。两条把配方写进
/// `Agent::known_recipes` 的发现路径见 [`resolve_read`] 与
/// [`resolve_experiment`]。
///
/// # 场地是脚下**摆着的那件家具**（家具层批次）
///
/// 第 5 步此前问的是「脚下的**地形**是不是 `required_station`」，本体
/// 因此只能拿 `lostland:floor_stone` 将就当铁匠铺地面——`crafting.json5`
/// 里逐字写着那是「一个**明知的将就**：真正该当场地的是炉子或铁砧那样
/// 的家具」。家具层落地后这条将就没有存在理由了：
/// [`crate::craft::RecipeRule::required_station`] 现在指向一件
/// **可以被放置的物品**（`ItemDef.furniture`），判定是
/// [`ll_world::state::WorldState::placed_at`]——脚下那一格**立着**的那
/// 一堆是不是它。
///
/// 判定仍然是「**站在这格上**」，不是「站在旁边」——`crafting-system.md`
/// 六节那条理由（相邻判定会引入「多个相邻工作台算哪个」这类不必要的
/// 问题）一字未改，换掉的只是「这格上的什么东西回答这个问题」。
///
/// 判据从「这一格上有没有一件带 `furniture` 标志的物品」换成了「这一格
/// 上**立着**的那一件是不是它」（家具放置状态批次）——一件躺在脚下、
/// 没有被放置的炉子**当不了场地**，必须先 [`Intent::Place`](crate::intent::Intent::Place) 立起来。
/// 这正是所有者那条裁定在制作侧的直接后果：放置与否是两种不同的状态，
/// 只有立起来的那种才是「设施」。
///
/// 「把一堆铁锭丢在脚下就能开工」同样不成立：铁锭 `placed` 恒为假
/// （[`resolve_drop`](super::inventory::resolve_drop) 只产出躺着的堆），且它连 `furniture` 都没有，
/// [`resolve_place`](super::inventory::resolve_place) 第 ① 道前置就拦住了。指向普通物品的配方会变成永远
/// 做不出来——这是刻意的，见 `ll_mod::content_audit::inspect_recipe`
/// 里那段注释。
///
/// # 全程静默失败
///
/// 任何一步不满足都返回空 `Vec<Effect>`：不产出效果、不消耗食材、
/// 不推进时间轴——与 [`resolve_use_skill`](super::progression::resolve_use_skill) 资源不足时静默不产出效果、
/// [`resolve_drop`](super::inventory::resolve_drop)/[`resolve_equip`](super::equipment::resolve_equip) 查不到物品时静默无效是同一条既有
/// 纪律。
///
/// # 坏掉的工具不算装着
///
/// 工具判定的谓词是 `def == required_tool && durability != Some(0)`，
/// **不是只比 `def` 相等**。`item-system.md` 六节裁定「耐久归零 = 损坏
/// 不可用」，[`derive_stats`](super::stats::derive_stats) 遍历装备时已经对 `durability == Some(0)`
/// 的堆直接跳过（见其文档「耐久归零」一节）——工具前置若只比 `def`，
/// 会出现「锤子已经烂了但还能打铁」这个与既有耐久语义直接矛盾的漏洞。
///
/// # 工具磨损（耐久扩面批次）
///
/// 项目所有者原话：「修理锤子也算是一种武器，也可以是带有功能性的
/// 物品。**只要使用就会减少耐久**。」——制作正是「使用工具」这件事在
/// 本引擎里唯一已经落地的形态，本函数因此在第 9 步产出一条
/// [`crate::effect::Effect::AdjustEquipmentDurability`]，让被配方点名
/// 的那件工具损失 [`TOOL_DURABILITY_LOSS_PER_CRAFT`] 点耐久。
///
/// 这正是 `crafting-system.md` 九节⑩「工具因制作而磨损」——该表当时
/// 把它标为「与所有者『只有装备武器才有耐久』的裁定直接冲突」而推迟。
/// **那条裁定已被推翻**（见上面的原话与
/// [`resolve_attack`](super::combat::resolve_attack) 文档「耐久消耗：两条通道」一节），⑩ 的唯一阻碍
/// 因此消失，本批次把它落地。
///
/// ## 只在制作**真的发生**时磨损
///
/// 效果排在全部前置与食材校验之后——任何一步不满足时本函数早已
/// `return Vec::new()`，工具一点耐久都不掉。「白试一次也磨损」既不
/// 符合「只要使用就会减少耐久」这句话（没做成就不算用过），也会让
/// 「站错地方点了一下制作」这种纯操作失误产生真实损失。
///
/// ## 两个条件缺一不可：带耐久，且带 `on-use` 标签
///
/// 判据与 [`resolve_attack`](super::combat::resolve_attack) 的「使用」通道逐字相同：
/// `ItemStack.durability.is_some()` 回答「这一件还有多少耐久」，
/// [`crate::item::ItemRule::wear_channels`] 含
/// [`WearChannels::ON_USE`] 回答「这类东西用了会不会磨损」。内容作者
/// 因此可以声明一件永不磨损的工具——不给它填耐久上限（`-1`），或者
/// 给它挂一个不声明任何磨损通道的纯分类标签。「哪些物品该有耐久、
/// 该磨损」自此完全是内容决策，见
/// `ll_mod::script_item_api::register_item_equip_mask` 与
/// `ll_mod::script_tag_api::register_tag` 两处文档。
///
/// ## 归零之后制作**失败**
///
/// 由第 6 步的既有谓词 `durability != Some(0)` 保证，本节不新增任何
/// 判定：磨到零的锤子从此打不了铁，直到修理系统把它修回正数。这条
/// 与本节的磨损产出构成一个闭环——工具会用坏，用坏了就不能用，正是
/// 「耐久」这个词的全部含义。反例测试见
/// `ll-mod/tests/example_mod_crafting.rs`
/// 「耐久归零的工具装着也打不了铁」。
///
/// ## 为什么第 6 步改成 `find` 而不是 `any`
///
/// 产出效果需要工具的**存储键**（[`crate::effect::Effect::AdjustEquipmentDurability`]
/// 按槽位定位），`any` 只回答"有没有"、拿不到键。改成 `find` 之后
/// 判据一字未改，只是把找到的那一条留了下来。
///
/// # 成品的耐久（第 9 步）
///
/// 成品是**刚造出来的**，耐久等于它那条定义声明的上限——走
/// [`ItemStack::freshly_made`] 那条共同规则，与盲盒产出
/// （[`resolve_identify`]）用的是同一个构造器。没有耐久概念的成品
/// （烤肉、铁铆钉这类材料/消耗品）仍然是 `None`，因为它们的
/// `max_durability` 本来就是 `None`。
///
/// 这一行此前是 `ItemStack::new(rule.product, rule.product_count)`
/// ——恒 `None`：工匠打出来的铁短剑耐久是"没有耐久概念"而不是 120，
/// 从此永不磨损。那是一条真实缺陷，不是设计，完整论证见
/// [`ItemStack::freshly_made`] 文档。
///
/// **`product_count > 1` 且成品带耐久**是一个内容层面的病态组合（一堆
/// `count` 为 N 的装备共用一份耐久）。本函数**不新增**运行期分支拦它,
/// 理由是这条组合的病态与耐久无关：改动之前它同样产出一堆 `count` 为
/// N、`stack_limit` 却是 1 的装备（带耐久的物品必然 `stack_limit == 1`,
/// 注册期硬校验），只是那时耐久恰好是 `None`。本改动没有让它更坏,也
/// 没有资格在这里替内容作者做「一次只能造一件装备」这条裁定。本体九条
/// 配方里没有这种组合——唯一 `product_count > 1` 的 `iron_rivet_batch`
/// 产的是可堆叠、无耐久的铁铆钉。
///
/// # 产出加成接线（制作类副职奖励批次）
///
/// 第 9 步的件数不是配方声明的 `product_count`，而是它经
/// [`crate::rule_modifier::craft_yield_bonus`] 加成、再经
/// [`crate::rule_modifier::craft_product_count`] 保底之后的结果。这是
/// 四条制作类副职（工匠/裁缝/炼金术士/厨师）「拿到之后给什么」的唯一
/// 落点，完整设计见 `knowledge/design/crafting-subclass-rewards.md`。
///
/// 闭环因此成立：**做够 N 件锻造品 → 得到工匠副职**（第 3 步的闸门与
/// [`crate::subclass::craft_progress_effects`] 那条既有计数）**→ 此后
/// 每件锻造品多出一件**。挂钩的动作与被奖励的动作是同一个动作。
///
/// ## 加成来自哪四路
///
/// 本函数多出的四个 `&dyn` 参数就是为这一步取的，它们**不是新增依赖**
/// ——`resolve_dispatch` 的参数表里本来就有这一组（`resolve_attack`/
/// `resolve_inspect` 已经各接一份），本步只是把它们再往下传一层。
/// [`crate::rule_modifier::agent_rule_modifiers`] 把种族/职业/副职三路
/// 天赋与**装备**汇成一份候选，因此「大师级铁砧锤」这件装备携带同一条
/// 修正是白拿的。
///
/// ## 对不带这条天赋的行动者逐位不变
///
/// 一条也没命中时 `craft_yield_bonus` 返回 `0`，而
/// `craft_product_count(n, 0) == n`——本步对既有内容与既有存档的可观察
/// 结果一个字节都没变。
///
/// ## 产出恒 ≥ 1
///
/// [`crate::rule_modifier::MINIMUM_CRAFT_PRODUCT_COUNT`]。加成允许为负
/// （「手艺生疏」这类负面天赋，与抗性允许「脆弱」同一条先例），但
/// 「消耗了材料却什么都没拿到」在机制层面不可能发生——那正是本函数
/// 「全程静默失败」一节之外、`crafting-system.md` 九节⑤在玩法上否决过
/// 的「制作失败」。
///
/// # 约束核对
///
/// - C3（随机全部来自 `DetRng::for_entity`）：不涉及，本函数全程零
///   随机。产出加成接线**没有引入第一次掷骰**——`craft_yield_bonus`
///   是一次纯查表聚合，随机流的取数顺序完全不受影响。制作失败判定
///   标为将来扩展，见设计文档九节⑤。
/// - C5（逻辑决策不得依赖哈希表迭代顺序）：满足。第 6 步遍历的
///   `agent.equipment` 是 `BTreeMap`（有序），第 7/8 步遍历的
///   `recipe.ingredients`/`agent.inventory` 都是 `Vec`（保序）。
/// - C1/C2/C4：不涉及（不新增脚本状态跨帧持有、不进时间轴队列、
///   不改后台推进）。
///
/// # 已知边界（继承自 `food-and-cooking-system.md` 四节，如实重复）
///
/// 第 7 步只认**第一条** `def` 匹配的堆，不跨多堆合并计数：背包里两堆
/// 各 1 个铁锭时，需要 2 个铁锭的配方会判定为「料不够」。第 9 步若
/// `product_count` 大到需要三堆以上，[`Effect::MergeIntoInventory`] 的
/// `resulting` 目前的「最多两条」语义装不下。两条都只在数量远超
/// `stack_limit` 时才失真。
///
/// # 行动开销
///
/// 一次制作 = 一次普通行动，`action_cost(BASE_ACTION_COST, speed)`，
/// 与 [`resolve_wait`](super::upkeep::resolve_wait)/[`resolve_use_item`](super::equipment::resolve_use_item) 完全相同的计费，不新增任何
/// 常量。「打一把剑应该比切一块肉久」需要一套可中断的多回合活动机制，
/// 引擎目前没有，做成「时间轴直接前进 2000」是一个明显错误的中间态，
/// 见设计文档五节。
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_craft(
    world: &WorldState,
    actor: EntityId,
    recipe: ContentIndex,
    recipes: &dyn RecipeCatalog,
    items: &dyn ItemCatalog,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Vec<Effect> {
    // ① 行动者。
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // ② 配方。
    let Some(rule) = recipes.recipe(recipe) else {
        return Vec::new();
    };
    // ③ 副职闸门——any-of：空列表即人人可做。
    let required_subclasses = recipes.category_required_subclasses(rule.category);
    if !required_subclasses.is_empty()
        && !required_subclasses
            .iter()
            .any(|needed| agent.subclasses.contains(needed))
    {
        return Vec::new();
    }
    // ④ 已知闸门（配方发现批次）——只对声明了 requires_discovery 的
    // 配方生效，见本函数文档「第 4 道闸门」一节。默认 false 的既有配方
    // 一律直接通过，这一步对它们是零成本的一次 bool 判断。
    if rule.requires_discovery && !agent.known_recipes.contains(&recipe) {
        return Vec::new();
    }
    // ⑤ 场地前置——「站在这格上」，一次 `WorldState::placed_at`（家具
    // 放置状态批次把它从 furniture_at 换过来，见本函数文档「场地是脚下
    // **立着**的那一件」一节）。
    if let Some(station) = rule.required_station
        && world.placed_at(agent.pos).map(|ground| ground.stack.def) != Some(station)
    {
        return Vec::new();
    }
    // ⑥ 工具前置——装备着且耐久未归零，见本函数文档「坏掉的工具」一节。
    // 用 `find` 而不是 `any`：第 10 步磨损需要这件工具的存储键，判据本身
    // 一字未改，见本函数文档「为什么第 6 步改成 `find`」一节。
    // `equipment` 是 `BTreeMap`（有序），同一件工具被装在多个槽位这种
    // 情形下取哪一条是确定的（约束 C5）。
    let mut equipped_tool: Option<EquipSlot> = None;
    if let Some(tool) = rule.required_tool {
        let found = agent
            .equipment
            .iter()
            .find(|(_, stack)| stack.def == tool && stack.durability != Some(0));
        match found {
            None => return Vec::new(),
            // 第 10 步只对「带耐久」**且**「带 `on-use` 标签」的工具记下
            // 槽位——两个条件缺一不可，见本函数文档「工具磨损」一节。
            // 判据与 `resolve_attack` 的「使用」通道逐字相同：一件东西
            // 用了会不会磨损，由它带的标签回答，不由它是工具还是武器、
            // 挂在哪个槽位回答。
            Some((&slot, stack)) if stack.durability.is_some() => {
                if items
                    .item(stack.def)
                    .is_some_and(|tool_rule| tool_rule.wear_channels.contains(WearChannels::ON_USE))
                {
                    equipped_tool = Some(slot);
                }
            }
            Some(_) => {}
        }
    }
    // ⑦ 食材校验——全部齐全才继续，缺任意一条都不消耗任何东西。判定
    // 与 resolve_experiment 第③步共用同一段（见 has_all_ingredients
    // 文档：共享的不只是循环，还有「只认第一条匹配堆」那条已知边界）。
    if !has_all_ingredients(agent, &rule) {
        return Vec::new();
    }

    // ⑧ 逐条产出消耗效果。`Effect::ConsumeInventoryItem` 恒扣一（见其
    // 文档「为什么没有 amount 字段」），要扣 N 个就产出 N 条——与
    // resolve_use_item 产出单条是同一个效果，只是重复次数不同。
    let mut effects: Vec<Effect> = Vec::new();
    for ingredient in &rule.ingredients {
        let durability = agent
            .inventory
            .iter()
            .find(|stack| stack.def == ingredient.item)
            .and_then(|stack| stack.durability);
        for _ in 0..ingredient.count {
            effects.push(Effect::ConsumeInventoryItem {
                actor,
                def: ingredient.item,
                durability,
            });
        }
    }

    // ⑨ 成品并进背包，复用 pick_up/equip/unequip 三处已经共用的那段
    // 「找可合并的旧堆 → 算合并结果」逻辑。成品是**刚造出来的**，耐久
    // 走 `ItemStack::freshly_made` 那条共同规则（满耐久；没有耐久概念
    // 的成品仍是 `None`），见本函数文档「成品的耐久」一节。
    // 查不到成品定义时按「没有耐久概念」处理，与本函数其余
    // `items.item(...)` 查询同一条「查不到就是查不到」纪律（ADR 0015）。
    //
    // 件数不再直接取 `rule.product_count`：制作类副职（工匠/裁缝/炼金
    // 术士/厨师）的天赋走 `RuleModifier::CraftYield` 在这里加成，见本
    // 函数文档「产出加成接线」一节。一条也没命中时 `craft_yield_bonus`
    // 返回 0，`craft_product_count(n, 0) == n`，对不带这条天赋的行动者
    // 与本批次之前逐位相同。
    let product_max_durability = items.item(rule.product).and_then(|def| def.max_durability);
    let product_count = craft_product_count(
        rule.product_count,
        craft_yield_bonus(
            &agent_rule_modifiers(
                agent,
                race_traits,
                class_traits,
                subclass_traits,
                traits,
                items,
            ),
            rule.category,
        ),
    );
    effects.push(merge_into_inventory_effect(
        agent,
        actor,
        ItemStack::freshly_made(rule.product, product_count, product_max_durability),
        items,
    ));

    // ⑩ 工具磨损——制作确实发生了才走到这里，见本函数文档「工具磨损」
    // 一节。`equipped_tool` 只在「配方点名了工具、身上确实装着一件没坏
    // 的、且它带耐久」三条同时成立时才是 `Some`。
    if let Some(slot) = equipped_tool {
        effects.push(Effect::AdjustEquipmentDurability {
            actor,
            slot,
            delta: -TOOL_DURABILITY_LOSS_PER_CRAFT,
        });
    }

    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    effects.push(Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    });
    effects
}

/// [`Intent::Read`](crate::intent::Intent::Read) 结算（配方发现批次）：读背包里那件东西声明教授的
/// 全部配方里，把行动者**还不知道的那些**写进
/// [`ll_world::entity::Agent::known_recipes`]，并按一次普通行动计费。
///
/// # 判定顺序
///
/// ```text
/// 1. 查 agent，查不到 → 空
/// 2. 背包里没有这一种东西 → 空
/// 3. 查不到物品规则 → 空                （ADR 0015：未注册当作没有）
/// 4. taught_recipes 为空（这件东西不可读）→ 空
/// 5. 逐条过滤掉已经知道的，一条不剩 → 空（「这本书我读透了」）
/// 6. 逐条产出 Effect::LearnRecipe
/// 7. study_experience > 0 时追加一条 Effect::GrantExperience，并按一次
///    普通行动计费
/// ```
///
/// # 书**不**被消耗
///
/// 与 [`resolve_use_item`](super::equipment::resolve_use_item) 最本质的一条差别（完整的逐步对照见
/// [`Intent::Read`](crate::intent::Intent::Read) 文档「为什么是新变体」一节的那张表）：本函数
/// **一条 [`Effect::ConsumeInventoryItem`] 都不产出**。读完一本书，书
/// 还在背上——这既是物理直觉，也让「把书传给同伴读」这件事无需任何额外
/// 机制就能成立。
///
/// # 第 5 步：读透了的书不再消耗回合
///
/// 全部条目都已知时返回空 `Vec`，因此**连时间都不推进**。这不是「静默
/// 吞掉一次操作」，而是与 [`resolve_pick_up`](super::inventory::resolve_pick_up)「脚下没东西就静默作废」
/// 完全同一条既有纪律：一次不可能产生任何结果的行动不该收费。它同时
/// 关掉了一条真实的刷取路径——经验产出（第 7 步，研究经验收窄批次已经
/// 挂上）就挂在这道闸门后面：若这一步产出效果或推进时间，反复读同一本
/// 书就会变成一台经验机器。
///
/// # 为什么效果恒施于发起者自身
///
/// 与 [`Intent::Use`](crate::intent::Intent::Use)/[`Intent::Read`](crate::intent::Intent::Read) 文档同一条范围裁定：读书的是
/// 自己，没有「读给别人听」这个真实场景需要表达。
///
/// # 约束核对
///
/// - C1：只产出 `Vec<Effect>`，一个字节的世界状态都不写。
/// - C3：全程零随机（与 [`resolve_experiment`] 相反，见其文档）——
///   一本书教什么是内容作者写死的事实，没有任何可掷骰的地方。
/// - C5：唯一遍历的两个容器是 `rule.taught_recipes` 与
///   `agent.known_recipes`，都是 `Vec`（保序），不涉及
///   `HashMap`/`HashSet`。
///
/// # 第二个钩子已经挂上：研读经验（研究经验收窄批次）
///
/// 项目所有者把研究类经验**收窄**成两条来源——「就收窄成通过未鉴定
/// 物品和书籍获取经验就好了」。书籍这一条就是第 7 步：读一本书值多少
/// 经验由内容字段 `ll_mod::item::ItemDef::study_experience` 声明
/// （另一条来源见 [`resolve_identify`]）。
///
/// 两件事值得点名：
///
/// - **它不是一条独立的「科研经验」数轴。** 那需要复制整台
///   [`crate::xp_curve`] 机器（自己的等级、自己的曲线、自己的升级
///   级联）而它们与既有那套逐字相同，正是 ADR 0021 点名要避免的抽象。
///   产出的就是既有的 [`crate::effect::Effect::GrantExperience`]。
/// - **防刷没有引入任何新的逐实体状态。** 第 5 步那道「一条不剩就整条
///   作废」的闸门本来就在，第 7 步只是挂在它后面——真教到新配方才有
///   产出，才谈得上经验。
///
/// # 尚未挂上的第三个钩子——如实标注
///
/// - **副职解锁**：[`crate::subclass::grant_subclass_effects`] 这个
///   共用出口已经在，缺的是 [`crate::subclass::SubclassUnlockCatalog`]
///   上的第二种触发器（当前只有「制作计数」一种，见其文档「为什么只有
///   制作这一种」）。
pub(super) fn resolve_read(
    world: &WorldState,
    actor: EntityId,
    def: ContentIndex,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    // ① 行动者。
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // ② 背包里得真的有这一种东西——与 resolve_use_item 第 2 步同款。
    if !agent.inventory.iter().any(|stack| stack.def == def) {
        return Vec::new();
    }
    // ③ 物品规则。④ 空列表 = 这件东西不可读（见 ItemRule::taught_recipes
    // 文档「为什么『可不可读』不是一个独立的布尔字段」一节）。
    let Some(rule) = items.item(def) else {
        return Vec::new();
    };

    // ⑤ 只留下还不知道的。全都知道时下面的 is_empty 分支会整条作废。
    let mut effects: Vec<Effect> = Vec::new();
    for recipe in &rule.taught_recipes {
        if agent.known_recipes.contains(recipe) {
            continue;
        }
        // 同一本书把同一条配方写了两遍时（内容作者的笔误），这里会
        // 产出两条 LearnRecipe，而 apply 是无条件 push——因此在产出侧
        // 就去重，与上面那道 `known_recipes` 过滤一起，保证
        // `known_recipes` 里不会出现重复项。
        if effects.iter().any(
            |effect| matches!(effect, Effect::LearnRecipe { recipe: known, .. } if known == recipe),
        ) {
            continue;
        }
        effects.push(Effect::LearnRecipe {
            actor,
            recipe: *recipe,
        });
    }
    if effects.is_empty() {
        return Vec::new();
    }

    // ⑥ 研读经验（研究经验收窄批次）——**只有走到这里才给**：上面的
    // is_empty 分支已经把「这本书我读透了」整条挡掉，因此反复读同一本
    // 书恒零收益，不需要任何新的逐实体「读过没有」状态。
    if rule.study_experience > 0 {
        effects.push(Effect::GrantExperience {
            target: actor,
            amount: rule.study_experience,
        });
    }

    // ⑦ 计费口径与 resolve_wait/resolve_use_item/resolve_craft 完全相同
    // （基础代价 × 敏捷速度），不新增任何常量。
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    effects.push(Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    });
    effects
}

/// [`Intent::Experiment`](crate::intent::Intent::Experiment) 结算（配方发现批次）：拿行动者手头现有的材料，
/// 在指定的配方类别里试做一次——命中某条**尚未知晓、且食材恰好齐全**的
/// 配方就学会它，按一次普通行动计费。项目所有者裁定「菜谱就是通过
/// **随机丢入东西煮**获取」的落点。
///
/// # 判定顺序
///
/// ```text
/// 1. 查 agent，查不到 → 空
/// 2. 副职闸门：类别要求非空且与 agent.subclasses 无交集 → 空
/// 3. 列出这个类别下的全部配方，逐条筛出「候选」：
///      requires_discovery == true          （不需要发现的配方无从「发现」）
///      && !known_recipes.contains(recipe)  （已经知道的不必再试）
///      && 食材全部齐全                      （手上真的有这些东西）
/// 4. 候选为空 → 空（这次什么都没试出来，也不消耗回合）
/// 5. 在候选里掷一次骰选中一条，产出 Effect::LearnRecipe 并计费
/// ```
///
/// # 为什么失败与成功都**不消耗食材**——本函数最重要的一条设计判断
///
/// `crafting-system.md` 九节⑤论证过「一次吃掉材料、玩家无法通过任何
/// 决策规避的失败只增加重复劳动」。那条论证在这里**更强**，不是更弱，
/// 四条理由各自独立成立：
///
/// 1. **玩家在做决定时手上没有任何信息。** 制作失败至少还能靠「提升
///    技能/换更好的工具」去规避；而「哪几味材料凑得成一条我还没发现的
///    配方」这个问题，在发现之前**定义上不可知**。让不可知的判断吃掉
///    真实资源，是纯粹的随机罚款，没有任何决策内容。
/// 2. **发现和制作是两件事。** 本函数成功时也**不产出任何成品**——它
///    只把一条配方写进脑子里。既然什么都没做出来，就没有什么材料「变成
///    了别的东西」。真正的消耗留在其后每一次真实的 [`Intent::Craft`](crate::intent::Intent::Craft)，
///    而那时玩家已经完全知道自己在做什么，消耗因此是有信息的代价。
/// 3. **消耗会让这个机制被绕开而不是被使用。** 试做要吃材料的话，最优
///    策略是囤着不试、等着捡书——一条所有者点名要的发现路径会退化成
///    没人走的路。
/// 4. **代价已经收过了。** 每次试做消耗一个完整回合（第 5 步的
///    [`Effect::ScheduleNext`]），而回合在 roguelike 里是硬通货：饥饿在
///    走、怪物在动、火把在烧。这是一条玩家能感知、也能通过「先找个安全
///    地方再试」来管理的真实代价。
///
/// # 那会不会退化成「每回合按一下试做」的无脑刷？
///
/// 不会，而且这一点由第 3 步的筛选条件**结构性**保证，不靠数值平衡：
/// 候选恒是「食材已经齐全的未知配方」。手上没有任何成套材料时候选为
/// 空，试一万次也是空；把当前手头能试出来的都试出来之后，候选同样为
/// 空。换句话说，「试做」的产出上限完全由**玩家搜集到了什么**决定，
/// 不由他按了多少次决定——刷的是探索与搜集，不是按键。
///
/// # 副职闸门（第 2 步）为什么照判
///
/// 与 [`resolve_craft`] 第 3 步同一份判据、同一个 `RecipeCatalog` 方法：
/// 做不了这一类的人，谈不上在这一类里试——不会打铁的人站在铁砧前把
/// 铁锭摆来摆去，不会「发现」出一把剑。
///
/// **这不会造出新的死锁**：`mods/lostland/crafting.json5` 与
/// `ll_mod::content_audit` 的 `detect_unlock_deadlocks` 已经共同保证
/// 「用来练出某个副职的类别」不会反过来要求那个副职（那个环装载期硬
/// 失败）。设了闸门的类别只可能是「已经有副职的人才碰得到的进阶类别」，
/// 而这正是它该有的样子。读书那条路径**不受闸门约束**（[`resolve_read`]
/// 完全不查类别）——知识可以先于资格获得，两条路径因此不是互相的备份，
/// 而是两种不同的获取方式。
///
/// # 随机流怎么构造（约束 C3）
///
/// 三元组取 `(world.seed, actor.as_u64(), world.clock.0 ^ 常量标签)`，
/// 与 [`resolve_inspect`](super::inventory::resolve_inspect) 的隐匿判定、[`resolve_attack`](super::combat::resolve_attack) 的骰子/偷袭两
/// 条流手法逐字相同：世界种子 + 发起者 + 当前时刻，异或一个只用来把
/// 这条流与同一 `(种子, 实体, 时刻)` 下其它流区分开的固定标签
/// （`EXPERIMENT_EVENT_TAG`，没有数值含义）。**新造一条流、只取一个
/// 数**，不是一条跨调用累进的长流，因此「这次没试成（候选为空、提前
/// 返回）」不会让后续任何取数错位。
///
/// 掷骰只用来**在多个候选之间选一个**，不用来判定「成不成功」——候选
/// 非空时必定学会一条。理由同上一节：成不成功已经由「食材齐不齐」这个
/// 玩家完全可控的条件回答了，再叠一层概率只是把可控的事重新变成不可控。
///
/// 候选列表的顺序由 [`crate::craft::RecipeCatalog::recipes_in_category`]
/// 保证按索引升序（见其文档），再经上面三条谓词过滤，全程 `Vec`，
/// 不涉及任何 `HashMap`/`HashSet` 迭代顺序（约束 C5）。
///
/// # 已知边界（与 [`resolve_craft`] 第 7 步逐字相同）
///
/// 食材齐全的判定只认**第一条** `def` 匹配的堆，不跨多堆合并计数——
/// 背包里两堆各 1 个铁锭时，需要 2 个的配方判定为「料不够」。两处共用
/// 同一段判定（[`has_all_ingredients`]），因此这条边界不会在两边漂移。
pub(super) fn resolve_experiment(
    world: &WorldState,
    actor: EntityId,
    category: ContentIndex,
    recipes: &dyn RecipeCatalog,
) -> Vec<Effect> {
    // ① 行动者。
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // ② 副职闸门——与 resolve_craft 第③步同一份判据、同一个方法。
    let required_subclasses = recipes.category_required_subclasses(category);
    if !required_subclasses.is_empty()
        && !required_subclasses
            .iter()
            .any(|needed| agent.subclasses.contains(needed))
    {
        return Vec::new();
    }
    // ③ 候选筛选，三条谓词全过才算。
    let candidates: Vec<ContentIndex> = recipes
        .recipes_in_category(category)
        .into_iter()
        .filter(|index| {
            if agent.known_recipes.contains(index) {
                return false;
            }
            let Some(rule) = recipes.recipe(*index) else {
                return false;
            };
            rule.requires_discovery && has_all_ingredients(agent, &rule)
        })
        .collect();
    // ④ 一条都试不出来：不产出效果，也不消耗时间。
    if candidates.is_empty() {
        return Vec::new();
    }

    // ⑤ 掷一次骰选中一条，见本函数文档「随机流怎么构造」一节。
    let mut rng = ll_core::rng::DetRng::for_entity(
        world.seed,
        actor.as_u64(),
        (world.clock.0 as u64) ^ EXPERIMENT_EVENT_TAG,
    );
    let picked = candidates[rng.gen_range(candidates.len() as u64) as usize];

    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![
        Effect::LearnRecipe {
            actor,
            recipe: picked,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// [`Intent::Identify`](crate::intent::Intent::Identify) 结算（未鉴定物品批次 + 盲盒批次）：鉴定背包里
/// 的一种未鉴定物品。**两条互斥的路径**，由这件物品有没有声明盲盒池
/// （`ItemRule::blind_box_pool`）决定：
///
/// | | 普通鉴定 | 盲盒 |
/// |---|---|---|
/// | 物品去向 | **留着**（只是你现在认识它了） | **被消耗** |
/// | 产出 | 无 | **一件随机物品** |
/// | 写世界状态 | `Agent::identified_items` 多一条 | 背包换了内容 |
/// | 性质 | 揭示 | **转化** |
///
/// # 判定顺序
///
/// ```text
/// 1. 查 agent，查不到 → 空
/// 2. 背包里没有这一种东西 → 空
/// 3. 查不到物品规则 → 空                     （ADR 0015：未注册当作没有）
/// 4. requires_identification 为假 → 空       （这件东西一眼就认得）
/// 5a. 不是盲盒：已经认识过这一种 → 空         （防刷闸门，见下）
///     否则 → Effect::IdentifyItem [+ GrantExperience] + 计费
/// 5b. 是盲盒：按权重抽一条 → ConsumeInventoryItem + MergeIntoInventory
///     [+ GrantExperience] + 计费
/// ```
///
/// # 防刷：普通鉴定靠「一次性事件」，不需要任何新的逐实体状态
///
/// 第 5a 步的闸门读的是 [`ll_world::entity::Agent::identified_items`]
/// ——那**同时**是这条路径的产出目标。于是「认出一个新种类」天然是一次
/// 一次性事件：第二次鉴定同一种东西恒返回空 `Vec`，既不给经验、**也不
/// 消耗时间**（与 [`resolve_read`] 第 5 步、[`resolve_pick_up`](super::inventory::resolve_pick_up)「脚下
/// 没东西就静默作废」同一条既有纪律：一次不可能产生任何结果的行动不该
/// 收费）。这条设计最值钱的性质就在这里——**它不需要任何新的逐实体
/// 「研究过没有」状态**，`identified_items` 本来就要存。
///
/// # ⚠ 盲盒是那条防刷原则的**有意例外**
///
/// 项目所有者裁定，原话：**「开盲盒都给吧，轻松点，这是游戏」**。第 5b
/// 步无条件给经验：不查产出物认不认识，也不查这种盒子开过没有。完整的
/// 取舍论证与那条「⚠ 给盲盒写配方会打开经验水龙头」的警告写在
/// `ll_mod::item::ItemDef::blind_box_pool` 文档里——写在**内容字段**上，
/// 是为了让日后给盲盒加配方的人在写下那条配方之前就看见它。
///
/// **普通鉴定与读书两条路径不受这条影响，一个字都没改。**
///
/// # 盲盒的随机流怎么构造（约束 C3）
///
/// 三元组取 `(world.seed, actor.as_u64(), world.clock.0 ^ 常量标签)`，
/// 与 [`resolve_experiment`]/[`resolve_inspect`](super::inventory::resolve_inspect)/[`resolve_attack`](super::combat::resolve_attack) 那
/// 几条流手法逐字相同：**新造一条流、只取一个数**，因此上面任何一步
/// 提前返回都不会让后续取数错位。标签是 [`BLIND_BOX_EVENT_TAG`]。
///
/// **没有盲盒声明时不构造流**——第 5a 步一行 `DetRng` 都不碰，与
/// [`resolve_attack`](super::combat::resolve_attack)「没有偷袭声明就不构造流」同一条既有纪律。
///
/// 加权选取本身**照抄** [`ll_world::weather::weather_kind_at`]：权重
/// 求和 → `gen_range(总和)` → 沿同一顺序前缀和 walk。不另发明，理由见
/// [`ll_sim::item::BlindBoxEntry`](crate::item::BlindBoxEntry) 文档。
/// 遍历的是 `Vec`（保序，约束 C5），不涉及 `HashMap`/`HashSet`。
///
/// # 产出物的耐久
///
/// 开出来的东西是**新的**：耐久等于产出物那条定义声明的上限，走
/// [`ItemStack::freshly_made`] 那条共同规则——与 [`resolve_craft`]
/// 造成品那一行**逐字相同**。盲盒刻意不在这里发明第二套答案。
///
/// 本节此前记录的是这条规则**还不存在**时的形状（两个产出点都恒把
/// 耐久设成 `None`，于是开出来的铁短剑永远不会磨损）；那条缺陷已随
/// [`ItemStack::freshly_made`] 落地一并修掉，见该构造器文档。
///
/// # 一个盲盒不能开出它自己
///
/// 由**注册期**拒绝（`ll_mod::content_schema_gear::apply_item_extras`），
/// 不在这里判。理由是效果顺序：`ConsumeInventoryItem` 与
/// `MergeIntoInventory` 都按 `(def, durability)` 定位同一堆，而后者的
/// `resulting` 是在**消耗之前**的背包上算出来的——自产出的盒子会让这
/// 两条效果互相抵消，症状是「开了个盒子，什么都没发生」。把它拦在注册
/// 期，这里就不需要一条只为一种病态内容存在的运行期分支。
///
/// # 约束核对
///
/// - C1：只产出 `Vec<Effect>`，一个字节的世界状态都不写。
/// - C3：随机只有盲盒那一路，走 `DetRng::for_entity`，见上。
/// - C5：遍历的三个容器（`agent.inventory`、`agent.identified_items`、
///   `rule.blind_box_pool`）都是 `Vec`，保序。
pub(super) fn resolve_identify(
    world: &WorldState,
    actor: EntityId,
    def: ContentIndex,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    // ① 行动者。
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // ② 背包里得真的有这一种东西——与 resolve_read 第 2 步同款，但这里
    // 还要留住这一堆本身：盲盒那一路要用它的耐久来定位被消耗的堆。
    let Some(held) = agent.inventory.iter().find(|stack| stack.def == def) else {
        return Vec::new();
    };
    let held_durability = held.durability;
    // ③ 物品规则。
    let Some(rule) = items.item(def) else {
        return Vec::new();
    };
    // ④ 一眼就认得的东西没有可鉴定的。
    if !rule.requires_identification {
        return Vec::new();
    }

    // 计费口径与 resolve_read/resolve_wait/resolve_craft 完全相同
    // （基础代价 × 敏捷速度），不新增任何常量。
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    let schedule = Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    };
    let experience = (rule.study_experience > 0).then_some(Effect::GrantExperience {
        target: actor,
        amount: rule.study_experience,
    });

    if rule.blind_box_pool.is_empty() {
        // ⑤a 普通鉴定：揭示，不转化。已经认识过就整条作废（防刷闸门）。
        if agent.identified_items.contains(&def) {
            return Vec::new();
        }
        let mut effects = vec![Effect::IdentifyItem { actor, def }];
        effects.extend(experience);
        effects.push(schedule);
        return effects;
    }

    // ⑤b 盲盒：转化。加权抽一条，手法照抄 weather_kind_at。
    let total: u64 = rule
        .blind_box_pool
        .iter()
        .map(|entry| u64::from(entry.weight))
        .sum();
    // 理论不可达：注册期已经拒绝了权重为 0 的候选（`ItemError::
    // DegenerateBlindBoxEntry`），非空池的总和必然 > 0。防御性地静默
    // 作废而不是让 `gen_range(0)` 去 panic——同 `weather_kind_at` 对
    // 「总和为 0」的处理立场。
    if total == 0 {
        return Vec::new();
    }
    let mut rng = ll_core::rng::DetRng::for_entity(
        world.seed,
        actor.as_u64(),
        (world.clock.0 as u64) ^ BLIND_BOX_EVENT_TAG,
    );
    let mut roll = rng.gen_range(total);
    let mut picked = rule.blind_box_pool[rule.blind_box_pool.len() - 1];
    for entry in &rule.blind_box_pool {
        let weight = u64::from(entry.weight);
        if roll < weight {
            picked = *entry;
            break;
        }
        roll -= weight;
    }

    let mut effects = vec![
        Effect::ConsumeInventoryItem {
            actor,
            def,
            durability: held_durability,
        },
        merge_into_inventory_effect(
            agent,
            actor,
            // 开出来的东西与制作出来的东西一样是"新的"，走同一条共同
            // 规则，见本函数文档「产出物的耐久」一节。
            ItemStack::freshly_made(
                picked.item,
                picked.count,
                items.item(picked.item).and_then(|def| def.max_durability),
            ),
            items,
        ),
    ];
    effects.extend(experience);
    effects.push(schedule);
    effects
}

/// 把 [`resolve_identify`] 盲盒那条随机流与同一 `(种子, 实体, 时刻)` 下
/// 其它流区分开的固定标签，没有数值含义上的特殊性——手法同
/// [`EXPERIMENT_EVENT_TAG`]，只要求「与别的流的三元组不同」。
pub(super) const BLIND_BOX_EVENT_TAG: u64 = 0x0B11_0DB0_0000_0000;

/// 把 [`resolve_experiment`] 那条随机流与同一 `(种子, 实体, 时刻)` 下
/// 其它流区分开的固定标签，没有数值含义上的特殊性——手法同
/// [`resolve_attack`](super::combat::resolve_attack) 内部的 `DAMAGE_FORMULA_DICE_EVENT_TAG`，只要求
/// 「与别的流的三元组不同」。
pub(super) const EXPERIMENT_EVENT_TAG: u64 = 0x0EE0_0BEE_0000_0000;

/// 行动者的背包是否凑得齐这条配方的全部食材——[`resolve_craft`] 第 7 步
/// 与 [`resolve_experiment`] 第 3 步共用的同一段判定。
///
/// 抽成函数的理由符合 ADR 0021「有真正可共享的算法」：两处共享的不只是
/// 一个循环，还包括那条**已知边界**（只认第一条 `def` 匹配的堆，不跨堆
/// 合并计数，见两个调用点各自文档）。写两遍的真正代价不是多几行，而是
/// 那条边界会在两边各自漂移——制作说「料不够」而试做说「料够」是一个
/// 玩家可见、且极难归因的缺陷。
pub(super) fn has_all_ingredients(agent: &Agent, rule: &RecipeRule) -> bool {
    rule.ingredients.iter().all(|ingredient| {
        let held = agent
            .inventory
            .iter()
            .find(|stack| stack.def == ingredient.item)
            .map_or(0, |stack| stack.count);
        held >= ingredient.count
    })
}
