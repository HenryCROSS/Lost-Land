//! 行为树运行期查询：`skill-ready?`/`self-has-profession?`/
//! `actor-inspection-suspicion`——分别把「这个技能现在能不能用」
//! 「活跃实体是不是这个职业」「我看到的这个人有多可疑」暴露给脚本，
//! 接上此前断掉的「AI 真的做出决策」最后一环（规格 §10.5 接线批次；
//! `self-has-profession?` 是卫兵职业接线批次新增，
//! `actor-inspection-suspicion` 是盗贼被动两分批次新增，见下文）。
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
use ll_script::api::handle::ScriptEntityHandle;
use ll_script::host::ScriptEngine;
use ll_sim::rule_modifier::{
    INSPECTION_SUSPICION_SCALE, agent_rule_modifiers, inspection_suspicion_permille,
};
use ll_world::entity::EntityId;

use crate::class::ClassTable;
use crate::item::ItemTable;
use crate::race::RaceTable;
use crate::registry::Registry;
use crate::trait_def::TraitTable;

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

/// 注册 `self-has-profession?` 进 `engine`（卫兵职业接线批次）——把
/// 「活跃实体的 `Agent.profession` 是不是这个职业」暴露给脚本，理由与
/// 用法都与 [`register_skill_ready_api`] 同构：`Agent.profession` 只是
/// 一个 [`ContentIndex`]，脚本给的是命名空间字符串（例如
/// `"lostland:guard"`），转换同样需要 `Registry`，因此同样落在
/// `ll-mod`，不是 `ll-script`。
///
/// # 为什么复用 [`skill_index_snapshot`]，不新写一份
///
/// `skill_index_snapshot` 的实际语义是「注册表里全部命名空间 ID 的
/// 快照」（不是只包含技能——名字沿用了它第一个调用方的名字，但函数体
/// 本身对内容类型一无所知，只是 `Registry::snapshot` 的整体映射）。
/// 职业 ID 与技能 ID 共享同一个 `Registry`（`ContentIndex` 号段是
/// 全局的，见 `ll_mod::class` 模块文档「下标空间是全局的」一节），
/// 因此这份快照天然也包含职业 ID，不需要再写一个只是把「技能」换成
/// 「职业」的重复函数（DRY）——[`crate::script_behavior_source::ScriptBehaviorSource::new`]
/// 因此对同一份 `skill_index` 快照调用两次注册函数,各自捕获自己需要
/// 的那一份克隆。
pub fn register_profession_check_api(
    engine: &mut ScriptEngine,
    class_index: BTreeMap<String, ContentIndex>,
) {
    engine.register_fn("self-has-profession?", move |name: String| -> bool {
        match class_index.get(&name) {
            Some(&index) => has_profession(index),
            None => false,
        }
    });
}

/// 当前决策实体（活跃实体）的 `Agent.profession` 是否等于 `class`。
fn has_profession(class: ContentIndex) -> bool {
    ll_script::api::actor::with_active_self(false, |_world, agent| agent.profession == class)
}

/// 行为树运行期查询 `actor-inspection-suspicion` 需要的那几张内容表的
/// **一次性快照**（盗贼被动两分批次新增）。
///
/// # 为什么是快照（`Clone`），不是借用
///
/// `ScriptEngine::register_fn` 注册的闭包要求 `'static`——借用进去的
/// 表会把生命周期传染给整个 [`crate::script_behavior_source::ScriptBehaviorSource`]，
/// 而它要作为 `ll_sim::behavior::BehaviorTreeSource` 被
/// `TurnEngine::advance_ai` 的 `&mut dyn FnMut` 持有，那条链路上没有
/// 任何一处能提供这个生命周期。
///
/// 快照在本模块有现成的先例与同一条正当性论证：见
/// [`register_skill_ready_api`] 文档「为什么用一次性快照，不是活跃
/// 指针」一节——`Registry` 与这四张表都在 mod 装载完成后就不再变化
/// （运行期不会有新 mod 中途注册新天赋），因此「快照」与「实时读」
/// 在语义上无差别，不存在两份真相漂移的可能。差别只在
/// [`skill_index_snapshot`] 折叠成了一份 `BTreeMap`，而这里必须留着
/// 整张表：本查询的答案依赖**实体运行期的状态**（种族/职业/等级/
/// 已装备物品），折不成一份与实体无关的静态映射。
///
/// # 为什么打包成一个结构体，不是四个参数
///
/// 与 `ll_sim::catalogs::ResolveCatalogs` 同一条既有手法：这四张表
/// 是「聚合规则修正」这一件事的完整输入，将来接第三、第四路来源
/// （技能/药品，见 `ll_sim::rule_modifier::agent_rule_modifiers` 文档）
/// 时只需要给本结构体加字段，不必再改一次
/// [`register_inspection_suspicion_api`] 与它全部调用点的签名。
#[derive(Debug, Clone, Default)]
pub struct BehaviorRuleCatalogs {
    /// 种族这一路天赋来源。
    pub race: RaceTable,
    /// 职业这一路天赋来源——`examplemod:cutpurse_training` 正是走这
    /// 一路（`register-class-trait`，3 级解锁）。
    pub class: ClassTable,
    /// 天赋定义表。
    pub traits: TraitTable,
    /// 物品定义表——规则修正的第二路来源（装备）。
    pub items: ItemTable,
}

