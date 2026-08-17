# P1 渲染与动画层 实施计划

> **给执行者：** 必须配合 `superpowers:subagent-driven-development` 逐任务实施。步骤使用 `- [ ]` 复选框追踪。

**目标：** 建立 `ll-render` crate，实现像素完美的 2D 精灵批渲染、纹理图集、相机、图层、Y 排序与循环动画；同时完成 `ll-platform` 为接入渲染所必需的接口改造。

**架构：** 渲染分两级——先把整个场景绘制到一张 **640×360 的离屏纹理**，再整数倍放大 blit 到窗口。这既保证像素完美，也让视觉回归测试可以直接读回那张离屏纹理比对，无需依赖窗口尺寸。精灵通过实例化渲染批量提交，一层一次 draw call。

**技术栈：** `wgpu` 30、`bytemuck` 1.25、`image` 0.25（仅 PNG 特性）、`pollster` 1.0、`serde_json`；均为 MIT / Apache-2.0

**规格：** [`docs/superpowers/specs/2026-08-16-lostland-design.md`](../specs/2026-08-16-lostland-design.md)
**上阶段交接：** [`knowledge/handoff/p0-to-p1.md`](../../../knowledge/handoff/p0-to-p1.md)

## 全局约束

逐条摘自规格，**每个任务都隐含包含本节**：

- **世界状态禁止浮点。** 渲染层内部可以用 `f32`（GPU 要求），但**这些值绝不可回流入 `WorldState` 或被存档序列化**。判断标准：这个浮点会不会被 `serde` 写进存档？会，就是错的。
- **精灵的「逻辑占地格数 footprint」与「视觉尺寸 + 锚点 pivot」必须是两个独立概念**（规格 §12.1）。此为硬性接口约束，**不得延后**——后期补入将推翻整个精灵批处理布局。
- 逻辑分辨率固定 **640×360**，瓦片网格恒定 **16×16**。普通单位 16×24 占 1 格，重点目标 32×48 占 2×2 格。
- **窗口缩放只能是整数倍**。非整数倍会让像素边缘出现宽窄不一的锯齿。
- 环面距离必须走 `ll-core::torus` 的方法，**禁止手写欧氏距离**。
- 所有公开项必须有文档注释；注释解释**为什么**而非复述代码。
- 文件 200–400 行为宜，800 行为上限。
- 测试遵循 **AAA 结构**，测试名描述行为，一个测试只断言一件事。
- 依赖许可证须在 MIT / Apache-2.0 / BSD / zlib / ISC / Unicode-3.0 / OFL-1.1 之内。
- 提交信息格式 `<type>: <描述>`，正文说明**为什么**，**不得含任何 AI 署名或生成工具标记**。
- 注释、文档、提交信息一律用中文。

## 文件结构

```
crates/ll-platform/src/window.rs        改造：暴露 Window、转发尺寸事件、整数帧号、帧预算
crates/ll-platform/src/lib.rs           清理：移除无构造点的错误变体
crates/ll-render/
  Cargo.toml
  src/lib.rs                            模块声明 + RenderError
  src/gpu.rs                            wgpu 设备/队列/surface 初始化与重建
  src/target.rs                         640×360 离屏纹理 + 整数倍放大 blit
  src/atlas.rs                          图集：PNG 解码、GPU 纹理上传、帧元数据
  src/sprite.rs                         Footprint/Pivot/SpriteSize/DrawOrder 等核心类型
  src/batch.rs                          实例化批处理与排序
  src/camera.rs                         相机与环面坐标 → 屏幕坐标换算
  src/anim.rs                           动画剪辑与播放状态
  src/shader/sprite.wgsl                精灵着色器
  src/shader/blit.wgsl                  放大 blit 着色器
  tests/sprite_blackbox.rs              排序的黑箱属性测试
  tests/atlas_blackbox.rs               图集元数据解析（含畸形输入）
  tests/visual/baseline/                视觉回归基准 PNG
  examples/p1_acceptance.rs             P1 验收 demo
assets/atlas/                           demo 用的占位图集（PNG + JSON）
```

---

### Task 1：`ll-platform` 接口改造

**Files:**
- Modify: `crates/ll-platform/src/window.rs`
- Modify: `crates/ll-platform/src/lib.rs`
- Modify: `crates/ll-platform/examples/p0_acceptance.rs`

**Interfaces:**
- Consumes: 现有 `InputState`、`GameKey`、`RepeatConfig`
- Produces（`AppHandler` 改为如下形态，这是 P1 后续全部任务的地基）:
  - `pub struct FrameId(pub u64)`，方法 `next(self) -> FrameId`
  - `fn on_resume(&mut self, window: Arc<Window>, size: PhysicalSize<u32>)`
  - `fn on_resize(&mut self, size: PhysicalSize<u32>)`
  - `fn on_frame(&mut self, frame: FrameId, input: &InputState)`
  - `fn on_exit(&mut self)`
  - `WindowConfig` 新增 `pub target_fps: u32`（默认 60）与方法 `frame_budget(&self) -> Duration`

> **为什么传 `Arc<Window>`**：wgpu 的 surface 需要持有窗口的生命周期。共享所有权比移交简单——移交之后平台层自己反而没法再用窗口。
>
> **为什么帧号是整数**：动画需要时间基准。墙钟浮点秒数会让动画状态无法安全进入世界状态；整数帧号可以。

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-platform/src/window.rs 的 tests 模块内
    #[test]
    fn 帧号逐帧递增() {
        // Arrange
        let frame = FrameId(0);

        // Act
        let next = frame.next();

        // Assert
        assert_eq!(next, FrameId(1));
    }

    #[test]
    fn 默认帧率为六十() {
        // 60 是像素游戏的通行帧率，且与常见显示器刷新率对齐。
        // Arrange & Act
        let config = WindowConfig::default();

        // Assert
        assert_eq!(config.target_fps, 60);
    }

    #[test]
    fn 帧预算由目标帧率算出() {
        // Arrange
        let config = WindowConfig {
            target_fps: 60,
            ..WindowConfig::default()
        };

        // Act
        let budget = config.frame_budget();

        // Assert
        assert_eq!(budget, Duration::from_nanos(16_666_666));
    }

    #[test]
    fn 目标帧率为零时退化为不节流() {
        // 配置文件可能写出 0，与其除零崩溃不如退化为不限帧。
        // Arrange
        let config = WindowConfig {
            target_fps: 0,
            ..WindowConfig::default()
        };

        // Act
        let budget = config.frame_budget();

        // Assert
        assert_eq!(budget, Duration::ZERO);
    }
```

- [ ] **Step 2：运行测试确认失败**

```bash
cargo test -p ll-platform window
```
预期：编译失败，`cannot find type FrameId in this scope`。

- [ ] **Step 3：实现 `FrameId` 与帧预算**

```rust
/// 单调递增的帧号。
///
/// 动画播放以此为时间基准而非墙钟浮点秒数——整数帧号可以安全地进入
/// 世界状态并被存档序列化，浮点秒数不行（会破坏跨平台确定性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FrameId(pub u64);

