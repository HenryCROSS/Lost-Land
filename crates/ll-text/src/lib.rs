//! 迷途大陆的文本渲染地基。
//!
//! # 只做地基，不做控件库
//!
//! 本 crate 只解决「文字能不能画出来」这一层：字体加载与注册
//! （[`fonts`]）、排版与断行（[`layout`]）、栅格化上屏（[`render`]）。
//! 九宫格切片边框、焦点导航、菜单/设置控件属于 P6 的完整像素 UI 控件库，
//! 不在本 crate 范围内。
//!
//! # 两条渲染通道，本 crate 只服务其中一条
//!
//! `ll-render` 现有的 [`ll_render::target::RenderTarget`] 是「640×360
//! 逻辑分辨率 + 整数倍放大」的世界层管线，服务像素美术。文本层刻意
//! **不复用这条管线**：[`render::TextRenderer::render`] 直接对调用方
//! 传入的 `wgpu::TextureView` 与其**原生像素尺寸**画字形，不经过
//! `RenderTarget`、不引用 [`ll_render::target::LOGICAL_WIDTH`]/
//! [`ll_render::target::LOGICAL_HEIGHT`]、不调用 `fit_viewport`——12×12
//! 像素方框下笔画多的汉字会糊成一团，栅格化这一步本身必须发生在原生
//! 分辨率，整数放大只会把糊了的像素等比例放大。详细论证见
//! `knowledge/pipelines/text-and-font-rendering.md` 第 1.2 节。
//!
//! 调用方通常的组织方式：先用 `RenderTarget::blit_to` 把世界层画到窗口
//! surface，再用 `TextRenderer::render` 对**同一张** surface 纹理视图
//! 追加一道 `LoadOp::Load`（不清屏）的渲染 pass 画文字——两条通道各自
//! 独立的渲染 pass，只是先后画到同一张目的纹理上，没有共享中间产物。
//!
//! # 浮点边界
//!
//! 排版结果（断行位置、字形前进宽度）是浮点且依赖字体/库版本，**这些
//! 值只用于当前帧的屏幕绘制，绝不可回流入 `ll-world`/`ll-sim` 的世界
//! 状态**——文本的语义内容（来自 Fluent 翻译键+变量插值）才需要确定性，
//! 「画出来长什么样」从来不需要跨平台逐位一致。这与 [ADR
//! 0002](../../../docs/superpowers/decisions/0002-integer-only-world-state.md)
//! 划定的「浮点只许在渲染层」是一致的。本 crate 的公开 API 不接收也不
//! 产出任何会被存档序列化的类型，这条边界因此不需要运行时校验，是
//! crate 边界本身保证的。

pub mod error;
pub mod fonts;
pub mod layout;
pub mod render;

pub use error::TextError;
pub use fonts::FontCatalog;
pub use layout::{GlyphOrigin, LayoutGlyphInfo, LayoutLineInfo, LayoutResult};
pub use render::{TextRenderer, TextRun};
