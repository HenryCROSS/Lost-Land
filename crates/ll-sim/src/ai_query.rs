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
use ll_world::sight_residency::fov_neighborhood_resident;
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
///
/// # 观察者所需的地形不在内存里时返回 `None`（不是崩溃）
///
/// 常驻区块集合**只围着玩家维护**（`ll_game::app` 的
/// `maintain_streaming`，半径 2 个区块），而 `advance_ai` 对常驻情况
/// 零过滤：时间轴弹出谁就结算谁。于是一个离屏 NPC 完全可能在自己脚下
/// 的区块早已被 LRU 驱逐之后，还照样跑到这里来算一次 FOV。此前的后果
/// 是所有者实机撞到的那次崩溃（`SurfaceWindow` 的常驻前置被违反，游戏
/// 当场退出）。
///
/// 现在先用
/// [`fov_neighborhood_resident`] 问一句「这次 FOV 会碰到的区块都在内存里吗」，不都在就**根本不构造
/// [`SurfaceWindow`]**，直接返回 `None`：看不见就是看不见（ADR 0015
/// 「查不到就是查不到」）。判据落在这里而不是把 `SurfaceWindow` 的
/// panic 改哑——那个 panic 守的是渲染路径的纪律，改哑等于永久拆掉它，
/// 完整论证见 `ll_world::sight_residency` 模块文档。
///
/// 这也不是新发明：`crate::resolve` 的 `resolve_move` 在目的地区块非
/// 常驻时早就静默作废，且所有者在批次 1 里明确裁定「不改」。移动已经
/// 这么降级了，感知跟着降级是一致的。
///
/// **仍未解决的是另一个问题**：离屏 NPC 到底该不该被逐个结算。那属于
/// P9 的 LOD 范围（规格 §9），本函数只保证「被结算到时不会把游戏搞
/// 崩」，不改 `advance_ai` 的结算范围。
pub fn nearest_visible_actor(
    world: &WorldState,
    self_id: EntityId,
    radius: u32,
) -> Option<EntityId> {
    let me = world.actors.get(self_id)?;
    if !fov_neighborhood_resident(&world.terrain, me.pos, radius) {
        return None;
    }
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
/// 这张表是**内容声明**（有向）的原样抄录，编年史层的战争推演直接按
/// 这个方向读它。**实体级撞格路由不是**——它对称化之后按「任一方向
/// ≥ 5」判定，见 [`declared_hostile`] 文档「内容声明有向、战斗判定
/// 对称」一节。两处口径不同是刻意的，不是漏改。
///
/// 即：撞格路由这一层，**哥布林与矮人矿邑互相敌对（6 把 3 拖了上来）、
/// 哥布林与无文化者互相敌对；农庄与石砦跟谁都不敌对**（它们那一侧
/// 一条声明都没有，哥布林朝它们的 4 也够不着阈值）。矿邑对哥布林那个
/// 刻意写低的 3 仍然在编年史层起作用——它决定的是「矮人会不会主动
/// 出兵讨伐」，不是「迎面撞上会不会拔刀」。
pub const HOSTILE_CULTURE_THRESHOLD: u32 = 5;

/// 两个实体之间是否存在**已声明的**对立关系——[`crate::turn`] 把一次
/// 「走进对方那一格」路由成攻击还是互换位置，问的是这个问题。
///
/// `a` 是**发起者**（走进对方那一格的那一位），`b` 是**占着那一格的
/// 那一位**。这个约定只用来读懂调用点，**判据本身对调两侧结果相同**
/// （见下文「文化判据是**对称**的」一节）——势力判据那一半的
/// `has_faction(a) || has_faction(b)` 也是对称的，[`is_hostile`] 则
/// 有向，但它只在势力归属有生产者之后才有机会被走到。
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
/// # 文化判据是**对称**的——所有者裁定，推翻了本函数此前的实现
///
/// **所有者原话**：「关于矮人那个，整体机制应该是**只要有一方处于敌对
/// 状态，另一方也会发起攻击**」。
///
/// 判据因此取两个方向的最大值：
///
/// ```text
/// 文化判据(a, b) = max(hostility(文化(a), 文化(b)), hostility(文化(b), 文化(a)))
///                  >= HOSTILE_CULTURE_THRESHOLD
/// ```
///
/// 没有文化的那一方（今天的玩家、以及没有文化的据点物化出来的 NPC）
/// **用「无文化」哨兵索引参与两个方向的查询**——它自己声明不了敌意
/// （`hostility(哨兵, 任何文化)` 查不到即 0），但对方朝「无文化」的那条
/// 声明照样能把它拖进敌对。「敌对是被声明出来的关系」这条原则一个字
/// 没改：`goblin_warband → cultureless` 的 6 是内容里真真正正写出来的
/// 一条声明，只不过它声明的对象是「缺席」本身。
///
/// # 内容声明有向、战斗判定对称——两者不矛盾
///
/// 这是本仓库里唯一容易读错的一处分层，必须写清楚：
///
/// | 层 | 方向性 | 谁在用 | 回答的问题 |
/// |---|---|---|---|
/// | **内容声明**（[`ll_world::culture::CultureAttrs::hostility`]） | **有向**，刻意允许不对称 | 内容作者 | 「A 有多恨 B」 |
/// | **编年史层战争推演**（`ll_world::chronicle` 的 `hostility_between`/`wage_wars`/`pick_target`） | **有向**，直接读上面那张表 | 历史模拟 | 「A 会不会出兵打 B」 |
/// | **实体级撞格路由**（本函数） | **对称**，取两个方向最大值 | 回合结算 | 「我走进你这一格意味着什么」 |
///
/// `mining_hold → goblin_warband` 那个刻意写低的 3 **一个字都没改，也
/// 没有失去意义**：它照样决定矮人矿邑「会不会主动出兵讨伐哥布林」（编
/// 年史层，答案仍然是「不太会」）。它不再决定的只有一件事——一个矮人
/// 矿工在地图上迎面撞见哥布林时会不会拔刀。这两个问题本来就该有不同的
/// 答案：出兵是集体决策，遭遇战不是。
///
/// # 为什么必须对称（而不是像此前那样按发起者取向）
///
/// 三条理由，任一条单独成立：
///
/// 1. **所有者裁定**（上引原话）。这是首要理由。
/// 2. **撞格路由必须给出对称答案**。一次遭遇不可能「我换你、你砍我」。
///    此前的有向实现之所以没当场自相矛盾，靠的是「互换只对受控实体
///    开放」这条外部约束——判据本身的自洽性挂在另一个模块的实现细节
///    上，是脆的。
/// 3. **有向判据会让被打的一方站着挨打**。哥布林对矮人矿工声明了 6，
///    矮人对哥布林只声明 3；有向判据下，哥布林砍矮人，矮人撞回去却是
///    一次失败的移动——它连还手都做不到。
///
/// # 它在**现有内容**上的可观测后果（对称之后）
///
/// 见 [`HOSTILE_CULTURE_THRESHOLD`] 文档那张表的**对称化**读法：任一
/// 方向 ≥ 5 即敌对。
///
/// | 配对（无序） | 两个方向 | 敌对？ |
/// |---|---|---|
/// | 无文化 ↔ `goblin_warband` | 0 / 6 | **敌对** |
/// | `mining_hold` ↔ `goblin_warband` | 3 / 6 | **敌对**（本次裁定改变的就是这一格） |
/// | `farmstead` ↔ `goblin_warband` | 0 / 4 | 不敌对 |
/// | `stonecutters` ↔ `goblin_warband` | 0 / 4 | 不敌对 |
/// | 无文化 ↔ `farmstead` | 0 / 0 | 不敌对 |
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

/// 文化判据那一半，见 [`declared_hostile`] 文档「文化判据是**对称**
/// 的」一节。
///
/// 表里没记「无文化」哨兵索引（没注册过、或这个世界压根没有文化这一
/// 层）时直接返回假：没有哨兵就无法把「缺席」翻译成一个可查的目标，
/// 这时诚实地什么都不判，而不是随便挑一个索引冒充。
///
/// 两个实体各自的文化（没有就是哨兵）算出来之后，**两个方向各查一次、
/// 取最大值**——所有者裁定「只要有一方处于敌对状态，另一方也会发起
/// 攻击」。因此本函数对调 `a`/`b` 之后结果恒等，撞格路由拿到的答案与
/// 「谁先动」无关。
fn culture_declares_hostile(a: &Agent, b: &Agent, cultures: Option<&CultureTable>) -> bool {
    let Some(cultures) = cultures else {
        return false;
    };
    let Some(cultureless) = cultures.cultureless() else {
        return false;
    };
    let cultureless = CultureKind::from_index(cultureless);
    let mover = culture_of(a).unwrap_or(cultureless);
    let target = culture_of(b).unwrap_or(cultureless);
    let score = cultures
        .hostility(Some(mover), Some(target))
        .max(cultures.hostility(Some(target), Some(mover)));
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
            // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
            gender: ll_world::entity::Gender::default(),
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
            home: None,
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
            buildings: ll_world::building::bare_building_fixture(),
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
    fn 只有一个方向声明了敌意时对调两侧结果相同() {
        // **所有者裁定的核心断言**：「只要有一方处于敌对状态，另一方
        // 也会发起攻击」。`table_with_hostility` 只声明了
        // `attacker → defender`，反方向一条都没有——对调之后仍然敌对，
        // 靠的正是 `culture_declares_hostile` 里那个 `.max(..)`。
        //
        // 与「双方都无文化」那几条不同：这里两侧**都有真实文化**，
        // 哨兵完全不参与，因此这条钉的是判据本身的对称性，不是哨兵
        // 回退路径。
        //
        // 故意改坏的反例（人工核验）：删掉 `.max(cultures.hostility(
        // Some(target), Some(mover)))`，第二条断言当场变红。
        // Arrange
        let (table, attacker, defender) = table_with_hostility(HOSTILE_CULTURE_THRESHOLD);
        let a = agent_of_culture(Some(attacker));
        let b = agent_of_culture(Some(defender));

        // Act & Assert
        assert!(declared_hostile(&a, &b, Some(&table)), "声明方向本来就敌对");
        assert!(
            declared_hostile(&b, &a, Some(&table)),
            "被声明的一方撞回去同样敌对——这一条是本次裁定改变的东西"
        );
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
        // `declared_hostile` 文档「文化判据是**对称**的」一节：发起者
        // （今天的玩家）自己声明不了任何敌意（`hostility(哨兵, 任何
        // 文化)` 恒 0），但对方朝「无文化」哨兵的那条声明照样把它拖进
        // 敌对。两个方向都断言，钉住的是判据的**对称性**本身。
        //
        // 故意改坏的反例（人工核验）：把 `culture_declares_hostile` 的
        // `.max(..)` 那一项删掉、只留 `hostility(mover, target)`，
        // 下面第一条断言当场变红。
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
                    buildings: ll_world::building::bare_building_fixture(),
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
        // 满值。见 ADR 0022 的反例验证。
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
                    buildings: ll_world::building::bare_building_fixture(),
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

    /// 造一个**多区块、只预热了出生点邻域**的世界：16×16 个区块、
    /// 边长 48（世界 768×768 格）。`WorldState::new` 只预热出生点周围
    /// 5×5 个区块，其余区块从未常驻过——这正是所有者实机崩溃时的世界
    /// 形态（玩家一路走过去物化了远处据点的 NPC，常驻集合随后把那些
    /// NPC 脚下的区块驱逐掉，它们却照样被时间轴弹出来跑行为树）。
    fn streamed_world() -> (WorldState, ll_world::terrain::BaseTerrainIds) {
        let zone_count = ll_core::torus::TorusSize::new(16, 16).expect("16x16 是合法尺寸");
        let layout = ll_world::zone::ZoneLayout::new(48, zone_count).expect("48 满足全部对齐约束");
        let (terrain_ids, terrain_table) = ll_world::terrain::base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        let world = WorldState::new(
            layout,
            &ll_world::generate::GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件");
        (world, terrain_ids)
    }

    /// 在给定坐标放一个实体，其余字段取默认。
    fn spawn_at(world: &mut WorldState, x: i32, y: i32) -> EntityId {
        let pos = world.size.wrap(x, y);
        let mut agent = agent_with_factions(&[]);
        agent.pos = pos;
        agent.current_space = ll_world::space::Space::surface(
            world.terrain.layout().tile_to_zone(pos).0,
            ll_core::ident::ContentIndex::default(),
        );
        world.actors.spawn(agent)
    }

    #[test]
    fn 观察者脚下的区块未常驻时看不见任何人而不是崩溃() {
        // 这条复现的是所有者实机撞到的那次崩溃：
        //   SurfaceWindow 假定视野范围内的区块都已经常驻，
        //   TorusPos { x: 1008, y: 0 } 所属区块尚未加载
        // 一个远处据点的卫兵被时间轴弹出来跑行为树，行为树调
        // `nearest_visible_actor`，它在观察者**自己的位置**上算 FOV，
        // 而那个位置所属的区块早已被 LRU 驱逐。
        //
        // Arrange：把观察者放进一个从未预热过的区块（区块 (8, 8)）。
        let (mut world, _ids) = streamed_world();
        let observer = spawn_at(&mut world, 8 * 48 + 24, 8 * 48 + 24);
        spawn_at(&mut world, 8 * 48 + 26, 8 * 48 + 24);
        let observer_zone = world
            .terrain
            .layout()
            .tile_to_zone(world.actors.get(observer).expect("刚放进去").pos)
            .0;
        assert!(
            !world.terrain.is_resident(observer_zone),
            "前置：观察者脚下的区块必须不常驻，否则这条用例复现不了任何东西"
        );

        // Act & Assert：看不见就是看不见（ADR 0015），不是崩溃。
        assert_eq!(
            nearest_visible_actor(&world, observer, NEARBY_ACTOR_VIEW_RADIUS),
            None
        );
    }

    #[test]
    fn 视野半径伸进相邻的非常驻区块时同样安全返回空() {
        // **最容易漏的那一半**：只判观察者脚下那一格是不够的。观察者
        // 站在常驻区块的边缘，FOV 会一路查到隔壁那个没常驻的区块里去。
        //
        // Arrange：出生点邻域预热半径是 2 个区块，于是区块 2 常驻、
        // 区块 3 不常驻。把观察者放在区块 (2, 0) 的最后一列。
        let (mut world, _ids) = streamed_world();
        let observer = spawn_at(&mut world, 2 * 48 + 47, 24);
        spawn_at(&mut world, 2 * 48 + 45, 24);
        let layout = *world.terrain.layout();
        let observer_zone = layout
            .tile_to_zone(world.actors.get(observer).expect("刚放进去").pos)
            .0;
        assert!(
            world.terrain.is_resident(observer_zone),
            "前置：观察者脚下这一个区块本身必须是常驻的"
        );
        assert!(
            !world
                .terrain
                .is_resident(layout.tile_to_zone(world.size.wrap(3 * 48, 24)).0),
            "前置：视野要伸进去的那个相邻区块必须不常驻"
        );

        // Act & Assert
        assert_eq!(
            nearest_visible_actor(&world, observer, NEARBY_ACTOR_VIEW_RADIUS),
            None
        );
    }

    #[test]
    fn 常驻齐全时依然照常看得见相邻的目标() {
        // 守住「降级只在该降级的时候发生」：常驻齐全时行为一个字没变。
        // Arrange：两人都站在出生点所在区块里，视野方框只覆盖已预热的
        // 区块。
        let (mut world, ids) = streamed_world();
        let observer = spawn_at(&mut world, 0, 0);
        let target = spawn_at(&mut world, 1, 0);
        world.terrain.set_terrain(world.size.wrap(0, 0), ids.grass);
        world.terrain.set_terrain(world.size.wrap(1, 0), ids.grass);

        // Act & Assert
        assert_eq!(
            nearest_visible_actor(&world, observer, NEARBY_ACTOR_VIEW_RADIUS),
            Some(target)
        );
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
