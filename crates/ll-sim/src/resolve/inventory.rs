//! `resolve::inventory`：在地面与背包之间搬运物品：拾取、搜刮、查看、丢弃、放置家具。
//!
//! 本模块由 [`crate::resolve`] 按意图族拆出（批次 16，纯搬移，没有改动任何
//! 逻辑）。拆分的依据不是行数而是「下一批要往哪里加东西」：意图分派是
//! 新意图族的必经之地，按族分开之后，加一族新意图等于加一个模块，而不是
//! 往一个八千行的文件中间插。分派表本身仍然在 [`crate::resolve`]。

use ll_core::ident::ContentIndex;
use ll_core::torus::TorusPos;
use ll_world::entity::{Agent, AttributeKind, EntityId};
use ll_world::space::Space;
use ll_world::state::WorldState;

use crate::check::{CHECK_DICE, CONCEALMENT_CHECK, CheckSide, opposed_check};
use crate::effect::{CarriedItemSlot, Effect, InspectedItem};
use crate::exposure::AmbientSource;
use crate::formula::attribute_modifier;
use crate::item::{ItemCatalog, ItemStack, can_merge, merge_stacks};
use crate::rule_modifier::{
    agent_rule_modifiers, check_reroll_value, check_roll_bias, concealment_check_modifier,
};
use crate::timeline::action_cost;
use crate::traits::{TraitCatalog, TraitGrantSource};

use super::stats::{derive_stats, effective_speed_from_dexterity};
use super::{BASE_ACTION_COST, schedule_after, within_reach};

