//! `app::surface`：把世界的一屏格子连同三层战争迷雾画出来。
//!
//! 本模块由 [`crate::app`] 按职责拆出（批次 16，纯搬移，没有改动任何逻辑）。
//! 拆分的依据不是行数而是「下一批要往哪里加东西」：对话批次要加一块屏、
//! UI 布局批次要改 HUD，两批原先撞在同一个文件的同两个函数上。主循环
//! （`impl AppHandler for Demo`）与 `Demo` 自身的状态仍然在 [`crate::app`]。

use ll_render::batch::SpriteInstance;
use ll_render::camera::{Camera, Zoom, apply_zoom};
use ll_render::sprite::{DrawOrder, Layer, footprint_bottom_screen_y, sprite_draw_position};
use ll_world::fov::compute_fov;
use ll_world::surface_store::SurfaceWindow;
use ll_world::weather::Weather;

use crate::atlas_miss::DrawResolution;
use crate::content::LoadedContent;
use crate::layout::{
    effective_sight_radius, effective_sight_radius_for_race, effective_tint, terrain_atlas_key,
    tile_tint,
};
use crate::surface_draw::{PLAYER_ENTITY, SurfaceDraw, TERRAIN_ENTITY_BASE, surface_draws};
use crate::world::GameWorld;

use super::gpu::{GpuResources, atlas_contains};

pub(super) fn render_surface(
    game_world: &GameWorld,
    content: &LoadedContent,
    camera: &Camera,
    zoom: Zoom,
    sprite_name: &str,
    resources: &mut GpuResources,
) {
    let world = &game_world.world;
    let player_agent = world.actors.get(game_world.player);
    let player_pos = player_agent.map(|agent| agent.pos).unwrap_or(camera.center);
    let profile = space_profile_of(content, world.surface_profile);
    let clock = world.clock;
    // 天气：**每帧派生一次**，随后这一帧的每一格都复用同一个值。绝不
    // 在逐格循环里再算——`Weather::derive` 要走一次加权遍历，虽然只有
    // 六条内容，放进每帧上万次的逐格循环仍然是白白浪费（ADR 0016/0017
    // 的热路径纪律）。天气不进 `WorldState`，这里是它在生产渲染路径上
    // 的唯一派生点，输入只有世界种子与世界时钟，见 `ll_world::weather`
    // 模块文档。
    let weather = Weather::derive(world.seed, clock, &content.weather_table);
    // 视野半径叠加玩家种族声明的夜间视野格数（见 `effective_sight_radius_for_race`
    // 模块文档「为什么接在这一步」一节）——玩家实体查不到（理论上不该
    // 发生，见上方 `unwrap_or(camera.center)` 同一条降级纪律）时退化到
    // 不叠加暗视的 `effective_sight_radius`，不是 panic。
    let radius = match player_agent {
        Some(agent) => effective_sight_radius_for_race(
            &profile,
            clock,
            weather,
            agent.race,
            &content.race_table,
        ),
        None => effective_sight_radius(&profile, clock, weather),
    };
    let tint = effective_tint(&profile, clock, weather);
    let layout = *world.terrain.layout();

    let visible = compute_fov(
        &SurfaceWindow::new(&world.terrain),
        &world.terrain_table,
        player_pos,
        radius,
    );

    let world_width = world.size.width() as u64;
    for pos in camera.visible_tiles_zoomed(zoom) {
        let explored = world.exploration.is_explored(&layout, pos);
        let Some(pos_tint) = tile_tint(visible.contains(pos), explored, tint) else {
            // 从未探索：不画，交给清屏背景表现「黑」，见本函数文档。
            continue;
        };
        let Some(kind) = world.terrain_at(pos) else {
            continue;
        };
        let Some(name) = terrain_atlas_key(kind, &content.terrain_ids, &content.registry) else {
            continue;
        };
        // 两步：先挑键（写去重账本，持 `&mut`），再取条目（`&self`）。
        // 地形瓦片每帧画满整屏，是全仓库最容易刷屏的一处——单个名字
        // 也走去重的那条路，不另开一条「只有一个候选就直接打日志」的
        // 旁路，见 [`GpuResources::resolve_key`] 文档。
        let Some(key) = resources.resolve_key(std::iter::once(name.as_str())) else {
            continue;
        };
        let Some((entry, uv)) = resources.entry_of(key) else {
            continue;
        };
        // 先按未缩放的相机换算拿到屏幕坐标（DrawOrder 排序用这份原始
        // 坐标即可——zoom 是围绕视口中心的单调变换，纵坐标的大小关系
        // 恒定不变，见 apply_zoom 文档），再对绘制位置单独套一次
        // apply_zoom。
        let (sx, sy) = camera.world_to_screen(pos);
        let order = DrawOrder::new(
            Layer::TERRAIN,
            sy,
            TERRAIN_ENTITY_BASE + pos.y() as u64 * world_width + pos.x() as u64,
        );
        let [zx, zy] = apply_zoom([sx as f32, sy as f32], zoom);
        resources.batch.push(
            order,
            sprite_instance(zx, zy, entry.sprite_size(), uv, pos_tint, zoom),
        );
    }

    // 地面物品堆 / 放置家具 / NPC——地形与玩家之外的三类世界内容，见
    // `crate::surface_draw` 模块文档「这个模块补的是哪个洞」。三类共用
    // 同一个 push 帮手，绘制层序由各自指令里的 `layer` 决定（地形
    // → 地面物品/家具 → NPC → 玩家）。
    for draw in surface_draws(world, &content.registry, game_world.player) {
        // **只画当前视野内的**，不画「记得那里曾经有东西」：迷雾记忆
        // （`ll_world::exploration`）记的是地形，不是物品与人——地形不会
        // 自己跑掉，物品与 NPC 会。把它们也按记忆画出来，等于让玩家隔着
        // 迷雾看见一堆早就被人捡走的东西。这条是本批次的判断，不是所有者
        // 的裁定；真要做「上次见到时那里有东西」的记忆层，需要先有一份
        // 存进 `WorldState` 的观察记录，不是渲染层能自己变出来的。
        if !visible.contains(draw.pos) {
            continue;
        }
        push_surface_draw(&draw, camera, tint, zoom, resources);
    }

    let (px, py) = camera.world_to_screen(player_pos);
    push_player_marker(px, py, sprite_name, tint, zoom, resources);
}

