//! [`Intent::TendTree`](crate::intent::Intent::TendTree) 的结算：砍伐、
//! 培植、采果三条路。
//!
//! 架构依据是 ADR 0009「默认派生，只存偏差」在树上的应用，两层的定义与
//! 代价见 [`ll_world::tree`] 模块文档；落地计划见
//! `docs/superpowers/plans/2026-09-01-batch32-trees.md` 第六节。
//!
//! # 三条硬约束在这里各自的落点
//!
//! - **C1（`apply` 是唯一写入口）**：本模块一个字节的世界状态都不改，
//!   只产出 [`Effect`]。**全部闸门在这里判完**——`apply` 收到的
//!   [`Effect::SetTreeDeviation`] 已经是算好的最终记录。
//! - **C3（随机流）**：本模块**一次 `DetRng` 都不取**。砍一棵树产出几份
//!   木料是树种的确定函数（[`TreeSpecies::timber_yield`]），不掷骰
//!   ——掷骰会让「砍树」这个动作出现在随机流里，而树是渲染层每帧都在
//!   问的东西，两者混在一起正是 [`ll_world::tree`] 模块文档警告的那件事。
//! - **C5（哈希容器）**：背包是 `Vec`，取第一条匹配（与
//!   `resolve_pick_up`/`resolve_trade` 的既有定位纪律相同）。全程不碰
//!   任何哈希容器。
//!
//! # 「树是部署在地形上的物品」这句要求在这里怎么落地
//!
//! 项目所有者的原话是「树变成部署在地形上的物品，可砍伐、可培植、
//! 可采果」。**树本身不是 `ground_items` 里的一条**（一百万棵存不下，
//! 见 ADR 0009），但它的**产出物**是真正的内容物品：木料与树种都在
//! `mods/lostland/items.json5` 里，走 [`crate::item::ItemCatalog`]。
//! 也就是说「像物品一样可交互」这一半是真的，「像物品一样各存一条」
//! 那一半被派生替代了——这正是所有者裁定并接受的那条代价。

use ll_world::entity::EntityId;
use ll_world::item::ItemStack;
use ll_world::state::WorldState;
use ll_world::terrain::TerrainKind;
use ll_world::tree::{TreeDeviation, TreeSpecies, derived_species_at, tree_at};

use crate::effect::Effect;
use crate::item::ItemCatalog;
use crate::timeline::action_cost;
use crate::tree::{TreeAction, TreeCatalog};

use super::inventory::merge_into_inventory_effect;
use super::stats::effective_speed_from_dexterity;
use super::{BASE_ACTION_COST, schedule_after, within_reach};

