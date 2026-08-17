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
    /// 结构合法，不保证语义合法，因此反序列化之后还要拒绝两类畸形
    /// 输入：零尺寸的帧矩形（会生成退化的四边形，部分驱动上是未定义
    /// 行为），以及重名条目（会让 [`Self::lookup`] 的结果取决于条目
    /// 顺序，是 mod 冲突的常见来源）。校验失败一律返回
    /// [`RenderError::AtlasMetadata`]，绝不 panic。
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

/// 已上传到 GPU 的图集纹理及其元数据。
pub struct Atlas {
    metadata: AtlasMetadata,
    texture: wgpu::Texture,
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
            texture,
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

    /// 图集纹理本身，供需要底层句柄的高级用法（如生成 mipmap）使用。
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
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
}
