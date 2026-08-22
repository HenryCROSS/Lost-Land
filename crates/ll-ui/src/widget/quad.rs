//! 纯色矩形：面板背景、边框、条形共用的最小 GPU 图元。
//!
//! # 为什么不复用 `ll-render` 的 `SpriteBatch`
//!
//! `SpriteBatch`（`ll_render::batch`）画的是 640×360 **逻辑分辨率**的
//! 世界层像素美术，着色器把这个分辨率写死成 WGSL 常量（见
//! `crates/ll-render/src/shader/sprite.wgsl` 顶部说明），随窗口整数倍
//! 放大后再 `blit_to` 到窗口 surface。`ll-text` 的
//! [`ll_text::TextRenderer`] 刻意**不走这条管线**——直接对窗口 surface
//! 的原生像素分辨率画字形（见 `ll_text` crate 顶层文档「两条渲染通道」
//! 一节），因为糊字号在整数放大后只会被等比例放大成更明显的糊。
//!
//! HUD 面板背景与本模块的填色矩形要与文本对齐在同一套坐标系里（同一
//! 块面板的边框与它里面的文字必须使用同一份原生像素坐标，否则窗口
//! 缩放/letterbox 时两者会跑偏），因此本模块与 `TextRenderer` 同属
//! 「blit 之后、原生分辨率、不清屏」的第二条通道，不接入 `SpriteBatch`
//! 的图集/UV/640×360 定宽管线——两者服务的是完全不同的坐标空间，硬塞
//! 进同一条管线只会制造需要来回换算的复杂度，换不来任何收益。
//!
//! # 即时模式：每帧全量重建，不保留任何跨帧实例
//!
//! 与 [`ll_render::batch::SpriteBatch::push`]/[`ll_text::TextRenderer::render`]
//! 同一条既有约定：调用方每帧把「本帧要画的全部矩形」整理成一个切片
//! 传进 [`QuadRenderer::render`]，本类型不缓存上一帧的实例、不做增量
//! 更新——`ll-render`/`ll-text` 两条既有管线都是这个模型（见任务书
//! 「即时模式还是保留模式」的核实结论），HUD 这一层没有理由另立一套
//! 保留模式的实例生命周期管理。

use bytemuck::{Pod, Zeroable};
use ll_render::wgpu;

/// 一个待绘制的填色矩形：位置、尺寸、颜色，均为窗口原生像素坐标——
/// 与 [`ll_text::TextRun`] 同一套坐标系，见模块文档。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct QuadInstance {
    /// 左上角像素坐标。
    pub position: [f32; 2],
    /// 像素尺寸。
    pub size: [f32; 2],
    /// 颜色（RGBA，`0.0..=1.0`）。
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    unit_pos: [f32; 2],
}

const QUAD_VERTICES: [QuadVertex; 6] = [
    QuadVertex {
        unit_pos: [0.0, 0.0],
    },
    QuadVertex {
        unit_pos: [1.0, 0.0],
    },
    QuadVertex {
        unit_pos: [0.0, 1.0],
    },
    QuadVertex {
        unit_pos: [0.0, 1.0],
    },
    QuadVertex {
        unit_pos: [1.0, 0.0],
    },
    QuadVertex {
        unit_pos: [1.0, 1.0],
    },
];

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ResolutionUniform {
    size: [f32; 2],
}

const INITIAL_INSTANCE_CAPACITY: usize = 64;

const QUAD_VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
const QUAD_VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: core::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &QUAD_VERTEX_ATTRIBUTES,
};

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32x4,
];
const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: core::mem::size_of::<QuadInstance>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Instance,
    attributes: &INSTANCE_ATTRIBUTES,
};

fn grow_capacity(current: usize, needed: usize) -> usize {
    let mut capacity = current.max(1);
    while capacity < needed {
        capacity *= 2;
    }
    capacity
}

