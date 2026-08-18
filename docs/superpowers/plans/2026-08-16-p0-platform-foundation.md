# P0 平台地基 实施计划

> **给执行者：** 必须配合 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐任务实施。步骤使用 `- [ ]` 复选框追踪。

**目标：** 搭起 Cargo workspace 与两个最底层 crate（`ll-core`、`ll-platform`），并建立全套 CI 质量门禁，使后续所有阶段都有可依赖的地基与可信的测试基础设施。

**架构：** `ll-core` 是零依赖的纯数据层，提供环面坐标、命名空间 ID、确定性 RNG、世界时间与定标整数类型；`ll-platform` 在其上封装窗口、输入、日志与并行任务池。两者之间依赖单向，由 Cargo 物理强制。本阶段不涉及渲染与游戏逻辑。

**技术栈：** Rust 2024 edition、`winit` 0.30.x（稳定线，**不用 0.31 beta**）、`tracing` 0.1、`tracing-subscriber` 0.3、`rayon` 1.12、`crossbeam-channel` 0.5、`proptest` 1.11、`criterion` 0.8

**规格：** [`docs/superpowers/specs/2026-08-16-lostland-design.md`](../specs/2026-08-16-lostland-design.md)

## 全局约束

以下逐条摘自规格，**每个任务的要求都隐含包含本节**：

- **世界状态禁止使用浮点数。** 位置、时间、货币、属性、比例一律整数。浮点仅允许出现在渲染与音频层，且不得回流入世界状态。理由：跨平台浮点差异会摧毁确定性存档与重放（规格 §17 高风险项）。
- 所有随机性来自 `hash(世界种子, 实体ID, 事件计数)` 的确定性流，**禁止全局 RNG**（规格 C3）。
- 环面距离必须走 `ll-core` 提供的类型，**任何地方禁止手写欧氏距离**（规格 §7.1）。
- 代码中禁止出现硬编码的用户可见字符串（规格 §11.3）。
- 文件 200–400 行为宜，800 行为上限（规格 §13）。
- 所有公开项必须有文档注释，注释解释**为什么**而非复述代码（规格 §13）。
- 依赖许可证必须在 MIT / Apache-2.0 / BSD / zlib / ISC / Unicode-DFS / OFL-1.1 之内（规格 §3）。
- 测试遵循 AAA 结构，测试名描述行为，一个测试只断言一件事（规格 §14.6）。
- 提交信息格式 `<type>: <描述>`，正文说明**为什么**，**不加任何 AI 署名**（`knowledge/workflow/branching.md`）。

## 文件结构

```
Cargo.toml                          workspace 根清单
deny.toml                           cargo-deny 许可证策略
.github/workflows/ci.yml            CI 门禁
crates/
  ll-core/
    Cargo.toml
    src/lib.rs                      模块声明
    src/scaled.rs                   Milli：千分之一定标整数
    src/torus.rs                    TorusSize / TorusPos / 三种距离度量
    src/ident.rs                    NamespacedId / Interner
    src/rng.rs                      splitmix64 与确定性 RNG
    src/time.rs                     Tick / 四季 / 昼夜
    src/hashing.rs                  StateHasher：世界状态摘要
    src/error.rs                    CoreError
    tests/torus_blackbox.rs         黑箱属性测试
    tests/ident_blackbox.rs         黑箱属性测试
    tests/determinism.rs            跨平台确定性黄金基准
    benches/torus.rs                criterion 基准
  ll-platform/
    Cargo.toml
    src/lib.rs                      模块声明 + PlatformError
    src/logging.rs                  tracing 初始化
    src/input.rs                    输入状态聚合
    src/window.rs                   winit 窗口与事件循环
    src/jobs.rs                     rayon 任务池 + crossbeam 通道
    tests/input_blackbox.rs         黑箱状态机属性测试
    examples/p0_acceptance.rs       P0 验收 demo
```

---

### Task 1：Workspace 骨架与 CI 门禁

**Files:**
- Modify: `Cargo.toml`（从单 crate 改为 workspace）
- Delete: `src/main.rs`
- Create: `deny.toml`
- Create: `.github/workflows/ci.yml`
- Create: `crates/ll-core/Cargo.toml`、`crates/ll-core/src/lib.rs` 及七个占位模块

**Interfaces:**
- Consumes: 无（首个任务）
- Produces: 可编译的 workspace；`ll-core` crate 存在且可被后续 crate 依赖

- [ ] **Step 1：改写根 `Cargo.toml` 为 workspace**

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"

# 依赖版本在此统一管理，各 crate 用 workspace = true 引用。
# 这样升级依赖只需改一处，避免各 crate 版本漂移。
[workspace.dependencies]
proptest = "1.11"
criterion = "0.8"

# 地基层的性能直接决定上层天花板，故 release 开满优化。
[profile.release]
lto = "thin"
codegen-units = 1

# 依赖也在开发构建下优化，否则 proptest 跑几千个用例会慢到没人愿意跑。
[profile.dev.package."*"]
opt-level = 2
```

- [ ] **Step 2：删除旧的单 crate 入口**

```bash
git rm src/main.rs
```

- [ ] **Step 3：创建 `crates/ll-core/Cargo.toml`**

```toml
[package]
name = "ll-core"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "迷途大陆的纯数据基础层：环面坐标、确定性随机、命名空间标识、世界时间"

# ll-core 必须保持零运行时依赖。它被所有其他 crate 依赖，
# 任何引入这里的依赖都会传染给整个项目。
[dependencies]

[dev-dependencies]
proptest.workspace = true
criterion.workspace = true

[[bench]]
name = "torus"
harness = false
```

- [ ] **Step 4：创建 `crates/ll-core/src/lib.rs`**

```rust
//! 迷途大陆的纯数据基础层。
//!
//! 本 crate 被项目中所有其他 crate 依赖，因此**必须保持零运行时依赖**。
//! 任何引入这里的第三方依赖都会传染给整个项目。
//!
//! 设计约束（源自总纲规格）：
//! - 世界状态禁止浮点数。跨平台浮点差异会摧毁确定性存档与重放。
//! - 随机性只能来自按实体 ID 派生的确定性流，禁止全局 RNG。
//! - 环面距离只能通过本 crate 的类型计算，禁止在别处手写。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod hashing;
pub mod ident;
pub mod rng;
pub mod scaled;
pub mod time;
pub mod torus;
```

- [ ] **Step 5：创建七个占位模块使其可编译**

为让本任务可独立验证，先建七个最小模块文件，各写一行文档注释。后续任务逐个填充。

```rust
// crates/ll-core/src/error.rs
//! 本层的错误类型。由 Task 2 填充。
```

其余六个同理，各写一行对应的文档注释：
- `hashing.rs` → `//! 世界状态摘要。由 Task 11 填充。`
- `ident.rs` → `//! 命名空间标识符。由 Task 4 填充。`
- `rng.rs` → `//! 确定性随机数。由 Task 5 填充。`
- `scaled.rs` → `//! 定标整数。由 Task 2 填充。`
- `time.rs` → `//! 世界时间。由 Task 6 填充。`
- `torus.rs` → `//! 环面坐标。由 Task 3 填充。`

- [ ] **Step 6：创建 `deny.toml`**

```toml
# 许可证门禁。规格 §3 要求全传递依赖树只允许宽松许可，
# 出现清单外许可证即构建失败——避免项目走到一半才发现
# 某个深层依赖是 GPL，被迫大改。

[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "OFL-1.1",
]
confidence-threshold = 0.9

[bans]
multiple-versions = "warn"

[advisories]
yanked = "deny"
```

- [ ] **Step 7：创建 `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -D warnings

jobs:
  # 双平台矩阵是硬要求：规格 §14.4 要求同一种子在 Windows 与 Linux
  # 上产出逐位相同的世界哈希。单平台 CI 无法发现确定性被破坏。
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: 格式检查
        run: cargo fmt --all --check
      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: 测试
        run: cargo test --workspace --all-targets

  licenses:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-deny
      - name: 许可证与安全公告扫描
        run: cargo deny check
```

- [ ] **Step 8：验证 workspace 可编译且门禁本地可跑**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```
预期：全部通过（此时尚无测试，`cargo test` 报 0 passed）。

- [ ] **Step 9：提交**

```bash
git add -A
git commit -F - <<'EOF'
chore: 建立 workspace 骨架与 CI 门禁

从单 crate 改为 workspace，因为 crate 边界即并行任务边界——依赖方向
由 Cargo 物理强制单向，组员各据一个 crate 就不会互相踩。

CI 采用 Windows + Linux 双平台矩阵。这不是「支持两个平台」这种泛泛
理由，而是规格 §14.4 要求同一种子在两平台产出逐位相同的世界哈希；
单平台 CI 无法发现确定性被破坏。

deny.toml 从第一个提交起就位，避免走到项目中期才发现某个深层传递
依赖是 GPL。
EOF
```

---

### Task 2：`Milli` 定标整数与 `CoreError`

**Files:**
- Modify: `crates/ll-core/src/scaled.rs`
- Modify: `crates/ll-core/src/error.rs`

**Interfaces:**
- Consumes: Task 1 的 crate 骨架
- Produces:
  - `pub const SCALE: i64 = 1_000`
  - `pub struct Milli(pub i64)`，常量 `Milli::ZERO`
  - `Milli::from_whole(whole: i64) -> Milli`
  - `Milli::whole(&self) -> i64`
  - `Milli::checked_mul_ratio(self, numerator: i64, denominator: i64) -> Option<Milli>`
  - `Milli::mul_ratio(self, numerator: i64, denominator: i64) -> Result<Milli, CoreError>`
  - `pub enum CoreError { InvalidIdentifier(String), DivisionByZero, Overflow }`

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-core/src/scaled.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 整数转定标值时放大一千倍() {
        // Arrange
        let whole = 7_i64;

        // Act
        let scaled = Milli::from_whole(whole);

        // Assert
        assert_eq!(scaled.0, 7_000);
    }

    #[test]
    fn 取整时向零截断而非向下取整() {
        // 向下取整会让「亏损 1.5 金币」变成亏 2 金币，与正数方向不对称，
        // 经济结算会产生系统性偏移。
        // Arrange
        let negative = Milli(-1_500);

        // Act
        let whole = negative.whole();

        // Assert
        assert_eq!(whole, -1);
    }

    #[test]
    fn 按比例缩放时分母为零返回空值() {
        // Arrange
        let value = Milli(1_000);

        // Act
        let result = value.checked_mul_ratio(3, 0);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 按比例缩放在溢出时返回空值而非回绕() {
        // Arrange
        let huge = Milli(i64::MAX);

        // Act
        let result = huge.checked_mul_ratio(2, 1);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 比例缩放的错误形式区分除零与溢出() {
        // Arrange
        let value = Milli(1_000);

        // Act
        let divide = value.mul_ratio(1, 0);
        let overflow = Milli(i64::MAX).mul_ratio(2, 1);

        // Assert
        assert_eq!(divide, Err(CoreError::DivisionByZero));
        assert_eq!(overflow, Err(CoreError::Overflow));
    }
}
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-core scaled
```
预期：编译失败，`cannot find type Milli in this scope`。

- [ ] **Step 3：实现 `scaled.rs`**

