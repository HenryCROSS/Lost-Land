//! 把离屏渲染目标的像素冻结成基准 PNG。
//!
//! **这是冻结视觉回归基准的入口，不是调试功能**——`tests/visual/baseline/`
//! 下的 PNG 就是从这里产出的，比对失败时的处置规矩见该目录的 README。

use crate::GpuResources;
use ll_render::target::{LOGICAL_HEIGHT, LOGICAL_WIDTH};

/// 把 `resources` 持有的离屏渲染目标当前像素读回并存成 `path` 处的 PNG。
pub(crate) fn save_baseline_png(resources: &GpuResources, path: &str) {
    let format = resources.gpu.surface_format();
    let Some(is_bgra) = bgra_channel_order(format) else {
        tracing::error!(?format, "无法识别的离屏目标像素格式，跳过存图");
        return;
    };

    let mut pixels = resources.render_target.read_pixels(&resources.gpu);
    if is_bgra {
        // wgpu 在多数原生后端上选中的 surface 格式是 BGRA，而 PNG/`image`
        // 期望 RGBA——两者只是 R、B 两个分量顺序相反，逐像素换回来即可，
        // 不需要重新解释整块字节。
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }

    if let Some(parent) = std::path::Path::new(path).parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        tracing::error!(%error, path, "创建基准目录失败，跳过存图");
        return;
    }

    match image::RgbaImage::from_raw(LOGICAL_WIDTH, LOGICAL_HEIGHT, pixels) {
        Some(image) => match image.save(path) {
            Ok(()) => tracing::info!(path, "baseline PNG saved"),
            Err(error) => tracing::error!(%error, path, "写出基准 PNG 失败"),
        },
        None => tracing::error!(path, "像素缓冲区大小与目标尺寸不匹配"),
    }
}

/// 判断像素格式是 BGRA 还是 RGBA 通道顺序。
///
/// 两种都是 `image` crate 能直接理解的 8 位无符号格式，唯一区别就是
/// R/B 分量顺序；除此之外的格式（例如某些平台可能选中的 10 位或浮点
/// 格式）本函数明确拒绝识别，交给调用方决定如何降级，而不是悄悄假设
/// 一种字节布局把图存歪。
fn bgra_channel_order(format: wgpu::TextureFormat) -> Option<bool> {
    match format {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => Some(true),
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra格式被识别为需要换回通道顺序() {
        // Arrange & Act & Assert
        assert_eq!(
            bgra_channel_order(wgpu::TextureFormat::Bgra8UnormSrgb),
            Some(true)
        );
    }

    #[test]
    fn rgba格式被识别为无需换通道顺序() {
        // Arrange & Act & Assert
        assert_eq!(
            bgra_channel_order(wgpu::TextureFormat::Rgba8UnormSrgb),
            Some(false)
        );
    }

    #[test]
    fn 无法识别的格式返回空值而非误判() {
        // 与其对未知格式假设一种字节布局把图存歪，不如显式拒绝识别，
        // 交给调用方决定如何降级。
        // Arrange & Act & Assert
        assert_eq!(bgra_channel_order(wgpu::TextureFormat::Rgba16Float), None);
    }
}
