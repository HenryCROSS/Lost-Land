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

/// 占地 `footprint` 格、左上角像素原点为 `tile_origin` 的图块，其锚点
/// （占地区域底边水平中点）在离屏渲染目标像素空间中的位置。
///
/// 无论是 1×1 的普通单位还是 2×2 的重点目标，锚点规则统一：脚站在
/// 占地格块的底边中点，而不是某个角上——这正是 [`Footprint`] 与
/// [`Pivot`] 解耦要支撑的表现（见模块文档「尺寸解耦」一节）。
///
/// # 屏幕坐标系
///
/// 与本 crate 其余部分一致，`y` 沿屏幕向下递增（原点在离屏渲染目标
/// 左上角），不是数学课本里向上为正的约定——见 [`DrawOrder::new`] 文档
/// 「跨接缝排序」一节给出的具体例子（更靠北/更靠上的实体 `y` 更小）。
pub fn footprint_anchor_pixel(tile_origin: (i32, i32), footprint: Footprint) -> (i32, i32) {
    let half_width_px = footprint.width as i32 * TILE_SIZE as i32 / 2;
    let height_px = footprint.height as i32 * TILE_SIZE as i32;
    (tile_origin.0 + half_width_px, tile_origin.1 + height_px)
}

/// 把「占地锚点」（[`footprint_anchor_pixel`]）与「图像内锚点」
/// （[`Pivot`]）相减，得到精灵图像左上角应绘制在离屏渲染目标像素
/// 空间中的位置。
///
/// # 为什么高出格子的部分向上溢出，而不是向下
///
/// 占地锚点固定在**占地区域的底边**（脚落地的地方），`pivot` 记录的是
/// 「图像内哪一点应该对准这个锚点」——对角色精灵而言几乎总是图像底边
/// 的脚底。把图像左上角放在 `锚点 − pivot`，等价于「先让图像脚底对齐
/// 锚点，再把整张图像按它自身的宽高向左上方铺开」。
///
/// 因此，当图像比占地格子更高（例如 16×24 的普通单位站在 16×16 的
/// 格子里，或 32×48 的重点目标站在 2×2 格、32×32 像素的占地里），
/// 多出来的高度只能往 `y` 更小的方向铺——也就是屏幕上**向上**（本 crate
/// 的屏幕坐标系 `y` 向下递增，见 [`footprint_anchor_pixel`] 文档「屏幕
/// 坐标系」一节）。这符合直觉里「脚站在格子里，头探出格子顶部」的画面，
/// 而不是反过来让脚悬在格子外面、图像从格子顶部往下多画出一截：
/// 若有调用方误把图像左上角直接摆在格子左上角（`tile_origin` 本身），
/// 效果恰好相反——脚会悬空探出格子底边，头却缩在格子内部。`ll-sim`
/// 的 P5 坐标验收 demo 曾经就是这样：它的玩家标记绘制函数一度绕开这条
/// 换算、直接把 `tile_origin` 当绘制原点，脚底因此凸出格子下方，而不是
/// 头顶探出格子上方。
///
/// # 调用方不得自行重实现这条换算
///
/// 这条「占地锚点 − pivot」的算术曾经只存在于某一个验收 demo 的私有
/// 模块里，其余 demo 各自抄一份、新增的 demo 则完全遗漏——遗漏不会在
/// 编译期或大多数单测里暴露，因为大多数 demo 画的都是 1×1 小图或纯色
/// 块，凑巧看不出偏移方向反了。统一收进这里，是让「调用即正确」，不必
/// 每个下游都重新推导一遍这条反直觉的符号。
pub fn sprite_draw_position(
    tile_origin: (i32, i32),
    footprint: Footprint,
    pivot: Pivot,
) -> [f32; 2] {
    let (anchor_x, anchor_y) = footprint_anchor_pixel(tile_origin, footprint);
    [
        (anchor_x - pivot.x as i32) as f32,
        (anchor_y - pivot.y as i32) as f32,
    ]
}

