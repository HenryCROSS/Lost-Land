//! 据点建筑地形的占位贴图：木/石两种材质的墙与地板、门的开与关、
//! 窗，以及上下两个方向的楼梯。
//!
//! # 这九张图为什么现在才存在
//!
//! `ll_world::terrain` 的 `define_base` 注册了 17 种本体地形，
//! `ll_game::layout::terrain_entry_name` 此前只认得其中 10 种——8 种
//! 自然地形各有一张图，`floor_stone`/`wall_stone` 借用
//! `terrain_dirt`/`terrain_mountain`。剩下 7 种（`floor_wood`/
//! `wall_wood`/`door_closed`/`door_open`/`window`/`stairs_up`/
//! `stairs_down`）在那张表里查不到，落到
//! `ll_game::layout::terrain_atlas_key` 的 Registry 回退路径上，拿注册
//! ID（`lostland:wall_wood` 这类）当图集键去查，图集里同样没有——于是
//! 每帧每格刷一条「图集条目缺失，跳过本次绘制」的 ERROR，据点、建筑、
//! 室内在屏幕上**一格都画不出来**。本模块补的是这九张图。
//!
//! 「九张」而不是「七张」：`floor_stone`/`wall_stone` 原本借用的那两张
//! 图有各自的本职（`terrain_dirt` 是泥土、`terrain_mountain` 是山），
//! 木地板一旦画成暖褐色木纹，就会和暖褐色的泥土糊在一起——项目所有者
//! 的验收方式是「走进据点看一眼」，木/石地板必须一眼可分。因此这两种
//! 石质建筑地形也各自拿到专属贴图，借用关系一并解除。这是本批次的
//! 判断，不是所有者原话。
//!
//! # 铺满整格，不留透明
//!
//! 与 `world_marks.rs` 那四张记号相反：那些画在地形**之上**
//! （`Layer::DECOR`/`Layer::ENTITY`），四周留透明让地形透出来；本模块
//! 九张全是**地形本身**（`Layer::TERRAIN`），是那一格的底层，必须像
//! 既有的 `terrain_*.png` 一样铺满 16×16 全部像素。留透明会让那一格
//! 露出清屏背景，读成「这里什么都没有」。
//!
//! # 配色的两条规则
//!
//! 唯一的硬要求仍是「能看出是什么、能和别的东西区分开」（项目所有者：
//! 「你先画点东西。以后我再精细化处理。」）。在这个前提下取值只守两条
//! 规则，其余不追求画风：
//!
//! 1. **材质分色相**：木质走暖褐（色相 ~28°），石质走中性冷灰
//!    （饱和度接近 0）。同一格里分不清「这是木头还是石头」时，看色相。
//! 2. **墙暗地板亮**：同材质下墙的明度明显低于地板（木墙 vs 木地板、
//!    石墙 vs 石地板都是这一条）。同一材质里分不清「这是墙还是地板」
//!    时，看明暗。两条规则正交，四种组合各占一个象限，因此
//!    木墙/石墙/木地板/石地板两两之间至少差一个维度。
//!
//! 门/窗/楼梯不参与这套两维分类：它们各自有一眼可辨的图形特征（门有
//! 整块门板与把手、窗有亮玻璃与十字窗棂、楼梯有明暗渐变的阶梯条带），
//! 靠形状而非颜色区分。

use crate::EntryRect;
use crate::sprite::{fill_rect, paint_patch};
use image::RgbaImage;

/// 木地板主色（暖褐、偏亮）——见模块文档「墙暗地板亮」。
const WOOD_FLOOR_BASE: (u8, u8, u8) = (152, 110, 66);
/// 木地板的板缝色。
const WOOD_FLOOR_SEAM: (u8, u8, u8) = (104, 72, 40);
/// 木墙主色（暖褐、明显更暗）。
const WOOD_WALL_BASE: (u8, u8, u8) = (104, 68, 38);
/// 木墙的板缝色。
const WOOD_WALL_SEAM: (u8, u8, u8) = (62, 40, 22);
/// 木墙顶端受光的横梁色——让墙读成「立着的一面」而不是「一块深色地板」。
const WOOD_WALL_BEAM: (u8, u8, u8) = (150, 108, 64);

