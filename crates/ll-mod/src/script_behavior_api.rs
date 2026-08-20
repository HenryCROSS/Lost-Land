//! 行为树运行期查询：`skill-ready?`——把「这个技能现在能不能用」暴露
//! 给脚本，接上此前断掉的「AI 真的做出决策」最后一环（规格 §10.5
//! 接线批次）。
//!
//! # 为什么这一个函数单独落在 `ll-mod`，其余行为树查询原语落在
//! `ll-script`
//!
//! `crates/ll-script/src/api/actor.rs` 的 `self-handle`/`nearby-enemy`/
//! `direction-toward` 只需要读 `WorldState`（地形/实体/归属），本 crate
//! 依赖方向允许的最下游就能实现，因此落在 `ll-script`。「技能是否已
//! 解锁/冷却中」这两条判断本身也只需要读 `WorldState`（`Agent::
//! unlocked_skills`/`skill_cooldowns`），**但**脚本给出的是一个命名
//! 空间字符串（`"examplemod:frostbolt"`），要把它转换成
//! `ll_core::ident::ContentIndex` 才能查 `Agent` 上的这两个字段——这个
//! 转换需要 [`crate::registry::Registry`]，它定义在本 crate，`ll-script`
//! 不能反过来依赖它（依赖方向 `ll-world` ← `ll-sim` ← `ll-script` ←
//! `ll-mod`，规格 §5）。与 `crate::script_skill_api` 等六个 `register-*`
//! 注册函数需要 `Registry` 是同一个理由，但**不是**同一套接线方式：
//! 那六个函数走装载期唯一共享的 `crate::active_registry`（一套
//! `thread_local!` 搭配 `set_active_registry`/`take_active_registry`），
//! 因为它们要在**同一次脚本求值窗口内**写同一个 `Registry`；本模块的
//! `skill-ready?` 是**运行期**的只读查询，`Registry` 在装载完成后不再
//! 改变，没有必要为它另起一套装载期才需要的「取走所有权再放回」的
//! 协议——见 [`register_skill_ready_api`] 文档「为什么用一次性快照，
//! 不是活跃指针」一节。

use std::collections::BTreeMap;

use ll_core::ident::ContentIndex;
use ll_script::host::ScriptEngine;

use crate::registry::Registry;

/// 把 `registry` 里全部已注册的命名空间 ID 快照成一份「完整字符串 →
/// `ContentIndex`」映射——键的形状与脚本里写的字面量一致
/// （`"examplemod:frostbolt"`），供 [`register_skill_ready_api`] 捕获
/// 进它注册的闭包。
pub fn skill_index_snapshot(registry: &Registry) -> BTreeMap<String, ContentIndex> {
    registry
        .snapshot()
        .into_iter()
        .filter_map(|id| {
            let index = registry.get(&id)?;
            Some((format!("{}:{}", id.namespace(), id.path()), index))
        })
        .collect()
}

/// 注册 `skill-ready?` 进 `engine`。
///
/// # 为什么用一次性快照，不是活跃指针
///
/// `Registry` 在 mod 装载完成后就不再变化（装载是一次性的启动期
/// 步骤，运行期不会有新 mod 中途注册新技能）——`skill_index` 因此可以
/// 在**构造这个 `ScriptEngine` 的那一刻**一次性算好，作为一份普通的
/// 拥有所有权的 `BTreeMap` 被闭包捕获，不需要像 `active_registry`
/// 那样在每次调用窗口前后「设置/取回」：没有东西会在两次调用之间
/// 变化，也就没有「取回」这一步要做的事。这比给运行期也接一套
/// `thread_local!` 活跃指针更简单，且不需要处理 `Registry`
/// 本身不是 `Send`/`Sync`、无法安全放进要求 `'static` 的闭包这个
/// 潜在问题（`BTreeMap<String, ContentIndex>` 是纯拥有的数据，
/// `ContentIndex` 是 `Copy`，天然满足）。
pub fn register_skill_ready_api(
    engine: &mut ScriptEngine,
    skill_index: BTreeMap<String, ContentIndex>,
) {
    engine.register_fn("skill-ready?", move |name: String| -> bool {
        match skill_index.get(&name) {
            Some(&index) => skill_ready(index),
            None => false,
        }
    });
}

