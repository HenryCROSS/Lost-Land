//! 极简像素点阵字体：把「NPC 要显示名字」「伤害数字可见」「时间轴侧栏
//! 显示出手顺序」这三条验收点落到实处。
//!
//! # 为什么现造一套字体，而不是找字体渲染库
//!
//! `ll-render` 目前只有图集精灵批渲染，没有任何文字排版能力——这本身
//! 合理（P3 之前没有任何验收点要求过屏幕文字）。引入一个真正的字体
//! 渲染 crate（`ab_glyph`/`fontdue` 之类）要新增一条外部依赖、一整套
//! 字形栅格化与排版管线，对「验收 demo 需要能看见几个名字与数字」这个
//! 需求而言是不成比例的重型方案，也不是本任务该做的架构决策。
//!
//! 这里改用最朴素的办法：手绘一套 4×6 像素的点阵字形（只覆盖 demo
//! 实际会用到的字符——见 [`CHARSET`]），在运行时把它们栅格化进一张
//! 内存中的 [`image::RgbaImage`]，拼接在图集图片下方，与图集原有的
//! 精灵条目共享同一张纹理、同一个 [`ll_render::batch::SpriteBatch`]。
//! 好处是不需要引入新依赖、不需要给 `ll-render` 添加新的公开 API，
//! 且天然满足「footprint 从图集条目读取」——字形本身也是走
//! [`ll_render::atlas::AtlasEntry`] 这条既有路径查出来的。
//!
//! # 为什么只覆盖这一小撮字符
//!
//! NPC 姓名来自 [`crate::spawn::demo_naming_rules`] 指定的音素表，
//! 音素表本身只用大写拉丁字母拼接（见该函数文档）；伤害数字只需要
//! 十个数字。把字符集收窄到「demo 实际会拼出来的字符」，换来的是
//! 27 个字形可以逐一手绘校对，而不是伪造一套自称覆盖全部 ASCII、
//! 实际大部分字形从未被人看过一眼的表格。

use image::{Rgba, RgbaImage};
use ll_render::atlas::{AtlasEntry, AtlasMetadata, FrameRect};
use ll_render::sprite::{Footprint, Pivot};

/// 单个字形的宽度（像素）。
pub(crate) const GLYPH_COLS: u32 = 4;
/// 单个字形的高度（像素）。
pub(crate) const GLYPH_ROWS: u32 = 6;

/// demo 会用到的全部字符：十个数字、[`crate::spawn::demo_naming_rules`]
/// 音素表覆盖的十七个大写字母、以及词间分隔用的空格。
///
/// 顺序即后续栅格化时在字体行里从左到右摆放的顺序，任意顺序都合法，
/// 这里按数字在前、字母其次、空格收尾，只是方便人工核对时定位。
pub(crate) const CHARSET: &str = "0123456789ABDEGIKLMNORSTUVZ ";

