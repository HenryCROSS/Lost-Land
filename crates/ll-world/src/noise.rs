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
//!
//! # 为什么 `octaves` 要从大陆尺度往细节叠加，而不是反过来
//!
//! 早期版本的 `octaves` 只在 [`CELL_SIZE`]（16 格）的基础上不断*加倍*
//! 频率（16、8、4、2 格的斑块），从未叠加过比 16 格更粗的层。实测过
//! 512×320 的默认生成结果：高度场的自相关在 d=16 格处已经跌到
//! 0.094——世界实际上是一堆边长 16 格、统计独立的碎斑块拼起来的，
//! 不存在任何比这更大尺度的结构，也就没有「大陆」可言。
//!
//! 修法是让最粗的一层覆盖与世界尺寸同量级的范围，而不是固定卡在
//! [`CELL_SIZE`]。[`TileableNoise`] 在构造时会算出 `period_x`/`period_y`
//! （已换算成 [`CELL_SIZE`] 格点数的世界周期）能同时被多大的 2 的幂
//! 整除——那正是「粗一层的格子边长再翻倍」还能保持两个轴都无缝平铺的
//! 上限，记作 `coarse_scale`。[`TileableNoise::octaves`] 就从
//! `CELL_SIZE * coarse_scale` 这个大陆尺度的格子开始采样、权重最高，
//! 逐层减半格子边长、减半权重，直到回到 [`CELL_SIZE`] 本身；此后若还有
//! 更多倍频，才继续往更细（比 [`CELL_SIZE`] 更小）的方向叠加，即原先
//! 「频率翻倍」那一段逻辑。
//!
//! 这个 `coarse_scale` 是从世界尺寸**自动推导**的，不是新增一条硬编码
//! 常量：世界越大，能用的大陆尺度就越大；世界尺寸恰好使
//! `period_x`/`period_y` 互质（没有公共的 2 的因子）时，`coarse_scale`
//! 退化为 1，`octaves` 也随之退化成早期版本的纯细节叠加——这是可接受
//! 的降级，不是需要额外校验拒绝的错误，世界尺寸的既有约束（必须是
//! [`CELL_SIZE`] 的整数倍）保持不变，没有新增任何前置校验。

use ll_core::rng::DetRng;

/// 一个噪声格子覆盖多少瓦片，同时也是 [`TileableNoise::octaves`] 里
/// **最细一层**的格子边长。
///
/// 取 16：地形起伏在这个尺度上肉眼舒适——不是「占视口宽度的某个分数」
/// （早前的文档在这里算错过一次：16 格其实约占 43 格视口宽度的五分之
/// 二，不是四十分之一；这条错误的比例描述后来被误认成了「一个尺度就
/// 够用」的理由，酿成大陆尺度结构缺失的缺陷，见模块文档）。世界的
/// 大陆尺度结构由 [`TileableNoise::octaves`] 自动推导的更粗一层负责，
/// 不由这个常量负责——调大或调小这个常量只影响细节颗粒度，不影响是否
/// 存在大陆尺度的起伏。
pub const CELL_SIZE: i32 = 16;

/// 采样值的上界（含）。用千分制与项目其余部分的比例表达保持一致。
const SCALE_MAX: i32 = 1000;

