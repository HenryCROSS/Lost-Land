//! P3 验收 Demo：证明「按键 → `Intent` → `resolve` → `Effect` → `apply`
//! → 渲染」这条链路真的串起来了——玩家与至少三个各有不同敏捷的敌人在
//! 非线性时间轴上轮流行动，攻击与受击都能在屏幕上看见。这是 P3 阶段
//! 的最后一个任务，验收的是前八个任务是否真的接成了一个整体，而不是
//! 各自测试通过、拼在一起却接不上。
//!
//! 六件事同时发生：
//!
//! 1. 玩家在真实生成的地形上移动，FOV 随移动更新（复用 P2 已验收的
//!    `ll_world::fov::compute_fov`，每帧现算不缓存）。
//! 2. 三个敌人敏捷各不相同（5/10/30，[`ll_world::entity::BaseStats::BASELINE`]
//!    的敏捷是 10），出手频率肉眼可见地不同——时间轴的核心验收点，见
//!    `crate::turn::TurnEngine::advance_ai` 文档。
//! 3. 左上角时间轴侧栏显示接下来几次出手顺序。
//! 4. 攻击与受击：撞向敌人即攻击（`crate::turn::route_player_intent`），
//!    伤害数字以飘字形式短暂显示在受创单位头顶。
//! 5. 敌人头顶显示名字——由 `ll_world::naming::given_name` 现算，验证
//!    该模块的纯函数命名真的能用。
//! 6. 跨南北接缝时遮挡关系正确：`crate::spawn::ENEMY_SEAM_OFFSET` 把
//!    敏捷最高的敌人特意摆在接缝另一侧（见其文档），`DrawOrder` 全程
//!    只认屏幕坐标（Task 1 的修复）。
//!
//! 运行：`cargo run -p ll-sim --example p3_acceptance`
//! 操作：方向键/WASD 移动或攻击（撞向敌人即攻击），`.` 原地等待一回合，
//! F2 存基准 PNG，Esc 退出。
//!
//! # 完整调用链
//!
//! 从按键到出图的每一步都能从上一步取到参数，对应
//! `.superpowers/sdd/2026-08-17-p3-turn-combat/task-9-brief.md` 的自查
//! 表逐项列出：
//!
//! - `InputState::was_activated`/`was_just_pressed` → [`ll_sim::intent::intent_from_input`]
//!   产出 `Option<Intent>`（`crate::turn::TurnEngine::try_player_turn`）。
//! - `crate::turn::route_player_intent` 把「移动到敌人格」路由成
//!   [`ll_sim::intent::Intent::Attack`]。
//! - [`ll_sim::resolve::resolve`]（`&WorldState`、`&Intent` → `Vec<Effect>`）
//!   内部查 `world.terrain`/`world.actors`/`action_cost`。
//! - `for effect in effects { apply(&mut world, &effect) }`
//!   （`crate::turn::TurnEngine::perform`），全程只有这一处写世界。
//! - [`ll_sim::timeline::Timeline::pop_next`] 决定下一个行动者；玩家
//!   之外全部走固定策略 AI（`crate::turn::ai_intent`）。
//! - [`ll_world::fov::compute_fov`] 用玩家当前位置与
//!   `ll_world::light::sight_radius_at` 求出的半径现算视野。
//! - [`ll_render::camera::Camera::world_to_screen`] 换算屏幕坐标，
//!   [`ll_render::sprite::DrawOrder::new`] 用**屏幕**纵坐标排序。
//! - [`ll_render::atlas::Atlas::uv_rect`] 查图集 UV，
//!   [`ll_render::batch::SpriteBatch::push`]/`flush` 一次性提交。
//!
//! # 文件拆分
//!
//! [`layout`] 放不依赖 GPU、也不改动世界数据的纯呈现计算；[`spawn`]
//! 放出生点搜索与战斗单位生成；[`turn`] 放回合引擎（时间轴推进、玩家
//! 输入路由、固定策略 AI）；[`font`] 放极简像素字体与图集扩展；
//! [`png`] 放基准 PNG 落盘；本文件只留 GPU 资源装配、`Demo` 状态与
//! [`ll_platform::window::AppHandler`] 接线——与 `p1_acceptance`/
//! `p2_acceptance` 同样的拆分理由，见各自模块文档。

mod font;
mod layout;
mod png;
mod spawn;
mod turn;

