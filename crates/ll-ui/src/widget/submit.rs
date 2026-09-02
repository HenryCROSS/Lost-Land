//! 把一帧 UI 提交给 GPU——**全 crate 唯一的提交出口**。
//!
//! # 为什么只许有一个
//!
//! 本模块之前有两个：`hud::render::render_hud` 走
//! [`LayeredFrame::draw_batches`]，`screen::render::render_screen` 自己
//! 按「纯色 → 贴图 → 文本」提交一遍。两条通道的先后**只由调用点两句话
//! 的书写顺序决定**——那正是 [`super::layer`] 模块文档开头那条实机缺陷
//! 的形状：「顺序」这条约定不在任何类型上，读代码看不出来，评审也看不
//! 出来。
//!
//! 规格 §7.5 N9 的裁定是把模态屏收进 [`UiLayer::Modal`](super::layer::UiLayer::Modal)，让
//! `draw_batches` 重新成为遮挡关系的唯一真相源。收进去之后**提交也必须
//! 只有一处**，否则「唯一真相源」这句话仍然只是一句注释。
//!
//! # 三个文本参数为什么可以是常量
//!
//! [`DrawBatch::Labels`] 只说「这一批文本行」，不说它来自哪一层——因此
//! 全帧的字号、行高、文字色必须是同一套。本仓库实测三者本来就相同：
//!
//! | | HUD | 模态屏 |
//! |---|---|---|
//! | 字号 | `hud::DEFAULT_FONT_SIZE` = 14.0 | `screen::SCREEN_FONT_SIZE` = 14.0 |
//! | 行高 | `hud::DEFAULT_LINE_HEIGHT` = 18.0 | `screen::SCREEN_LINE_HEIGHT` = 18.0 |
//! | 文字色 | `rgba(235, 235, 235, 255)` | 同左 |
//!
//! 合帧因此不改任何一个像素。**将来谁要把模态屏的字号调大，改的不是这里
//! 而是这条前提**：那时得让 `DrawBatch` 带上层信息，或者把字号收进
//! [`Label`](super::label::Label)。测试
//! `两块屏共用的字号行高与文字色三者相同` 盯着这条前提。

use ll_render::wgpu;
use ll_text::TextRenderer;

use super::layer::{DrawBatch, LayeredFrame};
use super::quad::QuadRenderer;
use super::textured_quad::TexturedQuadRenderer;
use glyphon::Color;

/// 全帧统一的文本颜色。
///
/// 收在这里而不是 `hud`/`screen` 各存一份：两处此前是**两个取值相同的
/// 私有常量**，那种「靠巧合相等」的东西正是本模块文档最后一节要钉死的。
pub const TEXT_COLOR: Color = Color::rgba(235, 235, 235, 255);

/// 全帧统一的字号（像素）。
pub const FONT_SIZE: f32 = 14.0;

/// 全帧统一的行高（像素）。
pub const LINE_HEIGHT: f32 = 18.0;

/// 把一帧 UI 提交到 `target` 上。
///
/// 收 `&mut LayeredFrame` 而不是 `&LayeredFrame`：**提交那一刻的取整**
/// （规格 L0）就发生在这里，见 [`LayeredFrame::snap_to_pixels`]。此前
/// HUD 与模态屏各自在建帧结尾取一次；收敛到提交出口之后，「提交那一刻」
/// 这句话字面成立，而且新加的任何一层自动被覆盖。
///
/// 文本渲染失败只记日志、不 panic——一行字画不出来不该拖垮整局游戏，
/// 与 `crate::hud` / `crate::screen` 此前各自的降级纪律逐字相同。
#[allow(clippy::too_many_arguments)]
pub fn submit_frame(
    frame: &mut LayeredFrame,
    quad_renderer: &mut QuadRenderer,
    textured_quad_renderer: &mut TexturedQuadRenderer,
    text_renderer: &mut TextRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::TextureView,
    resolution_width: u32,
    resolution_height: u32,
) {
    // 规格 L0：取整发生在**提交那一刻**，中间的布局计算照旧用 `f32`。
    frame.snap_to_pixels();
    for batch in frame.draw_batches() {
        match batch {
            DrawBatch::Quads(quads) => quad_renderer.render(
                device,
                queue,
                target,
                resolution_width,
                resolution_height,
                quads,
            ),
            DrawBatch::Textured(textured) => textured_quad_renderer.render(
                device,
                queue,
                target,
                resolution_width,
                resolution_height,
                textured,
            ),
            DrawBatch::Labels(labels) => {
                let runs: Vec<_> = labels
                    .iter()
                    .map(|label| label.to_text_run(FONT_SIZE, LINE_HEIGHT, TEXT_COLOR))
                    .collect();
                if let Err(error) = text_renderer.render(
                    device,
                    queue,
                    target,
                    resolution_width,
                    resolution_height,
                    &runs,
                ) {
                    tracing::error!(%error, "UI 文本渲染失败");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 两块屏共用的字号行高与文字色三者相同() {
        // **合帧的前置条件**，见模块文档最后一节：`DrawBatch::Labels`
        // 不带层信息，因此 HUD 与模态屏的文本参数必须是同一套。这三个
        // 常量此前分别住在 `hud::render` 与 `screen::render` 里，取值
        // 相同纯属巧合，没有任何东西盯着。
        //
        // 反例验证（已实跑）：把 `screen::SCREEN_FONT_SIZE` 改成 16.0，
        // 本条红在「模态屏的字号必须与提交出口一致」。
        // Arrange & Act & Assert
        assert_eq!(
            crate::hud::DEFAULT_FONT_SIZE,
            FONT_SIZE,
            "HUD 的字号必须与提交出口一致"
        );
        assert_eq!(
            crate::screen::SCREEN_FONT_SIZE,
            FONT_SIZE,
            "模态屏的字号必须与提交出口一致"
        );
        assert_eq!(
            crate::hud::DEFAULT_LINE_HEIGHT,
            LINE_HEIGHT,
            "HUD 的行高必须与提交出口一致"
        );
        assert_eq!(
            crate::screen::SCREEN_LINE_HEIGHT,
            LINE_HEIGHT,
            "模态屏的行高必须与提交出口一致"
        );
    }
}
