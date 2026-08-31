//! `app::gpu`：管住这一帧的 GPU 侧家当：图集贴图的装载与键解析、表面获取、上屏。
//!
//! 本模块由 [`crate::app`] 按职责拆出（批次 16，纯搬移，没有改动任何逻辑）。
//! 拆分的依据不是行数而是「下一批要往哪里加东西」：对话批次要加一块屏、
//! UI 布局批次要改 HUD，两批原先撞在同一个文件的同两个函数上。主循环
//! （`impl AppHandler for Demo`）与 `Demo` 自身的状态仍然在 [`crate::app`]。

use std::sync::Arc;

use ll_mod::asset_vfs::AssetVfs;
use ll_platform::config::{DisplayConfig, ScaleFilter};
use ll_platform::window::{PhysicalSize, Window};
use ll_render::atlas::{Atlas, AtlasEntry};
use ll_render::atlas_pack::{SpriteSource, pack_atlas};
use ll_render::batch::SpriteBatch;
use ll_render::gpu::GpuContext;
use ll_render::target::{BlitFilter, RenderTarget, fit_viewport};
use ll_render::wgpu;
use ll_text::TextRenderer;
use ll_ui::widget::quad::QuadRenderer;
use ll_ui::widget::skin::NineSliceSkin;
use ll_ui::widget::textured_quad::TexturedQuadRenderer;

use crate::atlas_miss::{ChainOutcome, MissLedger};

/// 把资产 VFS 已经解析完覆盖规则的精灵声明，逐个从磁盘读出图片字节，
/// 转换成 `ll_render::atlas_pack::pack_atlas` 需要的输入。
///
/// 单个精灵文件读不到（mod 作者声明的路径写错、文件被误删）只跳过
/// 那一条并记警告日志，不让整个图集打包失败——`pack_atlas` 自己对
/// 「读到了字节但解码失败」这一层已经做了同样的降级（见其模块文档
/// 「打包失败必须优雅」一节），这里补的是更前一步「连字节都读不到」
/// 的同一条降级路径。
///
/// # 为什么是 `pub`
///
/// `crates/ll-game/tests/surface_render.rs`（地表内容渲染的端到端验收）
/// 要在没有 GPU 的进程里，用**与真实游戏完全同一段代码**把真实
/// `assets/` + `mods/` 打成图集，再去问「这个图集键真的对应一张画了
/// 东西的图吗」。若那条验收自己另写一份读盘逻辑，它验的就不再是生产
/// 路径——ADR 0018 要的正是「经真实内容、走真实路径」的证据。本函数
/// 本身与 GPU 无关（只读文件、拼结构体），公开它不会把任何 GPU 状态
/// 泄漏出去。
pub fn load_sprite_sources(vfs: &AssetVfs) -> Vec<SpriteSource> {
    vfs.sprites
        .iter()
        .filter_map(|sprite| match std::fs::read(&sprite.source_file) {
            Ok(image_bytes) => Some(SpriteSource {
                name: sprite.atlas_name.clone(),
                image_bytes,
                pivot: ll_render::sprite::Pivot {
                    x: sprite.pivot.x,
                    y: sprite.pivot.y,
                },
                footprint: ll_render::sprite::Footprint {
                    width: sprite.footprint.width,
                    height: sprite.footprint.height,
                },
            }),
            Err(error) => {
                tracing::warn!(
                    name = %sprite.atlas_name,
                    path = %sprite.source_file.display(),
                    %error,
                    "精灵源文件读取失败，已跳过并降级"
                );
                None
            }
        })
        .collect()
}