/// [`Intent::PickUp`](crate::intent::Intent::PickUp) 结算（P6 第二批：背包与地面物品）：捡起 `actor`
/// 脚下那一堆 `def` 的**非容器**地面物品，若背包已有可合并的同种堆
/// （[`can_merge`]），一并算出合并结果。
///
/// # 为什么带 `def`（本批次改的形状）
///
/// 本变体此前是 `PickUp { actor }`——不指定捡哪一种，由本函数取「脚下
/// 第一条非容器堆」。理由当时写的是「捡东西的人事先并不知道地上那堆
/// 到底是什么」，且「多堆选择 UI 不在本批次范围内」。
///
/// 项目所有者的裁定推翻了它：
///
/// > 玩家互动的时候应该是显示一个列表让玩家选择捡起哪个，而 npc 则是
/// > 根据他需要的捡起来
///
/// 两侧都要求「由发起者指定捡哪一个」，本函数因此不再替任何人做这个
/// 选择——与 [`Intent::EnterSpace`](crate::intent::Intent::EnterSpace) 的 `target`「同一格多入口时选哪一个
/// 不该由 `resolve` 替玩家决定，那是一个真实的玩法选择」是同一条既有
/// 纪律的第二次落地。玩家那一侧的列表由 `ll_game::player_action` 建出
/// （顺序确定，见该模块文档）；NPC 那一侧今天还没有任何拾取行为，
/// 新形状只是把「按自己的需要挑」这件事变得**表达得出来**，如实标注。
///
/// # 够得着的范围（本批次新增的 `pos`）
///
/// 项目所有者定的交互形状是「按空格 → 扫一圈 → 选一格 → 选这格上的
/// 哪一样」，那一圈包含相邻格。若拾取仍然只认脚下，方向列表里选中一个
/// 相邻格之后能做的事就是零，整条交互是死的。
///
/// 范围是 [`INTERACT_REACH`](super::INTERACT_REACH)（切比雪夫距离 1，即脚下加相邻八格）——
/// **与移动的方向数一致**：[`Direction`](crate::intent::Direction) 是八向（含四条对角线），一个
/// 「伸手够得着的一圈」若只认正交四向，玩家会遇到「斜前方那堆东西看得
/// 见、走一步就到，却伸手够不着」这种毫无道理的不一致。
///
/// 距离用 [`ll_core::torus::TorusSize::chebyshev`]，不是自己减坐标：
/// 世界是环面，跨接缝时裸减法会算出一个绕整圈的巨大距离。
///
/// # 放置状态不影响能不能捡
///
/// 立着的炉子照样捡得走（`placed` 为真的那一堆和别的一样进这个函数）
/// ——这是「摆下去还能收回来」这条闭环的唯一出口。移除之后那一格自然
/// 就不再有放置物，`resolve_drop` 的闸门随之放行。
///
/// # 静默无效的三种情形
///
/// `actor` 不存在，脚下没有 `def` 这一堆，或脚下这一堆 `def` 是容器
/// （尸体，见下「为什么跳过容器」一节）——与 `resolve_attack`/
/// `resolve_open_door` 目标不存在时的既有纪律一致（见模块文档开篇
/// 「目标实体……若已不在 `world.actors` 中……一律返回空 `Vec`」），不是
/// 错误，只是这一步什么都不发生。
///
/// # 为什么跳过容器——**尸体已经不在这一类里了**
///
/// 容器（[`ll_world::item::GroundItemStack::contents`] 非空）不是
/// [`Intent::PickUp`](crate::intent::Intent::PickUp) 的合法目标——本函数只会把 `ground.stack` 这一个
/// 字段拿去合并进背包，容器真正的价值（`contents` 里的东西）会被原样
/// 丢在地上、永久不可达，这不是"物品异常地不能堆叠"那类可以接受的
/// 降级，是真实的数据丢失。搜刮容器走专门的
/// [`Intent::Loot`](crate::intent::Intent::Loot)（[`resolve_loot`]），本函数因此显式过滤掉
/// `!item.contents.is_empty()` 的地面物品。
///
/// **这道排除此前把尸体一并挡住了，那是一个死结**：尸体是容器 ⇒ 捡
/// 不起来 ⇒ `CORPSE_STACK_LIMIT` 只是一条诚实的声明。尸体平铺批次
/// （见 [`append_corpse_drop`](super::combat::append_corpse_drop) 文档「尸体不再是容器」一节）把尸体从
/// 容器这一类里摘了出去——排除本身一个字没改，改的是尸体不再满足它。
/// 今天**没有任何生产路径会造出 `contents` 非空的地面物品**，这道
/// 排除因此暂时空转，等箱子那批把它用起来。
///
/// # 同一格同一个 `def` 有两堆时取哪一条
///
/// 取 [`ll_world::state::WorldState::ground_items`] 存储顺序里的第一条
/// （`Vec` 保序，约束 C5）。这与 [`Effect::RemoveGroundItem`] 按
/// `(pos, def)` 定位的既有边界是同一条（见其文档）：两堆同 `def` 的东西
/// 摞在一格上时，「捡的是哪一堆」与「移除的是哪一堆」由同一个规则回答，
/// 因此不会出现「读了 A、删了 B」的错配。
///
/// # 拾取即归属；盗窃判定的挂载点不在本函数里
///
/// 所有者裁定「默认不归属于谁然后谁拿了就变成谁的」——本函数因此在
/// 产出 [`Effect::MergeIntoInventory`] 之前把这一堆的
/// [`owner`](ll_world::item::ItemStack::owner) 改写成
/// [`crate::ownership::pick_up_owner`] 算出来的值。
///
/// **判定住在那个函数里，不在这里**：`resolve.rs` 已近 8000 行（全仓
/// 最严重的既有行数违规），而归属判定将来只会长大（盗窃、目击、赃物
/// 标记）。设计文档二节 2.1 指定的挂载点是「`resolve_pick_up`」，
/// [`crate::ownership::pick_up_owner`] 就是它抽出来的那一半，判定需要的
/// 全部输入都在它的参数里——犯罪批次改那一个函数即可，不必再进本文件。
///
/// # 为什么合并结果由这里算好，`apply` 只做替换
///
/// 见 [`Effect::MergeIntoInventory`] 文档「为什么合并结果由 `resolve`
/// 算好」一节：`stack_limit` 查不到（`items` 没有这个 `def` 的记录）
/// 时按「不限量」处理（`u32::MAX`），理由见 [`NoItems`](crate::item::NoItems) 文档——没有
/// 真实的物品注册表可查不该表现成"这件物品异常地不能堆叠"。
pub(super) fn resolve_pick_up(
    world: &WorldState,
    actor: EntityId,
    pos: (i32, i32),
    def: ContentIndex,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let target = world.size.wrap(pos.0, pos.1);
    if !within_reach(world, agent.pos, target) {
        return Vec::new();
    }
    let Some(ground) = world
        .ground_items
        .iter()
        .find(|item| item.pos == target && item.stack.def == def && item.contents.is_empty())
    else {
        return Vec::new();
    };
    // 拾取即归属（归属批次，所有者原话「谁拿了就变成谁的」）——判定
    // 本身住在 crate::ownership::pick_up_owner，那里也是盗窃判定将来的
    // 挂载点，见该函数文档。这里只做机械的字段改写。
    let picked = ItemStack {
        owner: crate::ownership::pick_up_owner(world, agent, actor, ground.stack),
        ..ground.stack
    };

    vec![
        Effect::RemoveGroundItem {
            pos: ground.pos,
            def: picked.def,
        },
        merge_into_inventory_effect(agent, actor, picked, items),
    ]
}

