//! GPU 设备初始化与 surface 生命周期管理。
//!
//! 场景先绘制到固定分辨率的离屏纹理（见 [`crate::target`]），再放大 blit
//! 到这里管理的窗口 surface。设备与 surface 的生命周期因此与窗口尺寸解耦
//! ——`GpuContext` 只负责「能不能画」，不关心「画多大」。

use crate::RenderError;
use ll_platform::window::{PhysicalSize, Window};
use std::sync::Arc;

/// 持有 wgpu 设备、队列与窗口 surface。
///
/// 生命周期跨越整个应用运行期，由 [`ll_platform::window::AppHandler::on_resume`]
/// 创建一次。
pub struct GpuContext {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

impl GpuContext {
    /// 初始化 GPU 设备并为给定窗口配置绘制 surface。
    ///
    /// 适配器选择偏好高性能独显，但**不排斥软件后端**（`force_fallback_adapter:
    /// false` 只是不强制回退，并不排除 CPU 光栅化适配器如 Mesa lavapipe 被
    /// 选中）——CI 环境通常没有独显，若排斥软件后端，视觉回归测试将无法运行。
    ///
    /// `vsync` 选择呈现模式：`true` 用 [`wgpu::PresentMode::Fifo`]
    /// （垂直同步，画面撕裂绝不会发生，但帧率被锁定在显示器刷新率
    /// 以内——这是唯一所有平台都保证支持的模式，也是找不到 `vsync`
    /// 首选模式时的兜底），`false` 用 [`wgpu::PresentMode::Immediate`]
    /// （不等垂直消隐直接呈现，延迟最低但可能画面撕裂，多数平台支持，
    /// 但不保证——见 [`choose_present_mode`] 的回退逻辑）。不直接把
    /// wgpu 全部六种呈现模式（还有 `AutoVsync`/`AutoNoVsync`/
    /// `FifoRelaxed`/`Mailbox`）暴露给用户配置：多数游戏的「垂直同步」
    /// 选项就是这两选一，`Mailbox` 等模式不是所有平台都支持、语义也
    /// 更微妙（三缓冲低延迟），加进用户可选项只会让配置界面的选择
    /// 变得难以解释，而没有对应的实际收益（YAGNI）。
    pub fn new(
        window: Arc<Window>,
        size: PhysicalSize<u32>,
        vsync: bool,
    ) -> Result<GpuContext, RenderError> {
        let instance = wgpu::Instance::default();

        let surface = instance
            .create_surface(window)
            .map_err(|error| RenderError::SurfaceCreation(error.to_string()))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|_| RenderError::NoAdapter)?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ll-render device"),
            ..Default::default()
        }))
        .map_err(|error| RenderError::DeviceRequest(error.to_string()))?;

        let capabilities = surface.get_capabilities(&adapter);
        // 必须优先挑 sRGB 变体，不能无条件取 formats[0]：blit.wgsl 的
        // fs_main 是直通采样，不做任何色彩空间转换——离屏目标固定用
        // TARGET_FORMAT（sRGB，见 target.rs），采样时 GPU 按 sRGB 语义
        // 自动解码成线性值，这个线性值只有写进同样是 *Srgb 变体的 color
        // target 时才会被 GPU 自动重新编码回 sRGB。若这里选中非 sRGB
        // 格式（如 Bgra8Unorm），线性值会被当作 UNORM 字节直接写入，
        // 画面会整体偏暗。绝大多数平台的 surface 能力列表里都有 sRGB
        // 变体，找不到才退回首选格式并记一条警告——那种情况下画面偏暗
        // 是已知的、被显式记录下来的风险，而不是没人注意到的巧合。
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or_else(|| {
                let fallback = capabilities.formats[0];
                tracing::warn!(
                    ?fallback,
                    "该平台的 surface 能力列表中没有 sRGB 格式变体，\
                     blit 管线假设 color target 是 sRGB 的前提不成立，\
                     画面可能整体偏暗"
                );
                fallback
            });
        tracing::info!(?format, "selected surface format");
        let alpha_mode = capabilities.alpha_modes[0];

        let width = size.width.max(1);
        let height = size.height.max(1);

        let present_mode = choose_present_mode(&capabilities.present_modes, vsync);
        tracing::info!(?present_mode, vsync, "selected present mode");

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };

        // 用 max(1) 夹过的尺寸配置，即便调用方传入零尺寸也不会在此崩溃；
        // 但仍需以原始尺寸判断是否值得配置，避免用假造的 1×1 尺寸初始化
        // 出一个实际上不可见的 surface。
        if is_presentable(size.width, size.height) {
            surface.configure(&device, &config);
        }

        Ok(GpuContext {
            surface,
            device,
            queue,
            config,
        })
    }

    /// 窗口尺寸变化时重新配置 surface。
    ///
    /// 对零尺寸直接返回而不重配：窗口最小化时宽高会变成 `(0, 0)`，
    /// 平台层原样转发这个事件（见 `ll-platform` 的分层约定——平台层不该
    /// 知道 wgpu surface 的限制），重配到零尺寸会让 wgpu 报错甚至崩溃，
    /// 这是接 GPU 最常见的崩溃点。
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if !is_presentable(size.width, size.height) {
            return;
        }

        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    /// GPU 逻辑设备，用于创建缓冲、纹理、管线等资源。
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// 命令提交队列。
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 窗口 surface 实际使用的纹理格式。
    ///
    /// **与离屏渲染目标的格式是两件独立的事**（离屏目标固定用
    /// [`crate::target::TARGET_FORMAT`]，见该常量文档）——这个值只跟
    /// 窗口 surface 走，由 [`Self::new`] 里从 `capabilities.formats`
    /// 优先挑出的 sRGB 变体决定（找不到才退回首选格式，见该处注释）。
    /// [`crate::target::RenderTarget::blit_to`] 正是在「离屏目标固定的
    /// `TARGET_FORMAT`」与「这里返回的窗口 surface 格式」之间做放大与
    /// 格式转换，两者不需要也不应该相等。
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// 取得当前帧的呈现目标。
    ///
    /// 调用方需自行创建视图、渲染，然后把返回值传给
    /// `queue().present(frame)` 提交给合成器——wgpu 30 把 `present`
    /// 挪到了 `Queue` 上，不再是 `SurfaceTexture` 自己的方法。
    /// 不把这三步包成一个回调，是因为渲染内容由上层决定，回调会把控制
    /// 反转得毫无必要。
    ///
    /// `wgpu::Surface::get_current_texture` 在这个版本的 wgpu 里返回的
    /// 是 [`wgpu::CurrentSurfaceTexture`] 枚举（wgpu 30 把旧版
    /// `Result<SurfaceTexture, SurfaceError>` 改成了专用枚举，各变体的
    /// 语义与旧版 `SurfaceError` 基本对应），本方法按变体分别处理：
    ///
    /// - `Outdated`/`Lost`（窗口尺寸变化、显示器切换或驱动重置时会
    ///   发生）：用当前配置重新 `configure` 一次 surface 后重试一次
    ///   ——上层对「surface 需要重新配置」这件事本身无能为力，而重配
    ///   是标准且总能恢复的做法，不该把它当成需要向上传播的错误。
    /// - `Suboptimal`：纹理仍可正常绘制，只是不再是最优配置；正常
    ///   返回，同时顺带重配一次 surface，让下一帧恢复最优。
    /// - `Timeout`/`Occluded`：wgpu 自身文档建议「跳过本帧，稍后重试」
    ///   ——这不是真正的故障，这里转成 `Err` 交给调用方跳过本帧，而不是
    ///   当场重试到成功为止（重试到成功可能无限阻塞主循环）。
    /// - `Validation`：真正的错误，直接返回 `Err`。
    pub fn acquire_frame(&self) -> Result<wgpu::SurfaceTexture, RenderError> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Ok(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.surface.configure(&self.device, &self.config);
                Ok(frame)
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(frame),
                    other => Err(RenderError::SurfaceAcquire(format!(
                        "重新配置后仍无法取得 surface 帧：{other:?}"
                    ))),
                }
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Err(RenderError::SurfaceAcquire(
                    "本帧暂时取不到 surface 纹理，调用方应跳过本帧".to_string(),
                ))
            }
            wgpu::CurrentSurfaceTexture::Validation => Err(RenderError::SurfaceAcquire(
                "取得 surface 帧时发生校验错误".to_string(),
            )),
        }
    }
}