```rust
//! 定标整数：用整数表达需要小数精度的世界量。
//!
//! 世界状态**禁止使用浮点数**——同一段 `f64` 运算在 Windows 与 Linux 上
//! 可能产出不同的最低位，而确定性存档与重放要求两平台逐位一致。
//! 因此凡需要小数的世界量（价格、比例、速率）一律用本类型表达。
//!
//! 浮点仅允许出现在渲染与音频层，且结果不得回流入世界状态。

use crate::error::CoreError;

/// 每个整数单位对应的定标刻度数。
///
/// 取一千是因为经济系统的价格需要到「厘」的精度：若只到「分」，
/// 大宗商品按百分比抽税时的舍入误差会在长期模拟中累积成可观偏差。
pub const SCALE: i64 = 1_000;

/// 以千分之一为单位的定标整数。
///
/// `Milli(1_500)` 表示 1.5。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Milli(pub i64);

impl Milli {
    /// 零值。
    pub const ZERO: Milli = Milli(0);

    /// 由整数构造，自动放大 [`SCALE`] 倍。
    pub const fn from_whole(whole: i64) -> Self {
        Milli(whole * SCALE)
    }

    /// 取整数部分，**向零截断**。
    ///
    /// 选择向零截断而非向下取整，是为了让正负方向对称：向下取整会使
    /// `-1.5` 变成 `-2` 而 `1.5` 变成 `1`，经济结算中的盈亏就会产生
    /// 系统性偏移。
    pub const fn whole(&self) -> i64 {
        self.0 / SCALE
    }

    /// 按分数比例缩放，溢出或分母为零时返回 [`None`]。
    ///
    /// 用分数而非小数表达比例，同样是为了避开浮点。中间乘积用 [`i128`]
    /// 承接，避免先乘后除时的假溢出。
    pub fn checked_mul_ratio(self, numerator: i64, denominator: i64) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        let product = (self.0 as i128).checked_mul(numerator as i128)?;
        let quotient = product / (denominator as i128);
        i64::try_from(quotient).ok().map(Milli)
    }

    /// 按分数比例缩放，失败时返回具体错误原因。
    ///
    /// 供需要向上层报告失败原因的调用方使用。
    pub fn mul_ratio(self, numerator: i64, denominator: i64) -> Result<Self, CoreError> {
        if denominator == 0 {
            return Err(CoreError::DivisionByZero);
        }
        self.checked_mul_ratio(numerator, denominator)
            .ok_or(CoreError::Overflow)
    }
}
```

- [ ] **Step 4：实现 `error.rs`**

```rust
//! 基础层的错误类型。
//!
//! 刻意不引入 `thiserror` 等派生宏库：`ll-core` 必须保持零运行时依赖，
//! 而手写 `Display` 与 `Error` 的成本远低于让整个项目多背一个依赖。

use core::fmt;

/// 基础层可能产生的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// 命名空间标识符不合法，附带原始输入以便定位。
    InvalidIdentifier(String),
    /// 除数为零。
    DivisionByZero,
    /// 整数运算溢出。
    Overflow,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 此处文案面向开发者与日志，不面向玩家，故不走 i18n
        // （规格 §11.3 约束的是用户可见字符串）。
        match self {
            CoreError::InvalidIdentifier(raw) => {
                write!(f, "invalid namespaced identifier: {raw:?}")
            }
            CoreError::DivisionByZero => write!(f, "division by zero"),
            CoreError::Overflow => write!(f, "integer overflow"),
        }
    }
}

impl core::error::Error for CoreError {}
```

- [ ] **Step 5：运行测试确认通过**

```bash
cargo test -p ll-core scaled
```
预期：5 个测试全部 PASS。

- [ ] **Step 6：提交**

```bash
git add crates/ll-core/src/scaled.rs crates/ll-core/src/error.rs
git commit -F - <<'EOF'
feat: 定标整数 Milli 与基础错误类型

世界状态禁用浮点，因为同一段 f64 运算在 Windows 与 Linux 上可能产出
不同的最低位，而规格 §14.4 要求两平台的世界哈希逐位一致。凡需小数
精度的世界量改用千分之一定标整数表达。

取整选择向零截断而非向下取整：向下取整会让 -1.5 变成 -2 而 1.5 变成
1，正负不对称，经济结算的盈亏会产生系统性偏移。

比例运算用分数而非小数，中间乘积用 i128 承接以避免先乘后除时的假
溢出。

error.rs 手写 Display 而不引入 thiserror，是为了守住 ll-core 零运行时
依赖——它被所有 crate 依赖，任何依赖都会传染全项目。
EOF
```

---

### Task 3：环面坐标与距离度量

**Files:**
- Modify: `crates/ll-core/src/torus.rs`
- Create: `crates/ll-core/tests/torus_blackbox.rs`
- Create: `crates/ll-core/benches/torus.rs`

**Interfaces:**
- Consumes: Task 1 的 crate 骨架
- Produces:
  - `pub struct TorusSize`，构造 `TorusSize::new(width: u32, height: u32) -> Option<TorusSize>`，访问器 `width() -> u32`、`height() -> u32`
  - `pub struct TorusPos`，访问器 `x() -> i32`、`y() -> i32`
  - `TorusSize::wrap(&self, x: i32, y: i32) -> TorusPos`
  - `TorusSize::delta(&self, from: TorusPos, to: TorusPos) -> (i32, i32)`
  - `TorusSize::chebyshev(&self, a: TorusPos, b: TorusPos) -> u32`
  - `TorusSize::manhattan(&self, a: TorusPos, b: TorusPos) -> u32`
  - `TorusSize::squared_euclidean(&self, a: TorusPos, b: TorusPos) -> u64`

- [ ] **Step 1：写失败的单元测试（白箱）**

```rust
// 追加到 crates/ll-core/src/torus.rs 末尾
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
}
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-core torus
```
预期：编译失败，`cannot find type TorusSize in this scope`。

- [ ] **Step 3：实现 `torus.rs`**

```rust
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
    /// 构造世界尺寸。任一维度为零时返回 [`None`]。
    ///
    /// 零尺寸世界无法定义取模运算，与其在运行时除零崩溃，不如在构造点
    /// 就拒绝。
    pub const fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
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
```

- [ ] **Step 4：运行测试确认通过**

```bash
cargo test -p ll-core torus
```
预期：5 个测试全部 PASS。

- [ ] **Step 5：写黑箱属性测试**

```rust
// crates/ll-core/tests/torus_blackbox.rs
//! 环面坐标的黑箱属性测试。
//!
//! 本文件位于 `tests/`，只能访问 `ll-core` 的公开 API，看不到任何内部
//! 实现。这个限制是刻意的：它能发现「改了内部实现就崩」的脆弱设计。
//!
//! 具体用例只能证明「这一个输入是对的」；属性测试证明「所有输入都满足
//! 某不变量」。环面几何正是属性测试的理想对象——手写用例几乎不可能
//! 覆盖到所有绕法组合。

use ll_core::torus::TorusSize;
use proptest::prelude::*;

proptest! {
    #[test]
    fn 绕回后的坐标恒落在世界范围内(
        w in 1u32..500,
        h in 1u32..500,
        x in -1_000_000i32..1_000_000,
        y in -1_000_000i32..1_000_000,
    ) {
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");

        // Act
        let pos = world.wrap(x, y);

        // Assert
        prop_assert!(pos.x() >= 0 && pos.x() < w as i32);
        prop_assert!(pos.y() >= 0 && pos.y() < h as i32);
    }

    #[test]
    fn 切比雪夫距离对称(
        w in 1u32..500, h in 1u32..500,
        ax in 0i32..500, ay in 0i32..500,
        bx in 0i32..500, by in 0i32..500,
    ) {
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");
        let a = world.wrap(ax, ay);
        let b = world.wrap(bx, by);

        // Act & Assert
        prop_assert_eq!(world.chebyshev(a, b), world.chebyshev(b, a));
    }

    #[test]
    fn 任意两点的单轴距离不超过半个世界(
        w in 1u32..500, h in 1u32..500,
        ax in 0i32..500, ay in 0i32..500,
        bx in 0i32..500, by in 0i32..500,
    ) {
        // 这是环面拓扑的定义性质：绕远路永远不是最短路。
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");
        let a = world.wrap(ax, ay);
        let b = world.wrap(bx, by);

        // Act
        let (dx, dy) = world.delta(a, b);

        // Assert
        prop_assert!(dx.unsigned_abs() * 2 <= w);
        prop_assert!(dy.unsigned_abs() * 2 <= h);
    }

    #[test]
    fn 东西接缝处连续(w in 1u32..500, h in 1u32..500, y in 0i32..500) {
        // 地形生成依赖这条性质：若接缝不连续，玩家跨越边界时会看到
        // 地形突变。
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");

        // Act
        let west_edge = world.wrap(0, y);
        let east_wrapped = world.wrap(w as i32, y);

        // Assert
        prop_assert_eq!(west_edge, east_wrapped);
    }

    #[test]
    fn 曼哈顿距离不小于切比雪夫距离(
        w in 1u32..500, h in 1u32..500,
        ax in 0i32..500, ay in 0i32..500,
        bx in 0i32..500, by in 0i32..500,
    ) {
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");
        let a = world.wrap(ax, ay);
        let b = world.wrap(bx, by);

        // Act & Assert
        prop_assert!(world.manhattan(a, b) >= world.chebyshev(a, b));
    }
}
```

- [ ] **Step 6：运行属性测试**

```bash
cargo test -p ll-core --test torus_blackbox
```
预期：5 个属性各跑 256 个随机用例，全部 PASS。

- [ ] **Step 7：写性能基准**

```rust
// crates/ll-core/benches/torus.rs
//! 环面距离的性能基准。
//!
//! 距离计算会在视野、寻路、AI 目标选择中被每帧调用成千上万次，是最
//! 容易悄悄劣化的热点。基准的目的不是追求某个绝对数字，而是让后续
//! 改动引入的性能回归立刻可见。

use criterion::{Criterion, criterion_group, criterion_main};
use ll_core::torus::TorusSize;
use std::hint::black_box;

fn 切比雪夫距离基准(c: &mut Criterion) {
    let world = TorusSize::new(4096, 4096).expect("宽高均不为零");
    let a = world.wrap(10, 20);
    let b = world.wrap(4090, 4000);

    c.bench_function("chebyshev_across_seam", |bencher| {
        bencher.iter(|| world.chebyshev(black_box(a), black_box(b)))
    });
}

criterion_group!(benches, 切比雪夫距离基准);
criterion_main!(benches);
```

- [ ] **Step 8：验证基准可运行**

```bash
cargo bench -p ll-core --bench torus -- --test
```
预期：以测试模式跑通，不报错。

- [ ] **Step 9：提交**

```bash
git add crates/ll-core/src/torus.rs crates/ll-core/tests/torus_blackbox.rs crates/ll-core/benches/torus.rs
git commit -F - <<'EOF'
feat: 环面坐标与三种距离度量

大陆地图四面全连通，两点之间存在四条候选路径。把距离做成 TorusSize
的方法而非 TorusPos 的方法，是因为脱离世界尺寸时环面距离根本无法
定义——这个 API 形状让「忘记传尺寸」变成编译错误，而不是运行时的
错误答案。

TorusPos 字段私有、只能经 wrap 构造，以保证「坐标恒被规范化」这条
不变式无法从外部破坏。内部用 rem_euclid 而非 %，因为 Rust 的 % 对
负数返回负余数（-3 % 10 得 -3），会直接破坏不变式。

位移恰为半周时固定取正方向。必须有稳定的打破平局规则，否则同一局面
在不同调用间可能返回不同结果，破坏确定性重放。

只提供欧氏距离的平方而不开方：开方会引入浮点，而世界状态禁用浮点；
比较远近时平方值与原值单调等价。

属性测试放在 tests/ 而非内联，因为黑箱视角能发现「改了内部实现就崩」
的脆弱设计。环面几何是属性测试的理想对象——手写用例几乎不可能覆盖
所有绕法组合。
EOF
```

---

### Task 4：命名空间标识符与索引池

**Files:**
- Modify: `crates/ll-core/src/ident.rs`
- Create: `crates/ll-core/tests/ident_blackbox.rs`

