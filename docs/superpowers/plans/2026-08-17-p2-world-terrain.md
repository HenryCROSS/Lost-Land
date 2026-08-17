# P2 世界与地形层 实施计划

> **给执行者：** 必须配合 `superpowers:subagent-driven-development` 逐任务实施。步骤使用 `- [ ]` 复选框追踪。

**目标：** 建立 `ll-world` crate：可完整序列化的 `WorldState`、环面无缝地形生成、FOV 视野、世界时钟驱动的昼夜四季与光照、大陆地图与小地图数据视图。

**架构：** 世界状态是一个**纯数据、无指针、全整数**的结构体，可整体序列化。地形按块（chunk）存储以支撑大世界。**地形生成使用自研的整数可平铺值噪声**——现成噪声库产出浮点，会摧毁跨平台确定性。

**技术栈：** 仅 `ll-core` + `serde` + `tracing`。**不引入任何噪声库**（理由见 Task 2）。

**规格：** [`docs/superpowers/specs/2026-08-16-lostland-design.md`](../specs/2026-08-16-lostland-design.md)
**上阶段交接：** [`knowledge/handoff/p1-to-p2.md`](../../../knowledge/handoff/p1-to-p2.md)

## 全局约束

- **世界状态禁止浮点。** 地形、光照、视野一律整数。需要小数的量用 `ll_core::scaled::Milli`（千分之一）。**判断标准：这个值会不会被 `serde` 写进存档？会，就不能是浮点。**
- **禁止引入产出浮点的噪声库。** `noise` crate 许可证合规但产出 `f64`，会让不同平台生成出不同的世界，摧毁模式3 自由读档与确定性重放。
- 所有随机性来自 `DetRng::for_entity(世界种子, 实体ID, 事件计数)`，**禁止全局 RNG**。
- 环面距离与位移只能走 `ll_core::torus::TorusSize` 的方法，**禁止手写差值或欧氏距离**。
- **世界任一维度必须大于 43×25 格**（渲染层 `Camera::visible_tiles` 的跨度），否则地形填不满留黑块。
- 禁止硬编码用户可见字符串。
- 所有公开项必须有文档注释；注释解释**为什么**而非复述代码。
- 测试遵循 **AAA 结构**，测试名描述**行为**，一个测试只断言一件事。**测试名里不要出现混合大小写的 ASCII 子串**（会触发 `non_snake_case`，在 `-D warnings` 下挡门禁）。
- 文件 200–400 行为宜，800 行上限。
- 提交信息 `<type>: <描述>`，正文说明**为什么**，**不得含任何 AI 署名或生成工具标记**。中文。

## 可依赖的既有 API（已核实为当前代码，非记忆）

- `ll_core::torus`：`TorusSize::{new, width, height, wrap, delta, chebyshev, manhattan, squared_euclidean}`、`TorusPos::{x, y}`、`TorusSize::MAX_EXTENT`
- `ll_core::rng`：`DetRng::{for_entity, next_u64, gen_range, chance}`
- `ll_core::time`：`Tick`、`Season`、`TICKS_PER_MINUTE/HOUR/DAY`、`DAYS_PER_SEASON`、`SEASONS_PER_YEAR`、`Tick::{hour_of_day, day_of_year, season, is_daylight}`
- `ll_core::hashing`：`StateHasher::{new, write_u64, write_i64, finish}`
- `ll_core::scaled`：`Milli::{ZERO, from_whole, checked_from_whole, whole, checked_mul_ratio, mul_ratio}`、`SCALE`
- `ll_core::ident`：`NamespacedId::{parse, namespace, path}`、`ContentIndex::get`、`Interner::{new, intern, resolve, len, is_empty}`

## 文件结构

```
crates/ll-world/
  Cargo.toml
  src/lib.rs                    模块声明 + WorldError
  src/terrain.rs                TerrainKind 与其属性
  src/chunk.rs                  分块瓦片存储
  src/noise.rs                  整数可平铺值噪声
  src/generate.rs               地形生成
  src/fov.rs                    对称阴影投射视野
  src/light.rs                  昼夜四季 → 光照等级
  src/state.rs                  WorldState
  src/overview.rs               大陆地图与小地图数据视图
  tests/noise_blackbox.rs       无缝性与区间属性测试
  tests/fov_blackbox.rs         视野对称性属性测试
  tests/determinism.rs          世界生成的黄金基准
  examples/p2_acceptance.rs     接 ll-render 的验收 demo
```

