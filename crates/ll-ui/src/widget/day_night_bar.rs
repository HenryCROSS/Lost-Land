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
//! 规则的延伸：[`crate::hud::status_bar::status_bar_fields`] 显示的时间
//! 文本永远是 `world.clock` 的瞬时真实值,从不经过任何动画；本模块的
//! 指针位置由调用方（`crate::hud::render::build_hud_frame`）经
//! [`crate::widget::state::WidgetStateTable::animate`] 平滑过渡后再传
//! 进来——**本模块自己不做任何动画计算**,只负责「给定一个已经算好的
//! 归一化位置,画一个指针矩形」这一件事,动画逻辑留在调用点（与
//! [`crate::widget::bar::bar_quads`] 「`fraction` 由调用方算好再传入」
//! 同一条既有分工）。
//!
//! # 曾经的缺陷：指针画了，却被自己的底图吞掉
//!
//! 所有者实机反馈：「时间调，少了滑条……目前只显示了背景条」。
//!
//! **不是没画，也不是没有贴图**——[`day_night_pointer_quad`] 一直在产出
//! 指针矩形，位置换算也一直是对的。问题出在它落进**哪一批**：指针恒是
//! 纯色矩形，而整条底图在真实贴图皮肤下是贴图矩形，而纯色批次恒在贴图
//! 批次**之前**提交（见 [`crate::widget::layer`] 模块文档），于是底图把
//! 指针整个盖掉，屏幕上只剩一条背景。
//!
//! UI 层级（`UiLayer`）**修不了这一条**：底图与指针同属
//! [`crate::widget::layer::UiLayer::Hud`]，而层**内部**仍然是「纯色 →
//! 贴图」两道固定 pass。同一层里互相重叠的两块内容必须落进同一个容器，
//! 才谈得上用推入顺序决定遮挡（[`crate::widget::layer::LayerBatch`]
//! 文档写明了这条要求）。
//!
//! 因此指针现在也有自己的贴图（`ui_daynight_pointer`，见
//! `tools/ll-artgen/src/ui.rs::decorate_day_night_pointer`）：贴图皮肤下
//! 底图与指针同在贴图批次、指针后推，纯色回退下两者同在纯色批次、指针
//! 同样后推——两条路径各自内部有序，不再有跨批次的先后。
//!
//! **不允许「一半贴图一半纯色」**：[`crate::widget::skin`] 里若查不到
//! 指针贴图，整条昼夜滑条（含底图）一起退回纯色，而不是底图走贴图、
//! 指针走纯色——后者恰好复现上面那条缺陷。
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
/// [`crate::widget::bar::TexturedBarAppearance`]。
///
/// **指针也是贴图**（`pointer_uv`）。此前它是纯色，那正是模块文档
/// 「曾经的缺陷」一节记的那条实机问题的根因：纯色指针与贴图底图分处
/// 两道 pass，底图恒后提交、把指针整个盖住。原来的理由（「一个几像素
/// 宽的指示条不值得单独烧一张贴图」）在**开销**上没错，但它默认了两者
/// 的先后由推入顺序决定——那个前提是错的。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexturedDayNightBarAppearance {
    /// 整条贴图（`ui_daynight_bar`，见 `tools/ll-artgen/src/ui.rs`）在
    /// 图集里的 UV 矩形——左夜右昼的渐变，整条采样，不做任何裁切。
    pub track_uv: [f32; 4],
    /// 整条颜色调制。
    pub track_tint: [f32; 4],
    /// 指针贴图（`ui_daynight_pointer`）在图集里的 UV 矩形。
    pub pointer_uv: [f32; 4],
    /// 指针颜色调制——贴图本身已经是描边 + 暖白主体，默认不再染色。
    pub pointer_tint: [f32; 4],
}

/// 指针矩形的宽度（像素）。
///
/// 从 4 加宽到 8：指针现在是一张有描边、有中央竖槽的**滑块**贴图
/// （所有者原话「少了滑条，这个可能需要你另外画一个」），4 像素宽装不
/// 下「描边 + 主体 + 描边」这三段还留得出主体，拉伸后只会糊成一条。8
/// 与贴图画布的 8×16 同宽同比例，横向不做任何拉伸。
pub const POINTER_WIDTH: f32 = 8.0;

