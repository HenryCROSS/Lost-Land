//! 区块坐标换算——`ZoneLayout` 与瓦片坐标 ↔ 区块坐标的互逆转换。
//!
//! 落地设计文档五节「核心机制：全局连续噪声场的窗口采样」的前半段：
//! 区块只是全局连续噪声场的一个采样窗口，区块坐标与世界瓦片坐标是
//! 同一个环面的两种分辨率。真正调用噪声源、写入 [`crate::chunk::ChunkGrid`]
//! 的窗口化生成入口（[`crate::generate::generate_zone_window`]）放在
//! `generate.rs`——那两个底层函数（`terrain_at_coord`/`build_noise`）是
//! 模块私有的，只有同一个模块内新增的函数能直接调用，本模块因此只
//! 负责坐标换算这一半,不重复定义生成入口。
//!
//! # 两个 `(i, j)`：区块坐标与世界瓦片坐标
//!
//! `ZoneCoord`（区块坐标）与 `TorusPos`（世界瓦片坐标）是同一个类型在
//! 两种不同分辨率下的用法（设计文档三节）：区块坐标喂给区块级
//! [`TorusSize`]（`zone_count`），世界瓦片坐标喂给瓦片级 `TorusSize`
//! （[`ZoneLayout::tile_size`]）。两者不需要各自独立存储，是同一个环面
//! 的两种分辨率，`区块坐标 = 世界瓦片坐标 ÷ 区块边长`（整数除法），纯
//! 函数派生，不是第二个真相源——见 [`ZoneLayout::tile_to_zone`]。
//!
//! # 丙案：区块即存储单位（原甲案已被取代）
//!
//! 早期版本要求「区块尺寸必须是 `CHUNK_SIZE` 的整数倍」（甲案，设计
//! 文档十节）：区块内部当时按 32×32 分块存储。丙案取消了这层存储块
//! （见 [`crate::chunk`] 模块文档「为什么是单一 `Vec`，不再分块」）,
//! [`ZoneLayout::new`] 因此只保留「必须是 [`CELL_SIZE`] 的整数倍」这
//! 一条结构性约束（连续噪声无缝性的前提）；「区块具体多大、世界多少
//! 区块」仍然是可配置数值（设计文档十二节），由
//! [`ZoneLayout::default_config`] 给出一份内部自洽的默认值（128×128、
//! 48×32），调用方可以传别的值给 [`ZoneLayout::new`]。

use ll_core::torus::{TorusPos, TorusSize};
use serde::{Deserialize, Serialize};

use crate::WorldError;
use crate::noise::CELL_SIZE;
use crate::space::ZoneCoord;

/// 区块布局配置：区块边长（默认 128）+ 世界区块数（默认 48×32）。
///
/// 两者都是可配置数值，不是结构约束（见设计文档十二节）——真正不可
/// 违反的只有「区块边长必须是 [`CELL_SIZE`] 的整数倍」，由
/// [`Self::new`] 在构造点校验，构造成功之后的 `ZoneLayout` 恒满足
/// 这条约束，下游（[`crate::generate::generate_zone_window`]、
/// `SurfaceStore`）不需要重复校验。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ZoneLayoutRepr")]
pub struct ZoneLayout {
    zone_span: u32,
    zone_count: TorusSize,
}

/// [`ZoneLayout`] 反序列化的中转表示。
///
/// 见 [`ZoneLayout`] 文档：私有字段 + 校验构造函数的类型加 serde 须用
/// `try_from` 中转（ADR 0011），与 [`ll_core::torus::TorusSize`] 同一个
/// 模式——反序列化必须重新经过 [`ZoneLayout::new`] 的对齐校验,不能让
/// serde 绕过私有字段直接落地一个未经校验的区块边长。
#[derive(Deserialize)]
struct ZoneLayoutRepr {
    zone_span: u32,
    zone_count: TorusSize,
}

impl TryFrom<ZoneLayoutRepr> for ZoneLayout {
    type Error = String;

    fn try_from(repr: ZoneLayoutRepr) -> Result<Self, Self::Error> {
        ZoneLayout::new(repr.zone_span, repr.zone_count).map_err(|err| err.to_string())
    }
}