impl BehaviorRuleCatalogs {
    /// 从调用方持有的四张表各克隆一份，理由见类型文档「为什么是快照」。
    pub fn snapshot(
        race: &RaceTable,
        class: &ClassTable,
        traits: &TraitTable,
        items: &ItemTable,
    ) -> Self {
        Self {
            race: race.clone(),
            class: class.clone(),
            traits: traits.clone(),
            items: items.clone(),
        }
    }

    /// 一个实体此刻的「盘查意愿」千分比——
    /// `ll_sim::rule_modifier::inspection_suspicion_permille` 在这份
    /// 快照上的应用。查不到实体时返回
    /// [`INSPECTION_SUSPICION_SCALE`]（与常人无异），与本模块其余
    /// 查询同一条降级纪律。
    fn suspicion_permille(&self, world: &ll_world::state::WorldState, target: EntityId) -> i32 {
        match world.actors.get(target) {
            Some(agent) => inspection_suspicion_permille(&agent_rule_modifiers(
                agent,
                &self.race,
                &self.class,
                &self.traits,
                &self.items,
            )),
            None => INSPECTION_SUSPICION_SCALE,
        }
    }
}

/// 注册 `actor-inspection-suspicion` 进 `engine`（盗贼被动两分批次）。
///
/// `(actor-inspection-suspicion target)` → 千分比整数：`1000` 表示
/// 「与常人无异」，更小表示更不容易被盯上，`0` 表示永远不会被怀疑。
/// 句柄失效、没有活跃世界、或者目标身上一条
/// `RuleModifier::InspectionSuspicion` 都没有声明时，一律返回
/// [`INSPECTION_SUSPICION_SCALE`]——「查不到」与「没有这条被动」给出
/// 同一个确定值，与本模块其余查询「宿主接线可能有 bug，选一个确定值
/// 而不是 panic」同一条降级纪律。
///
/// # 为什么本被动的消费者是脚本，不是 `resolve`
///
/// 见 `ll_sim::rule_modifier::RuleModifier::InspectionSuspicion` 文档
/// 「消费者在脚本侧，不在 `resolve` 侧」一节：这条被动减的是「要不要
/// 发起盘查」那一次掷骰，而那次掷骰整个发生在行为树里
/// （`mods/example_mod/behavior.scm` 的 `guard-inspect-chance`），
/// `Intent::Inspect` 一旦产出，`resolve_inspect` 就恒执行、不重新
/// 判断该不该查。
///
/// # 为什么落在 `ll-mod` 而不是 `ll-script`
///
/// 与 [`register_profession_check_api`] 同一条判据（见模块文档
/// 「为什么这一个函数单独落在 `ll-mod`」一节）：本查询要走
/// `ll_sim::rule_modifier::agent_rule_modifiers`，它的四个参数是
/// `RaceTable`/`ClassTable`/`TraitTable`/`ItemTable` 这四张定义在本
/// crate 的表，`ll-script` 不能反过来依赖它们（规格 §5）。
///
/// # 为什么取一个目标句柄，不是零参读活跃实体
///
/// 与 `ll_script::api::actor` 的 `actor-stealthed?` 完全同一条理由：
/// 唯一的调用场景问的是「**我看到的这个人**有多可疑」——观察者
/// （卫兵，活跃实体）与被观察者是两个不同的实体。
/// # 为什么经 `with_active_self` 拿世界，而不是 `with_active_world`
///
/// `ll_script::api::query::with_active_world` 是 `pub(crate)`（只给
/// `ll-script` 自己的查询函数与 `api::state` 用），本 crate 够不着；
/// [`ll_script::api::actor::with_active_self`] 是**已经公开**的那一个
/// 跨 crate 入口，本模块的 [`has_profession`] 已经在用它。它多要求
/// 一个前置条件——「活跃实体存在」——而本查询逻辑上只需要活跃世界。
///
/// 这个多出来的前置条件在真实调用路径上恒成立且无副作用：本函数只
/// 可能从行为树的一次 tick 里被调用，而
/// [`crate::script_behavior_source::ScriptBehaviorSource`] 的 `decide`
/// （`ll_sim::behavior::BehaviorTreeSource` 的实现）每次 tick 都成对
/// 设置活跃世界与活跃实体（活跃实体就是发起查询的
/// 那个观察者，也就是卫兵自己）。前置条件万一不成立，两条路径给出的
/// 也是同一个降级值 [`INSPECTION_SUSPICION_SCALE`]。
///
/// 选它而不是把 `with_active_world` 改成 `pub`，是因为后者会把一个
/// 读裸指针的 `unsafe` 辅助函数的可见性放宽到整个工作区，而本批次
/// 从中得到的只是「少写一个用不上的闭包参数」——不值这个代价。
pub fn register_inspection_suspicion_api(
    engine: &mut ScriptEngine,
    catalogs: BehaviorRuleCatalogs,
) {
    engine.register_fn(
        "actor-inspection-suspicion",
        move |target: ScriptEntityHandle| -> i64 {
            ll_script::api::actor::with_active_self(
                i64::from(INSPECTION_SUSPICION_SCALE),
                |world, _observer| {
                    i64::from(catalogs.suspicion_permille(world, target.entity_id()))
                },
            )
        },
    );
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
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: unlocked,
            known_recipes: Vec::new(),
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
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
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

    /// 与 `test_world_with_agent` 同一套构造,但让调用方指定 `profession`
    /// ——`self-has-profession?` 要比对的正是这个字段,不能像
    /// `test_world_with_agent` 那样用一个跟外部 `Registry` 无关的本地
    /// interner 现造一个恒为 "lostland:tester" 的职业索引。
    fn test_world_with_profession(
        profession: ContentIndex,
    ) -> (ll_world::state::WorldState, ll_world::entity::EntityId) {
        let mut interner = ll_core::ident::Interner::new();
        let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        test_world_with_race_and_profession(race, profession)
    }

    /// 与 [`test_world_with_profession`] 对称：让调用方指定 `race`
    /// ——`actor-inspection-suspicion` 走的是天赋聚合，天赋的来源之一
    /// 正是种族，因此这一路同样不能用一个跟外部表无关的本地索引。
    fn test_world_with_race(
        race: ContentIndex,
    ) -> (ll_world::state::WorldState, ll_world::entity::EntityId) {
        let mut interner = ll_core::ident::Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
        test_world_with_race_and_profession(race, profession)
    }

    /// 上面两个帮手共用的构造——ADR 0021：抽出来的理由是两者除了
    /// 「哪个字段由调用方指定」之外逐字相同，不是对称好看。
    fn test_world_with_race_and_profession(
        race: ContentIndex,
        profession: ContentIndex,
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
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
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
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        (world, actor)
    }

    #[test]
    fn 职业匹配时has_profession判定为真() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let guard = interner.intern(NamespacedId::parse("lostland:guard").expect("合法标识符"));
        let (world, actor) = test_world_with_profession(guard);

        // Act
        let matched = with_active_world_for(&world, || {
            set_active_actor(actor);
            let matched = has_profession(guard);
            clear_active_actor();
            matched
        });

        // Assert
        assert!(matched);
    }

    #[test]
    fn 职业不匹配时has_profession判定为假() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let guard = interner.intern(NamespacedId::parse("lostland:guard").expect("合法标识符"));
        let warrior = interner.intern(NamespacedId::parse("lostland:warrior").expect("合法标识符"));
        let (world, actor) = test_world_with_profession(warrior);

        // Act
        let matched = with_active_world_for(&world, || {
            set_active_actor(actor);
            let matched = has_profession(guard);
            clear_active_actor();
            matched
        });

        // Assert
        assert!(!matched);
    }

    #[test]
    fn 注册后脚本能调用self_has_profession判断当前职业() {
        // Arrange：端到端——真实脚本源码经 ScriptEngine::load_source
        // 调用 self-has-profession?，不是直接在 Rust 里调
        // has_profession，理由同 `注册后脚本能调用skill_ready判断技能是否可用`。
        let mut registry = Registry::new();
        let guard_id = NamespacedId::parse("lostland:guard").expect("合法标识符");
        let guard = registry.intern(guard_id);
        let (world, actor) = test_world_with_profession(guard);
        let mut engine = ScriptEngine::new();
        register_profession_check_api(&mut engine, skill_index_snapshot(&registry));
        engine
            .load_source(r#"(define (probe) (self-has-profession? "lostland:guard"))"#.to_string())
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

    /// 帮手：把 `race`+`trait` 两张表填成「这个种族 1 级就授予一条
    /// 声明了 `multiplier_permille` 盘查意愿的天赋」，返回可直接喂给
    /// [`register_inspection_suspicion_api`] 的快照。
    fn suspicion_catalogs(race: ContentIndex, multiplier_permille: i32) -> BehaviorRuleCatalogs {
        let mut interner = ll_core::ident::Interner::new();
        let trait_id =
            interner.intern(NamespacedId::parse("yourmod:cutpurse_training").expect("合法标识符"));
        let mut race_table = RaceTable::new();
        race_table
            .define(
                race,
                crate::race::RaceAttrs {
                    display_name_key: NamespacedId::parse("yourmod:race.display")
                        .expect("合法标识符"),
                    stat_modifiers: BaseStats::BASELINE,
                    darkvision_cells: 0,
                    footprint: (1, 1),
                    lifespan_years: 80,
                    xp_reward: 0,
                    traits: Vec::new(),
                    starting_items: Vec::new(),
                },
            )
            .expect("首次定义恒成功");
        race_table
            .add_trait_grant(
                race,
                ll_sim::traits::TraitGrant {
                    trait_id,
                    unlock_level: 1,
                },
            )
            .expect("目标种族刚定义过");
        let mut trait_table = TraitTable::new();
        trait_table
            .define(
                trait_id,
                crate::trait_def::TraitAttrs {
                    display_name_key: NamespacedId::parse("yourmod:trait.display")
                        .expect("合法标识符"),
                    granted_skills: Vec::new(),
                    stat_modifiers: Vec::new(),
                    rule_modifiers: vec![
                        ll_sim::rule_modifier::RuleModifier::InspectionSuspicion {
                            multiplier_permille,
                        },
                    ],
                    granted_resource_pools: Vec::new(),
                },
            )
            .expect("首次定义恒成功");
        BehaviorRuleCatalogs {
            race: race_table,
            class: ClassTable::new(),
            traits: trait_table,
            items: ItemTable::new(),
        }
    }

    /// 空快照上任何目标都「与常人无异」——`actor-inspection-suspicion`
    /// 的降级值，也是本仓库既有那两条卫兵测试（它们传
    /// `BehaviorRuleCatalogs::default()`）行为不变的依据。
    #[test]
    fn 空内容表上盘查意愿恒为与常人无异() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        let (world, actor) = test_world_with_race(race);
        let catalogs = BehaviorRuleCatalogs::default();

        // Act
        let permille = catalogs.suspicion_permille(&world, actor);

        // Assert
        assert_eq!(permille, INSPECTION_SUSPICION_SCALE);
    }

    /// 真实声明了这条被动的实体给出声明值——反例是上一条。
    #[test]
    fn 声明了盘查意愿的实体给出声明的乘数() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let race = interner.intern(NamespacedId::parse("yourmod:cutpurse").expect("合法标识符"));
        let (world, actor) = test_world_with_race(race);
        let catalogs = suspicion_catalogs(race, 200);

        // Act
        let permille = catalogs.suspicion_permille(&world, actor);

        // Assert
        assert_eq!(permille, 200);
    }

    /// 端到端：真实脚本源码经 `ScriptEngine::load_source` 调用
    /// `actor-inspection-suspicion`，不是直接在 Rust 里调
    /// `suspicion_permille`——理由同
    /// `注册后脚本能调用self_has_profession判断当前职业`。
    ///
    /// 脚本拿到的目标句柄来自 `self-handle`（本例里观察者与被观察者
    /// 是同一个实体，本测试只关心「脚本真的能把一个句柄传进这个新
    /// 函数并拿回一个整数」这件事；「观察者与被观察者是两个实体」
    /// 那条语义由 `crates/ll-mod/tests/example_mod_rogue_passives.rs`
    /// 用真实 `guard-ai-tree` 覆盖）。
    #[test]
    fn 注册后脚本能调用actor_inspection_suspicion拿到千分比() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let race = interner.intern(NamespacedId::parse("yourmod:cutpurse").expect("合法标识符"));
        let (world, actor) = test_world_with_race(race);
        let mut engine = ScriptEngine::new();
        ll_script::api::actor::register(&mut engine);
        register_inspection_suspicion_api(&mut engine, suspicion_catalogs(race, 200));
        engine
            .load_source(
                r#"(define (probe) (actor-inspection-suspicion (self-handle)))"#.to_string(),
            )
            .unwrap();

        // Act
        let result = with_active_world_for(&world, || {
            set_active_actor(actor);
            let result = engine.call_raw("probe", Vec::new());
            clear_active_actor();
            result
        });

        // Assert
        assert_eq!(result, Ok(steel::rvals::SteelVal::IntV(200)));
    }
}
