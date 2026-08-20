//! 精灵图集：把多张贴图打包进一张纹理，减少绘制调用与纹理切换开销。
//!
//! 图集元数据来自第三方 mod，属于**外部不可信输入**：`serde_json`
//! 反序列化之后必须做语义校验，任何畸形输入只能返回 [`RenderError`]，
//! 绝不能 panic。

use crate::RenderError;
use crate::gpu::GpuContext;
use crate::sprite::{Footprint, Pivot, SpriteSize};
use std::collections::HashSet;

/// 图集纹理上的一块矩形区域，单位为像素，原点在图像左上角。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct FrameRect {
    /// 矩形左边到图像左边的像素距离。
    pub x: u16,
    /// 矩形上边到图像上边的像素距离。
    pub y: u16,
    /// 矩形宽度（像素）。
    pub width: u16,
    /// 矩形高度（像素）。
    pub height: u16,
}

/// 图集里的一条条目：一张贴图在图集中的位置及其摆放参数。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct AtlasEntry {
    /// 条目名，供 mod 与代码按名字引用。这是内部标识符，不是用户可见
    /// 文本，因此不受「禁止硬编码用户可见字符串」约束。
    pub name: String,
    /// 贴图在图集纹理上的位置。
    pub rect: FrameRect,
    /// 锚点，定义世界坐标对应贴图内的哪个像素。
    pub pivot: Pivot,
    /// 逻辑占地格数。
    pub footprint: Footprint,
}

impl AtlasEntry {
    /// 这个条目的视觉像素尺寸，即帧矩形的宽高。
    ///
    /// [`SpriteSize`] 描述「画多大」，与描述「占几格」的 [`Footprint`]
    /// 刻意分开（规格 §12.1，见 `sprite.rs` 模块文档）；这个方法是两者
    /// 在图集条目上的唯一交汇点——把 [`FrameRect`] 的宽高转换成调用方
    /// 应该用来绘制的视觉尺寸，而不需要自己伸手拆 `rect.width`/`rect.height`。
    pub fn sprite_size(&self) -> SpriteSize {
        SpriteSize {
            width: self.rect.width,
            height: self.rect.height,
        }
    }
}

/// 一张图集的完整元数据：所属图片文件与其中的全部条目。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct AtlasMetadata {
    /// 图集图片相对资产目录的文件名。
    pub image: String,
    /// 图集内的全部条目。
    pub entries: Vec<AtlasEntry>,
}

impl AtlasMetadata {
    /// 解析并校验图集元数据 JSON。
    ///
    /// 图集元数据来自第三方 mod，是外部不可信输入。`serde_json` 只保证
    /// 结构合法，不保证语义合法，因此反序列化之后还要拒绝畸形输入：
    ///
    /// - 零尺寸的帧矩形（会生成退化的四边形，部分驱动上是未定义行为）。
    /// - 零尺寸的占地格数（[`Footprint::tile_count`] 会静默返回 0，
    ///   下游拿到「占 0 格」的实体不会报错，只会悄悄从世界里消失——
    ///   这正是「外部输入只能返回错误、不能悄悄错」这条要求要挡的事）。
    /// - 落在帧矩形之外的锚点（[`Pivot`] 描述的是帧内的一个像素，越界
    ///   的锚点在批渲染阶段会算出指向图集之外的采样坐标）。
    /// - 重名条目（会让 [`Self::lookup`] 的结果取决于条目顺序，是 mod
    ///   冲突的常见来源）。
    ///
    /// 帧矩形是否超出图集图片的真实边界不在这里检查——`parse` 只看得到
    /// 元数据，看不到图片，这项校验在 [`Atlas::load`] 解码出图片尺寸后
    /// 进行。校验失败一律返回 [`RenderError::AtlasMetadata`]，绝不 panic。
    pub fn parse(json: &str) -> Result<AtlasMetadata, RenderError> {
        let metadata: AtlasMetadata = serde_json::from_str(json)
            .map_err(|error| RenderError::AtlasMetadata(error.to_string()))?;

        let mut seen_names = HashSet::with_capacity(metadata.entries.len());
        for entry in &metadata.entries {
            if entry.rect.width == 0 || entry.rect.height == 0 {
                return Err(RenderError::AtlasMetadata(format!(
                    "条目 '{}' 的帧矩形尺寸为零（{}x{}）",
                    entry.name, entry.rect.width, entry.rect.height
                )));
            }
            if entry.footprint.width == 0 || entry.footprint.height == 0 {
                return Err(RenderError::AtlasMetadata(format!(
                    "条目 '{}' 的占地格数为零（{}x{}）",
                    entry.name, entry.footprint.width, entry.footprint.height
                )));
            }
            if !pivot_within_rect(entry.pivot, entry.rect) {
                return Err(RenderError::AtlasMetadata(format!(
                    "条目 '{}' 的锚点 ({}, {}) 落在帧矩形之外（帧尺寸 {}x{}）",
                    entry.name, entry.pivot.x, entry.pivot.y, entry.rect.width, entry.rect.height
                )));
            }
            if !seen_names.insert(entry.name.as_str()) {
                return Err(RenderError::AtlasMetadata(format!(
                    "条目名重复：'{}'",
                    entry.name
                )));
            }
        }

        Ok(metadata)
    }

