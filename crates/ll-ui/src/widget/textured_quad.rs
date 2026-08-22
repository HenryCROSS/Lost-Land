//! 贴图矩形：真实九宫格边框/条形贴图（[`crate::widget::skin::NineSliceSkin`]）
//! 用的图元——[`crate::widget::quad::QuadRenderer`] 的姊妹,唯一区别是本渲染器会从
//! `ll-render` 的图集采样,而不是纯色填充。
//!
//! # 为什么是姊妹渲染器,不是给 `QuadRenderer` 加一个 `Option<UV>`
//!
//! `QuadRenderer`（[`super::quad`]）现在被 `FlatColorSkin` 消费,已经
//! 有稳定的调用点与测试。给它加纹理采样意味着：要么让纯色路径也背上
//! 一份「采样哪个纹理」的决定（引入不必要的耦合——纯色矩形从来不需要
//! 知道任何图集的存在),要么用 `Option<Atlas 引用>` 让同一个类型有两种
//! 运行模式（管线创建时的绑定组布局就不一样,`Option` 完全遮不住这个
//! 差异,只是把分支推迟到运行期,更难读)。两个独立、各自单一职责的
//! 渲染器,换来的是各自的构造函数、管线签名都不用去兼容对方的需求。
//!
//! # 与 `ll_render::batch::SpriteBatch` 的关系
//!
//! 采样的是**同一张**图集（`ll_render::atlas::Atlas`，由
//! `ll-game::app::GpuResources` 在启动时构造一次、`SpriteBatch`/本渲染
//! 器共享同一份），这正是「mod 换 UI 皮肤贴图与换世界贴图走同一条
//! 覆盖机制」这个设计成立的关键——两个渲染器都不重新加载/重新打包
//! 图集,只是各自用不同的着色器（`shader/textured_quad.wgsl` 用运行期
//! 分辨率 uniform，`sprite.wgsl` 写死 640x360)在不同的时间点（本渲染器
//! blit 之后、原生分辨率；`SpriteBatch` blit 之前、逻辑分辨率）采样它。

use bytemuck::{Pod, Zeroable};
use ll_render::atlas::Atlas;
use ll_render::wgpu;

/// 一个待绘制的贴图矩形：位置、尺寸、图集 UV 矩形、颜色调制（恒定
/// `[1.0;4]` 时不调制，直接显示采样结果）。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct TexturedQuadInstance {
    /// 左上角像素坐标。
    pub position: [f32; 2],
    /// 像素尺寸。
    pub size: [f32; 2],
    /// 图集 UV 矩形 `(u, v, 宽, 高)`，归一化到 `[0,1]`。
    pub uv_rect: [f32; 4],
    /// 颜色调制（RGBA），逐分量乘到采样结果上。
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

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32x4,
    4 => Float32x4,
];
const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: core::mem::size_of::<TexturedQuadInstance>() as wgpu::BufferAddress,
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

/// 持有本模块 GPU 资源的渲染器——形状与
/// [`crate::widget::quad::QuadRenderer`] 几乎一致,多出的只是纹理绑定
/// （图集贴图 + 采样器）,见模块文档。
pub struct TexturedQuadRenderer {
    vertex_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    resolution_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl TexturedQuadRenderer {
    /// 建立管线并绑定 `atlas` 的纹理视图/采样器——`atlas` 必须与
    /// `ll_render::batch::SpriteBatch` 使用的是同一份实例,这样贴图
    /// 内容（含 mod 覆盖）两条通道永远一致,不会出现「世界层显示的是
    /// mod 覆盖后的贴图，HUD 显示的是本体原图」这类不同步。
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        atlas: &Atlas,
    ) -> TexturedQuadRenderer {
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ll-ui textured quad vertex buffer"),
            size: (QUAD_VERTICES.len() * core::mem::size_of::<QuadVertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&QUAD_VERTICES));

        let instance_buffer = create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY);

        let resolution_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ll-ui textured quad resolution uniform"),
            size: core::mem::size_of::<ResolutionUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ll-ui textured quad bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ll-ui textured quad bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: resolution_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(atlas.texture_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(atlas.sampler()),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ll-ui textured quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shader/textured_quad.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ll-ui textured quad pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ll-ui textured quad pipeline"),
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        TexturedQuadRenderer {
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

    /// 把 `quads` 画进 `target`——**不清屏**，与
    /// [`crate::widget::quad::QuadRenderer::render`] 同一条纪律。
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        resolution_width: u32,
        resolution_height: u32,
        quads: &[TexturedQuadInstance],
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
            label: Some("ll-ui textured quad render encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ll-ui textured quad render pass"),
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
        label: Some("ll-ui textured quad instance buffer"),
        size: (capacity * core::mem::size_of::<TexturedQuadInstance>()) as wgpu::BufferAddress,
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
    fn textured_quad实例结构体大小符合着色器顶点属性预期() {
        // Arrange & Act：position(8) + size(8) + uv_rect(16) + color(16)
        // = 48 字节，与 shader/textured_quad.wgsl 里 @location(1..4) 的
        // 字段布局一一对应。
        let size = core::mem::size_of::<TexturedQuadInstance>();

        // Assert
        assert_eq!(size, 48);
    }
}
