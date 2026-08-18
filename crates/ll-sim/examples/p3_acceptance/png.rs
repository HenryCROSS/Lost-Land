//! 把离屏渲染目标的像素冻结成基准 PNG。
//!
//! 与 `ll-render` 的 `p1_acceptance::png`、`ll-world` 的
//! `p2_acceptance::png` 逐字同构，理由同样是「离屏渲染目标固定用
//! `TARGET_FORMAT`，与窗口 surface 格式无关」——三个 demo 落盘基准图
//! 的路径没有任何理由长得不一样。

use crate::GpuResources;
use ll_render::target::{LOGICAL_HEIGHT, LOGICAL_WIDTH, TARGET_FORMAT};

/// 把 `resources` 持有的离屏渲染目标当前像素读回并存成 `path` 处的 PNG。
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