    /// 按名字查找条目。名字不存在时返回 [`None`]，不是错误——调用方
    /// 决定是否把「找不到」当成错误。
    pub fn lookup(&self, name: &str) -> Option<&AtlasEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }
}

/// 锚点是否落在帧矩形范围内（含边界）。
///
/// 锚点允许恰好落在矩形右/下边缘（等于宽/高），因为像素坐标系里
/// 「宽度为 W 的矩形」的有效横坐标是 `[0, W]`——`W` 本身是右边界，
/// 不是越界。
fn pivot_within_rect(pivot: Pivot, rect: FrameRect) -> bool {
    pivot.x >= 0
        && pivot.y >= 0
        && (pivot.x as u32) <= rect.width as u32
        && (pivot.y as u32) <= rect.height as u32
}

/// 校验图集条目的帧矩形是否都落在图片真实边界内。
///
/// 抽成不依赖 GPU 的自由函数，是为了不需要真实 [`GpuContext`] 就能单测
/// 覆盖——校验只依赖解码出的图片尺寸，与 GPU 设备无关。
///
/// `rect.x`/`rect.width` 等字段都是 `u16`，先转 `u32` 再相加：若直接在
/// `u16` 上相加，两个接近 `u16::MAX` 的畸形值会在加法本身就溢出，
/// debug 下 panic、release 下静默环绕成一个看似合法的小矩形——那就是
/// 「元数据看起来完全合法」这种最难定位的错误的来源。转到 `u32` 后，
/// 两个 `u16` 之和的理论上限（约 13 万）远小于 `u32::MAX`，不会溢出。
fn validate_entries_within_image(
    entries: &[AtlasEntry],
    image_width: u32,
    image_height: u32,
) -> Result<(), RenderError> {
    for entry in entries {
        let rect = entry.rect;
        let right = rect.x as u32 + rect.width as u32;
        let bottom = rect.y as u32 + rect.height as u32;
        if right > image_width || bottom > image_height {
            return Err(RenderError::AtlasMetadata(format!(
                "条目 '{}' 的帧矩形超出图片边界：矩形右下角 ({right}, {bottom})，图片尺寸 {image_width}x{image_height}",
                entry.name
            )));
        }
    }
    Ok(())
}

/// 把图集条目的像素矩形换算成归一化 `(u, v, width, height)`。
///
/// 换算陷阱见 [`Atlas::uv_rect`] 文档；提成自由函数是为了不依赖真实
/// GPU 纹理就能单测覆盖（同样的做法见 `gpu.rs` 的 `is_presentable`）。
fn normalized_uv_rect(rect: FrameRect, image_width: u32, image_height: u32) -> [f32; 4] {
    let inset_x = axis_inset(rect.width);
    let inset_y = axis_inset(rect.height);
    let image_width = image_width as f32;
    let image_height = image_height as f32;

    [
        (rect.x as f32 + inset_x) / image_width,
        (rect.y as f32 + inset_y) / image_height,
        (rect.width as f32 - 2.0 * inset_x) / image_width,
        (rect.height as f32 - 2.0 * inset_y) / image_height,
    ]
}

/// 单边内缩量：正常帧取半个纹素（0.5px），窄于 1px 的极端帧退化为
/// 半宽，保证 `size - 2*inset` 恒不为负。
fn axis_inset(size: u16) -> f32 {
    (size as f32 / 2.0).min(0.5)
}