impl FrameId {
    /// 下一帧。
    ///
    /// 用 `wrapping_add`：以 60fps 计需连续运行约 97 亿年才会回绕，
    /// 但回绕总好过在极端情况下 panic。
    pub const fn next(self) -> Self {
        FrameId(self.0.wrapping_add(1))
    }
}
```

`WindowConfig` 增加字段 `pub target_fps: u32`（`Default` 为 60）与：

```rust
impl WindowConfig {
    /// 每帧的时间预算。目标帧率为零时返回零，表示不节流。
    ///
    /// 节流是必需的：主循环用 `ControlFlow::Poll`，不加预算会空转吃满
    /// 一核。P0 阶段无渲染时这只是浪费电，接入 GPU 后会直接抬高功耗与
    /// 温度。
    pub fn frame_budget(&self) -> Duration {
        if self.target_fps == 0 {
            return Duration::ZERO;
        }
        Duration::from_nanos(1_000_000_000 / self.target_fps as u64)
    }
}
```

- [ ] **Step 4：改造 `AppHandler` 与事件循环**

```rust
/// 上层需要实现的回调。
///
/// 平台层只负责把系统事件归约成输入状态并按帧驱动，不含任何游戏逻辑。
pub trait AppHandler {
    /// 窗口就绪时调用，此时可以创建 GPU surface。
    ///
    /// 传 `Arc<Window>` 而非 `&Window`：wgpu 的 surface 需要持有窗口的
    /// 生命周期，共享所有权比移交所有权简单——移交后平台层自己就没法
    /// 再用窗口了。
    fn on_resume(&mut self, window: Arc<Window>, size: PhysicalSize<u32>);

    /// 窗口尺寸或缩放因子变化时调用，surface 必须据此重建。
    fn on_resize(&mut self, size: PhysicalSize<u32>);

    /// 每帧调用一次。
    fn on_frame(&mut self, frame: FrameId, input: &InputState);

    /// 退出前调用，用于保存与清理。
    fn on_exit(&mut self);
}
```

`App` 结构体改为持有 `window: Option<Arc<Window>>`、`frame: FrameId`、`last_frame_at: Option<Instant>`。

`resumed` 建窗成功后调用 `self.handler.on_resume(window.clone(), window.inner_size())`。

`window_event` 新增两个分支：

```rust
            WindowEvent::Resized(size) => {
                self.handler.on_resize(size);
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // 缩放因子变化后物理尺寸随之改变，直接用当前尺寸重建。
                if let Some(window) = &self.window {
                    self.handler.on_resize(window.inner_size());
                }
            }
```

`RedrawRequested` 分支加入帧预算节流：

```rust
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let budget = self.config.frame_budget();

                // 未到帧预算就跳过本帧的逻辑与绘制，只重新申请重绘。
                // 这里刻意不 sleep——sleep 会让窗口事件的响应延迟一整个
                // 帧时长，拖动窗口时会明显卡顿。
                let too_early = match self.last_frame_at {
                    Some(last) => !budget.is_zero() && now.duration_since(last) < budget,
                    None => false,
                };
                if too_early {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    return;
                }
                self.last_frame_at = Some(now);

                self.input.begin_frame(now, self.config.repeat);
                self.handler.on_frame(self.frame, &self.input);
                self.input.end_frame();
                self.frame = self.frame.next();

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
```

- [ ] **Step 5：清理死代码**

先确认哪些 `PlatformError` 变体已无构造点：

```bash
grep -rn "PlatformError::" crates/ | grep -v "src/lib.rs"
```

按规格 §13 的代码卫生要求，删除确无构造点的变体及其 `Display` 分支。若 `run` 因此失去可返回的错误类型，新增 `PlatformError::EventLoop(String)` 承接事件循环失败。

- [ ] **Step 6：适配 P0 验收 demo**

`examples/p0_acceptance.rs` 的 `impl AppHandler for Demo` 补上 `on_resume` 与 `on_resize`（各写一行 `tracing` 日志即可），`on_frame` 签名加 `_frame: FrameId`。

- [ ] **Step 7：全量门禁**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```
预期：全部通过。

- [ ] **Step 8：提交**

```bash
git add crates/ll-platform
git commit -F - <<'EOF'
refactor: 平台层为接入渲染做接口改造

P0 的 AppHandler 是按「不渲染」设计的，接 wgpu 时三处必然要动，交接
清单里已经预告过：回调拿不到 Window（surface 建不出来）、没有尺寸变化
转发（窗口一改大画面就拉伸或崩）、on_frame 没有时间基准。

帧号用整数而非墙钟浮点秒数，因为动画状态要能安全进入世界状态并被存档
序列化——浮点会破坏跨平台确定性。

窗口用 Arc 共享而非移交所有权：wgpu 的 surface 需要持有窗口生命周期，
移交之后平台层自己反而没法再用窗口。

帧预算节流是必需的：主循环用 Poll，不加预算会空转吃满一核。P0 无渲染
时只是浪费电，接 GPU 后会直接抬高功耗与温度。节流用跳帧而非 sleep，
因为 sleep 会让窗口事件响应延迟一整个帧时长，拖动窗口时明显卡顿。

顺带删掉已无构造点的错误变体（规格 §13 代码卫生）。
EOF
```

---

### Task 2：`ll-render` crate 骨架与 wgpu 初始化

**Files:**
- Create: `crates/ll-render/Cargo.toml`、`src/lib.rs`、`src/gpu.rs`
- Create: `src/{target,atlas,sprite,batch,camera,anim}.rs`（各一行文档注释占位）

**Interfaces:**
- Consumes: `ll-core`、`ll-platform` 的 `Arc<Window>`
- Produces:
  - `pub struct GpuContext`，构造 `GpuContext::new(window: Arc<Window>, size: PhysicalSize<u32>) -> Result<GpuContext, RenderError>`
  - `GpuContext::resize(&mut self, size: PhysicalSize<u32>)`
  - `GpuContext::device(&self) -> &wgpu::Device`、`queue(&self) -> &wgpu::Queue`、`surface_format(&self) -> wgpu::TextureFormat`
  - `pub enum RenderError { NoAdapter, DeviceRequest(String), SurfaceCreation(String), AtlasDecode(String), AtlasMetadata(String) }`

- [ ] **Step 1：先核实 wgpu 30 的实际 API**

wgpu 的 API 在大版本间变动频繁（`Instance::new`、`request_adapter`、`request_device`、`SurfaceConfiguration` 的字段都曾变过）。**动手前必须先查文档**：

```bash
cargo doc -p wgpu --no-deps --open
```

重点确认：`Instance::new` 的参数类型、`request_adapter` 的返回类型是否为 `Result`、`request_device` 的参数个数、`SurfaceConfiguration` 的全部必填字段（尤其 `desired_maximum_frame_latency`）。

**若实际 API 与本计划的示例不符，以实际 API 为准，并在报告中说明差异——不要猜测，也不要为迁就计划而降级 wgpu 版本。**

- [ ] **Step 2：创建 `Cargo.toml`**

```toml
[package]
name = "ll-render"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "迷途大陆的渲染层：wgpu 精灵批渲染、图集、相机、图层、Y 排序"

[dependencies]
ll-core = { path = "../ll-core" }
ll-platform = { path = "../ll-platform" }
wgpu = "30"
bytemuck = { version = "1.25", features = ["derive"] }
# 只启用 PNG：图集是我们自己资产管线的产物，格式由我们决定，
# 没必要为此把 JPEG/GIF/TIFF 等一整套解码器编进来。
image = { version = "0.25", default-features = false, features = ["png"] }
pollster = "1.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"

[dev-dependencies]
proptest.workspace = true
```