/// [`Intent::Loot`](crate::intent::Intent::Loot) 结算：把 `actor` 脚下第一个容器
/// （[`ll_world::item::GroundItemStack::contents`] 非空）的全部内容物
/// 移进背包，容器本身随后从地面移除——「搜刮」是
/// 一次性、全部拿走，不支持挑拣部分战利品,与 `Intent::Drop`「不支持
/// 部分数量」同一条范围裁定（见其文档）：本批次的验收范围不需要战利品
/// 挑选 UI,提前引入只会制造一个当前没有测试覆盖的分支。
///
/// # 今天没有任何生产者会造出它的目标（尸体平铺批次）
///
/// 本函数**保留但暂时空转**：尸体曾经是唯一的容器生产者，尸体平铺
/// 批次把它摘走了（见 [`append_corpse_drop`](super::combat::append_corpse_drop) 文档「尸体不再是容器」
/// 一节），而箱子那批还没开工。保留而不是删掉，是因为箱子是
/// [`ll_world::item::GroundItemStack::contents`] 将来的正经消费者，
/// 删掉再写一遍是净损失——判据、`Effect` 复用、已知限制这几段论证都
/// 已经成立，不会因为暂时没有生产者而失效。
///
/// # 静默无效的两种情形
///
/// `actor` 不存在，或脚下没有任何容器（**今天恒是这一支**）——与
/// [`resolve_pick_up`] 同一条纪律。
///
/// # 为什么容器本身用 [`Effect::RemoveGroundItem`]，不新开一个变体
///
/// 与 [`resolve_pick_up`] 移除已拾取的普通地面物品是同一个机械操作
/// （按 `(pos, def)` 定位并移除），没有理由为"移除的这一条恰好是容器"
/// 单独发明一个效果变体——`apply` 侧的写入逻辑完全相同。
///
/// # 已知限制：容器按 `(pos, def)` 定位，多具同 `def` 容器共存一格时
/// 可能误删
///
/// 与 [`Effect::RemoveGroundItem`] 文档「为什么按 `(pos, def)` 定位」
/// 一节同一条既有限制：若同一格恰好摞着两具"生物种类相同"的尸体
/// （`def` 相同，见 [`append_corpse_drop`](super::combat::append_corpse_drop) 文档「尸体的 `def`」一节），
/// `Effect::RemoveGroundItem` 按 `(pos, def)` 匹配到的不保证是本函数
/// 读到的那一具——这是"第一条匹配"既有纪律（`Intent::PickUp` 文档
/// 「为什么不指定要捡哪一种」一节同一先例）在容器场景下的延伸,不是本
/// 批次新引入的缺陷,如实记录为已知边界情形。
///
/// # 已知限制：不处理"搜刮的多条战利品本可以互相合并"的情形
///
/// 与 [`merge_into_inventory_effect`] 文档「已知限制」一节同一条既有
/// 局限：每条内容物各自基于同一份背包快照判断"有没有可合并的旧堆"，
/// 不产生数据错误（数量守恒），只是可能错过一次本可以做的合并。
pub(super) fn resolve_loot(
    world: &WorldState,
    actor: EntityId,
    pos: (i32, i32),
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let target = world.size.wrap(pos.0, pos.1);
    if !within_reach(world, agent.pos, target) {
        return Vec::new();
    }
    let Some(container) = world
        .ground_items
        .iter()
        .find(|item| item.pos == target && !item.contents.is_empty())
    else {
        return Vec::new();
    };

    let mut effects = vec![Effect::RemoveGroundItem {
        pos: container.pos,
        def: container.stack.def,
    }];
    effects.extend(
        container
            .contents
            .iter()
            .map(|loot| merge_into_inventory_effect(agent, actor, *loot, items)),
    );
    effects
}

