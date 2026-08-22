//! 占位美术生成器：从 `assets/atlas/placeholder.json` 这份既有的布局
//! 规格出发，生成本体资产 VFS 需要的松散贴图树
//! （`assets/sprites/*.png` + `assets/sprites/manifest.json5`）。
//!
//! # 定位变化：从「烧图集」到「烧松散贴图」
//!
//! 此前本工具的唯一职责是把 `placeholder.json` 描述的多个条目矩形，
//! 绘制进**同一张**共享画布 `assets/atlas/placeholder.png`——图集本身
//! 在编译期被 `include_bytes!` 烧进二进制（`ll_game::app` 曾经的
//! `ATLAS_JSON`/`ATLAS_PNG` 常量）。资产 VFS 落地后，图集改成运行期由
//! `ll_render::atlas_pack::pack_atlas` 从**松散贴图**现场打包（见其
//! 模块文档「为什么本体资产也要走这条路径」一节）——本体资产因此不
//! 再有「一张预先摆好位置的共享画布」这个概念，`rect.x`/`rect.y` 这两
//! 个坐标字段对新管线毫无意义（矩形摆在图集哪个位置，完全由打包器
//! 决定），只有 `rect.width`/`rect.height`（决定单张贴图多大）与
//! `pivot`/`footprint`（决定怎么摆放）还有意义。
//!
//! 本工具的新主职责因此是 [`generate_loose_sprites`]：给每个条目单独
//! 画一张恰好 `width`×`height` 大小的独立画布，写到
//! `assets/sprites/<name>.png`，并产出配套的 `assets/sprites/manifest.json5`
//! （形状见 `ll_mod::asset_vfs` 模块文档「目录约定」一节：`name`/`file`/
//! `pivot`/`footprint`，**不含 `rect`**）。清单文件名后缀是 `.json5`
//! ——项目所有者 2026-08-20 裁定手写配置统一 JSON5（见
//! `ll_mod::asset_vfs` 模块文档「目录约定」一节），但本工具产出的内容
//! 仍然是普通 JSON：JSON 是 JSON5 的严格子集，生成端不需要注释/尾逗号，
//! 消费端（`ll_mod::asset_vfs`）统一用 `json5::from_str` 读取，两边不需要
//! 维护两套解析器。
//!
//! # 旧的共享画布生成能力保留，但已降级为「遗留」
//!
//! [`generate_legacy_shared_atlas`] 保留了此前逐字的行为（同一批像素
//! 绘制逻辑，画进 `assets/atlas/placeholder.png` 这张共享画布），
//! **不是因为运行期还需要它**——`ll-game` 本体二进制已经完全不读这
//! 张文件——而是因为 `crates/ll-render/examples/p1_acceptance`、
//! `crates/ll-sim/examples/p3_acceptance`/`p5_coordinate_acceptance`、
//! `crates/ll-ui/examples/p4_acceptance`、
//! `crates/ll-world/examples/p2_acceptance` 这五个更早批次的验收 demo
//! 仍然直接 `include_bytes!` 这张共享画布，它们的既有视觉回归基准
//! （冻结的截图比对）绑定的是这张图的具体像素内容——不在本次资产 VFS
//! 任务的范围内去重构五个独立 demo 各自的资产装载方式，因此继续保留
//! 生成它的能力，避免这批 demo 因为找不到最新脚本而悄悄用上一份过期
//! 像素。两个生成函数共用同一批像素绘制逻辑（`sprite.rs`/`terrain.rs`），
//! 不是两份独立实现。

mod color;
mod sprite;
mod terrain;
mod ui;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 图集条目在画布上的像素矩形，字段与 `placeholder.json` 里的 `rect`
/// 一一对应，但类型统一用 `u32`——JSON 里就是普通数字，不像
/// `ll_render::atlas::FrameRect` 那样要跟 GPU 顶点数据打交道、必须卡
/// 在 `u16`。
#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct EntryRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// 精灵锚点，字段形状与 `ll_mod::asset_vfs::SpritePivot`/
/// `ll_render::sprite::Pivot` 逐一对应——本工具是独立二进制，不依赖
/// 工作区任何一个库 crate（`[dependencies]` 只有 `image`/`serde`/
/// `serde_json`），因此各自维护一份同构的镜像类型，而不是共享定义。
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
struct Pivot {
    x: i16,
    y: i16,
}

