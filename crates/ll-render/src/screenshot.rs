//! 把离屏渲染目标当前的像素存成一张 PNG。
//!
//! # 为什么这个函数住在 `ll-render`
//!
//! 五个验收 demo 各自抄了一份等价的 `save_baseline_png`（
//! `crates/ll-render/examples/p1_acceptance/png.rs` 是最早那一份）。
//! 本体二进制（`ll-game`）接 `GameKey::Screenshot` 时不再抄第六份：
//! `ll-render` 已经把 `image` 列为正式依赖（`Cargo.toml`，为图集打包
//! 而引入），而 `ll-game` 没有——把函数放这里，`ll-game` 就不需要为
//! 一个存图功能把 `image` 从 `dev-dependencies` 提升成正式依赖。
//!
//! **本批次不重构那五份 demo 里的副本**（不在范围内）。它们与本函数
//! 逻辑等价，将来收拢是一次纯机械的替换。
//!
//! # 为什么读的是离屏目标，不是窗口 surface
//!
//! 与 [`crate::target::RenderTarget::read_pixels`] 文档同一条理由：
//! 离屏目标恒是固定的逻辑分辨率与固定的
//! [`crate::target::TARGET_FORMAT`]，读回的字节恒为 RGBA 顺序，
//! 不随运行环境的窗口分辨率与 surface 格式变化。
//!
//! **代价要说清楚**：这意味着存下来的图**不含 HUD 与模态屏**——那两条
//! 通道画在窗口 surface 的原生分辨率上，不经过离屏目标（见
//! `ll_ui::hud::render::render_hud` 文档「提交顺序」一节）。本函数存的
//! 是世界层画面。要连 HUD 一起存需要另读窗口 surface，那是一件不同的
//! 事，本批次不做。

use std::path::Path;

use crate::gpu::GpuContext;
use crate::target::{LOGICAL_HEIGHT, LOGICAL_WIDTH, RenderTarget, TARGET_FORMAT};

/// 存图失败的原因——只用于诊断日志，**调用方绝不该因为存图失败就让
/// 游戏崩溃**：按一次截图键失败，代价应当是一条日志，不是一局游戏。
#[derive(Debug)]
pub enum ScreenshotError {
    /// 建目录或写文件失败。
    Io(std::io::Error),
    /// 读回的像素数与目标尺寸对不上——按构造不该发生。
    PixelCountMismatch {
        /// 实际读回多少字节。
        actual: usize,
    },
    /// PNG 编码失败。
    Encode(image::ImageError),
}

impl std::fmt::Display for ScreenshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 下面三句面向开发者与日志，不面向玩家，与
        // `ll_core::error::CoreError` 的 Display 同一条既有约定，故逐行
        // 标注豁免。
        match self {
            ScreenshotError::Io(error) => write!(f, "截图写入失败：{error}"), // i18n-exempt
            ScreenshotError::PixelCountMismatch { actual } => {
                write!(f, "截图像素数与目标尺寸不匹配：读回 {actual} 字节") // i18n-exempt
            }
            ScreenshotError::Encode(error) => write!(f, "截图编码失败：{error}"), // i18n-exempt
        }
    }
}

impl std::error::Error for ScreenshotError {}

/// 把 `target` 当前的像素写成 `path` 处的 PNG，返回写出的尺寸。
///
/// 父目录不存在时自动建出来——截图目录是「第一次按下截图键的那一刻」
/// 才需要存在的东西，要求玩家自己先建一个目录是没有道理的。
pub fn save_png(
    gpu: &GpuContext,
    target: &RenderTarget,
    path: &Path,
) -> Result<(u32, u32), ScreenshotError> {
    let pixels = target.read_pixels(gpu);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(ScreenshotError::Io)?;
    }
    let Some(image) = image::RgbaImage::from_raw(LOGICAL_WIDTH, LOGICAL_HEIGHT, pixels) else {
        return Err(ScreenshotError::PixelCountMismatch { actual: 0 });
    };
    image.save(path).map_err(ScreenshotError::Encode)?;
    tracing::info!(path = %path.display(), format = ?TARGET_FORMAT, "截图已保存");
    Ok((LOGICAL_WIDTH, LOGICAL_HEIGHT))
}

/// 按帧号编出一个**不会覆盖任何既有文件**的截图文件名。
///
/// 不用「固定文件名 + 覆盖」：截图是玩家主动留下的东西，第二次按下
/// 截图键把第一张悄悄抹掉，是一个没有任何提示的数据丢失。帧号单调
/// 递增（`ll_platform::window::FrameId`），同一次会话内不会重复。
pub fn screenshot_file_name(frame: u64) -> String {
    format!("screenshot-{frame:012}.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 文件名按帧号补零对齐() {
        // 补零是为了让目录按文件名排序时恰好等于按时间排序。
        // Arrange & Act
        let 早 = screenshot_file_name(9);
        let 晚 = screenshot_file_name(10);

        // Assert
        assert!(早 < 晚);
    }

    #[test]
    fn 不同帧号给出不同文件名() {
        // 「第二次按截图把第一张抹掉」是这条不变式没守住时的症状。
        // Arrange & Act
        let 甲 = screenshot_file_name(1);
        let 乙 = screenshot_file_name(2);

        // Assert
        assert_ne!(甲, 乙);
    }

    #[test]
    fn 文件名以png结尾() {
        // Arrange & Act
        let name = screenshot_file_name(0);

        // Assert
        assert!(name.ends_with(".png"));
    }
}
