//! 按钮：普通/悬停/按下/禁用四种状态,鼠标点击与键盘/手柄确认键都能
//! 触发。
//!
//! # 只产出「触发了」这个事实,不直接改世界
//!
//! [`update_button`] 的返回值只回答「这一帧应该长什么样」与「这一帧
//! 有没有被触发」——触发之后要做什么（打开某个面板、提交某个表单）
//! 是调用方的事,调用方把 `triggered` 转成既有的 `Intent`/`Effect`
//! 管线能理解的意图,本函数从不直接修改 `WorldState` 或任何游戏逻辑
//! 状态,与 [ADR 0020](../../../../knowledge/decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)
//! 甲区/乙区的边界纪律、以及 `crate::widget::state` 模块文档「UI 状态
//! 绝不进 `WorldState`」一节完全一致。
//!
//! # 按下与触发：为什么不是「按下就触发」
//!
//! 桌面 UI 的通行手感是「按下时武装（arm），松开时若仍悬停在同一个
//! 控件上才算一次点击；按下后拖出控件范围再松开则视为取消」——这与
//! 「按下就触发」（多数游戏手柄的确认键手感）不同,但更符合鼠标用户
//! 的直觉预期（误按下、移开再松手不该误触发)。[`update_button`] 因此
//! 用 [`super::state::WidgetState::pressed`] 跨帧记住「这次按下是不是
//! 从我这里开始的」,只有「按下时命中我」与「松开时仍命中我」同时成立
//! 才算一次鼠标触发。
//!
//! 键盘/手柄的确认键走完全不同的判定：**没有**「拖出再拖回」这个
//! 物理动作可言,因此确认键采用「本帧激活即触发」（复用
//! `InputState::was_activated`,支持长按连续触发,与方向键在 Gameplay
//! 上下文下驱动移动是同一套自动重复语义),只要求这个按钮当前持有焦点
//! （`WidgetState::focused`,见 [`super::focus`]）。
//!
//! # 焦点也要有可见反馈
//!
//! 只用鼠标操作的界面里,「悬停」与「聚焦」是同一件事在做同一件事;
//! 但纯键盘/手柄操作时,玩家唯一能确认"当前会响应确认键的是哪个按钮"
//! 的办法就是看外观——因此 [`ButtonVisualState::Hovered`] 由「鼠标
//! 悬停」**或**「持有键盘/手柄焦点」两个条件之一触发,不是只认鼠标。
//! 这直接呼应任务书「焦点导航不能只是摆设」。

use super::geometry::Rect;
use super::panel::{
    FlatPanelAppearance, TexturedPanelAppearance, panel_quads, textured_panel_quads,
};
use super::quad::QuadInstance;
use super::skin::ButtonVisualState;
use super::state::{WidgetId, WidgetStateTable};
use super::textured_quad::TexturedQuadInstance;
use ll_platform::input::{GameKey, InputState, MouseButton};

/// 按钮的纯色样式——形状与 [`FlatPanelAppearance`] 完全一致（边框 +
/// 填充 + 边框厚度）,这不是巧合：按钮在几何上就是一块可点击的面板,
/// [`button_quads`] 直接复用 [`super::panel::panel_quads`] 的九宫格
/// 几何,不重新发明一套「画一个方框」的逻辑。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatButtonAppearance {
    /// 边框颜色。
    pub border_color: [f32; 4],
    /// 填充颜色——四种状态靠这个字段互相区分,见
    /// [`super::skin::ButtonVisualState`] 文档。
    pub fill_color: [f32; 4],
    /// 边框厚度（像素）。
    pub border_thickness: f32,
}

impl FlatButtonAppearance {
    /// 普通状态：与 [`FlatPanelAppearance::DEFAULT`] 同一份中性外观,
    /// 按钮在未交互时应当融入其余 HUD 面板,不抢眼。
    pub const NORMAL: FlatButtonAppearance = FlatButtonAppearance {
        border_color: [0.75, 0.75, 0.8, 0.9],
        fill_color: [0.05, 0.05, 0.08, 0.55],
        border_thickness: 2.0,
    };
    /// 悬停/聚焦状态：填充更亮、更不透明——「这个控件现在会响应输入」
    /// 的视觉承诺。
    pub const HOVERED: FlatButtonAppearance = FlatButtonAppearance {
        border_color: [0.85, 0.85, 0.95, 1.0],
        fill_color: [0.2, 0.35, 0.5, 0.75],
        border_thickness: 2.0,
    };
    /// 按下状态：比悬停更暗、边框更亮——常见的「按下去凹陷了」的色彩
    /// 提示（本项目没有真实的立体阴影美术，靠颜色对比代替）。
    pub const PRESSED: FlatButtonAppearance = FlatButtonAppearance {
        border_color: [0.95, 0.95, 1.0, 1.0],
        fill_color: [0.1, 0.2, 0.32, 0.9],
        border_thickness: 2.0,
    };
    /// 禁用状态：整体去饱和、更透明——「不要点我」的视觉提示,与三种
    /// 可交互状态在色相上都不同,不会被误认成任意一种可点击外观。
    pub const DISABLED: FlatButtonAppearance = FlatButtonAppearance {
        border_color: [0.4, 0.4, 0.42, 0.5],
        fill_color: [0.08, 0.08, 0.08, 0.35],
        border_thickness: 2.0,
    };
}

