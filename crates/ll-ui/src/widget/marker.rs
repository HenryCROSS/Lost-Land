//! 世界地图上的标记控件：把「标记画在哪一格、占多大」与「标记长什么
//! 样」分开，理由同 [`crate::widget::skin`] 模块文档。
//!
//! # 为什么单独一个模块，而不是塞进 `crate::hud::world_map`
//!
//! `crate::hud::world_map` 已经约 1300 行、**超出规格 §13 的 800 行
//! 上限**（既有违规，本批次不拆——只拆一个文件与仓库现状不一致，见
//! 2026-08-28 交接文档第四节第 8 条）。既然如此，新增的外观数据类型
//! 就不再往那个文件里加，放在它该在的地方：外观数据与
//! [`crate::widget::panel::TexturedPanelAppearance`] 等同属控件层，
//! 由皮肤解析、由 `hud` 消费。

use super::textured_quad::TexturedQuadInstance;

/// 世界地图标记的真实贴图外观，理由同
/// [`crate::widget::bar::TexturedBarAppearance`]。
///
/// **没有对应的 `Flat*` 类型**：纯色回退路径上标记只有一个颜色
/// （`crate::hud::world_map::PLAYER_MARKER_COLOR`），一个 `[f32; 4]`
/// 就够，为它包一层结构体只是多一个名字。这与面板/条形不同——那些的
/// 纯色外观本来就有两三个颜色要分别指定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexturedWorldMapMarkerAppearance {
    /// 标记贴图在图集里的 UV 矩形。
    pub uv: [f32; 4],
    /// 颜色调制——贴图自带描边与主体色，默认不染色。
    pub tint: [f32; 4],
}

/// 产出一块标记的贴图矩形：**整格铺满**，不做任何内缩。
///
/// # 为什么贴图路径不内缩，而纯色路径要内缩
///
/// 纯色路径画的是一块实心方块，不内缩就会把这一格的地形整个盖住，玩家
/// 看不出自己脚下是什么（`crate::hud::world_map::PLAYER_MARKER_INSET_FRACTION`
/// 的文档记的正是这条）。贴图路径不需要这条补偿：箭头自己的留白（四角
/// 与两侧是透明像素）已经把地形露出来了，而且是**沿图形轮廓**露，比一
/// 圈方形留白读起来更像「一个标记落在这一格上」。
///
/// 整格铺满还有一个实打实的好处：地图放到最密的档位时一格只有十来个
/// 像素，再按比例内缩一半，箭头会缩到看不出形状。
pub fn textured_marker_quad(
    x: f32,
    y: f32,
    cell_size: f32,
    style: &TexturedWorldMapMarkerAppearance,
) -> TexturedQuadInstance {
    TexturedQuadInstance {
        position: [x, y],
        size: [cell_size, cell_size],
        uv_rect: style.uv,
        color: style.tint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: TexturedWorldMapMarkerAppearance = TexturedWorldMapMarkerAppearance {
        uv: [0.25, 0.5, 0.125, 0.125],
        tint: [1.0, 1.0, 1.0, 1.0],
    };

    #[test]
    fn textured_marker_quad恒铺满整格() {
        // Arrange & Act
        let quad = textured_marker_quad(10.0, 20.0, 12.0, &SAMPLE);

        // Assert：位置即格子左上角，尺寸即格子边长，没有任何内缩。
        assert_eq!(quad.position, [10.0, 20.0]);
        assert_eq!(quad.size, [12.0, 12.0]);
    }

    #[test]
    fn textured_marker_quad原样采用皮肤给的uv与调制() {
        // Arrange & Act
        let quad = textured_marker_quad(0.0, 0.0, 8.0, &SAMPLE);

        // Assert
        assert_eq!(quad.uv_rect, SAMPLE.uv);
        assert_eq!(quad.color, SAMPLE.tint);
    }
}
