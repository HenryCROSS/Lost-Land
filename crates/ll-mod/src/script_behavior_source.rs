//! 接上「AI 真的做出决策」最后一环：[`ScriptBehaviorSource`] 是
//! `ll_sim::behavior::BehaviorTreeSource` 目前唯一的真实实现——持有一个
//! 已经装载了行为树脚本、注册好全部运行期查询 API 的
//! `ll_script::host::ScriptEngine`，`decide` 调用 `ll_script::behavior::tick`
//! 求值一次，再用 `ll_script::api::intent::parse_intent` 把结果翻译成
//! `ll_sim::intent::Intent`。
//!
//! # 四步链路，本文件把第四步接上
//!
//! ```text
//! ① 注册技能……      ll_mod::script_skill_api（既有）
//! ② 表达成 Intent     ll_script::api::intent::parse_intent（既有，此前零调用点）
//! ③ 结算扣血/致死     ll_sim::resolve::resolve_use_skill（既有）
//! ④ AI 真的做出决策   本文件——把①②③串成一条真实可跑的链路
//! ```
//!
//! 断点具体是什么、为什么断在这里，见 `ll_sim::behavior`/
//! `ll_script::behavior` 两处模块文档；本文件负责把三层（`ll-script`
//! 的求值器与查询 API、`ll-mod` 的 `Registry`/`SkillTable`）真正粘合成
//! 一个可以被 `ll_sim::behavior::resolve_ai_turn` 调用的
//! `BehaviorTreeSource` 实现。

use std::collections::BTreeMap;

use ll_core::ident::ContentIndex;
use ll_core::rng::DetRng;
use ll_script::host::{ScriptEngine, ScriptError};
use ll_sim::behavior::BehaviorTreeSource;
use ll_sim::intent::Intent;
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

use crate::registry::Registry;
use crate::script_behavior_api::{
    register_profession_check_api, register_skill_ready_api, skill_index_snapshot,
};

/// 装载了一棵行为树、能为脚本管理的实体产出真实 [`Intent`] 的决策来源。
pub struct ScriptBehaviorSource {
    engine: ScriptEngine,
    /// 每次 `decide` 都调用的零参入口函数名——见
    /// `ll_script::behavior::tick` 文档「树是怎么来的」。
    tree_entry_fn: String,
    /// 技能命名空间字符串 → `ContentIndex` 的一次性快照，供
    /// `parse_intent` 的 `resolve_skill` 回调复用——见
    /// `crate::script_behavior_api` 模块文档「为什么用一次性快照」。
    skill_index: BTreeMap<String, ContentIndex>,
    /// 喂给 `DetRng::for_entity` 的世界种子（约束 C3：全部随机性必须
    /// 来自按实体 ID 派生的确定性流，禁止全局 RNG 流）。
    world_seed: u64,
}

impl ScriptBehaviorSource {
    /// 装载 `source`（行为树脚本源码，见
    /// `mods/example_mod/behavior.scm`），注册全部运行期查询 API
    /// （`api::query`/`api::actor`/`api::rng`/`api::state`/`skill-ready?`），
    /// 返回一个可以立即用于 [`BehaviorTreeSource::decide`] 的实例。
    ///
    /// `mod_namespace` 传给 `api::state::register`（脚本状态存储的
    /// 命名空间隔离，见其模块文档）；`registry` 用于一次性解析
    /// `skill-ready?`/`parse_intent` 都要用到的技能字符串 →
    /// `ContentIndex` 映射（同一份映射两处复用，见
    /// [`skill_index_snapshot`] 文档）。
    pub fn new(
        source: &str,
        tree_entry_fn: impl Into<String>,
        mod_namespace: impl Into<String>,
        registry: &Registry,
        world_seed: u64,
    ) -> Result<Self, ScriptError> {
        let mut engine = ScriptEngine::new();
        ll_script::api::query::register(&mut engine);
        ll_script::api::actor::register(&mut engine);
        ll_script::api::rng::register(&mut engine);
        ll_script::api::state::register(&mut engine, mod_namespace);
        let skill_index = skill_index_snapshot(registry);
        register_skill_ready_api(&mut engine, skill_index.clone());
        // 卫兵职业接线批次：同一份快照（见 register_profession_check_api
        // 文档「为什么复用 skill_index_snapshot」一节）再喂给
        // self-has-profession?，让行为树能判断"我是不是卫兵职业"。
        register_profession_check_api(&mut engine, skill_index.clone());
        engine.load_source(source.to_string())?;
        Ok(Self {
            engine,
            tree_entry_fn: tree_entry_fn.into(),
            skill_index,
            world_seed,
        })
    }
}

