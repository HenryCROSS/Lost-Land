//! 归属在**决策层**的一半：拾取即归属，以及盗窃判定将来的挂载点。
//!
//! 类型本体（[`Owner`] 五个变体连同全部论证）住在
//! [`ll_world::ownership`]；本模块只放「什么时候归属会变」这条**判定**。
//! 落地 `knowledge/design/ownership-and-crime-detection.md` 二节 2.1
//! 那个挂载点的**前半**。
//!
//! # 为什么单独一个模块，不写进 `crate::resolve`
//!
//! `resolve.rs` 落地本模块时已近 8000 行（全仓最严重的既有文件行数
//! 违规，`2026-08-28-session-handoff.md` 四节第 8 条已登记）。归属判定
//! 将来只会长大——盗窃判定、目击、赃物标记全都要挂在同一个函数上——
//! 往那个文件里加只会让它更糟。本模块与 `resolve` 的接口只有一个纯
//! 函数，拆开零代价。
//!
//! 本模块仍落在字段接线门禁（`scripts/ci/check_field_consumers.py`）的
//! 决策层通配 `crates/ll-sim/src/*.rs` 内——这不是巧合，是选它作落点的
//! 理由之一：`ItemStack.owner` 的真实读者必须住在决策层，否则那个字段
//! 就是第十五个死字段。

use ll_world::entity::{Agent, EntityId};
use ll_world::item::ItemStack;
use ll_world::ownership::Owner;
use ll_world::state::WorldState;

/// 这一堆地面物品被 `actor` 捡起来之后，归属该变成什么。
///
/// # 所有者的裁定
///
/// > Owner ……也可以默认不归属于谁然后谁拿了就变成谁的。
///
/// 因此判据只有一条：**无主物被拾起后归拾取者**。
///
/// | 被捡的这堆现在的归属 | 拾取者 | 结果 |
/// |---|---|---|
/// | [`Owner::Unowned`] | 玩家 | [`Owner::Player`] |
/// | [`Owner::Unowned`] | 有 `remembered_id` 的 NPC | [`Owner::Npc`] |
/// | [`Owner::Unowned`] | 没有 `remembered_id` 的 NPC | **仍是 `Unowned`**，见下 |
/// | 任何**有主**的归属 | 任何人 | **原样不变**，见下 |
///
/// # 有主物被捡走时归属原样不变——这不是遗漏
///
/// 设计文档三节 3.3 表格第一行写死了这条：一次盗窃发生时
/// **`owner` 字段本身不变**（仍是原主人的），变的是另外挂一个
/// `stolen_marker`；直到销赃计时走完那一刻才真正改写 `owner`。
/// 这样犯罪记录要读的"原主人是谁"随时可从 `owner` 本身读出，不需要
/// 额外查历史事件。
///
/// 本批次**不落地** `stolen_marker`、不落地盗窃判定，因此这条分支现在
/// 只是「原样返回」——但它返回的值与设计文档要求的**完全一致**，犯罪
/// 批次要做的是在这里**追加**一个赃物标记，不是推翻这一行。
///
/// # 挂载点：盗窃判定进这个函数，不是进 `resolve_pick_up`
///
/// 设计文档 2.1 指定的挂载点是 `resolve_pick_up`；本函数是它抽出来的
/// 那一半，判定需要的全部输入都已经在参数里：
///
/// - `world`——目击判定要遍历 `world.actors`、要读 `world.clock`；
/// - `agent`/`actor`——肇事者是谁、站在哪；
/// - `picked`——被拿走的是什么、原本归谁。
///
/// 犯罪批次要加的三件事都落在这里：①「这次拾取算不算盗窃」的判据
/// （设计文档 2.1）、② `stolen_marker` 的**唯一**写入点（三节 3.3：
/// 只在 `None → Some` 那一刻写一次，此后任何转手都不得覆写）、
/// ③ `lostland:public_use` 标签的放行（六节）。目击判定（`witnessed_by`）
/// 与犯罪记录（`record_crime`）不进本函数——它们不只服务盗窃，是给
/// 全部 `CrimeKind` 共用的帮手，见设计文档 2.7。
///
/// # 为什么无名 NPC 捡到的东西继续无主
///
/// [`Owner::Npc`] 的载荷是
/// [`WorldId`](ll_core::ident::WorldId)（设计文档 1.2 的修正一），来源
/// 是 [`Agent::remembered_id`]——那是个**懒分配**的字段，今天唯一的
/// 真实分配点是死亡路径。给一个活着的无名 NPC 分配一个需要
/// `WorldState::remembered_id_of_or_assign`，而那是 `&mut self`：
/// `resolve` 只有 `&WorldState`（约束 C1），做不到。
///
/// 出路是新开一个 `Effect` 让 `apply` 去分配。**本批次不做**：那个
/// `Effect` 的唯一价值是给犯罪判定服务（要能追究一个 NPC，得先能称呼
/// 他），而犯罪判定整批都不在本批次范围内——现在加就是一个没有真实
/// 后果的机制。本批次取最保守的降级：无名 NPC 捡到的东西继续无主，
/// **这不会丢失任何东西**（物品照常进背包，数量守恒），只是它此刻没有
/// 一个可以永久称呼的主人。犯罪批次接线时改这里一处即可。
pub fn pick_up_owner(
    world: &WorldState,
    agent: &Agent,
    actor: EntityId,
    picked: ItemStack,
) -> Owner {
    if picked.owner.is_claimed() {
        return picked.owner;
    }
    holder_owner(world, agent, actor)
}