/// 石地板主色（中性灰、偏亮）。刻意比 `terrain_mountain` 的
/// `(128, 128, 132)` 更亮：山体是自然地形，石地板是人工铺面，两者会在
/// 同一屏里出现（据点建在山脚下），亮度差是它们唯一的区分。
const STONE_FLOOR_BASE: (u8, u8, u8) = (166, 166, 172);
/// 石地板的灰浆缝色。
const STONE_FLOOR_MORTAR: (u8, u8, u8) = (122, 122, 130);
/// 石墙主色（中性灰、明显更暗）。
const STONE_WALL_BASE: (u8, u8, u8) = (100, 100, 108);
/// 石墙的灰浆缝色。
const STONE_WALL_MORTAR: (u8, u8, u8) = (62, 62, 70);

/// 门/窗四周门框的暗色——木质的深褐，比木墙还暗，让框在墙里也能看出
/// 轮廓。
const FRAME_DARK: (u8, u8, u8) = (58, 38, 20);
/// 关着的门那块门板的颜色（比木墙亮、比木地板更饱和的橙褐）——门是
/// 「墙上那块和墙不一样的东西」，不能跟墙同色。
const DOOR_LEAF: (u8, u8, u8) = (176, 122, 56);
/// 门把手/合页的金属色。
const DOOR_HANDLE: (u8, u8, u8) = (226, 200, 120);
/// 门开着时那个门洞的颜色（近黑的暖灰）——不是透明：地形层必须铺满，
/// 见模块文档。
const DOORWAY_VOID: (u8, u8, u8) = (46, 38, 32);

/// 窗玻璃色（浅青）。与 `terrain_shallow_water` 的 `(86, 172, 214)`
/// 同处冷色一侧，但窗恒被 [`FRAME_DARK`] 的深框与十字窗棂切开，浅水
/// 是一整格连续的蓝——形状而非颜色是它们的区分。
const GLASS: (u8, u8, u8) = (168, 214, 228);
/// 窗棂色（与门框同一档暗色，木窗棂）。
const MULLION: (u8, u8, u8) = FRAME_DARK;

/// 楼梯踏面的最亮色。
const STAIR_LIGHT: (u8, u8, u8) = (198, 196, 190);
/// 楼梯踢面的最暗色。
const STAIR_DARK: (u8, u8, u8) = (58, 56, 54);
/// 上行楼梯的方向标记色（暖黄）——与下行的冷色标记拉开色相。
const STAIR_UP_MARK: (u8, u8, u8) = (240, 208, 96);
/// 下行楼梯的方向标记色（冷蓝）。
const STAIR_DOWN_MARK: (u8, u8, u8) = (96, 148, 220);

/// 木板的板宽（像素）：16 除得尽 4，一格恰好四块板。
const PLANK_SPAN: u32 = 4;

/// 砌块的宽与高（像素）：8×4，一格两列四行，错缝后是标准的顺砌。
const MASONRY_BLOCK_WIDTH: u32 = 8;
const MASONRY_BLOCK_HEIGHT: u32 = 4;

/// 门框/窗框的边厚（像素）。
const FRAME_THICKNESS: u32 = 2;

/// 楼梯的阶数：16 除得尽 4，一格四阶，每阶 4 像素高。
const STAIR_STEPS: u32 = 4;

/// 板材的铺设方向。
///
/// 只有这一个枚举，因为「板缝画在哪个方向」是木地板与木墙**唯一**的
/// 结构差异——地板的板横铺（人走在上面，视线俯视板面），墙的板竖立
/// （承重的立柱方向）。两者共用 [`paint_planks`] 一份算法，不是两份
/// 抄来抄去的坐标算术。
#[derive(Clone, Copy)]
enum PlankDirection {
    /// 板缝是横线，板横向铺开——地板。
    Horizontal,
    /// 板缝是竖线，板竖向立起——墙。
    Vertical,
}

