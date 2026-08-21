//! 行为树/AI 决策来源的依赖倒置接口（规格 §10.5 接线批次）。
//!
//! # 断点是什么
//!
//! 规格 §10.5：随从/敌人的行为树写成 Steel `.scm`，Rust 侧只实现 tick
//! 求值器（遍历 `selector`/`sequence`），节点判断/动作是脚本函数——见
//! `crates/ll-script/src/behavior.rs`。行为树的输出即 [`crate::intent::Intent`]，
//! 走既有的 `resolve → Effect → apply` 管线，与玩家按键、mod 注册的
//! 技能完全同一条管线（规格 §4）。
//!
//! 求值器本身要调 Steel 引擎（`ll_script::host::ScriptEngine`）——它
//! 的方法接收/返回 `steel::rvals::SteelVal`，这是 `steel-core` 的类型；
//! 但求值器的最终产物必须是本 crate 的 [`crate::intent::Intent`]。依赖
//! 方向 `ll-sim` ← `ll-script`（规格 §5）不允许本 crate 反过来依赖
//! `ll-script`，物理上没有办法让本 crate 直接调用求值器。
//!
//! # 依赖倒置：照 [`crate::skill::SkillCatalog`] 的先例
//!
//! [`crate::skill::SkillCatalog`] 已经示范过同一个处境的解法：`resolve`
//! 需要读技能定义，但技能定义存在下游的 `ll-mod`（`ll-sim` 不能反过来
//! 依赖它）——办法是在本 crate 定义一个只描述「需要什么」的 trait，
//! 由下游持有真正实现的一方去实现它。[`BehaviorTreeSource`] 是同一个
//! 手法在「AI 决策」上的应用：本 crate 只声明「给我世界与一个实体，
//! 还给我这一回合想做的 `Intent`」这个接口，`decide` 内部要不要跑
//! Steel 引擎、跑哪一棵树，本 crate 一概不关心——`ll_mod::script_behavior_source::ScriptBehaviorSource`
//! 是当前唯一的真实实现，它持有一个 `ll_script::host::ScriptEngine`
//! 并调用 `ll_script::behavior::tick`。
//!
//! # 有没有产生重复声明——先例的代价，这次没有踩中
//!
//! `SkillCatalog` 先例的模块文档记录过一次真实代价：`ll-mod` 曾经
//! 因为不允许依赖 `ll-sim`，被迫在自己那边重新声明一份结构相同的
//! `SkillEffect`/`ResourceCost`，直到 `ll-mod` 升级为 `ll-sim` 的
//! 生产依赖（`4a728ba`）才合并掉。本次没有这个问题：`ll-mod` 在
//! 接线批次之前就已经是 `ll-sim` 的生产依赖（见 `crates/ll-mod/Cargo.toml`
//! 「P5-B 接线批次」注释），[`BehaviorTreeSource`] 从落地第一天起就
//! 只有本 crate 这一份声明，`ll-mod`/`ll-script` 直接 `use` 它，没有
//! 平行的第二份定义需要维护或合并。
//!
//! # 为什么 `decide` 接收 `&mut dyn BehaviorTreeSource` 而不是 `&self`
//!
//! 真正的实现要调用 `ScriptEngine::call_raw`，这个方法要求 `&mut self`
//! （Steel VM 的求值本身是有状态的过程，即使脚本逻辑本身被约束成
//! 「无隐式跨帧状态」，见规格 §4 约束 C1）。`&mut self` **不是**约束
//! C1 意义上的「隐式状态」——它只是 Rust 层面「调用一次脚本求值」这个
//! 动作本身需要独占访问 VM 实例，与 VM 内部有没有存跨帧状态是两个
//! 不同的问题。约束 C1 管的是「VM 能不能随时从零重建、重建要不要
//! 迁移」，不是「调用求值需不需要 `&mut`」。
//!
//! # C1：本模块不写世界
//!
//! [`resolve_ai_turn`] 只接收 `&WorldState`（[`BehaviorTreeSource::decide`]
//! 同样只接收 `&WorldState`）——两者都不持有、也不可能拿到
//! `&mut WorldState`，行为树求值因此无法在这一层绕过 `apply` 直接写
//! 世界，见 [`crate::resolve`] 模块文档「C1」一节的同一条纪律。

use ll_world::entity::EntityId;
use ll_world::state::WorldState;

use crate::effect::Effect;
use crate::intent::Intent;
use crate::quest::QuestCatalog;
use crate::resolve::resolve_with_skills_and_quests;
use crate::skill::SkillCatalog;

