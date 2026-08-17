//! 精灵批渲染：把同一图集的多个精灵合并为一次绘制调用。
//!
//! # 一层一次 draw call
//!
//! 顶点缓冲只放一个单位四边形，每个精灵作为一个 **instance** 提交给
//! GPU，一批精灵一次 `draw` 画完，而不是逐精灵各发一次绘制调用。
//! 规格决策 15 要求支撑大量实体，逐精灵一次 draw call 在几百个实体时
//! 就会撞墙。[`SpriteBatch::flush`] 在提交前把全部待绘制精灵按
//! [`DrawOrder`] 排好序，一次绘制里各实例仍按正确的图层/纵坐标顺序
//! 光栅化，因此这一次调用天然满足「每层至多一次 draw call」——甚至
//! 更省，是整帧一次。
//!
//! # 实例缓冲成倍扩容
//!
//! 实例缓冲容量不足时成倍扩容并重建，而不是每帧按需精确分配：精确
//! 分配会在实体数量小幅波动时造成持续的缓冲重建，显存与 CPU 双双
//! 受损。扩容策略提成自由函数 [`grow_capacity`]，不依赖 GPU，可以
//! 单测覆盖。

use crate::atlas::Atlas;
use crate::gpu::GpuContext;
use crate::sprite::DrawOrder;
use bytemuck::{Pod, Zeroable};

/// 起始实例缓冲容量。
///
/// 任取一个够用大多数场景、又不至于初始就浪费太多显存的值；不足时
/// 由 [`grow_capacity`] 接管后续扩容，这个数字本身不需要精确。
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// 单位四边形的顶点，只携带 `[0,1]` 范围内的局部坐标。
///
/// 真正的屏幕位置与尺寸来自逐实例数据（见 [`SpriteInstance`]），顶点
/// 缓冲本身在整个 `SpriteBatch` 生命周期里恒定不变，因此只在
/// [`SpriteBatch::new`] 时上传一次。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    unit_pos: [f32; 2],
}

/// 两个三角形拼成的单位正方形，覆盖 `[0,1]×[0,1]`。
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

const QUAD_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];

const QUAD_VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: core::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &QUAD_ATTRIBUTES,
};

/// 一个精灵实例：屏幕位置、尺寸、UV 矩形、颜色调制。
///
/// `#[repr(C)]` + [`Pod`] + [`Zeroable`]：这份数据要按字节原样搬进 GPU
/// 实例缓冲。GPU 端按固定偏移读取各字段，结构体若含填充或字段重排，
/// 着色器读到的就是垃圾数据，而现象是画面错乱而不是编译或运行期
/// 报错，极难定位——[`tests::实例结构体大小符合着色器预期`] 就是为了
/// 在这类问题进入渲染流水线之前用一个断言挡住它。
///
/// 各字段均已由调用方（游戏侧渲染系统）按 [`crate::sprite::Pivot`]、
/// [`crate::camera::Camera`] 换算成离屏渲染目标（见 [`crate::target`]）
/// 的像素坐标；`SpriteBatch` 本身不做任何世界坐标换算。
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SpriteInstance {
    /// 精灵左上角在离屏目标像素空间中的位置。
    pub position: [f32; 2],
    /// 精灵在离屏目标像素空间中的尺寸。
    pub size: [f32; 2],
    /// 精灵在图集纹理里的采样矩形，归一化到 `[0,1]`：`(u, v, 宽, 高)`。
    pub uv_rect: [f32; 4],
    /// 逐精灵颜色调制（RGBA），逐分量乘到采样结果上，用于受击变色、
    /// 渐隐等效果。恒定不调制时传 `[1.0; 4]`。
    pub color: [f32; 4],
}

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32x4,
    4 => Float32x4,
];

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: core::mem::size_of::<SpriteInstance>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Instance,
    attributes: &INSTANCE_ATTRIBUTES,
};

/// 算出容纳 `needed` 个实例所需的新容量。
///
/// 成倍增长而非按需精确分配：每帧精确分配会在实体数量小幅波动时造成
/// 持续的缓冲重建，显存与 CPU 双双受损。
fn grow_capacity(current: usize, needed: usize) -> usize {
    let mut capacity = current.max(1);
    while capacity < needed {
        capacity *= 2;
    }
    capacity
}

/// 一批共享同一图集的精灵的批渲染器。
///
/// 每帧调用方通过 [`Self::push`] 收集本帧要画的精灵，调用
/// [`Self::flush`] 一次性提交。`flush` 只负责把已经算好的屏幕坐标搬上
/// GPU 并绘制，不含任何世界逻辑（拾取、碰撞、AI 等一概不在这里）。
pub struct SpriteBatch {
    vertex_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    pending: Vec<(DrawOrder, SpriteInstance)>,
}

impl SpriteBatch {
    /// 创建批渲染器：编译着色器、建立管线与绑定组、上传常驻的单位
    /// 四边形顶点缓冲，并预分配一份起始容量的实例缓冲。
    pub fn new(gpu: &GpuContext, atlas: &Atlas, format: wgpu::TextureFormat) -> SpriteBatch {
        let vertex_buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("ll-render sprite quad vertex buffer"),
            size: (QUAD_VERTICES.len() * core::mem::size_of::<QuadVertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue()
            .write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&QUAD_VERTICES));

