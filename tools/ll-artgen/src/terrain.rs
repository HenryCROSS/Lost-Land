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
    // 内容塞进共享画布只会把那四张冻结像素基准卷
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
    // 盐取 0：与本函数长出变体那一层之前**逐位相同**，既有那批地形 PNG
    // 因此一个字节都不用重画。
    fill_speckle(image, rect, spec, 0);
}

/// 主色 + 稀疏点缀这一层，`seed_salt` 混进地块种子。
///
/// 从 [`decorate_terrain_tile`] 抽出来只是为了让变体那一层复用同一套
/// 密度与配色，**不是**为了让点缀密度变成可调参数——三档 bucket 的边界
/// 仍然写死在这里，见 [`decorate_terrain_tile`] 文档里那串数字。
fn fill_speckle(image: &mut RgbaImage, rect: EntryRect, spec: &TerrainSpec, seed_salt: u32) {
    let palette = TerrainPalette::of(spec);

    // 地块种子取自它在图集里的像素坐标：见本函数与 hash_pixel 的文档。
    //
    // 松散贴图那条路径上每张图都是自己一张画布，`rect` 恒是 `(0, 0)`，
    // 因此种子恒为 0——同一批地形贴图的点缀**位置**其实完全一样，只是
    // 颜色不同。这对基准图没问题（本来就是不同地形），但对**同一种地形
    // 的多张变体**是致命的：不掺盐的话三张图的点缀会落在同一批像素上，
    // 差异只剩「颜色略有不同」。`seed_salt` 就是为这件事存在的。
    let tile_seed = ((rect.x << 16) | rect.y) ^ seed_salt;

    for local_y in 0..rect.height {
        for local_x in 0..rect.width {
            let bucket = hash_pixel(tile_seed, local_x, local_y) % 256;
            let (r, g, b) = match bucket {
                0..=4 => palette.analogous_a,
                5..=9 => palette.analogous_b,
                10..=12 => palette.accent,
                _ => spec.base,
            };
            image.put_pixel(rect.x + local_x, rect.y + local_y, Rgba([r, g, b, 255]));
        }
    }
}

/// 一种地形从主色派生出来的四个颜色。抽成一个类型，是为了让点缀那一层
/// 与变体图案那一层**物理上共用同一份换算**——「风格一致」因此是构造上
/// 保证的，不是靠两处各自手挑颜色再祈祷它们像。
struct TerrainPalette {
    /// 色相 +18°、提亮：同色系里浅的那一档。
    analogous_a: (u8, u8, u8),
    /// 色相 -18°、压暗：同色系里深的那一档。
    analogous_b: (u8, u8, u8),
    /// 互补色，按地形单独调过明度/饱和度。
    accent: (u8, u8, u8),
}

impl TerrainPalette {
    fn of(spec: &TerrainSpec) -> Self {
        let base_hsl = Hsl::from_rgb(spec.base.0, spec.base.1, spec.base.2);
        TerrainPalette {
            analogous_a: base_hsl
                .rotated(ANALOGOUS_HUE_SHIFT_DEG)
                .lighten(ANALOGOUS_LIGHTNESS_DELTA)
                .to_rgb(),
            analogous_b: base_hsl
                .rotated(-ANALOGOUS_HUE_SHIFT_DEG)
                .lighten(-ANALOGOUS_LIGHTNESS_DELTA)
                .to_rgb(),
            accent: base_hsl
                .rotated(180.0)
                .lighten(spec.accent_lightness_delta)
                .saturate(spec.accent_saturation_boost)
                .to_rgb(),
        }
    }

    /// 变体图案用的两个颜色：与点缀同色系、同色相偏移，只是明度差拉大
    /// 一档，见 [`MOTIF_LIGHTNESS_DELTA`]。
    fn motif(spec: &TerrainSpec) -> ((u8, u8, u8), (u8, u8, u8)) {
        let base_hsl = Hsl::from_rgb(spec.base.0, spec.base.1, spec.base.2);
        (
            base_hsl
                .rotated(ANALOGOUS_HUE_SHIFT_DEG)
                .lighten(MOTIF_LIGHTNESS_DELTA)
                .to_rgb(),
            base_hsl
                .rotated(-ANALOGOUS_HUE_SHIFT_DEG)
                .lighten(-MOTIF_LIGHTNESS_DELTA)
                .to_rgb(),
        )
    }
}

