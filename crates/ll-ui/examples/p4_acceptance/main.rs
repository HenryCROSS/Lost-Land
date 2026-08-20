//! P4 验收 demo：证明 mod 框架真的接通了——一个真实的 mod 目录被发现、
//! 解析、拓扑排序、脚本加载、内容注册，注册出的地形出现在游戏里、能
//! 走上去、属性生效；本体地形与 mod 地形除命名空间外在注册表里结构
//! 相同；三种故意写错的 mod（语法错误、白名单拒绝、缺失依赖，外加
//! 一个附加项：重复命名空间）被捕获、进程存活、错误显示在加载管理
//! 界面里；界面文字用思源黑体渲染（`ll-text`，任务 10）。
//!
//! # 完整调用链
//!
//! `mods/example_mod/{mod.toml,terrain.scm}`
//!   → [`ll_mod::pipeline::load_all`]（内部依次调用 discover_mods →
//!     parse_manifest → topo_sort → ScriptEngine::load_source →
//!     register-terrain → Registry::intern/TerrainTable::define）
//!   → [`crate::world::build_demo_world`] 把结果接进 [`WorldState`]，
//!     手动铺一小片熔岩地板紧挨着玩家出生点
//!   → 每帧：[`ll_sim::intent::intent_from_input`] → resolve → apply
//!     推进玩家移动，[`ll_ui::load_report_view`] 把加载报告画成文字
//!     叠加在窗口 surface 上。
//!
//! 运行：`cargo run -p ll-ui --example p4_acceptance`
//! 操作：方向键/WASD 移动，Tab 展开/折叠全部失败详情，`.`（Wait 键）
//! 对 examplemod 触发一次一键重载演示，F2 存世界层基准 PNG，Esc 退出。

mod layout;
mod png;
mod render;
mod world;

use std::collections::HashSet;
use std::sync::Arc;

use ll_core::ident::NamespacedId;
use ll_mod::load_report::LoadStatus;
use ll_mod::pipeline::reload_mod;
use ll_platform::input::{GameKey, InputState};
use ll_platform::logging::init_logging;
use ll_platform::window::{
    AppHandler, FrameId, FrameOutcome, PhysicalSize, Window, WindowConfig, run,
};
use ll_render::atlas::{Atlas, AtlasEntry, AtlasMetadata};
use ll_render::batch::SpriteBatch;
use ll_render::camera::Camera;
use ll_render::gpu::GpuContext;
use ll_render::target::{RenderTarget, fit_viewport};
use ll_render::wgpu;
use ll_sim::apply::apply;
use ll_sim::intent::intent_from_input;
use ll_sim::resolve::resolve;
use ll_text::TextRenderer;
use ll_ui::load_report_view::render_load_report;
use png::save_baseline_png;
use render::{push_player, push_terrain};
use world::{DemoWorld, build_demo_world};

/// 图集元数据 JSON，编译期内嵌——沿用既有 demo 共享的占位图集，理由
/// 与 p1/p2/p3_acceptance 一致：demo 之间没有理由各自维护一份美术
/// 资产。
const ATLAS_JSON: &str = include_str!("../../../../assets/atlas/placeholder.json");
/// 图集图片字节，编译期内嵌，理由同上。
const ATLAS_PNG: &[u8] = include_bytes!("../../../../assets/atlas/placeholder.png");

/// 视觉回归基准 PNG 的落盘路径。
const BASELINE_PNG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/visual/baseline/p4_acceptance.png"
);

/// 加载管理界面面板左上角留白（像素，原生分辨率）。
const PANEL_ORIGIN: (f32, f32) = (12.0, 12.0);

/// 一键重载提示的固定纵坐标（像素，原生分辨率）——报告面板最多
/// 「已加载」1 行 + 标题、「有警告」1 行标题、「失败」标题 + 最多 5×3
/// 展开行 + 交叉引用校验 1 行，留足空间避免正常情况下与面板重叠。
const RELOAD_HINT_Y: f32 = 460.0;

/// 存活于 `on_resume` 之后的 GPU 相关资源。
pub(crate) struct GpuResources {
    gpu: GpuContext,
    render_target: RenderTarget,
    atlas: Atlas,
    batch: SpriteBatch,
    text_renderer: TextRenderer,
    window_size: PhysicalSize<u32>,
}

