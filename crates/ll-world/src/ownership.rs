//! 物品归属——落地 `knowledge/design/ownership-and-crime-detection.md`
//! 一节「`Owner` 的形状」。
//!
//! # 为什么单独一个模块，不写在 [`crate::item`] 里
//!
//! `item.rs` 落地时已经 1294 行，规格 §13 的上限是 800——把一个新类型
//! 连同它的完整论证再塞进去只会让既有违规更深。本模块与
//! [`crate::item::ItemStack`] 的耦合只有一个字段
//! （[`ItemStack::owner`](crate::item::ItemStack::owner)），拆开没有
//! 任何来回引用的代价。
//!
//! # 这一批落地了什么、没落地什么
//!
//! 设计文档八节把「现在能做的」列了六条，本批次只认领与**归属本身**
//! 有关的那一半：
//!
//! - ✅ [`Owner`] 类型本体（五变体，含设计文档 1.2/1.3 两条修正）
//! - ✅ `ItemStack.owner` 字段、[`crate::item::can_merge`] 追加比较
//! - ✅ 拾取即归属（`ll_sim::resolve` 的 `resolve_pick_up`）
//! - ✅ `Effect::TransferOwnership`（合法转移的接口形状，调用方不存在）
//! - ❌ `StolenMarker`（销赃计时）、目击判定、`HistoricalEventKind::Crime`
//!   ——设计文档二、三、五节，整体归犯罪判定批次。它们只服务「盗窃」
//!   这一件事，提前落地就是又一批没有消费者的死字段。
//!
//! **盗窃判定的挂载点**留在 `ll_sim::resolve::pick_up_owner` 那个函数
//! 里（本 crate 不能引用它，依赖方向不允许，这里只点名），见该函数
//! 文档。
//!
//! # 与 `item-system.md` 三节原文的两处不同
//!
//! 设计文档一节的 1.2/1.3 两条修正**已在本模块采纳**，理由不在这里
//! 重复论证（见该文档），只记结论：
//!
//! | 变体 | `item-system.md` 原文 | 本模块 | 一句话理由 |
//! |---|---|---|---|
//! | `Npc` | `EntityId` | [`WorldId`] | `EntityId` 在 `despawn` 后故意失效，而「这原本是谁的」必须在主人死后仍读得出 |
//! | `Faction` | `ContentIndex` | [`WorldId`] | 与 [`OrgRef::Instance`](crate::entity::OrgRef::Instance) 对齐，势力是世界生成期产出的实例，不是装载期确定的类型 |

use ll_core::ident::WorldId;
use serde::{Deserialize, Serialize};

use crate::entity::EntityId;

