//! 游戏本体的 [`AppHandler`] 实现：把 [`crate::content`]/[`crate::world`]/
//! [`crate::save`] 接到窗口事件循环上——启动 → 装载内容 → 建世界/读档
//! 已经在 [`crate::run_game`] 完成，本模块只负责「每帧输入 → 世界推进 →
//! 渲染」与「退出前存档」。
//!
//! 渲染管线（图集加载、精灵批、相机、FOV 裁剪可见格）直接复用
//! `ll-render` 已经交付的部件，取舍与 `ll-sim` 的
//! `p5_coordinate_acceptance` 完全一致（同一批零件），差异只在本模块
//! 更薄——不做 Interior 出入、不做行走/待机触发式动画状态机、不画
//! 小地图（规格 §15 把这类打磨排在 P7，见任务顶层说明「不是做 UI
//! 项目」），聚焦「能玩、能存」这条最小闭环本身。

use std::path::PathBuf;
use std::sync::Arc;

use ll_platform::input::InputState;
use ll_platform::window::{AppHandler, FrameId, FrameOutcome, PhysicalSize, Window};
use ll_render::atlas::{Atlas, AtlasEntry, AtlasMetadata};
use ll_render::batch::{SpriteBatch, SpriteInstance};
use ll_render::camera::Camera;
use ll_render::gpu::GpuContext;
use ll_render::sprite::{DrawOrder, Layer, footprint_bottom_screen_y, sprite_draw_position};
use ll_render::target::{RenderTarget, fit_viewport};
use ll_render::wgpu;
use ll_sim::apply::apply;
use ll_sim::intent::intent_from_input;
use ll_sim::resolve::resolve;
use ll_world::fov::compute_fov;
use ll_world::space::Space;
use ll_world::surface_store::SurfaceWindow;

use crate::content::LoadedContent;
use crate::layout::{effective_sight_radius, effective_tint, terrain_entry_name};
use crate::save::save_game;
use crate::world::{GameWorld, STREAM_RADIUS_ZONES};

/// 玩家标记在绘制顺序里固定的实体号。
const PLAYER_ENTITY: u64 = 0;
/// 地形瓦片绘制顺序号的起始偏移。
const TERRAIN_ENTITY_BASE: u64 = 1;
/// 玩家精灵唯一必须存在的一帧——本体图集恒定内嵌它，见
/// `ll-sim` 的 `p5_coordinate_acceptance::FALLBACK_SPRITE` 同一取舍。
const PLAYER_SPRITE: &str = "hero_idle_0";

const ATLAS_JSON: &str = include_str!("../../../assets/atlas/placeholder.json");
const ATLAS_PNG: &[u8] = include_bytes!("../../../assets/atlas/placeholder.png");

/// 存活于 `on_resume` 之后的 GPU 相关资源——不能在 `Demo::new` 阶段
/// 就创建：窗口句柄要等 `on_resume` 才可用。
struct GpuResources {
    gpu: GpuContext,
    render_target: RenderTarget,
    atlas: Atlas,
    batch: SpriteBatch,
    window_size: PhysicalSize<u32>,
}