**Interfaces:**
- Consumes: Task 2 的 `CoreError`
- Produces:
  - `pub struct NamespacedId`，构造 `NamespacedId::parse(raw: &str) -> Result<NamespacedId, CoreError>`，访问器 `namespace() -> &str`、`path() -> &str`，实现 `Display`
  - `pub struct ContentIndex`，访问器 `get() -> u32`
  - `pub struct Interner`，方法 `new() -> Interner`、`intern(&mut self, id: NamespacedId) -> ContentIndex`、`resolve(&self, index: ContentIndex) -> Option<&NamespacedId>`、`len() -> usize`、`is_empty() -> bool`

- [ ] **Step 1：写失败的单元测试**

```rust
// 追加到 crates/ll-core/src/ident.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析合法标识符拆出命名空间与路径() {
        // Arrange
        let raw = "lostland:fireball";

        // Act
        let id = NamespacedId::parse(raw).expect("这是合法标识符");

        // Assert
        assert_eq!((id.namespace(), id.path()), ("lostland", "fireball"));
    }

    #[test]
    fn 缺少冒号时解析失败() {
        // Arrange
        let raw = "fireball";

        // Act
        let result = NamespacedId::parse(raw);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 含大写字母时解析失败() {
        // 强制小写是为了避免 MyMod:fire 与 mymod:fire 这类肉眼难辨的
        // 重复 ID。
        // Arrange
        let raw = "MyMod:fire";

        // Act
        let result = NamespacedId::parse(raw);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 路径中出现第二个冒号时解析失败() {
        // Arrange
        let raw = "mod:a:b";

        // Act
        let result = NamespacedId::parse(raw);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 同一标识符重复登记返回相同索引() {
        // Arrange
        let mut interner = Interner::new();
        let id = NamespacedId::parse("lostland:fireball").expect("合法");

        // Act
        let first = interner.intern(id.clone());
        let second = interner.intern(id);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 索引可反查回原标识符() {
        // 存档必须能把整数索引写回字符串，否则玩家调整 mod 加载顺序后，
        // 存档里的火球会变成一把椅子。
        // Arrange
        let mut interner = Interner::new();
        let id = NamespacedId::parse("yourmod:super_fire").expect("合法");
        let index = interner.intern(id.clone());

        // Act
        let resolved = interner.resolve(index);

        // Assert
        assert_eq!(resolved, Some(&id));
    }
}
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-core ident
```
预期：编译失败，`cannot find type NamespacedId in this scope`。

- [ ] **Step 3：实现 `ident.rs`**

```rust
//! 内容的命名空间标识符与运行时索引池。
//!
//! # 为什么 ID 必须是字符串而不是整数
//!
//! 本项目遵循「本体即 Mod」原则：本体内容与 mod 内容走完全相同的注册
//! 通道。若 ID 是裸整数，两个 mod 必然撞号。命名空间字符串
//! （`lostland:fireball`、`yourmod:fireball`）从根本上杜绝冲突。
//!
//! # 为什么还需要整数索引
//!
//! 字符串比较与哈希对每帧执行的热路径来说太慢。因此装载完成后把所有
//! 字符串 ID 一次性映射为紧凑整数：**外部看字符串保证不冲突，内部用
//! 整数保证性能**。
//!
//! # 存档必须写字符串
//!
//! 索引依赖加载顺序。若存档里写的是索引，玩家调整 mod 顺序后，存档中
//! 的火球会变成一把椅子。故存档需持久化字符串，或在存档头保存
//! 「索引 ↔ 字符串」映射表。

use crate::error::CoreError;
use std::collections::HashMap;
use std::fmt;

/// 内容标识符，形如 `命名空间:路径`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NamespacedId {
    namespace: Box<str>,
    path: Box<str>,
}

impl NamespacedId {
    /// 解析 `命名空间:路径` 形式的标识符。
    ///
    /// 两部分均只允许小写字母、数字、下划线、连字符与点号，且不得为空。
    /// 强制小写是为了避免 `MyMod:Fire` 与 `mymod:fire` 这类肉眼难辨的
    /// 重复 ID——这种冲突在 mod 生态里极难排查。
    pub fn parse(raw: &str) -> Result<Self, CoreError> {
        let invalid = || CoreError::InvalidIdentifier(raw.to_owned());

        // 用 split_once 而非 split(':')，因为路径中不允许再出现冒号；
        // 出现即视为非法，而不是静默忽略后半段。
        let (namespace, path) = raw.split_once(':').ok_or_else(invalid)?;

        if !is_valid_segment(namespace) || !is_valid_segment(path) {
            return Err(invalid());
        }

        Ok(NamespacedId {
            namespace: namespace.into(),
            path: path.into(),
        })
    }

    /// 命名空间部分，通常是 mod 的唯一名称。
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// 路径部分，标识该命名空间内的具体内容。
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// 判断标识符的一个段落是否合法。
fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.'))
}

impl fmt::Display for NamespacedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

/// 内容在运行时的紧凑索引。
///
/// **不可持久化**——索引依赖 mod 加载顺序，存档必须写字符串 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentIndex(u32);

impl ContentIndex {
    /// 取出底层原始索引值，供数组下标使用。
    pub const fn get(&self) -> u32 {
        self.0
    }
}

/// 字符串标识符与运行时索引之间的双向映射池。
#[derive(Debug, Default)]
pub struct Interner {
    to_index: HashMap<NamespacedId, ContentIndex>,
    to_id: Vec<NamespacedId>,
}

impl Interner {
    /// 建立空池。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个标识符并返回其索引。已登记者返回原索引。
    pub fn intern(&mut self, id: NamespacedId) -> ContentIndex {
        if let Some(existing) = self.to_index.get(&id) {
            return *existing;
        }
        // 索引即插入顺序下标，故 to_id 与 to_index 恒保持一致。
        let index = ContentIndex(self.to_id.len() as u32);
        self.to_id.push(id.clone());
        self.to_index.insert(id, index);
        index
    }

    /// 由索引反查标识符。存档写出时依赖此方法。
    pub fn resolve(&self, index: ContentIndex) -> Option<&NamespacedId> {
        self.to_id.get(index.get() as usize)
    }

    /// 已登记的标识符数量。
    pub fn len(&self) -> usize {
        self.to_id.len()
    }

    /// 池中是否尚无任何标识符。
    pub fn is_empty(&self) -> bool {
        self.to_id.is_empty()
    }
}
```

- [ ] **Step 4：运行测试确认通过**

```bash
cargo test -p ll-core ident
```
预期：6 个测试全部 PASS。

- [ ] **Step 5：写黑箱属性测试**

```rust
// crates/ll-core/tests/ident_blackbox.rs
//! 命名空间标识符的黑箱属性测试。

use ll_core::ident::{Interner, NamespacedId};
use proptest::prelude::*;

/// 生成合法段落的策略：小写字母、数字、下划线，长度 1..12。
fn 合法段落() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z0-9_]{1,12}").expect("正则合法")
}

proptest! {
    #[test]
    fn 解析与显示互为逆运算(ns in 合法段落(), path in 合法段落()) {
        // 存档写出时依赖 Display，读入时依赖 parse。两者若不互逆，
        // 存档就会在往返中损坏。
        // Arrange
        let raw = format!("{ns}:{path}");

        // Act
        let parsed = NamespacedId::parse(&raw).expect("由合法段落拼成");

        // Assert
        prop_assert_eq!(parsed.to_string(), raw);
    }

    #[test]
    fn 登记后索引反查恒得原标识符(ns in 合法段落(), path in 合法段落()) {
        // Arrange
        let mut interner = Interner::new();
        let id = NamespacedId::parse(&format!("{ns}:{path}")).expect("合法");

        // Act
        let index = interner.intern(id.clone());

        // Assert
        prop_assert_eq!(interner.resolve(index), Some(&id));
    }

    #[test]
    fn 任意输入都不会崩溃(raw in ".{0,64}") {
        // 标识符会来自第三方 mod 的清单文件，属于外部不可信输入。
        // 无论内容多畸形都只能返回 Err，绝不能 panic。
        // Act
        let _ = NamespacedId::parse(&raw);
    }
}
```

- [ ] **Step 6：运行属性测试**

```bash
cargo test -p ll-core --test ident_blackbox
```
预期：3 个属性全部 PASS。

- [ ] **Step 7：提交**

```bash
git add crates/ll-core/src/ident.rs crates/ll-core/tests/ident_blackbox.rs
git commit -F - <<'EOF'
feat: 命名空间标识符与运行时索引池

本体与 mod 走同一套注册通道，若 ID 是裸整数，两个 mod 必然撞号。
命名空间字符串从根本上杜绝冲突。但字符串哈希对每帧热路径太慢，故
装载后一次性映射为紧凑整数——外部看字符串保证不冲突，内部用整数
保证性能。

索引刻意标注为不可持久化：它依赖 mod 加载顺序，若存档写索引，玩家
调整 mod 顺序后存档里的火球会变成一把椅子。

强制小写是为了避免 MyMod:Fire 与 mymod:fire 这类肉眼难辨的重复 ID，
这种冲突在 mod 生态里极难排查。

解析用 split_once 而非 split(':')，路径中再出现冒号即视为非法，而不
是静默忽略后半段。

标识符来自第三方 mod 清单，属外部不可信输入，故加了「任意输入都不
崩溃」的属性测试。
EOF
```

---

### Task 5：确定性随机数

**Files:**
- Modify: `crates/ll-core/src/rng.rs`

**Interfaces:**
- Consumes: Task 1 的 crate 骨架
- Produces:
  - `pub struct DetRng`，构造 `DetRng::for_entity(world_seed: u64, entity_id: u64, event_counter: u64) -> DetRng`
  - `DetRng::next_u64(&mut self) -> u64`
  - `DetRng::gen_range(&mut self, exclusive_upper: u64) -> u64`
  - `DetRng::chance(&mut self, numerator: u32, denominator: u32) -> bool`

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-core/src/rng.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 相同三元组产出相同序列() {
        // 这是确定性重放与跨平台一致的基石。
        // Arrange
        let mut first = DetRng::for_entity(42, 7, 3);
        let mut second = DetRng::for_entity(42, 7, 3);

        // Act
        let a: Vec<u64> = (0..8).map(|_| first.next_u64()).collect();
        let b: Vec<u64> = (0..8).map(|_| second.next_u64()).collect();

        // Assert
        assert_eq!(a, b);
    }

    #[test]
    fn 不同实体在同一时刻产出不同序列() {
        // 若不同实体共享序列，一群怪物会做出完全相同的决策。
        // Arrange
        let mut first = DetRng::for_entity(42, 7, 0);
        let mut second = DetRng::for_entity(42, 8, 0);

        // Act
        let a = first.next_u64();
        let b = second.next_u64();

        // Assert
        assert_ne!(a, b);
    }

    #[test]
    fn 交换种子与实体号得到不同序列() {
        // 若三个输入是简单异或合成，(种子=1,实体=2) 与 (种子=2,实体=1)
        // 会得到同一条流，不同实体之间将出现可察觉的行为关联。
        // Arrange
        let mut first = DetRng::for_entity(1, 2, 0);
        let mut second = DetRng::for_entity(2, 1, 0);

        // Act & Assert
        assert_ne!(first.next_u64(), second.next_u64());
    }

    #[test]
    fn 取值范围上界为零时返回零() {
        // Arrange
        let mut rng = DetRng::for_entity(1, 1, 1);

        // Act
        let value = rng.gen_range(0);

        // Assert
        assert_eq!(value, 0);
    }

    #[test]
    fn 概率判定分母为零时恒为假() {
        // 与其除零崩溃，不如把无意义的概率当作永不发生。
        // Arrange
        let mut rng = DetRng::for_entity(1, 1, 1);

        // Act
        let hit = rng.chance(1, 0);

        // Assert
        assert!(!hit);
    }

    #[test]
    fn 概率为百分之百时恒为真() {
        // Arrange
        let mut rng = DetRng::for_entity(9, 9, 9);

        // Act & Assert
        for _ in 0..64 {
            assert!(rng.chance(1, 1));
        }
    }

    #[test]
    fn 取值恒落在指定范围内() {
        // Arrange
        let mut rng = DetRng::for_entity(0xDEAD_BEEF, 1, 1);
        let upper = 37_u64;

        // Act & Assert
        for _ in 0..10_000 {
            assert!(rng.gen_range(upper) < upper);
        }
    }

    #[test]
    fn 概率判定的实际频率接近标称值() {
        // 验证无模偏差。若用朴素的取余，此测试在小分母下会偏。
        // Arrange
        let mut rng = DetRng::for_entity(7, 7, 7);
        let trials = 100_000_usize;

        // Act
        let hits = (0..trials).filter(|_| rng.chance(1, 3)).count();

        // Assert
        let expected = trials / 3;
        let tolerance = trials / 50; // 允许 2% 偏差
        assert!(hits.abs_diff(expected) < tolerance);
    }
}
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-core rng
```
预期：编译失败，`cannot find type DetRng in this scope`。

- [ ] **Step 3：实现 `rng.rs`**

```rust
//! 确定性随机数。
//!
//! # 为什么禁止全局随机数流
//!
//! 全局流的取值结果取决于**谁先取**。一旦引入多线程并行结算，或读档后
//! 实体的处理顺序发生细微变化，整条序列就会错位，世界随之走向另一个
//! 平行宇宙——这会同时摧毁自由读档与确定性重放两项能力。
//!
//! 本模块的做法是让随机数由 `(世界种子, 实体 ID, 事件计数)` 三元组
//! **计算得出**而非从共享流中取出。同一个三元组在任何时候、任何线程、
//! 任何平台上都得到相同结果，因此并行结算天然安全，无需任何同步。
//!
//! 算法采用 splitmix64。选它的原因：实现只有几行、无需任何依赖
//! （`ll-core` 必须零依赖）、雪崩性质良好、且全部是整数运算，因而
//! 跨平台逐位一致。

