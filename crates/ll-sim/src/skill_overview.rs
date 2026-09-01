//! 技能树 UI 数据层——给定一个 `Agent`，返回技能树当前状态的一份可
//! 展示数据结构（P5-B 任务 8）。
//!
//! # 两个消费者：技能树视图与「学会一个技能」的前置判定
//!
//! [`SkillTreeCatalog`] 起初只服务 [`build_skill_tree_view`]。升级
//! 加点批次落地 `Intent::LearnSkill` 之后，`crate::resolve` 的
//! `resolve_learn_skill` 是它的第二个消费者——**刻意共用同一个目录、
//! 同一条前置规则**（前置技能全部在已解锁集合里）：面板上显示为
//! 「可解锁」的技能，就是那里学得会的技能，两处不会漂移。
//!
//! # 明确边界：不含渲染，不碰 `ll-render`/`ll-ui`
//!
//! `ll-ui` 完整像素控件库排在 P7（规格 §15 已明确）。本模块只交付
//! [`SkillTreeView`] 这一份纯数据结构与产出它的
//! [`build_skill_tree_view`]，不涉及任何像素/字体/图集——这份数据是
//! 未来 P7 UI 层的直接消费对象，不是本批次的职责。
//!
//! # 为什么落在 `ll-sim`，不是计划草案写的 `ll-world`
//!
//! P5-B 计划文档（任务 8）给出的落点是
//! `crates/ll-world/src/overview.rs` 或新增 `skill_overview.rs`，并
//! 明确标注"视实现时判断哪个更合适"。真实架构不允许这个选择：技能
//! 定义（`SkillDef`/`SkillTable`）按任务 2/3 的设计判断落在 `ll-mod`
//! （见其模块文档「为什么定义本身直接落在 `ll-mod`」一节），依赖方向
//! `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`（规格 §5）不允许
//! `ll-world` 反过来认识任何技能相关的类型——`build_skill_tree_view`
//! 需要读"这个技能的前置是什么""当前一共登记了哪些技能"，这些概念
//! 只存在于 `ll-mod`。与 [`crate::skill`]/[`crate::quest`] 同一个
//! 架构缺口，同一个解法：依赖倒置，[`SkillTreeCatalog`] trait 定义在
//! 这里（`ll-sim` 已经是 `SkillCatalog` 的家），`ll_mod::skill::SkillTable`
//! 实现它。
//!
//! # 为什么是独立的 `SkillTreeCatalog`，不是给 `SkillCatalog` 加方法
//!
//! [`crate::skill::SkillCatalog`] 现有的两个实现方
//! （[`crate::skill::NoSkills`]、`ll-sim/tests/skill_resolve.rs` 的
//! `FakeCatalog`）都只需要"查一条技能的冷却/资源/效果"这一件事，
//! 不需要"前置关系"或"全部已注册技能有哪些"——给一个已经稳定、有
//! 多处实现的 trait 加新的必需方法，会强迫它们全部补上一个用不到的
//! 实现。拆成独立 trait（继承 `SkillCatalog`，因为技能树视图确实也
//! 需要查冷却，见 [`build_skill_tree_view`] 的 `on_cooldown` 一节）
//! 只有真正需要这份额外信息的调用方才需要实现它。

use std::collections::BTreeSet;

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_world::entity::Agent;

use crate::skill::SkillCatalog;

/// [`build_skill_tree_view`] 需要的额外只读信息——单纯的
/// [`SkillCatalog`]（技能规则：冷却/资源/效果）不够，还需要"这个技能
/// 的前置是什么""当前一共登记了哪些技能"才能算出"可解锁但未解锁"这
/// 一档。
pub trait SkillTreeCatalog: SkillCatalog {
    /// 全部已注册技能的索引，任意确定顺序——
    /// [`build_skill_tree_view`] 自己会按 [`ContentIndex::get`] 升序
    /// 重新排序（约束 C5），调用方不需要预先排好。
    fn all_skills(&self) -> Vec<ContentIndex>;
    /// 给定技能的前置技能列表；未注册的索引返回空列表（对齐 ADR
    /// 0015：查不到就是查不到，不是这个技能"没有前置"与"根本不存在"
    /// 的语义混淆——调用方只在 `all_skills` 已经给出的索引上调用本
    /// 方法，不会撞见这个歧义）。
    fn prerequisites(&self, skill: ContentIndex) -> Vec<ContentIndex>;
}