- [ ] **Step 3：创建 `src/lib.rs`**

```rust
//! 迷途大陆的渲染层。
//!
//! # 两级渲染
//!
//! 场景先被绘制到一张固定 640×360 的**离屏纹理**，再整数倍放大 blit 到
//! 窗口。这样做有三个好处：像素完美与窗口尺寸彻底解耦；视觉回归测试可以
//! 直接读回那张离屏纹理比对，不受运行环境分辨率影响；将来加后处理效果
//! （昼夜染色、天气）只需在这一层插一道 pass。
//!
//! # 浮点边界
//!
//! 本层内部使用 `f32`（GPU 要求如此），但**这些值绝不可回流入世界状态或
//! 被存档序列化**。世界状态是整数格坐标，渲染层负责把它们换算成像素。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod anim;
pub mod atlas;
pub mod batch;
pub mod camera;
pub mod gpu;
pub mod sprite;
pub mod target;

use core::fmt;

/// 渲染层的错误。
#[derive(Debug)]
pub enum RenderError {
    /// 找不到可用的图形适配器。
    NoAdapter,
    /// 请求 GPU 设备失败。
    DeviceRequest(String),
    /// 创建绘制表面失败。
    SurfaceCreation(String),
    /// 图集图片解码失败。
    AtlasDecode(String),
    /// 图集元数据不合法。
    AtlasMetadata(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::NoAdapter => write!(f, "no suitable graphics adapter found"),
            RenderError::DeviceRequest(why) => write!(f, "failed to request device: {why}"),
            RenderError::SurfaceCreation(why) => write!(f, "failed to create surface: {why}"),
            RenderError::AtlasDecode(why) => write!(f, "failed to decode atlas image: {why}"),
            RenderError::AtlasMetadata(why) => write!(f, "invalid atlas metadata: {why}"),
        }
    }
}

impl core::error::Error for RenderError {}
```

- [ ] **Step 4：写失败的测试**

`GpuContext` 需要真实窗口，CI 里测不了。因此本任务只对不依赖 GPU 的判断做单测：

```rust
// 追加到 crates/ll-render/src/gpu.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 零宽度被判定为不可呈现() {
        // 窗口最小化时尺寸变为零，此时重配 surface 会让 wgpu 报错甚至崩溃。
        // Arrange & Act & Assert
        assert!(!is_presentable(0, 720));
    }

    #[test]
    fn 零高度被判定为不可呈现() {
        // Arrange & Act & Assert
        assert!(!is_presentable(1280, 0));
    }

    #[test]
    fn 正常尺寸可呈现() {
        // Arrange & Act & Assert
        assert!(is_presentable(1280, 720));
    }
}
```

- [ ] **Step 5：实现 `gpu.rs`**

按 Step 1 查到的实际 API 实现。以下是**要求**，具体写法随 API：

- 适配器选择偏好高性能，但**必须允许回退到软件后端**——CI 无独显，回退不了视觉回归测试就跑不起来。
- `resize` 对零尺寸**必须直接返回而不重配 surface**。这是接 GPU 最常见的崩溃点。
- 呈现模式选 `Fifo`（垂直同步）：它是唯一所有平台都保证支持的模式，且我们已有帧预算节流，不需要更激进的模式。
- 把零尺寸判断提取为自由函数以便单测：

```rust
/// 该尺寸是否可用于配置绘制表面。
///
/// 窗口最小化时尺寸会变成零，此时重新配置 surface 会让 wgpu 报错甚至
/// 崩溃。提取成自由函数是为了让这个判断能被单测覆盖——`GpuContext`
/// 本身需要真实窗口，在 CI 里测不了。
fn is_presentable(width: u32, height: u32) -> bool {
    width > 0 && height > 0
}
```

- [ ] **Step 6：验证与提交**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check
```

```bash
git add crates/ll-render Cargo.lock Cargo.toml
git commit -F - <<'EOF'
feat: 渲染层骨架与 wgpu 设备初始化

场景将先绘制到固定 640×360 的离屏纹理再整数倍放大，因此设备初始化与
窗口尺寸解耦。这个结构让像素完美不依赖窗口大小，也让视觉回归测试可以
直接读回离屏纹理比对，不受运行环境分辨率影响。

适配器选择保留软件后端回退，因为 CI 没有独显——回退不了视觉回归测试
就跑不起来。

resize 对零尺寸直接返回：窗口最小化时宽高变成 0，重配 surface 会让 wgpu
报错甚至崩溃，这是接 GPU 最常见的崩溃点。把判断提成自由函数是为了能
单测，GpuContext 本身需要真实窗口，CI 里测不了。

image 只启用 png 特性：图集是我们自己资产管线的产物，格式由我们决定，
没必要把 JPEG/GIF/TIFF 一整套解码器编进来。
EOF
```

---

### Task 3：离屏渲染目标与整数倍放大

**Files:**
- Modify: `crates/ll-render/src/target.rs`
- Create: `crates/ll-render/src/shader/blit.wgsl`

**Interfaces:**
- Consumes: Task 2 的 `GpuContext`
- Produces:
  - `pub const LOGICAL_WIDTH: u32 = 640`、`pub const LOGICAL_HEIGHT: u32 = 360`
  - `pub struct Viewport { pub scale: u32, pub offset_x: u32, pub offset_y: u32 }`
  - `pub fn fit_viewport(window_width: u32, window_height: u32) -> Viewport`
  - `pub struct RenderTarget`，方法 `new(&GpuContext) -> RenderTarget`、`view(&self) -> &wgpu::TextureView`、`blit_to(&self, &GpuContext, &wgpu::TextureView, Viewport)`、`read_pixels(&self, &GpuContext) -> Vec<u8>`

> `read_pixels` 是视觉回归测试的钩子，**不是调试功能**——它让渲染结果能被冻结成基准 PNG 比对。

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-render/src/target.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 窗口恰为二倍时取二倍缩放() {
        // Arrange & Act
        let viewport = fit_viewport(1280, 720);

        // Assert
        assert_eq!(viewport.scale, 2);
    }

    #[test]
    fn 非整数倍时向下取整而非拉伸() {
        // 非整数倍缩放会让相邻像素被放大成宽窄不一的方块，是像素美术
        // 最刺眼的瑕疵。宁可留黑边。
        // Arrange & Act
        let viewport = fit_viewport(1500, 800);

        // Assert
        assert_eq!(viewport.scale, 2);
    }

    #[test]
    fn 非整数倍时画面居中留黑边() {
        // Arrange & Act
        let viewport = fit_viewport(1500, 800);

        // Assert
        assert_eq!(viewport.offset_x, (1500 - LOGICAL_WIDTH * 2) / 2);
    }

    #[test]
    fn 窗口小于逻辑分辨率时仍取一倍() {
        // 零倍会让画面完全消失，比裁切更糟。
        // Arrange & Act
        let viewport = fit_viewport(320, 180);

        // Assert
        assert_eq!(viewport.scale, 1);
    }

    #[test]
    fn 缩放倍率受限于较短的一边() {
        // 宽度够 4 倍但高度只够 2 倍时必须取 2 倍，否则画面会被切掉。
        // Arrange & Act
        let viewport = fit_viewport(2560, 720);

        // Assert
        assert_eq!(viewport.scale, 2);
    }
}
```

