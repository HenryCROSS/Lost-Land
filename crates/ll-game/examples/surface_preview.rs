//! 地表内容渲染的**可视化**验收产物：把地形、地面物品堆、放置家具、
//! NPC、玩家按生产路径算出来的绘制指令拼成一张 PNG，直接用眼睛验收
//! 「跑起来能看见」。
//!
//! 用法（无需 GPU、无需窗口）：
//!
//! ```text
//! cargo run -p ll-game --example surface_preview
//! ```
//!
//! 产物写到 `crates/ll-game/tests/visual/surface_preview.png`，规矩与
//! `crates/ll-render/tests/visual/README.md` 那份视觉回归基准一致。
//!
//! # 这不是第二个渲染器，别把它当渲染器用
//!
//! 本文件**只做一件事**：把真实图集画布上的矩形，按生产代码算出来的
//! 位置原样拷贝到一张画布上。位置、层序、图集键的选择全部来自生产
//! 代码——[`ll_game::surface_draw::surface_draws`]、
//! [`ll_render::sprite::sprite_draw_position`]、
//! [`ll_render::sprite::DrawOrder`]、[`ll_render::camera::Camera`]——本
//! 文件一行都没有自己重推。
//!
//! 它与真正的渲染路径（`ll_game::app::render_surface` → `SpriteBatch`
//! → wgpu）唯一没有共享的是**着色器那一步**：真实路径会按 `tint` 给
//! 每个精灵乘一次颜色（昼夜/天气/迷雾），本文件恒按原色拷贝。因此
//! 这张图能证明「什么东西被画在了哪里、谁挡住谁」，**不能**证明
//! 「颜色对不对」。颜色那一侧仍然只有 GPU demo（按 F2 存基准，见
//! `crates/ll-render/tests/visual/README.md`）能验。
//!
//! 之所以还是做了这一份：真实 GPU demo 要开窗口、要人按 F2，
//! [ADR 0025](../../../knowledge/decisions/0025-demo-interaction-verification-forbids-sendkeys.md)
//! 又禁止用合成按键去自动化它——于是「跑一遍就能看见结果」这件事在
//! CI 与代理开发流程里根本没法自动完成。这张图把「接线对不对」这半边
//! 变成了可以随时一条命令复现的证据。

use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use ll_game::app::load_sprite_sources;
use ll_game::content::{LoadedContent, load_content};
use ll_game::surface_draw::{PLAYER_ENTITY, SurfaceDraw, surface_draws};
use ll_game::world::{GameWorld, build_new_world};
use ll_render::atlas_pack::{PackedAtlas, pack_atlas};
use ll_render::camera::Camera;
use ll_render::sprite::{
    DrawOrder, Layer, TILE_SIZE, footprint_bottom_screen_y, sprite_draw_position,
};
use ll_sim::item::ItemStack;
use ll_world::item::GroundItemStack;

