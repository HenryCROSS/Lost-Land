//! 面板：九宫格切片边框——状态栏、角色面板、背包、装备栏共用的背景
//! 容器。
//!
//! # 本模块只管几何,不管「长什么样」——那是 [`crate::widget::skin`] 的事
//!
//! [`panel_quads`] 只认 [`FlatPanelAppearance`] 这一份**已经算好**的
//! 外观数据（颜色、边框厚度）,从不知道调用方为什么选了这几个颜色。
//! HUD 四块面板的调用点（`crate::hud::render`）也不直接构造
//! `FlatPanelAppearance`——它们向 [`crate::widget::skin::Skin`] 要
//! `PanelStyleId::Window` 对应的外观,皮肤决定给纯色还是（未来）九宫格
//! 贴图。这条间接层是项目所有者的硬要求：**换皮肤只应该改「构造哪个
//! `Skin` 实现」这一处,控件（本模块）与调用点代码一行不动**——见
//! `crate::widget::skin` 模块文档的完整论证与两条验收问题的回答。
//!
//! # 没有边框美术，先用纯色，几何形状按真正的九宫格留
//!
//! 核实 `assets/sprites/`（`manifest.json5` 列出的全部条目）与
//! `mods/*/assets/sprites/`：全项目没有任何一张边框/面板类贴图，
//! 只有角色行走帧与地形瓦片。九宫格边框需要的美术资产不存在，本批次
//! 也不越权去画一套——[`panel_quads`] 因此把「一个矩形分解成九块」这个
//! 几何操作做实：四角固定尺寸、四边横向或纵向拉伸、中心双向拉伸，
//! 只是九块目前都填纯色（[`FlatPanelAppearance::border_color`]/
//! [`FlatPanelAppearance::fill_color`]）而不是从贴图采样。真正的边框
//! 美术到位后，九宫格的**几何**（九块怎么分、谁拉伸谁不拉伸）不需要
//! 重新设计——新增一个采样贴图的 `Skin` 实现与配套的纹理渲染路径即可
//! （`crate::widget::skin` 模块文档「加九宫格要改几处」一节），
//! `panel_quads` 的调用方（四块 HUD 面板）完全不需要跟着改。
//!
//! # 为什么不是「四条边 + 一个填充」更省事的五块方案
//!
//! 少画四个角（用边的延伸覆盖）在纯色时确实能省几个 draw
//! 实例，但一旦真正的边框贴图到位，四个角通常带弧度/花纹，不能简单
//! 用边的直线段覆盖——现在就按标准九宫格的九块划分，日后换贴图时
//! 角的位置、尺寸不需要重新推导。

use super::geometry::Rect;
use super::quad::QuadInstance;
use super::textured_quad::TexturedQuadInstance;

/// 面板的纯色样式——真正的边框美术到位前的占位,见模块文档。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatPanelAppearance {
    /// 边框（四角 + 四边）颜色。
    pub border_color: [f32; 4],
    /// 中心填充颜色——通常比边框更透明,让面板背后的世界层若隐若现。
    pub fill_color: [f32; 4],
    /// 边框厚度（像素）——四角是 `厚度 × 厚度` 的正方形,四边是
    /// `厚度` 宽/高、沿另一轴拉伸满面板剩余长度。
    pub border_thickness: f32,
}

impl FlatPanelAppearance {
    /// 一套朴素的默认样式：半透明深色背景 + 浅灰边框,足以在任意
    /// 世界背景前保持可读,不追求美术完成度（见模块文档「没有边框
    /// 美术」一节）。
    pub const DEFAULT: FlatPanelAppearance = FlatPanelAppearance {
        border_color: [0.75, 0.75, 0.8, 0.9],
        fill_color: [0.05, 0.05, 0.08, 0.55],
        border_thickness: 2.0,
    };
}

/// 真实九宫格贴图的外观数据——[`crate::widget::skin::NineSliceSkin`]
/// 消费,颜色调制通常恒为 `[1.0; 4]`（不调制,直接显示贴图真实颜色）,
/// 保留这个字段是为了与 [`textured_panel_quads`] 统一签名,也为「贴图
/// 之上再叠一层染色」（例如警告态面板整体偏红）这类未来需求留一个
/// 现成的挂点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexturedPanelAppearance {
    /// 四角与四边共用的边框贴图在图集里的 UV 矩形。
    pub border_uv: [f32; 4],
    /// 中心填充贴图在图集里的 UV 矩形。
    pub fill_uv: [f32; 4],
    /// 边框颜色调制。
    pub border_tint: [f32; 4],
    /// 中心颜色调制。
    pub fill_tint: [f32; 4],
    /// 边框厚度（像素），语义同 [`FlatPanelAppearance::border_thickness`]。
    pub border_thickness: f32,
}