/// 已上传到 GPU 的图集纹理及其元数据。
///
/// 只持有 [`wgpu::TextureView`] 而非原始 [`wgpu::Texture`]：`Texture::create_view`
/// 内部会克隆一份 `Texture` 句柄存进返回的 `TextureView`（wgpu 30 的实现如此），
/// 因此 view 本身已经足以让底层 GPU 资源存活到 `Atlas` 被丢弃为止，不需要
/// 额外持有一份原始句柄。
pub struct Atlas {
    metadata: AtlasMetadata,
    /// 图集纹理的真实像素尺寸，[`Self::uv_rect`] 把像素矩形换算成归一化
    /// UV 时要除以它——绝不能用逻辑分辨率或任何其他尺寸代替（见该方法
    /// 文档「换算陷阱」）。
    size: (u32, u32),
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
}

impl Atlas {
    /// 解码图集 PNG 并上传为 GPU 纹理。
    ///
    /// 采样器固定用 [`wgpu::FilterMode::Nearest`]：线性插值会把像素
    /// 美术的硬边缘糊掉，是最刺眼的瑕疵，必须避免（与
    /// [`crate::target::RenderTarget`] 的放大 blit 同一约束）。
    pub fn load(
        gpu: &GpuContext,
        metadata: AtlasMetadata,
        image_bytes: &[u8],
    ) -> Result<Atlas, RenderError> {
        let decoded = image::load_from_memory(image_bytes)
            .map_err(|error| RenderError::AtlasDecode(error.to_string()))?
            .to_rgba8();
        Atlas::from_rgba(gpu, metadata, decoded)
    }