/// 精灵占地格数，理由同 [`Pivot`]。
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
struct Footprint {
    width: u8,
    height: u8,
}

/// `placeholder.json` 里的一条条目。
#[derive(Debug, Deserialize)]
struct AtlasEntryJson {
    name: String,
    rect: EntryRect,
    pivot: Pivot,
    footprint: Footprint,
}

/// `placeholder.json` 的顶层结构，本工具只用得到 `entries`。
#[derive(Debug, Deserialize)]
struct AtlasJson {
    entries: Vec<AtlasEntryJson>,
}

/// 松散贴图清单里的一条条目——`ll_mod::asset_vfs` 期望的形状，
/// **不含 `rect`**：摆到图集哪个位置由运行期打包器决定，源清单不需要
/// 也不应该知道。
#[derive(Debug, Serialize)]
struct SpriteManifestEntryOut {
    name: String,
    file: String,
    pivot: Pivot,
    footprint: Footprint,
}

#[derive(Debug, Serialize)]
struct SpriteManifestOut {
    entries: Vec<SpriteManifestEntryOut>,
}

fn main() {
    let atlas_dir = atlas_dir();
    let json_path = atlas_dir.join("placeholder.json");
    let json_text = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", json_path.display()));
    let atlas: AtlasJson = serde_json::from_str(&json_text)
        .unwrap_or_else(|error| panic!("解析 {} 失败：{error}", json_path.display()));

    let sprite_count = generate_loose_sprites(&atlas, &sprites_dir());
    println!("已生成 {sprite_count} 张松散贴图与 assets/sprites/manifest.json5");

    generate_legacy_shared_atlas(&atlas, &atlas_dir);
    println!(
        "已重新生成遗留共享图集 {}",
        atlas_dir.join("placeholder.png").display()
    );

    generate_mod_demo_assets(&example_mod_assets_dir());
    println!("已生成 mods/example_mod 的真实资产 VFS 验收 demo（自带精灵 + 资产覆盖）");
}

/// 给 `mods/example_mod` 生成资产 VFS 的两条真实验收路径需要的美术：
///
/// 1. **mod 自带新精灵**——`assets/sprites/lava_floor.png` + 清单，
///    与 `terrain.scm` 注册的 `examplemod:lava_floor` 地形同名，供
///    `ll_game::layout::terrain_atlas_key` 的回退路径查到（见其模块
///    文档）——这是 ADR 0018「不能只靠单元测试自证」要求的真实产出：
///    这张图会在游戏里真的画出来，不只是清单/单测里存在。
/// 2. **mod 覆盖本体资产**——`assets/overrides/lostland/sprites/terrain_dirt.png`，
///    验证「同路径覆盖」在真实 mod 目录布局下也能被资产 VFS 正确解析
///    （见 `ll_mod::asset_vfs` 模块文档），不只是测试夹具里的临时目录
///    场景。
fn generate_mod_demo_assets(mod_assets_dir: &Path) {
    let sprites_dir = mod_assets_dir.join("sprites");
    std::fs::create_dir_all(&sprites_dir)
        .unwrap_or_else(|error| panic!("创建目录 {} 失败：{error}", sprites_dir.display()));

    let lava_rect = EntryRect {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    };
    let mut lava_image = RgbaImage::new(lava_rect.width, lava_rect.height);
    draw_entry(&mut lava_image, "lava_floor", lava_rect);
    lava_image
        .save(sprites_dir.join("lava_floor.png"))
        .expect("写入 lava_floor.png 不应失败");

    let manifest = SpriteManifestOut {
        entries: vec![SpriteManifestEntryOut {
            name: "lava_floor".to_string(),
            file: "lava_floor.png".to_string(),
            // 与本体 `terrain_dirt`（可通行的普通地板）同一档摆放参数
            // ——熔岩地板本身可通行（见 `terrain.scm` 的
            // `register-terrain` 调用），视觉呈现自然也该走同一类
            // 「铺满整格的地面纹理」而非站立单位的锚点/占地设定。
            pivot: Pivot { x: 0, y: 0 },
            footprint: Footprint {
                width: 1,
                height: 1,
            },
        }],
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).expect("序列化清单不应失败");
    std::fs::write(sprites_dir.join("manifest.json5"), manifest_json)
        .expect("写入 example_mod 精灵清单不应失败");

    let override_dir = mod_assets_dir
        .join("overrides")
        .join("lostland")
        .join("sprites");
    std::fs::create_dir_all(&override_dir)
        .unwrap_or_else(|error| panic!("创建目录 {} 失败：{error}", override_dir.display()));
    let override_rect = EntryRect {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    };
    let mut override_image = RgbaImage::new(override_rect.width, override_rect.height);
    draw_entry(
        &mut override_image,
        "examplemod_terrain_dirt_override",
        override_rect,
    );
    override_image
        .save(override_dir.join("terrain_dirt.png"))
        .expect("写入覆盖用 terrain_dirt.png 不应失败");
}