/// 有界地图最小视口跨度——与 [`crate::chunk::ChunkGrid::new`] 私有常量
/// `MIN_WORLD_WIDTH`/`MIN_WORLD_HEIGHT` 数值相同（43×25）。
///
/// # 为什么在这里重复一份常量而不是复用 `chunk.rs` 的
///
/// `chunk.rs` 里的这两个常量是私有的，没有必要为了让这一处校验复用
/// 它们而放宽其可见性——一个区块最终会被存进一个 `ChunkGrid`（关键
/// 设计判断 1：「区块 = 一个 ChunkGrid 实例」），若 `zone_span` 小于
/// 这个跨度，`ChunkGrid::new` 会在 `generate::generate_zone_window`
/// 内部返回 [`WorldError::WorldTooSmall`]——与其让这个失败推迟到生成
/// 那一刻才发生（生成入口因此不得不是 fallible 的，调用方每次都要
/// 处理一个「正常配置下不可能发生」的错误分支），不如在
/// [`ZoneLayout::new`] 构造点提前拒绝，让下游可以安全假设「一个构造
/// 成功的 `ZoneLayout`，它的每一个区块窗口都能生成成功」。
const MIN_ZONE_SPAN: u32 = 43;

impl ZoneLayout {
    /// 校验并构造区块布局。
    ///
    /// 失败情形：
    /// - `zone_span` 不是 [`CELL_SIZE`] 的整数倍（连续噪声无缝性的前提，
    ///   设计文档五节第一条——丙案取消存储块层之后，这是唯一的对齐
    ///   约束，见模块文档「丙案：区块即存储单位」）；
    /// - `zone_span` 小于 [`MIN_ZONE_SPAN`]（否则该区块对应的
    ///   `ChunkGrid` 构造不出来，见 [`MIN_ZONE_SPAN`] 文档）；
    /// - `zone_span * zone_count` 在任一维上超过
    ///   [`TorusSize::MAX_EXTENT`]（否则 [`Self::tile_size`] 无法构造）。
    pub fn new(zone_span: u32, zone_count: TorusSize) -> Result<Self, WorldError> {
        if zone_span < MIN_ZONE_SPAN || !zone_span.is_multiple_of(CELL_SIZE as u32) {
            return Err(WorldError::ZoneSpanNotAligned { zone_span });
        }

        let tile_width = u64::from(zone_span) * u64::from(zone_count.width());
        let tile_height = u64::from(zone_span) * u64::from(zone_count.height());
        if tile_width > u64::from(TorusSize::MAX_EXTENT)
            || tile_height > u64::from(TorusSize::MAX_EXTENT)
        {
            return Err(WorldError::ZoneSpanNotAligned { zone_span });
        }

        Ok(ZoneLayout {
            zone_span,
            zone_count,
        })
    }

    /// 设计文档十一节给出的默认配置：区块 128×128，世界 48×32 个区块
    /// （裁定 CS-2：这是数值，不是结构约束，可调）。
    pub fn default_config() -> Self {
        let zone_count = TorusSize::new(48, 32).expect("48x32 是合法的 TorusSize");
        ZoneLayout::new(128, zone_count).expect("128 满足全部对齐与跨度约束")
    }

    /// 区块边长（格）。
    pub const fn zone_span(&self) -> u32 {
        self.zone_span
    }

    /// 世界区块数，同时是区块坐标的取值范围（区块坐标本身构成一个
    /// 环面，见设计文档六节）。
    pub const fn zone_count(&self) -> TorusSize {
        self.zone_count
    }

    /// 单个区块内部使用的坐标上下文：一个 `zone_span × zone_span` 的
    /// `TorusSize`。区块内局部坐标（[`Self::tile_to_zone`] 的返回值、
    /// 窗口化生成写入 `ChunkGrid` 时用的坐标）都通过它构造，保证各处
    /// 用的是同一个尺寸上下文，不会互相漂移。
    pub fn local_size(&self) -> TorusSize {
        TorusSize::new(self.zone_span, self.zone_span)
            .expect("zone_span 已在 new() 中校验非零且不超过上限")
    }