/// 当前决策实体（活跃实体）能不能用 `skill`：已解锁，且不在冷却中。
///
/// 与 `ll_sim::resolve::resolve_use_skill` 门一/门二完全同一条判断
/// （惰性冷却判定，现比对世界时钟）——本函数不重复发明规则，只是把
/// 同一条规则暴露成脚本能在决策前先问一次的只读查询，真正的资源/
/// 冷却写入仍然只在 `resolve_use_skill`（经 `Effect` → `apply`）发生，
/// 本函数没有、也不能改任何东西。**不检查资源是否充足**——门四（资源
/// 检查）需要知道这个技能消耗多少资源，那份数据在
/// `ll_mod::skill::SkillTable`（本模块拿到的只是命名空间字符串到
/// `ContentIndex` 的映射，不是完整的技能定义视图）；资源不足时
/// `resolve_use_skill` 仍会静默拒绝，行为树因此可能白白选中一个用不
/// 起的技能，但不会产生错误效果——是本次接线刻意留下的已知简化，不是
/// 缺陷（脚本作者若需要更精确的判断，可以自己在 mod 内追加一个
/// 检查资源余量的查询函数）。
fn skill_ready(skill: ContentIndex) -> bool {
    ll_script::api::actor::with_active_self(false, |world, agent| {
        agent.unlocked_skills.contains(&skill)
            && !agent
                .skill_cooldowns
                .get(&skill)
                .is_some_and(|until| until.0 > world.clock.0)
    })
}

#[cfg(test)]
mod tests {
    use ll_core::ident::NamespacedId;
    use ll_core::time::Tick;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    use ll_script::api::actor::{clear_active_actor, set_active_actor};
    use ll_script::api::query::with_active_world_for;

    use super::*;

    fn test_world_with_agent(
        unlocked: Vec<ContentIndex>,
        cooldown_until: Option<(ContentIndex, ll_core::time::Tick)>,
    ) -> (ll_world::state::WorldState, ll_world::entity::EntityId) {
        let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        let mut world = ll_world::state::WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件");
        let mut interner = ll_core::ident::Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        let mut skill_cooldowns = std::collections::BTreeMap::new();
        if let Some((skill, until)) = cooldown_until {
            skill_cooldowns.insert(skill, until);
        }
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
            unlocked_skills: unlocked,
            skill_cooldowns,
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
        });
        (world, actor)
    }

    fn skill_index() -> ContentIndex {
        let mut interner = ll_core::ident::Interner::new();
        interner.intern(NamespacedId::parse("examplemod:frostbolt").expect("合法标识符"))
    }

    #[test]
    fn 已解锁且不在冷却中的技能判定为可用() {
        // Arrange
        let skill = skill_index();
        let (world, actor) = test_world_with_agent(vec![skill], None);

        // Act
        let ready = with_active_world_for(&world, || {
            set_active_actor(actor);
            let ready = skill_ready(skill);
            clear_active_actor();
            ready
        });

        // Assert
        assert!(ready);
    }

    #[test]
    fn 未解锁的技能判定为不可用() {
        // Arrange
        let skill = skill_index();
        let (world, actor) = test_world_with_agent(Vec::new(), None);

        // Act
        let ready = with_active_world_for(&world, || {
            set_active_actor(actor);
            let ready = skill_ready(skill);
            clear_active_actor();
            ready
        });

        // Assert
        assert!(!ready);
    }

    #[test]
    fn 冷却尚未结束的技能判定为不可用() {
        // Arrange
        let skill = skill_index();
        let (mut world, actor) = test_world_with_agent(vec![skill], Some((skill, Tick(100))));
        world.advance(1);

        // Act
        let ready = with_active_world_for(&world, || {
            set_active_actor(actor);
            let ready = skill_ready(skill);
            clear_active_actor();
            ready
        });

        // Assert
        assert!(!ready);
    }

    #[test]
    fn skill_index_snapshot能把注册表的字符串id映射回contentindex() {
        // Arrange
        let mut registry = Registry::new();
        let id = NamespacedId::parse("examplemod:frostbolt").expect("合法标识符");
        let index = registry.intern(id);

        // Act
        let snapshot = skill_index_snapshot(&registry);

        // Assert
        assert_eq!(snapshot.get("examplemod:frostbolt"), Some(&index));
    }

    #[test]
    fn 注册后脚本能调用skill_ready判断技能是否可用() {
        // Arrange：端到端——真实脚本源码经 ScriptEngine::load_source
        // 调用 skill-ready?，不是直接在 Rust 里调 skill_ready。
        let skill = skill_index();
        let (world, actor) = test_world_with_agent(vec![skill], None);
        let mut registry = Registry::new();
        registry.intern(NamespacedId::parse("examplemod:frostbolt").expect("合法标识符"));
        let mut engine = ScriptEngine::new();
        register_skill_ready_api(&mut engine, skill_index_snapshot(&registry));
        engine
            .load_source(r#"(define (probe) (skill-ready? "examplemod:frostbolt"))"#.to_string())
            .unwrap();

        // Act
        let result = with_active_world_for(&world, || {
            set_active_actor(actor);
            let result = engine.call_raw("probe", Vec::new());
            clear_active_actor();
            result
        });

        // Assert
        assert_eq!(result, Ok(steel::rvals::SteelVal::BoolV(true)));
    }
}