impl GpuResources {
    fn new(window: Arc<Window>, size: PhysicalSize<u32>) -> GpuResources {
        let gpu = GpuContext::new(window, size).expect("demo 环境应能取得可用的图形适配器");
        let render_target = RenderTarget::new(&gpu);

        let metadata = AtlasMetadata::parse(ATLAS_JSON).expect("内嵌图集元数据应为合法 JSON");
        let atlas = Atlas::load(&gpu, metadata, ATLAS_PNG).expect("内嵌图集应能上传为 GPU 纹理");
        let batch = SpriteBatch::new(&gpu, &atlas, render_target.format());

        let text_renderer = TextRenderer::new(gpu.device(), gpu.queue(), gpu.surface_format())
            .expect("内置字体资产应能正常加载");

        GpuResources {
            gpu,
            render_target,
            atlas,
            batch,
            text_renderer,
            window_size: size,
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.window_size = size;
    }

    fn lookup<'a>(&'a self, name: &str) -> Option<(&'a AtlasEntry, [f32; 4])> {
        let entry = self.atlas.metadata().lookup(name);
        let uv = self.atlas.uv_rect(name);
        match (entry, uv) {
            (Some(entry), Some(uv)) => Some((entry, uv)),
            _ => {
                tracing::error!(name, "图集条目缺失，跳过本次绘制");
                None
            }
        }
    }
}

/// P4 验收 demo 的完整状态。
struct Demo {
    demo_world: DemoWorld,
    camera: Camera,
    /// 当前是否展开全部失败 mod 的详情——用一个全局开关而不是逐个
    /// 记录每个 mod 各自的展开状态，这是本 demo 的展示取舍（Task 11
    /// 的核心交付物是"能展开"这个机制本身，逐条独立展开/折叠属于真正
    /// 的 UI 控件库才需要打磨的交互细节，P6 范围）。
    show_details: bool,
    /// 上一次「一键重载」演示的结果文案，展示在面板最下方。
    reload_hint: Option<String>,
    resources: Option<GpuResources>,
}

impl Demo {
    fn new() -> Demo {
        let demo_world = build_demo_world();
        let player_pos = demo_world
            .world
            .actors
            .get(demo_world.player)
            .expect("玩家刚生成，必然存在")
            .pos;
        let camera = Camera {
            center: player_pos,
            world: demo_world.world.size,
        };

        Demo {
            demo_world,
            camera,
            show_details: false,
            reload_hint: None,
            resources: None,
        }
    }

    /// 用玩家一帧输入推进移动——直接走 resolve/apply，不经过时间轴：
    /// 本 demo 没有 AI、没有回合调度，P3 已经验收过那条链路，P4 的
    /// 重点是 mod 加载，不需要重新证明一遍。
    fn advance_player(&mut self, input: &InputState) {
        let player = self.demo_world.player;
        let Some(intent) = intent_from_input(player, input) else {
            return;
        };
        let effects = resolve(&self.demo_world.world, &intent);
        for effect in &effects {
            apply(&mut self.demo_world.world, effect);
        }
        if let Some(agent) = self.demo_world.world.actors.get(player) {
            self.camera.center = agent.pos;
        }
    }

    /// 展开/折叠集合：`show_details` 为真时把报告里全部失败条目的
    /// id 都收进去，否则给出空集合。
    fn expanded_ids(&self) -> HashSet<NamespacedId> {
        if !self.show_details {
            return HashSet::new();
        }
        self.demo_world
            .report
            .entries_with(|status| matches!(status, LoadStatus::Failed(_)))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 对 examplemod 触发一次一键重载演示（Task 11「单个 mod 一键
    /// 重载」的最小可行版本，见 `ll_mod::pipeline::reload_mod`
    /// 文档）：重新解析清单、重新跑一遍脚本，把结果写进
    /// `reload_hint` 供面板展示。
    fn reload_example_mod(&mut self) {
        let status = reload_mod(&self.demo_world.example_mod_manifest);
        self.reload_hint = Some(match &status {
            LoadStatus::Loaded => "一键重载 examplemod：成功".to_string(),
            LoadStatus::Warning(msg) => format!("一键重载 examplemod：警告 {msg}"),
            LoadStatus::Failed(err) => format!(
                "一键重载 examplemod：失败（{:?}：{}）",
                err.stage, err.message
            ),
        });
        // `mod_self_id` 是 ll-mod 的 crate 内部帮手（pub(crate)），
        // demo 直接按同一个约定（namespace + ":self"）拼一个等价的
        // NamespacedId，不需要 ll-mod 为此多导出一个公开函数。
        let examplemod_id = NamespacedId::parse("examplemod:self").expect("字面量恒合法");
        self.demo_world.report.replace(&examplemod_id, status);
    }
}

impl AppHandler for Demo {
    fn on_resume(&mut self, window: Arc<Window>, size: PhysicalSize<u32>) {
        tracing::info!(width = size.width, height = size.height, "window resumed");
        self.resources = Some(GpuResources::new(window, size));
    }