/// 查一个字符的 4×6 点阵位图，`true` 表示该像素应绘制。
///
/// 未覆盖的字符返回全空白（而不是 panic 或裁掉）——demo 只应该传入
/// [`CHARSET`] 里的字符，但调用方若传入别的字符，静默留白好过让整个
/// 渲染帧因为一个字符崩溃；这与图集查不到条目时静默跳过（见
/// `p2_acceptance::GpuResources::lookup`）是同一个纪律。
pub(crate) fn glyph_pixels(ch: char) -> [[bool; GLYPH_COLS as usize]; GLYPH_ROWS as usize] {
    let rows: [&str; 6] = match ch {
        '0' => [".##.", "#..#", "#..#", "#..#", "#..#", ".##."],
        '1' => [".#..", "##..", ".#..", ".#..", ".#..", "###."],
        '2' => ["###.", "...#", ".##.", "#...", "#...", "####"],
        '3' => ["###.", "...#", ".##.", "...#", "...#", "###."],
        '4' => ["#..#", "#..#", "####", "...#", "...#", "...#"],
        '5' => ["####", "#...", "###.", "...#", "...#", "###."],
        '6' => [".##.", "#...", "###.", "#..#", "#..#", ".##."],
        '7' => ["####", "...#", "..#.", ".#..", ".#..", ".#.."],
        '8' => [".##.", "#..#", ".##.", "#..#", "#..#", ".##."],
        '9' => [".##.", "#..#", ".##.", "...#", "...#", ".##."],
        'A' => [".##.", "#..#", "#..#", "####", "#..#", "#..#"],
        'B' => ["###.", "#..#", "###.", "#..#", "#..#", "###."],
        'D' => ["###.", "#..#", "#..#", "#..#", "#..#", "###."],
        'E' => ["####", "#...", "###.", "#...", "#...", "####"],
        'G' => [".##.", "#...", "#.##", "#..#", "#..#", ".##."],
        'I' => ["###.", ".#..", ".#..", ".#..", ".#..", "###."],
        'K' => ["#..#", "#.#.", "##..", "#.#.", "#..#", "#..#"],
        'L' => ["#...", "#...", "#...", "#...", "#...", "####"],
        'M' => ["#..#", "####", "#..#", "#..#", "#..#", "#..#"],
        'N' => ["#..#", "##.#", "#.##", "#..#", "#..#", "#..#"],
        'O' => [".##.", "#..#", "#..#", "#..#", "#..#", ".##."],
        'R' => ["###.", "#..#", "###.", "#.#.", "#..#", "#..#"],
        'S' => [".###", "#...", ".##.", "...#", "...#", "###."],
        'T' => ["####", ".#..", ".#..", ".#..", ".#..", ".#.."],
        'U' => ["#..#", "#..#", "#..#", "#..#", "#..#", ".##."],
        'V' => ["#..#", "#..#", "#..#", "#..#", ".##.", ".##."],
        'Z' => ["####", "...#", "..#.", ".#..", "#...", "####"],
        _ => ["....", "....", "....", "....", "....", "...."],
    };

    let mut pixels = [[false; GLYPH_COLS as usize]; GLYPH_ROWS as usize];
    for (row_index, row) in rows.iter().enumerate() {
        for (col_index, symbol) in row.chars().enumerate() {
            pixels[row_index][col_index] = symbol == '#';
        }
    }
    pixels
}

/// 某个字符对应的图集条目名。空格没有可见像素，仍然给它一个条目名——
/// 排版侧一律按同一套「查条目、按 footprint/pivot 摆放」流程处理，不必
/// 为空格单独分支。
pub(crate) fn glyph_entry_name(ch: char) -> String {
    if ch == ' ' {
        "font_space".to_string()
    } else {
        format!("font_{ch}")
    }
}

