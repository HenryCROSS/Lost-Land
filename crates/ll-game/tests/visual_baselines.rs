//! 三张**纯 CPU** 视觉基准：地表内容、据点建筑地形、NPC 点名册。每条测试
//! 出一张图，与 `crates/ll-game/tests/visual/` 下同名的基准 PNG **逐像素**
//! 比对。
//!
//! 「每张图应当能一眼看见什么」写在 `crates/ll-game/tests/visual/README.md`，
//! 比对机制与 `LL_BLESS_VISUAL=1` 的规矩写在 `tests/visual_support/mod.rs`。
//!
//! # 这三条测试从哪来
//!
//! 它们此前是 `crates/ll-game/examples/{surface,settlement,npc_roster}_preview.rs`
//! 三个 example target。2026-08-29 项目所有者裁定去掉 `examples/`
//! （[ADR 0030](../../../knowledge/decisions/0030-remove-examples-acceptance-demos.md)），
//! 生产者随之删除，三张图变成**无法重新生成的历史留档**——而 ADR 0030
//! 「后果」一节把「把生成逻辑搬成测试」列为三条路之一，所有者批准走这一条。
//! 实施计划：`docs/superpowers/plans/2026-08-31-batch27-visual-baselines.md`。
//!
//! **不许改回 example**：`scripts/ci/check_no_examples.sh` 恒红，且它不接受
//! 「加进某张清单」这种消红方式。
//!
//! # 这不是第二个渲染器，别把它当渲染器用
//!
//! 本文件**只做三件事**：摆场景（哪一格放什么）、把真实图集画布上的矩形按
//! 生产代码算出来的位置拷进一张画布、整数放大存盘。**位置、层序、图集键的
//! 选择一行都没有自己重推**——这一点是被删的 example 的模块文档反复强调的，
//! 搬家之后不许退化。逐条对照：
//!
//! | 这件事 | 由谁算 |
//! | --- | --- |
//! | 世界长什么样 | [`ll_game::world::build_new_world`]（固定种子） |
//! | 图集怎么打 | [`ll_game::app::load_sprite_sources`] + [`ll_render::atlas_pack::pack_atlas`] |
//! | 地表内容画在哪一格、用哪个键 | [`ll_game::surface_draw::surface_draws`] |
//! | NPC 两层用哪两个键、谁压制谁 | [`ll_game::surface_draw::npc_draws`] + `SurfaceDraw::superseded_by` |
//! | 地形该查哪个图集键 | [`ll_game::layout::terrain_atlas_key`] |
//! | 世界坐标 → 屏幕坐标 | [`ll_render::camera::Camera::world_to_screen`] |
//! | 精灵落笔位置（锚点/脚印） | [`ll_render::sprite::sprite_draw_position`] |
//! | 谁挡住谁 | [`ll_render::sprite::DrawOrder`] + [`ll_render::sprite::footprint_bottom_screen_y`] |
//!
//! 也就是说：图里任何一格画错/画不出来，说明**生产代码**真的有问题，不是
//! 这个文件自己的锅。
//!
//! # 它证明什么，不证明什么
//!
//! **证明**：什么东西被画在了哪一格、谁挡住谁、每一格查的是哪个图集键、
//! 那个键在真实图集里对应的像素长什么样。
//!
//! **不证明**：颜色对不对。真实渲染路径会按 `tint` 给每个精灵乘一次颜色
//! （昼夜/天气/迷雾），这三张图恒按原色拷贝。颜色那一侧仍然只有 GPU demo
//! 能验，见 `crates/ll-render/tests/visual/README.md`。
//!
//! # 每条断言的反例是什么（本批开发中真的逐条改坏跑过）
//!
//! [ADR 0022](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)
//! 要求每条断言都用**故意改坏**的反例验证它真的会红。**注意编号是 0022 不是
//! 0018**——仓库刚在批次 25 扫除过 68 处这个误引。下面全部实跑过：
//!
//! | 改坏什么 | 哪条变红 | 红的理由 |
//! | --- | --- | --- |
//! | 把 `surface_preview.png` 换成它自己的**水平翻转版**（尺寸一模一样） | 地表内容 | `158432 / 479232 个像素不同`，**没有**报尺寸——证明咬住的是像素本身，不是「只比尺寸」 |
//! | 把 `settlement_preview.png` 换成名册那张图 | 据点建筑地形 | `尺寸不同：基准 1088×1048，实际 768×576` |
//! | `placed_furniture_draws` 的 `preferred_keys` 清空 | 地表内容 | 2928 个像素——锻炉退回通用紫罗兰箱体 |
//! | `npc_draws` 的 `body_keys` 不再优先取合成图 | 地表内容 + 名册 | 4192 / 291472 个像素——九族全部退回分层裸身子 |
//! | `terrain_atlas_key` 让关着的门冒充开着的门 | 据点建筑地形 + 地表内容 | 各 1248 个像素——正好是那一格门上两张图不同的部分 |
//! | `ll_world::settlement::grid_to_tile` 改回街道落地前的恒 1 格间距 | 地表内容 | 反过来**变绿**：与批次 13 那张历史留档逐位相同（见下方「逐位确定性」） |
//!
//! 每次都确认了**没变红的那些是该绿的**：名册不画地形，因此 `terrain_atlas_key`
//! 改坏时它必须绿；据点平面图不走 `surface_draw`，因此那两处改坏时它必须绿。
//!
//! ## 一条**没**咬住的，如实记在这里
//!
//! 把 `npc_draws` 里职业挂件层的 `superseded_by: composites` 清空（挂件不再
//! 被合成图压制）之后，**名册这张图仍然全绿**。查明原因不是测试写松了，而是
//! 那次覆盖在像素上是**空操作**：`tools/ll-artgen` 的合成图与独立的职业挂件
//! 贴图调的是**同一个** `npc::draw_profession_badge(image, rect, badge)`
//! （`composite.rs:395` 与 `main.rs:385`），同一块 6×6、同一个 `rect`，因此
//! 把挂件再叠一遍得到逐位相同的像素。
//!
//! **也就是说这张图证不了「挂件被压制」这条性质。** 它由
//! `tests/surface_render.rs` 那一侧的键选择断言守着，本文件不假装守到了。
//!
//! # 逐位确定性：实测结论
//!
//! 纯 CPU 出图**实测逐位确定**，而且跨越了十几个批次与一次「example → 测试」
//! 的搬家：`settlement_preview.png` 是批次 13 被删的那个 example 写出来的，
//! 本文件重新出的图与它**一个像素都不差**。`surface_preview.png` 同样——把
//! 唯一那处相关的生产改动（街道与巷宽）临时改回去，它也逐位回到那张历史留档。
//!
//! # 与「给机器看的那一半」的分工
//!
//! `tests/surface_render.rs`、`tests/atlas_coverage.rs`、`tests/npc_appearance.rs`
//! 断言的是「选中了哪个键、那块矩形有没有不透明像素、两两之间差多少」——
//! 那些判据都不看**画面整体**。本文件补的正是那一半：整幅画面一个像素都
//! 不许变。两边合起来才是完整证据。