    fn on_resize(&mut self, size: PhysicalSize<u32>) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.resize(size);
    }

    fn on_frame(&mut self, _frame: FrameId, input: &InputState) -> FrameOutcome {
        if input.was_just_pressed(GameKey::Cancel) {
            return FrameOutcome::Exit;
        }
        if input.was_just_pressed(GameKey::Menu) {
            self.show_details = !self.show_details;
        }
        if input.was_just_pressed(GameKey::Wait) {
            self.reload_example_mod();
        }

        self.advance_player(input);
        // 必须在借出 `self.resources` 的可变引用之前算好：`expanded_ids`
        // 是个取 `&self` 的方法，若放在下面按字段借用 `resources` 之后
        // 调用，借用检查器没办法从一次方法调用里看出它只读了
        // `self.show_details`/`self.demo_world.report`，会报出并不存在
        // 的借用冲突——与 p1/p2/p3_acceptance「取自由函数而非方法」的
        // 理由是同一件事，这里选择「提前算好」而不是「拆成自由函数」，
        // 因为只有这一处需要，不值得为此单独拆一个函数。
        let expanded = self.expanded_ids();

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        if input.was_just_pressed(GameKey::Screenshot) {
            save_baseline_png(resources, BASELINE_PNG_PATH);
        }

        let player_pos = self
            .demo_world
            .world
            .actors
            .get(self.demo_world.player)
            .map(|agent| agent.pos)
            .unwrap_or(self.camera.center);

        push_terrain(
            &self.demo_world.world,
            &self.demo_world.terrain_ids,
            self.demo_world.lava_kind,
            &self.camera,
            resources,
        );
        push_player(player_pos, &self.camera, resources);
        resources
            .batch
            .flush(&resources.gpu, resources.render_target.view());

        let frame = match resources.gpu.acquire_frame() {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, "跳过本帧的窗口呈现");
                return FrameOutcome::Continue;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let viewport = fit_viewport(resources.window_size.width, resources.window_size.height);
        resources
            .render_target
            .blit_to(&resources.gpu, &view, viewport);

        if let Err(error) = render_load_report(
            &mut resources.text_renderer,
            resources.gpu.device(),
            resources.gpu.queue(),
            &view,
            resources.window_size.width,
            resources.window_size.height,
            &self.demo_world.report,
            &expanded,
            PANEL_ORIGIN,
        ) {
            tracing::error!(%error, "加载管理界面渲染失败");
        }

        // 一键重载演示的结果文案——单独一道不清屏的文字 pass，画在报告
        // 面板下方固定位置。不并进 `render_load_report`：那是 ll-ui 的
        // 公开 API，重载提示纯粹是本 demo 自己的交互反馈，不该污染
        // 库函数的签名。
        if let Some(hint) = &self.reload_hint {
            let run = ll_text::TextRun {
                text: hint,
                x: PANEL_ORIGIN.0,
                y: RELOAD_HINT_Y,
                font_size: ll_ui::load_report_view::DEFAULT_FONT_SIZE,
                line_height: ll_ui::load_report_view::DEFAULT_LINE_HEIGHT,
                max_width: ll_ui::load_report_view::DEFAULT_MAX_WIDTH,
                color: glyphon::Color::rgba(120, 220, 255, 255),
                bold: true,
            };
            if let Err(error) = resources.text_renderer.render(
                resources.gpu.device(),
                resources.gpu.queue(),
                &view,
                resources.window_size.width,
                resources.window_size.height,
                &[run],
            ) {
                tracing::error!(%error, "一键重载提示渲染失败");
            }
        }

        resources.gpu.queue().present(frame);

        FrameOutcome::Continue
    }

    fn on_exit(&mut self) {
        tracing::info!("demo exiting");
    }
}

fn main() {
    init_logging(true).expect("首次初始化日志不应失败");
    tracing::info!(
        "P4 acceptance demo: arrows/WASD move, Tab toggles failed-mod details, \
         '.' reloads examplemod, F2 saves baseline PNG, Esc to quit"
    );

    let demo = Demo::new();
    tracing::info!(
        registry_total = demo.demo_world.report.entries.len(),
        loaded = demo.demo_world.report.loaded_count(),
        failed = demo.demo_world.report.failed_count(),
        "mod loading pipeline finished"
    );
    // P5-C 缺口修补批次：证明脚本注册的不只是地形——mods/example_mod/
    // gameplay.scm 的 register-class 调用是否真的写进了职业表。
    tracing::info!(
        necromancer_primary_attribute = ?demo.demo_world.necromancer_primary_attribute,
        "examplemod:necromancer class registration"
    );

    if let Err(error) = run(WindowConfig::default(), demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}