/// [`Intent::Inspect`](crate::intent::Intent::Inspect) 的结算：读 `target` 此刻背包与已装备的全部
/// 物品定义，打成一份快照，产出 [`Effect::Inspect`]——卫兵职业接线
/// 批次唯一的产出者，见该效果文档「为什么 apply 不把它写进
/// WorldState::history」一节。
///
/// `actor`/`target` 任一方已经不在 `world.actors`（同一批结算里被
/// 更早的效果销毁，或调用方给的句柄已经过期）都返回空 `Vec`——与本
/// 文件其余 `resolve_*` 同一条既有纪律（见 [`resolve`](super::resolve) 文档）。
///
/// # 不做任何合法性判断
///
/// `Owner`/`stolen_marker` 尚未落地（见 `Effect::Inspect` 文档「为什么
/// 没有任何是否违法的判断」一节引用的设计文档）——本函数只如实读出
/// `target` 此刻持有的物品定义列表，不比较、不裁定，"这堆东西是不是
/// `target` 自己的"这个问题本批次回答不了。
///
/// # 谁来判断"该不该发起这次盘查"
///
/// 不是本函数——是否发起盘查（卫兵职业、视野内是否有目标、这一次的
/// 概率判定）全部在 AI 决策阶段（行为树脚本，`ll_script::api::rng`
/// 的 `rng-chance` 原语）完成，`Intent::Inspect` 一旦产出，本函数
/// 恒执行、不重新判断"该不该查"——与 `resolve_attack` 不重新判断
/// "这一下该不该打"是同一条既有分工：决策在决定要不要产生这个
/// `Intent` 的那一步，`resolve` 只负责把已经决定要做的事翻译成
/// `Effect`。
///
/// # 盘查消耗一个回合（进展保证，不是手感取舍）
///
/// 本函数产出的第二条效果是 [`Effect::ScheduleNext`]，与
/// [`resolve_toggle_stealth`](super::movement::resolve_toggle_stealth)/[`resolve_move`](super::movement::resolve_move) 完全同一种算法
/// （`action_cost(BASE_ACTION_COST, 有效速度)`）。
///
/// 它此前**没有**这一条，那是一个真实缺陷，只是因为 `Intent::Inspect`
/// 至今没有任何调用方经由 [`crate::turn::TurnEngine`] 产出过而一直没有
/// 暴露：`TurnEngine::perform` 结算完一次行动后按 `Agent::next_action_at`
/// 把行动者重新排回时间轴，而没有 `ScheduleNext` 就意味着这个字段原地
/// 不动——同一条时间轴记录会在**同一个 tick** 被立刻再弹出，行为树又
/// 因为世界时钟没变而抽到同一个随机数、作出同一个决策，直到耗尽
/// `MAX_STEPS_PER_ADVANCE` 才放弃。这正是
/// [`crate::turn::TurnEngine::advance_ai`] 文档「必须保证进展（曾经的
/// 真实死循环）」一节描述的那条死循环，只是这一次的成因不在 AI 策略侧
/// 而在 `resolve` 侧。
///
/// 发现它的方式就是把卫兵行为树真的接上回合引擎跑一遍——「接线断在
/// 最后一环」这类缺陷只有在真的把线接上之后才会暴露下一环。
///
/// # 藏匿判定（盗贼被动两分批次）
///
/// 所有者裁定「被动可以分为 **2 种**，**不觉得可疑**，还有**查不出
/// 东西**」——后一种落在本函数：盘查照常发起、照常消耗一个回合，
/// 只是 `items_seen` 里被藏起来的那些物品不再出现。判据是
/// **`target` 自己**（不是盘查者）身上聚合出的
/// [`crate::rule_modifier::RuleModifier::InspectionConcealment`]，走
/// [`crate::rule_modifier::agent_rule_modifiers`] 这个唯一聚合点，
/// 与 [`resolve_attack`](super::combat::resolve_attack) 读偷袭声明是同一条既有路径。
///
/// 逐件掷骰，不是一次判定决定整份快照——形状的完整论证见
/// [`crate::rule_modifier::RuleModifier::InspectionConcealment`] 文档「为什么是逐件掷骰」
/// 一节。
///
/// # 换成对抗判定（判定系统落地批次）
///
/// 每一件物品掷的不再是一次「藏匿千分比」的硬币，而是一次**对抗
/// 判定**（[`crate::check::opposed_check`]，`3d20 + 修正` 双方各一轮）：
///
/// ```text
/// 盘查者（主动）：意志调整值            察觉
/// 被盘查者（被动）：敏捷调整值 + 藏匿修正   隐蔽
/// ```
///
/// 主动方赢下这一件，这一件才留在 `items_seen` 里。
///
/// **「察觉 = 意志调整值、隐蔽 = 敏捷调整值」是项目所有者的裁定**，
/// 不是本函数发明的映射；本仓库没有独立的感知属性，
/// [`ll_world::entity::AttributeKind::Willpower`] 是六项里承担 D&D
/// 「感知」概念的那一项（见其字段文档与
/// [`crate::formula::FormulaOperand::AttributeModifier`] 对 `wis-mod`
/// 的同一条说明）。调整值公式 `(属性 − 10) / 2` 复用
/// [`crate::formula::attribute_modifier`]，零新增字段、零存档影响。
///
/// 换掉的是什么：旧形状里搜身的人是谁完全不影响结果——一个眼神再好
/// 的卫兵与一个瞎子查同一个人，查到的东西逐位相同。对抗判定把盘查者
/// 放回了式子里。
///
/// 数值后果（`3d20`，双方属性均为基准 10 因而两侧调整值均为 0，
/// 天赋声明 9 点即半颗骰子）：这一件被藏住的概率从旧值 `800‰` 变成
/// `745‰`（主动方赢面 `255‰`）——同一档，但不再是一个与任何人无关的
/// 常数。旧的 `800‰` 本身是概率模型时代的自由参数，本批次不逐字复刻
/// 它，改用骰子量尺上有内在依据的「半颗骰子」，见
/// [`crate::check::CheckDice::half_die`]。
///
/// 槽位句柄批次把 `items_seen` 的元素从裸 `ContentIndex` 换成
/// [`crate::effect::InspectedItem`]（种类 + 位置），**这一步的粒度一个
/// 字都没变**：`retain` 仍然是一条记录一次掷骰，一条记录仍然对应一堆
/// 物品。取数次数因此与换形状之前逐位相同（同一份快照、同样的元素
/// 个数、同样的顺序），既有的确定性断言与那条「出现过查到一部分的中间
/// 结果」的端到端证据（`crates/ll-mod/tests/example_mod_rogue_passives.rs`）
/// 都不需要跟着改（换成对抗判定之后取数**次数**变了——每件从 1 次
/// 变成 `2M` 次——但取数的**粒度与顺序**仍然逐字相同：一条记录一次
/// 判定，顺序仍是快照自身的顺序）。真正被这次换形状加强的是**下游**：那条被动当初就是
/// 照着「逐堆比对归属」的粒度选的（见上述变体文档），而在旧形状里
/// 「逐堆」根本表达不出来。
///
/// **约束 C3**：随机走 `DetRng::for_entity(世界种子, 实体 ID, 事件
/// 计数)`，三元组的中间一项取 **`target`**（藏东西的那一方，判定属于
/// 它的被动，不属于盘查者），事件计数用一个与本文件其余流都不同的
/// 固定标签异或世界时钟——与 [`resolve_attack`](super::combat::resolve_attack) 里暴击/骰子/偷袭三条
/// 流互不相同是同一套取法。
///
/// **约束 C5**：取数顺序就是 `items_seen` 自身的顺序（先背包原始
/// 顺序、后装备槽位升序，两者都不触碰任何 `HashMap`）。没有任何来源
/// 声明藏匿时（`concealment_check_modifier` 返回 `None`）**完全不构造
/// 这条流**，与
/// [`resolve_attack`](super::combat::resolve_attack) 「没有声明偷袭就不构造额外 `DetRng` 流」同一条
/// 既有纪律：每次判定都是现场造流、只取要用的那几个数,不是一条跨
/// 调用累进的长流,因此「这次没取数」不会让后续任何取数错位。
///
/// # 为什么不在这里判断「盘查该不该发起」
///
/// 被动①（「不觉得可疑」，[`crate::rule_modifier::RuleModifier::InspectionSuspicion`]）
/// **不在本函数**——它减的是行为树掷骰那一步，见本函数文档上一节
/// 「谁来判断该不该发起这次盘查」与该变体自己的文档。两个被动分别
/// 落在链路的两环，是所有者裁定「分为 2 种」的直接落地。
/// `#[allow(clippy::too_many_arguments)]`：多出来的那一个是副职天赋
/// 接线批次新增的第三路天赋来源（`subclass_traits`）。它与
/// `race_traits`/`class_traits` 是并列的同一类依赖，打包成一个中间
/// 类型只会在这条转发链上多一层拆包——理由同本文件其余几处同款豁免。
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_inspect(
    world: &WorldState,
    actor: EntityId,
    target: EntityId,
    race_traits: &dyn TraitGrantSource,
    class_traits: &dyn TraitGrantSource,
    subclass_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    items: &dyn ItemCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(target_agent) = world.actors.get(target) else {
        return Vec::new();
    };
    // 每条记录带着「是什么」+「在哪」两半——背包那一半的「在哪」是
    // 下标，装备那一半是真实存储键（锚点槽位），见
    // `crate::effect::CarriedItemSlot` 文档。下标转 u32 不会截断：
    // 见该类型文档「为什么下标是 `u32`」一节。
    let mut items_seen: Vec<InspectedItem> = target_agent
        .inventory
        .iter()
        .enumerate()
        .map(|(index, stack)| InspectedItem {
            def: stack.def,
            slot: CarriedItemSlot::Inventory {
                index: index as u32,
            },
        })
        .collect();
    items_seen.extend(
        target_agent
            .equipment
            .iter()
            .map(|(slot, stack)| InspectedItem {
                def: stack.def,
                slot: CarriedItemSlot::Equipped { slot: *slot },
            }),
    );
    // 藏匿判定，见本函数文档「藏匿判定」一节。
    const INSPECT_CONCEAL_EVENT_TAG: u64 = 0x0C0A_1EA0_0000_0000;
    let target_modifiers = agent_rule_modifiers(
        target_agent,
        race_traits,
        class_traits,
        subclass_traits,
        traits,
        items,
    );
    // 一条也没有声明 → 完全跳过判定，一次抽取都不消耗（约束 C3），见
    // `concealment_check_modifier` 文档「缺省与声明 0」。显式声明成
    // `0` 是另一回事：判定照常发生，只是这一路贡献 0 点。
    if let Some(concealment) = concealment_check_modifier(&target_modifiers) {
        // 双方的属性调整值走 `derive_stats`（**派生值**，装备与状态
        // 效果加的属性在这里生效），不是裸 `BaseStats`——与
        // `resolve_attack` 读 `attacker_derived.attribute(..)` 同一条
        // 既有纪律。
        //
        // 用不带环境温度的 `derive_stats`（内部代入
        // `Temperature::TEMPERATE_BASELINE`）而不是 `derive_stats_at`：
        // 本函数没有 `ambient` 参数，而温度**只**惩罚力量一项
        // （`derive_stats_at` 里那一行 `attributes[Strength] -= penalty`），
        // 对本判定读的意志/敏捷两项逐位无影响。这不是将就，是这两项
        // 上两个函数确实等价。
        let inspector_derived = derive_stats(
            agent.stats,
            &agent.active_stat_modifiers,
            &agent.equipment,
            items,
            world.clock,
        );
        let target_derived = derive_stats(
            target_agent.stats,
            &target_agent.active_stat_modifiers,
            &target_agent.equipment,
            items,
            world.clock,
        );
        let inspector_modifiers = agent_rule_modifiers(
            agent,
            race_traits,
            class_traits,
            subclass_traits,
            traits,
            items,
        );
        // 察觉 = 意志调整值，隐蔽 = 敏捷调整值（项目所有者裁定）。
        let active = CheckSide {
            modifier: attribute_modifier(inspector_derived.attribute(AttributeKind::Willpower)),
            bias: check_roll_bias(&inspector_modifiers, CONCEALMENT_CHECK),
            reroll_on: check_reroll_value(&inspector_modifiers),
        };
        let passive = CheckSide {
            modifier: attribute_modifier(target_derived.attribute(AttributeKind::Dexterity))
                .saturating_add(i64::from(concealment)),
            bias: check_roll_bias(&target_modifiers, CONCEALMENT_CHECK),
            reroll_on: check_reroll_value(&target_modifiers),
        };
        let mut conceal_rng = ll_core::rng::DetRng::for_entity(
            world.seed,
            target.as_u64(),
            (world.clock.0 as u64) ^ INSPECT_CONCEAL_EVENT_TAG,
        );
        // 逐件一次对抗判定：搜身的人赢下这一件才看得见它。取数顺序
        // 就是 `items_seen` 自身的顺序，每件消耗 `2M`（含优劣势时
        // `4M`、含重掷时更多）个抽取，见 `crate::check` 模块文档。
        items_seen.retain(|_| {
            opposed_check(&CHECK_DICE, &active, &passive, &mut conceal_rng).active_wins()
        });
    }
    let cost = action_cost(
        BASE_ACTION_COST,
        effective_speed_from_dexterity(agent.stats.dexterity),
    );
    vec![
        Effect::Inspect {
            inspector: actor,
            target,
            items_seen,
        },
        Effect::ScheduleNext {
            actor,
            at: schedule_after(world, cost),
        },
    ]
}