    /// 把一张已经解码好的 RGBA 图像连同其元数据上传为 GPU 图集纹理。
    ///
    /// 与 [`Self::load`] 共享同一段校验 + 上传逻辑，区别只在输入：
    /// [`Self::load`] 从原始 PNG 字节解码，本方法接收调用方已经解码好
    /// 的画布——运行期图集打包（`crate::atlas_pack::pack_atlas`）本身
    /// 就要把多张松散贴图合成一张 [`image::RgbaImage`] 画布，若为了
    /// 复用 [`Self::load`] 而把这张画布重新编码成 PNG 再传进去解码
    /// 一遍，是一趟纯粹浪费的编解码往返——两边共享的画布类型让这趟
    /// 往返完全没有必要。
    pub fn from_rgba(
        gpu: &GpuContext,
        metadata: AtlasMetadata,
        decoded: image::RgbaImage,
    ) -> Result<Atlas, RenderError> {
        let (width, height) = decoded.dimensions();

        // 元数据本身只描述矩形数字，看不到图片；只有在这里拿到真实
        // 尺寸后，才能判断畸形或恶意的 mod 是否给出了越界矩形。不查这
        // 一项的后果是花屏或贴图错乱——而且因为元数据本身「看起来完全
        // 合法」，这类问题在批渲染阶段会极难定位。
        validate_entries_within_image(&metadata.entries, width, height)?;

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("ll-render atlas texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        gpu.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &decoded,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                // RGBA8 恒为每像素 4 字节，图片解码后已是紧凑行，
                // 不像 target.rs 的读回缓冲区那样需要处理对齐填充。
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = gpu.device().create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ll-render atlas sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Ok(Atlas {
            metadata,
            size: (width, height),
            view,
            sampler,
        })
    }

    /// 图集元数据，供批渲染按名字查条目。
    pub fn metadata(&self) -> &AtlasMetadata {
        &self.metadata
    }

    /// 图集纹理的视图，批渲染以此为采样源绑定到管线。
    pub fn texture_view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// 图集采样器，固定最近邻过滤。
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// 图集纹理的真实像素尺寸 `(width, height)`。
    ///
    /// 调用方（尤其是需要自行换算 UV 的场景）必须用这个值做分母，
    /// 不能用逻辑分辨率或任何猜测值——见 [`Self::uv_rect`] 文档。
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// 按条目名查出它在图集里的归一化 UV 矩形 `(u, v, width, height)`，
    /// 供 [`crate::batch::SpriteInstance::uv_rect`] 直接使用。
    ///
    /// 这是渲染层的核心知识，不该留给每个调用方各自重新推导——两个
    /// 换算陷阱都在这里被处理：
    ///
    /// 1. **必须除以图集纹理的真实像素尺寸**（[`Self::size`]），不是
    ///    逻辑分辨率 640×360——图集与离屏渲染目标是两张完全不同尺寸的
    ///    纹理，用错分母会让整张贴图的采样坐标系全错，表现为贴图整体
    ///    错位或被拉伸/压缩，而不是某一处局部瑕疵。
    /// 2. **半 texel 内缩**：即便采样器固定最近邻（见 [`Self::load`]），
    ///    把 UV 精确算在两个纹素的分界线上仍可能因浮点误差被舍入到
    ///    分界线另一侧，采样出邻居贴图的颜色——这是像素图集最常见也
    ///    最难定位的花屏成因之一：现象是「某个精灵的一条边缘偶尔混进
    ///    了旁边贴图的颜色」，且往往只在特定缩放或特定 GPU 上出现，
    ///    元数据与代码本身完全看不出问题。这里把矩形四边各内缩最多
    ///    0.5 像素（不足 0.5 像素宽/高的极端帧按实际半宽内缩，避免
    ///    缩成负数）：内缩幅度远小于一个纹素，最近邻过滤仍稳定选中
    ///    同一个纹素，不会引入任何肉眼可见的裁切。
    ///
    /// 条目名不存在时返回 [`None`]，理由同 [`AtlasMetadata::lookup`]。
    pub fn uv_rect(&self, name: &str) -> Option<[f32; 4]> {
        let entry = self.metadata.lookup(name)?;
        Some(normalized_uv_rect(entry.rect, self.size.0, self.size.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "image": "placeholder.png",
        "entries": [
            { "name": "hero_idle_0",
              "rect": { "x": 0, "y": 0, "width": 16, "height": 24 },
              "pivot": { "x": 8, "y": 24 },
              "footprint": { "width": 1, "height": 1 } }
        ]
    }"#;

    #[test]
    fn 解析合法元数据得到对应条目() {
        // Arrange & Act
        let metadata = AtlasMetadata::parse(SAMPLE).expect("样例是合法 JSON");

        // Assert
        assert_eq!(metadata.entries.len(), 1);
    }

    #[test]
    fn 可按名字查到条目() {
        // Arrange
        let metadata = AtlasMetadata::parse(SAMPLE).expect("样例是合法 JSON");

        // Act
        let entry = metadata.lookup("hero_idle_0");

        // Assert
        assert!(entry.is_some());
    }

    #[test]
    fn 查不到的名字返回空值() {
        // Arrange
        let metadata = AtlasMetadata::parse(SAMPLE).expect("样例是合法 JSON");

        // Act & Assert
        assert!(metadata.lookup("does_not_exist").is_none());
    }

    #[test]
    fn 畸形输入返回错误而非崩溃() {
        // 图集元数据会来自第三方 mod，属于外部不可信输入。
        // Arrange & Act
        let result = AtlasMetadata::parse("{ this is not json");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 零宽度的帧矩形被拒绝() {
        // 零宽或零高的帧会生成退化的四边形，在部分驱动上是未定义行为。
        // Arrange
        let broken = SAMPLE.replace("\"width\": 16", "\"width\": 0");

        // Act
        let result = AtlasMetadata::parse(&broken);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 重名条目被拒绝() {
        // 重名会让 lookup 的结果取决于顺序，是 mod 冲突的常见来源。
        // Arrange
        let entry = r#"{ "name": "hero_idle_0",
              "rect": { "x": 0, "y": 0, "width": 16, "height": 24 },
              "pivot": { "x": 8, "y": 24 },
              "footprint": { "width": 1, "height": 1 } },"#;
        let duplicated = SAMPLE.replace("\"entries\": [", &format!("\"entries\": [{entry}"));

        // Act
        let result = AtlasMetadata::parse(&duplicated);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 占地格数为零的条目被拒绝() {
        // tile_count 对零尺寸会静默返回 0，下游拿到的实体会悄悄从世界
        // 里消失而不报错，正是「外部输入只能返回错误」这条要求要挡的事。
        // Arrange
        let broken = SAMPLE.replace("\"width\": 1,", "\"width\": 0,");

        // Act
        let result = AtlasMetadata::parse(&broken);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 锚点落在帧矩形之外的条目被拒绝() {
        // 越界的锚点在批渲染阶段会算出指向图集之外的采样坐标。
        // Arrange
        let broken = SAMPLE.replace(
            "\"pivot\": { \"x\": 8, \"y\": 24 }",
            "\"pivot\": { \"x\": 8, \"y\": 999 }",
        );

        // Act
        let result = AtlasMetadata::parse(&broken);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 帧矩形超出图片边界的条目被拒绝() {
        // 构造一张真实解码出的 8x8 小图，模拟 Atlas::load 拿到的图片尺寸；
        // 畸形或恶意的 mod 完全可以给出比图片本身还大的矩形，若不查，
        // 后果是花屏或贴图错乱——而且元数据本身「看起来完全合法」，
        // 这类问题在批渲染阶段会极难定位。
        // Arrange
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(8, 8))
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .expect("内存编码 8x8 PNG 不应失败");
        let decoded = image::load_from_memory(&png_bytes)
            .expect("刚编码的 PNG 应能解码")
            .to_rgba8();
        let (width, height) = decoded.dimensions();

        let entries = vec![AtlasEntry {
            name: "oversized".to_string(),
            rect: FrameRect {
                x: 0,
                y: 0,
                width: 16,
                height: 16,
            },
            pivot: Pivot { x: 0, y: 0 },
            footprint: Footprint {
                width: 1,
                height: 1,
            },
        }];

        // Act
        let result = validate_entries_within_image(&entries, width, height);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 帧矩形恰好贴合图片边界时被接受() {
        // 边界值：矩形右下角恰好等于图片尺寸，不应被误判为越界。
        // Arrange
        let entries = vec![AtlasEntry {
            name: "exact_fit".to_string(),
            rect: FrameRect {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            pivot: Pivot { x: 0, y: 0 },
            footprint: Footprint {
                width: 1,
                height: 1,
            },
        }];

        // Act
        let result = validate_entries_within_image(&entries, 8, 8);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn uv矩形按图集真实尺寸而非逻辑分辨率换算() {
        // 这是评审点名的关键陷阱：分母必须是图集像素尺寸（这里 64），
        // 不能是逻辑分辨率 640。
        // Arrange
        let rect = FrameRect {
            x: 0,
            y: 0,
            width: 16,
            height: 24,
        };

        // Act
        let uv = normalized_uv_rect(rect, 64, 72);

        // Assert：宽高各按半 texel 内缩一整像素（两边各 0.5）。
        assert!((uv[2] - 15.0 / 64.0).abs() < f32::EPSILON);
        assert!((uv[3] - 23.0 / 72.0).abs() < f32::EPSILON);
    }

    #[test]
    fn uv矩形内缩后小于原始像素矩形换算值() {
        // 半 texel 内缩必须真的把矩形往内收，否则起不到防止采样越界到
        // 邻居贴图的作用。
        // Arrange
        let rect = FrameRect {
            x: 16,
            y: 0,
            width: 32,
            height: 48,
        };

        // Act
        let uv = normalized_uv_rect(rect, 64, 72);
        let naive_u = rect.x as f32 / 64.0;
        let naive_width = rect.width as f32 / 64.0;

        // Assert
        assert!(uv[0] > naive_u);
        assert!(uv[2] < naive_width);
    }

    #[test]
    fn 一像素宽的帧内缩后宽度不为负() {
        // 内缩量必须按帧实际宽高钳制，否则极窄的帧会算出负宽度。
        // Arrange
        let rect = FrameRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };

        // Act
        let uv = normalized_uv_rect(rect, 64, 64);

        // Assert
        assert!(uv[2] >= 0.0);
        assert!(uv[3] >= 0.0);
    }

    #[test]
    fn 查不到的条目名时uv矩形返回空值() {
        // Arrange
        let metadata = AtlasMetadata::parse(SAMPLE).expect("样例是合法 JSON");
        let atlas_size = (64u32, 72u32);

        // Act：绕开需要真实 GPU 的 Atlas::load，直接用同一份换算逻辑
        // 验证「查不到条目」这一分支——uv_rect 本身的整体行为已经在
        // 需要真实设备的黑箱测试里覆盖。
        let result = metadata
            .lookup("does_not_exist")
            .map(|entry| normalized_uv_rect(entry.rect, atlas_size.0, atlas_size.1));

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn 条目的视觉尺寸取自帧矩形的宽高() {
        // SpriteSize 描述「画多大」，Footprint 描述「占几格」；这个
        // 测试锁住 sprite_size() 取的是前者的数据源（帧矩形），不是
        // 误取 footprint。
        // Arrange
        let metadata = AtlasMetadata::parse(SAMPLE).expect("样例是合法 JSON");
        let entry = metadata.lookup("hero_idle_0").expect("样例含此条目");

        // Act
        let size = entry.sprite_size();

        // Assert
        assert_eq!((size.width, size.height), (16, 24));
    }
}
