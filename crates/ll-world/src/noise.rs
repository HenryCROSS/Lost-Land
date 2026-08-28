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
//!
//! # 一个更隐蔽的退化：正方形世界，边长恰为 2 的幂
//!
//! 上面那段话本身留了一个坑：`max_pow2_divisor(gcd(period_x, period_y))`
//! 不只在互质时退化，**在 `period_x == period_y` 且两者本身就是 2 的
//! 幂时也会退化**——只是退化到的不是 1，而是退化到 `period_x`/
//! `period_y` 本身。例如 64×64 的世界：`period_x = period_y = 4`，
//! `gcd(4, 4) = 4`，`max_pow2_divisor(4) = 4`，与两个周期恰好相等。
//! 那一层的格点周期因此变成 `period / coarse_scale = 1`——**整张世界
//! 只有一个格点**，`lattice_value` 对周期取模后恒为同一个值，全图范围
//! 内是常数。`octaves` 里权重最高的这一层因此不携带任何空间变化，只剩
//! 细节层的小抖动，不足以跨过深水到山地的阈值区间。实测过：64×64、
//! 128×128、256×256 这类最常见的方形 2 的幂世界尺寸，多数种子下只产出
//! 1～3 种地形，且是**静默**的——不报错、不变红，只是整张图几乎全是
//! 水，直到有人去数地形种类才会发现。
//!
//! 这个坑不能靠「在生成入口拒绝这批尺寸」绕开：世界尺寸最终要开放给
//! 玩家在开局界面选择，方形、2 的幂（64、128、256……）恰恰是最直觉的
//! 选项，把它们全部拒绝等于把最常被选中的选项都变成错误提示。所以
//! [`TileableNoise::new`] 改为用 [`safe_coarse_scale`] 而非
//! `max_pow2_divisor(gcd(..))` 本身：候选值命中某一轴的周期（即那一轴
//! 会退化成单点）时减半，直到两轴都严格大于它，或减到 1（两周期互质，
//! 真的没有大于 1 的公共二次幂因子可用——这种情形下退化到 1 仍然是
//! 无法避免的降级，保留上一段的结论）。减半后的值依旧整除
//! `gcd(period_x, period_y)`，无缝性因此不受影响——见
//! [`safe_coarse_scale`] 文档。

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
            coarse_scale: safe_coarse_scale(period_x, period_y),
        })
    }

    /// 这个噪声源对应的世界**瓦片高度** `H`。
    ///
    /// # 为什么读取器住在这里，而不是把世界高度再传一遍
    ///
    /// 气候条带（[`crate::climate`]）是 `y` 的周期函数，周期取世界高度
    /// 的一半，因此地形生成的热路径 `terrain_at_coord` 需要知道 `H`。
    /// 而 `H` 早就在 [`Self::new`] 的入参里被确定了（`period_y` 就是
    /// `H / CELL_SIZE`）——再从调用方多传一个 `world_height` 参数进来，
    /// 等于在生成链路上放第二份「世界多高」的真相源，两份一旦对不上，
    /// 症状是气候带与地形错位、且完全静默。
    ///
    /// 用乘法反算而不是新存一个字段，是同一条理由的另一面：字段会成为
    /// 第二个可以被写错的地方，乘法不会。
    #[must_use]
    pub fn tile_height(&self) -> u32 {
        self.period_y * CELL_SIZE as u32
    }

    /// 把自动推导出的大陆尺度**再缩小 `steps` 档**（每档减半），返回
    /// 新的噪声源——[`Self::new`] 的链式修饰，不是第二个构造函数。
    ///
    /// # 为什么这个旋钮必须存在（实测结论，见 `knowledge/design/worldgen-parameters.md`）
    ///
    /// [`safe_coarse_scale`] 推导出的大陆尺度**与世界尺寸成正比**：世界
    /// 放大一倍，最粗一层的格子边长也跟着放大一倍。实测把同一个种子的
    /// 世界从 64×48 区块放大到 128×96 区块（面积四倍），产出的水陆比例、
    /// 陆块数量、最大陆块占比三项**逐位相同**——只是整张图被等比例放大
    /// 了。也就是说，光靠 [`crate::generate::GenParams`] 原有的三个旋钮
    /// （海平面 / 山地阈值 / 倍频层数），「世界很大但岛很多很小」这种
    /// 形态根本无法表达：调高海平面只会同时**淹掉更多陆地**，得到的是
    /// 「一块被淹得只剩边角的大陆」，不是群岛。
    ///
    /// 本方法就是那个缺失的旋钮：它只改**大陆尺度层的格子边长**，不改
    /// 任何阈值——`steps = 2` 时大陆特征尺度缩到四分之一，同一个海平面
    /// 下水陆比例几乎不变，但陆地被切成数倍于原先的独立陆块。
    ///
    /// # 为什么减半不破坏无缝平铺
    ///
    /// 与 [`safe_coarse_scale`] 文档「为什么减半后仍然安全」一节完全
    /// 同一条论证：`coarse_scale` 恒是 `period_x`/`period_y` 的 2 的幂
    /// 公因数，一个 2 的幂公因数减半之后仍是 2 的幂、仍整除两个周期，
    /// [`Self::sample_at_scale`] 需要的「格点周期能整除」这条前提不受
    /// 影响。缩到 1 之后再缩没有意义（格子边长已经等于 [`CELL_SIZE`]，
    /// 大陆尺度层与细节层重合），故在 1 处饱和而不是继续右移到零。
    #[must_use]
    pub fn shrink_continents(self, steps: u32) -> Self {
        let scale = self.coarse_scale.checked_shr(steps).unwrap_or(0).max(1);
        TileableNoise {
            coarse_scale: scale,
            ..self
        }
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

/// 求 [`TileableNoise::octaves`] 最粗一层可用的格子放大倍数——
/// [`gcd`] 的最大二次幂因数，但**不允许取到会让该层退化成整图常数的
/// 那个值**。退化条件与修法见模块文档「一个更隐蔽的退化」一节。
///
/// # 为什么减半后仍然安全
///
/// `max_pow2_divisor(gcd(period_x, period_y))` 恒是 `period_x` 与
/// `period_y` 的公因数（`gcd` 本身是公因数，其二次幂部分自然也是）。
/// 一个 2 的幂公因数减半之后仍是 2 的幂、仍整除原来的 `gcd`——`sample_
/// at_scale` 需要的只是「格子边长能整除周期换算出的格点数」，不要求
/// 恰好取到最大值，减半不会破坏 [`TileableNoise::sample_at_scale`]
/// 用到的无缝性前提。
///
/// # 为什么循环最多迭代一次就会停下（写成循环仅为清晰，非性能考量）
///
/// 设 `scale` 是候选值。它恒有 `scale <= period_x` 且 `scale <=
/// period_y`（因为它整除二者）。若 `scale == period_x`，减半后的新值
/// `scale / 2 < scale <= period_y`，不可能再等于 `period_y`；若同时
/// `scale == period_y`，同理新值也不再等于 `period_x`。所以一次减半
/// 必然让两个相等条件同时不再成立——循环写成 `while` 只是为了在阅读
/// 时不必依赖这条数学论证也能确认终止，不是因为真的需要多次减半。
fn safe_coarse_scale(period_x: u32, period_y: u32) -> u32 {
    let mut scale = max_pow2_divisor(gcd(period_x, period_y));
    while scale > 1 && (scale == period_x || scale == period_y) {
        scale /= 2;
    }
    scale
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
    fn 周期存在公共二次幂因子且两轴都不等于该因子时无需减半() {
        // period_x = 24 = 8*3、period_y = 40 = 8*5，gcd = 8。8 既不
        // 等于 period_x 也不等于 period_y，两个轴换算到粗层的格点周期
        // 分别是 3、5，都大于 1——不会退化成单点，safe_coarse_scale
        // 不需要触发减半分支，直接得到 gcd 的最大二次幂因数本身。
        // Arrange & Act
        let noise = TileableNoise::new(1, 24, 40).expect("周期非零");

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
    fn 正方形世界边长恰为二的幂时粗层不再退化成整图常数() {
        // 这是本次要修的缺陷本身：period_x = period_y = 4（对应 64×64
        // 世界）曾经让 max_pow2_divisor(gcd(4,4)) 恰好等于 4，粗层格点
        // 周期退化为 1（整图单点、常数）。safe_coarse_scale 修复后应
        // 减半到 2，让粗层格点周期变成 4/2=2——两个轴都还有真实起伏。
        // Arrange & Act
        let noise = TileableNoise::new(1, 4, 4).expect("周期非零");

        // Assert
        assert_eq!(noise.coarse_scale, 2);
    }

    #[test]
    fn 较短边整除较长边且较短边恰为二的幂时粗层同样不退化() {
        // 非正方形也可能撞上同一个坑：period_x=4（本身是 2 的幂）、
        // period_y=12=4*3。gcd=4=period_x，候选值命中 x 轴，若不修复
        // x 轴会退化成单点（即便 y 轴仍有 3 格起伏）。修复后应减半到 2。
        // Arrange & Act
        let noise = TileableNoise::new(1, 4, 12).expect("周期非零");

        // Assert
        assert_eq!(noise.coarse_scale, 2);
    }

    #[test]
    fn 开局可选的方形二的幂区块数世界修复后不再退化() {
        // 开局界面若允许玩家把区块边长配置成 128（ZoneLayout::new 仍然
        // 接受这个取值，只是不再是 crate::zone::ZoneLayout::default_config
        // 给出的默认值，见其文档「为什么区块边长从 128 改成 48」）并选
        // 「N×N 个区块」，最直觉的方形选项恰恰是 2 的幂：64、128 个
        // 区块。换算成世界瓦片周期（除以 CELL_SIZE=16）：64 区块 →
        // 8192 瓦片 → period 512；128 区块 → 16384 瓦片 → period
        // 1024。两者都恰为 2 的幂，修复前会让 coarse_scale 退化成
        // period 本身；这里直接算这两个真实候选尺寸的周期，而不是用
        // 等价小尺寸代替——这条只断言 coarse_scale（O(1) 的纯数值推导，
        // 不涉及生成整张地图），跑大周期不产生实际性能负担。
        // Arrange & Act
        let noise_64_zones = TileableNoise::new(1, 512, 512).expect("周期非零");
        let noise_128_zones = TileableNoise::new(1, 1024, 1024).expect("周期非零");

        // Assert：粗层格点周期 = period / coarse_scale 必须大于 1，
        // 即 coarse_scale 不能恰好等于 period 本身。
        assert_ne!(
            noise_64_zones.coarse_scale, 512,
            "64 区块（8192×8192）的粗层退化成整图单点"
        );
        assert_ne!(
            noise_128_zones.coarse_scale, 1024,
            "128 区块（16384×16384）的粗层退化成整图单点"
        );
    }

    #[test]
    fn 生产默认区块布局对应的世界周期不触发大陆尺度层退化() {
        // 生产默认配置见 crate::zone::ZoneLayout::default_config：
        // 区块 48×48、世界 96×64 个区块，换算成瓦片周期
        // （48*96/16, 48*64/16）= (288, 192)。这条锁住「当前生产配置
        // 本就不在退化区间」这个结论——即便 safe_coarse_scale 没修，
        // gcd(288, 192) = 96，其最大二次幂因子是 32，不等于 288 也不
        // 等于 192，故 coarse_scale 本就不会退化；这里把这条结论写成
        // 断言，防止未来改动 default_config 时无声破坏它。
        //
        // 这不是巧合：48 / CELL_SIZE = 3 是奇数，不带任何 2 的因子,
        // `default_config` 文档「为什么区块边长从 128 改成 48」一节已经
        // 证明——只要区块边长的这个倍数是奇数，任何 zone_count 组合都
        // 不会触发这条退化,不需要逐一枚举验证,这里的具体数值断言只是
        // 把这条证明落成一条可执行的回归。
        // Arrange
        let period_x = 48 * 96 / CELL_SIZE as u32;
        let period_y = 48 * 64 / CELL_SIZE as u32;

        // Act
        let noise = TileableNoise::new(1, period_x, period_y).expect("周期非零");

        // Assert
        assert_ne!(noise.coarse_scale, period_x, "x 轴的粗层退化成整图单点");
        assert_ne!(noise.coarse_scale, period_y, "y 轴的粗层退化成整图单点");
    }

    #[test]
    fn 长方形预设区块数世界本就不触发大陆尺度层退化() {
        // 若开局界面推荐长方形预设（64×48、96×64、128×96、192×128 个
        // 区块，区块边长固定 48，见
        // ll_content::world_identity::RECOMMENDED_PRESETS），换算成瓦片
        // 周期（区块数 * 48 / 16 = 区块数 * 3）逐一验证：这些组合天然
        // 不会让 coarse_scale 撞上某一轴的周期本身——不依赖
        // safe_coarse_scale 的减半分支也能通过，用来确认「长方形预设
        // 本就安全」这个结论，而不是碰巧被这次的修复带着变安全。
        // Arrange
        let zone_count_presets = [(64u32, 48u32), (96, 64), (128, 96), (192, 128)];

        // Act & Assert
        for (zones_w, zones_h) in zone_count_presets {
            let period_x = zones_w * 48 / CELL_SIZE as u32;
            let period_y = zones_h * 48 / CELL_SIZE as u32;
            let noise = TileableNoise::new(1, period_x, period_y).expect("周期非零");
            assert_ne!(
                noise.coarse_scale, period_x,
                "{zones_w}x{zones_h} 区块预设的 x 轴粗层退化成整图单点"
            );
            assert_ne!(
                noise.coarse_scale, period_y,
                "{zones_w}x{zones_h} 区块预设的 y 轴粗层退化成整图单点"
            );
        }
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