- [ ] **Step 2：运行确认失败**

```bash
cargo test -p ll-render target
```
预期：`cannot find function fit_viewport in this scope`。

- [ ] **Step 3：实现**

```rust
/// 逻辑分辨率宽度。规格决策 6 固定为 640。
pub const LOGICAL_WIDTH: u32 = 640;

/// 逻辑分辨率高度。规格决策 6 固定为 360。
pub const LOGICAL_HEIGHT: u32 = 360;

/// 离屏画面在窗口中的摆放方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// 整数缩放倍率，恒 ≥ 1。
    pub scale: u32,
    /// 居中后的左侧黑边宽度。
    pub offset_x: u32,
    /// 居中后的上方黑边高度。
    pub offset_y: u32,
}

/// 算出给定窗口尺寸下的最大整数缩放与居中偏移。
///
/// **只允许整数倍**：非整数倍缩放会让相邻像素被放大成宽窄不一的方块，
/// 是像素美术最刺眼的瑕疵。宁可在四周留黑边，也不拉伸。
///
/// 倍率取宽高两个方向的较小者，否则画面会被窗口切掉。窗口小于逻辑
/// 分辨率时仍取 1 倍——零倍会让画面完全消失，比裁切更糟。
pub fn fit_viewport(window_width: u32, window_height: u32) -> Viewport {
    let scale = (window_width / LOGICAL_WIDTH)
        .min(window_height / LOGICAL_HEIGHT)
        .max(1);

    let drawn_width = LOGICAL_WIDTH * scale;
    let drawn_height = LOGICAL_HEIGHT * scale;

    Viewport {
        scale,
        // saturating_sub：窗口比画面还小时偏移取 0，而不是回绕成天文数字。
        offset_x: window_width.saturating_sub(drawn_width) / 2,
        offset_y: window_height.saturating_sub(drawn_height) / 2,
    }
}
```

`RenderTarget` 持有一张 `LOGICAL_WIDTH × LOGICAL_HEIGHT` 的纹理（格式与 surface 一致，用途附加 `COPY_SRC` 以便 `read_pixels`）及其视图，另有一条全屏三角形的 blit 管线。

`blit.wgsl` **必须用最近邻采样**（`FilterMode::Nearest`）——线性插值会把像素边缘糊掉，正是要避免的。

- [ ] **Step 4：验证并提交**

```bash
cargo test -p ll-render target && cargo clippy --workspace --all-targets -- -D warnings
```

```bash
git add crates/ll-render
git commit -F - <<'EOF'
feat: 离屏渲染目标与整数倍放大

场景绘制到固定 640×360 的离屏纹理再放大到窗口，使像素完美与窗口尺寸
彻底解耦。

只允许整数倍缩放：非整数倍会让相邻像素被放大成宽窄不一的方块，是像素
美术最刺眼的瑕疵。宁可四周留黑边也不拉伸。倍率取宽高两方向的较小者，
否则画面会被窗口切掉；窗口小于逻辑分辨率时仍取 1 倍，因为零倍会让画面
完全消失，比裁切更糟。

blit 用最近邻采样，线性插值会把像素边缘糊掉。

read_pixels 不是调试功能而是视觉回归测试的钩子——它让渲染结果能被冻结
成基准 PNG 比对，且不受运行环境分辨率影响。
EOF
```

---

### Task 4：精灵核心类型与 Y 排序

**Files:**
- Modify: `crates/ll-render/src/sprite.rs`
- Create: `crates/ll-render/tests/sprite_blackbox.rs`

**Interfaces:**
- Produces:
  - `pub const TILE_SIZE: u32 = 16`
  - `pub struct Footprint { pub width: u8, pub height: u8 }`，方法 `tile_count(&self) -> u32`
  - `pub struct SpriteSize { pub width: u16, pub height: u16 }`
  - `pub struct Pivot { pub x: i16, pub y: i16 }`
  - `pub struct Layer(pub u8)`，常量 `Layer::TERRAIN`、`DECOR`、`ENTITY`、`EFFECT`、`UI`
  - `pub struct DrawOrder`，构造 `DrawOrder::new(layer: Layer, foot_y: i32, entity: u64) -> DrawOrder`，实现 `Ord`

> **这是规格 §12.1 点名的硬性约束**：`Footprint`（占几格）与 `SpriteSize` + `Pivot`（画多大、锚在哪）必须是**独立类型**，不得合并。合并后重点目标的 32×48 精灵就无法既只占 2×2 格、又画得比格子高。

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-render/src/sprite.rs
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
        let footprint = Footprint { width: 1, height: 1 };

        // Act & Assert
        assert_eq!(footprint.tile_count(), 1);
    }

    #[test]
    fn 重点目标占四格() {
        // 32×48 的精灵占 2×2 格却画得比格子高——这正是尺寸解耦的意义。
        // Arrange
        let footprint = Footprint { width: 2, height: 2 };

        // Act & Assert
        assert_eq!(footprint.tile_count(), 4);
    }
}
```

- [ ] **Step 2：运行确认失败**

```bash
cargo test -p ll-render sprite
```
预期：`cannot find type DrawOrder in this scope`。

- [ ] **Step 3：实现**

```rust
/// 瓦片边长（像素）。规格决策 6 固定为 16。
pub const TILE_SIZE: u32 = 16;

/// 精灵的**逻辑占地格数**。
///
/// 与 [`SpriteSize`]（视觉像素尺寸）刻意分开：重点目标的精灵是 32×48
/// 像素，但只占 2×2 格——它画得比自己占的地方高。若把两者合并成一个
/// 概念，这种表现就做不出来，而后期再拆会推翻整个批处理布局
/// （规格 §12.1 明确要求此项不得延后）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
```

- [ ] **Step 4：写黑箱属性测试**

```rust
// crates/ll-render/tests/sprite_blackbox.rs
//! 绘制顺序的黑箱属性测试。
//!
//! 排序的正确性靠单个用例很难说清——真正要保证的是它构成一个**全序**：
//! 任意两个键必定可比较且顺序唯一。这正是属性测试的用武之地。

use ll_render::sprite::{DrawOrder, Layer};
use proptest::prelude::*;

fn any_order() -> impl Strategy<Value = DrawOrder> {
    (0u8..5, -1000i32..1000, 0u64..100)
        .prop_map(|(layer, foot_y, entity)| DrawOrder::new(Layer(layer), foot_y, entity))
}

proptest! {
    #[test]
    fn 排序键构成全序(a in any_order(), b in any_order()) {
        // 全序意味着：要么相等，要么严格一大一小，不存在「无法比较」。
        // Act & Assert
        prop_assert!(a < b || b < a || a == b);
    }

    #[test]
    fn 比较是反对称的(a in any_order(), b in any_order()) {
        // Act & Assert
        prop_assert_eq!(a < b, b > a);
    }

    #[test]
    fn 排序具有传递性(a in any_order(), b in any_order(), c in any_order()) {
        // 传递性若不成立，排序结果会依赖比较顺序，遮挡关系就会逐帧抖动。
        // Act & Assert
        if a < b && b < c {
            prop_assert!(a < c);
        }
    }
}
```

- [ ] **Step 5：验证并提交**

```bash
cargo test -p ll-render
```

```bash
git add crates/ll-render
git commit -F - <<'EOF'
feat: 精灵核心类型与 Y 排序键

