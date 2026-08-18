//! 精灵：图集中的一块矩形区域及其在世界中的摆放参数。
//!
//! # 尺寸解耦
//!
//! [`Footprint`]（逻辑占地格数）与 [`SpriteSize`] + [`Pivot`]（视觉像素
//! 尺寸、锚点）刻意分成三个独立类型，不合并成一个。重点目标的精灵是
//! 32×48 像素，却只占 2×2 格——它画得比自己占的地方高。若把「占几格」
//! 与「画多大」揉进同一个类型，这种表现就做不出来，而后期再拆会推翻
//! 整个批处理布局（规格 §12.1 点名此项不得延后）。

/// 瓦片边长（像素）。规格决策 6 固定为 16。
pub const TILE_SIZE: u32 = 16;

/// 精灵的**逻辑占地格数**。
///
/// 与 [`SpriteSize`]（视觉像素尺寸）刻意分开：重点目标的精灵是 32×48
/// 像素，但只占 2×2 格——它画得比自己占的地方高。若把两者合并成一个
/// 概念，这种表现就做不出来，而后期再拆会推翻整个批处理布局
/// （规格 §12.1 明确要求此项不得延后）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct Footprint {
    /// 横向占几格。
    pub width: u8,
    /// 纵向占几格。
    pub height: u8,
}

impl Footprint {
    /// 共占几格。
    pub const fn tile_count(&self) -> u32 {
        self.width as u32 * self.height as u32
    }
}

/// 精灵的**视觉像素尺寸**，与 [`Footprint`] 刻意分开，理由见模块文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteSize {
    /// 像素宽度。
    pub width: u16,
    /// 像素高度。
    pub height: u16,
}

/// 精灵图像内的锚点，相对图像左上角的像素偏移。
///
/// 世界坐标换算成屏幕坐标后，以此偏移定位精灵图像，使脚底（或其他
/// 设计选定的基准点）落在世界位置上，而不是图像左上角。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct Pivot {
    /// 锚点横向偏移。
    pub x: i16,
    /// 锚点纵向偏移。
    pub y: i16,
}

/// 绘制图层，构成 [`DrawOrder`] 的第一排序键。
///
/// 用具名常量而非枚举，是为了让存档/网络序列化后新增图层时无需迁移
/// 旧数据——数值本身即协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Layer(pub u8);

impl Layer {
    /// 地形。恒在最底层——同层内部仍按 `foot_y` 比较，只是地形自身
    /// 平铺不重叠，视觉上感知不到这层内部的排序差异。
    pub const TERRAIN: Layer = Layer(0);
    /// 装饰物（草丛、碎石等不参与遮挡逻辑但需要按 Y 排的物件）。
    pub const DECOR: Layer = Layer(1);
    /// 实体（角色、怪物、可交互物件）。
    pub const ENTITY: Layer = Layer(2);
    /// 特效（技能光效、粒子）。
    pub const EFFECT: Layer = Layer(3);
    /// UI 元素（血条、名字牌等挂在世界坐标上的界面）。
    pub const UI: Layer = Layer(4);
}

/// 绘制顺序键。
///
/// 字段顺序即比较优先级：先图层，再脚底纵坐标，最后实体号。
///
/// **必须用脚底纵坐标而非精灵原点**：用原点会让高精灵在视觉上错误地
/// 挡住前排单位。
///
/// **必须有实体号作第二排序键**：否则同一世界状态可能画出不同的遮挡
/// 关系，既是视觉抖动，也会让视觉回归测试无法冻结基准。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DrawOrder {
    layer: Layer,
    foot_y: i32,
    entity: u64,
}

impl DrawOrder {
    /// 构造绘制顺序键。
    ///
    /// `foot_y` 应为精灵脚底（而非图像原点）的**屏幕**纵坐标（相机
    /// 相对），不是世界纵坐标。
    ///
    /// 必须用屏幕坐标：环面世界里 `y = 世界高度 − 1` 与 `y = 0` 在屏幕上
    /// 相邻却相差整个世界高度，用世界坐标会让跨南北接缝的排序反转，
    /// 接缝北侧的单位被南侧的错误遮挡。
    ///
    /// 屏幕坐标由 `Camera::world_to_screen` 得出，它已处理环面最短位移，
    /// 因此接缝两侧的相邻格在屏幕上也相邻。
    ///
    /// `entity` 是稳定的实体标识，用于在图层与纵坐标都相同时打破平局。
    pub const fn new(layer: Layer, foot_y: i32, entity: u64) -> DrawOrder {
        DrawOrder {
            layer,
            foot_y,
            entity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 图层优先于纵坐标决定绘制顺序() {
        // 地形永远在实体之下，无论各自纵坐标如何。
        // Arrange
        let terrain_low = DrawOrder::new(Layer::TERRAIN, 999, 1);
        let entity_high = DrawOrder::new(Layer::ENTITY, 0, 2);

        // Act & Assert
        assert!(terrain_low < entity_high);
    }

    #[test]
    fn 同层内纵坐标小的先绘制() {
        // 靠上方的单位先画，才会被下方单位遮住，形成正确的前后关系。
        // Arrange
        let near = DrawOrder::new(Layer::ENTITY, 100, 1);
        let far = DrawOrder::new(Layer::ENTITY, 50, 2);

        // Act & Assert
        assert!(far < near);
    }

    #[test]
    fn 跨接缝的两个实体按屏幕纵坐标排序() {
        // 环面世界里 y=世界高度-1 与 y=0 在屏幕上相邻。若排序键用世界
        // 纵坐标，接缝北侧的单位会被南侧的错误遮挡。
        // Arrange：屏幕坐标下北侧单位 y 更小（更靠上，应先绘制）。
        let north_on_screen = DrawOrder::new(Layer::ENTITY, 100, 1);
        let south_on_screen = DrawOrder::new(Layer::ENTITY, 116, 2);

        // Act & Assert
        assert!(north_on_screen < south_on_screen);
    }

    #[test]
    fn 同层同纵坐标时按实体号打破平局() {
        // 必须有稳定的第二排序键，否则同一世界状态可能画出不同的遮挡
        // 关系——既是视觉抖动，也会让视觉回归测试无法冻结基准。
        // Arrange
        let first = DrawOrder::new(Layer::ENTITY, 100, 7);
        let second = DrawOrder::new(Layer::ENTITY, 100, 8);

        // Act & Assert
        assert!(first < second);
    }

    #[test]
    fn 普通单位占一格() {
        // Arrange
        let footprint = Footprint {
            width: 1,
            height: 1,
        };

        // Act & Assert
        assert_eq!(footprint.tile_count(), 1);
    }

    #[test]
    fn 重点目标占四格() {
        // 32×48 的精灵占 2×2 格却画得比格子高——这正是尺寸解耦的意义。
        // Arrange
        let footprint = Footprint {
            width: 2,
            height: 2,
        };

        // Act & Assert
        assert_eq!(footprint.tile_count(), 4);
    }
}
