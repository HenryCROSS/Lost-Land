//! 迷途大陆的世界层。
//!
//! 承接 `ll-core` 的纯数据基础设施，落地成具体的世界状态：环面地形、
//! 分块存储、噪声生成、视野与光照、昼夜四季、居民（[`entity`] 的实体
//! 存储与 [`naming`] 的名字生成）。本 crate 不接触渲染或平台细节——
//! 那些属于 `ll-render`/`ll-platform`，世界层只产出数据，由上层决定
//! 怎么画。世界的**演化**（时间轴调度、`Intent → resolve → Effect →
//! apply`、战斗结算）属于下游的 `ll-sim`，不在本 crate——见规格 §5
//! 的依赖顺序：`ll-world` 在前，`ll-sim` 依赖 `ll-world`，反过来会
//! 成环。
//!
//! # 浮点边界
//!
//! 世界状态禁止浮点数：跨平台浮点差异会摧毁确定性存档与重放（详见
//! `ll-core` 的说明）。本 crate 的所有模块全程使用整数与定点数。

use core::fmt;

pub mod bounded_grid;
pub mod chunk;
pub mod entity;
pub mod fov;
pub mod generate;
pub mod light;
pub mod naming;
pub mod noise;
pub mod overview;
pub mod space;
pub mod space_profile;
pub mod state;
pub mod surface_store;
pub mod terrain;
pub mod zone;

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
    /// 用于地形生成入口（[`crate::generate::generate_terrain`]）的前置
    /// 校验：世界宽高必须都是 [`crate::noise::CELL_SIZE`] 的整数倍，
    /// 否则接缝处会出现不连续。
    WorldNotTileable {
        /// 实际宽度（格）。
        width: u32,
        /// 实际高度（格）。
        height: u32,
    },
    /// 区块边长不满足对齐约束：必须同时是
    /// [`crate::noise::CELL_SIZE`]（连续噪声无缝性的前提）与
    /// [`crate::chunk::CHUNK_SIZE`]（甲案「区块 = 4×4 存储块」）的整数
    /// 倍，且不小于视口所需的最小跨度（否则区块内部的 `ChunkGrid` 构造
    /// 不出来）——见 [`crate::zone::ZoneLayout::new`] 文档。
    ZoneSpanNotAligned {
        /// 实际传入的区块边长（格）。
        zone_span: u32,
    },
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
            WorldError::ZoneSpanNotAligned { zone_span } => {
                write!(
                    f,
                    "区块边长 {zone_span} 不满足对齐约束（须为 CELL_SIZE 与 CHUNK_SIZE 的整数倍，且不小于最小视口跨度）"
                )
            }
        }
    }
}

impl core::error::Error for WorldError {}