use layout::{
    BASE_SIGHT_RADIUS, ambient_tint, footprint_bottom_screen_y, sprite_draw_position,
    terrain_entry_name,
};
use ll_core::torus::TorusPos;
use ll_platform::input::{GameKey, InputState};
use ll_platform::logging::init_logging;
use ll_platform::window::{
    AppHandler, FrameId, FrameOutcome, PhysicalSize, Window, WindowConfig, run,
};
use ll_render::atlas::{Atlas, AtlasEntry, AtlasMetadata};
use ll_render::batch::{SpriteBatch, SpriteInstance};
use ll_render::camera::Camera;
use ll_render::gpu::GpuContext;
use ll_render::sprite::{DrawOrder, Layer};
use ll_render::target::{RenderTarget, fit_viewport};
// 走 ll_render 重新导出的 wgpu，理由与 p1_acceptance/p2_acceptance 一致。
use ll_render::wgpu;
use ll_sim::timeline::Timeline;
use ll_world::entity::EntityId;
use ll_world::fov::{VisibleSet, compute_fov};
use ll_world::light::{ambient_light, sight_radius_at};
use ll_world::naming::{NamingRules, given_name};
use ll_world::state::WorldState;
use png::save_baseline_png;
use spawn::{Combatant, SpawnedActors, build_world, demo_naming_rules, spawn_actors};
use std::sync::Arc;
use turn::{DamagePopup, TurnEngine, tick_popups};

/// 绘制顺序号：地形瓦片的起始偏移。
const TERRAIN_ENTITY_BASE: u64 = 1;

/// 绘制顺序号：战斗单位精灵的起始偏移，远大于地形瓦片可能用到的最大值
/// （`WORLD_WIDTH * WORLD_HEIGHT + TERRAIN_ENTITY_BASE`），避免撞车——
/// 理由与 `p2_acceptance` 的 `MINIMAP_ENTITY_BASE` 一致：不同图层之间
/// 撞车本身无害（图层是 `DrawOrder` 的第一排序键），分开取值只是方便
/// 调试时按数值段判断来源。`EntityId::as_u64` 本身互不相同（世代索引，
/// 见其文档），直接加这个偏移即可保证与地形瓦片不撞号。
const ACTOR_ENTITY_BASE: u64 = 1_000_000;

/// 名字牌文字缩放：4×6 的字形按 1 倍绘制，悬浮在单位头顶已经够小巧，
/// 不需要放大占用过多视口空间。
const NAME_TEXT_SCALE: f32 = 1.0;

/// 伤害飘字缩放：比名字牌大一倍，突出「受击」这一瞬间的反馈。
const DAMAGE_TEXT_SCALE: f32 = 2.0;

/// 时间轴侧栏文字缩放。
const SIDEBAR_TEXT_SCALE: f32 = 2.0;

/// 时间轴侧栏左上角留白（像素）。
const SIDEBAR_MARGIN_PX: f32 = 4.0;

/// 时间轴侧栏每行高度（像素）：字形高度按缩放换算，再加 2 像素行距。
const SIDEBAR_ROW_HEIGHT_PX: f32 = font::GLYPH_ROWS as f32 * SIDEBAR_TEXT_SCALE + 2.0;

/// 时间轴侧栏显示接下来几次出手顺序。
const SIDEBAR_PREVIEW_COUNT: usize = 5;

/// 伤害飘字随存活时间上升的速度（像素 / 3 帧），营造轻微的浮起动画。
const DAMAGE_POPUP_RISE_PER_3_FRAMES: i32 = 1;

/// 图集元数据 JSON，编译期内嵌，不依赖运行时工作目录——与
/// `p2_acceptance` 引用同一份占位图集，理由见其文档：demo 之间没有
/// 理由各自维护一份美术资产。
const ATLAS_JSON: &str = include_str!("../../../../assets/atlas/placeholder.json");
/// 图集图片字节，编译期内嵌，理由同上。
const ATLAS_PNG: &[u8] = include_bytes!("../../../../assets/atlas/placeholder.png");

/// 视觉回归基准 PNG 的落盘路径。
const BASELINE_PNG_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/visual/baseline/p3_acceptance.png"
);

/// 存活于 `on_resume` 之后的 GPU 相关资源，与不依赖 GPU 的世界/回合
/// 状态分开存放——与 `p1_acceptance`/`p2_acceptance` 同样的拆分理由。
struct GpuResources {
    gpu: GpuContext,
    render_target: RenderTarget,
    atlas: Atlas,
    batch: SpriteBatch,
    window_size: PhysicalSize<u32>,
}