mod visual_support;

use image::{Rgba, RgbaImage};
use ll_core::ident::{ContentIndex, NamespacedId};
use ll_game::app::load_sprite_sources;
use ll_game::content::{LoadedContent, load_content};
use ll_game::surface_draw::{
    NPC_BADGE_ENTITY_BASE, NPC_ENTITY_BASE, PLAYER_ENTITY, SurfaceDraw, npc_draws, surface_draws,
};
use ll_game::world::{GameWorld, build_new_world};
use ll_render::atlas_pack::{PackedAtlas, pack_atlas};
use ll_render::camera::Camera;
use ll_render::sprite::{
    DrawOrder, Layer, TILE_SIZE, footprint_bottom_screen_y, sprite_draw_position,
};
use ll_sim::item::ItemStack;
use ll_world::item::GroundItemStack;
use ll_world::terrain::{BaseTerrainIds, TerrainKind};

use visual_support::{assert_matches_baseline, blit_copy, blit_over, repo_root, upscale};

/// 存盘前的整数放大倍数，三张图共用。16 像素的瓦片在看图工具里太小，看不出
/// 门把手、窗棂、胸口 6×6 徽记的形状。
///
/// **刻意不降到 1×**：1× 图信息量完全等价（最近邻放大不产生新信息）且省
/// 16 倍像素，但这三张基准的既定用途是「给人看」（README 首句），而且降
/// 尺寸会让本次搬迁失去与历史留档逐张比差异的判据。三张图合计一两百 KB，
/// 对仓库体积不构成问题。
const ZOOM: u32 = 4;

