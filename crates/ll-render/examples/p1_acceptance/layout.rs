//! 与 GPU 无关的纯计算：UV 归一化、地形花色、巡逻路径、精灵摆放。
//!
//! 单独成模块是为了让这些逻辑可以脱离 [`super::GpuResources`] 被单测
//! 覆盖——它们都是纯函数，不需要真实窗口或图形适配器。

use ll_core::torus::TorusPos;
use ll_platform::window::FrameId;
use ll_render::atlas::FrameRect;
use ll_render::sprite::{Footprint, Pivot, TILE_SIZE};

/// 把图集条目的像素矩形换算成 [`ll_render::batch::SpriteInstance::uv_rect`]
/// 需要的归一化 `(u, v, width, height)`。
///
/// 两个换算陷阱在这里被处理：
///
/// 1. **必须除以图集纹理的真实像素尺寸**（`image_width`/`image_height`），
///    不是逻辑分辨率 640×360——图集与离屏渲染目标是两张完全不同尺寸的
///    纹理，用错分母会让整张贴图的采样坐标系全错，表现为贴图整体错位
///    或被拉伸/压缩，而不是某一处局部瑕疵。
/// 2. **半 texel 内缩**：即便采样器固定最近邻（见 `atlas.rs`/`target.rs`
///    对 `FilterMode::Nearest` 的选择），把 UV 精确算在两个纹素的分界线
///    上仍可能因浮点误差被舍入到分界线另一侧，采样出邻居贴图的颜色——
///    这是像素图集最常见也最难定位的花屏成因之一：现象是「某个精灵的
///    一条边缘偶尔混进了旁边贴图的颜色」，而且往往只在特定缩放或特定
///    GPU 上出现，元数据与代码本身完全看不出问题。这里把矩形四边各
///    内缩最多 0.5 像素（不足 0.5 像素宽/高的极端帧按实际半宽内缩，
///    避免缩成负数）：内缩幅度远小于一个纹素，最近邻过滤仍稳定选中
///    同一个纹素，不会引入任何肉眼可见的裁切。
pub(crate) fn normalized_uv_rect(rect: FrameRect, image_width: u32, image_height: u32) -> [f32; 4] {
    let inset_x = axis_inset(rect.width);
    let inset_y = axis_inset(rect.height);
    let image_width = image_width as f32;
    let image_height = image_height as f32;

    [
        (rect.x as f32 + inset_x) / image_width,
        (rect.y as f32 + inset_y) / image_height,
        (rect.width as f32 - 2.0 * inset_x) / image_width,
        (rect.height as f32 - 2.0 * inset_y) / image_height,
    ]
}

/// 单边内缩量：正常帧取半个纹素（0.5px），窄于 1px 的极端帧退化为
/// 半宽，保证 `size - 2*inset` 恒不为负。
fn axis_inset(size: u16) -> f32 {
    (size as f32 / 2.0).min(0.5)
}

/// 世界格坐标决定地形贴图种类：奇偶棋盘格。
///
/// 用棋盘格而非纯色铺满，是因为纯色地形无法用肉眼验证「相机移动是否
/// 连续、绕回处是否无缝」——单色画面里，画面撕裂或错位的瓦片和正常
/// 瓦片长得一模一样。棋盘格的格线本身就是最敏感的错位探测器。
pub(crate) fn terrain_entry_name(pos: TorusPos) -> &'static str {
    if (pos.x() + pos.y()) % 2 == 0 {
        "terrain_grass"
    } else {
        "terrain_dirt"
    }
}

/// hero 巡逻路径在给定帧号时所在的纵坐标：在 `min_y`/`max_y` 之间往返，
/// 纯整数三角波。
///
/// 全程整数运算：世界状态不允许浮点（见 crate 顶层文档「浮点边界」），
/// 巡逻路径虽只存在于这个 demo 里，仍遵循同一纪律。
pub(crate) fn hero_patrol_y(frame: FrameId, min_y: i32, max_y: i32, frames_per_step: u64) -> i32 {
    let span = max_y - min_y;
    let period = (span as u64) * 2;
    let step = (frame.0 / frames_per_step) % period.max(1);
    let offset = if step <= span as u64 {
        step as i32
    } else {
        2 * span - step as i32
    };
    min_y + offset
}

/// 占地 `footprint` 格、左上角像素原点为 `tile_origin` 的图块，其锚点
/// （占地区域底边水平中点）在离屏目标像素空间中的位置。
///
/// 无论是 1×1 的普通单位还是 2×2 的重点目标，锚点规则统一：脚站在
/// 占地格块的底边中点，而不是某个角上——这正是 [`Footprint`] 与
/// [`Pivot`] 解耦要支撑的表现（见 `ll_render::sprite` 模块文档）。
pub(crate) fn footprint_anchor_pixel(tile_origin: (i32, i32), footprint: Footprint) -> (i32, i32) {
    let half_width_px = footprint.width as i32 * TILE_SIZE as i32 / 2;
    let height_px = footprint.height as i32 * TILE_SIZE as i32;
    (tile_origin.0 + half_width_px, tile_origin.1 + height_px)
}