/// 空技能树目录：一个技能都没有注册，任何技能的前置列表都是空的。
///
/// 复用 [`crate::skill::NoSkills`] 这个既有空对象，不另造一个
/// `NoSkillTree`：它已经是「技能这一路没接」的那个空实现，
/// [`SkillTreeCatalog`] 又恰好以 [`SkillCatalog`] 为超 trait——再造一
/// 个只多两个空方法的第二个空对象，会让调用方在两个语义完全相同的
/// 空实现之间做一次没有意义的选择（[ADR 0021](../../../../knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md)
/// 同一条判据的另一面：不为对称造第二个东西）。
///
/// **`all_skills` 返回空，不是「全部技能」**：没接目录时诚实的回答是
/// 「我不知道有哪些技能」，而 [`build_skill_tree_view`]/
/// `resolve_learn_skill` 在空列表上的行为（技能树全空、学任何技能都
/// 静默失败）正是「这一路没接」应有的表现，见 ADR 0015。
impl SkillTreeCatalog for crate::skill::NoSkills {
    fn all_skills(&self) -> Vec<ContentIndex> {
        Vec::new()
    }

    fn prerequisites(&self, _skill: ContentIndex) -> Vec<ContentIndex> {
        Vec::new()
    }
}

/// 技能树当前状态的可展示数据结构——[`build_skill_tree_view`] 的产出，
/// P7 UI 层的直接消费对象。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillTreeView {
    /// 已解锁的技能，按 [`ContentIndex::get`] 升序（约束 C5）。
    pub unlocked: Vec<ContentIndex>,
    /// 前置已满足但尚未解锁的技能，同样按升序排列。
    pub available: Vec<ContentIndex>,
    /// 冷却中的技能与其到期时刻——**到期时刻，不是剩余时长**，与
    /// [`ll_world::entity::Agent::skill_cooldowns`] 的存储形状一致
    /// （关键设计判断 4 的惰性到期判定）：调用方若要展示"还剩几回合"，
    /// 自己用 `until.0 - now.0` 现算，本结构不重复存一份会过时的
    /// 派生值。已过期的条目（`until.0 <= now.0`）不出现在这里——那些
    /// 技能已经可用，不叫"冷却中"。
    pub on_cooldown: Vec<(ContentIndex, Tick)>,
}