/// 三张图共用的固定世界种子——`tests/surface_render.rs` 用的也是这个值。
/// 固定种子是基准可比对的前提。
const WORLD_SEED: u64 = 20260826;

/// 装真实 `mods/` + `assets/`，与 `tests/surface_render.rs` 同一条推导。
fn real_content() -> LoadedContent {
    let root = repo_root();
    load_content(&root.join("mods"), &root.join("assets")).expect("真实 mods/ 应当装得起来")
}

/// 用生产路径把真实资产打成图集——与 `GpuResources::new` 跑的是同两步。
fn real_atlas(content: &LoadedContent) -> PackedAtlas {
    pack_atlas(&load_sprite_sources(&content.asset_vfs))
}

/// 固定种子的真实世界。
fn real_world(content: &LoadedContent) -> GameWorld {
    build_new_world(
        content,
        ll_world::generate::GenParams {
            seed: WORLD_SEED,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("默认参数应当建得出世界")
}

/// 按图集里查得到的第一个键取（与 `GpuResources::lookup_first` 同一条次序：
/// 优先内容自带键，兜底通用记号）。
fn first_present(atlas: &PackedAtlas, draw: &SurfaceDraw) -> Option<String> {
    draw.keys()
        .find(|name| atlas.metadata.lookup(name).is_some())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// 一、地表内容：surface_preview
// ---------------------------------------------------------------------------

/// 预览画布尺寸（格）——刻意比 640×360 的离屏目标小一大截：这张图是给人
/// 看的，只需要把玩家周围那几格摆的东西看清楚。
const PREVIEW_TILES_X: i32 = 13;
/// 同上。
const PREVIEW_TILES_Y: i32 = 9;

#[test]
fn 地表内容预览与基准逐像素一致() {
    // Arrange
    let content = real_content();
    let mut world = real_world(&content);
    let atlas = real_atlas(&content);
    arrange_surface_scene(&mut world, &content);

    let player_pos = world.world.actors.get(world.player).expect("玩家存在").pos;
    let camera = Camera {
        center: player_pos,
        world: world.world.size,
    };

    // Act：收集全部绘制项——地形一批 + 地表内容一批 + 玩家标记一条，全部
    // 带上生产代码里那一套 `DrawOrder`，最后统一排序。排序规则本身来自
    // `ll_render`，本文件不自己定义「谁挡住谁」。
    let mut items: Vec<(DrawOrder, String, (i32, i32))> = Vec::new();
    let world_width = u64::from(world.world.size.width());
    // 多铺一圈（`+ 1`）：预览画布的原点相对瓦片网格未必整格对齐，边缘那一
    // 列/一行若只铺到正好的范围，会在画布边上留一条没画到的黑边。
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
            if atlas.metadata.lookup(&name).is_none() {
                continue;
            }
            let (sx, sy) = camera.world_to_screen(pos);
            items.push((
                DrawOrder::new(
                    Layer::TERRAIN,
                    sy,
                    1 + pos.y() as u64 * world_width + pos.x() as u64,
                ),
                name,
                (sx, sy),
            ));
        }
    }

    for draw in surface_draws(&world.world, &content.registry, world.player) {
        let Some(name) = first_present(&atlas, &draw) else {
            continue;
        };
        let (sx, sy) = camera.world_to_screen(draw.pos);
        let entry = atlas.metadata.lookup(&name).expect("刚查到过");
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
            player_sprite.to_string(),
            (sx, sy),
        ));
    }

    items.sort_by_key(|(order, _, _)| *order);

    // Assert
    let canvas = compose_surface(&atlas, &items, camera, player_pos);
    assert_matches_baseline("surface_preview", &upscale(&canvas, ZOOM));
}

/// 摆一个能把四类记号一次看全的场景。
///
/// 两个 NPC **刻意取不同的种族与职业**（矮人铁匠与人类渔夫）：「所有 NPC
/// 长得一模一样」正是所有者报的现象，这张预览图如果只摆两个同族同职业的
/// 人，恰好会把那个现象重新藏起来。
fn arrange_surface_scene(world: &mut GameWorld, content: &LoadedContent) {
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

    for ((dx, dy), race, profession) in [
        ((0, -2), content.race_ids.dwarf, "lostland:blacksmith"),
        ((1, 2), content.race_ids.human, "lostland:fisher"),
    ] {
        let mut npc = player.clone();
        npc.pos = world.world.size.wrap(px + dx, py + dy);
        npc.race = race;
        if let Some(profession) = content.registry.get(&parse_id(profession)) {
            npc.profession = profession;
        }
        world.world.actors.spawn(npc);
    }
}