/// `mods/example_mod/assets/` 目录的绝对路径，推导方式同 [`atlas_dir`]
/// ——`tools/ll-artgen` 到仓库根同样固定隔两级 `../..`。
fn example_mod_assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/example_mod/assets")
}

/// 给每个条目单独画一张 `width`×`height` 的独立画布，写到
/// `sprites_dir/<name>.png`，并在同一目录下写出 `manifest.json5`。
/// 返回实际生成的贴图数量。
fn generate_loose_sprites(atlas: &AtlasJson, sprites_dir: &Path) -> usize {
    std::fs::create_dir_all(sprites_dir)
        .unwrap_or_else(|error| panic!("创建目录 {} 失败：{error}", sprites_dir.display()));

    let mut entries = Vec::with_capacity(atlas.entries.len());
    for entry in &atlas.entries {
        let mut image = RgbaImage::new(entry.rect.width, entry.rect.height);
        // 独立画布，本地坐标系原点即 (0, 0)——与共享画布版本唯一的
        // 差异只是 `rect.x`/`rect.y` 归零，绘制逻辑本身完全不变。
        let local_rect = EntryRect {
            x: 0,
            y: 0,
            width: entry.rect.width,
            height: entry.rect.height,
        };
        draw_entry(&mut image, &entry.name, local_rect);

        let file_name = format!("{}.png", entry.name);
        let png_path = sprites_dir.join(&file_name);
        image
            .save(&png_path)
            .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", png_path.display()));

        entries.push(SpriteManifestEntryOut {
            name: entry.name.clone(),
            file: file_name,
            pivot: entry.pivot,
            footprint: entry.footprint,
        });
    }

    let manifest = SpriteManifestOut { entries };
    let manifest_json = serde_json::to_string_pretty(&manifest).expect("序列化清单不应失败");
    let manifest_path = sprites_dir.join("manifest.json5");
    std::fs::write(&manifest_path, manifest_json)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", manifest_path.display()));

    atlas.entries.len()
}

/// 重新生成遗留的共享画布 `assets/atlas/placeholder.png`——理由见模块
/// 文档「旧的共享画布生成能力保留，但已降级为遗留」一节。逐字保留此前
/// 的行为：画布尺寸由全部条目矩形右下角推出，并断言一次已知值作为
/// 「`placeholder.json` 被意外改动」的哨兵。
fn generate_legacy_shared_atlas(atlas: &AtlasJson, atlas_dir: &Path) {
    let entries: Vec<(&str, EntryRect)> = atlas
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.rect))
        .collect();
    let (canvas_width, canvas_height) = canvas_size(&entries);
    // 96×112：96×96（合并资产 VFS 后的既有布局，见下方历史注释）再往下
    // 多出一整行（y 96..112，高 16），装 HUD 皮肤层的四张占位 UI 贴图
    // （`ui_panel_border`/`ui_panel_fill`/`ui_bar_track`/`ui_bar_fill`，
    // 见 `assets/atlas/placeholder.json` 与 `ui.rs`）——这四张贴图的
    // `rect.x`/`rect.y` 全部落在新增的这一整行里,不touch 任何既有条目
    // 的矩形（项目所有者硬要求「别动图集里既有条目的矩形」）。
    //
    // 历史：96×96 是比合并资产 VFS 前的 96×72 多出一整行（y 72..96），
    // 装当时新增的 4 张走路过渡帧（`hero_walk_2..5`，每张 16×24）——这
    // 些帧只在遗留共享画布这条路径里需要摆坐标，松散贴图路径
    // （`generate_loose_sprites`）不关心 `rect.x`/`rect.y`，各自的独立
    // 画布互不影响。
    assert_eq!(
        (canvas_width, canvas_height),
        (96, 112),
        "画布尺寸与已知布局不符，placeholder.json 的条目矩形可能被意外改动"
    );

    let mut image = RgbaImage::new(canvas_width, canvas_height);
    for &(name, rect) in &entries {
        draw_entry(&mut image, name, rect);
    }

    let png_path = atlas_dir.join("placeholder.png");
    image
        .save(&png_path)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", png_path.display()));
}