/// 九宫格的纯几何切分：四角（固定 `thickness × thickness`）+ 四边
/// （沿一个方向拉伸）+ 中心（双向拉伸），共 9 块，顺序固定为
/// 「四角、上边、下边、左边、右边、中心」——[`panel_quads`]/
/// [`textured_panel_quads`] 共用同一份几何,只是分别填色或填贴图 UV,
/// 不是两份重复实现。
///
/// `rect` 的宽高小于 `2 * thickness` 时,边/中心尺寸被钳制到零而不是
/// 负数（见 [`Rect::inset`]）——这种极端小尺寸不是本批次四块面板会
/// 出现的情况,钳制只是防止产出负尺寸矩形导致的未定义绘制行为。
///
/// # 九块全部取整（规格 L0）
///
/// 先把外框 [`Rect::snap`] 一次，再切，切完每一块再 `snap` 一次
/// （取整幂等，第二次只对付非整数的 `thickness`）。**取整的是边界不是
/// 尺寸**，因此四角的右边界恒等于上下边块的左边界——九块之间既不留缝
/// 也不重叠，见 [`Rect::snap`] 文档那一段推导。这是像素风面板边框不糊
/// 的全部原因。
fn nine_slice_rects(rect: Rect, thickness: f32) -> [Rect; 9] {
    let rect = rect.snap();
    let t = thickness;
    let inner = rect.inset(t);
    [
        Rect::new(rect.x, rect.y, t, t),
        Rect::new(rect.right() - t, rect.y, t, t),
        Rect::new(rect.x, rect.bottom() - t, t, t),
        Rect::new(rect.right() - t, rect.bottom() - t, t, t),
        Rect::new(inner.x, rect.y, inner.width, t),
        Rect::new(inner.x, rect.bottom() - t, inner.width, t),
        Rect::new(rect.x, inner.y, t, inner.height),
        Rect::new(rect.right() - t, inner.y, t, inner.height),
        Rect::new(inner.x, inner.y, inner.width, inner.height),
    ]
    .map(|slice| slice.snap())
}

/// 把 `rect` 按九宫格分解成填色矩形——四角固定尺寸,四边与中心按
/// `rect` 实际尺寸拉伸,前 8 块用 `style.border_color`,最后一块（中心）
/// 用 `style.fill_color`。
pub fn panel_quads(rect: Rect, style: &FlatPanelAppearance) -> Vec<QuadInstance> {
    let slices = nine_slice_rects(rect, style.border_thickness);
    slices
        .iter()
        .enumerate()
        .map(|(i, slice)| QuadInstance {
            position: [slice.x, slice.y],
            size: [slice.width, slice.height],
            color: if i == 8 {
                style.fill_color
            } else {
                style.border_color
            },
        })
        .collect()
}