---

### Task 1：crate 骨架、地形类型与分块存储

**Files:** 创建 `crates/ll-world/Cargo.toml`、`src/lib.rs`、`src/terrain.rs`、`src/chunk.rs`；其余模块建一行文档注释占位

**Interfaces Produces:**
- `pub enum WorldError { WorldTooSmall { width: u32, height: u32 }, WorldNotTileable { width: u32, height: u32 }, ChunkOutOfRange }`
- `pub struct TerrainKind(pub u16)`，常量 `DEEP_WATER`/`SHALLOW_WATER`/`SAND`/`GRASS`/`FOREST`/`HILL`/`MOUNTAIN`/`SNOW`
- `TerrainKind::{blocks_sight, blocks_move, move_cost}`（`move_cost -> u32`，不可通行时 `u32::MAX`）
- `pub const CHUNK_SIZE: u32 = 32`
- `pub struct ChunkGrid`，方法 `new(TorusSize) -> Result<ChunkGrid, WorldError>`、`world() -> TorusSize`、`terrain_at(TorusPos) -> TerrainKind`、`set_terrain(TorusPos, TerrainKind)`、`chunk_count() -> usize`

> **为什么分块**：世界可达数百万格，整块 `Vec` 一次性分配会吃掉大量内存且无法按需生成。32×32 是权衡点——再小则块管理开销占比过高，再大则单块内存浪费明显。

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-world/src/chunk.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::torus::TorusSize;

    fn grid() -> ChunkGrid {
        let world = TorusSize::new(64, 64).expect("常量非零");
        ChunkGrid::new(world).expect("64x64 大于视口跨度")
    }

    #[test]
    fn 世界小于视口跨度时构造失败() {
        // 世界任一维度小于 43×25 格时，渲染层相机会产出重复坐标，
        // 地形填不满留黑块。与其让缺陷在运行时表现为视觉异常，
        // 不如在构造点直接拒绝。
        // Arrange
        let tiny = TorusSize::new(20, 20).expect("常量非零");

        // Act
        let result = ChunkGrid::new(tiny);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 写入后可读回同一地形() {
        // Arrange
        let mut grid = grid();
        let pos = grid.world().wrap(10, 20);

        // Act
        grid.set_terrain(pos, TerrainKind::MOUNTAIN);

        // Assert
        assert_eq!(grid.terrain_at(pos), TerrainKind::MOUNTAIN);
    }

    #[test]
    fn 跨块边界的写入互不干扰() {
        // 块边界是分块存储最容易出错的地方：算错块索引会让写入落到邻块。
        // Arrange
        let mut grid = grid();
        let inside = grid.world().wrap(31, 31);
        let across = grid.world().wrap(32, 32);

        // Act
        grid.set_terrain(inside, TerrainKind::SAND);
        grid.set_terrain(across, TerrainKind::SNOW);

        // Assert
        assert_eq!(grid.terrain_at(inside), TerrainKind::SAND);
    }

    #[test]
    fn 环面绕回的坐标指向同一格() {
        // Arrange
        let mut grid = grid();
        let origin = grid.world().wrap(0, 0);
        let wrapped = grid.world().wrap(64, 64);

        // Act
        grid.set_terrain(origin, TerrainKind::FOREST);

        // Assert
        assert_eq!(grid.terrain_at(wrapped), TerrainKind::FOREST);
    }

    #[test]
    fn 山地阻挡视线() {
        // Arrange & Act & Assert
        assert!(TerrainKind::MOUNTAIN.blocks_sight());
    }

    #[test]
    fn 草地不阻挡视线() {
        // Arrange & Act & Assert
        assert!(!TerrainKind::GRASS.blocks_sight());
    }

    #[test]
    fn 不可通行地形的移动代价为最大值() {
        // 用 u32::MAX 而非 Option，让寻路算法不必对每格做分支判断。
        // Arrange & Act & Assert
        assert_eq!(TerrainKind::DEEP_WATER.move_cost(), u32::MAX);
    }
}
```

- [ ] **Step 2：运行确认失败**

```bash
cargo test -p ll-world chunk
```
预期：`cannot find type ChunkGrid in this scope`。

- [ ] **Step 3：创建 `Cargo.toml`**

```toml
[package]
name = "ll-world"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "迷途大陆的世界层：世界状态、环面地形、视野、昼夜四季"