/// 按条目名把绘制任务分派给地形点缀或角色标志。
///
/// 查不到对应画法时直接 panic 而不是留白：图集画布默认全透明（见
/// `RgbaImage::new`），静默跳过会让新增条目在图片里变成一个看不见的
/// 洞，且不会有任何报错——这正是项目「外部/意外输入只能报错，不能
/// 悄悄错」的一贯原则在这个小工具里的体现，即便这里的「输入」只是
/// 开发者自己往 JSON 里加的新条目。
fn draw_entry(image: &mut RgbaImage, name: &str, rect: EntryRect) {
    match name {
        "hero_idle_0" => sprite::decorate_hero_idle(image, rect),
        "hero_idle_1" => sprite::decorate_hero_idle_breath(image, rect),
        // 六帧行走循环，播放顺序见 `ll_game::animation::player_clips`
        // 等三处剪辑定义里的 `frames` 列表：
        // walk_0(接触) -> walk_2(过渡) -> walk_3(过腿) -> walk_1(接触)
        // -> walk_4(过渡) -> walk_5(过腿) -> 循环回 walk_0。
        // `foot_dx` 取值 2 → 4 → 7 → 10 → 8 → 5 → （回到 2）沿一条单调
        // 折返的路径走，相邻两帧位移只有 2~3 像素，比此前两帧一步位移
        // 8 像素（2 直接跳到 10）更连贯；`passing` 标记脚摆到中线附近
        // 的两帧（walk_3/walk_5），见 `sprite::decorate_hero_walk` 文档。
        // walk_0/walk_1 的像素内容与合并前完全一致（数值未改），只是
        // 现在多了 4 张新的过渡帧穿插在它们之间播放。
        "hero_walk_0" => sprite::decorate_hero_walk(image, rect, 2, false),
        "hero_walk_1" => sprite::decorate_hero_walk(image, rect, 10, false),
        "hero_walk_2" => sprite::decorate_hero_walk(image, rect, 4, false),
        "hero_walk_3" => sprite::decorate_hero_walk(image, rect, 7, true),
        "hero_walk_4" => sprite::decorate_hero_walk(image, rect, 8, false),
        "hero_walk_5" => sprite::decorate_hero_walk(image, rect, 5, true),
        "boss_idle_0" => sprite::decorate_boss(image, rect),
        // 昼夜滑条底图：水平渐变,不是 `TerrainSpec` 能表达的单一主色,
        // 单独按名字分派,见 `ui.rs::decorate_day_night_bar` 文档。
        "ui_daynight_bar" => ui::decorate_day_night_bar(image, rect),
        _ => match terrain::terrain_spec(name).or_else(|| ui::ui_spec(name)) {
            Some(spec) => terrain::decorate_terrain_tile(image, rect, spec),
            None => {
                panic!(
                    "不知道如何绘制条目 '{name}'：请在 sprite.rs、terrain.rs 或 ui.rs 里补一份画法"
                )
            }
        },
    }
}

/// 画布尺寸取全部条目矩形右下角坐标的最大值——只供
/// [`generate_legacy_shared_atlas`] 使用（松散贴图路径下每张画布只
/// 装一个条目，不需要这个计算）。
fn canvas_size(entries: &[(&str, EntryRect)]) -> (u32, u32) {
    let mut width = 0;
    let mut height = 0;
    for &(_, rect) in entries {
        width = width.max(rect.x + rect.width);
        height = height.max(rect.y + rect.height);
    }
    (width, height)
}

/// `assets/atlas/` 目录的绝对路径，相对本 crate 的 `Cargo.toml` 位置
/// 推导（`tools/ll-artgen` 到仓库根的 `assets/atlas` 固定隔两级
/// `../..`），不依赖运行时的当前工作目录——避免「在仓库根跑」和「在
/// 子目录跑」得到不同结果。
fn atlas_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/atlas")
}

