//! 与 GPU 无关的纯计算：世界/相机常量、地形→图集条目名映射、光照
//! 色调、精灵锚点换算。
//!
//! 拆分理由与 `ll-world` 的 `p2_acceptance::layout`、`ll-render` 的
//! `p1_acceptance::layout` 一致（见各自模块文档）：纯函数脱离 GPU 也能
//! 单测覆盖，常量只在这一处定义，不在 `main.rs` 与本模块测试里各抄
//! 一份。

use ll_core::time::{Season, TICKS_PER_HOUR, Tick};
use ll_render::sprite::{Footprint, Pivot, TILE_SIZE};
use ll_world::light::ambient_light;
use ll_world::terrain::TerrainKind;

/// 演示世界的宽度（格）。
///
/// 必须是噪声格点尺寸（16）的整数倍，且大于渲染层相机单帧可见的跨度
/// 43×25 格（见 `ll_world::chunk::ChunkGrid::new` 文档）。本 demo 的
/// 重点是回合与战斗，不需要 `p2_acceptance` 那么大的世界来演示「走到
/// 边缘绕回」，取一个更小的值以降低每帧 FOV/AI 决策的计算量。
pub(crate) const WORLD_WIDTH: u32 = 128;

/// 演示世界的高度（格），理由同 [`WORLD_WIDTH`]。
pub(crate) const WORLD_HEIGHT: u32 = 128;

/// 视野基准半径（格），随光照按 [`ll_world::light::sight_radius_at`]
/// 缩放。
pub(crate) const BASE_SIGHT_RADIUS: u32 = 12;

/// demo 开局时把时钟从 [`ll_world::state::WorldState::new`] 恒定的午夜
/// 推进到的初始刻度：正午——理由与 `p2_acceptance::layout::INITIAL_CLOCK_TICKS`
/// 完全一致（开局第一帧不该因为世界时钟恰好落在最暗的午夜而看起来
/// 一片漆黑）。
pub(crate) const INITIAL_CLOCK_TICKS: i64 = 12 * TICKS_PER_HOUR;

/// 把地形种类映射到图集条目名。
///
/// 与 `p2_acceptance::layout::terrain_entry_name` 逐字同构：同一份
/// 图集元数据、同一套八种自然地形，没有理由让两个 demo 的映射表各写
/// 一份、还可能悄悄漂移。
pub(crate) fn terrain_entry_name(kind: TerrainKind) -> Option<&'static str> {
    match kind {
        TerrainKind::DEEP_WATER => Some("terrain_deep_water"),
        TerrainKind::SHALLOW_WATER => Some("terrain_shallow_water"),
        TerrainKind::SAND => Some("terrain_sand"),
        TerrainKind::GRASS => Some("terrain_grass"),
        TerrainKind::FOREST => Some("terrain_forest"),
        TerrainKind::HILL => Some("terrain_hill"),
        TerrainKind::MOUNTAIN => Some("terrain_mountain"),
        TerrainKind::SNOW => Some("terrain_snow"),
        _ => None,
    }
}

/// 季节对画面颜色的调制，`[r, g, b]` 各分量在 `[0, 1]` 内——与
/// `p2_acceptance::layout::season_tint` 同一取舍，理由见其文档：
/// `ll_world::light` 只提供亮度缩放，色相是纯呈现层的选择。
fn season_tint(season: Season) -> [f32; 3] {
    match season {
        Season::Spring => [0.80, 1.00, 0.82],
        Season::Summer => [1.00, 0.98, 0.85],
        Season::Autumn => [1.00, 0.80, 0.55],
        Season::Winter => [0.72, 0.82, 1.00],
    }
}

/// 求某一世界时刻的画面颜色调制：昼夜亮度乘以季节色相，alpha 恒为 1。
pub(crate) fn ambient_tint(clock: Tick) -> [f32; 4] {
    let light = ambient_light(clock);
    let brightness = (light.0.clamp(0, 1000) as f32) / 1000.0;
    let [r, g, b] = season_tint(clock.season());
    [r * brightness, g * brightness, b * brightness, 1.0]
}

