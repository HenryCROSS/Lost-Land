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

pub mod anim;
pub mod atlas;
pub mod atlas_pack;
pub mod batch;
pub mod camera;
pub mod gpu;
pub mod sprite;
pub mod target;

// 向上层重新导出 wgpu。
//
// 本 crate 的公开 API 已经全是 wgpu 类型（`GpuContext::acquire_frame`
// 返回 `wgpu::SurfaceTexture`、`RenderTarget::view` 返回
// `&wgpu::TextureView`、`SpriteBatch::new` 接收 `wgpu::TextureFormat`），
// 不重新导出就等于要求每个下游自己声明一份 `wgpu = "30"` 依赖，还得
// 自己保证版本号与这里用的完全一致——版本一旦漂移，这些类型在下游看来
// 就是「不同的类型」，编译直接报错。这与 `ll-platform` 为守住「只有
// 平台层接触窗口库」这条分层承诺而重新导出 `Window`/`PhysicalSize`
// 是同一个问题的另一面：分层承诺不能以「下游必须偷偷自带同版本依赖」
// 为代价维持。
pub use wgpu;

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
    /// 取得当前可呈现的 surface 帧失败（重新配置并重试一次之后仍然失败，
    /// 或遇到 `Timeout`/`Occluded`/`Validation` 这类调用方应当跳过本帧
    /// 或直接视为故障的情形）。见 [`gpu::GpuContext::acquire_frame`]。
    SurfaceAcquire(String),
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
            RenderError::SurfaceAcquire(why) => write!(f, "failed to acquire surface frame: {why}"),
            RenderError::AtlasDecode(why) => write!(f, "failed to decode atlas image: {why}"),
            RenderError::AtlasMetadata(why) => write!(f, "invalid atlas metadata: {why}"),
        }
    }
}

impl core::error::Error for RenderError {}
