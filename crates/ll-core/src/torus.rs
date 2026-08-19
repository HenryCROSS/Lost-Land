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
///
/// `serde` 派生由同名 feature 开关（默认关闭）：见 `ll-core` 的
/// `Cargo.toml` 顶部说明。`WorldState` 需要它才能完整序列化。
///
/// # 反序列化必须重新经过 `new` 的校验（裁定 P2-6）
///
/// 若直接派生 `Deserialize`，serde 会绕过私有字段的访问控制、绕过
/// [`TorusSize::new`] 直接填字段。存档是外部不可信输入——玩家会手改、
/// 文件会损坏、旧版本存档可能带来意料之外的值——零尺寸的 `TorusSize`
/// 一旦这样混进来，[`TorusSize::wrap`] 里的 `rem_euclid` 会直接除零
/// panic。因此这里没有直接派生 `Deserialize`，而是用 `#[serde(try_from
/// = "TorusSizeRepr")]` 让反序列化必经一次 [`TorusSize::new`] 调用，
/// 不给绕过的余地。`Serialize` 不受影响，仍是直接派生——序列化只是把
/// 已经合法的值写出去，没有校验可绕。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "TorusSizeRepr"))]
pub struct TorusSize {
    width: u32,
    height: u32,
}

/// [`TorusSize`] 反序列化的中转表示，仅在 `serde` feature 下存在。
///
/// 见 [`TorusSize`] 文档「反序列化必须重新经过 `new` 的校验」一节：
/// 这个类型本身没有任何不变式，只是让 serde 有一个「先落地成普通字段，
/// 再交给 [`TryFrom`] 校验」的中转落点。
#[cfg(feature = "serde")]
#[derive(serde::Deserialize)]
struct TorusSizeRepr {
    width: u32,
    height: u32,
}

#[cfg(feature = "serde")]
impl TryFrom<TorusSizeRepr> for TorusSize {
    type Error = &'static str;

    /// 唯一的构造路径：委托给 [`TorusSize::new`]，宽高非零与不超过
    /// [`TorusSize::MAX_EXTENT`] 这两条校验因此对反序列化同样生效。
    fn try_from(raw: TorusSizeRepr) -> Result<Self, Self::Error> {
        TorusSize::new(raw.width, raw.height).ok_or("世界尺寸的宽高必须为正，且不超过 MAX_EXTENT")
    }
}

/// 环面世界上的一个位置。
///
/// 不变式：坐标恒被规范化到 `[0, width) × [0, height)`。字段私有以保证
/// 该不变式无法从外部破坏——只能经 [`TorusSize::wrap`] 构造。
///
/// `Ord`/`PartialOrd` 按 `(x, y)` 字典序派生（字段声明顺序即比较顺序）
/// ——纯粹是为了给区块索引、常驻集合快照这类需要确定性排序键的场景
/// （C5：禁止 `HashMap`/`HashSet` 迭代顺序参与逻辑判断）提供一个稳定
/// 排序依据，**不赋予任何游戏逻辑含义**：`(5, 0) < (0, 5)` 不代表前者
/// 在游戏世界里更「小」或更靠前，只是一个可复现的排序结果。
///
/// # 为什么可以直接派生 `Serialize`/`Deserialize`（不需要 [`TorusSize`] 那样的 `try_from`）
///
/// 与 [`crate::ident::ContentIndex`] 同一个理由（见其文档「为什么可以
/// 直接派生」一节）：`TorusPos` 自身没有任何**不依赖外部上下文**就能
/// 判断对不对的不变式——「坐标是否落在 `[0, width) × [0, height)` 内」
/// 只有配上产出它的那个 [`TorusSize`] 才有意义，脱离这个上下文，任意
/// `(i32, i32)` 都是一个结构上合法的 `TorusPos`。因此这里只做结构转换，
/// 不校验；真正的「这个坐标相对当前世界/网格是否越界」交给持有具体
/// 尺寸上下文的调用方在读取时做交叉校验（例如
/// `ll_world::state::WorldState` 反序列化时校验 `terrain.world() == size`
/// 的同一种手法）。
///
/// 这个 derive 是区块流式加载与 `Interior` 锚点（`coordinate-system-
/// and-layers.md` 批次 C）第一次真正需要 `TorusPos` 独立于 `WorldState`
/// 完整序列化——此前 `ll_world::state` 模块文档明确记录过这个缺口
/// （`Agent::pos` 因为「`ll-core` 里没有为它提供可脱离该上下文使用的
/// serde 实现」而被迫 `#[serde(skip)]`），本次补上。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    fn 环面坐标按横纵坐标字典序排序() {
        // 新增 Ord：只用于 C5 要求的确定性排序场景，不代表游戏逻辑上的
        // 「大小」。这里只验证字典序本身——x 小的排前面；x 相同时看 y。
        // Arrange
        let world = size();
        let smaller_x = world.wrap(0, 5);
        let larger_x = world.wrap(5, 0);
        let same_x_smaller_y = world.wrap(3, 1);
        let same_x_larger_y = world.wrap(3, 2);

        // Act & Assert
        assert!(smaller_x < larger_x);
        assert!(same_x_smaller_y < same_x_larger_y);
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

    // 裁定 P2-6：反序列化必须重新经过 TorusSize::new 的校验，见本文件
    // 顶部 TorusSize 文档「反序列化必须重新经过 new 的校验」一节。
    // 这三条测试只在 serde feature 开启时编译——不开启这个 feature，
    // serde::Deserialize 这个 trait 根本不存在，无从测起。
    #[cfg(feature = "serde")]
    mod serde_tests {
        use super::*;

        #[test]
        fn 零宽度的世界尺寸无法反序列化() {
            // 直接构造中转表示的 JSON，绕过 TorusSize::new 的入口，
            // 模拟被篡改或损坏的存档。
            // Arrange
            let json = r#"{"width":0,"height":10}"#;

            // Act
            let result: Result<TorusSize, _> = serde_json::from_str(json);

            // Assert
            assert!(result.is_err());
        }

        #[test]
        fn 零高度的世界尺寸无法反序列化() {
            // Arrange
            let json = r#"{"width":10,"height":0}"#;

            // Act
            let result: Result<TorusSize, _> = serde_json::from_str(json);

            // Assert
            assert!(result.is_err());
        }

        #[test]
        fn 合法的世界尺寸可以正常往返() {
            // Arrange
            let original = TorusSize::new(43, 25).expect("43x25 是合法的 TorusSize");

            // Act
            let json = serde_json::to_string(&original).expect("合法值必然可序列化");
            let decoded: TorusSize = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

            // Assert
            assert_eq!(decoded, original);
        }

        #[test]
        fn torus坐标可以正常往返() {
            // TorusPos 直接派生（不经过 try_from 中转），见其文档「为什么
            // 可以直接派生」——这里验证结构转换本身确实往返成立。
            // Arrange
            let world = TorusSize::new(10, 10).expect("10x10 是合法尺寸");
            let original = world.wrap(3, 7);

            // Act
            let json = serde_json::to_string(&original).expect("TorusPos 必然可序列化");
            let decoded: TorusPos = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

            // Assert
            assert_eq!(decoded, original);
        }
    }
}