/// splitmix64 的混合函数。
///
/// 常量取自算法原始定义，**不可随意更改**——它们经过雪崩性质验证，
/// 换成别的数会显著降低输出质量。
const fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 由 `(世界种子, 实体 ID, 事件计数)` 派生的确定性随机数发生器。
#[derive(Debug, Clone)]
pub struct DetRng {
    state: u64,
}

impl DetRng {
    /// 为某个实体在某个事件时刻派生一条随机流。
    ///
    /// 三个输入逐级混合而非简单异或：直接异或会让 `(种子=1, 实体=2)` 与
    /// `(种子=2, 实体=1)` 得到同一条流，造成不同实体之间出现可察觉的
    /// 行为关联。
    pub const fn for_entity(world_seed: u64, entity_id: u64, event_counter: u64) -> Self {
        let a = splitmix64(world_seed);
        let b = splitmix64(entity_id ^ a);
        let c = splitmix64(event_counter ^ b);
        DetRng { state: c }
    }

    /// 取下一个 64 位随机数。
    pub fn next_u64(&mut self) -> u64 {
        self.state = splitmix64(self.state);
        self.state
    }

    /// 取 `[0, exclusive_upper)` 内的随机数；上界为零时返回零。
    ///
    /// 采用 Lemire 的乘法取余法：比取余更快，且**无模偏差**。朴素的
    /// `next_u64() % n` 会让较小的值出现概率略高，这种偏差在百万次经济
    /// 模拟中会累积成可观的系统性倾斜。
    pub fn gen_range(&mut self, exclusive_upper: u64) -> u64 {
        if exclusive_upper == 0 {
            return 0;
        }
        let product = (self.next_u64() as u128) * (exclusive_upper as u128);
        (product >> 64) as u64
    }

    /// 以 `numerator / denominator` 的概率返回真。
    ///
    /// 分母为零时恒返回假——与其除零崩溃，不如把无意义的概率当作永不
    /// 发生，让上层的脚本错误降级策略能够接管。
    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        if denominator == 0 {
            return false;
        }
        self.gen_range(denominator as u64) < numerator as u64
    }
}
```

- [ ] **Step 4：运行测试确认通过**

```bash
cargo test -p ll-core rng
```
预期：8 个测试全部 PASS。

- [ ] **Step 5：提交**

```bash
git add crates/ll-core/src/rng.rs
git commit -F - <<'EOF'
feat: 由实体派生的确定性随机数

全局随机流的取值取决于「谁先取」。一旦并行结算或读档后实体处理顺序
有细微变化，整条序列就会错位，世界走向另一个平行宇宙——这会同时
摧毁自由读档与确定性重放。

改为让随机数由 (世界种子, 实体ID, 事件计数) 计算得出而非从共享流取
出。同一三元组在任何线程任何平台都得到相同结果，故并行结算天然安全，
无需任何同步。

三个输入逐级混合而非异或：直接异或会让 (种子=1,实体=2) 与
(种子=2,实体=1) 得到同一条流，不同实体之间会出现可察觉的行为关联。

gen_range 用 Lemire 乘法取余而非取余运算，因为朴素取余存在模偏差，
会让较小的值出现概率略高；这种偏差在百万次经济模拟中会累积成可观的
系统性倾斜。

选 splitmix64 是因为它实现只有几行、无需依赖（ll-core 必须零依赖）、
且全部整数运算，跨平台逐位一致。
EOF
```

---

### Task 6：世界时间与四季

**Files:**
- Modify: `crates/ll-core/src/time.rs`

**Interfaces:**
- Consumes: Task 1 的 crate 骨架
- Produces:
  - 常量 `TICKS_PER_MINUTE`、`TICKS_PER_HOUR`、`TICKS_PER_DAY`、`DAYS_PER_SEASON`、`SEASONS_PER_YEAR`（均为 `i64`）
  - `pub enum Season { Spring, Summer, Autumn, Winter }`
  - `pub struct Tick(pub i64)`
  - `Tick::hour_of_day(&self) -> i64`、`Tick::day_of_year(&self) -> i64`、`Tick::season(&self) -> Season`、`Tick::is_daylight(&self) -> bool`

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-core/src/time.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 一天结束时小时数归零() {
        // Arrange
        let midnight = Tick(TICKS_PER_DAY);

        // Act
        let hour = midnight.hour_of_day();

        // Assert
        assert_eq!(hour, 0);
    }

    #[test]
    fn 一年第一天属于春季() {
        // Arrange
        let start = Tick(0);

        // Act
        let season = start.season();

        // Assert
        assert_eq!(season, Season::Spring);
    }

    #[test]
    fn 跨过三个季节长度后进入冬季() {
        // Arrange
        let winter_start = Tick(TICKS_PER_DAY * DAYS_PER_SEASON * 3);

        // Act
        let season = winter_start.season();

        // Assert
        assert_eq!(season, Season::Winter);
    }

    #[test]
    fn 满一年后季节回到春季() {
        // 世界时钟会长期累加，季节必须正确循环而非越界。
        // Arrange
        let next_year = Tick(TICKS_PER_DAY * DAYS_PER_SEASON * SEASONS_PER_YEAR);

        // Act
        let season = next_year.season();

        // Assert
        assert_eq!(season, Season::Spring);
    }

    #[test]
    fn 午夜不是白昼() {
        // Arrange
        let midnight = Tick(0);

        // Act & Assert
        assert!(!midnight.is_daylight());
    }

    #[test]
    fn 正午是白昼() {
        // Arrange
        let noon = Tick(TICKS_PER_HOUR * 12);

        // Act & Assert
        assert!(noon.is_daylight());
    }

    #[test]
    fn 负时刻不会得到负小时数() {
        // 读档迁移或时间倒流类效果可能产生负值，用取余会得到负小时。
        // Arrange
        let before_start = Tick(-TICKS_PER_HOUR);

        // Act
        let hour = before_start.hour_of_day();

        // Assert
        assert_eq!(hour, 23);
    }
}
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-core time
```
预期：编译失败，`cannot find type Tick in this scope`。

- [ ] **Step 3：实现 `time.rs`**

```rust
//! 世界时间与四季。
//!
//! 全世界只有**一个时钟**，昼夜、季节、时间轴调度、经济推进全部由它
//! 派生。设立多个时钟必然导致它们逐渐失同步，进而出现「城镇已入冬但
//! 野外还是盛夏」这类缺陷。
//!
//! 时间以整数刻度表示，不使用浮点——理由同世界状态的其余部分：浮点会
//! 破坏跨平台确定性。

/// 一分钟对应的刻度数。
///
/// 取 60 使一刻度恰好等于一游戏秒，方便调试时肉眼换算。
pub const TICKS_PER_MINUTE: i64 = 60;

/// 一小时对应的刻度数。
pub const TICKS_PER_HOUR: i64 = TICKS_PER_MINUTE * 60;

/// 一天对应的刻度数。
pub const TICKS_PER_DAY: i64 = TICKS_PER_HOUR * 24;

/// 每个季节的天数。
pub const DAYS_PER_SEASON: i64 = 30;

/// 一年的季节数。
pub const SEASONS_PER_YEAR: i64 = 4;

/// 白昼开始的小时（含）。
const DAYLIGHT_START_HOUR: i64 = 6;

/// 白昼结束的小时（不含）。
const DAYLIGHT_END_HOUR: i64 = 18;

/// 四季。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Season {
    /// 春。
    Spring,
    /// 夏。
    Summer,
    /// 秋。
    Autumn,
    /// 冬。
    Winter,
}

/// 世界时刻，以刻度计，从世界创建那一刻开始计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tick(pub i64);

impl Tick {
    /// 当日的小时数，取值 `0..24`。
    pub const fn hour_of_day(&self) -> i64 {
        // rem_euclid 而非取余：世界时钟理论上不会为负，但读档迁移或
        // 时间倒流类效果可能产生负值，取余会得到负小时数。
        (self.0.rem_euclid(TICKS_PER_DAY)) / TICKS_PER_HOUR
    }

    /// 当年的第几天，取值 `0..(DAYS_PER_SEASON * SEASONS_PER_YEAR)`。
    pub const fn day_of_year(&self) -> i64 {
        let days_per_year = DAYS_PER_SEASON * SEASONS_PER_YEAR;
        (self.0.div_euclid(TICKS_PER_DAY)).rem_euclid(days_per_year)
    }

    /// 当前季节。
    pub const fn season(&self) -> Season {
        match self.day_of_year() / DAYS_PER_SEASON {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Autumn,
            // day_of_year 已对一年取模，故此分支只可能是第四季。
            _ => Season::Winter,
        }
    }

    /// 当前是否为白昼。
    ///
    /// 现阶段昼夜边界固定。后续若要让日照长度随季节变化，应在此处按
    /// [`Self::season`] 调整边界，而非另设时钟。
    pub const fn is_daylight(&self) -> bool {
        let hour = self.hour_of_day();
        hour >= DAYLIGHT_START_HOUR && hour < DAYLIGHT_END_HOUR
    }
}
```

- [ ] **Step 4：运行测试确认通过**

```bash
cargo test -p ll-core time
```
预期：7 个测试全部 PASS。

- [ ] **Step 5：提交**

```bash
git add crates/ll-core/src/time.rs
git commit -F - <<'EOF'
feat: 世界时间与四季

全世界只设一个时钟，昼夜、季节、时间轴调度、经济推进全部由它派生。
设立多个时钟必然逐渐失同步，出现「城镇已入冬但野外还是盛夏」这类
缺陷。

时间用整数刻度而非浮点，理由同世界状态其余部分：浮点破坏跨平台
确定性。一刻度等于一游戏秒，方便调试时肉眼换算。

取小时与天数用 rem_euclid / div_euclid 而非取余与整除：世界时钟理论
上不为负，但读档迁移或时间倒流类效果可能产生负值，用取余会得到负
小时数。

昼夜边界暂时固定。日后若要让日照长度随季节变化，应在 is_daylight 内
按季节调整边界，而不是另设一个时钟。
EOF
```

