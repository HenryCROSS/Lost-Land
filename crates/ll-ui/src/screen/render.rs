//! 把 [`super::build_screen_panel`] 算出的内容推进
//! [`UiLayer::Modal`] 那一层。
//!
//! # 本模块此前自己提交，规格 N9 之后不再
//!
//! 原文说的是「本模块只有一块屏、不需要分层：模态屏恒盖在 HUD 全部层级
//! 之上……本模块画的是**盖在 HUD 之上**的第四条渲染通道，调用方在
//! `render_hud` 之后调用它」。那条安排让「谁盖住谁」**只由调用点两句话
//! 的书写顺序决定**——`crate::widget::layer` 模块文档开头那条实机缺陷
//! 就是这个形状。
//!
//! 规格 §7.5 N9 因此裁定新增 [`UiLayer::Modal`]。本模块现在只负责
//! **往那一层里推**（[`push_screen_layer`]），提交交给全 crate 唯一的
//! 出口 [`crate::widget::submit::submit_frame`]。压暗背板照旧把世界层与
//! 整个 HUD 一起压暗，只不过这件事现在由**层级**保证，不由调用顺序保证。
//!
//! # 为什么压暗背板恒走纯色，不查皮肤
//!
//! 背板不是一块「面板」，是一层滤镜：它要的就是均匀压暗，没有边框、
//! 没有九宫格、也不该随皮肤换外观。这与
//! [`crate::hud::world_map::world_map_frame`]「恒只产出纯色矩形，从不
//! 产出贴图矩形」是同一类刻意例外，不是漏了查皮肤。

use ll_i18n::Catalog;

use ll_text::MeasureText;

use super::{ScreenData, build_screen_panel};
use crate::widget::highlight;
use crate::widget::layer::{LayeredFrame, UiLayer};
use crate::widget::panel::{panel_quads, textured_panel_quads};
use crate::widget::quad::QuadInstance;
use crate::widget::skin::{PanelStyleId, Skin};
use crate::widget::textured_quad::TexturedQuadInstance;

/// 把一块模态屏这一帧的内容推进 `frame` 的 [`UiLayer::Modal`] 层。
///
/// **不自己提交**：提交在 [`crate::widget::submit::submit_frame`]，
/// 取整（规格 L0）也在那里，见本模块文档。
///
/// 调用方（`ll_game::app`）传进来的 `frame` 里通常已经装着这一帧的 HUD
/// ——首页那一刻例外（世界还不存在，HUD 整块不参与），那时它是一个空帧。
/// 两种情形本函数一视同仁：模态屏恒在最上面这件事由层级说了算。
#[allow(clippy::too_many_arguments)]
pub fn push_screen_layer(
    frame: &mut LayeredFrame,
    data: &ScreenData<'_>,
    catalog: &Catalog,
    language: &str,
    skin: &dyn Skin,
    measure: &mut dyn MeasureText,
    screen_width: f32,
    screen_height: f32,
) {
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
    let batch = frame.layer_mut(UiLayer::Modal);
    batch.quads.extend(quads);
    batch.textured_quads.extend(textured_quads);
    batch.labels.extend(content.labels);
}

