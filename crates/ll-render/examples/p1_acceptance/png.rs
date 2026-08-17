//! 把离屏渲染目标的像素冻结成基准 PNG。
//!
//! **这是冻结视觉回归基准的入口，不是调试功能**——`tests/visual/baseline/`
//! 下的 PNG 就是从这里产出的，比对失败时的处置规矩见该目录的 README。

use crate::GpuResources;
use ll_render::target::{LOGICAL_HEIGHT, LOGICAL_WIDTH, TARGET_FORMAT};

/// 把 `resources` 持有的离屏渲染目标当前像素读回并存成 `path` 处的 PNG。
///
/// 不需要按格式判断是否要换 R/B 通道顺序：离屏渲染目标固定用
/// [`TARGET_FORMAT`]（`Rgba8UnormSrgb`），与窗口 surface 的格式无关
/// （见 `target.rs` 模块文档），`read_pixels` 读回的字节恒为 RGBA 顺序，
/// 可以直接交给 [`image::RgbaImage::from_raw`]。
pub(crate) fn save_baseline_png(resources: &GpuResources, path: &str) {
    let pixels = resources.render_target.read_pixels(&resources.gpu);

    if let Some(parent) = std::path::Path::new(path).parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        tracing::error!(%error, path, "创建基准目录失败，跳过存图");
        return;
    }

    match image::RgbaImage::from_raw(LOGICAL_WIDTH, LOGICAL_HEIGHT, pixels) {
        Some(image) => match image.save(path) {
            Ok(()) => tracing::info!(path, format = ?TARGET_FORMAT, "baseline PNG saved"),
            Err(error) => tracing::error!(%error, path, "写出基准 PNG 失败"),
        },
        None => tracing::error!(path, "像素缓冲区大小与目标尺寸不匹配"),
    }
}