/// 结算一次「对这棵树做点什么」。
///
/// # 闸门：任何一道不过就返回**空效果**
///
/// **共用的四道**（三条路都要过）：
///
/// 1. **发起者还在世界里**；
/// 2. **树木内容接线齐全**——[`TreeCatalog`] 三条查询都查得到。缺任何一条
///    就意味着这个会话根本没装树木内容，整条玩法不成立（[`NoTrees`](crate::tree::NoTrees)
///    正是这个情形）；
/// 3. **够得着**——[`within_reach`]，与 `Intent::PickUp` **调的是同一个
///    函数**，不在这里另写一份范围判据（ADR 0021）；
/// 4. **目标格是森林**——`forest` 地形保留当底图是所有者的要求原话。
///
/// **各自那一道**：
///
/// | 动作 | 额外闸门 |
/// |---|---|
/// | [`TreeAction::Fell`] | 那一格现在真的有树 |
/// | [`TreeAction::Harvest`] | 有树**且**果子已长好 |
/// | [`TreeAction::Plant`] | 那一格现在**没有**树**且**背包里有一颗树种 |
///
/// 空效果经 `TurnEngine::try_player_intent` 变成
/// `crate::turn::PlayerTurnOutcome::Nothing`——「按了但什么都没发生」。
/// 与 `resolve_trade` 逐条同一条纪律：不 panic、不产出一条什么都不做的
/// 效果、不新增一条反馈键。
///
/// # 为什么三条路都**消耗一个回合**
///
/// 与 `Intent::Wait` 同一条基础代价（[`BASE_ACTION_COST`] 经敏捷折算）。
/// 不计费的话，玩家可以在一个回合内把整片林子砍光——回合在 roguelike 里
/// 是硬通货（`resolve_craft` 文档那句话），而砍树的全部代价就是时间。
///
/// **这与交易「不消耗回合」不矛盾**：交易是两个人说定一笔账（对话不消耗
/// 回合，交接文档第〇之二第 2 条），砍树是抡斧头。
pub(super) fn resolve_tend_tree(
    world: &WorldState,
    actor: EntityId,
    pos: (i32, i32),
    action: TreeAction,
    trees: &dyn TreeCatalog,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // 内容没接线 ⇒ 这个世界里没有树可以砍。三条一起取，缺一不可。
    let (Some(forest), Some(timber_def), Some(seed_def)) =
        (trees.forest_terrain(), trees.timber(), trees.tree_seed())
    else {
        return Vec::new();
    };
    let forest = TerrainKind::from_index(forest);
    // **不手写取模**：环面换算走既有类型（`TorusSize::wrap`），
    // 与 `resolve_pick_up` 逐字同一行。
    let target = world.size.wrap(pos.0, pos.1);
    if !within_reach(world, agent.pos, target) {
        return Vec::new();
    }
    if world.terrain_at(target) != Some(forest) {
        return Vec::new();
    }

    // **两层的合流点只有 `tree_at` 一处**（ADR 0021）：本函数不自己拼
    // 「先查偏差再查派生」那套规则，问它就是了。
    let standing = tree_at(world, target, forest);

    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    let schedule = Effect::ScheduleNext {
        actor,
        at: schedule_after(world, cost),
    };

    match action {
        TreeAction::Fell => {
            let Some(tree) = standing else {
                return Vec::new();
            };
            let timber = ItemStack {
                def: timber_def,
                // 砍下来的木料**数量由树种决定**——这是「多树种」在玩法上
                // 唯一真实的差异（贴图之外），见 `TreeSpecies::timber_yield`。
                count: tree.species.timber_yield(),
                durability: None,
                // 砍下来的东西归砍的人所有，与「拾取即归属」同一条
                // （`crate::ownership::pick_up_owner` 那条纪律）。这里
                // 走 `holder_owner`：它回答的正是「这个人拿到手的东西
                // 归谁」，本函数不另写一份。
                owner: crate::ownership::holder_owner(world, agent, actor),
            };
            vec![
                Effect::SetTreeDeviation {
                    pos: target,
                    deviation: TreeDeviation::felled(),
                },
                merge_into_inventory_effect(agent, actor, timber, items),
                schedule,
            ]
        }
        TreeAction::Harvest => {
            let Some(tree) = standing else {
                return Vec::new();
            };
            if !tree.fruit_ready {
                return Vec::new();
            }
            let seed = ItemStack {
                def: seed_def,
                count: 1,
                durability: None,
                owner: crate::ownership::holder_owner(world, agent, actor),
            };
            vec![
                Effect::SetTreeDeviation {
                    pos: target,
                    // 树留着，只记下「什么时候采过」。**不是 `felled()`**
                    // ——采一次果就把树采没了是本函数最容易写错的一处。
                    deviation: TreeDeviation {
                        species: Some(tree.species),
                        harvested_at: Some(world.clock),
                    },
                },
                merge_into_inventory_effect(agent, actor, seed, items),
                schedule,
            ]
        }
        TreeAction::Plant => {
            if standing.is_some() {
                // 已经有树了，种不下第二棵。
                return Vec::new();
            }
            let Some(held) = agent.inventory.iter().find(|stack| stack.def == seed_def) else {
                return Vec::new();
            };
            // 种子是可堆叠材料，`durability` 恒 `None`；这里仍然照实
            // 传下去，而不是硬编码 `None`——那样一件将来带耐久的种子
            // 会静默定位到另一堆上。
            let species = planted_species(world, target);
            vec![
                Effect::ConsumeInventoryItem {
                    actor,
                    def: seed_def,
                    durability: held.durability,
                },
                Effect::SetTreeDeviation {
                    pos: target,
                    deviation: TreeDeviation::planted(species, world.clock),
                },
                schedule,
            ]
        }
    }
}

/// 在这一格种下一颗种子会长出什么树。
///
/// **由那块地的气候决定，不由种子决定**——因此这里调的是
/// [`derived_species_at`]（派生层「这一格的气候长什么树」那个函数），
/// 与树木分布走的是**同一个函数**，不是另一套规则。
///
/// 抽成一个具名函数而不是在上面内联：这条规则值得有个名字，且它是本模块
/// 唯一一处需要读 `terrain_shape`/`size` 的地方。
fn planted_species(world: &WorldState, pos: ll_core::torus::TorusPos) -> TreeSpecies {
    derived_species_at(
        world.seed,
        pos,
        world.size.height(),
        world.terrain_shape.climate_band_width,
    )
}