/// 铺满一格木板：先填主色，再按 [`PLANK_SPAN`] 每隔一块画一道板缝。
///
/// 板缝画在每块板的**起始边**上（`local % PLANK_SPAN == 0`），因此一格
/// 里恒有 `16 / PLANK_SPAN` 道缝，最左/最上那道压在格子边缘上——相邻
/// 两格拼起来时，两道边缘缝并排成一道 2 像素宽的粗缝，读成「这里是两
/// 块地板的接缝」，正是想要的效果。
fn paint_planks(
    image: &mut RgbaImage,
    rect: EntryRect,
    direction: PlankDirection,
    base: (u8, u8, u8),
    seam: (u8, u8, u8),
) {
    fill_rect(image, rect, base);
    match direction {
        PlankDirection::Horizontal => {
            let mut dy = 0;
            while dy < rect.height {
                paint_patch(image, rect, 0, dy, rect.width, 1, seam);
                dy += PLANK_SPAN;
            }
        }
        PlankDirection::Vertical => {
            let mut dx = 0;
            while dx < rect.width {
                paint_patch(image, rect, dx, 0, 1, rect.height, seam);
                dx += PLANK_SPAN;
            }
        }
    }
}

/// 铺满一格错缝砌体（顺砌）：先填灰浆色当底，再把砌块一块块盖上去，
/// 奇数行整体右移半块。
///
/// 底色取灰浆而不是砌块色，是为了让「缝」自然成为砌块之间没被盖住的
/// 那一像素，不需要第二遍画线——石地板与石墙因此共用同一份算法，只是
/// 传进来的两个颜色不同。
fn paint_masonry(image: &mut RgbaImage, rect: EntryRect, base: (u8, u8, u8), mortar: (u8, u8, u8)) {
    fill_rect(image, rect, mortar);
    let mut row = 0;
    let mut dy = 0;
    while dy < rect.height {
        // 奇数行右移半块 → 错缝。整块砌体的左端因此会露出半块，与
        // 相邻格拼起来时刚好接上另外半块。
        let shift = if row % 2 == 1 {
            MASONRY_BLOCK_WIDTH / 2
        } else {
            0
        };
        let mut dx = 0;
        while dx < rect.width {
            // 砌块比标称尺寸各少一像素，让出灰浆缝；起点带 shift 之后
            // 可能越过右边界，按剩余宽度截断。
            let start = dx + shift;
            if start >= rect.width {
                break;
            }
            let width = (MASONRY_BLOCK_WIDTH - 1).min(rect.width - start);
            let height = (MASONRY_BLOCK_HEIGHT - 1).min(rect.height - dy);
            paint_patch(image, rect, start, dy, width, height, base);
            dx += MASONRY_BLOCK_WIDTH;
        }
        // 错缝行左端露出的那半块：单独补一块，否则每隔一行左边会缺角。
        if shift > 0 {
            let width = shift - 1;
            let height = (MASONRY_BLOCK_HEIGHT - 1).min(rect.height - dy);
            paint_patch(image, rect, 0, dy, width, height, base);
        }
        row += 1;
        dy += MASONRY_BLOCK_HEIGHT;
    }
}

/// 沿一格四边画一圈厚 [`FRAME_THICKNESS`] 的框。
///
/// 门（开/关）与窗三张图共用：三者都是「墙上开的一个洞 + 洞里装的
/// 东西」，框就是洞的边。抽出来的是这一段四边坐标算术，不是为了对称
/// ——三处若各写一遍，改厚度要改三处、且极易在某一处把某条边写错一
/// 像素而不被发现。
fn paint_frame(image: &mut RgbaImage, rect: EntryRect, color: (u8, u8, u8)) {
    let t = FRAME_THICKNESS;
    paint_patch(image, rect, 0, 0, rect.width, t, color);
    paint_patch(image, rect, 0, rect.height - t, rect.width, t, color);
    paint_patch(image, rect, 0, 0, t, rect.height, color);
    paint_patch(image, rect, rect.width - t, 0, t, rect.height, color);
}

