//! 离屏渲染目标：固定 640×360 的逻辑分辨率纹理及其整数倍放大 blit。
//!
//! 场景先画到这张固定尺寸的纹理，再按 [`fit_viewport`] 算出的整数倍放大
//! blit 到窗口 surface。像素完美因此与窗口尺寸彻底解耦，[`RenderTarget::read_pixels`]
//! 也能把离屏画面冻结成基准 PNG，不受运行环境分辨率影响。

use crate::gpu::GpuContext;
use bytemuck::{Pod, Zeroable};

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// 连续缩放倍率，恒 ≥ 1.0——不再钉死整数档，见 [`fit_viewport`]
    /// 文档「为什么不再只允许整数倍」一节。
    pub scale: f32,
    /// 居中后的左侧黑边宽度。
    pub offset_x: u32,
    /// 居中后的上方黑边高度。
    pub offset_y: u32,
    /// 缩放后实际绘制内容的宽度（像素）。与 `offset_x` 一起预先算好、
    /// 存进结构体而非留给调用方各自用 `scale` 重新推导——[`RenderTarget::blit_to`]
    /// 与本模块的居中偏移计算必须用同一个取整结果，否则两处各自把
    /// `scale` 转成像素时的浮点取整方向若不一致，画面内容与黑边之间
    /// 会露出一条不属于任何一方的缝隙。
    pub drawn_width: u32,
    /// 缩放后实际绘制内容的高度（像素），理由同 `drawn_width`。
    pub drawn_height: u32,
}

