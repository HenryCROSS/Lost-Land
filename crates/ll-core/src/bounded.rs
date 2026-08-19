//! 有界局部坐标——供 `Interior`（地下城/建筑内部楼层）使用的非环绕坐标系。
//!
//! # 为什么不能复用 `TorusSize`/`TorusPos`
//!
//! [`crate::torus`] 的环面坐标在到达边界时绕回对侧——这对大陆世界地图
//! 是对的，但对一栋建筑内部、一处地下城这类彼此独立的有界局部地图是
//! 错的：若沿用环面的 `wrap`，当视野半径接近地图尺寸一半时会隔着地图
//! 两端相互看见（`TorusSize::wrap`/`squared_euclidean` 既有实现在有界
//! 场景下的直接推论，不是猜测）。见
//! `knowledge/design/coordinate-system-and-layers.md` 六节
//! 「`compute_fov` 是否真能原样复用」一节的核实结论。本模块提供一个
//! 形状与 `TorusSize`/`TorusPos` 类似、但坐标越界即拒绝、不提供任何
//! 「绕回」兜底的姊妹类型。
//!
//! # 与 `TorusSize` 的关键差异
//!
//! - [`TorusSize::wrap`](crate::torus::TorusSize::wrap) 对任意整数坐标
//!   恒成功（绕回世界内）；[`BoundedSize::try_pos`] 对越界坐标返回
//!   [`None`]——越界就是越界，没有「绕回」可以兜底，这正是有界局部
//!   地图（进了这扇门就是这扇门里的空间，走到墙边不会绕到墙的另一侧）
//!   与环面世界地图的本质区别。
//! - [`TorusSize::delta`](crate::torus::TorusSize::delta) 返回环面上的
//!   最短带符号位移（可能选择绕接缝的方向）；[`BoundedSize::delta`]
//!   就是两点的原始差值——有界地图没有接缝可绕，也就没有「走哪条路更
//!   近」这个二义性问题。

/// 有界局部地图的尺寸，同时充当该地图所有坐标运算的上下文。
///
/// 距离与位移是尺寸的方法而非坐标的方法，理由与
/// [`TorusSize`](crate::torus::TorusSize) 相同：脱离地图尺寸，两个坐标
/// 之间「是否越界」「距离多远」根本无法定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundedSize {
    width: u32,
    height: u32,
}

/// 有界局部地图上的一个位置。
///
/// 不变式：坐标恒在 `[0, width) × [0, height)` 内。字段私有以保证该
/// 不变式无法从外部破坏——只能经 [`BoundedSize::try_pos`] 构造，且越界
/// 输入直接返回 [`None`]，没有任何构造路径能产出越界的 `BoundedPos`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundedPos {
    x: i32,
    y: i32,
}

impl BoundedPos {
    /// 横坐标，恒在 `[0, width)` 内。
    pub const fn x(&self) -> i32 {
        self.x
    }

    /// 纵坐标，恒在 `[0, height)` 内。
    pub const fn y(&self) -> i32 {
        self.y
    }
}

impl BoundedSize {
    /// 有界地图的单维上限，与
    /// [`TorusSize::MAX_EXTENT`](crate::torus::TorusSize::MAX_EXTENT)
    /// 同一个理由：内部运算把宽高转为 `i32` 使用，超过这个上限会在
    /// 转换时静默出错，而静默错误正是本模块最该防的一类。
    pub const MAX_EXTENT: u32 = (i32::MAX / 2) as u32;

