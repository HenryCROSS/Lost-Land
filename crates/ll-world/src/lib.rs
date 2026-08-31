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
//!
//! # [`item`]（P6 第二批：背包与地面物品）
//!
//! 物品的运行时实例（[`item::ItemStack`]）与地面物品堆
//! （[`item::GroundItemStack`]）——从 `ll-sim` 挪到本模块，理由见该
//! 模块文档「为什么从 `ll-sim` 挪到本模块」一节：背包
//! （[`entity::Agent::inventory`]）与地面物品
//! （[`state::WorldState::ground_items`]）都是世界状态，必须定义在
//! `Agent`/`WorldState` 所在的本 crate，`ll-world` 不能依赖 `ll-sim`。

use core::fmt;

pub mod bounded_grid;
pub mod chronicle;
pub mod chunk;
pub mod climate;
pub mod culture;
pub mod entity;
pub mod exploration;
pub mod faction;
pub mod fov;
pub mod generate;
pub mod history;
pub mod interior;
pub mod item;
pub mod land;
pub mod light;
pub mod mod_state;
pub mod naming;
pub mod noise;
pub mod overview;
pub mod ownership;
pub mod resource;
pub mod settlement;
pub mod sight_residency;
pub mod space;
pub mod space_profile;
pub mod state;
pub mod surface_store;
pub mod temperature;
pub mod terrain;
pub mod terrain_shape;
pub mod weather;
pub mod world_map;
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
    /// 区块边长不满足对齐约束：必须是 [`crate::noise::CELL_SIZE`]
    /// （连续噪声无缝性的前提）的整数倍，且不小于视口所需的最小跨度
    /// （否则区块内部的 `ChunkGrid` 构造不出来）——见
    /// [`crate::zone::ZoneLayout::new`] 文档。
    ZoneSpanNotAligned {
        /// 实际传入的区块边长（格）。
        zone_span: u32,
    },
    /// 读档后未重新灌入 `terrain_table` 就试图使用世界。
    ///
    /// `terrain_table`（[`crate::state::WorldState::terrain_table`]）不
    /// 参与序列化——它是当前会话已加载 mod 集合的注册期产物，读档后
    /// 默认是空表。调用方必须在拿到当前会话重新注册出的表之后显式
    /// 替换它，见 [`crate::state::WorldState::assert_terrain_table_loaded`]
    /// 这个读档后置校验点。未灌入就直接使用会让地形查询全部退化成
    /// 安全兜底值，且不会有任何报错——这个变体把"灌没灌"从隐式的
    /// 静默正确变成显式的、必须处理的失败。
    TerrainTableNotReloaded,
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
                    "区块边长 {zone_span} 不满足对齐约束（须为 CELL_SIZE 的整数倍，且不小于最小视口跨度）"
                )
            }
            WorldError::TerrainTableNotReloaded => {
                write!(
                    f,
                    "读档后尚未用当前会话重新灌入 terrain_table，不能直接使用"
                )
            }
        }
    }
}

impl core::error::Error for WorldError {}
