//! HUD 控件的临时美术：面板九宫格边框/填充、条形底/填充。
//!
//! # 复用地形点缀算法，不另起一套风格
//!
//! 项目所有者明确要求「风格要和现有美术一套……不要另起一套风格」——
//! 本模块因此不新写一套绘制逻辑，而是直接复用 [`crate::terrain::TerrainSpec`]/
//! [`crate::terrain::decorate_terrain_tile`]：同一份「主色、邻近色点缀、
//! 互补色点缀」配方，只是换一套目标名字与颜色，构造出的贴图与地形
//! 瓦片是同一种视觉语言（稀疏色块点缀，约 5% 像素偏离主色）。
//!
//! # 只用两张贴图撑起完整的九宫格
//!
//! `ui_panel_border` 同时充当四个角与四条边——四个角原样绘制（不
//! 拉伸），四条边由渲染层（`ll_ui::widget::panel` 未来的贴图版本）沿
//! 一个方向拉伸,中心用 `ui_panel_fill` 双向拉伸。真实的九宫格贴图
//! （角有花纹、边不能简单复用）到位前，边框本身就是纯色 + 点缀，四个
//! 角与两条边共用同一张源图不会有任何视觉不一致——这正是
//! `crates/ll-ui/src/widget/panel.rs` 模块文档「为什么不是四条边+一个
//! 填充」一节讨论的「贴图到位后可能需要拆开」的具体验证：本批次的
//! 占位贴图恰好不需要拆。
//!
//! 条形同理：`ui_bar_track` 是未填充部分的底色，`ui_bar_fill` 是
//! 已填充部分,两者都不含内部九宫格切分,整张贴图按条形当前尺寸直接
//! 拉伸即可（条形没有「角」的概念）。

use crate::EntryRect;
use crate::color::Hsl;
use crate::terrain::{TerrainSpec, hash_pixel};
use image::{Rgba, RgbaImage};

/// 全部 UI 贴图的配方——复用 [`TerrainSpec`] 的形状（见模块文档
/// 「复用地形点缀算法」一节），颜色与
/// `crates/ll-ui/src/widget/panel.rs`/`bar.rs` 里
/// `FlatPanelAppearance::DEFAULT`/`FlatBarAppearance::DEFAULT` 的既有
/// 纯色选择保持同一套视觉方向（浅灰蓝边框、深蓝黑填充、深灰底、亮蓝
/// 青填充），换了真实贴图后玩家看到的色调不会突变。
const UI_SPECS: &[TerrainSpec] = &[
    TerrainSpec {
        name: "ui_panel_border",
        // 浅灰蓝——呼应 `FlatPanelAppearance::DEFAULT.border_color`
        // 约 `(191, 191, 204)`。
        base: (190, 195, 208),
        accent_lightness_delta: -0.30,
        accent_saturation_boost: 0.35,
    },
    TerrainSpec {
        name: "ui_panel_fill",
        // 深蓝黑——呼应 `FlatPanelAppearance::DEFAULT.fill_color`
        // 约 `(13, 13, 20)`。
        base: (16, 18, 26),
        accent_lightness_delta: 0.35,
        accent_saturation_boost: 0.3,
    },
    TerrainSpec {
        name: "ui_bar_track",
        // 深灰——呼应 `FlatBarAppearance::DEFAULT.background_color`
        // 约 `(51, 51, 56)`。
        base: (48, 50, 56),
        accent_lightness_delta: 0.25,
        accent_saturation_boost: 0.2,
    },
    TerrainSpec {
        name: "ui_bar_fill",
        // 亮蓝青——呼应 `FlatBarAppearance::DEFAULT.fill_color`
        // 约 `(102, 191, 242)`。
        base: (96, 190, 240),
        accent_lightness_delta: -0.25,
        accent_saturation_boost: 0.0,
    },
];

/// 按条目名查 UI 贴图配方；查不到返回 `None`，与
/// [`crate::terrain::terrain_spec`] 同一条约定。
pub(crate) fn ui_spec(name: &str) -> Option<&'static TerrainSpec> {
    UI_SPECS.iter().find(|spec| spec.name == name)
}