    /// 构造地图尺寸。任一维度为零或超过 [`Self::MAX_EXTENT`] 时返回
    /// [`None`]——零尺寸地图无法定义任何坐标，过大尺寸会在内部 `as i32`
    /// 转换时静默溢出。
    pub const fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        if width > Self::MAX_EXTENT || height > Self::MAX_EXTENT {
            return None;
        }
        Some(BoundedSize { width, height })
    }

    /// 地图宽度。
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// 地图高度。
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// 尝试把任意整数坐标构造成本地图内的一个位置。
    ///
    /// 越界（负数，或达到/超过 `width`/`height`）返回 [`None`]——与
    /// [`TorusSize::wrap`](crate::torus::TorusSize::wrap) 恒成功不同，
    /// 这里没有「绕回」可以兜底：越界就是越界，调用方必须显式处理
    /// 「这个方向走不通」，而不是被静默带到地图另一侧。
    pub fn try_pos(&self, x: i32, y: i32) -> Option<BoundedPos> {
        if x < 0 || y < 0 {
            return None;
        }
        if x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some(BoundedPos { x, y })
    }

    /// 从 `from` 到 `to` 的原始带符号位移。
    ///
    /// 不做最短路径计算——有界地图没有接缝可绕，`to` 相对 `from` 的
    /// 位移只有一种走法，不像环面那样存在「正向绕接缝更近还是反向
    /// 直走更近」的二义性。
    pub fn delta(&self, from: BoundedPos, to: BoundedPos) -> (i32, i32) {
        (to.x - from.x, to.y - from.y)
    }

    /// 欧氏距离的平方。
    ///
    /// 与 [`TorusSize::squared_euclidean`](crate::torus::TorusSize::squared_euclidean)
    /// 同一个理由只提供平方值：开方会引入浮点，而世界状态禁用浮点。
    pub fn squared_euclidean(&self, a: BoundedPos, b: BoundedPos) -> u64 {
        let (dx, dy) = self.delta(a, b);
        let dx = dx as i64;
        let dy = dy as i64;
        (dx * dx + dy * dy) as u64
    }

    /// 切比雪夫距离：允许八方向移动时的步数。
    pub fn chebyshev(&self, a: BoundedPos, b: BoundedPos) -> u32 {
        let (dx, dy) = self.delta(a, b);
        dx.unsigned_abs().max(dy.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size() -> BoundedSize {
        BoundedSize::new(10, 10).expect("10x10 是合法尺寸")
    }

    #[test]
    fn 尺寸为零时构造失败() {
        // Arrange & Act
        let result = BoundedSize::new(0, 10);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn 尺寸超过上限时构造失败() {
        // Arrange & Act
        let result = BoundedSize::new(BoundedSize::MAX_EXTENT + 1, 10);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn 负坐标返回none而非绕回() {
        // 有界地图没有「绕回」可以兜底：越界就是越界。
        // Arrange
        let map = size();

        // Act
        let pos = map.try_pos(-1, 3);

        // Assert
        assert!(pos.is_none());
    }

    #[test]
    fn 达到宽度上限的坐标返回none() {
        // width=10 时合法横坐标是 0..=9，x=10 已经越界——与
        // TorusSize::wrap 会把它绕回 0 不同，这里必须拒绝。
        // Arrange
        let map = size();

        // Act
        let pos = map.try_pos(10, 0);

        // Assert
        assert!(pos.is_none());
    }

    #[test]
    fn 合法坐标往返一致() {
        // Arrange
        let map = size();

        // Act
        let pos = map.try_pos(4, 7).expect("4,7 在 10x10 范围内");

        // Assert
        assert_eq!((pos.x(), pos.y()), (4, 7));
    }

    #[test]
    fn 位移不做环绕最短路径计算而是原始差值() {
        // 对照环面版本「位移在跨越接缝时取较短一侧」的测试：环面上
        // from=9 到 to=1 会取 dx=2（绕接缝更近）；有界地图没有接缝，
        // 必须老实返回 dx=-8。
        // Arrange
        let map = size();
        let from = map.try_pos(9, 0).expect("9,0 在范围内");
        let to = map.try_pos(1, 0).expect("1,0 在范围内");

        // Act
        let (dx, _dy) = map.delta(from, to);

        // Assert
        assert_eq!(dx, -8);
    }

    #[test]
    fn 切比雪夫距离取两轴中较大者() {
        // Arrange
        let map = size();
        let a = map.try_pos(0, 0).expect("0,0 在范围内");
        let b = map.try_pos(3, 5).expect("3,5 在范围内");

        // Act
        let distance = map.chebyshev(a, b);

        // Assert
        assert_eq!(distance, 5);
    }

    #[test]
    fn 欧氏距离平方按两轴差值计算() {
        // Arrange
        let map = size();
        let a = map.try_pos(0, 0).expect("0,0 在范围内");
        let b = map.try_pos(3, 4).expect("3,4 在范围内");

        // Act
        let squared = map.squared_euclidean(a, b);

        // Assert
        assert_eq!(squared, 25);
    }
}