/// 算出给定窗口尺寸下的连续缩放与居中偏移。
///
/// # 为什么不再只允许整数倍
///
/// 此前这里只允许整数倍缩放：非整数倍缩放配合最近邻采样会让相邻像素
/// 被放大成宽窄不一的方块，是像素美术最刺眼的瑕疵，宁可留黑边也不
/// 拉伸。项目所有者现在明确要「按比例平滑缩放」（配合
/// [`crate::camera::Zoom`] 与用户可选的
/// [`BlitFilter::SharpBilinear`]）——见 `blit.wgsl` 里锐利双线性采样
/// 的实现：只要采样端配合，非整数倍缩放不再必然产生像素不均的瑕疵，
/// 继续把倍率钉死在整数上就成了一条不再有存在理由的限制。
///
/// 倍率取宽高两个方向的较小者，否则画面会被窗口切掉。窗口小于逻辑
/// 分辨率时仍取 1 倍——零倍或小于一倍会让画面比窗口还小，与其看着
/// 更小的画面泡在更大的黑边里，不如保持「至少铺满一个方向」的既有
/// 承诺。
pub fn fit_viewport(window_width: u32, window_height: u32) -> Viewport {
    let scale = (window_width as f32 / LOGICAL_WIDTH as f32)
        .min(window_height as f32 / LOGICAL_HEIGHT as f32)
        .max(1.0);

    // floor 而非 round：绘制内容的像素尺寸绝不能超过窗口本身，否则
    // 下面 saturating_sub 算出的黑边宽度会因为「绘制内容」比窗口还大
    // 而在数学上说不通（虽然 saturating_sub 不会 panic，但语义已经
    // 错了——黑边应该恒为非负的「多出来的空间」，不是被摄断的差值）。
    let drawn_width = (LOGICAL_WIDTH as f32 * scale).floor() as u32;
    let drawn_height = (LOGICAL_HEIGHT as f32 * scale).floor() as u32;

    Viewport {
        scale,
        // saturating_sub：窗口比画面还小时偏移取 0，而不是回绕成天文数字。
        offset_x: window_width.saturating_sub(drawn_width) / 2,
        offset_y: window_height.saturating_sub(drawn_height) / 2,
        drawn_width,
        drawn_height,
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
    /// 逐帧改写的 blit 参数（缩放倍率、是否启用锐利双线性），两个
    /// bind group 共享同一块缓冲——`blit_to` 每次调用都会先写新值再
    /// 选择其中一个 bind group 绘制，两个 bind group 引用同一块缓冲
    /// 完全没问题：GPU 端读到的永远是这次调用刚写入的最新内容。
    params_buffer: wgpu::Buffer,
    /// [`BlitFilter::Nearest`] 对应的 bind group：采样器最近邻。
    bind_group_nearest: wgpu::BindGroup,
    /// [`BlitFilter::SharpBilinear`] 对应的 bind group：采样器线性
    /// 过滤，配合 `blit.wgsl` 里的手动 UV 重映射实现「纹素边界平滑、
    /// 纹素内部平坦」的锐利双线性效果。
    bind_group_sharp_bilinear: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

/// 离屏画面放大到窗口时使用的采样滤波方式。
///
/// # 为什么不是 MSAA
///
/// 传统多重采样抗锯齿（MSAA）平滑的是三角形边缘的锯齿——对这个项目
/// 不适用：`blit.wgsl` 画的是一个铺满视口的全屏三角形，边缘裁在视口
/// 之外，不存在需要抗锯齿的可见几何边缘；真正决定「画面看起来锯不
/// 锯齿」的是离屏画布本身的像素怎么被放大取样，这正是本枚举要解决
/// 的问题，与三角形光栅化无关。像素游戏的硬边缘是刻意画出来的美术
/// 语言，MSAA 会把它们和传统抗锯齿一样糊掉——这不是这类游戏需要的
/// 「抗锯齿」，真正对应的旋钮就是这里的采样方式选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlitFilter {
    /// 最近邻——像素边缘恒定锐利，但 [`fit_viewport`] 返回非整数倍
    /// `scale` 时会让相邻像素被放大成宽窄不一的方块、画面轻微抖动
    /// 闪烁，这是这类采样在非整数倍缩放下的经典瑕疵。
    #[default]
    Nearest,
    /// 锐利双线性（sharp bilinear）——只在纹素边界上做平滑过渡，纹素
    /// 内部保持平坦：任意倍率下像素边缘依然锐利，同时消除 `Nearest`
    /// 在非整数倍下的不均匀瑕疵。算法是公开发表的
    /// sharp-bilinear-simple 着色器（libretro shaders 项目，公有
    /// 领域），实现见 `shader/blit.wgsl`。
    SharpBilinear,
}

/// 逐帧写进 GPU 的 blit 参数。
///
/// `#[repr(C)]` + `Pod` + `Zeroable`：与 `batch.rs` 的 `SpriteInstance`
/// 同一理由，这份数据要按字节原样搬进 uniform 缓冲。
///
/// 两个 `f32` 字段之后补两个 `f32` 填充：WGSL 的 uniform 地址空间要求
/// 结构体大小是最大成员对齐（这里是 4 字节）的整数倍，8 字节本身已经
/// 满足，补齐到 16 字节是更保守的通行做法，避免某些驱动对 uniform
/// 结构体大小有额外的对齐期待。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BlitParams {
    /// 与 [`Viewport::scale`] 相同的连续缩放倍率——`blit.wgsl` 用它
    /// 算出锐利双线性平滑过渡区的宽度。
    scale: f32,
    /// `0.0` = 最近邻直通采样，`1.0` = 启用锐利双线性重映射。用 `f32`
    /// 而非 `bool`：WGSL uniform 块里没有可移植的布尔标量表示，复用
    /// 已经需要的浮点字段类型，省一次类型转换。
    sharp_bilinear: f32,
    _padding: [f32; 2],
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

        // 两个采样器分别服务 BlitFilter 的两个变体：Nearest 直通采样时
        // 用最近邻采样器；SharpBilinear 需要 GPU 硬件的线性插值作为
        // 「纹素边界平滑过渡」这一步的底层实现，`blit.wgsl` 在采样前
        // 先把 UV 重映射到「纹素内部保持平坦」的坐标，两步配合才是完整
        // 的锐利双线性算法（单独一个线性采样器不做 UV 重映射的话，就是
        // 会糊掉像素边缘的普通线性插值，达不到「边缘锐利」的要求）。
        let nearest_sampler = gpu.device().create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ll-render blit sampler (nearest)"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let linear_sampler = gpu.device().create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ll-render blit sampler (linear, sharp-bilinear base)"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
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
                            // Filtering（不再是 NonFiltering）：这个绑定槽位
                            // 现在要能同时容纳最近邻与线性两种采样器——
                            // BlitFilter::Nearest 绑最近邻、SharpBilinear
                            // 绑线性，两个 bind group 共用同一份管线与
                            // 布局。wgpu 的校验规则是 Filtering 允许绑定
                            // 任意 FilterMode 的采样器，NonFiltering 严格
                            // 只允许 Nearest；放宽到 Filtering 不会让
                            // Nearest 分支意外变得可被线性化——bind_group_nearest
                            // 绑的仍然是最近邻采样器，着色器该分支也仍是
                            // 直通采样，行为逐位不变。
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        let params_buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("ll-render blit params buffer"),
            size: core::mem::size_of::<BlitParams>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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

        let make_bind_group = |label: &str, sampler: &wgpu::Sampler| {
            gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let bind_group_nearest =
            make_bind_group("ll-render blit bind group (nearest)", &nearest_sampler);
        let bind_group_sharp_bilinear = make_bind_group(
            "ll-render blit bind group (sharp bilinear)",
            &linear_sampler,
        );

        RenderTarget {
            texture,
            view,
            format,
            params_buffer,
            bind_group_nearest,
            bind_group_sharp_bilinear,
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

    /// 把离屏画面按 `viewport` 放大 blit 到 `destination`，`filter` 选择
    /// 采样方式（见 [`BlitFilter`]）。
    ///
    /// 先把整个目标清成黑色再限定视口/裁剪矩形绘制：非整数倍缩放时四周
    /// 会留黑边，这一步就是黑边的来源，而不是靠额外的清屏调用。
    pub fn blit_to(
        &self,
        gpu: &GpuContext,
        destination: &wgpu::TextureView,
        viewport: Viewport,
        filter: BlitFilter,
    ) {
        let params = BlitParams {
            scale: viewport.scale,
            sharp_bilinear: match filter {
                BlitFilter::Nearest => 0.0,
                BlitFilter::SharpBilinear => 1.0,
            },
            _padding: [0.0; 2],
        };
        gpu.queue()
            .write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        let bind_group = match filter {
            BlitFilter::Nearest => &self.bind_group_nearest,
            BlitFilter::SharpBilinear => &self.bind_group_sharp_bilinear,
        };

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

            pass.set_viewport(
                viewport.offset_x as f32,
                viewport.offset_y as f32,
                viewport.drawn_width as f32,
                viewport.drawn_height as f32,
                0.0,
                1.0,
            );
            // 视口变换本身会把图元裁剪到这个矩形，scissor 再加一道保险：
            // 某些后端对超出视口的图元只做变换不做硬裁剪。
            pass.set_scissor_rect(
                viewport.offset_x,
                viewport.offset_y,
                viewport.drawn_width,
                viewport.drawn_height,
            );
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
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
        assert_eq!(viewport.scale, 2.0);
    }

    #[test]
    fn 非整数倍窗口尺寸产出连续缩放倍率() {
        // 此前这里强制向下取整到 2 倍、四周留黑边；缩放改成连续之后，
        // 1500×800 应该按较短边（高度）的比例算出一个非整数倍率，
        // 不再钉死成整数——非整数倍下的像素不均问题现在由
        // BlitFilter::SharpBilinear 负责，不再是这一层要回避的事。
        // Arrange & Act
        let viewport = fit_viewport(1500, 800);

        // Assert：800/360 < 1500/640，取较短边算出的比例。
        assert_eq!(viewport.scale, 800.0 / 360.0);
    }

    #[test]
    fn 连续缩放下画面居中留黑边() {
        // Arrange & Act
        let viewport = fit_viewport(1500, 800);

        // Assert
        assert_eq!(viewport.offset_x, (1500 - viewport.drawn_width) / 2);
    }

    #[test]
    fn 窗口小于逻辑分辨率时仍取一倍() {
        // 小于一倍会让画面比窗口还小，泡在更大的黑边里——保持「至少
        // 铺满一个方向」的既有承诺。
        // Arrange & Act
        let viewport = fit_viewport(320, 180);

        // Assert
        assert_eq!(viewport.scale, 1.0);
    }

    #[test]
    fn 缩放倍率受限于较短的一边() {
        // 宽度够 4 倍但高度只够 2 倍时必须取 2 倍，否则画面会被切掉。
        // Arrange & Act
        let viewport = fit_viewport(2560, 720);

        // Assert
        assert_eq!(viewport.scale, 2.0);
    }

    /// 从 WGSL 源码里挖出形如 `const NAME: f32 = 640.0;` 的字面量。
    ///
    /// 与 `batch.rs` 同名辅助函数逐字相同的最简解析,理由也相同——两处
    /// 各自解析各自的着色器文件,不合并成一个共享工具是因为两边都只是
    /// 测试代码,合并的收益不足以抵消跨模块暴露一个仅供测试用的私有
    /// 解析函数的成本。
    fn parse_wgsl_const_f32(source: &str, name: &str) -> Option<f32> {
        let needle = format!("const {name}");
        let after_name = &source[source.find(&needle)?..][needle.len()..];
        let after_eq = &after_name[after_name.find('=')? + 1..];
        let literal = &after_eq[..after_eq.find(';')?];
        literal.trim().parse::<f32>().ok()
    }

    #[test]
    fn 着色器中的锐利双线性纹理尺寸与rust常量保持一致() {
        // blit.wgsl 的 sharp_bilinear_uv 把逻辑分辨率写死成了 WGSL
        // 常量（与 sprite.wgsl 同一取舍,理由见 batch.rs 同名测试）,这份
        // 一致性同样光靠注释约束不住。
        // Arrange
        let source = include_str!("shader/blit.wgsl");

        // Act
        let shader_width = parse_wgsl_const_f32(source, "SOURCE_WIDTH")
            .expect("blit.wgsl 应含形如 `const SOURCE_WIDTH: f32 = 640.0;` 的常量定义");
        let shader_height = parse_wgsl_const_f32(source, "SOURCE_HEIGHT")
            .expect("blit.wgsl 应含形如 `const SOURCE_HEIGHT: f32 = 360.0;` 的常量定义");

        // Assert
        assert_eq!(shader_width, LOGICAL_WIDTH as f32);
        assert_eq!(shader_height, LOGICAL_HEIGHT as f32);
    }

    #[test]
    fn 绘制内容尺寸与缩放倍率一致() {
        // drawn_width/drawn_height 是 blit_to 与居中偏移计算共用的
        // 唯一真源，这里锁住它确实等于 scale 换算出的像素尺寸。
        // Arrange & Act
        let viewport = fit_viewport(1280, 720);

        // Assert
        assert_eq!(viewport.drawn_width, LOGICAL_WIDTH * 2);
        assert_eq!(viewport.drawn_height, LOGICAL_HEIGHT * 2);
    }
}