/// 给定一个 `Agent`，返回它当前的技能树状态。
///
/// `now` 由调用方显式传入（通常是 `WorldState::clock`）——本函数不
/// 接收 `&WorldState`，只需要 `Agent` 与当前时钟这两样，保持这份视图
/// 构建与"世界长什么样"解耦，方便测试与未来任何只想展示某个 `Agent`
/// 快照（例如离线存档浏览器）的调用方。
pub fn build_skill_tree_view(
    agent: &Agent,
    skills: &dyn SkillTreeCatalog,
    now: Tick,
) -> SkillTreeView {
    let unlocked_set: BTreeSet<ContentIndex> = agent.unlocked_skills.iter().copied().collect();

    let mut all = skills.all_skills();
    all.sort_by_key(ContentIndex::get);

    let mut available = Vec::new();
    for &skill in &all {
        if unlocked_set.contains(&skill) {
            continue;
        }
        let prerequisites = skills.prerequisites(skill);
        if prerequisites.iter().all(|p| unlocked_set.contains(p)) {
            available.push(skill);
        }
    }

    let mut on_cooldown: Vec<(ContentIndex, Tick)> = agent
        .skill_cooldowns
        .iter()
        .filter(|&(_, until)| until.0 > now.0)
        .map(|(&skill, &until)| (skill, until))
        .collect();
    on_cooldown.sort_by_key(|(skill, _)| skill.get());

    let mut unlocked: Vec<ContentIndex> = agent.unlocked_skills.clone();
    unlocked.sort_by_key(ContentIndex::get);

    SkillTreeView {
        unlocked,
        available,
        on_cooldown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::SkillRule;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_world::entity::BaseStats;
    use std::collections::BTreeMap;

    /// 一个持有固定前置关系图的测试目录——不需要真正的冷却/资源/效果
    /// 数据（`skill()` 恒返回 `None`，本模块的视图构建从不调用它），
    /// 只需要满足 `SkillCatalog` 这个 supertrait 的签名。
    struct FakeTreeCatalog {
        all: Vec<ContentIndex>,
        prerequisites: BTreeMap<ContentIndex, Vec<ContentIndex>>,
    }

    impl SkillCatalog for FakeTreeCatalog {
        fn skill(&self, _skill: ContentIndex) -> Option<SkillRule> {
            None
        }
    }

    impl SkillTreeCatalog for FakeTreeCatalog {
        fn all_skills(&self) -> Vec<ContentIndex> {
            self.all.clone()
        }

        fn prerequisites(&self, skill: ContentIndex) -> Vec<ContentIndex> {
            self.prerequisites.get(&skill).cloned().unwrap_or_default()
        }
    }

    /// 造两个互不相同的测试用技能索引：`(strike, power_strike)`——本
    /// 文件全部测试只需要一个"起点 + 一个前置指向它的后续"这样的最小
    /// 二节点图，不需要通用的"造 n 个索引"工具函数。
    fn two_skill_ids() -> (ContentIndex, ContentIndex) {
        let mut interner = Interner::new();
        let strike = interner.intern(NamespacedId::parse("test:strike").expect("合法标识符"));
        let power_strike =
            interner.intern(NamespacedId::parse("test:power_strike").expect("合法标识符"));
        (strike, power_strike)
    }

    fn blank_agent() -> Agent {
        let mut interner = Interner::new();
        let profession = interner.intern(NamespacedId::parse("lostland:tester").unwrap());
        let race = interner.intern(NamespacedId::parse("lostland:human").unwrap());
        Agent {
            // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
            gender: ll_world::entity::Gender::default(),
            pos: ll_core::torus::TorusSize::new(64, 64).unwrap().wrap(0, 0),
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
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
            identified_items: Vec::new(),
            skill_cooldowns: BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: BTreeMap::new(),
            current_space: ll_world::space::Space::surface(
                ll_core::torus::TorusSize::new(1, 1).unwrap().wrap(0, 0),
                ContentIndex::default(),
            ),
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
            home: None,
        }
    }

    #[test]
    fn 前置已满足但未解锁的技能出现在available中() {
        // Arrange：strike（已解锁）-> power_strike（前置是 strike）。
        let (strike, power_strike) = two_skill_ids();
        let catalog = FakeTreeCatalog {
            all: vec![strike, power_strike],
            prerequisites: BTreeMap::from([(power_strike, vec![strike])]),
        };
        let mut agent = blank_agent();
        agent.unlocked_skills.push(strike);

        // Act
        let view = build_skill_tree_view(&agent, &catalog, Tick(0));

        // Assert
        assert_eq!(view.available, vec![power_strike]);
    }

    #[test]
    fn 已解锁但前置未满足的技能不出现在available中() {
        // 防御性测试：即便 unlocked_skills 里出现了一个前置未满足的
        // 技能（正常游玩不应该发生，但数据一致性不应该依赖"正常游玩"
        // 这个假设），它也不应该同时出现在 available——available 只
        // 装"尚未解锁"的技能，已解锁的一律走 unlocked 那一档。
        // Arrange
        let (strike, power_strike) = two_skill_ids();
        let catalog = FakeTreeCatalog {
            all: vec![strike, power_strike],
            prerequisites: BTreeMap::from([(power_strike, vec![strike])]),
        };
        let mut agent = blank_agent();
        // 故意不解锁 strike，直接解锁 power_strike。
        agent.unlocked_skills.push(power_strike);

        // Act
        let view = build_skill_tree_view(&agent, &catalog, Tick(0));

        // Assert
        assert!(!view.available.contains(&power_strike));
        assert_eq!(view.unlocked, vec![power_strike]);
    }

    #[test]
    fn 冷却中的技能剩余到期时刻计算正确() {
        // Arrange
        let (strike, _) = two_skill_ids();
        let catalog = FakeTreeCatalog {
            all: vec![strike],
            prerequisites: BTreeMap::new(),
        };
        let mut agent = blank_agent();
        agent.unlocked_skills.push(strike);
        agent.skill_cooldowns.insert(strike, Tick(100));

        // Act
        let view = build_skill_tree_view(&agent, &catalog, Tick(40));

        // Assert
        assert_eq!(view.on_cooldown, vec![(strike, Tick(100))]);
    }

    #[test]
    fn 冷却已过期的技能不出现在on_cooldown中() {
        // Arrange：惰性判定——到期时刻早于当前时钟时不再算冷却中。
        let (strike, _) = two_skill_ids();
        let catalog = FakeTreeCatalog {
            all: vec![strike],
            prerequisites: BTreeMap::new(),
        };
        let mut agent = blank_agent();
        agent.unlocked_skills.push(strike);
        agent.skill_cooldowns.insert(strike, Tick(10));

        // Act
        let view = build_skill_tree_view(&agent, &catalog, Tick(50));

        // Assert
        assert!(view.on_cooldown.is_empty());
    }
}