/// 预览画布尺寸——刻意比 640×360 的离屏目标小一大截：这张图是给人看
/// 的，只需要把玩家周围那几格摆的东西看清楚，整幅 640×360 里绝大多数
/// 面积都是重复的地形。
const PREVIEW_TILES_X: i32 = 13;
const PREVIEW_TILES_Y: i32 = 9;
/// 存盘前的整数放大倍数——16 像素的瓦片在屏幕上太小，看不出记号的形状。
const ZOOM: u32 = 4;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() {
    let content =
        load_content(&repo_root().join("mods"), &repo_root().join("assets")).expect("装载真实内容");
    let mut world = build_new_world(&content, 20260826).expect("建世界");
    let atlas = pack_atlas(&load_sprite_sources(&content.asset_vfs));

    arrange_scene(&mut world, &content);

    let player_pos = world.world.actors.get(world.player).expect("玩家存在").pos;
    let camera = Camera {
        center: player_pos,
        world: world.world.size,
    };

    // 收集全部绘制项：地形一批 + 地表内容一批 + 玩家标记一条，全部带上
    // 生产代码里那一套 `DrawOrder`，最后统一排序——排序规则本身来自
    // `ll_render`，本文件不自己定义「谁挡住谁」。
    let mut items: Vec<(DrawOrder, &str, (i32, i32))> = Vec::new();
    let world_width = world.world.size.width() as u64;
    // 多铺一圈（`+ 1`）：预览画布的原点相对瓦片网格未必整格对齐，边缘
    // 那一列/一行若只铺到正好的范围，会在画布边上留一条没画到的黑边。
    for dy in -PREVIEW_TILES_Y / 2 - 1..=PREVIEW_TILES_Y / 2 + 1 {
        for dx in -PREVIEW_TILES_X / 2 - 1..=PREVIEW_TILES_X / 2 + 1 {
            let pos = world
                .world
                .size
                .wrap(player_pos.x() + dx, player_pos.y() + dy);
            let Some(kind) = world.world.terrain_at(pos) else {
                continue;
            };
            let Some(name) =
                ll_game::layout::terrain_atlas_key(kind, &content.terrain_ids, &content.registry)
            else {
                continue;
            };
            let Some(entry) = atlas.metadata.lookup(&name) else {
                continue;
            };
            let (sx, sy) = camera.world_to_screen(pos);
            items.push((
                DrawOrder::new(
                    Layer::TERRAIN,
                    sy,
                    1 + pos.y() as u64 * world_width + pos.x() as u64,
                ),
                leak(entry.name.clone()),
                (sx, sy),
            ));
        }
    }

    for draw in surface_draws(&world.world, &content.registry, world.player) {
        let Some(name) = first_present(&atlas, &draw) else {
            continue;
        };
        let (sx, sy) = camera.world_to_screen(draw.pos);
        let entry = atlas.metadata.lookup(name).expect("刚查到过");
        items.push((
            DrawOrder::new(
                draw.layer,
                footprint_bottom_screen_y(sy, entry.footprint.height),
                draw.entity,
            ),
            name,
            (sx, sy),
        ));
    }

    // 玩家标记：与 `render_surface` 里 `push_player_marker` 同一条。
    let player_sprite = "lostland:hero_idle_0";
    if let Some(entry) = atlas.metadata.lookup(player_sprite) {
        let (sx, sy) = camera.world_to_screen(player_pos);
        items.push((
            DrawOrder::new(
                Layer::ENTITY,
                footprint_bottom_screen_y(sy, entry.footprint.height),
                PLAYER_ENTITY,
            ),
            player_sprite,
            (sx, sy),
        ));
    }

    items.sort_by_key(|(order, _, _)| *order);

    let canvas = compose(&atlas, &items, camera, player_pos);
    let out = repo_root()
        .join("crates/ll-game/tests/visual")
        .join("surface_preview.png");
    std::fs::create_dir_all(out.parent().expect("有父目录")).expect("建目录");
    upscale(&canvas, ZOOM).save(&out).expect("写 PNG");
    println!("已写出 {}", out.display());
    println!("图中应当看得见：琥珀色的地面物品堆、紫罗兰的通用家具记号、");
    println!("带火口的锻炉、紫红色的 NPC、以及钢蓝色的玩家。");
}

/// 摆一个能把四类记号一次看全的场景。
fn arrange_scene(world: &mut GameWorld, content: &LoadedContent) {
    let player = world
        .world
        .actors
        .get(world.player)
        .expect("玩家存在")
        .clone();
    let (px, py) = (player.pos.x(), player.pos.y());

    // 一格躺三样东西（应当只出现一个团），另一格躺一样（同样一个团）。
    for id in [
        "lostland:iron_ingot",
        "lostland:iron_rivet",
        "lostland:smith_hammer",
    ] {
        drop_item(world, content, px - 2, py, id, false);
    }
    drop_item(world, content, px - 2, py + 2, "lostland:iron_ingot", false);
    // 一座有自带贴图的家具，一件没有自带贴图的放置物（走通用记号）。
    drop_item(world, content, px + 2, py, "lostland:forge", true);
    drop_item(world, content, px + 2, py + 2, "lostland:iron_ingot", true);

    // 两个 NPC：一个在玩家上方、一个在下方，用来看 ENTITY 层的前后遮挡。
    for (dx, dy) in [(0, -2), (1, 2)] {
        let mut npc = player.clone();
        npc.pos = world.world.size.wrap(px + dx, py + dy);
        world.world.actors.spawn(npc);
    }
}

