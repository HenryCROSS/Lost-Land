//! 占位美术生成器：从 `assets/atlas/placeholder.json` 这份既有的布局
//! 规格出发，生成本体资产 VFS 需要的松散贴图树
//! （`assets/sprites/*.png` + `assets/sprites/manifest.json`）。
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
//! `assets/sprites/<name>.png`，并产出配套的 `assets/sprites/manifest.json`
//! （形状见 `ll_mod::asset_vfs` 模块文档「目录约定」一节：`name`/`file`/
//! `pivot`/`footprint`，**不含 `rect`**）。
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
    println!("已生成 {sprite_count} 张松散贴图与 assets/sprites/manifest.json");

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
    std::fs::write(sprites_dir.join("manifest.json"), manifest_json)
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
/// `sprites_dir/<name>.png`，并在同一目录下写出 `manifest.json`。
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
    let manifest_path = sprites_dir.join("manifest.json");
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
    assert_eq!(
        (canvas_width, canvas_height),
        (96, 72),
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
        "hero_walk_0" => sprite::decorate_hero_walk(image, rect, 2),
        "hero_walk_1" => sprite::decorate_hero_walk(image, rect, 10),
        "boss_idle_0" => sprite::decorate_boss(image, rect),
        _ => match terrain::terrain_spec(name) {
            Some(spec) => terrain::decorate_terrain_tile(image, rect, spec),
            None => {
                panic!("不知道如何绘制条目 '{name}'：请在 sprite.rs 或 terrain.rs 里补一份画法")
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
    fn 真实图集json解析后能算出已知的96乘72画布() {
        // 用仓库里真实的 placeholder.json 验证解析与尺寸推导没有脱节——
        // 这条测试会随仓库内容变化，一旦布局被意外改动就会在这里先
        // 炸掉，而不是留到跑生成器时才被 assert_eq! 抓住。
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
        assert_eq!(size, (96, 72));
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
        assert!(out_dir.join("manifest.json").exists());

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
            std::fs::read_to_string(out_dir.join("manifest.json")).expect("清单应已写出");

        // Assert
        assert!(!manifest_text.contains("\"rect\""));

        // Cleanup
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