/// 按 `vsync` 偏好从该 surface 实际支持的呈现模式列表里选一个。
///
/// 首选模式若不在 `supported` 里，退回 [`wgpu::PresentMode::Fifo`]——
/// 这是 wgpu 文档保证「所有平台都支持」的唯一模式（见
/// [`GpuContext::new`] 文档），因此这条兜底恒安全，不需要再处理
/// 「兜底本身也不受支持」这种情况。
///
/// 提取成自由函数是为了让这个选择逻辑能被单测覆盖——`GpuContext`
/// 本身需要真实窗口与 GPU 适配器，在 CI 里测不了，这与 `is_presentable`
/// 提取的理由完全一致。
fn choose_present_mode(supported: &[wgpu::PresentMode], vsync: bool) -> wgpu::PresentMode {
    let preferred = if vsync {
        wgpu::PresentMode::Fifo
    } else {
        wgpu::PresentMode::Immediate
    };
    if supported.contains(&preferred) {
        preferred
    } else {
        tracing::warn!(
            ?preferred,
            "该平台不支持首选的呈现模式，退回 Fifo（垂直同步，所有平台都保证支持）"
        );
        wgpu::PresentMode::Fifo
    }
}

/// 该尺寸是否可用于配置绘制表面。
///
/// 窗口最小化时尺寸会变成零，此时重新配置 surface 会让 wgpu 报错甚至
/// 崩溃。提取成自由函数是为了让这个判断能被单测覆盖——`GpuContext`
/// 本身需要真实窗口，在 CI 里测不了。
fn is_presentable(width: u32, height: u32) -> bool {
    width > 0 && height > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 零宽度被判定为不可呈现() {
        // 窗口最小化时尺寸变为零，此时重配 surface 会让 wgpu 报错甚至崩溃。
        // Arrange & Act & Assert
        assert!(!is_presentable(0, 720));
    }

    #[test]
    fn 零高度被判定为不可呈现() {
        // Arrange & Act & Assert
        assert!(!is_presentable(1280, 0));
    }

    #[test]
    fn 正常尺寸可呈现() {
        // Arrange & Act & Assert
        assert!(is_presentable(1280, 720));
    }

    #[test]
    fn 请求垂直同步且受支持时选中fifo() {
        // Arrange
        let supported = [wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate];

        // Act
        let chosen = choose_present_mode(&supported, true);

        // Assert
        assert_eq!(chosen, wgpu::PresentMode::Fifo);
    }

    #[test]
    fn 不请求垂直同步且受支持时选中immediate() {
        // Arrange
        let supported = [wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate];

        // Act
        let chosen = choose_present_mode(&supported, false);

        // Assert
        assert_eq!(chosen, wgpu::PresentMode::Immediate);
    }

    #[test]
    fn 不请求垂直同步但平台不支持immediate时退回fifo() {
        // Arrange：只支持 Fifo 的平台列表。
        let supported = [wgpu::PresentMode::Fifo];

        // Act
        let chosen = choose_present_mode(&supported, false);

        // Assert
        assert_eq!(chosen, wgpu::PresentMode::Fifo);
    }
}