/// 变体贴图的条目名后缀，与 `ll_game::layout::TERRAIN_VARIANT_SUFFIX`
/// 必须逐字一致（`terrain_grass_alt1`）。
///
/// **这里是那个常量的第二份副本。** 本工具是一个不依赖 `ll-game` 的独立
/// 二进制（`Cargo.toml` 只有 `image`/`serde`/`serde_json`），把引擎那个
/// 常量引过来要付上整个 `ll-game` 依赖树的代价。副本会漂，但漂了当场红：
/// `crates/ll-game/tests/atlas_coverage.rs` 两个方向都锁着——声明侧算出
/// 的键在图集里查不到会红，图集里有声明侧数不出来的变体图也会红。
pub(crate) const VARIANT_SUFFIX: &str = "_alt";

/// 变体图案的明度偏移，比点缀那一层（[`ANALOGOUS_LIGHTNESS_DELTA`]，
/// 0.08）大一档。
///
/// 不是随手加大：点缀是**单像素散点**，靠密度读，明度差小一点反而自然；
/// 图案是**成块**的形状，块内外明度差太小会整团糊进主色，形状读不出来
/// ——那就退回「只换配色」，正是五族那批点名的失效方式。0.14 是能读出
/// 形状的下限档，再大就开始像「打了高光的立体贴图」，与这套平涂像素风
/// 不搭。
const MOTIF_LIGHTNESS_DELTA: f32 = 0.14;

/// 变体图案：8×8 的字符画，`#` 用浅的那一档、`,` 用深的那一档、`.`
/// 保持点缀层原样。
///
/// 写成字符画而不是坐标数组，理由与 `crates/ll-game/tests/visual_baselines.rs`
/// 的 `FLOOR_PLAN` 一样：形状在字符画里一眼能看出来，在数组字面量里
/// 看不出来。
///
/// **图案本身刻意不铺满整格**（8×8 摆在 16×16 里，四边留白）：地形贴图
/// 是要拼在一起的，边缘一动，相邻两格的接缝就会露出图案被切断的痕迹。
type Motif = &'static [&'static str];

/// 变体 1「丛簇」：两团实心块，一大一小，斜着错开。
const CLUMP_MOTIF: Motif = &[
    "..##....", ".#####..", ".#####,.", "..###,..", "...,....", "...##...", "..####..", "...##,..",
];

/// 变体 2「碎屑」：一条斜向细纹，配两处 2×2 的散块。与 [`CLUMP_MOTIF`]
/// 的差别是**结构**（线 vs 团），不只是位置——只挪位置的话缩到 16×16
/// 仍然读成同一张图。
const SCATTER_MOTIF: Motif = &[
    ",...##..", ".,,.##..", "..,,....", "...,,...", "..##,,..", "..##..,,", "......,,", "##.....,",
];

/// 变体号（从 1 起）到图案的映射，外加它在格子里的左上角落点。
///
/// 落点也随变体错开：两个图案即使形状不同，都压在同一处也会让「这一格
/// 中间总有东西」变成新的规则感。
const VARIANT_MOTIFS: [(Motif, u32, u32); 2] = [(CLUMP_MOTIF, 3, 2), (SCATTER_MOTIF, 5, 6)];

