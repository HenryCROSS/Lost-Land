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
use crate::input::{InputState, RepeatConfig};
use crate::keybind::{InputContext, KeyBindings, Modifiers};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::WindowId;

// 向上层重新导出窗口层的类型。
//
// 上层需要命名这两个类型才能接收 `AppHandler::on_resume` 的参数，但
// 不应为此直接依赖 winit——本项目只允许 ll-platform 接触窗口库，
// 这样将来更换窗口库时只需改这一个 crate，上层源码一行不动。
pub use winit::dpi::PhysicalSize;
pub use winit::window::Window;

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
    /// 按键自动重复的时序参数，逐帧驱动 [`InputState::begin_frame`]。
    pub repeat: RepeatConfig,
    /// 目标帧率，用于算出 [`WindowConfig::frame_budget`] 节流主循环。
    pub target_fps: u32,
    /// 物理按键 → 抽象动作的绑定表。
    ///
    /// 默认取 [`KeyBindings::default_bindings`]（第一档静态声明，见
    /// `crate::keybind` 模块文档）。上层若想跑一份自定义绑定（例如从
    /// 未来的配置文件加载出来的结果），在这里替换即可——事件循环本身
    /// 不关心这张表从哪来，只调用 [`KeyBindings::resolve`]。
    pub bindings: KeyBindings,
}

impl Default for WindowConfig {
    fn default() -> Self {
        WindowConfig {
            logical_width: 640,
            logical_height: 360,
            // 默认 2 倍得到 1280×720，在绝大多数显示器上都能完整显示。
            scale: 2,
            title_key: "window.title",
            repeat: RepeatConfig::default(),
            // 60 是像素游戏的通行帧率，且与常见显示器刷新率对齐。
            target_fps: 60,
            bindings: KeyBindings::default_bindings(),
        }
    }
}

impl WindowConfig {
    /// 每帧的时间预算。目标帧率为零时返回零，表示不节流。
    ///
    /// 节流是必需的：主循环用 `ControlFlow::Poll`，不加预算会空转吃满
    /// 一核。P0 阶段无渲染时这只是浪费电，接入 GPU 后会直接抬高功耗与
    /// 温度。
    pub fn frame_budget(&self) -> Duration {
        if self.target_fps == 0 {
            return Duration::ZERO;
        }
        Duration::from_nanos(1_000_000_000 / self.target_fps as u64)
    }
}

/// 单调递增的帧号。
///
/// 动画播放以此为时间基准而非墙钟浮点秒数——整数帧号可以安全地进入
/// 世界状态并被存档序列化，浮点秒数不行（会破坏跨平台确定性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FrameId(pub u64);

impl FrameId {
    /// 下一帧。
    ///
    /// 用 `wrapping_add`：以 60fps 计需连续运行约 97 亿年才会回绕，
    /// 但回绕总好过在极端情况下 panic。
    pub const fn next(self) -> Self {
        FrameId(self.0.wrapping_add(1))
    }
}

/// 一帧处理完毕后，上层希望事件循环如何继续。
///
/// 存在的理由：平台层不认识任何游戏概念，无从判断「玩家是否想退出」。
/// 让 [`AppHandler::on_frame`] 把这个决定回传，退出就成了上层的显式意图，
/// 而不是靠平台层猜某个按键的含义——退出可能来自 Esc、来自菜单里的
/// 「退出游戏」、来自剧情结局，平台层无从判断，也不该判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameOutcome {
    /// 继续驱动下一帧。
    #[default]
    Continue,
    /// 请求结束事件循环。
    Exit,
}

/// 上层需要实现的回调。
///
/// 平台层只负责把系统事件归约成输入状态并按帧驱动，不含任何游戏逻辑。
pub trait AppHandler {
    /// 窗口就绪时调用，此时可以创建 GPU surface。
    ///
    /// 传 `Arc<Window>` 而非 `&Window`：wgpu 的 surface 需要持有窗口的
    /// 生命周期，共享所有权比移交所有权简单——移交后平台层自己就没法
    /// 再用窗口了。
    fn on_resume(&mut self, window: Arc<Window>, size: PhysicalSize<u32>);

    /// 窗口尺寸或缩放因子变化时调用，surface 必须据此重建。
    ///
    /// `size` 可能为 `(0, 0)`（窗口最小化时）——这条零尺寸防线设在渲染层
    /// （`ll-render` 的 `GpuContext::resize` 对零尺寸直接返回而不重配
    /// surface），本层原样转发事件，不在此处过滤或校验。
    fn on_resize(&mut self, size: PhysicalSize<u32>);

    /// 每帧调用一次。返回值告诉平台层是否应当退出。
    ///
    /// 让退出成为上层的**显式意图**，平台层不必去猜某个按键的含义——
    /// 退出可能来自 Esc、来自菜单里的「退出游戏」、来自剧情结局，
    /// 平台层无从判断，也不该判断。
    fn on_frame(&mut self, frame: FrameId, input: &InputState) -> FrameOutcome;

