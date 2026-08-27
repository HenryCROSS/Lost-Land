//! NPC 外观的**可视化**验收产物：把「每一个有图的种族」× 「每一个有图
//! 的职业」拼成一张点名册 PNG，直接用眼睛验收所有者那句「npc 根据职业
//! 种族做出区别」。
//!
//! 用法（无需 GPU、无需窗口）：
//!
//! ```text
//! cargo run -p ll-game --example npc_roster_preview
//! ```
//!
//! 产物写到 `crates/ll-game/tests/visual/npc_roster_preview.png`，规矩与
//! `crates/ll-game/tests/visual/README.md` 那份一致。
//!
//! # 图怎么读
//!
//! 一行是一个种族，一列是一个职业。第一列**没有职业挂件**，是那个种族
//! 的裸身子——把它和同一行右边的格子对照，就能看清「挂件只改胸口那一
//! 块、别的一概不动」。
//!
//! # 它证明什么，不证明什么
//!
//! **证明**：每一格该查哪两个图集键，走的是生产路径上的
//! [`ll_game::surface_draw::npc_draws`]；图集走的是生产路径上的
//! [`load_sprite_sources`] + [`pack_atlas`]；两层怎么叠、谁在上，走的是
//! [`npc_draws`] 给出的 `SurfaceDraw::entity` 升序，与
//! `ll_render::sprite::DrawOrder` 在同层同脚底纵坐标时的比较键是同一个。
//! 这张图里任何一格画错/画不出来，说明生产代码真的有问题。
//!
//! **不证明**：颜色经过昼夜/天气/迷雾 tint 之后还对不对——与
//! `surface_preview`/`settlement_preview` 同一条局限，本文件恒按原色
//! 拷贝。
//!
//! 给机器看的那一半在 `crates/ll-game/tests/npc_appearance.rs`：那里逐条
//! 断言本体每个种族/职业都有自带贴图、52 种组合两两之间至少差整块徽记、
//! 同职业跨种族至少差四分之一张图。两边合起来才是完整证据。
//!
//! # 名册的内容不在这里写死
//!
//! 行与列都从 [`ll_mod::registry::Registry`] 现查（按 `race_table`/
//! `class_table` 的 `is_defined` 过滤），再筛掉「注册了但没配图」的那些。
//! 因此**加第 10 个种族之后重新跑一遍，它自己会出现在图上**，不需要有人
//! 记得回来改这个脚本。

use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use ll_core::ident::{ContentIndex, NamespacedId};
use ll_game::app::load_sprite_sources;
use ll_game::content::{LoadedContent, load_content};
use ll_game::surface_draw::{NPC_BADGE_ENTITY_BASE, NPC_ENTITY_BASE, npc_draws};
use ll_game::world::{GameWorld, build_new_world};
use ll_render::atlas_pack::{PackedAtlas, pack_atlas};

/// 存盘前的整数放大倍数——与 `settlement_preview`/`surface_preview` 取
/// 同一个值。
const ZOOM: u32 = 4;

/// 一格的宽高，与 NPC 贴图同尺寸。
const CELL_W: u32 = 16;
/// 同上。
const CELL_H: u32 = 24;

/// 格与格之间的留白（像素），免得相邻两个 NPC 的轮廓糊在一起。
const GAP: u32 = 2;

/// 预览底色（深石板灰）。**不来自图集**：这张图验的是 NPC 自己长什么
/// 样，铺一层真实地形只会多一个与验收无关的变量；但完全透明的底又会让
/// 深色像素在看图工具里读不出来，因此铺一个中性的暗色。
const BACKDROP: Rgba<u8> = Rgba([38, 40, 46, 255]);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 注册表里全部已定义属性的种族/职业，按注册顺序（`snapshot` 是 `Vec`，
/// 不经任何哈希容器——约束 C5）。
fn registered(content: &LoadedContent, race: bool) -> Vec<(NamespacedId, ContentIndex)> {
    content
        .registry
        .snapshot()
        .into_iter()
        .filter_map(|id| {
            let index = content.registry.get(&id)?;
            let defined = if race {
                content.race_table.is_defined(index)
            } else {
                content.class_table.is_defined(index)
            };
            defined.then_some((id, index))
        })
        .collect()
}

/// 这条内容有没有自带贴图——名册只列有图的那些，没图的种族/职业在图上
/// 只会是一片一模一样的通用记号，列出来没有信息量。
fn has_own_sprite(atlas: &PackedAtlas, id: &NamespacedId) -> bool {
    atlas.metadata.lookup(&id.to_string()).is_some()
}