/// 把一条 [`SurfaceDraw`] 变成一个精灵实例推进批次。
///
/// 三类世界内容（地面物品堆、放置家具、NPC）共用这一个消费点——这正是
/// `crate::surface_draw` 模块文档里「不许把同一段查图逻辑抄三遍」那条
/// 抽象理由的落点：查图次序（内容自带键 → 通用记号）在
/// [`SurfaceDraw::keys`]，锚点/缩放/绘制顺序换算在这里，两处各只有一份。
///
/// 查不到任何图集条目时**跳过**而不是 panic：整条候选链落空已经由
/// [`GpuResources::resolve_key`] 按去重策略留过一条 WARN，画面上少一个记号远好于
/// 让整局游戏崩掉（与 [`push_player_marker`] 同一条降级纪律）。
pub(super) fn push_surface_draw(
    draw: &SurfaceDraw,
    camera: &Camera,
    tint: [f32; 4],
    zoom: Zoom,
    resources: &mut GpuResources,
) {
    // 被压制的那一层整个不画——见 `SurfaceDraw::superseded_by` 字段
    // 文档。判据在这里而不是在 `surface_draw` 里，是因为「这个键在图集
    // 里查不查得到」只有拿得到图集的这一侧回答得了，而这一侧正是查图
    // 次序（`SurfaceDraw::keys`）的唯一消费点，两件事因此仍然只有一处。
    //
    // 压制判定与回退链走**同一个**决定点 [`MissLedger::resolve_draw`]，
    // 两者的探测都是静默的。此前压制判定调的是会打日志的取用接口，而
    // `superseded_by` 装的正是两个合成键、今天一张合成图都没有——于是
    // 每个 NPC 每帧刷两行 ERROR，正是所有者实机撞到的那一屏。见
    // `crate::atlas_miss` 模块文档。
    let resolution = {
        // 账本要可变借用、图集要共享借用，两者是 `GpuResources` 上不相
        // 干的字段，显式借出以免方法调用借走整个结构体。
        let atlas = &resources.atlas;
        resources.miss_ledger.resolve_draw(
            draw.superseded_by.iter().map(String::as_str),
            draw.keys(),
            |key| atlas_contains(atlas, key),
        )
    };
    let key = match resolution {
        DrawResolution::Draw { key } => key,
        DrawResolution::Superseded => return,
        DrawResolution::Missed { report } => {
            if let Some(candidates) = report {
                tracing::warn!(
                    %candidates,
                    "整条图集候选链都没有命中，这一层没有画出来（同一组候选只报一次）"
                );
            }
            return;
        }
    };
    // 取条目走 `&self`：`resolution` 已经把可变借用还回去了，接下来那句
    // 推批次因此仍然借得到 `&mut`，见 [`GpuResources::resolve_key`] 文档
    // 「为什么返回键名而不是条目」一节。
    let Some((entry, uv)) = resources.entry_of(key) else {
        return;
    };
    let (sx, sy) = camera.world_to_screen(draw.pos);
    let footprint = entry.footprint;
    // 锚点换算与 zoom 后处理的次序与 `push_player_marker` 逐字一致，
    // 理由见那边的注释。
    let [ax, ay] = sprite_draw_position((sx, sy), footprint, entry.pivot);
    let order = DrawOrder::new(
        draw.layer,
        footprint_bottom_screen_y(sy, footprint.height),
        draw.entity,
    );
    let [zx, zy] = apply_zoom([ax, ay], zoom);
    resources.batch.push(
        order,
        sprite_instance(zx, zy, entry.sprite_size(), uv, tint, zoom),
    );
}

