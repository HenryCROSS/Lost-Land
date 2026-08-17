//! 精灵图集：把多张贴图打包进一张纹理，减少绘制调用与纹理切换开销。
//!
//! 图集元数据来自第三方 mod，属于**外部不可信输入**：`serde_json`
//! 反序列化之后必须做语义校验，任何畸形输入只能返回 [`RenderError`]，
//! 绝不能 panic。

use crate::RenderError;
use crate::gpu::GpuContext;
use crate::sprite::{Footprint, Pivot};
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
/// 覆盖——校验只依赖解码出的图片尺寸，与 GPU 设备无关（同样的做法见
/// [`crate::gpu::is_presentable`]）。
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

/// 已上传到 GPU 的图集纹理及其元数据。
///
/// 只持有 [`wgpu::TextureView`] 而非原始 [`wgpu::Texture`]：`Texture::create_view`
/// 内部会克隆一份 `Texture` 句柄存进返回的 `TextureView`（wgpu 30 的实现如此），
/// 因此 view 本身已经足以让底层 GPU 资源存活到 `Atlas` 被丢弃为止，不需要
/// 额外持有一份原始句柄。
pub struct Atlas {
    metadata: AtlasMetadata,
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
        let (width, height) = decoded.dimensions();

        // 元数据本身只描述矩形数字，看不到图片；只有在这里解码出真实
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
}