fn drop_item(
    world: &mut GameWorld,
    content: &LoadedContent,
    x: i32,
    y: i32,
    id: &str,
    placed: bool,
) {
    let def = content
        .registry
        .get(&ll_core::ident::NamespacedId::parse(id).expect("字面量合法"))
        .unwrap_or_else(|| panic!("真实 mods/ 里应当注册了 {id}"));
    let pos = world.world.size.wrap(x, y);
    world.world.ground_items.push(GroundItemStack {
        pos,
        stack: ItemStack::new(def, 1),
        dropped_at: ll_core::time::Tick(0),
        contents: Vec::new(),
        placed,
    });
}

/// 与 `GpuResources::lookup_first` 同一条次序：优先内容自带键，兜底通用
/// 记号。
fn first_present(atlas: &PackedAtlas, draw: &SurfaceDraw) -> Option<&'static str> {
    draw.keys()
        .find(|name| atlas.metadata.lookup(name).is_some())
        .map(|name| leak(name.to_string()))
}

/// 借用期限太碎，直接泄漏成 `'static`——这是个一次性的验收小程序，进程
/// 跑完就退出，泄漏几十个短字符串没有任何实际后果，比为它引入一份
/// 生命周期缠绕的中间结构划算。
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// 把排好序的绘制项按顺序拷到画布上。
fn compose(
    atlas: &PackedAtlas,
    items: &[(DrawOrder, &str, (i32, i32))],
    camera: Camera,
    center: ll_core::torus::TorusPos,
) -> RgbaImage {
    let width = (PREVIEW_TILES_X as u32) * TILE_SIZE;
    let height = (PREVIEW_TILES_Y as u32) * TILE_SIZE;
    // 预览画布的原点相对离屏目标原点的偏移：把相机中心那一格摆到预览
    // 画布正中。
    let (cx, cy) = camera.world_to_screen(center);
    let origin_x = cx - (width as i32) / 2 + TILE_SIZE as i32 / 2;
    let origin_y = cy - (height as i32) / 2 + TILE_SIZE as i32 / 2;

    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([12, 12, 16, 255]));
    for (_, name, (sx, sy)) in items {
        let entry = atlas.metadata.lookup(name).expect("已经查到过");
        let [dx, dy] = sprite_draw_position((*sx, *sy), entry.footprint, entry.pivot);
        blit(
            &mut canvas,
            atlas,
            entry.rect.x,
            entry.rect.y,
            entry.rect.width,
            entry.rect.height,
            dx as i32 - origin_x,
            dy as i32 - origin_y,
        );
    }
    canvas
}

#[allow(clippy::too_many_arguments)]
fn blit(
    canvas: &mut RgbaImage,
    atlas: &PackedAtlas,
    src_x: u16,
    src_y: u16,
    w: u16,
    h: u16,
    dst_x: i32,
    dst_y: i32,
) {
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let (tx, ty) = (dst_x + col, dst_y + row);
            if tx < 0 || ty < 0 || tx >= canvas.width() as i32 || ty >= canvas.height() as i32 {
                continue;
            }
            let src = *atlas
                .canvas
                .get_pixel(u32::from(src_x) + col as u32, u32::from(src_y) + row as u32);
            // 全透明像素跳过——这正是「地面物品堆的背景留空」在画面上
            // 生效的地方：下面那格地形透得出来。
            if src.0[3] == 0 {
                continue;
            }
            canvas.put_pixel(tx as u32, ty as u32, src);
        }
    }
}

fn upscale(image: &RgbaImage, factor: u32) -> RgbaImage {
    let mut out = RgbaImage::new(image.width() * factor, image.height() * factor);
    for y in 0..out.height() {
        for x in 0..out.width() {
            out.put_pixel(x, y, *image.get_pixel(x / factor, y / factor));
        }
    }
    out
}
