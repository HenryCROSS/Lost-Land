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
use crate::input::{InputState, MouseButton, RepeatConfig};
use crate::keybind::{InputContext, KeyBindings, Modifiers, WheelDirection};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::WindowId;

/// 按**上层当前所处的输入上下文**把一次物理按键解析成抽象动作。
///
/// 这一个函数就是交接文档第四节第 17 条那条死路径的接线点：此前
/// [`App::window_event`] 把 [`InputContext::Gameplay`] 写死在调用里，
/// `crate::keybind::DEFAULT_MENU_BINDINGS` 那 11 条绑定因此在运行期
/// 永远查不到。
///
/// 抽成自由函数而不是留在事件循环里内联：`ApplicationHandler::window_event`
/// 需要一个只有 winit 事件循环才造得出的 `ActiveEventLoop`，测试进不去；
/// 抽出来之后「上下文真的来自 handler」这条断言才有地方落（见本模块
/// 测试 `上层处于菜单上下文时按键按菜单表解析`）。
fn resolve_key_for<H: AppHandler>(
    bindings: &KeyBindings,
    handler: &H,
    code: winit::keyboard::KeyCode,
    modifiers: Modifiers,
) -> Option<crate::input::GameKey> {
    bindings.resolve(code, modifiers, handler.input_context())
}

/// 滚轮版的 [`resolve_key_for`]——判重维度不同（`(方向, 上下文)`），
/// 但「上下文由上层给」这一条完全一致，见 `crate::keybind::WheelDirection`
/// 文档。
fn resolve_wheel_for<H: AppHandler>(
    bindings: &KeyBindings,
    handler: &H,
    direction: WheelDirection,
) -> Option<crate::input::GameKey> {
    bindings.resolve_wheel(direction, handler.input_context())
}