/// 一堆物品现在归谁——[`crate::item::ItemStack::owner`] 的类型。
///
/// # 全部由 `Copy` 类型组成，因此 `ItemStack` 保持 `Copy`
///
/// [`WorldId`]/[`EntityId`] 都已 `#[derive(Copy)]`，本枚举因此也是
/// `Copy`——这是一条必须维持的性质：`ItemStack` 现在被
/// [`crate::item::merge_stacks`]/[`crate::item::split_stack`] 按值搬来
/// 搬去，一旦某个变体带上堆分配的载荷，那两个函数与它们的全部调用点
/// 都要跟着改签名。
///
/// # 默认值是 [`Owner::Unowned`]，这不改变任何现有行为
///
/// 设计文档 1.5 原文：现有代码里构造地面物品的每一处隐含的语义都是
/// 「这堆东西没有主张归属的机制」，加这个字段只是把这条隐含语义显式
/// 化。因此 `#[derive(Default)]` + `#[default]` 落在 `Unowned` 上，
/// 存档也走 `#[serde(default)]`（见 `ItemStack::owner` 字段文档）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Owner {
    /// 无主——野外掉落、怪物尸体、任何「没有人对这堆东西主张所有权」
    /// 的物品。设计文档 1.5。
    ///
    /// **本批次唯一会被大量构造的变体**：世界生成、出生装备、制作产物、
    /// 尸体与遗物全部落在这里。
    #[default]
    Unowned,
    /// 玩家的。由「拾取即归属」产出（`ll_sim::resolve` 的
    /// `pick_up_owner`：拾取者是 `world.player_entity` 时）。
    ///
    /// # 为什么不是 `Npc(玩家的 remembered_id)`
    ///
    /// 玩家是唯一一个「不由世界生成产出、不进历史家族族谱、换角色之后
    /// 换一个实体但仍然是同一个玩家」的主体——把他挤进 `Npc` 这一支，
    /// 每一处判定都要先问一句「这个 `WorldId` 是不是玩家的」，而那个
    /// 问题今天没有稳定答案（死后换角色，见存档批次）。独立一个变体
    /// 让「这是玩家的东西」成为一次模式匹配，与
    /// `WorldState::player_entity` 是 `Option<EntityId>` 而不是塞进
    /// `actors` 里靠标志位区分是同一条既有取舍。
    Player,
    /// 某个具名 NPC 的私产。
    ///
    /// 载荷是 [`WorldId`] 而不是 [`EntityId`]（设计文档 1.2 的修正一）：
    /// `EntityId` 依赖 [`Arena`](crate::entity::Arena) 的世代号，实体
    /// `despawn` 之后旧句柄**故意**失效；而物品不该因为主人死亡就变得
    /// 「归属不可判断」——继承、随从叛离、犯罪记录追溯都需要在主人死后
    /// 仍能读出「这原本是谁的」。[`crate::history::KillRecord`] 的
    /// `killer`/`victim` 已经用 `WorldId` 解决过同一个问题。
    ///
    /// # 前置：这个 NPC 必须先有 `remembered_id`
    ///
    /// [`Agent::remembered_id`](crate::entity::Agent::remembered_id) 是
    /// 懒分配的，今天唯一的真实分配点是死亡路径。要给一个活着的 NPC
    /// 的东西打上本变体，得先经
    /// [`WorldState::remembered_id_of_or_assign`](crate::state::WorldState::remembered_id_of_or_assign)
    /// 分配一个——那是 `&mut self`，只有 `apply` 侧能做（约束 C1）。
    /// 本批次因此**只在拾取者已经有 `remembered_id` 时**才产出本变体，
    /// 无名 NPC 捡到的东西继续是 [`Owner::Unowned`]，见 `ll_sim::resolve`
    /// 的 `pick_up_owner` 文档。
    Npc(WorldId),
    /// 某个**组织实例**的公产——势力（P9 播种后）与**据点**。
    ///
    /// 载荷是 [`WorldId`] 而不是 [`ContentIndex`](ll_core::ident::ContentIndex)
    /// （设计文档 1.3 的修正二）：
    /// [`OrgRef`](crate::entity::OrgRef) 已经把六类归属分成
    /// `Def(ContentIndex)`（文化、职业——装载期确定的类型）与
    /// `Instance(WorldId)`（势力、宗教、行会、家族——世界生成期产出的
    /// 具体个体）两条轨道，势力明确落在 `Instance` 一侧。若这里继续用
    /// `ContentIndex`，「物品的势力归属」与「角色对势力的归属」会指向
    /// 同一个东西却用两套不相容的引用类型。
    ///
    /// # 「据点归属」用的就是本变体（本批次的裁定）
    ///
    /// 所有者原话：「一个建筑内的物品通常都是属于某个人的」。但**「住
    /// 在那儿的人」这个关系今天不存在**——
    /// [`stamp_settlement`](crate::settlement::stamp_settlement) 只盖楼、
    /// `ll_mod::roster` 的 `place_roster` 只摆人，建筑与居民之间零关联。
    ///
    /// 技术裁定：**先按据点归属**，表示法是
    /// `Owner::Faction(SettlementSite::id)`。三条理由：
    ///
    /// 1. 五个变体里只有本变体指向一个**集体**——`Player`/`Npc` 是自然
    ///    人，`Shop` 是商业设施，`Unowned` 是没有主张。「这是这座据点的
    ///    东西」是一次集体主张。
    /// 2. 载荷类型天然对齐：
    ///    [`SettlementSite::id`](crate::settlement::SettlementSite::id)
    ///    的字段文档原文是「永久标识，与历史事件、势力、家族共用
    ///    `WorldId` 空间」——据点 id 是这个空间里一个合法的值，不需要
    ///    任何换算，也不依赖 P9 的势力播种。
    /// 3. 它是一次**可收窄的加宽**：建筑↔居民关系落地后（据点建筑批次
    ///    的自然产物），家具归属从「这座据点的」细化成「住这儿的那个
    ///    NPC 的」，即本变体 → [`Owner::Npc`]，同一个字段换一个变体，
    ///    不动任何结构。
    ///
    /// **代价，如实记录**：本变体的名字此后承载两类 `WorldId`——势力
    /// 实例与据点。二者在 `WorldId` 空间里全局唯一、不会互相误认，但
    /// 名字不再逐字对应它装的东西。备选是加第六个变体
    /// `Settlement(WorldId)`；没选它是因为设计文档 1.1 明确「五变体已经
    /// 是 `item-system.md` 定型过的形状，未来系统都会对齐它」，本批次
    /// 同样没有权限单方面给别的系统将来要用的枚举加变体。**本批次不摆
    /// 家具**（那属据点建筑批次），因此今天没有任何一处真的构造本变体
    /// ——这条裁定要反转的话，只需要加变体、改一处将来才会写的赋值点。
    ///
    /// # 势力播种落地了：本变体第一次有真实的势力号可指（2026-08-29）
    ///
    /// 上面那句「势力（P9 播种后）」的前提已经变了。势力播种从 P9 撤出、
    /// 作为独立批次落地（交接文档第〇之二第 3 条与「第 3 条的后果」一节），
    /// [`crate::faction::FactionTable`] 现在住在
    /// [`crate::state::WorldState::factions`] 里，每个
    /// [`crate::faction::Faction`] 都有一个从编年史计数器分配的、与据点号
    /// **永不相等**的 `WorldId`。
    ///
    /// **本批次不构造任何 `Owner::Faction`**——它今天仍然零构造点，谁来
    /// 构造是「据点建筑/家具摆放」那一批的事。写在这里的只是**衔接说明**：
    ///
    /// - 从此有两种合法载荷可选：真正的势力号（`Faction::id`）与据点号
    ///   （`SettlementSite::id`）。上面那段「代价，如实记录」说的「本变体
    ///   的名字此后承载两类 `WorldId`」因此**仍然成立**，只是第二类不再
    ///   是权宜之计。
    /// - **两者之间的换算今天就有**：
    ///   [`crate::faction::FactionTable::faction_of`] 把据点号翻译成势力号。
    ///   摆家具那一批可以直接选「这是这座据点的」或「这是统治它的那个势力
    ///   的」，不需要再新增任何类型。
    /// - **本批次刻意不改本变体的语义**，一个字都没动：物品归属那批刚
    ///   落地，改它属于另一次裁定。
    ///
    /// # 本变体不再是零构造点了（据点建筑类型批次，2026-08-31）
    ///
    /// 上面两处「**本批次不摆家具**……因此今天没有任何一处真的构造本
    /// 变体」与「**本批次不构造任何 `Owner::Faction`**——它今天仍然零
    /// 构造点」两句话**原文保留**（追溯用），在这里更正：
    ///
    /// - **构造点**：`ll_game::settlement_spawn::furnish_settlement`
    ///   （本 crate 不能引用它，依赖方向不允许，这里只点名）。一座据点
    ///   物化时，它屋里的每一件家具在**构造 `ItemStack` 的那一刻**就
    ///   带上 `Owner::Faction(SettlementSite::id)`——不是事后回填。
    /// - **载荷选的是据点号，不是势力号**：上面那段衔接说明列出的两种
    ///   合法载荷里，摆家具那一批选了前者。理由是「一个势力下属多座
    ///   据点」——势力号比「这是这座据点的东西」**更宽**，而本变体文档
    ///   自己写明的收窄方向是 `Faction(据点) → Npc(住这儿的那个人)`，
    ///   用势力号会让那条收窄多绕一层。`FactionTable::faction_of` 随时
    ///   把据点号翻成势力号，反过来不行。
    /// - **「代价，如实记录」那一段仍然成立**：本变体的名字此后确实
    ///   承载两类 `WorldId`，而且现在**真的两类都可能出现**。
    ///
    /// 落地计划：`docs/superpowers/plans/2026-08-31-batch20-buildings.md`。
    Faction(WorldId),
    /// 商店库存。**设计文档 1.4 明确标注为待定，本批次原样保留、零
    /// 构造点。**
    ///
    /// 商店到底是「具名 NPC 摆摊」（那 [`Owner::Npc`] 就够用，本变体
    /// 可以不存在）还是「独立于任何角色的建筑设施」（那需要一个
    /// `StructureId` 之类的类型，而 `society-and-affiliation.md` 里的
    /// `StructureKind` 已在 2026-08-26 复核里被记成**已被否决**），
    /// 属经济/社会文档的地盘，本模块不代其发言。载荷因此也原样保留
    /// [`EntityId`]——改它需要先知道商店是什么，而不是反过来。
    Shop(EntityId),
}