impl GpuResources {
    fn new(window: Arc<Window>, size: PhysicalSize<u32>) -> GpuResources {
        let gpu = GpuContext::new(window, size).expect("demo 环境应能取得可用的图形适配器");
        let render_target = RenderTarget::new(&gpu);

        let base_metadata = AtlasMetadata::parse(ATLAS_JSON).expect("内嵌图集元数据应为合法 JSON");
        let base_image = image::load_from_memory(ATLAS_PNG)
            .expect("内嵌图集图片应能解码")
            .to_rgba8();
        // 把 CHARSET 覆盖的字形栅格化拼进图集图片下方，得到一张同时含
        // 游戏精灵与文字字形的组合纹理——理由见 `font` 模块文档「为什么
        // 现造一套字体」一节：这样只需一个 SpriteBatch、一次 flush，
        // 不必因为「文字」与「精灵」来自两张纹理而拆成两次绘制调用
        // （`SpriteBatch::flush` 每次都会先清屏，第二次调用会抹掉
        // 第一次画的内容，见其文档）。
        let (combined_image, combined_entries) =
            font::extend_atlas_with_font(&base_metadata, &base_image);
        let mut combined_png_bytes = Vec::new();
        image::DynamicImage::ImageRgba8(combined_image)
            .write_to(
                &mut std::io::Cursor::new(&mut combined_png_bytes),
                image::ImageFormat::Png,
            )
            .expect("内存编码组合图集 PNG 不应失败");
        let combined_metadata = AtlasMetadata {
            image: base_metadata.image.clone(),
            entries: combined_entries,
        };
        let atlas = Atlas::load(&gpu, combined_metadata, &combined_png_bytes)
            .expect("组合图集应能上传为 GPU 纹理");
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

    /// 查条目名对应的 [`AtlasEntry`] 与它的归一化 UV 矩形，找不到时记录
    /// 一条错误日志，理由与 `p1_acceptance`/`p2_acceptance` 的
    /// `GpuResources::lookup` 一致。
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

/// P3 验收 demo 的完整状态。
struct Demo {
    world: WorldState,
    engine: TurnEngine,
    actors: SpawnedActors,
    naming: NamingRules,
    camera: Camera,
    popups: Vec<DamagePopup>,
    resources: Option<GpuResources>,
}

impl Demo {
    fn new() -> Demo {
        let mut world = build_world();
        let mut timeline = Timeline::new();
        let actors = spawn_actors(&mut world, &mut timeline);
        let engine = TurnEngine::new(timeline);
        let player_pos = world
            .actors
            .get(actors.player.id)
            .expect("玩家刚生成，必然存在")
            .pos;
        let camera = Camera {
            center: player_pos,
            world: world.size,
        };

        Demo {
            world,
            engine,
            actors,
            naming: demo_naming_rules(),
            camera,
            popups: Vec::new(),
            resources: None,
        }
    }

    /// 推进一帧的回合逻辑：先把队列里排在玩家之前的 AI 行动全部结算
    /// 掉，再尝试用本帧输入结算玩家一次行动，成功则继续推进后续的 AI
    /// 回合直到再次轮到玩家。
    ///
    /// 玩家已死亡（`world.actors.get` 返回空）时跳过全部回合推进——
    /// 世界仍会照常渲染供观察，只是不再接受输入或继续模拟。
    fn advance_turns(&mut self, input: &InputState) {
        if self.world.actors.get(self.actors.player.id).is_none() {
            return;
        }
        let all = self.actors.all();
        let player = self.actors.player.id;
        self.engine
            .advance_ai(&mut self.world, player, &mut self.popups);
        let player_acted =
            self.engine
                .try_player_turn(&mut self.world, player, &all, input, &mut self.popups);
        if player_acted {
            self.engine
                .advance_ai(&mut self.world, player, &mut self.popups);
        }
        if let Some(agent) = self.world.actors.get(player) {
            self.camera.center = agent.pos;
        }
    }
}

/// 在 `all` 里按标识找出对应的 [`Combatant`]（含图集条目名与染色）。
fn find_combatant(all: &[Combatant], id: EntityId) -> Option<&Combatant> {
    all.iter().find(|combatant| combatant.id == id)
}

/// 把本帧应绘制的全部精灵推入 `resources.batch`。
///
/// 取自由函数而非 `Demo` 的方法，理由与 `p1_acceptance`/`p2_acceptance`
/// 的 `collect_sprites` 一致：`on_frame` 需要同时持有 `&mut self.resources`
/// 与 `self` 的其余字段，写成 `&self` 方法会让编译器把「借了整个
/// `self`」和「借了其中一个字段」混为一谈，报出并不存在的借用冲突。
#[allow(clippy::too_many_arguments)]
fn collect_sprites(
    world: &WorldState,
    engine: &TurnEngine,
    actors: &SpawnedActors,
    naming: &NamingRules,
    camera: &Camera,
    visible: &VisibleSet,
    popups: &[DamagePopup],
    tint: [f32; 4],
    resources: &mut GpuResources,
) {
    let all = actors.all();
    push_terrain(world, camera, visible, tint, resources);
    push_actors(world, &all, naming, camera, visible, tint, resources);
    push_damage_popups(popups, camera, resources);
    push_timeline_sidebar(engine, &all, naming, world.seed, resources);
}

/// 画出相机视口内、且落在本帧视野内的地形瓦片——与
/// `p2_acceptance::push_terrain` 同一实现，理由见其文档。
fn push_terrain(
    world: &WorldState,
    camera: &Camera,
    visible: &VisibleSet,
    tint: [f32; 4],
    resources: &mut GpuResources,
) {
    for pos in camera.visible_tiles() {
        if !visible.contains(pos) {
            continue;
        }
        let kind = world.terrain.terrain_at(pos);
        let Some(name) = terrain_entry_name(kind) else {
            continue;
        };
        let Some((entry, uv)) = resources.lookup(name) else {
            continue;
        };
        let (sx, sy) = camera.world_to_screen(pos);
        let order = DrawOrder::new(
            Layer::TERRAIN,
            sy,
            TERRAIN_ENTITY_BASE + pos.y() as u64 * world.size.width() as u64 + pos.x() as u64,
        );
        resources.batch.push(
            order,
            sprite_instance(sx as f32, sy as f32, entry.sprite_size(), uv, tint),
        );
    }
}

/// 画出玩家与全部存活且在视野内的敌人，各自头顶悬浮一个由
/// [`given_name`] 现算出的名字——验证 Task 2b 的纯函数命名真的能用。
///
/// **`footprint` 取自图集条目本身**（`entry.footprint`），不像 P2 demo
/// 那样硬编码 `Footprint { width: 1, height: 1 }`——`boss_idle_0` 的
/// 2×2 占地正是靠这一步才画得出来（规格 §12.1）。
fn push_actors(
    world: &WorldState,
    all: &[Combatant],
    naming: &NamingRules,
    camera: &Camera,
    visible: &VisibleSet,
    tint: [f32; 4],
    resources: &mut GpuResources,
) {
    for combatant in all {
        let Some(agent) = world.actors.get(combatant.id) else {
            continue;
        };
        if !visible.contains(agent.pos) {
            continue;
        }
        let Some((entry, uv)) = resources.lookup(combatant.sprite) else {
            continue;
        };
        // 把接下来要用的字段拷出来（三者均为 Copy 类型），不再持有
        // `entry`/`uv` 这两个借自 `resources` 的引用——它们的生命周期
        // 必须止步于紧接着的这一次 `resources.batch.push`，否则后面
        // 再碰 `resources`（无论是写 `.batch` 还是再查一次图集）都会被
        // 借用检查器拒绝：一次 `resources.lookup` 借的是整个 `&resources`，
        // 不是只借 `.atlas` 那一块。
        let footprint = entry.footprint;
        let pivot = entry.pivot;
        let sprite_size = entry.sprite_size();
        let (tile_x, tile_y) = camera.world_to_screen(agent.pos);
        let [px, py] = sprite_draw_position((tile_x, tile_y), footprint, pivot);
        let foot_y = footprint_bottom_screen_y(tile_y, footprint.height);
        let order = DrawOrder::new(
            Layer::ENTITY,
            foot_y,
            ACTOR_ENTITY_BASE + combatant.id.as_u64(),
        );
        let modulated = modulate(combatant.tint, tint);
        resources
            .batch
            .push(order, sprite_instance(px, py, sprite_size, uv, modulated));

        let name = given_name(naming, world.seed, combatant.id);
        let width = text_width(&name, NAME_TEXT_SCALE);
        let origin = (
            tile_x as f32 + sprite_size.width as f32 / 2.0 - width / 2.0,
            (foot_y - sprite_size.height as i32 - font::GLYPH_ROWS as i32) as f32,
        );
        push_text(
            resources,
            origin,
            NAME_TEXT_SCALE,
            &name,
            Layer::UI,
            0,
            [1.0, 1.0, 1.0, 1.0],
        );
    }
}

/// 把逐分量颜色调制相乘：单位自身的染色（区分不同敌人）再叠加一层
/// 昼夜/季节色调，夜晚时敌我双方都应一起变暗，而不是只有地形变暗、
/// 单位始终全亮这种不一致的呈现。
fn modulate(base: [f32; 4], ambient: [f32; 4]) -> [f32; 4] {
    [
        base[0] * ambient[0],
        base[1] * ambient[1],
        base[2] * ambient[2],
        base[3],
    ]
}

/// 画出全部存活的伤害飘字：数值随剩余存活帧数轻微上升，营造飘字效果。
fn push_damage_popups(popups: &[DamagePopup], camera: &Camera, resources: &mut GpuResources) {
    for popup in popups {
        let (tile_x, tile_y) = camera.world_to_screen(popup.pos);
        let age = turn::DAMAGE_POPUP_LIFETIME_FRAMES.saturating_sub(popup.remaining_frames);
        let rise = (age / 3) as i32 * DAMAGE_POPUP_RISE_PER_3_FRAMES;
        let text = popup.amount.unsigned_abs().to_string();
        let width = text_width(&text, DAMAGE_TEXT_SCALE);
        let origin = (
            tile_x as f32 + ll_render::sprite::TILE_SIZE as f32 / 2.0 - width / 2.0,
            (tile_y - rise) as f32,
        );
        push_text(
            resources,
            origin,
            DAMAGE_TEXT_SCALE,
            &text,
            Layer::EFFECT,
            0,
            [1.0, 0.3, 0.2, 1.0],
        );
    }
}

/// 画出左上角的时间轴侧栏：接下来 [`SIDEBAR_PREVIEW_COUNT`] 次出手的
/// 顺序，每行「序号 名字」。
fn push_timeline_sidebar(
    engine: &TurnEngine,
    all: &[Combatant],
    naming: &NamingRules,
    seed: u64,
    resources: &mut GpuResources,
) {
    for (row, entry) in engine
        .upcoming(SIDEBAR_PREVIEW_COUNT)
        .into_iter()
        .enumerate()
    {
        let Some(combatant) = find_combatant(all, entry.actor) else {
            continue;
        };
        // 与 push_actors 头顶名字牌用同一个 seed——同一个实体的名字在
        // 侧栏与地图上必须是同一个字符串，否则玩家会误以为侧栏在预告
        // 一个从未在地图上见过的角色。
        let name = given_name(naming, seed, combatant.id);
        let line = format!("{}{}", row + 1, name);
        let origin = (
            SIDEBAR_MARGIN_PX,
            SIDEBAR_MARGIN_PX + row as f32 * SIDEBAR_ROW_HEIGHT_PX,
        );
        push_text(
            resources,
            origin,
            SIDEBAR_TEXT_SCALE,
            &line,
            Layer::UI,
            0,
            [1.0, 1.0, 0.6, 1.0],
        );
    }
}

/// 一个字符在给定缩放下的横向步进（字形宽度 + 1 像素字距），乘以字符
/// 数得到整段文字的像素宽度——供居中摆放伤害飘字/名字牌使用。
fn text_width(text: &str, scale: f32) -> f32 {
    text.chars().count() as f32 * (font::GLYPH_COLS as f32 + 1.0) * scale
}

/// 从 `origin` 起横向逐字画出 `text`，每个字符对应组合图集里的一个
/// `font_*` 条目。查不到的字符（理论上不会发生，见 [`font::glyph_pixels`]
/// 文档）静默跳过，不中断整行绘制。
///
/// `order_entity`（`DrawOrder` 第三排序键）取
/// `UI_TEXT_ENTITY_BASE + 字符在本帧内的绘制序号`——每次调用内部各字符
/// 互不相同即可，不需要跨帧稳定，因为 UI 文字不参与「同一世界状态恒
/// 产出同一遮挡关系」这条视觉回归诉求（那条诉求针对的是地形与战斗
/// 单位，见 `ll_render::sprite::DrawOrder` 文档）。
fn push_text(
    resources: &mut GpuResources,
    origin: (f32, f32),
    scale: f32,
    text: &str,
    layer: Layer,
    foot_y: i32,
    tint: [f32; 4],
) {
    const UI_TEXT_ENTITY_BASE: u64 = 2_000_000_000;
    let mut cursor_x = origin.0;
    for (index, ch) in text.chars().enumerate() {
        let name = font::glyph_entry_name(ch);
        if let Some((entry, uv)) = resources.lookup(&name) {
            let size = entry.sprite_size();
            let order = DrawOrder::new(layer, foot_y, UI_TEXT_ENTITY_BASE + index as u64);
            resources.batch.push(
                order,
                SpriteInstance {
                    position: [cursor_x, origin.1],
                    size: [size.width as f32 * scale, size.height as f32 * scale],
                    uv_rect: uv,
                    color: tint,
                },
            );
        }
        cursor_x += (font::GLYPH_COLS as f32 + 1.0) * scale;
    }
}

/// 拼一个 [`SpriteInstance`]：位置+像素尺寸+UV+颜色调制。
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
        tracing::info!(width = size.width, height = size.height, "window resized");
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.resize(size);
        let viewport = fit_viewport(size.width, size.height);
        tracing::info!(
            scale = viewport.scale,
            offset_x = viewport.offset_x,
            offset_y = viewport.offset_y,
            "recomputed integer-scaled viewport"
        );
    }

