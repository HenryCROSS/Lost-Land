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
    /// 呈现模式固定为 `Fifo`（垂直同步）：这是唯一所有平台都保证支持的模式，
    /// 且帧节流已由平台层的帧预算完成，不需要更激进的呈现模式。
    pub fn new(window: Arc<Window>, size: PhysicalSize<u32>) -> Result<GpuContext, RenderError> {
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
        // 首选格式即可：图集与离屏目标已经用同一格式画好，交给 surface
        // 挑一个它保证支持的格式，不必强求 sRGB 与否。
        let format = capabilities.formats[0];
        let alpha_mode = capabilities.alpha_modes[0];

        let width = size.width.max(1);
        let height = size.height.max(1);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
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
    /// 离屏渲染目标（见 [`crate::target::RenderTarget`]）用同一格式创建，
    /// 这样 blit 管线不必处理格式转换。
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
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
}
