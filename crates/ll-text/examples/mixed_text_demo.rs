//! 最小可行验证：一行中英混排文本 + 一个 Tabler 图标画到原生分辨率的
//! 离屏纹理，存 PNG 作截图证据。
//!
//! 运行：`cargo run -p ll-text --example mixed_text_demo`
//!
//! # 为什么是 headless，不建窗口
//!
//! `wgpu::Instance::request_adapter` 不传 `compatible_surface`，请求的
//! 是一个不绑定任何 surface 的适配器——这样这条最短验证路径只依赖
//! 「这台机器有没有可用的图形适配器」，不额外依赖「能不能开一个真实
//! 窗口」。图形环境（适配器）本身若不可用，`expect` 会直接 panic 并
//! 打印原因，**不会伪装成功**，这是简报的硬性要求。
//!
//! # 为什么分辨率与 640×360 无关
//!
//! 本 demo 自建一张 [`CANVAS_WIDTH`]×[`CANVAS_HEIGHT`] 的离屏纹理，
//! **不复用** `ll_render::target::RenderTarget`（那张纹理固定
//! 640×360，是世界层的画布）。这直接体现 crate 文档「两条渲染通道」
//! 一节的架构决策：文本层画的是自己的原生分辨率纹理，与世界层的
//! 逻辑分辨率毫无关系。

use std::path::PathBuf;

use ll_render::wgpu;
use ll_text::{TextRenderer, TextRun};

/// 离屏画布宽度（像素）。选一个明显宽于世界层 640 逻辑分辨率的原生
/// 尺寸，直观体现文本层走原生分辨率这条架构决策。
const CANVAS_WIDTH: u32 = 960;
/// 离屏画布高度（像素）。
const CANVAS_HEIGHT: u32 = 240;

/// 离屏纹理固定格式，取与 `ll_render::target::TARGET_FORMAT` 一致的
/// sRGB 变体——原因相同：颜色空间语义要与写入的浮点颜色对齐，否则画面
/// 会整体偏暗，见 `ll_render::target` 模块文档。
const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// 截图证据落盘路径。
const OUTPUT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/visual/mixed_text_demo.png"
);

fn main() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        // headless：不绑定 surface，这样不依赖运行环境能否开窗口。
        compatible_surface: None,
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .expect("demo 环境应能取得可用的图形适配器（headless，无需窗口）");
    println!("adapter: {:?}", adapter.get_info());

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("ll-text headless device"),
        ..Default::default()
    }))
    .expect("请求 GPU 设备失败");

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ll-text demo canvas"),
        size: wgpu::Extent3d {
            width: CANVAS_WIDTH,
            height: CANVAS_HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CANVAS_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    clear_canvas(&device, &queue, &view);

    let mut text_renderer =
        TextRenderer::new(&device, &queue, CANVAS_FORMAT).expect("内置字体资产应能正常加载");

    // U+EB20 是 Tabler Icons「设置」图标实测到的 PUA 码位，见
    // knowledge/licenses/2026-08-18-ll-text-asset-import.md。混在标题
    // 行里，同时验证中文、拉丁字母、图标字体回退三者能否画在同一行。
    let runs = [
        TextRun {
            text: "迷途大陆 Lost Land 设置\u{EB20}",
            x: 24.0,
            y: 24.0,
            font_size: 28.0,
            line_height: 34.0,
            max_width: (CANVAS_WIDTH - 48) as f32,
            color: glyphon::Color::rgb(240, 240, 235),
            bold: true,
        },
        TextRun {
            text: "中英文混排 mixed text：从萤火虫沼泽出发，向北走三格即可抵达坍塌的钟塔遗迹。",
            x: 24.0,
            y: 80.0,
            font_size: 18.0,
            line_height: 24.0,
            max_width: (CANVAS_WIDTH - 48) as f32,
            color: glyphon::Color::rgb(205, 210, 215),
            bold: false,
        },
        // 附带一行 12px 小字号：管线文档第 4.1 节④把「思源黑体在
        // 12–16px 下是否粘连、可读」标注为「未核实」，是本任务第一次
        // 拿到真实渲染管线可以去验证的机会。这一行不是控件产出，只是
        // 顺手在同一张截图证据里回答这个具体问题。
        TextRun {
            text: "12px 小字号可读性核查：鑫囊攀繁鬻齉爨（笔画密集字）abcdefg 0123456789",
            x: 24.0,
            y: 140.0,
            font_size: 12.0,
            line_height: 16.0,
            max_width: (CANVAS_WIDTH - 48) as f32,
            color: glyphon::Color::rgb(180, 185, 190),
            bold: false,
        },
    ];

    text_renderer
        .render(&device, &queue, &view, CANVAS_WIDTH, CANVAS_HEIGHT, &runs)
        .expect("文本渲染失败");

    let pixels = read_pixels(
        &device,
        &queue,
        &texture,
        CANVAS_FORMAT,
        CANVAS_WIDTH,
        CANVAS_HEIGHT,
    );

    let path = PathBuf::from(OUTPUT_PATH);
    std::fs::create_dir_all(path.parent().expect("落盘路径应有父目录"))
        .expect("创建截图证据目录失败");
    image::save_buffer(
        &path,
        &pixels,
        CANVAS_WIDTH,
        CANVAS_HEIGHT,
        image::ColorType::Rgba8,
    )
    .expect("保存截图证据 PNG 失败");

    println!("已写入截图证据: {}", path.display());
}

/// 把画布清成深色底，模拟「世界层已经画好、文本层在上面追加一层」这个
/// 真实的两遍合成场景——`TextRenderer::render` 用 `LoadOp::Load`，若
/// 不预先清屏，读回的画面会是纹理刚分配时的未初始化内容。
fn clear_canvas(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ll-text demo clear encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ll-text demo clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.06,
                        g: 0.07,
                        b: 0.09,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    queue.submit(core::iter::once(encoder.finish()));
}

/// 把纹理像素读回 CPU 内存，按行剔除 wgpu 要求的对齐填充。
///
/// 与 `ll_render::target::RenderTarget::read_pixels` 是同一套手法
/// （对齐拷贝 + 逐行裁剪），本 demo 独立实现一份而不是复用它：那个
/// 方法是 `RenderTarget` 的私有实现细节，绑死在它自己持有的纹理字段
/// 上，不是一个可复用的自由函数。
fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let bytes_per_pixel = format
        .block_copy_size(None)
        .expect("演示画布用的是未压缩颜色格式，block size 恒有值");
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

    let buffer_size = (padded_bytes_per_row * height) as wgpu::BufferAddress;
    let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ll-text demo readback buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ll-text demo readback encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(core::iter::once(encoder.finish()));

    let slice = readback_buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("演示画布的读回缓冲区映射失败");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("等待 GPU 完成读回失败");

    let padded = slice
        .get_mapped_range()
        .expect("缓冲区已映射完成，取范围不应失败");
    let mut pixels = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    for row in 0..height as usize {
        let start = row * padded_bytes_per_row as usize;
        let end = start + unpadded_bytes_per_row as usize;
        pixels.extend_from_slice(&padded[start..end]);
    }
    drop(padded);
    readback_buffer.unmap();

    pixels
}