/// `assets/sprites/` 目录的绝对路径，推导方式同 [`atlas_dir`]。
fn sprites_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sprites")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 画布尺寸取全部条目右下角的最大值() {
        // Arrange
        let entries = vec![
            (
                "a",
                EntryRect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                },
            ),
            (
                "b",
                EntryRect {
                    x: 20,
                    y: 5,
                    width: 4,
                    height: 4,
                },
            ),
        ];

        // Act
        let size = canvas_size(&entries);

        // Assert
        assert_eq!(size, (24, 10));
    }

    #[test]
    fn 真实图集json解析后能算出已知的96乘112画布() {
        // 用仓库里真实的 placeholder.json 验证解析与尺寸推导没有脱节——
        // 这条测试会随仓库内容变化，一旦布局被意外改动就会在这里先
        // 炸掉，而不是留到跑生成器时才被 assert_eq! 抓住。112（不是
        // 96）是 HUD 皮肤层四张占位 UI 贴图新增一整行之后的当前尺寸，
        // 见 `generate_legacy_shared_atlas` 里的同一条断言与其历史注释。
        // Arrange
        let json_text = std::fs::read_to_string(atlas_dir().join("placeholder.json"))
            .expect("仓库应自带 placeholder.json");
        let atlas: AtlasJson = serde_json::from_str(&json_text).expect("应是合法 JSON");
        let entries: Vec<(&str, EntryRect)> = atlas
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.rect))
            .collect();

        // Act
        let size = canvas_size(&entries);

        // Assert
        assert_eq!(size, (96, 112));
    }

    #[test]
    fn 未知条目名会panic而不是静默留空() {
        // Arrange
        let mut image = RgbaImage::new(16, 16);
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        };

        // Act：捕获 panic，验证「查不到画法就报错」这条约定，见
        // draw_entry 的文档。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            draw_entry(&mut image, "terrain_lava", rect);
        }));

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 松散贴图生成的条目数与源json一致() {
        // Arrange
        let atlas = AtlasJson {
            entries: vec![AtlasEntryJson {
                name: "hero_idle_0".to_string(),
                rect: EntryRect {
                    x: 0,
                    y: 0,
                    width: 16,
                    height: 24,
                },
                pivot: Pivot { x: 8, y: 24 },
                footprint: Footprint {
                    width: 1,
                    height: 1,
                },
            }],
        };
        let out_dir = std::env::temp_dir().join(format!(
            "ll-artgen-loose-sprites-test-{}",
            std::process::id()
        ));

        // Act
        let count = generate_loose_sprites(&atlas, &out_dir);

        // Assert
        assert_eq!(count, 1);
        assert!(out_dir.join("hero_idle_0.png").exists());
        assert!(out_dir.join("manifest.json5").exists());

        // Cleanup
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn 松散贴图清单不含rect字段() {
        // 资产 VFS 期望的形状里，摆放位置由运行期打包器决定，源清单
        // 不应该携带 rect——这条测试直接检查产出的 JSON 文本，防止
        // 未来有人手滑把 rect 加回序列化结构体。
        // Arrange
        let atlas = AtlasJson {
            entries: vec![AtlasEntryJson {
                name: "hero_idle_0".to_string(),
                rect: EntryRect {
                    x: 0,
                    y: 0,
                    width: 16,
                    height: 24,
                },
                pivot: Pivot { x: 8, y: 24 },
                footprint: Footprint {
                    width: 1,
                    height: 1,
                },
            }],
        };
        let out_dir = std::env::temp_dir().join(format!(
            "ll-artgen-manifest-shape-test-{}",
            std::process::id()
        ));

        // Act
        generate_loose_sprites(&atlas, &out_dir);
        let manifest_text =
            std::fs::read_to_string(out_dir.join("manifest.json5")).expect("清单应已写出");

        // Assert
        assert!(!manifest_text.contains("\"rect\""));

        // Cleanup
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn 连续生成两次松散贴图产出逐位相同的文件() {
        // 「确定性」是资产 VFS 依赖的前提（见本工具模块文档、
        // `ll_render::atlas_pack` 模块文档「确定性」一节）：图集内容若
        // 不确定，将来的资产哈希与测试都会漂。本测试直接对生成出的
        // PNG/JSON 文件字节比对，而不是只比对内存里的像素缓冲区——
        // 覆盖到 PNG 编码这一步本身是否确定性的问题。
        // Arrange：用真实仓库的 placeholder.json，覆盖全部条目而不是
        // 挑一个，六帧新增的行走过渡帧也在其中。
        let json_text = std::fs::read_to_string(atlas_dir().join("placeholder.json"))
            .expect("仓库应自带 placeholder.json");
        let atlas: AtlasJson = serde_json::from_str(&json_text).expect("应是合法 JSON");
        let first_dir = std::env::temp_dir().join(format!(
            "ll-artgen-determinism-first-{}",
            std::process::id()
        ));
        let second_dir = std::env::temp_dir().join(format!(
            "ll-artgen-determinism-second-{}",
            std::process::id()
        ));

        // Act：各自独立生成一遍，互不复用任何缓存或中间状态。
        generate_loose_sprites(&atlas, &first_dir);
        generate_loose_sprites(&atlas, &second_dir);

        // Assert：manifest 与每张贴图的文件字节逐一相同。
        for entry in &atlas.entries {
            let file_name = format!("{}.png", entry.name);
            let first_bytes =
                std::fs::read(first_dir.join(&file_name)).expect("第一次生成应写出该文件");
            let second_bytes =
                std::fs::read(second_dir.join(&file_name)).expect("第二次生成应写出该文件");
            assert_eq!(first_bytes, second_bytes, "{file_name} 两次生成的字节不同");
        }
        let first_manifest = std::fs::read(first_dir.join("manifest.json5")).expect("应已写出");
        let second_manifest = std::fs::read(second_dir.join("manifest.json5")).expect("应已写出");
        assert_eq!(first_manifest, second_manifest);

        // Cleanup
        let _ = std::fs::remove_dir_all(&first_dir);
        let _ = std::fs::remove_dir_all(&second_dir);
    }

    /// 六帧行走循环的播放顺序，与三处消费方（`ll_game::animation::player_clips`、
    /// `p1_acceptance`、`p5_coordinate_acceptance` 的 `walk_clip`）保持
    /// 一致：接触 → 过渡 → 过腿 → 接触 → 过渡 → 过腿 → 循环回接触。
    const WALK_CYCLE_ORDER: [&str; 6] = [
        "hero_walk_0",
        "hero_walk_2",
        "hero_walk_3",
        "hero_walk_1",
        "hero_walk_4",
        "hero_walk_5",
    ];

    /// 统计两张同尺寸图片有多少像素不同——相邻帧像素差异的度量单位，
    /// 与项目所有者实测「待机两帧差 8/384」「行走两帧差 32/384」
    /// 「行走对待机差 48/384」用的是同一种量法，方便直接对照。
    fn count_differing_pixels(a: &RgbaImage, b: &RgbaImage) -> usize {
        assert_eq!(a.dimensions(), b.dimensions(), "只应比较同尺寸的两帧");
        a.pixels().zip(b.pixels()).filter(|(p, q)| p != q).count()
    }

    #[test]
    fn 六帧行走循环相邻帧像素差异全部小于两帧方案的直接互跳() {
        // 合并前只有两张走姿贴图，直接互跳的差异是 32/384（见
        // `animation.rs`/`sprite.rs` 模块文档引用的项目所有者实测
        // 数字）。本测试断言扩成六帧后，循环里**每一对相邻帧**
        // （含首尾循环回去那一对）的差异都严格小于 32——即六帧方案
        // 里没有任何一步比原来两帧方案的那一次硬切更突兀；同时差异必须
        // 大于 0，确保六帧不是复制粘贴出来的重复贴图。
        // Arrange
        const HERO_RECT: EntryRect = EntryRect {
            x: 0,
            y: 0,
            width: 16,
            height: 24,
        };
        let frames: Vec<RgbaImage> = WALK_CYCLE_ORDER
            .iter()
            .map(|&name| {
                let mut image = RgbaImage::new(HERO_RECT.width, HERO_RECT.height);
                draw_entry(&mut image, name, HERO_RECT);
                image
            })
            .collect();

        // Act & Assert：逐对相邻帧（含循环回第一帧的那一对）比较。
        for i in 0..frames.len() {
            let next = (i + 1) % frames.len();
            let diff = count_differing_pixels(&frames[i], &frames[next]);
            assert!(
                diff > 0,
                "{} 与 {} 应有可见差异，不应是同一张图",
                WALK_CYCLE_ORDER[i],
                WALK_CYCLE_ORDER[next]
            );
            assert!(
                diff < 32,
                "{} 与 {} 差异 {diff} 像素，应小于两帧方案直接互跳的 32 像素基准",
                WALK_CYCLE_ORDER[i],
                WALK_CYCLE_ORDER[next]
            );
        }
    }
}