[dependencies]
ll-core = { path = "../ll-core" }
serde = { version = "1", features = ["derive"] }
tracing = "0.1"

[dev-dependencies]
proptest.workspace = true
criterion.workspace = true
```

- [ ] **Step 4：实现 `terrain.rs`**

```rust
/// 地形种类。
///
/// 用 `u16` 而非枚举：mod 需要能注册新地形，而枚举无法在运行时扩展。
/// 本体的八种作为常量提供，注册表负责把命名空间 ID 映射到数值。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TerrainKind(pub u16);
```

八个常量按 0..8 编号。`move_cost` 对不可通行地形返回 `u32::MAX`。

- [ ] **Step 5：实现 `chunk.rs`**

```rust
/// 视口能容纳的最小世界宽度（格）。
///
/// 取自渲染层 `Camera::visible_tiles` 的实际跨度：横向
/// `LOGICAL_WIDTH / TILE_SIZE / 2 + 1 = 21` 向两侧展开共 43 格。
/// 世界小于这个跨度时会产出重复坐标，地形填不满留黑块。
///
/// 此处写死数值而非依赖 `ll-render`：世界层不应反向依赖渲染层。
/// 数值一致性由 `tests/determinism.rs` 中的断言守护。
const MIN_WORLD_WIDTH: u32 = 43;

/// 视口能容纳的最小世界高度（格）。理由同 [`MIN_WORLD_WIDTH`]。
const MIN_WORLD_HEIGHT: u32 = 25;
```

- [ ] **Step 6：验证并提交**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

```bash
git add crates/ll-world Cargo.toml Cargo.lock
git commit -F - <<'EOF'
feat: 世界层骨架、地形类型与分块存储

地形按 32×32 分块而非整块 Vec：世界可达数百万格，一次性分配会吃掉大量
内存且无法按需生成。32 是权衡点——再小则块管理开销占比过高，再大则单块
内存浪费明显。

TerrainKind 用 u16 而非枚举，因为 mod 需要能注册新地形，而枚举无法在
运行时扩展。本体八种作为常量，注册表负责映射命名空间 ID。

ChunkGrid::new 在构造点就拒绝小于 43×25 格的世界。这个跨度取自渲染层
相机的可见范围，世界小于它时会产出重复坐标、地形填不满留黑块——与其
让缺陷在运行时以视觉异常的形式出现，不如在构造点直接拒绝。

跨度数值在世界层写死而非依赖 ll-render，是为了不让世界层反向依赖渲染层；
一致性由 determinism 测试里的断言守护。

不可通行地形的移动代价用 u32::MAX 而非 Option，让寻路算法不必对每格
做分支判断。
EOF
```

---

### Task 2：整数可平铺值噪声

**Files:** `crates/ll-world/src/noise.rs`、`crates/ll-world/tests/noise_blackbox.rs`

**Interfaces Produces:**
- `pub struct TileableNoise`
- `TileableNoise::new(seed: u64, period_x: u32, period_y: u32) -> Option<TileableNoise>`
- `TileableNoise::sample(&self, x: i32, y: i32) -> i32`（返回 `0..=1000`）
- `TileableNoise::octaves(&self, x: i32, y: i32, octaves: u32) -> i32`（返回 `0..=1000`）
- `pub const CELL_SIZE: i32 = 16`

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-world/src/noise.rs 末尾
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
}
```

- [ ] **Step 2：运行确认失败**

```bash
cargo test -p ll-world noise
```

- [ ] **Step 3：实现 `noise.rs`**

```rust
//! 整数可平铺值噪声。
//!
//! # 为什么自研而不用现成噪声库
//!
//! `noise` crate 的许可证（Apache-2.0/MIT）是合规的，但它产出 `f64`。
//! 同一段浮点运算在不同平台可能产出不同的最低位，地形因此不同、世界
//! 哈希因此不同——P0 用整个阶段建立的跨平台确定性会被地形生成一举
//! 摧毁，而模式3 自由读档正建立在它之上。
//!
//! # 为什么用模格点而不是 4D 投影
//!
//! 让噪声在环面上无缝的常见做法，是把 2D 环面嵌入 4D 空间再采样 4D
//! 噪声。那是**为浮点噪声设计的绕法**，需要三角函数。
//!
//! 整数方案不需要绕：格点索引对周期取模即可。`lattice_x mod period_x`
//! 使 `x = 0` 与 `x = period_x * CELL_SIZE` 命中同一格点，**无缝是构造
//! 上保证的**，不是近似出来的。
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
```