fn main() {
    let root = repo_root();
    let content = load_content(&root.join("mods"), &root.join("assets")).expect("装载真实内容");
    let atlas = pack_atlas(&load_sprite_sources(&content.asset_vfs));
    let mut world = build_new_world(&content, ll_world::generate::GenParams::default())
        .expect("默认参数应当建得出世界");

    let races: Vec<_> = registered(&content, true)
        .into_iter()
        .filter(|(id, _)| has_own_sprite(&atlas, id))
        .collect();
    let professions: Vec<_> = registered(&content, false)
        .into_iter()
        .filter(|(id, _)| has_own_sprite(&atlas, id))
        .collect();
    assert!(!races.is_empty(), "一个有图的种族都没有，名册会是空的");

    // 第一列是「不带挂件」的裸身子：用一个必然没有挂件贴图的职业索引
    // 占位。`ContentIndex::default()` 指向注册表 0 号内容，那不是任何
    // 职业，因此挂件那一层查不到东西、什么都不画——正好就是要的效果。
    let columns: Vec<Option<(NamespacedId, ContentIndex)>> = std::iter::once(None)
        .chain(professions.into_iter().map(Some))
        .collect();

    let width = columns.len() as u32 * (CELL_W + GAP) + GAP;
    let height = races.len() as u32 * (CELL_H + GAP) + GAP;
    let mut canvas = RgbaImage::from_pixel(width, height, BACKDROP);

    for (row, (_, race)) in races.iter().enumerate() {
        for (col, column) in columns.iter().enumerate() {
            let profession = column
                .as_ref()
                .map(|(_, index)| *index)
                .unwrap_or_else(ContentIndex::default);
            let npc = spawn_npc(&mut world, *race, profession);
            let keys = npc_keys(&content, &world, &atlas, npc);
            let x = GAP + col as u32 * (CELL_W + GAP);
            let y = GAP + row as u32 * (CELL_H + GAP);
            for key in keys {
                blit(&mut canvas, &atlas, &key, x, y);
            }
        }
    }

    let out = root
        .join("crates/ll-game/tests/visual")
        .join("npc_roster_preview.png");
    upscale(&canvas, ZOOM).save(&out).expect("写入预览 PNG");
    println!(
        "已写出 {}（{} 个种族 × {} 列）",
        out.display(),
        races.len(),
        columns.len()
    );
}

/// 造一个种族/职业指定的 NPC 塞进世界，返回它的槽位。位置无所谓——本
/// 文件只用它选出来的图集键，不用它的坐标。
fn spawn_npc(
    world: &mut GameWorld,
    race: ContentIndex,
    profession: ContentIndex,
) -> ll_world::entity::EntityId {
    let mut agent = world
        .world
        .actors
        .get(world.player)
        .expect("玩家必然存在")
        .clone();
    agent.race = race;
    agent.profession = profession;
    world.world.actors.spawn(agent)
}

/// 这个 NPC 实际会画的图集键，按绘制顺序号升序——生产路径给的顺序，
/// 本文件不自己排。
fn npc_keys(
    content: &LoadedContent,
    world: &GameWorld,
    atlas: &PackedAtlas,
    npc: ll_world::entity::EntityId,
) -> Vec<String> {
    let slot = u64::from(npc.index());
    let mut mine: Vec<_> = npc_draws(&world.world, &content.registry, world.player)
        .into_iter()
        .filter(|draw| {
            draw.entity == NPC_ENTITY_BASE + slot || draw.entity == NPC_BADGE_ENTITY_BASE + slot
        })
        .collect();
    mine.sort_by_key(|draw| draw.entity);
    mine.iter()
        .filter_map(|draw| {
            draw.keys()
                .find(|name| atlas.metadata.lookup(name).is_some())
                .map(str::to_string)
        })
        .collect()
}

/// 把图集里一个条目按 source-over 叠到画布上（透明像素让底下透出来）。
fn blit(canvas: &mut RgbaImage, atlas: &PackedAtlas, key: &str, dst_x: u32, dst_y: u32) {
    let entry = atlas
        .metadata
        .lookup(key)
        .unwrap_or_else(|| panic!("图集里查不到条目 {key}"));
    let rect = entry.rect;
    for row in 0..u32::from(rect.height) {
        for col in 0..u32::from(rect.width) {
            let src = *atlas
                .canvas
                .get_pixel(u32::from(rect.x) + col, u32::from(rect.y) + row);
            if src.0[3] > 0 {
                canvas.put_pixel(dst_x + col, dst_y + row, src);
            }
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