/// 行为树/AI 决策来源：给定当前世界与一个实体，产出这个实体这一回合
/// 想做的 [`Intent`]。
///
/// 真正的求值（遍历 Steel `.scm` 写的 `selector`/`sequence` 结构）由
/// 下游实现（见模块文档「依赖倒置」一节）；`decide` 找不到脚本、脚本
/// 求值失败、或整棵树的每一条分支都失败时，返回 `None`——与规格
/// §10.2 第二道防线「降级而非崩溃」同一条纪律：AI 算不出这一回合该
/// 干什么，不是异常，[`resolve_ai_turn`] 据此产出空效果（这一回合
/// 什么都不发生），不会 panic，也不会让调用方看到 `Err`。
pub trait BehaviorTreeSource {
    /// 为 `actor` 产出这一回合的意图；找不到可用决策时返回 `None`。
    fn decide(&mut self, world: &WorldState, actor: EntityId) -> Option<Intent>;
}

/// 空决策来源：任何实体都拿不到任何行为树，恒返回 `None`。
///
/// 与 [`crate::skill::NoSkills`]/[`crate::quest::NoQuests`] 同一个模式
/// ——调用方没有接好脚本层（例如尚未装载任何 mod、或明确不需要 AI
/// 决策的调用场景，如测试只想验证纯移动/攻击结算）时的保底实现。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoBehavior;

impl BehaviorTreeSource for NoBehavior {
    fn decide(&mut self, _world: &WorldState, _actor: EntityId) -> Option<Intent> {
        None
    }
}

/// 「AI 回合」的最小可复用组合：向 `source` 要一个意图，拿到就走既有
/// 的 `resolve_with_skills_and_quests` 结算；拿不到（[`BehaviorTreeSource::decide`]
/// 返回 `None`）就产出空效果——不代替调用方决定「AI 什么都不做时该不该
/// 补发一个 `Intent::Wait`」，那是调用方（持有时间轴的一方）的职责，
/// 本函数只负责「决策 → 结算」这一段的接线，不含任何调度逻辑。
///
/// 不调用 [`crate::apply::apply`]——与本 crate 全部 `resolve*` 系入口
/// 同一条纪律（C1）：产出 `Vec<Effect>` 交回调用方，写世界的时机与
/// 顺序由调用方决定。
pub fn resolve_ai_turn(
    world: &WorldState,
    actor: EntityId,
    source: &mut dyn BehaviorTreeSource,
    skills: &dyn SkillCatalog,
    quests: &dyn QuestCatalog,
) -> Vec<Effect> {
    match source.decide(world, actor) {
        Some(intent) => resolve_with_skills_and_quests(world, &intent, skills, quests),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use ll_core::time::Tick;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    use crate::intent::Direction;
    use crate::quest::NoQuests;
    use crate::skill::NoSkills;

    use super::*;

    struct FixedDecision(Option<Intent>);

    impl BehaviorTreeSource for FixedDecision {
        fn decide(&mut self, _world: &WorldState, _actor: EntityId) -> Option<Intent> {
            self.0
        }
    }

    fn test_world() -> (WorldState, EntityId) {
        let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        let mut world = WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件");
        world
            .terrain
            .set_terrain(world.size.wrap(1, 0), terrain_ids.grass);
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let actor = world.actors.spawn(Agent {
            pos: spawn,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(
                world.terrain.layout().tile_to_zone(spawn).0,
                ll_core::ident::ContentIndex::default(),
            ),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        });
        (world, actor)
    }

    #[test]
    fn 决策来源返回意图时产出对应的效果() {
        // Arrange
        let (world, actor) = test_world();
        let mut source = FixedDecision(Some(Intent::Move {
            actor,
            dir: Direction::East,
        }));

        // Act
        let effects = resolve_ai_turn(&world, actor, &mut source, &NoSkills, &NoQuests);

        // Assert
        assert!(!effects.is_empty());
    }

    #[test]
    fn 决策来源返回空时产出空效果而非崩溃() {
        // Arrange
        let (world, actor) = test_world();
        let mut source = FixedDecision(None);

        // Act
        let effects = resolve_ai_turn(&world, actor, &mut source, &NoSkills, &NoQuests);

        // Assert
        assert!(effects.is_empty());
    }

    #[test]
    fn nobehavior恒不产出决策() {
        // Arrange
        let (world, actor) = test_world();
        let mut source = NoBehavior;

        // Act
        let intent = source.decide(&world, actor);

        // Assert
        assert_eq!(intent, None);
    }
}
