//! 地形色块点缀：给纯色地形贴图加稀疏像素点缀，让相邻地块在测试截图
//! 里能看出边界与纹理，而不是一片同色糊在一起。
//!
//! 点缀分两层，都基于同一个「色相环上转角度」的工具（见 `color.rs`）：
//! - 邻近色（analogous）：色相 ±18°、明度 ±0.08，制造同一地形内的
//!   层次感（草地里深浅不同的绿、水面深浅不同的蓝……）。
//! - 互补色（complementary）：色相 +180°，只用极少数像素，作为强对比
//!   标记（沙地里偏冷的暗点、深水里的暖色波光……）。
//!
//! 两层点缀合计约占每个 16×16 地块的 5%（13/256 像素），地形本身的
//! 主色调像素占比恒定在 95% 左右——足够稀疏，不会盖过地形本身的可
//! 辨认颜色，又足够密，能在测试截图的分辨率下看出纹理。

use crate::EntryRect;
use crate::color::Hsl;
use image::{Rgba, RgbaImage};

/// 邻近色的色相偏移量（度）。18° 落在色彩理论「邻近色」惯用的 15°~30°
/// 区间内，偏小是因为地块只有 16×16 像素——偏移太大在这个分辨率下会
/// 读成两种不同颜色，而不是同一颜色的深浅层次。
const ANALOGOUS_HUE_SHIFT_DEG: f32 = 18.0;

/// 邻近色的明度偏移量。刻意选小值：明度差太大会在小块面积上读成
/// 「加了高光/阴影的立体感」，这里只想要「同一色系里深浅不同的几笔」。
const ANALOGOUS_LIGHTNESS_DELTA: f32 = 0.08;

/// 一种地形的点缀配方：主色不变，互补点缀色的明度/饱和度偏移量按
/// 地形单独调，目的是让互补点缀始终能在主色背景上「跳出来」。
pub(crate) struct TerrainSpec {
    /// 图集条目名，用于从 JSON 派发到这份配方。
    pub(crate) name: &'static str,
    /// 地形主色，与 `assets/atlas/README.md`「地形色块」一节记录的
    /// 既有颜色逐一对应；点缀不改变这个值，因此「地形主色调不变」这
    /// 条硬约束天然满足。
    pub(crate) base: (u8, u8, u8),
    /// 互补点缀色相对主色的明度偏移。
    ///
    /// 选择「让点缀跳出来」而非固定符号：主色本身偏暗（深水、森林）
    /// 就调亮，主色本身偏亮（沙地、雪地）就调暗——否则互补色虽然色相
    /// 对了，明度却跟主色接近，稀疏的几个像素在 16×16 的小块上几乎
    /// 看不见。
    pub(crate) accent_lightness_delta: f32,
    /// 互补点缀色相对主色的饱和度偏移。
    ///
    /// 绝大多数地形不需要：主色本身饱和度够高，互补色相旋转 180° 后
    /// 已经足够醒目。只有 `terrain_mountain`（灰）与 `terrain_snow`
    /// （近白）例外——这两种颜色本身饱和度接近 0，色相这个维度在换算
    /// 回 RGB 时几乎不产生视觉差异，必须额外把饱和度顶上去，互补色相
    /// 才能真正显色，而不是又点出一个「浅一点/深一点的灰」。
    pub(crate) accent_saturation_boost: f32,
}