/// 按变体号给一块地形贴图填色并点缀。变体 0 就是 [`decorate_terrain_tile`]
/// 本身（**逐位相同**），变体 `>= 1` 在同一份配方上重新播种点缀、再叠一个
/// 几何图案。
///
/// # 为什么必须叠几何图案，不能只重新播种点缀
///
/// 点缀只占每格 5%（13/256 像素）。只换种子的话，两张图的差异上限就是
/// 那 26 个像素——缩到 16×16 的真实显示尺寸，读起来就是同一张图，
/// 「变体」这件事在画面上根本不成立。五族那批的教训是「只换配色会跌破
/// 可分辨门槛」，这里是同一条：**成块的形状差异是那 5% 之外必须另加的
/// 东西**。
///
/// 判据与实测数字见 `crates/ll-game/tests/atlas_coverage.rs` 的
/// `同一种地形的各张变体之间既看得出不同又仍然是同一种地形`，以及本文件
/// 下方 `变体之间的差异过得了可分辨门槛` 那条单测。
pub(crate) fn decorate_terrain_variant(
    image: &mut RgbaImage,
    rect: EntryRect,
    spec: &TerrainSpec,
    variant: u32,
) {
    // 盐用 splitmix 那个黄金比例常数乘一下，只要求「不同变体落到不同
    // 的点缀图案」，不需要任何统计学性质。
    fill_speckle(image, rect, spec, variant.wrapping_mul(0x9E37_79B1));
    if variant == 0 {
        return;
    }
    let (motif, offset_x, offset_y) = VARIANT_MOTIFS[(variant - 1) as usize];
    let (light, dark) = TerrainPalette::motif(spec);
    for (row_index, row) in motif.iter().enumerate() {
        for (col_index, cell) in row.chars().enumerate() {
            let (r, g, b) = match cell {
                '#' => light,
                ',' => dark,
                '.' => continue,
                // 未知字符直接 panic，与 `main.rs` 的 `draw_entry` 同一条
                // 原则：静默跳过会在图上留一个看不见的洞。
                other => panic!("变体图案里有未知字符 {other:?}"),
            };
            let x = rect.x + offset_x + col_index as u32;
            let y = rect.y + offset_y + row_index as u32;
            debug_assert!(
                x < rect.x + rect.width && y < rect.y + rect.height,
                "变体图案越出了这一格的矩形"
            );
            image.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }
}

/// 按条目名派发地形**变体**画法：名字形如 `<基准名>_alt<变体号>` 且基准名
/// 查得到配方时画出来并返回真，否则返回假交给下一支。
///
/// 与 `npc::draw_named`/`composite::draw_named` 同一个形状——`main.rs` 的
/// `draw_entry` 那张大 `match` 不必认识每一个变体名。
///
/// 变体号越界（没有对应图案）时**返回假**而不是画一张退化的图：返回假会
/// 让 `draw_entry` 落到末尾那一支 panic「不知道如何绘制条目」，加变体数
/// 却忘了加图案的人当场看得见；悄悄复用一个已有图案则会画出两张一模一样
/// 的「变体」，而门禁那边只会报「差异不够」，找起来绕一大圈。
pub(crate) fn draw_variant_named(image: &mut RgbaImage, name: &str, rect: EntryRect) -> bool {
    let Some((base, tail)) = name.rsplit_once(VARIANT_SUFFIX) else {
        return false;
    };
    let Ok(variant) = tail.parse::<u32>() else {
        return false;
    };
    if variant == 0 || (variant as usize) > VARIANT_MOTIFS.len() {
        return false;
    }
    let Some(spec) = terrain_spec(base) else {
        return false;
    };
    decorate_terrain_variant(image, rect, spec, variant);
    true
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

    /// 单独画一张 16×16 的变体图，返回它的像素。
    fn render_variant(name: &str, variant: u32) -> Vec<Rgba<u8>> {
        let spec = terrain_spec(name).unwrap_or_else(|| panic!("{name} 应当有配方"));
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        };
        let mut image = RgbaImage::new(16, 16);
        decorate_terrain_variant(&mut image, rect, spec, variant);
        image.pixels().copied().collect()
    }

    #[test]
    fn 变体零与基准逐位相同() {
        // 这条钉的是「既有那批地形 PNG 一个字节都不用重画」这句承诺。
        // 它一旦不成立，`assets/sprites/` 里十九张地形图会集体变动，
        // 两张视觉基准跟着变，而那些差异与「加了变体」这件事毫无关系。
        // Arrange
        let spec = terrain_spec("terrain_grass").expect("有配方");
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        };
        let mut baseline = RgbaImage::new(16, 16);
        let mut variant_zero = RgbaImage::new(16, 16);

        // Act
        decorate_terrain_tile(&mut baseline, rect, spec);
        decorate_terrain_variant(&mut variant_zero, rect, spec, 0);

        // Assert
        assert_eq!(baseline.as_raw(), variant_zero.as_raw());
    }

    #[test]
    fn 变体之间的差异过得了可分辨门槛() {
        // 下界 1/8：256 像素里 32 个。判据与门槛的完整论证见
        // `docs/superpowers/plans/2026-08-31-batch28-terrain-art.md` 四节。
        //
        // 这条与 `crates/ll-game/tests/atlas_coverage.rs` 那条同名判据
        // **不重复**：那边比的是真实资产打包进图集之后的像素（能抓到
        // 「两个清单条目指向同一个 PNG」），这边比的是绘制函数的输出
        // （能在跑 artgen 之前就抓到画法本身写塌了）——与家具那两条
        // 「同名单测 + 图集断言」的分工逐字相同。
        const MIN_DIFFERING: usize = 256 / 8;

        // Arrange & Act
        for (name, count) in [
            ("terrain_grass", 3),
            ("terrain_forest", 3),
            ("terrain_sand", 2),
        ] {
            let rendered: Vec<Vec<Rgba<u8>>> =
                (0..count).map(|v| render_variant(name, v)).collect();

            // Assert
            for (i, a) in rendered.iter().enumerate() {
                for (j, b) in rendered.iter().enumerate().skip(i + 1) {
                    let differing = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
                    assert!(
                        differing >= MIN_DIFFERING,
                        "{name} 的变体 {i} 与变体 {j} 只有 {differing} 个像素不同\
                         （门槛 {MIN_DIFFERING}）——缩到 16×16 会读成同一张图"
                    );
                }
            }
        }
    }

    #[test]
    fn 变体仍然铺满整格且主色仍然占多数() {
        // 上界那一侧：地形是那一格的底层，留透明会露出清屏背景；主色
        // 被图案吃掉太多则会读成另一种地形。
        //
        // 主色占比门槛取 70%：基准图是 95%（13/256 点缀），变体多叠一个
        // 约 25 像素的图案，理论下界约 85%。70% 留足余量，同时拦得住
        // 「图案铺满整格」那种画法。
        // Arrange & Act & Assert
        for (name, count) in [
            ("terrain_grass", 3),
            ("terrain_forest", 3),
            ("terrain_sand", 2),
        ] {
            let spec = terrain_spec(name).expect("有配方");
            let base = Rgba([spec.base.0, spec.base.1, spec.base.2, 255]);
            for variant in 0..count {
                let pixels = render_variant(name, variant);
                assert_eq!(
                    pixels.iter().filter(|p| p.0[3] != 255).count(),
                    0,
                    "{name} 变体 {variant} 有不透明度不足的像素"
                );
                let base_count = pixels.iter().filter(|&&p| p == base).count();
                assert!(
                    base_count * 100 >= pixels.len() * 70,
                    "{name} 变体 {variant} 的主色只剩 {base_count}/{}——读起来已经不是这种地形了",
                    pixels.len()
                );
            }
        }
    }

    #[test]
    fn 变体名派发只认基准名加后缀加十进制变体号() {
        // 反向锁那一侧的机制反例：派发认得太宽，`draw_entry` 就会把不该
        // 当变体的名字画成变体图（而不是 panic 报「不知道如何绘制」）。
        // Arrange
        let rect = EntryRect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        };
        let mut image = RgbaImage::new(16, 16);

        // Act & Assert
        assert!(draw_variant_named(&mut image, "terrain_grass_alt1", rect));
        assert!(draw_variant_named(&mut image, "terrain_grass_alt2", rect));
        // 变体 0 不是一张单独的图（它就是基准条目本身）。
        assert!(!draw_variant_named(&mut image, "terrain_grass_alt0", rect));
        // 没有第 3 个图案：宁可让 `draw_entry` panic，也不悄悄复用一个
        // 已有图案画出两张一模一样的「变体」。
        assert!(!draw_variant_named(&mut image, "terrain_grass_alt3", rect));
        // 基准名查不到配方。
        assert!(!draw_variant_named(&mut image, "terrain_lava_alt1", rect));
        // 后缀后面不是十进制数字。
        assert!(!draw_variant_named(&mut image, "terrain_grass_alt", rect));
        assert!(!draw_variant_named(&mut image, "terrain_grass_altx", rect));
        // 压根没有后缀。
        assert!(!draw_variant_named(&mut image, "terrain_grass", rect));
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