规格 §12.1 点名要求：逻辑占地格数与视觉尺寸+锚点必须是独立概念，且
不得延后。这里落实为 Footprint / SpriteSize / Pivot 三个类型。重点目标
的精灵 32×48 像素却只占 2×2 格——它画得比自己占的地方高；把两者合并
就做不出这种表现，而后期再拆会推翻整个批处理布局。

排序键用脚底纵坐标而非精灵原点：用原点会让高精灵在视觉上错误地挡住
前排单位。

必须有实体号作第二排序键构成稳定全序，否则同一世界状态可能画出不同的
遮挡关系——既是视觉抖动，也会让视觉回归测试无法冻结基准。

属性测试验的是全序、反对称、传递性这三条不变量，而不是逐个用例——
排序的正确性靠单个用例很难说清，而传递性若不成立，排序结果会依赖比较
顺序，遮挡关系就会逐帧抖动。
EOF
```

---

### Task 5：纹理图集与元数据校验

**Files:**
- Modify: `crates/ll-render/src/atlas.rs`
- Create: `crates/ll-render/tests/atlas_blackbox.rs`
- Create: `assets/atlas/placeholder.png`、`assets/atlas/placeholder.json`、`assets/atlas/README.md`

**Interfaces:**
- Produces:
  - `pub struct FrameRect { pub x: u16, pub y: u16, pub width: u16, pub height: u16 }`
  - `pub struct AtlasEntry { pub name: String, pub rect: FrameRect, pub pivot: Pivot, pub footprint: Footprint }`
  - `pub struct AtlasMetadata { pub image: String, pub entries: Vec<AtlasEntry> }`，方法 `parse(json: &str) -> Result<AtlasMetadata, RenderError>`、`lookup(&self, name: &str) -> Option<&AtlasEntry>`
  - `pub struct Atlas`，构造 `Atlas::load(&GpuContext, AtlasMetadata, &[u8]) -> Result<Atlas, RenderError>`

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-render/src/atlas.rs
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "image": "placeholder.png",
        "entries": [
            { "name": "hero_idle_0",
              "rect": { "x": 0, "y": 0, "width": 16, "height": 24 },
              "pivot": { "x": 8, "y": 24 },
              "footprint": { "width": 1, "height": 1 } }
        ]
    }"#;

    #[test]
    fn 解析合法元数据得到对应条目() {
        // Arrange & Act
        let metadata = AtlasMetadata::parse(SAMPLE).expect("样例是合法 JSON");

        // Assert
        assert_eq!(metadata.entries.len(), 1);
    }

    #[test]
    fn 可按名字查到条目() {
        // Arrange
        let metadata = AtlasMetadata::parse(SAMPLE).expect("样例是合法 JSON");

        // Act
        let entry = metadata.lookup("hero_idle_0");

        // Assert
        assert!(entry.is_some());
    }

    #[test]
    fn 查不到的名字返回空值() {
        // Arrange
        let metadata = AtlasMetadata::parse(SAMPLE).expect("样例是合法 JSON");

        // Act & Assert
        assert!(metadata.lookup("does_not_exist").is_none());
    }

    #[test]
    fn 畸形输入返回错误而非崩溃() {
        // 图集元数据会来自第三方 mod，属于外部不可信输入。
        // Arrange & Act
        let result = AtlasMetadata::parse("{ this is not json");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 零宽度的帧矩形被拒绝() {
        // 零宽或零高的帧会生成退化的四边形，在部分驱动上是未定义行为。
        // Arrange
        let broken = SAMPLE.replace("\"width\": 16", "\"width\": 0");

        // Act
        let result = AtlasMetadata::parse(&broken);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 重名条目被拒绝() {
        // 重名会让 lookup 的结果取决于顺序，是 mod 冲突的常见来源。
        // Arrange
        let entry = r#"{ "name": "hero_idle_0",
              "rect": { "x": 0, "y": 0, "width": 16, "height": 24 },
              "pivot": { "x": 8, "y": 24 },
              "footprint": { "width": 1, "height": 1 } },"#;
        let duplicated = SAMPLE.replace("\"entries\": [", &format!("\"entries\": [{entry}"));

        // Act
        let result = AtlasMetadata::parse(&duplicated);

        // Assert
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2：运行确认失败**

```bash
cargo test -p ll-render atlas
```
预期：`cannot find type AtlasMetadata in this scope`。

- [ ] **Step 3：实现**

`parse` 用 `serde_json` 反序列化后**必须做语义校验**：拒绝零尺寸矩形、拒绝重名条目。校验失败返回 `RenderError::AtlasMetadata` 并附带具体原因（要能定位到是哪个条目）。

`Atlas::load` 用 `image` 解码 PNG 为 RGBA8，上传为 GPU 纹理，采样器用 `FilterMode::Nearest`。

- [ ] **Step 4：生成占位图集**

写一段一次性脚本生成 `assets/atlas/placeholder.png`：一张 64×64 的 RGBA PNG，含四块内容——16×24 的「普通单位」（纯色块加一个可辨朝向的缺口）、32×48 的「重点目标」、两块 16×16 的地形。配套写 `placeholder.json`。

同时写 `assets/atlas/README.md`，注明这是**程序生成的临时占位资产、待美术替换**（规格 §13 的代码卫生要求：临时产物必须自带说明，否则半年后没人知道能不能删）。

- [ ] **Step 5：写黑箱模糊风格测试**

```rust
// crates/ll-render/tests/atlas_blackbox.rs
//! 图集元数据解析的黑箱测试。
//!
//! 图集元数据会来自第三方 mod，属于外部不可信输入。规格 §14.3 要求这类
//! 入口做模糊测试；在正式接入 cargo-fuzz 之前，先用属性测试守住
//! 「任意输入都不崩溃」这条底线。

use ll_render::atlas::AtlasMetadata;
use proptest::prelude::*;

/// 一段结构完整的合法元数据，供截断测试取前缀。
const FULL: &str = r#"{"image":"a.png","entries":[{"name":"x","rect":{"x":0,"y":0,"width":1,"height":1},"pivot":{"x":0,"y":0},"footprint":{"width":1,"height":1}}]}"#;

proptest! {
    #[test]
    fn 任意输入都不会崩溃(raw in ".{0,256}") {
        // Act：只要求不 panic，返回 Err 完全正常。
        let _ = AtlasMetadata::parse(&raw);
    }

    #[test]
    fn 截断的合法输入也不会崩溃(cut in 0usize..FULL.len()) {
        // 损坏的资产文件最常见的形态就是被截断。
        // Arrange
        let truncated = &FULL[..cut];

        // Act
        let _ = AtlasMetadata::parse(truncated);
    }
}
```

- [ ] **Step 6：验证并提交**

```bash
cargo test -p ll-render && cargo clippy --workspace --all-targets -- -D warnings
```

```bash
git add crates/ll-render assets/atlas
git commit -F - <<'EOF'
feat: 纹理图集与元数据校验

