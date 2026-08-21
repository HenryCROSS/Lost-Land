//! 真实的 schema 迁移函数：把 v1 存档主体升级到 v2。
//!
//! [`crate::migration`] 只搭了迁移链的机制骨架（「本任务只搭机制,不
//! 接入真实迁移函数」，见其模块文档），本模块是第一条真正注册进链条
//! 的迁移——补齐落地探索记忆批次带来的两处存档结构变化：
//!
//! 1. `ll_world::state::WorldState` 新增 `exploration` 字段（探索记忆，
//!    见 `ll_world::exploration` 模块文档）。
//! 2. `ll_world::interior::Interior` 新增 `origin` 字段（生成来源，
//!    ADR 0024 裁定 P5-7，见其字段文档）。
//!
//! 两处都是**在已有字段序列之间插入新字段**，`postcard` 是按声明顺序
//! 定位、不带字段名的编码格式（见 `ll_world::state` 模块文档
//! `current_interior` 字段一节的同类说明）——旧版 v1 字节流里根本没有
//! 这两个字段对应的字节，无法靠「跳过缺失字段」这种自描述格式才有的
//! 手段兼容，必须真正重新按 v1 的字段顺序解析、再按 v2 的字段顺序
//! 重新编码。
//!
//! # 为什么用「反序列化成 v1 镜像类型、重新用真实类型编码」而不是直接
//! 操作字节
//!
//! [`crate::migration::Migration::migrate`] 的签名是「原始字节进、
//! 原始字节出」，注释里说明这是为了不强迫每个迁移函数都依赖一个可能
//! 过期的中间反序列化类型——但那段话说的是「签名不假设存在中间表示」，
//! 不是禁止使用中间表示。这里选择用中间表示（本模块
//! 定义的 `WorldStateV1`/`InteriorV1` 等镜像类型）：手写补丁式的字节
//! 级插入需要对 `postcard` 的 varint 编码逐字段手动定位，对
//! `WorldState` 这种嵌套了 `SurfaceStore`/`ThinPopulation`/`Arena<Agent>`
//! 等多层变长结构的类型而言极易出错、且难以测试覆盖每一种可能的输入
//! 形状；而 v1 与 v2 之间绝大多数字段的类型**完全没变**（`SurfaceStore`/
//! `ThinPopulation`/`Arena<Agent>`/`Option<EntityId>` 等），只需要给
//! `WorldState`/`InteriorTable`/`Interior` 三个「形状变了」的类型各留
//! 一份 v1 镜像用于解析，其余字段直接复用真实类型解析、再原样搬进
//! 真实类型重新编码——比逐字节手写补丁更不容易出错，也更容易用真实
//! 数据往返测试验证。
//!
//! # 为什么没有 `Migration4To5`（击杀计数语义改为数全部击杀，决策二）
//!
//! 项目所有者裁定「一起计算，就是杀了 10 只」，否决了
//! `WorldState::kill_counts` 原有的互斥填充设计（决策一：完整历史记录
//! 与聚合计数二选一）——改为对每一场击杀都累加聚合计数，具名死者
//! 额外再产出完整记录，两者叠加（见
//! `ll_sim::resolve` 模块内部 `append_kill_history` 文档「决策二」
//! 一节完整论证）。这**不需要**新的 schema 版本：`kill_counts` 字段
//! 本身的类型（`BTreeMap<ContentIndex, u64>`）、在 `WorldState` 里的
//! 声明位置都没有变化——`postcard` 按声明顺序定位字段，语义变化（同一
//! 个字段现在被填得更满）不影响字节布局，不属于本模块文档开篇说明的
//! 「在已有字段序列之间插入新字段」那一类必须靠迁移函数补齐的破坏性
//! 变更。
//!
//! 老存档因此读进来后 `kill_counts` 的值**原样保留**，不会被本模块的
//! 任何迁移步骤自动补算成决策二语义下的"真实总击杀数"——完整论证
//! （包括核实过为什么不能从 `WorldState::history` 反推出老存档缺失的
//! 那部分计数）见 `ll_world::state::WorldState::kill_counts` 文档
//! 「决策二」与「老存档的计数是永久低估」两节，这里不重复。
use std::collections::HashMap;

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};
use ll_world::bounded_grid::BoundedGrid;
use ll_world::entity::{Affiliation, Agent, Arena, BaseStats, EntityId, Goal, ThinPopulation};
use ll_world::exploration::ExplorationMemory;
use ll_world::history::HistoricalEvent;
use ll_world::interior::{Interior, InteriorTable};
use ll_world::script_state::ScriptValue;
use ll_world::space::{Space, SpaceId};
use ll_world::state::WorldState;
use ll_world::surface_store::SurfaceStore;
use ll_world::terrain::TerrainTable;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::migration::{Migration, MigrationError};