impl Owner {
    /// 这堆东西有没有人主张所有权——`self != Unowned`。
    ///
    /// 抽成方法而不是让每个调用点各写一次 `!matches!(owner,
    /// Owner::Unowned)`：「有主/无主」这条二分是归属体系里唯一一条
    /// **对全部变体一视同仁**的判据（拾取即归属看它、将来的盗窃判定
    /// 看它、合法转移的「无主物谁都能转移」也看它），三处各写一遍
    /// `matches!` 正是 ADR 0021 点名要拦的重复实现。
    pub const fn is_claimed(self) -> bool {
        !matches!(self, Owner::Unowned)
    }
}

/// `apply` 侧的归属写入——设计文档四节
/// 「`apply` 侧对应的 `WorldState` 方法形状」。
///
/// # 为什么这个 `impl` 块在本模块，不在 `crate::state`
///
/// `state.rs` 已经 3400 行（规格 §13 的上限是 800，见
/// `2026-08-28-session-handoff.md` 四节第 8 条的既有违规登记）。Rust
/// 允许在同一 crate 的任意模块里给一个类型开 `impl` 块，归属的写入
/// 逻辑与归属类型放在一起读起来也更完整——`record_kill` 那类方法留在
/// `state.rs` 是历史原因，不是必须。
impl crate::state::WorldState {
    /// 把 `holder` 背包里第一条匹配 `(def, durability)` 的堆的归属改成
    /// `new_owner`；找不到就什么都不做，返回 `false`。
    ///
    /// # 机械执行，不做任何判断（约束 C1）
    ///
    /// 「这次转移合不合法」（发起方是不是当前 `owner`、有没有付钱、
    /// 任务完成没有）全部属于 `resolve`——本方法只做一次按键查找 + 一次
    /// 字段写入，与
    /// [`Self::apply`](crate::state::WorldState) 侧其余
    /// `Effect` 的写入者同一条既有分工。前置约束的完整文字见
    /// `ll_sim::effect::Effect::TransferOwnership` 文档（本 crate 不能
    /// 引用它，依赖方向不允许，这里只点名）。
    ///
    /// # 为什么返回 `bool` 而不是静默
    ///
    /// 「背包里没有这一堆」在正常路径上不可能发生（`resolve` 刚读到过
    /// 它）；真发生了说明同一批效果里有更早的一条把它移走了，那是一个
    /// 值得测试断言的事实。返回值让测试能直接钉住"确实改到了一堆"，
    /// 而不是靠事后翻背包。生产路径上调用方可以忽略它。
    ///
    /// # 定位到第一条匹配，不是全部
    ///
    /// 与 `Effect::RemoveFromInventory` 的 `apply` 实现逐字同一条纪律
    /// （`position` + 单条改写）：背包里理论上不会有两堆
    /// `(def, durability, owner)` 完全相同的东西（那两堆满足
    /// [`can_merge`](crate::item::can_merge)，早该合并成一堆），真出现
    /// 时改第一条与改哪一条没有可观察差异。
    pub fn transfer_item_ownership(
        &mut self,
        holder: crate::entity::EntityId,
        def: ll_core::ident::ContentIndex,
        durability: Option<i32>,
        new_owner: Owner,
    ) -> bool {
        let Some(agent) = self.actors.get_mut(holder) else {
            return false;
        };
        let Some(stack) = agent
            .inventory
            .iter_mut()
            .find(|stack| stack.def == def && stack.durability == durability)
        else {
            return false;
        };
        stack.owner = new_owner;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用帮手：从一个一次性计数器里取一个 [`WorldId`]——
    /// [`WorldId::next`] 是这个类型唯一的构造出口（没有 `from_raw`），
    /// 与 `crate::history` 的测试同一条既有手法。
    fn world_id(raw: u32) -> WorldId {
        let mut counter = raw;
        WorldId::next(&mut counter)
    }

    /// 测试用帮手：拼一个 [`EntityId`]——`EntityId::new` 是
    /// `pub(crate)`，本 crate 内的测试可以直接用，crate 外只能从
    /// `spawn` 拿。
    fn entity_id(index: u32, generation: u32) -> EntityId {
        EntityId::new(index, generation)
    }

    #[test]
    fn 默认归属是无主() {
        // 设计文档 1.5：加这个字段只是把「现有地面物品没有主张归属的
        // 机制」这条隐含语义显式化，默认值必须是无主，否则会凭空给
        // 世界上每一件既有物品安一个主人。
        assert_eq!(Owner::default(), Owner::Unowned);
    }

    #[test]
    fn 只有无主不算被主张() {
        // Arrange：五个变体各取一个代表。
        let owner_id = world_id(7);
        let entity = entity_id(0, 1);

        // Act + Assert
        assert!(!Owner::Unowned.is_claimed());
        assert!(Owner::Player.is_claimed());
        assert!(Owner::Npc(owner_id).is_claimed());
        assert!(Owner::Faction(owner_id).is_claimed());
        assert!(Owner::Shop(entity).is_claimed());
    }

    #[test]
    fn 同一个裸数值在npc与faction两支上不相等() {
        // 两个变体的载荷都是 WorldId（设计文档 1.2/1.3 的两条修正），
        // 判别式必须真的参与相等比较——否则「张三的东西」与「张三所在
        // 据点的公产」会被判成同一个归属，can_merge 会把两堆本不该合并
        // 的东西合起来。
        let id = world_id(42);
        assert_ne!(Owner::Npc(id), Owner::Faction(id));
    }

    #[test]
    fn 归属往返序列化() {
        // 归属进存档主体（ItemStack 的一个字段），五个变体都要能原样
        // 回来——带载荷的三个尤其：判别式与载荷任一丢失都会让读回来的
        // 世界里物品换了主人。
        let owner_id = world_id(9);
        for owner in [
            Owner::Unowned,
            Owner::Player,
            Owner::Npc(owner_id),
            Owner::Faction(owner_id),
            Owner::Shop(entity_id(3, 2)),
        ] {
            let json = serde_json::to_string(&owner).expect("Owner 全部变体可序列化");
            let back: Owner = serde_json::from_str(&json).expect("刚写出来的 JSON 必然读得回");
            assert_eq!(back, owner, "{owner:?} 序列化往返后变了");
        }
    }
}
