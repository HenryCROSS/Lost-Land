//! AI 决策用的**只读世界查询原语**：找目标、判方向。
//!
//! # 为什么这些函数住在 `ll-sim`
//!
//! 它们此前是 `ll_script::api::actor` 里的私有函数，只给 Steel 查询
//! 函数（`nearby-enemy`/`nearby-actor-in-view`/`direction-toward`）当
//! 实现。行为树本身搬进 Rust（见 [`crate::behavior`] 与
//! `ll_mod::native_behavior`）之后，同一批函数出现了第二个调用方，而
//! 那个调用方在 `ll-mod`——`ll-script` 是它的依赖，不能反过来。
//!
//! 依赖链是 `ll-world ← ll-sim ← ll-script ← ll-mod`（规格 §5），
//! `ll-sim` 是两个调用方共同的上游，也是这些函数语义上该在的地方：
//! 它们的输入是 `&WorldState`、输出是 `EntityId`/[`Direction`]，一个
//! Steel 类型都不碰。`ll_script::api::actor` 现在只是一层把它们包成
//! `SteelVal` 的适配。
//!
//! # 约束
//!
//! - **ADR 0023 / C1**：本模块全部函数只接 `&WorldState`（共享引用），
//!   物理上写不了世界。它们回答「我看到了什么」，不回答「我做了什么」。
//! - **C5**：候选者遍历走 `Arena::iter_with_id`（`Vec` 支撑的固定
//!   顺序），距离相等时按 `EntityId` 升序打破平局——没有一处依赖
//!   `HashMap` 迭代顺序。
//!
//! # 已知简化（原样承继，本批次不动）
//!
//! [`nearest_hostile`] 的「附近」是固定半径的**平方距离**筛选，不是
//! 真正的 FOV 可见性；[`nearest_visible_actor`] 才走真正的 FOV。两者
//! 的差别是刻意的：前者服务「野怪扑向最近的敌人」，后者服务「卫兵
//! 看见了谁」，隔着墙的目标只有后者会漏掉。
//!
//! [`is_hostile`] 同样是粗略近似：没有任何势力归属的实体（野怪）视为
//! 对谁都敌对。真正的声望/关系矩阵是
//! `knowledge/design/society-and-affiliation.md` 描述的 P8 范围。

use ll_world::culture::{CultureKind, CultureTable};
use ll_world::entity::{AffiliationKind, Agent, EntityId, OrgRef};
use ll_world::fov::compute_fov;
use ll_world::state::WorldState;
use ll_world::surface_store::SurfaceWindow;

use crate::intent::Direction;

/// [`nearest_hostile`] 的「附近」平方距离阈值——半径约 10 格。
pub const NEARBY_ENEMY_RANGE_SQ: i64 = 100;

/// [`nearest_visible_actor`] 默认的视野半径。
///
/// 与 `crate::resolve::EXPLORATION_SIGHT_RADIUS`（玩家探索标记）及设计
/// 文档 `DEFAULT_NPC_BASE_SIGHT_RADIUS` 建议值同一个量级（12），不是
/// 巧合：这是本代码库目前对「一个前景实体大致能看多远」的既有拍板值。
pub const NEARBY_ACTOR_VIEW_RADIUS: u32 = 12;

/// 找出离 `self_id` 最近、且对它敌对、且落在 [`NEARBY_ENEMY_RANGE_SQ`]
/// 内的实体；没有就是 `None`。
pub fn nearest_hostile(world: &WorldState, self_id: EntityId) -> Option<EntityId> {
    let me = world.actors.get(self_id)?;
    world
        .actors
        .iter_with_id()
        .filter(|(id, _)| *id != self_id)
        .filter(|(_, other)| is_hostile(me, other))
        .filter_map(|(id, other)| {
            let (dx, dy) = world.size.delta(me.pos, other.pos);
            let dist_sq = i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy);
            (dist_sq <= NEARBY_ENEMY_RANGE_SQ).then_some((dist_sq, id))
        })
        // 距离相等时按 EntityId 升序打破平局（C5）。
        .min_by_key(|&(dist_sq, id)| (dist_sq, id))
        .map(|(_, id)| id)
}

