//! 把本帧要画的精灵推入 `SpriteBatch`：地形瓦片、玩家。
//!
//! 拆成自由函数而非 `Demo` 的方法，理由与 p1/p2/p3_acceptance 一致：
//! `on_frame` 需要同时持有 `&mut self.resources` 与 `self` 的其余
//! 字段，写成 `&self` 方法会让借用检查器把两者混为一谈。

use ll_core::torus::TorusPos;
use ll_render::camera::Camera;
use ll_render::sprite::{DrawOrder, Layer, footprint_bottom_screen_y, sprite_draw_position};
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
        // demo 世界是单区块布局，WorldState::new 的出生点邻域预热已让
        // 它整体常驻，见 `world::is_walkable` 文档同一节。
        let kind = world
            .terrain_at(pos)
            .expect("demo 世界是单区块布局，已整体常驻");
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

/// 画出玩家精灵。
///
/// 占地锚点、图像左上角的换算全部走 `ll_render::sprite` 的公开函数
/// （[`sprite_draw_position`]/[`footprint_bottom_screen_y`]），不再在本
/// demo 里重复实现——这条换算曾经在四个验收 demo 里各写一份、还漏了
/// 第五份（见 `ll_render::sprite::sprite_draw_position` 文档「调用方
/// 不得自行重实现这条换算」一节），统一收口才能保证改一处、处处生效。
pub(crate) fn push_player(pos: TorusPos, camera: &Camera, resources: &mut GpuResources) {
    let Some((entry, uv)) = resources.lookup("hero_idle_0") else {
        return;
    };
    let footprint = entry.footprint;
    let sprite_size = entry.sprite_size();
    let (tile_x, tile_y) = camera.world_to_screen(pos);
    let [px, py] = sprite_draw_position((tile_x, tile_y), footprint, entry.pivot);
    let order = DrawOrder::new(
        Layer::ENTITY,
        footprint_bottom_screen_y(tile_y, footprint.height),
        PLAYER_ENTITY_BASE,
    );
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