/// 「东西到了 `actor` 手上之后归谁」——[`pick_up_owner`] 与对话赠送
/// （`crate::resolve` 的 `DialogueOutcome::GiveItem` 一支）共用的那一半。
///
/// 玩家是 [`Owner::Player`]；有 `remembered_id` 的 NPC 是
/// [`Owner::Npc`]；无名 NPC 是 [`Owner::Unowned`]（理由见
/// [`pick_up_owner`] 文档「为什么无名 NPC 捡到的东西继续无主」一节，
/// 那条降级对赠送逐字同样成立）。
///
/// # 为什么赠送不直接调 [`pick_up_owner`]
///
/// 那个函数的第一句是「原本就有主的东西保持原主」——那是**拾取**的
/// 语义（也是将来盗窃判定的挂载点：捡走别人的东西不会让它变成你的）。
/// 赠送是一次**合法转移**：说话人已经通过了 owner 校验硬前置，东西
/// 转手之后就归收方。两者在这一点上判据相反，共用会让将来的盗窃判定
/// 把每一次赠送也标成赃物。共用的只有这一半——「谁拿到手就是谁的」
/// 这条映射本身，那正是 ADR 0021 说的「共享算法」。
pub fn holder_owner(world: &WorldState, agent: &Agent, actor: EntityId) -> Owner {
    if world.player_entity == Some(actor) {
        return Owner::Player;
    }
    match agent.remembered_id {
        Some(id) => Owner::Npc(id),
        None => Owner::Unowned,
    }
}