/// 找出离 `self_id` 最近、且真的落在它 FOV 内的实体（**不看敌对
/// 关系**）；范围外或不存在时返回 `None`。
///
/// 两段式过滤：`world.size.chebyshev` 粗筛（`O(1)`/候选者）+
/// `VisibleSet::contains` 成员测试，只对观察者自己的位置算一次
/// [`compute_fov`]。隔着墙的目标因此找不到。
pub fn nearest_visible_actor(
    world: &WorldState,
    self_id: EntityId,
    radius: u32,
) -> Option<EntityId> {
    let me = world.actors.get(self_id)?;
    let visible = compute_fov(
        &SurfaceWindow::new(&world.terrain),
        &world.terrain_table,
        me.pos,
        radius,
    );
    world
        .actors
        .iter_with_id()
        .filter(|(id, _)| *id != self_id)
        .filter_map(|(id, other)| {
            let dist = world.size.chebyshev(me.pos, other.pos);
            if dist > radius {
                return None; // 粗筛：距离已经超出半径，FOV 不可能命中。
            }
            visible.contains(other.pos).then_some((dist, id))
        })
        .min_by_key(|&(dist, id)| (dist, id))
        .map(|(_, id)| id)
}

/// 从 `from` 指向 `to` 的八向之一；任一实体不存在时返回 `None`。
pub fn direction_toward(world: &WorldState, from: EntityId, to: EntityId) -> Option<Direction> {
    let (me, them) = (world.actors.get(from)?, world.actors.get(to)?);
    let (dx, dy) = world.size.delta(me.pos, them.pos);
    Some(direction_from_delta(dx, dy))
}

/// `(dx, dy)`（环面最短带符号位移）→ 八向。
///
/// 零位移（同格）没有明确方向，任意但稳定地退化为
/// [`Direction::North`]——与本模块其余查询「意料之外的输入选一个确定
/// 值而不是 panic」同一条纪律。
pub fn direction_from_delta(dx: i32, dy: i32) -> Direction {
    match (dx.signum(), dy.signum()) {
        (0, -1) | (0, 0) => Direction::North,
        (0, 1) => Direction::South,
        (-1, 0) => Direction::West,
        (1, 0) => Direction::East,
        (1, -1) => Direction::NorthEast,
        (1, 1) => Direction::SouthEast,
        (-1, 1) => Direction::SouthWest,
        (-1, -1) => Direction::NorthWest,
        // `i32::signum` 的值域恰为 {-1, 0, 1}，上面九种组合已穷尽；
        // 保留这一分支只是让编译器确认穷尽性。
        _ => Direction::North,
    }
}

/// `b` 是否对 `a` 敌对——粗略近似，见模块文档「已知简化」。
pub fn is_hostile(a: &Agent, b: &Agent) -> bool {
    let a_factions: Vec<_> = a
        .affiliations
        .iter()
        .filter(|aff| aff.kind == AffiliationKind::Faction)
        .map(|aff| aff.org)
        .collect();
    if a_factions.is_empty() {
        return true;
    }
    !b.affiliations
        .iter()
        .any(|aff| aff.kind == AffiliationKind::Faction && a_factions.contains(&aff.org))
}

/// 文化敌意达到多少就算「已声明敌对」——项目所有者裁定：「我觉得 5
/// 也没问题。」
///
/// # 与 [`ll_world::culture::MAX_HOSTILITY`]（7）的关系
///
/// 敌意分的值域是 `0..=7`，7 是上界（再高会让战争概率的分子达到分母，
/// 把「战争是少数派」那道闸门拆掉）。阈值取 5 因此落在值域的高段：
/// 内容要表达「这两伙人见面就动手」得**特意**写一个 5 以上的数，
/// 「互相看不顺眼但不至于拔刀」那一档（3、4）留在阈值之下。
///
/// # 它在**现有内容**上的可观测后果
///
/// 这一段才是这个数字的价值所在（`mods/lostland/cultures.json5` 实测）：
///
/// | 攻方 → 守方 | 敌意 | ≥ 5？ |
/// |---|---|---|
/// | `goblin_warband` → `cultureless`（无文化） | 6 | **敌对** |
/// | `goblin_warband` → `mining_hold` | 6 | **敌对** |
/// | `goblin_warband` → `farmstead` | 4 | 不敌对 |
/// | `goblin_warband` → `stonecutters` | 4 | 不敌对 |
/// | `mining_hold` → `goblin_warband` | 3 | 不敌对 |
///
/// 即：**哥布林只对矮人矿业据点与无文化者敌对，对农庄与石砦不敌对；
/// 矮人不主动敌对哥布林。** 那份刻意写进内容的不对称（矿邑对哥布林
/// 只有 3——出兵清剿，不是不共戴天）被阈值完整保住了。
pub const HOSTILE_CULTURE_THRESHOLD: u32 = 5;