/// 画 `terrain_floor_wood`：暖褐木地板，横向板缝。
pub(crate) fn decorate_floor_wood(image: &mut RgbaImage, rect: EntryRect) {
    paint_planks(
        image,
        rect,
        PlankDirection::Horizontal,
        WOOD_FLOOR_BASE,
        WOOD_FLOOR_SEAM,
    );
}

/// 画 `terrain_wall_wood`：暗一档的暖褐木墙，竖向板缝 + 顶端受光横梁。
///
/// 横梁是木墙独有的一笔（木地板没有）：只靠明暗差区分墙与地板，在
/// 昼夜/迷雾的 tint 乘上去之后可能被压平，多一条结构线让区分不只
/// 依赖亮度。
pub(crate) fn decorate_wall_wood(image: &mut RgbaImage, rect: EntryRect) {
    paint_planks(
        image,
        rect,
        PlankDirection::Vertical,
        WOOD_WALL_BASE,
        WOOD_WALL_SEAM,
    );
    paint_patch(image, rect, 0, 0, rect.width, 2, WOOD_WALL_BEAM);
    paint_patch(
        image,
        rect,
        0,
        rect.height - 2,
        rect.width,
        2,
        WOOD_WALL_SEAM,
    );
}

/// 画 `terrain_floor_stone`：亮一档的中性灰铺面石。
pub(crate) fn decorate_floor_stone(image: &mut RgbaImage, rect: EntryRect) {
    paint_masonry(image, rect, STONE_FLOOR_BASE, STONE_FLOOR_MORTAR);
}

/// 画 `terrain_wall_stone`：暗一档的中性灰砌石墙，理由同
/// [`decorate_wall_wood`] 的横梁——顶端加一条亮沿当墙帽。
pub(crate) fn decorate_wall_stone(image: &mut RgbaImage, rect: EntryRect) {
    paint_masonry(image, rect, STONE_WALL_BASE, STONE_WALL_MORTAR);
    paint_patch(image, rect, 0, 0, rect.width, 2, STONE_FLOOR_BASE);
}

/// 画 `terrain_door_closed`：深框 + 铺满门洞的整块门板 + 右侧把手。
///
/// 「废墟没有门」这条既有验收判据要看得见，前提就是门本身画得出来
/// 且与墙可分：门板色 [`DOOR_LEAF`] 比木墙亮且更饱和，把手是整格里
/// 唯一的金属亮点。
pub(crate) fn decorate_door_closed(image: &mut RgbaImage, rect: EntryRect) {
    fill_rect(image, rect, DOOR_LEAF);
    paint_frame(image, rect, FRAME_DARK);
    // 门板中缝：两扇对开门的暗示，也让门板不至于是一整块死板的纯色。
    paint_patch(
        image,
        rect,
        rect.width / 2,
        2,
        1,
        rect.height - 4,
        FRAME_DARK,
    );
    // 把手：中缝两侧各一点，位置在门的高度中线。
    paint_patch(image, rect, rect.width / 2 - 3, 7, 2, 2, DOOR_HANDLE);
    paint_patch(image, rect, rect.width / 2 + 2, 7, 2, 2, DOOR_HANDLE);
}

/// 画 `terrain_door_open`：同一副深框，但门板收到两侧、中间露出门洞。
///
/// 与 [`decorate_door_closed`] 的差异是**整格大部分像素**（中间 10 列
/// 从亮门板变成近黑门洞），不是一两笔细节——所有者的验收方式是「走进
/// 据点看一眼」，开与关必须在一眼之内分得开。
pub(crate) fn decorate_door_open(image: &mut RgbaImage, rect: EntryRect) {
    fill_rect(image, rect, DOORWAY_VOID);
    paint_frame(image, rect, FRAME_DARK);
    // 收到两侧的门板：各 3 像素宽，贴着框内侧立着。
    paint_patch(image, rect, 2, 2, 3, rect.height - 4, DOOR_LEAF);
    paint_patch(
        image,
        rect,
        rect.width - 5,
        2,
        3,
        rect.height - 4,
        DOOR_LEAF,
    );
    // 两扇门板各自的把手，与关着时同一高度——同一扇门转过去了，不是
    // 换了一件东西。
    paint_patch(image, rect, 3, 7, 1, 2, DOOR_HANDLE);
    paint_patch(image, rect, rect.width - 4, 7, 1, 2, DOOR_HANDLE);
}