        let instance_buffer = create_instance_buffer(gpu, INITIAL_INSTANCE_CAPACITY);

        let bind_group_layout =
            gpu.device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("ll-render sprite bind group layout"),
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
                            // NonFiltering：图集采样器固定最近邻（见
                            // atlas.rs），这里从类型层面拒绝线性过滤，
                            // 避免将来有人手滑把 sampler 换成线性插值,
                            // 糊掉像素美术的硬边缘。
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                            count: None,
                        },
                    ],
                });

        let bind_group = gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ll-render sprite bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(atlas.texture_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(atlas.sampler()),
                },
            ],
        });

        let shader = gpu
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ll-render sprite shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shader/sprite.wgsl").into()),
            });

        let pipeline_layout =
            gpu.device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("ll-render sprite pipeline layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    // 逻辑分辨率在着色器里写死为常量（见 sprite.wgsl 顶部
                    // 说明），不需要 immediate data。
                    immediate_size: 0,
                });

        let pipeline = gpu
            .device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ll-render sprite pipeline"),
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
                        // 图集贴图是直通（非预乘）alpha 的 PNG，标准
                        // alpha 混合让半透明像素与已画内容正确叠加。
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });

        SpriteBatch {
            vertex_buffer,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            bind_group,
            pipeline,
            pending: Vec::new(),
        }
    }

    /// 把一个精灵加入本帧待绘制队列。
    ///
    /// 只是收集，不立即触碰 GPU——真正的排序、扩容与提交都推迟到
    /// [`Self::flush`]，这样同一帧里的多次 `push` 不会有任何 GPU 开销。
    pub fn push(&mut self, order: DrawOrder, instance: SpriteInstance) {
        self.pending.push((order, instance));
    }

    /// 把本帧收集到的全部精灵排序、上传、绘制到 `target`，然后清空队列。
    ///
    /// 排序用 `sort_unstable_by_key`：[`DrawOrder`] 已含实体号作最终
    /// 平局打破键，本身就是全序，不需要稳定排序保留相等元素相对顺序
    /// 的额外开销。
    pub fn flush(&mut self, gpu: &GpuContext, target: &wgpu::TextureView) {
        self.pending.sort_unstable_by_key(|(order, _)| *order);

        let instances: Vec<SpriteInstance> =
            self.pending.iter().map(|(_, instance)| *instance).collect();
        self.ensure_capacity(gpu, instances.len());
        if !instances.is_empty() {
            gpu.queue()
                .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        }

        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ll-render sprite batch encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ll-render sprite batch pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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

            if !instances.is_empty() {
                // 一次绘制画完整批：实例已按 DrawOrder 排好序，光栅化
                // 顺序即绘制提交顺序，图层/纵坐标的前后遮挡关系在这一次
                // draw call 内就是正确的，不需要按图层拆成多次调用。
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                pass.draw(0..QUAD_VERTICES.len() as u32, 0..instances.len() as u32);
            }
        }

        gpu.queue().submit(core::iter::once(encoder.finish()));
        self.pending.clear();
    }

    /// 若待绘制实例数超出当前容量，成倍扩容并重建实例缓冲。
    fn ensure_capacity(&mut self, gpu: &GpuContext, needed: usize) {
        if needed <= self.instance_capacity {
            return;
        }
        let new_capacity = grow_capacity(self.instance_capacity, needed);
        self.instance_buffer = create_instance_buffer(gpu, new_capacity);
        self.instance_capacity = new_capacity;
    }
}

/// 创建一份能容纳 `capacity` 个 [`SpriteInstance`] 的实例缓冲。
fn create_instance_buffer(gpu: &GpuContext, capacity: usize) -> wgpu::Buffer {
    gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("ll-render sprite instance buffer"),
        size: (capacity * core::mem::size_of::<SpriteInstance>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite::{DrawOrder, Layer};

    #[test]
    fn 实例结构体大小符合着色器预期() {
        // GPU 缓冲要求确定的内存布局。结构体若含填充或字段重排，着色器
        // 读到的就是垃圾数据，而现象是画面错乱而非报错，极难定位。
        // Arrange & Act
        let size = core::mem::size_of::<SpriteInstance>();

        // Assert：位置尺寸 4×f32 + UV 4×f32 + 颜色 4×f32 = 48 字节
        assert_eq!(size, 48);
    }

    #[test]
    // 简报给定的测试代码字面量用 vec!，clippy 认为固定长度场景该用数组；
    // 保留简报原文，仅局部放行这条建议。
    #[allow(clippy::useless_vec)]
    fn 排序后按绘制顺序键升序排列() {
        // Arrange
        let mut items = vec![
            DrawOrder::new(Layer::ENTITY, 100, 1),
            DrawOrder::new(Layer::TERRAIN, 50, 2),
            DrawOrder::new(Layer::ENTITY, 50, 3),
        ];

        // Act
        items.sort_unstable();

        // Assert
        assert_eq!(items[0], DrawOrder::new(Layer::TERRAIN, 50, 2));
    }

    #[test]
    fn 扩容后容量至少翻倍() {
        // 每帧重新分配会在实体数量波动时造成持续的显存搅动。
        // Arrange
        let current = 256usize;

        // Act
        let grown = grow_capacity(current, 300);

        // Assert
        assert!(grown >= 512);
    }
}