/// 全部地形条目的点缀配方。颜色与 `assets/atlas/README.md`「地形色块」
/// 一节的既有取值逐一对应；`terrain_dirt` 是该表之外的既有条目，颜色
/// 同样取自现有像素，未改动。
const TERRAIN_SPECS: &[TerrainSpec] = &[
    TerrainSpec {
        name: "terrain_grass",
        base: (86, 125, 70),
        // 草地是暖绿，跳出来的互补点缀是亮玫红——像草丛里的野花，
        // 提亮而非压暗才不会被绿色主色吃掉。
        accent_lightness_delta: 0.30,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "terrain_dirt",
        base: (120, 100, 80),
        accent_lightness_delta: -0.20,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "terrain_deep_water",
        base: (24, 52, 128),
        // 深水很暗，互补点缀调亮成暖色波光，才能在暗蓝背景上跳出来。
        accent_lightness_delta: 0.35,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "terrain_shallow_water",
        base: (86, 172, 214),
        accent_lightness_delta: -0.20,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "terrain_sand",
        base: (214, 196, 140),
        // 用户原话「沙地里几粒偏冷的暗点」：沙是暖黄，互补色本就落在
        // 蓝紫这一侧（冷色），调暗让它读成阴影里的小石子。
        accent_lightness_delta: -0.35,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "terrain_forest",
        base: (32, 96, 40),
        accent_lightness_delta: 0.15,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "terrain_hill",
        base: (150, 138, 74),
        accent_lightness_delta: -0.20,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "terrain_mountain",
        base: (128, 128, 132),
        // 灰色饱和度接近 0，见 accent_saturation_boost 字段文档。
        accent_lightness_delta: 0.30,
        accent_saturation_boost: 0.5,
    },
    TerrainSpec {
        name: "terrain_snow",
        base: (238, 240, 244),
        // 近白色同样需要拉饱和度，否则互补色换算回来还是接近白色。
        accent_lightness_delta: -0.35,
        accent_saturation_boost: 0.4,
    },
    // 气候条带（规格 §7.1）新增的两种自然地形。它们**不进** 遗留共享
    // 画布（`assets/atlas/placeholder.json` 一个字未动），只走
    // `main.rs` 的 `LooseOnlyEntry` 通道——理由见那个类型的文档：新增
    // 内容塞进共享画布只会把五个更早批次验收 demo 的冻结像素基准卷
    // 进来。
    TerrainSpec {
        name: "terrain_desert",
        // 沙漠：比海岸 `terrain_sand`(214,196,140) 更深更橙。两者是两种
        // 地形（海滩 vs 沙漠），颜色必须一眼分得开，否则屏幕上又变回
        // 「借用」那种看不出区别的状态。
        base: (198, 154, 86),
        // 暖黄的互补色落在蓝紫这一侧，调暗读成沙丘背阴处的碎石。
        accent_lightness_delta: -0.30,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "terrain_tundra",
        // 冻原：比高山 `terrain_snow`(238,240,244) 更暗更青——低地冻土
        // 不是峰顶的新雪。
        base: (196, 206, 208),
        // 与 `terrain_snow` 同一条理由：近灰白色饱和度接近 0，不把饱和度
        // 顶上去，互补色相换算回 RGB 后仍是「另一种灰」。
        accent_lightness_delta: -0.30,
        accent_saturation_boost: 0.4,
    },
    // 以下两份配方不属于本体图集（不会出现在 `assets/atlas/placeholder.json`
    // 里），只供 `mods/example_mod` 的真实资产 VFS 验收 demo 使用——见
    // `main.rs` 里 `generate_mod_demo_assets` 一节。放进同一张配方表，
    // 是因为点缀算法（`decorate_terrain_tile`）与地形种类无关，没有
    // 理由为了区区两种地形另起一套绘制逻辑。
    TerrainSpec {
        name: "lava_floor",
        // 熔岩：饱和度极高的橙红，与本体全部地形色都拉开明显差距——
        // `examplemod:lava_floor` 是 mod 自己新增的地形种类,不是本体
        // 任何既有地形的变体,颜色不该跟任何一个既有配方相近。
        base: (226, 88, 24),
        accent_lightness_delta: 0.30,
        accent_saturation_boost: 0.0,
    },
    TerrainSpec {
        name: "examplemod_terrain_dirt_override",
        // 覆盖 demo：与本体 `terrain_dirt`（暖褐色 `(120, 100, 80)`）
        // 主色相近但明显更红——玩家应该能一眼看出「这块地被换了皮肤」，
        // 而不是「颜色几乎一样看不出差异」。
        base: (150, 70, 60),
        accent_lightness_delta: -0.20,
        accent_saturation_boost: 0.0,
    },
];

/// 按条目名查配方；查不到返回 `None`，由调用方决定如何处理——见
/// `main.rs` 里「未知条目直接报错」的说明。
pub(crate) fn terrain_spec(name: &str) -> Option<&'static TerrainSpec> {
    TERRAIN_SPECS.iter().find(|spec| spec.name == name)
}

