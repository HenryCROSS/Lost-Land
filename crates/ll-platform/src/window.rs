//! 窗口与事件循环。
//!
//! # 为什么渲染不单开线程
//!
//! winit 与 wgpu 在部分平台要求窗口创建与渲染提交处于同一线程，强行
//! 分离会在某些合成器上直接失败。真正的并行度来自 [`crate::jobs::JobPool`]
//! 承担的重计算，而不是把渲染搬走。
//!
//! # 整数缩放
//!
//! 逻辑分辨率固定 640×360，窗口尺寸恒为其整数倍。非整数倍缩放会让像素
//! 边缘出现宽窄不一的锯齿，是像素美术最刺眼的瑕疵。

use crate::PlatformError;
use crate::input::{GameKey, InputState};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// 窗口配置。
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// 逻辑宽度，规格规定为 640。
    pub logical_width: u32,
    /// 逻辑高度，规格规定为 360。
    pub logical_height: u32,
    /// 整数缩放倍率。
    pub scale: u32,
    /// 窗口标题的本地化键。
    ///
    /// 存键而非字面量，因为标题是用户可见字符串，必须走 i18n。
    pub title_key: &'static str,
}

impl Default for WindowConfig {
    fn default() -> Self {
        WindowConfig {
            logical_width: 640,
            logical_height: 360,
            // 默认 2 倍得到 1280×720，在绝大多数显示器上都能完整显示。
            scale: 2,
            title_key: "window.title",
        }
    }
}

/// 上层需要实现的帧回调。
///
/// 平台层只负责把事件归约成输入状态并按帧驱动，不含任何游戏逻辑。
pub trait AppHandler {
    /// 每帧调用一次，`input` 是本帧归约后的输入状态。
    fn on_frame(&mut self, input: &InputState);

    /// 窗口关闭前调用，用于保存与清理。
    fn on_exit(&mut self);
}

/// 把物理按键映射为游戏动作。
///
/// 同时支持方向键与 WASD：传统 Roguelike 玩家的手部姿势偏好差异很大，
/// 强制只用方向键会劝退相当一部分人。完整的按键重绑定在 P6 交付，此处
/// 先给出可用的默认布局。
pub fn map_physical_key(key: KeyCode) -> Option<GameKey> {
    let action = match key {
        KeyCode::ArrowUp | KeyCode::KeyW => GameKey::Up,
        KeyCode::ArrowDown | KeyCode::KeyS => GameKey::Down,
        KeyCode::ArrowLeft | KeyCode::KeyA => GameKey::Left,
        KeyCode::ArrowRight | KeyCode::KeyD => GameKey::Right,
        KeyCode::Enter | KeyCode::Space => GameKey::Confirm,
        KeyCode::Escape => GameKey::Cancel,
        KeyCode::Tab => GameKey::Menu,
        KeyCode::KeyM => GameKey::Map,
        KeyCode::Period => GameKey::Wait,
        _ => return None,
    };
    Some(action)
}

/// 事件循环的内部状态。
struct App<H: AppHandler> {
    config: WindowConfig,
    window: Option<Window>,
    input: InputState,
    handler: H,
}

impl<H: AppHandler> ApplicationHandler for App<H> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // resumed 在部分平台会被多次触发（例如从后台恢复），故必须幂等
        // ——重复建窗会泄漏资源。
        if self.window.is_some() {
            return;
        }

        let width = self.config.logical_width * self.config.scale;
        let height = self.config.logical_height * self.config.scale;

        let attributes = Window::default_attributes()
            // 标题此处暂用键名占位，i18n 接入后由上层设置真实标题。
            .with_title(self.config.title_key)
            .with_inner_size(winit::dpi::PhysicalSize::new(width, height))
            .with_resizable(false);

        match event_loop.create_window(attributes) {
            Ok(window) => {
                tracing::info!(width, height, "window created");
                window.request_redraw();
                self.window = Some(window);
            }
            Err(error) => {
                tracing::error!(%error, "failed to create window");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.handler.on_exit();
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(action) = map_physical_key(code) else {
                    return;
                };
                match event.state {
                    ElementState::Pressed => self.input.press(action),
                    ElementState::Released => self.input.release(action),
                }
            }
            WindowEvent::RedrawRequested => {
                self.handler.on_frame(&self.input);
                // 必须在逻辑处理之后清「刚按下」标志，放在之前会让所有
                // 「刚按下」判定永远为假。
                self.input.end_frame();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// 建窗并驱动事件循环，直到窗口关闭。
pub fn run<H: AppHandler + 'static>(config: WindowConfig, handler: H) -> Result<(), PlatformError> {
    let event_loop = EventLoop::new().map_err(|e| PlatformError::WindowCreation(e.to_string()))?;

    // Poll 而非 Wait：回合制虽然不需要持续重绘，但离屏世界推进要利用玩家
    // 思考的空窗期，因此主循环必须持续转动而不是阻塞等事件。
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        config,
        window: None,
        input: InputState::new(),
        handler,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| PlatformError::WindowCreation(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::KeyCode;

    #[test]
    fn 方向键映射到对应动作() {
        // Arrange
        let physical = KeyCode::ArrowUp;

        // Act
        let action = map_physical_key(physical);

        // Assert
        assert_eq!(action, Some(GameKey::Up));
    }

    #[test]
    fn 字母键位也映射到方向以适应手部姿势偏好() {
        // 传统 Roguelike 玩家习惯 WASD，强制只用方向键会劝退一部分人。
        // Arrange
        let physical = KeyCode::KeyW;

        // Act
        let action = map_physical_key(physical);

        // Assert
        assert_eq!(action, Some(GameKey::Up));
    }

    #[test]
    fn 未绑定的键返回空值() {
        // Arrange
        let physical = KeyCode::F13;

        // Act
        let action = map_physical_key(physical);

        // Assert
        assert_eq!(action, None);
    }

    #[test]
    fn 默认配置使用规格规定的逻辑分辨率() {
        // 规格 §2 决策 6：逻辑分辨率固定 640×360。
        // Arrange & Act
        let config = WindowConfig::default();

        // Assert
        assert_eq!((config.logical_width, config.logical_height), (640, 360));
    }
}