/// 把 `incoming` 这一堆物品合并进 `agent` 背包，产出对应的
/// [`Effect::MergeIntoInventory`]——[`resolve_pick_up`]/[`resolve_equip`](super::equipment::resolve_equip)
/// （卸下冲突槽位时）/[`resolve_unequip`](super::equipment::resolve_unequip) 三处共用同一段"找可合并的
/// 旧堆→算合并结果"逻辑，理由是三者都要回答同一个问题："这一堆物品
/// 放进背包后，背包状态该变成什么样"——`resolve_pick_up` 落地时
/// （P6 第二批）这段逻辑还只有它一处调用，装备栏位批次（P6 第三批）
/// 新增两处调用点后再抽取成帮手，避免三份几乎相同的代码分别漂移。
///
/// # 已知限制：不处理"同一批效果里两个新增堆本身能互相合并"的情形
///
/// 见 [`Effect::MergeIntoInventory`] 文档「为什么合并结果由 `resolve`
/// 算好」一节：`agent` 是调用方传入的**只读快照**，若 `resolve_equip`
/// 因双手武器占位冲突要连续卸下两件本可以互相合并的同类物品（例如
/// 两个完全相同的戒指各自被不同规则挤占），本函数各自独立基于同一份
/// 背包快照判断"有没有可合并的旧堆"，不会让这两个新卸下的堆彼此合并
/// ——不产生数据错误（数量守恒，物品不会丢失或复制），只是错过一次
/// 本可以做的合并。这是一个真实但边缘的场景（要求两件不同槽位的
/// 装备恰好实例状态完全相同），本批次不为它引入"batch 内部先自我
/// 合并一遍"的额外机制（YAGNI）。
pub(super) fn merge_into_inventory_effect(
    agent: &Agent,
    actor: EntityId,
    incoming: ItemStack,
    items: &dyn ItemCatalog,
) -> Effect {
    let existing = agent
        .inventory
        .iter()
        .find(|stack| can_merge(stack, &incoming));
    let (replaced, resulting) = match existing {
        Some(existing) => {
            let stack_limit = items
                .item(incoming.def)
                .map_or(u32::MAX, |rule| rule.stack_limit);
            match merge_stacks(*existing, incoming, stack_limit) {
                Ok((merged, overflow)) => {
                    let mut resulting = vec![merged];
                    resulting.extend(overflow);
                    (Some((existing.def, existing.durability)), resulting)
                }
                Err(_) => {
                    // can_merge 刚判定过真——merge_stacks 只会在 def/
                    // durability 不同时拒绝（见其文档），这里理论不可达，
                    // 保守回落到"不合并、直接追加"而不是 panic。
                    (None, vec![incoming])
                }
            }
        }
        None => (None, vec![incoming]),
    };
    Effect::MergeIntoInventory {
        actor,
        replaced,
        resulting,
    }
}