/// 画 `terrain_window`：深框 + 亮玻璃 + 十字窗棂。
///
/// 窗在规则上「不可通行但不阻挡视线」（见 `ll_world::terrain` 的
/// `define_base`），画成亮玻璃正对应「看得穿」这半条；深框与十字窗棂
/// 对应「过不去」那半条。
pub(crate) fn decorate_window(image: &mut RgbaImage, rect: EntryRect) {
    fill_rect(image, rect, WOOD_WALL_BASE);
    paint_patch(
        image,
        rect,
        FRAME_THICKNESS,
        FRAME_THICKNESS,
        rect.width - 2 * FRAME_THICKNESS,
        rect.height - 2 * FRAME_THICKNESS,
        GLASS,
    );
    paint_frame(image, rect, FRAME_DARK);
    // 十字窗棂：竖棂 + 横棂，各 2 像素宽，交叉居中。
    paint_patch(image, rect, rect.width / 2 - 1, 0, 2, rect.height, MULLION);
    paint_patch(image, rect, 0, rect.height / 2 - 1, rect.width, 2, MULLION);
}

/// 楼梯的走向。
#[derive(Clone, Copy)]
enum StairDirection {
    /// 越靠上越亮——踏面朝向观察者，读成「往上走」。
    Up,
    /// 越靠上越暗——阶梯没入地下，读成「往下走」。
    Down,
}

/// 铺满一格阶梯条带：[`STAIR_STEPS`] 条等高横带，明度沿走向单调变化，
/// 每条带底部压一道暗踢面线。
///
/// 上下两张图共用这一份算法，只有 [`StairDirection`] 一个参数不同——
/// 「明度从亮到暗」与「从暗到亮」是同一段插值的两个方向，抄两遍只会
/// 让某一遍的边界条件写错。
fn paint_step_bands(image: &mut RgbaImage, rect: EntryRect, direction: StairDirection) {
    let band_height = rect.height / STAIR_STEPS;
    for step in 0..STAIR_STEPS {
        // 顶端 step = 0。Up 时顶端最亮，Down 时顶端最暗。
        let brightness_index = match direction {
            StairDirection::Up => STAIR_STEPS - 1 - step,
            StairDirection::Down => step,
        };
        let t = brightness_index;
        let span = STAIR_STEPS - 1;
        let shade = (
            lerp_channel(STAIR_DARK.0, STAIR_LIGHT.0, t, span),
            lerp_channel(STAIR_DARK.1, STAIR_LIGHT.1, t, span),
            lerp_channel(STAIR_DARK.2, STAIR_LIGHT.2, t, span),
        );
        paint_patch(
            image,
            rect,
            0,
            step * band_height,
            rect.width,
            band_height,
            shade,
        );
        // 踢面：每条踏面下缘的一道暗线，让四条带读成「四级台阶」而不是
        // 「一块渐变色板」。
        paint_patch(
            image,
            rect,
            0,
            step * band_height + band_height - 1,
            rect.width,
            1,
            STAIR_DARK,
        );
    }
}

/// 通道整数插值：`from` 到 `to` 之间取第 `t`/`span` 档。
///
/// 定点整数运算，不用浮点——世界状态不允许浮点是 `ll-world` 的纪律，
/// 这里虽然只是生成美术，也没有理由引入浮点带来的舍入不确定性：同一
/// 份源码在任何平台上都该生成逐像素相同的 PNG。
fn lerp_channel(from: u8, to: u8, t: u32, span: u32) -> u8 {
    if span == 0 {
        return to;
    }
    let from = i32::from(from);
    let to = i32::from(to);
    let value = from + (to - from) * t as i32 / span as i32;
    value.clamp(0, 255) as u8
}