fn parse_id(id: &str) -> NamespacedId {
    NamespacedId::parse(id).expect("字面量合法")
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
        .get(&parse_id(id))
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

/// 把排好序的绘制项按顺序拷到画布上。
fn compose_surface(
    atlas: &PackedAtlas,
    items: &[(DrawOrder, String, (i32, i32))],
    camera: Camera,
    center: ll_core::torus::TorusPos,
) -> RgbaImage {
    let width = (PREVIEW_TILES_X as u32) * TILE_SIZE;
    let height = (PREVIEW_TILES_Y as u32) * TILE_SIZE;
    // 预览画布的原点相对离屏目标原点的偏移：把相机中心那一格摆到预览画布
    // 正中。
    let (cx, cy) = camera.world_to_screen(center);
    let origin_x = cx - (width as i32) / 2 + TILE_SIZE as i32 / 2;
    let origin_y = cy - (height as i32) / 2 + TILE_SIZE as i32 / 2;

    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([12, 12, 16, 255]));
    for (_, name, (sx, sy)) in items {
        let entry = atlas.metadata.lookup(name).expect("已经查到过");
        let [dx, dy] = sprite_draw_position((*sx, *sy), entry.footprint, entry.pivot);
        blit_over(
            &mut canvas,
            atlas,
            name,
            dx as i32 - origin_x,
            dy as i32 - origin_y,
        );
    }
    canvas
}

// ---------------------------------------------------------------------------
// 二、据点建筑地形：settlement_preview
// ---------------------------------------------------------------------------

/// 平面图里一个格子的字符编码。手写成字符画而不是二维枚举数组，是因为
/// 「这间屋子长什么样」在字符画里一眼能看出来，在数组字面量里看不出来。
///
/// - `#` 石墙　`=` 木墙　`.` 木地板　`,` 石地板
/// - `+` 关着的门　`/` 开着的门　`o` 窗
/// - `<` 上楼梯　`>` 下楼梯　`~` 草地（屋外）
///
/// **为什么手工摆平面图而不用真实世界**：世界生成不保证玩家出生点附近一定
/// 有据点，靠随机世界去看这九种地形是碰运气。手工摆的只有「哪一格是哪种
/// 地形」这一件事，每一格该查哪个图集键仍走 `terrain_atlas_key`。
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

#[test]
fn 据点建筑地形预览与基准逐像素一致() {
    // Arrange
    let content = real_content();
    let atlas = real_atlas(&content);
    let rows = FLOOR_PLAN.len() as u32;
    let cols = FLOOR_PLAN[0].chars().count() as u32;
    assert!(
        FLOOR_PLAN
            .iter()
            .all(|row| row.chars().count() as u32 == cols),
        "平面图各行长度必须一致"
    );

    // Act
    let mut canvas = RgbaImage::new(cols * TILE_SIZE, rows * TILE_SIZE);
    for (row_index, row) in FLOOR_PLAN.iter().enumerate() {
        for (col_index, cell) in row.chars().enumerate() {
            let kind = terrain_of(cell, &content.terrain_ids);
            // 生产路径：这一格该查哪个图集键，由 `terrain_atlas_key` 决定，
            // 本文件不自己推。
            let key =
                ll_game::layout::terrain_atlas_key(kind, &content.terrain_ids, &content.registry)
                    .unwrap_or_else(|| panic!("地形字符 {cell:?} 算不出图集键"));
            blit_copy(
                &mut canvas,
                &atlas,
                &key,
                (col_index as u32 * TILE_SIZE) as i32,
                (row_index as u32 * TILE_SIZE) as i32,
            );
        }
    }

    // Assert
    assert_matches_baseline("settlement_preview", &upscale(&canvas, ZOOM));
}

