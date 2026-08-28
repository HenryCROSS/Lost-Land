//! 气候条带：把环面上的 `y` 坐标换算成「纬度暖度」与「气候带」。
//!
//! # 规格出处（不是发挥）
//!
//! 规格 §7.1 与决策表第 23 条钉死了形状：
//!
//! > **气候为周期性条带**：两条赤道 + 两条极圈。玩家持续向北将穿越极地
//! > 后重新进入热带。
//!
//! 关键在「**两条**赤道 + **两条**极圈」——环面上没有极点，因此**不能**
//! 套用「一条赤道 + 两个极点」那种球面模型。一个完整的世界高度里必须
//! 恰好出现两次最暖、两次最冷。
//!
//! # 为什么是三角波，而且周期取世界高度的一半
//!
//! 一个周期的三角波内恰有一个极大与一个极小。要在整张世界（高度 `H`）
//! 里凑出两个极大与两个极小，周期就必须取 `H / 2`：
//!
//! ```text
//! y = 0      赤道 (warmth = 1000)
//! y = H/4    极圈 (warmth = 0)
//! y = H/2    赤道
//! y = 3H/4   极圈
//! y = H      赤道 ≡ y = 0      ← 接缝天然闭合
//! ```
//!
//! # 为什么全程整数，一个 `sin` 都不许有
//!
//! 「纬度的余弦」是这个函数最自然的写法，也正是本项目**禁止**的写法。
//! [ADR 0002](../../../knowledge/decisions/0002-integer-only-world-state.md)
//! 与 `docs/architecture/05-integer-discipline.md` 的判据是「结果会不会
//! 变成世界状态的一部分」——气候决定地形，地形是世界状态，所以气候的
//! 每一步都必须是整数。更具体地：IEEE 754 **不规定** `sin`/`cos` 的精度，
//! 不同 libm 实现结果不同，一旦用上，规格 §14.4 要求的「同一种子在
//! ubuntu 与 windows 两个 target 上产出逐位相同的世界」当场失效。
//!
//! 这与 §7.1 当年把地形噪声从「4D 投影（需要 `sin`/`cos`）」改成「模
//! 格点」是同一条理由。不要在气候这里把它重新引进来。
//!
//! # 接缝连续性是**构造上**的，不是靠修补
//!
//! 世界高度恒是 [`crate::noise::CELL_SIZE`]（16）的整数倍（否则
//! `ll_world::generate::build_noise` 直接拒绝生成），因此 `H / 2` 是 8 的
//! 整数倍，`H.rem_euclid(H / 2) == 0 == 0.rem_euclid(H / 2)`，于是
//! `warmth(0) == warmth(H)` 对**任何**合法世界高度恒成立。`tests/climate_blackbox.rs`
//! 用属性测试把这条钉死，与噪声层那条「可平铺整数噪声接缝处连续」
//! （规格 §14.2 属性测试表）同型。

/// 纬度暖度的取值上界（含）：赤道。
///
/// 用千分比而不是 `0..=100` 或浮点，与 [`crate::noise::TileableNoise`] 的
/// 输出区间、`ll_core::scaled::Milli`、
/// [`crate::generate::TerrainShape`] 的各个阈值三处既有惯例保持同一套
/// 标度——同一个世界里只有一种「比例怎么表示」的答案。
pub const WARMTH_MAX: i32 = 1000;

/// 纬度暖度的取值下界（含）：极圈。
pub const WARMTH_MIN: i32 = 0;

/// 气候带：把连续的纬度暖度切成三档，供地形分带使用。
///
/// 只有三档而不是「热带/亚热带/温带/寒带/极地」那种五档以上的分法：
/// 本批次只让气候调制**一段**高度带（见
/// [`crate::generate`] 的 `height_to_terrain`），三档已经能表达
/// 「沙漠—草原—冻原」这条完整的纬度梯度；再细分需要更多地形种类，
/// 属于后续内容批次。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClimateBand {
    /// 干热带：贴着两条赤道的那两条带。低海拔在这里是沙漠。
    Hot,
    /// 温带：赤道与极圈之间。地形分带与气候条带落地之前**逐位相同**。
    Temperate,
    /// 极地带：贴着两条极圈的那两条带。低海拔在这里是冻原。
    Polar,
}