/// v1 存档里的 `Interior` 形状——`origin` 字段落地之前，见模块文档。
/// 只用于解析旧字节，不在生产代码任何其它地方构造或暴露。
#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct InteriorV1 {
    id: SpaceId,
    anchor: TorusPos,
    profile: ContentIndex,
    floors: HashMap<i16, BoundedGrid>,
}

/// v1 存档里 `InteriorTable` 手写序列化产出的扁平表示——与
/// `ll_world::interior` 模块内部的 `InteriorTableData` 同一种「摊平成
/// `Vec`」手法，这里镜像的是 v1 时代（`interiors: Vec<InteriorV1>`）
/// 的版本。
#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct InteriorTableDataV1 {
    interiors: Vec<InteriorV1>,
}

/// v1 存档主体的顶层形状——`WorldState` 新增 `exploration` 字段之前，
/// 字段顺序与当年的 `WorldStateRepr` 一致（见模块文档）。
///
/// # 为什么 `actors` 是 `Arena<AgentV2>`，不是 `Arena<Agent>`
///
/// `Agent` 自身的形状在 v1/v2 之间没有变化（击杀与死亡记录批次新增
/// 的三个字段是 v2→v3 才发生的事），但**当前**（v3 之后）的真实
/// `Agent` 类型已经带上了那三个字段——继续用真实 `Agent` 类型解析 v1
/// 字节会按错位的字段布局解码，静默产出损坏数据而不是报错。
/// [`AgentV2`]（本文件下方定义，随 v2→v3 迁移一起引入）恰好是「v1/v2
/// 共同的旧形状」，用它解析 v1 字节是正确的，不是偷懒复用了一个名字
/// 不太对但恰好能编译的类型。
#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct WorldStateV1 {
    seed: u64,
    clock: Tick,
    size: TorusSize,
    terrain: SurfaceStore,
    interiors: InteriorTableDataV1,
    population: ThinPopulation,
    actors: Arena<AgentV2>,
    player_entity: Option<EntityId>,
    #[serde(with = "ll_world::script_state::serde_map")]
    global_script_state: BTreeMap<(String, String), ScriptValue>,
}

/// v1 → v2：补齐 `Interior::origin`（恒为 `None`，v1 存档里的空间实例
/// 全部是「来源不可重算」，见其字段文档）与 `WorldState::exploration`
/// （恒为空记忆——v1 存档写出时探索记忆这个概念还不存在，没有任何
/// 「已经探索过」的历史数据可继承，空记忆是唯一诚实的起点）。
pub struct Migration1To2;

impl Migration for Migration1To2 {
    fn source_version(&self) -> u32 {
        1
    }

    fn target_version(&self) -> u32 {
        2
    }