/// 占地 `footprint` 格、左上角像素原点为 `tile_origin` 的图块，其锚点
/// （占地区域底边水平中点）在离屏渲染目标像素空间中的位置。
///
/// 与 `ll-render`/`ll-world` 两份 `p1_acceptance`/`p2_acceptance` 的
/// 同名函数逐字同构：同一条渲染规则（脚站在占地格块的底边中点）不该
/// 在第三个 demo 里再漂出第三份实现。
pub(crate) fn footprint_anchor_pixel(tile_origin: (i32, i32), footprint: Footprint) -> (i32, i32) {
    let half_width_px = footprint.width as i32 * TILE_SIZE as i32 / 2;
    let height_px = footprint.height as i32 * TILE_SIZE as i32;
    (tile_origin.0 + half_width_px, tile_origin.1 + height_px)
}

/// 把「占地锚点」与「图像内锚点（pivot）」相减，得到精灵图像左上角
/// 应绘制在离屏渲染目标像素空间中的位置。
pub(crate) fn sprite_draw_position(
    tile_origin: (i32, i32),
    footprint: Footprint,
    pivot: Pivot,
) -> [f32; 2] {
    let (anchor_x, anchor_y) = footprint_anchor_pixel(tile_origin, footprint);
    [
        (anchor_x - pivot.x as i32) as f32,
        (anchor_y - pivot.y as i32) as f32,
    ]
}

/// 占地格块底边的**屏幕**纵坐标（像素），供 `ll_render::sprite::DrawOrder`
/// 用作 `foot_y`。
///
/// `screen_tile_y` 必须是占地左上角格的**屏幕**纵坐标
/// （`Camera::world_to_screen` 的返回值），不能是世界纵坐标——这正是
/// Task 1 修复的缺陷：环面世界里跨南北接缝时世界纵坐标的排序会反转，
/// 本 demo 故意把一个敌人摆在与玩家出生点隔着接缝的一侧（见
/// `spawn::ENEMY_FAST_OFFSET`），用真实渲染路径验证这条修复。
pub(crate) fn footprint_bottom_screen_y(screen_tile_y: i32, footprint_height: u8) -> i32 {
    screen_tile_y + footprint_height as i32 * TILE_SIZE as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 八种自然地形都能查到图集条目() {
        // Arrange
        let natural_kinds = [
            TerrainKind::DEEP_WATER,
            TerrainKind::SHALLOW_WATER,
            TerrainKind::SAND,
            TerrainKind::GRASS,
            TerrainKind::FOREST,
            TerrainKind::HILL,
            TerrainKind::MOUNTAIN,
            TerrainKind::SNOW,
        ];

        // Act & Assert
        for kind in natural_kinds {
            assert!(terrain_entry_name(kind).is_some());
        }
    }

    #[test]
    fn 建筑地形没有对应的地形图集条目() {
        // Arrange & Act & Assert
        assert!(terrain_entry_name(TerrainKind::WALL_STONE).is_none());
    }

    #[test]
    fn 正午的画面色调不为纯黑() {
        // Arrange
        let noon = Tick(12 * TICKS_PER_HOUR);

        // Act
        let [r, g, b, a] = ambient_tint(noon);

        // Assert
        assert!(r > 0.0 && g > 0.0 && b > 0.0 && a == 1.0);
    }

    #[test]
    fn 两格宽占地的锚点横向偏移大于单格占地() {
        // 验证「footprint 从图集条目读取」这条验收点背后的算法本身
        // 确实会随占地宽度变化，而不是巧合地对任何宽度都算出同一个值。
        // Arrange
        let one_by_one = Footprint {
            width: 1,
            height: 1,
        };
        let two_by_two = Footprint {
            width: 2,
            height: 2,
        };

        // Act
        let (ax1, _) = footprint_anchor_pixel((0, 0), one_by_one);
        let (ax2, _) = footprint_anchor_pixel((0, 0), two_by_two);

        // Assert
        assert!(ax2 > ax1);
    }

    #[test]
    fn 占地底边屏幕纵坐标随占地高度增加() {
        // Arrange & Act
        let one_tile = footprint_bottom_screen_y(10, 1);
        let two_tiles = footprint_bottom_screen_y(10, 2);

        // Assert
        assert!(two_tiles > one_tile);
    }
}