图集元数据会来自第三方 mod，属于外部不可信输入，因此 parse 在
serde_json 反序列化之后还要做语义校验：拒绝零尺寸矩形（会生成退化的
四边形，部分驱动上是未定义行为）、拒绝重名条目（会让 lookup 结果取决
于顺序，是 mod 冲突的常见来源）。

规格 §14.3 要求这类入口做模糊测试。正式接入 cargo-fuzz 之前，先用属性
测试守住「任意输入不崩溃」与「截断输入不崩溃」——损坏的资产文件最
常见的形态就是被截断。

采样器用最近邻，线性插值会把像素边缘糊掉。

占位图集是程序生成的临时资产，已在同目录 README 注明待美术替换——
临时产物必须自带说明，否则半年后没人知道能不能删。
EOF
```

---

### Task 6：相机与环面坐标换算

**Files:**
- Modify: `crates/ll-render/src/camera.rs`

**Interfaces:**
- Consumes: `ll_core::torus::{TorusPos, TorusSize}`、Task 3 的 `LOGICAL_WIDTH`/`LOGICAL_HEIGHT`、Task 4 的 `TILE_SIZE`
- Produces:
  - `pub struct Camera { pub center: TorusPos, pub world: TorusSize }`
  - `Camera::world_to_screen(&self, pos: TorusPos) -> (i32, i32)`
  - `Camera::visible_tiles(&self) -> Vec<TorusPos>`

> **环面跨接缝绘制不需要画多份拷贝。** 用 `TorusSize::delta` 求出目标相对相机的**最短带符号位移**，再乘以瓦片边长即得屏幕偏移——绕接缝的情形已被 `delta` 处理掉了。这比「画 2~4 份偏移拷贝」的常见做法简单得多，也快得多。

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-render/src/camera.rs
#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::torus::TorusSize;

    fn camera_at(x: i32, y: i32) -> Camera {
        let world = TorusSize::new(32, 32).expect("常量非零");
        Camera {
            center: world.wrap(x, y),
            world,
        }
    }

    #[test]
    fn 相机中心落在视口正中() {
        // Arrange
        let camera = camera_at(10, 10);

        // Act
        let (sx, sy) = camera.world_to_screen(camera.center);

        // Assert
        assert_eq!(
            (sx, sy),
            (LOGICAL_WIDTH as i32 / 2, LOGICAL_HEIGHT as i32 / 2)
        );
    }

    #[test]
    fn 相邻一格相差一个瓦片边长() {
        // Arrange
        let camera = camera_at(10, 10);
        let neighbour = camera.world.wrap(11, 10);

        // Act
        let (cx, _) = camera.world_to_screen(camera.center);
        let (nx, _) = camera.world_to_screen(neighbour);

        // Assert
        assert_eq!(nx - cx, TILE_SIZE as i32);
    }

    #[test]
    fn 跨接缝的目标按最短方向绘制而非绕远() {
        // 相机在 x=1、目标在 x=31，向西绕 2 格即到，不该被画到屏幕右侧
        // 30 格开外——那正是「小地图上明明相邻、画面上却在天边」的成因。
        // Arrange
        let camera = camera_at(1, 10);
        let target = camera.world.wrap(31, 10);

        // Act
        let (cx, _) = camera.world_to_screen(camera.center);
        let (tx, _) = camera.world_to_screen(target);

        // Assert
        assert_eq!(tx - cx, -2 * TILE_SIZE as i32);
    }

    #[test]
    fn 可见瓦片数量覆盖整个视口() {
        // 视口 640×360、瓦片 16×16，即 40×23 格（含边缘半格各多一列/行）。
        // Arrange
        let camera = camera_at(10, 10);

        // Act
        let tiles = camera.visible_tiles();

        // Assert
        assert!(tiles.len() >= (LOGICAL_WIDTH / TILE_SIZE) as usize * (LOGICAL_HEIGHT / TILE_SIZE) as usize);
    }
}
```

- [ ] **Step 2：运行确认失败**

```bash
cargo test -p ll-render camera
```
预期：`cannot find type Camera in this scope`。

- [ ] **Step 3：实现**

```rust
    /// 把环面世界坐标换算成屏幕像素坐标。
    ///
    /// # 为什么不需要画多份拷贝
    ///
    /// 环面世界里，相机附近的目标可能「绕过接缝」才最近。常见做法是把整个
    /// 场景画 2~4 份偏移拷贝以覆盖接缝，那既慢又容易出边界错误。
    ///
    /// 这里直接用 [`TorusSize::delta`] 求出目标相对相机的**最短带符号
    /// 位移**——绕接缝的情形已经被它处理掉了，每个目标仍然只画一次。
    /// 这是 P0 把距离与位移做成 `TorusSize` 的方法而非坐标的方法所带来的
    /// 直接回报。
    pub fn world_to_screen(&self, pos: TorusPos) -> (i32, i32) {
        let (dx, dy) = self.world.delta(self.center, pos);
        (
            LOGICAL_WIDTH as i32 / 2 + dx * TILE_SIZE as i32,
            LOGICAL_HEIGHT as i32 / 2 + dy * TILE_SIZE as i32,
        )
    }
```

`visible_tiles` 以相机为中心，向四周各取「半个视口 + 1 格」的范围，用 `wrap` 归一化后返回。多取的一格是为了让边缘的瓦片不会在相机移动时突然出现。

- [ ] **Step 4：验证并提交**

```bash
cargo test -p ll-render camera
```

```bash
git add crates/ll-render/src/camera.rs
git commit -F - <<'EOF'
feat: 相机与环面坐标换算

环面世界里相机附近的目标可能绕过接缝才最近。常见做法是把整个场景画
2~4 份偏移拷贝以覆盖接缝，既慢又容易出边界错误。

这里直接用 TorusSize::delta 求最短带符号位移——绕接缝的情形已经被它
处理掉了，每个目标仍然只画一次。这是 P0 把距离与位移做成 TorusSize 的
方法而非坐标的方法所带来的直接回报。

测试里那条「跨接缝的目标按最短方向绘制」守的正是「小地图上明明相邻、
画面上却在天边」这类缺陷。

visible_tiles 向四周各多取一格，是为了让边缘瓦片不会在相机移动时突然
出现。
EOF
```

---

### Task 7：精灵实例化批处理

**Files:**
- Modify: `crates/ll-render/src/batch.rs`
- Create: `crates/ll-render/src/shader/sprite.wgsl`

**Interfaces:**
- Produces:
  - `pub struct SpriteInstance`（`#[repr(C)]` + `bytemuck::Pod` + `Zeroable`）：屏幕位置、尺寸、UV 矩形、颜色调制
  - `pub struct SpriteBatch`，方法 `new(&GpuContext, &Atlas, wgpu::TextureFormat) -> SpriteBatch`、`push(&mut self, order: DrawOrder, instance: SpriteInstance)`、`flush(&mut self, &GpuContext, &wgpu::TextureView)`

实现要点（**这些是要求**）：