/// 两个实体之间是否存在**已声明的**对立关系——[`crate::turn`] 把一次
/// 「走进对方那一格」路由成攻击还是互换位置，问的是这个问题。
///
/// `a` 是**发起者**（走进对方那一格的那一位），`b` 是**占着那一格的
/// 那一位**；下面「文化判据」一节的方向性依赖这个约定。
///
/// `cultures` 是这个世界的文化表（连同它记着的「无文化」哨兵索引，见
/// [`CultureTable::cultureless`]）。传 `None` —— 或者传一张没注册过
/// 哨兵的表 —— 时文化判据整个不生效，只剩下面的势力判据，与本批次
/// 落地之前**逐位相同**。生产路径从 `WorldState` 的编年史句柄取表
/// （见 [`crate::turn`] 的撞格路由），没有编年史的世界（大量单元测试
/// 构造的裸世界）走的正是 `None` 这一支。
///
/// # 为什么不能直接用 [`is_hostile`]
///
/// [`is_hostile`] 回答的是另一个问题：「野怪该扑向谁」。它的兜底是
/// 「自己没有任何势力归属 → 对谁都敌对」，对一头怪物是对的，但
/// [`AffiliationKind::Faction`] 归属至今**没有任何生产者**（本批次给
/// `Agent::affiliations` 补上的第一个生产者挂的是
/// [`AffiliationKind::Culture`]，见 `ll_mod::roster::build_npc_agent`；
/// 势力播种是另一批），于是在真正能跑的游戏里 `is_hostile` 对**每一
/// 对**实体都返回真：玩家、农夫、铁匠彼此全是敌人。
///
/// 拿它当撞格路由的判据，后果是所有者实机看到的那一幕——走向一个
/// 农夫就把他砍了；若再把同一条路由接到 AI 那一侧，一整座村子会在
/// 随机游走互相撞上时当场械斗。
///
/// # 判据本身：文化在前，势力在后
///
/// ```text
/// declared_hostile(a, b) = 文化判据(a, b) || (双方至少一方有势力归属 && is_hostile(a, b))
/// ```
///
/// 文化判据**必须排在短路的前面**：`(has_faction(a) || has_faction(b))`
/// 这道闸门在势力归属零生产者的今天恒假，整条第二项因此恒假。排在它
/// 后面等于本批次是空操作。
///
/// # 文化判据的方向性——这一段推翻了本函数此前文档的一句话
///
/// **旧文档说「双方都没有任何势力归属时，他们之间不存在任何已声明的
/// 对立，不敌对」。这句话现在不再无条件成立**：一个身上什么归属都没有
/// 的玩家走向一个哥布林，双方确实都没有**势力**归属，但哥布林部落这份
/// 文化在 `cultures.json5` 里**已经声明**了「对无文化敌意 6」——那是一
/// 条真真正正被写出来的敌对声明，只不过它声明的对象是「缺席」本身。
/// 「敌对是被声明出来的关系」这条原则一个字没改，改的是「什么算一条
/// 声明」。
///
/// 具体取哪一个方向的敌意分：
///
/// - **发起者有文化** → 取 `hostility(发起者文化, 目标文化或哨兵)`。
///   这保住了内容里那份刻意的不对称：矮人矿工走向哥布林只有 3，不动
///   手；哥布林走向矮人矿工是 6，动手。
/// - **发起者没有文化**（今天的玩家，以及没有文化的据点物化出来的
///   NPC）→ 它自己声明不了任何敌意，改取**对方**朝「无文化」的那条
///   声明 `hostility(目标文化或哨兵, 哨兵)`。
///
/// # 为什么不是「两个方向取最大值」
///
/// 取最大值能保证判定对称，但会把上面那份不对称一并抹掉：矮人矿工
/// 撞哥布林会因为「哥布林对我有 6」而变成矮人主动砍人，`mining_hold →
/// goblin_warband` 那个刻意写低的 3 就再也观察不到了。
///
/// 而对称**只在可能发生互换位置的那些配对上才是必需的**——「我换你、
/// 你砍我」这种自相矛盾要成立，得有一方走互换那一支，而互换只对受控
/// 实体开放（见 [`crate::turn`] 的撞格路由「玩家优先度高于 NPC」一
/// 节）。玩家今天不挂任何归属，因此**凡是玩家参与的配对，两个方向算
/// 出来的都是同一个数**（都落在「发起者没有文化」那一支，取的都是
/// `hostility(对方文化, 哨兵)`），对称性在真正需要它的地方是成立的。
/// 两个 NPC 之间的不对称不会造成矛盾：NPC 撞上非敌对目标不互换，只是
/// 一次失败的移动。
///
/// # 这不是把同一个概念抄成两份（ADR 0021）
///
/// 两个函数回答的是两个不同的问题，答案本来就该不同：「一头怪物这一
/// 回合该扑向谁」（[`is_hostile`]）与「我走进你这一格意味着什么」
/// （本函数）。野怪照常经自己的行为树直接产出 [`crate::intent::Intent::Attack`]
/// ——它不需要、也从不经过撞格路由，因此本函数的收紧不会让任何怪物
/// 变温顺。真正的声望/关系矩阵落地（P8，
/// `knowledge/design/society-and-affiliation.md`）之后，两者都应当改
/// 去查那张矩阵，届时它们大概率会合并；在那之前把它们强行合成一个，
/// 只会让其中一个问题拿到错误答案，正是 ADR 0021 反面那一半警告的事。
pub fn declared_hostile(a: &Agent, b: &Agent, cultures: Option<&CultureTable>) -> bool {
    culture_declares_hostile(a, b, cultures)
        || ((has_faction(a) || has_faction(b)) && is_hostile(a, b))
}