/// 在阶梯条带上压一个指向明确的三角箭头。
///
/// `pointing_up` 决定三角的朝向。抽出来的理由与 [`paint_step_bands`]
/// 同：上下两张图的箭头是同一段逐行收窄的算术，方向相反。
fn paint_direction_arrow(
    image: &mut RgbaImage,
    rect: EntryRect,
    pointing_up: bool,
    color: (u8, u8, u8),
) {
    // 五行高的实心三角，横向居中；顶行 1 像素宽，逐行加宽 2 像素。
    const ARROW_ROWS: u32 = 5;
    let top = (rect.height - ARROW_ROWS) / 2;
    for row in 0..ARROW_ROWS {
        let widening = if pointing_up {
            row
        } else {
            ARROW_ROWS - 1 - row
        };
        let width = 1 + widening * 2;
        let dx = rect.width / 2 - widening;
        paint_patch(image, rect, dx, top + row, width, 1, color);
    }
}

/// 画 `terrain_stairs_up`：越往上越亮的四级阶梯 + 暖黄上行箭头。
pub(crate) fn decorate_stairs_up(image: &mut RgbaImage, rect: EntryRect) {
    paint_step_bands(image, rect, StairDirection::Up);
    paint_direction_arrow(image, rect, true, STAIR_UP_MARK);
}

