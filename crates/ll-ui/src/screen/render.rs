//! 把 [`super::build_screen_panel`] 算出的内容真正提交到屏幕。
//!
//! 三道 pass（纯色矩形 → 贴图矩形 → 文本）与
//! [`crate::hud::render::render_hud`] 的**层内**顺序同构，且同样全部用
//! `LoadOp::Load` 不清屏。区别在于本模块只有一块屏、不需要分层：模态屏
//! 恒盖在 HUD 全部层级之上（见 [`crate::widget::layer`] 模块文档「模态屏
//! 不在这里」一节），因此不存在「这块屏与那块屏谁在上面」的问题。本模块画的
//! 是**盖在 HUD 之上**的第四条通道，调用方
//! （`ll_game::app::Demo::on_frame`）在 `render_hud` 之后调用它，压暗
//! 背板因此会把世界层与 HUD 一起压暗，这正是「这块屏是模态的」要传达
//! 的视觉信息。
//!
//! # 为什么压暗背板恒走纯色，不查皮肤
//!
//! 背板不是一块「面板」，是一层滤镜：它要的就是均匀压暗，没有边框、
//! 没有九宫格、也不该随皮肤换外观。这与
//! [`crate::hud::world_map::world_map_frame`]「恒只产出纯色矩形，从不
//! 产出贴图矩形」是同一类刻意例外，不是漏了查皮肤。

use ll_i18n::Catalog;
use ll_render::wgpu;
use ll_text::TextRenderer;

use ll_text::MeasureText;

use super::{SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, ScreenData, build_screen_panel};
use crate::widget::panel::{panel_quads, textured_panel_quads};
use crate::widget::quad::{QuadInstance, QuadRenderer};
use crate::widget::skin::{PanelStyleId, Skin};
use crate::widget::textured_quad::{TexturedQuadInstance, TexturedQuadRenderer};
use glyphon::Color;

/// 文本颜色——与 HUD 面板统一，见 `crate::hud::render` 同名常量的理由。
const TEXT_COLOR: Color = Color::rgba(235, 235, 235, 255);

/// 一块模态屏这一帧全部需要提交给 GPU 的内容。
///
/// **刻意不用 [`crate::widget::layer::LayeredFrame`]**：那个类型解决的是
/// 「同一块画面里多块内容谁盖谁」，而模态屏这一层只有一块内容，整层又恒
/// 盖在 HUD 之上。套一层层级只会多出三个永远为空的层。
pub struct ScreenFrame {
    /// 皮肤给出纯色回退时的填色矩形，以及**恒存在**的压暗背板。
    pub quads: Vec<QuadInstance>,
    /// 皮肤给出真实贴图外观时的贴图矩形。
    pub textured_quads: Vec<TexturedQuadInstance>,
    /// 全部文本行。
    pub labels: Vec<crate::widget::label::Label>,
}

/// 现算这一帧模态屏需要的全部矩形与文本行——纯函数，不接触 GPU。
pub fn build_screen_frame(
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    skin: &dyn Skin,
    measure: &mut dyn MeasureText,
    screen_width: f32,
    screen_height: f32,
) -> ScreenFrame {
    let content = build_screen_panel(
        data,
        catalog,
        language,
        measure,
        screen_width,
        screen_height,
    );
    let mut quads = vec![super::backdrop_quad(content.backdrop)];
    let mut textured_quads = Vec::new();
    match skin.textured_panel(PanelStyleId::Window) {
        Some(appearance) => textured_quads.extend(textured_panel_quads(content.panel, &appearance)),
        None => quads.extend(panel_quads(
            content.panel,
            &skin.panel(PanelStyleId::Window),
        )),
    }
    push_row_highlights(data, &content, skin, &mut quads, &mut textured_quads);
    ScreenFrame {
        quads,
        textured_quads,
        labels: content.labels,
    }
}

