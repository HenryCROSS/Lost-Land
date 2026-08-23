//! 任务日志 UI 数据层——给定任务进度，返回当前任务日志的一份可展示
//! 数据结构（P5-B 任务 8）。
//!
//! # 明确边界：不含渲染
//!
//! 与 [`ll_sim::skill_overview`] 同一条纪律——本模块只交付
//! [`QuestLogView`] 这份纯数据结构与产出它的 [`build_quest_log_view`]，
//! 不涉及任何像素/字体/图集，是未来 P7 UI 层的直接消费对象。
//!
//! # 为什么落在 `ll-mod`，不是 `ll-sim`（与 `skill_overview` 不同的
//! 选择，如实记录两者为什么不对称）
//!
//! [`ll_sim::skill_overview::build_skill_tree_view`] 落在 `ll-sim`，
//! 是因为它只需要 `ContentIndex` 级别的信息（前置关系、已解锁集合），
//! 不需要任何字符串标识符。任务日志不同：它必须调用
//! [`crate::quest::unlocked_by`]（要求 `&QuestTable`，定义在本 crate）
//! 且要求"完成集合与 `unlocked_by` 消费的是同一批索引，不能自己另算
//! 一遍"（任务 8 的验收要求）——`unlocked_by` 只存在于 `ll-mod`，依赖
//! 方向不允许 `ll-sim` 反过来依赖它。本模块因此没有 `skill_overview`
//! 那样的依赖倒置空间可用：`QuestLogView` 的构建者必须待在能同时看见
//! `QuestTable`/`Registry`（反查任务的 `NamespacedId`，见
//! [`crate::quest::RegisteredQuests`] 文档同一个理由）与
//! `ll_sim::quest::is_quest_completed` 的地方——也就是 `ll-mod` 自己。

use ll_core::ident::ContentIndex;
use ll_sim::quest::is_quest_completed;
use ll_world::entity::Agent;

use crate::quest::{QuestTable, unlocked_by};
use crate::registry::Registry;

/// 任务日志当前状态的可展示数据结构——[`build_quest_log_view`] 的
/// 产出，P7 UI 层的直接消费对象。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestLogView {
    /// 已完成的任务节点，按 [`ContentIndex::get`] 升序（约束 C5，
    /// 继承自 [`QuestTable::defined_indices`] 的遍历顺序）。
    pub completed: Vec<ContentIndex>,
    /// 前置已满足、但尚未完成的任务节点——直接是
    /// [`unlocked_by`]`(quests, &completed)` 的返回值，不重新计算
    /// 一遍（任务 8 的验收要求：两者必须是同一个真相源）。
    pub unlocked_not_completed: Vec<ContentIndex>,
}

/// 给定一个 `Agent` 与它所属的任务表/注册表，返回它当前的任务日志。
///
/// `quests`/`registry` 必须出自同一次加载会话（`registry` 用来把
/// `QuestTable` 存的裸 `ContentIndex` 反查回
/// [`ll_sim::quest::is_quest_completed`] 需要的 `NamespacedId`）——与
/// [`crate::quest::RegisteredQuests`] 同一个前置条件。
pub fn build_quest_log_view(
    agent: &Agent,
    quests: &QuestTable,
    registry: &Registry,
) -> QuestLogView {
    let mut completed = Vec::new();
    for index in quests.defined_indices() {
        let Some(quest_id) = registry.resolve(index) else {
            // 索引反查失败（registry 与 quests 不是同一次加载产出）
            // 时静默跳过——与 RegisteredQuests::kill_count_quests 同一
            // 条 ADR 0015 纪律,不 panic。
            continue;
        };
        if is_quest_completed(agent, quest_id) {
            completed.push(index);
        }
    }
    let unlocked_not_completed = unlocked_by(quests, &completed);
    QuestLogView {
        completed,
        unlocked_not_completed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest::{QuestAttrs, QuestCondition, mark_quest_completed};
    use ll_sim::apply::apply;
    use ll_sim::effect::Effect;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::space::Space;
    use ll_world::state::WorldState;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;
    use std::collections::BTreeMap;

    fn test_world() -> WorldState {
        let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
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

    fn blank_agent(world: &WorldState) -> Agent {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(0, 0);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: ll_core::time::Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            skill_cooldowns: BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            mod_state: BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        }
    }

    /// 一张现造的、与本体内容无关的网状任务图：`root` 解锁
    /// `branch_a`/`branch_b` 两条分支。
    ///
    /// 本体那四条任务的定义已经搬进 `mods/lostland/quests.json5`，本模块
    /// 的单元测试验的是 [`build_quest_log_view`] 这套**机制**，不是
    /// 「本体有哪几条任务」——理由同 `crate::quest` 测试里的
    /// `sample_graph`。
    fn sample_graph() -> (Registry, QuestTable, [ContentIndex; 3]) {
        let mut registry = Registry::new();
        let mut table = QuestTable::new();
        let parse =
            |raw: &str| ll_core::ident::NamespacedId::parse(raw).expect("测试用标识符恒合法");
        let goblin = registry.intern(parse("testmod:goblin"));

        let define = |registry: &mut Registry,
                      table: &mut QuestTable,
                      raw: &str,
                      prerequisites: Vec<ContentIndex>| {
            let index = registry.intern(parse(raw));
            table
                .define(
                    index,
                    QuestAttrs {
                        prerequisites,
                        condition: QuestCondition::KillCount {
                            target_kind: goblin,
                            count: 1,
                        },
                    },
                )
                .expect("首次定义应当成功");
            index
        };

        let root = define(&mut registry, &mut table, "testmod:root", Vec::new());
        let branch_a = define(&mut registry, &mut table, "testmod:branch_a", vec![root]);
        let branch_b = define(&mut registry, &mut table, "testmod:branch_b", vec![root]);

        (registry, table, [root, branch_a, branch_b])
    }

    #[test]
    fn 未完成任何任务时只有起点任务出现在unlocked_not_completed() {
        // Arrange
        let (registry, table, _ids) = sample_graph();
        let world = test_world();
        let agent = blank_agent(&world);

        // Act
        let view = build_quest_log_view(&agent, &table, &registry);

        // Assert
        assert!(view.completed.is_empty());
        assert_eq!(
            view.unlocked_not_completed,
            unlocked_by(&table, &[]),
            "QuestLogView 与 quest::unlocked_by 的结果必须一致,不能自己另算一遍"
        );
    }

    #[test]
    fn 完成起点任务后两条分支同时出现在unlocked_not_completed() {
        // Arrange：root 完成后,branch_a/branch_b 应该同时可见——网状
        // 结构的直接验收（一个前置解锁多个后续）。
        let (registry, table, [root, branch_a, branch_b]) = sample_graph();
        let mut world = test_world();
        let actor = world.actors.spawn(blank_agent(&world));
        let quest_id = registry.resolve(root).expect("root 已注册").clone();
        apply(
            &mut world,
            &Effect::SetModState {
                writes: vec![mark_quest_completed(actor, &quest_id)],
            },
        );
        let agent = world.actors.get(actor).expect("刚生成的实体必然存在");

        // Act
        let view = build_quest_log_view(agent, &table, &registry);

        // Assert
        assert_eq!(view.completed, vec![root]);
        assert_eq!(view.unlocked_not_completed, vec![branch_a, branch_b]);
        assert_eq!(
            view.unlocked_not_completed,
            unlocked_by(&table, &view.completed)
        );
    }
}
