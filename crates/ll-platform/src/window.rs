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

/// 一帧处理完毕后，上层希望事件循环如何继续。
///
/// 存在的理由：平台层不认识任何游戏概念，无从判断「玩家是否想退出」。
/// 让 [`AppHandler::on_frame`] 把这个决定回传，退出就成了上层的显式意图，
/// 而不是靠平台层猜某个按键的含义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// 继续驱动下一帧。
    Continue,
    /// 请求结束事件循环。
    Exit,
}

/// 上层需要实现的帧回调。
///
/// 平台层只负责把事件归约成输入状态并按帧驱动，不含任何游戏逻辑。
pub trait AppHandler {
    /// 每帧调用一次，`input` 是本帧归约后的输入状态。
    ///
    /// 返回值决定事件循环是否继续，见 [`FrameOutcome`]。
    fn on_frame(&mut self, input: &InputState) -> FrameOutcome;

    /// 事件循环结束前调用一次，用于保存与清理。
    ///
    /// 无论退出是由窗口关闭按钮触发还是由 [`FrameOutcome::Exit`] 触发，
    /// 都**恰好调用一次**。
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
    /// 是否已经调用过 `on_exit`。
    ///
    /// 窗口关闭与主动退出是两条独立路径，都会触发收尾；没有这个标志，
    /// 某些平台上两条路径先后触发会让存档逻辑跑两遍。
    has_exited: bool,
}

impl<H: AppHandler> App<H> {
    /// 执行一次且仅一次收尾，然后请求事件循环退出。
    fn shutdown(&mut self, event_loop: &ActiveEventLoop) {
        if !self.has_exited {
            self.has_exited = true;
            self.handler.on_exit();
        }
        event_loop.exit();
    }
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
                self.shutdown(event_loop);
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
            WindowEvent::Focused(false) => {
                // 失焦后操作系统不再把按键事件送到本窗口，已按下的键将永远
                // 收不到松开事件。不清空会导致切回来时角色持续移动。
                self.input.clear();
            }
            WindowEvent::RedrawRequested => {
                let outcome = self.handler.on_frame(&self.input);
                // 必须在逻辑处理之后清「刚按下」标志，放在之前会让所有
                // 「刚按下」判定永远为假。
                self.input.end_frame();

                match outcome {
                    FrameOutcome::Exit => self.shutdown(event_loop),
                    FrameOutcome::Continue => {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// 建窗并驱动事件循环，直到窗口关闭。
pub fn run<H: AppHandler + 'static>(config: WindowConfig, handler: H) -> Result<(), PlatformError> {
    let event_loop = EventLoop::new().map_err(|e| PlatformError::EventLoop(e.to_string()))?;

    // Poll 而非 Wait：回合制虽然不需要持续重绘，但离屏世界推进要利用玩家
    // 思考的空窗期，因此主循环必须持续转动而不是阻塞等事件。
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        config,
        window: None,
        input: InputState::new(),
        handler,
        has_exited: false,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| PlatformError::EventLoop(e.to_string()))
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

    #[test]
    fn 取消键映射到退出动作() {
        // demo 与后续的「退出游戏」菜单都依赖这条映射，
        // 它曾经是全项目唯一映射却无人消费的死映射。
        // Arrange
        let physical = KeyCode::Escape;

        // Act
        let action = map_physical_key(physical);

        // Assert
        assert_eq!(action, Some(GameKey::Cancel));
    }
}