/// 画 `terrain_stairs_down`：越往上越暗的四级阶梯 + 冷蓝下行箭头。
pub(crate) fn decorate_stairs_down(image: &mut RgbaImage, rect: EntryRect) {
    paint_step_bands(image, rect, StairDirection::Down);
    paint_direction_arrow(image, rect, false, STAIR_DOWN_MARK);
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: EntryRect = EntryRect {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    };

    fn tile_of(draw: DrawFn) -> RgbaImage {
        let mut image = RgbaImage::new(TILE.width, TILE.height);
        draw(&mut image, TILE);
        image
    }

    /// 一张画法函数：接一张画布与一个矩形，把图画进去。与 `main.rs`
    /// 的分派表里那一列函数同型。
    type DrawFn = fn(&mut RgbaImage, EntryRect);

    /// 本模块九张图的画法函数，与 `main.rs` 的分派表一一对应。
    const ALL_DRAWS: &[(&str, DrawFn)] = &[
        ("terrain_floor_wood", decorate_floor_wood),
        ("terrain_floor_stone", decorate_floor_stone),
        ("terrain_wall_wood", decorate_wall_wood),
        ("terrain_wall_stone", decorate_wall_stone),
        ("terrain_door_closed", decorate_door_closed),
        ("terrain_door_open", decorate_door_open),
        ("terrain_window", decorate_window),
        ("terrain_stairs_up", decorate_stairs_up),
        ("terrain_stairs_down", decorate_stairs_down),
    ];

    #[test]
    fn 九张建筑地形都铺满整格不留透明像素() {
        // 地形层是那一格的底层，留透明会露出清屏背景，见模块文档。
        for &(name, draw) in ALL_DRAWS {
            // Arrange & Act
            let image = tile_of(draw);

            // Assert
            let transparent = image.pixels().filter(|p| p.0[3] != 255).count();
            assert_eq!(transparent, 0, "{name} 有 {transparent} 个非不透明像素");
        }
    }

    #[test]
    fn 九张建筑地形两两之间至少四分之一像素不同() {
        // 「能和别的东西区分开」这条硬要求的数字化：16×16 = 256 像素，
        // 门槛取 64（25%）。这不是「看起来好不好」的判据，是「两张图
        // 有没有被写成几乎一样」的下界。
        const MIN_DIFFERING_PIXELS: usize = 64;
        for (i, &(name_a, draw_a)) in ALL_DRAWS.iter().enumerate() {
            for &(name_b, draw_b) in &ALL_DRAWS[i + 1..] {
                // Arrange
                let a = tile_of(draw_a);
                let b = tile_of(draw_b);

                // Act
                let differing = a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count();

                // Assert
                assert!(
                    differing >= MIN_DIFFERING_PIXELS,
                    "{name_a} 与 {name_b} 只有 {differing} 个像素不同，低于门槛 {MIN_DIFFERING_PIXELS}"
                );
            }
        }
    }

    #[test]
    fn 同材质下墙比地板暗() {
        // 模块文档「墙暗地板亮」那条规则的可执行版本。用绿通道代表明度
        // ——木质与石质的三个通道在本模块的取值里同向变化，取任一通道
        // 的结论相同，取绿是因为它在人眼亮度感知里权重最高。
        let pairs = [
            (
                "木",
                tile_of(decorate_wall_wood),
                tile_of(decorate_floor_wood),
            ),
            (
                "石",
                tile_of(decorate_wall_stone),
                tile_of(decorate_floor_stone),
            ),
        ];
        for (material, wall, floor) in pairs {
            // Act
            let wall_mean: u32 =
                wall.pixels().map(|p| u32::from(p.0[1])).sum::<u32>() / wall.pixels().len() as u32;
            let floor_mean: u32 = floor.pixels().map(|p| u32::from(p.0[1])).sum::<u32>()
                / floor.pixels().len() as u32;

            // Assert
            assert!(
                wall_mean + 20 < floor_mean,
                "{material}墙平均明度 {wall_mean} 未明显低于{material}地板 {floor_mean}"
            );
        }
    }

    #[test]
    fn 上下楼梯的方向标记颜色不同且各自成片() {
        // 上下楼梯的阶梯条带互为镜像，若两个箭头也同色，玩家只能靠
        // 「哪头亮」去猜方向。这条断言钉的是「方向标记本身就分得开」。
        // Arrange
        let up = tile_of(decorate_stairs_up);
        let down = tile_of(decorate_stairs_down);

        // Act
        let up_mark = up
            .pixels()
            .filter(|p| (p.0[0], p.0[1], p.0[2]) == STAIR_UP_MARK)
            .count();
        let down_mark = down
            .pixels()
            .filter(|p| (p.0[0], p.0[1], p.0[2]) == STAIR_DOWN_MARK)
            .count();

        // Assert：五行三角实心面积 1+3+5+7+9 = 25 像素。
        assert_eq!(up_mark, 25, "上行箭头面积不对");
        assert_eq!(down_mark, 25, "下行箭头面积不对");
        assert_ne!(STAIR_UP_MARK, STAIR_DOWN_MARK);
    }

    #[test]
    fn 门开与关的差异集中在中间那片门洞() {
        // Arrange
        let closed = tile_of(decorate_door_closed);
        let open = tile_of(decorate_door_open);

        // Act：只数门框以内、两扇门板之间那 6 列（x 5..11）。
        let mut differing = 0;
        for y in 2..14 {
            for x in 5..11 {
                if closed.get_pixel(x, y) != open.get_pixel(x, y) {
                    differing += 1;
                }
            }
        }

        // Assert：那一片 6×12 = 72 像素里，绝大多数应当不同（关着是门板
        // 与中缝，开着是门洞）。
        assert!(differing >= 60, "门洞区域只有 {differing} 个像素不同");
    }

    #[test]
    fn 错缝砌体的奇数行左端不缺角() {
        // paint_masonry 里那句「错缝行左端露出的半块单独补一块」的
        // 反例守卫：删掉它这条会红。
        // Arrange & Act
        let stone = tile_of(decorate_floor_stone);

        // Assert：第二行（y = 4..7）的最左一列应当是砌块色而非灰浆色。
        let pixel = stone.get_pixel(0, 5);
        assert_eq!(
            (pixel.0[0], pixel.0[1], pixel.0[2]),
            STONE_FLOOR_BASE,
            "错缝行左端露出了灰浆底色，说明半块没有补上"
        );
    }
}