---

### Task 7：跨平台确定性回归框架

**Files:**
- Modify: `crates/ll-core/src/hashing.rs`
- Create: `crates/ll-core/tests/determinism.rs`

**Interfaces:**
- Consumes: Task 3 的 `TorusSize`、Task 5 的 `DetRng`、Task 6 的 `Tick`
- Produces：`pub struct StateHasher`，方法 `new() -> StateHasher`、`write_u64(&mut self, value: u64)`、`write_i64(&mut self, value: i64)`、`finish(&self) -> u64`

> 这是 P0 最重要的任务。规格 §14.4 把跨平台确定性列为最高优先级：若等到
> P9 才发现世界会分叉，届时已无从追溯是哪一层引入的浮点。框架必须在
> 地基阶段就位。

- [ ] **Step 1：写失败的单元测试**

```rust
// 追加到 crates/ll-core/src/hashing.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空哈希器返回算法规定的初值() {
        // Arrange
        let hasher = StateHasher::new();

        // Act
        let digest = hasher.finish();

        // Assert
        assert_eq!(digest, 0xCBF2_9CE4_8422_2325);
    }

    #[test]
    fn 混入顺序不同则摘要不同() {
        // 顺序敏感是必需的：若 (移动,攻击) 与 (攻击,移动) 摘要相同，
        // 就检测不出事件顺序被打乱这类确定性缺陷。
        // Arrange
        let mut first = StateHasher::new();
        let mut second = StateHasher::new();

        // Act
        first.write_u64(1);
        first.write_u64(2);
        second.write_u64(2);
        second.write_u64(1);

        // Assert
        assert_ne!(first.finish(), second.finish());
    }

    #[test]
    fn 正负数摘要不同() {
        // Arrange
        let mut positive = StateHasher::new();
        let mut negative = StateHasher::new();

        // Act
        positive.write_i64(1);
        negative.write_i64(-1);

        // Assert
        assert_ne!(positive.finish(), negative.finish());
    }
}
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-core hashing
```
预期：编译失败，`cannot find type StateHasher in this scope`。

- [ ] **Step 3：实现 `hashing.rs`**

```rust
//! 世界状态摘要。
//!
//! 用途是把整个世界状态归约成一个 64 位数字，使「两次运行是否产生了
//! 相同的世界」可以被一行断言检验。这是确定性重放与跨平台一致性回归
//! 的基础设施。
//!
//! # 为什么不用标准库的 Hasher
//!
//! `std::collections::hash_map::DefaultHasher` 的算法**不保证跨版本
//! 稳定**，标准库文档明确说明它可能在任何 Rust 版本变更。用它做黄金
//! 基准，会在某次工具链升级后集体失效，而那时无法区分是升级导致的
//! 还是真的引入了缺陷。
//!
//! 因此这里手写 FNV-1a：算法极简、完全由整数运算构成、由规范唯一确定，
//! 因而跨平台跨版本恒定。它不适合做哈希表（抗碰撞性一般），但用于
//! 检测「状态是否改变」完全足够。

/// FNV-1a 64 位的初始值，由算法规范定义。
const FNV_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;

/// FNV-1a 64 位的质数，由算法规范定义。
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// 世界状态的增量哈希器。
#[derive(Debug, Clone)]
pub struct StateHasher {
    digest: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl StateHasher {
    /// 建立初始哈希器。
    pub const fn new() -> Self {
        StateHasher {
            digest: FNV_OFFSET_BASIS,
        }
    }

    /// 混入一个无符号整数。
    ///
    /// 按小端序逐字节混入。**必须显式指定字节序**——依赖本机字节序会让
    /// 大端平台产出不同的哈希，正好破坏本模块存在的意义。
    pub fn write_u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.digest ^= byte as u64;
            self.digest = self.digest.wrapping_mul(FNV_PRIME);
        }
    }

    /// 混入一个有符号整数。
    pub fn write_i64(&mut self, value: i64) {
        // 直接按位重解释而非取绝对值：负数的位模式必须原样参与哈希，
        // 否则 -1 与 1 会得到相同摘要。
        self.write_u64(value as u64);
    }

    /// 取当前摘要。可在中途多次调用，不影响后续混入。
    pub const fn finish(&self) -> u64 {
        self.digest
    }
}
```

- [ ] **Step 4：运行单元测试确认通过**

```bash
cargo test -p ll-core hashing
```
预期：3 个测试全部 PASS。

- [ ] **Step 5：写黄金基准回归测试（期望值先填 0）**

```rust
// crates/ll-core/tests/determinism.rs
//! 跨平台确定性回归。
//!
//! 本文件里的期望值是**黄金基准**：它们由算法定义唯一确定，在 Windows
//! 与 Linux 上必须逐位相同。
//!
//! # 测试失败意味着什么
//!
//! 若某次改动让这里的摘要变了，只有两种可能：
//!
//! 1. 有意修改了算法或常量——那么更新期望值，并在提交信息里说明为什么。
//! 2. **无意引入了平台相关行为**（最常见的是浮点运算，或依赖了哈希表
//!    的遍历顺序）。这是必须立刻修复的缺陷。
//!
//! **绝不允许「测试挂了就把期望值改成实际值」**——那等于删掉这道防线。

use ll_core::hashing::StateHasher;
use ll_core::rng::DetRng;
use ll_core::time::{TICKS_PER_DAY, Tick};
use ll_core::torus::TorusSize;

/// 由首次运行记录的黄金基准。修改前请阅读本文件顶部说明。
const EXPECTED_RNG_DIGEST: u64 = 0;

/// 由首次运行记录的黄金基准。
const EXPECTED_TORUS_DIGEST: u64 = 0;

/// 由首次运行记录的黄金基准。
const EXPECTED_TIME_DIGEST: u64 = 0;

#[test]
fn 随机序列的摘要跨平台稳定() {
    // 这是整个确定性体系的守门测试。
    // Arrange
    let mut rng = DetRng::for_entity(0x1234_5678, 42, 0);
    let mut hasher = StateHasher::new();

    // Act
    for _ in 0..1_000 {
        hasher.write_u64(rng.next_u64());
    }

    // Assert
    assert_eq!(hasher.finish(), EXPECTED_RNG_DIGEST);
}

#[test]
fn 环面距离序列的摘要跨平台稳定() {
    // Arrange
    let world = TorusSize::new(4096, 4096).expect("宽高均不为零");
    let mut hasher = StateHasher::new();

    // Act
    for i in 0..500_i32 {
        let a = world.wrap(i * 7, i * 13);
        let b = world.wrap(4096 - i * 3, i * 29);
        hasher.write_u64(world.chebyshev(a, b) as u64);
        hasher.write_u64(world.squared_euclidean(a, b));
    }

    // Assert
    assert_eq!(hasher.finish(), EXPECTED_TORUS_DIGEST);
}

#[test]
fn 季节推进的摘要跨平台稳定() {
    // Arrange
    let mut hasher = StateHasher::new();

    // Act
    for day in 0..365_i64 {
        let tick = Tick(day * TICKS_PER_DAY);
        hasher.write_i64(tick.day_of_year());
        hasher.write_i64(tick.season() as i64);
        hasher.write_i64(tick.is_daylight() as i64);
    }

    // Assert
    assert_eq!(hasher.finish(), EXPECTED_TIME_DIGEST);
}
```

- [ ] **Step 6：运行以取得实测摘要**

```bash
cargo test -p ll-core --test determinism 2>&1 | grep -E "left|right"
```
预期：三个测试失败，输出中的 `left` 即实测摘要。

- [ ] **Step 7：填入黄金基准并复跑**

把三个 `EXPECTED_*_DIGEST` 常量替换为上一步得到的实测值，然后：

```bash
cargo test -p ll-core --test determinism
```
预期：3 个测试全部 PASS。

**注意**：只在此处首次建立基准时这样做。此后基准冻结，测试失败必须按文件顶部说明排查根因。

- [ ] **Step 8：提交**

```bash
git add crates/ll-core/src/hashing.rs crates/ll-core/tests/determinism.rs
git commit -F - <<'EOF'
feat: 跨平台确定性回归框架

把世界状态归约成一个 64 位数字，使「两次运行是否产生相同的世界」可以
被一行断言检验。这是自由读档与确定性重放的基础设施，必须在地基阶段
就位——若等到 P9 才发现世界会分叉，届时已无从追溯是哪一层引入的浮点。

不用标准库 DefaultHasher：其算法不保证跨版本稳定，标准库文档明确说明
可能在任何版本变更。用它做黄金基准会在某次工具链升级后集体失效，而
那时无法区分是升级导致的还是真引入了缺陷。改用手写 FNV-1a，由规范
唯一确定，跨平台跨版本恒定。

显式指定小端序而非依赖本机字节序——后者会让大端平台产出不同摘要，
正好破坏本模块存在的意义。

摘要对混入顺序敏感是刻意的：若 (移动,攻击) 与 (攻击,移动) 摘要相同，
就检测不出事件顺序被打乱这类缺陷。

determinism.rs 顶部写明了「测试挂了不许直接改期望值」，因为那等于
删掉这道防线。
EOF
```

---

### Task 8：平台层骨架与日志

**Files:**
- Create: `crates/ll-platform/Cargo.toml`
- Create: `crates/ll-platform/src/lib.rs`
- Create: `crates/ll-platform/src/logging.rs`
- Create: `crates/ll-platform/src/input.rs`、`src/jobs.rs`、`src/window.rs`（占位）

**Interfaces:**
- Consumes: `ll-core`
- Produces：`pub enum PlatformError { LoggingAlreadyInitialized, WindowCreation(String) }`；`pub fn init_logging(verbose: bool) -> Result<(), PlatformError>`

- [ ] **Step 1：创建 `crates/ll-platform/Cargo.toml`**

```toml
[package]
name = "ll-platform"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "迷途大陆的平台层：窗口、输入、日志、并行任务池"

[dependencies]
ll-core = { path = "../ll-core" }
# winit 锁在 0.30 稳定线。0.31 目前只有 beta，地基层不能压在 beta 上。
winit = "0.30"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
rayon = "1.12"
crossbeam-channel = "0.5"

[dev-dependencies]
proptest.workspace = true
```

- [ ] **Step 2：创建 `crates/ll-platform/src/lib.rs`**

```rust
//! 平台层：窗口、输入、日志与并行任务池。
//!
//! 本层封装一切与操作系统打交道的部分，使上层完全不感知平台差异。
//! 它**不含任何游戏逻辑**——判断某个类型该放这里还是放上层，标准是
//! 「换一个操作系统时它是否需要改」。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod input;
pub mod jobs;
pub mod logging;
pub mod window;

use core::fmt;

/// 平台层的错误。
#[derive(Debug)]
pub enum PlatformError {
    /// 日志系统重复初始化。
    LoggingAlreadyInitialized,
    /// 窗口创建或事件循环失败，附带底层原因。
    WindowCreation(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlatformError::LoggingAlreadyInitialized => {
                write!(f, "logging subsystem was already initialized")
            }
            PlatformError::WindowCreation(reason) => {
                write!(f, "failed to create window: {reason}")
            }
        }
    }
}

impl core::error::Error for PlatformError {}
```

- [ ] **Step 3：创建三个占位模块**

```rust
// crates/ll-platform/src/input.rs
//! 输入状态聚合。由 Task 9 填充。
```

`jobs.rs` → `//! 并行任务池。由 Task 10 填充。`，`window.rs` → `//! 窗口与事件循环。由 Task 11 填充。`

- [ ] **Step 4：写失败的测试**