/// 求某个（未经环面环绕的）`y` 坐标处的**纬度暖度**，千分比，
/// [`WARMTH_MIN`]..=[`WARMTH_MAX`]，1000 为赤道、0 为极圈。
///
/// `world_height` 是世界的瓦片高度 `H`（恒是
/// [`crate::noise::CELL_SIZE`] 的整数倍）。周期取 `H / 2`，理由见模块
/// 文档——这是「两条赤道 + 两条极圈」这条规格要求的直接落点。
///
/// 刻意接受**未环绕**的原始 `y`：接缝测试要比较 `y = 0` 与 `y = H` 这
/// 两个环绕后会被判成同一点的坐标，参数若是已环绕的
/// `ll_core::torus::TorusPos` 就根本构造不出这两个不同的输入——与
/// [`crate::generate`] 的 `terrain_at_coord` 同一条取舍。
///
/// # 恒等与边界
///
/// - `world_height == 0` 时返回 [`WARMTH_MAX`]（整图赤道）。这是防御性
///   分支：`TorusSize` 保证高度非零，正常路径到不了这里，但本函数是
///   `pub` 的，不能用 `%` 除零 panic 回应一个不该发生的输入。
/// - 负数 `y` 用 `rem_euclid` 而不是 `%`：`-1 % 8 == -1` 在 Rust 里是
///   合法结果，会让暖度算出负值；环面语义要的是「绕回去」。
pub fn warmth_at(y: i32, world_height: u32) -> i32 {
    let period = (world_height / 2) as i32;
    if period <= 0 {
        return WARMTH_MAX;
    }
    let t = y.rem_euclid(period);
    // 三角波：前半个周期从赤道线性降到极圈，后半个周期升回赤道。
    // 乘 2 再除周期即「走完半个周期正好跨满整个值域」，全整数、
    // 截断方向固定，跨平台逐位相同。
    if 2 * t <= period {
        WARMTH_MAX - 2 * WARMTH_MAX * t / period
    } else {
        2 * WARMTH_MAX * t / period - WARMTH_MAX
    }
}

/// 把纬度暖度切成 [`ClimateBand`]，`band_width` 是**每一侧**条带的宽度
/// （千分比，取自 [`crate::generate::TerrainShape::climate_band_width`]）。
///
/// # 为什么用严格不等号：`band_width == 0` 必须是**真正的**恒等
///
/// ```text
/// 干热带 ⇔ warmth >  WARMTH_MAX - band_width
/// 极地带 ⇔ warmth <                band_width
/// ```
///
/// `band_width == 0` 时两条判据分别退化成 `warmth > 1000` 与
/// `warmth < 0`，**恒假**，整图温带。若写成 `>=` / `<=`，赤道那一行
/// （`warmth == 1000`）与极圈那一行仍会各自落进两端，「关掉气候」就不
/// 再是逐位恒等——而黄金基准重冻的第 ② 步（把改动关掉、确认精确回到
/// 旧值）正是靠这条恒等性成立的，见
/// `docs/superpowers/plans/2026-08-27-batch3-climate-bands.md` D4。
pub fn band_from_warmth(warmth: i32, band_width: i32) -> ClimateBand {
    if warmth > WARMTH_MAX - band_width {
        ClimateBand::Hot
    } else if warmth < band_width {
        ClimateBand::Polar
    } else {
        ClimateBand::Temperate
    }
}