/// 空间的完整 [`ll_world::space_profile::SpaceProfile`]——`Space` 本身
/// 只携带一个索引，渲染层需要的是完整字段，从装载好的空间层属性表
/// 现查现拼，不缓存（理由与 `p5_coordinate_acceptance::DemoWorld::profile_of`
/// 一致）。
pub(super) fn space_profile_of(
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

/// 画出玩家标记。`sprite_name` 是当前动画帧应显示的图集条目名（由
/// [`AppHandler::on_frame`](ll_platform::window::AppHandler::on_frame) 通过 [`current_sprite_name`](ll_render::anim::current_sprite_name) 现算，缺帧时
/// 已经退回 [`FALLBACK_SPRITE`](crate::animation::FALLBACK_SPRITE)），不再恒定画同一帧静态图——这正是
/// 「接上行走/待机动画」这条修复的落点，见 [`crate::animation`]
/// 模块文档。
pub(super) fn push_player_marker(
    sx: i32,
    sy: i32,
    sprite_name: &str,
    tint: [f32; 4],
    zoom: Zoom,
    resources: &mut GpuResources,
) {
    let Some(key) = resources.resolve_key(std::iter::once(sprite_name)) else {
        return;
    };
    let Some((entry, uv)) = resources.entry_of(key) else {
        return;
    };
    let footprint = entry.footprint;
    // 锚点/pivot 换算全部走未缩放坐标——sprite.rs 的这套算术不知道、
    // 也不需要知道 zoom 存在（见 apply_zoom 文档「为什么是围绕视口
    // 自由函数」），缩放作为最后一步的后处理套在算出来的最终绘制
    // 位置上：这等价于把整张离屏画布围绕中心缩放，pivot 偏移量本身
    // 也会随之成比例缩放，marker 不会因为缩放而错位。
    let [px, py] = sprite_draw_position((sx, sy), footprint, entry.pivot);
    let order = DrawOrder::new(
        Layer::ENTITY,
        footprint_bottom_screen_y(sy, footprint.height),
        PLAYER_ENTITY,
    );
    let [zx, zy] = apply_zoom([px, py], zoom);
    resources.batch.push(
        order,
        sprite_instance(zx, zy, entry.sprite_size(), uv, tint, zoom),
    );
}

/// 构造一个精灵实例：`x`/`y` 应为已经套过 [`apply_zoom`] 的最终屏幕
/// 位置，`size` 是精灵未缩放的原生像素尺寸，本函数负责按 `zoom` 把
/// 它换算成绘制尺寸——位置需要「围绕中心」的特殊变换（`apply_zoom`），
/// 尺寸只是单纯的比例缩放，两者不能共用同一个变换，因此调用方必须
/// 先各自处理好再传进来，本函数只做最后的结构体组装。
pub(super) fn sprite_instance(
    x: f32,
    y: f32,
    size: ll_render::sprite::SpriteSize,
    uv_rect: [f32; 4],
    color: [f32; 4],
    zoom: Zoom,
) -> SpriteInstance {
    SpriteInstance {
        position: [x, y],
        size: [
            size.width as f32 * zoom.get(),
            size.height as f32 * zoom.get(),
        ],
        uv_rect,
        color,
    }
}