/// 存活于 `on_resume` 之后的 GPU 相关资源——不能在 `Demo::new` 阶段
/// 就创建：窗口句柄要等 `on_resume` 才可用。
pub(super) struct GpuResources {
    pub(super) gpu: GpuContext,
    pub(super) render_target: RenderTarget,
    pub(super) atlas: Atlas,
    pub(super) batch: SpriteBatch,
    pub(super) window_size: PhysicalSize<u32>,
    /// 离屏画面放大到窗口时的采样滤波方式，来自
    /// [`crate::run_game`] 装载的 [`DisplayConfig::scale_filter`]，
    /// 只读一份贯穿整个运行期——切换滤波方式需要重启（P7 之前没有
    /// 设置界面，见规格 §15），不是本体二进制现在要支持的场景。
    pub(super) blit_filter: BlitFilter,
    /// HUD 文本渲染器（P7 第一批：只读观测界面）——与世界层
    /// `render_target`/`batch` 是完全独立的第二条渲染通道，直接画在
    /// 窗口 surface 的原生分辨率上，见 `ll_text` crate 顶层文档「两条
    /// 渲染通道」一节与 `ll_ui::widget::quad` 模块文档。
    pub(super) text_renderer: TextRenderer,
    /// HUD 面板背景/经验条渲染器——与 `text_renderer` 同一条通道，见
    /// `ll_ui::widget::quad::QuadRenderer` 文档。
    pub(super) quad_renderer: QuadRenderer,
    /// HUD 真实贴图（九宫格边框/条形）渲染器——采样与 `batch` 同一份
    /// `atlas`，见 `ll_ui::widget::textured_quad::TexturedQuadRenderer`
    /// 文档「与 SpriteBatch 的关系」一节。
    pub(super) textured_quad_renderer: TexturedQuadRenderer,
    /// HUD 皮肤——引用 `atlas` 里 `ll-artgen` 生成的占位 UI 贴图,见
    /// `ll_ui::widget::skin::NineSliceSkin` 文档。构造好之后不依赖任何
    /// 运行期状态,贯穿整个会话复用同一份。
    pub(super) skin: NineSliceSkin,
    /// 「整条候选链都没命中」的去重账本，见 [`crate::atlas_miss`]。
    ///
    /// 它必须是**长寿**的（跟着 GPU 资源走完整个会话），不是每帧现造
    /// 一个——每帧现造等于每帧都是「第一次」，去重就完全不存在了，
    /// 日志会退回刷屏。
    pub(super) miss_ledger: MissLedger,
}

/// 这个图集键**在不在**——完全静默的存在性探测，「什么叫查得到」这条
/// 判据的唯一一份。
///
/// 返回类型是 `bool` 而不是条目本身，正是为了让它拿不到、也不可能顺手
/// 打出一条日志：回退链的中间候选未命中、以及压制键
/// （[`SurfaceDraw::superseded_by`](crate::surface_draw::SurfaceDraw::superseded_by)）不存在，都是**正常工作方式**，
/// 见 [`crate::atlas_miss`] 模块文档。
///
/// 写成自由函数而不是 `GpuResources` 的方法：两个调用点
/// （[`GpuResources::resolve_key`] 与 [`push_surface_draw`](super::surface::push_surface_draw)）都必须在
/// 可变借用去重账本的同时共享借用图集，方法形式的 `self.contains(..)`
/// 会借走整个 `GpuResources`。
pub(super) fn atlas_contains(atlas: &Atlas, name: &str) -> bool {
    atlas.metadata().lookup(name).is_some() && atlas.uv_rect(name).is_some()
}

impl GpuResources {
    /// `asset_vfs` 是本次装载会话已经解析完覆盖规则的资产清单
    /// （[`LoadedContent::asset_vfs`](crate::content::LoadedContent::asset_vfs)）——图集不再是编译期
    /// `include_bytes!` 烧死的单一 PNG，而是运行期从本体与全部 mod
    /// 的松散贴图现场打包，见 `ll_render::atlas_pack` 模块文档「为什么
    /// 本体资产也要走这条路径」一节。
    pub(super) fn new(
        window: Arc<Window>,
        size: PhysicalSize<u32>,
        display: DisplayConfig,
        asset_vfs: &AssetVfs,
    ) -> GpuResources {
        let gpu =
            GpuContext::new(window, size, display.vsync).expect("运行环境应能取得可用的图形适配器");
        let render_target = RenderTarget::new(&gpu);
        let sources = load_sprite_sources(asset_vfs);
        tracing::info!(sprite_count = sources.len(), "开始运行期图集打包");
        let packed = pack_atlas(&sources);
        let atlas = Atlas::from_rgba(&gpu, packed.metadata, packed.canvas)
            .expect("运行期打包的图集画布应能上传为 GPU 纹理");
        let batch = SpriteBatch::new(&gpu, &atlas, render_target.format());
        // HUD 两条子通道都画在窗口 surface 的原生分辨率上（不是
        // `render_target` 的 640×360 逻辑分辨率），格式必须是
        // `gpu.surface_format()`，不是 `render_target.format()`——
        // 两者当前多数环境下相同，但语义上前者才是「最终真正呈现的
        // 那张纹理」的格式，见 `ll_text::TextRenderer::new` 文档。
        let text_renderer = TextRenderer::new(gpu.device(), gpu.queue(), gpu.surface_format())
            .expect("内置字体资产应能正常装载");
        let quad_renderer = QuadRenderer::new(gpu.device(), gpu.queue(), gpu.surface_format());
        // 与 `text_renderer`/`quad_renderer` 同一条「原生分辨率、blit
        // 之后」通道,但采样的是 `atlas`——`NineSliceSkin::new` 在这里
        // 一次性查出全部需要的贴图 UV,之后每帧只是克隆已经查好的数据,
        // 不重复查图集。
        let textured_quad_renderer =
            TexturedQuadRenderer::new(gpu.device(), gpu.queue(), gpu.surface_format(), &atlas);
        let skin = NineSliceSkin::new(&atlas);
        GpuResources {
            gpu,
            render_target,
            atlas,
            batch,
            window_size: size,
            blit_filter: match display.scale_filter {
                ScaleFilter::Nearest => BlitFilter::Nearest,
                ScaleFilter::SharpBilinear => BlitFilter::SharpBilinear,
            },
            text_renderer,
            quad_renderer,
            textured_quad_renderer,
            skin,
            miss_ledger: MissLedger::new(),
        }
    }

