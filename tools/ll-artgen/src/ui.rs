//! HUD 控件的临时美术：面板九宫格边框/填充、条形底/填充。
//!
//! # 复用地形点缀算法，不另起一套风格
//!
//! 项目所有者明确要求「风格要和现有美术一套……不要另起一套风格」——
//! 本模块因此不新写一套绘制逻辑，而是直接复用 [`crate::terrain::TerrainSpec`]/
//! [`crate::terrain::decorate_terrain_tile`]：同一份「主色、邻近色点缀、
//! 互补色点缀」配方，只是换一套目标名字与颜色，构造出的贴图与地形
//! 瓦片是同一种视觉语言（稀疏色块点缀，约 5% 像素偏离主色）。
//!
//! # 只用两张贴图撑起完整的九宫格
//!
//! `ui_panel_border` 同时充当四个角与四条边——四个角原样绘制（不
//! 拉伸），四条边由渲染层（`ll_ui::widget::panel` 未来的贴图版本）沿
//! 一个方向拉伸,中心用 `ui_panel_fill` 双向拉伸。真实的九宫格贴图
//! （角有花纹、边不能简单复用）到位前，边框本身就是纯色 + 点缀，四个
//! 角与两条边共用同一张源图不会有任何视觉不一致——这正是
//! `crates/ll-ui/src/widget/panel.rs` 模块文档「为什么不是四条边+一个
//! 填充」一节讨论的「贴图到位后可能需要拆开」的具体验证：本批次的
//! 占位贴图恰好不需要拆。
//!
//! 条形同理：`ui_bar_track` 是未填充部分的底色，`ui_bar_fill` 是
//! 已填充部分,两者都不含内部九宫格切分,整张贴图按条形当前尺寸直接
//! 拉伸即可（条形没有「角」的概念）。

use crate::terrain::TerrainSpec;

/// 全部 UI 贴图的配方——复用 [`TerrainSpec`] 的形状（见模块文档
/// 「复用地形点缀算法」一节），颜色与
/// `crates/ll-ui/src/widget/panel.rs`/`bar.rs` 里
/// `FlatPanelAppearance::DEFAULT`/`FlatBarAppearance::DEFAULT` 的既有
/// 纯色选择保持同一套视觉方向（浅灰蓝边框、深蓝黑填充、深灰底、亮蓝
/// 青填充），换了真实贴图后玩家看到的色调不会突变。
const UI_SPECS: &[TerrainSpec] = &[
    TerrainSpec {
        name: "ui_panel_border",
        // 浅灰蓝——呼应 `FlatPanelAppearance::DEFAULT.border_color`
        // 约 `(191, 191, 204)`。
        base: (190, 195, 208),
        accent_lightness_delta: -0.30,
        accent_saturation_boost: 0.35,
    },
    TerrainSpec {
        name: "ui_panel_fill",
        // 深蓝黑——呼应 `FlatPanelAppearance::DEFAULT.fill_color`
        // 约 `(13, 13, 20)`。
        base: (16, 18, 26),
        accent_lightness_delta: 0.35,
        accent_saturation_boost: 0.3,
    },
    TerrainSpec {
        name: "ui_bar_track",
        // 深灰——呼应 `FlatBarAppearance::DEFAULT.background_color`
        // 约 `(51, 51, 56)`。
        base: (48, 50, 56),
        accent_lightness_delta: 0.25,
        accent_saturation_boost: 0.2,
    },
    TerrainSpec {
        name: "ui_bar_fill",
        // 亮蓝青——呼应 `FlatBarAppearance::DEFAULT.fill_color`
        // 约 `(102, 191, 242)`。
        base: (96, 190, 240),
        accent_lightness_delta: -0.25,
        accent_saturation_boost: 0.0,
    },
];

/// 按条目名查 UI 贴图配方；查不到返回 `None`，与
/// [`crate::terrain::terrain_spec`] 同一条约定。
pub(crate) fn ui_spec(name: &str) -> Option<&'static TerrainSpec> {
    UI_SPECS.iter().find(|spec| spec.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 四个ui贴图名都能查到配方() {
        // Arrange
        let names = [
            "ui_panel_border",
            "ui_panel_fill",
            "ui_bar_track",
            "ui_bar_fill",
        ];

        // Act & Assert
        for name in names {
            assert!(ui_spec(name).is_some(), "缺少配方：{name}");
        }
    }

    #[test]
    fn 未知ui贴图名查不到配方() {
        // Arrange & Act & Assert
        assert!(ui_spec("ui_nonexistent").is_none());
    }
}
