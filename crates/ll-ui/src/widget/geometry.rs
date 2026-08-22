//! 布局地基：一个矩形类型 + 「往下堆叠一块」这一件事。
//!
//! # 只做「能定位、能堆叠」，不做约束求解器
//!
//! 任务书要求的布局能力是「至少能定位与堆叠」——本模块只提供
//! [`Rect`] 本身与 [`Rect::stack_below`]/[`Rect::stack_right`] 两个纯
//! 几何操作，不引入 flexbox/grid 一类的约束求解。这不是偷懒省事：
//! 本批次四块面板（状态栏/角色/背包/装备）全部是固定分区平铺，从
//! 未出现过需要「父容器剩余空间按比例分配给子项」这类问题，提前建一
//! 个约束系统没有真实需求驱动（ADR 0021 同一条判断：抽象要有真实可
//! 共享的算法支撑）。真正需要更复杂布局的那一天，`Rect` 本身不需要
//! 改——新的布局算法可以是消费 `Rect` 的新函数，不必推翻这个类型。

/// 一个像素矩形：左上角坐标 + 宽高。所有字段都是原生分辨率像素——与
/// [`crate::widget::quad::QuadInstance`]/[`ll_text::TextRun`] 同一套
/// 坐标系（见 `ll_text` crate 顶层文档「两条渲染通道」一节：HUD 文本
/// 与本类型都工作在窗口原生像素空间，不是 640×360 逻辑分辨率）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// 左上角 x 坐标。
    pub x: f32,
    /// 左上角 y 坐标。
    pub y: f32,
    /// 宽度。
    pub width: f32,
    /// 高度。
    pub height: f32,
}

impl Rect {
    /// 新建一个矩形。
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// 左上角坐标——喂给 [`super::list::RowCursor::new`]/
    /// [`super::panel::panel_quads`] 一类接收 `(f32, f32)` 起点的接口。
    pub fn origin(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// 右边界坐标。
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// 下边界坐标。
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// 紧贴在 `self` 下方、间隔 `gap` 像素、宽度与 `self` 相同、高度为
    /// `height` 的新矩形——四块面板从上到下堆叠（状态栏在最上、其余
    /// 面板各自纵向堆叠自己的行）都用这一个操作。
    pub fn stack_below(&self, gap: f32, height: f32) -> Rect {
        Rect::new(self.x, self.bottom() + gap, self.width, height)
    }

    /// 紧贴在 `self` 右侧、间隔 `gap` 像素、高度与 `self` 相同、宽度为
    /// `width` 的新矩形——角色/背包/装备三块面板并排成三列时用它。
    pub fn stack_right(&self, gap: f32, width: f32) -> Rect {
        Rect::new(self.right() + gap, self.y, width, self.height)
    }

    /// 向内收缩 `inset` 像素得到的新矩形——面板内容相对面板背景边框
    /// 的内边距,由 [`crate::widget::panel::FlatPanelAppearance`] 消费。
    pub fn inset(&self, inset: f32) -> Rect {
        Rect::new(
            self.x + inset,
            self.y + inset,
            (self.width - inset * 2.0).max(0.0),
            (self.height - inset * 2.0).max(0.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_below产出的矩形紧贴原矩形下边界加间隔() {
        // Arrange
        let above = Rect::new(10.0, 20.0, 100.0, 30.0);

        // Act
        let below = above.stack_below(5.0, 40.0);

        // Assert
        assert_eq!(below, Rect::new(10.0, 55.0, 100.0, 40.0));
    }

    #[test]
    fn stack_right产出的矩形紧贴原矩形右边界加间隔() {
        // Arrange
        let left = Rect::new(10.0, 20.0, 100.0, 30.0);

        // Act
        let right = left.stack_right(5.0, 60.0);

        // Assert
        assert_eq!(right, Rect::new(115.0, 20.0, 60.0, 30.0));
    }

    #[test]
    fn inset向内收缩两侧各减去等量像素() {
        // Arrange
        let outer = Rect::new(0.0, 0.0, 100.0, 50.0);

        // Act
        let inner = outer.inset(10.0);

        // Assert
        assert_eq!(inner, Rect::new(10.0, 10.0, 80.0, 30.0));
    }

    #[test]
    fn inset收缩量超过尺寸一半时宽高钳制到零而非负数() {
        // Arrange：内边距 30，但宽只有 40（两侧各减 30 会变成 -20）。
        let outer = Rect::new(0.0, 0.0, 40.0, 100.0);

        // Act
        let inner = outer.inset(30.0);

        // Assert
        assert_eq!(inner.width, 0.0);
    }
}
