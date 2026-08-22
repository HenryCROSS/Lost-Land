//! 列表：把「逐行往下堆叠文字」这件事从四个面板模块各自的临时游标
//! 变量收口成一个正式控件。
//!
//! # 即时模式：每帧重新构造,不持有跨帧状态
//!
//! `RowCursor` 不是一棵常驻的行控件树——每次调用 HUD 面板的行生成
//! 函数都会现场 `RowCursor::new` 一个新的,用完即弃（下一帧再重新
//! 构造一个）。这与 `ll-render::batch::SpriteBatch`/
//! `ll_text::TextRenderer::render` 的既有即时模式完全一致：本项目的
//! 整条渲染管线每帧全量重新声明要画的内容,`RowCursor` 只是这套模式
//! 在「按行布局」这个子问题上的具体应用,不是引入了一种新的状态管理
//! 范式。
//!
//! # 背包/装备栏用它——物品列表、装备槽位列表
//!
//! 任务书点名「列表：背包物品、装备槽位」——[`crate::hud::inventory_panel`]/
//! [`crate::hud::equipment_panel`] 都用 `RowCursor` 逐行推进,不再各自
//! 手写 `cursor_y += line_height`。

use super::label::Label;

/// 一个纵向排布的行游标：从 `origin` 开始,每次 [`RowCursor::push`]
/// 产出一个新 [`Label`] 并把内部纵坐标下移 `row_height`。
pub struct RowCursor {
    x: f32,
    cursor_y: f32,
    row_height: f32,
}

impl RowCursor {
    /// 从 `origin`（左上角）开始,行高 `row_height` 像素。
    pub fn new(origin: (f32, f32), row_height: f32) -> RowCursor {
        RowCursor {
            x: origin.0,
            cursor_y: origin.1,
            row_height,
        }
    }

    /// 追加一行文字到 `labels`,并把内部游标下移一行——四个 HUD 面板
    /// 模块的「产出全部文本行」函数都是反复调用这一个方法拼出来的。
    pub fn push(&mut self, labels: &mut Vec<Label>, text: String) {
        labels.push(Label {
            text,
            x: self.x,
            y: self.cursor_y,
        });
        self.cursor_y += self.row_height;
    }

    /// 当前已经推进到的纵坐标——供调用方在写完全部行之后测量面板
    /// 实际占用的高度（例如拿去建 [`crate::widget::panel::panel_quads`]
    /// 的背景矩形高度）。
    pub fn cursor_y(&self) -> f32 {
        self.cursor_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push产出的每一行y坐标按行高递增() {
        // Arrange
        let mut cursor = RowCursor::new((10.0, 20.0), 16.0);
        let mut labels = Vec::new();

        // Act
        cursor.push(&mut labels, "row-a".to_string());
        cursor.push(&mut labels, "row-b".to_string());

        // Assert
        assert_eq!(labels[1].y - labels[0].y, 16.0);
    }

    #[test]
    fn push产出的每一行x坐标恒等于起点x() {
        // Arrange
        let mut cursor = RowCursor::new((10.0, 20.0), 16.0);
        let mut labels = Vec::new();

        // Act
        cursor.push(&mut labels, "row-a".to_string());
        cursor.push(&mut labels, "row-b".to_string());

        // Assert
        assert_eq!(labels[0].x, 10.0);
        assert_eq!(labels[1].x, 10.0);
    }

    #[test]
    fn cursor_y反映已经推进的行数() {
        // Arrange
        let mut cursor = RowCursor::new((0.0, 0.0), 16.0);
        let mut labels = Vec::new();

        // Act
        cursor.push(&mut labels, "row-a".to_string());
        cursor.push(&mut labels, "row-b".to_string());
        cursor.push(&mut labels, "row-c".to_string());

        // Assert
        assert_eq!(cursor.cursor_y(), 48.0);
    }
}