- [ ] **Step 4：运行确认通过**

```bash
cargo test -p ll-world noise
```

- [ ] **Step 5：写无缝性属性测试**

```rust
// crates/ll-world/tests/noise_blackbox.rs
//! 可平铺噪声的黑箱属性测试。
//!
//! 无缝性是这个模块存在的全部理由，而它只能靠属性测试来验——手写用例
//! 不可能覆盖所有接缝位置与周期组合。

use ll_world::noise::{CELL_SIZE, TileableNoise};
use proptest::prelude::*;

proptest! {
    #[test]
    fn 东西接缝处采样值连续(period in 2u32..16, y in -500i32..500) {
        // 接缝不连续时，玩家跨越世界东西边界会看到地形突变。
        // Arrange
        let noise = TileableNoise::new(0xABCD, period, period).expect("周期非零");
        let world_width = period as i32 * CELL_SIZE;

        // Act
        let west = noise.sample(0, y);
        let east = noise.sample(world_width, y);

        // Assert
        prop_assert_eq!(west, east);
    }

    #[test]
    fn 南北接缝处采样值连续(period in 2u32..16, x in -500i32..500) {
        // Arrange
        let noise = TileableNoise::new(0xABCD, period, period).expect("周期非零");
        let world_height = period as i32 * CELL_SIZE;

        // Act
        let north = noise.sample(x, 0);
        let south = noise.sample(x, world_height);

        // Assert
        prop_assert_eq!(north, south);
    }

    #[test]
    fn 任意坐标的采样值都在有效区间内(
        seed in any::<u64>(),
        period in 1u32..32,
        x in i32::MIN / 4..i32::MAX / 4,
        y in i32::MIN / 4..i32::MAX / 4,
    ) {
        // 极端坐标最容易触发溢出，而溢出后的值会静默越界。
        // Arrange
        let noise = TileableNoise::new(seed, period, period).expect("周期非零");

        // Act
        let value = noise.sample(x, y);

        // Assert
        prop_assert!((0..=1000).contains(&value));
    }

    #[test]
    fn 多倍频在任意层数下都不溢出(
        octaves in 0u32..24,
        x in -100_000i32..100_000,
        y in -100_000i32..100_000,
    ) {
        // 层数过多时频率翻倍会让坐标溢出。
        // Arrange
        let noise = TileableNoise::new(7, 8, 8).expect("周期非零");

        // Act
        let value = noise.octaves(x, y, octaves);

        // Assert
        prop_assert!((0..=1000).contains(&value));
    }
}
```

- [ ] **Step 6：验证并提交**

```bash
cargo test -p ll-world && cargo clippy --workspace --all-targets -- -D warnings
```

```bash
git add crates/ll-world
git commit -F - <<'EOF'
feat: 整数可平铺值噪声

不用 noise crate：它许可证合规但产出 f64。同一段浮点运算在不同平台可能
产出不同的最低位，地形因此不同、世界哈希因此不同——P0 用整个阶段建立的
跨平台确定性会被地形生成一举摧毁，而模式3 自由读档正建立在它之上。

无缝性用模格点实现，而不是常见的「2D 环面嵌入 4D 空间再采样 4D 噪声」。
后者是为浮点噪声设计的绕法、需要三角函数；整数方案把格点索引对周期取模
即可，无缝是构造上保证的而非近似出来的。代价是世界宽高必须是周期与格子
尺寸之积。

插值用五次多项式而非线性：线性插值会在格点处留下可见的方格棱线。中间
计算用 i64 承接以免立方溢出。

格点索引打包成 u64 时用移位而非相加，否则 (3,5) 与 (5,3) 会撞进同一个值，
地形上会出现对角线状伪影。

多倍频的频率翻倍用 checked 乘法：层数多时坐标会溢出，而溢出后的采样点
是错的却不会报错。
EOF
```

---

### Task 3：环面地形生成

**Files:** `crates/ll-world/src/generate.rs`

**Interfaces Produces:**
- `pub struct GenParams { pub seed: u64, pub sea_level: i32, pub mountain_level: i32, pub octaves: u32 }`，实现 `Default`（`sea_level: 400`、`mountain_level: 750`、`octaves: 4`）
- `pub fn generate_terrain(world: TorusSize, params: &GenParams) -> Result<ChunkGrid, WorldError>`

高度阈值（全部千分比整数）：