/// 文化判据那一半，见 [`declared_hostile`] 文档「文化判据的方向性」。
///
/// 表里没记「无文化」哨兵索引（没注册过、或这个世界压根没有文化这一
/// 层）时直接返回假：没有哨兵就无法把「缺席」翻译成一个可查的目标，
/// 这时诚实地什么都不判，而不是随便挑一个索引冒充。
fn culture_declares_hostile(a: &Agent, b: &Agent, cultures: Option<&CultureTable>) -> bool {
    let Some(cultures) = cultures else {
        return false;
    };
    let Some(cultureless) = cultures.cultureless() else {
        return false;
    };
    let cultureless = CultureKind::from_index(cultureless);
    let target = culture_of(b).unwrap_or(cultureless);
    let score = match culture_of(a) {
        Some(mover) => cultures.hostility(Some(mover), Some(target)),
        None => cultures.hostility(Some(target), Some(cultureless)),
    };
    score >= HOSTILE_CULTURE_THRESHOLD
}

/// 这个实体身上那条 [`AffiliationKind::Culture`] 归属指向的文化，没有
/// 就是 `None`（判定侧回退到「无文化」哨兵）。
///
/// 取**第一条**而不是全部：一个实体属于多种文化在本作里还没有任何内容
/// 设计，`ll_mod::roster::build_npc_agent` 至多挂一条。真出现第二条时
/// 这里取第一条是确定的（`Vec` 顺序），不依赖任何哈希容器（约束 C5）。
fn culture_of(agent: &Agent) -> Option<CultureKind> {
    agent.affiliations.iter().find_map(|aff| {
        match (aff.kind, aff.org) {
            (AffiliationKind::Culture, OrgRef::Def(index)) => Some(CultureKind::from_index(index)),
            // `Culture` 恒走 `OrgRef::Def`（见
            // `ll_world::entity::affiliation::OrgRef` 文档），这里的
            // `_` 只是让 `match` 穷尽，不是一条真实分支。
            _ => None,
        }
    })
}

/// 这个实体有没有声明过任何 [`AffiliationKind::Faction`] 归属。
fn has_faction(agent: &Agent) -> bool {
    agent
        .affiliations
        .iter()
        .any(|aff| aff.kind == AffiliationKind::Faction)
}

