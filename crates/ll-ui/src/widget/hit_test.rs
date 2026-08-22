//! 命中测试：这一帧光标下方是哪个控件。
//!
//! # 即时模式下怎么做——不依赖任何跨帧记忆
//!
//! `crate::widget` 模块文档「将来加焦点/选中」一节已经定过调：不需要
//! 给每个控件发一个持久 ID 再维护一棵「上一帧的控件树」去和「这一帧
//! 的控件树」做差分——「这一帧点的是哪一个」直接由「这一帧算出的第几
//! 个 [`Rect`] 包含点击坐标」现算得出。[`hit_test`] 正是这句话的实现：
//! 调用方每帧重新声明这一帧全部可交互控件的 `(WidgetId, Rect)`，本函数
//! 只是一次线性扫描，不持有、也不需要持有任何状态。
//!
//! # 后来居上：与渲染顺序保持一致
//!
//! `crate::hud::render` 模块文档已经确立「同一份 draw call 按实例顺序
//! 绘制，后追加的实例画在更上层」——世界地图浮层就是靠这条规则叠加在
//! 四块常驻面板之上（见其 `world_map` 参数文档）。命中测试若不遵守
//! 同一顺序，会出现「点到了看起来在上层、实际却先一步被判定命中的
//! 下层控件」这种反直觉行为：玩家看见的最上层是浮层，点击却命中了
//! 浮层下面被遮住的按钮。[`hit_test`] 因此按调用方给出的顺序保留
//! **最后一个**匹配的控件，不是第一个，与绘制顺序（后追加者居上）
//! 完全对齐——调用方只需要按绘制顺序传入 `widgets`，不需要额外反转。

use super::geometry::Rect;
use super::state::WidgetId;

/// 在 `widgets`（按绘制顺序排列，越靠后越靠上）中找出包含 `point` 的
/// 最上层控件的 id；没有任何一个包含则返回 `None`。
///
/// `point` 与 `widgets` 里的 `Rect` 必须是同一套坐标系——本项目里恒为
/// 窗口原生像素坐标（见 [`Rect`] 模块文档「坐标系」一节），调用方通常
/// 直接传 `ll_platform::input::InputState::cursor_position` 的返回值
/// （本 crate 不直接依赖那个具体类型，只要求调用方给出同一套坐标系的
/// `(f32, f32)`，保持这个函数本身足够通用、可脱离平台层单元测试）。
pub fn hit_test(
    point: (f32, f32),
    widgets: impl IntoIterator<Item = (WidgetId, Rect)>,
) -> Option<WidgetId> {
    widgets
        .into_iter()
        .filter(|(_, rect)| rect.contains(point))
        .map(|(id, _)| id)
        .last()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 点落在唯一控件内时命中该控件() {
        // Arrange
        let widgets = [("widget.a", Rect::new(0.0, 0.0, 100.0, 100.0))];

        // Act
        let hit = hit_test((10.0, 10.0), widgets);

        // Assert
        assert_eq!(hit, Some("widget.a"));
    }

    #[test]
    fn 点落在全部控件之外时不命中任何控件() {
        // Arrange
        let widgets = [("widget.a", Rect::new(0.0, 0.0, 10.0, 10.0))];

        // Act
        let hit = hit_test((500.0, 500.0), widgets);

        // Assert
        assert_eq!(hit, None);
    }

    #[test]
    fn 空控件列表恒不命中() {
        // Arrange
        let widgets: [(&str, Rect); 0] = [];

        // Act
        let hit = hit_test((0.0, 0.0), widgets);

        // Assert
        assert_eq!(hit, None);
    }

    #[test]
    fn 两个重叠控件命中后绘制的那一个() {
        // 模拟世界地图浮层盖在常驻面板之上——两者恰好在同一片区域
        // 重叠，命中测试应该判给后追加（视觉上在上层）的那一个。
        // Arrange
        let widgets = [
            ("panel.background", Rect::new(0.0, 0.0, 200.0, 200.0)),
            ("panel.overlay", Rect::new(50.0, 50.0, 100.0, 100.0)),
        ];

        // Act
        let hit = hit_test((75.0, 75.0), widgets);

        // Assert
        assert_eq!(hit, Some("panel.overlay"));
    }

    #[test]
    fn 点只落在下层控件范围内时命中下层控件() {
        // 上一条测试证明"重叠区域命中上层"，这条测试补上"非重叠区域
        // 仍然命中下层"——防止实现退化成"只要列表非空就恒返回最后
        // 一个"这种错误捷径。
        // Arrange
        let widgets = [
            ("panel.background", Rect::new(0.0, 0.0, 200.0, 200.0)),
            ("panel.overlay", Rect::new(50.0, 50.0, 100.0, 100.0)),
        ];

        // Act
        let hit = hit_test((10.0, 10.0), widgets);

        // Assert
        assert_eq!(hit, Some("panel.background"));
    }
}
