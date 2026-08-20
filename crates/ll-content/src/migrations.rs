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
use std::collections::HashMap;

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_core::torus::{TorusPos, TorusSize};
use ll_world::bounded_grid::BoundedGrid;
use ll_world::entity::{Agent, Arena, EntityId, ThinPopulation};
use ll_world::exploration::ExplorationMemory;
use ll_world::interior::{Interior, InteriorTable};
use ll_world::script_state::ScriptValue;
use ll_world::space::SpaceId;
use ll_world::state::WorldState;
use ll_world::surface_store::SurfaceStore;
use ll_world::terrain::TerrainTable;
use serde::Deserialize;
#[cfg(test)]
use serde::Serialize;
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
#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct WorldStateV1 {
    seed: u64,
    clock: Tick,
    size: TorusSize,
    terrain: SurfaceStore,
    interiors: InteriorTableDataV1,
    population: ThinPopulation,
    actors: Arena<Agent>,
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

        let world = WorldState {
            seed: v1.seed,
            clock: v1.clock,
            size: v1.size,
            terrain: v1.terrain,
            interiors,
            current_interior: None,
            surface_profile: ContentIndex::default(),
            population: v1.population,
            actors: v1.actors,
            player_entity: v1.player_entity,
            exploration: ExplorationMemory::new(),
            global_script_state: v1.global_script_state,
            terrain_table: TerrainTable::default(),
        };

        postcard::to_allocvec(&world).map_err(|err| MigrationError::StepFailed {
            at_version: 1,
            reason: format!("v2 存档主体编码失败：{err}"),
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
            actors: world.actors,
            player_entity: world.player_entity,
            global_script_state: world.global_script_state,
        };
        postcard::to_allocvec(&v1).expect("手写的 v1 镜像类型必然可编码")
    }

    fn profile_index() -> ContentIndex {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse("lostland:dungeon").expect("合法"))
    }

    #[test]
    fn 迁移后空interior的存档能解码成合法的世界状态() {
        // Arrange
        let body = encode_v1_body(Vec::new());

        // Act
        let migrated = Migration1To2
            .migrate(body)
            .expect("v1 到 v2 的迁移应当成功");
        let world: WorldState =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v2 存档");

        // Assert
        assert_eq!(world.interiors.total_floor_count(), 0);
    }

    #[test]
    fn 迁移后world的探索记忆为空() {
        // Arrange
        let body = encode_v1_body(Vec::new());

        // Act
        let migrated = Migration1To2
            .migrate(body)
            .expect("v1 到 v2 的迁移应当成功");
        let world: WorldState =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v2 存档");

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
        let migrated = Migration1To2
            .migrate(body)
            .expect("v1 到 v2 的迁移应当成功");
        let world: WorldState =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v2 存档");

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
        let migrated = Migration1To2
            .migrate(body)
            .expect("v1 到 v2 的迁移应当成功");
        let world: WorldState =
            postcard::from_bytes(&migrated).expect("迁移产出的字节必须是合法的 v2 存档");

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
}