    fn migrate(&self, body: Vec<u8>) -> Result<Vec<u8>, MigrationError> {
        let v1: WorldStateV1 =
            postcard::from_bytes(&body).map_err(|err| MigrationError::StepFailed {
                at_version: 1,
                reason: format!("v1 存档主体解码失败：{err}"),
            })?;

        let mut interiors = InteriorTable::new();
        for interior_v1 in v1.interiors.interiors {
            let mut interior =
                Interior::new(interior_v1.id, interior_v1.anchor, interior_v1.profile);
            for (floor, grid) in interior_v1.floors {
                interior.set_floor(floor, grid);
            }
            interiors.insert(interior);
        }

        // 产出 v2 形状（WorldStateV2），不是当前真实的 WorldState——
        // 真实 WorldState 现在是 v3 形状（多出 history/next_world_id/
        // Agent 三个新字段），若这里直接编码真实类型，本迁移就会悄悄
        // 把 v1 存档一步跳成 v3 形状的字节，但 MigrationChain 记录的
        // target_version 仍然是 2，链条自身的版本号与实际产出的字节
        // 形状会对不上——下一步 Migration2To3 会用 v2 形状去解码一份
        // 实际是 v3 形状的字节，同样静默错位。
        let v2 = WorldStateV2 {
            seed: v1.seed,
            clock: v1.clock,
            size: v1.size,
            terrain: v1.terrain,
            interiors,
            population: v1.population,
            actors: v1.actors,
            player_entity: v1.player_entity,
            exploration: ExplorationMemory::new(),
            global_script_state: v1.global_script_state,
        };

        postcard::to_allocvec(&v2).map_err(|err| MigrationError::StepFailed {
            at_version: 1,
            reason: format!("v2 存档主体编码失败：{err}"),
        })
    }
}

/// v2 存档里的 `Agent` 形状——击杀与死亡记录批次新增
/// `creature_kind`/`spawned_at`/`remembered_id` 三个字段之前，字段顺序
/// 与当前 `Agent` 的前 18 个字段一致（见 `ll_world::entity::agent`
/// 模块）。
///
/// # 为什么 `Serialize` 不是 `#[cfg(test)]` 专属
///
/// 与 `WorldStateV1`/`InteriorV1` 不同——那两个类型只在测试里构造
/// 「模拟旧字节」才需要编码，生产代码只解码它们；`AgentV2`/
/// `WorldStateV2` 还要供 [`Migration1To2`] 的生产 `migrate` 实现编码
/// 成 v2 形状的真实输出字节（见其文档「产出 v2 形状……」一节），因此
/// 两者的 `Serialize` 必须无条件派生。
#[derive(Deserialize, Serialize)]
struct AgentV2 {
    pos: TorusPos,
    stats: BaseStats,
    next_action_at: Tick,
    health: i32,
    affiliations: Vec<Affiliation>,
    wallet: i64,
    profession: ContentIndex,
    goals: Vec<Goal>,
    race: ContentIndex,
    current_space: Space,
    luck: i32,
    mana: i32,
    stamina: i32,
    unlocked_skills: Vec<ContentIndex>,
    skill_cooldowns: BTreeMap<ContentIndex, Tick>,
    subclasses: Vec<ContentIndex>,
    active_stat_modifiers:
        BTreeMap<ll_world::entity::AttributeKind, ll_world::entity::ActiveStatModifier>,
    #[serde(with = "ll_world::script_state::serde_map")]
    script_state: BTreeMap<(String, String), ScriptValue>,
}

/// v2 存档主体的顶层形状——`WorldState` 新增 `history`/`next_world_id`
/// 字段之前,字段顺序与当前 `WorldStateRepr` 一致（见
/// `ll_world::state` 模块文档）。`interiors`/`terrain`/`population`/
/// `exploration` 等字段自 v1→v2 之后形状未变,直接复用真实类型解析——
/// 与 [`Migration1To2`] 模块文档「为什么用……而不是直接操作字节」一节
/// 同一个理由：只给「形状变了」的类型（这里是 `Agent`/`WorldState`
/// 本身）各留一份镜像,其余字段复用真实类型。`Serialize` 无条件派生的
/// 理由见 [`AgentV2`] 文档同名一节。
#[derive(Deserialize, Serialize)]
struct WorldStateV2 {
    seed: u64,
    clock: Tick,
    size: TorusSize,
    terrain: SurfaceStore,
    interiors: InteriorTable,
    population: ThinPopulation,
    actors: Arena<AgentV2>,
    player_entity: Option<EntityId>,
    exploration: ExplorationMemory,
    #[serde(with = "ll_world::script_state::serde_map")]
    global_script_state: BTreeMap<(String, String), ScriptValue>,
}

