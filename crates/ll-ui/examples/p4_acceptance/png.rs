//! 把离屏渲染目标的像素冻结成基准 PNG。
//!
//! 与 p1/p2/p3_acceptance 的同名模块逐字同构（理由见各自文档：离屏
//! 渲染目标固定用 `TARGET_FORMAT`，与窗口 surface 格式无关）。
//!
//! **已知范围边界**：这份基准只覆盖世界层（地形 + 玩家精灵），不含
//! 加载管理界面的文字面板——文字画在窗口 surface 上（`ll_text` 的
//! 设计要求原生分辨率，不经过 `RenderTarget`，见其模块文档「两条
//! 渲染通道」一节），而窗口 surface 纹理只声明了 `RENDER_ATTACHMENT`
//! 用途，不含 `COPY_SRC`，无法读回。如实记录这条限制，不假装截图
//! 覆盖了文字面板。

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