/// **owner 校验硬前置**：这一堆东西，`giver` 交得出去吗？
///
/// `ll_world::ownership` 的设计文档四节给「合法转移」（赠送 / 购买 /
/// 任务发奖）立的那条前置，[`ll_sim::effect::Effect::TransferOwnership`](crate::effect::Effect::TransferOwnership)
/// 的文档逐字转述过一遍：
///
/// > 三种合法转移的 `resolve` 都**必须**校验「发起转移的一方确实是这堆
/// > 物品当前的 `owner`」（`Owner::Unowned` 除外，因为没有人的权益受损）。
/// > 不满足则不产出效果。
///
/// | 这一堆的归属 | 交得出去吗 | 理由 |
/// |---|---|---|
/// | [`Owner::Unowned`] | ✅ | 没有人的权益受损（效果文档原话） |
/// | [`Owner::Player`]，且交出方就是玩家 | ✅ | 是他自己的 |
/// | [`Owner::Npc`]，且号就是交出方的 | ✅ | 是他自己的 |
/// | 以上之外的任何有主归属 | ❌ | 不是他的 |
/// | [`Owner::Faction`] / [`Owner::Shop`] | ❌ | **公产，本批一律拒**，见下 |
///
/// `giver` 由**既有的** [`holder_owner`] 算出来——「这个实体名下的东西
/// 长什么样」只有那一处真相源，本函数不重新推一遍。
///
/// # `Owner::Faction` / `Owner::Shop` 一律拒
///
/// 「管理者能不能把据点的公产发给你」「店铺的货算不算店主的」都是玩法
/// 裁定，规格没写。不做是最保守、最容易反转的一档（反转成本是这张表加
/// 一行 + 一条「他属于那个势力吗」的查询）。这条是批次 29 第 4 条临时
/// 裁定，本批原样继承。
///
/// # 无名 NPC 名下的东西
///
/// `remembered_id` 是懒分配的（今天唯一的真实分配点是死亡路径），因此
/// 一个活着的 NPC 通常得到 `Owner::Unowned` 这个 `giver`——那时任何
/// `Owner::Npc(_)` 都不是「可证明属于他的」，一律拒。这与
/// [`pick_up_owner`] 那条「无名 NPC 捡到的东西继续无主」是同一处既有
/// 降级的两面：今天 NPC 的背包里全是 `Owner::Unowned` 的出生装备，
/// 那一档照常交得出去。
///
/// # 两个调用方，一份判据（ADR 0021）
///
/// 〔2026-08-31，批次 29〕本函数最初住在
/// `crates/ll-sim/src/resolve/dialogue.rs`，是那一批 `give-item` 的私有
/// 函数，入参是 `Option<WorldId>`（因为交出方恒是 NPC）。
///
/// 〔2026-09-01，批次 31〕交易是它的**第二个调用方**，而交易里交出方
/// 可能是玩家。**搬到这里并把入参泛化成 [`Owner`]，而不是在交易那一侧
/// 另写一份** ——两份判据分叉时没有任何东西会报错，正是 ADR 0021 点名
/// 要拦的形状。赠送那条路径的行为逐条不变：交出方是 NPC，
/// [`holder_owner`] 给的正是旧签名里那个 `Option<WorldId>` 的两种情形。
/// 唯一放宽的是「交出方恰好是玩家」这一格（旧实现恒拒，因为它写死了
/// 交出方不可能是玩家），泛化之后那一格是**正确**的那一档。
pub fn may_give_away(giver: Owner, owner: Owner) -> bool {
    match owner {
        Owner::Unowned => true,
        Owner::Player => giver == Owner::Player,
        Owner::Npc(id) => giver == Owner::Npc(id),
        Owner::Faction(_) | Owner::Shop(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{ContentIndex, Interner, NamespacedId};
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_world::entity::BaseStats;
    use ll_world::generate::GenParams;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    fn index(raw: &str) -> ContentIndex {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    /// 一个单区块的测试世界——与 `crate::resolve` 的同名夹具同一形状。
    fn test_world() -> WorldState {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    /// 造一个占位实体。本模块的判定只读 `remembered_id` 与
    /// `world.player_entity`，其余字段取占位值。
    fn spawn_agent(world: &mut WorldState) -> EntityId {
        let pos = world.size.wrap(5, 5);
        let zone = world.terrain.layout().tile_to_zone(pos).0;
        world.actors.spawn(Agent {
            gender: ll_world::entity::Gender::default(),
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: index("lostland:tester"),
            goals: Vec::new(),
            race: index("lostland:human"),
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
            home: None,
        })
    }

    #[test]
    fn 玩家捡起无主物之后它归玩家() {
        // Arrange
        let mut world = test_world();
        let player = spawn_agent(&mut world);
        world.player_entity = Some(player);
        let stack = ItemStack::new(index("lostland:iron_ingot"), 3);
        assert_eq!(stack.owner, Owner::Unowned, "夹具前提：地上的东西无主");

        // Act
        let agent = world.actors.get(player).expect("刚 spawn 的实体必然在");
        let owner = pick_up_owner(&world, agent, player, stack);

        // Assert
        assert_eq!(owner, Owner::Player);
    }

    #[test]
    fn 有名字的npc捡起无主物之后它归那个npc() {
        // Arrange
        let mut world = test_world();
        let npc = spawn_agent(&mut world);
        let assigned = world
            .remembered_id_of_or_assign(npc)
            .expect("给存在的实体分配 remembered_id 必然成功");
        let stack = ItemStack::new(index("lostland:iron_ingot"), 1);

        // Act
        let agent = world.actors.get(npc).expect("刚 spawn 的实体必然在");
        let owner = pick_up_owner(&world, agent, npc, stack);

        // Assert
        assert_eq!(owner, Owner::Npc(assigned));
    }

    #[test]
    fn 没名字的npc捡起无主物之后它仍然无主() {
        // resolve 只有 &WorldState（C1），给无名 NPC 懒分配
        // remembered_id 需要 &mut self，做不到——本批次取的降级。
        // Arrange
        let mut world = test_world();
        let npc = spawn_agent(&mut world);
        let stack = ItemStack::new(index("lostland:iron_ingot"), 1);

        // Act
        let agent = world.actors.get(npc).expect("刚 spawn 的实体必然在");
        assert_eq!(agent.remembered_id, None, "夹具前提：这个 NPC 还没有名字");
        let owner = pick_up_owner(&world, agent, npc, stack);

        // Assert
        assert_eq!(owner, Owner::Unowned);
    }

    #[test]
    fn 端到端玩家捡起地上的箭之后进背包的那一堆归玩家() {
        // 端到端：走真正的 Intent::PickUp → resolve → 效果序列，确认
        // 拾取即归属真的接在生产路径上，而不只是那个纯函数自己对。
        // Arrange
        let mut world = test_world();
        let player = spawn_agent(&mut world);
        world.player_entity = Some(player);
        let pos = world.actors.get(player).expect("刚 spawn 必然在").pos;
        let arrow = index("lostland:arrow");
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(arrow, 12),
            dropped_at: ll_core::time::Tick(0),
            contents: Vec::new(),
            placed: false,
        });

        // Act
        let effects = crate::resolve::resolve(
            &world,
            &crate::intent::Intent::PickUp {
                actor: player,
                pos: (pos.x(), pos.y()),
                def: arrow,
            },
        );

        // Assert
        let merged = effects
            .iter()
            .find_map(|effect| match effect {
                crate::effect::Effect::MergeIntoInventory { resulting, .. } => Some(resulting),
                _ => None,
            })
            .expect("拾取必然产出一条 MergeIntoInventory");
        assert_eq!(merged.len(), 1, "背包本来是空的，只会多出一堆");
        assert_eq!(
            merged[0].owner,
            Owner::Player,
            "进背包的那一堆应当已经归玩家"
        );
        assert_eq!(merged[0].count, 12, "数量守恒");
    }

    #[test]
    fn 有主物被别人捡走时归属原样不变() {
        // 设计文档三节 3.3 表格第一行：盗窃发生时 owner 字段本身不变，
        // 变的是（本批次尚未落地的）赃物标记。这一条现在就要正确，否则
        // 犯罪批次接线时会发现原主人已经被抹掉、无从追认。
        // Arrange
        let mut world = test_world();
        let thief = spawn_agent(&mut world);
        world.player_entity = Some(thief);
        let villager = spawn_agent(&mut world);
        let victim_id = world
            .remembered_id_of_or_assign(villager)
            .expect("给存在的实体分配 remembered_id 必然成功");
        let stack = ItemStack {
            owner: Owner::Npc(victim_id),
            ..ItemStack::new(index("lostland:iron_sword"), 1)
        };

        // Act
        let agent = world.actors.get(thief).expect("刚 spawn 的实体必然在");
        let owner = pick_up_owner(&world, agent, thief, stack);

        // Assert
        assert_eq!(
            owner,
            Owner::Npc(victim_id),
            "玩家拿走村民的东西，账面上它仍然是村民的——改写归属是销赃那一刻的事"
        );
    }
}