    /// 退出前调用，用于保存与清理。
    ///
    /// 无论退出是由窗口关闭按钮触发还是由 [`FrameOutcome::Exit`] 触发，
    /// 都**恰好调用一次**。
    fn on_exit(&mut self);
}

/// 事件循环的内部状态。
struct App<H: AppHandler> {
    config: WindowConfig,
    window: Option<Arc<Window>>,
    input: InputState,
    /// 当前按住的修饰键状态，随 `WindowEvent::ModifiersChanged` 更新。
    ///
    /// winit 把修饰键变化与按键事件报告为两条独立的 `WindowEvent`，
    /// 必须自己攒住最近一次的状态，供 `KeyboardInput` 事件到达时查询
    /// ——`KeyboardInput` 本身不随附修饰键信息。
    modifiers: Modifiers,
    handler: H,
    frame: FrameId,
    last_frame_at: Option<Instant>,
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
                let window = Arc::new(window);
                window.request_redraw();
                self.handler.on_resume(window.clone(), window.inner_size());
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
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = Modifiers::from(modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(action) =
                    self.config
                        .bindings
                        .resolve(code, self.modifiers, InputContext::Gameplay)
                else {
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
            WindowEvent::Resized(size) => {
                self.handler.on_resize(size);
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // 缩放因子变化后物理尺寸随之改变，直接用当前尺寸重建。
                if let Some(window) = &self.window {
                    self.handler.on_resize(window.inner_size());
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let budget = self.config.frame_budget();

                // 未到帧预算就跳过本帧的逻辑与绘制，只重新申请重绘。
                // 这里刻意不 sleep——sleep 会让窗口事件的响应延迟一整个
                // 帧时长，拖动窗口时会明显卡顿。
                let too_early = match self.last_frame_at {
                    Some(last) => !budget.is_zero() && now.duration_since(last) < budget,
                    None => false,
                };
                if too_early {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    return;
                }
                self.last_frame_at = Some(now);

                self.input.begin_frame(now, self.config.repeat);
                let outcome = self.handler.on_frame(self.frame, &self.input);
                // 必须在逻辑处理之后清「刚按下」与「本帧重复触发」标志，
                // 放在之前会让所有「刚按下」判定永远为假。
                self.input.end_frame();
                self.frame = self.frame.next();

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
        modifiers: Modifiers::NONE,
        handler,
        frame: FrameId::default(),
        last_frame_at: None,
        has_exited: false,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| PlatformError::EventLoop(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::GameKey;
    use winit::keyboard::KeyCode;

    #[test]
    fn 默认窗口配置的绑定表能解析方向键() {
        // 物理按键到动作的映射此前是本模块一段硬编码 match（
        // `map_physical_key`），现在改由 `crate::keybind::KeyBindings`
        // 承担——这里锁住 `WindowConfig::default()` 确实把默认绑定表
        // 接了进来，而不是留一个空表。
        // Arrange
        let config = WindowConfig::default();

        // Act
        let action =
            config
                .bindings
                .resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(GameKey::Up));
    }

    #[test]
    fn 默认窗口配置使用规格规定的逻辑分辨率() {
        // 规格 §2 决策 6：逻辑分辨率固定 640×360。
        // Arrange & Act
        let config = WindowConfig::default();

        // Assert
        assert_eq!((config.logical_width, config.logical_height), (640, 360));
    }

    #[test]
    fn 帧结果默认为继续而非退出() {
        // 平台层不该在没有上层显式意图时主动退出。
        // Arrange & Act
        let outcome = FrameOutcome::default();

        // Assert
        assert_eq!(outcome, FrameOutcome::Continue);
    }

    #[test]
    fn 帧号逐帧递增() {
        // Arrange
        let frame = FrameId(0);

        // Act
        let next = frame.next();

        // Assert
        assert_eq!(next, FrameId(1));
    }

    #[test]
    fn 默认帧率为六十() {
        // 60 是像素游戏的通行帧率，且与常见显示器刷新率对齐。
        // Arrange & Act
        let config = WindowConfig::default();

        // Assert
        assert_eq!(config.target_fps, 60);
    }

    #[test]
    fn 帧预算由目标帧率算出() {
        // Arrange
        let config = WindowConfig {
            target_fps: 60,
            ..WindowConfig::default()
        };

        // Act
        let budget = config.frame_budget();

        // Assert
        assert_eq!(budget, Duration::from_nanos(16_666_666));
    }

    #[test]
    fn 目标帧率为零时退化为不节流() {
        // 配置文件可能写出 0，与其除零崩溃不如退化为不限帧。
        // Arrange
        let config = WindowConfig {
            target_fps: 0,
            ..WindowConfig::default()
        };

        // Act
        let budget = config.frame_budget();

        // Assert
        assert_eq!(budget, Duration::ZERO);
    }
}