/// 把平面图字符翻译成地形种类。未知字符直接 panic——静默跳过会在图上留一个
/// 看不见的洞，与 `ll-artgen` 的 `draw_entry` 同一条原则。
fn terrain_of(cell: char, ids: &BaseTerrainIds) -> TerrainKind {
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

// ---------------------------------------------------------------------------
// 三、NPC 点名册：npc_roster_preview
// ---------------------------------------------------------------------------

/// 一格的宽高，与 NPC 贴图同尺寸。
const CELL_W: u32 = 16;
/// 同上。
const CELL_H: u32 = 24;
/// 格与格之间的留白（像素），免得相邻两个 NPC 的轮廓糊在一起。
const GAP: u32 = 2;
/// 预览底色（深石板灰）。**不来自图集**：这张图验的是 NPC 自己长什么样，
/// 铺一层真实地形只会多一个与验收无关的变量；但完全透明的底又会让深色像素
/// 在看图工具里读不出来，因此铺一个中性的暗色。
const BACKDROP: Rgba<u8> = Rgba([38, 40, 46, 255]);

#[test]
fn npc点名册预览与基准逐像素一致() {
    // Arrange
    let content = real_content();
    let atlas = real_atlas(&content);
    let mut world = real_world(&content);

    // 行与列都从注册表现查（按 `race_table`/`class_table` 的 `is_defined`
    // 过滤，再筛掉「注册了但没配图」的那些）。因此**加第 10 个种族之后重新
    // 跑一遍，它自己会出现在图上**，不需要有人记得回来改这个文件。
    let races: Vec<_> = registered(&content, true)
        .into_iter()
        .filter(|(id, _)| has_own_sprite(&atlas, id))
        .collect();
    let professions: Vec<_> = registered(&content, false)
        .into_iter()
        .filter(|(id, _)| has_own_sprite(&atlas, id))
        .collect();
    assert!(!races.is_empty(), "一个有图的种族都没有，名册会是空的");

    // 第一列是「不带挂件」的裸身子：用一个必然没有挂件贴图的职业索引占位。
    // `ContentIndex::default()` 指向注册表 0 号内容，那不是任何职业，因此
    // 挂件那一层查不到东西、什么都不画——正好就是要的效果。
    let columns: Vec<Option<ContentIndex>> = std::iter::once(None)
        .chain(professions.into_iter().map(|(_, index)| Some(index)))
        .collect();

    // Act
    let width = columns.len() as u32 * (CELL_W + GAP) + GAP;
    let height = races.len() as u32 * (CELL_H + GAP) + GAP;
    let mut canvas = RgbaImage::from_pixel(width, height, BACKDROP);
    for (row, (_, race)) in races.iter().enumerate() {
        for (col, column) in columns.iter().enumerate() {
            let profession = column.unwrap_or_default();
            let npc = spawn_npc(&mut world, *race, profession);
            let x = GAP + col as u32 * (CELL_W + GAP);
            let y = GAP + row as u32 * (CELL_H + GAP);
            for key in npc_keys(&content, &world, &atlas, npc) {
                blit_over(&mut canvas, &atlas, &key, x as i32, y as i32);
            }
        }
    }

    // Assert
    assert_matches_baseline("npc_roster_preview", &upscale(&canvas, ZOOM));
}

/// 注册表里全部已定义属性的种族/职业，按注册顺序（`snapshot` 是 `Vec`，不经
/// 任何哈希容器——约束 C5）。
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

/// 这条内容有没有自带贴图——名册只列有图的那些，没图的种族/职业在图上只会
/// 是一片一模一样的通用记号，列出来没有信息量。
fn has_own_sprite(atlas: &PackedAtlas, id: &NamespacedId) -> bool {
    atlas.metadata.lookup(&id.to_string()).is_some()
}

/// 造一个种族/职业指定的 NPC 塞进世界，返回它的槽位。位置无所谓——本文件
/// 只用它选出来的图集键，不用它的坐标。
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

/// 这个 NPC 实际会画的图集键，按绘制顺序号升序——生产路径给的顺序，本文件
/// 不自己排。
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
        // 被压制的那一层整个不画——与生产路径上的 `push_surface_draw` 第一句
        // 逐字一致。身子命中「种族 × 职业」合成图时，职业挂件层必须让位，
        // 否则这张预览图会画出屏幕上根本不存在的第二枚记号。
        .filter(|draw| {
            !draw
                .superseded_by
                .iter()
                .any(|key| atlas.metadata.lookup(key).is_some())
        })
        .filter_map(|draw| first_present(atlas, draw))
        .collect()
}