/// 把 [`CHARSET`] 里的每个字形栅格化进 `base_image` 下方新增的一整行，
/// 返回扩展后的图片与「原有条目 + 新增字形条目」的合并列表。
///
/// 不依赖 GPU，可以脱离真实图形适配器单测覆盖——与图集元数据校验
/// （`ll_render::atlas::AtlasMetadata::parse`）、UV 换算
/// （`ll_render::atlas::Atlas::uv_rect`）不需要真实纹理就能测试是同一个
/// 理由。真正上传到 GPU 是调用方（`main.rs`）在拿到这里的返回值后，
/// 编码成 PNG 字节再交给 [`ll_render::atlas::Atlas::load`] 的事。
pub(crate) fn extend_atlas_with_font(
    base_metadata: &AtlasMetadata,
    base_image: &RgbaImage,
) -> (RgbaImage, Vec<AtlasEntry>) {
    let glyph_count = CHARSET.chars().count() as u32;
    let font_row_width = glyph_count * GLYPH_COLS;
    let canvas_width = base_image.width().max(font_row_width);
    let font_row_y = base_image.height();
    let canvas_height = font_row_y + GLYPH_ROWS;

    let mut canvas = RgbaImage::from_pixel(canvas_width, canvas_height, Rgba([0, 0, 0, 0]));
    for (x, y, pixel) in base_image.enumerate_pixels() {
        canvas.put_pixel(x, y, *pixel);
    }

    let mut entries = base_metadata.entries.clone();
    for (glyph_index, ch) in CHARSET.chars().enumerate() {
        let origin_x = glyph_index as u32 * GLYPH_COLS;
        let bits = glyph_pixels(ch);
        for row in 0..GLYPH_ROWS {
            for col in 0..GLYPH_COLS {
                if bits[row as usize][col as usize] {
                    canvas.put_pixel(origin_x + col, font_row_y + row, Rgba([255, 255, 255, 255]));
                }
            }
        }
        entries.push(AtlasEntry {
            name: glyph_entry_name(ch),
            rect: FrameRect {
                x: origin_x as u16,
                y: font_row_y as u16,
                width: GLYPH_COLS as u16,
                height: GLYPH_ROWS as u16,
            },
            pivot: Pivot { x: 0, y: 0 },
            // 字形不参与「占地格数」意义上的世界摆放（UI 层文字不走
            // DrawOrder 的脚底排序），恒取 1×1 只是满足
            // AtlasEntry::footprint 非零这条通用不变式。
            footprint: Footprint {
                width: 1,
                height: 1,
            },
        });
    }

    (canvas, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metadata() -> AtlasMetadata {
        AtlasMetadata {
            image: "placeholder.png".to_string(),
            entries: vec![AtlasEntry {
                name: "hero_idle_0".to_string(),
                rect: FrameRect {
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
        }
    }

    #[test]
    fn 每个字符集里的字符都能查到非空字形() {
        // 空格允许全空白，其余字符至少应点亮一个像素——否则字符集里
        // 混进了一个没有实际形状、看起来会是空白方块的字符。
        // Arrange & Act & Assert
        for ch in CHARSET.chars() {
            if ch == ' ' {
                continue;
            }
            let bits = glyph_pixels(ch);
            let lit = bits.iter().flatten().filter(|&&on| on).count();
            assert!(lit > 0, "字符 '{ch}' 的字形不应全空白");
        }
    }

    #[test]
    fn 未覆盖的字符返回全空白而非崩溃() {
        // Arrange & Act
        let bits = glyph_pixels('#');

        // Assert
        assert!(bits.iter().flatten().all(|&on| !on));
    }

    #[test]
    fn 扩展后的图集保留原有条目() {
        // Arrange
        let metadata = sample_metadata();
        let base_image = RgbaImage::from_pixel(96, 64, Rgba([10, 20, 30, 255]));

        // Act
        let (_, entries) = extend_atlas_with_font(&metadata, &base_image);

        // Assert
        assert!(entries.iter().any(|entry| entry.name == "hero_idle_0"));
    }

    #[test]
    fn 扩展后的图集包含字符集里每个字符的条目() {
        // Arrange
        let metadata = sample_metadata();
        let base_image = RgbaImage::from_pixel(96, 64, Rgba([10, 20, 30, 255]));

        // Act
        let (_, entries) = extend_atlas_with_font(&metadata, &base_image);

        // Assert
        for ch in CHARSET.chars() {
            let name = glyph_entry_name(ch);
            assert!(
                entries.iter().any(|entry| entry.name == name),
                "缺少字符 '{ch}' 对应的条目 '{name}'"
            );
        }
    }

    #[test]
    fn 扩展后的图片高度包含原图与字体行两部分() {
        // Arrange
        let metadata = sample_metadata();
        let base_image = RgbaImage::from_pixel(96, 64, Rgba([10, 20, 30, 255]));

        // Act
        let (canvas, _) = extend_atlas_with_font(&metadata, &base_image);

        // Assert
        assert_eq!(canvas.height(), base_image.height() + GLYPH_ROWS);
    }

    #[test]
    fn 字体条目的占地格数为一以满足图集校验不变式() {
        // AtlasMetadata::parse 会拒绝占地格数为零的条目（见其文档）；
        // 本模块绕开了 parse 直接构造 AtlasEntry，这条测试独立锁住
        // 同一条不变式，避免将来有人改动时悄悄破坏它。
        // Arrange
        let metadata = sample_metadata();
        let base_image = RgbaImage::from_pixel(96, 64, Rgba([10, 20, 30, 255]));

        // Act
        let (_, entries) = extend_atlas_with_font(&metadata, &base_image);

        // Assert
        for entry in &entries {
            assert!(entry.footprint.tile_count() > 0);
        }
    }
}