/// 按钮的真实贴图外观——形状与 [`TexturedPanelAppearance`] 一致，理由
/// 同 [`FlatButtonAppearance`]。本批次没有按钮贴图资产（见
/// `super::skin::Skin::textured_button` 文档），这个类型目前没有任何
/// 生产者，只是把「加贴图只需要新增一个 `Skin` 实现」这条既有承诺
/// （见 `super::skin` 模块文档）在按钮上占位。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TexturedButtonAppearance {
    /// 边框贴图 UV。
    pub border_uv: [f32; 4],
    /// 填充贴图 UV。
    pub fill_uv: [f32; 4],
    /// 边框颜色调制。
    pub border_tint: [f32; 4],
    /// 填充颜色调制。
    pub fill_tint: [f32; 4],
    /// 边框厚度（像素）。
    pub border_thickness: f32,
}

/// 把 `rect` 按给定的纯色外观画成一个九宫格按钮——直接委托给
/// [`panel_quads`]，两者的几何完全相同（按钮就是一块可点击的面板，
/// 见 [`FlatButtonAppearance`] 文档），不重复实现九宫格切分。
pub fn button_quads(rect: Rect, appearance: &FlatButtonAppearance) -> Vec<QuadInstance> {
    panel_quads(
        rect,
        &FlatPanelAppearance {
            border_color: appearance.border_color,
            fill_color: appearance.fill_color,
            border_thickness: appearance.border_thickness,
        },
    )
}

/// 贴图版本，理由同 [`button_quads`]。
pub fn textured_button_quads(
    rect: Rect,
    appearance: &TexturedButtonAppearance,
) -> Vec<TexturedQuadInstance> {
    textured_panel_quads(
        rect,
        &TexturedPanelAppearance {
            border_uv: appearance.border_uv,
            fill_uv: appearance.fill_uv,
            border_tint: appearance.border_tint,
            fill_tint: appearance.fill_tint,
            border_thickness: appearance.border_thickness,
        },
    )
}

/// [`update_button`] 这一帧的产出：该用哪种视觉状态画，以及这一帧是否
/// 被触发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonOutcome {
    /// 这一帧应该用哪种外观画——皮肤据此解析出真正的颜色（见
    /// [`super::skin::Skin::button`]）。
    pub visual: ButtonVisualState,
    /// 这一帧是否被触发（鼠标点击完成，或键盘/手柄确认键激活）——见
    /// 模块文档「按下与触发」一节。调用方应当把这个布尔值转译成一个
    /// `Intent`/自定义 UI 事件，不在这里直接分支处理具体业务逻辑。
    pub triggered: bool,
}