| 高度 | 地形 |
|---|---|
| `< sea_level` | 深水 |
| `< sea_level + 50` | 浅水 |
| `< sea_level + 100` | 沙 |
| `< mountain_level − 150` | 草 |
| `< mountain_level − 50` | 林 |
| `< mountain_level` | 丘 |
| `< mountain_level + 100` | 山 |
| 其余 | 雪 |

> **入口必须校验世界宽高都是 `CELL_SIZE` 的整数倍**，否则噪声接缝不连续。不满足时返回 `WorldError::WorldNotTileable`。

- [ ] **Step 1–4：TDD 循环**

测试至少覆盖：
- `相同种子生成完全相同的地形`（逐格比对整张地图）
- `不同种子生成不同的地形`
- `世界宽度不是格子尺寸整数倍时生成失败`
- `海平面调高会增加水域格数`
- `东西接缝两侧的地形一致`——噪声无缝不等于地形无缝，阈值判断也可能引入不连续，必须单独验
- `南北接缝两侧的地形一致`

- [ ] **Step 5：提交**

```bash
git commit -F - <<'EOF'
feat: 环面无缝地形生成

高度阈值全部用千分比整数表达，与噪声输出区间一致，全程无浮点。

generate_terrain 在入口校验世界宽高必须是噪声格子尺寸的整数倍——不满足
时接缝处会不连续，玩家跨越世界边界会看到地形突变。与其让缺陷以视觉异常
的形式出现在运行时，不如在生成入口直接拒绝。

「接缝两侧地形一致」单独立测试，而不是依赖噪声层那条无缝性属性测试：
噪声无缝不等于地形无缝，阈值判断本身也可能引入不连续。
EOF
```

---

### Task 4：对称阴影投射视野

**Files:** `crates/ll-world/src/fov.rs`、`crates/ll-world/tests/fov_blackbox.rs`

**Interfaces Produces:**
- `pub struct VisibleSet`，方法 `contains(TorusPos) -> bool`、`len() -> usize`、`iter() -> impl Iterator<Item = TorusPos> + '_`
- `pub fn compute_fov(grid: &ChunkGrid, origin: TorusPos, radius: u32) -> VisibleSet`

> **必须用对称阴影投射**（symmetric shadowcasting），不是朴素射线投射。
>
> 非对称算法会产生「我看得见你、你看不见我」的局面——这在回合制里是**直接的玩法缺陷**：玩家会被自己看不见的敌人攻击，而那个敌人在视野判定里明明能看见玩家。
>
> 对称性必须由属性测试守护：`A 能看见 B ⟺ B 能看见 A`。

- [ ] **Step 1–4：TDD 循环**

单元测试至少覆盖：
- `原点自身恒可见`
- `半径为零时只看见原点`
- `墙后的格子不可见`
- `墙本身可见`——若墙也不可见，玩家会看到一圈无法解释的黑边，分不清是墙还是未探索区域
- `开阔地带的可见格数接近圆面积`

属性测试（`tests/fov_blackbox.rs`）：
- `视野是对称的`——随机地形上随机取两点，断言互相可见性一致
- `可见格恒在半径之内`（用 `TorusSize::chebyshev`）
- `任意输入都不崩溃`（含半径极大、原点贴着世界边缘）

- [ ] **Step 5：提交**

```bash
git commit -F - <<'EOF'
feat: 对称阴影投射视野

用对称阴影投射而非朴素射线投射。非对称算法会产生「我看得见你、你看不见
我」的局面——这在回合制里是直接的玩法缺陷：玩家会被自己看不见的敌人
攻击，而那个敌人在视野判定里明明能看见玩家。

对称性由属性测试守护（随机地形上随机取两点，断言互相可见性一致），
因为这条性质靠手写用例几乎不可能覆盖到真正出问题的那些几何配置。

墙本身可见而墙后不可见——若墙也不可见，玩家会看到一圈无法解释的黑边，
分不清那是墙还是未探索区域。
EOF
```

---

### Task 5：昼夜四季与光照

**Files:** `crates/ll-world/src/light.rs`

**Interfaces Produces:**
- `pub struct LightLevel(pub i32)`（`0..=1000` 千分比）
- `pub fn ambient_light(tick: Tick) -> LightLevel`
- `pub fn season_light_scale(season: Season) -> i32`（千分比：夏 1000、春秋 900、冬 750）
- `pub fn sight_radius_at(base_radius: u32, light: LightLevel) -> u32`

