//! 整数可平铺值噪声。
//!
//! # 为什么自研而不用 `noise` crate（已查源码实测评估）
//!
//! 先澄清一个常见误解：**并非所有浮点运算都跨平台不确定**。IEEE 754
//! 规定 `+ - * / sqrt` 必须逐位精确，Rust 不开 fast-math、LLVM 也不会
//! 重排浮点运算（浮点加法不满足结合律），因此只用四则运算的浮点噪声
//! 跨平台是确定的。真正不确定的是 `sin`/`cos`/`exp`/`pow` 这类超越
//! 函数——IEEE 754 没有规定它们的精度，不同 libm 实现结果不同。
//!
//! 查过 `noise` 0.9.0 源码：Perlin / Simplex / Value 的核心实现**不含
//! 超越函数**（只有 Worley 用了 `powf`，本项目不会用）。所以它的确定性
//! 本身不是问题。
//!
//! 不采用它的三条真实理由：
//!
//! 1. **它的 seamless 是「四角混合」而非真无缝**。源码里取 sw/se/nw/ne
//!    四个采样值做混合——边缘能对上，但中间会出现可见的糊化。真环面
//!    拓扑需要的是构造上的无缝。
//! 2. **它是整图构建器而非逐坐标采样器**。`PlaneMapBuilder` 一次生成
//!    整张噪声图，而本项目是分块惰性生成——玩家走到哪生成哪。用它就得
//!    开局把整个世界算完，与 LOD 和惰性追赶的设计直接冲突。
//! 3. **输出是 `f64`，仍需量化成整数**，多一道转换与一处边界舍入的坑。
//!
//! # 为什么用模格点而不是 4D 投影
//!
//! 让噪声在环面上无缝的另一种常见做法，是把 2D 环面嵌入 4D 空间再采样
//! 4D 噪声。**那条路需要 `sin`/`cos`——正是上面说的超越函数**，反而会
//! 真正引入跨平台不确定性。
//!
//! 模格点方案不需要绕：格点索引对周期取模即可。`lattice_x mod period_x`
//! 使 `x = 0` 与 `x = period_x * CELL_SIZE` 命中同一格点，**无缝是构造
//! 上保证的**，不是近似出来的，且全程整数。
//!
//! 代价：世界宽高必须是 `period * CELL_SIZE`，该约束由
//! [`crate::generate`] 在入口校验。

use ll_core::rng::DetRng;

/// 一个噪声格子覆盖多少瓦片。
///
/// 取 16 使一个格子约占视口宽度的四十分之一，地形起伏的尺度肉眼舒适。
pub const CELL_SIZE: i32 = 16;

/// 采样值的上界（含）。用千分制与项目其余部分的比例表达保持一致。
const SCALE_MAX: i32 = 1000;

/// 在环面上无缝平铺的整数值噪声。
#[derive(Debug, Clone, Copy)]
pub struct TileableNoise {
    seed: u64,
    period_x: u32,
    period_y: u32,
}

impl TileableNoise {
    /// 建立噪声源。周期以**格点数**计，任一为零时返回 [`None`]。
    pub fn new(seed: u64, period_x: u32, period_y: u32) -> Option<Self> {
        if period_x == 0 || period_y == 0 {
            return None;
        }
        Some(TileableNoise {
            seed,
            period_x,
            period_y,
        })
    }

    /// 取某个格点的伪随机值，落在 `0..=SCALE_MAX`。
    ///
    /// 格点索引先对周期取模——这正是无缝性的来源。
    fn lattice_value(&self, lattice_x: i32, lattice_y: i32) -> i32 {
        let wrapped_x = lattice_x.rem_euclid(self.period_x as i32) as u64;
        let wrapped_y = lattice_y.rem_euclid(self.period_y as i32) as u64;

        // 把二维格点索引打包成一个 u64 喂给确定性 RNG。用移位而非相加，
        // 否则 (3, 5) 与 (5, 3) 会撞进同一个值，地形出现对角线状伪影。
        let packed = (wrapped_x << 32) | wrapped_y;
        let mut rng = DetRng::for_entity(self.seed, packed, 0);
        rng.gen_range(SCALE_MAX as u64 + 1) as i32
    }

    /// 五次多项式平滑，输入输出均为 `0..=SCALE_MAX` 的千分比。
    ///
    /// 用 `6t⁵ − 15t⁴ + 10t³`（Perlin 的改进插值）而非线性插值：线性
    /// 插值会在格点处留下可见的方格棱线。中间用 `i64` 承接以免立方溢出。
    fn smooth(t: i32) -> i32 {
        let t = t as i64;
        let s = SCALE_MAX as i64;
        let t3 = t * t * t;
        let numerator = t3 * (t * (t * 6 - 15 * s) + 10 * s * s);
        (numerator / (s * s * s * s)) as i32
    }

    /// 两个整数之间按千分比 `t` 插值。
    fn lerp(a: i32, b: i32, t: i32) -> i32 {
        a + ((b - a) as i64 * t as i64 / SCALE_MAX as i64) as i32
    }