/// 把「占地锚点」与「图像内锚点（pivot）」相减，得到精灵图像左上角
/// 应绘制在离屏目标像素空间中的位置。
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

/// 占地格块底边的世界纵坐标（像素），供 `ll_render::sprite::DrawOrder`
/// 用作 `foot_y`。
///
/// **必须用占地底边而非精灵图像顶部**：高精灵若用图像顶部排序，会在
/// 视觉上错误地挡住本该在它前面的矮单位（`ll_render::sprite` 模块文档
/// 对此有详细说明）。
pub(crate) fn footprint_bottom_world_y(tile_y: i32, footprint_height: u8) -> i32 {
    (tile_y + footprint_height as i32) * TILE_SIZE as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::torus::TorusSize;
    use ll_render::sprite::{DrawOrder, Layer};

    const WORLD_WIDTH: u32 = 48;
    const WORLD_HEIGHT: u32 = 32;
    const BOSS_TILE: (i32, i32) = (23, 14);
    const PATROL_MIN_Y: i32 = 8;
    const PATROL_MAX_Y: i32 = 22;
    const PATROL_FRAMES_PER_STEP: u64 = 6;
    const BOSS_ENTITY: u64 = 0;
    const HERO_ENTITY: u64 = 1;

    #[test]
    fn uv矩形按图集真实尺寸而非逻辑分辨率换算() {
        // 这是评审点名的关键陷阱：分母必须是图集像素尺寸（这里 64），
        // 用逻辑分辨率 640 会让整张贴图的采样坐标系全错。
        // Arrange
        let rect = FrameRect {
            x: 0,
            y: 0,
            width: 16,
            height: 24,
        };

        // Act
        let uv = normalized_uv_rect(rect, 64, 72);

        // Assert：宽高各按半 texel 内缩一整像素（两边各 0.5），
        // 分母必须是图集真实尺寸 64/72，而不是逻辑分辨率 640/360。
        assert!((uv[2] - 15.0 / 64.0).abs() < f32::EPSILON);
        assert!((uv[3] - 23.0 / 72.0).abs() < f32::EPSILON);
    }

    #[test]
    fn uv矩形内缩后小于原始像素矩形换算值() {
        // 半 texel 内缩必须真的把矩形往内收，否则起不到防止采样越界到
        // 邻居贴图的作用。
        // Arrange
        let rect = FrameRect {
            x: 16,
            y: 0,
            width: 32,
            height: 48,
        };

        // Act
        let uv = normalized_uv_rect(rect, 64, 72);
        let naive_u = rect.x as f32 / 64.0;
        let naive_width = rect.width as f32 / 64.0;

        // Assert
        assert!(uv[0] > naive_u);
        assert!(uv[2] < naive_width);
    }

    #[test]
    fn 一像素宽的帧内缩后宽度不为负() {
        // 内缩量必须按帧实际宽高钳制，否则极窄的帧会算出负宽度。
        // Arrange
        let rect = FrameRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };

        // Act
        let uv = normalized_uv_rect(rect, 64, 64);

        // Assert
        assert!(uv[2] >= 0.0);
        assert!(uv[3] >= 0.0);
    }

    #[test]
    fn 棋盘格相邻格纹理种类交替() {
        // 棋盘格的意义在于让相机绕回处的错位一眼可见；前提是相邻格
        // 确实交替，而不是意外全同色。
        // Arrange
        let world = TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("常量非零");
        let a = world.wrap(4, 4);
        let b = world.wrap(5, 4);

        // Act & Assert
        assert_ne!(terrain_entry_name(a), terrain_entry_name(b));
    }

    #[test]
    fn 世界接缝两侧的棋盘格仍然交替() {
        // 宽高取偶数正是为了保证这一点：接缝处不应看起来像棋盘格断开。
        // Arrange
        let world = TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("常量非零");
        let last_column = world.wrap(WORLD_WIDTH as i32 - 1, 4);
        let wrapped_first_column = world.wrap(WORLD_WIDTH as i32, 4);

        // Act & Assert
        assert_ne!(
            terrain_entry_name(last_column),
            terrain_entry_name(wrapped_first_column)
        );
    }

    #[test]
    fn 巡逻路径在起点停留于下界() {
        // Arrange & Act
        let y = hero_patrol_y(
            FrameId(0),
            PATROL_MIN_Y,
            PATROL_MAX_Y,
            PATROL_FRAMES_PER_STEP,
        );

        // Assert
        assert_eq!(y, PATROL_MIN_Y);
    }

    #[test]
    fn 巡逻路径会到达上界() {
        // Arrange
        let span = (PATROL_MAX_Y - PATROL_MIN_Y) as u64;
        let frame = FrameId(span * PATROL_FRAMES_PER_STEP);

        // Act
        let y = hero_patrol_y(frame, PATROL_MIN_Y, PATROL_MAX_Y, PATROL_FRAMES_PER_STEP);

        // Assert
        assert_eq!(y, PATROL_MAX_Y);
    }

    #[test]
    fn 巡逻路径到达上界后折返下降() {
        // 三角波过了半程应该往回走，而不是继续单调递增或突然跳变。
        // Arrange
        let span = (PATROL_MAX_Y - PATROL_MIN_Y) as u64;
        let at_peak = FrameId(span * PATROL_FRAMES_PER_STEP);
        let one_step_later = FrameId(at_peak.0 + PATROL_FRAMES_PER_STEP);

        // Act
        let y = hero_patrol_y(
            one_step_later,
            PATROL_MIN_Y,
            PATROL_MAX_Y,
            PATROL_FRAMES_PER_STEP,
        );

        // Assert
        assert_eq!(y, PATROL_MAX_Y - 1);
    }

    #[test]
    fn 普通单位锚点在占地格底边中点() {
        // 1x1 占地的普通单位，锚点应落在这一格的水平中点、底边。
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
    fn 重点目标锚点在两格宽占地的底边中点() {
        // 2x2 占地的重点目标，锚点横坐标应是两格宽度的中点，而不是
        // 单格宽度的中点——这正是 footprint 与 pivot 解耦要覆盖的场景。
        // Arrange
        let footprint = Footprint {
            width: 2,
            height: 2,
        };

        // Act
        let (ax, ay) = footprint_anchor_pixel((100, 200), footprint);

        // Assert
        assert_eq!(
            (ax, ay),
            (100 + TILE_SIZE as i32, 200 + 2 * TILE_SIZE as i32)
        );
    }

    #[test]
    fn 重点目标的绘制位置比自己的占地更靠上() {
        // 32x48 的贴图占 2x2 格却要画得比格子高——绘制原点的 y 必须比
        // 单纯的格子顶部再往上偏移，否则贴图会从占地区域内部才开始画。
        // Arrange
        let footprint = Footprint {
            width: 2,
            height: 2,
        };
        let pivot = Pivot { x: 16, y: 48 };

        // Act
        let [_, draw_y] = sprite_draw_position((100, 200), footprint, pivot);

        // Assert：占地顶部是 200，绘制原点应比它更靠上（更小）。
        assert!(draw_y < 200.0);
    }

    #[test]
    fn 站在重点目标占地最下一行时普通单位排在前面() {
        // hero 的 foot_y 与 boss 的 foot_y 恰好相等时（hero 站在 boss
        // 占地的最后一行），必须由实体号打破平局，且 hero 应该排在后面
        // （画在上层、盖住 boss），因为它此刻站在最靠近镜头的落脚线上。
        // Arrange
        let boss_foot_y = footprint_bottom_world_y(BOSS_TILE.1, 2);
        let hero_tile_y = BOSS_TILE.1 + 1; // boss 占地的最下一行
        let hero_foot_y = footprint_bottom_world_y(hero_tile_y, 1);
        assert_eq!(
            boss_foot_y, hero_foot_y,
            "本测试的前提是两者 foot_y 恰好相等"
        );

        // Act
        let boss_order = DrawOrder::new(Layer::ENTITY, boss_foot_y, BOSS_ENTITY);
        let hero_order = DrawOrder::new(Layer::ENTITY, hero_foot_y, HERO_ENTITY);

        // Assert
        assert!(boss_order < hero_order);
    }

    #[test]
    fn 巡逻路径完全在重点目标上方时重点目标排在前面() {
        // Arrange
        let boss_foot_y = footprint_bottom_world_y(BOSS_TILE.1, 2);
        let hero_foot_y = footprint_bottom_world_y(PATROL_MIN_Y, 1);

        // Act
        let boss_order = DrawOrder::new(Layer::ENTITY, boss_foot_y, BOSS_ENTITY);
        let hero_order = DrawOrder::new(Layer::ENTITY, hero_foot_y, HERO_ENTITY);

        // Assert：boss 排在 hero 之后，也就是画在上层、遮住 hero。
        assert!(hero_order < boss_order);
    }

    #[test]
    fn 巡逻路径完全在重点目标下方时普通单位排在前面() {
        // Arrange
        let boss_foot_y = footprint_bottom_world_y(BOSS_TILE.1, 2);
        let hero_foot_y = footprint_bottom_world_y(PATROL_MAX_Y, 1);

        // Act
        let boss_order = DrawOrder::new(Layer::ENTITY, boss_foot_y, BOSS_ENTITY);
        let hero_order = DrawOrder::new(Layer::ENTITY, hero_foot_y, HERO_ENTITY);

        // Assert
        assert!(boss_order < hero_order);
    }
}