impl GpuResources {
    fn new(window: Arc<Window>, size: PhysicalSize<u32>) -> GpuResources {
        let gpu = GpuContext::new(window, size).expect("运行环境应能取得可用的图形适配器");
        let render_target = RenderTarget::new(&gpu);
        let metadata = AtlasMetadata::parse(ATLAS_JSON).expect("内嵌图集元数据应为合法 JSON");
        let atlas =
            Atlas::load(&gpu, metadata, ATLAS_PNG).expect("内嵌图集资源应能上传为 GPU 纹理");
        let batch = SpriteBatch::new(&gpu, &atlas, render_target.format());
        GpuResources {
            gpu,
            render_target,
            atlas,
            batch,
            window_size: size,
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
        self.window_size = size;
    }

    fn present(&self) {
        let frame = match self.gpu.acquire_frame() {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, "跳过本帧的窗口呈现");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let viewport = fit_viewport(self.window_size.width, self.window_size.height);
        self.render_target.blit_to(&self.gpu, &view, viewport);
        self.gpu.queue().present(frame);
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

/// 游戏本体的完整运行期状态。
pub struct Demo {
    content: LoadedContent,
    game_world: GameWorld,
    camera: Camera,
    save_path: PathBuf,
    character_name: String,
    resources: Option<GpuResources>,
}

impl Demo {
    /// 用已经装载好的内容与已经建好（新游戏或读档得来）的世界构造
    /// 运行期状态——两者都由 [`crate::run_game`] 在事件循环启动前准备好，
    /// 本类型不负责「建世界还是读档」这个决定本身。
    pub fn new(
        content: LoadedContent,
        game_world: GameWorld,
        save_path: PathBuf,
        character_name: String,
    ) -> Demo {
        let player_pos = game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家刚生成或刚读档，必然存在")
            .pos;
        let camera = Camera {
            center: player_pos,
            world: game_world.world.size,
        };
        Demo {
            content,
            game_world,
            camera,
            save_path,
            character_name,
            resources: None,
        }
    }

    /// 每帧输入处理：先维护流式邻域（必须排在移动之前，见
    /// `ll_world::surface_store::SurfaceStore::stream_neighborhood`
    /// 文档），再处理移动。
    fn advance(&mut self, input: &InputState) {
        self.maintain_streaming();

        let player = self.game_world.player;
        let intent = intent_from_input(player, input);
        if let Some(intent) = intent {
            let effects = resolve(&self.game_world.world, &intent);
            for effect in &effects {
                apply(&mut self.game_world.world, effect);
            }
        }

        if let Some(agent) = self.game_world.world.actors.get(player)
            && matches!(agent.current_space, Space::Surface { .. })
        {
            self.camera.center = agent.pos;
        }
    }

    fn maintain_streaming(&mut self) {
        let player = self.game_world.player;
        let Some(agent) = self.game_world.world.actors.get(player) else {
            return;
        };
        if !matches!(agent.current_space, Space::Surface { .. }) {
            return;
        }
        let pos = agent.pos;
        let clock = self.game_world.world.clock;
        self.game_world.world.terrain.stream_neighborhood(
            &self.game_world.noise,
            &self.game_world.params,
            &self.content.terrain_ids,
            pos,
            STREAM_RADIUS_ZONES,
            clock,
        );
    }

    /// 退出前存档——`on_exit` 恰好调用一次（`ll_platform::window`
    /// 文档保证），是「游玩 → 存档 → 退出」这条闭环里存档动作唯一的
    /// 触发点。
    fn save_on_exit(&self) {
        match save_game(
            &self.save_path,
            &self.content,
            &self.game_world,
            &self.character_name,
            "旷野",
            ll_content::mode::SaveMode::Permadeath,
        ) {
            Ok(()) => tracing::info!(path = %self.save_path.display(), "退出前存档完成"),
            Err(error) => tracing::error!(%error, "退出前存档失败"),
        }
    }
}

/// 把地表世界画到离屏目标：地形（仅可见格）+ 玩家标记。
fn render_surface(
    game_world: &GameWorld,
    content: &LoadedContent,
    camera: &Camera,
    resources: &mut GpuResources,
) {
    let world = &game_world.world;
    let player_pos = world
        .actors
        .get(game_world.player)
        .map(|agent| agent.pos)
        .unwrap_or(camera.center);
    let profile = space_profile_of(content, world.surface_profile);
    let clock = world.clock;
    let radius = effective_sight_radius(&profile, clock);
    let tint = effective_tint(&profile, clock);

    let visible = compute_fov(
        &SurfaceWindow::new(&world.terrain),
        &world.terrain_table,
        player_pos,
        radius,
    );

    let world_width = world.size.width() as u64;
    for pos in camera.visible_tiles() {
        if !visible.contains(pos) {
            continue;
        }
        let Some(kind) = world.terrain_at(pos) else {
            continue;
        };
        let Some(name) = terrain_entry_name(kind, &content.terrain_ids) else {
            continue;
        };
        let Some((entry, uv)) = resources.lookup(name) else {
            continue;
        };
        let (sx, sy) = camera.world_to_screen(pos);
        let order = DrawOrder::new(
            Layer::TERRAIN,
            sy,
            TERRAIN_ENTITY_BASE + pos.y() as u64 * world_width + pos.x() as u64,
        );
        resources.batch.push(
            order,
            sprite_instance(sx as f32, sy as f32, entry.sprite_size(), uv, tint),
        );
    }

    let (px, py) = camera.world_to_screen(player_pos);
    push_player_marker(px, py, tint, resources);
}

/// 空间的完整 [`ll_world::space_profile::SpaceProfile`]——`Space` 本身
/// 只携带一个索引，渲染层需要的是完整字段，从装载好的空间层属性表
/// 现查现拼，不缓存（理由与 `p5_coordinate_acceptance::DemoWorld::profile_of`
/// 一致）。
fn space_profile_of(
    content: &LoadedContent,
    index: ll_core::ident::ContentIndex,
) -> ll_world::space_profile::SpaceProfile {
    ll_world::space_profile::SpaceProfile {
        id: ll_core::ident::NamespacedId::parse("lostland:runtime_profile").expect("字面量恒合法"),
        ambient_light_floor: content.space_table.ambient_light_floor(index),
        exposed_to_sky: content.space_table.exposed_to_sky(index),
        base_temperature: content.space_table.base_temperature(index),
        diggable: content.space_table.diggable(index),
        buildable: content.space_table.buildable(index),
        reverb_tag: content.space_table.reverb_tag(index),
    }
}

fn push_player_marker(sx: i32, sy: i32, tint: [f32; 4], resources: &mut GpuResources) {
    let Some((entry, uv)) = resources.lookup(PLAYER_SPRITE) else {
        return;
    };
    let footprint = entry.footprint;
    let [px, py] = sprite_draw_position((sx, sy), footprint, entry.pivot);
    let order = DrawOrder::new(
        Layer::ENTITY,
        footprint_bottom_screen_y(sy, footprint.height),
        PLAYER_ENTITY,
    );
    resources.batch.push(
        order,
        sprite_instance(px, py, entry.sprite_size(), uv, tint),
    );
}

fn sprite_instance(
    x: f32,
    y: f32,
    size: ll_render::sprite::SpriteSize,
    uv_rect: [f32; 4],
    color: [f32; 4],
) -> SpriteInstance {
    SpriteInstance {
        position: [x, y],
        size: [size.width as f32, size.height as f32],
        uv_rect,
        color,
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
        if input.was_just_pressed(ll_platform::input::GameKey::Cancel) {
            return FrameOutcome::Exit;
        }

        self.advance(input);

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        render_surface(&self.game_world, &self.content, &self.camera, resources);

        resources
            .batch
            .flush(&resources.gpu, resources.render_target.view());
        resources.present();

        FrameOutcome::Continue
    }

    fn on_exit(&mut self) {
        tracing::info!("demo exiting");
        self.save_on_exit();
    }
}