/// [`Intent::Drop`](crate::intent::Intent::Drop) 结算（P6 第二批：背包与地面物品）：把 `actor` 背包
/// 里第一条匹配 `def` 的整堆丢在其当前脚下（见 `Intent::Drop` 文档
/// 「为什么是整堆」一节）。
///
/// # 丢弃与放置是两个动作（家具放置状态批次推翻了此前的形状）
///
/// 家具层那一批把两者合成了一条：「丢一件家具就是放置它」，于是
/// `resolve_drop` 内部按 `ItemDef.furniture` 分叉，家具走三道放置前置、
/// 别的东西不走。项目所有者的裁定推翻了这个形状：
///
/// > 家具如果是放置在那个地方，那物品就无法被丢在那，但是如果家具作为
/// > 一个物品而不是放置状态，就会和其他物品被丢在同一个地方
///
/// 也就是说「是不是家具」根本不是分叉点——**一件没被放置的家具就是
/// 普通物品**，丢它和丢一堆铁锭没有任何区别。真正的分叉点是玩家想做
/// 哪个动作：丢（[`Intent::Drop`](crate::intent::Intent::Drop)，本函数）还是放置
/// （[`Intent::Place`](crate::intent::Intent::Place)，[`resolve_place`]）。
///
/// 本函数因此**不再问这件东西是不是家具**，只保留一道与东西无关的
/// 前置：
///
/// - **这一格已经立着一件放置物时，什么都丢不下去**
///   （[`ll_world::state::WorldState::placed_at`]）。所有者原话的前半
///   句：「那物品就无法被丢在那」。判据是那一格上**已经立着东西**这个
///   事实，与手里丢的是什么无关——所有者另有一句「普通物品和第一点应
///   该是一样的」把这条明确成了通用规则，不是家具专属。
///
/// # 已知边界：只有 `Drop`/`Place` 两条路径认这道闸门
///
/// 尸体掉落（`append_corpse_drop`）与盲盒溢出等其余
/// `Effect::AddGroundItem` 产出点**不**判这一格立没立着东西——一个 NPC
/// 恰好死在锻炉那一格上，尸体照样会摞上去。如实标注为已知边界：把闸门
/// 铺到那些路径上需要它们各自能拿到 `WorldState` 并决定「放不下时尸体
/// 去哪」（挤到旁边一格？直接蒸发？），那是一次独立的裁定，不夹带在
/// 本批次里。
///
/// # 静默无效的情形
///
/// `actor` 不存在、背包里没有匹配 `def` 的堆、这一格已经立着一件放置
/// 物——与 [`resolve_pick_up`] 同一条「静默无效，不是错误」纪律。玩家
/// 那一侧看得见反馈，见 `crate::turn::PlayerTurnOutcome`。
pub(super) fn resolve_drop(world: &WorldState, actor: EntityId, def: ContentIndex) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(stack) = agent.inventory.iter().find(|stack| stack.def == def) else {
        return Vec::new();
    };
    if world.placed_at(agent.pos).is_some() {
        return Vec::new();
    }

    vec![
        Effect::RemoveFromInventory {
            actor,
            def,
            durability: stack.durability,
        },
        Effect::AddGroundItem {
            pos: agent.pos,
            stack: *stack,
            dropped_at: world.clock,
            // 普通丢弃恒不带容器内容物——contents 非空是尸体专属的
            // 判据，见 GroundItemStack::contents 文档。
            contents: Vec::new(),
            // 丢下去的东西是**躺着**的，不是立起来的——立起来走
            // `Intent::Place`，见本函数文档「丢弃与放置是两个动作」。
            placed: false,
        },
    ]
}