    /// 世界瓦片总尺寸 = `zone_span * zone_count`，供需要瓦片级
    /// `TorusSize` 的调用方（如 minimap/`SurfaceStore`）派生使用，不
    /// 单独存一份（同一个环面的两种分辨率，见模块文档）。
    pub fn tile_size(&self) -> TorusSize {
        TorusSize::new(
            self.zone_span * self.zone_count.width(),
            self.zone_span * self.zone_count.height(),
        )
        .expect("溢出与上限已在 new() 中校验过")
    }

    /// 把一个世界瓦片坐标（喂给 [`Self::tile_size`] 规范化出来的
    /// `TorusPos`）换算成（所属区块坐标，区块内局部坐标）。
    ///
    /// 纯函数派生，互为逆运算：`zone.x() * zone_span + local.x() ==
    /// pos.x()`（`y` 同理）——区块坐标不需要独立存储，见模块文档「两个
    /// `(i, j)`」一节。
    pub fn tile_to_zone(&self, pos: TorusPos) -> (ZoneCoord, TorusPos) {
        // TorusPos 的不变式保证坐标恒非负，直接转 u32 不丢信息。
        let x = pos.x() as u32;
        let y = pos.y() as u32;

        let zone_x = (x / self.zone_span) as i32;
        let zone_y = (y / self.zone_span) as i32;
        let local_x = (x % self.zone_span) as i32;
        let local_y = (y % self.zone_span) as i32;

        let zone = self.zone_count.wrap(zone_x, zone_y);
        let local = self.local_size().wrap(local_x, local_y);
        (zone, local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用区块布局：边长 64（满足 `>=43`、是 16 与 32 的倍数），
    /// 2×1 个区块，凑出一个 128×64 的世界。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(2, 1).expect("2x1 是合法尺寸");
        ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
    }

    #[test]
    fn 瓦片坐标到区块坐标的换算与区块内局部坐标的换算互为逆运算() {
        // Arrange
        let layout = test_layout();
        let tile_size = layout.tile_size();
        let pos = tile_size.wrap(70, 40); // 落在第二个区块（x >= 64）内

        // Act
        let (zone, local) = layout.tile_to_zone(pos);
        let reconstructed_x = zone.x() * layout.zone_span() as i32 + local.x();
        let reconstructed_y = zone.y() * layout.zone_span() as i32 + local.y();

        // Assert
        assert_eq!((reconstructed_x, reconstructed_y), (pos.x(), pos.y()));
    }

    #[test]
    fn 区块边长不是cell_size整数倍时构造zonelayout失败() {
        // 50 大于最小视口跨度 43,但不是 CELL_SIZE(16) 的倍数——丙案
        // 取消存储块层之后,这是唯一剩下的对齐约束。
        // Arrange
        let zone_count = TorusSize::new(4, 4).expect("4x4 是合法尺寸");

        // Act
        let result = ZoneLayout::new(50, zone_count);

        // Assert
        assert!(matches!(result, Err(WorldError::ZoneSpanNotAligned { .. })));
    }

    #[test]
    fn 区块边长小于最小视口跨度时构造zonelayout失败() {
        // 32 是 CELL_SIZE 的整数倍,对齐关系满足,但小于视口所需的
        // 43×25 跨度——对应的 ChunkGrid 根本构造不出来,必须在这里
        // 提前拒绝,而不是留到窗口化生成内部才失败。
        // Arrange
        let zone_count = TorusSize::new(4, 4).expect("4x4 是合法尺寸");

        // Act
        let result = ZoneLayout::new(32, zone_count);

        // Assert
        assert!(matches!(result, Err(WorldError::ZoneSpanNotAligned { .. })));
    }

    #[test]
    fn 区块边长是cell_size整数倍但不是32的倍数时构造成功() {
        // 48 曾经在甲案下失败（不是 CHUNK_SIZE(32) 的整数倍）——丙案
        // 取消存储块层之后,这条对齐约束不再存在,48 只要满足 CELL_SIZE
        // 整数倍与最小视口跨度即可成功构造。这条测试直接锁定丙案生效
        // 后的新行为,防止将来有人不小心把 32 的倍数约束又加回来。
        // Arrange
        let zone_count = TorusSize::new(4, 4).expect("4x4 是合法尺寸");

        // Act
        let result = ZoneLayout::new(48, zone_count);

        // Assert
        assert!(result.is_ok());
    }
}