/// 每帧调用一次：读一次 `input`，把 `id` 对应的悬停/按下/聚焦状态写回
/// `table`，返回这一帧该用哪种外观、是否被触发。
///
/// `rect` 应当是**这一帧**通过 [`super::hit_test::hit_test`] 确认过、
/// 排在最上层的那个矩形——本函数自己不做「我是不是被其它控件挡住了」
/// 这类判断，只回答「光标是否落在 `rect` 内」；调用方若先用
/// `hit_test` 筛过一遍再调用本函数，两者结果天然一致（同一份 `Rect`，
/// 同一次 `contains` 判定）。
///
/// `enabled` 为假时恒返回 [`ButtonVisualState::Disabled`]，且
/// `triggered` 恒为假——禁用态不响应鼠标也不响应键盘，见模块文档
/// 「按下与触发」一节。
pub fn update_button(
    table: &mut WidgetStateTable,
    id: WidgetId,
    rect: Rect,
    input: &InputState,
    enabled: bool,
) -> ButtonOutcome {
    let hovered = enabled
        && input
            .cursor_position()
            .is_some_and(|position| rect.contains(position));
    let focused = table.get(id).is_some_and(|state| state.focused);

    let previously_armed = table.get(id).is_some_and(|state| state.pressed);
    let mut armed = previously_armed;
    if enabled && hovered && input.was_mouse_just_pressed(MouseButton::Left) {
        armed = true;
    }
    let just_released = input.was_mouse_just_released(MouseButton::Left);
    if just_released {
        // 松开这一刻永远解除武装状态,不论松开时是否仍悬停在本控件上
        // ——见模块文档「按下与触发」一节。
        armed = false;
    }
    let mouse_triggered = enabled && just_released && previously_armed && hovered;
    let keyboard_triggered = enabled && focused && input.was_activated(GameKey::Confirm);

    let state = table.entry(id);
    state.hovered = hovered;
    state.pressed = armed;

    let visual = if !enabled {
        ButtonVisualState::Disabled
    } else if armed && hovered {
        ButtonVisualState::Pressed
    } else if hovered || focused {
        ButtonVisualState::Hovered
    } else {
        ButtonVisualState::Normal
    };

    ButtonOutcome {
        visual,
        triggered: mouse_triggered || keyboard_triggered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rect() -> Rect {
        Rect::new(10.0, 10.0, 100.0, 40.0)
    }

    #[test]
    fn button_quads恒产出九块() {
        // Arrange & Act
        let quads = button_quads(sample_rect(), &FlatButtonAppearance::NORMAL);

        // Assert
        assert_eq!(quads.len(), 9);
    }

    #[test]
    fn 光标未落在按钮上时视觉状态为普通() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.set_cursor_position((500.0, 500.0));

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert_eq!(outcome.visual, ButtonVisualState::Normal);
    }

    #[test]
    fn 光标悬停在按钮上时视觉状态为悬停() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.set_cursor_position((50.0, 20.0));

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert_eq!(outcome.visual, ButtonVisualState::Hovered);
    }

    #[test]
    fn 禁用按钮恒为禁用视觉状态() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.set_cursor_position((50.0, 20.0));

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, false);

        // Assert
        assert_eq!(outcome.visual, ButtonVisualState::Disabled);
    }

    #[test]
    fn 禁用按钮不响应鼠标点击() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.set_cursor_position((50.0, 20.0));
        input.mouse_press(MouseButton::Left);
        update_button(&mut table, "test.button", sample_rect(), &input, false);
        input.end_frame();
        input.mouse_release(MouseButton::Left);

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, false);

        // Assert
        assert!(!outcome.triggered);
    }

    #[test]
    fn 按下时悬停按钮视觉状态为按下() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.set_cursor_position((50.0, 20.0));
        input.mouse_press(MouseButton::Left);

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert_eq!(outcome.visual, ButtonVisualState::Pressed);
    }

    #[test]
    fn 按下且在同一控件上松开时触发() {
        // 完整的点击序列：按下命中控件,松开时仍悬停在同一个控件上。
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.set_cursor_position((50.0, 20.0));
        input.mouse_press(MouseButton::Left);
        update_button(&mut table, "test.button", sample_rect(), &input, true);
        input.end_frame();
        input.mouse_release(MouseButton::Left);

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert!(outcome.triggered);
    }

    #[test]
    fn 按下后拖出控件范围再松开不触发() {
        // 模拟误按下、移开再松手——不该误触发,见模块文档「按下与
        // 触发」一节。
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.set_cursor_position((50.0, 20.0));
        input.mouse_press(MouseButton::Left);
        update_button(&mut table, "test.button", sample_rect(), &input, true);
        input.end_frame();

        input.set_cursor_position((500.0, 500.0)); // 拖出控件范围
        input.mouse_release(MouseButton::Left);

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert!(!outcome.triggered);
    }

    #[test]
    fn 未按下直接松开不触发() {
        // 防止"只要松开就触发"这种错误捷径——必须先有一次命中本控件
        // 的按下。
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.set_cursor_position((50.0, 20.0));

        // Act：从未调用过 mouse_press。
        input.mouse_release(MouseButton::Left);
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert!(!outcome.triggered);
    }

    #[test]
    fn 聚焦的按钮按下确认键时触发() {
        // Arrange：不涉及任何光标位置/鼠标事件,证明纯键盘路径独立
        // 生效。
        let mut table = WidgetStateTable::new();
        table.entry("test.button").focused = true;
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert!(outcome.triggered);
    }

    #[test]
    fn 未聚焦的按钮按下确认键不触发() {
        // Arrange
        let mut table = WidgetStateTable::new();
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert!(!outcome.triggered);
    }

    #[test]
    fn 聚焦但未悬停的按钮视觉状态仍是悬停() {
        // 纯键盘操作时必须有可见反馈,见模块文档「焦点也要有可见反馈」
        // 一节。
        // Arrange
        let mut table = WidgetStateTable::new();
        table.entry("test.button").focused = true;
        let input = InputState::new(); // 没有光标位置

        // Act
        let outcome = update_button(&mut table, "test.button", sample_rect(), &input, true);

        // Assert
        assert_eq!(outcome.visual, ButtonVisualState::Hovered);
    }
}
