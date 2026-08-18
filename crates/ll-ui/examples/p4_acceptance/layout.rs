//! 与 GPU 无关的纯计算：世界常量、地形→图集条目名映射。
//!
//! 拆分理由与 `ll-sim` 的 `p3_acceptance::layout` 一致：纯函数脱离 GPU
//! 也能单测覆盖，常量只在这一处定义。

use ll_world::terrain::{BaseTerrainIds, TerrainKind};

/// 演示世界的宽度（格）。必须是噪声格点尺寸（16）的整数倍，且大于
/// 相机单帧可见跨度 43×25 格——见 `ll_world::chunk::ChunkGrid::new`
/// 文档。P4 demo 的重点是 mod 加载，不需要 P3 那么大的世界。
pub(crate) const WORLD_WIDTH: u32 = 64;
/// 演示世界的高度（格），理由同 [`WORLD_WIDTH`]。
pub(crate) const WORLD_HEIGHT: u32 = 64;

/// demo 开局时把时钟从午夜推进到正午——理由与 p2/p3_acceptance 完全
/// 一致（开局第一帧不该因为世界时钟恰好落在最暗的午夜而看起来一片
/// 漆黑）。
pub(crate) const INITIAL_CLOCK_TICKS: i64 = 12 * ll_core::time::TICKS_PER_HOUR;

/// 把地形种类映射到图集条目名，外加一个染色（RGBA，`[0,1]`）。
///
/// 与 p2/p3_acceptance 的 `terrain_entry_name` 同构，多返回一个染色
/// 分量——这是本 demo 特有的需求：`examplemod:lava_floor` 是运行期才
/// 分配出来的内容索引，占位图集里没有为它准备美术资产（P4 阶段没有
/// mod 美术资产管线），如实复用一个已有的地形条目（`terrain_sand`，
/// 视觉上足够扁平、适合被染色覆盖）再叠一层醒目的橙红色，让它在画面
/// 上一眼可辨，同时明确标注这是复用占位美术、不是真正的熔岩贴图。
pub(crate) fn terrain_entry_name_and_tint(
    kind: TerrainKind,
    terrain_ids: &BaseTerrainIds,
    lava_kind: Option<TerrainKind>,
) -> Option<(&'static str, [f32; 4])> {
    if Some(kind) == lava_kind {
        return Some(("terrain_sand", [1.0, 0.35, 0.15, 1.0]));
    }
    let neutral = [1.0, 1.0, 1.0, 1.0];
    if kind == terrain_ids.deep_water {
        Some(("terrain_deep_water", neutral))
    } else if kind == terrain_ids.shallow_water {
        Some(("terrain_shallow_water", neutral))
    } else if kind == terrain_ids.sand {
        Some(("terrain_sand", neutral))
    } else if kind == terrain_ids.grass {
        Some(("terrain_grass", neutral))
    } else if kind == terrain_ids.forest {
        Some(("terrain_forest", neutral))
    } else if kind == terrain_ids.hill {
        Some(("terrain_hill", neutral))
    } else if kind == terrain_ids.mountain {
        Some(("terrain_mountain", neutral))
    } else if kind == terrain_ids.snow {
        Some(("terrain_snow", neutral))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_world::terrain::base_terrain_fixture;

    #[test]
    fn 熔岩地形映射到沙地条目并染成橙红色() {
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        // 借用一个本体地形索引伪装成"lava_kind"，只测试映射函数本身
        // 的分支逻辑，不牵扯真实的 mod 加载。
        let fake_lava = terrain_ids.wall_wood;

        // Act
        let result = terrain_entry_name_and_tint(fake_lava, &terrain_ids, Some(fake_lava));

        // Assert
        let (name, tint) = result.expect("熔岩应当有对应的展示条目");
        assert_eq!(name, "terrain_sand");
        assert_ne!(tint, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn 自然地形保持白色不被误染色() {
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();

        // Act
        let result = terrain_entry_name_and_tint(terrain_ids.grass, &terrain_ids, None);

        // Assert
        let (_name, tint) = result.expect("草地应当有对应的展示条目");
        assert_eq!(tint, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn 建筑地形没有对应的展示条目() {
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();

        // Act
        let result = terrain_entry_name_and_tint(terrain_ids.wall_stone, &terrain_ids, None);

        // Assert
        assert!(result.is_none());
    }
}