```rust
// 追加到 crates/ll-platform/src/logging.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 重复初始化返回错误而非崩溃() {
        // 日志初始化失败绝不能让游戏起不来。热重载与测试场景下重复调用
        // 是常态，必须优雅拒绝。
        // Arrange：首次调用可能成功也可能已被同进程内其他测试占用，
        // 两种情况都不影响本测试要验证的行为。
        let _ = init_logging(false);

        // Act
        let second = init_logging(false);

        // Assert
        assert!(matches!(
            second,
            Err(crate::PlatformError::LoggingAlreadyInitialized)
        ));
    }
}
```

- [ ] **Step 5：运行测试确认失败**

```bash
cargo test -p ll-platform logging
```
预期：编译失败，`cannot find function init_logging`。

- [ ] **Step 6：实现 `logging.rs`**

```rust
//! 日志初始化。
//!
//! 选用 `tracing` 而非 `log`，因为本项目是多线程的：`tracing` 的 span
//! 能标明一条日志属于哪个任务、在哪条线程，而 `log` 只能给出扁平的一行
//! 文本。等到需要排查「离屏世界推进为何偶发出错」时，这个差别决定了
//! 能不能查出来。

use crate::PlatformError;
use tracing_subscriber::EnvFilter;

/// 初始化全局日志。
///
/// `verbose` 为真时默认级别提升到 `debug`。无论何种情况，环境变量
/// `LOSTLAND_LOG` 都拥有更高优先级，便于临时排查而无需重新编译。
///
/// 重复调用返回 [`PlatformError::LoggingAlreadyInitialized`] 而非 panic：
/// 热重载与测试场景下重复调用是常态，日志初始化失败绝不该让游戏起不来。
pub fn init_logging(verbose: bool) -> Result<(), PlatformError> {
    let default_level = if verbose { "debug" } else { "info" };

    let filter =
        EnvFilter::try_from_env("LOSTLAND_LOG").unwrap_or_else(|_| EnvFilter::new(default_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        // 显示线程名，因为在固定职责线程模型下，「这条日志来自哪条线程」
        // 往往是定位问题的第一线索。
        .with_thread_names(true)
        .with_target(true)
        .try_init()
        .map_err(|_| PlatformError::LoggingAlreadyInitialized)
}
```

- [ ] **Step 7：运行测试确认通过**

```bash
cargo test -p ll-platform logging
```
预期：1 个测试 PASS。

- [ ] **Step 8：提交**

```bash
git add crates/ll-platform
git commit -F - <<'EOF'
feat: 平台层骨架与日志初始化

选 tracing 而非 log，因为本项目是多线程的。tracing 的 span 能标明一条
日志属于哪个任务、在哪条线程；log 只能给出扁平文本。等到要排查
「离屏世界推进为何偶发出错」时，这个差别决定能不能查出来。

重复初始化返回 Err 而非 panic：热重载与测试场景下重复调用是常态，
日志初始化失败绝不该让游戏起不来。

winit 锁在 0.30 稳定线。0.31 当前只有 beta，地基层不能压在 beta 上。
EOF
```

---

### Task 9：输入状态聚合

**Files:**
- Modify: `crates/ll-platform/src/input.rs`
- Create: `crates/ll-platform/tests/input_blackbox.rs`

**Interfaces:**
- Consumes: Task 8 的 crate 骨架
- Produces:
  - `pub enum GameKey { Up, Down, Left, Right, Confirm, Cancel, Menu, Map, Wait }`
  - `pub struct InputState`，方法 `new() -> InputState`、`press(&mut self, key: GameKey)`、`release(&mut self, key: GameKey)`、`is_held(&self, key: GameKey) -> bool`、`was_just_pressed(&self, key: GameKey) -> bool`、`end_frame(&mut self)`

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-platform/src/input.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 按下后处于按住状态() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.press(GameKey::Confirm);

        // Assert
        assert!(input.is_held(GameKey::Confirm));
    }

    #[test]
    fn 刚按下的判定在帧结束后失效() {
        // 回合制里「刚按下」与「按住」必须区分：按住方向键应连续移动，
        // 但按住确认键不能反复触发同一个菜单项。
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        input.end_frame();

        // Assert
        assert!(!input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 帧结束后按住状态依然保留() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Right);

        // Act
        input.end_frame();

        // Assert
        assert!(input.is_held(GameKey::Right));
    }

    #[test]
    fn 松开后不再处于按住状态() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Left);

        // Act
        input.release(GameKey::Left);

        // Assert
        assert!(!input.is_held(GameKey::Left));
    }

    #[test]
    fn 重复按下不会重新触发刚按下判定() {
        // 操作系统的按键重复事件会连续发送按下，若不去重，长按确认键会
        // 把整个菜单一路点穿。
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);
        input.end_frame();

        // Act
        input.press(GameKey::Confirm);

        // Assert
        assert!(!input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 不同按键的状态互不干扰() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.press(GameKey::Up);

        // Assert
        assert!(!input.is_held(GameKey::Down));
    }
}
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-platform input
```
预期：编译失败，`cannot find type InputState in this scope`。

- [ ] **Step 3：实现 `input.rs`**

```rust
//! 输入状态聚合。
//!
//! 本模块把操作系统的按键事件归约为**游戏语义的动作**，而不是把物理
//! 按键直接暴露给上层。这样按键重绑定与手柄支持只需改本模块的映射表，
//! 上层逻辑一行都不用动。
//!
//! # 为什么要区分「按住」与「刚按下」
//!
//! 回合制里这两者语义完全不同：按住方向键应当连续移动，但按住确认键
//! 绝不能反复触发同一个菜单项。操作系统还会发送按键重复事件，若不去重，
//! 长按确认键会把整个菜单一路点穿。

/// 游戏语义的动作键。
///
/// 上层只认这些，不认物理按键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameKey {
    /// 向上移动或菜单上移。
    Up,
    /// 向下移动或菜单下移。
    Down,
    /// 向左移动或菜单左移。
    Left,
    /// 向右移动或菜单右移。
    Right,
    /// 确认。
    Confirm,
    /// 取消或返回。
    Cancel,
    /// 打开主菜单。
    Menu,
    /// 打开世界地图。
    Map,
    /// 原地等待一回合。
    Wait,
}

/// 动作键总数，用于状态数组定长。
const KEY_COUNT: usize = 9;

impl GameKey {
    /// 在状态数组中的下标。
    const fn index(self) -> usize {
        self as usize
    }
}

/// 一帧内的输入状态。
///
/// 用定长数组而非哈希集合：动作键数量固定且很少，数组查询是一次下标
/// 访问，而哈希查询涉及哈希计算与可能的冲突处理。输入查询在每帧的 UI
/// 与逻辑中被调用数十次，这个差别值得。
#[derive(Debug, Clone)]
pub struct InputState {
    held: [bool; KEY_COUNT],
    just_pressed: [bool; KEY_COUNT],
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    /// 建立全部松开的初始状态。
    pub const fn new() -> Self {
        InputState {
            held: [false; KEY_COUNT],
            just_pressed: [false; KEY_COUNT],
        }
    }

    /// 记录一次按下。
    ///
    /// 若该键本已按住，则不重新置起「刚按下」标志——这就是对操作系统
    /// 按键重复事件的去重。
    pub fn press(&mut self, key: GameKey) {
        let index = key.index();
        if !self.held[index] {
            self.just_pressed[index] = true;
        }
        self.held[index] = true;
    }

    /// 记录一次松开。
    pub fn release(&mut self, key: GameKey) {
        self.held[key.index()] = false;
    }

    /// 该键当前是否被按住。
    pub fn is_held(&self, key: GameKey) -> bool {
        self.held[key.index()]
    }

    /// 该键是否在本帧刚刚被按下。
    pub fn was_just_pressed(&self, key: GameKey) -> bool {
        self.just_pressed[key.index()]
    }

    /// 结束当前帧，清空「刚按下」标志。
    ///
    /// 必须在每帧逻辑处理**之后**调用。放在处理之前会让所有「刚按下」
    /// 判定永远为假。
    pub fn end_frame(&mut self) {
        self.just_pressed = [false; KEY_COUNT];
    }
}
```

- [ ] **Step 4：运行测试确认通过**

```bash
cargo test -p ll-platform input
```
预期：6 个测试全部 PASS。

- [ ] **Step 5：写黑箱状态机属性测试**

```rust
// crates/ll-platform/tests/input_blackbox.rs
//! 输入状态机的黑箱属性测试。
//!
//! 输入状态是个小型状态机，最容易出的错是「某个操作序列之后状态自相
//! 矛盾」。属性测试用随机操作序列轰炸它，比手写用例更容易撞出问题。

use ll_platform::input::{GameKey, InputState};
use proptest::prelude::*;

/// 对状态机施加的一次操作。
#[derive(Debug, Clone, Copy)]
enum Op {
    Press,
    Release,
    EndFrame,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![Just(Op::Press), Just(Op::Release), Just(Op::EndFrame)]
}

/// 对状态机施加一次操作。
fn apply(input: &mut InputState, key: GameKey, op: Op) {
    match op {
        Op::Press => input.press(key),
        Op::Release => input.release(key),
        Op::EndFrame => input.end_frame(),
    }
}

proptest! {
    #[test]
    fn 刚按下为真时必然处于按住状态(ops in prop::collection::vec(op_strategy(), 0..64)) {
        // 「刚按下但没按住」是自相矛盾的状态，任何操作序列都不该产生它。
        // Arrange
        let mut input = InputState::new();
        let key = GameKey::Confirm;

        // Act & Assert
        for op in ops {
            apply(&mut input, key, op);
            if input.was_just_pressed(key) {
                prop_assert!(input.is_held(key));
            }
        }
    }

    #[test]
    fn 结束帧后刚按下恒为假(ops in prop::collection::vec(op_strategy(), 0..64)) {
        // Arrange
        let mut input = InputState::new();
        let key = GameKey::Up;
        for op in ops {
            apply(&mut input, key, op);
        }

        // Act
        input.end_frame();

        // Assert
        prop_assert!(!input.was_just_pressed(key));
    }
}
```

- [ ] **Step 6：运行黑箱测试**

```bash
cargo test -p ll-platform --test input_blackbox
```
预期：2 个属性全部 PASS。

- [ ] **Step 7：提交**

```bash
git add crates/ll-platform/src/input.rs crates/ll-platform/tests/input_blackbox.rs
git commit -F - <<'EOF'
feat: 输入状态聚合与按键语义映射

把操作系统按键事件归约为游戏语义动作，而非把物理按键直接暴露给上层。
这样按键重绑定与手柄支持只需改本模块的映射表，上层逻辑一行不动。

区分「按住」与「刚按下」：回合制里两者语义完全不同——按住方向键应
连续移动，按住确认键绝不能反复触发同一菜单项。press 对已按住的键不
重新置标志，这是对操作系统按键重复事件的去重；否则长按确认键会把
整个菜单一路点穿。

用定长数组而非哈希集合：动作键数量固定且很少，数组查询是一次下标
访问；输入查询每帧被调用数十次，这个差别值得。