/// 昼夜滑条底图——夜（左）到昼（右）的水平渐变，点缀风格与
/// [`crate::terrain::decorate_terrain_tile`] 一致，但这张贴图**不是**
/// [`TerrainSpec`] 能表达的东西：`TerrainSpec` 只有一个主色 + 点缀,
/// 整张贴图是同一种颜色；昼夜滑条恰恰需要「颜色本身随水平位置变化」,
/// 因此不复用 `ui_spec`/`decorate_terrain_tile` 的调度路径,单独在
/// `main.rs::draw_entry` 里按名字直接分派到本函数。
///
/// # 配色：深靛蓝（夜）→ 暖金（昼）
///
/// 与项目所有者原话「左边是黑夜图案右边是白天图案」对应——夜端选深
/// 靛蓝（比纯黑更有层次，且与既有 UI 贴图的深色基调一致，见
/// `UI_SPECS` 里 `ui_panel_fill` 的选色说明），昼端选暖金（与
/// `terrain_sand`/`terrain_hill` 同一档暖色調,不是刺眼的纯白，保持
/// 与既有地形色板的整体风格一致）。
const DAYNIGHT_NIGHT_COLOR: (u8, u8, u8) = (18, 22, 54);
const DAYNIGHT_DAY_COLOR: (u8, u8, u8) = (250, 202, 96);

/// 点缀用的邻近色色相偏移/明度偏移，与
/// `crate::terrain::decorate_terrain_tile` 的既有取值一致——同一套视觉
/// 语言（稀疏点缀、约 5% 像素偏离本地基色），理由见模块文档。
const DAYNIGHT_ANALOGOUS_HUE_SHIFT_DEG: f32 = 18.0;
const DAYNIGHT_ANALOGOUS_LIGHTNESS_DELTA: f32 = 0.08;

/// 两个 8 位 RGB 颜色之间的线性插值，`t` 钳制到 `[0.0, 1.0]`。
fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let lerp_channel = |x: u8, y: u8| -> u8 {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (
        lerp_channel(a.0, b.0),
        lerp_channel(a.1, b.1),
        lerp_channel(a.2, b.2),
    )
}

/// 给昼夜滑条底图填色并点缀，`rect` 是它在画布上的像素矩形（松散贴图
/// 路径下画布就是这张图本身，`rect.x`/`rect.y` 恒为 0，见
/// `main.rs::generate_loose_sprites`）。
///
/// 每一列（同一个 `local_x`）先按水平位置在夜色/昼色之间线性插值算出
/// 这一列的「本地基色」，再用与 [`crate::terrain::decorate_terrain_tile`]
/// 相同的点缀算法（邻近色 + 互补色，[`hash_pixel`] 决定像素落入哪一
/// 桶）在本地基色上点缀——因此整张图既有从夜到昼的渐变，又保留与其余
/// UI/地形贴图一致的点缀质感，不是一张纯粹平滑、风格突兀的渐变图。
pub(crate) fn decorate_day_night_bar(image: &mut RgbaImage, rect: EntryRect) {
    let tile_seed = (rect.x << 16) | rect.y;
    // 宽度至少为 2 才有「从左到右」的渐变可言；本函数只服务已知的
    // `ui_daynight_bar` 条目（`assets/atlas/placeholder.json` 里固定
    // 32 像素宽），`max(2, ..)` 只是防御性下限，不代表这个尺寸是预期
    // 输入之外的情况。
    let denom = (rect.width.max(2) - 1) as f32;

    for local_x in 0..rect.width {
        let t = local_x as f32 / denom;
        let base = lerp_rgb(DAYNIGHT_NIGHT_COLOR, DAYNIGHT_DAY_COLOR, t);
        let base_hsl = Hsl::from_rgb(base.0, base.1, base.2);
        let analogous_a = base_hsl
            .rotated(DAYNIGHT_ANALOGOUS_HUE_SHIFT_DEG)
            .lighten(DAYNIGHT_ANALOGOUS_LIGHTNESS_DELTA)
            .to_rgb();
        let analogous_b = base_hsl
            .rotated(-DAYNIGHT_ANALOGOUS_HUE_SHIFT_DEG)
            .lighten(-DAYNIGHT_ANALOGOUS_LIGHTNESS_DELTA)
            .to_rgb();
        let accent = base_hsl.rotated(180.0).to_rgb();

        for local_y in 0..rect.height {
            let bucket = hash_pixel(tile_seed, local_x, local_y) % 256;
            let (r, g, b) = match bucket {
                0..=4 => analogous_a,
                5..=9 => analogous_b,
                10..=12 => accent,
                _ => base,
            };
            image.put_pixel(rect.x + local_x, rect.y + local_y, Rgba([r, g, b, 255]));
        }
    }
}

/// 昼夜滑条**滑块**的宽高（像素）。
///
/// 与底图（`ui_daynight_bar`）不同，滑块不做拉伸——它在屏幕上恒是
/// `ll_ui::widget::day_night_bar::POINTER_WIDTH` 宽、整条高，画布按同一
/// 个比例（宽:高 = 1:2）出图，拉伸后不会把描边拉成粗细不均的一圈。
pub(crate) const DAYNIGHT_POINTER_WIDTH: u32 = 8;
/// 滑块画布高度，理由同 [`DAYNIGHT_POINTER_WIDTH`]。
pub(crate) const DAYNIGHT_POINTER_HEIGHT: u32 = 16;