    pub(super) fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.window_size = size;
    }

    /// 取得本帧窗口 surface 纹理并把世界层（离屏 `render_target`）
    /// blit 上去——不在这一步就 `present`，留出空档让调用方在
    /// [`Demo::on_frame`](ll_platform::window::AppHandler::on_frame) 里追加 HUD 这第二条渲染通道（[`draw_hud`](super::hud_draw::draw_hud)），
    /// 再调用 [`Self::present_frame`] 真正提交。取不到可用 surface 帧时
    /// 返回 `None`，本帧直接跳过呈现（既有降级行为，只是从「一步做完」
    /// 拆成了两步）。
    pub(super) fn acquire_and_blit(&self) -> Option<(wgpu::SurfaceTexture, wgpu::TextureView)> {
        let frame = match self.gpu.acquire_frame() {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, "跳过本帧的窗口呈现");
                return None;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let viewport = fit_viewport(self.window_size.width, self.window_size.height);
        self.render_target
            .blit_to(&self.gpu, &view, viewport, self.blit_filter);
        Some((frame, view))
    }

    /// 真正把已经画好（世界层 + HUD）的 `frame` 提交呈现。
    pub(super) fn present_frame(&self, frame: wgpu::SurfaceTexture) {
        self.gpu.queue().present(frame);
    }

    /// 取一个**已经确认存在**的条目，静默；查不到返回 `None` 而不说话。
    ///
    /// 「说不说话」的决定全部收在 [`Self::resolve_key`] 里，本方法只负责
    /// 把两张表（元数据与 uv）查出来拼在一起。
    pub(super) fn entry_of<'a>(&'a self, name: &str) -> Option<(&'a AtlasEntry, [f32; 4])> {
        match (self.atlas.metadata().lookup(name), self.atlas.uv_rect(name)) {
            (Some(entry), Some(uv)) => Some((entry, uv)),
            _ => None,
        }
    }

    /// 按 `names` 给出的优先级挑出**第一个真的在图集里**的键；全部落空
    /// 时按去重策略留一条 `WARN`，同一组候选整个进程只留一次。
    ///
    /// # 为什么返回键名而不是条目
    ///
    /// 本方法要写账本，所以必须持 `&mut self`；而调用方拿到条目之后
    /// 紧接着就要以 `&mut` 使用 `GpuResources`（把精灵推进批次）。返回
    /// 一个从 `&mut self` 借出来的 `&AtlasEntry` 会让那次推批次借不到
    /// 可变引用。返回的 `&'n str` 借的是**调用方的候选表**，与 `self`
    /// 无关，可变借用因此在本方法返回时就结束了——调用方接着调
    /// [`Self::entry_of`]（`&self`）拿条目，借用形状与本次改动之前逐字
    /// 相同。
    ///
    /// # 日志级别为什么是 `WARN`
    ///
    /// 项目所有者裁定「这一类改成 Warning」。独立于裁定也成立：一条
    /// 落空的候选链可能是一层**本来就可选**的内容
    /// （[`SurfaceDraw::fallback_key`](crate::surface_draw::SurfaceDraw::fallback_key) 为 `None`，例如没有挂件贴图的
    /// 职业——那个字段的文档明写这是正常状态）。把正常状态记成 `ERROR`，
    /// `ERROR` 这个级别就再也不能表示「真出事了」。
    pub(super) fn resolve_key<'n>(
        &mut self,
        names: impl IntoIterator<Item = &'n str>,
    ) -> Option<&'n str> {
        // 显式借出这两个字段：账本要可变借用、图集要共享借用，两者是
        // `GpuResources` 上不相干的字段。探测判据走 [`atlas_contains`]
        // 而不是在这里再写一遍——它与 [`Self::contains`] 是同一份。
        let atlas = &self.atlas;
        match self
            .miss_ledger
            .resolve(names, |key| atlas_contains(atlas, key))
        {
            ChainOutcome::Hit { key, .. } => Some(key),
            ChainOutcome::MissedFirstTime { candidates } => {
                // 整条候选链一个都没命中，这一层这一帧真的没画出来。
                // **只有这一个分支会说话**，而且同一组候选只说一次。
                tracing::warn!(
                    %candidates,
                    "整条图集候选链都没有命中，这一层没有画出来（同一组候选只报一次）"
                );
                None
            }
            ChainOutcome::MissedAgain => None,
        }
    }
}