- 顶点缓冲只放一个单位四边形；每个精灵是一个 **instance**。这样一层的所有精灵一次 draw call 画完。规格决策 15 要求支撑大量实体，逐精灵一次 draw call 在几百个实体时就会撞墙。
- 排序用 `sort_unstable_by_key`——`DrawOrder` 已含实体号作最终平局打破键、本身就是全序，不需要稳定排序的额外开销。
- 实例缓冲容量不足时**成倍扩容并重建**，不要每帧重新分配——每帧分配会在实体数量波动时造成持续的显存搅动。
- `flush` 只负责把已算好的屏幕坐标搬上 GPU，**不含任何世界逻辑**。

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-render/src/batch.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite::{DrawOrder, Layer};

    #[test]
    fn 实例结构体大小符合着色器预期() {
        // GPU 缓冲要求确定的内存布局。结构体若含填充或字段重排，着色器
        // 读到的就是垃圾数据，而现象是画面错乱而非报错，极难定位。
        // Arrange & Act
        let size = core::mem::size_of::<SpriteInstance>();

        // Assert：位置尺寸 4×f32 + UV 4×f32 + 颜色 4×f32 = 48 字节
        assert_eq!(size, 48);
    }

    #[test]
    fn 排序后按绘制顺序键升序排列() {
        // Arrange
        let mut items = vec![
            DrawOrder::new(Layer::ENTITY, 100, 1),
            DrawOrder::new(Layer::TERRAIN, 50, 2),
            DrawOrder::new(Layer::ENTITY, 50, 3),
        ];

        // Act
        items.sort_unstable();

        // Assert
        assert_eq!(items[0], DrawOrder::new(Layer::TERRAIN, 50, 2));
    }

    #[test]
    fn 扩容后容量至少翻倍() {
        // 每帧重新分配会在实体数量波动时造成持续的显存搅动。
        // Arrange
        let current = 256usize;

        // Act
        let grown = grow_capacity(current, 300);

        // Assert
        assert!(grown >= 512);
    }
}
```

对应地把扩容策略提取成可测的自由函数：

```rust
/// 算出容纳 `needed` 个实例所需的新容量。
///
/// 成倍增长而非按需精确分配：每帧精确分配会在实体数量小幅波动时造成
/// 持续的缓冲重建，显存与 CPU 双双受损。
fn grow_capacity(current: usize, needed: usize) -> usize {
    let mut capacity = current.max(1);
    while capacity < needed {
        capacity *= 2;
    }
    capacity
}
```

- [ ] **Step 2：运行确认失败，Step 3：实现，Step 4：验证**

```bash
cargo test -p ll-render batch && cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5：提交**

```bash
git add crates/ll-render
git commit -F - <<'EOF'
feat: 精灵实例化批处理与渲染管线

顶点缓冲只放一个单位四边形，每个精灵作为 instance 提交，一层的所有精灵
一次 draw call 画完。规格决策 15 要求支撑大量实体，逐精灵一次 draw call
在几百个实体时就会撞墙。

排序用 sort_unstable：DrawOrder 已含实体号作最终平局打破键、本身就是
全序，不需要稳定排序的额外开销。

实例缓冲不足时成倍扩容，而不是每帧按需精确分配——精确分配会在实体数量
小幅波动时造成持续的缓冲重建，显存与 CPU 双双受损。扩容策略提成自由
函数以便单测。

加了内存布局断言：GPU 缓冲要求确定的布局，结构体若含填充或字段重排，
着色器读到的就是垃圾数据，而且现象是画面错乱而非报错，极难定位。
EOF
```

---

### Task 8：动画剪辑与播放

**Files:**
- Modify: `crates/ll-render/src/anim.rs`

**Interfaces:**
- Consumes: `ll_platform::window::FrameId`
- Produces:
  - `pub struct Clip { pub frames: Vec<String>, pub frames_per_step: u32, pub looping: bool }`
  - `pub struct Playback`，构造 `Playback::new(clip: usize, started_at: FrameId)`
  - `Playback::current_frame<'a>(&self, clips: &'a [Clip], now: FrameId) -> Option<&'a str>`

> **动画以整数帧号为时间基准，不用墙钟秒数。** 这样动画状态可以安全地进入世界状态并被存档序列化——存档读回后动画会精确接续，而不是跳到随机一帧。

- [ ] **Step 1：写失败的测试**

```rust
// 追加到 crates/ll-render/src/anim.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn walk_clip() -> Clip {
        Clip {
            frames: vec!["w0".into(), "w1".into(), "w2".into()],
            frames_per_step: 5,
            looping: true,
        }
    }

    #[test]
    fn 起始时刻停在第一帧() {
        // Arrange
        let clips = vec![walk_clip()];
        let playback = Playback::new(0, FrameId(100));

        // Act
        let frame = playback.current_frame(&clips, FrameId(100));

        // Assert
        assert_eq!(frame, Some("w0"));
    }

    #[test]
    fn 经过一个步长后推进到第二帧() {
        // Arrange
        let clips = vec![walk_clip()];
        let playback = Playback::new(0, FrameId(100));

        // Act
        let frame = playback.current_frame(&clips, FrameId(105));

        // Assert
        assert_eq!(frame, Some("w1"));
    }

    #[test]
    fn 循环剪辑播完后回到首帧() {
        // Arrange：3 帧 × 5 步长 = 15，故第 115 帧回到起点。
        let clips = vec![walk_clip()];
        let playback = Playback::new(0, FrameId(100));

        // Act
        let frame = playback.current_frame(&clips, FrameId(115));

        // Assert
        assert_eq!(frame, Some("w0"));
    }

    #[test]
    fn 非循环剪辑播完后停在末帧() {
        // 施法、受击这类一次性动画播完应停住，跳回起手姿势会像抽搐。
        // Arrange
        let clips = vec![Clip {
            looping: false,
            ..walk_clip()
        }];
        let playback = Playback::new(0, FrameId(100));

        // Act
        let frame = playback.current_frame(&clips, FrameId(999));

        // Assert
        assert_eq!(frame, Some("w2"));
    }

    #[test]
    fn 单帧剪辑恒返回该帧() {
        // 规格要求像素小人可以是静止的，也可以循环播放动画。
        // Arrange
        let clips = vec![Clip {
            frames: vec!["idle".into()],
            frames_per_step: 1,
            looping: true,
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act & Assert
        assert_eq!(playback.current_frame(&clips, FrameId(12345)), Some("idle"));
    }

    #[test]
    fn 空剪辑返回空值而非崩溃() {
        // 损坏的 mod 数据可能定义出没有任何帧的剪辑。
        // Arrange
        let clips = vec![Clip {
            frames: Vec::new(),
            frames_per_step: 5,
            looping: true,
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act & Assert
        assert_eq!(playback.current_frame(&clips, FrameId(10)), None);
    }

    #[test]
    fn 步长为零时停在首帧而非除零崩溃() {
        // 配置或 mod 可能写出 0。
        // Arrange
        let clips = vec![Clip {
            frames_per_step: 0,
            ..walk_clip()
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act & Assert
        assert_eq!(playback.current_frame(&clips, FrameId(999)), Some("w0"));
    }

    #[test]
    fn 剪辑索引越界返回空值() {
        // Arrange
        let clips = vec![walk_clip()];
        let playback = Playback::new(99, FrameId(0));

        // Act & Assert
        assert_eq!(playback.current_frame(&clips, FrameId(0)), None);
    }
}
```