/// 滑块的描边色——近黑的冷暗色。
///
/// 描边是这张图存在的**主要理由**：滑块要压在一条从深靛蓝（夜）一路
/// 渐变到暖金（昼）的底图上，任何单一颜色都会在渐变的某一段糊掉。一圈
/// 深色描边把滑块与底图彻底切开，无论它停在哪个时刻都读得出轮廓——与
/// `crate::world_marks` 那几张记号「先描边再填色」是同一条既有做法。
const POINTER_OUTLINE: (u8, u8, u8) = (18, 16, 22);
/// 滑块主体色——暖白，比底图昼端的暖金更亮更淡，因此在正午那一段也
/// 不会与底色同化。
const POINTER_BODY: (u8, u8, u8) = (248, 244, 232);
/// 滑块中央那道竖槽的颜色——比主体暗一档，让滑块读起来像一个「有厚度
/// 的把手」而不是一根白条。
const POINTER_GROOVE: (u8, u8, u8) = (152, 146, 132);

/// 画昼夜滑条的滑块：一圈描边 + 暖白主体 + 中央一道竖槽。
///
/// 尺寸恒按 `rect` 现算而不是写死像素坐标——`rect.width`/`rect.height`
/// 来自 [`DAYNIGHT_POINTER_WIDTH`]/[`DAYNIGHT_POINTER_HEIGHT`]，但本函数
/// 不假设那两个值具体是多少（改尺寸不必改画法）。
///
/// **整张图不透明**：滑块要压住底图，透明像素会让底图的渐变从滑块里透
/// 出来，正是「看不清滑块停在哪」的老问题。这与 `crate::world_marks`
/// 那几张「刻意留空底色」的世界记号不同——那些画在地形之上要露出地形，
/// 这一块的职责恰恰相反。
pub(crate) fn decorate_day_night_pointer(image: &mut RgbaImage, rect: EntryRect) {
    for local_y in 0..rect.height {
        for local_x in 0..rect.width {
            let on_border = local_x == 0
                || local_y == 0
                || local_x + 1 == rect.width
                || local_y + 1 == rect.height;
            // 中央竖槽：宽度取 1/4 画布宽（至少 1 像素），上下各留出
            // 两格描边与主体，槽因此不碰到边。
            let groove_half = (rect.width / 8).max(1);
            let center = rect.width / 2;
            let in_groove = local_x + groove_half >= center
                && local_x < center + groove_half
                && local_y >= 3
                && local_y + 3 < rect.height;
            let (r, g, b) = if on_border {
                POINTER_OUTLINE
            } else if in_groove {
                POINTER_GROOVE
            } else {
                POINTER_BODY
            };
            image.put_pixel(rect.x + local_x, rect.y + local_y, Rgba([r, g, b, 255]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 四个ui贴图名都能查到配方() {
        // Arrange
        let names = [
            "ui_panel_border",
            "ui_panel_fill",
            "ui_bar_track",
            "ui_bar_fill",
        ];

        // Act & Assert
        for name in names {
            assert!(ui_spec(name).is_some(), "缺少配方：{name}");
        }
    }

    #[test]
    fn 未知ui贴图名查不到配方() {
        // Arrange & Act & Assert
        assert!(ui_spec("ui_nonexistent").is_none());
    }

    #[test]
    fn lerp_rgb在起点返回第一个颜色() {
        // Arrange & Act
        let color = lerp_rgb((10, 20, 30), (200, 210, 220), 0.0);

        // Assert
        assert_eq!(color, (10, 20, 30));
    }

    #[test]
    fn lerp_rgb在终点返回第二个颜色() {
        // Arrange & Act
        let color = lerp_rgb((10, 20, 30), (200, 210, 220), 1.0);

        // Assert
        assert_eq!(color, (200, 210, 220));
    }

    #[test]
    fn decorate_day_night_bar最左列的主色接近夜色而不是昼色() {
        // 「主色」指点缀之外占多数的像素颜色——单独取最左列一整列的
        // 众数颜色，避开点缀像素造成的偶然误判。
        // Arrange
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: 32,
            height: 16,
        };
        let mut image = RgbaImage::new(rect.width, rect.height);

        // Act
        decorate_day_night_bar(&mut image, rect);
        let leftmost_pixel = image.get_pixel(0, 0);

        // Assert：最左列（`local_x == 0`）插值比例恒为 0，本地基色恒
        // 等于夜色本身；取 `(0, 0)` 这一个像素不受点缀干扰的概率并不
        // 保证为真，但夜色与昼色在红色通道上差距极大（18 对 250），
        // 即使这一像素落进点缀桶，点缀只是邻近色/互补色的小幅旋转，
        // 红色通道不会从 250 附近跳到接近 18。
        assert!(leftmost_pixel.0[0] < 128);
    }

    #[test]
    fn decorate_day_night_bar最右列的主色接近昼色而不是夜色() {
        // 理由同上一条测试，方向相反。
        // Arrange
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: 32,
            height: 16,
        };
        let mut image = RgbaImage::new(rect.width, rect.height);

        // Act
        decorate_day_night_bar(&mut image, rect);
        let rightmost_pixel = image.get_pixel(rect.width - 1, 0);

        // Assert
        assert!(rightmost_pixel.0[0] > 128);
    }

    #[test]
    fn decorate_day_night_bar连续两次生成产出逐位相同的像素() {
        // 确定性——见 `main.rs` 模块文档「确定性」相关测试的同一条
        // 纪律，本测试专门覆盖这一张新贴图。
        // Arrange
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: 32,
            height: 16,
        };
        let mut first = RgbaImage::new(rect.width, rect.height);
        let mut second = RgbaImage::new(rect.width, rect.height);

        // Act
        decorate_day_night_bar(&mut first, rect);
        decorate_day_night_bar(&mut second, rect);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn decorate_day_night_pointer整张图不透明() {
        // 滑块要压住底图的渐变，任何透明像素都会让底色透出来——这正是
        // 「看不清滑块停在哪」的老问题，见本函数文档。
        // Arrange
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: DAYNIGHT_POINTER_WIDTH,
            height: DAYNIGHT_POINTER_HEIGHT,
        };
        let mut image = RgbaImage::new(rect.width, rect.height);

        // Act
        decorate_day_night_pointer(&mut image, rect);

        // Assert
        for pixel in image.pixels() {
            assert_eq!(pixel.0[3], 255, "滑块贴图不应有半透明或透明像素");
        }
    }

    #[test]
    fn decorate_day_night_pointer四条边都是描边色() {
        // 描边是这张图能在深靛蓝到暖金整段渐变上都看得清的唯一依靠。
        // Arrange
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: DAYNIGHT_POINTER_WIDTH,
            height: DAYNIGHT_POINTER_HEIGHT,
        };
        let mut image = RgbaImage::new(rect.width, rect.height);

        // Act
        decorate_day_night_pointer(&mut image, rect);

        // Assert
        let outline = Rgba([POINTER_OUTLINE.0, POINTER_OUTLINE.1, POINTER_OUTLINE.2, 255]);
        for x in 0..rect.width {
            assert_eq!(*image.get_pixel(x, 0), outline);
            assert_eq!(*image.get_pixel(x, rect.height - 1), outline);
        }
        for y in 0..rect.height {
            assert_eq!(*image.get_pixel(0, y), outline);
            assert_eq!(*image.get_pixel(rect.width - 1, y), outline);
        }
    }

    #[test]
    fn 滑块主体与底图两端色都拉得开() {
        // 「在夜端和昼端都一眼可见」这条要求的程序化核实：主体色与底图
        // 的夜色、昼色都要有足够的通道差，而不是只跟其中一端不同。
        // Arrange
        let min_channel_distance = 60i32;

        // Act
        let to_night: i32 = [
            POINTER_BODY.0 as i32 - DAYNIGHT_NIGHT_COLOR.0 as i32,
            POINTER_BODY.1 as i32 - DAYNIGHT_NIGHT_COLOR.1 as i32,
            POINTER_BODY.2 as i32 - DAYNIGHT_NIGHT_COLOR.2 as i32,
        ]
        .iter()
        .map(|d| d.abs())
        .max()
        .expect("三个通道恒非空");
        let to_day: i32 = [
            POINTER_BODY.0 as i32 - DAYNIGHT_DAY_COLOR.0 as i32,
            POINTER_BODY.1 as i32 - DAYNIGHT_DAY_COLOR.1 as i32,
            POINTER_BODY.2 as i32 - DAYNIGHT_DAY_COLOR.2 as i32,
        ]
        .iter()
        .map(|d| d.abs())
        .max()
        .expect("三个通道恒非空");

        // Assert
        assert!(
            to_night >= min_channel_distance,
            "滑块在夜端看不清：{to_night}"
        );
        assert!(to_day >= min_channel_distance, "滑块在昼端看不清：{to_day}");
    }
}