属性测试用随机操作序列轰炸状态机，断言「刚按下为真时必然按住」这条
自洽性不变量——这类自相矛盾状态是手写用例最容易漏掉的。
EOF
```

---

### Task 10：并行任务池

**Files:**
- Modify: `crates/ll-platform/src/jobs.rs`

**Interfaces:**
- Consumes: Task 8 的 crate 骨架
- Produces:
  - `pub struct JobPool`，方法 `new(threads: usize) -> JobPool`、`thread_count(&self) -> usize`、`map_collect<T, R, F>(&self, items: &[T], f: F) -> Vec<R>`
  - 再导出 `pub use crossbeam_channel::{Receiver, Sender, unbounded as channel}`

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-platform/src/jobs.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 并行映射保持输入顺序() {
        // 顺序保持是确定性的前提。若结果顺序随线程调度变化，后续对结果
        // 做的任何折叠运算都会失去确定性。
        // Arrange
        let pool = JobPool::new(4);
        let input: Vec<u64> = (0..1_000).collect();

        // Act
        let output = pool.map_collect(&input, |n| n * 2);

        // Assert
        let expected: Vec<u64> = (0..1_000).map(|n| n * 2).collect();
        assert_eq!(output, expected);
    }

    #[test]
    fn 空输入返回空结果() {
        // Arrange
        let pool = JobPool::new(2);
        let input: Vec<u64> = Vec::new();

        // Act
        let output = pool.map_collect(&input, |n| n * 2);

        // Assert
        assert!(output.is_empty());
    }

    #[test]
    fn 线程数为零时退化为单线程而非崩溃() {
        // 配置文件可能写出 0，与其崩溃不如退化。
        // Arrange
        let pool = JobPool::new(0);
        let input = vec![1_u64, 2, 3];

        // Act
        let output = pool.map_collect(&input, |n| n + 1);

        // Assert
        assert_eq!(output, vec![2, 3, 4]);
    }

    #[test]
    fn 线程数为零时池内至少有一条线程() {
        // Arrange
        let pool = JobPool::new(0);

        // Act
        let count = pool.thread_count();

        // Assert
        assert_eq!(count, 1);
    }

    #[test]
    fn 通道可在线程间传递消息() {
        // Arrange
        let (sender, receiver) = channel::<u32>();

        // Act
        sender.send(7).expect("接收端仍存活");

        // Assert
        assert_eq!(receiver.recv().expect("已有消息在途"), 7);
    }
}
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-platform jobs
```
预期：编译失败，`cannot find type JobPool in this scope`。

- [ ] **Step 3：实现 `jobs.rs`**

```rust
//! 并行任务池与线程间通道。
//!
//! # 线程模型
//!
//! 项目采用**固定职责线程 + 任务池**，不引入异步运行时：
//!
//! - 主线程：窗口事件循环、输入采集、世界写入、渲染提交
//! - 任务池：只读的重计算（视野、寻路、地图生成、离屏世界推进）
//! - IO 线程：资产加载、存档写入、脚本热重载监听
//!
//! 不用异步运行时的原因是这里没有海量并发连接需要等待，只有 CPU 密集的
//! 批量计算；异步只会让函数签名被传染，却换不来任何好处。
//!
//! # 顺序保持是硬要求
//!
//! [`JobPool::map_collect`] 保证输出顺序与输入一致。这不是便利特性而是
//! 确定性的前提：若结果顺序随线程调度变化，后续对结果做的任何折叠运算
//! 都会失去确定性，跨平台世界摘要就对不上了。

use rayon::prelude::*;

pub use crossbeam_channel::{Receiver, Sender, unbounded as channel};

/// 承担只读重计算的并行任务池。
#[derive(Debug)]
pub struct JobPool {
    inner: rayon::ThreadPool,
}

impl JobPool {
    /// 建立任务池。
    ///
    /// `threads` 为零时退化为单线程——配置文件可能写出 0，与其崩溃不如
    /// 退化运行。
    pub fn new(threads: usize) -> Self {
        let threads = threads.max(1);
        let inner = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            // 线程命名后，日志与性能分析器里就能一眼看出是任务池线程。
            .thread_name(|index| format!("ll-job-{index}"))
            .build()
            .expect("线程数已在上方钳制为至少 1，构建不应失败");
        JobPool { inner }
    }

    /// 池中的线程数。
    pub fn thread_count(&self) -> usize {
        self.inner.current_num_threads()
    }

    /// 并行映射，**输出顺序与输入一致**。
    ///
    /// 闭包必须是纯函数：它会在多个线程上同时执行，任何共享可变状态都会
    /// 破坏确定性。这正是「意图—结算—效果」架构中结算阶段只读世界的
    /// 原因。
    pub fn map_collect<T, R, F>(&self, items: &[T], f: F) -> Vec<R>
    where
        T: Sync,
        R: Send,
        F: Fn(&T) -> R + Sync + Send,
    {
        self.inner.install(|| items.par_iter().map(f).collect())
    }
}
```

- [ ] **Step 4：运行测试确认通过**

```bash
cargo test -p ll-platform jobs
```
预期：5 个测试全部 PASS。

- [ ] **Step 5：提交**

```bash
git add crates/ll-platform/src/jobs.rs
git commit -F - <<'EOF'
feat: 并行任务池与线程间通道

采用固定职责线程 + 任务池，不引入异步运行时。这里没有海量并发连接
需要等待，只有 CPU 密集的批量计算；异步只会让函数签名被传染，换不来
任何好处。

map_collect 保证输出顺序与输入一致。这不是便利特性而是确定性的前提：
若结果顺序随线程调度变化，后续对结果的任何折叠运算都会失去确定性，
跨平台世界摘要就对不上了。

线程数为零时退化为单线程而非崩溃——配置文件可能写出 0。

线程命名后，日志与性能分析器里能一眼认出任务池线程。
EOF
```

---

### Task 11：窗口与事件循环

**Files:**
- Modify: `crates/ll-platform/src/window.rs`

**Interfaces:**
- Consumes: Task 8 的 `PlatformError`、Task 9 的 `InputState` 与 `GameKey`
- Produces:
  - `pub struct WindowConfig { pub logical_width: u32, pub logical_height: u32, pub scale: u32, pub title_key: &'static str }`，实现 `Default`
  - `pub trait AppHandler { fn on_frame(&mut self, input: &InputState); fn on_exit(&mut self); }`
  - `pub fn map_physical_key(key: winit::keyboard::KeyCode) -> Option<GameKey>`
  - `pub fn run<H: AppHandler + 'static>(config: WindowConfig, handler: H) -> Result<(), PlatformError>`

- [ ] **Step 1：确认 winit 0.30 的 API 形状**

winit 0.30 用 `ApplicationHandler` trait 驱动事件循环，而非旧版的闭包式 `run`。动手前先查文档确认 `resumed` 与 `window_event` 的签名：

```bash
cargo doc -p winit --no-deps --open
```

**若解析到的版本或 API 与本计划不符，停下来报告，不要猜测 API。**

- [ ] **Step 2：写失败的按键映射测试**

```rust
// 追加到 crates/ll-platform/src/window.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::KeyCode;

    #[test]
    fn 方向键映射到对应动作() {
        // Arrange
        let physical = KeyCode::ArrowUp;

        // Act
        let action = map_physical_key(physical);

        // Assert
        assert_eq!(action, Some(GameKey::Up));
    }

    #[test]
    fn 字母键位也映射到方向以适应手部姿势偏好() {
        // 传统 Roguelike 玩家习惯 WASD，强制只用方向键会劝退一部分人。
        // Arrange
        let physical = KeyCode::KeyW;

        // Act
        let action = map_physical_key(physical);

        // Assert
        assert_eq!(action, Some(GameKey::Up));
    }

    #[test]
    fn 未绑定的键返回空值() {
        // Arrange
        let physical = KeyCode::F13;

        // Act
        let action = map_physical_key(physical);

        // Assert
        assert_eq!(action, None);
    }

    #[test]
    fn 默认配置使用规格规定的逻辑分辨率() {
        // 规格 §2 决策 6：逻辑分辨率固定 640×360。
        // Arrange & Act
        let config = WindowConfig::default();

        // Assert
        assert_eq!(
            (config.logical_width, config.logical_height),
            (640, 360)
        );
    }
}
```

- [ ] **Step 3：运行测试确认失败**

```bash
cargo test -p ll-platform window
```
预期：编译失败，`cannot find function map_physical_key`。

- [ ] **Step 4：实现 `window.rs`**

```rust
//! 窗口与事件循环。
//!
//! # 为什么渲染不单开线程
//!
//! winit 与 wgpu 在部分平台要求窗口创建与渲染提交处于同一线程，强行
//! 分离会在某些合成器上直接失败。真正的并行度来自 [`crate::jobs::JobPool`]
//! 承担的重计算，而不是把渲染搬走。
//!
//! # 整数缩放
//!
//! 逻辑分辨率固定 640×360，窗口尺寸恒为其整数倍。非整数倍缩放会让像素
//! 边缘出现宽窄不一的锯齿，是像素美术最刺眼的瑕疵。

use crate::PlatformError;
use crate::input::{GameKey, InputState};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// 窗口配置。
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// 逻辑宽度，规格规定为 640。
    pub logical_width: u32,
    /// 逻辑高度，规格规定为 360。
    pub logical_height: u32,
    /// 整数缩放倍率。
    pub scale: u32,
    /// 窗口标题的本地化键。
    ///
    /// 存键而非字面量，因为标题是用户可见字符串，必须走 i18n。
    pub title_key: &'static str,
}

impl Default for WindowConfig {
    fn default() -> Self {
        WindowConfig {
            logical_width: 640,
            logical_height: 360,
            // 默认 2 倍得到 1280×720，在绝大多数显示器上都能完整显示。
            scale: 2,
            title_key: "window.title",
        }
    }
}

/// 上层需要实现的帧回调。
///
/// 平台层只负责把事件归约成输入状态并按帧驱动，不含任何游戏逻辑。
pub trait AppHandler {
    /// 每帧调用一次，`input` 是本帧归约后的输入状态。
    fn on_frame(&mut self, input: &InputState);

    /// 窗口关闭前调用，用于保存与清理。
    fn on_exit(&mut self);
}

/// 把物理按键映射为游戏动作。
///
/// 同时支持方向键与 WASD：传统 Roguelike 玩家的手部姿势偏好差异很大，
/// 强制只用方向键会劝退相当一部分人。完整的按键重绑定在 P7 交付，此处
/// 先给出可用的默认布局。
pub fn map_physical_key(key: KeyCode) -> Option<GameKey> {
    let action = match key {
        KeyCode::ArrowUp | KeyCode::KeyW => GameKey::Up,
        KeyCode::ArrowDown | KeyCode::KeyS => GameKey::Down,
        KeyCode::ArrowLeft | KeyCode::KeyA => GameKey::Left,
        KeyCode::ArrowRight | KeyCode::KeyD => GameKey::Right,
        KeyCode::Enter | KeyCode::Space => GameKey::Confirm,
        KeyCode::Escape => GameKey::Cancel,
        KeyCode::Tab => GameKey::Menu,
        KeyCode::KeyM => GameKey::Map,
        KeyCode::Period => GameKey::Wait,
        _ => return None,
    };
    Some(action)
}

/// 事件循环的内部状态。
struct App<H: AppHandler> {
    config: WindowConfig,
    window: Option<Window>,
    input: InputState,
    handler: H,
}

impl<H: AppHandler> ApplicationHandler for App<H> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // resumed 在部分平台会被多次触发（例如从后台恢复），故必须幂等
        // ——重复建窗会泄漏资源。
        if self.window.is_some() {
            return;
        }

        let width = self.config.logical_width * self.config.scale;
        let height = self.config.logical_height * self.config.scale;

        let attributes = Window::default_attributes()
            // 标题此处暂用键名占位，i18n 接入后由上层设置真实标题。
            .with_title(self.config.title_key)
            .with_inner_size(winit::dpi::PhysicalSize::new(width, height))
            .with_resizable(false);

        match event_loop.create_window(attributes) {
            Ok(window) => {
                tracing::info!(width, height, "window created");
                window.request_redraw();
                self.window = Some(window);
            }
            Err(error) => {
                tracing::error!(%error, "failed to create window");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.handler.on_exit();
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(action) = map_physical_key(code) else {
                    return;
                };
                match event.state {
                    ElementState::Pressed => self.input.press(action),
                    ElementState::Released => self.input.release(action),
                }
            }
            WindowEvent::RedrawRequested => {
                self.handler.on_frame(&self.input);
                // 必须在逻辑处理之后清「刚按下」标志，放在之前会让所有
                // 「刚按下」判定永远为假。
                self.input.end_frame();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// 建窗并驱动事件循环，直到窗口关闭。
pub fn run<H: AppHandler + 'static>(
    config: WindowConfig,
    handler: H,
) -> Result<(), PlatformError> {
    let event_loop =
        EventLoop::new().map_err(|e| PlatformError::WindowCreation(e.to_string()))?;

    // Poll 而非 Wait：回合制虽然不需要持续重绘，但离屏世界推进要利用玩家
    // 思考的空窗期，因此主循环必须持续转动而不是阻塞等事件。
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        config,
        window: None,
        input: InputState::new(),
        handler,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| PlatformError::WindowCreation(e.to_string()))
}
```

