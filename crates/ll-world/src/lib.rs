//! 迷途大陆的世界层。
//!
//! 承接 `ll-core` 的纯数据基础设施，落地成具体的世界状态：环面地形、
//! 分块存储、噪声生成、视野与光照、昼夜四季。本 crate 不接触渲染或
//! 平台细节——那些属于 `ll-render`/`ll-platform`，世界层只产出数据，
//! 由上层决定怎么画。
//!
//! # 浮点边界
//!
//! 世界状态禁止浮点数：跨平台浮点差异会摧毁确定性存档与重放（详见
//! `ll-core` 的说明）。本 crate 的所有模块全程使用整数与定点数。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::fmt;

pub mod chunk;
pub mod fov;
pub mod generate;
pub mod light;
pub mod noise;
pub mod overview;
pub mod state;
pub mod terrain;

/// 世界层的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldError {
    /// 世界尺寸小于渲染层视口所需的最小跨度。
    ///
    /// 世界小于这个跨度时，相机会产出重复坐标，地形填不满留黑块——
    /// 与其让缺陷在运行时以视觉异常的形式出现，不如在构造点直接拒绝。
    WorldTooSmall {
        /// 实际宽度（格）。
        width: u32,
        /// 实际高度（格）。
        height: u32,
    },
    /// 世界尺寸不能被噪声周期整除，无法保证接缝无缝。
    ///
    /// 用于地形生成入口的前置校验；本批次尚未接入生成流程，
    /// 这个变体暂时不会被构造。
    WorldNotTileable {
        /// 实际宽度（格）。
        width: u32,
        /// 实际高度（格）。
        height: u32,
    },
    /// 请求的分块索引超出世界范围。
    ChunkOutOfRange,
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorldError::WorldTooSmall { width, height } => {
                write!(f, "世界尺寸 {width}x{height} 小于视口所需的最小跨度")
            }
            WorldError::WorldNotTileable { width, height } => {
                write!(f, "世界尺寸 {width}x{height} 不能被噪声周期整除")
            }
            WorldError::ChunkOutOfRange => write!(f, "分块索引超出世界范围"),
        }
    }
}

impl core::error::Error for WorldError {}