    fn on_frame(&mut self, _frame: FrameId, input: &InputState) -> FrameOutcome {
        if input.was_just_pressed(GameKey::Cancel) {
            return FrameOutcome::Exit;
        }

        self.advance_turns(input);
        tick_popups(&mut self.popups);

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        if input.was_just_pressed(GameKey::Screenshot) {
            save_baseline_png(resources, BASELINE_PNG_PATH);
        }

        // 光照与视野半径每帧现算，绝不缓存——理由见 `ll_world::light`
        // 模块文档「光照是纯函数派生，绝不进世界状态」。
        let light = ambient_light(self.world.clock);
        let radius = sight_radius_at(BASE_SIGHT_RADIUS, light);
        // 传各个字段而非 `&self`：`self.resources` 上面已经借出一个
        // `&mut`，`player_pos_or_camera(&self.world, ..)` 这样按字段
        // 借用，编译器才能看出它与 `resources` 借用的是不相交的两块
        // 数据——理由与 `collect_sprites` 取自由函数而非方法完全一致
        // （见本文件顶部模块文档「文件拆分」一节引用的既有 demo 惯例）。
        let visible = compute_fov(
            &self.world.terrain,
            player_pos_or_camera(&self.world, self.actors.player.id, self.camera.center),
            radius,
        );
        let tint = ambient_tint(self.world.clock);

        collect_sprites(
            &self.world,
            &self.engine,
            &self.actors,
            &self.naming,
            &self.camera,
            &visible,
            &self.popups,
            tint,
            resources,
        );
        resources
            .batch
            .flush(&resources.gpu, resources.render_target.view());
        resources.present();

        FrameOutcome::Continue
    }