/// 把 winit 的鼠标按键换算成本项目的 [`MouseButton`]——只认左中右三键
/// （见 [`MouseButton`] 文档），winit 的 `Back`/`Forward`/`Other(_)`
/// 当前没有任何消费者，换算成 `None` 让调用方原样忽略这次事件,不是
/// 悄悄把它归到某个已有变体上（那会是一次错误的按键映射,比如把
/// 侧键误判成右键）。
fn map_mouse_button(button: winit::event::MouseButton) -> Option<MouseButton> {
    match button {
        winit::event::MouseButton::Left => Some(MouseButton::Left),
        winit::event::MouseButton::Right => Some(MouseButton::Right),
        winit::event::MouseButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

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
    /// 存键而非字面量，因为标题是用户可见字符串，必须走 i18n。**本层
    /// 自己不解析这个键**——`ll-platform` 不认识 Fluent，也不该认识：
    /// 平台层只管「怎么开一扇窗」，查表是表现层（`ll-i18n`）的职责。
    /// 真正显示的文本是 [`resolved_title`](Self::resolved_title)，由
    /// 调用方在装载完 `ll_i18n::Catalog` 后解析好再填进来。这个字段
    /// 保留下来是给调用方留一个「这个标题本该走哪个键」的可核对锚点，
    /// 也是缺省未解析场景（[`Default`] 实现）的兜底来源。
    pub title_key: &'static str,
    /// 已经解析好、真正会出现在窗口标题栏/任务栏上的文本。
    ///
    /// 默认等于 [`title_key`](Self::title_key) 本身——这不是偷懒，是
    /// 刻意与 `ll_i18n::Catalog` 缺键时的回退策略保持同一套语义（见其
    /// 模块文档「缺键与缺语言」一节）：没有真正解析过的标题，就该长得
    /// 像一个未解析的键，而不是猜一个看起来正常的占位字符串——那样会
    /// 把「i18n 根本没接上」这个缺陷伪装成「已经接上但选错了文案」。
    pub resolved_title: String,
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
            resolved_title: "window.title".to_string(),
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
    ///
    /// # `input` 为什么是 `&mut`
    ///
    /// 上层打开/关闭一块模态 UI 时必须清空按键状态
    /// （`ll_ui::widget::ui_mode::UiModeStack::push`/`pop` 要求
    /// `&mut InputState`）——那是**第三种「隐式全键松开」边界**，与窗口
    /// 失焦完全同一个函数、同一套语义，完整论证见
    /// `knowledge/design/action-capability-and-input-context.md` 2.3 节
    /// 与 `UiModeStack` 的模块文档。不给可变引用，那条不变式就没有任何
    /// 调用点能守住：打开菜单时按着的 W 会带着「已按住」的状态进菜单，
    /// 关闭菜单时按着的方向键会让角色立刻窜一格。
    fn on_frame(&mut self, frame: FrameId, input: &mut InputState) -> FrameOutcome;

    /// 本帧的物理按键该按哪个 [`InputContext`] 查绑定表。
    ///
    /// # 为什么由上层回答，而不是平台层自己维护
    ///
    /// 「现在是不是有一块模态 UI 盖在游戏画面上」是 UI 导航层的状态
    /// （`ll_ui::widget::ui_mode::UiModeStack`），而 `ll-ui` 排在
    /// `ll-platform` 的**下游**（规格 §5 依赖顺序），平台层物理上依赖
    /// 不到那个类型。反过来把栈下沉进 `ll-platform` 已经被设计文档
    /// 2.1 节明确否决：那会让 [`KeyBindings::resolve`] 从纯查表变成
    /// 关心「之前发生过什么」的有状态查询。
    ///
    /// 于是唯一剩下的接法就是本方法：栈住在**同时认识两边**的那一层
    /// （`ll_game::app::Demo`），平台层每次按键事件问它一句。
    ///
    /// 默认返回 [`InputContext::Gameplay`]——六个验收 demo 都没有模态
    /// UI，行为与本方法引入之前**逐位等价**，它们一行都不用改。
    fn input_context(&self) -> InputContext {
        InputContext::Gameplay
    }

    /// 取走上层这一帧新改好的键位绑定表；`None`（默认）表示没改过。
    ///
    /// # 为什么需要这条回流通道
    ///
    /// 真正被 [`Self::input_context`] 那条路径查的绑定表住在平台层
    /// （[`WindowConfig::bindings`]，`ll_game::run_game` 启动时把配置
    /// 里那份**移动**进来）。设置界面改的是上层自己那份草稿——不把改动
    /// 送回来，玩家会看到「设置界面里显示改好了，按下去还是旧的」。
    ///
    /// # 为什么是「取走」而不是「借出」
    ///
    /// 取走（`Option` 判空 + 少数几帧真的搬一次表）比每帧借出并比对
    /// 便宜得多，也让「谁能改绑定表」变成一个具名、可搜索的入口，而
    /// 不是一个到处都摸得到的 `&mut`。
    fn take_rebound_keys(&mut self) -> Option<KeyBindings> {
        None
    }

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
            .with_title(&self.config.resolved_title)
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
                // 原始物理键先无条件记一份——**必须排在 resolve 之前**：
                // 重绑定要的恰恰是那些当前还没有任何绑定的键，它们
                // 走不过下面那个 `else { return }`，见
                // `InputState::last_physical_key` 文档。
                if event.state == ElementState::Pressed {
                    self.input.record_physical_key(code);
                }
                let Some(action) =
                    resolve_key_for(&self.config.bindings, &self.handler, code, self.modifiers)
                else {
                    return;
                };
                match event.state {
                    ElementState::Pressed => self.input.press(action),
                    ElementState::Released => self.input.release(action),
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // 滚轮走独立的 resolve_wheel/pulse 入口，不复用键盘的
                // resolve/press——两者判重维度不同（键盘按 (key,
                // modifiers, context)，滚轮按 (direction, context)），
                // pulse 的语义也与 press 不同（不进入「按住」状态，见
                // `InputState::pulse` 文档），理由见
                // `crate::keybind::WheelDirection` 模块文档。
                if let Some(direction) = WheelDirection::from_scroll_delta(delta)
                    && let Some(action) =
                        resolve_wheel_for(&self.config.bindings, &self.handler, direction)
                {
                    self.input.pulse(action);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // `position` 已经是窗口原生像素坐标系下的物理坐标（winit
                // 的 `PhysicalPosition`），与 `ll_ui::widget::geometry::Rect`
                // 同一套坐标系，不需要任何换算——见
                // `InputState::cursor_position` 字段文档。
                self.input
                    .set_cursor_position((position.x as f32, position.y as f32));
            }
            WindowEvent::CursorLeft { .. } => {
                // 光标确认离开了窗口范围——与失焦不同，这里没有「光标其实
                // 还在原处只是收不到事件」的可能性，见
                // `InputState::clear_cursor_position` 文档。
                self.input.clear_cursor_position();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = map_mouse_button(button) else {
                    return;
                };
                match state {
                    ElementState::Pressed => self.input.mouse_press(button),
                    ElementState::Released => self.input.mouse_release(button),
                }
            }
            WindowEvent::Focused(false) => {
                // 失焦后操作系统不再把按键事件送到本窗口，已按下的键将永远
                // 收不到松开事件。不清空会导致切回来时角色持续移动。鼠标
                // 按键同理（见 `InputState::clear` 文档「鼠标按键同理」
                // 一节），`clear()` 已经把两者一并清空。
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
                let outcome = self.handler.on_frame(self.frame, &mut self.input);
                // 设置界面这一帧若改过键位，把新表换进来——见
                // `AppHandler::take_rebound_keys` 文档。放在 `on_frame`
                // 之后：改动就是在这一帧的 `on_frame` 里发生的。
                if let Some(bindings) = self.handler.take_rebound_keys() {
                    tracing::info!("键位绑定表已由上层替换");
                    self.config.bindings = bindings;
                }
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

    /// 只回答「我现在在哪个输入上下文」的最小 handler——其余回调全是
    /// 空实现：本组测试要验证的只有解析路径怎么取上下文，构造一个真实
    /// 的 `ll_game::app::Demo` 需要 GPU、内容表与整个世界。
    struct 固定上下文Handler {
        context: InputContext,
        rebound: Option<KeyBindings>,
    }

    impl AppHandler for 固定上下文Handler {
        fn on_resume(&mut self, _window: Arc<Window>, _size: PhysicalSize<u32>) {}
        fn on_resize(&mut self, _size: PhysicalSize<u32>) {}
        fn on_frame(&mut self, _frame: FrameId, _input: &mut InputState) -> FrameOutcome {
            FrameOutcome::Continue
        }
        fn on_exit(&mut self) {}
        fn input_context(&self) -> InputContext {
            self.context
        }
        fn take_rebound_keys(&mut self) -> Option<KeyBindings> {
            self.rebound.take()
        }
    }

    fn 固定上下文(context: InputContext) -> 固定上下文Handler {
        固定上下文Handler {
            context,
            rebound: None,
        }
    }

    #[test]
    fn 上层处于游戏内上下文时菜单键解析成打开菜单() {
        // Arrange：Tab 只在 Gameplay 上下文下绑给 GameKey::Menu。
        let config = WindowConfig::default();
        let handler = 固定上下文(InputContext::Gameplay);

        // Act
        let action = resolve_key_for(&config.bindings, &handler, KeyCode::Tab, Modifiers::NONE);

        // Assert
        assert_eq!(action, Some(GameKey::Menu));
    }

    #[test]
    fn 上层处于菜单上下文时按键按菜单表解析() {
        // 这一条是「InputContext::Menu 运行期是死路径」那条缺陷的直接
        // 回归断言：Tab 在 DEFAULT_MENU_BINDINGS 里**没有**任何绑定，
        // 一旦解析路径把上下文写死回 Gameplay，它就会解析出
        // GameKey::Menu，本断言当场变红。
        // Arrange
        let config = WindowConfig::default();
        let handler = 固定上下文(InputContext::Menu);

        // Act
        let action = resolve_key_for(&config.bindings, &handler, KeyCode::Tab, Modifiers::NONE);

        // Assert
        assert_eq!(action, None);
    }

    #[test]
    fn 菜单上下文下取消键仍解析得到取消动作() {
        // 上一条只证明了「菜单上下文不是游戏内上下文」；这一条证明菜单
        // 那张表真的被查到了，而不是解析路径整个失效。
        // Arrange
        let config = WindowConfig::default();
        let handler = 固定上下文(InputContext::Menu);

        // Act
        let action = resolve_key_for(&config.bindings, &handler, KeyCode::Escape, Modifiers::NONE);

        // Assert
        assert_eq!(action, Some(GameKey::Cancel));
    }

    #[test]
    fn 滚轮解析同样按上层给的上下文查表() {
        // DEFAULT_WHEEL_BINDINGS 只登记了 Gameplay 上下文的两个方向。
        // Arrange
        let config = WindowConfig::default();
        let 游戏内 = 固定上下文(InputContext::Gameplay);
        let 菜单里 = 固定上下文(InputContext::Menu);

        // Act
        let 游戏内动作 = resolve_wheel_for(&config.bindings, &游戏内, WheelDirection::Away);
        let 菜单里动作 = resolve_wheel_for(&config.bindings, &菜单里, WheelDirection::Away);

        // Assert
        assert_eq!(游戏内动作, Some(GameKey::ZoomIn));
        assert_eq!(菜单里动作, None);
    }

    #[test]
    fn 上层不改键位时取走绑定表得到空值() {
        // Arrange
        let mut handler = 固定上下文(InputContext::Gameplay);

        // Act
        let taken = handler.take_rebound_keys();

        // Assert
        assert!(taken.is_none());
    }

    #[test]
    fn 取走过一次之后不会再取到第二次() {
        // 「取走」而非「借出」的语义验证：同一份改动不该被重复搬运。
        // Arrange
        let mut handler = 固定上下文Handler {
            context: InputContext::Gameplay,
            rebound: Some(KeyBindings::default_bindings()),
        };

        // Act
        let 第一次 = handler.take_rebound_keys();
        let 第二次 = handler.take_rebound_keys();

        // Assert
        assert!(第一次.is_some());
        assert!(第二次.is_none());
    }

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
    fn 默认窗口配置的已解析标题等于未解析的键名本身() {
        // 未经真正的 i18n 解析时，`resolved_title` 应当长得像一个
        // 没被翻译过的键，而不是任何看起来正常的占位文案——理由见
        // `WindowConfig::resolved_title` 文档。
        // Arrange & Act
        let config = WindowConfig::default();

        // Assert
        assert_eq!(config.resolved_title, config.title_key);
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