规则：正午 1000，午夜 100，日出（5–7 点）与日落（17–19 点）线性渐变。视野半径按光照缩放，**下限为 1**。

- [ ] **TDD 循环**，测试至少覆盖：`正午光照最强`、`午夜光照不为零`、`日出时段光照递增`、`冬季光照弱于夏季`、`视野半径下限为一`、`光照为零时视野仍为一`

- [ ] **提交**

```bash
git commit -F - <<'EOF'
feat: 昼夜四季驱动的环境光照

午夜光照取 100 而非 0：全黑会让玩家什么都做不了，那不是难度而是卡住。
视野半径按光照缩放但下限为 1，理由相同——永远至少能看见脚下这一格。

光照与视野半径都是从世界时钟纯函数派生的，不存进世界状态。存进去必然
与时钟失同步，而这类缺陷会表现为「白天却一片漆黑」这种极难复现的现象。

冬季缩放取 750、明显低于其余季节，是为了让冬季在玩法上真正有压迫感——
四季若只是换个色板，就没有存在的必要。
EOF
```

---

### Task 6：`WorldState` 与序列化往返

**Files:** `crates/ll-world/src/state.rs`、`crates/ll-world/tests/determinism.rs`

**Interfaces Produces:**
- `pub struct WorldState { pub seed: u64, pub clock: Tick, pub size: TorusSize, pub terrain: ChunkGrid }`，全部派生 `Serialize`/`Deserialize`
- `WorldState::new(size: TorusSize, params: &GenParams) -> Result<WorldState, WorldError>`
- `WorldState::advance(&mut self, ticks: i64)`
- `WorldState::hash(&self) -> u64`

> `WorldState` **必须可完整序列化且无浮点**——这是模式3 自由读档的地基。
>
> `TorusSize` 与 `Tick` 目前可能未派生 serde；若未派生，**在 `ll-core` 中补上派生**（这是本任务允许触碰 `ll-core` 的唯一理由，且只加派生、不改逻辑）。

- [ ] **TDD 循环**，测试至少覆盖：
- `序列化往返后世界哈希不变`
- `相同种子与尺寸生成的世界哈希相同`
- `推进时钟会改变世界哈希`
- `世界尺寸下限常量与渲染层相机跨度一致`——守 Task 1 里写死的 43×25

黄金基准（`tests/determinism.rs`）：固定种子生成 64×64 世界并冻结哈希。**文件顶部必须写明「测试挂了不许直接把期望值改成实际值」的规矩**，与 `ll-core/tests/determinism.rs` 保持一致。

- [ ] **提交**

---

### Task 7：大陆地图与小地图数据视图

**Files:** `crates/ll-world/src/overview.rs`

**Interfaces Produces:**
- `pub struct OverviewCell { pub terrain: TerrainKind, pub explored: bool }`
- `pub fn minimap(world: &WorldState, center: TorusPos, span: u32) -> Vec<OverviewCell>`
- `pub fn continent_map(world: &WorldState, downsample: u32) -> Vec<OverviewCell>`

> 两者都是**只读数据视图，不持有状态、不缓存**。缓存会与世界状态失同步，而地图显示错误极难被玩家报告清楚（「地图上有座山，走过去却没有」这种描述无法定位）。

- [ ] **TDD 循环 + 提交**

---

### Task 8：P2 验收 Demo

**Files:** `crates/ll-world/examples/p2_acceptance.rs`

接上 P1 的渲染层，画出真实地形与视野。必须展示：

1. 一张真实生成的环面地形（能看出水 / 沙 / 草 / 林 / 山 / 雪的分布）
2. **走到世界边缘时地形无缝绕回**——环面拓扑与可平铺噪声的合验
3. 视野随移动更新，**墙后的格子不可见**
4. 按键推进世界时钟，**画面随昼夜变暗变亮、随季节变色**
5. 小地图显示在角落
6. 按 M 存图作为 P2 的视觉回归基准

**必须实测**：跑起来、无 wgpu validation error、拿到非全黑的渲染结果，并**如实报告哪些验证了、哪些没有**。

- [ ] **提交**

---

## 自查

### 完整调用链（P1 的教训要求的一节）

从玩家按键到屏幕出图，逐个 API 点名，确认每一步的参数都能从上一步取到：

