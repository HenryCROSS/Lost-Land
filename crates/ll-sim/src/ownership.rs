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
    if world.player_entity == Some(actor) {
        return Owner::Player;
    }
    match agent.remembered_id {
        Some(id) => Owner::Npc(id),
        None => Owner::Unowned,
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
