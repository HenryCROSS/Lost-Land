//! 栅格化上屏：`glyphon` 把排版结果画进 GPU 纹理。
//!
//! **原生分辨率，不接触 [`ll_render::target::RenderTarget`]**——见 crate
//! 顶层文档「两条渲染通道」一节。本模块不引用 `LOGICAL_WIDTH`/
//! `LOGICAL_HEIGHT`/`fit_viewport` 中的任何一个，[`TextRenderer::render`]
//! 接收的 `resolution_width`/`resolution_height` 必须是调用方目标纹理的
//! **真实原生像素尺寸**，不是 640×360 那个逻辑分辨率。

use cosmic_text::fontdb::Database;
use cosmic_text::{
    Attrs, Buffer as CosmicBuffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight,
};
use glyphon::{Cache, Resolution, TextArea, TextAtlas, TextBounds, Viewport};
use ll_render::wgpu;

use crate::error::TextError;
use crate::fonts::FontCatalog;
use crate::layout::{self, LayoutResult};

/// 一段待绘制的文本：内容、位置、字号与颜色。
///
/// **只做地基**：不支持富文本（同一段内混合多种字号/颜色），也不支持
/// 对齐/换行策略之外的排版控件——多字号/多颜色需求由调用方拆成多个
/// `TextRun`，九宫格边框、焦点导航这些真正的控件属于 P6。
#[derive(Debug, Clone, Copy)]
pub struct TextRun<'a> {
    /// 要绘制的文本，可以是中英文混排，也可以夹带图标字体的 PUA 字符。
    pub text: &'a str,
    /// 左上角在目标纹理里的 x 像素坐标（原生分辨率，不经过整数缩放）。
    pub x: f32,
    /// 左上角在目标纹理里的 y 像素坐标。
    pub y: f32,
    /// 字号（像素，字体原始设计尺寸下的度量）。
    pub font_size: f32,
    /// 行高（像素）。
    pub line_height: f32,
    /// 换行的最大宽度（像素）。
    pub max_width: f32,
    /// 文本颜色。
    pub color: glyphon::Color,
    /// 是否使用粗体字重。本项目只用一套字体家族，层次感靠字号与字重
    /// 拉开（管线文档第 2.5 节），这里因此只留一个布尔开关，不是完整
    /// 的字重枚举。
    pub bold: bool,
}

/// 持有 `glyphon`/`cosmic-text` 全部运行期状态的文本渲染器。
pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    catalog: FontCatalog,
    // `glyphon::Cache` 目前只在构造 `TextAtlas`/`Viewport` 时被引用，
    // 之后不再单独使用，但必须存活到 `TextRenderer` 本身被丢弃——
    // 保留这个字段纯粹是为了延长其生命周期，不是要在其他方法里用它。
    _cache: Cache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: glyphon::TextRenderer,
    format: wgpu::TextureFormat,
}

impl TextRenderer {
    /// 加载内置字体、初始化 `glyphon` 的 GPU 资源。
    ///
    /// **接口相对简报草稿的调整**：简报给的签名是
    /// `new(gpu: &ll_render::gpu::GpuContext)`，实现时改成直接接收
    /// `&wgpu::Device`/`&wgpu::Queue`/目的纹理格式三个原语——
    /// `GpuContext::new` 强制要求一个真实 `winit` 窗口（用于创建
    /// `Surface`），但本 crate 的 GPU 资源（字体图集、渲染管线）本身
    /// 不需要 surface，只需要设备与队列。继续按简报原样接收
    /// `&GpuContext` 会让「离屏渲染、不建窗口」这条本任务要求的最短
    /// 验证路径无法实现——headless 测试与截图证据都要求能在没有窗口的
    /// 环境下拿到可用的 `Device`/`Queue`。调用方在有 `GpuContext` 的
    /// 正常场景下传 `gpu.device()`/`gpu.queue()`/`gpu.surface_format()`
    /// 三个访问器的返回值即可，成本只是三个参数而不是一个引用。
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Result<TextRenderer, TextError> {
        let mut db = Database::new();
        let catalog = FontCatalog::load(&mut db)?;
        let font_system = FontSystem::new_with_locale_and_db("zh-CN".to_string(), db);
        let swash_cache = SwashCache::new();

        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            glyphon::TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);

        Ok(TextRenderer {
            font_system,
            swash_cache,
            catalog,
            _cache: cache,
            atlas,
            viewport,
            renderer,
            format,
        })
    }

    /// 目的纹理格式，恒为构造时传入的 `format` 参数。
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// 对一段文本排版但不绘制，供调用方在绘制前查询断行结果与占用尺寸。
    pub fn layout(
        &mut self,
        text: &str,
        font_size: f32,
        line_height: f32,
        max_width: f32,
    ) -> LayoutResult {
        layout::layout_text(
            &mut self.font_system,
            &self.catalog,
            text,
            font_size,
            line_height,
            max_width,
        )
    }

    /// 把 `runs` 画进 `target`。
    ///
    /// **不清屏**——渲染 pass 用 `LoadOp::Load`，把 `target` 上已有的
    /// 内容（通常是 `RenderTarget::blit_to` 刚画上去的世界层）原样
    /// 保留，文本只是在上面追加一层，两条渲染通道各自独立成一道 pass，
    /// 只是先后画到同一张目的纹理。
    ///
    /// `resolution_width`/`resolution_height` 必须是 `target` 的真实
    /// 原生像素尺寸——这两个数拿不到是因为 `wgpu::TextureView` 本身不
    /// 携带尺寸信息，只能由持有原始 `wgpu::Texture`/窗口尺寸的调用方
    /// 传入。
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        resolution_width: u32,
        resolution_height: u32,
        runs: &[TextRun<'_>],
    ) -> Result<(), TextError> {
        self.viewport.update(
            queue,
            Resolution {
                width: resolution_width,
                height: resolution_height,
            },
        );

        let mut buffers = Vec::with_capacity(runs.len());
        for run in runs {
            let mut buffer = CosmicBuffer::new(
                &mut self.font_system,
                Metrics::new(run.font_size, run.line_height),
            );
            buffer.set_size(Some(run.max_width), None);
            let weight = if run.bold {
                Weight::BOLD
            } else {
                Weight::NORMAL
            };
            let attrs = Attrs::new()
                .family(Family::Name(&self.catalog.text_family))
                .weight(weight);
            buffer.set_text(run.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.font_system, false);
            buffers.push((buffer, run));
        }

        let areas: Vec<TextArea<'_>> = buffers
            .iter()
            .map(|(buffer, run)| TextArea {
                buffer,
                left: run.x,
                top: run.y,
                scale: 1.0,
                bounds: TextBounds::default(),
                default_color: run.color,
                custom_glyphs: &[],
            })
            .collect();

        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash_cache,
            )
            .map_err(|error| TextError::Prepare(error.to_string()))?;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ll-text render encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ll-text render pass"),
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
            self.renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .map_err(|error| TextError::Render(error.to_string()))?;
        }
        queue.submit(core::iter::once(encoder.finish()));
        self.atlas.trim();

        Ok(())
    }
}
