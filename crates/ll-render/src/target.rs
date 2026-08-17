//! 离屏渲染目标：固定 640×360 的逻辑分辨率纹理及其整数倍放大 blit。
//!
//! 场景先画到这张固定尺寸的纹理，再按 [`fit_viewport`] 算出的整数倍放大
//! blit 到窗口 surface。像素完美因此与窗口尺寸彻底解耦，[`RenderTarget::read_pixels`]
//! 也能把离屏画面冻结成基准 PNG，不受运行环境分辨率影响。

use crate::gpu::GpuContext;

/// 逻辑分辨率宽度。规格决策 6 固定为 640。
pub const LOGICAL_WIDTH: u32 = 640;

/// 逻辑分辨率高度。规格决策 6 固定为 360。
pub const LOGICAL_HEIGHT: u32 = 360;

/// 离屏渲染目标固定使用的像素格式。
///
/// **必须与窗口 surface 格式脱钩、固定不变**——这是两件本来就不是
/// 同一回事的东西：
///
/// - 离屏纹理只被两处消费：场景批渲染画进去（[`crate::batch::SpriteBatch`]，
///   同样固定 sRGB 语义采样图集）、`read_pixels` 读出来做视觉回归比对。
///   两者都不关心运行它的窗口用什么 surface 格式。
/// - 若像早期实现那样让离屏格式抄自 `gpu.surface_format()`（平台决定，
///   常见的是非 sRGB 的 `Bgra8Unorm`），会同时踩两个坑：着色器从
///   sRGB 图集采到线性值、原样写进 UNORM 目标，画面整体发白；而且
///   离屏格式随平台变化，[`RenderTarget::read_pixels`] 产出的基准 PNG
///   就**不可跨平台比对**——这条基准存在的全部理由就是跨环境比对。
///
/// 与图集纹理固定用的格式一致（见 `atlas.rs` 里 `Atlas::load` 的说明），
/// 这样批渲染管线两端的颜色空间语义天然对齐，不需要额外转换。
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// 离屏画面在窗口中的摆放方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// 整数缩放倍率，恒 ≥ 1。
    pub scale: u32,
    /// 居中后的左侧黑边宽度。
    pub offset_x: u32,
    /// 居中后的上方黑边高度。
    pub offset_y: u32,
}

/// 算出给定窗口尺寸下的最大整数缩放与居中偏移。
///
/// **只允许整数倍**：非整数倍缩放会让相邻像素被放大成宽窄不一的方块，
/// 是像素美术最刺眼的瑕疵。宁可在四周留黑边，也不拉伸。
///
/// 倍率取宽高两个方向的较小者，否则画面会被窗口切掉。窗口小于逻辑
/// 分辨率时仍取 1 倍——零倍会让画面完全消失，比裁切更糟。
pub fn fit_viewport(window_width: u32, window_height: u32) -> Viewport {
    let scale = (window_width / LOGICAL_WIDTH)
        .min(window_height / LOGICAL_HEIGHT)
        .max(1);

    let drawn_width = LOGICAL_WIDTH * scale;
    let drawn_height = LOGICAL_HEIGHT * scale;

    Viewport {
        scale,
        // saturating_sub：窗口比画面还小时偏移取 0，而不是回绕成天文数字。
        offset_x: window_width.saturating_sub(drawn_width) / 2,
        offset_y: window_height.saturating_sub(drawn_height) / 2,
    }
}