/// v2 → v3：补齐 `WorldState::history`（恒为空——v2 存档写出时击杀与
/// 死亡记录这个概念还不存在，没有任何"已经发生"的历史事件可继承）、
/// `WorldState::next_world_id`（恒为 0——v2 存档从未分配过任何
/// `WorldId`，从零开始计数是唯一诚实的起点，见
/// `WorldId::next` 文档「永不复用」）与 `Agent` 新增的
/// `creature_kind`/`remembered_id`（恒为 `None`——迁移后的旧角色一律
/// 视为"尚未具名"/"未设置生物类型"）、`spawned_at`（恒为
/// `Tick(0)`——旧存档不知道具体出生时刻，世界纪元起点是唯一不需要
/// 编造数据的占位值；本字段本批次尚无任何消费方读取它，选用这个占位
/// 值不影响任何现有判定）。
///
/// # 为什么产出 `WorldStateV3`，不是当前真实的 `WorldState`
///
/// 理由与 [`Migration1To2`] 文档「产出 v2 形状……」一节完全相同（后来
/// 者是本注释被写下时已经踩过的同一个坑）：无名单位击杀计数批次给
/// `WorldState` 新增了 `kill_counts` 字段，真实 `WorldState` 现在是 v4
/// 形状，若这里直接编码真实类型，本迁移就会把 v2 存档一步跳成 v4 形状
/// 的字节，但 `MigrationChain` 记录的 `target_version` 仍然是 3，链条
/// 自身的版本号与实际产出的字节形状会对不上——下一步 `Migration3To4`
/// 会用 v3 形状去解码一份实际是 v4 形状的字节，同样静默错位。
pub struct Migration2To3;

impl Migration for Migration2To3 {
    fn source_version(&self) -> u32 {
        2
    }

    fn target_version(&self) -> u32 {
        3
    }

    fn migrate(&self, body: Vec<u8>) -> Result<Vec<u8>, MigrationError> {
        let v2: WorldStateV2 =
            postcard::from_bytes(&body).map_err(|err| MigrationError::StepFailed {
                at_version: 2,
                reason: format!("v2 存档主体解码失败：{err}"),
            })?;

        let actors = v2.actors.map(|agent| Agent {
            pos: agent.pos,
            stats: agent.stats,
            next_action_at: agent.next_action_at,
            health: agent.health,
            affiliations: agent.affiliations,
            wallet: agent.wallet,
            profession: agent.profession,
            goals: agent.goals,
            race: agent.race,
            current_space: agent.current_space,
            luck: agent.luck,
            mana: agent.mana,
            stamina: agent.stamina,
            unlocked_skills: agent.unlocked_skills,
            skill_cooldowns: agent.skill_cooldowns,
            subclasses: agent.subclasses,
            active_stat_modifiers: agent.active_stat_modifiers,
            script_state: agent.script_state,
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
        });

        let v3 = WorldStateV3 {
            seed: v2.seed,
            clock: v2.clock,
            size: v2.size,
            terrain: v2.terrain,
            interiors: v2.interiors,
            population: v2.population,
            actors,
            player_entity: v2.player_entity,
            exploration: v2.exploration,
            global_script_state: v2.global_script_state,
            history: Vec::new(),
            next_world_id: 0,
        };

        postcard::to_allocvec(&v3).map_err(|err| MigrationError::StepFailed {
            at_version: 2,
            reason: format!("v3 存档主体编码失败：{err}"),
        })
    }
}