/// 给聚焦行与悬停行各画一块高亮底——**「模态屏的每一行本身就是按钮」
/// 这句话的视觉部分**。
///
/// # 为什么跟着面板走同一个分支
///
/// 纯色与贴图是**两道 pass**，同一层里纯色永远被贴图盖住（见
/// `crate::widget::layer` 模块文档）。高亮若一律走纯色，那么装了窗口
/// 贴图的皮肤下它会被面板整块盖掉——玩家就再也看不到自己选中了哪一行。
/// 因此这里照抄面板自己那个 `match`：面板走贴图，高亮也走贴图（拿面板
/// 的填充 UV，用高亮色当调制），面板走纯色，高亮也走纯色。
///
/// 聚焦行与悬停行落在同一行时只画聚焦那一块：两块叠在一起会得到一个
/// 谁都没预期过的第三种颜色。
fn push_row_highlights(
    data: &ScreenData<'_>,
    content: &super::ScreenContent,
    skin: &dyn Skin,
    quads: &mut Vec<QuadInstance>,
    textured_quads: &mut Vec<TexturedQuadInstance>,
) {
    let hovered = data.hovered.filter(|row| *row != data.cursor);
    let rows = [
        (hovered, super::HOVER_HIGHLIGHT_COLOR),
        (Some(data.cursor), super::FOCUS_HIGHLIGHT_COLOR),
    ];
    for (row, color) in rows {
        let Some(rect) = row.and_then(|row| content.row_rects.get(row).copied()) else {
            continue;
        };
        match skin.textured_panel(PanelStyleId::Window) {
            Some(appearance) => textured_quads.push(TexturedQuadInstance {
                position: [rect.x, rect.y],
                size: [rect.width, rect.height],
                uv_rect: appearance.fill_uv,
                color,
            }),
            None => quads.push(super::row_highlight_quad(rect, color)),
        }
    }
}

/// 把一块模态屏画到 `target` 上。渲染失败只记日志、不 panic，与
/// [`crate::hud::render::render_hud`] 同一条降级纪律。
#[allow(clippy::too_many_arguments)]
pub fn render_screen(
    quad_renderer: &mut QuadRenderer,
    textured_quad_renderer: &mut TexturedQuadRenderer,
    text_renderer: &mut TextRenderer,
    // 见 `crate::hud::render::render_hud` 同名参数。
    measure: &mut dyn MeasureText,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::TextureView,
    resolution_width: u32,
    resolution_height: u32,
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    skin: &dyn Skin,
) {
    let frame = build_screen_frame(
        data,
        catalog,
        language,
        skin,
        measure,
        resolution_width as f32,
        resolution_height as f32,
    );

    quad_renderer.render(
        device,
        queue,
        target,
        resolution_width,
        resolution_height,
        &frame.quads,
    );
    textured_quad_renderer.render(
        device,
        queue,
        target,
        resolution_width,
        resolution_height,
        &frame.textured_quads,
    );

    render_labels(
        text_renderer,
        device,
        queue,
        target,
        resolution_width,
        resolution_height,
        &frame.labels,
    );
}

/// 把一屏文本行交给 [`TextRenderer`]——抽出来只为让
/// [`render_screen`] 不越过本仓库 50 行的函数上限；`Label` 借出的
/// `TextRun` 生命周期不能超过 `labels`，所以两步必须在同一个作用域里
/// 完成（见 `crate::widget::label::Label::to_text_run` 文档）。
fn render_labels(
    text_renderer: &mut TextRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::TextureView,
    resolution_width: u32,
    resolution_height: u32,
    labels: &[crate::widget::label::Label],
) {
    let runs: Vec<_> = labels
        .iter()
        .map(|label| {
            // 断行宽度取自标签自己（= 这块面板的内容宽），不再是
            // 面板宽本身——此前传的是 `SCREEN_WIDTH`，连自己那两侧
            // 各 10px 的内边距都没减掉，最后几个字会压在边框上。
            label.to_text_run(SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, TEXT_COLOR)
        })
        .collect();
    if let Err(error) = text_renderer.render(
        device,
        queue,
        target,
        resolution_width,
        resolution_height,
        &runs,
    ) {
        tracing::error!(%error, "模态屏文本渲染失败");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::skin::FlatColorSkin;
    use std::path::Path;

    fn 测试目录() -> Catalog {
        Catalog::load_one(
            crate::TEST_LOCALE_NAMESPACE,
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/locales")),
        )
    }

    #[test]
    fn 压暗背板恒是产出的第一块矩形() {
        // 顺序要紧：背板必须画在面板底下，否则面板会被自己的背板盖住。
        // Arrange
        let rows = vec!["甲".to_string()];
        let data = ScreenData {
            title_key: "screen-menu-title",
            rows: &rows,
            cursor: 0,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice: None,
            hovered: None,
        };

        // Act
        let frame = build_screen_frame(
            &data,
            &测试目录(),
            "zh-CN",
            &FlatColorSkin,
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert
        assert_eq!(frame.quads[0].size, [1280.0, 720.0]);
    }

    #[test]
    fn 纯色皮肤下不产出任何贴图矩形() {
        // Arrange
        let rows = vec!["甲".to_string()];
        let data = ScreenData {
            title_key: "screen-menu-title",
            rows: &rows,
            cursor: 0,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice: None,
            hovered: None,
        };

        // Act
        let frame = build_screen_frame(
            &data,
            &测试目录(),
            "zh-CN",
            &FlatColorSkin,
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert
        assert!(frame.textured_quads.is_empty());
    }
}