- [ ] **Step 2：运行确认失败，Step 3：实现，Step 4：验证**

```bash
cargo test -p ll-render anim
```

- [ ] **Step 5：提交**

```bash
git add crates/ll-render/src/anim.rs
git commit -F - <<'EOF'
feat: 动画剪辑与播放

动画以整数帧号为时间基准而非墙钟秒数。这样动画状态可以安全地进入世界
状态并被存档序列化——存档读回后动画精确接续，而不是跳到随机一帧；用
浮点秒数则做不到，还会破坏跨平台确定性。

非循环剪辑播完停在末帧而非跳回首帧：施法、受击这类一次性动画播完应
停住，跳回起手姿势会看起来像抽搐。

空剪辑返回 None、步长为零时停在首帧、剪辑索引越界返回 None——三者都
可能来自损坏的 mod 数据，必须降级而不是崩溃。
EOF
```

---

### Task 9：P1 验收 Demo 与视觉回归基线

**Files:**
- Create: `crates/ll-render/examples/p1_acceptance.rs`
- Create: `crates/ll-render/tests/visual/README.md`
- Create: `crates/ll-render/tests/visual/baseline/`（存放基准 PNG）

**Interfaces:** 消费前八个任务的全部公开 API

- [ ] **Step 1：编写验收 demo**

要求它同时展示以下各项，且每一项坏掉都能一眼看出来：

1. 一层瓦片地形铺满视口
2. 一个 16×24 的普通单位，**循环播放行走动画**
3. 一个 32×48 的重点目标，**占 2×2 格但画得更高**；当普通单位站在它上方时，它**正确遮挡**对方
4. 方向键移动相机，**走到世界边缘时画面无缝绕回**（不是跳变）
5. 窗口可缩放，画面始终整数倍居中、四周黑边
6. 按 `F2` 把当前离屏纹理存成 PNG——这是冻结视觉回归基准的入口

- [ ] **Step 2：运行并人工验证**

```bash
cargo run -p ll-render --example p1_acceptance
```

逐项确认（**无人值守环境无法交互，须在报告中如实说明哪些项未验证，不要谎报**）：

- [ ] 地形铺满、无缝隙、无撕裂
- [ ] 小人动画循环播放
- [ ] 重点目标画得比格子高，且遮挡关系正确
- [ ] 相机跨接缝时画面连续
- [ ] 拉伸窗口时画面保持整数倍且居中

- [ ] **Step 3：建立视觉回归基准**

按 `F2` 存出基准 PNG 到 `crates/ll-render/tests/visual/baseline/`，并写 `README.md`：

> 这些 PNG 是**视觉回归基准**。渲染改动导致比对失败时，必须先判断是有意的视觉调整还是缺陷；确认是有意调整才更新基准，并在提交信息里说明改了什么、为什么。
>
> **绝不允许「测试挂了就重新截图覆盖」**——那等于删掉这道防线。这条规矩与 `crates/ll-core/tests/determinism.rs` 顶部的黄金基准规矩是同一条。

- [ ] **Step 4：全量门禁**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check
```

- [ ] **Step 5：提交**

```bash
git add crates/ll-render assets
git commit -F - <<'EOF'
feat: P1 验收 demo 与视觉回归基线

规格决策 3 要求每阶段交付可独立运行的验收 demo。这个 demo 把渲染层的
每一条硬性约束都摆到画面上：精灵尺寸解耦（32×48 的重点目标占 2×2 格
却画得更高）、Y 排序遮挡、环面跨接缝的无缝绕回、整数倍缩放与黑边。
任何一条坏了都能一眼看出来，而不必去读代码。

F2 存图是冻结视觉回归基准的入口，不是调试功能。基准目录的 README 写明
了规矩：比对失败时先判断是有意调整还是缺陷，绝不允许「挂了就重新截图
覆盖」——那等于删掉这道防线。这与 ll-core 确定性黄金基准的规矩一致。
EOF
```

---

## 自查

### 规格覆盖

| 规格要求 | 对应任务 |
|---|---|
| 决策 1 自研 2D 精灵批渲染层 | Task 7 |
| 决策 3 每阶段交付验收 demo | Task 9 |
| 决策 6 640×360 / 16×16 / 尺寸解耦 | Task 3、Task 4 |
| 决策 15 支撑大量实体 | Task 7（实例化批处理） |
| 决策 23 环面拓扑（渲染侧） | Task 6 |
| §5 `ll-render` 职责边界 | Task 2–8 |
| §12.1 Footprint 与 SpriteSize+Pivot 解耦 | Task 4（规格点名不得延后） |
| §13 文件规模、文档注释、代码卫生 | 全部任务 + Task 1 清死代码 |
| §14.1 L1 单元测试 | Task 1–8 |
| §14.1 L2 属性测试 | Task 4、Task 5 |
| §14.1 L3 黑箱集成测试 | Task 4、Task 5 的 `tests/` |
| §14.1 L7 视觉回归 | Task 3（`read_pixels`）+ Task 9（基准与规矩） |
| §14.3 模糊测试入口（图集元数据） | Task 5 |
| §14.6 AAA 结构、行为化测试名 | 全部测试 |
| 交接清单 三处平台层接口改动 | Task 1 |
| 交接清单 Y 排序稳定全序 | Task 4 |
| 交接清单 帧预算节流 | Task 1 |
| 交接清单 死代码清理 | Task 1 |
| 交接清单 浮点不得回流世界状态 | 全局约束 + Task 8（整数帧号） |

### 有意留给后续阶段的缺口（非遗漏）

- **`cargo-fuzz` 正式接入**仍未做。Task 5 用属性测试守住了「任意输入不崩溃」这条底线；正式 fuzz 目标随 P4 的 mod 清单解析、P5 的存档反序列化一并建立，届时才有足够多的入口值得配一套 fuzz 基础设施。
- **视觉回归的 CI 自动比对**未接入。Task 9 只建立基准与人工比对入口；在 CI 上跑需要无头 GPU（lavapipe 或 WARP），验证成本较高，放在 P1 收尾后单独处理。
- **`cargo-llvm-cov` 与 `cargo-mutants`** 仍未接入 CI，理由同 P0：先把双平台矩阵与许可证门禁跑稳。P2 接入。
- **后处理效果**（昼夜染色、天气）不在本阶段。两级渲染结构已为它预留插入点，P2 做昼夜时在 blit 前加一道 pass 即可。
- **文本与字体渲染**属 `ll-text`，不在 P1。
- **子格插值**（角色在两格之间平滑移动）不在 P1。P1 只做整格对齐绘制；插值涉及浮点，接入时必须遵守交接清单第二节的纪律：插值结果只用于本帧绘制，绝不回写世界状态。

### 类型一致性核对

`FrameId` / `Footprint` / `SpriteSize` / `Pivot` / `Layer` / `DrawOrder` / `TILE_SIZE` / `LOGICAL_WIDTH` / `LOGICAL_HEIGHT` / `Viewport` / `RenderTarget` / `GpuContext` / `RenderError` / `FrameRect` / `AtlasEntry` / `AtlasMetadata` / `Atlas` / `Camera` / `SpriteInstance` / `SpriteBatch` / `Clip` / `Playback` 的命名与签名在 Task 9 的验收 demo 中被完整串联调用，与各自定义处一致。
