//! 与 GPU 无关的纯计算：地形 → 图集条目名映射、光照 → 视野半径换算。
//!
//! 拆成独立文件、脱离窗口/GPU 也能被 `cargo test --workspace` 覆盖的
//! 理由，与 `ll-sim` 的 `p5_coordinate_acceptance::layout` 一致，见其
//! 模块文档。本文件的 [`terrain_entry_name`]/[`effective_sight_radius`]/
//! [`effective_tint`] 是同一套换算的独立实现（不是同一份代码的引用
//! ——`p5_coordinate_acceptance` 是 `ll-sim` 的一个 `examples/` 目录，
//! 不是可供下游 crate 依赖的库 API，见 Cargo 对 `examples/` 的可见性
//! 规则），保持逻辑一致但物理上各自独立。

use ll_core::time::Tick;
use ll_world::light::sight_radius_at;
use ll_world::space_profile::{SpaceProfile, effective_ambient_light};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

/// 地表视野基准半径（格），随光照缩放。
pub const BASE_SIGHT_RADIUS: u32 = 12;

/// 把地形种类映射到图集条目名——覆盖本体注册的全部自然地形。
pub fn terrain_entry_name(kind: TerrainKind, ids: &BaseTerrainIds) -> Option<&'static str> {
    if kind == ids.deep_water {
        Some("terrain_deep_water")
    } else if kind == ids.shallow_water {
        Some("terrain_shallow_water")
    } else if kind == ids.sand {
        Some("terrain_sand")
    } else if kind == ids.grass {
        Some("terrain_grass")
    } else if kind == ids.forest {
        Some("terrain_forest")
    } else if kind == ids.hill {
        Some("terrain_hill")
    } else if kind == ids.mountain {
        Some("terrain_mountain")
    } else if kind == ids.snow {
        Some("terrain_snow")
    } else if kind == ids.floor_stone {
        Some("terrain_dirt")
    } else if kind == ids.wall_stone {
        Some("terrain_mountain")
    } else {
        None
    }
}

/// 给定空间在某一世界时刻的有效光照换算出的视野半径。
pub fn effective_sight_radius(profile: &SpaceProfile, clock: Tick) -> u32 {
    let light = effective_ambient_light(profile, clock);
    sight_radius_at(BASE_SIGHT_RADIUS, light)
}

/// 画面整体亮度调制（灰阶）。
pub fn effective_tint(profile: &SpaceProfile, clock: Tick) -> [f32; 4] {
    let light = effective_ambient_light(profile, clock).0.clamp(0, 1000) as f32 / 1000.0;
    [light, light, light, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 全部自然地形都能查到图集条目() {
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let kinds = [
            ids.deep_water,
            ids.shallow_water,
            ids.sand,
            ids.grass,
            ids.forest,
            ids.hill,
            ids.mountain,
            ids.snow,
        ];

        // Act & Assert
        for kind in kinds {
            assert!(terrain_entry_name(kind, &ids).is_some());
        }
    }

    #[test]
    fn 光照全灭时视野半径缩小到基准值以下() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_dark").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: false,
            base_temperature: 0,
            diggable: false,
            buildable: false,
            reverb_tag: None,
        };

        // Act
        let radius = effective_sight_radius(&profile, Tick(0));

        // Assert
        assert!(radius < BASE_SIGHT_RADIUS);
    }
}
