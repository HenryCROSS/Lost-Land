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

    /// 把四条边界各取整到最近的整数像素——**像素画唯一真正需要的那一次
    /// 取整**（规格 L0，`knowledge/design/ui-and-navigation.md` §6.1）。
    ///
    /// # 为什么取整的是「边界」，不是「原点 + 尺寸」
    ///
    /// 这是本方法全部的设计内容，也是「相邻两块之间不留缝、也不重叠」
    /// 这条性质**唯一**的来源：
    ///
    /// 两块相邻矩形共享的那条边（左边那块的 `right()` 与右边那块的 `x`）
    /// 是**同一个 `f32` 值**。`round()` 是函数，同一个输入必然给出同一个
    /// 输出，于是两块取整之后仍然共享同一条边——缝与叠都不可能出现。
    ///
    /// 换成「分别取整 `x` 与 `width`」就不成立了：`x = 0.6`、`width = 1.8`
    /// 时，`round(0.6) + round(1.8) = 1 + 2 = 3`，而右边那块的
    /// `round(0.6 + 1.8) = round(2.4) = 2`——两块之间叠了一像素。
    ///
    /// # 为什么要有这件事
    ///
    /// ADR 0002 的范围是**世界状态**，明文允许渲染层用浮点，本层的 `f32`
    /// 不是违规（见 `crate::widget::geometry` 与
    /// `knowledge/design/animation-and-vfx-boundary.md`）。像素画糊掉的
    /// 原因不是浮点本身，是**半像素边界**：一条落在 `x = 10.5` 的边会被
    /// 光栅化成两列各半亮的像素。取整只需要发生在**提交那一刻**，中间的
    /// 布局计算照旧用 `f32`。
    ///
    /// 取整是**幂等**的（对已经是整数的矩形调它得到自己），因此在积木内部
    /// 与帧出口各调一次不会互相打架，见
    /// [`crate::widget::layer::LayeredFrame::snap_to_pixels`]。
    pub fn snap(&self) -> Rect {
        let x = self.x.round();
        let y = self.y.round();
        Rect::new(x, y, self.right().round() - x, self.bottom().round() - y)
    }

    /// `point` 是否落在这个矩形内——命中测试（[`crate::widget::hit_test`]）
    /// 与按钮悬停判定（[`crate::widget::button`]）共用的唯一几何判据。
    ///
    /// 左闭右开、上闭下开（`x` 落在 `[self.x, self.right())`，`y` 同理）：
    /// 两个左右相邻、边界重合的控件（例如面板九宫格切出来的两块）不会
    /// 因为「恰好落在公共边界上」而被同时判定命中——命中测试因此天然
    /// 保证「同一个点最多算落在其中一块的范围内」，不需要调用方自己
    /// 再处理边界重叠。
    pub fn contains(&self, point: (f32, f32)) -> bool {
        let (x, y) = point;
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
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

    #[test]
    fn contains对矩形内部的点返回真() {
        // Arrange
        let rect = Rect::new(10.0, 10.0, 100.0, 50.0);

        // Act & Assert
        assert!(rect.contains((50.0, 30.0)));
    }

    #[test]
    fn contains对矩形外部的点返回假() {
        // Arrange
        let rect = Rect::new(10.0, 10.0, 100.0, 50.0);

        // Act & Assert
        assert!(!rect.contains((200.0, 200.0)));
    }

    #[test]
    fn contains对左上角边界点返回真() {
        // Arrange：左闭右开、上闭下开——左上角属于闭区间一侧。
        let rect = Rect::new(10.0, 10.0, 100.0, 50.0);

        // Act & Assert
        assert!(rect.contains((10.0, 10.0)));
    }

    #[test]
    fn snap把四条边界都取整到整数像素() {
        // 规格 L0。**先自证输入真的带半像素**——否则「取整后是整数」
        // 对一个本来就是整数的输入恒绿（本会话点名的假绿形状之二：
        // 被断言的对象根本不存在）。
        // Arrange
        let rect = Rect::new(10.4, 20.6, 100.3, 60.7);
        assert!(
            rect.x.fract() != 0.0 && rect.right().fract() != 0.0,
            "测试输入必须真的带半像素，否则这条断言恒绿"
        );

        // Act
        let snapped = rect.snap();

        // Assert
        for v in [snapped.x, snapped.y, snapped.right(), snapped.bottom()] {
            assert_eq!(v.fract(), 0.0, "取整后 {v} 仍带小数");
        }
    }

    #[test]
    fn snap取整的是边界因此相邻两块既不留缝也不重叠() {
        // 这是 `snap` 全部的设计内容，见其文档那一段推导。
        //
        // 反例验证（已实跑）：把 `snap` 改成分别取整 `x` 与 `width`
        // （`Rect::new(x.round(), y.round(), width.round(), height.round())`），
        // 本条当场红——左块右边界 3、右块左边界 2，叠了一像素。
        // Arrange：左块的右边界与右块的左边界是**同一个** f32。
        let 边界 = 2.4_f32;
        let 左 = Rect::new(0.6, 0.0, 边界 - 0.6, 10.0);
        let 右 = Rect::new(边界, 0.0, 5.0, 10.0);
        assert_eq!(左.right(), 右.x, "两块必须真的共享同一条边");

        // Act
        let (左, 右) = (左.snap(), 右.snap());

        // Assert
        assert_eq!(
            左.right(),
            右.x,
            "取整后两块之间出现了缝或重叠：左块右边界 {}，右块左边界 {}",
            左.right(),
            右.x
        );
    }

    #[test]
    fn snap是幂等的() {
        // 积木内部取一次、帧出口再取一次，两次不能互相打架，见
        // `crate::widget::layer::LayeredFrame::snap_to_pixels`。
        // Arrange
        let rect = Rect::new(10.4, 20.6, 100.3, 60.7);

        // Act
        let 一次 = rect.snap();
        let 两次 = 一次.snap();

        // Assert
        assert_eq!(一次, 两次);
    }

    #[test]
    fn contains对右下角边界点返回假() {
        // Arrange：右下角属于开区间一侧，恰好落在边界上不算命中——
        // 这保证两个边界重合的相邻矩形不会同时判定命中同一个点。
        let rect = Rect::new(10.0, 10.0, 100.0, 50.0);

        // Act & Assert
        assert!(!rect.contains((rect.right(), rect.bottom())));
    }
}
