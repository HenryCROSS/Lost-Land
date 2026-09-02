//! 行高亮：「这一行现在被选中 / 指针正悬在它上面」的**唯一**视觉表达。
//!
//! # 为什么从 `crate::screen` 搬到这里（规格 F7）
//!
//! 高亮矩形此前只有模态屏在用，因此两个颜色常量与那段皮肤分支就住在
//! `crate::screen` 里。规格 F7 要求 HUD 的动作菜单也把光标从**文字前缀**
//! 改成高亮矩形（见 [`crate::hud::action_menu`] 模块文档），于是同一段
//! 逻辑出现了第二个消费者。
//!
//! `hud` 与 `screen` 彼此不依赖、都依赖 `widget`——这与批次 30 把间距刻度
//! 搬进 [`super::metrics`] 是同一条理由：**两边都看得见的地方只有这里**。
//! 搬家之后实现仍然只有一份，`crate::screen` 的两个常量改成 `pub use`
//! 重导出，公开路径一条都没断。
//!
//! # 为什么高亮要跟着面板走同一个皮肤分支
//!
//! 纯色与贴图是**两道 pass**，同一层里纯色永远被贴图盖住（见
//! [`super::layer`] 模块文档）。高亮若一律走纯色，那么装了窗口贴图的
//! 皮肤下它会被面板整块盖掉——玩家就再也看不到自己选中了哪一行。因此
//! [`push_row_highlight`] 照抄面板自己那个 `match`：面板走贴图，高亮也走
//! 贴图（拿面板的填充 UV，用高亮色当调制），面板走纯色，高亮也走纯色。

use super::geometry::Rect;
use super::quad::QuadInstance;
use super::skin::{PanelStyleId, Skin};
use super::textured_quad::TexturedQuadInstance;

/// 聚焦行的高亮矩形颜色（RGBA）——**「这一行现在会响应确认键」的视觉
/// 承诺**，与 [`super::button::FlatButtonAppearance::HOVERED`] 的填充
/// 同一份色，几处读起来要像同一个产品。
pub const FOCUS_HIGHLIGHT_COLOR: [f32; 4] = [0.2, 0.35, 0.5, 0.75];

/// 悬停行的高亮矩形颜色（RGBA）——比聚焦淡一档：指针划过去**不改变
/// 焦点**（见 `ll_game::pointer` 模块文档那四条约定），它只是在说
/// 「点下去会是这一行」。
pub const HOVER_HIGHLIGHT_COLOR: [f32; 4] = [0.2, 0.35, 0.5, 0.32];

/// 把某一行的矩形画成一块高亮底——纯色那一条路径。
pub fn row_highlight_quad(rect: Rect, color: [f32; 4]) -> QuadInstance {
    QuadInstance {
        position: [rect.x, rect.y],
        size: [rect.width, rect.height],
        color,
    }
}

/// 推入一块行高亮，皮肤分支见模块文档最后一节。
///
/// 调用方负责决定**哪一行**要高亮（模态屏是聚焦行与悬停行两块，HUD
/// 动作菜单只有光标那一块）；本函数只管把一个已经算好的矩形按当前皮肤
/// 推进正确的那一批里。
pub fn push_row_highlight(
    rect: Rect,
    color: [f32; 4],
    skin: &dyn Skin,
    quads: &mut Vec<QuadInstance>,
    textured_quads: &mut Vec<TexturedQuadInstance>,
) {
    match skin.textured_panel(PanelStyleId::Window) {
        Some(appearance) => textured_quads.push(TexturedQuadInstance {
            position: [rect.x, rect.y],
            size: [rect.width, rect.height],
            uv_rect: appearance.fill_uv,
            color,
        }),
        None => quads.push(row_highlight_quad(rect, color)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::skin::FlatColorSkin;

    #[test]
    fn 纯色皮肤下高亮走纯色那一批() {
        // Arrange
        let skin = FlatColorSkin;
        let mut quads = Vec::new();
        let mut textured = Vec::new();

        // Act
        push_row_highlight(
            Rect::new(3.0, 5.0, 100.0, 18.0),
            FOCUS_HIGHLIGHT_COLOR,
            &skin,
            &mut quads,
            &mut textured,
        );

        // Assert
        assert!(textured.is_empty(), "纯色皮肤不该产出贴图矩形");
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].position, [3.0, 5.0]);
        assert_eq!(quads[0].size, [100.0, 18.0]);
        assert_eq!(quads[0].color, FOCUS_HIGHLIGHT_COLOR);
    }

    #[test]
    fn 聚焦与悬停两块高亮同形不同透明度() {
        // 两者的区别只该是那一档透明度——它承载的是「焦点在这儿」与
        // 「点下去会是这儿」两句不同的话，混成同一档就没人分得清。
        //
        // 走 `push_row_highlight` 而不是直接比两个常量：直接比是对常量
        // 的断言（clippy 也拦），而且量的不是任何生产路径。
        // Arrange
        let skin = FlatColorSkin;
        let rect = Rect::new(0.0, 0.0, 10.0, 18.0);
        let mut quads = Vec::new();
        let mut textured = Vec::new();

        // Act
        push_row_highlight(
            rect,
            FOCUS_HIGHLIGHT_COLOR,
            &skin,
            &mut quads,
            &mut textured,
        );
        push_row_highlight(
            rect,
            HOVER_HIGHLIGHT_COLOR,
            &skin,
            &mut quads,
            &mut textured,
        );

        // Assert
        assert_eq!(quads.len(), 2, "两次推入就该有两块");
        assert_eq!(quads[0].size, quads[1].size, "同一行的两种高亮同形");
        assert_eq!(
            quads[0].color[..3],
            quads[1].color[..3],
            "两者只该差在透明度上"
        );
        assert!(
            quads[0].color[3] > quads[1].color[3],
            "聚焦 {} 应当比悬停 {} 更不透明",
            quads[0].color[3],
            quads[1].color[3]
        );
    }
}