/// 持有本模块 GPU 资源的渲染器：管线、常驻顶点缓冲、可扩容的实例
/// 缓冲、分辨率 uniform——结构与 [`ll_text::TextRenderer`]/
/// [`ll_render::batch::SpriteBatch`] 同一个形状（构造时建管线，
/// `render` 时把当前分辨率与实例数据搬上 GPU 并提交）。
pub struct QuadRenderer {
    vertex_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    resolution_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl QuadRenderer {
    /// 建立管线、上传常驻单位四边形顶点缓冲、预分配一份起始容量的
    /// 实例缓冲——与 [`ll_text::TextRenderer::new`] 同一种「接收原语
    /// 而非 `GpuContext`」的取舍：本类型的 GPU 资源本身不需要 surface，
    /// 只需要设备/队列/目的纹理格式，headless 测试与截图证据都要求能
    /// 在没有真实窗口的环境下构造它。
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> QuadRenderer {
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ll-ui quad vertex buffer"),
            size: (QUAD_VERTICES.len() * core::mem::size_of::<QuadVertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&QUAD_VERTICES));

        let instance_buffer = create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY);

        let resolution_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ll-ui quad resolution uniform"),
            size: core::mem::size_of::<ResolutionUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ll-ui quad bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ll-ui quad bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: resolution_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ll-ui quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shader/quad.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ll-ui quad pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ll-ui quad pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(QUAD_VERTEX_LAYOUT), Some(INSTANCE_LAYOUT)],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // 面板半透明背景需要与已画内容（世界层 + 之前画过的
                    // 矩形）正确叠加，标准 alpha 混合。
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        QuadRenderer {
            vertex_buffer,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            resolution_buffer,
            bind_group,
            pipeline,
        }
    }

    fn ensure_capacity(&mut self, device: &wgpu::Device, needed: usize) {
        if needed <= self.instance_capacity {
            return;
        }
        let new_capacity = grow_capacity(self.instance_capacity, needed);
        self.instance_buffer = create_instance_buffer(device, new_capacity);
        self.instance_capacity = new_capacity;
    }

    /// 把 `quads` 画进 `target`——**不清屏**（`LoadOp::Load`），与
    /// [`ll_text::TextRenderer::render`] 同一条既有纪律：本函数只在
    /// 已经画好的内容上追加一层，调用方负责先用
    /// `RenderTarget::blit_to` 画好世界层。
    ///
    /// `resolution_width`/`resolution_height` 必须是 `target` 的真实
    /// 原生像素尺寸，理由与 [`ll_text::TextRenderer::render`] 完全一致
    /// （见其文档）。
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        resolution_width: u32,
        resolution_height: u32,
        quads: &[QuadInstance],
    ) {
        queue.write_buffer(
            &self.resolution_buffer,
            0,
            bytemuck::bytes_of(&ResolutionUniform {
                size: [resolution_width as f32, resolution_height as f32],
            }),
        );

        self.ensure_capacity(device, quads.len());
        if !quads.is_empty() {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(quads));
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ll-ui quad render encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ll-ui quad render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if !quads.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                pass.draw(0..QUAD_VERTICES.len() as u32, 0..quads.len() as u32);
            }
        }
        queue.submit(core::iter::once(encoder.finish()));
    }
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ll-ui quad instance buffer"),
        size: (capacity * core::mem::size_of::<QuadInstance>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grow_capacity在需求超出当前容量时按倍数增长到刚好够用() {
        // Arrange & Act
        let grown = grow_capacity(64, 100);

        // Assert
        assert_eq!(grown, 128);
    }

    #[test]
    fn grow_capacity在需求未超出当前容量时原样返回() {
        // Arrange & Act
        let grown = grow_capacity(64, 10);

        // Assert
        assert_eq!(grown, 64);
    }

    #[test]
    fn quad实例结构体大小符合着色器顶点属性预期() {
        // Arrange & Act：position(8) + size(8) + color(16) = 32 字节，
        // 与 shader/quad.wgsl 里 @location(1..3) 的字段布局一一对应——
        // 与 `ll_render::batch` 同名测试同一条防御目的。
        let size = core::mem::size_of::<QuadInstance>();

        // Assert
        assert_eq!(size, 32);
    }
}