/// 在环面上无缝平铺的整数值噪声。
#[derive(Debug, Clone, Copy)]
pub struct TileableNoise {
    seed: u64,
    period_x: u32,
    period_y: u32,
    /// [`Self::octaves`] 最粗一层可用的格子放大倍数，恒为 2 的幂。
    /// 取值与推导理由见模块文档「为什么 `octaves` 要从大陆尺度往细节
    /// 叠加」一节；构造时算好存下来，避免每次调用 `octaves` 都重算一遍
    /// 最大公约数——那是地形生成的热路径，世界每一格都要调用一次。
    coarse_scale: u32,
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
            coarse_scale: max_pow2_divisor(gcd(period_x, period_y)),
        })
    }

    /// 取某个格点的伪随机值，落在 `0..=SCALE_MAX`。给定的 `period_x`/
    /// `period_y` 是该格点所在层的格点周期——粗一层的周期更短。
    ///
    /// 格点索引先对周期取模——这正是无缝性的来源。
    fn lattice_value(&self, lattice_x: i32, lattice_y: i32, period_x: u32, period_y: u32) -> i32 {
        let wrapped_x = lattice_x.rem_euclid(period_x as i32) as u64;
        let wrapped_y = lattice_y.rem_euclid(period_y as i32) as u64;

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

    /// 在给定瓦片坐标、给定格子边长 `cell_size` 与该层格点周期
    /// `period_x`/`period_y` 下采样，返回 `0..=SCALE_MAX`。
    ///
    /// [`Self::sample`] 与 [`Self::octaves`] 的每一层都经这个函数
    /// 求值，唯一的区别是三个参数——不同尺度共用同一套双线性格点
    /// 插值逻辑，没有理由为「粗一层」与「细一层」各写一份。
    fn sample_at_scale(&self, x: i32, y: i32, cell_size: i32, period_x: u32, period_y: u32) -> i32 {
        let lattice_x = x.div_euclid(cell_size);
        let lattice_y = y.div_euclid(cell_size);

        // 格内偏移换算成千分比供插值使用。
        let frac_x = (x.rem_euclid(cell_size) as i64 * SCALE_MAX as i64 / cell_size as i64) as i32;
        let frac_y = (y.rem_euclid(cell_size) as i64 * SCALE_MAX as i64 / cell_size as i64) as i32;

        let smooth_x = Self::smooth(frac_x);
        let smooth_y = Self::smooth(frac_y);

        let top_left = self.lattice_value(lattice_x, lattice_y, period_x, period_y);
        let top_right = self.lattice_value(lattice_x + 1, lattice_y, period_x, period_y);
        let bottom_left = self.lattice_value(lattice_x, lattice_y + 1, period_x, period_y);
        let bottom_right = self.lattice_value(lattice_x + 1, lattice_y + 1, period_x, period_y);

        let top = Self::lerp(top_left, top_right, smooth_x);
        let bottom = Self::lerp(bottom_left, bottom_right, smooth_x);
        Self::lerp(top, bottom, smooth_y).clamp(0, SCALE_MAX)
    }

    /// 在给定瓦片坐标处采样，返回 `0..=SCALE_MAX`。格子边长固定为
    /// [`CELL_SIZE`]，即 [`Self::octaves`] 倍频序列里最细的那一层。
    pub fn sample(&self, x: i32, y: i32) -> i32 {
        self.sample_at_scale(x, y, CELL_SIZE, self.period_x, self.period_y)
    }

    /// 多倍频叠加，返回 `0..=SCALE_MAX`。`octaves` 为零时退化为单次
    /// 采样——比返回零更符合直觉，也让调用方不必特判。
    ///
    /// 叠加顺序**从大陆尺度到细节**，理由见模块文档：前
    /// `coarse_levels`（`coarse_scale` 字段能提供的层数）层从格子
    /// 边长 `CELL_SIZE * coarse_scale` 开始，每层边长减半、权重减半，
    /// 直到回到 `CELL_SIZE` 本身；此后若 `octaves` 还有余量，才继续
    /// 沿用原先「频率翻倍」的路数往更细的方向叠加。两段权重都是「减半」
    /// ，在 `CELL_SIZE` 处自然衔接，不会在拼接点产生权重突变。
    pub fn octaves(&self, x: i32, y: i32, octaves: u32) -> i32 {
        if octaves == 0 {
            return self.sample(x, y);
        }

        // coarse_scale 恒为 2 的幂（见 new()），trailing_zeros 就是能从
        // 它往下减半到 1 的次数；层数在此基础上加一（把 1 本身也算作
        // 一层）。
        let coarse_levels = self.coarse_scale.trailing_zeros() + 1;

        let mut total: i64 = 0;
        let mut amplitude: i64 = SCALE_MAX as i64;
        let mut total_amplitude: i64 = 0;

        for i in 0..octaves {
            let value = if i < coarse_levels {
                let scale = self.coarse_scale >> i;
                let Some(cell_size) = CELL_SIZE.checked_mul(scale as i32) else {
                    break;
                };
                self.sample_at_scale(
                    x,
                    y,
                    cell_size,
                    self.period_x / scale,
                    self.period_y / scale,
                )
            } else {
                // 已经叠完大陆尺度到 CELL_SIZE 的全部层，继续往更细的
                // 方向走：频率从 2 开始（频率 1 就是上面 i = coarse_levels
                // - 1 时的 CELL_SIZE 层，不能重复叠加）。
                let extra = i - coarse_levels;
                let Some(frequency) = 1_i32.checked_shl(extra + 1) else {
                    break;
                };
                let Some(sx) = x.checked_mul(frequency) else {
                    break;
                };
                let Some(sy) = y.checked_mul(frequency) else {
                    break;
                };
                self.sample_at_scale(sx, sy, CELL_SIZE, self.period_x, self.period_y)
            };

            total += value as i64 * amplitude;
            total_amplitude += amplitude;
            amplitude /= 2;

            // 振幅衰减到零后继续叠加没有意义。
            if amplitude == 0 {
                break;
            }
        }

        (total / total_amplitude.max(1)).clamp(0, SCALE_MAX as i64) as i32
    }
}

/// 欧几里得算法求最大公约数。两个入参恒为正（[`TileableNoise::new`]
/// 已拒绝零周期），故不必处理 `gcd(0, 0)` 这类边界。
fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// 求 `n` 的最大二次幂因数，即 `n` 能被 `2^k` 整除的最大 `2^k`。
///
/// `n` 的二进制末尾连续零的个数就是它含有的 2 的因子个数
/// （`n.trailing_zeros()`）——`n` 为正时这个值恒有定义。
fn max_pow2_divisor(n: u32) -> u32 {
    1 << n.trailing_zeros()
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

    #[test]
    fn 周期存在公共二次幂因子时求得对应的粗层倍数() {
        // period_x = period_y = 8 = 2^3，两者的最大公约数本身就是 8，
        // 最粗一层应能把 CELL_SIZE 放大到 8 倍。
        // Arrange & Act
        let noise = TileableNoise::new(1, 8, 8).expect("周期非零");

        // Assert
        assert_eq!(noise.coarse_scale, 8);
    }

    #[test]
    fn 周期互质时没有可用的粗层退化为一() {
        // gcd(3, 5) = 1，没有比 CELL_SIZE 更粗、仍能在两个轴上无缝
        // 平铺的层可用，coarse_scale 退化为 1（即完全不叠加粗层）。
        // Arrange & Act
        let noise = TileableNoise::new(1, 3, 5).expect("周期非零");

        // Assert
        assert_eq!(noise.coarse_scale, 1);
    }

    #[test]
    fn 存在粗层时倍频结果与最细层单次采样不同() {
        // 锁住「确实叠加了更粗的层」这个行为，而不只是「值没越界」：
        // 若 octaves() 悄悄退化回只采样最细层，这条测试会先发现。
        // Arrange
        let noise = noise();

        // Act
        let with_coarse = noise.octaves(20, 20, 2);
        let finest_only = noise.sample(20, 20);

        // Assert
        assert_ne!(with_coarse, finest_only);
    }
}