/// 固定 640×360 的离屏渲染目标及其放大 blit 管线。
///
/// 离屏纹理固定用 [`TARGET_FORMAT`]（与窗口 surface 格式无关，理由见
/// 该常量文档）；blit 管线把它采样后写进的 color target 则必须用
/// [`GpuContext::surface_format`]——那才是最终提交给合成器的那张
/// 纹理的真实格式，这是两件不同的事，不能共用一个变量表示。
pub struct RenderTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    format: wgpu::TextureFormat,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl RenderTarget {
    /// 创建离屏纹理与放大 blit 所需的全部 GPU 资源。
    pub fn new(gpu: &GpuContext) -> RenderTarget {
        let format = TARGET_FORMAT;

        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("ll-render offscreen target"),
            size: wgpu::Extent3d {
                width: LOGICAL_WIDTH,
                height: LOGICAL_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            // RENDER_ATTACHMENT：场景绘制的目的地；TEXTURE_BINDING：blit
            // 时作为采样源；COPY_SRC：read_pixels 这个视觉回归钩子要把它
            // 拷进可读缓冲区。
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = gpu.device().create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ll-render blit sampler"),
            // 最近邻：线性插值会把整数倍放大后的像素边缘糊掉，是像素美术
            // 最刺眼的瑕疵，必须避免。
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let shader = gpu
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ll-render blit shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shader/blit.wgsl").into()),
            });

