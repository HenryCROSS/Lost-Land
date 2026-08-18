//! 把本帧要画的精灵推入 `SpriteBatch`：地形瓦片、玩家。
//!
//! 拆成自由函数而非 `Demo` 的方法，理由与 p1/p2/p3_acceptance 一致：
//! `on_frame` 需要同时持有 `&mut self.resources` 与 `self` 的其余
//! 字段，写成 `&self` 方法会让借用检查器把两者混为一谈。

use ll_core::torus::TorusPos;
use ll_render::camera::Camera;
use ll_render::sprite::{DrawOrder, Layer};
use ll_world::state::WorldState;
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

use crate::GpuResources;
use crate::layout::terrain_entry_name_and_tint;

/// 绘制顺序号：地形瓦片的起始偏移。
const TERRAIN_ENTITY_BASE: u64 = 1;
/// 绘制顺序号：玩家精灵，远大于地形瓦片可能用到的最大值，避免撞车
/// ——理由与 p1/p2/p3_acceptance 一致。
const PLAYER_ENTITY_BASE: u64 = 1_000_000;

/// 画出相机视口内的地形瓦片，`examplemod:lava_floor` 按
/// [`terrain_entry_name_and_tint`] 的规则染色。
pub(crate) fn push_terrain(
    world: &WorldState,
    terrain_ids: &BaseTerrainIds,
    lava_kind: Option<TerrainKind>,
    camera: &Camera,
    resources: &mut GpuResources,
) {
    for pos in camera.visible_tiles() {
        let kind = world.terrain.terrain_at(pos);
        let Some((name, tint)) = terrain_entry_name_and_tint(kind, terrain_ids, lava_kind) else {
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
            ll_render::batch::SpriteInstance {
                position: [sx as f32, sy as f32],
                size: [
                    entry.sprite_size().width as f32,
                    entry.sprite_size().height as f32,
                ],
                uv_rect: uv,
                color: tint,
            },
        );
    }
}

/// 占地格块锚点像素坐标（占地矩形底边中点）——与
/// p1/p2/p3_acceptance 的 `footprint_anchor_pixel` 同一算法：横向取
/// 占地宽度一半，纵向取占地高度（贴底边）。
fn footprint_anchor_pixel(
    tile_origin: (i32, i32),
    footprint: ll_render::sprite::Footprint,
) -> (i32, i32) {
    let half_width_px = footprint.width as i32 * ll_render::sprite::TILE_SIZE as i32 / 2;
    let height_px = footprint.height as i32 * ll_render::sprite::TILE_SIZE as i32;
    (tile_origin.0 + half_width_px, tile_origin.1 + height_px)
}

/// 画出玩家精灵。
pub(crate) fn push_player(pos: TorusPos, camera: &Camera, resources: &mut GpuResources) {
    let Some((entry, uv)) = resources.lookup("hero_idle_0") else {
        return;
    };
    let footprint = entry.footprint;
    let pivot = entry.pivot;
    let sprite_size = entry.sprite_size();
    let (tile_x, tile_y) = camera.world_to_screen(pos);
    let (anchor_x, anchor_y) = footprint_anchor_pixel((tile_x, tile_y), footprint);
    let px = (anchor_x - pivot.x as i32) as f32;
    let py = (anchor_y - pivot.y as i32) as f32;
    let order = DrawOrder::new(Layer::ENTITY, anchor_y, PLAYER_ENTITY_BASE);
    resources.batch.push(
        order,
        ll_render::batch::SpriteInstance {
            position: [px, py],
            size: [sprite_size.width as f32, sprite_size.height as f32],
            uv_rect: uv,
            color: [1.0, 1.0, 1.0, 1.0],
        },
    );
}