/// 把 `rect` 按九宫格分解成贴图矩形——几何与 [`panel_quads`] 完全相同
/// （见 [`nine_slice_rects`]），前 8 块采样 `style.border_uv`，最后一块
/// （中心）采样 `style.fill_uv`。
pub fn textured_panel_quads(
    rect: Rect,
    style: &TexturedPanelAppearance,
) -> Vec<TexturedQuadInstance> {
    let slices = nine_slice_rects(rect, style.border_thickness);
    slices
        .iter()
        .enumerate()
        .map(|(i, slice)| {
            let is_center = i == 8;
            TexturedQuadInstance {
                position: [slice.x, slice.y],
                size: [slice.width, slice.height],
                uv_rect: if is_center {
                    style.fill_uv
                } else {
                    style.border_uv
                },
                color: if is_center {
                    style.fill_tint
                } else {
                    style.border_tint
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_quads恒产出九块() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);

        // Act
        let quads = panel_quads(rect, &FlatPanelAppearance::DEFAULT);

        // Assert
        assert_eq!(quads.len(), 9);
    }

    #[test]
    fn panel_quads的中心矩形按边框厚度向内收缩() {
        // Arrange
        let rect = Rect::new(10.0, 20.0, 100.0, 60.0);
        let style = FlatPanelAppearance {
            border_thickness: 3.0,
            ..FlatPanelAppearance::DEFAULT
        };

        // Act
        let quads = panel_quads(rect, &style);
        let center = quads.last().expect("九块的最后一块是中心填充");

        // Assert
        assert_eq!(center.position, [13.0, 23.0]);
    }

    #[test]
    fn panel_quads的四角尺寸恒等于边框厚度() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        let style = FlatPanelAppearance {
            border_thickness: 4.0,
            ..FlatPanelAppearance::DEFAULT
        };

        // Act
        let quads = panel_quads(rect, &style);

        // Assert：前四块是四角。
        for corner in &quads[0..4] {
            assert_eq!(corner.size, [4.0, 4.0]);
        }
    }

    #[test]
    fn panel_quads的中心填充颜色与边框颜色不同() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        let style = FlatPanelAppearance::DEFAULT;

        // Act
        let quads = panel_quads(rect, &style);
        let center = quads.last().expect("九块的最后一块是中心填充");

        // Assert
        assert_ne!(center.color, style.border_color);
        assert_eq!(center.color, style.fill_color);
    }

    #[test]
    fn panel_quads对非整数矩形产出的九块边界全是整数且互不留缝重叠() {
        // 规格 L0 的判据原文（`knowledge/design/ui-and-navigation.md`
        // §6.1）。**先自证输入真的带半像素**，否则整条恒绿。
        //
        // 反例验证（已实跑）：把 `nine_slice_rects` 里的
        // `.map(|slice| slice.snap())` 与开头那句 `let rect = rect.snap();`
        // 一起去掉，本条红在「第 0 块的 x 带小数」。
        // Arrange
        let rect = Rect::new(10.3, 20.7, 100.4, 60.6);
        assert!(
            rect.x.fract() != 0.0 && rect.right().fract() != 0.0,
            "测试输入必须真的带半像素"
        );
        let style = FlatPanelAppearance {
            border_thickness: 2.5,
            ..FlatPanelAppearance::DEFAULT
        };

        // Act
        let quads = panel_quads(rect, &style);

        // Assert 一：九块的四条边界全是整数。
        for (i, quad) in quads.iter().enumerate() {
            for (名, v) in [
                ("x", quad.position[0]),
                ("y", quad.position[1]),
                ("right", quad.position[0] + quad.size[0]),
                ("bottom", quad.position[1] + quad.size[1]),
            ] {
                assert_eq!(v.fract(), 0.0, "第 {i} 块的 {名} = {v} 带小数");
            }
        }

        // Assert 二：左上角的右边界恰等于上边块的左边界（无缝无叠）。
        // 顺序见 `nine_slice_rects`：0 左上角、4 上边。
        let 左上角 = &quads[0];
        let 上边 = &quads[4];
        assert_eq!(
            左上角.position[0] + 左上角.size[0],
            上边.position[0],
            "左上角与上边之间出现了缝或重叠"
        );
        // 左边（6）的下边界恰等于左下角（2）的上边界。
        let 左边 = &quads[6];
        let 左下角 = &quads[2];
        assert_eq!(
            左边.position[1] + 左边.size[1],
            左下角.position[1],
            "左边与左下角之间出现了缝或重叠"
        );
    }

    #[test]
    fn textured_panel_quads恒产出九块() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        let style = TexturedPanelAppearance {
            border_uv: [0.0, 0.0, 0.5, 0.5],
            fill_uv: [0.5, 0.5, 0.5, 0.5],
            border_tint: [1.0, 1.0, 1.0, 1.0],
            fill_tint: [1.0, 1.0, 1.0, 1.0],
            border_thickness: 2.0,
        };

        // Act
        let quads = textured_panel_quads(rect, &style);

        // Assert
        assert_eq!(quads.len(), 9);
    }

    #[test]
    fn textured_panel_quads的中心块采样fill_uv而非border_uv() {
        // Arrange
        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        let style = TexturedPanelAppearance {
            border_uv: [0.0, 0.0, 0.5, 0.5],
            fill_uv: [0.5, 0.5, 0.5, 0.5],
            border_tint: [1.0, 1.0, 1.0, 1.0],
            fill_tint: [1.0, 1.0, 1.0, 1.0],
            border_thickness: 2.0,
        };

        // Act
        let quads = textured_panel_quads(rect, &style);
        let center = quads.last().expect("九块的最后一块是中心填充");

        // Assert
        assert_eq!(center.uv_rect, style.fill_uv);
    }

    #[test]
    fn textured_panel_quads与panel_quads的几何完全一致() {
        // 两条函数共用 `nine_slice_rects`,这条测试直接验证两者产出的
        // 位置/尺寸逐块相同——防止未来有人改了其中一份几何却忘了改
        // 另一份。
        // Arrange
        let rect = Rect::new(10.0, 20.0, 100.0, 60.0);
        let flat_style = FlatPanelAppearance {
            border_thickness: 3.0,
            ..FlatPanelAppearance::DEFAULT
        };
        let textured_style = TexturedPanelAppearance {
            border_uv: [0.0, 0.0, 0.5, 0.5],
            fill_uv: [0.5, 0.5, 0.5, 0.5],
            border_tint: [1.0, 1.0, 1.0, 1.0],
            fill_tint: [1.0, 1.0, 1.0, 1.0],
            border_thickness: 3.0,
        };

        // Act
        let flat = panel_quads(rect, &flat_style);
        let textured = textured_panel_quads(rect, &textured_style);

        // Assert
        for (f, t) in flat.iter().zip(textured.iter()) {
            assert_eq!(f.position, t.position);
            assert_eq!(f.size, t.size);
        }
    }
}