/// [`Intent::Place`](crate::intent::Intent::Place) 结算（家具放置状态批次）：把 `actor` 背包里第一条
/// 匹配 `def` 的整堆**立**在其当前脚下——地面上多出一条
/// [`ll_world::item::GroundItemStack`]，且它的
/// [`placed`](ll_world::item::GroundItemStack::placed) 为真。
///
/// 与 [`resolve_drop`] 的分工见该函数文档「丢弃与放置是两个动作」一节。
///
/// # 四道前置，任一不成立整条意图静默作废
///
/// 1. **这件东西可不可以被放置**——[`crate::item::ItemRule::furniture`]。
///    这是 `ItemDef.furniture` 在新形状下的确切含义：它回答「**能不能**
///    立起来」，回答不了「**现在**立没立」（那是实例状态，见
///    `GroundItemStack::placed` 文档）。查不到物品规则时按「不能放置」
///    处理——与 [`resolve_equip`](super::equipment::resolve_equip) 对查不到规则时拒绝装备同一条保守方向：
///    放置会产生持久世界状态变化，必须要求内容明确声明。
/// 2. **这个空间允不允许建造**——
///    [`crate::exposure::AmbientSource::buildable_in`]，本体地表与建筑
///    内部为真、洞窟与地下城为假。这是 `SpaceProfile::buildable`
///    落地至今第一个真实玩法消费者。
/// 3. **脚下这一格有没有被地形占着**——判据是地形自己声明的
///    `TerrainDef::blocks_move`。所有者原话「有些地方上已经有物品了，
///    例如墙啊，之类的乱七八糟，应该就没办法再放置其他东西了」：一堵
///    墙就是那一格上已经有的那件东西。**不是**一张写死在引擎里的地形
///    黑名单——`blocks_move` 是内容（`terrain.json5`）逐条声明的字段，
///    任何 mod 新增的地形自带答案。查不到地形（区块没常驻）按「没被占
///    着」处理，与 `resolve` 一贯对查不到内容的降级方向一致。
/// 4. **这一格是不是已经立着一件东西**——一格至多一件放置物，与
///    [`resolve_drop`] 那道闸门是同一个查询
///    （[`ll_world::state::WorldState::placed_at`]），不是两份判据。
///
/// # 为什么不检查这一格上躺着的普通物品
///
/// 立一座炉子在一堆铁锭上面，物理上没什么荒谬的（炉子占的是这一格的
/// 「设施位」，铁锭还散在地上）。真要禁止，代价是玩家得先把脚下清干净
/// 才能放东西，而「清干净」本身要靠一次次拾取——那是给一个没有玩法收益
/// 的规则配一套繁琐操作。所有者的原话只约束了反方向（**立着的**挡住
/// 丢弃），本函数不额外发明第二条。
pub(super) fn resolve_place(
    world: &WorldState,
    actor: EntityId,
    def: ContentIndex,
    items: &dyn ItemCatalog,
    ambient: AmbientSource<'_>,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let Some(stack) = agent.inventory.iter().find(|stack| stack.def == def) else {
        return Vec::new();
    };
    // ① 这件东西可不可以被放置。
    if !items.item(def).is_some_and(|rule| rule.furniture) {
        return Vec::new();
    }
    if !can_place_at(world, agent.pos, &agent.current_space, ambient) {
        return Vec::new();
    }

    vec![
        Effect::RemoveFromInventory {
            actor,
            def,
            durability: stack.durability,
        },
        Effect::AddGroundItem {
            pos: agent.pos,
            stack: *stack,
            dropped_at: world.clock,
            contents: Vec::new(),
            placed: true,
        },
    ]
}

/// 这一格立得下东西吗——[`resolve_place`] 的第 ②③④ 道前置。
///
/// 抽成函数而不是内联：它同时是「已经立着的是不是它」那条场地前置
/// （`resolve_craft` 第 ⑤ 步经
/// [`ll_world::state::WorldState::placed_at`]）的镜像，两者必须对同一个
/// 「一格至多一件放置物」的前提取一致的口径。第 ① 道前置（这件东西可
/// 不可以被放置）留在 `resolve_place` 里：它问的是**物品**，不是**格子**，
/// 与本函数的三条不是同一类判断。
pub(super) fn can_place_at(
    world: &WorldState,
    pos: TorusPos,
    space: &Space,
    ambient: AmbientSource<'_>,
) -> bool {
    // ② 这个空间允不允许建造。
    if !ambient.buildable_in(space) {
        return false;
    }
    // ③ 脚下这一格有没有被地形占着。
    if world
        .terrain_at(pos)
        .is_some_and(|kind| world.terrain_table.blocks_move(kind))
    {
        return false;
    }
    // ④ 这一格是不是已经立着一件东西。
    world.placed_at(pos).is_none()
}
