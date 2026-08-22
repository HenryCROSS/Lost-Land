//! 昼夜滑条：一条横向长条表示一天，一个指针标出当前时刻——项目所有者
//! 的新设计（原话：「时间动画可以是一个滑条，然后有个指针……左边是黑夜
//! 图案右边是白天图案」），替代此前「只有状态栏一行文字」的时间显示。
//!
//! # 与经验条/资源条不是同一类条形——整条恒显示，不按比例裁切
//!
//! [`crate::widget::bar::bar_quads`]/[`crate::widget::bar::two_layer_bar_quads`]
//! 画的是「填了多少」（背景 + 按比例缩放的前景），语义是进度/资源量。
//! 昼夜滑条不一样：它画的是**一整天**，任何时刻这一整条都应该完整可见
//! （左夜右昼的渐变贴图本身就是「一天的全貌」），变化的只是**指针停在
//! 哪个位置**——因此本模块只有一块「整条」矩形（不裁切）+ 一块「指针」
//! 矩形，没有 `fraction` 缩放前景这个概念，与 `bar`/`two_layer_bar`
//! 是两种不同的几何,不应该勉强复用。
//!
//! # 数字瞬时，指针平滑——两者从不共用一个数
//!
//! 与 [`crate::widget::anim`] 模块文档「数字瞬时，条形动画」同一条硬
//! 规则的延伸：[`crate::hud::status_bar::status_bar_text`] 显示的时间
//! 文本永远是 `world.clock` 的瞬时真实值,从不经过任何动画；本模块的
//! 指针位置由调用方（`crate::hud::render::build_hud_frame`）经
//! [`crate::widget::state::WidgetStateTable::animate`] 平滑过渡后再传
//! 进来——**本模块自己不做任何动画计算**,只负责「给定一个已经算好的
//! 归一化位置,画一个指针矩形」这一件事,动画逻辑留在调用点（与
//! [`crate::widget::bar::bar_quads`] 「`fraction` 由调用方算好再传入」
//! 同一条既有分工）。
//!
//! # 颜色走皮肤层
//!
//! 背景整条与指针的颜色都不是本模块决定的——见
//! [`crate::widget::skin::Skin::day_night_bar`]/
//! [`crate::widget::skin::Skin::textured_day_night_bar`]，本模块的两个
//! `*_quads` 函数只认已经解析好的 [`FlatDayNightBarAppearance`]/
//! [`TexturedDayNightBarAppearance`]，与 `panel`/`bar` 两个既有控件同一
//! 条纪律。

use super::geometry::Rect;
use super::quad::QuadInstance;
use super::textured_quad::TexturedQuadInstance;

/// 昼夜滑条的纯色回退外观。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatDayNightBarAppearance {
    /// 整条背景色——纯色回退没有「左夜右昼」的渐变贴图可用，退化成
    /// 一块统一底色，指针本身仍然会画出来，不会因为没有渐变贴图就让
    /// 整条滑条消失。
    pub track_color: [f32; 4],
    /// 指针颜色，需要与 `track_color` 有足够对比度才能一眼看清。
    pub pointer_color: [f32; 4],
}

impl FlatDayNightBarAppearance {
    /// 朴素默认样式：深蓝灰底 + 暖黄指针。
    pub const DEFAULT: FlatDayNightBarAppearance = FlatDayNightBarAppearance {
        track_color: [0.14, 0.16, 0.24, 0.85],
        pointer_color: [0.98, 0.85, 0.35, 1.0],
    };
}

/// 昼夜滑条的真实贴图外观——理由同
/// [`crate::widget::bar::TexturedBarAppearance`]。指针**仍然是纯色**
/// （`pointer_color`，不是贴图 UV）：一个几像素宽的指示条不值得为它
/// 单独烧一张贴图，纯色在任何背景上都能靠调制颜色保持可辨识，见
/// `crate::widget::skin` 模块里 `DAYNIGHT_POINTER_TINT` 的选色说明。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexturedDayNightBarAppearance {
    /// 整条贴图（`ui_daynight_bar`，见 `tools/ll-artgen/src/ui.rs`）在
    /// 图集里的 UV 矩形——左夜右昼的渐变，整条采样，不做任何裁切。
    pub track_uv: [f32; 4],
    /// 整条颜色调制。
    pub track_tint: [f32; 4],
    /// 指针颜色。
    pub pointer_color: [f32; 4],
}

/// 指针矩形的宽度（像素）——足够窄以像一根指针，又足够宽以在原生
/// 分辨率下不因抗锯齿糊成看不清的一条线。
pub const POINTER_WIDTH: f32 = 4.0;

