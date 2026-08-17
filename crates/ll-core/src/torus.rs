//! 环面（torus）拓扑的坐标与距离。
//!
//! 大陆世界地图四面全连通：向东走出边界会从西侧回来，南北同理。
//! 因此**两点之间存在四条候选路径**，真实距离是其中最短的一条。
//!
//! # 为什么必须用本模块而不能手写距离
//!
//! 只要项目中有任何一处写了普通的欧氏距离，就会出现「小地图上明明
//! 相邻、寻路却绕了半个世界」这类缺陷——而且极难定位，因为出错的
//! 地方看起来完全正常。该约束**自 P1 起**由 CI 静态检查强制；在此之前
//! 由人工评审把关（规格 §7.1）。
//!
//! # 适用范围
//!
//! 环面拓扑**仅适用于大陆世界地图层**。进入具体区域后的分区场景是
//! 有界局部地图，四周由地形自然收边，不做环绕。

/// 环面世界的尺寸，同时充当该世界所有坐标运算的上下文。
///
/// 距离与位移是尺寸的方法而非坐标的方法，因为脱离世界尺寸，两个环面
/// 坐标之间的距离根本无法定义。这个 API 形状让「忘记传尺寸」变成编译
/// 错误，而不是运行时的错误答案。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TorusSize {
    width: u32,
    height: u32,
}

/// 环面世界上的一个位置。
///
/// 不变式：坐标恒被规范化到 `[0, width) × [0, height)`。字段私有以保证
/// 该不变式无法从外部破坏——只能经 [`TorusSize::wrap`] 构造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TorusPos {
    x: i32,
    y: i32,
}

impl TorusPos {
    /// 规范化后的横坐标，恒在 `[0, width)` 内。
    pub const fn x(&self) -> i32 {
        self.x
    }

    /// 规范化后的纵坐标，恒在 `[0, height)` 内。
    pub const fn y(&self) -> i32 {
        self.y
    }
}

impl TorusSize {
    /// 环面尺寸的单维上限。
    ///
    /// 内部运算把宽高转为 `i32` 使用，且 `shortest_offset` 内有 `forward * 2`，
    /// 故上限取 `i32::MAX` 的一半：超过它会静默产生错误的绕回结果，
    /// 而静默错误正是本模块最该防的那一类。该上限远超任何可玩世界尺寸。
    pub const MAX_EXTENT: u32 = (i32::MAX / 2) as u32;

    /// 构造世界尺寸。任一维度为零或超过 [`Self::MAX_EXTENT`] 时返回 [`None`]。
    ///
    /// 零尺寸世界无法定义取模运算；过大尺寸会在内部 `as i32` 转换时静默溢出。
    /// 与其在运行时给出错误答案，不如在构造点就拒绝。
    pub const fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        if width > Self::MAX_EXTENT || height > Self::MAX_EXTENT {
            return None;
        }
        Some(TorusSize { width, height })
    }

    /// 世界宽度。
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// 世界高度。
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// 把任意整数坐标绕回世界范围内。
    pub fn wrap(&self, x: i32, y: i32) -> TorusPos {
        TorusPos {
            // rem_euclid 而非 %：Rust 的 % 对负数返回负余数，
            // -3 % 10 得 -3 而非 7，会直接破坏不变式。
            x: x.rem_euclid(self.width as i32),
            y: y.rem_euclid(self.height as i32),
        }
    }

    /// 从 `from` 到 `to` 的最短带符号位移。
    ///
    /// 返回值可正可负，表示应朝哪个方向走以及走多远。当正反两向等长时
    /// 固定取正方向——**必须有稳定的打破平局规则**，否则同一局面在不同
    /// 调用间可能返回不同结果，破坏确定性重放。
    pub fn delta(&self, from: TorusPos, to: TorusPos) -> (i32, i32) {
        (
            Self::shortest_offset(to.x - from.x, self.width as i32),
            Self::shortest_offset(to.y - from.y, self.height as i32),
        )
    }

    /// 单轴上的最短带符号位移。
    fn shortest_offset(raw: i32, extent: i32) -> i32 {
        let forward = raw.rem_euclid(extent);
        // 若正向距离超过半周，则反向更近。用 `>` 而非 `>=` 使恰好半周时
        // 取正方向，即上文的打破平局规则。
        if forward * 2 > extent {
            forward - extent
        } else {
            forward
        }
    }

    /// 切比雪夫距离：允许八方向移动时的步数。
    ///
    /// 这是瓦片地图上最常用的度量——斜走一步与直走一步代价相同。
    pub fn chebyshev(&self, a: TorusPos, b: TorusPos) -> u32 {
        let (dx, dy) = self.delta(a, b);
        dx.unsigned_abs().max(dy.unsigned_abs())
    }

    /// 曼哈顿距离：仅允许四方向移动时的步数。
    pub fn manhattan(&self, a: TorusPos, b: TorusPos) -> u32 {
        let (dx, dy) = self.delta(a, b);
        dx.unsigned_abs() + dy.unsigned_abs()
    }

    /// 欧氏距离的平方。
    ///
    /// 刻意只提供平方值而不开方：开方会引入浮点，而世界状态禁用浮点。
    /// 比较远近时平方值与原值单调等价，绝大多数场景不需要开方。
    pub fn squared_euclidean(&self, a: TorusPos, b: TorusPos) -> u64 {
        let (dx, dy) = self.delta(a, b);
        let dx = dx as i64;
        let dy = dy as i64;
        (dx * dx + dy * dy) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size() -> TorusSize {
        TorusSize::new(10, 10).expect("10x10 是合法尺寸")
    }

    #[test]
    fn 坐标超出范围时绕回世界内() {
        // Arrange
        let world = size();

        // Act
        let wrapped = world.wrap(12, -3);

        // Assert
        assert_eq!((wrapped.x(), wrapped.y()), (2, 7));
    }

    #[test]
    fn 位移在跨越接缝时取较短一侧() {
        // 从 x=9 到 x=1，向东绕 2 步，向西走 8 步，应取 +2。
        // Arrange
        let world = size();
        let from = world.wrap(9, 0);
        let to = world.wrap(1, 0);

        // Act
        let (dx, _dy) = world.delta(from, to);

        // Assert
        assert_eq!(dx, 2);
    }

    #[test]
    fn 位移恰为半周时固定取正方向() {
        // 正反两向等长，必须有稳定的打破平局规则，否则同一局面在不同
        // 调用间可能返回不同结果，破坏确定性。
        // Arrange
        let world = size();
        let from = world.wrap(0, 0);
        let to = world.wrap(5, 0);

        // Act
        let (dx, _dy) = world.delta(from, to);

        // Assert
        assert_eq!(dx, 5);
    }

    #[test]
    fn 尺寸为零时构造失败() {
        // Arrange & Act
        let result = TorusSize::new(0, 10);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn 切比雪夫距离在对角跨接缝时取较大轴() {
        // Arrange
        let world = size();
        let a = world.wrap(9, 9);
        let b = world.wrap(1, 0);

        // Act
        let distance = world.chebyshev(a, b);

        // Assert
        assert_eq!(distance, 2);
    }

    #[test]
    fn 尺寸超过上限时构造失败() {
        // 超过上限的宽度在内部 as i32 转换时会变成负数，
        // 导致绕回结果静默出错，故必须在构造点拒绝。
        // Arrange & Act
        let result = TorusSize::new(TorusSize::MAX_EXTENT + 1, 10);

        // Assert
        assert!(result.is_none());
    }
}
