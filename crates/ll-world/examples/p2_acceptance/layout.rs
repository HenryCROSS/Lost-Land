//! 与 GPU 无关的纯计算：地形→图集条目名映射、光照色调、小地图版式、
//! 精灵锚点换算。
//!
//! 单独成模块是为了让这些逻辑可以脱离 GPU 资源被单测覆盖——它们都是
//! 纯函数，不需要真实窗口或图形适配器。理由与 `ll-render` 的
//! `p1_acceptance::layout` 一致（见其模块文档）。出生点搜索与山脊雕刻
//! 这类改动 [`ll_world::chunk::ChunkGrid`] 内容的逻辑不在本文件，
//! 放在 [`crate::spawn`]——本文件只留「给定数据现算现出」的纯函数。
//!
//! # 常量必须只有一处定义
//!
//! 世界尺寸、迷你地图版式这些数字如果在 `main.rs` 与本模块测试里各抄
//! 一份，两份数字长得一样不代表它们是同一个东西——这条教训来自 P1
//! `p1_acceptance::layout` 模块文档，此处照做：全部收作本模块唯一的
//! 权威定义。

use ll_core::time::{DAYS_PER_SEASON, Season, TICKS_PER_DAY, TICKS_PER_HOUR, Tick};
use ll_render::sprite::{Footprint, Pivot, TILE_SIZE};
use ll_world::light::ambient_light;
use ll_world::terrain::TerrainKind;

/// 演示世界的宽度（格）。
///
/// 必须是噪声格点尺寸（16）的整数倍（否则 `generate_terrain` 拒绝
/// 生成），且大于渲染层相机单帧可见的跨度 43×25 格（否则地形填不满
/// 留黑块，见 `ll_world::chunk::ChunkGrid::new` 文档）。取 512 远大于
/// 两条下限，也让「走到边缘绕回」这条验收点有足够的行走距离可演示，
/// 不会一开局就贴着接缝。
pub(crate) const WORLD_WIDTH: u32 = 512;

/// 演示世界的高度（格），理由同 [`WORLD_WIDTH`]。
pub(crate) const WORLD_HEIGHT: u32 = 320;

/// 视野基准半径（格），随光照按 [`ll_world::light::sight_radius_at`]
/// 缩放。取 14：小于相机视口的半跨度（约 21 格），能在画面内看见
/// 「视野边界之外是黑的」这一圈，同时大到能稳定包住出生点旁人工
/// 摆放的山脊（见 [`crate::spawn::WALL_RIDGE_OFFSET`]/
/// [`crate::spawn::WALL_RIDGE_LEN`]）。
pub(crate) const BASE_SIGHT_RADIUS: u32 = 14;

/// 世界时钟每次按等待键推进的刻度数：一小时，足以让昼夜光照曲线
/// （见 `ll_world::light`）在几次按键内就跨越日出/日落窗口。
pub(crate) const CLOCK_STEP_HOUR: i64 = TICKS_PER_HOUR;

/// demo 开局时把时钟从 [`ll_world::state::WorldState::new`] 恒定的
/// 午夜（`Tick(0)`）推进到的初始刻度：正午。
///
/// `WorldState::new` 固定从零开始计时（见其文档），零点恰好是一天里
/// 最暗的时刻（[`ll_world::light::ambient_light`] 的「午夜」取值）。
/// 若不调整，开局第一帧的主视口会因为亮度只有个位百分比而几乎全黑，
/// 「能看出水/沙/草/林/山/雪的分布」这条验收点就只能靠小地图（本 demo
/// 里小地图不受光照影响，见 `push_minimap` 文档）撑住，主视口本身却
/// 演示不出来。把开局时刻改到正午纯粹是呈现层的选择——只是在世界
/// 创建后立刻调用一次公开的 [`ll_world::state::WorldState::advance`]，
/// 不改变时钟本身的运转方式，玩家仍可按等待/确认键继续推进，正常体验
/// 昼夜与四季变化。
pub(crate) const INITIAL_CLOCK_TICKS: i64 = 12 * TICKS_PER_HOUR;

/// 世界时钟每次按确认键推进的刻度数：一整个季节长度，用于快速演示
/// 「随季节变色」，不必等几十次按键才跨过一个季节边界。
pub(crate) const CLOCK_STEP_SEASON: i64 = DAYS_PER_SEASON * TICKS_PER_DAY;

/// 小地图每格对应的世界格数（下采样倍率），供
/// [`ll_world::overview::continent_map`] 使用。
pub(crate) const MINIMAP_DOWNSAMPLE: u32 = 8;

/// 小地图每格在离屏目标上的像素边长。
///
/// 取 2：512×320 的世界按 8 倍下采样得到 64×40 格，乘 2 像素得到
/// 128×80 像素的小地图，能塞进 640×360 视口的角落而不过分遮挡主画面。
pub(crate) const MINIMAP_CELL_PX: i32 = 2;

/// 小地图左上角与离屏目标左上角的留白（像素）。
pub(crate) const MINIMAP_MARGIN_PX: i32 = 4;

/// 把地形种类映射到图集条目名。
///
/// 只覆盖本 demo 会用到的种类：八种自然地形（含用作演示山脊的
/// [`TerrainKind::MOUNTAIN`]，见 [`crate::spawn`]）。其余种类（建筑
/// 地形等）本 demo 的世界里不会出现，返回 [`None`]，调用方据此跳过
/// 绘制并记一条日志——这与 `p1_acceptance` 的 `GpuResources::lookup`
/// 对「查不到条目」的处理方式一致：不假设调用方传入的地形一定在映射
/// 表里。
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