    /// 在给定瓦片坐标处采样，返回 `0..=SCALE_MAX`。
    pub fn sample(&self, x: i32, y: i32) -> i32 {
        let lattice_x = x.div_euclid(CELL_SIZE);
        let lattice_y = y.div_euclid(CELL_SIZE);

        // 格内偏移换算成千分比供插值使用。
        let frac_x = x.rem_euclid(CELL_SIZE) * SCALE_MAX / CELL_SIZE;
        let frac_y = y.rem_euclid(CELL_SIZE) * SCALE_MAX / CELL_SIZE;

        let smooth_x = Self::smooth(frac_x);
        let smooth_y = Self::smooth(frac_y);

        let top_left = self.lattice_value(lattice_x, lattice_y);
        let top_right = self.lattice_value(lattice_x + 1, lattice_y);
        let bottom_left = self.lattice_value(lattice_x, lattice_y + 1);
        let bottom_right = self.lattice_value(lattice_x + 1, lattice_y + 1);

        let top = Self::lerp(top_left, top_right, smooth_x);
        let bottom = Self::lerp(bottom_left, bottom_right, smooth_x);
        Self::lerp(top, bottom, smooth_y).clamp(0, SCALE_MAX)
    }

    /// 多倍频叠加，返回 `0..=SCALE_MAX`。
    ///
    /// 每层频率翻倍、振幅减半，叠出细节层次。`octaves` 为零时退化为单次
    /// 采样——比返回零更符合直觉，也让调用方不必特判。
    pub fn octaves(&self, x: i32, y: i32, octaves: u32) -> i32 {
        if octaves == 0 {
            return self.sample(x, y);
        }

        let mut total: i64 = 0;
        let mut amplitude: i64 = SCALE_MAX as i64;
        let mut total_amplitude: i64 = 0;
        let mut frequency: i32 = 1;

        for _ in 0..octaves {
            // 频率翻倍可能让坐标溢出，用 checked 乘法在溢出前停止叠加。
            let Some(sx) = x.checked_mul(frequency) else {
                break;
            };
            let Some(sy) = y.checked_mul(frequency) else {
                break;
            };

            total += self.sample(sx, sy) as i64 * amplitude;
            total_amplitude += amplitude;
            amplitude /= 2;

            // 振幅衰减到零后继续叠加没有意义。
            if amplitude == 0 {
                break;
            }
            let Some(next) = frequency.checked_mul(2) else {
                break;
            };
            frequency = next;
        }

        (total / total_amplitude.max(1)).clamp(0, SCALE_MAX as i64) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise() -> TileableNoise {
        TileableNoise::new(0x1234_5678, 8, 8).expect("周期非零")
    }

    #[test]
    fn 采样值恒落在闭区间零到一千内() {
        // 下游按千分比使用这个值，超出区间会让地形阈值判断全部失效。
        // Arrange
        let noise = noise();

        // Act & Assert
        for y in -50..50 {
            for x in -50..50 {
                assert!((0..=SCALE_MAX).contains(&noise.sample(x, y)));
            }
        }
    }

    #[test]
    fn 相同坐标恒得相同采样值() {
        // 确定性是地形可复现的前提。
        // Arrange
        let noise = noise();

        // Act
        let first = noise.sample(17, 42);
        let second = noise.sample(17, 42);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 不同种子产出不同采样值() {
        // Arrange
        let a = TileableNoise::new(1, 8, 8).expect("周期非零");
        let b = TileableNoise::new(2, 8, 8).expect("周期非零");

        // Act & Assert
        assert_ne!(a.sample(10, 10), b.sample(10, 10));
    }

    #[test]
    fn 周期为零时构造失败() {
        // 周期为零会在取模时除零。
        // Arrange & Act
        let result = TileableNoise::new(1, 0, 8);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn 多倍频结果仍落在闭区间零到一千内() {
        // 倍频叠加最容易出的错就是没归一化回原区间。
        // Arrange
        let noise = noise();

        // Act & Assert
        for x in -30..30 {
            assert!((0..=SCALE_MAX).contains(&noise.octaves(x, x * 3, 4)));
        }
    }

    #[test]
    fn 倍频数为零时退化为单次采样() {
        // 返回零会让调用方不得不特判，退化更符合直觉。
        // Arrange
        let noise = noise();

        // Act & Assert
        assert_eq!(noise.octaves(5, 7, 0), noise.sample(5, 7));
    }

    #[test]
    fn 五次多项式平滑在下边界返回零() {
        // sample() 里传给 smooth 的 frac 由 rem_euclid(CELL_SIZE) *
        // SCALE_MAX / CELL_SIZE 算出，整数截断下最大只到 937，永远碰
        // 不到上边界——必须直接调用 smooth 才能锁住 0 这一端。
        // Arrange & Act
        let value = TileableNoise::smooth(0);

        // Assert
        assert_eq!(value, 0);
    }

    #[test]
    fn 五次多项式平滑在上边界返回上界值() {
        // 理由同上一条：sample() 的间接调用永远到不了 SCALE_MAX 这一端，
        // 必须直接调用 smooth 才能验证它。
        // Arrange & Act
        let value = TileableNoise::smooth(SCALE_MAX);

        // Assert
        assert_eq!(value, SCALE_MAX);
    }
}