- [ ] **Step 5：运行测试确认通过**

```bash
cargo test -p ll-platform window
```
预期：4 个测试全部 PASS。

- [ ] **Step 6：提交**

```bash
git add crates/ll-platform/src/window.rs
git commit -F - <<'EOF'
feat: 窗口与事件循环

渲染不单开线程：winit 与 wgpu 在部分平台要求窗口创建与渲染提交同线程，
强行分离会在某些合成器上直接失败。真正的并行度来自任务池承担的重
计算，而不是把渲染搬走。

窗口尺寸恒为逻辑分辨率的整数倍。非整数倍缩放会让像素边缘出现宽窄
不一的锯齿，是像素美术最刺眼的瑕疵。

resumed 做成幂等：部分平台从后台恢复时会重复触发，重复建窗会泄漏
资源。

控制流用 Poll 而非 Wait。回合制本身不需要持续重绘，但离屏世界推进要
利用玩家思考的空窗期，主循环必须持续转动而非阻塞等事件。

按键同时支持方向键与 WASD：传统 Roguelike 玩家手部姿势偏好差异很大，
强制只用方向键会劝退一部分人。完整重绑定在 P7 交付。

窗口标题存本地化键而非字面量，因为它是用户可见字符串，必须走 i18n。
EOF
```

---

### Task 12：P0 验收 Demo

**Files:**
- Create: `crates/ll-platform/examples/p0_acceptance.rs`

**Interfaces:**
- Consumes: 前十一个任务的全部公开 API
- Produces：可执行的验收 demo，证明 P0 地基端到端可用

> 规格 §15 硬性要求：每阶段必须交付可独立运行的 `examples/` 验收 demo。
> 这是「逐层铺地基」策略的配套保险——没有它，地基层会长期无法验证，
> 架构缺陷会一直潜伏到最上层才爆。

- [ ] **Step 1：编写验收 demo**

```rust
//! P0 验收 Demo。
//!
//! 证明平台地基端到端可用：开窗、收输入、日志、并行任务池、环面坐标、
//! 确定性随机、世界时间、状态摘要全部串起来。
//!
//! 本 demo **不渲染任何画面**——渲染是 P1 的职责。窗口是黑的，一切反馈
//! 通过日志输出。这是刻意的：地基层的验收不该依赖尚不存在的上层。
//!
//! 运行：`cargo run -p ll-platform --example p0_acceptance`
//! 操作：方向键或 WASD 移动光标，M 打印世界快照，Esc 退出。

use ll_core::hashing::StateHasher;
use ll_core::rng::DetRng;
use ll_core::time::{TICKS_PER_HOUR, Tick};
use ll_core::torus::{TorusPos, TorusSize};
use ll_platform::input::{GameKey, InputState};
use ll_platform::jobs::JobPool;
use ll_platform::logging::init_logging;
use ll_platform::window::{AppHandler, WindowConfig, run};

/// 演示用的极小世界，尺寸取小以便肉眼观察绕回行为。
const WORLD_WIDTH: u32 = 32;

/// 演示用世界的高度。
const WORLD_HEIGHT: u32 = 32;

/// 每次移动推进的世界时间。
const TICKS_PER_MOVE: i64 = TICKS_PER_HOUR;

/// 演示用的固定世界种子。
const DEMO_SEED: u64 = 0x105_71A_4D;

/// 演示用的主角实体号。
const PLAYER_ENTITY: u64 = 1;

struct Demo {
    world: TorusSize,
    cursor: TorusPos,
    clock: Tick,
    pool: JobPool,
    move_count: u64,
}

impl Demo {
    fn new() -> Self {
        let world =
            TorusSize::new(WORLD_WIDTH, WORLD_HEIGHT).expect("演示世界尺寸为常量且非零");
        Demo {
            world,
            cursor: world.wrap(0, 0),
            clock: Tick(0),
            pool: JobPool::new(4),
            move_count: 0,
        }
    }

    /// 按位移推进光标与世界时钟。
    fn step(&mut self, dx: i32, dy: i32) {
        self.cursor = self.world.wrap(self.cursor.x() + dx, self.cursor.y() + dy);
        self.clock = Tick(self.clock.0 + TICKS_PER_MOVE);
        self.move_count += 1;

        // 每次移动都为该次事件派生一条独立随机流，演示「随机数由三元组
        // 算出而非从共享流取出」。
        let mut rng = DetRng::for_entity(DEMO_SEED, PLAYER_ENTITY, self.move_count);
        let flavour = rng.gen_range(100);

        tracing::info!(
            x = self.cursor.x(),
            y = self.cursor.y(),
            hour = self.clock.hour_of_day(),
            season = ?self.clock.season(),
            daylight = self.clock.is_daylight(),
            flavour,
            "cursor moved"
        );
    }

    /// 用任务池并行计算全世界每格到光标的距离，并摘要结果。
    ///
    /// 这同时验证三件事：任务池顺序保持、环面距离正确、状态摘要可用。
    fn snapshot(&self) {
        let cells: Vec<TorusPos> = (0..WORLD_HEIGHT as i32)
            .flat_map(|y| (0..WORLD_WIDTH as i32).map(move |x| (x, y)))
            .map(|(x, y)| self.world.wrap(x, y))
            .collect();

        let cursor = self.cursor;
        let world = self.world;
        let distances = self
            .pool
            .map_collect(&cells, |cell| world.chebyshev(cursor, *cell));

        let mut hasher = StateHasher::new();
        hasher.write_i64(cursor.x() as i64);
        hasher.write_i64(cursor.y() as i64);
        hasher.write_i64(self.clock.0);
        for distance in &distances {
            hasher.write_u64(*distance as u64);
        }

        tracing::info!(
            cells = distances.len(),
            threads = self.pool.thread_count(),
            digest = format_args!("{:#018x}", hasher.finish()),
            "world snapshot"
        );
    }
}

impl AppHandler for Demo {
    fn on_frame(&mut self, input: &InputState) {
        // 只响应「刚按下」，否则按住方向键会让光标瞬间飞出去——这正是
        // 输入层区分两种状态的实际价值。
        if input.was_just_pressed(GameKey::Up) {
            self.step(0, -1);
        }
        if input.was_just_pressed(GameKey::Down) {
            self.step(0, 1);
        }
        if input.was_just_pressed(GameKey::Left) {
            self.step(-1, 0);
        }
        if input.was_just_pressed(GameKey::Right) {
            self.step(1, 0);
        }
        if input.was_just_pressed(GameKey::Map) {
            self.snapshot();
        }
    }

    fn on_exit(&mut self) {
        tracing::info!(moves = self.move_count, "demo exiting");
    }
}

fn main() {
    init_logging(true).expect("首次初始化日志不应失败");
    tracing::info!("P0 acceptance demo: arrows/WASD to move, M for snapshot, Esc to quit");

    let demo = Demo::new();
    if let Err(error) = run(WindowConfig::default(), demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}
```

- [ ] **Step 2：编译并运行 demo**

```bash
cargo run -p ll-platform --example p0_acceptance
```

预期：
- 出现 1280×720 的黑色窗口（无渲染，符合预期）
- 按方向键时终端持续输出坐标、小时、季节、昼夜与随机值
- 在 `x=31` 处再按右键，坐标绕回 `x=0`——这是环面拓扑的肉眼验证
- 按 M 输出 1024 格的距离摘要与线程数
- 按 Esc 或关窗时打印移动总数并正常退出

- [ ] **Step 3：全量质量门禁**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --all-targets
```
预期：全部通过。

- [ ] **Step 4：提交**

```bash
git add crates/ll-platform/examples/p0_acceptance.rs
git commit -F - <<'EOF'
feat: P0 验收 demo

规格 §15 要求每阶段交付可独立运行的验收 demo。这是「逐层铺地基」策略
的配套保险：没有它，地基层会在很长时间里无法验证，架构缺陷会一直潜伏
到最上层才爆。

demo 刻意不渲染任何画面——渲染是 P1 的职责，地基层的验收不该依赖尚不
存在的上层。窗口是黑的，一切反馈走日志。

它把八个模块串成一条链路：开窗收输入、并行算全图距离、环面绕回、按
事件派生随机数、推进世界时钟与季节、摘要整个快照。任何一环坏了 demo
都会立刻暴露。

只响应「刚按下」而非「按住」，否则按住方向键光标会瞬间飞出去——这正是
输入层区分两种状态的实际价值。
EOF
```

---

## 自查

### 规格覆盖

| 规格要求 | 对应任务 |
|---|---|
| §3 许可证扫描 | Task 1（`deny.toml` + CI） |
| §5 workspace 与 crate 分层 | Task 1、Task 8 |
| §6 固定职责线程 + 任务池 | Task 10 |
| §7.1 环面坐标、禁止手写欧氏距离 | Task 3 |
| §7.2 世界时钟、昼夜、四季 | Task 6 |
| §10.4 命名空间 ID 与索引映射 | Task 4 |
| §11.3 禁止硬编码用户可见字符串 | Task 11（标题存本地化键） |
| §13 文档注释、模块化、注释讲为什么 | 全部任务 |
| §14.1 L1 单元测试 | Task 2–11 |
| §14.1 L2 属性测试 | Task 3、Task 4、Task 9 |
| §14.1 L3 黑箱集成测试 | Task 3、Task 4、Task 9 的 `tests/` |
| §14.1 L8 性能基准 | Task 3 |
| §14.2 环面与 ID 的不变量 | Task 3、Task 4 |
| §14.4 跨平台确定性 | Task 7 + Task 1 的双平台矩阵 |
| §14.6 AAA 结构、行为化测试名 | 全部测试 |
| §14.7 CI 门禁 | Task 1 |
| §15 每阶段交付验收 demo | Task 12 |
| §17 世界状态禁用浮点 | Task 2（`Milli`）+ 全局约束 |
| 约束 C3 每实体确定性 RNG | Task 5 |

### 有意留给后续阶段的缺口（非遗漏）

- **覆盖率与变异测试**（`cargo-llvm-cov`、`cargo-mutants`）未接入 CI。P0 代码量尚小，先把双平台矩阵与许可证门禁跑通；这两项在 P1 接入。
- **模糊测试**（`cargo-fuzz`）未接入。P0 唯一解析外部数据的入口是 `NamespacedId::parse`，已用「任意输入都不崩溃」的属性测试覆盖。真正的 fuzz 目标（存档反序列化、mod 清单）出现在 P4 与 P5。
- **快照测试**（`insta`）与**视觉回归**未接入。P0 无渲染输出可快照，P1 接入。
- **E2E 测试**未接入。P0 没有完整游戏循环可端到端跑，Task 12 的验收 demo 是此阶段的等价物。真正的 E2E 在 P3 有了回合循环后建立。
- **硬编码字符串检查**与**手写欧氏距离检查**两个自研 CI 脚本未实现。P0 代码量小，靠评审把关；脚本随 P1 的 `ll-render` 一同交付，届时才有足够样本验证检查器不误报。

### 类型一致性核对

`TorusSize` / `TorusPos` / `Tick` / `Season` / `Milli` / `CoreError` / `DetRng` / `NamespacedId` / `ContentIndex` / `Interner` / `StateHasher` / `InputState` / `GameKey` / `JobPool` / `WindowConfig` / `AppHandler` / `PlatformError` 的命名与签名，在 Task 12 的验收 demo 中被完整串联调用了一遍，与各自定义处一致。
