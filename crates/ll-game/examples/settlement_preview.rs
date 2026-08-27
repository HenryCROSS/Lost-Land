//! 据点建筑地形的**可视化**验收产物：把一间手工摆好的小屋（石墙 + 木
//! 墙 + 木地板 + 石地板 + 关着的门 + 开着的门 + 窗 + 上下楼梯）按生产
//! 路径算出来的图集键拼成一张 PNG，直接用眼睛验收「走进据点看得见、
//! 墙/地板/门/窗互相分得开」。
//!
//! 用法（无需 GPU、无需窗口）：
//!
//! ```text
//! cargo run -p ll-game --example settlement_preview
//! ```
//!
//! 产物写到 `crates/ll-game/tests/visual/settlement_preview.png`，规矩
//! 与 `crates/ll-game/tests/visual/README.md` 那份一致。
//!
//! # 与 `surface_preview` 的分工
//!
//! [`surface_preview`](../surface_preview.rs) 验的是**地表内容**（物品
//! 堆/家具/NPC/玩家）画在了哪一格、谁挡住谁——它用的是真实生成的世界，
//! 玩家周围恰好有什么由种子决定。本文件验的是**建筑地形本身长什么样**：
//! 世界生成不保证玩家出生点附近一定有据点，靠随机世界去看这九种地形
//! 是碰运气，因此这里手工摆一间小屋的平面图。
//!
//! 手工摆的只有「哪一格是哪种地形」这一件事。每一格该查哪个图集键，
//! 走的仍是生产路径上的 [`ll_game::layout::terrain_atlas_key`]；图集
//! 本身走的仍是生产路径上的 [`load_sprite_sources`] + [`pack_atlas`]。
//! 换句话说：这张图里任何一格画错/画不出来，说明生产代码真的有问题，
//! 不是预览脚本自己的锅。
//!
//! # 它证明什么，不证明什么
//!
//! **证明**：这九种建筑地形在真实图集里都查得到、都有图、且互相长得
//! 不一样。
//!
//! **不证明**：颜色经过昼夜/天气/迷雾 tint 之后还对不对——与
//! `surface_preview` 同一条局限，本文件恒按原色拷贝。
//!
//! 给机器看的那一半在 `crates/ll-game/tests/atlas_coverage.rs`：那里
//! 逐条断言 17 种地形都查得到条目、都铺满整格、两两之间至少四分之一
//! 像素不同。两边合起来才是完整证据。

use std::path::PathBuf;

use image::RgbaImage;
use ll_game::app::load_sprite_sources;
use ll_game::content::load_content;
use ll_render::atlas_pack::{PackedAtlas, pack_atlas};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

/// 存盘前的整数放大倍数——16 像素的瓦片在屏幕上太小，看不出门把手、
/// 窗棂、阶梯这些结构。与 `surface_preview` 取同一个值。
const ZOOM: u32 = 4;

/// 平面图里一个格子的字符编码。手写成字符画而不是二维枚举数组，是因为
/// 「这间屋子长什么样」在字符画里一眼能看出来，在数组字面量里看不出来。
///
/// - `#` 石墙　`=` 木墙　`.` 木地板　`,` 石地板
/// - `+` 关着的门　`/` 开着的门　`o` 窗
/// - `<` 上楼梯　`>` 下楼梯　`~` 草地（屋外）
const FLOOR_PLAN: &[&str] = &[
    "~~~~~~~~~~~~",
    "~##########~",
    "~#,,,,#....#",
    "~#,,,,#....#",
    "~#,,,,=....o",
    "~#,<,,=..>.#",
    "~#,,,,#....#",
    "~####+####/#",
    "~~~~~~~~~~~~",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 把平面图字符翻译成地形种类。未知字符直接 panic——静默跳过会在图上
/// 留一个看不见的洞，与 `ll-artgen` 的 `draw_entry` 同一条原则。
fn kind_of(cell: char, ids: &BaseTerrainIds) -> TerrainKind {
    match cell {
        '#' => ids.wall_stone,
        '=' => ids.wall_wood,
        '.' => ids.floor_wood,
        ',' => ids.floor_stone,
        '+' => ids.door_closed,
        '/' => ids.door_open,
        'o' => ids.window,
        '<' => ids.stairs_up,
        '>' => ids.stairs_down,
        '~' => ids.grass,
        other => panic!("平面图里有未知字符 {other:?}"),
    }
}

fn main() {
    let content =
        load_content(&repo_root().join("mods"), &repo_root().join("assets")).expect("装载真实内容");
    let atlas = pack_atlas(&load_sprite_sources(&content.asset_vfs));

    let rows = FLOOR_PLAN.len() as u32;
    let cols = FLOOR_PLAN[0].chars().count() as u32;
    assert!(
        FLOOR_PLAN
            .iter()
            .all(|row| row.chars().count() as u32 == cols),
        "平面图各行长度必须一致"
    );

    let tile = 16u32;
    let mut canvas = RgbaImage::new(cols * tile, rows * tile);
    for (row_index, row) in FLOOR_PLAN.iter().enumerate() {
        for (col_index, cell) in row.chars().enumerate() {
            let kind = kind_of(cell, &content.terrain_ids);
            // 生产路径：这一格该查哪个图集键，由 `terrain_atlas_key`
            // 决定，本文件不自己推。
            let key =
                ll_game::layout::terrain_atlas_key(kind, &content.terrain_ids, &content.registry)
                    .unwrap_or_else(|| panic!("地形字符 {cell:?} 算不出图集键"));
            let entry = atlas
                .metadata
                .lookup(&key)
                .unwrap_or_else(|| panic!("图集里查不到条目 {key}（地形字符 {cell:?}）"));
            blit(
                &mut canvas,
                &atlas,
                entry.rect.x,
                entry.rect.y,
                entry.rect.width,
                entry.rect.height,
                col_index as u32 * tile,
                row_index as u32 * tile,
            );
        }
    }

    let out = repo_root()
        .join("crates/ll-game/tests/visual")
        .join("settlement_preview.png");
    upscale(&canvas, ZOOM).save(&out).expect("写入预览 PNG");
    println!("已写出 {}", out.display());
}

#[allow(clippy::too_many_arguments)]
fn blit(
    canvas: &mut RgbaImage,
    atlas: &PackedAtlas,
    src_x: u16,
    src_y: u16,
    w: u16,
    h: u16,
    dst_x: u32,
    dst_y: u32,
) {
    for row in 0..u32::from(h) {
        for col in 0..u32::from(w) {
            let src = *atlas
                .canvas
                .get_pixel(u32::from(src_x) + col, u32::from(src_y) + row);
            canvas.put_pixel(dst_x + col, dst_y + row, src);
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