    fn on_exit(&mut self) {
        tracing::info!("demo exiting");
    }
}

/// 玩家存活时用玩家当前位置求视野，玩家已死亡（`actors.get` 落空）时
/// 退回相机中心（`Demo::advance_turns` 里最后一次更新的位置）——保证
/// FOV 查询恒有一个合法的原点可用，不会因为玩家死亡而 panic。
///
/// 接受拆开的字段而非 `&Demo`：调用点（`on_frame`）此时已经借出
/// `self.resources` 的 `&mut`，传 `&Demo` 会把这个借用与 `self` 的
/// 其余字段混为一谈，报出并不存在的借用冲突——理由与
/// `collect_sprites` 取自由函数而非方法一致。
fn player_pos_or_camera(world: &WorldState, player: EntityId, camera_center: TorusPos) -> TorusPos {
    world
        .actors
        .get(player)
        .map(|agent| agent.pos)
        .unwrap_or(camera_center)
}

fn main() {
    init_logging(true).expect("首次初始化日志不应失败");
    tracing::info!(
        "P3 acceptance demo: arrows/WASD move or attack (walk into an enemy to attack), \
         '.' waits a turn, F2 saves baseline PNG, Esc to quit"
    );

    let demo = Demo::new();
    if let Err(error) = run(WindowConfig::default(), demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}