/// 占地格块底边的**屏幕**纵坐标（像素），供 [`DrawOrder::new`] 的
/// `foot_y` 参数使用。
///
/// **必须用占地底边而非精灵图像顶部**：高精灵若用图像顶部排序，会在
/// 视觉上错误地挡住本该在它前面的矮单位——`foot_y` 应反映「脚站在
/// 哪一条线上」，与图像本身画得多高无关（图像越过格子顶部的部分不该
/// 参与遮挡排序）。
///
/// `screen_tile_y` 必须是占地左上角格的**屏幕**纵坐标（`Camera::world_to_screen`
/// 或 `BoundedCamera::world_to_screen` 的返回值），不能是世界纵坐标——
/// 环面世界里跨南北接缝时世界纵坐标的排序会反转，详见 [`DrawOrder::new`]
/// 文档「跨接缝排序」一节。
pub fn footprint_bottom_screen_y(screen_tile_y: i32, footprint_height: u8) -> i32 {
    screen_tile_y + footprint_height as i32 * TILE_SIZE as i32
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

    #[test]
    fn 单格占地锚点在格底边中点() {
        // Arrange
        let footprint = Footprint {
            width: 1,
            height: 1,
        };

        // Act
        let anchor = footprint_anchor_pixel((100, 200), footprint);

        // Assert
        assert_eq!(anchor, (100 + TILE_SIZE as i32 / 2, 200 + TILE_SIZE as i32));
    }

    #[test]
    fn 两格宽占地锚点横坐标取两格宽度中点() {
        // Arrange
        let footprint = Footprint {
            width: 2,
            height: 2,
        };

        // Act
        let anchor = footprint_anchor_pixel((100, 200), footprint);

        // Assert
        assert_eq!(anchor, (100 + TILE_SIZE as i32, 200 + 2 * TILE_SIZE as i32));
    }

    #[test]
    fn 十六乘二十四单格精灵绘制原点锁定实际坐标() {
        // 16×24 的 `hero_idle_0`、pivot (8, 24)、1×1 占地——与图集元数据
        // 实际取值一致（见 assets/atlas/placeholder.json）。这条测试锁的
        // 是具体数字，不是「比格顶更靠上」这类方向性断言：符号一旦被
        // 意外改成加号，这里必须变红。
        // Arrange
        let footprint = Footprint {
            width: 1,
            height: 1,
        };
        let pivot = Pivot { x: 8, y: 24 };

        // Act
        let draw_position = sprite_draw_position((100, 200), footprint, pivot);

        // Assert：锚点 (108, 216) 减 pivot (8, 24) = (100, 192)——绘制
        // 原点比占地顶部（200）高 8 像素，图像底边（192+24=216）恰好
        // 落在占地底边。
        assert_eq!(draw_position, [100.0, 192.0]);
    }

    #[test]
    fn 三十二乘四十八双格精灵绘制原点锁定实际坐标() {
        // 32×48 的 `boss_idle_0`、pivot (16, 48)、2×2 占地——与图集元数据
        // 实际取值一致。覆盖与上一条测试不同的 footprint/pivot 尺寸，
        // 确认换算对「大一号」的精灵同样正确，不是碰巧凑对了 16×24。
        // Arrange
        let footprint = Footprint {
            width: 2,
            height: 2,
        };
        let pivot = Pivot { x: 16, y: 48 };

        // Act
        let draw_position = sprite_draw_position((100, 200), footprint, pivot);

        // Assert：锚点 (116, 232) 减 pivot (16, 48) = (100, 184)——绘制
        // 原点比占地顶部（200）高 16 像素，图像底边（184+48=232）恰好
        // 落在 2×2 占地的底边。
        assert_eq!(draw_position, [100.0, 184.0]);
    }

    #[test]
    fn 占地底边屏幕纵坐标随占地高度增加() {
        // Arrange & Act
        let one_tile = footprint_bottom_screen_y(10, 1);
        let two_tiles = footprint_bottom_screen_y(10, 2);

        // Assert
        assert!(two_tiles > one_tile);
    }
}