/// 一个实体此刻在不在潜行。
///
/// 潜行**不改可见性**：卫兵照常看得见潜行中的目标
/// （[`nearest_visible_actor`] 一个字都没改），只是「要不要把这个人
/// 当回事」那次判定的成功率降下来，见 `ll_mod::native_behavior` 的
/// 卫兵盘查概率。
pub fn is_stealthed(world: &WorldState, target: EntityId) -> bool {
    world
        .actors
        .get(target)
        .is_some_and(|agent| agent.stealthed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::ContentIndex;

    #[test]
    fn 同格退化为北而不是恐慌() {
        // Arrange & Act & Assert
        assert_eq!(direction_from_delta(0, 0), Direction::North);
    }

    #[test]
    fn 八个方向各自映射到对应的对角或正向() {
        // Arrange & Act & Assert
        assert_eq!(direction_from_delta(0, -3), Direction::North);
        assert_eq!(direction_from_delta(0, 2), Direction::South);
        assert_eq!(direction_from_delta(-4, 0), Direction::West);
        assert_eq!(direction_from_delta(1, 0), Direction::East);
        assert_eq!(direction_from_delta(2, -2), Direction::NorthEast);
        assert_eq!(direction_from_delta(3, 5), Direction::SouthEast);
        assert_eq!(direction_from_delta(-1, 7), Direction::SouthWest);
        assert_eq!(direction_from_delta(-9, -1), Direction::NorthWest);
    }

    /// 造一个除 `affiliations` 之外全取默认值的 `Agent`——本组用例只读
    /// 这一个字段，其余字段填什么都不影响
    /// [`is_hostile`]/[`declared_hostile`] 的返回值（两者的实现只遍历
    /// `affiliations`）。
    fn agent_with_factions(factions: &[u32]) -> Agent {
        let size = ll_core::torus::TorusSize::new(64, 64).expect("64x64 是合法尺寸");
        let index = ll_core::ident::ContentIndex::default();
        Agent {
            pos: size.wrap(0, 0),
            stats: ll_world::entity::BaseStats::BASELINE,
            next_action_at: ll_core::time::Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: factions
                .iter()
                .map(|id| ll_world::entity::Affiliation {
                    kind: AffiliationKind::Faction,
                    org: ll_world::entity::OrgRef::Instance(ll_core::ident::WorldId::next(&mut {
                        *id
                    })),
                    standing: 0,
                })
                .collect(),
            wallet: 0,
            profession: index,
            goals: Vec::new(),
            race: index,
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
            current_space: ll_world::space::Space::surface(size.wrap(0, 0), index),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        }
    }

    /// 造一张只有两条文化的敌意表：`attacker` 对 `defender` 的敌意分
    /// 恰好是 `score`，另外记下一个从未 `define` 过的「无文化」哨兵
    /// 索引——形状与生产路径完全一致（本体的
    /// `mods/lostland/cultures.json5` 也是只 `intern` 不 `define`）。
    ///
    /// 返回 `(表, 攻方文化, 守方文化)`。
    fn table_with_hostility(score: u32) -> (CultureTable, ContentIndex, ContentIndex) {
        let mut interner = ll_core::ident::Interner::new();
        let mut id = |raw: &str| {
            interner.intern(ll_core::ident::NamespacedId::parse(raw).expect("合法标识符"))
        };
        let attacker = id("test:attacker");
        let defender = id("test:defender");
        let cultureless = id("test:cultureless");
        let founder = id("test:race");
        let terrain = ll_world::terrain::TerrainKind::from_index(id("test:terrain"));
        let attrs = |hostility: Vec<(ContentIndex, u32)>| ll_world::culture::CultureAttrs {
            display_name_key: ll_core::ident::NamespacedId::parse("test:name").expect("合法"),
            economy: ll_world::resource::ResourceCategory::Food,
            home_terrain: terrain,
            wall_terrain: terrain,
            founder_races: vec![(founder, 1)],
            hostility,
        };
        let mut table = CultureTable::new();
        table
            .define(attacker, attrs(vec![(defender, score)]))
            .expect("测试用文化声明合法");
        table.define(defender, attrs(Vec::new())).expect("同上");
        table.set_cultureless(cultureless);
        (table, attacker, defender)
    }

    /// 造一个只挂一条文化归属的 `Agent`。
    fn agent_of_culture(culture: Option<ContentIndex>) -> Agent {
        let mut agent = agent_with_factions(&[]);
        agent.affiliations = culture
            .map(|index| ll_world::entity::Affiliation {
                kind: AffiliationKind::Culture,
                org: OrgRef::Def(index),
                standing: 1000,
            })
            .into_iter()
            .collect();
        agent
    }

    #[test]
    fn 敌意恰好等于阈值就算敌对() {
        // 钉住 `>=` 不被写成 `>`。**这条与下一条必须成对存在**：单独
        // 一条「等于阈值算敌对」可以被「恒为真」蒙混过去。
        // Arrange
        let (table, attacker, defender) = table_with_hostility(HOSTILE_CULTURE_THRESHOLD);
        let a = agent_of_culture(Some(attacker));
        let b = agent_of_culture(Some(defender));

        // Act & Assert
        assert!(declared_hostile(&a, &b, Some(&table)));
    }

    #[test]
    fn 敌意比阈值低一分就不算敌对() {
        // Arrange
        let (table, attacker, defender) = table_with_hostility(HOSTILE_CULTURE_THRESHOLD - 1);
        let a = agent_of_culture(Some(attacker));
        let b = agent_of_culture(Some(defender));

        // Act & Assert
        assert!(!declared_hostile(&a, &b, Some(&table)));
    }

    #[test]
    fn 没有文化的发起者改取对方朝无文化的那条声明() {
        // `declared_hostile` 文档「文化判据的方向性」那两支里的第二支：
        // 发起者（今天的玩家）声明不了任何敌意，判定改看对方朝「无文
        // 化」哨兵的声明。这条同时钉住「玩家参与的配对两个方向答案
        // 相同」——对称性只在可能发生互换的那些配对上才是必需的。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let mut id = |raw: &str| {
            interner.intern(ll_core::ident::NamespacedId::parse(raw).expect("合法标识符"))
        };
        let hater = id("test:hater");
        let cultureless = id("test:cultureless");
        let founder = id("test:race");
        let terrain = ll_world::terrain::TerrainKind::from_index(id("test:terrain"));
        let mut table = CultureTable::new();
        table
            .define(
                hater,
                ll_world::culture::CultureAttrs {
                    display_name_key: ll_core::ident::NamespacedId::parse("test:name")
                        .expect("合法"),
                    economy: ll_world::resource::ResourceCategory::Food,
                    home_terrain: terrain,
                    wall_terrain: terrain,
                    founder_races: vec![(founder, 1)],
                    // 只对「无文化」声明敌意，对任何真文化都不声明。
                    hostility: vec![(cultureless, HOSTILE_CULTURE_THRESHOLD)],
                },
            )
            .expect("测试用文化声明合法");
        table.set_cultureless(cultureless);
        let drifter = agent_of_culture(None);
        let hater_agent = agent_of_culture(Some(hater));

        // Act & Assert：两个方向都敌对——这正是「不会出现我换你、你砍
        // 我」的那条保证。
        assert!(declared_hostile(&drifter, &hater_agent, Some(&table)));
        assert!(declared_hostile(&hater_agent, &drifter, Some(&table)));
    }

    #[test]
    fn 表里没记无文化哨兵时文化判据整个不生效() {
        // 诚实降级：没有哨兵就无法把「缺席」翻译成一个可查的目标，
        // 这时什么都不判，而不是随便挑一个索引冒充。
        // Arrange：同一张表，只差一次 `set_cultureless`。
        let (with_sentinel, attacker, defender) =
            table_with_hostility(ll_world::culture::MAX_HOSTILITY);
        let mut without_sentinel = CultureTable::new();
        // 重建一张形状相同、但没记哨兵的表：直接把上面那张的定义抄一遍
        // 不可行（`CultureTable` 没有导出遍历属性的接口），因此这里只
        // 需要一条「攻方对无文化敌意拉满」的定义就够——判据在拿不到
        // 哨兵时应当在读敌意分**之前**就返回假。
        let mut interner = ll_core::ident::Interner::new();
        let mut id = |raw: &str| {
            interner.intern(ll_core::ident::NamespacedId::parse(raw).expect("合法标识符"))
        };
        // **哨兵刻意 intern 在最前面**，于是它拿到的正是
        // `ContentIndex::default()`（索引 0）。这不是巧合式的夹具细节：
        // 它让「拿不到哨兵就退回默认索引」这种偷懒写法在本条里当场
        // 变红——退回默认索引恰好等于退回哨兵本身，敌意分会从 0 跳到
        // 满值。见 ADR 0018 的反例验证。
        let cultureless = id("test:cultureless");
        let hater = id("test:hater");
        let founder = id("test:race");
        let terrain = ll_world::terrain::TerrainKind::from_index(id("test:terrain"));
        without_sentinel
            .define(
                hater,
                ll_world::culture::CultureAttrs {
                    display_name_key: ll_core::ident::NamespacedId::parse("test:name")
                        .expect("合法"),
                    economy: ll_world::resource::ResourceCategory::Food,
                    home_terrain: terrain,
                    wall_terrain: terrain,
                    founder_races: vec![(founder, 1)],
                    hostility: vec![(cultureless, ll_world::culture::MAX_HOSTILITY)],
                },
            )
            .expect("测试用文化声明合法");
        let drifter = agent_of_culture(None);
        let hater_agent = agent_of_culture(Some(hater));

        // Act & Assert：没哨兵 → 不敌对。对照组是另一张**记了**哨兵的
        // 表，同一条判据在那里确实判得出敌对，证明本条不是空转。
        assert!(!declared_hostile(
            &drifter,
            &hater_agent,
            Some(&without_sentinel)
        ));
        assert!(declared_hostile(
            &agent_of_culture(Some(attacker)),
            &agent_of_culture(Some(defender)),
            Some(&with_sentinel)
        ));
    }

    #[test]
    fn 没有文化表时两个都没有势力归属的实体之间没有已声明的敌对关系() {
        // 势力这一半的退化形态：`AffiliationKind::Faction` 归属至今没有
        // 任何生产者，见 `declared_hostile` 文档。传 `None` 关掉文化
        // 判据之后，本函数就只剩这一半——这条同时是「文化判据关掉
        // 之后行为与本批次落地之前逐位相同」的证据。
        // Arrange
        let a = agent_with_factions(&[]);
        let b = agent_with_factions(&[]);

        // Act & Assert
        assert!(!declared_hostile(&a, &b, None));
        assert!(!declared_hostile(&b, &a, None));
    }

    #[test]
    fn is_hostile在双方都没有势力归属时恒为真() {
        // **这条不是在夸奖 `is_hostile`，是把它今天的退化形态钉住**：
        // 它是「野怪该扑向谁」的判据，兜底是「自己没归属就对谁都敌对」，
        // 于是在当前内容下对每一对实体都返回真。撞格路由**不能**用它，
        // 理由与替代判据见 `declared_hostile` 文档。这条一旦变红，说明
        // 势力归属终于有生产者了（或者兜底改了），届时应当重新审视
        // `declared_hostile` 是否还需要单独存在。
        // Arrange
        let a = agent_with_factions(&[]);
        let b = agent_with_factions(&[]);

        // Act & Assert
        assert!(is_hostile(&a, &b));
    }

    #[test]
    fn 分属不同势力的两人已声明敌对() {
        // Arrange
        let a = agent_with_factions(&[1]);
        let b = agent_with_factions(&[2]);

        // Act & Assert
        assert!(declared_hostile(&a, &b, None));
    }

    #[test]
    fn 同属一个势力的两人不敌对() {
        // Arrange
        let a = agent_with_factions(&[7]);
        let b = agent_with_factions(&[7]);

        // Act & Assert
        assert!(!declared_hostile(&a, &b, None));
    }

    #[test]
    fn 只有一方声明了势力时仍然按既有判据算成敌对() {
        // 有归属的一方与一个「谁都不属于」的流浪者之间存在一条真实的
        // 单向声明：这个流浪者不在我的势力里。这一支保留 `is_hostile`
        // 的既有语义不变。
        // Arrange
        let affiliated = agent_with_factions(&[3]);
        let drifter = agent_with_factions(&[]);

        // Act & Assert
        assert!(declared_hostile(&affiliated, &drifter, None));
        assert!(declared_hostile(&drifter, &affiliated, None));
    }
}