/// v3 存档主体的顶层形状——`WorldState` 新增 `kill_counts` 字段（无名
/// 单位击杀计数批次）之前，字段顺序与当时的 `WorldStateRepr` 一致。
/// `Agent` 自身的形状在 v3/v4 之间没有变化（本批次只改 `WorldState`
/// 本身，不改 `Agent`），因此直接复用真实 `Agent` 类型解析——与
/// [`Migration2To3`] 模块文档「为什么用……而不是直接操作字节」一节同一
/// 个理由：只给「形状变了」的类型（这里只有 `WorldState` 本身）留一份
/// 镜像，其余字段复用真实类型。`Serialize` 无条件派生的理由见
/// [`AgentV2`] 文档「为什么 Serialize 不是 cfg(test) 专属」一节——本
/// 类型同样要供 [`Migration2To3`] 的生产 `migrate` 实现编码成 v3 形状
/// 的真实输出字节。
#[derive(Deserialize, Serialize)]
struct WorldStateV3 {
    seed: u64,
    clock: Tick,
    size: TorusSize,
    terrain: SurfaceStore,
    interiors: InteriorTable,
    population: ThinPopulation,
    actors: Arena<Agent>,
    player_entity: Option<EntityId>,
    exploration: ExplorationMemory,
    #[serde(with = "ll_world::script_state::serde_map")]
    global_script_state: BTreeMap<(String, String), ScriptValue>,
    history: Vec<HistoricalEvent>,
    next_world_id: u32,
}

/// v3 → v4：补齐 `WorldState::kill_counts`（恒为空表——v3 存档写出时
/// 无名单位击杀计数这个概念还不存在，没有任何"已经发生"的无名击杀可
/// 继承，空表是唯一诚实的起点,见 `ll_world::state::WorldState::kill_counts`
/// 文档）。
pub struct Migration3To4;

impl Migration for Migration3To4 {
    fn source_version(&self) -> u32 {
        3
    }

    fn target_version(&self) -> u32 {
        4
    }