        let bind_group_layout =
            gpu.device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("ll-render blit bind group layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            // NonFiltering：不只是声明，还是约束——这个绑定槽
                            // 位从类型层面拒绝任何线性过滤采样器，避免将来
                            // 有人手滑把 sampler 换成线性插值。
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                            count: None,
                        },
                    ],
                });

        let pipeline_layout =
            gpu.device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("ll-render blit pipeline layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    // blit 着色器不用立即数（wgpu 30 把 push constant 改名为
                    // immediate data），无需分配。
                    immediate_size: 0,
                });

        let pipeline = gpu
            .device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ll-render blit pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                // 默认拓扑即三角形列表，与全屏三角形 trick 的三个顶点匹配。
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    // 必须用 gpu.surface_format()，不是离屏纹理的 `format`：
                    // 这个管线画的目的地是 blit_to 传入的窗口 surface 纹理，
                    // 其真实格式恒等于 gpu.surface_format()，与离屏纹理固定
                    // 的 TARGET_FORMAT 是两件独立的事。
                    targets: &[Some(wgpu::ColorTargetState {
                        format: gpu.surface_format(),
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });

        let bind_group = gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ll-render blit bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        RenderTarget {
            texture,
            view,
            format,
            bind_group,
            pipeline,
        }
    }

    /// 离屏纹理的视图，场景批渲染以此为绘制目标。
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// 离屏纹理的像素格式，恒为 [`TARGET_FORMAT`]。
    ///
    /// 场景批渲染（[`crate::batch::SpriteBatch::new`]）的管线要画进这张
    /// 离屏纹理，其 color target 格式必须与它一致——调用方应传这个
    /// 访问器的返回值，而不是 `GpuContext::surface_format()`（那是窗口
    /// surface 的格式，两者不再保证相等，见模块文档）。
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// 把离屏画面按 `viewport` 整数倍放大 blit 到 `destination`。
    ///
    /// 先把整个目标清成黑色再限定视口/裁剪矩形绘制：非整数倍缩放时四周
    /// 会留黑边，这一步就是黑边的来源，而不是靠额外的清屏调用。
    pub fn blit_to(&self, gpu: &GpuContext, destination: &wgpu::TextureView, viewport: Viewport) {
        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ll-render blit encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ll-render blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            let drawn_width = (LOGICAL_WIDTH * viewport.scale) as f32;
            let drawn_height = (LOGICAL_HEIGHT * viewport.scale) as f32;
            pass.set_viewport(
                viewport.offset_x as f32,
                viewport.offset_y as f32,
                drawn_width,
                drawn_height,
                0.0,
                1.0,
            );
            // 视口变换本身会把图元裁剪到这个矩形，scissor 再加一道保险：
            // 某些后端对超出视口的图元只做变换不做硬裁剪。
            pass.set_scissor_rect(
                viewport.offset_x,
                viewport.offset_y,
                LOGICAL_WIDTH * viewport.scale,
                LOGICAL_HEIGHT * viewport.scale,
            );
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            // 全屏三角形：三个顶点即可覆盖视口，见 blit.wgsl 里的推导。
            pass.draw(0..3, 0..1);
        }

        gpu.queue().submit(core::iter::once(encoder.finish()));
    }

    /// 把离屏纹理的像素读回 CPU 内存，按行剔除对齐填充，返回紧凑的 RGBA 字节。
    ///
    /// 这是视觉回归测试的钩子，**不是调试功能**——测试用它把渲染结果与
    /// 冻结的基准 PNG 逐像素比对，而基准比对必须在离屏坐标系里做，不然
    /// 结果会随运行环境的窗口分辨率变化。
    pub fn read_pixels(&self, gpu: &GpuContext) -> Vec<u8> {
        let bytes_per_pixel = self
            .format
            .block_copy_size(None)
            .expect("离屏目标用的是未压缩颜色格式，block size 恒有值");
        let unpadded_bytes_per_row = LOGICAL_WIDTH * bytes_per_pixel;
        // wgpu 要求拷贝目的缓冲区每行按 COPY_BYTES_PER_ROW_ALIGNMENT 对齐，
        // 读回后要把这份填充裁掉，调用方才能拿到纯粹的像素数据。
        let padded_bytes_per_row = unpadded_bytes_per_row
            .div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

        let buffer_size = (padded_bytes_per_row * LOGICAL_HEIGHT) as wgpu::BufferAddress;
        let readback_buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("ll-render readback buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ll-render readback encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(LOGICAL_HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: LOGICAL_WIDTH,
                height: LOGICAL_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue().submit(core::iter::once(encoder.finish()));

        let slice = readback_buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| {
            result.expect("视觉回归测试的读回缓冲区映射失败");
        });
        // 阻塞轮询：视觉回归测试要的是确定性结果而非帧内低延迟，等到映射
        // 完成再继续，比注册异步回调后再取值简单得多。
        gpu.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("等待 GPU 完成读回失败");

        let padded = slice
            .get_mapped_range()
            .expect("缓冲区已映射完成，取范围不应失败");
        let mut pixels = Vec::with_capacity((unpadded_bytes_per_row * LOGICAL_HEIGHT) as usize);
        for row in 0..LOGICAL_HEIGHT as usize {
            let start = row * padded_bytes_per_row as usize;
            let end = start + unpadded_bytes_per_row as usize;
            pixels.extend_from_slice(&padded[start..end]);
        }
        drop(padded);
        readback_buffer.unmap();

        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 窗口恰为二倍时取二倍缩放() {
        // Arrange & Act
        let viewport = fit_viewport(1280, 720);

        // Assert
        assert_eq!(viewport.scale, 2);
    }

    #[test]
    fn 非整数倍时向下取整而非拉伸() {
        // 非整数倍缩放会让相邻像素被放大成宽窄不一的方块，是像素美术
        // 最刺眼的瑕疵。宁可留黑边。
        // Arrange & Act
        let viewport = fit_viewport(1500, 800);

        // Assert
        assert_eq!(viewport.scale, 2);
    }

    #[test]
    fn 非整数倍时画面居中留黑边() {
        // Arrange & Act
        let viewport = fit_viewport(1500, 800);

        // Assert
        assert_eq!(viewport.offset_x, (1500 - LOGICAL_WIDTH * 2) / 2);
    }

    #[test]
    fn 窗口小于逻辑分辨率时仍取一倍() {
        // 零倍会让画面完全消失，比裁切更糟。
        // Arrange & Act
        let viewport = fit_viewport(320, 180);

        // Assert
        assert_eq!(viewport.scale, 1);
    }

    #[test]
    fn 缩放倍率受限于较短的一边() {
        // 宽度够 4 倍但高度只够 2 倍时必须取 2 倍，否则画面会被切掉。
        // Arrange & Act
        let viewport = fit_viewport(2560, 720);

        // Assert
        assert_eq!(viewport.scale, 2);
    }
}