```
InputState::was_activated(GameKey::Right)                     ← ll-platform
  ↓ 玩家移动意图
world.size.wrap(旧位置.x() + 1, 旧位置.y())                    ← ll-core::torus
  ↓ TorusPos（新位置）
ambient_light(world.clock)                                    ← ll-world::light
  ↓ LightLevel ✓（world.clock 是 Tick）
sight_radius_at(基础半径, 光照)                                ← ll-world::light
  ↓ u32 半径 ✓
compute_fov(&world.terrain, 新位置, 半径)                      ← ll-world::fov
  ↓ 需要 &ChunkGrid ✓（world.terrain）、TorusPos ✓、u32 ✓
  ↓ VisibleSet
Camera { center: 新位置, world: world.size }                   ← ll-render::camera
  ↓ 需要 TorusPos ✓、TorusSize ✓
camera.visible_tiles()
  ↓ Vec<TorusPos> ✓
对每个 tile：
  world.terrain.terrain_at(tile)      → TerrainKind ✓
  visible.contains(tile)              → bool ✓
  地形 → 图集条目名                     ← 本 crate 的映射表（Task 8 新建）
  atlas.uv_rect(名字)                 → Option<[f32; 4]> ✓   ← ll-render::atlas
  atlas.metadata().lookup(名字)       → Option<&AtlasEntry> ✓
  entry.sprite_size() / entry.footprint → 视觉尺寸与占地 ✓
  camera.world_to_screen(tile)        → (i32, i32) ✓
  DrawOrder::new(Layer::TERRAIN, 脚底Y, 实体号) ✓             ← ll-render::sprite
  batch.push(order, SpriteInstance { .. }) ✓                 ← ll-render::batch
batch.flush(&gpu, render_target.view())                        ← ll-render
  ↓ 需要 &GpuContext ✓、&TextureView ✓（render_target.view()）
gpu.acquire_frame()                   → SurfaceTexture ✓      ← ll-render::gpu
frame.texture.create_view(..)         → TextureView ✓（wgpu 经 ll_render::wgpu 重导出 ✓）
render_target.blit_to(&gpu, &view, fit_viewport(窗口宽, 窗口高)) ✓
gpu.queue().present(frame) ✓
```

**这条链上每一步的参数都能从上一步或已有状态取到，无断裂。** 唯一需要本阶段新建的是「`TerrainKind` → 图集条目名」的映射表，已列入 Task 8。

### 规格覆盖

| 规格要求 | 对应任务 |
|---|---|
| 决策 7 大陆地图 + 分区场景、小地图 | Task 7 |
| 决策 23 真环面四面全连通 | Task 2、Task 3 |
| §7.2 世界时钟、昼夜、四季 | Task 5 |
| §7.3 地形与视野 | Task 1、Task 4 |
| §11.2 存档（世界状态可完整序列化） | Task 6 |
| §14.2 属性测试：接缝连续、序列化往返 | Task 2、Task 3、Task 6 |
| §14.4 跨平台确定性 | Task 2（整数噪声）、Task 6（黄金基准） |
| §15 每阶段交付验收 demo | Task 8 |
| 交接：世界维度须大于 43×25 | Task 1（构造点校验） |
| 交接：浮点不得回流世界状态 | 全局约束、Task 2、Task 5 |

### 与规格的一处偏离（实施第一步须先处理）

规格 §14.2 写的是「**4D 噪声**地形：接缝处连续」。本计划改用**模格点整数噪声**——「接缝连续」这条性质不变，但实现手段不同。理由：4D 投影是为浮点噪声设计的绕法，而本项目禁止浮点进入世界状态。

**实施的第一步应先更新规格 §14.2 的措辞**，避免文档与代码不一致（§13 明确视其为缺陷）。

### 有意留给后续阶段的缺口

- **分区场景**（有界局部地图）本阶段只在 Task 7 提供数据视图；真正的场景加载与切换属 P5。
- **寻路**不在 P2。`TerrainKind::move_cost` 已就位，供 P3 的行动结算与 P7 的行为树使用。
- **实体存储**不在 P2。`WorldState` 本阶段只含种子、时钟、尺寸、地形；实体随 P3 的 Intent/Effect 管线一并加入。
- **`cargo-llvm-cov` 与 `cargo-mutants`** 仍未接入 CI。P2 结束时代码量已足够支撑，建议在 P2 收尾单独处理。
- **CI 视觉回归自动比对**仍需先验证「无独显环境的软件后端回退」（P1 交接的未决项）。