/// 季节对画面颜色的调制，`[r, g, b]` 各分量在 `[0, 1]` 内。
///
/// [`ll_world::light::season_light_scale`] 只管亮度（千分比标量），
/// 不改变色相；「随季节变色」这条验收点要求的是色相本身的变化，
/// `ll_world::light` 没有提供色相表——查过该模块全部导出，只有亮度
/// 缩放，没有色彩相关 API，这是纯呈现层的取舍，不属于世界状态，因此
/// 留在本 demo 里现算，不需要请求 `ll_world` 新增接口。四季色调只是
/// 视觉呈现，不产出任何要进世界状态或存档的数值。
fn season_tint(season: Season) -> [f32; 3] {
    match season {
        Season::Spring => [0.80, 1.00, 0.82],
        Season::Summer => [1.00, 0.98, 0.85],
        Season::Autumn => [1.00, 0.80, 0.55],
        Season::Winter => [0.72, 0.82, 1.00],
    }
}

/// 求某一世界时刻的画面颜色调制：昼夜亮度（[`ambient_light`]）乘以
/// 季节色相（[`season_tint`]），alpha 恒为 1（不透明）。
///
/// 全程 `f32`：这是渲染层的呈现细节，不回流入 [`ll_world::state::WorldState`]
/// ——调用方只传入只读的 `Tick` 现算现出，不缓存、不持有。
pub(crate) fn ambient_tint(clock: Tick) -> [f32; 4] {
    let light = ambient_light(clock);
    let brightness = (light.0.clamp(0, 1000) as f32) / 1000.0;
    let [r, g, b] = season_tint(clock.season());
    [r * brightness, g * brightness, b * brightness, 1.0]
}

/// 小地图第 `(col, row)` 格在离屏目标像素空间中的左上角位置。
pub(crate) fn minimap_cell_screen_pos(col: u32, row: u32) -> (i32, i32) {
    (
        MINIMAP_MARGIN_PX + col as i32 * MINIMAP_CELL_PX,
        MINIMAP_MARGIN_PX + row as i32 * MINIMAP_CELL_PX,
    )
}

/// 占地 `footprint` 格、左上角像素原点为 `tile_origin` 的图块，其锚点
/// （占地区域底边水平中点）在离屏渲染目标像素空间中的位置。
///
/// 与 `ll-render` 的 `p1_acceptance::layout::footprint_anchor_pixel`
/// 逐字同构：同一条渲染规则（脚站在占地格块的底边中点）没有理由在
/// 两个 demo 里各写一份、算法还可能悄悄漂移。
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
/// `screen_tile_y` 必须是占地左上角格的**屏幕**纵坐标（`Camera::world_to_screen`
/// 的返回值），不能是世界纵坐标——环面世界里跨南北接缝时世界纵坐标
/// 的排序会反转，详见 `ll_render::sprite::DrawOrder::new` 文档。
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
        // demo 的世界里不会出现建筑地形，映射表也确实没覆盖它——
        // 这条测试锁住「没覆盖」是刻意的，不是遗漏。
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
    fn 午夜的画面色调暗于正午() {
        // Arrange
        let noon = Tick(12 * TICKS_PER_HOUR);
        let midnight = Tick(0);

        // Act
        let [noon_r, noon_g, noon_b, _] = ambient_tint(noon);
        let [night_r, night_g, night_b, _] = ambient_tint(midnight);

        // Assert：三个分量都应变暗，而不只是某一个分量巧合更小。
        assert!(night_r < noon_r);
        assert!(night_g < noon_g);
        assert!(night_b < noon_b);
    }

    #[test]
    fn 冬季与夏季正午的色调不同() {
        // 「随季节变色」要求的是色相变化，不只是亮度——这条测试比较
        // 同为正午（昼夜亮度曲线相同取值点）的两个季节，任何差异都必然
        // 来自季节色相而非光照曲线本身。
        // Arrange
        let summer_noon = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);
        let winter_noon = Tick(90 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);

        // Act
        let summer = ambient_tint(summer_noon);
        let winter = ambient_tint(winter_noon);

        // Assert
        assert_ne!(summer, winter);
    }

    #[test]
    fn 小地图第一格贴着留白角落() {
        // Arrange & Act
        let (x, y) = minimap_cell_screen_pos(0, 0);

        // Assert
        assert_eq!((x, y), (MINIMAP_MARGIN_PX, MINIMAP_MARGIN_PX));
    }

    #[test]
    fn 小地图相邻格相差一个格宽() {
        // Arrange & Act
        let (x0, _) = minimap_cell_screen_pos(0, 0);
        let (x1, _) = minimap_cell_screen_pos(1, 0);

        // Assert
        assert_eq!(x1 - x0, MINIMAP_CELL_PX);
    }

    #[test]
    fn 单格占地精灵锚点在格底边中点() {
        // Arrange
        let footprint = Footprint {
            width: 1,
            height: 1,
        };

        // Act
        let (ax, ay) = footprint_anchor_pixel((100, 200), footprint);

        // Assert
        assert_eq!(
            (ax, ay),
            (100 + TILE_SIZE as i32 / 2, 200 + TILE_SIZE as i32)
        );
    }

    #[test]
    fn 精灵绘制位置比占地顶部更靠上() {
        // hero_idle_0 贴图 16x24 却只占 1x1 格——绘制原点的 y 必须比
        // 单纯的格子顶部再往上偏移，否则贴图会从占地区域内部才开始画。
        // Arrange
        let footprint = Footprint {
            width: 1,
            height: 1,
        };
        let pivot = Pivot { x: 8, y: 24 };

        // Act
        let [_, draw_y] = sprite_draw_position((100, 200), footprint, pivot);

        // Assert：占地顶部是 200，绘制原点应比它更靠上（更小）。
        assert!(draw_y < 200.0);
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