/// 给聚焦行与悬停行各画一块高亮底——**「模态屏的每一行本身就是按钮」
/// 这句话的视觉部分**，也是规格 F7 落地之后「光标在第几行」**唯一**的
/// 视觉表达（此前还有一份 `"> "` 文字前缀，已经拔掉，见
/// [`crate::screen::screen_text_lines`]）。
///
/// 颜色与皮肤分支都在 [`crate::widget::highlight`]——本函数只决定
/// **哪几行**要高亮。
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
        (hovered, highlight::HOVER_HIGHLIGHT_COLOR),
        (Some(data.cursor), highlight::FOCUS_HIGHLIGHT_COLOR),
    ];
    for (row, color) in rows {
        let Some(rect) = row.and_then(|row| content.row_rects.get(row).copied()) else {
            continue;
        };
        highlight::push_row_highlight(rect, color, skin, quads, textured_quads);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::SCREEN_LINE_HEIGHT;
    use crate::widget::layer::DrawBatch;
    use crate::widget::skin::FlatColorSkin;
    use std::path::Path;

    /// 走生产路径建一帧、并取出模态层——测试全部经它，不自己拼数据。
    fn 模态层(data: &ScreenData<'_>, catalog: &Catalog) -> LayeredFrame {
        let mut frame = LayeredFrame::default();
        push_screen_layer(
            &mut frame,
            data,
            catalog,
            "zh-CN",
            &FlatColorSkin,
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );
        frame
    }

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
            title_args: None,
            rows: &rows,
            cursor: 0,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice: None,
            hovered: None,
        };

        // Act
        let frame = 模态层(&data, &测试目录());

        // Assert
        assert_eq!(frame.layer(UiLayer::Modal).quads[0].size, [1280.0, 720.0]);
    }

    #[test]
    fn 纯色皮肤下不产出任何贴图矩形() {
        // Arrange
        let rows = vec!["甲".to_string()];
        let data = ScreenData {
            title_key: "screen-menu-title",
            title_args: None,
            rows: &rows,
            cursor: 0,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice: None,
            hovered: None,
        };

        // Act
        let frame = 模态层(&data, &测试目录());

        // Assert
        assert!(frame.layer(UiLayer::Modal).textured_quads.is_empty());
    }

    /// 这一帧里那**一块**聚焦高亮——先断言恰好一块，再返回它。
    /// 找不到就 panic 而不是返回 `None`：一个「找不到就跳过」的助手会
    /// 让调用它的断言在高亮消失那天集体空转。
    fn 唯一的聚焦高亮(frame: &LayeredFrame) -> QuadInstance {
        let 高亮: Vec<_> = frame
            .layer(UiLayer::Modal)
            .quads
            .iter()
            .filter(|q| q.color == highlight::FOCUS_HIGHLIGHT_COLOR)
            .copied()
            .collect();
        assert_eq!(
            高亮.len(),
            1,
            "应当恰好一块聚焦高亮，实际 {} 块",
            高亮.len()
        );
        高亮[0]
    }

    #[test]
    fn 模态屏的内容落在模态层且排在浮层之后() {
        // **规格 N9 在生产路径上的判据**：`crate::widget::layer` 里那条
        // 单元测试证的是「层序对」，本条证的是「模态屏真的推进了那一层」
        // ——两件事，缺一条判据就绕得过去。
        //
        // 反例验证（已实跑）：把 `push_screen_layer` 里
        // `frame.layer_mut(UiLayer::Modal)` 改成 `UiLayer::Popup`，
        // 本条红在「模态层应当拿到这一屏的全部内容」。
        // Arrange：先往浮层推一块，模拟同一帧里开着的世界地图。
        let rows = vec!["甲".to_string()];
        let data = ScreenData {
            title_key: "screen-menu-title",
            title_args: None,
            rows: &rows,
            cursor: 0,
            empty_key: "screen-menu-empty",
            hint_key: "screen-menu-hint",
            notice: None,
            hovered: None,
        };
        let mut frame = LayeredFrame::default();
        frame.layer_mut(UiLayer::Overlay).quads.push(QuadInstance {
            position: [0.0, 0.0],
            size: [10.0, 10.0],
            color: [1.0, 0.0, 0.0, 1.0],
        });

        // Act
        push_screen_layer(
            &mut frame,
            &data,
            &测试目录(),
            "zh-CN",
            &FlatColorSkin,
            &mut crate::测试测量器(),
            1280.0,
            720.0,
        );

        // Assert：先证明模态层真的拿到了东西（否则下面的次序断言在比空气）。
        assert!(
            !frame.layer(UiLayer::Modal).quads.is_empty()
                && !frame.layer(UiLayer::Modal).labels.is_empty(),
            "模态层应当拿到这一屏的全部内容（压暗背板 + 面板 + 文本行）"
        );
        let batches = frame.draw_batches();
        let 浮层 = batches
            .iter()
            .position(|b| matches!(b, DrawBatch::Quads(q) if q.len() == 1 && q[0].color[0] == 1.0))
            .expect("浮层那一块应当在提交序列里");
        let 模态背板 = batches
            .iter()
            .position(|b| matches!(b, DrawBatch::Quads(q) if q[0].size == [1280.0, 720.0]))
            .expect("压暗背板应当在提交序列里");
        assert!(
            浮层 < 模态背板,
            "模态屏必须排在浮层之后，实际 浮层={浮层} 模态={模态背板}"
        );
    }

    #[test]
    fn 模态屏的高亮矩形落在光标那一行上() {
        // **规格 W7 / F7**：行文字里已经没有 `"> "` 了（见
        // `crate::screen` 的「行文字里不再有任何光标记号」），选中态
        // 唯一的表达就是这一块矩形——这条就是「拔掉文字前缀之后哪一行
        // 被选中仍然验得出来」在模态屏这一侧的证据。
        //
        // 走 `push_screen_layer` 这条**生产渲染路径**，期望值从生产
        // 代码自己的 `screen_row_rects` 现取。
        //
        // 反例验证（已实跑）：把 `push_row_highlights` 里
        // `(Some(data.cursor), …)` 改成 `(Some(0), …)`，本条红在
        // 「光标在第 1 行时高亮没落在那一行上」。
        // Arrange
        let catalog = 测试目录();
        let rows: Vec<String> = (0..4).map(|n| format!("行{n}")).collect();

        for cursor in 0..rows.len() {
            let data = ScreenData {
                title_key: "screen-menu-title",
                title_args: None,
                rows: &rows,
                cursor,
                empty_key: "screen-menu-empty",
                hint_key: "screen-menu-hint",
                notice: None,
                hovered: None,
            };

            // Act
            let frame = 模态层(&data, &catalog);
            let 高亮 = 唯一的聚焦高亮(&frame);

            // Assert
            let 期望 = super::super::screen_row_rects(
                &data,
                &catalog,
                "zh-CN",
                &mut crate::测试测量器(),
                1280.0,
                720.0,
            )[cursor];
            assert_eq!(
                高亮.position,
                [期望.x, 期望.y],
                "光标在第 {cursor} 行时高亮没落在那一行上"
            );
            assert_eq!(高亮.size, [期望.width, 期望.height]);
        }
    }

    #[test]
    fn 光标每下移一行模态屏的高亮就跟着下移一整行高() {
        // 与上一条互补：上一条比的是「高亮 == 第 cursor 行的矩形」，
        // 两边同源；万一行矩形全算成同一个，那一条会照样绿。这一条盯
        // 的正是那种退化。
        //
        // 反例验证（已实跑）：`push_row_highlights` 的下标写死成 0，
        // 本条红在「差 0 应当是 18」。
        // Arrange
        let catalog = 测试目录();
        let rows: Vec<String> = (0..4).map(|n| format!("行{n}")).collect();

        // Act
        let ys: Vec<f32> = (0..rows.len())
            .map(|cursor| {
                let data = ScreenData {
                    title_key: "screen-menu-title",
                    title_args: None,
                    rows: &rows,
                    cursor,
                    empty_key: "screen-menu-empty",
                    hint_key: "screen-menu-hint",
                    notice: None,
                    hovered: None,
                };
                唯一的聚焦高亮(&模态层(&data, &catalog)).position[1]
            })
            .collect();

        // Assert
        for pair in ys.windows(2) {
            assert_eq!(
                pair[1] - pair[0],
                SCREEN_LINE_HEIGHT,
                "相邻两行的高亮应当正好差一整行高，实际 {ys:?}"
            );
        }
    }
}