/// [`warmth_at`] 与 [`band_from_warmth`] 的组合：直接问「这个 `y` 落在
/// 哪条气候带」。
///
/// 地形生成走的是这一个入口，两个半步只在测试与探针里单独用到——
/// 生产路径上只有一条链，不存在两处各拼一遍的漂移风险。
pub fn band_at(y: i32, world_height: u32, band_width: i32) -> ClimateBand {
    band_from_warmth(warmth_at(y, world_height), band_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试世界高度：320 是 `CELL_SIZE`（16）的整数倍，且与
    /// `p2_acceptance` 演示世界的高度一致，四个特征纬度都是整数格。
    const H: u32 = 320;

    #[test]
    fn 一个世界高度里恰有两条赤道与两条极圈() {
        // 这是规格 §7.1「两条赤道 + 两条极圈」这句话的可执行版本。
        // 反例（本次开发实跑）：把 warmth_at 的周期从 H/2 改成 H，
        // 本条报「赤道条数 1 != 2」。
        // Arrange
        let mut equators = 0;
        let mut poles = 0;

        // Act
        for y in 0..H as i32 {
            let warmth = warmth_at(y, H);
            if warmth == WARMTH_MAX {
                equators += 1;
            }
            if warmth == WARMTH_MIN {
                poles += 1;
            }
        }

        // Assert
        assert_eq!(equators, 2, "一个世界高度里应当恰有两条赤道");
        assert_eq!(poles, 2, "一个世界高度里应当恰有两条极圈");
    }

    #[test]
    fn 赤道与极圈落在四分之一处() {
        // Arrange & Act & Assert
        let h = H as i32;
        assert_eq!(warmth_at(0, H), WARMTH_MAX, "y=0 是赤道");
        assert_eq!(warmth_at(h / 4, H), WARMTH_MIN, "y=H/4 是极圈");
        assert_eq!(warmth_at(h / 2, H), WARMTH_MAX, "y=H/2 是第二条赤道");
        assert_eq!(warmth_at(3 * h / 4, H), WARMTH_MIN, "y=3H/4 是第二条极圈");
    }

    #[test]
    fn 持续向北穿过极地之后重新进入热带() {
        // 规格 §7.1 那句「玩家持续向北将穿越极地后重新进入热带」的
        // 可执行版本：从赤道一路向北，暖度必须先降到极圈、再升回赤道。
        // Arrange
        let h = H as i32;
        let width = 250;

        // Act
        let journey: Vec<ClimateBand> = (0..=h / 2).map(|y| band_at(y, H, width)).collect();

        // Assert
        assert_eq!(journey.first(), Some(&ClimateBand::Hot), "出发时在热带");
        assert!(journey.contains(&ClimateBand::Polar), "半程必须穿过极地带");
        assert_eq!(
            journey.last(),
            Some(&ClimateBand::Hot),
            "走完半个世界高度重新进入热带"
        );
    }

    #[test]
    fn 带宽为零时整图都是温带() {
        // 这是「关掉气候即恒等」这条性质的最小单元版本；世界摘要那一层
        // 的版本在 crates/ll-world/tests/determinism.rs。
        // 反例（本次开发实跑）：把 band_from_warmth 的 `>` 改成 `>=`，
        // 本条在 y=0（赤道）报 Hot != Temperate。
        // Arrange & Act & Assert
        for y in 0..H as i32 {
            assert_eq!(
                band_at(y, H, 0),
                ClimateBand::Temperate,
                "带宽为零时 y={y} 应当是温带"
            );
        }
    }

    #[test]
    fn 暖度恒落在千分比区间内() {
        // Arrange & Act & Assert
        for y in -(H as i32)..2 * H as i32 {
            let warmth = warmth_at(y, H);
            assert!(
                (WARMTH_MIN..=WARMTH_MAX).contains(&warmth),
                "y={y} 的暖度 {warmth} 越出千分比区间"
            );
        }
    }

    #[test]
    fn 世界高度为零时不panic() {
        // TorusSize 保证高度非零，正常路径到不了；本函数是 pub 的，
        // 除零 panic 不是可接受的回应方式。
        // Arrange & Act & Assert
        assert_eq!(warmth_at(7, 0), WARMTH_MAX);
    }
}