/// 一次简单的整数哈希，把「地块种子 + 像素本地坐标」拌成一个看起来
/// 均匀分布的 32 位值，用来决定某个像素是否落入点缀。
///
/// 地块种子取自它在图集里的像素坐标（见 [`decorate_terrain_tile`] 的
/// `tile_seed` 计算），这就是「用格子坐标做种子」——同一张图集只要不改
/// JSON 布局，重新跑生成器会算出逐像素相同的点缀图案，不是每次运行
/// 都不一样的随机噪点。算法是 SplitMix 风格的有限位混合，只要求
/// 「确定性、分布看起来均匀」，不需要密码学强度。
///
/// `pub(crate)`——`ui.rs` 的 `decorate_day_night_bar`（昼夜滑条渐变贴图）
/// 复用同一份哈希做点缀取样，不是另起一套随机源：两处都要求「同一次
/// 生成内确定、看起来均匀分布」，没有理由维护两份算法。
pub(crate) fn hash_pixel(tile_seed: u32, local_x: u32, local_y: u32) -> u32 {
    let mut h = tile_seed ^ local_x.wrapping_mul(0x9E37_79B1) ^ local_y.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}

/// 给一块地形贴图填色并点缀，`rect` 是它在图集画布上的像素矩形。
///
/// 点缀密度固定：256 个像素里，约 5 个落入邻近色 A、约 5 个落入邻近色
/// B、约 3 个落入互补点缀色，其余约 243 个（95%）保持主色——这是
/// 「稀疏点缀、不改变主色调」这条硬约束的具体数字化。
pub(crate) fn decorate_terrain_tile(image: &mut RgbaImage, rect: EntryRect, spec: &TerrainSpec) {
    let base_hsl = Hsl::from_rgb(spec.base.0, spec.base.1, spec.base.2);
    let analogous_a = base_hsl
        .rotated(ANALOGOUS_HUE_SHIFT_DEG)
        .lighten(ANALOGOUS_LIGHTNESS_DELTA)
        .to_rgb();
    let analogous_b = base_hsl
        .rotated(-ANALOGOUS_HUE_SHIFT_DEG)
        .lighten(-ANALOGOUS_LIGHTNESS_DELTA)
        .to_rgb();
    let accent = base_hsl
        .rotated(180.0)
        .lighten(spec.accent_lightness_delta)
        .saturate(spec.accent_saturation_boost)
        .to_rgb();

    // 地块种子取自它在图集里的像素坐标：见本函数与 hash_pixel 的文档。
    let tile_seed = (rect.x << 16) | rect.y;

    for local_y in 0..rect.height {
        for local_x in 0..rect.width {
            let bucket = hash_pixel(tile_seed, local_x, local_y) % 256;
            let (r, g, b) = match bucket {
                0..=4 => analogous_a,
                5..=9 => analogous_b,
                10..=12 => accent,
                _ => spec.base,
            };
            image.put_pixel(rect.x + local_x, rect.y + local_y, Rgba([r, g, b, 255]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 八个已知地形名都能查到配方() {
        // Arrange
        let names = [
            "terrain_grass",
            "terrain_dirt",
            "terrain_deep_water",
            "terrain_shallow_water",
            "terrain_sand",
            "terrain_forest",
            "terrain_hill",
            "terrain_mountain",
            "terrain_snow",
        ];

        // Act & Assert
        for name in names {
            assert!(terrain_spec(name).is_some(), "缺少配方：{name}");
        }
    }

    #[test]
    fn 未知地形名查不到配方() {
        // Arrange & Act & Assert
        assert!(terrain_spec("terrain_lava").is_none());
    }

    #[test]
    fn 点缀后主色像素占比不低于九成() {
        // Arrange：随便取一种地形验证密度比例，密度算法与地形无关。
        let spec = terrain_spec("terrain_grass").expect("terrain_grass 有配方");
        let rect = EntryRect {
            x: 48,
            y: 0,
            width: 16,
            height: 16,
        };
        let mut image = RgbaImage::new(64, 16);

        // Act
        decorate_terrain_tile(&mut image, rect, spec);
        let base = Rgba([spec.base.0, spec.base.1, spec.base.2, 255]);
        let base_count = image.pixels().filter(|&&p| p == base).count();

        // Assert：256 像素里最多约 13 个点缀像素，主色占比应远高于 90%。
        assert!(base_count as f32 / 256.0 > 0.9);
    }
}