/// 指针这一帧的矩形——纯色与贴图两条路径**共用同一份几何**。
///
/// `pointer_fraction` 是 0.0（当日 00:00，最左）到 1.0（次日 00:00 前
/// 一刻，最右）之间的归一化位置，钳制到 `[0.0, 1.0]` 理由同
/// [`crate::widget::bar::bar_quads`]（防御调用方传入越界值，不代表越界
/// 是预期状态）。可移动距离整体收缩 [`POINTER_WIDTH`]，使指针在
/// `pointer_fraction == 1.0` 时仍然完整落在 `rect` 内部，不会有一半探出
/// 滑条右边界。
///
/// 抽成一个函数而不是在两条路径里各写一遍：位置算法一旦分叉，贴图皮肤
/// 与纯色回退下的指针会停在不同的地方，而这种差异只有同时看两套皮肤才
/// 发现得了。
fn pointer_rect(rect: Rect, pointer_fraction: f32) -> Rect {
    let clamped = pointer_fraction.clamp(0.0, 1.0);
    let travel = (rect.width - POINTER_WIDTH).max(0.0);
    Rect::new(
        rect.x + travel * clamped,
        rect.y,
        POINTER_WIDTH,
        rect.height,
    )
}

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

/// 产出**纯色回退**路径的指针矩形，几何见 [`pointer_rect`]。
pub fn day_night_pointer_quad(
    rect: Rect,
    pointer_color: [f32; 4],
    pointer_fraction: f32,
) -> QuadInstance {
    let pointer = pointer_rect(rect, pointer_fraction);
    QuadInstance {
        position: [pointer.x, pointer.y],
        size: [pointer.width, pointer.height],
        color: pointer_color,
    }
}

/// 产出**贴图**路径的指针矩形，几何与 [`day_night_pointer_quad`] 逐位
/// 相同（两者共用 [`pointer_rect`]）。
///
/// 调用方必须把它推在 [`textured_day_night_bar_quads`] **之后**——同一
/// 批贴图矩形里后推的画在上面，见模块文档「曾经的缺陷」一节。
pub fn textured_day_night_pointer_quad(
    rect: Rect,
    style: &TexturedDayNightBarAppearance,
    pointer_fraction: f32,
) -> TexturedQuadInstance {
    let pointer = pointer_rect(rect, pointer_fraction);
    TexturedQuadInstance {
        position: [pointer.x, pointer.y],
        size: [pointer.width, pointer.height],
        uv_rect: style.pointer_uv,
        color: style.pointer_tint,
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
            pointer_uv: [0.0, 0.5, 1.0, 0.5],
            pointer_tint: [1.0, 1.0, 1.0, 1.0],
        };

        // Act
        let flat = day_night_bar_quads(rect, &FlatDayNightBarAppearance::DEFAULT);
        let textured = textured_day_night_bar_quads(rect, &textured_style);

        // Assert
        assert_eq!(flat[0].position, textured[0].position);
        assert_eq!(flat[0].size, textured[0].size);
    }

    #[test]
    fn 贴图指针与纯色指针的几何逐位相同() {
        // 两条路径共用 `pointer_rect`。这条钉住它们不会各写一份位置
        // 算法——一旦分叉，换皮肤时指针会停在不同的地方，而这种差异
        // 只有同时看两套皮肤才发现得了。
        // Arrange
        let rect = Rect::new(10.0, 20.0, 300.0, 14.0);
        let style = TexturedDayNightBarAppearance {
            track_uv: [0.0, 0.0, 1.0, 0.5],
            track_tint: [1.0, 1.0, 1.0, 1.0],
            pointer_uv: [0.0, 0.5, 1.0, 0.5],
            pointer_tint: [1.0, 1.0, 1.0, 1.0],
        };

        // Act & Assert：整条取样若干位置，逐个比对。
        for step in 0..=10 {
            let fraction = step as f32 / 10.0;
            let flat = day_night_pointer_quad(rect, [1.0, 1.0, 1.0, 1.0], fraction);
            let textured = textured_day_night_pointer_quad(rect, &style, fraction);
            assert_eq!(
                flat.position, textured.position,
                "比例 {fraction} 位置不一致"
            );
            assert_eq!(flat.size, textured.size, "比例 {fraction} 尺寸不一致");
        }
    }

    #[test]
    fn 贴图指针采的是指针uv不是底图uv() {
        // 「指针有了自己的贴图」这条的直接核实——若有人图省事让指针复用
        // `track_uv`，屏幕上会是一小片渐变底图而不是滑块。
        // Arrange
        let rect = Rect::new(0.0, 0.0, 300.0, 14.0);
        let style = TexturedDayNightBarAppearance {
            track_uv: [0.0, 0.0, 1.0, 0.5],
            track_tint: [1.0, 1.0, 1.0, 1.0],
            pointer_uv: [0.0, 0.5, 1.0, 0.5],
            pointer_tint: [1.0, 1.0, 1.0, 1.0],
        };

        // Act
        let pointer = textured_day_night_pointer_quad(rect, &style, 0.5);

        // Assert
        assert_eq!(pointer.uv_rect, style.pointer_uv);
        assert_ne!(pointer.uv_rect, style.track_uv);
    }
}