    fn migrate(&self, body: Vec<u8>) -> Result<Vec<u8>, MigrationError> {
        let v3: WorldStateV3 =
            postcard::from_bytes(&body).map_err(|err| MigrationError::StepFailed {
                at_version: 3,
                reason: format!("v3 存档主体解码失败：{err}"),
            })?;

        let world = WorldState {
            seed: v3.seed,
            clock: v3.clock,
            size: v3.size,
            terrain: v3.terrain,
            interiors: v3.interiors,
            current_interior: None,
            surface_profile: ContentIndex::default(),
            population: v3.population,
            actors: v3.actors,
            player_entity: v3.player_entity,
            exploration: v3.exploration,
            global_script_state: v3.global_script_state,
            terrain_table: TerrainTable::default(),
            history: v3.history,
            next_world_id: v3.next_world_id,
            kill_counts: BTreeMap::new(),
        };

        postcard::to_allocvec(&world).map_err(|err| MigrationError::StepFailed {
            at_version: 3,
            reason: format!("v4 存档主体编码失败：{err}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId, WorldId};
    use ll_world::generate::GenParams;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    fn v1_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        ZoneLayout::new(48, zone_count).expect("48 满足全部对齐与跨度约束")
    }

    /// 手写编码一份 v1 形状的存档主体字节——真实生产代码从来不会再
    /// 产出这种字节（`WorldState`/`Interior` 现在恒带 `exploration`/
    /// `origin`），这里手工构造 `WorldStateV1`/`InteriorV1` 正是为了
    /// 独立于当前生产类型，模拟「升级前写盘的真实旧字节」。
    fn encode_v1_body(interiors: Vec<InteriorV1>) -> Vec<u8> {
        let layout = v1_layout();
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        let world = WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件");

        let v1 = WorldStateV1 {
            seed: world.seed,
            clock: world.clock,
            size: world.size,
            terrain: world.terrain,
            interiors: InteriorTableDataV1 { interiors },
            population: world.population,
            // WorldState::new 从不 spawn 任何 Agent（见其文档），这里
            // 恒是空池——Arena<AgentV2> 是「v1/v2 共同旧形状」，与真实
            // WorldState 现在的 Arena<Agent>（v3 形状）不是同一个类型,
            // 不能直接搬 world.actors 过来，但两者在「空」这个状态下
            // 反正没有任何字段差异需要体现。
            actors: Arena::new(),
            player_entity: world.player_entity,
            global_script_state: world.global_script_state,
        };
        postcard::to_allocvec(&v1).expect("手写的 v1 镜像类型必然可编码")
    }

    fn profile_index() -> ContentIndex {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse("lostland:dungeon").expect("合法"))
    }

    /// 把一份 v1 形状的存档主体字节沿着完整的迁移链（v1→v2→v3→v4）升
    /// 级，解码成当前真实的 `WorldState`——`Migration1To2`/`Migration2To3`
    /// 单独的产出分别是 v2/v3 形状（`WorldStateV2`/`WorldStateV3`），
    /// 不能再直接当作当前 `WorldState` 解码（见 `Migration1To2` 文档
    /// 「产出 v2 形状……」一节），本仓库已知的"当前最新 schema"因此永远
    /// 是"把链条走完"，不是"跑完某一具体的一步"。
    fn migrate_v1_body_to_current(body: Vec<u8>) -> WorldState {
        let v2_body = Migration1To2
            .migrate(body)
            .expect("v1 到 v2 的迁移应当成功");
        let v3_body = Migration2To3
            .migrate(v2_body)
            .expect("v2 到 v3 的迁移应当成功");
        let v4_body = Migration3To4
            .migrate(v3_body)
            .expect("v3 到 v4 的迁移应当成功");
        postcard::from_bytes(&v4_body).expect("迁移产出的字节必须是合法的 v4 存档")
    }

    #[test]
    fn 迁移后空interior的存档能解码成合法的世界状态() {
        // Arrange
        let body = encode_v1_body(Vec::new());

        // Act
        let world = migrate_v1_body_to_current(body);

        // Assert
        assert_eq!(world.interiors.total_floor_count(), 0);
    }

    #[test]
    fn 迁移后world的探索记忆为空() {
        // Arrange
        let body = encode_v1_body(Vec::new());

        // Act
        let world = migrate_v1_body_to_current(body);

        // Assert
        assert_eq!(world.exploration, ExplorationMemory::new());
    }

    #[test]
    fn 迁移后v1里的interior保留id锚点与楼层内容() {
        // Arrange
        let mut counter = 0u32;
        let id = WorldId::next(&mut counter);
        let anchor = v1_layout().tile_size().wrap(3, 3);
        let profile = profile_index();
        let (terrain_ids, _table) = base_terrain_fixture();
        let size = ll_core::bounded::BoundedSize::new(4, 4).expect("4x4 是合法尺寸");
        let grid = BoundedGrid::new(size, terrain_ids.floor_stone);
        let mut floors = HashMap::new();
        floors.insert(0i16, grid);
        let body = encode_v1_body(vec![InteriorV1 {
            id,
            anchor,
            profile,
            floors,
        }]);

        // Act
        let world = migrate_v1_body_to_current(body);

        // Assert
        let interior = world
            .interiors
            .get(id)
            .expect("迁移后应当保留这个 Interior");
        assert_eq!(interior.anchor, anchor);
    }

    #[test]
    fn 迁移后v1里的interior来源标记为不可重算() {
        // Arrange
        let mut counter = 0u32;
        let id = WorldId::next(&mut counter);
        let anchor = v1_layout().tile_size().wrap(3, 3);
        let profile = profile_index();
        let body = encode_v1_body(vec![InteriorV1 {
            id,
            anchor,
            profile,
            floors: HashMap::new(),
        }]);

        // Act
        let world = migrate_v1_body_to_current(body);

        // Assert
        let interior = world
            .interiors
            .get(id)
            .expect("迁移后应当保留这个 Interior");
        assert!(interior.origin.is_none());
    }

    #[test]
    fn 损坏的v1字节迁移失败而不panic() {
        // Arrange
        let corrupted = vec![0xFFu8; 4];

        // Act
        let result = Migration1To2.migrate(corrupted);

        // Assert
        assert!(matches!(
            result,
            Err(MigrationError::StepFailed { at_version: 1, .. })
        ));
    }

    /// 手写编码一份 v2 形状的存档主体字节——理由同 [`encode_v1_body`]：
    /// 独立于当前生产类型，模拟"升级前写盘的真实旧字节"。`agent` 为
    /// `Some` 时会往 `actors` 里放一个具体的 v2 形状 `Agent`，供「迁移
    /// 后数据没丢」这条断言使用。
    fn encode_v2_body(agent: Option<AgentV2>) -> Vec<u8> {
        let world = base_v1_world();
        let mut actors: Arena<AgentV2> = Arena::new();
        if let Some(agent) = agent {
            actors.spawn(agent);
        }
        let v2 = WorldStateV2 {
            seed: world.seed,
            clock: world.clock,
            size: world.size,
            terrain: world.terrain,
            interiors: world.interiors,
            population: world.population,
            actors,
            player_entity: None,
            exploration: world.exploration,
            global_script_state: world.global_script_state,
        };
        postcard::to_allocvec(&v2).expect("手写的 v2 镜像类型必然可编码")
    }

    /// [`encode_v1_body`]/[`encode_v2_body`] 共用的空世界构造——只是
    /// 借用 `WorldState::new` 拿到一份地形/尺寸数据，不消费其
    /// `actors`/`player_entity`。
    fn base_v1_world() -> WorldState {
        let layout = v1_layout();
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

    /// 一个字段取值互不相同的 v2 `Agent`——供往返测试确认每个字段都
    /// 真的被迁移函数原样保留，而不是巧合地相等。
    fn sample_agent_v2(world: &WorldState) -> AgentV2 {
        let mut interner = Interner::new();
        let profession = interner.intern(NamespacedId::parse("lostland:farmer").expect("合法"));
        let race = interner.intern(NamespacedId::parse("lostland:dwarf").expect("合法"));
        let pos = world.size.wrap(3, 5);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        AgentV2 {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(7),
            health: 55,
            affiliations: Vec::new(),
            wallet: 321,
            profession,
            goals: Vec::new(),
            race,
            current_space: Space::surface(zone, ContentIndex::default()),
            luck: 4,
            mana: 20,
            stamina: 30,
            unlocked_skills: Vec::new(),
            skill_cooldowns: BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: BTreeMap::new(),
            script_state: BTreeMap::new(),
        }
    }

    #[test]
    fn 迁移后v2里的agent核心字段原样保留() {
        // Arrange
        let world = base_v1_world();
        let agent_v2 = sample_agent_v2(&world);
        let (expected_pos, expected_health, expected_wallet) =
            (agent_v2.pos, agent_v2.health, agent_v2.wallet);
        let body = encode_v2_body(Some(agent_v2));

        // Act
        let migrated = Migration2To3
            .migrate(body)
            .expect("v2 到 v3 的迁移应当成功");
        let migrated_world: WorldStateV3 =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v3 存档");

        // Assert：只断言这一批"迁移前就存在的字段"，新字段的默认值由
        // 下面几条独立测试各自断言。
        let agent = migrated_world
            .actors
            .iter()
            .next()
            .expect("迁移后应当保留这个 Agent");
        assert_eq!(
            (agent.pos, agent.health, agent.wallet),
            (expected_pos, expected_health, expected_wallet)
        );
    }

    #[test]
    fn 迁移后v2里的agent新字段取诚实默认值而非编造数据() {
        // Arrange
        let world = base_v1_world();
        let body = encode_v2_body(Some(sample_agent_v2(&world)));

        // Act
        let migrated = Migration2To3
            .migrate(body)
            .expect("v2 到 v3 的迁移应当成功");
        let migrated_world: WorldStateV3 =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v3 存档");

        // Assert
        let agent = migrated_world
            .actors
            .iter()
            .next()
            .expect("迁移后应当保留这个 Agent");
        assert_eq!(agent.creature_kind, None);
        assert_eq!(agent.spawned_at, Tick(0));
        assert_eq!(agent.remembered_id, None);
    }

    #[test]
    fn 迁移后world的历史事件日志为空() {
        // Arrange
        let body = encode_v2_body(None);

        // Act
        let migrated = Migration2To3
            .migrate(body)
            .expect("v2 到 v3 的迁移应当成功");
        let migrated_world: WorldStateV3 =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v3 存档");

        // Assert
        assert!(migrated_world.history.is_empty());
    }

    #[test]
    fn 迁移后world的worldid分配计数器为零() {
        // Arrange
        let body = encode_v2_body(None);

        // Act
        let migrated = Migration2To3
            .migrate(body)
            .expect("v2 到 v3 的迁移应当成功");
        let migrated_world: WorldStateV3 =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v3 存档");

        // Assert
        assert_eq!(migrated_world.next_world_id, 0);
    }

    #[test]
    fn 损坏的v2字节迁移失败而不panic() {
        // Arrange
        let corrupted = vec![0xFFu8; 4];

        // Act
        let result = Migration2To3.migrate(corrupted);

        // Assert
        assert!(matches!(
            result,
            Err(MigrationError::StepFailed { at_version: 2, .. })
        ));
    }

    /// 手写编码一份 v3 形状的存档主体字节——理由同 [`encode_v1_body`]/
    /// [`encode_v2_body`]：独立于当前生产类型，模拟"升级前写盘的真实旧
    /// 字节"。`history` 非空时供「迁移后既有历史事件原样保留」这条断言
    /// 使用。
    fn encode_v3_body(history: Vec<HistoricalEvent>, next_world_id: u32) -> Vec<u8> {
        let world = base_v1_world();
        let v3 = WorldStateV3 {
            seed: world.seed,
            clock: world.clock,
            size: world.size,
            terrain: world.terrain,
            interiors: world.interiors,
            population: world.population,
            actors: Arena::new(),
            player_entity: None,
            exploration: world.exploration,
            global_script_state: world.global_script_state,
            history,
            next_world_id,
        };
        postcard::to_allocvec(&v3).expect("手写的 v3 镜像类型必然可编码")
    }

    #[test]
    fn 迁移后world的kill_counts为空() {
        // Arrange
        let body = encode_v3_body(Vec::new(), 0);

        // Act
        let migrated = Migration3To4
            .migrate(body)
            .expect("v3 到 v4 的迁移应当成功");
        let migrated_world: WorldState =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v4 存档");

        // Assert
        assert!(migrated_world.kill_counts.is_empty());
    }

    #[test]
    fn 迁移后v3里既有的历史事件与worldid计数器原样保留() {
        // 与「新字段取诚实默认值」相对：v3 已经存在的数据不应该被这次
        // 迁移悄悄丢掉或改写。
        // Arrange
        let mut counter = 5u32;
        let event = HistoricalEvent {
            id: ll_core::ident::WorldId::next(&mut counter),
            at: Tick(10),
            location: v1_layout().tile_size().wrap(1, 1),
            kind: ll_world::history::HistoricalEventKind::Kill(ll_world::history::KillRecord {
                killer: None,
                victim: ll_core::ident::WorldId::next(&mut counter),
                cause: ll_world::history::KillCause::Fall,
                killing_blow: ll_world::history::KillingBlow {
                    damage: 5,
                    remaining_health: -1,
                },
                victim_state: ll_world::history::VictimState::UNKNOWN,
            }),
        };
        let body = encode_v3_body(vec![event.clone()], 7);

        // Act
        let migrated = Migration3To4
            .migrate(body)
            .expect("v3 到 v4 的迁移应当成功");
        let migrated_world: WorldState =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v4 存档");

        // Assert
        assert_eq!(migrated_world.history, vec![event]);
        assert_eq!(migrated_world.next_world_id, 7);
    }

    #[test]
    fn 损坏的v3字节迁移失败而不panic() {
        // Arrange
        let corrupted = vec![0xFFu8; 4];

        // Act
        let result = Migration3To4.migrate(corrupted);

        // Assert
        assert!(matches!(
            result,
            Err(MigrationError::StepFailed { at_version: 3, .. })
        ));
    }
}
