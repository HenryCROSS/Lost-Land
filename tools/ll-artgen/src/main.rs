//! 占位图集生成器：把 `assets/atlas/placeholder.json` 里的每个条目按
//! 名字分派给地形点缀（见 `terrain.rs`）或角色标志（见 `sprite.rs`）
//! 绘制到一张新画布上，覆盖写回 `assets/atlas/placeholder.png`。
//!
//! # 为什么是代码生成而不是手工画图
//!
//! 1. 点缀像素的位置由「格子坐标 + 像素坐标」哈希决定（见
//!    `terrain::decorate_terrain_tile`），手工画不出这种确定性图案，
//!    也没法在改一个参数后精确复现。
//! 2. 配色参数（色相偏移、明度偏移）集中在 `terrain.rs` 的一张表里，
//!    调整某个地形的点缀效果是改一个数字再重新跑一遍，不是重新画图。
//! 3. 图集条目的矩形坐标只有一份权威定义——`placeholder.json`。本
//!    生成器直接读它来决定往哪里画，不在代码里重复抄一份坐标（那样
//!    两份坐标迟早会漂移）；新增/调整条目时，改 JSON 后重新跑生成器
//!    就有对应的像素，不需要手动同步。
//!
//! # 只读图集布局，不改图集布局
//!
//! 本工具只从 JSON 读 `name`/`rect`，用来决定「在画布哪个矩形范围内
//! 画什么」；`pivot`/`footprint` 与本工具无关（那是渲染层的摆放逻辑，
//! 见 `ll_render::atlas`），本工具完全不读取也不产出这两个字段，因此
//! 不存在「生成器跑一遍就把 pivot/footprint 改掉」的风险。

mod color;
mod sprite;
mod terrain;

use image::RgbaImage;
use serde::Deserialize;
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

/// `placeholder.json` 里的一条条目：本工具只关心名字与矩形。`pivot`/
/// `footprint` 见模块文档「只读图集布局」一节，故意不反序列化它们
/// （`serde` 默认忽略 JSON 里多出的字段，不需要显式声明也能正确跳过）。
#[derive(Debug, Deserialize)]
struct AtlasEntryJson {
    name: String,
    rect: EntryRect,
}

/// `placeholder.json` 的顶层结构，本工具只用得到 `entries`。
#[derive(Debug, Deserialize)]
struct AtlasJson {
    entries: Vec<AtlasEntryJson>,
}

fn main() {
    let assets_dir = atlas_dir();
    let json_path = assets_dir.join("placeholder.json");
    let png_path = assets_dir.join("placeholder.png");

    let json_text = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", json_path.display()));
    let atlas: AtlasJson = serde_json::from_str(&json_text)
        .unwrap_or_else(|error| panic!("解析 {} 失败：{error}", json_path.display()));

    let (canvas_width, canvas_height) = canvas_size(&atlas.entries);
    // 画布尺寸完全由 JSON 里各条目矩形的右下角推出，不硬编码 96×72——
    // 但这里仍断言一次已知值，作为「JSON 被意外改动导致画布尺寸悄悄
    // 变化」的哨兵：这类改动会让全部验收 demo 的贴图位置错位，值得在
    // 生成阶段就喊出来，而不是留到渲染层才报错。
    assert_eq!(
        (canvas_width, canvas_height),
        (96, 72),
        "画布尺寸与已知布局不符，placeholder.json 的条目矩形可能被意外改动"
    );

    let mut image = RgbaImage::new(canvas_width, canvas_height);
    for entry in &atlas.entries {
        draw_entry(&mut image, &entry.name, entry.rect);
    }

    image
        .save(&png_path)
        .unwrap_or_else(|error| panic!("写入 {} 失败：{error}", png_path.display()));

    println!(
        "已重新生成 {}（{canvas_width}×{canvas_height}，共 {} 个条目）",
        png_path.display(),
        atlas.entries.len()
    );
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

/// 画布尺寸取全部条目矩形右下角坐标的最大值——与
/// `ll_render::atlas::validate_entries_within_image` 检查的是同一件事
/// 的反面：那边校验「矩形不超出图片」，这里保证「图片刚好能装下全部
/// 矩形」，两者共同保证图片与元数据互相吻合。
fn canvas_size(entries: &[AtlasEntryJson]) -> (u32, u32) {
    let mut width = 0;
    let mut height = 0;
    for entry in entries {
        width = width.max(entry.rect.x + entry.rect.width);
        height = height.max(entry.rect.y + entry.rect.height);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 画布尺寸取全部条目右下角的最大值() {
        // Arrange
        let entries = vec![
            AtlasEntryJson {
                name: "a".to_string(),
                rect: EntryRect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10,
                },
            },
            AtlasEntryJson {
                name: "b".to_string(),
                rect: EntryRect {
                    x: 20,
                    y: 5,
                    width: 4,
                    height: 4,
                },
            },
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
        // 炸掉，而不是留到跑生成器时才被 main() 里的 assert_eq! 抓住。
        // Arrange
        let json_text = std::fs::read_to_string(atlas_dir().join("placeholder.json"))
            .expect("仓库应自带 placeholder.json");
        let atlas: AtlasJson = serde_json::from_str(&json_text).expect("应是合法 JSON");

        // Act
        let size = canvas_size(&atlas.entries);

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
}