/// 产出昼夜滑条整条背景的填色矩形——恒一块，不裁切，见模块文档
/// 「与经验条/资源条不是同一类条形」一节。
pub fn day_night_bar_quads(rect: Rect, style: &FlatDayNightBarAppearance) -> Vec<QuadInstance> {
    vec![QuadInstance {
        position: [rect.x, rect.y],
        size: [rect.width, rect.height],
        color: style.track_color,
    }]
}

/// 产出昼夜滑条整条背景的贴图矩形，几何与 [`day_night_bar_quads`]
/// 完全相同。
pub fn textured_day_night_bar_quads(
    rect: Rect,
    style: &TexturedDayNightBarAppearance,
) -> Vec<TexturedQuadInstance> {
    vec![TexturedQuadInstance {
        position: [rect.x, rect.y],
        size: [rect.width, rect.height],
        uv_rect: style.track_uv,
        color: style.track_tint,
    }]
}

/// 产出指针矩形——`pointer_fraction` 是 0.0（当日 00:00，最左）到 1.0
/// （次日 00:00 前一刻，最右）之间的归一化位置，钳制到 `[0.0, 1.0]`
/// 理由同 [`crate::widget::bar::bar_quads`]（防御调用方传入越界值，
/// 不代表越界是预期状态）。指针矩形整体向左收缩
/// [`POINTER_WIDTH`]，使指针在 `pointer_fraction == 1.0` 时仍然完整
/// 落在 `rect` 内部，不会有一半探出滑条右边界。
pub fn day_night_pointer_quad(
    rect: Rect,
    pointer_color: [f32; 4],
    pointer_fraction: f32,
) -> QuadInstance {
    let clamped = pointer_fraction.clamp(0.0, 1.0);
    let travel = (rect.width - POINTER_WIDTH).max(0.0);
    QuadInstance {
        position: [rect.x + travel * clamped, rect.y],
        size: [POINTER_WIDTH, rect.height],
        color: pointer_color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_night_bar_quads恒产出一块覆盖整个矩形的背景() {
        // Arrange
        let rect = Rect::new(10.0, 20.0, 300.0, 14.0);

        // Act
        let quads = day_night_bar_quads(rect, &FlatDayNightBarAppearance::DEFAULT);

        // Assert
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].size, [300.0, 14.0]);
    }

    #[test]
    fn day_night_pointer_quad在归一化位置零时贴在矩形左边界() {
        // Arrange
        let rect = Rect::new(10.0, 20.0, 300.0, 14.0);

        // Act
        let pointer = day_night_pointer_quad(rect, [1.0, 1.0, 1.0, 1.0], 0.0);

        // Assert
        assert_eq!(pointer.position[0], 10.0);
    }

    #[test]
    fn day_night_pointer_quad在归一化位置一时仍完整落在矩形内() {
        // Arrange
        let rect = Rect::new(10.0, 20.0, 300.0, 14.0);

        // Act
        let pointer = day_night_pointer_quad(rect, [1.0, 1.0, 1.0, 1.0], 1.0);

        // Assert：指针右边界不应超出滑条右边界。
        assert!(pointer.position[0] + pointer.size[0] <= rect.right());
    }

    #[test]
    fn day_night_pointer_quad对越界比例钳制到零到一之间() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 300.0, 14.0);

        // Act
        let below_zero = day_night_pointer_quad(rect, [1.0, 1.0, 1.0, 1.0], -0.5);
        let above_one = day_night_pointer_quad(rect, [1.0, 1.0, 1.0, 1.0], 1.5);

        // Assert
        assert_eq!(below_zero.position[0], 0.0);
        assert_eq!(above_one.position[0], rect.width - POINTER_WIDTH);
    }

    #[test]
    fn textured_day_night_bar_quads与day_night_bar_quads的几何完全一致() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 300.0, 14.0);
        let textured_style = TexturedDayNightBarAppearance {
            track_uv: [0.0, 0.0, 1.0, 1.0],
            track_tint: [1.0, 1.0, 1.0, 1.0],
            pointer_color: [1.0, 0.9, 0.5, 1.0],
        };

        // Act
        let flat = day_night_bar_quads(rect, &FlatDayNightBarAppearance::DEFAULT);
        let textured = textured_day_night_bar_quads(rect, &textured_style);

        // Assert
        assert_eq!(flat[0].position, textured[0].position);
        assert_eq!(flat[0].size, textured[0].size);
    }
}