impl BehaviorTreeSource for ScriptBehaviorSource {
    /// 求值一次行为树：设置活跃世界/活跃实体/活跃随机流三个调用窗口
    /// 指针（与 `ll_script::api::query`/`api::actor`/`api::rng` 各自的
    /// 模块文档同一条调用约定——设置、求值、清空，缺一步都会让下一次
    /// 调用张冠李戴），跑 [`ll_script::behavior::tick`]，再用
    /// [`ll_script::api::intent::parse_intent`] 把结果翻成 [`Intent`]。
    ///
    /// # C1：这里不写世界
    ///
    /// [`ll_script::api::query::with_active_world_for`] 只接收
    /// `world: &WorldState`（共享引用）——本函数自身没有、也不可能拿到
    /// `&mut WorldState`，脚本查询函数同样只能读。真正的写入仍然只
    /// 发生在调用方对 [`ll_sim::resolve::resolve_with_skills_and_quests`]
    /// 产出的 `Effect` 调用 `apply` 之后，本函数只负责「决策」这一步。
    ///
    /// # 为什么不直接调用 `ll_script::api::query::set_active_world`
    ///
    /// 那是 `unsafe fn`——本 crate 继承工作区 `unsafe_code = "forbid"`，
    /// 没有能力写 `unsafe` 块。`with_active_world_for` 把设置/清空这
    /// 一对调用封装成一个安全函数，`unsafe` 完全留在 `ll-script`
    /// 内部，见其文档。
    ///
    /// # C3：随机性走 `DetRng::for_entity`
    ///
    /// `event_counter` 取当前世界时钟——同一个实体在同一个世界时刻
    /// 只会决策一次（回合制，见 `ll_sim::timeline` 模块文档），用世界
    /// 时钟当计数器天然满足「同一实体不同决策事件要给出不同的流」这条
    /// 要求，且不需要额外在 `Agent` 上新增一个「决策计数器」字段——见
    /// `ll_sim::behavior` 模块文档同一条纪律。
    fn decide(&mut self, world: &WorldState, actor: EntityId) -> Option<Intent> {
        let rng = DetRng::for_entity(self.world_seed, actor.as_u64(), world.clock.0 as u64);
        let engine = &mut self.engine;
        let tree_entry_fn = &self.tree_entry_fn;
        let result = ll_script::api::query::with_active_world_for(world, || {
            ll_script::api::actor::set_active_actor(actor);
            ll_script::api::rng::set_active_rng(rng);

            let result = ll_script::behavior::tick(engine, tree_entry_fn);

            ll_script::api::rng::clear_active_rng();
            ll_script::api::actor::clear_active_actor();
            result
        });

        let skill_index = &self.skill_index;
        result.and_then(|value| {
            ll_script::api::intent::parse_intent(actor, &value, &|name| {
                skill_index.get(name).copied()
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use ll_core::ident::{Interner, NamespacedId};
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_sim::quest::NoQuests;
    use ll_sim::resolve::resolve_with_skills_and_quests;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::space::Space;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    use crate::skill::{ResourceCost, ResourceKind, SkillAttrs, SkillEffect, SkillTable};

    use super::*;

    /// 与 `mods/example_mod/behavior.scm` 内容一致（见其文件头注释的
    /// 交叉引用）——测试用内联字符串而不是读磁盘文件，理由同本仓库
    /// 其余脚本装载测试（`ll-mod::pipeline` 等模块的既有惯例：真实的
    /// `.scm` 文件是给读者看的示例，测试用内联源码避免依赖 `cargo test`
    /// 的工作目录假设）。
    const BEHAVIOR_SCRIPT: &str = r#"
        (define (goblin-try-skill)
          (let ([enemy (nearby-enemy)])
            (if (and enemy (skill-ready? "examplemod:frostbolt"))
                (list 'use-skill "examplemod:frostbolt" enemy)
                #f)))

        (define (goblin-try-attack)
          (let ([enemy (nearby-enemy)])
            (if enemy (list 'attack enemy) #f)))

        (define (goblin-try-approach)
          (let ([enemy (nearby-enemy)])
            (if enemy (list 'move (direction-toward enemy)) 'wait)))

        (define (goblin-ai-tree)
          (quote (selector
                   (goblin-try-skill)
                   (goblin-try-attack)
                   (goblin-try-approach))))
        "#;

    fn test_world() -> WorldState {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
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

    fn spawn_agent_at(
        world: &mut WorldState,
        x: i32,
        y: i32,
        unlocked: Vec<ContentIndex>,
    ) -> EntityId {
        let mut interner = Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(x, y);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        world.actors.spawn(Agent {
            pos,
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
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ll_core::ident::ContentIndex::default()),
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
        })
    }

    /// 端到端验收：真实的行为树求值 → `parse_intent` → `resolve` →
    /// `Effect` → `apply` 全链路——敌人（有一个已解锁、未冷却的技能）
    /// 发现附近的玩家，选中技能而不是普通攻击，打中之后玩家生命值
    /// 真的下降，技能冷却真的写回 `Agent`。
    #[test]
    fn 敌人的行为树选中技能并真的打伤玩家() {
        // Arrange：注册表 + 一个真实技能定义（冰霜箭：耗 5 点法力，
        // 冷却 20 tick，造成 15 点伤害）。
        let mut registry = Registry::new();
        let skill_id = NamespacedId::parse("examplemod:frostbolt").expect("合法标识符");
        let skill_index = registry.intern(skill_id);
        let mut skill_table = SkillTable::new();
        skill_table
            .define(
                skill_index,
                SkillAttrs {
                    owning_class: None,
                    prerequisites: Vec::new(),
                    cooldown_ticks: 20,
                    resource_cost: ResourceCost::Amount(ResourceKind::Mana, 5),
                    effect: SkillEffect::DealDamage { base: 15 },
                },
            )
            .expect("测试声明内部自洽");

        let mut world = test_world();
        let enemy = spawn_agent_at(&mut world, 5, 5, vec![skill_index]);
        let player = spawn_agent_at(&mut world, 7, 5, Vec::new());
        let player_health_before = world.actors.get(player).expect("刚生成必然存在").health;

        let mut source = ScriptBehaviorSource::new(
            BEHAVIOR_SCRIPT,
            "goblin-ai-tree",
            "examplemod",
            &registry,
            1,
        )
        .expect("测试脚本应当能通过白名单并装载成功");

        // Act：① 行为树决策。
        let intent = source
            .decide(&world, enemy)
            .expect("附近有可攻击目标且技能可用，应当产出决策");

        // Assert：① 选中的是「使用技能」而不是普通攻击或移动——技能
        // 优先级验证。
        assert_eq!(
            intent,
            Intent::UseSkill {
                actor: enemy,
                skill: skill_index,
                target: Some(player),
            }
        );

        // Act：③ 结算。
        let effects = resolve_with_skills_and_quests(&world, &intent, &skill_table, &NoQuests);

        // Assert：③ 产出了真实的伤害效果与冷却写入。
        assert!(effects.iter().any(|effect| matches!(
            effect,
            ll_sim::effect::Effect::Damage { target, amount }
                if *target == player && *amount == 15
        )));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            ll_sim::effect::Effect::SetSkillCooldown { actor, skill, .. }
                if *actor == enemy && *skill == skill_index
        )));

        // Act：apply——唯一写入口。
        for effect in &effects {
            ll_sim::apply::apply(&mut world, effect);
        }

        // Assert：玩家生命值真的下降，敌人的技能冷却真的写回。
        let player_after = world.actors.get(player).expect("刚生成必然存在");
        assert_eq!(player_after.health, player_health_before - 15);
        let enemy_after = world.actors.get(enemy).expect("刚生成必然存在");
        assert!(enemy_after.skill_cooldowns.contains_key(&skill_index));
    }

    /// 技能不可用（未解锁）时，行为树应当降级为普通攻击——证明
    /// selector 的分支优先级真的在起作用，不是恰好只测了「技能可用」
    /// 这一条路径。
    #[test]
    fn 技能未解锁时行为树降级为普通攻击() {
        // Arrange：敌人没有任何 unlocked_skills。
        let registry = Registry::new();
        let mut world = test_world();
        let enemy = spawn_agent_at(&mut world, 5, 5, Vec::new());
        let player = spawn_agent_at(&mut world, 7, 5, Vec::new());

        let mut source = ScriptBehaviorSource::new(
            BEHAVIOR_SCRIPT,
            "goblin-ai-tree",
            "examplemod",
            &registry,
            1,
        )
        .expect("测试脚本应当能通过白名单并装载成功");

        // Act
        let intent = source
            .decide(&world, enemy)
            .expect("附近有目标，即使技能不可用也应当降级为攻击");

        // Assert
        assert_eq!(
            intent,
            Intent::Attack {
                actor: enemy,
                target: player
            }
        );
    }

    /// 附近没有任何目标时，行为树应当降级为移动/等待——三层 fallback
    /// 的最后一层。
    #[test]
    fn 附近没有目标时行为树降级为等待() {
        // Arrange：敌人附近（NEARBY_ENEMY_RANGE_SQ 之外）没有其他实体。
        let registry = Registry::new();
        let mut world = test_world();
        let enemy = spawn_agent_at(&mut world, 5, 5, Vec::new());

        let mut source = ScriptBehaviorSource::new(
            BEHAVIOR_SCRIPT,
            "goblin-ai-tree",
            "examplemod",
            &registry,
            1,
        )
        .expect("测试脚本应当能通过白名单并装载成功");

        // Act
        let intent = source.decide(&world, enemy).expect("兜底分支恒产出决策");

        // Assert
        assert_eq!(intent, Intent::Wait { actor: enemy });
    }
}
